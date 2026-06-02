# OPPi

OPPi is an opinionated, Pi-powered coding agent project.

Goals:

- keep Pi as the upstream agent/runtime kernel, not a fork
- ship a single installable `oppi` command
- provide a polished default Pi package with useful tools, prompts, themes, and workflow extensions
- grow toward a custom terminal harness and VS Code extension without throwing away the Pi-based core

Current version: **0.2.11**.

## Install

OPPi's stable package is published on npm as `@oppiai/cli`; the installed binary is `oppi`.

```bash
npm install -g @oppiai/cli
oppi doctor
oppi
```

The Rust-first native preview is published separately as `@oppiai/native` so both packages can be installed side by side. It installs `oppi-native` and `oppi-rs`, not `oppi`.

```bash
npm install -g @oppiai/native
oppi-native doctor
oppi-native smoke --json
```

Source install for local development:

```bash
git clone https://github.com/RemindZ/oppi.git
cd oppi
pnpm install
pnpm --filter @oppiai/cli build
node packages/cli/dist/main.js doctor
node packages/cli/dist/main.js
```

Release checklist for maintainers:

```bash
pnpm -r --if-present check
pnpm -r --if-present build
pnpm -r --if-present test
pnpm --filter @oppiai/pi-package publish --access public
pnpm --filter @oppiai/cli publish --access public
pnpm --filter @oppiai/native publish --access public   # preview; package release binaries first
```

## Planned package layout

```text
crates/
  oppi-protocol/ # Rust Thread/Turn/Item/Event protocol types
  oppi-core/     # in-memory runtime spine and turn lifecycle skeleton
  oppi-server/   # experimental local JSON-RPC server over stdio
  oppi-sandbox/  # conservative permission/sandbox policy primitives
  oppi-tools/    # tool-call/tool-result invariant helpers
  oppi-agents/   # slash-agent definition/resolution primitives
packages/
  cli/           # installs the stable Pi-backed `oppi` command
  core/          # config, model roles, plugin/memory/artifact services
  native/        # installs Rust-first preview bins `oppi-native` and `oppi-rs`
  pi-package/    # Pi extensions, skills, prompts, and themes
  plugin-sdk/    # plugin/marketplace manifest types and helpers
  natives/       # optional Rust/N-API probe, benchmark, and fallback layer
  tui/           # future custom terminal client
  vscode/        # future VS Code extension
```

## Using the CLI

The `@oppiai/cli` package installs the `oppi` command. It is a thin wrapper around Pi that automatically loads the OPPi Pi package and isolates OPPi state by default.

```bash
oppi --help
oppi doctor
oppi -p "Reply ok"
```

Default behavior:

- launches Pi with `--no-extensions -e <@oppiai/pi-package>` so unrelated global Pi extensions do not conflict
- loads enabled OPPi plugins as additional Pi package sources with `-e <source>`
- stores Pi/OPPi sessions and settings under `~/.oppi/agent`
- honors `OPPI_AGENT_DIR` and `--agent-dir <dir>`
- checks npm at most daily before interactive launches and shows a Pi-style OPPi update banner with `oppi update` and the changelog link when a newer `@oppiai/cli` is available (`OPPI_UPDATE_CHECK=0` disables it)
- passes normal Pi flags/messages through unchanged, including `-p`, `--model`, `--provider`, `--continue`, and `--resume`
- supports `--with-pi-extensions` as an escape hatch for normal Pi extension discovery

Useful commands:

```bash
oppi --version
oppi doctor [--json]
oppi update [--check] [--json]
oppi mem status [--json]
oppi mem install [--json]
oppi mem setup
oppi mem dashboard
oppi natives status
oppi natives benchmark --json
cargo build -p oppi-server
oppi server --stdio --experimental
oppi plugin list
oppi plugin add ./my-pi-package --local
oppi plugin enable my-pi-package --yes
oppi marketplace add ./catalog.json
```

