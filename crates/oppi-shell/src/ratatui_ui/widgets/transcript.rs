//! Transcript widget: typed semantic rows, fixed gutter/label columns, wrapping, bottom anchoring.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in super::super) enum TranscriptColumn {
    Gutter,
    Label,
    Gap,
    Body,
}

pub(in super::super) const TRANSCRIPT_COLUMNS: [TranscriptColumn; 4] = [
    TranscriptColumn::Gutter,
    TranscriptColumn::Label,
    TranscriptColumn::Gap,
    TranscriptColumn::Body,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in super::super) struct TranscriptVisualContract {
    pub(in super::super) gutter_width: usize,
    pub(in super::super) label_width: usize,
    pub(in super::super) gap_width: usize,
    pub(in super::super) user_gutter: &'static str,
    pub(in super::super) assistant_gutter: &'static str,
    pub(in super::super) other_gutter: &'static str,
}

pub(in super::super) const TRANSCRIPT_VISUAL_CONTRACT: TranscriptVisualContract =
    TranscriptVisualContract {
        gutter_width: 2,
        label_width: 8,
        gap_width: 1,
        user_gutter: "▍",
        assistant_gutter: "▍",
        other_gutter: "│",
    };

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in super::super) struct SemanticRowVisualContract {
    pub(in super::super) diff_add_prefix: &'static str,
    pub(in super::super) diff_remove_prefix: &'static str,
    pub(in super::super) artifact_scheme: &'static str,
    pub(in super::super) metadata_separator: &'static str,
    pub(in super::super) denial_prefix: &'static str,
    pub(in super::super) approval_hint: &'static str,
}

pub(in super::super) const SEMANTIC_ROW_VISUAL_CONTRACT: SemanticRowVisualContract =
    SemanticRowVisualContract {
        diff_add_prefix: "+ ",
        diff_remove_prefix: "- ",
        artifact_scheme: "artifact://",
        metadata_separator: " · ",
        denial_prefix: "write blocked:",
        approval_hint: "/approve",
    };
