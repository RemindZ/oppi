// Mirrored OPPi Rust runtime protocol types.
//
// Source of truth: crates/oppi-protocol/src/lib.rs. These types intentionally
// cover the stable client-facing envelope, initialize negotiation, runtime
// errors, and sandbox contract first. Replace this mirror with generated types
// once the Rust schema generation pipeline lands.

export const OPPI_PROTOCOL_VERSION = "0.1.0" as const;
export const OPPI_MIN_PROTOCOL_VERSION = "0.1.0" as const;

export type JsonRpcId = string | number | null;

export type JsonRpcRequest<Params = unknown> = {
  jsonrpc?: "2.0";
  id: JsonRpcId;
  method: string;
  params?: Params;
};

export type JsonRpcSuccess<Result = unknown> = {
  jsonrpc: "2.0";
  id: JsonRpcId;
  result: Result;
};

export type JsonRpcFailure<Data = unknown> = {
  jsonrpc: "2.0";
  id: JsonRpcId;
  error: JsonRpcError<Data>;
};

export type JsonRpcResponse<Result = unknown, Data = unknown> = JsonRpcSuccess<Result> | JsonRpcFailure<Data>;

export type JsonRpcError<Data = unknown> = {
  code: number;
  message: string;
  data?: Data;
};

export type RuntimeErrorCategory =
  | "not_found"
  | "already_exists"
  | "already_resolved"
  | "ownership_mismatch"
  | "terminal_state"
  | "approval_required"
  | "tool_pairing"
  | "event_store"
  | "provider"
  | "invalid_request";

export type RuntimeErrorData = {
  code: string;
  category: RuntimeErrorCategory;
};

export type RuntimeCapability =
  | "threads"
  | "turns"
  | "events"
  | "tools"
  | "approvals"
  | "questions"
  | "sandbox"
  | "agents"
  | "memory"
  | "todos"
  | "mcp"
  | "models"
  | "side-questions"
  | "persistence-replay"
  | "commands"
  | (string & {});

export type InitializeParams = {
  clientName: string;
  clientVersion?: string;
  protocolVersion?: string;
  clientCapabilities?: RuntimeCapability[];
};

export type InitializeResult = {
  protocolVersion: string;
  minProtocolVersion: string;
  protocolCompatible: boolean;
  serverName: string;
  serverVersion: string;
  serverCapabilities: RuntimeCapability[];
  acceptedClientCapabilities: RuntimeCapability[];
};

export type ToolCall = {
  id: string;
  name: string;
  namespace?: string;
  arguments?: unknown;
};

export type ToolResultStatus = "ok" | "error" | "denied" | "aborted";

export type ToolResult = {
  callId: string;
  status: ToolResultStatus;
  output?: string;
  error?: string;
};

export type TodoStatus = "pending" | "in_progress" | "completed" | "blocked" | "cancelled";
export type TodoPriority = "low" | "medium" | "high";

export type TodoItem = {
  id: string;
  content: string;
  status: TodoStatus;
  priority?: TodoPriority;
  phase?: string;
  notes?: string;
};

export type TodoState = {
  todos: TodoItem[];
  summary: string;
};

export type FollowUpStatus = "queued" | "running" | "completed";

export type FollowUpItemContext = {
  id: string;
  text: string;
  status: FollowUpStatus;
};

export type FollowUpChainContext = {
  chainId?: string;
  rootPrompt: string;
  followUps?: FollowUpItemContext[];
  currentFollowUpId?: string;
  promptVariantAppend?: string;
};

export type AskUserOption = {
  id: string;
  label: string;
  description?: string;
};

export type AskUserQuestion = {
  id: string;
  question: string;
  options?: AskUserOption[];
  allowCustom?: boolean;
  defaultOptionId?: string;
  required?: boolean;
};

export type AskUserRequest = {
  id: string;
  title?: string;
  questions?: AskUserQuestion[];
  toolCallId?: string;
};

export type AskUserAnswer = {
  questionId: string;
  optionId?: string;
  label?: string;
  text?: string;
  skipped?: boolean;
};

export type AskUserResponse = {
  requestId: string;
  answers?: AskUserAnswer[];
  cancelled?: boolean;
  timedOut?: boolean;
};

export type SuggestedNextMessage = {
  message: string;
  confidence: number;
  reason?: string;
};

export type TodoListResult = {
  state: TodoState;
};

export type TodoOutcome = {
  id: string;
  content: string;
  status: TodoStatus;
  phase?: string;
  notes?: string;
  outcome: string;
  updatedAt?: string;
};

export type CompactionHandoffDetails = {
  source: string;
  version?: number;
  compactedAt?: string;
  remainingTodos?: TodoItem[];
  completedOutcomes?: TodoOutcome[];
};

