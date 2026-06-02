# Ratatui design fixtures

These fixtures pin the current native Ratatui design contract. They are no longer a blocked-promotion checklist.

- R1/R2/R3 are immutable target contracts from `.reference/design-v2/tab1-ratatui.jsx`.
- Frames 03-10 are accepted Rust-rendered contracts from the Plan 61/62 target-UI closure. Update them only for intentional product/design changes, and keep `docs/ratatui-design-parity-audit.md` in sync.
- Plan 61 and Plan 62 closed the Ratatui default-promotion gate. A focused fixture failure is a regression or an intentional target change to review, not evidence that the default UI is still experimental.

Run guidance: `cargo test -p oppi-shell ratatui_design` for focused design tests, then `cargo test -p oppi-shell` before broad UI changes.
