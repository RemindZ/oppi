# Native TUI style and rendering brief

This brief is the design reference for making the Rust-native OPPi shell feel like a polished, Pi-compatible coding TUI while staying honest about its current preview status. It is a product/design spec, not an implementation patch.

## Scope

Target surface:

- `crates/oppi-shell/src/tui.rs` — retained terminal UI loop, renderer, overlays, editor, slash palette, and frame layout.
- `crates/oppi-shell/src/main.rs` — session state, event rendering, semantic dock state, theming tokens, line-mode fallback, and command behavior.
- `packages/native/README.md` — native package/user-facing status.
- `docs/runtime-jsonrpc.md` — Rust server/client contract and event vocabulary used by the TUI.

Design references audited before this brief:

- Pi TUI docs: `docs/tui.md`, `docs/themes.md`, and `docs/keybindings.md` from the installed `@mariozechner/pi-coding-agent` package.
- Pi interactive components: assistant/user/custom messages, footer, tool execution, bordered loader, and login dialog.
- OPPi Pi package surfaces: docked UI, footer/usage, themes, permissions, todos, suggestions, background tasks, memory, and effort/model controls.
- OPPi themes: `packages/pi-package/themes/oppi-cyan.json` and `packages/pi-package/themes/oppi-cyan-light.json`.

## Product intent

Native OPPi should become a calm, legible, low-flicker coding console that preserves the useful habits users learned in Pi/OPPi:

1. **Transcript first.** The conversation stays readable and does not jump unpredictably.
2. **Input anchored.** The editor, command palettes, and setting panels live at the bottom and push content upward instead of covering it.
3. **State is visible.** Provider, model, permissions, memory, todos, approvals, background work, and queued follow-ups are always inspectable.
4. **Actions are explicit.** Risky provider smoke, login, permissions, and memory maintenance flows show the command or decision being made.
5. **Fallbacks are honest.** The native shell remains a retained TUI preview until it reaches rich Pi parity; `--no-tui` and stable Pi UI remain valid escape hatches.

## Current native TUI architecture

The current retained TUI already has the right spine:

```text
retained_tui_interactive_loop
  ├─ TuiTerminalGuard: raw mode + hide cursor, restore on drop
  ├─ NativeTuiState: editor, overlay, spinner, slash-palette selection
  ├─ session.sync_ui_docks(): Rust session state -> semantic dock lines
  ├─ render_native_tui_frame(): full frame as width-safe strings
  ├─ DifferentialRenderer: CSI 2026 synchronized output + changed-tail redraw
  ├─ crossterm event polling: key/resize handling
  └─ session.poll_turn_events_silent(): transcript/dock updates from server events
```

Important implementation facts to preserve:

- Rendering is retained and frame-based, but outputs plain strings rather than a component tree.
- The renderer uses synchronized terminal updates (`CSI ?2026 h/l`) and differential redraw to reduce flicker.
- The shell must restore cursor, colors, and raw mode even on error.
- Interactive terminals default to retained TUI; `--no-tui`, pipes, and non-interactive usage remain line-mode.
- Session, provider, permissions, todos, suggestions, background tasks, approvals, and questions are Rust-owned state, not hidden Pi UI state.

## Target frame anatomy

The TUI should keep this vertical order:

```text
╭─ status/header ─────────────────────────────────────────────╮
│ transcript scrollback                                        │
│ assistant/user/tool/agent/artifact messages                  │
├─ docked panels ──────────────────────────────────────────────┤
│ todos / approvals / questions / background / suggestions     │
├─ overlay or command palette, when active ────────────────────┤
│ settings, sessions, models, login, provider, memory, slash    │
├─ editor ─────────────────────────────────────────────────────┤
│ multi-line input with visible cursor                         │
╰─ footer/status/help ─────────────────────────────────────────╯
```

### Header

Current header content:

- spinner or pause/ready marker
- `OPPi native`
- selected model
- provider label
- permission mode
- thread id

Desired behavior:

- Keep the header single-line and stable across spinner ticks.
- Truncate long thread/model/provider labels from the middle when width is constrained.
- Show warning color only for actionable degraded state: missing provider credentials, sandbox unavailable, protocol mismatch, paused approval/question.
- Avoid filling the header with every setting. It should answer: “what am I talking to and is it safe/ready?”

Recommended priority order at narrow widths:

1. status/spinner
2. product label
3. provider/model
4. permissions
5. shortened thread id

### Transcript

The transcript is the main reading surface. It should render semantic events, not raw logs.

Message styling targets:

- User messages: compact label and plain body.
- Assistant messages: markdown-aware body with subdued label; avoid large boxes around normal prose.
- Tool calls: digest-first rows with status, tool name, target/path/command summary, duration, and artifact/log hint.
- Tool details: expandable later; for now, keep summarized text and file/log paths visible.
- Agent/subagent events: show role/personality, start/complete/block status, and handoff result.
- Artifacts: visible `artifact:` rows with kind, path, MIME/type hint, dimensions when available, and overwrite/provenance notes.
- Errors/denials: red/yellow, concise, and actionable.

Scrolling/retention targets:

- Current `RetainedTui.scrollback` stores strings. Keep this simple path until rich scrolling is implemented.
- Add explicit scroll offset when implementing PageUp/PageDown transcript navigation; do not overload history navigation while focus is in the editor unless a modifier is held.
- Preserve multiline assistant markdown as readable wrapped paragraphs, not one huge truncated line.
- Width-safe rendering must treat ANSI sequences as zero-width and reset colors after truncation.

### Docked panels

OPPi’s Pi package established docked panels above the editor. Native should keep the same mental model.

Current dock sources:

- active todos
- pending approval
- pending `ask_user` question
- background task summary
- suggested next message
- footer/status line from session state

Rendering targets:

- Docks should be sparse when idle: one dim `docks: idle` separator is enough.
- Active docks should use stable labels: `todos`, `approval`, `question`, `background`, `suggestion`, `status`.
- Long content should wrap or truncate after preserving the action token (`/approve`, `/deny`, `/answer`, `/background read`, etc.).
- Approvals and protected-file denials deserve a stronger visual treatment than ordinary status docks.
- Todos should show counts plus the current in-progress/blocked item, not the entire backlog.

### Overlays and settings panels

Native overlays currently include:

- top-level settings
- nested settings panels
- session picker
- model/role picker

Settings panel groups should remain:

1. Theme
2. Permissions
3. Model
4. Provider
5. Login
6. Sessions
7. Memory

Design rules:

- Overlays are anchored above the editor and consume only bounded rows.
- `Esc` goes back one level or closes; it should not lose typed editor text unless focus is explicitly in a search field.
- `Enter`/`Space` activate the selected item.
- Search/filter text should be visually distinct from command input.
- Current values use a `●` marker; selected rows use an arrow and accent color.
- Provider smoke stays an explicit inserted command (`/provider smoke ...`) rather than a surprise network action.

### Slash palette

The slash palette should feel like Pi’s command discovery, but fit the Rust shell’s current string renderer.

Current behavior:

- Appears under the editor for slash buffers.
- Shows up to five entries.
- Supports keyboard selection.

Targets:

- Keep the palette below the editor or immediately above the footer; it must not cover transcript text without changing layout reservations.
- Show command label, concise detail, and enough hint text to teach navigation.
- Prefer prefix and fuzzy matching that ranks common OPPi commands first (`/settings`, `/permissions`, `/model`, `/login`, `/memory`, `/todos`, `/background`).
- Suppress the palette after command execution until the next slash edit.

### Editor

The editor is the most important affordance for trust. Current `LineEditor` supports text insertion, cursor movement, history, newline, submit, follow-up, steer, restore queued follow-up, clear, interrupt, and exit.

Required behavior:

- `Enter`: submit normal turn.
- `Shift+Enter`: insert newline.
- `Alt+Enter`: queue/submit follow-up.
- `Ctrl+Enter`: steer active turn.
- `Alt+Up`: restore latest queued follow-up into the editor.
- `Ctrl+C`: interrupt when idle/running; double `Ctrl+C` exits through `/exit`.
- `Ctrl+D`: exit only when the buffer is empty.
- `Esc`: clear buffer or interrupt, depending on state.

Rendering targets:

- Show a visible cursor in the buffer; do not rely on terminal cursor position.
- Preserve multiline input up to a bounded editor height, then scroll the buffer window around the cursor.
- Show busy hint only while a turn is running.
- Keep command-insertion flows safe: settings/provider/login panels may prefill commands into the editor instead of executing risky actions.

### Footer

The footer should be useful but quiet. Stable OPPi/Pi footer behavior includes provider/model, permissions, usage/context, memory, and hotkey help.

Current native footer content:

- status (`ready`, `running`, `waiting`)
- provider
- model
- permissions
- active todo count
- queued follow-up count
- key hints

Targets:

- Use two footer rows only when width allows. On narrow terminals, collapse to one row with essential status.
- Put volatile values left; hotkeys right or on a second dim row.
- Include memory status once Hoppi-native controls are active enough to make it meaningful.
- Avoid repeating data already prominent in an active overlay.

## Theming

Current native tokens are intentionally minimal:

```rust
ThemeTokens {
    accent,
    dim,
    code,
    success,
    warning,
    error,
    reset,
}
```

Keep these as the compatibility floor, then expand toward Pi/OPPi semantic tokens.

Recommended semantic token map:

| Native token | Purpose | Pi/OPPi visual target |
| --- | --- | --- |
| `accent` | selected rows, product label, primary affordance | OPPi cyan/blue |
| `dim` | help, separators, inactive metadata | low-contrast gray |
| `code` | inline code, paths, commands, model ids | magenta/purple or theme code color |
| `success` | completed tools, ready state | green |
| `warning` | missing credentials, pending approval, sandbox degraded | yellow/amber |
| `error` | failures, denials, protocol/auth errors | red |
| `panel` | future: dock/overlay border text | theme surface border |
| `muted_panel` | future: inactive boxes/separators | low-contrast border |

The native theme loader should eventually read the same practical theme concepts as OPPi’s Pi package:

- dark and light modes
- OPPi cyan accent
- no-color/plain fallback
- user-selected theme persisted across sessions
- `.oppi/theme.txt` reload path already used by native shell

Rules:

- Respect `NO_COLOR` and `TERM=dumb`.
- Never encode state only with color; keep markers like `●`, `→`, `!`, `x`, or text labels.
- Always append reset after painted segments.
- Truncation must preserve reset if ANSI was used.

## Rendering quality bar

A native coding TUI is only acceptable if it is boringly stable.

Minimum bar:

- No full-screen flicker on spinner ticks.
- Resize invalidates layout and clears stale tails.
- Width-safe rows in every renderer path.
- Raw mode and hidden cursor restored on panic/error where possible.
- No prompt/editor corruption after tool output, background events, or provider streaming.
- No unbounded output in docks, overlays, or footer.
- Works with `NO_COLOR=1` and dumb terminals through fallback paths.

Known current gaps to resolve before default promotion:

- Transcript is string scrollback, not a semantic virtual list.
- Markdown wrapping is still limited by string truncation in frame rows.
- Overlay focus/search handling is simple.
- The footer and header can repeat or crowd narrow terminals.
- Rich Pi component affordances are approximated, not fully ported.

## Adaptive layout requirements

### Width breakpoints

- `< 60 columns`: essentials only; hide long thread id and second hotkey row.
- `60-90 columns`: normal rows with truncated metadata.
- `> 90 columns`: include richer details such as cwd, fork parent, provider/model detail, artifact hints.

### Height breakpoints

- `< 16 rows`: transcript gets at least 3 rows; docks and overlays must aggressively cap rows.
- `16-28 rows`: standard layout with 3-5 dock/palette rows.
- `> 28 rows`: overlays may show up to 10 rows; transcript gets the rest.

### Reservation order

When space is tight, reserve in this order:

1. header
2. editor
3. footer essentials
4. active overlay
5. active approval/question dock
6. transcript minimum
7. optional todos/background/suggestion docks
8. optional hotkey/help rows

## Interaction model

### Global keys

Recommended global map, consistent with current implementation and Pi habits:

| Key | Behavior |
| --- | --- |
| `Ctrl+L` | open model selector |
| `Ctrl+P` | cycle next model or open settings depending current mode; avoid ambiguity in final design |
| `Ctrl+Shift+P` | previous model |
| `Shift+Tab` | cycle reasoning effort |
| `Esc` | close overlay/back, clear editor, or interrupt by context |
| `Ctrl+C` | interrupt; double exits |
| `Ctrl+D` | exit when editor is empty |

Note: current footer hints show `Ctrl+P` as next model while `LineEditor` also treats `CtrlP` as `OpenSettings` in the line-editor action model. Resolve this inconsistency before default promotion by choosing one global behavior and documenting it in `packages/native/README.md`.

### Command safety

- Login, provider, permission, and memory maintenance flows should prefer visible command insertion when an action has side effects.
- Networked actions, live provider smoke, and account/browser flows must remain user-initiated.
- Protected-file and sandbox decisions should surface in the approval/denial dock, not only in transcript text.

## Specific native panels

### Sessions panel

Keep:

- filter by title, id, path, status, fork parent
- current-session marker
- resume-on-enter
- width-safe rows

Improve:

- middle-truncate long cwd/thread ids
- show fork lineage compactly
- show last activity time when available from server
- support explicit `/resume <id>` from command input for keyboard-only users

### Model/role selector

Keep:

- session model rows
- role override rows
- inherit rows
- current markers
- persisted role profile writes

Improve:

- group sections visually: session model, reviewer, planner, executor, etc.
- show provider source for each model when multiple providers exist
- warn when selected model does not support tool calls, reasoning effort, or streaming

### Provider/Login panels

Keep:

- explicit OpenAI-compatible env-ref setup
- explicit Codex subscription login flow
- explicit Meridian status/install/start/use/stop controls
- `/provider validate` and inserted `/provider smoke` command

Improve:

