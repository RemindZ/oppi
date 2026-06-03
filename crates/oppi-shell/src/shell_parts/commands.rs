use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum PermissionCommandRoute {
    Status,
    History,
    ReviewContext,
    ClearSession,
    ValidateSandbox,
    SetSandboxRootsHere,
    SetSandboxRootsDefault,
    SetMode(PermissionMode),
    SetReviewTimeoutMs(u64),
    SetReviewerModel(Option<String>),
    SetSandboxInternet(bool),
    SetSandboxSsh(bool),
    SetSandboxProjectRoots(Vec<String>),
    SetSandboxReadableRoots(Vec<String>),
    SetSandboxWritableRoots(Vec<String>),
    EditSandboxRoots {
        kind: PermissionSandboxRootListKind,
        edit: PermissionSandboxRootListEdit,
        roots: Vec<String>,
    },
    SetProtectedPatterns(Vec<String>),
}

fn permissions_usage() -> &'static str {
    "usage: /permissions [status|history|context|timeout <seconds>|reviewer-model <auto|provider/model>|sandbox validate|sandbox roots <here|default>|sandbox internet <on|off>|sandbox ssh <on|off>|sandbox project <paths|default>|sandbox project add|remove <paths>|sandbox read <paths|default>|sandbox read add|remove <paths>|sandbox write <paths|default>|sandbox write add|remove <paths>|sandbox protected <patterns|default>|clear-session|read-only|default|auto-review|full-access]"
}

pub(super) fn permissions_command_route(args: &str) -> Result<PermissionCommandRoute, String> {
    let parts = args.split_whitespace().collect::<Vec<_>>();
    match parts.as_slice() {
        [] | ["status"] => Ok(PermissionCommandRoute::Status),
        ["history"] => Ok(PermissionCommandRoute::History),
        ["context"] | ["review-context"] | ["guardian-context"] => {
            Ok(PermissionCommandRoute::ReviewContext)
        }
        ["clear-session"] | ["clear"] => Ok(PermissionCommandRoute::ClearSession),
        ["sandbox"] | ["sandbox", "status"] => Ok(PermissionCommandRoute::Status),
        ["sandbox", "validate"] | ["sandbox", "check"] => {
            Ok(PermissionCommandRoute::ValidateSandbox)
        }
        ["sandbox", "roots", "here" | "cwd" | "current" | "workspace"] => {
            Ok(PermissionCommandRoute::SetSandboxRootsHere)
        }
        ["sandbox", "roots", "default" | "clear" | "reset"] => {
            Ok(PermissionCommandRoute::SetSandboxRootsDefault)
        }
        ["sandbox", "internet", value] => Ok(PermissionCommandRoute::SetSandboxInternet(
            parse_sandbox_bool(value)?,
        )),
        ["sandbox", "ssh", value] => Ok(PermissionCommandRoute::SetSandboxSsh(parse_sandbox_bool(
            value,
        )?)),
        [
            "sandbox",
            "project",
            edit @ ("add" | "append" | "remove" | "rm" | "delete"),
            rest @ ..,
        ] => Ok(PermissionCommandRoute::EditSandboxRoots {
            kind: PermissionSandboxRootListKind::Project,
            edit: parse_sandbox_root_list_edit(edit)?,
            roots: parse_sandbox_root_edit_paths(&rest.join(" "))?,
        }),
        ["sandbox", "project", rest @ ..] => Ok(PermissionCommandRoute::SetSandboxProjectRoots(
            parse_sandbox_roots(&rest.join(" "))?,
        )),
        [
            "sandbox",
            "read" | "readable",
            edit @ ("add" | "append" | "remove" | "rm" | "delete"),
            rest @ ..,
        ] => Ok(PermissionCommandRoute::EditSandboxRoots {
            kind: PermissionSandboxRootListKind::Readable,
            edit: parse_sandbox_root_list_edit(edit)?,
            roots: parse_sandbox_root_edit_paths(&rest.join(" "))?,
        }),
        ["sandbox", "read" | "readable", rest @ ..] => Ok(
            PermissionCommandRoute::SetSandboxReadableRoots(parse_sandbox_roots(&rest.join(" "))?),
        ),
        [
            "sandbox",
            "write" | "writable",
            edit @ ("add" | "append" | "remove" | "rm" | "delete"),
            rest @ ..,
        ] => Ok(PermissionCommandRoute::EditSandboxRoots {
            kind: PermissionSandboxRootListKind::Writable,
            edit: parse_sandbox_root_list_edit(edit)?,
            roots: parse_sandbox_root_edit_paths(&rest.join(" "))?,
        }),
        ["sandbox", "write" | "writable", rest @ ..] => Ok(
            PermissionCommandRoute::SetSandboxWritableRoots(parse_sandbox_roots(&rest.join(" "))?),
        ),
        ["sandbox", "protected"] => Ok(PermissionCommandRoute::SetProtectedPatterns(Vec::new())),
        ["sandbox", "protected", rest @ ..] => Ok(PermissionCommandRoute::SetProtectedPatterns(
            parse_protected_patterns(&rest.join(" "))?,
        )),
        ["protected", rest @ ..] => Ok(PermissionCommandRoute::SetProtectedPatterns(
            parse_protected_patterns(&rest.join(" "))?,
        )),
        ["timeout", seconds] => {
            let seconds = seconds
                .parse::<u64>()
                .ok()
                .filter(|value| *value > 0)
                .ok_or_else(|| permissions_usage().to_string())?;
            let timeout_ms = seconds
                .checked_mul(1_000)
                .ok_or_else(|| permissions_usage().to_string())?;
            Ok(PermissionCommandRoute::SetReviewTimeoutMs(timeout_ms))
        }
        ["reviewer-model", "auto"] => Ok(PermissionCommandRoute::SetReviewerModel(None)),
        ["reviewer-model", model] => {
            let model = model.trim();
            let valid_provider_model = model.split_once('/').is_some_and(|(provider, model)| {
                !provider.trim().is_empty() && !model.trim().is_empty()
            });
            if valid_provider_model {
                Ok(PermissionCommandRoute::SetReviewerModel(Some(
                    model.to_string(),
                )))
            } else {
                Err(permissions_usage().to_string())
            }
        }
        [mode] => parse_permission_mode(mode)
            .map(PermissionCommandRoute::SetMode)
            .ok_or_else(|| permissions_usage().to_string()),
        _ => Err(permissions_usage().to_string()),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PermissionSandboxRootListKind {
    Project,
    Readable,
    Writable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PermissionSandboxRootListEdit {
    Add,
    Remove,
}

fn parse_sandbox_bool(raw: &str) -> Result<bool, String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "on" | "true" | "allow" | "allowed" | "yes" => Ok(true),
        "off" | "false" | "block" | "blocked" | "no" => Ok(false),
        _ => Err(permissions_usage().to_string()),
    }
}

fn parse_sandbox_root_list_edit(raw: &str) -> Result<PermissionSandboxRootListEdit, String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "add" | "append" => Ok(PermissionSandboxRootListEdit::Add),
        "remove" | "rm" | "delete" => Ok(PermissionSandboxRootListEdit::Remove),
        _ => Err(permissions_usage().to_string()),
    }
}

fn parse_sandbox_roots(raw: &str) -> Result<Vec<String>, String> {
    let raw = raw.trim();
    if raw.is_empty() || raw.eq_ignore_ascii_case("default") || raw.eq_ignore_ascii_case("clear") {
        return Ok(Vec::new());
    }
    parse_csv_or_whitespace_list(raw)
}

fn parse_sandbox_root_edit_paths(raw: &str) -> Result<Vec<String>, String> {
    let roots = parse_sandbox_roots(raw)?;
    if roots.is_empty() {
        Err(permissions_usage().to_string())
    } else {
        Ok(roots)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PermissionSandboxRootPreset {
    Here,
    Default,
}

pub(super) fn apply_permission_sandbox_root_preset(
    settings: &mut PermissionSandboxSettings,
    preset: PermissionSandboxRootPreset,
    cwd: &str,
) {
    match preset {
        PermissionSandboxRootPreset::Here => {
            let roots = vec![cwd.to_string()];
            settings.project_roots = roots.clone();
            settings.readable_roots = roots.clone();
            settings.writable_roots = roots;
        }
        PermissionSandboxRootPreset::Default => {
            settings.project_roots.clear();
            settings.readable_roots.clear();
            settings.writable_roots.clear();
        }
    }
}

pub(super) fn apply_permission_sandbox_root_list_edit(
    roots: &mut Vec<String>,
    edit: PermissionSandboxRootListEdit,
    requested: &[String],
) -> bool {
    let requested = normalize_string_slice(requested);
    let mut next = normalize_string_slice(roots);
    match edit {
        PermissionSandboxRootListEdit::Add => {
            for root in requested {
                if !next.contains(&root) {
                    next.push(root);
                }
            }
        }
        PermissionSandboxRootListEdit::Remove => {
            next.retain(|root| !requested.contains(root));
        }
    }
    let changed = roots.as_slice() != next.as_slice();
    *roots = next;
    changed
}

pub(super) fn permission_sandbox_root_list_mut(
    settings: &mut PermissionSandboxSettings,
    kind: PermissionSandboxRootListKind,
) -> &mut Vec<String> {
    match kind {
        PermissionSandboxRootListKind::Project => &mut settings.project_roots,
        PermissionSandboxRootListKind::Readable => &mut settings.readable_roots,
        PermissionSandboxRootListKind::Writable => &mut settings.writable_roots,
    }
}

pub(super) fn permission_sandbox_root_list_labels(
    kind: PermissionSandboxRootListKind,
) -> (&'static str, &'static str) {
    match kind {
        PermissionSandboxRootListKind::Project => ("project", "default"),
        PermissionSandboxRootListKind::Readable => ("readable", "project roots"),
        PermissionSandboxRootListKind::Writable => ("writable", "project roots"),
    }
}

fn parse_protected_patterns(raw: &str) -> Result<Vec<String>, String> {
    let raw = raw.trim();
    if raw.is_empty() || raw.eq_ignore_ascii_case("default") || raw.eq_ignore_ascii_case("clear") {
        return Ok(Vec::new());
    }
    parse_csv_or_whitespace_list(raw)
}

fn parse_csv_or_whitespace_list(raw: &str) -> Result<Vec<String>, String> {
    let parts = if raw.contains(',') {
        raw.split(',').collect::<Vec<_>>()
    } else {
        raw.split_whitespace().collect::<Vec<_>>()
    };
    let mut patterns = Vec::new();
    for part in parts {
        let item = part.trim();
        if item.is_empty() {
            continue;
        }
        if item.len() > 240 || item.contains('\0') {
            return Err(permissions_usage().to_string());
        }
        let item = item.to_string();
        if !patterns.contains(&item) {
            patterns.push(item);
        }
    }
    Ok(patterns)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum AskUserCommandRoute {
    Status,
    SetTimeoutMs(u64),
}

fn ask_user_usage() -> &'static str {
    "usage: /ask-user [status|timeout <off|1|2|5|10>]"
}

pub(super) fn ask_user_timeout_minutes_to_ms(minutes: u64) -> Option<u64> {
    ASK_USER_ALLOWED_TIMEOUT_MINUTES
        .contains(&minutes)
        .then(|| minutes.saturating_mul(60_000))
}

pub(super) fn ask_user_timeout_ms_to_minutes(timeout_ms: u64) -> Option<u64> {
    if timeout_ms == 0 {
        return Some(0);
    }
    (timeout_ms % 60_000 == 0)
        .then_some(timeout_ms / 60_000)
        .filter(|minutes| ASK_USER_ALLOWED_TIMEOUT_MINUTES.contains(minutes))
}

pub(super) fn ask_user_command_route(args: &str) -> Result<AskUserCommandRoute, String> {
    let parts = args.split_whitespace().collect::<Vec<_>>();
    match parts.as_slice() {
        [] | ["status"] => Ok(AskUserCommandRoute::Status),
        ["timeout", "off"] => Ok(AskUserCommandRoute::SetTimeoutMs(0)),
        ["timeout", minutes] => {
            let minutes = minutes
                .parse::<u64>()
                .ok()
                .and_then(ask_user_timeout_minutes_to_ms)
                .ok_or_else(|| ask_user_usage().to_string())?;
            Ok(AskUserCommandRoute::SetTimeoutMs(minutes))
        }
        _ => Err(ask_user_usage().to_string()),
    }
}

fn follow_up_usage() -> &'static str {
    "usage: /follow-up [status|mode <all|one-at-a-time>]"
}

pub(super) fn parse_follow_up_mode(raw: &str) -> Option<FollowUpMode> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "all" => Some(FollowUpMode::All),
        "one" | "one-at-a-time" | "one_at_a_time" | "one-at-time" => Some(FollowUpMode::OneAtATime),
        _ => None,
    }
}

pub(super) fn follow_up_command_route(args: &str) -> Result<FollowUpCommandRoute, String> {
    let parts = args.split_whitespace().collect::<Vec<_>>();
    match parts.as_slice() {
        [] | ["status"] => Ok(FollowUpCommandRoute::Status),
        ["mode", mode] => parse_follow_up_mode(mode)
            .map(FollowUpCommandRoute::SetMode)
            .ok_or_else(|| follow_up_usage().to_string()),
        [mode] => parse_follow_up_mode(mode)
            .map(FollowUpCommandRoute::SetMode)
            .ok_or_else(|| follow_up_usage().to_string()),
        _ => Err(follow_up_usage().to_string()),
    }
}

pub(super) fn format_follow_up_status(mode: FollowUpMode, queued: usize) -> String {
    format!("follow-up: mode {}; queued {}", mode.as_str(), queued)
}

pub(super) fn follow_up_mode_toggle_arg(mode: FollowUpMode) -> &'static str {
    match mode {
        FollowUpMode::All => "one-at-a-time",
        FollowUpMode::OneAtATime => "all",
    }
}

pub(super) fn steer_usage() -> &'static str {
    "usage: /steer [status|mode <all|one-at-a-time>|<text>]"
}

pub(super) fn parse_steering_mode(raw: &str) -> Option<SteeringMode> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "all" => Some(SteeringMode::All),
        "one" | "one-at-a-time" | "one_at_a_time" | "one-at-time" => Some(SteeringMode::OneAtATime),
        _ => None,
    }
}

pub(super) fn steer_command_route(args: &str) -> Result<SteerCommandRoute, String> {
    let trimmed = args.trim();
    let parts = trimmed.split_whitespace().collect::<Vec<_>>();
    match parts.as_slice() {
        [] | ["status"] => Ok(SteerCommandRoute::Status),
        ["mode", mode] => parse_steering_mode(mode)
            .map(SteerCommandRoute::SetMode)
            .ok_or_else(|| steer_usage().to_string()),
        ["mode", ..] => Err(steer_usage().to_string()),
        _ => Ok(SteerCommandRoute::Send(trimmed.to_string())),
    }
}

pub(super) fn format_steering_status(mode: SteeringMode, queued: usize) -> String {
    format!("steer: mode {}; queued {}", mode.as_str(), queued)
}

pub(super) fn steering_mode_toggle_arg(mode: SteeringMode) -> &'static str {
    match mode {
        SteeringMode::All => "one-at-a-time",
        SteeringMode::OneAtATime => "all",
    }
}

fn startup_usage() -> &'static str {
    "usage: /startup [status|quiet <on|off>]"
}

pub(super) fn startup_command_route(args: &str) -> Result<StartupCommandRoute, String> {
    let parts = args.split_whitespace().collect::<Vec<_>>();
    match parts.as_slice() {
        [] | ["status"] => Ok(StartupCommandRoute::Status),
        ["quiet", value] => parse_footer_bool(value)
            .map(StartupCommandRoute::SetQuietStartup)
            .map_err(|_| startup_usage().to_string()),
        _ => Err(startup_usage().to_string()),
    }
}

pub(super) fn format_startup_status(quiet_startup: bool) -> String {
    format!("startup: quiet {}", enabled_label(quiet_startup))
}

pub(super) fn quiet_startup_toggle_arg(quiet_startup: bool) -> &'static str {
    if quiet_startup { "off" } else { "on" }
}

fn cursor_usage() -> &'static str {
    "usage: /cursor [status|hardware <on|off>]"
}

pub(super) fn cursor_command_route(args: &str) -> Result<CursorCommandRoute, String> {
    let parts = args.split_whitespace().collect::<Vec<_>>();
    match parts.as_slice() {
        [] | ["status"] | ["settings"] => Ok(CursorCommandRoute::Status),
        ["hardware", value] | ["hardware-cursor", value] | ["cursor", value] => {
            parse_footer_bool(value)
                .map(CursorCommandRoute::SetHardwareCursor)
                .map_err(|_| cursor_usage().to_string())
        }
        _ => Err(cursor_usage().to_string()),
    }
}

pub(super) fn format_cursor_status(show_hardware_cursor: bool) -> String {
    format!("cursor: hardware {}", enabled_label(show_hardware_cursor))
}

