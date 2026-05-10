//! Render-neutral Ratatui state model.
//!
//! This module separates fixture-only design frames from live production modes.
//! The legacy renderer still accepts `RatatuiFrameMode`; adapters in `mod.rs`
//! bridge these enums until the widget split is complete.

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DesignFrameKind {
    Idle,
    Running,
    ToolsArtifactDenial,
    AskUser,
    Background,
    Todos,
    Slash,
    Settings,
    Narrow,
    Tiny,
}

#[cfg(test)]
impl DesignFrameKind {
    pub(super) const ALL: [DesignFrameKind; 10] = [
        DesignFrameKind::Idle,
        DesignFrameKind::Running,
        DesignFrameKind::ToolsArtifactDenial,
        DesignFrameKind::AskUser,
        DesignFrameKind::Background,
        DesignFrameKind::Todos,
        DesignFrameKind::Slash,
        DesignFrameKind::Settings,
        DesignFrameKind::Narrow,
        DesignFrameKind::Tiny,
    ];
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LiveRatatuiMode {
    Idle,
    Running,
    Question,
    Approval,
    Background,
    Todos,
    Suggestion,
    Slash,
    Settings,
}
