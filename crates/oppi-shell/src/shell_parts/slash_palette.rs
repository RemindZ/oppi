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
enum FollowUpCommandRoute {
    Status,
    SetMode(FollowUpMode),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SteerCommandRoute {
    Status,
    SetMode(SteeringMode),
    Send(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum StartupCommandRoute {
    Status,
    SetQuietStartup(bool),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CursorCommandRoute {
    Status,
    SetHardwareCursor(bool),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TerminalCommandRoute {
    Status,
    SetProgress(bool),
    SetClearOnShrink(bool),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum EditorCommandRoute {
    Status,
    SetPaddingX(usize),
    SetDoubleEscapeAction(DoubleEscapeAction),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TreeCommandRoute {
    Show,
    Entries {
        after: u64,
        limit: Option<usize>,
    },
    Label {
        event_id: u64,
        labels: Vec<String>,
    },
    Fork {
        event_id: u64,
        title: Option<String>,
    },
    Edit {
        event_id: u64,
        input: String,
    },
    Status,
    Fold(String),
    Unfold(String),
    Toggle(String),
    Rename {
        thread_id: String,
        title: String,
    },
    Archive(String),
    SetFilter(TreeFilterMode),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SkillsCommandRoute {
    List,
    Status,
    SetCommands(bool),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum AutocompleteCommandRoute {
    Status,
    SetMaxVisible(usize),
}

const DEFAULT_AUTOCOMPLETE_MAX_VISIBLE: usize = 5;
const MIN_AUTOCOMPLETE_MAX_VISIBLE: usize = 3;
const MAX_AUTOCOMPLETE_MAX_VISIBLE: usize = 20;
const AUTOCOMPLETE_VISIBLE_CYCLE_VALUES: [usize; 6] = [3, 5, 7, 10, 15, 20];
const DEFAULT_EDITOR_PADDING_X: usize = 0;
const EDITOR_PADDING_X_VALUES: [usize; 4] = [0, 1, 2, 3];
const DOUBLE_ESCAPE_WINDOW: Duration = Duration::from_millis(500);
const TREE_FILTER_MODES: [TreeFilterMode; 5] = [
    TreeFilterMode::Default,
    TreeFilterMode::NoTools,
    TreeFilterMode::UserOnly,
    TreeFilterMode::LabeledOnly,
    TreeFilterMode::All,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DoubleEscapeAction {
    Tree,
    Fork,
    None,
}

impl DoubleEscapeAction {
    fn as_str(self) -> &'static str {
        match self {
            Self::Tree => "tree",
            Self::Fork => "fork",
            Self::None => "none",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TreeFilterMode {
    Default,
    NoTools,
    UserOnly,
    LabeledOnly,
    All,
}

impl TreeFilterMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::NoTools => "no-tools",
            Self::UserOnly => "user-only",
            Self::LabeledOnly => "labeled-only",
            Self::All => "all",
        }
    }
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
        usage: "/settings [section|search <query>]",
        description: "Open native settings or list backed line-mode setting commands",
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
        usage: "/models [--all] [filter]",
        description: "List available models",
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
        usage: "/tree [entries|sessions|status|filter <default|no-tools|user-only|labeled-only|all>|fork|edit|resubmit|label|unlabel|fold|unfold|toggle|rename|delete]",
        description: "Show transcript entries; use /sessions or /tree sessions for the session tree",
        aliases: &[],
    },
    SlashCommandSpec {
        command: "/sessions",
        usage: "/sessions [rename|delete|fold|unfold|toggle|entries|status|filter]",
        description: "Show or manage the native session tree",
        aliases: &[],
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
        usage: "/steer [status|mode <all|one-at-a-time>|<text>]",
        description: "Send steering input or configure active-turn delivery",
        aliases: &[],
    },
    SlashCommandSpec {
        command: "/follow-up",
        usage: "/follow-up [status|mode <all|one-at-a-time>]",
        description: "Show or set queued follow-up delivery mode",
        aliases: &["/followups"],
    },
    SlashCommandSpec {
        command: "/startup",
        usage: "/startup [status|quiet <on|off>]",
        description: "Show or set Pi-compatible startup behavior",
        aliases: &["/quiet-startup"],
    },
    SlashCommandSpec {
        command: "/autocomplete",
        usage: "/autocomplete [status|items <3..20>]",
        description: "Show or set Pi-compatible autocomplete visible item count",
        aliases: &["/autocomplete-max-visible"],
    },
    SlashCommandSpec {
        command: "/cursor",
        usage: "/cursor [status|hardware <on|off>]",
        description: "Show or set Pi-compatible terminal hardware cursor behavior",
        aliases: &["/hardware-cursor"],
    },
    SlashCommandSpec {
        command: "/terminal",
        usage: "/terminal [status|progress <on|off>|clear-on-shrink <on|off>]",
        description: "Show or set Pi-compatible terminal behavior",
        aliases: &["/terminal-progress"],
    },
    SlashCommandSpec {
        command: "/editor",
        usage: "/editor [status|padding <0|1|2|3>|double-escape <tree|fork|none>]",
        description: "Show or set Pi-compatible editor behavior",
        aliases: &["/input"],
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
        description: "Set Pi-compatible default thinking with a model-aware slider",
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
        command: "/footer",
        usage: "/footer [status|path|help|usage|model|permission|memory|context]",
        description: "Configure persistent footer visibility",
        aliases: &[],
    },
    SlashCommandSpec {
        command: "/images",
        usage: "/images [status|capabilities|show|width|auto-resize|block|protocol]",
        description: "Configure terminal image preview and provider image handling",
        aliases: &["/image", "/image-settings"],
    },
    SlashCommandSpec {
        command: "/memory",
        usage: "/memory [status|jobs|on|off|startup-recall|task-start-recall|turn-summaries|agent-model|deep-model|idle-dream|sync|dashboard|settings|compaction|maintenance]",
        description: "Control Hoppi memory; bare opens native memory panel",
        aliases: &["/mem", "/memory-maintenance", "/idle-compact"],
    },
    SlashCommandSpec {
        command: "/agents",
        usage: "/agents [list|show|create|edit|delete|dispatch]",
        description: "Manage and dispatch native agents",
        aliases: &[],
    },
    SlashCommandSpec {
        command: "/skills",
        usage: "/skills [list|status|commands <on|off>]",
        description: "List discovered skills and toggle /skill:name commands",
        aliases: &[],
    },
    SlashCommandSpec {
        command: "/prompts",
        usage: "/prompts",
        description: "List discovered prompt templates",
        aliases: &["/prompt-templates"],
    },
    SlashCommandSpec {
        command: "/graphify",
        usage: "/graphify [status|install|commands]",
        description: "Inspect Graphify codebase context setup",
        aliases: &[],
    },
    SlashCommandSpec {
        command: "/theme",
        usage: "/theme [name|reload|preview [name]]",
        description: "Show, preview, or change built-in/discovered Pi themes; bare opens native theme panel",
        aliases: &["/themes"],
    },
    SlashCommandSpec {
        command: "/permissions",
        usage: "/permissions [status|history|timeout|reviewer-model|clear-session|mode]",
        description: "Show/set permissions and Guardian review settings",
        aliases: &[],
    },
    SlashCommandSpec {
        command: "/mcp",
        usage: "/mcp [list|connect|test|reload|auth|trust-plugin|disable-plugin]",
        description: "Show or manage native MCP broker status",
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
        command: "/ask-user",
        usage: "/ask-user [status|timeout <off|1|2|5|10>]",
        description: "Configure ask_user question timeout",
        aliases: &["/ask_user", "/questions"],
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

fn effective_project_roots(settings: &PermissionSandboxSettings, cwd: &str) -> Vec<String> {
    let project_roots = normalize_string_slice(&settings.project_roots);
    if project_roots.is_empty() {
        vec![cwd.to_string()]
    } else {
        project_roots
    }
}

fn effective_readable_roots(
    settings: &PermissionSandboxSettings,
    project_roots: &[String],
) -> Vec<String> {
    let readable_roots = normalize_string_slice(&settings.readable_roots);
    if readable_roots.is_empty() {
        project_roots.to_vec()
    } else {
        readable_roots
    }
}

fn effective_writable_roots(
    settings: &PermissionSandboxSettings,
    project_roots: &[String],
) -> Vec<String> {
    let writable_roots = normalize_string_slice(&settings.writable_roots);
    if writable_roots.is_empty() {
        project_roots.to_vec()
    } else {
        writable_roots
    }
}

#[cfg(test)]
fn sandbox_policy_json_with_protected_patterns(
    mode: PermissionMode,
    cwd: &str,
    protected_patterns: &[String],
) -> Value {
    sandbox_policy_json_with_settings(
        mode,
        cwd,
        &PermissionSandboxSettings {
            protected_patterns: protected_patterns.to_vec(),
            ..PermissionSandboxSettings::default()
        },
    )
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
            | "/provider"
            | "/login"
            | "/meridian"
            | "/memory"
            | "/model"
            | "/roles"
            | "/resume"
            | "/sessions"
            | "/tree"
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
            | "/sessions"
            | "/prompt-variant"
            | "/todos"
            | "/goal"
            | "/theme"
            | "/memory"
            | "/agents"
            | "/prompts"
            | "/effort"
            | "/background"
            | "/review"
            | "/audit"
            | "/init"
            | "/independent"
            | "/bug-report"
            | "/feature-request"
            | "/permissions"
            | "/ask-user"
            | "/mcp"
            | "/keys"
            | "/debug"
            | "/usage"
            | "/footer"
            | "/images"
            | "/autocomplete"
            | "/cursor"
            | "/terminal"
            | "/editor"
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

fn slash_command_palette_items_with_skills(
    query: &str,
    skills: &[ResolvedSkill],
    enable_skill_commands: bool,
) -> Vec<SlashPaletteItem> {
    let mut seen = BTreeSet::new();
    let mut items = Vec::new();
    for item in slash_command_palette_items(query)
        .into_iter()
        .chain(skill_slash_command_items(
            skills,
            enable_skill_commands,
            query,
        ))
    {
        if seen.insert(item.insert.clone()) {
            items.push(item);
        }
    }
    items
}

fn skill_slash_command_items(
    skills: &[ResolvedSkill],
    enable_skill_commands: bool,
    query: &str,
) -> Vec<SlashPaletteItem> {
    if !enable_skill_commands {
        return Vec::new();
    }
    let query = query.trim().trim_start_matches('/').to_ascii_lowercase();
    let mut scored = skills
        .iter()
        .filter_map(|skill| {
            let name = skill.active.name.trim();
            if name.is_empty() {
                return None;
            }
            let command = format!("skill:{name}");
            let command_lower = command.to_ascii_lowercase();
            let name_lower = name.to_ascii_lowercase();
            let description_lower = skill.active.description.to_ascii_lowercase();
            let score = if query.is_empty() {
                0
            } else if command_lower.starts_with(&query) {
                1
            } else if let Some(name_query) = query.strip_prefix("skill:") {
                if name_lower.starts_with(name_query) {
                    2
                } else {
                    return None;
                }
            } else if name_lower.starts_with(&query) {
                4
            } else if name_lower.contains(&query) || description_lower.contains(&query) {
                5
            } else {
                return None;
            };
            Some((score, name.to_string(), skill))
        })
        .collect::<Vec<_>>();
    scored.sort_by_key(|(score, name, _)| (*score, name.to_ascii_lowercase()));
    scored
        .into_iter()
        .map(|(_, name, skill)| {
            let label = format!("/skill:{name}");
            let source = skill_source_label(skill.active.source);
            let detail = if skill.active.description.trim().is_empty() {
                format!("{source} skill")
            } else {
                format!("{source} skill: {}", skill.active.description.trim())
            };
            SlashPaletteItem {
                label,
                insert: format!("/skill:{name} "),
                detail,
            }
        })
        .collect()
}

fn skill_source_label(source: SkillSource) -> &'static str {
    match source {
        SkillSource::BuiltIn => "built-in",
        SkillSource::User => "user",
        SkillSource::Project => "project",
        SkillSource::Plugin => "plugin",
        SkillSource::Cli => "cli",
    }
}

fn arg_item(label: &str, insert: &str, detail: &str) -> SlashPaletteItem {
    SlashPaletteItem {
        label: label.to_string(),
        insert: insert.to_string(),
        detail: detail.to_string(),
    }
}

#[derive(Debug, Clone, Copy)]
struct SettingsCommandEntry {
    section: &'static str,
    label: &'static str,
    command: &'static str,
    description: &'static str,
    keywords: &'static str,
}

#[derive(Debug, Clone, Copy)]
struct SettingsInfoEntry {
    section: &'static str,
    label: &'static str,
    status: &'static str,
    description: &'static str,
    keywords: &'static str,
}

const SETTINGS_SECTIONS: &[(&str, &str)] = &[
    ("general", "usage, help, ask-user"),
    (
        "pi",
        "model, effort, provider, login, follow-ups, startup, terminal",
    ),
    ("footer", "footer visibility"),
    ("images", "image preview and provider handling"),
    ("permissions", "Guardian review and sandbox controls"),
    ("theme", "theme preview and apply"),
    ("memory", "Hoppi memory controls"),
    ("compaction", "idle and smart compaction"),
    ("workspace", "sessions, resume, and tree"),
    ("mcp", "MCP servers, tools, and resources"),
];

const SETTINGS_COMMAND_ENTRIES: &[SettingsCommandEntry] = &[
    SettingsCommandEntry {
        section: "general",
        label: "Command help",
        command: "/help",
        description: "List the supported slash command surface.",
        keywords: "commands keybindings shortcuts",
    },
    SettingsCommandEntry {
        section: "general",
        label: "Usage and status",
        command: "/usage",
        description: "Show local usage/status counters and diagnostics.",
        keywords: "stats status footer",
    },
    SettingsCommandEntry {
        section: "general",
        label: "Question timeout",
        command: "/ask-user timeout <off|1|2|5|10>",
        description: "Persist the backed ask_user question timeout.",
        keywords: "ask user ask-user questions timeout",
    },
    SettingsCommandEntry {
        section: "pi",
        label: "Main model",
        command: "/model <id>",
        description: "Set the session model.",
        keywords: "model picker ctrl-l scoped",
    },
    SettingsCommandEntry {
        section: "pi",
        label: "Scoped models",
        command: "/scoped-models list|enable|disable",
        description: "Control Pi-compatible enabled model patterns.",
        keywords: "model picker cycle ctrl-p",
    },
    SettingsCommandEntry {
        section: "pi",
        label: "Default thinking",
        command: "/effort <low|medium|high>",
        description: "Persist Pi-compatible defaultThinkingLevel for reasoning-capable models.",
        keywords: "reasoning thinking effort default shift-tab",
    },
    SettingsCommandEntry {
        section: "pi",
        label: "Provider setup",
        command: "/provider status|validate|auth-env|base-url",
        description: "Inspect and configure direct provider readiness.",
        keywords: "provider api openai compatible base url auth",
    },
    SettingsCommandEntry {
        section: "pi",
        label: "Login",
        command: "/login subscription|api",
        description: "Choose subscription OAuth or env-ref API login.",
        keywords: "auth codex copilot claude meridian oauth",
    },
    SettingsCommandEntry {
        section: "pi",
        label: "Follow-up mode",
        command: "/follow-up mode <all|one-at-a-time>",
        description: "Persist Pi-compatible queued follow-up delivery mode.",
        keywords: "follow-up followups queue delivery alt enter",
    },
    SettingsCommandEntry {
        section: "pi",
        label: "Steering mode",
        command: "/steer mode <all|one-at-a-time>",
        description: "Persist Pi-compatible active-turn steering delivery mode.",
        keywords: "steer steering queue delivery ctrl enter",
    },
    SettingsCommandEntry {
        section: "pi",
        label: "Quiet startup",
        command: "/startup quiet <on|off>",
        description: "Persist Pi-compatible quietStartup for the native ready banner.",
        keywords: "startup quiet banner verbose",
    },
    SettingsCommandEntry {
        section: "pi",
        label: "Skill commands",
        command: "/skills commands <on|off>",
        description: "Persist Pi-compatible enableSkillCommands for /skill:name prompts.",
        keywords: "skills slash commands enableSkillCommands",
    },
    SettingsCommandEntry {
        section: "pi",
        label: "Autocomplete items",
        command: "/autocomplete items <3..20>",
        description: "Persist Pi-compatible autocompleteMaxVisible for command palettes.",
        keywords: "autocomplete dropdown visible items autocompleteMaxVisible commands",
    },
    SettingsCommandEntry {
        section: "pi",
        label: "Hardware cursor",
        command: "/cursor hardware <on|off>",
        description: "Persist Pi-compatible showHardwareCursor for native TUI loops.",
        keywords: "cursor hardware terminal ime showHardwareCursor",
    },
    SettingsCommandEntry {
        section: "pi",
        label: "Terminal progress",
        command: "/terminal progress <on|off>",
        description: "Persist Pi-compatible terminal.showTerminalProgress for OSC 9;4 turn progress.",
        keywords: "terminal progress osc 9;4 showTerminalProgress",
    },
    SettingsCommandEntry {
        section: "pi",
        label: "Editor padding",
        command: "/editor padding <0|1|2|3>",
        description: "Persist Pi-compatible editorPaddingX for native input padding.",
        keywords: "editor input padding editorPaddingX gutter",
    },
    SettingsCommandEntry {
        section: "pi",
        label: "Double escape",
        command: "/editor double-escape <tree|fork|none>",
        description: "Persist Pi-compatible doubleEscapeAction for empty-editor Escape presses.",
        keywords: "editor escape double doubleEscapeAction tree fork none",
    },
    SettingsCommandEntry {
        section: "pi",
        label: "Tree filter",
        command: "/tree filter <default|no-tools|user-only|labeled-only|all>",
        description: "Persist Pi-compatible treeFilterMode for transcript entries and sessions.",
        keywords: "tree entries session filter treeFilterMode default no-tools user-only labeled-only all",
    },
    SettingsCommandEntry {
        section: "footer",
        label: "Footer status",
        command: "/footer status",
        description: "Show persisted footer visibility settings.",
        keywords: "status visibility",
    },
    SettingsCommandEntry {
        section: "footer",
        label: "Footer visibility",
        command: "/footer path|help|model|permission|memory|context <on|off>",
        description: "Toggle backed footer chips and help/path rows.",
        keywords: "chips workspace path help context",
    },
    SettingsCommandEntry {
        section: "footer",
        label: "Usage chips",
        command: "/footer usage <session-weekly|session|weekly|off>",
        description: "Choose which usage counters appear in the footer.",
        keywords: "stats counters weekly session",
    },
    SettingsCommandEntry {
        section: "images",
        label: "Image status",
        command: "/images status",
        description: "Show persisted image preview/provider settings.",
        keywords: "image preview provider",
    },
    SettingsCommandEntry {
        section: "images",
        label: "Terminal capabilities",
        command: "/images capabilities",
        description: "Report detected inline image protocol and fallbacks.",
        keywords: "kitty wezterm iterm sixel protocol terminal probe",
    },
    SettingsCommandEntry {
        section: "images",
        label: "Preview protocol",
        command: "/images protocol <auto|kitty|wezterm|iterm2|sixel|off>",
        description: "Persist a terminal inline-image protocol override.",
        keywords: "kitty wezterm iterm sixel protocol terminal override",
    },
    SettingsCommandEntry {
        section: "images",
        label: "Inline previews",
        command: "/images show <on|off>",
        description: "Enable or disable terminal image previews.",
        keywords: "preview visibility",
    },
    SettingsCommandEntry {
        section: "images",
        label: "Preview width",
        command: "/images width <60|80|120>",
        description: "Persist the target inline preview width in cells.",
        keywords: "image width cells resize",
    },
    SettingsCommandEntry {
        section: "images",
        label: "Provider image handling",
        command: "/images auto-resize|block <on|off>",
        description: "Control provider image auto-resize and blocking settings.",
        keywords: "provider resize block",
    },
    SettingsCommandEntry {
        section: "permissions",
        label: "Permission status",
        command: "/permissions status",
        description: "Show effective mode, Guardian, sandbox, and protected-pattern state.",
        keywords: "guardian sandbox protected roots status",
    },
    SettingsCommandEntry {
        section: "permissions",
        label: "Review history",
        command: "/permissions history",
        description: "Show recent Guardian review decisions.",
        keywords: "guardian auto-review approvals denials",
    },
    SettingsCommandEntry {
        section: "permissions",
        label: "Guardian context",
        command: "/permissions context",
        description: "Show the auto-review prompt skeleton, context inputs, and reviewer helper tools.",
        keywords: "guardian prompt context reviewer tools auto-review",
    },
    SettingsCommandEntry {
        section: "permissions",
        label: "Guardian timeout",
        command: "/permissions timeout <seconds>",
        description: "Persist the auto-review timeout.",
        keywords: "guardian reviewer model timeout",
    },
    SettingsCommandEntry {
        section: "permissions",
        label: "Reviewer model",
        command: "/permissions reviewer-model <auto|provider/model>",
        description: "Persist the reviewer model override.",
        keywords: "guardian model auto-review",
    },
    SettingsCommandEntry {
        section: "permissions",
        label: "Sandbox validation",
        command: "/permissions sandbox validate",
        description: "Preview effective sandbox roots, root sources, and path issues.",
        keywords: "sandbox roots root validate preview",
    },
    SettingsCommandEntry {
        section: "permissions",
        label: "Sandbox presets",
        command: "/permissions sandbox roots <here|default>",
        description: "Set all sandbox roots to the workspace or restore fallback behavior.",
        keywords: "sandbox roots root project read write preset",
    },
    SettingsCommandEntry {
        section: "permissions",
        label: "Sandbox network",
        command: "/permissions sandbox internet|ssh <on|off>",
        description: "Persist backed sandbox network and SSH allowances.",
        keywords: "sandbox network internet ssh",
    },
    SettingsCommandEntry {
        section: "permissions",
        label: "Sandbox root lists",
        command: "/permissions sandbox project|read|write <paths|default>",
        description: "Replace backed project, readable, or writable sandbox roots.",
        keywords: "sandbox roots root project readable writable read write",
    },
    SettingsCommandEntry {
        section: "permissions",
        label: "Sandbox root edits",
        command: "/permissions sandbox project|read|write add|remove <paths>",
        description: "Append or remove configured sandbox roots without replacing the list.",
        keywords: "sandbox roots root add remove multi-select",
    },
    SettingsCommandEntry {
        section: "permissions",
        label: "Protected patterns",
        command: "/permissions sandbox protected <patterns|default>",
        description: "Persist extra protected file/path patterns.",
        keywords: "sandbox protected env ssh pem key",
    },
    SettingsCommandEntry {
        section: "theme",
        label: "Theme status",
        command: "/theme",
        description: "Show current and discovered themes.",
        keywords: "themes picker status",
    },
    SettingsCommandEntry {
        section: "theme",
        label: "Theme preview",
        command: "/theme preview [name]",
        description: "List themes or render a terminal-safe preview sample.",
        keywords: "theme preview picker sample",
    },
    SettingsCommandEntry {
        section: "theme",
        label: "Apply theme",
        command: "/theme <name>",
        description: "Apply a built-in or discovered theme.",
        keywords: "theme apply current",
    },
    SettingsCommandEntry {
        section: "theme",
        label: "Reload project theme",
        command: "/theme reload",
        description: "Reload the project theme file.",
        keywords: "theme reload project plugin",
    },
    SettingsCommandEntry {
        section: "memory",
        label: "Memory status",
        command: "/memory status",
        description: "Show Hoppi backend, sync, recall, and write settings.",
        keywords: "hoppi memory status backend",
    },
    SettingsCommandEntry {
        section: "memory",
        label: "Memory jobs",
        command: "/memory jobs",
        description: "Show Rust-owned memory worker queue and recent jobs.",
        keywords: "hoppi memory jobs queue worker status",
    },
    SettingsCommandEntry {
        section: "memory",
        label: "Memory behavior",
        command: "/memory on|off|startup-recall|task-start-recall|turn-summaries",
        description: "Persist shared Hoppi behavior switches.",
        keywords: "hoppi memory recall summaries",
    },
    SettingsCommandEntry {
        section: "memory",
        label: "Memory models",
        command: "/memory agent-model|deep-model|idle-dream",
        description: "Persist Hoppi model and idle maintenance settings.",
        keywords: "hoppi deep dream idle consolidation model",
    },
    SettingsCommandEntry {
        section: "memory",
        label: "Memory sync settings",
        command: "/memory sync repo|remote|pull-startup|push-exit|background|encryption|conflict",
        description: "Configure shared Git sync metadata.",
        keywords: "hoppi sync github repo pull push background",
    },
    SettingsCommandEntry {
        section: "memory",
        label: "Manual sync",
        command: "/memory sync now|pull|push",
        description: "Run explicit Hoppi sync actions through the installed backend.",
        keywords: "hoppi sync action manual",
    },
    SettingsCommandEntry {
        section: "memory",
        label: "Memory dashboard",
        command: "/memory dashboard",
        description: "Open the installed Hoppi dashboard through the native bridge.",
        keywords: "hoppi dashboard url localhost",
    },
    SettingsCommandEntry {
        section: "memory",
        label: "Memory records",
        command: "/memory list|forget|pin|unpin|edit|supersede",
        description: "List and mutate Hoppi records through normalized runtime routes.",
        keywords: "hoppi records recall remember pin edit forget supersede",
    },
    SettingsCommandEntry {
        section: "memory",
        label: "Backend package",
        command: "/memory install-hoppi [--yes]",
        description: "Show approval guidance or install/update the managed Hoppi package.",
        keywords: "hoppi install package backend",
    },
    SettingsCommandEntry {
        section: "compaction",
        label: "Compaction status",
        command: "/idle-compact status",
        description: "Show idle/smart thresholds, usage telemetry, and readiness.",
        keywords: "compact compaction context threshold",
    },
    SettingsCommandEntry {
        section: "compaction",
        label: "Compact now",
        command: "/idle-compact compact",
        description: "Create a deterministic todo-aware handoff immediately.",
        keywords: "compact compaction handoff todos",
    },
    SettingsCommandEntry {
        section: "compaction",
        label: "Idle compact settings",
        command: "/idle-compact on|off|idle-time|idle-threshold|smart-threshold",
        description: "Persist idle/smart compaction settings.",
        keywords: "compact compaction idle smart threshold",
    },
    SettingsCommandEntry {
        section: "compaction",
        label: "Memory maintenance",
        command: "/memory maintenance dry-run|apply",
        description: "Run visible Hoppi maintenance through the installed backend.",
        keywords: "hoppi memory consolidate maintenance",
    },
    SettingsCommandEntry {
        section: "workspace",
        label: "Sessions",
        command: "/sessions",
        description: "Open or list native session history.",
        keywords: "workspace resume history",
    },
    SettingsCommandEntry {
        section: "workspace",
        label: "Resume",
        command: "/resume",
        description: "Resume a prior session for this workspace.",
        keywords: "workspace sessions history",
    },
    SettingsCommandEntry {
        section: "workspace",
        label: "Tree",
        command: "/tree",
        description: "Show transcript entries for the active thread.",
        keywords: "workspace transcript entries tree",
    },
    SettingsCommandEntry {
        section: "mcp",
        label: "MCP status",
        command: "/mcp list",
        description: "List configured MCP servers, tools, resources, and subscriptions.",
        keywords: "mcp servers tools resources subscriptions",
    },
    SettingsCommandEntry {
        section: "mcp",
        label: "MCP connection",
        command: "/mcp connect|test|reload|auth <server-id>",
        description: "Connect, test, reload, or start auth for an MCP server.",
        keywords: "mcp server connection auth reload test",
    },
    SettingsCommandEntry {
        section: "mcp",
        label: "MCP resources",
        command: "/mcp resources|read|subscribe|unsubscribe",
        description: "List, read, and manage MCP resource subscriptions.",
        keywords: "mcp resources resource subscriptions updates",
    },
];

const SETTINGS_INFO_ENTRIES: &[SettingsInfoEntry] = &[
    SettingsInfoEntry {
        section: "pi",
        label: "Collapse changelog",
        status: "stable Pi only",
        description: "Show condensed changelog output after stable Pi updates.",
        keywords: "changelog collapse updates stable pi only",
    },
    SettingsInfoEntry {
        section: "pi",
        label: "Install telemetry",
        status: "stable Pi only",
        description: "Send stable Pi's anonymous install/update telemetry ping.",
        keywords: "install telemetry analytics ping stable pi only",
    },
];

fn settings_section_alias(section: &str) -> Option<&'static str> {
    match section.trim().to_ascii_lowercase().as_str() {
        "general" | "gen" => Some("general"),
        "pi" | "model" | "models" | "provider" | "login" => Some("pi"),
        "footer" | "status" => Some("footer"),
        "image" | "images" | "preview" | "previews" => Some("images"),
        "permission" | "permissions" | "guardian" | "sandbox" => Some("permissions"),
        "theme" | "themes" => Some("theme"),
        "memory" | "mem" | "hoppi" => Some("memory"),
        "compact" | "compaction" | "idle-compact" => Some("compaction"),
        "workspace" | "sessions" | "session" | "resume" | "tree" => Some("workspace"),
        "mcp" => Some("mcp"),
        _ => None,
    }
}

fn format_settings_command_panel(args: &str) -> Result<String, String> {
    let trimmed = args.trim();
    if trimmed.is_empty()
        || matches!(
            trimmed.to_ascii_lowercase().as_str(),
            "status" | "sections" | "list"
        )
    {
        return Ok(format_settings_root_panel());
    }

    let lower = trimmed.to_ascii_lowercase();
    if lower == "search" || lower.starts_with("search ") {
        let query = trimmed.strip_prefix("search").unwrap_or_default().trim();
        return Ok(format_settings_search_panel(query));
    }

    if let Some(section) = settings_section_alias(trimmed) {
        return Ok(format_settings_section_panel(section));
    }

    Err(format!(
        "settings: unknown section `{trimmed}`. Use /settings, /settings <section>, or /settings search <query>."
    ))
}

fn format_settings_root_panel() -> String {
    let mut rendered = [
        "settings sections".to_string(),
        "Use /settings <section> for backed line-mode commands, or /settings search sandbox to find matching controls.".to_string(),
        "In retained/Ratatui TUI, bare /settings opens the interactive settings panel.".to_string(),
        String::new(),
    ]
    .join("\n");
    for (section, description) in SETTINGS_SECTIONS {
        rendered.push_str(&format!(
            "- {section}: /settings {section} ({description})\n"
        ));
    }
    rendered.trim_end().to_string()
}

fn format_settings_section_panel(section: &str) -> String {
    let description = SETTINGS_SECTIONS
        .iter()
        .find_map(|(slug, description)| (*slug == section).then_some(*description))
        .unwrap_or("backed settings");
    let entries = SETTINGS_COMMAND_ENTRIES
        .iter()
        .filter(|entry| entry.section == section)
        .collect::<Vec<_>>();
    let mut rendered = format!(
        "settings: {section}\n{description}\n\n{}",
        format_settings_entries(&entries)
    );
    let info_entries = SETTINGS_INFO_ENTRIES
        .iter()
        .filter(|entry| entry.section == section)
        .collect::<Vec<_>>();
    if !info_entries.is_empty() {
        rendered.push_str("\n\nstable Pi-only rows\n");
        rendered.push_str(&format_settings_info_entries(&info_entries));
        rendered.push_str("\nThese are not editable native toggles.");
    }
    rendered.push_str("\n\nUse /settings search <query> to search backed commands and notes.");
    rendered
}

fn format_settings_search_panel(query: &str) -> String {
    let terms = query
        .split_whitespace()
        .map(|term| term.trim_matches('/').to_ascii_lowercase())
        .filter(|term| !term.is_empty())
        .collect::<Vec<_>>();
    if terms.is_empty() {
        return "settings search: provide a query, e.g. /settings search sandbox roots".to_string();
    }
    let entries = SETTINGS_COMMAND_ENTRIES
        .iter()
        .filter(|entry| settings_entry_matches(entry, &terms))
        .collect::<Vec<_>>();
    let mut rendered = format!("settings search: {query}\n");
    if entries.is_empty() {
        rendered.push_str(
            "No backed settings commands matched. Try /settings sections or a broader query.",
        );
    } else {
        rendered.push('\n');
        rendered.push_str(&format_settings_entries(&entries));
    }
    rendered
}

fn settings_entry_matches(entry: &SettingsCommandEntry, terms: &[String]) -> bool {
    let haystack = format!(
        "{} {} {} {} {}",
        entry.section, entry.label, entry.command, entry.description, entry.keywords
    )
    .to_ascii_lowercase();
    terms.iter().all(|term| haystack.contains(term))
}

fn settings_info_entry_matches(entry: &SettingsInfoEntry, terms: &[String]) -> bool {
    let haystack = format!(
        "{} {} {} {} {}",
        entry.section, entry.label, entry.status, entry.description, entry.keywords
    )
    .to_ascii_lowercase();
    terms.iter().all(|term| haystack.contains(term))
}

fn format_settings_entries(entries: &[&SettingsCommandEntry]) -> String {
    entries
        .iter()
        .map(|entry| {
            format!(
                "- {}: {}\n  {}",
                entry.label, entry.command, entry.description
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_settings_info_entries(entries: &[&SettingsInfoEntry]) -> String {
    entries
        .iter()
        .map(|entry| {
            format!(
                "- {}: {}\n  {}",
                entry.label, entry.status, entry.description
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn slash_arg_suggestions(command: &str, args: &str) -> Vec<SlashPaletteItem> {
    let mut items = match command {
        "/settings" | "/prefs" | "/preferences" | "/settings:oppi" | "/oppi-settings" => vec![
            arg_item(
                "search",
                "/settings search ",
                "Search backed setting commands",
            ),
            arg_item("general", "/settings general", "Usage, help, ask-user"),
            arg_item("pi", "/settings pi", "Model, provider, follow-ups"),
            arg_item("footer", "/settings footer", "Footer visibility"),
            arg_item("images", "/settings images", "Image preview settings"),
            arg_item(
                "permissions",
                "/settings permissions",
                "Guardian and sandbox",
            ),
            arg_item("theme", "/settings theme", "Theme preview and apply"),
            arg_item("memory", "/settings memory", "Hoppi memory controls"),
            arg_item(
                "compaction",
                "/settings compaction",
                "Idle and smart compaction",
            ),
            arg_item("workspace", "/settings workspace", "Sessions and tree"),
            arg_item("mcp", "/settings mcp", "MCP server and resource commands"),
        ],
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
        "/follow-up" | "/followups" => vec![
            arg_item("status", "/follow-up status", "Show mode and queue depth"),
            arg_item(
                "mode all",
                "/follow-up mode all",
                "Deliver queued follow-ups together",
            ),
            arg_item(
                "mode one-at-a-time",
                "/follow-up mode one-at-a-time",
                "Deliver queued follow-ups separately",
            ),
        ],
        "/steer" => vec![
            arg_item("status", "/steer status", "Show mode and queue depth"),
            arg_item(
                "mode all",
                "/steer mode all",
                "Deliver every steering message to the active turn",
            ),
            arg_item(
                "mode one-at-a-time",
                "/steer mode one-at-a-time",
                "Queue extra steering messages separately",
            ),
            arg_item("<text>", "/steer ", "Steer the active turn"),
        ],
        "/startup" | "/quiet-startup" => vec![
            arg_item("status", "/startup status", "Show startup behavior"),
            arg_item("quiet on", "/startup quiet on", "Suppress ready banner"),
            arg_item("quiet off", "/startup quiet off", "Show ready banner"),
        ],
        "/autocomplete" | "/autocomplete-max-visible" => vec![
            arg_item("status", "/autocomplete status", "Show visible item count"),
            arg_item(
                "items 3",
                "/autocomplete items 3",
                "Show three autocomplete items",
            ),
            arg_item(
                "items 5",
                "/autocomplete items 5",
                "Show five autocomplete items",
            ),
            arg_item(
                "items 7",
                "/autocomplete items 7",
                "Show seven autocomplete items",
            ),
            arg_item(
                "items 10",
                "/autocomplete items 10",
                "Show ten autocomplete items",
            ),
            arg_item(
                "items 15",
                "/autocomplete items 15",
                "Show fifteen autocomplete items",
            ),
            arg_item(
                "items 20",
                "/autocomplete items 20",
                "Show twenty autocomplete items",
            ),
        ],
        "/cursor" | "/hardware-cursor" => vec![
            arg_item("status", "/cursor status", "Show cursor behavior"),
            arg_item(
                "hardware on",
                "/cursor hardware on",
                "Keep the terminal hardware cursor visible",
            ),
            arg_item(
                "hardware off",
                "/cursor hardware off",
                "Hide the terminal hardware cursor in native TUI",
            ),
        ],
        "/terminal" | "/terminal-progress" => vec![
            arg_item(
                "status",
                "/terminal status",
                "Show terminal progress behavior",
            ),
            arg_item(
                "progress on",
                "/terminal progress on",
                "Emit OSC 9;4 progress while turns are active",
            ),
            arg_item(
                "progress off",
                "/terminal progress off",
                "Disable OSC 9;4 turn progress indicators",
            ),
            arg_item(
                "clear-on-shrink on",
                "/terminal clear-on-shrink on",
                "Clear stale retained rows when terminal content shrinks",
            ),
            arg_item(
                "clear-on-shrink off",
                "/terminal clear-on-shrink off",
                "Leave stale-row cleanup disabled for Pi-compatible defaults",
            ),
        ],
        "/editor" | "/input" => vec![
            arg_item("status", "/editor status", "Show editor behavior"),
            arg_item(
                "padding 0",
                "/editor padding 0",
                "Use no extra input padding",
            ),
            arg_item(
                "padding 1",
                "/editor padding 1",
                "Use one column of input padding",
            ),
            arg_item(
                "padding 2",
                "/editor padding 2",
                "Use two columns of input padding",
            ),
            arg_item(
                "padding 3",
                "/editor padding 3",
                "Use three columns of input padding",
            ),
            arg_item(
                "double-escape tree",
                "/editor double-escape tree",
                "Open transcript entries after double Escape with an empty editor",
            ),
            arg_item(
                "double-escape fork",
                "/editor double-escape fork",
                "Prepare a fork action after double Escape with an empty editor",
            ),
            arg_item(
                "double-escape none",
                "/editor double-escape none",
                "Disable the empty-editor double Escape shortcut",
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
        "/permissions" => vec![
            arg_item("status", &format!("{command} status"), "Show active policy"),
            arg_item(
                "history",
                &format!("{command} history"),
                "Show Guardian reviews",
            ),
            arg_item(
                "context",
                &format!("{command} context"),
                "Show Guardian prompt context",
            ),
            arg_item(
                "timeout 30",
                &format!("{command} timeout 30"),
                "Set reviewer timeout",
            ),
            arg_item(
                "reviewer-model auto",
                &format!("{command} reviewer-model auto"),
                "Use the parent provider",
            ),
            arg_item(
                "sandbox validate",
                &format!("{command} sandbox validate"),
                "Validate effective sandbox roots",
            ),
            arg_item(
                "sandbox roots here",
                &format!("{command} sandbox roots here"),
                "Set all sandbox roots to the current workspace",
            ),
            arg_item(
                "sandbox roots default",
                &format!("{command} sandbox roots default"),
                "Reset sandbox roots to fallback behavior",
            ),
            arg_item(
                "sandbox project add",
                &format!("{command} sandbox project add "),
                "Append project sandbox roots",
            ),
            arg_item(
                "sandbox project remove",
                &format!("{command} sandbox project remove "),
                "Remove project sandbox roots",
            ),
            arg_item(
                "sandbox read add",
                &format!("{command} sandbox read add "),
                "Append readable sandbox roots",
            ),
            arg_item(
                "sandbox read remove",
                &format!("{command} sandbox read remove "),
                "Remove readable sandbox roots",
            ),
            arg_item(
                "sandbox write add",
                &format!("{command} sandbox write add "),
                "Append writable sandbox roots",
            ),
            arg_item(
                "sandbox write remove",
                &format!("{command} sandbox write remove "),
                "Remove writable sandbox roots",
            ),
            arg_item(
                "sandbox protected",
                &format!("{command} sandbox protected "),
                "Edit extra protected path patterns",
            ),
            arg_item(
                "clear-session",
                &format!("{command} clear-session"),
                "Clear Guardian session state",
            ),
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
        "/ask-user" | "/ask_user" | "/questions" => vec![
            arg_item(
                "status",
                &format!("{command} status"),
                "Show timeout setting",
            ),
            arg_item(
                "timeout off",
                &format!("{command} timeout off"),
                "Disable question timeout",
            ),
            arg_item(
                "timeout 5",
                &format!("{command} timeout 5"),
                "Use 5 minute timeout",
            ),
            arg_item(
                "timeout 10",
                &format!("{command} timeout 10"),
                "Use 10 minute timeout",
            ),
        ],
        "/footer" => vec![
            arg_item(
                "status",
                "/footer status",
                "Show footer visibility settings",
            ),
            arg_item("path on", "/footer path on", "Show workspace path"),
            arg_item("path off", "/footer path off", "Hide workspace path"),
            arg_item("help on", "/footer help on", "Show hotkey help row"),
            arg_item("help off", "/footer help off", "Hide hotkey help row"),
            arg_item(
                "usage session-weekly",
                "/footer usage session-weekly",
                "Show session and weekly usage",
            ),
            arg_item(
                "usage session",
                "/footer usage session",
                "Show session usage only",
            ),
            arg_item(
                "usage weekly",
                "/footer usage weekly",
                "Show weekly usage only",
            ),
            arg_item("usage off", "/footer usage off", "Hide usage chips"),
            arg_item("model on", "/footer model on", "Show model chip"),
            arg_item("model off", "/footer model off", "Hide model chip"),
            arg_item(
                "permission on",
                "/footer permission on",
                "Show permission chip",
            ),
            arg_item(
                "permission off",
                "/footer permission off",
                "Hide permission chip",
            ),
            arg_item("memory on", "/footer memory on", "Show memory chip"),
            arg_item("memory off", "/footer memory off", "Hide memory chip"),
            arg_item("context on", "/footer context on", "Show context chip"),
            arg_item("context off", "/footer context off", "Hide context chip"),
        ],
        "/images" => vec![
            arg_item("status", "/images status", "Show image settings"),
            arg_item(
                "capabilities",
                "/images capabilities",
                "Show terminal image preview support",
            ),
            arg_item(
                "protocol auto",
                "/images protocol auto",
                "Auto-detect inline image protocol",
            ),
            arg_item(
                "protocol kitty",
                "/images protocol kitty",
                "Force Kitty inline image protocol",
            ),
            arg_item(
                "protocol wezterm",
                "/images protocol wezterm",
                "Force WezTerm inline image protocol",
            ),
            arg_item(
                "protocol iterm2",
                "/images protocol iterm2",
                "Force iTerm2 inline image protocol",
            ),
            arg_item(
                "protocol sixel",
                "/images protocol sixel",
                "Force Sixel inline image protocol",
            ),
            arg_item(
                "protocol off",
                "/images protocol off",
                "Disable inline image protocol detection",
            ),
            arg_item("show on", "/images show on", "Render inline image previews"),
            arg_item("show off", "/images show off", "Hide inline image previews"),
            arg_item("width 60", "/images width 60", "Use 60-cell image previews"),
            arg_item("width 80", "/images width 80", "Use 80-cell image previews"),
            arg_item(
                "width 120",
                "/images width 120",
                "Use 120-cell image previews",
            ),
            arg_item(
                "auto-resize on",
                "/images auto-resize on",
                "Resize large input images before providers",
            ),
            arg_item(
                "auto-resize off",
                "/images auto-resize off",
                "Keep input image size unchanged",
            ),
            arg_item(
                "block on",
                "/images block on",
                "Prevent images from being sent to providers",
            ),
            arg_item(
                "block off",
                "/images block off",
                "Allow images to be sent to providers",
            ),
        ],
        "/mcp" => vec![
            arg_item("list", "/mcp list", "Show servers, tools, and diagnostics"),
            arg_item(
                "connect",
                "/mcp connect ",
                "Initialize a trusted local server",
            ),
            arg_item("test", "/mcp test ", "Record/check server diagnostics"),
            arg_item("reload", "/mcp reload ", "Reload server metadata"),
            arg_item("auth", "/mcp auth ", "Start auth flow when supported"),
            arg_item(
                "resources",
                "/mcp resources ",
                "List resources exposed by a trusted MCP server, optionally from a cursor",
            ),
            arg_item(
                "templates",
                "/mcp templates ",
                "List resource templates exposed by a trusted MCP server, optionally from a cursor",
            ),
            arg_item(
                "read",
                "/mcp read ",
                "Read one resource from a trusted MCP server",
            ),
            arg_item(
                "subscribe",
                "/mcp subscribe ",
                "Subscribe to updates for one MCP resource",
            ),
            arg_item(
                "unsubscribe",
                "/mcp unsubscribe ",
                "Stop updates for one MCP resource",
            ),
            arg_item(
                "prompts",
                "/mcp prompts ",
                "List prompts exposed by a trusted MCP server, optionally from a cursor",
            ),
            arg_item(
                "prompt",
                "/mcp prompt ",
                "Get one prompt from a trusted MCP server",
            ),
            arg_item(
                "trust-plugin",
                "/mcp trust-plugin ",
                "Trust and enable a plugin MCP server",
            ),
            arg_item(
                "disable-plugin",
                "/mcp disable-plugin ",
                "Disable a plugin MCP server trust record",
            ),
        ],
        "/theme" => vec![
            arg_item("preview", "/theme preview", "Preview available themes"),
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
            arg_item("jobs", "/memory jobs", "Show memory worker jobs"),
            arg_item(
                "jobs cancel",
                "/memory jobs cancel ",
                "Cancel an active memory worker job",
            ),
            arg_item("on", "/memory on", "Enable runtime-backed memory"),
            arg_item("off", "/memory off", "Disable memory"),
            arg_item(
                "startup-recall on",
                "/memory startup-recall on",
                "Inject startup memory context",
            ),
            arg_item(
                "startup-recall off",
                "/memory startup-recall off",
                "Skip startup memory context",
            ),
            arg_item(
                "task-start-recall on",
                "/memory task-start-recall on",
                "Recall memories at task start",
            ),
            arg_item(
                "task-start-recall off",
                "/memory task-start-recall off",
                "Skip task-start recall",
            ),
            arg_item(
                "turn-summaries on",
                "/memory turn-summaries on",
                "Save bounded turn summaries",
            ),
            arg_item(
                "turn-summaries off",
                "/memory turn-summaries off",
                "Skip turn summary writes",
            ),
            arg_item(
                "agent-model auto",
                "/memory agent-model auto",
                "Use automatic memory distillation model",
            ),
            arg_item(
                "agent-model gpt",
                "/memory agent-model gpt",
                "Use GPT-backed memory distillation",
            ),
            arg_item(
                "deep-model auto",
                "/memory deep-model auto",
                "Use automatic deep-dream model policy",
            ),
            arg_item(
                "deep-model sonnet",
                "/memory deep-model sonnet",
                "Use Sonnet for deep memory reconciliation",
            ),
            arg_item(
                "idle-dream on",
                "/memory idle-dream on",
                "Enable idle memory consolidation",
            ),
            arg_item(
                "idle-dream off",
                "/memory idle-dream off",
                "Disable idle memory consolidation",
            ),
            arg_item(
                "install-offer ask",
                "/memory install-offer ask",
                "Offer Hoppi install on first start",
            ),
            arg_item(
                "install-offer dismissed",
                "/memory install-offer dismissed",
                "Dismiss the first-start Hoppi install offer",
            ),
            arg_item(
                "install-hoppi",
                "/memory install-hoppi",
                "Show Hoppi install approval",
            ),
            arg_item(
                "install-hoppi --yes",
                "/memory install-hoppi --yes",
                "Install/update Hoppi after approval",
            ),
            arg_item("sync on", "/memory sync on", "Enable Hoppi sync setting"),
            arg_item("sync off", "/memory sync off", "Disable Hoppi sync"),
            arg_item(
                "sync now",
                "/memory sync now",
                "Run Hoppi pull + push sync now",
            ),
            arg_item(
                "sync pull",
                "/memory sync pull",
                "Pull shared Hoppi memory now",
            ),
            arg_item(
                "sync push",
                "/memory sync push",
                "Push shared Hoppi memory now",
            ),
            arg_item(
                "sync repo",
                "/memory sync repo ",
                "Set local Hoppi sync repository path",
            ),
            arg_item(
                "sync remote",
                "/memory sync remote ",
                "Set Hoppi sync git remote URL",
            ),
            arg_item(
                "sync remote none",
                "/memory sync remote none",
                "Clear Hoppi sync remote URL",
            ),
            arg_item(
                "sync pull-startup on",
                "/memory sync pull-startup on",
                "Pull Hoppi sync repo at startup",
            ),
            arg_item(
                "sync push-exit on",
                "/memory sync push-exit on",
                "Push Hoppi sync repo on exit",
            ),
            arg_item(
                "sync background 30m",
                "/memory sync background 30m",
                "Set background sync interval",
            ),
            arg_item(
                "sync encryption env",
                "/memory sync encryption env",
                "Use passphrase env var encryption",
            ),
            arg_item(
                "sync encryption file",
                "/memory sync encryption file",
                "Use passphrase file encryption",
            ),
            arg_item(
                "sync passphrase-env",
                "/memory sync passphrase-env OPPI_HOPPI_SYNC_PASSPHRASE",
                "Set sync passphrase env var",
            ),
            arg_item(
                "sync passphrase-file",
                "/memory sync passphrase-file ",
                "Set sync passphrase file path",
            ),
            arg_item(
                "sync conflict keep-both",
                "/memory sync conflict keep-both",
                "Keep both files on sync conflicts",
            ),
            arg_item(
                "sync conflict keep-local",
                "/memory sync conflict keep-local",
                "Prefer local files on sync conflicts",
            ),
            arg_item("dashboard", "/memory dashboard", "Open memory dashboard"),
            arg_item("list", "/memory list", "List cached Hoppi memories"),
            arg_item(
                "forget",
                "/memory forget ",
                "Forget a memory by id with --yes",
            ),
            arg_item("pin", "/memory pin ", "Pin a memory by id with --yes"),
            arg_item("unpin", "/memory unpin ", "Unpin a memory by id with --yes"),
            arg_item("settings", "/memory settings", "Open memory settings"),
            arg_item(
                "compaction",
                "/memory compaction ",
                "Configure idle/smart compaction thresholds",
            ),
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
            arg_item(
                "compact",
                "/memory compact ",
                "Record or derive a compaction summary",
            ),
        ],
        "/idle-compact" => vec![
            arg_item("status", "/idle-compact status", "Show compaction settings"),
            arg_item(
                "compact",
                "/idle-compact compact",
                "Create a todo-aware handoff now",
            ),
            arg_item("on", "/idle-compact on", "Enable idle compaction"),
            arg_item("off", "/idle-compact off", "Disable idle compaction"),
            arg_item(
                "idle-time",
                "/idle-compact idle-time ",
                "Set idle minutes: 2, 5, or 10",
            ),
            arg_item(
                "idle-threshold",
                "/idle-compact idle-threshold ",
                "Set idle context threshold",
            ),
            arg_item(
                "smart-threshold",
                "/idle-compact smart-threshold ",
                "Set smart compact threshold",
            ),
        ],
        "/agents" => vec![
            arg_item("list", "/agents list", "List discovered agents"),
            arg_item("show", "/agents show ", "Show an agent definition"),
            arg_item("create", "/agents create ", "Create a project agent"),
            arg_item("create user", "/agents create user ", "Create a user agent"),
            arg_item("edit", "/agents edit ", "Open or print an agent definition"),
            arg_item("delete", "/agents delete ", "Delete an agent with --yes"),
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
        "/skills" => vec![
            arg_item("list", "/skills list", "List active skill definitions"),
            arg_item("status", "/skills status", "Show /skill:name command state"),
            arg_item(
                "commands on",
                "/skills commands on",
                "Enable /skill:name commands",
            ),
            arg_item(
                "commands off",
                "/skills commands off",
                "Disable /skill:name commands",
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
        "/models" => vec![
            arg_item(
                "--all",
                "/models --all ",
                "Include unavailable registry entries",
            ),
            arg_item("<filter>", "/models ", "Filter available model ids"),
        ],
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
        "/tree" => vec![
            arg_item(
                "entries",
                "/tree entries",
                "Show completed transcript entries for the active thread",
            ),
            arg_item("status", "/tree status", "Show the configured tree filter"),
            arg_item(
                "filter default",
                "/tree filter default",
                "Default active-session tree",
            ),
            arg_item(
                "filter no-tools",
                "/tree filter no-tools",
                "Pi-compatible no-tools filter",
            ),
            arg_item(
                "filter user-only",
                "/tree filter user-only",
                "Pi-compatible user-only filter",
            ),
            arg_item(
                "filter labeled-only",
                "/tree filter labeled-only",
                "Show titled/labeled sessions",
            ),
            arg_item(
                "filter all",
                "/tree filter all",
                "Show all sessions, including archived",
            ),
            arg_item("fold", "/tree fold ", "Collapse a fork subtree"),
            arg_item("unfold", "/tree unfold ", "Expand a fork subtree"),
            arg_item("toggle", "/tree toggle ", "Toggle a fork subtree"),
        ],
        "/sessions" => vec![
            arg_item("rename", "/sessions rename ", "Rename a saved session"),
            arg_item("delete", "/sessions delete ", "Archive a saved session"),
            arg_item("fold", "/sessions fold ", "Collapse a fork subtree"),
            arg_item("unfold", "/sessions unfold ", "Expand a fork subtree"),
            arg_item("toggle", "/sessions toggle ", "Toggle a fork subtree"),
            arg_item(
                "entries",
                "/sessions entries",
                "Show completed transcript entries for the active thread",
            ),
            arg_item(
                "status",
                "/sessions status",
                "Show the configured tree filter",
            ),
            arg_item(
                "filter default",
                "/sessions filter default",
                "Default session-tree filter",
            ),
            arg_item(
                "filter all",
                "/sessions filter all",
                "Show all sessions, including archived",
            ),
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
    slash_palette_for_buffer_with_skills(buffer, selected, &[], false)
}

fn slash_palette_for_buffer_with_skills(
    buffer: &str,
    selected: usize,
    skills: &[ResolvedSkill],
    enable_skill_commands: bool,
) -> Option<SlashPalette> {
    let parsed = parse_slash_buffer(buffer)?;
    if !parsed.has_args {
        let items = slash_command_palette_items_with_skills(
            parsed.command_token,
            skills,
            enable_skill_commands,
        );
        if items.is_empty() {
            return None;
        }
        let selected = selected.min(items.len().saturating_sub(1));
        let command_count = visible_slash_command_specs().len()
            + if enable_skill_commands {
                skills.len()
            } else {
                0
            };
        return Some(SlashPalette {
            mode: SlashPaletteMode::Commands,
            query: parsed.command_token.to_string(),
            title: format!("slash commands ({command_count})"),
            hint: "type to filter • ↑↓/PgUp/PgDn choose • Tab completes • Enter chooses/runs"
                .to_string(),
            items,
            selected,
        });
    }

    let Some(spec) = find_slash_command_spec(parsed.command_token) else {
        let items = slash_command_palette_items_with_skills(
            parsed.command_token,
            skills,
            enable_skill_commands,
        );
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
    let mut palette = slash_palette_for_buffer_with_skills(
        buffer,
        selected,
        &session.skill_command_cache,
        session.enable_skill_commands,
    )?;
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
        "/permissions" => {
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