pub(super) fn hardware_cursor_toggle_arg(show_hardware_cursor: bool) -> &'static str {
    if show_hardware_cursor { "off" } else { "on" }
}

pub(super) fn terminal_cursor_setup_sequence(show_hardware_cursor: bool) -> &'static str {
    if show_hardware_cursor {
        ""
    } else {
        "\x1b[?25l"
    }
}

fn terminal_usage() -> &'static str {
    "usage: /terminal [status|progress <on|off>|clear-on-shrink <on|off>]"
}

pub(super) fn terminal_command_route(args: &str) -> Result<TerminalCommandRoute, String> {
    let parts = args.split_whitespace().collect::<Vec<_>>();
    match parts.as_slice() {
        [] | ["status"] | ["settings"] => Ok(TerminalCommandRoute::Status),
        ["progress", value] | ["terminal-progress", value] => parse_footer_bool(value)
            .map(TerminalCommandRoute::SetProgress)
            .map_err(|_| terminal_usage().to_string()),
        ["clear-on-shrink", value] | ["shrink", value] => parse_footer_bool(value)
            .map(TerminalCommandRoute::SetClearOnShrink)
            .map_err(|_| terminal_usage().to_string()),
        _ => Err(terminal_usage().to_string()),
    }
}

pub(super) fn format_terminal_status(
    show_terminal_progress: bool,
    clear_on_shrink: bool,
) -> String {
    format!(
        "terminal: progress {}, clear-on-shrink {}",
        enabled_label(show_terminal_progress),
        enabled_label(clear_on_shrink)
    )
}

pub(super) fn terminal_progress_toggle_arg(show_terminal_progress: bool) -> &'static str {
    if show_terminal_progress { "off" } else { "on" }
}

pub(super) fn terminal_clear_on_shrink_toggle_arg(clear_on_shrink: bool) -> &'static str {
    if clear_on_shrink { "off" } else { "on" }
}

pub(super) fn terminal_progress_sequence(active: bool) -> &'static str {
    if active {
        "\x1b]9;4;3\x07"
    } else {
        "\x1b]9;4;0;\x07"
    }
}

pub(super) fn terminal_progress_output(
    enabled: bool,
    active: bool,
    json_output: bool,
) -> Option<&'static str> {
    if enabled && !json_output {
        Some(terminal_progress_sequence(active))
    } else {
        None
    }
}

pub(super) fn editor_usage() -> &'static str {
    "usage: /editor [status|padding <0|1|2|3>|double-escape <tree|fork|none>]"
}

pub(super) fn editor_command_route(args: &str) -> Result<EditorCommandRoute, String> {
    let parts = args.split_whitespace().collect::<Vec<_>>();
    match parts.as_slice() {
        [] | ["status"] | ["settings"] => Ok(EditorCommandRoute::Status),
        ["padding", value] | ["padding-x", value] | ["pad", value] => parse_editor_padding_x(value)
            .map(EditorCommandRoute::SetPaddingX)
            .map_err(|_| editor_usage().to_string()),
        ["double-escape", value] | ["double_escape", value] | ["escape", value] => {
            parse_double_escape_action(value).map(EditorCommandRoute::SetDoubleEscapeAction)
        }
        _ => Err(editor_usage().to_string()),
    }
}

fn parse_editor_padding_x(raw: &str) -> Result<usize, String> {
    raw.parse::<usize>()
        .ok()
        .and_then(normalize_editor_padding_x)
        .ok_or_else(|| editor_usage().to_string())
}

pub(super) fn normalize_editor_padding_x(value: usize) -> Option<usize> {
    EDITOR_PADDING_X_VALUES
        .iter()
        .copied()
        .find(|allowed| *allowed == value)
}

pub(super) fn parse_double_escape_action(raw: &str) -> Result<DoubleEscapeAction, String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "tree" => Ok(DoubleEscapeAction::Tree),
        "fork" | "branch" => Ok(DoubleEscapeAction::Fork),
        "none" | "off" | "disabled" => Ok(DoubleEscapeAction::None),
        _ => Err(editor_usage().to_string()),
    }
}

pub(super) fn format_editor_status(
    padding_x: usize,
    double_escape_action: DoubleEscapeAction,
) -> String {
    format!(
        "editor: padding {padding_x}, double-escape {}",
        double_escape_action.as_str()
    )
}

pub(super) fn editor_padding_x_toggle_arg(current: usize) -> usize {
    EDITOR_PADDING_X_VALUES
        .iter()
        .copied()
        .find(|value| *value > current)
        .unwrap_or(EDITOR_PADDING_X_VALUES[0])
}

pub(super) fn double_escape_action_toggle_arg(current: DoubleEscapeAction) -> &'static str {
    match current {
        DoubleEscapeAction::Tree => "fork",
        DoubleEscapeAction::Fork => "none",
        DoubleEscapeAction::None => "tree",
    }
}

pub(super) const TREE_ENTRY_DEFAULT_LIMIT: usize = 5_000;
const TREE_ENTRY_MAX_LIMIT: usize = 5_000;

fn tree_usage() -> &'static str {
    "usage: /tree [entries [after|--after <event-id>] [limit|--limit <1..5000>]|status|filter <default|no-tools|user-only|labeled-only|all>|fork <event-id> [title]|edit <event-id> <replacement>|resubmit <event-id> <replacement>|label <event-id> <label>[,<label>...]|unlabel <event-id>|fold <thread-id>|unfold <thread-id>|toggle <thread-id>|rename <thread-id> <title>|delete <thread-id>]"
}

pub(super) fn parse_tree_filter_mode(raw: &str) -> Result<TreeFilterMode, String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "default" => Ok(TreeFilterMode::Default),
        "no-tools" | "notools" | "no_tools" => Ok(TreeFilterMode::NoTools),
        "user-only" | "user" | "user_only" => Ok(TreeFilterMode::UserOnly),
        "labeled-only" | "labeled" | "label" | "labels" | "labeled_only" => {
            Ok(TreeFilterMode::LabeledOnly)
        }
        "all" => Ok(TreeFilterMode::All),
        _ => Err(tree_usage().to_string()),
    }
}

pub(super) fn tree_filter_mode_toggle_arg(current: TreeFilterMode) -> &'static str {
    let index = TREE_FILTER_MODES
        .iter()
        .position(|mode| *mode == current)
        .unwrap_or(0);
    TREE_FILTER_MODES[(index + 1) % TREE_FILTER_MODES.len()].as_str()
}

pub(super) fn tree_command_route_for_command(
    command: &str,
    args: &[&str],
) -> Result<TreeCommandRoute, String> {
    match command {
        "/tree" => match args {
            [] => Ok(TreeCommandRoute::Entries {
                after: 0,
                limit: None,
            }),
            ["sessions"] | ["threads"] => Ok(TreeCommandRoute::Show),
            ["sessions", rest @ ..] | ["threads", rest @ ..] => tree_command_route(rest),
            _ => tree_command_route(args),
        },
        _ => tree_command_route(args),
    }
}

pub(super) fn tree_command_route(args: &[&str]) -> Result<TreeCommandRoute, String> {
    match args {
        [] => Ok(TreeCommandRoute::Show),
        ["status"] | ["settings"] => Ok(TreeCommandRoute::Status),
        [head, rest @ ..] if matches!(*head, "entries" | "messages" | "items") => {
            parse_tree_entries_args(rest)
        }
        ["filter", value] | ["tree-filter", value] => {
            parse_tree_filter_mode(value).map(TreeCommandRoute::SetFilter)
        }
        ["fork" | "branch", event_id, title @ ..] => {
            let title = if title.is_empty() {
                None
            } else {
                let title = title.join(" ");
                if title.trim().is_empty() {
                    return Err(tree_usage().to_string());
                }
                Some(title)
            };
            Ok(TreeCommandRoute::Fork {
                event_id: parse_tree_entry_cursor(event_id)?,
                title,
            })
        }
        ["edit" | "resubmit" | "rewrite", event_id, input @ ..] if !input.is_empty() => {
            let input = input.join(" ");
            if input.trim().is_empty() {
                Err(tree_usage().to_string())
            } else {
                Ok(TreeCommandRoute::Edit {
                    event_id: parse_tree_entry_cursor(event_id)?,
                    input,
                })
            }
        }
        ["label", event_id, labels @ ..] if !labels.is_empty() => Ok(TreeCommandRoute::Label {
            event_id: parse_tree_entry_cursor(event_id)?,
            labels: parse_tree_labels(labels)?,
        }),
        ["unlabel", event_id] | ["clear-label", event_id] | ["clear-labels", event_id] => {
            Ok(TreeCommandRoute::Label {
                event_id: parse_tree_entry_cursor(event_id)?,
                labels: Vec::new(),
            })
        }
        ["fold", thread_id] => Ok(TreeCommandRoute::Fold((*thread_id).to_string())),
        ["unfold", thread_id] => Ok(TreeCommandRoute::Unfold((*thread_id).to_string())),
        ["toggle", thread_id] => Ok(TreeCommandRoute::Toggle((*thread_id).to_string())),
        ["rename", thread_id, title @ ..] if !title.is_empty() => {
            let title = title.join(" ");
            if title.trim().is_empty() {
                Err(tree_usage().to_string())
            } else {
                Ok(TreeCommandRoute::Rename {
                    thread_id: (*thread_id).to_string(),
                    title,
                })
            }
        }
        ["delete", thread_id] | ["archive", thread_id] => {
            Ok(TreeCommandRoute::Archive((*thread_id).to_string()))
        }
        _ => Err(tree_usage().to_string()),
    }
}

fn parse_tree_entries_args(args: &[&str]) -> Result<TreeCommandRoute, String> {
    let mut after = 0;
    let mut limit = None;
    let mut index = 0;
    while index < args.len() {
        match args[index] {
            "after" | "--after" => {
                index += 1;
                let Some(event_id) = args.get(index) else {
                    return Err(tree_usage().to_string());
                };
                after = parse_tree_entry_cursor(event_id)?;
            }
            "limit" | "--limit" => {
                index += 1;
                let Some(raw_limit) = args.get(index) else {
                    return Err(tree_usage().to_string());
                };
                limit = Some(parse_tree_entry_limit(raw_limit)?);
            }
            _ => return Err(tree_usage().to_string()),
        }
        index += 1;
    }
    Ok(TreeCommandRoute::Entries { after, limit })
}

fn parse_tree_entry_cursor(raw: &str) -> Result<u64, String> {
    raw.parse::<u64>().map_err(|_| tree_usage().to_string())
}

fn parse_tree_entry_limit(raw: &str) -> Result<usize, String> {
    match raw.parse::<usize>() {
        Ok(value) if (1..=TREE_ENTRY_MAX_LIMIT).contains(&value) => Ok(value),
        _ => Err(tree_usage().to_string()),
    }
}

fn parse_tree_labels(raw_labels: &[&str]) -> Result<Vec<String>, String> {
    let labels = raw_labels
        .iter()
        .flat_map(|raw| raw.split(','))
        .map(str::trim)
        .filter(|label| !label.is_empty())
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    if labels.is_empty() {
        Err(tree_usage().to_string())
    } else {
        Ok(labels)
    }
}

pub(super) fn format_tree_status(mode: TreeFilterMode) -> String {
    format!("tree: filter {}", mode.as_str())
}

fn tree_item_visible(entry: &ThreadItemEntry, filter_mode: TreeFilterMode) -> bool {
    match filter_mode {
        TreeFilterMode::All => true,
        TreeFilterMode::UserOnly => matches!(entry.item.kind, ItemKind::UserMessage { .. }),
        TreeFilterMode::LabeledOnly => !entry.labels.is_empty(),
        TreeFilterMode::Default | TreeFilterMode::NoTools => !matches!(
            entry.item.kind,
            ItemKind::ToolCall(_) | ItemKind::ToolResult(_)
        ),
    }
}

pub(super) fn tool_result_status_label(status: ToolResultStatus) -> &'static str {
    match status {
        ToolResultStatus::Ok => "ok",
        ToolResultStatus::Denied => "denied",
        ToolResultStatus::Error => "error",
        ToolResultStatus::Aborted => "aborted",
    }
}

pub(super) fn tree_item_kind_name(kind: &ItemKind) -> &'static str {
    match kind {
        ItemKind::UserMessage { .. } => "user",
        ItemKind::AssistantMessage { .. } => "assistant",
        ItemKind::Reasoning { .. } => "reasoning",
        ItemKind::ToolCall(_) => "tool",
        ItemKind::ToolResult(_) => "toolResult",
        ItemKind::ApprovalRequest(_) => "approvalRequest",
        ItemKind::ApprovalDecision(_) => "approvalDecision",
        ItemKind::QuestionRequest(_) => "questionRequest",
        ItemKind::QuestionResponse(_) => "questionResponse",
        ItemKind::Diagnostic(_) => "diagnostic",
        ItemKind::HandoffSummary { .. } => "handoffSummary",
    }
}

pub(super) fn tree_item_label(kind: &ItemKind) -> String {
    match kind {
        ItemKind::UserMessage { .. } => "user".to_string(),
        ItemKind::AssistantMessage { .. } => "assistant".to_string(),
        ItemKind::Reasoning { .. } => "reasoning".to_string(),
        ItemKind::ToolCall(call) => format!("tool {}", call.name),
        ItemKind::ToolResult(result) => format!("tool-result {}", result.call_id),
        ItemKind::ApprovalRequest(request) => format!("approval-request {}", request.id),
        ItemKind::ApprovalDecision(decision) => {
            format!("approval-decision {}", decision.request_id)
        }
        ItemKind::QuestionRequest(request) => format!("question-request {}", request.id),
        ItemKind::QuestionResponse(response) => {
            format!("question-response {}", response.request_id)
        }
        ItemKind::Diagnostic(diagnostic) => format!("diagnostic {:?}", diagnostic.level),
        ItemKind::HandoffSummary { .. } => "handoff-summary".to_string(),
    }
}

fn compact_tree_preview(raw: &str) -> String {
    let mut preview = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if preview.chars().count() > 96 {
        preview = preview.chars().take(93).collect::<String>();
        preview.push_str("...");
    }
    preview
}

pub(super) fn tree_item_preview(kind: &ItemKind) -> String {
    match kind {
        ItemKind::UserMessage { text }
        | ItemKind::AssistantMessage { text }
        | ItemKind::Reasoning { text } => compact_tree_preview(text),
        ItemKind::ToolCall(call) => compact_tree_preview(&call.arguments.to_string()),
        ItemKind::ToolResult(result) => result
            .error
            .as_deref()
            .or(result.output.as_deref())
            .map(compact_tree_preview)
            .filter(|preview| !preview.is_empty())
            .unwrap_or_else(|| tool_result_status_label(result.status).to_string()),
        ItemKind::ApprovalRequest(request) => compact_tree_preview(&request.reason),
        ItemKind::ApprovalDecision(decision) => decision
            .message
            .as_deref()
            .map(compact_tree_preview)
            .filter(|preview| !preview.is_empty())
            .unwrap_or_else(|| format!("{:?}", decision.decision).to_ascii_lowercase()),
        ItemKind::QuestionRequest(request) => compact_tree_preview(&request.prompt),
        ItemKind::QuestionResponse(response) => compact_tree_preview(&response.answer),
        ItemKind::Diagnostic(diagnostic) => compact_tree_preview(&diagnostic.message),
        ItemKind::HandoffSummary { summary } => compact_tree_preview(summary),
    }
}

pub(super) fn thread_item_tree_json_from_entries(entries: &[ThreadItemEntry]) -> Vec<Value> {
    entries
        .iter()
        .map(|entry| {
            let item = &entry.item;
            json!({
                "eventId": entry.event_id,
                "parentEventId": entry.parent_event_id,
                "childEventIds": entry.child_event_ids,
                "completedAtMs": entry.completed_at_ms,
                "id": item.id.clone(),
                "turnId": item.turn_id.clone(),
                "kind": tree_item_kind_name(&item.kind),
                "label": tree_item_label(&item.kind),
                "labels": entry.labels,
                "preview": tree_item_preview(&item.kind),
            })
        })
        .collect()
}

pub(super) fn thread_item_tree_next_after(
    entries: &[ThreadItemEntry],
    total_count: u64,
) -> Option<u64> {
    if entries.len() as u64 >= total_count {
        return None;
    }
    entries.last().map(|entry| entry.event_id)
}

pub(super) fn format_thread_item_branch(branch: &ThreadItemBranch) -> String {
    let mut parts = vec![format!(
        "branch: {} depth {}",
        branch.thread_id, branch.depth
    )];
    if !branch.path.is_empty() {
        parts.push(format!("path {}", branch.path.join(" > ")));
    }
    if let Some(parent_thread_id) = &branch.parent_thread_id {
        parts.push(format!("parent {parent_thread_id}"));
    }
    if !branch.child_thread_ids.is_empty() {
        parts.push(format!("children {}", branch.child_thread_ids.join(",")));
    }
    if !branch.sibling_thread_ids.is_empty() {
        parts.push(format!("siblings {}", branch.sibling_thread_ids.join(",")));
    }
    parts.join(" ")
}

