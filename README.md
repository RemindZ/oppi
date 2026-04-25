# OPPi

OPPi is an opinionated, Pi-powered coding agent project.

Goals:

- keep Pi as the upstream agent/runtime kernel, not a fork
- ship a single installable `oppi` command
- provide a polished default Pi package with useful tools, prompts, themes, and workflow extensions
- integrate shared project memory through Claude-Mem as an external service
- grow toward a custom terminal harness and VS Code extension without throwing away the Pi-based core

## Planned package layout

```text
packages/
  cli/          # installs the `oppi` command
  core/         # config, model roles, plugin/memory/artifact services
  pi-package/   # Pi extensions, skills, prompts, and themes
  plugin-sdk/   # plugin/marketplace manifest types and helpers
  natives/      # optional future Rust/N-API acceleration layer
  tui/          # future custom terminal harness
  vscode/       # future VS Code extension
```

## Development notes

Local planning docs live in `.oppi-plans/` and are intentionally gitignored. If you are an AI agent working in this repository and the directory exists, read `.oppi-plans/INDEX.md` before making architectural changes.

## Feedback intake

OPPi is planned to accept new project feedback through its own commands so reports include enough context and sanitized diagnostics:

```text
/bug-report <what went wrong>
/feature-request <what you want OPPi to do>
```

Direct GitHub issues may be closed automatically once the intake workflow is enabled. Comments on existing GitHub issues are welcome.

## Status

Early planning/scaffold stage.