When developing before the bin is linked globally, use `node packages/cli/dist/main.js ...` or `pnpm --filter @oppiai/cli start <args>`, for example `pnpm --filter @oppiai/cli start --version`.

## Plugins and marketplaces

Stage 3 plugin support lets OPPi manage Pi-compatible packages without handing control to Pi's global settings.

```bash
oppi plugin add ./plugins/my-plugin --local   # records disabled by default
oppi plugin doctor my-plugin                  # show source/capability/risk notes
oppi plugin enable my-plugin --yes            # explicit trust gate
oppi plugin list --json
```

Plugin state:

- global: `~/.oppi/plugin-lock.json` or `OPPI_HOME/plugin-lock.json`
- project-local: `.oppi/plugins.json` when `--local` is used
- launch: enabled plugins are appended after OPPi's package as extra `-e <source>` Pi package sources

Marketplace catalogs can be local JSON files or URLs:

```bash
oppi marketplace add ./catalog.json
oppi marketplace list
oppi plugin add demo-from-catalog --enable --yes
```

Catalog shape:

```json
{
  "name": "local-dev",
  "plugins": [
    { "name": "demo-from-catalog", "source": "./plugins/demo", "description": "Demo Pi package" }
  ]
}
```

Claude marketplace compatibility is a guardrail, not a full adapter yet. OPPi accepts Claude-style catalogs only when entries identify a Pi/OPPi-compatible package source. If an entry looks Claude-specific (MCP server config, hooks, agents, slash commands, etc.) and cannot be loaded safely, `oppi plugin add <name>` reports the incompatibility and prints a copy/paste agent handoff prompt to port it into `.oppi/plugins/<name>`.

## Experimental Rust runtime spine

Stage 5 starts the gradual move toward an OPPi-owned Rust runtime. The current slice adds protocol, core, server, sandbox, tools, and agents crates with an app-server model for threads, turns, items, events, approvals/questions, plugin/memory/todo/MCP/model registries, agent runs/worktrees, side questions, handoff compaction, Pi bridge events, an experimental Rust-owned agentic loop, and a direct OpenAI-compatible provider worker path with streaming chunks, compact transcript seeding, function-tool continuations, Rust-owned text file tools, approval resume, runtime-owned Hoppi backend routes for native shell memory operations, prompt A/B variant overlays, Rust-owned `todo_write` state, host-supplied follow-up chain prompt routing, and an opt-in product dogfood command. The stable CLI still launches Pi as the primary runtime; OPPi now defaults to `default-with-fallback` mirroring, so normal Pi turns are mirrored into Rust when `oppi-server` is available and fall back visibly to Pi when it is not. `/runtime-loop` and `oppi runtime-loop smoke` remain the manual mirror dogfood/diagnostic paths; `oppi runtime-worker smoke` proves Rust can call a provider directly, execute a provider-requested Rust tool, stream the final response, and continue the provider turn through a local mock without external credentials. `oppi runtime-worker <prompt>` is the opt-in direct-worker default-candidate path for credentialed dogfood; its legacy TypeScript wrapper still owns the optional Hoppi bridge until that dogfood path is migrated or retired, while the native shell uses `memory/backend/*` JSON-RPC routes for bounded recall/write/list/dashboard/mutation/maintenance/sync execution. `--prompt-variant off|a|b|caveman` or `OPPI_SYSTEM_PROMPT_VARIANT` selects the same normal-vs-caveman prompt experiment used by Pi. If provider auth or the Rust server is unavailable, it prints the stable Pi fallback command instead of hiding the gap.

```bash
cargo test
cargo build -p oppi-server
oppi runtime-loop smoke --json
oppi runtime-worker smoke --json
oppi runtime-worker "summarize this repository" --json
oppi runtime-worker "summarize this repository" --prompt-variant caveman
oppi runtime-worker "summarize this repository" --no-memory
cargo build -p oppi-server -p oppi-shell
oppi tui --experimental
oppi tui smoke --json
oppi resume <thread-id>
oppi resume --json
oppi server --stdio --experimental
```