pub(super) fn format_thread_item_labels_result(result: &ThreadItemLabelsResult) -> String {
    if result.labels.is_empty() {
        format!("tree label: event {} cleared", result.event_id)
    } else {
        format!(
            "tree label: event {} labels {}",
            result.event_id,
            result.labels.join(",")
        )
    }
}

pub(super) fn format_thread_item_tree_from_entries(
    entries: &[ThreadItemEntry],
    branch: Option<&ThreadItemBranch>,
    filter_mode: TreeFilterMode,
    after: u64,
    total_count: u64,
    next_after: Option<u64>,
) -> String {
    let mut lines = vec![
        "entries:".to_string(),
        format!("filter: {}", filter_mode.as_str()),
        format!("shown: {}/{}", entries.len(), total_count),
    ];
    if let Some(branch) = branch {
        lines.push(format_thread_item_branch(branch));
    }
    if after > 0 {
        lines.push(format!("after: {after}"));
    }
    if let Some(next_after) = next_after {
        lines.push(format!("next: /tree entries after {next_after}"));
    }

    let mut count = 0;
    for entry in entries
        .iter()
        .filter(|entry| tree_item_visible(entry, filter_mode))
    {
        count += 1;
        let item = &entry.item;
        let suffix = tree_item_preview(&item.kind);
        let suffix = if suffix.is_empty() {
            String::new()
        } else {
            format!(" - {suffix}")
        };
        let timestamp_suffix = if entry.completed_at_ms == 0 {
            String::new()
        } else {
            format!(" @{}", entry.completed_at_ms)
        };
        let parent_suffix = entry
            .parent_event_id
            .map(|parent| format!(" parent {parent}"))
            .unwrap_or_default();
        let children_suffix = if entry.child_event_ids.is_empty() {
            String::new()
        } else {
            let children = entry
                .child_event_ids
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(",");
            format!(" children {children}")
        };
        let labels_suffix = if entry.labels.is_empty() {
            String::new()
        } else {
            format!(" labels {}", entry.labels.join(","))
        };
        lines.push(format!(
            "  {} {} {}{}{}{}{}{}",
            tree_item_label(&item.kind),
            item.id,
            item.turn_id,
            suffix,
            timestamp_suffix,
            parent_suffix,
            children_suffix,
            labels_suffix
        ));
    }
    if count == 0 {
        lines.push("  none".to_string());
    }
    lines.join("\n")
}

#[cfg(test)]
fn format_thread_item_tree_from_items(
    items: &[oppi_protocol::Item],
    filter_mode: TreeFilterMode,
) -> String {
    let entries = items
        .iter()
        .enumerate()
        .map(|(index, item)| ThreadItemEntry {
            event_id: (index as u64) + 1,
            parent_event_id: None,
            child_event_ids: Vec::new(),
            completed_at_ms: 0,
            labels: Vec::new(),
            item: item.clone(),
        })
        .collect::<Vec<_>>();
    format_thread_item_tree_from_entries(&entries, None, filter_mode, 0, entries.len() as u64, None)
}

pub(super) fn autocomplete_usage() -> &'static str {
    "usage: /autocomplete [status|items <3..20>]"
}

pub(super) fn autocomplete_command_route(args: &str) -> Result<AutocompleteCommandRoute, String> {
    let parts = args.split_whitespace().collect::<Vec<_>>();
    match parts.as_slice() {
        [] | ["status"] | ["settings"] => Ok(AutocompleteCommandRoute::Status),
        ["items", value]
        | ["item", value]
        | ["visible", value]
        | ["max-visible", value]
        | ["max", value] => parse_autocomplete_max_visible(value)
            .map(AutocompleteCommandRoute::SetMaxVisible)
            .map_err(|_| autocomplete_usage().to_string()),
        _ => Err(autocomplete_usage().to_string()),
    }
}

fn parse_autocomplete_max_visible(raw: &str) -> Result<usize, String> {
    raw.parse::<usize>()
        .ok()
        .filter(|value| {
            *value >= MIN_AUTOCOMPLETE_MAX_VISIBLE && *value <= MAX_AUTOCOMPLETE_MAX_VISIBLE
        })
        .ok_or_else(|| autocomplete_usage().to_string())
}

pub(super) fn normalize_autocomplete_max_visible(value: usize) -> Option<usize> {
    (MIN_AUTOCOMPLETE_MAX_VISIBLE..=MAX_AUTOCOMPLETE_MAX_VISIBLE)
        .contains(&value)
        .then_some(value)
}

pub(super) fn format_autocomplete_status(max_visible: usize) -> String {
    format!("autocomplete: visible items {max_visible}")
}

pub(super) fn autocomplete_max_visible_toggle_arg(current: usize) -> usize {
    AUTOCOMPLETE_VISIBLE_CYCLE_VALUES
        .iter()
        .copied()
        .find(|value| *value > current)
        .unwrap_or(AUTOCOMPLETE_VISIBLE_CYCLE_VALUES[0])
}

fn skills_usage() -> &'static str {
    "usage: /skills [list|status|commands <on|off>]"
}

pub(super) fn skills_command_route(args: &str) -> Result<SkillsCommandRoute, String> {
    let parts = args.split_whitespace().collect::<Vec<_>>();
    match parts.as_slice() {
        [] | ["list"] => Ok(SkillsCommandRoute::List),
        ["status"] | ["commands"] => Ok(SkillsCommandRoute::Status),
        ["commands", value] => parse_footer_bool(value)
            .map(SkillsCommandRoute::SetCommands)
            .map_err(|_| skills_usage().to_string()),
        _ => Err(skills_usage().to_string()),
    }
}

pub(super) fn format_skills_status(enable_skill_commands: bool) -> String {
    format!(
        "skills: /skill:name commands {}",
        enabled_label(enable_skill_commands)
    )
}

pub(super) fn skill_commands_toggle_arg(enable_skill_commands: bool) -> &'static str {
    if enable_skill_commands { "off" } else { "on" }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CompactionCommandRoute {
    Status,
    CompactNow,
    SetIdleEnabled(bool),
    SetIdleMinutes(u64),
    SetIdleThresholdPercent(u64),
    SetSmartThresholdPercent(u64),
}

fn compaction_usage() -> String {
    format!(
        "usage: /idle-compact [status|compact|on|off|idle-time <{}>|idle-threshold <{}>|smart-threshold <{}>]",
        allowed_values_label(&VALID_IDLE_COMPACT_MINUTES),
        allowed_values_label(&VALID_IDLE_COMPACT_THRESHOLDS),
        allowed_values_label(&VALID_SMART_COMPACT_THRESHOLDS)
    )
}

pub(super) fn compaction_command_route(args: &str) -> Result<CompactionCommandRoute, String> {
    let parts = args.split_whitespace().collect::<Vec<_>>();
    match parts.as_slice() {
        [] | ["status"] | ["settings"] => Ok(CompactionCommandRoute::Status),
        ["compact"] | ["compact-now"] | ["now"] => Ok(CompactionCommandRoute::CompactNow),
        ["on"] | ["enable"] | ["enabled"] => Ok(CompactionCommandRoute::SetIdleEnabled(true)),
        ["off"] | ["disable"] | ["disabled"] => Ok(CompactionCommandRoute::SetIdleEnabled(false)),
        ["idle", value] | ["idle-compaction", value] => Ok(CompactionCommandRoute::SetIdleEnabled(
            parse_compaction_bool(value)?,
        )),
        ["idle-time", value] | ["idle", "time", value] => {
            Ok(CompactionCommandRoute::SetIdleMinutes(
                parse_allowed_compaction_value(value, &VALID_IDLE_COMPACT_MINUTES)?,
            ))
        }
        ["idle-threshold", value] | ["idle", "threshold", value] | ["threshold", value] => {
            Ok(CompactionCommandRoute::SetIdleThresholdPercent(
                parse_allowed_compaction_value(value, &VALID_IDLE_COMPACT_THRESHOLDS)?,
            ))
        }
        ["smart-threshold", value] | ["smart", "threshold", value] => {
            Ok(CompactionCommandRoute::SetSmartThresholdPercent(
                parse_allowed_compaction_value(value, &VALID_SMART_COMPACT_THRESHOLDS)?,
            ))
        }
        _ => Err(compaction_usage()),
    }
}

fn parse_compaction_bool(raw: &str) -> Result<bool, String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "on" | "true" | "enabled" | "enable" | "yes" => Ok(true),
        "off" | "false" | "disabled" | "disable" | "no" => Ok(false),
        _ => Err(compaction_usage()),
    }
}

fn parse_allowed_compaction_value(raw: &str, allowed: &[u64]) -> Result<u64, String> {
    raw.parse::<u64>()
        .ok()
        .filter(|value| allowed.contains(value))
        .ok_or_else(compaction_usage)
}

