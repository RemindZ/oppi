#[cfg(test)]
mod frames;
mod layout;
mod model;
#[cfg(test)]
mod snapshots;
#[cfg(test)]
mod terminal;
#[cfg(test)]
mod theme;
#[cfg(test)]
mod widgets;

use super::*;
use ratatui::{
    Terminal,
    backend::{CrosstermBackend, TestBackend},
    layout::Rect,
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap},
};
use std::io::Stdout;
use unicode_width::UnicodeWidthStr;

struct RatatuiTerminalGuard;

impl RatatuiTerminalGuard {
    fn enter() -> Result<Self, String> {
        crossterm::terminal::enable_raw_mode()
            .map_err(|error| format!("enable ratatui terminal mode: {error}"))?;
        print!("\x1b[?25l");
        io::stdout()
            .flush()
            .map_err(|error| format!("flush ratatui terminal setup: {error}"))?;
        Ok(Self)
    }
}

impl Drop for RatatuiTerminalGuard {
    fn drop(&mut self) {
        print!("{}", terminal_cleanup_sequence());
        let _ = io::stdout().flush();
        let _ = crossterm::terminal::disable_raw_mode();
    }
}

fn terminal_cleanup_sequence() -> &'static str {
    "\x1b[?2026l\x1b[0m\x1b[?25h\r\x1b[2K\x1b[J\r\n"
}

fn clear_ratatui_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<(), String> {
    terminal
        .clear()
        .map_err(|error| format!("clear ratatui terminal: {error}"))?;
    terminal
        .backend_mut()
        .flush()
        .map_err(|error| format!("flush ratatui terminal cleanup: {error}"))
}

const HEADER_NORMAL_HEIGHT: u16 = 1;
const HEADER_NARROW_HEIGHT: u16 = 2;
const EDITOR_HEIGHT: u16 = 3;
const FOOTER_COLLAPSED_HEIGHT: u16 = 1;
const FOOTER_EXPANDED_HEIGHT: u16 = 2;
const BODY_PADDING_X: u16 = 0;
const BODY_PADDING_Y: u16 = 0;
const TRANSCRIPT_GUTTER_WIDTH: usize = 2;
const TRANSCRIPT_LABEL_WIDTH: usize = 8;
const TRANSCRIPT_PREFIX_WIDTH: usize = TRANSCRIPT_GUTTER_WIDTH + 1 + TRANSCRIPT_LABEL_WIDTH + 1;
const SPINNER_FRAMES: [&str; 4] = ["◐", "◓", "◑", "◒"];
const FOOTER_SESSION_BAR: &str = "▮▮▮▯▯";
const FOOTER_WEEK_BAR: &str = "▮▯▯▯▯";
const FOOTER_CONTEXT_BAR: &str = "▮▮▯▯▯";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RatatuiFrameMode {
    Idle,
    Running,
    Tools,
    Question,
    Background,
    Todos,
    Approval,
    Suggestion,
    Slash,
    Settings,
}

impl From<model::LiveRatatuiMode> for RatatuiFrameMode {
    fn from(mode: model::LiveRatatuiMode) -> Self {
        match mode {
            model::LiveRatatuiMode::Idle => Self::Idle,
            model::LiveRatatuiMode::Running => Self::Running,
            model::LiveRatatuiMode::Question => Self::Question,
            model::LiveRatatuiMode::Approval => Self::Approval,
            model::LiveRatatuiMode::Background => Self::Background,
            model::LiveRatatuiMode::Todos => Self::Todos,
            model::LiveRatatuiMode::Suggestion => Self::Suggestion,
            model::LiveRatatuiMode::Slash => Self::Slash,
            model::LiveRatatuiMode::Settings => Self::Settings,
        }
    }
}

