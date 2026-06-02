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
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub created_at_ms: u64,
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
    ThreadForkPointSet {
        thread_id: ThreadId,
        source_thread_id: ThreadId,
        source_event_id: EventId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        history_cutoff_event_id: Option<EventId>,
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
    ThreadItemLabelsUpdated {
        target_event_id: EventId,
        labels: Vec<String>,
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
    PermissionReviewStarted {
        record: GuardianReviewRecord,
    },
    PermissionReviewCompleted {
        record: GuardianReviewRecord,
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
    MemoryJobUpdated {
        job: MemoryJobRef,
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent_thread_id: Option<ThreadId>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        side_thread_id: Option<ThreadId>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        side_turn_id: Option<TurnId>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        status: Option<TurnStatus>,
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

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_schema: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub annotations: BTreeMap<String, serde_json::Value>,
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
    #[serde(
        default = "default_ask_user_timeout_ms",
        skip_serializing_if = "is_default_ask_user_timeout_ms"
    )]
    pub timeout_ms: u64,
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

pub const DEFAULT_ASK_USER_TIMEOUT_MS: u64 = 5 * 60 * 1_000;

fn default_ask_user_timeout_ms() -> u64 {
    DEFAULT_ASK_USER_TIMEOUT_MS
}

fn is_default_ask_user_timeout_ms(value: &u64) -> bool {
    *value == DEFAULT_ASK_USER_TIMEOUT_MS
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AskUserSettings {
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AskUserSettingsUpdateParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<ThreadId>,
    pub settings: AskUserSettings,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AskUserSettingsResult {
    pub settings: AskUserSettings,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GuardianReviewOutcome {
    Allow,
    Deny,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GuardianAuthorizationLevel {
    Unknown,
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GuardianCacheScope {
    None,
    Exact,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GuardianReviewStatus {
    Reviewing,
    Approved,
    Denied,
    FailedClosed,
    Cached,
    CircuitBlocked,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GuardianRiskSummary {
    pub category: String,
    pub reason: String,
    #[serde(default)]
    pub protected_hits: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hints: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GuardianReviewDecision {
    pub outcome: GuardianReviewOutcome,
    pub risk_level: RiskLevel,
    pub user_authorization: GuardianAuthorizationLevel,
    pub rationale: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_scope: Option<GuardianCacheScope>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GuardianReviewRecord {
    pub id: String,
    pub timestamp: String,
    pub status: GuardianReviewStatus,
    pub mode: PermissionMode,
    pub tool_name: String,
    pub tool_call_id: ToolCallId,
    pub signature: String,
    pub circuit_key: String,
    pub summary: String,
    pub risk: GuardianRiskSummary,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision: Option<GuardianReviewDecision>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewer_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cached_from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strict_json: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GuardianReviewSettings {
    pub timeout_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewer_model: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionReviewHistoryParams {
    pub thread_id: ThreadId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionReviewHistoryResult {
    pub records: Vec<GuardianReviewRecord>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionReviewContextParams {
    pub thread_id: ThreadId,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionReviewContextResult {
    pub settings: GuardianReviewSettings,
    pub reviewer_provider_policy: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prompt_context: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reviewer_tools: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hard_denies: Vec<String>,
    pub output_contract: serde_json::Value,
    pub prompt_skeleton: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionReviewClearSessionParams {
    pub thread_id: ThreadId,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionReviewClearSessionResult {
    pub cleared_reviews: usize,
    pub cleared_cache_entries: usize,
    pub cleared_circuit_keys: usize,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionReviewSettingsUpdateParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<ThreadId>,
    pub settings: GuardianReviewSettings,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionReviewSettingsResult {
    pub settings: GuardianReviewSettings,
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_denylist: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_mode: Option<PermissionMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_mode: Option<String>,
    #[serde(default)]
    pub background: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_root: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub isolation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skills: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_turns: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    #[serde(default)]
    pub read_only: bool,
    pub instructions: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentSource {
    BuiltIn,
    ClaudeUser,
    User,
    ClaudeProject,
    Project,
    Plugin,
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
    Plugin,
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
pub struct PromptTemplateRef {
    pub id: String,
    pub name: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub argument_hint: Option<String>,
    pub source: PromptTemplateSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub priority: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PromptTemplateSource {
    BuiltIn,
    User,
    Plugin,
    Project,
    Cli,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedPromptTemplate {
    pub active: PromptTemplateRef,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shadowed: Vec<PromptTemplateRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeRef {
    pub id: String,
    pub name: String,
    pub source: ThemeSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub priority: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ThemeSource {
    BuiltIn,
    User,
    Plugin,
    Project,
    Cli,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedTheme {
    pub active: ThemeRef,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shadowed: Vec<ThemeRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRun {
    pub id: AgentRunId,
    pub thread_id: ThreadId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_thread_id: Option<ThreadId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub child_thread_id: Option<ThreadId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub child_turn_id: Option<TurnId>,
    pub agent_name: String,
    pub status: AgentRunStatus,
    pub task: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_root: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_root: Option<String>,
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
pub enum PluginSourceType {
    Local,
    Npm,
    Git,
    Url,
    Marketplace,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PluginMaterializationStatus {
    NotRequired,
    NotMaterialized,
    Materializing,
    Materialized,
    Failed,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginProvenance {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub marketplace: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub catalog_url: Option<String>,
    pub resolved_source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginMaterializationRef {
    pub status: PluginMaterializationStatus,
    pub source_type: PluginSourceType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manifest_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checksum: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub materialized_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<PluginProvenance>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<PluginDiagnostic>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PluginExecutableBoundaryKind {
    StaticResourcesOnly,
    JsonRpcSubprocess,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PluginExecutableBoundaryStatus {
    Unsupported,
    Disabled,
    Enabled,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginExecutableBoundaryRef {
    pub kind: PluginExecutableBoundaryKind,
    pub status: PluginExecutableBoundaryStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capabilities: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<PluginDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginRef {
    pub id: PluginId,
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trusted: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<PluginScope>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_type: Option<PluginSourceType>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub materialization: Option<PluginMaterializationRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub executable_boundary: Option<PluginExecutableBoundaryRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resources: Vec<PluginResourceRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<PluginDiagnostic>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PluginScope {
    Global,
    Project,
    Env,
    Runtime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PluginResourceKind {
    Skill,
    Extension,
    Prompt,
    Theme,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PluginResourceStatus {
    Loaded,
    Unsupported,
    Skipped,
    Missing,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginResourceRef {
    pub kind: PluginResourceKind,
    pub path: String,
    pub status: PluginResourceStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginDiagnostic {
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub severity: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginListParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<ThreadId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginMaterializeParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<ThreadId>,
    pub plugin_id: PluginId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<PluginScope>,
    #[serde(default)]
    pub yes: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginMaterializeResult {
    pub plugin: PluginRef,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<Event>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginMcpTrustParams {
    pub thread_id: ThreadId,
    pub plugin_id: PluginId,
    pub server_id: McpServerId,
    pub enabled: bool,
    pub trusted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryStatus {
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub initialized: bool,
    pub backend: String,
    pub scope: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub store_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(default)]
    pub memory_count: u64,
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub pinned_count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_write_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_consolidated_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_depth: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dashboard_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_owner: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_adapter: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_adapter_kind: Option<String>,
}

pub type MemoryJobId = String;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MemoryJobKind {
    Status,
    Recall,
    Remember,
    Consolidate,
    Sync,
    Dashboard,
    List,
    RecordMutation,
    Compact,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MemoryJobStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    Skipped,
    Cancelled,
}

impl MemoryJobStatus {
    pub fn is_active(self) -> bool {
        matches!(self, Self::Queued | Self::Running)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryJobRef {
    pub id: MemoryJobId,
    pub thread_id: ThreadId,
    pub kind: MemoryJobKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage: Option<String>,
    pub status: MemoryJobStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queued_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<String>,
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub record_count: u64,
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub deleted_count: u64,
    #[serde(default, skip_serializing_if = "is_false")]
    pub dry_run: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub backend_applied: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deep_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_used: Option<String>,
}

impl Default for MemoryStatus {
    fn default() -> Self {
        Self {
            enabled: false,
            initialized: false,
            backend: "none".to_string(),
            scope: "project".to_string(),
            store_path: None,
            project_id: None,
            memory_count: 0,
            pinned_count: 0,
            last_write_at: None,
            last_consolidated_at: None,
            queue_depth: None,
            dashboard_url: None,
            execution_owner: None,
            execution_adapter: None,
            execution_adapter_kind: None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpToolsCapability {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub list_changed: Option<bool>,
}

impl McpToolsCapability {
    pub fn is_empty(&self) -> bool {
        self.list_changed.is_none()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpResourcesCapability {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subscribe: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub list_changed: Option<bool>,
}

impl McpResourcesCapability {
    pub fn is_empty(&self) -> bool {
        self.subscribe.is_none() && self.list_changed.is_none()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpPromptsCapability {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub list_changed: Option<bool>,
}

impl McpPromptsCapability {
    pub fn is_empty(&self) -> bool {
        self.list_changed.is_none()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerCapabilities {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<McpToolsCapability>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resources: Option<McpResourcesCapability>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompts: Option<McpPromptsCapability>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sampling: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elicitation: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tasks: Option<serde_json::Value>,
}

impl McpServerCapabilities {
    pub fn is_empty(&self) -> bool {
        self.tools.is_none()
            && self.resources.is_none()
            && self.prompts.is_none()
            && self.sampling.is_none()
            && self.elicitation.is_none()
            && self.tasks.is_none()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpListRefreshHints {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<McpListRefreshHint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resources: Option<McpListRefreshHint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompts: Option<McpListRefreshHint>,
}

impl McpListRefreshHints {
    pub fn is_empty(&self) -> bool {
        self.tools.is_none() && self.resources.is_none() && self.prompts.is_none()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpListRefreshHint {
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub refresh_count: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerRef {
    pub id: McpServerId,
    pub name: String,
    pub status: McpServerStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<McpServerSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transport: Option<McpTransportConfig>,
    #[serde(default)]
    pub trusted: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_refs: Vec<McpToolRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resource_subscriptions: Vec<McpResourceSubscriptionRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when_to_use: Option<String>,
    #[serde(default, skip_serializing_if = "McpServerCapabilities::is_empty")]
    pub capabilities: McpServerCapabilities,
    #[serde(default, skip_serializing_if = "McpListRefreshHints::is_empty")]
    pub list_refresh_hints: McpListRefreshHints,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<McpDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpResourceSubscriptionRef {
    pub uri: String,
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub update_count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_updated_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerConfig {
    pub id: McpServerId,
    pub name: String,
    pub source: McpServerSource,
    pub transport: McpTransportConfig,
    pub enabled: bool,
    pub trusted: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when_to_use: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum McpServerSource {
    Runtime,
    User,
    Project,
    Plugin,
    Cli,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum McpTransportConfig {
    Stdio {
        command: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        args: Vec<String>,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        env: BTreeMap<String, String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cwd: Option<String>,
    },
    StreamableHttp {
        endpoint: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        auth_ref: Option<String>,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpToolAnnotations {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read_only_hint: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub destructive_hint: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotent_hint: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open_world_hint: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpToolRef {
    pub server_id: McpServerId,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub input_schema: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<serde_json::Value>,
    #[serde(default)]
    pub annotations: McpToolAnnotations,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpDiagnostic {
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub severity: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum McpServerStatus {
    Enabled,
    Disconnected,
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

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpListParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<ThreadId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpConnectParams {
    pub thread_id: ThreadId,
    pub server_id: McpServerId,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpCallToolParams {
    pub thread_id: ThreadId,
    pub server_id: McpServerId,
    pub tool_name: String,
    #[serde(default)]
    pub arguments: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpCallToolResult {
    pub server_id: McpServerId,
    pub tool_name: String,
    pub status: ToolResultStatus,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub content: Vec<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structured_content: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpResourceRef {
    pub server_id: McpServerId,
    pub uri: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub annotations: BTreeMap<String, serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpListResourcesParams {
    pub thread_id: ThreadId,
    pub server_id: McpServerId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpListResourcesResult {
    pub server_id: McpServerId,
    pub status: ToolResultStatus,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resources: Vec<McpResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpResourceTemplateRef {
    pub server_id: McpServerId,
    pub uri_template: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub annotations: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpListResourceTemplatesParams {
    pub thread_id: ThreadId,
    pub server_id: McpServerId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpListResourceTemplatesResult {
    pub server_id: McpServerId,
    pub status: ToolResultStatus,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resource_templates: Vec<McpResourceTemplateRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpReadResourceParams {
    pub thread_id: ThreadId,
    pub server_id: McpServerId,
    pub uri: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpResourceContent {
    pub uri: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blob: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpReadResourceResult {
    pub server_id: McpServerId,
    pub uri: String,
    pub status: ToolResultStatus,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub contents: Vec<McpResourceContent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpResourceSubscriptionParams {
    pub thread_id: ThreadId,
    pub server_id: McpServerId,
    pub uri: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpResourceSubscriptionResult {
    pub server_id: McpServerId,
    pub uri: String,
    pub status: ToolResultStatus,
    pub subscribed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpResourceUpdatedParams {
    pub thread_id: ThreadId,
    pub server_id: McpServerId,
    pub uri: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub received_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpResourceUpdatedResult {
    pub server_id: McpServerId,
    pub uri: String,
    pub status: ToolResultStatus,
    pub updated: bool,
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub update_count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpPromptArgumentRef {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpPromptRef {
    pub server_id: McpServerId,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub arguments: Vec<McpPromptArgumentRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpListPromptsParams {
    pub thread_id: ThreadId,
    pub server_id: McpServerId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpListPromptsResult {
    pub server_id: McpServerId,
    pub status: ToolResultStatus,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prompts: Vec<McpPromptRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpGetPromptParams {
    pub thread_id: ThreadId,
    pub server_id: McpServerId,
    pub name: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub arguments: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpGetPromptResult {
    pub server_id: McpServerId,
    pub name: String,
    pub status: ToolResultStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub messages: Vec<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelRef {
    pub id: ModelId,
    pub provider: String,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelRegistryListParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<ThreadId>,
    pub provider: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_model_id: Option<ModelId>,
    #[serde(default)]
    pub scope: Vec<String>,
    #[serde(default)]
    pub available_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelRegistryListResult {
    pub provider: String,
    #[serde(default)]
    pub scope: Vec<String>,
    pub available_only: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_model: Option<ModelRef>,
    pub items: Vec<ModelRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diagnostic>,
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
    Unavailable,
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
            Self::Unavailable => "unavailable",
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThreadItemFilter {
    Default,
    NoTools,
    UserOnly,
    LabeledOnly,
    All,
}

impl Default for ThreadItemFilter {
    fn default() -> Self {
        Self::Default
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadItemsParams {
    pub thread_id: ThreadId,
    #[serde(default)]
    pub after: EventId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter: Option<ThreadItemFilter>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadItemLabelsUpdateParams {
    pub thread_id: ThreadId,
    pub event_id: EventId,
    #[serde(default)]
    pub labels: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadItemForkParams {
    pub thread_id: ThreadId,
    pub event_id: EventId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadItemEditForkParams {
    pub thread_id: ThreadId,
    pub event_id: EventId,
    pub input: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadItemEditForkResult {
    pub thread: Thread,
    pub source_event_id: EventId,
    pub history_cutoff_event_id: EventId,
    pub input: String,
    pub events: Vec<Event>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadItemEntry {
    pub event_id: EventId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_event_id: Option<EventId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub child_event_ids: Vec<EventId>,
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub completed_at_ms: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<String>,
    pub item: Item,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadItemBranch {
    pub thread_id: ThreadId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_thread_id: Option<ThreadId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fork_event_id: Option<EventId>,
    #[serde(default)]
    pub child_thread_ids: Vec<ThreadId>,
    #[serde(default)]
    pub sibling_thread_ids: Vec<ThreadId>,
    #[serde(default)]
    pub path: Vec<ThreadId>,
    #[serde(default)]
    pub depth: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadItemsResult {
    pub thread_id: ThreadId,
    #[serde(default)]
    pub branch: ThreadItemBranch,
    pub filter: ThreadItemFilter,
    pub after: EventId,
    pub entries: Vec<ThreadItemEntry>,
    pub items: Vec<Item>,
    pub total_count: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadItemLabelsResult {
    pub thread_id: ThreadId,
    pub event_id: EventId,
    pub labels: Vec<String>,
    pub events: Vec<Event>,
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

fn is_zero_u64(value: &u64) -> bool {
    *value == 0
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackgroundAgentRequest {
    pub run_id: AgentRunId,
    pub parent_thread_id: ThreadId,
    pub parent_turn_id: TurnId,
    pub child_thread_id: ThreadId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub child_turn_id: Option<TurnId>,
    pub execution_root: String,
    pub input: AgenticTurnParams,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_definitions: Vec<ToolDefinition>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRunPollParams {
    pub thread_id: ThreadId,
    pub run_id: AgentRunId,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRunResumeParams {
    pub thread_id: ThreadId,
    pub run_id: AgentRunId,
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
    pub approved_tool_call_ids: Vec<ToolCallId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancellation: Option<AgenticCancellation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_continuations: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRunPollResult {
    pub run: AgentRun,
    pub events: Vec<Event>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FailedAgentFile {
    pub path: String,
    pub source: AgentSource,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentListParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<ThreadId>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentListResult {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<ResolvedAgent>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub failed: Vec<FailedAgentFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentShowParams {
    pub thread_id: ThreadId,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentWriteParams {
    pub thread_id: ThreadId,
    pub source: AgentSource,
    pub definition: AgentDefinition,
    #[serde(default)]
    pub overwrite: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentDeleteParams {
    pub thread_id: ThreadId,
    pub source: AgentSource,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillListParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<ThreadId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillExpandParams {
    pub thread_id: ThreadId,
    pub input: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillExpandResult {
    pub input: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill: Option<SkillRef>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptTemplateListParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<ThreadId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptTemplateExpandParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<ThreadId>,
    pub name: String,
    #[serde(default)]
    pub args: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_variant_append: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptTemplateExpandResult {
    pub name: String,
    pub input: String,
    pub template: PromptTemplateRef,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeListParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<ThreadId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeLoadParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<ThreadId>,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeLoadResult {
    pub theme: ThemeRef,
    pub colors: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub export: Option<serde_json::Value>,
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MemoryRecord {
    pub id: String,
    pub content: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub pinned: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub starred: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strength: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retrieval_strength: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retrieval_count: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_retrieved_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub superseded_by: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parents: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryListParams {
    pub thread_id: ThreadId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u64>,
    #[serde(default)]
    pub pinned_only: bool,
    #[serde(default)]
    pub include_superseded: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryJobsListParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<ThreadId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u64>,
    #[serde(default)]
    pub include_finished: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryJobUpdateParams {
    pub thread_id: ThreadId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job_id: Option<MemoryJobId>,
    pub kind: MemoryJobKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage: Option<String>,
    pub status: MemoryJobStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queued_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<String>,
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub record_count: u64,
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub deleted_count: u64,
    #[serde(default, skip_serializing_if = "is_false")]
    pub dry_run: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub backend_applied: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deep_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_used: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryJobCancelParams {
    pub thread_id: ThreadId,
    pub job_id: MemoryJobId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryRecordsSetParams {
    pub thread_id: ThreadId,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub records: Vec<MemoryRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<MemoryStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryRecallParams {
    pub thread_id: ThreadId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_markdown: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub omitted_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub records: Vec<MemoryRecord>,
    #[serde(default)]
    pub backend_queried: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<MemoryStatus>,
    #[serde(default)]
    pub truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryRememberParams {
    pub thread_id: ThreadId,
    pub content: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_session_id: Option<String>,
    #[serde(default)]
    pub backend_applied: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record: Option<MemoryRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<MemoryStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryRecurringTaskCandidate {
    pub task: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rationale: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prompt_hints: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outcomes: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub related_memory_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryRecurringCandidatesParams {
    pub thread_id: ThreadId,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub candidates: Vec<MemoryRecurringTaskCandidate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_project: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recent_user_texts: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recent_assistant_texts: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryRecurringCandidatesResult {
    pub status: MemoryStatus,
    pub available: bool,
    #[serde(default)]
    pub saved_count: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub records: Vec<MemoryRecord>,
    pub message: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub controls: Vec<MemoryControl>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diagnostic>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<Event>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryDistillTurnParams {
    pub thread_id: ThreadId,
    pub user_text: String,
    pub assistant_text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_project: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recent_user_texts: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recent_assistant_texts: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_model: Option<String>,
    pub model_provider: DirectModelProviderConfig,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryDistillTurnResult {
    pub status: MemoryStatus,
    pub available: bool,
    #[serde(default)]
    pub remembered: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record: Option<MemoryRecord>,
    #[serde(default)]
    pub recurring_saved_count: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recurring_records: Vec<MemoryRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_used: Option<String>,
    pub message: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub controls: Vec<MemoryControl>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diagnostic>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<Event>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryListResult {
    pub status: MemoryStatus,
    pub available: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    #[serde(default)]
    pub pinned_only: bool,
    #[serde(default)]
    pub include_superseded: bool,
    pub total_count: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<MemoryRecord>,
    pub message: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub controls: Vec<MemoryControl>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diagnostic>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<Event>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryJobsResult {
    pub status: MemoryStatus,
    pub queue_depth: u64,
    pub total_count: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<MemoryJobRef>,
    pub message: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub controls: Vec<MemoryControl>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diagnostic>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<Event>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryRecallResult {
    pub status: MemoryStatus,
    pub available: bool,
    #[serde(default)]
    pub backend_queried: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_markdown: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub omitted_reason: Option<String>,
    #[serde(default)]
    pub context_bytes: u64,
    #[serde(default)]
    pub truncated: bool,
    pub total_count: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<MemoryRecord>,
    #[serde(default)]
    pub cache_updated: bool,
    pub message: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub controls: Vec<MemoryControl>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diagnostic>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<Event>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryRememberResult {
    pub status: MemoryStatus,
    pub available: bool,
    #[serde(default)]
    pub saved: bool,
    #[serde(default)]
    pub backend_applied: bool,
    #[serde(default)]
    pub cache_updated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record: Option<MemoryRecord>,
    pub message: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub controls: Vec<MemoryControl>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diagnostic>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<Event>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryConsolidateParams {
    pub thread_id: ThreadId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_used: Option<String>,
    #[serde(default)]
    pub dry_run: bool,
    #[serde(default)]
    pub backend_applied: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub records: Vec<MemoryRecord>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deleted_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<MemoryStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryConsolidateResult {
    pub status: MemoryStatus,
    pub available: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_used: Option<String>,
    #[serde(default)]
    pub dry_run: bool,
    #[serde(default)]
    pub backend_applied: bool,
    #[serde(default)]
    pub cache_updated: bool,
    #[serde(default)]
    pub created_count: u64,
    #[serde(default)]
    pub deleted_count: u64,
    pub total_count: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<MemoryRecord>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deleted_ids: Vec<String>,
    pub message: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub controls: Vec<MemoryControl>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diagnostic>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<Event>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryRecordForgetParams {
    pub thread_id: ThreadId,
    pub id: String,
    #[serde(default)]
    pub backend_applied: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<MemoryStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryRecordPinParams {
    pub thread_id: ThreadId,
    pub id: String,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub backend_applied: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record: Option<MemoryRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<MemoryStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MemoryRecordPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pinned: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub starred: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub superseded_by: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryRecordUpdateParams {
    pub thread_id: ThreadId,
    pub id: String,
    #[serde(default)]
    pub patch: MemoryRecordPatch,
    #[serde(default)]
    pub backend_applied: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record: Option<MemoryRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<MemoryStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryRecordSupersedeParams {
    pub thread_id: ThreadId,
    pub id: String,
    pub superseded_by: String,
    #[serde(default)]
    pub backend_applied: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record: Option<MemoryRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<MemoryStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryRecordMutationResult {
    pub status: MemoryStatus,
    pub action: String,
    pub id: String,
    #[serde(default)]
    pub backend_applied: bool,
    #[serde(default)]
    pub cache_updated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record: Option<MemoryRecord>,
    pub message: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub controls: Vec<MemoryControl>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diagnostic>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<Event>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MemoryBackendOperation {
    Status,
    Recall,
    Remember,
    List,
    Dashboard,
    Forget,
    Pin,
    Update,
    Supersede,
    Consolidate,
    Sync,
}

impl MemoryBackendOperation {
    pub const ALL: [Self; 11] = [
        Self::Status,
        Self::Recall,
        Self::Remember,
        Self::List,
        Self::Dashboard,
        Self::Forget,
        Self::Pin,
        Self::Update,
        Self::Supersede,
        Self::Consolidate,
        Self::Sync,
    ];
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryBackendAdapterInfo {
    pub id: String,
    pub kind: String,
    #[serde(default)]
    pub embedded: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub operations: Vec<MemoryBackendOperation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MemoryBackendSyncDirection {
    Pull,
    Push,
    Both,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryBackendStatusParams {
    pub thread_id: ThreadId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryBackendRecallParams {
    pub thread_id: ThreadId,
    pub prompt: String,
    pub include_startup: bool,
    pub include_task: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryBackendRememberParams {
    pub thread_id: ThreadId,
    pub content: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryBackendListParams {
    pub thread_id: ThreadId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u64>,
    #[serde(default)]
    pub pinned_only: bool,
    #[serde(default)]
    pub include_superseded: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryBackendDashboardParams {
    pub thread_id: ThreadId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryBackendForgetParams {
    pub thread_id: ThreadId,
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryBackendPinParams {
    pub thread_id: ThreadId,
    pub id: String,
    #[serde(default)]
    pub pinned: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryBackendUpdateParams {
    pub thread_id: ThreadId,
    pub id: String,
    #[serde(default)]
    pub patch: MemoryRecordPatch,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryBackendSupersedeParams {
    pub thread_id: ThreadId,
    pub id: String,
    pub superseded_by: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryBackendConsolidateParams {
    pub thread_id: ThreadId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage: Option<String>,
    #[serde(default)]
    pub dry_run: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deep_model: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryBackendSyncParams {
    pub thread_id: ThreadId,
    pub direction: MemoryBackendSyncDirection,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conflict_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub passphrase: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryBackendExecutionResult {
    pub job_id: MemoryJobId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    pub operation: MemoryBackendOperation,
    pub normalized: serde_json::Value,
    pub backend: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adapter_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default)]
    pub timed_out: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    #[serde(default)]
    pub cancelled: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diagnostic>,
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
pub struct MemoryDashboardParams {
    pub thread_id: ThreadId,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryDashboardResult {
    pub status: MemoryStatus,
    pub available: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_url: Option<String>,
    pub message: String,
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SideQuestionParams {
    pub thread_id: ThreadId,
    pub question: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_turn_id: Option<TurnId>,
    #[serde(default, skip_serializing_if = "ExecutionMode::is_blocking")]
    pub execution_mode: ExecutionMode,
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
pub struct CommandPrepareParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<ThreadId>,
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
    pub parent_thread_id: ThreadId,
    pub side_thread_id: ThreadId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side_turn_id: Option<TurnId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<TurnStatus>,
    pub events: Vec<Event>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[cfg(test)]
    impl AgentDefinition {
        fn minimal_for_test(
            name: &str,
            source: AgentSource,
            description: &str,
            instructions: &str,
        ) -> Self {
            Self {
                name: name.to_string(),
                description: description.to_string(),
                source: Some(source),
                tools: Vec::new(),
                tool_denylist: Vec::new(),
                model: None,
                effort: None,
                permission_mode: None,
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
            }
        }
    }

    #[test]
    fn serializes_golden_turn_event() {
        let event = Event {
            id: 7,
            thread_id: "thread-1".to_string(),
            turn_id: Some("turn-1".to_string()),
            created_at_ms: 1_700_000_000_000,
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
                "createdAtMs": 1_700_000_000_000u64,
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
    fn memory_status_protocol_preserves_hoppi_backend_fields() {
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
            "dashboardUrl": "http://hoppi.localhost:49152",
            "executionOwner": "runtime",
            "executionAdapter": "hoppi-node-bridge",
            "executionAdapterKind": "node-typescript"
        }))
        .unwrap();

        let actual = serde_json::to_value(status).unwrap();
        assert_eq!(actual["enabled"], json!(true));
        assert_eq!(actual["initialized"], json!(true));
        assert_eq!(actual["backend"], json!("hoppi"));
        assert_eq!(actual["scope"], json!("project"));
        assert_eq!(actual["storePath"], json!("C:/Users/matth/.oppi/hoppi"));
        assert_eq!(actual["projectId"], json!("oppi-native"));
        assert_eq!(actual["memoryCount"], json!(12));
        assert_eq!(actual["pinnedCount"], json!(3));
        assert_eq!(actual["lastWriteAt"], json!("2026-05-29T12:00:00Z"));
        assert_eq!(actual["lastConsolidatedAt"], json!("2026-05-29T11:00:00Z"));
        assert_eq!(actual["queueDepth"], json!(2));
        assert_eq!(
            actual["dashboardUrl"],
            json!("http://hoppi.localhost:49152")
        );
        assert_eq!(actual["executionOwner"], json!("runtime"));
        assert_eq!(actual["executionAdapter"], json!("hoppi-node-bridge"));
        assert_eq!(actual["executionAdapterKind"], json!("node-typescript"));
    }

    #[test]
    fn memory_job_protocol_reports_queue_visibility_contracts() {
        let job = MemoryJobRef {
            id: "mem-job-1".to_string(),
            thread_id: "thread-1".to_string(),
            kind: MemoryJobKind::Consolidate,
            stage: Some("idle-dream".to_string()),
            status: MemoryJobStatus::Running,
            source: Some("native-shell".to_string()),
            message: Some("Running idle Hoppi consolidation.".to_string()),
            queued_at: Some("2026-05-31T12:00:00Z".to_string()),
            started_at: Some("2026-05-31T12:00:01Z".to_string()),
            finished_at: None,
            record_count: 2,
            deleted_count: 1,
            dry_run: false,
            backend_applied: false,
            agent_model: Some("claude".to_string()),
            deep_model: Some("gpt-5.5".to_string()),
            model_used: Some("gpt-5.5".to_string()),
        };

        let update = MemoryJobUpdateParams {
            thread_id: "thread-1".to_string(),
            job_id: Some(job.id.clone()),
            kind: job.kind,
            stage: job.stage.clone(),
            status: job.status,
            source: job.source.clone(),
            message: job.message.clone(),
            queued_at: job.queued_at.clone(),
            started_at: job.started_at.clone(),
            finished_at: None,
            record_count: job.record_count,
            deleted_count: job.deleted_count,
            dry_run: job.dry_run,
            backend_applied: job.backend_applied,
            agent_model: job.agent_model.clone(),
            deep_model: job.deep_model.clone(),
            model_used: job.model_used.clone(),
        };
        let result = MemoryJobsResult {
            status: MemoryStatus {
                enabled: true,
                initialized: true,
                backend: "hoppi".to_string(),
                scope: "project".to_string(),
                queue_depth: Some(1),
                ..MemoryStatus::default()
            },
            queue_depth: 1,
            total_count: 1,
            items: vec![job],
            message: "Runtime memory worker has 1 active job.".to_string(),
            controls: vec![MemoryControl {
                id: "memory-status".to_string(),
                label: "Memory status".to_string(),
                command: "/memory status".to_string(),
                description: Some("Show current Hoppi memory status.".to_string()),
            }],
            diagnostics: Vec::new(),
            events: Vec::new(),
        };

        let update_json = serde_json::to_value(update).unwrap();
        assert_eq!(update_json["threadId"], json!("thread-1"));
        assert_eq!(update_json["jobId"], json!("mem-job-1"));
        assert_eq!(update_json["kind"], json!("consolidate"));
        assert_eq!(update_json["status"], json!("running"));
        assert_eq!(update_json["stage"], json!("idle-dream"));
        assert_eq!(update_json["agentModel"], json!("claude"));
        assert_eq!(update_json["deepModel"], json!("gpt-5.5"));
        assert_eq!(update_json["modelUsed"], json!("gpt-5.5"));

        let cancel = MemoryJobCancelParams {
            thread_id: "thread-1".to_string(),
            job_id: "mem-job-1".to_string(),
            reason: Some("user requested cancellation".to_string()),
            finished_at: Some("2026-05-31T12:00:05Z".to_string()),
        };
        let cancel_json = serde_json::to_value(cancel).unwrap();
        assert_eq!(cancel_json["threadId"], json!("thread-1"));
        assert_eq!(cancel_json["jobId"], json!("mem-job-1"));
        assert_eq!(cancel_json["reason"], json!("user requested cancellation"));
        assert_eq!(cancel_json["finishedAt"], json!("2026-05-31T12:00:05Z"));

        let result_json = serde_json::to_value(result).unwrap();
        assert_eq!(result_json["queueDepth"], json!(1));
        assert_eq!(result_json["status"]["queueDepth"], json!(1));
        assert_eq!(result_json["items"][0]["id"], json!("mem-job-1"));
        assert_eq!(result_json["items"][0]["status"], json!("running"));
        assert_eq!(result_json["items"][0]["kind"], json!("consolidate"));
    }

    #[test]
    fn memory_backend_recall_params_serialize_thread_prompt_and_flags() {
        let params = MemoryBackendRecallParams {
            thread_id: "thread-1".to_string(),
            prompt: "finish Rust-owned memory worker".to_string(),
            include_startup: true,
            include_task: true,
        };

        let value = serde_json::to_value(&params).unwrap();

        assert_eq!(value["threadId"], json!("thread-1"));
        assert_eq!(value["prompt"], json!("finish Rust-owned memory worker"));
        assert_eq!(value["includeStartup"], json!(true));
        assert_eq!(value["includeTask"], json!(true));
    }

    #[test]
    fn memory_backend_job_result_preserves_normalized_payload_and_job_metadata() {
        let result = MemoryBackendExecutionResult {
            job_id: "native-recall-job-1".to_string(),
            request_id: Some("rpc-42".to_string()),
            operation: MemoryBackendOperation::Recall,
            normalized: serde_json::json!({
                "contextMarkdown": "Remember current native memory ownership.",
                "records": []
            }),
            backend: "hoppi-node-bridge".to_string(),
            adapter_kind: Some("node-typescript".to_string()),
            summary: Some("Recalled 0 Hoppi records for provider context.".to_string()),
            timed_out: false,
            timeout_ms: Some(120_000),
            cancelled: false,
            diagnostics: Vec::new(),
        };

        let value = serde_json::to_value(&result).unwrap();

        assert_eq!(value["jobId"], json!("native-recall-job-1"));
        assert_eq!(value["requestId"], json!("rpc-42"));
        assert_eq!(value["operation"], json!("recall"));
        assert_eq!(value["backend"], json!("hoppi-node-bridge"));
        assert_eq!(value["adapterKind"], json!("node-typescript"));
        assert_eq!(value["timedOut"], json!(false));
        assert_eq!(value["timeoutMs"], json!(120000));
        assert_eq!(value["cancelled"], json!(false));
        assert_eq!(
            value["normalized"]["contextMarkdown"],
            json!("Remember current native memory ownership.")
        );
    }

    #[test]
    fn memory_backend_adapter_info_serializes_kind_and_operations() {
        let info = MemoryBackendAdapterInfo {
            id: "hoppi-node-bridge".to_string(),
            kind: "node-typescript".to_string(),
            embedded: false,
            operations: vec![
                MemoryBackendOperation::Status,
                MemoryBackendOperation::Recall,
                MemoryBackendOperation::Remember,
            ],
            diagnostics: Vec::new(),
        };

        let value = serde_json::to_value(info).unwrap();

        assert_eq!(value["id"], json!("hoppi-node-bridge"));
        assert_eq!(value["kind"], json!("node-typescript"));
        assert_eq!(value["embedded"], json!(false));
        assert_eq!(value["operations"][0], json!("status"));
    }

    #[test]
    fn memory_backend_params_cover_route_catalog_without_json_passthrough() {
        let status = MemoryBackendStatusParams {
            thread_id: "thread-1".to_string(),
        };
        let remember = MemoryBackendRememberParams {
            thread_id: "thread-1".to_string(),
            content: "Save a native memory summary.".to_string(),
            tags: vec!["oppi-native-shell".to_string()],
            source: Some("oppi:native-shell".to_string()),
            source_session_id: Some("thread-1:turn-9".to_string()),
            layer: Some("semantic".to_string()),
        };
        let list = MemoryBackendListParams {
            thread_id: "thread-1".to_string(),
            query: Some("native memory".to_string()),
            limit: Some(25),
            pinned_only: true,
            include_superseded: true,
        };
        let dashboard = MemoryBackendDashboardParams {
            thread_id: "thread-1".to_string(),
        };
        let forget = MemoryBackendForgetParams {
            thread_id: "thread-1".to_string(),
            id: "mem-old".to_string(),
        };
        let pin = MemoryBackendPinParams {
            thread_id: "thread-1".to_string(),
            id: "mem-important".to_string(),
            pinned: true,
        };
        let update = MemoryBackendUpdateParams {
            thread_id: "thread-1".to_string(),
            id: "mem-edit".to_string(),
            patch: MemoryRecordPatch {
                content: Some("Edited through runtime-owned backend route.".to_string()),
                tags: Some(vec!["runtime-owned".to_string()]),
                ..MemoryRecordPatch::default()
            },
        };
        let supersede = MemoryBackendSupersedeParams {
            thread_id: "thread-1".to_string(),
            id: "mem-old".to_string(),
            superseded_by: "mem-new".to_string(),
        };
        let consolidate = MemoryBackendConsolidateParams {
            thread_id: "thread-1".to_string(),
            stage: Some("idle-dream".to_string()),
            dry_run: false,
            limit: Some(50),
            budget: Some(1500),
            agent_model: Some("auto".to_string()),
            deep_model: Some("sonnet".to_string()),
        };
        let sync = MemoryBackendSyncParams {
            thread_id: "thread-1".to_string(),
            direction: MemoryBackendSyncDirection::Pull,
            repo_path: Some("E:/RepositoryV2/oppi/.hoppi-sync".to_string()),
            remote: Some("git@example.com:memories.git".to_string()),
            conflict_mode: Some("keep-local".to_string()),
            passphrase: Some("runtime-passphrase".to_string()),
            message: Some("Hoppi memory sync (startup)".to_string()),
        };

        assert_eq!(
            serde_json::to_value(status).unwrap()["threadId"],
            json!("thread-1")
        );
        assert_eq!(
            serde_json::to_value(remember).unwrap()["sourceSessionId"],
            json!("thread-1:turn-9")
        );
        assert_eq!(
            serde_json::to_value(list).unwrap()["pinnedOnly"],
            json!(true)
        );
        assert_eq!(
            serde_json::to_value(dashboard).unwrap()["threadId"],
            json!("thread-1")
        );
        assert_eq!(
            serde_json::to_value(forget).unwrap()["id"],
            json!("mem-old")
        );
        assert_eq!(serde_json::to_value(pin).unwrap()["pinned"], json!(true));
        assert_eq!(
            serde_json::to_value(update).unwrap()["patch"]["tags"],
            json!(["runtime-owned"])
        );
        assert_eq!(
            serde_json::to_value(supersede).unwrap()["supersededBy"],
            json!("mem-new")
        );
        assert_eq!(
            serde_json::to_value(consolidate).unwrap()["agentModel"],
            json!("auto")
        );
        assert_eq!(
            serde_json::to_value(sync).unwrap()["direction"],
            json!("pull")
        );
        let sync = serde_json::to_value(MemoryBackendSyncParams {
            thread_id: "thread-1".to_string(),
            direction: MemoryBackendSyncDirection::Push,
            repo_path: Some("E:/RepositoryV2/oppi/.hoppi-sync".to_string()),
            remote: Some("git@example.com:memories.git".to_string()),
            conflict_mode: Some("keep-local".to_string()),
            passphrase: Some("runtime-passphrase".to_string()),
            message: Some("Hoppi memory sync (exit)".to_string()),
        })
        .unwrap();
        assert_eq!(sync["repoPath"], json!("E:/RepositoryV2/oppi/.hoppi-sync"));
        assert_eq!(sync["remote"], json!("git@example.com:memories.git"));
        assert_eq!(sync["conflictMode"], json!("keep-local"));
        assert_eq!(sync["passphrase"], json!("runtime-passphrase"));
        assert_eq!(sync["message"], json!("Hoppi memory sync (exit)"));
    }

    #[test]
    fn memory_dashboard_protocol_reports_runtime_dashboard_state() {
        let result = MemoryDashboardResult {
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
            available: true,
            url: Some("http://hoppi.localhost:49152".to_string()),
            fallback_url: Some("http://localhost:49152".to_string()),
            message: "Hoppi dashboard is available from runtime memory status.".to_string(),
            controls: vec![MemoryControl {
                id: "open-dashboard".to_string(),
                label: "Open dashboard".to_string(),
                command: "/memory dashboard".to_string(),
                description: Some("Open the current Hoppi dashboard URL.".to_string()),
            }],
            diagnostics: Vec::new(),
            events: Vec::new(),
        };

        let actual = serde_json::to_value(result).unwrap();
        assert_eq!(actual["available"], json!(true));
        assert_eq!(actual["url"], json!("http://hoppi.localhost:49152"));
        assert_eq!(actual["fallbackUrl"], json!("http://localhost:49152"));
        assert_eq!(
            actual["status"]["dashboardUrl"],
            json!("http://hoppi.localhost:49152")
        );
        assert_eq!(actual["status"]["pinnedCount"], json!(3));
        assert_eq!(actual["controls"][0]["command"], json!("/memory dashboard"));
    }

    #[test]
    fn memory_list_protocol_reports_runtime_cached_hoppi_records() {
        let record = MemoryRecord {
            id: "mem-1".to_string(),
            content: "Native OPPi owns the memory list contract.".to_string(),
            tags: vec![
                "oppi-native-shell".to_string(),
                "oppi-turn-summary".to_string(),
            ],
            pinned: true,
            starred: false,
            layer: Some("semantic".to_string()),
            confidence: Some("verified".to_string()),
            source: Some("oppi:native-shell".to_string()),
            created_at: Some("2026-05-29T12:00:00Z".to_string()),
            updated_at: Some("2026-05-29T12:30:00Z".to_string()),
            strength: Some(0.75),
            retrieval_strength: Some(0.9),
            retrieval_count: Some(4),
            last_retrieved_at: Some("2026-05-29T13:00:00Z".to_string()),
            superseded_by: None,
            parents: vec!["mem-parent".to_string()],
        };
        let result = MemoryListResult {
            status: MemoryStatus {
                enabled: true,
                initialized: true,
                backend: "hoppi".to_string(),
                scope: "project".to_string(),
                memory_count: 1,
                pinned_count: 1,
                ..MemoryStatus::default()
            },
            available: true,
            query: Some("native".to_string()),
            pinned_only: true,
            include_superseded: false,
            total_count: 1,
            items: vec![record.clone()],
            message: "Runtime memory list contains 1 cached record.".to_string(),
            controls: vec![MemoryControl {
                id: "open-dashboard".to_string(),
                label: "Open dashboard".to_string(),
                command: "/memory dashboard".to_string(),
                description: None,
            }],
            diagnostics: Vec::new(),
            events: Vec::new(),
        };
        let set = MemoryRecordsSetParams {
            thread_id: "thread-1".to_string(),
            records: vec![record],
            status: Some(result.status.clone()),
            source: Some("hoppi-list".to_string()),
        };

        let actual = serde_json::to_value(result).unwrap();
        assert_eq!(actual["available"], json!(true));
        assert_eq!(actual["totalCount"], json!(1));
        assert_eq!(actual["items"][0]["id"], json!("mem-1"));
        assert_eq!(actual["items"][0]["pinned"], json!(true));
        assert_eq!(actual["items"][0]["retrievalStrength"], json!(0.9));
        assert_eq!(actual["items"][0]["parents"][0], json!("mem-parent"));
        let set_json = serde_json::to_value(set).unwrap();
        assert_eq!(
            set_json["records"][0]["createdAt"],
            json!("2026-05-29T12:00:00Z")
        );
        assert_eq!(set_json["source"], json!("hoppi-list"));
    }

    #[test]
    fn memory_recall_protocol_reports_context_and_cached_hoppi_records() {
        let record = MemoryRecord {
            id: "mem-recall".to_string(),
            content: "Recall context should be normalized through Rust.".to_string(),
            tags: vec!["oppi-native-shell".to_string()],
            pinned: true,
            retrieval_strength: Some(0.88),
            retrieval_count: Some(2),
            source: Some("oppi:native-shell".to_string()),
            ..MemoryRecord::default()
        };
        let params = MemoryRecallParams {
            thread_id: "thread-1".to_string(),
            query: Some("ship recall boundary".to_string()),
            context_markdown: Some("Memory block for the provider".to_string()),
            omitted_reason: None,
            records: vec![record.clone()],
            backend_queried: true,
            status: Some(MemoryStatus {
                enabled: true,
                initialized: true,
                backend: "hoppi".to_string(),
                scope: "project".to_string(),
                memory_count: 4,
                pinned_count: 1,
                ..MemoryStatus::default()
            }),
            truncated: false,
            source: Some("hoppi-recall".to_string()),
        };
        let result = MemoryRecallResult {
            status: params.status.clone().unwrap(),
            available: true,
            backend_queried: true,
            query: params.query.clone(),
            context_markdown: params.context_markdown.clone(),
            omitted_reason: None,
            context_bytes: 29,
            truncated: false,
            total_count: 1,
            items: vec![record],
            cache_updated: true,
            message: "Recalled Hoppi context for this turn.".to_string(),
            controls: vec![MemoryControl {
                id: "refresh-list".to_string(),
                label: "Refresh list".to_string(),
                command: "/memory list".to_string(),
                description: None,
            }],
            diagnostics: Vec::new(),
            events: Vec::new(),
        };

        let params_json = serde_json::to_value(params).unwrap();
        assert_eq!(params_json["backendQueried"], json!(true));
        assert_eq!(
            params_json["contextMarkdown"],
            json!("Memory block for the provider")
        );
        assert_eq!(params_json["records"][0]["id"], json!("mem-recall"));
        assert_eq!(params_json["records"][0]["retrievalStrength"], json!(0.88));
        let result_json = serde_json::to_value(result).unwrap();
        assert_eq!(result_json["available"], json!(true));
        assert_eq!(result_json["backendQueried"], json!(true));
        assert_eq!(result_json["cacheUpdated"], json!(true));
        assert_eq!(result_json["totalCount"], json!(1));
        assert_eq!(
            result_json["contextMarkdown"],
            json!("Memory block for the provider")
        );
        assert_eq!(result_json["items"][0]["id"], json!("mem-recall"));
        assert_eq!(result_json["status"]["memoryCount"], json!(4));
    }

    #[test]
    fn memory_remember_protocol_reports_backend_write_and_cached_record() {
        let record = MemoryRecord {
            id: "mem-remember".to_string(),
            content: "Remember writes should be normalized through Rust.".to_string(),
            tags: vec![
                "oppi-native-shell".to_string(),
                "oppi-turn-summary".to_string(),
            ],
            layer: Some("buffer".to_string()),
            confidence: Some("observed".to_string()),
            source: Some("oppi:native-shell".to_string()),
            ..MemoryRecord::default()
        };
        let params = MemoryRememberParams {
            thread_id: "thread-1".to_string(),
            content: record.content.clone(),
            tags: record.tags.clone(),
            layer: record.layer.clone(),
            confidence: record.confidence.clone(),
            source_session_id: Some("thread-1:turn-1".to_string()),
            backend_applied: true,
            record: Some(record.clone()),
            status: Some(MemoryStatus {
                enabled: true,
                initialized: true,
                backend: "hoppi".to_string(),
                scope: "project".to_string(),
                memory_count: 7,
                pinned_count: 2,
                ..MemoryStatus::default()
            }),
            reason: Some("turn-summary".to_string()),
            source: Some("hoppi-remember".to_string()),
        };
        let result = MemoryRememberResult {
            status: params.status.clone().unwrap(),
            available: true,
            saved: true,
            backend_applied: true,
            cache_updated: true,
            record: Some(record),
            message: "Saved Hoppi memory record mem-remember and updated Rust cache.".to_string(),
            controls: vec![MemoryControl {
                id: "refresh-list".to_string(),
                label: "Refresh list".to_string(),
                command: "/memory list".to_string(),
                description: None,
            }],
            diagnostics: Vec::new(),
            events: Vec::new(),
        };

        let params_json = serde_json::to_value(params).unwrap();
        assert_eq!(params_json["backendApplied"], json!(true));
        assert_eq!(params_json["record"]["id"], json!("mem-remember"));
        assert_eq!(params_json["sourceSessionId"], json!("thread-1:turn-1"));
        assert_eq!(params_json["reason"], json!("turn-summary"));
        let result_json = serde_json::to_value(result).unwrap();
        assert_eq!(result_json["saved"], json!(true));
        assert_eq!(result_json["backendApplied"], json!(true));
        assert_eq!(result_json["cacheUpdated"], json!(true));
        assert_eq!(result_json["record"]["id"], json!("mem-remember"));
        assert_eq!(result_json["status"]["memoryCount"], json!(7));
    }

    #[test]
    fn memory_recurring_candidates_protocol_reports_system_scope_records() {
        let candidate = MemoryRecurringTaskCandidate {
            task: "Audit native memory parity after each backend slice".to_string(),
            rationale: Some(
                "Repeated plan-alignment work should become a reusable skill.".to_string(),
            ),
            prompt_hints: vec!["continue with the plan".to_string()],
            outcomes: vec!["updated Plan 61 with verification evidence".to_string()],
            related_memory_ids: vec!["mem-1".to_string()],
            tags: vec!["plan75".to_string()],
        };
        let params = MemoryRecurringCandidatesParams {
            thread_id: "thread-1".to_string(),
            candidates: vec![candidate],
            source_project: Some("E:/RepositoryV2/oppi".to_string()),
            source_session_id: Some("session-1".to_string()),
            recent_user_texts: vec!["continue with the plan".to_string()],
            recent_assistant_texts: vec!["Plan 75 evidence updated.".to_string()],
        };
        let record = MemoryRecord {
            id: "recurring-task-audit-native-memory-parity".to_string(),
            content: "Recurring task candidate\nTask: Audit native memory parity after each backend slice".to_string(),
            tags: vec![
                "oppi-recurring-task-candidate".to_string(),
                "system-scope".to_string(),
                "skill-candidate".to_string(),
                "plan75".to_string(),
            ],
            source: Some("oppi:recurring-task-mining".to_string()),
            layer: Some("semantic".to_string()),
            confidence: Some("observed".to_string()),
            ..MemoryRecord::default()
        };
        let result = MemoryRecurringCandidatesResult {
            status: MemoryStatus {
                enabled: true,
                initialized: true,
                backend: "runtime".to_string(),
                scope: "system".to_string(),
                memory_count: 1,
                ..MemoryStatus::default()
            },
            available: true,
            saved_count: 1,
            records: vec![record],
            message: "Recorded 1 recurring task candidate.".to_string(),
            controls: Vec::new(),
            diagnostics: Vec::new(),
            events: Vec::new(),
        };

        let params_json = serde_json::to_value(params).unwrap();
        assert_eq!(
            params_json["candidates"][0]["promptHints"][0],
            json!("continue with the plan")
        );
        assert_eq!(params_json["sourceSessionId"], json!("session-1"));
        assert_eq!(
            params_json["recentUserTexts"][0],
            json!("continue with the plan")
        );
        let result_json = serde_json::to_value(result).unwrap();
        assert_eq!(result_json["savedCount"], json!(1));
        assert_eq!(
            result_json["records"][0]["tags"][0],
            json!("oppi-recurring-task-candidate")
        );
        assert_eq!(
            result_json["records"][0]["source"],
            json!("oppi:recurring-task-mining")
        );
    }

    #[test]
    fn memory_distill_turn_protocol_reports_model_and_saved_candidates() {
        let params = MemoryDistillTurnParams {
            thread_id: "thread-1".to_string(),
            user_text: "continue with the plan".to_string(),
            assistant_text: "Added a runtime memory route and tests.".to_string(),
            source_project: Some("E:/RepositoryV2/oppi".to_string()),
            source_session_id: Some("session-1".to_string()),
            recent_user_texts: vec!["continue with the plan".to_string()],
            recent_assistant_texts: vec!["Plan 75 evidence updated.".to_string()],
            agent_model: Some("gpt".to_string()),
            model_provider: DirectModelProviderConfig {
                kind: DirectModelProviderKind::OpenAiCompatible,
                model: "gpt-5.4-mini".to_string(),
                base_url: None,
                api_key_env: None,
                system_prompt: None,
                temperature: None,
                reasoning_effort: None,
                max_output_tokens: None,
                stream: false,
            },
        };
        let record = MemoryRecord {
            id: "turn-memory-continue-with-the-plan".to_string(),
            content: "Turn memory\nRequest: continue with the plan".to_string(),
            tags: vec![
                "oppi-turn-summary".to_string(),
                "agent_end".to_string(),
                "plan75".to_string(),
            ],
            source: Some("oppi:turn-distillation".to_string()),
            layer: Some("semantic".to_string()),
            confidence: Some("observed".to_string()),
            ..MemoryRecord::default()
        };
        let candidate = MemoryRecord {
            id: "recurring-task-audit-native-memory-parity".to_string(),
            content: "Recurring task candidate\nTask: Audit native memory parity".to_string(),
            source: Some("oppi:recurring-task-mining".to_string()),
            ..MemoryRecord::default()
        };
        let result = MemoryDistillTurnResult {
            status: MemoryStatus {
                enabled: true,
                initialized: true,
                backend: "runtime".to_string(),
                scope: "system".to_string(),
                memory_count: 2,
                ..MemoryStatus::default()
            },
            available: true,
            remembered: true,
            record: Some(record),
            recurring_saved_count: 1,
            recurring_records: vec![candidate],
            model_used: Some("gpt-5.4-mini".to_string()),
            message: "Distilled turn memory and recorded 1 recurring task candidate.".to_string(),
            controls: Vec::new(),
            diagnostics: Vec::new(),
            events: Vec::new(),
        };

        let params_json = serde_json::to_value(params).unwrap();
        assert_eq!(params_json["modelProvider"]["model"], json!("gpt-5.4-mini"));
        assert_eq!(params_json["agentModel"], json!("gpt"));
        assert_eq!(params_json["sourceSessionId"], json!("session-1"));
        let result_json = serde_json::to_value(result).unwrap();
        assert_eq!(result_json["remembered"], json!(true));
        assert_eq!(result_json["recurringSavedCount"], json!(1));
        assert_eq!(result_json["modelUsed"], json!("gpt-5.4-mini"));
        assert_eq!(
            result_json["recurringRecords"][0]["source"],
            json!("oppi:recurring-task-mining")
        );
    }

    #[test]
    fn memory_forget_protocol_reports_backend_and_cache_mutation() {
        let record = MemoryRecord {
            id: "mem-1".to_string(),
            content: "Forget this cached Hoppi memory.".to_string(),
            pinned: true,
            source: Some("oppi:native-shell".to_string()),
            ..MemoryRecord::default()
        };
        let result = MemoryRecordMutationResult {
            status: MemoryStatus {
                enabled: true,
                initialized: true,
                backend: "hoppi".to_string(),
                scope: "project".to_string(),
                memory_count: 1,
                pinned_count: 0,
                ..MemoryStatus::default()
            },
            action: "forget".to_string(),
            id: "mem-1".to_string(),
            backend_applied: true,
            cache_updated: true,
            record: Some(record.clone()),
            message: "Forgot memory record mem-1 and removed it from Rust cache.".to_string(),
            controls: vec![MemoryControl {
                id: "refresh-list".to_string(),
                label: "Refresh list".to_string(),
                command: "/memory list".to_string(),
                description: None,
            }],
            diagnostics: Vec::new(),
            events: Vec::new(),
        };
        let params = MemoryRecordForgetParams {
            thread_id: "thread-1".to_string(),
            id: "mem-1".to_string(),
            backend_applied: true,
            status: Some(result.status.clone()),
            source: Some("hoppi-forget".to_string()),
        };

        let actual = serde_json::to_value(result).unwrap();
        assert_eq!(actual["action"], json!("forget"));
        assert_eq!(actual["id"], json!("mem-1"));
        assert_eq!(actual["backendApplied"], json!(true));
        assert_eq!(actual["cacheUpdated"], json!(true));
        assert_eq!(actual["record"]["id"], json!("mem-1"));
        assert_eq!(actual["status"]["memoryCount"], json!(1));
        let params_json = serde_json::to_value(params).unwrap();
        assert_eq!(params_json["backendApplied"], json!(true));
        assert_eq!(params_json["source"], json!("hoppi-forget"));
    }

    #[test]
    fn memory_pin_protocol_reports_backend_and_cache_mutation() {
        let record = MemoryRecord {
            id: "mem-1".to_string(),
            content: "Pin this cached Hoppi memory.".to_string(),
            pinned: true,
            source: Some("oppi:native-shell".to_string()),
            ..MemoryRecord::default()
        };
        let result = MemoryRecordMutationResult {
            status: MemoryStatus {
                enabled: true,
                initialized: true,
                backend: "hoppi".to_string(),
                scope: "project".to_string(),
                memory_count: 1,
                pinned_count: 1,
                ..MemoryStatus::default()
            },
            action: "pin".to_string(),
            id: "mem-1".to_string(),
            backend_applied: true,
            cache_updated: true,
            record: Some(record.clone()),
            message: "Pinned memory record mem-1 and updated Rust cache.".to_string(),
            controls: vec![MemoryControl {
                id: "refresh-list".to_string(),
                label: "Refresh list".to_string(),
                command: "/memory list".to_string(),
                description: None,
            }],
            diagnostics: Vec::new(),
            events: Vec::new(),
        };
        let params = MemoryRecordPinParams {
            thread_id: "thread-1".to_string(),
            id: "mem-1".to_string(),
            pinned: true,
            backend_applied: true,
            record: Some(record),
            status: Some(result.status.clone()),
            source: Some("hoppi-pin".to_string()),
        };

        let actual = serde_json::to_value(result).unwrap();
        assert_eq!(actual["action"], json!("pin"));
        assert_eq!(actual["id"], json!("mem-1"));
        assert_eq!(actual["backendApplied"], json!(true));
        assert_eq!(actual["cacheUpdated"], json!(true));
        assert_eq!(actual["record"]["pinned"], json!(true));
        assert_eq!(actual["status"]["pinnedCount"], json!(1));
        let params_json = serde_json::to_value(params).unwrap();
        assert_eq!(params_json["pinned"], json!(true));
        assert_eq!(params_json["record"]["id"], json!("mem-1"));
        assert_eq!(params_json["source"], json!("hoppi-pin"));
    }

    #[test]
    fn memory_update_protocol_reports_backend_and_cache_mutation() {
        let patch = MemoryRecordPatch {
            content: Some("Updated cached Hoppi memory.".to_string()),
            tags: Some(vec!["verified".to_string(), "native".to_string()]),
            confidence: Some("verified".to_string()),
            ..MemoryRecordPatch::default()
        };
        let record = MemoryRecord {
            id: "mem-1".to_string(),
            content: "Updated cached Hoppi memory.".to_string(),
            tags: vec!["verified".to_string(), "native".to_string()],
            confidence: Some("verified".to_string()),
            source: Some("oppi:native-shell".to_string()),
            ..MemoryRecord::default()
        };
        let result = MemoryRecordMutationResult {
            status: MemoryStatus {
                enabled: true,
                initialized: true,
                backend: "hoppi".to_string(),
                scope: "project".to_string(),
                memory_count: 1,
                pinned_count: 0,
                ..MemoryStatus::default()
            },
            action: "update".to_string(),
            id: "mem-1".to_string(),
            backend_applied: true,
            cache_updated: true,
            record: Some(record.clone()),
            message: "Updated memory record mem-1 and updated Rust cache.".to_string(),
            controls: vec![MemoryControl {
                id: "refresh-list".to_string(),
                label: "Refresh list".to_string(),
                command: "/memory list".to_string(),
                description: None,
            }],
            diagnostics: Vec::new(),
            events: Vec::new(),
        };
        let params = MemoryRecordUpdateParams {
            thread_id: "thread-1".to_string(),
            id: "mem-1".to_string(),
            patch,
            backend_applied: true,
            record: Some(record),
            status: Some(result.status.clone()),
            source: Some("hoppi-update".to_string()),
        };

        let actual = serde_json::to_value(result).unwrap();
        assert_eq!(actual["action"], json!("update"));
        assert_eq!(actual["id"], json!("mem-1"));
        assert_eq!(actual["backendApplied"], json!(true));
        assert_eq!(actual["cacheUpdated"], json!(true));
        assert_eq!(
            actual["record"]["content"],
            json!("Updated cached Hoppi memory.")
        );
        assert_eq!(actual["record"]["confidence"], json!("verified"));
        let params_json = serde_json::to_value(params).unwrap();
        assert_eq!(params_json["backendApplied"], json!(true));
        assert_eq!(
            params_json["patch"]["content"],
            json!("Updated cached Hoppi memory.")
        );
        assert_eq!(params_json["patch"]["tags"], json!(["verified", "native"]));
        assert_eq!(params_json["source"], json!("hoppi-update"));
    }

    #[test]
    fn memory_supersede_protocol_reports_backend_and_cache_mutation() {
        let record = MemoryRecord {
            id: "mem-old".to_string(),
            content: "Older cached Hoppi memory.".to_string(),
            superseded_by: Some("mem-new".to_string()),
            source: Some("oppi:native-shell".to_string()),
            ..MemoryRecord::default()
        };
        let result = MemoryRecordMutationResult {
            status: MemoryStatus {
                enabled: true,
                initialized: true,
                backend: "hoppi".to_string(),
                scope: "project".to_string(),
                memory_count: 2,
                pinned_count: 0,
                ..MemoryStatus::default()
            },
            action: "supersede".to_string(),
            id: "mem-old".to_string(),
            backend_applied: true,
            cache_updated: true,
            record: Some(record.clone()),
            message:
                "Marked memory record mem-old as superseded by mem-new and updated Rust cache."
                    .to_string(),
            controls: vec![MemoryControl {
                id: "refresh-list".to_string(),
                label: "Refresh list".to_string(),
                command: "/memory list all".to_string(),
                description: None,
            }],
            diagnostics: Vec::new(),
            events: Vec::new(),
        };
        let params = MemoryRecordSupersedeParams {
            thread_id: "thread-1".to_string(),
            id: "mem-old".to_string(),
            superseded_by: "mem-new".to_string(),
            backend_applied: true,
            record: Some(record),
            status: Some(result.status.clone()),
            source: Some("hoppi-supersede".to_string()),
        };

        let actual = serde_json::to_value(result).unwrap();
        assert_eq!(actual["action"], json!("supersede"));
        assert_eq!(actual["id"], json!("mem-old"));
        assert_eq!(actual["backendApplied"], json!(true));
        assert_eq!(actual["cacheUpdated"], json!(true));
        assert_eq!(actual["record"]["supersededBy"], json!("mem-new"));
        let params_json = serde_json::to_value(params).unwrap();
        assert_eq!(params_json["supersededBy"], json!("mem-new"));
        assert_eq!(params_json["record"]["id"], json!("mem-old"));
        assert_eq!(params_json["source"], json!("hoppi-supersede"));
    }

    #[test]
    fn memory_consolidate_protocol_reports_backend_maintenance_result() {
        let record = MemoryRecord {
            id: "mem-consolidated".to_string(),
            content: "Consolidated native memory result.".to_string(),
            tags: vec!["oppi-native-shell".to_string(), "consolidated".to_string()],
            confidence: Some("verified".to_string()),
            source: Some("hoppi:maintenance".to_string()),
            parents: vec!["mem-noise".to_string(), "mem-duplicate".to_string()],
            ..MemoryRecord::default()
        };
        let status = MemoryStatus {
            enabled: true,
            initialized: true,
            backend: "hoppi".to_string(),
            scope: "project".to_string(),
            memory_count: 1,
            pinned_count: 0,
            last_consolidated_at: Some("2026-05-31T10:00:00Z".to_string()),
            queue_depth: Some(0),
            ..MemoryStatus::default()
        };
        let params = MemoryConsolidateParams {
            thread_id: "thread-1".to_string(),
            stage: Some("idle-dream".to_string()),
            summary: Some("Merged duplicate implementation notes.".to_string()),
            model_used: Some("gpt-5.4-mini".to_string()),
            dry_run: false,
            backend_applied: true,
            records: vec![record.clone()],
            deleted_ids: vec!["mem-noise".to_string(), "mem-duplicate".to_string()],
            status: Some(status.clone()),
            source: Some("hoppi-consolidate".to_string()),
        };
        let result = MemoryConsolidateResult {
            status,
            available: true,
            stage: Some("idle-dream".to_string()),
            summary: Some("Merged duplicate implementation notes.".to_string()),
            model_used: Some("gpt-5.4-mini".to_string()),
            dry_run: false,
            backend_applied: true,
            cache_updated: true,
            created_count: 1,
            deleted_count: 2,
            total_count: 1,
            items: vec![record],
            deleted_ids: vec!["mem-noise".to_string(), "mem-duplicate".to_string()],
            message: "Applied Hoppi consolidation and updated Rust cache.".to_string(),
            controls: Vec::new(),
            diagnostics: Vec::new(),
            events: Vec::new(),
        };

        let params_json = serde_json::to_value(params).unwrap();
        assert_eq!(params_json["stage"], json!("idle-dream"));
        assert_eq!(params_json["modelUsed"], json!("gpt-5.4-mini"));
        assert_eq!(params_json["backendApplied"], json!(true));
        assert_eq!(
            params_json["deletedIds"],
            json!(["mem-noise", "mem-duplicate"])
        );
        assert_eq!(params_json["records"][0]["parents"][0], json!("mem-noise"));
        let result_json = serde_json::to_value(result).unwrap();
        assert_eq!(result_json["available"], json!(true));
        assert_eq!(result_json["cacheUpdated"], json!(true));
        assert_eq!(result_json["modelUsed"], json!("gpt-5.4-mini"));
        assert_eq!(result_json["createdCount"], json!(1));
        assert_eq!(result_json["deletedCount"], json!(2));
        assert_eq!(
            result_json["status"]["lastConsolidatedAt"],
            json!("2026-05-31T10:00:00Z")
        );
        assert_eq!(result_json["status"]["queueDepth"], json!(0));
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
            created_at_ms: 10,
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
    fn thread_items_protocol_uses_camel_and_kebab_case_filters() {
        let params = ThreadItemsParams {
            thread_id: "thread-1".to_string(),
            after: 42,
            filter: Some(ThreadItemFilter::NoTools),
            limit: Some(50),
        };
        let actual = serde_json::to_value(params).unwrap();
        assert_eq!(
            actual,
            json!({
                "threadId": "thread-1",
                "after": 42,
                "filter": "no-tools",
                "limit": 50
            })
        );

        let round_trip: ThreadItemsParams = serde_json::from_value(json!({
            "threadId": "thread-1",
            "filter": "user-only"
        }))
        .unwrap();
        assert_eq!(round_trip.filter, Some(ThreadItemFilter::UserOnly));
        assert_eq!(round_trip.after, 0);

        let label_params = ThreadItemLabelsUpdateParams {
            thread_id: "thread-1".to_string(),
            event_id: 43,
            labels: vec!["checkpoint".to_string(), "needs-review".to_string()],
        };
        assert_eq!(
            serde_json::to_value(label_params).unwrap(),
            json!({
                "threadId": "thread-1",
                "eventId": 43,
                "labels": ["checkpoint", "needs-review"]
            })
        );

        let fork_params = ThreadItemForkParams {
            thread_id: "thread-1".to_string(),
            event_id: 43,
            title: Some("rewrite from here".to_string()),
        };
        assert_eq!(
            serde_json::to_value(fork_params).unwrap(),
            json!({
                "threadId": "thread-1",
                "eventId": 43,
                "title": "rewrite from here"
            })
        );

        let edit_fork_params = ThreadItemEditForkParams {
            thread_id: "thread-1".to_string(),
            event_id: 43,
            input: "edited request".to_string(),
            title: Some("edit from here".to_string()),
        };
        assert_eq!(
            serde_json::to_value(edit_fork_params).unwrap(),
            json!({
                "threadId": "thread-1",
                "eventId": 43,
                "input": "edited request",
                "title": "edit from here"
            })
        );

        let result = ThreadItemsResult {
            thread_id: "thread-1".to_string(),
            branch: ThreadItemBranch {
                thread_id: "thread-1".to_string(),
                parent_thread_id: Some("thread-root".to_string()),
                fork_event_id: Some(42),
                child_thread_ids: vec!["thread-2".to_string()],
                sibling_thread_ids: vec!["thread-3".to_string()],
                path: vec!["thread-root".to_string(), "thread-1".to_string()],
                depth: 1,
            },
            filter: ThreadItemFilter::Default,
            after: 42,
            entries: vec![ThreadItemEntry {
                event_id: 43,
                parent_event_id: Some(41),
                child_event_ids: vec![44],
                completed_at_ms: 1_700_000_000_043,
                labels: vec!["checkpoint".to_string()],
                item: Item {
                    id: "item-1".to_string(),
                    thread_id: "thread-1".to_string(),
                    turn_id: "turn-1".to_string(),
                    kind: ItemKind::UserMessage {
                        text: "hello".to_string(),
                    },
                },
            }],
            items: Vec::new(),
            total_count: 0,
        };
        let actual = serde_json::to_value(result).unwrap();
        assert_eq!(actual["threadId"], json!("thread-1"));
        assert_eq!(actual["branch"]["threadId"], json!("thread-1"));
        assert_eq!(actual["branch"]["parentThreadId"], json!("thread-root"));
        assert_eq!(actual["branch"]["forkEventId"], json!(42));
        assert_eq!(actual["branch"]["childThreadIds"], json!(["thread-2"]));
        assert_eq!(actual["branch"]["siblingThreadIds"], json!(["thread-3"]));
        assert_eq!(actual["branch"]["path"], json!(["thread-root", "thread-1"]));
        assert_eq!(actual["branch"]["depth"], json!(1));
        assert_eq!(actual["filter"], json!("default"));
        assert_eq!(actual["after"], json!(42));
        assert_eq!(actual["entries"][0]["eventId"], json!(43));
        assert_eq!(actual["entries"][0]["parentEventId"], json!(41));
        assert_eq!(actual["entries"][0]["childEventIds"], json!([44]));
        assert_eq!(
            actual["entries"][0]["completedAtMs"],
            json!(1_700_000_000_043u64)
        );
        assert_eq!(actual["entries"][0]["labels"], json!(["checkpoint"]));
        assert_eq!(
            actual["entries"][0]["item"]["kind"]["type"],
            json!("userMessage")
        );
        assert_eq!(actual["totalCount"], json!(0));

        let labels_result = ThreadItemLabelsResult {
            thread_id: "thread-1".to_string(),
            event_id: 43,
            labels: vec!["checkpoint".to_string()],
            events: vec![Event {
                id: 45,
                thread_id: "thread-1".to_string(),
                turn_id: None,
                created_at_ms: 1_700_000_000_045,
                kind: EventKind::ThreadItemLabelsUpdated {
                    target_event_id: 43,
                    labels: vec!["checkpoint".to_string()],
                },
            }],
        };
        let actual = serde_json::to_value(labels_result).unwrap();
        assert_eq!(actual["eventId"], json!(43));
        assert_eq!(actual["labels"], json!(["checkpoint"]));
        assert_eq!(
            actual["events"][0]["kind"],
            json!({
                "type": "threadItemLabelsUpdated",
                "targetEventId": 43,
                "labels": ["checkpoint"]
            })
        );

        let fork_point_event = Event {
            id: 46,
            thread_id: "thread-2".to_string(),
            turn_id: None,
            created_at_ms: 1_700_000_000_046,
            kind: EventKind::ThreadForkPointSet {
                thread_id: "thread-2".to_string(),
                source_thread_id: "thread-1".to_string(),
                source_event_id: 43,
                history_cutoff_event_id: None,
            },
        };
        let actual = serde_json::to_value(fork_point_event).unwrap();
        assert_eq!(
            actual["kind"],
            json!({
                "type": "threadForkPointSet",
                "threadId": "thread-2",
                "sourceThreadId": "thread-1",
                "sourceEventId": 43
            })
        );

        let edit_fork_point_event = Event {
            id: 47,
            thread_id: "thread-3".to_string(),
            turn_id: None,
            created_at_ms: 1_700_000_000_047,
            kind: EventKind::ThreadForkPointSet {
                thread_id: "thread-3".to_string(),
                source_thread_id: "thread-1".to_string(),
                source_event_id: 43,
                history_cutoff_event_id: Some(42),
            },
        };
        let actual = serde_json::to_value(edit_fork_point_event).unwrap();
        assert_eq!(
            actual["kind"],
            json!({
                "type": "threadForkPointSet",
                "threadId": "thread-3",
                "sourceThreadId": "thread-1",
                "sourceEventId": 43,
                "historyCutoffEventId": 42
            })
        );
    }

    #[test]
    fn agent_definition_serializes_full_markdown_metadata() {
        let definition = AgentDefinition {
            name: "reviewer".to_string(),
            description: "Reviews code".to_string(),
            source: Some(AgentSource::ClaudeProject),
            tools: vec!["read".to_string()],
            tool_denylist: vec!["shell_exec".to_string()],
            model: Some("coding".to_string()),
            effort: Some("strong".to_string()),
            permission_mode: Some(PermissionMode::ReadOnly),
            memory_mode: Some("summary".to_string()),
            background: true,
            worktree_root: Some("worktrees/reviewer".to_string()),
            isolation: Some("worktree".to_string()),
            color: Some("cyan".to_string()),
            skills: vec!["code-review".to_string()],
            max_turns: Some(4),
            path: Some(".claude/agents/reviewer.md".to_string()),
            filename: Some("reviewer.md".to_string()),
            read_only: true,
            instructions: "Inspect the diff.".to_string(),
        };

        let value = serde_json::to_value(&definition).unwrap();
        assert_eq!(value["source"], json!("claudeProject"));
        assert_eq!(value["toolDenylist"], json!(["shell_exec"]));
        assert_eq!(value["memoryMode"], json!("summary"));
        assert_eq!(value["worktreeRoot"], json!("worktrees/reviewer"));
        assert_eq!(value["readOnly"], json!(true));
    }

    #[test]
    fn agent_list_result_reports_failed_files() {
        let result = AgentListResult {
            items: vec![ResolvedAgent {
                active: AgentDefinition::minimal_for_test(
                    "Plan",
                    AgentSource::BuiltIn,
                    "Built-in planner",
                    "Plan carefully.",
                ),
                shadowed: Vec::new(),
            }],
            failed: vec![FailedAgentFile {
                path: ".oppi/agents/bad.md".to_string(),
                source: AgentSource::Project,
                message: "missing required `name` frontmatter".to_string(),
            }],
        };

        let value = serde_json::to_value(&result).unwrap();
        assert_eq!(value["items"][0]["active"]["name"], json!("Plan"));
        assert_eq!(value["failed"][0]["source"], json!("project"));
    }

    #[test]
    fn side_question_protocol_links_parent_and_side_turns() {
        let params = SideQuestionParams {
            thread_id: "parent-thread".to_string(),
            question: "What context does Guardian get?".to_string(),
            parent_turn_id: Some("turn-parent".to_string()),
            execution_mode: ExecutionMode::Background,
            sandbox_policy: None,
            model_steps: Vec::new(),
            model_provider: None,
            tool_definitions: Vec::new(),
            approved_tool_call_ids: Vec::new(),
            cancellation: None,
            max_continuations: Some(1),
        };

        let value = serde_json::to_value(&params).unwrap();
        assert_eq!(value["threadId"], json!("parent-thread"));
        assert_eq!(value["parentTurnId"], json!("turn-parent"));
        assert_eq!(value["executionMode"], json!("background"));
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
    fn serializes_model_registry_list_contract_as_camel_case() {
        let params = ModelRegistryListParams {
            thread_id: Some("thread-1".to_string()),
            provider: "openai-codex".to_string(),
            selected_model_id: Some("gpt-5.5".to_string()),
            scope: vec!["openai-codex/gpt-5.4".to_string()],
            available_only: true,
        };
        let params = serde_json::to_value(params).unwrap();
        assert_eq!(params["threadId"], json!("thread-1"));
        assert_eq!(params["selectedModelId"], json!("gpt-5.5"));
        assert_eq!(params["availableOnly"], json!(true));
        assert_eq!(params["scope"][0], json!("openai-codex/gpt-5.4"));

        let selected = ModelRef {
            id: "gpt-5.5".to_string(),
            provider: "openai-codex".to_string(),
            display_name: "GPT 5.5".to_string(),
            context_window: Some(400000),
            role: None,
        };
        let result = ModelRegistryListResult {
            provider: "openai-codex".to_string(),
            scope: vec!["openai-codex/gpt-5.4".to_string()],
            available_only: true,
            selected_model: Some(selected.clone()),
            items: vec![selected],
            diagnostics: vec![Diagnostic {
                level: DiagnosticLevel::Warning,
                message: "models.json unsupported provider API".to_string(),
                metadata: BTreeMap::from([("provider".to_string(), "google-proxy".to_string())]),
            }],
        };
        let result = serde_json::to_value(result).unwrap();
        assert_eq!(result["provider"], json!("openai-codex"));
        assert_eq!(result["availableOnly"], json!(true));
        assert_eq!(result["selectedModel"]["id"], json!("gpt-5.5"));
        assert_eq!(result["selectedModel"]["contextWindow"], json!(400000));
        assert_eq!(result["items"][0]["displayName"], json!("GPT 5.5"));
        assert_eq!(result["diagnostics"][0]["level"], json!("warning"));
        assert_eq!(
            result["diagnostics"][0]["metadata"]["provider"],
            json!("google-proxy")
        );
    }

    fn guardian_review_record() -> GuardianReviewRecord {
        GuardianReviewRecord {
            id: "review-1".to_string(),
            timestamp: "2026-05-26T10:00:00Z".to_string(),
            status: GuardianReviewStatus::Approved,
            mode: PermissionMode::AutoReview,
            tool_name: "write_file".to_string(),
            tool_call_id: "call-1".to_string(),
            signature: "oppi::write_file\n{\"path\":\"docs/notes.md\"}".to_string(),
            circuit_key: "oppi::write_file:workspace-write:docs/notes.md".to_string(),
            summary: "write docs/notes.md".to_string(),
            risk: GuardianRiskSummary {
                category: "workspace-write".to_string(),
                reason: "writes a project file".to_string(),
                protected_hits: Vec::new(),
                paths: vec!["docs/notes.md".to_string()],
                hints: vec!["user requested docs update".to_string()],
            },
            decision: Some(GuardianReviewDecision {
                outcome: GuardianReviewOutcome::Allow,
                risk_level: RiskLevel::Low,
                user_authorization: GuardianAuthorizationLevel::High,
                rationale: "User explicitly asked to update docs/notes.md.".to_string(),
                cache_scope: Some(GuardianCacheScope::Exact),
            }),
            reviewer_model: Some("openai-codex/gpt-5.5".to_string()),
            cached_from: None,
            strict_json: Some(
                r#"{"outcome":"allow","risk_level":"low","user_authorization":"high","rationale":"User explicitly asked to update docs/notes.md.","cache_scope":"exact"}"#
                    .to_string(),
            ),
        }
    }

    #[test]
    fn serializes_guardian_review_record_as_camel_case() {
        let actual = serde_json::to_value(guardian_review_record()).unwrap();

        assert_eq!(actual["status"], json!("approved"));
        assert_eq!(actual["mode"], json!("auto-review"));
        assert_eq!(actual["toolName"], json!("write_file"));
        assert_eq!(actual["toolCallId"], json!("call-1"));
        assert_eq!(actual["risk"]["protectedHits"], json!([]));
        assert_eq!(actual["risk"]["paths"][0], json!("docs/notes.md"));
        assert_eq!(actual["decision"]["outcome"], json!("allow"));
        assert_eq!(actual["decision"]["riskLevel"], json!("low"));
        assert_eq!(actual["decision"]["userAuthorization"], json!("high"));
        assert_eq!(actual["decision"]["cacheScope"], json!("exact"));
        assert_eq!(actual["reviewerModel"], json!("openai-codex/gpt-5.5"));
    }

    #[test]
    fn serializes_guardian_review_lifecycle_events() {
        let started = Event {
            id: 20,
            thread_id: "thread-1".to_string(),
            turn_id: Some("turn-1".to_string()),
            created_at_ms: 20,
            kind: EventKind::PermissionReviewStarted {
                record: guardian_review_record(),
            },
        };
        let completed = Event {
            id: 21,
            thread_id: "thread-1".to_string(),
            turn_id: Some("turn-1".to_string()),
            created_at_ms: 21,
            kind: EventKind::PermissionReviewCompleted {
                record: guardian_review_record(),
            },
        };

        let started = serde_json::to_value(started).unwrap();
        let completed = serde_json::to_value(completed).unwrap();
        assert_eq!(started["kind"]["type"], json!("permissionReviewStarted"));
        assert_eq!(
            completed["kind"]["type"],
            json!("permissionReviewCompleted")
        );
        assert_eq!(started["kind"]["record"]["toolCallId"], json!("call-1"));
        assert_eq!(completed["kind"]["record"]["toolCallId"], json!("call-1"));
    }

    #[test]
    fn serializes_permission_review_context_as_camel_case() {
        let result = PermissionReviewContextResult {
            settings: GuardianReviewSettings {
                timeout_ms: 45_000,
                reviewer_model: Some("openai-codex/gpt-5.5".to_string()),
            },
            reviewer_provider_policy: "non-Meridian OpenAI-compatible reviewer".to_string(),
            prompt_context: vec![
                "permission/cwd/protected-pattern context".to_string(),
                "compact runtime transcript".to_string(),
            ],
            reviewer_tools: vec![
                "oppi_review_read".to_string(),
                "oppi_review_ls".to_string(),
                "oppi_review_grep".to_string(),
            ],
            hard_denies: vec!["protected paths".to_string(), "raw secrets".to_string()],
            output_contract: json!({
                "outcome": "allow|deny",
                "cache_scope": "none|exact"
            }),
            prompt_skeleton: "Return exactly this JSON shape with no markdown.\nProposed tool call JSON:\n<tool call>".to_string(),
        };

        let actual = serde_json::to_value(result).unwrap();
        assert_eq!(actual["settings"]["timeoutMs"], json!(45_000));
        assert_eq!(
            actual["reviewerProviderPolicy"],
            json!("non-Meridian OpenAI-compatible reviewer")
        );
        assert_eq!(
            actual["promptContext"][1],
            json!("compact runtime transcript")
        );
        assert_eq!(actual["reviewerTools"][2], json!("oppi_review_grep"));
        assert_eq!(actual["hardDenies"][0], json!("protected paths"));
        assert_eq!(actual["outputContract"]["cache_scope"], json!("none|exact"));
        assert!(
            actual["promptSkeleton"]
                .as_str()
                .unwrap()
                .contains("Proposed tool call JSON")
        );
    }

    #[test]
    fn serializes_ask_user_timeout_settings_and_request_metadata() {
        let request = AskUserRequest {
            id: "question-1".to_string(),
            title: Some("Choose scope".to_string()),
            questions: vec![AskUserQuestion {
                id: "scope".to_string(),
                question: "Which test scope?".to_string(),
                options: vec![AskUserOption {
                    id: "changed".to_string(),
                    label: "Changed package".to_string(),
                    description: None,
                }],
                allow_custom: Some(false),
                default_option_id: Some("changed".to_string()),
                required: true,
            }],
            tool_call_id: Some("ask-1".to_string()),
            timeout_ms: 120_000,
        };

        let value = serde_json::to_value(request).unwrap();
        assert_eq!(value["timeoutMs"], json!(120_000));

        let settings = AskUserSettings { timeout_ms: 0 };
        let update = AskUserSettingsUpdateParams {
            thread_id: Some("thread-1".to_string()),
            settings: settings.clone(),
        };
        let result = AskUserSettingsResult { settings };

        let update = serde_json::to_value(update).unwrap();
        let result = serde_json::to_value(result).unwrap();
        assert_eq!(update["settings"]["timeoutMs"], json!(0));
        assert_eq!(result["settings"]["timeoutMs"], json!(0));
    }

    #[test]
    fn serializes_todo_state_event() {
        let event = Event {
            id: 4,
            thread_id: "thread-1".to_string(),
            turn_id: Some("turn-1".to_string()),
            created_at_ms: 4,
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
            created_at_ms: 5,
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
            created_at_ms: 3,
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
            created_at_ms: 4,
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
            created_at_ms: 3,
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
    fn serializes_resolved_prompt_template() {
        let template = ResolvedPromptTemplate {
            active: PromptTemplateRef {
                id: "component".to_string(),
                name: "component".to_string(),
                description: "Create a component".to_string(),
                argument_hint: Some("<name> [features]".to_string()),
                source: PromptTemplateSource::Plugin,
                path: Some("/repo/plugin/prompts/component.md".to_string()),
                priority: 15,
            },
            shadowed: vec![PromptTemplateRef {
                id: "component".to_string(),
                name: "component".to_string(),
                description: "Project override".to_string(),
                argument_hint: None,
                source: PromptTemplateSource::Project,
                path: Some(".oppi/prompts/component.md".to_string()),
                priority: 20,
            }],
        };
        let actual = serde_json::to_value(template).unwrap();
        assert_eq!(actual["active"]["name"], json!("component"));
        assert_eq!(actual["active"]["argumentHint"], json!("<name> [features]"));
        assert_eq!(actual["active"]["source"], json!("plugin"));
        assert_eq!(actual["active"]["priority"], json!(15));
        assert_eq!(actual["shadowed"][0]["source"], json!("project"));
        assert!(actual["shadowed"][0].get("argumentHint").is_none());
    }

    #[test]
    fn serializes_prompt_template_list_and_expand_contracts() {
        let list = PromptTemplateListParams {
            thread_id: Some("thread-1".to_string()),
        };
        let actual = serde_json::to_value(list).unwrap();
        assert_eq!(actual["threadId"], json!("thread-1"));

        let expand = PromptTemplateExpandParams {
            thread_id: Some("thread-1".to_string()),
            name: "component".to_string(),
            args: "Button \"click handler\"".to_string(),
            prompt_variant_append: Some("Variant append".to_string()),
        };
        let actual = serde_json::to_value(expand).unwrap();
        assert_eq!(actual["threadId"], json!("thread-1"));
        assert_eq!(actual["name"], json!("component"));
        assert_eq!(actual["args"], json!("Button \"click handler\""));
        assert_eq!(actual["promptVariantAppend"], json!("Variant append"));

        let result = PromptTemplateExpandResult {
            name: "component".to_string(),
            input: "Create Button".to_string(),
            template: PromptTemplateRef {
                id: "component".to_string(),
                name: "component".to_string(),
                description: "Create a component".to_string(),
                argument_hint: None,
                source: PromptTemplateSource::Plugin,
                path: Some("/repo/plugin/prompts/component.md".to_string()),
                priority: 15,
            },
            notes: vec!["Expanded native prompt template from plugin prompt-pack".to_string()],
        };
        let actual = serde_json::to_value(result).unwrap();
        assert_eq!(actual["name"], json!("component"));
        assert_eq!(actual["template"]["source"], json!("plugin"));
        assert_eq!(
            actual["notes"][0],
            json!("Expanded native prompt template from plugin prompt-pack")
        );
    }

    #[test]
    fn serializes_resolved_theme() {
        let theme = ResolvedTheme {
            active: ThemeRef {
                id: "acme".to_string(),
                name: "acme".to_string(),
                source: ThemeSource::Plugin,
                path: Some("/repo/plugin/themes/acme.json".to_string()),
                priority: 15,
            },
            shadowed: vec![ThemeRef {
                id: "acme".to_string(),
                name: "acme".to_string(),
                source: ThemeSource::Project,
                path: Some(".oppi/themes/acme.json".to_string()),
                priority: 20,
            }],
        };
        let actual = serde_json::to_value(theme).unwrap();
        assert_eq!(actual["active"]["name"], json!("acme"));
        assert_eq!(actual["active"]["source"], json!("plugin"));
        assert_eq!(
            actual["active"]["path"],
            json!("/repo/plugin/themes/acme.json")
        );
        assert_eq!(actual["shadowed"][0]["source"], json!("project"));
    }

    #[test]
    fn serializes_theme_list_and_load_contracts() {
        let list = ThemeListParams {
            thread_id: Some("thread-1".to_string()),
        };
        let actual = serde_json::to_value(list).unwrap();
        assert_eq!(actual["threadId"], json!("thread-1"));

        let load = ThemeLoadParams {
            thread_id: Some("thread-1".to_string()),
            name: "acme".to_string(),
        };
        let actual = serde_json::to_value(load).unwrap();
        assert_eq!(actual["threadId"], json!("thread-1"));
        assert_eq!(actual["name"], json!("acme"));

        let result = ThemeLoadResult {
            theme: ThemeRef {
                id: "acme".to_string(),
                name: "acme".to_string(),
                source: ThemeSource::Plugin,
                path: Some("/repo/plugin/themes/acme.json".to_string()),
                priority: 15,
            },
            colors: json!({
                "accent": "#39d7e5",
                "text": "",
                "permissionAutoReviewingBg": "#102b35"
            }),
            export: Some(json!({ "pageBg": "#0f1419" })),
        };
        let actual = serde_json::to_value(result).unwrap();
        assert_eq!(actual["theme"]["source"], json!("plugin"));
        assert_eq!(actual["colors"]["accent"], json!("#39d7e5"));
        assert_eq!(
            actual["colors"]["permissionAutoReviewingBg"],
            json!("#102b35")
        );
        assert_eq!(actual["export"]["pageBg"], json!("#0f1419"));
    }

    #[test]
    fn serializes_command_prepare_thread_scope() {
        let params = CommandPrepareParams {
            thread_id: Some("thread-1".to_string()),
            command: "component".to_string(),
            args: "Button".to_string(),
            context: json!({ "role": "executor" }),
            prompt_variant_append: Some("Variant append".to_string()),
        };
        let actual = serde_json::to_value(params).unwrap();
        assert_eq!(actual["threadId"], json!("thread-1"));
        assert_eq!(actual["command"], json!("component"));
        assert_eq!(actual["promptVariantAppend"], json!("Variant append"));
    }

    #[test]
    fn serializes_mcp_config_status_and_tool_schema_contracts() {
        let config = McpServerConfig {
            id: "fs".to_string(),
            name: "filesystem".to_string(),
            source: McpServerSource::Project,
            transport: McpTransportConfig::Stdio {
                command: "node".to_string(),
                args: vec!["server.js".to_string()],
                env: BTreeMap::from([("SAFE_ENV".to_string(), "1".to_string())]),
                cwd: Some("/repo".to_string()),
            },
            enabled: true,
            trusted: true,
            when_to_use: Some("when filesystem context is required".to_string()),
        };
        let actual = serde_json::to_value(&config).unwrap();
        assert_eq!(actual["id"], json!("fs"));
        assert_eq!(actual["source"], json!("project"));
        assert_eq!(actual["transport"]["kind"], json!("stdio"));
        assert_eq!(actual["transport"]["command"], json!("node"));
        assert_eq!(actual["transport"]["env"]["SAFE_ENV"], json!("1"));
        assert_eq!(
            actual["whenToUse"],
            json!("when filesystem context is required")
        );

        let http = McpTransportConfig::StreamableHttp {
            endpoint: "https://mcp.example.test/mcp".to_string(),
            auth_ref: Some("env:OPPI_MCP_TOKEN".to_string()),
        };
        let actual = serde_json::to_value(http).unwrap();
        assert_eq!(actual["kind"], json!("streamableHttp"));
        assert_eq!(actual["authRef"], json!("env:OPPI_MCP_TOKEN"));

        let tool = McpToolRef {
            server_id: "fs".to_string(),
            name: "read_file".to_string(),
            title: Some("Read file".to_string()),
            description: Some("Read a file".to_string()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" }
                },
                "required": ["path"]
            }),
            output_schema: Some(json!({
                "type": "object",
                "properties": {
                    "text": { "type": "string" }
                }
            })),
            annotations: McpToolAnnotations {
                read_only_hint: Some(true),
                destructive_hint: Some(false),
                idempotent_hint: Some(true),
                open_world_hint: Some(false),
            },
        };
        let actual = serde_json::to_value(&tool).unwrap();
        assert_eq!(actual["serverId"], json!("fs"));
        assert_eq!(
            actual["inputSchema"]["properties"]["path"]["type"],
            "string"
        );
        assert_eq!(
            actual["outputSchema"]["properties"]["text"]["type"],
            "string"
        );
        assert_eq!(actual["annotations"]["readOnlyHint"], json!(true));

        let server = McpServerRef {
            id: "fs".to_string(),
            name: "filesystem".to_string(),
            status: McpServerStatus::Enabled,
            source: Some(McpServerSource::Project),
            transport: None,
            trusted: true,
            description: None,
            tools: vec!["read_file".to_string()],
            tool_refs: vec![tool.clone()],
            resource_subscriptions: vec![McpResourceSubscriptionRef {
                uri: "file:///repo/README.md".to_string(),
                update_count: 2,
                last_updated_at: Some("2026-05-31T12:00:00Z".to_string()),
            }],
            when_to_use: None,
            diagnostics: Vec::new(),
            capabilities: McpServerCapabilities {
                tools: Some(McpToolsCapability {
                    list_changed: Some(true),
                }),
                resources: Some(McpResourcesCapability {
                    subscribe: Some(true),
                    list_changed: Some(true),
                }),
                prompts: Some(McpPromptsCapability {
                    list_changed: Some(false),
                }),
                sampling: Some(json!({ "context": {} })),
                elicitation: Some(json!({})),
                tasks: Some(json!({ "listChanged": true })),
            },
            list_refresh_hints: McpListRefreshHints::default(),
        };
        let actual = serde_json::to_value(&server).unwrap();
        assert_eq!(actual["capabilities"]["tools"]["listChanged"], json!(true));
        assert_eq!(
            actual["capabilities"]["resources"]["subscribe"],
            json!(true)
        );
        assert_eq!(
            actual["capabilities"]["resources"]["listChanged"],
            json!(true)
        );
        assert_eq!(
            actual["capabilities"]["prompts"]["listChanged"],
            json!(false)
        );
        assert_eq!(actual["capabilities"]["sampling"]["context"], json!({}));
        assert_eq!(actual["capabilities"]["elicitation"], json!({}));
        assert_eq!(actual["capabilities"]["tasks"]["listChanged"], json!(true));
        assert_eq!(
            actual["resourceSubscriptions"][0]["uri"],
            json!("file:///repo/README.md")
        );
        assert_eq!(actual["resourceSubscriptions"][0]["updateCount"], json!(2));
        assert_eq!(
            actual["resourceSubscriptions"][0]["lastUpdatedAt"],
            json!("2026-05-31T12:00:00Z")
        );

        let params = McpCallToolParams {
            thread_id: "thread-1".to_string(),
            server_id: "fs".to_string(),
            tool_name: "read_file".to_string(),
            arguments: json!({ "path": "README.md" }),
        };
        let actual = serde_json::to_value(params).unwrap();
        assert_eq!(actual["threadId"], json!("thread-1"));
        assert_eq!(actual["serverId"], json!("fs"));
        assert_eq!(actual["toolName"], json!("read_file"));

        let result = McpCallToolResult {
            server_id: "fs".to_string(),
            tool_name: "read_file".to_string(),
            status: ToolResultStatus::Ok,
            content: vec![json!({ "type": "text", "text": "README contents" })],
            structured_content: Some(json!({ "bytes": 42 })),
            error: None,
        };
        let actual = serde_json::to_value(result).unwrap();
        assert_eq!(actual["status"], json!("ok"));
        assert_eq!(actual["content"][0]["text"], json!("README contents"));
        assert_eq!(actual["structuredContent"]["bytes"], json!(42));
        assert!(actual.get("error").is_none());
    }

    #[test]
    fn serializes_mcp_resource_list_contracts() {
        let resource = McpResourceRef {
            server_id: "docs".to_string(),
            uri: "file:///repo/README.md".to_string(),
            name: "readme".to_string(),
            title: Some("Project README".to_string()),
            description: Some("Human-facing project overview".to_string()),
            mime_type: Some("text/markdown".to_string()),
            annotations: BTreeMap::from([("audience".to_string(), json!(["assistant"]))]),
            size: Some(2048),
        };
        let actual = serde_json::to_value(&resource).unwrap();
        assert_eq!(actual["serverId"], json!("docs"));
        assert_eq!(actual["mimeType"], json!("text/markdown"));
        assert_eq!(actual["annotations"]["audience"][0], json!("assistant"));

        let result = McpListResourcesResult {
            server_id: "docs".to_string(),
            status: ToolResultStatus::Ok,
            resources: vec![resource],
            next_cursor: Some("page-2".to_string()),
            error: None,
        };
        let actual = serde_json::to_value(&result).unwrap();
        assert_eq!(actual["serverId"], json!("docs"));
        assert_eq!(
            actual["resources"][0]["uri"],
            json!("file:///repo/README.md")
        );
        assert_eq!(actual["nextCursor"], json!("page-2"));
    }

    #[test]
    fn serializes_mcp_resource_template_list_contracts() {
        let template = McpResourceTemplateRef {
            server_id: "docs".to_string(),
            uri_template: "file:///repo/{path}".to_string(),
            name: "repo-file".to_string(),
            title: Some("Repository file".to_string()),
            description: Some("Read a repository file by path".to_string()),
            mime_type: Some("text/plain".to_string()),
            annotations: BTreeMap::from([("audience".to_string(), json!(["assistant"]))]),
        };
        let actual = serde_json::to_value(&template).unwrap();
        assert_eq!(actual["serverId"], json!("docs"));
        assert_eq!(actual["uriTemplate"], json!("file:///repo/{path}"));
        assert_eq!(actual["mimeType"], json!("text/plain"));
        assert_eq!(actual["annotations"]["audience"][0], json!("assistant"));

        let params = McpListResourceTemplatesParams {
            thread_id: "thread-1".to_string(),
            server_id: "docs".to_string(),
            cursor: Some("page-1".to_string()),
        };
        let actual = serde_json::to_value(params).unwrap();
        assert_eq!(actual["threadId"], json!("thread-1"));
        assert_eq!(actual["serverId"], json!("docs"));
        assert_eq!(actual["cursor"], json!("page-1"));

        let result = McpListResourceTemplatesResult {
            server_id: "docs".to_string(),
            status: ToolResultStatus::Ok,
            resource_templates: vec![template],
            next_cursor: Some("page-2".to_string()),
            error: None,
        };
        let actual = serde_json::to_value(&result).unwrap();
        assert_eq!(
            actual["resourceTemplates"][0]["uriTemplate"],
            json!("file:///repo/{path}")
        );
        assert_eq!(actual["nextCursor"], json!("page-2"));
    }

    #[test]
    fn serializes_mcp_resource_read_contracts() {
        let params = McpReadResourceParams {
            thread_id: "thread-1".to_string(),
            server_id: "docs".to_string(),
            uri: "file:///repo/README.md".to_string(),
        };
        let actual = serde_json::to_value(params).unwrap();
        assert_eq!(actual["threadId"], json!("thread-1"));
        assert_eq!(actual["serverId"], json!("docs"));
        assert_eq!(actual["uri"], json!("file:///repo/README.md"));

        let result = McpReadResourceResult {
            server_id: "docs".to_string(),
            uri: "file:///repo/README.md".to_string(),
            status: ToolResultStatus::Ok,
            contents: vec![
                McpResourceContent {
                    uri: "file:///repo/README.md".to_string(),
                    mime_type: Some("text/markdown".to_string()),
                    text: Some("# README".to_string()),
                    blob: None,
                },
                McpResourceContent {
                    uri: "file:///repo/logo.png".to_string(),
                    mime_type: Some("image/png".to_string()),
                    text: None,
                    blob: Some("aW1hZ2U=".to_string()),
                },
            ],
            error: None,
        };
        let actual = serde_json::to_value(result).unwrap();
        assert_eq!(actual["serverId"], json!("docs"));
        assert_eq!(actual["contents"][0]["mimeType"], json!("text/markdown"));
        assert_eq!(actual["contents"][0]["text"], json!("# README"));
        assert_eq!(actual["contents"][1]["blob"], json!("aW1hZ2U="));
    }

    #[test]
    fn serializes_mcp_resource_subscription_contracts() {
        let params = McpResourceSubscriptionParams {
            thread_id: "thread-1".to_string(),
            server_id: "docs".to_string(),
            uri: "file:///repo/README.md".to_string(),
        };
        let actual = serde_json::to_value(params).unwrap();
        assert_eq!(actual["threadId"], json!("thread-1"));
        assert_eq!(actual["serverId"], json!("docs"));
        assert_eq!(actual["uri"], json!("file:///repo/README.md"));

        let subscribed = McpResourceSubscriptionResult {
            server_id: "docs".to_string(),
            uri: "file:///repo/README.md".to_string(),
            status: ToolResultStatus::Ok,
            subscribed: true,
            error: None,
        };
        let actual = serde_json::to_value(subscribed).unwrap();
        assert_eq!(actual["serverId"], json!("docs"));
        assert_eq!(actual["uri"], json!("file:///repo/README.md"));
        assert_eq!(actual["status"], json!("ok"));
        assert_eq!(actual["subscribed"], json!(true));

        let unsubscribed = McpResourceSubscriptionResult {
            server_id: "docs".to_string(),
            uri: "file:///repo/README.md".to_string(),
            status: ToolResultStatus::Ok,
            subscribed: false,
            error: None,
        };
        let actual = serde_json::to_value(unsubscribed).unwrap();
        assert_eq!(actual["subscribed"], json!(false));
        assert!(actual.get("error").is_none());

        let updated = McpResourceUpdatedParams {
            thread_id: "thread-1".to_string(),
            server_id: "docs".to_string(),
            uri: "file:///repo/README.md".to_string(),
            received_at: Some("2026-05-31T12:00:00Z".to_string()),
        };
        let actual = serde_json::to_value(updated).unwrap();
        assert_eq!(actual["threadId"], json!("thread-1"));
        assert_eq!(actual["serverId"], json!("docs"));
        assert_eq!(actual["uri"], json!("file:///repo/README.md"));
        assert_eq!(actual["receivedAt"], json!("2026-05-31T12:00:00Z"));

        let result = McpResourceUpdatedResult {
            server_id: "docs".to_string(),
            uri: "file:///repo/README.md".to_string(),
            status: ToolResultStatus::Ok,
            updated: true,
            update_count: 1,
            error: None,
        };
        let actual = serde_json::to_value(result).unwrap();
        assert_eq!(actual["serverId"], json!("docs"));
        assert_eq!(actual["status"], json!("ok"));
        assert_eq!(actual["updated"], json!(true));
        assert_eq!(actual["updateCount"], json!(1));
        assert!(actual.get("error").is_none());
    }

    #[test]
    fn serializes_mcp_prompt_list_and_get_contracts() {
        let prompt = McpPromptRef {
            server_id: "docs".to_string(),
            name: "review-change".to_string(),
            title: Some("Review change".to_string()),
            description: Some("Review a diff with a specific focus".to_string()),
            arguments: vec![McpPromptArgumentRef {
                name: "focus".to_string(),
                title: Some("Focus".to_string()),
                description: Some("Review focus".to_string()),
                required: Some(true),
            }],
        };
        let actual = serde_json::to_value(&prompt).unwrap();
        assert_eq!(actual["serverId"], json!("docs"));
        assert_eq!(actual["name"], json!("review-change"));
        assert_eq!(actual["arguments"][0]["required"], json!(true));

        let list = McpListPromptsResult {
            server_id: "docs".to_string(),
            status: ToolResultStatus::Ok,
            prompts: vec![prompt],
            next_cursor: Some("page-2".to_string()),
            error: None,
        };
        let actual = serde_json::to_value(list).unwrap();
        assert_eq!(actual["prompts"][0]["title"], json!("Review change"));
        assert_eq!(actual["nextCursor"], json!("page-2"));

        let params = McpGetPromptParams {
            thread_id: "thread-1".to_string(),
            server_id: "docs".to_string(),
            name: "review-change".to_string(),
            arguments: BTreeMap::from([("focus".to_string(), "security".to_string())]),
        };
        let actual = serde_json::to_value(params).unwrap();
        assert_eq!(actual["threadId"], json!("thread-1"));
        assert_eq!(actual["name"], json!("review-change"));
        assert_eq!(actual["arguments"]["focus"], json!("security"));

        let result = McpGetPromptResult {
            server_id: "docs".to_string(),
            name: "review-change".to_string(),
            status: ToolResultStatus::Ok,
            description: Some("Review prompt".to_string()),
            messages: vec![json!({
                "role": "user",
                "content": { "type": "text", "text": "Review for security." }
            })],
            error: None,
        };
        let actual = serde_json::to_value(result).unwrap();
        assert_eq!(actual["description"], json!("Review prompt"));
        assert_eq!(actual["messages"][0]["role"], json!("user"));
    }

    #[test]
    fn serializes_tool_definition_with_precise_json_schemas() {
        let definition = ToolDefinition {
            name: "read_file".to_string(),
            namespace: Some("mcp_fs".to_string()),
            description: Some("Read a file through MCP".to_string()),
            concurrency_safe: true,
            requires_approval: false,
            capabilities: vec!["mcp".to_string(), "mcp:fs".to_string()],
            input_schema: Some(json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" }
                },
                "required": ["path"]
            })),
            output_schema: Some(json!({
                "type": "object",
                "properties": {
                    "text": { "type": "string" }
                }
            })),
            annotations: BTreeMap::from([
                ("readOnlyHint".to_string(), json!(true)),
                ("openWorldHint".to_string(), json!(false)),
            ]),
        };
        let actual = serde_json::to_value(&definition).unwrap();
        assert_eq!(actual["namespace"], json!("mcp_fs"));
        assert_eq!(
            actual["inputSchema"]["properties"]["path"]["type"],
            "string"
        );
        assert_eq!(
            actual["outputSchema"]["properties"]["text"]["type"],
            "string"
        );
        assert_eq!(actual["annotations"]["readOnlyHint"], json!(true));

        let legacy = ToolDefinition {
            name: "todo_write".to_string(),
            namespace: None,
            description: None,
            concurrency_safe: false,
            requires_approval: false,
            capabilities: Vec::new(),
            input_schema: None,
            output_schema: None,
            annotations: BTreeMap::new(),
        };
        let actual = serde_json::to_value(legacy).unwrap();
        assert!(actual.get("inputSchema").is_none());
        assert!(actual.get("outputSchema").is_none());
        assert!(actual.get("annotations").is_none());
    }

    #[test]
    fn serializes_plugin_materialization_contract() {
        let state = PluginMaterializationRef {
            status: PluginMaterializationStatus::Materialized,
            source_type: PluginSourceType::Marketplace,
            cache_path: Some("C:/Users/matth/.oppi/plugins/cache/demo-abc123".to_string()),
            manifest_path: Some("package.json".to_string()),
            checksum: Some("sha256:abc123".to_string()),
            materialized_at: Some("2026-05-26T12:00:00Z".to_string()),
            provenance: Some(PluginProvenance {
                marketplace: Some("local-dev".to_string()),
                catalog_url: Some("./catalog.json".to_string()),
                resolved_source: "npm:@demo/plugin".to_string(),
                resolved_version: Some("1.2.3".to_string()),
            }),
            diagnostics: vec![PluginDiagnostic {
                code: "plugin_materialized_static_only".to_string(),
                message: "Static resources may load; executable handlers require a separate boundary enablement."
                    .to_string(),
                severity: Some("info".to_string()),
            }],
        };
        let value = serde_json::to_value(&state).unwrap();
        assert_eq!(value["status"], "materialized");
        assert_eq!(value["sourceType"], "marketplace");
        assert_eq!(value["provenance"]["resolvedSource"], "npm:@demo/plugin");
    }

    #[test]
    fn serializes_plugin_executable_boundary_contract() {
        let boundary = PluginExecutableBoundaryRef {
            kind: PluginExecutableBoundaryKind::JsonRpcSubprocess,
            status: PluginExecutableBoundaryStatus::Disabled,
            command: Some("node".to_string()),
            args: vec!["dist/oppi-plugin-runner.js".to_string()],
            capabilities: vec!["commands".to_string(), "tools".to_string()],
            diagnostics: vec![PluginDiagnostic {
                code: "plugin_runner_requires_enablement".to_string(),
                message: "Executable plugin runner is disabled until explicitly enabled."
                    .to_string(),
                severity: Some("warning".to_string()),
            }],
        };
        let value = serde_json::to_value(&boundary).unwrap();
        assert_eq!(value["kind"], "jsonRpcSubprocess");
        assert_eq!(value["status"], "disabled");
    }

    #[test]
    fn serializes_plugin_ref_with_resources_and_diagnostics() {
        let plugin = PluginRef {
            id: "plugin-pack".to_string(),
            source: "/repo/plugin-pack".to_string(),
            name: Some("plugin-pack".to_string()),
            version: Some("1.0.0".to_string()),
            description: Some("Plugin pack".to_string()),
            enabled: true,
            trusted: Some(true),
            scope: Some(PluginScope::Project),
            source_type: None,
            materialization: None,
            executable_boundary: None,
            resources: vec![
                PluginResourceRef {
                    kind: PluginResourceKind::Skill,
                    path: "skills/plugin-scout/SKILL.md".to_string(),
                    status: PluginResourceStatus::Loaded,
                    reason: None,
                },
                PluginResourceRef {
                    kind: PluginResourceKind::Extension,
                    path: "extensions/index.ts".to_string(),
                    status: PluginResourceStatus::Unsupported,
                    reason: Some("Pi extensions are not executed by Rust native".to_string()),
                },
            ],
            diagnostics: vec![PluginDiagnostic {
                code: "pi_extension_unsupported".to_string(),
                message: "Extension resources are visible but not executed by Rust native"
                    .to_string(),
                severity: Some("warning".to_string()),
            }],
        };
        let actual = serde_json::to_value(plugin).unwrap();
        assert_eq!(actual["name"], json!("plugin-pack"));
        assert_eq!(actual["trusted"], json!(true));
        assert_eq!(actual["scope"], json!("project"));
        assert_eq!(actual["resources"][0]["kind"], json!("skill"));
        assert_eq!(actual["resources"][0]["status"], json!("loaded"));
        assert_eq!(actual["resources"][1]["kind"], json!("extension"));
        assert_eq!(actual["resources"][1]["status"], json!("unsupported"));
        assert_eq!(
            actual["diagnostics"][0]["code"],
            json!("pi_extension_unsupported")
        );
    }

    #[test]
    fn serializes_agent_started_event() {
        let event = Event {
            id: 2,
            thread_id: "thread-1".to_string(),
            turn_id: None,
            created_at_ms: 2,
            kind: EventKind::AgentStarted {
                run: AgentRun {
                    id: "agent-run-1".to_string(),
                    thread_id: "thread-1".to_string(),
                    parent_thread_id: Some("parent-thread".to_string()),
                    child_thread_id: Some("child-thread".to_string()),
                    child_turn_id: Some("child-turn".to_string()),
                    agent_name: "reviewer".to_string(),
                    status: AgentRunStatus::Running,
                    task: "review changes".to_string(),
                    worktree_root: Some("/repo-wt".to_string()),
                    execution_root: Some("/repo-wt".to_string()),
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
        assert_eq!(
            actual["kind"]["run"]["parentThreadId"],
            json!("parent-thread")
        );
        assert_eq!(
            actual["kind"]["run"]["childThreadId"],
            json!("child-thread")
        );
        assert_eq!(actual["kind"]["run"]["childTurnId"], json!("child-turn"));
        assert_eq!(actual["kind"]["run"]["worktreeRoot"], json!("/repo-wt"));
        assert_eq!(actual["kind"]["run"]["executionRoot"], json!("/repo-wt"));
        assert_eq!(actual["kind"]["run"]["role"], json!("reviewer"));
        assert_eq!(actual["kind"]["run"]["model"], json!("gpt-review"));
        assert_eq!(actual["kind"]["run"]["background"], json!(true));
        assert_eq!(actual["kind"]["run"]["permissionMode"], json!("read-only"));
        assert_eq!(actual["kind"]["run"]["toolAllowlist"], json!(["read_file"]));
        assert_eq!(actual["kind"]["run"]["skills"], json!(["independent"]));
    }
}