fn allowed_values_label(values: &[u64]) -> String {
    values
        .iter()
        .map(u64::to_string)
        .collect::<Vec<_>>()
        .join("|")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FooterSettingBool {
    ShowPath,
    ShowHelpBar,
    ShowModel,
    ShowPermission,
    ShowMemory,
    ShowContext,
}

impl FooterSettingBool {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::ShowPath => "workspace path",
            Self::ShowHelpBar => "hotkey help",
            Self::ShowModel => "model chip",
            Self::ShowPermission => "permission chip",
            Self::ShowMemory => "memory chip",
            Self::ShowContext => "context chip",
        }
    }

    pub(super) fn apply(self, settings: &mut FooterSettings, shown: bool) {
        match self {
            Self::ShowPath => settings.show_path = shown,
            Self::ShowHelpBar => settings.show_help_bar = shown,
            Self::ShowModel => settings.show_model = shown,
            Self::ShowPermission => settings.show_permission = shown,
            Self::ShowMemory => settings.show_memory = shown,
            Self::ShowContext => settings.show_context = shown,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FooterCommandRoute {
    Status,
    SetBool(FooterSettingBool, bool),
    SetUsageDisplay(FooterUsageDisplay),
}

fn footer_usage() -> &'static str {
    "usage: /footer [status|path <on|off>|help <on|off>|usage <session-weekly|session|weekly|off>|model <on|off>|permission <on|off>|memory <on|off>|context <on|off>]"
}

pub(super) fn footer_command_route(args: &str) -> Result<FooterCommandRoute, String> {
    let parts = args.split_whitespace().collect::<Vec<_>>();
    match parts.as_slice() {
        [] | ["status"] => Ok(FooterCommandRoute::Status),
        ["path" | "workspace", value] => Ok(FooterCommandRoute::SetBool(
            FooterSettingBool::ShowPath,
            parse_footer_bool(value)?,
        )),
        ["help" | "hotkeys", value] => Ok(FooterCommandRoute::SetBool(
            FooterSettingBool::ShowHelpBar,
            parse_footer_bool(value)?,
        )),
        ["model", value] => Ok(FooterCommandRoute::SetBool(
            FooterSettingBool::ShowModel,
            parse_footer_bool(value)?,
        )),
        ["permission" | "permissions" | "perm", value] => Ok(FooterCommandRoute::SetBool(
            FooterSettingBool::ShowPermission,
            parse_footer_bool(value)?,
        )),
        ["memory", value] => Ok(FooterCommandRoute::SetBool(
            FooterSettingBool::ShowMemory,
            parse_footer_bool(value)?,
        )),
        ["context" | "ctx", value] => Ok(FooterCommandRoute::SetBool(
            FooterSettingBool::ShowContext,
            parse_footer_bool(value)?,
        )),
        ["usage", value] => Ok(FooterCommandRoute::SetUsageDisplay(
            parse_footer_usage_display(value)?,
        )),
        _ => Err(footer_usage().to_string()),
    }
}

fn parse_footer_bool(raw: &str) -> Result<bool, String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "on" | "show" | "shown" | "true" | "enabled" | "enable" | "allow" | "allowed" | "yes" => {
            Ok(true)
        }
        "off" | "hide" | "hidden" | "false" | "disabled" | "disable" | "block" | "blocked"
        | "deny" | "denied" | "no" => Ok(false),
        _ => Err(footer_usage().to_string()),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ImageSettingsCommandRoute {
    Status,
    Capabilities,
    SetShowImages(bool),
    SetImageWidthCells(u64),
    SetAutoResizeImages(bool),
    SetBlockImages(bool),
    SetPreviewProtocol(ImagePreviewProtocolOverride),
}

fn image_settings_usage() -> String {
    format!(
        "usage: /images [status|capabilities|show <on|off>|width <{}>|auto-resize <on|off>|block <on|off>|protocol <{}>]",
        allowed_values_label(&VALID_IMAGE_WIDTH_CELLS),
        IMAGE_PREVIEW_PROTOCOL_VALUES_LABEL
    )
}

pub(super) fn image_settings_command_route(
    args: &str,
) -> Result<ImageSettingsCommandRoute, String> {
    let parts = args.split_whitespace().collect::<Vec<_>>();
    match parts.as_slice() {
        [] | ["status"] | ["settings"] => Ok(ImageSettingsCommandRoute::Status),
        ["capabilities"] | ["caps"] | ["detect"] => Ok(ImageSettingsCommandRoute::Capabilities),
        ["show" | "preview" | "previews", value] => Ok(ImageSettingsCommandRoute::SetShowImages(
            parse_image_settings_bool(value)?,
        )),
        ["width", value] | ["image-width", value] => Ok(
            ImageSettingsCommandRoute::SetImageWidthCells(parse_allowed_image_width(value)?),
        ),
        ["auto-resize", value] | ["resize", value] | ["auto", "resize", value] => Ok(
            ImageSettingsCommandRoute::SetAutoResizeImages(parse_image_settings_bool(value)?),
        ),
        ["block", value] | ["block-images", value] => Ok(
            ImageSettingsCommandRoute::SetBlockImages(parse_image_settings_bool(value)?),
        ),
        ["protocol", value] | ["preview-protocol", value] | ["terminal-protocol", value] => {
            Ok(ImageSettingsCommandRoute::SetPreviewProtocol(
                parse_image_preview_protocol_override(value)?,
            ))
        }
        _ => Err(image_settings_usage()),
    }
}

fn parse_image_settings_bool(raw: &str) -> Result<bool, String> {
    parse_footer_bool(raw).map_err(|_| image_settings_usage())
}

fn parse_allowed_image_width(raw: &str) -> Result<u64, String> {
    raw.parse::<u64>()
        .ok()
        .filter(|value| VALID_IMAGE_WIDTH_CELLS.contains(value))
        .ok_or_else(image_settings_usage)
}

pub(super) fn parse_image_preview_protocol_override(
    raw: &str,
) -> Result<ImagePreviewProtocolOverride, String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "auto" | "detect" | "default" => Ok(ImagePreviewProtocolOverride::Auto),
        "kitty" | "kitty-local-file" => Ok(ImagePreviewProtocolOverride::Kitty),
        "wezterm" => Ok(ImagePreviewProtocolOverride::Wezterm),
        "iterm" | "iterm2" | "iterm2-inline-file" => Ok(ImagePreviewProtocolOverride::Iterm2),
        "sixel" => Ok(ImagePreviewProtocolOverride::Sixel),
        "off" | "none" | "disabled" | "disable" => Ok(ImagePreviewProtocolOverride::Off),
        _ => Err(image_settings_usage()),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum MemorySettingsCommandRoute {
    Status,
    SetEnabled(bool),
    SetStartupRecall(bool),
    SetTaskStartRecall(bool),
    SetTurnSummaries(bool),
    SetAgentModel(&'static str),
    SetDeepModel(&'static str),
    SetIdleConsolidation(bool),
    SetHoppiInstallOffer(&'static str),
    InstallHoppi(bool),
    SetSyncEnabled(bool),
    SetSyncRepoPath(Option<String>),
    SetSyncRepoUrl(Option<String>),
    SetSyncPullOnStartup(bool),
    SetSyncPushOnExit(bool),
    SetSyncBackground(&'static str),
    SetSyncEncryption {
        encryption: &'static str,
        passphrase_source: &'static str,
    },
    SetSyncPassphraseEnv(String),
    SetSyncPassphraseFile(Option<String>),
    SetSyncConflictMode(&'static str),
    RunSync(HoppiSyncDirection),
}

pub(super) fn memory_settings_usage() -> &'static str {
    "usage: /memory [status|jobs|jobs all|jobs cancel <id>|on|off|startup-recall <on|off>|task-start-recall <on|off>|turn-summaries <on|off>|agent-model <auto|claude|gpt>|deep-model <auto|sonnet|gpt-5.5>|idle-dream <on|off>|install-offer <ask|dismissed>|install-hoppi [--yes]|sync <on|off>|sync now|sync pull|sync push|sync repo <path|none>|sync remote <url|none>|sync pull-startup <on|off>|sync push-exit <on|off>|sync background <off|15m|30m|60m>|sync encryption <none|env|file>|sync passphrase-env <ENV>|sync passphrase-file <path|none>|sync conflict <keep-both|keep-local|overwrite-local>|dashboard|list|forget <id> --yes|edit <id> content <text> --yes|edit <id> content=<text> tags=a,b confidence=verified supersededBy=<id> --yes|supersede <old-id> <new-id> --yes|pin <id> --yes|unpin <id> --yes|settings|compaction|maintenance [dry-run|apply]|compact [summary]]"
}

pub(super) fn parse_memory_update_patch_args(args: &[&str]) -> Result<Value, String> {
    if args.is_empty() {
        return Err(memory_settings_usage().to_string());
    }
    let mut patch = serde_json::Map::new();
    if args[0].eq_ignore_ascii_case("content") {
        let content = args[1..].join(" ");
        let content = content.trim();
        if content.is_empty() {
            return Err(memory_settings_usage().to_string());
        }
        patch.insert("content".to_string(), json!(content));
        return Ok(Value::Object(patch));
    }

    for arg in args {
        let Some((key, raw_value)) = arg.split_once('=') else {
            return Err(memory_settings_usage().to_string());
        };
        let key = key.trim().to_ascii_lowercase();
        let value = raw_value.trim();
        match key.as_str() {
            "content" => {
                if value.is_empty() {
                    return Err(memory_settings_usage().to_string());
                }
                patch.insert("content".to_string(), json!(value));
            }
            "tags" => {
                patch.insert(
                    "tags".to_string(),
                    json!(parse_csv_or_whitespace_list(value)?),
                );
            }
            "confidence" => {
                if value.is_empty() {
                    return Err(memory_settings_usage().to_string());
                }
                patch.insert("confidence".to_string(), json!(value));
            }
            "source" => {
                if value.is_empty() {
                    return Err(memory_settings_usage().to_string());
                }
                patch.insert("source".to_string(), json!(value));
            }
            "pinned" | "pin" => {
                patch.insert(
                    "pinned".to_string(),
                    json!(parse_memory_update_bool(value)?),
                );
            }
            "starred" | "star" => {
                patch.insert(
                    "starred".to_string(),
                    json!(parse_memory_update_bool(value)?),
                );
            }
            "supersededby" | "superseded-by" | "supersedes" => {
                if value.is_empty() {
                    return Err(memory_settings_usage().to_string());
                }
                patch.insert("supersededBy".to_string(), json!(value));
            }
            _ => return Err(memory_settings_usage().to_string()),
        };
    }
    if patch.is_empty() {
        return Err(memory_settings_usage().to_string());
    }
    Ok(Value::Object(patch))
}

fn parse_memory_update_bool(raw: &str) -> Result<bool, String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "true" | "on" | "yes" | "1" => Ok(true),
        "false" | "off" | "no" | "0" => Ok(false),
        _ => Err(memory_settings_usage().to_string()),
    }
}

pub(super) fn memory_settings_command_route(
    args: &str,
) -> Result<MemorySettingsCommandRoute, String> {
    let parts = args.split_whitespace().collect::<Vec<_>>();
    match parts.as_slice() {
        [] | ["status"] => Ok(MemorySettingsCommandRoute::Status),
        ["on"] | ["enable"] | ["enabled"] => Ok(MemorySettingsCommandRoute::SetEnabled(true)),
        ["off"] | ["disable"] | ["disabled"] => Ok(MemorySettingsCommandRoute::SetEnabled(false)),
        ["startup-recall", value] | ["startup", "recall", value] => Ok(
            MemorySettingsCommandRoute::SetStartupRecall(parse_memory_settings_bool(value)?),
        ),
        ["task-start-recall", value] | ["task", "start", "recall", value] => Ok(
            MemorySettingsCommandRoute::SetTaskStartRecall(parse_memory_settings_bool(value)?),
        ),
        ["turn-summaries", value] | ["turn-summary", value] | ["summaries", value] => Ok(
            MemorySettingsCommandRoute::SetTurnSummaries(parse_memory_settings_bool(value)?),
        ),
        ["agent-model", value] | ["memory-agent-model", value] => Ok(
            MemorySettingsCommandRoute::SetAgentModel(parse_memory_agent_model(value)?),
        ),
        ["deep-model", value] | ["deep-dream-model", value] => Ok(
            MemorySettingsCommandRoute::SetDeepModel(parse_deep_memory_model(value)?),
        ),
        ["idle-dream", value] | ["idle-consolidation", value] => Ok(
            MemorySettingsCommandRoute::SetIdleConsolidation(parse_memory_settings_bool(value)?),
        ),
        ["install-offer", value] | ["first-start-install-offer", value] => Ok(
            MemorySettingsCommandRoute::SetHoppiInstallOffer(parse_hoppi_install_offer(value)?),
        ),
        ["install-hoppi", flags @ ..]
        | ["install", flags @ ..]
        | ["backend", "install", flags @ ..] => Ok(MemorySettingsCommandRoute::InstallHoppi(
            memory_action_approved(flags),
        )),
        ["sync", "now"] => Ok(MemorySettingsCommandRoute::RunSync(
            HoppiSyncDirection::Both,
        )),
        ["sync", "pull"] => Ok(MemorySettingsCommandRoute::RunSync(
            HoppiSyncDirection::Pull,
        )),
        ["sync", "push"] => Ok(MemorySettingsCommandRoute::RunSync(
            HoppiSyncDirection::Push,
        )),
        ["sync", value] => Ok(MemorySettingsCommandRoute::SetSyncEnabled(
            parse_memory_settings_bool(value)?,
        )),
        ["sync", "repo", rest @ ..] | ["sync-repo", rest @ ..] => Ok(
            MemorySettingsCommandRoute::SetSyncRepoPath(parse_memory_sync_optional_value(rest)?),
        ),
        ["sync", "remote", rest @ ..]
        | ["sync", "url", rest @ ..]
        | ["sync-remote", rest @ ..]
        | ["sync-url", rest @ ..] => Ok(MemorySettingsCommandRoute::SetSyncRepoUrl(
            parse_memory_sync_optional_value(rest)?,
        )),
        ["sync", "pull-startup", value] | ["sync-pull-startup", value] => Ok(
            MemorySettingsCommandRoute::SetSyncPullOnStartup(parse_memory_settings_bool(value)?),
        ),
        ["sync", "push-exit", value] | ["sync-push-exit", value] => Ok(
            MemorySettingsCommandRoute::SetSyncPushOnExit(parse_memory_settings_bool(value)?),
        ),
        ["sync", "background", value] | ["sync-background", value] => Ok(
            MemorySettingsCommandRoute::SetSyncBackground(parse_sync_background(value)?),
        ),
        ["sync", "encryption", value] | ["sync-encryption", value] => {
            let (encryption, passphrase_source) = parse_sync_encryption(value)?;
            Ok(MemorySettingsCommandRoute::SetSyncEncryption {
                encryption,
                passphrase_source,
            })
        }
        ["sync", "passphrase-env", value] | ["sync-passphrase-env", value] => {
            Ok(MemorySettingsCommandRoute::SetSyncPassphraseEnv(
                parse_sync_passphrase_env(value)?.to_string(),
            ))
        }
        ["sync", "passphrase-file", rest @ ..] | ["sync-passphrase-file", rest @ ..] => {
            Ok(MemorySettingsCommandRoute::SetSyncPassphraseFile(
                parse_memory_sync_optional_value(rest)?,
            ))
        }
        ["sync", "conflict", value] | ["sync-conflict", value] => Ok(
            MemorySettingsCommandRoute::SetSyncConflictMode(parse_sync_conflict_mode(value)?),
        ),
        _ => Err(memory_settings_usage().to_string()),
    }
}

pub(super) fn parse_memory_settings_bool(raw: &str) -> Result<bool, String> {
    parse_footer_bool(raw).map_err(|_| memory_settings_usage().to_string())
}

pub(super) fn parse_memory_agent_model(raw: &str) -> Result<&'static str, String> {
    let normalized = raw.trim().to_ascii_lowercase();
    VALID_MEMORY_AGENT_MODELS
        .iter()
        .copied()
        .find(|value| *value == normalized)
        .ok_or_else(|| memory_settings_usage().to_string())
}

fn memory_action_approved(flags: &[&str]) -> bool {
    flags.iter().any(|flag| {
        matches!(
            flag.trim().to_ascii_lowercase().as_str(),
            "--yes" | "yes" | "--approve" | "approve" | "--i-understand"
        )
    })
}

pub(super) fn parse_hoppi_install_offer(raw: &str) -> Result<&'static str, String> {
    let normalized = raw.trim().to_ascii_lowercase();
    VALID_HOPPI_INSTALL_OFFERS
        .iter()
        .copied()
        .find(|value| *value == normalized)
        .ok_or_else(|| memory_settings_usage().to_string())
}

pub(super) fn parse_memory_sync_optional_value(parts: &[&str]) -> Result<Option<String>, String> {
    let value = parts.join(" ");
    let value = value.trim();
    if value.is_empty() {
        return Err(memory_settings_usage().to_string());
    }
    if matches!(
        value.to_ascii_lowercase().as_str(),
        "none" | "unset" | "default" | "off"
    ) {
        Ok(None)
    } else {
        Ok(Some(value.to_string()))
    }
}

pub(super) fn parse_sync_background(raw: &str) -> Result<&'static str, String> {
    let normalized = raw.trim().to_ascii_lowercase();
    VALID_SYNC_BACKGROUND
        .iter()
        .copied()
        .find(|value| *value == normalized)
        .ok_or_else(|| memory_settings_usage().to_string())
}

pub(super) fn parse_sync_encryption(raw: &str) -> Result<(&'static str, &'static str), String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "none" | "off" => Ok(("none", "env")),
        "env" | "passphrase-env" => Ok(("passphrase", "env")),
        "file" | "passphrase-file" => Ok(("passphrase", "file")),
        _ => Err(memory_settings_usage().to_string()),
    }
}

pub(super) fn parse_sync_passphrase_env(raw: &str) -> Result<&str, String> {
    let value = raw.trim();
    if value.is_empty()
        || !value
            .chars()
            .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_')
        || value.chars().next().is_some_and(|ch| ch.is_ascii_digit())
    {
        Err(memory_settings_usage().to_string())
    } else {
        Ok(value)
    }
}

pub(super) fn parse_sync_conflict_mode(raw: &str) -> Result<&'static str, String> {
    let normalized = raw.trim().to_ascii_lowercase();
    VALID_SYNC_CONFLICT_MODES
        .iter()
        .copied()
        .find(|value| *value == normalized)
        .ok_or_else(|| memory_settings_usage().to_string())
}

pub(super) fn parse_deep_memory_model(raw: &str) -> Result<&'static str, String> {
    let normalized = raw.trim().to_ascii_lowercase();
    VALID_DEEP_MEMORY_MODELS
        .iter()
        .copied()
        .find(|value| *value == normalized)
        .ok_or_else(|| memory_settings_usage().to_string())
}

pub(super) fn parse_footer_usage_display(raw: &str) -> Result<FooterUsageDisplay, String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "session-weekly" | "session+weekly" | "session_weekly" | "both" | "all" => {
            Ok(FooterUsageDisplay::SessionWeekly)
        }
        "session" | "sessions" => Ok(FooterUsageDisplay::Session),
        "weekly" | "week" => Ok(FooterUsageDisplay::Weekly),
        "off" | "none" | "hide" | "hidden" => Ok(FooterUsageDisplay::Off),
        _ => Err(footer_usage().to_string()),
    }
}

pub(super) fn format_ask_user_timeout_label(timeout_ms: u64) -> String {
    match ask_user_timeout_ms_to_minutes(timeout_ms) {
        Some(0) => "off".to_string(),
        Some(minutes) => format!("{minutes}m"),
        None => format!("{timeout_ms}ms"),
    }
}

pub(super) fn parse_permission_mode(raw: &str) -> Option<PermissionMode> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "read-only" | "readonly" | "ro" => Some(PermissionMode::ReadOnly),
        "default" => Some(PermissionMode::Default),
        "auto-review" | "autoreview" | "auto" => Some(PermissionMode::AutoReview),
        "full-access" | "full" | "danger" => Some(PermissionMode::FullAccess),
        _ => None,
    }
}

pub(super) fn network_policy_label_for_settings(
    mode: PermissionMode,
    settings: &PermissionSandboxSettings,
) -> &'static str {
    if mode == PermissionMode::FullAccess || settings.allow_internet || settings.allow_ssh {
        "enabled"
    } else {
        "disabled"
    }
}

pub(super) fn filesystem_policy_for_mode(mode: PermissionMode) -> &'static str {
    match mode {
        PermissionMode::ReadOnly => "readOnly",
        PermissionMode::FullAccess => "unrestricted",
        PermissionMode::Default | PermissionMode::AutoReview => "workspaceWrite",
    }
}

#[cfg(test)]
pub(super) fn sandbox_policy_json(mode: PermissionMode, cwd: &str) -> Value {
    sandbox_policy_json_with_protected_patterns(mode, cwd, &[])
}

pub(super) fn sandbox_policy_json_with_settings(
    mode: PermissionMode,
    cwd: &str,
    settings: &PermissionSandboxSettings,
) -> Value {
    let project_roots = effective_project_roots(settings, cwd);
    let readable_roots = effective_readable_roots(settings, &project_roots);
    let writable_roots = if mode == PermissionMode::ReadOnly {
        Vec::<String>::new()
    } else {
        effective_writable_roots(settings, &project_roots)
    };
    json!({
        "permissionProfile": {
            "mode": mode,
            "readableRoots": readable_roots,
            "writableRoots": writable_roots,
            "filesystemRules": [],
            "protectedPatterns": protected_patterns_for_policy(&settings.protected_patterns),
        },
        "network": if network_policy_label_for_settings(mode, settings) == "enabled" { NetworkPolicy::Enabled } else { NetworkPolicy::Disabled },
        "filesystem": match mode {
            PermissionMode::ReadOnly => FilesystemPolicy::ReadOnly,
            PermissionMode::FullAccess => FilesystemPolicy::Unrestricted,
            PermissionMode::Default | PermissionMode::AutoReview => FilesystemPolicy::WorkspaceWrite,
        },
    })
}

fn protected_patterns_for_policy(additional: &[String]) -> Vec<String> {
    let mut patterns = DEFAULT_TURN_PROTECTED_PATTERNS
        .iter()
        .map(|pattern| (*pattern).to_string())
        .collect::<Vec<_>>();
    for pattern in normalize_string_slice(additional) {
        if !patterns.contains(&pattern) {
            patterns.push(pattern);
        }
    }
    patterns
}

pub(super) fn format_protected_patterns_label(patterns: &[String]) -> String {
    let patterns = normalize_string_slice(patterns);
    if patterns.is_empty() {
        "default".to_string()
    } else {
        patterns.join(", ")
    }
}

pub(super) fn format_sandbox_roots_label(roots: &[String], fallback: &str) -> String {
    let roots = normalize_string_slice(roots);
    if roots.is_empty() {
        fallback.to_string()
    } else {
        roots.join(", ")
    }
}

pub(super) fn format_sandbox_flag_label(allowed: bool) -> &'static str {
    if allowed { "allowed" } else { "blocked" }
}

pub(super) fn permission_sandbox_settings_json(settings: &PermissionSandboxSettings) -> Value {
    json!({
        "allowInternet": settings.allow_internet,
        "allowSsh": settings.allow_ssh,
        "projectRoots": normalize_string_slice(&settings.project_roots),
        "readableRoots": normalize_string_slice(&settings.readable_roots),
        "writableRoots": normalize_string_slice(&settings.writable_roots),
        "protectedPatterns": normalize_string_slice(&settings.protected_patterns),
    })
}

pub(super) fn permission_sandbox_validation_value(
    mode: PermissionMode,
    cwd: &str,
    settings: &PermissionSandboxSettings,
) -> Value {
    let project_roots = effective_project_roots(settings, cwd);
    let readable_roots = effective_readable_roots(settings, &project_roots);
    let writable_roots = if mode == PermissionMode::ReadOnly {
        Vec::<String>::new()
    } else {
        effective_writable_roots(settings, &project_roots)
    };
    let project_source = if normalize_string_slice(&settings.project_roots).is_empty() {
        "default"
    } else {
        "configured"
    };
    let readable_source = if normalize_string_slice(&settings.readable_roots).is_empty() {
        "default"
    } else {
        "configured"
    };
    let writable_source = if mode == PermissionMode::ReadOnly {
        "read-only"
    } else if normalize_string_slice(&settings.writable_roots).is_empty() {
        "default"
    } else {
        "configured"
    };
    let project_entries = sandbox_root_validation_entries(cwd, &project_roots, project_source);
    let readable_entries = sandbox_root_validation_entries(cwd, &readable_roots, readable_source);
    let writable_entries = sandbox_root_validation_entries(cwd, &writable_roots, writable_source);
    let issue_count = [&project_entries, &readable_entries, &writable_entries]
        .into_iter()
        .flat_map(|entries| entries.iter())
        .filter(|entry| entry.get("status").and_then(Value::as_str) != Some("ok"))
        .count();
    json!({
        "ok": issue_count == 0,
        "issueCount": issue_count,
        "mode": mode,
        "filesystem": filesystem_policy_for_mode(mode),
        "network": network_policy_label_for_settings(mode, settings),
        "roots": {
            "project": project_entries,
            "readable": readable_entries,
            "writable": writable_entries,
        },
        "protectedPatterns": protected_patterns_for_policy(&settings.protected_patterns),
    })
}

