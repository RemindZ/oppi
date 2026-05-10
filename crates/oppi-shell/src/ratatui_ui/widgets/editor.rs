//! Editor widget: 3-row bordered input, prompt gutter, buffer, cursor, running hint.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in super::super) enum EditorCell {
    Border,
    PromptGutter,
    Buffer,
    Cursor,
    RunningHint,
}

pub(in super::super) const EDITOR_CELLS: [EditorCell; 5] = [
    EditorCell::Border,
    EditorCell::PromptGutter,
    EditorCell::Buffer,
    EditorCell::Cursor,
    EditorCell::RunningHint,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in super::super) struct EditorVisualContract {
    pub(in super::super) height: u16,
    pub(in super::super) border_top_left: &'static str,
    pub(in super::super) border_bottom_left: &'static str,
    pub(in super::super) prompt_gutter: &'static str,
    pub(in super::super) cursor: &'static str,
    pub(in super::super) running_title_prefix: &'static str,
}

pub(in super::super) const EDITOR_VISUAL_CONTRACT: EditorVisualContract = EditorVisualContract {
    height: 3,
    border_top_left: "╭",
    border_bottom_left: "╰",
    prompt_gutter: "› ",
    cursor: "█",
    running_title_prefix: " turn running · ",
};