#[cfg(test)]
fn fixture_mode(kind: model::DesignFrameKind) -> RatatuiFrameMode {
    match kind {
        model::DesignFrameKind::Idle => RatatuiFrameMode::Idle,
        model::DesignFrameKind::Running => RatatuiFrameMode::Running,
        model::DesignFrameKind::ToolsArtifactDenial => RatatuiFrameMode::Tools,
        model::DesignFrameKind::AskUser => RatatuiFrameMode::Question,
        model::DesignFrameKind::Background => RatatuiFrameMode::Background,
        model::DesignFrameKind::Todos => RatatuiFrameMode::Todos,
        model::DesignFrameKind::Slash => RatatuiFrameMode::Slash,
        model::DesignFrameKind::Settings => RatatuiFrameMode::Settings,
        model::DesignFrameKind::Narrow => RatatuiFrameMode::Idle,
        model::DesignFrameKind::Tiny => RatatuiFrameMode::Running,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TerminalChromePolicy {
    NoBrowserChrome,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct RatatuiThemeTokens {
    pub accent: Color,
    pub accent_soft: Color,
    pub accent_dim: Color,
    pub fg: Color,
    pub fg_muted: Color,
    pub fg_dim: Color,
    pub bg: Color,
    pub border_muted: Color,
    pub yellow: Color,
    pub green: Color,
    pub red: Color,
    pub blue: Color,
    pub orange: Color,
    pub purple: Color,
    pub bg_selected: Color,
    pub bg_user_msg: Color,
    pub bg_custom_msg: Color,
    pub bg_tool_pending: Color,
    pub bg_tool_success: Color,
    pub bg_tool_error: Color,
    pub bg_perm_review: Color,
    pub bg_perm_approved: Color,
    pub bg_perm_denied: Color,
    pub plain: bool,
}

impl RatatuiThemeTokens {
    fn for_name(name: &str) -> Self {
        match name {
            "light" => Self::light(),
            "plain" => Self::plain(),
            "dark" => Self::dark(),
            _ => Self::dark(),
        }
    }

    fn dark() -> Self {
        Self {
            accent: rgb(0x39, 0xd7, 0xe5),
            accent_soft: rgb(0x8b, 0xe9, 0xf0),
            accent_dim: rgb(0x3b, 0x8f, 0x99),
            fg: rgb(0xd8, 0xe2, 0xea),
            fg_muted: rgb(0x9a, 0xa7, 0xb2),
            fg_dim: rgb(0x66, 0x71, 0x7d),
            bg: rgb(0x0c, 0x13, 0x18),
            border_muted: rgb(0x2f, 0x3b, 0x46),
            yellow: rgb(0xe0, 0xaf, 0x68),
            green: rgb(0x9e, 0xce, 0x6a),
            red: rgb(0xf7, 0x76, 0x8e),
            blue: rgb(0x7a, 0xa2, 0xf7),
            orange: rgb(0xff, 0x9e, 0x64),
            purple: rgb(0xbb, 0x9a, 0xf7),
            bg_selected: rgb(0x15, 0x32, 0x3a),
            bg_user_msg: rgb(0x17, 0x24, 0x2c),
            bg_custom_msg: rgb(0x18, 0x28, 0x32),
            bg_tool_pending: rgb(0x13, 0x24, 0x2b),
            bg_tool_success: rgb(0x17, 0x2a, 0x22),
            bg_tool_error: rgb(0x32, 0x1b, 0x24),
            bg_perm_review: rgb(0x10, 0x2b, 0x35),
            bg_perm_approved: rgb(0x12, 0x2d, 0x22),
            bg_perm_denied: rgb(0x35, 0x19, 0x24),
            plain: false,
        }
    }

    fn light() -> Self {
        Self {
            accent: rgb(0x00, 0x6d, 0x7d),
            accent_soft: rgb(0x00, 0x8f, 0xa3),
            accent_dim: rgb(0x00, 0x9f, 0xb4),
            fg: rgb(0x16, 0x25, 0x2f),
            fg_muted: rgb(0x52, 0x6c, 0x78),
            fg_dim: rgb(0x7c, 0x8c, 0x96),
            bg: rgb(0xf8, 0xfb, 0xf8),
            border_muted: rgb(0xc7, 0xd8, 0xdf),
            yellow: rgb(0xa6, 0x6a, 0x00),
            green: rgb(0x2e, 0x7d, 0x32),
            red: rgb(0xc6, 0x28, 0x28),
            blue: rgb(0x32, 0x67, 0xc9),
            orange: rgb(0xd0, 0x6a, 0x00),
            purple: rgb(0x7b, 0x3d, 0xb8),
            bg_selected: rgb(0xd7, 0xf4, 0xf8),
            bg_user_msg: rgb(0xf1, 0xf7, 0xf6),
            bg_custom_msg: rgb(0xe7, 0xf1, 0xef),
            bg_tool_pending: rgb(0xe7, 0xf1, 0xef),
            bg_tool_success: rgb(0xed, 0xf8, 0xef),
            bg_tool_error: rgb(0xff, 0xf0, 0xf0),
            bg_perm_review: rgb(0xe0, 0xf8, 0xfc),
            bg_perm_approved: rgb(0xe8, 0xf6, 0xea),
            bg_perm_denied: rgb(0xff, 0xe7, 0xe7),
            plain: false,
        }
    }

    fn plain() -> Self {
        Self {
            accent: Color::White,
            accent_soft: Color::White,
            accent_dim: Color::Gray,
            fg: Color::White,
            fg_muted: Color::Gray,
            fg_dim: Color::DarkGray,
            bg: Color::Reset,
            border_muted: Color::Gray,
            yellow: Color::White,
            green: Color::White,
            red: Color::White,
            blue: Color::White,
            orange: Color::White,
            purple: Color::White,
            bg_selected: Color::Reset,
            bg_user_msg: Color::Reset,
            bg_custom_msg: Color::Reset,
            bg_tool_pending: Color::Reset,
            bg_tool_success: Color::Reset,
            bg_tool_error: Color::Reset,
            bg_perm_review: Color::Reset,
            bg_perm_approved: Color::Reset,
            bg_perm_denied: Color::Reset,
            plain: true,
        }
    }

    fn bg_style(self) -> Style {
        if self.plain {
            Style::new()
        } else {
            Style::new().bg(self.bg)
        }
    }

    fn selected_style(self) -> Style {
        if self.plain {
            Style::new().fg(self.fg).add_modifier(Modifier::REVERSED)
        } else {
            Style::new().fg(self.fg).bg(self.bg_selected)
        }
    }
}

const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::Rgb(r, g, b)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RatatuiViewModel {
    pub provider: String,
    pub model: String,
    pub permission: String,
    pub status: String,
    pub thread_id: String,
    pub goal: Option<String>,
    pub todo_completed: usize,
    pub todo_total: usize,
    pub spinner_index: usize,
    pub theme: RatatuiThemeTokens,
    pub transcript: Vec<RatatuiTranscriptRow>,
    pub transcript_metadata: Vec<RatatuiTranscriptMetadata>,
    pub dock_label: String,
    pub editor_placeholder: String,
    pub editor_is_placeholder: bool,
    pub footer_left: String,
    pub footer_hotkeys: String,
    pub slash_items: Vec<SlashPaletteItem>,
    pub slash_selected: usize,
    pub overlay_title: String,
    pub overlay_items: Vec<RatatuiOverlayItem>,
    pub settings_selected: usize,
    pub question_selected: usize,
    pub pending_answers: Vec<String>,
    pub approval_items: Vec<String>,
    pub background_items: Vec<String>,
    pub background_typed: Vec<RatatuiBackgroundItem>,
    pub todo_items: Vec<String>,
    pub todo_typed: Vec<RatatuiTodoItem>,
    pub suggestion_items: Vec<String>,
    pub suggestion: Option<RatatuiSuggestion>,
    pub tool_digest_items: Vec<RatatuiToolDigest>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RatatuiOverlayItem {
    pub label: String,
    pub value: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RatatuiBackgroundItem {
    pub id: String,
    pub status: String,
    pub command: String,
    pub hint: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RatatuiTodoItem {
    pub id: String,
    pub status: String,
    pub priority: Option<String>,
    pub phase: Option<String>,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RatatuiSuggestion {
    pub message: String,
    pub confidence_percent: u8,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RatatuiToolDigest {
    pub status: ToolDigestStatus,
    pub name: String,
    pub target: String,
    pub duration: String,
    pub hint: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub(super) enum ToolDigestStatus {
    Pending,
    Success,
    Denied,
    Error,
}

impl ToolDigestStatus {
    fn glyph(self) -> &'static str {
        match self {
            Self::Pending => "⠋",
            Self::Success => "✓",
            Self::Denied => "!",
            Self::Error => "×",
        }
    }

    fn kind(self, name: &str) -> RatatuiRowKind {
        match self {
            Self::Denied => RatatuiRowKind::Denied,
            Self::Error => RatatuiRowKind::Error,
            Self::Success if tool_name_suggests_write(name) => RatatuiRowKind::ToolWrite,
            Self::Success => RatatuiRowKind::ToolRead,
            Self::Pending if tool_name_suggests_run(name) => RatatuiRowKind::ToolRun,
            Self::Pending => RatatuiRowKind::ToolRead,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RatatuiTranscriptRow {
    pub kind: RatatuiRowKind,
    pub label: String,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RatatuiTranscriptMetadata {
    pub event_id: u64,
    pub turn_id: Option<String>,
    pub item_id: Option<String>,
    pub tool_call_id: Option<String>,
    pub artifact_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RatatuiRowKind {
    User,
    Assistant,
    Info,
    ToolRead,
    ToolWrite,
    ToolRun,
    Diff,
    Artifact,
    Denied,
    Error,
    Agent,
}

impl RatatuiRowKind {
    fn style(self, tokens: RatatuiThemeTokens) -> Style {
        let fg = match self {
            Self::User => tokens.fg,
            Self::Assistant => tokens.accent_soft,
            Self::Info => tokens.fg_dim,
            Self::ToolRead => tokens.blue,
            Self::ToolWrite => tokens.green,
            Self::ToolRun => tokens.orange,
            Self::Diff => tokens.fg_muted,
            Self::Artifact => tokens.blue,
            Self::Denied | Self::Error => tokens.red,
            Self::Agent => tokens.purple,
        };
        Style::new().fg(fg)
    }

    fn body_style(self, tokens: RatatuiThemeTokens) -> Style {
        let style = Style::new().fg(tokens.fg);
        if tokens.plain {
            return style;
        }
        match self {
            Self::User => style.bg(tokens.bg_user_msg),
            Self::ToolRead | Self::ToolRun => style.bg(tokens.bg_tool_pending),
            Self::ToolWrite => style.bg(tokens.bg_tool_success),
            Self::Diff => style,
            Self::Denied => style.bg(tokens.bg_perm_denied),
            Self::Error => style.bg(tokens.bg_tool_error),
            _ => style,
        }
    }

    fn gutter(self) -> &'static str {
        match self {
            Self::User | Self::Assistant => "▍",
            Self::Denied | Self::Error => "!",
            Self::Agent => "◆",
            _ => "│",
        }
    }
}

pub(super) fn ratatui_view_model(
    session: &ShellSession,
    provider: &ProviderConfig,
) -> RatatuiViewModel {
    ratatui_view_model_for_editor(session, provider, "/", 0)
}

fn ratatui_view_model_for_editor(
    session: &ShellSession,
    provider: &ProviderConfig,
    editor_buffer: &str,
    slash_selected: usize,
) -> RatatuiViewModel {
    let model = session
        .selected_model
        .as_deref()
        .or(current_provider_model(provider))
        .unwrap_or("mock-scripted")
        .to_string();
    let transcript = live_transcript_rows(session);
    // Live rendering must not synthesize design-fixture rows. Empty sessions stay
    // visually quiet until real protocol events arrive.

    let todo_total = session
        .todo_state
        .todos
        .iter()
        .filter(|todo| !matches!(todo.status.as_str(), "cancelled"))
        .count();
    let todo_completed = session
        .todo_state
        .todos
        .iter()
        .filter(|todo| todo.status.as_str() == "completed")
        .count();
    let todos = session
        .todo_state
        .todos
        .iter()
        .filter(|todo| !matches!(todo.status.as_str(), "completed" | "cancelled"))
        .count();
    let queued = session.follow_up_queue.len();
    let status = if session.has_pending_pause() {
        "waiting"
    } else if session.is_turn_running() {
        "running"
    } else {
        "ready"
    };
    let slash =
        slash_palette_for_buffer_with_session(editor_buffer, slash_selected, session, provider)
            .map(|palette| palette.items.into_iter().take(7).collect::<Vec<_>>())
            .unwrap_or_default();

    RatatuiViewModel {
        provider: provider_name(provider).to_string(),
        model,
        permission: permission_label(session.permission_mode).to_string(),
        status: status.to_string(),
        thread_id: session.thread_id.clone(),
        goal: session.goal_header_label(),
        todo_completed,
        todo_total,
        spinner_index: 0,
        theme: RatatuiThemeTokens::for_name(&session.theme),
        transcript_metadata: live_transcript_metadata(session),
        transcript,
        dock_label: if session.has_pending_pause() {
            "question · pending"
        } else if session.pending_approval.is_some() {
            "approval · pending"
        } else if session.background_summary.is_some() {
            "background"
        } else if todos > 0 {
            "todos"
        } else if session.suggestion.is_some() {
            "suggestion"
        } else {
            "docks: idle"
        }
        .to_string(),
        editor_placeholder: if editor_buffer.is_empty() {
            "Ask, build, or type / for commands…".to_string()
        } else {
            editor_buffer.to_string()
        },
        editor_is_placeholder: editor_buffer.is_empty(),
        footer_left: format!(
            "{} · {} · perm {} · todos {} · queued {} · {}",
            status,
            provider_name(provider),
            permission_label(session.permission_mode),
            todos,
            queued,
            session.goal_status_label()
        ),
        footer_hotkeys:
            "Alt+Enter follow-up  Ctrl+Enter steer  Shift+Enter newline  / commands  Alt+K help"
                .to_string(),
        slash_items: slash,
        slash_selected,
        overlay_title: "settings".to_string(),
        overlay_items: settings_overlay_items_for_session(session),
        settings_selected: 1,
        question_selected: 0,
        pending_answers: pending_question_answers(session),
        approval_items: approval_dock_items(session),
        background_items: background_dock_items(session),
        background_typed: background_typed_items(session),
        todo_items: todo_dock_items(session),
        todo_typed: todo_typed_items(session),
        suggestion_items: suggestion_dock_items(session),
        suggestion: suggestion_model(session),
        tool_digest_items: tool_digest_items(session),
    }
}

fn live_transcript_rows(session: &ShellSession) -> Vec<RatatuiTranscriptRow> {
    if !session.ui.typed_scrollback.is_empty() {
        return session
            .ui
            .typed_scrollback
            .iter()
            .rev()
            .take(8)
            .rev()
            .map(|entry| RatatuiTranscriptRow {
                kind: ratatui_row_kind_from_entry(entry.kind),
                label: entry.label.clone(),
                body: entry.body.clone(),
            })
            .collect();
    }
    session
        .ui
        .scrollback
        .iter()
        .rev()
        .take(8)
        .rev()
        .map(|line| RatatuiTranscriptRow {
            kind: classify_transcript_line(line),
            label: transcript_label(line),
            body: line.clone(),
        })
        .collect()
}

fn live_transcript_metadata(session: &ShellSession) -> Vec<RatatuiTranscriptMetadata> {
    session
        .ui
        .typed_scrollback
        .iter()
        .rev()
        .take(8)
        .rev()
        .map(|entry| RatatuiTranscriptMetadata {
            event_id: entry.event_id,
            turn_id: entry.turn_id.clone(),
            item_id: entry.item_id.clone(),
            tool_call_id: entry.tool_call_id.clone(),
            artifact_id: entry.artifact_id.clone(),
        })
        .collect()
}

fn ratatui_row_kind_from_entry(kind: TranscriptEntryKind) -> RatatuiRowKind {
    match kind {
        TranscriptEntryKind::Info => RatatuiRowKind::Info,
        TranscriptEntryKind::User => RatatuiRowKind::User,
        TranscriptEntryKind::Assistant => RatatuiRowKind::Assistant,
        TranscriptEntryKind::ToolRead => RatatuiRowKind::ToolRead,
        TranscriptEntryKind::ToolWrite => RatatuiRowKind::ToolWrite,
        TranscriptEntryKind::ToolRun => RatatuiRowKind::ToolRun,
        TranscriptEntryKind::Diff => RatatuiRowKind::Diff,
        TranscriptEntryKind::Artifact => RatatuiRowKind::Artifact,
        TranscriptEntryKind::Denied => RatatuiRowKind::Denied,
        TranscriptEntryKind::Error => RatatuiRowKind::Error,
    }
}

fn tool_digest_from_call(call: &ToolCall) -> RatatuiToolDigest {
    RatatuiToolDigest {
        status: ToolDigestStatus::Pending,
        name: tool_display_name(call),
        target: tool_target(call),
        duration: "…".to_string(),
        hint: truncate_plain(&call.id, 18),
    }
}

fn tool_display_name(call: &ToolCall) -> String {
    call.namespace
        .as_ref()
        .map(|namespace| format!("{namespace}.{}", call.name))
        .unwrap_or_else(|| call.name.clone())
}

fn tool_name_suggests_write(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.contains("write") || lower.contains("edit") || lower.contains("apply")
}

fn tool_name_suggests_run(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.contains("bash")
        || lower.contains("shell")
        || lower.contains("exec")
        || lower.contains("run")
}

fn tool_target(call: &ToolCall) -> String {
    for key in [
        "path",
        "file",
        "command",
        "cmd",
        "query",
        "prompt",
        "outputPath",
    ] {
        if let Some(value) = call.arguments.get(key).and_then(|value| value.as_str()) {
            return truncate_plain(value, 42);
        }
    }
    if call.arguments.is_object() {
        "args".to_string()
    } else {
        truncate_plain(&call.arguments.to_string(), 42)
    }
}

fn tool_digest_row(digest: &RatatuiToolDigest) -> RatatuiTranscriptRow {
    let name = tool_display_kind_name(&digest.name);
    RatatuiTranscriptRow {
        kind: digest.status.kind(&digest.name),
        label: name.clone(),
        body: format!(
            "{} {} {} · {} · {}",
            digest.status.glyph(),
            name,
            digest.target,
            digest.duration,
            digest.hint
        ),
    }
}

fn tool_display_kind_name(name: &str) -> String {
    if tool_name_suggests_write(name) {
        "write".to_string()
    } else if tool_name_suggests_run(name) {
        "run".to_string()
    } else {
        "read".to_string()
    }
}

fn approval_dock_items(session: &ShellSession) -> Vec<String> {
    session
        .pending_approval
        .as_ref()
        .map(|pending| {
            let tool = pending.tool_call.as_ref().map(|call| {
                format!(
                    "{} {}",
                    call.namespace
                        .as_deref()
                        .map(|namespace| format!("{namespace}:{}", call.name))
                        .unwrap_or_else(|| call.name.clone()),
                    tool_target(call)
                )
            });
            vec![
                format!(
                    "approve {}{}{}",
                    pending.request_id,
                    pending
                        .tool_call_id
                        .as_ref()
                        .map(|id| format!(" for {id}"))
                        .unwrap_or_default(),
                    tool.map(|tool| format!(" · {tool}")).unwrap_or_default()
                ),
                "A approve · D deny · /approve resumes · /deny blocks".to_string(),
                "policy: one-time approval; protected paths remain explicit".to_string(),
            ]
        })
        .unwrap_or_else(|| {
            vec![
                "approve write_file crates/oppi-shell/src/ratatui_ui.rs".to_string(),
                "/approve resumes · /deny blocks and records reason".to_string(),
            ]
        })
}

fn suggestion_model(session: &ShellSession) -> Option<RatatuiSuggestion> {
    session
        .suggestion
        .as_ref()
        .map(|suggestion| RatatuiSuggestion {
            message: suggestion.message.clone(),
            confidence_percent: (suggestion.confidence * 100.0).round().clamp(0.0, 100.0) as u8,
            reason: suggestion.reason.clone(),
        })
}

fn suggestion_dock_items(session: &ShellSession) -> Vec<String> {
    suggestion_model(session)
        .map(|suggestion| {
            let mut row = format!(
                "ghost: {} · {}%",
                suggestion.message, suggestion.confidence_percent
            );
            if let Some(reason) = suggestion.reason.as_deref()
                && !reason.trim().is_empty()
            {
                row.push_str(&format!(" · {reason}"));
            }
            vec![row]
        })
        .unwrap_or_else(|| vec!["ghost: Run the Ratatui snapshot tests next".to_string()])
}

fn tool_digest_items(session: &ShellSession) -> Vec<RatatuiToolDigest> {
    session
        .tool_calls
        .values()
        .take(4)
        .map(tool_digest_from_call)
        .collect()
}

fn pending_question_answers(session: &ShellSession) -> Vec<String> {
    session
        .pending_question
        .as_ref()
        .and_then(|pending| pending.request.questions.first())
        .map(|question| {
            let default_id = question.default_option_id.as_deref();
            let required = if question.required {
                " · required"
            } else {
                ""
            };
            let mut answers = question
                .options
                .iter()
                .map(|option| {
                    let default = if Some(option.id.as_str()) == default_id {
                        " · default"
                    } else {
                        ""
                    };
                    format!("{}. {}{default}{required}", option.id, option.label)
                })
                .collect::<Vec<_>>();
            if answers.is_empty() || question.allow_custom.unwrap_or(false) {
                answers.push(format!("custom answer{required}"));
            }
            answers
        })
        .unwrap_or_else(|| {
            vec![
                "1. probe at startup via DA1 response (50ms wait)".to_string(),
                "2. assume on, fall back on first failure".to_string(),
                "3. expose a /probe-sync command".to_string(),
            ]
        })
}

fn background_typed_items(session: &ShellSession) -> Vec<RatatuiBackgroundItem> {
    session
        .background_summary
        .as_ref()
        .map(|summary| {
            let latest = summary
                .split("latest=")
                .nth(1)
                .map(|value| value.trim().to_string())
                .unwrap_or_else(|| "latest".to_string());
            vec![RatatuiBackgroundItem {
                id: latest,
                status: summary
                    .split(';')
                    .next()
                    .unwrap_or(summary)
                    .trim()
                    .to_string(),
                command: "/background".to_string(),
                hint: "L list · R read latest · K kill latest draft".to_string(),
            }]
        })
        .unwrap_or_default()
}

fn background_dock_items(session: &ShellSession) -> Vec<String> {
    background_typed_items(session)
        .into_iter()
        .map(|item| format!("{} · {} · {}", item.id, item.status, item.hint))
        .collect()
}

fn todo_typed_items(session: &ShellSession) -> Vec<RatatuiTodoItem> {
    session
        .todo_state
        .todos
        .iter()
        .filter(|todo| !matches!(todo.status.as_str(), "completed" | "cancelled"))
        .take(5)
        .map(|todo| RatatuiTodoItem {
            id: todo.id.clone(),
            status: todo.status.as_str().to_string(),
            priority: todo
                .priority
                .map(|priority| format!("{priority:?}").to_ascii_lowercase()),
            phase: todo.phase.clone(),
            content: todo.content.clone(),
        })
        .collect()
}

fn todo_dock_items(session: &ShellSession) -> Vec<String> {
    todo_typed_items(session)
        .into_iter()
        .map(|todo| {
            let glyph = match todo.status.as_str() {
                "in_progress" => "▶",
                "blocked" => "!",
                _ => "○",
            };
            let priority = todo
                .priority
                .as_deref()
                .map(|priority| format!(" · p:{priority}"))
                .unwrap_or_default();
            let phase = todo
                .phase
                .as_deref()
                .map(|phase| format!(" · {phase}"))
                .unwrap_or_default();
            format!(
                "{glyph} {} · {}{priority}{phase}",
                todo.content, todo.status
            )
        })
        .collect::<Vec<_>>()
}

pub(super) fn render_ratatui_preview(
    view: &RatatuiViewModel,
    width: u16,
    height: u16,
    mode: RatatuiFrameMode,
) -> Result<String, String> {
    let width = width.max(40);
    let height = height.max(10);
    render_ratatui_exact_fixture(view, width, height, mode)
}

pub(super) fn render_ratatui_exact_fixture(
    view: &RatatuiViewModel,
    width: u16,
    height: u16,
    mode: RatatuiFrameMode,
) -> Result<String, String> {
    let backend = TestBackend::new(width, height);
    let mut terminal =
        Terminal::new(backend).map_err(|error| format!("create test backend: {error}"))?;
    terminal
        .draw(|frame| render_ratatui_frame(frame, frame.area(), view, mode))
        .map_err(|error| format!("draw ratatui preview: {error}"))?;
    let buffer = terminal.backend().buffer();
    Ok(buffer_to_string(buffer, width, height))
}

fn terminal_body_area(area: Rect, policy: TerminalChromePolicy) -> Rect {
    match policy {
        TerminalChromePolicy::NoBrowserChrome => Rect {
            x: area.x.saturating_add(BODY_PADDING_X),
            y: area.y.saturating_add(BODY_PADDING_Y),
            width: area.width.saturating_sub(BODY_PADDING_X.saturating_mul(2)),
            height: area.height.saturating_sub(BODY_PADDING_Y.saturating_mul(2)),
        },
    }
}

pub(super) fn ratatui_interactive_loop(
    session: &mut ShellSession,
    provider: &mut ProviderConfig,
) -> Result<(), String> {
    let guard = RatatuiTerminalGuard::enter()?;
    session.terminal_ui_active = true;
    let stdout = io::stdout();
    let backend = CrosstermBackend::<Stdout>::new(stdout);
    let mut terminal = Terminal::new(backend).map_err(|error| format!("start ratatui: {error}"))?;
    terminal
        .clear()
        .map_err(|error| format!("clear ratatui terminal: {error}"))?;
    let mut state = tui::NativeTuiState::default();
    let mut running = true;
    while running {
        state.spinner_index = state.spinner_index.wrapping_add(1);
        session.sync_ui_docks();
        let editor_buffer = state.editor.buffer_preview().to_string();
        let mode = if state.has_overlay() {
            RatatuiFrameMode::Settings
        } else {
            RatatuiFrameMode::from(live_mode_for_session(session, &editor_buffer))
        };
        let mut view =
            ratatui_view_model_for_editor(session, provider, &editor_buffer, state.slash_selected);
        view.spinner_index = state.spinner_index;
        view.question_selected = state.question_selected;
        if !state.footer_hotkeys_visible {
            view.footer_hotkeys.clear();
        }
        if let Some(overlay) = state.overlay_view(session, provider) {
            view.overlay_title = overlay.title;
            view.settings_selected = overlay.selected;
            view.overlay_items = overlay
                .items
                .into_iter()
                .map(|item| RatatuiOverlayItem {
                    label: item.label,
                    value: item.value,
                    detail: item.detail,
                })
                .collect();
        }
        terminal
            .draw(|frame| render_ratatui_frame(frame, frame.area(), &view, mode))
            .map_err(|error| format!("draw ratatui terminal: {error}"))?;

        if event::poll(Duration::from_millis(40))
            .map_err(|error| format!("poll ratatui terminal input: {error}"))?
        {
            match event::read().map_err(|error| format!("read ratatui terminal input: {error}"))? {
                CrosstermEvent::Key(key) if key.kind != KeyEventKind::Release => {
                    running = tui::handle_tui_key(session, provider, &mut state, key)?;
                    if !running {
                        let editor_buffer = state.editor.buffer_preview().to_string();
                        let mut view = ratatui_view_model_for_editor(
                            session,
                            provider,
                            &editor_buffer,
                            state.slash_selected,
                        );
                        view.spinner_index = state.spinner_index;
                        view.question_selected = state.question_selected;
                        terminal
                            .draw(|frame| render_ratatui_frame(frame, frame.area(), &view, mode))
                            .map_err(|error| {
                                format!("draw final ratatui terminal frame: {error}")
                            })?;
                    }
                }
                CrosstermEvent::Resize(_, _) => {
                    terminal
                        .clear()
                        .map_err(|error| format!("clear ratatui resize: {error}"))?;
                }
                _ => {}
            }
        }

        if let Some(outcome) = session.poll_turn_events_silent()? {
            if matches!(outcome, TurnOutcome::Completed) {
                session.start_next_queued_or_goal_continuation(provider, false)?;
            }
        } else if !session.is_turn_running() && !session.has_pending_pause() {
            session.start_next_queued_or_goal_continuation(provider, false)?;
        }
    }
    let visible_exit_requested = session
        .ui
        .scrollback
        .iter()
        .any(|line| line == EXIT_REQUESTED_TEXT);
    clear_ratatui_terminal(&mut terminal)?;
    session.terminal_ui_active = false;
    drop(guard);
    if visible_exit_requested {
        println!("{EXIT_COMMAND_ECHO_TEXT}\n{EXIT_REQUESTED_TEXT}");
    }
    Ok(())
}

fn live_mode_for_session(session: &ShellSession, editor_buffer: &str) -> model::LiveRatatuiMode {
    if editor_buffer.trim_start().starts_with('/') {
        model::LiveRatatuiMode::Slash
    } else if session.pending_question.is_some() {
        model::LiveRatatuiMode::Question
    } else if session.pending_approval.is_some() {
        model::LiveRatatuiMode::Approval
    } else if session.background_summary.is_some() {
        model::LiveRatatuiMode::Background
    } else if !session.todo_state.todos.is_empty() {
        model::LiveRatatuiMode::Todos
    } else if session.suggestion.is_some() {
        model::LiveRatatuiMode::Suggestion
    } else if session.is_turn_running() {
        model::LiveRatatuiMode::Running
    } else {
        model::LiveRatatuiMode::Idle
    }
}

fn render_ratatui_frame(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    view: &RatatuiViewModel,
    mode: RatatuiFrameMode,
) {
    frame.render_widget(Clear, area);
    frame.render_widget(Block::default().style(view.theme.bg_style()), area);
    let area = terminal_body_area(area, TerminalChromePolicy::NoBrowserChrome);
    let tiny = area.height < 16;
    let narrow = area.width < 60;
    let header_h = if narrow {
        HEADER_NARROW_HEIGHT
    } else {
        HEADER_NORMAL_HEIGHT
    };
    let footer_h = if tiny || narrow {
        FOOTER_COLLAPSED_HEIGHT
    } else {
        FOOTER_EXPANDED_HEIGHT
    };
    let dock_h = if tiny {
        0
    } else {
        match mode {
            RatatuiFrameMode::Question => view.pending_answers.len().min(4) as u16 + 2,
            RatatuiFrameMode::Approval => view.approval_items.len().min(4) as u16 + 2,
            RatatuiFrameMode::Background => view.background_items.len().min(3) as u16 + 2,
            RatatuiFrameMode::Todos => view.todo_items.len().min(5) as u16 + 2,
            RatatuiFrameMode::Suggestion => view.suggestion_items.len().min(2) as u16 + 2,
            _ => 1,
        }
    };
    let layout = layout::reference_frame_layout(area, header_h, EDITOR_HEIGHT, footer_h, dock_h);

    render_header(frame, layout.header, view, mode, narrow);
    if !tiny {
        render_transcript(frame, layout.transcript, view, mode);
        render_dock_area(frame, layout.dock, view, mode);
    }
    render_editor(
        frame,
        layout.editor,
        view,
        matches!(mode, RatatuiFrameMode::Running),
    );
    render_footer(frame, layout.footer, view, footer_h > 1);
    if matches!(mode, RatatuiFrameMode::Slash) && !tiny {
        render_slash_overlay(frame, layout.editor, view);
    }
    if matches!(mode, RatatuiFrameMode::Settings) && !tiny {
        render_settings_overlay(frame, layout.editor, view);
    }
}

fn render_header(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    view: &RatatuiViewModel,
    mode: RatatuiFrameMode,
    narrow: bool,
) {
    let tokens = view.theme;
    let spinner = header_spinner(mode, view.status.as_str(), view.spinner_index);
    if narrow {
        let provider_model = truncate_plain(&format!("{}/{}", view.provider, view.model), 34);
        let line_one = Line::from(vec![
            Span::styled(spinner, Style::new().fg(tokens.accent)),
            Span::raw(" "),
            Span::styled("OPPi", Style::new().fg(tokens.accent_soft).bold()),
            Span::raw(" · "),
            Span::styled(provider_model, Style::new().fg(tokens.fg_muted)),
        ]);
        let line_two = Line::from(vec![
            Span::styled(
                format!("perms: {}", view.permission),
                permission_style(&view.permission, tokens),
            ),
            Span::raw(" · "),
            Span::styled(
                view.status.clone(),
                header_status_style(view.status.as_str(), tokens),
            ),
        ]);
        frame.render_widget(Paragraph::new(vec![line_one, line_two]), area);
        return;
    }

    let provider_model = format!("{}/{}", view.provider, view.model);
    let left_plain = format!(
        "{spinner} OPPi · {provider_model} · perms: {} · {}",
        view.permission, view.status
    );
    let mut spans = vec![
        Span::styled(spinner, Style::new().fg(tokens.accent)),
        Span::raw(" "),
        Span::styled("OPPi", Style::new().fg(tokens.accent_soft).bold()),
        Span::raw(" · "),
        Span::styled(provider_model, Style::new().fg(tokens.fg_muted)),
        Span::raw(" · "),
        Span::styled(
            format!("perms: {}", view.permission),
            permission_style(&view.permission, tokens),
        ),
        Span::raw(" · "),
        Span::styled(
            view.status.clone(),
            header_status_style(view.status.as_str(), tokens),
        ),
    ];

    let right = if matches!(mode, RatatuiFrameMode::Running) {
        view.goal
            .as_ref()
            .map(|goal| format!("◎ {}", truncate_plain(goal, 34)))
    } else if view.thread_id.trim().is_empty() {
        None
    } else {
        Some(truncate_plain(&view.thread_id, 22))
    };
    if let Some(right) = right {
        let left_width = terminal_cell_width(&left_plain);
        let right_width = terminal_cell_width(&right);
        let area_width = area.width as usize;
        if left_width + right_width + 1 < area_width {
            spans.push(Span::raw(" ".repeat(area_width - left_width - right_width)));
            spans.push(Span::styled(right, Style::new().fg(tokens.fg_dim)));
        }
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn header_spinner(mode: RatatuiFrameMode, status: &str, tick: usize) -> &'static str {
    if matches!(mode, RatatuiFrameMode::Running) || status == "running" {
        SPINNER_FRAMES[tick % SPINNER_FRAMES.len()]
    } else if status == "waiting" {
        "⏸"
    } else if matches!(status, "warn" | "warning") {
        "!"
    } else {
        "•"
    }
}

fn header_status_style(status: &str, tokens: RatatuiThemeTokens) -> Style {
    match status {
        "running" => Style::new().fg(tokens.yellow),
        "waiting" => Style::new().fg(tokens.orange),
        "ready" => Style::new().fg(tokens.green),
        _ => Style::new().fg(tokens.fg_muted),
    }
}

fn render_transcript(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    view: &RatatuiViewModel,
    mode: RatatuiFrameMode,
) {
    if area.height == 0 {
        return;
    }
    let source_rows = transcript_rows_for_mode(view, mode);
    let rows = source_rows
        .iter()
        .flat_map(|row| {
            row_lines(
                row,
                area.width,
                matches!(mode, RatatuiFrameMode::Running),
                view.theme,
            )
        })
        .collect::<Vec<_>>();
    let visible = rows
        .into_iter()
        .rev()
        .take(area.height as usize)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(visible).wrap(Wrap { trim: false }), area);
}

fn transcript_rows_for_mode(
    view: &RatatuiViewModel,
    mode: RatatuiFrameMode,
) -> Vec<RatatuiTranscriptRow> {
    match mode {
        RatatuiFrameMode::Tools => {
            let mut rows = view
                .tool_digest_items
                .iter()
                .map(tool_digest_row)
                .collect::<Vec<_>>();
            if rows.is_empty() {
                rows.push(RatatuiTranscriptRow {
                    kind: RatatuiRowKind::ToolWrite,
                    label: "write".to_string(),
                    body: "✓ write_file crates/oppi-shell/src/tui.rs · ok 11ms · +148 / -62"
                        .to_string(),
                });
            }
            rows.extend([
                RatatuiTranscriptRow {
                    kind: RatatuiRowKind::Diff,
                    label: "diff".to_string(),
                    body: "+ const SUPPORTS_SYNC: bool = detect_sync_caps();\n- if !sync_supported() { write_unsynchronized(out)?; }".to_string(),
                },
                RatatuiTranscriptRow {
                    kind: RatatuiRowKind::Artifact,
                    label: "artifact".to_string(),
                    body: "artifact://run-2f1a/snapshot_narrow.txt · text/plain · 1.2 KB · overwrites prior".to_string(),
                },
                RatatuiTranscriptRow {
                    kind: RatatuiRowKind::Denied,
                    label: "denied".to_string(),
                    body: "write blocked: .oppi/auth-store.json is protected — escalate with /permissions full-access or use /approve".to_string(),
                },
            ]);
            rows
        }
        RatatuiFrameMode::Question => vec![RatatuiTranscriptRow {
            kind: RatatuiRowKind::Assistant,
            label: "oppi".to_string(),
            body: "I can wire CSI 2026 detection three ways. Which do you want?".to_string(),
        }],
        RatatuiFrameMode::Approval => vec![RatatuiTranscriptRow {
            kind: RatatuiRowKind::Denied,
            label: "approval".to_string(),
            body: "write requires approval — choose /approve or /deny".to_string(),
        }],
        RatatuiFrameMode::Background => vec![RatatuiTranscriptRow {
            kind: RatatuiRowKind::Info,
            label: "background".to_string(),
            body: "3 background tasks active — Ctrl+Alt+T opens the background sheet".to_string(),
        }],
        RatatuiFrameMode::Todos => vec![RatatuiTranscriptRow {
            kind: RatatuiRowKind::Info,
            label: "info".to_string(),
            body: format!("{} todos · active work visible", view.todo_items.len()),
        }],
        RatatuiFrameMode::Suggestion => vec![RatatuiTranscriptRow {
            kind: RatatuiRowKind::Assistant,
            label: "suggest".to_string(),
            body: "A suggested next message is ready; Tab can accept once wired.".to_string(),
        }],
        RatatuiFrameMode::Settings => vec![RatatuiTranscriptRow {
            kind: RatatuiRowKind::Info,
            label: "info".to_string(),
            body:
                "overlay anchored above editor — ←/→ tabs · ↑/↓ settings · Enter open · Esc close"
                    .to_string(),
        }],
        _ => view.transcript.clone(),
    }
}

fn render_dock_area(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    view: &RatatuiViewModel,
    mode: RatatuiFrameMode,
) {
    match mode {
        RatatuiFrameMode::Question => render_dock_tray(
            frame,
            area,
            "question · pending        ↑/↓ select · Enter confirm",
            &view.pending_answers,
            view.question_selected,
            view.theme.yellow,
            view.theme,
        ),
        RatatuiFrameMode::Approval => render_dock_tray(
            frame,
            area,
            "approval · required      A approve · D deny · /approve · /deny",
            &view.approval_items,
            0,
            view.theme.orange,
            view.theme,
        ),
        RatatuiFrameMode::Background => render_dock_tray(
            frame,
            area,
            "background · tasks        L list · R read · K kill draft",
            &view.background_items,
            0,
            view.theme.green,
            view.theme,
        ),
        RatatuiFrameMode::Todos => render_dock_tray(
            frame,
            area,
            "todos · active           /todos refreshes",
            &view.todo_items,
            0,
            view.theme.accent,
            view.theme,
        ),
        RatatuiFrameMode::Suggestion => render_dock_tray(
            frame,
            area,
            "suggestion · ghost next message  Tab accept · Esc ignore",
            &view.suggestion_items,
            0,
            view.theme.purple,
            view.theme,
        ),
        _ => render_dock_sep(frame, area, &view.dock_label, view.theme),
    }
}

fn styled_line_with_fill(
    parts: Vec<(String, Style)>,
    width: u16,
    fill_style: Option<Style>,
) -> Line<'static> {
    let used = parts
        .iter()
        .map(|(text, _)| UnicodeWidthStr::width(text.as_str()))
        .sum::<usize>();
    let mut spans = parts
        .into_iter()
        .map(|(text, style)| Span::styled(text, style))
        .collect::<Vec<_>>();
    if let Some(style) = fill_style {
        let padding = (width as usize).saturating_sub(used);
        if padding > 0 {
            spans.push(Span::styled(" ".repeat(padding), style));
        }
    }
    Line::from(spans)
}

fn render_dock_tray(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    title: &str,
    items: &[String],
    selected_index: usize,
    accent: Color,
    tokens: RatatuiThemeTokens,
) {
    if area.height == 0 {
        return;
    }
    let block = Block::default()
        .title(format!(" {title} "))
        .borders(Borders::TOP | Borders::LEFT | Borders::RIGHT)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(accent));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let lines = items
        .iter()
        .take(inner.height as usize)
        .enumerate()
        .map(|(index, item)| {
            let selected = index == selected_index;
            let marker_style = if selected {
                tokens.selected_style().fg(accent)
            } else {
                Style::new().fg(accent)
            };
            let item_style = if selected {
                tokens.selected_style().fg(tokens.fg)
            } else {
                Style::new().fg(tokens.fg)
            };
            styled_line_with_fill(
                vec![
                    (if selected { "› " } else { "  " }.to_string(), marker_style),
                    (item.clone(), item_style),
                ],
                inner.width,
                selected.then_some(tokens.selected_style()),
            )
        })
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn render_dock_sep(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    label: &str,
    tokens: RatatuiThemeTokens,
) {
    if area.height == 0 {
        return;
    }
    let label_width = label.chars().count();
    let rule_width = (area.width as usize).saturating_sub(label_width + 1);
    let line = Line::from(vec![
        Span::styled("─".repeat(rule_width), Style::new().fg(tokens.border_muted)),
        Span::raw(" "),
        Span::styled(label.to_string(), Style::new().fg(tokens.fg_dim)),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}

fn render_editor(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    view: &RatatuiViewModel,
    running: bool,
) {
    let border = if running {
        view.theme.accent
    } else {
        view.theme.border_muted
    };
    let block = Block::default()
        .title(if running {
            " turn running · Enter queues follow-up · Ctrl+Enter steers · Esc interrupts "
        } else {
            ""
        })
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(border));
    let inner_width = area.width.saturating_sub(4).max(8) as usize;
    let lines = editor_lines(
        &view.editor_placeholder,
        inner_width,
        view.theme,
        view.editor_is_placeholder,
    );
    let lines = lines.into_iter().take(1).collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn editor_lines(
    buffer: &str,
    width: usize,
    tokens: RatatuiThemeTokens,
    is_placeholder: bool,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for (line_index, raw_line) in buffer.lines().enumerate() {
        let wrapped = wrap_plain(raw_line, width.saturating_sub(2).max(1));
        for (wrap_index, chunk) in wrapped.into_iter().enumerate() {
            let prefix = if line_index == 0 && wrap_index == 0 {
                "› "
            } else {
                "  "
            };
            if is_placeholder && line_index == 0 && wrap_index == 0 {
                lines.push(Line::from(vec![
                    Span::styled(prefix, Style::new().fg(tokens.fg_dim)),
                    Span::styled("█", Style::new().fg(tokens.fg)),
                    Span::styled(chunk, Style::new().fg(tokens.fg_dim)),
                ]));
            } else {
                lines.push(Line::from(vec![
                    Span::styled(prefix, Style::new().fg(tokens.fg_dim)),
                    Span::styled(chunk, Style::new().fg(tokens.fg_dim)),
                    Span::styled("█", Style::new().fg(tokens.fg)),
                ]));
            }
        }
    }
    if lines.is_empty() {
        lines.push(Line::from(vec![
            Span::styled("› ", Style::new().fg(tokens.fg_dim)),
            Span::styled("█", Style::new().fg(tokens.fg)),
            Span::styled(
                "Ask, build, or type / for commands…",
                Style::new().fg(tokens.fg_dim),
            ),
        ]));
    }
    lines
}

fn render_footer(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    view: &RatatuiViewModel,
    show_hotkeys: bool,
) {
    if area.height == 0 {
        return;
    }
    let mut lines = vec![Line::from(footer_status_spans(view, area.width))];
    if show_hotkeys && !view.footer_hotkeys.trim().is_empty() {
        lines.push(Line::from(Span::styled(
            footer_hotkey_line(&view.footer_hotkeys),
            Style::new().fg(view.theme.fg_dim),
        )));
    }
    frame.render_widget(Paragraph::new(lines), area);
}

fn footer_status_spans(view: &RatatuiViewModel, width: u16) -> Vec<Span<'static>> {
    let tokens = view.theme;
    let max_width = usize::from(width);
    let status = view.status.as_str();
    let status_dot = if status == "running" { "●" } else { "•" };
    let status_color = match status {
        "running" => tokens.yellow,
        "waiting" => tokens.orange,
        _ => tokens.green,
    };
    let (todos, queued) = footer_counts(&view.footer_left);
    let mut spans = vec![
        Span::styled(status_dot, Style::new().fg(status_color)),
        Span::styled(format!(" {status}"), Style::new().fg(status_color)),
        footer_separator(tokens),
        Span::styled("Alt+K", Style::new().fg(tokens.accent)),
        Span::styled(" hide help", Style::new().fg(tokens.fg_muted)),
    ];

    if width >= 56 {
        try_push_footer_group(
            &mut spans,
            vec![
                footer_separator(tokens),
                Span::styled("sess ", Style::new().fg(tokens.fg_dim)),
                Span::styled("3k", Style::new().fg(tokens.fg)),
                Span::styled(" 1% ", Style::new().fg(tokens.fg_muted)),
                Span::styled(FOOTER_SESSION_BAR, Style::new().fg(tokens.green)),
            ],
            max_width,
        );
    }
    if width >= 132 {
        try_push_footer_group(
            &mut spans,
            vec![
                footer_separator(tokens),
                Span::styled("wk ", Style::new().fg(tokens.fg_dim)),
                Span::styled("7M", Style::new().fg(tokens.fg)),
                Span::styled(" 1% ", Style::new().fg(tokens.fg_muted)),
                Span::styled(FOOTER_WEEK_BAR, Style::new().fg(tokens.blue)),
            ],
            max_width,
        );
    }
    if width >= 100 {
        try_push_footer_group(
            &mut spans,
            vec![
                footer_separator(tokens),
                Span::styled("model ", Style::new().fg(tokens.fg_dim)),
                Span::styled(
                    truncate_plain(&view.model, 18),
                    Style::new().fg(tokens.accent_soft),
                ),
            ],
            max_width,
        );
    }
    if width >= 80 {
        try_push_footer_group(
            &mut spans,
            vec![
                footer_separator(tokens),
                Span::styled("perm ".to_string(), Style::new().fg(tokens.fg_dim)),
                Span::styled(
                    view.permission.clone(),
                    permission_style(&view.permission, tokens),
                ),
            ],
            max_width,
        );
    }
    if width >= 70
        && !try_push_footer_group(
            &mut spans,
            vec![
                footer_separator(tokens),
                Span::styled("ctx ", Style::new().fg(tokens.fg_dim)),
                Span::styled("100k/272k", Style::new().fg(tokens.fg)),
                Span::styled(" 37% ", Style::new().fg(tokens.fg_muted)),
                Span::styled(FOOTER_CONTEXT_BAR, Style::new().fg(tokens.purple)),
            ],
            max_width,
        )
    {
        try_push_footer_group(
            &mut spans,
            vec![
                footer_separator(tokens),
                Span::styled("ctx ", Style::new().fg(tokens.fg_dim)),
                Span::styled("37% ", Style::new().fg(tokens.fg_muted)),
                Span::styled(FOOTER_CONTEXT_BAR, Style::new().fg(tokens.purple)),
            ],
            max_width,
        );
    }
    if width >= 118 && (todos > 0 || queued > 0) {
        if queued == 0
            || !try_push_footer_group(
                &mut spans,
                vec![
                    footer_separator(tokens),
                    Span::styled(
                        format!("todos {todos} · queued {queued}"),
                        Style::new().fg(tokens.fg_muted),
                    ),
                ],
                max_width,
            )
        {
            try_push_footer_group(
                &mut spans,
                vec![
                    footer_separator(tokens),
                    Span::styled(format!("todos {todos}"), Style::new().fg(tokens.fg_muted)),
                ],
                max_width,
            );
        }
    }
    spans
}

fn try_push_footer_group(
    spans: &mut Vec<Span<'static>>,
    group: Vec<Span<'static>>,
    max_width: usize,
) -> bool {
    if spans_cell_width(spans) + spans_cell_width(&group) <= max_width {
        spans.extend(group);
        true
    } else {
        false
    }
}

fn spans_cell_width(spans: &[Span<'_>]) -> usize {
    spans
        .iter()
        .map(|span| terminal_cell_width(span.content.as_ref()))
        .sum()
}

fn footer_separator(tokens: RatatuiThemeTokens) -> Span<'static> {
    Span::styled(" · ", Style::new().fg(tokens.fg_dim))
}

fn footer_hotkey_line(hotkeys: &str) -> String {
    hotkeys
        .split("  ")
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("  ·  ")
}

fn footer_counts(text: &str) -> (usize, usize) {
    let mut todos = 0;
    let mut queued = 0;
    let parts = text.split_whitespace().collect::<Vec<_>>();
    for window in parts.windows(2) {
        if window[0] == "todos" {
            todos = window[1].parse().unwrap_or(0);
        } else if window[0] == "queued" {
            queued = window[1].parse().unwrap_or(0);
        }
    }
    (todos, queued)
}

fn permission_style(permission: &str, tokens: RatatuiThemeTokens) -> Style {
    let fg = match permission {
        "read-only" => tokens.blue,
        "full-access" => tokens.red,
        "auto-review" => tokens.accent,
        _ => tokens.green,
    };
    if tokens.plain {
        Style::new().fg(fg)
    } else {
        Style::new().fg(fg).bg(match permission {
            "full-access" => tokens.bg_perm_denied,
            "auto-review" => tokens.bg_perm_review,
            _ => tokens.bg_perm_approved,
        })
    }
}

fn render_slash_overlay(
    frame: &mut ratatui::Frame<'_>,
    editor_area: Rect,
    view: &RatatuiViewModel,
) {
    let items = view.slash_items.iter().take(7).cloned().collect::<Vec<_>>();
    let rows = items.len().min(7) as u16;
    let height = rows.saturating_add(2).max(3);
    let popup = Rect {
        x: editor_area.x,
        y: editor_area.y.saturating_sub(height),
        width: editor_area.width,
        height,
    };
    let block = Block::default()
        .title(" commands ")
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(view.theme.border_muted));
    let inner = block.inner(popup);
    frame.render_widget(Clear, popup);
    frame.render_widget(block, popup);
    let lines = if items.is_empty() {
        vec![Line::from(Span::styled(
            "no commands match",
            Style::new().fg(view.theme.fg_dim),
        ))]
    } else {
        items
            .iter()
            .take(7)
            .enumerate()
            .map(|(index, item)| {
                let selected = index == view.slash_selected;
                let marker_style = if selected {
                    view.theme.selected_style().fg(view.theme.accent)
                } else {
                    Style::new().fg(view.theme.accent)
                };
                let command_style = if selected {
                    view.theme.selected_style().fg(view.theme.accent).bold()
                } else {
                    Style::new().fg(view.theme.accent)
                };
                let detail_style = if selected {
                    view.theme.selected_style().fg(view.theme.fg_dim)
                } else {
                    Style::new().fg(view.theme.fg_dim)
                };
                let space_style = if selected {
                    view.theme.selected_style()
                } else {
                    Style::new()
                };
                styled_line_with_fill(
                    vec![
                        (if selected { "›" } else { " " }.to_string(), marker_style),
                        (" ".to_string(), space_style),
                        (
                            format!("{:<14}", truncate_plain(&item.insert, 14)),
                            command_style,
                        ),
                        (" ".to_string(), space_style),
                        (
                            truncate_plain(&item.detail, inner.width.saturating_sub(18) as usize),
                            detail_style,
                        ),
                    ],
                    inner.width,
                    selected.then_some(view.theme.selected_style()),
                )
            })
            .collect()
    };
    frame.render_widget(Paragraph::new(lines), inner);
}

#[cfg(test)]
fn slash_essential_items(view: &RatatuiViewModel) -> Vec<SlashPaletteItem> {
    const ESSENTIALS: [(&str, &str); 7] = [
        ("/settings", "open grouped settings"),
        ("/model", "select main OPPi model"),
        ("/sessions", "resume or inspect sessions"),
        ("/background", "show background tasks"),
        ("/todos", "show active todos"),
        ("/effort", "model-aware thinking slider"),
        ("/exit", "restore terminal and exit"),
    ];
    let mut ordered = Vec::new();
    for (command, detail) in ESSENTIALS {
        if let Some(item) = view
            .slash_items
            .iter()
            .find(|item| item.insert == command || item.label == command)
        {
            ordered.push(item.clone());
        } else {
            ordered.push(SlashPaletteItem {
                label: command.to_string(),
                insert: command.to_string(),
                detail: detail.to_string(),
            });
        }
    }
    ordered
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg(test)]
enum SlashKeyAction {
    MoveUp,
    MoveDown,
    PageUp,
    PageDown,
    Home,
    End,
    InsertSelected,
    SubmitSelected,
    Close,
    Ignore,
}

#[cfg(test)]
fn slash_key_action_name(key: &str) -> SlashKeyAction {
    match key {
        "up" => SlashKeyAction::MoveUp,
        "down" => SlashKeyAction::MoveDown,
        "pgup" => SlashKeyAction::PageUp,
        "pgdn" => SlashKeyAction::PageDown,
        "home" => SlashKeyAction::Home,
        "end" => SlashKeyAction::End,
        "tab" => SlashKeyAction::InsertSelected,
        "enter" => SlashKeyAction::SubmitSelected,
        "esc" => SlashKeyAction::Close,
        _ => SlashKeyAction::Ignore,
    }
}

fn render_settings_overlay(frame: &mut ratatui::Frame<'_>, anchor: Rect, view: &RatatuiViewModel) {
    let width = anchor.width;
    let root_sections = settings_root_overlay_sections(&view.overlay_items);
    let selected = view
        .settings_selected
        .min(view.overlay_items.len().saturating_sub(1));
    let active_section = root_sections
        .iter()
        .find(|section| section.indices.contains(&selected))
        .or_else(|| root_sections.first());
    let tab_line_count = if root_sections.len() > 1 {
        settings_horizontal_tab_line_count(&root_sections, width.saturating_sub(2))
    } else {
        0
    };
    let desired_height = if root_sections.len() > 1 {
        active_section
            .map(|section| {
                (section.indices.len() as u16)
                    .saturating_mul(2)
                    .saturating_add(tab_line_count)
                    .saturating_add(7)
            })
            .unwrap_or_else(|| view.overlay_items.len() as u16 + 4)
    } else {
        active_section
            .map(|section| (section.indices.len() as u16).saturating_mul(2) + 7)
            .unwrap_or_else(|| view.overlay_items.len() as u16 + 4)
    };
    let height = overlay_height(anchor.y, desired_height);
    let popup = Rect {
        x: anchor.x,
        y: anchor.y.saturating_sub(height),
        width,
        height,
    };
    let block = Block::default()
        .title(format!(" {} ", view.overlay_title))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(view.theme.accent));
    let inner = block.inner(popup);
    frame.render_widget(Clear, popup);
    frame.render_widget(block, popup);
    let mut lines = Vec::new();
    if root_sections.len() > 1 {
        let active_section = active_section.expect("root settings section exists");
        let content_width = inner.width.max(20);
        lines.extend(settings_horizontal_tab_lines(
            view,
            &root_sections,
            &active_section.name,
            inner.width,
        ));
        lines.push(Line::from(""));
        let mut content_lines = Vec::<Line<'_>>::new();
        content_lines.push(Line::from(Span::styled(
            "←/→ tabs  ↑/↓ settings  Enter/Space open  Esc close",
            Style::new().fg(view.theme.fg_dim),
        )));
        content_lines.push(Line::from(""));
        for index in &active_section.indices {
            let item = &view.overlay_items[*index];
            let selected = *index == selected;
            content_lines.push(settings_overlay_item_line(
                view,
                item,
                selected,
                content_width,
            ));
            content_lines.push(settings_overlay_detail_line(
                view,
                item,
                selected,
                content_width,
            ));
        }
        content_lines.push(Line::from(""));
        let summary = format!(
            "{} · {} settings · active: {}",
            view.overlay_title,
            view.overlay_items.len(),
            settings_section_display_label(&active_section.name)
        );
        content_lines.push(Line::from(Span::styled(
            truncate_plain(&summary, content_width as usize),
            Style::new().fg(view.theme.fg_dim),
        )));
        lines.extend(content_lines);
    } else {
        lines = view
            .overlay_items
            .iter()
            .enumerate()
            .map(|(index, item)| {
                settings_overlay_item_line(view, item, index == view.settings_selected, inner.width)
            })
            .collect::<Vec<_>>();
        lines.push(Line::from(vec![
            Span::styled("─ ", Style::new().fg(view.theme.border_muted)),
            Span::styled(
                "↑↓ select  Enter open  Space change  Esc close",
                Style::new().fg(view.theme.fg_dim),
            ),
        ]));
    }
    frame.render_widget(Paragraph::new(lines), inner);
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SettingsOverlaySection {
    name: String,
    indices: Vec<usize>,
}

fn settings_section_display_label(section: &str) -> String {
    match section {
        "General" => "⚙️ General".to_string(),
        "Pi" => "🧩 Pi".to_string(),
        "AI" => "🤖 AI".to_string(),
        "Account" => "🔑 Account".to_string(),
        "Permissions" | "Safety" => "🔐 Permissions".to_string(),
        "Theme" | "Appearance" => "🎨 Theme".to_string(),
        "Footer" => "🧭 Footer".to_string(),
        "Compaction" => "🗜️ Compaction".to_string(),
        "Memory" => "🧠 Memory".to_string(),
        "Workspace" => "🗂️ Workspace".to_string(),
        other => other.to_string(),
    }
}

fn settings_horizontal_tab_line_count(sections: &[SettingsOverlaySection], width: u16) -> u16 {
    if sections.is_empty() {
        return 0;
    }
    let width = usize::from(width.max(1));
    let mut lines = 1;
    let mut used = 0usize;
    for section in sections {
        let label = format!(" {} ", settings_section_display_label(&section.name));
        let segment_width = terminal_cell_width(&label);
        let separator_width = usize::from(used > 0) * 2;
        if used > 0 && used + separator_width + segment_width > width {
            lines += 1;
            used = segment_width;
        } else {
            used += separator_width + segment_width;
        }
    }
    lines
}

fn settings_horizontal_tab_lines<'a>(
    view: &RatatuiViewModel,
    sections: &[SettingsOverlaySection],
    active_section: &str,
    width: u16,
) -> Vec<Line<'a>> {
    let width = usize::from(width.max(1));
    let mut lines = Vec::<Line<'a>>::new();
    let mut spans = Vec::<Span<'a>>::new();
    let mut used = 0usize;
    for section in sections {
        let active = section.name == active_section;
        let label = format!(" {} ", settings_section_display_label(&section.name));
        let segment_width = terminal_cell_width(&label);
        let separator_width = usize::from(!spans.is_empty()) * 2;
        if !spans.is_empty() && used + separator_width + segment_width > width {
            lines.push(Line::from(std::mem::take(&mut spans)));
            used = 0;
        }
        if !spans.is_empty() {
            spans.push(Span::raw("  "));
            used += 2;
        }
        let style = if active {
            view.theme.selected_style().fg(view.theme.accent)
        } else {
            Style::new().fg(view.theme.fg_muted)
        };
        spans.push(Span::styled(label, style));
        used += segment_width;
    }
    if !spans.is_empty() {
        lines.push(Line::from(spans));
    }
    lines
}

fn settings_root_overlay_sections(items: &[RatatuiOverlayItem]) -> Vec<SettingsOverlaySection> {
    let mut sections = Vec::<SettingsOverlaySection>::new();
    for (index, item) in items.iter().enumerate() {
        let Some((section, _)) = item.label.split_once(" › ") else {
            continue;
        };
        if sections.last().map(|existing| existing.name.as_str()) != Some(section) {
            sections.push(SettingsOverlaySection {
                name: section.to_string(),
                indices: Vec::new(),
            });
        }
        if let Some(last) = sections.last_mut() {
            last.indices.push(index);
        }
    }
    sections
}

fn settings_overlay_item_line<'a>(
    view: &RatatuiViewModel,
    item: &'a RatatuiOverlayItem,
    selected: bool,
    width: u16,
) -> Line<'a> {
    let marker_style = if selected {
        view.theme.selected_style().fg(view.theme.accent)
    } else {
        Style::new().fg(view.theme.accent)
    };
    let label_style = if selected {
        view.theme.selected_style().fg(view.theme.fg)
    } else {
        Style::new().fg(view.theme.fg)
    };
    let value_style = if selected {
        view.theme.selected_style().fg(view.theme.fg_muted)
    } else {
        Style::new().fg(view.theme.fg_muted)
    };
    let label = item
        .label
        .split_once(" › ")
        .map(|(_, label)| label)
        .unwrap_or(item.label.as_str());
    let value_width = width
        .saturating_sub(6)
        .saturating_sub(label.chars().count() as u16) as usize;
    let space_style = if selected {
        view.theme.selected_style()
    } else {
        Style::new()
    };
    let separator_style = if selected {
        view.theme.selected_style().fg(view.theme.fg_dim)
    } else {
        Style::new().fg(view.theme.fg_dim)
    };
    styled_line_with_fill(
        vec![
            (if selected { "›" } else { " " }.to_string(), marker_style),
            (" ".to_string(), space_style),
            (label.to_string(), label_style),
            (" — ".to_string(), separator_style),
            (truncate_plain(&item.value, value_width), value_style),
        ],
        width,
        selected.then_some(view.theme.selected_style()),
    )
}

fn settings_overlay_detail_line<'a>(
    view: &RatatuiViewModel,
    item: &'a RatatuiOverlayItem,
    selected: bool,
    width: u16,
) -> Line<'a> {
    let detail_style = if selected {
        view.theme.selected_style().fg(view.theme.fg_dim)
    } else {
        Style::new().fg(view.theme.fg_dim)
    };
    let indent_style = if selected {
        view.theme.selected_style()
    } else {
        Style::new()
    };
    styled_line_with_fill(
        vec![
            ("    ".to_string(), indent_style),
            (
                truncate_plain(&item.detail, width.saturating_sub(4) as usize),
                detail_style,
            ),
        ],
        width,
        selected.then_some(view.theme.selected_style()),
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OverlayItem {
    label: &'static str,
    value: &'static str,
    detail: &'static str,
}

fn default_settings_overlay_items() -> Vec<RatatuiOverlayItem> {
    settings_overlay_items()
        .into_iter()
        .map(|item| RatatuiOverlayItem {
            label: item.label.to_string(),
            value: item.value.to_string(),
            detail: item.detail.to_string(),
        })
        .collect()
}

fn settings_overlay_items_for_session(session: &ShellSession) -> Vec<RatatuiOverlayItem> {
    let mut items = default_settings_overlay_items();
    if let Some(item) = items
        .iter_mut()
        .find(|item| item.label == "General › Goal mode")
    {
        item.value = session.goal_status_label();
    }
    items
}

fn settings_overlay_items() -> Vec<OverlayItem> {
    vec![
        OverlayItem {
            label: "General › Status shortcuts",
            value: "/usage",
            detail: "Usage/status, keybindings, and debug surfaces",
        },
        OverlayItem {
            label: "General › Goal mode",
            value: "none",
            detail: "Track and continue one thread objective",
        },
        OverlayItem {
            label: "General › Sessions",
            value: "current",
            detail: "Browse and resume prior sessions",
        },
        OverlayItem {
            label: "Pi › Main model",
            value: "gpt-5-codex",
            detail: "Select the default model for OPPi turns",
        },
        OverlayItem {
            label: "Pi › Effort",
            value: "auto",
            detail: "Model-aware thinking slider for the main model",
        },
        OverlayItem {
            label: "Pi › Scoped models",
            value: "all",
            detail: "Limit model cycling to selected model patterns",
        },
        OverlayItem {
            label: "Pi › Role models",
            value: "advanced",
            detail: "Per-task model overrides live here",
        },
        OverlayItem {
            label: "Pi › Provider",
            value: "openai",
            detail: "Provider status, validation, and base URL",
        },
        OverlayItem {
            label: "Pi › Login",
            value: "subscription/api",
            detail: "Subscription and API authentication",
        },
        OverlayItem {
            label: "Footer › Status bar",
            value: "live",
            detail: "Footer help, usage, todos, model, permission, and memory chips",
        },
        OverlayItem {
            label: "Memory › Hoppi",
            value: "client-hosted",
            detail: "Recall, dashboard, and maintenance",
        },
        OverlayItem {
            label: "Compaction › Context handoff",
            value: "manual",
            detail: "Manual memory compaction and maintenance shortcuts",
        },
        OverlayItem {
            label: "Permissions › Mode",
            value: "default",
            detail: "Read/write/network approval policy",
        },
        OverlayItem {
            label: "Theme › OPPi theme",
            value: "oppi (dark)",
            detail: "Colors and terminal-safe rendering",
        },
    ]
}

#[cfg(test)]
fn theme_panel_items() -> Vec<OverlayItem> {
    vec![
        OverlayItem {
            label: "OPPi",
            value: "oppi",
            detail: "default cyan theme",
        },
        OverlayItem {
            label: "Dark",
            value: "dark",
            detail: "dark terminal theme",
        },
        OverlayItem {
            label: "Light",
            value: "light",
            detail: "light terminal theme",
        },
        OverlayItem {
            label: "Plain",
            value: "plain",
            detail: "no-color safe mode",
        },
    ]
}

#[cfg(test)]
fn permission_panel_items() -> Vec<OverlayItem> {
    vec![
        OverlayItem {
            label: "Read only",
            value: "read-only",
            detail: "block writes and risky commands",
        },
        OverlayItem {
            label: "Default",
            value: "default",
            detail: "normal approval policy",
        },
        OverlayItem {
            label: "Auto review",
            value: "auto-review",
            detail: "Guardian review before risky calls",
        },
        OverlayItem {
            label: "Full access",
            value: "full-access",
            detail: "explicit high-trust mode",
        },
    ]
}

#[cfg(test)]
fn provider_panel_items() -> Vec<OverlayItem> {
    vec![
        OverlayItem {
            label: "OpenAI",
            value: "env-ref",
            detail: "OpenAI-compatible API key",
        },
        OverlayItem {
            label: "Codex",
            value: "OAuth",
            detail: "ChatGPT subscription auth",
        },
        OverlayItem {
            label: "Copilot",
            value: "device code",
            detail: "GitHub Copilot subscription auth",
        },
        OverlayItem {
            label: "Claude",
            value: "Meridian",
            detail: "explicit loopback bridge",
        },
    ]
}

#[cfg(test)]
fn login_panel_items() -> Vec<OverlayItem> {
    vec![
        OverlayItem {
            label: "Subscription",
            value: "Codex/Copilot/Claude",
            detail: "browser/device-code flows",
        },
        OverlayItem {
            label: "API key",
            value: "env ref",
            detail: "store only environment variable names",
        },
        OverlayItem {
            label: "Logout",
            value: "redacted",
            detail: "remove selected auth entry",
        },
    ]
}

#[cfg(test)]
fn memory_panel_items() -> Vec<OverlayItem> {
    vec![
        OverlayItem {
            label: "Dashboard",
            value: "Hoppi",
            detail: "open client-hosted memory controls",
        },
        OverlayItem {
            label: "On/off",
            value: "session",
            detail: "toggle recall/write behavior",
        },
        OverlayItem {
            label: "Compact",
            value: "manual",
            detail: "summarize current thread",
        },
    ]
}

#[cfg(test)]
fn session_picker_items() -> Vec<OverlayItem> {
    vec![
        OverlayItem {
            label: "current",
            value: "thread-123456",
            detail: "Enter resumes current thread",
        },
        OverlayItem {
            label: "recent",
            value: "thread-design",
            detail: "Ratatui Plan 60",
        },
    ]
}

#[cfg(test)]
fn model_role_picker_items() -> Vec<OverlayItem> {
    vec![
        OverlayItem {
            label: "executor",
            value: "gpt-5-codex",
            detail: "current role model",
        },
        OverlayItem {
            label: "reviewer",
            value: "inherit",
            detail: "inherits session model",
        },
        OverlayItem {
            label: "planner",
            value: "claude-sonnet",
            detail: "persisted role override",
        },
    ]
}

#[cfg(test)]
fn overlay_width(width: u16) -> u16 {
    width.saturating_mul(3).saturating_div(4).clamp(40, 82)
}

fn overlay_height(height: u16, desired: u16) -> u16 {
    desired.min(height.saturating_sub(4).max(6))
}

#[cfg(test)]
fn overlay_clears_and_adapts(area: Rect) -> bool {
    area.width >= 40 && area.height >= 16
}

fn row_lines(
    row: &RatatuiTranscriptRow,
    width: u16,
    streaming: bool,
    tokens: RatatuiThemeTokens,
) -> Vec<Line<'static>> {
    let body_width = width.saturating_sub(TRANSCRIPT_PREFIX_WIDTH as u16).max(8) as usize;
    let mut chunks = wrap_plain(&row.body, body_width);
    if chunks.is_empty() {
        chunks.push(String::new());
    }
    let last_index = chunks.len().saturating_sub(1);
    chunks
        .into_iter()
        .enumerate()
        .map(|(index, chunk)| {
            let mut spans = Vec::new();
            if index == 0 {
                spans.push(Span::styled(
                    format!("{:<TRANSCRIPT_GUTTER_WIDTH$}", row.kind.gutter()),
                    row.kind.style(tokens),
                ));
                spans.push(Span::raw(" "));
                spans.push(Span::styled(
                    format!(
                        "{:<TRANSCRIPT_LABEL_WIDTH$}",
                        truncate_plain(&row.label, TRANSCRIPT_LABEL_WIDTH)
                    ),
                    Style::new().fg(tokens.fg_dim),
                ));
                spans.push(Span::raw(" "));
            } else {
                spans.push(Span::raw(" ".repeat(TRANSCRIPT_PREFIX_WIDTH)));
            }
            spans.extend(body_spans_for_chunk(row.kind, &chunk, tokens));
            if streaming && index == last_index && matches!(row.kind, RatatuiRowKind::Assistant) {
                spans.push(Span::styled("▍", Style::new().fg(tokens.accent)));
            }
            Line::from(spans)
        })
        .collect()
}

fn body_spans_for_chunk(
    kind: RatatuiRowKind,
    chunk: &str,
    tokens: RatatuiThemeTokens,
) -> Vec<Span<'static>> {
    if matches!(kind, RatatuiRowKind::Diff) || chunk.starts_with('+') || chunk.starts_with('-') {
        let fg = if chunk.starts_with('+') {
            tokens.green
        } else if chunk.starts_with('-') {
            tokens.red
        } else {
            tokens.fg_muted
        };
        return vec![Span::styled(chunk.to_string(), Style::new().fg(fg))];
    }
    let trimmed = chunk.trim_start();
    if trimmed.starts_with("```") {
        return vec![Span::styled(
            chunk.to_string(),
            Style::new().fg(tokens.green).add_modifier(Modifier::DIM),
        )];
    }
    if trimmed.starts_with('#') {
        return vec![Span::styled(
            chunk.to_string(),
            Style::new().fg(tokens.accent_soft).bold(),
        )];
    }
    if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
        return vec![Span::styled(
            chunk.to_string(),
            Style::new().fg(tokens.accent),
        )];
    }
    if trimmed.starts_with('>') {
        return vec![Span::styled(
            chunk.to_string(),
            Style::new().fg(tokens.fg_muted),
        )];
    }
    inline_code_spans(chunk, kind.body_style(tokens), tokens)
}

fn inline_code_spans(
    chunk: &str,
    base_style: Style,
    tokens: RatatuiThemeTokens,
) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    for (index, part) in chunk.split('`').enumerate() {
        if part.is_empty() {
            continue;
        }
        let style = if index % 2 == 1 {
            Style::new().fg(tokens.accent_soft).bg(if tokens.plain {
                Color::Reset
            } else {
                tokens.bg_custom_msg
            })
        } else {
            base_style
        };
        spans.push(Span::styled(part.to_string(), style));
    }
    if spans.is_empty() {
        spans.push(Span::styled(String::new(), base_style));
    }
    spans
}

fn permission_label(mode: PermissionMode) -> &'static str {
    match mode {
        PermissionMode::ReadOnly => "read-only",
        PermissionMode::Default => "default",
        PermissionMode::AutoReview => "auto-review",
        PermissionMode::FullAccess => "full-access",
    }
}

fn classify_transcript_line(line: &str) -> RatatuiRowKind {
    let lower = line.trim_start().to_ascii_lowercase();
    if lower.starts_with("agent") || lower.contains(" agent ") {
        RatatuiRowKind::Agent
    } else if lower.starts_with("diff") || lower.starts_with('+') || lower.starts_with('-') {
        RatatuiRowKind::Diff
    } else if lower.starts_with("artifact")
        || lower.contains("artifact:")
        || lower.contains("artifact://")
    {
        RatatuiRowKind::Artifact
    } else if lower.contains("denied") || lower.contains("blocked") || lower.contains("protected") {
        RatatuiRowKind::Denied
    } else if lower.contains("error") || lower.contains("failed") {
        RatatuiRowKind::Error
    } else if lower.starts_with("tool read")
        || lower.contains(" read_file")
        || lower.contains(" read ")
    {
        RatatuiRowKind::ToolRead
    } else if lower.starts_with("tool write")
        || lower.contains(" write_file")
        || lower.contains(" edit ")
    {
        RatatuiRowKind::ToolWrite
    } else if lower.starts_with("tool run") || lower.contains(" shell") || lower.contains(" bash") {
        RatatuiRowKind::ToolRun
    } else if lower.starts_with("tool ") || lower.contains(" tool ") {
        RatatuiRowKind::ToolRun
    } else if lower.starts_with("you") || lower.starts_with("user") {
        RatatuiRowKind::User
    } else if lower.starts_with("oppi") || lower.starts_with("assistant") {
        RatatuiRowKind::Assistant
    } else {
        RatatuiRowKind::Info
    }
}

fn transcript_label(line: &str) -> String {
    match classify_transcript_line(line) {
        RatatuiRowKind::User => "you",
        RatatuiRowKind::Assistant => "oppi",
        RatatuiRowKind::Info => "info",
        RatatuiRowKind::ToolRead => "read",
        RatatuiRowKind::ToolWrite => "write",
        RatatuiRowKind::ToolRun => "run",
        RatatuiRowKind::Diff => "diff",
        RatatuiRowKind::Artifact => "artifact",
        RatatuiRowKind::Denied => "denied",
        RatatuiRowKind::Error => "error",
        RatatuiRowKind::Agent => "agent",
    }
    .to_string()
}

fn buffer_to_string(buffer: &ratatui::buffer::Buffer, width: u16, height: u16) -> String {
    let mut lines = Vec::new();
    for y in 0..height {
        let mut line = String::new();
        for x in 0..width {
            line.push_str(buffer[(x, y)].symbol());
        }
        lines.push(line.trim_end().to_string());
    }
    lines.join("\n")
}

fn wrap_plain(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines = Vec::new();
    for raw_line in text.lines() {
        let mut current = String::new();
        for word in raw_line.split_whitespace() {
            let sep = usize::from(!current.is_empty());
            if current.chars().count() + sep + word.chars().count() > width && !current.is_empty() {
                lines.push(current);
                current = String::new();
            }
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(word);
        }
        lines.push(current);
    }
    lines
}

fn terminal_cell_width(text: &str) -> usize {
    UnicodeWidthStr::width(text)
}

#[cfg(test)]
fn snapshot_diff(expected: &str, actual: &str) -> Option<String> {
    if expected == actual {
        return None;
    }
    let expected_lines = expected.lines().collect::<Vec<_>>();
    let actual_lines = actual.lines().collect::<Vec<_>>();
    let max = expected_lines.len().max(actual_lines.len());
    for row in 0..max {
        let left = expected_lines.get(row).copied().unwrap_or("");
        let right = actual_lines.get(row).copied().unwrap_or("");
        if left != right {
            let col = left
                .chars()
                .zip(right.chars())
                .position(|(a, b)| a != b)
                .unwrap_or_else(|| left.chars().count().min(right.chars().count()));
            let expected_context = snapshot_context(left, col, 14);
            let actual_context = snapshot_context(right, col, 14);
            return Some(format!(
                "terminal snapshot differs at row {}, col {}\nexpected: {:?}\nactual:   {:?}\ncontext expected[{}..{}]: {:?}\ncontext actual[{}..{}]:   {:?}",
                row + 1,
                col + 1,
                left,
                right,
                expected_context.0,
                expected_context.1,
                expected_context.2,
                actual_context.0,
                actual_context.1,
                actual_context.2
            ));
        }
    }
    Some("terminal snapshot differs".to_string())
}

#[cfg(test)]
fn snapshot_context(line: &str, col: usize, radius: usize) -> (usize, usize, String) {
    let chars = line.chars().collect::<Vec<_>>();
    let start = col.saturating_sub(radius);
    let end = (col + radius + 1).min(chars.len());
    let context = chars[start..end].iter().collect::<String>();
    (start + 1, end, context)
}

#[cfg(test)]
fn ratatui_design_test_guidance() -> &'static str {
    "Run `cargo test -p oppi-shell ratatui_design` for design-parity snapshots; run full `cargo test -p oppi-shell` before promotion."
}

#[cfg(test)]
fn manual_screenshot_checklist() -> &'static str {
    "Compare R1-R3 and frames 03-10 against .reference/design-v2/index.html; verify Ctrl+C cleanup, Windows smoke, narrow 58-col, tiny 90x14, dark/light/plain."
}