fn sandbox_root_validation_entries(cwd: &str, roots: &[String], source: &str) -> Vec<Value> {
    roots
        .iter()
        .map(|root| {
            json!({
                "path": root,
                "status": sandbox_root_status(cwd, root),
                "source": source,
            })
        })
        .collect()
}

fn sandbox_root_status(cwd: &str, root: &str) -> &'static str {
    let path = Path::new(root);
    let check_path = if path.is_absolute() {
        PathBuf::from(path)
    } else {
        Path::new(cwd).join(path)
    };
    if !check_path.exists() {
        "missing"
    } else if !check_path.is_dir() {
        "not-directory"
    } else {
        "ok"
    }
}

pub(super) fn format_permission_sandbox_validation(value: &Value) -> String {
    let issue_count = value.get("issueCount").and_then(Value::as_u64).unwrap_or(0);
    let issue_label = match issue_count {
        0 => "ok".to_string(),
        1 => "1 issue".to_string(),
        count => format!("{count} issues"),
    };
    let filesystem = value
        .get("filesystem")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let network = value
        .get("network")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let protected_count = value
        .get("protectedPatterns")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    let mut lines = vec![
        format!("sandbox validation: {issue_label}"),
        format!("effective policy: filesystem={filesystem}, network={network}"),
        format!("protected patterns: {protected_count} active"),
    ];
    lines.extend(format_sandbox_validation_roots(
        value,
        "project",
        "project roots",
    ));
    lines.extend(format_sandbox_validation_roots(
        value,
        "readable",
        "readable roots",
    ));
    lines.extend(format_sandbox_validation_roots(
        value,
        "writable",
        "writable roots",
    ));
    lines.join("\n")
}

fn format_sandbox_validation_roots(value: &Value, key: &str, label: &str) -> Vec<String> {
    let entries = value
        .get("roots")
        .and_then(|roots| roots.get(key))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut lines = vec![format!("{label}:")];
    if entries.is_empty() {
        lines.push("- none".to_string());
        return lines;
    }
    for entry in entries {
        let status = entry
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let path = entry.get("path").and_then(Value::as_str).unwrap_or("");
        let source = entry
            .get("source")
            .and_then(Value::as_str)
            .unwrap_or("configured");
        lines.push(format!("- {status} {path} ({source})"));
    }
    lines
}

pub(super) fn side_question_request_params(
    thread_id: &str,
    question: &str,
    provider: &ProviderConfig,
    cwd: &str,
    permission_mode: PermissionMode,
    sandbox_settings: &PermissionSandboxSettings,
    prompt_variant: &str,
) -> Value {
    let mut params = json!({
        "threadId": thread_id,
        "question": question,
        "executionMode": "blocking",
        "sandboxPolicy": sandbox_policy_json_with_settings(
            permission_mode,
            cwd,
            sandbox_settings,
        ),
    });
    match provider {
        ProviderConfig::Mock => {
            params["modelSteps"] = mock_model_steps_for_prompt(question, cwd);
            if let Some(tool_definitions) = mock_tool_definitions_for_prompt(question) {
                params["toolDefinitions"] = tool_definitions;
            }
        }
        ProviderConfig::OpenAiCompatible(config) => {
            let mut provider_json = openai_provider_json(config);
            apply_feature_routing_to_provider(&mut provider_json, prompt_variant);
            apply_prompt_variant_to_provider(&mut provider_json, prompt_variant);
            params["modelProvider"] = provider_json;
            params["maxContinuations"] = json!(8);
        }
    }
    params
}

pub(super) fn memory_compact_summary_arg(rest: &[&str]) -> String {
    rest.join(" ")
}

pub(super) fn parse_mcp_prompt_arguments(
    rest: &[&str],
) -> Result<BTreeMap<String, String>, String> {
    let mut arguments = BTreeMap::new();
    for raw in rest {
        let Some((key, value)) = raw.split_once('=') else {
            return Err(format!(
                "mcp prompt arguments must use key=value: {}",
                compact_mcp_text(raw, 80)
            ));
        };
        let key = key.trim();
        if key.is_empty() {
            return Err("mcp prompt argument key cannot be empty".to_string());
        }
        arguments.insert(key.to_string(), value.to_string());
    }
    Ok(arguments)
}

pub(super) fn parse_mcp_list_cursor_arg(
    command: &str,
    rest: &[&str],
) -> Result<Option<String>, String> {
    let usage = || format!("usage: /mcp {command} <id> [cursor]");
    let cursor = match rest {
        [] => return Ok(None),
        ["--cursor"] | ["cursor"] => return Err(usage()),
        ["--cursor", cursor] | ["cursor", cursor] | [cursor] => *cursor,
        _ => return Err(usage()),
    };
    let cursor = cursor.trim();
    if cursor.is_empty() {
        return Err(format!("{}; cursor cannot be empty", usage()));
    }
    Ok(Some(cursor.to_string()))
}

pub(super) fn mcp_list_request_params_json(
    thread_id: &str,
    server_id: &str,
    cursor: Option<&str>,
) -> Value {
    let mut params = json!({
        "threadId": thread_id,
        "serverId": server_id,
    });
    if let Some(cursor) = cursor.map(str::trim).filter(|cursor| !cursor.is_empty()) {
        params["cursor"] = json!(cursor);
    }
    params
}

pub(super) fn mcp_command_usage() -> &'static str {
    "usage: /mcp [list|connect <id>|test <id>|reload <id>|auth <id>|resources <id> [cursor]|templates <id> [cursor]|read <id> <uri>|subscribe <id> <uri>|unsubscribe <id> <uri>|prompts <id> [cursor]|prompt <id> <name> [key=value...]|trust-plugin <plugin-id> <server-id>|disable-plugin <plugin-id> <server-id>]"
}

fn mcp_transport_kind(item: &Value) -> &str {
    item.get("transport")
        .and_then(|transport| transport.get("kind"))
        .and_then(Value::as_str)
        .unwrap_or("unknown")
}

