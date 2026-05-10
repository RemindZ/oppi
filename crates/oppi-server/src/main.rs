use oppi_core::{
    Runtime, TurnInterruptRegistry,
    event_store::{EventStore, FilesystemEventStore, StoreNamespace},
};
use oppi_protocol::*;
use oppi_sandbox::{
    SandboxManager, current_sandbox_status, execute_sandboxed,
    install_windows_wfp_filters_for_account, windows_wfp_status,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io::{self, BufRead, Write};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    #[allow(dead_code)]
    jsonrpc: Option<String>,
    id: Value,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    jsonrpc: &'static str,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
struct JsonRpcError {
    code: i64,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct WindowsWfpInstallParams {
    account: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StdioAction {
    Continue,
    Shutdown,
}

const SERVER_EVENTS_LIST_DEFAULT_LIMIT: usize = 1_000;
const SERVER_EVENTS_LIST_MAX_LIMIT: usize = 5_000;
const BACKGROUND_TURN_START_TIMEOUT: Duration = Duration::from_secs(2);
const BACKGROUND_TURN_INTERRUPT_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone)]
struct ServerState {
    runtime: Arc<Mutex<Runtime>>,
    event_mirror: Arc<Mutex<Vec<Event>>>,
    turn_interrupts: TurnInterruptRegistry,
    persistent_store: Option<Arc<Mutex<FilesystemEventStore>>>,
    persisted_event_id: Arc<Mutex<EventId>>,
}

impl ServerState {
    fn new() -> Self {
        Self::from_env().unwrap_or_else(|error| {
            eprintln!("oppi-server persistence disabled: {error}");
            Self::without_persistence()
        })
    }

    fn without_persistence() -> Self {
        let event_mirror = Arc::new(Mutex::new(Vec::new()));
        let mut runtime = Runtime::new();
        runtime.set_event_mirror(Some(event_mirror.clone()));
        let turn_interrupts = runtime.turn_interrupt_registry();
        Self {
            runtime: Arc::new(Mutex::new(runtime)),
            event_mirror,
            turn_interrupts,
            persistent_store: None,
            persisted_event_id: Arc::new(Mutex::new(0)),
        }
    }

    fn from_env() -> Result<Self, String> {
        let Some(root) = runtime_store_root_from_env() else {
            return Ok(Self::without_persistence());
        };
        let namespace = StoreNamespace::new(
            std::env::var("OPPI_RUNTIME_STORE_PROJECT_ID")
                .ok()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| "oppi-shell".to_string()),
            std::env::var("OPPI_RUNTIME_STORE_ID")
                .ok()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| "default".to_string()),
        );
        Self::with_persistence(root, namespace)
    }

    fn with_persistence(root: PathBuf, namespace: StoreNamespace) -> Result<Self, String> {
        let store = FilesystemEventStore::with_namespace(root, namespace)
            .map_err(|error| format!("open event store: {error:?}"))?;
        let events = store
            .list_all_events()
            .map_err(|error| format!("load persisted events: {error:?}"))?;
        let mut runtime = if events.is_empty() {
            Runtime::new()
        } else {
            Runtime::replay_events(&events).map_err(|error| error.message)?
        };
        runtime.reserve_thread_counter(
            store
                .max_thread_counter_hint()
                .map_err(|error| format!("scan persisted thread ids: {error:?}"))?,
        );
        let event_mirror = Arc::new(Mutex::new(events.clone()));
        runtime.set_event_mirror(Some(event_mirror.clone()));
        let recover = runtime.recover_incomplete_turns("server restarted before turn completed");
        let turn_interrupts = runtime.turn_interrupt_registry();
        let last_loaded = events.iter().map(|event| event.id).max().unwrap_or(0);
        let state = Self {
            runtime: Arc::new(Mutex::new(runtime)),
            event_mirror,
            turn_interrupts,
            persistent_store: Some(Arc::new(Mutex::new(store))),
            persisted_event_id: Arc::new(Mutex::new(last_loaded)),
        };
        if !recover.events.is_empty() {
            state.persist_new_events().map_err(|error| error.message)?;
        }
        Ok(state)
    }

    fn events_after_mirror(
        &self,
        params: EventsListParams,
    ) -> Result<EventsListResult, JsonRpcError> {
        let limit = params
            .limit
            .unwrap_or(SERVER_EVENTS_LIST_DEFAULT_LIMIT)
            .min(SERVER_EVENTS_LIST_MAX_LIMIT);
        let events = self
            .event_mirror
            .lock()
            .map_err(|_| json_rpc_internal("event mirror lock poisoned"))?
            .iter()
            .filter(|event| event.thread_id == params.thread_id && event.id > params.after)
            .take(limit)
            .cloned()
            .collect();
        Ok(EventsListResult { events })
    }

    fn run_agentic_background(
        &self,
        mut params: AgenticTurnParams,
    ) -> Result<AgenticTurnResult, JsonRpcError> {
        let thread_id = params.thread_id.clone();
        let after = self.last_mirrored_event_id()?;
        params.execution_mode = ExecutionMode::Blocking;
        let runtime = self.runtime.clone();
        let result_slot: Arc<Mutex<Option<Result<AgenticTurnResult, RuntimeError>>>> =
            Arc::new(Mutex::new(None));
        let result_slot_for_thread = result_slot.clone();
        thread::spawn(move || {
            let result = match runtime.lock() {
                Ok(mut runtime) => runtime.run_agentic_turn(params),
                Err(_) => Err(RuntimeError::new(
                    "runtime_lock_poisoned",
                    RuntimeErrorCategory::EventStore,
                    "runtime lock poisoned while running background turn",
                )),
            };
            if let Ok(mut slot) = result_slot_for_thread.lock() {
                *slot = Some(result);
            }
        });

        let started = self.wait_for_background_turn_start(&thread_id, after, &result_slot)?;
        let events = self
            .events_after_mirror(EventsListParams {
                thread_id,
                after,
                limit: None,
            })?
            .events;
        Ok(agentic_result_from_background_events(started, events))
    }

    fn request_background_interrupt(
        &self,
        params: TurnInterruptParams,
    ) -> Result<EventsListResult, JsonRpcError> {
        let after = self.last_mirrored_event_id()?;
        self.turn_interrupts
            .request(params.turn_id.clone(), params.reason)
            .map_err(json_rpc_internal)?;
        let started = Instant::now();
        loop {
            let result = self.events_after_mirror(EventsListParams {
                thread_id: params.thread_id.clone(),
                after,
                limit: None,
            })?;
            let observed = result.events.iter().any(|event| {
                event.turn_id.as_deref() == Some(params.turn_id.as_str())
                    && matches!(
                        event.kind,
                        EventKind::TurnInterrupted { .. } | EventKind::TurnAborted { .. }
                    )
            });
            if observed || started.elapsed() >= BACKGROUND_TURN_INTERRUPT_TIMEOUT {
                return Ok(result);
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn wait_for_background_turn_start(
        &self,
        thread_id: &str,
        after: EventId,
        result_slot: &Arc<Mutex<Option<Result<AgenticTurnResult, RuntimeError>>>>,
    ) -> Result<Turn, JsonRpcError> {
        let started = Instant::now();
        loop {
            if let Some(turn) = self.find_turn_started_after(thread_id, after)? {
                return Ok(turn);
            }
            if let Some(result) = result_slot
                .lock()
                .map_err(|_| json_rpc_internal("background result lock poisoned"))?
                .as_ref()
            {
                match result {
                    Ok(result) => return Ok(result.turn.clone()),
                    Err(error) => return Err(runtime_error(error.clone())),
                }
            }
            if started.elapsed() >= BACKGROUND_TURN_START_TIMEOUT {
                return Err(JsonRpcError {
                    code: -32000,
                    message: "background turn did not start before timeout".to_string(),
                    data: Some(json!({
                        "code": "background_turn_start_timeout",
                        "category": "terminal_state"
                    })),
                });
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn find_turn_started_after(
        &self,
        thread_id: &str,
        after: EventId,
    ) -> Result<Option<Turn>, JsonRpcError> {
        Ok(self
            .event_mirror
            .lock()
            .map_err(|_| json_rpc_internal("event mirror lock poisoned"))?
            .iter()
            .filter(|event| event.thread_id == thread_id && event.id > after)
            .find_map(|event| match &event.kind {
                EventKind::TurnStarted { turn } => Some(turn.clone()),
                _ => None,
            }))
    }

    fn last_mirrored_event_id(&self) -> Result<EventId, JsonRpcError> {
        Ok(self
            .event_mirror
            .lock()
            .map_err(|_| json_rpc_internal("event mirror lock poisoned"))?
            .last()
            .map(|event| event.id)
            .unwrap_or(0))
    }

    fn persist_new_events(&self) -> Result<(), RuntimeError> {
        let Some(store) = &self.persistent_store else {
            return Ok(());
        };
        let last_persisted = *self.persisted_event_id.lock().map_err(|_| {
            RuntimeError::new(
                "persistence_lock_poisoned",
                RuntimeErrorCategory::EventStore,
                "persistence event cursor lock poisoned",
            )
        })?;
        let events = self
            .event_mirror
            .lock()
            .map_err(|_| {
                RuntimeError::new(
                    "event_mirror_lock_poisoned",
                    RuntimeErrorCategory::EventStore,
                    "event mirror lock poisoned while persisting events",
                )
            })?
            .iter()
            .filter(|event| event.id > last_persisted)
            .cloned()
            .collect::<Vec<_>>();
        if events.is_empty() {
            return Ok(());
        }
        let mut max_id = last_persisted;
        let mut store = store.lock().map_err(|_| {
            RuntimeError::new(
                "persistence_store_lock_poisoned",
                RuntimeErrorCategory::EventStore,
                "persistent event store lock poisoned",
            )
        })?;
        for event in events {
            max_id = max_id.max(event.id);
            store.append(event).map_err(|error| {
                RuntimeError::new(
                    "persistent_event_append_failed",
                    RuntimeErrorCategory::EventStore,
                    format!("persist event failed: {error:?}"),
                )
            })?;
        }
        *self.persisted_event_id.lock().map_err(|_| {
            RuntimeError::new(
                "persistence_lock_poisoned",
                RuntimeErrorCategory::EventStore,
                "persistence event cursor lock poisoned",
            )
        })? = max_id;
        Ok(())
    }
}

fn runtime_store_root_from_env() -> Option<PathBuf> {
    std::env::var("OPPI_RUNTIME_STORE_DIR")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn agentic_result_from_background_events(mut turn: Turn, events: Vec<Event>) -> AgenticTurnResult {
    let mut awaiting_approval = None;
    let mut awaiting_question = None;
    for event in &events {
        if event.turn_id.as_deref() != Some(turn.id.as_str()) {
            continue;
        }
        match &event.kind {
            EventKind::TurnPhaseChanged { phase } => turn.phase = *phase,
            EventKind::TurnCompleted { .. } => turn.status = TurnStatus::Completed,
            EventKind::TurnInterrupted { .. } | EventKind::TurnAborted { .. } => {
                turn.status = TurnStatus::Aborted;
            }
            EventKind::ApprovalRequested { request } => {
                turn.status = TurnStatus::WaitingForApproval;
                awaiting_approval = Some(request.clone());
            }
            EventKind::AskUserRequested { request } => {
                turn.status = TurnStatus::WaitingForUser;
                awaiting_question = Some(request.clone());
            }
            _ => {}
        }
    }
    AgenticTurnResult {
        turn,
        events,
        awaiting_approval,
        awaiting_question,
    }
}

fn json_rpc_internal(message: impl Into<String>) -> JsonRpcError {
    JsonRpcError {
        code: -32603,
        message: message.into(),
        data: Some(json!({ "code": "internal_error" })),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!(
            "oppi-server --stdio\n\nExperimental OPPi JSON-RPC server. Reads one JSON-RPC request per stdin line and writes one response per stdout line."
        );
        return;
    }
    if args.iter().any(|arg| arg == "--version" || arg == "-V") {
        println!("{}", env!("CARGO_PKG_VERSION"));
        return;
    }
    if !args.is_empty() && !args.iter().any(|arg| arg == "--stdio") {
        eprintln!("unknown arguments: {}", args.join(" "));
        std::process::exit(2);
    }

    if let Err(error) = run_stdio() {
        eprintln!("oppi-server failed: {error}");
        std::process::exit(1);
    }
}

fn run_stdio() -> io::Result<()> {
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    let state = ServerState::new();

    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let (response, action) = response_for_line_with_state_action(&state, &line);
        writeln!(
            stdout,
            "{}",
            serde_json::to_string(&response).expect("serialize JSON-RPC response")
        )?;
        stdout.flush()?;
        if action == StdioAction::Shutdown {
            break;
        }
    }

    Ok(())
}

#[cfg(test)]
fn response_for_line(runtime: &mut Runtime, line: &str) -> JsonRpcResponse {
    response_for_line_with_action(runtime, line).0
}

#[cfg(test)]
fn response_for_line_with_state(state: &ServerState, line: &str) -> JsonRpcResponse {
    response_for_line_with_state_action(state, line).0
}

fn response_for_line_with_state_action(
    state: &ServerState,
    line: &str,
) -> (JsonRpcResponse, StdioAction) {
    match serde_json::from_str::<JsonRpcRequest>(line) {
        Ok(request) => {
            let shutdown_requested = request.method == "server/shutdown";
            let request_id = request.id.clone();
            let mut response = handle_state_request(state, request);
            if response.error.is_none()
                && let Err(error) = state.persist_new_events()
            {
                response = JsonRpcResponse {
                    jsonrpc: "2.0",
                    id: request_id,
                    result: None,
                    error: Some(runtime_error(error)),
                };
            }
            let action = if shutdown_requested && response.error.is_none() {
                StdioAction::Shutdown
            } else {
                StdioAction::Continue
            };
            (response, action)
        }
        Err(error) => (
            JsonRpcResponse {
                jsonrpc: "2.0",
                id: Value::Null,
                result: None,
                error: Some(JsonRpcError {
                    code: -32700,
                    message: "parse error".to_string(),
                    data: Some(json!({ "detail": error.to_string() })),
                }),
            },
            StdioAction::Continue,
        ),
    }
}

#[cfg(test)]
fn response_for_line_with_action(
    runtime: &mut Runtime,
    line: &str,
) -> (JsonRpcResponse, StdioAction) {
    match serde_json::from_str::<JsonRpcRequest>(line) {
        Ok(request) => {
            let shutdown_requested = request.method == "server/shutdown";
            let response = handle_request(runtime, request);
            let action = if shutdown_requested && response.error.is_none() {
                StdioAction::Shutdown
            } else {
                StdioAction::Continue
            };
            (response, action)
        }
        Err(error) => (
            JsonRpcResponse {
                jsonrpc: "2.0",
                id: Value::Null,
                result: None,
                error: Some(JsonRpcError {
                    code: -32700,
                    message: "parse error".to_string(),
                    data: Some(json!({ "detail": error.to_string() })),
                }),
            },
            StdioAction::Continue,
        ),
    }
}

fn handle_state_request(state: &ServerState, request: JsonRpcRequest) -> JsonRpcResponse {
    let required_token = std::env::var("OPPI_SERVER_AUTH_TOKEN")
        .ok()
        .filter(|token| !token.is_empty());
    handle_state_request_with_auth(state, request, required_token.as_deref())
}

fn handle_state_request_with_auth(
    state: &ServerState,
    request: JsonRpcRequest,
    required_token: Option<&str>,
) -> JsonRpcResponse {
    let id = request.id.clone();
    match catch_unwind(AssertUnwindSafe(|| {
        handle_state_request_with_auth_inner(state, request, required_token)
    })) {
        Ok(response) => response,
        Err(_) => JsonRpcResponse {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(JsonRpcError {
                code: -32603,
                message: "internal error".to_string(),
                data: Some(json!({ "code": "internal_panic" })),
            }),
        },
    }
}

fn handle_state_request_with_auth_inner(
    state: &ServerState,
    request: JsonRpcRequest,
    required_token: Option<&str>,
) -> JsonRpcResponse {
    let id = request.id.clone();
    if matches!(
        request.method.as_str(),
        "sandbox/exec" | "sandbox/windows-wfp-install"
    ) && required_token.is_none()
    {
        return JsonRpcResponse {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(JsonRpcError {
                code: -32001,
                message: format!("{} requires OPPI_SERVER_AUTH_TOKEN", request.method),
                data: Some(json!({ "code": "auth_required" })),
            }),
        };
    }
    if let Some(token) = required_token
        && request.method != "initialize"
        && !request_has_valid_auth_token(&request.params, token)
    {
        return JsonRpcResponse {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(JsonRpcError {
                code: -32001,
                message: "authentication required".to_string(),
                data: Some(json!({ "code": "auth_required" })),
            }),
        };
    }

    let result = match request.method.as_str() {
        "events/list" => decode::<EventsListParams>(request.params)
            .and_then(|params| match state.runtime.try_lock() {
                Ok(runtime) => runtime.events_after(params).map_err(runtime_error),
                Err(std::sync::TryLockError::WouldBlock) => state.events_after_mirror(params),
                Err(std::sync::TryLockError::Poisoned(_)) => {
                    Err(json_rpc_internal("runtime lock poisoned"))
                }
            })
            .map(|result| json!(result)),
        "turn/run-agentic" => decode::<AgenticTurnParams>(request.params).and_then(|params| {
            if params.execution_mode == ExecutionMode::Background {
                state
                    .run_agentic_background(params)
                    .map(|result| json!(result))
            } else {
                state
                    .runtime
                    .lock()
                    .map_err(|_| json_rpc_internal("runtime lock poisoned"))?
                    .run_agentic_turn(params)
                    .map(|result| json!(result))
                    .map_err(runtime_error)
            }
        }),
        "turn/interrupt" => decode::<TurnInterruptParams>(request.params).and_then(|params| {
            match state.runtime.try_lock() {
                Ok(mut runtime) => runtime
                    .interrupt_turn(&params.thread_id, &params.turn_id, params.reason)
                    .map(|result| json!(result))
                    .map_err(runtime_error),
                Err(std::sync::TryLockError::WouldBlock) => state
                    .request_background_interrupt(params)
                    .map(|result| json!(result)),
                Err(std::sync::TryLockError::Poisoned(_)) => {
                    Err(json_rpc_internal("runtime lock poisoned"))
                }
            }
        }),
        _ => match state.runtime.lock() {
            Ok(mut runtime) => {
                return handle_request_with_auth_inner(&mut runtime, request, required_token);
            }
            Err(_) => Err(json_rpc_internal("runtime lock poisoned")),
        },
    };

    match result {
        Ok(value) => JsonRpcResponse {
            jsonrpc: "2.0",
            id,
            result: Some(value),
            error: None,
        },
        Err(error) => JsonRpcResponse {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(error),
        },
    }
}

#[cfg(test)]
fn handle_request(runtime: &mut Runtime, request: JsonRpcRequest) -> JsonRpcResponse {
    let required_token = std::env::var("OPPI_SERVER_AUTH_TOKEN")
        .ok()
        .filter(|token| !token.is_empty());
    handle_request_with_auth(runtime, request, required_token.as_deref())
}

#[cfg(test)]
fn handle_request_with_auth(
    runtime: &mut Runtime,
    request: JsonRpcRequest,
    required_token: Option<&str>,
) -> JsonRpcResponse {
    let id = request.id.clone();
    match catch_unwind(AssertUnwindSafe(|| {
        handle_request_with_auth_inner(runtime, request, required_token)
    })) {
        Ok(response) => response,
        Err(_) => JsonRpcResponse {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(JsonRpcError {
                code: -32603,
                message: "internal error".to_string(),
                data: Some(json!({ "code": "internal_panic" })),
            }),
        },
    }
}

fn handle_request_with_auth_inner(
    runtime: &mut Runtime,
    request: JsonRpcRequest,
    required_token: Option<&str>,
) -> JsonRpcResponse {
    let id = request.id;
    if matches!(
        request.method.as_str(),
        "sandbox/exec" | "sandbox/windows-wfp-install"
    ) && required_token.is_none()
    {
        return JsonRpcResponse {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(JsonRpcError {
                code: -32001,
                message: format!("{} requires OPPI_SERVER_AUTH_TOKEN", request.method),
                data: Some(json!({ "code": "auth_required" })),
            }),
        };
    }
    if let Some(token) = required_token
        && request.method != "initialize"
        && !request_has_valid_auth_token(&request.params, token)
    {
        return JsonRpcResponse {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(JsonRpcError {
                code: -32001,
                message: "authentication required".to_string(),
                data: Some(json!({ "code": "auth_required" })),
            }),
        };
    }
    let result = match request.method.as_str() {
        #[cfg(test)]
        "__test/panic" => panic!("intentional JSON-RPC panic-boundary test"),
        "initialize" => decode::<InitializeParams>(request.params)
            .map(|params| json!(runtime.initialize(params))),
        "server/shutdown" => Ok(json!({ "shuttingDown": true })),
        "runtime/metrics" => Ok(json!(runtime.metrics())),
        "debug/bundle" => {
            let sandbox = current_sandbox_status();
            let diagnostic = Diagnostic {
                level: DiagnosticLevel::Info,
                message: sandbox.message.clone(),
                metadata: BTreeMap::from([
                    ("component".to_string(), "sandbox".to_string()),
                    ("platform".to_string(), sandbox.platform),
                    (
                        "enforcement".to_string(),
                        serde_json::to_value(sandbox.enforcement)
                            .ok()
                            .and_then(|value| value.as_str().map(str::to_string))
                            .unwrap_or_else(|| "unknown".to_string()),
                    ),
                    ("supported".to_string(), sandbox.supported.to_string()),
                ]),
            };
            Ok(json!(runtime.debug_bundle(vec![diagnostic])))
        }
        "thread/list" => Ok(json!(runtime.list_threads())),
        "thread/start" => decode::<ThreadStartParams>(request.params)
            .map(|params| json!(runtime.start_thread(params))),
        "thread/resume" => decode_thread_id(request.params)
            .and_then(|thread_id| runtime.resume_thread(&thread_id).map_err(runtime_error))
            .map(|result| json!(result)),
        "thread/fork" => decode::<ThreadForkParams>(request.params)
            .and_then(|params| {
                runtime
                    .fork_thread(&params.thread_id, params.title)
                    .map_err(runtime_error)
            })
            .map(|result| json!(result)),
        "thread/rename" => decode::<ThreadRenameParams>(request.params)
            .and_then(|params| {
                runtime
                    .rename_thread(&params.thread_id, params.title)
                    .map_err(runtime_error)
            })
            .map(|result| json!(result)),
        "thread/archive" | "thread/delete" => decode_thread_id(request.params)
            .and_then(|thread_id| runtime.archive_thread(&thread_id).map_err(runtime_error))
            .map(|result| json!(result)),
        "thread/goal/get" => decode::<ThreadGoalGetParams>(request.params)
            .and_then(|params| runtime.get_thread_goal(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "thread/goal/set" => decode::<ThreadGoalSetParams>(request.params)
            .and_then(|params| runtime.set_thread_goal(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "thread/goal/clear" => decode::<ThreadGoalClearParams>(request.params)
            .and_then(|params| runtime.clear_thread_goal(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "thread/goal/continuation" => decode::<ThreadGoalContinuationParams>(request.params)
            .and_then(|params| {
                runtime
                    .next_thread_goal_continuation(params)
                    .map_err(runtime_error)
            })
            .map(|result| json!(result)),
        "turn/start" => decode::<TurnStartParams>(request.params)
            .and_then(|params| runtime.start_turn(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "turn/run-agentic" => decode::<AgenticTurnParams>(request.params)
            .and_then(|params| runtime.run_agentic_turn(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "turn/resume-agentic" => decode::<AgenticTurnResumeParams>(request.params)
            .and_then(|params| runtime.resume_agentic_turn(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "turn/steer" => decode::<TurnSteerParams>(request.params)
            .and_then(|params| {
                runtime
                    .steer_turn(&params.thread_id, &params.turn_id, params.input)
                    .map_err(runtime_error)
            })
            .map(|result| json!(result)),
        "turn/interrupt" => decode::<TurnInterruptParams>(request.params)
            .and_then(|params| {
                runtime
                    .interrupt_turn(&params.thread_id, &params.turn_id, params.reason)
                    .map_err(runtime_error)
            })
            .map(|result| json!(result)),
        "tool/record" => decode::<ToolRecordParams>(request.params)
            .and_then(|params| runtime.record_tool(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "tool/batch" => decode::<ToolBatchRecordParams>(request.params)
            .and_then(|params| runtime.record_tool_batch(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "command/prepare" => decode::<CommandPrepareParams>(request.params)
            .and_then(|params| runtime.prepare_command(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "sandbox/status" => Ok(json!(current_sandbox_status())),
        "sandbox/windows-wfp-status" => {
            let status = windows_wfp_status();
            Ok(json!({
                "available": status.available,
                "filterCount": status.filter_count,
                "message": status.message,
            }))
        }
        "sandbox/windows-wfp-install" => decode::<WindowsWfpInstallParams>(request.params)
            .and_then(|params| {
                install_windows_wfp_filters_for_account(&params.account)
                    .map(|installed| json!({ "installed": installed, "account": params.account }))
                    .map_err(|message| JsonRpcError {
                        code: -32020,
                        message,
                        data: Some(json!({ "code": "windows_wfp_install_failed" })),
                    })
            }),
        "sandbox/plan" => decode::<SandboxPlanParams>(request.params).map(|params| {
            let manager = SandboxManager::detect();
            let transform = manager.transform(&params.policy, &params.request, params.preference);
            json!(SandboxPlanResult::from(transform))
        }),
        "sandbox/exec" => decode::<SandboxExecParams>(request.params)
            .map(|params| json!(execute_sandboxed(params))),
        "approval/request" => decode::<ApprovalRequestParams>(request.params)
            .and_then(|params| {
                runtime
                    .request_approval(&params.thread_id, params.request)
                    .map_err(runtime_error)
            })
            .map(|result| json!(result)),
        "approval/respond" => decode::<ApprovalRespondParams>(request.params)
            .and_then(|params| runtime.respond_approval(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "question/request" => decode::<QuestionRequestParams>(request.params)
            .and_then(|params| {
                runtime
                    .request_question(&params.thread_id, params.request)
                    .map_err(runtime_error)
            })
            .map(|result| json!(result)),
        "question/respond" => decode::<QuestionRespondParams>(request.params)
            .and_then(|params| runtime.respond_question(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "plugin/register" => decode::<PluginRegisterParams>(request.params)
            .and_then(|params| {
                runtime
                    .register_plugin(&params.thread_id, params.plugin)
                    .map_err(runtime_error)
            })
            .map(|result| json!(result)),
        "plugin/list" => Ok(json!(runtime.list_plugins())),
        "background/list" => Ok(json!(runtime.list_background_tasks())),
        "background/read" => decode::<BackgroundReadParams>(request.params)
            .and_then(|params| runtime.read_background_task(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "background/kill" => decode::<BackgroundKillParams>(request.params)
            .and_then(|params| runtime.kill_background_task(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "memory/status" => Ok(json!(runtime.memory_status())),
        "todos/list" => Ok(json!(runtime.todos_state())),
        "todos/client-action" => decode::<TodoClientActionParams>(request.params)
            .and_then(|params| {
                runtime
                    .apply_todo_client_action(params)
                    .map_err(runtime_error)
            })
            .map(|result| json!(result)),
        "memory/set" => decode::<MemorySetParams>(request.params)
            .and_then(|params| runtime.set_memory_status(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "memory/control" => decode::<MemoryControlParams>(request.params)
            .and_then(|params| runtime.memory_control(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "memory/compact" => decode::<HandoffCompactParams>(request.params)
            .and_then(|params| runtime.compact_handoff(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "mcp/register" => decode::<McpRegisterParams>(request.params)
            .and_then(|params| {
                runtime
                    .register_mcp(&params.thread_id, params.server)
                    .map_err(runtime_error)
            })
            .map(|result| json!(result)),
        "mcp/list" => Ok(json!(runtime.list_mcp())),
        "mcp/action" => decode::<McpActionParams>(request.params)
            .and_then(|params| runtime.mcp_action(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "mcp/reload" => decode::<McpServerActionParams>(request.params)
            .map(|params| McpActionParams {
                thread_id: params.thread_id,
                server_id: params.server_id,
                action: McpAction::Reload,
            })
            .and_then(|params| runtime.mcp_action(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "mcp/test" => decode::<McpServerActionParams>(request.params)
            .map(|params| McpActionParams {
                thread_id: params.thread_id,
                server_id: params.server_id,
                action: McpAction::Test,
            })
            .and_then(|params| runtime.mcp_action(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "mcp/auth" => decode::<McpServerActionParams>(request.params)
            .map(|params| McpActionParams {
                thread_id: params.thread_id,
                server_id: params.server_id,
                action: McpAction::Auth,
            })
            .and_then(|params| runtime.mcp_action(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "model/register" => decode::<ModelRegisterParams>(request.params)
            .and_then(|params| {
                runtime
                    .register_model(&params.thread_id, params.model)
                    .map_err(runtime_error)
            })
            .map(|result| json!(result)),
        "model/list" => Ok(json!(runtime.list_models())),
        "model/select" => decode::<ModelSelectParams>(request.params)
            .and_then(|params| runtime.select_model(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "agents/register" => decode::<AgentRegisterParams>(request.params)
            .and_then(|params| {
                runtime
                    .register_agent(&params.thread_id, params.agent)
                    .map_err(runtime_error)
            })
            .map(|result| json!(result)),
        "agents/list" => Ok(json!(runtime.list_agents())),
        "skills/list" => decode::<SkillListParams>(request.params)
            .and_then(|params| runtime.list_skills(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "agents/dispatch" => decode::<AgentDispatchParams>(request.params)
            .and_then(|params| runtime.dispatch_agent(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "agents/block" => decode::<AgentBlockParams>(request.params)
            .and_then(|params| runtime.block_agent(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "agents/complete" => decode::<AgentCompleteParams>(request.params)
            .and_then(|params| runtime.complete_agent(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "side-question/ask" => decode::<SideQuestionParams>(request.params)
            .and_then(|params| runtime.side_question(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "pi/bridge-event" => decode::<PiBridgeEventParams>(request.params)
            .and_then(|params| runtime.bridge_pi_event(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "events/list" => decode::<EventsListParams>(request.params)
            .and_then(|params| runtime.events_after(params).map_err(runtime_error))
            .map(|result| json!(result)),
        _ => Err(JsonRpcError {
            code: -32601,
            message: format!("method not found: {}", request.method),
            data: None,
        }),
    };

    match result {
        Ok(result) => JsonRpcResponse {
            jsonrpc: "2.0",
            id,
            result: Some(result),
            error: None,
        },
        Err(error) => JsonRpcResponse {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(error),
        },
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ThreadForkParams {
    thread_id: String,
    #[serde(default)]
    title: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ThreadRenameParams {
    thread_id: String,
    title: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TurnSteerParams {
    thread_id: String,
    turn_id: String,
    input: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TurnInterruptParams {
    thread_id: String,
    turn_id: String,
    reason: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApprovalRequestParams {
    thread_id: String,
    request: ApprovalRequest,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct QuestionRequestParams {
    thread_id: String,
    request: QuestionRequest,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PluginRegisterParams {
    thread_id: String,
    plugin: PluginRef,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct McpRegisterParams {
    thread_id: String,
    server: McpServerRef,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct McpServerActionParams {
    thread_id: String,
    server_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModelRegisterParams {
    thread_id: String,
    model: ModelRef,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentRegisterParams {
    thread_id: String,
    agent: AgentDefinition,
}

fn request_has_valid_auth_token(params: &Value, required_token: &str) -> bool {
    params
        .get("authToken")
        .and_then(Value::as_str)
        .is_some_and(|token| token == required_token)
}

fn decode<T>(value: Value) -> Result<T, JsonRpcError>
where
    T: for<'de> Deserialize<'de>,
{
    serde_json::from_value(value).map_err(|error| JsonRpcError {
        code: -32602,
        message: "invalid params".to_string(),
        data: Some(json!({ "detail": error.to_string() })),
    })
}

fn decode_thread_id(value: Value) -> Result<String, JsonRpcError> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Params {
        thread_id: String,
    }
    decode::<Params>(value).map(|params| params.thread_id)
}

fn runtime_error(error: RuntimeError) -> JsonRpcError {
    JsonRpcError {
        code: -32000,
        message: error.message,
        data: Some(json!({
            "code": error.code,
            "category": error.category.as_str(),
        })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store_root(name: &str) -> PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("{name}-{}-{unique}", std::process::id()))
    }

    fn read_http_request(stream: &mut std::net::TcpStream) -> String {
        use std::io::Read;
        let mut bytes = Vec::new();
        let mut buffer = [0u8; 1024];
        loop {
            let read = stream.read(&mut buffer).unwrap();
            if read == 0 {
                break;
            }
            bytes.extend_from_slice(&buffer[..read]);
            if bytes.windows(4).any(|window| window == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&bytes).to_string();
                let content_length = headers
                    .lines()
                    .find_map(|line| line.strip_prefix("Content-Length:"))
                    .and_then(|value| value.trim().parse::<usize>().ok())
                    .unwrap_or(0);
                let header_end = bytes
                    .windows(4)
                    .position(|window| window == b"\r\n\r\n")
                    .map(|index| index + 4)
                    .unwrap_or(bytes.len());
                while bytes.len().saturating_sub(header_end) < content_length {
                    let read = stream.read(&mut buffer).unwrap();
                    if read == 0 {
                        break;
                    }
                    bytes.extend_from_slice(&buffer[..read]);
                }
                break;
            }
        }
        String::from_utf8_lossy(&bytes).to_string()
    }

    fn start_mock_openai_server(
        responses: Vec<String>,
    ) -> (String, std::thread::JoinHandle<Vec<String>>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            let mut requests = Vec::new();
            for response in responses {
                let (mut stream, _) = listener.accept().unwrap();
                requests.push(read_http_request(&mut stream));
                let reply = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    response.len(),
                    response
                );
                stream.write_all(reply.as_bytes()).unwrap();
            }
            requests
        });
        (format!("http://{address}/v1"), handle)
    }

    #[test]
    fn initialize_returns_protocol_metadata() {
        let mut runtime = Runtime::new();
        let request = JsonRpcRequest {
            jsonrpc: Some("2.0".to_string()),
            id: json!(1),
            method: "initialize".to_string(),
            params: json!({
                "clientName": "test",
                "protocolVersion": "0.1.0",
                "clientCapabilities": ["sandbox", "unknown-future-capability"]
            }),
        };
        let response = handle_request(&mut runtime, request);
        assert!(response.error.is_none());
        let result = response.result.unwrap();
        assert_eq!(result["serverName"], json!("oppi-server"));
        assert_eq!(result["protocolCompatible"], json!(true));
        assert_eq!(result["acceptedClientCapabilities"], json!(["sandbox"]));
    }

    #[test]
    fn initialize_reports_incompatible_future_major_protocol() {
        let mut runtime = Runtime::new();
        let request = JsonRpcRequest {
            jsonrpc: Some("2.0".to_string()),
            id: json!(1),
            method: "initialize".to_string(),
            params: json!({ "clientName": "test", "protocolVersion": "99.0.0" }),
        };
        let response = handle_request(&mut runtime, request);
        assert!(response.error.is_none());
        assert_eq!(response.result.unwrap()["protocolCompatible"], json!(false));
    }

    #[test]
    fn thread_goal_rpc_set_get_clear_round_trips() {
        let mut runtime = Runtime::new();
        let start = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(1),
                method: "thread/start".to_string(),
                params: json!({ "project": { "id": "project", "cwd": "/repo" } }),
            },
        );
        let thread_id = start.result.unwrap()["thread"]["id"]
            .as_str()
            .unwrap()
            .to_string();

        let set = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(2),
                method: "thread/goal/set".to_string(),
                params: json!({
                    "threadId": thread_id,
                    "objective": "Ship /goal",
                    "status": "active",
                    "tokenBudget": 10_000
                }),
            },
        );
        assert!(set.error.is_none(), "{:?}", set.error);
        assert_eq!(
            set.result.unwrap()["goal"]["objective"],
            json!("Ship /goal")
        );

        let get = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(3),
                method: "thread/goal/get".to_string(),
                params: json!({ "threadId": thread_id }),
            },
        );
        assert_eq!(get.result.unwrap()["goal"]["tokenBudget"], json!(10_000));

        let clear = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(4),
                method: "thread/goal/clear".to_string(),
                params: json!({ "threadId": thread_id }),
            },
        );
        assert_eq!(clear.result.unwrap()["cleared"], json!(true));
    }

    #[test]
    fn thread_rename_and_archive_round_trip_over_json_rpc() {
        let mut runtime = Runtime::new();
        let start = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(1),
                method: "thread/start".to_string(),
                params: json!({
                    "project": { "id": "project", "cwd": "/repo" },
                    "title": "Original"
                }),
            },
        );
        let thread_id = start.result.unwrap()["thread"]["id"]
            .as_str()
            .unwrap()
            .to_string();

        let renamed = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(2),
                method: "thread/rename".to_string(),
                params: json!({ "threadId": thread_id, "title": "Renamed" }),
            },
        );
        assert!(renamed.error.is_none(), "{:?}", renamed.error);
        assert_eq!(renamed.result.unwrap()["thread"]["title"], json!("Renamed"));

        let archived = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(3),
                method: "thread/archive".to_string(),
                params: json!({ "threadId": thread_id }),
            },
        );
        assert!(archived.error.is_none(), "{:?}", archived.error);
        assert_eq!(
            archived.result.unwrap()["thread"]["status"],
            json!("archived")
        );
    }

    #[test]
    fn thread_goal_continuation_rpc_returns_prompt_and_guard() {
        let mut runtime = Runtime::new();
        let start = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(1),
                method: "thread/start".to_string(),
                params: json!({ "project": { "id": "project", "cwd": "/repo" } }),
            },
        );
        let thread_id = start.result.unwrap()["thread"]["id"]
            .as_str()
            .unwrap()
            .to_string();
        let _ = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(2),
                method: "thread/goal/set".to_string(),
                params: json!({
                    "threadId": thread_id,
                    "objective": "Ship continuation",
                    "status": "active"
                }),
            },
        );

        let first = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(3),
                method: "thread/goal/continuation".to_string(),
                params: json!({ "threadId": thread_id, "maxContinuations": 1 }),
            },
        );
        let first = first.result.unwrap();
        assert_eq!(first["continuation"], json!(1));
        assert!(
            first["prompt"]
                .as_str()
                .unwrap()
                .contains("Ship continuation")
        );

        let blocked = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(4),
                method: "thread/goal/continuation".to_string(),
                params: json!({ "threadId": thread_id, "maxContinuations": 1 }),
            },
        );
        let blocked = blocked.result.unwrap();
        assert!(blocked["prompt"].is_null());
        assert_eq!(blocked["goal"]["status"], json!("paused"));
        assert!(blocked["blockedReason"].as_str().unwrap().contains("guard"));
    }

    #[test]
    fn auth_token_gate_rejects_unauthenticated_non_initialize_requests() {
        let mut runtime = Runtime::new();
        let response = handle_request_with_auth(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(1),
                method: "thread/start".to_string(),
                params: json!({ "project": { "id": "project", "cwd": "/repo" } }),
            },
            Some("secret"),
        );
        assert_eq!(response.error.unwrap().code, -32001);
    }

    #[test]
    fn auth_token_gate_accepts_matching_capability_token() {
        let mut runtime = Runtime::new();
        let response = handle_request_with_auth(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(1),
                method: "thread/start".to_string(),
                params: json!({
                    "authToken": "secret",
                    "project": { "id": "project", "cwd": "/repo" }
                }),
            },
            Some("secret"),
        );
        assert!(response.error.is_none(), "{:?}", response.error);
        assert_eq!(response.result.unwrap()["thread"]["id"], json!("thread-1"));
    }

    #[test]
    fn runtime_errors_include_stable_code_and_category() {
        let mut runtime = Runtime::new();
        let response = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(1),
                method: "events/list".to_string(),
                params: json!({ "threadId": "missing-thread" }),
            },
        );
        let error = response.error.expect("runtime error");
        assert_eq!(error.code, -32000);
        assert_eq!(
            error.data.as_ref().unwrap()["code"],
            json!("thread_not_found")
        );
        assert_eq!(error.data.unwrap()["category"], json!("not_found"));
    }

    #[test]
    fn background_agentic_turn_mirrors_events_for_polling() {
        let state = ServerState::new();
        let start = response_for_line_with_state(
            &state,
            &json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "thread/start",
                "params": { "project": { "id": "project", "cwd": "/repo" } }
            })
            .to_string(),
        );
        assert!(start.error.is_none(), "{:?}", start.error);
        let thread_id = start.result.unwrap()["thread"]["id"]
            .as_str()
            .unwrap()
            .to_string();

        let run = response_for_line_with_state(
            &state,
            &json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "turn/run-agentic",
                "params": {
                    "threadId": thread_id,
                    "input": "ask before continuing",
                    "executionMode": "background",
                    "modelSteps": [{
                        "toolCalls": [{
                            "id": "ask-1",
                            "namespace": "oppi",
                            "name": "ask_user",
                            "arguments": {
                                "title": "Choose",
                                "questions": [{
                                    "id": "q1",
                                    "question": "Proceed?",
                                    "options": [{ "id": "yes", "label": "Yes" }]
                                }]
                            }
                        }]
                    }]
                }
            })
            .to_string(),
        );
        assert!(run.error.is_none(), "{:?}", run.error);
        let result = run.result.unwrap();
        let turn_id = result["turn"]["id"].as_str().unwrap().to_string();
        assert!(
            result["turn"]["status"] == json!("running")
                || result["turn"]["status"] == json!("waitingForUser")
        );
        assert!(
            result["events"]
                .as_array()
                .unwrap()
                .iter()
                .any(|event| { event["kind"]["type"] == json!("turnStarted") })
        );

        let mut listed = Vec::new();
        for _ in 0..50 {
            let response = response_for_line_with_state(
                &state,
                &json!({
                    "jsonrpc": "2.0",
                    "id": 3,
                    "method": "events/list",
                    "params": { "threadId": thread_id, "after": 0 }
                })
                .to_string(),
            );
            assert!(response.error.is_none(), "{:?}", response.error);
            listed = response.result.unwrap()["events"]
                .as_array()
                .unwrap()
                .clone();
            if listed
                .iter()
                .any(|event| event["kind"]["type"] == json!("askUserRequested"))
            {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert!(listed.iter().any(|event| {
            event["turnId"] == json!(turn_id) && event["kind"]["type"] == json!("askUserRequested")
        }));
    }

    #[test]
    fn background_agentic_turn_streams_deltas_before_completion() {
        let state = ServerState::new();
        let start = response_for_line_with_state(
            &state,
            &json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "thread/start",
                "params": { "project": { "id": "project", "cwd": "/repo" } }
            })
            .to_string(),
        );
        assert!(start.error.is_none(), "{:?}", start.error);
        let thread_id = start.result.unwrap()["thread"]["id"]
            .as_str()
            .unwrap()
            .to_string();

        let run = response_for_line_with_state(
            &state,
            &json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "turn/run-agentic",
                "params": {
                    "threadId": thread_id,
                    "input": "stream while slow tool runs",
                    "executionMode": "background",
                    "modelSteps": [
                        {
                            "assistantDeltas": ["streamed ", "delta"],
                            "toolCalls": [{
                                "id": "slow-echo",
                                "namespace": "oppi",
                                "name": "echo",
                                "arguments": { "output": "slow", "delayMs": 250 }
                            }]
                        },
                        { "assistantDeltas": ["done"], "finalResponse": true }
                    ],
                    "maxContinuations": 2
                }
            })
            .to_string(),
        );
        assert!(run.error.is_none(), "{:?}", run.error);
        let turn_id = run.result.unwrap()["turn"]["id"]
            .as_str()
            .unwrap()
            .to_string();

        let mut listed = Vec::new();
        let mut saw_delta_before_completion = false;
        for _ in 0..50 {
            let response = response_for_line_with_state(
                &state,
                &json!({
                    "jsonrpc": "2.0",
                    "id": 3,
                    "method": "events/list",
                    "params": { "threadId": thread_id, "after": 0 }
                })
                .to_string(),
            );
            assert!(response.error.is_none(), "{:?}", response.error);
            listed = response.result.unwrap()["events"]
                .as_array()
                .unwrap()
                .clone();
            let saw_delta = listed.iter().any(|event| {
                event["turnId"] == json!(turn_id) && event["kind"]["type"] == json!("itemDelta")
            });
            let saw_completed = listed.iter().any(|event| {
                event["turnId"] == json!(turn_id) && event["kind"]["type"] == json!("turnCompleted")
            });
            if saw_delta && !saw_completed {
                saw_delta_before_completion = true;
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert!(saw_delta_before_completion, "events: {listed:?}");
    }

    #[test]
    fn background_agentic_turn_can_be_interrupted_while_runtime_busy() {
        let state = ServerState::new();
        let start = response_for_line_with_state(
            &state,
            &json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "thread/start",
                "params": { "project": { "id": "project", "cwd": "/repo" } }
            })
            .to_string(),
        );
        assert!(start.error.is_none(), "{:?}", start.error);
        let thread_id = start.result.unwrap()["thread"]["id"]
            .as_str()
            .unwrap()
            .to_string();

        let run = response_for_line_with_state(
            &state,
            &json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "turn/run-agentic",
                "params": {
                    "threadId": thread_id,
                    "input": "interrupt slow tool",
                    "executionMode": "background",
                    "modelSteps": [{
                        "assistantDeltas": ["working"],
                        "toolCalls": [{
                            "id": "slow-echo",
                            "namespace": "oppi",
                            "name": "echo",
                            "arguments": { "output": "slow", "delayMs": 250 }
                        }]
                    }],
                    "maxContinuations": 2
                }
            })
            .to_string(),
        );
        assert!(run.error.is_none(), "{:?}", run.error);
        let turn_id = run.result.unwrap()["turn"]["id"]
            .as_str()
            .unwrap()
            .to_string();

        let interrupted = response_for_line_with_state(
            &state,
            &json!({
                "jsonrpc": "2.0",
                "id": 3,
                "method": "turn/interrupt",
                "params": { "threadId": thread_id, "turnId": turn_id, "reason": "test interrupt" }
            })
            .to_string(),
        );
        assert!(interrupted.error.is_none(), "{:?}", interrupted.error);
        let events = interrupted.result.unwrap()["events"]
            .as_array()
            .unwrap()
            .clone();
        assert!(
            events.iter().any(|event| {
                event["turnId"] == json!(turn_id)
                    && event["kind"]["type"] == json!("turnInterrupted")
            }),
            "events: {events:?}"
        );

        let listed = response_for_line_with_state(
            &state,
            &json!({
                "jsonrpc": "2.0",
                "id": 4,
                "method": "events/list",
                "params": { "threadId": thread_id, "after": 0 }
            })
            .to_string(),
        );
        assert!(listed.error.is_none(), "{:?}", listed.error);
        let all_events = listed.result.unwrap()["events"].as_array().unwrap().clone();
        assert!(
            !all_events.iter().any(|event| {
                event["turnId"] == json!(turn_id) && event["kind"]["type"] == json!("turnCompleted")
            }),
            "events: {all_events:?}"
        );
    }

    #[test]
    fn background_direct_provider_approval_resume_preserves_tool_transcript() {
        let root = std::env::temp_dir().join(format!(
            "oppi-server-bg-resume-{}-{}",
            std::process::id(),
            1
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let first = json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "write-approved",
                        "type": "function",
                        "function": {
                            "name": "oppi__write_file",
                            "arguments": "{\"path\":\"approved.txt\",\"content\":\"approved\"}"
                        }
                    }]
                }
            }]
        })
        .to_string();
        let second = json!({
            "choices": [{
                "message": { "role": "assistant", "content": "continued after approval" }
            }]
        })
        .to_string();
        let (base_url, server) = start_mock_openai_server(vec![first, second]);
        let key_name = "OPPI_SERVER_BG_RESUME_API_KEY";
        unsafe { std::env::set_var(key_name, "test-key") };
        let state = ServerState::new();
        let start = response_for_line_with_state(
            &state,
            &json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "thread/start",
                "params": { "project": { "id": "project", "cwd": root.display().to_string() } }
            })
            .to_string(),
        );
        assert!(start.error.is_none(), "{:?}", start.error);
        let thread_id = start.result.unwrap()["thread"]["id"]
            .as_str()
            .unwrap()
            .to_string();

        let run = response_for_line_with_state(
            &state,
            &json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "turn/run-agentic",
                "params": {
                    "threadId": thread_id,
                    "input": "write with approval",
                    "executionMode": "background",
                    "modelProvider": {
                        "kind": "openai-compatible",
                        "model": "mock",
                        "baseUrl": base_url,
                        "apiKeyEnv": key_name,
                        "stream": false
                    },
                    "maxContinuations": 2
                }
            })
            .to_string(),
        );
        assert!(run.error.is_none(), "{:?}", run.error);
        let turn_id = run.result.unwrap()["turn"]["id"]
            .as_str()
            .unwrap()
            .to_string();

        let mut listed = Vec::new();
        for _ in 0..50 {
            let response = response_for_line_with_state(
                &state,
                &json!({
                    "jsonrpc": "2.0",
                    "id": 3,
                    "method": "events/list",
                    "params": { "threadId": thread_id, "after": 0 }
                })
                .to_string(),
            );
            assert!(response.error.is_none(), "{:?}", response.error);
            listed = response.result.unwrap()["events"]
                .as_array()
                .unwrap()
                .clone();
            if listed.iter().any(|event| {
                event["turnId"] == json!(turn_id)
                    && event["kind"]["type"] == json!("approvalRequested")
            }) {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert!(
            listed.iter().any(|event| {
                event["turnId"] == json!(turn_id)
                    && event["kind"]["type"] == json!("approvalRequested")
            }),
            "events: {listed:?}"
        );

        let resumed = response_for_line_with_state(
            &state,
            &json!({
                "jsonrpc": "2.0",
                "id": 4,
                "method": "turn/resume-agentic",
                "params": {
                    "threadId": thread_id,
                    "turnId": turn_id,
                    "approvedToolCallIds": ["write-approved"],
                    "maxContinuations": 2
                }
            })
            .to_string(),
        );
        unsafe { std::env::remove_var(key_name) };
        assert!(resumed.error.is_none(), "{:?}", resumed.error);
        assert_eq!(
            resumed.result.as_ref().unwrap()["turn"]["status"],
            json!("completed")
        );
        assert_eq!(
            std::fs::read_to_string(root.join("approved.txt")).unwrap(),
            "approved"
        );
        let requests = server.join().unwrap();
        assert!(
            requests[1].contains("\"role\":\"tool\""),
            "request: {}",
            requests[1]
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn lists_background_tasks_over_json_rpc() {
        let mut runtime = Runtime::new();
        let response = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(1),
                method: "background/list".to_string(),
                params: json!({}),
            },
        );
        assert!(response.error.is_none(), "{:?}", response.error);
        assert_eq!(response.result.unwrap()["items"], json!([]));
    }

    #[test]
    fn runs_agentic_turn_over_json_rpc() {
        let mut runtime = Runtime::new();
        let start = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(1),
                method: "thread/start".to_string(),
                params: json!({ "project": { "id": "project", "cwd": "/repo" } }),
            },
        );
        assert!(start.error.is_none(), "{:?}", start.error);
        let run = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(2),
                method: "turn/run-agentic".to_string(),
                params: json!({
                    "threadId": "thread-1",
                    "input": "call echo",
                    "modelSteps": [
                        {
                            "assistantDeltas": ["Calling echo."],
                            "toolCalls": [
                                {
                                    "id": "echo-1",
                                    "name": "echo",
                                    "namespace": "oppi",
                                    "arguments": { "output": "hello" }
                                }
                            ]
                        },
                        {
                            "assistantDeltas": ["Done."],
                            "finalResponse": true
                        }
                    ]
                }),
            },
        );
        assert!(run.error.is_none(), "{:?}", run.error);
        let result = run.result.unwrap();
        assert_eq!(result["turn"]["status"], json!("completed"));
        assert!(
            result["events"]
                .as_array()
                .unwrap()
                .iter()
                .any(|event| { event["kind"]["type"] == json!("toolCallCompleted") })
        );
    }

    #[test]
    fn runtime_metrics_and_debug_bundle_are_available_over_json_rpc() {
        let mut runtime = Runtime::new();
        let start = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(1),
                method: "thread/start".to_string(),
                params: json!({ "project": { "id": "project", "cwd": "/repo" } }),
            },
        );
        assert!(start.error.is_none(), "{:?}", start.error);

        let metrics = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(2),
                method: "runtime/metrics".to_string(),
                params: json!({}),
            },
        );
        assert!(metrics.error.is_none(), "{:?}", metrics.error);
        assert_eq!(metrics.result.unwrap()["threadCount"], json!(1));

        let bundle = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(3),
                method: "debug/bundle".to_string(),
                params: json!({}),
            },
        );
        assert!(bundle.error.is_none(), "{:?}", bundle.error);
        let result = bundle.result.unwrap();
        assert_eq!(result["redacted"], json!(true));
        assert_eq!(result["metrics"]["threadCount"], json!(1));
        assert!(
            result["diagnostics"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| { item["metadata"]["component"] == json!("sandbox") })
        );
    }

    #[test]
    fn server_shutdown_returns_ack_and_stdio_shutdown_action() {
        let mut runtime = Runtime::new();
        let (response, action) = response_for_line_with_action(
            &mut runtime,
            r#"{"jsonrpc":"2.0","id":"shutdown","method":"server/shutdown","params":{}}"#,
        );
        assert_eq!(action, StdioAction::Shutdown);
        assert!(response.error.is_none(), "{:?}", response.error);
        assert_eq!(response.id, json!("shutdown"));
        assert_eq!(response.result.unwrap()["shuttingDown"], json!(true));
    }

    #[test]
    fn persistent_state_replays_threads_and_events_across_server_restart() {
        let root = temp_store_root("oppi-server-persist");
        let namespace = StoreNamespace::new("project", "runtime");
        let state = ServerState::with_persistence(root.clone(), namespace.clone()).unwrap();
        let start = response_for_line_with_state(
            &state,
            &json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "thread/start",
                "params": { "project": { "id": "project", "cwd": "/repo" }, "title": "Persisted" }
            })
            .to_string(),
        );
        assert!(start.error.is_none(), "{:?}", start.error);
        let thread_id = start.result.as_ref().unwrap()["thread"]["id"]
            .as_str()
            .unwrap();
        assert_eq!(thread_id, "thread-1");
        drop(state);

        let restarted = ServerState::with_persistence(root.clone(), namespace).unwrap();
        let list = response_for_line_with_state(
            &restarted,
            &json!({ "jsonrpc": "2.0", "id": 2, "method": "thread/list", "params": {} })
                .to_string(),
        );
        assert!(list.error.is_none(), "{:?}", list.error);
        assert_eq!(
            list.result.as_ref().unwrap()["items"][0]["id"],
            json!("thread-1")
        );
        let events = response_for_line_with_state(
            &restarted,
            &json!({ "jsonrpc": "2.0", "id": 3, "method": "events/list", "params": { "threadId": "thread-1", "after": 0 } }).to_string(),
        );
        assert!(events.error.is_none(), "{:?}", events.error);
        assert!(events.result.unwrap()["events"].as_array().unwrap().len() >= 1);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn persistent_state_skips_corrupt_threads_without_reusing_their_ids() {
        let root = temp_store_root("oppi-server-corrupt-thread");
        let namespace = StoreNamespace::new("project", "runtime");
        let state = ServerState::with_persistence(root.clone(), namespace.clone()).unwrap();
        let start = response_for_line_with_state(
            &state,
            &json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "thread/start",
                "params": { "project": { "id": "project", "cwd": "/repo" }, "title": "Persisted" }
            })
            .to_string(),
        );
        assert!(start.error.is_none(), "{:?}", start.error);
        drop(state);

        let corrupt_path = root
            .join("projects")
            .join("project")
            .join("runtimes")
            .join("runtime")
            .join("events")
            .join("thread-9.jsonl");
        std::fs::write(corrupt_path, "{{not valid json}}\n").unwrap();

        let restarted = ServerState::with_persistence(root.clone(), namespace).unwrap();
        let list = response_for_line_with_state(
            &restarted,
            &json!({ "jsonrpc": "2.0", "id": 2, "method": "thread/list", "params": {} })
                .to_string(),
        );
        assert!(list.error.is_none(), "{:?}", list.error);
        assert_eq!(
            list.result.as_ref().unwrap()["items"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            list.result.as_ref().unwrap()["items"][0]["id"],
            json!("thread-1")
        );

        let next = response_for_line_with_state(
            &restarted,
            &json!({
                "jsonrpc": "2.0",
                "id": 3,
                "method": "thread/start",
                "params": { "project": { "id": "project", "cwd": "/repo" }, "title": "Next" }
            })
            .to_string(),
        );
        assert!(next.error.is_none(), "{:?}", next.error);
        assert_eq!(
            next.result.as_ref().unwrap()["thread"]["id"],
            json!("thread-10")
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn todo_client_actions_mark_done_and_clear_with_events() {
        let state = ServerState::without_persistence();
        let start = response_for_line_with_state(
            &state,
            &json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "thread/start",
                "params": { "project": { "id": "project", "cwd": "/repo" }, "title": "Todos" }
            })
            .to_string(),
        );
        assert!(start.error.is_none(), "{:?}", start.error);
        let thread_id = start.result.as_ref().unwrap()["thread"]["id"]
            .as_str()
            .unwrap();

        let seed = response_for_line_with_state(
            &state,
            &json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "turn/run-agentic",
                "params": {
                    "threadId": thread_id,
                    "input": "seed todos",
                    "modelSteps": [{
                        "toolCalls": [{
                            "id": "todo-seed",
                            "name": "todo_write",
                            "arguments": {
                                "summary": "Seeded todos",
                                "todos": [
                                    { "id": "impl", "content": "Implement todo actions", "status": "in_progress", "priority": "high" },
                                    { "id": "verify", "content": "Verify todo actions", "status": "pending", "priority": "medium" }
                                ]
                            }
                        }],
                        "finalResponse": true
                    }]
                }
            })
            .to_string(),
        );
        assert!(seed.error.is_none(), "{:?}", seed.error);

        let done = response_for_line_with_state(
            &state,
            &json!({
                "jsonrpc": "2.0",
                "id": 3,
                "method": "todos/client-action",
                "params": { "threadId": thread_id, "action": "done", "id": "impl" }
            })
            .to_string(),
        );
        assert!(done.error.is_none(), "{:?}", done.error);
        assert_eq!(
            done.result.as_ref().unwrap()["state"]["todos"][0]["status"],
            json!("completed")
        );
        assert_eq!(
            done.result.as_ref().unwrap()["state"]["todos"][1]["status"],
            json!("pending")
        );
        assert!(
            done.result.as_ref().unwrap()["events"]
                .as_array()
                .unwrap()
                .iter()
                .any(|event| event["kind"]["type"] == json!("todosUpdated"))
        );

        let clear = response_for_line_with_state(
            &state,
            &json!({
                "jsonrpc": "2.0",
                "id": 4,
                "method": "todos/client-action",
                "params": { "threadId": thread_id, "action": "clear" }
            })
            .to_string(),
        );
        assert!(clear.error.is_none(), "{:?}", clear.error);
        assert_eq!(clear.result.as_ref().unwrap()["state"]["todos"], json!([]));
        assert!(
            clear.result.as_ref().unwrap()["events"]
                .as_array()
                .unwrap()
                .iter()
                .any(|event| event["kind"]["type"] == json!("todosUpdated"))
        );
    }

    #[test]
    fn malformed_json_lines_return_parse_errors_without_mutation() {
        let mut runtime = Runtime::new();
        let malformed_lines = [
            "{",
            "not json",
            "[]",
            "null",
            "{\"jsonrpc\":\"2.0\",\"id\":1}",
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":42}",
        ];

        for line in malformed_lines {
            let response = response_for_line(&mut runtime, line);
            assert_eq!(response.id, Value::Null, "line: {line}");
            assert_eq!(
                response.error.as_ref().unwrap().code,
                -32700,
                "line: {line}"
            );
            serde_json::to_string(&response).expect("response remains JSON serializable");
        }

        let follow_up = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(1),
                method: "thread/start".to_string(),
                params: json!({ "project": { "id": "project", "cwd": "/repo" } }),
            },
        );
        assert!(follow_up.error.is_none(), "{:?}", follow_up.error);
        assert_eq!(follow_up.result.unwrap()["thread"]["id"], json!("thread-1"));
    }

    #[test]
    fn malformed_protocol_params_do_not_panic_or_mutate_runtime() {
        let mut runtime = Runtime::new();
        let malformed_params = [
            json!(null),
            json!([]),
            json!(42),
            json!("string"),
            json!({}),
            json!({ "threadId": 42, "turnId": [], "project": { "id": null } }),
            json!({ "threadId": "missing", "turnId": "missing", "input": { "nested": true } }),
        ];
        let methods = [
            "initialize",
            "thread/start",
            "thread/resume",
            "thread/fork",
            "turn/start",
            "turn/steer",
            "turn/interrupt",
            "tool/record",
            "tool/batch",
            "approval/request",
            "approval/respond",
            "question/request",
            "question/respond",
            "plugin/register",
            "memory/set",
            "memory/control",
            "memory/compact",
            "mcp/register",
            "mcp/action",
            "model/register",
            "model/select",
            "agents/register",
            "agents/dispatch",
            "agents/block",
            "agents/complete",
            "side-question/ask",
            "pi/bridge-event",
            "events/list",
            "sandbox/plan",
        ];

        let mut id = 0;
        for method in methods {
            for params in &malformed_params {
                id += 1;
                let response = handle_request(
                    &mut runtime,
                    JsonRpcRequest {
                        jsonrpc: Some("2.0".to_string()),
                        id: json!(id),
                        method: method.to_string(),
                        params: params.clone(),
                    },
                );
                assert!(
                    response.result.is_none(),
                    "{method} unexpectedly succeeded for {params}"
                );
                assert!(
                    response.error.is_some(),
                    "{method} returned neither result nor error"
                );
                serde_json::to_string(&response).expect("response remains JSON serializable");
            }
        }

        let follow_up = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!("after-fuzz"),
                method: "thread/start".to_string(),
                params: json!({ "project": { "id": "project", "cwd": "/repo" } }),
            },
        );
        assert!(follow_up.error.is_none(), "{:?}", follow_up.error);
        assert_eq!(follow_up.result.unwrap()["thread"]["id"], json!("thread-1"));
    }

    #[test]
    fn panic_boundary_returns_internal_error_and_keeps_server_usable() {
        let mut runtime = Runtime::new();
        let panic_response = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!("panic-id"),
                method: "__test/panic".to_string(),
                params: json!({}),
            },
        );
        let error = panic_response.error.expect("panic response error");
        assert_eq!(panic_response.id, json!("panic-id"));
        assert_eq!(error.code, -32603);
        assert_eq!(error.data.unwrap()["code"], json!("internal_panic"));

        let follow_up = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(2),
                method: "thread/start".to_string(),
                params: json!({ "project": { "id": "project", "cwd": "/repo" } }),
            },
        );
        assert!(follow_up.error.is_none(), "{:?}", follow_up.error);
        assert_eq!(follow_up.result.unwrap()["thread"]["id"], json!("thread-1"));
    }

    #[test]
    fn invalid_params_return_json_rpc_invalid_params_without_mutation() {
        let mut runtime = Runtime::new();
        let bad = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(1),
                method: "thread/start".to_string(),
                params: json!({ "project": { "id": 42 } }),
            },
        );
        assert_eq!(bad.error.as_ref().unwrap().code, -32602);

        let good = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(2),
                method: "thread/start".to_string(),
                params: json!({ "project": { "id": "project", "cwd": "/repo" } }),
            },
        );
        assert!(good.error.is_none(), "{:?}", good.error);
        assert_eq!(good.result.unwrap()["thread"]["id"], json!("thread-1"));
    }

    #[test]
    fn returns_sandbox_plan_over_json_rpc() {
        let mut runtime = Runtime::new();
        let response = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(1),
                method: "sandbox/plan".to_string(),
                params: json!({
                    "preference": "auto",
                    "policy": {
                        "permissionProfile": {
                            "mode": "default",
                            "writableRoots": ["/repo"],
                            "protectedPatterns": []
                        },
                        "network": "disabled",
                        "filesystem": "workspaceWrite"
                    },
                    "request": {
                        "command": "echo hi",
                        "cwd": "/repo",
                        "writesFiles": false,
                        "usesNetwork": false
                    }
                }),
            },
        );
        assert!(response.error.is_none(), "{:?}", response.error);
        let result = response.result.unwrap();
        assert_eq!(result["decision"]["type"], json!("allow"));
        assert!(
            matches!(
                result["plan"]["enforcement"].as_str(),
                Some("reviewOnly" | "osSandbox")
            ),
            "unexpected enforcement: {}",
            result["plan"]["enforcement"]
        );
    }

    #[test]
    fn sandbox_exec_requires_configured_auth_token() {
        let mut runtime = Runtime::new();
        let response = handle_request_with_auth(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(1),
                method: "sandbox/exec".to_string(),
                params: json!({}),
            },
            None,
        );
        assert_eq!(response.error.unwrap().code, -32001);
    }

    #[test]
    fn windows_wfp_install_requires_configured_auth_token() {
        let mut runtime = Runtime::new();
        let response = handle_request_with_auth(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(1),
                method: "sandbox/windows-wfp-install".to_string(),
                params: json!({ "account": "OPPiSandbox" }),
            },
            None,
        );
        assert_eq!(response.error.unwrap().code, -32001);
    }

    #[test]
    fn windows_wfp_status_returns_json_shape() {
        let mut runtime = Runtime::new();
        let response = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(1),
                method: "sandbox/windows-wfp-status".to_string(),
                params: json!({}),
            },
        );
        assert!(response.error.is_none(), "{:?}", response.error);
        let result = response.result.unwrap();
        assert!(result.get("available").is_some());
        assert!(result.get("filterCount").is_some());
        assert!(result.get("message").is_some());
    }

    #[test]
    fn sandbox_exec_returns_policy_denial_over_json_rpc() {
        let mut runtime = Runtime::new();
        let response = handle_request_with_auth(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(1),
                method: "sandbox/exec".to_string(),
                params: json!({
                    "authToken": "secret",
                    "preference": "auto",
                    "policy": {
                        "permissionProfile": {
                            "mode": "read-only",
                            "writableRoots": ["/repo"],
                            "protectedPatterns": []
                        },
                        "network": "ask",
                        "filesystem": "readOnly"
                    },
                    "request": {
                        "command": "echo hi > file",
                        "cwd": "/repo",
                        "writesFiles": true,
                        "usesNetwork": false
                    }
                }),
            },
            Some("secret"),
        );
        assert!(response.error.is_none(), "{:?}", response.error);
        assert_eq!(response.result.unwrap()["decision"]["type"], json!("deny"));
    }

    #[test]
    fn records_parallel_tool_batch_over_json_rpc() {
        let mut runtime = Runtime::new();
        let start = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(1),
                method: "thread/start".to_string(),
                params: json!({ "project": { "id": "project", "cwd": "/repo" } }),
            },
        );
        assert!(start.error.is_none());
        let turn = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(2),
                method: "turn/start".to_string(),
                params: json!({ "threadId": "thread-1", "input": "inspect", "deferCompletion": true }),
            },
        );
        assert!(turn.error.is_none());
        let batch = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(3),
                method: "tool/batch".to_string(),
                params: json!({
                    "threadId": "thread-1",
                    "turnId": "turn-1",
                    "maxConcurrency": 10,
                    "tools": [
                        {
                            "concurrencySafe": true,
                            "call": { "id": "read-1", "name": "read", "arguments": { "path": "README.md" } },
                            "result": { "callId": "read-1", "status": "ok", "output": "read" }
                        },
                        {
                            "concurrencySafe": true,
                            "call": { "id": "grep-1", "name": "grep", "arguments": { "pattern": "OPPi" } },
                            "result": { "callId": "grep-1", "status": "ok", "output": "grep" }
                        },
                        {
                            "call": { "id": "edit-1", "name": "edit", "arguments": {} },
                            "result": { "callId": "edit-1", "status": "ok", "output": "edited" }
                        }
                    ]
                }),
            },
        );
        assert!(batch.error.is_none(), "{:?}", batch.error);
        let result = batch.result.unwrap();
        assert_eq!(result["batches"].as_array().unwrap().len(), 2);
        assert_eq!(result["batches"][0]["execution"], json!("concurrent"));
        assert_eq!(result["batches"][1]["execution"], json!("exclusive"));
    }

    #[test]
    fn lists_skills_over_json_rpc() {
        let mut runtime = Runtime::new();
        let start = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(1),
                method: "thread/start".to_string(),
                params: json!({ "project": { "id": "project", "cwd": "/repo" } }),
            },
        );
        assert!(start.error.is_none());
        let response = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(2),
                method: "skills/list".to_string(),
                params: json!({ "threadId": "thread-1" }),
            },
        );
        assert!(response.error.is_none());
        let result = response.result.unwrap();
        assert!(result["items"].as_array().unwrap().iter().any(|item| {
            item["active"]["name"] == json!("imagegen")
                && item["active"]["source"] == json!("builtIn")
        }));
    }

    #[test]
    fn dispatches_agent_over_json_rpc() {
        let mut runtime = Runtime::new();
        let start = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(1),
                method: "thread/start".to_string(),
                params: json!({ "project": { "id": "project", "cwd": "/repo" } }),
            },
        );
        assert!(start.error.is_none());
        let register = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(2),
                method: "agents/register".to_string(),
                params: json!({
                    "threadId": "thread-1",
                    "agent": {
                        "name": "reviewer",
                        "description": "review code",
                        "instructions": "Review carefully.",
                        "background": true,
                        "worktreeRoot": "/repo-wt"
                    }
                }),
            },
        );
        assert!(register.error.is_none());
        let dispatch = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(3),
                method: "agents/dispatch".to_string(),
                params: json!({ "threadId": "thread-1", "agentName": "reviewer", "task": "audit", "role": "subagent", "model": "gpt-sub", "effort": "high" }),
            },
        );
        assert!(dispatch.error.is_none());
        let result = dispatch.result.unwrap();
        assert_eq!(result["run"]["worktreeRoot"], json!("/repo-wt"));
        assert_eq!(result["run"]["role"], json!("subagent"));
        assert_eq!(result["run"]["model"], json!("gpt-sub"));
        assert_eq!(result["run"]["effort"], json!("high"));
    }
}
