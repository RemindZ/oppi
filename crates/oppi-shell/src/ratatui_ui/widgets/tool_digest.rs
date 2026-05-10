//! ToolDigest widget: glyph, name, target, duration, hint, status color.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in super::super) enum ToolDigestCell {
    Glyph,
    Name,
    Target,
    Duration,
    Hint,
}

pub(in super::super) const TOOL_DIGEST_CELLS: [ToolDigestCell; 5] = [
    ToolDigestCell::Glyph,
    ToolDigestCell::Name,
    ToolDigestCell::Target,
    ToolDigestCell::Duration,
    ToolDigestCell::Hint,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in super::super) struct ToolDigestVisualContract {
    pub(in super::super) separator: &'static str,
    pub(in super::super) success_glyph: &'static str,
    pub(in super::super) denied_glyph: &'static str,
    pub(in super::super) pending_glyph: &'static str,
}

pub(in super::super) const TOOL_DIGEST_VISUAL_CONTRACT: ToolDigestVisualContract =
    ToolDigestVisualContract {
        separator: " · ",
        success_glyph: "✓",
        denied_glyph: "!",
        pending_glyph: "◐",
    };