fn mcp_tool_names(item: &Value) -> Vec<String> {
    let mut names = item
        .get("tools")
        .and_then(Value::as_array)
        .map(|tools| {
            tools
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if names.is_empty() {
        names = item
            .get("toolRefs")
            .and_then(Value::as_array)
            .map(|tools| {
                tools
                    .iter()
                    .filter_map(|tool| tool.get("name").and_then(Value::as_str))
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
    }
    names
}

fn mcp_capability_flag_label(
    capabilities: &Value,
    section: &str,
    flag: &str,
    label: &str,
) -> Option<String> {
    let value = capabilities
        .get(section)
        .and_then(|section| section.get(flag))
        .and_then(Value::as_bool)?;
    if value {
        Some(label.to_string())
    } else {
        Some(format!("{label}=false"))
    }
}

fn mcp_server_capability_labels(item: &Value) -> Vec<String> {
    let Some(capabilities) = item.get("capabilities").and_then(Value::as_object) else {
        return Vec::new();
    };
    let capabilities = Value::Object(capabilities.clone());
    let mut labels = Vec::new();
    for (section, flag, label) in [
        ("tools", "listChanged", "tools.listChanged"),
        ("resources", "subscribe", "resources.subscribe"),
        ("resources", "listChanged", "resources.listChanged"),
        ("prompts", "listChanged", "prompts.listChanged"),
    ] {
        if let Some(label) = mcp_capability_flag_label(&capabilities, section, flag, label) {
            labels.push(label);
        }
    }
    for key in ["sampling", "elicitation", "tasks"] {
        if capabilities.get(key).is_some_and(|value| !value.is_null()) {
            labels.push(format!("{key}(deferred)"));
        }
    }
    labels
}

fn compact_mcp_text(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let keep = max_chars.saturating_sub(3);
    let mut rendered = text.chars().take(keep).collect::<String>();
    rendered.push_str("...");
    rendered
}

fn mcp_status_summary_counts(value: &Value) -> (usize, usize, usize, usize) {
    let Some(items) = value.get("items").and_then(Value::as_array) else {
        return (0, 0, 0, 0);
    };
    let mut enabled = 0usize;
    let mut tools = 0usize;
    let mut diagnostics = 0usize;
    for item in items {
        if item
            .get("status")
            .and_then(Value::as_str)
            .is_some_and(|status| status == "enabled")
        {
            enabled += 1;
        }
        tools += mcp_tool_names(item).len();
        diagnostics += item
            .get("diagnostics")
            .and_then(Value::as_array)
            .map(Vec::len)
            .unwrap_or(0);
    }
    (items.len(), enabled, tools, diagnostics)
}

pub(super) fn format_mcp_status_summary(value: &Value) -> String {
    let (servers, enabled, tools, diagnostics) = mcp_status_summary_counts(value);
    if servers == 0 {
        return "none".to_string();
    }
    let mut parts = vec![format!(
        "{servers} server{}",
        if servers == 1 { "" } else { "s" }
    )];
    if enabled > 0 {
        parts.push(format!("{enabled} enabled"));
    }
    parts.push(format!("{tools} tools"));
    if diagnostics > 0 {
        parts.push(format!("{diagnostics} diagnostics"));
    }
    parts.join(", ")
}

pub(super) fn format_mcp_status_list(value: &Value) -> String {
    let Some(items) = value.get("items").and_then(Value::as_array) else {
        return "mcp: status unavailable".to_string();
    };
    if items.is_empty() {
        return "mcp: no servers configured".to_string();
    }
    let mut lines = Vec::new();
    for item in items {
        let id = item.get("id").and_then(Value::as_str).unwrap_or("mcp");
        let source = item
            .get("source")
            .and_then(Value::as_str)
            .unwrap_or("runtime");
        let status = item
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let transport = mcp_transport_kind(item);
        let tools = mcp_tool_names(item);
        lines.push(format!(
            "{id} [{source} {transport} {status}] tools={}",
            tools.len()
        ));
        if !tools.is_empty() {
            let shown = tools.iter().take(5).cloned().collect::<Vec<_>>();
            let more = tools.len().saturating_sub(shown.len());
            let suffix = if more > 0 {
                format!(" (+{more} more)")
            } else {
                String::new()
            };
            lines.push(format!("  {}", shown.join(", ") + &suffix));
        }
        let capability_labels = mcp_server_capability_labels(item);
        if !capability_labels.is_empty() {
            lines.push(format!("  capabilities={}", capability_labels.join(", ")));
        }
        let refresh_labels = mcp_list_refresh_hint_labels(item);
        if !refresh_labels.is_empty() {
            lines.push(format!("  refresh={}", refresh_labels.join(", ")));
        }
        if let Some(subscriptions) = item
            .get("resourceSubscriptions")
            .and_then(Value::as_array)
            .filter(|subscriptions| !subscriptions.is_empty())
        {
            lines.push(format!("  subscriptions={}", subscriptions.len()));
            for subscription in subscriptions.iter().take(5) {
                if let Some(uri) = subscription.get("uri").and_then(Value::as_str) {
                    let mut suffix = String::new();
                    let update_count = subscription
                        .get("updateCount")
                        .and_then(Value::as_u64)
                        .unwrap_or(0);
                    let last_updated_at = subscription
                        .get("lastUpdatedAt")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .trim();
                    if update_count > 0 {
                        suffix = format!(" (updates={update_count}");
                        if !last_updated_at.is_empty() {
                            suffix.push_str(", last=");
                            suffix.push_str(&compact_mcp_text(last_updated_at, 80));
                        }
                        suffix.push(')');
                    }
                    lines.push(format!(
                        "    {}{}",
                        compact_mcp_text(uri.trim(), 160),
                        suffix
                    ));
                }
            }
            let more = subscriptions.len().saturating_sub(5);
            if more > 0 {
                lines.push(format!("    +{more} more"));
            }
        }
        if let Some(diagnostics) = item.get("diagnostics").and_then(Value::as_array) {
            for diagnostic in diagnostics {
                let code = diagnostic
                    .get("code")
                    .and_then(Value::as_str)
                    .unwrap_or("mcp_diagnostic");
                let message = diagnostic
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("diagnostic");
                lines.push(format!(
                    "  {code}: {}",
                    compact_mcp_text(message.trim(), 180)
                ));
            }
        }
    }
    lines.join("\n")
}

fn mcp_list_refresh_hint_labels(item: &Value) -> Vec<String> {
    let Some(hints) = item.get("listRefreshHints").and_then(Value::as_object) else {
        return Vec::new();
    };
    ["tools", "resources", "prompts"]
        .into_iter()
        .filter_map(|kind| {
            let refresh_count = hints
                .get(kind)
                .and_then(|hint| hint.get("refreshCount"))
                .and_then(Value::as_u64)
                .unwrap_or(0);
            (refresh_count > 0).then(|| format!("{kind}:{refresh_count}"))
        })
        .collect()
}

pub(super) fn format_mcp_resources_list(value: &Value) -> String {
    let server_id = value
        .get("serverId")
        .and_then(Value::as_str)
        .unwrap_or("mcp");
    let status = value
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    if status != "ok" {
        let error = value
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("resource listing failed");
        return format!(
            "mcp resources {server_id}: {status}\n  {}",
            compact_mcp_text(error.trim(), 180)
        );
    }
    let Some(resources) = value.get("resources").and_then(Value::as_array) else {
        return format!("mcp resources {server_id}: unavailable");
    };
    let mut lines = vec![format!(
        "mcp resources {server_id}: {} resource{}",
        resources.len(),
        if resources.len() == 1 { "" } else { "s" }
    )];
    for resource in resources.iter().take(10) {
        let uri = resource.get("uri").and_then(Value::as_str).unwrap_or("-");
        let name = resource
            .get("title")
            .or_else(|| resource.get("name"))
            .and_then(Value::as_str)
            .unwrap_or("resource");
        lines.push(format!(
            "  {} <{}>",
            compact_mcp_text(name.trim(), 80),
            compact_mcp_text(uri.trim(), 140)
        ));
        let mut details = Vec::new();
        if let Some(mime) = resource.get("mimeType").and_then(Value::as_str) {
            details.push(mime.to_string());
        }
        if let Some(size) = resource.get("size").and_then(Value::as_u64) {
            details.push(format!("{size} bytes"));
        }
        if !details.is_empty() {
            lines.push(format!("    {}", details.join(", ")));
        }
    }
    let hidden = resources.len().saturating_sub(10);
    if hidden > 0 {
        lines.push(format!("  (+{hidden} more)"));
    }
    if let Some(next_cursor) = value
        .get("nextCursor")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        lines.push(format!(
            "  next cursor: {}",
            compact_mcp_text(next_cursor, 80)
        ));
    }
    lines.join("\n")
}

pub(super) fn format_mcp_resource_templates_list(value: &Value) -> String {
    let server_id = value
        .get("serverId")
        .and_then(Value::as_str)
        .unwrap_or("mcp");
    let status = value
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    if status != "ok" {
        let error = value
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("resource template listing failed");
        return format!(
            "mcp templates {server_id}: {status}\n  {}",
            compact_mcp_text(error.trim(), 180)
        );
    }
    let Some(templates) = value.get("resourceTemplates").and_then(Value::as_array) else {
        return format!("mcp templates {server_id}: unavailable");
    };
    let mut lines = vec![format!(
        "mcp templates {server_id}: {} template{}",
        templates.len(),
        if templates.len() == 1 { "" } else { "s" }
    )];
    for template in templates.iter().take(10) {
        let uri_template = template
            .get("uriTemplate")
            .and_then(Value::as_str)
            .unwrap_or("-");
        let name = template
            .get("title")
            .or_else(|| template.get("name"))
            .and_then(Value::as_str)
            .unwrap_or("template");
        lines.push(format!(
            "  {} <{}>",
            compact_mcp_text(name.trim(), 80),
            compact_mcp_text(uri_template.trim(), 140)
        ));
        let mut details = Vec::new();
        if let Some(mime) = template.get("mimeType").and_then(Value::as_str) {
            details.push(mime.to_string());
        }
        if !details.is_empty() {
            lines.push(format!("    {}", details.join(", ")));
        }
    }
    let hidden = templates.len().saturating_sub(10);
    if hidden > 0 {
        lines.push(format!("  (+{hidden} more)"));
    }
    if let Some(next_cursor) = value
        .get("nextCursor")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        lines.push(format!(
            "  next cursor: {}",
            compact_mcp_text(next_cursor, 80)
        ));
    }
    lines.join("\n")
}

pub(super) fn format_mcp_prompts_list(value: &Value) -> String {
    let server_id = value
        .get("serverId")
        .and_then(Value::as_str)
        .unwrap_or("mcp");
    let status = value
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    if status != "ok" {
        let error = value
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("prompt listing failed");
        return format!(
            "mcp prompts {server_id}: {status}\n  {}",
            compact_mcp_text(error.trim(), 180)
        );
    }
    let Some(prompts) = value.get("prompts").and_then(Value::as_array) else {
        return format!("mcp prompts {server_id}: unavailable");
    };
    let mut lines = vec![format!(
        "mcp prompts {server_id}: {} prompt{}",
        prompts.len(),
        if prompts.len() == 1 { "" } else { "s" }
    )];
    for prompt in prompts.iter().take(10) {
        let name = prompt.get("name").and_then(Value::as_str).unwrap_or("-");
        let title = prompt.get("title").and_then(Value::as_str).unwrap_or(name);
        lines.push(format!(
            "  {} <{}>",
            compact_mcp_text(title.trim(), 80),
            compact_mcp_text(name.trim(), 80)
        ));
        let args = prompt
            .get("arguments")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .take(6)
                    .filter_map(|item| {
                        let name = item.get("name").and_then(Value::as_str)?;
                        let suffix = if item
                            .get("required")
                            .and_then(Value::as_bool)
                            .unwrap_or(false)
                        {
                            " required"
                        } else {
                            ""
                        };
                        Some(format!("{}{}", compact_mcp_text(name.trim(), 40), suffix))
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if !args.is_empty() {
            lines.push(format!("    args: {}", args.join(", ")));
        }
    }
    let hidden = prompts.len().saturating_sub(10);
    if hidden > 0 {
        lines.push(format!("  (+{hidden} more)"));
    }
    if let Some(next_cursor) = value
        .get("nextCursor")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        lines.push(format!(
            "  next cursor: {}",
            compact_mcp_text(next_cursor, 80)
        ));
    }
    lines.join("\n")
}

fn mcp_prompt_content_summary(content: &Value) -> Vec<String> {
    let Some(content_type) = content.get("type").and_then(Value::as_str) else {
        return vec!["unknown content".to_string()];
    };
    match content_type {
        "text" => content
            .get("text")
            .and_then(Value::as_str)
            .map(|text| {
                let mut lines = vec!["text".to_string()];
                for line in text.lines().take(6) {
                    lines.push(format!("  {}", compact_mcp_text(line.trim_end(), 180)));
                }
                if text.lines().count() > 6 {
                    lines.push("  ...".to_string());
                }
                lines
            })
            .unwrap_or_else(|| vec!["text".to_string()]),
        "resource" => {
            let resource = content.get("resource").unwrap_or(&Value::Null);
            let uri = resource.get("uri").and_then(Value::as_str).unwrap_or("-");
            let mime = resource
                .get("mimeType")
                .and_then(Value::as_str)
                .unwrap_or("application/octet-stream");
            vec![format!(
                "resource {} ({})",
                compact_mcp_text(uri, 120),
                compact_mcp_text(mime, 60)
            )]
        }
        "image" | "audio" => {
            let mime = content
                .get("mimeType")
                .and_then(Value::as_str)
                .unwrap_or("application/octet-stream");
            let data_len = content
                .get("data")
                .and_then(Value::as_str)
                .map(str::len)
                .unwrap_or(0);
            vec![format!(
                "{} {} base64 chars ({})",
                content_type,
                data_len,
                compact_mcp_text(mime, 60)
            )]
        }
        other => vec![format!("{} content", compact_mcp_text(other, 60))],
    }
}

pub(super) fn format_mcp_prompt_get_result(value: &Value) -> String {
    let server_id = value
        .get("serverId")
        .and_then(Value::as_str)
        .unwrap_or("mcp");
    let name = value.get("name").and_then(Value::as_str).unwrap_or("-");
    let status = value
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    if status != "ok" {
        let error = value
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("prompt get failed");
        return format!(
            "mcp prompt {server_id}/{name}: {status}\n  {}",
            compact_mcp_text(error.trim(), 180)
        );
    }
    let Some(messages) = value.get("messages").and_then(Value::as_array) else {
        return format!("mcp prompt {server_id}/{name}: unavailable");
    };
    let mut lines = vec![format!("mcp prompt {server_id}/{name}: ok")];
    if let Some(description) = value
        .get("description")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        lines.push(format!("  {}", compact_mcp_text(description, 160)));
    }
    for message in messages.iter().take(8) {
        let role = message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("message");
        let content = message.get("content").unwrap_or(&Value::Null);
        let summary = mcp_prompt_content_summary(content);
        if let Some(first) = summary.first() {
            lines.push(format!("  {} {}", compact_mcp_text(role, 40), first));
        }
        for detail in summary.iter().skip(1) {
            lines.push(format!("    {}", detail));
        }
    }
    let hidden = messages.len().saturating_sub(8);
    if hidden > 0 {
        lines.push(format!("  (+{hidden} more messages)"));
    }
    lines.join("\n")
}

pub(super) fn format_mcp_resource_read_result(value: &Value) -> String {
    let server_id = value
        .get("serverId")
        .and_then(Value::as_str)
        .unwrap_or("mcp");
    let uri = value.get("uri").and_then(Value::as_str).unwrap_or("-");
    let status = value
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    if status != "ok" {
        let error = value
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("resource read failed");
        return format!(
            "mcp read {server_id}: {status}\n  {}",
            compact_mcp_text(error.trim(), 180)
        );
    }
    let Some(contents) = value.get("contents").and_then(Value::as_array) else {
        return format!("mcp read {server_id}: unavailable");
    };
    let mut lines = vec![format!(
        "mcp read {server_id}: {}",
        compact_mcp_text(uri, 160)
    )];
    for content in contents.iter().take(5) {
        let content_uri = content.get("uri").and_then(Value::as_str).unwrap_or(uri);
        let mime = content
            .get("mimeType")
            .and_then(Value::as_str)
            .unwrap_or("application/octet-stream");
        if let Some(text) = content.get("text").and_then(Value::as_str) {
            lines.push(format!(
                "  {} text <{}>",
                mime,
                compact_mcp_text(content_uri, 120)
            ));
            for line in text.lines().take(8) {
                lines.push(format!("    {}", compact_mcp_text(line.trim_end(), 180)));
            }
            if text.lines().count() > 8 {
                lines.push("    ...".to_string());
            }
        } else if let Some(blob) = content.get("blob").and_then(Value::as_str) {
            lines.push(format!(
                "  {} blob, {} base64 chars <{}>",
                mime,
                blob.len(),
                compact_mcp_text(content_uri, 120)
            ));
        } else {
            lines.push(format!(
                "  {} empty <{}>",
                mime,
                compact_mcp_text(content_uri, 120)
            ));
        }
    }
    let hidden = contents.len().saturating_sub(5);
    if hidden > 0 {
        lines.push(format!("  (+{hidden} more content entries)"));
    }
    lines.join("\n")
}

pub(super) fn format_mcp_resource_subscription_result(action: &str, value: &Value) -> String {
    let server_id = value
        .get("serverId")
        .and_then(Value::as_str)
        .unwrap_or("mcp");
    let uri = value.get("uri").and_then(Value::as_str).unwrap_or("-");
    let status = value
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    if status != "ok" {
        let error = value
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("resource subscription failed");
        return format!(
            "mcp {action} {server_id}: {status}\n  {}",
            compact_mcp_text(error.trim(), 180)
        );
    }
    let state = if value
        .get("subscribed")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        "subscribed"
    } else {
        "unsubscribed"
    };
    format!(
        "mcp {action} {server_id}: {}\n  {state}",
        compact_mcp_text(uri, 160)
    )
}

pub(super) fn format_mcp_action_result(action: &str, server_id: &str, value: &Value) -> String {
    let mut headline_status = "ok".to_string();
    let mut detail_lines = Vec::new();
    if let Some(events) = value.get("events").and_then(Value::as_array) {
        for event in events {
            let Some(kind) = event.get("kind") else {
                continue;
            };
            match kind.get("type").and_then(Value::as_str) {
                Some("mcpServerRegistered") => {
                    if let Some(server) = kind.get("server") {
                        let status = server
                            .get("status")
                            .and_then(Value::as_str)
                            .unwrap_or("unknown");
                        if action == "auth" && status == "authRequired" {
                            headline_status = status.to_string();
                        }
                        let tool_count = mcp_tool_names(server).len();
                        detail_lines.push(format!("  {server_id}: {status}, tools={tool_count}"));
                    }
                }
                Some("diagnostic") => {
                    if let Some(message) = kind
                        .get("diagnostic")
                        .and_then(|diagnostic| diagnostic.get("message"))
                        .and_then(Value::as_str)
                    {
                        detail_lines.push(format!("  {}", compact_mcp_text(message.trim(), 180)));
                    }
                }
                _ => {}
            }
        }
    }
    let mut lines = vec![format!("mcp {action} {server_id}: {headline_status}")];
    lines.extend(detail_lines);
    lines.join("\n")
}

pub(super) fn format_permission_review_history(value: &Value) -> String {
    let Some(records) = value.get("records").and_then(Value::as_array) else {
        return "permissions history: unavailable".to_string();
    };
    if records.is_empty() {
        return "permissions history: no Guardian review records".to_string();
    }
    let mut lines = vec!["permissions history:".to_string()];
    for record in records {
        let status = record
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let tool = record
            .get("toolName")
            .and_then(Value::as_str)
            .unwrap_or("tool");
        let timestamp = record
            .get("timestamp")
            .and_then(Value::as_str)
            .unwrap_or("-");
        let risk = record
            .get("decision")
            .and_then(|decision| decision.get("riskLevel"))
            .and_then(Value::as_str)
            .or_else(|| {
                record
                    .get("risk")
                    .and_then(|risk| risk.get("category"))
                    .and_then(Value::as_str)
            })
            .unwrap_or("unknown");
        let authorization = record
            .get("decision")
            .and_then(|decision| decision.get("userAuthorization"))
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let rationale = record
            .get("decision")
            .and_then(|decision| decision.get("rationale"))
            .and_then(Value::as_str)
            .or_else(|| record.get("summary").and_then(Value::as_str))
            .unwrap_or("");
        lines.push(format!(
            "- {timestamp} {status} {tool} risk={risk} auth={authorization}: {rationale}"
        ));
    }
    lines.join("\n")
}

fn value_string_array(value: &Value, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect()
}

pub(super) fn format_permission_review_context(value: &Value) -> String {
    let settings = value.get("settings").cloned().unwrap_or_else(|| json!({}));
    let timeout_seconds = settings
        .get("timeoutMs")
        .and_then(Value::as_u64)
        .map(|timeout_ms| timeout_ms / 1_000)
        .unwrap_or(0);
    let reviewer_model = settings
        .get("reviewerModel")
        .and_then(Value::as_str)
        .unwrap_or("auto");
    let provider_policy = value
        .get("reviewerProviderPolicy")
        .and_then(Value::as_str)
        .unwrap_or("isolated reviewer");
    let mut lines = vec![
        "permissions Guardian context:".to_string(),
        format!("reviewer model={reviewer_model} timeout={timeout_seconds}s"),
        format!("provider: {}", compact_mcp_text(provider_policy, 180)),
    ];
    let prompt_context = value_string_array(value, "promptContext");
    if !prompt_context.is_empty() {
        lines.push("prompt receives:".to_string());
        lines.extend(prompt_context.iter().map(|item| format!("- {item}")));
    }
    let reviewer_tools = value_string_array(value, "reviewerTools");
    if !reviewer_tools.is_empty() {
        lines.push(format!("reviewer tools: {}", reviewer_tools.join(", ")));
    }
    let hard_denies = value_string_array(value, "hardDenies");
    if !hard_denies.is_empty() {
        lines.push("hard denies before reviewer:".to_string());
        lines.extend(hard_denies.iter().map(|item| format!("- {item}")));
    }
    if let Some(prompt) = value
        .get("promptSkeleton")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|prompt| !prompt.is_empty())
    {
        lines.push("prompt skeleton:".to_string());
        lines.push(compact_mcp_text(prompt, 1_200));
    }
    lines.join("\n")
}

