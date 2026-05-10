//! Fixture-frame builders for `.reference/design-v2` parity.
//!
//! Fake/mock content belongs here, not in the live production render path.

use super::model::DesignFrameKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct DesignFrameFixture {
    pub(super) kind: DesignFrameKind,
    pub(super) width: u16,
    pub(super) height: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DesignFrameContent {
    pub(super) title: &'static str,
    pub(super) meta: Option<&'static str>,
    pub(super) header: DesignHeaderFixture,
    pub(super) transcript: Vec<DesignTranscriptRowFixture>,
    pub(super) dock: DesignDockFixture,
    pub(super) editor: DesignEditorFixture,
    pub(super) footer: DesignFooterFixture,
    pub(super) slash_items: Vec<DesignSlashItemFixture>,
    pub(super) overlay: Option<DesignOverlayFixture>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct DesignHeaderFixture {
    pub(super) status: &'static str,
    pub(super) goal: Option<&'static str>,
    pub(super) warn: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct DesignTranscriptRowFixture {
    pub(super) kind: &'static str,
    pub(super) label: &'static str,
    pub(super) gutter: &'static str,
    pub(super) body: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DesignDockFixture {
    pub(super) kind: DesignDockKind,
    pub(super) title: &'static str,
    pub(super) hint: Option<&'static str>,
    pub(super) rows: Vec<DesignDockRowFixture>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DesignDockKind {
    Separator,
    Question,
    Background,
    Todos,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct DesignDockRowFixture {
    pub(super) glyph: &'static str,
    pub(super) label: &'static str,
    pub(super) detail: &'static str,
    pub(super) selected: bool,
    pub(super) confirmed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct DesignEditorFixture {
    pub(super) placeholder: &'static str,
    pub(super) running: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct DesignFooterFixture {
    pub(super) status: &'static str,
    pub(super) todos: usize,
    pub(super) queued: usize,
    pub(super) narrow: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct DesignSlashItemFixture {
    pub(super) command: &'static str,
    pub(super) detail: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DesignOverlayFixture {
    pub(super) title: &'static str,
    pub(super) help: &'static str,
    pub(super) selected: usize,
    pub(super) confirmed: Option<usize>,
    pub(super) items: Vec<DesignOverlayItemFixture>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct DesignOverlayItemFixture {
    pub(super) label: &'static str,
    pub(super) value: &'static str,
    pub(super) detail: &'static str,
    pub(super) current: bool,
}

pub(super) fn design_frame_fixture(kind: DesignFrameKind) -> DesignFrameFixture {
    let (width, height) = match kind {
        DesignFrameKind::Narrow => (58, 22),
        DesignFrameKind::Tiny => (90, 14),
        _ => (120, 32),
    };
    DesignFrameFixture {
        kind,
        width,
        height,
    }
}

pub(super) fn design_frame_fixture_content(kind: DesignFrameKind) -> DesignFrameContent {
    match kind {
        DesignFrameKind::ToolsArtifactDenial => frame_tools_artifact_denial(),
        DesignFrameKind::AskUser => frame_ask_user(),
        DesignFrameKind::Background => frame_background(),
        DesignFrameKind::Todos => frame_todos(),
        DesignFrameKind::Slash => frame_slash(),
        DesignFrameKind::Settings => frame_settings(),
        DesignFrameKind::Narrow => frame_narrow(),
        DesignFrameKind::Tiny => frame_tiny(),
        _ => base_placeholder_frame(kind),
    }
}

fn base_placeholder_frame(kind: DesignFrameKind) -> DesignFrameContent {
    DesignFrameContent {
        title: match kind {
            DesignFrameKind::Idle => "oppi-native · idle",
            DesignFrameKind::Running => "oppi-native · turn running",
            _ => "oppi-native",
        },
        meta: None,
        header: DesignHeaderFixture {
            status: "ready",
            goal: None,
            warn: false,
        },
        transcript: Vec::new(),
        dock: separator("docks: idle"),
        editor: DesignEditorFixture {
            placeholder: "Ask, build, or type / for commands…",
            running: false,
        },
        footer: footer("ready", 0, 0, false),
        slash_items: Vec::new(),
        overlay: None,
    }
}

fn frame_tools_artifact_denial() -> DesignFrameContent {
    DesignFrameContent {
        title: "oppi-native · semantic transcript",
        meta: Some("rich rows"),
        header: header("ready", None, false),
        transcript: vec![
            row(
                "tool",
                "tool",
                "│",
                "✓ write_file crates/oppi-shell/src/tui.rs · 11ms · +148 / −62",
            ),
            row(
                "tool",
                "tool",
                "│",
                "+ const SUPPORTS_SYNC: bool = detect_sync_caps();\n- if !sync_supported() { write_unsynchronized(out)?; }",
            ),
            row(
                "artifact",
                "artifact",
                "│",
                "artifact://run-2f1a/snapshot_narrow.txt · text/plain · 1.2 KB · overwrites prior",
            ),
            row(
                "error",
                "denied",
                "│",
                "write blocked: .oppi/auth-store.json is on the protected-files list — escalate with /permissions full-access or use /approve",
            ),
            row(
                "assistant",
                "oppi",
                "▍",
                "snapshot recorded; protected file left untouched. Ready to retry the cargo test once you confirm.",
            ),
        ],
        dock: separator("docks: idle"),
        editor: editor("Ask, build, or type / for commands…", false),
        footer: footer("ready", 3, 0, false),
        slash_items: Vec::new(),
        overlay: None,
    }
}

fn frame_ask_user() -> DesignFrameContent {
    DesignFrameContent {
        title: "oppi-native · question pending",
        meta: None,
        header: header("waiting", None, true),
        transcript: vec![row(
            "assistant",
            "oppi",
            "▍",
            "I can wire CSI 2026 detection three ways. Which do you want?",
        )],
        dock: DesignDockFixture {
            kind: DesignDockKind::Question,
            title: "question · pending",
            hint: Some("↑/↓ select · Enter confirm"),
            rows: vec![
                dock_row(
                    "1.",
                    "probe at startup via DA1 response (50ms wait)",
                    "",
                    true,
                    false,
                ),
                dock_row(
                    "2.",
                    "assume on, fall back on first failure (no probe)",
                    "",
                    false,
                    false,
                ),
                dock_row(
                    "3.",
                    "expose a /probe-sync command, default off",
                    "",
                    false,
                    false,
                ),
            ],
        },
        editor: editor("press Enter to confirm selection", false),
        footer: footer("waiting", 2, 0, false),
        slash_items: Vec::new(),
        overlay: None,
    }
}

fn frame_background() -> DesignFrameContent {
    DesignFrameContent {
        title: "oppi-native · background work",
        meta: None,
        header: header("ready", None, false),
        transcript: vec![row(
            "info",
            "info",
            "│",
            "3 background tasks active — Ctrl+Alt+T opens the background sheet",
        )],
        dock: DesignDockFixture {
            kind: DesignDockKind::Background,
            title: "background · 3 tasks",
            hint: Some("Ctrl+Alt+T"),
            rows: vec![
                dock_row(
                    "⠋",
                    "watch:cargo-check",
                    "· running 2m 14s · 0 errors so far",
                    false,
                    false,
                ),
                dock_row("✓", "fmt:rustfmt", "· done 1m 02s · 14 files", false, false),
                dock_row(
                    "!",
                    "read:docs/runtime-jsonrpc.md",
                    "· paused awaiting approval",
                    false,
                    false,
                ),
            ],
        },
        editor: editor("Ask, build, or type / for commands…", false),
        footer: footer("ready", 1, 0, false),
        slash_items: Vec::new(),
        overlay: None,
    }
}

fn frame_todos() -> DesignFrameContent {
    DesignFrameContent {
        title: "oppi-native · todos",
        meta: None,
        header: header("ready", None, false),
        transcript: vec![row(
            "info",
            "info",
            "│",
            "5 todos · 1 in progress · 1 blocked",
        )],
        dock: DesignDockFixture {
            kind: DesignDockKind::Todos,
            title: "todos · 5",
            hint: Some("1 in progress · 1 blocked"),
            rows: vec![
                dock_row(
                    "▶",
                    "resolve Ctrl+P ambiguity (settings vs cycle)",
                    "· in progress · 8m",
                    false,
                    false,
                ),
                dock_row(
                    "!",
                    "verify cargo test on Windows host",
                    "· blocked: no host",
                    false,
                    false,
                ),
                dock_row(
                    "○",
                    "add PageUp/PageDown transcript scroll",
                    "",
                    false,
                    false,
                ),
                dock_row(
                    "○",
                    "middle-truncate cwd in sessions picker",
                    "",
                    false,
                    false,
                ),
                dock_row(
                    "○",
                    "verify footer/header design parity capture",
                    "",
                    false,
                    false,
                ),
            ],
        },
        editor: editor("Ask, build, or type / for commands…", false),
        footer: footer("ready", 5, 0, false),
        slash_items: Vec::new(),
        overlay: None,
    }
}

fn frame_slash() -> DesignFrameContent {
    DesignFrameContent {
        title: "oppi-native · slash palette",
        meta: Some("type to filter"),
        header: header("ready", None, false),
        transcript: vec![row(
            "info",
            "hint",
            "│",
            "type below — palette unfolds upward from the input · ↑/↓ select · Tab inserts · Enter submits",
        )],
        dock: separator("docks: idle"),
        editor: editor("/", false),
        footer: footer("ready", 0, 0, false),
        slash_items: vec![
            slash_item("/settings", "General · Pi · Footer · Memory · Permissions"),
            slash_item("/model", "Select main OPPi model"),
            slash_item("/sessions", "Browse, resume, fork prior sessions"),
            slash_item("/background", "Manage background tasks"),
            slash_item("/todos", "Show or edit the current todo list"),
            slash_item("/effort", "Model-aware thinking slider"),
            slash_item("/exit", "Quit the native shell"),
        ],
        overlay: None,
    }
}

fn frame_settings() -> DesignFrameContent {
    DesignFrameContent {
        title: "oppi-native · /settings",
        meta: Some("overlay"),
        header: header("ready", None, false),
        transcript: vec![row(
            "info",
            "info",
            "│",
            "overlay anchored above editor — ←/→ tabs · ↑/↓ settings · Enter open · Esc close",
        )],
        dock: separator("docks: idle"),
        editor: editor("Ask, build, or type / for commands…", false),
        footer: footer("ready", 0, 0, false),
        slash_items: Vec::new(),
        overlay: Some(DesignOverlayFixture {
            title: "settings",
            help: "←/→ tabs · ↑/↓ settings · Enter open · Esc close",
            selected: 1,
            confirmed: None,
            items: vec![
                overlay_item(
                    "General › Status shortcuts",
                    "/usage",
                    "Usage/status, keybindings, and debug surfaces",
                    false,
                ),
                overlay_item(
                    "General › Goal mode",
                    "none",
                    "Track and continue one thread objective",
                    true,
                ),
                overlay_item(
                    "General › Sessions",
                    "thr_8f4b21",
                    "Browse and resume prior sessions",
                    false,
                ),
                overlay_item(
                    "Pi › Main model",
                    "gpt-5-codex",
                    "Select the default model for OPPi turns",
                    false,
                ),
                overlay_item(
                    "Pi › Effort",
                    "auto",
                    "Model-aware thinking slider for the main model",
                    false,
                ),
                overlay_item(
                    "Pi › Scoped models",
                    "all",
                    "Limit model cycling to selected model patterns",
                    false,
                ),
                overlay_item(
                    "Pi › Role models",
                    "advanced",
                    "Per-task model overrides live here",
                    false,
                ),
                overlay_item(
                    "Pi › Provider",
                    "openai",
                    "Provider status, validation, and base URL",
                    false,
                ),
                overlay_item(
                    "Pi › Login",
                    "subscription/api",
                    "Subscription and API authentication",
                    false,
                ),
                overlay_item(
                    "Footer › Status bar",
                    "live",
                    "Footer help, usage, todos, model, permission, and memory chips",
                    false,
                ),
                overlay_item(
                    "Memory › Hoppi",
                    "client-hosted",
                    "Recall, dashboard, and maintenance",
                    false,
                ),
                overlay_item(
                    "Compaction › Context handoff",
                    "manual",
                    "Manual memory compaction and maintenance shortcuts",
                    false,
                ),
                overlay_item(
                    "Permissions › Mode",
                    "default",
                    "Read/write/network approval policy",
                    false,
                ),
                overlay_item(
                    "Theme › OPPi theme",
                    "oppi (dark)",
                    "Colors and terminal-safe rendering",
                    false,
                ),
            ],
        }),
    }
}

fn frame_narrow() -> DesignFrameContent {
    DesignFrameContent {
        title: "oppi-native · 56 × 22",
        meta: Some("narrow"),
        header: header("ready", Some("gpt-5-codex"), false),
        transcript: vec![
            row("user", "you", "▍", "tighten narrow header"),
            row(
                "assistant",
                "oppi",
                "▍",
                "Plan\n· drop perm + thread\n· middle-truncate model\n· hide hotkey row 2",
            ),
            row("tool", "tool", "│", "✓ grep …/header.rs · 9ms"),
        ],
        dock: separator("idle"),
        editor: editor("…", false),
        footer: footer("ready", 0, 0, true),
        slash_items: Vec::new(),
        overlay: None,
    }
}

fn frame_tiny() -> DesignFrameContent {
    DesignFrameContent {
        title: "oppi-native · 90 × 14",
        meta: Some("tiny height"),
        header: header("running", None, false),
        transcript: vec![
            row("assistant", "oppi", "▍", "executing snapshot tests▍"),
            row("tool", "tool", "│", "⠋ run cargo test --quiet · 3s"),
        ],
        dock: separator("docks: idle"),
        editor: editor("…", true),
        footer: footer("running", 0, 0, false),
        slash_items: Vec::new(),
        overlay: None,
    }
}

fn header(status: &'static str, goal: Option<&'static str>, warn: bool) -> DesignHeaderFixture {
    DesignHeaderFixture { status, goal, warn }
}

fn row(
    kind: &'static str,
    label: &'static str,
    gutter: &'static str,
    body: &'static str,
) -> DesignTranscriptRowFixture {
    DesignTranscriptRowFixture {
        kind,
        label,
        gutter,
        body,
    }
}

fn separator(title: &'static str) -> DesignDockFixture {
    DesignDockFixture {
        kind: DesignDockKind::Separator,
        title,
        hint: None,
        rows: Vec::new(),
    }
}

fn dock_row(
    glyph: &'static str,
    label: &'static str,
    detail: &'static str,
    selected: bool,
    confirmed: bool,
) -> DesignDockRowFixture {
    DesignDockRowFixture {
        glyph,
        label,
        detail,
        selected,
        confirmed,
    }
}

fn editor(placeholder: &'static str, running: bool) -> DesignEditorFixture {
    DesignEditorFixture {
        placeholder,
        running,
    }
}

fn footer(status: &'static str, todos: usize, queued: usize, narrow: bool) -> DesignFooterFixture {
    DesignFooterFixture {
        status,
        todos,
        queued,
        narrow,
    }
}

fn slash_item(command: &'static str, detail: &'static str) -> DesignSlashItemFixture {
    DesignSlashItemFixture { command, detail }
}

fn overlay_item(
    label: &'static str,
    value: &'static str,
    detail: &'static str,
    current: bool,
) -> DesignOverlayItemFixture {
    DesignOverlayItemFixture {
        label,
        value,
        detail,
        current,
    }
}
