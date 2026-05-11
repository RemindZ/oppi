use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use oppi_protocol::{
    AgentDefinition, AgentRun, AgentSource, ApprovalRequest, ArtifactMetadata, AskUserRequest,
    Event, EventKind, FilesystemPolicy, ItemKind, ModelRef, NetworkPolicy, PermissionMode,
    ResolvedAgent, RuntimeListResult, SuggestedNextMessage, Thread, ThreadGoal, ThreadGoalStatus,
    ThreadStatus, TodoClientAction, TodoClientActionResult, TodoListResult, TodoState, ToolCall,
    ToolResultStatus,
};
mod ratatui_ui;
mod tui;

use crossterm::event::{self, Event as CrosstermEvent, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::terminal;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::env;
use std::fs;
use std::io::{self, BufRead, BufReader, IsTerminal, Read, Write};
use std::net::{TcpListener, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const POLL_INTERVAL: Duration = Duration::from_millis(40);
const TURN_TIMEOUT: Duration = Duration::from_secs(180);
const EXIT_COMMAND_ECHO_TEXT: &str = "› /exit";
const EXIT_REQUESTED_TEXT: &str = "exit requested (/exit)";
const MERIDIAN_DEFAULT_BASE_URL: &str = "http://127.0.0.1:3456";
const MERIDIAN_PACKAGE_NAME: &str = "@rynfar/meridian";
const GPT_MAIN_DEFAULT_MODEL: &str = "gpt-5.5";
const GPT_CODING_SUBAGENT_DEFAULT_MODEL: &str = "gpt-5.3-codex";
const CLAUDE_MAIN_DEFAULT_MODEL: &str = "claude-opus-4-6";
const CLAUDE_CODING_SUBAGENT_DEFAULT_MODEL: &str = "claude-sonnet-4-6";
const MERIDIAN_DEFAULT_MODEL: &str = CLAUDE_MAIN_DEFAULT_MODEL;
const MERIDIAN_API_KEY_ENV: &str = "OPPI_MERIDIAN_API_KEY";
const OPENAI_CODEX_PROVIDER_ID: &str = "openai-codex";
const OPENAI_CODEX_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const OPENAI_CODEX_AUTHORIZE_URL: &str = "https://auth.openai.com/oauth/authorize";
const OPENAI_CODEX_TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
const OPENAI_CODEX_REDIRECT_URI: &str = "http://localhost:1455/auth/callback";
const OPENAI_CODEX_SCOPE: &str = "openid profile email offline_access";
const OPENAI_DIRECT_DEFAULT_MODEL: &str = GPT_MAIN_DEFAULT_MODEL;
const OPENAI_CODEX_DEFAULT_MODEL: &str = GPT_MAIN_DEFAULT_MODEL;
const GITHUB_COPILOT_PROVIDER_ID: &str = "github-copilot";
const GOAL_CONTINUATION_INPUT: &str = "Continue working toward the active thread goal.";
const GOAL_CONTINUATION_SYSTEM_HEADING: &str = "OPPi goal continuation";
const GOAL_CONTINUATION_CAP: u32 = 8;
const OPPI_FEATURE_ROUTING_SYSTEM_APPEND: &str =
    include_str!("../../../systemprompts/main/oppi-feature-routing-system-append.md");
const OPPI_FEATURE_ROUTING_VARIANT_A_APPEND: &str = include_str!(
    "../../../systemprompts/experiments/promptname_a/oppi-feature-routing-system-append.md"
);
const OPPI_FEATURE_ROUTING_VARIANT_B_APPEND: &str = include_str!(
    "../../../systemprompts/experiments/promptname_b/oppi-feature-routing-system-append.md"
);
const GITHUB_COPILOT_CLIENT_ID: &str = "Iv1.b507a08c87ecfe98";
const GITHUB_COPILOT_DEFAULT_DOMAIN: &str = "github.com";
const GITHUB_COPILOT_DEFAULT_BASE_URL: &str = "https://api.individual.githubcopilot.com";
const GITHUB_COPILOT_DEFAULT_MODEL: &str = GPT_MAIN_DEFAULT_MODEL;

// Native fallback catalogs mirror Pi's installed built-in catalogs for the
// provider paths OPPi owns directly. Pi remains the source of truth for the full
// registry; native uses these only when it cannot ask Pi's ModelRegistry.
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

#[derive(Debug, Clone, PartialEq)]
struct ShellCommand {
    initial_prompt: Option<String>,
    server: Option<PathBuf>,
    resume_thread: Option<String>,
    list_sessions: bool,
    json: bool,
    interactive: bool,
    raw: bool,
    ratatui: bool,
    provider: ProviderConfig,
}

#[derive(Debug, Clone, PartialEq)]
enum ProviderConfig {
    Mock,
    OpenAiCompatible(OpenAiCompatibleConfig),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DirectProviderFlavor {
    OpenAiCompatible,
    OpenAiCodex,
    GitHubCopilot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ThinkingLevel {
    Off,
    Minimal,
    Low,
    Medium,
    High,
    XHigh,
}

const ALL_THINKING_LEVELS: [ThinkingLevel; 6] = [
    ThinkingLevel::Off,
    ThinkingLevel::Minimal,
    ThinkingLevel::Low,
    ThinkingLevel::Medium,
    ThinkingLevel::High,
    ThinkingLevel::XHigh,
];

impl ThinkingLevel {
    fn as_str(self) -> &'static str {
        match self {
            ThinkingLevel::Off => "off",
            ThinkingLevel::Minimal => "minimal",
            ThinkingLevel::Low => "low",
            ThinkingLevel::Medium => "medium",
            ThinkingLevel::High => "high",
            ThinkingLevel::XHigh => "xhigh",
        }
    }

    fn label(self) -> &'static str {
        match self {
            ThinkingLevel::Off => "Off",
            ThinkingLevel::Minimal => "Minimal",
            ThinkingLevel::Low => "Low",
            ThinkingLevel::Medium => "Medium",
            ThinkingLevel::High => "High",
            ThinkingLevel::XHigh => "XHigh",
        }
    }

    fn description(self) -> &'static str {
        match self {
            ThinkingLevel::Off => "No explicit reasoning effort.",
            ThinkingLevel::Minimal => "Fastest reasoning-capable setting.",
            ThinkingLevel::Low => "Light reasoning for simple edits and quick answers.",
            ThinkingLevel::Medium => "Balanced reasoning for normal coding work.",
            ThinkingLevel::High => {
                "Deep reasoning for complex debugging, architecture, and multi-step work."
            }
            ThinkingLevel::XHigh => "Maximum effort where the selected model supports it.",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct OpenAiCompatibleConfig {
    flavor: DirectProviderFlavor,
    model: String,
    base_url: Option<String>,
    api_key_env: Option<String>,
    system_prompt: Option<String>,
    temperature: Option<f32>,
    reasoning_effort: Option<String>,
    max_output_tokens: Option<u32>,
    stream: bool,
}

impl ProviderConfig {
    fn label(&self) -> &'static str {
        match self {
            Self::Mock => "mock scripted provider",
            Self::OpenAiCompatible(config) => match config.flavor {
                DirectProviderFlavor::OpenAiCompatible => "OpenAI-compatible direct provider",
                DirectProviderFlavor::OpenAiCodex => "ChatGPT/Codex subscription provider",
                DirectProviderFlavor::GitHubCopilot => "GitHub Copilot subscription provider",
            },
        }
    }
}

const ROLE_NAMES: [&str; 6] = [
    "planner",
    "thinking",
    "reviewer",
    "orchestrator",
    "executor",
    "subagent",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LoginPicker {
    Root,
    Subscription,
    Api,
    Claude,
    MeridianInstallApproval,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SlashCommandSpec {
    command: &'static str,
    usage: &'static str,
    description: &'static str,
    aliases: &'static [&'static str],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SlashPaletteMode {
    Commands,
    Arguments,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SlashPaletteItem {
    label: String,
    insert: String,
    detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SlashPaletteAccept {
    Insert(String),
    Submit(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum GoalBudgetRoute {
    Unchanged,
    Set(i64),
    Clear,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum GoalCommandRoute {
    Get,
    Clear,
    Set {
        objective: Option<String>,
        status: Option<ThreadGoalStatus>,
        token_budget: GoalBudgetRoute,
    },
    CreateObjective(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SlashPalette {
    mode: SlashPaletteMode,
    query: String,
    title: String,
    hint: String,
    items: Vec<SlashPaletteItem>,
    selected: usize,
}

const SLASH_COMMAND_SPECS: &[SlashCommandSpec] = &[
    SlashCommandSpec {
        command: "/help",
        usage: "/help",
        description: "Show the visible OPPi slash command list",
        aliases: &["/commands"],
    },
    SlashCommandSpec {
        command: "/settings",
        usage: "/settings",
        description: "Open the native settings panel",
        aliases: &["/prefs", "/preferences", "/settings:oppi", "/oppi-settings"],
    },
    SlashCommandSpec {
        command: "/login",
        usage: "/login [subscription|api]",
        description: "Configure provider auth; bare opens native login panel",
        aliases: &[],
    },
    SlashCommandSpec {
        command: "/logout",
        usage: "/logout [provider|anthropic]",
        description: "Clear session provider or stop Meridian",
        aliases: &[],
    },
    SlashCommandSpec {
        command: "/provider",
        usage: "/provider [status|validate|smoke|policy]",
        description: "Inspect/configure provider; bare opens native provider panel",
        aliases: &[],
    },
    SlashCommandSpec {
        command: "/model",
        usage: "/model [id|role <role> <id|inherit>]",
        description: "Select session or role-scoped model",
        aliases: &[],
    },
    SlashCommandSpec {
        command: "/models",
        usage: "/models [filter]",
        description: "List known models",
        aliases: &[],
    },
    SlashCommandSpec {
        command: "/scoped-models",
        usage: "/scoped-models [list|enable <model>|disable <model>|clear]",
        description: "Manage model scope for /model and Ctrl+P cycling",
        aliases: &[],
    },
    SlashCommandSpec {
        command: "/roles",
        usage: "/roles [role [model|inherit]]",
        description: "Show or set role model profiles",
        aliases: &["/role-model"],
    },
    SlashCommandSpec {
        command: "/resume",
        usage: "/resume [thread-id]",
        description: "Resume a saved session, or list sessions for this project",
        aliases: &[],
    },
    SlashCommandSpec {
        command: "/new",
        usage: "/new [title]",
        description: "Start a new thread/session",
        aliases: &["/clear", "/reset"],
    },
    SlashCommandSpec {
        command: "/fork",
        usage: "/fork [title]",
        description: "Fork the current thread",
        aliases: &[],
    },
    SlashCommandSpec {
        command: "/tree",
        usage: "/tree [fold|unfold|toggle|rename|delete]",
        description: "Show or manage the thread/session tree; opens a picker in native TUI",
        aliases: &["/sessions"],
    },
    SlashCommandSpec {
        command: "/debug",
        usage: "/debug",
        description: "Print a redacted debug bundle",
        aliases: &[],
    },
    SlashCommandSpec {
        command: "/ui",
        usage: "/ui [width height]",
        description: "Render retained-frame debug output",
        aliases: &["/frame"],
    },
    SlashCommandSpec {
        command: "/keys",
        usage: "/keys",
        description: "Show keybinding and terminal capability notes",
        aliases: &["/keybindings", "/oppi-terminal-setup"],
    },
    SlashCommandSpec {
        command: "/steer",
        usage: "/steer <text>",
        description: "Send steering input to the active turn",
        aliases: &[],
    },
    SlashCommandSpec {
        command: "/interrupt",
        usage: "/interrupt",
        description: "Interrupt/cancel the active turn",
        aliases: &["/cancel"],
    },
    SlashCommandSpec {
        command: "/effort",
        usage: "/effort [off|minimal|low|medium|high|xhigh|auto]",
        description: "Set thinking effort with a model-aware slider",
        aliases: &[],
    },
    SlashCommandSpec {
        command: "/prompt-variant",
        usage: "/prompt-variant [off|a|b|caveman]",
        description: "Select prompt variant overlay",
        aliases: &["/variant"],
    },
    SlashCommandSpec {
        command: "/todos",
        usage: "/todos [clear|done [id]]",
        description: "Show todos or apply client-only clear/done actions",
        aliases: &[],
    },
    SlashCommandSpec {
        command: "/goal",
        usage: "/goal [objective|pause|resume|clear|done|budget]",
        description: "Track and continue a thread goal",
        aliases: &[],
    },
    SlashCommandSpec {
        command: "/suggest-next",
        usage: "/suggest-next [show|accept|clear|debug]",
        description: "Inspect or apply ghost next-message suggestions",
        aliases: &[],
    },
    SlashCommandSpec {
        command: "/usage",
        usage: "/usage",
        description: "Show native local usage/status snapshot",
        aliases: &["/stats"],
    },
    SlashCommandSpec {
        command: "/memory",
        usage: "/memory [status|dashboard|settings|maintenance]",
        description: "Control Hoppi memory; bare opens native memory panel",
        aliases: &["/mem", "/memory-maintenance", "/idle-compact"],
    },
    SlashCommandSpec {
        command: "/agents",
        usage: "/agents [list|dispatch|block|complete]",
        description: "List or dispatch native agents",
        aliases: &[],
    },
    SlashCommandSpec {
        command: "/skills",
        usage: "/skills",
        description: "List discovered skills",
        aliases: &[],
    },
    SlashCommandSpec {
        command: "/graphify",
        usage: "/graphify [status|install|commands]",
        description: "Inspect Graphify codebase context setup",
        aliases: &[],
    },
    SlashCommandSpec {
        command: "/theme",
        usage: "/theme [oppi|dark|light|plain|reload]",
        description: "Show/change theme; bare opens native theme panel",
        aliases: &["/themes"],
    },
    SlashCommandSpec {
        command: "/sandbox",
        usage: "/sandbox [mode]",
        description: "Show sandbox/permission status; bare opens native panel",
        aliases: &[],
    },
    SlashCommandSpec {
        command: "/permissions",
        usage: "/permissions [mode]",
        description: "Show/set permission mode; bare opens native panel",
        aliases: &[],
    },
    SlashCommandSpec {
        command: "/background",
        usage: "/background [list|read|kill]",
        description: "List/read/kill background shell tasks",
        aliases: &[],
    },
    SlashCommandSpec {
        command: "/review",
        usage: "/review [focus]",
        description: "Run the reviewer command profile",
        aliases: &[],
    },
    SlashCommandSpec {
        command: "/audit",
        usage: "/audit [focus]",
        description: "Run audit/reviewer flow",
        aliases: &[],
    },
    SlashCommandSpec {
        command: "/init",
        usage: "/init [context]",
        description: "Initialize/update project instructions",
        aliases: &[],
    },
    SlashCommandSpec {
        command: "/independent",
        usage: "/independent <plan/task>",
        description: "Run independent execution flow",
        aliases: &[],
    },
    SlashCommandSpec {
        command: "/bug-report",
        usage: "/bug-report <summary>",
        description: "Draft/submit structured bug feedback",
        aliases: &[],
    },
    SlashCommandSpec {
        command: "/feature-request",
        usage: "/feature-request <summary>",
        description: "Draft/submit structured feature feedback",
        aliases: &[],
    },
    SlashCommandSpec {
        command: "/approve",
        usage: "/approve",
        description: "Approve the pending tool action",
        aliases: &[],
    },
    SlashCommandSpec {
        command: "/deny",
        usage: "/deny",
        description: "Deny the pending tool action",
        aliases: &[],
    },
    SlashCommandSpec {
        command: "/answer",
        usage: "/answer <text-or-option-id>",
        description: "Answer a pending ask_user question",
        aliases: &[],
    },
    SlashCommandSpec {
        command: "/again",
        usage: "/again",
        description: "Repeat or queue the previous prompt",
        aliases: &[],
    },
    SlashCommandSpec {
        command: "/btw",
        usage: "/btw <question>",
        description: "Ask a side question without mutating the parent transcript",
        aliases: &[],
    },
    SlashCommandSpec {
        command: "/meridian",
        usage: "/meridian [status|install|start|use|stop]",
        description: "Manage the explicit Claude/Meridian provider bridge",
        aliases: &[],
    },
    SlashCommandSpec {
        command: "/runtime-loop",
        usage: "/runtime-loop [status]",
        description: "Show native Rust loop status; native shell already runs through Rust",
        aliases: &[],
    },
    SlashCommandSpec {
        command: "/exit",
        usage: "/exit",
        description: "Exit the shell safely",
        aliases: &["/quit"],
    },
];

#[derive(Debug, Clone, Copy)]
struct ParsedSlashBuffer<'a> {
    command_token: &'a str,
    args: &'a str,
    has_args: bool,
}

fn parse_slash_buffer(buffer: &str) -> Option<ParsedSlashBuffer<'_>> {
    let trimmed = buffer.trim_start();
    if !trimmed.starts_with('/') || trimmed.contains('\n') {
        return None;
    }
    if let Some(index) = trimmed.find(char::is_whitespace) {
        Some(ParsedSlashBuffer {
            command_token: &trimmed[..index],
            args: trimmed[index..].trim_start(),
            has_args: true,
        })
    } else {
        Some(ParsedSlashBuffer {
            command_token: trimmed,
            args: "",
            has_args: false,
        })
    }
}

fn find_slash_command_spec(command: &str) -> Option<&'static SlashCommandSpec> {
    let command = command.trim();
    SLASH_COMMAND_SPECS.iter().find(|spec| {
        spec.command.eq_ignore_ascii_case(command)
            || spec
                .aliases
                .iter()
                .any(|alias| alias.eq_ignore_ascii_case(command))
    })
}

fn is_exact_slash_command(command: &str) -> bool {
    find_slash_command_spec(command).is_some()
}

fn command_completion_text(spec: &SlashCommandSpec) -> String {
    if native_bare_picker_command(spec.command)
        || slash_arg_suggestions(spec.command, "").is_empty()
    {
        spec.command.to_string()
    } else {
        format!("{} ", spec.command)
    }
}

fn native_bare_picker_command(command: &str) -> bool {
    matches!(
        command,
        "/theme"
            | "/effort"
            | "/permissions"
            | "/sandbox"
            | "/provider"
            | "/login"
            | "/meridian"
            | "/memory"
            | "/model"
            | "/roles"
            | "/resume"
            | "/runtime-loop"
    )
}

fn command_spec_match_score(spec: &SlashCommandSpec, query: &str) -> Option<u8> {
    let raw = query.trim().to_ascii_lowercase();
    let query = raw.trim_start_matches('/');
    if query.is_empty() {
        return Some(0);
    }
    if spec
        .command
        .trim_start_matches('/')
        .to_ascii_lowercase()
        .starts_with(query)
    {
        return Some(0);
    }
    if spec.aliases.iter().any(|alias| {
        alias
            .trim_start_matches('/')
            .to_ascii_lowercase()
            .starts_with(query)
    }) {
        return Some(1);
    }
    if spec.usage.to_ascii_lowercase().contains(query) {
        return Some(2);
    }
    if spec.description.to_ascii_lowercase().contains(query) {
        return Some(3);
    }
    None
}

fn visible_slash_command_specs() -> Vec<&'static SlashCommandSpec> {
    SLASH_COMMAND_SPECS
        .iter()
        .filter(|spec| slash_command_visible_in_palette(spec.command))
        .collect()
}

fn slash_command_visible_in_palette(command: &str) -> bool {
    matches!(
        command,
        "/help"
            | "/settings"
            | "/login"
            | "/logout"
            | "/model"
            | "/resume"
            | "/new"
            | "/fork"
            | "/tree"
            | "/prompt-variant"
            | "/todos"
            | "/goal"
            | "/theme"
            | "/memory"
            | "/agents"
            | "/effort"
            | "/background"
            | "/review"
            | "/audit"
            | "/init"
            | "/independent"
            | "/bug-report"
            | "/feature-request"
            | "/permissions"
            | "/keys"
            | "/debug"
            | "/usage"
            | "/stats"
            | "/suggest-next"
            | "/btw"
            | "/meridian"
            | "/runtime-loop"
            | "/exit"
    )
}

fn slash_command_palette_items(query: &str) -> Vec<SlashPaletteItem> {
    let mut specs = visible_slash_command_specs()
        .into_iter()
        .filter_map(|spec| command_spec_match_score(spec, query).map(|score| (score, spec)))
        .collect::<Vec<_>>();
    specs.sort_by_key(|(score, spec)| (*score, spec.command.len(), spec.command));
    specs
        .into_iter()
        .map(|(_, spec)| {
            let alias_hint = if spec.aliases.is_empty() {
                String::new()
            } else {
                format!(" aliases: {}", spec.aliases.join(", "))
            };
            SlashPaletteItem {
                label: spec.usage.to_string(),
                insert: command_completion_text(spec),
                detail: format!("{}{}", spec.description, alias_hint),
            }
        })
        .collect()
}

fn arg_item(label: &str, insert: &str, detail: &str) -> SlashPaletteItem {
    SlashPaletteItem {
        label: label.to_string(),
        insert: insert.to_string(),
        detail: detail.to_string(),
    }
}

fn slash_arg_suggestions(command: &str, args: &str) -> Vec<SlashPaletteItem> {
    let mut items = match command {
        "/login" => vec![
            arg_item(
                "subscription",
                "/login subscription",
                "Open subscription choices",
            ),
            arg_item("api", "/login api", "Open API/env-ref setup"),
            arg_item(
                "api openai env",
                "/login api openai env ",
                "Set env-ref credential name",
            ),
            arg_item(
                "subscription claude status",
                "/login subscription claude status",
                "Check Meridian bridge",
            ),
            arg_item(
                "subscription claude install",
                "/login subscription claude install ",
                "Requires --yes/approval",
            ),
            arg_item(
                "subscription claude start",
                "/login subscription claude start",
                "Start explicit Meridian bridge",
            ),
            arg_item(
                "subscription claude use",
                "/login subscription claude use",
                "Use Meridian; model stays in /model",
            ),
            arg_item(
                "subscription claude stop",
                "/login subscription claude stop",
                "Stop shell-owned Meridian",
            ),
            arg_item(
                "codex/chatgpt",
                "/login subscription codex",
                "Start native ChatGPT/Codex OAuth login",
            ),
            arg_item(
                "copilot",
                "/login subscription copilot",
                "Start GitHub Copilot device-code OAuth login",
            ),
        ],
        "/logout" => vec![
            arg_item(
                "provider",
                "/logout provider",
                "Clear session direct provider",
            ),
            arg_item(
                "anthropic",
                "/logout anthropic",
                "Stop shell-owned Meridian",
            ),
        ],
        "/provider" => vec![
            arg_item(
                "status",
                "/provider status",
                "Local/redacted provider status",
            ),
            arg_item(
                "validate",
                "/provider validate",
                "Local readiness check; no live call",
            ),
            arg_item(
                "smoke",
                "/provider smoke ",
                "Run explicit live smoke prompt",
            ),
            arg_item(
                "auth-env",
                "/provider auth-env ",
                "Set env-ref credential name",
            ),
            arg_item(
                "base-url",
                "/provider base-url ",
                "Set compatible API base URL",
            ),
            arg_item("policy", "/provider policy", "Show provider safety policy"),
            arg_item(
                "anthropic",
                "/provider anthropic",
                "Explain Anthropic/Meridian stance",
            ),
        ],
        "/model" => {
            let mut items = vec![
                arg_item("<model-id>", "/model ", "Set the session model id"),
                arg_item("role <role>", "/model role ", "Set a role-scoped model"),
            ];
            for role in ROLE_NAMES {
                items.push(arg_item(
                    &format!("role {role}"),
                    &format!("/model role {role} "),
                    "Set this role model id or inherit",
                ));
                items.push(arg_item(
                    &format!("role {role} inherit"),
                    &format!("/model role {role} inherit"),
                    "Clear this role override",
                ));
            }
            items
        }
        "/roles" | "/role-model" => {
            let mut items = vec![arg_item("all roles", "/roles", "Show all role profiles")];
            for role in ROLE_NAMES {
                items.push(arg_item(
                    role,
                    &format!("/roles {role}"),
                    "Show role profile",
                ));
                items.push(arg_item(
                    &format!("{role} inherit"),
                    &format!("/roles {role} inherit"),
                    "Clear role override",
                ));
            }
            items
        }
        "/permissions" | "/sandbox" => vec![
            arg_item(
                "read-only",
                &format!("{command} read-only"),
                "Block writes/network",
            ),
            arg_item(
                "default",
                &format!("{command} default"),
                "Default OPPi policy",
            ),
            arg_item(
                "auto-review",
                &format!("{command} auto-review"),
                "Review risky actions",
            ),
            arg_item(
                "full-access",
                &format!("{command} full-access"),
                "Unrestricted turn policy",
            ),
        ],
        "/theme" => vec![
            arg_item("oppi", "/theme oppi", "OPPi theme"),
            arg_item("dark", "/theme dark", "Dark ANSI theme"),
            arg_item("light", "/theme light", "Light ANSI theme"),
            arg_item("plain", "/theme plain", "No-color/plain theme"),
            arg_item("reload", "/theme reload", "Reload .oppi/theme.txt"),
        ],
        "/prompt-variant" | "/variant" => vec![
            arg_item("off", "/prompt-variant off", "Disable variant overlay"),
            arg_item("a", "/prompt-variant a", "Variant A"),
            arg_item("b", "/prompt-variant b", "Variant B"),
            arg_item("caveman", "/prompt-variant caveman", "Caveman overlay"),
        ],
        "/effort" => vec![
            arg_item(
                "auto",
                "/effort auto",
                "Use recommended effort for current model",
            ),
            arg_item("off", "/effort off", "Disable explicit reasoning"),
            arg_item(
                "minimal",
                "/effort minimal",
                "Fastest reasoning-capable setting",
            ),
            arg_item("low", "/effort low", "Low reasoning effort"),
            arg_item("medium", "/effort medium", "Medium reasoning effort"),
            arg_item("high", "/effort high", "High reasoning effort"),
            arg_item("xhigh", "/effort xhigh", "Maximum effort where supported"),
        ],
        "/goal" => vec![
            arg_item("pause", "/goal pause", "Pause active goal"),
            arg_item("resume", "/goal resume", "Resume paused goal"),
            arg_item("clear", "/goal clear", "Clear current goal"),
            arg_item("done", "/goal done", "Mark current goal complete"),
            arg_item("budget", "/goal budget ", "Set token budget"),
            arg_item("budget clear", "/goal budget clear", "Clear token budget"),
            arg_item("replace", "/goal replace ", "Replace active goal"),
        ],
        "/background" => vec![
            arg_item(
                "list",
                "/background list",
                "List process-local background tasks",
            ),
            arg_item("read", "/background read", "Read latest task output"),
            arg_item(
                "read latest",
                "/background read latest",
                "Read latest task tail",
            ),
            arg_item("kill", "/background kill", "Kill latest running task"),
            arg_item(
                "kill latest",
                "/background kill latest",
                "Kill latest running task",
            ),
        ],
        "/memory" | "/mem" => vec![
            arg_item("status", "/memory status", "Show memory status"),
            arg_item("on", "/memory on", "Enable client-hosted memory"),
            arg_item("off", "/memory off", "Disable memory"),
            arg_item("dashboard", "/memory dashboard", "Open memory dashboard"),
            arg_item("settings", "/memory settings", "Open memory settings"),
            arg_item(
                "maintenance dry-run",
                "/memory maintenance dry-run",
                "Preview maintenance",
            ),
            arg_item(
                "maintenance apply",
                "/memory maintenance apply",
                "Apply explicit maintenance",
            ),
            arg_item("compact", "/memory compact ", "Record a compaction summary"),
        ],
        "/agents" => vec![
            arg_item("list", "/agents list", "List agents and dispatch profile"),
            arg_item("dispatch", "/agents dispatch ", "Dispatch <agent> <task>"),
            arg_item("block", "/agents block ", "Block <run-id> <reason>"),
            arg_item(
                "complete",
                "/agents complete ",
                "Complete <run-id> <output>",
            ),
            arg_item(
                "import",
                "/agents import ",
                "Import project/user agent markdown",
            ),
            arg_item(
                "export",
                "/agents export ",
                "Export an agent markdown definition",
            ),
        ],
        "/graphify" => vec![
            arg_item(
                "status",
                "/graphify status",
                "Detect Graphify setup/artifacts",
            ),
            arg_item(
                "install",
                "/graphify install",
                "Show user-approved install guidance",
            ),
            arg_item("commands", "/graphify commands", "Show Graphify commands"),
        ],
        "/suggest-next" => vec![
            arg_item(
                "show",
                "/suggest-next show",
                "Show current ghost suggestion",
            ),
            arg_item(
                "accept",
                "/suggest-next accept",
                "Accept into editor in native TUI",
            ),
            arg_item(
                "clear",
                "/suggest-next clear",
                "Clear current ghost suggestion",
            ),
            arg_item("debug", "/suggest-next debug", "Show confidence and reason"),
        ],
        "/usage" | "/stats" => vec![arg_item(
            "status",
            "/usage",
            "Show local usage/status snapshot",
        )],
        "/todos" => vec![
            arg_item("clear", "/todos clear", "Clear the client-side todo list"),
            arg_item("done", "/todos done", "Mark active todos completed"),
            arg_item("done <id>", "/todos done ", "Mark one todo id completed"),
        ],
        "/meridian" => vec![
            arg_item(
                "status",
                "/meridian status",
                "Check Meridian bridge readiness",
            ),
            arg_item(
                "install",
                "/meridian install ",
                "Install managed bridge after explicit approval",
            ),
            arg_item("start", "/meridian start", "Start visible loopback bridge"),
            arg_item("use", "/meridian use", "Use already-running bridge"),
            arg_item("stop", "/meridian stop", "Stop shell-owned bridge"),
        ],
        "/runtime-loop" => vec![arg_item(
            "status",
            "/runtime-loop status",
            "Show native Rust loop status",
        )],
        "/models" => vec![arg_item("<filter>", "/models ", "Filter known model ids")],
        "/scoped-models" => vec![
            arg_item("list", "/scoped-models list", "Show current model scope"),
            arg_item("enable", "/scoped-models enable ", "Add a model to scope"),
            arg_item(
                "disable",
                "/scoped-models disable ",
                "Remove a model from scope",
            ),
            arg_item("clear", "/scoped-models clear", "Use all available models"),
        ],
        "/resume" => vec![arg_item(
            "<thread-id>",
            "/resume ",
            "Paste a thread id; /resume opens picker in native TUI",
        )],
        "/tree" | "/sessions" => vec![
            arg_item("fold", "/tree fold ", "Collapse a fork subtree"),
            arg_item("unfold", "/tree unfold ", "Expand a fork subtree"),
            arg_item("toggle", "/tree toggle ", "Toggle a fork subtree"),
            arg_item("rename", "/sessions rename ", "Rename a saved session"),
            arg_item("delete", "/sessions delete ", "Archive a saved session"),
        ],
        "/new" => vec![arg_item("[title]", "/new ", "Optional session title")],
        "/fork" => vec![arg_item("[title]", "/fork ", "Optional fork title")],
        "/ui" | "/frame" => vec![
            arg_item("width height", "/ui 100 30", "Preview retained frame size"),
            arg_item(
                "ratatui width height",
                "/ui ratatui 100 30",
                "Preview Ratatui frame size",
            ),
        ],
        "/steer" => vec![arg_item("<text>", "/steer ", "Steer the active turn")],
        "/answer" => vec![arg_item(
            "<text-or-option-id>",
            "/answer ",
            "Answer pending question",
        )],
        "/review" => vec![arg_item("[focus]", "/review ", "Optional review focus")],
        "/audit" => vec![arg_item("[focus]", "/audit ", "Optional audit focus")],
        "/btw" => vec![arg_item(
            "[question]",
            "/btw ",
            "Ask a one-shot side question",
        )],
        "/init" => vec![arg_item("[context]", "/init ", "Optional init context")],
        "/independent" => vec![arg_item(
            "<plan/task>",
            "/independent ",
            "Plan or checklist to execute",
        )],
        "/bug-report" => vec![arg_item("<summary>", "/bug-report ", "Bug summary/details")],
        "/feature-request" => vec![arg_item(
            "<summary>",
            "/feature-request ",
            "Feature summary/details",
        )],
        _ => Vec::new(),
    };
    let needle = args.trim_start().to_ascii_lowercase();
    if !needle.is_empty() {
        items.retain(|item| {
            let remainder = item
                .insert
                .strip_prefix(command)
                .unwrap_or(item.insert.as_str())
                .trim_start()
                .to_ascii_lowercase();
            remainder.starts_with(&needle) || item.label.to_ascii_lowercase().starts_with(&needle)
        });
    }
    items
}

fn effort_arg_suggestions(provider: &ProviderConfig, args: &str) -> Vec<SlashPaletteItem> {
    let recommended = recommended_effort_level_for_provider(provider);
    let current = current_effort_level_for_provider(provider);
    let mut items = vec![arg_item(
        "auto",
        "/effort auto",
        &format!(
            "Use recommended effort ({}) for current model",
            effort_level_label_for_provider(provider, recommended)
        ),
    )];
    for level in allowed_effort_levels_for_provider(provider) {
        let mut detail = level.description().to_string();
        if level == current {
            detail.push_str(" (current)");
        }
        if level == recommended {
            detail.push_str(" (recommended)");
        }
        items.push(arg_item(
            level.as_str(),
            &format!("/effort {}", level.as_str()),
            &detail,
        ));
    }

    let needle = args.trim_start().to_ascii_lowercase();
    if !needle.is_empty() {
        items.retain(|item| {
            let remainder = item
                .insert
                .strip_prefix("/effort")
                .unwrap_or(item.insert.as_str())
                .trim_start()
                .to_ascii_lowercase();
            remainder.starts_with(&needle) || item.label.to_ascii_lowercase().starts_with(&needle)
        });
    }
    items
}

fn slash_palette_for_buffer(buffer: &str, selected: usize) -> Option<SlashPalette> {
    let parsed = parse_slash_buffer(buffer)?;
    if !parsed.has_args {
        let items = slash_command_palette_items(parsed.command_token);
        if items.is_empty() {
            return None;
        }
        let selected = selected.min(items.len().saturating_sub(1));
        return Some(SlashPalette {
            mode: SlashPaletteMode::Commands,
            query: parsed.command_token.to_string(),
            title: format!("slash commands ({})", visible_slash_command_specs().len()),
            hint: "type to filter • ↑↓/PgUp/PgDn choose • Tab completes • Enter chooses/runs"
                .to_string(),
            items,
            selected,
        });
    }

    let Some(spec) = find_slash_command_spec(parsed.command_token) else {
        let items = slash_command_palette_items(parsed.command_token);
        if items.is_empty() {
            return None;
        }
        let selected = selected.min(items.len().saturating_sub(1));
        return Some(SlashPalette {
            mode: SlashPaletteMode::Commands,
            query: parsed.command_token.to_string(),
            title: "matching slash commands".to_string(),
            hint: "unknown command • ↑↓ choose • Tab/Enter inserts selected command".to_string(),
            items,
            selected,
        });
    };

    let items = slash_arg_suggestions(spec.command, parsed.args);
    if items.is_empty() {
        return None;
    }
    let selected = selected.min(items.len().saturating_sub(1));
    Some(SlashPalette {
        mode: SlashPaletteMode::Arguments,
        query: parsed.args.to_string(),
        title: format!("{} arguments", spec.command),
        hint: format!(
            "usage: {} • ↑↓ choose • Tab completes • Enter chooses/runs",
            spec.usage
        ),
        items,
        selected,
    })
}

#[cfg(test)]
fn slash_palette_accept(
    buffer: &str,
    selected: usize,
    navigated: bool,
    submit_allowed: bool,
) -> Option<SlashPaletteAccept> {
    let palette = slash_palette_for_buffer(buffer, selected)?;
    slash_palette_accept_from_palette(buffer, &palette, navigated, submit_allowed)
}

fn slash_palette_accept_from_palette(
    buffer: &str,
    palette: &SlashPalette,
    navigated: bool,
    submit_allowed: bool,
) -> Option<SlashPaletteAccept> {
    let item = palette.items.get(palette.selected)?;
    let submit = submit_allowed && slash_palette_item_can_submit(item);
    match palette.mode {
        SlashPaletteMode::Commands => {
            if !navigated && is_exact_slash_command(&palette.query) {
                None
            } else if submit {
                Some(SlashPaletteAccept::Submit(item.insert.clone()))
            } else {
                Some(SlashPaletteAccept::Insert(item.insert.clone()))
            }
        }
        SlashPaletteMode::Arguments => {
            let current = buffer.trim_start().trim_end();
            let inserted = item.insert.trim_end();
            if !navigated && current == inserted && !item.insert.ends_with(' ') {
                None
            } else if submit {
                Some(SlashPaletteAccept::Submit(item.insert.clone()))
            } else {
                Some(SlashPaletteAccept::Insert(item.insert.clone()))
            }
        }
    }
}

fn slash_palette_for_buffer_with_session(
    buffer: &str,
    selected: usize,
    session: &ShellSession,
    provider: &ProviderConfig,
) -> Option<SlashPalette> {
    let parsed = parse_slash_buffer(buffer)?;
    let mut palette = slash_palette_for_buffer(buffer, selected)?;
    if palette.mode != SlashPaletteMode::Arguments {
        return Some(palette);
    }
    let spec = find_slash_command_spec(parsed.command_token)?;
    let dynamic = slash_dynamic_arg_suggestions(spec.command, parsed.args, session, provider);
    if dynamic.is_empty() {
        return Some(palette);
    }
    if spec.command == "/effort" {
        palette.items = dynamic;
        palette.selected = palette.selected.min(palette.items.len().saturating_sub(1));
        return Some(palette);
    }
    let mut seen = BTreeSet::new();
    let mut merged = Vec::new();
    for item in dynamic.into_iter().chain(palette.items.into_iter()) {
        if seen.insert(item.insert.clone()) {
            merged.push(item);
        }
    }
    palette.items = merged;
    palette.selected = selected.min(palette.items.len().saturating_sub(1));
    Some(palette)
}

fn slash_dynamic_arg_suggestions(
    command: &str,
    args: &str,
    session: &ShellSession,
    provider: &ProviderConfig,
) -> Vec<SlashPaletteItem> {
    let needle = args.trim_start().to_ascii_lowercase();
    match command {
        "/model" if !needle.starts_with("role ") => main_model_ids_for_provider(session, provider)
            .into_iter()
            .filter(|model| needle.is_empty() || model.to_ascii_lowercase().contains(&needle))
            .take(12)
            .map(|model| {
                let detail = if session.session_model(provider) == Some(model.as_str()) {
                    "current main model"
                } else {
                    "available for current provider"
                };
                arg_item(&model, &format!("/model {model}"), detail)
            })
            .collect(),
        "/answer" => session
            .pending_question
            .as_ref()
            .map(|pending| {
                pending
                    .request
                    .questions
                    .iter()
                    .flat_map(|question| {
                        question.options.iter().map(move |option| {
                            let label = format!("{} — {}", option.id, option.label);
                            let detail = option
                                .description
                                .as_deref()
                                .unwrap_or(question.question.as_str());
                            arg_item(&label, &format!("/answer {}", option.id), detail)
                        })
                    })
                    .filter(|item| {
                        needle.is_empty()
                            || item.label.to_ascii_lowercase().contains(&needle)
                            || item.insert.to_ascii_lowercase().contains(&needle)
                    })
                    .collect()
            })
            .unwrap_or_default(),
        "/permissions" | "/sandbox" => {
            let current = session.permission_mode.as_str();
            slash_arg_suggestions(command, args)
                .into_iter()
                .map(|mut item| {
                    if item.insert.ends_with(current) {
                        item.detail.push_str(" (current)");
                    }
                    item
                })
                .collect()
        }
        "/theme" => slash_arg_suggestions(command, args)
            .into_iter()
            .map(|mut item| {
                if item.insert.ends_with(&session.theme) {
                    item.detail.push_str(" (current)");
                }
                item
            })
            .collect(),
        "/effort" => effort_arg_suggestions(provider, args),
        _ => Vec::new(),
    }
}

fn slash_palette_item_can_submit(item: &SlashPaletteItem) -> bool {
    let insert = item.insert.trim();
    !item.insert.ends_with(' ')
        && !insert.is_empty()
        && !insert.contains('<')
        && !item.label.contains('<')
        && !item.label.starts_with('[')
}

fn slash_command_help_text() -> String {
    let mut lines = vec![
        "slash commands:".to_string(),
        "TUI: type `/` for the OPPi command palette; five rows show at a time; ↑↓/PgUp/PgDn selects; Tab completes; Enter chooses/runs when complete. Type after a command to see argument hints.".to_string(),
    ];
    for spec in visible_slash_command_specs() {
        let aliases = if spec.aliases.is_empty() {
            String::new()
        } else {
            format!(" (aliases: {})", spec.aliases.join(", "))
        };
        lines.push(format!(
            "- {:<44} {}{}",
            spec.usage, spec.description, aliases
        ));
    }
    lines.join("\n")
}

fn main() {
    if let Err(error) = run(env::args().skip(1).collect()) {
        eprintln!("oppi-shell: {error}");
        std::process::exit(1);
    }
}

fn run(args: Vec<String>) -> Result<(), String> {
    let command = parse_args(args)?;
    if command.list_sessions {
        return list_sessions_command(
            command.server.clone().unwrap_or_else(default_server_path),
            command.json,
        );
    }
    let mut provider = command.provider.clone();
    let mut session = ShellSession::connect(
        command.server.clone().unwrap_or_else(default_server_path),
        command.resume_thread.clone(),
    )?;
    if let Some(saved_effort) = load_reasoning_effort_setting(&session.role_profile_path)
        && matches!(&provider, ProviderConfig::OpenAiCompatible(config) if config.reasoning_effort.is_none())
        && allowed_effort_levels_for_provider(&provider).contains(&saved_effort)
    {
        set_provider_effort_level(&mut provider, saved_effort);
    }
    session.register_provider_model(&provider)?;
    if !command.json {
        println!(
            "OPPi Rust shell ready ({}). Type a prompt, /help, /exit, or Ctrl+C twice in native/raw mode to stop.",
            provider.label()
        );
    }

    if let Some(prompt) = command.initial_prompt.as_deref() {
        let outcome =
            session.run_turn_for_role(prompt, &provider, command.json, Some("executor"))?;
        if !command.interactive {
            session.shutdown();
            outcome.into_result()?;
            return Ok(());
        }
    }

    if command.interactive {
        interactive_loop(
            &mut session,
            &mut provider,
            command.json,
            command.raw,
            command.ratatui,
        )?;
    }

    session.shutdown();
    Ok(())
}

fn interactive_loop(
    session: &mut ShellSession,
    provider: &mut ProviderConfig,
    json: bool,
    raw: bool,
    ratatui: bool,
) -> Result<(), String> {
    if raw && !json && io::stdin().is_terminal() && io::stdout().is_terminal() {
        if ratatui {
            return ratatui_ui::ratatui_interactive_loop(session, provider);
        }
        return tui::retained_tui_interactive_loop(session, provider);
    }
    line_interactive_loop(session, provider, json)
}

fn line_interactive_loop(
    session: &mut ShellSession,
    provider: &mut ProviderConfig,
    json: bool,
) -> Result<(), String> {
    let stdin = io::stdin();
    let mut input = String::new();
    let mut editor = LineEditor::default();
    loop {
        if !json {
            session.render_docks();
            print!("\noppi> ");
            io::stdout()
                .flush()
                .map_err(|error| format!("flush prompt: {error}"))?;
        }
        input.clear();
        let bytes = stdin
            .read_line(&mut input)
            .map_err(|error| format!("read prompt: {error}"))?;
        let action = if bytes == 0 {
            editor.handle(EditorInput::CtrlD)
        } else {
            editor.handle(EditorInput::Text(
                input.trim_end_matches(['\r', '\n']).to_string(),
            ));
            editor.handle(EditorInput::Enter)
        };
        match action {
            EditorAction::Exit => break,
            EditorAction::Submit(prompt) => {
                let prompt = prompt.trim();
                if prompt.is_empty() {
                    continue;
                }
                if prompt.starts_with('/') {
                    if !session.handle_command(prompt, provider, json)? {
                        break;
                    }
                    session.drain_follow_ups(provider, json)?;
                    continue;
                }
                if session.handle_login_picker_input(prompt, provider, json)? {
                    continue;
                }
                if session.is_turn_running() || session.has_pending_pause() {
                    session.queue_follow_up(prompt, json)?;
                    continue;
                }
                let _ = session.run_turn_for_role(prompt, provider, json, Some("executor"))?;
                session.drain_follow_ups(provider, json)?;
            }
            EditorAction::SubmitFollowUp(prompt) => {
                session.queue_follow_up(prompt.trim(), json)?;
                session.drain_follow_ups(provider, json)?;
            }
            EditorAction::Steer(prompt) => {
                session.steer_active_turn(prompt.trim(), json)?;
            }
            EditorAction::RestoreQueued => {
                if let Some(restored) = session.restore_latest_follow_up() {
                    editor.replace_buffer(restored);
                    session.print_text("restored queued follow-up into editor", json)?;
                } else {
                    session.print_text("no queued follow-up to restore", json)?;
                }
            }
            EditorAction::OpenSettings => {
                session.print_text("settings subpanels require the retained TUI; run without --no-tui or use /theme, /permissions, /model, /provider, /login, /tree, and /memory commands directly", json)?;
            }
            EditorAction::Interrupt => {
                session.interrupt_active_turn(json)?;
            }
            EditorAction::None | EditorAction::Cleared => continue,
        }
    }
    Ok(())
}

#[allow(dead_code)]
fn raw_interactive_loop(
    session: &mut ShellSession,
    provider: &mut ProviderConfig,
    json: bool,
) -> Result<(), String> {
    if !json {
        println!(
            "raw key mode: Enter submits/queues while busy, Shift+Enter newline, Alt+Enter queues, Ctrl+Enter steers, Alt+Up restores queued follow-up, Escape/Ctrl+C interrupt-or-clear, Ctrl+C twice runs /exit, Ctrl+D exits. {}",
            terminal_capability_summary(true)
        );
    }
    let (tx, rx) = mpsc::channel::<EditorInput>();
    spawn_raw_input_thread(tx);
    let mut editor = LineEditor::default();
    let mut input_open = true;
    loop {
        match rx.recv_timeout(POLL_INTERVAL) {
            Ok(input) => {
                let action = editor.handle(input);
                if !handle_raw_editor_action(session, provider, json, &mut editor, action)? {
                    break;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => input_open = false,
        }

        if let Some(outcome) = session.poll_turn_events(json)? {
            if matches!(outcome, TurnOutcome::Completed) {
                session.start_next_queued_or_goal_continuation(provider, json)?;
            }
        } else if !session.is_turn_running() && !session.has_pending_pause() {
            session.start_next_queued_or_goal_continuation(provider, json)?;
        }

        if !input_open && !session.is_turn_running() {
            break;
        }
    }
    Ok(())
}

#[allow(dead_code)]
fn handle_raw_editor_action(
    session: &mut ShellSession,
    provider: &mut ProviderConfig,
    json: bool,
    editor: &mut LineEditor,
    action: EditorAction,
) -> Result<bool, String> {
    match action {
        EditorAction::None => Ok(true),
        EditorAction::Cleared => {
            session.print_text("editor cleared", json)?;
            Ok(true)
        }
        EditorAction::Exit => {
            if session.is_turn_running() {
                session.print_text(
                    "turn still running; press Ctrl+C/Escape to interrupt before Ctrl+D exit",
                    json,
                )?;
                Ok(true)
            } else {
                Ok(false)
            }
        }
        EditorAction::Submit(prompt) => {
            let prompt = prompt.trim();
            if prompt.is_empty() {
                return Ok(true);
            }
            if prompt.starts_with('/') {
                return session.handle_command(prompt, provider, json);
            }
            if session.handle_login_picker_input(prompt, provider, json)? {
                return Ok(true);
            }
            if session.is_turn_running() || session.has_pending_pause() {
                session.queue_follow_up(prompt, json)?;
            } else {
                session.start_turn_for_role(prompt, provider, json, Some("executor"))?;
            }
            Ok(true)
        }
        EditorAction::SubmitFollowUp(prompt) => {
            session.queue_follow_up(prompt.trim(), json)?;
            if !session.is_turn_running() && !session.has_pending_pause() {
                session.start_next_queued_follow_up(provider, json)?;
            }
            Ok(true)
        }
        EditorAction::Steer(prompt) => {
            session.steer_active_turn(prompt.trim(), json)?;
            Ok(true)
        }
        EditorAction::RestoreQueued => {
            if let Some(restored) = session.restore_latest_follow_up() {
                editor.replace_buffer(restored);
                session.print_text(
                    &format!(
                        "restored queued follow-up into editor: {}",
                        editor.buffer_preview()
                    ),
                    json,
                )?;
            } else {
                session.print_text("no queued follow-up to restore", json)?;
            }
            Ok(true)
        }
        EditorAction::OpenSettings => {
            session.print_text(
                "settings panel is available in the retained TUI; use /settings or Ctrl+P there",
                json,
            )?;
            Ok(true)
        }
        EditorAction::Interrupt => {
            session.interrupt_active_turn(json)?;
            Ok(true)
        }
    }
}

#[allow(dead_code)]
fn spawn_raw_input_thread(tx: mpsc::Sender<EditorInput>) {
    std::thread::spawn(move || {
        let stdin = io::stdin();
        let mut parser = RawInputParser::default();
        for byte in stdin.lock().bytes() {
            let Ok(byte) = byte else {
                break;
            };
            for input in parser.push(byte) {
                if tx.send(input).is_err() {
                    return;
                }
            }
        }
        for input in parser.finish() {
            if tx.send(input).is_err() {
                return;
            }
        }
    });
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
enum EditorInput {
    Text(String),
    Enter,
    ShiftEnter,
    AltEnter,
    CtrlEnter,
    AltUp,
    Escape,
    CtrlC,
    CtrlD,
    Backspace,
    CtrlBackspace,
    AltBackspace,
    Delete,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
    Tab,
    BackTab,
    CtrlP,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum EditorAction {
    None,
    Submit(String),
    SubmitFollowUp(String),
    Steer(String),
    RestoreQueued,
    OpenSettings,
    Interrupt,
    Cleared,
    Exit,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct LineEditor {
    buffer: String,
    cursor: usize,
    history: Vec<String>,
    history_index: Option<usize>,
    ctrl_c_armed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EditorActionKind {
    Normal,
    FollowUp,
    Steer,
}

#[derive(Debug, Default)]
struct RawInputParser {
    escape: Vec<u8>,
}

impl RawInputParser {
    fn push(&mut self, byte: u8) -> Vec<EditorInput> {
        if !self.escape.is_empty() {
            self.escape.push(byte);
            if let Some(input) = parse_escape_sequence(&self.escape) {
                self.escape.clear();
                return vec![input];
            }
            if !escape_sequence_may_continue(&self.escape) {
                let mut output = vec![EditorInput::Escape];
                let replay = self.escape.split_off(1);
                self.escape.clear();
                for byte in replay {
                    output.extend(self.push(byte));
                }
                return output;
            }
            return Vec::new();
        }

        match byte {
            0x03 => vec![EditorInput::CtrlC],
            0x04 => vec![EditorInput::CtrlD],
            0x17 => vec![EditorInput::CtrlBackspace],
            0x08 | 0x7f => vec![EditorInput::Backspace],
            0x09 => vec![EditorInput::Tab],
            0x0d | 0x0a => vec![EditorInput::Enter],
            0x1b => {
                self.escape.push(byte);
                Vec::new()
            }
            byte if byte.is_ascii_control() => Vec::new(),
            byte => vec![EditorInput::Text((byte as char).to_string())],
        }
    }

    fn finish(&mut self) -> Vec<EditorInput> {
        if self.escape.is_empty() {
            Vec::new()
        } else {
            self.escape.clear();
            vec![EditorInput::Escape]
        }
    }
}

fn parse_escape_sequence(bytes: &[u8]) -> Option<EditorInput> {
    match bytes {
        [0x1b, 0x0d] | [0x1b, 0x0a] => Some(EditorInput::AltEnter),
        [0x1b, b'[', b'A'] => Some(EditorInput::Up),
        [0x1b, b'[', b'B'] => Some(EditorInput::Down),
        [0x1b, b'[', b'C'] => Some(EditorInput::Right),
        [0x1b, b'[', b'D'] => Some(EditorInput::Left),
        [0x1b, b'[', b'H'] | [0x1b, b'O', b'H'] => Some(EditorInput::Home),
        [0x1b, b'[', b'F'] | [0x1b, b'O', b'F'] => Some(EditorInput::End),
        [0x1b, 0x08] | [0x1b, 0x7f] => Some(EditorInput::AltBackspace),
        [0x1b, b'[', b'1', b'2', b'7', b';', b'5', b'u'] | [0x1b, b'[', b'8', b';', b'5', b'u'] => {
            Some(EditorInput::CtrlBackspace)
        }
        [0x1b, b'[', b'3', b'~'] => Some(EditorInput::Delete),
        [0x1b, b'[', b'5', b'~'] => Some(EditorInput::PageUp),
        [0x1b, b'[', b'6', b'~'] => Some(EditorInput::PageDown),
        [0x1b, b'[', b'Z'] => Some(EditorInput::BackTab),
        [0x1b, 0x1b, b'[', b'A'] | [0x1b, b'[', b'1', b';', b'3', b'A'] => Some(EditorInput::AltUp),
        [0x1b, b'[', b'1', b'3', b';', b'2', b'u']
        | [0x1b, b'[', b'2', b'7', b';', b'2', b';', b'1', b'3', b'~'] => {
            Some(EditorInput::ShiftEnter)
        }
        [0x1b, b'[', b'1', b'3', b';', b'5', b'u']
        | [0x1b, b'[', b'2', b'7', b';', b'5', b';', b'1', b'3', b'~'] => {
            Some(EditorInput::CtrlEnter)
        }
        _ => None,
    }
}

fn escape_sequence_may_continue(bytes: &[u8]) -> bool {
    const CANDIDATES: [&[u8]; 23] = [
        &[0x1b, 0x0d],
        &[0x1b, 0x0a],
        &[0x1b, b'[', b'A'],
        &[0x1b, b'[', b'B'],
        &[0x1b, b'[', b'C'],
        &[0x1b, b'[', b'D'],
        &[0x1b, b'[', b'H'],
        &[0x1b, b'[', b'F'],
        &[0x1b, b'O', b'H'],
        &[0x1b, b'O', b'F'],
        &[0x1b, 0x08],
        &[0x1b, 0x7f],
        &[0x1b, b'[', b'1', b'2', b'7', b';', b'5', b'u'],
        &[0x1b, b'[', b'8', b';', b'5', b'u'],
        &[0x1b, b'[', b'3', b'~'],
        &[0x1b, b'[', b'5', b'~'],
        &[0x1b, b'[', b'6', b'~'],
        &[0x1b, b'[', b'Z'],
        &[0x1b, 0x1b, b'[', b'A'],
        &[0x1b, b'[', b'1', b';', b'3', b'A'],
        &[0x1b, b'[', b'1', b'3', b';', b'2', b'u'],
        &[0x1b, b'[', b'1', b'3', b';', b'5', b'u'],
        &[0x1b, b'[', b'2', b'7'],
    ];
    CANDIDATES
        .iter()
        .any(|candidate| candidate.starts_with(bytes))
        || matches!(
            bytes,
            [0x1b, b'[', b'2']
                | [0x1b, b'[', b'2', b'7']
                | [0x1b, b'[', b'2', b'7', b';']
                | [0x1b, b'[', b'2', b'7', b';', b'2']
                | [0x1b, b'[', b'2', b'7', b';', b'5']
                | [0x1b, b'[', b'2', b'7', b';', b'2', b';']
                | [0x1b, b'[', b'2', b'7', b';', b'5', b';']
                | [0x1b, b'[', b'2', b'7', b';', b'2', b';', b'1']
                | [0x1b, b'[', b'2', b'7', b';', b'5', b';', b'1']
                | [0x1b, b'[', b'2', b'7', b';', b'2', b';', b'1', b'3']
                | [0x1b, b'[', b'2', b'7', b';', b'5', b';', b'1', b'3']
        )
}

impl LineEditor {
    fn handle(&mut self, input: EditorInput) -> EditorAction {
        if !matches!(input, EditorInput::CtrlC) {
            self.ctrl_c_armed = false;
        }
        match input {
            EditorInput::Text(text) => {
                self.insert_text(&text);
                EditorAction::None
            }
            EditorInput::ShiftEnter => {
                self.insert_text("\n");
                EditorAction::None
            }
            EditorInput::Enter => self.submit_as(EditorActionKind::Normal),
            EditorInput::AltEnter => self.submit_as(EditorActionKind::FollowUp),
            EditorInput::CtrlEnter => self.submit_as(EditorActionKind::Steer),
            EditorInput::AltUp => EditorAction::RestoreQueued,
            EditorInput::CtrlP => EditorAction::OpenSettings,
            EditorInput::Backspace => {
                self.delete_backward();
                EditorAction::None
            }
            EditorInput::CtrlBackspace => {
                self.delete_word_backward();
                EditorAction::None
            }
            EditorInput::AltBackspace => {
                self.delete_current_line();
                EditorAction::None
            }
            EditorInput::Delete => {
                self.delete_forward();
                EditorAction::None
            }
            EditorInput::Left => {
                self.move_left();
                EditorAction::None
            }
            EditorInput::Right => {
                self.move_right();
                EditorAction::None
            }
            EditorInput::Home => {
                self.move_start();
                EditorAction::None
            }
            EditorInput::End => {
                self.move_end();
                EditorAction::None
            }
            EditorInput::Up | EditorInput::PageUp => {
                self.history_prev();
                EditorAction::None
            }
            EditorInput::Down | EditorInput::PageDown => {
                self.history_next();
                EditorAction::None
            }
            EditorInput::Tab | EditorInput::BackTab => EditorAction::None,
            EditorInput::Escape => {
                if self.buffer.is_empty() {
                    EditorAction::Interrupt
                } else {
                    self.clear();
                    EditorAction::Cleared
                }
            }
            EditorInput::CtrlC => {
                if self.ctrl_c_armed {
                    self.clear();
                    EditorAction::Submit("/exit".to_string())
                } else {
                    self.ctrl_c_armed = true;
                    if self.buffer.is_empty() {
                        EditorAction::Interrupt
                    } else {
                        self.clear();
                        self.ctrl_c_armed = true;
                        EditorAction::Cleared
                    }
                }
            }
            EditorInput::CtrlD => {
                if self.buffer.is_empty() {
                    EditorAction::Exit
                } else {
                    self.clear();
                    EditorAction::Cleared
                }
            }
        }
    }

    fn submit_as(&mut self, kind: EditorActionKind) -> EditorAction {
        let submitted = self.buffer.trim_end().to_string();
        self.clear();
        if submitted.trim().is_empty() {
            return EditorAction::None;
        }
        self.push_history(&submitted);
        match kind {
            EditorActionKind::Normal => EditorAction::Submit(submitted),
            EditorActionKind::FollowUp => EditorAction::SubmitFollowUp(submitted),
            EditorActionKind::Steer => EditorAction::Steer(submitted),
        }
    }

    fn insert_text(&mut self, text: &str) {
        self.cursor = self.cursor.min(self.buffer.len());
        self.buffer.insert_str(self.cursor, text);
        self.cursor += text.len();
        self.history_index = None;
    }

    fn delete_backward(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let previous = previous_char_boundary(&self.buffer, self.cursor);
        self.buffer.drain(previous..self.cursor);
        self.cursor = previous;
        self.history_index = None;
    }

    fn delete_forward(&mut self) {
        if self.cursor >= self.buffer.len() {
            return;
        }
        let next = next_char_boundary(&self.buffer, self.cursor);
        self.buffer.drain(self.cursor..next);
        self.history_index = None;
    }

    fn delete_word_backward(&mut self) {
        let cursor = self.cursor.min(self.buffer.len());
        if cursor == 0 {
            return;
        }
        let mut start = cursor;
        while start > 0 {
            let previous = previous_char_boundary(&self.buffer, start);
            let ch = self.buffer[previous..start].chars().next().unwrap_or(' ');
            if ch.is_whitespace() {
                start = previous;
            } else {
                break;
            }
        }
        while start > 0 {
            let previous = previous_char_boundary(&self.buffer, start);
            let ch = self.buffer[previous..start].chars().next().unwrap_or(' ');
            if ch.is_whitespace() {
                break;
            }
            start = previous;
        }
        self.buffer.drain(start..cursor);
        self.cursor = start;
        self.history_index = None;
    }

    fn delete_current_line(&mut self) {
        if self.buffer.is_empty() {
            return;
        }
        let cursor = self.cursor.min(self.buffer.len());
        let start = self.buffer[..cursor]
            .rfind('\n')
            .map(|index| index + 1)
            .unwrap_or(0);
        let end = self.buffer[cursor..]
            .find('\n')
            .map(|offset| cursor + offset)
            .unwrap_or(self.buffer.len());
        self.buffer.drain(start..end);
        self.cursor = start;
        self.history_index = None;
    }

    fn move_left(&mut self) {
        self.cursor = previous_char_boundary(&self.buffer, self.cursor);
    }

    fn move_right(&mut self) {
        self.cursor = next_char_boundary(&self.buffer, self.cursor);
    }

    fn move_start(&mut self) {
        self.cursor = self.buffer[..self.cursor.min(self.buffer.len())]
            .rfind('\n')
            .map(|index| index + 1)
            .unwrap_or(0);
    }

    fn move_end(&mut self) {
        let cursor = self.cursor.min(self.buffer.len());
        self.cursor = self.buffer[cursor..]
            .find('\n')
            .map(|offset| cursor + offset)
            .unwrap_or(self.buffer.len());
    }

    fn history_prev(&mut self) {
        if self.history.is_empty() {
            return;
        }
        let index = self
            .history_index
            .map(|index| index.saturating_sub(1))
            .unwrap_or_else(|| self.history.len().saturating_sub(1));
        self.history_index = Some(index);
        self.replace_buffer(self.history[index].clone());
    }

    fn history_next(&mut self) {
        let Some(index) = self.history_index else {
            return;
        };
        if index + 1 >= self.history.len() {
            self.history_index = None;
            self.replace_buffer(String::new());
        } else {
            let next = index + 1;
            self.history_index = Some(next);
            self.replace_buffer(self.history[next].clone());
        }
    }

    fn push_history(&mut self, submitted: &str) {
        if self.history.last().is_some_and(|last| last == submitted) {
            self.history_index = None;
            return;
        }
        self.history.push(submitted.to_string());
        if self.history.len() > 200 {
            self.history.remove(0);
        }
        self.history_index = None;
    }

    fn clear(&mut self) {
        self.buffer.clear();
        self.cursor = 0;
        self.history_index = None;
        self.ctrl_c_armed = false;
    }

    fn arm_ctrl_c_exit(&mut self) {
        self.ctrl_c_armed = true;
    }

    fn ctrl_c_exit_armed(&self) -> bool {
        self.ctrl_c_armed
    }

    fn replace_buffer(&mut self, text: String) {
        self.buffer = text;
        self.cursor = self.buffer.len();
        self.history_index = None;
    }

    fn buffer_preview(&self) -> &str {
        &self.buffer
    }

    fn cursor(&self) -> usize {
        self.cursor.min(self.buffer.len())
    }
}

fn previous_char_boundary(text: &str, cursor: usize) -> usize {
    let cursor = cursor.min(text.len());
    if cursor == 0 {
        return 0;
    }
    text[..cursor]
        .char_indices()
        .last()
        .map(|(index, _)| index)
        .unwrap_or(0)
}

fn next_char_boundary(text: &str, cursor: usize) -> usize {
    let cursor = cursor.min(text.len());
    if cursor >= text.len() {
        return text.len();
    }
    text[cursor..]
        .char_indices()
        .nth(1)
        .map(|(offset, _)| cursor + offset)
        .unwrap_or(text.len())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EventOutputMode {
    Text,
    Json,
    Silent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TurnOutcome {
    Completed,
    Paused,
    Aborted(String),
    Interrupted(String),
}

impl TurnOutcome {
    fn into_result(self) -> Result<(), String> {
        match self {
            Self::Completed | Self::Paused => Ok(()),
            Self::Aborted(reason) => Err(format!("turn aborted: {reason}")),
            Self::Interrupted(reason) => Err(format!("turn interrupted: {reason}")),
        }
    }
}

#[derive(Debug, Clone)]
struct PendingApproval {
    turn_id: String,
    request_id: String,
    tool_call_id: Option<String>,
    tool_call: Option<ToolCall>,
}

#[derive(Debug, Clone)]
struct PendingQuestion {
    turn_id: String,
    request: AskUserRequest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum UiComponent {
    Transcript,
    Todos,
    Approval,
    Question,
    Background,
    Suggestion,
    Footer,
}

impl UiComponent {
    fn label(self) -> &'static str {
        match self {
            Self::Transcript => "transcript",
            Self::Todos => "todos",
            Self::Approval => "approval",
            Self::Question => "question",
            Self::Background => "background",
            Self::Suggestion => "suggestion",
            Self::Footer => "footer",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct DockState {
    todos: Vec<String>,
    approval: Option<String>,
    question: Option<String>,
    background: Option<String>,
    suggestion: Option<String>,
    footer: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TranscriptEntryKind {
    Info,
    User,
    Assistant,
    ToolRead,
    ToolWrite,
    ToolRun,
    Diff,
    Artifact,
    Denied,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TranscriptEntry {
    kind: TranscriptEntryKind,
    label: String,
    body: String,
    event_id: u64,
    turn_id: Option<String>,
    item_id: Option<String>,
    tool_call_id: Option<String>,
    artifact_id: Option<String>,
}

#[derive(Debug, Clone)]
struct RetainedTui {
    scrollback: VecDeque<String>,
    typed_scrollback: VecDeque<TranscriptEntry>,
    max_scrollback: usize,
    docks: DockState,
    dirty: BTreeSet<UiComponent>,
}

impl RetainedTui {
    fn new(max_scrollback: usize) -> Self {
        let mut dirty = BTreeSet::new();
        dirty.insert(UiComponent::Transcript);
        dirty.insert(UiComponent::Footer);
        Self {
            scrollback: VecDeque::new(),
            typed_scrollback: VecDeque::new(),
            max_scrollback,
            docks: DockState::default(),
            dirty,
        }
    }

    fn clear(&mut self) {
        self.scrollback.clear();
        self.typed_scrollback.clear();
        self.docks = DockState::default();
        self.dirty = [
            UiComponent::Transcript,
            UiComponent::Todos,
            UiComponent::Approval,
            UiComponent::Question,
            UiComponent::Background,
            UiComponent::Suggestion,
            UiComponent::Footer,
        ]
        .into_iter()
        .collect();
    }

    fn push_transcript(&mut self, line: impl Into<String>) {
        let line = line.into();
        if line.trim().is_empty() {
            return;
        }
        self.push_transcript_entry(TranscriptEntry {
            kind: TranscriptEntryKind::Info,
            label: "log".to_string(),
            body: line,
            event_id: 0,
            turn_id: None,
            item_id: None,
            tool_call_id: None,
            artifact_id: None,
        });
    }

    fn push_event_transcript(
        &mut self,
        event: &Event,
        rendered: String,
        calls: &BTreeMap<String, ToolCall>,
    ) {
        if rendered.trim().is_empty() {
            return;
        }
        let entry = transcript_entry_from_event(event, redact_render_text(&rendered), calls);
        self.push_transcript_entry(entry);
    }

    fn push_transcript_entry(&mut self, mut entry: TranscriptEntry) {
        entry.body = redact_render_text(&entry.body);
        if entry.body.trim().is_empty() {
            return;
        }
        self.scrollback.push_back(entry.body.clone());
        self.typed_scrollback.push_back(entry);
        while self.scrollback.len() > self.max_scrollback {
            self.scrollback.pop_front();
        }
        while self.typed_scrollback.len() > self.max_scrollback {
            self.typed_scrollback.pop_front();
        }
        self.dirty.insert(UiComponent::Transcript);
    }

    fn update_docks(&mut self, next: DockState) {
        if self.docks.todos != next.todos {
            self.dirty.insert(UiComponent::Todos);
        }
        if self.docks.approval != next.approval {
            self.dirty.insert(UiComponent::Approval);
        }
        if self.docks.question != next.question {
            self.dirty.insert(UiComponent::Question);
        }
        if self.docks.background != next.background {
            self.dirty.insert(UiComponent::Background);
        }
        if self.docks.suggestion != next.suggestion {
            self.dirty.insert(UiComponent::Suggestion);
        }
        if self.docks.footer != next.footer {
            self.dirty.insert(UiComponent::Footer);
        }
        self.docks = next;
    }

    fn dirty_components(&self) -> Vec<&'static str> {
        self.dirty.iter().copied().map(UiComponent::label).collect()
    }

    fn clear_dirty(&mut self) {
        self.dirty.clear();
    }

    fn render_docks(&self, width: usize) -> String {
        let mut lines = self.dock_lines(width);
        if lines.is_empty() {
            return String::new();
        }
        lines.insert(
            0,
            width_safe_line("─ OPPi docked panels (line-mode fallback) ─", width),
        );
        lines.join("\n")
    }

    fn render_frame(&self, width: usize, height: usize) -> String {
        let width = width.max(24);
        let height = height.max(6);
        let dock_lines = self.dock_lines(width);
        let reserved = dock_lines.len().saturating_add(3);
        let transcript_height = height.saturating_sub(reserved).max(1);
        let mut lines = vec![width_safe_line(
            "╭─ OPPi retained scrollback (foundation) ─",
            width,
        )];
        let mut rendered_transcript = Vec::new();
        for entry in self.scrollback.iter().rev().take(transcript_height).rev() {
            for line in entry.lines() {
                rendered_transcript.push(width_safe_line(line, width));
            }
        }
        if rendered_transcript.is_empty() {
            rendered_transcript.push(width_safe_line("scrollback: empty", width));
        }
        lines.extend(
            rendered_transcript
                .into_iter()
                .rev()
                .take(transcript_height)
                .collect::<Vec<_>>()
                .into_iter()
                .rev(),
        );
        lines.push(width_safe_line("├─ docks ─", width));
        if dock_lines.is_empty() {
            lines.push(width_safe_line("docks: idle", width));
        } else {
            lines.extend(dock_lines);
        }
        lines.push(width_safe_line(
            "╰─ line-mode fallback remains available ─",
            width,
        ));
        lines.join("\n")
    }

    fn dock_lines(&self, width: usize) -> Vec<String> {
        let mut lines = Vec::new();
        if !self.docks.todos.is_empty() {
            lines.push(width_safe_line(
                &format!("todos: {}", self.docks.todos.join(" | ")),
                width,
            ));
        }
        if let Some(approval) = &self.docks.approval {
            lines.push(width_safe_line(&format!("approval: {approval}"), width));
        }
        if let Some(question) = &self.docks.question {
            lines.push(width_safe_line(&format!("question: {question}"), width));
        }
        if let Some(background) = &self.docks.background {
            lines.push(width_safe_line(&format!("background: {background}"), width));
        }
        if let Some(suggestion) = &self.docks.suggestion {
            lines.push(width_safe_line(&format!("suggestion: {suggestion}"), width));
        }
        if !self.docks.footer.trim().is_empty() {
            lines.push(width_safe_line(&self.docks.footer, width));
        }
        lines
    }
}

fn redact_render_text(text: &str) -> String {
    text.lines()
        .map(|line| {
            line.split_whitespace()
                .map(|token| {
                    let lower = token.to_ascii_lowercase();
                    if token.starts_with("sk-")
                        || lower == "bearer"
                        || lower.starts_with("bearer ")
                        || lower.starts_with("oppi_openai_api_key=")
                        || lower.starts_with("openai_api_key=")
                        || lower.contains("api_key=")
                        || lower.contains("apikey=")
                    {
                        "[REDACTED]".to_string()
                    } else {
                        token.to_string()
                    }
                })
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn transcript_entry_from_event(
    event: &Event,
    rendered: String,
    calls: &BTreeMap<String, ToolCall>,
) -> TranscriptEntry {
    let mut entry = TranscriptEntry {
        kind: TranscriptEntryKind::Info,
        label: "info".to_string(),
        body: rendered,
        event_id: event.id,
        turn_id: event.turn_id.clone(),
        item_id: None,
        tool_call_id: None,
        artifact_id: None,
    };
    match &event.kind {
        EventKind::ItemStarted { item } | EventKind::ItemCompleted { item } => {
            entry.item_id = Some(item.id.clone());
            entry.turn_id = Some(item.turn_id.clone());
            match &item.kind {
                ItemKind::UserMessage { text } => {
                    entry.kind = TranscriptEntryKind::User;
                    entry.label = "user".to_string();
                    entry.body = text.clone();
                }
                ItemKind::AssistantMessage { text } | ItemKind::Reasoning { text } => {
                    entry.kind = TranscriptEntryKind::Assistant;
                    entry.label = "oppi".to_string();
                    entry.body = text.clone();
                }
                ItemKind::ToolCall(call) => {
                    entry.kind = tool_transcript_kind(call);
                    entry.label = tool_display_kind_name_main(&call.name).to_string();
                    entry.tool_call_id = Some(call.id.clone());
                }
                ItemKind::ToolResult(result) => {
                    entry.kind = if matches!(result.status, ToolResultStatus::Error) {
                        TranscriptEntryKind::Error
                    } else {
                        calls
                            .get(&result.call_id)
                            .map(tool_transcript_kind)
                            .unwrap_or(TranscriptEntryKind::ToolRead)
                    };
                    entry.label = calls
                        .get(&result.call_id)
                        .map(|call| tool_display_kind_name_main(&call.name).to_string())
                        .unwrap_or_else(|| "tool".to_string());
                    entry.tool_call_id = Some(result.call_id.clone());
                }
                ItemKind::Diagnostic(_) => {
                    entry.kind = TranscriptEntryKind::Error;
                    entry.label = "diag".to_string();
                }
                ItemKind::HandoffSummary { summary } => {
                    entry.kind = TranscriptEntryKind::Info;
                    entry.label = "compact".to_string();
                    entry.body = summary.clone();
                }
                _ => {}
            }
        }
        EventKind::ItemDelta { item_id, delta } => {
            entry.item_id = Some(item_id.clone());
            entry.kind = if delta.trim_start().starts_with(['+', '-']) {
                TranscriptEntryKind::Diff
            } else {
                TranscriptEntryKind::Assistant
            };
            entry.label = "oppi".to_string();
            entry.body = delta.clone();
        }
        EventKind::ToolCallStarted { call } => {
            entry.kind = tool_transcript_kind(call);
            entry.label = tool_display_kind_name_main(&call.name).to_string();
            entry.tool_call_id = Some(call.id.clone());
        }
        EventKind::ToolCallCompleted { result } => {
            entry.kind = if matches!(result.status, ToolResultStatus::Error) {
                TranscriptEntryKind::Error
            } else {
                calls
                    .get(&result.call_id)
                    .map(tool_transcript_kind)
                    .unwrap_or(TranscriptEntryKind::ToolRead)
            };
            entry.label = calls
                .get(&result.call_id)
                .map(|call| tool_display_kind_name_main(&call.name).to_string())
                .unwrap_or_else(|| "tool".to_string());
            entry.tool_call_id = Some(result.call_id.clone());
        }
        EventKind::ArtifactCreated { artifact } => {
            entry.kind = TranscriptEntryKind::Artifact;
            entry.label = "artifact".to_string();
            entry.tool_call_id = Some(artifact.tool_call_id.clone());
            entry.artifact_id = Some(artifact.id.clone());
            entry.body = artifact_transcript_body(artifact);
        }
        EventKind::ApprovalRequested { request } => {
            entry.kind = TranscriptEntryKind::Denied;
            entry.label = "approval".to_string();
            entry.tool_call_id = request.tool_call.as_ref().map(tool_call_id);
        }
        EventKind::AskUserRequested { .. } => {
            entry.kind = TranscriptEntryKind::Info;
            entry.label = "question".to_string();
        }
        EventKind::Diagnostic { .. } | EventKind::TurnAborted { .. } => {
            entry.kind = TranscriptEntryKind::Error;
            entry.label = "error".to_string();
        }
        _ => {}
    }
    entry
}

fn tool_display_kind_name_main(name: &str) -> &'static str {
    if tool_name_suggests_write_main(name) {
        "write"
    } else if tool_name_suggests_run_main(name) {
        "run"
    } else {
        "read"
    }
}

fn tool_transcript_kind(call: &ToolCall) -> TranscriptEntryKind {
    if tool_name_suggests_write_main(&call.name) {
        TranscriptEntryKind::ToolWrite
    } else if tool_name_suggests_run_main(&call.name) {
        TranscriptEntryKind::ToolRun
    } else {
        TranscriptEntryKind::ToolRead
    }
}

fn tool_name_suggests_write_main(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.contains("write")
        || lower.contains("edit")
        || lower.contains("delete")
        || lower.contains("patch")
}

fn tool_name_suggests_run_main(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.contains("shell") || lower.contains("bash") || lower.contains("exec")
}

fn artifact_transcript_body(artifact: &ArtifactMetadata) -> String {
    let mut parts = vec![format!("artifact://{}", artifact.output_path)];
    if let Some(mime) = artifact.mime_type.as_deref() {
        parts.push(mime.to_string());
    }
    if let Some(bytes) = artifact.bytes {
        parts.push(format_bytes(bytes));
    }
    if let (Some(width), Some(height)) = (artifact.width, artifact.height) {
        parts.push(format!("{width}x{height}"));
    }
    parts.push(format!("id={}", artifact.id));
    parts.push(format!("tool={}", artifact.tool_call_id));
    parts.join(" · ")
}

struct ShellSession {
    rpc: RpcClient,
    thread_id: String,
    cwd: String,
    cursor: u64,
    pending_approval: Option<PendingApproval>,
    pending_question: Option<PendingQuestion>,
    active_turn_id: Option<String>,
    todo_state: TodoState,
    current_goal: Option<ThreadGoal>,
    suggestion: Option<SuggestedNextMessage>,
    permission_mode: PermissionMode,
    permission_mode_source: String,
    follow_up_queue: VecDeque<String>,
    last_prompt: Option<String>,
    prompt_variant: String,
    theme: String,
    selected_model: Option<String>,
    known_model_ids: BTreeSet<String>,
    scoped_model_ids: Vec<String>,
    role_models: BTreeMap<String, String>,
    role_profile_path: PathBuf,
    tool_calls: BTreeMap<String, ToolCall>,
    background_summary: Option<String>,
    folded_threads: BTreeSet<String>,
    pending_login_picker: Option<LoginPicker>,
    meridian_child: Option<Child>,
    ui: RetainedTui,
    terminal_ui_active: bool,
}

impl ShellSession {
    fn connect(server_path: PathBuf, resume_thread: Option<String>) -> Result<Self, String> {
        let mut rpc = RpcClient::spawn(server_path)?;
        initialize_rpc_client(&mut rpc)?;

        let cwd = env::current_dir().map_err(|error| format!("read cwd: {error}"))?;
        let cwd = cwd.display().to_string();
        let thread = if let Some(thread_id) = resume_thread {
            rpc.request("thread/resume", json!({ "threadId": thread_id }))?
        } else {
            rpc.request(
                "thread/start",
                json!({
                    "project": {
                        "id": "oppi-shell",
                        "cwd": cwd.clone()
                    },
                    "title": "OPPi shell"
                }),
            )?
        };
        let thread_id = thread["thread"]["id"]
            .as_str()
            .ok_or_else(|| "thread start/resume returned no thread id".to_string())?
            .to_string();

        let role_profile_path = role_profile_settings_path();
        let role_models = load_role_profiles(&role_profile_path);
        let scoped_model_ids = load_enabled_model_scope(&role_profile_path);
        let (permission_mode, permission_mode_source) =
            load_permission_mode_setting(&role_profile_path);
        let prompt_variant = load_prompt_variant_setting(&role_profile_path);
        let mut session = Self {
            rpc,
            thread_id,
            cwd,
            cursor: 0,
            pending_approval: None,
            pending_question: None,
            active_turn_id: None,
            todo_state: TodoState::default(),
            current_goal: None,
            suggestion: None,
            permission_mode,
            permission_mode_source,
            follow_up_queue: VecDeque::new(),
            last_prompt: None,
            prompt_variant,
            theme: "oppi".to_string(),
            selected_model: None,
            known_model_ids: BTreeSet::new(),
            scoped_model_ids,
            role_models,
            role_profile_path,
            tool_calls: BTreeMap::new(),
            background_summary: None,
            folded_threads: BTreeSet::new(),
            pending_login_picker: None,
            meridian_child: None,
            ui: RetainedTui::new(2_000),
            terminal_ui_active: false,
        };
        session.load_thread_state(false)?;
        Ok(session)
    }

    fn start_turn_for_role(
        &mut self,
        prompt: &str,
        provider: &ProviderConfig,
        _json: bool,
        role: Option<&str>,
    ) -> Result<String, String> {
        self.start_turn_for_role_with_system_append(prompt, provider, _json, role, None)
    }

    fn start_turn_for_role_with_system_append(
        &mut self,
        prompt: &str,
        provider: &ProviderConfig,
        _json: bool,
        role: Option<&str>,
        system_append: Option<&str>,
    ) -> Result<String, String> {
        self.last_prompt = Some(prompt.to_string());
        let effective_provider = self.provider_for_role(provider, role);
        if let Some(role_name) = role
            && let Some(model) = self.effective_model_for_role(role_name, provider)
        {
            let _ = self.register_model_ref(
                &model,
                provider_name(&effective_provider),
                Some(role_name),
            );
        }
        let mut params = json!({
            "threadId": self.thread_id,
            "input": prompt,
            "executionMode": "background",
            "sandboxPolicy": sandbox_policy_json(self.permission_mode, &self.cwd),
        });
        match &effective_provider {
            ProviderConfig::Mock => {
                params["modelSteps"] = mock_model_steps_for_prompt(prompt, &self.cwd);
                if let Some(tool_definitions) = mock_tool_definitions_for_prompt(prompt) {
                    params["toolDefinitions"] = tool_definitions;
                }
            }
            ProviderConfig::OpenAiCompatible(config) => {
                let mut provider_json = openai_provider_json(config);
                apply_feature_routing_to_provider(&mut provider_json, &self.prompt_variant);
                apply_prompt_variant_to_provider(&mut provider_json, &self.prompt_variant);
                if let Some(system_append) = system_append {
                    append_system_prompt_to_provider(
                        &mut provider_json,
                        GOAL_CONTINUATION_SYSTEM_HEADING,
                        system_append,
                    );
                }
                params["modelProvider"] = provider_json;
                params["maxContinuations"] = json!(8);
            }
        }

        let value = self.rpc.request("turn/run-agentic", params)?;
        let turn_id = value["turn"]["id"]
            .as_str()
            .ok_or_else(|| "turn/run-agentic returned no turn id".to_string())?
            .to_string();
        self.active_turn_id = Some(turn_id.clone());
        self.sync_ui_docks();
        Ok(turn_id)
    }

    fn run_turn_for_role(
        &mut self,
        prompt: &str,
        provider: &ProviderConfig,
        json: bool,
        role: Option<&str>,
    ) -> Result<TurnOutcome, String> {
        self.start_turn_for_role(prompt, provider, json, role)?;
        self.poll_until_turn_boundary(json)
    }

    fn run_turn_for_role_with_system_append(
        &mut self,
        prompt: &str,
        provider: &ProviderConfig,
        json: bool,
        role: Option<&str>,
        system_append: Option<&str>,
    ) -> Result<TurnOutcome, String> {
        self.start_turn_for_role_with_system_append(prompt, provider, json, role, system_append)?;
        self.poll_until_turn_boundary(json)
    }

    fn handle_command(
        &mut self,
        line: &str,
        provider: &mut ProviderConfig,
        json: bool,
    ) -> Result<bool, String> {
        let mut parts = line.split_whitespace();
        let command = parts.next().unwrap_or_default();
        match command {
            "/exit" | "/quit" => {
                self.print_exit_requested(json)?;
                Ok(false)
            }
            "/help" | "/commands" => {
                self.print_text(&slash_command_help_text(), json)?;
                Ok(true)
            }
            "/settings" | "/prefs" | "/preferences" | "/settings:oppi" | "/oppi-settings" => {
                self.print_text(
                    "settings: in the retained TUI, /settings opens an arrow-key settings panel with nested theme, permissions, provider, login, model, sessions, and memory panels. Ctrl+L opens the model selector, Ctrl+P/Ctrl+Shift+P cycles models, and Shift+Tab cycles effort. In line-mode use /theme, /permissions, /model, /provider, /login, /tree, and /memory directly. In native TUI, bare /theme, /permissions, /provider, /login, /memory, /tree, /sessions, /resume, /model, /models, and /roles open native pickers.",
                    json,
                )?;
                Ok(true)
            }
            "/login" | "/logout" => {
                self.handle_login_command(command, provider, parts.collect::<Vec<_>>(), json)?;
                Ok(true)
            }
            "/provider" => {
                self.handle_provider_command(provider, parts.collect::<Vec<_>>(), json)?;
                Ok(true)
            }
            "/usage" | "/stats" => {
                self.print_text(&format_usage_panel(self, provider), json)?;
                Ok(true)
            }
            "/suggest-next" => {
                self.handle_suggest_next_command(parts.collect::<Vec<_>>(), json)?;
                Ok(true)
            }
            "/todos" => {
                self.handle_todos_command(parts.collect::<Vec<_>>(), json)?;
                Ok(true)
            }
            "/goal" => {
                let args = parts.collect::<Vec<_>>().join(" ");
                self.handle_goal_command(args.trim(), json)?;
                Ok(true)
            }
            "/memory" | "/mem" => {
                self.handle_memory_command(parts.collect::<Vec<_>>(), json)?;
                Ok(true)
            }
            "/memory-maintenance" => {
                let args = parts.collect::<Vec<_>>();
                let mut mapped = vec!["maintenance"];
                match args.as_slice() {
                    [] | ["dry-run"] | ["status"] => mapped.push("dry-run"),
                    ["apply", ..] => mapped.push("apply"),
                    ["help"] => mapped.clear(),
                    _ => mapped.push("dry-run"),
                }
                self.handle_memory_command(mapped, json)?;
                Ok(true)
            }
            "/idle-compact" => {
                self.handle_memory_command(vec!["settings"], json)?;
                Ok(true)
            }
            "/sandbox" | "/permissions" => {
                let args = parts.collect::<Vec<_>>().join(" ");
                self.handle_permissions_command(args.trim(), json)?;
                Ok(true)
            }
            "/models" => {
                let args = parts.collect::<Vec<_>>().join(" ");
                self.handle_models_command(provider, args.trim(), json)?;
                Ok(true)
            }
            "/scoped-models" => {
                self.handle_scoped_models_command(provider, parts.collect::<Vec<_>>(), json)?;
                Ok(true)
            }
            "/agents" => {
                self.handle_agents_command(provider, parts.collect::<Vec<_>>(), json)?;
                Ok(true)
            }
            "/skills" => {
                self.handle_skills_command(json)?;
                Ok(true)
            }
            "/graphify" => {
                self.handle_graphify_command(parts.collect::<Vec<_>>(), json)?;
                Ok(true)
            }
            "/prompt-variant" | "/variant" => {
                let args = parts.collect::<Vec<_>>().join(" ");
                self.handle_prompt_variant_command(args.trim(), json)?;
                Ok(true)
            }
            "/theme" | "/themes" => {
                let args = parts.collect::<Vec<_>>().join(" ");
                self.handle_theme_command(args.trim(), json)?;
                Ok(true)
            }
            "/model" => {
                self.handle_model_command(provider, parts.collect::<Vec<_>>(), json)?;
                Ok(true)
            }
            "/resume" => {
                self.handle_resume_command(parts.collect::<Vec<_>>(), json)?;
                Ok(true)
            }
            "/new" | "/clear" | "/reset" => {
                self.handle_new_thread_command(parts.collect::<Vec<_>>(), json)?;
                Ok(true)
            }
            "/fork" => {
                self.handle_fork_command(parts.collect::<Vec<_>>(), json)?;
                Ok(true)
            }
            "/tree" | "/sessions" => {
                self.handle_tree_command(parts.collect::<Vec<_>>(), json)?;
                Ok(true)
            }
            "/debug" => {
                self.handle_debug_command(json)?;
                Ok(true)
            }
            "/ui" | "/frame" => {
                self.handle_ui_command(provider, parts.collect::<Vec<_>>(), json)?;
                Ok(true)
            }
            "/keys" | "/keybindings" | "/oppi-terminal-setup" => {
                self.print_text(terminal_capability_summary(false), json)?;
                Ok(true)
            }
            "/steer" => {
                let input = parts.collect::<Vec<_>>().join(" ");
                self.steer_active_turn(input.trim(), json)?;
                Ok(true)
            }
            "/interrupt" | "/cancel" => {
                self.interrupt_active_turn(json)?;
                Ok(true)
            }
            "/roles" | "/role-model" => {
                self.handle_role_model_command(provider, parts.collect::<Vec<_>>(), json)?;
                Ok(true)
            }
            "/effort" => {
                let args = parts.collect::<Vec<_>>().join(" ");
                self.handle_effort_command(provider, args.trim(), json)?;
                Ok(true)
            }
            "/background" => {
                self.handle_background_command(parts.collect::<Vec<_>>(), json)?;
                Ok(true)
            }
            "/meridian" => {
                let mut mapped = vec!["meridian"];
                let args = parts.collect::<Vec<_>>();
                if args.is_empty() {
                    mapped.push("status");
                } else {
                    mapped.extend(args);
                }
                self.handle_login_command("/login", provider, mapped, json)?;
                Ok(true)
            }
            "/runtime-loop" => {
                let debug = self.rpc.request("debug/bundle", json!({}))?;
                if json {
                    self.print_value(
                        "runtimeLoop",
                        json!({
                            "mode": "native-rust-owned",
                            "threadId": self.thread_id,
                            "server": "oppi-server",
                            "turnLoop": "turn/run-agentic",
                            "fallback": "stable Pi remains available through normal `oppi`",
                            "serverMetrics": debug.get("metrics").cloned().unwrap_or(Value::Null),
                        }),
                        json,
                    )?;
                } else {
                    self.print_text(
                        "runtime-loop: native shell already runs turns through Rust `turn/run-agentic`; stable Pi remains the fallback through normal `oppi`. Use `oppi runtime-loop smoke --json` from the CLI for the mirror smoke matrix.",
                        json,
                    )?;
                }
                Ok(true)
            }
            "/review" | "/audit" | "/init" | "/independent" | "/bug-report"
            | "/feature-request" => {
                let args = parts.collect::<Vec<_>>().join(" ");
                self.prepare_and_run_command(command, &args, provider, json)?;
                Ok(true)
            }
            "/approve" => {
                self.resume_pending_approval(provider, json)?;
                Ok(true)
            }
            "/deny" => {
                self.deny_pending_approval(json)?;
                Ok(true)
            }
            "/answer" => {
                let answer = parts.collect::<Vec<_>>().join(" ");
                self.answer_pending_question(answer.trim(), provider, json)?;
                Ok(true)
            }
            "/btw" => {
                let question = parts.collect::<Vec<_>>().join(" ");
                self.handle_btw_command(question.trim(), json)?;
                Ok(true)
            }
            "/again" => {
                let prompt = self
                    .last_prompt
                    .clone()
                    .ok_or_else(|| "no previous prompt".to_string())?;
                if self.is_turn_running() || self.has_pending_pause() {
                    self.queue_follow_up(&prompt, json)?;
                } else {
                    let _ = self.run_turn_for_role(&prompt, provider, json, Some("executor"))?;
                }
                Ok(true)
            }
            other => {
                self.print_text(&format!("unknown command: {other}. Try /help."), json)?;
                Ok(true)
            }
        }
    }

    fn handle_login_picker_input(
        &mut self,
        input: &str,
        provider: &mut ProviderConfig,
        json_output: bool,
    ) -> Result<bool, String> {
        let Some(picker) = self.pending_login_picker else {
            return Ok(false);
        };
        let choice = normalize_login_choice(input);
        if matches!(choice.as_str(), "q" | "quit" | "cancel" | "back") {
            self.pending_login_picker = None;
            self.print_text("login picker closed", json_output)?;
            return Ok(true);
        }
        match picker {
            LoginPicker::Root => match choice.as_str() {
                "1" | "subscription" | "subscriptions" => {
                    self.pending_login_picker = Some(LoginPicker::Subscription);
                    self.print_text(&login_subscription_picker_panel(), json_output)?;
                }
                "2" | "api" | "apikey" | "api-key" => {
                    self.pending_login_picker = Some(LoginPicker::Api);
                    self.print_text(&login_api_picker_panel(), json_output)?;
                }
                _ => self.print_text(&format!("unknown login choice `{input}`\n{}", login_root_picker_panel(provider)), json_output)?,
            },
            LoginPicker::Subscription => match choice.as_str() {
                "1" | "codex" | "chatgpt" | "chat-gpt" => {
                    self.pending_login_picker = None;
                    self.configure_codex_login(provider, &[], json_output)?;
                }
                "2" | "claude" | "anthropic" | "meridian" => {
                    self.pending_login_picker = Some(LoginPicker::Claude);
                    self.print_text(&login_claude_picker_panel(), json_output)?;
                }
                "3" | "copilot" | "microsoft" | "github" => {
                    self.pending_login_picker = None;
                    self.configure_github_copilot_login(provider, &[], json_output)?;
                }
                _ => self.print_text(&format!("unknown subscription choice `{input}`\n{}", login_subscription_picker_panel()), json_output)?,
            },
            LoginPicker::Api => match choice.as_str() {
                "1" | "openai" | "openai-compatible" | "env" => {
                    self.pending_login_picker = None;
                    self.print_text(login_openai_instructions(), json_output)?;
                }
                _ => self.print_text(&format!("unknown API choice `{input}`\n{}", login_api_picker_panel()), json_output)?,
            },
            LoginPicker::Claude => match choice.as_str() {
                "1" | "status" => self.print_text(&meridian_status_panel(), json_output)?,
                "2" | "login" | "auth" => {
                    self.pending_login_picker = None;
                    self.run_claude_code_login(json_output)?;
                }
                "3" | "install" => {
                    self.pending_login_picker = Some(LoginPicker::MeridianInstallApproval);
                    self.print_text(&login_meridian_install_approval_panel(), json_output)?;
                }
                "4" | "start" => {
                    let result = self.start_meridian_bridge()?;
                    self.print_text(&result, json_output)?;
                    self.configure_meridian_login(provider, json_output)?;
                    self.pending_login_picker = None;
                }
                "5" | "use" | "configure" => {
                    self.configure_meridian_login(provider, json_output)?;
                    self.pending_login_picker = None;
                }
                "6" | "stop" => {
                    self.stop_meridian_bridge(json_output)?;
                    self.pending_login_picker = None;
                }
                _ => self.print_text(&format!("unknown Claude login choice `{input}`\n{}", login_claude_picker_panel()), json_output)?,
            },
            LoginPicker::MeridianInstallApproval => match choice.as_str() {
                "yes" | "y" | "approve" | "install" => {
                    self.pending_login_picker = None;
                    self.print_text("Installing managed Meridian bridge after explicit approval.", json_output)?;
                    let result = install_meridian_package()?;
                    self.print_text(&result, json_output)?;
                }
                "no" | "n" | "deny" => {
                    self.pending_login_picker = Some(LoginPicker::Claude);
                    self.print_text("Meridian install cancelled. Returning to Claude login options.", json_output)?;
                    self.print_text(&login_claude_picker_panel(), json_output)?;
                }
                _ => self.print_text(&format!("Please type `yes` to approve installing {MERIDIAN_PACKAGE_NAME}, or `no` to cancel."), json_output)?,
            },
        }
        Ok(true)
    }

    fn handle_login_command(
        &mut self,
        command: &str,
        provider: &mut ProviderConfig,
        args: Vec<&str>,
        json_output: bool,
    ) -> Result<(), String> {
        if command == "/logout" {
            return self.handle_logout_command(provider, args, json_output);
        }
        match args.as_slice() {
            [] | ["status"] => {
                self.pending_login_picker = Some(LoginPicker::Root);
                self.print_text(&login_root_picker_panel(provider), json_output)
            }
            ["subscription"] | ["subscriptions"] => {
                self.pending_login_picker = Some(LoginPicker::Subscription);
                self.print_text(&login_subscription_picker_panel(), json_output)
            }
            ["subscription", "codex" | "chatgpt", flags @ ..]
            | ["chatgpt" | "codex", flags @ ..] => {
                self.pending_login_picker = None;
                self.configure_codex_login(provider, flags, json_output)
            }
            ["subscription", "copilot" | "microsoft" | "github", flags @ ..]
            | ["copilot" | "microsoft" | "github", flags @ ..] => {
                self.pending_login_picker = None;
                self.configure_github_copilot_login(provider, flags, json_output)
            }
            ["api"] => {
                self.pending_login_picker = Some(LoginPicker::Api);
                self.print_text(&login_api_picker_panel(), json_output)
            }
            ["api", "openai"] | ["api", "openai", "status"] | ["openai"]
            | ["openai", "status"] => self.print_text(login_openai_instructions(), json_output),
            ["api", "openai", "env", env_name]
            | ["api", "openai", "auth-env", env_name]
            | ["openai", "env", env_name]
            | ["openai", "auth-env", env_name] => {
                self.pending_login_picker = None;
                self.configure_openai_env_login(provider, env_name, json_output)
            }
            ["subscription", "claude" | "anthropic" | "meridian"]
            | ["anthropic"]
            | ["claude"]
            | ["meridian"] => {
                self.pending_login_picker = Some(LoginPicker::Claude);
                self.print_text(&login_claude_picker_panel(), json_output)
            }
            ["subscription", "claude" | "anthropic" | "meridian", "status"]
            | ["anthropic" | "claude" | "meridian", "status"] => {
                self.print_text(&meridian_status_panel(), json_output)
            }
            ["subscription", "claude" | "anthropic" | "meridian", "login"]
            | ["anthropic" | "claude" | "meridian", "login"]
            | ["subscription", "claude" | "anthropic" | "meridian", "auth"]
            | ["anthropic" | "claude" | "meridian", "auth"] => {
                self.pending_login_picker = None;
                self.run_claude_code_login(json_output)
            }
            ["subscription", "claude" | "anthropic" | "meridian", "install", flags @ ..]
            | ["anthropic" | "claude" | "meridian", "install", flags @ ..] => {
                if !login_action_approved(flags) {
                    self.pending_login_picker = Some(LoginPicker::MeridianInstallApproval);
                    return self.print_text(&login_meridian_install_approval_panel(), json_output);
                }
                self.pending_login_picker = None;
                self.print_text("Installing managed Meridian bridge after explicit approval.", json_output)?;
                let result = install_meridian_package()?;
                self.print_text(&result, json_output)
            }
            ["subscription", "claude" | "anthropic" | "meridian", "start"]
            | ["anthropic" | "claude" | "meridian", "start"] => {
                self.pending_login_picker = None;
                let result = self.start_meridian_bridge()?;
                self.print_text(&result, json_output)?;
                self.configure_meridian_login(provider, json_output)
            }
            ["subscription", "claude" | "anthropic" | "meridian", "use"]
            | ["anthropic" | "claude" | "meridian", "use"] => {
                self.pending_login_picker = None;
                self.configure_meridian_login(provider, json_output)
            }
            ["subscription", "claude" | "anthropic" | "meridian", "use", _model]
            | ["anthropic" | "claude" | "meridian", "use", _model] => self.print_text(
                "login Claude no longer accepts a model argument. Use `/model <model-id>` or `/model role <role> <model-id>` first, then `/login subscription claude use`.",
                json_output,
            ),
            ["subscription", "claude" | "anthropic" | "meridian", "stop"]
            | ["anthropic" | "claude" | "meridian", "stop"] => {
                self.pending_login_picker = None;
                self.stop_meridian_bridge(json_output)
            }
            ["gemini" | "antigravity"] => {
                self.pending_login_picker = None;
                self.print_text(login_delegated_provider_text(args[0]), json_output)
            }
            _ => self.print_text(login_usage(), json_output),
        }
    }

    fn handle_logout_command(
        &mut self,
        provider: &mut ProviderConfig,
        args: Vec<&str>,
        json_output: bool,
    ) -> Result<(), String> {
        match args.as_slice() {
            [] | ["provider"] => {
                *provider = ProviderConfig::Mock;
                self.register_provider_model(provider)?;
                self.print_text("logout: native direct-provider settings cleared for this shell session; persistent Pi OAuth/API credentials are not touched. Use stable `oppi` /logout for Pi-managed credentials.", json_output)
            }
            ["anthropic" | "claude" | "meridian"] => self.stop_meridian_bridge(json_output),
            _ => self.print_text("usage: /logout [provider|anthropic]", json_output),
        }
    }

    fn configure_openai_env_login(
        &mut self,
        provider: &mut ProviderConfig,
        env_name: &str,
        json_output: bool,
    ) -> Result<(), String> {
        let env_name = env_name.trim();
        if !is_safe_api_key_env_name(env_name) {
            return Err("/login openai env accepts only env-reference names such as OPPI_OPENAI_API_KEY or OPENAI_API_KEY; raw keys are rejected".to_string());
        }
        let model = self
            .session_model(provider)
            .map(str::to_string)
            .or_else(default_model_from_env)
            .unwrap_or_else(|| OPENAI_DIRECT_DEFAULT_MODEL.to_string());
        *provider = ProviderConfig::OpenAiCompatible(with_default_reasoning_effort(
            OpenAiCompatibleConfig {
                flavor: DirectProviderFlavor::OpenAiCompatible,
                model,
                base_url: None,
                api_key_env: Some(env_name.to_string()),
                system_prompt: None,
                temperature: None,
                reasoning_effort: None,
                max_output_tokens: None,
                stream: true,
            },
        ));
        self.register_provider_model(provider)?;
        self.print_text(
            &format!(
                "login openai: configured direct OpenAI-compatible provider with env reference {env_name} ({}). Raw secrets were not stored.",
                if env::var(env_name).ok().filter(|value| !value.trim().is_empty()).is_some() { "present" } else { "missing" }
            ),
            json_output,
        )
    }

    fn configure_codex_login(
        &mut self,
        provider: &mut ProviderConfig,
        flags: &[&str],
        json_output: bool,
    ) -> Result<(), String> {
        let force = login_action_approved(flags)
            || flags.iter().any(|flag| {
                matches!(
                    flag.trim().to_ascii_lowercase().as_str(),
                    "--force" | "force" | "relogin"
                )
            });
        let auth_path = auth_store_path();
        if force || !codex_auth_present_at(&auth_path) {
            let (verifier, state, url) = create_codex_oauth_flow()?;
            let listener = CodexOAuthListener::bind()?;
            self.print_text(
                &format!(
                    "login Codex: opening browser for ChatGPT/Codex OAuth. If it does not open, paste this URL into your browser:\n{url}\nWaiting for local callback at {OPENAI_CODEX_REDIRECT_URI}..."
                ),
                json_output,
            )?;
            if let Err(error) = open_browser(&url) {
                self.print_text(
                    &format!("login Codex: could not open browser automatically ({error}); open the URL above manually."),
                    json_output,
                )?;
            }
            let code = listener.wait_for_code(&state, Duration::from_secs(300))?;
            let token = exchange_codex_oauth_code(&code, &verifier)?;
            persist_codex_oauth(&auth_path, token)?;
            self.print_text(
                &format!(
                    "login Codex: OAuth credential saved to {} (tokens redacted).",
                    format_path_for_display(&auth_path)
                ),
                json_output,
            )?;
        }

        let model = self
            .session_model(provider)
            .filter(|model| *model != "mock-scripted")
            .map(str::to_string)
            .or_else(|| env::var("OPPI_OPENAI_CODEX_MODEL").ok())
            .or_else(default_model_from_env)
            .unwrap_or_else(|| OPENAI_CODEX_DEFAULT_MODEL.to_string());
        *provider = ProviderConfig::OpenAiCompatible(with_default_reasoning_effort(
            OpenAiCompatibleConfig {
                flavor: DirectProviderFlavor::OpenAiCodex,
                model,
                base_url: env::var("OPPI_OPENAI_CODEX_BASE_URL")
                    .ok()
                    .or_else(|| env::var("OPPI_CODEX_BASE_URL").ok()),
                api_key_env: None,
                system_prompt: None,
                temperature: None,
                reasoning_effort: None,
                max_output_tokens: None,
                stream: true,
            },
        ));
        self.register_provider_model(provider)?;
        self.print_text(
            "login Codex: configured ChatGPT/Codex subscription provider for this shell session. Run `/provider validate`, then `/provider smoke` for an explicit live call. Model choice stays in `/model`.",
            json_output,
        )
    }

    fn configure_github_copilot_login(
        &mut self,
        provider: &mut ProviderConfig,
        flags: &[&str],
        json_output: bool,
    ) -> Result<(), String> {
        let force = login_action_approved(flags)
            || flags.iter().any(|flag| {
                matches!(
                    flag.trim().to_ascii_lowercase().as_str(),
                    "--force" | "force" | "relogin"
                )
            });
        let enterprise_domain = github_copilot_enterprise_domain_from_flags(flags)?;
        let auth_path = github_copilot_auth_store_path();
        if force || !github_copilot_auth_present_at(&auth_path) {
            let domain = enterprise_domain
                .as_deref()
                .unwrap_or(GITHUB_COPILOT_DEFAULT_DOMAIN);
            let device = start_github_copilot_device_flow(domain)?;
            self.print_text(
                &format!(
                    "login Copilot: open {} and enter code {}. Waiting for GitHub device authorization...",
                    device.verification_uri, device.user_code
                ),
                json_output,
            )?;
            if let Err(error) = open_browser(&device.verification_uri) {
                self.print_text(
                    &format!("login Copilot: could not open browser automatically ({error}); open the URL above manually."),
                    json_output,
                )?;
            }
            let github_access = poll_github_copilot_device_flow(domain, &device)?;
            let token = refresh_github_copilot_token(&github_access, enterprise_domain.as_deref())?;
            persist_github_copilot_oauth(
                &auth_path,
                &github_access,
                enterprise_domain.as_deref(),
                token,
            )?;
            self.print_text(
                &format!(
                    "login Copilot: OAuth credential saved to {} (tokens redacted).",
                    format_path_for_display(&auth_path)
                ),
                json_output,
            )?;
        }

        let auth = read_github_copilot_auth_at(&auth_path)?;
        let base_url = github_copilot_base_url(&auth);
        let model = self
            .session_model(provider)
            .filter(|model| *model != "mock-scripted")
            .map(str::to_string)
            .or_else(|| env::var("OPPI_GITHUB_COPILOT_MODEL").ok())
            .or_else(default_model_from_env)
            .unwrap_or_else(|| GITHUB_COPILOT_DEFAULT_MODEL.to_string());
        *provider = ProviderConfig::OpenAiCompatible(with_default_reasoning_effort(
            OpenAiCompatibleConfig {
                flavor: DirectProviderFlavor::GitHubCopilot,
                model,
                base_url: Some(base_url),
                api_key_env: None,
                system_prompt: None,
                temperature: None,
                reasoning_effort: None,
                max_output_tokens: None,
                stream: true,
            },
        ));
        self.register_provider_model(provider)?;
        self.print_text(
            "login Copilot: configured GitHub Copilot subscription provider for this shell session. Use `/model <openai-compatible-copilot-model>` for model choice; `/provider smoke` is the explicit live call.",
            json_output,
        )
    }

    fn configure_meridian_login(
        &mut self,
        provider: &mut ProviderConfig,
        json_output: bool,
    ) -> Result<(), String> {
        let selected_model = self
            .session_model(provider)
            .filter(|model| *model != "mock-scripted")
            .unwrap_or(MERIDIAN_DEFAULT_MODEL)
            .to_string();
        let config = meridian_provider_config(Some(&selected_model));
        let model = config.model.clone();
        let base_url = config.base_url.clone().unwrap_or_else(meridian_base_url);
        *provider = ProviderConfig::OpenAiCompatible(config);
        self.register_provider_model(provider)?;
        self.print_text(
            &format!(
                "login Claude: configured explicit Meridian bridge at {}. Model is {model}; change it with `/model <model-id>` or `/model role <role> <model-id>`, not `/login`. Authentication stays in Claude Code/Meridian; OPPi sends only placeholder auth to the loopback bridge.",
                redacted_base_url_label(&base_url)
            ),
            json_output,
        )
    }

    fn start_meridian_bridge(&mut self) -> Result<String, String> {
        if meridian_reachable() {
            return Ok(format!(
                "Meridian already reachable at {}",
                meridian_base_url()
            ));
        }
        let mut failures = Vec::new();
        for command in meridian_start_candidates() {
            match spawn_meridian_command(&command) {
                Ok(mut child) => {
                    if wait_for_meridian(Duration::from_secs(12)) {
                        self.meridian_child = Some(child);
                        return Ok(format!(
                            "Started Meridian at {} via {}",
                            meridian_base_url(),
                            command.display()
                        ));
                    }
                    let _ = child.kill();
                    failures.push(format!("{}: did not become reachable", command.display()));
                }
                Err(error) => failures.push(format!("{}: {error}", command.display())),
            }
        }
        Err(format!(
            "Could not start Meridian without hidden fallback. Run `/login anthropic install`, run `claude login`, start `meridian` yourself, or set OPPI_MERIDIAN_COMMAND. Tried: {}",
            failures.join("; ")
        ))
    }

    fn stop_meridian_bridge(&mut self, json_output: bool) -> Result<(), String> {
        if let Some(mut child) = self.meridian_child.take() {
            let _ = child.kill();
            return self.print_text(
                "Stopped Meridian process started by this OPPi shell session.",
                json_output,
            );
        }
        self.print_text("Meridian was not started by this OPPi shell session; if you run it externally, stop that process directly.", json_output)
    }

    fn run_claude_code_login(&mut self, json_output: bool) -> Result<(), String> {
        if json_output {
            return self.print_text(
                "login Claude: run `claude login` in an interactive terminal, then `/login subscription claude start` or `/login subscription claude use`. Meridian owns token refresh through the Claude Code SDK; OPPi does not extract Claude tokens.",
                json_output,
            );
        }
        if self.terminal_ui_active {
            return self.print_text(
                "login Claude: leave the retained TUI or use --no-tui, run `claude login` in the terminal, then return to `/login subscription claude start`. OPPi will not run an interactive Claude Code login while raw TUI mode owns the terminal.",
                json_output,
            );
        }
        self.print_text(
            "login Claude: launching explicit `claude login` in this terminal. Tokens stay with Claude Code/Meridian; OPPi does not read or store them.",
            json_output,
        )?;
        let status = Command::new("claude")
            .arg("login")
            .status()
            .map_err(|error| {
                format!("spawn `claude login`: {error}. Install Claude Code first, then retry.")
            })?;
        if status.success() {
            self.print_text(
                "login Claude: Claude Code login finished. Next: `/login subscription claude start` or `/login subscription claude use`.",
                json_output,
            )
        } else {
            Err(format!("`claude login` exited with status {status}"))
        }
    }

    fn handle_provider_command(
        &mut self,
        provider: &mut ProviderConfig,
        args: Vec<&str>,
        json_output: bool,
    ) -> Result<(), String> {
        match args.as_slice() {
            [] | ["status"] => {
                let models = self.list_models().unwrap_or_default();
                self.print_text(&provider_status_panel(provider, &models, self), json_output)
            }
            ["auth-env" | "key-env", env_name] => match provider {
                ProviderConfig::Mock => self.print_text(
                    "provider auth-env: mock provider does not use credentials",
                    json_output,
                ),
                ProviderConfig::OpenAiCompatible(config) => {
                    let env_name = env_name.trim();
                    if !is_safe_api_key_env_name(env_name) {
                        return Err("credential setup accepts only env-reference names such as OPPI_OPENAI_API_KEY or OPENAI_API_KEY; raw keys are rejected".to_string());
                    }
                    config.api_key_env = Some(env_name.to_string());
                    self.print_text(
                        &format!(
                            "provider api key env set to {env_name} ({})",
                            if env::var(env_name).ok().filter(|value| !value.trim().is_empty()).is_some() { "present" } else { "missing" }
                        ),
                        json_output,
                    )
                }
            },
            ["base-url", url] => match provider {
                ProviderConfig::Mock => self.print_text(
                    "provider base-url: mock provider does not use a network endpoint",
                    json_output,
                ),
                ProviderConfig::OpenAiCompatible(config) => {
                    config.base_url = Some((*url).to_string());
                    self.print_text(
                        &format!("provider base URL set to {}", redacted_base_url_label(url)),
                        json_output,
                    )
                }
            },
            ["validate"] => self.print_text(&provider_validation_panel(provider), json_output),
            ["smoke", prompt @ ..] => {
                let prompt = if prompt.is_empty() {
                    "Reply with OPPi provider validation OK.".to_string()
                } else {
                    prompt.join(" ")
                };
                if !provider_local_validation_ready(provider) {
                    return Err("provider smoke requires a configured model and present API key env; run /provider validate for redacted diagnostics".to_string());
                }
                let _ = self.run_turn_for_role(&prompt, provider, json_output, Some("executor"))?;
                Ok(())
            }
            ["policy"] => self.print_text(provider_policy_text(), json_output),
            ["anthropic" | "anthropic-status" | "compatibility"] => {
                self.print_text(anthropic_provider_evaluation(), json_output)
            }
            _ => self.print_text(
                "usage: /provider [status|auth-env <ENV>|base-url <url>|validate|smoke [prompt]|policy|anthropic]",
                json_output,
            ),
        }
    }

    fn handle_todos_command(&mut self, args: Vec<&str>, json_output: bool) -> Result<(), String> {
        match parse_todos_command_args(&args)? {
            TodoCommand::List => {
                let value = self.rpc.request("todos/list", json!({}))?;
                let result: TodoListResult = serde_json::from_value(value.clone())
                    .map_err(|error| format!("decode todos/list: {error}"))?;
                self.todo_state = result.state;
                if json_output {
                    self.print_value("todos", value, json_output)
                } else {
                    self.print_text(&format_todos(&self.todo_state), json_output)
                }
            }
            TodoCommand::ClientAction { action, id } => {
                let mut params = json!({
                    "threadId": self.thread_id,
                    "action": todo_client_action_name(action),
                });
                if let Some(id) = id.as_deref() {
                    params["id"] = json!(id);
                }
                let value = self.rpc.request("todos/client-action", params)?;
                let result: TodoClientActionResult = serde_json::from_value(value.clone())
                    .map_err(|error| format!("decode todos/client-action: {error}"))?;
                self.todo_state = result.state;
                if json_output {
                    self.print_value("todos", value, json_output)
                } else {
                    self.print_text(&format_todos_with_summary(&self.todo_state), json_output)
                }
            }
        }
    }

    fn handle_models_command(
        &mut self,
        provider: &ProviderConfig,
        filter: &str,
        json_output: bool,
    ) -> Result<(), String> {
        let models = main_model_refs_for_provider(self, provider);
        if json_output {
            self.print_value(
                "models",
                json!({
                    "items": filter_models(&models, filter),
                    "selectedModel": self.selected_model,
                    "filter": if filter.is_empty() { Value::Null } else { json!(filter) },
                    "roleModels": self.role_models,
                    "roleProfilePath": self.role_profile_path.display().to_string(),
                    "source": "native provider catalog + current selection",
                }),
                json_output,
            )
        } else {
            let rendered = format_model_list(
                &models,
                self.selected_model.as_deref(),
                filter,
                &self.role_models,
            );
            self.print_text(&rendered, json_output)
        }
    }

    fn handle_suggest_next_command(
        &mut self,
        args: Vec<&str>,
        json_output: bool,
    ) -> Result<(), String> {
        match args.as_slice() {
            [] | ["show"] => {
                let message = self
                    .suggestion
                    .as_ref()
                    .map(format_suggestion_summary)
                    .unwrap_or_else(|| "suggest-next: no active ghost suggestion".to_string());
                self.print_text(&message, json_output)
            }
            ["debug"] => {
                let message = self
                    .suggestion
                    .as_ref()
                    .map(format_suggestion_debug)
                    .unwrap_or_else(|| "suggest-next: no active ghost suggestion".to_string());
                self.print_text(&message, json_output)
            }
            ["clear" | "ignore"] => {
                self.suggestion = None;
                self.print_text("suggest-next: cleared", json_output)
            }
            ["accept"] => self.print_text(
                "suggest-next: in native TUI, Tab accepts the ghost suggestion into the editor; /suggest-next accept is handled before command dispatch when typed in the editor",
                json_output,
            ),
            _ => self.print_text(
                "usage: /suggest-next [show|accept|clear|debug]",
                json_output,
            ),
        }
    }

    fn handle_goal_command(&mut self, args: &str, json_output: bool) -> Result<(), String> {
        let route = match goal_command_route(args) {
            Ok(route) => route,
            Err(message) if message.starts_with("usage:") => {
                return self.print_text(&message, json_output);
            }
            Err(message) => return Err(message),
        };

        let value = match route {
            GoalCommandRoute::Get => self
                .rpc
                .request("thread/goal/get", json!({ "threadId": self.thread_id }))?,
            GoalCommandRoute::Clear => self.rpc.request(
                "thread/goal/clear",
                json!({
                    "threadId": self.thread_id,
                }),
            )?,
            GoalCommandRoute::Set {
                objective,
                status,
                token_budget,
            } => self.rpc.request(
                "thread/goal/set",
                goal_set_params(&self.thread_id, objective, status, token_budget),
            )?,
            GoalCommandRoute::CreateObjective(objective) => {
                let current = self
                    .rpc
                    .request("thread/goal/get", json!({ "threadId": self.thread_id }))?;
                self.apply_goal_response(&current)?;
                if let Some(goal) = decode_goal_from_response(&current)?
                    && goal.status != ThreadGoalStatus::Complete
                {
                    return self.print_text(
                        "Goal already active. Use /goal replace <objective> to replace it.",
                        json_output,
                    );
                }
                self.rpc.request(
                    "thread/goal/set",
                    json!({
                        "threadId": self.thread_id,
                        "objective": objective,
                        "status": "active",
                    }),
                )?
            }
        };
        self.apply_goal_response(&value)?;

        if json_output {
            self.print_value("goal", value, json_output)
        } else if value.get("cleared").is_some() {
            let cleared = value
                .get("cleared")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            self.print_text(
                if cleared {
                    "Goal cleared"
                } else {
                    "Goal already clear"
                },
                json_output,
            )
        } else {
            self.print_text(&format_goal_response(&value), json_output)
        }
    }

    fn apply_goal_response(&mut self, value: &Value) -> Result<(), String> {
        if value.get("goal").is_some() {
            self.current_goal = decode_goal_from_response(value)?;
            self.sync_ui_docks();
        } else if value.get("cleared").is_some() {
            self.current_goal = None;
            self.sync_ui_docks();
        }
        Ok(())
    }

    fn handle_scoped_models_command(
        &mut self,
        provider: &ProviderConfig,
        args: Vec<&str>,
        json_output: bool,
    ) -> Result<(), String> {
        match args.as_slice() {
            [] | ["list"] => {
                let rendered = format_scoped_models(self, provider);
                self.print_text(&rendered, json_output)
            }
            ["enable", model] | ["add", model] => {
                let model = model.trim();
                if model.is_empty() {
                    return Err(
                        "usage: /scoped-models enable <model-id|provider/model-id>".to_string()
                    );
                }
                if !self.scoped_model_ids.iter().any(|item| item == model) {
                    self.scoped_model_ids.push(model.to_string());
                    save_enabled_model_scope(
                        &self.role_profile_path,
                        Some(self.scoped_model_ids.as_slice()),
                    )?;
                }
                self.print_text(&format_scoped_models(self, provider), json_output)
            }
            ["disable" | "remove", model] => {
                let model = model.trim();
                self.scoped_model_ids.retain(|item| item != model);
                save_enabled_model_scope(
                    &self.role_profile_path,
                    Some(self.scoped_model_ids.as_slice()),
                )?;
                self.print_text(&format_scoped_models(self, provider), json_output)
            }
            ["clear" | "all"] => {
                self.scoped_model_ids.clear();
                save_enabled_model_scope(&self.role_profile_path, None)?;
                self.print_text(
                    "scoped models cleared; /model and Ctrl+P use all current provider models",
                    json_output,
                )
            }
            _ => self.print_text(
                "usage: /scoped-models [list|enable <model>|disable <model>|clear]",
                json_output,
            ),
        }
    }

    fn handle_model_command(
        &mut self,
        provider: &mut ProviderConfig,
        args: Vec<&str>,
        json_output: bool,
    ) -> Result<(), String> {
        match args.as_slice() {
            [] => self.print_text(&self.model_status(provider), json_output),
            ["role", role_args @ ..] => {
                self.handle_role_model_command(provider, role_args.to_vec(), json_output)
            }
            [model] => self.select_session_model(provider, model, json_output),
            _ => self.print_text(
                "usage: /model [model-id|role <role> <model-id|inherit>]",
                json_output,
            ),
        }
    }

    fn handle_role_model_command(
        &mut self,
        provider: &mut ProviderConfig,
        args: Vec<&str>,
        json_output: bool,
    ) -> Result<(), String> {
        match args.as_slice() {
            [] => self.print_text(
                &format!(
                    "{}\nsource: {}",
                    format_role_profiles(&self.role_models, self.session_model(provider)),
                    self.role_profile_path.display()
                ),
                json_output,
            ),
            [role] => {
                let role = normalize_role(role).ok_or_else(|| role_usage().to_string())?;
                let value = self
                    .role_models
                    .get(role)
                    .map(String::as_str)
                    .unwrap_or("inherit");
                self.print_text(&format!("role {role}: {value}"), json_output)
            }
            [role, "inherit" | "off" | "unset"] => {
                let role = normalize_role(role).ok_or_else(|| role_usage().to_string())?;
                self.role_models.remove(role);
                save_role_profiles(&self.role_profile_path, &self.role_models)?;
                self.print_text(
                    &format!(
                        "role {role} now inherits {} (persisted)",
                        self.session_model(provider).unwrap_or("session model")
                    ),
                    json_output,
                )
            }
            [role, model] => {
                let role = normalize_role(role).ok_or_else(|| role_usage().to_string())?;
                self.role_models
                    .insert(role.to_string(), (*model).to_string());
                save_role_profiles(&self.role_profile_path, &self.role_models)?;
                self.register_model_ref(model, provider_name(provider), Some(role))?;
                self.print_text(
                    &format!("role {role} model set to {model} (persisted)"),
                    json_output,
                )
            }
            _ => self.print_text(role_usage(), json_output),
        }
    }

    fn select_session_model(
        &mut self,
        provider: &mut ProviderConfig,
        model: &str,
        json_output: bool,
    ) -> Result<(), String> {
        self.register_model_ref(model, provider_name(provider), None)?;
        let value = self.rpc.request(
            "model/select",
            json!({ "threadId": self.thread_id, "modelId": model }),
        )?;
        self.selected_model = Some(model.to_string());
        if let ProviderConfig::OpenAiCompatible(config) = provider {
            config.model = model.to_string();
        }
        if json_output {
            self.print_value("modelSelected", value, json_output)
        } else {
            self.print_text(&format!("model set to {model}"), json_output)
        }
    }

    fn session_model<'a>(&'a self, provider: &'a ProviderConfig) -> Option<&'a str> {
        self.selected_model
            .as_deref()
            .or_else(|| current_provider_model(provider))
    }

    fn model_status(&self, provider: &ProviderConfig) -> String {
        let session_model = self.session_model(provider).unwrap_or("none");
        format!(
            "model: {session_model}\nselected: {}\n{}\nroleProfileSource: {}",
            self.selected_model.as_deref().unwrap_or("none"),
            format_role_profiles(&self.role_models, self.session_model(provider)),
            self.role_profile_path.display()
        )
    }

    fn register_provider_model(&mut self, provider: &ProviderConfig) -> Result<(), String> {
        match provider {
            ProviderConfig::Mock => {
                self.register_model_ref("mock-scripted", "mock", None)?;
                self.selected_model = Some("mock-scripted".to_string());
                let _ = self.rpc.request(
                    "model/select",
                    json!({ "threadId": self.thread_id, "modelId": "mock-scripted" }),
                )?;
            }
            ProviderConfig::OpenAiCompatible(config) => {
                self.register_model_ref(&config.model, "openai-compatible", None)?;
                self.selected_model = Some(config.model.clone());
                let _ = self.rpc.request(
                    "model/select",
                    json!({ "threadId": self.thread_id, "modelId": config.model }),
                )?;
            }
        }
        self.register_role_model_refs(provider)
    }

    fn register_role_model_refs(&mut self, provider: &ProviderConfig) -> Result<(), String> {
        let provider = provider_name(provider);
        for (role, model) in self.role_models.clone() {
            self.register_model_ref(&model, provider, Some(&role))?;
        }
        Ok(())
    }

    fn register_model_ref(
        &mut self,
        model: &str,
        provider: &str,
        role: Option<&str>,
    ) -> Result<(), String> {
        let model = model.trim();
        if model.is_empty() {
            return Err("model id cannot be empty".to_string());
        }
        self.known_model_ids.insert(model.to_string());
        self.rpc.request(
            "model/register",
            json!({
                "threadId": self.thread_id,
                "model": {
                    "id": model,
                    "provider": provider,
                    "displayName": model,
                    "role": role,
                }
            }),
        )?;
        Ok(())
    }

    fn list_models(&mut self) -> Result<Vec<ModelRef>, String> {
        let value = self.rpc.request("model/list", json!({}))?;
        let result: RuntimeListResult<ModelRef> =
            serde_json::from_value(value).map_err(|error| format!("decode model/list: {error}"))?;
        for model in &result.items {
            self.known_model_ids.insert(model.id.clone());
        }
        Ok(result.items)
    }

    fn provider_for_role(&self, provider: &ProviderConfig, role: Option<&str>) -> ProviderConfig {
        provider_for_role_config(provider, &self.role_models, role)
    }

    fn provider_for_role_with_complexity(
        &self,
        provider: &ProviderConfig,
        role: Option<&str>,
        promote_complex_subagent: bool,
    ) -> ProviderConfig {
        provider_for_role_config_with_complexity(
            provider,
            &self.role_models,
            role,
            promote_complex_subagent,
        )
    }

    fn effective_model_for_role(&self, role: &str, provider: &ProviderConfig) -> Option<String> {
        let role = normalize_role(role)?;
        let effective = self.provider_for_role(provider, Some(role));
        current_provider_model(&effective).map(str::to_string)
    }

    fn effective_effort_for_role(&self, role: &str, provider: &ProviderConfig) -> Option<String> {
        let effective = self.provider_for_role(provider, Some(role));
        match effective {
            ProviderConfig::OpenAiCompatible(config) => config.reasoning_effort,
            ProviderConfig::Mock => None,
        }
    }

    fn role_execution_profile(&self, role: &str, provider: &ProviderConfig) -> String {
        let normalized = normalize_role(role).unwrap_or("executor");
        let model = self
            .effective_model_for_role(normalized, provider)
            .unwrap_or_else(|| "provider default".to_string());
        let model_source = if self.role_models.contains_key(normalized) {
            "profile"
        } else if default_model_for_role(provider, normalized, false).is_some() {
            "default"
        } else {
            "inherit"
        };
        let effort = self
            .effective_effort_for_role(normalized, provider)
            .unwrap_or_else(|| "off".to_string());
        format!("role={normalized} model={model} ({model_source}) effort={effort}")
    }

    fn handle_resume_command(&mut self, args: Vec<&str>, json_output: bool) -> Result<(), String> {
        let Some(thread_id) = args.first().copied() else {
            let value = self.rpc.request("thread/list", json!({}))?;
            let result: RuntimeListResult<Thread> = serde_json::from_value(value)
                .map_err(|error| format!("decode thread/list: {error}"))?;
            if json_output {
                let items: Vec<Thread> = result
                    .items
                    .into_iter()
                    .filter(|thread| same_project_cwd(&thread.project.cwd, &self.cwd))
                    .collect();
                return self.print_value(
                    "resume",
                    json!({
                        "currentThread": &self.thread_id,
                        "projectCwd": &self.cwd,
                        "items": items,
                    }),
                    json_output,
                );
            }
            let rendered = format_resume_session_list(
                &result.items,
                &self.thread_id,
                &self.cwd,
                &self.folded_threads,
            );
            return self.print_text(&rendered, json_output);
        };
        let value = self
            .rpc
            .request("thread/resume", json!({ "threadId": thread_id }))?;
        self.switch_to_thread(value, json_output, "resumed")
    }

    fn handle_new_thread_command(
        &mut self,
        args: Vec<&str>,
        json_output: bool,
    ) -> Result<(), String> {
        let title = if args.is_empty() {
            "OPPi shell".to_string()
        } else {
            args.join(" ")
        };
        let value = self.rpc.request(
            "thread/start",
            json!({
                "project": { "id": "oppi-shell", "cwd": self.cwd.clone() },
                "title": title,
            }),
        )?;
        self.switch_to_thread(value, json_output, "started")
    }

    fn handle_fork_command(&mut self, args: Vec<&str>, json_output: bool) -> Result<(), String> {
        let title = if args.is_empty() {
            format!("Fork of {}", self.thread_id)
        } else {
            args.join(" ")
        };
        let value = self.rpc.request(
            "thread/fork",
            json!({ "threadId": self.thread_id, "title": title }),
        )?;
        self.switch_to_thread(value, json_output, "forked")
    }

    fn handle_tree_command(&mut self, args: Vec<&str>, json_output: bool) -> Result<(), String> {
        if let Some(action) = args.first().copied() {
            match action {
                "fold" | "collapse" => {
                    let thread_id = args
                        .get(1)
                        .copied()
                        .ok_or_else(|| "usage: /tree fold <thread-id>".to_string())?;
                    self.folded_threads.insert(thread_id.to_string());
                    return self.print_text(&format!("session {thread_id} folded"), json_output);
                }
                "unfold" | "expand" => {
                    let thread_id = args
                        .get(1)
                        .copied()
                        .ok_or_else(|| "usage: /tree unfold <thread-id>".to_string())?;
                    self.folded_threads.remove(thread_id);
                    return self.print_text(&format!("session {thread_id} unfolded"), json_output);
                }
                "toggle" => {
                    let thread_id = args
                        .get(1)
                        .copied()
                        .ok_or_else(|| "usage: /tree toggle <thread-id>".to_string())?;
                    let state = if self.folded_threads.remove(thread_id) {
                        "unfolded"
                    } else {
                        self.folded_threads.insert(thread_id.to_string());
                        "folded"
                    };
                    return self.print_text(&format!("session {thread_id} {state}"), json_output);
                }
                "rename" => {
                    let thread_id = args
                        .get(1)
                        .copied()
                        .ok_or_else(|| "usage: /sessions rename <thread-id> <title>".to_string())?;
                    let title = args.get(2..).unwrap_or(&[]).join(" ");
                    if title.trim().is_empty() {
                        return Err("usage: /sessions rename <thread-id> <title>".to_string());
                    }
                    let value = self.rpc.request(
                        "thread/rename",
                        json!({ "threadId": thread_id, "title": title }),
                    )?;
                    return self.print_value("session", value, json_output);
                }
                "delete" | "archive" => {
                    let thread_id = args
                        .get(1)
                        .copied()
                        .ok_or_else(|| "usage: /sessions delete <thread-id>".to_string())?;
                    let value = self
                        .rpc
                        .request("thread/archive", json!({ "threadId": thread_id }))?;
                    return self.print_value("session", value, json_output);
                }
                _ => {}
            }
        }
        let value = self.rpc.request("thread/list", json!({}))?;
        if json_output {
            return self.print_value("tree", value, json_output);
        }
        let result: RuntimeListResult<Thread> = serde_json::from_value(value)
            .map_err(|error| format!("decode thread/list: {error}"))?;
        let mut rendered =
            format_thread_tree_with_folded(&result.items, &self.thread_id, &self.folded_threads);
        if let Some(summary) = self.latest_handoff_summary()? {
            rendered.push_str(&format!("\nsummary: {summary}"));
        }
        rendered.push_str(
            "\nnote: use /resume <thread-id>, /sessions rename <thread-id> <title>, /sessions delete <thread-id>, or /tree fold <thread-id>",
        );
        self.print_text(&rendered, json_output)
    }

    fn handle_debug_command(&mut self, json_output: bool) -> Result<(), String> {
        let mut value = self.rpc.request("debug/bundle", json!({}))?;
        value["client"] = json!({
            "name": "oppi-shell",
            "version": env!("CARGO_PKG_VERSION"),
            "threadId": self.thread_id,
            "lastPrompt": self.last_prompt.as_deref().map(redact_debug_text),
            "currentGoal": self.current_goal.as_ref(),
            "followUpQueue": self.follow_up_queue.len(),
            "cursor": self.cursor,
            "activeTurnId": self.active_turn_id,
            "roleModels": self.role_models,
            "roleProfilePath": self.role_profile_path.display().to_string(),
            "retainedUi": {
                "scrollbackLines": self.ui.scrollback.len(),
                "dirtyComponents": self.ui.dirty_components(),
            },
        });
        self.print_value("debug", value, json_output)
    }

    fn handle_ui_command(
        &mut self,
        provider: &ProviderConfig,
        args: Vec<&str>,
        json_output: bool,
    ) -> Result<(), String> {
        let ratatui = args
            .first()
            .is_some_and(|value| matches!(*value, "ratatui" | "rt"));
        let size_args = if ratatui { &args[1..] } else { args.as_slice() };
        let width = size_args
            .first()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or_else(terminal_width);
        let height = size_args
            .get(1)
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(24);
        self.sync_ui_docks();
        if ratatui {
            let mode = size_args
                .get(2)
                .map(|value| match *value {
                    "running" | "run" => ratatui_ui::RatatuiFrameMode::Running,
                    "tools" | "tool" | "artifact" | "denial" => ratatui_ui::RatatuiFrameMode::Tools,
                    "question" | "ask" | "ask_user" => ratatui_ui::RatatuiFrameMode::Question,
                    "background" | "bg" => ratatui_ui::RatatuiFrameMode::Background,
                    "todos" | "todo" => ratatui_ui::RatatuiFrameMode::Todos,
                    "slash" | "commands" => ratatui_ui::RatatuiFrameMode::Slash,
                    "settings" | "overlay" => ratatui_ui::RatatuiFrameMode::Settings,
                    _ => ratatui_ui::RatatuiFrameMode::Idle,
                })
                .unwrap_or(ratatui_ui::RatatuiFrameMode::Idle);
            let view = ratatui_ui::ratatui_view_model(self, provider);
            let frame =
                ratatui_ui::render_ratatui_preview(&view, width as u16, height as u16, mode)?;
            if json_output {
                self.print_value(
                    "ui",
                    json!({
                        "renderer": "ratatui",
                        "width": width,
                        "height": height,
                        "mode": format!("{mode:?}"),
                        "frame": frame,
                    }),
                    json_output,
                )?;
            } else {
                self.print_text(&frame, json_output)?;
            }
            return Ok(());
        }
        if json_output {
            self.print_value(
                "ui",
                json!({
                    "renderer": "retained",
                    "width": width,
                    "height": height,
                    "dirtyComponents": self.ui.dirty_components(),
                    "scrollbackLines": self.ui.scrollback.len(),
                    "frame": self.ui.render_frame(width, height),
                }),
                json_output,
            )?;
        } else {
            let mut rendered = self.ui.render_frame(width, height);
            let dirty = self.ui.dirty_components();
            if !dirty.is_empty() {
                rendered.push_str(&format!("\ninvalidated: {}", dirty.join(", ")));
            }
            self.print_text(&rendered, json_output)?;
        }
        self.ui.clear_dirty();
        Ok(())
    }

    fn switch_to_thread(
        &mut self,
        value: Value,
        json_output: bool,
        verb: &str,
    ) -> Result<(), String> {
        let thread_id = value["thread"]["id"]
            .as_str()
            .ok_or_else(|| "thread response returned no thread id".to_string())?
            .to_string();
        self.thread_id = thread_id.clone();
        self.cursor = 0;
        self.pending_approval = None;
        self.pending_question = None;
        self.active_turn_id = None;
        self.current_goal = None;
        self.tool_calls.clear();
        self.background_summary = None;
        self.follow_up_queue.clear();
        self.ui.clear();
        self.load_thread_state(json_output)?;
        if json_output {
            self.print_value("thread", value, json_output)
        } else {
            let mut rendered = format!("{verb} thread {thread_id}");
            if let Some(summary) = self.latest_handoff_summary()? {
                rendered.push_str(&format!("\nlatest handoff: {summary}"));
            }
            self.print_text(&rendered, json_output)
        }
    }

    fn load_thread_state(&mut self, render_pauses: bool) -> Result<(), String> {
        self.current_goal = None;
        let listed = self.rpc.request(
            "events/list",
            json!({ "threadId": self.thread_id, "after": 0, "limit": 5000 }),
        )?;
        let events = listed["events"]
            .as_array()
            .ok_or_else(|| "events/list returned invalid events".to_string())?;
        for raw in events {
            let event: Event = serde_json::from_value(raw.clone())
                .map_err(|error| format!("decode event: {error}"))?;
            self.cursor = self.cursor.max(event.id);
            match &event.kind {
                EventKind::ToolCallStarted { call } => {
                    self.tool_calls.insert(call.id.clone(), call.clone());
                }
                EventKind::ThreadGoalUpdated { goal } => {
                    self.current_goal = Some(goal.clone());
                }
                EventKind::ThreadGoalCleared { .. } => {
                    self.current_goal = None;
                }
                EventKind::TodosUpdated { state } => self.todo_state = state.clone(),
                EventKind::SuggestionOffered { suggestion } if suggestion.confidence >= 0.70 => {
                    self.suggestion = Some(suggestion.clone());
                }
                EventKind::ApprovalRequested { request } => {
                    self.pending_approval = Some(PendingApproval {
                        turn_id: event.turn_id.clone().unwrap_or_default(),
                        request_id: request.id.clone(),
                        tool_call_id: request.tool_call.as_ref().map(tool_call_id),
                        tool_call: request.tool_call.clone(),
                    });
                    if render_pauses {
                        println!("{}", format_approval_panel(request));
                    }
                }
                EventKind::ApprovalResolved { decision } => {
                    if self
                        .pending_approval
                        .as_ref()
                        .is_some_and(|pending| pending.request_id == decision.request_id)
                    {
                        self.pending_approval = None;
                    }
                }
                EventKind::AskUserRequested { request } => {
                    self.pending_question = Some(PendingQuestion {
                        turn_id: event.turn_id.clone().unwrap_or_default(),
                        request: request.clone(),
                    });
                    if render_pauses {
                        println!("{}", format_ask_user_panel(request));
                    }
                }
                EventKind::AskUserResolved { response } => {
                    if self
                        .pending_question
                        .as_ref()
                        .is_some_and(|pending| pending.request.id == response.request_id)
                    {
                        self.pending_question = None;
                    }
                }
                EventKind::TurnCompleted { .. }
                | EventKind::TurnAborted { .. }
                | EventKind::TurnInterrupted { .. } => {
                    if self.pending_approval.as_ref().is_some_and(|pending| {
                        Some(pending.turn_id.as_str()) == event.turn_id.as_deref()
                    }) {
                        self.pending_approval = None;
                    }
                    if self.pending_question.as_ref().is_some_and(|pending| {
                        Some(pending.turn_id.as_str()) == event.turn_id.as_deref()
                    }) {
                        self.pending_question = None;
                    }
                }
                _ => {}
            }
            if let Some(line) = render_event_line_themed(&event, &self.theme, &self.tool_calls) {
                self.ui
                    .push_event_transcript(&event, line, &self.tool_calls);
            }
            self.sync_ui_docks();
        }
        Ok(())
    }

    fn latest_handoff_summary(&mut self) -> Result<Option<String>, String> {
        let listed = self.rpc.request(
            "events/list",
            json!({ "threadId": self.thread_id, "after": 0, "limit": 5000 }),
        )?;
        let events = listed["events"]
            .as_array()
            .ok_or_else(|| "events/list returned invalid events".to_string())?;
        let mut latest = None;
        for raw in events {
            let event: Event = serde_json::from_value(raw.clone())
                .map_err(|error| format!("decode event: {error}"))?;
            if let EventKind::HandoffCompacted { summary, details } = event.kind {
                let mut line = summary;
                if let Some(details) = details {
                    line.push_str(&format!(
                        " ({} remaining, {} completed outcomes)",
                        details.remaining_todos.len(),
                        details.completed_outcomes.len()
                    ));
                }
                latest = Some(line);
            }
        }
        Ok(latest)
    }

    fn handle_effort_command(
        &mut self,
        provider: &mut ProviderConfig,
        effort: &str,
        json_output: bool,
    ) -> Result<(), String> {
        let requested = effort.trim();
        if requested.is_empty() {
            if json_output {
                return self.print_value("effort", effort_status_json(provider), json_output);
            }
            return self.print_text(&format_effort_status(provider), json_output);
        }

        let level = if requested.eq_ignore_ascii_case("auto") {
            recommended_effort_level_for_provider(provider)
        } else {
            normalize_thinking_level(requested).ok_or_else(|| {
                "Unknown effort. Use off, minimal, low, medium, high, xhigh, or auto.".to_string()
            })?
        };
        let allowed = allowed_effort_levels_for_provider(provider);
        if !allowed.contains(&level) {
            let allowed_text = allowed
                .iter()
                .map(|level| level.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            return self.print_text(
                &format!(
                    "{} does not expose '{}' effort in this slider. Allowed: {}.",
                    effort_model_name(provider),
                    level.as_str(),
                    allowed_text
                ),
                json_output,
            );
        }

        set_provider_effort_level(provider, level);
        save_reasoning_effort_setting(&self.role_profile_path, level)?;
        if json_output {
            return self.print_value("effort", effort_status_json(provider), json_output);
        }
        self.print_text(
            &format!(
                "Effort set to {} for {}.",
                effort_level_label_for_provider(provider, level),
                effort_model_name(provider)
            ),
            json_output,
        )
    }

    fn handle_permissions_command(&mut self, args: &str, json_output: bool) -> Result<(), String> {
        if !args.is_empty() {
            let mode = parse_permission_mode(args).ok_or_else(|| {
                "usage: /permissions [read-only|default|auto-review|full-access]".to_string()
            })?;
            self.permission_mode = mode;
            self.permission_mode_source = "session/settings".to_string();
            save_permission_mode_setting(&self.role_profile_path, mode)?;
        }
        let sandbox = self.rpc.request("sandbox/status", json!({}))?;
        if json_output {
            self.print_value(
                "permissions",
                json!({
                    "mode": self.permission_mode,
                    "source": self.permission_mode_source.as_str(),
                    "policy": sandbox_policy_json(self.permission_mode, &self.cwd),
                    "sandbox": sandbox,
                }),
                json_output,
            )?;
        } else {
            let rendered = format!(
                "permissions: {} (source: {})\npolicy: filesystem={}, network={}\nsandbox: {}",
                self.permission_mode.as_str(),
                self.permission_mode_source,
                filesystem_policy_for_mode(self.permission_mode),
                network_policy_for_mode(self.permission_mode),
                sandbox
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("status unavailable")
            );
            self.print_text(&rendered, json_output)?;
        }
        Ok(())
    }

    fn handle_memory_command(&mut self, args: Vec<&str>, json_output: bool) -> Result<(), String> {
        match args.as_slice() {
            [] | ["status"] => {
                let value = self.rpc.request("memory/status", json!({}))?;
                self.print_value("memory", value, json_output)
            }
            ["on"] | ["off"] => {
                let enabled = args[0] == "on";
                let value = self.rpc.request(
                    "memory/set",
                    json!({
                        "threadId": self.thread_id,
                        "status": { "enabled": enabled, "backend": if enabled { "client" } else { "none" }, "scope": "project", "memoryCount": 0 }
                    }),
                )?;
                self.print_value("memory", value, json_output)
            }
            ["dashboard"] => self.handle_memory_control("dashboard", false, json_output),
            ["settings"] => self.handle_memory_control("settings", false, json_output),
            ["maintain"] | ["maintenance"] | ["maintenance", "dry-run"] | ["maintain", "dry-run"] => {
                self.handle_memory_control("maintenance", false, json_output)
            }
            ["maintenance", "apply"] | ["maintain", "apply"] => {
                self.handle_memory_control("maintenance", true, json_output)
            }
            ["compact", rest @ ..] if !rest.is_empty() => {
                let summary = rest.join(" ");
                let value = self.rpc.request(
                    "memory/compact",
                    json!({ "threadId": self.thread_id, "summary": summary }),
                )?;
                self.print_value("memoryCompact", value, json_output)
            }
            _ => self.print_text("usage: /memory [status|on|off|dashboard|settings|maintenance [dry-run|apply]|compact <summary>]", json_output),
        }
    }

    fn handle_memory_control(
        &mut self,
        action: &str,
        apply: bool,
        json_output: bool,
    ) -> Result<(), String> {
        let value = self.rpc.request(
            "memory/control",
            json!({ "threadId": self.thread_id, "action": action, "apply": apply }),
        )?;
        if json_output {
            self.print_value("memoryControl", value, json_output)
        } else {
            let rendered = format_memory_control(&value);
            self.print_text(&rendered, json_output)
        }
    }

    fn handle_skills_command(&mut self, json_output: bool) -> Result<(), String> {
        let value = self
            .rpc
            .request("skills/list", json!({ "threadId": self.thread_id }))?;
        if json_output {
            return self.print_value("skills", value, json_output);
        }
        let items = value
            .get("items")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if items.is_empty() {
            return self.print_text("no skills discovered", json_output);
        }
        let mut rendered = "skills (active definitions; project > user > built-in):".to_string();
        for item in items {
            let Some(active) = item.get("active") else {
                continue;
            };
            let name = active
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("skill");
            let source = active
                .get("source")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let description = active
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let shadowed = item
                .get("shadowed")
                .and_then(Value::as_array)
                .map(Vec::len)
                .unwrap_or(0);
            rendered.push_str(&format!("\n- {name} [{source}]: {description}"));
            if shadowed > 0 {
                rendered.push_str(&format!(
                    "\n  shadows {shadowed} lower-priority definition(s)"
                ));
            }
        }
        self.print_text(&rendered, json_output)
    }

    fn handle_graphify_command(
        &mut self,
        args: Vec<&str>,
        json_output: bool,
    ) -> Result<(), String> {
        match args.as_slice() {
            [] | ["status"] => {
                let status = graphify_status(&self.cwd);
                if json_output {
                    self.print_value("graphify", graphify_status_json(&status), json_output)
                } else {
                    self.print_text(&format_graphify_status(&status), json_output)
                }
            }
            ["install" | "setup"] => self.print_text(graphify_install_guidance(), json_output),
            ["commands" | "help"] => self.print_text(graphify_command_guidance(), json_output),
            _ => self.print_text("usage: /graphify [status|install|commands]", json_output),
        }
    }

    fn handle_agents_command(
        &mut self,
        provider: &ProviderConfig,
        args: Vec<&str>,
        json_output: bool,
    ) -> Result<(), String> {
        match args.as_slice() {
            [] | ["list"] => {
                let value = self.rpc.request("agents/list", json!({}))?;
                if !json_output {
                    self.print_text(
                        &format!(
                            "agent dispatch profile: {}",
                            self.role_execution_profile("subagent", provider)
                        ),
                        json_output,
                    )?;
                }
                self.print_value("agents", value, json_output)
            }
            ["dispatch", name, task @ ..] if !task.is_empty() => {
                let role = "subagent";
                let task = task.join(" ");
                let promoted = complex_subagent_task(&task);
                let effective_provider =
                    self.provider_for_role_with_complexity(provider, Some(role), promoted);
                let model = current_provider_model(&effective_provider).map(str::to_string);
                let effort = match effective_provider {
                    ProviderConfig::OpenAiCompatible(config) => config.reasoning_effort,
                    ProviderConfig::Mock => None,
                };
                if !json_output {
                    let source = if self.role_models.contains_key(role) {
                        "profile"
                    } else if promoted {
                        "promoted"
                    } else if default_model_for_role(provider, role, false).is_some() {
                        "default"
                    } else {
                        "inherit"
                    };
                    self.print_text(
                        &format!(
                            "agent dispatch profile: role={role} model={} ({source}) effort={}",
                            model.as_deref().unwrap_or("provider default"),
                            effort.as_deref().unwrap_or("off")
                        ),
                        json_output,
                    )?;
                }
                let value = self.rpc.request(
                    "agents/dispatch",
                    json!({
                        "threadId": self.thread_id,
                        "agentName": name,
                        "task": task,
                        "background": false,
                        "role": role,
                        "model": model,
                        "effort": effort,
                    }),
                )?;
                self.print_value("agentDispatch", value, json_output)
            }
            ["block", run_id, reason @ ..] if !reason.is_empty() => {
                let value = self.rpc.request(
                    "agents/block",
                    json!({ "threadId": self.thread_id, "runId": run_id, "reason": reason.join(" ") }),
                )?;
                self.print_value("agentBlock", value, json_output)
            }
            ["complete", run_id, output @ ..] if !output.is_empty() => {
                let value = self.rpc.request(
                    "agents/complete",
                    json!({ "threadId": self.thread_id, "runId": run_id, "output": output.join(" ") }),
                )?;
                self.print_value("agentComplete", value, json_output)
            }
            ["import", rest @ ..] => {
                let cwd = Path::new(&self.cwd);
                let route = parse_agent_import_route(rest, cwd)?;
                let raw = fs::read_to_string(&route.path)
                    .map_err(|error| format!("read agent markdown {}: {error}", route.path.display()))?;
                let agent = parse_agent_markdown(&raw, route.source)?;
                let value = self.rpc.request(
                    "agents/register",
                    json!({ "threadId": self.thread_id, "agent": agent }),
                )?;
                if json_output {
                    self.print_value(
                        "agentImport",
                        json!({ "path": route.path.display().to_string(), "source": route.source, "result": value }),
                        json_output,
                    )
                } else {
                    self.print_text(
                        &format!("imported agent markdown from {}", route.path.display()),
                        json_output,
                    )
                }
            }
            ["export", rest @ ..] => {
                let cwd = Path::new(&self.cwd);
                let agent_dir = default_agent_dir();
                let route = parse_agent_export_route(rest, cwd, &agent_dir)?;
                let value = self.rpc.request("agents/list", json!({}))?;
                let agent = active_agent_from_list(value, &route.agent_name)?;
                let markdown = format_agent_markdown(&agent);
                if let Some(parent) = route.path.parent() {
                    fs::create_dir_all(parent).map_err(|error| {
                        format!("create agent markdown dir {}: {error}", parent.display())
                    })?;
                }
                fs::write(&route.path, markdown)
                    .map_err(|error| format!("write agent markdown {}: {error}", route.path.display()))?;
                if json_output {
                    self.print_value(
                        "agentExport",
                        json!({ "agent": route.agent_name, "path": route.path.display().to_string() }),
                        json_output,
                    )
                } else {
                    self.print_text(
                        &format!(
                            "exported agent {} to {}",
                            route.agent_name,
                            route.path.display()
                        ),
                        json_output,
                    )
                }
            }
            _ => self.print_text("usage: /agents [list|dispatch <name> <task>|block <run-id> <reason>|complete <run-id> <output>|import [project|user] <path>|export <name> [project|user|path]]", json_output),
        }
    }

    fn handle_prompt_variant_command(
        &mut self,
        variant: &str,
        json_output: bool,
    ) -> Result<(), String> {
        if !variant.is_empty() {
            let normalized = normalize_prompt_variant(variant)
                .ok_or_else(|| "usage: /prompt-variant [off|a|b|caveman]".to_string())?;
            save_prompt_variant_setting(&self.role_profile_path, &normalized)?;
            self.prompt_variant = normalized;
        }
        self.print_text(
            &format!("prompt variant: {}", self.prompt_variant),
            json_output,
        )
    }

    fn handle_theme_command(&mut self, theme: &str, json_output: bool) -> Result<(), String> {
        if !theme.is_empty() {
            if theme.trim().eq_ignore_ascii_case("reload") {
                if let Some(loaded) = load_theme_file()? {
                    self.theme = loaded;
                }
            } else {
                self.theme = normalize_theme_name(theme)
                    .ok_or_else(|| "usage: /theme [oppi|dark|light|plain|reload]".to_string())?;
            }
        }
        self.print_text(&format!("theme: {}", self.theme), json_output)
    }

    fn prepare_and_run_command(
        &mut self,
        command: &str,
        args: &str,
        provider: &ProviderConfig,
        json_output: bool,
    ) -> Result<(), String> {
        let role = role_for_command(command);
        let prepared = self.rpc.request(
            "command/prepare",
            json!({
                "command": command.trim_start_matches('/'),
                "args": args,
                "context": {
                    "role": role,
                    "roleModel": self.effective_model_for_role(role, provider),
                    "roleEffort": self.effective_effort_for_role(role, provider),
                },
                "promptVariantAppend": prompt_variant_append(&self.prompt_variant),
            }),
        )?;
        if !json_output {
            let mut rendered = format!(
                "command profile: {}",
                self.role_execution_profile(role, provider)
            );
            if let Some(notes) = prepared.get("notes").and_then(Value::as_array) {
                for note in notes.iter().filter_map(Value::as_str) {
                    rendered.push_str(&format!("\nnote: {note}"));
                }
            }
            self.print_text(&rendered, json_output)?;
        }
        let input = prepared["input"]
            .as_str()
            .ok_or_else(|| "command/prepare returned no input".to_string())?
            .to_string();
        let _ = self.run_turn_for_role(&input, provider, json_output, Some(role))?;
        Ok(())
    }

    fn resolve_background_task_id(
        &mut self,
        requested: Option<&str>,
        running_only: bool,
    ) -> Result<String, String> {
        if let Some(task_id) = requested
            && !is_background_latest_alias(task_id)
        {
            return Ok(task_id.to_string());
        }
        let value = self.rpc.request("background/list", json!({}))?;
        select_background_task_id(&value, running_only).ok_or_else(|| {
            if running_only {
                "no running background task; pass an explicit task id or run /background list".to_string()
            } else {
                "no background tasks; background shell tasks are process-local and reset when the server restarts".to_string()
            }
        })
    }

    fn print_background_read(
        &mut self,
        task_id: &str,
        max_bytes: usize,
        json: bool,
    ) -> Result<(), String> {
        let value = self.rpc.request(
            "background/read",
            json!({ "taskId": task_id, "maxBytes": max_bytes }),
        )?;
        self.background_summary = Some(background_summary_from_read(&value));
        self.sync_ui_docks();
        if json {
            self.print_value("backgroundRead", value, json)
        } else {
            let rendered = format_background_read(&value);
            self.print_text(&rendered, json)
        }
    }

    fn print_background_kill(&mut self, task_id: &str, json: bool) -> Result<(), String> {
        let value = self
            .rpc
            .request("background/kill", json!({ "taskId": task_id }))?;
        self.background_summary = Some(background_summary_from_kill(&value));
        self.sync_ui_docks();
        if json {
            self.print_value("backgroundKill", value, json)
        } else {
            let rendered = format_background_kill(&value);
            self.print_text(&rendered, json)
        }
    }

    fn handle_background_command(&mut self, args: Vec<&str>, json: bool) -> Result<(), String> {
        match args.as_slice() {
            [] | ["list"] => {
                let value = self.rpc.request("background/list", json!({}))?;
                self.background_summary = Some(background_summary_from_list(&value));
                self.sync_ui_docks();
                if json {
                    self.print_value("background", value, json)
                } else {
                    let rendered = format_background_list(&value);
                    self.print_text(&rendered, json)
                }
            }
            ["read"] => {
                let task_id = self.resolve_background_task_id(None, false)?;
                self.print_background_read(&task_id, 30_000, json)
            }
            ["read", task_id] => {
                let task_id = self.resolve_background_task_id(Some(task_id), false)?;
                self.print_background_read(&task_id, 30_000, json)
            }
            ["read", task_id, max_bytes] => {
                let max_bytes = max_bytes
                    .parse::<usize>()
                    .map_err(|error| format!("invalid max bytes: {error}"))?;
                let task_id = self.resolve_background_task_id(Some(task_id), false)?;
                self.print_background_read(&task_id, max_bytes, json)
            }
            ["kill"] => {
                let task_id = self.resolve_background_task_id(None, true)?;
                self.print_background_kill(&task_id, json)
            }
            ["kill", task_id] => {
                let task_id = self.resolve_background_task_id(Some(task_id), true)?;
                self.print_background_kill(&task_id, json)
            }
            _ => self.print_text(
                "usage: /background [list|read [task-id|latest] [max-bytes]|kill [task-id|latest]]",
                json,
            ),
        }
    }

    fn handle_btw_command(&mut self, question: &str, json_output: bool) -> Result<(), String> {
        if question.is_empty() {
            return self.print_text("usage: /btw <question>", json_output);
        }
        let value = self.rpc.request(
            "side-question/ask",
            json!({ "threadId": self.thread_id, "question": question }),
        )?;
        if json_output {
            self.print_value("sideQuestion", value, json_output)
        } else {
            let answer = value
                .get("answer")
                .and_then(Value::as_str)
                .unwrap_or("side question answered");
            self.print_text(answer, json_output)
        }
    }

    fn resume_pending_approval(
        &mut self,
        provider: &ProviderConfig,
        json_output: bool,
    ) -> Result<(), String> {
        let pending = self
            .pending_approval
            .take()
            .ok_or_else(|| "no pending approval".to_string())?;
        let approved_tool_call_ids = pending
            .tool_call_id
            .as_ref()
            .map(|id| vec![id.clone()])
            .unwrap_or_default();
        let mut params = json!({
            "threadId": self.thread_id,
            "turnId": pending.turn_id,
            "approvedToolCallIds": approved_tool_call_ids,
        });
        match provider {
            ProviderConfig::Mock => {
                params["modelSteps"] = mock_model_steps_after_approval(pending.tool_call.as_ref());
            }
            ProviderConfig::OpenAiCompatible(config) => {
                let mut provider_json = openai_provider_json(config);
                apply_feature_routing_to_provider(&mut provider_json, &self.prompt_variant);
                apply_prompt_variant_to_provider(&mut provider_json, &self.prompt_variant);
                params["modelProvider"] = provider_json;
                params["maxContinuations"] = json!(8);
            }
        }
        self.rpc.request("turn/resume-agentic", params)?;
        self.print_text(&format!("approved {}", pending.request_id), json_output)?;
        let _ = self.poll_until_turn_boundary(json_output)?;
        Ok(())
    }

    fn deny_pending_approval(&mut self, json_output: bool) -> Result<(), String> {
        let pending = self
            .pending_approval
            .take()
            .ok_or_else(|| "no pending approval".to_string())?;
        self.rpc.request(
            "turn/interrupt",
            json!({
                "threadId": self.thread_id,
                "turnId": pending.turn_id,
                "reason": format!("approval denied: {}", pending.request_id),
            }),
        )?;
        self.print_text(&format!("denied {}", pending.request_id), json_output)?;
        Ok(())
    }

    fn answer_pending_question(
        &mut self,
        answer: &str,
        provider: &ProviderConfig,
        json_output: bool,
    ) -> Result<(), String> {
        if answer.is_empty() {
            return Err("usage: /answer <text-or-option-id>".to_string());
        }
        let pending = self
            .pending_question
            .take()
            .ok_or_else(|| "no pending ask_user question".to_string())?;
        let answers = pending
            .request
            .questions
            .iter()
            .map(|question| {
                if let Some(option) = question.options.iter().find(|option| option.id == answer) {
                    json!({
                        "questionId": question.id,
                        "optionId": option.id,
                        "label": option.label,
                    })
                } else {
                    json!({
                        "questionId": question.id,
                        "text": answer,
                    })
                }
            })
            .collect::<Vec<_>>();
        let mut params = json!({
            "threadId": self.thread_id,
            "turnId": pending.turn_id,
            "askUserResponse": {
                "requestId": pending.request.id,
                "answers": answers,
            },
        });
        match provider {
            ProviderConfig::Mock => {
                params["modelSteps"] = mock_model_steps_after_answer();
            }
            ProviderConfig::OpenAiCompatible(config) => {
                let mut provider_json = openai_provider_json(config);
                apply_feature_routing_to_provider(&mut provider_json, &self.prompt_variant);
                apply_prompt_variant_to_provider(&mut provider_json, &self.prompt_variant);
                params["modelProvider"] = provider_json;
                params["maxContinuations"] = json!(8);
            }
        }
        self.rpc.request("turn/resume-agentic", params)?;
        self.print_text("answered pending question", json_output)?;
        let _ = self.poll_until_turn_boundary(json_output)?;
        Ok(())
    }

    fn poll_turn_events(&mut self, json_output: bool) -> Result<Option<TurnOutcome>, String> {
        self.poll_turn_events_with_output(if json_output {
            EventOutputMode::Json
        } else {
            EventOutputMode::Text
        })
    }

    fn poll_turn_events_silent(&mut self) -> Result<Option<TurnOutcome>, String> {
        self.poll_turn_events_with_output(EventOutputMode::Silent)
    }

    fn poll_turn_events_with_output(
        &mut self,
        output: EventOutputMode,
    ) -> Result<Option<TurnOutcome>, String> {
        let listed = self.rpc.request(
            "events/list",
            json!({
                "threadId": self.thread_id,
                "after": self.cursor,
                "limit": 1000
            }),
        )?;
        let events = listed["events"]
            .as_array()
            .ok_or_else(|| "events/list returned invalid events".to_string())?;
        let mut outcome = None;
        for raw in events {
            let event: Event = serde_json::from_value(raw.clone())
                .map_err(|error| format!("decode event: {error}"))?;
            self.cursor = self.cursor.max(event.id);
            let rendered = render_event_line_themed(&event, &self.theme, &self.tool_calls);
            match output {
                EventOutputMode::Json => println!(
                    "{}",
                    serde_json::to_string(&event).map_err(|error| error.to_string())?
                ),
                EventOutputMode::Text => {
                    if let Some(line) = &rendered {
                        println!("{line}");
                    }
                }
                EventOutputMode::Silent => {}
            }
            if let Some(line) = rendered {
                self.ui
                    .push_event_transcript(&event, line, &self.tool_calls);
            }
            match &event.kind {
                EventKind::TurnStarted { turn } => {
                    self.active_turn_id = Some(turn.id.clone());
                }
                EventKind::TurnCompleted { .. } => {
                    self.active_turn_id = None;
                    outcome = Some(TurnOutcome::Completed);
                }
                EventKind::TurnAborted { reason } => {
                    self.active_turn_id = None;
                    outcome = Some(TurnOutcome::Aborted(reason.clone()));
                }
                EventKind::TurnInterrupted { reason } => {
                    self.active_turn_id = None;
                    outcome = Some(TurnOutcome::Interrupted(reason.clone()));
                }
                EventKind::ApprovalRequested { request } => {
                    self.pending_approval = Some(PendingApproval {
                        turn_id: event.turn_id.clone().unwrap_or_default(),
                        request_id: request.id.clone(),
                        tool_call_id: request.tool_call.as_ref().map(tool_call_id),
                        tool_call: request.tool_call.clone(),
                    });
                    self.active_turn_id = None;
                    outcome = Some(TurnOutcome::Paused);
                }
                EventKind::AskUserRequested { request } => {
                    self.pending_question = Some(PendingQuestion {
                        turn_id: event.turn_id.clone().unwrap_or_default(),
                        request: request.clone(),
                    });
                    self.active_turn_id = None;
                    outcome = Some(TurnOutcome::Paused);
                }
                EventKind::ToolCallStarted { call } => {
                    self.tool_calls.insert(call.id.clone(), call.clone());
                }
                EventKind::ThreadGoalUpdated { goal } => {
                    self.current_goal = Some(goal.clone());
                }
                EventKind::ThreadGoalCleared { .. } => {
                    self.current_goal = None;
                }
                EventKind::TodosUpdated { state } => {
                    self.todo_state = state.clone();
                }
                EventKind::SuggestionOffered { suggestion } => {
                    if suggestion.confidence >= 0.70 {
                        self.suggestion = Some(suggestion.clone());
                    }
                }
                _ => {}
            }
            self.sync_ui_docks();
        }
        Ok(outcome)
    }

    fn poll_until_turn_boundary(&mut self, json_output: bool) -> Result<TurnOutcome, String> {
        let started = Instant::now();
        loop {
            if let Some(outcome) = self.poll_turn_events(json_output)? {
                return Ok(outcome);
            }
            if started.elapsed() > TURN_TIMEOUT {
                return Err("timed out waiting for turn event boundary".to_string());
            }
            std::thread::sleep(POLL_INTERVAL);
        }
    }

    fn has_pending_pause(&self) -> bool {
        self.pending_approval.is_some() || self.pending_question.is_some()
    }

    fn is_turn_running(&self) -> bool {
        self.active_turn_id.is_some()
    }

    fn mutable_turn_id(&self) -> Option<String> {
        self.active_turn_id
            .clone()
            .or_else(|| {
                self.pending_approval
                    .as_ref()
                    .map(|pending| pending.turn_id.clone())
            })
            .or_else(|| {
                self.pending_question
                    .as_ref()
                    .map(|pending| pending.turn_id.clone())
            })
    }

    fn steer_active_turn(&mut self, input: &str, json_output: bool) -> Result<(), String> {
        if input.trim().is_empty() {
            return Err("usage: /steer <text> or Ctrl+Enter with editor text".to_string());
        }
        let turn_id = self
            .mutable_turn_id()
            .ok_or_else(|| "no active or paused turn to steer".to_string())?;
        let value = self.rpc.request(
            "turn/steer",
            json!({ "threadId": self.thread_id, "turnId": turn_id, "input": input }),
        )?;
        if json_output {
            self.print_value("steer", value, json_output)
        } else {
            self.print_text("steering input sent to active turn", json_output)
        }
    }

    fn interrupt_active_turn(&mut self, json_output: bool) -> Result<(), String> {
        let Some(turn_id) = self.mutable_turn_id() else {
            return self.print_text("no active turn to interrupt", json_output);
        };
        let value = self.rpc.request(
            "turn/interrupt",
            json!({
                "threadId": self.thread_id,
                "turnId": turn_id,
                "reason": "user interrupt from native shell",
            }),
        )?;
        self.active_turn_id = None;
        self.pending_approval = None;
        self.pending_question = None;
        self.sync_ui_docks();
        if json_output {
            self.print_value("interrupt", value, json_output)
        } else {
            self.print_text("interrupt requested", json_output)
        }
    }

    fn restore_latest_follow_up(&mut self) -> Option<String> {
        let restored = self.follow_up_queue.pop_back();
        self.sync_ui_docks();
        restored
    }

    fn queue_follow_up(&mut self, prompt: &str, json_output: bool) -> Result<(), String> {
        let prompt = prompt.trim();
        if prompt.is_empty() {
            return Ok(());
        }
        self.follow_up_queue.push_back(prompt.to_string());
        self.sync_ui_docks();
        self.print_text(
            &format!("queued follow-up #{}", self.follow_up_queue.len()),
            json_output,
        )
    }

    fn start_next_queued_follow_up(
        &mut self,
        provider: &ProviderConfig,
        json_output: bool,
    ) -> Result<bool, String> {
        if self.is_turn_running() || self.has_pending_pause() {
            return Ok(false);
        }
        let Some(prompt) = self.follow_up_queue.pop_front() else {
            self.sync_ui_docks();
            return Ok(false);
        };
        self.sync_ui_docks();
        self.print_text(&format!("running queued follow-up: {prompt}"), json_output)?;
        self.start_turn_for_role(&prompt, provider, json_output, Some("executor"))?;
        Ok(true)
    }

    fn start_next_queued_or_goal_continuation(
        &mut self,
        provider: &ProviderConfig,
        json_output: bool,
    ) -> Result<(), String> {
        if self.start_next_queued_follow_up(provider, json_output)? {
            return Ok(());
        }
        self.start_goal_continuation_turn(provider, json_output)
            .map(|_| ())
    }

    fn drain_follow_ups(
        &mut self,
        provider: &ProviderConfig,
        json_output: bool,
    ) -> Result<(), String> {
        while !self.has_pending_pause() {
            let Some(prompt) = self.follow_up_queue.pop_front() else {
                break;
            };
            self.print_text(&format!("running queued follow-up: {prompt}"), json_output)?;
            let outcome =
                self.run_turn_for_role(&prompt, provider, json_output, Some("executor"))?;
            if !matches!(outcome, TurnOutcome::Completed) {
                break;
            }
        }
        while !self.has_pending_pause()
            && !self.is_turn_running()
            && self.follow_up_queue.is_empty()
        {
            let Some((prompt, continuation)) = self.claim_goal_continuation_prompt(json_output)?
            else {
                break;
            };
            self.print_text(&format!("continuing goal #{continuation}"), json_output)?;
            let outcome = self.run_turn_for_role_with_system_append(
                GOAL_CONTINUATION_INPUT,
                provider,
                json_output,
                Some("executor"),
                Some(&prompt),
            )?;
            if !matches!(outcome, TurnOutcome::Completed) {
                break;
            }
        }
        Ok(())
    }

    fn start_goal_continuation_turn(
        &mut self,
        provider: &ProviderConfig,
        json_output: bool,
    ) -> Result<bool, String> {
        let Some((prompt, continuation)) = self.claim_goal_continuation_prompt(json_output)? else {
            return Ok(false);
        };
        self.print_text(&format!("continuing goal #{continuation}"), json_output)?;
        self.start_turn_for_role_with_system_append(
            GOAL_CONTINUATION_INPUT,
            provider,
            json_output,
            Some("executor"),
            Some(&prompt),
        )?;
        Ok(true)
    }

    fn claim_goal_continuation_prompt(
        &mut self,
        json_output: bool,
    ) -> Result<Option<(String, u32)>, String> {
        if self.is_turn_running()
            || self.has_pending_pause()
            || !self.follow_up_queue.is_empty()
            || !self
                .current_goal
                .as_ref()
                .is_some_and(|goal| goal.status == ThreadGoalStatus::Active)
        {
            return Ok(None);
        }
        let value = self.rpc.request(
            "thread/goal/continuation",
            json!({
                "threadId": self.thread_id,
                "maxContinuations": GOAL_CONTINUATION_CAP,
            }),
        )?;
        self.apply_goal_response(&value)?;
        if let Some(reason) = value.get("blockedReason").and_then(Value::as_str) {
            self.print_text(&format!("goal continuation stopped: {reason}"), json_output)?;
            return Ok(None);
        }
        let Some(prompt) = value.get("prompt").and_then(Value::as_str) else {
            return Ok(None);
        };
        let continuation = value
            .get("continuation")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            .min(u32::MAX as u64) as u32;
        Ok(Some((prompt.to_string(), continuation)))
    }

    fn sync_ui_docks(&mut self) {
        let active = active_todos(&self.todo_state);
        let approval = self.pending_approval.as_ref().map(|pending| {
            format!(
                "{} pending{} — /approve or /deny",
                pending.request_id,
                pending
                    .tool_call_id
                    .as_ref()
                    .map(|id| format!(" for {id}"))
                    .unwrap_or_default()
            )
        });
        let question = self.pending_question.as_ref().map(|pending| {
            format!(
                "{} — /answer <text-or-option-id>",
                pending
                    .request
                    .title
                    .as_deref()
                    .unwrap_or("user input requested")
            )
        });
        let suggestion = self
            .suggestion
            .as_ref()
            .map(|suggestion| suggestion.message.clone());
        let active_todo_count = active.len();
        let goal = self.goal_status_label();
        let footer = format!(
            "status: role=executor model={} permissions={} memory=client-hosted todos={} queued={} goal={} diagnostics=line-mode/raw-deferred variant={} theme={} thread={}",
            self.selected_model.as_deref().unwrap_or("none"),
            self.permission_mode.as_str(),
            active_todo_count,
            self.follow_up_queue.len(),
            goal,
            self.prompt_variant,
            self.theme,
            self.thread_id
        );
        self.ui.update_docks(DockState {
            todos: active,
            approval,
            question,
            background: self.background_summary.clone(),
            suggestion,
            footer,
        });
    }

    fn goal_status_label(&self) -> String {
        format_goal_status_label(self.current_goal.as_ref())
    }

    fn goal_header_label(&self) -> Option<String> {
        if let Some(follow_up) = self.follow_up_queue.front() {
            return Some(follow_up.clone());
        }
        self.current_goal.as_ref().and_then(|goal| {
            (!matches!(goal.status, ThreadGoalStatus::Complete)).then(|| goal.objective.clone())
        })
    }

    fn render_docks(&mut self) {
        self.sync_ui_docks();
        let rendered = self.ui.render_docks(terminal_width());
        if !rendered.trim().is_empty() {
            println!("{rendered}");
        }
        self.ui.clear_dirty();
    }

    fn print_exit_requested(&mut self, json_output: bool) -> Result<(), String> {
        let resume_command = exit_resume_command(&self.thread_id);
        if self.terminal_ui_active && !json_output {
            self.ui.push_transcript(EXIT_COMMAND_ECHO_TEXT.to_string());
            self.ui.push_transcript(EXIT_REQUESTED_TEXT.to_string());
            self.ui.push_transcript(resume_command);
            self.sync_ui_docks();
            return Ok(());
        }
        self.print_text(
            &format!("{EXIT_REQUESTED_TEXT}\n{resume_command}"),
            json_output,
        )
    }

    fn print_text(&mut self, text: &str, json_output: bool) -> Result<(), String> {
        if self.terminal_ui_active && !json_output {
            self.ui.push_transcript(text.to_string());
            self.sync_ui_docks();
            return Ok(());
        }
        if json_output {
            println!("{}", json!({ "shell": text }));
        } else {
            println!("{text}");
        }
        Ok(())
    }

    fn print_value(&mut self, label: &str, value: Value, json_output: bool) -> Result<(), String> {
        if json_output {
            println!("{}", json!({ label: value }));
        } else {
            let rendered = format!(
                "{label}: {}",
                serde_json::to_string_pretty(&value).map_err(|error| error.to_string())?
            );
            if self.terminal_ui_active {
                self.ui.push_transcript(rendered);
                self.sync_ui_docks();
            } else {
                println!("{rendered}");
            }
        }
        Ok(())
    }

    fn shutdown(&mut self) {
        if let Some(mut child) = self.meridian_child.take() {
            let _ = child.kill();
        }
        let _ = self.rpc.request("server/shutdown", json!({}));
    }
}

fn initialize_rpc_client(rpc: &mut RpcClient) -> Result<(), String> {
    let init = rpc.request(
        "initialize",
        json!({
            "clientName": "oppi-shell",
            "clientVersion": env!("CARGO_PKG_VERSION"),
            "protocolVersion": oppi_protocol::OPPI_PROTOCOL_VERSION,
            "clientCapabilities": ["threads", "turns", "events", "background-turns"]
        }),
    )?;
    validate_initialize_response(&init)
}

fn list_sessions_command(server_path: PathBuf, json_output: bool) -> Result<(), String> {
    let mut rpc = RpcClient::spawn(server_path)?;
    initialize_rpc_client(&mut rpc)?;
    let cwd = env::current_dir().map_err(|error| format!("read cwd: {error}"))?;
    let cwd = cwd.display().to_string();
    let value = rpc.request("thread/list", json!({}))?;
    let result: RuntimeListResult<Thread> =
        serde_json::from_value(value).map_err(|error| format!("decode thread/list: {error}"))?;
    if json_output {
        let items: Vec<Thread> = result
            .items
            .into_iter()
            .filter(|thread| same_project_cwd(&thread.project.cwd, &cwd))
            .collect();
        println!(
            "{}",
            json!({
                "resume": {
                    "projectCwd": cwd,
                    "items": items,
                }
            })
        );
    } else {
        println!(
            "{}",
            format_resume_session_list(&result.items, "", &cwd, &BTreeSet::new())
        );
    }
    let _ = rpc.request("server/shutdown", json!({}));
    Ok(())
}

fn tool_call_id(call: &ToolCall) -> String {
    call.id.clone()
}

fn validate_initialize_response(init: &Value) -> Result<(), String> {
    if init["protocolCompatible"] == json!(false) {
        return Err(format!(
            "oppi-server protocol is incompatible: server={} min={}",
            init["protocolVersion"], init["minProtocolVersion"]
        ));
    }
    if init
        .get("protocolVersion")
        .and_then(Value::as_str)
        .is_none()
    {
        return Err("initialize response missing protocolVersion".to_string());
    }
    Ok(())
}

fn permission_mode_from_env() -> Option<PermissionMode> {
    env::var("OPPI_RUNTIME_WORKER_PERMISSION_MODE")
        .ok()
        .as_deref()
        .and_then(parse_permission_mode)
}

fn parse_permission_mode(raw: &str) -> Option<PermissionMode> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "read-only" | "readonly" | "ro" => Some(PermissionMode::ReadOnly),
        "default" => Some(PermissionMode::Default),
        "auto-review" | "autoreview" | "auto" => Some(PermissionMode::AutoReview),
        "full-access" | "full" | "danger" => Some(PermissionMode::FullAccess),
        _ => None,
    }
}

fn network_policy_for_mode(mode: PermissionMode) -> &'static str {
    match mode {
        PermissionMode::FullAccess => "enabled",
        _ => "disabled",
    }
}

fn filesystem_policy_for_mode(mode: PermissionMode) -> &'static str {
    match mode {
        PermissionMode::ReadOnly => "readOnly",
        PermissionMode::FullAccess => "unrestricted",
        PermissionMode::Default | PermissionMode::AutoReview => "workspaceWrite",
    }
}

fn sandbox_policy_json(mode: PermissionMode, cwd: &str) -> Value {
    let writable_roots = if mode == PermissionMode::ReadOnly {
        Vec::<String>::new()
    } else {
        vec![cwd.to_string()]
    };
    json!({
        "permissionProfile": {
            "mode": mode,
            "readableRoots": [cwd],
            "writableRoots": writable_roots,
            "filesystemRules": [],
            "protectedPatterns": [".env*", ".ssh/", "*.pem", "*.key", ".git/config", ".git/hooks/", ".npmrc", ".pypirc", ".mcp.json", ".claude.json"],
        },
        "network": if mode == PermissionMode::FullAccess { NetworkPolicy::Enabled } else { NetworkPolicy::Disabled },
        "filesystem": match mode {
            PermissionMode::ReadOnly => FilesystemPolicy::ReadOnly,
            PermissionMode::FullAccess => FilesystemPolicy::Unrestricted,
            PermissionMode::Default | PermissionMode::AutoReview => FilesystemPolicy::WorkspaceWrite,
        },
    })
}

fn mock_model_steps_for_prompt(prompt: &str, cwd: &str) -> Value {
    let lower = prompt.to_ascii_lowercase();
    if lower.contains("oppi-dogfood-repo-edit") {
        return json!([{
            "assistantDeltas": ["Dogfood repo edit approval requested."],
            "toolCalls": [{
                "id": "oppi-dogfood-repo-write",
                "name": "write_file",
                "namespace": "oppi",
                "arguments": {
                    "path": "docs/native-shell-dogfood.md",
                    "content": "# Native Shell Dogfood\n\nThis file is intentionally maintained by the Plan 50 native shell dogfood flow.\n\nThe flow proves that `oppi-shell` can request approval, resume the same turn, and write a non-trivial repository file through Rust-owned file tools.\n"
                }
            }],
            "finalResponse": false
        }]);
    }
    if lower.contains("oppi-dogfood-approval") {
        return json!([{
            "assistantDeltas": ["Dogfood approval pause requested."],
            "toolCalls": [{
                "id": "oppi-dogfood-approval-echo",
                "name": "echo",
                "namespace": "oppi",
                "arguments": { "output": "approved dogfood output", "requireApproval": true }
            }],
            "finalResponse": false
        }]);
    }
    if lower.contains("oppi-dogfood-ask-user") {
        return json!([{
            "assistantDeltas": ["Dogfood ask_user pause requested."],
            "toolCalls": [{
                "id": "oppi-dogfood-ask-user",
                "name": "ask_user",
                "namespace": "oppi",
                "arguments": {
                    "title": "Dogfood question",
                    "questions": [{
                        "id": "path",
                        "question": "Which dogfood path should OPPi take?",
                        "options": [{ "id": "safe", "label": "Safe path" }],
                        "defaultOptionId": "safe"
                    }]
                }
            }],
            "finalResponse": false
        }]);
    }
    if lower.contains("oppi-dogfood-background-start") {
        return json!([{
            "assistantDeltas": ["Dogfood background task approval requested."],
            "toolCalls": [{
                "id": "oppi-dogfood-background-shell",
                "name": "shell_exec",
                "namespace": "oppi",
                "arguments": {
                    "command": dogfood_background_command(),
                    "cwd": cwd,
                    "runInBackground": true
                }
            }],
            "finalResponse": false
        }]);
    }
    if lower.contains("oppi-dogfood-readonly-write") {
        return json!([{
            "assistantDeltas": ["Dogfood read-only write denial requested."],
            "toolCalls": [{
                "id": "oppi-dogfood-readonly-write",
                "name": "write_file",
                "namespace": "oppi",
                "arguments": {
                    "path": "docs/native-shell-readonly-denied.md",
                    "content": "This write should be denied in read-only mode.\n"
                }
            }],
            "finalResponse": false
        }]);
    }
    if lower.contains("oppi-dogfood-protected-path") {
        return json!([{
            "assistantDeltas": ["Dogfood protected-path denial requested."],
            "toolCalls": [{
                "id": "oppi-dogfood-protected-path",
                "name": "write_file",
                "namespace": "oppi",
                "arguments": {
                    "path": ".env.oppi-dogfood",
                    "content": "OPPI_DOGFOOD_SHOULD_NOT_WRITE=1\n"
                }
            }],
            "finalResponse": false
        }]);
    }
    if lower.contains("oppi-dogfood-network-disabled") {
        return json!([{
            "assistantDeltas": ["Dogfood network-disabled denial requested."],
            "toolCalls": [{
                "id": "oppi-dogfood-network-disabled",
                "name": "shell_exec",
                "namespace": "oppi",
                "arguments": {
                    "command": "node -e \"console.log('network should not run')\"",
                    "cwd": cwd,
                    "usesNetwork": true
                }
            }],
            "finalResponse": false
        }]);
    }
    if lower.contains("oppi-dogfood-missing-image") {
        return json!([{
            "assistantDeltas": ["Dogfood missing image backend check."],
            "toolCalls": [{
                "id": "oppi-dogfood-missing-image",
                "name": "image_gen",
                "namespace": "oppi",
                "arguments": { "prompt": "A small OPPi dogfood robot" }
            }],
            "finalResponse": true
        }]);
    }
    json!([{
        "assistantDeltas": ["OPPi native shell mock response from the Rust runtime."],
        "finalResponse": true
    }])
}

fn mock_tool_definitions_for_prompt(prompt: &str) -> Option<Value> {
    let lower = prompt.to_ascii_lowercase();
    if lower.contains("oppi-dogfood-missing-image") {
        return Some(json!([{
            "name": "image_gen",
            "namespace": "oppi",
            "description": "Dogfood image generation tool definition; Rust should fail closed without an approved backend.",
            "concurrencySafe": false,
            "requiresApproval": false,
            "capabilities": ["image"]
        }]));
    }
    None
}

fn mock_model_steps_after_approval(call: Option<&ToolCall>) -> Value {
    if let Some(call) = call {
        return json!([
            {
                "assistantDeltas": [],
                "toolCalls": [call],
                "finalResponse": false
            },
            {
                "assistantDeltas": ["Approved dogfood action completed."],
                "finalResponse": true
            }
        ]);
    }
    json!([{
        "assistantDeltas": ["Approved dogfood action completed."],
        "finalResponse": true
    }])
}

fn mock_model_steps_after_answer() -> Value {
    json!([{
        "assistantDeltas": ["Answered dogfood question and continuing."],
        "finalResponse": true
    }])
}

fn dogfood_background_command() -> String {
    if cfg!(windows) {
        "echo oppi-background-dogfood & ping -n 20 127.0.0.1 > nul".to_string()
    } else {
        "/bin/echo oppi-background-dogfood; sleep 20".to_string()
    }
}

fn redact_debug_text(value: &str) -> String {
    let mut text = value.replace('\n', " ");
    for marker in ["sk-", "ghp_", "xoxb-"] {
        if let Some(index) = text.find(marker) {
            text.truncate(index);
            text.push_str("<redacted>");
        }
    }
    if text.len() > 160 {
        text.truncate(160);
        text.push('…');
    }
    text
}

fn active_todos(state: &TodoState) -> Vec<String> {
    state
        .todos
        .iter()
        .filter(|todo| {
            !matches!(
                todo.status,
                oppi_protocol::TodoStatus::Completed | oppi_protocol::TodoStatus::Cancelled
            )
        })
        .map(|todo| format!("{}:{:?}", todo.id, todo.status))
        .collect()
}

fn format_todos(state: &TodoState) -> String {
    let active = active_todos(state);
    if active.is_empty() {
        "todos: none".to_string()
    } else if state.summary.trim().is_empty() {
        format!("todos: {}", active.join(" | "))
    } else {
        format!("todos: {}\nsummary: {}", active.join(" | "), state.summary)
    }
}

fn format_todos_with_summary(state: &TodoState) -> String {
    let mut rendered = format_todos(state);
    if state.todos.is_empty() && !state.summary.trim().is_empty() {
        rendered.push_str(&format!("\nsummary: {}", state.summary));
    }
    rendered
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TodoCommand {
    List,
    ClientAction {
        action: TodoClientAction,
        id: Option<String>,
    },
}

fn parse_todos_command_args(args: &[&str]) -> Result<TodoCommand, String> {
    match args {
        [] | ["list"] | ["show"] => Ok(TodoCommand::List),
        ["clear"] | ["reset"] => Ok(TodoCommand::ClientAction {
            action: TodoClientAction::Clear,
            id: None,
        }),
        ["done"] | ["complete"] => Ok(TodoCommand::ClientAction {
            action: TodoClientAction::Done,
            id: None,
        }),
        ["done", "all"] | ["complete", "all"] => Ok(TodoCommand::ClientAction {
            action: TodoClientAction::Done,
            id: None,
        }),
        ["done", id] | ["complete", id] if !id.trim().is_empty() => Ok(TodoCommand::ClientAction {
            action: TodoClientAction::Done,
            id: Some((*id).to_string()),
        }),
        _ => Err("usage: /todos [list|clear|done [id|all]]".to_string()),
    }
}

fn todo_client_action_name(action: TodoClientAction) -> &'static str {
    match action {
        TodoClientAction::Clear => "clear",
        TodoClientAction::Done => "done",
    }
}

fn parse_agent_markdown(text: &str, source: AgentSource) -> Result<AgentDefinition, String> {
    let (frontmatter, body) = split_agent_frontmatter(text)
        .ok_or_else(|| "agent markdown requires YAML frontmatter".to_string())?;
    let fields = parse_agent_frontmatter(frontmatter);
    let name = required_agent_field(&fields, "name")?;
    let description = required_agent_field(&fields, "description")?;
    let instructions = body.trim().to_string();
    if instructions.is_empty() {
        return Err("agent markdown requires instruction body".to_string());
    }
    Ok(AgentDefinition {
        name,
        description,
        source: Some(source),
        tools: parse_agent_list_field(fields.get("tools").map(String::as_str)),
        model: optional_agent_field(&fields, "model"),
        effort: optional_agent_field(&fields, "effort"),
        permission_mode: optional_agent_field(&fields, "permissionMode")
            .or_else(|| optional_agent_field(&fields, "permission_mode"))
            .map(|value| parse_agent_permission_mode(&value))
            .transpose()?,
        background: optional_agent_field(&fields, "background").is_some_and(|value| {
            matches!(
                value.to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        }),
        worktree_root: optional_agent_field(&fields, "worktreeRoot")
            .or_else(|| optional_agent_field(&fields, "worktree_root")),
        instructions,
    })
}

fn format_agent_markdown(agent: &AgentDefinition) -> String {
    let mut lines = vec![
        "---".to_string(),
        format!("name: {}", agent.name),
        format!("description: {}", quote_agent_yaml(&agent.description)),
    ];
    if !agent.tools.is_empty() {
        lines.push(format!("tools: {}", agent.tools.join(", ")));
    }
    if let Some(model) = agent.model.as_deref() {
        lines.push(format!("model: {model}"));
    }
    if let Some(effort) = agent.effort.as_deref() {
        lines.push(format!("effort: {effort}"));
    }
    if let Some(mode) = agent.permission_mode {
        lines.push(format!("permissionMode: {}", mode.as_str()));
    }
    if agent.background {
        lines.push("background: true".to_string());
    }
    if let Some(worktree_root) = agent.worktree_root.as_deref() {
        lines.push(format!("worktreeRoot: {worktree_root}"));
    }
    lines.push("---".to_string());
    lines.push(String::new());
    lines.push(agent.instructions.trim().to_string());
    lines.push(String::new());
    lines.join("\n")
}

fn split_agent_frontmatter(text: &str) -> Option<(&str, &str)> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text).trim_start();
    let rest = text.strip_prefix("---")?;
    let rest = rest
        .strip_prefix("\r\n")
        .or_else(|| rest.strip_prefix('\n'))
        .unwrap_or(rest);
    let marker = rest.find("\n---").or_else(|| rest.find("\r\n---"))?;
    let (frontmatter, after) = rest.split_at(marker);
    let body = after
        .trim_start_matches("\r\n")
        .trim_start_matches('\n')
        .trim_start_matches("---")
        .trim_start_matches("\r\n")
        .trim_start_matches('\n');
    Some((frontmatter, body))
}

fn parse_agent_frontmatter(frontmatter: &str) -> BTreeMap<String, String> {
    let mut fields = BTreeMap::new();
    for line in frontmatter.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once(':') {
            fields.insert(
                key.trim().to_string(),
                strip_agent_quotes(value.trim()).to_string(),
            );
        }
    }
    fields
}

fn required_agent_field(fields: &BTreeMap<String, String>, key: &str) -> Result<String, String> {
    optional_agent_field(fields, key).ok_or_else(|| format!("agent markdown requires {key}"))
}

fn optional_agent_field(fields: &BTreeMap<String, String>, key: &str) -> Option<String> {
    fields
        .get(key)
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn parse_agent_list_field(value: Option<&str>) -> Vec<String> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Vec::new();
    };
    let value = value
        .strip_prefix('[')
        .and_then(|inner| inner.strip_suffix(']'))
        .unwrap_or(value);
    value
        .split(',')
        .map(|item| strip_agent_quotes(item.trim()).trim().to_string())
        .filter(|item| !item.is_empty())
        .collect()
}

fn parse_agent_permission_mode(value: &str) -> Result<PermissionMode, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "read-only" | "readonly" | "ro" => Ok(PermissionMode::ReadOnly),
        "default" => Ok(PermissionMode::Default),
        "auto-review" | "autoreview" | "auto" => Ok(PermissionMode::AutoReview),
        "full-access" | "full" | "danger" => Ok(PermissionMode::FullAccess),
        other => Err(format!("unknown agent permissionMode: {other}")),
    }
}

fn strip_agent_quotes(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .or_else(|| {
            value
                .strip_prefix('\'')
                .and_then(|value| value.strip_suffix('\''))
        })
        .unwrap_or(value)
}

fn quote_agent_yaml(value: &str) -> String {
    format!(
        "\"{}\"",
        value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AgentMarkdownTarget {
    Project,
    User,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AgentImportRoute {
    source: AgentSource,
    path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AgentExportRoute {
    agent_name: String,
    path: PathBuf,
}

fn parse_agent_import_route(args: &[&str], cwd: &Path) -> Result<AgentImportRoute, String> {
    let (source, path) = match args {
        ["project", path] => (AgentSource::Project, *path),
        ["user", path] => (AgentSource::User, *path),
        [path] => (AgentSource::Project, *path),
        _ => return Err("usage: /agents import [project|user] <path.md>".to_string()),
    };
    Ok(AgentImportRoute {
        source,
        path: resolve_agent_markdown_path(cwd, path),
    })
}

fn parse_agent_export_route(
    args: &[&str],
    cwd: &Path,
    agent_dir: &Path,
) -> Result<AgentExportRoute, String> {
    let Some(agent_name) = args
        .first()
        .copied()
        .filter(|value| !value.trim().is_empty())
    else {
        return Err("usage: /agents export <name> [project|user|path.md]".to_string());
    };
    let path = match args.get(1).copied() {
        None | Some("project") => {
            default_agent_markdown_path(cwd, agent_dir, AgentMarkdownTarget::Project, agent_name)?
        }
        Some("user") => {
            default_agent_markdown_path(cwd, agent_dir, AgentMarkdownTarget::User, agent_name)?
        }
        Some(path) => resolve_agent_markdown_path(cwd, path),
    };
    Ok(AgentExportRoute {
        agent_name: agent_name.to_string(),
        path,
    })
}

fn active_agent_from_list(value: Value, agent_name: &str) -> Result<AgentDefinition, String> {
    let result: RuntimeListResult<ResolvedAgent> =
        serde_json::from_value(value).map_err(|error| format!("decode agents/list: {error}"))?;
    let names = result
        .items
        .iter()
        .map(|item| item.active.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    result
        .items
        .into_iter()
        .find(|item| item.active.name == agent_name)
        .map(|item| item.active)
        .ok_or_else(|| {
            if names.is_empty() {
                format!("unknown agent {agent_name}; no agents are registered")
            } else {
                format!("unknown agent {agent_name}; available agents: {names}")
            }
        })
}

fn resolve_agent_markdown_path(cwd: &Path, path: &str) -> PathBuf {
    let path = PathBuf::from(path);
    if path.is_absolute() {
        path
    } else {
        cwd.join(path)
    }
}

fn default_agent_markdown_path(
    cwd: &Path,
    agent_dir: &Path,
    target: AgentMarkdownTarget,
    agent_name: &str,
) -> Result<PathBuf, String> {
    let file_name = format!("{}.md", agent_markdown_file_stem(agent_name)?);
    let dir = match target {
        AgentMarkdownTarget::Project => cwd.join(".oppi").join("agents"),
        AgentMarkdownTarget::User => agent_dir.join("oppi").join("agents"),
    };
    Ok(dir.join(file_name))
}

fn agent_markdown_file_stem(agent_name: &str) -> Result<String, String> {
    let mut stem = String::new();
    let mut last_dash = false;
    for ch in agent_name.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            stem.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if matches!(ch, '-' | '_' | ' ') && !stem.is_empty() && !last_dash {
            stem.push('-');
            last_dash = true;
        }
    }
    while stem.ends_with('-') {
        stem.pop();
    }
    if stem.is_empty() {
        Err("agent name cannot produce a markdown file name".to_string())
    } else {
        Ok(stem)
    }
}

fn goal_command_route(args: &str) -> Result<GoalCommandRoute, String> {
    let args = args.trim();
    if args.is_empty() {
        return Ok(GoalCommandRoute::Get);
    }

    let mut parts = args.split_whitespace();
    let action = parts.next().unwrap_or_default();
    let rest = parts.collect::<Vec<_>>().join(" ");
    match action.to_ascii_lowercase().as_str() {
        "clear" => Ok(GoalCommandRoute::Clear),
        "pause" => Ok(GoalCommandRoute::Set {
            objective: None,
            status: Some(ThreadGoalStatus::Paused),
            token_budget: GoalBudgetRoute::Unchanged,
        }),
        "resume" => Ok(GoalCommandRoute::Set {
            objective: None,
            status: Some(ThreadGoalStatus::Active),
            token_budget: GoalBudgetRoute::Unchanged,
        }),
        "done" | "complete" => Ok(GoalCommandRoute::Set {
            objective: None,
            status: Some(ThreadGoalStatus::Complete),
            token_budget: GoalBudgetRoute::Unchanged,
        }),
        "budget" => {
            let budget = rest.trim();
            if budget.is_empty() {
                return Err("usage: /goal budget <positive-token-count>|clear".to_string());
            }
            if budget.eq_ignore_ascii_case("clear") {
                return Ok(GoalCommandRoute::Set {
                    objective: None,
                    status: None,
                    token_budget: GoalBudgetRoute::Clear,
                });
            }
            let parsed = budget
                .parse::<i64>()
                .map_err(|_| "goal token budget must be a positive integer".to_string())?;
            if parsed <= 0 {
                return Err("goal token budget must be a positive integer".to_string());
            }
            Ok(GoalCommandRoute::Set {
                objective: None,
                status: None,
                token_budget: GoalBudgetRoute::Set(parsed),
            })
        }
        "replace" => {
            let objective = rest.trim();
            if objective.is_empty() {
                return Err("usage: /goal replace <objective>".to_string());
            }
            Ok(GoalCommandRoute::Set {
                objective: Some(objective.to_string()),
                status: Some(ThreadGoalStatus::Active),
                token_budget: GoalBudgetRoute::Unchanged,
            })
        }
        _ => Ok(GoalCommandRoute::CreateObjective(args.to_string())),
    }
}

fn goal_set_params(
    thread_id: &str,
    objective: Option<String>,
    status: Option<ThreadGoalStatus>,
    token_budget: GoalBudgetRoute,
) -> Value {
    let mut params = json!({ "threadId": thread_id });
    if let Some(objective) = objective {
        params["objective"] = json!(objective);
    }
    if let Some(status) = status {
        params["status"] = json!(thread_goal_status_rpc_label(status));
    }
    match token_budget {
        GoalBudgetRoute::Unchanged => {}
        GoalBudgetRoute::Set(budget) => params["tokenBudget"] = json!(budget),
        GoalBudgetRoute::Clear => params["tokenBudget"] = Value::Null,
    }
    params
}

fn thread_goal_status_rpc_label(status: ThreadGoalStatus) -> &'static str {
    match status {
        ThreadGoalStatus::Active => "active",
        ThreadGoalStatus::Paused => "paused",
        ThreadGoalStatus::BudgetLimited => "budgetLimited",
        ThreadGoalStatus::Complete => "complete",
    }
}

fn decode_goal_from_response(value: &Value) -> Result<Option<ThreadGoal>, String> {
    let Some(goal) = value.get("goal") else {
        return Ok(None);
    };
    if goal.is_null() {
        return Ok(None);
    }
    serde_json::from_value(goal.clone())
        .map(Some)
        .map_err(|error| format!("decode thread goal: {error}"))
}

fn format_goal_response(value: &Value) -> String {
    let Ok(Some(goal)) = decode_goal_from_response(value) else {
        return "goal: none\nusage: /goal <objective>".to_string();
    };
    let budget = goal
        .token_budget
        .map(|budget| budget.to_string())
        .unwrap_or_else(|| "none".to_string());
    format!(
        "Goal {}: {}\ntime: {}s; tokens: {}; budget: {}",
        thread_goal_status_label(goal.status),
        goal.objective,
        goal.time_used_seconds,
        goal.tokens_used,
        budget
    )
}

fn thread_goal_status_label(status: ThreadGoalStatus) -> &'static str {
    match status {
        ThreadGoalStatus::Active => "active",
        ThreadGoalStatus::Paused => "paused",
        ThreadGoalStatus::BudgetLimited => "budget-limited",
        ThreadGoalStatus::Complete => "complete",
    }
}

fn format_goal_status_label(goal: Option<&ThreadGoal>) -> String {
    let Some(goal) = goal else {
        return "none".to_string();
    };
    match goal.status {
        ThreadGoalStatus::Active => {
            format!(
                "goal active {}",
                format_goal_duration(goal.time_used_seconds)
            )
        }
        ThreadGoalStatus::Paused => "goal paused".to_string(),
        ThreadGoalStatus::BudgetLimited => {
            if let Some(budget) = goal.token_budget {
                format!(
                    "goal budget {}/{}",
                    format_compact_i64(goal.tokens_used),
                    format_compact_i64(budget)
                )
            } else {
                "goal budget".to_string()
            }
        }
        ThreadGoalStatus::Complete => {
            format!(
                "goal complete {} tokens",
                format_compact_i64(goal.tokens_used)
            )
        }
    }
}

fn format_goal_duration(seconds: i64) -> String {
    let seconds = seconds.max(0);
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3_600 {
        format!("{}m", seconds / 60)
    } else {
        let hours = seconds / 3_600;
        let minutes = (seconds % 3_600) / 60;
        if minutes == 0 {
            format!("{hours}h")
        } else {
            format!("{hours}h{minutes}m")
        }
    }
}

fn format_compact_i64(value: i64) -> String {
    let abs = value.abs();
    if abs >= 1_000_000 {
        trim_compact_suffix(format!("{:.1}M", value as f64 / 1_000_000.0))
    } else if abs >= 1_000 {
        trim_compact_suffix(format!("{:.1}K", value as f64 / 1_000.0))
    } else {
        value.to_string()
    }
}

fn trim_compact_suffix(value: String) -> String {
    value.replace(".0K", "K").replace(".0M", "M")
}

fn format_memory_control(value: &Value) -> String {
    let title = value
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("Hoppi memory");
    let summary = value
        .get("summary")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let status = value.get("status").cloned().unwrap_or_else(|| json!({}));
    let enabled = status
        .get("enabled")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let backend = status
        .get("backend")
        .and_then(Value::as_str)
        .unwrap_or("none");
    let scope = status
        .get("scope")
        .and_then(Value::as_str)
        .unwrap_or("project");
    let count = status
        .get("memoryCount")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let mut lines = vec![
        title.to_string(),
        format!(
            "status: {} backend={} scope={} memories={}",
            if enabled { "enabled" } else { "disabled" },
            backend,
            scope,
            count
        ),
    ];
    if !summary.trim().is_empty() {
        lines.push(summary.to_string());
    }
    let controls = value
        .get("controls")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if !controls.is_empty() {
        lines.push("controls:".to_string());
        for control in controls {
            let label = control
                .get("label")
                .and_then(Value::as_str)
                .unwrap_or("control");
            let command = control
                .get("command")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let description = control
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or_default();
            lines.push(format!("- {label}: {command}"));
            if !description.is_empty() {
                lines.push(format!("  {description}"));
            }
        }
    }
    lines.join("\n")
}

fn terminal_width() -> usize {
    env::var("COLUMNS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|width| *width >= 20)
        .unwrap_or(100)
}

fn terminal_capability_summary(raw_requested: bool) -> &'static str {
    if raw_requested {
        if cfg!(windows) {
            "terminal: raw byte parser enabled; Windows consoles may degrade Ctrl/Alt combos unless the terminal emits ANSI key sequences; line-mode fallback remains available."
        } else {
            "terminal: raw byte parser enabled; terminals with kitty/CSI-u style Ctrl/Shift+Enter get richer keys; line-mode fallback remains available."
        }
    } else if ansi_enabled() {
        "keybindings: type `/` to open the scrollable slash-command palette; ↑↓/PgUp/PgDn choose, Tab completes, Enter chooses/runs complete items, Esc closes. Enter submit, busy Enter queue, Alt+Enter explicit queue, Ctrl+Enter steer, Alt+Up restore, Shift+Enter newline, Escape/Ctrl+C interrupt-or-clear, Ctrl+C twice runs /exit, Ctrl+D safe exit. Raw capture is opt-in with --raw; unsupported terminals degrade to line-mode commands (/steer, /interrupt, /again). Color: ANSI enabled."
    } else {
        "keybindings: type `/` to open the scrollable slash-command palette where raw TUI is available; Tab completes and Enter chooses/runs complete items. Raw capture is opt-in with --raw but this terminal is plain/no-color; unsupported key chords degrade to line-mode commands (/steer, /interrupt, /again). Color: plain fallback."
    }
}

fn width_safe_line(line: &str, width: usize) -> String {
    let width = width.max(1);
    let mut chars = line.chars();
    let mut output = String::new();
    for _ in 0..width {
        let Some(ch) = chars.next() else {
            return line.to_string();
        };
        output.push(ch);
    }
    if chars.next().is_some() {
        if width == 1 {
            "…".to_string()
        } else {
            output.pop();
            output.push('…');
            output
        }
    } else {
        line.to_string()
    }
}

fn login_usage() -> &'static str {
    "usage: /login, /login subscription, /login subscription codex [--force], /login subscription copilot [--force] [--enterprise <domain>], /login api, /login subscription claude [status|install --yes|start|use|stop], /login api openai env <ENV>. Model choice stays in /model or future /settings; raw keys are never accepted."
}

fn normalize_login_choice(input: &str) -> String {
    input
        .trim()
        .trim_start_matches('/')
        .to_ascii_lowercase()
        .replace('_', "-")
}

fn login_action_approved(flags: &[&str]) -> bool {
    flags.iter().any(|flag| {
        matches!(
            flag.trim().to_ascii_lowercase().as_str(),
            "--yes" | "yes" | "--approve" | "approve" | "--i-understand"
        )
    })
}

fn login_root_picker_panel(provider: &ProviderConfig) -> String {
    format!(
        "login: native provider setup\ncurrent: {}\n\nChoose auth type:\n  1. Subscription\n  2. API\n\nType 1/2, or run `/login subscription` / `/login api`.\npolicy: credentials stay in Pi/OPPi auth store, env references, or a managed provider bridge; raw secrets are not stored or sent over JSON-RPC.\n{}",
        provider.label(),
        login_usage()
    )
}

fn login_subscription_picker_panel() -> String {
    "login subscription providers\n  1. Codex (ChatGPT) — native browser OAuth; saves redacted Pi/OPPi auth-store token\n  2. Claude (Anthropic via Meridian) — explicit managed loopback bridge; install asks approval\n  3. Copilot (Microsoft/GitHub) — Pi-compatible device-code OAuth; saves redacted auth-store token\n\nType 1/2/3, or run `/login subscription codex`, `/login subscription copilot`, or `/login subscription claude`.".to_string()
}

fn login_api_picker_panel() -> String {
    "login API providers\n  1. OpenAI-compatible API via env reference\n\nSet OPPI_OPENAI_API_KEY/OPENAI_API_KEY in your shell, then run `/login api openai env OPPI_OPENAI_API_KEY`. Raw keys are rejected. Use `/model <id>` for model selection.".to_string()
}

fn login_openai_instructions() -> &'static str {
    "login API/OpenAI-compatible: set OPPI_OPENAI_API_KEY or OPENAI_API_KEY in your shell, then run `/login api openai env OPPI_OPENAI_API_KEY`. Use `/model <id>` or `/model role <role> <id>` for model selection. Raw API keys are rejected."
}

fn login_claude_picker_panel() -> String {
    format!(
        "login Claude (Anthropic via Meridian)\n  1. Status\n  2. Run explicit Claude Code login (`claude login`)\n  3. Install managed Meridian bridge (asks approval; package {MERIDIAN_PACKAGE_NAME})\n  4. Start visible loopback bridge and configure provider\n  5. Use already-running bridge\n  6. Stop bridge started by this shell\n\nMeridian authenticates through the Claude Code SDK and refreshes tokens there; OPPi does not extract or store Claude tokens. Model selection stays in `/model` or `/model role ...`; `/login` never picks the model. No hidden npx/proxy/credential-helper spawning is used. Managed install target: {}",
        format_path_for_display(&managed_packages_dir())
    )
}

fn login_meridian_install_approval_panel() -> String {
    format!(
        "Approval required: install managed Meridian bridge package {MERIDIAN_PACKAGE_NAME} into {}. This may use npm/network and can modify OPPi's managed packages directory. Type `yes` to approve, `no` to cancel, or run `/login subscription claude install --yes`.",
        format_path_for_display(&managed_packages_dir())
    )
}

fn login_delegated_provider_text(provider: &str) -> &'static str {
    match provider {
        "chatgpt" | "codex" => {
            "login chatgpt/codex: native OAuth is available through `/login subscription codex`; it opens the browser, saves tokens in the protected Pi/OPPi auth store, and never prints raw tokens."
        }
        "copilot" | "github" => {
            "login copilot: native GitHub Copilot OAuth is available through `/login subscription copilot [--enterprise <domain>]`; it uses Pi's device-code flow, saves tokens in the protected auth store, and never prints raw tokens."
        }
        "gemini" => {
            "login gemini: stable Pi already supports Gemini CLI via /login. Native OAuth storage is still pending, so use stable `oppi` /login for now; the native shell will not attempt hidden auth."
        }
        "antigravity" => {
            "login antigravity: stable Pi already supports Google Antigravity via /login. Native OAuth storage is still pending, so use stable `oppi` /login for now; the native shell will not attempt hidden auth."
        }
        _ => {
            "login: provider is not native yet; use stable `oppi` /login or configure an explicit OpenAI-compatible endpoint."
        }
    }
}

#[derive(Debug, Clone)]
struct GraphifyStatus {
    cli_path: Option<PathBuf>,
    graph_root: Option<PathBuf>,
    legacy_root: Option<PathBuf>,
    report_path: Option<PathBuf>,
    wiki_index_path: Option<PathBuf>,
    graph_json_path: Option<PathBuf>,
    needs_update: bool,
    config_path: Option<PathBuf>,
}

impl GraphifyStatus {
    fn configured(&self) -> bool {
        self.graph_root.is_some() || self.legacy_root.is_some() || self.config_path.is_some()
    }
}

fn executable_names(name: &str) -> Vec<String> {
    if cfg!(windows) {
        let pathext = env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string());
        let mut names = vec![name.to_string()];
        for ext in pathext.split(';').filter(|ext| !ext.trim().is_empty()) {
            names.push(format!("{}{}", name, ext.to_ascii_lowercase()));
            names.push(format!("{}{}", name, ext.to_ascii_uppercase()));
        }
        names.sort();
        names.dedup();
        names
    } else {
        vec![name.to_string()]
    }
}

fn find_executable_on_path(name: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    let names = executable_names(name);
    for dir in env::split_paths(&path) {
        for candidate in &names {
            let path = dir.join(candidate);
            if path.is_file() {
                return Some(path);
            }
        }
    }
    None
}

fn graphify_status(cwd: &str) -> GraphifyStatus {
    let cwd = PathBuf::from(cwd);
    let graph_root = cwd.join(".graphify");
    let legacy_root = cwd.join("graphify-out");
    let active_root = if graph_root.exists() {
        Some(graph_root.clone())
    } else if legacy_root.exists() {
        Some(legacy_root.clone())
    } else {
        None
    };
    let artifact = |name: &str| {
        active_root
            .as_ref()
            .map(|root| root.join(name))
            .filter(|path| path.exists())
    };
    GraphifyStatus {
        cli_path: find_executable_on_path("graphify"),
        graph_root: graph_root.exists().then_some(graph_root.clone()),
        legacy_root: legacy_root.exists().then_some(legacy_root),
        report_path: artifact("GRAPH_REPORT.md"),
        wiki_index_path: active_root
            .as_ref()
            .map(|root| root.join("wiki").join("index.md"))
            .filter(|path| path.exists()),
        graph_json_path: artifact("graph.json"),
        needs_update: graph_root.join("needs_update").exists()
            || active_root
                .as_ref()
                .is_some_and(|root| root.join("needs_update").exists()),
        config_path: [cwd.join("graphify.yaml"), cwd.join("graphify.yml")]
            .into_iter()
            .find(|path| path.exists()),
    }
}

fn graphify_status_json(status: &GraphifyStatus) -> Value {
    json!({
        "cliPath": status.cli_path.as_ref().map(|path| path.display().to_string()),
        "configured": status.configured(),
        "graphRoot": status.graph_root.as_ref().map(|path| path.display().to_string()),
        "legacyRoot": status.legacy_root.as_ref().map(|path| path.display().to_string()),
        "reportPath": status.report_path.as_ref().map(|path| path.display().to_string()),
        "wikiIndexPath": status.wiki_index_path.as_ref().map(|path| path.display().to_string()),
        "graphJsonPath": status.graph_json_path.as_ref().map(|path| path.display().to_string()),
        "needsUpdate": status.needs_update,
        "configPath": status.config_path.as_ref().map(|path| path.display().to_string()),
        "installGuidance": if status.cli_path.is_some() { Value::Null } else { json!("Ask before installing: npm install -g graphifyy; graphify install") },
    })
}

fn graphify_path(path: &Option<PathBuf>) -> String {
    path.as_ref()
        .map(|path| format_path_for_display(path))
        .unwrap_or_else(|| "missing".to_string())
}

fn format_graphify_status(status: &GraphifyStatus) -> String {
    let mut lines = vec![
        "Graphify codebase graph".to_string(),
        format!("cli: {}", graphify_path(&status.cli_path)),
        format!("configured: {}", status.configured()),
        format!("graphRoot: {}", graphify_path(&status.graph_root)),
        format!("legacyRoot: {}", graphify_path(&status.legacy_root)),
        format!("wiki: {}", graphify_path(&status.wiki_index_path)),
        format!("report: {}", graphify_path(&status.report_path)),
        format!("graphJson: {}", graphify_path(&status.graph_json_path)),
        format!("needsUpdate: {}", status.needs_update),
    ];
    if let Some(config) = &status.config_path {
        lines.push(format!("config: {}", format_path_for_display(config)));
    }
    if status.cli_path.is_none() || !status.configured() {
        lines.push("setup: ask first, then run `npm install -g graphifyy` and `graphify install`; OPPi will not install or mutate hooks silently.".to_string());
    }
    lines.push("usage: /skill:graphify or ask architecture/dependency questions; prefer graph artifacts before broad raw search.".to_string());
    lines.join("\n")
}

fn graphify_install_guidance() -> &'static str {
    "Graphify install is user-approved only. Suggested commands after approval:\n  npm install -g graphifyy\n  graphify install\n  graphify scope inspect . --scope auto\n  graphify detect .\nThe install command prints a mutation preview before writing assistant instructions, hooks, MCP, or plugin config. OPPi will not run it silently."
}

fn graphify_command_guidance() -> &'static str {
    "Graphify commands:\n  graphify scope inspect . --scope auto\n  graphify detect .\n  graphify update .\n  graphify query \"show the auth flow\"\n  graphify path \"Frontend\" \"Database\"\n  graphify explain \"DigestAuth\"\n  graphify review-analysis\n  graphify portable-check .graphify\nUse --all only for intentional full recursive knowledge/document scans."
}

fn current_provider_model(provider: &ProviderConfig) -> Option<&str> {
    match provider {
        ProviderConfig::Mock => Some("mock-scripted"),
        ProviderConfig::OpenAiCompatible(config) => Some(config.model.as_str()),
    }
}

fn provider_name(provider: &ProviderConfig) -> &'static str {
    match provider {
        ProviderConfig::Mock => "mock",
        ProviderConfig::OpenAiCompatible(config) => match config.flavor {
            DirectProviderFlavor::OpenAiCompatible => "openai-compatible",
            DirectProviderFlavor::OpenAiCodex => "openai-codex",
            DirectProviderFlavor::GitHubCopilot => "github-copilot",
        },
    }
}

fn with_default_reasoning_effort(mut config: OpenAiCompatibleConfig) -> OpenAiCompatibleConfig {
    if config.reasoning_effort.is_none() {
        config.reasoning_effort = default_reasoning_effort_for_config(&config);
    }
    config
}

fn default_reasoning_effort_for_config(config: &OpenAiCompatibleConfig) -> Option<String> {
    let provider = ProviderConfig::OpenAiCompatible(config.clone());
    let level = if config.model == GPT_MAIN_DEFAULT_MODEL {
        ThinkingLevel::XHigh
    } else if is_anthropic_like_model(&provider, &config.model) {
        ThinkingLevel::High
    } else {
        recommended_effort_level_for_provider(&provider)
    };
    if level == ThinkingLevel::Off
        || !allowed_effort_levels_for_model(&provider, &config.model).contains(&level)
    {
        None
    } else {
        Some(level.as_str().to_string())
    }
}

fn normalize_thinking_level(input: &str) -> Option<ThinkingLevel> {
    match input.trim().to_ascii_lowercase().as_str() {
        "off" | "none" => Some(ThinkingLevel::Off),
        "minimal" | "min" => Some(ThinkingLevel::Minimal),
        "low" => Some(ThinkingLevel::Low),
        "medium" | "med" => Some(ThinkingLevel::Medium),
        "high" => Some(ThinkingLevel::High),
        "xhigh" | "max" => Some(ThinkingLevel::XHigh),
        _ => None,
    }
}

fn normalize_current_thinking_level(value: Option<&str>) -> ThinkingLevel {
    value
        .and_then(normalize_thinking_level)
        .unwrap_or(ThinkingLevel::Off)
}

fn effort_model_name(provider: &ProviderConfig) -> String {
    format!(
        "{}/{}",
        provider_name(provider),
        current_provider_model(provider).unwrap_or("unknown")
    )
}

fn is_anthropic_like_model(provider: &ProviderConfig, model: &str) -> bool {
    let provider_name = provider_name(provider).to_ascii_lowercase();
    let id = model.to_ascii_lowercase();
    provider_name.contains("anthropic")
        || provider_name == "meridian"
        || provider_uses_meridian(provider)
        || id.contains("claude")
        || id.contains("opus")
        || id.contains("sonnet")
}

fn provider_uses_meridian(provider: &ProviderConfig) -> bool {
    matches!(provider, ProviderConfig::OpenAiCompatible(config) if provider_uses_meridian_placeholder(config))
}

fn supports_xhigh_effort(provider: &ProviderConfig, model: &str) -> bool {
    let id = model.to_ascii_lowercase();
    id.contains("gpt-5.2")
        || id.contains("gpt-5.3")
        || id.contains("gpt-5.4")
        || id.contains("gpt-5.5")
        || id.contains("gpt-5.1-codex-max")
        || id.contains("opus-4-6")
        || id.contains("opus-4.6")
        || id.contains("opus-4-7")
        || id.contains("opus-4.7")
        || (provider_uses_meridian(provider) && (id.contains("opus") || id.contains("sonnet")))
}

fn is_reasoning_capable_model(provider: &ProviderConfig, model: &str) -> bool {
    let provider_label = provider_name(provider).to_ascii_lowercase();
    let id = model.to_ascii_lowercase();
    (provider_label.contains("copilot")
        && (id.contains("gpt-5")
            || id.contains("o3")
            || id.contains("o4")
            || id.contains("claude")
            || id.contains("sonnet")
            || id.contains("opus")
            || id.contains("gemini")))
        || id.contains("gpt-5")
        || id.contains("o3")
        || id.contains("o4")
        || is_anthropic_like_model(provider, model)
        || id.contains("gemini-2.5")
        || id.contains("gemini-3")
}

fn allowed_effort_levels_for_model(provider: &ProviderConfig, model: &str) -> Vec<ThinkingLevel> {
    if !is_reasoning_capable_model(provider, model) {
        return vec![ThinkingLevel::Off];
    }
    ALL_THINKING_LEVELS
        .into_iter()
        .filter(|level| *level != ThinkingLevel::XHigh || supports_xhigh_effort(provider, model))
        .collect()
}

fn allowed_effort_levels_for_provider(provider: &ProviderConfig) -> Vec<ThinkingLevel> {
    let model = current_provider_model(provider).unwrap_or("unknown");
    allowed_effort_levels_for_model(provider, model)
}

fn recommended_effort_level_for_provider(provider: &ProviderConfig) -> ThinkingLevel {
    let model = current_provider_model(provider).unwrap_or("unknown");
    if !is_reasoning_capable_model(provider, model) {
        return ThinkingLevel::Off;
    }
    let id = model.to_ascii_lowercase();
    let provider_label = provider_name(provider).to_ascii_lowercase();
    if id.contains("gpt-5.5") {
        return ThinkingLevel::XHigh;
    }
    if is_anthropic_like_model(provider, model) {
        return ThinkingLevel::High;
    }
    if provider_label == "openai"
        || provider_label.contains("openai")
        || provider_label.contains("copilot")
        || id.contains("gpt-5")
        || id.contains("o3")
        || id.contains("o4")
    {
        return if supports_xhigh_effort(provider, model) {
            ThinkingLevel::High
        } else {
            ThinkingLevel::Medium
        };
    }
    if provider_label.contains("google") || id.contains("gemini") {
        return ThinkingLevel::Medium;
    }
    ThinkingLevel::Medium
}

fn current_effort_level_for_provider(provider: &ProviderConfig) -> ThinkingLevel {
    match provider {
        ProviderConfig::Mock => ThinkingLevel::Off,
        ProviderConfig::OpenAiCompatible(config) => {
            normalize_current_thinking_level(config.reasoning_effort.as_deref())
        }
    }
}

fn effort_level_label_for_provider(
    provider: &ProviderConfig,
    level: ThinkingLevel,
) -> &'static str {
    let model = current_provider_model(provider).unwrap_or("unknown");
    if level == ThinkingLevel::XHigh && is_anthropic_like_model(provider, model) {
        "Max"
    } else {
        level.label()
    }
}

fn set_provider_effort_level(provider: &mut ProviderConfig, level: ThinkingLevel) {
    if let ProviderConfig::OpenAiCompatible(config) = provider {
        config.reasoning_effort = if level == ThinkingLevel::Off {
            None
        } else {
            Some(level.as_str().to_string())
        };
    }
}

fn format_effort_status(provider: &ProviderConfig) -> String {
    let current = current_effort_level_for_provider(provider);
    let recommended = recommended_effort_level_for_provider(provider);
    let allowed = allowed_effort_levels_for_provider(provider);
    let allowed_text = allowed
        .iter()
        .map(|level| level.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let max = allowed.last().copied().unwrap_or(ThinkingLevel::Off);
    let support = if allowed.as_slice() == [ThinkingLevel::Off] {
        "Current model is not marked reasoning-capable, so the rail is locked to Off.".to_string()
    } else {
        format!(
            "Current model's rail caps at {}.",
            effort_level_label_for_provider(provider, max)
        )
    };
    format!(
        "effort: {} for {}\nrecommended: {}\nallowed: {}\n{}\nuse /effort auto or /effort <level>; bare /effort opens the native slider in the TUI",
        effort_level_label_for_provider(provider, current),
        effort_model_name(provider),
        effort_level_label_for_provider(provider, recommended),
        allowed_text,
        support
    )
}

fn effort_status_json(provider: &ProviderConfig) -> Value {
    let current = current_effort_level_for_provider(provider);
    let recommended = recommended_effort_level_for_provider(provider);
    let allowed = allowed_effort_levels_for_provider(provider);
    json!({
        "model": effort_model_name(provider),
        "current": current.as_str(),
        "currentLabel": effort_level_label_for_provider(provider, current),
        "recommended": recommended.as_str(),
        "recommendedLabel": effort_level_label_for_provider(provider, recommended),
        "allowed": allowed.iter().map(|level| level.as_str()).collect::<Vec<_>>(),
        "reasoningCapable": allowed.as_slice() != [ThinkingLevel::Off],
    })
}

fn native_model_catalog_for_provider(provider: &ProviderConfig) -> &'static [&'static str] {
    match provider {
        ProviderConfig::Mock => &["mock-scripted"],
        ProviderConfig::OpenAiCompatible(config)
            if config.flavor == DirectProviderFlavor::OpenAiCompatible
                && provider_uses_meridian_placeholder(config) =>
        {
            MERIDIAN_MODEL_CATALOG
        }
        ProviderConfig::OpenAiCompatible(config) => match config.flavor {
            DirectProviderFlavor::OpenAiCompatible => {
                if config.base_url.is_none() {
                    OPENAI_DIRECT_MODEL_CATALOG
                } else {
                    &[]
                }
            }
            DirectProviderFlavor::OpenAiCodex => OPENAI_CODEX_MODEL_CATALOG,
            DirectProviderFlavor::GitHubCopilot => GITHUB_COPILOT_MODEL_CATALOG,
        },
    }
}

fn model_ref_for_provider(provider: &ProviderConfig, model: &str, role: Option<&str>) -> ModelRef {
    ModelRef {
        id: model.to_string(),
        provider: provider_name(provider).to_string(),
        display_name: model.to_string(),
        role: role.map(str::to_string),
    }
}

fn models_json_path() -> PathBuf {
    env::var_os("OPPI_MODELS_JSON_PATH")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| default_agent_dir().join("models.json"))
}

fn provider_aliases_for_models_json(provider: &ProviderConfig) -> Vec<&'static str> {
    match provider {
        ProviderConfig::Mock => vec!["mock"],
        ProviderConfig::OpenAiCompatible(config)
            if config.flavor == DirectProviderFlavor::OpenAiCompatible
                && provider_uses_meridian_placeholder(config) =>
        {
            vec!["meridian", "anthropic"]
        }
        ProviderConfig::OpenAiCompatible(config) => match config.flavor {
            DirectProviderFlavor::OpenAiCompatible => vec!["openai-compatible", "openai"],
            DirectProviderFlavor::OpenAiCodex => vec!["openai-codex"],
            DirectProviderFlavor::GitHubCopilot => vec!["github-copilot"],
        },
    }
}

fn custom_model_ids_from_models_json(provider: &ProviderConfig) -> Vec<String> {
    let path = models_json_path();
    let Ok(raw) = fs::read_to_string(&path) else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<Value>(&raw) else {
        return Vec::new();
    };
    let Some(providers) = value.get("providers").and_then(Value::as_object) else {
        return Vec::new();
    };
    let aliases = provider_aliases_for_models_json(provider);
    let mut out = Vec::new();
    for alias in aliases {
        let Some(models) = providers
            .get(alias)
            .and_then(|provider| provider.get("models"))
            .and_then(Value::as_array)
        else {
            continue;
        };
        for model in models {
            if let Some(id) = model.get("id").and_then(Value::as_str) {
                let id = id.trim();
                if !id.is_empty() && !out.iter().any(|existing| existing == id) {
                    out.push(id.to_string());
                }
            }
        }
    }
    out
}

fn native_model_ids_for_provider(provider: &ProviderConfig) -> Vec<String> {
    let mut seen = BTreeSet::<String>::new();
    let mut out = Vec::<String>::new();
    for model in native_model_catalog_for_provider(provider) {
        if seen.insert((*model).to_string()) {
            out.push((*model).to_string());
        }
    }
    for model in custom_model_ids_from_models_json(provider) {
        if seen.insert(model.clone()) {
            out.push(model);
        }
    }
    out
}

fn thinking_suffix_stripped(pattern: &str) -> &str {
    let Some((prefix, suffix)) = pattern.rsplit_once(':') else {
        return pattern;
    };
    match suffix {
        "off" | "minimal" | "low" | "medium" | "high" | "xhigh" => prefix,
        _ => pattern,
    }
}

fn wildcard_match(pattern: &str, value: &str) -> bool {
    fn inner(pattern: &[u8], value: &[u8]) -> bool {
        match pattern.split_first() {
            None => value.is_empty(),
            Some((&b'*', rest)) => {
                inner(rest, value) || (!value.is_empty() && inner(pattern, &value[1..]))
            }
            Some((&b'?', rest)) => !value.is_empty() && inner(rest, &value[1..]),
            Some((&p, rest)) => {
                !value.is_empty() && p.eq_ignore_ascii_case(&value[0]) && inner(rest, &value[1..])
            }
        }
    }
    inner(pattern.as_bytes(), value.as_bytes())
}

fn model_matches_scope_pattern(provider: &ProviderConfig, model: &str, pattern: &str) -> bool {
    let pattern = thinking_suffix_stripped(pattern.trim());
    if pattern.is_empty() {
        return false;
    }
    let model_lower = model.to_ascii_lowercase();
    let canonicals = provider_aliases_for_models_json(provider)
        .into_iter()
        .map(|provider| format!("{}/{model_lower}", provider.to_ascii_lowercase()))
        .collect::<Vec<_>>();
    let pattern_lower = pattern.to_ascii_lowercase();
    if pattern_lower.contains('*') || pattern_lower.contains('?') {
        wildcard_match(&pattern_lower, &model_lower)
            || canonicals
                .iter()
                .any(|canonical| wildcard_match(&pattern_lower, canonical))
    } else {
        pattern_lower == model_lower
            || canonicals
                .iter()
                .any(|canonical| pattern_lower == *canonical)
    }
}

fn scoped_model_ids_for_patterns(patterns: &[String], provider: &ProviderConfig) -> Vec<String> {
    if patterns.is_empty() {
        return Vec::new();
    }
    let catalog = native_model_ids_for_provider(provider);
    let mut seen = BTreeSet::<String>::new();
    let mut out = Vec::new();
    for pattern in patterns {
        for model in &catalog {
            if model_matches_scope_pattern(provider, model, pattern) && seen.insert(model.clone()) {
                out.push(model.clone());
            }
        }
    }
    out
}

fn scoped_model_ids_for_provider(session: &ShellSession, provider: &ProviderConfig) -> Vec<String> {
    scoped_model_ids_for_patterns(&session.scoped_model_ids, provider)
}

fn main_model_ids_for_selection(
    current: Option<&str>,
    scoped_patterns: &[String],
    provider: &ProviderConfig,
) -> Vec<String> {
    let mut seen = BTreeSet::<String>::new();
    let mut out = Vec::<String>::new();
    let mut push_model = |model: &str| {
        let model = model.trim();
        if !model.is_empty() && seen.insert(model.to_string()) {
            out.push(model.to_string());
        }
    };
    if let Some(current) = current {
        push_model(current);
    }
    let scoped = scoped_model_ids_for_patterns(scoped_patterns, provider);
    let ordered = if scoped.is_empty() {
        native_model_ids_for_provider(provider)
    } else {
        scoped
    };
    for model in ordered {
        push_model(&model);
    }
    out
}

fn main_model_refs_for_provider(
    session: &ShellSession,
    provider: &ProviderConfig,
) -> Vec<ModelRef> {
    // Match Pi selector behavior: current selection first, then either the
    // enabled/scoped order or the current provider catalog order. Do not use
    // historical known_model_ids here; those include stale prior selections and
    // role-only overrides, which is how old models leaked into the main selector.
    main_model_ids_for_selection(
        session.session_model(provider),
        &session.scoped_model_ids,
        provider,
    )
    .into_iter()
    .map(|model| model_ref_for_provider(provider, &model, None))
    .collect()
}

fn main_model_ids_for_provider(session: &ShellSession, provider: &ProviderConfig) -> Vec<String> {
    main_model_refs_for_provider(session, provider)
        .into_iter()
        .map(|model| model.id)
        .collect()
}

fn provider_api_key_env(config: &OpenAiCompatibleConfig) -> &str {
    if config.flavor == DirectProviderFlavor::OpenAiCodex {
        return "Pi/OPPi auth store (openai-codex)";
    }
    if config.flavor == DirectProviderFlavor::GitHubCopilot {
        return "Pi/OPPi auth store (github-copilot)";
    }
    config
        .api_key_env
        .as_deref()
        .unwrap_or("OPPI_OPENAI_API_KEY or OPENAI_API_KEY")
}

fn configured_api_key_env(config: &OpenAiCompatibleConfig) -> Option<String> {
    if let Some(name) = config.api_key_env.as_deref() {
        return env::var(name)
            .ok()
            .filter(|value| !value.trim().is_empty())
            .map(|_| name.to_string());
    }
    if env::var("OPPI_OPENAI_API_KEY")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .is_some()
    {
        return Some("OPPI_OPENAI_API_KEY".to_string());
    }
    if env::var("OPENAI_API_KEY")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .is_some()
    {
        return Some("OPENAI_API_KEY".to_string());
    }
    None
}

fn provider_auth_present(config: &OpenAiCompatibleConfig) -> bool {
    if config.flavor == DirectProviderFlavor::OpenAiCodex {
        return codex_auth_present_at(&auth_store_path());
    }
    if config.flavor == DirectProviderFlavor::GitHubCopilot {
        return github_copilot_auth_present_at(&github_copilot_auth_store_path());
    }
    configured_api_key_env(config).is_some() || provider_uses_meridian_placeholder(config)
}

fn provider_auth_status_label(config: &OpenAiCompatibleConfig) -> &'static str {
    if config.flavor == DirectProviderFlavor::OpenAiCodex {
        return if codex_auth_present_at(&auth_store_path()) {
            "stored oauth"
        } else {
            "missing"
        };
    }
    if config.flavor == DirectProviderFlavor::GitHubCopilot {
        return if github_copilot_auth_present_at(&github_copilot_auth_store_path()) {
            "stored oauth"
        } else {
            "missing"
        };
    }
    if configured_api_key_env(config).is_some() {
        "present"
    } else if provider_uses_meridian_placeholder(config) {
        "loopback placeholder"
    } else {
        "missing"
    }
}

fn default_authenticated_provider_config() -> Option<ProviderConfig> {
    let codex_present = codex_auth_present_at(&auth_store_path());
    let copilot_auth = read_github_copilot_auth_at(&github_copilot_auth_store_path()).ok();
    default_authenticated_provider_config_from_state(
        codex_present,
        copilot_auth.as_ref(),
        meridian_reachable(),
    )
}

fn default_authenticated_provider_config_from_state(
    codex_present: bool,
    copilot_auth: Option<&GitHubCopilotStoredAuth>,
    claude_bridge_reachable: bool,
) -> Option<ProviderConfig> {
    if codex_present {
        return Some(ProviderConfig::OpenAiCompatible(
            with_default_reasoning_effort(OpenAiCompatibleConfig {
                flavor: DirectProviderFlavor::OpenAiCodex,
                model: OPENAI_CODEX_DEFAULT_MODEL.to_string(),
                base_url: None,
                api_key_env: None,
                system_prompt: None,
                temperature: None,
                reasoning_effort: None,
                max_output_tokens: None,
                stream: true,
            }),
        ));
    }

    if let Some(auth) = copilot_auth {
        return Some(ProviderConfig::OpenAiCompatible(
            with_default_reasoning_effort(OpenAiCompatibleConfig {
                flavor: DirectProviderFlavor::GitHubCopilot,
                model: GITHUB_COPILOT_DEFAULT_MODEL.to_string(),
                base_url: Some(github_copilot_base_url(auth)),
                api_key_env: None,
                system_prompt: None,
                temperature: None,
                reasoning_effort: None,
                max_output_tokens: None,
                stream: true,
            }),
        ));
    }

    if claude_bridge_reachable {
        return Some(ProviderConfig::OpenAiCompatible(meridian_provider_config(
            None,
        )));
    }

    None
}

fn is_safe_api_key_env_name(name: &str) -> bool {
    if name.is_empty() || name.len() > 128 {
        return false;
    }
    if !name
        .chars()
        .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_')
    {
        return false;
    }
    name == "OPPI_OPENAI_API_KEY"
        || name == "OPENAI_API_KEY"
        || (name.starts_with("OPPI_") && name.ends_with("_API_KEY"))
        || (name.starts_with("OPENAI_") && name.ends_with("_API_KEY"))
        || (name.starts_with("AZURE_OPENAI_") && name.ends_with("_API_KEY"))
}

fn meridian_base_url() -> String {
    env::var("OPPI_MERIDIAN_BASE_URL")
        .ok()
        .or_else(|| env::var("MERIDIAN_BASE_URL").ok())
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| MERIDIAN_DEFAULT_BASE_URL.to_string())
}

fn meridian_package_spec() -> String {
    env::var("OPPI_MERIDIAN_PACKAGE_SPEC")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| format!("{MERIDIAN_PACKAGE_NAME}@latest"))
}

fn oppi_home_dir() -> PathBuf {
    env::var_os("OPPI_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".oppi")
        })
}

fn managed_packages_dir() -> PathBuf {
    oppi_home_dir().join("packages")
}

fn meridian_bin_name() -> String {
    format!("meridian{}", if cfg!(windows) { ".cmd" } else { "" })
}

fn managed_meridian_bin_path() -> PathBuf {
    managed_packages_dir()
        .join("node_modules")
        .join(".bin")
        .join(meridian_bin_name())
}

fn local_meridian_bin_path() -> PathBuf {
    env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("node_modules")
        .join(".bin")
        .join(meridian_bin_name())
}

fn managed_meridian_module_path() -> PathBuf {
    managed_packages_dir()
        .join("node_modules")
        .join("@rynfar")
        .join("meridian")
        .join("dist")
        .join("server.js")
}

fn format_path_for_display(path: &Path) -> String {
    let text = path.display().to_string();
    if let Some(home) = home_dir() {
        let home = home.display().to_string();
        if text.starts_with(&home) {
            return format!("~{}", &text[home.len()..]);
        }
    }
    text
}

fn ensure_managed_packages_root() -> Result<PathBuf, String> {
    let root = managed_packages_dir();
    fs::create_dir_all(&root)
        .map_err(|error| format!("create managed packages dir {}: {error}", root.display()))?;
    let package_json = root.join("package.json");
    if !package_json.exists() {
        fs::write(
            &package_json,
            "{\n  \"private\": true,\n  \"name\": \"oppi-managed-packages\",\n  \"description\": \"OPPi managed optional packages.\"\n}\n",
        )
        .map_err(|error| format!("write {}: {error}", package_json.display()))?;
    }
    Ok(root)
}

fn compact_process_output(output: &[u8]) -> String {
    let text = String::from_utf8_lossy(output).replace(['\r', '\n'], " ");
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.chars().count() > 800 {
        text.chars()
            .rev()
            .take(800)
            .collect::<String>()
            .chars()
            .rev()
            .collect()
    } else {
        text
    }
}

fn install_meridian_package() -> Result<String, String> {
    let root = ensure_managed_packages_root()?;
    let spec = meridian_package_spec();
    let output = Command::new(if cfg!(windows) { "npm.cmd" } else { "npm" })
        .args([
            "install",
            spec.as_str(),
            "--save-exact",
            "--no-audit",
            "--no-fund",
        ])
        .current_dir(&root)
        .output()
        .map_err(|error| format!("spawn npm install: {error}"))?;
    if output.status.success() && managed_meridian_bin_path().exists() {
        Ok(format!(
            "Installed Meridian bridge {} into {}. Next: run `claude login` if needed, choose a model with `/model`, then `/login subscription claude start`.",
            spec,
            format_path_for_display(&root)
        ))
    } else {
        let tail = compact_process_output(&[output.stdout, output.stderr].concat());
        Err(format!(
            "Meridian install failed with status {}{}{}",
            output.status,
            if tail.is_empty() { "" } else { ": " },
            tail
        ))
    }
}

fn base_url_host_port(base: &str) -> Option<(String, u16)> {
    let (scheme, rest) = base
        .split_once("://")
        .map(|(scheme, rest)| (scheme, rest))
        .unwrap_or(("http", base));
    let authority = rest.split('/').next()?.rsplit('@').next()?.trim();
    let (host, port) = if authority.starts_with('[') {
        let end = authority.find(']')?;
        let host = authority[1..end].to_string();
        let port = authority[end + 1..]
            .strip_prefix(':')
            .and_then(|raw| raw.parse().ok());
        (host, port)
    } else if let Some((host, port)) = authority.rsplit_once(':') {
        (host.to_string(), port.parse().ok())
    } else {
        (authority.to_string(), None)
    };
    let default_port = if scheme.eq_ignore_ascii_case("https") {
        443
    } else {
        80
    };
    Some((host, port.unwrap_or(default_port)))
}

fn meridian_host_port() -> Option<(String, u16)> {
    base_url_host_port(&meridian_base_url())
}

fn is_loopback_host(host: &str) -> bool {
    let host = host.trim_matches(['[', ']']).to_ascii_lowercase();
    host == "localhost" || host == "127.0.0.1" || host == "::1"
}

fn meridian_base_url_is_loopback() -> bool {
    meridian_host_port()
        .map(|(host, _)| is_loopback_host(&host))
        .unwrap_or(false)
}

fn tcp_reachable(host: &str, port: u16, timeout: Duration) -> bool {
    let Ok(addrs) = (host, port).to_socket_addrs() else {
        return false;
    };
    for addr in addrs {
        if TcpStream::connect_timeout(&addr, timeout).is_ok() {
            return true;
        }
    }
    false
}

fn meridian_reachable() -> bool {
    meridian_host_port()
        .map(|(host, port)| tcp_reachable(&host, port, Duration::from_millis(300)))
        .unwrap_or(false)
}

fn wait_for_meridian(timeout: Duration) -> bool {
    let started = Instant::now();
    while started.elapsed() < timeout {
        if meridian_reachable() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(300));
    }
    false
}

fn meridian_start_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(command) = env::var_os("OPPI_MERIDIAN_COMMAND")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
    {
        candidates.push(command);
    }
    let managed = managed_meridian_bin_path();
    if managed.exists() {
        candidates.push(managed);
    }
    let local = local_meridian_bin_path();
    if local.exists() {
        candidates.push(local);
    }
    candidates.push(PathBuf::from("meridian"));
    candidates
}

fn spawn_meridian_command(command: &Path) -> Result<Child, String> {
    let (host, port) = meridian_host_port().unwrap_or_else(|| ("127.0.0.1".to_string(), 3456));
    let mut cmd = Command::new(command);
    cmd.env(
        "MERIDIAN_DEFAULT_AGENT",
        env::var("MERIDIAN_DEFAULT_AGENT").unwrap_or_else(|_| "pi".to_string()),
    )
    .env(
        "MERIDIAN_PASSTHROUGH",
        env::var("MERIDIAN_PASSTHROUGH").unwrap_or_else(|_| "1".to_string()),
    )
    .env("MERIDIAN_HOST", host)
    .env("MERIDIAN_PORT", port.to_string())
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::null());
    cmd.spawn().map_err(|error| format!("spawn: {error}"))
}

fn claude_auth_status_summary() -> String {
    match Command::new("claude").args(["auth", "status"]).output() {
        Ok(output) if output.status.success() => {
            let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if let Ok(value) = serde_json::from_str::<Value>(&text) {
                let logged_in = value
                    .get("loggedIn")
                    .and_then(Value::as_bool)
                    .map(|value| if value { "yes" } else { "no" })
                    .unwrap_or("unknown");
                let subscription = value
                    .get("subscriptionType")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                format!("loggedIn={logged_in}; subscription={subscription}")
            } else if text.is_empty() {
                "status command succeeded".to_string()
            } else {
                compact_process_output(text.as_bytes())
            }
        }
        Ok(output) => {
            let tail = compact_process_output(&[output.stdout, output.stderr].concat());
            format!(
                "unavailable: claude auth status exited {}{}{}",
                output.status,
                if tail.is_empty() { "" } else { ": " },
                tail
            )
        }
        Err(error) => {
            format!("unavailable: {error}; run `claude login` after installing Claude Code")
        }
    }
}

fn meridian_status_panel() -> String {
    let reachable = meridian_reachable();
    format!(
        "Meridian Claude bridge\nbaseUrl: {}\nloopbackOnly: {}\nreachable: {}\nclaudeCodeAuth: {}\nmanagedBin: {} ({})\nlocalBin: {} ({})\nmanagedModule: {} ({})\npolicy: no hidden npx/proxy/credential-helper spawning; install requires explicit approval through /login subscription claude install --yes or the picker; Claude tokens remain owned by Claude Code/Meridian\nnext: `/login subscription claude login` if needed, choose a model with `/model`, then `/login subscription claude start` or `/login subscription claude use`.",
        redacted_base_url_label(&meridian_base_url()),
        meridian_base_url_is_loopback(),
        if reachable { "yes" } else { "no" },
        claude_auth_status_summary(),
        format_path_for_display(&managed_meridian_bin_path()),
        if managed_meridian_bin_path().exists() {
            "installed"
        } else {
            "missing"
        },
        format_path_for_display(&local_meridian_bin_path()),
        if local_meridian_bin_path().exists() {
            "available"
        } else {
            "missing"
        },
        format_path_for_display(&managed_meridian_module_path()),
        if managed_meridian_module_path().exists() {
            "present"
        } else {
            "missing"
        },
    )
}

fn meridian_provider_config(model: Option<&str>) -> OpenAiCompatibleConfig {
    with_default_reasoning_effort(OpenAiCompatibleConfig {
        flavor: DirectProviderFlavor::OpenAiCompatible,
        model: model
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(MERIDIAN_DEFAULT_MODEL)
            .to_string(),
        base_url: Some(meridian_base_url()),
        api_key_env: Some(MERIDIAN_API_KEY_ENV.to_string()),
        system_prompt: None,
        temperature: None,
        reasoning_effort: None,
        max_output_tokens: None,
        stream: true,
    })
}

fn provider_uses_meridian_placeholder(config: &OpenAiCompatibleConfig) -> bool {
    config.api_key_env.as_deref() == Some(MERIDIAN_API_KEY_ENV)
        && config
            .base_url
            .as_deref()
            .and_then(base_url_host_port)
            .is_some_and(|(host, _)| is_loopback_host(&host))
}

fn redacted_base_url_label(raw: &str) -> String {
    let value = raw.trim();
    if value.is_empty() {
        return "default OpenAI-compatible endpoint".to_string();
    }
    let (scheme, rest) = value
        .split_once("://")
        .map(|(scheme, rest)| (format!("{scheme}://"), rest))
        .unwrap_or_else(|| (String::new(), value));
    let host = rest
        .split('/')
        .next()
        .unwrap_or(rest)
        .rsplit('@')
        .next()
        .unwrap_or(rest);
    format!("{scheme}{host}")
}

fn provider_status_panel(
    provider: &ProviderConfig,
    models: &[ModelRef],
    session: &ShellSession,
) -> String {
    match provider {
        ProviderConfig::Mock => format!(
            "provider: mock scripted\nauth: not required\nliveCalls: none\nselectedModel: {}\nregisteredModels: {}\n{}",
            session.selected_model.as_deref().unwrap_or("mock-scripted"),
            models.len(),
            format_role_profiles(
                &session.role_models,
                session
                    .selected_model
                    .as_deref()
                    .or(current_provider_model(provider)),
            )
        ),
        ProviderConfig::OpenAiCompatible(config) => {
            let auth_env = provider_api_key_env(config);
            let auth_status = provider_auth_status_label(config);
            let provider_label = provider_name(provider);
            let base_default = match config.flavor {
                DirectProviderFlavor::OpenAiCompatible => {
                    "OPPI_OPENAI_BASE_URL or https://api.openai.com/v1"
                }
                DirectProviderFlavor::OpenAiCodex => {
                    "OPPI_OPENAI_CODEX_BASE_URL or https://chatgpt.com/backend-api"
                }
                DirectProviderFlavor::GitHubCopilot => "GitHub Copilot token proxy endpoint",
            };
            let login_hint = match config.flavor {
                DirectProviderFlavor::OpenAiCompatible => {
                    "/login api openai env <ENV>, /login subscription codex, /login subscription copilot, or /login subscription claude"
                }
                DirectProviderFlavor::OpenAiCodex => {
                    "/login subscription codex [--force] refreshes browser OAuth; model stays in /model"
                }
                DirectProviderFlavor::GitHubCopilot => {
                    "/login subscription copilot [--force] [--enterprise <domain>] uses Pi's GitHub device-code flow; model stays in /model"
                }
            };
            format!(
                "provider: {provider_label}\nmodel: {}\nselectedModel: {}\nbaseUrl: {}\nauth: {} ({})\nstream: {}{}{}{}\nregisteredModels: {}\nliveCalls: none for status/validate; /provider smoke is the explicit live call\ndiagnostics: secrets redacted; only env-reference names, auth-store labels, and endpoint hosts are shown\nlogin: {login_hint}\n{}",
                config.model,
                session
                    .selected_model
                    .as_deref()
                    .unwrap_or(config.model.as_str()),
                config
                    .base_url
                    .as_deref()
                    .map(redacted_base_url_label)
                    .unwrap_or_else(|| base_default.to_string()),
                auth_env,
                auth_status,
                config.stream,
                config
                    .reasoning_effort
                    .as_ref()
                    .map(|value| format!("\nreasoningEffort: {value}"))
                    .unwrap_or_default(),
                config
                    .max_output_tokens
                    .map(|value| format!("\nmaxOutputTokens: {value}"))
                    .unwrap_or_default(),
                config
                    .temperature
                    .map(|value| format!("\ntemperature: {value}"))
                    .unwrap_or_default(),
                models.len(),
                format_role_profiles(
                    &session.role_models,
                    session
                        .selected_model
                        .as_deref()
                        .or(current_provider_model(provider)),
                )
            )
        }
    }
}

fn provider_validation_panel(provider: &ProviderConfig) -> String {
    match provider {
        ProviderConfig::Mock => {
            "provider validation: mock provider ready; no credentials, network, or live calls used"
                .to_string()
        }
        ProviderConfig::OpenAiCompatible(config) => {
            let auth_env = provider_api_key_env(config);
            let auth_status = provider_auth_status_label(config);
            let provider_label = provider_name(provider);
            let base_default = match config.flavor {
                DirectProviderFlavor::OpenAiCompatible => {
                    "OPPI_OPENAI_BASE_URL or https://api.openai.com/v1"
                }
                DirectProviderFlavor::OpenAiCodex => {
                    "OPPI_OPENAI_CODEX_BASE_URL or https://chatgpt.com/backend-api"
                }
                DirectProviderFlavor::GitHubCopilot => "GitHub Copilot token proxy endpoint",
            };
            let auth_guidance = match config.flavor {
                DirectProviderFlavor::OpenAiCompatible => {
                    "API/env auth: use `/login api openai env <ENV>`, or use subscription routes for Codex/Copilot/Claude"
                }
                DirectProviderFlavor::OpenAiCodex => {
                    "Codex auth: use `/login subscription codex [--force]`; tokens stay in the protected auth store"
                }
                DirectProviderFlavor::GitHubCopilot => {
                    "Copilot auth: use `/login subscription copilot [--force] [--enterprise <domain>]`; tokens stay in the protected auth store"
                }
            };
            format!(
                "provider validation: {}\nprovider: {provider_label}\nmodel: {}\nbaseUrl: {}\nauth: {} ({})\nliveCalls: none during validation\n{auth_guidance}\nanthropic: use `/login subscription claude` for explicit managed Meridian setup; install asks approval and no hidden proxy spawning\nsmoke: run /provider smoke to make an explicit redacted live provider call",
                if provider_local_validation_ready(provider) {
                    "ready"
                } else {
                    "blocked"
                },
                config.model,
                config
                    .base_url
                    .as_deref()
                    .map(redacted_base_url_label)
                    .unwrap_or_else(|| base_default.to_string()),
                auth_env,
                auth_status
            )
        }
    }
}

fn provider_local_validation_ready(provider: &ProviderConfig) -> bool {
    match provider {
        ProviderConfig::Mock => true,
        ProviderConfig::OpenAiCompatible(config) => {
            !config.model.trim().is_empty() && provider_auth_present(config)
        }
    }
}

fn provider_for_role_config(
    provider: &ProviderConfig,
    role_models: &BTreeMap<String, String>,
    role: Option<&str>,
) -> ProviderConfig {
    provider_for_role_config_with_complexity(provider, role_models, role, false)
}

fn provider_for_role_config_with_complexity(
    provider: &ProviderConfig,
    role_models: &BTreeMap<String, String>,
    role: Option<&str>,
    promote_complex_subagent: bool,
) -> ProviderConfig {
    let Some(role) = role.and_then(normalize_role) else {
        return provider.clone();
    };
    let explicit_model = role_models.get(role).cloned();
    let model = explicit_model.clone().or_else(|| {
        default_model_for_role(provider, role, promote_complex_subagent).map(str::to_string)
    });
    let Some(model) = model else {
        return provider.clone();
    };
    match provider {
        ProviderConfig::Mock => ProviderConfig::Mock,
        ProviderConfig::OpenAiCompatible(config) => {
            let mut config = config.clone();
            config.model = model.clone();
            if explicit_model.is_none() {
                config.reasoning_effort =
                    default_effort_for_role_model(provider, role, &model, promote_complex_subagent);
            }
            ProviderConfig::OpenAiCompatible(config)
        }
    }
}

fn default_model_for_role(
    provider: &ProviderConfig,
    role: &str,
    promote_complex_subagent: bool,
) -> Option<&'static str> {
    let role = normalize_role(role)?;
    if role != "subagent" {
        return None;
    }
    let strong_role = promote_complex_subagent;
    match provider {
        ProviderConfig::Mock => None,
        ProviderConfig::OpenAiCompatible(config)
            if provider_uses_meridian_placeholder(config)
                || (config.flavor == DirectProviderFlavor::OpenAiCompatible
                    && is_anthropic_like_model(provider, &config.model)) =>
        {
            Some(if strong_role {
                CLAUDE_MAIN_DEFAULT_MODEL
            } else {
                CLAUDE_CODING_SUBAGENT_DEFAULT_MODEL
            })
        }
        ProviderConfig::OpenAiCompatible(config)
            if matches!(
                config.flavor,
                DirectProviderFlavor::OpenAiCodex | DirectProviderFlavor::GitHubCopilot
            ) || (config.flavor == DirectProviderFlavor::OpenAiCompatible
                && (config.base_url.is_none() || config.model.starts_with("gpt-"))) =>
        {
            Some(if strong_role {
                GPT_MAIN_DEFAULT_MODEL
            } else {
                GPT_CODING_SUBAGENT_DEFAULT_MODEL
            })
        }
        ProviderConfig::OpenAiCompatible(_) => None,
    }
}

fn default_effort_for_role_model(
    provider: &ProviderConfig,
    role: &str,
    model: &str,
    promote_complex_subagent: bool,
) -> Option<String> {
    let role = normalize_role(role)?;
    let role_provider = provider_with_model(provider, model)?;
    let level = if is_anthropic_like_model(&role_provider, model) {
        ThinkingLevel::High
    } else if role == "subagent" && !promote_complex_subagent {
        ThinkingLevel::High
    } else if model == GPT_MAIN_DEFAULT_MODEL {
        ThinkingLevel::XHigh
    } else {
        recommended_effort_level_for_provider(&role_provider)
    };
    if level == ThinkingLevel::Off
        || !allowed_effort_levels_for_model(&role_provider, model).contains(&level)
    {
        None
    } else {
        Some(level.as_str().to_string())
    }
}

fn provider_with_model(provider: &ProviderConfig, model: &str) -> Option<ProviderConfig> {
    match provider {
        ProviderConfig::Mock => None,
        ProviderConfig::OpenAiCompatible(config) => {
            let mut config = config.clone();
            config.model = model.to_string();
            Some(ProviderConfig::OpenAiCompatible(config))
        }
    }
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

fn provider_policy_text() -> &'static str {
    "provider policy: native shell owns mock, OpenAI-compatible direct API, ChatGPT/Codex OAuth, and GitHub Copilot device-code OAuth today, with /login as the primary setup UX. /login is a picker for Subscription vs API; model choice belongs to /model or future /settings. Status/validate are local and redacted; /provider smoke is the only provider command that makes a live model call. Codex and Copilot OAuth open the browser after explicit selection and store tokens only in the protected auth store. Anthropic/Claude subscription support uses an explicit user-selected managed Meridian bridge over the Claude Code SDK. OPPi must not spawn Meridian, npx, subscription proxies, external brokers, or credential helper processes implicitly; /login subscription claude install requires explicit user approval and /login subscription claude login is the explicit Claude Code auth step."
}

fn anthropic_provider_evaluation() -> &'static str {
    "anthropic evaluation: native /login exposes the subscription-auth path through an explicit managed Meridian bridge over the Claude Code SDK. Meridian runs on loopback with visible install/login/start/stop/use lifecycle, forces client-owned passthrough tools for OPPi, avoids implicit npx or hidden proxy spawning, and asks before managed install. Claude OAuth tokens stay in Claude Code/Meridian; OPPi does not extract or store them. Model selection stays in /model or future /settings."
}

fn normalize_role(raw: &str) -> Option<&'static str> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "planner" | "plan" | "planning" => Some("planner"),
        "thinking" | "thinker" | "think" => Some("thinking"),
        "reviewer" | "review" | "audit" => Some("reviewer"),
        "orchestrator" | "orchestration" | "orchestrate" => Some("orchestrator"),
        "executor" | "execute" | "coding" | "coder" => Some("executor"),
        "subagent" | "subagents" | "agent" | "agents" => Some("subagent"),
        _ => None,
    }
}

fn role_usage() -> &'static str {
    "usage: /model role <planner|thinking|reviewer|orchestrator|executor|subagent> <model-id|inherit>"
}

fn role_for_command(command: &str) -> &'static str {
    match command {
        "/review" | "/audit" => "reviewer",
        "/init" => "planner",
        "/independent" => "orchestrator",
        "/bug-report" | "/feature-request" => "planner",
        _ => "executor",
    }
}

fn format_suggestion_summary(suggestion: &SuggestedNextMessage) -> String {
    format!(
        "suggest-next: {}\nconfidence: {}%\nuse: Tab in suggestion dock, or /suggest-next clear",
        suggestion.message,
        (suggestion.confidence * 100.0).round() as u32
    )
}

fn format_suggestion_debug(suggestion: &SuggestedNextMessage) -> String {
    let reason = suggestion
        .reason
        .as_deref()
        .filter(|reason| !reason.trim().is_empty())
        .unwrap_or("not provided");
    format!(
        "suggest-next debug:\nmessage: {}\nconfidence: {:.2}\nreason: {}",
        suggestion.message, suggestion.confidence, reason
    )
}

fn format_usage_panel(session: &ShellSession, provider: &ProviderConfig) -> String {
    let model = session.session_model(provider).unwrap_or("none");
    let todos_total = session.todo_state.todos.len();
    let todos_active = session
        .todo_state
        .todos
        .iter()
        .filter(|todo| !matches!(todo.status.as_str(), "completed" | "cancelled"))
        .count();
    let suggestion = session
        .suggestion
        .as_ref()
        .map(|suggestion| format!("{}%", (suggestion.confidence * 100.0).round() as u32))
        .unwrap_or_else(|| "none".to_string());
    format!(
        "usage/status:\nprovider: {}\nmodel: {}\nthread: {}\npermission: {} ({})\ngoal: {}\ntodos: {}/{} active\nqueued follow-ups: {}\nsuggestion: {}\nnote: native usage is provider-neutral/local only; use stable Pi /usage for live subscription windows until native provider usage is designed.",
        provider_name(provider),
        model,
        session.thread_id,
        session.permission_mode.as_str(),
        session.permission_mode_source,
        session.goal_status_label(),
        todos_active,
        todos_total,
        session.follow_up_queue.len(),
        suggestion
    )
}

fn format_role_profiles(
    role_models: &BTreeMap<String, String>,
    session_model: Option<&str>,
) -> String {
    let inherited = session_model.unwrap_or("session model");
    let mut lines = vec!["roleModels:".to_string()];
    for role in ROLE_NAMES {
        let model = role_models
            .get(role)
            .map(String::as_str)
            .unwrap_or(inherited);
        let suffix = if role_models.contains_key(role) {
            ""
        } else {
            " (inherit)"
        };
        lines.push(format!("  {role}: {model}{suffix}"));
    }
    lines.join("\n")
}

fn filter_models(models: &[ModelRef], filter: &str) -> Vec<ModelRef> {
    let needle = filter.trim().to_ascii_lowercase();
    models
        .iter()
        .filter(|model| {
            needle.is_empty()
                || model.id.to_ascii_lowercase().contains(&needle)
                || model.provider.to_ascii_lowercase().contains(&needle)
                || model.display_name.to_ascii_lowercase().contains(&needle)
                || model
                    .role
                    .as_deref()
                    .unwrap_or_default()
                    .to_ascii_lowercase()
                    .contains(&needle)
        })
        .cloned()
        .collect()
}

fn format_model_list(
    models: &[ModelRef],
    selected: Option<&str>,
    filter: &str,
    role_models: &BTreeMap<String, String>,
) -> String {
    let filtered = filter_models(models, filter);
    let mut lines = vec![format!(
        "models: {}{}",
        filtered.len(),
        if filter.trim().is_empty() {
            String::new()
        } else {
            format!(" filtered by {:?}", filter.trim())
        }
    )];
    if filtered.is_empty() {
        lines.push("  none".to_string());
    } else {
        for model in filtered {
            let marker = if selected == Some(model.id.as_str()) {
                "*"
            } else {
                " "
            };
            let role = model
                .role
                .map(|role| format!(" role={role}"))
                .unwrap_or_default();
            lines.push(format!(
                "{marker} {} [{}]{}",
                model.id, model.provider, role
            ));
        }
    }
    lines.push(format_role_profiles(role_models, selected));
    lines.join("\n")
}

fn format_scoped_models(session: &ShellSession, provider: &ProviderConfig) -> String {
    let catalog = native_model_ids_for_provider(provider);
    let resolved = scoped_model_ids_for_provider(session, provider);
    let mut lines = vec!["scoped models:".to_string()];
    if session.scoped_model_ids.is_empty() {
        lines.push("  all current provider models (no scope configured)".to_string());
    } else {
        lines.push(format!(
            "  patterns: {}",
            session.scoped_model_ids.join(", ")
        ));
        if resolved.is_empty() {
            lines.push("  resolved: none for current provider".to_string());
        } else {
            lines.push(format!("  resolved: {}", resolved.join(", ")));
        }
    }
    lines.push(format!("  current provider: {}", provider_name(provider)));
    lines.push(format!("  available: {}", catalog.join(", ")));
    lines.push("usage: /scoped-models enable <model|provider/model|glob>, /scoped-models disable <pattern>, /scoped-models clear".to_string());
    lines.join("\n")
}

#[cfg(test)]
fn format_thread_tree(threads: &[Thread], active_thread_id: &str) -> String {
    format_thread_tree_with_folded(threads, active_thread_id, &BTreeSet::new())
}

fn format_thread_tree_with_folded(
    threads: &[Thread],
    active_thread_id: &str,
    folded_threads: &BTreeSet<String>,
) -> String {
    let mut children: BTreeMap<Option<String>, Vec<&Thread>> = BTreeMap::new();
    for thread in threads {
        children
            .entry(thread.forked_from.clone())
            .or_default()
            .push(thread);
    }
    for group in children.values_mut() {
        group.sort_by(|left, right| left.id.cmp(&right.id));
    }
    let mut lines = vec!["threads:".to_string()];
    let mut visited = BTreeMap::<String, bool>::new();
    fn mark_descendants(
        thread: &Thread,
        children: &BTreeMap<Option<String>, Vec<&Thread>>,
        visited: &mut BTreeMap<String, bool>,
    ) {
        if let Some(kids) = children.get(&Some(thread.id.clone())) {
            for child in kids {
                visited.insert(child.id.clone(), true);
                mark_descendants(child, children, visited);
            }
        }
    }
    fn render(
        thread: &Thread,
        children: &BTreeMap<Option<String>, Vec<&Thread>>,
        active_thread_id: &str,
        depth: usize,
        lines: &mut Vec<String>,
        visited: &mut BTreeMap<String, bool>,
        folded_threads: &BTreeSet<String>,
    ) {
        if visited.insert(thread.id.clone(), true).is_some() {
            return;
        }
        let marker = if thread.id == active_thread_id {
            "*"
        } else {
            " "
        };
        let title = thread.title.as_deref().unwrap_or("untitled");
        let fork = thread
            .forked_from
            .as_deref()
            .map(|parent| format!(" forkedFrom={parent}"))
            .unwrap_or_default();
        let archived = if thread.status == ThreadStatus::Archived {
            " [archived]"
        } else {
            ""
        };
        let child_count = children
            .get(&Some(thread.id.clone()))
            .map(Vec::len)
            .unwrap_or(0);
        let fold = if child_count > 0 && folded_threads.contains(&thread.id) {
            format!(" [+{child_count}]")
        } else if child_count > 0 {
            " [-]".to_string()
        } else {
            String::new()
        };
        lines.push(format!(
            "{}{} {}{} — {}{}{}",
            "  ".repeat(depth),
            marker,
            thread.id,
            fold,
            title,
            archived,
            fork
        ));
        if folded_threads.contains(&thread.id) {
            mark_descendants(thread, children, visited);
        } else if let Some(kids) = children.get(&Some(thread.id.clone())) {
            for child in kids {
                render(
                    child,
                    children,
                    active_thread_id,
                    depth + 1,
                    lines,
                    visited,
                    folded_threads,
                );
            }
        }
    }
    if let Some(roots) = children.get(&None) {
        for root in roots {
            render(
                root,
                &children,
                active_thread_id,
                0,
                &mut lines,
                &mut visited,
                folded_threads,
            );
        }
    }
    for thread in threads {
        if !visited.contains_key(&thread.id) {
            render(
                thread,
                &children,
                active_thread_id,
                0,
                &mut lines,
                &mut visited,
                folded_threads,
            );
        }
    }
    if threads.is_empty() {
        lines.push("  none".to_string());
    }
    lines.join("\n")
}

fn exit_resume_command(thread_id: &str) -> String {
    format!("oppi resume {thread_id}")
}

fn same_project_cwd(left: &str, right: &str) -> bool {
    let normalize = |value: &str| value.trim().replace('\\', "/");
    let left = normalize(left);
    let right = normalize(right);
    if cfg!(windows) {
        left.eq_ignore_ascii_case(&right)
    } else {
        left == right
    }
}

fn format_resume_session_list(
    threads: &[Thread],
    active_thread_id: &str,
    cwd: &str,
    folded_threads: &BTreeSet<String>,
) -> String {
    let project_threads: Vec<Thread> = threads
        .iter()
        .filter(|thread| same_project_cwd(&thread.project.cwd, cwd))
        .cloned()
        .collect();
    let tree = format_thread_tree_with_folded(&project_threads, active_thread_id, folded_threads)
        .replacen("threads:", "recent sessions in this project:", 1);
    let current_thread = if active_thread_id.is_empty() {
        "current thread: none".to_string()
    } else {
        format!("current thread: {active_thread_id}")
    };
    format!("{current_thread}\n{tree}\nnote: use /resume <thread-id> or oppi resume <thread-id>")
}

fn normalize_theme_name(raw: &str) -> Option<String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "oppi" | "dark" | "light" | "plain" => Some(raw.trim().to_ascii_lowercase()),
        _ => None,
    }
}

fn load_theme_file() -> Result<Option<String>, String> {
    let path = env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(".oppi")
        .join("theme.txt");
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&path)
        .map_err(|error| format!("read {}: {error}", path.display()))?;
    let theme = normalize_theme_name(raw.lines().next().unwrap_or_default()).ok_or_else(|| {
        format!(
            "{} must contain one of: oppi, dark, light, plain",
            path.display()
        )
    })?;
    Ok(Some(theme))
}

fn normalize_prompt_variant(raw: &str) -> Option<String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "off" | "none" => Some("off".to_string()),
        "a" | "promptname_a" | "prompt-name-a" => Some("a".to_string()),
        "b" | "promptname_b" | "prompt-name-b" => Some("b".to_string()),
        "caveman" => Some("caveman".to_string()),
        _ => None,
    }
}

fn prompt_variant_append(variant: &str) -> Option<String> {
    match variant {
        "a" => Some("Prompt variant A: be concise, autonomy-forward, and explicit about OPPi safety surfaces.".to_string()),
        "b" => Some("Prompt variant B: use a more deliberate planning cadence and preserve visible task state.".to_string()),
        "caveman" => Some("Caveman variant: short words, direct actions, no fluff; still follow OPPi safety rules.".to_string()),
        _ => None,
    }
}

fn prompt_variant_feature_routing_append(variant: &str) -> Option<&'static str> {
    match variant {
        "a" => Some(OPPI_FEATURE_ROUTING_VARIANT_A_APPEND),
        "b" | "caveman" => Some(OPPI_FEATURE_ROUTING_VARIANT_B_APPEND),
        _ => None,
    }
}

fn apply_feature_routing_to_provider(provider: &mut Value, variant: &str) {
    append_system_prompt_to_provider_once(
        provider,
        "OPPi feature routing",
        OPPI_FEATURE_ROUTING_SYSTEM_APPEND,
    );
    if let Some(append) = prompt_variant_feature_routing_append(variant) {
        append_system_prompt_to_provider_once(provider, "OPPi feature routing variant", append);
    }
}

fn apply_prompt_variant_to_provider(provider: &mut Value, variant: &str) {
    let Some(append) = prompt_variant_append(variant) else {
        return;
    };
    append_system_prompt_to_provider(provider, "OPPi prompt variant", append.as_str());
}

fn append_system_prompt_to_provider_once(provider: &mut Value, heading: &str, append: &str) {
    let existing = provider
        .get("systemPrompt")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if existing.contains(&format!("{heading}:")) {
        return;
    }
    append_system_prompt_to_provider(provider, heading, append);
}

fn append_system_prompt_to_provider(provider: &mut Value, heading: &str, append: &str) {
    let append = append.trim();
    if append.is_empty() {
        return;
    }
    let block = format!("{heading}:\n{append}");
    let system = provider
        .get("systemPrompt")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(|value| format!("{value}\n\n{block}"))
        .unwrap_or(block);
    provider["systemPrompt"] = json!(system);
}

fn openai_provider_json(config: &OpenAiCompatibleConfig) -> Value {
    let kind = match config.flavor {
        DirectProviderFlavor::OpenAiCompatible => "openai-compatible",
        DirectProviderFlavor::OpenAiCodex => "openai-codex",
        DirectProviderFlavor::GitHubCopilot => "github-copilot",
    };
    let mut value = json!({
        "kind": kind,
        "model": config.model,
        "stream": config.stream,
    });
    if let Some(base_url) = &config.base_url {
        value["baseUrl"] = json!(base_url);
    }
    if let Some(api_key_env) = &config.api_key_env {
        value["apiKeyEnv"] = json!(api_key_env);
    }
    if let Some(system_prompt) = &config.system_prompt {
        value["systemPrompt"] = json!(system_prompt);
    }
    if let Some(temperature) = config.temperature {
        value["temperature"] = json!(temperature);
    }
    if let Some(reasoning_effort) = &config.reasoning_effort {
        value["reasoningEffort"] = json!(reasoning_effort);
    }
    if let Some(max_output_tokens) = config.max_output_tokens {
        value["maxOutputTokens"] = json!(max_output_tokens);
    }
    value
}

fn parse_args(args: Vec<String>) -> Result<ShellCommand, String> {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_help();
        std::process::exit(0);
    }
    let mut prompt_parts = Vec::new();
    let mut server = None;
    let mut resume_thread = None;
    let mut list_sessions = false;
    let mut json = false;
    let mut mock = false;
    let mut provider_requested = false;
    let mut provider_flavor = DirectProviderFlavor::OpenAiCompatible;
    let mut interactive = false;
    let mut raw = default_retained_tui_enabled();
    let mut ratatui = default_ratatui_enabled();
    let mut model: Option<String> = None;
    let mut base_url = None;
    let mut api_key_env = None;
    let mut system_prompt = None;
    let mut temperature = None;
    let mut reasoning_effort = None;
    let mut max_output_tokens = None;
    let mut stream = true;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--json" => json = true,
            "--mock" => mock = true,
            "--interactive" | "-i" => interactive = true,
            "--raw" => {
                raw = true;
                interactive = true;
            }
            "--ratatui" => {
                ratatui = true;
                raw = true;
                interactive = true;
            }
            "--no-ratatui" => ratatui = false,
            "--line" | "--no-raw" | "--no-tui" => raw = false,
            "--no-stream" => {
                provider_requested = true;
                stream = false;
            }
            "--provider" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| "--provider requires mock or openai-compatible".to_string())?;
                match value.as_str() {
                    "mock" => mock = true,
                    "openai-compatible" | "openai" => provider_requested = true,
                    "openai-codex" | "codex" | "chatgpt" => {
                        provider_requested = true;
                        provider_flavor = DirectProviderFlavor::OpenAiCodex;
                    }
                    "github-copilot" | "copilot" | "github" => {
                        provider_requested = true;
                        provider_flavor = DirectProviderFlavor::GitHubCopilot;
                    }
                    other => return Err(format!("unsupported provider: {other}")),
                }
            }
            "--server" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| "--server requires a path".to_string())?;
                server = Some(PathBuf::from(value));
            }
            "--resume" => {
                index += 1;
                resume_thread = Some(required_option_value(&args, index, "--resume")?);
            }
            "--list-sessions" => list_sessions = true,
            "--model" => {
                index += 1;
                model = Some(required_option_value(&args, index, "--model")?);
                provider_requested = true;
            }
            "--base-url" => {
                index += 1;
                base_url = Some(required_option_value(&args, index, "--base-url")?);
                provider_requested = true;
            }
            "--api-key-env" => {
                index += 1;
                api_key_env = Some(required_option_value(&args, index, "--api-key-env")?);
                provider_requested = true;
            }
            "--system" => {
                index += 1;
                system_prompt = Some(required_option_value(&args, index, "--system")?);
                provider_requested = true;
            }
            "--temperature" => {
                index += 1;
                let raw = required_option_value(&args, index, "--temperature")?;
                temperature = Some(
                    raw.parse::<f32>()
                        .map_err(|error| format!("invalid --temperature: {error}"))?,
                );
                provider_requested = true;
            }
            "--effort" | "--reasoning-effort" => {
                index += 1;
                reasoning_effort = Some(required_option_value(
                    &args,
                    index,
                    args[index - 1].as_str(),
                )?);
                provider_requested = true;
            }
            "--max-output-tokens" => {
                index += 1;
                let raw = required_option_value(&args, index, "--max-output-tokens")?;
                max_output_tokens = Some(
                    raw.parse::<u32>()
                        .map_err(|error| format!("invalid --max-output-tokens: {error}"))?,
                );
                provider_requested = true;
            }
            arg if arg.starts_with("--server=") => {
                server = Some(PathBuf::from(arg.trim_start_matches("--server=")));
            }
            arg if arg.starts_with("--resume=") => {
                resume_thread = Some(arg.trim_start_matches("--resume=").to_string());
            }
            arg if arg == "--sessions" => list_sessions = true,
            arg if arg.starts_with("--model=") => {
                model = Some(arg.trim_start_matches("--model=").to_string());
                provider_requested = true;
            }
            arg if arg.starts_with("--base-url=") => {
                base_url = Some(arg.trim_start_matches("--base-url=").to_string());
                provider_requested = true;
            }
            arg if arg.starts_with("--api-key-env=") => {
                api_key_env = Some(arg.trim_start_matches("--api-key-env=").to_string());
                provider_requested = true;
            }
            arg if arg.starts_with("--system=") => {
                system_prompt = Some(arg.trim_start_matches("--system=").to_string());
                provider_requested = true;
            }
            arg if arg.starts_with("--effort=") => {
                reasoning_effort = Some(arg.trim_start_matches("--effort=").to_string());
                provider_requested = true;
            }
            arg if arg.starts_with("--reasoning-effort=") => {
                reasoning_effort = Some(arg.trim_start_matches("--reasoning-effort=").to_string());
                provider_requested = true;
            }
            arg if arg.starts_with("--max-output-tokens=") => {
                let raw = arg.trim_start_matches("--max-output-tokens=");
                max_output_tokens = Some(
                    raw.parse::<u32>()
                        .map_err(|error| format!("invalid --max-output-tokens: {error}"))?,
                );
                provider_requested = true;
            }
            arg if arg.starts_with("--temperature=") => {
                let raw = arg.trim_start_matches("--temperature=");
                temperature = Some(
                    raw.parse::<f32>()
                        .map_err(|error| format!("invalid --temperature: {error}"))?,
                );
                provider_requested = true;
            }
            arg if arg.starts_with('-') => return Err(format!("unknown option: {arg}")),
            value => prompt_parts.push(value.to_string()),
        }
        index += 1;
    }

    if mock && provider_requested && !list_sessions {
        return Err("choose either --mock or an OpenAI-compatible provider, not both".to_string());
    }

    let can_use_authenticated_default = !provider_requested
        && model.is_none()
        && base_url.is_none()
        && api_key_env.is_none()
        && system_prompt.is_none()
        && temperature.is_none()
        && reasoning_effort.is_none()
        && max_output_tokens.is_none();
    let authenticated_default = can_use_authenticated_default
        .then(default_authenticated_provider_config)
        .flatten();

    let provider = if list_sessions || mock {
        ProviderConfig::Mock
    } else if let Some(provider) = authenticated_default {
        provider
    } else {
        let model = model
            .or_else(default_model_from_env)
            .or_else(|| match provider_flavor {
                DirectProviderFlavor::OpenAiCodex => Some(OPENAI_CODEX_DEFAULT_MODEL.to_string()),
                DirectProviderFlavor::GitHubCopilot => {
                    Some(GITHUB_COPILOT_DEFAULT_MODEL.to_string())
                }
                DirectProviderFlavor::OpenAiCompatible => None,
            });
        if let Some(model) = model {
            ProviderConfig::OpenAiCompatible(with_default_reasoning_effort(
                OpenAiCompatibleConfig {
                    flavor: provider_flavor,
                    model,
                    base_url,
                    api_key_env: if matches!(
                        provider_flavor,
                        DirectProviderFlavor::OpenAiCodex | DirectProviderFlavor::GitHubCopilot
                    ) {
                        None
                    } else {
                        api_key_env
                    },
                    system_prompt,
                    temperature,
                    reasoning_effort,
                    max_output_tokens,
                    stream,
                },
            ))
        } else {
            return Err(
                "choose --mock for the scripted shell or pass --model/OPPI_RUNTIME_WORKER_MODEL for a real OpenAI-compatible provider"
                    .to_string(),
            );
        }
    };

    let prompt = prompt_parts.join(" ").trim().to_string();
    let initial_prompt = if prompt.is_empty() {
        None
    } else {
        Some(prompt)
    };
    if list_sessions {
        interactive = false;
    } else {
        interactive |= initial_prompt.is_none();
    }
    Ok(ShellCommand {
        initial_prompt,
        server,
        resume_thread,
        list_sessions,
        json,
        interactive,
        raw,
        ratatui,
        provider,
    })
}

fn required_option_value(args: &[String], index: usize, option: &str) -> Result<String, String> {
    args.get(index)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("{option} requires a value"))
}

fn default_retained_tui_enabled() -> bool {
    if let Ok(value) = env::var("OPPI_SHELL_RAW").or_else(|_| env::var("OPPI_SHELL_TUI")) {
        return matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        );
    }
    if env::var("OPPI_SHELL_LINE").ok().is_some_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    }) {
        return false;
    }
    io::stdin().is_terminal() && io::stdout().is_terminal()
}

fn default_ratatui_enabled() -> bool {
    if let Ok(value) = env::var("OPPI_SHELL_RATATUI") {
        return !matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "0" | "false" | "no" | "off"
        );
    }
    true
}

fn default_model_from_env() -> Option<String> {
    [
        "OPPI_RUNTIME_WORKER_MODEL",
        "OPPI_OPENAI_MODEL",
        "OPENAI_MODEL",
    ]
    .iter()
    .find_map(|name| {
        env::var(name)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    })
}

fn print_help() {
    println!(
        "oppi-shell --mock [--interactive] [--raw|--no-tui] [--ratatui] [--json] [--server <path>] [--resume <thread-id>] [prompt]\n\
         oppi-shell --mock --list-sessions [--json] [--server <path>]\n\
         oppi-shell --model <model> [--provider openai-compatible|openai-codex] [--base-url <url>] [--api-key-env <ENV>] [--resume <thread-id>] [--interactive] [--raw|--no-tui] [--ratatui] [prompt]\n\n\
         Experimental native shell for oppi-server --stdio. In an interactive terminal it opens the Ratatui Rust UI by default: structured layout bands, semantic transcript rows, dock trays, editor, slash palette, settings overlay, and adaptive narrow/tiny breakpoints. Use --no-ratatui for the retained ANSI fallback and --no-tui/--line for plain line mode. Provider/model commands include /login, /provider, /models, /model, /roles, and explicit /provider smoke. Real provider mode uses OpenAI-compatible API env refs or native ChatGPT/Codex OAuth; /login anthropic uses an explicit managed Meridian loopback bridge."
    );
}

fn default_runtime_store_dir() -> PathBuf {
    env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(".oppi")
        .join("runtime-store")
}

fn home_dir() -> Option<PathBuf> {
    env::var_os("USERPROFILE")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("HOME")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        })
}

fn default_agent_dir() -> PathBuf {
    env::var_os("OPPI_AGENT_DIR")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("PI_CODING_AGENT_DIR")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        })
        .unwrap_or_else(|| {
            home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".oppi")
                .join("agent")
        })
}

fn auth_store_path() -> PathBuf {
    env::var_os("OPPI_OPENAI_CODEX_AUTH_PATH")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| default_agent_dir().join("auth.json"))
}

fn auth_lock_path(path: &Path) -> PathBuf {
    let mut lock_path = path.as_os_str().to_os_string();
    lock_path.push(".lock");
    PathBuf::from(lock_path)
}

#[derive(Debug)]
struct CodexAuthLock {
    path: PathBuf,
}

impl CodexAuthLock {
    fn acquire(auth_path: &Path) -> Result<Self, String> {
        if let Some(parent) = auth_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("create auth dir {}: {error}", parent.display()))?;
        }
        let lock_path = auth_lock_path(auth_path);
        let mut delay = Duration::from_millis(100);
        for attempt in 0..10 {
            match fs::create_dir(&lock_path) {
                Ok(()) => return Ok(Self { path: lock_path }),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    if auth_lock_is_stale(&lock_path) {
                        let _ = remove_auth_lock_path(&lock_path);
                        continue;
                    }
                    if attempt == 9 {
                        return Err(format!(
                            "could not acquire Pi-compatible auth lock {}; another OPPi/Pi process may be refreshing credentials",
                            lock_path.display()
                        ));
                    }
                    std::thread::sleep(delay);
                    delay = (delay * 2).min(Duration::from_secs(2));
                }
                Err(error) => {
                    return Err(format!("create auth lock {}: {error}", lock_path.display()));
                }
            }
        }
        Err(format!(
            "could not acquire auth lock {}",
            lock_path.display()
        ))
    }
}

impl Drop for CodexAuthLock {
    fn drop(&mut self) {
        let _ = remove_auth_lock_path(&self.path);
    }
}

fn auth_lock_is_stale(path: &Path) -> bool {
    fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| modified.elapsed().ok())
        .is_some_and(|elapsed| elapsed > Duration::from_secs(30))
}

fn remove_auth_lock_path(path: &Path) -> std::io::Result<()> {
    fs::remove_dir(path).or_else(|dir_error| {
        if path.is_file() {
            fs::remove_file(path)
        } else {
            Err(dir_error)
        }
    })
}

fn role_profile_settings_path() -> PathBuf {
    env::var_os("OPPI_ROLE_PROFILES_PATH")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| default_agent_dir().join("settings.json"))
}

fn read_json_or_empty(path: &Path) -> Result<Value, String> {
    match fs::read_to_string(path) {
        Ok(raw) => serde_json::from_str(&raw)
            .map_err(|error| format!("read {}: invalid JSON: {error}", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(json!({})),
        Err(error) => Err(format!("read {}: {error}", path.display())),
    }
}

fn write_private_json(path: &Path, data: &Value) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("create auth dir {}: {error}", parent.display()))?;
    }
    let rendered = serde_json::to_string_pretty(data)
        .map_err(|error| format!("serialize {}: {error}", path.display()))?;
    fs::write(path, format!("{rendered}\n"))
        .map_err(|error| format!("write {}: {error}", path.display()))?;
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

fn form_url_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut output = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                output.push(b' ');
                index += 1;
            }
            b'%' if index + 2 < bytes.len() => {
                let hex = &input[index + 1..index + 3];
                if let Ok(value) = u8::from_str_radix(hex, 16) {
                    output.push(value);
                    index += 3;
                } else {
                    output.push(bytes[index]);
                    index += 1;
                }
            }
            byte => {
                output.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&output).to_string()
}

fn query_param(path: &str, key: &str) -> Option<String> {
    let query = path
        .split_once('?')?
        .1
        .split('#')
        .next()
        .unwrap_or_default();
    query.split('&').find_map(|pair| {
        let (raw_key, raw_value) = pair.split_once('=')?;
        (form_url_decode(raw_key) == key).then(|| form_url_decode(raw_value))
    })
}

fn codex_token_form(params: &[(&str, &str)]) -> String {
    params
        .iter()
        .map(|(key, value)| format!("{}={}", form_url_encode(key), form_url_encode(value)))
        .collect::<Vec<_>>()
        .join("&")
}

fn codex_auth_present_at(path: &Path) -> bool {
    read_json_or_empty(path).ok().is_some_and(|data| {
        let Some(credential) = data.get(OPENAI_CODEX_PROVIDER_ID) else {
            return false;
        };
        credential.get("type").and_then(Value::as_str) == Some("oauth")
            && credential
                .get("access")
                .and_then(Value::as_str)
                .is_some_and(|value| !value.trim().is_empty())
            && credential
                .get("refresh")
                .and_then(Value::as_str)
                .is_some_and(|value| !value.trim().is_empty())
            && credential
                .get("accountId")
                .and_then(Value::as_str)
                .is_some_and(|value| !value.trim().is_empty())
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GitHubCopilotStoredAuth {
    access_token: String,
    refresh_token: String,
    expires: i64,
    enterprise_domain: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GitHubCopilotDeviceFlow {
    device_code: String,
    user_code: String,
    verification_uri: String,
    interval_seconds: u64,
    expires_in_seconds: u64,
}

fn github_copilot_auth_store_path() -> PathBuf {
    env::var_os("OPPI_GITHUB_COPILOT_AUTH_PATH")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(auth_store_path)
}

fn read_github_copilot_auth_at(path: &Path) -> Result<GitHubCopilotStoredAuth, String> {
    let data = read_json_or_empty(path)?;
    let credential = data.get(GITHUB_COPILOT_PROVIDER_ID).ok_or_else(|| {
        format!(
            "GitHub Copilot auth missing in {}; run `/login subscription copilot`",
            path.display()
        )
    })?;
    if credential.get("type").and_then(Value::as_str) != Some("oauth") {
        return Err(format!(
            "GitHub Copilot credential in {} is not an OAuth credential",
            path.display()
        ));
    }
    Ok(GitHubCopilotStoredAuth {
        access_token: credential
            .get("access")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| "GitHub Copilot credential is missing access token".to_string())?
            .to_string(),
        refresh_token: credential
            .get("refresh")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| "GitHub Copilot credential is missing refresh token".to_string())?
            .to_string(),
        expires: credential
            .get("expires")
            .and_then(Value::as_i64)
            .ok_or_else(|| "GitHub Copilot credential is missing expiry".to_string())?,
        enterprise_domain: credential
            .get("enterpriseUrl")
            .or_else(|| credential.get("enterpriseDomain"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
    })
}

fn github_copilot_auth_present_at(path: &Path) -> bool {
    read_github_copilot_auth_at(path).is_ok()
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

fn github_copilot_base_url(auth: &GitHubCopilotStoredAuth) -> String {
    github_copilot_base_url_from_token(&auth.access_token)
        .or_else(|| {
            auth.enterprise_domain
                .as_ref()
                .map(|domain| format!("https://copilot-api.{domain}"))
        })
        .unwrap_or_else(|| GITHUB_COPILOT_DEFAULT_BASE_URL.to_string())
}

fn normalize_github_enterprise_domain(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }
    let without_scheme = trimmed
        .strip_prefix("https://")
        .or_else(|| trimmed.strip_prefix("http://"))
        .unwrap_or(trimmed);
    let host = without_scheme
        .split('/')
        .next()
        .unwrap_or(without_scheme)
        .trim()
        .trim_matches('.');
    if host.is_empty()
        || !host
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '.')
    {
        return None;
    }
    Some(host.to_ascii_lowercase())
}

fn github_copilot_enterprise_domain_from_flags(flags: &[&str]) -> Result<Option<String>, String> {
    let mut index = 0;
    while index < flags.len() {
        let flag = flags[index].trim();
        if let Some(value) = flag
            .strip_prefix("--enterprise=")
            .or_else(|| flag.strip_prefix("--domain="))
        {
            return normalize_github_enterprise_domain(value)
                .map(Some)
                .ok_or_else(|| "invalid GitHub Enterprise domain".to_string());
        }
        if matches!(flag, "--enterprise" | "enterprise" | "--domain" | "domain") {
            let Some(value) = flags.get(index + 1) else {
                return Err("GitHub Copilot enterprise login requires a domain".to_string());
            };
            return normalize_github_enterprise_domain(value)
                .map(Some)
                .ok_or_else(|| "invalid GitHub Enterprise domain".to_string());
        }
        index += 1;
    }
    Ok(None)
}

fn github_copilot_urls(domain: &str) -> (String, String, String) {
    (
        format!("https://{domain}/login/device/code"),
        format!("https://{domain}/login/oauth/access_token"),
        format!("https://api.{domain}/copilot_internal/v2/token"),
    )
}

fn start_github_copilot_device_flow(domain: &str) -> Result<GitHubCopilotDeviceFlow, String> {
    let (device_url, _, _) = github_copilot_urls(domain);
    let body = codex_token_form(&[
        ("client_id", GITHUB_COPILOT_CLIENT_ID),
        ("scope", "read:user"),
    ]);
    let value = ureq::post(&device_url)
        .set("accept", "application/json")
        .set("content-type", "application/x-www-form-urlencoded")
        .set("User-Agent", "GitHubCopilotChat/0.35.0")
        .send_string(&body)
        .map_err(|error| format!("GitHub Copilot device-code request failed: {error}"))?
        .into_json::<Value>()
        .map_err(|error| {
            format!("GitHub Copilot device-code response was invalid JSON: {error}")
        })?;
    Ok(GitHubCopilotDeviceFlow {
        device_code: value
            .get("device_code")
            .and_then(Value::as_str)
            .ok_or_else(|| "GitHub Copilot device-code response missing device_code".to_string())?
            .to_string(),
        user_code: value
            .get("user_code")
            .and_then(Value::as_str)
            .ok_or_else(|| "GitHub Copilot device-code response missing user_code".to_string())?
            .to_string(),
        verification_uri: value
            .get("verification_uri")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                "GitHub Copilot device-code response missing verification_uri".to_string()
            })?
            .to_string(),
        interval_seconds: value
            .get("interval")
            .and_then(Value::as_u64)
            .filter(|value| *value > 0)
            .ok_or_else(|| "GitHub Copilot device-code response missing interval".to_string())?,
        expires_in_seconds: value
            .get("expires_in")
            .and_then(Value::as_u64)
            .filter(|value| *value > 0)
            .ok_or_else(|| "GitHub Copilot device-code response missing expires_in".to_string())?,
    })
}

fn poll_github_copilot_device_flow(
    domain: &str,
    device: &GitHubCopilotDeviceFlow,
) -> Result<String, String> {
    let (_, token_url, _) = github_copilot_urls(domain);
    let deadline = Instant::now() + Duration::from_secs(device.expires_in_seconds);
    let mut interval = Duration::from_secs(device.interval_seconds).max(Duration::from_secs(1));
    let mut multiplier = 1.2f64;
    let mut slow_downs = 0u32;
    while Instant::now() < deadline {
        let wait = interval.mul_f64(multiplier).min(Duration::from_secs(30));
        std::thread::sleep(wait);
        let body = codex_token_form(&[
            ("client_id", GITHUB_COPILOT_CLIENT_ID),
            ("device_code", &device.device_code),
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
        ]);
        let value = match ureq::post(&token_url)
            .set("accept", "application/json")
            .set("content-type", "application/x-www-form-urlencoded")
            .set("User-Agent", "GitHubCopilotChat/0.35.0")
            .send_string(&body)
        {
            Ok(response) => response.into_json::<Value>().map_err(|error| {
                format!("GitHub Copilot device-token response was invalid JSON: {error}")
            })?,
            Err(ureq::Error::Status(_, response)) => {
                response.into_json::<Value>().unwrap_or_else(|_| json!({}))
            }
            Err(error) => {
                return Err(format!(
                    "GitHub Copilot device-token request failed: {error}"
                ));
            }
        };
        if let Some(access) = value
            .get("access_token")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
        {
            return Ok(access.to_string());
        }
        match value.get("error").and_then(Value::as_str) {
            Some("authorization_pending") => continue,
            Some("slow_down") => {
                slow_downs += 1;
                interval = value
                    .get("interval")
                    .and_then(Value::as_u64)
                    .filter(|value| *value > 0)
                    .map(Duration::from_secs)
                    .unwrap_or_else(|| interval + Duration::from_secs(5));
                multiplier = 1.4;
            }
            Some(error) => {
                let description = value
                    .get("error_description")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                return Err(format!(
                    "GitHub Copilot device flow failed: {error}{}{}",
                    if description.is_empty() { "" } else { ": " },
                    description
                ));
            }
            None => {
                return Err("GitHub Copilot device-token response missing access_token".to_string());
            }
        }
    }
    if slow_downs > 0 {
        Err("GitHub Copilot device flow timed out after slow_down responses; check local clock drift and try again.".to_string())
    } else {
        Err("GitHub Copilot device flow timed out".to_string())
    }
}

fn refresh_github_copilot_token(
    github_access_token: &str,
    enterprise_domain: Option<&str>,
) -> Result<Value, String> {
    let domain = enterprise_domain.unwrap_or(GITHUB_COPILOT_DEFAULT_DOMAIN);
    let (_, _, token_url) = github_copilot_urls(domain);
    ureq::get(&token_url)
        .set("accept", "application/json")
        .set("authorization", &format!("Bearer {github_access_token}"))
        .set("User-Agent", "GitHubCopilotChat/0.35.0")
        .set("Editor-Version", "vscode/1.107.0")
        .set("Editor-Plugin-Version", "copilot-chat/0.35.0")
        .set("Copilot-Integration-Id", "vscode-chat")
        .call()
        .map_err(|error| format!("GitHub Copilot token refresh failed: {error}"))?
        .into_json::<Value>()
        .map_err(|error| format!("GitHub Copilot token refresh returned invalid JSON: {error}"))
}

fn persist_github_copilot_oauth(
    path: &Path,
    github_access_token: &str,
    enterprise_domain: Option<&str>,
    token: Value,
) -> Result<(), String> {
    let access = token
        .get("token")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "GitHub Copilot token refresh did not return token".to_string())?;
    let expires_at = token
        .get("expires_at")
        .and_then(Value::as_i64)
        .filter(|value| *value > 0)
        .ok_or_else(|| "GitHub Copilot token refresh did not return expires_at".to_string())?;
    let expires = expires_at
        .saturating_mul(1000)
        .saturating_sub(5 * 60 * 1000);
    let _lock = CodexAuthLock::acquire(path)?;
    let mut data = read_json_or_empty(path)?;
    if !data.is_object() {
        data = json!({});
    }
    let mut credential = json!({
        "type": "oauth",
        "access": access,
        "refresh": github_access_token,
        "expires": expires,
    });
    if let Some(domain) = enterprise_domain {
        credential["enterpriseUrl"] = json!(domain);
    }
    data[GITHUB_COPILOT_PROVIDER_ID] = credential;
    write_private_json(path, &data)
}

fn oauth_random_bytes<const N: usize>() -> Result<[u8; N], String> {
    let mut bytes = [0u8; N];
    getrandom::getrandom(&mut bytes)
        .map_err(|error| format!("generate OAuth random bytes: {error}"))?;
    Ok(bytes)
}

fn create_codex_oauth_flow() -> Result<(String, String, String), String> {
    let verifier = URL_SAFE_NO_PAD.encode(oauth_random_bytes::<32>()?);
    let digest = ring::digest::digest(&ring::digest::SHA256, verifier.as_bytes());
    let challenge = URL_SAFE_NO_PAD.encode(digest.as_ref());
    let state = oauth_random_bytes::<16>()?
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let params = [
        ("response_type", "code"),
        ("client_id", OPENAI_CODEX_CLIENT_ID),
        ("redirect_uri", OPENAI_CODEX_REDIRECT_URI),
        ("scope", OPENAI_CODEX_SCOPE),
        ("code_challenge", challenge.as_str()),
        ("code_challenge_method", "S256"),
        ("state", state.as_str()),
        ("id_token_add_organizations", "true"),
        ("codex_cli_simplified_flow", "true"),
        ("originator", "pi"),
    ];
    let query = params
        .iter()
        .map(|(key, value)| format!("{}={}", form_url_encode(key), form_url_encode(value)))
        .collect::<Vec<_>>()
        .join("&");
    Ok((
        verifier,
        state,
        format!("{OPENAI_CODEX_AUTHORIZE_URL}?{query}"),
    ))
}

fn open_browser(url: &str) -> Result<(), String> {
    let mut command = if cfg!(windows) {
        let mut command = Command::new("cmd");
        command.args(["/C", "start", "", url]);
        command
    } else if cfg!(target_os = "macos") {
        let mut command = Command::new("open");
        command.arg(url);
        command
    } else {
        let mut command = Command::new("xdg-open");
        command.arg(url);
        command
    };
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("open browser for Codex login: {error}"))
}

fn oauth_response_html(message: &str) -> String {
    format!(
        "<!doctype html><html><body><h1>OPPi Codex login</h1><p>{}</p></body></html>",
        message
    )
}

fn read_http_request_path(stream: &mut TcpStream) -> Result<String, String> {
    let mut reader = BufReader::new(stream.try_clone().map_err(|error| error.to_string())?);
    let mut request_line = String::new();
    reader
        .read_line(&mut request_line)
        .map_err(|error| format!("read OAuth callback: {error}"))?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let path = parts.next().unwrap_or_default();
    if method != "GET" || path.is_empty() {
        return Err("OAuth callback did not send a GET request".to_string());
    }
    Ok(path.to_string())
}

fn write_oauth_http_response(stream: &mut TcpStream, status: &str, message: &str) {
    let body = oauth_response_html(message);
    let response = format!(
        "HTTP/1.1 {status}\r\ncontent-type: text/html; charset=utf-8\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

struct CodexOAuthListener {
    listener: TcpListener,
    host: String,
}

impl CodexOAuthListener {
    fn bind() -> Result<Self, String> {
        let host = env::var("PI_OAUTH_CALLBACK_HOST")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "127.0.0.1".to_string());
        let listener = TcpListener::bind((host.as_str(), 1455))
            .map_err(|error| format!("bind OAuth callback at {host}:1455: {error}"))?;
        listener
            .set_nonblocking(true)
            .map_err(|error| format!("configure OAuth callback listener: {error}"))?;
        Ok(Self { listener, host })
    }

    fn wait_for_code(&self, state: &str, timeout: Duration) -> Result<String, String> {
        let started = Instant::now();
        while started.elapsed() < timeout {
            match self.listener.accept() {
                Ok((mut stream, _addr)) => {
                    let path = match read_http_request_path(&mut stream) {
                        Ok(path) => path,
                        Err(error) => {
                            write_oauth_http_response(&mut stream, "400 Bad Request", &error);
                            return Err(error);
                        }
                    };
                    if !path.starts_with("/auth/callback") {
                        write_oauth_http_response(
                            &mut stream,
                            "404 Not Found",
                            "Callback route not found.",
                        );
                        continue;
                    }
                    let Some(code) = query_param(&path, "code") else {
                        write_oauth_http_response(
                            &mut stream,
                            "400 Bad Request",
                            "Missing authorization code.",
                        );
                        return Err("OAuth callback was missing the authorization code".to_string());
                    };
                    if query_param(&path, "state").as_deref() != Some(state) {
                        write_oauth_http_response(
                            &mut stream,
                            "400 Bad Request",
                            "State mismatch.",
                        );
                        return Err("OAuth callback state mismatch".to_string());
                    }
                    write_oauth_http_response(
                        &mut stream,
                        "200 OK",
                        "OpenAI authentication completed. You can close this window.",
                    );
                    return Ok(code);
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(100));
                }
                Err(error) => return Err(format!("accept OAuth callback: {error}")),
            }
        }
        Err(format!(
            "Timed out waiting for the browser OAuth callback from ChatGPT/Codex at http://{}:1455/auth/callback.",
            self.host
        ))
    }
}

fn decode_jwt_payload(token: &str) -> Result<Value, String> {
    let payload = token
        .split('.')
        .nth(1)
        .ok_or_else(|| "OAuth access token is not a JWT".to_string())?;
    let decoded = URL_SAFE_NO_PAD
        .decode(payload.as_bytes())
        .map_err(|error| format!("decode OAuth access token payload: {error}"))?;
    serde_json::from_slice(&decoded)
        .map_err(|error| format!("parse OAuth access token payload: {error}"))
}

fn codex_account_id(access_token: &str) -> Result<String, String> {
    decode_jwt_payload(access_token)?
        .get("https://api.openai.com/auth")
        .and_then(|auth| auth.get("chatgpt_account_id"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "OAuth access token did not include a ChatGPT account id".to_string())
}

fn exchange_codex_oauth_code(code: &str, verifier: &str) -> Result<Value, String> {
    let body = [
        ("grant_type", "authorization_code"),
        ("client_id", OPENAI_CODEX_CLIENT_ID),
        ("code", code),
        ("code_verifier", verifier),
        ("redirect_uri", OPENAI_CODEX_REDIRECT_URI),
    ]
    .iter()
    .map(|(key, value)| format!("{}={}", form_url_encode(key), form_url_encode(value)))
    .collect::<Vec<_>>()
    .join("&");
    let response = ureq::post(OPENAI_CODEX_TOKEN_URL)
        .set("content-type", "application/x-www-form-urlencoded")
        .send_string(&body)
        .map_err(|error| match error {
            ureq::Error::Status(status, _) => {
                format!("Codex OAuth token exchange failed with HTTP {status}")
            }
            other => format!("Codex OAuth token exchange failed: {other}"),
        })?;
    response
        .into_json::<Value>()
        .map_err(|error| format!("Codex OAuth token exchange returned invalid JSON: {error}"))
}

fn persist_codex_oauth(path: &Path, token: Value) -> Result<(), String> {
    let access = token
        .get("access_token")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "Codex OAuth token exchange did not return access_token".to_string())?;
    let refresh = token
        .get("refresh_token")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "Codex OAuth token exchange did not return refresh_token".to_string())?;
    let expires_in = token
        .get("expires_in")
        .and_then(Value::as_i64)
        .filter(|value| *value > 0)
        .ok_or_else(|| "Codex OAuth token exchange did not return expires_in".to_string())?;
    let account_id = codex_account_id(access)?;
    let expires = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .saturating_add((expires_in as u128).saturating_mul(1000))
        .min(i64::MAX as u128) as i64;
    let _lock = CodexAuthLock::acquire(path)?;
    let mut data = read_json_or_empty(path)?;
    if !data.is_object() {
        data = json!({});
    }
    data[OPENAI_CODEX_PROVIDER_ID] = json!({
        "type": "oauth",
        "access": access,
        "refresh": refresh,
        "expires": expires,
        "accountId": account_id,
    });
    write_private_json(path, &data)
}

fn read_settings_json(path: &Path) -> Result<Value, String> {
    match fs::read_to_string(path) {
        Ok(raw) => serde_json::from_str(&raw)
            .map_err(|error| format!("read settings {}: invalid JSON: {error}", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(json!({})),
        Err(error) => Err(format!("read settings {}: {error}", path.display())),
    }
}

fn load_role_profiles(path: &Path) -> BTreeMap<String, String> {
    let Ok(data) = read_settings_json(path) else {
        return BTreeMap::new();
    };
    let Some(raw) = data
        .get("oppi")
        .and_then(|oppi| oppi.get("roleModels"))
        .and_then(Value::as_object)
    else {
        return BTreeMap::new();
    };
    raw.iter()
        .filter_map(|(role, model)| {
            let role = normalize_role(role)?;
            let model = model.as_str()?.trim();
            (!model.is_empty()).then(|| (role.to_string(), model.to_string()))
        })
        .collect()
}

fn save_role_profiles(path: &Path, role_models: &BTreeMap<String, String>) -> Result<(), String> {
    let mut data = read_settings_json(path)?;
    if !data.is_object() {
        data = json!({});
    }
    if !data.get("oppi").is_some_and(Value::is_object) {
        data["oppi"] = json!({});
    }
    data["oppi"]["roleModels"] = json!(role_models);
    write_settings_json(path, &data)
}

fn load_permission_mode_setting(path: &Path) -> (PermissionMode, String) {
    if let Some(mode) = permission_mode_from_env() {
        return (mode, "env:OPPI_RUNTIME_WORKER_PERMISSION_MODE".to_string());
    }
    if let Some(mode) = read_settings_json(path).ok().and_then(|data| {
        data.get("oppi")
            .and_then(|oppi| oppi.get("permissionMode"))
            .and_then(Value::as_str)
            .and_then(parse_permission_mode)
    }) {
        return (mode, "settings".to_string());
    }
    (PermissionMode::AutoReview, "default".to_string())
}

fn save_permission_mode_setting(path: &Path, mode: PermissionMode) -> Result<(), String> {
    let mut data = read_settings_json(path)?;
    if !data.is_object() {
        data = json!({});
    }
    if !data.get("oppi").is_some_and(Value::is_object) {
        data["oppi"] = json!({});
    }
    data["oppi"]["permissionMode"] = json!(mode.as_str());
    write_settings_json(path, &data)
}

fn load_reasoning_effort_setting(path: &Path) -> Option<ThinkingLevel> {
    read_settings_json(path).ok().and_then(|data| {
        data.get("oppi")
            .and_then(|oppi| oppi.get("reasoningEffort"))
            .and_then(Value::as_str)
            .and_then(normalize_thinking_level)
    })
}

fn save_reasoning_effort_setting(path: &Path, level: ThinkingLevel) -> Result<(), String> {
    let mut data = read_settings_json(path)?;
    if !data.is_object() {
        data = json!({});
    }
    if !data.get("oppi").is_some_and(Value::is_object) {
        data["oppi"] = json!({});
    }
    data["oppi"]["reasoningEffort"] = json!(level.as_str());
    write_settings_json(path, &data)
}

fn load_prompt_variant_setting(path: &Path) -> String {
    read_settings_json(path)
        .ok()
        .and_then(|data| {
            data.get("oppi")
                .and_then(|oppi| oppi.get("promptVariant"))
                .and_then(Value::as_str)
                .and_then(normalize_prompt_variant)
        })
        .unwrap_or_else(|| "off".to_string())
}

fn save_prompt_variant_setting(path: &Path, variant: &str) -> Result<(), String> {
    let normalized = normalize_prompt_variant(variant)
        .ok_or_else(|| "usage: /prompt-variant [off|a|b|caveman]".to_string())?;
    let mut data = read_settings_json(path)?;
    if !data.is_object() {
        data = json!({});
    }
    if !data.get("oppi").is_some_and(Value::is_object) {
        data["oppi"] = json!({});
    }
    data["oppi"]["promptVariant"] = json!(normalized);
    write_settings_json(path, &data)
}

fn load_enabled_model_scope(path: &Path) -> Vec<String> {
    let Ok(data) = read_settings_json(path) else {
        return Vec::new();
    };
    data.get("enabledModels")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn save_enabled_model_scope(path: &Path, models: Option<&[String]>) -> Result<(), String> {
    let mut data = read_settings_json(path)?;
    if !data.is_object() {
        data = json!({});
    }
    if let Some(models) = models.filter(|models| !models.is_empty()) {
        data["enabledModels"] = json!(models);
    } else if let Some(object) = data.as_object_mut() {
        object.remove("enabledModels");
    }
    write_settings_json(path, &data)
}

fn write_settings_json(path: &Path, data: &Value) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("create settings dir {}: {error}", parent.display()))?;
    }
    let rendered = serde_json::to_string_pretty(data)
        .map_err(|error| format!("serialize settings {}: {error}", path.display()))?;
    fs::write(path, format!("{rendered}\n"))
        .map_err(|error| format!("write settings {}: {error}", path.display()))
}

fn default_server_path() -> PathBuf {
    if let Ok(path) = env::var("OPPI_SERVER_BIN") {
        let path = path.trim();
        if !path.is_empty() {
            return PathBuf::from(path);
        }
    }
    if let Ok(exe) = env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let candidate = dir.join(format!("oppi-server{}", env::consts::EXE_SUFFIX));
        if candidate.exists() {
            return candidate;
        }
    }
    PathBuf::from(format!("oppi-server{}", env::consts::EXE_SUFFIX))
}

fn format_approval_panel(request: &ApprovalRequest) -> String {
    let mut lines = vec![
        "approval needed".to_string(),
        format!("  reason: {}", request.reason),
        format!("  risk: {:?}", request.risk),
    ];
    if let Some(call) = &request.tool_call {
        lines.push(format!(
            "  tool: {}:{} ({})",
            call.namespace.as_deref().unwrap_or("tool"),
            call.name,
            call.id
        ));
        lines.push(format!("  args: {}", compact_json(&call.arguments)));
    }
    lines.push("  action: /approve or /deny".to_string());
    lines.join("\n")
}

fn format_ask_user_panel(request: &AskUserRequest) -> String {
    let mut lines = vec![format!(
        "question: {}",
        request.title.as_deref().unwrap_or("user input requested")
    )];
    for question in &request.questions {
        lines.push(format!("  {}: {}", question.id, question.question));
        for option in &question.options {
            lines.push(format!("    - {}: {}", option.id, option.label));
        }
    }
    lines.push("  action: /answer <text-or-option-id>".to_string());
    lines.join("\n")
}

fn compact_json(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string())
}

fn is_background_latest_alias(value: &str) -> bool {
    matches!(value.to_ascii_lowercase().as_str(), "latest" | "last" | ".")
}

fn current_time_ms() -> Option<u64> {
    Some(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()?
            .as_millis()
            .min(u128::from(u64::MAX)) as u64,
    )
}

fn json_u64(value: &Value, key: &str) -> Option<u64> {
    value.get(key).and_then(|value| {
        value
            .as_u64()
            .or_else(|| value.as_i64().and_then(|n| n.try_into().ok()))
    })
}

fn task_started_at_ms(task: &Value) -> Option<u64> {
    json_u64(task, "startedAtMs")
}

fn task_output_bytes(task: &Value) -> Option<u64> {
    json_u64(task, "outputBytes")
}

fn select_background_task_id(value: &Value, running_only: bool) -> Option<String> {
    let items = value.get("items")?.as_array()?;
    items
        .iter()
        .filter(|item| {
            !running_only
                || item
                    .get("status")
                    .and_then(Value::as_str)
                    .is_some_and(|status| status.eq_ignore_ascii_case("running"))
        })
        .filter_map(|item| {
            Some((
                task_started_at_ms(item).unwrap_or_default(),
                item.get("id")?.as_str()?.to_string(),
            ))
        })
        .max_by(|(left_started, left_id), (right_started, right_id)| {
            left_started
                .cmp(right_started)
                .then_with(|| left_id.cmp(right_id))
        })
        .map(|(_, id)| id)
}

fn format_duration_ms(ms: u64) -> String {
    if ms < 1_000 {
        format!("{ms}ms")
    } else if ms < 60_000 {
        format!("{}s", ms / 1_000)
    } else {
        format!("{}m{}s", ms / 60_000, (ms % 60_000) / 1_000)
    }
}

fn format_task_elapsed(task: &Value) -> Option<String> {
    let started = task_started_at_ms(task)?;
    let status = task.get("status").and_then(Value::as_str).unwrap_or("");
    if status.eq_ignore_ascii_case("running") {
        let now = current_time_ms()?;
        return now
            .checked_sub(started)
            .map(|ms| format!("for {}", format_duration_ms(ms)));
    }
    let finished = json_u64(task, "finishedAtMs")?;
    finished
        .checked_sub(started)
        .map(|ms| format!("ran {}", format_duration_ms(ms)))
}

fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes}B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1}KiB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1}MiB", bytes as f64 / (1024.0 * 1024.0))
    }
}

fn shorten_command(command: &str) -> String {
    const MAX: usize = 96;
    if command.chars().count() <= MAX {
        return command.to_string();
    }
    let head = command
        .chars()
        .take(MAX.saturating_sub(1))
        .collect::<String>();
    format!("{head}…")
}

fn format_background_meta(task: &Value) -> String {
    let mut meta = vec![
        task.get("status")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string(),
    ];
    if let Some(elapsed) = format_task_elapsed(task) {
        meta.push(elapsed);
    }
    if let Some(code) = task.get("exitCode").and_then(Value::as_i64) {
        meta.push(format!("exit {code}"));
    }
    if let Some(bytes) = task_output_bytes(task) {
        meta.push(format_bytes(bytes));
    }
    meta.join(", ")
}

fn format_background_counts(items: &[Value]) -> String {
    let mut counts = BTreeMap::<String, usize>::new();
    for item in items {
        let status = item
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_ascii_lowercase();
        *counts.entry(status).or_default() += 1;
    }
    counts
        .into_iter()
        .map(|(status, count)| format!("{count} {status}"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn background_summary_from_list(value: &Value) -> String {
    let Some(items) = value.get("items").and_then(Value::as_array) else {
        return "status unavailable".to_string();
    };
    if items.is_empty() {
        return "no process-local tasks".to_string();
    }
    let latest = select_background_task_id(value, false).unwrap_or_else(|| "latest".to_string());
    format!("{}; latest={latest}", format_background_counts(items))
}

fn background_summary_from_read(value: &Value) -> String {
    let task = value.get("task").unwrap_or(&Value::Null);
    let id = task.get("id").and_then(Value::as_str).unwrap_or("task");
    let status = task
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let bytes = value
        .get("outputBytes")
        .and_then(Value::as_u64)
        .or_else(|| task.get("outputBytes").and_then(Value::as_u64))
        .unwrap_or(0);
    format!("read {id} ({status}, {})", format_bytes(bytes))
}

fn background_summary_from_kill(value: &Value) -> String {
    let task = value.get("task").unwrap_or(&Value::Null);
    let id = task.get("id").and_then(Value::as_str).unwrap_or("task");
    let status = task
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    format!("kill requested for {id} ({status})")
}

fn format_background_list(value: &Value) -> String {
    let Some(items) = value.get("items").and_then(Value::as_array) else {
        return format!("background: {}", compact_json(value));
    };
    if items.is_empty() {
        return "background: none (process-local; tasks reset when the server restarts)"
            .to_string();
    }
    let mut sorted = items.iter().collect::<Vec<_>>();
    sorted.sort_by(|left, right| {
        task_started_at_ms(right)
            .cmp(&task_started_at_ms(left))
            .then_with(|| {
                left.get("id")
                    .and_then(Value::as_str)
                    .cmp(&right.get("id").and_then(Value::as_str))
            })
    });
    let mut lines = vec![format!(
        "background: {} task(s) ({})",
        items.len(),
        format_background_counts(items)
    )];
    for item in sorted {
        let id = item.get("id").and_then(Value::as_str).unwrap_or("unknown");
        let command = item.get("command").and_then(Value::as_str).unwrap_or("");
        lines.push(format!(
            "  {id} [{}] {}",
            format_background_meta(item),
            shorten_command(command)
        ));
        let cwd = item.get("cwd").and_then(Value::as_str).unwrap_or("");
        let log = item.get("outputPath").and_then(Value::as_str).unwrap_or("");
        if !cwd.is_empty() || !log.is_empty() {
            lines.push(format!("    cwd: {cwd}  log: {log}"));
        }
    }
    lines.push(
        "actions: /background read [latest|id] [bytes], /background kill [latest|id]".to_string(),
    );
    lines.join("\n")
}

fn format_background_read(value: &Value) -> String {
    let task = value.get("task").unwrap_or(&Value::Null);
    let id = task.get("id").and_then(Value::as_str).unwrap_or("unknown");
    let output = value.get("output").and_then(Value::as_str).unwrap_or("");
    let output_bytes = value
        .get("outputBytes")
        .and_then(Value::as_u64)
        .or_else(|| task_output_bytes(task));
    let max_bytes = json_u64(value, "maxBytes");
    let mut lines = vec![format!(
        "background {id} [{}]",
        format_background_meta(task)
    )];
    if let Some(log) = task.get("outputPath").and_then(Value::as_str)
        && !log.is_empty()
    {
        lines.push(format!("log: {log}"));
    }
    if output.is_empty() {
        lines.push("output: no output yet".to_string());
    } else {
        let tail_note = if value
            .get("truncated")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            match (max_bytes, output_bytes) {
                (Some(max), Some(total)) => format!(
                    "tail: last {} of {}",
                    format_bytes(max),
                    format_bytes(total)
                ),
                _ => "tail: truncated".to_string(),
            }
        } else {
            match output_bytes {
                Some(total) => format!("output: {}", format_bytes(total)),
                None => "output:".to_string(),
            }
        };
        lines.push(tail_note);
        lines.push(trim_output_tail(output, 12_000));
    }
    lines.join("\n")
}

fn format_background_kill(value: &Value) -> String {
    let task = value.get("task").unwrap_or(&Value::Null);
    let mut lines = vec![format!(
        "background {} [{}]: {}",
        task.get("id").and_then(Value::as_str).unwrap_or("unknown"),
        format_background_meta(task),
        value
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("updated")
    )];
    if let Some(log) = task.get("outputPath").and_then(Value::as_str)
        && !log.is_empty()
    {
        lines.push(format!("log: {log}"));
    }
    lines.join("\n")
}

fn trim_output_tail(output: &str, max_chars: usize) -> String {
    if output.chars().count() <= max_chars {
        return output.to_string();
    }
    let tail = output
        .chars()
        .rev()
        .take(max_chars)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<String>();
    format!("…\n{tail}")
}

#[cfg(test)]
fn render_event_line(event: &Event) -> Option<String> {
    render_event_line_themed(event, "plain", &BTreeMap::new())
}

#[derive(Debug, Clone, Copy)]
struct ThemeTokens {
    accent: &'static str,
    dim: &'static str,
    code: &'static str,
    success: &'static str,
    warning: &'static str,
    error: &'static str,
    reset: &'static str,
}

fn plain_theme_tokens() -> ThemeTokens {
    ThemeTokens {
        accent: "",
        dim: "",
        code: "",
        success: "",
        warning: "",
        error: "",
        reset: "",
    }
}

fn theme_tokens(name: &str) -> ThemeTokens {
    if !ansi_enabled() || name == "plain" {
        return plain_theme_tokens();
    }
    ansi_theme_tokens(name)
}

fn ansi_theme_tokens(name: &str) -> ThemeTokens {
    match name {
        "light" => ThemeTokens {
            accent: "\x1b[34;1m",
            dim: "\x1b[2m",
            code: "\x1b[35m",
            success: "\x1b[32m",
            warning: "\x1b[33m",
            error: "\x1b[31m",
            reset: "\x1b[0m",
        },
        "dark" => ThemeTokens {
            accent: "\x1b[36;1m",
            dim: "\x1b[2m",
            code: "\x1b[35m",
            success: "\x1b[32m",
            warning: "\x1b[33m",
            error: "\x1b[31m",
            reset: "\x1b[0m",
        },
        _ => ThemeTokens {
            accent: "\x1b[38;5;81;1m",
            dim: "\x1b[2m",
            code: "\x1b[38;5;141m",
            success: "\x1b[38;5;114m",
            warning: "\x1b[38;5;221m",
            error: "\x1b[38;5;203m",
            reset: "\x1b[0m",
        },
    }
}

fn ansi_enabled() -> bool {
    env::var_os("NO_COLOR").is_none() && env::var("TERM").map(|term| term != "dumb").unwrap_or(true)
}

fn paint(tokens: ThemeTokens, color: &'static str, text: impl AsRef<str>) -> String {
    let text = text.as_ref();
    if color.is_empty() {
        text.to_string()
    } else {
        format!("{color}{text}{}", tokens.reset)
    }
}

fn render_event_line_themed(
    event: &Event,
    theme: &str,
    calls: &BTreeMap<String, ToolCall>,
) -> Option<String> {
    let tokens = theme_tokens(theme);
    match &event.kind {
        EventKind::ThreadStarted { thread } => Some(format!(
            "thread {} started",
            paint(tokens, tokens.accent, &thread.id)
        )),
        EventKind::ThreadResumed { thread } => Some(format!(
            "thread {} resumed",
            paint(tokens, tokens.accent, &thread.id)
        )),
        EventKind::ThreadForked {
            thread,
            from_thread_id,
        } => Some(format!(
            "thread {} forked from {from_thread_id}",
            paint(tokens, tokens.accent, &thread.id)
        )),
        EventKind::ThreadUpdated { thread } => Some(format!(
            "thread {} updated ({:?})",
            paint(tokens, tokens.accent, &thread.id),
            thread.status
        )),
        EventKind::ThreadGoalUpdated { goal } => Some(format!(
            "{}: {}",
            format_goal_status_label(Some(goal)),
            goal.objective
        )),
        EventKind::ThreadGoalCleared { .. } => Some("goal cleared".to_string()),
        EventKind::TurnStarted { turn } => Some(format!(
            "turn {} started",
            paint(tokens, tokens.dim, &turn.id)
        )),
        EventKind::TurnPhaseChanged { phase } => Some(format!(
            "phase: {}",
            paint(tokens, tokens.dim, format!("{phase:?}"))
        )),
        EventKind::ItemCompleted { item } => match &item.kind {
            oppi_protocol::ItemKind::UserMessage { text } => Some(format!("user: {text}")),
            oppi_protocol::ItemKind::AssistantMessage { text } => {
                Some(format!("assistant:\n{}", render_markdown(text, theme)))
            }
            _ => None,
        },
        EventKind::ItemDelta { delta, .. } => {
            Some(format!("assistant Δ {}", render_markdown(delta, theme)))
        }
        EventKind::ToolCallStarted { call } => Some(format_tool_call_started(call, tokens)),
        EventKind::ToolCallCompleted { result } => Some(format_tool_result_digest(
            result,
            calls.get(&result.call_id),
            tokens,
        )),
        EventKind::ArtifactCreated { artifact } => Some(format_artifact_created(artifact, tokens)),
        EventKind::AgentStarted { run } => Some(format_agent_started(run, tokens)),
        EventKind::AgentBlocked { run_id, reason } => Some(format!(
            "agent {} blocked: {}",
            paint(tokens, tokens.warning, run_id),
            paint(tokens, tokens.warning, reason)
        )),
        EventKind::AgentCompleted { run_id, output } => Some(format!(
            "agent {} completed: {}",
            paint(tokens, tokens.success, run_id),
            render_markdown(output, "plain")
        )),
        EventKind::ApprovalRequested { request } => Some(format_approval_panel(request)),
        EventKind::AskUserRequested { request } => Some(format_ask_user_panel(request)),
        EventKind::Diagnostic { diagnostic } => Some(format!("diagnostic: {}", diagnostic.message)),
        EventKind::TurnCompleted { turn_id } => Some(format!(
            "turn {} completed",
            paint(tokens, tokens.success, turn_id)
        )),
        EventKind::TurnAborted { reason } => Some(format!(
            "turn aborted: {}",
            paint(tokens, tokens.error, reason)
        )),
        EventKind::TurnInterrupted { reason } => Some(format!(
            "turn interrupted: {}",
            paint(tokens, tokens.warning, reason)
        )),
        _ => None,
    }
}

fn render_markdown(text: &str, theme: &str) -> String {
    let tokens = theme_tokens(theme);
    let mut in_code = false;
    let mut lines = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            in_code = !in_code;
            lines.push(paint(tokens, tokens.dim, line));
        } else if in_code {
            let color = if trimmed.starts_with('+') {
                tokens.success
            } else if trimmed.starts_with('-') {
                tokens.error
            } else if trimmed.starts_with('@') {
                tokens.warning
            } else {
                tokens.code
            };
            lines.push(paint(tokens, color, line));
        } else if trimmed.starts_with('#') {
            lines.push(paint(tokens, tokens.accent, line));
        } else if trimmed.starts_with('>') {
            lines.push(paint(tokens, tokens.dim, line));
        } else if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
            lines.push(format!("  • {}", trimmed[2..].trim_start()));
        } else {
            lines.push(line.to_string());
        }
    }
    if lines.is_empty() {
        text.to_string()
    } else {
        lines.join("\n")
    }
}

fn format_tool_call_started(call: &ToolCall, tokens: ThemeTokens) -> String {
    let namespace = call.namespace.as_deref().unwrap_or("tool");
    format!(
        "tool {} started {}",
        paint(tokens, tokens.accent, format!("{namespace}:{}", call.name)),
        paint(tokens, tokens.dim, format!("({})", call.id))
    )
}

fn format_tool_result_digest(
    result: &oppi_protocol::ToolResult,
    call: Option<&ToolCall>,
    tokens: ThemeTokens,
) -> String {
    let status = match result.status {
        oppi_protocol::ToolResultStatus::Ok => paint(tokens, tokens.success, "ok"),
        oppi_protocol::ToolResultStatus::Denied => paint(tokens, tokens.warning, "denied"),
        oppi_protocol::ToolResultStatus::Error => paint(tokens, tokens.error, "error"),
        oppi_protocol::ToolResultStatus::Aborted => paint(tokens, tokens.warning, "aborted"),
    };
    let tool = call
        .map(|call| {
            format!(
                "{}:{}",
                call.namespace.as_deref().unwrap_or("tool"),
                call.name
            )
        })
        .unwrap_or_else(|| result.call_id.clone());
    let mut line = format!("tool {tool} → {status}");
    if let Some(output) = result
        .output
        .as_deref()
        .filter(|output| !output.trim().is_empty())
    {
        let preview = trim_output_tail(output.trim(), 600);
        line.push_str(&format!("\n{}", paint(tokens, tokens.dim, preview)));
        if let Some(artifact) = artifact_hint(output) {
            line.push_str(&format!(
                "\nartifact: {}",
                paint(tokens, tokens.accent, artifact)
            ));
        }
    }
    if let Some(error) = result.error.as_deref() {
        line.push_str(&format!("\n{}", paint(tokens, tokens.error, error)));
    }
    line
}

fn format_agent_started(run: &AgentRun, tokens: ThemeTokens) -> String {
    let mut meta = Vec::new();
    if run.background {
        meta.push("background=true".to_string());
    }
    if let Some(role) = run.role.as_deref() {
        meta.push(format!("role={role}"));
    }
    if let Some(model) = run.model.as_deref() {
        meta.push(format!("model={model}"));
    }
    if let Some(effort) = run.effort.as_deref() {
        meta.push(format!("effort={effort}"));
    }
    if let Some(mode) = run.permission_mode {
        meta.push(format!("permissions={}", mode.as_str()));
    }
    if let Some(memory) = run.memory_mode.as_deref() {
        meta.push(format!("memory={memory}"));
    }
    if !run.tool_allowlist.is_empty() {
        meta.push(format!("tools={}", run.tool_allowlist.join(",")));
    }
    if !run.tool_denylist.is_empty() {
        meta.push(format!("deny={}", run.tool_denylist.join(",")));
    }
    if let Some(max_turns) = run.max_turns {
        meta.push(format!("maxTurns={max_turns}"));
    }
    let suffix = if meta.is_empty() {
        String::new()
    } else {
        format!(" ({})", meta.join("; "))
    };
    format!(
        "agent {} started {}: {}{}",
        paint(tokens, tokens.accent, &run.id),
        paint(tokens, tokens.accent, &run.agent_name),
        run.task,
        suffix
    )
}

fn format_artifact_created(artifact: &ArtifactMetadata, tokens: ThemeTokens) -> String {
    let mut line = format!(
        "artifact {} created: {}",
        paint(tokens, tokens.accent, &artifact.id),
        paint(tokens, tokens.accent, &artifact.output_path)
    );
    let mut meta = Vec::new();
    if let Some(mime) = artifact.mime_type.as_deref() {
        meta.push(mime.to_string());
    }
    if let (Some(width), Some(height)) = (artifact.width, artifact.height) {
        meta.push(format!("{width}x{height}"));
    }
    if let Some(bytes) = artifact.bytes {
        meta.push(format_bytes(bytes));
    }
    if let Some(backend) = artifact.backend.as_deref() {
        meta.push(format!("backend={backend}"));
    }
    if let Some(model) = artifact.model.as_deref() {
        meta.push(format!("model={model}"));
    }
    if !meta.is_empty() {
        line.push_str(&format!(" ({})", meta.join(", ")));
    }
    if !artifact.source_images.is_empty() {
        line.push_str(&format!("\nsources: {}", artifact.source_images.join(", ")));
    }
    if let Some(mask) = artifact.mask.as_deref() {
        line.push_str(&format!("\nmask: {mask}"));
    }
    line
}

fn artifact_hint(output: &str) -> Option<String> {
    let value: Value = serde_json::from_str(output).ok()?;
    for key in ["outputPath", "path", "file"] {
        if let Some(path) = value.get(key).and_then(Value::as_str) {
            return Some(path.to_string());
        }
    }
    None
}

struct RpcClient {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<std::process::ChildStdout>,
    next_id: u64,
}

impl RpcClient {
    fn spawn(server: PathBuf) -> Result<Self, String> {
        let mut command = Command::new(&server);
        command
            .arg("--stdio")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        if env::var("OPPI_RUNTIME_STORE_DIR")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .is_none()
        {
            command.env("OPPI_RUNTIME_STORE_DIR", default_runtime_store_dir());
        }
        let mut child = command
            .spawn()
            .map_err(|error| format!("spawn {}: {error}", server.display()))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| "oppi-server stdin unavailable".to_string())?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "oppi-server stdout unavailable".to_string())?;
        Ok(Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
            next_id: 1,
        })
    }

    fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        let request = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        writeln!(self.stdin, "{request}").map_err(|error| format!("write request: {error}"))?;
        self.stdin
            .flush()
            .map_err(|error| format!("flush request: {error}"))?;
        let mut line = String::new();
        self.stdout
            .read_line(&mut line)
            .map_err(|error| format!("read response: {error}"))?;
        if line.trim().is_empty() {
            return Err("oppi-server closed stdout".to_string());
        }
        let response: Value =
            serde_json::from_str(&line).map_err(|error| format!("decode response: {error}"))?;
        if let Some(error) = response.get("error").filter(|error| !error.is_null()) {
            return Err(error.to_string());
        }
        response
            .get("result")
            .cloned()
            .ok_or_else(|| "JSON-RPC response missing result".to_string())
    }
}

impl Drop for RpcClient {
    fn drop(&mut self) {
        let _ = self.child.try_wait();
        let _ = self.child.kill();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_codex_access_token(account_id: &str) -> String {
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"none"}"#);
        let payload = URL_SAFE_NO_PAD.encode(
            format!(r#"{{"https://api.openai.com/auth":{{"chatgpt_account_id":"{account_id}"}}}}"#)
                .as_bytes(),
        );
        format!("{header}.{payload}.sig")
    }

    #[test]
    fn parses_mock_prompt() {
        let parsed = parse_args(vec!["--mock".into(), "hello".into(), "world".into()]).unwrap();
        assert_eq!(parsed.initial_prompt.as_deref(), Some("hello world"));
        assert!(matches!(parsed.provider, ProviderConfig::Mock));
        assert!(!parsed.json);
        assert!(!parsed.interactive);
    }

    #[test]
    fn parses_interactive_without_prompt() {
        let parsed = parse_args(vec!["--mock".into()]).unwrap();
        assert_eq!(parsed.initial_prompt, None);
        assert!(parsed.interactive);
    }

    #[test]
    fn parses_raw_mode_flag() {
        let parsed = parse_args(vec!["--mock".into(), "--raw".into()]).unwrap();
        assert!(parsed.raw);
        assert!(parsed.interactive);
    }

    #[test]
    fn parses_ratatui_as_default_ui() {
        let parsed = parse_args(vec!["--mock".into()]).unwrap();
        assert!(parsed.ratatui);
    }

    #[test]
    fn parses_ratatui_preview_flag() {
        let parsed = parse_args(vec!["--mock".into(), "--ratatui".into()]).unwrap();
        assert!(parsed.raw);
        assert!(parsed.ratatui);
        assert!(parsed.interactive);
    }

    #[test]
    fn parses_no_ratatui_fallback_flag() {
        let parsed = parse_args(vec!["--mock".into(), "--no-ratatui".into()]).unwrap();
        assert!(!parsed.ratatui);
    }

    #[test]
    fn parses_resume_thread_flag() {
        let parsed =
            parse_args(vec!["--mock".into(), "--resume".into(), "thread-7".into()]).unwrap();
        assert_eq!(parsed.resume_thread.as_deref(), Some("thread-7"));
    }

    #[test]
    fn parses_session_list_flag_as_non_interactive() {
        let parsed = parse_args(vec!["--list-sessions".into()]).unwrap();
        assert!(parsed.list_sessions);
        assert!(!parsed.interactive);
        assert!(matches!(parsed.provider, ProviderConfig::Mock));
    }

    #[test]
    fn dogfood_background_command_uses_portable_shell_marker() {
        let command = dogfood_background_command();
        assert!(command.contains("oppi-background-dogfood"));
        assert!(!command.contains("node -e"));
        if cfg!(windows) {
            assert!(command.contains("ping -n"));
        } else {
            assert!(command.starts_with("/bin/echo "));
            assert!(command.contains("; sleep "));
        }
    }

    #[test]
    fn parses_real_provider_flags() {
        let parsed = parse_args(vec![
            "--model".into(),
            "gpt-test".into(),
            "--base-url".into(),
            "http://127.0.0.1:3000/v1".into(),
            "--api-key-env".into(),
            "OPPI_TEST_API_KEY".into(),
            "--no-stream".into(),
            "hello".into(),
        ])
        .unwrap();
        match parsed.provider {
            ProviderConfig::OpenAiCompatible(config) => {
                assert_eq!(config.model, "gpt-test");
                assert_eq!(config.base_url.as_deref(), Some("http://127.0.0.1:3000/v1"));
                assert_eq!(config.api_key_env.as_deref(), Some("OPPI_TEST_API_KEY"));
                assert!(!config.stream);
            }
            ProviderConfig::Mock => panic!("expected real provider"),
        }
        assert_eq!(parsed.initial_prompt.as_deref(), Some("hello"));
    }

    #[test]
    fn rejects_provider_without_model() {
        let error = parse_args(vec!["--api-key-env".into(), "OPPI_TEST_API_KEY".into()])
            .expect_err("missing model should fail");
        assert!(error.contains("--model"));
    }

    #[test]
    fn renders_core_events() {
        let event = Event {
            id: 1,
            thread_id: "thread-1".to_string(),
            turn_id: Some("turn-1".to_string()),
            kind: EventKind::ItemDelta {
                item_id: "item-1".to_string(),
                delta: "hi".to_string(),
            },
        };
        assert_eq!(
            render_event_line(&event),
            Some("assistant Δ hi".to_string())
        );

        let agent_started = Event {
            id: 2,
            thread_id: "thread-1".to_string(),
            turn_id: Some("turn-1".to_string()),
            kind: EventKind::AgentStarted {
                run: AgentRun {
                    id: "agent-run-1".to_string(),
                    thread_id: "thread-1".to_string(),
                    agent_name: "general-purpose".to_string(),
                    status: oppi_protocol::AgentRunStatus::Running,
                    task: "inspect".to_string(),
                    worktree_root: None,
                    background: false,
                    role: Some("subagent".to_string()),
                    model: Some("gpt-sub".to_string()),
                    effort: Some("medium".to_string()),
                    permission_mode: Some(PermissionMode::ReadOnly),
                    memory_mode: Some("disabled".to_string()),
                    tool_allowlist: vec!["read_file".to_string()],
                    tool_denylist: vec!["shell_exec".to_string()],
                    isolation: Some("thread".to_string()),
                    color: Some("cyan".to_string()),
                    skills: vec!["independent".to_string()],
                    max_turns: Some(2),
                },
            },
        };
        let rendered = render_event_line(&agent_started).unwrap();
        assert!(rendered.contains("agent-run-1"));
        assert!(rendered.contains("role=subagent"));
        assert!(rendered.contains("permissions=read-only"));
        assert!(rendered.contains("tools=read_file"));

        let memory_panel = format_memory_control(&json!({
            "title": "Hoppi memory dashboard",
            "summary": "client-hosted Hoppi controls; no hidden model session was started",
            "status": { "enabled": true, "backend": "client", "scope": "project", "memoryCount": 3 },
            "controls": [
                { "id": "maintenance", "label": "Preview maintenance", "command": "/memory maintenance dry-run", "description": "Explicit only" }
            ]
        }));
        assert!(memory_panel.contains("Hoppi memory dashboard"));
        assert!(memory_panel.contains("status: enabled backend=client"));
        assert!(memory_panel.contains("/memory maintenance dry-run"));
    }

    #[test]
    fn editor_handles_submit_multiline_escape_and_ctrl_d() {
        let mut editor = LineEditor::default();
        assert_eq!(
            editor.handle(EditorInput::Text("hello".to_string())),
            EditorAction::None
        );
        assert_eq!(editor.handle(EditorInput::ShiftEnter), EditorAction::None);
        assert_eq!(
            editor.handle(EditorInput::Text("world".to_string())),
            EditorAction::None
        );
        assert_eq!(
            editor.handle(EditorInput::Enter),
            EditorAction::Submit("hello\nworld".to_string())
        );
        assert_eq!(
            editor.handle(EditorInput::Text("draft".to_string())),
            EditorAction::None
        );
        assert_eq!(editor.handle(EditorInput::Escape), EditorAction::Cleared);
        assert_eq!(editor.handle(EditorInput::CtrlD), EditorAction::Exit);

        assert_eq!(
            editor.handle(EditorInput::Text("queue me".to_string())),
            EditorAction::None
        );
        assert_eq!(
            editor.handle(EditorInput::AltEnter),
            EditorAction::SubmitFollowUp("queue me".to_string())
        );
        assert_eq!(
            editor.handle(EditorInput::Text("steer me".to_string())),
            EditorAction::None
        );
        assert_eq!(
            editor.handle(EditorInput::CtrlEnter),
            EditorAction::Steer("steer me".to_string())
        );
        assert_eq!(
            editor.handle(EditorInput::AltUp),
            EditorAction::RestoreQueued
        );
        assert_eq!(editor.handle(EditorInput::CtrlC), EditorAction::Interrupt);
        assert_eq!(
            editor.handle(EditorInput::CtrlC),
            EditorAction::Submit("/exit".to_string())
        );

        let mut editor = LineEditor::default();
        assert_eq!(
            editor.handle(EditorInput::Text("draft".to_string())),
            EditorAction::None
        );
        assert_eq!(editor.handle(EditorInput::CtrlC), EditorAction::Cleared);
        assert_eq!(
            editor.handle(EditorInput::CtrlC),
            EditorAction::Submit("/exit".to_string())
        );

        let mut editor = LineEditor::default();
        assert_eq!(editor.handle(EditorInput::CtrlC), EditorAction::Interrupt);
        assert_eq!(
            editor.handle(EditorInput::Text("keep running".to_string())),
            EditorAction::None
        );
        assert_eq!(editor.handle(EditorInput::CtrlC), EditorAction::Cleared);
    }

    #[test]
    fn editor_ctrl_backspace_deletes_word_and_alt_backspace_deletes_line() {
        let mut editor = LineEditor::default();
        assert_eq!(
            editor.handle(EditorInput::Text("hello brave world".to_string())),
            EditorAction::None
        );
        assert_eq!(
            editor.handle(EditorInput::CtrlBackspace),
            EditorAction::None
        );
        assert_eq!(editor.buffer_preview(), "hello brave ");
        assert_eq!(
            editor.handle(EditorInput::CtrlBackspace),
            EditorAction::None
        );
        assert_eq!(editor.buffer_preview(), "hello ");
        assert_eq!(editor.handle(EditorInput::AltBackspace), EditorAction::None);
        assert_eq!(editor.buffer_preview(), "");

        assert_eq!(
            editor.handle(EditorInput::Text("one\ntwo three\nfour".to_string())),
            EditorAction::None
        );
        editor.move_start();
        assert_eq!(editor.handle(EditorInput::AltBackspace), EditorAction::None);
        assert_eq!(editor.buffer_preview(), "one\ntwo three\n");
    }

    #[test]
    fn raw_key_parser_maps_supported_sequences() {
        let mut parser = RawInputParser::default();
        let inputs = b"hi\x1b\rthere\x1b[13;2u\x1b[13;5u\x1b[1;3A\x17\x1b\x7f\x03\x04"
            .iter()
            .flat_map(|byte| parser.push(*byte))
            .collect::<Vec<_>>();
        assert!(inputs.contains(&EditorInput::Text("h".to_string())));
        assert!(inputs.contains(&EditorInput::AltEnter));
        assert!(inputs.contains(&EditorInput::ShiftEnter));
        assert!(inputs.contains(&EditorInput::CtrlEnter));
        assert!(inputs.contains(&EditorInput::AltUp));
        assert!(inputs.contains(&EditorInput::CtrlBackspace));
        assert!(inputs.contains(&EditorInput::AltBackspace));
        assert!(inputs.contains(&EditorInput::CtrlC));
        assert!(inputs.contains(&EditorInput::CtrlD));
        assert_eq!(RawInputParser::default().push(0x09), vec![EditorInput::Tab]);
        assert_eq!(parser.finish(), Vec::<EditorInput>::new());
    }

    #[test]
    fn slash_palette_lists_commands_filters_and_accepts_arguments() {
        let all = slash_palette_for_buffer("/", 0).expect("slash palette should open");
        assert_eq!(all.mode, SlashPaletteMode::Commands);
        assert_eq!(all.items.len(), visible_slash_command_specs().len());
        assert!(all.items.iter().any(|item| item.insert == "/permissions"));
        assert!(all.items.iter().any(|item| item.insert == "/audit "));
        assert!(all.items.iter().any(|item| item.insert == "/btw "));
        assert!(!all.items.iter().any(|item| item.insert == "/provider"));
        assert!(
            all.items
                .iter()
                .any(|item| item.insert == "/feature-request ")
        );

        let filtered = slash_palette_for_buffer("/per", 0).expect("filtered palette");
        assert!(
            filtered
                .items
                .iter()
                .any(|item| item.insert == "/permissions")
        );
        assert_eq!(
            slash_palette_accept("/per", 0, false, true),
            Some(SlashPaletteAccept::Submit("/permissions".to_string()))
        );
        assert_eq!(
            slash_palette_accept("/per", 0, false, false),
            Some(SlashPaletteAccept::Insert("/permissions".to_string()))
        );

        let provider_args = slash_palette_for_buffer("/provider ", 1).expect("provider args");
        assert_eq!(provider_args.mode, SlashPaletteMode::Arguments);
        assert!(
            provider_args
                .items
                .iter()
                .any(|item| item.insert == "/provider validate")
        );
        assert_eq!(
            slash_palette_accept("/provider v", 0, false, true),
            Some(SlashPaletteAccept::Submit("/provider validate".to_string()))
        );
        assert_eq!(
            slash_palette_accept("/provider v", 0, false, false),
            Some(SlashPaletteAccept::Insert("/provider validate".to_string()))
        );
        assert_eq!(
            slash_palette_accept("/provider validate", 0, false, true),
            None
        );
        assert_eq!(
            slash_palette_accept("/effor", 0, false, false),
            Some(SlashPaletteAccept::Insert("/effort".to_string()))
        );
        assert_eq!(slash_palette_accept("/effort", 0, false, true), None);
        assert!(
            slash_palette_for_buffer("/auth", 0)
                .expect("fuzzy auth search")
                .items
                .iter()
                .any(|item| item.insert == "/login")
        );

        let help = slash_command_help_text();
        assert!(help.contains("/login [subscription|api]"));
        assert!(help.contains("/permissions [mode]"));
        assert!(!help.lines().any(|line| line.starts_with("- /commands")));
        assert!(help.contains("Tab completes"));
    }

    #[test]
    fn slash_palette_includes_pi_package_compatibility_commands() {
        let aliases = [
            ("/effort", "/effort"),
            ("/exit", "/exit"),
            ("/bug-report", "/bug-report"),
            ("/feature-request", "/feature-request"),
            ("/background", "/background"),
            ("/agents", "/agents"),
            ("/independent", "/independent"),
            ("/init", "/init"),
            ("/settings:oppi", "/settings"),
            ("/oppi-settings", "/settings"),
            ("/memory", "/memory"),
            ("/memory-maintenance", "/memory"),
            ("/idle-compact", "/memory"),
            ("/meridian", "/meridian"),
            ("/permissions", "/permissions"),
            ("/prompt-variant", "/prompt-variant"),
            ("/runtime-loop", "/runtime-loop"),
            ("/review", "/review"),
            ("/suggest-next", "/suggest-next"),
            ("/oppi-terminal-setup", "/keys"),
            ("/todos", "/todos"),
            ("/usage", "/usage"),
            ("/stats", "/usage"),
            ("/theme", "/theme"),
            ("/themes", "/theme"),
            ("/clear", "/new"),
            ("/reset", "/new"),
            ("/btw", "/btw"),
        ];
        for (alias, canonical) in aliases {
            let spec = find_slash_command_spec(alias).expect(alias);
            assert_eq!(
                spec.command, canonical,
                "{alias} should route to {canonical}"
            );
        }

        assert_eq!(
            find_slash_command_spec("/meridian")
                .expect("meridian command")
                .command,
            "/meridian"
        );
        assert_eq!(
            find_slash_command_spec("/runtime-loop")
                .expect("runtime-loop command")
                .command,
            "/runtime-loop"
        );

        let all = slash_palette_for_buffer("/", 0).expect("slash palette should open");
        assert!(all.items.iter().any(|item| item.insert == "/meridian"));
        assert!(all.items.iter().any(|item| item.insert == "/runtime-loop"));

        let help = slash_command_help_text();
        assert!(help.contains("/meridian"));
        assert!(help.contains("/runtime-loop"));
        assert!(help.contains("/settings:oppi"));
        assert!(help.contains("/oppi-settings"));
        assert!(help.contains("/memory-maintenance"));
        assert!(help.contains("/idle-compact"));
        assert!(help.contains("/oppi-terminal-setup"));
        assert!(help.contains("/btw <question>"));
    }

    #[test]
    fn slash_palette_includes_goal_command_and_arguments() {
        let all = slash_palette_for_buffer("/", 0).expect("slash palette");
        assert!(all.items.iter().any(|item| item.label.starts_with("/goal")));

        let goal_args = slash_palette_for_buffer("/goal ", 0).expect("goal args");
        assert!(
            goal_args
                .items
                .iter()
                .any(|item| item.insert == "/goal pause")
        );
        assert!(
            goal_args
                .items
                .iter()
                .any(|item| item.insert == "/goal resume")
        );
        assert!(
            goal_args
                .items
                .iter()
                .any(|item| item.insert == "/goal clear")
        );
    }

    #[test]
    fn goal_command_route_maps_actions_to_rpc_intent() {
        assert!(matches!(
            goal_command_route(" ").unwrap(),
            GoalCommandRoute::Get
        ));
        assert!(matches!(
            goal_command_route("clear").unwrap(),
            GoalCommandRoute::Clear
        ));
        assert_eq!(
            goal_command_route("pause").unwrap(),
            GoalCommandRoute::Set {
                objective: None,
                status: Some(ThreadGoalStatus::Paused),
                token_budget: GoalBudgetRoute::Unchanged
            }
        );
        assert_eq!(
            goal_command_route("budget 12500").unwrap(),
            GoalCommandRoute::Set {
                objective: None,
                status: None,
                token_budget: GoalBudgetRoute::Set(12_500)
            }
        );
        assert_eq!(
            goal_command_route("budget clear").unwrap(),
            GoalCommandRoute::Set {
                objective: None,
                status: None,
                token_budget: GoalBudgetRoute::Clear
            }
        );
        assert_eq!(
            goal_command_route("replace Ship native goal mode").unwrap(),
            GoalCommandRoute::Set {
                objective: Some("Ship native goal mode".to_string()),
                status: Some(ThreadGoalStatus::Active),
                token_budget: GoalBudgetRoute::Unchanged
            }
        );
        assert_eq!(
            goal_command_route("Ship native goal mode").unwrap(),
            GoalCommandRoute::CreateObjective("Ship native goal mode".to_string())
        );
    }

    #[test]
    fn rejects_protocol_mismatch_response() {
        let error = validate_initialize_response(&json!({
            "protocolVersion": "99.0.0",
            "minProtocolVersion": "99.0.0",
            "protocolCompatible": false
        }))
        .expect_err("mismatched protocol should fail");
        assert!(error.contains("incompatible"));
    }

    #[test]
    fn missing_server_reports_spawn_error() {
        let error = match RpcClient::spawn(PathBuf::from("definitely-missing-oppi-server-for-test"))
        {
            Ok(_) => panic!("missing server should fail"),
            Err(error) => error,
        };
        assert!(error.contains("spawn"));
    }

    #[test]
    fn permission_modes_map_to_turn_sandbox_policy() {
        let read_only = sandbox_policy_json(PermissionMode::ReadOnly, "/repo");
        assert_eq!(read_only["permissionProfile"]["mode"], json!("read-only"));
        assert_eq!(read_only["permissionProfile"]["writableRoots"], json!([]));
        assert_eq!(read_only["filesystem"], json!("readOnly"));
        assert_eq!(read_only["network"], json!("disabled"));

        let full = sandbox_policy_json(PermissionMode::FullAccess, "/repo");
        assert_eq!(full["permissionProfile"]["mode"], json!("full-access"));
        assert_eq!(full["filesystem"], json!("unrestricted"));
        assert_eq!(full["network"], json!("enabled"));
    }

    #[test]
    fn todo_dock_renders_only_active_todos() {
        let state = serde_json::from_value::<TodoState>(json!({
            "summary": "working",
            "todos": [
                { "id": "a", "content": "Active", "status": "in_progress" },
                { "id": "b", "content": "Done", "status": "completed" }
            ]
        }))
        .unwrap();
        let rendered = format_todos(&state);
        assert!(rendered.contains("a:InProgress"));
        assert!(!rendered.contains("b:"));
        assert!(rendered.contains("summary: working"));
    }

    #[test]
    fn parses_todos_client_actions() {
        assert_eq!(parse_todos_command_args(&[]).unwrap(), TodoCommand::List);
        assert_eq!(
            parse_todos_command_args(&["clear"]).unwrap(),
            TodoCommand::ClientAction {
                action: TodoClientAction::Clear,
                id: None,
            }
        );
        assert_eq!(
            parse_todos_command_args(&["done"]).unwrap(),
            TodoCommand::ClientAction {
                action: TodoClientAction::Done,
                id: None,
            }
        );
        assert_eq!(
            parse_todos_command_args(&["done", "impl"]).unwrap(),
            TodoCommand::ClientAction {
                action: TodoClientAction::Done,
                id: Some("impl".to_string()),
            }
        );
        assert_eq!(
            parse_todos_command_args(&["complete", "verify"]).unwrap(),
            TodoCommand::ClientAction {
                action: TodoClientAction::Done,
                id: Some("verify".to_string()),
            }
        );
        assert!(parse_todos_command_args(&["wat"]).is_err());
    }

    #[test]
    fn agent_markdown_round_trips_native_definition() {
        let markdown = r#"---
name: repo-helper
description: "Helps with repo maintenance"
tools: read_file, write_file
model: gpt-5.3-codex
effort: high
permissionMode: auto-review
background: true
worktreeRoot: .worktrees/repo-helper
---

You are a careful repo helper.
"#;
        let agent = parse_agent_markdown(markdown, AgentSource::Project).unwrap();
        assert_eq!(agent.name, "repo-helper");
        assert_eq!(agent.description, "Helps with repo maintenance");
        assert_eq!(agent.tools, vec!["read_file", "write_file"]);
        assert_eq!(agent.model.as_deref(), Some("gpt-5.3-codex"));
        assert_eq!(agent.effort.as_deref(), Some("high"));
        assert_eq!(agent.permission_mode, Some(PermissionMode::AutoReview));
        assert!(agent.background);
        assert_eq!(
            agent.worktree_root.as_deref(),
            Some(".worktrees/repo-helper")
        );
        assert_eq!(agent.instructions, "You are a careful repo helper.");

        let exported = format_agent_markdown(&agent);
        assert!(exported.contains("name: repo-helper"));
        assert!(exported.contains("description: \"Helps with repo maintenance\""));
        assert!(exported.contains("tools: read_file, write_file"));
        assert!(exported.contains("permissionMode: auto-review"));
        assert!(exported.contains("worktreeRoot: .worktrees/repo-helper"));
        assert!(exported.contains("You are a careful repo helper."));
    }

    #[test]
    fn agent_markdown_targets_project_and_user_locations() {
        let cwd = Path::new("C:/repo");
        let agent_dir = Path::new("C:/oppi-agent");
        assert_eq!(
            default_agent_markdown_path(
                cwd,
                agent_dir,
                AgentMarkdownTarget::Project,
                "Repo Helper"
            )
            .unwrap(),
            PathBuf::from("C:/repo/.oppi/agents/repo-helper.md")
        );
        assert_eq!(
            default_agent_markdown_path(cwd, agent_dir, AgentMarkdownTarget::User, "Repo Helper")
                .unwrap(),
            PathBuf::from("C:/oppi-agent/oppi/agents/repo-helper.md")
        );
        assert!(
            default_agent_markdown_path(cwd, agent_dir, AgentMarkdownTarget::Project, "!!!")
                .is_err()
        );
    }

    #[test]
    fn parses_agent_markdown_import_export_routes() {
        let cwd = Path::new("C:/repo");
        let agent_dir = Path::new("C:/oppi-agent");
        let imported = parse_agent_import_route(&["user", "agents/repo-helper.md"], cwd).unwrap();
        assert_eq!(imported.source, AgentSource::User);
        assert_eq!(
            imported.path,
            PathBuf::from("C:/repo/agents/repo-helper.md")
        );

        let exported = parse_agent_export_route(&["repo-helper", "user"], cwd, agent_dir).unwrap();
        assert_eq!(exported.agent_name, "repo-helper");
        assert_eq!(
            exported.path,
            PathBuf::from("C:/oppi-agent/oppi/agents/repo-helper.md")
        );

        let explicit =
            parse_agent_export_route(&["repo-helper", "exports/helper.md"], cwd, agent_dir)
                .unwrap();
        assert_eq!(explicit.path, PathBuf::from("C:/repo/exports/helper.md"));
    }

    #[test]
    fn finds_active_agent_from_list_for_markdown_export() {
        let value = json!({
            "items": [
                {
                    "active": {
                        "name": "repo-helper",
                        "description": "Helps with repo maintenance",
                        "source": "project",
                        "instructions": "You are careful."
                    },
                    "shadowed": []
                }
            ]
        });
        let agent = active_agent_from_list(value, "repo-helper").unwrap();
        assert_eq!(agent.name, "repo-helper");
        assert_eq!(agent.source, Some(AgentSource::Project));

        let missing = active_agent_from_list(json!({ "items": [] }), "missing").unwrap_err();
        assert!(missing.contains("unknown agent missing"));
    }

    #[test]
    fn renderer_handles_markdown_background_and_artifacts() {
        let rendered = render_markdown("# Title\n- one\n```diff\n+ add\n- del\n```", "plain");
        assert!(rendered.contains("# Title"));
        assert!(rendered.contains("• one"));
        assert!(rendered.contains("+ add"));

        let background = format_background_read(&json!({
            "task": { "id": "task-1", "status": "completed", "outputBytes": 4, "outputPath": "out.log" },
            "output": "done",
            "outputBytes": 4,
            "maxBytes": 30000
        }));
        assert!(background.contains("task-1"));
        assert!(background.contains("done"));
        assert!(background.contains("out.log"));

        let now = current_time_ms().unwrap_or(2_000);
        let background_list = json!({
            "items": [
                { "id": "old", "status": "completed", "command": "echo old", "cwd": "/repo", "outputPath": "old.log", "startedAtMs": now.saturating_sub(5_000), "finishedAtMs": now.saturating_sub(3_000), "exitCode": 0, "outputBytes": 3 },
                { "id": "new", "status": "running", "command": "echo new", "cwd": "/repo", "outputPath": "new.log", "startedAtMs": now.saturating_sub(1_000), "outputBytes": 7 }
            ]
        });
        let rendered_list = format_background_list(&background_list);
        assert!(rendered_list.contains("1 running"));
        assert!(rendered_list.contains("1 completed"));
        assert!(rendered_list.contains("actions: /background read"));
        assert_eq!(
            select_background_task_id(&background_list, false).as_deref(),
            Some("new")
        );
        assert_eq!(
            select_background_task_id(&background_list, true).as_deref(),
            Some("new")
        );
        assert!(background_summary_from_list(&background_list).contains("latest=new"));

        assert_eq!(
            artifact_hint(r#"{"outputPath":"output/image.png"}"#).as_deref(),
            Some("output/image.png")
        );

        let artifact_line = format_artifact_created(
            &ArtifactMetadata {
                id: "artifact-image-1".to_string(),
                tool_call_id: "image-1".to_string(),
                output_path: "output/image.png".to_string(),
                mime_type: Some("image/png".to_string()),
                width: Some(64),
                height: Some(32),
                source_images: vec!["input.png".to_string()],
                mask: Some("mask.png".to_string()),
                backend: Some("openai-images".to_string()),
                model: Some("gpt-image-2".to_string()),
                bytes: Some(1024),
                diagnostics: Vec::new(),
            },
            theme_tokens("plain"),
        );
        assert!(artifact_line.contains("output/image.png"));
        assert!(artifact_line.contains("64x32"));
        assert!(artifact_line.contains("backend=openai-images"));
        assert!(artifact_line.contains("sources: input.png"));
        assert!(artifact_line.contains("mask.png"));
    }

    #[test]
    fn retained_tui_keeps_scrollback_docks_and_dirty_components() {
        let mut ui = RetainedTui::new(3);
        ui.push_transcript("one");
        ui.push_transcript("two");
        ui.push_transcript("three");
        ui.push_transcript("four");
        ui.push_transcript(EXIT_COMMAND_ECHO_TEXT);
        ui.push_transcript(EXIT_REQUESTED_TEXT);
        assert_eq!(ui.scrollback.len(), 3);
        assert!(!ui.scrollback.iter().any(|line| line == "one"));
        assert!(ui.dirty_components().contains(&"transcript"));
        ui.clear_dirty();

        ui.update_docks(DockState {
            todos: vec!["50-12:InProgress".to_string()],
            approval: Some("approval-1 pending — /approve or /deny".to_string()),
            question: None,
            background: Some("1 running; latest=task-1".to_string()),
            suggestion: Some("ship it".to_string()),
            footer: "status: role=executor model=mock permissions=auto-review memory=client-hosted todos=1 queued=0 diagnostics=line-mode/raw-deferred variant=off theme=plain thread=thread-1".to_string(),
        });
        let dirty = ui.dirty_components();
        assert!(dirty.contains(&"todos"));
        assert!(dirty.contains(&"approval"));
        assert!(dirty.contains(&"background"));
        assert!(dirty.contains(&"footer"));

        let frame = ui.render_frame(72, 12);
        assert!(frame.contains("OPPi retained scrollback"));
        assert!(frame.contains("four"));
        assert!(frame.contains(EXIT_COMMAND_ECHO_TEXT));
        assert!(frame.contains(EXIT_REQUESTED_TEXT));
        assert!(frame.contains("50-12:InProgress"));
        assert!(frame.contains("line-mode fallback"));
        assert!(!frame.contains("one"));
        for line in frame.lines() {
            assert!(line.chars().count() <= 72, "too wide: {line}");
        }
    }

    #[test]
    fn retained_tui_redacts_credentials_before_render_storage() {
        let mut ui = RetainedTui::new(4);
        ui.push_transcript("OPENAI_API_KEY=sk-secret Bearer sk-token api_key=raw");
        let line = ui.scrollback.back().expect("redacted line");
        assert!(!line.contains("sk-secret"));
        assert!(!line.contains("sk-token"));
        assert!(!line.contains("api_key=raw"));
        assert!(line.contains("[REDACTED]"));
    }

    #[test]
    fn retained_tui_preserves_typed_event_metadata_for_ratatui() {
        let mut ui = RetainedTui::new(8);
        let call = ToolCall {
            id: "tool-1".to_string(),
            name: "write_file".to_string(),
            namespace: Some("oppi".to_string()),
            arguments: json!({"path":"docs/out.md"}),
        };
        let mut calls = BTreeMap::new();
        calls.insert(call.id.clone(), call.clone());
        let tool_event = Event {
            id: 41,
            thread_id: "thread-1".to_string(),
            turn_id: Some("turn-1".to_string()),
            kind: EventKind::ToolCallStarted { call },
        };
        ui.push_event_transcript(&tool_event, "tool started".to_string(), &calls);
        let artifact_event = Event {
            id: 42,
            thread_id: "thread-1".to_string(),
            turn_id: Some("turn-1".to_string()),
            kind: EventKind::ArtifactCreated {
                artifact: ArtifactMetadata {
                    id: "artifact-1".to_string(),
                    tool_call_id: "tool-1".to_string(),
                    output_path: "docs/out.md".to_string(),
                    mime_type: Some("text/markdown".to_string()),
                    width: None,
                    height: None,
                    source_images: Vec::new(),
                    mask: None,
                    backend: None,
                    model: None,
                    bytes: Some(2048),
                    diagnostics: Vec::new(),
                },
            },
        };
        ui.push_event_transcript(&artifact_event, "artifact created".to_string(), &calls);

        assert_eq!(ui.typed_scrollback.len(), 2);
        assert_eq!(ui.typed_scrollback[0].kind, TranscriptEntryKind::ToolWrite);
        assert_eq!(
            ui.typed_scrollback[0].tool_call_id.as_deref(),
            Some("tool-1")
        );
        assert_eq!(ui.typed_scrollback[1].kind, TranscriptEntryKind::Artifact);
        assert_eq!(ui.typed_scrollback[1].event_id, 42);
        assert_eq!(
            ui.typed_scrollback[1].artifact_id.as_deref(),
            Some("artifact-1")
        );
        assert!(
            ui.typed_scrollback[1]
                .body
                .contains("artifact://docs/out.md")
        );
        assert!(ui.typed_scrollback[1].body.contains("text/markdown"));
        assert!(ui.typed_scrollback[1].body.contains("2.0KiB"));
    }

    fn sample_retained_ui_snapshot() -> RetainedTui {
        let mut ui = RetainedTui::new(20);
        ui.push_transcript("assistant:\n# Plan\n  • one\n```diff\n+ add\n- del\n```");
        ui.push_transcript("tool oppi:write_file started (write-1)");
        ui.push_transcript(
            "tool oppi:write_file → ok\n{\"outputPath\":\"docs/native-shell-dogfood.md\"}\nartifact: docs/native-shell-dogfood.md",
        );
        ui.push_transcript(format_artifact_created(
            &ArtifactMetadata {
                id: "artifact-image-1".to_string(),
                tool_call_id: "image-1".to_string(),
                output_path: "output/image.png".to_string(),
                mime_type: Some("image/png".to_string()),
                width: Some(64),
                height: Some(64),
                source_images: Vec::new(),
                mask: None,
                backend: Some("openai-images".to_string()),
                model: Some("gpt-image-2".to_string()),
                bytes: Some(1024),
                diagnostics: Vec::new(),
            },
            theme_tokens("plain"),
        ));
        ui.update_docks(DockState {
            todos: vec!["50-14:InProgress".to_string(), "50-15:Pending".to_string()],
            approval: Some("approval-1 pending for write-1 — /approve or /deny".to_string()),
            question: Some("Choose path — /answer <text-or-option-id>".to_string()),
            background: Some("1 running, 1 completed; latest=task-2".to_string()),
            suggestion: Some("Run snapshots".to_string()),
            footer: "status: role=executor model=mock-scripted permissions=auto-review memory=client-hosted todos=2 queued=1 diagnostics=line-mode/raw-deferred variant=off theme=plain thread=thread-1".to_string(),
        });
        ui
    }

    fn assert_or_update_snapshot(name: &str, actual: &str) {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("snapshots")
            .join(name);
        let actual_with_newline = if actual.ends_with('\n') {
            actual.to_string()
        } else {
            format!("{actual}\n")
        };
        let actual = normalize_snapshot_newlines(&actual_with_newline);
        if env::var("OPPI_UPDATE_SNAPSHOTS").ok().as_deref() == Some("1") {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, actual).unwrap();
            return;
        }
        let expected = normalize_snapshot_newlines(
            &fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("read snapshot {}: {error}", path.display())),
        );
        assert_eq!(actual, expected, "snapshot {} changed", path.display());
    }

    fn normalize_snapshot_newlines(value: &str) -> String {
        value.replace("\r\n", "\n").replace('\r', "\n")
    }

    fn theme_palette_snapshot() -> String {
        ["plain", "light", "dark", "oppi"]
            .iter()
            .map(|theme| {
                let tokens = if *theme == "plain" {
                    plain_theme_tokens()
                } else {
                    ansi_theme_tokens(theme)
                };
                let sample = format!(
                    "{}# Title{} | {}code{} | {}ok{} | {}warn{} | {}err{}",
                    tokens.accent,
                    tokens.reset,
                    tokens.code,
                    tokens.reset,
                    tokens.success,
                    tokens.reset,
                    tokens.warning,
                    tokens.reset,
                    tokens.error,
                    tokens.reset
                );
                format!(
                    "{theme}: accent={:?} dim={:?} code={:?} success={:?} warning={:?} error={:?} sample={:?}",
                    tokens.accent,
                    tokens.dim,
                    tokens.code,
                    tokens.success,
                    tokens.warning,
                    tokens.error,
                    sample
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn ui_snapshots_cover_widths_and_themes() {
        let ui = sample_retained_ui_snapshot();
        assert_or_update_snapshot("ui-frame-44.snap", &ui.render_frame(44, 18));
        assert_or_update_snapshot("ui-frame-72.snap", &ui.render_frame(72, 18));
        assert_or_update_snapshot("ui-frame-110.snap", &ui.render_frame(110, 18));
        assert_or_update_snapshot("theme-palette.snap", &theme_palette_snapshot());
    }

    #[test]
    fn thread_tree_marks_active_and_forks() {
        let project = oppi_protocol::ProjectRef {
            id: "project".to_string(),
            cwd: "/repo".to_string(),
            display_name: None,
            workspace_roots: Vec::new(),
        };
        let threads = vec![
            Thread {
                id: "thread-1".to_string(),
                project: project.clone(),
                status: oppi_protocol::ThreadStatus::Active,
                title: Some("Root".to_string()),
                forked_from: None,
            },
            Thread {
                id: "thread-2".to_string(),
                project,
                status: oppi_protocol::ThreadStatus::Active,
                title: Some("Branch".to_string()),
                forked_from: Some("thread-1".to_string()),
            },
        ];
        let rendered = format_thread_tree(&threads, "thread-2");
        assert!(rendered.contains("thread-1"));
        assert!(rendered.contains("* thread-2"));
        assert!(rendered.contains("forkedFrom=thread-1"));
    }

    #[test]
    fn thread_tree_can_fold_subtrees_and_marks_archived_sessions() {
        let project = oppi_protocol::ProjectRef {
            id: "project".to_string(),
            cwd: "/repo".to_string(),
            display_name: None,
            workspace_roots: Vec::new(),
        };
        let threads = vec![
            Thread {
                id: "thread-1".to_string(),
                project: project.clone(),
                status: oppi_protocol::ThreadStatus::Active,
                title: Some("Root".to_string()),
                forked_from: None,
            },
            Thread {
                id: "thread-2".to_string(),
                project: project.clone(),
                status: oppi_protocol::ThreadStatus::Archived,
                title: Some("Old branch".to_string()),
                forked_from: Some("thread-1".to_string()),
            },
            Thread {
                id: "thread-3".to_string(),
                project,
                status: oppi_protocol::ThreadStatus::Active,
                title: Some("Leaf".to_string()),
                forked_from: Some("thread-2".to_string()),
            },
        ];
        let open = format_thread_tree(&threads, "thread-1");
        assert!(open.contains("thread-2"));
        assert!(open.contains("[archived]"));

        let folded = BTreeSet::from(["thread-1".to_string()]);
        let rendered = format_thread_tree_with_folded(&threads, "thread-1", &folded);
        assert!(rendered.contains("thread-1 [+1]"));
        assert!(!rendered.contains("thread-2"));
    }

    #[test]
    fn exit_resume_command_is_the_last_exit_instruction() {
        let output = format!(
            "{EXIT_REQUESTED_TEXT}\n{}",
            exit_resume_command("019e0965-e833-7093-88ea-79a2baf0fc48")
        );
        assert_eq!(
            output.lines().last(),
            Some("oppi resume 019e0965-e833-7093-88ea-79a2baf0fc48")
        );
    }

    #[test]
    fn bare_resume_lists_current_project_sessions() {
        let current_project = oppi_protocol::ProjectRef {
            id: "project".to_string(),
            cwd: "/repo".to_string(),
            display_name: None,
            workspace_roots: Vec::new(),
        };
        let other_project = oppi_protocol::ProjectRef {
            id: "other".to_string(),
            cwd: "/other".to_string(),
            display_name: None,
            workspace_roots: Vec::new(),
        };
        let threads = vec![
            Thread {
                id: "thread-1".to_string(),
                project: current_project.clone(),
                status: oppi_protocol::ThreadStatus::Active,
                title: Some("Root".to_string()),
                forked_from: None,
            },
            Thread {
                id: "thread-2".to_string(),
                project: current_project,
                status: oppi_protocol::ThreadStatus::Active,
                title: Some("Branch".to_string()),
                forked_from: Some("thread-1".to_string()),
            },
            Thread {
                id: "thread-3".to_string(),
                project: other_project,
                status: oppi_protocol::ThreadStatus::Active,
                title: Some("Elsewhere".to_string()),
                forked_from: None,
            },
        ];
        let rendered = format_resume_session_list(&threads, "thread-2", "/repo", &BTreeSet::new());
        assert!(rendered.contains("current thread: thread-2"));
        assert!(rendered.contains("recent sessions in this project:"));
        assert!(rendered.contains("thread-1"));
        assert!(rendered.contains("* thread-2"));
        assert!(!rendered.contains("thread-3"));
        assert!(rendered.contains("oppi resume <thread-id>"));
    }

    #[test]
    fn role_profiles_persist_to_settings_json() {
        let root = env::temp_dir().join(format!("oppi-role-profiles-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let path = root.join("settings.json");
        fs::write(
            &path,
            r#"{"oppi":{"promptVariant":"a","roleModels":{"reviewer":"old-review"}},"kept":true}"#,
        )
        .unwrap();

        let mut role_models = BTreeMap::new();
        role_models.insert("reviewer".to_string(), "gpt-review".to_string());
        role_models.insert("executor".to_string(), "gpt-exec".to_string());
        save_role_profiles(&path, &role_models).unwrap();
        save_permission_mode_setting(&path, PermissionMode::ReadOnly).unwrap();
        save_reasoning_effort_setting(&path, ThinkingLevel::High).unwrap();
        save_prompt_variant_setting(&path, "caveman").unwrap();

        let loaded = load_role_profiles(&path);
        assert_eq!(
            loaded.get("reviewer").map(String::as_str),
            Some("gpt-review")
        );
        assert_eq!(loaded.get("executor").map(String::as_str), Some("gpt-exec"));
        let raw: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(raw["oppi"]["promptVariant"], json!("caveman"));
        assert_eq!(raw["oppi"]["permissionMode"], json!("read-only"));
        assert_eq!(raw["oppi"]["reasoningEffort"], json!("high"));
        assert_eq!(load_prompt_variant_setting(&path), "caveman");
        assert_eq!(
            load_permission_mode_setting(&path).0,
            PermissionMode::ReadOnly
        );
        assert_eq!(
            load_reasoning_effort_setting(&path),
            Some(ThinkingLevel::High)
        );
        assert_eq!(raw["kept"], json!(true));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn role_profiles_route_openai_provider_models() {
        let mut role_models = BTreeMap::new();
        role_models.insert("reviewer".to_string(), "gpt-review".to_string());
        let provider = ProviderConfig::OpenAiCompatible(OpenAiCompatibleConfig {
            flavor: DirectProviderFlavor::OpenAiCompatible,
            model: "gpt-exec".to_string(),
            base_url: None,
            api_key_env: Some("OPPI_OPENAI_API_KEY".to_string()),
            system_prompt: None,
            temperature: None,
            reasoning_effort: None,
            max_output_tokens: None,
            stream: true,
        });
        match provider_for_role_config(&provider, &role_models, Some("reviewer")) {
            ProviderConfig::OpenAiCompatible(config) => assert_eq!(config.model, "gpt-review"),
            ProviderConfig::Mock => panic!("expected openai-compatible provider"),
        }
        match provider_for_role_config(&provider, &role_models, Some("executor")) {
            ProviderConfig::OpenAiCompatible(config) => assert_eq!(config.model, "gpt-exec"),
            ProviderConfig::Mock => panic!("expected openai-compatible provider"),
        }
        assert_eq!(role_for_command("/review"), "reviewer");
        assert_eq!(role_for_command("/independent"), "orchestrator");
    }

    #[test]
    fn default_role_models_use_gpt_for_main_and_codex_for_coding_subagents() {
        let role_models = BTreeMap::new();
        let provider = ProviderConfig::OpenAiCompatible(OpenAiCompatibleConfig {
            flavor: DirectProviderFlavor::OpenAiCodex,
            model: OPENAI_CODEX_DEFAULT_MODEL.to_string(),
            base_url: None,
            api_key_env: None,
            system_prompt: None,
            temperature: None,
            reasoning_effort: Some("xhigh".to_string()),
            max_output_tokens: None,
            stream: true,
        });

        match provider_for_role_config(&provider, &role_models, Some("executor")) {
            ProviderConfig::OpenAiCompatible(config) => {
                assert_eq!(config.model, GPT_MAIN_DEFAULT_MODEL);
                assert_eq!(config.reasoning_effort.as_deref(), Some("xhigh"));
            }
            ProviderConfig::Mock => panic!("expected openai-compatible provider"),
        }
        match provider_for_role_config(&provider, &role_models, Some("subagent")) {
            ProviderConfig::OpenAiCompatible(config) => {
                assert_eq!(config.model, GPT_CODING_SUBAGENT_DEFAULT_MODEL);
                assert_eq!(config.reasoning_effort.as_deref(), Some("high"));
            }
            ProviderConfig::Mock => panic!("expected openai-compatible provider"),
        }
        match provider_for_role_config_with_complexity(
            &provider,
            &role_models,
            Some("subagent"),
            true,
        ) {
            ProviderConfig::OpenAiCompatible(config) => {
                assert_eq!(config.model, GPT_MAIN_DEFAULT_MODEL);
                assert_eq!(config.reasoning_effort.as_deref(), Some("xhigh"));
            }
            ProviderConfig::Mock => panic!("expected openai-compatible provider"),
        }
    }

    #[test]
    fn default_role_models_use_opus_and_sonnet_for_claude_only_provider() {
        let role_models = BTreeMap::new();
        let provider = ProviderConfig::OpenAiCompatible(meridian_provider_config(None));

        match provider_for_role_config(&provider, &role_models, Some("executor")) {
            ProviderConfig::OpenAiCompatible(config) => {
                assert_eq!(config.model, CLAUDE_MAIN_DEFAULT_MODEL);
                assert_eq!(config.reasoning_effort.as_deref(), Some("high"));
            }
            ProviderConfig::Mock => panic!("expected openai-compatible provider"),
        }
        match provider_for_role_config(&provider, &role_models, Some("subagent")) {
            ProviderConfig::OpenAiCompatible(config) => {
                assert_eq!(config.model, CLAUDE_CODING_SUBAGENT_DEFAULT_MODEL);
                assert_eq!(config.reasoning_effort.as_deref(), Some("high"));
            }
            ProviderConfig::Mock => panic!("expected openai-compatible provider"),
        }
    }

    #[test]
    fn authenticated_default_provider_prefers_gpt_subscription_over_claude_bridge() {
        let copilot = GitHubCopilotStoredAuth {
            access_token: "proxy-ep=proxy.individual.githubcopilot.com".to_string(),
            refresh_token: "refresh".to_string(),
            expires: 1,
            enterprise_domain: None,
        };

        match default_authenticated_provider_config_from_state(true, Some(&copilot), true)
            .expect("codex default")
        {
            ProviderConfig::OpenAiCompatible(config) => {
                assert_eq!(config.flavor, DirectProviderFlavor::OpenAiCodex);
                assert_eq!(config.model, GPT_MAIN_DEFAULT_MODEL);
                assert_eq!(config.reasoning_effort.as_deref(), Some("xhigh"));
            }
            ProviderConfig::Mock => panic!("expected openai-compatible provider"),
        }

        match default_authenticated_provider_config_from_state(false, Some(&copilot), true)
            .expect("copilot default")
        {
            ProviderConfig::OpenAiCompatible(config) => {
                assert_eq!(config.flavor, DirectProviderFlavor::GitHubCopilot);
                assert_eq!(config.model, GPT_MAIN_DEFAULT_MODEL);
                assert_eq!(config.reasoning_effort.as_deref(), Some("xhigh"));
            }
            ProviderConfig::Mock => panic!("expected openai-compatible provider"),
        }

        match default_authenticated_provider_config_from_state(false, None, true)
            .expect("claude-only default")
        {
            ProviderConfig::OpenAiCompatible(config) => {
                assert_eq!(config.model, CLAUDE_MAIN_DEFAULT_MODEL);
                assert_eq!(config.reasoning_effort.as_deref(), Some("high"));
            }
            ProviderConfig::Mock => panic!("expected openai-compatible provider"),
        }
    }

    #[test]
    fn codex_oauth_persist_uses_pi_auth_lock_and_preserves_other_credentials() {
        let root = env::temp_dir().join(format!("oppi-codex-auth-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let path = root.join("auth.json");
        fs::write(
            &path,
            r#"{
  "other-provider": { "type": "api_key", "key": "keep-me" }
}
"#,
        )
        .unwrap();

        persist_codex_oauth(
            &path,
            json!({
                "access_token": fake_codex_access_token("acct_test"),
                "refresh_token": "refresh_test",
                "expires_in": 3600,
            }),
        )
        .unwrap();

        let stored = read_json_or_empty(&path).unwrap();
        assert_eq!(stored["other-provider"]["key"], json!("keep-me"));
        assert_eq!(stored[OPENAI_CODEX_PROVIDER_ID]["type"], json!("oauth"));
        assert_eq!(
            stored[OPENAI_CODEX_PROVIDER_ID]["refresh"],
            json!("refresh_test")
        );
        assert_eq!(
            stored[OPENAI_CODEX_PROVIDER_ID]["accountId"],
            json!("acct_test")
        );
        assert!(!auth_lock_path(&path).exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn copilot_oauth_persist_uses_pi_auth_store_and_derives_base_url() {
        let root = env::temp_dir().join(format!("oppi-copilot-auth-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let path = root.join("auth.json");
        fs::write(&path, r#"{"other":{"type":"api_key","key":"keep"}}"#).unwrap();

        persist_github_copilot_oauth(
            &path,
            "github-refresh-token",
            Some("ghe.example.com"),
            json!({
                "token": "tid=1;proxy-ep=proxy.enterprise.githubcopilot.com;exp=4102444800;",
                "expires_at": 4_102_444_800i64,
            }),
        )
        .unwrap();

        let stored = read_json_or_empty(&path).unwrap();
        assert_eq!(stored["other"]["key"], json!("keep"));
        assert_eq!(stored[GITHUB_COPILOT_PROVIDER_ID]["type"], json!("oauth"));
        assert_eq!(
            stored[GITHUB_COPILOT_PROVIDER_ID]["refresh"],
            json!("github-refresh-token")
        );
        assert_eq!(
            stored[GITHUB_COPILOT_PROVIDER_ID]["enterpriseUrl"],
            json!("ghe.example.com")
        );
        let auth = read_github_copilot_auth_at(&path).unwrap();
        assert_eq!(
            github_copilot_base_url(&auth),
            "https://api.enterprise.githubcopilot.com"
        );
        assert_eq!(
            normalize_github_enterprise_domain("https://GHE.EXAMPLE.com/org"),
            Some("ghe.example.com".to_string())
        );
        assert!(!auth_lock_path(&path).exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn suggestion_command_formatters_show_debug_without_secrets() {
        let suggestion = SuggestedNextMessage {
            message: "run the smoke test".to_string(),
            confidence: 0.82,
            reason: Some("short likely next step".to_string()),
        };
        let summary = format_suggestion_summary(&suggestion);
        assert!(summary.contains("run the smoke test"));
        assert!(summary.contains("82%"));
        let debug = format_suggestion_debug(&suggestion);
        assert!(debug.contains("confidence: 0.82"));
        assert!(debug.contains("short likely next step"));
    }

    #[test]
    fn effort_levels_are_model_dependent_like_pi_oppi() {
        let gpt54 = ProviderConfig::OpenAiCompatible(OpenAiCompatibleConfig {
            flavor: DirectProviderFlavor::OpenAiCodex,
            model: "gpt-5.4".to_string(),
            base_url: None,
            api_key_env: None,
            system_prompt: None,
            temperature: None,
            reasoning_effort: Some("medium".to_string()),
            max_output_tokens: None,
            stream: true,
        });
        let allowed = allowed_effort_levels_for_provider(&gpt54);
        assert_eq!(allowed.first(), Some(&ThinkingLevel::Off));
        assert_eq!(allowed.last(), Some(&ThinkingLevel::XHigh));
        assert_eq!(
            recommended_effort_level_for_provider(&gpt54),
            ThinkingLevel::High
        );
        assert!(format_effort_status(&gpt54).contains("recommended: High"));

        let gpt41 = ProviderConfig::OpenAiCompatible(OpenAiCompatibleConfig {
            flavor: DirectProviderFlavor::OpenAiCompatible,
            model: "gpt-4.1".to_string(),
            base_url: None,
            api_key_env: Some("OPPI_OPENAI_API_KEY".to_string()),
            system_prompt: None,
            temperature: None,
            reasoning_effort: None,
            max_output_tokens: None,
            stream: true,
        });
        assert_eq!(
            allowed_effort_levels_for_provider(&gpt41),
            vec![ThinkingLevel::Off]
        );
        assert_eq!(
            recommended_effort_level_for_provider(&gpt41),
            ThinkingLevel::Off
        );
        assert!(format_effort_status(&gpt41).contains("locked to Off"));

        let claude = ProviderConfig::OpenAiCompatible(OpenAiCompatibleConfig {
            flavor: DirectProviderFlavor::OpenAiCompatible,
            model: "claude-opus-4.7".to_string(),
            base_url: Some("http://127.0.0.1:3456".to_string()),
            api_key_env: Some(MERIDIAN_API_KEY_ENV.to_string()),
            system_prompt: None,
            temperature: None,
            reasoning_effort: Some("xhigh".to_string()),
            max_output_tokens: None,
            stream: true,
        });
        assert_eq!(
            effort_level_label_for_provider(&claude, ThinkingLevel::XHigh),
            "Max"
        );
        assert_eq!(normalize_thinking_level("max"), Some(ThinkingLevel::XHigh));
        assert_eq!(normalize_thinking_level("none"), Some(ThinkingLevel::Off));
    }

    #[test]
    fn effort_argument_suggestions_follow_model_limits() {
        let gpt51 = ProviderConfig::OpenAiCompatible(OpenAiCompatibleConfig {
            flavor: DirectProviderFlavor::OpenAiCodex,
            model: "gpt-5.1".to_string(),
            base_url: None,
            api_key_env: None,
            system_prompt: None,
            temperature: None,
            reasoning_effort: Some("medium".to_string()),
            max_output_tokens: None,
            stream: true,
        });
        let limited = effort_arg_suggestions(&gpt51, "");
        assert!(limited.iter().any(|item| item.insert == "/effort auto"));
        assert!(limited.iter().any(|item| item.insert == "/effort high"));
        assert!(!limited.iter().any(|item| item.insert == "/effort xhigh"));

        let gpt54 = ProviderConfig::OpenAiCompatible(OpenAiCompatibleConfig {
            flavor: DirectProviderFlavor::OpenAiCodex,
            model: "gpt-5.4".to_string(),
            base_url: None,
            api_key_env: None,
            system_prompt: None,
            temperature: None,
            reasoning_effort: Some("medium".to_string()),
            max_output_tokens: None,
            stream: true,
        });
        let full = effort_arg_suggestions(&gpt54, "x");
        assert_eq!(
            full.iter()
                .map(|item| item.insert.as_str())
                .collect::<Vec<_>>(),
            vec!["/effort xhigh"]
        );
    }

    #[test]
    fn native_model_catalog_tracks_current_pi_defaults_and_provider_order() {
        let codex = ProviderConfig::OpenAiCompatible(OpenAiCompatibleConfig {
            flavor: DirectProviderFlavor::OpenAiCodex,
            model: OPENAI_CODEX_DEFAULT_MODEL.to_string(),
            base_url: None,
            api_key_env: None,
            system_prompt: None,
            temperature: None,
            reasoning_effort: Some("medium".to_string()),
            max_output_tokens: None,
            stream: true,
        });
        let codex_catalog = native_model_catalog_for_provider(&codex);
        assert_eq!(
            codex_catalog.first().copied(),
            Some(OPENAI_CODEX_DEFAULT_MODEL)
        );
        assert!(codex_catalog.contains(&"gpt-5.4"));
        assert!(!matches!(codex_catalog.first(), Some(&"gpt-5.1-codex")));

        let copilot = ProviderConfig::OpenAiCompatible(OpenAiCompatibleConfig {
            flavor: DirectProviderFlavor::GitHubCopilot,
            model: GITHUB_COPILOT_DEFAULT_MODEL.to_string(),
            base_url: Some(GITHUB_COPILOT_DEFAULT_BASE_URL.to_string()),
            api_key_env: None,
            system_prompt: None,
            temperature: None,
            reasoning_effort: None,
            max_output_tokens: None,
            stream: true,
        });
        let copilot_catalog = native_model_catalog_for_provider(&copilot);
        assert_eq!(
            copilot_catalog.first().copied(),
            Some(GITHUB_COPILOT_DEFAULT_MODEL)
        );
        assert!(copilot_catalog.contains(&"claude-opus-4.7"));
    }

    #[test]
    fn scoped_model_scope_preserves_order_and_pi_settings_key() {
        let provider = ProviderConfig::OpenAiCompatible(OpenAiCompatibleConfig {
            flavor: DirectProviderFlavor::OpenAiCodex,
            model: OPENAI_CODEX_DEFAULT_MODEL.to_string(),
            base_url: None,
            api_key_env: None,
            system_prompt: None,
            temperature: None,
            reasoning_effort: Some("medium".to_string()),
            max_output_tokens: None,
            stream: true,
        });
        let patterns = vec!["openai-codex/gpt-5.4".to_string(), "*mini".to_string()];
        let scoped = scoped_model_ids_for_patterns(&patterns, &provider);
        assert_eq!(scoped[0], "gpt-5.4");
        assert!(scoped.iter().any(|model| model == "gpt-5.4-mini"));
        let ordered = main_model_ids_for_selection(Some("gpt-5.5"), &patterns, &provider);
        assert_eq!(ordered[0], "gpt-5.5");
        assert_eq!(ordered[1], "gpt-5.4");

        let root = env::temp_dir().join(format!("oppi-enabled-models-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let path = root.join("settings.json");
        fs::write(&path, r#"{"oppi":{"promptVariant":"a"},"kept":true}"#).unwrap();
        save_enabled_model_scope(&path, Some(patterns.as_slice())).unwrap();
        assert_eq!(load_enabled_model_scope(&path), patterns);
        let raw: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(raw["enabledModels"][0], json!("openai-codex/gpt-5.4"));
        assert_eq!(raw["oppi"]["promptVariant"], json!("a"));
        save_enabled_model_scope(&path, None).unwrap();
        let raw: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert!(raw.get("enabledModels").is_none());
        assert_eq!(raw["kept"], json!(true));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn provider_status_redacts_credentials_and_roles_filter_models() {
        let mut role_models = BTreeMap::new();
        role_models.insert("reviewer".to_string(), "gpt-review".to_string());
        let roles = format_role_profiles(&role_models, Some("gpt-main"));
        assert!(roles.contains("reviewer: gpt-review"));
        assert!(roles.contains("executor: gpt-main (inherit)"));

        let models = vec![
            ModelRef {
                id: "gpt-main".to_string(),
                provider: "openai-compatible".to_string(),
                display_name: "Main".to_string(),
                role: None,
            },
            ModelRef {
                id: "gpt-review".to_string(),
                provider: "openai-compatible".to_string(),
                display_name: "Reviewer".to_string(),
                role: Some("reviewer".to_string()),
            },
        ];
        let rendered = format_model_list(&models, Some("gpt-main"), "review", &role_models);
        assert!(rendered.contains("gpt-review"));
        assert!(!rendered.contains("gpt-main ["));

        assert!(is_safe_api_key_env_name("OPPI_OPENAI_API_KEY"));
        assert!(!is_safe_api_key_env_name("sk-secret"));
        assert_eq!(
            redacted_base_url_label("https://user:secret@example.com/v1"),
            "https://example.com"
        );
        let provider = ProviderConfig::OpenAiCompatible(OpenAiCompatibleConfig {
            flavor: DirectProviderFlavor::OpenAiCompatible,
            model: "gpt-main".to_string(),
            base_url: Some("https://user:secret@example.com/v1".to_string()),
            api_key_env: Some("OPPI_OPENAI_API_KEY".to_string()),
            system_prompt: None,
            temperature: None,
            reasoning_effort: Some("medium".to_string()),
            max_output_tokens: None,
            stream: true,
        });
        let validation = provider_validation_panel(&provider);
        assert!(validation.contains("liveCalls: none"));
        assert!(validation.contains("/login subscription claude"));
        assert!(!validation.contains("user:secret"));
        assert!(provider_policy_text().contains("/login as the primary"));
        assert!(provider_policy_text().contains("must not spawn Meridian"));
        assert!(anthropic_provider_evaluation().contains("managed Meridian bridge"));

        let meridian = meridian_provider_config(Some("claude-opus-4-6"));
        assert_eq!(meridian.model, "claude-opus-4-6");
        assert_eq!(meridian.api_key_env.as_deref(), Some(MERIDIAN_API_KEY_ENV));
        let loopback_meridian = OpenAiCompatibleConfig {
            base_url: Some("http://127.0.0.1:3456".to_string()),
            ..meridian.clone()
        };
        assert!(provider_auth_present(&loopback_meridian));
        assert!(login_root_picker_panel(&ProviderConfig::Mock).contains("Subscription"));
        assert!(login_subscription_picker_panel().contains("Codex (ChatGPT)"));
        assert!(login_subscription_picker_panel().contains("native browser OAuth"));
        let codex = OpenAiCompatibleConfig {
            flavor: DirectProviderFlavor::OpenAiCodex,
            model: OPENAI_CODEX_DEFAULT_MODEL.to_string(),
            base_url: None,
            api_key_env: None,
            system_prompt: None,
            temperature: None,
            reasoning_effort: Some("medium".to_string()),
            max_output_tokens: None,
            stream: true,
        };
        let codex_json = openai_provider_json(&codex);
        assert_eq!(codex_json["kind"], json!("openai-codex"));
        assert_eq!(
            provider_name(&ProviderConfig::OpenAiCompatible(codex)),
            "openai-codex"
        );
        let copilot = OpenAiCompatibleConfig {
            flavor: DirectProviderFlavor::GitHubCopilot,
            model: GITHUB_COPILOT_DEFAULT_MODEL.to_string(),
            base_url: Some(GITHUB_COPILOT_DEFAULT_BASE_URL.to_string()),
            api_key_env: None,
            system_prompt: None,
            temperature: None,
            reasoning_effort: None,
            max_output_tokens: None,
            stream: true,
        };
        assert_eq!(
            openai_provider_json(&copilot)["kind"],
            json!("github-copilot")
        );
        assert_eq!(
            provider_name(&ProviderConfig::OpenAiCompatible(copilot)),
            "github-copilot"
        );
        assert!(login_subscription_picker_panel().contains("device-code OAuth"));
        assert!(login_claude_picker_panel().contains("Claude Code login"));
        assert!(login_claude_picker_panel().contains("Model selection stays in `/model`"));
        assert!(login_meridian_install_approval_panel().contains("Approval required"));
        assert!(login_action_approved(&["--yes"]));
        assert!(!login_action_approved(&[]));
        assert!(meridian_status_panel().contains("no hidden npx"));
    }

    #[test]
    fn graphify_status_detects_artifacts_and_guidance() {
        let root = env::temp_dir().join(format!("oppi-graphify-status-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".graphify/wiki")).unwrap();
        fs::write(root.join(".graphify/GRAPH_REPORT.md"), "# Graph").unwrap();
        fs::write(root.join(".graphify/wiki/index.md"), "# Wiki").unwrap();
        fs::write(root.join(".graphify/graph.json"), "{}").unwrap();
        fs::write(root.join(".graphify/needs_update"), "").unwrap();
        fs::write(root.join("graphify.yaml"), "inputs:\n  scope: auto\n").unwrap();

        let status = graphify_status(&root.display().to_string());
        assert!(status.configured());
        assert!(status.needs_update);
        assert!(status.report_path.is_some());
        assert!(status.wiki_index_path.is_some());
        let rendered = format_graphify_status(&status);
        assert!(rendered.contains("Graphify codebase graph"));
        assert!(rendered.contains("needsUpdate: true"));
        assert!(graphify_install_guidance().contains("npm install -g graphifyy"));
        assert!(graphify_command_guidance().contains("graphify query"));
        let payload = graphify_status_json(&status);
        assert_eq!(payload["configured"], json!(true));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn prompt_variant_appends_to_provider_system_prompt() {
        let config = OpenAiCompatibleConfig {
            flavor: DirectProviderFlavor::OpenAiCompatible,
            model: "mock".to_string(),
            base_url: None,
            api_key_env: None,
            system_prompt: Some("Base system".to_string()),
            temperature: None,
            reasoning_effort: None,
            max_output_tokens: None,
            stream: true,
        };
        let mut provider = openai_provider_json(&config);
        apply_prompt_variant_to_provider(&mut provider, "caveman");
        assert!(
            provider["systemPrompt"]
                .as_str()
                .unwrap()
                .contains("Base system")
        );
        assert!(
            provider["systemPrompt"]
                .as_str()
                .unwrap()
                .contains("Caveman")
        );
        assert_eq!(
            normalize_prompt_variant("promptname_a").as_deref(),
            Some("a")
        );
    }

    #[test]
    fn feature_routing_appends_to_native_provider_without_duplicates() {
        let config = OpenAiCompatibleConfig {
            flavor: DirectProviderFlavor::OpenAiCompatible,
            model: "mock".to_string(),
            base_url: None,
            api_key_env: None,
            system_prompt: Some("Base system".to_string()),
            temperature: None,
            reasoning_effort: None,
            max_output_tokens: None,
            stream: true,
        };
        let mut provider = openai_provider_json(&config);
        apply_feature_routing_to_provider(&mut provider, "caveman");
        apply_feature_routing_to_provider(&mut provider, "caveman");

        let system = provider["systemPrompt"].as_str().unwrap();
        assert!(system.contains("Base system"));
        assert!(system.contains("OPPi feature routing"));
        assert!(system.contains("Use OPPi's extra capabilities"));
        assert!(system.contains("promptname_b"));
        assert_eq!(system.matches("OPPi feature routing:").count(), 1);
        assert_eq!(system.matches("OPPi feature routing variant:").count(), 1);
    }

    #[test]
    fn goal_continuation_appends_to_provider_system_prompt() {
        let config = OpenAiCompatibleConfig {
            flavor: DirectProviderFlavor::OpenAiCompatible,
            model: "mock".to_string(),
            base_url: None,
            api_key_env: None,
            system_prompt: Some("Base system".to_string()),
            temperature: None,
            reasoning_effort: None,
            max_output_tokens: None,
            stream: true,
        };
        let mut provider = openai_provider_json(&config);

        append_system_prompt_to_provider(
            &mut provider,
            GOAL_CONTINUATION_SYSTEM_HEADING,
            "Continue <safely>.",
        );

        let system = provider["systemPrompt"].as_str().unwrap();
        assert!(system.contains("Base system"));
        assert!(system.contains(GOAL_CONTINUATION_SYSTEM_HEADING));
        assert!(system.contains("Continue <safely>."));
    }

    #[test]
    fn approval_and_question_panels_include_resume_actions() {
        let approval = serde_json::from_value::<ApprovalRequest>(json!({
            "id": "approval-1",
            "reason": "write requires approval",
            "risk": "medium",
            "toolCall": { "id": "write-1", "name": "write_file", "namespace": "oppi", "arguments": { "path": "x.txt" } }
        }))
        .unwrap();
        let panel = format_approval_panel(&approval);
        assert!(panel.contains("risk: Medium"));
        assert!(panel.contains("oppi:write_file"));
        assert!(panel.contains("/approve"));
        assert!(panel.contains("/deny"));

        let question = serde_json::from_value::<AskUserRequest>(json!({
            "id": "question-1",
            "title": "Choose",
            "questions": [{ "id": "q1", "question": "Proceed?", "options": [{ "id": "yes", "label": "Yes" }] }]
        }))
        .unwrap();
        let panel = format_ask_user_panel(&question);
        assert!(panel.contains("q1: Proceed?"));
        assert!(panel.contains("yes: Yes"));
        assert!(panel.contains("/answer"));
    }
}
