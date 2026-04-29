# OPPi Reference Repositories

This directory is for local source-level reference clones that OPPi is allowed to study.

The cloned repositories themselves are ignored by git. Keep only this README and `manifest.json` under version control.

Allowed references from `AGENTS.md` / `.oppi-plans/INDEX.md`:

- Pi / `pi-mono` — MIT
- Oh My Pi — MIT
- Oh My Pi Plugins — MIT
- OpenAI Codex CLI — Apache-2.0
- Caveman — MIT; system-prompt strategy reference

## Expected layout

```text
.reference/
  pi-mono/
  oh-my-pi/
  oh-my-pi-plugins/
  codex/
  caveman/
```

## Refresh

```bash
git -C .reference/pi-mono pull --ff-only
git -C .reference/oh-my-pi pull --ff-only
git -C .reference/oh-my-pi-plugins pull --ff-only
git -C .reference/codex pull --ff-only
git -C .reference/caveman pull --ff-only
```

## Hygiene

Use these repositories for architecture and behavior comparison. Do not inspect, clone, or copy leaked proprietary source. Do not add new source-level references without checking `AGENTS.md` and the planning docs first.
