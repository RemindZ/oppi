//! Snapshot helpers for terminal-cell goldens.

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SnapshotContext {
    pub(super) start_col: usize,
    pub(super) end_col: usize,
    pub(super) text: String,
}
