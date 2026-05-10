//! Terminal lifecycle helpers for the Ratatui UI.
//!
//! Plan 61 keeps the original implementation in `mod.rs` while the renderer is
//! split into focused modules. This module is the landing zone for raw-mode,
//! cleanup, resize, and platform-specific terminal behavior.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TerminalCleanupAction {
    LeaveSynchronizedOutput,
    ResetStyle,
    ShowCursor,
    ClearCurrentLine,
    ClearBelowCursor,
    FinishWithNewline,
}

pub(super) const CLEANUP_ACTIONS: [TerminalCleanupAction; 6] = [
    TerminalCleanupAction::LeaveSynchronizedOutput,
    TerminalCleanupAction::ResetStyle,
    TerminalCleanupAction::ShowCursor,
    TerminalCleanupAction::ClearCurrentLine,
    TerminalCleanupAction::ClearBelowCursor,
    TerminalCleanupAction::FinishWithNewline,
];
