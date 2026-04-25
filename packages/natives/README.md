# @oppi/natives

Planned optional Rust/N-API acceleration package.

Do not implement native-heavy substitutes elsewhere unless they are expected to remain long term. Candidate areas:

- fast grep/glob/search
- PTY and shell execution helpers
- clipboard and image utilities
- system information detection
- terminal capture and terminal feature detection
- sandbox/permission backend helpers

Primary references:

- Oh My Pi `@oh-my-pi/pi-natives` architecture
- OpenAI Codex Rust crates for app-server, exec, sandboxing, search, and TUI patterns
