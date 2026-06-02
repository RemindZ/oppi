use base64::{
    Engine as _,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use oppi_protocol::{
    AgentDefinition, AgentListResult, AgentRun, AgentRunPollResult, AgentRunStatus, AgentSource,
    ApprovalRequest, ArtifactMetadata, AskUserAnswer, AskUserRequest, AskUserSettings,
    DEFAULT_ASK_USER_TIMEOUT_MS, Diagnostic, Event, EventKind, FilesystemPolicy,
    GuardianReviewSettings, ItemKind, MemoryConsolidateResult, MemoryRecallResult, MemoryRecord,
    MemoryRememberResult, MemoryStatus, ModelRef, ModelRegistryListResult, NetworkPolicy,
    PermissionMode, ResolvedAgent, ResolvedSkill, RuntimeListResult, SkillSource,
    SuggestedNextMessage, Thread, ThreadGoal, ThreadGoalStatus, ThreadItemBranch, ThreadItemEntry,
    ThreadItemLabelsResult, ThreadItemsResult, ThreadStatus, TodoClientAction,
    TodoClientActionResult, TodoListResult, TodoState, ToolCall, ToolResultStatus,
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
const STARTER_AGENT_INSTRUCTIONS: &str = "You are a focused OPPi subagent.\n\nReplace this prompt with specific instructions for the delegated work this agent should handle.";
const MERIDIAN_DEFAULT_BASE_URL: &str = "http://127.0.0.1:3456";
const MERIDIAN_PACKAGE_NAME: &str = "@rynfar/meridian";
const HOPPI_PACKAGE_NAME: &str = "@oppiai/hoppi-memory";
const HOPPI_LEGACY_PACKAGE_NAME: &str = "hoppi-memory";
const HOPPI_BRIDGE_TIMEOUT: Duration = Duration::from_secs(120);
const HOPPI_MEMORY_DRAIN_TIMEOUT: Duration = Duration::from_secs(5);
const HOPPI_MEMORY_DRAIN_POLL: Duration = Duration::from_millis(100);
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
const OPENAI_CODEX_DEFAULT_BASE_URL: &str = "https://chatgpt.com/backend-api";
const USAGE_REFRESH_INTERVAL: Duration = Duration::from_secs(60);
const USAGE_FETCH_TIMEOUT: Duration = Duration::from_secs(4);
const DEFAULT_PERMISSION_REVIEW_TIMEOUT_MS: u64 = 45_000;
const ASK_USER_ALLOWED_TIMEOUT_MINUTES: [u64; 5] = [0, 1, 2, 5, 10];
const VALID_IDLE_COMPACT_MINUTES: [u64; 3] = [2, 5, 10];
const VALID_IDLE_COMPACT_THRESHOLDS: [u64; 4] = [50, 60, 70, 80];
const VALID_SMART_COMPACT_THRESHOLDS: [u64; 6] = [50, 55, 60, 65, 70, 75];
const VALID_IMAGE_WIDTH_CELLS: [u64; 3] = [60, 80, 120];
const IMAGE_PREVIEW_PROTOCOL_VALUES_LABEL: &str = "auto|kitty|wezterm|iterm2|sixel|off";
const ITERM2_INLINE_IMAGE_MAX_BYTES: u64 = 4 * 1024 * 1024;
const SIXEL_INLINE_IMAGE_MAX_BYTES: u64 = 4 * 1024 * 1024;
const VALID_MEMORY_AGENT_MODELS: [&str; 3] = ["auto", "claude", "gpt"];
const VALID_DEEP_MEMORY_MODELS: [&str; 3] = ["auto", "sonnet", "gpt-5.5"];
const VALID_HOPPI_INSTALL_OFFERS: [&str; 2] = ["ask", "dismissed"];
const DEFAULT_SYNC_PASSPHRASE_ENV: &str = "OPPI_HOPPI_SYNC_PASSPHRASE";
const VALID_SYNC_BACKGROUND: [&str; 4] = ["off", "15m", "30m", "60m"];
const VALID_SYNC_CONFLICT_MODES: [&str; 3] = ["keep-both", "keep-local", "overwrite-local"];
const DEFAULT_TURN_PROTECTED_PATTERNS: &[&str] = &[
    ".env*",
    ".ssh/",
    "*.pem",
    "*.key",
    ".git/config",
    ".git/hooks/",
    ".npmrc",
    ".pypirc",
    ".mcp.json",
    ".claude.json",
];
const OPENAI_DIRECT_DEFAULT_MODEL: &str = GPT_MAIN_DEFAULT_MODEL;
const OPENAI_CODEX_DEFAULT_MODEL: &str = GPT_MAIN_DEFAULT_MODEL;
const GITHUB_COPILOT_PROVIDER_ID: &str = "github-copilot";
const GOAL_CONTINUATION_INPUT: &str = "Continue working toward the active thread goal.";
const GOAL_CONTINUATION_SYSTEM_HEADING: &str = "OPPi goal continuation";
const GOAL_CONTINUATION_CAP: u32 = 8;
const HOPPI_MEMORY_CONTEXT_MAX_BYTES: usize = 12_000;
const HOPPI_MEMORY_QUERY_MAX_CHARS: usize = 4_000;
const HOPPI_IDLE_CONSOLIDATE_AFTER: Duration = Duration::from_secs(90);
const HOPPI_IDLE_CONSOLIDATE_BUDGET: u64 = 1_500;
const HOPPI_MEMORY_SYSTEM_HEADING: &str = "OPPi Hoppi memory";
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
const MOCK_PROVIDER_DISPLAY_LABEL: &str = "local test provider";
const MOCK_MODEL_ID: &str = "local-test";
const MOCK_MODEL_DISPLAY_LABEL: &str = "local-test";

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
    reasoning_effort_explicit: bool,
    provider: ProviderConfig,
}

fn install_hoppi_package() -> Result<String, String> {
    let root = ensure_managed_packages_root()?;
    let spec = hoppi_package_spec();
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
    if output.status.success() && managed_hoppi_module_path().exists() {
        Ok(format!(
            "Installed Hoppi backend {} into {}. Next: run `/memory status`, `/memory dashboard`, or configure sync from the Hoppi dashboard.",
            spec,
            format_path_for_display(&root)
        ))
    } else {
        let tail = compact_process_output(&[output.stdout, output.stderr].concat());
        Err(format!(
            "Hoppi install failed with status {}{}{}",
            output.status,
            if tail.is_empty() { "" } else { ": " },
            tail
        ))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NewThreadMode {
    New,
    Clear,
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

fn display_model_id(model: &str) -> &str {
    if model == MOCK_MODEL_ID {
        MOCK_MODEL_DISPLAY_LABEL
    } else {
        model
    }
}

#[derive(Debug, Clone, PartialEq)]
struct OpenAiCompatibleConfig {
    flavor: DirectProviderFlavor,
    provider_id: Option<String>,
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
            Self::Mock => MOCK_PROVIDER_DISPLAY_LABEL,
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

include!("shell_parts/slash_palette.rs");
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
    if command.list_sessions {
        session.handle_resume_command(Vec::new(), command.json)?;
        session.shutdown();
        return Ok(());
    }
    apply_saved_reasoning_effort(
        &mut provider,
        &session.role_profile_path,
        command.reasoning_effort_explicit,
    );
    session.register_provider_model(&provider)?;
    if should_print_startup_banner(command.json, session.quiet_startup) {
        println!(
            "OPPi Rust shell ready ({}). Type a prompt, /help, /exit, or Ctrl+C twice in native/raw mode to stop.",
            provider.label()
        );
    }

    if command.interactive && !command.json {
        maybe_offer_hoppi_install_on_startup(&mut session)?;
    }
    maybe_run_hoppi_startup_sync(&mut session, command.json)?;

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
        session.maybe_remember_hoppi_exit_recap();
        session.maybe_drain_memory_jobs(command.json)?;
        maybe_run_hoppi_exit_sync(&mut session, command.json)?;
    }

    session.shutdown();
    Ok(())
}

fn should_print_startup_banner(json_output: bool, quiet_startup: bool) -> bool {
    !json_output && !quiet_startup
}

include!("shell_parts/interactive.rs");
include!("shell_parts/session.rs");
include!("shell_parts/commands.rs");

include!("shell_parts/models_and_memory.rs");

include!("shell_parts/provider_ui.rs");
include!("shell_parts/auth_settings.rs");

include!("shell_parts/rendering.rs");

include!("shell_parts/tests.rs");