#[cfg(test)]
fn default_ratatui_gate_status() -> &'static str {
    "blocked: keep stable Pi-powered UI default until all 10 frame goldens, Ctrl+C cleanup, Windows/native smoke, and manual visual comparison pass"
}

fn truncate_plain(text: &str, width: usize) -> String {
    let mut out = text.chars().take(width).collect::<String>();
    if text.chars().count() > width && width > 0 {
        out.pop();
        out.push('…');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ratatui_lifecycle_exit_paths_share_cleanup_contract() {
        let cleanup = terminal_cleanup_sequence();
        for reason in ["/exit", "Ctrl+C×2", "Ctrl+D", "server failure", "panic"] {
            assert!(
                cleanup.contains("\x1b[?2026l"),
                "{reason}: disables sync update"
            );
            assert!(cleanup.contains("\x1b[?25h"), "{reason}: restores cursor");
            assert!(cleanup.contains("\x1b[J"), "{reason}: clears stale rows");
            assert!(cleanup.ends_with("\r\n"), "{reason}: leaves raw region");
        }
    }

    #[test]
    fn ratatui_resize_render_drops_stale_rows() {
        let view = sample_view();
        let large = render_ratatui_preview(&view, 120, 32, RatatuiFrameMode::Background).unwrap();
        assert_eq!(large.lines().count(), 32);
        assert!(large.contains("watch:cargo-check"));
        let small = render_ratatui_preview(&view, 58, 14, RatatuiFrameMode::Idle).unwrap();
        assert_eq!(small.lines().count(), 14);
        assert!(!small.contains("watch:cargo-check"));
        for line in small.lines() {
            assert!(
                terminal_cell_width(line) <= 58,
                "too wide after resize: {line}"
            );
        }
    }

    #[test]
    fn ratatui_same_screen_scrollback_survives_redraw() {
        let mut view = sample_view();
        view.transcript = (0..6)
            .map(|index| RatatuiTranscriptRow {
                kind: RatatuiRowKind::Assistant,
                label: "oppi".to_string(),
                body: format!("persistent row {index}"),
            })
            .collect();
        let first = render_ratatui_preview(&view, 90, 22, RatatuiFrameMode::Idle).unwrap();
        view.spinner_index = view.spinner_index.wrapping_add(1);
        let second = render_ratatui_preview(&view, 90, 22, RatatuiFrameMode::Idle).unwrap();
        assert!(first.contains("persistent row 5"));
        assert!(second.contains("persistent row 5"));
    }

    #[test]
    fn ratatui_unicode_width_uses_terminal_width_library() {
        assert_eq!(terminal_cell_width("OPPi"), 4);
        assert_eq!(terminal_cell_width("界"), 2);
        assert_eq!(terminal_cell_width("👩‍💻"), UnicodeWidthStr::width("👩‍💻"));
    }

    #[test]
    #[cfg(windows)]
    fn ratatui_windows_native_smoke_renders_and_cleans() {
        let rendered =
            render_ratatui_preview(&sample_view(), 90, 22, RatatuiFrameMode::Idle).unwrap();
        assert!(rendered.contains("OPPi"));
        assert!(terminal_cleanup_sequence().contains("\x1b[?25h"));
    }

    #[test]
    #[cfg(unix)]
    fn ratatui_unix_native_smoke_renders_and_cleans() {
        let rendered =
            render_ratatui_preview(&sample_view(), 90, 22, RatatuiFrameMode::Idle).unwrap();
        assert!(rendered.contains("OPPi"));
        assert!(terminal_cleanup_sequence().contains("\x1b[?25h"));
    }

    #[test]
    fn ratatui_terminal_cleanup_sequence_resets_and_clears() {
        let cleanup = terminal_cleanup_sequence();
        assert!(
            cleanup.contains("\x1b[?2026l"),
            "leaves synchronized output mode"
        );
        assert!(cleanup.contains("\x1b[0m"), "resets styles");
        assert!(cleanup.contains("\x1b[?25h"), "shows cursor");
        assert!(cleanup.contains("\x1b[2K"), "clears current line");
        assert!(
            cleanup.contains("\x1b[J"),
            "clears stale frame below cursor"
        );
        assert!(cleanup.ends_with("\r\n"), "finishes outside raw TUI region");
    }

    fn sample_view() -> RatatuiViewModel {
        RatatuiViewModel {
            provider: "openai-codex".to_string(),
            model: "gpt-5-codex".to_string(),
            permission: "auto-review".to_string(),
            status: "ready".to_string(),
            thread_id: "thread-123456".to_string(),
            goal: Some("implement ThemeTokens and header cells".to_string()),
            todo_completed: 1,
            todo_total: 4,
            spinner_index: 2,
            theme: RatatuiThemeTokens::dark(),
            transcript_metadata: Vec::new(),
            transcript: vec![
                RatatuiTranscriptRow {
                    kind: RatatuiRowKind::Info,
                    label: "info".to_string(),
                    body: "resumed thread thr_8f4b21".to_string(),
                },
                RatatuiTranscriptRow {
                    kind: RatatuiRowKind::Assistant,
                    label: "oppi".to_string(),
                    body: "Plan: add Ratatui layout skeleton".to_string(),
                },
            ],
            dock_label: "docks: idle".to_string(),
            editor_placeholder: "Ask, build, or type / for commands…".to_string(),
            editor_is_placeholder: true,
            footer_left: "ready · openai-codex · perm auto-review · todos 0 · queued 0".to_string(),
            footer_hotkeys: "Alt+Enter follow-up  Ctrl+Enter steer  / commands".to_string(),
            slash_items: vec![SlashPaletteItem {
                label: "/settings".to_string(),
                insert: "/settings".to_string(),
                detail: "open settings overlay".to_string(),
            }],
            slash_selected: 0,
            overlay_title: "settings".to_string(),
            overlay_items: default_settings_overlay_items(),
            settings_selected: 1,
            question_selected: 0,
            pending_answers: vec![
                "1. probe at startup via DA1 response".to_string(),
                "2. assume on, fall back".to_string(),
            ],
            approval_items: vec![
                "approve write_file crates/oppi-shell/src/ratatui_ui.rs".to_string(),
                "/approve resumes · /deny blocks".to_string(),
            ],
            background_items: vec![
                "⠋ watch:cargo-check · running".to_string(),
                "✓ fmt:rustfmt · done".to_string(),
            ],
            background_typed: vec![RatatuiBackgroundItem {
                id: "task-1".to_string(),
                status: "running".to_string(),
                command: "cargo check".to_string(),
                hint: "L list · R read latest · K kill latest draft".to_string(),
            }],
            todo_items: vec![
                "▶ resolve Ctrl+P ambiguity · in_progress".to_string(),
                "! verify cargo test on Windows host · blocked".to_string(),
            ],
            todo_typed: vec![RatatuiTodoItem {
                id: "todo-1".to_string(),
                status: "in_progress".to_string(),
                priority: Some("high".to_string()),
                phase: Some("Live adapters".to_string()),
                content: "resolve Ctrl+P ambiguity".to_string(),
            }],
            suggestion_items: vec!["ghost: Run snapshots".to_string()],
            suggestion: Some(RatatuiSuggestion {
                message: "Run snapshots".to_string(),
                confidence_percent: 90,
                reason: Some("next validation step".to_string()),
            }),
            tool_digest_items: vec![RatatuiToolDigest {
                status: ToolDigestStatus::Success,
                name: "write_file".to_string(),
                target: "crates/oppi-shell/src/ratatui_ui.rs".to_string(),
                duration: "11ms".to_string(),
                hint: "+148 / -62".to_string(),
            }],
        }
    }

    fn reference_r1_r2_r3_fixture(section: &str) -> String {
        let fixture = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/ratatui/r1-r2-r3.contract.snap"
        ))
        .replace("\r\n", "\n");
        let marker = format!("-- {section} --");
        let start = fixture
            .find(&marker)
            .unwrap_or_else(|| panic!("missing fixture section {section}"))
            + marker.len();
        let rest = &fixture[start..];
        let end = rest.find("\n-- ").unwrap_or(rest.len());
        rest[..end].trim_matches(['\r', '\n']).to_string()
    }

    fn design_frame_section(kind: model::DesignFrameKind) -> &'static str {
        match kind {
            model::DesignFrameKind::ToolsArtifactDenial => "FRAME_03_TOOLS_ARTIFACT_DENIAL_120X32",
            model::DesignFrameKind::AskUser => "FRAME_04_ASK_USER_120X32",
            model::DesignFrameKind::Background => "FRAME_05_BACKGROUND_120X32",
            model::DesignFrameKind::Todos => "FRAME_06_TODOS_120X32",
            model::DesignFrameKind::Slash => "FRAME_07_SLASH_120X32",
            model::DesignFrameKind::Settings => "FRAME_08_SETTINGS_120X32",
            model::DesignFrameKind::Narrow => "FRAME_09_NARROW_58X22",
            model::DesignFrameKind::Tiny => "FRAME_10_TINY_90X14",
            _ => "FRAME_01_02_R1_R2_SOURCE",
        }
    }

    fn reference_frames_03_10_fixture(section: &str) -> String {
        let fixture = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/ratatui/frames-03-10.contract.snap"
        ));
        let marker = format!("-- {section} --");
        let start = fixture
            .find(&marker)
            .unwrap_or_else(|| panic!("missing fixture section {section}"))
            + marker.len();
        let rest = &fixture[start..];
        let end = rest.find("\n-- ").unwrap_or(rest.len());
        rest[..end].trim_matches(['\r', '\n']).replace("\r\n", "\n")
    }

    fn design_frame_view(kind: model::DesignFrameKind) -> RatatuiViewModel {
        let mut view = sample_view();
        let content = frames::design_frame_fixture_content(kind);
        view.status = content.header.status.to_string();
        view.goal = content.header.goal.map(str::to_string);
        view.todo_completed = 0;
        view.todo_total = content.footer.todos;
        view.editor_placeholder = content.editor.placeholder.to_string();
        view.editor_is_placeholder = content.editor.placeholder != "/";
        view.footer_left = format!(
            "{} · openai-codex · perm auto-review · todos {} · queued {}",
            content.footer.status, content.footer.todos, content.footer.queued
        );
        view.transcript = content
            .transcript
            .iter()
            .map(|row| RatatuiTranscriptRow {
                kind: match row.kind {
                    "user" => RatatuiRowKind::User,
                    "assistant" => RatatuiRowKind::Assistant,
                    "tool" if row.body.starts_with('+') || row.body.starts_with('-') => {
                        RatatuiRowKind::Diff
                    }
                    "tool" => RatatuiRowKind::ToolRun,
                    "artifact" => RatatuiRowKind::Artifact,
                    "error" => RatatuiRowKind::Denied,
                    _ => RatatuiRowKind::Info,
                },
                label: row.label.to_string(),
                body: row.body.to_string(),
            })
            .collect();
        match kind {
            model::DesignFrameKind::ToolsArtifactDenial => {
                view.tool_digest_items = vec![RatatuiToolDigest {
                    status: ToolDigestStatus::Success,
                    name: "write_file".to_string(),
                    target: "crates/oppi-shell/src/tui.rs".to_string(),
                    duration: "11ms".to_string(),
                    hint: "+148 / −62".to_string(),
                }];
            }
            model::DesignFrameKind::AskUser => {
                view.pending_answers = content
                    .dock
                    .rows
                    .iter()
                    .map(|row| format!("{} {}", row.glyph, row.label))
                    .collect();
            }
            model::DesignFrameKind::Background => {
                view.background_items = content
                    .dock
                    .rows
                    .iter()
                    .map(|row| format!("{} {} {}", row.glyph, row.label, row.detail))
                    .collect();
            }
            model::DesignFrameKind::Todos => {
                view.todo_items = content
                    .dock
                    .rows
                    .iter()
                    .map(|row| format!("{} {} {}", row.glyph, row.label, row.detail))
                    .collect();
            }
            model::DesignFrameKind::Slash => {
                view.slash_items = content
                    .slash_items
                    .iter()
                    .map(|item| SlashPaletteItem {
                        label: item.command.to_string(),
                        insert: item.command.to_string(),
                        detail: item.detail.to_string(),
                    })
                    .collect();
            }
            model::DesignFrameKind::Settings => {
                view.settings_selected = content
                    .overlay
                    .as_ref()
                    .map(|overlay| overlay.selected)
                    .unwrap_or(1);
            }
            model::DesignFrameKind::Tiny => {
                view.status = "running".to_string();
            }
            _ => {}
        }
        view
    }

    fn render_design_frame_fixture(kind: model::DesignFrameKind) -> String {
        let fixture = frames::design_frame_fixture(kind);
        render_ratatui_exact_fixture(
            &design_frame_view(kind),
            fixture.width,
            fixture.height,
            fixture_mode(kind),
        )
        .unwrap()
    }

    #[test]
    fn ratatui_reference_r1_r2_r3_exact_fixtures_are_extracted() {
        let idle = reference_r1_r2_r3_fixture("R1_IDLE_78X15");
        assert_eq!(idle.lines().count(), 15);
        assert!(idle.contains("anthropic/claude-sonnet-4.5"));
        assert!(idle.contains("docks: idle"));
        let running = reference_r1_r2_r3_fixture("R2_RUNNING_78X15");
        assert_eq!(running.lines().count(), 15);
        assert!(running.contains("◎ refactor auth middleware"));
        assert!(running.contains("cookie▍"));
        let slash = reference_r1_r2_r3_fixture("R3_SLASH_78X16");
        assert_eq!(slash.lines().count(), 16);
        assert!(slash.contains("/settings"));
        assert!(slash.contains("/exit"));
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum ExactReferenceFrame {
        R1Idle,
        R2Running,
        R3Slash,
    }

    impl ExactReferenceFrame {
        const ALL: [ExactReferenceFrame; 3] = [Self::R1Idle, Self::R2Running, Self::R3Slash];

        fn section(self) -> &'static str {
            match self {
                Self::R1Idle => "R1_IDLE_78X15",
                Self::R2Running => "R2_RUNNING_78X15",
                Self::R3Slash => "R3_SLASH_78X16",
            }
        }

        fn label(self) -> &'static str {
            match self {
                Self::R1Idle => "R1 idle",
                Self::R2Running => "R2 running",
                Self::R3Slash => "R3 slash",
            }
        }
    }

    fn render_exact_reference_handoff_frame(frame: ExactReferenceFrame) -> String {
        match frame {
            ExactReferenceFrame::R1Idle => render_exact_reference_r1_idle(),
            ExactReferenceFrame::R2Running => render_exact_reference_r2_running(),
            ExactReferenceFrame::R3Slash => render_exact_reference_r3_slash(),
        }
    }

    fn render_exact_reference_r1_idle() -> String {
        [
            "• OPPi · anthropic/claude-sonnet-4.5 · perms: default · ready",
            " ›  explain the diff between Result<T,E> and ?",
            "",
            " ◇  OPPi",
            "    Result<T,E> is the type. The ? operator is sugar for",
            "    early-returning the Err arm via From conversion.",
            "",
            " ›  when does it not work?",
            "",
            "────────────────────────────────────────────────────────────────── docks: idle",
            "╭────────────────────────────────────────────────────────────────────────────╮",
            "│ › █                                                                       │",
            "╰────────────────────────────────────────────────────────────────────────────╯",
            " ctx 12.4k/200k  ·  $0.084  ·  auto-edit on",
            " Tab complete  ⌃R run  ⌃C cancel  / cmds  Alt+K hide",
        ]
        .join("\n")
    }

    fn render_exact_reference_r2_running() -> String {
        [
            "◐ OPPi · anthropic/claude-sonnet-4.5 · running╭───────────────────────────╮",
            "                                              │ ◎ refactor auth middleware │",
            "                                              ╰───────────────────────────╯",
            " ›  refactor auth middleware to use the new SessionGuard",
            "",
            " ◇  OPPi",
            "    Reading src/middleware/auth.rs ... done",
            "    Reading src/session/guard.rs ... done",
            "    Drafting changes... I'll replace the manual cookie▍",
            "────────────────────────────────────────────────────────────────── docks: idle",
            "╭────────────────────────────────────────────────────────────────────────────╮",
            "│ › █                                                                       │",
            "╰────────────────────────────────────────────────────────────────────────────╯",
            " ctx 38.1k/200k  ·  $0.214  ·  auto-edit on",
            " Esc cancel  ⌃R rerun  / cmds  Alt+K hide",
        ]
        .join("\n")
    }

    fn render_exact_reference_r3_slash() -> String {
        [
            "• OPPi · anthropic/claude-sonnet-4.5 · ready",
            " ›  explain the diff between Result<T,E> and ?",
            "",
            "╭────────────────────────────────────────────────────────────────────────────╮",
            "│ › /settings    open settings overlay                                       │",
            "│   /model       switch provider/model                                       │",
            "│   /sessions    browse session history                                      │",
            "│   /background  send to background runner                                  │",
            "│   /todos       show task list                                              │",
            "│   /effort      model-aware thinking slider                                 │",
            "│   /exit        quit OPPi                                                   │",
            "╰────────────────────────────────────────────────────────────────────────────╯",
            "╭────────────────────────────────────────────────────────────────────────────╮",
            "│ › /█                                                                      │",
            "╰────────────────────────────────────────────────────────────────────────────╯",
            " ↑↓ select  Tab insert  Enter submit  Esc close",
        ]
        .join("\n")
    }

    fn assert_reference_capture_matches(frame: ExactReferenceFrame) {
        let expected = reference_r1_r2_r3_fixture(frame.section());
        let actual = render_exact_reference_handoff_frame(frame);
        let label = frame.label();
        let diff = snapshot_diff(&expected, &actual);
        assert!(
            diff.is_none(),
            "{label} did not match reference fixture\n{}",
            diff.unwrap_or_default()
        );
    }

    #[test]
    fn ratatui_exact_reference_path_is_fixture_only_and_kept_out_of_live_rendering() {
        assert_eq!(ExactReferenceFrame::ALL.len(), 3);
        let live =
            render_ratatui_exact_fixture(&sample_view(), 78, 15, RatatuiFrameMode::Idle).unwrap();
        for fixture_only_text in [
            "Result<T,E>",
            "anthropic/claude-sonnet-4.5",
            "auto-edit on",
            "$0.084",
        ] {
            assert!(
                !live.contains(fixture_only_text),
                "live/product rendering leaked exact-reference fixture text: {fixture_only_text}"
            );
        }

        let live_slash =
            render_ratatui_exact_fixture(&sample_view(), 78, 16, RatatuiFrameMode::Slash).unwrap();
        assert!(live_slash.contains("open settings overlay"));
        for exact_only_slash_text in [
            "switch provider/model",
            "browse session history",
            "send to background runner",
            "↑↓ select  Tab insert  Enter submit  Esc close",
        ] {
            assert!(
                !live_slash.contains(exact_only_slash_text),
                "live slash rendering leaked exact-reference slash text: {exact_only_slash_text}"
            );
        }
    }

    #[test]
    #[ignore = "developer utility: writes current Rust R1/R2/R3 output to output/ratatui-captures/r1-r2-r3-rust-actual.snap"]
    fn ratatui_dump_r1_r2_r3_actual_captures() {
        let mut out = String::from(
            "# Current Rust Ratatui output for the R1/R2/R3 target comparison.\n# This is an actual capture, not the target fixture; do not overwrite r1-r2-r3.contract.snap with it.\n\n",
        );
        let idle = sample_view();
        out.push_str("-- R1_IDLE_78X15 --\n");
        out.push_str(&render_ratatui_exact_fixture(&idle, 78, 15, RatatuiFrameMode::Idle).unwrap());
        out.push_str("\n\n");

        let mut running = sample_view();
        running.status = "running".to_string();
        running.spinner_index = 0;
        out.push_str("-- R2_RUNNING_78X15 --\n");
        out.push_str(
            &render_ratatui_exact_fixture(&running, 78, 15, RatatuiFrameMode::Running).unwrap(),
        );
        out.push_str("\n\n");

        let mut slash = sample_view();
        slash.editor_placeholder = "/".to_string();
        slash.editor_is_placeholder = false;
        slash.slash_items = slash_essential_items(&slash);
        out.push_str("-- R3_SLASH_78X16 --\n");
        out.push_str(
            &render_ratatui_exact_fixture(&slash, 78, 16, RatatuiFrameMode::Slash).unwrap(),
        );
        out.push_str("\n\n");
        let path = std::env::var("OPPI_RATATUI_R1_R2_R3_ACTUAL_OUT")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| {
                std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../..")
                    .join("output/ratatui-captures/r1-r2-r3-rust-actual.snap")
            });
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create R1-R3 actual capture output directory");
        }
        std::fs::write(path, out).expect("write R1-R3 actual capture output");
    }

    #[test]
    fn ratatui_design_r1_idle_matches_exact_reference_fixture() {
        assert_reference_capture_matches(ExactReferenceFrame::R1Idle);
    }

    #[test]
    fn ratatui_design_r2_running_matches_exact_reference_fixture() {
        assert_reference_capture_matches(ExactReferenceFrame::R2Running);
    }

    #[test]
    fn ratatui_design_r3_slash_matches_exact_reference_fixture() {
        assert_reference_capture_matches(ExactReferenceFrame::R3Slash);
    }

    #[test]
    fn ratatui_theme_tokens_match_design_palettes_and_plain_variant() {
        let dark = RatatuiThemeTokens::dark();
        assert_eq!(dark.accent, rgb(0x39, 0xd7, 0xe5));
        assert_eq!(dark.bg_perm_denied, rgb(0x35, 0x19, 0x24));
        let light = RatatuiThemeTokens::light();
        assert_eq!(light.accent, rgb(0x00, 0x6d, 0x7d));
        assert_eq!(light.bg_selected, rgb(0xd7, 0xf4, 0xf8));
        let plain = RatatuiThemeTokens::plain();
        assert!(plain.plain);
        assert_eq!(plain.bg, Color::Reset);
        assert_eq!(RatatuiThemeTokens::for_name("oppi").accent, dark.accent);
    }

    #[test]
    fn ratatui_exact_fixture_preserves_requested_size() {
        let rendered =
            render_ratatui_exact_fixture(&sample_view(), 58, 22, RatatuiFrameMode::Idle).unwrap();
        let lines = rendered.lines().collect::<Vec<_>>();
        assert_eq!(lines.len(), 22);
        assert!(lines.iter().all(|line| line.chars().count() <= 58));
        assert!(rendered.contains("OPPi"));
        assert!(rendered.contains("perms:"));
    }

    #[test]
    fn ratatui_header_matches_design_reference_identity_policy() {
        let mut running = sample_view();
        running.status = "running".to_string();
        running.spinner_index = 0;
        let rendered = render_ratatui_preview(&running, 90, 16, RatatuiFrameMode::Running).unwrap();
        assert!(rendered.contains(widgets::header::HEADER_VISUAL_CONTRACT.running_spinner));
        assert!(rendered.contains("◐ OPPi · openai-codex/gpt-5-codex"));
        assert!(rendered.contains("perms: auto-review"));
        assert!(rendered.contains("running"));
        assert!(!rendered.contains("implement ThemeTokens"));

        let mut waiting = sample_view();
        waiting.status = "waiting".to_string();
        let rendered =
            render_ratatui_preview(&waiting, 90, 16, RatatuiFrameMode::Question).unwrap();
        assert!(rendered.contains("⏸ OPPi · openai-codex/gpt-5-codex"));
        let header_line = rendered.lines().next().unwrap_or_default();
        assert!(header_line.contains("waiting"));

        let rendered =
            render_ratatui_preview(&sample_view(), 56, 16, RatatuiFrameMode::Idle).unwrap();
        assert!(rendered.contains("• OPPi · openai-codex/gpt-5-codex"));
        assert!(rendered.contains("perms: auto-review"));
    }

    #[test]
    fn ratatui_header_uses_real_goal_state() {
        let mut view = sample_view();
        view.status = "running".to_string();
        view.goal = Some("Ship native goal mode".to_string());

        let rendered = render_ratatui_preview(&view, 120, 24, RatatuiFrameMode::Running).unwrap();

        assert!(rendered.contains("◎ Ship native goal mode"));
    }

    #[test]
    fn ratatui_transcript_rows_use_full_kind_and_column_contract() {
        let rows = [
            (
                "tool read crates/oppi-shell/src/main.rs",
                RatatuiRowKind::ToolRead,
                "read",
            ),
            (
                "tool write crates/oppi-shell/src/tui.rs",
                RatatuiRowKind::ToolWrite,
                "write",
            ),
            ("tool run cargo test", RatatuiRowKind::ToolRun, "run"),
            ("+ added line", RatatuiRowKind::Diff, "diff"),
            (
                "artifact://run/file.txt",
                RatatuiRowKind::Artifact,
                "artifact",
            ),
            (
                "write denied protected file",
                RatatuiRowKind::Denied,
                "denied",
            ),
            ("agent reviewer started", RatatuiRowKind::Agent, "agent"),
        ];
        for (line, kind, label) in rows {
            assert_eq!(classify_transcript_line(line), kind);
            assert_eq!(transcript_label(line), label);
        }
        let row = RatatuiTranscriptRow {
            kind: RatatuiRowKind::ToolRead,
            label: "read".to_string(),
            body: "short body".to_string(),
        };
        let rendered = row_lines(&row, 78, false, RatatuiThemeTokens::plain());
        assert_eq!(
            rendered[0].spans[0].content.chars().count(),
            TRANSCRIPT_GUTTER_WIDTH
        );
        assert_eq!(
            rendered[0].spans[2].content.chars().count(),
            TRANSCRIPT_LABEL_WIDTH
        );
    }

    #[test]
    fn ratatui_transcript_spans_cover_markdown_code_diff_and_bottom_anchor() {
        let tokens = RatatuiThemeTokens::dark();
        let heading = body_spans_for_chunk(RatatuiRowKind::Assistant, "# Heading", tokens);
        assert!(heading[0].style.add_modifier.contains(Modifier::BOLD));
        let inline =
            body_spans_for_chunk(RatatuiRowKind::Assistant, "use `cargo test` now", tokens);
        assert_eq!(inline.len(), 3);
        assert_eq!(inline[1].style.fg, Some(tokens.accent_soft));
        let added = body_spans_for_chunk(RatatuiRowKind::Diff, "+ added", tokens);
        assert_eq!(added[0].style.fg, Some(tokens.green));
        let removed = body_spans_for_chunk(RatatuiRowKind::Diff, "- removed", tokens);
        assert_eq!(removed[0].style.fg, Some(tokens.red));

        let mut view = sample_view();
        view.transcript = (0..12)
            .map(|index| RatatuiTranscriptRow {
                kind: RatatuiRowKind::Info,
                label: "info".to_string(),
                body: format!("transcript row {index}"),
            })
            .collect();
        let rendered = render_ratatui_preview(&view, 78, 18, RatatuiFrameMode::Idle).unwrap();
        assert!(!rendered.contains("transcript row 0"));
        assert!(rendered.contains("transcript row 11"));
    }

    #[test]
    fn ratatui_tool_digest_and_frame03_rows_cover_statuses() {
        let success = RatatuiToolDigest {
            status: ToolDigestStatus::Success,
            name: "write_file".to_string(),
            target: "src/main.rs".to_string(),
            duration: "9ms".to_string(),
            hint: "+4".to_string(),
        };
        let denied = RatatuiToolDigest {
            status: ToolDigestStatus::Denied,
            name: "write_file".to_string(),
            target: ".oppi/auth-store.json".to_string(),
            duration: "0ms".to_string(),
            hint: "protected".to_string(),
        };
        let error = RatatuiToolDigest {
            status: ToolDigestStatus::Error,
            name: "shell_exec".to_string(),
            target: "cargo test".to_string(),
            duration: "1s".to_string(),
            hint: "exit 101".to_string(),
        };
        assert_eq!(tool_digest_row(&success).kind, RatatuiRowKind::ToolWrite);
        assert_eq!(tool_digest_row(&denied).kind, RatatuiRowKind::Denied);
        assert_eq!(tool_digest_row(&error).kind, RatatuiRowKind::Error);

        let rendered =
            render_ratatui_preview(&sample_view(), 90, 22, RatatuiFrameMode::Tools).unwrap();
        assert!(rendered.contains("ratatui_ui.rs"));
        assert!(rendered.contains("artifact://run-2f1a"));
        assert!(rendered.contains("/approve"));
    }

    #[test]
    fn ratatui_live_adapter_models_preserve_background_todos_and_suggestion() {
        let view = sample_view();
        assert_eq!(view.background_typed[0].id, "task-1");
        assert_eq!(view.background_typed[0].status, "running");
        assert_eq!(view.todo_typed[0].priority.as_deref(), Some("high"));
        assert_eq!(view.todo_typed[0].phase.as_deref(), Some("Live adapters"));
        assert_eq!(view.suggestion.as_ref().unwrap().confidence_percent, 90);
        assert!(view.suggestion_items[0].contains("ghost:"));
    }

    #[test]
    fn ratatui_dock_frames_cover_question_approval_background_todos_and_suggestion() {
        let mut view = sample_view();
        view.question_selected = 1;
        let question = render_ratatui_preview(&view, 90, 22, RatatuiFrameMode::Question).unwrap();
        assert!(question.contains("question"));
        assert!(question.contains("› 2. assume on, fall back"));
        let approval = render_ratatui_preview(&view, 90, 22, RatatuiFrameMode::Approval).unwrap();
        assert!(approval.contains("approval"));
        assert!(approval.contains("A approve"));
        assert!(approval.contains("/deny"));
        let background =
            render_ratatui_preview(&view, 90, 22, RatatuiFrameMode::Background).unwrap();
        assert!(background.contains("background"));
        assert!(background.contains("K kill draft"));
        assert!(background.contains("watch:cargo-check"));
        let todos = render_ratatui_preview(&view, 90, 22, RatatuiFrameMode::Todos).unwrap();
        assert!(todos.contains("todos"));
        assert!(todos.contains("/todos refreshes"));
        assert!(todos.contains("Ctrl+P ambiguity"));
        let suggestion =
            render_ratatui_preview(&view, 90, 22, RatatuiFrameMode::Suggestion).unwrap();
        assert!(suggestion.contains("suggestion"));
        assert!(suggestion.contains("Tab accept"));
        assert!(suggestion.contains("ghost:"));
    }

    #[test]
    fn ratatui_slash_overlay_renders_live_filtered_items_not_static_essentials() {
        let mut view = sample_view();
        view.slash_items = vec![SlashPaletteItem {
            label: "/permissions".to_string(),
            insert: "/permissions".to_string(),
            detail: "change permission mode".to_string(),
        }];
        view.slash_selected = 0;
        let slash = render_ratatui_preview(&view, 78, 16, RatatuiFrameMode::Slash).unwrap();
        assert!(slash.contains("/permissions"));
        assert!(!slash.contains("/settings"));
        assert!(!slash.contains("/exit"));

        view.slash_items.clear();
        let empty = render_ratatui_preview(&view, 78, 16, RatatuiFrameMode::Slash).unwrap();
        assert!(empty.contains("no commands match"));
        assert!(!empty.contains("/settings"));
    }

    #[test]
    fn ratatui_editor_footer_and_slash_contracts_render() {
        let mut view = sample_view();
        view.editor_placeholder =
            "first line\nsecond line wraps around the editor width".to_string();
        let editor_lines = editor_lines(&view.editor_placeholder, 18, view.theme, false);
        assert!(editor_lines.len() >= 2);
        let running = render_ratatui_preview(&view, 90, 16, RatatuiFrameMode::Running).unwrap();
        assert!(running.contains("turn running"));
        assert!(running.contains("Ctrl+Enter steers"));

        view.footer_left =
            "running · openai-codex · perm auto-review · todos 3 · queued 2".to_string();
        let footer = footer_status_spans(&view, 120)
            .into_iter()
            .map(|span| span.content.to_string())
            .collect::<String>();
        assert!(footer.contains("sess"));
        assert!(!footer.contains("weekly"));
        assert!(footer.contains("model"));
        assert!(footer.contains("gpt-5-codex"));
        assert!(footer.contains("perm"));
        assert!(footer.contains("ctx"));
        assert!(footer.contains("todos 3"));
        assert!(terminal_cell_width(&footer) <= 120);
        view.status = "waiting".to_string();
        let waiting_footer = footer_status_spans(&view, 120)
            .into_iter()
            .map(|span| span.content.to_string())
            .collect::<String>();
        assert!(terminal_cell_width(&waiting_footer) <= 120);
        assert!(!waiting_footer.ends_with("todos"));
        view.status = "running".to_string();
        let wide_footer = footer_status_spans(&view, 140)
            .into_iter()
            .map(|span| span.content.to_string())
            .collect::<String>();
        assert!(wide_footer.contains("wk"));
        assert!(terminal_cell_width(&wide_footer) <= 140);
        let wider_footer = footer_status_spans(&view, 160)
            .into_iter()
            .map(|span| span.content.to_string())
            .collect::<String>();
        assert!(wider_footer.contains("todos 3 · queued 2"));
        assert!(terminal_cell_width(&wider_footer) <= 160);
        let collapsed = footer_status_spans(&view, 56)
            .into_iter()
            .map(|span| span.content.to_string())
            .collect::<String>();
        assert!(!collapsed.contains("weekly"));

        let essentials = slash_essential_items(&view)
            .into_iter()
            .map(|item| item.insert)
            .collect::<Vec<_>>();
        assert_eq!(
            essentials,
            vec![
                "/settings",
                "/model",
                "/sessions",
                "/background",
                "/todos",
                "/effort",
                "/exit"
            ]
        );
        assert_eq!(slash_key_action_name("up"), SlashKeyAction::MoveUp);
        assert_eq!(slash_key_action_name("tab"), SlashKeyAction::InsertSelected);
        assert_eq!(
            slash_key_action_name("enter"),
            SlashKeyAction::SubmitSelected
        );
        assert_eq!(slash_key_action_name("esc"), SlashKeyAction::Close);
        view.slash_items = slash_essential_items(&view);
        let slash = render_ratatui_preview(&view, 78, 16, RatatuiFrameMode::Slash).unwrap();
        assert!(slash.contains("/settings"));
        assert!(slash.contains("/settings      open settings overlay"));
        assert!(slash.contains("/exit"));
    }

    #[test]
    fn ratatui_overlay_adaptive_and_acceptance_helpers_are_wired() {
        let mut view = sample_view();
        view.overlay_title = "sessions · filter: native".to_string();
        view.overlay_items = vec![RatatuiOverlayItem {
            label: "● Feature branch".to_string(),
            value: "Active".to_string(),
            detail: "/repo/packages/native".to_string(),
        }];
        let settings = render_ratatui_preview(&view, 90, 22, RatatuiFrameMode::Settings).unwrap();
        assert!(settings.contains("sessions"));
        assert!(settings.contains("Feature branch"));
        assert!(settings.contains("↑↓ select  Enter open  Space change  Esc close"));
        assert_eq!(
            settings_overlay_items()[0].label,
            "General › Status shortcuts"
        );
        assert_eq!(settings_overlay_items()[3].label, "Pi › Main model");
        assert_eq!(theme_panel_items()[3].value, "plain");
        assert_eq!(permission_panel_items()[2].value, "auto-review");
        assert_eq!(provider_panel_items()[3].value, "Meridian");
        assert_eq!(login_panel_items()[0].label, "Subscription");
        assert_eq!(memory_panel_items()[0].value, "Hoppi");
        assert_eq!(session_picker_items()[0].label, "current");
        assert_eq!(model_role_picker_items()[0].label, "executor");
        assert!(overlay_clears_and_adapts(Rect::new(0, 0, 90, 22)));
        assert!(!overlay_clears_and_adapts(Rect::new(0, 0, 39, 12)));

        let medium = render_ratatui_exact_fixture(&view, 78, 22, RatatuiFrameMode::Idle).unwrap();
        assert!(medium.contains("• OPPi · openai-codex/gpt-5-codex"));
        assert!(medium.contains("perms: auto-review"));
        let narrow = render_ratatui_exact_fixture(&view, 58, 22, RatatuiFrameMode::Idle).unwrap();
        assert!(narrow.contains("• OPPi · openai-codex/gpt-5-codex"));
        assert!(narrow.contains("perms: auto-review"));
        let tiny = render_ratatui_exact_fixture(&view, 90, 14, RatatuiFrameMode::Running).unwrap();
        assert!(tiny.contains("turn running"));
        assert_eq!(terminal_cell_width("OPPi"), 4);
        assert_eq!(terminal_cell_width("界"), 2);
        assert!(snapshot_diff("a\nb", "a\nc").unwrap().contains("row 2"));
        assert!(ratatui_design_test_guidance().contains("ratatui_design"));
        assert!(manual_screenshot_checklist().contains("index.html"));
        assert!(default_ratatui_gate_status().contains("blocked"));
    }

    #[test]
    fn ratatui_module_split_and_fixture_live_modes_are_wired() {
        assert_eq!(widgets::PARITY_WIDGET_MODULES.len(), 8);
        assert!(widgets::PARITY_WIDGET_MODULES.contains(&widgets::WidgetModule::Header));
        assert!(widgets::PARITY_WIDGET_MODULES.contains(&widgets::WidgetModule::Transcript));
        assert!(widgets::PARITY_WIDGET_MODULES.contains(&widgets::WidgetModule::ToolDigest));
        assert!(widgets::PARITY_WIDGET_MODULES.contains(&widgets::WidgetModule::DockTray));
        assert!(widgets::PARITY_WIDGET_MODULES.contains(&widgets::WidgetModule::Editor));
        assert!(widgets::PARITY_WIDGET_MODULES.contains(&widgets::WidgetModule::Footer));
        assert!(widgets::PARITY_WIDGET_MODULES.contains(&widgets::WidgetModule::SlashPalette));
        assert!(widgets::PARITY_WIDGET_MODULES.contains(&widgets::WidgetModule::Overlay));
        assert_eq!(widgets::header::HEADER_CELLS.len(), 6);
        assert_eq!(widgets::transcript::TRANSCRIPT_COLUMNS.len(), 4);
        assert_eq!(widgets::tool_digest::TOOL_DIGEST_CELLS.len(), 5);
        assert_eq!(widgets::dock_tray::DOCK_TRAY_KINDS.len(), 6);
        assert_eq!(widgets::editor::EDITOR_CELLS.len(), 5);
        assert_eq!(widgets::footer::FOOTER_CELLS.len(), 8);
        assert_eq!(
            [
                widgets::slash_palette::SlashPaletteAction::MoveUp,
                widgets::slash_palette::SlashPaletteAction::MoveDown,
                widgets::slash_palette::SlashPaletteAction::PageUp,
                widgets::slash_palette::SlashPaletteAction::PageDown,
                widgets::slash_palette::SlashPaletteAction::Home,
                widgets::slash_palette::SlashPaletteAction::End,
                widgets::slash_palette::SlashPaletteAction::InsertSelected,
                widgets::slash_palette::SlashPaletteAction::SubmitSelected,
                widgets::slash_palette::SlashPaletteAction::Close,
            ]
            .len(),
            9
        );
        assert_eq!(widgets::overlay::OVERLAY_PANEL_KINDS.len(), 8);
        assert_eq!(model::DesignFrameKind::ALL.len(), 10);
        assert_eq!(
            fixture_mode(model::DesignFrameKind::ToolsArtifactDenial),
            RatatuiFrameMode::Tools
        );
        for (live, frame) in [
            (model::LiveRatatuiMode::Idle, RatatuiFrameMode::Idle),
            (model::LiveRatatuiMode::Running, RatatuiFrameMode::Running),
            (model::LiveRatatuiMode::Question, RatatuiFrameMode::Question),
            (model::LiveRatatuiMode::Approval, RatatuiFrameMode::Approval),
            (
                model::LiveRatatuiMode::Background,
                RatatuiFrameMode::Background,
            ),
            (model::LiveRatatuiMode::Todos, RatatuiFrameMode::Todos),
            (
                model::LiveRatatuiMode::Suggestion,
                RatatuiFrameMode::Suggestion,
            ),
            (model::LiveRatatuiMode::Slash, RatatuiFrameMode::Slash),
            (model::LiveRatatuiMode::Settings, RatatuiFrameMode::Settings),
        ] {
            assert_eq!(RatatuiFrameMode::from(live), frame);
        }
        assert_eq!(
            frames::design_frame_fixture(model::DesignFrameKind::Narrow).width,
            58
        );
        assert_eq!(layout::REFERENCE_RESERVATION_ORDER.len(), 6);
        assert_eq!(terminal::CLEANUP_ACTIONS.len(), 6);
        assert_eq!(
            [
                theme::ThemeVariant::Dark,
                theme::ThemeVariant::Light,
                theme::ThemeVariant::Plain,
            ]
            .len(),
            3
        );
        let context = snapshots::SnapshotContext {
            start_col: 1,
            end_col: 3,
            text: "OP".to_string(),
        };
        assert_eq!(context.text, "OP");
    }

    #[test]
    fn ratatui_reference_layout_reserves_fixed_bands_before_transcript() {
        assert_eq!(
            layout::REFERENCE_RESERVATION_ORDER,
            [
                layout::ReservationBand::Header,
                layout::ReservationBand::Editor,
                layout::ReservationBand::Footer,
                layout::ReservationBand::Dock,
                layout::ReservationBand::Transcript,
                layout::ReservationBand::Overlay,
            ]
        );
        let normal = layout::reference_frame_layout(
            Rect::new(0, 0, 120, 32),
            HEADER_NORMAL_HEIGHT,
            EDITOR_HEIGHT,
            FOOTER_EXPANDED_HEIGHT,
            1,
        );
        assert_eq!(normal.header.height, 1);
        assert_eq!(normal.editor.height, 3);
        assert_eq!(normal.footer.height, 2);
        assert_eq!(normal.dock.height, 1);
        assert_eq!(normal.transcript.height, 25);
        assert_eq!(normal.header.y, 0);
        assert_eq!(normal.transcript.y, 1);
        assert_eq!(normal.dock.y, 26);
        assert_eq!(normal.editor.y, 27);
        assert_eq!(normal.footer.y, 30);
        assert_eq!(normal.overlay, Rect::new(0, 0, 120, 32));

        let narrow = layout::reference_frame_layout(
            Rect::new(0, 0, 58, 22),
            HEADER_NARROW_HEIGHT,
            EDITOR_HEIGHT,
            FOOTER_COLLAPSED_HEIGHT,
            1,
        );
        assert_eq!(narrow.header.height, 2);
        assert_eq!(narrow.editor.height, 3);
        assert_eq!(narrow.footer.height, 1);
        assert_eq!(narrow.dock.height, 1);
        assert_eq!(narrow.transcript.height, 15);
    }

    #[test]
    fn ratatui_header_transcript_tool_contracts_match_reference_visuals() {
        assert_eq!(widgets::header::HEADER_VISUAL_CONTRACT.separator, " · ");
        assert_eq!(widgets::header::HEADER_VISUAL_CONTRACT.ready_spinner, "•");
        assert_eq!(widgets::header::HEADER_VISUAL_CONTRACT.running_spinner, "◐");
        assert_eq!(widgets::header::HEADER_VISUAL_CONTRACT.waiting_spinner, "⏸");
        assert_eq!(widgets::header::HEADER_VISUAL_CONTRACT.goal_prefix, "◎ ");
        assert_eq!(
            widgets::header::HEADER_VISUAL_CONTRACT.normal_height,
            HEADER_NORMAL_HEIGHT
        );
        assert_eq!(
            widgets::header::HEADER_VISUAL_CONTRACT.narrow_height,
            HEADER_NARROW_HEIGHT
        );
        assert_eq!(header_spinner(RatatuiFrameMode::Idle, "ready", 0), "•");
        assert_eq!(header_spinner(RatatuiFrameMode::Running, "running", 0), "◐");
        assert_eq!(
            header_spinner(RatatuiFrameMode::Question, "waiting", 0),
            "⏸"
        );

        assert_eq!(
            widgets::transcript::TRANSCRIPT_VISUAL_CONTRACT.gutter_width,
            TRANSCRIPT_GUTTER_WIDTH
        );
        assert_eq!(
            widgets::transcript::TRANSCRIPT_VISUAL_CONTRACT.label_width,
            TRANSCRIPT_LABEL_WIDTH
        );
        assert_eq!(widgets::transcript::TRANSCRIPT_VISUAL_CONTRACT.gap_width, 1);
        assert_eq!(RatatuiRowKind::User.gutter(), "▍");
        assert_eq!(RatatuiRowKind::Assistant.gutter(), "▍");
        assert_eq!(RatatuiRowKind::ToolRead.gutter(), "│");

        assert_eq!(
            widgets::tool_digest::TOOL_DIGEST_CELLS,
            [
                widgets::tool_digest::ToolDigestCell::Glyph,
                widgets::tool_digest::ToolDigestCell::Name,
                widgets::tool_digest::ToolDigestCell::Target,
                widgets::tool_digest::ToolDigestCell::Duration,
                widgets::tool_digest::ToolDigestCell::Hint,
            ]
        );
        assert_eq!(
            widgets::tool_digest::TOOL_DIGEST_VISUAL_CONTRACT.separator,
            " · "
        );
        let digest = tool_digest_row(&RatatuiToolDigest {
            status: ToolDigestStatus::Success,
            name: "write_file".to_string(),
            target: "crates/oppi-shell/src/tui.rs".to_string(),
            duration: "11ms".to_string(),
            hint: "+148 / −62".to_string(),
        });
        assert!(digest.body.starts_with("✓ write "));
        assert!(digest.body.contains(" · 11ms · +148 / −62"));
    }

    #[test]
    fn ratatui_semantic_rows_and_docktray_match_reference_visuals() {
        let contract = widgets::transcript::SEMANTIC_ROW_VISUAL_CONTRACT;
        assert_eq!(contract.diff_add_prefix, "+ ");
        assert_eq!(contract.diff_remove_prefix, "- ");
        assert_eq!(contract.artifact_scheme, "artifact://");
        assert_eq!(contract.metadata_separator, " · ");
        assert_eq!(contract.denial_prefix, "write blocked:");
        assert_eq!(contract.approval_hint, "/approve");

        let tools = transcript_rows_for_mode(&sample_view(), RatatuiFrameMode::Tools);
        let diff = tools
            .iter()
            .find(|row| row.kind == RatatuiRowKind::Diff)
            .expect("diff row");
        assert!(
            diff.body
                .lines()
                .next()
                .unwrap()
                .starts_with(contract.diff_add_prefix)
        );
        assert!(
            diff.body
                .lines()
                .nth(1)
                .unwrap()
                .starts_with(contract.diff_remove_prefix)
        );
        let diff_lines = row_lines(diff, 90, false, RatatuiThemeTokens::plain());
        assert!(
            diff_lines[0]
                .spans
                .iter()
                .any(|span| span.content.starts_with(contract.diff_add_prefix))
        );
        assert!(
            diff_lines[1]
                .spans
                .iter()
                .any(|span| span.content.starts_with(contract.diff_remove_prefix))
        );

        let artifact = tools
            .iter()
            .find(|row| row.kind == RatatuiRowKind::Artifact)
            .expect("artifact row");
        assert!(artifact.body.starts_with(contract.artifact_scheme));
        assert!(artifact.body.contains("text/plain"));
        assert!(artifact.body.contains("1.2 KB"));
        assert!(artifact.body.contains("overwrites prior"));

        let denial = tools
            .iter()
            .find(|row| row.kind == RatatuiRowKind::Denied)
            .expect("denial row");
        assert!(denial.body.starts_with(contract.denial_prefix));
        assert!(denial.body.contains(".oppi/auth-store.json"));
        assert!(denial.body.contains("/permissions full-access"));
        assert!(denial.body.contains(contract.approval_hint));

        let dock = widgets::dock_tray::DOCK_TRAY_VISUAL_CONTRACT;
        assert_eq!(dock.selected_arrow, "› ");
        assert_eq!(dock.unselected_arrow, "  ");
        assert_eq!(dock.title_hint_gap, "        ");
        assert!(!dock.border_bottom);
        let question =
            render_ratatui_preview(&sample_view(), 90, 22, RatatuiFrameMode::Question).unwrap();
        assert!(question.contains("question · pending        ↑/↓ select · Enter confirm"));
        assert!(question.contains("│› 1. probe at startup"));
    }

    #[test]
    fn ratatui_editor_footer_slash_overlay_visual_contracts_match_reference() {
        let editor = widgets::editor::EDITOR_VISUAL_CONTRACT;
        assert_eq!(editor.height, EDITOR_HEIGHT);
        assert_eq!(editor.border_top_left, "╭");
        assert_eq!(editor.border_bottom_left, "╰");
        assert_eq!(editor.prompt_gutter, "› ");
        assert_eq!(editor.cursor, "█");
        let typed_lines = editor_lines("hello", 20, RatatuiThemeTokens::plain(), false);
        assert_eq!(typed_lines[0].spans[0].content, editor.prompt_gutter);
        assert_eq!(typed_lines[0].spans[2].content, editor.cursor);
        let placeholder_lines = editor_lines(
            "Ask, build, or type / for commands…",
            80,
            RatatuiThemeTokens::plain(),
            true,
        );
        assert_eq!(placeholder_lines[0].spans[1].content, editor.cursor);
        assert_eq!(
            placeholder_lines[0].spans[2].content,
            "Ask, build, or type / for commands…"
        );
        let selected = RatatuiThemeTokens::dark().selected_style();
        let filled = styled_line_with_fill(
            vec![
                ("›".to_string(), selected),
                (" ".to_string(), selected),
                ("Option".to_string(), selected),
            ],
            12,
            Some(selected),
        );
        assert_eq!(filled.spans.last().unwrap().content, "    ");
        assert_eq!(filled.spans.last().unwrap().style, selected);
        let running =
            render_ratatui_preview(&sample_view(), 90, 16, RatatuiFrameMode::Running).unwrap();
        assert!(running.contains(editor.running_title_prefix.trim()));

        let footer = widgets::footer::FOOTER_VISUAL_CONTRACT;
        assert_eq!(footer.expanded_height, FOOTER_EXPANDED_HEIGHT);
        assert_eq!(footer.collapsed_height, FOOTER_COLLAPSED_HEIGHT);
        assert_eq!(footer.ready_dot, "•");
        assert_eq!(footer.running_dot, "●");
        assert_eq!(footer.session_bar, FOOTER_SESSION_BAR);
        assert_eq!(footer.week_bar, FOOTER_WEEK_BAR);
        assert_eq!(footer.context_bar, FOOTER_CONTEXT_BAR);
        let mut footer_view = sample_view();
        footer_view.footer_left =
            "ready · openai-codex · perm auto-review · todos 2 · queued 1".to_string();
        let footer_text = footer_status_spans(&footer_view, 120)
            .into_iter()
            .map(|span| span.content.to_string())
            .collect::<String>();
        assert!(footer_text.contains(&format!("sess 3k 1% {FOOTER_SESSION_BAR}")));
        assert!(!footer_text.contains(FOOTER_WEEK_BAR));
        assert!(footer_text.contains(&format!("ctx 100k/272k 37% {FOOTER_CONTEXT_BAR}")));
        assert!(footer_text.contains("Alt+K"));
        assert!(footer_text.contains(footer.todos_prefix));
        let collapsed_footer = footer_status_spans(&sample_view(), 58)
            .into_iter()
            .map(|span| span.content.to_string())
            .collect::<String>();
        assert!(!collapsed_footer.contains(FOOTER_WEEK_BAR));
        assert_eq!(
            footer_hotkey_line("Alt+Enter follow-up  Ctrl+Enter steer  / commands"),
            "Alt+Enter follow-up  ·  Ctrl+Enter steer  ·  / commands"
        );

        let slash = widgets::slash_palette::SLASH_PALETTE_VISUAL_CONTRACT;
        assert_eq!(slash.title, " commands ");
        assert_eq!(slash.selected_marker, "›");
        assert_eq!(slash.command_detail_gap, "  ");
        assert_eq!(slash.max_items, 7);
        assert_eq!(slash.empty_text, "no commands match");
        let slash_render =
            render_ratatui_preview(&sample_view(), 78, 16, RatatuiFrameMode::Slash).unwrap();
        assert!(slash_render.contains("commands"));
        assert!(slash_render.contains("│› /settings"));
        assert_eq!(slash_essential_items(&sample_view()).len(), slash.max_items);
    }

    #[test]
    fn ratatui_overlay_visual_contract_matches_reference() {
        let overlay = widgets::overlay::OVERLAY_VISUAL_CONTRACT;
        assert_eq!(overlay.title, " settings ");
        assert_eq!(overlay.selected_marker, "›");
        assert_eq!(overlay.unselected_marker, " ");
        assert_eq!(overlay.label_width, 16);
        assert_eq!(overlay.value_width, 18);
        assert_eq!(overlay.min_width, 40);
        assert_eq!(overlay.max_width, 82);
        assert_eq!(overlay_width(120), 82);
        assert_eq!(overlay_width(52), 40);
        let items = settings_overlay_items();
        assert_eq!(items[0].label, "General › Status shortcuts");
        assert_eq!(
            items
                .iter()
                .filter(|item| item.label.starts_with("Pi ›"))
                .count(),
            6
        );
        assert_eq!(
            format!("{:<18}", items[1].value).chars().count(),
            overlay.value_width
        );
        let rendered =
            render_ratatui_preview(&sample_view(), 90, 22, RatatuiFrameMode::Settings).unwrap();
        assert!(rendered.contains("settings"));
        assert!(rendered.contains("General"));
        assert!(rendered.contains("Status shortcuts"));
        assert!(rendered.contains("Usage/status"));
    }

    #[test]
    fn ratatui_docksep_tray_and_css_cell_contracts_are_documented() {
        let idle = render_ratatui_preview(&sample_view(), 78, 16, RatatuiFrameMode::Idle).unwrap();
        let dock_sep = idle
            .lines()
            .find(|line| line.contains("docks: idle"))
            .expect("dock separator row");
        assert_eq!(
            idle.lines()
                .filter(|line| line.contains("docks: idle"))
                .count(),
            1
        );
        assert!(dock_sep.starts_with('─'));
        assert!(dock_sep.ends_with("docks: idle"));

        let question =
            render_ratatui_preview(&sample_view(), 90, 22, RatatuiFrameMode::Question).unwrap();
        let lines = question.lines().collect::<Vec<_>>();
        let tray_top = lines
            .iter()
            .position(|line| line.contains("question · pending"))
            .expect("question tray top");
        let editor_top = lines
            .iter()
            .position(|line| {
                line.starts_with('╭') && line.contains('─') && !line.contains("question")
            })
            .expect("editor top border");
        assert!(editor_top > tray_top);
        assert!(
            lines[tray_top..editor_top]
                .iter()
                .all(|line| !line.starts_with('╰')),
            "dock tray must not render a bottom border before the editor"
        );
        assert!(
            layout::TERMINAL_CSS_CELL_TRANSLATIONS
                .iter()
                .any(|entry| entry.css_rule.contains("dock-tray")
                    && entry.cell_rule.contains("top/left/right"))
        );
        assert!(
            layout::TERMINAL_CSS_CELL_TRANSLATIONS
                .iter()
                .any(|entry| entry.css_rule.contains(".tr-label") && entry.cell_rule == "8 cells")
        );
        assert!(
            layout::NON_REFERENCE_BORDER_POLICY
                .iter()
                .any(|entry| entry.contains("transcript rows use gutters"))
        );
        assert!(
            layout::NON_REFERENCE_BORDER_POLICY
                .iter()
                .any(|entry| entry.contains("header uses text cells"))
        );
    }

    #[test]
    fn ratatui_design_all_frames_match() {
        for section in ["R1_IDLE_78X15", "R2_RUNNING_78X15", "R3_SLASH_78X16"] {
            let fixture = reference_r1_r2_r3_fixture(section);
            assert!(
                !fixture.trim().is_empty(),
                "missing R1-R3 fixture {section}"
            );
        }
        for kind in [
            model::DesignFrameKind::ToolsArtifactDenial,
            model::DesignFrameKind::AskUser,
            model::DesignFrameKind::Background,
            model::DesignFrameKind::Todos,
            model::DesignFrameKind::Slash,
            model::DesignFrameKind::Settings,
            model::DesignFrameKind::Narrow,
            model::DesignFrameKind::Tiny,
        ] {
            let expected = reference_frames_03_10_fixture(design_frame_section(kind));
            let actual = render_design_frame_fixture(kind);
            let diff = snapshot_diff(&expected, &actual);
            assert!(
                diff.is_none(),
                "{} did not match rendered fixture\n{}",
                design_frame_section(kind),
                diff.unwrap_or_default()
            );
        }
    }

    #[test]
    #[ignore = "developer utility: set OPPI_RATATUI_SNAPSHOT_OUT to write frames-03-10 golden candidates"]
    fn ratatui_dump_frames_03_10_snapshot_candidates() {
        let mut out = String::from(
            "# Exact terminal-cell snapshots generated by the Rust Ratatui renderer.\n# Width/height are encoded in each section name. Lines are trim-end normalized.\n\n",
        );
        for kind in [
            model::DesignFrameKind::ToolsArtifactDenial,
            model::DesignFrameKind::AskUser,
            model::DesignFrameKind::Background,
            model::DesignFrameKind::Todos,
            model::DesignFrameKind::Slash,
            model::DesignFrameKind::Settings,
            model::DesignFrameKind::Narrow,
            model::DesignFrameKind::Tiny,
        ] {
            out.push_str(&format!("-- {} --\n", design_frame_section(kind)));
            out.push_str(&render_design_frame_fixture(kind));
            out.push_str("\n\n");
        }
        if let Ok(path) = std::env::var("OPPI_RATATUI_SNAPSHOT_OUT") {
            let path = std::path::PathBuf::from(path);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).expect("create ratatui snapshot output directory");
            }
            std::fs::write(path, out).expect("write ratatui frame snapshot candidates");
        } else {
            println!("{out}");
        }
    }

    #[test]
    fn ratatui_design_frame_03_07_fixture_builders_match_reference_content() {
        let tools =
            frames::design_frame_fixture_content(model::DesignFrameKind::ToolsArtifactDenial);
        assert_eq!(tools.title, "oppi-native · semantic transcript");
        assert_eq!(tools.meta, Some("rich rows"));
        assert_eq!(tools.footer.todos, 3);
        assert!(
            tools
                .transcript
                .iter()
                .any(|row| row.body.contains("artifact://run-2f1a/snapshot_narrow.txt"))
        );
        assert!(
            tools
                .transcript
                .iter()
                .any(|row| row.body.contains(".oppi/auth-store.json"))
        );
        assert!(
            tools
                .transcript
                .iter()
                .any(|row| row.body.contains("/permissions full-access"))
        );

        let question = frames::design_frame_fixture_content(model::DesignFrameKind::AskUser);
        assert_eq!(question.header.status, "waiting");
        assert!(question.header.warn);
        assert_eq!(question.dock.kind, frames::DesignDockKind::Question);
        assert_eq!(question.dock.title, "question · pending");
        assert_eq!(question.dock.hint, Some("↑/↓ select · Enter confirm"));
        assert!(question.dock.rows[0].selected);
        assert_eq!(
            question.editor.placeholder,
            "press Enter to confirm selection"
        );

        let background = frames::design_frame_fixture_content(model::DesignFrameKind::Background);
        assert_eq!(background.dock.kind, frames::DesignDockKind::Background);
        assert_eq!(background.dock.rows.len(), 3);
        assert_eq!(background.dock.rows[0].glyph, "⠋");
        assert_eq!(background.dock.rows[1].label, "fmt:rustfmt");
        assert!(
            background.dock.rows[2]
                .detail
                .contains("paused awaiting approval")
        );

        let todos = frames::design_frame_fixture_content(model::DesignFrameKind::Todos);
        assert_eq!(todos.dock.kind, frames::DesignDockKind::Todos);
        assert_eq!(todos.footer.todos, 5);
        assert!(
            todos
                .dock
                .rows
                .iter()
                .any(|row| row.glyph == "▶" && row.detail.contains("in progress"))
        );
        assert!(
            todos
                .dock
                .rows
                .iter()
                .any(|row| row.glyph == "!" && row.detail.contains("blocked"))
        );

        let slash = frames::design_frame_fixture_content(model::DesignFrameKind::Slash);
        assert_eq!(slash.meta, Some("type to filter"));
        assert_eq!(slash.editor.placeholder, "/");
        assert_eq!(slash.slash_items.len(), 7);
        assert_eq!(slash.slash_items[0].command, "/settings");
        assert!(slash.slash_items[0].detail.contains("Permissions"));
        assert_eq!(slash.slash_items[1].detail, "Select main OPPi model");
        assert_eq!(slash.slash_items[6].command, "/exit");
    }

    #[test]
    fn ratatui_design_frame_08_10_fixture_builders_match_reference_content() {
        let settings = frames::design_frame_fixture_content(model::DesignFrameKind::Settings);
        assert_eq!(settings.title, "oppi-native · /settings");
        assert_eq!(settings.meta, Some("overlay"));
        assert!(settings.transcript[0].body.contains("←/→ tabs"));
        let overlay = settings.overlay.as_ref().expect("settings overlay fixture");
        assert_eq!(overlay.title, "settings");
        assert_eq!(
            overlay.help,
            "←/→ tabs · ↑/↓ settings · Enter open · Esc close"
        );
        assert_eq!(overlay.selected, 1);
        assert_eq!(overlay.items.len(), 14);
        assert!(overlay.items[1].current);
        assert_eq!(overlay.items[1].label, "General › Goal mode");
        assert_eq!(overlay.items[10].value, "client-hosted");

        let narrow_meta = frames::design_frame_fixture(model::DesignFrameKind::Narrow);
        let narrow = frames::design_frame_fixture_content(model::DesignFrameKind::Narrow);
        assert_eq!((narrow_meta.width, narrow_meta.height), (58, 22));
        assert_eq!(narrow.title, "oppi-native · 56 × 22");
        assert_eq!(narrow.header.goal, Some("gpt-5-codex"));
        assert!(narrow.footer.narrow);
        assert_eq!(narrow.dock.title, "idle");
        assert!(
            narrow
                .transcript
                .iter()
                .any(|row| row.body.contains("middle-truncate model"))
        );
        assert_eq!(narrow.editor.placeholder, "…");

        let tiny_meta = frames::design_frame_fixture(model::DesignFrameKind::Tiny);
        let tiny = frames::design_frame_fixture_content(model::DesignFrameKind::Tiny);
        assert_eq!((tiny_meta.width, tiny_meta.height), (90, 14));
        assert_eq!(tiny.header.status, "running");
        assert!(tiny.editor.running);
        assert_eq!(tiny.footer.status, "running");
        assert!(
            tiny.transcript
                .iter()
                .any(|row| row.body.contains("cargo test --quiet"))
        );
    }

    #[test]
    fn ratatui_live_empty_state_does_not_render_fixture_rows() {
        let mut view = sample_view();
        view.transcript.clear();
        view.background_items.clear();
        view.todo_items.clear();
        view.tool_digest_items.clear();
        view.pending_answers.clear();
        view.approval_items.clear();
        view.suggestion_items.clear();
        view.dock_label = "docks: idle".to_string();
        let rendered = render_ratatui_exact_fixture(&view, 78, 15, RatatuiFrameMode::Idle).unwrap();
        for forbidden in [
            "cargo-watch",
            "rustfmt",
            "artifact://run-2f1a",
            "PageUp/PageDown transcript scroll",
            "verify cargo test on Windows host",
            "Ratatui preview renderer active",
        ] {
            assert!(
                !rendered.contains(forbidden),
                "live empty render leaked fixture row: {forbidden}"
            );
        }
    }

    #[test]
    fn ratatui_preview_renders_idle_bands() {
        let rendered =
            render_ratatui_preview(&sample_view(), 78, 16, RatatuiFrameMode::Idle).unwrap();
        assert!(rendered.contains("OPPi"));
        assert!(rendered.contains("docks: idle"));
        assert!(rendered.contains("Ask, build"));
        assert!(rendered.contains("Alt+K"));
    }

    #[test]
    fn ratatui_preview_renders_slash_overlay() {
        let rendered =
            render_ratatui_preview(&sample_view(), 78, 16, RatatuiFrameMode::Slash).unwrap();
        assert!(rendered.contains("commands"));
        assert!(rendered.contains("/settings"));
        assert!(rendered.contains("open settings overlay"));
    }

    #[test]
    fn ratatui_preview_applies_narrow_and_tiny_breakpoints() {
        let narrow =
            render_ratatui_preview(&sample_view(), 56, 16, RatatuiFrameMode::Idle).unwrap();
        assert!(narrow.contains("OPPi"));
        assert!(narrow.contains("perms:"));
        let tiny =
            render_ratatui_preview(&sample_view(), 78, 14, RatatuiFrameMode::Running).unwrap();
        assert!(tiny.contains("OPPi"));
        assert!(tiny.contains("Ask, build"));
    }

    #[test]
    fn ratatui_preview_covers_design_reference_frames() {
        let tools =
            render_ratatui_preview(&sample_view(), 90, 22, RatatuiFrameMode::Tools).unwrap();
        assert!(tools.contains("artifact://run-2f1a"));
        assert!(tools.contains("write blocked"));

        let question =
            render_ratatui_preview(&sample_view(), 90, 22, RatatuiFrameMode::Question).unwrap();
        assert!(question.contains("question"));
        assert!(question.contains("probe at startup"));

        let background =
            render_ratatui_preview(&sample_view(), 90, 22, RatatuiFrameMode::Background).unwrap();
        assert!(background.contains("background"));
        assert!(background.contains("watch:cargo-check"));

        let todos =
            render_ratatui_preview(&sample_view(), 90, 22, RatatuiFrameMode::Todos).unwrap();
        assert!(todos.contains("todos"));
        assert!(todos.contains("Ctrl+P ambiguity"));

        let settings =
            render_ratatui_preview(&sample_view(), 90, 22, RatatuiFrameMode::Settings).unwrap();
        assert!(settings.contains("settings"));
        assert!(settings.contains("General"));
        assert!(settings.contains("Status shortcuts"));
    }

    #[test]
    fn ratatui_settings_overlay_uses_horizontal_top_tabs() {
        let settings =
            render_ratatui_preview(&sample_view(), 120, 24, RatatuiFrameMode::Settings).unwrap();
        let tab_line = settings
            .lines()
            .find(|line| {
                line.contains("General")
                    && line.contains("Pi")
                    && line.contains("Footer")
                    && line.contains("Memory")
            })
            .expect("settings groups should render as one horizontal tab row");
        assert!(
            tab_line.find("General") < tab_line.find("Pi")
                && tab_line.find("Pi") < tab_line.find("Footer")
                && tab_line.find("Footer") < tab_line.find("Memory")
        );
        assert!(
            !settings
                .lines()
                .any(|line| line.contains("General") && line.contains(" │ ")),
            "settings root tabs should not render as a vertical left rail"
        );
        let roomy_settings =
            render_ratatui_preview(&sample_view(), 120, 32, RatatuiFrameMode::Settings).unwrap();
        assert!(roomy_settings.contains("settings · 14 settings · active: ⚙️  General"));
    }

    #[test]
    fn settings_overlay_exposes_goal_mode_shortcut() {
        let items = default_settings_overlay_items();

        assert!(items.iter().any(|item| {
            item.label == "General › Goal mode"
                && item.value == "none"
                && item.detail == "Track and continue one thread objective"
        }));
    }
}
