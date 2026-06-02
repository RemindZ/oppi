#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    static TEST_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    struct EnvVarGuard {
        key: &'static str,
        previous: Option<std::ffi::OsString>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
            let previous = std::env::var_os(key);
            unsafe { std::env::set_var(key, value) };
            Self { key, previous }
        }

        fn remove(key: &'static str) -> Self {
            let previous = std::env::var_os(key);
            unsafe { std::env::remove_var(key) };
            Self { key, previous }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => unsafe { std::env::set_var(self.key, value) },
                None => unsafe { std::env::remove_var(self.key) },
            }
        }
    }

    fn fake_codex_access_token(account_id: &str) -> String {
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"none"}"#);
        let payload = URL_SAFE_NO_PAD.encode(
            format!(r#"{{"https://api.openai.com/auth":{{"chatgpt_account_id":"{account_id}"}}}}"#)
                .as_bytes(),
        );
        format!("{header}.{payload}.sig")
    }

    fn project() -> ProjectRef {
        ProjectRef {
            id: "project-1".to_string(),
            cwd: "/repo".to_string(),
            display_name: Some("repo".to_string()),
            workspace_roots: vec![WorkspaceRoot {
                path: "/repo".to_string(),
                label: None,
                git_remote: None,
            }],
        }
    }

    fn thread(runtime: &mut Runtime) -> Thread {
        runtime
            .start_thread(ThreadStartParams {
                project: project(),
                title: Some("main".to_string()),
            })
            .thread
    }

    #[derive(Debug)]
    struct MockMemoryBackendRunner {
        recall_context: String,
    }

    impl Default for MockMemoryBackendRunner {
        fn default() -> Self {
            Self {
                recall_context: "Mock memory context".to_string(),
            }
        }
    }

    impl MockMemoryBackendRunner {
        fn recall(context: impl Into<String>) -> Self {
            Self {
                recall_context: context.into(),
            }
        }
    }

    impl MemoryBackendRunner for MockMemoryBackendRunner {
        fn adapter_info(&self) -> MemoryBackendAdapterInfo {
            MemoryBackendAdapterInfo {
                id: "mock-hoppi".to_string(),
                kind: "mock-rust".to_string(),
                embedded: true,
                operations: MemoryBackendOperation::ALL.to_vec(),
                diagnostics: Vec::new(),
            }
        }

        fn status(
            &self,
            _thread: &Thread,
            _params: &MemoryBackendStatusParams,
        ) -> Result<MemoryBackendRawResult, RuntimeError> {
            Ok(MemoryBackendRawResult {
                backend: "mock-hoppi".to_string(),
                adapter_kind: Some("mock-rust".to_string()),
                value: json!({
                    "status": {
                        "enabled": true,
                        "initialized": true,
                        "backend": "hoppi",
                        "scope": "project",
                        "memoryCount": 2,
                        "pinnedCount": 1,
                        "dashboardUrl": "http://hoppi.localhost:49152"
                    }
                }),
            })
        }

        fn recall(
            &self,
            _thread: &Thread,
            _params: &MemoryBackendRecallParams,
        ) -> Result<MemoryBackendRawResult, RuntimeError> {
            Ok(MemoryBackendRawResult {
                backend: "mock-hoppi".to_string(),
                adapter_kind: Some("mock-rust".to_string()),
                value: json!({
                    "contextMarkdown": self.recall_context,
                    "records": []
                }),
            })
        }

        fn remember(
            &self,
            _thread: &Thread,
            params: &MemoryBackendRememberParams,
        ) -> Result<MemoryBackendRawResult, RuntimeError> {
            Ok(MemoryBackendRawResult {
                backend: "mock-hoppi".to_string(),
                adapter_kind: Some("mock-rust".to_string()),
                value: json!({
                    "saved": true,
                    "record": {
                        "id": "mem-remember",
                        "content": params.content,
                        "tags": params.tags,
                        "layer": params.layer,
                        "confidence": "observed",
                        "source": params.source
                    },
                    "status": {
                        "enabled": true,
                        "initialized": true,
                        "backend": "hoppi",
                        "scope": "project",
                        "memoryCount": 1,
                        "pinnedCount": 0
                    }
                }),
            })
        }

        fn list(
            &self,
            _thread: &Thread,
            _params: &MemoryBackendListParams,
        ) -> Result<MemoryBackendRawResult, RuntimeError> {
            Ok(MemoryBackendRawResult {
                backend: "mock-hoppi".to_string(),
                adapter_kind: Some("mock-rust".to_string()),
                value: json!({
                    "records": [
                        {
                            "id": "mem-list",
                            "content": "Backend route list item.",
                            "pinned": true
                        },
                        {
                            "id": "mem-other",
                            "content": "Other memory."
                        }
                    ],
                    "status": {
                        "enabled": true,
                        "initialized": true,
                        "backend": "hoppi",
                        "scope": "project",
                        "memoryCount": 2,
                        "pinnedCount": 1
                    }
                }),
            })
        }

        fn dashboard(
            &self,
            _thread: &Thread,
            _params: &MemoryBackendDashboardParams,
        ) -> Result<MemoryBackendRawResult, RuntimeError> {
            self.status(
                _thread,
                &MemoryBackendStatusParams {
                    thread_id: _params.thread_id.clone(),
                },
            )
        }

        fn forget(
            &self,
            _thread: &Thread,
            params: &MemoryBackendForgetParams,
        ) -> Result<MemoryBackendRawResult, RuntimeError> {
            Ok(MemoryBackendRawResult {
                backend: "mock-hoppi".to_string(),
                adapter_kind: Some("mock-rust".to_string()),
                value: json!({
                    "id": params.id,
                    "memoryCount": 1,
                    "pinnedCount": 0
                }),
            })
        }

        fn pin(
            &self,
            _thread: &Thread,
            params: &MemoryBackendPinParams,
        ) -> Result<MemoryBackendRawResult, RuntimeError> {
            Ok(MemoryBackendRawResult {
                backend: "mock-hoppi".to_string(),
                adapter_kind: Some("mock-rust".to_string()),
                value: json!({
                    "id": params.id,
                    "record": {
                        "id": params.id,
                        "content": "Pinned by backend route.",
                        "pinned": params.pinned
                    },
                    "memoryCount": 1,
                    "pinnedCount": if params.pinned { 1 } else { 0 }
                }),
            })
        }

        fn update(
            &self,
            _thread: &Thread,
            params: &MemoryBackendUpdateParams,
        ) -> Result<MemoryBackendRawResult, RuntimeError> {
            Ok(MemoryBackendRawResult {
                backend: "mock-hoppi".to_string(),
                adapter_kind: Some("mock-rust".to_string()),
                value: json!({
                    "id": params.id,
                    "record": {
                        "id": params.id,
                        "content": params.patch.content.clone().unwrap_or_else(|| "Edited by backend route.".to_string()),
                        "tags": params.patch.tags.clone().unwrap_or_default()
                    },
                    "memoryCount": 1,
                    "pinnedCount": 0
                }),
            })
        }

        fn supersede(
            &self,
            _thread: &Thread,
            params: &MemoryBackendSupersedeParams,
        ) -> Result<MemoryBackendRawResult, RuntimeError> {
            Ok(MemoryBackendRawResult {
                backend: "mock-hoppi".to_string(),
                adapter_kind: Some("mock-rust".to_string()),
                value: json!({
                    "id": params.id,
                    "record": {
                        "id": params.id,
                        "content": "Edited by backend route.",
                        "supersededBy": params.superseded_by
                    },
                    "memoryCount": 1,
                    "pinnedCount": 0
                }),
            })
        }

        fn consolidate(
            &self,
            _thread: &Thread,
            params: &MemoryBackendConsolidateParams,
        ) -> Result<MemoryBackendRawResult, RuntimeError> {
            Ok(MemoryBackendRawResult {
                backend: "mock-hoppi".to_string(),
                adapter_kind: Some("mock-rust".to_string()),
                value: json!({
                    "dryRun": params.dry_run,
                    "summary": "Merged backend route memories.",
                    "modelUsed": "gpt-5.5",
                    "records": [
                        {
                            "id": "mem-consolidated",
                            "content": "Consolidated backend route memory."
                        }
                    ],
                    "deletedIds": ["mem-noise"],
                    "status": {
                        "enabled": true,
                        "initialized": true,
                        "backend": "hoppi",
                        "scope": "project",
                        "memoryCount": 1,
                        "pinnedCount": 0,
                        "queueDepth": 0
                    }
                }),
            })
        }

        fn sync(
            &self,
            _thread: &Thread,
            params: &MemoryBackendSyncParams,
        ) -> Result<MemoryBackendRawResult, RuntimeError> {
            Ok(MemoryBackendRawResult {
                backend: "mock-hoppi".to_string(),
                adapter_kind: Some("mock-rust".to_string()),
                value: json!({
                    "imported": 2,
                    "exported": 1,
                    "committed": true,
                    "pushed": params.direction != MemoryBackendSyncDirection::Pull,
                    "direction": params.direction
                }),
            })
        }
    }

    fn assert_memory_backend_job(
        runtime: &mut Runtime,
        thread_id: &str,
        job_id: &str,
        kind: MemoryJobKind,
        stage: &str,
        record_count: u64,
        deleted_count: u64,
    ) {
        let jobs = runtime
            .list_memory_jobs(MemoryJobsListParams {
                thread_id: Some(thread_id.to_string()),
                include_finished: true,
                limit: Some(50),
            })
            .unwrap();
        assert_eq!(jobs.queue_depth, 0);
        assert!(jobs.items.iter().any(|job| {
            job.id == job_id
                && job.kind == kind
                && job.status == MemoryJobStatus::Succeeded
                && job.stage.as_deref() == Some(stage)
                && job.source.as_deref() == Some("memory-backend")
                && job.record_count == record_count
                && job.deleted_count == deleted_count
                && job.backend_applied
        }));
    }

    fn temp_project_root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "oppi-{name}-{}-{}",
            std::process::id(),
            now_millis()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn thread_with_cwd(runtime: &mut Runtime, cwd: &Path) -> Thread {
        runtime
            .start_thread(ThreadStartParams {
                project: ProjectRef {
                    id: "mcp-project".to_string(),
                    cwd: cwd.display().to_string(),
                    display_name: Some("mcp-project".to_string()),
                    workspace_roots: Vec::new(),
                },
                title: Some("mcp".to_string()),
            })
            .thread
    }

    fn write_project_plugin_store(root: &Path, content: Value) {
        fs::create_dir_all(root.join(".oppi")).unwrap();
        fs::write(
            root.join(".oppi/plugins.json"),
            serde_json::to_string_pretty(&content).unwrap(),
        )
        .unwrap();
    }

    fn write_project_plugin_runner_store(root: &Path, content: Value) {
        fs::create_dir_all(root.join(".oppi")).unwrap();
        fs::write(
            root.join(".oppi/plugin-runners.json"),
            serde_json::to_string_pretty(&content).unwrap(),
        )
        .unwrap();
    }

    fn mock_plugin_runner_sequence(values: Vec<Value>) -> Vec<Value> {
        values
    }

    fn plugin_runner_initialize_result(capabilities: Vec<&str>) -> Value {
        json!({
            "serverInfo": {
                "name": "mock-plugin-runner",
                "version": "1.0.0"
            },
            "capabilities": capabilities
                .into_iter()
                .map(|capability| (capability.to_string(), json!({})))
                .collect::<serde_json::Map<_, _>>()
        })
    }

    fn plugin_runner_tool(name: &str, input_schema: Value, read_only: bool) -> Value {
        json!({
            "name": name,
            "description": format!("Mock plugin runner tool {name}"),
            "inputSchema": input_schema,
            "readOnly": read_only
        })
    }

    fn plugin_runner_tools_list_result(tools: Vec<Value>) -> Value {
        json!({ "tools": tools })
    }

    fn agentic_call(id: &str, output: &str) -> ToolCall {
        ToolCall {
            id: id.to_string(),
            name: "echo".to_string(),
            namespace: Some("oppi".to_string()),
            arguments: json!({ "output": output }),
        }
    }

    fn agent_tool_call(id: &str, arguments: Value) -> ToolCall {
        ToolCall {
            id: id.to_string(),
            name: "AgentTool".to_string(),
            namespace: Some("oppi".to_string()),
            arguments,
        }
    }

    fn non_approval_write_file_definition() -> ToolDefinition {
        let mut definition = default_tool_registry()
            .get(Some("oppi"), "write_file")
            .expect("write_file should be registered")
            .clone();
        definition.requires_approval = false;
        definition
    }

    fn completed_tool_output_json(result: &AgenticTurnResult, call_id: &str) -> Value {
        let output = result
            .events
            .iter()
            .find_map(|event| match &event.kind {
                EventKind::ToolCallCompleted { result }
                    if result.call_id == call_id && result.output.is_some() =>
                {
                    result.output.as_deref()
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("missing output for tool call {call_id}"));
        serde_json::from_str(output).unwrap_or_else(|error| {
            panic!("tool call {call_id} output should be JSON: {error}: {output}")
        })
    }

    fn tool(id: &str, name: &str, concurrency_safe: bool) -> SimulatedToolUse {
        SimulatedToolUse {
            call: ToolCall {
                id: id.to_string(),
                name: name.to_string(),
                namespace: Some("functions".to_string()),
                arguments: json!({}),
            },
            result: ToolResult {
                call_id: id.to_string(),
                status: ToolResultStatus::Ok,
                output: Some("ok".to_string()),
                error: None,
            },
            require_approval: false,
            concurrency_safe,
        }
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

    fn assistant_text_from_events(events: &[Event]) -> String {
        events
            .iter()
            .filter_map(|event| match &event.kind {
                EventKind::ItemDelta { delta, .. } => Some(delta.as_str()),
                EventKind::ItemCompleted {
                    item:
                        Item {
                            kind: ItemKind::AssistantMessage { text },
                            ..
                        },
                } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("")
    }

    fn phase_trace_from_events(events: &[Event]) -> Vec<TurnPhase> {
        events
            .iter()
            .filter_map(|event| match &event.kind {
                EventKind::TurnStarted { turn } => Some(turn.phase),
                EventKind::TurnPhaseChanged { phase } => Some(*phase),
                _ => None,
            })
            .collect()
    }

    fn start_mock_openai_server(
        responses: Vec<String>,
    ) -> (String, std::thread::JoinHandle<Vec<String>>) {
        use std::io::Write;
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

    fn start_mock_http_byte_server(
        responses: Vec<(String, Vec<u8>)>,
    ) -> (String, std::thread::JoinHandle<Vec<String>>) {
        use std::io::Write;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            let mut requests = Vec::new();
            for (content_type, response) in responses {
                let deadline = Instant::now() + Duration::from_secs(2);
                let stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break Some(stream),
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            if Instant::now() >= deadline {
                                break None;
                            }
                            std::thread::sleep(Duration::from_millis(10));
                        }
                        Err(_) => break None,
                    }
                };
                let Some(mut stream) = stream else {
                    break;
                };
                requests.push(read_http_request(&mut stream));
                let header = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                    response.len()
                );
                stream.write_all(header.as_bytes()).unwrap();
                stream.write_all(&response).unwrap();
            }
            requests
        });
        (format!("http://{address}"), handle)
    }

    struct MockMcpHttpResponse {
        status: &'static str,
        headers: Vec<(&'static str, String)>,
        body: String,
    }

    impl MockMcpHttpResponse {
        fn json(body: Value) -> Self {
            Self {
                status: "200 OK",
                headers: vec![("content-type", "application/json".to_string())],
                body: body.to_string(),
            }
        }

        fn accepted() -> Self {
            Self {
                status: "202 Accepted",
                headers: Vec::new(),
                body: String::new(),
            }
        }

        fn with_header(mut self, key: &'static str, value: impl Into<String>) -> Self {
            self.headers.push((key, value.into()));
            self
        }
    }

    fn start_mock_mcp_http_server(
        responses: Vec<MockMcpHttpResponse>,
    ) -> (String, std::thread::JoinHandle<Vec<String>>) {
        use std::io::Write;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            let mut requests = Vec::new();
            for response in responses {
                let deadline = Instant::now() + Duration::from_secs(2);
                let stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break Some(stream),
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            if Instant::now() >= deadline {
                                break None;
                            }
                            std::thread::sleep(Duration::from_millis(10));
                        }
                        Err(_) => break None,
                    }
                };
                let Some(mut stream) = stream else {
                    break;
                };
                requests.push(read_http_request(&mut stream));
                let mut header = format!("HTTP/1.1 {}\r\n", response.status);
                for (key, value) in response.headers {
                    header.push_str(key);
                    header.push_str(": ");
                    header.push_str(&value);
                    header.push_str("\r\n");
                }
                header.push_str(&format!(
                    "content-length: {}\r\nconnection: close\r\n\r\n",
                    response.body.len()
                ));
                stream.write_all(header.as_bytes()).unwrap();
                stream.write_all(response.body.as_bytes()).unwrap();
            }
            requests
        });
        (format!("http://{address}/mcp"), handle)
    }

    #[test]
    fn command_prepare_expands_review_and_independent_prompts() {
        let runtime = Runtime::new();
        let review = runtime
            .prepare_command(CommandPrepareParams {
                thread_id: None,
                command: "/review".to_string(),
                args: String::new(),
                context: json!({ "mode": "base-branch", "baseBranch": "main", "mergeBaseSha": "abc123" }),
                prompt_variant_append: None,
            })
            .unwrap();
        assert_eq!(review.command, "review");
        assert_eq!(review.system_prompt_profile.as_deref(), Some("review"));
        assert!(review.input.contains("git diff abc123"));

        let independent = runtime
            .prepare_command(CommandPrepareParams {
                thread_id: None,
                command: "independent".to_string(),
                args: "@.oppi-plans/99-slashcommand-parity.md".to_string(),
                context: json!({}),
                prompt_variant_append: Some("Variant append".to_string()),
            })
            .unwrap();
        assert!(independent.input.contains("Use the independent skill"));
        assert!(independent.input.contains("Variant append"));
    }

    #[test]
    fn command_prepare_expands_feedback_and_init_prompts() {
        let runtime = Runtime::new();
        let feedback = runtime
            .prepare_command(CommandPrepareParams {
                thread_id: None,
                command: "/bug-report".to_string(),
                args: "The footer flickers".to_string(),
                context: json!({}),
                prompt_variant_append: None,
            })
            .unwrap();
        assert!(feedback.input.contains("The footer flickers"));
        assert!(feedback.input.contains("oppi_feedback_submit"));

        let init = runtime
            .prepare_command(CommandPrepareParams {
                thread_id: None,
                command: "init".to_string(),
                args: String::new(),
                context: json!({ "existingAgentsMd": "# Old guidance" }),
                prompt_variant_append: None,
            })
            .unwrap();
        assert!(init.input.contains("Generate a file named AGENTS.md"));
        assert!(init.input.contains("# Old guidance"));
    }

    #[test]
    fn starts_thread_and_records_event() {
        let mut runtime = Runtime::new();
        let result = runtime.start_thread(ThreadStartParams {
            project: project(),
            title: Some("main".to_string()),
        });
        assert_eq!(result.thread.id, "thread-1");
        assert_eq!(result.events.len(), 1);
        assert!(matches!(
            result.events[0].kind,
            EventKind::ThreadStarted { .. }
        ));
    }

    #[test]
    fn start_turn_emits_all_runtime_phases_assistant_and_completion() {
        let mut runtime = Runtime::new();
        let thread = thread(&mut runtime);
        let result = runtime
            .start_turn(TurnStartParams {
                thread_id: thread.id.clone(),
                input: "hello".to_string(),
                assistant_response: Some("hi".to_string()),
                simulated_tool: None,
                requested_continuations: 0,
                stop_hook_feedback: None,
                defer_completion: false,
            })
            .unwrap();
        assert_eq!(result.turn.status, TurnStatus::Completed);
        assert_eq!(result.turn.phase, TurnPhase::Await);
        assert!(result.events.iter().any(|event| matches!(
            event.kind,
            EventKind::TurnPhaseChanged {
                phase: TurnPhase::Tools
            }
        )));
        assert!(result.events.iter().any(|event| matches!(
            event.kind,
            EventKind::ItemCompleted {
                item: Item {
                    kind: ItemKind::AssistantMessage { .. },
                    ..
                }
            }
        )));
        assert!(
            result
                .events
                .iter()
                .any(|event| matches!(event.kind, EventKind::TurnCompleted { .. }))
        );
    }

    #[test]
    fn agentic_turn_streams_calls_tool_continues_and_completes() {
        let mut runtime = Runtime::new();
        let thread = thread(&mut runtime);
        let result = runtime
            .run_agentic_turn(AgenticTurnParams {
                thread_id: thread.id,
                input: "use a tool".to_string(),
                execution_mode: ExecutionMode::Blocking,
                follow_up: None,
                sandbox_policy: None,
                model_steps: vec![
                    ScriptedModelStep {
                        assistant_deltas: vec!["I will ".to_string(), "call echo.".to_string()],
                        tool_calls: vec![agentic_call("echo-1", "tool output")],
                        tool_results: Vec::new(),
                        final_response: false,
                    },
                    ScriptedModelStep {
                        assistant_deltas: vec!["Tool said: tool output".to_string()],
                        tool_calls: Vec::new(),
                        tool_results: Vec::new(),
                        final_response: true,
                    },
                ],
                model_provider: None,
                tool_definitions: Vec::new(),
                approved_tool_call_ids: Vec::new(),
                cancellation: None,
                max_continuations: Some(4),
            })
            .unwrap();

        assert_eq!(result.turn.status, TurnStatus::Completed);
        assert_eq!(result.turn.phase, TurnPhase::Await);
        assert!(
            result
                .events
                .iter()
                .any(|event| matches!(event.kind, EventKind::ItemDelta { .. }))
        );
        assert!(result.events.iter().any(|event| matches!(
            event.kind,
            EventKind::ToolCallCompleted {
                result: ToolResult {
                    status: ToolResultStatus::Ok,
                    ..
                }
            }
        )));
        let phases: Vec<_> = result
            .events
            .iter()
            .filter_map(|event| match event.kind {
                EventKind::TurnPhaseChanged { phase } => Some(phase),
                _ => None,
            })
            .collect();
        assert_eq!(phases.first(), Some(&TurnPhase::Message));
        assert_eq!(phases.last(), Some(&TurnPhase::Await));
        assert!(
            phases
                .windows(2)
                .all(|pair| valid_agentic_transition(pair[0], pair[1]))
        );
    }

    #[test]
    fn agentic_turn_emits_complete_11_step_workflow_contract() {
        let mut runtime = Runtime::new();
        let thread = thread(&mut runtime);
        let result = runtime
            .run_agentic_turn(AgenticTurnParams {
                thread_id: thread.id,
                input: "prove the full workflow".to_string(),
                execution_mode: ExecutionMode::Blocking,
                follow_up: None,
                sandbox_policy: None,
                model_steps: vec![
                    ScriptedModelStep {
                        assistant_deltas: vec!["Checking ".to_string(), "with a tool.".to_string()],
                        tool_calls: vec![agentic_call("contract-echo", "contract output")],
                        tool_results: Vec::new(),
                        final_response: false,
                    },
                    ScriptedModelStep {
                        assistant_deltas: vec!["Done after tool.".to_string()],
                        tool_calls: Vec::new(),
                        tool_results: Vec::new(),
                        final_response: true,
                    },
                ],
                model_provider: None,
                tool_definitions: Vec::new(),
                approved_tool_call_ids: Vec::new(),
                cancellation: None,
                max_continuations: Some(4),
            })
            .unwrap();

        assert_eq!(result.turn.status, TurnStatus::Completed);
        assert_eq!(
            phase_trace_from_events(&result.events),
            vec![
                TurnPhase::Input,
                TurnPhase::Message,
                TurnPhase::History,
                TurnPhase::System,
                TurnPhase::Api,
                TurnPhase::Tokens,
                TurnPhase::Tools,
                TurnPhase::Loop,
                TurnPhase::Api,
                TurnPhase::Tokens,
                TurnPhase::Tools,
                TurnPhase::Loop,
                TurnPhase::Render,
                TurnPhase::Hooks,
                TurnPhase::Await,
            ]
        );
        assert!(
            result
                .events
                .iter()
                .any(|event| matches!(event.kind, EventKind::ItemDelta { .. })),
            "Tokens phase must surface streamed assistant deltas"
        );
        assert_eq!(
            result
                .events
                .iter()
                .filter(|event| matches!(
                    &event.kind,
                    EventKind::ToolCallCompleted { result }
                        if result.call_id == "contract-echo"
                ))
                .count(),
            1,
            "Tools phase must produce exactly one result for the model tool call"
        );
        assert!(
            result.events.iter().any(|event| matches!(
                &event.kind,
                EventKind::Diagnostic { diagnostic }
                    if diagnostic.message == "agentic turn reached final response"
                        && diagnostic.metadata.get("hook").map(String::as_str) == Some("stop")
            )),
            "Hooks phase must record final stop-hook diagnostics before Await"
        );
        assert!(
            result
                .events
                .iter()
                .any(|event| matches!(event.kind, EventKind::TurnCompleted { .. })),
            "Await phase must leave a completed, replayable terminal turn"
        );
    }

    #[test]
    fn openai_compatible_response_maps_to_direct_model_step() {
        let step = parse_openai_compatible_chat_response(json!({
            "choices": [{ "message": { "role": "assistant", "content": "direct hello" } }]
        }))
        .unwrap();

        assert_eq!(step.assistant_deltas, vec!["direct hello".to_string()]);
        assert!(step.final_response);
        assert!(step.tool_calls.is_empty());
    }

    #[test]
    fn openai_codex_stream_maps_text_to_direct_model_step() {
        let raw = concat!(
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Codex \"}\n\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"OK\"}\n\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"output\":[]}}\n\n",
        );
        let (step, items, chunks, tokens) =
            parse_openai_codex_stream(BufReader::new(raw.as_bytes()), &BTreeMap::new()).unwrap();

        assert_eq!(
            step.assistant_deltas,
            vec!["Codex ".to_string(), "OK".to_string()]
        );
        assert!(step.final_response);
        assert!(items.is_empty());
        assert_eq!(chunks, 3);
        assert!(tokens.is_none());
    }

    #[test]
    fn openai_codex_stream_maps_function_calls() {
        let definition = ToolDefinition {
            name: "echo".to_string(),
            namespace: Some("oppi".to_string()),
            description: Some("Echo output".to_string()),
            concurrency_safe: true,
            requires_approval: false,
            capabilities: Vec::new(),
            ..Default::default()
        };
        let tools = BTreeMap::from([(openai_tool_name(&definition), definition)]);
        let raw = "data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"function_call\",\"id\":\"fc_1\",\"call_id\":\"call_1\",\"name\":\"oppi__echo\",\"arguments\":\"{\\\"output\\\":\\\"from codex\\\"}\"}}\n\n";
        let (step, items, _chunks, _tokens) =
            parse_openai_codex_stream(BufReader::new(raw.as_bytes()), &tools).unwrap();

        assert!(!step.final_response);
        assert_eq!(step.tool_calls.len(), 1);
        assert_eq!(step.tool_calls[0].id, "call_1|fc_1");
        assert_eq!(step.tool_calls[0].namespace.as_deref(), Some("oppi"));
        assert_eq!(step.tool_calls[0].arguments["output"], json!("from codex"));
        assert_eq!(items.len(), 1);
    }

    #[test]
    fn direct_provider_parses_openai_tool_calls() {
        let definition = ToolDefinition {
            name: "echo".to_string(),
            namespace: Some("oppi".to_string()),
            description: Some("Echo output".to_string()),
            concurrency_safe: true,
            requires_approval: false,
            capabilities: Vec::new(),
            ..Default::default()
        };
        let tools = BTreeMap::from([(openai_tool_name(&definition), definition)]);
        let response = json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call-1",
                        "type": "function",
                        "function": { "name": "oppi__echo", "arguments": "{\"output\":\"from provider\"}" }
                    }]
                }
            }]
        });
        let step =
            parse_openai_compatible_message(openai_response_message(&response).unwrap(), &tools)
                .unwrap();

        assert!(!step.final_response);
        assert_eq!(step.tool_calls.len(), 1);
        assert_eq!(step.tool_calls[0].namespace.as_deref(), Some("oppi"));
        assert_eq!(step.tool_calls[0].name, "echo");
        assert_eq!(
            step.tool_calls[0].arguments["output"],
            json!("from provider")
        );
    }

    #[test]
    fn direct_provider_rejects_unknown_tool_calls() {
        let response = json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call-1",
                        "type": "function",
                        "function": { "name": "unknown_tool", "arguments": "{}" }
                    }]
                }
            }]
        });
        let error = parse_openai_compatible_message(
            openai_response_message(&response).unwrap(),
            &BTreeMap::new(),
        )
        .unwrap_err();

        assert_eq!(error.code, "provider_tool_call_unknown_tool");
        assert_eq!(error.category, RuntimeErrorCategory::Provider);
    }

    #[test]
    fn direct_provider_rejects_non_array_tool_calls() {
        let response = json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": { "id": "call-1" }
                }
            }]
        });
        let error = parse_openai_compatible_message(
            openai_response_message(&response).unwrap(),
            &BTreeMap::new(),
        )
        .unwrap_err();

        assert_eq!(error.code, "provider_tool_calls_invalid_shape");
        assert_eq!(error.category, RuntimeErrorCategory::Provider);
    }

    #[test]
    fn direct_provider_rejects_duplicate_tool_call_ids() {
        let definition = ToolDefinition {
            name: "echo".to_string(),
            namespace: Some("oppi".to_string()),
            description: Some("Echo output".to_string()),
            concurrency_safe: true,
            requires_approval: false,
            capabilities: Vec::new(),
            ..Default::default()
        };
        let tools = BTreeMap::from([(openai_tool_name(&definition), definition)]);
        let response = json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [
                        {
                            "id": "call-1",
                            "type": "function",
                            "function": { "name": "oppi__echo", "arguments": "{}" }
                        },
                        {
                            "id": "call-1",
                            "type": "function",
                            "function": { "name": "oppi__echo", "arguments": "{}" }
                        }
                    ]
                }
            }]
        });
        let error =
            parse_openai_compatible_message(openai_response_message(&response).unwrap(), &tools)
                .unwrap_err();

        assert_eq!(error.code, "provider_tool_call_duplicate_id");
        assert_eq!(error.category, RuntimeErrorCategory::Provider);
    }

    #[test]
    fn direct_provider_rejects_non_object_tool_arguments() {
        let definition = ToolDefinition {
            name: "echo".to_string(),
            namespace: Some("oppi".to_string()),
            description: Some("Echo output".to_string()),
            concurrency_safe: true,
            requires_approval: false,
            capabilities: Vec::new(),
            ..Default::default()
        };
        let tools = BTreeMap::from([(openai_tool_name(&definition), definition)]);
        let response = json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call-1",
                        "type": "function",
                        "function": { "name": "oppi__echo", "arguments": "[]" }
                    }]
                }
            }]
        });
        let error =
            parse_openai_compatible_message(openai_response_message(&response).unwrap(), &tools)
                .unwrap_err();

        assert_eq!(error.code, "provider_tool_arguments_invalid_shape");
        assert_eq!(error.category, RuntimeErrorCategory::Provider);
    }

    #[test]
    fn openai_streaming_response_maps_chunks_and_tool_calls() {
        let definition = ToolDefinition {
            name: "echo".to_string(),
            namespace: Some("oppi".to_string()),
            description: Some("Echo output".to_string()),
            concurrency_safe: true,
            requires_approval: false,
            capabilities: Vec::new(),
            ..Default::default()
        };
        let tools = BTreeMap::from([(openai_tool_name(&definition), definition)]);
        let mut stream = [
            json!({ "choices": [{ "delta": { "content": "hel" } }] }),
            json!({ "choices": [{ "delta": { "content": "lo" } }] }),
            json!({
                "choices": [{
                    "delta": {
                        "tool_calls": [{
                            "index": 0,
                            "id": "call-1",
                            "type": "function",
                            "function": {
                                "name": "oppi__echo",
                                "arguments": "{\"output\":\"",
                            },
                        }],
                    },
                }],
            }),
            json!({
                "choices": [{
                    "delta": {
                        "tool_calls": [{
                            "index": 0,
                            "function": { "arguments": "streamed\"}" },
                        }],
                    },
                }],
            }),
        ]
        .into_iter()
        .map(|chunk| format!("data: {chunk}\n\n"))
        .collect::<String>();
        stream.push_str("data: [DONE]\n\n");
        let (step, message, chunks, tokens) =
            parse_openai_compatible_stream(std::io::Cursor::new(stream), &tools).unwrap();

        assert_eq!(chunks, 4);
        assert_eq!(
            step.assistant_deltas,
            vec!["hel".to_string(), "lo".to_string()]
        );
        assert_eq!(step.tool_calls.len(), 1);
        assert_eq!(step.tool_calls[0].arguments["output"], json!("streamed"));
        assert_eq!(message["content"], json!("hello"));
        assert!(tokens.is_none());
    }

    #[test]
    fn openai_streaming_response_rejects_non_array_tool_calls() {
        let stream = format!(
            "data: {}\n\ndata: [DONE]\n\n",
            json!({ "choices": [{ "delta": { "tool_calls": { "index": 0 } } }] })
        );
        let error = parse_openai_compatible_stream(std::io::Cursor::new(stream), &BTreeMap::new())
            .unwrap_err();

        assert_eq!(error.code, "provider_tool_calls_invalid_shape");
        assert_eq!(error.category, RuntimeErrorCategory::Provider);
    }

    #[test]
    fn provider_history_and_debug_bundle_include_redacted_provider_diagnostics() {
        let mut runtime = Runtime::new();
        let thread = thread(&mut runtime);
        runtime
            .start_turn(TurnStartParams {
                thread_id: thread.id.clone(),
                input: "previous user".to_string(),
                assistant_response: Some("previous assistant".to_string()),
                simulated_tool: None,
                requested_continuations: 0,
                stop_hook_feedback: None,
                defer_completion: false,
            })
            .unwrap();
        runtime
            .compact_handoff(HandoffCompactParams {
                thread_id: thread.id.clone(),
                summary: "compact prior context".to_string(),
                details: None,
            })
            .unwrap();
        let history = runtime.compact_provider_history(&thread.id, "turn-2");
        assert!(
            history
                .iter()
                .any(|message| message.role == ProviderMessageRole::System)
        );
        assert!(
            history
                .iter()
                .any(|message| message.content == "previous user")
        );
        assert!(
            history
                .iter()
                .any(|message| message.content == "previous assistant")
        );

        let diagnostic = provider_diagnostic(
            &DirectModelProviderConfig {
                kind: DirectModelProviderKind::OpenAiCompatible,
                model: "mock-model".to_string(),
                base_url: Some("http://user:secret@127.0.0.1/v1".to_string()),
                api_key_env: Some("SECRET_ENV".to_string()),
                system_prompt: None,
                temperature: None,
                reasoning_effort: None,
                max_output_tokens: None,
                stream: true,
            },
            "http://user:secret@127.0.0.1/v1/chat/completions",
            200,
            Duration::from_millis(7),
            true,
            3,
            1,
            history.len(),
            None,
        );
        let event = runtime.event(&thread.id, None, EventKind::Diagnostic { diagnostic });
        assert!(matches!(event.kind, EventKind::Diagnostic { .. }));
        let bundle = runtime.debug_bundle(Vec::new());
        let provider = bundle
            .diagnostics
            .iter()
            .find(|diagnostic| {
                diagnostic.metadata.get("component") == Some(&"provider".to_string())
            })
            .unwrap();
        assert_eq!(
            provider.metadata.get("endpoint"),
            Some(&"http://127.0.0.1".to_string())
        );
        assert!(!format!("{:?}", provider).contains("secret"));
    }

    #[test]
    fn rust_owned_file_tools_read_search_write_and_edit() {
        let root = std::env::temp_dir().join(format!("oppi-file-tools-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("input.txt"), "alpha\nbeta\n").unwrap();
        let mut runtime = Runtime::new();
        let thread = runtime
            .start_thread(ThreadStartParams {
                project: ProjectRef {
                    id: "file-tools".to_string(),
                    cwd: root.display().to_string(),
                    display_name: Some("file-tools".to_string()),
                    workspace_roots: Vec::new(),
                },
                title: Some("file tools".to_string()),
            })
            .thread;
        let result = runtime
            .run_agentic_turn(AgenticTurnParams {
                thread_id: thread.id,
                input: "use file tools".to_string(),
                execution_mode: ExecutionMode::Blocking,
                follow_up: None,
                sandbox_policy: None,
                model_steps: vec![ScriptedModelStep {
                    assistant_deltas: Vec::new(),
                    tool_calls: vec![
                        ToolCall {
                            id: "read-1".to_string(),
                            name: "read_file".to_string(),
                            namespace: Some("oppi".to_string()),
                            arguments: json!({ "path": "input.txt" }),
                        },
                        ToolCall {
                            id: "search-1".to_string(),
                            name: "search_files".to_string(),
                            namespace: Some("oppi".to_string()),
                            arguments: json!({ "path": ".", "query": "beta" }),
                        },
                        ToolCall {
                            id: "write-1".to_string(),
                            name: "write_file".to_string(),
                            namespace: Some("oppi".to_string()),
                            arguments: json!({ "path": "output.txt", "content": "before" }),
                        },
                        ToolCall {
                            id: "edit-1".to_string(),
                            name: "edit_file".to_string(),
                            namespace: Some("oppi".to_string()),
                            arguments: json!({ "path": "output.txt", "oldText": "before", "newText": "after" }),
                        },
                    ],
                    tool_results: Vec::new(),
                    final_response: true,
                }],
                model_provider: None,
                tool_definitions: Vec::new(),
                approved_tool_call_ids: vec!["write-1".to_string(), "edit-1".to_string()],
                cancellation: None,
                max_continuations: Some(1),
            })
            .unwrap();

        assert_eq!(result.turn.status, TurnStatus::Completed);
        assert_eq!(
            fs::read_to_string(root.join("output.txt")).unwrap(),
            "after"
        );
        let outputs = result
            .events
            .iter()
            .filter_map(|event| match &event.kind {
                EventKind::ToolCallCompleted { result } => result.output.as_deref(),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(outputs.contains("alpha"));
        assert!(outputs.contains("beta"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn protected_path_policy_applies_to_file_shell_and_image_outputs() {
        let root = std::env::temp_dir().join(format!("oppi-protected-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let mut runtime = Runtime::new();
        let thread = runtime
            .start_thread(ThreadStartParams {
                project: ProjectRef {
                    id: "protected".to_string(),
                    cwd: root.display().to_string(),
                    display_name: Some("protected".to_string()),
                    workspace_roots: Vec::new(),
                },
                title: Some("protected".to_string()),
            })
            .thread;
        let policy = default_policy(PermissionProfile {
            mode: PermissionMode::FullAccess,
            readable_roots: vec![root.display().to_string()],
            writable_roots: vec![root.display().to_string()],
            filesystem_rules: Vec::new(),
            protected_patterns: Vec::new(),
        });
        let result = runtime
            .run_agentic_turn(AgenticTurnParams {
                thread_id: thread.id,
                input: "try protected paths".to_string(),
                execution_mode: ExecutionMode::Blocking,
                follow_up: None,
                sandbox_policy: Some(policy.clone()),
                model_steps: vec![ScriptedModelStep {
                    assistant_deltas: Vec::new(),
                    tool_calls: vec![
                        ToolCall {
                            id: "protected-file".to_string(),
                            name: "write_file".to_string(),
                            namespace: Some("oppi".to_string()),
                            arguments: json!({ "path": ".env", "content": "SECRET=1\n" }),
                        },
                        ToolCall {
                            id: "protected-shell".to_string(),
                            name: "shell_exec".to_string(),
                            namespace: Some("oppi".to_string()),
                            arguments: json!({
                                "command": "cat .env",
                                "cwd": root.display().to_string(),
                                "policy": policy,
                                "approvalGranted": true,
                            }),
                        },
                        ToolCall {
                            id: "protected-image".to_string(),
                            name: "image_gen".to_string(),
                            namespace: Some("oppi".to_string()),
                            arguments: json!({
                                "prompt": "robot",
                                "outputPath": ".env.generated.png",
                            }),
                        },
                    ],
                    tool_results: Vec::new(),
                    final_response: true,
                }],
                model_provider: None,
                tool_definitions: Vec::new(),
                approved_tool_call_ids: vec![
                    "protected-file".to_string(),
                    "protected-shell".to_string(),
                ],
                cancellation: None,
                max_continuations: Some(1),
            })
            .unwrap();

        let completed = result
            .events
            .iter()
            .filter_map(|event| match &event.kind {
                EventKind::ToolCallCompleted { result } => Some((
                    result.call_id.as_str(),
                    result.status,
                    result.error.as_deref().unwrap_or(""),
                )),
                _ => None,
            })
            .collect::<Vec<_>>();
        let denied = completed
            .iter()
            .filter_map(|(_, status, error)| {
                if *status == ToolResultStatus::Denied {
                    Some(*error)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        assert_eq!(denied.len(), 3, "{completed:?}");
        assert!(denied.iter().all(|error| error.contains("protected")));
        assert!(!root.join(".env").exists());
        assert!(!root.join(".env.generated.png").exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn rust_default_registry_exposes_pi_package_tool_names() {
        let registry = default_tool_registry();
        for name in [
            "todo_write",
            "ask_user",
            "suggest_next_message",
            "oppi_feedback_submit",
            "render_mermaid",
            "image_gen",
            "AgentTool",
            "shell_exec",
            "shell_task",
            "oppi_review_read",
            "oppi_review_ls",
            "oppi_review_grep",
        ] {
            assert!(
                registry.get(Some("oppi"), name).is_some(),
                "default Rust registry should expose oppi::{name}"
            );
        }
    }

    #[test]
    fn rust_owned_review_helper_tools_match_pi_auto_review_surface() {
        let root = std::env::temp_dir().join(format!("oppi-review-tools-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("README.md"), "alpha\nbeta\ngamma\n").unwrap();
        fs::write(root.join("src").join("lib.rs"), "fn gamma() {}\n").unwrap();

        let mut runtime = Runtime::new();
        let thread = runtime
            .start_thread(ThreadStartParams {
                project: ProjectRef {
                    id: "review-tools".to_string(),
                    cwd: root.display().to_string(),
                    display_name: Some("review-tools".to_string()),
                    workspace_roots: Vec::new(),
                },
                title: Some("review helper tools".to_string()),
            })
            .thread;
        let result = runtime
            .run_agentic_turn(AgenticTurnParams {
                thread_id: thread.id,
                input: "use review helper tools".to_string(),
                execution_mode: ExecutionMode::Blocking,
                follow_up: None,
                sandbox_policy: None,
                model_steps: vec![ScriptedModelStep {
                    assistant_deltas: Vec::new(),
                    tool_calls: vec![
                        ToolCall {
                            id: "review-read".to_string(),
                            name: "oppi_review_read".to_string(),
                            namespace: Some("oppi".to_string()),
                            arguments: json!({ "path": "README.md", "offset": 2, "limit": 1 }),
                        },
                        ToolCall {
                            id: "review-ls".to_string(),
                            name: "oppi_review_ls".to_string(),
                            namespace: Some("oppi".to_string()),
                            arguments: json!({ "path": "." }),
                        },
                        ToolCall {
                            id: "review-grep".to_string(),
                            name: "oppi_review_grep".to_string(),
                            namespace: Some("oppi".to_string()),
                            arguments: json!({ "path": ".", "pattern": "gamma" }),
                        },
                    ],
                    tool_results: Vec::new(),
                    final_response: true,
                }],
                model_provider: None,
                tool_definitions: Vec::new(),
                approved_tool_call_ids: Vec::new(),
                cancellation: None,
                max_continuations: Some(1),
            })
            .unwrap();

        assert_eq!(result.turn.status, TurnStatus::Completed);
        let outputs = result
            .events
            .iter()
            .filter_map(|event| match &event.kind {
                EventKind::ToolCallCompleted { result } => result.output.as_deref(),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(outputs.iter().any(|output| output.trim() == "beta"));
        assert!(
            outputs
                .iter()
                .any(|output| output.contains("README.md") && output.contains("src"))
        );
        assert!(
            outputs
                .iter()
                .any(|output| output.contains("README.md") && output.contains("gamma"))
        );
        assert!(
            outputs
                .iter()
                .any(|output| output.contains("src") && output.contains("gamma"))
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn turn_sandbox_policy_blocks_approved_write_in_read_only_mode() {
        let root =
            std::env::temp_dir().join(format!("oppi-readonly-policy-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let mut runtime = Runtime::new();
        let thread = runtime
            .start_thread(ThreadStartParams {
                project: ProjectRef {
                    id: "readonly-policy".to_string(),
                    cwd: root.display().to_string(),
                    display_name: Some("readonly-policy".to_string()),
                    workspace_roots: Vec::new(),
                },
                title: Some("readonly policy".to_string()),
            })
            .thread;
        let result = runtime
            .run_agentic_turn(AgenticTurnParams {
                thread_id: thread.id,
                input: "try approved write in read-only mode".to_string(),
                execution_mode: ExecutionMode::Blocking,
                follow_up: None,
                sandbox_policy: Some(default_policy(PermissionProfile {
                    mode: PermissionMode::ReadOnly,
                    readable_roots: vec![root.display().to_string()],
                    writable_roots: Vec::new(),
                    filesystem_rules: Vec::new(),
                    protected_patterns: Vec::new(),
                })),
                model_steps: vec![ScriptedModelStep {
                    assistant_deltas: Vec::new(),
                    tool_calls: vec![ToolCall {
                        id: "write-readonly".to_string(),
                        name: "write_file".to_string(),
                        namespace: Some("oppi".to_string()),
                        arguments: json!({ "path": "blocked.txt", "content": "nope" }),
                    }],
                    tool_results: Vec::new(),
                    final_response: true,
                }],
                model_provider: None,
                tool_definitions: Vec::new(),
                approved_tool_call_ids: vec!["write-readonly".to_string()],
                cancellation: None,
                max_continuations: Some(1),
            })
            .unwrap();

        assert_eq!(result.turn.status, TurnStatus::Completed);
        assert!(!root.join("blocked.txt").exists());
        assert!(result.events.iter().any(|event| match &event.kind {
            EventKind::ToolCallCompleted { result } => result.status == ToolResultStatus::Denied,
            _ => false,
        }));
        assert!(result.events.iter().any(|event| match &event.kind {
            EventKind::Diagnostic { diagnostic } =>
                diagnostic.metadata.get("component") == Some(&"permissions".to_string()),
            _ => false,
        }));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn feedback_tool_writes_sanitized_local_draft() {
        let root = std::env::temp_dir().join(format!("oppi-feedback-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let mut runtime = Runtime::new();
        let thread = runtime
            .start_thread(ThreadStartParams {
                project: ProjectRef {
                    id: "feedback".to_string(),
                    cwd: root.display().to_string(),
                    display_name: Some("feedback".to_string()),
                    workspace_roots: Vec::new(),
                },
                title: Some("feedback".to_string()),
            })
            .thread;
        let result = runtime
            .run_agentic_turn(AgenticTurnParams {
                thread_id: thread.id,
                input: "file feedback".to_string(),
                execution_mode: ExecutionMode::Blocking,
                follow_up: None,
                sandbox_policy: Some(default_policy(PermissionProfile {
                    mode: PermissionMode::Default,
                    readable_roots: vec![root.display().to_string()],
                    writable_roots: vec![root.display().to_string()],
                    filesystem_rules: Vec::new(),
                    protected_patterns: Vec::new(),
                })),
                model_steps: vec![ScriptedModelStep {
                    assistant_deltas: Vec::new(),
                    tool_calls: vec![ToolCall {
                        id: "feedback-1".to_string(),
                        name: "oppi_feedback_submit".to_string(),
                        namespace: Some("oppi".to_string()),
                        arguments: json!({
                            "type": "bug-report",
                            "summary": "Feedback draft parity",
                            "whatHappened": "Saw token=super-secret in logs",
                            "expectedBehavior": "Secrets are redacted",
                            "reproduction": "Run the draft path"
                        }),
                    }],
                    tool_results: Vec::new(),
                    final_response: true,
                }],
                model_provider: None,
                tool_definitions: Vec::new(),
                approved_tool_call_ids: Vec::new(),
                cancellation: None,
                max_continuations: Some(1),
            })
            .unwrap();

        assert_eq!(result.turn.status, TurnStatus::Completed);
        let drafts = fs::read_dir(root.join(".oppi").join("feedback-drafts"))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(drafts.len(), 1);
        let draft = fs::read_to_string(drafts[0].path()).unwrap();
        assert!(draft.contains("Feedback draft parity"));
        assert!(draft.contains("<redacted>"));
        assert!(!draft.contains("super-secret"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn shell_exec_background_task_can_be_read_by_shell_task() {
        let root = std::env::temp_dir().join(format!("oppi-shell-task-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let mut runtime = Runtime::new();
        let thread = runtime
            .start_thread(ThreadStartParams {
                project: ProjectRef {
                    id: "shell-task".to_string(),
                    cwd: root.display().to_string(),
                    display_name: Some("shell-task".to_string()),
                    workspace_roots: Vec::new(),
                },
                title: Some("shell task".to_string()),
            })
            .thread;
        let policy = default_policy(PermissionProfile {
            mode: PermissionMode::FullAccess,
            readable_roots: vec![root.display().to_string()],
            writable_roots: vec![root.display().to_string()],
            filesystem_rules: Vec::new(),
            protected_patterns: Vec::new(),
        });
        let background_command = if cfg!(windows) {
            "echo shell-task-parity & ping -n 10 127.0.0.1 > nul"
        } else {
            "printf 'shell-task-parity\\n'; sleep 10"
        };
        let started = runtime
            .run_agentic_turn(AgenticTurnParams {
                thread_id: thread.id.clone(),
                input: "start background shell".to_string(),
                execution_mode: ExecutionMode::Blocking,
                follow_up: None,
                sandbox_policy: Some(policy),
                model_steps: vec![ScriptedModelStep {
                    assistant_deltas: Vec::new(),
                    tool_calls: vec![ToolCall {
                        id: "shell-bg".to_string(),
                        name: "shell_exec".to_string(),
                        namespace: Some("oppi".to_string()),
                        arguments: json!({
                            "command": background_command,
                            "runInBackground": true,
                            "cwd": root.display().to_string(),
                            "sandboxPreference": "forbid"
                        }),
                    }],
                    tool_results: Vec::new(),
                    final_response: true,
                }],
                model_provider: None,
                tool_definitions: Vec::new(),
                approved_tool_call_ids: vec!["shell-bg".to_string()],
                cancellation: None,
                max_continuations: Some(1),
            })
            .unwrap();
        let start_results = started
            .events
            .iter()
            .filter_map(|event| match &event.kind {
                EventKind::ToolCallCompleted { result } => Some(result),
                _ => None,
            })
            .collect::<Vec<_>>();
        let start_output = start_results
            .iter()
            .filter_map(|result| result.output.as_deref())
            .collect::<Vec<_>>()
            .join("\n");
        if !start_output.contains("background shell task started: ") {
            let denied = start_results.iter().any(|result| {
                result.status == ToolResultStatus::Denied
                    && result.error.as_deref().is_some_and(|error| {
                        error.contains("sandboxed background execution is unavailable")
                            || error.contains("required OS sandbox")
                            || error.contains("background adapter")
                    })
            });
            assert!(denied, "expected background sandbox fail-closed denial");
            assert!(runtime.list_background_tasks().items.is_empty());
            let _ = fs::remove_dir_all(&root);
            return;
        }
        let task_id = start_output
            .lines()
            .find_map(|line| line.strip_prefix("background shell task started: "))
            .unwrap()
            .to_string();

        for _ in 0..20 {
            let output = runtime
                .read_background_task(BackgroundReadParams {
                    task_id: task_id.clone(),
                    max_bytes: Some(30_000),
                })
                .unwrap()
                .output;
            if output.contains("shell-task-parity") {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let read = runtime
            .run_agentic_turn(AgenticTurnParams {
                thread_id: thread.id,
                input: "read background shell".to_string(),
                execution_mode: ExecutionMode::Blocking,
                follow_up: None,
                sandbox_policy: None,
                model_steps: vec![ScriptedModelStep {
                    assistant_deltas: Vec::new(),
                    tool_calls: vec![ToolCall {
                        id: "shell-read".to_string(),
                        name: "shell_task".to_string(),
                        namespace: Some("oppi".to_string()),
                        arguments: json!({ "action": "read", "taskId": task_id }),
                    }],
                    tool_results: Vec::new(),
                    final_response: true,
                }],
                model_provider: None,
                tool_definitions: Vec::new(),
                approved_tool_call_ids: Vec::new(),
                cancellation: None,
                max_continuations: Some(1),
            })
            .unwrap();
        let read_output = read
            .events
            .iter()
            .filter_map(|event| match &event.kind {
                EventKind::ToolCallCompleted { result } => result.output.as_deref(),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(read_output.contains("shell-task-parity"));

        let listed = runtime.list_background_tasks();
        assert_eq!(listed.items.len(), 1);
        assert_eq!(listed.items[0].id, task_id);
        assert_eq!(listed.items[0].cwd, root.display().to_string());
        let direct_read = runtime
            .read_background_task(BackgroundReadParams {
                task_id: task_id.clone(),
                max_bytes: Some(30_000),
            })
            .unwrap();
        assert!(direct_read.output.contains("shell-task-parity"));
        let killed = runtime
            .kill_background_task(BackgroundKillParams {
                task_id: task_id.clone(),
            })
            .unwrap();
        assert_eq!(killed.task.id, task_id);
        assert_eq!(killed.task.status, BackgroundTaskStatus::Killed);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn render_mermaid_tool_outputs_ascii_and_optional_file() {
        let root = std::env::temp_dir().join(format!("oppi-mermaid-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let mut runtime = Runtime::new();
        let thread = runtime
            .start_thread(ThreadStartParams {
                project: ProjectRef {
                    id: "mermaid".to_string(),
                    cwd: root.display().to_string(),
                    display_name: Some("mermaid".to_string()),
                    workspace_roots: Vec::new(),
                },
                title: Some("mermaid".to_string()),
            })
            .thread;
        let result = runtime
            .run_agentic_turn(AgenticTurnParams {
                thread_id: thread.id,
                input: "render a diagram".to_string(),
                execution_mode: ExecutionMode::Blocking,
                follow_up: None,
                sandbox_policy: Some(default_policy(PermissionProfile {
                    mode: PermissionMode::Default,
                    readable_roots: vec![root.display().to_string()],
                    writable_roots: vec![root.display().to_string()],
                    filesystem_rules: Vec::new(),
                    protected_patterns: Vec::new(),
                })),
                model_steps: vec![ScriptedModelStep {
                    assistant_deltas: Vec::new(),
                    tool_calls: vec![ToolCall {
                        id: "mermaid-1".to_string(),
                        name: "render_mermaid".to_string(),
                        namespace: Some("oppi".to_string()),
                        arguments: json!({
                            "mermaid": "flowchart TD\n  A[Start] --> B[Done]",
                            "outputPath": "diagram.txt"
                        }),
                    }],
                    tool_results: Vec::new(),
                    final_response: true,
                }],
                model_provider: None,
                tool_definitions: Vec::new(),
                approved_tool_call_ids: Vec::new(),
                cancellation: None,
                max_continuations: Some(1),
            })
            .unwrap();

        assert_eq!(result.turn.status, TurnStatus::Completed);
        let saved = fs::read_to_string(root.join("diagram.txt")).unwrap();
        assert!(saved.contains("Start -> Done"));
        assert!(result.events.iter().any(|event| {
            match &event.kind {
                EventKind::ToolCallCompleted { result } => result
                    .output
                    .as_deref()
                    .is_some_and(|output| output.contains("Rust fallback")),
                _ => false,
            }
        }));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn suggest_next_tool_emits_nonblocking_suggestion_event() {
        let mut runtime = Runtime::new();
        let thread = thread(&mut runtime);
        let result = runtime
            .run_agentic_turn(AgenticTurnParams {
                thread_id: thread.id,
                input: "offer a likely reply".to_string(),
                execution_mode: ExecutionMode::Blocking,
                follow_up: None,
                sandbox_policy: None,
                model_steps: vec![ScriptedModelStep {
                    assistant_deltas: Vec::new(),
                    tool_calls: vec![ToolCall {
                        id: "suggest-1".to_string(),
                        name: "suggest_next_message".to_string(),
                        namespace: Some("oppi".to_string()),
                        arguments: json!({
                            "message": "Run the tests",
                            "confidence": 0.91,
                            "reason": "The user will likely ask for validation."
                        }),
                    }],
                    tool_results: Vec::new(),
                    final_response: true,
                }],
                model_provider: None,
                tool_definitions: Vec::new(),
                approved_tool_call_ids: Vec::new(),
                cancellation: None,
                max_continuations: Some(1),
            })
            .unwrap();

        assert_eq!(result.turn.status, TurnStatus::Completed);
        assert!(result.events.iter().any(|event| match &event.kind {
            EventKind::SuggestionOffered { suggestion } =>
                suggestion.message == "Run the tests" && suggestion.confidence > 0.9,
            _ => false,
        }));
    }

    #[test]
    fn rust_default_registry_exposes_codex_goal_tools() {
        let registry = default_tool_registry();
        assert!(registry.get(None, "get_goal").is_some());
        assert!(registry.get(None, "create_goal").is_some());
        assert!(registry.get(None, "update_goal").is_some());
    }

    #[test]
    fn goal_tool_update_rejects_non_complete_status() {
        let mut runtime = Runtime::new();
        let thread_id = thread(&mut runtime).id;
        runtime
            .set_thread_goal(ThreadGoalSetParams {
                thread_id: thread_id.clone(),
                objective: Some("Do the thing".to_string()),
                status: Some(ThreadGoalStatus::Active),
                token_budget: Some(None),
            })
            .unwrap();
        let result = runtime.execute_goal_tool(
            &thread_id,
            &ToolCall {
                id: "call-1".to_string(),
                namespace: None,
                name: "update_goal".to_string(),
                arguments: json!({ "status": "paused" }),
            },
        );

        assert_eq!(result.status, ToolResultStatus::Error);
        assert!(
            result
                .error
                .unwrap()
                .contains("only mark the existing goal complete")
        );
    }

    #[test]
    fn ask_user_tool_pauses_and_resumes_agentic_turn() {
        let mut runtime = Runtime::new();
        let thread = thread(&mut runtime);
        let paused = runtime
            .run_agentic_turn(AgenticTurnParams {
                thread_id: thread.id.clone(),
                input: "ask before proceeding".to_string(),
                execution_mode: ExecutionMode::Blocking,
                follow_up: None,
                sandbox_policy: None,
                model_steps: vec![ScriptedModelStep {
                    assistant_deltas: vec!["I need a choice.".to_string()],
                    tool_calls: vec![ToolCall {
                        id: "ask-1".to_string(),
                        name: "ask_user".to_string(),
                        namespace: Some("oppi".to_string()),
                        arguments: json!({
                            "title": "Pick a path",
                            "questions": [{
                                "id": "path",
                                "question": "Which path should I take?",
                                "options": [{ "id": "safe", "label": "Safe path" }],
                                "defaultOptionId": "safe"
                            }]
                        }),
                    }],
                    tool_results: Vec::new(),
                    final_response: false,
                }],
                model_provider: None,
                tool_definitions: Vec::new(),
                approved_tool_call_ids: Vec::new(),
                cancellation: None,
                max_continuations: Some(2),
            })
            .unwrap();

        assert_eq!(paused.turn.status, TurnStatus::WaitingForUser);
        let request = paused.awaiting_question.clone().unwrap();
        assert_eq!(request.title.as_deref(), Some("Pick a path"));
        assert_eq!(request.questions[0].id, "path");
        assert!(
            paused
                .events
                .iter()
                .any(|event| matches!(event.kind, EventKind::AskUserRequested { .. }))
        );

        let resumed = runtime
            .resume_agentic_turn(AgenticTurnResumeParams {
                thread_id: thread.id,
                turn_id: paused.turn.id,
                follow_up: None,
                ask_user_response: Some(AskUserResponse {
                    request_id: request.id.clone(),
                    answers: vec![AskUserAnswer {
                        question_id: "path".to_string(),
                        option_id: Some("safe".to_string()),
                        label: Some("Safe path".to_string()),
                        text: None,
                        skipped: false,
                    }],
                    cancelled: false,
                    timed_out: false,
                }),
                sandbox_policy: None,
                model_steps: vec![ScriptedModelStep {
                    assistant_deltas: vec!["Continuing with the safe path.".to_string()],
                    tool_calls: Vec::new(),
                    tool_results: Vec::new(),
                    final_response: true,
                }],
                model_provider: None,
                tool_definitions: Vec::new(),
                approved_tool_call_ids: Vec::new(),
                cancellation: None,
                max_continuations: Some(2),
            })
            .unwrap();

        assert_eq!(resumed.turn.status, TurnStatus::Completed);
        let outputs = resumed
            .events
            .iter()
            .filter_map(|event| match &event.kind {
                EventKind::ToolCallCompleted { result } => result.output.as_deref(),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(outputs.contains("path: safe (Safe path)"));
        assert!(
            resumed
                .events
                .iter()
                .any(|event| matches!(event.kind, EventKind::AskUserResolved { .. }))
        );
    }

    #[test]
    fn ask_user_settings_control_pause_timeout_metadata() {
        let mut runtime = Runtime::new();
        let thread = thread(&mut runtime);
        runtime
            .update_ask_user_settings(AskUserSettingsUpdateParams {
                thread_id: Some(thread.id.clone()),
                settings: AskUserSettings {
                    timeout_ms: 120_000,
                },
            })
            .unwrap();

        let paused = runtime
            .run_agentic_turn(AgenticTurnParams {
                thread_id: thread.id.clone(),
                input: "ask with timeout".to_string(),
                execution_mode: ExecutionMode::Blocking,
                follow_up: None,
                sandbox_policy: None,
                model_steps: vec![ScriptedModelStep {
                    assistant_deltas: vec!["I need a choice.".to_string()],
                    tool_calls: vec![ToolCall {
                        id: "ask-1".to_string(),
                        name: "ask_user".to_string(),
                        namespace: Some("oppi".to_string()),
                        arguments: json!({
                            "questions": [{
                                "id": "scope",
                                "question": "Which scope?",
                                "options": [{ "id": "changed", "label": "Changed package" }],
                                "defaultOptionId": "changed"
                            }]
                        }),
                    }],
                    tool_results: Vec::new(),
                    final_response: false,
                }],
                model_provider: None,
                tool_definitions: Vec::new(),
                approved_tool_call_ids: Vec::new(),
                cancellation: None,
                max_continuations: Some(2),
            })
            .unwrap();

        let request = paused.awaiting_question.unwrap();
        assert_eq!(request.timeout_ms, 120_000);
        let event_timeout = paused
            .events
            .iter()
            .find_map(|event| match &event.kind {
                EventKind::AskUserRequested { request } => Some(request.timeout_ms),
                _ => None,
            })
            .unwrap();
        assert_eq!(event_timeout, 120_000);
    }

    #[test]
    fn rust_owned_todo_write_updates_runtime_state() {
        let mut runtime = Runtime::new();
        let thread = thread(&mut runtime);
        let result = runtime
            .run_agentic_turn(AgenticTurnParams {
                thread_id: thread.id,
                input: "track work".to_string(),
                execution_mode: ExecutionMode::Blocking,
                follow_up: None,
                sandbox_policy: None,
                model_steps: vec![ScriptedModelStep {
                    assistant_deltas: Vec::new(),
                    tool_calls: vec![ToolCall {
                        id: "todo-1".to_string(),
                        name: "todo_write".to_string(),
                        namespace: Some("oppi".to_string()),
                        arguments: json!({
                            "summary": "Started todo parity",
                            "todos": [
                                { "id": "inspect", "content": "Inspect todo parity", "status": "completed", "priority": "high", "phase": "discovery", "notes": "done" },
                                { "id": "impl", "content": "Implement todo_write", "status": "in_progress", "priority": "high" }
                            ]
                        }),
                    }],
                    tool_results: Vec::new(),
                    final_response: true,
                }],
                model_provider: None,
                tool_definitions: Vec::new(),
                approved_tool_call_ids: Vec::new(),
                cancellation: None,
                max_continuations: Some(1),
            })
            .unwrap();

        assert_eq!(result.turn.status, TurnStatus::Completed);
        let todo_state = runtime.todos_state().state;
        assert_eq!(todo_state.summary, "Started todo parity");
        assert_eq!(todo_state.todos.len(), 2);
        assert_eq!(todo_state.todos[1].status, TodoStatus::InProgress);
        assert!(
            result
                .events
                .iter()
                .any(|event| matches!(event.kind, EventKind::TodosUpdated { .. }))
        );
        let output = result
            .events
            .iter()
            .filter_map(|event| match &event.kind {
                EventKind::ToolCallCompleted { result } => result.output.as_deref(),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(output.contains("Updated todos: Started todo parity"));
        assert!(output.contains("[in_progress] impl"));
    }

    #[test]
    fn direct_provider_executes_todo_write_and_persists_state() {
        let first = json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "todo-provider-1",
                        "type": "function",
                        "function": {
                            "name": "oppi__todo_write",
                            "arguments": serde_json::to_string(&json!({
                                "summary": "Provider updated todos",
                                "todos": [
                                    { "id": "todo", "content": "Port todo_write", "status": "completed", "priority": "high" }
                                ]
                            })).unwrap()
                        }
                    }]
                }
            }]
        })
        .to_string();
        let second = json!({
            "choices": [{
                "message": { "role": "assistant", "content": "todos updated" }
            }]
        })
        .to_string();
        let (base_url, server) = start_mock_openai_server(vec![first, second]);
        let key_name = format!("OPPI_TODO_PROVIDER_TEST_{}_API_KEY", std::process::id());
        unsafe { std::env::set_var(&key_name, "test-key") };
        let mut runtime = Runtime::new();
        let thread = thread(&mut runtime);
        let result = runtime
            .run_agentic_turn(AgenticTurnParams {
                thread_id: thread.id,
                input: "update todos".to_string(),
                execution_mode: ExecutionMode::Blocking,
                follow_up: None,
                sandbox_policy: None,
                model_steps: Vec::new(),
                model_provider: Some(DirectModelProviderConfig {
                    kind: DirectModelProviderKind::OpenAiCompatible,
                    model: "mock".to_string(),
                    base_url: Some(base_url),
                    api_key_env: Some(key_name.clone()),
                    system_prompt: Some("You are OPPi.".to_string()),
                    temperature: None,
                    reasoning_effort: None,
                    max_output_tokens: None,
                    stream: false,
                }),
                tool_definitions: Vec::new(),
                approved_tool_call_ids: Vec::new(),
                cancellation: None,
                max_continuations: Some(2),
            })
            .unwrap();
        unsafe { std::env::remove_var(&key_name) };
        let requests = server.join().unwrap();

        assert_eq!(result.turn.status, TurnStatus::Completed);
        assert!(requests[0].contains("oppi__todo_write"));
        assert_eq!(
            runtime.todos_state().state.summary,
            "Provider updated todos"
        );
        assert_eq!(
            runtime.todos_state().state.todos[0].status,
            TodoStatus::Completed
        );
        assert!(
            result
                .events
                .iter()
                .any(|event| matches!(event.kind, EventKind::TodosUpdated { .. }))
        );
    }

    #[test]
    fn direct_provider_sends_reasoning_effort_when_configured() {
        let response = json!({
            "choices": [{
                "message": { "role": "assistant", "content": "effort handled" }
            }]
        })
        .to_string();
        let (base_url, server) = start_mock_openai_server(vec![response]);
        let key_name = format!("OPPI_EFFORT_PROVIDER_TEST_{}_API_KEY", std::process::id());
        unsafe { std::env::set_var(&key_name, "test-key") };
        let mut runtime = Runtime::new();
        let thread = thread(&mut runtime);
        let result = runtime
            .run_agentic_turn(AgenticTurnParams {
                thread_id: thread.id,
                input: "think harder".to_string(),
                execution_mode: ExecutionMode::Blocking,
                follow_up: None,
                sandbox_policy: None,
                model_steps: Vec::new(),
                model_provider: Some(DirectModelProviderConfig {
                    kind: DirectModelProviderKind::OpenAiCompatible,
                    model: "mock".to_string(),
                    base_url: Some(base_url),
                    api_key_env: Some(key_name.clone()),
                    system_prompt: Some("You are OPPi.".to_string()),
                    temperature: None,
                    reasoning_effort: Some("high".to_string()),
                    max_output_tokens: None,
                    stream: false,
                }),
                tool_definitions: Vec::new(),
                approved_tool_call_ids: Vec::new(),
                cancellation: None,
                max_continuations: Some(1),
            })
            .unwrap();
        unsafe { std::env::remove_var(&key_name) };
        let requests = server.join().unwrap();
        assert_eq!(result.turn.status, TurnStatus::Completed);
        assert!(requests[0].contains("\"reasoning_effort\":\"high\""));
    }

    #[test]
    fn direct_provider_applies_follow_up_context_to_system_prompt() {
        let response = json!({
            "choices": [{
                "message": { "role": "assistant", "content": "follow-up handled" }
            }]
        })
        .to_string();
        let (base_url, server) = start_mock_openai_server(vec![response]);
        let key_name = format!(
            "OPPI_FOLLOW_UP_PROVIDER_TEST_{}_API_KEY",
            std::process::id()
        );
        unsafe { std::env::set_var(&key_name, "test-key") };
        let mut runtime = Runtime::new();
        let thread = thread(&mut runtime);
        let result = runtime
            .run_agentic_turn(AgenticTurnParams {
                thread_id: thread.id,
                input: "current follow-up".to_string(),
                execution_mode: ExecutionMode::Blocking,
                follow_up: Some(FollowUpChainContext {
                    chain_id: Some("fup-test".to_string()),
                    root_prompt: "original task".to_string(),
                    follow_ups: vec![
                        FollowUpItemContext {
                            id: "1".to_string(),
                            text: "current follow-up".to_string(),
                            status: FollowUpStatus::Running,
                        },
                        FollowUpItemContext {
                            id: "2".to_string(),
                            text: "pending follow-up".to_string(),
                            status: FollowUpStatus::Queued,
                        },
                    ],
                    current_follow_up_id: Some("1".to_string()),
                    prompt_variant_append: Some("Variant guidance for follow-up.".to_string()),
                }),
                sandbox_policy: None,
                model_steps: Vec::new(),
                model_provider: Some(DirectModelProviderConfig {
                    kind: DirectModelProviderKind::OpenAiCompatible,
                    model: "mock".to_string(),
                    base_url: Some(base_url),
                    api_key_env: Some(key_name.clone()),
                    system_prompt: Some("You are OPPi.".to_string()),
                    temperature: None,
                    reasoning_effort: None,
                    max_output_tokens: None,
                    stream: false,
                }),
                tool_definitions: Vec::new(),
                approved_tool_call_ids: Vec::new(),
                cancellation: None,
                max_continuations: Some(1),
            })
            .unwrap();
        unsafe { std::env::remove_var(&key_name) };
        let requests = server.join().unwrap();

        assert_eq!(result.turn.status, TurnStatus::Completed);
        assert!(requests[0].contains("OPPi follow-up chain context"));
        assert!(requests[0].contains("Initial standalone request: original task"));
        assert!(requests[0].contains("There is 1 additional queued follow-up"));
        assert!(requests[0].contains("Variant guidance for follow-up."));
        assert!(result.events.iter().any(|event| match &event.kind {
            EventKind::Diagnostic { diagnostic } =>
                diagnostic.metadata.get("component") == Some(&"follow-up".to_string()),
            _ => false,
        }));
    }

    #[test]
    fn direct_provider_approval_resume_executes_pending_tool_and_continues() {
        let root = std::env::temp_dir().join(format!("oppi-direct-resume-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
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
        let key_name = format!("OPPI_DIRECT_RESUME_TEST_{}_API_KEY", std::process::id());
        unsafe { std::env::set_var(&key_name, "test-key") };
        let mut runtime = Runtime::new();
        let thread = runtime
            .start_thread(ThreadStartParams {
                project: ProjectRef {
                    id: "direct-resume".to_string(),
                    cwd: root.display().to_string(),
                    display_name: Some("direct-resume".to_string()),
                    workspace_roots: Vec::new(),
                },
                title: Some("direct resume".to_string()),
            })
            .thread;
        let paused = runtime
            .run_agentic_turn(AgenticTurnParams {
                thread_id: thread.id.clone(),
                input: "write with approval".to_string(),
                execution_mode: ExecutionMode::Blocking,
                follow_up: None,
                sandbox_policy: None,
                model_steps: Vec::new(),
                model_provider: Some(DirectModelProviderConfig {
                    kind: DirectModelProviderKind::OpenAiCompatible,
                    model: "mock".to_string(),
                    base_url: Some(base_url),
                    api_key_env: Some(key_name.clone()),
                    system_prompt: None,
                    temperature: None,
                    reasoning_effort: None,
                    max_output_tokens: None,
                    stream: false,
                }),
                tool_definitions: Vec::new(),
                approved_tool_call_ids: Vec::new(),
                cancellation: None,
                max_continuations: Some(2),
            })
            .unwrap();
        assert_eq!(paused.turn.status, TurnStatus::WaitingForApproval);
        let events = runtime
            .events_after(EventsListParams {
                thread_id: thread.id.clone(),
                after: 0,
                limit: None,
            })
            .unwrap()
            .events;
        assert!(
            events.iter().any(|event| {
                matches!(event.kind, EventKind::ProviderTranscriptSnapshot { .. })
            })
        );
        let mut replayed = Runtime::replay_events(&events).unwrap();
        assert!(replayed.direct_provider_turns.contains_key(&paused.turn.id));
        assert!(
            replayed
                .recover_incomplete_turns("restart")
                .events
                .is_empty()
        );

        let resumed = replayed
            .resume_agentic_turn(AgenticTurnResumeParams {
                thread_id: thread.id,
                turn_id: paused.turn.id,
                follow_up: None,
                ask_user_response: None,
                sandbox_policy: None,
                model_steps: Vec::new(),
                model_provider: None,
                tool_definitions: Vec::new(),
                approved_tool_call_ids: vec!["write-approved".to_string()],
                cancellation: None,
                max_continuations: Some(2),
            })
            .unwrap();
        unsafe { std::env::remove_var(&key_name) };
        let requests = server.join().unwrap();

        assert_eq!(resumed.turn.status, TurnStatus::Completed);
        assert_eq!(
            fs::read_to_string(root.join("approved.txt")).unwrap(),
            "approved"
        );
        assert!(requests[1].contains("\"role\":\"tool\""));
        assert!(assistant_text_from_events(&resumed.events).contains("continued after approval"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn direct_provider_ask_user_resume_replays_snapshot_after_restart() {
        let first = json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "ask-user-direct",
                        "type": "function",
                        "function": {
                            "name": "oppi__ask_user",
                            "arguments": serde_json::to_string(&json!({
                                "title": "Need input",
                                "questions": [{ "id": "q1", "question": "Proceed?", "options": [{ "id": "yes", "label": "Yes" }] }]
                            })).unwrap()
                        }
                    }]
                }
            }]
        })
        .to_string();
        let second = json!({
            "choices": [{
                "message": { "role": "assistant", "content": "continued after answer" }
            }]
        })
        .to_string();
        let (base_url, server) = start_mock_openai_server(vec![first, second]);
        let key_name = format!("OPPI_DIRECT_ASK_REPLAY_TEST_{}_API_KEY", std::process::id());
        unsafe { std::env::set_var(&key_name, "test-key") };
        let mut runtime = Runtime::new();
        let thread = thread(&mut runtime);
        let paused = runtime
            .run_agentic_turn(AgenticTurnParams {
                thread_id: thread.id.clone(),
                input: "ask before continuing".to_string(),
                execution_mode: ExecutionMode::Blocking,
                follow_up: None,
                sandbox_policy: None,
                model_steps: Vec::new(),
                model_provider: Some(DirectModelProviderConfig {
                    kind: DirectModelProviderKind::OpenAiCompatible,
                    model: "mock".to_string(),
                    base_url: Some(base_url),
                    api_key_env: Some(key_name.clone()),
                    system_prompt: None,
                    temperature: None,
                    reasoning_effort: None,
                    max_output_tokens: None,
                    stream: false,
                }),
                tool_definitions: Vec::new(),
                approved_tool_call_ids: Vec::new(),
                cancellation: None,
                max_continuations: Some(2),
            })
            .unwrap();
        assert_eq!(paused.turn.status, TurnStatus::WaitingForUser);
        let request = paused.awaiting_question.clone().unwrap();
        let events = runtime
            .events_after(EventsListParams {
                thread_id: thread.id.clone(),
                after: 0,
                limit: None,
            })
            .unwrap()
            .events;
        let mut replayed = Runtime::replay_events(&events).unwrap();
        assert!(replayed.direct_provider_turns.contains_key(&paused.turn.id));
        assert!(
            replayed
                .recover_incomplete_turns("restart")
                .events
                .is_empty()
        );
        let resumed = replayed
            .resume_agentic_turn(AgenticTurnResumeParams {
                thread_id: thread.id,
                turn_id: paused.turn.id,
                follow_up: None,
                ask_user_response: Some(AskUserResponse {
                    request_id: request.id,
                    answers: vec![AskUserAnswer {
                        question_id: "q1".to_string(),
                        option_id: Some("yes".to_string()),
                        label: Some("Yes".to_string()),
                        text: None,
                        skipped: false,
                    }],
                    cancelled: false,
                    timed_out: false,
                }),
                sandbox_policy: None,
                model_steps: Vec::new(),
                model_provider: None,
                tool_definitions: Vec::new(),
                approved_tool_call_ids: Vec::new(),
                cancellation: None,
                max_continuations: Some(2),
            })
            .unwrap();
        unsafe { std::env::remove_var(&key_name) };
        let requests = server.join().unwrap();
        assert_eq!(resumed.turn.status, TurnStatus::Completed);
        assert!(requests[1].contains("\"role\":\"tool\""));
        assert!(assistant_text_from_events(&resumed.events).contains("continued after answer"));
    }

    #[test]
    fn direct_provider_rejects_unapproved_api_key_env_names() {
        let mut runtime = Runtime::new();
        let thread = thread(&mut runtime);
        let error = runtime
            .run_agentic_turn(AgenticTurnParams {
                thread_id: thread.id.clone(),
                input: "hello direct provider".to_string(),
                execution_mode: ExecutionMode::Blocking,
                follow_up: None,
                sandbox_policy: None,
                model_steps: Vec::new(),
                model_provider: Some(DirectModelProviderConfig {
                    kind: DirectModelProviderKind::OpenAiCompatible,
                    model: "mock-model".to_string(),
                    base_url: Some("http://127.0.0.1:9/v1".to_string()),
                    api_key_env: Some("PATH".to_string()),
                    system_prompt: None,
                    temperature: None,
                    reasoning_effort: None,
                    max_output_tokens: None,
                    stream: false,
                }),
                tool_definitions: Vec::new(),
                approved_tool_call_ids: Vec::new(),
                cancellation: None,
                max_continuations: Some(1),
            })
            .unwrap_err();

        assert_eq!(error.code, "provider_api_key_env_not_allowed");
        assert_eq!(error.category, RuntimeErrorCategory::Provider);
        assert_eq!(
            runtime.metrics().turn_status_counts.get("aborted").copied(),
            Some(1)
        );
    }

    #[test]
    fn codex_access_token_account_id_matches_pi_jwt_claim() {
        assert_eq!(
            codex_account_id_from_access_token(&fake_codex_access_token("acct_test")).unwrap(),
            "acct_test"
        );
        let error = codex_account_id_from_access_token("not-a-jwt").unwrap_err();
        assert_eq!(error.code, "provider_auth_invalid");
    }

    #[test]
    fn codex_auth_uses_valid_access_token_without_locking() {
        let root =
            std::env::temp_dir().join(format!("oppi-codex-valid-no-lock-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let auth_path = root.join("auth.json");
        let lock_path = codex_auth_lock_path(&auth_path);
        fs::write(
            &auth_path,
            serde_json::to_vec(&json!({
                "openai-codex": {
                    "type": "oauth",
                    "access": fake_codex_access_token("acct_test"),
                    "refresh": "refresh-token",
                    "expires": now_millis_i64().saturating_add(600_000),
                    "accountId": "acct_test"
                }
            }))
            .unwrap(),
        )
        .unwrap();

        let old = std::env::var_os("OPPI_OPENAI_CODEX_AUTH_PATH");
        unsafe { std::env::set_var("OPPI_OPENAI_CODEX_AUTH_PATH", &auth_path) };
        let auth = read_or_refresh_codex_auth(&ureq::Agent::new()).unwrap();
        match old {
            Some(value) => unsafe { std::env::set_var("OPPI_OPENAI_CODEX_AUTH_PATH", value) },
            None => unsafe { std::env::remove_var("OPPI_OPENAI_CODEX_AUTH_PATH") },
        }

        assert_eq!(auth.account_id, "acct_test");
        assert!(
            !lock_path.exists(),
            "fresh Codex auth should not create a lock directory before refresh is needed"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn codex_request_body_omits_unsupported_max_output_tokens() {
        let provider = OpenAiCodexModelProvider::new(
            DirectModelProviderConfig {
                kind: DirectModelProviderKind::OpenAiCodex,
                model: "gpt-5.4".to_string(),
                base_url: None,
                api_key_env: None,
                system_prompt: Some("You are OPPi.".to_string()),
                temperature: None,
                reasoning_effort: Some("medium".to_string()),
                max_output_tokens: Some(16),
                stream: true,
            },
            Vec::new(),
        );
        let body = provider.request_body(&ModelRequest {
            thread_id: "thread-1".to_string(),
            turn_id: "turn-1".to_string(),
            input: "reply ok".to_string(),
            continuation: 0,
            history: Vec::new(),
        });
        assert_eq!(body["model"], json!("gpt-5.4"));
        assert_eq!(body["instructions"], json!("You are OPPi."));
        assert_eq!(body["reasoning"]["effort"], json!("medium"));
        assert!(body.get("max_output_tokens").is_none());
    }

    #[test]
    fn codex_tool_schema_includes_empty_properties_object() {
        let definition = ToolDefinition {
            name: "create_goal".to_string(),
            namespace: None,
            description: Some("Create a goal".to_string()),
            concurrency_safe: false,
            requires_approval: false,
            capabilities: Vec::new(),
            ..Default::default()
        };
        let tool = codex_tool_definition("create_goal", &definition);
        assert_eq!(tool["parameters"]["type"], json!("object"));
        assert_eq!(tool["parameters"]["properties"], json!({}));
        assert_eq!(tool["parameters"]["additionalProperties"], json!(true));
    }

    #[test]
    fn github_copilot_auth_helpers_match_pi_behavior() {
        let auth = GitHubCopilotAuth {
            access_token: "tid=1;proxy-ep=proxy.individual.githubcopilot.com;exp=4102444800;"
                .to_string(),
            refresh_token: "github-refresh".to_string(),
            expires: 4_102_444_800_000,
            enterprise_domain: None,
        };
        assert_eq!(
            github_copilot_base_url(&auth),
            "https://api.individual.githubcopilot.com"
        );
        assert_eq!(
            github_copilot_initiator(&[json!({ "role": "tool", "content": "ok" })]),
            "agent"
        );
        assert_eq!(
            github_copilot_initiator(&[json!({ "role": "user", "content": "hi" })]),
            "user"
        );
    }

    #[test]
    fn direct_provider_requires_env_api_key_without_mutation() {
        let mut runtime = Runtime::new();
        let thread = thread(&mut runtime);
        let error = runtime
            .run_agentic_turn(AgenticTurnParams {
                thread_id: thread.id.clone(),
                input: "hello direct provider".to_string(),
                execution_mode: ExecutionMode::Blocking,
                follow_up: None,
                sandbox_policy: None,
                model_steps: Vec::new(),
                model_provider: Some(DirectModelProviderConfig {
                    kind: DirectModelProviderKind::OpenAiCompatible,
                    model: "mock-model".to_string(),
                    base_url: Some("http://127.0.0.1:9/v1".to_string()),
                    api_key_env: Some("OPPI_TEST_MISSING_DIRECT_PROVIDER_API_KEY".to_string()),
                    system_prompt: None,
                    temperature: None,
                    reasoning_effort: None,
                    max_output_tokens: None,
                    stream: false,
                }),
                tool_definitions: Vec::new(),
                approved_tool_call_ids: Vec::new(),
                cancellation: None,
                max_continuations: Some(1),
            })
            .unwrap_err();

        assert_eq!(error.code, "provider_auth_missing");
        assert_eq!(error.category, RuntimeErrorCategory::Provider);
        assert_eq!(
            runtime.metrics().turn_status_counts.get("aborted").copied(),
            Some(1)
        );
    }

    #[test]
    fn direct_provider_allows_meridian_placeholder_key_only_for_loopback() {
        unsafe { std::env::remove_var(MERIDIAN_API_KEY_ENV) };
        let loopback = OpenAiCompatibleModelProvider::new(
            DirectModelProviderConfig {
                kind: DirectModelProviderKind::OpenAiCompatible,
                model: "claude-sonnet-4-6".to_string(),
                base_url: Some("http://127.0.0.1:3456/v1".to_string()),
                api_key_env: Some(MERIDIAN_API_KEY_ENV.to_string()),
                system_prompt: None,
                temperature: None,
                reasoning_effort: None,
                max_output_tokens: None,
                stream: false,
            },
            Vec::new(),
        );
        assert_eq!(loopback.api_key().unwrap(), "x");

        let remote = OpenAiCompatibleModelProvider::new(
            DirectModelProviderConfig {
                kind: DirectModelProviderKind::OpenAiCompatible,
                model: "claude-sonnet-4-6".to_string(),
                base_url: Some("https://example.com/v1".to_string()),
                api_key_env: Some(MERIDIAN_API_KEY_ENV.to_string()),
                system_prompt: None,
                temperature: None,
                reasoning_effort: None,
                max_output_tokens: None,
                stream: false,
            },
            Vec::new(),
        );
        let error = remote.api_key().unwrap_err();
        assert_eq!(error.code, "provider_auth_missing");
    }

    #[test]
    fn adapter_backed_tool_results_complete_without_local_echo() {
        let mut runtime = Runtime::new();
        let thread = thread(&mut runtime);
        let result = runtime
            .run_agentic_turn(AgenticTurnParams {
                thread_id: thread.id,
                input: "adapter provided a tool result".to_string(),
                execution_mode: ExecutionMode::Blocking,
                follow_up: None,
                sandbox_policy: None,
                model_steps: vec![ScriptedModelStep {
                    assistant_deltas: vec!["Reading through Pi.".to_string()],
                    tool_calls: vec![ToolCall {
                        id: "pi-read-1".to_string(),
                        name: "read".to_string(),
                        namespace: Some("pi".to_string()),
                        arguments: json!({ "path": "README.md" }),
                    }],
                    tool_results: vec![ToolResult {
                        call_id: "pi-read-1".to_string(),
                        status: ToolResultStatus::Ok,
                        output: Some("adapter file contents".to_string()),
                        error: None,
                    }],
                    final_response: true,
                }],
                model_provider: None,
                tool_definitions: vec![ToolDefinition {
                    name: "read".to_string(),
                    namespace: Some("pi".to_string()),
                    description: Some("Pi read tool".to_string()),
                    concurrency_safe: true,
                    requires_approval: false,
                    capabilities: vec!["filesystem".to_string()],
                    ..Default::default()
                }],
                approved_tool_call_ids: Vec::new(),
                cancellation: None,
                max_continuations: Some(2),
            })
            .unwrap();

        assert_eq!(result.turn.status, TurnStatus::Completed);
        assert!(result.events.iter().any(|event| matches!(
            &event.kind,
            EventKind::ToolCallCompleted {
                result: ToolResult {
                    status: ToolResultStatus::Ok,
                    output: Some(output),
                    ..
                }
            } if output == "adapter file contents"
        )));
    }

    #[test]
    fn image_gen_adapter_result_is_accepted_without_rust_execution() {
        let mut runtime = Runtime::new();
        let thread = thread(&mut runtime);
        let result = runtime
            .run_agentic_turn(AgenticTurnParams {
                thread_id: thread.id,
                input: "generate an image".to_string(),
                execution_mode: ExecutionMode::Blocking,
                follow_up: None,
                sandbox_policy: None,
                model_steps: vec![ScriptedModelStep {
                    assistant_deltas: Vec::new(),
                    tool_calls: vec![ToolCall {
                        id: "image-1".to_string(),
                        name: "image_gen".to_string(),
                        namespace: None,
                        arguments: json!({ "prompt": "a small robot" }),
                    }],
                    tool_results: vec![ToolResult {
                        call_id: "image-1".to_string(),
                        status: ToolResultStatus::Ok,
                        output: Some("Generated image: output/imagegen/robot.png".to_string()),
                        error: None,
                    }],
                    final_response: true,
                }],
                model_provider: None,
                tool_definitions: vec![ToolDefinition {
                    name: "image_gen".to_string(),
                    namespace: None,
                    description: Some("Host-provided image generator".to_string()),
                    concurrency_safe: false,
                    requires_approval: false,
                    capabilities: vec!["image-generation".to_string()],
                    ..Default::default()
                }],
                approved_tool_call_ids: Vec::new(),
                cancellation: None,
                max_continuations: Some(2),
            })
            .unwrap();

        assert_eq!(result.turn.status, TurnStatus::Completed);
        assert!(result.events.iter().any(|event| matches!(
            &event.kind,
            EventKind::ToolCallCompleted {
                result: ToolResult {
                    status: ToolResultStatus::Ok,
                    output: Some(output),
                    ..
                }
            } if output.contains("output/imagegen/robot.png")
        )));
    }

    #[test]
    fn image_gen_native_backend_writes_b64_output() {
        let root = std::env::temp_dir().join(format!("oppi-image-gen-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let response = json!({ "data": [{ "b64_json": "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/p9sAAAAASUVORK5CYII=" }] }).to_string();
        let (base_url, server) = start_mock_openai_server(vec![response]);
        let key_name = format!("OPPI_IMAGE_GEN_TEST_{}_API_KEY", std::process::id());
        unsafe { std::env::set_var(&key_name, "test-key") };
        let mut runtime = Runtime::new();
        let thread = runtime
            .start_thread(ThreadStartParams {
                project: ProjectRef {
                    id: "image-project".to_string(),
                    cwd: root.display().to_string(),
                    display_name: Some("image-project".to_string()),
                    workspace_roots: Vec::new(),
                },
                title: Some("image".to_string()),
            })
            .thread;
        let policy = default_policy(PermissionProfile {
            mode: PermissionMode::FullAccess,
            readable_roots: vec![root.display().to_string()],
            writable_roots: vec![root.display().to_string()],
            filesystem_rules: Vec::new(),
            protected_patterns: Vec::new(),
        });
        let result = runtime
            .run_agentic_turn(AgenticTurnParams {
                thread_id: thread.id,
                input: "generate an image".to_string(),
                execution_mode: ExecutionMode::Blocking,
                follow_up: None,
                sandbox_policy: Some(policy),
                model_steps: vec![ScriptedModelStep {
                    assistant_deltas: Vec::new(),
                    tool_calls: vec![ToolCall {
                        id: "image-1".to_string(),
                        name: "image_gen".to_string(),
                        namespace: None,
                        arguments: json!({
                            "prompt": "a small robot",
                            "baseUrl": base_url,
                            "apiKeyEnv": key_name,
                            "outputPath": "output/test.png",
                            "outputFormat": "png"
                        }),
                    }],
                    tool_results: Vec::new(),
                    final_response: true,
                }],
                model_provider: None,
                tool_definitions: vec![ToolDefinition {
                    name: "image_gen".to_string(),
                    namespace: None,
                    description: Some("Native image generator".to_string()),
                    concurrency_safe: false,
                    requires_approval: false,
                    capabilities: vec!["image-generation".to_string()],
                    ..Default::default()
                }],
                approved_tool_call_ids: Vec::new(),
                cancellation: None,
                max_continuations: Some(2),
            })
            .unwrap();
        unsafe { std::env::remove_var(&key_name) };
        let requests = server.join().unwrap();

        assert_eq!(result.turn.status, TurnStatus::Completed);
        assert!(
            fs::read(root.join("output/test.png"))
                .unwrap()
                .starts_with(b"\x89PNG")
        );
        assert!(requests[0].contains("POST /v1/images/generations"));
        assert!(requests[0].contains("authorization: Bearer test-key"));
        assert!(requests[0].contains("\"model\":\"gpt-image-2\""));
        assert!(result.events.iter().any(|event| matches!(
            &event.kind,
            EventKind::ToolCallCompleted {
                result: ToolResult {
                    status: ToolResultStatus::Ok,
                    output: Some(output),
                    ..
                }
            } if output.contains("test.png") && output.contains("openai-images")
        )));
        assert!(result.events.iter().any(|event| matches!(
            &event.kind,
            EventKind::ArtifactCreated { artifact }
                if artifact.output_path.contains("test.png")
                    && artifact.mime_type.as_deref() == Some("image/png")
                    && artifact.width == Some(1)
                    && artifact.height == Some(1)
                    && artifact.source_images.is_empty()
                    && artifact.mask.is_none()
                    && artifact.backend.as_deref() == Some("openai-images")
        )));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn image_gen_native_backend_downloads_url_outputs() {
        let root = std::env::temp_dir().join(format!("oppi-image-gen-url-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let png = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/p9sAAAAASUVORK5CYII=";
        let png_bytes = decode_base64(png).unwrap();
        let (image_base_url, image_server) =
            start_mock_http_byte_server(vec![("image/png".to_string(), png_bytes.clone())]);
        let image_url = format!("{image_base_url}/generated.png");
        let response = json!({ "data": [{ "url": image_url }] }).to_string();
        let (base_url, api_server) = start_mock_openai_server(vec![response]);
        let key_name = format!("OPPI_IMAGE_GEN_URL_TEST_{}_API_KEY", std::process::id());
        unsafe { std::env::set_var(&key_name, "test-key") };
        let mut runtime = Runtime::new();
        let thread = runtime
            .start_thread(ThreadStartParams {
                project: ProjectRef {
                    id: "image-url-project".to_string(),
                    cwd: root.display().to_string(),
                    display_name: Some("image-url-project".to_string()),
                    workspace_roots: Vec::new(),
                },
                title: Some("image url".to_string()),
            })
            .thread;
        let policy = default_policy(PermissionProfile {
            mode: PermissionMode::FullAccess,
            readable_roots: vec![root.display().to_string()],
            writable_roots: vec![root.display().to_string()],
            filesystem_rules: Vec::new(),
            protected_patterns: Vec::new(),
        });
        let result = run_image_tool_once(
            &mut runtime,
            thread.id,
            json!({
                "prompt": "a small robot from url response",
                "baseUrl": base_url,
                "apiKeyEnv": key_name,
                "outputPath": "output/url.png",
                "outputFormat": "png"
            }),
            Some(policy),
        );
        unsafe { std::env::remove_var(&key_name) };
        let api_requests = api_server.join().unwrap();
        let image_requests = image_server.join().unwrap();

        assert_eq!(result.turn.status, TurnStatus::Completed);
        assert_eq!(fs::read(root.join("output/url.png")).unwrap(), png_bytes);
        assert!(api_requests[0].contains("POST /v1/images/generations"));
        assert!(image_requests[0].contains("GET /generated.png"));
        assert!(result.events.iter().any(|event| matches!(
            &event.kind,
            EventKind::ToolCallCompleted {
                result: ToolResult {
                    status: ToolResultStatus::Ok,
                    output: Some(output),
                    ..
                }
            } if output.contains("url.png")
                && output.contains("openai-images")
                && !output.contains("returned a URL instead of b64_json")
        )));
        assert!(result.events.iter().any(|event| matches!(
            &event.kind,
            EventKind::ArtifactCreated { artifact }
                if artifact.output_path.contains("url.png")
                    && artifact.mime_type.as_deref() == Some("image/png")
                    && artifact.width == Some(1)
                    && artifact.height == Some(1)
        )));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn image_gen_codex_native_backend_writes_image_generation_result() {
        let _env_lock = TEST_ENV_LOCK.lock().unwrap();
        let root = std::env::temp_dir().join(format!(
            "oppi-image-gen-codex-native-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let png = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/p9sAAAAASUVORK5CYII=";
        let png_bytes = decode_base64(png).unwrap();
        let sse = format!(
            "data: {{\"type\":\"response.output_item.done\",\"item\":{{\"type\":\"image_generation_call\",\"status\":\"completed\",\"result\":\"{png}\",\"revised_prompt\":\"a tiny robot\"}}}}\n\n\
             data: {{\"type\":\"response.completed\",\"response\":{{\"output\":[]}}}}\n\n"
        );
        let (base_url, server) =
            start_mock_http_byte_server(vec![("text/event-stream".to_string(), sse.into_bytes())]);
        let auth_path = root.join("auth.json");
        fs::write(
            &auth_path,
            serde_json::to_vec(&json!({
                "openai-codex": {
                    "type": "oauth",
                    "access": fake_codex_access_token("acct_image"),
                    "refresh": "refresh-token",
                    "expires": now_millis_i64().saturating_add(600_000),
                    "accountId": "acct_image"
                }
            }))
            .unwrap(),
        )
        .unwrap();
        let _auth_path = EnvVarGuard::set("OPPI_OPENAI_CODEX_AUTH_PATH", &auth_path);
        let _base_url = EnvVarGuard::set("OPPI_OPENAI_CODEX_BASE_URL", format!("{base_url}/v1"));
        let mut runtime = Runtime::new();
        let thread = runtime
            .start_thread(ThreadStartParams {
                project: ProjectRef {
                    id: "image-codex-native-project".to_string(),
                    cwd: root.display().to_string(),
                    display_name: Some("image-codex-native-project".to_string()),
                    workspace_roots: Vec::new(),
                },
                title: Some("image codex native".to_string()),
            })
            .thread;
        let policy = default_policy(PermissionProfile {
            mode: PermissionMode::FullAccess,
            readable_roots: vec![root.display().to_string()],
            writable_roots: vec![root.display().to_string()],
            filesystem_rules: Vec::new(),
            protected_patterns: Vec::new(),
        });
        let result = run_image_tool_once(
            &mut runtime,
            thread.id,
            json!({
                "prompt": "a tiny robot",
                "backend": "codex_native",
                "outputPath": "output/codex.png",
                "outputFormat": "png"
            }),
            Some(policy),
        );
        let requests = server.join().unwrap();

        assert_eq!(result.turn.status, TurnStatus::Completed);
        if let Some(error) = image_tool_error(&result) {
            panic!("codex-native image_gen should succeed: {error}");
        }
        assert_eq!(fs::read(root.join("output/codex.png")).unwrap(), png_bytes);
        assert_eq!(requests.len(), 1);
        assert!(requests[0].contains("POST /v1/codex/responses"));
        assert!(requests[0].contains("authorization: Bearer "));
        assert!(requests[0].contains("chatgpt-account-id: acct_image"));
        assert!(requests[0].contains("\"type\":\"image_generation\""));
        assert!(requests[0].contains("\"output_format\":\"png\""));
        assert!(result.events.iter().any(|event| matches!(
            &event.kind,
            EventKind::ToolCallCompleted {
                result: ToolResult {
                    status: ToolResultStatus::Ok,
                    output: Some(output),
                    ..
                }
            } if output.contains("codex.png")
                && output.contains("codex-native")
                && output.contains("a tiny robot")
        )));
        assert!(result.events.iter().any(|event| matches!(
            &event.kind,
            EventKind::ArtifactCreated { artifact }
                if artifact.output_path.contains("codex.png")
                    && artifact.mime_type.as_deref() == Some("image/png")
                    && artifact.backend.as_deref() == Some("codex-native")
                    && artifact.model.as_deref() == Some("gpt-5.2")
        )));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn image_gen_auto_backend_uses_codex_native_for_codex_parent() {
        let _env_lock = TEST_ENV_LOCK.lock().unwrap();
        let root =
            std::env::temp_dir().join(format!("oppi-image-gen-codex-auto-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let png = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/p9sAAAAASUVORK5CYII=";
        let sse = format!(
            "data: {{\"type\":\"response.output_item.done\",\"item\":{{\"type\":\"image_generation_call\",\"status\":\"completed\",\"result\":\"{png}\",\"revised_prompt\":\"auto robot\"}}}}\n\n"
        );
        let (base_url, server) =
            start_mock_http_byte_server(vec![("text/event-stream".to_string(), sse.into_bytes())]);
        let auth_path = root.join("auth.json");
        fs::write(
            &auth_path,
            serde_json::to_vec(&json!({
                "openai-codex": {
                    "type": "oauth",
                    "access": fake_codex_access_token("acct_auto_image"),
                    "refresh": "refresh-token",
                    "expires": now_millis_i64().saturating_add(600_000),
                    "accountId": "acct_auto_image"
                }
            }))
            .unwrap(),
        )
        .unwrap();
        let _auth_path = EnvVarGuard::set("OPPI_OPENAI_CODEX_AUTH_PATH", &auth_path);
        let mut runtime = Runtime::new();
        let thread = runtime
            .start_thread(ThreadStartParams {
                project: ProjectRef {
                    id: "image-codex-auto-project".to_string(),
                    cwd: root.display().to_string(),
                    display_name: Some("image-codex-auto-project".to_string()),
                    workspace_roots: Vec::new(),
                },
                title: Some("image codex auto".to_string()),
            })
            .thread;
        let turn = Turn {
            id: "turn-image-codex-auto".to_string(),
            thread_id: thread.id,
            status: TurnStatus::Running,
            phase: TurnPhase::Tools,
            parent_turn_id: None,
        };
        runtime.turn_sandbox_policies.insert(
            turn.id.clone(),
            default_policy(PermissionProfile {
                mode: PermissionMode::FullAccess,
                readable_roots: vec![root.display().to_string()],
                writable_roots: vec![root.display().to_string()],
                filesystem_rules: Vec::new(),
                protected_patterns: Vec::new(),
            }),
        );
        let parent = DirectModelProviderConfig {
            kind: DirectModelProviderKind::OpenAiCodex,
            model: "gpt-5.2".to_string(),
            base_url: Some(format!("{base_url}/v1")),
            api_key_env: None,
            system_prompt: Some("You are OPPi.".to_string()),
            temperature: None,
            reasoning_effort: None,
            max_output_tokens: None,
            stream: true,
        };
        let call = ToolCall {
            id: "image-auto".to_string(),
            name: "image_gen".to_string(),
            namespace: None,
            arguments: json!({
                "prompt": "auto robot",
                "outputPath": "output/auto-codex.png",
                "outputFormat": "png"
            }),
        };

        let output = runtime
            .execute_image_gen_tool_inner(&turn, &call, Some(&parent))
            .expect("auto Codex-native image_gen should succeed");
        let value = serde_json::from_str::<Value>(&output).unwrap();
        let requests = server.join().unwrap();

        assert_eq!(value["backend"], "codex-native");
        assert_eq!(value["model"], "gpt-5.2");
        assert_eq!(value["revisedPrompt"], "auto robot");
        assert!(root.join("output/auto-codex.png").exists());
        assert_eq!(requests.len(), 1);
        assert!(requests[0].contains("POST /v1/codex/responses"));
        assert!(requests[0].contains("chatgpt-account-id: acct_auto_image"));
        assert!(!requests[0].contains("/images/generations"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn image_gen_native_backend_posts_multipart_edits_for_sources_and_mask() {
        let root = std::env::temp_dir().join(format!("oppi-image-gen-edit-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let png = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/p9sAAAAASUVORK5CYII=";
        fs::write(root.join("input.png"), decode_base64(png).unwrap()).unwrap();
        fs::write(root.join("mask.png"), decode_base64(png).unwrap()).unwrap();
        let response = json!({ "data": [{ "b64_json": png }] }).to_string();
        let (base_url, server) = start_mock_openai_server(vec![response]);
        let key_name = format!("OPPI_IMAGE_GEN_EDIT_TEST_{}_API_KEY", std::process::id());
        unsafe { std::env::set_var(&key_name, "test-key") };
        let mut runtime = Runtime::new();
        let thread = runtime
            .start_thread(ThreadStartParams {
                project: ProjectRef {
                    id: "image-edit-project".to_string(),
                    cwd: root.display().to_string(),
                    display_name: Some("image-edit-project".to_string()),
                    workspace_roots: Vec::new(),
                },
                title: Some("image edit".to_string()),
            })
            .thread;
        let policy = default_policy(PermissionProfile {
            mode: PermissionMode::FullAccess,
            readable_roots: vec![root.display().to_string()],
            writable_roots: vec![root.display().to_string()],
            filesystem_rules: Vec::new(),
            protected_patterns: Vec::new(),
        });
        let result = run_image_tool_once(
            &mut runtime,
            thread.id,
            json!({
                "prompt": "turn the robot blue",
                "baseUrl": base_url,
                "apiKeyEnv": key_name,
                "outputPath": "output/edit.png",
                "outputFormat": "png",
                "model": "gpt-image-1.5",
                "images": ["input.png"],
                "mask": "mask.png",
                "inputFidelity": "high"
            }),
            Some(policy),
        );
        unsafe { std::env::remove_var(&key_name) };
        let requests = server.join().unwrap();

        assert_eq!(result.turn.status, TurnStatus::Completed);
        assert!(
            fs::read(root.join("output/edit.png"))
                .unwrap()
                .starts_with(b"\x89PNG")
        );
        assert!(requests[0].contains("POST /v1/images/edits"));
        assert!(requests[0].contains("multipart/form-data; boundary="));
        assert!(requests[0].contains("name=\"prompt\""));
        assert!(requests[0].contains("turn the robot blue"));
        assert!(requests[0].contains("name=\"image\"; filename=\"input.png\""));
        assert!(requests[0].contains("name=\"mask\"; filename=\"mask.png\""));
        assert!(requests[0].contains("name=\"input_fidelity\""));
        assert!(requests[0].contains("gpt-image-1.5"));
        assert!(result.events.iter().any(|event| matches!(
            &event.kind,
            EventKind::ArtifactCreated { artifact }
                if artifact.output_path.contains("edit.png")
                    && artifact.source_images == vec!["input.png".to_string()]
                    && artifact.mask.as_deref() == Some("mask.png")
        )));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn image_gen_native_backend_writes_multiple_b64_outputs() {
        let root =
            std::env::temp_dir().join(format!("oppi-image-gen-multi-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let png = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/p9sAAAAASUVORK5CYII=";
        let response = json!({ "data": [{ "b64_json": png }, { "b64_json": png }] }).to_string();
        let (base_url, server) = start_mock_openai_server(vec![response]);
        let key_name = format!("OPPI_IMAGE_GEN_MULTI_TEST_{}_API_KEY", std::process::id());
        unsafe { std::env::set_var(&key_name, "test-key") };
        let mut runtime = Runtime::new();
        let thread = runtime
            .start_thread(ThreadStartParams {
                project: ProjectRef {
                    id: "image-multi-project".to_string(),
                    cwd: root.display().to_string(),
                    display_name: Some("image-multi-project".to_string()),
                    workspace_roots: Vec::new(),
                },
                title: Some("image multi".to_string()),
            })
            .thread;
        let policy = default_policy(PermissionProfile {
            mode: PermissionMode::FullAccess,
            readable_roots: vec![root.display().to_string()],
            writable_roots: vec![root.display().to_string()],
            filesystem_rules: Vec::new(),
            protected_patterns: Vec::new(),
        });
        let result = run_image_tool_once(
            &mut runtime,
            thread.id,
            json!({
                "prompt": "two small robots",
                "baseUrl": base_url,
                "apiKeyEnv": key_name,
                "outputPath": "output/robot.png",
                "outputFormat": "png",
                "n": 2
            }),
            Some(policy),
        );
        unsafe { std::env::remove_var(&key_name) };
        if let Some(error) = image_tool_error(&result) {
            panic!("multi-output image_gen should succeed: {error}");
        }
        let requests = server.join().unwrap();

        assert_eq!(result.turn.status, TurnStatus::Completed);
        assert!(
            fs::read(root.join("output/robot-1.png"))
                .unwrap()
                .starts_with(b"\x89PNG")
        );
        assert!(
            fs::read(root.join("output/robot-2.png"))
                .unwrap()
                .starts_with(b"\x89PNG")
        );
        assert!(requests[0].contains("\"n\":2"));
        assert!(result.events.iter().any(|event| matches!(
            &event.kind,
            EventKind::ToolCallCompleted {
                result: ToolResult {
                    status: ToolResultStatus::Ok,
                    output: Some(output),
                    ..
                }
            } if output.contains("robot-1.png")
                && output.contains("robot-2.png")
                && output.contains("\"outputPaths\"")
        )));
        assert!(result.events.iter().any(|event| matches!(
            &event.kind,
            EventKind::ArtifactCreated { artifact }
                if artifact.output_path.contains("robot-1.png")
                    && artifact.mime_type.as_deref() == Some("image/png")
                    && artifact.backend.as_deref() == Some("openai-images")
        )));
        let _ = fs::remove_dir_all(&root);
    }

    fn image_tool_definition() -> ToolDefinition {
        ToolDefinition {
            name: "image_gen".to_string(),
            namespace: None,
            description: Some("Native image generator".to_string()),
            concurrency_safe: false,
            requires_approval: false,
            capabilities: vec!["image-generation".to_string()],
            ..Default::default()
        }
    }

    fn run_image_tool_once(
        runtime: &mut Runtime,
        thread_id: ThreadId,
        arguments: Value,
        sandbox_policy: Option<SandboxPolicy>,
    ) -> AgenticTurnResult {
        runtime
            .run_agentic_turn(AgenticTurnParams {
                thread_id,
                input: "generate an image".to_string(),
                execution_mode: ExecutionMode::Blocking,
                follow_up: None,
                sandbox_policy,
                model_steps: vec![ScriptedModelStep {
                    assistant_deltas: Vec::new(),
                    tool_calls: vec![ToolCall {
                        id: "image-1".to_string(),
                        name: "image_gen".to_string(),
                        namespace: None,
                        arguments,
                    }],
                    tool_results: Vec::new(),
                    final_response: true,
                }],
                model_provider: None,
                tool_definitions: vec![image_tool_definition()],
                approved_tool_call_ids: Vec::new(),
                cancellation: None,
                max_continuations: Some(2),
            })
            .unwrap()
    }

    fn image_tool_error(result: &AgenticTurnResult) -> Option<String> {
        result.events.iter().find_map(|event| match &event.kind {
            EventKind::ToolCallCompleted {
                result:
                    ToolResult {
                        status,
                        error: Some(error),
                        ..
                    },
            } if *status != ToolResultStatus::Ok => Some(error.clone()),
            _ => None,
        })
    }

    #[test]
    fn image_gen_without_adapter_result_fails_closed() {
        let mut runtime = Runtime::new();
        let thread = thread(&mut runtime);
        let result = runtime
            .run_agentic_turn(AgenticTurnParams {
                thread_id: thread.id,
                input: "generate an image".to_string(),
                execution_mode: ExecutionMode::Blocking,
                follow_up: None,
                sandbox_policy: None,
                model_steps: vec![ScriptedModelStep {
                    assistant_deltas: Vec::new(),
                    tool_calls: vec![ToolCall {
                        id: "image-1".to_string(),
                        name: "image_gen".to_string(),
                        namespace: None,
                        arguments: json!({ "prompt": "a small robot" }),
                    }],
                    tool_results: Vec::new(),
                    final_response: true,
                }],
                model_provider: None,
                tool_definitions: vec![ToolDefinition {
                    name: "image_gen".to_string(),
                    namespace: None,
                    description: Some("Host-provided image generator".to_string()),
                    concurrency_safe: false,
                    requires_approval: false,
                    capabilities: vec!["image-generation".to_string()],
                    ..Default::default()
                }],
                approved_tool_call_ids: Vec::new(),
                cancellation: None,
                max_continuations: Some(2),
            })
            .unwrap();

        assert_eq!(result.turn.status, TurnStatus::Completed);
        assert!(result.events.iter().any(|event| matches!(
            &event.kind,
            EventKind::ToolCallCompleted {
                result: ToolResult {
                    status: ToolResultStatus::Error,
                    output: None,
                    error: Some(error),
                    ..
                }
            } if error.contains("image_gen requires")
        )));
    }

    #[test]
    fn image_gen_network_disabled_fails_closed_after_credentials() {
        let key_name = format!("OPPI_IMAGE_NETWORK_TEST_{}_API_KEY", std::process::id());
        unsafe { std::env::set_var(&key_name, "test-key") };
        let mut runtime = Runtime::new();
        let thread = thread(&mut runtime);
        let result = run_image_tool_once(
            &mut runtime,
            thread.id,
            json!({ "prompt": "robot", "apiKeyEnv": key_name }),
            None,
        );
        unsafe { std::env::remove_var(&key_name) };
        let error = image_tool_error(&result).unwrap();
        assert!(error.contains("network policy enabled"));
        assert!(result.events.iter().any(|event| matches!(
            &event.kind,
            EventKind::ToolCallCompleted {
                result: ToolResult {
                    status: ToolResultStatus::Denied,
                    ..
                }
            }
        )));
    }

    #[test]
    fn image_gen_rejects_existing_output_without_overwrite() {
        let root =
            std::env::temp_dir().join(format!("oppi-image-overwrite-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("output")).unwrap();
        fs::write(root.join("output/existing.png"), b"old").unwrap();
        let mut runtime = Runtime::new();
        let thread = runtime
            .start_thread(ThreadStartParams {
                project: ProjectRef {
                    id: "image-overwrite".to_string(),
                    cwd: root.display().to_string(),
                    display_name: None,
                    workspace_roots: Vec::new(),
                },
                title: None,
            })
            .thread;
        let result = run_image_tool_once(
            &mut runtime,
            thread.id,
            json!({ "prompt": "robot", "outputPath": "output/existing.png" }),
            None,
        );
        let error = image_tool_error(&result).unwrap();
        assert!(error.contains("output already exists"));
        assert_eq!(fs::read(root.join("output/existing.png")).unwrap(), b"old");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn image_gen_transparent_background_requires_confirmation_and_model() {
        let mut runtime = Runtime::new();
        let thread = thread(&mut runtime);
        let result = run_image_tool_once(
            &mut runtime,
            thread.id,
            json!({ "prompt": "robot", "background": "transparent" }),
            None,
        );
        let error = image_tool_error(&result).unwrap();
        assert!(error.contains("transparent background requires explicit confirmation"));
    }

    #[test]
    fn image_gen_rejects_invalid_image_api_options_before_backend_setup() {
        let cases = vec![
            (
                json!({ "prompt": "robot", "model": "dall-e-3" }),
                "model must be a GPT Image model",
            ),
            (
                json!({ "prompt": "robot", "outputFormat": "bmp" }),
                "outputFormat must be png, jpeg, jpg, or webp",
            ),
            (
                json!({ "prompt": "robot", "quality": "ultra" }),
                "quality must be low, medium, high, or auto",
            ),
            (
                json!({ "prompt": "robot", "background": "blurred" }),
                "background must be transparent, opaque, or auto",
            ),
            (
                json!({ "prompt": "robot", "moderation": "strict" }),
                "moderation must be low or auto",
            ),
            (
                json!({ "prompt": "robot", "outputCompression": 101 }),
                "outputCompression must be between 0 and 100",
            ),
            (
                json!({ "prompt": "robot", "inputFidelity": "low" }),
                "inputFidelity is not supported with gpt-image-2",
            ),
            (
                json!({ "prompt": "robot", "size": "1025x1024" }),
                "gpt-image-2 size width and height must be multiples of 16px",
            ),
        ];

        for (arguments, expected) in cases {
            let mut runtime = Runtime::new();
            let thread = thread(&mut runtime);
            let result = run_image_tool_once(&mut runtime, thread.id, arguments, None);
            let error = image_tool_error(&result).unwrap();
            assert!(
                error.contains(expected),
                "expected error containing {expected:?}, got {error:?}"
            );
            assert!(
                !error.contains("API key") && !error.contains("network policy"),
                "validation should fail before credential/network setup, got {error:?}"
            );
        }
    }

    #[test]
    fn agentic_tool_pairing_errors_abort_turn() {
        let mut runtime = Runtime::new();
        let thread = thread(&mut runtime);
        let error = runtime
            .run_agentic_turn(AgenticTurnParams {
                thread_id: thread.id.clone(),
                input: "bad adapter result".to_string(),
                execution_mode: ExecutionMode::Blocking,
                follow_up: None,
                sandbox_policy: None,
                model_steps: vec![ScriptedModelStep {
                    assistant_deltas: vec!["Bad result.".to_string()],
                    tool_calls: Vec::new(),
                    tool_results: vec![ToolResult {
                        call_id: "missing-call".to_string(),
                        status: ToolResultStatus::Ok,
                        output: Some("orphan".to_string()),
                        error: None,
                    }],
                    final_response: false,
                }],
                model_provider: None,
                tool_definitions: Vec::new(),
                approved_tool_call_ids: Vec::new(),
                cancellation: None,
                max_continuations: Some(1),
            })
            .unwrap_err();

        assert_eq!(error.code, "tool_result_without_call");
        let events = runtime
            .events_after(EventsListParams {
                thread_id: thread.id,
                after: 0,
                limit: None,
            })
            .unwrap()
            .events;
        assert!(
            events
                .iter()
                .any(|event| matches!(event.kind, EventKind::TurnAborted { .. }))
        );
        let metrics = runtime.metrics();
        assert_eq!(metrics.turn_status_counts.get("aborted").copied(), Some(1));
        assert_eq!(metrics.turn_status_counts.get("running").copied(), None);
    }

    #[test]
    fn shell_tool_routes_through_sandbox_exec_boundary() {
        let mut runtime = Runtime::new();
        let cwd = std::env::current_dir().unwrap().display().to_string();
        let thread = runtime
            .start_thread(ThreadStartParams {
                project: ProjectRef {
                    id: "shell-project".to_string(),
                    cwd: cwd.clone(),
                    display_name: Some("shell-project".to_string()),
                    workspace_roots: Vec::new(),
                },
                title: Some("shell".to_string()),
            })
            .thread;
        let policy = default_policy(PermissionProfile {
            mode: PermissionMode::FullAccess,
            readable_roots: Vec::new(),
            writable_roots: Vec::new(),
            filesystem_rules: Vec::new(),
            protected_patterns: Vec::new(),
        });
        let result = runtime
            .run_agentic_turn(AgenticTurnParams {
                thread_id: thread.id,
                input: "run shell".to_string(),
                execution_mode: ExecutionMode::Blocking,
                follow_up: None,
                sandbox_policy: None,
                model_steps: vec![ScriptedModelStep {
                    assistant_deltas: vec!["Running shell.".to_string()],
                    tool_calls: vec![ToolCall {
                        id: "shell-1".to_string(),
                        name: "shell_exec".to_string(),
                        namespace: Some("oppi".to_string()),
                        arguments: json!({
                            "command": "echo plan21-shell",
                            "cwd": cwd,
                            "policy": policy,
                            "sandboxPreference": "forbid",
                            "approvalGranted": true,
                            "maxOutputBytes": 4096,
                            "timeoutMs": 5000,
                        }),
                    }],
                    tool_results: Vec::new(),
                    final_response: true,
                }],
                model_provider: None,
                tool_definitions: Vec::new(),
                approved_tool_call_ids: vec!["shell-1".to_string()],
                cancellation: None,
                max_continuations: Some(2),
            })
            .unwrap();

        assert_eq!(result.turn.status, TurnStatus::Completed);
        assert!(result.events.iter().any(|event| matches!(
            &event.kind,
            EventKind::ToolCallCompleted {
                result: ToolResult {
                    status: ToolResultStatus::Ok,
                    output: Some(output),
                    ..
                }
            } if output.contains("plan21-shell")
        )));
    }

    #[test]
    fn cancellation_aborts_matching_in_flight_tool_call() {
        let mut runtime = Runtime::new();
        let thread = thread(&mut runtime);
        let result = runtime
            .run_agentic_turn(AgenticTurnParams {
                thread_id: thread.id,
                input: "cancel tool".to_string(),
                execution_mode: ExecutionMode::Blocking,
                follow_up: None,
                sandbox_policy: None,
                model_steps: vec![ScriptedModelStep {
                    assistant_deltas: Vec::new(),
                    tool_calls: vec![agentic_call("echo-cancel", "never")],
                    tool_results: Vec::new(),
                    final_response: false,
                }],
                model_provider: None,
                tool_definitions: Vec::new(),
                approved_tool_call_ids: Vec::new(),
                cancellation: Some(AgenticCancellation {
                    reason: "user cancelled running tool".to_string(),
                    before_model_continuation: None,
                    tool_call_ids: vec!["echo-cancel".to_string()],
                }),
                max_continuations: Some(2),
            })
            .unwrap();

        assert_eq!(result.turn.status, TurnStatus::Aborted);
        assert!(result.events.iter().any(|event| matches!(
            event.kind,
            EventKind::ToolCallCompleted {
                result: ToolResult {
                    status: ToolResultStatus::Aborted,
                    ..
                }
            }
        )));
        assert!(
            result
                .events
                .iter()
                .any(|event| matches!(event.kind, EventKind::TurnAborted { .. }))
        );
    }

    #[test]
    fn agentic_turn_pauses_for_approval_and_resumes_same_turn() {
        let mut runtime = Runtime::new();
        let thread = thread(&mut runtime);
        let mut call = agentic_call("echo-approval", "approved output");
        call.arguments = json!({ "output": "approved output", "requireApproval": true });
        let paused = runtime
            .run_agentic_turn(AgenticTurnParams {
                thread_id: thread.id.clone(),
                input: "needs approval".to_string(),
                execution_mode: ExecutionMode::Blocking,
                follow_up: None,
                sandbox_policy: None,
                model_steps: vec![ScriptedModelStep {
                    assistant_deltas: vec!["Need approval.".to_string()],
                    tool_calls: vec![call.clone()],
                    tool_results: Vec::new(),
                    final_response: false,
                }],
                model_provider: None,
                tool_definitions: Vec::new(),
                approved_tool_call_ids: Vec::new(),
                cancellation: None,
                max_continuations: Some(4),
            })
            .unwrap();
        assert_eq!(paused.turn.status, TurnStatus::WaitingForApproval);
        assert_eq!(
            paused
                .awaiting_approval
                .as_ref()
                .unwrap()
                .tool_call
                .as_ref()
                .unwrap()
                .id,
            "echo-approval"
        );

        let resumed = runtime
            .resume_agentic_turn(AgenticTurnResumeParams {
                thread_id: thread.id,
                turn_id: paused.turn.id.clone(),
                follow_up: None,
                ask_user_response: None,
                sandbox_policy: None,
                model_steps: vec![
                    ScriptedModelStep {
                        assistant_deltas: Vec::new(),
                        tool_calls: vec![call],
                        tool_results: Vec::new(),
                        final_response: false,
                    },
                    ScriptedModelStep {
                        assistant_deltas: vec!["Approved and done.".to_string()],
                        tool_calls: Vec::new(),
                        tool_results: Vec::new(),
                        final_response: true,
                    },
                ],
                model_provider: None,
                tool_definitions: Vec::new(),
                approved_tool_call_ids: vec!["echo-approval".to_string()],
                cancellation: None,
                max_continuations: Some(4),
            })
            .unwrap();
        assert_eq!(resumed.turn.id, paused.turn.id);
        assert_eq!(resumed.turn.status, TurnStatus::Completed);
        assert!(
            resumed
                .events
                .iter()
                .any(|event| matches!(event.kind, EventKind::ApprovalResolved { .. }))
        );
        assert!(
            resumed
                .events
                .iter()
                .any(|event| matches!(event.kind, EventKind::ToolCallCompleted { .. }))
        );
    }

    #[test]
    fn runtime_tool_definition_policy_requires_approval_without_prompt_flag() {
        let mut runtime = Runtime::new();
        let thread = thread(&mut runtime);
        let call = ToolCall {
            id: "risky-custom".to_string(),
            name: "risky_custom".to_string(),
            namespace: Some("oppi".to_string()),
            arguments: json!({ "path": "output.txt" }),
        };

        let paused = runtime
            .run_agentic_turn(AgenticTurnParams {
                thread_id: thread.id,
                input: "run risky custom tool".to_string(),
                execution_mode: ExecutionMode::Blocking,
                follow_up: None,
                sandbox_policy: None,
                model_steps: vec![ScriptedModelStep {
                    assistant_deltas: Vec::new(),
                    tool_calls: vec![call],
                    tool_results: Vec::new(),
                    final_response: false,
                }],
                model_provider: None,
                tool_definitions: vec![ToolDefinition {
                    name: "risky_custom".to_string(),
                    namespace: Some("oppi".to_string()),
                    description: Some("A host-owned risky tool.".to_string()),
                    concurrency_safe: false,
                    requires_approval: true,
                    capabilities: vec!["filesystem".to_string(), "write".to_string()],
                    ..Default::default()
                }],
                approved_tool_call_ids: Vec::new(),
                cancellation: None,
                max_continuations: Some(1),
            })
            .unwrap();

        assert_eq!(paused.turn.status, TurnStatus::WaitingForApproval);
        let approval = paused.awaiting_approval.unwrap();
        assert_eq!(
            approval.tool_call.as_ref().map(|call| call.id.as_str()),
            Some("risky-custom")
        );
        assert_eq!(runtime.metrics().pending_approvals, 1);
    }

    fn guardian_auto_review_policy(root: &Path) -> SandboxPolicy {
        default_policy(PermissionProfile {
            mode: PermissionMode::AutoReview,
            readable_roots: vec![root.display().to_string()],
            writable_roots: vec![root.display().to_string()],
            filesystem_rules: Vec::new(),
            protected_patterns: vec![".env*".to_string()],
        })
    }

    fn guardian_model_write_response(call_id: &str, path: &str, content: &str) -> String {
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "",
                    "tool_calls": [{
                        "id": call_id,
                        "type": "function",
                        "function": {
                            "name": "oppi__write_file",
                            "arguments": json!({ "path": path, "content": content }).to_string()
                        }
                    }]
                }
            }]
        })
        .to_string()
    }

    fn guardian_model_final_response(text: &str) -> String {
        json!({
            "choices": [{
                "message": { "role": "assistant", "content": text }
            }]
        })
        .to_string()
    }

    fn guardian_reviewer_decision_response(
        outcome: &str,
        risk_level: &str,
        authorization: &str,
        cache_scope: &str,
    ) -> String {
        let decision = json!({
            "outcome": outcome,
            "risk_level": risk_level,
            "user_authorization": authorization,
            "rationale": format!("Guardian reviewer returned {outcome}."),
            "cache_scope": cache_scope,
        })
        .to_string();
        json!({
            "choices": [{
                "message": { "role": "assistant", "content": decision }
            }]
        })
        .to_string()
    }

    fn guardian_auto_review_thread(runtime: &mut Runtime, root: &Path) -> Thread {
        runtime
            .start_thread(ThreadStartParams {
                project: ProjectRef {
                    id: "guardian".to_string(),
                    cwd: root.display().to_string(),
                    display_name: None,
                    workspace_roots: Vec::new(),
                },
                title: None,
            })
            .thread
    }

    fn guardian_auto_review_provider(
        base_url: String,
        key_name: String,
    ) -> DirectModelProviderConfig {
        DirectModelProviderConfig {
            kind: DirectModelProviderKind::OpenAiCompatible,
            model: "mock-reviewer".to_string(),
            base_url: Some(base_url),
            api_key_env: Some(key_name),
            system_prompt: Some("You are OPPi.".to_string()),
            temperature: None,
            reasoning_effort: None,
            max_output_tokens: None,
            stream: false,
        }
    }

    fn guardian_run_write_turn(
        runtime: &mut Runtime,
        thread_id: String,
        root: &Path,
        base_url: String,
        key_name: String,
        _call_id: &str,
        path: &str,
    ) -> AgenticTurnResult {
        runtime
            .run_agentic_turn(AgenticTurnParams {
                thread_id,
                input: format!("write {path}"),
                execution_mode: ExecutionMode::Blocking,
                follow_up: None,
                sandbox_policy: Some(guardian_auto_review_policy(root)),
                model_steps: Vec::new(),
                model_provider: Some(guardian_auto_review_provider(base_url, key_name)),
                tool_definitions: Vec::new(),
                approved_tool_call_ids: Vec::new(),
                cancellation: None,
                max_continuations: Some(3),
            })
            .unwrap()
    }

    fn guardian_review_completed_records(events: &[Event]) -> Vec<GuardianReviewRecord> {
        events
            .iter()
            .filter_map(|event| match &event.kind {
                EventKind::PermissionReviewCompleted { record } => Some(record.clone()),
                _ => None,
            })
            .collect()
    }

    fn guardian_reviewer_request_count(requests: &[String]) -> usize {
        requests
            .iter()
            .filter(|request| request.contains("Review the proposed OPPi tool call"))
            .count()
    }

    #[test]
    fn guardian_auto_review_allow_executes_tool_without_user_approval() {
        let root = std::env::temp_dir().join(format!("oppi-guardian-allow-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let responses = vec![
            guardian_model_write_response("write-doc", "docs/out.md", "ok"),
            guardian_reviewer_decision_response("allow", "low", "high", "exact"),
            guardian_model_final_response("done"),
        ];
        let (base_url, server) = start_mock_openai_server(responses);
        let key_name = format!("OPPI_GUARDIAN_ALLOW_{}_API_KEY", std::process::id());
        unsafe { std::env::set_var(&key_name, "test-key") };
        let mut runtime = Runtime::new();
        let thread = guardian_auto_review_thread(&mut runtime, &root);

        let result = guardian_run_write_turn(
            &mut runtime,
            thread.id,
            &root,
            base_url,
            key_name.clone(),
            "write-doc",
            "docs/out.md",
        );
        unsafe { std::env::remove_var(&key_name) };

        assert_eq!(result.turn.status, TurnStatus::Completed);
        assert_eq!(fs::read_to_string(root.join("docs/out.md")).unwrap(), "ok");
        assert!(
            !result
                .events
                .iter()
                .any(|event| matches!(event.kind, EventKind::ApprovalRequested { .. }))
        );
        let records = guardian_review_completed_records(&result.events);
        assert!(records.iter().any(|record| {
            record.tool_call_id == "write-doc" && record.status == GuardianReviewStatus::Approved
        }));
        let requests = server.join().unwrap();
        assert_eq!(guardian_reviewer_request_count(&requests), 1);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn guardian_auto_review_deny_returns_tool_error_without_user_approval() {
        let root = std::env::temp_dir().join(format!("oppi-guardian-deny-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let responses = vec![
            guardian_model_write_response("write-doc", "docs/out.md", "nope"),
            guardian_reviewer_decision_response("deny", "high", "low", "none"),
            guardian_model_final_response("denied"),
        ];
        let (base_url, server) = start_mock_openai_server(responses);
        let key_name = format!("OPPI_GUARDIAN_DENY_{}_API_KEY", std::process::id());
        unsafe { std::env::set_var(&key_name, "test-key") };
        let mut runtime = Runtime::new();
        let thread = guardian_auto_review_thread(&mut runtime, &root);

        let result = guardian_run_write_turn(
            &mut runtime,
            thread.id,
            &root,
            base_url,
            key_name.clone(),
            "write-doc",
            "docs/out.md",
        );
        unsafe { std::env::remove_var(&key_name) };

        assert_eq!(result.turn.status, TurnStatus::Completed);
        assert!(!root.join("docs/out.md").exists());
        assert!(
            !result
                .events
                .iter()
                .any(|event| matches!(event.kind, EventKind::ApprovalRequested { .. }))
        );
        assert!(result.events.iter().any(|event| matches!(
            &event.kind,
            EventKind::ToolCallCompleted {
                result: ToolResult {
                    call_id,
                    status: ToolResultStatus::Denied,
                    ..
                }
            } if call_id == "write-doc"
        )));
        let records = guardian_review_completed_records(&result.events);
        assert!(records.iter().any(|record| {
            record.tool_call_id == "write-doc" && record.status == GuardianReviewStatus::Denied
        }));
        let requests = server.join().unwrap();
        assert_eq!(guardian_reviewer_request_count(&requests), 1);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn guardian_auto_review_cached_allow_skips_second_reviewer_call() {
        let root = std::env::temp_dir().join(format!("oppi-guardian-cache-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let responses = vec![
            guardian_model_write_response("write-doc", "docs/out.md", "ok"),
            guardian_reviewer_decision_response("allow", "low", "high", "exact"),
            guardian_model_final_response("first done"),
            guardian_model_write_response("write-doc", "docs/out.md", "ok"),
            guardian_model_final_response("second done"),
        ];
        let (base_url, server) = start_mock_openai_server(responses);
        let key_name = format!("OPPI_GUARDIAN_CACHE_{}_API_KEY", std::process::id());
        unsafe { std::env::set_var(&key_name, "test-key") };
        let mut runtime = Runtime::new();
        let thread = guardian_auto_review_thread(&mut runtime, &root);

        let first = guardian_run_write_turn(
            &mut runtime,
            thread.id.clone(),
            &root,
            base_url.clone(),
            key_name.clone(),
            "write-doc",
            "docs/out.md",
        );
        let second = guardian_run_write_turn(
            &mut runtime,
            thread.id,
            &root,
            base_url,
            key_name.clone(),
            "write-doc",
            "docs/out.md",
        );
        unsafe { std::env::remove_var(&key_name) };

        assert_eq!(first.turn.status, TurnStatus::Completed);
        assert_eq!(second.turn.status, TurnStatus::Completed);
        let second_records = guardian_review_completed_records(&second.events);
        assert!(second_records.iter().any(|record| {
            record.tool_call_id == "write-doc" && record.status == GuardianReviewStatus::Cached
        }));
        let requests = server.join().unwrap();
        assert_eq!(guardian_reviewer_request_count(&requests), 1);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn guardian_auto_review_circuit_blocks_repeated_denials() {
        let root =
            std::env::temp_dir().join(format!("oppi-guardian-circuit-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let responses = vec![
            guardian_model_write_response("write-doc", "docs/out.md", "nope"),
            guardian_reviewer_decision_response("deny", "high", "low", "none"),
            guardian_model_final_response("denied one"),
            guardian_model_write_response("write-doc", "docs/out.md", "nope"),
            guardian_reviewer_decision_response("deny", "high", "low", "none"),
            guardian_model_final_response("denied two"),
            guardian_model_write_response("write-doc", "docs/out.md", "nope"),
            guardian_reviewer_decision_response("deny", "high", "low", "none"),
            guardian_model_final_response("denied three"),
            guardian_model_write_response("write-doc", "docs/out.md", "nope"),
            guardian_model_final_response("circuit blocked"),
        ];
        let (base_url, server) = start_mock_openai_server(responses);
        let key_name = format!("OPPI_GUARDIAN_CIRCUIT_{}_API_KEY", std::process::id());
        unsafe { std::env::set_var(&key_name, "test-key") };
        let mut runtime = Runtime::new();
        let thread = guardian_auto_review_thread(&mut runtime, &root);

        for _ in 0..3 {
            let result = guardian_run_write_turn(
                &mut runtime,
                thread.id.clone(),
                &root,
                base_url.clone(),
                key_name.clone(),
                "write-doc",
                "docs/out.md",
            );
            assert_eq!(result.turn.status, TurnStatus::Completed);
        }
        let blocked = guardian_run_write_turn(
            &mut runtime,
            thread.id,
            &root,
            base_url,
            key_name.clone(),
            "write-doc",
            "docs/out.md",
        );
        unsafe { std::env::remove_var(&key_name) };
        let requests = server.join().unwrap();

        assert_eq!(blocked.turn.status, TurnStatus::Completed);
        assert_eq!(guardian_reviewer_request_count(&requests), 3);
        let records = guardian_review_completed_records(&blocked.events);
        assert!(records.iter().any(|record| {
            record.tool_call_id == "write-doc"
                && record.status == GuardianReviewStatus::CircuitBlocked
        }));
        assert!(!root.join("docs/out.md").exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn guardian_auto_review_denies_protected_tool_without_user_approval() {
        let root =
            std::env::temp_dir().join(format!("oppi-guardian-protected-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let responses = vec![
            guardian_model_write_response("write-env", ".env", "SECRET=1"),
            guardian_model_final_response("protected denied"),
        ];
        let (base_url, server) = start_mock_openai_server(responses);
        let key_name = format!("OPPI_GUARDIAN_PROTECTED_{}_API_KEY", std::process::id());
        unsafe { std::env::set_var(&key_name, "test-key") };
        let mut runtime = Runtime::new();
        let thread = guardian_auto_review_thread(&mut runtime, &root);
        let result = guardian_run_write_turn(
            &mut runtime,
            thread.id,
            &root,
            base_url,
            key_name.clone(),
            "write-env",
            ".env",
        );
        unsafe { std::env::remove_var(&key_name) };

        assert_eq!(result.turn.status, TurnStatus::Completed);
        assert!(
            !result
                .events
                .iter()
                .any(|event| matches!(event.kind, EventKind::ApprovalRequested { .. }))
        );
        let denied = result
            .events
            .iter()
            .find_map(|event| match &event.kind {
                EventKind::ToolCallCompleted { result } if result.call_id == "write-env" => {
                    Some(result)
                }
                _ => None,
            })
            .expect("denied tool result");
        assert_eq!(denied.status, ToolResultStatus::Denied);
        assert!(
            denied
                .output
                .as_deref()
                .unwrap_or_default()
                .contains(r#""outcome":"deny""#)
        );
        let requests = server.join().unwrap();
        assert_eq!(guardian_reviewer_request_count(&requests), 0);
        assert!(!root.join(".env").exists());
        let _ = fs::remove_dir_all(&root);
    }

    fn guardian_state_call(id: &str, path: &str) -> ToolCall {
        ToolCall {
            id: id.to_string(),
            name: "write_file".to_string(),
            namespace: Some("oppi".to_string()),
            arguments: json!({ "path": path, "content": "ok" }),
        }
    }

    fn guardian_state_record(
        id: &str,
        signature: &str,
        circuit_key: &str,
        risk_level: RiskLevel,
        user_authorization: GuardianAuthorizationLevel,
        cache_scope: Option<GuardianCacheScope>,
        outcome: GuardianReviewOutcome,
    ) -> GuardianReviewRecord {
        GuardianReviewRecord {
            id: id.to_string(),
            timestamp: "2026-05-26T12:00:00Z".to_string(),
            status: match outcome {
                GuardianReviewOutcome::Allow => GuardianReviewStatus::Approved,
                GuardianReviewOutcome::Deny => GuardianReviewStatus::Denied,
            },
            mode: PermissionMode::AutoReview,
            tool_name: "write_file".to_string(),
            tool_call_id: format!("{id}-call"),
            signature: signature.to_string(),
            circuit_key: circuit_key.to_string(),
            summary: "write docs file".to_string(),
            risk: GuardianRiskSummary {
                category: "workspace-write".to_string(),
                reason: "writes a project file".to_string(),
                protected_hits: Vec::new(),
                paths: vec!["docs/out.md".to_string()],
                hints: Vec::new(),
            },
            decision: Some(oppi_protocol::GuardianReviewDecision {
                outcome,
                risk_level,
                user_authorization,
                rationale: "reviewed by Guardian".to_string(),
                cache_scope,
            }),
            reviewer_model: Some("openai-codex/gpt-5.5".to_string()),
            cached_from: None,
            strict_json: Some(r#"{"outcome":"allow"}"#.to_string()),
        }
    }

    #[test]
    fn guardian_state_caches_only_low_risk_authorized_exact_approvals() {
        let mut runtime = Runtime::new();
        assert_eq!(
            runtime.guardian_settings.timeout_ms,
            DEFAULT_GUARDIAN_REVIEW_TIMEOUT_MS
        );
        assert_eq!(runtime.guardian_settings.reviewer_model, None);
        let call = guardian_state_call("write-doc", "docs/out.md");
        let risk = GuardianRiskSummary {
            category: "workspace-write".to_string(),
            reason: "writes a project file".to_string(),
            protected_hits: Vec::new(),
            paths: vec!["docs/out.md".to_string()],
            hints: Vec::new(),
        };
        let signature = guardian_tool_signature(&call);
        assert!(signature.starts_with("oppi::write_file\n"));
        assert!(signature.contains(r#""path":"docs/out.md""#));
        assert_eq!(
            guardian_circuit_key(&call, &risk),
            "oppi::write_file:workspace-write:docs/out.md"
        );

        let low_high_exact = guardian_state_record(
            "review-cache",
            &signature,
            "circuit-cache",
            RiskLevel::Low,
            GuardianAuthorizationLevel::High,
            Some(GuardianCacheScope::Exact),
            GuardianReviewOutcome::Allow,
        );
        let medium_high_exact = guardian_state_record(
            "review-medium-risk",
            "sig-medium-risk",
            "circuit-medium-risk",
            RiskLevel::Medium,
            GuardianAuthorizationLevel::High,
            Some(GuardianCacheScope::Exact),
            GuardianReviewOutcome::Allow,
        );
        let low_unknown_exact = guardian_state_record(
            "review-unknown-auth",
            "sig-unknown-auth",
            "circuit-unknown-auth",
            RiskLevel::Low,
            GuardianAuthorizationLevel::Unknown,
            Some(GuardianCacheScope::Exact),
            GuardianReviewOutcome::Allow,
        );
        let low_high_none = guardian_state_record(
            "review-no-cache-scope",
            "sig-no-cache-scope",
            "circuit-no-cache-scope",
            RiskLevel::Low,
            GuardianAuthorizationLevel::High,
            None,
            GuardianReviewOutcome::Allow,
        );

        assert!(guardian_can_cache(&low_high_exact));
        assert!(!guardian_can_cache(&medium_high_exact));
        assert!(!guardian_can_cache(&low_unknown_exact));
        assert!(!guardian_can_cache(&low_high_none));

        runtime.guardian_remember_cache(&low_high_exact);
        runtime.guardian_remember_cache(&medium_high_exact);
        runtime.guardian_remember_cache(&low_unknown_exact);
        runtime.guardian_remember_cache(&low_high_none);

        assert_eq!(
            runtime
                .guardian_cached_record(&signature)
                .map(|record| record.id),
            Some("review-cache".to_string())
        );
        assert!(runtime.guardian_cached_record("sig-medium-risk").is_none());
        assert!(runtime.guardian_cached_record("sig-unknown-auth").is_none());
        assert!(
            runtime
                .guardian_cached_record("sig-no-cache-scope")
                .is_none()
        );
    }

    #[test]
    fn guardian_state_opens_circuit_after_three_similar_denials() {
        let mut runtime = Runtime::new();
        let circuit_key = "oppi::write_file:workspace-write:docs/out.md";

        assert!(!runtime.guardian_circuit_open(circuit_key));
        runtime.guardian_record_denial(circuit_key);
        assert!(!runtime.guardian_circuit_open(circuit_key));
        runtime.guardian_record_denial(circuit_key);
        assert!(!runtime.guardian_circuit_open(circuit_key));
        runtime.guardian_record_denial(circuit_key);
        assert!(runtime.guardian_circuit_open(circuit_key));
    }

    #[test]
    fn guardian_state_replays_review_history_cache_and_circuit() {
        let approved = guardian_state_record(
            "review-allow",
            "sig-allow",
            "circuit-allow",
            RiskLevel::Low,
            GuardianAuthorizationLevel::High,
            Some(GuardianCacheScope::Exact),
            GuardianReviewOutcome::Allow,
        );
        let mut reviewing = approved.clone();
        reviewing.status = GuardianReviewStatus::Reviewing;
        reviewing.decision = None;
        let denials = (1..=3)
            .map(|index| {
                guardian_state_record(
                    &format!("review-deny-{index}"),
                    &format!("sig-deny-{index}"),
                    "circuit-deny",
                    RiskLevel::High,
                    GuardianAuthorizationLevel::Low,
                    None,
                    GuardianReviewOutcome::Deny,
                )
            })
            .collect::<Vec<_>>();
        let mut events = vec![
            Event {
                id: 1,
                thread_id: "thread-1".to_string(),
                turn_id: Some("turn-1".to_string()),
                created_at_ms: 1,
                kind: EventKind::PermissionReviewStarted { record: reviewing },
            },
            Event {
                id: 2,
                thread_id: "thread-1".to_string(),
                turn_id: Some("turn-1".to_string()),
                created_at_ms: 2,
                kind: EventKind::PermissionReviewCompleted {
                    record: approved.clone(),
                },
            },
        ];
        for (offset, denial) in denials.into_iter().enumerate() {
            events.push(Event {
                id: offset as u64 + 3,
                thread_id: "thread-1".to_string(),
                turn_id: Some("turn-1".to_string()),
                created_at_ms: offset as u64 + 3,
                kind: EventKind::PermissionReviewCompleted { record: denial },
            });
        }

        let replayed = Runtime::replay_events(&events).unwrap();

        assert_eq!(replayed.guardian_review_records.len(), 4);
        assert_eq!(
            replayed
                .guardian_review_records
                .front()
                .map(|record| record.status),
            Some(GuardianReviewStatus::Approved)
        );
        assert_eq!(
            replayed
                .guardian_cached_record("sig-allow")
                .map(|record| record.id),
            Some("review-allow".to_string())
        );
        assert!(replayed.guardian_circuit_open("circuit-deny"));
    }

    #[test]
    fn model_registry_lists_models_json_entries_with_selection_scope_and_diagnostics() {
        let _lock = TEST_ENV_LOCK.lock().unwrap();
        let root =
            std::env::temp_dir().join(format!("oppi-core-model-registry-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let models_path = root.join("models.json");
        fs::write(
            &models_path,
            r#"{
              "providers": {
                "openai-codex": {
                  "apiKey": "!do-not-run-during-listing",
                  "models": [
                    { "id": "gpt-local-coder", "name": "Local Coder" }
                  ]
                },
                "google-proxy": {
                  "baseUrl": "https://generativelanguage.googleapis.com/v1beta",
                  "api": "google-generative-ai",
                  "apiKey": "GEMINI_API_KEY",
                  "models": [
                    { "id": "gemma-4-31b-it" }
                  ]
                }
              }
            }"#,
        )
        .unwrap();
        let _models_json = EnvVarGuard::set("OPPI_MODELS_JSON_PATH", &models_path);

        let runtime = Runtime::new();
        let result = runtime.list_model_registry(ModelRegistryListParams {
            thread_id: None,
            provider: "openai-codex".to_string(),
            selected_model_id: Some(GPT_MAIN_DEFAULT_MODEL.to_string()),
            scope: vec!["openai-codex/gpt-local-coder".to_string()],
            available_only: true,
        });

        assert_eq!(result.provider, "openai-codex");
        assert!(result.available_only);
        assert_eq!(result.scope, vec!["openai-codex/gpt-local-coder"]);
        assert_eq!(
            result
                .selected_model
                .as_ref()
                .map(|model| model.id.as_str()),
            Some(GPT_MAIN_DEFAULT_MODEL)
        );
        assert_eq!(result.items[0].id, GPT_MAIN_DEFAULT_MODEL);
        assert!(result.items.iter().any(|model| {
            model.id == "gpt-local-coder"
                && model.display_name == "Local Coder"
                && model.provider == "openai-codex"
        }));
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.level == DiagnosticLevel::Warning
                && diagnostic.message.contains("unsupported provider API")
                && diagnostic.message.contains("google-generative-ai")
        }));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn model_registry_exposes_context_window_from_models_json() {
        let _lock = TEST_ENV_LOCK.lock().unwrap();
        let root = std::env::temp_dir().join(format!(
            "oppi-core-model-registry-context-window-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let models_path = root.join("models.json");
        fs::write(
            &models_path,
            r#"{
              "providers": {
                "openai-codex": {
                  "apiKey": "test-token",
                  "modelOverrides": {
                    "gpt-5.5": {
                      "name": "GPT 5.5 Huge",
                      "contextWindow": 400000
                    }
                  },
                  "models": [
                    {
                      "id": "gpt-local-long",
                      "name": "Local Long",
                      "contextWindow": 131000
                    }
                  ]
                }
              }
            }"#,
        )
        .unwrap();
        let _models_json = EnvVarGuard::set("OPPI_MODELS_JSON_PATH", &models_path);

        let runtime = Runtime::new();
        let result = runtime.list_model_registry(ModelRegistryListParams {
            thread_id: None,
            provider: "openai-codex".to_string(),
            selected_model_id: Some(GPT_MAIN_DEFAULT_MODEL.to_string()),
            scope: Vec::new(),
            available_only: true,
        });

        assert_eq!(
            result
                .selected_model
                .as_ref()
                .and_then(|model| model.context_window),
            Some(400000)
        );
        assert!(result.items.iter().any(|model| {
            model.id == "gpt-local-long"
                && model.display_name == "Local Long"
                && model.context_window == Some(131000)
        }));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn permission_review_state_settings_history_and_clear_session_round_trip() {
        let mut runtime = Runtime::new();
        let thread = thread(&mut runtime);
        let thread_id = thread.id.clone();
        let settings = runtime
            .update_permission_review_settings(PermissionReviewSettingsUpdateParams {
                thread_id: Some(thread_id.clone()),
                settings: GuardianReviewSettings {
                    timeout_ms: 30_000,
                    reviewer_model: Some("openai-codex/gpt-5.5".to_string()),
                },
            })
            .unwrap()
            .settings;

        assert_eq!(settings.timeout_ms, 30_000);
        assert_eq!(
            runtime
                .permission_review_settings()
                .settings
                .reviewer_model
                .as_deref(),
            Some("openai-codex/gpt-5.5")
        );

        let record = guardian_state_record(
            "review-settings",
            "sig-settings",
            "circuit-settings",
            RiskLevel::Low,
            GuardianAuthorizationLevel::High,
            Some(GuardianCacheScope::Exact),
            GuardianReviewOutcome::Allow,
        );
        runtime.guardian_remember_review_record(record);

        let history = runtime
            .permission_review_history(PermissionReviewHistoryParams {
                thread_id: thread_id.clone(),
                limit: Some(10),
            })
            .unwrap();
        assert_eq!(history.records.len(), 1);
        assert_eq!(history.records[0].id, "review-settings");
        assert!(runtime.guardian_cached_record("sig-settings").is_some());

        let cleared = runtime
            .clear_permission_review_session(PermissionReviewClearSessionParams {
                thread_id: thread_id.clone(),
            })
            .unwrap();
        assert_eq!(cleared.cleared_reviews, 1);
        assert_eq!(cleared.cleared_cache_entries, 1);
        assert_eq!(
            runtime
                .permission_review_history(PermissionReviewHistoryParams {
                    thread_id,
                    limit: Some(10),
                })
                .unwrap()
                .records
                .len(),
            0
        );
        assert!(runtime.guardian_cached_record("sig-settings").is_none());
    }

    fn guardian_reviewer_turn(runtime: &mut Runtime) -> Turn {
        let thread = thread(runtime);
        Turn {
            id: "turn-reviewer".to_string(),
            thread_id: thread.id,
            status: TurnStatus::Running,
            phase: TurnPhase::Loop,
            parent_turn_id: None,
        }
    }

    fn guardian_reviewer_parent_config(
        base_url: String,
        key_name: String,
    ) -> DirectModelProviderConfig {
        DirectModelProviderConfig {
            kind: DirectModelProviderKind::OpenAiCompatible,
            model: "mock-reviewer".to_string(),
            base_url: Some(base_url),
            api_key_env: Some(key_name),
            system_prompt: Some("Parent prompt".to_string()),
            temperature: Some(0.7),
            reasoning_effort: Some("high".to_string()),
            max_output_tokens: Some(4096),
            stream: true,
        }
    }

    fn guardian_reviewer_risk() -> GuardianRiskSummary {
        GuardianRiskSummary {
            category: "workspace-write".to_string(),
            reason: "writes a project file".to_string(),
            protected_hits: Vec::new(),
            paths: vec!["docs/out.md".to_string()],
            hints: vec!["user requested docs update".to_string()],
        }
    }

    #[test]
    fn guardian_reviewer_tools_are_not_visible_to_main_model() {
        let registry = default_tool_registry();
        for name in ["oppi_review_read", "oppi_review_ls", "oppi_review_grep"] {
            assert!(
                registry.get(Some("oppi"), name).is_some(),
                "reviewer helper should remain locally registered: {name}"
            );
        }

        let names = model_visible_tool_definitions(&registry)
            .into_iter()
            .map(|definition| definition.name)
            .collect::<BTreeSet<_>>();
        for name in ["read_file", "search_files", "write_file", "edit_file"] {
            assert!(names.contains(name), "main model should see oppi::{name}");
        }
        for name in ["oppi_review_read", "oppi_review_ls", "oppi_review_grep"] {
            assert!(
                !names.contains(name),
                "main model must not see reviewer-only tool {name}"
            );
        }
    }

    #[test]
    fn guardian_review_prompt_includes_pi_style_output_contract() {
        let mut runtime = Runtime::new();
        let turn = guardian_reviewer_turn(&mut runtime);
        let parent = DirectModelProviderConfig {
            kind: DirectModelProviderKind::OpenAiCompatible,
            model: "mock-main".to_string(),
            base_url: Some("http://127.0.0.1:3456/v1".to_string()),
            api_key_env: Some("OPPI_TEST_KEY".to_string()),
            system_prompt: Some("Parent system prompt with current behavior rules.".to_string()),
            temperature: None,
            reasoning_effort: None,
            max_output_tokens: None,
            stream: false,
        };
        let prompt = runtime.guardian_review_prompt(
            &turn,
            Some(&parent),
            &guardian_state_call("write-doc", "docs/out.md"),
            &guardian_reviewer_risk(),
        );

        assert!(prompt.contains("Return exactly this JSON shape with no markdown"));
        assert!(prompt.contains("Current OPPi/Pi system prompt excerpt:"));
        assert!(prompt.contains("Parent system prompt with current behavior rules."));
        for field in [
            r#""outcome": "allow" | "deny""#,
            r#""risk_level": "low" | "medium" | "high" | "critical""#,
            r#""user_authorization": "unknown" | "low" | "medium" | "high""#,
            r#""rationale": "one concise sentence grounded in the conversation and tool call""#,
            r#""cache_scope": "none" | "exact""#,
        ] {
            assert!(
                prompt.contains(field),
                "missing Guardian output contract field: {field}"
            );
        }
    }

    #[test]
    fn permission_review_context_describes_guardian_prompt_inputs() {
        let mut runtime = Runtime::new();
        let thread = thread(&mut runtime);
        let context = runtime
            .permission_review_context(PermissionReviewContextParams {
                thread_id: thread.id,
            })
            .unwrap();

        assert!(context.reviewer_provider_policy.contains("non-Meridian"));
        assert!(
            context
                .prompt_context
                .iter()
                .any(|item| item.contains("parent system prompt excerpt"))
        );
        assert!(
            context
                .prompt_context
                .iter()
                .any(|item| item.contains("recent Guardian records"))
        );
        assert_eq!(
            context.reviewer_tools,
            vec!["oppi_review_read", "oppi_review_ls", "oppi_review_grep"]
        );
        assert!(
            context
                .hard_denies
                .iter()
                .any(|item| item.contains("protected"))
        );
        assert_eq!(context.output_contract["outcome"], json!("allow|deny"));
        assert!(
            context
                .prompt_skeleton
                .contains("Current OPPi/Pi system prompt excerpt")
        );
        assert!(context.prompt_skeleton.contains("Proposed tool call JSON"));
    }

    #[test]
    fn guardian_reviewer_rejects_meridian_backed_provider() {
        let parent = DirectModelProviderConfig {
            kind: DirectModelProviderKind::OpenAiCompatible,
            model: "claude-sonnet-4-6".to_string(),
            base_url: Some("http://127.0.0.1:3456/v1".to_string()),
            api_key_env: Some(MERIDIAN_API_KEY_ENV.to_string()),
            system_prompt: None,
            temperature: None,
            reasoning_effort: None,
            max_output_tokens: None,
            stream: false,
        };
        let error = guardian_reviewer_provider_config(
            Some(&parent),
            &GuardianReviewSettings {
                timeout_ms: DEFAULT_GUARDIAN_REVIEW_TIMEOUT_MS,
                reviewer_model: None,
            },
        )
        .unwrap_err();

        assert_eq!(error.code, "provider_auth_missing");
        assert!(error.message.contains("non-Meridian reviewer provider"));
    }

    #[test]
    fn guardian_reviewer_allows_non_meridian_openai_compatible_decision() {
        let response = json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": r#"{"outcome":"allow","risk_level":"low","user_authorization":"high","rationale":"User explicitly asked to update docs/out.md.","cache_scope":"exact"}"#
                }
            }]
        })
        .to_string();
        let (base_url, server) = start_mock_openai_server(vec![response]);
        let key_name = format!(
            "OPPI_GUARDIAN_REVIEWER_ALLOW_{}_API_KEY",
            std::process::id()
        );
        unsafe { std::env::set_var(&key_name, "test-key") };
        let mut runtime = Runtime::new();
        let turn = guardian_reviewer_turn(&mut runtime);
        let parent = guardian_reviewer_parent_config(base_url, key_name.clone());
        let record = runtime.run_guardian_reviewer(
            &turn,
            Some(&parent),
            &guardian_state_call("write-doc", "docs/out.md"),
            guardian_reviewer_risk(),
        );
        unsafe { std::env::remove_var(&key_name) };
        let requests = server.join().unwrap();

        assert_eq!(record.status, GuardianReviewStatus::Approved);
        let decision = record.decision.expect("review decision");
        assert_eq!(decision.outcome, GuardianReviewOutcome::Allow);
        assert_eq!(decision.risk_level, RiskLevel::Low);
        assert_eq!(
            decision.user_authorization,
            GuardianAuthorizationLevel::High
        );
        assert_eq!(decision.cache_scope, Some(GuardianCacheScope::Exact));
        assert_eq!(record.reviewer_model.as_deref(), Some("mock-reviewer"));
        assert_eq!(requests.len(), 1);
        assert!(requests[0].contains("Return exactly one JSON object"));
        assert!(requests[0].contains("authorization: Bearer test-key"));
    }

    #[test]
    fn guardian_reviewer_denies_malformed_json_fail_closed() {
        let response = json!({
            "choices": [{
                "message": { "role": "assistant", "content": "I think this is safe." }
            }]
        })
        .to_string();
        let (base_url, server) = start_mock_openai_server(vec![response]);
        let key_name = format!("OPPI_GUARDIAN_REVIEWER_DENY_{}_API_KEY", std::process::id());
        unsafe { std::env::set_var(&key_name, "test-key") };
        let mut runtime = Runtime::new();
        let turn = guardian_reviewer_turn(&mut runtime);
        let parent = guardian_reviewer_parent_config(base_url, key_name.clone());
        let record = runtime.run_guardian_reviewer(
            &turn,
            Some(&parent),
            &guardian_state_call("write-doc", "docs/out.md"),
            guardian_reviewer_risk(),
        );
        unsafe { std::env::remove_var(&key_name) };
        let requests = server.join().unwrap();

        assert_eq!(record.status, GuardianReviewStatus::FailedClosed);
        let decision = record.decision.expect("review decision");
        assert_eq!(decision.outcome, GuardianReviewOutcome::Deny);
        assert_eq!(decision.risk_level, RiskLevel::Critical);
        assert_eq!(
            decision.user_authorization,
            GuardianAuthorizationLevel::Unknown
        );
        assert!(
            decision.rationale.starts_with("Auto-review failed closed:"),
            "{}",
            decision.rationale
        );
        assert_eq!(requests.len(), 1);
    }

    #[test]
    fn agentic_loop_guard_aborts_after_bounded_continuations() {
        let mut runtime = Runtime::new();
        let thread = thread(&mut runtime);
        let result = runtime
            .run_agentic_turn(AgenticTurnParams {
                thread_id: thread.id,
                input: "loop once only".to_string(),
                execution_mode: ExecutionMode::Blocking,
                follow_up: None,
                sandbox_policy: None,
                model_steps: vec![
                    ScriptedModelStep {
                        assistant_deltas: Vec::new(),
                        tool_calls: vec![agentic_call("echo-1", "one")],
                        tool_results: Vec::new(),
                        final_response: false,
                    },
                    ScriptedModelStep {
                        assistant_deltas: Vec::new(),
                        tool_calls: vec![agentic_call("echo-2", "two")],
                        tool_results: Vec::new(),
                        final_response: false,
                    },
                ],
                model_provider: None,
                tool_definitions: Vec::new(),
                approved_tool_call_ids: Vec::new(),
                cancellation: None,
                max_continuations: Some(0),
            })
            .unwrap();
        assert_eq!(result.turn.status, TurnStatus::Aborted);
        assert!(
            result
                .events
                .iter()
                .any(|event| matches!(event.kind, EventKind::TurnAborted { .. }))
        );
    }

    #[test]
    fn agentic_waiting_turn_can_be_cancelled_with_interrupt() {
        let mut runtime = Runtime::new();
        let thread = thread(&mut runtime);
        let mut call = agentic_call("echo-cancel", "cancel");
        call.arguments = json!({ "output": "cancel", "requireApproval": true });
        let paused = runtime
            .run_agentic_turn(AgenticTurnParams {
                thread_id: thread.id.clone(),
                input: "cancel me".to_string(),
                execution_mode: ExecutionMode::Blocking,
                follow_up: None,
                sandbox_policy: None,
                model_steps: vec![ScriptedModelStep {
                    assistant_deltas: Vec::new(),
                    tool_calls: vec![call],
                    tool_results: Vec::new(),
                    final_response: false,
                }],
                model_provider: None,
                tool_definitions: Vec::new(),
                approved_tool_call_ids: Vec::new(),
                cancellation: None,
                max_continuations: Some(4),
            })
            .unwrap();
        assert_eq!(paused.turn.status, TurnStatus::WaitingForApproval);
        let interrupted = runtime
            .interrupt_turn(&thread.id, &paused.turn.id, "user cancelled".to_string())
            .unwrap();
        assert_eq!(runtime.turns[&paused.turn.id].status, TurnStatus::Aborted);
        assert!(
            interrupted
                .events
                .iter()
                .any(|event| matches!(event.kind, EventKind::TurnInterrupted { .. }))
        );
    }

    #[test]
    fn thread_goal_set_get_clear_round_trips_through_runtime() {
        let mut runtime = Runtime::new();
        let thread_id = runtime
            .start_thread(ThreadStartParams {
                project: project(),
                title: Some("Goal test".to_string()),
            })
            .thread
            .id;

        let set = runtime
            .set_thread_goal(ThreadGoalSetParams {
                thread_id: thread_id.clone(),
                objective: Some("Ship native goal mode".to_string()),
                status: Some(ThreadGoalStatus::Active),
                token_budget: Some(Some(10_000)),
            })
            .unwrap();

        assert_eq!(set.goal.objective, "Ship native goal mode");
        assert_eq!(set.goal.status, ThreadGoalStatus::Active);
        assert_eq!(set.goal.token_budget, Some(10_000));
        assert_eq!(
            runtime
                .get_thread_goal(ThreadGoalGetParams {
                    thread_id: thread_id.clone()
                })
                .unwrap()
                .goal
                .unwrap()
                .objective,
            "Ship native goal mode"
        );
        assert!(
            runtime
                .clear_thread_goal(ThreadGoalClearParams { thread_id })
                .unwrap()
                .cleared
        );
    }

    #[test]
    fn replay_restores_thread_goal_state() {
        let mut runtime = Runtime::new();
        let thread_id = runtime
            .start_thread(ThreadStartParams {
                project: project(),
                title: None,
            })
            .thread
            .id;
        runtime
            .set_thread_goal(ThreadGoalSetParams {
                thread_id: thread_id.clone(),
                objective: Some("Persist this goal".to_string()),
                status: Some(ThreadGoalStatus::Active),
                token_budget: Some(None),
            })
            .unwrap();

        let events = runtime
            .events_after(EventsListParams {
                thread_id: thread_id.clone(),
                after: 0,
                limit: None,
            })
            .unwrap()
            .events;
        let replayed = Runtime::replay_events(&events).unwrap();
        assert_eq!(
            replayed
                .get_thread_goal(ThreadGoalGetParams { thread_id })
                .unwrap()
                .goal
                .unwrap()
                .objective,
            "Persist this goal"
        );
    }

    #[test]
    fn thread_rename_and_archive_survive_replay() {
        let mut runtime = Runtime::new();
        let started = runtime.start_thread(ThreadStartParams {
            project: project(),
            title: Some("Original".to_string()),
        });
        let thread_id = started.thread.id.clone();
        let renamed = runtime
            .rename_thread(&thread_id, "Renamed session".to_string())
            .unwrap();
        let archived = runtime.archive_thread(&thread_id).unwrap();

        assert_eq!(renamed.thread.title.as_deref(), Some("Renamed session"));
        assert_eq!(archived.thread.status, ThreadStatus::Archived);

        let events = started
            .events
            .into_iter()
            .chain(renamed.events)
            .chain(archived.events)
            .collect::<Vec<_>>();
        let replayed = Runtime::replay_events(&events).unwrap();
        let thread = replayed.thread(&thread_id).unwrap();
        assert_eq!(thread.title.as_deref(), Some("Renamed session"));
        assert_eq!(thread.status, ThreadStatus::Archived);
    }

    #[test]
    fn goal_accounting_marks_budget_limited_when_tokens_cross_budget() {
        let mut goal = ThreadGoal {
            thread_id: "thread-1".to_string(),
            objective: "Stay within budget".to_string(),
            status: ThreadGoalStatus::Active,
            token_budget: Some(100),
            tokens_used: 90,
            time_used_seconds: 0,
            created_at_ms: 1,
            updated_at_ms: 1,
        };

        apply_goal_accounting_delta(&mut goal, 15, 30, 31);
        assert_eq!(goal.tokens_used, 105);
        assert_eq!(goal.time_used_seconds, 30);
        assert_eq!(goal.status, ThreadGoalStatus::BudgetLimited);
    }

    #[test]
    fn goal_accounting_uses_openai_compatible_total_tokens() {
        let response = json!({
            "choices": [{
                "message": { "role": "assistant", "content": "done" }
            }],
            "usage": { "prompt_tokens": 7, "completion_tokens": 8, "total_tokens": 15 }
        })
        .to_string();
        let (base_url, server) = start_mock_openai_server(vec![response]);
        let key_name = format!("OPPI_GOAL_ACCOUNTING_TEST_{}_API_KEY", std::process::id());
        unsafe { std::env::set_var(&key_name, "test-key") };
        let mut runtime = Runtime::new();
        let thread = thread(&mut runtime);
        runtime
            .set_thread_goal(ThreadGoalSetParams {
                thread_id: thread.id.clone(),
                objective: Some("Stay inside budget".to_string()),
                status: Some(ThreadGoalStatus::Active),
                token_budget: Some(Some(10)),
            })
            .unwrap();

        let result = runtime
            .run_agentic_turn(AgenticTurnParams {
                thread_id: thread.id.clone(),
                input: "finish within budget".to_string(),
                execution_mode: ExecutionMode::Blocking,
                follow_up: None,
                sandbox_policy: None,
                model_steps: Vec::new(),
                model_provider: Some(DirectModelProviderConfig {
                    kind: DirectModelProviderKind::OpenAiCompatible,
                    model: "mock".to_string(),
                    base_url: Some(base_url),
                    api_key_env: Some(key_name.clone()),
                    system_prompt: Some("You are OPPi.".to_string()),
                    temperature: None,
                    reasoning_effort: None,
                    max_output_tokens: None,
                    stream: false,
                }),
                tool_definitions: Vec::new(),
                approved_tool_call_ids: Vec::new(),
                cancellation: None,
                max_continuations: Some(1),
            })
            .unwrap();
        unsafe { std::env::remove_var(&key_name) };
        let _ = server.join().unwrap();

        assert_eq!(result.turn.status, TurnStatus::Completed);
        let goal = runtime
            .get_thread_goal(ThreadGoalGetParams {
                thread_id: thread.id,
            })
            .unwrap()
            .goal
            .unwrap();
        assert_eq!(goal.tokens_used, 15);
        assert_eq!(goal.status, ThreadGoalStatus::BudgetLimited);
        assert!(result.events.iter().any(|event| match &event.kind {
            EventKind::ThreadGoalUpdated { goal } => goal.status == ThreadGoalStatus::BudgetLimited,
            _ => false,
        }));
        let provider_diagnostic = result
            .events
            .iter()
            .find_map(|event| match &event.kind {
                EventKind::Diagnostic { diagnostic }
                    if diagnostic
                        .metadata
                        .get("component")
                        .is_some_and(|value| value == "provider") =>
                {
                    Some(diagnostic)
                }
                _ => None,
            })
            .expect("provider diagnostic");
        assert_eq!(
            provider_diagnostic
                .metadata
                .get("inputTokens")
                .map(String::as_str),
            Some("7")
        );
        assert_eq!(
            provider_diagnostic
                .metadata
                .get("outputTokens")
                .map(String::as_str),
            Some("8")
        );
        assert_eq!(
            provider_diagnostic
                .metadata
                .get("totalTokens")
                .map(String::as_str),
            Some("15")
        );
    }

    fn test_goal(objective: &str, status: ThreadGoalStatus) -> ThreadGoal {
        ThreadGoal {
            thread_id: "thread-1".to_string(),
            objective: objective.to_string(),
            status,
            token_budget: Some(1_000),
            tokens_used: 250,
            time_used_seconds: 30,
            created_at_ms: 1,
            updated_at_ms: 2,
        }
    }

    #[test]
    fn goal_continuation_prompt_escapes_untrusted_objective() {
        let goal = test_goal("<delete everything>", ThreadGoalStatus::Active);
        let prompt = render_goal_continuation_prompt(&goal);
        assert!(prompt.contains("&lt;delete everything&gt;"));
        assert!(prompt.contains("<untrusted_objective>"));
    }

    #[test]
    fn goal_budget_limit_prompt_does_not_invite_new_work() {
        let goal = test_goal("Finish the project", ThreadGoalStatus::BudgetLimited);
        let prompt = render_goal_budget_limit_prompt(&goal);
        assert!(prompt.contains("do not start new substantive work"));
        assert!(prompt.contains("Do not call update_goal unless the goal is actually complete"));
    }

    #[test]
    fn thread_goal_continuation_claim_renders_once_and_pauses_at_cap() {
        let mut runtime = Runtime::new();
        let thread = thread(&mut runtime);
        runtime
            .set_thread_goal(ThreadGoalSetParams {
                thread_id: thread.id.clone(),
                objective: Some("Ship continuation".to_string()),
                status: Some(ThreadGoalStatus::Active),
                token_budget: None,
            })
            .unwrap();

        let first = runtime
            .next_thread_goal_continuation(ThreadGoalContinuationParams {
                thread_id: thread.id.clone(),
                max_continuations: Some(1),
            })
            .unwrap();
        assert_eq!(first.continuation, Some(1));
        assert!(
            first
                .prompt
                .as_deref()
                .unwrap_or_default()
                .contains("Ship continuation")
        );

        let blocked = runtime
            .next_thread_goal_continuation(ThreadGoalContinuationParams {
                thread_id: thread.id.clone(),
                max_continuations: Some(1),
            })
            .unwrap();
        assert_eq!(blocked.prompt, None);
        assert_eq!(blocked.continuation, Some(2));
        assert_eq!(
            blocked.goal.as_ref().map(|goal| goal.status),
            Some(ThreadGoalStatus::Paused)
        );
        assert!(
            blocked
                .blocked_reason
                .as_deref()
                .unwrap_or_default()
                .contains("continuation guard")
        );
    }

    #[test]
    fn replay_rebuilds_events_and_runtime_counters() {
        let mut runtime = Runtime::new();
        let thread = thread(&mut runtime);
        runtime
            .start_turn(TurnStartParams {
                thread_id: thread.id.clone(),
                input: "persist me".to_string(),
                assistant_response: Some("persisted".to_string()),
                simulated_tool: None,
                requested_continuations: 0,
                stop_hook_feedback: None,
                defer_completion: false,
            })
            .unwrap();
        let events = runtime
            .events_after(EventsListParams {
                thread_id: thread.id.clone(),
                after: 0,
                limit: None,
            })
            .unwrap()
            .events;

        let mut replayed = Runtime::replay_events(&events).unwrap();
        assert_eq!(
            replayed
                .events_after(EventsListParams {
                    thread_id: thread.id.clone(),
                    after: 0,
                    limit: None,
                })
                .unwrap()
                .events,
            events
        );
        let next = replayed
            .start_turn(TurnStartParams {
                thread_id: thread.id,
                input: "next".to_string(),
                assistant_response: None,
                simulated_tool: None,
                requested_continuations: 0,
                stop_hook_feedback: None,
                defer_completion: true,
            })
            .unwrap();
        assert_eq!(next.turn.id, "turn-2");
    }

    #[test]
    fn metrics_and_debug_bundle_are_redacted_and_count_runtime_state() {
        let mut runtime = Runtime::new();
        let thread = thread(&mut runtime);
        runtime
            .start_turn(TurnStartParams {
                thread_id: thread.id.clone(),
                input: "metrics".to_string(),
                assistant_response: Some("done".to_string()),
                simulated_tool: None,
                requested_continuations: 0,
                stop_hook_feedback: None,
                defer_completion: false,
            })
            .unwrap();
        runtime
            .request_question(
                &thread.id,
                QuestionRequest {
                    id: "q-custom".to_string(),
                    prompt: "Need input?".to_string(),
                    options: Vec::new(),
                },
            )
            .unwrap();

        let metrics = runtime.metrics();
        assert_eq!(metrics.thread_count, 1);
        assert_eq!(metrics.turn_count, 1);
        assert_eq!(metrics.pending_questions, 1);
        assert_eq!(metrics.turn_status_counts["completed"], 1);

        let bundle = runtime.debug_bundle(vec![Diagnostic {
            level: DiagnosticLevel::Info,
            message: "test diagnostic".to_string(),
            metadata: BTreeMap::new(),
        }]);
        assert!(bundle.redacted);
        assert_eq!(bundle.schema_version, 1);
        assert_eq!(bundle.metrics, metrics);
        assert_eq!(bundle.diagnostics.len(), 1);
    }

    #[test]
    fn events_after_applies_bounded_limit_and_cursor() {
        let mut runtime = Runtime::new();
        let thread = thread(&mut runtime);
        runtime
            .start_turn(TurnStartParams {
                thread_id: thread.id.clone(),
                input: "many events".to_string(),
                assistant_response: Some("done".to_string()),
                simulated_tool: None,
                requested_continuations: 0,
                stop_hook_feedback: None,
                defer_completion: false,
            })
            .unwrap();

        let limited = runtime
            .events_after(EventsListParams {
                thread_id: thread.id.clone(),
                after: 0,
                limit: Some(3),
            })
            .unwrap()
            .events;
        assert_eq!(limited.len(), 3);

        let after_first = runtime
            .events_after(EventsListParams {
                thread_id: thread.id,
                after: limited[0].id,
                limit: Some(2),
            })
            .unwrap()
            .events;
        assert_eq!(after_first.len(), 2);
        assert!(after_first.iter().all(|event| event.id > limited[0].id));
    }

    #[test]
    fn recovery_aborts_incomplete_replayed_turns() {
        let mut runtime = Runtime::new();
        let thread = thread(&mut runtime);
        let turn = runtime
            .start_turn(TurnStartParams {
                thread_id: thread.id.clone(),
                input: "crash mid-turn".to_string(),
                assistant_response: None,
                simulated_tool: None,
                requested_continuations: 0,
                stop_hook_feedback: None,
                defer_completion: true,
            })
            .unwrap()
            .turn;
        let events = runtime
            .events_after(EventsListParams {
                thread_id: thread.id.clone(),
                after: 0,
                limit: None,
            })
            .unwrap()
            .events;

        let mut replayed = Runtime::replay_events(&events).unwrap();
        let recovered = replayed.recover_incomplete_turns("crash recovery aborted incomplete turn");
        assert!(
            recovered
                .events
                .iter()
                .any(|event| matches!(event.kind, EventKind::TurnAborted { .. }))
        );
        assert_eq!(
            replayed
                .interrupt_turn(&thread.id, &turn.id, "late".to_string())
                .unwrap_err()
                .code,
            "turn_not_mutable"
        );
    }

    #[test]
    fn recovery_clears_pending_pauses_for_aborted_turns() {
        let mut runtime = Runtime::new();
        let thread = thread(&mut runtime);
        let approval_paused = runtime
            .run_agentic_turn(AgenticTurnParams {
                thread_id: thread.id.clone(),
                input: "pause for approval".to_string(),
                execution_mode: ExecutionMode::Blocking,
                follow_up: None,
                sandbox_policy: None,
                model_steps: vec![ScriptedModelStep {
                    assistant_deltas: Vec::new(),
                    tool_calls: vec![ToolCall {
                        id: "approval-pause".to_string(),
                        name: "echo".to_string(),
                        namespace: Some("oppi".to_string()),
                        arguments: json!({
                            "output": "approval pause",
                            "requireApproval": true
                        }),
                    }],
                    tool_results: Vec::new(),
                    final_response: false,
                }],
                model_provider: None,
                tool_definitions: Vec::new(),
                approved_tool_call_ids: Vec::new(),
                cancellation: None,
                max_continuations: Some(1),
            })
            .unwrap();
        assert_eq!(approval_paused.turn.status, TurnStatus::WaitingForApproval);

        let ask_paused = runtime
            .run_agentic_turn(AgenticTurnParams {
                thread_id: thread.id.clone(),
                input: "pause for user".to_string(),
                execution_mode: ExecutionMode::Blocking,
                follow_up: None,
                sandbox_policy: None,
                model_steps: vec![ScriptedModelStep {
                    assistant_deltas: Vec::new(),
                    tool_calls: vec![ToolCall {
                        id: "ask-pause".to_string(),
                        name: "ask_user".to_string(),
                        namespace: Some("oppi".to_string()),
                        arguments: json!({
                            "title": "Pick",
                            "questions": [{
                                "id": "path",
                                "question": "Which path?",
                                "options": [{ "id": "safe", "label": "Safe" }],
                                "defaultOptionId": "safe"
                            }]
                        }),
                    }],
                    tool_results: Vec::new(),
                    final_response: false,
                }],
                model_provider: None,
                tool_definitions: Vec::new(),
                approved_tool_call_ids: Vec::new(),
                cancellation: None,
                max_continuations: Some(1),
            })
            .unwrap();
        assert_eq!(ask_paused.turn.status, TurnStatus::WaitingForUser);
        assert_eq!(runtime.metrics().pending_approvals, 1);
        assert_eq!(runtime.metrics().pending_questions, 1);

        let events = runtime
            .events_after(EventsListParams {
                thread_id: thread.id.clone(),
                after: 0,
                limit: None,
            })
            .unwrap()
            .events;
        let mut replayed = Runtime::replay_events(&events).unwrap();
        replayed.recover_incomplete_turns("server restarted before turn completed");
        let metrics = replayed.metrics();
        assert_eq!(metrics.turn_status_counts.get("aborted").copied(), Some(2));
        assert_eq!(metrics.pending_approvals, 0);
        assert_eq!(metrics.pending_questions, 0);

        let recovered_events = replayed
            .events_after(EventsListParams {
                thread_id: thread.id,
                after: 0,
                limit: None,
            })
            .unwrap()
            .events;
        let replayed_again = Runtime::replay_events(&recovered_events).unwrap();
        let replayed_metrics = replayed_again.metrics();
        assert_eq!(replayed_metrics.pending_approvals, 0);
        assert_eq!(replayed_metrics.pending_questions, 0);
    }

    #[test]
    fn simulated_tool_call_gets_exactly_one_result_or_denial() {
        let mut runtime = Runtime::new();
        let thread = thread(&mut runtime);
        let result = runtime
            .start_turn(TurnStartParams {
                thread_id: thread.id,
                input: "run tool".to_string(),
                assistant_response: None,
                simulated_tool: Some(SimulatedToolUse {
                    call: ToolCall {
                        id: "call-1".to_string(),
                        name: "write".to_string(),
                        namespace: None,
                        arguments: json!({}),
                    },
                    result: ToolResult {
                        call_id: "call-1".to_string(),
                        status: ToolResultStatus::Denied,
                        output: None,
                        error: Some("blocked".to_string()),
                    },
                    require_approval: true,
                    concurrency_safe: false,
                }),
                requested_continuations: 0,
                stop_hook_feedback: None,
                defer_completion: false,
            })
            .unwrap();
        assert!(
            result
                .events
                .iter()
                .any(|event| matches!(event.kind, EventKind::ToolCallStarted { .. }))
        );
        assert!(
            result
                .events
                .iter()
                .any(|event| matches!(event.kind, EventKind::ApprovalRequested { .. }))
        );
        assert!(result.events.iter().any(|event| matches!(
            event.kind,
            EventKind::ToolCallCompleted {
                result: ToolResult {
                    status: ToolResultStatus::Denied,
                    ..
                }
            }
        )));
    }

    #[test]
    fn tool_batch_partitions_parallel_safe_calls_and_exclusive_mutations() {
        let mut runtime = Runtime::new();
        let thread = thread(&mut runtime);
        let turn = runtime
            .start_turn(TurnStartParams {
                thread_id: thread.id.clone(),
                input: "batch".to_string(),
                assistant_response: None,
                simulated_tool: None,
                requested_continuations: 0,
                stop_hook_feedback: None,
                defer_completion: true,
            })
            .unwrap()
            .turn;

        let result = runtime
            .record_tool_batch(ToolBatchRecordParams {
                thread_id: thread.id,
                turn_id: turn.id,
                tools: vec![
                    tool("read-1", "read", true),
                    tool("grep-1", "grep", true),
                    tool("edit-1", "edit", false),
                    tool("read-2", "read", true),
                ],
                max_concurrency: Some(10),
            })
            .unwrap();

        assert_eq!(result.batches.len(), 3);
        assert_eq!(result.batches[0].execution, ToolBatchExecution::Concurrent);
        assert_eq!(result.batches[0].tool_call_ids, vec!["read-1", "grep-1"]);
        assert_eq!(result.batches[1].execution, ToolBatchExecution::Exclusive);
        assert_eq!(result.batches[1].tool_call_ids, vec!["edit-1"]);
        assert!(
            result
                .events
                .iter()
                .any(|event| matches!(event.kind, EventKind::ToolBatchStarted { .. }))
        );
        assert_eq!(
            result
                .events
                .iter()
                .filter(|event| matches!(event.kind, EventKind::ToolCallCompleted { .. }))
                .count(),
            4
        );
    }

    #[test]
    fn rejects_turn_operations_for_wrong_thread_without_partial_events() {
        let mut runtime = Runtime::new();
        let thread_one = thread(&mut runtime);
        let thread_two = thread(&mut runtime);
        let turn = runtime
            .start_turn(TurnStartParams {
                thread_id: thread_one.id.clone(),
                input: "thread one".to_string(),
                assistant_response: None,
                simulated_tool: None,
                requested_continuations: 0,
                stop_hook_feedback: None,
                defer_completion: false,
            })
            .unwrap()
            .turn;
        let before = runtime
            .events_after(EventsListParams {
                thread_id: thread_two.id.clone(),
                after: 0,
                limit: None,
            })
            .unwrap()
            .events
            .len();

        let result = runtime.record_tool(ToolRecordParams {
            thread_id: thread_two.id.clone(),
            turn_id: turn.id,
            call: tool("read-1", "read", false).call,
            result: ToolResult {
                call_id: "read-1".to_string(),
                status: ToolResultStatus::Ok,
                output: Some("ok".to_string()),
                error: None,
            },
        });

        assert_eq!(result.unwrap_err().code, "turn_thread_mismatch");
        assert_eq!(
            runtime
                .events_after(EventsListParams {
                    thread_id: thread_two.id,
                    after: 0,
                    limit: None,
                })
                .unwrap()
                .events
                .len(),
            before
        );
    }

    #[test]
    fn failed_tool_batch_does_not_persist_started_event() {
        let mut runtime = Runtime::new();
        let thread = thread(&mut runtime);
        let turn = runtime
            .start_turn(TurnStartParams {
                thread_id: thread.id.clone(),
                input: "bad batch".to_string(),
                assistant_response: None,
                simulated_tool: None,
                requested_continuations: 0,
                stop_hook_feedback: None,
                defer_completion: true,
            })
            .unwrap()
            .turn;
        let after = runtime
            .events_after(EventsListParams {
                thread_id: thread.id.clone(),
                after: 0,
                limit: None,
            })
            .unwrap()
            .events
            .last()
            .map(|event| event.id)
            .unwrap_or(0);

        let mut bad = tool("read-1", "read", true);
        bad.result.call_id = "other-call".to_string();
        let result = runtime.record_tool_batch(ToolBatchRecordParams {
            thread_id: thread.id.clone(),
            turn_id: turn.id,
            tools: vec![bad],
            max_concurrency: Some(4),
        });

        assert_eq!(result.unwrap_err().code, "tool_pair_mismatch");
        let new_events = runtime
            .events_after(EventsListParams {
                thread_id: thread.id,
                after,
                limit: None,
            })
            .unwrap()
            .events;
        assert!(new_events.is_empty());
    }

    #[test]
    fn memory_controls_return_native_hoppi_dashboard_without_hidden_work() {
        let mut runtime = Runtime::new();
        let runtime_thread = thread(&mut runtime);
        runtime
            .set_memory_status(MemorySetParams {
                thread_id: runtime_thread.id.clone(),
                status: MemoryStatus {
                    enabled: true,
                    initialized: true,
                    backend: "client".to_string(),
                    scope: "project".to_string(),
                    memory_count: 7,
                    ..MemoryStatus::default()
                },
            })
            .unwrap();
        let dashboard = runtime
            .memory_control(MemoryControlParams {
                thread_id: runtime_thread.id.clone(),
                action: "dashboard".to_string(),
                apply: false,
            })
            .unwrap();
        assert!(dashboard.summary.contains("runtime-owned backend broker"));
        assert!(
            dashboard
                .summary
                .contains("bounded Node/TypeScript adapter")
        );
        assert!(
            dashboard
                .controls
                .iter()
                .any(|control| control.command == "/memory off")
        );
        assert!(dashboard.events.iter().any(|event| matches!(
            &event.kind,
            EventKind::Diagnostic { diagnostic }
                if diagnostic.message == "memory control action"
                    && diagnostic.metadata.get("executionOwner") == Some(&"runtime".to_string())
                    && diagnostic.metadata.get("adapterKind") == Some(&"node-typescript".to_string())
        )));
        let maintenance = runtime
            .memory_control(MemoryControlParams {
                thread_id: runtime_thread.id,
                action: "maintenance".to_string(),
                apply: true,
            })
            .unwrap();
        assert!(maintenance.summary.contains("no hidden model session"));
        assert!(
            maintenance
                .controls
                .iter()
                .any(|control| control.command == "/memory maintenance apply")
        );
    }

    #[test]
    fn memory_status_preserves_hoppi_backend_contract_fields() {
        let mut runtime = Runtime::new();
        let thread = thread(&mut runtime);
        let status: MemoryStatus = serde_json::from_value(json!({
            "enabled": true,
            "initialized": true,
            "backend": "hoppi",
            "scope": "project",
            "storePath": "C:/Users/matth/.oppi/hoppi",
            "projectId": "oppi-native",
            "memoryCount": 12,
            "pinnedCount": 3,
            "lastWriteAt": "2026-05-29T12:00:00Z",
            "lastConsolidatedAt": "2026-05-29T11:00:00Z",
            "queueDepth": 2,
            "dashboardUrl": "http://hoppi.localhost:49152"
        }))
        .unwrap();

        runtime
            .set_memory_status(MemorySetParams {
                thread_id: thread.id,
                status,
            })
            .unwrap();

        let actual = serde_json::to_value(runtime.memory_status()).unwrap();
        assert_eq!(actual["initialized"], json!(true));
        assert_eq!(actual["storePath"], json!("C:/Users/matth/.oppi/hoppi"));
        assert_eq!(actual["projectId"], json!("oppi-native"));
        assert_eq!(actual["pinnedCount"], json!(3));
        assert_eq!(
            actual["dashboardUrl"],
            json!("http://hoppi.localhost:49152")
        );
    }

    #[test]
    fn memory_dashboard_returns_runtime_status_without_hidden_work() {
        let mut runtime = Runtime::new();
        let runtime_thread = thread(&mut runtime);
        runtime
            .set_memory_status(MemorySetParams {
                thread_id: runtime_thread.id.clone(),
                status: MemoryStatus {
                    enabled: true,
                    initialized: true,
                    backend: "hoppi".to_string(),
                    scope: "project".to_string(),
                    dashboard_url: Some("http://hoppi.localhost:49152".to_string()),
                    memory_count: 12,
                    pinned_count: 3,
                    ..MemoryStatus::default()
                },
            })
            .unwrap();

        let running = runtime
            .memory_dashboard(MemoryDashboardParams {
                thread_id: runtime_thread.id.clone(),
            })
            .unwrap();
        assert!(running.available);
        assert_eq!(running.url.as_deref(), Some("http://hoppi.localhost:49152"));
        assert_eq!(
            running.fallback_url.as_deref(),
            Some("http://localhost:49152")
        );
        assert!(running.message.contains("runtime memory status"));
        assert!(
            running
                .controls
                .iter()
                .any(|control| control.command == "/memory dashboard")
        );
        assert!(running.events.iter().any(|event| matches!(
            &event.kind,
            EventKind::Diagnostic { diagnostic }
                if diagnostic.message == "memory dashboard status"
                    && diagnostic.metadata.get("dashboardAvailable") == Some(&"true".to_string())
                    && diagnostic.metadata.get("runtimeOwned") == Some(&"true".to_string())
        )));

        let mut fresh = Runtime::new();
        let fresh_thread = thread(&mut fresh);
        let missing = fresh
            .memory_dashboard(MemoryDashboardParams {
                thread_id: fresh_thread.id,
            })
            .unwrap();
        assert!(!missing.available);
        assert!(missing.url.is_none());
        assert!(missing.message.contains("/memory dashboard"));
    }

    #[test]
    fn memory_list_returns_cached_hoppi_records_without_hidden_work() {
        let mut runtime = Runtime::new();
        let runtime_thread = thread(&mut runtime);
        let record = MemoryRecord {
            id: "mem-1".to_string(),
            content: "Native memory list should be queryable through Rust.".to_string(),
            tags: vec!["oppi-native-shell".to_string(), "verified".to_string()],
            pinned: true,
            confidence: Some("verified".to_string()),
            source: Some("oppi:native-shell".to_string()),
            ..MemoryRecord::default()
        };
        let cached = runtime
            .set_memory_records(MemoryRecordsSetParams {
                thread_id: runtime_thread.id.clone(),
                records: vec![
                    record.clone(),
                    MemoryRecord {
                        id: "mem-superseded".to_string(),
                        content: "Old native memory".to_string(),
                        superseded_by: Some("mem-1".to_string()),
                        ..MemoryRecord::default()
                    },
                ],
                status: Some(MemoryStatus {
                    enabled: true,
                    initialized: true,
                    backend: "hoppi".to_string(),
                    scope: "project".to_string(),
                    memory_count: 2,
                    pinned_count: 1,
                    ..MemoryStatus::default()
                }),
                source: Some("hoppi-list".to_string()),
            })
            .unwrap();
        assert!(cached.events.iter().any(|event| matches!(
            &event.kind,
            EventKind::Diagnostic { diagnostic }
                if diagnostic.message == "memory records cached"
                    && diagnostic.metadata.get("runtimeOwned") == Some(&"true".to_string())
        )));

        let listed = runtime
            .memory_list(MemoryListParams {
                thread_id: runtime_thread.id.clone(),
                query: Some("queryable".to_string()),
                limit: Some(10),
                pinned_only: true,
                include_superseded: false,
            })
            .unwrap();
        assert!(listed.available);
        assert_eq!(listed.total_count, 1);
        assert_eq!(listed.items, vec![record]);
        assert!(listed.message.contains("1 cached record"));
        assert!(
            listed
                .controls
                .iter()
                .any(|control| control.command == "/memory dashboard")
        );
        assert!(listed.events.iter().any(|event| matches!(
            &event.kind,
            EventKind::Diagnostic { diagnostic }
                if diagnostic.message == "memory list status"
                    && diagnostic.metadata.get("cached") == Some(&"true".to_string())
                    && diagnostic.metadata.get("runtimeOwned") == Some(&"true".to_string())
        )));

        let mut fresh = Runtime::new();
        let fresh_thread = thread(&mut fresh);
        let missing = fresh
            .memory_list(MemoryListParams {
                thread_id: fresh_thread.id,
                query: None,
                limit: None,
                pinned_only: false,
                include_superseded: false,
            })
            .unwrap();
        assert!(!missing.available);
        assert!(missing.items.is_empty());
        assert!(missing.message.contains("/memory list"));
    }

    #[test]
    fn memory_recall_records_context_and_cached_hoppi_records_without_hidden_work() {
        let mut runtime = Runtime::new();
        let runtime_thread = thread(&mut runtime);
        runtime
            .set_memory_records(MemoryRecordsSetParams {
                thread_id: runtime_thread.id.clone(),
                records: vec![MemoryRecord {
                    id: "mem-existing".to_string(),
                    content: "Existing memory should survive recall upserts.".to_string(),
                    ..MemoryRecord::default()
                }],
                status: Some(MemoryStatus {
                    enabled: true,
                    initialized: true,
                    backend: "hoppi".to_string(),
                    scope: "project".to_string(),
                    memory_count: 1,
                    pinned_count: 0,
                    ..MemoryStatus::default()
                }),
                source: Some("hoppi-list".to_string()),
            })
            .unwrap();

        let recalled_record = MemoryRecord {
            id: "mem-recall".to_string(),
            content: "Runtime recall should cache normalized Hoppi records.".to_string(),
            tags: vec!["recall".to_string()],
            pinned: true,
            retrieval_strength: Some(0.91),
            retrieval_count: Some(3),
            ..MemoryRecord::default()
        };
        let recalled = runtime
            .memory_recall(MemoryRecallParams {
                thread_id: runtime_thread.id.clone(),
                query: Some("ship native recall boundary".to_string()),
                context_markdown: Some(
                    "Remember that recalled context now flows through Rust.".to_string(),
                ),
                omitted_reason: None,
                records: vec![recalled_record.clone()],
                backend_queried: true,
                status: Some(MemoryStatus {
                    enabled: true,
                    initialized: true,
                    backend: "hoppi".to_string(),
                    scope: "project".to_string(),
                    store_path: Some("C:/Users/matth/.oppi/hoppi".to_string()),
                    memory_count: 2,
                    pinned_count: 1,
                    ..MemoryStatus::default()
                }),
                truncated: false,
                source: Some("hoppi-recall".to_string()),
            })
            .unwrap();

        assert!(recalled.available);
        assert!(recalled.backend_queried);
        assert!(recalled.cache_updated);
        assert_eq!(
            recalled.context_markdown.as_deref(),
            Some("Remember that recalled context now flows through Rust.")
        );
        assert_eq!(recalled.context_bytes, 54);
        assert_eq!(recalled.total_count, 1);
        assert_eq!(recalled.items, vec![recalled_record.clone()]);
        assert_eq!(recalled.status.memory_count, 2);
        assert_eq!(recalled.status.pinned_count, 1);
        assert!(recalled.message.contains("Recalled Hoppi context"));
        assert!(recalled.events.iter().any(|event| matches!(
            &event.kind,
            EventKind::Diagnostic { diagnostic }
                if diagnostic.message == "memory recall recorded"
                    && diagnostic.metadata.get("runtimeOwned") == Some(&"true".to_string())
                    && diagnostic.metadata.get("backendQueried") == Some(&"true".to_string())
                    && diagnostic.metadata.get("cacheUpdated") == Some(&"true".to_string())
        )));

        let listed = runtime
            .memory_list(MemoryListParams {
                thread_id: runtime_thread.id,
                query: None,
                limit: None,
                pinned_only: false,
                include_superseded: false,
            })
            .unwrap();
        assert_eq!(listed.total_count, 2);
        assert!(
            listed
                .items
                .iter()
                .any(|record| record.id == "mem-existing")
        );
        assert!(listed.items.iter().any(|record| record == &recalled_record));
    }

    #[test]
    fn memory_backend_recall_rejects_wrong_thread() {
        let mut runtime = Runtime::new();
        let error = runtime
            .memory_backend_recall(MemoryBackendRecallParams {
                thread_id: "missing-thread".to_string(),
                prompt: "hello".to_string(),
                include_startup: true,
                include_task: true,
            })
            .unwrap_err();

        assert_eq!(error.code, "thread_not_found");
    }

    #[test]
    fn memory_backend_recall_reports_running_and_terminal_jobs() {
        let mut runtime = Runtime::new();
        let runtime_thread = thread(&mut runtime);
        runtime.set_memory_backend_runner_for_test(Box::new(MockMemoryBackendRunner::recall(
            "Native memory context",
        )));

        let result = runtime
            .memory_backend_recall(MemoryBackendRecallParams {
                thread_id: runtime_thread.id.clone(),
                prompt: "continue the plan".to_string(),
                include_startup: true,
                include_task: true,
            })
            .unwrap();

        assert_eq!(
            result.normalized["contextMarkdown"],
            json!("Native memory context")
        );
        assert_eq!(result.operation, MemoryBackendOperation::Recall);
        assert_eq!(result.backend, "mock-hoppi");
        assert!(!result.job_id.is_empty());
        assert_eq!(
            result.summary.as_deref(),
            Some("Recalled Hoppi context through runtime backend broker.")
        );
        assert!(!result.timed_out);
        assert!(!result.cancelled);

        let jobs = runtime
            .list_memory_jobs(MemoryJobsListParams {
                thread_id: Some(runtime_thread.id),
                include_finished: true,
                limit: Some(10),
            })
            .unwrap();
        assert_eq!(jobs.queue_depth, 0);
        assert!(jobs.items.iter().any(|job| {
            job.id == result.job_id
                && job.kind == MemoryJobKind::Recall
                && job.status == MemoryJobStatus::Succeeded
                && job.stage.as_deref() == Some("recall-startup-task")
                && job.source.as_deref() == Some("memory-backend")
                && job.backend_applied
        }));
    }

    #[test]
    fn memory_backend_status_updates_runtime_status_and_job() {
        let mut runtime = Runtime::new();
        let runtime_thread = thread(&mut runtime);
        runtime.set_memory_backend_runner_for_test(Box::new(MockMemoryBackendRunner::default()));

        let adapter_info = MockMemoryBackendRunner::default().adapter_info();
        assert_eq!(adapter_info.id, "mock-hoppi");
        assert_eq!(adapter_info.kind, "mock-rust");
        assert!(adapter_info.embedded);
        assert_eq!(
            adapter_info.operations,
            MemoryBackendOperation::ALL.to_vec()
        );

        let result = runtime
            .memory_backend_status(MemoryBackendStatusParams {
                thread_id: runtime_thread.id.clone(),
            })
            .unwrap();

        assert_eq!(result.operation, MemoryBackendOperation::Status);
        assert_eq!(result.adapter_kind.as_deref(), Some("mock-rust"));
        assert_eq!(result.normalized["memoryCount"], json!(2));
        assert_eq!(
            result.normalized["dashboardUrl"],
            json!("http://hoppi.localhost:49152")
        );
        assert_eq!(result.normalized["executionOwner"], json!("runtime"));
        assert_eq!(result.normalized["executionAdapter"], json!("mock-hoppi"));
        assert_eq!(
            result.normalized["executionAdapterKind"],
            json!("mock-rust")
        );
        assert_memory_backend_job(
            &mut runtime,
            &runtime_thread.id,
            &result.job_id,
            MemoryJobKind::Status,
            "status",
            2,
            0,
        );
    }

    #[test]
    fn memory_backend_remember_normalizes_saved_record_and_job() {
        let mut runtime = Runtime::new();
        let runtime_thread = thread(&mut runtime);
        runtime.set_memory_backend_runner_for_test(Box::new(MockMemoryBackendRunner::default()));

        let result = runtime
            .memory_backend_remember(MemoryBackendRememberParams {
                thread_id: runtime_thread.id.clone(),
                content: " remember this through the backend ".to_string(),
                tags: vec![" backend ".to_string(), " ".to_string()],
                source: Some("oppi:test".to_string()),
                source_session_id: Some("thread-1:turn-1".to_string()),
                layer: Some("buffer".to_string()),
            })
            .unwrap();

        assert_eq!(result.operation, MemoryBackendOperation::Remember);
        assert_eq!(result.backend, "mock-hoppi");
        assert_eq!(result.normalized["saved"], json!(true));
        assert_eq!(result.normalized["backendApplied"], json!(true));
        assert_eq!(result.normalized["record"]["id"], json!("mem-remember"));
        assert_memory_backend_job(
            &mut runtime,
            &runtime_thread.id,
            &result.job_id,
            MemoryJobKind::Remember,
            "remember",
            1,
            0,
        );
    }

    #[test]
    fn memory_backend_list_caches_backend_records_before_filtering() {
        let mut runtime = Runtime::new();
        let runtime_thread = thread(&mut runtime);
        runtime.set_memory_backend_runner_for_test(Box::new(MockMemoryBackendRunner::default()));

        let result = runtime
            .memory_backend_list(MemoryBackendListParams {
                thread_id: runtime_thread.id.clone(),
                query: Some("backend route".to_string()),
                limit: Some(5),
                pinned_only: true,
                include_superseded: false,
            })
            .unwrap();

        assert_eq!(result.operation, MemoryBackendOperation::List);
        assert_eq!(result.normalized["available"], json!(true));
        assert_eq!(result.normalized["totalCount"], json!(1));
        assert_eq!(result.normalized["items"][0]["id"], json!("mem-list"));
        assert_eq!(result.normalized["status"]["pinnedCount"], json!(1));
        assert_memory_backend_job(
            &mut runtime,
            &runtime_thread.id,
            &result.job_id,
            MemoryJobKind::List,
            "list-pinned",
            1,
            0,
        );
    }

    #[test]
    fn memory_backend_dashboard_updates_status_before_normalizing() {
        let mut runtime = Runtime::new();
        let runtime_thread = thread(&mut runtime);
        runtime.set_memory_backend_runner_for_test(Box::new(MockMemoryBackendRunner::default()));

        let result = runtime
            .memory_backend_dashboard(MemoryBackendDashboardParams {
                thread_id: runtime_thread.id.clone(),
            })
            .unwrap();

        assert_eq!(result.operation, MemoryBackendOperation::Dashboard);
        assert_eq!(result.normalized["available"], json!(true));
        assert_eq!(
            result.normalized["url"],
            json!("http://hoppi.localhost:49152")
        );
        assert_eq!(
            result.normalized["fallbackUrl"],
            json!("http://localhost:49152")
        );
        assert_memory_backend_job(
            &mut runtime,
            &runtime_thread.id,
            &result.job_id,
            MemoryJobKind::Dashboard,
            "dashboard-start",
            2,
            0,
        );
    }

    #[test]
    fn memory_backend_record_mutation_routes_normalize_backend_results() {
        let mut runtime = Runtime::new();
        let runtime_thread = thread(&mut runtime);
        runtime.set_memory_backend_runner_for_test(Box::new(MockMemoryBackendRunner::default()));
        runtime
            .set_memory_records(MemoryRecordsSetParams {
                thread_id: runtime_thread.id.clone(),
                records: vec![
                    MemoryRecord {
                        id: "mem-forget".to_string(),
                        content: "Forget this through backend route.".to_string(),
                        ..MemoryRecord::default()
                    },
                    MemoryRecord {
                        id: "mem-edit".to_string(),
                        content: "Edit this through backend route.".to_string(),
                        ..MemoryRecord::default()
                    },
                ],
                status: None,
                source: Some("test".to_string()),
            })
            .unwrap();

        let forgotten = runtime
            .memory_backend_forget(MemoryBackendForgetParams {
                thread_id: runtime_thread.id.clone(),
                id: " mem-forget ".to_string(),
            })
            .unwrap();
        assert_eq!(forgotten.operation, MemoryBackendOperation::Forget);
        assert_eq!(forgotten.normalized["action"], json!("forget"));
        assert_eq!(forgotten.normalized["cacheUpdated"], json!(true));
        assert_memory_backend_job(
            &mut runtime,
            &runtime_thread.id,
            &forgotten.job_id,
            MemoryJobKind::RecordMutation,
            "forget",
            0,
            1,
        );

        let pinned = runtime
            .memory_backend_pin(MemoryBackendPinParams {
                thread_id: runtime_thread.id.clone(),
                id: "mem-edit".to_string(),
                pinned: true,
            })
            .unwrap();
        assert_eq!(pinned.operation, MemoryBackendOperation::Pin);
        assert_eq!(pinned.normalized["record"]["pinned"], json!(true));

        let updated = runtime
            .memory_backend_update(MemoryBackendUpdateParams {
                thread_id: runtime_thread.id.clone(),
                id: "mem-edit".to_string(),
                patch: MemoryRecordPatch {
                    content: Some("Edited by backend route.".to_string()),
                    tags: Some(vec!["backend".to_string()]),
                    ..MemoryRecordPatch::default()
                },
            })
            .unwrap();
        assert_eq!(updated.operation, MemoryBackendOperation::Update);
        assert_eq!(
            updated.normalized["record"]["content"],
            json!("Edited by backend route.")
        );

        let superseded = runtime
            .memory_backend_supersede(MemoryBackendSupersedeParams {
                thread_id: runtime_thread.id.clone(),
                id: "mem-edit".to_string(),
                superseded_by: "mem-new".to_string(),
            })
            .unwrap();
        assert_eq!(superseded.operation, MemoryBackendOperation::Supersede);
        assert_eq!(
            superseded.normalized["record"]["supersededBy"],
            json!("mem-new")
        );
    }

    #[test]
    fn memory_backend_consolidate_and_sync_report_jobs() {
        let mut runtime = Runtime::new();
        let runtime_thread = thread(&mut runtime);
        runtime.set_memory_backend_runner_for_test(Box::new(MockMemoryBackendRunner::default()));
        let mirrored_events = Arc::new(Mutex::new(Vec::new()));
        runtime.set_event_mirror(Some(mirrored_events.clone()));
        runtime
            .set_memory_records(MemoryRecordsSetParams {
                thread_id: runtime_thread.id.clone(),
                records: vec![MemoryRecord {
                    id: "mem-noise".to_string(),
                    content: "Noisy memory.".to_string(),
                    ..MemoryRecord::default()
                }],
                status: None,
                source: Some("test".to_string()),
            })
            .unwrap();

        let consolidated = runtime
            .memory_backend_consolidate(MemoryBackendConsolidateParams {
                thread_id: runtime_thread.id.clone(),
                stage: Some(" deep-dream ".to_string()),
                dry_run: false,
                limit: Some(50),
                budget: Some(1500),
                agent_model: Some(" claude ".to_string()),
                deep_model: Some(" gpt-5.5 ".to_string()),
            })
            .unwrap();
        assert_eq!(consolidated.operation, MemoryBackendOperation::Consolidate);
        assert_eq!(consolidated.normalized["backendApplied"], json!(true));
        assert_eq!(consolidated.normalized["createdCount"], json!(1));
        assert_eq!(consolidated.normalized["deletedCount"], json!(1));
        let mirrored_events = mirrored_events.lock().unwrap();
        assert!(mirrored_events.iter().any(|event| matches!(
            &event.kind,
            EventKind::MemoryJobUpdated { job }
                if job.id == consolidated.job_id
                    && job.kind == MemoryJobKind::Consolidate
                    && job.status == MemoryJobStatus::Running
                    && job.stage.as_deref() == Some("deep-dream")
                    && job.agent_model.as_deref() == Some("claude")
                    && job.deep_model.as_deref() == Some("gpt-5.5")
                    && job.model_used.is_none()
        )));
        drop(mirrored_events);
        assert_memory_backend_job(
            &mut runtime,
            &runtime_thread.id,
            &consolidated.job_id,
            MemoryJobKind::Consolidate,
            "deep-dream",
            1,
            1,
        );
        let jobs = runtime
            .list_memory_jobs(MemoryJobsListParams {
                thread_id: Some(runtime_thread.id.clone()),
                include_finished: true,
                limit: Some(50),
            })
            .unwrap();
        let job = jobs
            .items
            .iter()
            .find(|job| job.id == consolidated.job_id)
            .unwrap();
        assert_eq!(job.agent_model.as_deref(), Some("claude"));
        assert_eq!(job.deep_model.as_deref(), Some("gpt-5.5"));
        assert_eq!(job.model_used.as_deref(), Some("gpt-5.5"));

        let synced = runtime
            .memory_backend_sync(MemoryBackendSyncParams {
                thread_id: runtime_thread.id.clone(),
                direction: MemoryBackendSyncDirection::Both,
                repo_path: Some("E:/RepositoryV2/oppi/.hoppi-sync".to_string()),
                remote: Some("git@example.com:memories.git".to_string()),
                conflict_mode: Some("keep-local".to_string()),
                passphrase: Some("runtime-passphrase".to_string()),
                message: Some("Hoppi memory sync (test)".to_string()),
            })
            .unwrap();
        assert_eq!(synced.operation, MemoryBackendOperation::Sync);
        assert_eq!(synced.normalized["imported"], json!(2));
        assert_eq!(synced.normalized["exported"], json!(1));
        assert_memory_backend_job(
            &mut runtime,
            &runtime_thread.id,
            &synced.job_id,
            MemoryJobKind::Sync,
            "sync-both",
            3,
            0,
        );
    }

    #[test]
    fn memory_remember_records_backend_write_and_cached_record_without_hidden_work() {
        let mut runtime = Runtime::new();
        let runtime_thread = thread(&mut runtime);
        let saved_record = MemoryRecord {
            id: "mem-remember".to_string(),
            content: "Runtime remember should cache normalized Hoppi writes.".to_string(),
            tags: vec![
                "oppi-native-shell".to_string(),
                "oppi-turn-summary".to_string(),
            ],
            layer: Some("buffer".to_string()),
            confidence: Some("observed".to_string()),
            source: Some("oppi:native-shell".to_string()),
            ..MemoryRecord::default()
        };

        let remembered = runtime
            .memory_remember(MemoryRememberParams {
                thread_id: runtime_thread.id.clone(),
                content: saved_record.content.clone(),
                tags: saved_record.tags.clone(),
                layer: saved_record.layer.clone(),
                confidence: saved_record.confidence.clone(),
                source_session_id: Some("thread-1:turn-1".to_string()),
                backend_applied: true,
                record: Some(saved_record.clone()),
                status: Some(MemoryStatus {
                    enabled: true,
                    initialized: true,
                    backend: "hoppi".to_string(),
                    scope: "project".to_string(),
                    memory_count: 1,
                    pinned_count: 0,
                    store_path: Some("C:/Users/matth/.oppi/hoppi".to_string()),
                    ..MemoryStatus::default()
                }),
                reason: Some("turn-summary".to_string()),
                source: Some("hoppi-remember".to_string()),
            })
            .unwrap();

        assert!(remembered.available);
        assert!(remembered.saved);
        assert!(remembered.backend_applied);
        assert!(remembered.cache_updated);
        assert_eq!(remembered.record, Some(saved_record.clone()));
        assert_eq!(remembered.status.memory_count, 1);
        assert_eq!(
            remembered.status.store_path.as_deref(),
            Some("C:/Users/matth/.oppi/hoppi")
        );
        assert!(remembered.message.contains("mem-remember"));
        assert!(remembered.events.iter().any(|event| matches!(
            &event.kind,
            EventKind::Diagnostic { diagnostic }
                if diagnostic.message == "memory remember recorded"
                    && diagnostic.metadata.get("runtimeOwned") == Some(&"true".to_string())
                    && diagnostic.metadata.get("backendApplied") == Some(&"true".to_string())
                    && diagnostic.metadata.get("cacheUpdated") == Some(&"true".to_string())
                    && diagnostic.metadata.get("reason") == Some(&"turn-summary".to_string())
        )));

        let listed = runtime
            .memory_list(MemoryListParams {
                thread_id: runtime_thread.id,
                query: Some("normalized hoppi writes".to_string()),
                limit: None,
                pinned_only: false,
                include_superseded: false,
            })
            .unwrap();
        assert_eq!(listed.total_count, 1);
        assert_eq!(listed.items, vec![saved_record]);
    }

    #[test]
    fn memory_recurring_candidates_records_system_scope_records_and_visible_job() {
        let mut runtime = Runtime::new();
        let runtime_thread = thread(&mut runtime);
        let mirrored_events = Arc::new(Mutex::new(Vec::new()));
        runtime.set_event_mirror(Some(mirrored_events.clone()));

        let result = runtime
            .memory_recurring_candidates(MemoryRecurringCandidatesParams {
                thread_id: runtime_thread.id.clone(),
                source_project: Some("E:/RepositoryV2/oppi".to_string()),
                source_session_id: Some("session-42".to_string()),
                recent_user_texts: vec!["continue with the plan".to_string()],
                recent_assistant_texts: vec!["Updated Plan 75 evidence.".to_string()],
                candidates: vec![
                    MemoryRecurringTaskCandidate {
                        task: "Audit native memory parity after each backend slice".to_string(),
                        rationale: Some(
                            "This plan-alignment loop repeats across native passes.".to_string(),
                        ),
                        prompt_hints: vec!["continue with the plan".to_string()],
                        outcomes: vec!["Plan 61 gets fresh verification evidence".to_string()],
                        related_memory_ids: vec!["mem-1".to_string()],
                        tags: vec!["plan75".to_string(), "project:private".to_string()],
                    },
                    MemoryRecurringTaskCandidate {
                        task: "tiny".to_string(),
                        ..MemoryRecurringTaskCandidate::default()
                    },
                    MemoryRecurringTaskCandidate {
                        task: "Turn repeated review fixes into reusable coding skills".to_string(),
                        rationale: None,
                        prompt_hints: vec!["fix the issues you discovered".to_string()],
                        outcomes: vec![
                            "focused regression tests before implementation".to_string(),
                        ],
                        related_memory_ids: Vec::new(),
                        tags: vec!["reviews".to_string()],
                    },
                ],
            })
            .unwrap();

        assert!(result.available);
        assert_eq!(result.saved_count, 2);
        assert_eq!(result.records.len(), 2);
        let first = &result.records[0];
        assert!(first.id.starts_with("recurring-task-"));
        assert!(first.content.contains("Recurring task candidate"));
        assert!(first.content.contains("Task: Audit native memory parity"));
        assert!(
            first
                .content
                .contains("Source project: E:/RepositoryV2/oppi")
        );
        assert!(first.content.contains("Source session: session-42"));
        assert!(
            first
                .content
                .contains("Approval gate: only propose skill creation")
        );
        assert!(first.content.contains("Recent user wording:"));
        assert!(first.content.contains("- continue with the plan"));
        assert!(
            first
                .tags
                .contains(&"oppi-recurring-task-candidate".to_string())
        );
        assert!(first.tags.contains(&"system-scope".to_string()));
        assert!(first.tags.contains(&"skill-candidate".to_string()));
        assert!(first.tags.contains(&"plan75".to_string()));
        assert!(!first.tags.iter().any(|tag| tag.starts_with("project:")));
        assert_eq!(first.layer.as_deref(), Some("semantic"));
        assert_eq!(first.confidence.as_deref(), Some("observed"));
        assert_eq!(first.source.as_deref(), Some("oppi:recurring-task-mining"));

        let mirrored_events = mirrored_events.lock().unwrap();
        assert!(mirrored_events.iter().any(|event| matches!(
            &event.kind,
            EventKind::MemoryJobUpdated { job }
                if job.kind == MemoryJobKind::Remember
                    && job.status == MemoryJobStatus::Running
                    && job.stage.as_deref() == Some("recurring-task-mining")
                    && job.source.as_deref() == Some("memory-recurring-task-mining")
        )));
        drop(mirrored_events);

        let jobs = runtime
            .list_memory_jobs(MemoryJobsListParams {
                thread_id: Some(runtime_thread.id.clone()),
                include_finished: true,
                limit: Some(10),
            })
            .unwrap();
        assert!(jobs.items.iter().any(|job| {
            job.kind == MemoryJobKind::Remember
                && job.status == MemoryJobStatus::Succeeded
                && job.stage.as_deref() == Some("recurring-task-mining")
                && job.source.as_deref() == Some("memory-recurring-task-mining")
                && job.record_count == 2
        }));

        let listed = runtime
            .memory_list(MemoryListParams {
                thread_id: runtime_thread.id,
                query: Some("reusable coding skills".to_string()),
                limit: None,
                pinned_only: false,
                include_superseded: false,
            })
            .unwrap();
        assert_eq!(listed.total_count, 1);
        assert_eq!(
            listed.items[0].source.as_deref(),
            Some("oppi:recurring-task-mining")
        );
    }

    #[test]
    fn memory_distill_turn_runs_model_records_memory_and_recurring_candidates_with_visible_jobs() {
        let response = json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": serde_json::to_string(&json!({
                        "remember": true,
                        "request": "Continue Plan 75",
                        "completed": ["Added runtime memory distillation route"],
                        "learned": ["Rust owns recurring candidate visibility"],
                        "decisions": ["Keep skill creation approval gated"],
                        "next": ["Wire native shell to distiller"],
                        "files": ["crates/oppi-core/src/lib.rs"],
                        "tags": ["plan75"],
                        "recurringTasks": [{
                            "task": "Audit native memory parity after backend slices",
                            "rationale": "Plan alignment repeats across standalone work",
                            "promptHints": ["continue with the plan"],
                            "outcomes": ["Plan 61 receives fresh evidence"],
                            "relatedMemoryIds": ["mem-1"],
                            "tags": ["plan75", "project:private"]
                        }]
                    })).unwrap()
                }
            }],
            "usage": { "prompt_tokens": 11, "completion_tokens": 7, "total_tokens": 18 }
        })
        .to_string();
        let (base_url, server) = start_mock_openai_server(vec![response]);
        let key_name = format!("OPPI_MEMORY_DISTILL_TEST_{}_API_KEY", std::process::id());
        unsafe { std::env::set_var(&key_name, "test-key") };
        let mut runtime = Runtime::new();
        let runtime_thread = thread(&mut runtime);

        let result = runtime
            .memory_distill_turn(MemoryDistillTurnParams {
                thread_id: runtime_thread.id.clone(),
                user_text: "continue with the plan".to_string(),
                assistant_text: "Added a runtime memory route and tests.".to_string(),
                source_project: Some("E:/RepositoryV2/oppi".to_string()),
                source_session_id: Some("session-42".to_string()),
                recent_user_texts: vec!["continue with the plan".to_string()],
                recent_assistant_texts: vec!["Plan 75 evidence updated.".to_string()],
                agent_model: Some("gpt".to_string()),
                model_provider: DirectModelProviderConfig {
                    kind: DirectModelProviderKind::OpenAiCompatible,
                    model: "mock-memory-model".to_string(),
                    base_url: Some(base_url),
                    api_key_env: Some(key_name.clone()),
                    system_prompt: None,
                    temperature: Some(0.2),
                    reasoning_effort: None,
                    max_output_tokens: None,
                    stream: false,
                },
            })
            .unwrap();
        unsafe { std::env::remove_var(&key_name) };
        let requests = server.join().unwrap();

        assert!(result.available);
        assert!(result.remembered);
        assert_eq!(result.model_used.as_deref(), Some("mock-memory-model"));
        let record = result.record.as_ref().expect("distilled memory record");
        assert!(record.id.starts_with("turn-memory-"));
        assert!(record.content.contains("Turn memory"));
        assert!(record.content.contains("Request: Continue Plan 75"));
        assert!(
            record
                .content
                .contains("Added runtime memory distillation route")
        );
        assert!(record.tags.contains(&"oppi-turn-summary".to_string()));
        assert!(record.tags.contains(&"agent_end".to_string()));
        assert!(record.tags.contains(&"plan75".to_string()));
        assert_eq!(record.source.as_deref(), Some("oppi:turn-distillation"));
        assert_eq!(result.recurring_saved_count, 1);
        assert_eq!(result.recurring_records.len(), 1);
        assert_eq!(
            result.recurring_records[0].source.as_deref(),
            Some("oppi:recurring-task-mining")
        );
        assert!(
            result.recurring_records[0]
                .tags
                .contains(&"plan75".to_string())
        );
        assert!(
            !result.recurring_records[0]
                .tags
                .iter()
                .any(|tag| tag.starts_with("project:"))
        );

        let jobs = runtime
            .list_memory_jobs(MemoryJobsListParams {
                thread_id: Some(runtime_thread.id.clone()),
                include_finished: true,
                limit: Some(10),
            })
            .unwrap();
        assert!(jobs.items.iter().any(|job| {
            job.kind == MemoryJobKind::Remember
                && job.status == MemoryJobStatus::Succeeded
                && job.stage.as_deref() == Some("turn-distillation")
                && job.source.as_deref() == Some("memory-distiller")
                && job.model_used.as_deref() == Some("mock-memory-model")
                && job.record_count == 2
        }));
        assert!(jobs.items.iter().any(|job| {
            job.kind == MemoryJobKind::Remember
                && job.status == MemoryJobStatus::Succeeded
                && job.stage.as_deref() == Some("recurring-task-mining")
                && job.record_count == 1
        }));
        assert!(requests[0].contains("OPPi's memory distiller"));
        assert!(requests[0].contains("continue with the plan"));
        assert!(requests[0].contains("\"max_tokens\":420"));
        assert!(requests[0].contains("\"reasoning_effort\":\"minimal\""));
    }

    #[test]
    fn memory_forget_removes_cached_hoppi_record_after_host_delete() {
        let mut runtime = Runtime::new();
        let runtime_thread = thread(&mut runtime);
        let pinned = MemoryRecord {
            id: "mem-1".to_string(),
            content: "Forget this native memory through the Hoppi bridge.".to_string(),
            pinned: true,
            ..MemoryRecord::default()
        };
        let survivor = MemoryRecord {
            id: "mem-2".to_string(),
            content: "Keep this memory.".to_string(),
            ..MemoryRecord::default()
        };
        runtime
            .set_memory_records(MemoryRecordsSetParams {
                thread_id: runtime_thread.id.clone(),
                records: vec![pinned.clone(), survivor.clone()],
                status: Some(MemoryStatus {
                    enabled: true,
                    initialized: true,
                    backend: "hoppi".to_string(),
                    scope: "project".to_string(),
                    memory_count: 2,
                    pinned_count: 1,
                    ..MemoryStatus::default()
                }),
                source: Some("hoppi-list".to_string()),
            })
            .unwrap();

        let forgotten = runtime
            .forget_memory_record(MemoryRecordForgetParams {
                thread_id: runtime_thread.id.clone(),
                id: "mem-1".to_string(),
                backend_applied: true,
                status: None,
                source: Some("hoppi-forget".to_string()),
            })
            .unwrap();

        assert_eq!(forgotten.action, "forget");
        assert_eq!(forgotten.id, "mem-1");
        assert!(forgotten.backend_applied);
        assert!(forgotten.cache_updated);
        assert_eq!(forgotten.record, Some(pinned));
        assert_eq!(forgotten.status.memory_count, 1);
        assert_eq!(forgotten.status.pinned_count, 0);
        assert!(forgotten.message.contains("mem-1"));
        assert!(forgotten.events.iter().any(|event| matches!(
            &event.kind,
            EventKind::Diagnostic { diagnostic }
                if diagnostic.message == "memory record forgotten"
                    && diagnostic.metadata.get("runtimeOwned") == Some(&"true".to_string())
                    && diagnostic.metadata.get("cacheUpdated") == Some(&"true".to_string())
        )));

        let listed = runtime
            .memory_list(MemoryListParams {
                thread_id: runtime_thread.id,
                query: None,
                limit: None,
                pinned_only: false,
                include_superseded: true,
            })
            .unwrap();
        assert_eq!(listed.total_count, 1);
        assert_eq!(listed.items, vec![survivor]);
    }

    #[test]
    fn memory_pin_updates_cached_hoppi_record_after_host_mutation() {
        let mut runtime = Runtime::new();
        let runtime_thread = thread(&mut runtime);
        let record = MemoryRecord {
            id: "mem-1".to_string(),
            content: "Pin this native memory through the Hoppi bridge.".to_string(),
            pinned: false,
            ..MemoryRecord::default()
        };
        runtime
            .set_memory_records(MemoryRecordsSetParams {
                thread_id: runtime_thread.id.clone(),
                records: vec![record],
                status: Some(MemoryStatus {
                    enabled: true,
                    initialized: true,
                    backend: "hoppi".to_string(),
                    scope: "project".to_string(),
                    memory_count: 1,
                    pinned_count: 0,
                    ..MemoryStatus::default()
                }),
                source: Some("hoppi-list".to_string()),
            })
            .unwrap();

        let pinned = runtime
            .pin_memory_record(MemoryRecordPinParams {
                thread_id: runtime_thread.id.clone(),
                id: "mem-1".to_string(),
                pinned: true,
                backend_applied: true,
                record: Some(MemoryRecord {
                    id: "mem-1".to_string(),
                    content: "Pin this native memory through the Hoppi bridge.".to_string(),
                    pinned: true,
                    ..MemoryRecord::default()
                }),
                status: None,
                source: Some("hoppi-pin".to_string()),
            })
            .unwrap();

        assert_eq!(pinned.action, "pin");
        assert_eq!(pinned.id, "mem-1");
        assert!(pinned.backend_applied);
        assert!(pinned.cache_updated);
        assert_eq!(
            pinned.record.as_ref().map(|record| record.pinned),
            Some(true)
        );
        assert_eq!(pinned.status.memory_count, 1);
        assert_eq!(pinned.status.pinned_count, 1);
        assert!(pinned.events.iter().any(|event| matches!(
            &event.kind,
            EventKind::Diagnostic { diagnostic }
                if diagnostic.message == "memory record pin updated"
                    && diagnostic.metadata.get("runtimeOwned") == Some(&"true".to_string())
                    && diagnostic.metadata.get("pinned") == Some(&"true".to_string())
        )));

        let listed = runtime
            .memory_list(MemoryListParams {
                thread_id: runtime_thread.id,
                query: None,
                limit: None,
                pinned_only: true,
                include_superseded: false,
            })
            .unwrap();
        assert_eq!(listed.total_count, 1);
        assert_eq!(listed.items[0].id, "mem-1");
        assert!(listed.items[0].pinned);
    }

    #[test]
    fn memory_update_updates_cached_hoppi_record_after_host_mutation() {
        let mut runtime = Runtime::new();
        let runtime_thread = thread(&mut runtime);
        let record = MemoryRecord {
            id: "mem-1".to_string(),
            content: "Edit this native memory through the Hoppi bridge.".to_string(),
            tags: vec!["draft".to_string()],
            confidence: Some("observed".to_string()),
            ..MemoryRecord::default()
        };
        runtime
            .set_memory_records(MemoryRecordsSetParams {
                thread_id: runtime_thread.id.clone(),
                records: vec![record],
                status: Some(MemoryStatus {
                    enabled: true,
                    initialized: true,
                    backend: "hoppi".to_string(),
                    scope: "project".to_string(),
                    memory_count: 1,
                    pinned_count: 0,
                    ..MemoryStatus::default()
                }),
                source: Some("hoppi-list".to_string()),
            })
            .unwrap();

        let updated = runtime
            .update_memory_record(MemoryRecordUpdateParams {
                thread_id: runtime_thread.id.clone(),
                id: "mem-1".to_string(),
                patch: MemoryRecordPatch {
                    content: Some("Edited through native update.".to_string()),
                    tags: Some(vec!["verified".to_string(), "native".to_string()]),
                    confidence: Some("verified".to_string()),
                    ..MemoryRecordPatch::default()
                },
                backend_applied: true,
                record: Some(MemoryRecord {
                    id: "mem-1".to_string(),
                    content: "Edited through native update.".to_string(),
                    tags: vec!["verified".to_string(), "native".to_string()],
                    confidence: Some("verified".to_string()),
                    ..MemoryRecord::default()
                }),
                status: None,
                source: Some("hoppi-update".to_string()),
            })
            .unwrap();

        assert_eq!(updated.action, "update");
        assert_eq!(updated.id, "mem-1");
        assert!(updated.backend_applied);
        assert!(updated.cache_updated);
        let record = updated.record.as_ref().unwrap();
        assert_eq!(record.content, "Edited through native update.");
        assert_eq!(record.tags, vec!["verified", "native"]);
        assert_eq!(record.confidence.as_deref(), Some("verified"));
        assert_eq!(updated.status.memory_count, 1);
        assert_eq!(updated.status.pinned_count, 0);
        assert!(updated.events.iter().any(|event| matches!(
            &event.kind,
            EventKind::Diagnostic { diagnostic }
                if diagnostic.message == "memory record updated"
                    && diagnostic.metadata.get("runtimeOwned") == Some(&"true".to_string())
                    && diagnostic.metadata.get("cacheUpdated") == Some(&"true".to_string())
        )));

        let listed = runtime
            .memory_list(MemoryListParams {
                thread_id: runtime_thread.id,
                query: Some("edited".to_string()),
                limit: None,
                pinned_only: false,
                include_superseded: false,
            })
            .unwrap();
        assert_eq!(listed.total_count, 1);
        assert_eq!(listed.items[0].id, "mem-1");
        assert_eq!(listed.items[0].content, "Edited through native update.");
    }

    #[test]
    fn memory_supersede_marks_cached_record_and_hides_it_by_default() {
        let mut runtime = Runtime::new();
        let runtime_thread = thread(&mut runtime);
        runtime
            .set_memory_records(MemoryRecordsSetParams {
                thread_id: runtime_thread.id.clone(),
                records: vec![
                    MemoryRecord {
                        id: "mem-old".to_string(),
                        content: "Older memory that should be superseded.".to_string(),
                        pinned: true,
                        ..MemoryRecord::default()
                    },
                    MemoryRecord {
                        id: "mem-new".to_string(),
                        content: "Newer memory that replaces the old one.".to_string(),
                        ..MemoryRecord::default()
                    },
                ],
                status: Some(MemoryStatus {
                    enabled: true,
                    initialized: true,
                    backend: "hoppi".to_string(),
                    scope: "project".to_string(),
                    memory_count: 2,
                    pinned_count: 1,
                    ..MemoryStatus::default()
                }),
                source: Some("hoppi-list".to_string()),
            })
            .unwrap();

        let superseded = runtime
            .supersede_memory_record(MemoryRecordSupersedeParams {
                thread_id: runtime_thread.id.clone(),
                id: "mem-old".to_string(),
                superseded_by: "mem-new".to_string(),
                backend_applied: true,
                record: None,
                status: None,
                source: Some("hoppi-supersede".to_string()),
            })
            .unwrap();

        assert_eq!(superseded.action, "supersede");
        assert_eq!(superseded.id, "mem-old");
        assert!(superseded.backend_applied);
        assert!(superseded.cache_updated);
        assert_eq!(
            superseded
                .record
                .as_ref()
                .and_then(|record| record.superseded_by.as_deref()),
            Some("mem-new")
        );
        assert_eq!(superseded.status.memory_count, 2);
        assert_eq!(superseded.status.pinned_count, 1);
        assert!(superseded.events.iter().any(|event| matches!(
            &event.kind,
            EventKind::Diagnostic { diagnostic }
                if diagnostic.message == "memory record superseded"
                    && diagnostic.metadata.get("runtimeOwned") == Some(&"true".to_string())
                    && diagnostic.metadata.get("supersededBy") == Some(&"mem-new".to_string())
        )));

        let default_list = runtime
            .memory_list(MemoryListParams {
                thread_id: runtime_thread.id.clone(),
                query: None,
                limit: None,
                pinned_only: false,
                include_superseded: false,
            })
            .unwrap();
        assert_eq!(default_list.total_count, 1);
        assert_eq!(default_list.items[0].id, "mem-new");

        let full_list = runtime
            .memory_list(MemoryListParams {
                thread_id: runtime_thread.id,
                query: None,
                limit: None,
                pinned_only: false,
                include_superseded: true,
            })
            .unwrap();
        assert_eq!(full_list.total_count, 2);
        assert_eq!(full_list.items[0].id, "mem-old");
        assert_eq!(full_list.items[0].superseded_by.as_deref(), Some("mem-new"));
    }

    #[test]
    fn memory_consolidate_records_backend_result_and_updates_cache() {
        let mut runtime = Runtime::new();
        let runtime_thread = thread(&mut runtime);
        runtime
            .set_memory_records(MemoryRecordsSetParams {
                thread_id: runtime_thread.id.clone(),
                records: vec![
                    MemoryRecord {
                        id: "mem-noise".to_string(),
                        content: "Noisy duplicate detail.".to_string(),
                        ..MemoryRecord::default()
                    },
                    MemoryRecord {
                        id: "mem-keep".to_string(),
                        content: "Keep this memory.".to_string(),
                        pinned: true,
                        ..MemoryRecord::default()
                    },
                ],
                status: Some(MemoryStatus {
                    enabled: true,
                    initialized: true,
                    backend: "hoppi".to_string(),
                    scope: "project".to_string(),
                    memory_count: 2,
                    pinned_count: 1,
                    queue_depth: Some(1),
                    ..MemoryStatus::default()
                }),
                source: Some("hoppi-list".to_string()),
            })
            .unwrap();

        let consolidated_record = MemoryRecord {
            id: "mem-consolidated".to_string(),
            content: "Consolidated implementation memory.".to_string(),
            tags: vec!["consolidated".to_string()],
            confidence: Some("verified".to_string()),
            parents: vec!["mem-noise".to_string()],
            ..MemoryRecord::default()
        };
        let consolidated = runtime
            .memory_consolidate(MemoryConsolidateParams {
                thread_id: runtime_thread.id.clone(),
                stage: Some("idle-dream".to_string()),
                summary: Some("Merged duplicate implementation detail.".to_string()),
                model_used: Some("gpt-5.4-mini".to_string()),
                dry_run: false,
                backend_applied: true,
                records: vec![consolidated_record.clone()],
                deleted_ids: vec!["mem-noise".to_string()],
                status: Some(MemoryStatus {
                    enabled: true,
                    initialized: true,
                    backend: "hoppi".to_string(),
                    scope: "project".to_string(),
                    memory_count: 2,
                    pinned_count: 1,
                    last_consolidated_at: Some("2026-05-31T10:00:00Z".to_string()),
                    queue_depth: Some(0),
                    ..MemoryStatus::default()
                }),
                source: Some("hoppi-consolidate".to_string()),
            })
            .unwrap();

        assert!(consolidated.available);
        assert_eq!(consolidated.stage.as_deref(), Some("idle-dream"));
        assert!(consolidated.backend_applied);
        assert!(consolidated.cache_updated);
        assert_eq!(consolidated.model_used.as_deref(), Some("gpt-5.4-mini"));
        assert_eq!(consolidated.created_count, 1);
        assert_eq!(consolidated.deleted_count, 1);
        assert_eq!(consolidated.deleted_ids, vec!["mem-noise"]);
        assert_eq!(consolidated.status.memory_count, 2);
        assert_eq!(consolidated.status.pinned_count, 1);
        assert_eq!(
            consolidated.status.last_consolidated_at.as_deref(),
            Some("2026-05-31T10:00:00Z")
        );
        assert!(consolidated.events.iter().any(|event| matches!(
            &event.kind,
            EventKind::Diagnostic { diagnostic }
                if diagnostic.message == "memory consolidation recorded"
                    && diagnostic.metadata.get("runtimeOwned") == Some(&"true".to_string())
                    && diagnostic.metadata.get("backendApplied") == Some(&"true".to_string())
                    && diagnostic.metadata.get("cacheUpdated") == Some(&"true".to_string())
                    && diagnostic.metadata.get("stage") == Some(&"idle-dream".to_string())
                    && diagnostic.metadata.get("modelUsed") == Some(&"gpt-5.4-mini".to_string())
        )));

        let listed = runtime
            .memory_list(MemoryListParams {
                thread_id: runtime_thread.id,
                query: None,
                limit: None,
                pinned_only: false,
                include_superseded: true,
            })
            .unwrap();
        assert_eq!(listed.total_count, 2);
        assert!(!listed.items.iter().any(|record| record.id == "mem-noise"));
        assert!(listed.items.iter().any(|record| record.id == "mem-keep"));
        assert!(
            listed
                .items
                .iter()
                .any(|record| record == &consolidated_record)
        );
    }

    #[test]
    fn memory_jobs_track_active_queue_depth_and_history() {
        let mut runtime = Runtime::new();
        let runtime_thread = thread(&mut runtime);

        let started = runtime
            .update_memory_job(MemoryJobUpdateParams {
                thread_id: runtime_thread.id.clone(),
                job_id: Some("mem-job-1".to_string()),
                kind: MemoryJobKind::Consolidate,
                stage: Some("idle-dream".to_string()),
                status: MemoryJobStatus::Running,
                source: Some("native-shell".to_string()),
                message: Some("Running idle Hoppi consolidation.".to_string()),
                queued_at: Some("2026-05-31T12:00:00Z".to_string()),
                started_at: Some("2026-05-31T12:00:01Z".to_string()),
                finished_at: None,
                record_count: 0,
                deleted_count: 0,
                dry_run: false,
                backend_applied: false,
                agent_model: None,
                deep_model: None,
                model_used: None,
            })
            .unwrap();

        assert_eq!(started.queue_depth, 1);
        assert_eq!(started.status.queue_depth, Some(1));
        assert_eq!(started.items[0].status, MemoryJobStatus::Running);
        assert_eq!(runtime.memory_status().queue_depth, Some(1));

        let finished = runtime
            .update_memory_job(MemoryJobUpdateParams {
                thread_id: runtime_thread.id.clone(),
                job_id: Some("mem-job-1".to_string()),
                kind: MemoryJobKind::Consolidate,
                stage: Some("idle-dream".to_string()),
                status: MemoryJobStatus::Succeeded,
                source: Some("native-shell".to_string()),
                message: Some("Applied idle Hoppi consolidation.".to_string()),
                queued_at: None,
                started_at: None,
                finished_at: Some("2026-05-31T12:00:10Z".to_string()),
                record_count: 2,
                deleted_count: 1,
                dry_run: false,
                backend_applied: true,
                agent_model: None,
                deep_model: None,
                model_used: None,
            })
            .unwrap();

        assert_eq!(finished.queue_depth, 0);
        assert_eq!(finished.status.queue_depth, Some(0));
        assert_eq!(runtime.memory_status().queue_depth, Some(0));

        let listed = runtime
            .list_memory_jobs(MemoryJobsListParams {
                thread_id: Some(runtime_thread.id),
                limit: Some(10),
                include_finished: true,
            })
            .unwrap();
        assert_eq!(listed.total_count, 1);
        assert_eq!(listed.items[0].id, "mem-job-1");
        assert_eq!(listed.items[0].status, MemoryJobStatus::Succeeded);
        assert_eq!(listed.items[0].record_count, 2);
        assert_eq!(listed.items[0].deleted_count, 1);
    }

    #[test]
    fn memory_jobs_cancel_active_jobs_and_update_queue_depth() {
        let mut runtime = Runtime::new();
        let runtime_thread = thread(&mut runtime);

        runtime
            .update_memory_job(MemoryJobUpdateParams {
                thread_id: runtime_thread.id.clone(),
                job_id: Some("mem-job-cancel".to_string()),
                kind: MemoryJobKind::Remember,
                stage: Some("turn-summary".to_string()),
                status: MemoryJobStatus::Running,
                source: Some("native-shell".to_string()),
                message: Some("Saving Hoppi memory.".to_string()),
                queued_at: Some("2026-05-31T12:00:00Z".to_string()),
                started_at: Some("2026-05-31T12:00:01Z".to_string()),
                finished_at: None,
                record_count: 0,
                deleted_count: 0,
                dry_run: false,
                backend_applied: false,
                agent_model: None,
                deep_model: None,
                model_used: None,
            })
            .unwrap();
        assert_eq!(runtime.memory_status().queue_depth, Some(1));

        let cancelled = runtime
            .cancel_memory_job(MemoryJobCancelParams {
                thread_id: runtime_thread.id.clone(),
                job_id: "mem-job-cancel".to_string(),
                reason: Some("user requested cancellation".to_string()),
                finished_at: Some("2026-05-31T12:00:05Z".to_string()),
            })
            .unwrap();

        assert_eq!(cancelled.queue_depth, 0);
        assert_eq!(cancelled.status.queue_depth, Some(0));
        assert_eq!(runtime.memory_status().queue_depth, Some(0));
        let job = cancelled
            .items
            .iter()
            .find(|job| job.id == "mem-job-cancel")
            .unwrap();
        assert_eq!(job.status, MemoryJobStatus::Cancelled);
        assert_eq!(job.kind, MemoryJobKind::Remember);
        assert_eq!(job.stage.as_deref(), Some("turn-summary"));
        assert_eq!(job.message.as_deref(), Some("user requested cancellation"));
        assert_eq!(job.finished_at.as_deref(), Some("2026-05-31T12:00:05Z"));
        assert_eq!(cancelled.events.len(), 1);

        let repeat = runtime.cancel_memory_job(MemoryJobCancelParams {
            thread_id: runtime_thread.id,
            job_id: "mem-job-cancel".to_string(),
            reason: Some("again".to_string()),
            finished_at: None,
        });
        assert!(repeat.is_err());
    }

    #[test]
    fn memory_backend_job_finish_does_not_overwrite_cancelled_job() {
        let mut runtime = Runtime::new();
        let runtime_thread = thread(&mut runtime);
        let job_id = runtime
            .start_memory_backend_job(
                &runtime_thread.id,
                MemoryJobKind::Sync,
                "sync-both",
                "Syncing Hoppi memories through runtime backend broker.",
                false,
            )
            .unwrap();

        runtime
            .cancel_memory_job(MemoryJobCancelParams {
                thread_id: runtime_thread.id.clone(),
                job_id: job_id.clone(),
                reason: Some("user cancelled sync".to_string()),
                finished_at: Some("2026-06-01T12:00:00Z".to_string()),
            })
            .unwrap();

        runtime
            .finish_memory_backend_job(
                &runtime_thread.id,
                &job_id,
                MemoryJobKind::Sync,
                "sync-both",
                "Synced Hoppi memories through runtime backend broker.",
                4,
                0,
                false,
                true,
            )
            .unwrap();

        let jobs = runtime
            .list_memory_jobs(MemoryJobsListParams {
                thread_id: Some(runtime_thread.id),
                include_finished: true,
                limit: Some(10),
            })
            .unwrap();
        assert_eq!(jobs.queue_depth, 0);
        let job = jobs.items.iter().find(|job| job.id == job_id).unwrap();
        assert_eq!(job.status, MemoryJobStatus::Cancelled);
        assert_eq!(job.message.as_deref(), Some("user cancelled sync"));
        assert_eq!(job.record_count, 0);
        assert!(!job.backend_applied);

        let result = runtime
            .memory_backend_execution_result(
                job_id,
                MemoryBackendOperation::Sync,
                &MemoryBackendRawResult {
                    backend: "mock-hoppi".to_string(),
                    adapter_kind: Some("mock-rust".to_string()),
                    value: json!({ "imported": 4 }),
                },
                &json!({ "imported": 4 }),
                "Synced Hoppi memories through runtime backend broker.".to_string(),
                Vec::new(),
            )
            .unwrap();
        assert!(result.cancelled);
    }

    #[test]
    fn skill_discovery_lists_builtins_and_project_overrides() {
        let root = std::env::temp_dir().join(format!("oppi-skills-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".agents/skills/independent")).unwrap();
        fs::write(
            root.join(".agents/skills/independent/SKILL.md"),
            "---\nname: independent\ndescription: Project-specific autonomous plan runner. Use for project plans.\n---\n\n# Project Independent\n",
        )
        .unwrap();
        let mut runtime = Runtime::new();
        let thread = runtime
            .start_thread(ThreadStartParams {
                project: ProjectRef {
                    id: "skills".to_string(),
                    cwd: root.display().to_string(),
                    display_name: None,
                    workspace_roots: Vec::new(),
                },
                title: None,
            })
            .thread;
        let skills = runtime
            .list_skills(SkillListParams {
                thread_id: Some(thread.id),
            })
            .unwrap()
            .items;
        assert!(skills.iter().any(|skill| skill.active.name == "imagegen"));
        assert!(skills.iter().any(|skill| skill.active.name == "graphify"));
        let independent = skills
            .iter()
            .find(|skill| skill.active.name == "independent")
            .unwrap();
        assert_eq!(independent.active.source, SkillSource::Project);
        assert!(
            independent
                .shadowed
                .iter()
                .any(|skill| skill.source == SkillSource::BuiltIn)
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn skill_discovery_loads_enabled_trusted_local_plugin_skills() {
        let root = std::env::temp_dir().join(format!("oppi-plugin-skills-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let plugin = root.join("plugin-pack");
        fs::create_dir_all(plugin.join("skills/plugin-scout")).unwrap();
        fs::write(
            plugin.join("package.json"),
            r#"{"name":"plugin-pack","version":"1.0.0","pi":{"skills":["./skills"],"extensions":["./extensions"]}}"#,
        )
        .unwrap();
        fs::write(
            plugin.join("skills/plugin-scout/SKILL.md"),
            "---\nname: plugin-scout\ndescription: Inspect enabled plugin resources for native OPPi.\n---\n\n# Plugin Scout\n",
        )
        .unwrap();
        fs::create_dir_all(root.join(".oppi")).unwrap();
        fs::write(
            root.join(".oppi/plugins.json"),
            format!(
                r#"{{
  "version": 1,
  "plugins": [
    {{
      "name": "plugin-pack",
      "source": {},
      "sourceType": "local",
      "enabled": true,
      "trusted": true,
      "addedAt": "2026-05-26T00:00:00.000Z",
      "updatedAt": "2026-05-26T00:00:00.000Z"
    }}
  ]
}}"#,
                serde_json::to_string(&plugin.display().to_string()).unwrap()
            ),
        )
        .unwrap();

        let mut runtime = Runtime::new();
        let thread = runtime
            .start_thread(ThreadStartParams {
                project: ProjectRef {
                    id: "plugin-skills".to_string(),
                    cwd: root.display().to_string(),
                    display_name: None,
                    workspace_roots: Vec::new(),
                },
                title: None,
            })
            .thread;
        let skills = runtime
            .list_skills(SkillListParams {
                thread_id: Some(thread.id),
            })
            .unwrap()
            .items;
        let plugin_skill = skills
            .iter()
            .find(|skill| skill.active.name == "plugin-scout")
            .expect("enabled trusted plugin skill should be discovered");
        assert_eq!(plugin_skill.active.source, SkillSource::Plugin);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn plugin_prompt_templates_load_from_enabled_trusted_local_plugins() {
        let _env_lock = TEST_ENV_LOCK.lock().unwrap();
        let root = std::env::temp_dir().join(format!("oppi-plugin-prompts-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let plugin = root.join("prompt-pack");
        let oppi_home = root.join("oppi-home");
        fs::create_dir_all(plugin.join("prompts")).unwrap();
        fs::create_dir_all(root.join(".oppi")).unwrap();
        fs::create_dir_all(&oppi_home).unwrap();
        let _oppi_home = EnvVarGuard::set("OPPI_HOME", &oppi_home);
        let _plugin_sources = EnvVarGuard::remove("OPPI_PLUGIN_SOURCES");
        fs::write(
            plugin.join("package.json"),
            r#"{"name":"prompt-pack","version":"1.0.0","pi":{"prompts":["./prompts"]}}"#,
        )
        .unwrap();
        fs::write(
            plugin.join("prompts/component.md"),
            "---\ndescription: Create a component\nargument-hint: \"<name> [features]\"\n---\nCreate a React component named $1 with features: $@.\n",
        )
        .unwrap();
        fs::write(
            root.join(".oppi/plugins.json"),
            format!(
                r#"{{
  "version": 1,
  "plugins": [
    {{
      "name": "prompt-pack",
      "source": {},
      "sourceType": "local",
      "enabled": true,
      "trusted": true
    }}
  ]
}}"#,
                serde_json::to_string(&plugin.display().to_string()).unwrap()
            ),
        )
        .unwrap();

        let mut runtime = Runtime::new();
        let thread = runtime
            .start_thread(ThreadStartParams {
                project: ProjectRef {
                    id: "plugin-prompts".to_string(),
                    cwd: root.display().to_string(),
                    display_name: None,
                    workspace_roots: Vec::new(),
                },
                title: None,
            })
            .thread;
        let templates = runtime
            .list_prompt_templates(PromptTemplateListParams {
                thread_id: Some(thread.id.clone()),
            })
            .unwrap()
            .items;
        let component = templates
            .iter()
            .find(|template| template.active.name == "component")
            .expect("enabled trusted plugin prompt should be discovered");
        assert_eq!(component.active.source, PromptTemplateSource::Plugin);
        assert_eq!(
            component.active.argument_hint.as_deref(),
            Some("<name> [features]")
        );

        let plugins = runtime
            .list_plugins(PluginListParams {
                thread_id: Some(thread.id),
            })
            .unwrap()
            .items;
        let plugin = plugins
            .iter()
            .find(|plugin| plugin.id == "prompt-pack")
            .expect("configured prompt plugin should be listed");
        assert!(plugin.resources.iter().any(|resource| {
            resource.kind == PluginResourceKind::Prompt
                && resource.status == PluginResourceStatus::Loaded
                && resource.path.ends_with("prompts/component.md")
        }));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn plugin_prompt_templates_filter_disabled_untrusted_and_non_local_entries() {
        let _env_lock = TEST_ENV_LOCK.lock().unwrap();
        let root = std::env::temp_dir().join(format!(
            "oppi-plugin-prompt-boundaries-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        let oppi_home = root.join("oppi-home");
        fs::create_dir_all(root.join(".oppi")).unwrap();
        fs::create_dir_all(&oppi_home).unwrap();
        let _oppi_home = EnvVarGuard::set("OPPI_HOME", &oppi_home);
        let _plugin_sources = EnvVarGuard::remove("OPPI_PLUGIN_SOURCES");
        let cases = [
            ("disabled-prompt", "local", false, true),
            ("untrusted-prompt", "local", true, false),
            ("npm-prompt", "npm", true, true),
        ];
        for (name, _, _, _) in cases {
            let plugin = root.join(name);
            fs::create_dir_all(plugin.join("prompts")).unwrap();
            fs::write(
                plugin.join("package.json"),
                r#"{"name":"boundary-prompts","version":"1.0.0","pi":{"prompts":["./prompts"]}}"#,
            )
            .unwrap();
            fs::write(
                plugin.join("prompts").join(format!("{name}.md")),
                format!(
                    "---\ndescription: Boundary test prompt.\n---\nThis prompt must not load: {name}\n"
                ),
            )
            .unwrap();
        }
        let entries = cases
            .iter()
            .map(|(name, source_type, enabled, trusted)| {
                let source = serde_json::to_string(&root.join(name).display().to_string()).unwrap();
                format!(
                    r#"{{
      "name": "{name}",
      "source": {source},
      "sourceType": "{source_type}",
      "enabled": {enabled},
      "trusted": {trusted}
    }}"#
                )
            })
            .collect::<Vec<_>>()
            .join(",\n");
        fs::write(
            root.join(".oppi/plugins.json"),
            format!("{{\n  \"version\": 1,\n  \"plugins\": [\n{entries}\n  ]\n}}\n"),
        )
        .unwrap();

        let mut runtime = Runtime::new();
        let thread = runtime
            .start_thread(ThreadStartParams {
                project: ProjectRef {
                    id: "plugin-prompt-boundaries".to_string(),
                    cwd: root.display().to_string(),
                    display_name: None,
                    workspace_roots: Vec::new(),
                },
                title: None,
            })
            .thread;
        let templates = runtime
            .list_prompt_templates(PromptTemplateListParams {
                thread_id: Some(thread.id),
            })
            .unwrap()
            .items;
        for name in ["disabled-prompt", "untrusted-prompt", "npm-prompt"] {
            assert!(
                templates
                    .iter()
                    .all(|template| template.active.name != name),
                "{name} must not be loaded as a native plugin prompt template"
            );
        }

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn prompt_template_expand_supports_pi_argument_substitution() {
        let _env_lock = TEST_ENV_LOCK.lock().unwrap();
        let root =
            std::env::temp_dir().join(format!("oppi-plugin-prompt-expand-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let plugin = root.join("prompt-pack");
        let oppi_home = root.join("oppi-home");
        fs::create_dir_all(plugin.join("prompts")).unwrap();
        fs::create_dir_all(root.join(".oppi")).unwrap();
        fs::create_dir_all(&oppi_home).unwrap();
        let _oppi_home = EnvVarGuard::set("OPPI_HOME", &oppi_home);
        let _plugin_sources = EnvVarGuard::remove("OPPI_PLUGIN_SOURCES");
        fs::write(
            plugin.join("package.json"),
            r#"{"name":"prompt-pack","version":"1.0.0","pi":{"prompts":["./prompts"]}}"#,
        )
        .unwrap();
        fs::write(
            plugin.join("prompts/component.md"),
            "---\ndescription: Create a component\n---\nname=$1\nsecond=$2\nall=$@\narguments=$ARGUMENTS\nfrom2=${@:2}\nonly2=${@:2:1}\n",
        )
        .unwrap();
        fs::write(
            root.join(".oppi/plugins.json"),
            format!(
                r#"{{
  "version": 1,
  "plugins": [
    {{
      "name": "prompt-pack",
      "source": {},
      "sourceType": "local",
      "enabled": true,
      "trusted": true
    }}
  ]
}}"#,
                serde_json::to_string(&plugin.display().to_string()).unwrap()
            ),
        )
        .unwrap();

        let mut runtime = Runtime::new();
        let thread = runtime
            .start_thread(ThreadStartParams {
                project: ProjectRef {
                    id: "plugin-prompt-expand".to_string(),
                    cwd: root.display().to_string(),
                    display_name: None,
                    workspace_roots: Vec::new(),
                },
                title: None,
            })
            .thread;
        let expanded = runtime
            .expand_prompt_template(PromptTemplateExpandParams {
                thread_id: Some(thread.id),
                name: "component".to_string(),
                args: "Button \"click handler\" disabled".to_string(),
                prompt_variant_append: Some("Variant append".to_string()),
            })
            .unwrap();
        assert!(expanded.input.contains("name=Button"));
        assert!(expanded.input.contains("second=click handler"));
        assert!(expanded.input.contains("all=Button click handler disabled"));
        assert!(
            expanded
                .input
                .contains("arguments=Button click handler disabled")
        );
        assert!(expanded.input.contains("from2=click handler disabled"));
        assert!(expanded.input.contains("only2=click handler"));
        assert!(expanded.input.ends_with("\n\nVariant append"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn command_prepare_expands_plugin_prompt_template_after_builtin_commands() {
        let _env_lock = TEST_ENV_LOCK.lock().unwrap();
        let root =
            std::env::temp_dir().join(format!("oppi-plugin-prompt-command-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let plugin = root.join("prompt-pack");
        let oppi_home = root.join("oppi-home");
        fs::create_dir_all(plugin.join("prompts")).unwrap();
        fs::create_dir_all(root.join(".oppi")).unwrap();
        fs::create_dir_all(&oppi_home).unwrap();
        let _oppi_home = EnvVarGuard::set("OPPI_HOME", &oppi_home);
        let _plugin_sources = EnvVarGuard::remove("OPPI_PLUGIN_SOURCES");
        fs::write(
            plugin.join("package.json"),
            r#"{"name":"prompt-pack","version":"1.0.0","pi":{"prompts":["./prompts"]}}"#,
        )
        .unwrap();
        fs::write(
            plugin.join("prompts/component.md"),
            "---\ndescription: Create a component\n---\nCreate component $1.\n",
        )
        .unwrap();
        fs::write(
            plugin.join("prompts/review.md"),
            "---\ndescription: Plugin review prompt\n---\nPlugin review should not replace built-in review.\n",
        )
        .unwrap();
        fs::write(
            root.join(".oppi/plugins.json"),
            format!(
                r#"{{
  "version": 1,
  "plugins": [
    {{
      "name": "prompt-pack",
      "source": {},
      "sourceType": "local",
      "enabled": true,
      "trusted": true
    }}
  ]
}}"#,
                serde_json::to_string(&plugin.display().to_string()).unwrap()
            ),
        )
        .unwrap();

        let mut runtime = Runtime::new();
        let thread = runtime
            .start_thread(ThreadStartParams {
                project: ProjectRef {
                    id: "plugin-prompt-command".to_string(),
                    cwd: root.display().to_string(),
                    display_name: None,
                    workspace_roots: Vec::new(),
                },
                title: None,
            })
            .thread;
        let component = runtime
            .prepare_command(CommandPrepareParams {
                thread_id: Some(thread.id.clone()),
                command: "component".to_string(),
                args: "Button".to_string(),
                context: json!({}),
                prompt_variant_append: None,
            })
            .unwrap();
        assert_eq!(component.command, "component");
        assert!(component.input.contains("Create component Button."));
        assert!(
            component
                .notes
                .iter()
                .any(|note| note.contains("prompt template"))
        );

        let review = runtime
            .prepare_command(CommandPrepareParams {
                thread_id: Some(thread.id),
                command: "review".to_string(),
                args: String::new(),
                context: json!({}),
                prompt_variant_append: None,
            })
            .unwrap();
        assert_eq!(review.system_prompt_profile.as_deref(), Some("review"));
        assert!(!review.input.contains("Plugin review should not replace"));

        let _ = fs::remove_dir_all(&root);
    }

    fn valid_pi_theme_json(name: &str) -> String {
        let mut colors = serde_json::Map::new();
        for token in [
            "accent",
            "border",
            "borderAccent",
            "borderMuted",
            "success",
            "error",
            "warning",
            "muted",
            "dim",
            "text",
            "thinkingText",
            "selectedBg",
            "userMessageBg",
            "userMessageText",
            "customMessageBg",
            "customMessageText",
            "customMessageLabel",
            "toolPendingBg",
            "toolSuccessBg",
            "toolErrorBg",
            "toolTitle",
            "toolOutput",
            "mdHeading",
            "mdLink",
            "mdLinkUrl",
            "mdCode",
            "mdCodeBlock",
            "mdCodeBlockBorder",
            "mdQuote",
            "mdQuoteBorder",
            "mdHr",
            "mdListBullet",
            "toolDiffAdded",
            "toolDiffRemoved",
            "toolDiffContext",
            "syntaxComment",
            "syntaxKeyword",
            "syntaxFunction",
            "syntaxVariable",
            "syntaxString",
            "syntaxNumber",
            "syntaxType",
            "syntaxOperator",
            "syntaxPunctuation",
            "thinkingOff",
            "thinkingMinimal",
            "thinkingLow",
            "thinkingMedium",
            "thinkingHigh",
            "thinkingXhigh",
            "bashMode",
        ] {
            colors.insert(token.to_string(), json!("#223344"));
        }
        colors.insert("accent".to_string(), json!("brand"));
        colors.insert("borderAccent".to_string(), json!("brandSoft"));
        colors.insert("muted".to_string(), json!(242));
        colors.insert("text".to_string(), json!(""));
        colors.insert("warning".to_string(), json!("warn"));
        colors.insert("selectedBg".to_string(), json!("#111111"));
        colors.insert("permissionAutoReviewingBg".to_string(), json!("brand"));
        colors.insert("permissionAutoApprovedBg".to_string(), json!("#002200"));
        colors.insert("permissionAutoDeniedBg".to_string(), json!("#220000"));
        json!({
            "name": name,
            "vars": {
                "brand": "#123abc",
                "brandSoft": "brand",
                "warn": 220,
                "page": "#010203"
            },
            "colors": colors,
            "export": {
                "pageBg": "page",
                "cardBg": "#0a0b0c"
            }
        })
        .to_string()
    }

    #[test]
    fn plugin_themes_load_from_enabled_trusted_local_plugins() {
        let _env_lock = TEST_ENV_LOCK.lock().unwrap();
        let root =
            std::env::temp_dir().join(format!("oppi-plugin-theme-load-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let plugin = root.join("theme-pack");
        let oppi_home = root.join("oppi-home");
        fs::create_dir_all(plugin.join("themes")).unwrap();
        fs::create_dir_all(root.join(".oppi")).unwrap();
        fs::create_dir_all(&oppi_home).unwrap();
        let _oppi_home = EnvVarGuard::set("OPPI_HOME", &oppi_home);
        let _plugin_sources = EnvVarGuard::remove("OPPI_PLUGIN_SOURCES");
        fs::write(
            plugin.join("package.json"),
            r#"{"name":"theme-pack","version":"1.0.0","pi":{"themes":["./themes"]}}"#,
        )
        .unwrap();
        fs::write(plugin.join("themes/acme.json"), valid_pi_theme_json("acme")).unwrap();
        fs::write(
            root.join(".oppi/plugins.json"),
            format!(
                r#"{{
  "version": 1,
  "plugins": [
    {{
      "name": "theme-pack",
      "source": {},
      "sourceType": "local",
      "enabled": true,
      "trusted": true
    }}
  ]
}}"#,
                serde_json::to_string(&plugin.display().to_string()).unwrap()
            ),
        )
        .unwrap();

        let mut runtime = Runtime::new();
        let thread = runtime
            .start_thread(ThreadStartParams {
                project: ProjectRef {
                    id: "plugin-themes".to_string(),
                    cwd: root.display().to_string(),
                    display_name: None,
                    workspace_roots: Vec::new(),
                },
                title: None,
            })
            .thread;
        let themes = runtime
            .list_themes(ThemeListParams {
                thread_id: Some(thread.id.clone()),
            })
            .unwrap()
            .items;
        let acme = themes
            .iter()
            .find(|theme| theme.active.name == "acme")
            .expect("enabled trusted plugin theme should be discovered");
        assert_eq!(acme.active.source, ThemeSource::Plugin);
        let loaded = runtime
            .load_theme(ThemeLoadParams {
                thread_id: Some(thread.id.clone()),
                name: "acme".to_string(),
            })
            .unwrap();
        assert_eq!(loaded.theme.name, "acme");
        assert_eq!(loaded.colors["accent"], json!("#123abc"));
        assert_eq!(loaded.colors["borderAccent"], json!("#123abc"));
        assert_eq!(loaded.colors["muted"], json!(242));
        assert_eq!(loaded.colors["text"], json!(""));
        assert_eq!(loaded.colors["permissionAutoReviewingBg"], json!("#123abc"));
        assert_eq!(loaded.export.as_ref().unwrap()["pageBg"], json!("#010203"));

        let plugins = runtime
            .list_plugins(PluginListParams {
                thread_id: Some(thread.id),
            })
            .unwrap()
            .items;
        let plugin = plugins
            .iter()
            .find(|plugin| plugin.id == "theme-pack")
            .expect("configured theme plugin should be listed");
        assert!(plugin.resources.iter().any(|resource| {
            resource.kind == PluginResourceKind::Theme
                && resource.status == PluginResourceStatus::Loaded
                && resource.path.ends_with("themes/acme.json")
        }));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn builtin_themes_include_pi_oppi_cyan_names_and_alias() {
        let runtime = Runtime::new();
        let themes = runtime
            .list_themes(ThemeListParams { thread_id: None })
            .unwrap()
            .items;
        let names = themes
            .iter()
            .map(|theme| theme.active.name.as_str())
            .collect::<BTreeSet<_>>();

        assert!(names.contains("oppi-cyan"));
        assert!(names.contains("oppi-cyan-light"));
        assert_eq!(
            themes
                .iter()
                .find(|theme| theme.active.name == "oppi-cyan")
                .unwrap()
                .active
                .source,
            ThemeSource::BuiltIn
        );

        let dark = runtime
            .load_theme(ThemeLoadParams {
                thread_id: None,
                name: "oppi-cyan-dark".to_string(),
            })
            .unwrap();
        assert_eq!(dark.theme.name, "oppi-cyan");
        assert_eq!(dark.colors["accent"], json!("#39d7e5"));
        assert_eq!(dark.export.as_ref().unwrap()["pageBg"], json!("#0f1419"));

        let light = runtime
            .load_theme(ThemeLoadParams {
                thread_id: None,
                name: "oppi-cyan-light".to_string(),
            })
            .unwrap();
        assert_eq!(light.colors["text"], json!("#16252f"));
        assert_eq!(light.colors["accent"], json!("#006d7d"));
        assert_eq!(light.export.as_ref().unwrap()["pageBg"], json!("#eef7f4"));
    }

    #[test]
    fn plugin_themes_filter_disabled_untrusted_and_non_local_entries() {
        let _env_lock = TEST_ENV_LOCK.lock().unwrap();
        let root = std::env::temp_dir().join(format!(
            "oppi-plugin-theme-boundaries-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        let oppi_home = root.join("oppi-home");
        fs::create_dir_all(root.join(".oppi")).unwrap();
        fs::create_dir_all(&oppi_home).unwrap();
        let _oppi_home = EnvVarGuard::set("OPPI_HOME", &oppi_home);
        let _plugin_sources = EnvVarGuard::remove("OPPI_PLUGIN_SOURCES");
        let cases = [
            ("disabled-theme", "local", false, true),
            ("untrusted-theme", "local", true, false),
            ("npm-theme", "npm", true, true),
        ];
        for (name, _, _, _) in cases {
            let plugin = root.join(name);
            fs::create_dir_all(plugin.join("themes")).unwrap();
            fs::write(
                plugin.join("package.json"),
                r#"{"name":"boundary-themes","version":"1.0.0","pi":{"themes":["./themes"]}}"#,
            )
            .unwrap();
            fs::write(
                plugin.join("themes").join(format!("{name}.json")),
                valid_pi_theme_json(name),
            )
            .unwrap();
        }
        let entries = cases
            .iter()
            .map(|(name, source_type, enabled, trusted)| {
                let source = serde_json::to_string(&root.join(name).display().to_string()).unwrap();
                format!(
                    r#"{{
      "name": "{name}",
      "source": {source},
      "sourceType": "{source_type}",
      "enabled": {enabled},
      "trusted": {trusted}
    }}"#
                )
            })
            .collect::<Vec<_>>()
            .join(",\n");
        fs::write(
            root.join(".oppi/plugins.json"),
            format!("{{\n  \"version\": 1,\n  \"plugins\": [\n{entries}\n  ]\n}}\n"),
        )
        .unwrap();

        let mut runtime = Runtime::new();
        let thread = runtime
            .start_thread(ThreadStartParams {
                project: ProjectRef {
                    id: "plugin-theme-boundaries".to_string(),
                    cwd: root.display().to_string(),
                    display_name: None,
                    workspace_roots: Vec::new(),
                },
                title: None,
            })
            .thread;
        let themes = runtime
            .list_themes(ThemeListParams {
                thread_id: Some(thread.id),
            })
            .unwrap()
            .items;
        for name in ["disabled-theme", "untrusted-theme", "npm-theme"] {
            assert!(
                themes.iter().all(|theme| theme.active.name != name),
                "{name} must not be loaded as a native plugin theme"
            );
        }

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn theme_load_resolves_vars_and_pi_color_values() {
        let _env_lock = TEST_ENV_LOCK.lock().unwrap();
        let root =
            std::env::temp_dir().join(format!("oppi-plugin-theme-vars-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let plugin = root.join("theme-pack");
        let oppi_home = root.join("oppi-home");
        fs::create_dir_all(plugin.join("themes")).unwrap();
        fs::create_dir_all(root.join(".oppi")).unwrap();
        fs::create_dir_all(&oppi_home).unwrap();
        let _oppi_home = EnvVarGuard::set("OPPI_HOME", &oppi_home);
        let _plugin_sources = EnvVarGuard::remove("OPPI_PLUGIN_SOURCES");
        fs::write(
            plugin.join("package.json"),
            r#"{"name":"theme-pack","version":"1.0.0","pi":{"themes":["./themes"]}}"#,
        )
        .unwrap();
        fs::write(plugin.join("themes/acme.json"), valid_pi_theme_json("acme")).unwrap();
        fs::write(
            root.join(".oppi/plugins.json"),
            format!(
                r#"{{
  "version": 1,
  "plugins": [
    {{
      "name": "theme-pack",
      "source": {},
      "sourceType": "local",
      "enabled": true,
      "trusted": true
    }}
  ]
}}"#,
                serde_json::to_string(&plugin.display().to_string()).unwrap()
            ),
        )
        .unwrap();

        let mut runtime = Runtime::new();
        let thread = runtime
            .start_thread(ThreadStartParams {
                project: ProjectRef {
                    id: "plugin-theme-vars".to_string(),
                    cwd: root.display().to_string(),
                    display_name: None,
                    workspace_roots: Vec::new(),
                },
                title: None,
            })
            .thread;
        let loaded = runtime
            .load_theme(ThemeLoadParams {
                thread_id: Some(thread.id),
                name: "acme".to_string(),
            })
            .unwrap();
        assert_eq!(loaded.colors["accent"], json!("#123abc"));
        assert_eq!(loaded.colors["warning"], json!(220));
        assert_eq!(loaded.colors["text"], json!(""));
        assert_eq!(loaded.colors["permissionAutoApprovedBg"], json!("#002200"));
        assert_eq!(loaded.export.as_ref().unwrap()["cardBg"], json!("#0a0b0c"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn plugin_list_reports_invalid_theme_resource_diagnostics() {
        let _env_lock = TEST_ENV_LOCK.lock().unwrap();
        let root =
            std::env::temp_dir().join(format!("oppi-plugin-theme-invalid-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let plugin = root.join("theme-pack");
        let oppi_home = root.join("oppi-home");
        fs::create_dir_all(plugin.join("themes")).unwrap();
        fs::create_dir_all(root.join(".oppi")).unwrap();
        fs::create_dir_all(&oppi_home).unwrap();
        let _oppi_home = EnvVarGuard::set("OPPI_HOME", &oppi_home);
        let _plugin_sources = EnvVarGuard::remove("OPPI_PLUGIN_SOURCES");
        fs::write(
            plugin.join("package.json"),
            r#"{"name":"theme-pack","version":"1.0.0","pi":{"themes":["./themes","./missing","./bad.txt"]}}"#,
        )
        .unwrap();
        fs::write(plugin.join("themes/acme.json"), valid_pi_theme_json("acme")).unwrap();
        fs::write(
            plugin.join("themes/broken.json"),
            r#"{"name":"broken","colors":{"accent":"missingVar"}}"#,
        )
        .unwrap();
        fs::write(plugin.join("bad.txt"), "not a theme").unwrap();
        fs::write(
            root.join(".oppi/plugins.json"),
            format!(
                r#"{{
  "version": 1,
  "plugins": [
    {{
      "name": "theme-pack",
      "source": {},
      "sourceType": "local",
      "enabled": true,
      "trusted": true
    }}
  ]
}}"#,
                serde_json::to_string(&plugin.display().to_string()).unwrap()
            ),
        )
        .unwrap();

        let mut runtime = Runtime::new();
        let thread = runtime
            .start_thread(ThreadStartParams {
                project: ProjectRef {
                    id: "plugin-theme-invalid".to_string(),
                    cwd: root.display().to_string(),
                    display_name: None,
                    workspace_roots: Vec::new(),
                },
                title: None,
            })
            .thread;
        let plugins = runtime
            .list_plugins(PluginListParams {
                thread_id: Some(thread.id),
            })
            .unwrap()
            .items;
        let plugin = plugins
            .iter()
            .find(|plugin| plugin.id == "theme-pack")
            .expect("configured theme plugin should be listed");
        assert!(plugin.resources.iter().any(|resource| {
            resource.kind == PluginResourceKind::Theme
                && resource.status == PluginResourceStatus::Loaded
                && resource.path.ends_with("themes/acme.json")
        }));
        assert!(plugin.resources.iter().any(|resource| {
            resource.kind == PluginResourceKind::Theme
                && resource.status == PluginResourceStatus::Unsupported
                && resource.path.ends_with("themes/broken.json")
                && resource
                    .reason
                    .as_deref()
                    .is_some_and(|reason| reason.contains("missing required color token"))
        }));
        assert!(plugin.resources.iter().any(|resource| {
            resource.kind == PluginResourceKind::Theme
                && resource.status == PluginResourceStatus::Missing
                && resource.path.ends_with("missing")
        }));
        assert!(plugin.resources.iter().any(|resource| {
            resource.kind == PluginResourceKind::Theme
                && resource.status == PluginResourceStatus::Unsupported
                && resource.path.ends_with("bad.txt")
                && resource
                    .reason
                    .as_deref()
                    .is_some_and(|reason| reason.contains("json"))
        }));
        assert!(
            plugin
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "plugin_resource_unsupported")
        );
        assert!(
            plugin
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "plugin_resource_missing")
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn skill_discovery_loads_enabled_trusted_global_plugin_store_skills() {
        let _env_lock = TEST_ENV_LOCK.lock().unwrap();
        let root =
            std::env::temp_dir().join(format!("oppi-global-plugin-skills-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let oppi_home = root.join("oppi-home");
        let workspace = root.join("workspace");
        let plugin = root.join("global-plugin");
        fs::create_dir_all(plugin.join("skills/global-scout")).unwrap();
        fs::create_dir_all(&workspace).unwrap();
        fs::create_dir_all(&oppi_home).unwrap();
        let _oppi_home = EnvVarGuard::set("OPPI_HOME", &oppi_home);
        let _plugin_sources = EnvVarGuard::remove("OPPI_PLUGIN_SOURCES");
        fs::write(
            plugin.join("package.json"),
            r#"{"name":"global-plugin","version":"1.0.0","pi":{"skills":["./skills"]}}"#,
        )
        .unwrap();
        fs::write(
            plugin.join("skills/global-scout/SKILL.md"),
            "---\nname: global-scout\ndescription: Inspect globally enabled plugin resources for native OPPi.\n---\n\n# Global Scout\n",
        )
        .unwrap();
        fs::write(
            oppi_home.join("plugin-lock.json"),
            format!(
                r#"{{
  "version": 1,
  "plugins": [
    {{
      "name": "global-plugin",
      "source": {},
      "sourceType": "local",
      "enabled": true,
      "trusted": true
    }}
  ]
}}"#,
                serde_json::to_string(&plugin.display().to_string()).unwrap()
            ),
        )
        .unwrap();

        let mut runtime = Runtime::new();
        let thread = runtime
            .start_thread(ThreadStartParams {
                project: ProjectRef {
                    id: "global-plugin-skills".to_string(),
                    cwd: workspace.display().to_string(),
                    display_name: None,
                    workspace_roots: Vec::new(),
                },
                title: None,
            })
            .thread;
        let skills = runtime
            .list_skills(SkillListParams {
                thread_id: Some(thread.id.clone()),
            })
            .unwrap()
            .items;
        let plugin_skill = skills
            .iter()
            .find(|skill| skill.active.name == "global-scout")
            .expect("enabled trusted global plugin skill should be discovered");
        assert_eq!(plugin_skill.active.source, SkillSource::Plugin);
        let plugins = runtime
            .list_plugins(PluginListParams {
                thread_id: Some(thread.id),
            })
            .unwrap()
            .items;
        let plugin = plugins
            .iter()
            .find(|plugin| plugin.id == "global-plugin")
            .expect("global plugin should be listed");
        assert_eq!(plugin.scope, Some(PluginScope::Global));
        assert!(plugin.resources.iter().any(|resource| {
            resource.kind == PluginResourceKind::Skill
                && resource.status == PluginResourceStatus::Loaded
                && resource.path.ends_with("skills/global-scout/SKILL.md")
        }));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn skill_discovery_loads_explicit_oppi_plugin_sources() {
        let _env_lock = TEST_ENV_LOCK.lock().unwrap();
        let root =
            std::env::temp_dir().join(format!("oppi-env-plugin-skills-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let oppi_home = root.join("empty-oppi-home");
        let workspace = root.join("workspace");
        let plugin = root.join("env-plugin");
        fs::create_dir_all(plugin.join("skills/env-scout")).unwrap();
        fs::create_dir_all(&workspace).unwrap();
        fs::create_dir_all(&oppi_home).unwrap();
        let _oppi_home = EnvVarGuard::set("OPPI_HOME", &oppi_home);
        let _plugin_sources = EnvVarGuard::set("OPPI_PLUGIN_SOURCES", &plugin);
        fs::write(
            plugin.join("package.json"),
            r#"{"name":"env-plugin","version":"1.0.0","pi":{"skills":["./skills"]}}"#,
        )
        .unwrap();
        fs::write(
            plugin.join("skills/env-scout/SKILL.md"),
            "---\nname: env-scout\ndescription: Inspect explicit OPPI_PLUGIN_SOURCES plugin resources.\n---\n\n# Env Scout\n",
        )
        .unwrap();

        let mut runtime = Runtime::new();
        let thread = runtime
            .start_thread(ThreadStartParams {
                project: ProjectRef {
                    id: "env-plugin-skills".to_string(),
                    cwd: workspace.display().to_string(),
                    display_name: None,
                    workspace_roots: Vec::new(),
                },
                title: None,
            })
            .thread;
        let skills = runtime
            .list_skills(SkillListParams {
                thread_id: Some(thread.id.clone()),
            })
            .unwrap()
            .items;
        let plugin_skill = skills
            .iter()
            .find(|skill| skill.active.name == "env-scout")
            .expect("OPPI_PLUGIN_SOURCES plugin skill should be discovered");
        assert_eq!(plugin_skill.active.source, SkillSource::Plugin);
        let plugins = runtime
            .list_plugins(PluginListParams {
                thread_id: Some(thread.id),
            })
            .unwrap()
            .items;
        let plugin = plugins
            .iter()
            .find(|plugin| plugin.id == "env-plugin")
            .expect("explicit plugin source should be listed");
        assert_eq!(plugin.scope, Some(PluginScope::Env));
        assert_eq!(plugin.trusted, Some(true));
        assert!(plugin.resources.iter().any(|resource| {
            resource.kind == PluginResourceKind::Skill
                && resource.status == PluginResourceStatus::Loaded
                && resource.path.ends_with("skills/env-scout/SKILL.md")
        }));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn plugin_skills_filter_disabled_untrusted_and_non_local_store_entries() {
        let root = std::env::temp_dir().join(format!(
            "oppi-plugin-skill-boundaries-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".oppi")).unwrap();
        let cases = [
            ("disabled-scout", "local", false, true),
            ("untrusted-scout", "local", true, false),
            ("npm-scout", "npm", true, true),
        ];
        for (name, _, _, _) in cases {
            let plugin = root.join(name);
            fs::create_dir_all(plugin.join("skills").join(name)).unwrap();
            fs::write(
                plugin.join("package.json"),
                r#"{"name":"boundary-plugin","version":"1.0.0","pi":{"skills":["./skills"]}}"#,
            )
            .unwrap();
            fs::write(
                plugin.join("skills").join(name).join("SKILL.md"),
                format!(
                    "---\nname: {name}\ndescription: Boundary test skill for native plugins.\n---\n\n# {name}\n"
                ),
            )
            .unwrap();
        }
        let entries = cases
            .iter()
            .map(|(name, source_type, enabled, trusted)| {
                let source = serde_json::to_string(&root.join(name).display().to_string()).unwrap();
                format!(
                    r#"{{
      "name": "{name}",
      "source": {source},
      "sourceType": "{source_type}",
      "enabled": {enabled},
      "trusted": {trusted},
      "addedAt": "2026-05-26T00:00:00.000Z",
      "updatedAt": "2026-05-26T00:00:00.000Z"
    }}"#
                )
            })
            .collect::<Vec<_>>()
            .join(",\n");
        fs::write(
            root.join(".oppi/plugins.json"),
            format!("{{\n  \"version\": 1,\n  \"plugins\": [\n{entries}\n  ]\n}}\n"),
        )
        .unwrap();

        let mut runtime = Runtime::new();
        let thread = runtime
            .start_thread(ThreadStartParams {
                project: ProjectRef {
                    id: "plugin-boundaries".to_string(),
                    cwd: root.display().to_string(),
                    display_name: None,
                    workspace_roots: Vec::new(),
                },
                title: None,
            })
            .thread;
        let skills = runtime
            .list_skills(SkillListParams {
                thread_id: Some(thread.id),
            })
            .unwrap()
            .items;
        for name in ["disabled-scout", "untrusted-scout", "npm-scout"] {
            assert!(
                skills.iter().all(|skill| skill.active.name != name),
                "{name} must not be loaded as a native plugin skill"
            );
        }

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn plugin_skills_ignore_extension_only_plugin_resources() {
        let root =
            std::env::temp_dir().join(format!("oppi-plugin-extension-only-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let plugin = root.join("extension-only");
        fs::create_dir_all(plugin.join("extensions")).unwrap();
        fs::write(
            plugin.join("package.json"),
            r#"{"name":"extension-only","version":"1.0.0","pi":{"extensions":["./extensions/index.ts"]}}"#,
        )
        .unwrap();
        fs::write(
            plugin.join("extensions/index.ts"),
            "export default function extensionOnly() { throw new Error('should not run'); }\n",
        )
        .unwrap();
        fs::create_dir_all(root.join(".oppi")).unwrap();
        fs::write(
            root.join(".oppi/plugins.json"),
            format!(
                r#"{{
  "version": 1,
  "plugins": [
    {{
      "name": "extension-only",
      "source": {},
      "sourceType": "local",
      "enabled": true,
      "trusted": true,
      "addedAt": "2026-05-26T00:00:00.000Z",
      "updatedAt": "2026-05-26T00:00:00.000Z"
    }}
  ]
}}"#,
                serde_json::to_string(&plugin.display().to_string()).unwrap()
            ),
        )
        .unwrap();

        let mut runtime = Runtime::new();
        let thread = runtime
            .start_thread(ThreadStartParams {
                project: ProjectRef {
                    id: "plugin-extension-only".to_string(),
                    cwd: root.display().to_string(),
                    display_name: None,
                    workspace_roots: Vec::new(),
                },
                title: None,
            })
            .thread;
        let skills = runtime
            .list_skills(SkillListParams {
                thread_id: Some(thread.id),
            })
            .unwrap()
            .items;
        assert!(
            skills
                .iter()
                .all(|skill| skill.active.name != "extension-only"),
            "extension-only plugins must not execute or register native skill resources"
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn plugin_list_reports_local_plugin_as_static_materialization_not_required() {
        let _env_lock = TEST_ENV_LOCK.lock().unwrap();
        let root = temp_project_root("plugin-local-materialization");
        let oppi_home = root.join("oppi-home");
        let plugin = root.join("plugin-pack");
        fs::create_dir_all(plugin.join("skills/scout")).unwrap();
        fs::create_dir_all(&oppi_home).unwrap();
        let _oppi_home = EnvVarGuard::set("OPPI_HOME", &oppi_home);
        let _plugin_sources = EnvVarGuard::remove("OPPI_PLUGIN_SOURCES");
        fs::write(
            plugin.join("package.json"),
            r#"{"name":"plugin-pack","version":"1.0.0","pi":{"skills":["./skills"]}}"#,
        )
        .unwrap();
        write_project_plugin_store(
            &root,
            json!({
                "version": 1,
                "plugins": [{
                    "name": "plugin-pack",
                    "source": plugin.display().to_string(),
                    "sourceType": "local",
                    "enabled": true,
                    "trusted": true
                }]
            }),
        );

        let mut runtime = Runtime::new();
        let thread = thread_with_cwd(&mut runtime, &root);
        let listed = runtime
            .list_plugins(PluginListParams {
                thread_id: Some(thread.id),
            })
            .unwrap()
            .items;
        let plugin = listed.iter().find(|item| item.id == "plugin-pack").unwrap();
        let materialization = plugin.materialization.as_ref().unwrap();
        assert_eq!(
            materialization.status,
            PluginMaterializationStatus::NotRequired
        );
        assert_eq!(materialization.source_type, PluginSourceType::Local);
        assert!(materialization.cache_path.is_none());

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn plugin_list_reports_marketplace_plugin_as_not_materialized() {
        let _env_lock = TEST_ENV_LOCK.lock().unwrap();
        let root = temp_project_root("plugin-market-materialization");
        let oppi_home = root.join("oppi-home");
        fs::create_dir_all(&oppi_home).unwrap();
        let _oppi_home = EnvVarGuard::set("OPPI_HOME", &oppi_home);
        let _plugin_sources = EnvVarGuard::remove("OPPI_PLUGIN_SOURCES");
        write_project_plugin_store(
            &root,
            json!({
                "version": 1,
                "plugins": [{
                    "name": "market-pack",
                    "source": "npm:@demo/market-pack",
                    "sourceType": "marketplace",
                    "enabled": true,
                    "trusted": true,
                    "marketplace": "demo"
                }]
            }),
        );

        let mut runtime = Runtime::new();
        let thread = thread_with_cwd(&mut runtime, &root);
        let plugin = runtime
            .list_plugins(PluginListParams {
                thread_id: Some(thread.id),
            })
            .unwrap()
            .items
            .into_iter()
            .find(|item| item.id == "market-pack")
            .unwrap();
        let materialization = plugin.materialization.unwrap();
        assert_eq!(
            materialization.status,
            PluginMaterializationStatus::NotMaterialized
        );
        assert_eq!(materialization.source_type, PluginSourceType::Marketplace);
        assert!(materialization.diagnostics.iter().any(|diag| {
            diag.code == "plugin_materialization_required" && diag.message.contains("native cache")
        }));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn materialized_marketplace_plugin_static_resources_load_from_cache() {
        let _env_lock = TEST_ENV_LOCK.lock().unwrap();
        let root = temp_project_root("plugin-materialized-static-resources");
        let oppi_home = root.join("oppi-home");
        let cache_root = oppi_home.join("plugins/cache/market-pack-abc123");
        fs::create_dir_all(cache_root.join("skills/scout")).unwrap();
        fs::create_dir_all(oppi_home.join("plugins")).unwrap();
        let _oppi_home = EnvVarGuard::set("OPPI_HOME", &oppi_home);
        let _plugin_sources = EnvVarGuard::remove("OPPI_PLUGIN_SOURCES");
        fs::write(
            cache_root.join("package.json"),
            r#"{"name":"market-pack","version":"1.0.0","pi":{"skills":["./skills"]}}"#,
        )
        .unwrap();
        fs::write(
            cache_root.join("skills/scout/SKILL.md"),
            "---\nname: market-scout\ndescription: Inspect materialized marketplace plugin resources.\n---\n\n# Market Scout\n",
        )
        .unwrap();
        write_project_plugin_store(
            &root,
            json!({
                "version": 1,
                "plugins": [{
                    "name": "market-pack",
                    "source": "npm:@demo/market-pack",
                    "sourceType": "marketplace",
                    "enabled": true,
                    "trusted": true,
                    "marketplace": "demo"
                }]
            }),
        );
        fs::write(
            oppi_home.join("plugins/materialized.json"),
            serde_json::to_string_pretty(&json!({
                "version": 1,
                "records": [{
                    "pluginId": "market-pack",
                    "source": "npm:@demo/market-pack",
                    "sourceType": "marketplace",
                    "scope": "project",
                    "cachePath": cache_root.display().to_string(),
                    "checksum": "sha256:abc123",
                    "manifestName": "market-pack",
                    "manifestVersion": "1.0.0",
                    "provenance": {
                        "marketplace": "demo",
                        "resolvedSource": "npm:@demo/market-pack",
                        "resolvedVersion": "1.0.0"
                    },
                    "materializedAt": "2026-05-26T12:00:00Z"
                }]
            }))
            .unwrap(),
        )
        .unwrap();

        let mut runtime = Runtime::new();
        let thread = thread_with_cwd(&mut runtime, &root);
        let plugins = runtime
            .list_plugins(PluginListParams {
                thread_id: Some(thread.id.clone()),
            })
            .unwrap()
            .items;
        let plugin = plugins
            .iter()
            .find(|item| item.id == "market-pack")
            .unwrap();
        let materialization = plugin.materialization.as_ref().unwrap();
        assert_eq!(
            materialization.status,
            PluginMaterializationStatus::Materialized
        );
        assert_eq!(
            materialization.cache_path.as_deref(),
            Some(&*cache_root.display().to_string())
        );
        assert!(plugin.resources.iter().any(|resource| {
            resource.kind == PluginResourceKind::Skill
                && resource.status == PluginResourceStatus::Loaded
                && resource.path.ends_with("skills/scout/SKILL.md")
        }));

        let skills = runtime
            .list_skills(SkillListParams {
                thread_id: Some(thread.id),
            })
            .unwrap()
            .items;
        assert!(skills.iter().any(|skill| {
            skill.active.name == "market-scout" && skill.active.source == SkillSource::Plugin
        }));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn plugin_list_reports_configured_resources_and_diagnostics() {
        let root = std::env::temp_dir().join(format!(
            "oppi-plugin-list-diagnostics-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        let plugin = root.join("plugin-pack");
        fs::create_dir_all(plugin.join("skills/plugin-scout")).unwrap();
        fs::create_dir_all(plugin.join("extensions")).unwrap();
        fs::write(
            plugin.join("package.json"),
            r#"{"name":"plugin-pack","version":"1.0.0","description":"Plugin pack","pi":{"skills":["./skills"],"extensions":["./extensions/index.ts"]}}"#,
        )
        .unwrap();
        fs::write(
            plugin.join("skills/plugin-scout/SKILL.md"),
            "---\nname: plugin-scout\ndescription: Inspect enabled plugin resources for native OPPi.\n---\n\n# Plugin Scout\n",
        )
        .unwrap();
        fs::write(
            plugin.join("extensions/index.ts"),
            "export default function plugin() {}\n",
        )
        .unwrap();
        fs::create_dir_all(root.join(".oppi")).unwrap();
        fs::write(
            root.join(".oppi/plugins.json"),
            format!(
                r#"{{
  "version": 1,
  "plugins": [
    {{
      "name": "plugin-pack",
      "source": {},
      "sourceType": "local",
      "enabled": true,
      "trusted": true,
      "description": "Stored description",
      "addedAt": "2026-05-26T00:00:00.000Z",
      "updatedAt": "2026-05-26T00:00:00.000Z"
    }}
  ]
}}"#,
                serde_json::to_string(&plugin.display().to_string()).unwrap()
            ),
        )
        .unwrap();

        let mut runtime = Runtime::new();
        let thread = runtime
            .start_thread(ThreadStartParams {
                project: ProjectRef {
                    id: "plugin-list".to_string(),
                    cwd: root.display().to_string(),
                    display_name: None,
                    workspace_roots: Vec::new(),
                },
                title: None,
            })
            .thread;
        let plugins = runtime
            .list_plugins(PluginListParams {
                thread_id: Some(thread.id),
            })
            .unwrap()
            .items;
        let plugin = plugins
            .iter()
            .find(|plugin| plugin.id == "plugin-pack")
            .expect("configured plugin should be listed");
        assert_eq!(plugin.name.as_deref(), Some("plugin-pack"));
        assert_eq!(plugin.version.as_deref(), Some("1.0.0"));
        assert_eq!(plugin.trusted, Some(true));
        assert_eq!(plugin.scope, Some(PluginScope::Project));
        assert!(plugin.resources.iter().any(|resource| {
            resource.kind == PluginResourceKind::Skill
                && resource.status == PluginResourceStatus::Loaded
                && resource.path.ends_with("skills/plugin-scout/SKILL.md")
        }));
        assert!(plugin.resources.iter().any(|resource| {
            resource.kind == PluginResourceKind::Extension
                && resource.status == PluginResourceStatus::Unsupported
                && resource.path.ends_with("extensions/index.ts")
        }));
        assert!(plugin.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "pi_extension_unsupported"
                && diagnostic.message.contains("not executed by Rust native")
        }));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn plugin_list_reports_non_local_marketplace_and_risk_diagnostics() {
        let root = std::env::temp_dir().join(format!(
            "oppi-plugin-risk-diagnostics-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".oppi")).unwrap();
        fs::write(
            root.join(".oppi/plugins.json"),
            r#"{
  "version": 1,
  "plugins": [
    {
      "name": "market-plugin",
      "source": "marketplace:danger-plugin",
      "sourceType": "marketplace",
      "enabled": true,
      "trusted": true,
      "marketplace": "demo-market",
      "capabilities": ["shell", "network", "native"],
      "manifest": {
        "name": "market-plugin",
        "version": "1.2.3",
        "capabilities": ["exec"]
      }
    }
  ]
}"#,
        )
        .unwrap();

        let mut runtime = Runtime::new();
        let thread = runtime
            .start_thread(ThreadStartParams {
                project: ProjectRef {
                    id: "plugin-risk".to_string(),
                    cwd: root.display().to_string(),
                    display_name: None,
                    workspace_roots: Vec::new(),
                },
                title: None,
            })
            .thread;
        let plugins = runtime
            .list_plugins(PluginListParams {
                thread_id: Some(thread.id),
            })
            .unwrap()
            .items;
        let plugin = plugins
            .iter()
            .find(|plugin| plugin.id == "market-plugin")
            .expect("configured marketplace plugin should be listed");
        for code in [
            "non_local_plugin_unsupported",
            "marketplace_plugin_not_materialized",
            "plugin_risk_shell_exec",
            "plugin_risk_network",
            "plugin_risk_native",
        ] {
            assert!(
                plugin
                    .diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.code == code),
                "missing diagnostic {code}: {:?}",
                plugin.diagnostics
            );
        }

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn plugin_list_reports_claude_shaped_local_plugin_handoff_diagnostics() {
        let root = std::env::temp_dir().join(format!(
            "oppi-claude-plugin-diagnostics-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        let plugin = root.join("claude-plugin");
        fs::create_dir_all(&plugin).unwrap();
        fs::create_dir_all(root.join(".oppi")).unwrap();
        fs::write(
            plugin.join("package.json"),
            r#"{
  "name": "claude-mcp-demo",
  "version": "1.0.0",
  "mcpServers": { "demo": { "command": "node", "args": ["server.js"] } }
}"#,
        )
        .unwrap();
        fs::write(
            root.join(".oppi/plugins.json"),
            format!(
                r#"{{
  "version": 1,
  "plugins": [
    {{
      "name": "claude-mcp-demo",
      "source": {},
      "sourceType": "local",
      "enabled": true,
      "trusted": true
    }}
  ]
}}"#,
                serde_json::to_string(&plugin.display().to_string()).unwrap()
            ),
        )
        .unwrap();

        let mut runtime = Runtime::new();
        let thread = runtime
            .start_thread(ThreadStartParams {
                project: ProjectRef {
                    id: "claude-plugin".to_string(),
                    cwd: root.display().to_string(),
                    display_name: None,
                    workspace_roots: Vec::new(),
                },
                title: None,
            })
            .thread;
        let plugins = runtime
            .list_plugins(PluginListParams {
                thread_id: Some(thread.id),
            })
            .unwrap()
            .items;
        let plugin = plugins
            .iter()
            .find(|plugin| plugin.id == "claude-mcp-demo")
            .expect("Claude-shaped local plugin should still be listed");
        let diagnostic = plugin
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code == "claude_plugin_requires_port")
            .expect("Claude-shaped plugin should report a porting handoff diagnostic");
        assert!(diagnostic.message.contains("mcpServers"));
        assert!(diagnostic.message.contains("Port"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn plugin_list_reports_local_package_install_and_native_build_risks() {
        let root = std::env::temp_dir().join(format!(
            "oppi-local-plugin-risk-diagnostics-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        let plugin = root.join("risky-plugin");
        fs::create_dir_all(&plugin).unwrap();
        fs::create_dir_all(root.join(".oppi")).unwrap();
        fs::write(
            plugin.join("package.json"),
            r#"{
  "name": "risky-plugin",
  "version": "1.0.0",
  "scripts": {
    "postinstall": "node install.js"
  },
  "gypfile": true
}"#,
        )
        .unwrap();
        fs::write(
            root.join(".oppi/plugins.json"),
            format!(
                r#"{{
  "version": 1,
  "plugins": [
    {{
      "name": "risky-plugin",
      "source": {},
      "sourceType": "local",
      "enabled": true,
      "trusted": true
    }}
  ]
}}"#,
                serde_json::to_string(&plugin.display().to_string()).unwrap()
            ),
        )
        .unwrap();

        let mut runtime = Runtime::new();
        let thread = runtime
            .start_thread(ThreadStartParams {
                project: ProjectRef {
                    id: "risky-plugin".to_string(),
                    cwd: root.display().to_string(),
                    display_name: None,
                    workspace_roots: Vec::new(),
                },
                title: None,
            })
            .thread;
        let plugins = runtime
            .list_plugins(PluginListParams {
                thread_id: Some(thread.id),
            })
            .unwrap()
            .items;
        let plugin = plugins
            .iter()
            .find(|plugin| plugin.id == "risky-plugin")
            .expect("local plugin should be listed");
        assert!(plugin.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "plugin_risk_install_script"
                && diagnostic.message.contains("postinstall")
        }));
        assert!(plugin.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "plugin_risk_native_build" && diagnostic.message.contains("gypfile")
        }));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn direct_provider_turn_injects_relevant_plugin_skill_prompt() {
        let root =
            std::env::temp_dir().join(format!("oppi-plugin-skill-inject-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let plugin = root.join("plugin-pack");
        fs::create_dir_all(plugin.join("skills/plugin-scout")).unwrap();
        fs::write(
            plugin.join("package.json"),
            r#"{"name":"plugin-pack","version":"1.0.0","pi":{"skills":["./skills"]}}"#,
        )
        .unwrap();
        fs::write(
            plugin.join("skills/plugin-scout/SKILL.md"),
            "---\nname: plugin-scout\ndescription: Inspect enabled plugin resources for native OPPi.\n---\n\n# Plugin Scout\n",
        )
        .unwrap();
        fs::create_dir_all(root.join(".oppi")).unwrap();
        fs::write(
            root.join(".oppi/plugins.json"),
            format!(
                r#"{{
  "version": 1,
  "plugins": [
    {{
      "name": "plugin-pack",
      "source": {},
      "sourceType": "local",
      "enabled": true,
      "trusted": true,
      "addedAt": "2026-05-26T00:00:00.000Z",
      "updatedAt": "2026-05-26T00:00:00.000Z"
    }}
  ]
}}"#,
                serde_json::to_string(&plugin.display().to_string()).unwrap()
            ),
        )
        .unwrap();
        let response = json!({
            "choices": [{ "message": { "role": "assistant", "content": "plugin skill noted" } }]
        })
        .to_string();
        let (base_url, server) = start_mock_openai_server(vec![response]);
        let key_name = format!("OPPI_PLUGIN_SKILL_TEST_{}_API_KEY", std::process::id());
        unsafe { std::env::set_var(&key_name, "test-key") };
        let mut runtime = Runtime::new();
        let thread = runtime
            .start_thread(ThreadStartParams {
                project: ProjectRef {
                    id: "plugin-skill-inject".to_string(),
                    cwd: root.display().to_string(),
                    display_name: None,
                    workspace_roots: Vec::new(),
                },
                title: None,
            })
            .thread;
        let result = runtime
            .run_agentic_turn(AgenticTurnParams {
                thread_id: thread.id,
                input: "Inspect enabled plugin resources for native OPPi".to_string(),
                execution_mode: ExecutionMode::Blocking,
                follow_up: None,
                sandbox_policy: None,
                model_steps: Vec::new(),
                model_provider: Some(DirectModelProviderConfig {
                    kind: DirectModelProviderKind::OpenAiCompatible,
                    model: "mock".to_string(),
                    base_url: Some(base_url),
                    api_key_env: Some(key_name.clone()),
                    system_prompt: Some("base system".to_string()),
                    temperature: None,
                    reasoning_effort: None,
                    max_output_tokens: None,
                    stream: false,
                }),
                tool_definitions: Vec::new(),
                approved_tool_call_ids: Vec::new(),
                cancellation: None,
                max_continuations: Some(1),
            })
            .unwrap();
        unsafe { std::env::remove_var(&key_name) };
        let requests = server.join().unwrap();
        assert_eq!(result.turn.status, TurnStatus::Completed);
        assert!(result.events.iter().any(|event| matches!(
            &event.kind,
            EventKind::Diagnostic { diagnostic }
                if diagnostic.message == "skill instructions injected"
                    && diagnostic.metadata.get("skills").is_some_and(|value| value.contains("plugin-scout"))
        )));
        assert!(requests[0].contains("base system"));
        assert!(requests[0].contains("Skill: plugin-scout"));
        assert!(requests[0].contains("# Plugin Scout"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn skill_injection_selects_relevant_builtin_instructions() {
        let root = std::env::temp_dir().join(format!("oppi-skill-inject-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let mut runtime = Runtime::new();
        let thread = runtime
            .start_thread(ThreadStartParams {
                project: ProjectRef {
                    id: "skill-inject".to_string(),
                    cwd: root.display().to_string(),
                    display_name: None,
                    workspace_roots: Vec::new(),
                },
                title: None,
            })
            .thread;
        let (prompt, refs) = runtime
            .skill_injection_prompt(&thread.id, "Generate an image of a cheerful robot", &[])
            .unwrap()
            .unwrap();
        assert!(refs.iter().any(|skill| skill.name == "imagegen"));
        assert!(prompt.contains("Skill: imagegen"));
        assert!(prompt.contains("# Image Generation"));
        assert!(!refs.iter().any(|skill| skill.name == "mermaid-diagrams"));
        let graphify = runtime
            .skill_injection_prompt(
                &thread.id,
                "Map the repo-wide architecture and dependency blast radius",
                &[],
            )
            .unwrap()
            .unwrap();
        assert!(graphify.1.iter().any(|skill| skill.name == "graphify"));
        assert!(graphify.0.contains("Skill: graphify"));
        let explicit = runtime
            .skill_injection_prompt(
                &thread.id,
                "Use the requested skill only",
                &["mermaid-diagrams".to_string()],
            )
            .unwrap()
            .unwrap();
        assert!(
            explicit
                .1
                .iter()
                .any(|skill| skill.name == "mermaid-diagrams")
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn skill_command_expands_active_skill_with_arguments() {
        let root =
            std::env::temp_dir().join(format!("oppi-skill-command-expand-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".pi/skills/test-skill")).unwrap();
        fs::write(
            root.join(".pi/skills/test-skill/SKILL.md"),
            "---\nname: test-skill\ndescription: Test explicit skill command expansion.\n---\n\n# Test Skill\n\nUse the explicit skill body.\n",
        )
        .unwrap();
        let mut runtime = Runtime::new();
        let thread = runtime
            .start_thread(ThreadStartParams {
                project: ProjectRef {
                    id: "skill-command-expand".to_string(),
                    cwd: root.display().to_string(),
                    display_name: None,
                    workspace_roots: Vec::new(),
                },
                title: None,
            })
            .thread;

        let result = runtime
            .expand_skill_command(SkillExpandParams {
                thread_id: thread.id,
                input: "/skill:test-skill explain this".to_string(),
            })
            .unwrap();

        assert_eq!(
            result.skill.as_ref().map(|skill| skill.name.as_str()),
            Some("test-skill")
        );
        assert!(
            result
                .input
                .contains("<skill name=\"test-skill\" location=\"")
        );
        assert!(result.input.contains("References are relative to"));
        assert!(result.input.contains("# Test Skill"));
        assert!(result.input.contains("Use the explicit skill body."));
        assert!(result.input.ends_with("explain this"));
        assert!(!result.input.contains("description: Test explicit"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn direct_provider_turn_injects_relevant_skill_prompt() {
        let response = json!({
            "choices": [{ "message": { "role": "assistant", "content": "image plan ready" } }]
        })
        .to_string();
        let (base_url, server) = start_mock_openai_server(vec![response]);
        let key_name = format!("OPPI_SKILL_INJECT_TEST_{}_API_KEY", std::process::id());
        unsafe { std::env::set_var(&key_name, "test-key") };
        let mut runtime = Runtime::new();
        let thread = thread(&mut runtime);
        let result = runtime
            .run_agentic_turn(AgenticTurnParams {
                thread_id: thread.id,
                input: "Generate an image of a cheerful robot".to_string(),
                execution_mode: ExecutionMode::Blocking,
                follow_up: None,
                sandbox_policy: None,
                model_steps: Vec::new(),
                model_provider: Some(DirectModelProviderConfig {
                    kind: DirectModelProviderKind::OpenAiCompatible,
                    model: "mock".to_string(),
                    base_url: Some(base_url),
                    api_key_env: Some(key_name.clone()),
                    system_prompt: Some("base system".to_string()),
                    temperature: None,
                    reasoning_effort: None,
                    max_output_tokens: None,
                    stream: false,
                }),
                tool_definitions: Vec::new(),
                approved_tool_call_ids: Vec::new(),
                cancellation: None,
                max_continuations: Some(1),
            })
            .unwrap();
        unsafe { std::env::remove_var(&key_name) };
        let requests = server.join().unwrap();
        assert_eq!(result.turn.status, TurnStatus::Completed);
        assert!(result.events.iter().any(|event| matches!(
            &event.kind,
            EventKind::Diagnostic { diagnostic }
                if diagnostic.message == "skill instructions injected"
                    && diagnostic.metadata.get("skills").is_some_and(|value| value.contains("imagegen"))
        )));
        assert!(requests[0].contains("base system"));
        assert!(requests[0].contains("Skill: imagegen"));
        assert!(!requests[0].contains("Skill: mermaid-diagrams"));
    }

    #[test]
    fn subagent_model_aliases_resolve_to_provider_defaults() {
        let provider = DirectModelProviderConfig {
            kind: DirectModelProviderKind::OpenAiCodex,
            model: GPT_MAIN_DEFAULT_MODEL.to_string(),
            base_url: None,
            api_key_env: None,
            system_prompt: None,
            temperature: None,
            reasoning_effort: Some("xhigh".to_string()),
            max_output_tokens: None,
            stream: true,
        };
        let mut policy = ResolvedAgentToolPolicy {
            background: false,
            role: Some("subagent".to_string()),
            model: Some("coding".to_string()),
            effort: None,
            permission_mode: None,
            network_policy: None,
            memory_mode: None,
            worktree_root: None,
            tool_allowlist: Vec::new(),
            tool_denylist: Vec::new(),
            isolation: None,
            color: None,
            skills: Vec::new(),
            max_turns: None,
        };
        resolve_subagent_model_policy(&mut policy, Some(&provider), "edit one module");
        assert_eq!(
            policy.model.as_deref(),
            Some(GPT_CODING_SUBAGENT_DEFAULT_MODEL)
        );
        assert_eq!(policy.effort.as_deref(), Some("high"));

        policy.model = Some("strong".to_string());
        policy.effort = None;
        resolve_subagent_model_policy(&mut policy, Some(&provider), "multi-file refactor");
        assert_eq!(policy.model.as_deref(), Some(GPT_MAIN_DEFAULT_MODEL));
        assert_eq!(policy.effort.as_deref(), Some("xhigh"));

        let claude = DirectModelProviderConfig {
            kind: DirectModelProviderKind::OpenAiCompatible,
            model: CLAUDE_MAIN_DEFAULT_MODEL.to_string(),
            base_url: Some("http://127.0.0.1:3456".to_string()),
            api_key_env: Some(MERIDIAN_API_KEY_ENV.to_string()),
            system_prompt: None,
            temperature: None,
            reasoning_effort: Some("high".to_string()),
            max_output_tokens: None,
            stream: true,
        };
        policy.model = Some("coding".to_string());
        policy.effort = None;
        resolve_subagent_model_policy(&mut policy, Some(&claude), "edit one module");
        assert_eq!(
            policy.model.as_deref(),
            Some(CLAUDE_CODING_SUBAGENT_DEFAULT_MODEL)
        );
        assert_eq!(policy.effort.as_deref(), Some("high"));
    }

    #[test]
    fn native_agent_tool_runs_nested_subagent_and_streams_lifecycle() {
        let mut runtime = Runtime::new();
        let thread = thread(&mut runtime);
        let result = runtime
            .run_agentic_turn(AgenticTurnParams {
                thread_id: thread.id.clone(),
                input: "delegate this".to_string(),
                execution_mode: ExecutionMode::Blocking,
                follow_up: None,
                sandbox_policy: Some(default_policy(PermissionProfile {
                    mode: PermissionMode::Default,
                    readable_roots: vec!["/repo".to_string()],
                    writable_roots: vec!["/repo".to_string()],
                    filesystem_rules: Vec::new(),
                    protected_patterns: Vec::new(),
                })),
                model_steps: vec![ScriptedModelStep {
                    assistant_deltas: vec!["I will delegate. ".to_string()],
                    tool_calls: vec![agent_tool_call(
                        "agent-1",
                        json!({
                            "agentName": "general-purpose",
                            "task": "summarize nested work",
                            "modelSteps": [
                                { "assistantDeltas": ["nested done"], "finalResponse": true }
                            ],
                            "role": "subagent",
                            "model": "gpt-sub",
                            "effort": "medium",
                            "memoryMode": "disabled",
                            "toolDenylist": ["shell_exec"],
                            "isolation": "thread",
                            "color": "cyan",
                            "skills": ["independent"],
                            "maxTurns": 2
                        }),
                    )],
                    tool_results: Vec::new(),
                    final_response: true,
                }],
                model_provider: None,
                tool_definitions: Vec::new(),
                approved_tool_call_ids: Vec::new(),
                cancellation: None,
                max_continuations: Some(3),
            })
            .unwrap();

        assert_eq!(result.turn.status, TurnStatus::Completed);
        assert!(result.events.iter().any(|event| matches!(
            &event.kind,
            EventKind::AgentStarted { run }
                if run.agent_name == "general-purpose"
                    && run.role.as_deref() == Some("subagent")
                    && run.model.as_deref() == Some("gpt-sub")
                    && run.memory_mode.as_deref() == Some("disabled")
                    && run.tool_denylist == vec!["shell_exec".to_string()]
                    && run.skills == vec!["independent".to_string()]
                    && run.max_turns == Some(2)
        )));
        assert!(result.events.iter().any(|event| matches!(
            &event.kind,
            EventKind::TurnStarted { turn }
                if turn.parent_turn_id.as_deref() == Some(result.turn.id.as_str())
        )));
        assert!(result.events.iter().any(|event| matches!(
            &event.kind,
            EventKind::AgentCompleted { output, .. } if output.contains("nested done")
        )));
        assert!(result.events.iter().any(|event| matches!(
            &event.kind,
            EventKind::ToolCallCompleted {
                result: ToolResult {
                    status: ToolResultStatus::Ok,
                    output: Some(output),
                    ..
                }
            } if output.contains("\"status\":\"completed\"")
                && output.contains("nested done")
        )));
    }

    #[test]
    fn native_agent_tool_enforces_allowlist_and_permission_floor() {
        let root = std::env::temp_dir().join(format!("oppi-agent-tool-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let mut runtime = Runtime::new();
        let thread = runtime
            .start_thread(ThreadStartParams {
                project: ProjectRef {
                    id: "agent-tool".to_string(),
                    cwd: root.display().to_string(),
                    display_name: None,
                    workspace_roots: Vec::new(),
                },
                title: None,
            })
            .thread;
        runtime
            .register_agent(
                &thread.id,
                AgentDefinition {
                    name: "readonly-worker".to_string(),
                    description: "read only".to_string(),
                    source: Some(AgentSource::Project),
                    tools: vec!["read_file".to_string()],
                    tool_denylist: Vec::new(),
                    model: None,
                    effort: None,
                    permission_mode: Some(PermissionMode::ReadOnly),
                    memory_mode: None,
                    background: false,
                    worktree_root: None,
                    isolation: None,
                    color: None,
                    skills: Vec::new(),
                    max_turns: None,
                    path: None,
                    filename: None,
                    read_only: false,
                    instructions: "Read only.".to_string(),
                },
            )
            .unwrap();
        let result = runtime
            .run_agentic_turn(AgenticTurnParams {
                thread_id: thread.id.clone(),
                input: "delegate write".to_string(),
                execution_mode: ExecutionMode::Blocking,
                follow_up: None,
                sandbox_policy: Some(default_policy(PermissionProfile {
                    mode: PermissionMode::FullAccess,
                    readable_roots: vec![root.display().to_string()],
                    writable_roots: vec![root.display().to_string()],
                    filesystem_rules: Vec::new(),
                    protected_patterns: Vec::new(),
                })),
                model_steps: vec![ScriptedModelStep {
                    assistant_deltas: Vec::new(),
                    tool_calls: vec![agent_tool_call(
                        "agent-1",
                        json!({
                            "agentName": "readonly-worker",
                            "task": "try to write",
                            "permissionMode": "full-access",
                            "modelSteps": [
                                {
                                    "toolCalls": [
                                        {
                                            "id": "write-1",
                                            "name": "write_file",
                                            "namespace": "oppi",
                                            "arguments": { "path": "subagent.txt", "content": "nope" }
                                        }
                                    ],
                                    "finalResponse": true
                                }
                            ]
                        }),
                    )],
                    tool_results: Vec::new(),
                    final_response: true,
                }],
                model_provider: None,
                tool_definitions: Vec::new(),
                approved_tool_call_ids: Vec::new(),
                cancellation: None,
                max_continuations: Some(3),
            })
            .unwrap();

        assert!(result.events.iter().any(|event| matches!(
            &event.kind,
            EventKind::Diagnostic { diagnostic }
                if diagnostic.message == "native subagent execution policy applied"
                    && diagnostic.metadata.get("permissionMode") == Some(&"read-only".to_string())
        )));
        assert!(result.events.iter().any(|event| matches!(
            &event.kind,
            EventKind::ToolCallCompleted {
                result: ToolResult {
                    call_id,
                    status: ToolResultStatus::Error,
                    error: Some(error),
                    ..
                }
            } if call_id == "write-1" && error.contains("tool not registered")
        )));
        assert!(!root.join("subagent.txt").exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn agent_tool_uses_worktree_root_as_child_execution_root() {
        let root = temp_project_root("agent-tool-worktree-root");
        let parent = root.join("parent");
        let child = parent.join("worktrees").join("reviewer");
        fs::create_dir_all(&child).unwrap();

        let mut runtime = Runtime::new();
        let thread = thread_with_cwd(&mut runtime, &parent);
        let result = runtime
            .run_agentic_turn(AgenticTurnParams {
                thread_id: thread.id.clone(),
                input: "delegate child-root write".to_string(),
                execution_mode: ExecutionMode::Blocking,
                follow_up: None,
                sandbox_policy: Some(default_policy(PermissionProfile {
                    mode: PermissionMode::Default,
                    readable_roots: vec![parent.display().to_string()],
                    writable_roots: vec![parent.display().to_string()],
                    filesystem_rules: Vec::new(),
                    protected_patterns: Vec::new(),
                })),
                model_steps: vec![ScriptedModelStep {
                    assistant_deltas: Vec::new(),
                    tool_calls: vec![agent_tool_call(
                        "agent-1",
                        json!({
                            "agentName": "general-purpose",
                            "task": "write inside child root",
                            "worktreeRoot": child.display().to_string(),
                            "modelSteps": [
                                {
                                    "toolCalls": [
                                        {
                                            "id": "write-child",
                                            "name": "write_file",
                                            "namespace": "oppi",
                                            "arguments": {
                                                "path": "child.txt",
                                                "content": "ok"
                                            }
                                        }
                                    ],
                                    "finalResponse": true
                                }
                            ]
                        }),
                    )],
                    tool_results: Vec::new(),
                    final_response: true,
                }],
                model_provider: None,
                tool_definitions: vec![non_approval_write_file_definition()],
                approved_tool_call_ids: Vec::new(),
                cancellation: None,
                max_continuations: Some(3),
            })
            .unwrap();

        let output = completed_tool_output_json(&result, "agent-1");
        assert_eq!(output["worktreeRoot"], json!(child.display().to_string()));
        assert_eq!(output["executionRoot"], json!(child.display().to_string()));
        assert!(child.join("child.txt").exists());
        assert!(!parent.join("child.txt").exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn agent_tool_worktree_policy_blocks_parent_root_write() {
        let root = temp_project_root("agent-tool-worktree-policy");
        let parent = root.join("parent");
        let child = parent.join("worktrees").join("reviewer");
        fs::create_dir_all(&child).unwrap();
        let parent_target = parent.join("parent.txt");

        let mut runtime = Runtime::new();
        let thread = thread_with_cwd(&mut runtime, &parent);
        let result = runtime
            .run_agentic_turn(AgenticTurnParams {
                thread_id: thread.id.clone(),
                input: "delegate parent-root write".to_string(),
                execution_mode: ExecutionMode::Blocking,
                follow_up: None,
                sandbox_policy: Some(default_policy(PermissionProfile {
                    mode: PermissionMode::Default,
                    readable_roots: vec![parent.display().to_string()],
                    writable_roots: vec![parent.display().to_string()],
                    filesystem_rules: Vec::new(),
                    protected_patterns: Vec::new(),
                })),
                model_steps: vec![ScriptedModelStep {
                    assistant_deltas: Vec::new(),
                    tool_calls: vec![agent_tool_call(
                        "agent-1",
                        json!({
                            "agentName": "general-purpose",
                            "task": "try parent write",
                            "worktreeRoot": child.display().to_string(),
                            "modelSteps": [
                                {
                                    "toolCalls": [
                                        {
                                            "id": "write-parent",
                                            "name": "write_file",
                                            "namespace": "oppi",
                                            "arguments": {
                                                "path": parent_target.display().to_string(),
                                                "content": "no"
                                            }
                                        }
                                    ],
                                    "finalResponse": true
                                }
                            ]
                        }),
                    )],
                    tool_results: Vec::new(),
                    final_response: true,
                }],
                model_provider: None,
                tool_definitions: vec![non_approval_write_file_definition()],
                approved_tool_call_ids: Vec::new(),
                cancellation: None,
                max_continuations: Some(3),
            })
            .unwrap();

        assert!(result.events.iter().any(|event| matches!(
            &event.kind,
            EventKind::ToolCallCompleted {
                result: ToolResult {
                    call_id,
                    status: ToolResultStatus::Denied,
                    error: Some(error),
                    ..
                }
            } if call_id == "write-parent" && error.contains("project working directory")
        )));
        assert!(!parent_target.exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn background_agent_tool_returns_running_handle_without_blocking_parent() {
        let root = temp_project_root("agent-tool-background-handle");
        let parent = root.join("parent");
        fs::create_dir_all(&parent).unwrap();

        let mut runtime = Runtime::new();
        let thread = thread_with_cwd(&mut runtime, &parent);
        let result = runtime
            .run_agentic_turn(AgenticTurnParams {
                thread_id: thread.id.clone(),
                input: "delegate slow background verification".to_string(),
                execution_mode: ExecutionMode::Blocking,
                follow_up: None,
                sandbox_policy: Some(default_policy(PermissionProfile {
                    mode: PermissionMode::Default,
                    readable_roots: vec![parent.display().to_string()],
                    writable_roots: vec![parent.display().to_string()],
                    filesystem_rules: Vec::new(),
                    protected_patterns: Vec::new(),
                })),
                model_steps: vec![ScriptedModelStep {
                    assistant_deltas: Vec::new(),
                    tool_calls: vec![agent_tool_call(
                        "agent-1",
                        json!({
                            "agentName": "general-purpose",
                            "task": "verify slowly",
                            "background": true,
                            "modelSteps": [
                                {
                                    "assistantDeltas": ["still checking"],
                                    "finalResponse": false
                                }
                            ]
                        }),
                    )],
                    tool_results: Vec::new(),
                    final_response: true,
                }],
                model_provider: None,
                tool_definitions: Vec::new(),
                approved_tool_call_ids: Vec::new(),
                cancellation: None,
                max_continuations: Some(3),
            })
            .unwrap();

        let output = completed_tool_output_json(&result, "agent-1");
        assert_eq!(output["status"], json!("running"));
        assert!(output["runId"].as_str().unwrap().starts_with("agent-run-"));
        assert_eq!(output["background"], json!(true));
        assert!(
            !output
                .to_string()
                .contains("parent tool completion remains blocking")
        );
        assert!(!result.events.iter().any(|event| matches!(
            &event.kind,
            EventKind::ItemCompleted { item }
                if matches!(&item.kind, ItemKind::AssistantMessage { text } if text.contains("still checking"))
        )));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn background_agent_poll_completes_pending_child_run() {
        let root = temp_project_root("agent-tool-background-poll");
        let parent = root.join("parent");
        fs::create_dir_all(&parent).unwrap();

        let mut runtime = Runtime::new();
        let thread = thread_with_cwd(&mut runtime, &parent);
        let result = runtime
            .run_agentic_turn(AgenticTurnParams {
                thread_id: thread.id.clone(),
                input: "delegate background verification".to_string(),
                execution_mode: ExecutionMode::Blocking,
                follow_up: None,
                sandbox_policy: Some(default_policy(PermissionProfile {
                    mode: PermissionMode::Default,
                    readable_roots: vec![parent.display().to_string()],
                    writable_roots: vec![parent.display().to_string()],
                    filesystem_rules: Vec::new(),
                    protected_patterns: Vec::new(),
                })),
                model_steps: vec![ScriptedModelStep {
                    assistant_deltas: Vec::new(),
                    tool_calls: vec![agent_tool_call(
                        "agent-1",
                        json!({
                            "agentName": "general-purpose",
                            "task": "finish in background",
                            "background": true,
                            "modelSteps": [
                                {
                                    "assistantDeltas": ["background child done"],
                                    "finalResponse": true
                                }
                            ]
                        }),
                    )],
                    tool_results: Vec::new(),
                    final_response: true,
                }],
                model_provider: None,
                tool_definitions: Vec::new(),
                approved_tool_call_ids: Vec::new(),
                cancellation: None,
                max_continuations: Some(3),
            })
            .unwrap();

        let output = completed_tool_output_json(&result, "agent-1");
        let run_id = output["runId"].as_str().unwrap().to_string();
        let polled = runtime
            .poll_agent_run(AgentRunPollParams {
                thread_id: thread.id.clone(),
                run_id: run_id.clone(),
            })
            .unwrap();

        assert_eq!(polled.run.status, AgentRunStatus::Completed);
        assert_eq!(
            polled.run.child_thread_id,
            output["childThreadId"].as_str().map(str::to_string)
        );
        assert!(polled.run.child_turn_id.is_some());
        assert_eq!(polled.output.as_deref(), Some("background child done"));
        assert!(polled.events.iter().any(|event| matches!(
            &event.kind,
            EventKind::AgentCompleted { run_id: completed_run_id, output }
                if completed_run_id == &run_id && output == "background child done"
        )));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn background_agent_resume_approval_completes_child_run() {
        let root = temp_project_root("agent-tool-background-approval");
        let parent = root.join("parent");
        fs::create_dir_all(&parent).unwrap();
        let mut child_call = agentic_call("echo-approval", "approved child output");
        child_call.arguments = json!({
            "output": "approved child output",
            "requireApproval": true
        });

        let mut runtime = Runtime::new();
        let thread = thread_with_cwd(&mut runtime, &parent);
        let result = runtime
            .run_agentic_turn(AgenticTurnParams {
                thread_id: thread.id.clone(),
                input: "delegate background approval".to_string(),
                execution_mode: ExecutionMode::Blocking,
                follow_up: None,
                sandbox_policy: None,
                model_steps: vec![ScriptedModelStep {
                    assistant_deltas: Vec::new(),
                    tool_calls: vec![agent_tool_call(
                        "agent-1",
                        json!({
                            "agentName": "general-purpose",
                            "task": "wait for approval",
                            "background": true,
                            "modelSteps": [{
                                "toolCalls": [child_call.clone()],
                                "finalResponse": false
                            }]
                        }),
                    )],
                    tool_results: Vec::new(),
                    final_response: true,
                }],
                model_provider: None,
                tool_definitions: Vec::new(),
                approved_tool_call_ids: Vec::new(),
                cancellation: None,
                max_continuations: Some(3),
            })
            .unwrap();
        let output = completed_tool_output_json(&result, "agent-1");
        let run_id = output["runId"].as_str().unwrap().to_string();

        let blocked = runtime
            .poll_agent_run(AgentRunPollParams {
                thread_id: thread.id.clone(),
                run_id: run_id.clone(),
            })
            .unwrap();
        assert_eq!(blocked.run.status, AgentRunStatus::Blocked);
        let child_turn_id = blocked.run.child_turn_id.clone().unwrap();
        assert!(blocked.events.iter().any(|event| matches!(
            &event.kind,
            EventKind::ApprovalRequested { request }
                if request.tool_call.as_ref().map(|call| call.id.as_str()) == Some("echo-approval")
        )));

        let resumed = runtime
            .resume_agent_run(AgentRunResumeParams {
                thread_id: thread.id.clone(),
                run_id: run_id.clone(),
                follow_up: None,
                ask_user_response: None,
                sandbox_policy: None,
                model_steps: vec![
                    ScriptedModelStep {
                        assistant_deltas: Vec::new(),
                        tool_calls: vec![child_call],
                        tool_results: Vec::new(),
                        final_response: false,
                    },
                    ScriptedModelStep {
                        assistant_deltas: vec!["Approved background child done.".to_string()],
                        tool_calls: Vec::new(),
                        tool_results: Vec::new(),
                        final_response: true,
                    },
                ],
                model_provider: None,
                approved_tool_call_ids: vec!["echo-approval".to_string()],
                cancellation: None,
                max_continuations: Some(4),
            })
            .unwrap();

        assert_eq!(resumed.run.status, AgentRunStatus::Completed);
        assert_eq!(
            resumed.run.child_turn_id.as_deref(),
            Some(child_turn_id.as_str())
        );
        assert_eq!(
            resumed.output.as_deref(),
            Some("Approved background child done.")
        );
        assert!(
            resumed
                .events
                .iter()
                .any(|event| matches!(&event.kind, EventKind::ApprovalResolved { .. }))
        );
        assert!(resumed.events.iter().any(|event| matches!(
            &event.kind,
            EventKind::AgentCompleted { run_id: completed_run_id, output }
                if completed_run_id == run_id.as_str() && output == "Approved background child done."
        )));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn background_agent_resume_question_completes_child_run() {
        let root = temp_project_root("agent-tool-background-question");
        let parent = root.join("parent");
        fs::create_dir_all(&parent).unwrap();

        let mut runtime = Runtime::new();
        let thread = thread_with_cwd(&mut runtime, &parent);
        let result = runtime
            .run_agentic_turn(AgenticTurnParams {
                thread_id: thread.id.clone(),
                input: "delegate background question".to_string(),
                execution_mode: ExecutionMode::Blocking,
                follow_up: None,
                sandbox_policy: None,
                model_steps: vec![ScriptedModelStep {
                    assistant_deltas: Vec::new(),
                    tool_calls: vec![agent_tool_call(
                        "agent-1",
                        json!({
                            "agentName": "general-purpose",
                            "task": "ask and continue",
                            "background": true,
                            "modelSteps": [{
                                "toolCalls": [{
                                    "id": "ask-1",
                                    "name": "ask_user",
                                    "namespace": "oppi",
                                    "arguments": {
                                        "title": "Choose",
                                        "questions": [{
                                            "id": "path",
                                            "question": "Which path?",
                                            "options": [{ "id": "safe", "label": "Safe path" }]
                                        }]
                                    }
                                }],
                                "finalResponse": false
                            }]
                        }),
                    )],
                    tool_results: Vec::new(),
                    final_response: true,
                }],
                model_provider: None,
                tool_definitions: Vec::new(),
                approved_tool_call_ids: Vec::new(),
                cancellation: None,
                max_continuations: Some(3),
            })
            .unwrap();
        let output = completed_tool_output_json(&result, "agent-1");
        let run_id = output["runId"].as_str().unwrap().to_string();

        let blocked = runtime
            .poll_agent_run(AgentRunPollParams {
                thread_id: thread.id.clone(),
                run_id: run_id.clone(),
            })
            .unwrap();
        assert_eq!(blocked.run.status, AgentRunStatus::Blocked);
        let question = blocked
            .events
            .iter()
            .find_map(|event| match &event.kind {
                EventKind::AskUserRequested { request } => Some(request.clone()),
                _ => None,
            })
            .expect("background child should request user input");

        let resumed = runtime
            .resume_agent_run(AgentRunResumeParams {
                thread_id: thread.id.clone(),
                run_id: run_id.clone(),
                follow_up: None,
                ask_user_response: Some(AskUserResponse {
                    request_id: question.id,
                    answers: vec![AskUserAnswer {
                        question_id: "path".to_string(),
                        option_id: Some("safe".to_string()),
                        label: Some("Safe path".to_string()),
                        text: None,
                        skipped: false,
                    }],
                    cancelled: false,
                    timed_out: false,
                }),
                sandbox_policy: None,
                model_steps: vec![ScriptedModelStep {
                    assistant_deltas: vec!["Question answered background child done.".to_string()],
                    tool_calls: Vec::new(),
                    tool_results: Vec::new(),
                    final_response: true,
                }],
                model_provider: None,
                approved_tool_call_ids: Vec::new(),
                cancellation: None,
                max_continuations: Some(4),
            })
            .unwrap();

        assert_eq!(resumed.run.status, AgentRunStatus::Completed);
        assert_eq!(
            resumed.output.as_deref(),
            Some("Question answered background child done.")
        );
        assert!(
            resumed
                .events
                .iter()
                .any(|event| matches!(&event.kind, EventKind::AskUserResolved { .. }))
        );
        assert!(resumed.events.iter().any(|event| matches!(
            &event.kind,
            EventKind::AgentCompleted { run_id: completed_run_id, output }
                if completed_run_id == run_id.as_str()
                    && output == "Question answered background child done."
        )));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn registering_agent_override_preserves_shadowed_definition() {
        let mut runtime = Runtime::new();
        let thread = thread(&mut runtime);
        runtime
            .register_agent(
                &thread.id,
                AgentDefinition {
                    name: "Explore".to_string(),
                    description: "project explorer".to_string(),
                    source: Some(AgentSource::Project),
                    tools: vec!["read".to_string()],
                    tool_denylist: Vec::new(),
                    model: None,
                    effort: None,
                    permission_mode: Some(PermissionMode::ReadOnly),
                    memory_mode: None,
                    background: false,
                    worktree_root: None,
                    isolation: None,
                    color: None,
                    skills: Vec::new(),
                    max_turns: None,
                    path: None,
                    filename: None,
                    read_only: false,
                    instructions: "Project-specific exploration.".to_string(),
                },
            )
            .unwrap();

        let explore = runtime
            .list_agents(AgentListParams {
                thread_id: Some(thread.id.clone()),
            })
            .unwrap()
            .items
            .into_iter()
            .find(|agent| agent.active.name == "Explore")
            .unwrap();
        assert_eq!(explore.active.source, Some(AgentSource::Project));
        assert!(
            explore
                .shadowed
                .iter()
                .any(|agent| agent.source == Some(AgentSource::BuiltIn))
        );
    }

    #[test]
    fn agents_list_loads_project_markdown_without_runtime_register() {
        let root = temp_project_root("agents-list-project-markdown");
        fs::create_dir_all(root.join(".oppi").join("agents")).unwrap();
        fs::write(
            root.join(".oppi").join("agents").join("reviewer.md"),
            "---\nname: reviewer\ndescription: Project reviewer\n---\nReview the code.\n",
        )
        .unwrap();

        let mut runtime = Runtime::new();
        let thread = thread_with_cwd(&mut runtime, &root);
        let result = runtime
            .list_agents(AgentListParams {
                thread_id: Some(thread.id.clone()),
            })
            .unwrap();

        assert!(result.items.iter().any(|agent| {
            agent.active.name == "reviewer" && agent.active.source == Some(AgentSource::Project)
        }));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn agents_delete_denies_claude_read_only_source() {
        let root = temp_project_root("agents-delete-claude-read-only");
        fs::create_dir_all(root.join(".claude").join("agents")).unwrap();
        fs::write(
            root.join(".claude").join("agents").join("reviewer.md"),
            "---\nname: reviewer\ndescription: Claude reviewer\n---\nRead only.\n",
        )
        .unwrap();

        let mut runtime = Runtime::new();
        let thread = thread_with_cwd(&mut runtime, &root);
        let err = runtime
            .delete_agent(AgentDeleteParams {
                thread_id: thread.id,
                source: AgentSource::ClaudeProject,
                name: "reviewer".to_string(),
            })
            .unwrap_err();

        assert!(err.message.contains("read-only") || err.message.contains("not writable"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn rejects_agent_lifecycle_updates_for_wrong_thread() {
        let mut runtime = Runtime::new();
        let thread_one = thread(&mut runtime);
        let thread_two = thread(&mut runtime);
        let run = runtime
            .dispatch_agent(AgentDispatchParams {
                thread_id: thread_one.id.clone(),
                agent_name: "general-purpose".to_string(),
                task: "work".to_string(),
                worktree_root: None,
                background: false,
                role: None,
                model: None,
                effort: None,
                permission_mode: None,
                memory_mode: None,
                tool_allowlist: Vec::new(),
                tool_denylist: Vec::new(),
                isolation: None,
                color: None,
                skills: Vec::new(),
                max_turns: None,
            })
            .unwrap()
            .run;

        let result = runtime.block_agent(AgentBlockParams {
            thread_id: thread_two.id,
            run_id: run.id.clone(),
            reason: "wrong thread".to_string(),
        });

        assert_eq!(result.unwrap_err().code, "agent_run_thread_mismatch");
        assert_eq!(
            runtime.agent_runs.get(&run.id).unwrap().status,
            AgentRunStatus::Running
        );
    }

    #[test]
    fn approval_and_question_responses_must_use_owner_thread() {
        let mut runtime = Runtime::new();
        let thread_one = thread(&mut runtime);
        let thread_two = thread(&mut runtime);
        runtime
            .request_approval(
                &thread_one.id,
                ApprovalRequest {
                    id: "approval-custom".to_string(),
                    reason: "test".to_string(),
                    risk: RiskLevel::Medium,
                    tool_call: None,
                },
            )
            .unwrap();
        runtime
            .request_question(
                &thread_one.id,
                QuestionRequest {
                    id: "question-custom".to_string(),
                    prompt: "continue?".to_string(),
                    options: Vec::new(),
                },
            )
            .unwrap();

        let approval = runtime.respond_approval(ApprovalRespondParams {
            thread_id: thread_two.id.clone(),
            request_id: "approval-custom".to_string(),
            decision: ApprovalOutcome::Approved,
            message: None,
        });
        let question = runtime.respond_question(QuestionRespondParams {
            thread_id: thread_two.id,
            request_id: "question-custom".to_string(),
            answer: "yes".to_string(),
        });

        assert_eq!(approval.unwrap_err().code, "approval_thread_mismatch");
        assert_eq!(question.unwrap_err().code, "question_thread_mismatch");
    }

    #[test]
    fn duplicate_approval_and_question_ids_are_rejected() {
        let mut runtime = Runtime::new();
        let thread = thread(&mut runtime);
        let approval = ApprovalRequest {
            id: "approval-custom".to_string(),
            reason: "test".to_string(),
            risk: RiskLevel::Medium,
            tool_call: None,
        };
        runtime
            .request_approval(&thread.id, approval.clone())
            .unwrap();
        assert_eq!(
            runtime
                .request_approval(&thread.id, approval)
                .unwrap_err()
                .code,
            "approval_already_exists"
        );

        let question = QuestionRequest {
            id: "question-custom".to_string(),
            prompt: "continue?".to_string(),
            options: Vec::new(),
        };
        runtime
            .request_question(&thread.id, question.clone())
            .unwrap();
        assert_eq!(
            runtime
                .request_question(&thread.id, question)
                .unwrap_err()
                .code,
            "question_already_exists"
        );
    }

    #[test]
    fn start_turn_rejects_bad_simulated_tool_without_partial_events() {
        let mut runtime = Runtime::new();
        let thread = thread(&mut runtime);
        let after = runtime
            .events_after(EventsListParams {
                thread_id: thread.id.clone(),
                after: 0,
                limit: None,
            })
            .unwrap()
            .events
            .last()
            .map(|event| event.id)
            .unwrap_or(0);
        let mut bad = tool("read-1", "read", false);
        bad.result.call_id = "other-call".to_string();

        let result = runtime.start_turn(TurnStartParams {
            thread_id: thread.id.clone(),
            input: "bad".to_string(),
            assistant_response: None,
            simulated_tool: Some(bad),
            requested_continuations: 0,
            stop_hook_feedback: None,
            defer_completion: false,
        });

        assert_eq!(result.unwrap_err().code, "tool_pair_mismatch");
        assert!(
            runtime
                .events_after(EventsListParams {
                    thread_id: thread.id,
                    after,
                    limit: None,
                })
                .unwrap()
                .events
                .is_empty()
        );
    }

    #[test]
    fn thread_items_lists_completed_items_with_runtime_filters() {
        let mut runtime = Runtime::new();
        let thread = thread(&mut runtime);
        runtime
            .start_turn(TurnStartParams {
                thread_id: thread.id.clone(),
                input: "show status".to_string(),
                assistant_response: Some("status looks clean".to_string()),
                simulated_tool: Some(tool("call-1", "shell_exec", true)),
                requested_continuations: 0,
                stop_hook_feedback: None,
                defer_completion: false,
            })
            .unwrap();

        let default_items = runtime
            .list_thread_items(ThreadItemsParams {
                thread_id: thread.id.clone(),
                after: 0,
                filter: Some(ThreadItemFilter::Default),
                limit: None,
            })
            .unwrap();
        assert_eq!(default_items.filter, ThreadItemFilter::Default);
        assert_eq!(default_items.after, 0);
        assert_eq!(default_items.branch.thread_id, thread.id);
        assert_eq!(default_items.branch.parent_thread_id, None);
        assert_eq!(default_items.branch.child_thread_ids, Vec::<String>::new());
        assert_eq!(
            default_items.branch.sibling_thread_ids,
            Vec::<String>::new()
        );
        assert_eq!(default_items.branch.path, vec![thread.id.clone()]);
        assert_eq!(default_items.branch.depth, 0);
        assert_eq!(default_items.total_count, 2);
        assert_eq!(default_items.entries.len(), 2);
        assert_eq!(default_items.entries[0].parent_event_id, None);
        assert_eq!(
            default_items.entries[1].parent_event_id,
            Some(default_items.entries[0].event_id)
        );
        assert_eq!(
            default_items.entries[0].child_event_ids,
            vec![default_items.entries[1].event_id]
        );
        assert!(default_items.entries[1].child_event_ids.is_empty());
        assert!(default_items.entries[0].completed_at_ms > 0);
        assert!(
            default_items.entries[1].completed_at_ms >= default_items.entries[0].completed_at_ms
        );
        assert_eq!(default_items.entries[0].item, default_items.items[0]);
        assert_eq!(default_items.items.len(), 2);
        assert!(matches!(
            default_items.items[0].kind,
            ItemKind::UserMessage { .. }
        ));
        assert!(matches!(
            default_items.items[1].kind,
            ItemKind::AssistantMessage { .. }
        ));

        let all_items = runtime
            .list_thread_items(ThreadItemsParams {
                thread_id: thread.id.clone(),
                after: 0,
                filter: Some(ThreadItemFilter::All),
                limit: None,
            })
            .unwrap();
        assert_eq!(all_items.total_count, 4);
        assert!(
            all_items
                .items
                .iter()
                .any(|item| matches!(item.kind, ItemKind::ToolCall(_)))
        );
        assert!(
            all_items
                .items
                .iter()
                .any(|item| matches!(item.kind, ItemKind::ToolResult(_)))
        );

        let user_items = runtime
            .list_thread_items(ThreadItemsParams {
                thread_id: thread.id.clone(),
                after: 0,
                filter: Some(ThreadItemFilter::UserOnly),
                limit: Some(10),
            })
            .unwrap();
        assert_eq!(user_items.total_count, 1);
        assert_eq!(user_items.items.len(), 1);
        assert!(matches!(
            user_items.items[0].kind,
            ItemKind::UserMessage { .. }
        ));

        let labeled_event_id = all_items.entries[0].event_id;
        let labeled = runtime
            .set_thread_item_labels(ThreadItemLabelsUpdateParams {
                thread_id: thread.id.clone(),
                event_id: labeled_event_id,
                labels: vec!["checkpoint".to_string(), "checkpoint".to_string()],
            })
            .unwrap();
        assert_eq!(labeled.labels, vec!["checkpoint"]);
        assert!(labeled.events.iter().any(|event| {
            match event.kind.clone() {
                EventKind::ThreadItemLabelsUpdated {
                    target_event_id,
                    labels,
                } => {
                    target_event_id == labeled_event_id && labels == vec!["checkpoint".to_string()]
                }
                _ => false,
            }
        }));

        let labeled_items = runtime
            .list_thread_items(ThreadItemsParams {
                thread_id: thread.id.clone(),
                after: 0,
                filter: Some(ThreadItemFilter::LabeledOnly),
                limit: None,
            })
            .unwrap();
        assert_eq!(labeled_items.total_count, 1);
        assert_eq!(labeled_items.entries[0].event_id, labeled_event_id);
        assert_eq!(labeled_items.entries[0].labels, vec!["checkpoint"]);

        let replay_events = runtime.event_store.list_thread(&thread.id).unwrap();
        let replayed = Runtime::replay_events(&replay_events).unwrap();
        let replayed_labeled_items = replayed
            .list_thread_items(ThreadItemsParams {
                thread_id: thread.id.clone(),
                after: 0,
                filter: Some(ThreadItemFilter::LabeledOnly),
                limit: None,
            })
            .unwrap();
        assert_eq!(replayed_labeled_items.total_count, 1);
        assert_eq!(replayed_labeled_items.entries[0].labels, vec!["checkpoint"]);

        let limited_items = runtime
            .list_thread_items(ThreadItemsParams {
                thread_id: thread.id.clone(),
                after: 0,
                filter: Some(ThreadItemFilter::All),
                limit: Some(1),
            })
            .unwrap();
        assert_eq!(limited_items.total_count, 4);
        assert_eq!(limited_items.items.len(), 1);
        assert_eq!(limited_items.entries.len(), 1);
        assert_eq!(
            limited_items.entries[0].child_event_ids,
            vec![all_items.entries[1].event_id]
        );

        let after_first_user = all_items.entries[0].event_id;
        let cursor_items = runtime
            .list_thread_items(ThreadItemsParams {
                thread_id: thread.id,
                after: after_first_user,
                filter: Some(ThreadItemFilter::All),
                limit: None,
            })
            .unwrap();
        assert_eq!(cursor_items.after, after_first_user);
        assert_eq!(cursor_items.total_count, 3);
        assert!(
            cursor_items
                .entries
                .iter()
                .all(|entry| entry.event_id > after_first_user)
        );
        assert!(matches!(
            cursor_items.entries[0].item.kind,
            ItemKind::ToolCall(_)
        ));
        assert_eq!(
            cursor_items.entries[0].parent_event_id,
            Some(after_first_user)
        );
        assert_eq!(
            cursor_items.entries[0].child_event_ids,
            vec![cursor_items.entries[1].event_id]
        );
    }

    #[test]
    fn thread_items_include_branch_metadata_from_fork_lineage() {
        let mut runtime = Runtime::new();
        let root = thread(&mut runtime);
        let first_child = runtime
            .fork_thread(&root.id, Some("first child".to_string()))
            .unwrap()
            .thread;
        let second_child = runtime
            .fork_thread(&root.id, Some("second child".to_string()))
            .unwrap()
            .thread;
        let grandchild = runtime
            .fork_thread(&first_child.id, Some("grandchild".to_string()))
            .unwrap()
            .thread;

        let root_items = runtime
            .list_thread_items(ThreadItemsParams {
                thread_id: root.id.clone(),
                after: 0,
                filter: Some(ThreadItemFilter::Default),
                limit: None,
            })
            .unwrap();
        assert_eq!(root_items.branch.thread_id, root.id);
        assert_eq!(root_items.branch.parent_thread_id, None);
        assert_eq!(
            root_items.branch.child_thread_ids,
            vec![first_child.id.clone(), second_child.id.clone()]
        );
        assert_eq!(root_items.branch.sibling_thread_ids, Vec::<String>::new());
        assert_eq!(root_items.branch.path, vec![root.id.clone()]);
        assert_eq!(root_items.branch.depth, 0);

        let grandchild_items = runtime
            .list_thread_items(ThreadItemsParams {
                thread_id: grandchild.id.clone(),
                after: 0,
                filter: Some(ThreadItemFilter::Default),
                limit: None,
            })
            .unwrap();
        assert_eq!(grandchild_items.branch.thread_id, grandchild.id);
        assert_eq!(
            grandchild_items.branch.parent_thread_id,
            Some(first_child.id.clone())
        );
        assert_eq!(
            grandchild_items.branch.child_thread_ids,
            Vec::<String>::new()
        );
        assert_eq!(
            grandchild_items.branch.sibling_thread_ids,
            Vec::<String>::new()
        );
        assert_eq!(
            grandchild_items.branch.path,
            vec![
                root.id.clone(),
                first_child.id.clone(),
                grandchild.id.clone()
            ]
        );
        assert_eq!(grandchild_items.branch.depth, 2);

        let second_child_items = runtime
            .list_thread_items(ThreadItemsParams {
                thread_id: second_child.id.clone(),
                after: 0,
                filter: Some(ThreadItemFilter::Default),
                limit: None,
            })
            .unwrap();
        assert_eq!(
            second_child_items.branch.sibling_thread_ids,
            vec![first_child.id]
        );
    }

    #[test]
    fn item_fork_uses_parent_history_only_through_selected_event() {
        let mut runtime = Runtime::new();
        let root = thread(&mut runtime);
        let first = runtime
            .start_turn(TurnStartParams {
                thread_id: root.id.clone(),
                input: "first question".to_string(),
                assistant_response: Some("first answer".to_string()),
                simulated_tool: None,
                requested_continuations: 0,
                stop_hook_feedback: None,
                defer_completion: false,
            })
            .unwrap();
        let first_assistant_event_id = first
            .events
            .iter()
            .find_map(|event| match &event.kind {
                EventKind::ItemCompleted { item }
                    if matches!(item.kind, ItemKind::AssistantMessage { .. }) =>
                {
                    Some(event.id)
                }
                _ => None,
            })
            .expect("first assistant item event");
        runtime
            .start_turn(TurnStartParams {
                thread_id: root.id.clone(),
                input: "second question".to_string(),
                assistant_response: Some("second answer".to_string()),
                simulated_tool: None,
                requested_continuations: 0,
                stop_hook_feedback: None,
                defer_completion: false,
            })
            .unwrap();

        let forked = runtime
            .fork_thread_from_item(ThreadItemForkParams {
                thread_id: root.id.clone(),
                event_id: first_assistant_event_id,
                title: Some("rewrite first".to_string()),
            })
            .unwrap()
            .thread;
        runtime
            .start_turn(TurnStartParams {
                thread_id: forked.id.clone(),
                input: "child rewrite".to_string(),
                assistant_response: Some("child answer".to_string()),
                simulated_tool: None,
                requested_continuations: 0,
                stop_hook_feedback: None,
                defer_completion: false,
            })
            .unwrap();

        let history = runtime.compact_provider_history(&forked.id, "turn-next");
        let joined = history
            .iter()
            .map(|message| message.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("first question"));
        assert!(joined.contains("first answer"));
        assert!(joined.contains("child rewrite"));
        assert!(joined.contains("child answer"));
        assert!(!joined.contains("second question"));
        assert!(!joined.contains("second answer"));

        let branch = runtime
            .list_thread_items(ThreadItemsParams {
                thread_id: forked.id.clone(),
                after: 0,
                filter: Some(ThreadItemFilter::Default),
                limit: None,
            })
            .unwrap()
            .branch;
        assert_eq!(branch.parent_thread_id, Some(root.id.clone()));
        assert_eq!(branch.fork_event_id, Some(first_assistant_event_id));

        let mut replay_events = runtime.event_store.list_thread(&root.id).unwrap();
        replay_events.extend(runtime.event_store.list_thread(&forked.id).unwrap());
        let replayed = Runtime::replay_events(&replay_events).unwrap();
        let replayed_branch = replayed
            .list_thread_items(ThreadItemsParams {
                thread_id: forked.id,
                after: 0,
                filter: Some(ThreadItemFilter::Default),
                limit: None,
            })
            .unwrap()
            .branch;
        assert_eq!(
            replayed_branch.fork_event_id,
            Some(first_assistant_event_id)
        );
    }

    #[test]
    fn item_edit_fork_replaces_selected_user_message_history() {
        let mut runtime = Runtime::new();
        let root = thread(&mut runtime);
        runtime
            .start_turn(TurnStartParams {
                thread_id: root.id.clone(),
                input: "setup question".to_string(),
                assistant_response: Some("setup answer".to_string()),
                simulated_tool: None,
                requested_continuations: 0,
                stop_hook_feedback: None,
                defer_completion: false,
            })
            .unwrap();
        let old = runtime
            .start_turn(TurnStartParams {
                thread_id: root.id.clone(),
                input: "old request".to_string(),
                assistant_response: Some("old answer".to_string()),
                simulated_tool: None,
                requested_continuations: 0,
                stop_hook_feedback: None,
                defer_completion: false,
            })
            .unwrap();
        let old_user_event_id = old
            .events
            .iter()
            .find_map(|event| match &event.kind {
                EventKind::ItemCompleted { item }
                    if matches!(item.kind, ItemKind::UserMessage { .. }) =>
                {
                    Some(event.id)
                }
                _ => None,
            })
            .expect("old user item event");

        let edited = runtime
            .fork_thread_from_item_edit(ThreadItemEditForkParams {
                thread_id: root.id.clone(),
                event_id: old_user_event_id,
                input: "edited request".to_string(),
                title: Some("edit old request".to_string()),
            })
            .unwrap();
        assert_eq!(edited.input, "edited request");
        assert_eq!(edited.source_event_id, old_user_event_id);
        assert!(edited.history_cutoff_event_id < old_user_event_id);
        let forked = edited.thread;
        runtime
            .start_turn(TurnStartParams {
                thread_id: forked.id.clone(),
                input: edited.input,
                assistant_response: Some("edited answer".to_string()),
                simulated_tool: None,
                requested_continuations: 0,
                stop_hook_feedback: None,
                defer_completion: false,
            })
            .unwrap();

        let history = runtime.compact_provider_history(&forked.id, "turn-next");
        let joined = history
            .iter()
            .map(|message| message.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("setup question"));
        assert!(joined.contains("setup answer"));
        assert!(joined.contains("edited request"));
        assert!(joined.contains("edited answer"));
        assert!(!joined.contains("old request"));
        assert!(!joined.contains("old answer"));

        let branch = runtime
            .list_thread_items(ThreadItemsParams {
                thread_id: forked.id.clone(),
                after: 0,
                filter: Some(ThreadItemFilter::Default),
                limit: None,
            })
            .unwrap()
            .branch;
        assert_eq!(branch.parent_thread_id, Some(root.id.clone()));
        assert_eq!(branch.fork_event_id, Some(old_user_event_id));

        let mut replay_events = runtime.event_store.list_thread(&root.id).unwrap();
        replay_events.extend(runtime.event_store.list_thread(&forked.id).unwrap());
        let replayed = Runtime::replay_events(&replay_events).unwrap();
        let replayed_history = replayed.compact_provider_history(&forked.id, "turn-next");
        let replayed_joined = replayed_history
            .iter()
            .map(|message| message.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(replayed_joined.contains("edited request"));
        assert!(!replayed_joined.contains("old request"));
    }

    #[test]
    fn approval_gated_tools_cannot_complete_successfully_before_approval() {
        let mut runtime = Runtime::new();
        let thread = thread(&mut runtime);
        let turn = runtime
            .start_turn(TurnStartParams {
                thread_id: thread.id.clone(),
                input: "turn".to_string(),
                assistant_response: None,
                simulated_tool: None,
                requested_continuations: 0,
                stop_hook_feedback: None,
                defer_completion: true,
            })
            .unwrap()
            .turn;
        let mut gated = tool("write-1", "write", false);
        gated.require_approval = true;

        let result = runtime.record_tool_batch(ToolBatchRecordParams {
            thread_id: thread.id,
            turn_id: turn.id,
            tools: vec![gated],
            max_concurrency: None,
        });

        assert_eq!(result.unwrap_err().code, "tool_requires_approval");
    }

    #[test]
    fn terminal_turns_reject_late_mutations() {
        let mut runtime = Runtime::new();
        let thread = thread(&mut runtime);
        let turn = runtime
            .start_turn(TurnStartParams {
                thread_id: thread.id.clone(),
                input: "complete".to_string(),
                assistant_response: None,
                simulated_tool: None,
                requested_continuations: 0,
                stop_hook_feedback: None,
                defer_completion: false,
            })
            .unwrap()
            .turn;

        let interrupt = runtime.interrupt_turn(&thread.id, &turn.id, "late".to_string());
        assert_eq!(interrupt.unwrap_err().code, "turn_not_mutable");
        let record = runtime.record_tool(ToolRecordParams {
            thread_id: thread.id,
            turn_id: turn.id,
            call: tool("late-call", "read", false).call,
            result: ToolResult {
                call_id: "late-call".to_string(),
                status: ToolResultStatus::Ok,
                output: Some("late".to_string()),
                error: None,
            },
        });
        assert_eq!(record.unwrap_err().code, "turn_not_mutable");
    }

    #[test]
    fn approval_and_question_resolution_is_single_use() {
        let mut runtime = Runtime::new();
        let thread = thread(&mut runtime);
        runtime
            .request_approval(
                &thread.id,
                ApprovalRequest {
                    id: "approval-once".to_string(),
                    reason: "test".to_string(),
                    risk: RiskLevel::Medium,
                    tool_call: None,
                },
            )
            .unwrap();
        runtime
            .respond_approval(ApprovalRespondParams {
                thread_id: thread.id.clone(),
                request_id: "approval-once".to_string(),
                decision: ApprovalOutcome::Approved,
                message: None,
            })
            .unwrap();
        assert_eq!(
            runtime
                .respond_approval(ApprovalRespondParams {
                    thread_id: thread.id.clone(),
                    request_id: "approval-once".to_string(),
                    decision: ApprovalOutcome::Denied,
                    message: None,
                })
                .unwrap_err()
                .code,
            "approval_already_resolved"
        );

        runtime
            .request_question(
                &thread.id,
                QuestionRequest {
                    id: "question-once".to_string(),
                    prompt: "continue?".to_string(),
                    options: Vec::new(),
                },
            )
            .unwrap();
        runtime
            .respond_question(QuestionRespondParams {
                thread_id: thread.id.clone(),
                request_id: "question-once".to_string(),
                answer: "yes".to_string(),
            })
            .unwrap();
        assert_eq!(
            runtime
                .respond_question(QuestionRespondParams {
                    thread_id: thread.id,
                    request_id: "question-once".to_string(),
                    answer: "no".to_string(),
                })
                .unwrap_err()
                .code,
            "question_already_resolved"
        );
    }

    #[test]
    fn approved_tool_call_can_complete_successfully_once_bound_to_approval() {
        let mut runtime = Runtime::new();
        let thread = thread(&mut runtime);
        let turn = runtime
            .start_turn(TurnStartParams {
                thread_id: thread.id.clone(),
                input: "turn".to_string(),
                assistant_response: None,
                simulated_tool: None,
                requested_continuations: 0,
                stop_hook_feedback: None,
                defer_completion: true,
            })
            .unwrap()
            .turn;
        let mut gated = tool("write-approved", "write", false);
        gated.require_approval = true;
        runtime
            .request_approval(
                &thread.id,
                ApprovalRequest {
                    id: "approval-write".to_string(),
                    reason: "write".to_string(),
                    risk: RiskLevel::Medium,
                    tool_call: Some(gated.call.clone()),
                },
            )
            .unwrap();
        runtime
            .respond_approval(ApprovalRespondParams {
                thread_id: thread.id.clone(),
                request_id: "approval-write".to_string(),
                decision: ApprovalOutcome::Approved,
                message: None,
            })
            .unwrap();

        let result = runtime
            .record_tool_batch(ToolBatchRecordParams {
                thread_id: thread.id,
                turn_id: turn.id,
                tools: vec![gated],
                max_concurrency: None,
            })
            .unwrap();
        assert!(
            result
                .events
                .iter()
                .any(|event| matches!(event.kind, EventKind::ToolCallCompleted { .. }))
        );
    }

    #[test]
    fn approval_and_question_events_do_not_emit_fake_turn_items() {
        let mut runtime = Runtime::new();
        let thread = thread(&mut runtime);
        let approval = runtime
            .request_approval(
                &thread.id,
                ApprovalRequest {
                    id: "".to_string(),
                    reason: "test".to_string(),
                    risk: RiskLevel::Low,
                    tool_call: None,
                },
            )
            .unwrap();
        let question = runtime
            .request_question(
                &thread.id,
                QuestionRequest {
                    id: "".to_string(),
                    prompt: "continue?".to_string(),
                    options: Vec::new(),
                },
            )
            .unwrap();

        assert!(approval.events.iter().all(|event| {
            !matches!(event.kind, EventKind::ItemCompleted { .. }) && event.turn_id.is_none()
        }));
        assert!(question.events.iter().all(|event| {
            !matches!(event.kind, EventKind::ItemCompleted { .. }) && event.turn_id.is_none()
        }));
    }

    #[test]
    fn same_source_agent_overrides_are_preserved_as_shadowed_layers() {
        let mut runtime = Runtime::new();
        let thread = thread(&mut runtime);
        for instructions in ["first", "second"] {
            runtime
                .register_agent(
                    &thread.id,
                    AgentDefinition {
                        name: "reviewer".to_string(),
                        description: instructions.to_string(),
                        source: Some(AgentSource::Project),
                        tools: vec!["read".to_string()],
                        tool_denylist: Vec::new(),
                        model: None,
                        effort: None,
                        permission_mode: Some(PermissionMode::ReadOnly),
                        memory_mode: None,
                        background: false,
                        worktree_root: None,
                        isolation: None,
                        color: None,
                        skills: Vec::new(),
                        max_turns: None,
                        path: None,
                        filename: None,
                        read_only: false,
                        instructions: instructions.to_string(),
                    },
                )
                .unwrap();
        }

        let reviewer = runtime
            .list_agents(AgentListParams {
                thread_id: Some(thread.id.clone()),
            })
            .unwrap()
            .items
            .into_iter()
            .find(|agent| agent.active.name == "reviewer")
            .unwrap();
        assert_eq!(reviewer.active.instructions, "second");
        assert!(
            reviewer
                .shadowed
                .iter()
                .any(|agent| agent.instructions == "first")
        );
    }

    #[test]
    fn separate_runtime_instances_do_not_share_threads_or_events() {
        let mut runtime_one = Runtime::new();
        let runtime_two = Runtime::new();
        let thread_one = thread(&mut runtime_one);

        assert_eq!(
            runtime_two
                .events_after(EventsListParams {
                    thread_id: thread_one.id,
                    after: 0,
                    limit: None,
                })
                .unwrap_err()
                .code,
            "thread_not_found"
        );
    }

    #[test]
    fn continuation_guard_aborts_loop() {
        let mut runtime = Runtime::new();
        let thread = thread(&mut runtime);
        let result = runtime
            .start_turn(TurnStartParams {
                thread_id: thread.id,
                input: "loop".to_string(),
                assistant_response: None,
                simulated_tool: None,
                requested_continuations: MAX_CONTINUATIONS + 1,
                stop_hook_feedback: None,
                defer_completion: false,
            })
            .unwrap();
        assert_eq!(result.turn.status, TurnStatus::Aborted);
        assert!(
            result
                .events
                .iter()
                .any(|event| matches!(event.kind, EventKind::TurnAborted { .. }))
        );
    }

    #[test]
    fn compaction_and_side_question_are_events_without_parent_message_mutation() {
        let mut runtime = Runtime::new();
        runtime.todos = TodoState {
            summary: "todo checkpoint".to_string(),
            todos: vec![
                TodoItem {
                    id: "active".to_string(),
                    content: "Continue runtime work".to_string(),
                    status: TodoStatus::InProgress,
                    priority: Some(TodoPriority::High),
                    phase: Some("implementation".to_string()),
                    notes: None,
                },
                TodoItem {
                    id: "done".to_string(),
                    content: "Archive outcome".to_string(),
                    status: TodoStatus::Completed,
                    priority: None,
                    phase: Some("validation".to_string()),
                    notes: Some("Outcome preserved".to_string()),
                },
            ],
        };
        let thread = thread(&mut runtime);
        let compact = runtime
            .compact_handoff(HandoffCompactParams {
                thread_id: thread.id.clone(),
                summary: "done items archived".to_string(),
                details: None,
            })
            .unwrap();
        let parent_item_events_before = runtime
            .event_store
            .list_thread(&thread.id)
            .unwrap()
            .into_iter()
            .filter(|event| matches!(event.kind, EventKind::ItemCompleted { .. }))
            .count();
        let side = runtime
            .side_question(SideQuestionParams {
                thread_id: thread.id.clone(),
                question: "what changed?".to_string(),
                parent_turn_id: None,
                execution_mode: ExecutionMode::Blocking,
                sandbox_policy: None,
                model_steps: Vec::new(),
                model_provider: None,
                tool_definitions: Vec::new(),
                approved_tool_call_ids: Vec::new(),
                cancellation: None,
                max_continuations: None,
            })
            .unwrap();
        let compact_details = compact
            .events
            .iter()
            .find_map(|event| match &event.kind {
                EventKind::HandoffCompacted { details, .. } => details.as_ref(),
                _ => None,
            })
            .expect("compaction details");
        assert_eq!(compact_details.remaining_todos.len(), 1);
        assert_eq!(
            compact_details.completed_outcomes[0].outcome,
            "Outcome preserved"
        );
        assert!(
            side.events
                .iter()
                .any(|event| matches!(event.kind, EventKind::SideQuestionAnswered { .. }))
        );
        let parent_item_events_after = runtime
            .event_store
            .list_thread(&thread.id)
            .unwrap()
            .into_iter()
            .filter(|event| matches!(event.kind, EventKind::ItemCompleted { .. }))
            .count();
        assert_eq!(parent_item_events_after, parent_item_events_before);
        assert!(
            side.events
                .iter()
                .filter(|event| matches!(event.kind, EventKind::ItemCompleted { .. }))
                .all(|event| event.thread_id == side.side_thread_id)
        );
    }

    #[test]
    fn compact_handoff_builds_todo_aware_summary_when_summary_is_blank() {
        let mut runtime = Runtime::new();
        runtime.todos = TodoState {
            summary: "todo checkpoint".to_string(),
            todos: vec![
                TodoItem {
                    id: "active".to_string(),
                    content: "Continue runtime work".to_string(),
                    status: TodoStatus::InProgress,
                    priority: Some(TodoPriority::High),
                    phase: Some("implementation".to_string()),
                    notes: None,
                },
                TodoItem {
                    id: "done".to_string(),
                    content: "Archive outcome".to_string(),
                    status: TodoStatus::Completed,
                    priority: None,
                    phase: Some("validation".to_string()),
                    notes: Some("Outcome preserved".to_string()),
                },
            ],
        };
        let thread = thread(&mut runtime);
        let compact = runtime
            .compact_handoff(HandoffCompactParams {
                thread_id: thread.id,
                summary: "   ".to_string(),
                details: None,
            })
            .unwrap();
        let summary = compact
            .events
            .iter()
            .find_map(|event| match &event.kind {
                EventKind::HandoffCompacted { summary, .. } => Some(summary.as_str()),
                _ => None,
            })
            .expect("compaction summary");

        assert!(summary.contains("OPPi todo-aware handoff"));
        assert!(summary.contains("Remaining todos:"));
        assert!(summary.contains("Continue runtime work"));
        assert!(summary.contains("Completed/cancelled outcomes:"));
        assert!(summary.contains("Outcome preserved"));
        assert!(summary.contains("Final response note:"));
    }

    #[test]
    fn side_question_runs_provider_turn_in_side_thread_without_parent_mutation() {
        let mut runtime = Runtime::new();
        let thread = thread(&mut runtime);
        let parent_before = runtime
            .event_store
            .list_thread(&thread.id)
            .unwrap()
            .into_iter()
            .filter(|event| matches!(event.kind, EventKind::ItemCompleted { .. }))
            .count();

        let result = runtime
            .side_question(SideQuestionParams {
                thread_id: thread.id.clone(),
                question: "What context does Guardian get?".to_string(),
                parent_turn_id: None,
                execution_mode: ExecutionMode::Blocking,
                sandbox_policy: None,
                model_steps: vec![ScriptedModelStep {
                    assistant_deltas: vec![
                        "Guardian sees the tool call and bounded history.".to_string(),
                    ],
                    tool_calls: Vec::new(),
                    tool_results: Vec::new(),
                    final_response: true,
                }],
                model_provider: None,
                tool_definitions: Vec::new(),
                approved_tool_call_ids: Vec::new(),
                cancellation: None,
                max_continuations: Some(1),
            })
            .unwrap();

        assert_eq!(result.parent_thread_id, thread.id);
        assert_ne!(result.side_thread_id, result.parent_thread_id);
        assert!(result.answer.contains("Guardian sees"));
        assert!(result.side_turn_id.is_some());
        assert_eq!(result.status, Some(TurnStatus::Completed));

        let parent_after = runtime
            .event_store
            .list_thread(&result.parent_thread_id)
            .unwrap()
            .into_iter()
            .filter(|event| matches!(event.kind, EventKind::ItemCompleted { .. }))
            .count();
        assert_eq!(parent_after, parent_before);
        assert!(
            runtime
                .event_store
                .list_thread(&result.side_thread_id)
                .unwrap()
                .into_iter()
                .any(|event| {
                    matches!(
                        event.kind,
                        EventKind::ItemCompleted {
                            item: Item {
                                kind: ItemKind::AssistantMessage { .. },
                                ..
                            }
                        }
                    )
                })
        );
    }

    #[test]
    fn mcp_action_records_status_flow() {
        let mut runtime = Runtime::new();
        let thread = thread(&mut runtime);
        runtime
            .register_mcp(
                &thread.id,
                McpServerRef {
                    id: "fs".to_string(),
                    name: "filesystem".to_string(),
                    status: McpServerStatus::Enabled,
                    source: Some(McpServerSource::Runtime),
                    transport: None,
                    trusted: true,
                    description: Some("File tools".to_string()),
                    tools: vec!["read".to_string()],
                    tool_refs: Vec::new(),
                    resource_subscriptions: Vec::new(),
                    when_to_use: Some("when filesystem context is required".to_string()),
                    capabilities: McpServerCapabilities::default(),
                    list_refresh_hints: McpListRefreshHints::default(),
                    diagnostics: Vec::new(),
                },
            )
            .unwrap();
        let result = runtime
            .mcp_action(McpActionParams {
                thread_id: thread.id,
                server_id: "fs".to_string(),
                action: McpAction::Test,
            })
            .unwrap();
        assert!(
            result
                .events
                .iter()
                .any(|event| matches!(event.kind, EventKind::Diagnostic { .. }))
        );
    }

    #[test]
    fn mcp_auth_action_reports_deferred_oauth_and_auth_ref_guidance() {
        let mut runtime = Runtime::new();
        let thread = thread(&mut runtime);
        runtime
            .register_mcp(
                &thread.id,
                McpServerRef {
                    id: "remote".to_string(),
                    name: "remote".to_string(),
                    status: McpServerStatus::Enabled,
                    source: Some(McpServerSource::Runtime),
                    transport: None,
                    trusted: true,
                    description: None,
                    tools: Vec::new(),
                    tool_refs: Vec::new(),
                    resource_subscriptions: Vec::new(),
                    when_to_use: None,
                    capabilities: McpServerCapabilities::default(),
                    list_refresh_hints: McpListRefreshHints::default(),
                    diagnostics: Vec::new(),
                },
            )
            .unwrap();

        let result = runtime
            .mcp_action(McpActionParams {
                thread_id: thread.id,
                server_id: "remote".to_string(),
                action: McpAction::Auth,
            })
            .unwrap();

        assert!(result.events.iter().any(|event| {
            matches!(
                &event.kind,
                EventKind::McpServerRegistered { server }
                    if server.status == McpServerStatus::AuthRequired
                        && server.diagnostics.iter().any(|diagnostic| {
                            diagnostic.code == "mcp_auth_required"
                                && diagnostic.message.contains("OAuth/browser auth is deferred")
                                && diagnostic.message.contains("authRef")
                        })
            )
        }));
        assert!(result.events.iter().any(|event| {
            matches!(
                &event.kind,
                EventKind::Diagnostic { diagnostic }
                    if diagnostic.message.contains("OAuth/browser auth is deferred")
                        && diagnostic.message.contains("authRef")
            )
        }));
    }

    #[test]
    fn mcp_auth_action_reports_streamable_http_auth_ref_env_state() {
        let env_name = format!("OPPI_MCP_AUTH_MISSING_{}", std::process::id());
        unsafe { std::env::remove_var(&env_name) };
        let mut runtime = Runtime::new();
        let thread = thread(&mut runtime);
        runtime
            .register_mcp(
                &thread.id,
                McpServerRef {
                    id: "remote-http".to_string(),
                    name: "remote-http".to_string(),
                    status: McpServerStatus::Enabled,
                    source: Some(McpServerSource::Runtime),
                    transport: Some(McpTransportConfig::StreamableHttp {
                        endpoint: "https://mcp.example.test/mcp".to_string(),
                        auth_ref: Some(format!("env:{env_name}")),
                    }),
                    trusted: true,
                    description: None,
                    tools: Vec::new(),
                    tool_refs: Vec::new(),
                    resource_subscriptions: Vec::new(),
                    when_to_use: None,
                    capabilities: McpServerCapabilities::default(),
                    list_refresh_hints: McpListRefreshHints::default(),
                    diagnostics: Vec::new(),
                },
            )
            .unwrap();

        let result = runtime
            .mcp_action(McpActionParams {
                thread_id: thread.id,
                server_id: "remote-http".to_string(),
                action: McpAction::Auth,
            })
            .unwrap();

        assert!(result.events.iter().any(|event| {
            matches!(
                &event.kind,
                EventKind::Diagnostic { diagnostic }
                    if diagnostic.message.contains("OAuth/browser auth is deferred")
                        && diagnostic.message.contains(&format!("authRef: \"env:{env_name}\""))
                        && diagnostic.message.contains(&format!("environment variable {env_name} is not set"))
                        && diagnostic.metadata.get("code").map(String::as_str) == Some("mcp_auth_required")
                        && diagnostic.metadata.get("authRefEnv").map(String::as_str) == Some(env_name.as_str())
                        && diagnostic.metadata.get("authRefState").map(String::as_str) == Some("missing")
            )
        }));
        assert!(result.events.iter().any(|event| {
            matches!(
                &event.kind,
                EventKind::McpServerRegistered { server }
                    if server.status == McpServerStatus::AuthRequired
                        && server.diagnostics.iter().any(|diagnostic| {
                            diagnostic.code == "mcp_auth_required"
                                && diagnostic.message.contains(&env_name)
                                && diagnostic.message.contains("not set")
                        })
            )
        }));
    }

    #[test]
    fn mcp_config_imports_project_mcp_json_as_untrusted_disabled() {
        let root = temp_project_root("mcp-project-config");
        fs::write(
            root.join(".mcp.json"),
            json!({
                "mcpServers": {
                    "fs": {
                        "command": "node",
                        "args": ["server.js"],
                        "env": {
                            "SAFE_ENV": "1",
                            "API_TOKEN": "raw-secret-token"
                        }
                    }
                }
            })
            .to_string(),
        )
        .unwrap();

        let mut runtime = Runtime::new();
        let thread = thread_with_cwd(&mut runtime, &root);
        let listed = runtime
            .list_mcp(McpListParams {
                thread_id: Some(thread.id),
            })
            .unwrap();
        let server = listed
            .items
            .iter()
            .find(|item| item.id == "fs")
            .expect("project .mcp.json server should be listed");

        assert_eq!(server.status, McpServerStatus::Disabled);
        assert_eq!(server.source, Some(McpServerSource::Project));
        assert_eq!(
            server.transport,
            Some(McpTransportConfig::Stdio {
                command: "node".to_string(),
                args: vec!["server.js".to_string()],
                env: BTreeMap::from([("SAFE_ENV".to_string(), "1".to_string())]),
                cwd: Some(root.display().to_string()),
            })
        );
        assert!(server.diagnostics.iter().any(|diag| {
            diag.code == "mcp_env_secret_redacted" && diag.message.contains("API_TOKEN")
        }));
        assert!(!server.trusted);
        assert!(server.diagnostics.iter().any(|diag| {
            diag.code == "mcp_requires_trust" && diag.message.contains("disabled until trusted")
        }));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn mcp_config_reports_user_http_sources_without_local_auth_as_untrusted() {
        let _env_lock = TEST_ENV_LOCK.lock().unwrap();
        let root = temp_project_root("mcp-user-http-config");
        let oppi_home = root.join("oppi-home");
        fs::create_dir_all(&oppi_home).unwrap();
        fs::write(
            oppi_home.join("mcp.json"),
            json!({
                "mcpServers": {
                    "remote": { "url": "http://example.com/mcp" },
                    "localNoAuth": { "url": "http://localhost:3111/mcp" }
                }
            })
            .to_string(),
        )
        .unwrap();
        let _home = EnvVarGuard::set("OPPI_HOME", &oppi_home);

        let mut runtime = Runtime::new();
        let thread = thread_with_cwd(&mut runtime, &root);
        let listed = runtime
            .list_mcp(McpListParams {
                thread_id: Some(thread.id),
            })
            .unwrap();

        for id in ["remote", "localNoAuth"] {
            let server = listed
                .items
                .iter()
                .find(|item| item.id == id)
                .unwrap_or_else(|| panic!("missing imported user MCP server {id}"));
            assert_eq!(server.status, McpServerStatus::Disabled);
            assert_eq!(server.source, Some(McpServerSource::User));
            assert!(!server.trusted);
            assert!(server.diagnostics.iter().any(|diag| {
                diag.code == "mcp_transport_untrusted" && diag.message.contains("authRef")
            }));
        }

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn mcp_config_imports_cli_mcp_config_as_untrusted_disabled() {
        let _env_lock = TEST_ENV_LOCK.lock().unwrap();
        let root = temp_project_root("mcp-cli-config");
        let cli_config = root.join("cli-mcp.json");
        fs::write(
            &cli_config,
            json!({
                "mcpServers": {
                    "cli-docs": {
                        "name": "CLI docs",
                        "command": "node",
                        "args": ["mcp-docs.js"],
                        "whenToUse": "when CLI-provided docs context is needed"
                    }
                }
            })
            .to_string(),
        )
        .unwrap();
        let _cli_mcp = EnvVarGuard::set("OPPI_MCP_CONFIG", &cli_config);

        let mut runtime = Runtime::new();
        let thread = thread_with_cwd(&mut runtime, &root);
        let listed = runtime
            .list_mcp(McpListParams {
                thread_id: Some(thread.id),
            })
            .unwrap();
        let server = listed
            .items
            .iter()
            .find(|item| item.id == "cli-docs")
            .expect("CLI MCP config server should be listed");

        assert_eq!(server.status, McpServerStatus::Disabled);
        assert_eq!(server.source, Some(McpServerSource::Cli));
        assert_eq!(server.name, "CLI docs");
        assert_eq!(
            server.when_to_use.as_deref(),
            Some("when CLI-provided docs context is needed")
        );
        assert_eq!(
            server.transport,
            Some(McpTransportConfig::Stdio {
                command: "node".to_string(),
                args: vec!["mcp-docs.js".to_string()],
                env: BTreeMap::new(),
                cwd: Some(root.display().to_string()),
            })
        );
        assert!(!server.trusted);
        assert!(server.diagnostics.iter().any(|diag| {
            diag.code == "mcp_requires_trust" && diag.message.contains("disabled until trusted")
        }));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn mcp_config_reports_plugin_manifest_servers_as_metadata_only() {
        let _env_lock = TEST_ENV_LOCK.lock().unwrap();
        let root = temp_project_root("mcp-plugin-config");
        let plugin = root.join("plugin-pack");
        fs::create_dir_all(root.join(".oppi")).unwrap();
        fs::create_dir_all(&plugin).unwrap();
        fs::write(
            plugin.join("package.json"),
            r#"{
  "name": "plugin-pack",
  "version": "1.0.0",
  "mcpServers": {
    "pluginFs": {
      "command": "node",
      "args": ["server.js"]
    }
  }
}"#,
        )
        .unwrap();
        fs::write(
            root.join(".oppi/plugins.json"),
            format!(
                r#"{{
  "version": 1,
  "plugins": [
    {{
      "name": "plugin-pack",
      "source": {},
      "sourceType": "local",
      "enabled": true,
      "trusted": true
    }}
  ]
}}"#,
                serde_json::to_string(&plugin.display().to_string()).unwrap()
            ),
        )
        .unwrap();

        let mut runtime = Runtime::new();
        let thread = thread_with_cwd(&mut runtime, &root);
        let listed = runtime
            .list_mcp(McpListParams {
                thread_id: Some(thread.id),
            })
            .unwrap();
        let server = listed
            .items
            .iter()
            .find(|item| item.id == "pluginFs")
            .expect("plugin manifest MCP server should be visible");

        assert_eq!(server.status, McpServerStatus::Disabled);
        assert_eq!(server.source, Some(McpServerSource::Plugin));
        assert!(!server.trusted);
        assert!(server.diagnostics.iter().any(|diag| {
            diag.code == "mcp_plugin_metadata_only"
                && diag.message.contains("not executed by Rust native")
        }));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn plugin_mcp_server_can_be_enabled_without_enabling_plugin_runner() {
        let _env_lock = TEST_ENV_LOCK.lock().unwrap();
        let root = temp_project_root("plugin-mcp-trust");
        let plugin = root.join("plugin-pack");
        fs::create_dir_all(&plugin).unwrap();
        fs::write(
            plugin.join("package.json"),
            r#"{
  "name": "plugin-pack",
  "version": "1.0.0",
  "mcpServers": {
    "demo": {
      "command": "node",
      "args": ["server.js"]
    }
  }
}"#,
        )
        .unwrap();
        write_project_plugin_store(
            &root,
            json!({
                "version": 1,
                "plugins": [{
                    "name": "plugin-pack",
                    "source": plugin.display().to_string(),
                    "sourceType": "local",
                    "enabled": true,
                    "trusted": true
                }]
            }),
        );

        let mut runtime = Runtime::new();
        let thread = thread_with_cwd(&mut runtime, &root);
        let before = runtime
            .list_mcp(McpListParams {
                thread_id: Some(thread.id.clone()),
            })
            .unwrap();
        let server = before
            .items
            .iter()
            .find(|item| item.id == "demo")
            .expect("plugin MCP server should be listed before trust");
        assert_eq!(server.status, McpServerStatus::Disabled);
        assert!(!server.trusted);
        assert!(
            server
                .diagnostics
                .iter()
                .any(|diag| diag.code == "mcp_plugin_metadata_only")
        );

        runtime
            .trust_plugin_mcp(PluginMcpTrustParams {
                thread_id: thread.id.clone(),
                plugin_id: "plugin-pack".to_string(),
                server_id: "demo".to_string(),
                enabled: true,
                trusted: true,
            })
            .unwrap();

        let after = runtime
            .list_mcp(McpListParams {
                thread_id: Some(thread.id.clone()),
            })
            .unwrap();
        let server = after
            .items
            .iter()
            .find(|item| item.id == "demo")
            .expect("trusted plugin MCP server should remain listed");
        assert_eq!(server.status, McpServerStatus::Disconnected);
        assert!(server.trusted);
        assert!(
            server
                .diagnostics
                .iter()
                .all(|diag| diag.code != "mcp_plugin_metadata_only")
        );
        assert!(
            server
                .diagnostics
                .iter()
                .all(|diag| diag.code != "mcp_requires_trust")
        );
        runtime.mcp_mock_servers.insert(
            "demo".to_string(),
            VecDeque::from(vec![
                json!({
                    "protocolVersion": "2025-11-25",
                    "serverInfo": { "name": "plugin-demo", "version": "1.0.0" },
                    "capabilities": { "tools": {} }
                }),
                json!({
                    "tools": [{
                        "name": "inspect",
                        "description": "Inspect plugin data",
                        "inputSchema": { "type": "object", "properties": {} }
                    }]
                }),
            ]),
        );
        let connected = runtime
            .connect_mcp(McpConnectParams {
                thread_id: thread.id,
                server_id: "demo".to_string(),
            })
            .unwrap();
        assert!(connected.events.iter().any(|event| matches!(
            &event.kind,
            EventKind::McpServerRegistered { server }
                if server.id == "demo"
                    && server.status == McpServerStatus::Enabled
                    && server.trusted
                    && server.tools == vec!["inspect".to_string()]
        )));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn plugin_runner_manifest_is_reported_disabled_until_enabled() {
        let _env_lock = TEST_ENV_LOCK.lock().unwrap();
        let root = temp_project_root("plugin-runner-boundary");
        let plugin = root.join("runner-plugin");
        fs::create_dir_all(plugin.join("dist")).unwrap();
        fs::write(plugin.join("dist/runner.js"), "process.stdin.resume();\n").unwrap();
        fs::write(
            plugin.join("package.json"),
            r#"{
  "name": "runner-plugin",
  "version": "1.0.0",
  "oppi": {
    "runtime": {
      "kind": "jsonRpcSubprocess",
      "command": "node",
      "args": ["dist/runner.js"],
      "capabilities": ["commands", "tools"]
    }
  }
}"#,
        )
        .unwrap();
        write_project_plugin_store(
            &root,
            json!({
                "version": 1,
                "plugins": [{
                    "name": "runner-plugin",
                    "source": plugin.display().to_string(),
                    "sourceType": "local",
                    "enabled": true,
                    "trusted": true
                }]
            }),
        );

        let mut runtime = Runtime::new();
        let thread = thread_with_cwd(&mut runtime, &root);
        let plugin = runtime
            .list_plugins(PluginListParams {
                thread_id: Some(thread.id),
            })
            .unwrap()
            .items
            .into_iter()
            .find(|item| item.id == "runner-plugin")
            .unwrap();
        let boundary = plugin.executable_boundary.unwrap();

        assert_eq!(
            boundary.kind,
            PluginExecutableBoundaryKind::JsonRpcSubprocess
        );
        assert_eq!(boundary.status, PluginExecutableBoundaryStatus::Disabled);
        assert_eq!(boundary.command.as_deref(), Some("node"));
        assert_eq!(boundary.args, vec!["dist/runner.js".to_string()]);
        assert!(boundary.diagnostics.iter().any(|diag| {
            diag.code == "plugin_runner_requires_enablement"
                && diag.message.contains("disabled until explicitly enabled")
        }));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn enabled_mock_plugin_runner_discovers_tools_but_does_not_bypass_guardian() {
        let _env_lock = TEST_ENV_LOCK.lock().unwrap();
        let root = temp_project_root("plugin-runner-tools");
        let plugin = root.join("runner-plugin");
        fs::create_dir_all(plugin.join("dist")).unwrap();
        fs::write(plugin.join("dist/runner.js"), "process.stdin.resume();\n").unwrap();
        fs::write(
            plugin.join("package.json"),
            r#"{
  "name": "runner-plugin",
  "version": "1.0.0",
  "oppi": {
    "runtime": {
      "kind": "jsonRpcSubprocess",
      "command": "node",
      "args": ["dist/runner.js"],
      "capabilities": ["tools"]
    }
  }
}"#,
        )
        .unwrap();
        write_project_plugin_store(
            &root,
            json!({
                "version": 1,
                "plugins": [{
                    "name": "runner-plugin",
                    "source": plugin.display().to_string(),
                    "sourceType": "local",
                    "enabled": true,
                    "trusted": true
                }]
            }),
        );
        write_project_plugin_runner_store(
            &root,
            json!({
                "version": 1,
                "runners": [{
                    "pluginId": "runner-plugin",
                    "enabled": true,
                    "trusted": true,
                    "updatedAt": "2026-05-26T12:00:00Z"
                }]
            }),
        );

        let mut runtime = Runtime::new();
        let thread = thread_with_cwd(&mut runtime, &root);
        runtime.register_mock_plugin_runner(
            "runner-plugin",
            mock_plugin_runner_sequence(vec![
                plugin_runner_initialize_result(vec!["tools"]),
                plugin_runner_tools_list_result(vec![plugin_runner_tool(
                    "danger_write",
                    json!({
                        "type": "object",
                        "properties": { "path": { "type": "string" } },
                        "required": ["path"]
                    }),
                    false,
                )]),
            ]),
        );

        let refs = runtime
            .plugin_runner_tool_definitions_for_thread(&thread.id)
            .unwrap();
        let tool = refs
            .iter()
            .find(|tool| tool.name == "danger_write")
            .unwrap();
        assert_eq!(tool.namespace.as_deref(), Some("plugin_runner-plugin"));
        assert!(tool.requires_approval);
        assert!(!tool.concurrency_safe);
        assert!(
            tool.capabilities
                .iter()
                .any(|capability| capability == "plugin-runner")
        );
        assert!(
            tool.capabilities
                .iter()
                .any(|capability| capability == "plugin:runner-plugin")
        );
        let review = guardian_review_request_for_tool(
            tool,
            &ToolCall {
                id: "runner-call".to_string(),
                name: "danger_write".to_string(),
                namespace: tool.namespace.clone(),
                arguments: json!({ "path": "docs/out.md" }),
            },
            &root.display().to_string(),
        )
        .unwrap();
        assert!(
            review.is_some(),
            "approval-gated plugin runner tools must have a Guardian review request"
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn mcp_connect_initializes_and_ingests_tools_from_mock_transport() {
        let mut runtime = Runtime::new();
        let thread = thread(&mut runtime);
        runtime.register_mock_mcp_server(
            "fs",
            vec![
                json!({
                    "protocolVersion": "2025-11-25",
                    "serverInfo": { "name": "mock-fs", "version": "1.0.0" },
                    "capabilities": {
                        "tools": { "listChanged": true },
                        "resources": { "subscribe": true, "listChanged": true },
                        "prompts": { "listChanged": false },
                        "sampling": {},
                        "elicitation": {},
                        "tasks": {}
                    }
                }),
                json!({
                    "tools": [{
                        "name": "read_file",
                        "title": "Read file",
                        "description": "Read a file",
                        "inputSchema": {
                            "type": "object",
                            "properties": { "path": { "type": "string" } },
                            "required": ["path"]
                        },
                        "annotations": {
                            "readOnlyHint": true,
                            "destructiveHint": false
                        }
                    }]
                }),
            ],
        );

        let result = runtime
            .connect_mcp(McpConnectParams {
                thread_id: thread.id.clone(),
                server_id: "fs".to_string(),
            })
            .unwrap();
        assert!(result.events.iter().any(|event| {
            matches!(
                &event.kind,
                EventKind::McpServerRegistered { server }
                    if server.id == "fs"
                        && server.status == McpServerStatus::Enabled
                        && server.tools == vec!["read_file".to_string()]
                        && server.tool_refs.iter().any(|tool| {
                            tool.name == "read_file"
                                && tool.input_schema["properties"]["path"]["type"] == json!("string")
                                && tool.annotations.read_only_hint == Some(true)
                        })
            )
        }));

        let listed = runtime
            .list_mcp(McpListParams {
                thread_id: Some(thread.id),
            })
            .unwrap();
        let server = listed
            .items
            .iter()
            .find(|item| item.id == "fs")
            .expect("connected mock server should be listed");
        assert_eq!(server.status, McpServerStatus::Enabled);
        assert_eq!(server.tools, vec!["read_file".to_string()]);
        assert_eq!(
            server.capabilities.tools.as_ref().unwrap().list_changed,
            Some(true)
        );
        assert_eq!(
            server.capabilities.resources.as_ref().unwrap().subscribe,
            Some(true)
        );
        assert_eq!(
            server.capabilities.resources.as_ref().unwrap().list_changed,
            Some(true)
        );
        assert_eq!(
            server.capabilities.prompts.as_ref().unwrap().list_changed,
            Some(false)
        );
        assert_eq!(server.capabilities.sampling.as_ref(), Some(&json!({})));
        assert_eq!(server.capabilities.elicitation.as_ref(), Some(&json!({})));
        assert_eq!(server.capabilities.tasks.as_ref(), Some(&json!({})));
        assert!(server.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "mcp_notification_listener_deferred"
                && diagnostic.message.contains("active MCP requests")
        }));
    }

    #[test]
    fn mcp_resources_list_routes_connected_mock_server_resources() {
        let mut runtime = Runtime::new();
        let thread = thread(&mut runtime);
        runtime.register_mock_mcp_server(
            "docs",
            vec![
                json!({
                    "protocolVersion": "2025-11-25",
                    "serverInfo": { "name": "mock-docs", "version": "1.0.0" },
                    "capabilities": { "tools": {}, "resources": {} }
                }),
                json!({ "tools": [] }),
            ],
        );
        runtime
            .connect_mcp(McpConnectParams {
                thread_id: thread.id.clone(),
                server_id: "docs".to_string(),
            })
            .unwrap();
        runtime.mcp_mock_servers.insert(
            "docs".to_string(),
            VecDeque::from(vec![
                json!({
                    "protocolVersion": "2025-11-25",
                    "serverInfo": { "name": "mock-docs", "version": "1.0.0" },
                    "capabilities": { "resources": {} }
                }),
                json!({
                    "resources": [{
                        "uri": "file:///repo/README.md",
                        "name": "readme",
                        "title": "Project README",
                        "description": "Human-facing project overview",
                        "mimeType": "text/markdown",
                        "annotations": { "audience": ["assistant"] },
                        "size": 2048
                    }],
                    "nextCursor": "page-2"
                }),
            ]),
        );

        let result = runtime
            .list_mcp_resources(McpListResourcesParams {
                thread_id: thread.id,
                server_id: "docs".to_string(),
                cursor: None,
            })
            .unwrap();

        assert_eq!(result.status, ToolResultStatus::Ok);
        assert_eq!(result.next_cursor.as_deref(), Some("page-2"));
        assert_eq!(result.resources.len(), 1);
        let resource = &result.resources[0];
        assert_eq!(resource.server_id, "docs");
        assert_eq!(resource.uri, "file:///repo/README.md");
        assert_eq!(resource.name, "readme");
        assert_eq!(resource.title.as_deref(), Some("Project README"));
        assert_eq!(resource.mime_type.as_deref(), Some("text/markdown"));
        assert_eq!(resource.annotations["audience"][0], json!("assistant"));
        assert_eq!(resource.size, Some(2048));
    }

    #[test]
    fn mcp_resource_templates_list_routes_connected_mock_server_templates() {
        let mut runtime = Runtime::new();
        let thread = thread(&mut runtime);
        runtime.register_mock_mcp_server(
            "docs",
            vec![
                json!({
                    "protocolVersion": "2025-11-25",
                    "serverInfo": { "name": "mock-docs", "version": "1.0.0" },
                    "capabilities": { "tools": {}, "resources": {} }
                }),
                json!({ "tools": [] }),
            ],
        );
        runtime
            .connect_mcp(McpConnectParams {
                thread_id: thread.id.clone(),
                server_id: "docs".to_string(),
            })
            .unwrap();
        runtime.mcp_mock_servers.insert(
            "docs".to_string(),
            VecDeque::from(vec![
                json!({
                    "protocolVersion": "2025-11-25",
                    "serverInfo": { "name": "mock-docs", "version": "1.0.0" },
                    "capabilities": { "resources": {} }
                }),
                json!({
                    "resourceTemplates": [{
                        "uriTemplate": "file:///repo/{path}",
                        "name": "repo-file",
                        "title": "Repository file",
                        "description": "Read a repository file by path",
                        "mimeType": "text/plain",
                        "annotations": { "audience": ["assistant"] }
                    }],
                    "nextCursor": "page-2"
                }),
            ]),
        );

        let result = runtime
            .list_mcp_resource_templates(McpListResourceTemplatesParams {
                thread_id: thread.id,
                server_id: "docs".to_string(),
                cursor: None,
            })
            .unwrap();

        assert_eq!(result.status, ToolResultStatus::Ok);
        assert_eq!(result.next_cursor.as_deref(), Some("page-2"));
        assert_eq!(result.resource_templates.len(), 1);
        let template = &result.resource_templates[0];
        assert_eq!(template.server_id, "docs");
        assert_eq!(template.uri_template, "file:///repo/{path}");
        assert_eq!(template.name, "repo-file");
        assert_eq!(template.title.as_deref(), Some("Repository file"));
        assert_eq!(template.mime_type.as_deref(), Some("text/plain"));
        assert_eq!(template.annotations["audience"][0], json!("assistant"));
    }

    #[test]
    fn mcp_prompts_list_and_get_route_connected_mock_server_prompts() {
        let mut runtime = Runtime::new();
        let thread = thread(&mut runtime);
        runtime.register_mock_mcp_server(
            "docs",
            vec![
                json!({
                    "protocolVersion": "2025-11-25",
                    "serverInfo": { "name": "mock-docs", "version": "1.0.0" },
                    "capabilities": { "tools": {}, "prompts": {} }
                }),
                json!({ "tools": [] }),
            ],
        );
        runtime
            .connect_mcp(McpConnectParams {
                thread_id: thread.id.clone(),
                server_id: "docs".to_string(),
            })
            .unwrap();
        runtime.mcp_mock_servers.insert(
            "docs".to_string(),
            VecDeque::from(vec![
                json!({
                    "protocolVersion": "2025-11-25",
                    "serverInfo": { "name": "mock-docs", "version": "1.0.0" },
                    "capabilities": { "prompts": {} }
                }),
                json!({
                    "prompts": [{
                        "name": "review-change",
                        "title": "Review change",
                        "description": "Review a diff with a specific focus",
                        "arguments": [{
                            "name": "focus",
                            "title": "Focus",
                            "description": "Review focus",
                            "required": true
                        }]
                    }],
                    "nextCursor": "page-2"
                }),
            ]),
        );

        let listed = runtime
            .list_mcp_prompts(McpListPromptsParams {
                thread_id: thread.id.clone(),
                server_id: "docs".to_string(),
                cursor: None,
            })
            .unwrap();

        assert_eq!(listed.status, ToolResultStatus::Ok);
        assert_eq!(listed.next_cursor.as_deref(), Some("page-2"));
        assert_eq!(listed.prompts.len(), 1);
        let prompt = &listed.prompts[0];
        assert_eq!(prompt.server_id, "docs");
        assert_eq!(prompt.name, "review-change");
        assert_eq!(prompt.title.as_deref(), Some("Review change"));
        assert_eq!(prompt.arguments[0].name, "focus");
        assert_eq!(prompt.arguments[0].required, Some(true));

        runtime.mcp_mock_servers.insert(
            "docs".to_string(),
            VecDeque::from(vec![
                json!({
                    "protocolVersion": "2025-11-25",
                    "serverInfo": { "name": "mock-docs", "version": "1.0.0" },
                    "capabilities": { "prompts": {} }
                }),
                json!({
                    "description": "Review prompt",
                    "messages": [{
                        "role": "user",
                        "content": {
                            "type": "text",
                            "text": "Review this change for security."
                        }
                    }]
                }),
            ]),
        );

        let fetched = runtime
            .get_mcp_prompt(McpGetPromptParams {
                thread_id: thread.id,
                server_id: "docs".to_string(),
                name: "review-change".to_string(),
                arguments: BTreeMap::from([("focus".to_string(), "security".to_string())]),
            })
            .unwrap();

        assert_eq!(fetched.status, ToolResultStatus::Ok);
        assert_eq!(fetched.name, "review-change");
        assert_eq!(fetched.description.as_deref(), Some("Review prompt"));
        assert_eq!(fetched.messages.len(), 1);
        assert_eq!(fetched.messages[0]["role"], json!("user"));
        assert_eq!(
            fetched.messages[0]["content"]["text"],
            json!("Review this change for security.")
        );
    }

    #[test]
    fn mcp_resource_read_routes_connected_mock_server_contents() {
        let mut runtime = Runtime::new();
        let thread = thread(&mut runtime);
        runtime.register_mock_mcp_server(
            "docs",
            vec![
                json!({
                    "protocolVersion": "2025-11-25",
                    "serverInfo": { "name": "mock-docs", "version": "1.0.0" },
                    "capabilities": { "tools": {}, "resources": {} }
                }),
                json!({ "tools": [] }),
            ],
        );
        runtime
            .connect_mcp(McpConnectParams {
                thread_id: thread.id.clone(),
                server_id: "docs".to_string(),
            })
            .unwrap();
        runtime.mcp_mock_servers.insert(
            "docs".to_string(),
            VecDeque::from(vec![
                json!({
                    "protocolVersion": "2025-11-25",
                    "serverInfo": { "name": "mock-docs", "version": "1.0.0" },
                    "capabilities": { "resources": {} }
                }),
                json!({
                    "contents": [{
                        "uri": "file:///repo/README.md",
                        "mimeType": "text/markdown",
                        "text": "# README\nNative MCP resources can be read."
                    }]
                }),
            ]),
        );

        let result = runtime
            .read_mcp_resource(McpReadResourceParams {
                thread_id: thread.id,
                server_id: "docs".to_string(),
                uri: "file:///repo/README.md".to_string(),
            })
            .unwrap();

        assert_eq!(result.status, ToolResultStatus::Ok);
        assert_eq!(result.uri, "file:///repo/README.md");
        assert_eq!(result.contents.len(), 1);
        assert_eq!(result.contents[0].uri, "file:///repo/README.md");
        assert_eq!(
            result.contents[0].mime_type.as_deref(),
            Some("text/markdown")
        );
        assert_eq!(
            result.contents[0].text.as_deref(),
            Some("# README\nNative MCP resources can be read.")
        );
        assert_eq!(result.contents[0].blob, None);
    }

    #[test]
    fn mcp_resource_subscribe_is_capability_gated_and_routes_request() {
        let mut runtime = Runtime::new();
        let thread = thread(&mut runtime);
        runtime.register_mock_mcp_server(
            "docs",
            vec![
                json!({
                    "protocolVersion": "2025-11-25",
                    "serverInfo": { "name": "mock-docs", "version": "1.0.0" },
                    "capabilities": { "tools": {}, "resources": { "subscribe": true } }
                }),
                json!({ "tools": [] }),
            ],
        );
        runtime
            .connect_mcp(McpConnectParams {
                thread_id: thread.id.clone(),
                server_id: "docs".to_string(),
            })
            .unwrap();
        runtime.mcp_mock_servers.insert(
            "docs".to_string(),
            VecDeque::from(vec![
                json!({
                    "protocolVersion": "2025-11-25",
                    "serverInfo": { "name": "mock-docs", "version": "1.0.0" },
                    "capabilities": { "resources": { "subscribe": true } }
                }),
                json!({}),
            ]),
        );

        let result = runtime
            .subscribe_mcp_resource(McpResourceSubscriptionParams {
                thread_id: thread.id.clone(),
                server_id: "docs".to_string(),
                uri: "file:///repo/README.md".to_string(),
            })
            .unwrap();

        assert_eq!(result.status, ToolResultStatus::Ok);
        assert_eq!(result.uri, "file:///repo/README.md");
        assert!(result.subscribed);
        let listed = runtime
            .list_mcp(McpListParams {
                thread_id: Some(thread.id.clone()),
            })
            .unwrap();
        let server = listed
            .items
            .iter()
            .find(|item| item.id == "docs")
            .expect("connected docs server");
        assert_eq!(server.resource_subscriptions.len(), 1);
        assert_eq!(
            server.resource_subscriptions[0].uri,
            "file:///repo/README.md"
        );

        runtime.mcp_mock_servers.insert(
            "docs".to_string(),
            VecDeque::from(vec![
                json!({
                    "protocolVersion": "2025-11-25",
                    "serverInfo": { "name": "mock-docs", "version": "1.0.0" },
                    "capabilities": { "resources": { "subscribe": true } }
                }),
                json!({}),
            ]),
        );
        let result = runtime
            .unsubscribe_mcp_resource(McpResourceSubscriptionParams {
                thread_id: thread.id.clone(),
                server_id: "docs".to_string(),
                uri: "file:///repo/README.md".to_string(),
            })
            .unwrap();

        assert_eq!(result.status, ToolResultStatus::Ok);
        assert!(!result.subscribed);
        let listed = runtime
            .list_mcp(McpListParams {
                thread_id: Some(thread.id),
            })
            .unwrap();
        let server = listed
            .items
            .iter()
            .find(|item| item.id == "docs")
            .expect("connected docs server");
        assert!(server.resource_subscriptions.is_empty());
    }

    #[test]
    fn mcp_resource_subscribe_denies_without_server_capability() {
        let mut runtime = Runtime::new();
        let thread = thread(&mut runtime);
        runtime.register_mock_mcp_server(
            "docs",
            vec![
                json!({
                    "protocolVersion": "2025-11-25",
                    "serverInfo": { "name": "mock-docs", "version": "1.0.0" },
                    "capabilities": { "tools": {}, "resources": {} }
                }),
                json!({ "tools": [] }),
            ],
        );
        runtime
            .connect_mcp(McpConnectParams {
                thread_id: thread.id.clone(),
                server_id: "docs".to_string(),
            })
            .unwrap();

        let result = runtime
            .subscribe_mcp_resource(McpResourceSubscriptionParams {
                thread_id: thread.id,
                server_id: "docs".to_string(),
                uri: "file:///repo/README.md".to_string(),
            })
            .unwrap();

        assert_eq!(result.status, ToolResultStatus::Denied);
        assert!(!result.subscribed);
        assert!(
            result
                .error
                .as_deref()
                .unwrap_or_default()
                .contains("resources.subscribe")
        );
    }

    #[test]
    fn mcp_resource_update_notification_marks_active_subscription_state() {
        let mut runtime = Runtime::new();
        let thread = thread(&mut runtime);
        runtime.register_mock_mcp_server(
            "docs",
            vec![
                json!({
                    "protocolVersion": "2025-11-25",
                    "serverInfo": { "name": "mock-docs", "version": "1.0.0" },
                    "capabilities": { "tools": {}, "resources": { "subscribe": true } }
                }),
                json!({ "tools": [] }),
            ],
        );
        runtime
            .connect_mcp(McpConnectParams {
                thread_id: thread.id.clone(),
                server_id: "docs".to_string(),
            })
            .unwrap();
        runtime.mcp_mock_servers.insert(
            "docs".to_string(),
            VecDeque::from(vec![
                json!({
                    "protocolVersion": "2025-11-25",
                    "serverInfo": { "name": "mock-docs", "version": "1.0.0" },
                    "capabilities": { "resources": { "subscribe": true } }
                }),
                json!({}),
            ]),
        );
        runtime
            .subscribe_mcp_resource(McpResourceSubscriptionParams {
                thread_id: thread.id.clone(),
                server_id: "docs".to_string(),
                uri: "file:///repo/README.md".to_string(),
            })
            .unwrap();

        let result = runtime
            .mcp_resource_updated(McpResourceUpdatedParams {
                thread_id: thread.id.clone(),
                server_id: "docs".to_string(),
                uri: "file:///repo/README.md".to_string(),
                received_at: Some("2026-05-31T12:00:00Z".to_string()),
            })
            .unwrap();

        assert_eq!(result.status, ToolResultStatus::Ok);
        assert!(result.updated);
        assert_eq!(result.update_count, 1);
        let listed = runtime
            .list_mcp(McpListParams {
                thread_id: Some(thread.id.clone()),
            })
            .unwrap();
        let server = listed
            .items
            .iter()
            .find(|item| item.id == "docs")
            .expect("connected docs server");
        assert_eq!(server.resource_subscriptions.len(), 1);
        assert_eq!(server.resource_subscriptions[0].update_count, 1);
        assert_eq!(
            server.resource_subscriptions[0].last_updated_at.as_deref(),
            Some("2026-05-31T12:00:00Z")
        );
        let events = runtime
            .events_after(EventsListParams {
                thread_id: thread.id.clone(),
                after: 0,
                limit: None,
            })
            .unwrap()
            .events;
        assert!(events.iter().any(|event| match &event.kind {
            EventKind::Diagnostic { diagnostic } =>
                diagnostic.message == "MCP subscribed resource updated"
                    && diagnostic.metadata.get("serverId").map(String::as_str) == Some("docs")
                    && diagnostic.metadata.get("updateCount").map(String::as_str) == Some("1"),
            _ => false,
        }));

        let denied = runtime
            .mcp_resource_updated(McpResourceUpdatedParams {
                thread_id: thread.id,
                server_id: "docs".to_string(),
                uri: "file:///repo/NOT-SUBSCRIBED.md".to_string(),
                received_at: None,
            })
            .unwrap();

        assert_eq!(denied.status, ToolResultStatus::Denied);
        assert!(!denied.updated);
        assert_eq!(denied.update_count, 0);
        assert!(
            denied
                .error
                .as_deref()
                .unwrap_or_default()
                .contains("not actively subscribed")
        );
    }

    #[test]
    fn mcp_transport_resource_update_notification_marks_active_subscription_state() {
        let mut runtime = Runtime::new();
        let thread = thread(&mut runtime);
        runtime.register_mock_mcp_server(
            "docs",
            vec![
                json!({
                    "protocolVersion": "2025-11-25",
                    "serverInfo": { "name": "mock-docs", "version": "1.0.0" },
                    "capabilities": { "tools": {}, "resources": { "subscribe": true } }
                }),
                json!({ "tools": [] }),
            ],
        );
        runtime
            .connect_mcp(McpConnectParams {
                thread_id: thread.id.clone(),
                server_id: "docs".to_string(),
            })
            .unwrap();
        runtime.mcp_mock_servers.insert(
            "docs".to_string(),
            VecDeque::from(vec![
                json!({
                    "protocolVersion": "2025-11-25",
                    "serverInfo": { "name": "mock-docs", "version": "1.0.0" },
                    "capabilities": { "resources": { "subscribe": true } }
                }),
                json!({}),
            ]),
        );
        runtime
            .subscribe_mcp_resource(McpResourceSubscriptionParams {
                thread_id: thread.id.clone(),
                server_id: "docs".to_string(),
                uri: "file:///repo/README.md".to_string(),
            })
            .unwrap();
        runtime.mcp_mock_servers.insert(
            "docs".to_string(),
            VecDeque::from(vec![
                json!({
                    "protocolVersion": "2025-11-25",
                    "serverInfo": { "name": "mock-docs", "version": "1.0.0" },
                    "capabilities": { "resources": { "subscribe": true } }
                }),
                json!({
                    "jsonrpc": "2.0",
                    "method": "notifications/resources/updated",
                    "params": { "uri": "file:///repo/README.md" }
                }),
                json!({
                    "contents": [{
                        "uri": "file:///repo/README.md",
                        "mimeType": "text/markdown",
                        "text": "# README\nUpdated through a transport notification."
                    }]
                }),
            ]),
        );

        let read = runtime
            .read_mcp_resource(McpReadResourceParams {
                thread_id: thread.id.clone(),
                server_id: "docs".to_string(),
                uri: "file:///repo/README.md".to_string(),
            })
            .unwrap();

        assert_eq!(read.status, ToolResultStatus::Ok);
        assert_eq!(read.contents.len(), 1);
        let listed = runtime
            .list_mcp(McpListParams {
                thread_id: Some(thread.id),
            })
            .unwrap();
        let server = listed
            .items
            .iter()
            .find(|item| item.id == "docs")
            .expect("connected docs server");
        assert_eq!(server.resource_subscriptions.len(), 1);
        assert_eq!(server.resource_subscriptions[0].update_count, 1);
        assert!(server.resource_subscriptions[0].last_updated_at.is_some());
    }

    #[test]
    fn mcp_transport_list_changed_notification_emits_refresh_diagnostic() {
        let mut runtime = Runtime::new();
        let thread = thread(&mut runtime);
        runtime.register_mock_mcp_server(
            "docs",
            vec![
                json!({
                    "protocolVersion": "2025-11-25",
                    "serverInfo": { "name": "mock-docs", "version": "1.0.0" },
                    "capabilities": { "tools": {}, "resources": { "listChanged": true } }
                }),
                json!({ "tools": [] }),
            ],
        );
        runtime
            .connect_mcp(McpConnectParams {
                thread_id: thread.id.clone(),
                server_id: "docs".to_string(),
            })
            .unwrap();
        runtime.mcp_mock_servers.insert(
            "docs".to_string(),
            VecDeque::from(vec![
                json!({
                    "protocolVersion": "2025-11-25",
                    "serverInfo": { "name": "mock-docs", "version": "1.0.0" },
                    "capabilities": { "resources": { "listChanged": true } }
                }),
                json!({
                    "jsonrpc": "2.0",
                    "method": "notifications/resources/list_changed"
                }),
                json!({
                    "resources": [{
                        "uri": "file:///repo/README.md",
                        "name": "README.md",
                        "mimeType": "text/markdown"
                    }]
                }),
            ]),
        );

        let listed_resources = runtime
            .list_mcp_resources(McpListResourcesParams {
                thread_id: thread.id.clone(),
                server_id: "docs".to_string(),
                cursor: None,
            })
            .unwrap();

        assert_eq!(listed_resources.status, ToolResultStatus::Ok);
        assert_eq!(listed_resources.resources.len(), 1);
        let listed = runtime
            .list_mcp(McpListParams {
                thread_id: Some(thread.id.clone()),
            })
            .unwrap();
        let server = listed
            .items
            .iter()
            .find(|item| item.id == "docs")
            .expect("connected docs server");
        assert!(server.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "mcp_resources_list_changed"
                && diagnostic.message.contains("resources list changed")
        }));
        let server_json = serde_json::to_value(server).unwrap();
        assert_eq!(
            server_json["listRefreshHints"]["resources"]["refreshCount"],
            json!(1)
        );
        let events = runtime
            .events_after(EventsListParams {
                thread_id: thread.id,
                after: 0,
                limit: None,
            })
            .unwrap()
            .events;
        assert!(events.iter().any(|event| match &event.kind {
            EventKind::Diagnostic { diagnostic } =>
                diagnostic.message == "MCP resources list changed"
                    && diagnostic.metadata.get("component").map(String::as_str) == Some("mcp")
                    && diagnostic.metadata.get("code").map(String::as_str)
                        == Some("mcp_resources_list_changed")
                    && diagnostic.metadata.get("serverId").map(String::as_str) == Some("docs")
                    && diagnostic.metadata.get("listKind").map(String::as_str) == Some("resources"),
            _ => false,
        }));
    }

    #[test]
    fn mcp_successful_resource_list_refresh_clears_resource_refresh_hint() {
        let mut runtime = Runtime::new();
        let thread = thread(&mut runtime);
        runtime.register_mock_mcp_server(
            "docs",
            vec![
                json!({
                    "protocolVersion": "2025-11-25",
                    "serverInfo": { "name": "mock-docs", "version": "1.0.0" },
                    "capabilities": {
                        "tools": {},
                        "resources": { "listChanged": true }
                    }
                }),
                json!({
                    "tools": [{
                        "name": "ping",
                        "description": "Ping docs",
                        "inputSchema": { "type": "object" },
                        "annotations": { "readOnlyHint": true }
                    }]
                }),
            ],
        );
        runtime
            .connect_mcp(McpConnectParams {
                thread_id: thread.id.clone(),
                server_id: "docs".to_string(),
            })
            .unwrap();
        runtime.mcp_mock_servers.insert(
            "docs".to_string(),
            VecDeque::from(vec![
                json!({
                    "protocolVersion": "2025-11-25",
                    "serverInfo": { "name": "mock-docs", "version": "1.0.0" },
                    "capabilities": { "tools": {}, "resources": { "listChanged": true } }
                }),
                json!({
                    "jsonrpc": "2.0",
                    "method": "notifications/resources/list_changed"
                }),
                json!({
                    "content": [{ "type": "text", "text": "pong" }]
                }),
            ]),
        );

        let call_result = runtime
            .call_mcp_tool(McpCallToolParams {
                thread_id: thread.id.clone(),
                server_id: "docs".to_string(),
                tool_name: "ping".to_string(),
                arguments: json!({}),
            })
            .unwrap();
        assert_eq!(call_result.status, ToolResultStatus::Ok);
        let listed = runtime
            .list_mcp(McpListParams {
                thread_id: Some(thread.id.clone()),
            })
            .unwrap();
        let server = listed
            .items
            .iter()
            .find(|item| item.id == "docs")
            .expect("connected docs server");
        assert_eq!(
            server
                .list_refresh_hints
                .resources
                .as_ref()
                .map(|hint| hint.refresh_count),
            Some(1)
        );

        runtime.mcp_mock_servers.insert(
            "docs".to_string(),
            VecDeque::from(vec![
                json!({
                    "protocolVersion": "2025-11-25",
                    "serverInfo": { "name": "mock-docs", "version": "1.0.0" },
                    "capabilities": { "resources": { "listChanged": true } }
                }),
                json!({
                    "resources": [{
                        "uri": "file:///repo/README.md",
                        "name": "README.md",
                        "mimeType": "text/markdown"
                    }]
                }),
            ]),
        );

        let listed_resources = runtime
            .list_mcp_resources(McpListResourcesParams {
                thread_id: thread.id.clone(),
                server_id: "docs".to_string(),
                cursor: None,
            })
            .unwrap();

        assert_eq!(listed_resources.status, ToolResultStatus::Ok);
        assert_eq!(listed_resources.resources.len(), 1);
        let listed = runtime
            .list_mcp(McpListParams {
                thread_id: Some(thread.id.clone()),
            })
            .unwrap();
        let server = listed
            .items
            .iter()
            .find(|item| item.id == "docs")
            .expect("connected docs server");
        assert_eq!(server.list_refresh_hints.resources, None);
        assert!(
            !server
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "mcp_resources_list_changed")
        );
    }

    #[test]
    fn mcp_successful_prompt_list_refresh_clears_prompt_refresh_hint() {
        let mut runtime = Runtime::new();
        let thread = thread(&mut runtime);
        runtime.register_mock_mcp_server(
            "docs",
            vec![
                json!({
                    "protocolVersion": "2025-11-25",
                    "serverInfo": { "name": "mock-docs", "version": "1.0.0" },
                    "capabilities": {
                        "tools": {},
                        "prompts": { "listChanged": true }
                    }
                }),
                json!({
                    "tools": [{
                        "name": "ping",
                        "description": "Ping docs",
                        "inputSchema": { "type": "object" },
                        "annotations": { "readOnlyHint": true }
                    }]
                }),
            ],
        );
        runtime
            .connect_mcp(McpConnectParams {
                thread_id: thread.id.clone(),
                server_id: "docs".to_string(),
            })
            .unwrap();
        runtime.mcp_mock_servers.insert(
            "docs".to_string(),
            VecDeque::from(vec![
                json!({
                    "protocolVersion": "2025-11-25",
                    "serverInfo": { "name": "mock-docs", "version": "1.0.0" },
                    "capabilities": { "tools": {}, "prompts": { "listChanged": true } }
                }),
                json!({
                    "jsonrpc": "2.0",
                    "method": "notifications/prompts/list_changed"
                }),
                json!({
                    "content": [{ "type": "text", "text": "pong" }]
                }),
            ]),
        );

        let call_result = runtime
            .call_mcp_tool(McpCallToolParams {
                thread_id: thread.id.clone(),
                server_id: "docs".to_string(),
                tool_name: "ping".to_string(),
                arguments: json!({}),
            })
            .unwrap();
        assert_eq!(call_result.status, ToolResultStatus::Ok);
        let listed = runtime
            .list_mcp(McpListParams {
                thread_id: Some(thread.id.clone()),
            })
            .unwrap();
        let server = listed
            .items
            .iter()
            .find(|item| item.id == "docs")
            .expect("connected docs server");
        assert_eq!(
            server
                .list_refresh_hints
                .prompts
                .as_ref()
                .map(|hint| hint.refresh_count),
            Some(1)
        );

        runtime.mcp_mock_servers.insert(
            "docs".to_string(),
            VecDeque::from(vec![
                json!({
                    "protocolVersion": "2025-11-25",
                    "serverInfo": { "name": "mock-docs", "version": "1.0.0" },
                    "capabilities": { "prompts": { "listChanged": true } }
                }),
                json!({
                    "prompts": [{
                        "name": "review-change",
                        "title": "Review change"
                    }]
                }),
            ]),
        );

        let listed_prompts = runtime
            .list_mcp_prompts(McpListPromptsParams {
                thread_id: thread.id.clone(),
                server_id: "docs".to_string(),
                cursor: None,
            })
            .unwrap();

        assert_eq!(listed_prompts.status, ToolResultStatus::Ok);
        assert_eq!(listed_prompts.prompts.len(), 1);
        let listed = runtime
            .list_mcp(McpListParams {
                thread_id: Some(thread.id.clone()),
            })
            .unwrap();
        let server = listed
            .items
            .iter()
            .find(|item| item.id == "docs")
            .expect("connected docs server");
        assert_eq!(server.list_refresh_hints.prompts, None);
        assert!(
            !server
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "mcp_prompts_list_changed")
        );
    }

    #[test]
    fn mcp_successful_tools_refresh_clears_tools_refresh_hint() {
        let mut runtime = Runtime::new();
        let thread = thread(&mut runtime);
        runtime.register_mock_mcp_server(
            "docs",
            vec![
                json!({
                    "protocolVersion": "2025-11-25",
                    "serverInfo": { "name": "mock-docs", "version": "1.0.0" },
                    "capabilities": { "tools": { "listChanged": true } }
                }),
                json!({
                    "tools": [{
                        "name": "ping",
                        "description": "Ping docs",
                        "inputSchema": { "type": "object" },
                        "annotations": { "readOnlyHint": true }
                    }]
                }),
            ],
        );
        runtime
            .connect_mcp(McpConnectParams {
                thread_id: thread.id.clone(),
                server_id: "docs".to_string(),
            })
            .unwrap();
        runtime.mcp_mock_servers.insert(
            "docs".to_string(),
            VecDeque::from(vec![
                json!({
                    "protocolVersion": "2025-11-25",
                    "serverInfo": { "name": "mock-docs", "version": "1.0.0" },
                    "capabilities": { "tools": { "listChanged": true } }
                }),
                json!({
                    "jsonrpc": "2.0",
                    "method": "notifications/tools/list_changed"
                }),
                json!({
                    "content": [{ "type": "text", "text": "pong" }]
                }),
            ]),
        );

        let call_result = runtime
            .call_mcp_tool(McpCallToolParams {
                thread_id: thread.id.clone(),
                server_id: "docs".to_string(),
                tool_name: "ping".to_string(),
                arguments: json!({}),
            })
            .unwrap();
        assert_eq!(call_result.status, ToolResultStatus::Ok);
        let listed = runtime
            .list_mcp(McpListParams {
                thread_id: Some(thread.id.clone()),
            })
            .unwrap();
        let server = listed
            .items
            .iter()
            .find(|item| item.id == "docs")
            .expect("connected docs server");
        assert_eq!(
            server
                .list_refresh_hints
                .tools
                .as_ref()
                .map(|hint| hint.refresh_count),
            Some(1)
        );

        runtime.mcp_mock_servers.insert(
            "docs".to_string(),
            VecDeque::from(vec![
                json!({
                    "protocolVersion": "2025-11-25",
                    "serverInfo": { "name": "mock-docs", "version": "1.0.0" },
                    "capabilities": { "tools": { "listChanged": true } }
                }),
                json!({
                    "tools": [{
                        "name": "ping",
                        "description": "Ping docs",
                        "inputSchema": { "type": "object" },
                        "annotations": { "readOnlyHint": true }
                    }]
                }),
            ]),
        );

        runtime
            .connect_mcp(McpConnectParams {
                thread_id: thread.id.clone(),
                server_id: "docs".to_string(),
            })
            .unwrap();
        let listed = runtime
            .list_mcp(McpListParams {
                thread_id: Some(thread.id.clone()),
            })
            .unwrap();
        let server = listed
            .items
            .iter()
            .find(|item| item.id == "docs")
            .expect("connected docs server");
        assert_eq!(server.list_refresh_hints.tools, None);
        assert!(
            !server
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "mcp_tools_list_changed")
        );
    }

    #[test]
    fn mcp_subscribed_resource_context_is_injected_into_direct_provider_turn() {
        let response = json!({
            "choices": [{ "message": { "role": "assistant", "content": "used subscribed docs" } }]
        })
        .to_string();
        let (base_url, server) = start_mock_openai_server(vec![response]);
        let key_name = format!("OPPI_MCP_SUBSCRIBED_CONTEXT_{}_API_KEY", std::process::id());
        unsafe { std::env::set_var(&key_name, "test-key") };
        let mut runtime = Runtime::new();
        let thread = thread(&mut runtime);
        runtime.register_mock_mcp_server(
            "docs",
            vec![
                json!({
                    "protocolVersion": "2025-11-25",
                    "serverInfo": { "name": "mock-docs", "version": "1.0.0" },
                    "capabilities": { "tools": {}, "resources": { "subscribe": true } }
                }),
                json!({ "tools": [] }),
            ],
        );
        runtime
            .connect_mcp(McpConnectParams {
                thread_id: thread.id.clone(),
                server_id: "docs".to_string(),
            })
            .unwrap();
        runtime.mcp_mock_servers.insert(
            "docs".to_string(),
            VecDeque::from(vec![
                json!({
                    "protocolVersion": "2025-11-25",
                    "serverInfo": { "name": "mock-docs", "version": "1.0.0" },
                    "capabilities": { "resources": { "subscribe": true } }
                }),
                json!({}),
            ]),
        );
        runtime
            .subscribe_mcp_resource(McpResourceSubscriptionParams {
                thread_id: thread.id.clone(),
                server_id: "docs".to_string(),
                uri: "file:///repo/README.md".to_string(),
            })
            .unwrap();
        runtime.mcp_mock_servers.insert(
            "docs".to_string(),
            VecDeque::from(vec![
                json!({
                    "protocolVersion": "2025-11-25",
                    "serverInfo": { "name": "mock-docs", "version": "1.0.0" },
                    "capabilities": { "resources": { "subscribe": true } }
                }),
                json!({
                    "contents": [{
                        "uri": "file:///repo/README.md",
                        "mimeType": "text/markdown",
                        "text": "# Subscribed README\nUse this current project guidance."
                    }]
                }),
            ]),
        );

        let result = runtime
            .run_agentic_turn(AgenticTurnParams {
                thread_id: thread.id,
                input: "Use the subscribed docs".to_string(),
                execution_mode: ExecutionMode::Blocking,
                follow_up: None,
                sandbox_policy: None,
                model_steps: Vec::new(),
                model_provider: Some(DirectModelProviderConfig {
                    kind: DirectModelProviderKind::OpenAiCompatible,
                    model: "mock".to_string(),
                    base_url: Some(base_url),
                    api_key_env: Some(key_name.clone()),
                    system_prompt: Some("base system".to_string()),
                    temperature: None,
                    reasoning_effort: None,
                    max_output_tokens: None,
                    stream: false,
                }),
                tool_definitions: Vec::new(),
                approved_tool_call_ids: Vec::new(),
                cancellation: None,
                max_continuations: Some(1),
            })
            .unwrap();
        unsafe { std::env::remove_var(&key_name) };
        let requests = server.join().unwrap();
        assert_eq!(result.turn.status, TurnStatus::Completed);
        assert!(result.events.iter().any(|event| matches!(
            &event.kind,
            EventKind::Diagnostic { diagnostic }
                if diagnostic.message == "MCP subscribed resources injected"
                    && diagnostic.metadata.get("resources").is_some_and(|value| value == "1")
        )));
        assert!(requests[0].contains("base system"));
        assert!(requests[0].contains("Subscribed MCP resource context"));
        assert!(requests[0].contains("docs file:///repo/README.md"));
        assert!(requests[0].contains("# Subscribed README"));
    }

    #[test]
    fn mcp_test_action_runs_mock_handshake_and_updates_tools() {
        let mut runtime = Runtime::new();
        let thread = thread(&mut runtime);
        runtime.register_mock_mcp_server(
            "search",
            vec![
                json!({
                    "protocolVersion": "2025-11-25",
                    "serverInfo": { "name": "mock-search" },
                    "capabilities": { "tools": {} }
                }),
                json!({
                    "tools": [{
                        "name": "query",
                        "description": "Search docs",
                        "inputSchema": {
                            "type": "object",
                            "properties": { "q": { "type": "string" } }
                        },
                        "annotations": { "readOnlyHint": true }
                    }]
                }),
            ],
        );

        let result = runtime
            .mcp_action(McpActionParams {
                thread_id: thread.id.clone(),
                server_id: "search".to_string(),
                action: McpAction::Test,
            })
            .unwrap();
        assert!(result.events.iter().any(|event| {
            matches!(
                &event.kind,
                EventKind::McpServerRegistered { server }
                    if server.id == "search" && server.tools == vec!["query".to_string()]
            )
        }));
    }

    #[test]
    fn mcp_connect_records_failed_status_for_malformed_tools_list() {
        let mut runtime = Runtime::new();
        let thread = thread(&mut runtime);
        runtime.register_mock_mcp_server(
            "broken",
            vec![
                json!({
                    "protocolVersion": "2025-11-25",
                    "serverInfo": { "name": "broken" },
                    "capabilities": { "tools": {} }
                }),
                json!({ "notTools": [] }),
            ],
        );

        let result = runtime
            .connect_mcp(McpConnectParams {
                thread_id: thread.id,
                server_id: "broken".to_string(),
            })
            .unwrap();

        assert!(result.events.iter().any(|event| {
            matches!(
                &event.kind,
                EventKind::McpServerRegistered { server }
                    if server.id == "broken" && server.status == McpServerStatus::Failed
            )
        }));
        assert!(result.events.iter().any(|event| {
            matches!(
                &event.kind,
                EventKind::Diagnostic { diagnostic }
                    if diagnostic.metadata.get("code").map(String::as_str)
                        == Some("mcp_tools_list_malformed")
            )
        }));
    }

    #[test]
    fn mcp_tool_schema_injected_into_turn_registry_and_provider_defs() {
        let mut runtime = Runtime::new();
        let thread = thread(&mut runtime);
        runtime.register_mock_mcp_server(
            "fs",
            vec![
                json!({
                    "protocolVersion": "2025-11-25",
                    "serverInfo": { "name": "mock-fs" },
                    "capabilities": { "tools": {} }
                }),
                json!({
                    "tools": [{
                        "name": "read-file",
                        "title": "Read file",
                        "description": "Read a file through MCP",
                        "inputSchema": {
                            "type": "object",
                            "properties": { "path": { "type": "string" } },
                            "required": ["path"],
                            "additionalProperties": false
                        },
                        "annotations": {
                            "readOnlyHint": true,
                            "idempotentHint": true
                        }
                    }]
                }),
            ],
        );
        runtime
            .connect_mcp(McpConnectParams {
                thread_id: thread.id,
                server_id: "fs".to_string(),
            })
            .unwrap();

        let registry = runtime.turn_tool_registry(Vec::new());
        let definition = registry
            .get(Some("mcp_fs"), "read-file")
            .expect("connected MCP tool should be model-visible");
        assert_eq!(definition.capabilities, vec!["mcp", "mcp:fs"]);
        assert!(definition.concurrency_safe);
        assert!(!definition.requires_approval);
        assert_eq!(
            definition.input_schema.as_ref().unwrap()["properties"]["path"]["type"],
            json!("string")
        );

        let openai = openai_tool_definition("mcp_fs__read-file", definition);
        assert_eq!(
            openai["function"]["parameters"]["properties"]["path"]["type"],
            json!("string")
        );
        assert_eq!(
            openai["function"]["parameters"]["additionalProperties"],
            json!(false)
        );

        let codex = codex_tool_definition("mcp_fs__read-file", definition);
        assert_eq!(
            codex["parameters"]["properties"]["path"]["type"],
            json!("string")
        );
        assert_eq!(codex["parameters"]["additionalProperties"], json!(false));
    }

    #[test]
    fn mcp_tool_call_routes_to_tools_call_and_returns_content() {
        let mut runtime = Runtime::new();
        let thread = thread(&mut runtime);
        runtime.register_mock_mcp_server(
            "fs",
            vec![
                json!({
                    "protocolVersion": "2025-11-25",
                    "serverInfo": { "name": "mock-fs" },
                    "capabilities": { "tools": {} }
                }),
                json!({
                    "tools": [{
                        "name": "read_file",
                        "description": "Read a file through MCP",
                        "inputSchema": {
                            "type": "object",
                            "properties": { "path": { "type": "string" } },
                            "required": ["path"]
                        },
                        "annotations": { "readOnlyHint": true }
                    }]
                }),
            ],
        );
        runtime
            .connect_mcp(McpConnectParams {
                thread_id: thread.id.clone(),
                server_id: "fs".to_string(),
            })
            .unwrap();
        runtime.mcp_mock_servers.insert(
            "fs".to_string(),
            VecDeque::from(vec![
                json!({
                    "protocolVersion": "2025-11-25",
                    "serverInfo": { "name": "mock-fs" },
                    "capabilities": { "tools": {} }
                }),
                json!({
                    "content": [{ "type": "text", "text": "README contents from MCP" }]
                }),
            ]),
        );

        let result = runtime
            .run_agentic_turn(AgenticTurnParams {
                thread_id: thread.id,
                input: "read README through MCP".to_string(),
                execution_mode: ExecutionMode::Blocking,
                follow_up: None,
                sandbox_policy: None,
                model_steps: vec![ScriptedModelStep {
                    assistant_deltas: Vec::new(),
                    tool_calls: vec![ToolCall {
                        id: "mcp-read".to_string(),
                        namespace: Some("mcp_fs".to_string()),
                        name: "read_file".to_string(),
                        arguments: json!({ "path": "README.md" }),
                    }],
                    tool_results: Vec::new(),
                    final_response: false,
                }],
                model_provider: None,
                tool_definitions: Vec::new(),
                approved_tool_call_ids: Vec::new(),
                cancellation: None,
                max_continuations: Some(1),
            })
            .unwrap();

        let output = result.events.iter().find_map(|event| match &event.kind {
            EventKind::ToolCallCompleted { result } if result.call_id == "mcp-read" => {
                result.output.as_deref()
            }
            _ => None,
        });
        assert!(
            output.is_some_and(|text| text.contains("README contents from MCP")),
            "MCP tools/call output should be observed by the model"
        );
    }

    #[test]
    fn mcp_streamable_http_connects_and_calls_with_auth_and_session() {
        let _env_lock = TEST_ENV_LOCK.lock().unwrap();
        let _token = EnvVarGuard::set("OPPI_MCP_HTTP_TEST_TOKEN", "test-token");
        let (endpoint, server) = start_mock_mcp_http_server(vec![
            MockMcpHttpResponse::json(json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": {
                    "protocolVersion": "2025-06-18",
                    "serverInfo": { "name": "mock-http" },
                    "capabilities": { "tools": {} }
                }
            }))
            .with_header("Mcp-Session-Id", "sess-123"),
            MockMcpHttpResponse::accepted(),
            MockMcpHttpResponse::json(json!({
                "jsonrpc": "2.0",
                "id": 2,
                "result": {
                    "tools": [{
                        "name": "read_file",
                        "description": "Read a file through Streamable HTTP MCP",
                        "inputSchema": {
                            "type": "object",
                            "properties": { "path": { "type": "string" } },
                            "required": ["path"]
                        },
                        "annotations": { "readOnlyHint": true }
                    }]
                }
            })),
            MockMcpHttpResponse::json(json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": {
                    "content": [{ "type": "text", "text": "README via HTTP MCP" }]
                }
            })),
        ]);
        let mut runtime = Runtime::new();
        let root = temp_project_root("mcp-http-streamable");
        let thread = thread_with_cwd(&mut runtime, &root);
        runtime
            .register_mcp(
                &thread.id,
                McpServerRef {
                    id: "http".to_string(),
                    name: "HTTP MCP".to_string(),
                    status: McpServerStatus::Enabled,
                    source: Some(McpServerSource::Runtime),
                    transport: Some(McpTransportConfig::StreamableHttp {
                        endpoint: endpoint.clone(),
                        auth_ref: Some("env:OPPI_MCP_HTTP_TEST_TOKEN".to_string()),
                    }),
                    trusted: true,
                    description: Some("Streamable HTTP MCP server".to_string()),
                    tools: Vec::new(),
                    tool_refs: Vec::new(),
                    resource_subscriptions: Vec::new(),
                    when_to_use: None,
                    capabilities: McpServerCapabilities::default(),
                    list_refresh_hints: McpListRefreshHints::default(),
                    diagnostics: Vec::new(),
                },
            )
            .unwrap();

        runtime
            .connect_mcp(McpConnectParams {
                thread_id: thread.id.clone(),
                server_id: "http".to_string(),
            })
            .unwrap();
        let mut policy = default_policy(PermissionProfile {
            mode: PermissionMode::Default,
            readable_roots: vec![root.display().to_string()],
            writable_roots: vec![root.display().to_string()],
            filesystem_rules: Vec::new(),
            protected_patterns: Vec::new(),
        });
        policy.network = NetworkPolicy::Enabled;
        let result = runtime
            .run_agentic_turn(AgenticTurnParams {
                thread_id: thread.id,
                input: "read README through HTTP MCP".to_string(),
                execution_mode: ExecutionMode::Blocking,
                follow_up: None,
                sandbox_policy: Some(policy),
                model_steps: vec![ScriptedModelStep {
                    assistant_deltas: Vec::new(),
                    tool_calls: vec![ToolCall {
                        id: "mcp-http-read".to_string(),
                        namespace: Some("mcp_http".to_string()),
                        name: "read_file".to_string(),
                        arguments: json!({ "path": "README.md" }),
                    }],
                    tool_results: Vec::new(),
                    final_response: false,
                }],
                model_provider: None,
                tool_definitions: Vec::new(),
                approved_tool_call_ids: Vec::new(),
                cancellation: None,
                max_continuations: Some(1),
            })
            .unwrap();

        assert!(result.events.iter().any(|event| matches!(
            &event.kind,
            EventKind::ToolCallCompleted { result }
                if result.call_id == "mcp-http-read"
                    && result.output.as_deref().is_some_and(|output| output.contains("README via HTTP MCP"))
        )));
        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 4);
        let requests = requests
            .iter()
            .map(|request| request.to_ascii_lowercase())
            .collect::<Vec<_>>();
        assert!(requests.iter().all(|request| {
            request.starts_with("post /mcp http/1.1")
                && request.contains("authorization: bearer test-token")
                && request.contains("accept: application/json, text/event-stream")
        }));
        assert!(requests[0].contains("\"method\":\"initialize\""));
        assert!(requests[1].contains("\"method\":\"notifications/initialized\""));
        assert!(requests[2].contains("\"method\":\"tools/list\""));
        assert!(requests[3].contains("\"method\":\"tools/call\""));
        for request in &requests[1..] {
            assert!(request.contains("mcp-session-id: sess-123"));
            assert!(request.contains("mcp-protocol-version: 2025-06-18"));
        }
    }

    #[test]
    fn mcp_streamable_http_sse_response_returns_matching_jsonrpc_result() {
        let result = mcp_http_sse_response_result(
            "tools/call",
            7,
            "event: message\n\
             data: {\"jsonrpc\":\"2.0\",\"id\":6,\"result\":{\"ignored\":true}}\n\n\
             event: message\n\
             data: {\"jsonrpc\":\"2.0\",\"id\":7,\"result\":{\"content\":[{\"type\":\"text\",\"text\":\"from sse\"}]}}\n\n",
        )
        .unwrap();

        assert_eq!(result["content"][0]["text"].as_str(), Some("from sse"));
    }

    #[test]
    fn mcp_streamable_http_sse_response_collects_resource_update_notifications() {
        let (result, notifications) = mcp_http_sse_response_result_with_notifications(
            "resources/read",
            7,
            "event: message\n\
             data: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/resources/updated\",\"params\":{\"uri\":\"file:///repo/README.md\"}}\n\n\
             event: message\n\
             data: {\"jsonrpc\":\"2.0\",\"id\":7,\"result\":{\"contents\":[{\"uri\":\"file:///repo/README.md\",\"text\":\"updated\"}]}}\n\n",
        )
        .unwrap();

        assert_eq!(result["contents"][0]["text"].as_str(), Some("updated"));
        assert_eq!(notifications.len(), 1);
        assert_eq!(
            notifications[0]["method"].as_str(),
            Some("notifications/resources/updated")
        );
        assert_eq!(
            notifications[0]["params"]["uri"].as_str(),
            Some("file:///repo/README.md")
        );
    }

    #[test]
    fn mcp_streamable_http_tool_obeys_turn_network_policy() {
        let _env_lock = TEST_ENV_LOCK.lock().unwrap();
        let _token = EnvVarGuard::set("OPPI_MCP_HTTP_TEST_TOKEN", "test-token");
        let root = temp_project_root("mcp-http-network-policy");
        let mut runtime = Runtime::new();
        let thread = thread_with_cwd(&mut runtime, &root);
        runtime
            .register_mcp(
                &thread.id,
                McpServerRef {
                    id: "http".to_string(),
                    name: "HTTP MCP".to_string(),
                    status: McpServerStatus::Enabled,
                    source: Some(McpServerSource::Runtime),
                    transport: Some(McpTransportConfig::StreamableHttp {
                        endpoint: "http://127.0.0.1:9/mcp".to_string(),
                        auth_ref: Some("env:OPPI_MCP_HTTP_TEST_TOKEN".to_string()),
                    }),
                    trusted: true,
                    description: Some("Streamable HTTP MCP server".to_string()),
                    tools: vec!["read_file".to_string()],
                    tool_refs: vec![McpToolRef {
                        server_id: "http".to_string(),
                        name: "read_file".to_string(),
                        title: None,
                        description: Some("Read a file through Streamable HTTP MCP".to_string()),
                        input_schema: json!({
                            "type": "object",
                            "properties": { "path": { "type": "string" } },
                            "required": ["path"]
                        }),
                        output_schema: None,
                        annotations: McpToolAnnotations {
                            read_only_hint: Some(true),
                            destructive_hint: Some(false),
                            idempotent_hint: None,
                            open_world_hint: Some(false),
                        },
                    }],
                    resource_subscriptions: Vec::new(),
                    when_to_use: None,
                    capabilities: McpServerCapabilities::default(),
                    list_refresh_hints: McpListRefreshHints::default(),
                    diagnostics: Vec::new(),
                },
            )
            .unwrap();
        let mut policy = default_policy(PermissionProfile {
            mode: PermissionMode::Default,
            readable_roots: vec![root.display().to_string()],
            writable_roots: vec![root.display().to_string()],
            filesystem_rules: Vec::new(),
            protected_patterns: Vec::new(),
        });
        policy.network = NetworkPolicy::Disabled;

        let result = runtime
            .run_agentic_turn(AgenticTurnParams {
                thread_id: thread.id,
                input: "read README through HTTP MCP".to_string(),
                execution_mode: ExecutionMode::Blocking,
                follow_up: None,
                sandbox_policy: Some(policy),
                model_steps: vec![ScriptedModelStep {
                    assistant_deltas: Vec::new(),
                    tool_calls: vec![ToolCall {
                        id: "mcp-http-read".to_string(),
                        namespace: Some("mcp_http".to_string()),
                        name: "read_file".to_string(),
                        arguments: json!({ "path": "README.md" }),
                    }],
                    tool_results: Vec::new(),
                    final_response: false,
                }],
                model_provider: None,
                tool_definitions: Vec::new(),
                approved_tool_call_ids: Vec::new(),
                cancellation: None,
                max_continuations: Some(1),
            })
            .unwrap();

        assert!(result.events.iter().any(|event| matches!(
            &event.kind,
            EventKind::ToolCallCompleted { result }
                if result.call_id == "mcp-http-read"
                    && result.status == ToolResultStatus::Denied
                    && result.error.as_deref().is_some_and(|error| error.contains("Streamable HTTP"))
        )));
    }

    #[test]
    fn mcp_open_world_tool_obeys_turn_network_policy() {
        let root = temp_project_root("mcp-network-policy");
        let mut runtime = Runtime::new();
        let thread = thread_with_cwd(&mut runtime, &root);
        runtime.register_mock_mcp_server(
            "web",
            vec![
                json!({
                    "protocolVersion": "2025-11-25",
                    "serverInfo": { "name": "mock-web" },
                    "capabilities": { "tools": {} }
                }),
                json!({
                    "tools": [{
                        "name": "fetch_url",
                        "description": "Fetch a URL through MCP",
                        "inputSchema": {
                            "type": "object",
                            "properties": { "url": { "type": "string" } },
                            "required": ["url"]
                        },
                        "annotations": {
                            "readOnlyHint": true,
                            "openWorldHint": true
                        }
                    }]
                }),
            ],
        );
        runtime
            .connect_mcp(McpConnectParams {
                thread_id: thread.id.clone(),
                server_id: "web".to_string(),
            })
            .unwrap();
        runtime.mcp_mock_servers.insert(
            "web".to_string(),
            VecDeque::from(vec![json!({
                "content": [{ "type": "text", "text": "remote contents" }]
            })]),
        );
        let mut policy = default_policy(PermissionProfile {
            mode: PermissionMode::Default,
            readable_roots: vec![root.display().to_string()],
            writable_roots: vec![root.display().to_string()],
            filesystem_rules: Vec::new(),
            protected_patterns: Vec::new(),
        });
        policy.network = NetworkPolicy::Disabled;

        let result = runtime
            .run_agentic_turn(AgenticTurnParams {
                thread_id: thread.id,
                input: "fetch through MCP".to_string(),
                execution_mode: ExecutionMode::Blocking,
                follow_up: None,
                sandbox_policy: Some(policy),
                model_steps: vec![ScriptedModelStep {
                    assistant_deltas: Vec::new(),
                    tool_calls: vec![ToolCall {
                        id: "mcp-fetch".to_string(),
                        namespace: Some("mcp_web".to_string()),
                        name: "fetch_url".to_string(),
                        arguments: json!({ "url": "https://example.com" }),
                    }],
                    tool_results: Vec::new(),
                    final_response: false,
                }],
                model_provider: None,
                tool_definitions: Vec::new(),
                approved_tool_call_ids: Vec::new(),
                cancellation: None,
                max_continuations: Some(1),
            })
            .unwrap();

        assert!(result.events.iter().any(|event| {
            matches!(
                &event.kind,
                EventKind::ToolCallCompleted { result }
                    if result.call_id == "mcp-fetch"
                        && result.status == ToolResultStatus::Denied
                        && result.error.as_deref().is_some_and(|error| {
                            error.contains("network policy enabled")
                        })
            )
        }));
        assert!(
            runtime
                .mcp_mock_servers
                .get("web")
                .is_some_and(|responses| responses.len() == 1),
            "network-denied MCP calls must not reach the transport"
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn mcp_write_tool_obeys_read_only_permission_mode() {
        let root = temp_project_root("mcp-read-only-policy");
        let mut runtime = Runtime::new();
        let thread = thread_with_cwd(&mut runtime, &root);
        runtime.register_mock_mcp_server(
            "shellish",
            vec![
                json!({
                    "protocolVersion": "2025-11-25",
                    "serverInfo": { "name": "mock-shellish" },
                    "capabilities": { "tools": {} }
                }),
                json!({
                    "tools": [{
                        "name": "delete_everything",
                        "description": "Delete files through MCP",
                        "inputSchema": {
                            "type": "object",
                            "properties": { "target": { "type": "string" } },
                            "required": ["target"]
                        },
                        "annotations": {
                            "readOnlyHint": false,
                            "destructiveHint": true
                        }
                    }]
                }),
            ],
        );
        runtime
            .connect_mcp(McpConnectParams {
                thread_id: thread.id.clone(),
                server_id: "shellish".to_string(),
            })
            .unwrap();
        runtime.mcp_mock_servers.insert(
            "shellish".to_string(),
            VecDeque::from(vec![json!({
                "content": [{ "type": "text", "text": "deleted" }]
            })]),
        );
        let policy = default_policy(PermissionProfile {
            mode: PermissionMode::ReadOnly,
            readable_roots: vec![root.display().to_string()],
            writable_roots: Vec::new(),
            filesystem_rules: Vec::new(),
            protected_patterns: Vec::new(),
        });

        let result = runtime
            .run_agentic_turn(AgenticTurnParams {
                thread_id: thread.id,
                input: "delete through MCP".to_string(),
                execution_mode: ExecutionMode::Blocking,
                follow_up: None,
                sandbox_policy: Some(policy),
                model_steps: vec![ScriptedModelStep {
                    assistant_deltas: Vec::new(),
                    tool_calls: vec![ToolCall {
                        id: "mcp-delete".to_string(),
                        namespace: Some("mcp_shellish".to_string()),
                        name: "delete_everything".to_string(),
                        arguments: json!({ "target": "all" }),
                    }],
                    tool_results: Vec::new(),
                    final_response: false,
                }],
                model_provider: None,
                tool_definitions: Vec::new(),
                approved_tool_call_ids: vec!["mcp-delete".to_string()],
                cancellation: None,
                max_continuations: Some(1),
            })
            .unwrap();

        assert!(result.events.iter().any(|event| {
            matches!(
                &event.kind,
                EventKind::ToolCallCompleted { result }
                    if result.call_id == "mcp-delete"
                        && result.status == ToolResultStatus::Denied
                        && result.error.as_deref().is_some_and(|error| {
                            error.contains("read-only permission profile blocks file writes")
                        })
            )
        }));
        assert!(
            runtime
                .mcp_mock_servers
                .get("shellish")
                .is_some_and(|responses| responses.len() == 1),
            "read-only denied MCP calls must not reach the transport"
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn mcp_tools_do_not_bypass_subagent_tool_allowlist() {
        let mut runtime = Runtime::new();
        let thread = thread(&mut runtime);
        runtime.register_mock_mcp_server(
            "fs",
            vec![
                json!({
                    "protocolVersion": "2025-11-25",
                    "serverInfo": { "name": "mock-fs" },
                    "capabilities": { "tools": {} }
                }),
                json!({
                    "tools": [{
                        "name": "read_file",
                        "description": "Read a file through MCP",
                        "inputSchema": {
                            "type": "object",
                            "properties": { "path": { "type": "string" } },
                            "required": ["path"]
                        },
                        "annotations": { "readOnlyHint": true }
                    }]
                }),
            ],
        );
        runtime
            .connect_mcp(McpConnectParams {
                thread_id: thread.id.clone(),
                server_id: "fs".to_string(),
            })
            .unwrap();
        runtime.mcp_mock_servers.insert(
            "fs".to_string(),
            VecDeque::from(vec![json!({
                "content": [{ "type": "text", "text": "child mcp contents" }]
            })]),
        );

        let result = runtime
            .run_agentic_turn(AgenticTurnParams {
                thread_id: thread.id,
                input: "delegate read-only child work".to_string(),
                execution_mode: ExecutionMode::Blocking,
                follow_up: None,
                sandbox_policy: None,
                model_steps: vec![ScriptedModelStep {
                    assistant_deltas: Vec::new(),
                    tool_calls: vec![agent_tool_call(
                        "agent-mcp-filter",
                        json!({
                            "agentName": "general-purpose",
                            "task": "try the MCP read tool",
                            "toolAllowlist": ["read"],
                            "modelSteps": [{
                                "toolCalls": [{
                                    "id": "child-mcp-read",
                                    "namespace": "mcp_fs",
                                    "name": "read_file",
                                    "arguments": { "path": "README.md" }
                                }],
                                "finalResponse": true
                            }]
                        }),
                    )],
                    tool_results: Vec::new(),
                    final_response: true,
                }],
                model_provider: None,
                tool_definitions: Vec::new(),
                approved_tool_call_ids: Vec::new(),
                cancellation: None,
                max_continuations: Some(3),
            })
            .unwrap();

        assert!(result.events.iter().any(|event| {
            matches!(
                &event.kind,
                EventKind::Diagnostic { diagnostic }
                    if diagnostic.message == "native subagent execution policy applied"
                        && diagnostic.metadata.get("toolCount").is_some_and(|count| count != "0")
            )
        }));
        assert!(result.events.iter().any(|event| {
            matches!(
                &event.kind,
                EventKind::ToolCallCompleted { result }
                    if result.call_id == "child-mcp-read"
                        && result.status == ToolResultStatus::Error
                        && result.error.as_deref().is_some_and(|error| {
                            error.contains("tool not registered")
                        })
            )
        }));
        assert!(
            runtime
                .mcp_mock_servers
                .get("fs")
                .is_some_and(|responses| responses.len() == 1),
            "subagent-filtered MCP calls must not reach the transport"
        );
    }

    #[test]
    fn mcp_destructive_tool_requires_guardian_review() {
        let root = temp_project_root("mcp-destructive-review");
        let mut runtime = Runtime::new();
        let thread = thread_with_cwd(&mut runtime, &root);
        runtime.register_mock_mcp_server(
            "shellish",
            vec![
                json!({
                    "protocolVersion": "2025-11-25",
                    "serverInfo": { "name": "mock-shellish" },
                    "capabilities": { "tools": {} }
                }),
                json!({
                    "tools": [{
                        "name": "delete_everything",
                        "description": "Delete files through MCP",
                        "inputSchema": {
                            "type": "object",
                            "properties": { "target": { "type": "string" } },
                            "required": ["target"]
                        },
                        "annotations": {
                            "readOnlyHint": false,
                            "destructiveHint": true
                        }
                    }]
                }),
            ],
        );
        runtime
            .connect_mcp(McpConnectParams {
                thread_id: thread.id.clone(),
                server_id: "shellish".to_string(),
            })
            .unwrap();

        let result = runtime
            .run_agentic_turn(AgenticTurnParams {
                thread_id: thread.id,
                input: "delete via MCP".to_string(),
                execution_mode: ExecutionMode::Blocking,
                follow_up: None,
                sandbox_policy: Some(guardian_auto_review_policy(&root)),
                model_steps: vec![ScriptedModelStep {
                    assistant_deltas: Vec::new(),
                    tool_calls: vec![ToolCall {
                        id: "mcp-delete".to_string(),
                        namespace: Some("mcp_shellish".to_string()),
                        name: "delete_everything".to_string(),
                        arguments: json!({ "target": "all" }),
                    }],
                    tool_results: Vec::new(),
                    final_response: false,
                }],
                model_provider: None,
                tool_definitions: Vec::new(),
                approved_tool_call_ids: Vec::new(),
                cancellation: None,
                max_continuations: Some(1),
            })
            .unwrap();

        assert!(result.events.iter().any(|event| {
            matches!(
                &event.kind,
                EventKind::PermissionReviewCompleted { record }
                    if record.tool_call_id == "mcp-delete"
            )
        }));
        assert!(result.events.iter().any(|event| {
            matches!(
                &event.kind,
                EventKind::ToolCallCompleted { result }
                    if result.call_id == "mcp-delete" && result.status == ToolResultStatus::Denied
            )
        }));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn mcp_tool_call_validates_required_schema_fields_before_transport() {
        let mut runtime = Runtime::new();
        let thread = thread(&mut runtime);
        runtime.register_mock_mcp_server(
            "fs",
            vec![
                json!({
                    "protocolVersion": "2025-11-25",
                    "serverInfo": { "name": "mock-fs" },
                    "capabilities": { "tools": {} }
                }),
                json!({
                    "tools": [{
                        "name": "read_file",
                        "description": "Read a file through MCP",
                        "inputSchema": {
                            "type": "object",
                            "properties": { "path": { "type": "string" } },
                            "required": ["path"]
                        },
                        "annotations": { "readOnlyHint": true }
                    }]
                }),
            ],
        );
        runtime
            .connect_mcp(McpConnectParams {
                thread_id: thread.id.clone(),
                server_id: "fs".to_string(),
            })
            .unwrap();

        let result = runtime
            .run_agentic_turn(AgenticTurnParams {
                thread_id: thread.id,
                input: "read README through MCP".to_string(),
                execution_mode: ExecutionMode::Blocking,
                follow_up: None,
                sandbox_policy: None,
                model_steps: vec![ScriptedModelStep {
                    assistant_deltas: Vec::new(),
                    tool_calls: vec![ToolCall {
                        id: "mcp-read-missing-path".to_string(),
                        namespace: Some("mcp_fs".to_string()),
                        name: "read_file".to_string(),
                        arguments: json!({}),
                    }],
                    tool_results: Vec::new(),
                    final_response: false,
                }],
                model_provider: None,
                tool_definitions: Vec::new(),
                approved_tool_call_ids: Vec::new(),
                cancellation: None,
                max_continuations: Some(1),
            })
            .unwrap();

        assert!(result.events.iter().any(|event| {
            matches!(
                &event.kind,
                EventKind::ToolCallCompleted { result }
                    if result.call_id == "mcp-read-missing-path"
                        && result.status == ToolResultStatus::Error
                        && result.error.as_deref().is_some_and(|error| {
                            error.contains("missing required field path")
                        })
            )
        }));
    }

    #[test]
    fn agent_dispatch_owns_worktree_status() {
        let mut runtime = Runtime::new();
        let thread = thread(&mut runtime);
        runtime
            .register_agent(
                &thread.id,
                AgentDefinition {
                    name: "reviewer".to_string(),
                    description: "review changes".to_string(),
                    source: Some(AgentSource::Project),
                    tools: vec!["read".to_string()],
                    tool_denylist: Vec::new(),
                    model: None,
                    effort: Some("medium".to_string()),
                    permission_mode: Some(PermissionMode::AutoReview),
                    memory_mode: None,
                    background: true,
                    worktree_root: Some("/repo-wt".to_string()),
                    isolation: None,
                    color: None,
                    skills: Vec::new(),
                    max_turns: None,
                    path: None,
                    filename: None,
                    read_only: false,
                    instructions: "Review carefully.".to_string(),
                },
            )
            .unwrap();
        let run = runtime
            .dispatch_agent(AgentDispatchParams {
                thread_id: thread.id.clone(),
                agent_name: "reviewer".to_string(),
                task: "audit".to_string(),
                worktree_root: None,
                background: true,
                role: Some("subagent".to_string()),
                model: Some("gpt-sub".to_string()),
                effort: Some("medium".to_string()),
                permission_mode: None,
                memory_mode: Some("disabled".to_string()),
                tool_allowlist: Vec::new(),
                tool_denylist: vec!["shell_exec".to_string()],
                isolation: Some("thread".to_string()),
                color: Some("cyan".to_string()),
                skills: vec!["independent".to_string()],
                max_turns: Some(2),
            })
            .unwrap()
            .run;
        assert_eq!(run.worktree_root.as_deref(), Some("/repo-wt"));
        assert!(run.background);
        assert_eq!(run.role.as_deref(), Some("subagent"));
        assert_eq!(run.model.as_deref(), Some("gpt-sub"));
        assert_eq!(run.effort.as_deref(), Some("medium"));
        assert_eq!(run.permission_mode, Some(PermissionMode::AutoReview));
        assert_eq!(run.memory_mode.as_deref(), Some("disabled"));
        assert_eq!(run.tool_allowlist, vec!["read".to_string()]);
        assert_eq!(run.tool_denylist, vec!["shell_exec".to_string()]);
        assert_eq!(run.isolation.as_deref(), Some("thread"));
        assert_eq!(run.color.as_deref(), Some("cyan"));
        assert_eq!(run.skills, vec!["independent".to_string()]);
        assert_eq!(run.max_turns, Some(2));
        let blocked = runtime
            .block_agent(AgentBlockParams {
                thread_id: thread.id,
                run_id: run.id,
                reason: "needs approval".to_string(),
            })
            .unwrap();
        assert!(
            blocked
                .events
                .iter()
                .any(|event| matches!(event.kind, EventKind::AgentBlocked { .. }))
        );
    }
}