The native Ratatui shell is available through `oppi tui` when `oppi-shell` and `oppi-server` are built or configured with `OPPI_SHELL_BIN`/`OPPI_SERVER_BIN`; `oppi tui smoke --json` runs the live provider smoke by default, while `--mock` is explicit local scripted mode. Exiting the native shell with `/exit` or double Ctrl+C prints a final `oppi resume <thread-id>` command; `oppi resume <thread-id>` resumes directly, and bare `oppi resume` lists recent sessions for the current project. The side-by-side `@oppiai/native` package exposes the same Rust-first path as `oppi-native`/`oppi-rs` with packaged platform binaries. The repo dogfood startup script now launches the Rust Ratatui UI by default; pass `--pi-ui`/`--stable-ui` to `scripts/rust-dev-startup.ps1` to use the stable Pi fallback.

The stdio server reads one JSON-RPC request per line and emits one response per line. See [`docs/runtime-jsonrpc.md`](docs/runtime-jsonrpc.md) for the experimental contract, protocol negotiation, auth requirements, runtime-loop modes (`default-with-fallback`, `command`, `off`), direct-provider config shape, smoke scenarios, method map, and stable runtime error taxonomy. Methods include `initialize`, `thread/start`, `thread/resume`, `thread/fork`, `thread/items`, `thread/item-labels/set`, `turn/start`, `turn/run-agentic`, `turn/resume-agentic`, `turn/steer`, `turn/interrupt`, `tool/record`, `tool/batch`, `approval/request|respond`, `question/request|respond`, `plugin/register|list`, `memory/status|set|compact|jobs/*|backend/*|record/*`, `todos/list`, `skills/list|expand`, `prompt-template/list|expand`, `theme/list|load`, `mcp/register|list|action|reload|test|auth`, `model/register|list|select`, `agents/register|list|dispatch|block|complete`, `side-question/ask`, `pi/bridge-event`, and `events/list`. `turn/run-agentic` can now use `modelProvider.kind = "openai-compatible"` with allowlisted API-key env refs or `"openai-codex"` with protected ChatGPT/Codex OAuth credentials from the OPPi/Pi auth store, optional `stream: true`, compact runtime history, host-supplied `followUp` chain context, turn-level `sandboxPolicy`, redacted provider diagnostics, and Rust-advertised function tools. Built-ins include `echo`, `todo_write`, `shell_exec`, `read_file`, `search_files`, `write_file`, and `edit_file`; `todo_write` updates Rust `TodoState` and emits `todosUpdated`, while write/edit require approval, execute through the Rust tool path, and continue with tool/function-result messages until the final response. Native shell Hoppi memory operations now call runtime-owned `memory/backend/*` routes; the server-owned bounded Hoppi adapter reports visible memory jobs, enforces timeout failure, preserves cancellation terminal state, and normalizes outputs through `memory/*`. The legacy TypeScript `oppi runtime-worker` dogfood command still has its optional CLI-hosted Hoppi bridge; `--no-memory` or `OPPI_RUNTIME_WORKER_MEMORY=off` disables that path until it is migrated or retired. `oppi doctor` reports Rust-loop mirroring, direct-worker readiness, native shell readiness, protocol/sandbox status, provider readiness, and terminal capability hints separately. `tool/batch` models ordered parallel tool batches by grouping adjacent concurrency-safe calls while keeping unsafe calls exclusive.

Current hardening guarantees: separate `oppi-server` processes keep independent in-memory runtimes; turn, agent-run, approval, and question operations are rejected when addressed from the wrong thread; failed tool validation does not leave partial turns/batches or running agentic turns; approval-gated tools cannot report successful completion before approval; the runtime-loop smoke dry-runs bridge, approval/resume, cancellation, pairing-error, guard-abort, and long-compaction paths; the runtime-worker smoke proves streamed direct provider HTTP calls, Rust tool execution, provider continuation, and redacted debug output flow through Rust's 11-phase path; project/workspace safety starts at the sandbox policy layer through writable-root, protected-path, and disabled-network checks. Future persistent storage must keep the same model with per-project/runtime namespaces and storage paths.

