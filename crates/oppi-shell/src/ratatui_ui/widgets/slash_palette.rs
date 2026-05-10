//! Slash palette widget: bottom-anchored overlay, filter, selected row, command/detail columns.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in super::super) enum SlashPaletteAction {
    MoveUp,
    MoveDown,
    PageUp,
    PageDown,
    Home,
    End,
    InsertSelected,
    SubmitSelected,
    Close,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in super::super) struct SlashPaletteVisualContract {
    pub(in super::super) title: &'static str,
    pub(in super::super) selected_marker: &'static str,
    pub(in super::super) unselected_marker: &'static str,
    pub(in super::super) command_detail_gap: &'static str,
    pub(in super::super) max_items: usize,
    pub(in super::super) empty_text: &'static str,
}

pub(in super::super) const SLASH_PALETTE_VISUAL_CONTRACT: SlashPaletteVisualContract =
    SlashPaletteVisualContract {
        title: " commands ",
        selected_marker: "›",
        unselected_marker: " ",
        command_detail_gap: "  ",
        max_items: 7,
        empty_text: "no commands match",
    };
