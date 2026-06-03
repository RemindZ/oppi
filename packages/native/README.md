# @oppiai/native

Rust-first OPPi native UI package. It installs side-by-side with the legacy Pi-backed package:

```bash
npm install -g @oppiai/cli      # legacy package; installs `oppi`
npm install -g @oppiai/native   # Rust/Ratatui native UI; installs `oppi-native` and `oppi-rs`
```

`@oppiai/native` intentionally does **not** claim the `oppi` binary in side-by-side installs. The startup script and native package now treat Rust/Ratatui as the proper native UI path; package ownership of the bare `oppi` bin remains a separate publish/handoff decision.

## Commands

```bash
oppi-native --version
oppi-native doctor --json
oppi-native smoke --json
oppi-native                 # launches the Ratatui native UI
oppi-native --mock          # launches the Ratatui native UI with the mock provider
oppi-native --no-tui --mock # line-mode fallback for pipes/debugging
oppi-native server --stdio  # runs the bundled server over stdio
```

`oppi-rs` is an alias for `oppi-native`.

## Management bridge commands

`oppi-native` owns Rust shell/server launch directly, and delegates existing management commands to `@oppiai/cli` so native-only users do not accidentally send management words to the model:

```bash
oppi-native plugin list --json
oppi-native marketplace list --json
oppi-native mem status
oppi-native sandbox status --json
oppi-native runtime-worker smoke --json
```

The bridge preserves the stable OPPi plugin stores (`OPPI_HOME/plugin-lock.json` and project `.oppi/plugins.json`) and marketplace registry. Rust-native runtime loading covers enabled, trusted local or materialized plugin skills, prompt templates, and themes, and thread-aware `plugin/list` reports source type, materialization, executable-boundary, resource, and diagnostic metadata for configured plugins. `oppi-native plugin materialize ...` delegates to the stable CLI management bridge; package copy/cache mutation stays out of the runtime JSON-RPC server. Executable plugin code is not Pi `-e` loading: native execution requires an explicit `jsonRpcSubprocess` runner manifest, project `.oppi/plugin-runners.json` enable/trust, and the Rust sandbox/Guardian tool path. Pi extensions, app-server handlers, remote/non-local fetchers, and implicit native code execution remain tracked in `.oppi-plans/61-rust-native-plan-gap-audit.md`.

## Binary resolution

Normal npm installs should not require users to build Rust locally. Published tarballs include platform binaries under:

```text
bin/<platform>-<arch>/oppi-shell[.exe]
bin/<platform>-<arch>/oppi-server[.exe]
```

The launcher resolves binaries in this order:

1. explicit env overrides: `OPPI_NATIVE_SHELL_BIN` / `OPPI_NATIVE_SERVER_BIN` (or legacy `OPPI_SHELL_BIN` / `OPPI_SERVER_BIN`)
2. `OPPI_NATIVE_BIN_DIR`
3. package-bundled `bin/<platform>-<arch>/` or `prebuilds/<platform>-<arch>/`
4. separately installed platform packages such as `@oppiai/native-win32-x64` if present
5. monorepo development builds under `target/debug` or `target/release`

For local development before packaging binaries:

```powershell
.\scripts\rust-dev-startup.ps1              # build Rust, then open the proper Ratatui native UI
.\scripts\rust-dev-startup.ps1 --no-build   # reuse existing builds, then open the Ratatui native UI
```

The dev startup script is intentionally only the proper Ratatui native UI launcher. The legacy Pi-powered UI remains available by running `packages/cli/dist/main.js` directly, but the Rust/Ratatui path is now the native daily-driver target.

Provider auth:

- `/login subscription codex` opens browser OAuth for ChatGPT/Codex, stores tokens in the protected OPPi/Pi `auth.json`, and configures the native `openai-codex` provider without printing raw tokens.
- `/login api openai env <ENV>` configures an OpenAI-compatible API provider by env-reference only.
- `/login subscription copilot [--enterprise <domain>]` uses Pi's GitHub device-code OAuth flow, stores tokens in the protected auth store, and configures the native `github-copilot` provider.
- `/login subscription claude` uses the explicit Meridian bridge flow over Claude Code SDK auth; installs/starts are never hidden, and `/login subscription claude login` is the explicit `claude login` step when needed.

Memory backend status:

- `OPPI_MEMORY_BACKEND=auto` or unset keeps the bounded Node/TypeScript Hoppi adapter as the default when available.
- `OPPI_MEMORY_BACKEND=node` forces that Node/TypeScript adapter.
- `OPPI_MEMORY_BACKEND=embedded` forces the Rust embedded JSON/lexical adapter. Embedded currently supports status, remember, recall, list, forget, pin/unpin, update, and supersede, but dashboard UI, embeddings, remote sync, richer search, and deeper model maintenance remain follow-up work.
- Completed native direct-provider turns now call runtime `memory/distill-turn` when Memory and turn summaries are enabled, using the provider that ran or resumed the turn. The route returns a visible `jobId`, honors `memory/jobs/cancel` for late terminal updates, and treats resume as retrying the same bounded turn text as a new job. Mock/no-model turns keep the bounded `memory/backend/remember` summary fallback.

Manual equivalent:

```bash
cargo build -p oppi-server -p oppi-shell
pnpm --filter @oppiai/native build
node packages/native/dist/main.js doctor
node packages/native/dist/main.js smoke --json
```

For publish/pack preparation, build the release binaries and verify the npm dry-run pack list before publishing:

```bash
pnpm --filter @oppiai/native build:binaries
pnpm --filter @oppiai/native verify:pack
npm pack --workspace packages/native --dry-run --ignore-scripts
```

`verify:pack` checks that the dry-run tarball file list includes `dist/main.js`, `dist/main.d.ts`, `README.md`, `package.json`, `bin/<platform>-<arch>/manifest.json`, `oppi-server[.exe]`, and `oppi-shell[.exe]` for the current platform. The package has no `postinstall` compile step.

## Relationship to other packages

- `@oppiai/cli` remains the stable Pi-backed `oppi` package.
- `@oppiai/native` is the Rust-first preview launcher.
- `@oppiai/natives` is a separate optional Node-loadable native-helper/probe package, not the end-user Rust shell launcher.
