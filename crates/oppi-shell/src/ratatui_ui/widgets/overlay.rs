//! Overlay widget: settings, sessions, model/role, provider, login, memory panels.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in super::super) enum OverlayPanelKind {
    Settings,
    Theme,
    Permissions,
    Provider,
    Login,
    Memory,
    Sessions,
    ModelRole,
}

pub(in super::super) const OVERLAY_PANEL_KINDS: [OverlayPanelKind; 8] = [
    OverlayPanelKind::Settings,
    OverlayPanelKind::Theme,
    OverlayPanelKind::Permissions,
    OverlayPanelKind::Provider,
    OverlayPanelKind::Login,
    OverlayPanelKind::Memory,
    OverlayPanelKind::Sessions,
    OverlayPanelKind::ModelRole,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in super::super) struct OverlayVisualContract {
    pub(in super::super) title: &'static str,
    pub(in super::super) selected_marker: &'static str,
    pub(in super::super) unselected_marker: &'static str,
    pub(in super::super) label_width: usize,
    pub(in super::super) value_width: usize,
    pub(in super::super) min_width: u16,
    pub(in super::super) max_width: u16,
}

pub(in super::super) const OVERLAY_VISUAL_CONTRACT: OverlayVisualContract = OverlayVisualContract {
    title: " settings ",
    selected_marker: "›",
    unselected_marker: " ",
    label_width: 16,
    value_width: 18,
    min_width: 40,
    max_width: 82,
};