export type HandoffCompactParams = {
  threadId: string;
  summary: string;
  details?: CompactionHandoffDetails;
};

export type CommandPrepareParams = {
  command: string;
  args?: string;
  context?: unknown;
  promptVariantAppend?: string;
};

export type CommandPrepareResult = {
  command: string;
  input: string;
  systemPromptProfile?: "review" | "audit" | (string & {});
  notes?: string[];
};

export type RuntimeToolDefinition = {
  name: string;
  namespace?: string;
  description?: string;
  concurrencySafe?: boolean;
  requiresApproval?: boolean;
  capabilities?: string[];
};

export type ExecutionMode = "blocking" | "background";

export type ScriptedModelStep = {
  assistantDeltas?: string[];
  toolCalls?: ToolCall[];
  /** Adapter-provided tool results keyed by callId; omitted calls run through Rust tool executors. */
  toolResults?: ToolResult[];
  finalResponse?: boolean;
};

export type DirectModelProviderConfig = {
  kind: "openai-compatible" | "openai-codex" | "github-copilot";
  model: string;
  /** Base URL such as https://api.openai.com/v1 or a local OpenAI-compatible endpoint. */
  baseUrl?: string;
  /** Environment variable name that contains the API key; raw keys should not be sent over JSON-RPC. */
  apiKeyEnv?: string;
  systemPrompt?: string;
  temperature?: number;
  /** Optional provider-specific reasoning effort. For OpenAI-compatible providers, off/empty is omitted and low/medium/high/minimal are sent as reasoning_effort. */
  reasoningEffort?: string;
  maxOutputTokens?: number;
  /** Enable OpenAI-compatible SSE streaming where supported; defaults to non-streaming. */
  stream?: boolean;
};

export type AgenticCancellation = {
  reason: string;
  beforeModelContinuation?: number;
  toolCallIds?: string[];
};

export type AgenticTurnParams = {
  threadId: string;
  input: string;
  /** Defaults to blocking. Use background for native clients that poll events/list while the turn runs, including live deltas, pauses, resumes, and interrupt events. */
  executionMode?: ExecutionMode;
  /** Hidden routing context for follow-up prompts queued while an earlier turn was running. */
  followUp?: FollowUpChainContext;
  /** Turn-level sandbox/permission policy used by Rust-owned shell and file tools. */
  sandboxPolicy?: SandboxPolicy;
  modelSteps?: ScriptedModelStep[];
  /** Selects a direct Rust provider instead of scripted/adapter-provided model steps. */
  modelProvider?: DirectModelProviderConfig;
  /** Per-turn tool definitions advertised to direct providers and used by Rust/adapters for execution policy. */
  toolDefinitions?: RuntimeToolDefinition[];
  approvedToolCallIds?: string[];
  cancellation?: AgenticCancellation;
  maxContinuations?: number;
};

export type AgenticTurnResumeParams = {
  threadId: string;
  turnId: string;
  /** Optional follow-up context when resuming a provider turn without persisted provider messages. */
  followUp?: FollowUpChainContext;
  /** Structured answer payload when resuming a turn paused by ask_user. */
  askUserResponse?: AskUserResponse;
  /** Optional replacement turn-level sandbox/permission policy for resumed tool execution. */
  sandboxPolicy?: SandboxPolicy;
  modelSteps?: ScriptedModelStep[];
  modelProvider?: DirectModelProviderConfig;
  /** Per-turn tool definitions advertised to direct providers and used by Rust/adapters for execution policy. */
  toolDefinitions?: RuntimeToolDefinition[];
  approvedToolCallIds?: string[];
  cancellation?: AgenticCancellation;
  maxContinuations?: number;
};

export type AgenticTurnResult<Turn = unknown, Event = unknown, ApprovalRequest = unknown> = {
  turn: Turn;
  events: Event[];
  awaitingApproval?: ApprovalRequest;
  awaitingQuestion?: AskUserRequest;
};

export type EventsListParams = {
  threadId: string;
  after?: number;
  /** Defaults to 1000 and is capped by the runtime at 5000. */
  limit?: number;
};

export type EventsListResult<Event = unknown> = {
  events: Event[];
};

export type ServerShutdownResult = {
  shuttingDown: true;
};

export type BackgroundTaskStatus = "running" | "completed" | "failed" | "killed";

export type BackgroundTaskInfo = {
  id: string;
  command: string;
  cwd: string;
  outputPath: string;
  status: BackgroundTaskStatus;
};

export type BackgroundListResult = {
  items: BackgroundTaskInfo[];
};

export type BackgroundReadParams = {
  taskId: string;
  maxBytes?: number;
};

