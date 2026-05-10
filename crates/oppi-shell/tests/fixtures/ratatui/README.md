# Ratatui design fixtures

These fixtures pin the Plan 60 acceptance contract for native Ratatui design parity.

- R1/R2/R3 come from `.reference/design-v2/tab1-ratatui.jsx` terminal-grid contracts.
- Frames 03-10 are Rust-rendered fixture contracts while the renderer converges toward exact visual parity.
- Promotion remains gated until these files become exact cell snapshots and pass on Windows/Unix with manual comparison against `.reference/design-v2/index.html`.

Run guidance: `cargo test -p oppi-shell ratatui_design` for focused design tests, then `cargo test -p oppi-shell` before any promotion.
