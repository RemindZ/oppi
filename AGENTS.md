# OPPi Agent Instructions

OPPi is an opinionated coding-agent product built on top of Pi. Treat Pi as the upstream runtime dependency, not a fork.

## Before changing code

- If `.oppi-plans/INDEX.md` exists, read it before non-trivial changes, then read the relevant plan files it points to.
- For Pi API, extension, theme, skill, prompt, SDK, or TUI work, read the relevant Pi docs under the installed `@mariozechner/pi-coding-agent` package before implementing.
- Check `git status --short` and avoid overwriting user changes.

## Source hygiene

- Do not inspect, clone, or copy leaked proprietary source.
- Allowed source-level references: Pi / `pi-mono` (MIT), Oh My Pi (MIT), OpenAI Codex CLI (Apache-2.0), and Caveman (MIT; system-prompt strategy reference).
- Claude-Mem was evaluated and removed as a dead-end integration because its worker can wake Claude Code-backed processing unexpectedly. Do not reintroduce it without an explicit product decision.
- Keep public docs free of private planning notes. Local planning belongs in `.oppi-plans/`, which is gitignored.

## Current repo shape

```text
packages/
  cli/            # thin `oppi` binary wrapper around Pi + OPPi package
  core/           # shared core package scaffold
  intake-worker/  # Cloudflare Worker for feedback intake -> GitHub issues
  pi-package/     # current implementation target: Pi extensions, skills, prompts, themes
  plugin-sdk/     # plugin manifest types/helpers scaffold
  natives/        # future native/Rust strategy placeholder
  tui/            # future custom terminal harness placeholder
  vscode/         # future VS Code extension placeholder
systemprompts/    # prompt catalog; update when runtime prompts change
scripts/          # repo setup scripts, including reference clone setup
```

Stage 1 lives mostly in `packages/pi-package`; Stage 2 lives in `packages/cli`. Prefer reusable Pi extensions, skills, prompts, and themes before adding custom runtime/harness code.

## Current Pi package features

`packages/pi-package/package.json` registers OPPi extensions for:

- OPPi defaults, docked UI, enter routing, terminal setup
- `/prompt-variant` for system-prompt A/B overlays
- `/effort`, `/theme`/`/themes`, usage/status footer, tool digest
- `image_gen`, `/review`, `/init`
- `todo_write` + `/todos`, `ask_user`
- `/permissions` with `read-only`, `default`, `auto-review`, and `full-access`
- smart/idle compaction
- Meridian provider bridge
- feedback commands/tools
- `mermaid-diagrams` skill for concise Mermaid diagrams

## Permissions guidance

- Default OPPi permission mode is `auto-review`.
- OPPi auto-review is an extension-layer Guardian-style reviewer: isolated model session, bounded read-only reviewer tools, strict JSON decision, timeout/fail-closed behavior, visible lifecycle state, conservative exact-call cache, and repeated-denial circuit breaker.
- Compared with Codex CLI, OPPi does not yet have a native sandbox, exec policy, child-process interception, or network policy boundary. Treat Stage 1 permissions as a strong UX/review layer, not a hard security boundary.
- Protected files (`.env*`, `.ssh/`, `*.pem`, `*.key`, `.git/config`, `.git/hooks/`, `.npmrc`, `.pypirc`, `.mcp.json`, `.claude.json`) require special care even in permissive modes.

## Development commands

- Install/check workspace dependencies with pnpm from the repo root.
- Main smoke commands:

```bash
pi --no-extensions -e ./packages/pi-package
pnpm --filter @oppiai/cli build
node packages/cli/dist/main.js doctor
```

- Useful manual checks inside Pi:

```text
/effort
/permissions
/todos
/review
/init
/exit
```

- Run available package checks before finalizing changes:

```bash
pnpm -r --if-present check
pnpm -r --if-present build
pnpm -r --if-present test
```

## UX tone

Keep OPPi concise, polished, professional, and a little playful. Avoid noisy UI, hidden critical errors, and throwaway implementations for native-heavy features.
