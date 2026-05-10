# @oppiai/natives

Optional native-helper probes, benchmarks, and JS fallbacks for OPPi.

This is not the Rust-first end-user launcher. The side-by-side native preview package is `@oppiai/native`, which installs `oppi-native`/`oppi-rs` and bundles `oppi-shell` plus `oppi-server` binaries.

## Stage 4 stance

Rust/N-API is the right final form for low-level helpers when OPPi needs native OS access, reliable PTY/sandbox behavior, or measured performance wins. The `oppi` wrapper itself remains TypeScript/Node for now because it primarily orchestrates Pi, package loading, settings, plugins, and memory.

This package is intentionally conservative:

- no bundled Rust module is required to install OPPi
- JS fallback/probe code always works first
- native code is added only after a benchmark or capability gap justifies it
- Windows install remains sane because there is no mandatory compile step

## API

```ts
import { benchmarkSearch, getNativeStatus } from "@oppiai/natives";

console.log(getNativeStatus());
console.log(await benchmarkSearch({ root: process.cwd(), query: "oppi" }));
```

`getNativeStatus()` checks for a future `oppi_natives.node` module through:

1. `OPPI_NATIVE_MODULE`
2. `dist/oppi_natives.node`
3. `prebuilds/<platform>-<arch>/oppi_natives.node`

If none exists, OPPi reports graceful fallback mode.

`benchmarkSearch()` compares the bounded JS recursive fallback against an external native search baseline (`rg`) when available. It returns a recommendation of `defer-native` or `investigate-native-search`; it is a decision signal, not a production search API. A positive native-search recommendation should feed a focused Rust/N-API design spike once Stage 5 defines the runtime API that would consume it.

## CLI

The main CLI exposes this package through:

```bash
oppi natives status
oppi natives status --json
oppi natives benchmark
oppi natives benchmark --json
```

Use those commands before adding any Rust implementation.

## Candidate future Rust modules

- fast repository search/glob/grep if benchmarked wins are large enough
- PTY/session helpers for the Stage 5 runtime
- clipboard/image/system helpers if terminal/JS behavior is insufficient
- sandbox/permission helpers when OPPi owns a stronger execution boundary

Primary allowed references:

- Oh My Pi native architecture (MIT)
- OpenAI Codex Rust crates for app-server, exec, sandboxing, search, and TUI patterns (Apache-2.0)
