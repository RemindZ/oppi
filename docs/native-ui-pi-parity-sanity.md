# Native UI Pi-Parity Sanity Pass

Date: 2026-05-27

Scope: sanity check the Rust/Ratatui native OPPi UI against the original Pi-based OPPi behavior and the active native plans, with special attention to model selection, slash command behavior, text input, settings, and formatting.

## Model selector and stale model diagnosis

### What Pi does

Source checked:

- `@mariozechner/pi-coding-agent/docs/models.md`
- `dist/core/model-registry.js`
- `dist/core/model-resolver.js`
- `dist/modes/interactive/components/model-selector.js`
- `dist/core/agent-session.js`

Pi does **not** build `/model` from an ad-hoc historical list. It:

1. refreshes `ModelRegistry` when opening the model selector;
2. loads built-in provider catalogs plus `~/.pi/agent/models.json` custom models/overrides;
3. filters to models with configured auth via `getAvailable()`;
4. supports fuzzy matching against `id`, `name`, and `provider/id`;
5. renders the current model first, then keeps provider/catalog order;
6. uses scoped models only when the session/settings scope is active, with Tab toggling all/scoped.

Pi's current installed defaults are newer than native's old constants:

| Provider | Pi current default |
|---|---|
| openai | `gpt-5.5` |
| openai-codex | `gpt-5.5` |
| github-copilot | `gpt-5.5` |

### What native was doing wrong

Native had stale hardcoded defaults and then amplified them through `known_model_ids`:

- ChatGPT/Codex, Copilot, and OpenAI-compatible login paths used older non-5.5 fallback models.
- model picker and `/model` slash suggestions read from `session.known_model_ids`, a `BTreeSet` populated by prior selections, registered role models, and runtime registrations.

That meant old models could appear as first-class suggestions even when they were only historical/session-local artifacts, and role-model overrides leaked into the main OPPi model picker.

### Native fix in this pass

Native now:

- uses current Pi defaults for native-owned provider paths: OpenAI direct `gpt-5.4`, Codex `gpt-5.5`, Copilot `gpt-5.4`;
- derives `/models`, the main model picker, Ctrl+P cycling, scoped-model listing, Settings -> Scoped models, and `/model ...` slash suggestions from the Rust runtime `model/registry` route or its last rendered registry cache instead of rebuilding from shell-local catalog authority;
- keeps historical `known_model_ids` out of the main selector so stale prior selections and role-only overrides no longer leak in;
- keeps role/per-task model controls in settings/role flows, not in the main selector;
- reloads `models.json` through the runtime-owned registry path, preserving display names, custom models, model override names, auth-aware availability, scoped ordering, selected-model metadata, and visible diagnostics for unsupported provider APIs or malformed registry data;
- matches Pi's interactive model selector ergonomics by opening on scoped models when `enabledModels` resolves entries, rendering `Scope: all | scoped`, and using Tab to switch between runtime-backed all/scoped lists while preserving search.

Remaining gap: native now owns the registry listing path for shell and non-shell clients, including the Pi-style all/scoped picker toggle. It still rejects unsupported custom provider APIs such as `anthropic-messages` and `google-generative-ai` until the Rust provider layer supports them.

## UI/formatting sanity against plans

### Matches / acceptable now

- Header follows the design-reference identity policy: spinner, `OPPi`, provider/model, permission mode, status, and responsive thread/goal context in Ratatui. The retained ANSI fallback uses the same identity string in its header title.
- Main model picker is anchored above the editor/input area via the shared overlay bridge instead of floating as an unrelated mid-screen menu.
- `/model` is now scoped to the main OPPi model; role/per-task selection lives under settings/role model surfaces.
- `/effort` now follows Pi OPPi's current-model thinking slider behavior: model-aware allowed levels, recommended `auto`, aliases, and a docked inline slider above input.
- Slash palette renders live filtered command items instead of static essentials, including empty-state handling.
- Selected rows in slash/settings/dock lists apply selected styling across internal/trailing whitespace.
- Editor placeholder cursor is terminal-style and left-positioned: `› █Ask, build, or type / for commands…`.
- Raw/full-TUI input covers Enter, Shift+Enter, Alt+Enter, Ctrl+Enter, Alt+Up, Escape, Ctrl+C, Ctrl+D, Tab, BackTab, arrows, PgUp/PgDn, Home/End, Backspace/Delete, Ctrl+Backspace, and Alt+Backspace.
- Settings root is no longer a single up/down list; it is grouped with emoji sections matching the Pi-based `/settings:oppi` direction, and Ratatui renders it with a vertical tab rail plus active-section rows.
- Settings root supports type-to-search, filtered navigation, Backspace editing, and a no-match state in retained and Ratatui native UI.
- Settings nested panels route to safe existing commands for theme, permissions, provider, login, model/session, and memory.
- Ratatui design frame gates and live interaction tests cover the major anchored overlays/docks.

