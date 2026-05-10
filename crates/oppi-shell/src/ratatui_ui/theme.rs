//! Theme-token landing zone for the Ratatui UI split.
//!
//! The concrete `RatatuiThemeTokens` type still lives in `mod.rs` during this
//! first split to avoid churn. Future Plan 61 slices should move it here.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ThemeVariant {
    Dark,
    Light,
    Plain,
}
