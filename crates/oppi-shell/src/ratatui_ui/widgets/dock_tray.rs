//! DockTray widgets: separator, question, approval, background, todos, suggestion.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in super::super) enum DockTrayKind {
    Separator,
    Question,
    Approval,
    Background,
    Todos,
    Suggestion,
}

pub(in super::super) const DOCK_TRAY_KINDS: [DockTrayKind; 6] = [
    DockTrayKind::Separator,
    DockTrayKind::Question,
    DockTrayKind::Approval,
    DockTrayKind::Background,
    DockTrayKind::Todos,
    DockTrayKind::Suggestion,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in super::super) struct DockTrayVisualContract {
    pub(in super::super) selected_arrow: &'static str,
    pub(in super::super) unselected_arrow: &'static str,
    pub(in super::super) title_hint_gap: &'static str,
    pub(in super::super) border_bottom: bool,
    pub(in super::super) body_left_padding: usize,
}

pub(in super::super) const DOCK_TRAY_VISUAL_CONTRACT: DockTrayVisualContract =
    DockTrayVisualContract {
        selected_arrow: "› ",
        unselected_arrow: "  ",
        title_hint_gap: "        ",
        border_bottom: false,
        body_left_padding: 1,
    };
