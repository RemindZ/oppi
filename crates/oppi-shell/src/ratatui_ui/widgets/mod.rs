//! Ratatui widget modules for Plan 61 mock parity.

pub(super) mod dock_tray;
pub(super) mod editor;
pub(super) mod footer;
pub(super) mod header;
pub(super) mod overlay;
pub(super) mod slash_palette;
pub(super) mod tool_digest;
pub(super) mod transcript;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WidgetModule {
    Header,
    Transcript,
    ToolDigest,
    DockTray,
    Editor,
    Footer,
    SlashPalette,
    Overlay,
}

pub(super) const PARITY_WIDGET_MODULES: [WidgetModule; 8] = [
    WidgetModule::Header,
    WidgetModule::Transcript,
    WidgetModule::ToolDigest,
    WidgetModule::DockTray,
    WidgetModule::Editor,
    WidgetModule::Footer,
    WidgetModule::SlashPalette,
    WidgetModule::Overlay,
];