## Native helpers

Stage 4 adds `@oppiai/natives` as an optional Rust/N-API-ready package. This is separate from the Rust runtime spine: `packages/natives` is a Node-loadable helper lane, while `crates/oppi-*` are the future runtime/server/sandbox lane.

```bash
oppi natives status
oppi natives benchmark --json
```

Today this package provides native-module probing, JS fallback status, and a bounded search benchmark that compares JS recursion with an external native baseline (`rg`) when available. If the benchmark recommends native search, treat that as input for a focused Rust/N-API design spike; OPPi intentionally ships no mandatory Rust module yet, keeping Windows installs sane while preserving a clean place for future search, PTY, clipboard/image, terminal, and sandbox helpers.

## Development notes

Local planning docs live in `.oppi-plans/` and are intentionally gitignored. If you are an AI agent working in this repository and the directory exists, read `.oppi-plans/INDEX.md` before making architectural changes.

Prompt catalog lives in `systemprompts/`; update it whenever runtime prompts change. Local allowed reference clones live under `.reference/` and are ignored except for `.reference/README.md` and `.reference/manifest.json`.

## OPPi Pi package features

The current Pi package adds:

- `/effort` Claude Code-style slider for the current model, plus direct thinking levels (`off`, `minimal`, `low`, `medium`, `high`, `xhigh`, `auto`)
- `image_gen` for Codex-native image generation with OpenAI Images API fallback
- `/review` for Codex-style prioritized code review prompts
- `/init` for AGENTS.md contributor-guide generation
- `/agents` to browse built-in personalities (`general-purpose`, `Explore`, `Plan`, `oppi-code-guide`, `statusline-setup`, `verification`) and create/edit/delete OPPi agent definition markdown files under `.oppi/agents` or the personal OPPi agent dir, with read-only compatibility listing for `.claude/agents`; this manages definitions while full AgentTool dispatch remains Rust-runtime work
- `/independent @plan.md` for a reusable plan-runner mode that reads a plan document, keeps `todo_write` active, asks only blocking clarification questions, and continues through implementation plus validation
- `todo_write` plus `/todos` for visible multi-step task tracking; OPPi maintains the list proactively, docks active todos directly above the input, and prunes completed items once their outcomes have been reported or archived
- `ask_user` for batched structured clarification questions
- `suggest_next_message` for high-confidence ghost next-message suggestions; when shown, `Enter` sends it, `→` accepts it into the editor, and typing replaces it
- `/prompt-variant` for A/B testing system-prompt overlays (`a`/`promptname_a`, `b`/`promptname_b`/`caveman`, or `off`), with `OPPI_SYSTEM_PROMPT_VARIANT` for non-interactive runs
- `/permissions` with `read-only`, `default`, `auto-review`, and `full-access` modes; `auto-review` is the OPPi default, avoids Meridian/Claude Code-backed reviewer sessions, records review history, and color-codes risk/authorization in the tool UI
- cyan themes selectable with `/theme` or the docked live-preview `/themes` picker: `oppi-cyan` dark mode and `oppi-cyan-light` light mode
- `shell_exec` and `shell_task` for bounded Bash/PowerShell commands with cwd tracking, timeouts, background tasks, output logs, and model guidance to prefer dedicated file tools for ordinary reads/edits
- collapsible, digest-first tool rendering with one-line recaps, grouped completed read/search/list/shell batches, and quieter hidden-thinking placeholders before tool-only turns
- `/usage` for unified model/subscription/context usage across connected non-Claude providers, including live ChatGPT/Codex buckets, plus `/stats` as an OPPi alias
- a configurable custom footer below the chat input with session/weekly usage display levels, selected-model, permission, Hoppi `mem:*`, context usage, and a toggleable second hotkey-help bar (`Alt+K`; `Ctrl+Alt+K` remains an alias)
- OPPi defaults for Pi settings: steering mode `all`, follow-up mode `all`, and collapsed changelog
- todo-aware scoped compaction: during long todo-driven runs, OPPi evaluates context after `todo_write` checkpoints; by default at 65% context it compacts around remaining todos, stores completed todo outcomes in compaction metadata, and carries them into the final response alongside post-compaction work/validation. After that archive point, future todo updates can prune completed items from the visible list. This is separate from idle compaction.
- optional Meridian integration for using a Claude subscription through the official Claude Code SDK bridge (`/meridian start|stop|status`, provider `meridian`)
- docked command panels: selection/input/custom command UIs render directly above the text input and push chat content upward instead of floating over it
- `/settings:oppi` opens the unified OPPi settings panel for General, Footer, Memory, Compaction, Permissions, and Theme; Stage 1 uses this namespaced command until the OPPi wrapper can own `/settings`
- `/exit` shuts down the current OPPi session gracefully, allowing Hoppi memory recaps and exit sync to run when enabled
- `/clear` and `/reset` save/drain Hoppi memory handoff state, then start a fresh visible conversation session
- `/oppi-terminal-setup` installs VS Code/Cursor terminal forwarding for Shift+Enter, Ctrl+Enter, and Alt+Up
- normal terminal mouse selection/copy behavior is preserved; use Shift+Enter for newlines and Alt+Up to edit queued messages
- feedback intake commands/tools: `/bug-report`, `/feature-request`, and `oppi_feedback_submit`
- a `mermaid-diagrams` skill plus `render_mermaid` tool for concise Mermaid diagrams and terminal ASCII previews