pub(super) fn mock_model_steps_for_prompt(prompt: &str, cwd: &str) -> Value {
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

pub(super) fn mock_tool_definitions_for_prompt(prompt: &str) -> Option<Value> {
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

pub(super) fn mock_model_steps_after_approval(call: Option<&ToolCall>) -> Value {
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

pub(super) fn mock_model_steps_after_answer() -> Value {
    json!([{
        "assistantDeltas": ["Answered dogfood question and continuing."],
        "finalResponse": true
    }])
}

pub(super) fn dogfood_background_command() -> String {
    if cfg!(windows) {
        "echo oppi-background-dogfood & ping -n 20 127.0.0.1 > nul".to_string()
    } else {
        "/bin/echo oppi-background-dogfood; sleep 20".to_string()
    }
}

pub(super) fn redact_debug_text(value: &str) -> String {
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

pub(super) fn active_todos(state: &TodoState) -> Vec<String> {
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

pub(super) fn format_todos(state: &TodoState) -> String {
    let active = active_todos(state);
    if active.is_empty() {
        "todos: none".to_string()
    } else if state.summary.trim().is_empty() {
        format!("todos: {}", active.join(" | "))
    } else {
        format!("todos: {}\nsummary: {}", active.join(" | "), state.summary)
    }
}

pub(super) fn format_todos_with_summary(state: &TodoState) -> String {
    let mut rendered = format_todos(state);
    if state.todos.is_empty() && !state.summary.trim().is_empty() {
        rendered.push_str(&format!("\nsummary: {}", state.summary));
    }
    rendered
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum TodoCommand {
    List,
    ClientAction {
        action: TodoClientAction,
        id: Option<String>,
    },
}

pub(super) fn parse_todos_command_args(args: &[&str]) -> Result<TodoCommand, String> {
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

pub(super) fn todo_client_action_name(action: TodoClientAction) -> &'static str {
    match action {
        TodoClientAction::Clear => "clear",
        TodoClientAction::Done => "done",
    }
}

pub(super) fn parse_agent_markdown(
    text: &str,
    source: AgentSource,
) -> Result<AgentDefinition, String> {
    oppi_agents::parse_agent_markdown(text, source, None, None, false)
}

pub(super) fn format_agent_markdown(agent: &AgentDefinition) -> String {
    oppi_agents::format_agent_markdown(agent)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AgentMarkdownTarget {
    Project,
    User,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct AgentImportRoute {
    pub(super) source: AgentSource,
    pub(super) path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct AgentExportRoute {
    pub(super) agent_name: String,
    pub(super) path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum AgentsRoute {
    List,
    Show {
        name: String,
    },
    Create {
        source: AgentSource,
        name: String,
        description: String,
    },
    Edit {
        name: String,
    },
    Delete {
        name: String,
        confirmed: bool,
    },
    Dispatch {
        name: String,
        task: String,
    },
    Poll {
        run_id: String,
    },
    Block {
        run_id: String,
        reason: String,
    },
    Complete {
        run_id: String,
        output: String,
    },
    Import(AgentImportRoute),
    Export(AgentExportRoute),
}

#[cfg(test)]
pub(super) fn parse_agents_route(input: &str) -> Result<AgentsRoute, String> {
    let parts = input.split_whitespace().collect::<Vec<_>>();
    parse_agents_route_parts(&parts, Path::new("."), Path::new("."))
}

pub(super) fn parse_agents_route_parts(
    args: &[&str],
    cwd: &Path,
    agent_dir: &Path,
) -> Result<AgentsRoute, String> {
    match args {
        [] | ["list"] => Ok(AgentsRoute::List),
        ["show", name] if !name.trim().is_empty() => Ok(AgentsRoute::Show {
            name: (*name).to_string(),
        }),
        ["create", "user", name, description @ ..] if !description.is_empty() => {
            Ok(AgentsRoute::Create {
                source: AgentSource::User,
                name: (*name).to_string(),
                description: description.join(" "),
            })
        }
        ["create", name, description @ ..] if !description.is_empty() => Ok(AgentsRoute::Create {
            source: AgentSource::Project,
            name: (*name).to_string(),
            description: description.join(" "),
        }),
        ["edit", name] if !name.trim().is_empty() => Ok(AgentsRoute::Edit {
            name: (*name).to_string(),
        }),
        ["delete", name] if !name.trim().is_empty() => Ok(AgentsRoute::Delete {
            name: (*name).to_string(),
            confirmed: false,
        }),
        ["delete", name, "--yes"] if !name.trim().is_empty() => Ok(AgentsRoute::Delete {
            name: (*name).to_string(),
            confirmed: true,
        }),
        ["dispatch", name, task @ ..] if !task.is_empty() => Ok(AgentsRoute::Dispatch {
            name: (*name).to_string(),
            task: task.join(" "),
        }),
        ["poll", run_id] | ["status", run_id] if !run_id.trim().is_empty() => {
            Ok(AgentsRoute::Poll {
                run_id: (*run_id).to_string(),
            })
        }
        ["block", run_id, reason @ ..] if !reason.is_empty() => Ok(AgentsRoute::Block {
            run_id: (*run_id).to_string(),
            reason: reason.join(" "),
        }),
        ["complete", run_id, output @ ..] if !output.is_empty() => Ok(AgentsRoute::Complete {
            run_id: (*run_id).to_string(),
            output: output.join(" "),
        }),
        ["import", rest @ ..] => Ok(AgentsRoute::Import(parse_agent_import_route(rest, cwd)?)),
        ["export", rest @ ..] => Ok(AgentsRoute::Export(parse_agent_export_route(
            rest, cwd, agent_dir,
        )?)),
        _ => Err("usage: /agents [list|show <name>|create [user] <name> <description>|edit <name>|delete <name> --yes|dispatch <name> <task>|poll <run-id>|status <run-id>|block <run-id> <reason>|complete <run-id> <output>|import [project|user] <path>|export <name> [project|user|path]]".to_string()),
    }
}

pub(super) fn parse_agent_import_route(
    args: &[&str],
    cwd: &Path,
) -> Result<AgentImportRoute, String> {
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

pub(super) fn parse_agent_export_route(
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

pub(super) fn active_agent_from_list(
    value: Value,
    agent_name: &str,
) -> Result<AgentDefinition, String> {
    let result: AgentListResult =
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

fn hoppi_module_path_for_pin() -> Result<PathBuf, String> {
    hoppi_candidate_module_paths_for_startup_offer()
        .into_iter()
        .find(|path| path.exists())
        .ok_or_else(|| {
            format!(
                "Hoppi pin skipped: Hoppi backend is not installed. Run `/memory install-hoppi --yes` or set OPPI_HOPPI_MODULE."
            )
        })
}

pub(super) fn format_agents_list(value: &Value) -> Result<String, String> {
    let result: AgentListResult = serde_json::from_value(value.clone())
        .map_err(|error| format!("decode agents/list: {error}"))?;
    if result.items.is_empty() && result.failed.is_empty() {
        return Ok("no agents discovered".to_string());
    }

    let mut rendered = "agents (active definitions; project > user > built-in):".to_string();
    for item in result.items {
        let active = item.active;
        rendered.push_str(&format!(
            "\n- {} [{}]{}: {}",
            active.name,
            agent_source_label(active.source),
            if active.read_only || !agent_source_is_writable(active.source) {
                " read-only"
            } else {
                ""
            },
            active.description
        ));
        if !active.tools.is_empty() {
            rendered.push_str(&format!("\n  tools: {}", active.tools.join(", ")));
        }
        if !active.tool_denylist.is_empty() {
            rendered.push_str(&format!(
                "\n  disallowed: {}",
                active.tool_denylist.join(", ")
            ));
        }
        if let Some(path) = active.path.as_deref() {
            rendered.push_str(&format!("\n  path: {path}"));
        }
        if !item.shadowed.is_empty() {
            rendered.push_str(&format!(
                "\n  shadows {} lower-priority definition(s)",
                item.shadowed.len()
            ));
        }
    }

    if !result.failed.is_empty() {
        rendered.push_str("\n\nfailed agent files:");
        for failed in result.failed {
            rendered.push_str(&format!(
                "\n- {} {}: {}",
                agent_source_label(Some(failed.source)),
                failed.path,
                failed.message
            ));
        }
    }
    Ok(rendered)
}

pub(super) fn format_agent_show(resolved: &ResolvedAgent) -> String {
    let mut rendered = format_agent_definition_detail(&resolved.active);
    if !resolved.shadowed.is_empty() {
        rendered.push_str("\n\nShadowed:");
        for definition in &resolved.shadowed {
            rendered.push_str(&format!(
                "\n- {} [{}]{}: {}",
                definition.name,
                agent_source_label(definition.source),
                if definition.read_only || !agent_source_is_writable(definition.source) {
                    " read-only"
                } else {
                    ""
                },
                definition.description
            ));
            if let Some(path) = definition.path.as_deref() {
                rendered.push_str(&format!("\n  Path: {path}"));
            }
        }
    }
    rendered
}

fn format_agent_definition_detail(agent: &AgentDefinition) -> String {
    let mut rendered = format!(
        "{} [{}]{}",
        agent.name,
        agent_source_label(agent.source),
        if agent.read_only || !agent_source_is_writable(agent.source) {
            " read-only"
        } else {
            ""
        }
    );
    rendered.push_str(&format!("\nDescription: {}", agent.description));
    if !agent.tools.is_empty() {
        rendered.push_str(&format!("\nTools: {}", agent.tools.join(", ")));
    }
    if !agent.tool_denylist.is_empty() {
        rendered.push_str(&format!(
            "\nDisallowed tools: {}",
            agent.tool_denylist.join(", ")
        ));
    }
    if let Some(model) = agent.model.as_deref() {
        rendered.push_str(&format!("\nModel: {model}"));
    }
    if let Some(effort) = agent.effort.as_deref() {
        rendered.push_str(&format!("\nEffort: {effort}"));
    }
    if let Some(mode) = agent.permission_mode {
        rendered.push_str(&format!("\nPermission mode: {}", mode.as_str()));
    }
    if let Some(memory) = agent.memory_mode.as_deref() {
        rendered.push_str(&format!("\nMemory: {memory}"));
    }
    if agent.background {
        rendered.push_str("\nBackground: true");
    }
    if let Some(root) = agent.worktree_root.as_deref() {
        rendered.push_str(&format!("\nWorktree root: {root}"));
    }
    if let Some(isolation) = agent.isolation.as_deref() {
        rendered.push_str(&format!("\nIsolation: {isolation}"));
    }
    if let Some(color) = agent.color.as_deref() {
        rendered.push_str(&format!("\nColor: {color}"));
    }
    if !agent.skills.is_empty() {
        rendered.push_str(&format!("\nSkills: {}", agent.skills.join(", ")));
    }
    if let Some(max_turns) = agent.max_turns {
        rendered.push_str(&format!("\nMax turns: {max_turns}"));
    }
    if let Some(path) = agent.path.as_deref() {
        rendered.push_str(&format!("\nPath: {path}"));
    }
    rendered.push_str("\n\n");
    rendered.push_str(agent.instructions.trim());
    rendered
}

pub(super) fn agent_source_label(source: Option<AgentSource>) -> &'static str {
    match source {
        Some(AgentSource::BuiltIn) => "built-in",
        Some(AgentSource::ClaudeUser) => "claude-user",
        Some(AgentSource::User) => "user",
        Some(AgentSource::ClaudeProject) => "claude-project",
        Some(AgentSource::Project) => "project",
        Some(AgentSource::Plugin) => "plugin",
        Some(AgentSource::Cli) => "cli",
        Some(AgentSource::ManagedPolicy) => "managed-policy",
        None => "unknown",
    }
}

pub(super) fn agent_source_is_writable(source: Option<AgentSource>) -> bool {
    matches!(source, Some(AgentSource::Project | AgentSource::User))
}

pub(super) fn configured_agent_editor() -> Option<String> {
    env::var("OPPI_EDITOR")
        .or_else(|_| env::var("EDITOR"))
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

pub(super) fn launch_agent_editor(editor: &str, path: &Path) -> Result<(), String> {
    let mut parts = editor.split_whitespace();
    let Some(command) = parts.next() else {
        return Err("editor command is empty".to_string());
    };
    let status = Command::new(command)
        .args(parts)
        .arg(path)
        .status()
        .map_err(|error| format!("launch editor {editor}: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("editor {editor} exited with status {status}"))
    }
}

fn resolve_agent_markdown_path(cwd: &Path, path: &str) -> PathBuf {
    let path = PathBuf::from(path);
    if path.is_absolute() {
        path
    } else {
        cwd.join(path)
    }
}

pub(super) fn default_agent_markdown_path(
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

pub(super) fn goal_command_route(args: &str) -> Result<GoalCommandRoute, String> {
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

pub(super) fn goal_set_params(
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

pub(super) fn decode_goal_from_response(value: &Value) -> Result<Option<ThreadGoal>, String> {
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

pub(super) fn format_goal_response(value: &Value) -> String {
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

pub(super) fn format_goal_status_label(goal: Option<&ThreadGoal>) -> String {
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

pub(super) fn format_compact_tokens(value: i64) -> String {
    format_compact_i64(value).to_ascii_lowercase()
}

pub(super) fn format_reset_epoch(epoch_seconds: u64) -> String {
    let now = current_time_ms().unwrap_or_default() / 1000;
    if epoch_seconds <= now {
        return "now".to_string();
    }
    format_duration_ms((epoch_seconds - now).saturating_mul(1000))
}

fn trim_compact_suffix(value: String) -> String {
    value.replace(".0K", "K").replace(".0M", "M")
}

pub(super) fn format_memory_status_summary(value: &Value) -> String {
    let enabled = value
        .get("enabled")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let backend = value
        .get("backend")
        .and_then(Value::as_str)
        .unwrap_or("none");
    let scope = value
        .get("scope")
        .and_then(Value::as_str)
        .unwrap_or("project");
    let count = value
        .get("memoryCount")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    let mut parts = vec![
        enabled_label(enabled).to_string(),
        format!("backend {backend}"),
        format!("scope {scope}"),
        format!("{count} memories"),
    ];
    if let Some(owner) = value
        .get("executionOwner")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        parts.push(format!("owner {owner}"));
    }
    if let Some(adapter) = value
        .get("executionAdapter")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        parts.push(format!("adapter {adapter}"));
    }
    format!("memory runtime: {}", parts.join(" · "))
}

pub(super) fn format_memory_control(value: &Value) -> String {
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
    let initialized = status
        .get("initialized")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let pinned_count = status
        .get("pinnedCount")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let store_path = status
        .get("storePath")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty());
    let dashboard_url = status
        .get("dashboardUrl")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty());
    let execution_owner = status
        .get("executionOwner")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let execution_adapter = status
        .get("executionAdapter")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let mut lines = vec![
        title.to_string(),
        format!(
            "status: {} backend={} scope={} memories={} pinned={} initialized={}",
            if enabled { "enabled" } else { "disabled" },
            backend,
            scope,
            count,
            pinned_count,
            enabled_label(initialized)
        ),
    ];
    if let Some(execution_owner) = execution_owner {
        lines.push(format!("owner={execution_owner}"));
    }
    if let Some(execution_adapter) = execution_adapter {
        lines.push(format!("adapter={execution_adapter}"));
    }
    if let Some(store_path) = store_path {
        lines.push(format!("store={store_path}"));
    }
    if let Some(dashboard_url) = dashboard_url {
        lines.push(format!("dashboard={dashboard_url}"));
    }
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

pub(super) fn format_memory_dashboard(value: &Value) -> String {
    let available = value
        .get("available")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let message = value
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let status = value.get("status").cloned().unwrap_or_else(|| json!({}));
    let count = status
        .get("memoryCount")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let pinned_count = status
        .get("pinnedCount")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let store_path = status
        .get("storePath")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty());
    let url = value
        .get("url")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty());
    let fallback = value
        .get("fallbackUrl")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty());
    let mut lines = Vec::new();
    if available {
        if let Some(url) = url {
            lines.push(format!("Hoppi dashboard: {url}"));
        } else {
            lines.push("Hoppi dashboard: available".to_string());
        }
    } else {
        lines.push("Hoppi dashboard: unavailable".to_string());
    }
    if let Some(fallback) = fallback
        && url != Some(fallback)
    {
        lines.push(format!("Fallback: {fallback}"));
    }
    lines.push(format!("memories={count} pinned={pinned_count}"));
    if let Some(store_path) = store_path {
        lines.push(format!("store: {store_path}"));
    }
    if !message.trim().is_empty() {
        lines.push(message.to_string());
    }
    lines.join("\n")
}

pub(super) fn format_memory_list(value: &Value) -> String {
    let available = value
        .get("available")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let total_count = value.get("totalCount").and_then(Value::as_u64).unwrap_or(0);
    let items = value
        .get("items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let message = value
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let mut lines = Vec::new();
    if available {
        lines.push(format!("Hoppi memories: {total_count}"));
    } else {
        lines.push("Hoppi memories: unavailable".to_string());
    }
    if items.is_empty() {
        lines.push("No cached memories matched.".to_string());
    } else {
        for item in items {
            let id = item.get("id").and_then(Value::as_str).unwrap_or("memory");
            let content = item
                .get("content")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            let pinned = item.get("pinned").and_then(Value::as_bool).unwrap_or(false)
                || item
                    .get("starred")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
            let marker = if pinned { "[pinned] " } else { "" };
            let display_content = if content.chars().count() > 160 {
                format!("{}...", content.chars().take(157).collect::<String>())
            } else {
                content
            };
            lines.push(format!("- {marker}{id}: {display_content}"));
            let tags = item
                .get("tags")
                .and_then(Value::as_array)
                .map(|tags| {
                    tags.iter()
                        .filter_map(Value::as_str)
                        .filter(|tag| !tag.trim().is_empty())
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default();
            if !tags.is_empty() {
                lines.push(format!("  tags={tags}"));
            }
            let source = item
                .get("source")
                .and_then(Value::as_str)
                .filter(|source| !source.trim().is_empty());
            let confidence = item
                .get("confidence")
                .and_then(Value::as_str)
                .filter(|confidence| !confidence.trim().is_empty());
            if source.is_some() || confidence.is_some() {
                lines.push(format!(
                    "  source={} confidence={}",
                    source.unwrap_or("unknown"),
                    confidence.unwrap_or("unknown")
                ));
            }
        }
    }
    if !message.trim().is_empty() {
        lines.push(message.to_string());
    }
    lines.join("\n")
}

pub(super) fn format_memory_forget(value: &Value) -> String {
    let id = value.get("id").and_then(Value::as_str).unwrap_or("memory");
    let backend_applied = value
        .get("backendApplied")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let cache_updated = value
        .get("cacheUpdated")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let message = value
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let status = value.get("status").cloned().unwrap_or_else(|| json!({}));
    let count = status
        .get("memoryCount")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let pinned_count = status
        .get("pinnedCount")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let mut lines = vec![format!("Forgot memory: {id}")];
    lines.push(format!(
        "backend={} cache {}",
        if backend_applied {
            "applied"
        } else {
            "not reported"
        },
        if cache_updated {
            "updated"
        } else {
            "unchanged"
        }
    ));
    lines.push(format!("memories={count} pinned={pinned_count}"));
    if !message.trim().is_empty() {
        lines.push(message.to_string());
    }
    lines.join("\n")
}

pub(super) fn format_memory_pin(value: &Value) -> String {
    let id = value.get("id").and_then(Value::as_str).unwrap_or("memory");
    let action = value.get("action").and_then(Value::as_str).unwrap_or("pin");
    let backend_applied = value
        .get("backendApplied")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let cache_updated = value
        .get("cacheUpdated")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let message = value
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let status = value.get("status").cloned().unwrap_or_else(|| json!({}));
    let count = status
        .get("memoryCount")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let pinned_count = status
        .get("pinnedCount")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let title = if action == "unpin" {
        format!("Unpinned memory: {id}")
    } else {
        format!("Pinned memory: {id}")
    };
    let mut lines = vec![title];
    lines.push(format!(
        "backend={} cache {}",
        if backend_applied {
            "applied"
        } else {
            "not reported"
        },
        if cache_updated {
            "updated"
        } else {
            "unchanged"
        }
    ));
    lines.push(format!("memories={count} pinned={pinned_count}"));
    if !message.trim().is_empty() {
        lines.push(message.to_string());
    }
    lines.join("\n")
}

pub(super) fn format_memory_update(value: &Value) -> String {
    let id = value.get("id").and_then(Value::as_str).unwrap_or("memory");
    let backend_applied = value
        .get("backendApplied")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let cache_updated = value
        .get("cacheUpdated")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let message = value
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let status = value.get("status").cloned().unwrap_or_else(|| json!({}));
    let count = status
        .get("memoryCount")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let pinned_count = status
        .get("pinnedCount")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let mut lines = vec![format!("Updated memory: {id}")];
    lines.push(format!(
        "backend={} cache {}",
        if backend_applied {
            "applied"
        } else {
            "not reported"
        },
        if cache_updated {
            "updated"
        } else {
            "unchanged"
        }
    ));
    lines.push(format!("memories={count} pinned={pinned_count}"));
    if !message.trim().is_empty() {
        lines.push(message.to_string());
    }
    lines.join("\n")
}

pub(super) fn format_memory_supersede(value: &Value) -> String {
    let id = value.get("id").and_then(Value::as_str).unwrap_or("memory");
    let superseded_by = value
        .get("record")
        .and_then(|record| record.get("supersededBy"))
        .and_then(Value::as_str)
        .or_else(|| value.get("supersededBy").and_then(Value::as_str))
        .unwrap_or("replacement");
    let backend_applied = value
        .get("backendApplied")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let cache_updated = value
        .get("cacheUpdated")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let message = value
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let status = value.get("status").cloned().unwrap_or_else(|| json!({}));
    let count = status
        .get("memoryCount")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let pinned_count = status
        .get("pinnedCount")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let mut lines = vec![format!("Superseded memory: {id} -> {superseded_by}")];
    lines.push(format!(
        "backend={} cache {}",
        if backend_applied {
            "applied"
        } else {
            "not reported"
        },
        if cache_updated {
            "updated"
        } else {
            "unchanged"
        }
    ));
    lines.push(format!("memories={count} pinned={pinned_count}"));
    if !message.trim().is_empty() {
        lines.push(message.to_string());
    }
    lines.join("\n")
}

pub(super) fn format_memory_consolidate(value: &Value) -> String {
    let dry_run = value
        .get("dryRun")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let backend_applied = value
        .get("backendApplied")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let cache_updated = value
        .get("cacheUpdated")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let stage = value
        .get("stage")
        .and_then(Value::as_str)
        .filter(|stage| !stage.trim().is_empty())
        .unwrap_or("maintenance");
    let created = value
        .get("createdCount")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let deleted = value
        .get("deletedCount")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let total = value.get("totalCount").and_then(Value::as_u64).unwrap_or(0);
    let message = value
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let summary = value
        .get("summary")
        .and_then(Value::as_str)
        .filter(|summary| !summary.trim().is_empty());
    let model_used = value
        .get("modelUsed")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|model| !model.is_empty());
    let status = value.get("status").cloned().unwrap_or_else(|| json!({}));
    let count = status
        .get("memoryCount")
        .and_then(Value::as_u64)
        .unwrap_or(total);
    let pinned_count = status
        .get("pinnedCount")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let queue_depth = status.get("queueDepth").and_then(Value::as_u64);
    let mut lines = vec![format!(
        "Hoppi maintenance: {}",
        if dry_run { "preview" } else { "applied" }
    )];
    let mut stage_line = format!(
        "stage={stage} backend={} cache {}",
        if backend_applied {
            "applied"
        } else {
            "not reported"
        },
        if cache_updated {
            "updated"
        } else {
            "unchanged"
        }
    );
    if let Some(model_used) = model_used {
        stage_line.push_str(&format!(" model={model_used}"));
    }
    lines.push(stage_line);
    lines.push(format!(
        "created={created} deleted={deleted} total={total} memories={count} pinned={pinned_count}"
    ));
    if let Some(queue_depth) = queue_depth {
        lines.push(format!("queue={queue_depth}"));
    }
    if let Some(summary) = summary {
        lines.push(summary.to_string());
    }
    let deleted_ids = value
        .get("deletedIds")
        .and_then(Value::as_array)
        .map(|ids| {
            ids.iter()
                .filter_map(Value::as_str)
                .filter(|id| !id.trim().is_empty())
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    if !deleted_ids.is_empty() {
        lines.push(format!("deleted ids: {deleted_ids}"));
    }
    if !message.trim().is_empty() {
        lines.push(message.to_string());
    }
    lines.join("\n")
}

pub(super) fn memory_jobs_active_count(value: &Value) -> u64 {
    value
        .get("queueDepth")
        .and_then(Value::as_u64)
        .unwrap_or_else(|| {
            value
                .get("items")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter(|item| memory_job_is_active(item))
                        .count() as u64
                })
                .unwrap_or(0)
        })
}

fn memory_job_is_active(item: &Value) -> bool {
    matches!(
        item.get("status").and_then(Value::as_str),
        Some("queued" | "running")
    )
}

fn memory_active_job_labels(value: &Value) -> Vec<String> {
    value
        .get("items")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|item| memory_job_is_active(item))
        .take(5)
        .map(|item| {
            let id = item.get("id").and_then(Value::as_str).unwrap_or("job");
            let status = item
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("active");
            format!("{id}({status})")
        })
        .collect()
}

pub(super) fn format_memory_drain_timeout(value: &Value, timeout: Duration) -> String {
    let active = memory_jobs_active_count(value);
    let label = if active == 1 { "job" } else { "jobs" };
    let mut message = format!(
        "memory: drain timed out after {}s with {active} active {label}; continuing",
        timeout.as_secs()
    );
    let labels = memory_active_job_labels(value);
    if !labels.is_empty() {
        message.push_str(&format!(": {}", labels.join(", ")));
    }
    message
}

pub(super) fn format_memory_jobs(value: &Value) -> String {
    let queue_depth = value.get("queueDepth").and_then(Value::as_u64).unwrap_or(0);
    let total = value.get("totalCount").and_then(Value::as_u64).unwrap_or(0);
    let message = value
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let items = value
        .get("items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut lines = vec![format!("memory jobs: active={queue_depth} total={total}")];
    if !message.trim().is_empty() {
        lines.push(message.to_string());
    }
    if items.is_empty() {
        lines.push("no memory jobs recorded".to_string());
        return lines.join("\n");
    }
    for item in items.into_iter().take(8) {
        let id = item.get("id").and_then(Value::as_str).unwrap_or("job");
        let kind = item.get("kind").and_then(Value::as_str).unwrap_or("memory");
        let status = item
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let stage = item
            .get("stage")
            .and_then(Value::as_str)
            .filter(|stage| !stage.trim().is_empty());
        let records = item.get("recordCount").and_then(Value::as_u64).unwrap_or(0);
        let deleted = item
            .get("deletedCount")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let source = item
            .get("source")
            .and_then(Value::as_str)
            .filter(|source| !source.trim().is_empty());
        let mut row = format!("- {id}: {kind}");
        if let Some(stage) = stage {
            row.push_str(&format!(" stage={stage}"));
        }
        row.push_str(&format!(" status={status}"));
        if records > 0 {
            row.push_str(&format!(" records={records}"));
        }
        if deleted > 0 {
            row.push_str(&format!(" deleted={deleted}"));
        }
        if let Some(source) = source {
            row.push_str(&format!(" source={source}"));
        }
        lines.push(row);
        if let Some(message) = item
            .get("message")
            .and_then(Value::as_str)
            .filter(|message| !message.trim().is_empty())
        {
            lines.push(format!("  {message}"));
        }
    }
    lines.join("\n")
}

pub(super) fn format_prompt_templates(value: &Value) -> String {
    let items = value
        .get("items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if items.is_empty() {
        return "no prompt templates discovered".to_string();
    }
    let mut rendered =
        "prompts (active templates; project > plugin > user > built-in):".to_string();
    for item in items {
        let Some(active) = item.get("active") else {
            continue;
        };
        let name = active
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("prompt")
            .trim_start_matches('/');
        let source = active
            .get("source")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let description = active
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let argument_hint = active
            .get("argumentHint")
            .and_then(Value::as_str)
            .filter(|hint| !hint.trim().is_empty())
            .map(|hint| format!(" {}", hint.trim()))
            .unwrap_or_default();
        let shadowed = item
            .get("shadowed")
            .and_then(Value::as_array)
            .map(Vec::len)
            .unwrap_or(0);
        rendered.push_str(&format!(
            "\n- /{name}{argument_hint} [{source}]: {description}"
        ));
        if let Some(path) = active.get("path").and_then(Value::as_str) {
            rendered.push_str(&format!("\n  {path}"));
        }
        if shadowed > 0 {
            rendered.push_str(&format!(
                "\n  shadows {shadowed} lower-priority template(s)"
            ));
        }
    }
    rendered
}

pub(super) fn format_theme_status(
    current: &str,
    loaded: Option<&LoadedShellTheme>,
    list: Option<&Value>,
) -> String {
    let source = loaded
        .map(|theme| theme.source.as_str())
        .unwrap_or_else(|| {
            if normalize_theme_name(current).is_some() {
                "builtIn"
            } else {
                "unknown"
            }
        });
    let mut rendered = format!("theme: {current} [{source}]");
    let Some(list) = list else {
        return rendered;
    };
    let items = list
        .get("items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if items.is_empty() {
        return rendered;
    }
    rendered.push_str("\navailable themes:");
    for entry in active_theme_entries_from_theme_list(&Value::Object(
        [("items".to_string(), Value::Array(items))]
            .into_iter()
            .collect(),
    )) {
        rendered.push_str(&format!("\n- {} [{}]", entry.name, entry.source));
        if let Some(path) = entry.path {
            rendered.push_str(&format!("\n  {path}"));
        }
    }
    rendered
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ThemeListEntry {
    pub(super) name: String,
    pub(super) source: String,
    pub(super) path: Option<String>,
}

pub(super) fn active_theme_entries_from_theme_list(list: &Value) -> Vec<ThemeListEntry> {
    list.get("items")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let active = item.get("active")?;
            let name = active
                .get("name")
                .and_then(Value::as_str)?
                .trim()
                .to_string();
            if name.is_empty() {
                return None;
            }
            let source = active
                .get("source")
                .and_then(Value::as_str)
                .filter(|source| !source.trim().is_empty())
                .unwrap_or("unknown")
                .to_string();
            let path = active
                .get("path")
                .and_then(Value::as_str)
                .filter(|path| !path.trim().is_empty())
                .map(str::to_string);
            Some(ThemeListEntry { name, source, path })
        })
        .collect()
}

fn fallback_theme_preview_entries() -> Vec<ThemeListEntry> {
    ["oppi", "dark", "light", "plain"]
        .into_iter()
        .map(|name| ThemeListEntry {
            name: name.to_string(),
            source: "builtIn".to_string(),
            path: None,
        })
        .collect()
}

fn theme_preview_entries(list: Option<&Value>) -> Vec<ThemeListEntry> {
    let entries = list
        .map(active_theme_entries_from_theme_list)
        .unwrap_or_default();
    if entries.is_empty() {
        fallback_theme_preview_entries()
    } else {
        entries
    }
}

pub(super) fn theme_preview_index_arg(raw: &str) -> bool {
    let raw = raw.trim();
    !raw.is_empty() && raw.chars().all(|ch| ch.is_ascii_digit())
}

pub(super) fn theme_preview_index_selection(list: Option<&Value>, raw: &str) -> Option<String> {
    let index = raw.trim().parse::<usize>().ok()?;
    if index == 0 {
        return None;
    }
    theme_preview_entries(list)
        .get(index - 1)
        .map(|entry| entry.name.clone())
}

pub(super) fn format_theme_preview_index(current: &str, list: Option<&Value>) -> String {
    let mut rendered = format!("theme preview picker\ncurrent: {current}\n\navailable themes:");
    for (index, entry) in theme_preview_entries(list).into_iter().enumerate() {
        let number = index + 1;
        rendered.push_str(&format!("\n{number}. {} [{}]", entry.name, entry.source));
        if let Some(normalized) = normalize_theme_name(&entry.name) {
            rendered.push_str(&format!(
                "\n   {}",
                theme_preview_sample(theme_tokens(&normalized))
            ));
        }
        if let Some(path) = entry.path {
            rendered.push_str(&format!("\n   {path}"));
        }
    }
    rendered.push_str(
        "\n\nUse /theme preview <name> or /theme preview <number> to inspect one theme, then /theme <name> or /theme <number> to apply it. Example: /theme preview 2, then /theme 2.",
    );
    rendered
}

pub(super) fn format_theme_preview_detail(name: &str, source: &str, tokens: ThemeTokens) -> String {
    [
        format!("theme preview: {name} [{source}]"),
        theme_preview_sample(tokens),
        format!("Use /theme {name} to apply this theme, or /theme preview to list themes."),
    ]
    .join("\n")
}

fn theme_preview_sample(tokens: ThemeTokens) -> String {
    format!(
        "sample: {}assistant{} {}`code`{} {}tool ok{} {}warning{} {}error{}",
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
    )
}

pub(super) fn terminal_width() -> usize {
    env::var("COLUMNS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|width| *width >= 20)
        .unwrap_or(100)
}

pub(super) fn terminal_capability_summary(raw_requested: bool) -> &'static str {
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

pub(super) fn width_safe_line(line: &str, width: usize) -> String {
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

pub(super) fn login_usage() -> &'static str {
    "usage: /login, /login subscription, /login subscription codex [--force], /login subscription copilot [--force] [--enterprise <domain>], /login api, /login subscription claude [status|install --yes|start|use|stop], /login api openai env <ENV>. Model choice stays in /model or future /settings; raw keys are never accepted."
}

pub(super) fn normalize_login_choice(input: &str) -> String {
    input
        .trim()
        .trim_start_matches('/')
        .to_ascii_lowercase()
        .replace('_', "-")
}

pub(super) fn login_action_approved(flags: &[&str]) -> bool {
    flags.iter().any(|flag| {
        matches!(
            flag.trim().to_ascii_lowercase().as_str(),
            "--yes" | "yes" | "--approve" | "approve" | "--i-understand"
        )
    })
}

pub(super) fn login_root_picker_panel(provider: &ProviderConfig) -> String {
    format!(
        "login: native provider setup\ncurrent: {}\n\nChoose auth type:\n  1. Subscription\n  2. API\n\nType 1/2, or run `/login subscription` / `/login api`.\npolicy: credentials stay in Pi/OPPi auth store, env references, or a managed provider bridge; raw secrets are not stored or sent over JSON-RPC.\n{}",
        provider.label(),
        login_usage()
    )
}

pub(super) fn login_subscription_picker_panel() -> String {
    "login subscription providers\n  1. Codex (ChatGPT) — native browser OAuth; saves redacted Pi/OPPi auth-store token\n  2. Claude (Anthropic via Meridian) — explicit managed loopback bridge; install asks approval\n  3. Copilot (Microsoft/GitHub) — Pi-compatible device-code OAuth; saves redacted auth-store token\n\nType 1/2/3, or run `/login subscription codex`, `/login subscription copilot`, or `/login subscription claude`.".to_string()
}

pub(super) fn login_api_picker_panel() -> String {
    "login API providers\n  1. OpenAI-compatible API via env reference\n\nSet OPPI_OPENAI_API_KEY/OPENAI_API_KEY in your shell, then run `/login api openai env OPPI_OPENAI_API_KEY`. Raw keys are rejected. Use `/model <id>` for model selection.".to_string()
}

pub(super) fn login_openai_instructions() -> &'static str {
    "login API/OpenAI-compatible: set OPPI_OPENAI_API_KEY or OPENAI_API_KEY in your shell, then run `/login api openai env OPPI_OPENAI_API_KEY`. Use `/model <id>` or `/model role <role> <id>` for model selection. Raw API keys are rejected."
}

pub(super) fn login_claude_picker_panel() -> String {
    format!(
        "login Claude (Anthropic via Meridian)\n  1. Status\n  2. Run explicit Claude Code login (`claude login`)\n  3. Install managed Meridian bridge (asks approval; package {MERIDIAN_PACKAGE_NAME})\n  4. Start visible loopback bridge and configure provider\n  5. Use already-running bridge\n  6. Stop bridge started by this shell\n\nMeridian authenticates through the Claude Code SDK and refreshes tokens there; OPPi does not extract or store Claude tokens. Model selection stays in `/model` or `/model role ...`; `/login` never picks the model. No hidden npx/proxy/credential-helper spawning is used. Managed install target: {}",
        format_path_for_display(&managed_packages_dir())
    )
}

pub(super) fn login_meridian_install_approval_panel() -> String {
    format!(
        "Approval required: install managed Meridian bridge package {MERIDIAN_PACKAGE_NAME} into {}. This may use npm/network and can modify OPPi's managed packages directory. Type `yes` to approve, `no` to cancel, or run `/login subscription claude install --yes`.",
        format_path_for_display(&managed_packages_dir())
    )
}

pub(super) fn hoppi_install_approval_panel() -> String {
    format!(
        "Approval required: install/update Hoppi backend package {HOPPI_PACKAGE_NAME} into {}. This may use npm/network and can modify OPPi's managed packages directory. Type `yes` to approve, `no` to cancel, or run `/memory install-hoppi --yes`. OPPi performs no hidden install.",
        format_path_for_display(&managed_packages_dir())
    )
}

pub(super) fn login_delegated_provider_text(provider: &str) -> &'static str {
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
pub(super) struct GraphifyStatus {
    pub(super) cli_path: Option<PathBuf>,
    pub(super) graph_root: Option<PathBuf>,
    pub(super) legacy_root: Option<PathBuf>,
    pub(super) report_path: Option<PathBuf>,
    pub(super) wiki_index_path: Option<PathBuf>,
    pub(super) graph_json_path: Option<PathBuf>,
    pub(super) needs_update: bool,
    pub(super) config_path: Option<PathBuf>,
}

impl GraphifyStatus {
    pub(super) fn configured(&self) -> bool {
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

pub(super) fn graphify_status(cwd: &str) -> GraphifyStatus {
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

pub(super) fn graphify_status_json(status: &GraphifyStatus) -> Value {
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

pub(super) fn format_graphify_status(status: &GraphifyStatus) -> String {
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

pub(super) fn graphify_install_guidance() -> &'static str {
    "Graphify install is user-approved only. Suggested commands after approval:\n  npm install -g graphifyy\n  graphify install\n  graphify scope inspect . --scope auto\n  graphify detect .\nThe install command prints a mutation preview before writing assistant instructions, hooks, MCP, or plugin config. OPPi will not run it silently."
}

pub(super) fn graphify_command_guidance() -> &'static str {
    "Graphify commands:\n  graphify scope inspect . --scope auto\n  graphify detect .\n  graphify update .\n  graphify query \"show the auth flow\"\n  graphify path \"Frontend\" \"Database\"\n  graphify explain \"DigestAuth\"\n  graphify review-analysis\n  graphify portable-check .graphify\nUse --all only for intentional full recursive knowledge/document scans."
}
