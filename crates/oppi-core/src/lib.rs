//! In-memory runtime core for the first OPPi Rust-spine slice.
//!
//! This crate deliberately starts as a deterministic runtime skeleton. It owns
//! Thread/Turn/Item/Event state and emits the semantic phases that future model,
//! tool, approval, sandbox, and UI adapters consume.

pub mod event_store;

mod goals;
#[path = "core_parts/memory_helpers.rs"]
mod memory_helpers;
#[path = "core_parts/plugins_mcp.rs"]
mod plugins_mcp;
#[path = "core_parts/providers.rs"]
mod providers;
#[path = "core_parts/tools_guardian_registry.rs"]
mod tools_guardian_registry;

use base64::{
    Engine as _,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use event_store::{EventStore, InMemoryEventStore};
use goals::{
    apply_goal_accounting_delta, completion_budget_report, goal_tool_output, new_goal,
    render_goal_budget_limit_prompt, render_goal_continuation_prompt, status_after_budget,
    validate_goal_budget, validate_goal_objective,
};
use memory_helpers::*;
use oppi_agents::{AgentIndexOptions, built_in_agent_definitions, load_agent_index};
use oppi_protocol::*;
use oppi_sandbox::{
    PolicyDecision, SandboxBackgroundSpawnParams, SandboxedBackgroundProcess, default_policy,
    evaluate_exec, execute_sandboxed, spawn_sandboxed_background,
};
use oppi_tools::{
    ToolPairingTracker, ToolRegistry, describe_tool_batch, partition_ordered_tool_batches,
};
use plugins_mcp::*;
use providers::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tools_guardian_registry::*;

const MAX_CONTINUATIONS: u32 = 8;
const DEFAULT_EVENTS_LIST_LIMIT: usize = 1_000;
const MAX_EVENTS_LIST_LIMIT: usize = 5_000;
const PROVIDER_HISTORY_MAX_MESSAGES: usize = 24;
const PROVIDER_HISTORY_MAX_CHARS: usize = 16_000;
const GUARDIAN_REVIEW_HISTORY_LIMIT: usize = 100;
const GUARDIAN_DENIAL_CIRCUIT_WINDOW_MS: u64 = 10 * 60 * 1_000;
const GUARDIAN_DENIAL_CIRCUIT_THRESHOLD: usize = 3;
const DEFAULT_GUARDIAN_REVIEW_TIMEOUT_MS: u64 = 45_000;
const GUARDIAN_REVIEWER_MAX_CONTINUATIONS: u32 = 3;
const GUARDIAN_REVIEWER_MAX_OUTPUT_TOKENS: u32 = 512;
const IMAGE_GEN_MAX_INPUT_BYTES: u64 = 50 * 1024 * 1024;
const GPT_IMAGE_2_MIN_PIXELS: u64 = 655_360;
const GPT_IMAGE_2_MAX_PIXELS: u64 = 8_294_400;
const GPT_IMAGE_2_MAX_EDGE: u64 = 3_840;
const GPT_IMAGE_2_MAX_RATIO: f64 = 3.0;
const DEFAULT_CODEX_IMAGE_MODEL: &str = "gpt-5.2";
const CODEX_NATIVE_IMAGE_INSTRUCTIONS: &str = "You are an image generation adapter. For the user's request, call the image_generation tool exactly once. Do not answer with text unless image generation is refused or impossible.";
const RECURRING_TASK_MAX_CANDIDATES: usize = 2;
const RECURRING_TASK_MIN_TASK_CHARS: usize = 12;
const MEMORY_DISTILLER_MAX_PROMPT_CHARS: usize = 8_000;
const MEMORY_DISTILLER_MAX_OUTPUT_TOKENS: u32 = 420;
const MEMORY_DISTILLER_PROMPT: &str = r#"You are OPPi's memory distiller. You observe one completed coding-agent turn and decide whether to save a compact memory for future sessions.

Record durable technical signal only:
- shipped changes, bug fixes, configuration/docs updates, tests or validation outcomes
- decisions, trade-offs, gotchas, root causes, user preferences, and concrete next steps
- specific file paths or components when they help future recall

Skip routine chatter, empty status checks, raw terminal dumps, package installs with no finding, and repeated information already obvious from the turn.

Return only JSON matching this shape:
{
  "remember": true | false,
  "request": "short user request",
  "completed": ["what changed or was delivered"],
  "learned": ["durable finding, root cause, gotcha, or behavior"],
  "decisions": ["decision or trade-off"],
  "next": ["active next step"],
  "files": ["path/or/component"],
  "tags": ["short-topic"],
  "recurringTasks": [{"task": "repeatable work pattern", "rationale": "why this may deserve a future skill", "promptHints": ["user wording or trigger"], "outcomes": ["useful delivered outcome"], "relatedMemoryIds": ["optional memory id"], "tags": ["short-topic"]}]
}

Rules:
- Max 3 items in each array, max 18 words per item.
- Max 2 recurringTasks, and only include patterns likely to recur across projects or sessions.
- Recurring task candidates are system-scope drafts only. They must never create or update skills without future user approval.
- Use standalone statements; avoid pronouns without a referent.
- Do not describe the act of summarizing or observing.
- If remember is false, all arrays may be empty."#;
const ASK_USER_ALLOWED_TIMEOUT_MS: [u64; 5] =
    [0, 60_000, 120_000, DEFAULT_ASK_USER_TIMEOUT_MS, 600_000];
const PI_REQUIRED_THEME_COLOR_TOKENS: [&str; 51] = [
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
];
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
const OPENAI_DIRECT_MODEL_CATALOG: &[&str] = &[
    "gpt-5.5",
    "gpt-5.4",
    "gpt-5.4-mini",
    "gpt-5.4-nano",
    "gpt-5.4-pro",
    "gpt-5.3-codex",
    "gpt-5.3-codex-spark",
];
const OPENAI_CODEX_MODEL_CATALOG: &[&str] = &[
    "gpt-5.5",
    "gpt-5.4",
    "gpt-5.4-mini",
    "gpt-5.3-codex",
    "gpt-5.3-codex-spark",
    "gpt-5.2-codex",
    "gpt-5.2",
    "gpt-5.1-codex-max",
    "gpt-5.1-codex-mini",
    "gpt-5.1",
];
const GITHUB_COPILOT_MODEL_CATALOG: &[&str] = &[
    "gpt-5.5",
    "gpt-5.4",
    "gpt-5.4-mini",
    "gpt-5.3-codex",
    "gpt-5.2-codex",
    "gpt-5.2",
    "gpt-5.1-codex",
    "gpt-5.1-codex-max",
    "gpt-5.1-codex-mini",
    "gpt-5.1",
    "claude-opus-4.7",
    "claude-opus-4.6",
    "claude-sonnet-4.6",
    "claude-sonnet-4.5",
    "claude-haiku-4.5",
    "gemini-3.1-pro-preview",
    "gemini-3-pro-preview",
    "grok-code-fast-1",
];
const MERIDIAN_MODEL_CATALOG: &[&str] =
    &["claude-opus-4-6", "claude-sonnet-4-6", "claude-haiku-4-5"];

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

#[derive(Debug)]
struct McpSubscribedResourceContext {
    prompt: String,
    resources: usize,
    skipped: usize,
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
    if let Some(namespace) = call.namespace.as_deref() {
        return registry.get(Some(namespace), &call.name).or_else(|| {
            (namespace == "oppi")
                .then(|| registry.get(None, &call.name))
                .flatten()
        });
    }
    registry
        .get(Some("oppi"), &call.name)
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
        worktree_root: first_string_arg(&call.arguments, &["worktreeRoot", "worktree_root"])
            .or_else(|| agent.worktree_root.clone()),
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

fn side_question_title_preview(question: &str) -> String {
    let compact = question.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut preview = compact.chars().take(48).collect::<String>();
    if compact.chars().count() > 48 {
        preview.push_str("...");
    }
    if preview.is_empty() {
        "btw".to_string()
    } else {
        format!("btw:{preview}")
    }
}

fn side_question_turn_input(question: &str) -> String {
    format!("Answer this side question without changing the parent transcript:\n\n{question}")
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

include!("core_parts/runtime_impl.rs");

include!("core_parts/tests.rs");