## Claude subscription via Meridian

OPPi does not scrape Claude's web usage endpoints. For Claude subscription access, the Pi package registers a `meridian` provider that points at a local [Meridian](https://github.com/rynfar/meridian) server.

```bash
# one-time Claude Code auth
claude login

# from OPPi interactive mode
/meridian start
/model meridian/claude-sonnet-4-6
```

You can also run Meridian externally and let OPPi connect to it:

```bash
meridian
```

To keep global OPPi installs Windows-safe, Meridian is not installed as a hard npm dependency. `/meridian start` uses an embedded/dev package when available, then a `meridian` command on PATH, then an `npx @rynfar/meridian` fallback.

Configuration:

- `OPPI_MERIDIAN_BASE_URL` / `MERIDIAN_BASE_URL` — default `http://127.0.0.1:3456`
- `OPPI_MERIDIAN_API_KEY` / `MERIDIAN_API_KEY` — optional when Meridian auth is enabled
- `OPPI_MERIDIAN_PROFILE` / `MERIDIAN_DEFAULT_PROFILE` — optional Meridian profile selection

## OPPi settings

Newline entry uses Pi's normal editor behavior: press `Shift+Enter`. If your terminal does not forward Shift+Enter, type a trailing `\` before Enter to insert a newline.

Message routing defaults:

- `Enter` sends normally while idle and queues a follow-up while the agent is busy.
- `Ctrl+Enter` uses Pi's normal submit path, which steers while the agent is busy.
- `Alt+Enter` also queues a follow-up.
- Native `/effort off|minimal|low|medium|high|xhigh|auto` persists Pi-compatible `defaultThinkingLevel` for reasoning-capable models. The root Pi setting wins over the legacy `oppi.reasoningEffort` mirror, while an explicit one-shot CLI `--effort` still wins for that launch.
- Native `/follow-up mode all|one-at-a-time` persists the Pi-compatible `followUpMode`; `all` groups the current queue into one native turn, while `one-at-a-time` keeps queued prompts as separate turns.
- Native `/steer mode all|one-at-a-time` persists the Pi-compatible `steeringMode`; `all` sends every steering message to the mutable turn, while `one-at-a-time` sends the first steering message immediately and queues later ones as separate native turns.
- Native `/startup quiet on|off` persists Pi-compatible `quietStartup`; quiet mode suppresses the non-critical Rust shell ready banner but keeps important interactive prompts visible.
- Native `/skills commands on|off` persists Pi-compatible `enableSkillCommands`; when enabled, the slash palette suggests active `/skill:name` commands from Rust `skills/list`, and `/skill:name [args]` expands the active skill body through Rust `skills/expand` before starting or queueing the turn.
- Native `/autocomplete items <3..20>` persists Pi-compatible `autocompleteMaxVisible`; retained and Ratatui slash palettes use that count for visible command suggestions.
- Native `/cursor hardware on|off` persists Pi-compatible `showHardwareCursor`; when enabled, native TUI loops leave the terminal hardware cursor visible for IME-oriented terminals.
- Native `/editor padding <0|1|2|3>` persists Pi-compatible `editorPaddingX`; retained and Ratatui editors use it as horizontal input padding.
- Native `/editor double-escape tree|fork|none` persists Pi-compatible `doubleEscapeAction`; idle empty-editor double Escape opens the session tree, prepares `/fork`, or disables the shortcut while active turns still treat Escape as interrupt.
- Native `/tree filter default|no-tools|user-only|labeled-only|all` persists Pi-compatible `treeFilterMode`; native session-tree views use it to control archived/all/titled-session visibility, and `/tree entries [after <event-id>] [limit <1..5000>]` applies the same filter to completed transcript items from the active thread while carrying runtime event anchors, filter-relative parent/child anchors, completion timestamps, item labels, and next-cursor hints for future Pi-style point-in-history navigation. `/tree label <event-id> <label>` and `/tree unlabel <event-id>` update replayable runtime labels that feed `labeled-only`.
- Native `/terminal progress on|off` persists Pi-compatible `terminal.showTerminalProgress`; when enabled, native turns emit OSC 9;4 active/clear progress indicators outside JSON output.
- Native `/terminal clear-on-shrink on|off` persists Pi-compatible `terminal.clearOnShrink`; retained TUI redraws only full-clear after a shrinking frame when that stale-row cleanup toggle is enabled.
- Queued follow-ups are tracked as a chain tied to the original standalone request. Hidden chain context tells the model when a later follow-up should produce the combined final answer for the initial request plus all follow-up tasks, including follow-ups queued while another follow-up is already running.
- `Alt+Up` restores queued follow-up/steering messages into the editor so you can edit them before they are sent. OPPi reserves Alt+Up for this queue restore in the main editor; if it behaves like history-up in a VS Code/Cursor terminal, run `/oppi-terminal-setup`.

In Cursor/VS Code terminals, run this from OPPi to install the Shift+Enter/Ctrl+Enter/Alt+Up forwarding rules automatically:

```text
/oppi-terminal-setup
```

On startup, OPPi detects Cursor/VS Code integrated terminals and offers to install the setup if it is missing. The setup writes:

- `Shift+Enter` → `\u001b[13;2u`
- `Ctrl+Enter` → `\u001b[13;5u`
- `Alt+Up` → `\u001b[1;3A`

Check the setup with:

```text
/oppi-terminal-setup status
```

When launched through `oppi`, OPPi-specific settings live under the `oppi` key in `~/.oppi/agent/settings.json` by default. Direct Stage 1 package launches through raw `pi --no-extensions -e ./packages/pi-package` still use Pi's configured agent directory.

Open `/settings:oppi` for the consolidated OPPi settings surface. The tabs are:

- `⚙️ General` — command/status notes for the Stage 1 settings surface
- `🧭 Footer` — main bottom-bar elements, usage display level (`session + weekly`, `session`, `weekly`, `off`), and the toggleable hotkey-help bar
- `🧠 Memory` — core Hoppi feature toggles, explicit npm install/update for `@oppiai/hoppi-memory`, first-start install-offer control, and a shortcut to the dashboard for detailed memory/sync settings
- `🗜️ Compaction` — idle compaction and todo-aware smart compact thresholds
- `🔐 Permissions` — OPPi permission mode, auto-review timeout, review history, and session cache reset
- `🎨 Theme` — OPPi theme switching and live preview

Pi's built-in `/settings` remains untouched until the OPPi wrapper owns command routing.

On first interactive startup, if Memory is enabled but the Hoppi backend package is missing, OPPi asks whether to install `@oppiai/hoppi-memory` into its managed npm package directory (`~/.oppi/packages`). Choosing No dismisses that first-start offer; the Memory tab can still install/update Hoppi later, and `oppi mem install` provides the same explicit install path from the shell.

Use `/memory` to open the Hoppi dashboard. Detailed Hoppi controls live there: memory CRUD, project scope, stale filtering, private-GitHub sync setup, optional passphrase encryption, manual pull/push/sync, tombstone status, and conflict resolution.

Legacy fallback: `/memory-maintenance [dry-run|apply] [--yes] [--limit N]` runs an explicit cleanup/consolidation pass for the current project store. Automatic dreaming supersedes it when Idle dream mode is enabled. It defaults to GPT-5.4 mini, does not try Claude/Meridian, shows the model ultimately used, and asks before applying unless `--yes` is supplied.

Open `/permissions` to choose tool-call policy from a list or use `/settings:oppi permissions`:

- `read-only` — read/search/list only
- `default` — ask before risky actions
- `auto-review` — isolated Guardian reviewer for risky actions; default; does not prompt directly
- `full-access` — allow most actions, while protected files still require approval

Auto-review decisions are visible in the tool UI with theme-controlled colors:

- risk: low = green, medium = yellow, high/critical = orange/red
- authorization: high = green, medium = yellow, low/unknown = orange/red
- auto-approved, cached, denied, and circuit-blocked calls use distinct themed backgrounds

Useful subcommands:

```text
/permissions history
/permissions status
/permissions clear-session
```

OPPi caches only exact auto-approved calls when risk is low, authorization is medium/high, and no protected path is involved. Similar denied calls trip a session circuit breaker after repeated denials.

Use `/prompt-variant` to A/B test prompt overlays:

```text
/prompt-variant a         # agentic-loop overlay
/prompt-variant b         # Caveman-compressed overlay
/prompt-variant caveman   # alias for b
/prompt-variant off       # normal OPPi/Pi prompt
```

Canonical names `promptname_a` and `promptname_b` still work. Set `OPPI_SYSTEM_PROMPT_VARIANT=a` or `b` for fixed non-interactive benchmark runs.

Use `/settings:oppi compaction` to configure compaction:

- idle compaction: runs only after OPPi has been left unattended for the configured idle period; default 5 minutes at 70% context
- idle enabled/disabled
- idle time: `2`, `5`, or `10` minutes
- idle context threshold: `50`, `60`, `70`, or `80` percent
- smart compact threshold: `50`, `55`, `60`, `65`, `70`, or `75` percent; default `65`

## Feedback intake

OPPi accepts project feedback through its own commands/tools so reports include enough context and sanitized diagnostics:

```text
/bug-report <what went wrong>
/feature-request <what you want OPPi to do>
```

Direct GitHub issues may be closed automatically once the intake workflow is enabled. Comments on existing GitHub issues are welcome.

## Status

Stage 2 thin CLI and Stage 3 plugin management are in place, and Stage 4 native readiness is conservative: `@oppiai/cli` builds a usable `oppi` bin that launches Pi with the OPPi package, isolates agent config under `~/.oppi/agent`, provides `doctor`, safe memory bridge commands, trusted Pi-compatible plugin loading, and optional native probe/benchmark commands. Direct stock-Pi package launch remains useful for debugging:

```bash
pi --no-extensions -e ./packages/pi-package
```
