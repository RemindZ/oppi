use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ProviderMessageRole {
    System,
    User,
    Assistant,
}

impl ProviderMessageRole {
    pub(super) fn as_str(&self) -> &'static str {
        match self {
            Self::System => "system",
            Self::User => "user",
            Self::Assistant => "assistant",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ProviderHistoryMessage {
    pub(super) role: ProviderMessageRole,
    pub(super) content: String,
    pub(super) turn_id: Option<TurnId>,
}

#[derive(Debug, Clone)]
pub(super) struct DirectProviderResumeState {
    pub(super) config: DirectModelProviderConfig,
    pub(super) messages: Vec<Value>,
}

impl DirectProviderResumeState {
    pub(super) fn replay_snapshot(&self, turn_id: &str) -> ProviderTranscriptReplaySnapshot {
        ProviderTranscriptReplaySnapshot {
            turn_id: turn_id.to_string(),
            model_provider: self.config.clone(),
            messages: self.messages.clone(),
        }
    }

    pub(super) fn from_replay_snapshot(snapshot: &ProviderTranscriptReplaySnapshot) -> Self {
        Self {
            config: snapshot.model_provider.clone(),
            messages: snapshot.messages.clone(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct TurnInterruptRegistry {
    pub(super) requests: Arc<Mutex<BTreeMap<TurnId, String>>>,
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

    pub(super) fn take(&self, turn_id: &str) -> Option<String> {
        self.requests
            .lock()
            .ok()
            .and_then(|mut requests| requests.remove(turn_id))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct MemoryBackendRawResult {
    pub backend: String,
    pub adapter_kind: Option<String>,
    pub value: Value,
}

pub trait MemoryBackendRunner: std::fmt::Debug + Send + Sync {
    fn adapter_info(&self) -> MemoryBackendAdapterInfo;

    fn status(
        &self,
        thread: &Thread,
        params: &MemoryBackendStatusParams,
    ) -> Result<MemoryBackendRawResult, RuntimeError>;

    fn recall(
        &self,
        thread: &Thread,
        params: &MemoryBackendRecallParams,
    ) -> Result<MemoryBackendRawResult, RuntimeError>;

    fn remember(
        &self,
        thread: &Thread,
        params: &MemoryBackendRememberParams,
    ) -> Result<MemoryBackendRawResult, RuntimeError>;

    fn list(
        &self,
        thread: &Thread,
        params: &MemoryBackendListParams,
    ) -> Result<MemoryBackendRawResult, RuntimeError>;

    fn dashboard(
        &self,
        thread: &Thread,
        params: &MemoryBackendDashboardParams,
    ) -> Result<MemoryBackendRawResult, RuntimeError>;

    fn forget(
        &self,
        thread: &Thread,
        params: &MemoryBackendForgetParams,
    ) -> Result<MemoryBackendRawResult, RuntimeError>;

    fn pin(
        &self,
        thread: &Thread,
        params: &MemoryBackendPinParams,
    ) -> Result<MemoryBackendRawResult, RuntimeError>;

    fn update(
        &self,
        thread: &Thread,
        params: &MemoryBackendUpdateParams,
    ) -> Result<MemoryBackendRawResult, RuntimeError>;

    fn supersede(
        &self,
        thread: &Thread,
        params: &MemoryBackendSupersedeParams,
    ) -> Result<MemoryBackendRawResult, RuntimeError>;

    fn consolidate(
        &self,
        thread: &Thread,
        params: &MemoryBackendConsolidateParams,
    ) -> Result<MemoryBackendRawResult, RuntimeError>;

    fn sync(
        &self,
        thread: &Thread,
        params: &MemoryBackendSyncParams,
    ) -> Result<MemoryBackendRawResult, RuntimeError>;
}

#[derive(Debug, Clone)]
pub(super) struct ModelProviderStep {
    pub(super) step: ScriptedModelStep,
    pub(super) diagnostics: Vec<Diagnostic>,
    pub(super) known_token_delta: i64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct ProviderTokenUsage {
    pub(super) input_tokens: Option<i64>,
    pub(super) output_tokens: Option<i64>,
    pub(super) total_tokens: Option<i64>,
}

impl ProviderTokenUsage {
    fn total(self) -> Option<i64> {
        self.total_tokens
            .or_else(|| Some(self.input_tokens?.saturating_add(self.output_tokens?)))
    }
}

#[derive(Debug, Clone)]
pub(super) struct AgenticToolExecution {
    pub(super) result: ToolResult,
    pub(super) side_events: Vec<Event>,
}

impl AgenticToolExecution {
    pub(super) fn result(result: ToolResult) -> Self {
        Self {
            result,
            side_events: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum GuardianReviewDecisionKind {
    Allow,
    Deny,
}

#[derive(Debug, Clone)]
pub(super) struct GuardianReviewResult {
    pub(super) decision: GuardianReviewDecisionKind,
    pub(super) risk: RiskLevel,
    pub(super) reason: String,
    pub(super) strict_json: String,
}

#[derive(Debug)]
pub struct Runtime {
    pub(super) next_thread: u64,
    pub(super) next_turn: u64,
    pub(super) next_item: u64,
    pub(super) next_event: u64,
    pub(super) next_approval: u64,
    pub(super) next_question: u64,
    pub(super) next_agent_run: u64,
    pub(super) next_memory_job: u64,
    pub(super) threads: BTreeMap<ThreadId, Thread>,
    pub(super) turns: BTreeMap<TurnId, Turn>,
    pub(super) thread_item_labels: BTreeMap<(ThreadId, EventId), Vec<String>>,
    pub(super) thread_fork_points: BTreeMap<ThreadId, EventId>,
    pub(super) thread_history_cutoffs: BTreeMap<ThreadId, EventId>,
    pub(super) event_store: InMemoryEventStore,
    pub(super) event_mirror: Option<Arc<Mutex<Vec<Event>>>>,
    pub(super) approvals: BTreeMap<ApprovalId, OwnedApprovalRequest>,
    pub(super) questions: BTreeMap<QuestionId, OwnedQuestionRequest>,
    pub(super) plugins: BTreeMap<PluginId, PluginRef>,
    pub(super) memory: MemoryStatus,
    pub(super) memory_records: Option<Vec<MemoryRecord>>,
    pub(super) memory_jobs: BTreeMap<MemoryJobId, MemoryJobRef>,
    pub(super) memory_backend_runner: Option<Arc<dyn MemoryBackendRunner>>,
    pub(super) todos: TodoState,
    pub(super) goals: BTreeMap<ThreadId, ThreadGoal>,
    pub(super) goal_active_started_at_ms: BTreeMap<ThreadId, u64>,
    pub(super) goal_continuations: BTreeMap<ThreadId, u32>,
    pub(super) suggested_next: Option<SuggestedNextMessage>,
    pub(super) mcp_servers: BTreeMap<McpServerId, McpServerRef>,
    pub(super) mcp_http_sessions: BTreeMap<McpServerId, McpHttpSessionState>,
    #[cfg(test)]
    pub(super) mcp_mock_servers: BTreeMap<McpServerId, VecDeque<Value>>,
    pub(super) plugin_runner_tools: BTreeMap<PluginId, Vec<PluginRunnerToolRef>>,
    #[cfg(test)]
    pub(super) plugin_runner_mock_runners: BTreeMap<PluginId, VecDeque<Value>>,
    pub(super) models: BTreeMap<ModelId, ModelRef>,
    pub(super) selected_model: Option<ModelId>,
    pub(super) tool_registry: ToolRegistry,
    pub(super) agents: BTreeMap<String, Vec<AgentDefinition>>,
    pub(super) agent_runs: BTreeMap<AgentRunId, AgentRun>,
    pub(super) pending_background_agents: BTreeMap<AgentRunId, BackgroundAgentRequest>,
    pub(super) background_agent_resumes: BTreeMap<AgentRunId, BackgroundAgentResumeState>,
    pub(super) direct_provider_turns: BTreeMap<TurnId, DirectProviderResumeState>,
    pub(super) turn_sandbox_policies: BTreeMap<TurnId, SandboxPolicy>,
    pub(super) turn_interrupts: TurnInterruptRegistry,
    pub(super) shell_tasks: BTreeMap<String, ShellTaskRecord>,
    pub(super) guardian_review_records: VecDeque<GuardianReviewRecord>,
    pub(super) guardian_exact_cache: BTreeMap<String, GuardianReviewRecord>,
    pub(super) guardian_denial_circuit: BTreeMap<String, VecDeque<u64>>,
    pub(super) guardian_settings: GuardianReviewSettings,
    pub(super) ask_user_settings: AskUserSettings,
}

#[derive(Debug)]
pub(super) struct ShellTaskRecord {
    pub(super) id: String,
    pub(super) command: String,
    pub(super) cwd: String,
    pub(super) output_path: PathBuf,
    pub(super) status: ShellTaskStatus,
    pub(super) child: Option<SandboxedBackgroundProcess>,
    pub(super) started_at_ms: u64,
    pub(super) finished_at_ms: Option<u64>,
    pub(super) exit_code: Option<i32>,
}

#[derive(Debug, Clone)]
pub(super) struct BackgroundAgentResumeState {
    pub(super) parent_thread_id: ThreadId,
    pub(super) child_thread_id: ThreadId,
    pub(super) child_turn_id: TurnId,
    pub(super) execution_root: String,
    pub(super) tool_definitions: Vec<ToolDefinition>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ShellTaskStatus {
    Running,
    Completed,
    Failed,
    Killed,
}

impl ShellTaskStatus {
    pub(super) fn as_str(&self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Killed => "killed",
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct SkillCandidate {
    pub(super) reference: SkillRef,
    pub(super) content: String,
}

#[derive(Debug, Clone)]
pub(super) struct PromptTemplateCandidate {
    pub(super) reference: PromptTemplateRef,
    pub(super) body: String,
}

#[derive(Debug, Clone)]
pub(super) struct ThemeCandidate {
    pub(super) reference: ThemeRef,
    pub(super) colors: Value,
    pub(super) export: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub(super) struct StoredPluginStore {
    #[serde(default)]
    pub(super) plugins: Vec<StoredPluginEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct StoredPluginEntry {
    #[serde(default)]
    pub(super) name: Option<String>,
    #[serde(default)]
    pub(super) source: String,
    #[serde(default)]
    pub(super) source_type: Option<String>,
    #[serde(default)]
    pub(super) enabled: bool,
    #[serde(default)]
    pub(super) trusted: bool,
    #[serde(default)]
    pub(super) description: Option<String>,
    #[serde(default)]
    pub(super) version: Option<String>,
    #[serde(default)]
    pub(super) license: Option<String>,
    #[serde(default)]
    pub(super) capabilities: Vec<String>,
    #[serde(default)]
    pub(super) manifest: Option<Value>,
    #[serde(default)]
    pub(super) marketplace: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PluginMaterializationStore {
    pub(super) version: u32,
    #[serde(default)]
    pub(super) records: Vec<StoredPluginMaterializationRecord>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct StoredPluginMaterializationRecord {
    pub(super) plugin_id: String,
    pub(super) source: String,
    pub(super) source_type: String,
    pub(super) scope: PluginScope,
    pub(super) cache_path: String,
    pub(super) checksum: String,
    #[serde(default)]
    pub(super) manifest_name: Option<String>,
    #[serde(default)]
    pub(super) manifest_version: Option<String>,
    #[serde(default)]
    pub(super) provenance: Option<PluginProvenance>,
    pub(super) materialized_at: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PluginMcpTrustStore {
    pub(super) version: u32,
    #[serde(default)]
    pub(super) servers: Vec<StoredPluginMcpTrustRecord>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct StoredPluginMcpTrustRecord {
    pub(super) plugin_id: String,
    pub(super) server_id: String,
    #[serde(default)]
    pub(super) enabled: bool,
    #[serde(default)]
    pub(super) trusted: bool,
    pub(super) updated_at: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PluginRunnerStore {
    pub(super) version: u32,
    #[serde(default)]
    pub(super) runners: Vec<StoredPluginRunnerRecord>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct StoredPluginRunnerRecord {
    pub(super) plugin_id: String,
    #[serde(default)]
    pub(super) enabled: bool,
    #[serde(default)]
    pub(super) trusted: bool,
    pub(super) updated_at: String,
}

#[derive(Debug, Clone)]
pub(super) struct ConfiguredPluginMcpRoot {
    pub(super) plugin_id: String,
    pub(super) root: PathBuf,
}

#[derive(Debug, Clone)]
pub(super) struct ConfiguredPluginRunner {
    pub(super) plugin_id: String,
    pub(super) root: PathBuf,
    pub(super) command: String,
    pub(super) args: Vec<String>,
    pub(super) capabilities: Vec<String>,
}

#[derive(Debug, Clone)]
pub(super) struct PluginRunnerToolRef {
    pub(super) plugin_id: String,
    pub(super) name: String,
    pub(super) description: Option<String>,
    pub(super) input_schema: Value,
    pub(super) read_only: bool,
}

#[derive(Debug, Clone)]
pub(super) struct ResolvedAgentToolPolicy {
    pub(super) background: bool,
    pub(super) role: Option<String>,
    pub(super) model: Option<String>,
    pub(super) effort: Option<String>,
    pub(super) permission_mode: Option<PermissionMode>,
    pub(super) network_policy: Option<NetworkPolicy>,
    pub(super) memory_mode: Option<String>,
    pub(super) worktree_root: Option<String>,
    pub(super) tool_allowlist: Vec<String>,
    pub(super) tool_denylist: Vec<String>,
    pub(super) isolation: Option<String>,
    pub(super) color: Option<String>,
    pub(super) skills: Vec<String>,
    pub(super) max_turns: Option<u32>,
}

fn background_task_status(status: ShellTaskStatus) -> BackgroundTaskStatus {
    match status {
        ShellTaskStatus::Running => BackgroundTaskStatus::Running,
        ShellTaskStatus::Completed => BackgroundTaskStatus::Completed,
        ShellTaskStatus::Failed => BackgroundTaskStatus::Failed,
        ShellTaskStatus::Killed => BackgroundTaskStatus::Killed,
    }
}

pub(super) fn background_task_info(task: &ShellTaskRecord) -> BackgroundTaskInfo {
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

pub(super) fn shell_task_line(task: &ShellTaskRecord) -> String {
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

pub(super) fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

pub(super) fn ask_user_timeout_ms_is_allowed(timeout_ms: u64) -> bool {
    ASK_USER_ALLOWED_TIMEOUT_MS.contains(&timeout_ms)
}

#[derive(Debug, Clone)]
pub(super) struct OwnedApprovalRequest {
    pub(super) thread_id: ThreadId,
    pub(super) turn_id: Option<TurnId>,
    pub(super) tool_call_id: Option<ToolCallId>,
    pub(super) tool_call: Option<ToolCall>,
    pub(super) outcome: Option<ApprovalOutcome>,
}

#[derive(Debug, Clone)]
pub(super) struct OwnedQuestionRequest {
    pub(super) thread_id: ThreadId,
    pub(super) turn_id: Option<TurnId>,
    pub(super) tool_call_id: Option<ToolCallId>,
    pub(super) tool_call: Option<ToolCall>,
    pub(super) resolved: bool,
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
            next_memory_job: 0,
            threads: BTreeMap::new(),
            turns: BTreeMap::new(),
            thread_item_labels: BTreeMap::new(),
            thread_fork_points: BTreeMap::new(),
            thread_history_cutoffs: BTreeMap::new(),
            event_store: InMemoryEventStore::new(),
            event_mirror: None,
            approvals: BTreeMap::new(),
            questions: BTreeMap::new(),
            plugins: BTreeMap::new(),
            memory: MemoryStatus::default(),
            memory_records: None,
            memory_jobs: BTreeMap::new(),
            memory_backend_runner: None,
            todos: TodoState::default(),
            goals: BTreeMap::new(),
            goal_active_started_at_ms: BTreeMap::new(),
            goal_continuations: BTreeMap::new(),
            suggested_next: None,
            mcp_servers: BTreeMap::new(),
            mcp_http_sessions: BTreeMap::new(),
            #[cfg(test)]
            mcp_mock_servers: BTreeMap::new(),
            plugin_runner_tools: BTreeMap::new(),
            #[cfg(test)]
            plugin_runner_mock_runners: BTreeMap::new(),
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
            pending_background_agents: BTreeMap::new(),
            background_agent_resumes: BTreeMap::new(),
            direct_provider_turns: BTreeMap::new(),
            turn_sandbox_policies: BTreeMap::new(),
            turn_interrupts: TurnInterruptRegistry::default(),
            shell_tasks: BTreeMap::new(),
            guardian_review_records: VecDeque::new(),
            guardian_exact_cache: BTreeMap::new(),
            guardian_denial_circuit: BTreeMap::new(),
            guardian_settings: GuardianReviewSettings {
                timeout_ms: DEFAULT_GUARDIAN_REVIEW_TIMEOUT_MS,
                reviewer_model: None,
            },
            ask_user_settings: AskUserSettings {
                timeout_ms: DEFAULT_ASK_USER_TIMEOUT_MS,
            },
        }
    }
}

pub(super) fn tool_registry_from_definitions(definitions: Vec<ToolDefinition>) -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    for definition in definitions {
        registry.register(definition);
    }
    registry
}

pub(super) fn model_visible_tool_definitions(registry: &ToolRegistry) -> Vec<ToolDefinition> {
    registry
        .list()
        .into_iter()
        .filter(|definition| {
            !definition
                .capabilities
                .iter()
                .any(|capability| capability == "review-tool")
        })
        .collect()
}

pub(super) fn reviewer_tool_definitions(registry: &ToolRegistry) -> Vec<ToolDefinition> {
    registry
        .list()
        .into_iter()
        .filter(|definition| {
            definition
                .capabilities
                .iter()
                .any(|capability| capability == "review-tool")
        })
        .collect()
}

pub(super) fn default_tool_registry() -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    registry.register(ToolDefinition {
        name: "echo".to_string(),
        namespace: Some("oppi".to_string()),
        description: Some("Return the provided output argument as a tool result.".to_string()),
        concurrency_safe: true,
        requires_approval: false,
        capabilities: Vec::new(),
        ..Default::default()
    });
    registry.register(ToolDefinition {
        name: "get_goal".to_string(),
        namespace: None,
        description: Some("Get the current goal for this thread, including status, budget, tokens used, elapsed time, and remaining token budget.".to_string()),
        concurrency_safe: true,
        requires_approval: false,
        capabilities: vec!["goal".to_string(), "state".to_string(), "read".to_string()],
        ..Default::default()
    });
    registry.register(ToolDefinition {
        name: "create_goal".to_string(),
        namespace: None,
        description: Some("Create a goal only when explicitly requested by the user or system/developer instructions. Fails if a non-complete goal already exists.".to_string()),
        concurrency_safe: false,
        requires_approval: false,
        capabilities: vec!["goal".to_string(), "state".to_string(), "write".to_string()],
        ..Default::default()
    });
    registry.register(ToolDefinition {
        name: "update_goal".to_string(),
        namespace: None,
        description: Some("Update the existing goal only to mark it complete after the objective is actually achieved; pause/resume/budget are user/runtime controls.".to_string()),
        concurrency_safe: false,
        requires_approval: false,
        capabilities: vec!["goal".to_string(), "state".to_string(), "write".to_string()],
        ..Default::default()
    });
    registry.register(ToolDefinition {
        name: "todo_write".to_string(),
        namespace: Some("oppi".to_string()),
        description: Some("Create or update the current task todo list. Arguments: todos is the full current list of {id, content, status, priority?, phase?, notes?}; summary briefly explains what changed. Status values: pending, in_progress, completed, blocked, cancelled. Always send the full list, not a patch.".to_string()),
        concurrency_safe: false,
        requires_approval: false,
        capabilities: vec!["state".to_string(), "todos".to_string()],
        ..Default::default()
    });
    registry.register(ToolDefinition {
        name: "ask_user".to_string(),
        namespace: Some("oppi".to_string()),
        description: Some("Ask the user one or more structured questions and pause the turn until the host supplies answers.".to_string()),
        concurrency_safe: false,
        requires_approval: false,
        capabilities: vec!["questions".to_string(), "user-input".to_string()],
        ..Default::default()
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
        ..Default::default()
    });
    registry.register(ToolDefinition {
        name: "oppi_feedback_submit".to_string(),
        namespace: Some("oppi".to_string()),
        description: Some("Create a sanitized OPPi bug report or feature request draft, with optional intake-worker submission only when explicitly configured at runtime.".to_string()),
        concurrency_safe: false,
        requires_approval: false,
        capabilities: vec!["feedback".to_string(), "diagnostics".to_string()],
        ..Default::default()
    });
    registry.register(ToolDefinition {
        name: "render_mermaid".to_string(),
        namespace: Some("oppi".to_string()),
        description: Some("Render Mermaid diagram source as terminal-friendly ASCII using Rust's deterministic fallback renderer.".to_string()),
        concurrency_safe: true,
        requires_approval: false,
        capabilities: vec!["mermaid".to_string(), "render".to_string()],
        ..Default::default()
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
        ..Default::default()
    });
    registry.register(ToolDefinition {
        name: "AgentTool".to_string(),
        namespace: Some("oppi".to_string()),
        description: Some("Delegate a scoped task to a registered native OPPi subagent with Rust-enforced tool, permission, model/effort, memory, isolation, skill, and max-turn policy.".to_string()),
        concurrency_safe: false,
        requires_approval: false,
        capabilities: vec!["agent".to_string(), "subagent".to_string()],
        ..Default::default()
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
        ..Default::default()
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
        ..Default::default()
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
        ..Default::default()
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
        ..Default::default()
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
        ..Default::default()
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
        ..Default::default()
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
        ..Default::default()
    });
    registry.register(ToolDefinition {
        name: "write_file".to_string(),
        namespace: Some("oppi".to_string()),
        description: Some("Write a UTF-8 text file inside the project after approval and Rust preflight policy checks.".to_string()),
        concurrency_safe: false,
        requires_approval: true,
        capabilities: vec!["filesystem".to_string(), "write".to_string()],
        ..Default::default()
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
        ..Default::default()
    });
    registry
}

pub(super) fn server_capabilities() -> Vec<String> {
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

pub(super) const INIT_USER_PROMPT: &str =
    include_str!("../../../../systemprompts/commands/init-user-prompt.md");
pub(super) const BUILTIN_IMAGEGEN_SKILL: &str =
    include_str!("../../../../packages/pi-package/skills/imagegen/SKILL.md");
pub(super) const BUILTIN_INDEPENDENT_SKILL: &str =
    include_str!("../../../../packages/pi-package/skills/independent/SKILL.md");
pub(super) const BUILTIN_MERMAID_SKILL: &str =
    include_str!("../../../../packages/pi-package/skills/mermaid-diagrams/SKILL.md");
pub(super) const BUILTIN_GRAPHIFY_SKILL: &str =
    include_str!("../../../../packages/pi-package/skills/graphify/SKILL.md");
pub(super) const MAX_EXISTING_AGENTS_CHARS: usize = 32_000;
pub(super) const MAX_SKILL_CONTENT_CHARS: usize = 8_000;
pub(super) const MAX_SKILL_INJECTION_CHARS: usize = 16_000;
pub(super) const MAX_MCP_SUBSCRIBED_RESOURCES: usize = 5;
pub(super) const MAX_MCP_SUBSCRIBED_RESOURCE_CHARS: usize = 2_000;
pub(super) const MAX_MCP_SUBSCRIBED_CONTEXT_CHARS: usize = 8_000;
pub(super) const MAX_PROMPT_TEMPLATE_CHARS: usize = 32_000;
pub(super) const GUARDIAN_REVIEWER_SYSTEM_PROMPT: &str = "You are OPPi Guardian, an isolated permission reviewer. Decide whether the proposed tool call should be allowed or denied. Return exactly one JSON object. Do not include markdown fences or prose.";
pub(super) const GUARDIAN_REVIEWER_OUTPUT_CONTRACT: &str = r#"Return exactly this JSON shape with no markdown:
{
  "outcome": "allow" | "deny",
  "risk_level": "low" | "medium" | "high" | "critical",
  "user_authorization": "unknown" | "low" | "medium" | "high",
  "rationale": "one concise sentence grounded in the conversation and tool call",
  "cache_scope": "none" | "exact"
}"#;

pub(super) fn protocol_version_is_compatible(client_version: &str) -> bool {
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
pub(super) struct ModelRequest {
    pub(super) thread_id: ThreadId,
    pub(super) turn_id: TurnId,
    pub(super) input: String,
    pub(super) continuation: u32,
    pub(super) history: Vec<ProviderHistoryMessage>,
}

pub(super) trait ModelProvider {
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
    pub(super) steps: VecDeque<ScriptedModelStep>,
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

pub(super) struct OpenAiCompatibleModelProvider {
    pub(super) config: DirectModelProviderConfig,
    pub(super) agent: ureq::Agent,
    pub(super) messages: Vec<Value>,
    pub(super) tools: BTreeMap<String, ToolDefinition>,
}

impl OpenAiCompatibleModelProvider {
    pub(super) fn new(config: DirectModelProviderConfig, tools: Vec<ToolDefinition>) -> Self {
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

    pub(super) fn api_key(&self) -> Result<String, RuntimeError> {
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
        let (step, message, chunk_count, known_token_usage) = match response {
            Ok(response) => {
                status = response.status();
                if stream {
                    let reader = BufReader::new(response.into_reader());
                    let (step, message, chunk_count, known_token_usage) =
                        parse_openai_compatible_stream(reader, &self.tools)?;
                    (step, message, chunk_count, known_token_usage)
                } else {
                    let value = response.into_json::<Value>().map_err(|error| {
                        RuntimeError::new(
                            "provider_response_decode_failed",
                            RuntimeErrorCategory::Provider,
                            format!("direct provider returned invalid JSON: {error}"),
                        )
                    })?;
                    let known_token_usage = provider_usage(&value);
                    let message = openai_response_message(&value)?.clone();
                    let step = parse_openai_compatible_message(&message, &self.tools)?;
                    let chunk_count = step.assistant_deltas.len();
                    (step, message, chunk_count, known_token_usage)
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
            known_token_usage,
        )];
        let known_token_delta = known_token_usage
            .and_then(ProviderTokenUsage::total)
            .unwrap_or(0);
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
pub(super) struct GitHubCopilotAuth {
    pub(super) access_token: String,
    pub(super) refresh_token: String,
    pub(super) expires: i64,
    pub(super) enterprise_domain: Option<String>,
}

pub(super) fn github_copilot_auth_path() -> PathBuf {
    std::env::var_os("OPPI_GITHUB_COPILOT_AUTH_PATH")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| default_oppi_agent_dir().join("auth.json"))
}

pub(super) fn parse_github_copilot_auth(raw: &Value) -> Option<GitHubCopilotAuth> {
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

pub(super) fn read_github_copilot_auth_json(path: &Path) -> Result<Value, RuntimeError> {
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

pub(super) fn github_copilot_base_url(auth: &GitHubCopilotAuth) -> String {
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

pub(super) fn github_copilot_initiator(messages: &[Value]) -> &'static str {
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
pub(super) struct CodexAuth {
    pub(super) access_token: String,
    pub(super) refresh_token: String,
    pub(super) expires: i64,
    pub(super) account_id: String,
}

pub(super) struct OpenAiCodexModelProvider {
    pub(super) config: DirectModelProviderConfig,
    pub(super) agent: ureq::Agent,
    pub(super) messages: Vec<Value>,
    pub(super) tools: BTreeMap<String, ToolDefinition>,
}

impl OpenAiCodexModelProvider {
    pub(super) fn new(config: DirectModelProviderConfig, tools: Vec<ToolDefinition>) -> Self {
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

    pub(super) fn request_body(&self, request: &ModelRequest) -> Value {
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
        let (step, output_items, chunk_count, known_token_usage) = match response {
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
            known_token_usage,
        )];
        let known_token_delta = known_token_usage
            .and_then(ProviderTokenUsage::total)
            .unwrap_or(0);
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

pub(super) fn default_oppi_agent_dir() -> PathBuf {
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

pub(super) fn codex_auth_path() -> PathBuf {
    std::env::var_os("OPPI_OPENAI_CODEX_AUTH_PATH")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| default_oppi_agent_dir().join("auth.json"))
}

pub(super) fn codex_auth_lock_path(path: &Path) -> PathBuf {
    let mut lock_path = path.as_os_str().to_os_string();
    lock_path.push(".lock");
    PathBuf::from(lock_path)
}

#[derive(Debug)]
struct CodexAuthLock {
    pub(super) path: PathBuf,
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

pub(super) fn now_millis_i64() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}

pub(super) fn parse_codex_auth(raw: &Value) -> Option<CodexAuth> {
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

pub(super) fn read_codex_auth_json(path: &Path) -> Result<Value, RuntimeError> {
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

pub(super) fn codex_account_id_from_access_token(
    access_token: &str,
) -> Result<String, RuntimeError> {
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

pub(super) fn read_or_refresh_codex_auth(agent: &ureq::Agent) -> Result<CodexAuth, RuntimeError> {
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

pub(super) fn codex_user_agent() -> String {
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

pub(super) fn codex_tool_definition(name: &str, definition: &ToolDefinition) -> Value {
    json!({
        "type": "function",
        "name": name,
        "description": definition.description.clone().unwrap_or_else(|| format!("OPPi tool {}", definition.name)),
        "parameters": provider_tool_parameters(definition),
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
    pub(super) item_id: Option<String>,
    pub(super) call_id: Option<String>,
    pub(super) name: Option<String>,
    pub(super) arguments: String,
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

pub(super) fn parse_openai_codex_stream<R: BufRead>(
    mut reader: R,
    tools: &BTreeMap<String, ToolDefinition>,
) -> Result<
    (
        ScriptedModelStep,
        Vec<Value>,
        usize,
        Option<ProviderTokenUsage>,
    ),
    RuntimeError,
> {
    let mut content_chunks = Vec::new();
    let mut final_items = Vec::new();
    let mut raw_tool_calls = Vec::new();
    let mut partial_calls: BTreeMap<String, PartialCodexFunctionCall> = BTreeMap::new();
    let mut seen_tool_ids = BTreeSet::new();
    let mut chunk_count = 0usize;
    let mut known_token_usage = None;
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
                            if let Some(usage) = provider_usage(&event) {
                                known_token_usage = Some(usage);
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
    Ok((step, final_items, chunk_count, known_token_usage))
}

pub(super) fn model_provider(
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

pub(super) fn provider_api_key_env_allowed(name: &str) -> bool {
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
    pub(super) id: Option<String>,
    pub(super) call_type: Option<String>,
    pub(super) function_name: Option<String>,
    pub(super) arguments: String,
}

pub(super) fn parse_openai_compatible_stream<R: BufRead>(
    reader: R,
    tools: &BTreeMap<String, ToolDefinition>,
) -> Result<(ScriptedModelStep, Value, usize, Option<ProviderTokenUsage>), RuntimeError> {
    let mut content_chunks = Vec::new();
    let mut partial_calls: BTreeMap<u64, PartialOpenAiToolCall> = BTreeMap::new();
    let mut chunk_count = 0usize;
    let mut known_token_usage = None;

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
        if let Some(usage) = provider_usage(&value) {
            known_token_usage = Some(usage);
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
    Ok((step, message, chunk_count, known_token_usage))
}

fn provider_usage(value: &Value) -> Option<ProviderTokenUsage> {
    let usage = value.get("usage").or_else(|| {
        value
            .get("response")
            .and_then(|response| response.get("usage"))
    })?;
    let input_tokens = usage
        .get("input_tokens")
        .or_else(|| usage.get("prompt_tokens"))
        .or_else(|| usage.get("inputTokens"))
        .and_then(Value::as_i64);
    let output_tokens = usage
        .get("output_tokens")
        .or_else(|| usage.get("completion_tokens"))
        .or_else(|| usage.get("outputTokens"))
        .and_then(Value::as_i64);
    let total_tokens = usage
        .get("total_tokens")
        .or_else(|| usage.get("totalTokens"))
        .and_then(Value::as_i64)
        .or_else(|| Some(input_tokens?.saturating_add(output_tokens?)));
    (input_tokens.is_some() || output_tokens.is_some() || total_tokens.is_some()).then_some(
        ProviderTokenUsage {
            input_tokens,
            output_tokens,
            total_tokens,
        },
    )
}

pub(super) fn provider_diagnostic(
    config: &DirectModelProviderConfig,
    endpoint: &str,
    status: u16,
    elapsed: Duration,
    stream: bool,
    chunk_count: usize,
    tool_call_count: usize,
    history_message_count: usize,
    usage: Option<ProviderTokenUsage>,
) -> Diagnostic {
    let provider_label = match config.kind {
        DirectModelProviderKind::OpenAiCompatible => "openai-compatible",
        DirectModelProviderKind::OpenAiCodex => "openai-codex",
        DirectModelProviderKind::GitHubCopilot => "github-copilot",
    };
    let mut metadata = BTreeMap::from([
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
    ]);
    if let Some(usage) = usage {
        if let Some(input) = usage.input_tokens {
            metadata.insert("inputTokens".to_string(), input.to_string());
        }
        if let Some(output) = usage.output_tokens {
            metadata.insert("outputTokens".to_string(), output.to_string());
        }
        if let Some(total) = usage.total() {
            metadata.insert("totalTokens".to_string(), total.to_string());
        }
    }
    Diagnostic {
        level: DiagnosticLevel::Info,
        message: "direct provider request completed".to_string(),
        metadata,
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

pub(super) fn openai_response_message(value: &Value) -> Result<&Value, RuntimeError> {
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
pub(super) fn parse_openai_compatible_chat_response(
    value: Value,
) -> Result<ScriptedModelStep, RuntimeError> {
    parse_openai_compatible_message(openai_response_message(&value)?, &BTreeMap::new())
}

pub(super) fn parse_openai_compatible_message(
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

pub(super) fn tool_call_from_provider_snapshot(
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

pub(super) fn openai_tool_name(definition: &ToolDefinition) -> String {
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

pub(super) fn openai_tool_definition(name: &str, definition: &ToolDefinition) -> Value {
    json!({
        "type": "function",
        "function": {
            "name": name,
            "description": definition.description.clone().unwrap_or_else(|| format!("OPPi tool {}", definition.name)),
            "parameters": provider_tool_parameters(definition),
        },
    })
}

fn provider_tool_parameters(definition: &ToolDefinition) -> Value {
    definition.input_schema.clone().unwrap_or_else(|| {
        json!({
            "type": "object",
            "properties": {},
            "additionalProperties": true,
        })
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

pub(super) fn compact_provider_error(detail: &str) -> String {
    let compact = detail.split_whitespace().collect::<Vec<_>>().join(" ");
    truncate_chars(&compact, 300)
}

pub(super) fn redacted_endpoint(endpoint: &str) -> String {
    let (scheme, rest) = endpoint
        .split_once("://")
        .map(|(scheme, rest)| (format!("{scheme}://"), rest))
        .unwrap_or_else(|| (String::new(), endpoint));
    let without_auth = rest.rsplit('@').next().unwrap_or(rest);
    format!("{scheme}{without_auth}")
}

pub(super) fn compact_history_text(text: &str) -> String {
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

pub(super) fn append_system_prompt(base: Option<String>, heading: &str, append: &str) -> String {
    [
        base.unwrap_or_default().trim().to_string(),
        format!("{heading}:\n{append}"),
    ]
    .into_iter()
    .filter(|part| !part.trim().is_empty())
    .collect::<Vec<_>>()
    .join("\n\n")
}

pub(super) fn append_follow_up_to_model_provider(
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

pub(super) fn follow_up_pending_count(context: &FollowUpChainContext) -> usize {
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

fn todo_priority_label(priority: TodoPriority) -> &'static str {
    match priority {
        TodoPriority::Low => "low",
        TodoPriority::Medium => "medium",
        TodoPriority::High => "high",
    }
}

fn todo_metadata_suffix(
    id: &str,
    priority: Option<TodoPriority>,
    phase: Option<&str>,
    notes: Option<&str>,
) -> String {
    let mut parts = vec![format!("id: {id}")];
    if let Some(priority) = priority {
        parts.push(format!("priority: {}", todo_priority_label(priority)));
    }
    if let Some(phase) = phase.map(str::trim).filter(|phase| !phase.is_empty()) {
        parts.push(format!("phase: {phase}"));
    }
    if let Some(notes) = notes.map(str::trim).filter(|notes| !notes.is_empty()) {
        parts.push(format!("notes: {notes}"));
    }
    format!(" ({})", parts.join(", "))
}

pub(super) fn compaction_details_from_todos(todos: &TodoState) -> Option<CompactionHandoffDetails> {
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

pub(super) fn deterministic_compaction_summary(
    details: Option<&CompactionHandoffDetails>,
) -> String {
    let Some(details) = details else {
        return [
            "OPPi manual handoff.",
            "",
            "No current todo state was available. Continue from the latest visible transcript and any prior compacted summary.",
        ]
        .join("\n");
    };

    let mut lines = vec![
        "OPPi todo-aware handoff.".to_string(),
        String::new(),
        "Remaining todos:".to_string(),
    ];
    if details.remaining_todos.is_empty() {
        lines.push("- No remaining todos. Preserve final-response context only.".to_string());
    } else {
        for todo in &details.remaining_todos {
            lines.push(format!(
                "- [{}] {}{}",
                todo.status.as_str(),
                todo.content,
                todo_metadata_suffix(
                    &todo.id,
                    todo.priority,
                    todo.phase.as_deref(),
                    todo.notes.as_deref(),
                )
            ));
        }
    }

    lines.push(String::new());
    lines.push("Completed/cancelled outcomes:".to_string());
    if details.completed_outcomes.is_empty() {
        lines.push("- No completed or cancelled outcomes were archived.".to_string());
    } else {
        for outcome in &details.completed_outcomes {
            lines.push(format!(
                "- [{}] {}{}: {}",
                outcome.status.as_str(),
                outcome.content,
                todo_metadata_suffix(
                    &outcome.id,
                    None,
                    outcome.phase.as_deref(),
                    outcome.notes.as_deref(),
                ),
                outcome.outcome
            ));
        }
    }

    lines.push(String::new());
    lines.push("Final response note: combine archived completed/cancelled outcomes with any post-compaction work or validation before replying to the user.".to_string());
    lines.join("\n")
}