export type BackgroundReadResult = {
  task: BackgroundTaskInfo;
  output: string;
};

export type BackgroundKillParams = {
  taskId: string;
};

export type BackgroundKillResult = {
  task: BackgroundTaskInfo;
  message: string;
};

export type RuntimeMetrics = {
  threadCount: number;
  turnCount: number;
  eventCount: number;
  pendingApprovals: number;
  pendingQuestions: number;
  pluginCount: number;
  mcpServerCount: number;
  modelCount: number;
  agentDefinitionCount: number;
  agentRunCount: number;
  turnStatusCounts?: Record<string, number>;
};

export type DebugBundle = {
  schemaVersion: number;
  redacted: boolean;
  metrics: RuntimeMetrics;
  diagnostics?: Diagnostic[];
};

export type PermissionMode = "read-only" | "default" | "auto-review" | "full-access";
export type NetworkPolicy = "disabled" | "ask" | "enabled";
export type FilesystemPolicy = "readOnly" | "workspaceWrite" | "unrestricted";
export type SandboxPreference = "auto" | "require" | "forbid";
export type SandboxType = "none" | "macosSeatbelt" | "linuxLandlock" | "linuxBubblewrap" | "windowsRestrictedToken";
export type SandboxEnforcement = "none" | "reviewOnly" | "osSandbox";
export type RiskLevel = "low" | "medium" | "high" | "critical";

export type FilesystemRoot =
  | { type: "cwd" }
  | { type: "workspace" }
  | { type: "projectRoots"; subpath?: string }
  | { type: "home" }
  | { type: "temp" }
  | { type: "platformDefaults" }
  | { type: "unknown"; path: string }
  | { type: "path"; path: string };

export type FilesystemAccess = "read" | "write" | "none" | "readOnly" | "readWrite" | "denyRead";

export type FilesystemRule = {
  root: FilesystemRoot;
  access: FilesystemAccess;
};

export type PermissionProfile = {
  mode: PermissionMode;
  readableRoots?: string[];
  writableRoots: string[];
  filesystemRules?: FilesystemRule[];
  protectedPatterns: string[];
};

export type SandboxPolicy = {
  permissionProfile: PermissionProfile;
  network: NetworkPolicy;
  filesystem: FilesystemPolicy;
};

export type ManagedNetworkConfig = {
  httpProxy?: string;
  httpsProxy?: string;
  allProxy?: string;
  noProxy?: string[];
  allowLoopback?: boolean;
};

export type SandboxExecRequest = {
  command: string;
  cwd: string;
  writesFiles?: boolean;
  usesNetwork?: boolean;
  touchesProtectedPath?: boolean;
  touchedPaths?: string[];
};

export type DiagnosticLevel = "info" | "warning" | "error";

export type Diagnostic = {
  level: DiagnosticLevel;
  message: string;
  metadata?: Record<string, string>;
};

export type SandboxExecPlan = {
  command: string;
  cwd: string;
  sandboxType: SandboxType;
  enforcement: SandboxEnforcement;
  filesystem: FilesystemPolicy;
  network: NetworkPolicy;
  managedNetwork?: ManagedNetworkConfig;
  readableRoots?: string[];
  writableRoots?: string[];
  filesystemRules?: FilesystemRule[];
  protectedPatterns?: string[];
  diagnostics?: Diagnostic[];
};

export type SandboxPolicyDecision =
  | { type: "allow" }
  | { type: "ask"; risk: RiskLevel; reason: string }
  | { type: "deny"; reason: string };

export type SandboxPlanParams = {
  policy: SandboxPolicy;
  request: SandboxExecRequest;
  preference?: SandboxPreference;
};

export type SandboxPlanResult = {
  decision: SandboxPolicyDecision;
  plan?: SandboxExecPlan;
};

export type SandboxExecParams = SandboxPlanParams & {
  approvalGranted?: boolean;
  managedNetwork?: ManagedNetworkConfig;
  timeoutMs?: number;
  maxOutputBytes?: number;
};

export type SandboxAuditRecord = {
  action: string;
  decision: SandboxPolicyDecision;
  command: string;
  reason?: string;
  enforcement?: SandboxEnforcement;
  sandboxType?: SandboxType;
};

export type SandboxExecResult = {
  decision: SandboxPolicyDecision;
  plan?: SandboxExecPlan;
  exitCode?: number | null;
  stdout: string;
  stderr: string;
  timedOut: boolean;
  audit?: SandboxAuditRecord[];
};

export type SandboxStatus = {
  supported: boolean;
  enforcement: SandboxEnforcement;
  platform: string;
  message: string;
  availableSandboxTypes?: SandboxType[];
};
