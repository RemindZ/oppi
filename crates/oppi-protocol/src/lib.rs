//! OPPi runtime protocol types.
//!
//! These types are the Rust-owned schema for the Stage 5 runtime spine. They are
//! intentionally UI-independent and JSON-friendly so the CLI, future TUI, and VS
//! Code clients can consume the same semantic event stream.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const OPPI_PROTOCOL_VERSION: &str = "0.1.0";
pub const OPPI_MIN_PROTOCOL_VERSION: &str = "0.1.0";

pub type ProjectId = String;
pub type ThreadId = String;
pub type TurnId = String;
pub type ItemId = String;
pub type EventId = u64;
pub type ToolCallId = String;
pub type ApprovalId = String;
pub type QuestionId = String;
pub type AgentRunId = String;
pub type PluginId = String;
pub type McpServerId = String;
pub type ModelId = String;
pub type SkillId = String;
pub type FollowUpChainId = String;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRef {
    pub id: ProjectId,
    pub cwd: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub workspace_roots: Vec<WorkspaceRoot>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceRoot {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_remote: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Thread {
    pub id: ThreadId,
    pub project: ProjectRef,
    pub status: ThreadStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forked_from: Option<ThreadId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ThreadStatus {
    Active,
    Archived,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadGoal {
    pub thread_id: ThreadId,
    pub objective: String,
    pub status: ThreadGoalStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_budget: Option<i64>,
    pub tokens_used: i64,
    pub time_used_seconds: i64,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ThreadGoalStatus {
    Active,
    Paused,
    BudgetLimited,
    Complete,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Turn {
    pub id: TurnId,
    pub thread_id: ThreadId,
    pub status: TurnStatus,
    pub phase: TurnPhase,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_turn_id: Option<TurnId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TurnStatus {
    Queued,
    Running,
    WaitingForApproval,
    WaitingForUser,
    Completed,
    Aborted,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TurnPhase {
    Input,
    Message,
    History,
    System,
    Api,
    Tokens,
    Tools,
    Loop,
    Render,
    Hooks,
    Await,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub id: ItemId,
    pub thread_id: ThreadId,
    pub turn_id: TurnId,
    pub kind: ItemKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ItemKind {
    UserMessage { text: String },
    AssistantMessage { text: String },
    Reasoning { text: String },
    ToolCall(ToolCall),
    ToolResult(ToolResult),
    ApprovalRequest(ApprovalRequest),
    ApprovalDecision(ApprovalDecision),
    QuestionRequest(QuestionRequest),
    QuestionResponse(QuestionResponse),
    Diagnostic(Diagnostic),
    HandoffSummary { summary: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub id: EventId,
    pub thread_id: ThreadId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<TurnId>,
    pub kind: EventKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum EventKind {
    ThreadStarted {
        thread: Thread,
    },
    ThreadResumed {
        thread: Thread,
    },
    ThreadForked {
        thread: Thread,
        from_thread_id: ThreadId,
    },
    ThreadUpdated {
        thread: Thread,
    },
    ThreadGoalUpdated {
        goal: ThreadGoal,
    },
    ThreadGoalCleared {
        thread_id: ThreadId,
    },
    TurnStarted {
        turn: Turn,
    },
    TurnPhaseChanged {
        phase: TurnPhase,
    },
    TurnCompleted {
        turn_id: TurnId,
    },
    TurnInterrupted {
        reason: String,
    },
    TurnAborted {
        reason: String,
    },
    ItemStarted {
        item: Item,
    },
    ItemDelta {
        item_id: ItemId,
        delta: String,
    },
    ItemCompleted {
        item: Item,
    },
    ToolCallStarted {
        call: ToolCall,
    },
    ToolCallCompleted {
        result: ToolResult,
    },
    ArtifactCreated {
        artifact: ArtifactMetadata,
    },
    TodosUpdated {
        state: TodoState,
    },
    ToolBatchStarted {
        batch: ToolExecutionBatch,
    },
    ToolBatchCompleted {
        batch_id: String,
        status: ToolBatchStatus,
    },
    ApprovalRequested {
        request: ApprovalRequest,
    },
    ApprovalResolved {
        decision: ApprovalDecision,
    },
    QuestionRequested {
        request: QuestionRequest,
    },
    QuestionResolved {
        response: QuestionResponse,
    },
    AskUserRequested {
        request: AskUserRequest,
    },
    AskUserResolved {
        response: AskUserResponse,
    },
    ProviderTranscriptSnapshot {
        snapshot: ProviderTranscriptReplaySnapshot,
    },
    SuggestionOffered {
        suggestion: SuggestedNextMessage,
    },
    AgentStarted {
        run: AgentRun,
    },
    AgentBlocked {
        run_id: AgentRunId,
        reason: String,
    },
    AgentCompleted {
        run_id: AgentRunId,
        output: String,
    },
    SandboxStatus {
        status: SandboxStatus,
    },
    HandoffCompacted {
        summary: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        details: Option<CompactionHandoffDetails>,
    },
    PluginRegistered {
        plugin: PluginRef,
    },
    MemoryStatusChanged {
        status: MemoryStatus,
    },
    McpServerRegistered {
        server: McpServerRef,
    },
    ModelRegistered {
        model: ModelRef,
    },
    ModelSelected {
        model_id: ModelId,
    },
    PiAdapterEvent {
        name: String,
        payload: serde_json::Value,
    },
    SideQuestionAnswered {
        question: String,
        answer: String,
    },
    Diagnostic {
        diagnostic: Diagnostic,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCall {
    pub id: ToolCallId,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    #[serde(default)]
    pub arguments: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolResult {
    pub call_id: ToolCallId,
    pub status: ToolResultStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactMetadata {
    pub id: String,
    pub tool_call_id: ToolCallId,
    pub output_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_images: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mask: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolDefinition {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub concurrency_safe: bool,
    #[serde(default)]
    pub requires_approval: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ToolResultStatus {
    Ok,
    Denied,
    Error,
    Aborted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TodoStatus {
    Pending,
    InProgress,
    Completed,
    Blocked,
    Cancelled,
}

impl TodoStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::InProgress => "in_progress",
            Self::Completed => "completed",
            Self::Blocked => "blocked",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TodoPriority {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TodoItem {
    pub id: String,
    pub content: String,
    pub status: TodoStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<TodoPriority>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TodoState {
    #[serde(default)]
    pub todos: Vec<TodoItem>,
    #[serde(default)]
    pub summary: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FollowUpStatus {
    Queued,
    Running,
    Completed,
}

impl FollowUpStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Completed => "completed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FollowUpItemContext {
    pub id: String,
    pub text: String,
    pub status: FollowUpStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FollowUpChainContext {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chain_id: Option<FollowUpChainId>,
    pub root_prompt: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub follow_ups: Vec<FollowUpItemContext>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_follow_up_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_variant_append: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TodoListResult {
    pub state: TodoState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TodoClientAction {
    Clear,
    Done,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TodoClientActionParams {
    pub thread_id: ThreadId,
    pub action: TodoClientAction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TodoClientActionResult {
    pub state: TodoState,
    pub events: Vec<Event>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ToolBatchExecution {
    Concurrent,
    Exclusive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ToolBatchStatus {
    Completed,
    Failed,
    Aborted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolExecutionBatch {
    pub id: String,
    pub execution: ToolBatchExecution,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_call_ids: Vec<ToolCallId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub concurrency_limit: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SimulatedToolUse {
    pub call: ToolCall,
    pub result: ToolResult,
    #[serde(default)]
    pub require_approval: bool,
    #[serde(default)]
    pub concurrency_safe: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalRequest {
    pub id: ApprovalId,
    pub reason: String,
    pub risk: RiskLevel,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call: Option<ToolCall>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalDecision {
    pub request_id: ApprovalId,
    pub decision: ApprovalOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ApprovalOutcome {
    Approved,
    Denied,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestionRequest {
    pub id: QuestionId,
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestionResponse {
    pub request_id: QuestionId,
    pub answer: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AskUserOption {
    pub id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AskUserQuestion {
    pub id: String,
    pub question: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<AskUserOption>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_custom: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_option_id: Option<String>,
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AskUserRequest {
    pub id: QuestionId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub questions: Vec<AskUserQuestion>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<ToolCallId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AskUserAnswer {
    pub question_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub option_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub skipped: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AskUserResponse {
    pub request_id: QuestionId,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub answers: Vec<AskUserAnswer>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub cancelled: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub timed_out: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SuggestedNextMessage {
    pub message: String,
    pub confidence: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RiskLevel {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionProfile {
    pub mode: PermissionMode,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub readable_roots: Vec<String>,
    #[serde(default)]
    pub writable_roots: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub filesystem_rules: Vec<FilesystemRule>,
    #[serde(default)]
    pub protected_patterns: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PermissionMode {
    ReadOnly,
    Default,
    AutoReview,
    FullAccess,
}

impl PermissionMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ReadOnly => "read-only",
            Self::Default => "default",
            Self::AutoReview => "auto-review",
            Self::FullAccess => "full-access",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxPolicy {
    pub permission_profile: PermissionProfile,
    pub network: NetworkPolicy,
    pub filesystem: FilesystemPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdditionalPermissionProfile {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub readable_roots: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub writable_roots: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub filesystem_rules: Vec<FilesystemRule>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network: Option<NetworkPolicy>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub protected_patterns: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConcreteSandboxPolicy {
    pub sandbox_preference: SandboxPreference,
    pub sandbox_type: SandboxType,
    pub filesystem: FilesystemPolicy,
    pub network: NetworkPolicy,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub writable_roots: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub protected_patterns: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub filesystem_rules: Vec<FilesystemRule>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxExecRequest {
    pub command: String,
    pub cwd: String,
    #[serde(default)]
    pub writes_files: bool,
    #[serde(default)]
    pub uses_network: bool,
    #[serde(default)]
    pub touches_protected_path: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub touched_paths: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxExecPlan {
    pub command: String,
    pub cwd: String,
    pub sandbox_type: SandboxType,
    pub enforcement: SandboxEnforcement,
    pub filesystem: FilesystemPolicy,
    pub network: NetworkPolicy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub managed_network: Option<ManagedNetworkConfig>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub readable_roots: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub writable_roots: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub filesystem_rules: Vec<FilesystemRule>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub protected_patterns: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedNetworkConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http_proxy: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub https_proxy: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub all_proxy: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub no_proxy: Vec<String>,
    #[serde(default)]
    pub allow_loopback: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SandboxPolicyDecision {
    Allow,
    Ask { risk: RiskLevel, reason: String },
    Deny { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxPlanParams {
    pub policy: SandboxPolicy,
    pub request: SandboxExecRequest,
    #[serde(default = "default_sandbox_preference")]
    pub preference: SandboxPreference,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxPlanResult {
    pub decision: SandboxPolicyDecision,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<SandboxExecPlan>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxExecParams {
    pub policy: SandboxPolicy,
    pub request: SandboxExecRequest,
    #[serde(default = "default_sandbox_preference")]
    pub preference: SandboxPreference,
    #[serde(default)]
    pub approval_granted: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub managed_network: Option<ManagedNetworkConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_bytes: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxExecResult {
    pub decision: SandboxPolicyDecision,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<SandboxExecPlan>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(default)]
    pub stdout: String,
    #[serde(default)]
    pub stderr: String,
    #[serde(default)]
    pub timed_out: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub audit: Vec<SandboxAuditRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxAuditRecord {
    pub action: String,
    pub decision: SandboxPolicyDecision,
    pub command: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enforcement: Option<SandboxEnforcement>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox_type: Option<SandboxType>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolPermissionManifest {
    pub tool_name: String,
    #[serde(default)]
    pub additional_permissions: AdditionalPermissionProfile,
    #[serde(default = "default_sandbox_preference")]
    pub sandbox_preference: SandboxPreference,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxUserConfig {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub project_roots: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub readable_roots: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub writable_roots: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub filesystem_rules: Vec<FilesystemRule>,
    #[serde(default)]
    pub allow_internet: bool,
    #[serde(default)]
    pub allow_ssh: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub managed_network: Option<ManagedNetworkConfig>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub protected_patterns: Vec<String>,
    #[serde(default = "default_sandbox_preference")]
    pub sandbox_preference: SandboxPreference,
}

fn default_sandbox_preference() -> SandboxPreference {
    SandboxPreference::Auto
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilesystemRule {
    pub root: FilesystemRoot,
    pub access: FilesystemAccess,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum FilesystemRoot {
    Cwd,
    Workspace,
    ProjectRoots {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        subpath: Option<String>,
    },
    Home,
    Temp,
    PlatformDefaults,
    Unknown {
        path: String,
    },
    Path {
        path: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FilesystemAccess {
    Read,
    Write,
    None,
    ReadOnly,
    ReadWrite,
    DenyRead,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NetworkPolicy {
    Disabled,
    Ask,
    Enabled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FilesystemPolicy {
    ReadOnly,
    WorkspaceWrite,
    Unrestricted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SandboxPreference {
    Auto,
    Require,
    Forbid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SandboxType {
    None,
    MacosSeatbelt,
    LinuxLandlock,
    LinuxBubblewrap,
    WindowsRestrictedToken,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxStatus {
    pub supported: bool,
    pub enforcement: SandboxEnforcement,
    pub platform: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub available_sandbox_types: Vec<SandboxType>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SandboxEnforcement {
    None,
    ReviewOnly,
    OsSandbox,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentDefinition {
    pub name: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<AgentSource>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_mode: Option<PermissionMode>,
    #[serde(default)]
    pub background: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_root: Option<String>,
    pub instructions: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentSource {
    BuiltIn,
    Plugin,
    User,
    Project,
    Cli,
    ManagedPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedAgent {
    pub active: AgentDefinition,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shadowed: Vec<AgentDefinition>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillRef {
    pub id: SkillId,
    pub name: String,
    pub description: String,
    pub source: SkillSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub priority: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SkillSource {
    BuiltIn,
    User,
    Project,
    Cli,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedSkill {
    pub active: SkillRef,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shadowed: Vec<SkillRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRun {
    pub id: AgentRunId,
    pub thread_id: ThreadId,
    pub agent_name: String,
    pub status: AgentRunStatus,
    pub task: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_root: Option<String>,
    #[serde(default)]
    pub background: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_mode: Option<PermissionMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_allowlist: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_denylist: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub isolation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skills: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_turns: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentRunStatus {
    Running,
    Blocked,
    Completed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginRef {
    pub id: PluginId,
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryStatus {
    pub enabled: bool,
    pub backend: String,
    pub scope: String,
    #[serde(default)]
    pub memory_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerRef {
    pub id: McpServerId,
    pub name: String,
    pub status: McpServerStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when_to_use: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum McpServerStatus {
    Enabled,
    Disabled,
    Failed,
    AuthRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpActionParams {
    pub thread_id: ThreadId,
    pub server_id: McpServerId,
    pub action: McpAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum McpAction {
    Reload,
    Test,
    Auth,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelRef {
    pub id: ModelId,
    pub provider: String,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    pub level: DiagnosticLevel,
    pub message: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DiagnosticLevel {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeErrorCategory {
    NotFound,
    AlreadyExists,
    AlreadyResolved,
    OwnershipMismatch,
    TerminalState,
    ApprovalRequired,
    ToolPairing,
    EventStore,
    Provider,
    InvalidRequest,
}

impl RuntimeErrorCategory {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NotFound => "not_found",
            Self::AlreadyExists => "already_exists",
            Self::AlreadyResolved => "already_resolved",
            Self::OwnershipMismatch => "ownership_mismatch",
            Self::TerminalState => "terminal_state",
            Self::ApprovalRequired => "approval_required",
            Self::ToolPairing => "tool_pairing",
            Self::EventStore => "event_store",
            Self::Provider => "provider",
            Self::InvalidRequest => "invalid_request",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeError {
    pub code: String,
    pub category: RuntimeErrorCategory,
    pub message: String,
}

impl RuntimeError {
    pub fn new(
        code: impl Into<String>,
        category: RuntimeErrorCategory,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code: code.into(),
            category,
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeParams {
    pub client_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol_version: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub client_capabilities: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResult {
    pub protocol_version: String,
    pub min_protocol_version: String,
    pub protocol_compatible: bool,
    pub server_name: String,
    pub server_version: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub server_capabilities: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub accepted_client_capabilities: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadStartParams {
    pub project: ProjectRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadStartResult {
    pub thread: Thread,
    pub events: Vec<Event>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadGoalGetParams {
    pub thread_id: ThreadId,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadGoalGetResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal: Option<ThreadGoal>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadGoalSetParams {
    pub thread_id: ThreadId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub objective: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<ThreadGoalStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_budget: Option<Option<i64>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadGoalSetResult {
    pub goal: ThreadGoal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadGoalClearParams {
    pub thread_id: ThreadId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadGoalClearResult {
    pub cleared: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadGoalContinuationParams {
    pub thread_id: ThreadId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_continuations: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadGoalContinuationResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal: Option<ThreadGoal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub continuation: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnStartParams {
    pub thread_id: ThreadId,
    pub input: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assistant_response: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub simulated_tool: Option<SimulatedToolUse>,
    #[serde(default)]
    pub requested_continuations: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_hook_feedback: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub defer_completion: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnStartResult {
    pub turn: Turn,
    pub events: Vec<Event>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ExecutionMode {
    Blocking,
    Background,
}

impl Default for ExecutionMode {
    fn default() -> Self {
        Self::Blocking
    }
}

impl ExecutionMode {
    pub fn is_blocking(&self) -> bool {
        matches!(self, Self::Blocking)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgenticTurnParams {
    pub thread_id: ThreadId,
    pub input: String,
    #[serde(default, skip_serializing_if = "ExecutionMode::is_blocking")]
    pub execution_mode: ExecutionMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub follow_up: Option<FollowUpChainContext>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox_policy: Option<SandboxPolicy>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub model_steps: Vec<ScriptedModelStep>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_provider: Option<DirectModelProviderConfig>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_definitions: Vec<ToolDefinition>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub approved_tool_call_ids: Vec<ToolCallId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancellation: Option<AgenticCancellation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_continuations: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgenticTurnResumeParams {
    pub thread_id: ThreadId,
    pub turn_id: TurnId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub follow_up: Option<FollowUpChainContext>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ask_user_response: Option<AskUserResponse>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox_policy: Option<SandboxPolicy>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub model_steps: Vec<ScriptedModelStep>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_provider: Option<DirectModelProviderConfig>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_definitions: Vec<ToolDefinition>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub approved_tool_call_ids: Vec<ToolCallId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancellation: Option<AgenticCancellation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_continuations: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptedModelStep {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assistant_deltas: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_results: Vec<ToolResult>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub final_response: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderTranscriptReplaySnapshot {
    pub turn_id: TurnId,
    pub model_provider: DirectModelProviderConfig,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub messages: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectModelProviderConfig {
    pub kind: DirectModelProviderKind,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key_env: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub stream: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DirectModelProviderKind {
    #[serde(rename = "openai-compatible")]
    OpenAiCompatible,
    #[serde(rename = "openai-codex")]
    OpenAiCodex,
    #[serde(rename = "github-copilot")]
    GitHubCopilot,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgenticCancellation {
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before_model_continuation: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_call_ids: Vec<ToolCallId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgenticTurnResult {
    pub turn: Turn,
    pub events: Vec<Event>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub awaiting_approval: Option<ApprovalRequest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub awaiting_question: Option<AskUserRequest>,
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn is_true(value: &bool) -> bool {
    *value
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventsListParams {
    pub thread_id: ThreadId,
    #[serde(default)]
    pub after: EventId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventsListResult {
    pub events: Vec<Event>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeMetrics {
    pub thread_count: u64,
    pub turn_count: u64,
    pub event_count: u64,
    pub pending_approvals: u64,
    pub pending_questions: u64,
    pub plugin_count: u64,
    pub mcp_server_count: u64,
    pub model_count: u64,
    pub agent_definition_count: u64,
    pub agent_run_count: u64,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub turn_status_counts: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DebugBundle {
    pub schema_version: u32,
    pub redacted: bool,
    pub metrics: RuntimeMetrics,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolRecordParams {
    pub thread_id: ThreadId,
    pub turn_id: TurnId,
    pub call: ToolCall,
    pub result: ToolResult,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolBatchRecordParams {
    pub thread_id: ThreadId,
    pub turn_id: TurnId,
    pub tools: Vec<SimulatedToolUse>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_concurrency: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolBatchRecordResult {
    pub batches: Vec<ToolExecutionBatch>,
    pub events: Vec<Event>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalRespondParams {
    pub thread_id: ThreadId,
    pub request_id: ApprovalId,
    pub decision: ApprovalOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestionRespondParams {
    pub thread_id: ThreadId,
    pub request_id: QuestionId,
    pub answer: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeListResult<T> {
    pub items: Vec<T>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BackgroundTaskStatus {
    Running,
    Completed,
    Failed,
    Killed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackgroundTaskInfo {
    pub id: String,
    pub command: String,
    pub cwd: String,
    pub output_path: String,
    pub status: BackgroundTaskStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_bytes: Option<u64>,
}

pub type BackgroundListResult = RuntimeListResult<BackgroundTaskInfo>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackgroundReadParams {
    pub task_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_bytes: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackgroundReadResult {
    pub task: BackgroundTaskInfo,
    pub output: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_bytes: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub truncated: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackgroundKillParams {
    pub task_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackgroundKillResult {
    pub task: BackgroundTaskInfo,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentDispatchParams {
    pub thread_id: ThreadId,
    pub agent_name: String,
    pub task: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_root: Option<String>,
    #[serde(default)]
    pub background: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_mode: Option<PermissionMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_allowlist: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_denylist: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub isolation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skills: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_turns: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentDispatchResult {
    pub run: AgentRun,
    pub events: Vec<Event>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillListParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<ThreadId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCompleteParams {
    pub thread_id: ThreadId,
    pub run_id: AgentRunId,
    pub output: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentBlockParams {
    pub thread_id: ThreadId,
    pub run_id: AgentRunId,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemorySetParams {
    pub thread_id: ThreadId,
    pub status: MemoryStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryControlParams {
    pub thread_id: ThreadId,
    pub action: String,
    #[serde(default)]
    pub apply: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryControl {
    pub id: String,
    pub label: String,
    pub command: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryControlResult {
    pub title: String,
    pub summary: String,
    pub status: MemoryStatus,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub controls: Vec<MemoryControl>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diagnostic>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<Event>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HandoffCompactParams {
    pub thread_id: ThreadId,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<CompactionHandoffDetails>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TodoOutcome {
    pub id: String,
    pub content: String,
    pub status: TodoStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    pub outcome: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactionHandoffDetails {
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compacted_at: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub remaining_todos: Vec<TodoItem>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub completed_outcomes: Vec<TodoOutcome>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelSelectParams {
    pub thread_id: ThreadId,
    pub model_id: ModelId,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PiBridgeEventParams {
    pub thread_id: ThreadId,
    pub name: String,
    #[serde(default)]
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SideQuestionParams {
    pub thread_id: ThreadId,
    pub question: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandPrepareParams {
    pub command: String,
    #[serde(default)]
    pub args: String,
    #[serde(default)]
    pub context: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_variant_append: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandPrepareResult {
    pub command: String,
    pub input: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_prompt_profile: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SideQuestionResult {
    pub answer: String,
    pub events: Vec<Event>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn serializes_golden_turn_event() {
        let event = Event {
            id: 7,
            thread_id: "thread-1".to_string(),
            turn_id: Some("turn-1".to_string()),
            kind: EventKind::TurnPhaseChanged {
                phase: TurnPhase::Tools,
            },
        };

        let actual = serde_json::to_value(event).unwrap();
        assert_eq!(
            actual,
            json!({
                "id": 7,
                "threadId": "thread-1",
                "turnId": "turn-1",
                "kind": { "type": "turnPhaseChanged", "phase": "tools" }
            })
        );
    }

    #[test]
    fn serializes_initialize_capability_negotiation_fields() {
        let result = InitializeResult {
            protocol_version: OPPI_PROTOCOL_VERSION.to_string(),
            min_protocol_version: OPPI_MIN_PROTOCOL_VERSION.to_string(),
            protocol_compatible: true,
            server_name: "oppi-server".to_string(),
            server_version: "0.0.0-test".to_string(),
            server_capabilities: vec!["threads".to_string(), "sandbox".to_string()],
            accepted_client_capabilities: vec!["sandbox".to_string()],
        };

        let actual = serde_json::to_value(result).unwrap();
        assert_eq!(actual["protocolVersion"], json!(OPPI_PROTOCOL_VERSION));
        assert_eq!(
            actual["minProtocolVersion"],
            json!(OPPI_MIN_PROTOCOL_VERSION)
        );
        assert_eq!(actual["protocolCompatible"], json!(true));
        assert_eq!(actual["serverCapabilities"], json!(["threads", "sandbox"]));
        assert_eq!(actual["acceptedClientCapabilities"], json!(["sandbox"]));
    }

    #[test]
    fn thread_goal_protocol_serializes_camel_case_status_and_budget() {
        let goal = ThreadGoal {
            thread_id: "thread-1".to_string(),
            objective: "Ship native goal mode".to_string(),
            status: ThreadGoalStatus::BudgetLimited,
            token_budget: Some(10_000),
            tokens_used: 10_500,
            time_used_seconds: 125,
            created_at_ms: 1_700_000_000_000,
            updated_at_ms: 1_700_000_001_000,
        };

        let actual = serde_json::to_value(goal).unwrap();
        assert_eq!(actual["status"], json!("budgetLimited"));
        assert_eq!(actual["tokenBudget"], json!(10_000));
        assert_eq!(actual["tokensUsed"], json!(10_500));
    }

    #[test]
    fn thread_goal_events_round_trip() {
        let event = Event {
            id: 1,
            thread_id: "thread-1".to_string(),
            turn_id: None,
            kind: EventKind::ThreadGoalUpdated {
                goal: ThreadGoal {
                    thread_id: "thread-1".to_string(),
                    objective: "Finish the goal".to_string(),
                    status: ThreadGoalStatus::Active,
                    token_budget: None,
                    tokens_used: 0,
                    time_used_seconds: 0,
                    created_at_ms: 10,
                    updated_at_ms: 10,
                },
            },
        };

        let encoded = serde_json::to_string(&event).unwrap();
        assert!(encoded.contains("\"threadGoalUpdated\""));
        assert_eq!(serde_json::from_str::<Event>(&encoded).unwrap(), event);
    }

    #[test]
    fn thread_goal_continuation_protocol_uses_camel_case() {
        let params = ThreadGoalContinuationParams {
            thread_id: "thread-1".to_string(),
            max_continuations: Some(3),
        };
        let actual = serde_json::to_value(params).unwrap();
        assert_eq!(
            actual,
            json!({
                "threadId": "thread-1",
                "maxContinuations": 3
            })
        );

        let result = ThreadGoalContinuationResult {
            goal: None,
            prompt: Some("continue".to_string()),
            continuation: Some(2),
            blocked_reason: Some("paused".to_string()),
        };
        let actual = serde_json::to_value(result).unwrap();
        assert_eq!(actual["blockedReason"], json!("paused"));
        assert_eq!(actual["continuation"], json!(2));
    }

    #[test]
    fn serializes_runtime_error_category_taxonomy() {
        let error = RuntimeError::new(
            "thread_not_found",
            RuntimeErrorCategory::NotFound,
            "thread not found: thread-404",
        );

        let actual = serde_json::to_value(error).unwrap();
        assert_eq!(actual["code"], json!("thread_not_found"));
        assert_eq!(actual["category"], json!("not_found"));
        assert_eq!(
            RuntimeErrorCategory::OwnershipMismatch.as_str(),
            "ownership_mismatch"
        );
        assert_eq!(RuntimeErrorCategory::Provider.as_str(), "provider");
    }

    #[test]
    fn serializes_direct_model_provider_config() {
        let params = AgenticTurnParams {
            thread_id: "thread-1".to_string(),
            input: "hello".to_string(),
            execution_mode: ExecutionMode::Blocking,
            follow_up: Some(FollowUpChainContext {
                chain_id: Some("follow-1".to_string()),
                root_prompt: "original task".to_string(),
                follow_ups: vec![FollowUpItemContext {
                    id: "1".to_string(),
                    text: "current follow-up".to_string(),
                    status: FollowUpStatus::Running,
                }],
                current_follow_up_id: Some("1".to_string()),
                prompt_variant_append: Some("variant note".to_string()),
            }),
            sandbox_policy: Some(SandboxPolicy {
                permission_profile: PermissionProfile {
                    mode: PermissionMode::ReadOnly,
                    readable_roots: vec!["/repo".to_string()],
                    writable_roots: Vec::new(),
                    filesystem_rules: Vec::new(),
                    protected_patterns: Vec::new(),
                },
                network: NetworkPolicy::Disabled,
                filesystem: FilesystemPolicy::ReadOnly,
            }),
            model_steps: Vec::new(),
            model_provider: Some(DirectModelProviderConfig {
                kind: DirectModelProviderKind::OpenAiCompatible,
                model: "gpt-test".to_string(),
                base_url: Some("http://127.0.0.1:3000/v1".to_string()),
                api_key_env: Some("OPPI_TEST_API_KEY".to_string()),
                system_prompt: Some("You are OPPi.".to_string()),
                temperature: Some(0.2),
                reasoning_effort: Some("medium".to_string()),
                max_output_tokens: Some(128),
                stream: true,
            }),
            tool_definitions: Vec::new(),
            approved_tool_call_ids: Vec::new(),
            cancellation: None,
            max_continuations: Some(1),
        };

        let actual = serde_json::to_value(params).unwrap();
        assert_eq!(actual["followUp"]["chainId"], json!("follow-1"));
        assert_eq!(
            actual["followUp"]["followUps"][0]["status"],
            json!("running")
        );
        assert_eq!(
            actual["sandboxPolicy"]["permissionProfile"]["mode"],
            json!("read-only")
        );
        assert_eq!(actual["modelProvider"]["kind"], json!("openai-compatible"));
        assert_eq!(
            actual["modelProvider"]["apiKeyEnv"],
            json!("OPPI_TEST_API_KEY")
        );
        assert_eq!(actual["modelProvider"]["reasoningEffort"], json!("medium"));
        assert_eq!(actual["modelProvider"]["maxOutputTokens"], json!(128));
        assert_eq!(actual["modelProvider"]["stream"], json!(true));
    }

    #[test]
    fn serializes_permission_mode_as_kebab_case() {
        let profile = PermissionProfile {
            mode: PermissionMode::AutoReview,
            readable_roots: vec!["/repo".to_string()],
            writable_roots: vec!["/repo".to_string()],
            filesystem_rules: Vec::new(),
            protected_patterns: vec![".env*".to_string()],
        };

        let actual = serde_json::to_value(profile).unwrap();
        assert_eq!(actual["mode"], json!("auto-review"));
    }

    #[test]
    fn serializes_todo_state_event() {
        let event = Event {
            id: 4,
            thread_id: "thread-1".to_string(),
            turn_id: Some("turn-1".to_string()),
            kind: EventKind::TodosUpdated {
                state: TodoState {
                    summary: "Plan updated".to_string(),
                    todos: vec![TodoItem {
                        id: "impl".to_string(),
                        content: "Implement todo parity".to_string(),
                        status: TodoStatus::InProgress,
                        priority: Some(TodoPriority::High),
                        phase: Some("implementation".to_string()),
                        notes: None,
                    }],
                },
            },
        };
        let actual = serde_json::to_value(event).unwrap();
        assert_eq!(actual["kind"]["type"], json!("todosUpdated"));
        assert_eq!(
            actual["kind"]["state"]["todos"][0]["status"],
            json!("in_progress")
        );
        assert_eq!(
            actual["kind"]["state"]["todos"][0]["priority"],
            json!("high")
        );
    }

    #[test]
    fn serializes_background_task_info() {
        let result = BackgroundListResult {
            items: vec![BackgroundTaskInfo {
                id: "shell-1".to_string(),
                command: "echo hi".to_string(),
                cwd: "/repo".to_string(),
                output_path: "/repo/output/shelltool/shell-1.log".to_string(),
                status: BackgroundTaskStatus::Running,
                started_at_ms: Some(123),
                finished_at_ms: None,
                exit_code: None,
                output_bytes: Some(7),
            }],
        };

        let actual = serde_json::to_value(result).unwrap();
        assert_eq!(actual["items"][0]["id"], json!("shell-1"));
        assert_eq!(
            actual["items"][0]["outputPath"],
            json!("/repo/output/shelltool/shell-1.log")
        );
        assert_eq!(actual["items"][0]["status"], json!("running"));
        assert_eq!(actual["items"][0]["startedAtMs"], json!(123));
        assert_eq!(actual["items"][0]["outputBytes"], json!(7));
    }

    #[test]
    fn serializes_compaction_handoff_details() {
        let event = Event {
            id: 5,
            thread_id: "thread-1".to_string(),
            turn_id: None,
            kind: EventKind::HandoffCompacted {
                summary: "compact summary".to_string(),
                details: Some(CompactionHandoffDetails {
                    source: "oppi-runtime".to_string(),
                    version: Some(1),
                    compacted_at: Some("2026-01-01T00:00:00.000Z".to_string()),
                    remaining_todos: vec![TodoItem {
                        id: "active".to_string(),
                        content: "Keep working".to_string(),
                        status: TodoStatus::InProgress,
                        priority: None,
                        phase: None,
                        notes: None,
                    }],
                    completed_outcomes: vec![TodoOutcome {
                        id: "done".to_string(),
                        content: "Finish slice".to_string(),
                        status: TodoStatus::Completed,
                        phase: None,
                        notes: Some("Validated".to_string()),
                        outcome: "Validated".to_string(),
                        updated_at: None,
                    }],
                }),
            },
        };

        let actual = serde_json::to_value(event).unwrap();
        assert_eq!(actual["kind"]["type"], json!("handoffCompacted"));
        assert_eq!(
            actual["kind"]["details"]["remainingTodos"][0]["status"],
            json!("in_progress")
        );
        assert_eq!(
            actual["kind"]["details"]["completedOutcomes"][0]["outcome"],
            json!("Validated")
        );
    }

    #[test]
    fn serializes_tool_batch_started_event() {
        let event = Event {
            id: 3,
            thread_id: "thread-1".to_string(),
            turn_id: Some("turn-1".to_string()),
            kind: EventKind::ToolBatchStarted {
                batch: ToolExecutionBatch {
                    id: "turn-1-batch-1".to_string(),
                    execution: ToolBatchExecution::Concurrent,
                    tool_call_ids: vec!["read-1".to_string(), "grep-1".to_string()],
                    concurrency_limit: Some(10),
                },
            },
        };
        let actual = serde_json::to_value(event).unwrap();
        assert_eq!(actual["kind"]["type"], json!("toolBatchStarted"));
        assert_eq!(actual["kind"]["batch"]["execution"], json!("concurrent"));
        assert_eq!(
            actual["kind"]["batch"]["toolCallIds"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn serializes_artifact_created_event() {
        let event = Event {
            id: 4,
            thread_id: "thread-1".to_string(),
            turn_id: Some("turn-1".to_string()),
            kind: EventKind::ArtifactCreated {
                artifact: ArtifactMetadata {
                    id: "artifact-image-1".to_string(),
                    tool_call_id: "image-1".to_string(),
                    output_path: "output/image.png".to_string(),
                    mime_type: Some("image/png".to_string()),
                    width: Some(64),
                    height: Some(64),
                    source_images: vec!["input.png".to_string()],
                    mask: Some("mask.png".to_string()),
                    backend: Some("openai-images".to_string()),
                    model: Some("gpt-image-2".to_string()),
                    bytes: Some(12),
                    diagnostics: Vec::new(),
                },
            },
        };
        let actual = serde_json::to_value(event).unwrap();
        assert_eq!(actual["kind"]["type"], json!("artifactCreated"));
        assert_eq!(
            actual["kind"]["artifact"]["outputPath"],
            json!("output/image.png")
        );
        assert_eq!(actual["kind"]["artifact"]["mimeType"], json!("image/png"));
    }

    #[test]
    fn serializes_provider_transcript_snapshot_event() {
        let event = Event {
            id: 3,
            thread_id: "thread-1".to_string(),
            turn_id: Some("turn-1".to_string()),
            kind: EventKind::ProviderTranscriptSnapshot {
                snapshot: ProviderTranscriptReplaySnapshot {
                    turn_id: "turn-1".to_string(),
                    model_provider: DirectModelProviderConfig {
                        kind: DirectModelProviderKind::OpenAiCompatible,
                        model: "gpt-test".to_string(),
                        base_url: Some("https://example.com/v1".to_string()),
                        api_key_env: Some("OPPI_TEST_API_KEY".to_string()),
                        system_prompt: None,
                        temperature: None,
                        reasoning_effort: None,
                        max_output_tokens: None,
                        stream: false,
                    },
                    messages: vec![json!({ "role": "assistant", "content": "paused" })],
                },
            },
        };
        let actual = serde_json::to_value(event).unwrap();
        assert_eq!(actual["kind"]["type"], json!("providerTranscriptSnapshot"));
        assert_eq!(actual["kind"]["snapshot"]["turnId"], json!("turn-1"));
        assert_eq!(
            actual["kind"]["snapshot"]["modelProvider"]["model"],
            json!("gpt-test")
        );
    }

    #[test]
    fn serializes_resolved_skill() {
        let skill = ResolvedSkill {
            active: SkillRef {
                id: "imagegen".to_string(),
                name: "imagegen".to_string(),
                description: "Generate images".to_string(),
                source: SkillSource::BuiltIn,
                path: Some("builtin:imagegen".to_string()),
                priority: 0,
            },
            shadowed: vec![SkillRef {
                id: "imagegen".to_string(),
                name: "imagegen".to_string(),
                description: "User override".to_string(),
                source: SkillSource::User,
                path: Some("/skills/imagegen/SKILL.md".to_string()),
                priority: 10,
            }],
        };
        let actual = serde_json::to_value(skill).unwrap();
        assert_eq!(actual["active"]["name"], json!("imagegen"));
        assert_eq!(actual["active"]["source"], json!("builtIn"));
        assert_eq!(actual["shadowed"][0]["source"], json!("user"));
    }

    #[test]
    fn serializes_agent_started_event() {
        let event = Event {
            id: 2,
            thread_id: "thread-1".to_string(),
            turn_id: None,
            kind: EventKind::AgentStarted {
                run: AgentRun {
                    id: "agent-run-1".to_string(),
                    thread_id: "thread-1".to_string(),
                    agent_name: "reviewer".to_string(),
                    status: AgentRunStatus::Running,
                    task: "review changes".to_string(),
                    worktree_root: Some("/repo-wt".to_string()),
                    background: true,
                    role: Some("reviewer".to_string()),
                    model: Some("gpt-review".to_string()),
                    effort: Some("medium".to_string()),
                    permission_mode: Some(PermissionMode::ReadOnly),
                    memory_mode: Some("disabled".to_string()),
                    tool_allowlist: vec!["read_file".to_string()],
                    tool_denylist: vec!["shell_exec".to_string()],
                    isolation: Some("thread".to_string()),
                    color: Some("cyan".to_string()),
                    skills: vec!["independent".to_string()],
                    max_turns: Some(3),
                },
            },
        };
        let actual = serde_json::to_value(event).unwrap();
        assert_eq!(actual["kind"]["type"], json!("agentStarted"));
        assert_eq!(actual["kind"]["run"]["worktreeRoot"], json!("/repo-wt"));
        assert_eq!(actual["kind"]["run"]["role"], json!("reviewer"));
        assert_eq!(actual["kind"]["run"]["model"], json!("gpt-review"));
        assert_eq!(actual["kind"]["run"]["background"], json!(true));
        assert_eq!(actual["kind"]["run"]["permissionMode"], json!("read-only"));
        assert_eq!(actual["kind"]["run"]["toolAllowlist"], json!(["read_file"]));
        assert_eq!(actual["kind"]["run"]["skills"], json!(["independent"]));
    }
}
