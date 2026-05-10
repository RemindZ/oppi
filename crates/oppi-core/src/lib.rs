//! In-memory runtime core for the first OPPi Rust-spine slice.
//!
//! This crate deliberately starts as a deterministic runtime skeleton. It owns
//! Thread/Turn/Item/Event state and emits the semantic phases that future model,
//! tool, approval, sandbox, and UI adapters consume.

pub mod event_store;

mod goals;

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use event_store::{EventStore, InMemoryEventStore};
use goals::{
    apply_goal_accounting_delta, completion_budget_report, goal_tool_output, new_goal,
    render_goal_budget_limit_prompt, render_goal_continuation_prompt, status_after_budget,
    validate_goal_budget, validate_goal_objective,
};
use oppi_agents::{built_in_agent_definitions, resolve_active_agents};
use oppi_protocol::*;
use oppi_sandbox::{
    PolicyDecision, SandboxBackgroundSpawnParams, SandboxedBackgroundProcess, default_policy,
    evaluate_exec, execute_sandboxed, spawn_sandboxed_background,
};
use oppi_tools::{
    ToolPairingTracker, ToolRegistry, describe_tool_batch, partition_ordered_tool_batches,
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const MAX_CONTINUATIONS: u32 = 8;
const DEFAULT_EVENTS_LIST_LIMIT: usize = 1_000;
const MAX_EVENTS_LIST_LIMIT: usize = 5_000;
const PROVIDER_HISTORY_MAX_MESSAGES: usize = 24;
const PROVIDER_HISTORY_MAX_CHARS: usize = 16_000;
const MERIDIAN_API_KEY_ENV: &str = "OPPI_MERIDIAN_API_KEY";
const OPENAI_CODEX_PROVIDER_ID: &str = "openai-codex";
const OPENAI_CODEX_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const OPENAI_CODEX_TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
const OPENAI_CODEX_DEFAULT_BASE_URL: &str = "https://chatgpt.com/backend-api";
const GITHUB_COPILOT_PROVIDER_ID: &str = "github-copilot";
const GITHUB_COPILOT_DEFAULT_BASE_URL: &str = "https://api.individual.githubcopilot.com";
const GITHUB_COPILOT_DEFAULT_DOMAIN: &str = "github.com";
const GPT_MAIN_DEFAULT_MODEL: &str = "gpt-5.5";
const GPT_CODING_SUBAGENT_DEFAULT_MODEL: &str = "gpt-5.3-codex";
const CLAUDE_MAIN_DEFAULT_MODEL: &str = "claude-opus-4-6";
const CLAUDE_CODING_SUBAGENT_DEFAULT_MODEL: &str = "claude-sonnet-4-6";

#[derive(Debug, Clone, PartialEq, Eq)]
enum ProviderMessageRole {
    System,
    User,
    Assistant,
}

impl ProviderMessageRole {
    fn as_str(&self) -> &'static str {
        match self {
            Self::System => "system",
            Self::User => "user",
            Self::Assistant => "assistant",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProviderHistoryMessage {
    role: ProviderMessageRole,
    content: String,
    turn_id: Option<TurnId>,
}

#[derive(Debug, Clone)]
struct DirectProviderResumeState {
    config: DirectModelProviderConfig,
    messages: Vec<Value>,
}

impl DirectProviderResumeState {
    fn replay_snapshot(&self, turn_id: &str) -> ProviderTranscriptReplaySnapshot {
        ProviderTranscriptReplaySnapshot {
            turn_id: turn_id.to_string(),
            model_provider: self.config.clone(),
            messages: self.messages.clone(),
        }
    }

    fn from_replay_snapshot(snapshot: &ProviderTranscriptReplaySnapshot) -> Self {
        Self {
            config: snapshot.model_provider.clone(),
            messages: snapshot.messages.clone(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct TurnInterruptRegistry {
    requests: Arc<Mutex<BTreeMap<TurnId, String>>>,
}

impl TurnInterruptRegistry {
    pub fn request(&self, turn_id: TurnId, reason: String) -> Result<(), String> {
        let mut requests = self
            .requests
            .lock()
            .map_err(|_| "turn interrupt registry lock poisoned".to_string())?;
        requests.insert(turn_id, reason);
        Ok(())
    }

    fn take(&self, turn_id: &str) -> Option<String> {
        self.requests
            .lock()
            .ok()
            .and_then(|mut requests| requests.remove(turn_id))
    }
}

#[derive(Debug, Clone)]
struct ModelProviderStep {
    step: ScriptedModelStep,
    diagnostics: Vec<Diagnostic>,
    known_token_delta: i64,
}

#[derive(Debug, Clone)]
struct AgenticToolExecution {
    result: ToolResult,
    side_events: Vec<Event>,
}

impl AgenticToolExecution {
    fn result(result: ToolResult) -> Self {
        Self {
            result,
            side_events: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GuardianReviewDecision {
    Ask,
    Deny,
}

#[derive(Debug, Clone)]
struct GuardianReviewResult {
    decision: GuardianReviewDecision,
    risk: RiskLevel,
    reason: String,
    strict_json: String,
}

#[derive(Debug)]
pub struct Runtime {
    next_thread: u64,
    next_turn: u64,
    next_item: u64,
    next_event: u64,
    next_approval: u64,
    next_question: u64,
    next_agent_run: u64,
    threads: BTreeMap<ThreadId, Thread>,
    turns: BTreeMap<TurnId, Turn>,
    event_store: InMemoryEventStore,
    event_mirror: Option<Arc<Mutex<Vec<Event>>>>,
    approvals: BTreeMap<ApprovalId, OwnedApprovalRequest>,
    questions: BTreeMap<QuestionId, OwnedQuestionRequest>,
    plugins: BTreeMap<PluginId, PluginRef>,
    memory: MemoryStatus,
    todos: TodoState,
    goals: BTreeMap<ThreadId, ThreadGoal>,
    goal_active_started_at_ms: BTreeMap<ThreadId, u64>,
    goal_continuations: BTreeMap<ThreadId, u32>,
    suggested_next: Option<SuggestedNextMessage>,
    mcp_servers: BTreeMap<McpServerId, McpServerRef>,
    models: BTreeMap<ModelId, ModelRef>,
    selected_model: Option<ModelId>,
    tool_registry: ToolRegistry,
    agents: BTreeMap<String, Vec<AgentDefinition>>,
    agent_runs: BTreeMap<AgentRunId, AgentRun>,
    direct_provider_turns: BTreeMap<TurnId, DirectProviderResumeState>,
    turn_sandbox_policies: BTreeMap<TurnId, SandboxPolicy>,
    turn_interrupts: TurnInterruptRegistry,
    shell_tasks: BTreeMap<String, ShellTaskRecord>,
}

#[derive(Debug)]
struct ShellTaskRecord {
    id: String,
    command: String,
    cwd: String,
    output_path: PathBuf,
    status: ShellTaskStatus,
    child: Option<SandboxedBackgroundProcess>,
    started_at_ms: u64,
    finished_at_ms: Option<u64>,
    exit_code: Option<i32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShellTaskStatus {
    Running,
    Completed,
    Failed,
    Killed,
}

impl ShellTaskStatus {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Killed => "killed",
        }
    }
}

#[derive(Debug, Clone)]
struct SkillCandidate {
    reference: SkillRef,
    content: String,
}

#[derive(Debug, Clone)]
struct ResolvedAgentToolPolicy {
    background: bool,
    role: Option<String>,
    model: Option<String>,
    effort: Option<String>,
    permission_mode: Option<PermissionMode>,
    network_policy: Option<NetworkPolicy>,
    memory_mode: Option<String>,
    tool_allowlist: Vec<String>,
    tool_denylist: Vec<String>,
    isolation: Option<String>,
    color: Option<String>,
    skills: Vec<String>,
    max_turns: Option<u32>,
}

fn background_task_status(status: ShellTaskStatus) -> BackgroundTaskStatus {
    match status {
        ShellTaskStatus::Running => BackgroundTaskStatus::Running,
        ShellTaskStatus::Completed => BackgroundTaskStatus::Completed,
        ShellTaskStatus::Failed => BackgroundTaskStatus::Failed,
        ShellTaskStatus::Killed => BackgroundTaskStatus::Killed,
    }
}

fn background_task_info(task: &ShellTaskRecord) -> BackgroundTaskInfo {
    BackgroundTaskInfo {
        id: task.id.clone(),
        command: task.command.clone(),
        cwd: task.cwd.clone(),
        output_path: task.output_path.display().to_string(),
        status: background_task_status(task.status),
        started_at_ms: Some(task.started_at_ms),
        finished_at_ms: task.finished_at_ms,
        exit_code: task.exit_code,
        output_bytes: task_output_bytes(task),
    }
}

fn task_output_bytes(task: &ShellTaskRecord) -> Option<u64> {
    fs::metadata(&task.output_path)
        .ok()
        .map(|metadata| metadata.len())
}

fn shell_task_line(task: &ShellTaskRecord) -> String {
    let bytes = task_output_bytes(task)
        .map(|bytes| format!(", {bytes}B"))
        .unwrap_or_default();
    let exit = task
        .exit_code
        .map(|code| format!(", exit={code}"))
        .unwrap_or_default();
    format!(
        "{} [{}{}{}] {} (cwd: {}) -> {}",
        task.id,
        task.status.as_str(),
        exit,
        bytes,
        task.command,
        task.cwd,
        task.output_path.display()
    )
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

#[derive(Debug, Clone)]
struct OwnedApprovalRequest {
    thread_id: ThreadId,
    turn_id: Option<TurnId>,
    tool_call_id: Option<ToolCallId>,
    tool_call: Option<ToolCall>,
    outcome: Option<ApprovalOutcome>,
}

#[derive(Debug, Clone)]
struct OwnedQuestionRequest {
    thread_id: ThreadId,
    turn_id: Option<TurnId>,
    tool_call_id: Option<ToolCallId>,
    tool_call: Option<ToolCall>,
    resolved: bool,
}

impl Default for Runtime {
    fn default() -> Self {
        Self {
            next_thread: 0,
            next_turn: 0,
            next_item: 0,
            next_event: 0,
            next_approval: 0,
            next_question: 0,
            next_agent_run: 0,
            threads: BTreeMap::new(),
            turns: BTreeMap::new(),
            event_store: InMemoryEventStore::new(),
            event_mirror: None,
            approvals: BTreeMap::new(),
            questions: BTreeMap::new(),
            plugins: BTreeMap::new(),
            memory: MemoryStatus {
                enabled: false,
                backend: "none".to_string(),
                scope: "project".to_string(),
                memory_count: 0,
            },
            todos: TodoState::default(),
            goals: BTreeMap::new(),
            goal_active_started_at_ms: BTreeMap::new(),
            goal_continuations: BTreeMap::new(),
            suggested_next: None,
            mcp_servers: BTreeMap::new(),
            models: BTreeMap::new(),
            selected_model: None,
            tool_registry: default_tool_registry(),
            agents: built_in_agent_definitions().into_iter().fold(
                BTreeMap::new(),
                |mut agents, agent| {
                    agents.entry(agent.name.clone()).or_default().push(agent);
                    agents
                },
            ),
            agent_runs: BTreeMap::new(),
            direct_provider_turns: BTreeMap::new(),
            turn_sandbox_policies: BTreeMap::new(),
            turn_interrupts: TurnInterruptRegistry::default(),
            shell_tasks: BTreeMap::new(),
        }
    }
}

fn tool_registry_from_definitions(definitions: Vec<ToolDefinition>) -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    for definition in definitions {
        registry.register(definition);
    }
    registry
}

fn default_tool_registry() -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    registry.register(ToolDefinition {
        name: "echo".to_string(),
        namespace: Some("oppi".to_string()),
        description: Some("Return the provided output argument as a tool result.".to_string()),
        concurrency_safe: true,
        requires_approval: false,
        capabilities: Vec::new(),
    });
    registry.register(ToolDefinition {
        name: "get_goal".to_string(),
        namespace: None,
        description: Some("Get the current goal for this thread, including status, budget, tokens used, elapsed time, and remaining token budget.".to_string()),
        concurrency_safe: true,
        requires_approval: false,
        capabilities: vec!["goal".to_string(), "state".to_string(), "read".to_string()],
    });
    registry.register(ToolDefinition {
        name: "create_goal".to_string(),
        namespace: None,
        description: Some("Create a goal only when explicitly requested by the user or system/developer instructions. Fails if a non-complete goal already exists.".to_string()),
        concurrency_safe: false,
        requires_approval: false,
        capabilities: vec!["goal".to_string(), "state".to_string(), "write".to_string()],
    });
    registry.register(ToolDefinition {
        name: "update_goal".to_string(),
        namespace: None,
        description: Some("Update the existing goal only to mark it complete after the objective is actually achieved; pause/resume/budget are user/runtime controls.".to_string()),
        concurrency_safe: false,
        requires_approval: false,
        capabilities: vec!["goal".to_string(), "state".to_string(), "write".to_string()],
    });
    registry.register(ToolDefinition {
        name: "todo_write".to_string(),
        namespace: Some("oppi".to_string()),
        description: Some("Create or update the current task todo list. Arguments: todos is the full current list of {id, content, status, priority?, phase?, notes?}; summary briefly explains what changed. Status values: pending, in_progress, completed, blocked, cancelled. Always send the full list, not a patch.".to_string()),
        concurrency_safe: false,
        requires_approval: false,
        capabilities: vec!["state".to_string(), "todos".to_string()],
    });
    registry.register(ToolDefinition {
        name: "ask_user".to_string(),
        namespace: Some("oppi".to_string()),
        description: Some("Ask the user one or more structured questions and pause the turn until the host supplies answers.".to_string()),
        concurrency_safe: false,
        requires_approval: false,
        capabilities: vec!["questions".to_string(), "user-input".to_string()],
    });
    registry.register(ToolDefinition {
        name: "suggest_next_message".to_string(),
        namespace: Some("oppi".to_string()),
        description: Some(
            "Offer a high-confidence ghost suggestion for the user's likely next short message."
                .to_string(),
        ),
        concurrency_safe: true,
        requires_approval: false,
        capabilities: vec!["suggestion".to_string(), "ui".to_string()],
    });
    registry.register(ToolDefinition {
        name: "oppi_feedback_submit".to_string(),
        namespace: Some("oppi".to_string()),
        description: Some("Create a sanitized OPPi bug report or feature request draft, with optional intake-worker submission only when explicitly configured at runtime.".to_string()),
        concurrency_safe: false,
        requires_approval: false,
        capabilities: vec!["feedback".to_string(), "diagnostics".to_string()],
    });
    registry.register(ToolDefinition {
        name: "render_mermaid".to_string(),
        namespace: Some("oppi".to_string()),
        description: Some("Render Mermaid diagram source as terminal-friendly ASCII using Rust's deterministic fallback renderer.".to_string()),
        concurrency_safe: true,
        requires_approval: false,
        capabilities: vec!["mermaid".to_string(), "render".to_string()],
    });
    registry.register(ToolDefinition {
        name: "image_gen".to_string(),
        namespace: Some("oppi".to_string()),
        description: Some("Generate or edit an image through an explicit host adapter or approved native image backend; missing credentials/backend fail closed.".to_string()),
        concurrency_safe: false,
        requires_approval: false,
        capabilities: vec![
            "image-generation".to_string(),
            "artifact".to_string(),
            "network".to_string(),
        ],
    });
    registry.register(ToolDefinition {
        name: "AgentTool".to_string(),
        namespace: Some("oppi".to_string()),
        description: Some("Delegate a scoped task to a registered native OPPi subagent with Rust-enforced tool, permission, model/effort, memory, isolation, skill, and max-turn policy.".to_string()),
        concurrency_safe: false,
        requires_approval: false,
        capabilities: vec!["agent".to_string(), "subagent".to_string()],
    });
    registry.register(ToolDefinition {
        name: "shell_exec".to_string(),
        namespace: Some("oppi".to_string()),
        description: Some(
            "Execute a shell command through the Rust sandbox/exec boundary.".to_string(),
        ),
        concurrency_safe: false,
        requires_approval: true,
        capabilities: vec!["process".to_string(), "sandbox-exec".to_string()],
    });
    registry.register(ToolDefinition {
        name: "shell_task".to_string(),
        namespace: Some("oppi".to_string()),
        description: Some(
            "List, read, or kill background shell_exec tasks started by the Rust runtime."
                .to_string(),
        ),
        concurrency_safe: false,
        requires_approval: false,
        capabilities: vec!["process".to_string(), "background-task".to_string()],
    });
    registry.register(ToolDefinition {
        name: "read_file".to_string(),
        namespace: Some("oppi".to_string()),
        description: Some(
            "Read a UTF-8 text file inside the project after Rust preflight policy checks."
                .to_string(),
        ),
        concurrency_safe: true,
        requires_approval: false,
        capabilities: vec!["filesystem".to_string(), "read".to_string()],
    });
    registry.register(ToolDefinition {
        name: "oppi_review_read".to_string(),
        namespace: Some("oppi".to_string()),
        description: Some(
            "Read bounded non-protected project files for auto-review/reviewer flows.".to_string(),
        ),
        concurrency_safe: true,
        requires_approval: false,
        capabilities: vec![
            "filesystem".to_string(),
            "read".to_string(),
            "review-tool".to_string(),
        ],
    });
    registry.register(ToolDefinition {
        name: "oppi_review_ls".to_string(),
        namespace: Some("oppi".to_string()),
        description: Some(
            "List a bounded project directory for auto-review/reviewer flows.".to_string(),
        ),
        concurrency_safe: true,
        requires_approval: false,
        capabilities: vec![
            "filesystem".to_string(),
            "list".to_string(),
            "review-tool".to_string(),
        ],
    });
    registry.register(ToolDefinition {
        name: "oppi_review_grep".to_string(),
        namespace: Some("oppi".to_string()),
        description: Some(
            "Search bounded non-protected project files for auto-review/reviewer flows."
                .to_string(),
        ),
        concurrency_safe: true,
        requires_approval: false,
        capabilities: vec![
            "filesystem".to_string(),
            "search".to_string(),
            "review-tool".to_string(),
        ],
    });
    registry.register(ToolDefinition {
        name: "search_files".to_string(),
        namespace: Some("oppi".to_string()),
        description: Some(
            "Search UTF-8 project files for a query after Rust preflight policy checks."
                .to_string(),
        ),
        concurrency_safe: true,
        requires_approval: false,
        capabilities: vec!["filesystem".to_string(), "search".to_string()],
    });
    registry.register(ToolDefinition {
        name: "write_file".to_string(),
        namespace: Some("oppi".to_string()),
        description: Some("Write a UTF-8 text file inside the project after approval and Rust preflight policy checks.".to_string()),
        concurrency_safe: false,
        requires_approval: true,
        capabilities: vec!["filesystem".to_string(), "write".to_string()],
    });
    registry.register(ToolDefinition {
        name: "edit_file".to_string(),
        namespace: Some("oppi".to_string()),
        description: Some(
            "Replace text in a project file after approval and Rust preflight policy checks."
                .to_string(),
        ),
        concurrency_safe: false,
        requires_approval: true,
        capabilities: vec!["filesystem".to_string(), "edit".to_string()],
    });
    registry
}

fn server_capabilities() -> Vec<String> {
    vec![
        "threads".to_string(),
        "turns".to_string(),
        "events".to_string(),
        "tools".to_string(),
        "approvals".to_string(),
        "questions".to_string(),
        "sandbox".to_string(),
        "agents".to_string(),
        "memory".to_string(),
        "todos".to_string(),
        "goals".to_string(),
        "mcp".to_string(),
        "models".to_string(),
        "direct-provider".to_string(),
        "openai-compatible-provider".to_string(),
        "openai-codex-provider".to_string(),
        "background-turns".to_string(),
        "side-questions".to_string(),
        "persistence-replay".to_string(),
        "commands".to_string(),
        "native-agent-tool".to_string(),
    ]
}

const INIT_USER_PROMPT: &str = include_str!("../../../systemprompts/commands/init-user-prompt.md");
const BUILTIN_IMAGEGEN_SKILL: &str =
    include_str!("../../../packages/pi-package/skills/imagegen/SKILL.md");
const BUILTIN_INDEPENDENT_SKILL: &str =
    include_str!("../../../packages/pi-package/skills/independent/SKILL.md");
const BUILTIN_MERMAID_SKILL: &str =
    include_str!("../../../packages/pi-package/skills/mermaid-diagrams/SKILL.md");
const BUILTIN_GRAPHIFY_SKILL: &str =
    include_str!("../../../packages/pi-package/skills/graphify/SKILL.md");
const MAX_EXISTING_AGENTS_CHARS: usize = 32_000;
const MAX_SKILL_CONTENT_CHARS: usize = 8_000;
const MAX_SKILL_INJECTION_CHARS: usize = 16_000;

fn protocol_version_is_compatible(client_version: &str) -> bool {
    let Some((client_major, client_minor)) = protocol_major_minor(client_version) else {
        return false;
    };
    let Some((server_major, server_minor)) = protocol_major_minor(OPPI_PROTOCOL_VERSION) else {
        return false;
    };
    let Some((min_major, min_minor)) = protocol_major_minor(OPPI_MIN_PROTOCOL_VERSION) else {
        return false;
    };
    client_major == server_major
        && client_minor <= server_minor
        && (client_major, client_minor) >= (min_major, min_minor)
}

fn protocol_major_minor(version: &str) -> Option<(u64, u64)> {
    let mut parts = version.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().unwrap_or("0").parse().ok()?;
    Some((major, minor))
}

#[derive(Debug, Clone)]
struct ModelRequest {
    thread_id: ThreadId,
    turn_id: TurnId,
    input: String,
    continuation: u32,
    history: Vec<ProviderHistoryMessage>,
}

trait ModelProvider {
    fn next_step(&mut self, request: &ModelRequest) -> Result<ModelProviderStep, RuntimeError>;

    fn observe_tool_result(
        &mut self,
        _call: &ToolCall,
        _result: &ToolResult,
    ) -> Result<(), RuntimeError> {
        Ok(())
    }

    fn snapshot(&self) -> Option<DirectProviderResumeState> {
        None
    }

    fn direct_config(&self) -> Option<DirectModelProviderConfig> {
        None
    }
}

struct ScriptedModelProvider {
    steps: VecDeque<ScriptedModelStep>,
}

impl ScriptedModelProvider {
    fn new(steps: Vec<ScriptedModelStep>) -> Self {
        Self {
            steps: VecDeque::from(steps),
        }
    }
}

impl ModelProvider for ScriptedModelProvider {
    fn next_step(&mut self, request: &ModelRequest) -> Result<ModelProviderStep, RuntimeError> {
        Ok(ModelProviderStep {
            step: self.steps.pop_front().unwrap_or_else(|| ScriptedModelStep {
                assistant_deltas: vec![format!(
                    "OPPi Rust loop completed {} continuation {} for {} on {}.",
                    request.turn_id, request.continuation, request.thread_id, request.input
                )],
                tool_calls: Vec::new(),
                tool_results: Vec::new(),
                final_response: true,
            }),
            diagnostics: Vec::new(),
            known_token_delta: 0,
        })
    }
}

struct OpenAiCompatibleModelProvider {
    config: DirectModelProviderConfig,
    agent: ureq::Agent,
    messages: Vec<Value>,
    tools: BTreeMap<String, ToolDefinition>,
}

impl OpenAiCompatibleModelProvider {
    fn new(config: DirectModelProviderConfig, tools: Vec<ToolDefinition>) -> Self {
        Self::with_messages(config, tools, Vec::new())
    }

    fn from_resume_state(state: DirectProviderResumeState, tools: Vec<ToolDefinition>) -> Self {
        Self::with_messages(state.config, tools, state.messages)
    }

    fn with_messages(
        config: DirectModelProviderConfig,
        tools: Vec<ToolDefinition>,
        messages: Vec<Value>,
    ) -> Self {
        Self {
            config,
            agent: ureq::AgentBuilder::new()
                .timeout(Duration::from_secs(90))
                .build(),
            messages,
            tools: tools
                .into_iter()
                .map(|definition| (openai_tool_name(&definition), definition))
                .collect(),
        }
    }

    fn endpoint(&self, copilot_auth: Option<&GitHubCopilotAuth>) -> String {
        let base = if self.config.kind == DirectModelProviderKind::GitHubCopilot {
            self.config
                .base_url
                .clone()
                .or_else(|| copilot_auth.map(github_copilot_base_url))
                .unwrap_or_else(|| GITHUB_COPILOT_DEFAULT_BASE_URL.to_string())
        } else {
            self.config
                .base_url
                .clone()
                .or_else(|| std::env::var("OPPI_OPENAI_BASE_URL").ok())
                .unwrap_or_else(|| "https://api.openai.com/v1".to_string())
        };
        let base = base.trim_end_matches('/');
        if base.ends_with("/chat/completions") {
            base.to_string()
        } else {
            format!("{base}/chat/completions")
        }
    }

    fn api_key(&self) -> Result<String, RuntimeError> {
        let candidates = provider_api_key_env_candidates(self.config.api_key_env.as_deref())?;
        for name in &candidates {
            if let Ok(value) = std::env::var(name) {
                let value = value.trim().to_string();
                if !value.is_empty() {
                    return Ok(value);
                }
            }
        }
        if provider_uses_meridian_placeholder_key(&self.config) {
            return Ok("x".to_string());
        }
        Err(RuntimeError::new(
            "provider_auth_missing",
            RuntimeErrorCategory::Provider,
            format!(
                "direct provider requires an API key in one of: {}",
                candidates.join(", ")
            ),
        ))
    }

    fn seed_messages(&mut self, request: &ModelRequest) {
        if !self.messages.is_empty() {
            return;
        }
        if let Some(system) = self
            .config
            .system_prompt
            .as_ref()
            .filter(|value| !value.is_empty())
        {
            self.messages
                .push(json!({ "role": "system", "content": system }));
        }
        for message in &request.history {
            if !message.content.trim().is_empty() {
                self.messages.push(json!({
                    "role": message.role.as_str(),
                    "content": message.content,
                }));
            }
        }
        let current_user_in_history = request.history.iter().any(|message| {
            message.role == ProviderMessageRole::User
                && message.turn_id.as_deref() == Some(request.turn_id.as_str())
        });
        if !current_user_in_history {
            self.messages
                .push(json!({ "role": "user", "content": request.input }));
        }
    }

    fn request_body(&self, stream: bool) -> Value {
        let mut body = json!({
            "model": self.config.model,
            "messages": self.messages,
            "stream": stream,
        });
        if !self.tools.is_empty() {
            body["tools"] = Value::Array(
                self.tools
                    .iter()
                    .map(|(name, definition)| openai_tool_definition(name, definition))
                    .collect(),
            );
            body["tool_choice"] = json!("auto");
        }
        if let Some(temperature) = self.config.temperature {
            body["temperature"] = json!(temperature);
        }
        if let Some(reasoning_effort) = self
            .config
            .reasoning_effort
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty() && *value != "off")
        {
            body["reasoning_effort"] = json!(reasoning_effort);
        }
        if let Some(max_output_tokens) = self.config.max_output_tokens {
            body["max_tokens"] = json!(max_output_tokens);
        }
        body
    }
}

impl ModelProvider for OpenAiCompatibleModelProvider {
    fn next_step(&mut self, request: &ModelRequest) -> Result<ModelProviderStep, RuntimeError> {
        self.seed_messages(request);
        let copilot_auth = if self.config.kind == DirectModelProviderKind::GitHubCopilot {
            Some(read_or_refresh_github_copilot_auth(&self.agent)?)
        } else {
            None
        };
        let api_key = if let Some(auth) = &copilot_auth {
            auth.access_token.clone()
        } else {
            self.api_key()?
        };
        let endpoint = self.endpoint(copilot_auth.as_ref());
        let stream = self.config.stream;
        let body = self.request_body(stream);
        let started = Instant::now();
        let mut provider_request = self
            .agent
            .post(&endpoint)
            .set("authorization", &format!("Bearer {api_key}"))
            .set("content-type", "application/json");
        if self.config.kind == DirectModelProviderKind::GitHubCopilot {
            provider_request = apply_github_copilot_headers(provider_request, &self.messages);
        }
        let response = provider_request.send_json(body);
        let status;
        let (step, message, chunk_count, known_token_delta) = match response {
            Ok(response) => {
                status = response.status();
                if stream {
                    let reader = BufReader::new(response.into_reader());
                    let (step, message, chunk_count, known_token_delta) =
                        parse_openai_compatible_stream(reader, &self.tools)?;
                    (step, message, chunk_count, known_token_delta)
                } else {
                    let value = response.into_json::<Value>().map_err(|error| {
                        RuntimeError::new(
                            "provider_response_decode_failed",
                            RuntimeErrorCategory::Provider,
                            format!("direct provider returned invalid JSON: {error}"),
                        )
                    })?;
                    let known_token_delta = provider_usage_total_tokens(&value).unwrap_or(0);
                    let message = openai_response_message(&value)?.clone();
                    let step = parse_openai_compatible_message(&message, &self.tools)?;
                    let chunk_count = step.assistant_deltas.len();
                    (step, message, chunk_count, known_token_delta)
                }
            }
            Err(ureq::Error::Status(error_status, response)) => {
                let detail = response.into_string().unwrap_or_default();
                return Err(RuntimeError::new(
                    "provider_http_error",
                    RuntimeErrorCategory::Provider,
                    format!(
                        "direct provider HTTP request failed with status {error_status}{}",
                        if detail.trim().is_empty() {
                            String::new()
                        } else {
                            format!(": {}", compact_provider_error(&detail))
                        }
                    ),
                ));
            }
            Err(error) => {
                return Err(RuntimeError::new(
                    "provider_transport_error",
                    RuntimeErrorCategory::Provider,
                    format!("direct provider transport error: {error}"),
                ));
            }
        };
        self.messages.push(message);
        let diagnostics = vec![provider_diagnostic(
            &self.config,
            &endpoint,
            status,
            started.elapsed(),
            stream,
            chunk_count,
            step.tool_calls.len(),
            request.history.len(),
        )];
        Ok(ModelProviderStep {
            step,
            diagnostics,
            known_token_delta,
        })
    }

    fn observe_tool_result(
        &mut self,
        call: &ToolCall,
        result: &ToolResult,
    ) -> Result<(), RuntimeError> {
        self.messages.push(json!({
            "role": "tool",
            "tool_call_id": call.id,
            "content": openai_tool_result_content(result),
        }));
        Ok(())
    }

    fn snapshot(&self) -> Option<DirectProviderResumeState> {
        Some(DirectProviderResumeState {
            config: self.config.clone(),
            messages: self.messages.clone(),
        })
    }

    fn direct_config(&self) -> Option<DirectModelProviderConfig> {
        Some(self.config.clone())
    }
}

#[derive(Debug, Clone)]
struct GitHubCopilotAuth {
    access_token: String,
    refresh_token: String,
    expires: i64,
    enterprise_domain: Option<String>,
}

fn github_copilot_auth_path() -> PathBuf {
    std::env::var_os("OPPI_GITHUB_COPILOT_AUTH_PATH")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| default_oppi_agent_dir().join("auth.json"))
}

fn parse_github_copilot_auth(raw: &Value) -> Option<GitHubCopilotAuth> {
    let credential = raw.get(GITHUB_COPILOT_PROVIDER_ID)?;
    if credential.get("type").and_then(Value::as_str) != Some("oauth") {
        return None;
    }
    Some(GitHubCopilotAuth {
        access_token: credential.get("access")?.as_str()?.to_string(),
        refresh_token: credential.get("refresh")?.as_str()?.to_string(),
        expires: credential.get("expires")?.as_i64()?,
        enterprise_domain: credential
            .get("enterpriseUrl")
            .or_else(|| credential.get("enterpriseDomain"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
    })
}

fn read_github_copilot_auth_json(path: &Path) -> Result<Value, RuntimeError> {
    let raw = fs::read_to_string(path).map_err(|error| {
        RuntimeError::new(
            "provider_auth_missing",
            RuntimeErrorCategory::Provider,
            format!(
                "No GitHub Copilot OAuth credentials found at {} ({error}). Run `/login subscription copilot` first.",
                path.display()
            ),
        )
    })?;
    serde_json::from_str(&raw).map_err(|error| {
        RuntimeError::new(
            "provider_auth_invalid",
            RuntimeErrorCategory::Provider,
            format!(
                "GitHub Copilot auth store {} is invalid JSON: {error}",
                path.display()
            ),
        )
    })
}

fn github_copilot_base_url_from_token(token: &str) -> Option<String> {
    token.split(';').find_map(|part| {
        let (key, value) = part.split_once('=')?;
        if key.trim() != "proxy-ep" {
            return None;
        }
        let api_host = value.trim().strip_prefix("proxy.").unwrap_or(value.trim());
        Some(format!("https://api.{api_host}"))
    })
}

fn github_copilot_base_url(auth: &GitHubCopilotAuth) -> String {
    github_copilot_base_url_from_token(&auth.access_token)
        .or_else(|| {
            auth.enterprise_domain
                .as_ref()
                .map(|domain| format!("https://copilot-api.{domain}"))
        })
        .unwrap_or_else(|| GITHUB_COPILOT_DEFAULT_BASE_URL.to_string())
}

fn github_copilot_token_url(domain: Option<&str>) -> String {
    format!(
        "https://api.{}/copilot_internal/v2/token",
        domain.unwrap_or(GITHUB_COPILOT_DEFAULT_DOMAIN)
    )
}

fn refresh_github_copilot_auth(
    agent: &ureq::Agent,
    auth: &GitHubCopilotAuth,
) -> Result<GitHubCopilotAuth, RuntimeError> {
    let url = github_copilot_token_url(auth.enterprise_domain.as_deref());
    let response = agent
        .get(&url)
        .set("accept", "application/json")
        .set("authorization", &format!("Bearer {}", auth.refresh_token))
        .set("User-Agent", "GitHubCopilotChat/0.35.0")
        .set("Editor-Version", "vscode/1.107.0")
        .set("Editor-Plugin-Version", "copilot-chat/0.35.0")
        .set("Copilot-Integration-Id", "vscode-chat")
        .call()
        .map_err(|error| match error {
            ureq::Error::Status(status, _) => RuntimeError::new(
                "provider_auth_refresh_failed",
                RuntimeErrorCategory::Provider,
                format!("GitHub Copilot token refresh failed with HTTP status {status}. Run `/login subscription copilot` again."),
            ),
            other => RuntimeError::new(
                "provider_auth_refresh_failed",
                RuntimeErrorCategory::Provider,
                format!("GitHub Copilot token refresh transport failed: {other}"),
            ),
        })?;
    let value = response.into_json::<Value>().map_err(|error| {
        RuntimeError::new(
            "provider_auth_refresh_failed",
            RuntimeErrorCategory::Provider,
            format!("GitHub Copilot token refresh returned invalid JSON: {error}"),
        )
    })?;
    let access = value
        .get("token")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            RuntimeError::new(
                "provider_auth_refresh_failed",
                RuntimeErrorCategory::Provider,
                "GitHub Copilot token refresh did not return token".to_string(),
            )
        })?;
    let expires_at = value
        .get("expires_at")
        .and_then(Value::as_i64)
        .filter(|value| *value > 0)
        .ok_or_else(|| {
            RuntimeError::new(
                "provider_auth_refresh_failed",
                RuntimeErrorCategory::Provider,
                "GitHub Copilot token refresh did not return expires_at".to_string(),
            )
        })?;
    Ok(GitHubCopilotAuth {
        access_token: access.to_string(),
        refresh_token: auth.refresh_token.clone(),
        expires: expires_at
            .saturating_mul(1000)
            .saturating_sub(5 * 60 * 1000),
        enterprise_domain: auth.enterprise_domain.clone(),
    })
}

fn read_or_refresh_github_copilot_auth(
    agent: &ureq::Agent,
) -> Result<GitHubCopilotAuth, RuntimeError> {
    let path = github_copilot_auth_path();
    let _lock = CodexAuthLock::acquire(&path)?;
    let mut data = read_github_copilot_auth_json(&path)?;
    let auth = parse_github_copilot_auth(&data).ok_or_else(|| {
        RuntimeError::new(
            "provider_auth_missing",
            RuntimeErrorCategory::Provider,
            format!(
                "No GitHub Copilot OAuth credential named `{GITHUB_COPILOT_PROVIDER_ID}` found in {}. Run `/login subscription copilot` first.",
                path.display()
            ),
        )
    })?;
    if auth.expires > now_millis_i64().saturating_add(60_000) {
        return Ok(auth);
    }
    let refreshed = refresh_github_copilot_auth(agent, &auth)?;
    if !data.is_object() {
        data = json!({});
    }
    let mut credential = json!({
        "type": "oauth",
        "access": refreshed.access_token,
        "refresh": refreshed.refresh_token,
        "expires": refreshed.expires,
    });
    if let Some(domain) = &refreshed.enterprise_domain {
        credential["enterpriseUrl"] = json!(domain);
    }
    data[GITHUB_COPILOT_PROVIDER_ID] = credential;
    write_codex_auth_json(&path, &data)?;
    parse_github_copilot_auth(&data).ok_or_else(|| {
        RuntimeError::new(
            "provider_auth_invalid",
            RuntimeErrorCategory::Provider,
            "GitHub Copilot OAuth credential could not be re-read after refresh".to_string(),
        )
    })
}

fn github_copilot_initiator(messages: &[Value]) -> &'static str {
    messages
        .last()
        .and_then(|message| message.get("role"))
        .and_then(Value::as_str)
        .is_some_and(|role| role != "user")
        .then_some("agent")
        .unwrap_or("user")
}

fn apply_github_copilot_headers(request: ureq::Request, messages: &[Value]) -> ureq::Request {
    request
        .set("User-Agent", "GitHubCopilotChat/0.35.0")
        .set("Editor-Version", "vscode/1.107.0")
        .set("Editor-Plugin-Version", "copilot-chat/0.35.0")
        .set("Copilot-Integration-Id", "vscode-chat")
        .set("X-Initiator", github_copilot_initiator(messages))
        .set("Openai-Intent", "conversation-edits")
}

#[derive(Debug, Clone)]
struct CodexAuth {
    access_token: String,
    refresh_token: String,
    expires: i64,
    account_id: String,
}

struct OpenAiCodexModelProvider {
    config: DirectModelProviderConfig,
    agent: ureq::Agent,
    messages: Vec<Value>,
    tools: BTreeMap<String, ToolDefinition>,
}

impl OpenAiCodexModelProvider {
    fn new(config: DirectModelProviderConfig, tools: Vec<ToolDefinition>) -> Self {
        Self::with_messages(config, tools, Vec::new())
    }

    fn from_resume_state(state: DirectProviderResumeState, tools: Vec<ToolDefinition>) -> Self {
        Self::with_messages(state.config, tools, state.messages)
    }

    fn with_messages(
        config: DirectModelProviderConfig,
        tools: Vec<ToolDefinition>,
        messages: Vec<Value>,
    ) -> Self {
        Self {
            config,
            agent: ureq::AgentBuilder::new()
                .timeout(Duration::from_secs(120))
                .build(),
            messages,
            tools: tools
                .into_iter()
                .map(|definition| (openai_tool_name(&definition), definition))
                .collect(),
        }
    }

    fn endpoint(&self) -> String {
        let base = self
            .config
            .base_url
            .clone()
            .or_else(|| std::env::var("OPPI_OPENAI_CODEX_BASE_URL").ok())
            .or_else(|| std::env::var("OPPI_CODEX_BASE_URL").ok())
            .unwrap_or_else(|| OPENAI_CODEX_DEFAULT_BASE_URL.to_string());
        let base = base.trim_end_matches('/');
        if base.ends_with("/codex/responses") {
            base.to_string()
        } else if base.ends_with("/codex") {
            format!("{base}/responses")
        } else {
            format!("{base}/codex/responses")
        }
    }

    fn auth(&self) -> Result<CodexAuth, RuntimeError> {
        read_or_refresh_codex_auth(&self.agent)
    }

    fn seed_messages(&mut self, request: &ModelRequest) {
        if !self.messages.is_empty() {
            return;
        }
        for message in &request.history {
            if message.content.trim().is_empty() {
                continue;
            }
            match &message.role {
                ProviderMessageRole::User => {
                    self.messages.push(codex_user_message(&message.content))
                }
                ProviderMessageRole::Assistant => self
                    .messages
                    .push(codex_assistant_message(&message.content)),
                ProviderMessageRole::System => {}
            }
        }
        let current_user_in_history = request.history.iter().any(|message| {
            message.role == ProviderMessageRole::User
                && message.turn_id.as_deref() == Some(request.turn_id.as_str())
        });
        if !current_user_in_history {
            self.messages.push(codex_user_message(&request.input));
        }
    }

    fn request_body(&self, request: &ModelRequest) -> Value {
        let mut body = json!({
            "model": self.config.model,
            "store": false,
            "stream": true,
            "input": self.messages,
            "text": { "verbosity": "low" },
            "include": ["reasoning.encrypted_content"],
            "tool_choice": "auto",
            "parallel_tool_calls": true,
        });
        if let Some(system) = self
            .config
            .system_prompt
            .as_ref()
            .filter(|value| !value.trim().is_empty())
        {
            body["instructions"] = json!(system);
        }
        if !self.tools.is_empty() {
            body["tools"] = Value::Array(
                self.tools
                    .iter()
                    .map(|(name, definition)| codex_tool_definition(name, definition))
                    .collect(),
            );
        }
        if let Some(temperature) = self.config.temperature {
            body["temperature"] = json!(temperature);
        }
        if let Some(reasoning_effort) = self
            .config
            .reasoning_effort
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty() && *value != "off")
        {
            body["reasoning"] = json!({
                "effort": reasoning_effort,
                "summary": "auto",
            });
        }
        body["prompt_cache_key"] = json!(request.thread_id);
        body
    }
}

impl ModelProvider for OpenAiCodexModelProvider {
    fn next_step(&mut self, request: &ModelRequest) -> Result<ModelProviderStep, RuntimeError> {
        self.seed_messages(request);
        let auth = self.auth()?;
        let endpoint = self.endpoint();
        let body = self.request_body(request);
        let started = Instant::now();
        let response = self
            .agent
            .post(&endpoint)
            .set("authorization", &format!("Bearer {}", auth.access_token))
            .set("chatgpt-account-id", &auth.account_id)
            .set("originator", "pi")
            .set("User-Agent", &codex_user_agent())
            .set("OpenAI-Beta", "responses=experimental")
            .set("accept", "text/event-stream")
            .set("content-type", "application/json")
            .set("x-client-request-id", &request.turn_id)
            .set("session_id", &request.thread_id)
            .send_json(body);
        let status;
        let (step, output_items, chunk_count, known_token_delta) = match response {
            Ok(response) => {
                status = response.status();
                let reader = BufReader::new(response.into_reader());
                parse_openai_codex_stream(reader, &self.tools)?
            }
            Err(ureq::Error::Status(error_status, response)) => {
                let detail = response.into_string().unwrap_or_default();
                return Err(RuntimeError::new(
                    "provider_http_error",
                    RuntimeErrorCategory::Provider,
                    format!(
                        "Codex subscription provider HTTP request failed with status {error_status}{}",
                        if detail.trim().is_empty() {
                            String::new()
                        } else {
                            format!(": {}", compact_provider_error(&detail))
                        }
                    ),
                ));
            }
            Err(error) => {
                return Err(RuntimeError::new(
                    "provider_transport_error",
                    RuntimeErrorCategory::Provider,
                    format!("Codex subscription provider transport error: {error}"),
                ));
            }
        };
        if output_items.is_empty() {
            let text = step.assistant_deltas.join("");
            if !text.trim().is_empty() {
                self.messages.push(codex_assistant_message(&text));
            }
        } else {
            self.messages.extend(output_items);
        }
        let diagnostics = vec![provider_diagnostic(
            &self.config,
            &endpoint,
            status,
            started.elapsed(),
            true,
            chunk_count,
            step.tool_calls.len(),
            request.history.len(),
        )];
        Ok(ModelProviderStep {
            step,
            diagnostics,
            known_token_delta,
        })
    }

    fn observe_tool_result(
        &mut self,
        call: &ToolCall,
        result: &ToolResult,
    ) -> Result<(), RuntimeError> {
        let call_id = call.id.split('|').next().unwrap_or(call.id.as_str());
        self.messages.push(json!({
            "type": "function_call_output",
            "call_id": call_id,
            "output": openai_tool_result_content(result),
        }));
        Ok(())
    }

    fn snapshot(&self) -> Option<DirectProviderResumeState> {
        Some(DirectProviderResumeState {
            config: self.config.clone(),
            messages: self.messages.clone(),
        })
    }

    fn direct_config(&self) -> Option<DirectModelProviderConfig> {
        Some(self.config.clone())
    }
}

fn user_home_dir_for_auth() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

fn default_oppi_agent_dir() -> PathBuf {
    std::env::var_os("OPPI_AGENT_DIR")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("PI_CODING_AGENT_DIR")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        })
        .unwrap_or_else(|| {
            user_home_dir_for_auth()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".oppi")
                .join("agent")
        })
}

fn codex_auth_path() -> PathBuf {
    std::env::var_os("OPPI_OPENAI_CODEX_AUTH_PATH")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| default_oppi_agent_dir().join("auth.json"))
}

fn codex_auth_lock_path(path: &Path) -> PathBuf {
    let mut lock_path = path.as_os_str().to_os_string();
    lock_path.push(".lock");
    PathBuf::from(lock_path)
}

#[derive(Debug)]
struct CodexAuthLock {
    path: PathBuf,
}

impl CodexAuthLock {
    fn acquire(auth_path: &Path) -> Result<Self, RuntimeError> {
        if let Some(parent) = auth_path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                RuntimeError::new(
                    "provider_auth_lock_failed",
                    RuntimeErrorCategory::Provider,
                    format!("Could not create auth dir {}: {error}", parent.display()),
                )
            })?;
        }
        let lock_path = codex_auth_lock_path(auth_path);
        let mut delay = Duration::from_millis(100);
        for attempt in 0..10 {
            match fs::create_dir(&lock_path) {
                Ok(()) => return Ok(Self { path: lock_path }),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    if codex_auth_lock_is_stale(&lock_path) {
                        let _ = remove_codex_auth_lock_path(&lock_path);
                        continue;
                    }
                    if attempt == 9 {
                        return Err(RuntimeError::new(
                            "provider_auth_lock_failed",
                            RuntimeErrorCategory::Provider,
                            format!(
                                "Could not acquire Pi-compatible auth lock {}. Another OPPi/Pi process may be refreshing credentials.",
                                lock_path.display()
                            ),
                        ));
                    }
                    std::thread::sleep(delay);
                    delay = (delay * 2).min(Duration::from_secs(2));
                }
                Err(error) => {
                    return Err(RuntimeError::new(
                        "provider_auth_lock_failed",
                        RuntimeErrorCategory::Provider,
                        format!(
                            "Could not acquire auth lock {}: {error}",
                            lock_path.display()
                        ),
                    ));
                }
            }
        }
        Err(RuntimeError::new(
            "provider_auth_lock_failed",
            RuntimeErrorCategory::Provider,
            format!("Could not acquire auth lock {}", lock_path.display()),
        ))
    }
}

impl Drop for CodexAuthLock {
    fn drop(&mut self) {
        let _ = remove_codex_auth_lock_path(&self.path);
    }
}

fn codex_auth_lock_is_stale(path: &Path) -> bool {
    fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| modified.elapsed().ok())
        .is_some_and(|elapsed| elapsed > Duration::from_secs(30))
}

fn remove_codex_auth_lock_path(path: &Path) -> std::io::Result<()> {
    fs::remove_dir(path).or_else(|dir_error| {
        if path.is_file() {
            fs::remove_file(path)
        } else {
            Err(dir_error)
        }
    })
}

fn now_millis_i64() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}

fn parse_codex_auth(raw: &Value) -> Option<CodexAuth> {
    let credential = raw.get(OPENAI_CODEX_PROVIDER_ID)?;
    if credential.get("type").and_then(Value::as_str) != Some("oauth") {
        return None;
    }
    Some(CodexAuth {
        access_token: credential.get("access")?.as_str()?.to_string(),
        refresh_token: credential.get("refresh")?.as_str()?.to_string(),
        expires: credential.get("expires")?.as_i64()?,
        account_id: credential.get("accountId")?.as_str()?.to_string(),
    })
}

fn read_codex_auth_json(path: &Path) -> Result<Value, RuntimeError> {
    let raw = fs::read_to_string(path).map_err(|error| {
        RuntimeError::new(
            "provider_auth_missing",
            RuntimeErrorCategory::Provider,
            format!(
                "No ChatGPT/Codex OAuth credentials found at {} ({error}). Run `/login subscription codex` first.",
                path.display()
            ),
        )
    })?;
    serde_json::from_str(&raw).map_err(|error| {
        RuntimeError::new(
            "provider_auth_invalid",
            RuntimeErrorCategory::Provider,
            format!(
                "Codex auth store {} is invalid JSON: {error}",
                path.display()
            ),
        )
    })
}

fn write_codex_auth_json(path: &Path, data: &Value) -> Result<(), RuntimeError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            RuntimeError::new(
                "provider_auth_write_failed",
                RuntimeErrorCategory::Provider,
                format!("Could not create auth dir {}: {error}", parent.display()),
            )
        })?;
    }
    let rendered = serde_json::to_string_pretty(data).map_err(|error| {
        RuntimeError::new(
            "provider_auth_write_failed",
            RuntimeErrorCategory::Provider,
            format!("Could not serialize Codex auth store: {error}"),
        )
    })?;
    fs::write(path, format!("{rendered}\n")).map_err(|error| {
        RuntimeError::new(
            "provider_auth_write_failed",
            RuntimeErrorCategory::Provider,
            format!(
                "Could not write Codex auth store {}: {error}",
                path.display()
            ),
        )
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

fn form_url_encode(input: &str) -> String {
    input
        .bytes()
        .flat_map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                vec![byte as char]
            }
            b' ' => vec!['+'],
            _ => format!("%{byte:02X}").chars().collect::<Vec<_>>(),
        })
        .collect()
}

fn codex_token_form(params: &[(&str, &str)]) -> String {
    params
        .iter()
        .map(|(key, value)| format!("{}={}", form_url_encode(key), form_url_encode(value)))
        .collect::<Vec<_>>()
        .join("&")
}

fn codex_account_id_from_access_token(access_token: &str) -> Result<String, RuntimeError> {
    let payload = access_token.split('.').nth(1).ok_or_else(|| {
        RuntimeError::new(
            "provider_auth_invalid",
            RuntimeErrorCategory::Provider,
            "Codex OAuth access token is not a JWT".to_string(),
        )
    })?;
    let decoded = URL_SAFE_NO_PAD
        .decode(payload.as_bytes())
        .map_err(|error| {
            RuntimeError::new(
                "provider_auth_invalid",
                RuntimeErrorCategory::Provider,
                format!("Could not decode Codex OAuth access-token payload: {error}"),
            )
        })?;
    let value = serde_json::from_slice::<Value>(&decoded).map_err(|error| {
        RuntimeError::new(
            "provider_auth_invalid",
            RuntimeErrorCategory::Provider,
            format!("Could not parse Codex OAuth access-token payload: {error}"),
        )
    })?;
    value
        .get("https://api.openai.com/auth")
        .and_then(|auth| auth.get("chatgpt_account_id"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            RuntimeError::new(
                "provider_auth_invalid",
                RuntimeErrorCategory::Provider,
                "Codex OAuth access token did not include a ChatGPT account id".to_string(),
            )
        })
}

fn refresh_codex_auth(agent: &ureq::Agent, auth: &CodexAuth) -> Result<CodexAuth, RuntimeError> {
    let body = codex_token_form(&[
        ("grant_type", "refresh_token"),
        ("refresh_token", &auth.refresh_token),
        ("client_id", OPENAI_CODEX_CLIENT_ID),
    ]);
    let response = agent
        .post(OPENAI_CODEX_TOKEN_URL)
        .set("content-type", "application/x-www-form-urlencoded")
        .send_string(&body)
        .map_err(|error| match error {
            ureq::Error::Status(status, _) => RuntimeError::new(
                "provider_auth_refresh_failed",
                RuntimeErrorCategory::Provider,
                format!("Codex OAuth refresh failed with HTTP status {status}. Run `/login subscription codex` again."),
            ),
            other => RuntimeError::new(
                "provider_auth_refresh_failed",
                RuntimeErrorCategory::Provider,
                format!("Codex OAuth refresh transport failed: {other}"),
            ),
        })?;
    let value = response.into_json::<Value>().map_err(|error| {
        RuntimeError::new(
            "provider_auth_refresh_failed",
            RuntimeErrorCategory::Provider,
            format!("Codex OAuth refresh returned invalid JSON: {error}"),
        )
    })?;
    let access = value
        .get("access_token")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            RuntimeError::new(
                "provider_auth_refresh_failed",
                RuntimeErrorCategory::Provider,
                "Codex OAuth refresh did not return an access token".to_string(),
            )
        })?;
    let refresh = value
        .get("refresh_token")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            RuntimeError::new(
                "provider_auth_refresh_failed",
                RuntimeErrorCategory::Provider,
                "Codex OAuth refresh did not return a refresh token".to_string(),
            )
        })?;
    let expires_in = value
        .get("expires_in")
        .and_then(Value::as_i64)
        .filter(|value| *value > 0)
        .ok_or_else(|| {
            RuntimeError::new(
                "provider_auth_refresh_failed",
                RuntimeErrorCategory::Provider,
                "Codex OAuth refresh did not return expires_in".to_string(),
            )
        })?;
    Ok(CodexAuth {
        access_token: access.to_string(),
        refresh_token: refresh.to_string(),
        expires: now_millis_i64().saturating_add(expires_in.saturating_mul(1000)),
        account_id: codex_account_id_from_access_token(access)?,
    })
}

fn read_or_refresh_codex_auth(agent: &ureq::Agent) -> Result<CodexAuth, RuntimeError> {
    let path = codex_auth_path();
    let data = read_codex_auth_json(&path)?;
    let auth = parse_codex_auth(&data).ok_or_else(|| {
        RuntimeError::new(
            "provider_auth_missing",
            RuntimeErrorCategory::Provider,
            format!(
                "No ChatGPT/Codex OAuth credential named `{OPENAI_CODEX_PROVIDER_ID}` found in {}. Run `/login subscription codex` first.",
                path.display()
            ),
        )
    })?;
    if auth.expires > now_millis_i64().saturating_add(60_000) {
        return Ok(auth);
    }
    let _lock = CodexAuthLock::acquire(&path)?;
    let mut data = read_codex_auth_json(&path)?;
    let auth = parse_codex_auth(&data).ok_or_else(|| {
        RuntimeError::new(
            "provider_auth_missing",
            RuntimeErrorCategory::Provider,
            format!(
                "No ChatGPT/Codex OAuth credential named `{OPENAI_CODEX_PROVIDER_ID}` found in {}. Run `/login subscription codex` first.",
                path.display()
            ),
        )
    })?;
    if auth.expires > now_millis_i64().saturating_add(60_000) {
        return Ok(auth);
    }
    let refreshed = refresh_codex_auth(agent, &auth)?;
    if !data.is_object() {
        data = json!({});
    }
    data[OPENAI_CODEX_PROVIDER_ID] = json!({
        "type": "oauth",
        "access": refreshed.access_token,
        "refresh": refreshed.refresh_token,
        "expires": refreshed.expires,
        "accountId": refreshed.account_id,
    });
    write_codex_auth_json(&path, &data)?;
    parse_codex_auth(&data).ok_or_else(|| {
        RuntimeError::new(
            "provider_auth_invalid",
            RuntimeErrorCategory::Provider,
            "Codex OAuth credential could not be re-read after refresh".to_string(),
        )
    })
}

fn codex_user_agent() -> String {
    format!(
        "pi ({} {}; {})",
        std::env::consts::OS,
        std::env::consts::FAMILY,
        std::env::consts::ARCH
    )
}

fn codex_user_message(text: &str) -> Value {
    json!({
        "role": "user",
        "content": [{ "type": "input_text", "text": text }],
    })
}

fn codex_assistant_message(text: &str) -> Value {
    json!({
        "type": "message",
        "role": "assistant",
        "content": [{ "type": "output_text", "text": text, "annotations": [] }],
        "status": "completed",
    })
}

fn codex_tool_definition(name: &str, definition: &ToolDefinition) -> Value {
    json!({
        "type": "function",
        "name": name,
        "description": definition.description.clone().unwrap_or_else(|| format!("OPPi tool {}", definition.name)),
        "parameters": {
            "type": "object",
            "properties": {},
            "additionalProperties": true,
        },
        "strict": false,
    })
}

fn codex_item_text(item: &Value) -> String {
    item.get("content")
        .and_then(Value::as_array)
        .map(|parts| {
            parts
                .iter()
                .filter_map(|part| {
                    part.get("text")
                        .and_then(Value::as_str)
                        .or_else(|| part.get("content").and_then(Value::as_str))
                })
                .collect::<Vec<_>>()
                .join("")
        })
        .unwrap_or_default()
}

fn codex_function_call_to_openai_raw(item: &Value) -> Option<Value> {
    if item.get("type").and_then(Value::as_str) != Some("function_call") {
        return None;
    }
    let call_id = item
        .get("call_id")
        .and_then(Value::as_str)
        .or_else(|| item.get("callId").and_then(Value::as_str))
        .or_else(|| item.get("id").and_then(Value::as_str))?
        .to_string();
    let item_id = item
        .get("id")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty());
    let id = item_id
        .filter(|value| *value != call_id)
        .map(|value| format!("{call_id}|{value}"))
        .unwrap_or_else(|| call_id.clone());
    let name = item.get("name").and_then(Value::as_str)?.to_string();
    let arguments = match item.get("arguments") {
        Some(Value::String(raw)) => raw.clone(),
        Some(value) => value.to_string(),
        None => "{}".to_string(),
    };
    Some(json!({
        "id": id,
        "type": "function",
        "function": {
            "name": name,
            "arguments": arguments,
        }
    }))
}

#[derive(Debug, Default)]
struct PartialCodexFunctionCall {
    item_id: Option<String>,
    call_id: Option<String>,
    name: Option<String>,
    arguments: String,
}

impl PartialCodexFunctionCall {
    fn to_item(&self) -> Option<Value> {
        Some(json!({
            "type": "function_call",
            "id": self.item_id.as_deref()?,
            "call_id": self.call_id.as_deref().or(self.item_id.as_deref())?,
            "name": self.name.as_deref()?,
            "arguments": self.arguments,
        }))
    }
}

fn push_codex_function_call(
    raw_tool_calls: &mut Vec<Value>,
    final_items: &mut Vec<Value>,
    seen_ids: &mut BTreeSet<String>,
    item: Value,
) {
    if let Some(raw) = codex_function_call_to_openai_raw(&item)
        && let Some(id) = raw.get("id").and_then(Value::as_str)
        && seen_ids.insert(id.to_string())
    {
        raw_tool_calls.push(raw);
        final_items.push(item);
    }
}

fn parse_openai_codex_stream<R: BufRead>(
    mut reader: R,
    tools: &BTreeMap<String, ToolDefinition>,
) -> Result<(ScriptedModelStep, Vec<Value>, usize, i64), RuntimeError> {
    let mut content_chunks = Vec::new();
    let mut final_items = Vec::new();
    let mut raw_tool_calls = Vec::new();
    let mut partial_calls: BTreeMap<String, PartialCodexFunctionCall> = BTreeMap::new();
    let mut seen_tool_ids = BTreeSet::new();
    let mut chunk_count = 0usize;
    let mut known_token_delta = 0i64;
    let mut block = Vec::<String>::new();
    let mut line = String::new();

    loop {
        line.clear();
        let read = reader.read_line(&mut line).map_err(|error| {
            RuntimeError::new(
                "provider_stream_read_failed",
                RuntimeErrorCategory::Provider,
                format!("Codex provider streaming read failed: {error}"),
            )
        })?;
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if read == 0 || trimmed.is_empty() {
            if !block.is_empty() {
                let data = block
                    .iter()
                    .filter_map(|entry| entry.strip_prefix("data:"))
                    .map(str::trim)
                    .collect::<Vec<_>>()
                    .join("\n");
                block.clear();
                if !data.is_empty() && data != "[DONE]" {
                    let event: Value = serde_json::from_str(&data).map_err(|error| {
                        RuntimeError::new(
                            "provider_stream_decode_failed",
                            RuntimeErrorCategory::Provider,
                            format!("Codex provider returned invalid SSE JSON: {error}"),
                        )
                    })?;
                    chunk_count += 1;
                    let event_type = event
                        .get("type")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    match event_type {
                        "response.output_text.delta" => {
                            if let Some(delta) = event.get("delta").and_then(Value::as_str) {
                                content_chunks.push(delta.to_string());
                            }
                        }
                        "response.function_call_arguments.delta" => {
                            let item_id = event
                                .get("item_id")
                                .or_else(|| event.get("itemId"))
                                .and_then(Value::as_str)
                                .unwrap_or("function_call")
                                .to_string();
                            let entry = partial_calls.entry(item_id.clone()).or_default();
                            entry.item_id.get_or_insert(item_id);
                            if let Some(delta) = event.get("delta").and_then(Value::as_str) {
                                entry.arguments.push_str(delta);
                            }
                        }
                        "response.output_item.added" => {
                            if let Some(item) = event.get("item")
                                && item.get("type").and_then(Value::as_str) == Some("function_call")
                            {
                                let item_id = item
                                    .get("id")
                                    .and_then(Value::as_str)
                                    .unwrap_or("function_call")
                                    .to_string();
                                let entry = partial_calls.entry(item_id.clone()).or_default();
                                entry.item_id.get_or_insert(item_id);
                                if let Some(call_id) = item.get("call_id").and_then(Value::as_str) {
                                    entry.call_id = Some(call_id.to_string());
                                }
                                if let Some(name) = item.get("name").and_then(Value::as_str) {
                                    entry.name = Some(name.to_string());
                                }
                            }
                        }
                        "response.output_item.done" => {
                            if let Some(item) = event.get("item") {
                                match item.get("type").and_then(Value::as_str) {
                                    Some("message") => {
                                        let text = codex_item_text(item);
                                        if !text.is_empty() && content_chunks.is_empty() {
                                            content_chunks.push(text);
                                        }
                                        final_items.push(item.clone());
                                    }
                                    Some("function_call") => push_codex_function_call(
                                        &mut raw_tool_calls,
                                        &mut final_items,
                                        &mut seen_tool_ids,
                                        item.clone(),
                                    ),
                                    _ => {}
                                }
                            }
                        }
                        "response.completed" | "response.done" | "response.incomplete" => {
                            if let Some(tokens) = provider_usage_total_tokens(&event) {
                                known_token_delta = tokens;
                            }
                            if let Some(items) = event
                                .get("response")
                                .and_then(|response| response.get("output"))
                                .and_then(Value::as_array)
                            {
                                for item in items {
                                    match item.get("type").and_then(Value::as_str) {
                                        Some("message") => {
                                            let text = codex_item_text(item);
                                            if !text.is_empty() && content_chunks.is_empty() {
                                                content_chunks.push(text);
                                            }
                                            if final_items.iter().all(|existing| existing != item) {
                                                final_items.push(item.clone());
                                            }
                                        }
                                        Some("function_call") => push_codex_function_call(
                                            &mut raw_tool_calls,
                                            &mut final_items,
                                            &mut seen_tool_ids,
                                            item.clone(),
                                        ),
                                        _ => {}
                                    }
                                }
                            }
                        }
                        "response.failed" => {
                            let message = event
                                .get("response")
                                .and_then(|response| response.get("error"))
                                .and_then(|error| error.get("message"))
                                .and_then(Value::as_str)
                                .unwrap_or("Codex response failed");
                            return Err(RuntimeError::new(
                                "provider_response_failed",
                                RuntimeErrorCategory::Provider,
                                message.to_string(),
                            ));
                        }
                        "error" => {
                            let message = event
                                .get("message")
                                .and_then(Value::as_str)
                                .or_else(|| event.get("code").and_then(Value::as_str))
                                .unwrap_or("Codex provider returned an error event");
                            return Err(RuntimeError::new(
                                "provider_response_failed",
                                RuntimeErrorCategory::Provider,
                                message.to_string(),
                            ));
                        }
                        _ => {}
                    }
                }
            }
            if read == 0 {
                break;
            }
        } else {
            block.push(trimmed.to_string());
        }
    }

    for partial in partial_calls.values() {
        if let Some(item) = partial.to_item() {
            push_codex_function_call(
                &mut raw_tool_calls,
                &mut final_items,
                &mut seen_tool_ids,
                item,
            );
        }
    }

    let content = content_chunks.concat();
    let mut message = json!({
        "role": "assistant",
        "content": if content.is_empty() { Value::Null } else { Value::String(content) },
    });
    if !raw_tool_calls.is_empty() {
        message["tool_calls"] = Value::Array(raw_tool_calls);
    }
    let mut step = parse_openai_compatible_message(&message, tools)?;
    step.assistant_deltas = content_chunks;
    Ok((step, final_items, chunk_count, known_token_delta))
}

fn model_provider(
    config: Option<DirectModelProviderConfig>,
    model_steps: Vec<ScriptedModelStep>,
    tools: Vec<ToolDefinition>,
    resume_state: Option<DirectProviderResumeState>,
) -> Result<Box<dyn ModelProvider>, RuntimeError> {
    if (config.is_some() || resume_state.is_some()) && !model_steps.is_empty() {
        return Err(RuntimeError::new(
            "provider_and_scripted_steps_conflict",
            RuntimeErrorCategory::InvalidRequest,
            "modelProvider cannot be combined with scripted modelSteps",
        ));
    }
    match (resume_state, config) {
        (Some(mut state), config) => {
            if let Some(config) = config {
                state.config = config;
            }
            match state.config.kind {
                DirectModelProviderKind::OpenAiCompatible
                | DirectModelProviderKind::GitHubCopilot => Ok(Box::new(
                    OpenAiCompatibleModelProvider::from_resume_state(state, tools),
                )),
                DirectModelProviderKind::OpenAiCodex => Ok(Box::new(
                    OpenAiCodexModelProvider::from_resume_state(state, tools),
                )),
            }
        }
        (None, Some(config)) => match config.kind {
            DirectModelProviderKind::OpenAiCompatible | DirectModelProviderKind::GitHubCopilot => {
                Ok(Box::new(OpenAiCompatibleModelProvider::new(config, tools)))
            }
            DirectModelProviderKind::OpenAiCodex => {
                Ok(Box::new(OpenAiCodexModelProvider::new(config, tools)))
            }
        },
        (None, None) => Ok(Box::new(ScriptedModelProvider::new(model_steps))),
    }
}

fn provider_api_key_env_candidates(explicit: Option<&str>) -> Result<Vec<String>, RuntimeError> {
    if let Some(name) = explicit.map(str::trim).filter(|name| !name.is_empty()) {
        if provider_api_key_env_allowed(name) {
            return Ok(vec![name.to_string()]);
        }
        return Err(RuntimeError::new(
            "provider_api_key_env_not_allowed",
            RuntimeErrorCategory::Provider,
            format!(
                "direct provider apiKeyEnv is not allowed: {name}. Use OPPI_OPENAI_API_KEY, OPENAI_API_KEY, or an OPPI_*_API_KEY variable."
            ),
        ));
    }
    Ok(vec![
        "OPPI_OPENAI_API_KEY".to_string(),
        "OPENAI_API_KEY".to_string(),
    ])
}

fn provider_api_key_env_allowed(name: &str) -> bool {
    if name.len() > 128 || name.is_empty() {
        return false;
    }
    if !name
        .chars()
        .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_')
    {
        return false;
    }
    matches!(name, "OPPI_OPENAI_API_KEY" | "OPENAI_API_KEY")
        || (name.starts_with("OPPI_") && name.ends_with("_API_KEY"))
        || (name.starts_with("OPENAI_") && name.ends_with("_API_KEY"))
        || (name.starts_with("AZURE_OPENAI_") && name.ends_with("_API_KEY"))
}

fn base_url_host(base_url: &str) -> Option<String> {
    let rest = base_url
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(base_url);
    let authority = rest.split('/').next()?.rsplit('@').next()?.trim();
    if authority.starts_with('[') {
        let end = authority.find(']')?;
        return Some(authority[1..end].to_string());
    }
    Some(
        authority
            .rsplit_once(':')
            .map(|(host, _)| host)
            .unwrap_or(authority)
            .to_string(),
    )
}

fn is_loopback_host(host: &str) -> bool {
    let host = host.trim_matches(['[', ']']).to_ascii_lowercase();
    host == "localhost" || host == "127.0.0.1" || host == "::1"
}

fn provider_uses_meridian_placeholder_key(config: &DirectModelProviderConfig) -> bool {
    config.api_key_env.as_deref() == Some(MERIDIAN_API_KEY_ENV)
        && config
            .base_url
            .as_deref()
            .and_then(base_url_host)
            .is_some_and(|host| is_loopback_host(&host))
}

#[derive(Debug, Default)]
struct PartialOpenAiToolCall {
    id: Option<String>,
    call_type: Option<String>,
    function_name: Option<String>,
    arguments: String,
}

fn parse_openai_compatible_stream<R: BufRead>(
    reader: R,
    tools: &BTreeMap<String, ToolDefinition>,
) -> Result<(ScriptedModelStep, Value, usize, i64), RuntimeError> {
    let mut content_chunks = Vec::new();
    let mut partial_calls: BTreeMap<u64, PartialOpenAiToolCall> = BTreeMap::new();
    let mut chunk_count = 0usize;
    let mut known_token_delta = 0i64;

    for line in reader.lines() {
        let line = line.map_err(|error| {
            RuntimeError::new(
                "provider_stream_read_failed",
                RuntimeErrorCategory::Provider,
                format!("direct provider streaming read failed: {error}"),
            )
        })?;
        let line = line.trim();
        if line.is_empty() || line.starts_with(':') {
            continue;
        }
        let Some(data) = line.strip_prefix("data:").map(str::trim) else {
            continue;
        };
        if data == "[DONE]" {
            break;
        }
        let value: Value = serde_json::from_str(data).map_err(|error| {
            RuntimeError::new(
                "provider_stream_chunk_invalid_json",
                RuntimeErrorCategory::Provider,
                format!("direct provider returned invalid streaming JSON: {error}"),
            )
        })?;
        chunk_count += 1;
        if let Some(tokens) = provider_usage_total_tokens(&value) {
            known_token_delta = tokens;
        }
        let Some(delta) = value
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|choices| choices.first())
            .and_then(|choice| choice.get("delta"))
        else {
            continue;
        };
        if let Some(content) = delta.get("content").and_then(Value::as_str)
            && !content.is_empty()
        {
            content_chunks.push(content.to_string());
        }
        if let Some(raw_tool_calls) = delta.get("tool_calls") {
            if raw_tool_calls.is_null() {
                continue;
            }
            let Some(raw_calls) = raw_tool_calls.as_array() else {
                return Err(RuntimeError::new(
                    "provider_tool_calls_invalid_shape",
                    RuntimeErrorCategory::Provider,
                    "direct provider returned non-array streaming tool calls",
                ));
            };
            for raw in raw_calls {
                let index = raw
                    .get("index")
                    .and_then(Value::as_u64)
                    .unwrap_or(partial_calls.len() as u64);
                let partial = partial_calls.entry(index).or_default();
                if let Some(id) = raw.get("id").and_then(Value::as_str)
                    && !id.is_empty()
                {
                    partial.id = Some(id.to_string());
                }
                if let Some(call_type) = raw.get("type").and_then(Value::as_str)
                    && !call_type.is_empty()
                {
                    partial.call_type = Some(call_type.to_string());
                }
                if let Some(function) = raw.get("function") {
                    if let Some(name) = function.get("name").and_then(Value::as_str)
                        && !name.is_empty()
                    {
                        let entry = partial.function_name.get_or_insert_with(String::new);
                        entry.push_str(name);
                    }
                    if let Some(arguments) = function.get("arguments").and_then(Value::as_str) {
                        partial.arguments.push_str(arguments);
                    }
                }
            }
        }
    }

    let raw_tool_calls = partial_calls
        .into_values()
        .map(|partial| {
            let mut raw = json!({
                "type": partial.call_type.unwrap_or_else(|| "function".to_string()),
                "function": {
                    "name": partial.function_name.unwrap_or_default(),
                    "arguments": partial.arguments,
                },
            });
            if let Some(id) = partial.id {
                raw["id"] = json!(id);
            }
            raw
        })
        .collect::<Vec<_>>();
    let mut message = json!({
        "role": "assistant",
        "content": if content_chunks.is_empty() {
            Value::Null
        } else {
            Value::String(content_chunks.concat())
        },
    });
    if !raw_tool_calls.is_empty() {
        message["tool_calls"] = Value::Array(raw_tool_calls);
    }
    let mut step = parse_openai_compatible_message(&message, tools)?;
    step.assistant_deltas = content_chunks;
    Ok((step, message, chunk_count, known_token_delta))
}

fn provider_usage_total_tokens(value: &Value) -> Option<i64> {
    let usage = value.get("usage").or_else(|| {
        value
            .get("response")
            .and_then(|response| response.get("usage"))
    })?;
    usage
        .get("total_tokens")
        .or_else(|| usage.get("totalTokens"))
        .and_then(Value::as_i64)
        .or_else(|| {
            let input = usage
                .get("input_tokens")
                .or_else(|| usage.get("prompt_tokens"))
                .or_else(|| usage.get("inputTokens"))
                .and_then(Value::as_i64)?;
            let output = usage
                .get("output_tokens")
                .or_else(|| usage.get("completion_tokens"))
                .or_else(|| usage.get("outputTokens"))
                .and_then(Value::as_i64)?;
            Some(input.saturating_add(output))
        })
}

fn provider_diagnostic(
    config: &DirectModelProviderConfig,
    endpoint: &str,
    status: u16,
    elapsed: Duration,
    stream: bool,
    chunk_count: usize,
    tool_call_count: usize,
    history_message_count: usize,
) -> Diagnostic {
    let provider_label = match config.kind {
        DirectModelProviderKind::OpenAiCompatible => "openai-compatible",
        DirectModelProviderKind::OpenAiCodex => "openai-codex",
        DirectModelProviderKind::GitHubCopilot => "github-copilot",
    };
    Diagnostic {
        level: DiagnosticLevel::Info,
        message: "direct provider request completed".to_string(),
        metadata: BTreeMap::from([
            ("component".to_string(), "provider".to_string()),
            ("provider".to_string(), provider_label.to_string()),
            ("model".to_string(), config.model.clone()),
            ("endpoint".to_string(), provider_endpoint_label(endpoint)),
            ("status".to_string(), status.to_string()),
            ("durationMs".to_string(), elapsed.as_millis().to_string()),
            ("stream".to_string(), stream.to_string()),
            ("chunks".to_string(), chunk_count.to_string()),
            ("toolCalls".to_string(), tool_call_count.to_string()),
            (
                "historyMessages".to_string(),
                history_message_count.to_string(),
            ),
        ]),
    }
}

fn provider_endpoint_label(endpoint: &str) -> String {
    let (scheme, rest) = endpoint
        .split_once("://")
        .map(|(scheme, rest)| (format!("{scheme}://"), rest))
        .unwrap_or_else(|| (String::new(), endpoint));
    let host = rest.split('/').next().unwrap_or(rest);
    let host = host.rsplit('@').next().unwrap_or(host);
    format!("{scheme}{host}")
}

fn openai_response_message(value: &Value) -> Result<&Value, RuntimeError> {
    value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))
        .ok_or_else(|| {
            RuntimeError::new(
                "provider_response_missing_message",
                RuntimeErrorCategory::Provider,
                "direct provider response did not include choices[0].message",
            )
        })
}

#[cfg(test)]
fn parse_openai_compatible_chat_response(value: Value) -> Result<ScriptedModelStep, RuntimeError> {
    parse_openai_compatible_message(openai_response_message(&value)?, &BTreeMap::new())
}

fn parse_openai_compatible_message(
    message: &Value,
    tools: &BTreeMap<String, ToolDefinition>,
) -> Result<ScriptedModelStep, RuntimeError> {
    let tool_calls = parse_openai_tool_calls(message, tools)?;
    let content = openai_message_content(message);
    Ok(ScriptedModelStep {
        assistant_deltas: if content.is_empty() {
            Vec::new()
        } else {
            vec![content]
        },
        final_response: tool_calls.is_empty(),
        tool_calls,
        tool_results: Vec::new(),
    })
}

fn parse_openai_tool_calls(
    message: &Value,
    tools: &BTreeMap<String, ToolDefinition>,
) -> Result<Vec<ToolCall>, RuntimeError> {
    let Some(raw_tool_calls) = message.get("tool_calls") else {
        return Ok(Vec::new());
    };
    if raw_tool_calls.is_null() {
        return Ok(Vec::new());
    }
    let Some(raw_calls) = raw_tool_calls.as_array() else {
        return Err(RuntimeError::new(
            "provider_tool_calls_invalid_shape",
            RuntimeErrorCategory::Provider,
            "direct provider returned non-array tool calls",
        ));
    };
    let mut calls = Vec::new();
    let mut seen_call_ids = BTreeSet::new();
    for raw in raw_calls {
        if raw
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("function")
            != "function"
        {
            return Err(RuntimeError::new(
                "provider_tool_call_type_unsupported",
                RuntimeErrorCategory::Provider,
                "direct provider returned a non-function tool call",
            ));
        }
        let id = raw
            .get("id")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                RuntimeError::new(
                    "provider_tool_call_missing_id",
                    RuntimeErrorCategory::Provider,
                    "direct provider returned a tool call without an id",
                )
            })?;
        if !seen_call_ids.insert(id.to_string()) {
            return Err(RuntimeError::new(
                "provider_tool_call_duplicate_id",
                RuntimeErrorCategory::Provider,
                format!("direct provider returned duplicate tool call id: {id}"),
            ));
        }
        let function = raw.get("function").ok_or_else(|| {
            RuntimeError::new(
                "provider_tool_call_missing_function",
                RuntimeErrorCategory::Provider,
                "direct provider returned a tool call without a function payload",
            )
        })?;
        let provider_name = function
            .get("name")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                RuntimeError::new(
                    "provider_tool_call_missing_name",
                    RuntimeErrorCategory::Provider,
                    "direct provider returned a function tool call without a name",
                )
            })?;
        let definition = tools.get(provider_name).ok_or_else(|| {
            RuntimeError::new(
                "provider_tool_call_unknown_tool",
                RuntimeErrorCategory::Provider,
                format!("direct provider requested unknown tool: {provider_name}"),
            )
        })?;
        let arguments = match function.get("arguments") {
            Some(Value::String(arguments)) if arguments.trim().is_empty() => json!({}),
            Some(Value::String(arguments)) => {
                let parsed: Value = serde_json::from_str(arguments).map_err(|error| {
                    RuntimeError::new(
                        "provider_tool_arguments_invalid_json",
                        RuntimeErrorCategory::Provider,
                        format!("direct provider returned invalid JSON tool arguments: {error}"),
                    )
                })?;
                if !parsed.is_object() {
                    return Err(RuntimeError::new(
                        "provider_tool_arguments_invalid_shape",
                        RuntimeErrorCategory::Provider,
                        "direct provider returned non-object tool arguments",
                    ));
                }
                parsed
            }
            Some(Value::Object(_)) => function
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({})),
            Some(Value::Null) | None => json!({}),
            Some(_) => {
                return Err(RuntimeError::new(
                    "provider_tool_arguments_invalid_shape",
                    RuntimeErrorCategory::Provider,
                    "direct provider returned non-object tool arguments",
                ));
            }
        };
        calls.push(ToolCall {
            id: id.to_string(),
            name: definition.name.clone(),
            namespace: definition.namespace.clone(),
            arguments,
        });
    }
    Ok(calls)
}

fn openai_message_content(message: &Value) -> String {
    match message.get("content") {
        Some(Value::String(content)) => content.clone(),
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(|part| {
                part.get("text")
                    .and_then(Value::as_str)
                    .or_else(|| part.get("content").and_then(Value::as_str))
            })
            .collect::<Vec<_>>()
            .join(""),
        Some(Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    }
}

fn tool_call_from_provider_snapshot(
    state: &DirectProviderResumeState,
    call_id: &str,
) -> Option<ToolCall> {
    for message in state.messages.iter().rev() {
        if let Some(calls) = message.get("tool_calls").and_then(Value::as_array) {
            for raw in calls {
                if raw.get("id").and_then(Value::as_str) != Some(call_id) {
                    continue;
                }
                let function = raw.get("function")?;
                let provider_name = function.get("name")?.as_str()?;
                let (namespace, name) = decode_openai_tool_name(provider_name);
                let arguments = match function.get("arguments") {
                    Some(Value::String(raw)) => serde_json::from_str(raw).unwrap_or(Value::Null),
                    Some(value) => value.clone(),
                    None => Value::Null,
                };
                return Some(ToolCall {
                    id: call_id.to_string(),
                    name,
                    namespace,
                    arguments,
                });
            }
        }

        if message.get("type").and_then(Value::as_str) == Some("function_call") {
            let provider_call_id = message
                .get("call_id")
                .and_then(Value::as_str)
                .or_else(|| message.get("id").and_then(Value::as_str))?;
            let item_id = message.get("id").and_then(Value::as_str);
            let combined_id = item_id
                .filter(|value| *value != provider_call_id)
                .map(|value| format!("{provider_call_id}|{value}"))
                .unwrap_or_else(|| provider_call_id.to_string());
            if combined_id != call_id && provider_call_id != call_id {
                continue;
            }
            let provider_name = message.get("name")?.as_str()?;
            let (namespace, name) = decode_openai_tool_name(provider_name);
            let arguments = match message.get("arguments") {
                Some(Value::String(raw)) => serde_json::from_str(raw).unwrap_or(Value::Null),
                Some(value) => value.clone(),
                None => Value::Null,
            };
            return Some(ToolCall {
                id: call_id.to_string(),
                name,
                namespace,
                arguments,
            });
        }
    }
    None
}

fn decode_openai_tool_name(provider_name: &str) -> (Option<String>, String) {
    if let Some((namespace, name)) = provider_name.split_once("__")
        && !namespace.is_empty()
        && !name.is_empty()
    {
        return (Some(namespace.to_string()), name.to_string());
    }
    (None, provider_name.to_string())
}

fn openai_tool_name(definition: &ToolDefinition) -> String {
    let raw = match definition.namespace.as_deref() {
        Some(namespace) if !namespace.is_empty() => format!("{namespace}__{}", definition.name),
        _ => definition.name.clone(),
    };
    let sanitized = raw
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    sanitized.chars().take(64).collect()
}

fn openai_tool_definition(name: &str, definition: &ToolDefinition) -> Value {
    json!({
        "type": "function",
        "function": {
            "name": name,
            "description": definition.description.clone().unwrap_or_else(|| format!("OPPi tool {}", definition.name)),
            "parameters": {
                "type": "object",
                "additionalProperties": true,
            },
        },
    })
}

fn openai_tool_result_content(result: &ToolResult) -> String {
    if result.status == ToolResultStatus::Ok {
        return result.output.clone().unwrap_or_default();
    }
    json!({
        "status": result.status,
        "output": result.output,
        "error": result.error,
    })
    .to_string()
}

fn compact_provider_error(detail: &str) -> String {
    let compact = detail.split_whitespace().collect::<Vec<_>>().join(" ");
    truncate_chars(&compact, 300)
}

fn redacted_endpoint(endpoint: &str) -> String {
    let (scheme, rest) = endpoint
        .split_once("://")
        .map(|(scheme, rest)| (format!("{scheme}://"), rest))
        .unwrap_or_else(|| (String::new(), endpoint));
    let without_auth = rest.rsplit('@').next().unwrap_or(rest);
    format!("{scheme}{without_auth}")
}

fn compact_history_text(text: &str) -> String {
    truncate_chars(text.trim(), 4_000)
}

fn compact_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn format_follow_up_context(context: &FollowUpChainContext, current_input: &str) -> String {
    let current_id = context.current_follow_up_id.as_deref();
    let rows = if context.follow_ups.is_empty() {
        vec![format!(
            "- current: {}",
            truncate_chars(&compact_whitespace(current_input), 300)
        )]
    } else {
        context
            .follow_ups
            .iter()
            .map(|follow_up| {
                let status = if current_id == Some(follow_up.id.as_str()) {
                    "current"
                } else {
                    follow_up.status.as_str()
                };
                format!(
                    "- {status}: {}",
                    truncate_chars(&compact_whitespace(&follow_up.text), 300)
                )
            })
            .collect()
    };
    let pending = context
        .follow_ups
        .iter()
        .filter(|follow_up| {
            follow_up.status == FollowUpStatus::Queued && current_id != Some(follow_up.id.as_str())
        })
        .count();
    let final_instruction = if pending > 0 {
        format!(
            "There {} {} additional queued follow-up{} after this one. Keep this response operational; the last follow-up should provide the combined final answer.",
            if pending == 1 { "is" } else { "are" },
            pending,
            if pending == 1 { "" } else { "s" },
        )
    } else {
        "No further follow-ups were queued when this turn started. When this turn is complete, provide the combined final answer for the initial request and every follow-up in this chain.".to_string()
    };
    let mut guidance = vec![
        "OPPi follow-up chain context:".to_string(),
        "This user prompt is a follow-up queued while an earlier answer was still running. Treat it as part of the same user-visible task, not as an unrelated standalone request.".to_string(),
        format!(
            "Initial standalone request: {}",
            truncate_chars(&compact_whitespace(&context.root_prompt), 700)
        ),
        "Follow-up ledger:".to_string(),
        rows.join("\n"),
        final_instruction,
        "Do not dump this ledger verbatim. Use it to make the final user-facing answer cover the initial request plus all completed follow-ups.".to_string(),
    ].join("\n");
    if let Some(append) = context
        .prompt_variant_append
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    {
        guidance.push_str("\n\n");
        guidance.push_str(append);
    }
    guidance
}

fn append_system_prompt(base: Option<String>, heading: &str, append: &str) -> String {
    [
        base.unwrap_or_default().trim().to_string(),
        format!("{heading}:\n{append}"),
    ]
    .into_iter()
    .filter(|part| !part.trim().is_empty())
    .collect::<Vec<_>>()
    .join("\n\n")
}

fn append_follow_up_to_model_provider(
    mut config: Option<DirectModelProviderConfig>,
    follow_up: Option<&FollowUpChainContext>,
    current_input: &str,
) -> Option<DirectModelProviderConfig> {
    if let (Some(config), Some(follow_up)) = (config.as_mut(), follow_up) {
        let base = config.system_prompt.take().unwrap_or_default();
        let guidance = format_follow_up_context(follow_up, current_input);
        config.system_prompt = Some(
            [base.trim(), guidance.trim()]
                .into_iter()
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join("\n\n"),
        );
    }
    config
}

fn follow_up_pending_count(context: &FollowUpChainContext) -> usize {
    let current_id = context.current_follow_up_id.as_deref();
    context
        .follow_ups
        .iter()
        .filter(|follow_up| {
            follow_up.status == FollowUpStatus::Queued && current_id != Some(follow_up.id.as_str())
        })
        .count()
}

fn todo_outcome(todo: &TodoItem) -> Option<TodoOutcome> {
    if todo.status != TodoStatus::Completed && todo.status != TodoStatus::Cancelled {
        return None;
    }
    Some(TodoOutcome {
        id: todo.id.clone(),
        content: todo.content.clone(),
        status: todo.status,
        phase: todo.phase.clone(),
        notes: todo.notes.clone(),
        outcome: todo
            .notes
            .clone()
            .filter(|notes| !notes.trim().is_empty())
            .unwrap_or_else(|| {
                format!(
                    "{}: {}",
                    if todo.status == TodoStatus::Completed {
                        "Completed"
                    } else {
                        "Cancelled"
                    },
                    todo.content
                )
            }),
        updated_at: None,
    })
}

fn compaction_details_from_todos(todos: &TodoState) -> Option<CompactionHandoffDetails> {
    if todos.todos.is_empty() {
        return None;
    }
    let remaining_todos = todos
        .todos
        .iter()
        .filter(|todo| todo.status != TodoStatus::Completed && todo.status != TodoStatus::Cancelled)
        .cloned()
        .collect::<Vec<_>>();
    let completed_outcomes = todos
        .todos
        .iter()
        .filter_map(todo_outcome)
        .collect::<Vec<_>>();
    Some(CompactionHandoffDetails {
        source: "oppi-runtime".to_string(),
        version: Some(1),
        compacted_at: None,
        remaining_todos,
        completed_outcomes,
    })
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    let mut iter = text.chars();
    let truncated = iter.by_ref().take(max_chars).collect::<String>();
    if iter.next().is_some() {
        format!("{truncated}…")
    } else {
        truncated
    }
}

#[derive(Debug)]
struct AgenticLoopState {
    current: TurnPhase,
}

impl AgenticLoopState {
    fn new() -> Self {
        Self {
            current: TurnPhase::Input,
        }
    }

    fn transition(&mut self, next: TurnPhase) -> Result<(), RuntimeError> {
        if valid_agentic_transition(self.current, next) {
            self.current = next;
            Ok(())
        } else {
            Err(RuntimeError::new(
                "invalid_phase_transition",
                RuntimeErrorCategory::InvalidRequest,
                format!(
                    "invalid 11-phase transition: {:?} -> {:?}",
                    self.current, next
                ),
            ))
        }
    }
}

fn valid_agentic_transition(current: TurnPhase, next: TurnPhase) -> bool {
    use TurnPhase::*;
    matches!(
        (current, next),
        (Input, Message)
            | (Message, History)
            | (History, System)
            | (System, Api)
            | (Api, Tokens)
            | (Tokens, Tools)
            | (Tools, Loop)
            | (Loop, Api)
            | (Loop, Render)
            | (Render, Hooks)
            | (Hooks, Await)
    )
}

fn lookup_tool<'a>(registry: &'a ToolRegistry, call: &ToolCall) -> Option<&'a ToolDefinition> {
    registry
        .get(call.namespace.as_deref(), &call.name)
        .or_else(|| registry.get(Some("oppi"), &call.name))
        .or_else(|| registry.get(Some("pi"), &call.name))
        .or_else(|| registry.get(None, &call.name))
}

fn is_shell_tool(definition: &ToolDefinition, call: &ToolCall) -> bool {
    matches!(call.name.as_str(), "shell_exec" | "bash" | "shell" | "exec")
        || (definition
            .capabilities
            .iter()
            .any(|capability| matches!(capability.as_str(), "process" | "shell" | "sandbox-exec"))
            && !is_shell_task_tool(definition, call))
}

fn is_shell_task_tool(definition: &ToolDefinition, call: &ToolCall) -> bool {
    call.name == "shell_task"
        || definition
            .capabilities
            .iter()
            .any(|capability| capability == "background-task")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FileToolKind {
    Read,
    List,
    Search,
    Write,
    Edit,
}

fn is_todo_tool(definition: &ToolDefinition, call: &ToolCall) -> bool {
    matches!(call.name.as_str(), "todo_write" | "todos")
        || definition
            .capabilities
            .iter()
            .any(|capability| capability == "todos")
}

fn is_ask_user_tool(definition: &ToolDefinition, call: &ToolCall) -> bool {
    call.name == "ask_user"
        || definition
            .capabilities
            .iter()
            .any(|capability| matches!(capability.as_str(), "questions" | "user-input"))
}

fn is_suggest_next_tool(definition: &ToolDefinition, call: &ToolCall) -> bool {
    call.name == "suggest_next_message"
        || definition
            .capabilities
            .iter()
            .any(|capability| matches!(capability.as_str(), "suggestion" | "ghost-suggestion"))
}

fn is_goal_tool(definition: &ToolDefinition, call: &ToolCall) -> bool {
    matches!(
        call.name.as_str(),
        "get_goal" | "create_goal" | "update_goal"
    ) || definition
        .capabilities
        .iter()
        .any(|capability| capability == "goal")
}

fn is_render_mermaid_tool(definition: &ToolDefinition, call: &ToolCall) -> bool {
    call.name == "render_mermaid"
        || definition
            .capabilities
            .iter()
            .any(|capability| capability == "mermaid")
}

fn is_feedback_tool(definition: &ToolDefinition, call: &ToolCall) -> bool {
    call.name == "oppi_feedback_submit"
        || definition
            .capabilities
            .iter()
            .any(|capability| capability == "feedback")
}

fn is_image_gen_tool(definition: &ToolDefinition, call: &ToolCall) -> bool {
    matches!(call.name.as_str(), "image_gen" | "image_generation")
        || definition.capabilities.iter().any(|capability| {
            matches!(
                capability.as_str(),
                "image" | "image-generation" | "image_gen"
            )
        })
}

fn is_agent_tool(definition: &ToolDefinition, call: &ToolCall) -> bool {
    matches!(
        call.name.as_str(),
        "AgentTool" | "agent" | "subagent" | "dispatch_agent"
    ) || definition
        .capabilities
        .iter()
        .any(|capability| matches!(capability.as_str(), "agent" | "subagent"))
}

fn first_string_arg(arguments: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        arguments
            .get(*key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    })
}

fn string_list_arg(arguments: &Value, keys: &[&str]) -> Vec<String> {
    keys.iter()
        .find_map(|key| arguments.get(*key))
        .map(|value| match value {
            Value::Array(items) => items
                .iter()
                .filter_map(Value::as_str)
                .flat_map(|value| value.split(','))
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect(),
            Value::String(value) => value
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect(),
            _ => Vec::new(),
        })
        .unwrap_or_default()
}

fn bool_arg(arguments: &Value, key: &str) -> Option<bool> {
    arguments.get(key).and_then(Value::as_bool)
}

fn u32_arg(arguments: &Value, keys: &[&str]) -> Option<u32> {
    keys.iter().find_map(|key| {
        arguments
            .get(*key)
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
    })
}

fn i64_arg(arguments: &Value, keys: &[&str]) -> Option<i64> {
    keys.iter()
        .find_map(|key| arguments.get(*key).and_then(Value::as_i64))
}

fn goal_tool_budget_arg(arguments: &Value) -> Result<Option<i64>, String> {
    if ["tokenBudget", "token_budget"]
        .iter()
        .any(|key| arguments.get(*key).is_some_and(Value::is_null))
    {
        return Ok(None);
    }
    let token_budget = i64_arg(arguments, &["tokenBudget", "token_budget"]);
    validate_goal_budget(token_budget)?;
    Ok(token_budget)
}

fn permission_mode_arg(arguments: &Value) -> Option<PermissionMode> {
    [
        "permissionMode",
        "permission_mode",
        "permissions",
        "permission",
    ]
    .iter()
    .find_map(|key| arguments.get(*key))
    .and_then(|value| {
        serde_json::from_value::<PermissionMode>(value.clone())
            .ok()
            .or_else(|| match value.as_str().unwrap_or_default() {
                "readonly" | "read_only" | "read-only" => Some(PermissionMode::ReadOnly),
                "default" => Some(PermissionMode::Default),
                "auto-review" | "auto_review" | "autoreview" => Some(PermissionMode::AutoReview),
                "full-access" | "full_access" | "full" => Some(PermissionMode::FullAccess),
                _ => None,
            })
    })
}

fn agent_tool_agent_name(call: &ToolCall) -> String {
    first_string_arg(
        &call.arguments,
        &["agentName", "agent_name", "agent", "name"],
    )
    .unwrap_or_else(|| "general-purpose".to_string())
}

fn agent_tool_task(call: &ToolCall) -> Result<String, (ToolResultStatus, String)> {
    first_string_arg(
        &call.arguments,
        &["task", "prompt", "input", "instructions", "query"],
    )
    .ok_or_else(|| {
        (
            ToolResultStatus::Error,
            "AgentTool requires a non-empty task argument".to_string(),
        )
    })
}

fn resolve_agent_tool_policy(
    agent: &AgentDefinition,
    call: &ToolCall,
) -> Result<ResolvedAgentToolPolicy, (ToolResultStatus, String)> {
    let mut max_turns = u32_arg(&call.arguments, &["maxTurns", "max_turns", "maxIterations"]);
    if max_turns == Some(0) {
        return Err((
            ToolResultStatus::Error,
            "AgentTool maxTurns must be greater than zero".to_string(),
        ));
    }
    if max_turns.is_none() {
        max_turns = Some(3);
    }
    let tool_allowlist = {
        let from_call = string_list_arg(
            &call.arguments,
            &["toolAllowlist", "tool_allowlist", "allowedTools", "tools"],
        );
        if from_call.is_empty() {
            agent.tools.clone()
        } else {
            from_call
        }
    };
    Ok(ResolvedAgentToolPolicy {
        background: bool_arg(&call.arguments, "background").unwrap_or(agent.background),
        role: first_string_arg(&call.arguments, &["role"]).or_else(|| Some("subagent".to_string())),
        model: first_string_arg(&call.arguments, &["model", "modelId", "model_id"])
            .or_else(|| agent.model.clone()),
        effort: first_string_arg(
            &call.arguments,
            &["effort", "reasoningEffort", "reasoning_effort"],
        )
        .or_else(|| agent.effort.clone()),
        permission_mode: match (agent.permission_mode, permission_mode_arg(&call.arguments)) {
            (Some(agent_mode), Some(call_mode)) => Some(min_permission_mode(agent_mode, call_mode)),
            (Some(agent_mode), None) => Some(agent_mode),
            (None, Some(call_mode)) => Some(call_mode),
            (None, None) => None,
        },
        network_policy: network_policy_arg(&call.arguments),
        memory_mode: first_string_arg(&call.arguments, &["memoryMode", "memory_mode", "memory"]),
        tool_allowlist,
        tool_denylist: string_list_arg(
            &call.arguments,
            &["toolDenylist", "tool_denylist", "deniedTools"],
        ),
        isolation: first_string_arg(&call.arguments, &["isolation", "isolationMode"]),
        color: first_string_arg(&call.arguments, &["color", "colour"]),
        skills: string_list_arg(&call.arguments, &["skills", "skillNames", "skill_names"]),
        max_turns,
    })
}

fn resolve_subagent_model_policy(
    policy: &mut ResolvedAgentToolPolicy,
    parent_provider: Option<&DirectModelProviderConfig>,
    task: &str,
) {
    let Some(parent_provider) = parent_provider else {
        return;
    };
    let raw_model = policy.model.as_deref().unwrap_or("auto").trim();
    let alias = raw_model.to_ascii_lowercase();
    let strong = matches!(alias.as_str(), "strong" | "smart" | "smarter" | "complex")
        || (matches!(alias.as_str(), "auto" | "default") && complex_subagent_task(task));
    let coding = alias.is_empty()
        || matches!(
            alias.as_str(),
            "auto" | "default" | "coding" | "coder" | "fast" | "subagent"
        );

    if matches!(alias.as_str(), "inherit" | "main" | "current") {
        policy.model = None;
        return;
    }

    if (strong || coding)
        && let Some(model) = default_subagent_model_for_provider(parent_provider, strong)
    {
        policy.model = Some(model.to_string());
        if policy
            .effort
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .is_none()
        {
            policy.effort = Some(default_subagent_effort_for_model(model, strong).to_string());
        }
    }
}

fn default_subagent_model_for_provider(
    provider: &DirectModelProviderConfig,
    strong: bool,
) -> Option<&'static str> {
    if provider_uses_claude_defaults(provider) {
        Some(if strong {
            CLAUDE_MAIN_DEFAULT_MODEL
        } else {
            CLAUDE_CODING_SUBAGENT_DEFAULT_MODEL
        })
    } else if provider_uses_gpt_defaults(provider) {
        Some(if strong {
            GPT_MAIN_DEFAULT_MODEL
        } else {
            GPT_CODING_SUBAGENT_DEFAULT_MODEL
        })
    } else {
        None
    }
}

fn default_subagent_effort_for_model(model: &str, strong: bool) -> &'static str {
    let id = model.to_ascii_lowercase();
    if id.contains("claude") || id.contains("opus") || id.contains("sonnet") {
        "high"
    } else if strong && id.contains("gpt-5.5") {
        "xhigh"
    } else {
        "high"
    }
}

fn provider_uses_claude_defaults(provider: &DirectModelProviderConfig) -> bool {
    let id = provider.model.to_ascii_lowercase();
    provider.api_key_env.as_deref() == Some(MERIDIAN_API_KEY_ENV)
        || id.contains("claude")
        || id.contains("opus")
        || id.contains("sonnet")
}

fn provider_uses_gpt_defaults(provider: &DirectModelProviderConfig) -> bool {
    matches!(
        provider.kind,
        DirectModelProviderKind::OpenAiCodex | DirectModelProviderKind::GitHubCopilot
    ) || provider.base_url.is_none()
        || provider.model.to_ascii_lowercase().starts_with("gpt-")
}

fn complex_subagent_task(task: &str) -> bool {
    let lower = task.to_ascii_lowercase();
    let word_count = lower.split_whitespace().count();
    word_count >= 48
        || lower.len() >= 280
        || [
            "multi-file",
            "multiple files",
            "whole codebase",
            "architecture",
            "refactor",
            "migration",
            "complex",
            "lengthy",
            "deep",
            "audit",
            "plan",
        ]
        .iter()
        .any(|needle| lower.contains(needle))
}

fn agent_tool_model_steps(
    call: &ToolCall,
) -> Result<Vec<ScriptedModelStep>, (ToolResultStatus, String)> {
    let Some(value) = call
        .arguments
        .get("modelSteps")
        .or_else(|| call.arguments.get("model_steps"))
    else {
        return Ok(Vec::new());
    };
    serde_json::from_value::<Vec<ScriptedModelStep>>(value.clone()).map_err(|error| {
        (
            ToolResultStatus::Error,
            format!("AgentTool modelSteps are invalid: {error}"),
        )
    })
}

fn agent_tool_model_provider(
    call: &ToolCall,
) -> Result<Option<DirectModelProviderConfig>, (ToolResultStatus, String)> {
    let Some(value) = call
        .arguments
        .get("modelProvider")
        .or_else(|| call.arguments.get("model_provider"))
    else {
        return Ok(None);
    };
    serde_json::from_value::<DirectModelProviderConfig>(value.clone())
        .map(Some)
        .map_err(|error| {
            (
                ToolResultStatus::Error,
                format!("AgentTool modelProvider is invalid: {error}"),
            )
        })
}

fn runtime_error_as_tool(error: RuntimeError) -> (ToolResultStatus, String) {
    (ToolResultStatus::Error, error.message)
}

fn permission_rank(mode: PermissionMode) -> u8 {
    match mode {
        PermissionMode::ReadOnly => 0,
        PermissionMode::Default => 1,
        PermissionMode::AutoReview => 2,
        PermissionMode::FullAccess => 3,
    }
}

fn min_permission_mode(left: PermissionMode, right: PermissionMode) -> PermissionMode {
    if permission_rank(left) <= permission_rank(right) {
        left
    } else {
        right
    }
}

fn network_rank(policy: NetworkPolicy) -> u8 {
    match policy {
        NetworkPolicy::Disabled => 0,
        NetworkPolicy::Ask => 1,
        NetworkPolicy::Enabled => 2,
    }
}

fn min_network_policy(left: NetworkPolicy, right: NetworkPolicy) -> NetworkPolicy {
    if network_rank(left) <= network_rank(right) {
        left
    } else {
        right
    }
}

fn network_policy_arg(arguments: &Value) -> Option<NetworkPolicy> {
    ["network", "networkPolicy", "network_policy"]
        .iter()
        .find_map(|key| arguments.get(*key))
        .and_then(|value| serde_json::from_value::<NetworkPolicy>(value.clone()).ok())
}

fn filesystem_for_permission(parent: FilesystemPolicy, mode: PermissionMode) -> FilesystemPolicy {
    if mode == PermissionMode::ReadOnly || parent == FilesystemPolicy::ReadOnly {
        FilesystemPolicy::ReadOnly
    } else {
        parent
    }
}

fn agent_tool_match_token(token: &str, definition: &ToolDefinition) -> bool {
    let token = token.trim();
    if token == "*" {
        return true;
    }
    let normalized = token.to_ascii_lowercase().replace('-', "_");
    let name = definition.name.to_ascii_lowercase().replace('-', "_");
    if normalized == name {
        return true;
    }
    if let Some(namespace) = definition.namespace.as_deref() {
        let namespace = namespace.to_ascii_lowercase().replace('-', "_");
        if normalized == format!("{namespace}::{name}")
            || normalized == format!("{namespace}__{name}")
        {
            return true;
        }
    }
    if definition
        .capabilities
        .iter()
        .any(|capability| capability.to_ascii_lowercase().replace('-', "_") == normalized)
    {
        return true;
    }
    match normalized.as_str() {
        "read" | "ls" => {
            definition
                .capabilities
                .iter()
                .any(|capability| matches!(capability.as_str(), "read" | "filesystem"))
                && !definition
                    .capabilities
                    .iter()
                    .any(|capability| matches!(capability.as_str(), "write" | "edit"))
        }
        "grep" | "find" | "search" => definition
            .capabilities
            .iter()
            .any(|capability| matches!(capability.as_str(), "search" | "filesystem")),
        "write" => definition
            .capabilities
            .iter()
            .any(|capability| capability == "write"),
        "edit" => definition
            .capabilities
            .iter()
            .any(|capability| capability == "edit"),
        "shell" | "exec" | "bash" => definition
            .capabilities
            .iter()
            .any(|capability| matches!(capability.as_str(), "process" | "shell" | "sandbox-exec")),
        _ => false,
    }
}

fn agent_tool_definition_allowed(
    definition: &ToolDefinition,
    allowlist: &[String],
    denylist: &[String],
) -> bool {
    if denylist
        .iter()
        .any(|token| agent_tool_match_token(token, definition))
    {
        return false;
    }
    allowlist
        .iter()
        .any(|token| agent_tool_match_token(token, definition))
}

fn subagent_output_from_events(events: &[Event]) -> Option<String> {
    events.iter().rev().find_map(|event| match &event.kind {
        EventKind::ItemCompleted { item } => match &item.kind {
            ItemKind::AssistantMessage { text } if !text.trim().is_empty() => Some(text.clone()),
            _ => None,
        },
        _ => None,
    })
}

fn subagent_system_prompt(
    agent: &AgentDefinition,
    policy: &ResolvedAgentToolPolicy,
    parent_system_prompt: Option<&str>,
    memory: &MemoryStatus,
    skill_instructions: Option<&str>,
) -> String {
    let mut prompt = String::new();
    if let Some(parent) = parent_system_prompt
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        prompt.push_str(parent);
        prompt.push_str("\n\n---\n");
    }
    prompt.push_str(&format!(
        "You are the native OPPi subagent `{}`.\nDescription: {}\n\nInstructions:\n{}\n\n",
        agent.name, agent.description, agent.instructions
    ));
    prompt.push_str("Execution policy enforced by Rust:\n");
    prompt.push_str(&format!(
        "- allowed tools: {}\n",
        if policy.tool_allowlist.is_empty() {
            "(none)".to_string()
        } else {
            policy.tool_allowlist.join(", ")
        }
    ));
    if !policy.tool_denylist.is_empty() {
        prompt.push_str(&format!(
            "- denied tools: {}\n",
            policy.tool_denylist.join(", ")
        ));
    }
    prompt.push_str(&format!(
        "- permission mode: {}\n",
        policy
            .permission_mode
            .map(|mode| mode.as_str().to_string())
            .unwrap_or_else(|| "inherit".to_string())
    ));
    prompt.push_str(&format!(
        "- memory mode: {}; current memory backend: {}, enabled: {}, count: {}\n",
        policy.memory_mode.as_deref().unwrap_or("inherit"),
        memory.backend,
        memory.enabled,
        memory.memory_count
    ));
    if !policy.skills.is_empty() {
        prompt.push_str(&format!(
            "- requested skills: {}\n",
            policy.skills.join(", ")
        ));
    }
    if let Some(skills) = skill_instructions.filter(|value| !value.trim().is_empty()) {
        prompt.push_str("\nRelevant skill instructions:\n");
        prompt.push_str(skills);
        prompt.push('\n');
    }
    prompt.push_str("Complete only the delegated task. Report concise results and blockers.\n");
    prompt
}

fn skill_source_priority(source: SkillSource) -> u8 {
    match source {
        SkillSource::BuiltIn => 0,
        SkillSource::User => 10,
        SkillSource::Project => 20,
        SkillSource::Cli => 30,
    }
}

fn builtin_skill_candidates() -> Vec<SkillCandidate> {
    [
        ("builtin:imagegen", BUILTIN_IMAGEGEN_SKILL),
        ("builtin:independent", BUILTIN_INDEPENDENT_SKILL),
        ("builtin:mermaid-diagrams", BUILTIN_MERMAID_SKILL),
        ("builtin:graphify", BUILTIN_GRAPHIFY_SKILL),
    ]
    .into_iter()
    .filter_map(|(path, content)| parse_skill_candidate(content, path, SkillSource::BuiltIn))
    .collect()
}

fn parse_skill_candidate(content: &str, path: &str, source: SkillSource) -> Option<SkillCandidate> {
    let (name, description) = parse_skill_frontmatter(content)?;
    let priority = skill_source_priority(source);
    Some(SkillCandidate {
        reference: SkillRef {
            id: name.clone(),
            name,
            description,
            source,
            path: Some(path.to_string()),
            priority,
        },
        content: truncate_chars(content, MAX_SKILL_CONTENT_CHARS),
    })
}

fn parse_skill_frontmatter(content: &str) -> Option<(String, String)> {
    let mut lines = content.lines();
    if lines.next()?.trim() != "---" {
        return None;
    }
    let mut name = None;
    let mut description = None;
    for line in lines {
        let trimmed = line.trim();
        if trimmed == "---" {
            break;
        }
        let Some((key, value)) = trimmed.split_once(':') else {
            continue;
        };
        let value = parse_frontmatter_scalar(value.trim());
        match key.trim() {
            "name" => name = Some(value),
            "description" => description = Some(value),
            _ => {}
        }
    }
    let name = name?.trim().to_string();
    let description = description?.trim().to_string();
    if name.is_empty() || description.is_empty() {
        None
    } else {
        Some((name, description))
    }
}

fn parse_frontmatter_scalar(value: &str) -> String {
    value
        .trim_matches(|ch| ch == '"' || ch == '\'')
        .trim()
        .to_string()
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("OPPI_HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

fn collect_skill_files(
    root: &Path,
    include_root_md: bool,
    depth: usize,
    output: &mut Vec<PathBuf>,
) {
    if output.len() >= 200 || depth > 8 || !root.is_dir() {
        return;
    }
    let root_skill = root.join("SKILL.md");
    if root_skill.is_file() {
        output.push(root_skill);
    }
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        if output.len() >= 200 {
            return;
        }
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            let skill = path.join("SKILL.md");
            if skill.is_file() {
                output.push(skill);
            }
            collect_skill_files(&path, false, depth + 1, output);
        } else if include_root_md
            && depth == 0
            && path.extension().and_then(|ext| ext.to_str()) == Some("md")
        {
            output.push(path);
        }
    }
}

fn project_skill_roots(cwd: &str) -> Vec<(PathBuf, bool, SkillSource)> {
    let mut roots = Vec::new();
    let mut current = PathBuf::from(cwd);
    loop {
        roots.push((
            current.join(".pi").join("skills"),
            true,
            SkillSource::Project,
        ));
        roots.push((
            current.join(".agents").join("skills"),
            false,
            SkillSource::Project,
        ));
        if current.join(".git").exists() || !current.pop() {
            break;
        }
    }
    roots
}

fn user_skill_roots() -> Vec<(PathBuf, bool, SkillSource)> {
    let mut roots = Vec::new();
    for key in ["OPPI_USER_SKILLS_DIR", "OPPI_SKILLS_DIR"] {
        if let Some(value) = std::env::var_os(key) {
            roots.push((PathBuf::from(value), true, SkillSource::User));
        }
    }
    if let Some(home) = home_dir() {
        roots.push((
            home.join(".pi").join("agent").join("skills"),
            true,
            SkillSource::User,
        ));
        roots.push((
            home.join(".agents").join("skills"),
            false,
            SkillSource::User,
        ));
    }
    roots
}

fn file_skill_candidates(roots: Vec<(PathBuf, bool, SkillSource)>) -> Vec<SkillCandidate> {
    let mut candidates = Vec::new();
    for (root, include_root_md, source) in roots {
        let mut files = Vec::new();
        collect_skill_files(&root, include_root_md, 0, &mut files);
        files.sort();
        files.dedup();
        for file in files {
            let Ok(content) = fs::read_to_string(&file) else {
                continue;
            };
            if let Some(candidate) =
                parse_skill_candidate(&content, &file.display().to_string(), source)
            {
                candidates.push(candidate);
            }
        }
    }
    candidates
}

fn resolve_skill_candidates(
    candidates: Vec<SkillCandidate>,
) -> Vec<(SkillCandidate, Vec<SkillRef>)> {
    let mut by_name: BTreeMap<String, Vec<SkillCandidate>> = BTreeMap::new();
    for candidate in candidates {
        by_name
            .entry(candidate.reference.name.clone())
            .or_default()
            .push(candidate);
    }
    let mut resolved = by_name
        .into_values()
        .filter_map(|mut candidates| {
            candidates.sort_by(|left, right| {
                right
                    .reference
                    .priority
                    .cmp(&left.reference.priority)
                    .then_with(|| left.reference.path.cmp(&right.reference.path))
            });
            let active = candidates.remove(0);
            let shadowed = candidates
                .into_iter()
                .map(|candidate| candidate.reference)
                .collect::<Vec<_>>();
            Some((active, shadowed))
        })
        .collect::<Vec<_>>();
    resolved.sort_by(|left, right| left.0.reference.name.cmp(&right.0.reference.name));
    resolved
}

fn skill_relevant(skill: &SkillRef, input: &str, explicit_names: &[String]) -> bool {
    let name = skill.name.to_ascii_lowercase();
    if explicit_names
        .iter()
        .any(|value| value.eq_ignore_ascii_case(&skill.name))
    {
        return true;
    }
    let input = input.to_ascii_lowercase();
    match name.as_str() {
        "imagegen" => [
            "image",
            "draw",
            "generate",
            "render",
            "picture",
            "logo",
            "illustration",
            "edit image",
        ]
        .iter()
        .any(|needle| input.contains(needle)),
        "mermaid-diagrams" => [
            "mermaid",
            "diagram",
            "flowchart",
            "sequence diagram",
            "state machine",
            "architecture map",
        ]
        .iter()
        .any(|needle| input.contains(needle)),
        "independent" => [
            "plan",
            "roadmap",
            "checklist",
            "todo",
            "autonomous",
            "continue",
            "work through",
            "finish line",
        ]
        .iter()
        .any(|needle| input.contains(needle)),
        "graphify" => [
            "graphify",
            "codebase graph",
            "knowledge graph",
            "architecture",
            "dependency",
            "dependencies",
            "cross-module",
            "repo-wide",
            "blast radius",
            "impact analysis",
            "god node",
            "community",
            "module map",
            "call graph",
            "why connected",
        ]
        .iter()
        .any(|needle| input.contains(needle)),
        _ => skill
            .description
            .split(|ch: char| !ch.is_alphanumeric())
            .filter(|word| word.len() >= 6)
            .any(|word| input.contains(&word.to_ascii_lowercase())),
    }
}

fn skill_injection_section(skill: &SkillCandidate) -> String {
    format!(
        "## Skill: {}\nSource: {:?}{}\nDescription: {}\n\n{}\n",
        skill.reference.name,
        skill.reference.source,
        skill
            .reference
            .path
            .as_deref()
            .map(|path| format!(" ({path})"))
            .unwrap_or_default(),
        skill.reference.description,
        skill.content
    )
}

fn image_api_key_env_candidates(explicit: Option<&str>) -> Result<Vec<String>, RuntimeError> {
    if let Some(name) = explicit {
        if provider_api_key_env_allowed(name) {
            return Ok(vec![name.to_string()]);
        }
        return Err(RuntimeError::new(
            "image_api_key_env_not_allowed",
            RuntimeErrorCategory::InvalidRequest,
            "image_gen apiKeyEnv must be an env-reference such as OPPI_IMAGE_API_KEY, OPPI_OPENAI_API_KEY, OPENAI_API_KEY, or OPPI_*_API_KEY",
        ));
    }
    Ok(vec![
        "OPPI_IMAGE_API_KEY".to_string(),
        "OPPI_OPENAI_API_KEY".to_string(),
        "OPENAI_API_KEY".to_string(),
    ])
}

fn image_generation_endpoint(call: &ToolCall) -> String {
    let base = call
        .arguments
        .get("baseUrl")
        .or_else(|| call.arguments.get("base_url"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| std::env::var("OPPI_IMAGE_API_BASE_URL").ok())
        .or_else(|| std::env::var("OPPI_OPENAI_BASE_URL").ok())
        .unwrap_or_else(|| "https://api.openai.com/v1".to_string());
    let base = base.trim_end_matches('/');
    if base.ends_with("/images/generations") {
        base.to_string()
    } else {
        format!("{base}/images/generations")
    }
}

fn image_generation_request_body(
    call: &ToolCall,
    model: &str,
    prompt: &str,
    output_format: &str,
    background: Option<&str>,
) -> Value {
    let mut body = json!({
        "model": model,
        "prompt": prompt,
        "n": 1,
        "output_format": output_format,
    });
    for (arg, wire) in [
        ("size", "size"),
        ("quality", "quality"),
        ("moderation", "moderation"),
    ] {
        if let Some(value) = call.arguments.get(arg).and_then(Value::as_str)
            && !value.trim().is_empty()
        {
            body[wire] = json!(value.trim());
        }
    }
    if let Some(background) = background {
        body["background"] = json!(background);
    }
    body
}

fn default_image_output_path(prompt: &str, output_format: &str) -> String {
    let mut slug = prompt
        .chars()
        .filter_map(|ch| {
            if ch.is_ascii_alphanumeric() {
                Some(ch.to_ascii_lowercase())
            } else if ch.is_whitespace() || matches!(ch, '-' | '_' | '.') {
                Some('-')
            } else {
                None
            }
        })
        .collect::<String>();
    while slug.contains("--") {
        slug = slug.replace("--", "-");
    }
    slug = slug.trim_matches('-').chars().take(48).collect();
    if slug.is_empty() {
        slug = "image".to_string();
    }
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("output/imagegen/{stamp}-{slug}.{output_format}")
}

fn mime_type_for_image_format(format: &str) -> &'static str {
    match format {
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        _ => "image/png",
    }
}

fn image_dimensions(bytes: &[u8], format: &str) -> (Option<u32>, Option<u32>) {
    if matches!(format, "png" | "") && bytes.len() >= 24 && bytes.starts_with(b"\x89PNG\r\n\x1a\n")
    {
        let width = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
        let height = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
        return (Some(width), Some(height));
    }
    (None, None)
}

fn artifact_metadata_from_tool_result(
    call: &ToolCall,
    result: &ToolResult,
) -> Option<ArtifactMetadata> {
    if result.status != ToolResultStatus::Ok {
        return None;
    }
    let output = result.output.as_deref()?;
    let value: Value = serde_json::from_str(output).ok()?;
    let output_path = value
        .get("outputPath")
        .or_else(|| value.get("path"))
        .or_else(|| value.get("file"))
        .and_then(Value::as_str)?
        .to_string();
    let dimensions = value.get("dimensions");
    let width = value
        .get("width")
        .and_then(Value::as_u64)
        .or_else(|| {
            dimensions
                .and_then(|value| value.get("width"))
                .and_then(Value::as_u64)
        })
        .and_then(|value| u32::try_from(value).ok());
    let height = value
        .get("height")
        .and_then(Value::as_u64)
        .or_else(|| {
            dimensions
                .and_then(|value| value.get("height"))
                .and_then(Value::as_u64)
        })
        .and_then(|value| u32::try_from(value).ok());
    let source_images = call
        .arguments
        .get("images")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mask = call
        .arguments
        .get("mask")
        .and_then(Value::as_str)
        .map(str::to_string);
    let mut diagnostics = Vec::new();
    if value.get("endpoint").is_some() || value.get("durationMs").is_some() {
        let mut metadata = BTreeMap::new();
        if let Some(endpoint) = value.get("endpoint").and_then(Value::as_str) {
            metadata.insert("endpoint".to_string(), redacted_endpoint(endpoint));
        }
        if let Some(duration) = value.get("durationMs").and_then(Value::as_u64) {
            metadata.insert("durationMs".to_string(), duration.to_string());
        }
        diagnostics.push(Diagnostic {
            level: DiagnosticLevel::Info,
            message: "image artifact generated".to_string(),
            metadata,
        });
    }
    Some(ArtifactMetadata {
        id: format!("artifact-{}", call.id),
        tool_call_id: call.id.clone(),
        output_path,
        mime_type: value
            .get("mimeType")
            .and_then(Value::as_str)
            .map(str::to_string),
        width,
        height,
        source_images,
        mask,
        backend: value
            .get("backend")
            .and_then(Value::as_str)
            .map(str::to_string),
        model: value
            .get("model")
            .and_then(Value::as_str)
            .map(str::to_string),
        bytes: value.get("bytes").and_then(Value::as_u64),
        diagnostics,
    })
}

fn decode_base64(raw: &str) -> Result<Vec<u8>, String> {
    let mut buffer = 0u32;
    let mut bits = 0u8;
    let mut output = Vec::new();
    for ch in raw.chars().filter(|ch| !ch.is_whitespace()) {
        let value = match ch {
            'A'..='Z' => ch as u8 - b'A',
            'a'..='z' => ch as u8 - b'a' + 26,
            '0'..='9' => ch as u8 - b'0' + 52,
            '+' => 62,
            '/' => 63,
            '=' => break,
            _ => return Err("image_gen backend returned invalid base64 image data".to_string()),
        } as u32;
        buffer = (buffer << 6) | value;
        bits += 6;
        while bits >= 8 {
            bits -= 8;
            output.push(((buffer >> bits) & 0xff) as u8);
        }
    }
    if output.is_empty() {
        return Err("image_gen backend returned empty image data".to_string());
    }
    Ok(output)
}

fn tool_delay_ms(call: &ToolCall) -> Option<u64> {
    call.arguments
        .get("delayMs")
        .or_else(|| call.arguments.get("delay_ms"))
        .and_then(serde_json::Value::as_u64)
        .map(|delay| delay.min(5_000))
        .filter(|delay| *delay > 0)
}

fn file_tool_kind(definition: &ToolDefinition, call: &ToolCall) -> Option<FileToolKind> {
    match call.name.as_str() {
        "read" | "read_file" | "oppi_review_read" => return Some(FileToolKind::Read),
        "ls" | "list" | "list_files" | "oppi_review_ls" => return Some(FileToolKind::List),
        "search" | "search_files" | "grep" | "oppi_review_grep" => {
            return Some(FileToolKind::Search);
        }
        "write" | "write_file" => return Some(FileToolKind::Write),
        "edit" | "edit_file" => return Some(FileToolKind::Edit),
        _ => {}
    }
    if definition
        .capabilities
        .iter()
        .any(|capability| capability == "filesystem")
    {
        if definition
            .capabilities
            .iter()
            .any(|capability| capability == "search")
        {
            return Some(FileToolKind::Search);
        }
        if definition
            .capabilities
            .iter()
            .any(|capability| capability == "write")
        {
            return Some(FileToolKind::Write);
        }
        if definition
            .capabilities
            .iter()
            .any(|capability| capability == "edit")
        {
            return Some(FileToolKind::Edit);
        }
        if definition
            .capabilities
            .iter()
            .any(|capability| capability == "list")
        {
            return Some(FileToolKind::List);
        }
        if definition
            .capabilities
            .iter()
            .any(|capability| capability == "read")
        {
            return Some(FileToolKind::Read);
        }
    }
    None
}

fn required_string_arg<'a>(
    call: &'a ToolCall,
    key: &str,
) -> Result<&'a str, (ToolResultStatus, String)> {
    call.arguments
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            (
                ToolResultStatus::Error,
                format!("file tool requires a string {key} argument"),
            )
        })
}

fn usize_arg(call: &ToolCall, key: &str) -> Option<usize> {
    call.arguments
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
}

fn todo_status(value: &Value) -> Result<TodoStatus, (ToolResultStatus, String)> {
    match value.as_str().unwrap_or_default() {
        "pending" => Ok(TodoStatus::Pending),
        "in_progress" | "in-progress" | "active" => Ok(TodoStatus::InProgress),
        "completed" | "done" => Ok(TodoStatus::Completed),
        "blocked" => Ok(TodoStatus::Blocked),
        "cancelled" | "canceled" => Ok(TodoStatus::Cancelled),
        other => Err((
            ToolResultStatus::Error,
            format!(
                "todo_write invalid status '{other}'; expected pending, in_progress, completed, blocked, or cancelled"
            ),
        )),
    }
}

fn todo_priority(
    value: Option<&Value>,
) -> Result<Option<TodoPriority>, (ToolResultStatus, String)> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    match value.as_str().unwrap_or_default() {
        "low" => Ok(Some(TodoPriority::Low)),
        "medium" => Ok(Some(TodoPriority::Medium)),
        "high" => Ok(Some(TodoPriority::High)),
        other => Err((
            ToolResultStatus::Error,
            format!("todo_write invalid priority '{other}'; expected low, medium, or high"),
        )),
    }
}

fn optional_todo_string(value: Option<&Value>, max_chars: usize) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| truncate_chars(value, max_chars))
}

fn todo_summary_for(todos: &[TodoItem]) -> String {
    let completed = todos
        .iter()
        .filter(|todo| todo.status == TodoStatus::Completed)
        .count();
    let active = todos
        .iter()
        .filter(|todo| todo.status == TodoStatus::InProgress)
        .count();
    let blocked = todos
        .iter()
        .filter(|todo| todo.status == TodoStatus::Blocked)
        .count();
    let mut parts = vec![
        format!("{} total", todos.len()),
        format!("{completed} done"),
    ];
    if active > 0 {
        parts.push(format!("{active} active"));
    }
    if blocked > 0 {
        parts.push(format!("{blocked} blocked"));
    }
    parts.join(", ")
}

fn clean_suggestion_text(raw: &str, max_chars: usize) -> Option<String> {
    let mut text = raw
        .trim()
        .trim_matches(|ch: char| "[](){}\"'`".contains(ch) || ch.is_whitespace())
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if text.is_empty() {
        return None;
    }
    if text.chars().count() > max_chars {
        text = truncate_chars(&text, max_chars.saturating_sub(1));
    }
    Some(text)
}

fn normalize_suggest_next(
    call: &ToolCall,
) -> Result<Option<SuggestedNextMessage>, (ToolResultStatus, String)> {
    let Some(raw_message) = call.arguments.get("message").and_then(Value::as_str) else {
        return Err((
            ToolResultStatus::Error,
            "suggest_next_message requires a message string".to_string(),
        ));
    };
    let Some(message) = clean_suggestion_text(raw_message, 180) else {
        return Ok(None);
    };
    let confidence = call
        .arguments
        .get("confidence")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    if !confidence.is_finite() {
        return Ok(None);
    }
    if confidence < 0.7 {
        return Ok(None);
    }
    let reason = call
        .arguments
        .get("reason")
        .and_then(Value::as_str)
        .and_then(|value| clean_suggestion_text(value, 240));
    Ok(Some(SuggestedNextMessage {
        message,
        confidence: confidence.min(1.0).max(0.0) as f32,
        reason,
    }))
}

fn normalize_ask_user_request(
    request_id: QuestionId,
    call: &ToolCall,
) -> Result<AskUserRequest, (ToolResultStatus, String)> {
    let title = call
        .arguments
        .get("title")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| truncate_chars(value, 160));
    let questions = call
        .arguments
        .get("questions")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            (
                ToolResultStatus::Error,
                "ask_user requires a non-empty questions array".to_string(),
            )
        })?;
    if questions.is_empty() {
        return Err((
            ToolResultStatus::Error,
            "ask_user requires at least one question".to_string(),
        ));
    }
    if questions.len() > 12 {
        return Err((
            ToolResultStatus::Error,
            "ask_user accepts at most 12 questions in one request".to_string(),
        ));
    }

    let mut normalized = Vec::new();
    for (index, raw) in questions.iter().enumerate() {
        let object = raw.as_object().ok_or_else(|| {
            (
                ToolResultStatus::Error,
                format!("ask_user question {} must be an object", index + 1),
            )
        })?;
        let id = object
            .get("id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| truncate_chars(value, 64))
            .unwrap_or_else(|| format!("q{}", index + 1));
        let question = object
            .get("question")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| truncate_chars(value, 500))
            .ok_or_else(|| {
                (
                    ToolResultStatus::Error,
                    format!("ask_user question {} requires a question string", index + 1),
                )
            })?;
        let options = object
            .get("options")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .enumerate()
                    .map(|(option_index, raw_option)| {
                        let option = raw_option.as_object().ok_or_else(|| {
                            (
                                ToolResultStatus::Error,
                                format!(
                                    "ask_user question {} option {} must be an object",
                                    index + 1,
                                    option_index + 1
                                ),
                            )
                        })?;
                        let option_id = option
                            .get("id")
                            .and_then(Value::as_str)
                            .map(str::trim)
                            .filter(|value| !value.is_empty())
                            .map(|value| truncate_chars(value, 64))
                            .unwrap_or_else(|| format!("option-{}", option_index + 1));
                        let label = option
                            .get("label")
                            .and_then(Value::as_str)
                            .map(str::trim)
                            .filter(|value| !value.is_empty())
                            .map(|value| truncate_chars(value, 200))
                            .unwrap_or_else(|| option_id.clone());
                        let description = option
                            .get("description")
                            .and_then(Value::as_str)
                            .map(str::trim)
                            .filter(|value| !value.is_empty())
                            .map(|value| truncate_chars(value, 400));
                        Ok(AskUserOption {
                            id: option_id,
                            label,
                            description,
                        })
                    })
                    .collect::<Result<Vec<_>, (ToolResultStatus, String)>>()
            })
            .transpose()?
            .unwrap_or_default();
        normalized.push(AskUserQuestion {
            id,
            question,
            options,
            allow_custom: object.get("allowCustom").and_then(Value::as_bool),
            default_option_id: object
                .get("defaultOptionId")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(|value| truncate_chars(value, 64)),
            required: object
                .get("required")
                .and_then(Value::as_bool)
                .unwrap_or(true),
        });
    }

    Ok(AskUserRequest {
        id: request_id,
        title,
        questions: normalized,
        tool_call_id: Some(call.id.clone()),
    })
}

fn ask_user_answer_text(answer: &AskUserAnswer) -> String {
    if answer.skipped {
        return "skipped".to_string();
    }
    if let Some(text) = answer.text.as_ref().filter(|text| !text.trim().is_empty()) {
        return format!("custom: {}", truncate_chars(text.trim(), 500));
    }
    if let Some(option_id) = answer.option_id.as_ref() {
        if let Some(label) = answer
            .label
            .as_ref()
            .filter(|label| !label.trim().is_empty())
        {
            return format!("{} ({})", option_id, truncate_chars(label.trim(), 200));
        }
        return option_id.clone();
    }
    answer
        .label
        .as_ref()
        .map(|label| truncate_chars(label.trim(), 200))
        .filter(|label| !label.is_empty())
        .unwrap_or_else(|| "answered".to_string())
}

fn ask_user_response_text(response: &AskUserResponse) -> String {
    if response.cancelled {
        return "User cancelled the questionnaire.".to_string();
    }
    let prefix = if response.timed_out {
        "Questionnaire timed out; unanswered questions used recommended/default answers.\n"
    } else {
        ""
    };
    let answers = response
        .answers
        .iter()
        .map(|answer| format!("{}: {}", answer.question_id, ask_user_answer_text(answer)))
        .collect::<Vec<_>>()
        .join("\n");
    format!("{}{}", prefix, answers)
}

fn parse_mermaid_node_label(token: &str) -> String {
    let trimmed = token
        .trim()
        .trim_matches(|ch: char| ch == ';' || ch == ',')
        .trim();
    for (open, close) in [('[', ']'), ('(', ')'), ('{', '}')] {
        if let Some(start) = trimmed.find(open)
            && let Some(end) = trimmed[start + 1..].find(close)
        {
            let label = trimmed[start + 1..start + 1 + end].trim();
            if !label.is_empty() {
                return label.to_string();
            }
        }
    }
    trimmed
        .trim_matches(|ch: char| ch == '-' || ch == '>' || ch == '<' || ch == '|')
        .trim()
        .to_string()
}

fn render_mermaid_flowchart(lines: &[&str]) -> Option<String> {
    let mut edges = Vec::new();
    let mut standalone = Vec::new();
    let arrows = ["-->", "==>", "-.->", "---"];
    for line in lines {
        let cleaned = line.split("%%").next().unwrap_or_default().trim();
        if cleaned.is_empty()
            || cleaned.starts_with("flowchart")
            || cleaned.starts_with("graph")
            || cleaned.starts_with("subgraph")
            || cleaned == "end"
        {
            continue;
        }
        if let Some((arrow, index)) = arrows
            .iter()
            .filter_map(|arrow| cleaned.find(arrow).map(|index| (*arrow, index)))
            .next()
        {
            let from = parse_mermaid_node_label(&cleaned[..index]);
            let to = parse_mermaid_node_label(&cleaned[index + arrow.len()..]);
            if !from.is_empty() && !to.is_empty() {
                edges.push(format!("  {from} -> {to}"));
            }
        } else {
            let node = parse_mermaid_node_label(cleaned);
            if !node.is_empty() {
                standalone.push(format!("  [{node}]"));
            }
        }
    }
    if edges.is_empty() && standalone.is_empty() {
        None
    } else {
        Some(
            [
                vec![
                    "Mermaid flowchart (simple ASCII fallback)".to_string(),
                    String::new(),
                ],
                edges,
                standalone,
            ]
            .concat()
            .join("\n"),
        )
    }
}

fn render_mermaid_sequence(lines: &[&str]) -> Option<String> {
    let mut participants = BTreeSet::new();
    let mut messages = Vec::new();
    for line in lines {
        let cleaned = line.split("%%").next().unwrap_or_default().trim();
        if cleaned.is_empty() || cleaned.starts_with("sequenceDiagram") {
            continue;
        }
        if let Some(rest) = cleaned.strip_prefix("participant ") {
            let label = rest
                .split_once(" as ")
                .map(|(_, label)| label)
                .unwrap_or(rest)
                .trim();
            if !label.is_empty() {
                participants.insert(label.to_string());
            }
            continue;
        }
        let Some((left, body)) = cleaned.split_once(':') else {
            continue;
        };
        let arrow = ["-->>", "->>", "-->", "->"]
            .iter()
            .find_map(|arrow| left.find(arrow).map(|index| (*arrow, index)));
        if let Some((arrow, index)) = arrow {
            let from = left[..index].trim();
            let to = left[index + arrow.len()..]
                .trim_matches(|ch| ch == '+' || ch == '-')
                .trim();
            if !from.is_empty() && !to.is_empty() {
                participants.insert(from.to_string());
                participants.insert(to.to_string());
                messages.push(format!("  {from} -> {to}: {}", body.trim()));
            }
        }
    }
    if messages.is_empty() {
        None
    } else {
        Some(
            [
                vec![
                    "Mermaid sequence diagram (simple ASCII fallback)".to_string(),
                    format!(
                        "Participants: {}",
                        participants.into_iter().collect::<Vec<_>>().join(", ")
                    ),
                    String::new(),
                ],
                messages,
            ]
            .concat()
            .join("\n"),
        )
    }
}

fn feedback_clamp(value: Option<&Value>, max_chars: usize) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| truncate_chars(value, max_chars))
}

fn feedback_sanitize(text: &str) -> String {
    text.split_whitespace()
        .map(|token| {
            let lower = token.to_lowercase();
            if lower.starts_with("sk-")
                || lower.starts_with("ghp_")
                || lower.starts_with("gho_")
                || lower.contains("token=")
                || lower.contains("password=")
                || lower.contains("secret=")
                || lower.contains("api_key=")
            {
                "<redacted>".to_string()
            } else {
                token.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn feedback_section(title: &str, body: Option<String>) -> String {
    let Some(body) = body else {
        return String::new();
    };
    let body = feedback_sanitize(&body);
    if body.trim().is_empty() {
        String::new()
    } else {
        format!("## {title}\n\n{body}\n\n")
    }
}

fn feedback_list_section(title: &str, value: Option<&Value>) -> String {
    let items = value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str())
                .map(|item| feedback_sanitize(&truncate_chars(item.trim(), 500)))
                .filter(|item| !item.is_empty())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if items.is_empty() {
        String::new()
    } else {
        format!(
            "## {title}\n\n{}\n\n",
            items
                .iter()
                .map(|item| format!("- {item}"))
                .collect::<Vec<_>>()
                .join("\n")
        )
    }
}

fn feedback_title(kind: &str, summary: &str) -> String {
    let prefix = if kind == "bug-report" {
        "[Bug]"
    } else {
        "[Feature]"
    };
    truncate_chars(&format!("{prefix} {summary}"), 140)
}

fn feedback_body(
    kind: &str,
    call: &ToolCall,
    cwd: &str,
) -> Result<(String, String, String), (ToolResultStatus, String)> {
    let summary = feedback_clamp(call.arguments.get("summary"), 160).ok_or_else(|| {
        (
            ToolResultStatus::Error,
            "oppi_feedback_submit requires a summary".to_string(),
        )
    })?;
    let mut missing = Vec::new();
    let mut body = String::new();
    body.push_str(&feedback_section("Summary", Some(summary.clone())));
    if kind == "bug-report" {
        let what = feedback_clamp(call.arguments.get("whatHappened"), 12_000)
            .or_else(|| feedback_clamp(call.arguments.get("description"), 12_000));
        let expected = feedback_clamp(call.arguments.get("expectedBehavior"), 12_000);
        let repro = feedback_clamp(call.arguments.get("reproduction"), 12_000);
        if what.is_none() {
            missing.push("what happened");
        }
        if expected.is_none() {
            missing.push("expected behavior");
        }
        if repro.is_none() {
            missing.push("reproduction/context");
        }
        body.push_str(&feedback_section("What happened", what));
        body.push_str(&feedback_section("Expected behavior", expected));
        body.push_str(&feedback_section("Reproduction / context", repro));
        body.push_str(&feedback_section(
            "Impact",
            feedback_clamp(call.arguments.get("impact"), 12_000),
        ));
    } else {
        let requested = feedback_clamp(call.arguments.get("requestedBehavior"), 12_000)
            .or_else(|| feedback_clamp(call.arguments.get("description"), 12_000));
        let value = feedback_clamp(call.arguments.get("userValue"), 12_000);
        let workflow = feedback_clamp(call.arguments.get("exampleWorkflow"), 12_000);
        let criteria = feedback_list_section(
            "Acceptance criteria",
            call.arguments.get("acceptanceCriteria"),
        );
        if requested.is_none() {
            missing.push("requested behavior");
        }
        if value.is_none() {
            missing.push("why it matters / workflow value");
        }
        if workflow.is_none() && criteria.is_empty() {
            missing.push("example workflow or acceptance criteria");
        }
        body.push_str(&feedback_section("Requested behavior", requested));
        body.push_str(&feedback_section("Why this matters", value));
        body.push_str(&feedback_section("Example workflow", workflow));
        body.push_str(&criteria);
    }
    if !missing.is_empty() {
        return Err((
            ToolResultStatus::Error,
            format!(
                "oppi_feedback_submit needs more context: {}",
                missing.join(", ")
            ),
        ));
    }
    if call
        .arguments
        .get("includeDiagnostics")
        .and_then(Value::as_bool)
        .unwrap_or(true)
    {
        body.push_str(&format!(
            "## Sanitized diagnostics\n\n- cwd: {}\n- platform: {}\n\n",
            feedback_sanitize(cwd),
            std::env::consts::OS
        ));
    }
    body.push_str("---\n\nCreated from OPPi feedback intake. Sensitive values are redacted client-side and again by the intake worker when submitted.\n");
    let repo = feedback_clamp(call.arguments.get("repo"), 200)
        .unwrap_or_else(|| "RemindZ/oppi".to_string());
    Ok((feedback_title(kind, &summary), body, repo))
}

fn render_mermaid_fallback(source: &str) -> Result<String, (ToolResultStatus, String)> {
    let source = source.replace("\r\n", "\n").replace('\r', "\n");
    let lines = source.lines().collect::<Vec<_>>();
    let first = lines
        .iter()
        .map(|line| line.trim())
        .find(|line| !line.is_empty() && !line.starts_with("%%"))
        .unwrap_or_default();
    if first.starts_with("flowchart") || first.starts_with("graph") {
        return render_mermaid_flowchart(&lines).ok_or_else(|| {
            (
                ToolResultStatus::Error,
                "render_mermaid fallback could not parse any flowchart nodes or edges".to_string(),
            )
        });
    }
    if first.starts_with("sequenceDiagram") {
        return render_mermaid_sequence(&lines).ok_or_else(|| {
            (
                ToolResultStatus::Error,
                "render_mermaid fallback could not parse any sequence messages".to_string(),
            )
        });
    }
    Err((
        ToolResultStatus::Error,
        "render_mermaid fallback supports simple flowchart/graph and sequenceDiagram inputs"
            .to_string(),
    ))
}

fn normalize_todo_write_state(call: &ToolCall) -> Result<TodoState, (ToolResultStatus, String)> {
    let items = call
        .arguments
        .get("todos")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            (
                ToolResultStatus::Error,
                "todo_write requires a todos array containing the full current list".to_string(),
            )
        })?;
    let mut seen = BTreeSet::new();
    let mut todos = Vec::new();
    for (index, item) in items.iter().enumerate() {
        let object = item.as_object().ok_or_else(|| {
            (
                ToolResultStatus::Error,
                format!("todo_write item {} must be an object", index + 1),
            )
        })?;
        let raw_id = object
            .get("id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| truncate_chars(value, 64))
            .unwrap_or_else(|| (index + 1).to_string());
        let id = if seen.insert(raw_id.clone()) {
            raw_id
        } else {
            let deduped = format!("{}-{}", raw_id, index + 1);
            seen.insert(deduped.clone());
            deduped
        };
        let content = object
            .get("content")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| truncate_chars(value, 300))
            .unwrap_or_else(|| "Untitled task".to_string());
        let status = todo_status(object.get("status").unwrap_or(&Value::Null))?;
        todos.push(TodoItem {
            id,
            content,
            status,
            priority: todo_priority(object.get("priority"))?,
            phase: optional_todo_string(object.get("phase"), 80),
            notes: optional_todo_string(object.get("notes"), 500),
        });
    }
    let summary = call
        .arguments
        .get("summary")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| truncate_chars(value, 500))
        .unwrap_or_else(|| todo_summary_for(&todos));
    Ok(TodoState { todos, summary })
}

fn todo_write_output(state: &TodoState) -> String {
    let mut lines = vec![format!("Updated todos: {}", state.summary)];
    for todo in &state.todos {
        lines.push(format!(
            "- [{}] {}: {}",
            todo.status.as_str(),
            todo.id,
            todo.content
        ));
    }
    lines.join("\n")
}

fn read_task_tail(path: &Path, max_bytes: usize) -> String {
    let bytes = fs::read(path).unwrap_or_default();
    if bytes.len() <= max_bytes {
        return String::from_utf8_lossy(&bytes).to_string();
    }
    String::from_utf8_lossy(&bytes[bytes.len() - max_bytes..]).to_string()
}

fn file_io_error(error: std::io::Error) -> (ToolResultStatus, String) {
    (
        ToolResultStatus::Error,
        format!("file tool I/O error: {error}"),
    )
}

fn resolve_project_path(cwd: &str, raw: &str) -> Result<PathBuf, (ToolResultStatus, String)> {
    let cwd = lexical_normalize(Path::new(cwd));
    let raw_path = Path::new(raw);
    let path = if raw_path.is_absolute() {
        raw_path.to_path_buf()
    } else {
        cwd.join(raw_path)
    };
    let normalized = lexical_normalize(&path);
    if !normalized.starts_with(&cwd) {
        return Err((
            ToolResultStatus::Denied,
            "path escapes the project working directory".to_string(),
        ));
    }
    Ok(normalized)
}

fn lexical_normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::Normal(part) => normalized.push(part),
        }
    }
    normalized
}

fn default_file_tool_policy(cwd: &str, writes: bool) -> SandboxPolicy {
    default_policy(PermissionProfile {
        mode: if writes {
            PermissionMode::Default
        } else {
            PermissionMode::ReadOnly
        },
        readable_roots: vec![cwd.to_string()],
        writable_roots: if writes {
            vec![cwd.to_string()]
        } else {
            Vec::new()
        },
        filesystem_rules: Vec::new(),
        protected_patterns: Vec::new(),
    })
}

fn default_shell_policy(cwd: &str) -> SandboxPolicy {
    default_policy(PermissionProfile {
        mode: PermissionMode::ReadOnly,
        readable_roots: vec![cwd.to_string()],
        writable_roots: Vec::new(),
        filesystem_rules: Vec::new(),
        protected_patterns: Vec::new(),
    })
}

fn policy_with_cwd_defaults(mut policy: SandboxPolicy, cwd: &str) -> SandboxPolicy {
    if policy.permission_profile.readable_roots.is_empty() {
        policy
            .permission_profile
            .readable_roots
            .push(cwd.to_string());
    }
    if policy.filesystem != FilesystemPolicy::ReadOnly
        && policy.permission_profile.writable_roots.is_empty()
        && policy.filesystem != FilesystemPolicy::Unrestricted
    {
        policy
            .permission_profile
            .writable_roots
            .push(cwd.to_string());
    }
    policy
}

fn file_path_allowed_by_roots(
    path: &Path,
    cwd: &str,
    policy: &SandboxPolicy,
    writes: bool,
) -> bool {
    if policy.filesystem == FilesystemPolicy::Unrestricted {
        return true;
    }
    if writes && policy.filesystem == FilesystemPolicy::ReadOnly {
        return false;
    }
    let mut roots = if writes {
        policy.permission_profile.writable_roots.clone()
    } else if policy.permission_profile.readable_roots.is_empty() {
        policy.permission_profile.writable_roots.clone()
    } else {
        policy.permission_profile.readable_roots.clone()
    };
    if roots.is_empty() {
        roots.push(cwd.to_string());
    }
    let path = lexical_normalize(path);
    roots.iter().any(|root| {
        let root = Path::new(root);
        let root = if root.is_absolute() {
            lexical_normalize(root)
        } else {
            lexical_normalize(&Path::new(cwd).join(root))
        };
        path.starts_with(root)
    })
}

fn protected_path_preflight_denial(
    policy: &SandboxPolicy,
    request: &SandboxExecRequest,
) -> Option<String> {
    match evaluate_exec(policy, request) {
        PolicyDecision::Ask { reason, .. } if reason.contains("protected path") => Some(reason),
        _ => None,
    }
}

fn risk_level_name(risk: RiskLevel) -> &'static str {
    match risk {
        RiskLevel::Low => "low",
        RiskLevel::Medium => "medium",
        RiskLevel::High => "high",
        RiskLevel::Critical => "critical",
    }
}

fn guardian_decision_name(decision: GuardianReviewDecision) -> &'static str {
    match decision {
        GuardianReviewDecision::Ask => "ask",
        GuardianReviewDecision::Deny => "deny",
    }
}

fn guardian_review_result(
    decision: GuardianReviewDecision,
    risk: RiskLevel,
    reason: impl Into<String>,
    call: &ToolCall,
) -> GuardianReviewResult {
    let reason = reason.into();
    let strict_json = json!({
        "decision": guardian_decision_name(decision),
        "risk": risk_level_name(risk),
        "reason": reason,
        "tool": {
            "id": &call.id,
            "namespace": &call.namespace,
            "name": &call.name,
        },
        "reviewer": {
            "kind": "guardian-local",
            "isolated": true,
            "boundedReadOnly": true,
            "toolAllowlist": ["oppi_review_read", "oppi_review_ls", "oppi_review_grep"],
            "strictJson": true,
            "timeoutMs": 750,
            "failClosed": decision == GuardianReviewDecision::Deny,
        }
    })
    .to_string();
    GuardianReviewResult {
        decision,
        risk,
        reason,
        strict_json,
    }
}

fn value_contains_raw_secret(value: &Value) -> bool {
    match value {
        Value::String(text) => string_contains_raw_secret(text),
        Value::Array(items) => items.iter().any(value_contains_raw_secret),
        Value::Object(map) => map.values().any(value_contains_raw_secret),
        _ => false,
    }
}

fn string_contains_raw_secret(text: &str) -> bool {
    text.split_whitespace().any(|token| {
        let lower = token.to_ascii_lowercase();
        lower.starts_with("sk-")
            || lower.starts_with("ghp_")
            || lower.starts_with("gho_")
            || lower.starts_with("xoxb-")
            || lower.starts_with("bearer sk-")
            || lower.contains("token=")
            || lower.contains("password=")
            || lower.contains("secret=")
            || lower.contains("api_key=sk-")
    })
}

fn file_tool_writes(kind: FileToolKind) -> bool {
    matches!(kind, FileToolKind::Write | FileToolKind::Edit)
}

fn guardian_file_tool_request(
    call: &ToolCall,
    kind: FileToolKind,
    cwd: &str,
) -> Result<SandboxExecRequest, String> {
    let Some(raw_path) = first_string_arg(&call.arguments, &["path"]) else {
        return Err("file tool path is missing; auto-review fails closed".to_string());
    };
    let path = resolve_project_path(cwd, &raw_path).map_err(|(_, error)| error)?;
    Ok(SandboxExecRequest {
        command: format!("file-tool:{}", call.name),
        cwd: cwd.to_string(),
        writes_files: file_tool_writes(kind),
        uses_network: false,
        touches_protected_path: false,
        touched_paths: vec![path.to_string_lossy().to_string()],
    })
}

fn guardian_shell_tool_request(call: &ToolCall, cwd: &str) -> Result<SandboxExecRequest, String> {
    let command = first_string_arg(&call.arguments, &["command"])
        .ok_or_else(|| "shell command is missing; auto-review fails closed".to_string())?;
    Ok(SandboxExecRequest {
        command,
        cwd: first_string_arg(&call.arguments, &["cwd"]).unwrap_or_else(|| cwd.to_string()),
        writes_files: call
            .arguments
            .get("writesFiles")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        uses_network: call
            .arguments
            .get("usesNetwork")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        touches_protected_path: call
            .arguments
            .get("touchesProtectedPath")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        touched_paths: call
            .arguments
            .get("touchedPaths")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
    })
}

fn guardian_review_request_for_tool(
    definition: &ToolDefinition,
    call: &ToolCall,
    cwd: &str,
) -> Result<Option<(SandboxPolicy, SandboxExecRequest)>, String> {
    if is_shell_tool(definition, call) {
        return Ok(Some((
            default_shell_policy(cwd),
            guardian_shell_tool_request(call, cwd)?,
        )));
    }
    if let Some(kind) = file_tool_kind(definition, call) {
        return Ok(Some((
            default_file_tool_policy(cwd, file_tool_writes(kind)),
            guardian_file_tool_request(call, kind, cwd)?,
        )));
    }
    Ok(None)
}

#[allow(clippy::too_many_arguments)]
fn search_files_under(
    runtime: &Runtime,
    turn: &Turn,
    call: &ToolCall,
    cwd: &str,
    root: &Path,
    query: &str,
    max_results: usize,
    max_bytes_per_file: usize,
    results: &mut Vec<String>,
) {
    if results.len() >= max_results {
        return;
    }
    if root.is_file() {
        search_one_file(
            runtime,
            turn,
            call,
            cwd,
            root,
            query,
            max_results,
            max_bytes_per_file,
            results,
        );
        return;
    }
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        if results.len() >= max_results {
            return;
        }
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            search_files_under(
                runtime,
                turn,
                call,
                cwd,
                &path,
                query,
                max_results,
                max_bytes_per_file,
                results,
            );
        } else if file_type.is_file() {
            search_one_file(
                runtime,
                turn,
                call,
                cwd,
                &path,
                query,
                max_results,
                max_bytes_per_file,
                results,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn search_one_file(
    runtime: &Runtime,
    turn: &Turn,
    call: &ToolCall,
    cwd: &str,
    path: &Path,
    query: &str,
    max_results: usize,
    max_bytes_per_file: usize,
    results: &mut Vec<String>,
) {
    if results.len() >= max_results
        || runtime
            .preflight_file_access(turn, call, cwd, path, false)
            .is_err()
    {
        return;
    }
    let display = path.display().to_string();
    if display.contains(query) {
        results.push(display.clone());
        if results.len() >= max_results {
            return;
        }
    }
    let Ok(bytes) = fs::read(path) else {
        return;
    };
    let text = String::from_utf8_lossy(&bytes[..bytes.len().min(max_bytes_per_file)]);
    for (index, line) in text.lines().enumerate() {
        if line.contains(query) {
            results.push(format!(
                "{}:{}:{}",
                display,
                index + 1,
                truncate_chars(line.trim(), 240)
            ));
            if results.len() >= max_results {
                return;
            }
        }
    }
}

fn tool_results_by_call_id(
    step: &ScriptedModelStep,
) -> Result<BTreeMap<ToolCallId, ToolResult>, RuntimeError> {
    let mut results = BTreeMap::new();
    for result in &step.tool_results {
        if results
            .insert(result.call_id.clone(), result.clone())
            .is_some()
        {
            return Err(RuntimeError::new(
                "duplicate_tool_result",
                RuntimeErrorCategory::ToolPairing,
                format!("duplicate tool result for call {}", result.call_id),
            ));
        }
    }
    Ok(results)
}

fn cancellation_before_model_reason(
    cancellation: &Option<AgenticCancellation>,
    continuation: u32,
) -> Option<String> {
    let cancellation = cancellation.as_ref()?;
    if cancellation.before_model_continuation == Some(continuation) {
        Some(cancellation.reason.clone())
    } else {
        None
    }
}

fn cancellation_tool_reason(
    cancellation: &Option<AgenticCancellation>,
    call_id: &str,
) -> Option<String> {
    let cancellation = cancellation.as_ref()?;
    if cancellation.tool_call_ids.iter().any(|id| id == call_id) {
        Some(cancellation.reason.clone())
    } else {
        None
    }
}

fn command_context_string<'a>(context: &'a Value, key: &str) -> Option<&'a str> {
    context
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
}

fn render_command_template(template: &str, vars: &[(&str, String)]) -> String {
    vars.iter().fold(template.to_string(), |acc, (key, value)| {
        acc.replace(&format!("{{{{{key}}}}}"), value)
    })
}

fn command_variant_append(params: &CommandPrepareParams, _surface: &str) -> String {
    params
        .prompt_variant_append
        .as_deref()
        .filter(|append| !append.trim().is_empty())
        .map(|append| format!("\n\n{append}"))
        .unwrap_or_default()
}

fn normalize_runtime_command(command: &str) -> String {
    command
        .trim()
        .trim_start_matches('/')
        .to_ascii_lowercase()
        .replace('_', "-")
}

fn prepare_independent_command(
    params: &CommandPrepareParams,
) -> Result<CommandPrepareResult, RuntimeError> {
    let args = params.args.trim();
    if args.is_empty() {
        return Err(RuntimeError::new(
            "command_prepare_missing_args",
            RuntimeErrorCategory::InvalidRequest,
            "/independent requires a plan path or scope, for example /independent @plan.md",
        ));
    }
    let input = format!(
        "Use the independent skill to execute the referenced plan document(s) to completion.\n\nPlan document / scope:\n{args}\n\nOperating mode:\n- First load and follow the full `independent` skill instructions.\n- Read the referenced plan document(s) completely. If an item starts with `@`, resolve it as a file path from the current working directory unless the environment says otherwise.\n- Create and maintain a `todo_write` execution plan.\n- Do not stop after planning; continue through implementation, docs, validation, and final reporting.\n- Ask clarification questions only when genuinely blocked by a product decision, secret/account access, destructive operation, production deploy/publish, or irreversible architectural choice. Use the structured question tool if available.\n- Choose reasonable defaults and keep working when details are underspecified.\n- Run relevant validation before marking work complete.\n- Commit only if the user request or project instructions allow it; never publish or deploy unless explicitly requested.\n\nBegin now.{}",
        command_variant_append(params, "independent-user-prompt-append.md")
    );
    Ok(CommandPrepareResult {
        command: "independent".to_string(),
        input,
        system_prompt_profile: None,
        notes: vec![
            "Host UI may still own @path autocomplete; Rust owns the generated user prompt."
                .to_string(),
        ],
    })
}

fn prepare_init_command(params: &CommandPrepareParams) -> CommandPrepareResult {
    let mut input = INIT_USER_PROMPT.trim().to_string();
    if let Some(existing) = command_context_string(&params.context, "existingAgentsMd") {
        let truncated = truncate_chars(existing, MAX_EXISTING_AGENTS_CHARS);
        input = format!(
            "{input}\n\nAGENTS.md already exists, so refresh it instead of skipping.\n\nClaude Code-style refresh workflow:\n\n1. Inspect the repository as needed.\n2. Draft a fresh AGENTS.md in your working context using the current repository state.\n3. Compare that fresh draft against the existing AGENTS.md below.\n4. Validate old instructions before preserving them; remove or revise stale details.\n5. If important guidance is missing or stale, update AGENTS.md with a concise merged version.\n6. If the existing file is already accurate, say so and leave it unchanged.\n7. End with the required completion status: Result, Added, Changed, Removed, and Validation.\n\nExisting AGENTS.md:\n\n```markdown\n{truncated}\n```"
        );
    }
    input.push_str(&command_variant_append(
        params,
        "init-user-prompt-append.md",
    ));
    CommandPrepareResult {
        command: "init".to_string(),
        input,
        system_prompt_profile: None,
        notes: vec![
            "Host may pass context.existingAgentsMd when refreshing an existing AGENTS.md."
                .to_string(),
        ],
    }
}

fn prepare_feedback_command(params: &CommandPrepareParams, kind: &str) -> CommandPrepareResult {
    let command_name = if kind == "bug-report" {
        "/bug-report"
    } else {
        "/feature-request"
    };
    let required = if kind == "bug-report" {
        "summary, what happened, expected behavior, reproduction/context, and whether diagnostics/logs may be included"
    } else {
        "summary, requested behavior, why it matters, example workflow or acceptance criteria, and whether diagnostics may be included"
    };
    let starter = if params.args.trim().is_empty() {
        format!("The user started {command_name} without a description.")
    } else {
        format!(
            "The user started {command_name} with this description:\n\n{}",
            params.args.trim()
        )
    };
    CommandPrepareResult {
        command: kind.to_string(),
        input: format!(
            "{starter}\n\nYour job: help create a high-quality OPPi {}.\n\nVerify enough context before submitting. Required context: {required}. If anything important is missing, ask concise follow-up questions first. Once you have enough context, call the `oppi_feedback_submit` tool with structured fields. Prefer includeDiagnostics=true. Only set includeLogs=true if the user agrees or logs are clearly needed. Do not include secrets or raw private conversation history.{}",
            if kind == "bug-report" {
                "bug report"
            } else {
                "feature request"
            },
            command_variant_append(params, "feedback-triage-user-prompt-append.md")
        ),
        system_prompt_profile: None,
        notes: vec![
            "Feedback submission itself is handled by the oppi_feedback_submit tool.".to_string(),
        ],
    }
}

fn prepare_review_command(params: &CommandPrepareParams) -> CommandPrepareResult {
    let mode = command_context_string(&params.context, "mode")
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    let args = params.args.trim();
    if mode == "audit"
        || args.eq_ignore_ascii_case("audit")
        || args.to_ascii_lowercase().starts_with("audit ")
    {
        let focus = command_context_string(&params.context, "focus")
            .map(str::to_string)
            .or_else(|| {
                args.strip_prefix("audit")
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
            });
        let input = if let Some(focus) = focus {
            format!(
                "Run the full OPPi codebase audit workflow with this additional user focus:\n\n{focus}\n\nKeep the mandatory `.temp-audit` markdown queue structure, dependency freshness checkpoint, current best-practice baseline, approval gates, and batch fix loop."
            )
        } else {
            "Run the full OPPi codebase audit workflow for this repository.\n\nFirst create or update `.temp-audit/INDEX.md`, `.temp-audit/00-inventory-and-dependencies.md`, and the six category audit files. Catalogue packages, APIs, entrypoints, commands/tools/extensions, dependencies, scripts, runtimes, and current best practices. Research latest stable dependency/tooling versions, then pause with the inventory and discrepancy table before applying updates.\n\nAfter I approve dependency/tooling decisions, continue through the markdown-backed queues for bugs/correctness, duplication/maintainability, reliability/ops/cross-platform, security/data safety, test gaps/regressions, and docs/prompt/catalog drift. Fix approved issues in focused batches and keep the `.temp-audit` files as the detailed source of truth.".to_string()
        };
        return CommandPrepareResult {
            command: "review".to_string(),
            input,
            system_prompt_profile: Some("audit".to_string()),
            notes: vec![
                "Host should append the audit system-prompt profile before the agent turn."
                    .to_string(),
            ],
        };
    }

    let input = if !args.is_empty() && mode.is_empty() {
        args.to_string()
    } else if mode == "base-branch"
        || command_context_string(&params.context, "baseBranch").is_some()
    {
        let branch =
            command_context_string(&params.context, "baseBranch").unwrap_or("<base_branch>");
        if let Some(merge_base) = command_context_string(&params.context, "mergeBaseSha") {
            render_command_template(
                "Review the code changes against the base branch '{{base_branch}}'. The merge base commit for this comparison is {{merge_base_sha}}. Run `git diff {{merge_base_sha}}` to inspect the changes relative to {{base_branch}}. Provide prioritized, actionable findings.",
                &[
                    ("base_branch", branch.to_string()),
                    ("merge_base_sha", merge_base.to_string()),
                ],
            )
        } else {
            render_command_template(
                "Review the code changes against the base branch '{{branch}}'. Start by finding the merge diff between the current branch and {{branch}}'s upstream e.g. (`git merge-base HEAD \"$(git rev-parse --abbrev-ref \"{{branch}}@{upstream}\")\"`), then run `git diff` against that SHA to see what changes we would merge into the {{branch}} branch. Provide prioritized, actionable findings.",
                &[("branch", branch.to_string())],
            )
        }
    } else if mode == "commit" || command_context_string(&params.context, "sha").is_some() {
        let sha = command_context_string(&params.context, "sha").unwrap_or("<sha>");
        if let Some(title) = command_context_string(&params.context, "title") {
            render_command_template(
                "Review the code changes introduced by commit {{sha}} (\"{{title}}\"). Provide prioritized, actionable findings.",
                &[("sha", sha.to_string()), ("title", title.to_string())],
            )
        } else {
            render_command_template(
                "Review the code changes introduced by commit {{sha}}. Provide prioritized, actionable findings.",
                &[("sha", sha.to_string())],
            )
        }
    } else {
        "Review the current code changes (staged, unstaged, and untracked files) and provide prioritized findings.".to_string()
    };

    CommandPrepareResult {
        command: "review".to_string(),
        input,
        system_prompt_profile: Some("review".to_string()),
        notes: vec![
            "Host should append the review system-prompt profile before the agent turn."
                .to_string(),
        ],
    }
}

fn prepare_runtime_command(
    params: CommandPrepareParams,
) -> Result<CommandPrepareResult, RuntimeError> {
    match normalize_runtime_command(&params.command).as_str() {
        "independent" => prepare_independent_command(&params),
        "init" => Ok(prepare_init_command(&params)),
        "bug-report" => Ok(prepare_feedback_command(&params, "bug-report")),
        "feature-request" => Ok(prepare_feedback_command(&params, "feature-request")),
        "review" | "audit" => Ok(prepare_review_command(&CommandPrepareParams {
            command: "review".to_string(),
            args: if normalize_runtime_command(&params.command) == "audit"
                && !params.args.trim_start().starts_with("audit")
            {
                format!("audit {}", params.args).trim().to_string()
            } else {
                params.args.clone()
            },
            context: params.context.clone(),
            prompt_variant_append: params.prompt_variant_append.clone(),
        })),
        other => Err(RuntimeError::new(
            "command_prepare_unknown",
            RuntimeErrorCategory::InvalidRequest,
            format!("command/prepare does not support /{other} yet"),
        )),
    }
}

impl Runtime {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_event_mirror(&mut self, mirror: Option<Arc<Mutex<Vec<Event>>>>) {
        self.event_mirror = mirror;
    }

    pub fn turn_interrupt_registry(&self) -> TurnInterruptRegistry {
        self.turn_interrupts.clone()
    }

    pub fn set_turn_interrupt_registry(&mut self, registry: TurnInterruptRegistry) {
        self.turn_interrupts = registry;
    }

    pub fn initialize(&self, params: InitializeParams) -> InitializeResult {
        let server_capabilities = server_capabilities();
        InitializeResult {
            protocol_version: OPPI_PROTOCOL_VERSION.to_string(),
            min_protocol_version: OPPI_MIN_PROTOCOL_VERSION.to_string(),
            protocol_compatible: params
                .protocol_version
                .as_deref()
                .is_none_or(protocol_version_is_compatible),
            server_name: "oppi-server".to_string(),
            server_version: env!("CARGO_PKG_VERSION").to_string(),
            accepted_client_capabilities: params
                .client_capabilities
                .into_iter()
                .filter(|capability| server_capabilities.contains(capability))
                .collect(),
            server_capabilities,
        }
    }

    pub fn prepare_command(
        &self,
        params: CommandPrepareParams,
    ) -> Result<CommandPrepareResult, RuntimeError> {
        prepare_runtime_command(params)
    }

    pub fn list_threads(&self) -> RuntimeListResult<Thread> {
        RuntimeListResult {
            items: self.threads.values().cloned().collect(),
        }
    }

    pub fn start_thread(&mut self, params: ThreadStartParams) -> ThreadStartResult {
        self.next_thread += 1;
        let thread = Thread {
            id: format!("thread-{}", self.next_thread),
            project: params.project,
            status: ThreadStatus::Active,
            title: params.title,
            forked_from: None,
        };
        self.threads.insert(thread.id.clone(), thread.clone());
        let event = self.event(
            &thread.id,
            None,
            EventKind::ThreadStarted {
                thread: thread.clone(),
            },
        );
        ThreadStartResult {
            thread,
            events: vec![event],
        }
    }

    pub fn resume_thread(&mut self, thread_id: &str) -> Result<ThreadStartResult, RuntimeError> {
        let thread = self.thread(thread_id)?.clone();
        let event = self.event(
            &thread.id,
            None,
            EventKind::ThreadResumed {
                thread: thread.clone(),
            },
        );
        Ok(ThreadStartResult {
            thread,
            events: vec![event],
        })
    }

    pub fn fork_thread(
        &mut self,
        from_thread_id: &str,
        title: Option<String>,
    ) -> Result<ThreadStartResult, RuntimeError> {
        let source = self.thread(from_thread_id)?.clone();
        self.next_thread += 1;
        let thread = Thread {
            id: format!("thread-{}", self.next_thread),
            project: source.project,
            status: ThreadStatus::Active,
            title,
            forked_from: Some(source.id.clone()),
        };
        self.threads.insert(thread.id.clone(), thread.clone());
        let event = self.event(
            &thread.id,
            None,
            EventKind::ThreadForked {
                thread: thread.clone(),
                from_thread_id: source.id,
            },
        );
        Ok(ThreadStartResult {
            thread,
            events: vec![event],
        })
    }

    pub fn rename_thread(
        &mut self,
        thread_id: &str,
        title: String,
    ) -> Result<ThreadStartResult, RuntimeError> {
        let title = normalize_thread_title(&title)?;
        let thread = self
            .threads
            .get_mut(thread_id)
            .ok_or_else(|| not_found("thread", thread_id))?;
        thread.title = title;
        let thread = thread.clone();
        let event = self.event(
            &thread.id,
            None,
            EventKind::ThreadUpdated {
                thread: thread.clone(),
            },
        );
        Ok(ThreadStartResult {
            thread,
            events: vec![event],
        })
    }

    pub fn archive_thread(&mut self, thread_id: &str) -> Result<ThreadStartResult, RuntimeError> {
        let thread = self
            .threads
            .get_mut(thread_id)
            .ok_or_else(|| not_found("thread", thread_id))?;
        thread.status = ThreadStatus::Archived;
        let thread = thread.clone();
        let event = self.event(
            &thread.id,
            None,
            EventKind::ThreadUpdated {
                thread: thread.clone(),
            },
        );
        Ok(ThreadStartResult {
            thread,
            events: vec![event],
        })
    }

    pub fn get_thread_goal(
        &self,
        params: ThreadGoalGetParams,
    ) -> Result<ThreadGoalGetResult, RuntimeError> {
        self.thread(&params.thread_id)?;
        Ok(ThreadGoalGetResult {
            goal: self.goals.get(&params.thread_id).cloned(),
        })
    }

    pub fn set_thread_goal(
        &mut self,
        params: ThreadGoalSetParams,
    ) -> Result<ThreadGoalSetResult, RuntimeError> {
        self.thread(&params.thread_id)?;
        let thread_id = params.thread_id;
        let now_ms = now_millis();
        let mut goal = if let Some(objective) = params.objective {
            let objective = validate_goal_objective(&objective).map_err(invalid_goal_request)?;
            let token_budget = params.token_budget.unwrap_or(None);
            validate_goal_budget(token_budget).map_err(invalid_goal_request)?;
            self.goal_continuations.remove(&thread_id);
            new_goal(
                thread_id.clone(),
                objective,
                params.status.unwrap_or(ThreadGoalStatus::Active),
                token_budget,
                now_ms,
            )
        } else {
            let mut goal = self
                .goals
                .get(&thread_id)
                .cloned()
                .ok_or_else(|| not_found("thread goal", &thread_id))?;
            if let Some(status) = params.status {
                goal.status = status;
            }
            if let Some(token_budget) = params.token_budget {
                validate_goal_budget(token_budget).map_err(invalid_goal_request)?;
                goal.token_budget = token_budget;
            }
            goal.updated_at_ms = now_ms;
            goal
        };
        goal.status = status_after_budget(&goal);
        if goal.status == ThreadGoalStatus::Active {
            self.goal_active_started_at_ms
                .entry(thread_id.clone())
                .or_insert(now_ms);
        } else {
            self.goal_active_started_at_ms.remove(&thread_id);
        }
        self.goals.insert(thread_id.clone(), goal.clone());
        self.event(
            &thread_id,
            None,
            EventKind::ThreadGoalUpdated { goal: goal.clone() },
        );
        Ok(ThreadGoalSetResult { goal })
    }

    pub fn clear_thread_goal(
        &mut self,
        params: ThreadGoalClearParams,
    ) -> Result<ThreadGoalClearResult, RuntimeError> {
        self.thread(&params.thread_id)?;
        let cleared = self.goals.remove(&params.thread_id).is_some();
        self.goal_active_started_at_ms.remove(&params.thread_id);
        self.goal_continuations.remove(&params.thread_id);
        if cleared {
            self.event(
                &params.thread_id,
                None,
                EventKind::ThreadGoalCleared {
                    thread_id: params.thread_id.clone(),
                },
            );
        }
        Ok(ThreadGoalClearResult { cleared })
    }

    pub fn render_thread_goal_continuation_prompt(
        &self,
        thread_id: &str,
    ) -> Result<Option<String>, RuntimeError> {
        self.thread(thread_id)?;
        Ok(self.goals.get(thread_id).and_then(|goal| {
            (goal.status == ThreadGoalStatus::Active).then(|| render_goal_continuation_prompt(goal))
        }))
    }

    pub fn render_thread_goal_budget_limit_prompt(
        &self,
        thread_id: &str,
    ) -> Result<Option<String>, RuntimeError> {
        self.thread(thread_id)?;
        Ok(self.goals.get(thread_id).and_then(|goal| {
            (goal.status == ThreadGoalStatus::BudgetLimited)
                .then(|| render_goal_budget_limit_prompt(goal))
        }))
    }

    pub fn next_thread_goal_continuation(
        &mut self,
        params: ThreadGoalContinuationParams,
    ) -> Result<ThreadGoalContinuationResult, RuntimeError> {
        self.thread(&params.thread_id)?;
        let Some(goal) = self.goals.get(&params.thread_id).cloned() else {
            return Ok(ThreadGoalContinuationResult {
                goal: None,
                prompt: None,
                continuation: None,
                blocked_reason: Some("no active thread goal".to_string()),
            });
        };
        if goal.status != ThreadGoalStatus::Active {
            return Ok(ThreadGoalContinuationResult {
                goal: Some(goal.clone()),
                prompt: None,
                continuation: None,
                blocked_reason: Some(format!(
                    "thread goal is {}; automatic continuation only runs for active goals",
                    thread_goal_status_name(goal.status)
                )),
            });
        }

        let max_continuations = params.max_continuations.unwrap_or(MAX_CONTINUATIONS);
        let continuation = self
            .goal_continuations
            .entry(params.thread_id.clone())
            .and_modify(|value| *value = value.saturating_add(1))
            .or_insert(1);
        let continuation = *continuation;
        if continuation > max_continuations {
            let mut blocked_goal = goal.clone();
            blocked_goal.status = ThreadGoalStatus::Paused;
            blocked_goal.updated_at_ms = now_millis();
            self.goals
                .insert(params.thread_id.clone(), blocked_goal.clone());
            self.goal_active_started_at_ms.remove(&params.thread_id);
            self.event(
                &params.thread_id,
                None,
                EventKind::ThreadGoalUpdated {
                    goal: blocked_goal.clone(),
                },
            );
            return Ok(ThreadGoalContinuationResult {
                goal: Some(blocked_goal),
                prompt: None,
                continuation: Some(continuation),
                blocked_reason: Some(format!(
                    "goal continuation guard reached after {max_continuations} continuation(s)"
                )),
            });
        }

        Ok(ThreadGoalContinuationResult {
            goal: Some(goal.clone()),
            prompt: Some(render_goal_continuation_prompt(&goal)),
            continuation: Some(continuation),
            blocked_reason: None,
        })
    }

    fn begin_goal_turn_accounting(&mut self, thread_id: &str) {
        if self
            .goals
            .get(thread_id)
            .is_some_and(|goal| goal.status == ThreadGoalStatus::Active)
        {
            self.goal_active_started_at_ms
                .entry(thread_id.to_string())
                .or_insert_with(now_millis);
        }
    }

    fn account_active_goal(
        &mut self,
        thread_id: &str,
        token_delta: i64,
        elapsed_seconds: i64,
        emitted: &mut Vec<Event>,
    ) {
        if token_delta <= 0 && elapsed_seconds <= 0 {
            return;
        }
        let now_ms = now_millis();
        let updated = self.goals.get_mut(thread_id).and_then(|goal| {
            if matches!(
                goal.status,
                ThreadGoalStatus::Active | ThreadGoalStatus::BudgetLimited
            ) {
                apply_goal_accounting_delta(goal, token_delta, elapsed_seconds, now_ms);
                Some(goal.clone())
            } else {
                None
            }
        });
        if let Some(goal) = updated {
            emitted.push(self.event(thread_id, None, EventKind::ThreadGoalUpdated { goal }));
        }
    }

    fn finish_goal_turn_accounting(
        &mut self,
        thread_id: &str,
        pause_active: bool,
        emitted: &mut Vec<Event>,
    ) {
        let elapsed_seconds = self
            .goal_active_started_at_ms
            .remove(thread_id)
            .map(|started_ms| {
                let elapsed_ms = now_millis().saturating_sub(started_ms);
                (elapsed_ms / 1_000).min(i64::MAX as u64) as i64
            })
            .unwrap_or(0);
        self.account_active_goal(thread_id, 0, elapsed_seconds, emitted);
        if pause_active {
            let updated = self.goals.get_mut(thread_id).and_then(|goal| {
                if goal.status == ThreadGoalStatus::Active {
                    goal.status = ThreadGoalStatus::Paused;
                    goal.updated_at_ms = now_millis();
                    Some(goal.clone())
                } else {
                    None
                }
            });
            if let Some(goal) = updated {
                emitted.push(self.event(thread_id, None, EventKind::ThreadGoalUpdated { goal }));
            }
        }
    }

    pub fn start_turn(&mut self, params: TurnStartParams) -> Result<TurnStartResult, RuntimeError> {
        self.thread(&params.thread_id)?;
        let next_turn_id = format!("turn-{}", self.next_turn + 1);
        if let Some(simulated) = params.simulated_tool.as_ref() {
            self.validate_tool_batch_items(
                &params.thread_id,
                &next_turn_id,
                std::slice::from_ref(simulated),
            )?;
        }
        self.next_turn += 1;
        let mut turn = Turn {
            id: next_turn_id,
            thread_id: params.thread_id.clone(),
            status: TurnStatus::Running,
            phase: TurnPhase::Input,
            parent_turn_id: None,
        };
        self.turns.insert(turn.id.clone(), turn.clone());
        self.begin_goal_turn_accounting(&turn.thread_id);

        let mut emitted = Vec::new();
        emitted.push(self.event(
            &turn.thread_id,
            Some(&turn.id),
            EventKind::TurnStarted { turn: turn.clone() },
        ));
        let user_item = self.item(
            &turn.thread_id,
            &turn.id,
            ItemKind::UserMessage {
                text: params.input.clone(),
            },
        );
        emitted.push(self.event(
            &turn.thread_id,
            Some(&turn.id),
            EventKind::ItemStarted {
                item: user_item.clone(),
            },
        ));
        emitted.push(self.event(
            &turn.thread_id,
            Some(&turn.id),
            EventKind::ItemCompleted { item: user_item },
        ));

        for phase in [
            TurnPhase::Message,
            TurnPhase::History,
            TurnPhase::System,
            TurnPhase::Api,
            TurnPhase::Tokens,
        ] {
            self.set_phase(&mut turn, phase, &mut emitted);
        }

        if params.requested_continuations > MAX_CONTINUATIONS {
            self.set_phase(&mut turn, TurnPhase::Loop, &mut emitted);
            let diagnostic = Diagnostic {
                level: DiagnosticLevel::Error,
                message: format!(
                    "continuation guard blocked {} requested continuations; max is {MAX_CONTINUATIONS}",
                    params.requested_continuations
                ),
                metadata: BTreeMap::new(),
            };
            emitted.push(self.event(
                &turn.thread_id,
                Some(&turn.id),
                EventKind::Diagnostic { diagnostic },
            ));
            turn.status = TurnStatus::Aborted;
            self.turns.insert(turn.id.clone(), turn.clone());
            emitted.push(self.event(
                &turn.thread_id,
                Some(&turn.id),
                EventKind::TurnAborted {
                    reason: "continuation guard exceeded".to_string(),
                },
            ));
            return Ok(TurnStartResult {
                turn,
                events: emitted,
            });
        }

        self.set_phase(&mut turn, TurnPhase::Tools, &mut emitted);
        if let Some(simulated) = params.simulated_tool {
            emitted.extend(self.record_tool_pair(&turn.thread_id, &turn.id, simulated)?);
        }

        if params.defer_completion {
            self.turns.insert(turn.id.clone(), turn.clone());
            return Ok(TurnStartResult {
                turn,
                events: emitted,
            });
        }

        self.set_phase(&mut turn, TurnPhase::Loop, &mut emitted);
        self.set_phase(&mut turn, TurnPhase::Render, &mut emitted);
        let assistant_text = params.assistant_response.unwrap_or_else(|| {
            format!(
                "OPPi runtime accepted turn {} and represented it as semantic events.",
                turn.id
            )
        });
        let assistant_item = self.item(
            &turn.thread_id,
            &turn.id,
            ItemKind::AssistantMessage {
                text: assistant_text,
            },
        );
        emitted.push(self.event(
            &turn.thread_id,
            Some(&turn.id),
            EventKind::ItemStarted {
                item: assistant_item.clone(),
            },
        ));
        emitted.push(self.event(
            &turn.thread_id,
            Some(&turn.id),
            EventKind::ItemCompleted {
                item: assistant_item,
            },
        ));

        self.set_phase(&mut turn, TurnPhase::Hooks, &mut emitted);
        if let Some(feedback) = params.stop_hook_feedback {
            let diagnostic = Diagnostic {
                level: DiagnosticLevel::Info,
                message: "stop-hook continuation recorded".to_string(),
                metadata: BTreeMap::from([("feedback".to_string(), feedback)]),
            };
            emitted.push(self.event(
                &turn.thread_id,
                Some(&turn.id),
                EventKind::Diagnostic { diagnostic },
            ));
        }
        self.set_phase(&mut turn, TurnPhase::Await, &mut emitted);

        turn.status = TurnStatus::Completed;
        self.turns.insert(turn.id.clone(), turn.clone());
        emitted.push(self.event(
            &turn.thread_id,
            Some(&turn.id),
            EventKind::TurnCompleted {
                turn_id: turn.id.clone(),
            },
        ));

        Ok(TurnStartResult {
            turn,
            events: emitted,
        })
    }

    pub fn run_agentic_turn(
        &mut self,
        params: AgenticTurnParams,
    ) -> Result<AgenticTurnResult, RuntimeError> {
        self.run_agentic_turn_with_parent(params, None, None)
    }

    fn run_agentic_turn_with_parent(
        &mut self,
        params: AgenticTurnParams,
        parent_turn_id: Option<TurnId>,
        restricted_tool_registry: Option<ToolRegistry>,
    ) -> Result<AgenticTurnResult, RuntimeError> {
        self.thread(&params.thread_id)?;
        let input = params.input;
        let follow_up = params.follow_up;
        let sandbox_policy = params.sandbox_policy;
        self.next_turn += 1;
        let mut turn = Turn {
            id: format!("turn-{}", self.next_turn),
            thread_id: params.thread_id.clone(),
            status: TurnStatus::Running,
            phase: TurnPhase::Input,
            parent_turn_id,
        };
        self.turns.insert(turn.id.clone(), turn.clone());

        let mut emitted = Vec::new();
        emitted.push(self.event(
            &turn.thread_id,
            Some(&turn.id),
            EventKind::TurnStarted { turn: turn.clone() },
        ));
        let user_item = self.item(
            &turn.thread_id,
            &turn.id,
            ItemKind::UserMessage {
                text: input.clone(),
            },
        );
        emitted.push(self.event(
            &turn.thread_id,
            Some(&turn.id),
            EventKind::ItemStarted {
                item: user_item.clone(),
            },
        ));
        emitted.push(self.event(
            &turn.thread_id,
            Some(&turn.id),
            EventKind::ItemCompleted { item: user_item },
        ));

        let mut state = AgenticLoopState::new();
        for phase in [TurnPhase::Message, TurnPhase::History, TurnPhase::System] {
            self.transition_agentic_phase(&mut state, &mut turn, phase, &mut emitted)?;
        }
        if let Some(policy) = sandbox_policy {
            let mode = policy.permission_profile.mode;
            self.turn_sandbox_policies.insert(turn.id.clone(), policy);
            emitted.push(self.event(
                &turn.thread_id,
                Some(&turn.id),
                EventKind::Diagnostic {
                    diagnostic: Diagnostic {
                        level: DiagnosticLevel::Info,
                        message: "sandbox policy attached to turn".to_string(),
                        metadata: BTreeMap::from([
                            ("component".to_string(), "permissions".to_string()),
                            ("mode".to_string(), mode.as_str().to_string()),
                        ]),
                    },
                },
            ));
        }
        if let Some(context) = follow_up.as_ref() {
            emitted.push(
                self.event(
                    &turn.thread_id,
                    Some(&turn.id),
                    EventKind::Diagnostic {
                        diagnostic: Diagnostic {
                            level: DiagnosticLevel::Info,
                            message: "follow-up chain context applied".to_string(),
                            metadata: BTreeMap::from([
                                ("component".to_string(), "follow-up".to_string()),
                                (
                                    "chainId".to_string(),
                                    context
                                        .chain_id
                                        .clone()
                                        .unwrap_or_else(|| "unknown".to_string()),
                                ),
                                (
                                    "pendingFollowUps".to_string(),
                                    follow_up_pending_count(context).to_string(),
                                ),
                            ]),
                        },
                    },
                ),
            );
        }
        let approved = params.approved_tool_call_ids.into_iter().collect();
        let tool_registry = restricted_tool_registry
            .unwrap_or_else(|| self.turn_tool_registry(params.tool_definitions));
        let mut model_provider_config =
            append_follow_up_to_model_provider(params.model_provider, follow_up.as_ref(), &input);
        if let Some(config) = model_provider_config.as_mut()
            && let Some((skill_prompt, skills)) =
                self.skill_injection_prompt(&turn.thread_id, &input, &[])?
        {
            config.system_prompt = Some(append_system_prompt(
                config.system_prompt.take(),
                "Relevant OPPi skill instructions",
                &skill_prompt,
            ));
            emitted.push(
                self.event(
                    &turn.thread_id,
                    Some(&turn.id),
                    EventKind::Diagnostic {
                        diagnostic: Diagnostic {
                            level: DiagnosticLevel::Info,
                            message: "skill instructions injected".to_string(),
                            metadata: BTreeMap::from([(
                                "skills".to_string(),
                                skills
                                    .iter()
                                    .map(|skill| skill.name.clone())
                                    .collect::<Vec<_>>()
                                    .join(","),
                            )]),
                        },
                    },
                ),
            );
        }
        let mut provider = model_provider(
            model_provider_config,
            params.model_steps,
            tool_registry.list(),
            None,
        )?;
        self.drive_agentic_turn(
            turn,
            state,
            input,
            provider.as_mut(),
            &tool_registry,
            approved,
            params.cancellation,
            params.max_continuations.unwrap_or(MAX_CONTINUATIONS),
            0,
            Vec::new(),
            emitted,
        )
    }

    pub fn resume_agentic_turn(
        &mut self,
        params: AgenticTurnResumeParams,
    ) -> Result<AgenticTurnResult, RuntimeError> {
        self.thread(&params.thread_id)?;
        let mut turn = self
            .turn_in_thread(&params.thread_id, &params.turn_id)?
            .clone();
        self.begin_goal_turn_accounting(&turn.thread_id);
        let ask_user_response = params.ask_user_response;
        if ask_user_response.is_some() {
            if turn.status != TurnStatus::WaitingForUser {
                return Err(RuntimeError::new(
                    "turn_not_waiting_for_user",
                    RuntimeErrorCategory::TerminalState,
                    format!("turn {} is not waiting for user input", turn.id),
                ));
            }
        } else if turn.status != TurnStatus::WaitingForApproval {
            return Err(RuntimeError::new(
                "turn_not_waiting_for_approval",
                RuntimeErrorCategory::TerminalState,
                format!("turn {} is not waiting for approval", turn.id),
            ));
        }
        let approved: BTreeSet<_> = params.approved_tool_call_ids.into_iter().collect();
        let mut emitted = Vec::new();
        let mut answered_tool_result = None;
        if let Some(response) = ask_user_response {
            let tool_call = {
                let owner = self.question_in_thread_mut(&params.thread_id, &response.request_id)?;
                if owner.turn_id.as_deref() != Some(&params.turn_id) {
                    return Err(RuntimeError::new(
                        "question_turn_mismatch",
                        RuntimeErrorCategory::InvalidRequest,
                        format!(
                            "question {} does not belong to turn {}",
                            response.request_id, params.turn_id
                        ),
                    ));
                }
                if owner.resolved {
                    return Err(already_resolved("question", &response.request_id));
                }
                owner.resolved = true;
                let linked_tool_call_id = owner.tool_call_id.clone();
                owner.tool_call.clone().ok_or_else(|| {
                    RuntimeError::new(
                        "question_missing_tool_call",
                        RuntimeErrorCategory::InvalidRequest,
                        format!(
                            "question {} is not linked to a resumable tool call{}",
                            response.request_id,
                            linked_tool_call_id
                                .as_ref()
                                .map(|id| format!(" (toolCallId: {id})"))
                                .unwrap_or_default()
                        ),
                    )
                })?
            };
            let output = ask_user_response_text(&response);
            let result = ToolResult {
                call_id: tool_call.id.clone(),
                status: ToolResultStatus::Ok,
                output: Some(output),
                error: None,
            };
            emitted.push(self.event(
                &params.thread_id,
                Some(&params.turn_id),
                EventKind::AskUserResolved { response },
            ));
            answered_tool_result = Some((tool_call, result));
        }
        let mut approval_events = Vec::new();
        for (approval_id, approval) in self.approvals.iter_mut() {
            if approval.thread_id == params.thread_id
                && approval.turn_id.as_deref() == Some(&params.turn_id)
                && approval.outcome.is_none()
                && approval
                    .tool_call_id
                    .as_ref()
                    .is_some_and(|call_id| approved.contains(call_id))
            {
                approval.outcome = Some(ApprovalOutcome::Approved);
                approval_events.push(ApprovalDecision {
                    request_id: approval_id.clone(),
                    decision: ApprovalOutcome::Approved,
                    message: Some("approved for agentic turn resume".to_string()),
                });
            }
        }
        for decision in approval_events {
            emitted.push(self.event(
                &params.thread_id,
                Some(&params.turn_id),
                EventKind::ApprovalResolved { decision },
            ));
        }
        turn.status = TurnStatus::Running;
        let state = AgenticLoopState {
            current: TurnPhase::Loop,
        };
        let tool_registry = self.turn_tool_registry(params.tool_definitions);
        if let Some(policy) = params.sandbox_policy {
            self.turn_sandbox_policies.insert(turn.id.clone(), policy);
        }
        let resume_state = self.direct_provider_turns.remove(&params.turn_id);
        let pending_tool_calls = if resume_state.is_some() {
            self.pending_approved_tool_calls(&params.thread_id, &params.turn_id, &approved)
        } else {
            Vec::new()
        };
        let follow_up = if resume_state.is_none() {
            params.follow_up.as_ref()
        } else {
            None
        };
        let model_provider_config = append_follow_up_to_model_provider(
            params.model_provider,
            follow_up,
            "resume approved agentic turn",
        );
        let mut provider = model_provider(
            model_provider_config,
            params.model_steps,
            tool_registry.list(),
            resume_state,
        )?;
        if let Some((tool_call, result)) = answered_tool_result {
            emitted.push(self.event(
                &params.thread_id,
                Some(&params.turn_id),
                EventKind::ToolCallCompleted {
                    result: result.clone(),
                },
            ));
            provider.observe_tool_result(&tool_call, &result)?;
        }
        self.drive_agentic_turn(
            turn,
            state,
            "resume approved agentic turn".to_string(),
            provider.as_mut(),
            &tool_registry,
            approved,
            params.cancellation,
            params.max_continuations.unwrap_or(MAX_CONTINUATIONS),
            0,
            pending_tool_calls,
            emitted,
        )
    }

    pub fn record_tool(
        &mut self,
        params: ToolRecordParams,
    ) -> Result<EventsListResult, RuntimeError> {
        self.thread(&params.thread_id)?;
        let turn = self.turn_in_thread(&params.thread_id, &params.turn_id)?;
        ensure_turn_mutable(turn)?;
        let simulated = SimulatedToolUse {
            call: params.call,
            result: params.result,
            require_approval: false,
            concurrency_safe: false,
        };
        let events = self.record_tool_pair(&params.thread_id, &params.turn_id, simulated)?;
        Ok(EventsListResult { events })
    }

    pub fn record_tool_batch(
        &mut self,
        params: ToolBatchRecordParams,
    ) -> Result<ToolBatchRecordResult, RuntimeError> {
        self.thread(&params.thread_id)?;
        let turn = self.turn_in_thread(&params.thread_id, &params.turn_id)?;
        ensure_turn_mutable(turn)?;
        if params.tools.is_empty() {
            return Ok(ToolBatchRecordResult {
                batches: Vec::new(),
                events: Vec::new(),
            });
        }

        let planned = partition_ordered_tool_batches(params.tools);
        for batch in &planned {
            self.validate_tool_batch_items(&params.thread_id, &params.turn_id, &batch.tools)?;
        }
        let mut batches = Vec::new();
        let mut events = Vec::new();
        for (index, batch) in planned.into_iter().enumerate() {
            let batch_id = format!("{}-batch-{}", params.turn_id, index + 1);
            let descriptor = describe_tool_batch(batch_id.clone(), &batch, params.max_concurrency);
            events.push(self.event(
                &params.thread_id,
                Some(&params.turn_id),
                EventKind::ToolBatchStarted {
                    batch: descriptor.clone(),
                },
            ));
            events.extend(self.record_tool_batch_items(
                &params.thread_id,
                &params.turn_id,
                batch.tools,
                batch.execution,
            )?);
            let status = if descriptor.tool_call_ids.is_empty() {
                ToolBatchStatus::Aborted
            } else {
                ToolBatchStatus::Completed
            };
            events.push(self.event(
                &params.thread_id,
                Some(&params.turn_id),
                EventKind::ToolBatchCompleted {
                    batch_id: batch_id.clone(),
                    status,
                },
            ));
            batches.push(descriptor);
        }
        Ok(ToolBatchRecordResult { batches, events })
    }

    pub fn steer_turn(
        &mut self,
        thread_id: &str,
        turn_id: &str,
        input: String,
    ) -> Result<EventsListResult, RuntimeError> {
        self.thread(thread_id)?;
        let turn = self.turn_in_thread(thread_id, turn_id)?.clone();
        ensure_turn_mutable(&turn)?;
        let item = self.item(thread_id, &turn.id, ItemKind::UserMessage { text: input });
        let event = self.event(thread_id, Some(&turn.id), EventKind::ItemCompleted { item });
        Ok(EventsListResult {
            events: vec![event],
        })
    }

    pub fn interrupt_turn(
        &mut self,
        thread_id: &str,
        turn_id: &str,
        reason: String,
    ) -> Result<EventsListResult, RuntimeError> {
        self.thread(thread_id)?;
        let mut turn = self.turn_in_thread(thread_id, turn_id)?.clone();
        ensure_turn_mutable(&turn)?;
        turn.status = TurnStatus::Aborted;
        self.direct_provider_turns.remove(&turn.id);
        let _ = self.turn_interrupts.take(&turn.id);
        self.turns.insert(turn.id.clone(), turn.clone());
        let event = self.event(
            thread_id,
            Some(&turn.id),
            EventKind::TurnInterrupted { reason },
        );
        Ok(EventsListResult {
            events: vec![event],
        })
    }

    pub fn request_approval(
        &mut self,
        thread_id: &str,
        mut request: ApprovalRequest,
    ) -> Result<EventsListResult, RuntimeError> {
        self.thread(thread_id)?;
        if request.id.is_empty() {
            self.next_approval += 1;
            request.id = format!("approval-{}", self.next_approval);
        }
        if self.approvals.contains_key(&request.id) {
            return Err(already_exists("approval", &request.id));
        }
        self.approvals.insert(
            request.id.clone(),
            OwnedApprovalRequest {
                thread_id: thread_id.to_string(),
                turn_id: None,
                tool_call_id: request.tool_call.as_ref().map(|call| call.id.clone()),
                tool_call: request.tool_call.clone(),
                outcome: None,
            },
        );
        let event = self.event(thread_id, None, EventKind::ApprovalRequested { request });
        Ok(EventsListResult {
            events: vec![event],
        })
    }

    pub fn respond_approval(
        &mut self,
        params: ApprovalRespondParams,
    ) -> Result<EventsListResult, RuntimeError> {
        self.thread(&params.thread_id)?;
        let turn_id = {
            let owner = self.approval_in_thread_mut(&params.thread_id, &params.request_id)?;
            if owner.outcome.is_some() {
                return Err(already_resolved("approval", &params.request_id));
            }
            owner.outcome = Some(params.decision);
            owner.turn_id.clone()
        };
        let decision = ApprovalDecision {
            request_id: params.request_id,
            decision: params.decision,
            message: params.message,
        };
        let event = self.event(
            &params.thread_id,
            turn_id.as_deref(),
            EventKind::ApprovalResolved { decision },
        );
        Ok(EventsListResult {
            events: vec![event],
        })
    }

    pub fn request_question(
        &mut self,
        thread_id: &str,
        mut request: QuestionRequest,
    ) -> Result<EventsListResult, RuntimeError> {
        self.thread(thread_id)?;
        if request.id.is_empty() {
            self.next_question += 1;
            request.id = format!("question-{}", self.next_question);
        }
        if self.questions.contains_key(&request.id) {
            return Err(already_exists("question", &request.id));
        }
        self.questions.insert(
            request.id.clone(),
            OwnedQuestionRequest {
                thread_id: thread_id.to_string(),
                turn_id: None,
                tool_call_id: None,
                tool_call: None,
                resolved: false,
            },
        );
        let event = self.event(thread_id, None, EventKind::QuestionRequested { request });
        Ok(EventsListResult {
            events: vec![event],
        })
    }

    pub fn respond_question(
        &mut self,
        params: QuestionRespondParams,
    ) -> Result<EventsListResult, RuntimeError> {
        self.thread(&params.thread_id)?;
        {
            let owner = self.question_in_thread_mut(&params.thread_id, &params.request_id)?;
            if owner.resolved {
                return Err(already_resolved("question", &params.request_id));
            }
            owner.resolved = true;
        }
        let response = QuestionResponse {
            request_id: params.request_id,
            answer: params.answer,
        };
        let event = self.event(
            &params.thread_id,
            None,
            EventKind::QuestionResolved { response },
        );
        Ok(EventsListResult {
            events: vec![event],
        })
    }

    pub fn register_plugin(
        &mut self,
        thread_id: &str,
        plugin: PluginRef,
    ) -> Result<EventsListResult, RuntimeError> {
        self.thread(thread_id)?;
        self.plugins.insert(plugin.id.clone(), plugin.clone());
        let event = self.event(thread_id, None, EventKind::PluginRegistered { plugin });
        Ok(EventsListResult {
            events: vec![event],
        })
    }

    pub fn list_plugins(&self) -> RuntimeListResult<PluginRef> {
        RuntimeListResult {
            items: self.plugins.values().cloned().collect(),
        }
    }

    pub fn set_memory_status(
        &mut self,
        params: MemorySetParams,
    ) -> Result<EventsListResult, RuntimeError> {
        self.thread(&params.thread_id)?;
        self.memory = params.status.clone();
        let event = self.event(
            &params.thread_id,
            None,
            EventKind::MemoryStatusChanged {
                status: params.status,
            },
        );
        Ok(EventsListResult {
            events: vec![event],
        })
    }

    pub fn memory_status(&self) -> MemoryStatus {
        self.memory.clone()
    }

    pub fn memory_control(
        &mut self,
        params: MemoryControlParams,
    ) -> Result<MemoryControlResult, RuntimeError> {
        self.thread(&params.thread_id)?;
        let action = params.action.trim().to_ascii_lowercase();
        let status = self.memory.clone();
        let mut controls = vec![
            MemoryControl {
                id: "dashboard".to_string(),
                label: "Open dashboard".to_string(),
                command: "/memory dashboard".to_string(),
                description: Some("Show client-hosted Hoppi status and safe actions.".to_string()),
            },
            MemoryControl {
                id: "enable".to_string(),
                label: "Enable memory".to_string(),
                command: "/memory on".to_string(),
                description: Some(
                    "Use the client-hosted Hoppi bridge when the host provides it.".to_string(),
                ),
            },
            MemoryControl {
                id: "disable".to_string(),
                label: "Disable memory".to_string(),
                command: "/memory off".to_string(),
                description: Some("Disable memory recall/write for native turns.".to_string()),
            },
            MemoryControl {
                id: "maintenance-dry-run".to_string(),
                label: "Preview maintenance".to_string(),
                command: "/memory maintenance dry-run".to_string(),
                description: Some(
                    "Explicitly preview Hoppi maintenance; no hidden idle model session."
                        .to_string(),
                ),
            },
        ];
        let (title, summary) = match action.as_str() {
            "dashboard" | "status" => (
                "Hoppi memory dashboard".to_string(),
                format!(
                    "Memory is {} with backend '{}' in {} scope ({} memories). Hoppi remains client-hosted; Rust exposes status and controls only.",
                    if status.enabled {
                        "enabled"
                    } else {
                        "disabled"
                    },
                    status.backend,
                    status.scope,
                    status.memory_count
                ),
            ),
            "settings" => {
                controls.push(MemoryControl {
                    id: "compact".to_string(),
                    label: "Record compact summary".to_string(),
                    command: "/memory compact <summary>".to_string(),
                    description: Some("Persist an explicit handoff summary event; does not invoke hidden memory workers.".to_string()),
                });
                (
                    "Hoppi memory settings".to_string(),
                    "Native settings are explicit controls over the client-hosted Hoppi bridge: enable, disable, compact, and maintenance preview/apply.".to_string(),
                )
            }
            "maintenance" | "maintain" => {
                controls.push(MemoryControl {
                    id: "maintenance-apply".to_string(),
                    label: "Apply maintenance".to_string(),
                    command: "/memory maintenance apply".to_string(),
                    description: Some("Explicit operator action; Rust records intent and never starts hidden idle/deep sessions.".to_string()),
                });
                if params.apply {
                    (
                        "Hoppi memory maintenance".to_string(),
                        "Explicit maintenance apply requested. Native Rust recorded the request for the client-hosted Hoppi bridge; no hidden model session was started.".to_string(),
                    )
                } else {
                    (
                        "Hoppi memory maintenance preview".to_string(),
                        "Dry-run maintenance preview: native Rust reports controls and status only; no memories were changed and no hidden model session was started.".to_string(),
                    )
                }
            }
            other => {
                return Err(RuntimeError::new(
                    "memory_control_unknown_action",
                    RuntimeErrorCategory::InvalidRequest,
                    format!("unknown memory control action: {other}"),
                ));
            }
        };
        let mut metadata = BTreeMap::new();
        metadata.insert("action".to_string(), action);
        metadata.insert("enabled".to_string(), status.enabled.to_string());
        metadata.insert("backend".to_string(), status.backend.clone());
        metadata.insert("scope".to_string(), status.scope.clone());
        metadata.insert("memoryCount".to_string(), status.memory_count.to_string());
        metadata.insert("clientHosted".to_string(), "true".to_string());
        let diagnostic = Diagnostic {
            level: DiagnosticLevel::Info,
            message: "memory control action".to_string(),
            metadata,
        };
        let event = self.event(
            &params.thread_id,
            None,
            EventKind::Diagnostic {
                diagnostic: diagnostic.clone(),
            },
        );
        Ok(MemoryControlResult {
            title,
            summary,
            status,
            controls,
            diagnostics: vec![diagnostic],
            events: vec![event],
        })
    }

    pub fn todos_state(&self) -> TodoListResult {
        TodoListResult {
            state: self.todos.clone(),
        }
    }

    pub fn apply_todo_client_action(
        &mut self,
        params: TodoClientActionParams,
    ) -> Result<TodoClientActionResult, RuntimeError> {
        self.thread(&params.thread_id)?;

        let mut state = self.todos.clone();
        match params.action {
            TodoClientAction::Clear => {
                state.todos.clear();
                state.summary = "Client cleared todos".to_string();
            }
            TodoClientAction::Done => match params.id {
                Some(id) => {
                    let todo = state
                        .todos
                        .iter_mut()
                        .find(|todo| todo.id == id)
                        .ok_or_else(|| not_found("todo", &id))?;
                    todo.status = TodoStatus::Completed;
                    state.summary = format!("Client marked todo {id} done");
                }
                None => {
                    let mut completed = 0usize;
                    for todo in &mut state.todos {
                        if todo.status != TodoStatus::Completed
                            && todo.status != TodoStatus::Cancelled
                        {
                            todo.status = TodoStatus::Completed;
                            completed += 1;
                        }
                    }
                    state.summary = if completed == 0 {
                        "Client found no active todos to mark done".to_string()
                    } else {
                        format!("Client marked {completed} todo(s) done")
                    };
                }
            },
        }

        self.todos = state.clone();
        let event = self.event(
            &params.thread_id,
            None,
            EventKind::TodosUpdated {
                state: state.clone(),
            },
        );
        Ok(TodoClientActionResult {
            state,
            events: vec![event],
        })
    }

    pub fn compact_handoff(
        &mut self,
        params: HandoffCompactParams,
    ) -> Result<EventsListResult, RuntimeError> {
        self.thread(&params.thread_id)?;
        let details = params
            .details
            .or_else(|| compaction_details_from_todos(&self.todos));
        let event = self.event(
            &params.thread_id,
            None,
            EventKind::HandoffCompacted {
                summary: params.summary,
                details,
            },
        );
        Ok(EventsListResult {
            events: vec![event],
        })
    }

    pub fn register_mcp(
        &mut self,
        thread_id: &str,
        server: McpServerRef,
    ) -> Result<EventsListResult, RuntimeError> {
        self.thread(thread_id)?;
        self.mcp_servers.insert(server.id.clone(), server.clone());
        let event = self.event(thread_id, None, EventKind::McpServerRegistered { server });
        Ok(EventsListResult {
            events: vec![event],
        })
    }

    pub fn list_mcp(&self) -> RuntimeListResult<McpServerRef> {
        RuntimeListResult {
            items: self.mcp_servers.values().cloned().collect(),
        }
    }

    pub fn mcp_action(
        &mut self,
        params: McpActionParams,
    ) -> Result<EventsListResult, RuntimeError> {
        self.thread(&params.thread_id)?;
        if !self.mcp_servers.contains_key(&params.server_id) {
            return Err(not_found("mcp server", &params.server_id));
        }
        let action = match params.action {
            McpAction::Reload => "reload",
            McpAction::Test => "test",
            McpAction::Auth => "auth",
        };
        let diagnostic = Diagnostic {
            level: DiagnosticLevel::Info,
            message: format!("MCP {action} flow recorded for {}", params.server_id),
            metadata: BTreeMap::from([("serverId".to_string(), params.server_id)]),
        };
        let event = self.event(
            &params.thread_id,
            None,
            EventKind::Diagnostic { diagnostic },
        );
        Ok(EventsListResult {
            events: vec![event],
        })
    }

    pub fn register_model(
        &mut self,
        thread_id: &str,
        model: ModelRef,
    ) -> Result<EventsListResult, RuntimeError> {
        self.thread(thread_id)?;
        self.models.insert(model.id.clone(), model.clone());
        let event = self.event(thread_id, None, EventKind::ModelRegistered { model });
        Ok(EventsListResult {
            events: vec![event],
        })
    }

    pub fn list_models(&self) -> RuntimeListResult<ModelRef> {
        RuntimeListResult {
            items: self.models.values().cloned().collect(),
        }
    }

    pub fn select_model(
        &mut self,
        params: ModelSelectParams,
    ) -> Result<EventsListResult, RuntimeError> {
        self.thread(&params.thread_id)?;
        if !self.models.contains_key(&params.model_id) {
            return Err(not_found("model", &params.model_id));
        }
        self.selected_model = Some(params.model_id.clone());
        let event = self.event(
            &params.thread_id,
            None,
            EventKind::ModelSelected {
                model_id: params.model_id,
            },
        );
        Ok(EventsListResult {
            events: vec![event],
        })
    }

    pub fn register_agent(
        &mut self,
        thread_id: &str,
        agent: AgentDefinition,
    ) -> Result<EventsListResult, RuntimeError> {
        self.thread(thread_id)?;
        let definitions = self.agents.entry(agent.name.clone()).or_default();
        definitions.push(agent.clone());
        let diagnostic = Diagnostic {
            level: DiagnosticLevel::Info,
            message: format!("registered agent {}", agent.name),
            metadata: BTreeMap::from([("description".to_string(), agent.description)]),
        };
        let event = self.event(thread_id, None, EventKind::Diagnostic { diagnostic });
        Ok(EventsListResult {
            events: vec![event],
        })
    }

    pub fn list_agents(&self) -> RuntimeListResult<ResolvedAgent> {
        RuntimeListResult {
            items: resolve_active_agents(
                self.agents
                    .values()
                    .flat_map(|definitions| definitions.iter().cloned())
                    .collect(),
            ),
        }
    }

    pub fn list_skills(
        &self,
        params: SkillListParams,
    ) -> Result<RuntimeListResult<ResolvedSkill>, RuntimeError> {
        let cwd = if let Some(thread_id) = params.thread_id.as_deref() {
            Some(self.thread(thread_id)?.project.cwd.clone())
        } else {
            None
        };
        let items = resolve_skill_candidates(self.discover_skill_candidates(cwd.as_deref()))
            .into_iter()
            .map(|(active, shadowed)| ResolvedSkill {
                active: active.reference,
                shadowed,
            })
            .collect();
        Ok(RuntimeListResult { items })
    }

    pub fn dispatch_agent(
        &mut self,
        params: AgentDispatchParams,
    ) -> Result<AgentDispatchResult, RuntimeError> {
        self.thread(&params.thread_id)?;
        let agent = self.active_agent(&params.agent_name)?;
        let tool_allowlist = if params.tool_allowlist.is_empty() {
            agent.tools.clone()
        } else {
            params.tool_allowlist.clone()
        };
        let permission_mode = params.permission_mode.or(agent.permission_mode);
        self.next_agent_run += 1;
        let run = AgentRun {
            id: format!("agent-run-{}", self.next_agent_run),
            thread_id: params.thread_id.clone(),
            agent_name: agent.name,
            status: AgentRunStatus::Running,
            task: params.task,
            worktree_root: params.worktree_root.or(agent.worktree_root),
            background: params.background || agent.background,
            role: params.role,
            model: params.model.or(agent.model),
            effort: params.effort.or(agent.effort),
            permission_mode,
            memory_mode: params.memory_mode,
            tool_allowlist,
            tool_denylist: params.tool_denylist,
            isolation: params.isolation,
            color: params.color,
            skills: params.skills,
            max_turns: params.max_turns,
        };
        self.agent_runs.insert(run.id.clone(), run.clone());
        let event = self.event(
            &params.thread_id,
            None,
            EventKind::AgentStarted { run: run.clone() },
        );
        Ok(AgentDispatchResult {
            run,
            events: vec![event],
        })
    }

    pub fn block_agent(
        &mut self,
        params: AgentBlockParams,
    ) -> Result<EventsListResult, RuntimeError> {
        self.thread(&params.thread_id)?;
        self.agent_run_in_thread(&params.thread_id, &params.run_id)?;
        let run = self.agent_run_mut(&params.run_id)?;
        run.status = AgentRunStatus::Blocked;
        let event = self.event(
            &params.thread_id,
            None,
            EventKind::AgentBlocked {
                run_id: params.run_id,
                reason: params.reason,
            },
        );
        Ok(EventsListResult {
            events: vec![event],
        })
    }

    pub fn complete_agent(
        &mut self,
        params: AgentCompleteParams,
    ) -> Result<EventsListResult, RuntimeError> {
        self.thread(&params.thread_id)?;
        self.agent_run_in_thread(&params.thread_id, &params.run_id)?;
        let run = self.agent_run_mut(&params.run_id)?;
        run.status = AgentRunStatus::Completed;
        let event = self.event(
            &params.thread_id,
            None,
            EventKind::AgentCompleted {
                run_id: params.run_id,
                output: params.output,
            },
        );
        Ok(EventsListResult {
            events: vec![event],
        })
    }

    pub fn side_question(
        &mut self,
        params: SideQuestionParams,
    ) -> Result<SideQuestionResult, RuntimeError> {
        self.thread(&params.thread_id)?;
        let answer = format!(
            "Side question answered from the current thread snapshot without mutating the parent transcript: {}",
            params.question
        );
        let event = self.event(
            &params.thread_id,
            None,
            EventKind::SideQuestionAnswered {
                question: params.question,
                answer: answer.clone(),
            },
        );
        Ok(SideQuestionResult {
            answer,
            events: vec![event],
        })
    }

    pub fn bridge_pi_event(
        &mut self,
        params: PiBridgeEventParams,
    ) -> Result<EventsListResult, RuntimeError> {
        self.thread(&params.thread_id)?;
        let event = self.event(
            &params.thread_id,
            None,
            EventKind::PiAdapterEvent {
                name: params.name,
                payload: params.payload,
            },
        );
        Ok(EventsListResult {
            events: vec![event],
        })
    }

    pub fn events_after(&self, params: EventsListParams) -> Result<EventsListResult, RuntimeError> {
        self.thread(&params.thread_id)?;
        let limit = params
            .limit
            .unwrap_or(DEFAULT_EVENTS_LIST_LIMIT)
            .min(MAX_EVENTS_LIST_LIMIT);
        let events = self
            .event_store
            .list_after_limit(&params.thread_id, params.after, limit)
            .map_err(event_store_error)?;
        Ok(EventsListResult { events })
    }

    pub fn metrics(&self) -> RuntimeMetrics {
        let mut turn_status_counts = BTreeMap::new();
        for turn in self.turns.values() {
            *turn_status_counts
                .entry(turn_status_name(turn.status).to_string())
                .or_insert(0) += 1;
        }
        RuntimeMetrics {
            thread_count: self.threads.len() as u64,
            turn_count: self.turns.len() as u64,
            event_count: self.next_event,
            pending_approvals: self
                .approvals
                .values()
                .filter(|approval| approval.outcome.is_none())
                .count() as u64,
            pending_questions: self
                .questions
                .values()
                .filter(|question| !question.resolved)
                .count() as u64,
            plugin_count: self.plugins.len() as u64,
            mcp_server_count: self.mcp_servers.len() as u64,
            model_count: self.models.len() as u64,
            agent_definition_count: self.agents.values().map(Vec::len).sum::<usize>() as u64,
            agent_run_count: self.agent_runs.len() as u64,
            turn_status_counts,
        }
    }

    pub fn debug_bundle(&self, mut diagnostics: Vec<Diagnostic>) -> DebugBundle {
        diagnostics.extend(self.recent_diagnostics(25));
        DebugBundle {
            schema_version: 1,
            redacted: true,
            metrics: self.metrics(),
            diagnostics,
        }
    }

    fn recent_diagnostics(&self, limit: usize) -> Vec<Diagnostic> {
        let mut diagnostics = self
            .event_store
            .threads()
            .into_iter()
            .flat_map(|thread_id| self.event_store.list_thread(&thread_id).unwrap_or_default())
            .filter_map(|event| match event.kind {
                EventKind::Diagnostic { diagnostic } => Some(diagnostic),
                _ => None,
            })
            .collect::<Vec<_>>();
        diagnostics.reverse();
        diagnostics.truncate(limit);
        diagnostics.reverse();
        diagnostics
    }

    fn turn_has_provider_replay_snapshot(&self, turn: &Turn) -> bool {
        matches!(
            turn.status,
            TurnStatus::WaitingForApproval | TurnStatus::WaitingForUser
        ) && self.direct_provider_turns.contains_key(&turn.id)
    }

    fn clear_pending_pauses_for_turn(&mut self, turn_id: &str) {
        for approval in self.approvals.values_mut() {
            if approval.turn_id.as_deref() == Some(turn_id) && approval.outcome.is_none() {
                approval.outcome = Some(ApprovalOutcome::Cancelled);
            }
        }
        for question in self.questions.values_mut() {
            if question.turn_id.as_deref() == Some(turn_id) {
                question.resolved = true;
            }
        }
    }

    pub fn recover_incomplete_turns(&mut self, reason: impl Into<String>) -> EventsListResult {
        let reason = reason.into();
        let recoverable: Vec<(ThreadId, TurnId)> = self
            .turns
            .values()
            .filter(|turn| !turn_is_terminal(turn))
            .filter(|turn| !self.turn_has_provider_replay_snapshot(turn))
            .map(|turn| (turn.thread_id.clone(), turn.id.clone()))
            .collect();
        let mut events = Vec::new();
        for (thread_id, turn_id) in recoverable {
            if let Some(turn) = self.turns.get_mut(&turn_id) {
                turn.status = TurnStatus::Aborted;
            }
            self.clear_pending_pauses_for_turn(&turn_id);
            events.push(self.event(
                &thread_id,
                Some(&turn_id),
                EventKind::TurnAborted {
                    reason: reason.clone(),
                },
            ));
        }
        EventsListResult { events }
    }

    pub fn replay_events(events: &[Event]) -> Result<Self, RuntimeError> {
        let mut runtime = Runtime::new();
        runtime.threads.clear();
        runtime.turns.clear();
        runtime.approvals.clear();
        runtime.questions.clear();
        runtime.plugins.clear();
        runtime.todos = TodoState::default();
        runtime.goals.clear();
        runtime.goal_active_started_at_ms.clear();
        runtime.goal_continuations.clear();
        runtime.mcp_servers.clear();
        runtime.models.clear();
        runtime.agent_runs.clear();
        runtime.direct_provider_turns.clear();
        runtime.selected_model = None;
        runtime.event_store = InMemoryEventStore::new();

        for event in events {
            runtime.next_event = runtime.next_event.max(event.id);
            runtime
                .event_store
                .append(event.clone())
                .map_err(event_store_error)?;
            runtime.apply_replayed_event(event)?;
        }
        Ok(runtime)
    }

    pub fn reserve_thread_counter(&mut self, counter: u64) {
        self.next_thread = self.next_thread.max(counter);
    }

    fn apply_replayed_event(&mut self, event: &Event) -> Result<(), RuntimeError> {
        match &event.kind {
            EventKind::ThreadStarted { thread }
            | EventKind::ThreadResumed { thread }
            | EventKind::ThreadForked { thread, .. }
            | EventKind::ThreadUpdated { thread } => {
                self.bump_thread_counter(&thread.id);
                self.threads.insert(thread.id.clone(), thread.clone());
            }
            EventKind::ThreadGoalUpdated { goal } => {
                self.goals.insert(goal.thread_id.clone(), goal.clone());
                if goal.status == ThreadGoalStatus::Active {
                    self.goal_active_started_at_ms
                        .insert(goal.thread_id.clone(), goal.updated_at_ms);
                } else {
                    self.goal_active_started_at_ms.remove(&goal.thread_id);
                }
            }
            EventKind::ThreadGoalCleared { thread_id } => {
                self.goals.remove(thread_id);
                self.goal_active_started_at_ms.remove(thread_id);
                self.goal_continuations.remove(thread_id);
            }
            EventKind::TurnStarted { turn } => {
                self.bump_turn_counter(&turn.id);
                self.turns.insert(turn.id.clone(), turn.clone());
            }
            EventKind::TurnPhaseChanged { phase } => {
                if let Some(turn_id) = event.turn_id.as_deref()
                    && let Some(turn) = self.turns.get_mut(turn_id)
                {
                    turn.phase = *phase;
                }
            }
            EventKind::TurnCompleted { turn_id } => {
                if let Some(turn) = self.turns.get_mut(turn_id) {
                    turn.status = TurnStatus::Completed;
                }
                self.clear_pending_pauses_for_turn(turn_id);
                self.direct_provider_turns.remove(turn_id);
            }
            EventKind::TurnInterrupted { .. } | EventKind::TurnAborted { .. } => {
                if let Some(turn_id) = event.turn_id.as_deref() {
                    if let Some(turn) = self.turns.get_mut(turn_id) {
                        turn.status = TurnStatus::Aborted;
                    }
                    self.clear_pending_pauses_for_turn(turn_id);
                    self.direct_provider_turns.remove(turn_id);
                }
            }
            EventKind::ApprovalRequested { request } => {
                self.bump_approval_counter(&request.id);
                if let Some(turn_id) = event.turn_id.as_deref()
                    && let Some(turn) = self.turns.get_mut(turn_id)
                {
                    turn.status = TurnStatus::WaitingForApproval;
                }
                self.approvals.insert(
                    request.id.clone(),
                    OwnedApprovalRequest {
                        thread_id: event.thread_id.clone(),
                        turn_id: event.turn_id.clone(),
                        tool_call_id: request.tool_call.as_ref().map(|call| call.id.clone()),
                        tool_call: request.tool_call.clone(),
                        outcome: None,
                    },
                );
            }
            EventKind::ApprovalResolved { decision } => {
                if let Some(owner) = self.approvals.get_mut(&decision.request_id) {
                    owner.outcome = Some(decision.decision);
                }
            }
            EventKind::QuestionRequested { request } => {
                self.bump_question_counter(&request.id);
                self.questions.insert(
                    request.id.clone(),
                    OwnedQuestionRequest {
                        thread_id: event.thread_id.clone(),
                        turn_id: None,
                        tool_call_id: None,
                        tool_call: None,
                        resolved: false,
                    },
                );
            }
            EventKind::QuestionResolved { response } => {
                if let Some(owner) = self.questions.get_mut(&response.request_id) {
                    owner.resolved = true;
                }
            }
            EventKind::AskUserRequested { request } => {
                self.bump_question_counter(&request.id);
                if let Some(turn_id) = event.turn_id.as_deref()
                    && let Some(turn) = self.turns.get_mut(turn_id)
                {
                    turn.status = TurnStatus::WaitingForUser;
                }
                let tool_call = event
                    .turn_id
                    .as_deref()
                    .and_then(|turn_id| self.direct_provider_turns.get(turn_id))
                    .and_then(|state| {
                        request
                            .tool_call_id
                            .as_deref()
                            .and_then(|call_id| tool_call_from_provider_snapshot(state, call_id))
                    });
                self.questions.insert(
                    request.id.clone(),
                    OwnedQuestionRequest {
                        thread_id: event.thread_id.clone(),
                        turn_id: event.turn_id.clone(),
                        tool_call_id: request.tool_call_id.clone(),
                        tool_call,
                        resolved: false,
                    },
                );
            }
            EventKind::AskUserResolved { response } => {
                if let Some(owner) = self.questions.get_mut(&response.request_id) {
                    owner.resolved = true;
                }
            }
            EventKind::ProviderTranscriptSnapshot { snapshot } => {
                self.direct_provider_turns.insert(
                    snapshot.turn_id.clone(),
                    DirectProviderResumeState::from_replay_snapshot(snapshot),
                );
            }
            EventKind::SuggestionOffered { suggestion } => {
                self.suggested_next = Some(suggestion.clone());
            }
            EventKind::PluginRegistered { plugin } => {
                self.plugins.insert(plugin.id.clone(), plugin.clone());
            }
            EventKind::MemoryStatusChanged { status } => {
                self.memory = status.clone();
            }
            EventKind::TodosUpdated { state } => {
                self.todos = state.clone();
            }
            EventKind::McpServerRegistered { server } => {
                self.mcp_servers.insert(server.id.clone(), server.clone());
            }
            EventKind::ModelRegistered { model } => {
                self.models.insert(model.id.clone(), model.clone());
            }
            EventKind::ModelSelected { model_id } => {
                self.selected_model = Some(model_id.clone());
            }
            EventKind::AgentStarted { run } => {
                self.bump_agent_run_counter(&run.id);
                self.agent_runs.insert(run.id.clone(), run.clone());
            }
            EventKind::AgentBlocked { run_id, .. } => {
                if let Some(run) = self.agent_runs.get_mut(run_id) {
                    run.status = AgentRunStatus::Blocked;
                }
            }
            EventKind::AgentCompleted { run_id, .. } => {
                if let Some(run) = self.agent_runs.get_mut(run_id) {
                    run.status = AgentRunStatus::Completed;
                }
            }
            EventKind::ItemStarted { item } | EventKind::ItemCompleted { item } => {
                self.bump_item_counter(&item.id);
            }
            EventKind::ItemDelta { .. }
            | EventKind::ToolCallStarted { .. }
            | EventKind::ToolCallCompleted { .. }
            | EventKind::ArtifactCreated { .. }
            | EventKind::ToolBatchStarted { .. }
            | EventKind::ToolBatchCompleted { .. }
            | EventKind::SandboxStatus { .. }
            | EventKind::HandoffCompacted { .. }
            | EventKind::PiAdapterEvent { .. }
            | EventKind::SideQuestionAnswered { .. }
            | EventKind::Diagnostic { .. } => {}
        }
        Ok(())
    }

    fn persist_provider_replay_snapshot(
        &mut self,
        turn: &Turn,
        provider: &mut dyn ModelProvider,
        emitted: &mut Vec<Event>,
    ) {
        if let Some(snapshot) = provider.snapshot() {
            emitted.push(self.event(
                &turn.thread_id,
                Some(&turn.id),
                EventKind::ProviderTranscriptSnapshot {
                    snapshot: snapshot.replay_snapshot(&turn.id),
                },
            ));
            self.direct_provider_turns.insert(turn.id.clone(), snapshot);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn drive_agentic_turn(
        &mut self,
        mut turn: Turn,
        mut state: AgenticLoopState,
        input: String,
        provider: &mut dyn ModelProvider,
        tool_registry: &ToolRegistry,
        approved_tool_call_ids: BTreeSet<ToolCallId>,
        cancellation: Option<AgenticCancellation>,
        max_continuations: u32,
        mut continuation: u32,
        pending_tool_calls: Vec<ToolCall>,
        mut emitted: Vec<Event>,
    ) -> Result<AgenticTurnResult, RuntimeError> {
        for call in pending_tool_calls {
            if let Some(reason) = self.take_requested_interrupt(&turn.id) {
                return Ok(self.interrupt_agentic_turn(turn, reason, emitted));
            }
            if let Some(reason) = cancellation_tool_reason(&cancellation, &call.id) {
                emitted.push(self.event(
                    &turn.thread_id,
                    Some(&turn.id),
                    EventKind::ToolCallStarted { call: call.clone() },
                ));
                emitted.push(self.event(
                    &turn.thread_id,
                    Some(&turn.id),
                    EventKind::ToolCallCompleted {
                        result: ToolResult {
                            call_id: call.id.clone(),
                            status: ToolResultStatus::Aborted,
                            output: None,
                            error: Some(reason.clone()),
                        },
                    },
                ));
                return Ok(self.abort_agentic_turn(turn, reason, emitted));
            }
            self.execute_and_observe_agentic_tool(
                &turn,
                tool_registry,
                provider,
                &call,
                None,
                &mut emitted,
            )?;
        }

        loop {
            if let Some(reason) = self.take_requested_interrupt(&turn.id) {
                return Ok(self.interrupt_agentic_turn(turn, reason, emitted));
            }

            if continuation > max_continuations {
                if state.current != TurnPhase::Loop {
                    self.transition_agentic_phase(
                        &mut state,
                        &mut turn,
                        TurnPhase::Loop,
                        &mut emitted,
                    )?;
                }
                turn.status = TurnStatus::Aborted;
                self.direct_provider_turns.remove(&turn.id);
                self.turns.insert(turn.id.clone(), turn.clone());
                emitted.push(self.event(
                    &turn.thread_id,
                    Some(&turn.id),
                    EventKind::TurnAborted {
                        reason: "continuation guard exceeded".to_string(),
                    },
                ));
                return Ok(AgenticTurnResult {
                    turn,
                    events: emitted,
                    awaiting_approval: None,
                    awaiting_question: None,
                });
            }

            if let Some(reason) = cancellation_before_model_reason(&cancellation, continuation) {
                return Ok(self.abort_agentic_turn(turn, reason, emitted));
            }

            self.transition_agentic_phase(&mut state, &mut turn, TurnPhase::Api, &mut emitted)?;
            let request = ModelRequest {
                thread_id: turn.thread_id.clone(),
                turn_id: turn.id.clone(),
                input: input.clone(),
                continuation,
                history: self.compact_provider_history(&turn.thread_id, &turn.id),
            };
            let provider_step = match provider.next_step(&request) {
                Ok(step) => step,
                Err(error) => {
                    let reason = error.message.clone();
                    let _ = self.abort_agentic_turn(turn, reason, emitted);
                    return Err(error);
                }
            };
            if let Some(reason) = self.take_requested_interrupt(&turn.id) {
                return Ok(self.interrupt_agentic_turn(turn, reason, emitted));
            }
            self.emit_provider_diagnostics(&turn, provider_step.diagnostics, &mut emitted);
            self.account_active_goal(
                &turn.thread_id,
                provider_step.known_token_delta,
                0,
                &mut emitted,
            );
            let step = provider_step.step;
            self.transition_agentic_phase(&mut state, &mut turn, TurnPhase::Tokens, &mut emitted)?;
            self.stream_assistant_deltas(&turn, &step.assistant_deltas, &mut emitted);
            if let Some(reason) = self.take_requested_interrupt(&turn.id) {
                return Ok(self.interrupt_agentic_turn(turn, reason, emitted));
            }
            self.transition_agentic_phase(&mut state, &mut turn, TurnPhase::Tools, &mut emitted)?;
            let mut provided_results = match tool_results_by_call_id(&step) {
                Ok(results) => results,
                Err(error) => {
                    let reason = error.message.clone();
                    let _ = self.abort_agentic_turn(turn, reason, emitted);
                    return Err(error);
                }
            };

            for call in step.tool_calls.clone() {
                if let Some(reason) = self.take_requested_interrupt(&turn.id) {
                    return Ok(self.interrupt_agentic_turn(turn, reason, emitted));
                }
                if let Some(reason) = cancellation_tool_reason(&cancellation, &call.id) {
                    emitted.push(self.event(
                        &turn.thread_id,
                        Some(&turn.id),
                        EventKind::ToolCallStarted { call: call.clone() },
                    ));
                    emitted.push(self.event(
                        &turn.thread_id,
                        Some(&turn.id),
                        EventKind::ToolCallCompleted {
                            result: ToolResult {
                                call_id: call.id.clone(),
                                status: ToolResultStatus::Aborted,
                                output: None,
                                error: Some(reason.clone()),
                            },
                        },
                    ));
                    return Ok(self.abort_agentic_turn(turn, reason, emitted));
                }
                let provided_result = provided_results.remove(&call.id);
                if provided_result.is_none()
                    && lookup_tool(tool_registry, &call)
                        .is_some_and(|definition| is_ask_user_tool(definition, &call))
                {
                    self.next_question += 1;
                    let request_id = format!("question-{}", self.next_question);
                    match normalize_ask_user_request(request_id.clone(), &call) {
                        Ok(request) => {
                            emitted.push(self.event(
                                &turn.thread_id,
                                Some(&turn.id),
                                EventKind::ToolCallStarted { call: call.clone() },
                            ));
                            self.questions.insert(
                                request.id.clone(),
                                OwnedQuestionRequest {
                                    thread_id: turn.thread_id.clone(),
                                    turn_id: Some(turn.id.clone()),
                                    tool_call_id: Some(call.id.clone()),
                                    tool_call: Some(call.clone()),
                                    resolved: false,
                                },
                            );
                            self.persist_provider_replay_snapshot(&turn, provider, &mut emitted);
                            self.finish_goal_turn_accounting(&turn.thread_id, false, &mut emitted);
                            turn.status = TurnStatus::WaitingForUser;
                            self.turns.insert(turn.id.clone(), turn.clone());
                            self.emit_hook_diagnostic(
                                &turn,
                                "user-input",
                                format!("tool {} paused for user input", call.name),
                                &mut emitted,
                            );
                            emitted.push(self.event(
                                &turn.thread_id,
                                Some(&turn.id),
                                EventKind::AskUserRequested {
                                    request: request.clone(),
                                },
                            ));
                            return Ok(AgenticTurnResult {
                                turn,
                                events: emitted,
                                awaiting_approval: None,
                                awaiting_question: Some(request),
                            });
                        }
                        Err((status, error)) => {
                            emitted.push(self.event(
                                &turn.thread_id,
                                Some(&turn.id),
                                EventKind::ToolCallStarted { call: call.clone() },
                            ));
                            let result = ToolResult {
                                call_id: call.id.clone(),
                                status,
                                output: None,
                                error: Some(error),
                            };
                            emitted.push(self.event(
                                &turn.thread_id,
                                Some(&turn.id),
                                EventKind::ToolCallCompleted {
                                    result: result.clone(),
                                },
                            ));
                            provider.observe_tool_result(&call, &result)?;
                            continue;
                        }
                    }
                }
                let requires_approval = self.agentic_tool_requires_approval(tool_registry, &call);
                if provided_result.is_none()
                    && requires_approval
                    && !approved_tool_call_ids.contains(&call.id)
                    && !self.has_approved_tool_call(&turn.thread_id, &turn.id, &call.id)
                {
                    if let Some(review) =
                        self.guardian_auto_review_tool_call(&turn, tool_registry, &call)
                    {
                        self.emit_guardian_review_diagnostic(&turn, &review, &mut emitted);
                        if review.decision == GuardianReviewDecision::Deny {
                            emitted.push(self.event(
                                &turn.thread_id,
                                Some(&turn.id),
                                EventKind::ToolCallStarted { call: call.clone() },
                            ));
                            let result = ToolResult {
                                call_id: call.id.clone(),
                                status: ToolResultStatus::Denied,
                                output: Some(review.strict_json.clone()),
                                error: Some(review.reason.clone()),
                            };
                            emitted.push(self.event(
                                &turn.thread_id,
                                Some(&turn.id),
                                EventKind::ToolCallCompleted {
                                    result: result.clone(),
                                },
                            ));
                            provider.observe_tool_result(&call, &result)?;
                            continue;
                        }
                    }
                    self.next_approval += 1;
                    let request = ApprovalRequest {
                        id: format!("approval-{}", self.next_approval),
                        reason: format!("tool {} requires approval", call.name),
                        risk: RiskLevel::Medium,
                        tool_call: Some(call.clone()),
                    };
                    self.approvals.insert(
                        request.id.clone(),
                        OwnedApprovalRequest {
                            thread_id: turn.thread_id.clone(),
                            turn_id: Some(turn.id.clone()),
                            tool_call_id: Some(call.id.clone()),
                            tool_call: Some(call.clone()),
                            outcome: None,
                        },
                    );
                    self.persist_provider_replay_snapshot(&turn, provider, &mut emitted);
                    self.finish_goal_turn_accounting(&turn.thread_id, false, &mut emitted);
                    turn.status = TurnStatus::WaitingForApproval;
                    self.turns.insert(turn.id.clone(), turn.clone());
                    self.emit_hook_diagnostic(
                        &turn,
                        "policy",
                        format!("tool {} paused for approval", call.name),
                        &mut emitted,
                    );
                    emitted.push(self.event(
                        &turn.thread_id,
                        Some(&turn.id),
                        EventKind::ApprovalRequested {
                            request: request.clone(),
                        },
                    ));
                    return Ok(AgenticTurnResult {
                        turn,
                        events: emitted,
                        awaiting_approval: Some(request),
                        awaiting_question: None,
                    });
                }
                if let Err(error) = self.execute_and_observe_agentic_tool(
                    &turn,
                    tool_registry,
                    provider,
                    &call,
                    provided_result,
                    &mut emitted,
                ) {
                    let reason = error.message.clone();
                    let _ = self.abort_agentic_turn(turn, reason, emitted);
                    return Err(error);
                }
                if let Some(reason) = self.take_requested_interrupt(&turn.id) {
                    return Ok(self.interrupt_agentic_turn(turn, reason, emitted));
                }
            }

            if let Some(reason) = self.take_requested_interrupt(&turn.id) {
                return Ok(self.interrupt_agentic_turn(turn, reason, emitted));
            }

            if !provided_results.is_empty() {
                let error = RuntimeError::new(
                    "tool_result_without_call",
                    RuntimeErrorCategory::ToolPairing,
                    format!(
                        "model step provided tool results without matching calls: {}",
                        provided_results
                            .keys()
                            .cloned()
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                );
                let reason = error.message.clone();
                let _ = self.abort_agentic_turn(turn, reason, emitted);
                return Err(error);
            }

            self.transition_agentic_phase(&mut state, &mut turn, TurnPhase::Loop, &mut emitted)?;
            if let Some(reason) = self.take_requested_interrupt(&turn.id) {
                return Ok(self.interrupt_agentic_turn(turn, reason, emitted));
            }
            if step.final_response || step.tool_calls.is_empty() {
                self.transition_agentic_phase(
                    &mut state,
                    &mut turn,
                    TurnPhase::Render,
                    &mut emitted,
                )?;
                self.transition_agentic_phase(
                    &mut state,
                    &mut turn,
                    TurnPhase::Hooks,
                    &mut emitted,
                )?;
                self.emit_hook_diagnostic(
                    &turn,
                    "stop",
                    "agentic turn reached final response".to_string(),
                    &mut emitted,
                );
                self.transition_agentic_phase(
                    &mut state,
                    &mut turn,
                    TurnPhase::Await,
                    &mut emitted,
                )?;
                turn.status = TurnStatus::Completed;
                self.finish_goal_turn_accounting(&turn.thread_id, false, &mut emitted);
                self.direct_provider_turns.remove(&turn.id);
                self.turns.insert(turn.id.clone(), turn.clone());
                emitted.push(self.event(
                    &turn.thread_id,
                    Some(&turn.id),
                    EventKind::TurnCompleted {
                        turn_id: turn.id.clone(),
                    },
                ));
                return Ok(AgenticTurnResult {
                    turn,
                    events: emitted,
                    awaiting_approval: None,
                    awaiting_question: None,
                });
            }
            continuation += 1;
        }
    }

    fn abort_agentic_turn(
        &mut self,
        mut turn: Turn,
        reason: String,
        mut emitted: Vec<Event>,
    ) -> AgenticTurnResult {
        turn.status = TurnStatus::Aborted;
        self.direct_provider_turns.remove(&turn.id);
        self.turns.insert(turn.id.clone(), turn.clone());
        self.finish_goal_turn_accounting(&turn.thread_id, true, &mut emitted);
        emitted.push(self.event(
            &turn.thread_id,
            Some(&turn.id),
            EventKind::TurnAborted { reason },
        ));
        AgenticTurnResult {
            turn,
            events: emitted,
            awaiting_approval: None,
            awaiting_question: None,
        }
    }

    fn take_requested_interrupt(&self, turn_id: &str) -> Option<String> {
        self.turn_interrupts.take(turn_id)
    }

    fn interrupt_agentic_turn(
        &mut self,
        mut turn: Turn,
        reason: String,
        mut emitted: Vec<Event>,
    ) -> AgenticTurnResult {
        turn.status = TurnStatus::Aborted;
        self.direct_provider_turns.remove(&turn.id);
        self.turns.insert(turn.id.clone(), turn.clone());
        self.finish_goal_turn_accounting(&turn.thread_id, true, &mut emitted);
        emitted.push(self.event(
            &turn.thread_id,
            Some(&turn.id),
            EventKind::TurnInterrupted { reason },
        ));
        AgenticTurnResult {
            turn,
            events: emitted,
            awaiting_approval: None,
            awaiting_question: None,
        }
    }

    fn turn_tool_registry(&self, definitions: Vec<ToolDefinition>) -> ToolRegistry {
        let mut registry = self.tool_registry.clone();
        for definition in definitions {
            registry.register(definition);
        }
        registry
    }

    fn discover_skill_candidates(&self, cwd: Option<&str>) -> Vec<SkillCandidate> {
        let mut candidates = builtin_skill_candidates();
        candidates.extend(file_skill_candidates(user_skill_roots()));
        if let Some(cwd) = cwd {
            candidates.extend(file_skill_candidates(project_skill_roots(cwd)));
        }
        candidates
    }

    fn selected_skill_candidates(
        &self,
        thread_id: &str,
        input: &str,
        explicit_names: &[String],
    ) -> Result<Vec<SkillCandidate>, RuntimeError> {
        let cwd = self.thread(thread_id)?.project.cwd.clone();
        Ok(
            resolve_skill_candidates(self.discover_skill_candidates(Some(&cwd)))
                .into_iter()
                .map(|(active, _)| active)
                .filter(|skill| skill_relevant(&skill.reference, input, explicit_names))
                .collect(),
        )
    }

    fn skill_injection_prompt(
        &self,
        thread_id: &str,
        input: &str,
        explicit_names: &[String],
    ) -> Result<Option<(String, Vec<SkillRef>)>, RuntimeError> {
        let selected = self.selected_skill_candidates(thread_id, input, explicit_names)?;
        if selected.is_empty() {
            return Ok(None);
        }
        let mut prompt = String::from(
            "The following skill instructions were selected deterministically by OPPi. Use them only when they are relevant to the current task.\n",
        );
        let mut refs = Vec::new();
        for skill in selected {
            if prompt.chars().count() >= MAX_SKILL_INJECTION_CHARS {
                break;
            }
            prompt.push_str("\n---\n");
            prompt.push_str(&skill_injection_section(&skill));
            refs.push(skill.reference);
        }
        Ok(Some((
            truncate_chars(&prompt, MAX_SKILL_INJECTION_CHARS),
            refs,
        )))
    }

    fn pending_approved_tool_calls(
        &self,
        thread_id: &str,
        turn_id: &str,
        approved: &BTreeSet<ToolCallId>,
    ) -> Vec<ToolCall> {
        self.approvals
            .values()
            .filter(|approval| {
                approval.thread_id == thread_id
                    && approval.turn_id.as_deref() == Some(turn_id)
                    && approval.outcome == Some(ApprovalOutcome::Approved)
                    && approval
                        .tool_call_id
                        .as_ref()
                        .is_some_and(|call_id| approved.contains(call_id))
            })
            .filter_map(|approval| approval.tool_call.clone())
            .collect()
    }

    fn compact_provider_history(
        &self,
        thread_id: &str,
        current_turn_id: &str,
    ) -> Vec<ProviderHistoryMessage> {
        let mut messages = Vec::new();
        let mut summaries = Vec::new();
        for event in self.event_store.list_thread(thread_id).unwrap_or_default() {
            match event.kind {
                EventKind::HandoffCompacted { summary, .. } if !summary.trim().is_empty() => {
                    summaries.push(summary);
                }
                EventKind::ItemCompleted { item } => match item.kind {
                    ItemKind::UserMessage { text } if !text.trim().is_empty() => {
                        messages.push(ProviderHistoryMessage {
                            role: ProviderMessageRole::User,
                            content: compact_history_text(&text),
                            turn_id: Some(item.turn_id),
                        });
                    }
                    ItemKind::AssistantMessage { text } if !text.trim().is_empty() => {
                        messages.push(ProviderHistoryMessage {
                            role: ProviderMessageRole::Assistant,
                            content: compact_history_text(&text),
                            turn_id: Some(item.turn_id),
                        });
                    }
                    _ => {}
                },
                _ => {}
            }
        }

        let mut compact = Vec::new();
        if let Some(summary) = summaries.last() {
            compact.push(ProviderHistoryMessage {
                role: ProviderMessageRole::System,
                content: format!(
                    "Prior compacted conversation summary:\n{}",
                    compact_history_text(summary)
                ),
                turn_id: None,
            });
        }

        let mut total_chars = compact
            .iter()
            .map(|message| message.content.len())
            .sum::<usize>();
        let mut tail = Vec::new();
        for message in messages.into_iter().rev() {
            let must_keep_current_user = message.role == ProviderMessageRole::User
                && message.turn_id.as_deref() == Some(current_turn_id);
            if !must_keep_current_user
                && (tail.len() >= PROVIDER_HISTORY_MAX_MESSAGES
                    || total_chars + message.content.len() > PROVIDER_HISTORY_MAX_CHARS)
            {
                continue;
            }
            total_chars += message.content.len();
            tail.push(message);
        }
        tail.reverse();
        compact.extend(tail);
        compact
    }

    fn emit_provider_diagnostics(
        &mut self,
        turn: &Turn,
        diagnostics: Vec<Diagnostic>,
        emitted: &mut Vec<Event>,
    ) {
        for diagnostic in diagnostics {
            emitted.push(self.event(
                &turn.thread_id,
                Some(&turn.id),
                EventKind::Diagnostic { diagnostic },
            ));
        }
    }

    fn execute_and_observe_agentic_tool(
        &mut self,
        turn: &Turn,
        tool_registry: &ToolRegistry,
        provider: &mut dyn ModelProvider,
        call: &ToolCall,
        provided_result: Option<ToolResult>,
        emitted: &mut Vec<Event>,
    ) -> Result<(), RuntimeError> {
        emitted.push(self.event(
            &turn.thread_id,
            Some(&turn.id),
            EventKind::ToolCallStarted { call: call.clone() },
        ));
        let parent_provider_config = provider.direct_config();
        let outcome = self.execute_agentic_tool(
            turn,
            tool_registry,
            call,
            provided_result,
            parent_provider_config,
        );
        emitted.extend(outcome.side_events);
        let result = outcome.result;
        if result.status != ToolResultStatus::Ok {
            self.emit_hook_diagnostic(
                turn,
                "error-recovery",
                format!("tool {} returned {:?}", call.name, result.status),
                emitted,
            );
        }
        emitted.push(self.event(
            &turn.thread_id,
            Some(&turn.id),
            EventKind::ToolCallCompleted {
                result: result.clone(),
            },
        ));
        if let Some(artifact) = artifact_metadata_from_tool_result(call, &result) {
            emitted.push(self.event(
                &turn.thread_id,
                Some(&turn.id),
                EventKind::ArtifactCreated { artifact },
            ));
        }
        if result.status == ToolResultStatus::Ok
            && lookup_tool(tool_registry, call)
                .is_some_and(|definition| is_suggest_next_tool(definition, call))
            && let Some(suggestion) = self.suggested_next.clone()
        {
            emitted.push(self.event(
                &turn.thread_id,
                Some(&turn.id),
                EventKind::SuggestionOffered { suggestion },
            ));
        }
        if result.status == ToolResultStatus::Ok
            && lookup_tool(tool_registry, call)
                .is_some_and(|definition| is_todo_tool(definition, call))
        {
            emitted.push(self.event(
                &turn.thread_id,
                Some(&turn.id),
                EventKind::TodosUpdated {
                    state: self.todos.clone(),
                },
            ));
        }
        provider.observe_tool_result(call, &result)
    }

    fn transition_agentic_phase(
        &mut self,
        state: &mut AgenticLoopState,
        turn: &mut Turn,
        phase: TurnPhase,
        emitted: &mut Vec<Event>,
    ) -> Result<(), RuntimeError> {
        state.transition(phase)?;
        self.set_phase(turn, phase, emitted);
        Ok(())
    }

    fn stream_assistant_deltas(&mut self, turn: &Turn, deltas: &[String], events: &mut Vec<Event>) {
        if deltas.is_empty() {
            return;
        }
        let item = self.item(
            &turn.thread_id,
            &turn.id,
            ItemKind::AssistantMessage {
                text: deltas.concat(),
            },
        );
        events.push(self.event(
            &turn.thread_id,
            Some(&turn.id),
            EventKind::ItemStarted { item: item.clone() },
        ));
        for delta in deltas {
            events.push(self.event(
                &turn.thread_id,
                Some(&turn.id),
                EventKind::ItemDelta {
                    item_id: item.id.clone(),
                    delta: delta.clone(),
                },
            ));
        }
        events.push(self.event(
            &turn.thread_id,
            Some(&turn.id),
            EventKind::ItemCompleted { item },
        ));
    }

    fn agentic_tool_requires_approval(&self, registry: &ToolRegistry, call: &ToolCall) -> bool {
        call.arguments
            .get("requireApproval")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
            || lookup_tool(registry, call).is_some_and(|definition| definition.requires_approval)
    }

    fn execute_agentic_tool(
        &mut self,
        turn: &Turn,
        registry: &ToolRegistry,
        call: &ToolCall,
        provided_result: Option<ToolResult>,
        parent_provider_config: Option<DirectModelProviderConfig>,
    ) -> AgenticToolExecution {
        let Some(definition) = lookup_tool(registry, call) else {
            return AgenticToolExecution::result(ToolResult {
                call_id: call.id.clone(),
                status: ToolResultStatus::Error,
                output: None,
                error: Some(format!("tool not registered: {}", call.name)),
            });
        };
        if let Some(result) = provided_result {
            return AgenticToolExecution::result(result);
        }
        if is_agent_tool(definition, call) {
            return self.execute_agent_tool(turn, registry, call, parent_provider_config);
        }
        if is_todo_tool(definition, call) {
            return AgenticToolExecution::result(self.execute_todo_write_tool(call));
        }
        if is_suggest_next_tool(definition, call) {
            return AgenticToolExecution::result(self.execute_suggest_next_tool(call));
        }
        if is_goal_tool(definition, call) {
            return AgenticToolExecution::result(self.execute_goal_tool(&turn.thread_id, call));
        }
        if is_feedback_tool(definition, call) {
            return AgenticToolExecution::result(self.execute_feedback_tool(turn, call));
        }
        if is_render_mermaid_tool(definition, call) {
            return AgenticToolExecution::result(self.execute_render_mermaid_tool(turn, call));
        }
        if is_shell_task_tool(definition, call) {
            return AgenticToolExecution::result(self.execute_shell_task_tool(call));
        }
        if is_shell_tool(definition, call) {
            return AgenticToolExecution::result(self.execute_shell_tool(turn, call));
        }
        if let Some(kind) = file_tool_kind(definition, call) {
            return AgenticToolExecution::result(self.execute_file_tool(turn, call, kind));
        }
        if is_image_gen_tool(definition, call) {
            return AgenticToolExecution::result(self.execute_image_gen_tool(turn, call));
        }
        if let Some(delay_ms) = tool_delay_ms(call) {
            std::thread::sleep(Duration::from_millis(delay_ms));
        }
        let output = call
            .arguments
            .get("output")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
            .or_else(|| Some(call.arguments.to_string()));
        AgenticToolExecution::result(ToolResult {
            call_id: call.id.clone(),
            status: ToolResultStatus::Ok,
            output,
            error: None,
        })
    }

    fn execute_agent_tool(
        &mut self,
        parent_turn: &Turn,
        parent_registry: &ToolRegistry,
        call: &ToolCall,
        parent_provider_config: Option<DirectModelProviderConfig>,
    ) -> AgenticToolExecution {
        let mut side_events = Vec::new();
        let result = match self.execute_agent_tool_inner(
            parent_turn,
            parent_registry,
            call,
            parent_provider_config,
            &mut side_events,
        ) {
            Ok(result) => result,
            Err((status, error)) => ToolResult {
                call_id: call.id.clone(),
                status,
                output: None,
                error: Some(error),
            },
        };
        AgenticToolExecution {
            result,
            side_events,
        }
    }

    fn execute_agent_tool_inner(
        &mut self,
        parent_turn: &Turn,
        parent_registry: &ToolRegistry,
        call: &ToolCall,
        parent_provider_config: Option<DirectModelProviderConfig>,
        side_events: &mut Vec<Event>,
    ) -> Result<ToolResult, (ToolResultStatus, String)> {
        let agent_name = agent_tool_agent_name(call);
        let task = agent_tool_task(call)?;
        let agent = self
            .active_agent(&agent_name)
            .map_err(runtime_error_as_tool)?;
        let mut policy = resolve_agent_tool_policy(&agent, call)?;
        let mut model_provider = agent_tool_model_provider(call)?.or(parent_provider_config);
        resolve_subagent_model_policy(&mut policy, model_provider.as_ref(), &task);
        let thread = self
            .thread(&parent_turn.thread_id)
            .map_err(runtime_error_as_tool)?
            .clone();
        let dispatch = self
            .dispatch_agent(AgentDispatchParams {
                thread_id: parent_turn.thread_id.clone(),
                agent_name: agent.name.clone(),
                task: task.clone(),
                worktree_root: first_string_arg(
                    &call.arguments,
                    &["worktreeRoot", "worktree_root"],
                )
                .or_else(|| agent.worktree_root.clone()),
                background: policy.background,
                role: policy.role.clone(),
                model: policy.model.clone(),
                effort: policy.effort.clone(),
                permission_mode: policy.permission_mode,
                memory_mode: policy.memory_mode.clone(),
                tool_allowlist: policy.tool_allowlist.clone(),
                tool_denylist: policy.tool_denylist.clone(),
                isolation: policy.isolation.clone(),
                color: policy.color.clone(),
                skills: policy.skills.clone(),
                max_turns: policy.max_turns,
            })
            .map_err(runtime_error_as_tool)?;
        let run = dispatch.run.clone();
        side_events.extend(dispatch.events);

        let parent_policy = self.sandbox_policy_for_turn(
            parent_turn,
            &thread.project.cwd,
            default_policy(PermissionProfile {
                mode: PermissionMode::Default,
                readable_roots: vec![thread.project.cwd.clone()],
                writable_roots: vec![thread.project.cwd.clone()],
                filesystem_rules: Vec::new(),
                protected_patterns: Vec::new(),
            }),
        );
        let child_policy =
            self.subagent_sandbox_policy(&parent_policy, &policy, &thread.project.cwd);
        let child_tools = self.subagent_tool_definitions(parent_registry, &policy);
        let model_steps = agent_tool_model_steps(call)?;
        let subagent_skill_injection = self
            .skill_injection_prompt(&parent_turn.thread_id, &task, &policy.skills)
            .map_err(runtime_error_as_tool)?;
        if let Some(config) = model_provider.as_mut() {
            if let Some(model) = policy
                .model
                .as_deref()
                .filter(|value| !value.trim().is_empty())
            {
                config.model = model.to_string();
            }
            if let Some(effort) = policy
                .effort
                .as_deref()
                .filter(|value| !value.trim().is_empty())
            {
                config.reasoning_effort = Some(effort.to_string());
            }
            config.system_prompt = Some(subagent_system_prompt(
                &agent,
                &policy,
                config.system_prompt.as_deref(),
                &self.memory,
                subagent_skill_injection
                    .as_ref()
                    .map(|(prompt, _)| prompt.as_str()),
            ));
        }
        let model_steps = if model_steps.is_empty() && model_provider.is_none() {
            vec![ScriptedModelStep {
                assistant_deltas: vec![format!(
                    "Subagent {} completed delegated task in native scripted mode: {}",
                    agent.name, task
                )],
                tool_calls: Vec::new(),
                tool_results: Vec::new(),
                final_response: true,
            }]
        } else {
            model_steps
        };

        side_events.push(
            self.event(
                &parent_turn.thread_id,
                Some(&parent_turn.id),
                EventKind::Diagnostic {
                    diagnostic: Diagnostic {
                        level: DiagnosticLevel::Info,
                        message: "native subagent execution policy applied".to_string(),
                        metadata: BTreeMap::from([
                            ("agent".to_string(), agent.name.clone()),
                            ("runId".to_string(), run.id.clone()),
                            (
                                "permissionMode".to_string(),
                                child_policy.permission_profile.mode.as_str().to_string(),
                            ),
                            ("background".to_string(), policy.background.to_string()),
                            ("toolCount".to_string(), child_tools.len().to_string()),
                            (
                                "memoryMode".to_string(),
                                policy
                                    .memory_mode
                                    .clone()
                                    .unwrap_or_else(|| "inherit".to_string()),
                            ),
                            (
                                "maxTurns".to_string(),
                                policy.max_turns.unwrap_or(3).to_string(),
                            ),
                        ]),
                    },
                },
            ),
        );
        if let Some((_, skills)) = &subagent_skill_injection {
            side_events.push(
                self.event(
                    &parent_turn.thread_id,
                    Some(&parent_turn.id),
                    EventKind::Diagnostic {
                        diagnostic: Diagnostic {
                            level: DiagnosticLevel::Info,
                            message: "subagent skill instructions injected".to_string(),
                            metadata: BTreeMap::from([(
                                "skills".to_string(),
                                skills
                                    .iter()
                                    .map(|skill| skill.name.clone())
                                    .collect::<Vec<_>>()
                                    .join(","),
                            )]),
                        },
                    },
                ),
            );
        }
        if policy.background {
            side_events.push(self.event(
                &parent_turn.thread_id,
                Some(&parent_turn.id),
                EventKind::Diagnostic {
                    diagnostic: Diagnostic {
                        level: DiagnosticLevel::Info,
                        message: "background subagent flag recorded; core nested execution runs in-process so parent tool completion remains blocking".to_string(),
                        metadata: BTreeMap::from([("runId".to_string(), run.id.clone())]),
                    },
                },
            ));
        }

        let child = self
            .run_agentic_turn_with_parent(
                AgenticTurnParams {
                    thread_id: parent_turn.thread_id.clone(),
                    input: format!(
                        "Subagent {} delegated task from {} via {}:\n{}",
                        agent.name, parent_turn.id, call.id, task
                    ),
                    execution_mode: if policy.background {
                        ExecutionMode::Background
                    } else {
                        ExecutionMode::Blocking
                    },
                    follow_up: None,
                    sandbox_policy: Some(child_policy.clone()),
                    model_steps,
                    model_provider,
                    tool_definitions: Vec::new(),
                    approved_tool_call_ids: Vec::new(),
                    cancellation: None,
                    max_continuations: policy.max_turns,
                },
                Some(parent_turn.id.clone()),
                Some(tool_registry_from_definitions(child_tools.clone())),
            )
            .map_err(runtime_error_as_tool)?;
        side_events.extend(child.events.clone());

        if let Some(request) = child.awaiting_approval {
            let reason = format!(
                "subagent {} blocked waiting for approval {}",
                agent.name, request.id
            );
            let blocked = self
                .block_agent(AgentBlockParams {
                    thread_id: parent_turn.thread_id.clone(),
                    run_id: run.id.clone(),
                    reason: reason.clone(),
                })
                .map_err(runtime_error_as_tool)?;
            side_events.extend(blocked.events);
            return Ok(ToolResult {
                call_id: call.id.clone(),
                status: ToolResultStatus::Error,
                output: Some(
                    json!({
                        "status": "blocked",
                        "runId": run.id,
                        "agentName": agent.name,
                        "reason": reason,
                        "approvalId": request.id,
                    })
                    .to_string(),
                ),
                error: Some(reason),
            });
        }
        if let Some(request) = child.awaiting_question {
            let reason = format!(
                "subagent {} blocked waiting for user input {}",
                agent.name, request.id
            );
            let blocked = self
                .block_agent(AgentBlockParams {
                    thread_id: parent_turn.thread_id.clone(),
                    run_id: run.id.clone(),
                    reason: reason.clone(),
                })
                .map_err(runtime_error_as_tool)?;
            side_events.extend(blocked.events);
            return Ok(ToolResult {
                call_id: call.id.clone(),
                status: ToolResultStatus::Error,
                output: Some(
                    json!({
                        "status": "blocked",
                        "runId": run.id,
                        "agentName": agent.name,
                        "reason": reason,
                        "questionId": request.id,
                    })
                    .to_string(),
                ),
                error: Some(reason),
            });
        }
        if child.turn.status != TurnStatus::Completed {
            let reason = format!(
                "subagent {} ended with status {:?}",
                agent.name, child.turn.status
            );
            let blocked = self
                .block_agent(AgentBlockParams {
                    thread_id: parent_turn.thread_id.clone(),
                    run_id: run.id.clone(),
                    reason: reason.clone(),
                })
                .map_err(runtime_error_as_tool)?;
            side_events.extend(blocked.events);
            return Ok(ToolResult {
                call_id: call.id.clone(),
                status: ToolResultStatus::Error,
                output: None,
                error: Some(reason),
            });
        }

        let output = subagent_output_from_events(&child.events).unwrap_or_else(|| {
            format!(
                "Subagent {} completed {} without assistant text",
                agent.name, child.turn.id
            )
        });
        let completed = self
            .complete_agent(AgentCompleteParams {
                thread_id: parent_turn.thread_id.clone(),
                run_id: run.id.clone(),
                output: output.clone(),
            })
            .map_err(runtime_error_as_tool)?;
        side_events.extend(completed.events);
        Ok(ToolResult {
            call_id: call.id.clone(),
            status: ToolResultStatus::Ok,
            output: Some(
                json!({
                    "status": "completed",
                    "runId": run.id,
                    "agentName": agent.name,
                    "turnId": child.turn.id,
                    "role": policy.role,
                    "model": policy.model,
                    "effort": policy.effort,
                    "permissionMode": child_policy.permission_profile.mode.as_str(),
                    "memoryMode": policy.memory_mode.unwrap_or_else(|| "inherit".to_string()),
                    "background": policy.background,
                    "isolation": policy.isolation,
                    "color": policy.color,
                    "skills": policy.skills,
                    "maxTurns": policy.max_turns,
                    "output": output,
                })
                .to_string(),
            ),
            error: None,
        })
    }

    fn subagent_sandbox_policy(
        &self,
        parent_policy: &SandboxPolicy,
        policy: &ResolvedAgentToolPolicy,
        cwd: &str,
    ) -> SandboxPolicy {
        let requested_mode = policy
            .permission_mode
            .unwrap_or(parent_policy.permission_profile.mode);
        let mode = min_permission_mode(parent_policy.permission_profile.mode, requested_mode);
        let requested_network = policy.network_policy.unwrap_or(parent_policy.network);
        let mut child = parent_policy.clone();
        child.permission_profile.mode = mode;
        child.filesystem = filesystem_for_permission(parent_policy.filesystem, mode);
        child.network = min_network_policy(parent_policy.network, requested_network);
        child.permission_profile.readable_roots =
            if child.permission_profile.readable_roots.is_empty() {
                vec![cwd.to_string()]
            } else {
                child.permission_profile.readable_roots.clone()
            };
        if child.filesystem == FilesystemPolicy::ReadOnly {
            child.permission_profile.writable_roots.clear();
        } else if child.permission_profile.writable_roots.is_empty() {
            child
                .permission_profile
                .writable_roots
                .push(cwd.to_string());
        }
        child
    }

    fn subagent_tool_definitions(
        &self,
        parent_registry: &ToolRegistry,
        policy: &ResolvedAgentToolPolicy,
    ) -> Vec<ToolDefinition> {
        parent_registry
            .list()
            .into_iter()
            .filter(|definition| {
                agent_tool_definition_allowed(
                    definition,
                    &policy.tool_allowlist,
                    &policy.tool_denylist,
                )
            })
            .collect()
    }

    fn execute_suggest_next_tool(&mut self, call: &ToolCall) -> ToolResult {
        match normalize_suggest_next(call) {
            Ok(Some(suggestion)) => {
                let message = suggestion.message.clone();
                self.suggested_next = Some(suggestion);
                ToolResult {
                    call_id: call.id.clone(),
                    status: ToolResultStatus::Ok,
                    output: Some(format!("Suggestion queued for host UI: {}", message)),
                    error: None,
                }
            }
            Ok(None) => {
                self.suggested_next = None;
                ToolResult {
                    call_id: call.id.clone(),
                    status: ToolResultStatus::Ok,
                    output: Some(
                        "Suggestion not shown: confidence is below 0.70 or message is empty"
                            .to_string(),
                    ),
                    error: None,
                }
            }
            Err((status, error)) => ToolResult {
                call_id: call.id.clone(),
                status,
                output: None,
                error: Some(error),
            },
        }
    }

    fn execute_goal_tool(&mut self, thread_id: &str, call: &ToolCall) -> ToolResult {
        let result = match call.name.as_str() {
            "get_goal" => self
                .get_thread_goal(ThreadGoalGetParams {
                    thread_id: thread_id.to_string(),
                })
                .map(|result| goal_tool_output(result.goal.as_ref(), None))
                .map_err(|error| error.message),
            "create_goal" => self.create_goal_from_tool(thread_id, call),
            "update_goal" => self.update_goal_from_tool(thread_id, call),
            other => Err(format!("unsupported goal tool: {other}")),
        };
        match result {
            Ok(output) => ToolResult {
                call_id: call.id.clone(),
                status: ToolResultStatus::Ok,
                output: Some(output),
                error: None,
            },
            Err(error) => ToolResult {
                call_id: call.id.clone(),
                status: ToolResultStatus::Error,
                output: None,
                error: Some(error),
            },
        }
    }

    fn create_goal_from_tool(
        &mut self,
        thread_id: &str,
        call: &ToolCall,
    ) -> Result<String, String> {
        if let Some(existing) = self.goals.get(thread_id)
            && existing.status != ThreadGoalStatus::Complete
        {
            return Err(
                "create_goal can only start a goal when no non-complete goal exists".to_string(),
            );
        }
        let objective = first_string_arg(&call.arguments, &["objective"])
            .ok_or_else(|| "create_goal requires an objective string".to_string())?;
        let token_budget = goal_tool_budget_arg(&call.arguments)?;
        let result = self
            .set_thread_goal(ThreadGoalSetParams {
                thread_id: thread_id.to_string(),
                objective: Some(objective),
                status: Some(ThreadGoalStatus::Active),
                token_budget: Some(token_budget),
            })
            .map_err(|error| error.message)?;
        Ok(goal_tool_output(Some(&result.goal), None))
    }

    fn update_goal_from_tool(
        &mut self,
        thread_id: &str,
        call: &ToolCall,
    ) -> Result<String, String> {
        let status = first_string_arg(&call.arguments, &["status"])
            .ok_or_else(|| "update_goal requires status".to_string())?
            .to_ascii_lowercase();
        if status != "complete" {
            return Err(
                "update_goal can only mark the existing goal complete; pause, resume, clear, and budget changes are user/runtime controls."
                    .to_string(),
            );
        }
        let result = self
            .set_thread_goal(ThreadGoalSetParams {
                thread_id: thread_id.to_string(),
                objective: None,
                status: Some(ThreadGoalStatus::Complete),
                token_budget: None,
            })
            .map_err(|error| error.message)?;
        let report = completion_budget_report(&result.goal);
        Ok(goal_tool_output(Some(&result.goal), report))
    }

    fn execute_todo_write_tool(&mut self, call: &ToolCall) -> ToolResult {
        match normalize_todo_write_state(call) {
            Ok(state) => {
                self.todos = state.clone();
                ToolResult {
                    call_id: call.id.clone(),
                    status: ToolResultStatus::Ok,
                    output: Some(todo_write_output(&state)),
                    error: None,
                }
            }
            Err((status, error)) => ToolResult {
                call_id: call.id.clone(),
                status,
                output: None,
                error: Some(error),
            },
        }
    }

    fn sandbox_policy_for_turn(
        &self,
        turn: &Turn,
        cwd: &str,
        fallback: SandboxPolicy,
    ) -> SandboxPolicy {
        self.turn_sandbox_policies
            .get(&turn.id)
            .cloned()
            .map(|policy| policy_with_cwd_defaults(policy, cwd))
            .unwrap_or(fallback)
    }

    fn guardian_auto_review_tool_call(
        &self,
        turn: &Turn,
        registry: &ToolRegistry,
        call: &ToolCall,
    ) -> Option<GuardianReviewResult> {
        let definition = lookup_tool(registry, call)?;
        let cwd = self
            .threads
            .get(&turn.thread_id)
            .map(|thread| thread.project.cwd.clone())
            .unwrap_or_else(|| ".".to_string());
        let Ok(Some((fallback, request))) =
            guardian_review_request_for_tool(definition, call, &cwd)
        else {
            return None;
        };
        let policy = call
            .arguments
            .get("policy")
            .cloned()
            .and_then(|value| serde_json::from_value::<SandboxPolicy>(value).ok())
            .unwrap_or_else(|| self.sandbox_policy_for_turn(turn, &cwd, fallback));
        if policy.permission_profile.mode != PermissionMode::AutoReview {
            return None;
        }
        if call.arguments.to_string().len() > 32_000 {
            return Some(guardian_review_result(
                GuardianReviewDecision::Deny,
                RiskLevel::Critical,
                "auto-review input exceeded bounded argument limit",
                call,
            ));
        }
        if value_contains_raw_secret(&call.arguments) {
            return Some(guardian_review_result(
                GuardianReviewDecision::Deny,
                RiskLevel::High,
                "auto-review denied raw secret-bearing tool arguments",
                call,
            ));
        }
        match evaluate_exec(&policy, &request) {
            PolicyDecision::Deny { reason } => Some(guardian_review_result(
                GuardianReviewDecision::Deny,
                RiskLevel::High,
                reason,
                call,
            )),
            PolicyDecision::Ask { risk, reason } if reason.contains("protected path") => {
                Some(guardian_review_result(
                    GuardianReviewDecision::Deny,
                    risk.max(RiskLevel::High),
                    reason,
                    call,
                ))
            }
            PolicyDecision::Ask { risk, reason } => Some(guardian_review_result(
                GuardianReviewDecision::Ask,
                risk,
                format!("guardian auto-review completed; user approval still required: {reason}"),
                call,
            )),
            PolicyDecision::Allow => Some(guardian_review_result(
                GuardianReviewDecision::Ask,
                RiskLevel::Medium,
                "guardian auto-review completed; user approval still required for approval-gated tool",
                call,
            )),
        }
    }

    fn emit_guardian_review_diagnostic(
        &mut self,
        turn: &Turn,
        review: &GuardianReviewResult,
        events: &mut Vec<Event>,
    ) {
        events.push(self.event(
            &turn.thread_id,
            Some(&turn.id),
            EventKind::Diagnostic {
                diagnostic: Diagnostic {
                    level: match review.decision {
                        GuardianReviewDecision::Ask => DiagnosticLevel::Info,
                        GuardianReviewDecision::Deny => DiagnosticLevel::Warning,
                    },
                    message: "guardian auto-review decision recorded".to_string(),
                    metadata: BTreeMap::from([
                        ("component".to_string(), "guardian-auto-review".to_string()),
                        (
                            "decision".to_string(),
                            guardian_decision_name(review.decision).to_string(),
                        ),
                        ("risk".to_string(), risk_level_name(review.risk).to_string()),
                        ("reason".to_string(), review.reason.clone()),
                        ("strictJson".to_string(), review.strict_json.clone()),
                    ]),
                },
            },
        ));
    }

    fn execute_shell_task_tool(&mut self, call: &ToolCall) -> ToolResult {
        self.refresh_shell_tasks();
        let action = call
            .arguments
            .get("action")
            .and_then(Value::as_str)
            .unwrap_or("list");
        let output = (|| -> Result<String, String> {
            match action {
                "list" => {
                    if self.shell_tasks.is_empty() {
                        Ok(
                            "no background shell tasks (background shell tasks are process-local)"
                                .to_string(),
                        )
                    } else {
                        Ok(self
                            .shell_tasks
                            .values()
                            .map(shell_task_line)
                            .collect::<Vec<_>>()
                            .join("\n"))
                    }
                }
                "read" => {
                    let task_id =
                        required_string_arg(call, "taskId").map_err(|(_, error)| error)?;
                    let max_bytes = usize_arg(call, "maxBytes").unwrap_or(30_000);
                    let read = self
                        .read_background_task(BackgroundReadParams {
                            task_id: task_id.to_string(),
                            max_bytes: Some(max_bytes),
                        })
                        .map_err(|error| error.message)?;
                    let status = format!("{:?}", read.task.status).to_lowercase();
                    let prefix = if read.truncated.unwrap_or(false) {
                        format!(
                            "background shell task {task_id} [{status}] tail ({} of {}B):",
                            max_bytes,
                            read.output_bytes.unwrap_or_default()
                        )
                    } else {
                        format!("background shell task {task_id} [{status}] output:")
                    };
                    if read.output.is_empty() {
                        Ok(format!(
                            "background shell task {task_id} [{status}]: no output yet"
                        ))
                    } else {
                        Ok(format!("{prefix}\n{}", read.output))
                    }
                }
                "kill" => {
                    let task_id =
                        required_string_arg(call, "taskId").map_err(|(_, error)| error)?;
                    self.kill_background_task(BackgroundKillParams {
                        task_id: task_id.to_string(),
                    })
                    .map(|result| result.message)
                    .map_err(|error| error.message)
                }
                other => Err(format!(
                    "shell_task action must be list, read, or kill; got {other}"
                )),
            }
        })();
        match output {
            Ok(output) => ToolResult {
                call_id: call.id.clone(),
                status: ToolResultStatus::Ok,
                output: Some(output),
                error: None,
            },
            Err(error) => ToolResult {
                call_id: call.id.clone(),
                status: ToolResultStatus::Error,
                output: None,
                error: Some(error),
            },
        }
    }

    pub fn list_background_tasks(&mut self) -> BackgroundListResult {
        self.refresh_shell_tasks();
        let mut items = self
            .shell_tasks
            .values()
            .map(background_task_info)
            .collect::<Vec<_>>();
        items.sort_by(|a, b| {
            b.started_at_ms
                .cmp(&a.started_at_ms)
                .then_with(|| a.id.cmp(&b.id))
        });
        BackgroundListResult { items }
    }

    pub fn read_background_task(
        &mut self,
        params: BackgroundReadParams,
    ) -> Result<BackgroundReadResult, RuntimeError> {
        self.refresh_shell_tasks();
        let task = self
            .shell_tasks
            .get(&params.task_id)
            .ok_or_else(|| not_found("background_task", &params.task_id))?;
        let max_bytes = params.max_bytes.unwrap_or(30_000);
        let info = background_task_info(task);
        let output = read_task_tail(&task.output_path, max_bytes);
        let truncated = info
            .output_bytes
            .map(|bytes| bytes > max_bytes as u64)
            .filter(|value| *value);
        Ok(BackgroundReadResult {
            output,
            output_bytes: info.output_bytes,
            max_bytes: Some(max_bytes),
            truncated,
            task: info,
        })
    }

    pub fn kill_background_task(
        &mut self,
        params: BackgroundKillParams,
    ) -> Result<BackgroundKillResult, RuntimeError> {
        self.refresh_shell_tasks();
        let task = self
            .shell_tasks
            .get_mut(&params.task_id)
            .ok_or_else(|| not_found("background_task", &params.task_id))?;
        let message = if task.status == ShellTaskStatus::Running {
            if let Some(child) = task.child.as_mut() {
                let _ = child.kill();
                let _ = child.wait();
            }
            task.child = None;
            task.status = ShellTaskStatus::Killed;
            task.finished_at_ms = Some(now_millis());
            task.exit_code = None;
            format!("killed background shell task {}", params.task_id)
        } else {
            format!(
                "background shell task {} is already {}",
                params.task_id,
                task.status.as_str()
            )
        };
        Ok(BackgroundKillResult {
            task: background_task_info(task),
            message,
        })
    }

    fn refresh_shell_tasks(&mut self) {
        for task in self.shell_tasks.values_mut() {
            if task.status != ShellTaskStatus::Running {
                continue;
            }
            if let Some(child) = task.child.as_mut() {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        task.status = if status.success() {
                            ShellTaskStatus::Completed
                        } else {
                            ShellTaskStatus::Failed
                        };
                        task.exit_code = status.code();
                        task.finished_at_ms = Some(now_millis());
                        task.child = None;
                    }
                    Ok(None) => {}
                    Err(_) => {
                        task.status = ShellTaskStatus::Failed;
                        task.finished_at_ms = Some(now_millis());
                        task.child = None;
                    }
                }
            }
        }
    }

    fn start_background_shell_task(
        &mut self,
        call: &ToolCall,
        policy: SandboxPolicy,
        request: SandboxExecRequest,
        cwd: String,
    ) -> ToolResult {
        if let Some(reason) = protected_path_preflight_denial(&policy, &request) {
            return ToolResult {
                call_id: call.id.clone(),
                status: ToolResultStatus::Denied,
                output: None,
                error: Some(reason),
            };
        }
        let task_id = format!(
            "shell-{}-{}",
            self.next_event + 1,
            self.shell_tasks.len() + 1
        );
        let output_dir = Path::new(&cwd).join("output").join("shelltool");
        if let Err(error) = fs::create_dir_all(&output_dir) {
            return ToolResult {
                call_id: call.id.clone(),
                status: ToolResultStatus::Error,
                output: None,
                error: Some(format!("failed to create shell output directory: {error}")),
            };
        }
        let output_path = output_dir.join(format!("{task_id}.log"));
        let stdout = match fs::File::create(&output_path) {
            Ok(file) => file,
            Err(error) => {
                return ToolResult {
                    call_id: call.id.clone(),
                    status: ToolResultStatus::Error,
                    output: None,
                    error: Some(format!("failed to create shell output file: {error}")),
                };
            }
        };
        let stderr = match stdout.try_clone() {
            Ok(file) => file,
            Err(error) => {
                return ToolResult {
                    call_id: call.id.clone(),
                    status: ToolResultStatus::Error,
                    output: None,
                    error: Some(format!("failed to clone shell output file: {error}")),
                };
            }
        };
        let spawn = spawn_sandboxed_background(SandboxBackgroundSpawnParams {
            policy: policy.clone(),
            request: request.clone(),
            preference: SandboxPreference::Require,
            approval_granted: true,
            stdout,
            stderr,
            managed_network: None,
        });
        match spawn {
            Ok((child, plan)) => {
                self.shell_tasks.insert(
                    task_id.clone(),
                    ShellTaskRecord {
                        id: task_id.clone(),
                        command: request.command,
                        cwd,
                        output_path: output_path.clone(),
                        status: ShellTaskStatus::Running,
                        child: Some(child),
                        started_at_ms: now_millis(),
                        finished_at_ms: None,
                        exit_code: None,
                    },
                );
                ToolResult {
                    call_id: call.id.clone(),
                    status: ToolResultStatus::Ok,
                    output: Some(format!(
                        "background shell task started: {task_id}\noutput: {}\nsandbox: {:?}/{:?}",
                        output_path.display(),
                        plan.enforcement,
                        plan.sandbox_type
                    )),
                    error: None,
                }
            }
            Err(error) => ToolResult {
                call_id: call.id.clone(),
                status: ToolResultStatus::Denied,
                output: None,
                error: Some(format!(
                    "background shell task was not started because sandboxed background execution is unavailable: {error}"
                )),
            },
        }
    }

    fn execute_shell_tool(&mut self, turn: &Turn, call: &ToolCall) -> ToolResult {
        let Some(command) = call
            .arguments
            .get("command")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
        else {
            return ToolResult {
                call_id: call.id.clone(),
                status: ToolResultStatus::Error,
                output: None,
                error: Some("shell tool requires a string command argument".to_string()),
            };
        };
        let cwd = call
            .arguments
            .get("cwd")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
            .or_else(|| {
                self.threads
                    .get(&turn.thread_id)
                    .map(|thread| thread.project.cwd.clone())
            })
            .unwrap_or_else(|| ".".to_string());
        let request = SandboxExecRequest {
            command,
            cwd: cwd.clone(),
            writes_files: call
                .arguments
                .get("writesFiles")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
            uses_network: call
                .arguments
                .get("usesNetwork")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
            touches_protected_path: call
                .arguments
                .get("touchesProtectedPath")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
            touched_paths: call
                .arguments
                .get("touchedPaths")
                .and_then(serde_json::Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default(),
        };
        let policy = call
            .arguments
            .get("policy")
            .cloned()
            .and_then(|value| serde_json::from_value::<SandboxPolicy>(value).ok())
            .unwrap_or_else(|| {
                self.sandbox_policy_for_turn(turn, &cwd, default_shell_policy(&cwd))
            });
        let run_in_background = call
            .arguments
            .get("runInBackground")
            .or_else(|| call.arguments.get("run_in_background"))
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        if let Some(reason) = protected_path_preflight_denial(&policy, &request) {
            return ToolResult {
                call_id: call.id.clone(),
                status: ToolResultStatus::Denied,
                output: None,
                error: Some(reason),
            };
        }
        if run_in_background {
            return self.start_background_shell_task(call, policy, request, cwd);
        }
        let result = execute_sandboxed(SandboxExecParams {
            policy,
            request,
            preference: SandboxPreference::Auto,
            approval_granted: call
                .arguments
                .get("approvalGranted")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
            managed_network: call
                .arguments
                .get("managedNetwork")
                .cloned()
                .and_then(|value| serde_json::from_value(value).ok()),
            timeout_ms: call
                .arguments
                .get("timeoutMs")
                .and_then(serde_json::Value::as_u64),
            max_output_bytes: call
                .arguments
                .get("maxOutputBytes")
                .and_then(serde_json::Value::as_u64)
                .and_then(|value| usize::try_from(value).ok()),
        });
        let output = [result.stdout.trim_end(), result.stderr.trim_end()]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        match result.decision {
            SandboxPolicyDecision::Allow if result.timed_out => ToolResult {
                call_id: call.id.clone(),
                status: ToolResultStatus::Aborted,
                output: if output.is_empty() {
                    None
                } else {
                    Some(output)
                },
                error: Some("sandboxed command timed out".to_string()),
            },
            SandboxPolicyDecision::Allow if result.exit_code == Some(0) => ToolResult {
                call_id: call.id.clone(),
                status: ToolResultStatus::Ok,
                output: Some(output),
                error: None,
            },
            SandboxPolicyDecision::Allow => ToolResult {
                call_id: call.id.clone(),
                status: ToolResultStatus::Error,
                output: if output.is_empty() {
                    None
                } else {
                    Some(output)
                },
                error: Some(format!(
                    "sandboxed command exited with {:?}",
                    result.exit_code
                )),
            },
            SandboxPolicyDecision::Deny { reason } => ToolResult {
                call_id: call.id.clone(),
                status: ToolResultStatus::Denied,
                output: None,
                error: Some(reason),
            },
            SandboxPolicyDecision::Ask { reason, .. } => ToolResult {
                call_id: call.id.clone(),
                status: ToolResultStatus::Denied,
                output: None,
                error: Some(reason),
            },
        }
    }

    fn execute_feedback_tool(&self, turn: &Turn, call: &ToolCall) -> ToolResult {
        match self.execute_feedback_tool_inner(turn, call) {
            Ok(output) => ToolResult {
                call_id: call.id.clone(),
                status: ToolResultStatus::Ok,
                output: Some(output),
                error: None,
            },
            Err((status, error)) => ToolResult {
                call_id: call.id.clone(),
                status,
                output: None,
                error: Some(error),
            },
        }
    }

    fn execute_feedback_tool_inner(
        &self,
        turn: &Turn,
        call: &ToolCall,
    ) -> Result<String, (ToolResultStatus, String)> {
        let kind = call
            .arguments
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                (
                    ToolResultStatus::Error,
                    "oppi_feedback_submit requires type bug-report or feature-request".to_string(),
                )
            })?;
        if kind != "bug-report" && kind != "feature-request" {
            return Err((
                ToolResultStatus::Error,
                "type must be bug-report or feature-request".to_string(),
            ));
        }
        let cwd = self
            .threads
            .get(&turn.thread_id)
            .map(|thread| thread.project.cwd.clone())
            .unwrap_or_else(|| ".".to_string());
        let (title, body, repo) = feedback_body(kind, call, &cwd)?;
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let draft_path =
            resolve_project_path(&cwd, &format!(".oppi/feedback-drafts/{stamp}-{kind}.md"))?;
        self.preflight_file_access(turn, call, &cwd, &draft_path, true)?;
        if let Some(parent) = draft_path.parent() {
            fs::create_dir_all(parent).map_err(file_io_error)?;
        }
        fs::write(&draft_path, format!("# {title}\n\n{body}")).map_err(file_io_error)?;

        let endpoint = std::env::var("OPPI_FEEDBACK_ENDPOINT")
            .ok()
            .map(|value| value.trim().trim_end_matches('/').to_string())
            .filter(|value| !value.is_empty());
        let disabled = std::env::var("OPPI_FEEDBACK_DISABLED")
            .ok()
            .is_some_and(|value| matches!(value.trim(), "1" | "true" | "on" | "yes"));
        if disabled || endpoint.is_none() {
            return Ok(format!(
                "Feedback draft written to {} for repo {repo}. Set OPPI_FEEDBACK_ENDPOINT to enable configured intake submission.",
                draft_path.display()
            ));
        }
        let policy = self.sandbox_policy_for_turn(
            turn,
            &cwd,
            default_policy(PermissionProfile {
                mode: PermissionMode::ReadOnly,
                readable_roots: vec![cwd.clone()],
                writable_roots: Vec::new(),
                filesystem_rules: Vec::new(),
                protected_patterns: Vec::new(),
            }),
        );
        if policy.network != NetworkPolicy::Enabled {
            return Ok(format!(
                "Feedback draft written to {}. Intake submission skipped because turn network policy is not enabled.",
                draft_path.display()
            ));
        }
        let endpoint = endpoint.unwrap();
        let url = format!("{endpoint}/v1/intake/{kind}");
        let request = ureq::post(&url).set("content-type", "application/json");
        let request = if let Ok(token) = std::env::var("OPPI_FEEDBACK_TOKEN") {
            if token.trim().is_empty() {
                request
            } else {
                request.set("x-oppi-intake-token", token.trim())
            }
        } else {
            request
        };
        match request.send_json(json!({
            "repo": repo,
            "type": kind,
            "title": title,
            "body": body,
            "labels": if kind == "bug-report" { vec!["bug"] } else { vec!["enhancement"] },
        })) {
            Ok(response) => {
                let value = response.into_json::<Value>().unwrap_or_else(|_| json!({}));
                let issue_url = value
                    .get("issueUrl")
                    .or_else(|| value.get("html_url"))
                    .and_then(Value::as_str)
                    .unwrap_or("submitted");
                Ok(format!(
                    "Feedback submitted: {issue_url}\nDraft copy: {}",
                    draft_path.display()
                ))
            }
            Err(error) => Ok(format!(
                "Feedback draft written to {}. Intake submission failed: {}",
                draft_path.display(),
                feedback_sanitize(&error.to_string())
            )),
        }
    }

    fn execute_render_mermaid_tool(&self, turn: &Turn, call: &ToolCall) -> ToolResult {
        match self.execute_render_mermaid_tool_inner(turn, call) {
            Ok(output) => ToolResult {
                call_id: call.id.clone(),
                status: ToolResultStatus::Ok,
                output: Some(output),
                error: None,
            },
            Err((status, error)) => ToolResult {
                call_id: call.id.clone(),
                status,
                output: None,
                error: Some(error),
            },
        }
    }

    fn execute_image_gen_tool(&self, turn: &Turn, call: &ToolCall) -> ToolResult {
        match self.execute_image_gen_tool_inner(turn, call) {
            Ok(output) => ToolResult {
                call_id: call.id.clone(),
                status: ToolResultStatus::Ok,
                output: Some(output),
                error: None,
            },
            Err((status, error)) => ToolResult {
                call_id: call.id.clone(),
                status,
                output: None,
                error: Some(error),
            },
        }
    }

    fn execute_image_gen_tool_inner(
        &self,
        turn: &Turn,
        call: &ToolCall,
    ) -> Result<String, (ToolResultStatus, String)> {
        let prompt = call
            .arguments
            .get("prompt")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                (
                    ToolResultStatus::Error,
                    "image_gen requires a non-empty prompt".to_string(),
                )
            })?;
        let model = call
            .arguments
            .get("model")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("gpt-image-2")
            .trim();
        let output_format = call
            .arguments
            .get("outputFormat")
            .or_else(|| call.arguments.get("output_format"))
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("png")
            .trim()
            .trim_start_matches('.')
            .to_ascii_lowercase();
        let background = call
            .arguments
            .get("background")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty());
        if background == Some("transparent") {
            let confirmed = call
                .arguments
                .get("transparentBackgroundConfirmed")
                .or_else(|| call.arguments.get("confirmTransparentBackground"))
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if !confirmed || model != "gpt-image-1.5" {
                return Err((
                    ToolResultStatus::Error,
                    "image_gen transparent background requires explicit confirmation and model gpt-image-1.5; gpt-image-2 does not support true transparent output".to_string(),
                ));
            }
        }
        let n = call.arguments.get("n").and_then(Value::as_u64).unwrap_or(1);
        if n != 1 {
            return Err((
                ToolResultStatus::Error,
                "image_gen native backend currently supports n=1; use a host adapter for multiple outputs".to_string(),
            ));
        }
        let cwd = self
            .threads
            .get(&turn.thread_id)
            .map(|thread| thread.project.cwd.clone())
            .unwrap_or_else(|| ".".to_string());
        let output_path = call
            .arguments
            .get("outputPath")
            .or_else(|| call.arguments.get("output_path"))
            .and_then(Value::as_str)
            .map(|value| value.to_string())
            .unwrap_or_else(|| default_image_output_path(prompt, &output_format));
        let path = resolve_project_path(&cwd, &output_path)?;
        self.preflight_file_access(turn, call, &cwd, &path, true)?;
        let overwrite = call
            .arguments
            .get("overwrite")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if path.exists() && !overwrite {
            return Err((
                ToolResultStatus::Error,
                format!(
                    "image_gen output already exists: {}. Set overwrite=true or choose another outputPath.",
                    path.display()
                ),
            ));
        }
        let explicit_env = call
            .arguments
            .get("apiKeyEnv")
            .or_else(|| call.arguments.get("api_key_env"))
            .and_then(Value::as_str);
        let candidates = image_api_key_env_candidates(explicit_env).map_err(|error| {
            (
                ToolResultStatus::Denied,
                format!("image_gen credential policy denied: {}", error.message),
            )
        })?;
        let api_key = candidates
            .iter()
            .find_map(|name| {
                std::env::var(name)
                    .ok()
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty())
            })
            .ok_or_else(|| {
                (
                    ToolResultStatus::Error,
                    format!(
                        "image_gen requires an approved image backend API key env: {}",
                        candidates.join(", ")
                    ),
                )
            })?;
        let policy = self.sandbox_policy_for_turn(
            turn,
            &cwd,
            default_policy(PermissionProfile {
                mode: PermissionMode::ReadOnly,
                readable_roots: vec![cwd.clone()],
                writable_roots: Vec::new(),
                filesystem_rules: Vec::new(),
                protected_patterns: Vec::new(),
            }),
        );
        if policy.network != NetworkPolicy::Enabled {
            return Err((
                ToolResultStatus::Denied,
                "image_gen requires network policy enabled for the native image backend"
                    .to_string(),
            ));
        }
        let endpoint = image_generation_endpoint(call);
        let body = image_generation_request_body(call, model, prompt, &output_format, background);
        let started = Instant::now();
        let response = ureq::post(&endpoint)
            .set("authorization", &format!("Bearer {api_key}"))
            .set("content-type", "application/json")
            .send_json(body);
        let value = match response {
            Ok(response) => response.into_json::<Value>().map_err(|error| {
                (
                    ToolResultStatus::Error,
                    format!("image_gen backend returned invalid JSON: {error}"),
                )
            })?,
            Err(ureq::Error::Status(status, response)) => {
                let detail = response.into_string().unwrap_or_default();
                return Err((
                    ToolResultStatus::Error,
                    format!(
                        "image_gen backend HTTP request failed with status {status}{}",
                        if detail.trim().is_empty() {
                            String::new()
                        } else {
                            format!(": {}", compact_provider_error(&detail))
                        }
                    ),
                ));
            }
            Err(error) => {
                return Err((
                    ToolResultStatus::Error,
                    format!("image_gen backend transport error: {error}"),
                ));
            }
        };
        let image = value
            .get("data")
            .and_then(Value::as_array)
            .and_then(|items| items.first())
            .ok_or_else(|| {
                (
                    ToolResultStatus::Error,
                    "image_gen backend response did not include data[0]".to_string(),
                )
            })?;
        let bytes = if let Some(b64) = image.get("b64_json").and_then(Value::as_str) {
            decode_base64(b64).map_err(|error| (ToolResultStatus::Error, error))?
        } else if let Some(url) = image.get("url").and_then(Value::as_str) {
            return Ok(json!({
                "message": "image_gen backend returned a URL instead of b64_json; download is intentionally not implicit yet",
                "url": url,
                "backend": "openai-images",
                "model": model,
                "endpoint": redacted_endpoint(&endpoint),
                "durationMs": started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
            })
            .to_string());
        } else {
            return Err((
                ToolResultStatus::Error,
                "image_gen backend response requires data[0].b64_json or data[0].url".to_string(),
            ));
        };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(file_io_error)?;
        }
        fs::write(&path, &bytes).map_err(file_io_error)?;
        let (width, height) = image_dimensions(&bytes, &output_format);
        let mut output = json!({
            "message": "Generated image with native OpenAI-compatible image backend",
            "outputPath": path.display().to_string(),
            "mimeType": mime_type_for_image_format(&output_format),
            "bytes": bytes.len(),
            "backend": "openai-images",
            "model": model,
            "endpoint": redacted_endpoint(&endpoint),
            "durationMs": started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
        });
        if let Some(width) = width {
            output["width"] = json!(width);
        }
        if let Some(height) = height {
            output["height"] = json!(height);
        }
        Ok(output.to_string())
    }

    fn execute_render_mermaid_tool_inner(
        &self,
        turn: &Turn,
        call: &ToolCall,
    ) -> Result<String, (ToolResultStatus, String)> {
        let source = required_string_arg(call, "mermaid")?
            .replace("\r\n", "\n")
            .replace('\r', "\n")
            .trim()
            .to_string();
        if source.is_empty() {
            return Err((
                ToolResultStatus::Error,
                "mermaid must not be empty".to_string(),
            ));
        }
        if source.len() > 20_000 {
            return Err((
                ToolResultStatus::Error,
                format!(
                    "Mermaid source is too large ({} chars; max 20000)",
                    source.len()
                ),
            ));
        }
        let ascii = render_mermaid_fallback(&source)?;
        let cwd = self
            .threads
            .get(&turn.thread_id)
            .map(|thread| thread.project.cwd.clone())
            .unwrap_or_else(|| ".".to_string());
        let saved = if let Some(output_path) =
            call.arguments.get("outputPath").and_then(Value::as_str)
        {
            let path = resolve_project_path(&cwd, output_path)?;
            self.preflight_file_access(turn, call, &cwd, &path, true)?;
            let overwrite = call
                .arguments
                .get("overwrite")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if path.exists() && !overwrite {
                return Err((
                    ToolResultStatus::Error,
                    format!(
                        "Output already exists: {}. Set overwrite=true or choose another outputPath.",
                        path.display()
                    ),
                ));
            }
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(file_io_error)?;
            }
            fs::write(&path, &ascii).map_err(file_io_error)?;
            Some(path.display().to_string())
        } else {
            None
        };
        let mut lines = vec!["Rendered Mermaid diagram with Rust fallback.".to_string()];
        if let Some(path) = saved {
            lines.push(format!("Saved ASCII output to {path}."));
        }
        lines.push(String::new());
        lines.push("```text".to_string());
        lines.push(ascii);
        lines.push("```".to_string());
        Ok(lines.join("\n"))
    }

    fn execute_file_tool(&self, turn: &Turn, call: &ToolCall, kind: FileToolKind) -> ToolResult {
        match self.execute_file_tool_inner(turn, call, kind) {
            Ok(output) => ToolResult {
                call_id: call.id.clone(),
                status: ToolResultStatus::Ok,
                output: Some(output),
                error: None,
            },
            Err((status, error)) => ToolResult {
                call_id: call.id.clone(),
                status,
                output: None,
                error: Some(error),
            },
        }
    }

    fn execute_file_tool_inner(
        &self,
        turn: &Turn,
        call: &ToolCall,
        kind: FileToolKind,
    ) -> Result<String, (ToolResultStatus, String)> {
        let cwd = self
            .threads
            .get(&turn.thread_id)
            .map(|thread| thread.project.cwd.clone())
            .unwrap_or_else(|| ".".to_string());
        match kind {
            FileToolKind::Read => {
                let path = required_string_arg(call, "path")?;
                let path = resolve_project_path(&cwd, &path)?;
                self.preflight_file_access(turn, call, &cwd, &path, false)?;
                let max_bytes = usize_arg(call, "maxBytes").unwrap_or(65_536);
                if call.arguments.get("offset").is_some() || call.arguments.get("limit").is_some() {
                    let offset = usize_arg(call, "offset").unwrap_or(1).max(1);
                    let limit = usize_arg(call, "limit").unwrap_or(200).max(1);
                    let text = fs::read_to_string(&path).map_err(file_io_error)?;
                    return Ok(text
                        .lines()
                        .skip(offset.saturating_sub(1))
                        .take(limit)
                        .collect::<Vec<_>>()
                        .join("\n"));
                }
                let bytes = fs::read(&path).map_err(file_io_error)?;
                Ok(String::from_utf8_lossy(&bytes[..bytes.len().min(max_bytes)]).to_string())
            }
            FileToolKind::List => {
                let root = call
                    .arguments
                    .get("path")
                    .and_then(Value::as_str)
                    .unwrap_or(".");
                let root = resolve_project_path(&cwd, root)?;
                self.preflight_file_access(turn, call, &cwd, &root, false)?;
                let max_results = usize_arg(call, "maxResults")
                    .or_else(|| usize_arg(call, "limit"))
                    .unwrap_or(200)
                    .max(1);
                let mut entries = fs::read_dir(&root)
                    .map_err(file_io_error)?
                    .filter_map(Result::ok)
                    .map(|entry| {
                        let name = entry.file_name().to_string_lossy().to_string();
                        let suffix = if entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false)
                        {
                            "/"
                        } else {
                            ""
                        };
                        format!("{name}{suffix}")
                    })
                    .collect::<Vec<_>>();
                entries.sort();
                entries.truncate(max_results);
                if entries.is_empty() {
                    Ok("empty directory".to_string())
                } else {
                    Ok(entries.join("\n"))
                }
            }
            FileToolKind::Write => {
                let path = required_string_arg(call, "path")?;
                let content = required_string_arg(call, "content")?;
                let path = resolve_project_path(&cwd, &path)?;
                self.preflight_file_access(turn, call, &cwd, &path, true)?;
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent).map_err(file_io_error)?;
                }
                fs::write(&path, content).map_err(file_io_error)?;
                Ok(format!(
                    "wrote {} bytes to {}",
                    content.len(),
                    path.display()
                ))
            }
            FileToolKind::Edit => {
                let path = required_string_arg(call, "path")?;
                let old_text = required_string_arg(call, "oldText")?;
                let new_text = required_string_arg(call, "newText")?;
                let replace_all = call
                    .arguments
                    .get("replaceAll")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let path = resolve_project_path(&cwd, &path)?;
                self.preflight_file_access(turn, call, &cwd, &path, true)?;
                let text = fs::read_to_string(&path).map_err(file_io_error)?;
                let matches = text.matches(old_text).count();
                if matches == 0 {
                    return Err((ToolResultStatus::Error, "oldText was not found".to_string()));
                }
                if matches > 1 && !replace_all {
                    return Err((
                        ToolResultStatus::Error,
                        format!("oldText matched {matches} times; set replaceAll to true"),
                    ));
                }
                let updated = if replace_all {
                    text.replace(old_text, new_text)
                } else {
                    text.replacen(old_text, new_text, 1)
                };
                fs::write(&path, updated).map_err(file_io_error)?;
                Ok(format!(
                    "edited {} replacement(s) in {}",
                    matches,
                    path.display()
                ))
            }
            FileToolKind::Search => {
                let query =
                    first_string_arg(&call.arguments, &["query", "pattern"]).ok_or_else(|| {
                        (
                            ToolResultStatus::Error,
                            "tool argument 'query' or 'pattern' must be a non-empty string"
                                .to_string(),
                        )
                    })?;
                let root = call
                    .arguments
                    .get("path")
                    .and_then(Value::as_str)
                    .unwrap_or(".");
                let root = resolve_project_path(&cwd, root)?;
                self.preflight_file_access(turn, call, &cwd, &root, false)?;
                let max_results = usize_arg(call, "maxResults").unwrap_or(50);
                let max_bytes_per_file = usize_arg(call, "maxBytesPerFile").unwrap_or(1_048_576);
                let mut results = Vec::new();
                search_files_under(
                    self,
                    turn,
                    call,
                    &cwd,
                    &root,
                    &query,
                    max_results,
                    max_bytes_per_file,
                    &mut results,
                );
                if results.is_empty() {
                    Ok("no matches".to_string())
                } else {
                    Ok(results.join("\n"))
                }
            }
        }
    }

    fn preflight_file_access(
        &self,
        turn: &Turn,
        call: &ToolCall,
        cwd: &str,
        path: &Path,
        writes: bool,
    ) -> Result<(), (ToolResultStatus, String)> {
        let policy = call
            .arguments
            .get("policy")
            .cloned()
            .and_then(|value| serde_json::from_value::<SandboxPolicy>(value).ok())
            .unwrap_or_else(|| {
                self.sandbox_policy_for_turn(turn, cwd, default_file_tool_policy(cwd, writes))
            });
        if !file_path_allowed_by_roots(path, cwd, &policy, writes) {
            return Err((
                ToolResultStatus::Denied,
                "filesystem policy blocks access outside configured roots".to_string(),
            ));
        }
        let request = SandboxExecRequest {
            command: format!("file-tool:{}", call.name),
            cwd: cwd.to_string(),
            writes_files: writes,
            uses_network: false,
            touches_protected_path: false,
            touched_paths: vec![path.to_string_lossy().to_string()],
        };
        match evaluate_exec(&policy, &request) {
            PolicyDecision::Allow => Ok(()),
            PolicyDecision::Deny { reason } => Err((ToolResultStatus::Denied, reason)),
            PolicyDecision::Ask { reason, .. } => Err((ToolResultStatus::Denied, reason)),
        }
    }

    fn emit_hook_diagnostic(
        &mut self,
        turn: &Turn,
        hook: &str,
        message: String,
        events: &mut Vec<Event>,
    ) {
        events.push(self.event(
            &turn.thread_id,
            Some(&turn.id),
            EventKind::Diagnostic {
                diagnostic: Diagnostic {
                    level: DiagnosticLevel::Info,
                    message,
                    metadata: BTreeMap::from([("hook".to_string(), hook.to_string())]),
                },
            },
        ));
    }

    fn record_tool_pair(
        &mut self,
        thread_id: &str,
        turn_id: &str,
        simulated: SimulatedToolUse,
    ) -> Result<Vec<Event>, RuntimeError> {
        self.record_tool_batch_items(
            thread_id,
            turn_id,
            vec![simulated],
            ToolBatchExecution::Exclusive,
        )
    }

    fn record_tool_batch_items(
        &mut self,
        thread_id: &str,
        turn_id: &str,
        tools: Vec<SimulatedToolUse>,
        execution: ToolBatchExecution,
    ) -> Result<Vec<Event>, RuntimeError> {
        self.validate_tool_batch_items(thread_id, turn_id, &tools)?;

        let mut events = Vec::new();
        match execution {
            ToolBatchExecution::Concurrent => {
                for simulated in &tools {
                    self.emit_tool_call_started(thread_id, turn_id, simulated, &mut events);
                }
                for simulated in tools {
                    self.emit_tool_result_completed(thread_id, turn_id, simulated, &mut events);
                }
            }
            ToolBatchExecution::Exclusive => {
                for simulated in tools {
                    self.emit_tool_call_started(thread_id, turn_id, &simulated, &mut events);
                    self.emit_tool_result_completed(thread_id, turn_id, simulated, &mut events);
                }
            }
        }
        Ok(events)
    }

    fn emit_tool_call_started(
        &mut self,
        thread_id: &str,
        turn_id: &str,
        simulated: &SimulatedToolUse,
        events: &mut Vec<Event>,
    ) {
        let call_item = self.item(
            thread_id,
            turn_id,
            ItemKind::ToolCall(simulated.call.clone()),
        );
        events.push(self.event(
            thread_id,
            Some(turn_id),
            EventKind::ToolCallStarted {
                call: simulated.call.clone(),
            },
        ));
        events.push(self.event(
            thread_id,
            Some(turn_id),
            EventKind::ItemCompleted { item: call_item },
        ));

        if simulated.require_approval
            && !self.has_approved_tool_call(thread_id, turn_id, &simulated.call.id)
        {
            self.next_approval += 1;
            let request = ApprovalRequest {
                id: format!("approval-{}", self.next_approval),
                reason: format!("tool {} requires approval", simulated.call.name),
                risk: RiskLevel::Medium,
                tool_call: Some(simulated.call.clone()),
            };
            self.approvals.insert(
                request.id.clone(),
                OwnedApprovalRequest {
                    thread_id: thread_id.to_string(),
                    turn_id: Some(turn_id.to_string()),
                    tool_call_id: Some(simulated.call.id.clone()),
                    tool_call: Some(simulated.call.clone()),
                    outcome: None,
                },
            );
            events.push(self.event(
                thread_id,
                Some(turn_id),
                EventKind::ApprovalRequested { request },
            ));
        }
    }

    fn emit_tool_result_completed(
        &mut self,
        thread_id: &str,
        turn_id: &str,
        simulated: SimulatedToolUse,
        events: &mut Vec<Event>,
    ) {
        let result_item = self.item(
            thread_id,
            turn_id,
            ItemKind::ToolResult(simulated.result.clone()),
        );
        events.push(self.event(
            thread_id,
            Some(turn_id),
            EventKind::ToolCallCompleted {
                result: simulated.result.clone(),
            },
        ));
        events.push(self.event(
            thread_id,
            Some(turn_id),
            EventKind::ItemCompleted { item: result_item },
        ));
    }

    fn set_phase(&mut self, turn: &mut Turn, phase: TurnPhase, emitted: &mut Vec<Event>) {
        turn.phase = phase;
        self.turns.insert(turn.id.clone(), turn.clone());
        emitted.push(self.event(
            &turn.thread_id,
            Some(&turn.id),
            EventKind::TurnPhaseChanged { phase },
        ));
    }

    fn thread(&self, thread_id: &str) -> Result<&Thread, RuntimeError> {
        self.threads
            .get(thread_id)
            .ok_or_else(|| not_found("thread", thread_id))
    }

    fn turn(&self, turn_id: &str) -> Result<&Turn, RuntimeError> {
        self.turns
            .get(turn_id)
            .ok_or_else(|| not_found("turn", turn_id))
    }

    fn turn_in_thread(&self, thread_id: &str, turn_id: &str) -> Result<&Turn, RuntimeError> {
        let turn = self.turn(turn_id)?;
        if turn.thread_id != thread_id {
            return Err(RuntimeError::new(
                "turn_thread_mismatch",
                RuntimeErrorCategory::OwnershipMismatch,
                format!("turn {turn_id} does not belong to thread {thread_id}"),
            ));
        }
        Ok(turn)
    }

    fn active_agent(&self, name: &str) -> Result<AgentDefinition, RuntimeError> {
        let definitions = self
            .agents
            .get(name)
            .ok_or_else(|| not_found("agent", name))?;
        resolve_active_agents(definitions.clone())
            .into_iter()
            .next()
            .map(|resolved| resolved.active)
            .ok_or_else(|| not_found("agent", name))
    }

    fn agent_run(&self, run_id: &str) -> Result<&AgentRun, RuntimeError> {
        self.agent_runs
            .get(run_id)
            .ok_or_else(|| not_found("agent run", run_id))
    }

    fn agent_run_in_thread(
        &self,
        thread_id: &str,
        run_id: &str,
    ) -> Result<&AgentRun, RuntimeError> {
        let run = self.agent_run(run_id)?;
        if run.thread_id != thread_id {
            return Err(RuntimeError::new(
                "agent_run_thread_mismatch",
                RuntimeErrorCategory::OwnershipMismatch,
                format!("agent run {run_id} does not belong to thread {thread_id}"),
            ));
        }
        Ok(run)
    }

    fn approval_in_thread_mut(
        &mut self,
        thread_id: &str,
        approval_id: &str,
    ) -> Result<&mut OwnedApprovalRequest, RuntimeError> {
        let owner = self
            .approvals
            .get_mut(approval_id)
            .ok_or_else(|| not_found("approval", approval_id))?;
        if owner.thread_id != thread_id {
            return Err(RuntimeError::new(
                "approval_thread_mismatch",
                RuntimeErrorCategory::OwnershipMismatch,
                format!("approval {approval_id} does not belong to thread {thread_id}"),
            ));
        }
        Ok(owner)
    }

    fn question_in_thread_mut(
        &mut self,
        thread_id: &str,
        question_id: &str,
    ) -> Result<&mut OwnedQuestionRequest, RuntimeError> {
        let owner = self
            .questions
            .get_mut(question_id)
            .ok_or_else(|| not_found("question", question_id))?;
        if owner.thread_id != thread_id {
            return Err(RuntimeError::new(
                "question_thread_mismatch",
                RuntimeErrorCategory::OwnershipMismatch,
                format!("question {question_id} does not belong to thread {thread_id}"),
            ));
        }
        Ok(owner)
    }

    fn has_approved_tool_call(&self, thread_id: &str, turn_id: &str, call_id: &str) -> bool {
        self.approvals.values().any(|owner| {
            owner.thread_id == thread_id
                && owner.tool_call_id.as_deref() == Some(call_id)
                && owner.outcome == Some(ApprovalOutcome::Approved)
                && match owner.turn_id.as_deref() {
                    Some(owner_turn_id) => owner_turn_id == turn_id,
                    None => true,
                }
        })
    }

    fn validate_tool_batch_items(
        &self,
        thread_id: &str,
        turn_id: &str,
        tools: &[SimulatedToolUse],
    ) -> Result<(), RuntimeError> {
        validate_tool_pairing(tools)?;
        for simulated in tools {
            if simulated.require_approval
                && simulated.result.status == ToolResultStatus::Ok
                && !self.has_approved_tool_call(thread_id, turn_id, &simulated.call.id)
            {
                return Err(RuntimeError::new(
                    "tool_requires_approval",
                    RuntimeErrorCategory::ApprovalRequired,
                    format!(
                        "tool call {} requires approval before it can complete successfully",
                        simulated.call.id
                    ),
                ));
            }
        }
        Ok(())
    }

    fn agent_run_mut(&mut self, run_id: &str) -> Result<&mut AgentRun, RuntimeError> {
        self.agent_runs
            .get_mut(run_id)
            .ok_or_else(|| not_found("agent run", run_id))
    }

    fn bump_thread_counter(&mut self, id: &str) {
        self.next_thread = self.next_thread.max(numeric_suffix(id).unwrap_or(0));
    }

    fn bump_turn_counter(&mut self, id: &str) {
        self.next_turn = self.next_turn.max(numeric_suffix(id).unwrap_or(0));
    }

    fn bump_item_counter(&mut self, id: &str) {
        self.next_item = self.next_item.max(numeric_suffix(id).unwrap_or(0));
    }

    fn bump_approval_counter(&mut self, id: &str) {
        self.next_approval = self.next_approval.max(numeric_suffix(id).unwrap_or(0));
    }

    fn bump_question_counter(&mut self, id: &str) {
        self.next_question = self.next_question.max(numeric_suffix(id).unwrap_or(0));
    }

    fn bump_agent_run_counter(&mut self, id: &str) {
        self.next_agent_run = self.next_agent_run.max(numeric_suffix(id).unwrap_or(0));
    }

    fn item(&mut self, thread_id: &str, turn_id: &str, kind: ItemKind) -> Item {
        self.next_item += 1;
        Item {
            id: format!("item-{}", self.next_item),
            thread_id: thread_id.to_string(),
            turn_id: turn_id.to_string(),
            kind,
        }
    }

    fn event(&mut self, thread_id: &str, turn_id: Option<&str>, kind: EventKind) -> Event {
        self.next_event += 1;
        let event = Event {
            id: self.next_event,
            thread_id: thread_id.to_string(),
            turn_id: turn_id.map(ToString::to_string),
            kind,
        };
        self.event_store
            .append(event.clone())
            .expect("in-memory event store append should not fail");
        if let Some(mirror) = &self.event_mirror
            && let Ok(mut events) = mirror.lock()
        {
            events.push(event.clone());
        }
        event
    }
}

fn numeric_suffix(id: &str) -> Option<u64> {
    id.rsplit_once('-')?.1.parse().ok()
}

fn normalize_thread_title(title: &str) -> Result<Option<String>, RuntimeError> {
    let title = title.trim();
    if title.is_empty() {
        return Ok(None);
    }
    if title.chars().count() > 160 {
        return Err(RuntimeError::new(
            "invalid_thread_title",
            RuntimeErrorCategory::InvalidRequest,
            "thread title must be 160 characters or fewer",
        ));
    }
    Ok(Some(title.to_string()))
}

fn event_store_error(error: event_store::EventStoreError) -> RuntimeError {
    RuntimeError::new(
        "event_store_error",
        RuntimeErrorCategory::EventStore,
        format!("event store error: {error:?}"),
    )
}

fn not_found(kind: &str, id: &str) -> RuntimeError {
    RuntimeError::new(
        format!("{}_not_found", kind.replace(' ', "_")),
        RuntimeErrorCategory::NotFound,
        format!("{kind} not found: {id}"),
    )
}

fn already_exists(kind: &str, id: &str) -> RuntimeError {
    RuntimeError::new(
        format!("{}_already_exists", kind.replace(' ', "_")),
        RuntimeErrorCategory::AlreadyExists,
        format!("{kind} already exists: {id}"),
    )
}

fn already_resolved(kind: &str, id: &str) -> RuntimeError {
    RuntimeError::new(
        format!("{}_already_resolved", kind.replace(' ', "_")),
        RuntimeErrorCategory::AlreadyResolved,
        format!("{kind} already resolved: {id}"),
    )
}

fn invalid_goal_request(message: String) -> RuntimeError {
    RuntimeError::new(
        "invalid_goal",
        RuntimeErrorCategory::InvalidRequest,
        message,
    )
}

fn thread_goal_status_name(status: ThreadGoalStatus) -> &'static str {
    match status {
        ThreadGoalStatus::Active => "active",
        ThreadGoalStatus::Paused => "paused",
        ThreadGoalStatus::BudgetLimited => "budget-limited",
        ThreadGoalStatus::Complete => "complete",
    }
}

fn turn_status_name(status: TurnStatus) -> &'static str {
    match status {
        TurnStatus::Queued => "queued",
        TurnStatus::Running => "running",
        TurnStatus::WaitingForApproval => "waitingForApproval",
        TurnStatus::WaitingForUser => "waitingForUser",
        TurnStatus::Completed => "completed",
        TurnStatus::Aborted => "aborted",
        TurnStatus::Failed => "failed",
    }
}

fn turn_is_terminal(turn: &Turn) -> bool {
    matches!(
        turn.status,
        TurnStatus::Completed | TurnStatus::Aborted | TurnStatus::Failed
    )
}

fn ensure_turn_mutable(turn: &Turn) -> Result<(), RuntimeError> {
    if turn_is_terminal(turn) {
        return Err(RuntimeError::new(
            "turn_not_mutable",
            RuntimeErrorCategory::TerminalState,
            format!("turn {} is terminal and cannot be mutated", turn.id),
        ));
    }
    Ok(())
}

fn validate_tool_pairing(tools: &[SimulatedToolUse]) -> Result<(), RuntimeError> {
    let mut tracker = ToolPairingTracker::new();
    for simulated in tools {
        if simulated.result.call_id != simulated.call.id {
            return Err(RuntimeError::new(
                "tool_pair_mismatch",
                RuntimeErrorCategory::ToolPairing,
                format!(
                    "tool result {} does not match tool call {}",
                    simulated.result.call_id, simulated.call.id
                ),
            ));
        }
        tracker
            .record_call(simulated.call.clone())
            .map_err(tool_error)?;
    }
    for simulated in tools {
        tracker
            .record_result(simulated.result.clone())
            .map_err(tool_error)?;
    }
    tracker.finish().map_err(tool_error)
}

fn tool_error(error: oppi_tools::ToolPairingError) -> RuntimeError {
    RuntimeError::new(
        "tool_pairing_error",
        RuntimeErrorCategory::ToolPairing,
        format!("{error:?}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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

    #[test]
    fn command_prepare_expands_review_and_independent_prompts() {
        let runtime = Runtime::new();
        let review = runtime
            .prepare_command(CommandPrepareParams {
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
        assert_eq!(tokens, 0);
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
        assert_eq!(tokens, 0);
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
                            "command": "echo shell-task-parity",
                            "runInBackground": true,
                            "cwd": root.display().to_string()
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
                            "outputFormat": "png",
                            "images": ["input.png"],
                            "mask": "mask.png"
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
                    && artifact.source_images == vec!["input.png".to_string()]
                    && artifact.mask.as_deref() == Some("mask.png")
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

    #[test]
    fn guardian_auto_review_records_strict_json_before_user_approval() {
        let root = std::env::temp_dir().join(format!("oppi-guardian-ask-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();

        let mut runtime = Runtime::new();
        let thread = runtime
            .start_thread(ThreadStartParams {
                project: ProjectRef {
                    id: "guardian".to_string(),
                    cwd: root.display().to_string(),
                    display_name: None,
                    workspace_roots: Vec::new(),
                },
                title: None,
            })
            .thread;
        let result = runtime
            .run_agentic_turn(AgenticTurnParams {
                thread_id: thread.id.clone(),
                input: "write with guardian".to_string(),
                execution_mode: ExecutionMode::Blocking,
                follow_up: None,
                sandbox_policy: Some(default_policy(PermissionProfile {
                    mode: PermissionMode::AutoReview,
                    readable_roots: vec![root.display().to_string()],
                    writable_roots: vec![root.display().to_string()],
                    filesystem_rules: Vec::new(),
                    protected_patterns: vec![".env*".to_string()],
                })),
                model_steps: vec![ScriptedModelStep {
                    assistant_deltas: Vec::new(),
                    tool_calls: vec![ToolCall {
                        id: "write-doc".to_string(),
                        name: "write_file".to_string(),
                        namespace: Some("oppi".to_string()),
                        arguments: json!({ "path": "docs/out.md", "content": "ok" }),
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

        assert_eq!(result.turn.status, TurnStatus::WaitingForApproval);
        let diagnostic = result
            .events
            .iter()
            .find_map(|event| match &event.kind {
                EventKind::Diagnostic { diagnostic }
                    if diagnostic.metadata.get("component").map(String::as_str)
                        == Some("guardian-auto-review") =>
                {
                    Some(diagnostic)
                }
                _ => None,
            })
            .expect("guardian diagnostic");
        assert_eq!(
            diagnostic.metadata.get("decision").map(String::as_str),
            Some("ask")
        );
        let strict_json = diagnostic.metadata.get("strictJson").unwrap();
        assert!(strict_json.contains(r#""decision":"ask""#));
        assert!(strict_json.contains("oppi_review_read"));
        assert!(
            result
                .events
                .iter()
                .any(|event| matches!(event.kind, EventKind::ApprovalRequested { .. }))
        );
    }

    #[test]
    fn guardian_auto_review_denies_protected_tool_without_user_approval() {
        let root = std::env::temp_dir().join(format!("oppi-guardian-deny-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();

        let mut runtime = Runtime::new();
        let thread = runtime
            .start_thread(ThreadStartParams {
                project: ProjectRef {
                    id: "guardian".to_string(),
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
                input: "try protected write".to_string(),
                execution_mode: ExecutionMode::Blocking,
                follow_up: None,
                sandbox_policy: Some(default_policy(PermissionProfile {
                    mode: PermissionMode::AutoReview,
                    readable_roots: vec![root.display().to_string()],
                    writable_roots: vec![root.display().to_string()],
                    filesystem_rules: Vec::new(),
                    protected_patterns: vec![".env*".to_string()],
                })),
                model_steps: vec![ScriptedModelStep {
                    assistant_deltas: Vec::new(),
                    tool_calls: vec![ToolCall {
                        id: "write-env".to_string(),
                        name: "write_file".to_string(),
                        namespace: Some("oppi".to_string()),
                        arguments: json!({ "path": ".env", "content": "SECRET=1" }),
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
                .contains(r#""decision":"deny""#)
        );
        assert!(!root.join(".env").exists());
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
            "usage": { "total_tokens": 15 }
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
        let thread = thread(&mut runtime);
        runtime
            .set_memory_status(MemorySetParams {
                thread_id: thread.id.clone(),
                status: MemoryStatus {
                    enabled: true,
                    backend: "client".to_string(),
                    scope: "project".to_string(),
                    memory_count: 7,
                },
            })
            .unwrap();
        let dashboard = runtime
            .memory_control(MemoryControlParams {
                thread_id: thread.id.clone(),
                action: "dashboard".to_string(),
                apply: false,
            })
            .unwrap();
        assert!(dashboard.summary.contains("client-hosted"));
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
                    && diagnostic.metadata.get("clientHosted") == Some(&"true".to_string())
        )));
        let maintenance = runtime
            .memory_control(MemoryControlParams {
                thread_id: thread.id,
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
                    model: None,
                    effort: None,
                    permission_mode: Some(PermissionMode::ReadOnly),
                    background: false,
                    worktree_root: None,
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
                    model: None,
                    effort: None,
                    permission_mode: Some(PermissionMode::ReadOnly),
                    background: false,
                    worktree_root: None,
                    instructions: "Project-specific exploration.".to_string(),
                },
            )
            .unwrap();

        let explore = runtime
            .list_agents()
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
                        model: None,
                        effort: None,
                        permission_mode: Some(PermissionMode::ReadOnly),
                        background: false,
                        worktree_root: None,
                        instructions: instructions.to_string(),
                    },
                )
                .unwrap();
        }

        let reviewer = runtime
            .list_agents()
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
        let side = runtime
            .side_question(SideQuestionParams {
                thread_id: thread.id,
                question: "what changed?".to_string(),
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
        assert!(
            side.events
                .iter()
                .all(|event| !matches!(event.kind, EventKind::ItemCompleted { .. }))
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
                    description: Some("File tools".to_string()),
                    tools: vec!["read".to_string()],
                    when_to_use: Some("when filesystem context is required".to_string()),
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
                    model: None,
                    effort: Some("medium".to_string()),
                    permission_mode: Some(PermissionMode::AutoReview),
                    background: true,
                    worktree_root: Some("/repo-wt".to_string()),
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