### Still short of original Pi-based OPPi

These are not regressions from the latest patch, but they are real parity gaps:

1. **Settings depth** — original Pi OPPi settings had ~63 rows across General, Pi, Footer, Memory, Compaction, Permissions, and Theme. Native currently has the correct grouped shape, but many toggles are not backed by native state yet.
2. **Pi model registry parity** — native now loads Rust-owned built-ins plus `models.json`, applies display/custom/override data, filters auth-aware availability, routes model picker/cycling/scoped listing through the shared registry route/cache, and supports Pi's all/scoped Tab toggle. It still lacks non-OpenAI custom provider execution support.
3. **Scoped model selector** — Pi supports a richer all/scoped toggle inside the selector. Native now has `/scoped-models` list/enable/disable/clear, persisted root `enabledModels`, scoped ordering for cycling, Settings -> Scoped models backed by runtime registry data, and the Tab-driven all/scoped model picker toggle.
4. **Command-specific rich pickers** — native slash autocomplete is live and broad, but some Pi command flows still have richer selector/detail behavior than native.
5. **Terminal capability diagnostics** — Plan 50 still calls out degraded keybinding/color diagnostics as incomplete.
6. **Persistent advanced settings** — footer visibility, permission reviewer/sandbox controls, compaction thresholds, first-pass `/idle-compact status` trigger telemetry, visible idle/smart compaction offers with `/idle-compact compact` follow-through, explicit todo-aware compaction handoff, terminal-safe theme preview, Pi-compatible image settings, shared Hoppi memory behavior/model/idle/install-offer/sync switches, and the explicit Hoppi backend package install/update action now have native backing, and PNG/JPEG/WebP/GIF image artifacts have Kitty/WezTerm plus bounded iTerm2 inline previews when terminal support is advertised; Sixel/richer image capability negotiation and telemetry/changelog/editor/autocomplete options still need real native backing before they should be editable in native settings.

## Gap work-through status

Started immediately after the audit:

| Gap | Status | Native change |
|---|---|---|
| Settings depth | improved | Added General, Footer, Compaction, Theme preview, and Scoped Models surfaces with real backed commands/shortcuts; unsupported Pi-only toggles remain intentionally absent. |
| Pi model registry parity | improved | `model/registry` is now the shared Rust authority for listing built-ins, `models.json` display/custom/override data, auth-aware availability, selected/scoped ordering, and diagnostics. `/models`, Ctrl+L model picker, Ctrl+P cycling, scoped listing, and scoped settings consume that route or its runtime cache, and Ctrl+L exposes Pi's all/scoped Tab toggle. Remaining gap is unsupported non-OpenAI custom provider APIs. |
| Settings search | fixed | Settings root now supports type-to-search, filtered navigation, Backspace editing, and no-match state. |
| Scoped model selector | fixed for picker/list parity | Added `/scoped-models` list/enable/disable/clear, persisted to Pi-compatible `enabledModels`, plus Settings -> Pi -> Scoped models; list/cycle/settings candidates now come from the runtime registry path/cache, and the model picker starts in scoped mode with Tab all/scoped switching when scoped models exist. |
| Command-specific rich pickers | improved | Added visible `/usage`/`/stats`, `/suggest-next` show/accept/clear/debug, and made `/keys`/`/debug` visible slash commands. |
| Terminal capability diagnostics | fixed for discoverability | Existing `/keys` terminal/keybinding diagnostics are now slash-visible and reachable from Settings → General/Footer. |
| Persistent advanced settings | improved | Permission mode changes now persist to settings (`oppi.permissionMode`) with visible source/precedence; `/effort` persists `oppi.reasoningEffort`; scoped model settings persist to root `enabledModels`; footer visibility persists under `oppi.footer`; compaction threshold controls persist under `oppi.idleCompact` / `oppi.smartCompact`; `/idle-compact status` reports context-window-aware trigger telemetry when available; idle shell loops now print visible smart/idle compact offers that point to backed `/idle-compact compact`; `/theme preview [name]` now backs the Theme preview picker row; `/images` persists Pi-compatible `oppi.images` settings; `/memory` persists shared `oppi.memory` behavior/model/idle/install-offer/sync switches and exposes an approval-gated Hoppi backend install/update action; all are exposed in retained/Ratatui settings. Other advanced Pi toggles still require native backing before editable UI. |

## Plan alignment

- Plan 61/62 visual and interaction work is still valid: Ratatui frame parity, anchored overlays, live adapters, and default UI promotion remain covered by tests.
- Plan 50 remains the honest default-promotion source of truth. The native path is usable and promoted for dogfood, but full replacement quality still depends on remaining settings/model-registry/security/dogfood items.
- This pass closes the immediate stale model-list issue and documents the remaining Pi parity gaps instead of pretending unsupported Pi-only toggles are solved.