- show redacted credential source (`env:OPENAI_API_KEY`, `auth-store:codex`, `meridian:local`) without secrets
- show network policy status before smoke actions
- show token refresh failures as actionable rows

### Permissions panel

Keep modes aligned with OPPi policy:

- `read-only`
- `default`
- `auto-review`
- `full-access`

Improve:

- show what each mode changes for file writes, shell execution, background tasks, and network access
- show sandbox availability separately from policy choice
- show session approval cache status and clear action

### Memory panel

Keep native honesty:

- Hoppi controls use the runtime-owned backend broker; the default adapter is still bounded Node/TypeScript until pure Rust Hoppi lands.
- No hidden maintenance sessions.
- Maintenance actions are explicit commands.

Improve:

- show memory enabled/disabled/source status in one row
- expose dashboard/settings/maintenance as navigable subpanels once data is available
- show redacted diagnostics only

## Event-to-UI mapping

Use `docs/runtime-jsonrpc.md` as the source of truth for events. Recommended mapping:

| Runtime event | TUI destination |
| --- | --- |
| `threadStarted`, `threadResumed`, `threadForked` | transcript info row; header thread id update |
| `turnStarted`, phase changes | header spinner/status; transcript only when useful |
| assistant/user item completed | transcript |
| tool call started/completed/failed | transcript digest; approval dock if pending |
| `todosUpdated` | todo dock and transcript only for notable changes |
| `artifactCreated` | transcript artifact row; future artifact shelf |
| approval requested/responded | approval dock + transcript outcome |
| question requested/responded | question dock + transcript outcome |
| background task updates | background dock; transcript on start/finish/fail |
| provider transcript snapshot/replay | transcript only in debug/diagnostic mode |

## Accessibility and terminal compatibility

- All controls must be keyboard-first.
- Do not require mouse support.
- Keep text labels for all colored state.
- Use ASCII-safe fallbacks for heavy box drawing and braille spinners when terminal capability detection says to.
- Make `plain` theme snapshots reliable for tests.
- Avoid rapid animation when no turn is running.

## Testing checklist

Unit/snapshot tests should cover:

- width-safe rendering for header, transcript rows, docks, editor, slash palette, settings, sessions, and models
- ANSI visible-width calculations and truncation reset behavior
- empty, narrow, and tiny terminal frames
- no-color/plain theme output
- overlay navigation and back behavior
- command insertion vs direct command execution for risky settings actions
- `Ctrl+C`, `Ctrl+D`, `Esc`, `Shift+Enter`, `Alt+Enter`, `Ctrl+Enter`, `Alt+Up`
- provider/login panels without credentials
- pending approval/question docks
- background and todo dock updates

Manual smoke should cover:

```powershell
cargo build -p oppi-server -p oppi-shell
cargo test -p oppi-shell
oppi-native --mock
oppi tui smoke --json
```

Interactive manual checks:

- Launch native TUI and resize repeatedly.
- Type multiline input and submit.
- Open `/settings`, all subpanels, `/sessions`, and `/model`.
- Trigger slash palette and select commands.
- Queue a follow-up while a turn is running.
- Run provider validation without leaking credentials.
- Use `NO_COLOR=1` or equivalent and confirm the UI remains readable.

## Default-promotion acceptance criteria

The native shell should not be described as a full stable Pi replacement until all of these are true:

1. No known flicker/editor-corruption bugs in normal local terminals.
2. Stable provider/login/settings flows work without line-mode escape.
3. Permissions, approvals, and sandbox degradation are visible and enforced by Rust/OS policy.
4. Transcript rendering handles markdown, tool digests, artifacts, denials, and background tasks cleanly.
5. Sessions and model/role selection are navigable and persisted as designed.
6. Live provider smoke passes with credential redaction checks.
7. Docs clearly distinguish stable Pi UI, native preview, and fallback commands.

## Near-term redesign backlog

Recommended order for the next focused TUI slices:

1. Resolve `Ctrl+P` ambiguity between settings and model cycling.
2. Add semantic transcript rows before adding rich markdown features.
3. Improve narrow-width header/footer collapse.
4. Add focused overlay/search state so filters do not conflict with editor buffer behavior.
5. Add transcript scroll offset and PageUp/PageDown navigation.
6. Expand theme tokens and map OPPi cyan/dark/light consistently.
7. Add snapshot tests for complete frames in plain/no-color mode.
8. Add provider/login redaction smoke once local Codex credentials are available.

## Non-goals

- Do not fork Pi’s TUI runtime.
- Do not copy proprietary CLI source or private behavior.
- Do not hide provider/network/login actions behind implicit panel activation.
- Do not claim parity while native is still a line/retained preview.
- Do not remove stable Pi UI fallback until default-promotion gates are explicitly met.
