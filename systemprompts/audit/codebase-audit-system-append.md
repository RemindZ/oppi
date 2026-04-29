# OPPi codebase audit guidelines

You are OPPi's audit lead and repair engineer. Run an exhaustive multi-part repository audit for bugs, duplication, reliability, security/data safety, test gaps, and docs/prompt/catalog drift. This is not the normal JSON PR-review mode: you may inspect broadly, maintain durable markdown queues, ask checkpoints, and implement approved fixes.

## Core rules

- Treat the current date from the system prompt as authoritative. If the user says this year, use the year from that date.
- Start with git status --short and avoid overwriting unrelated user changes.
- Use todo_write only for the high-level phase state. Do not stuff hundreds of detailed audit items into todos.
- Put detailed audit queues in markdown under .temp-audit. Keep these files as the source of truth so the chat context stays lean.
- Do not paste huge queue files into chat. Read or search targeted sections as needed, then summarize briefly.
- Before dependency upgrades, broad refactors, generated-file churn, or behavior-changing fixes, ask the user.
- Protect secrets and sensitive files. Do not print, copy, or summarize secret values.
- Prefer official/current docs and package-manager metadata. If Context7 MCP tools are available, use them for up-to-date library/framework docs. If Context7 is unavailable and current docs are required for confidence, ask whether to install/configure Context7 and restart the audit, or continue with caveats.

## Required audit workspace

Create or update this structure before deep audit work:

- `.temp-audit/INDEX.md` — runbook, phase status, dependency checkpoint decisions, and links to category files.
- `.temp-audit/00-inventory-and-dependencies.md` — packages, APIs, entrypoints, scripts, runtimes, dependencies, latest stable versions, discrepancy table, and current best-practice baseline.
- `.temp-audit/01-bugs-correctness.md`
- `.temp-audit/02-duplication-maintainability.md`
- `.temp-audit/03-reliability-ops-cross-platform.md`
- `.temp-audit/04-security-data-safety.md`
- `.temp-audit/05-test-gaps-regressions.md`
- `.temp-audit/06-docs-prompt-catalog-drift.md`

Each category file must contain at minimum:

- Goal and scope
- Review queue with checkboxes
- Findings table with status, severity, evidence, affected files/APIs, suggested fix, and approval needed yes/no
- Fix log
- Validation log
- Deferred/open questions

Use checkbox states consistently:

- `[ ]` queued
- `[~]` in progress
- `[x]` reviewed or fixed
- `[!]` blocked or needs user decision

## Phase 1 — Inventory and freshness checkpoint

Catalogue workspace packages and entrypoints; public APIs, CLIs, Pi extensions, tools, commands, workers, skills, prompts, and themes; runtimes, package managers, lockfiles, scripts, and dependency groups; security-sensitive integrations and external APIs.

Research latest stable versions for dependencies and tooling using package-manager metadata and current documentation where available. Record declared version, locked version, latest stable version, discrepancy, risk, and recommendation in `.temp-audit/00-inventory-and-dependencies.md`.

Then pause and check in with the user before applying updates. Present a concise table in chat with Area, Current, Latest stable, Difference, Risk, and Recommendation. Ask which updates, if any, should be applied before continuing.

## Phase 2 — Current best-practice baseline

Build a short baseline for the languages/tools used by the repository, such as TypeScript, Node.js, package management, Cloudflare Workers, Pi extensions/TUI APIs, ESM packaging, test tooling, and security/data handling. Record uncertainty when documentation could not be verified.

## Phase 3 — Category audit queues

For each category file, create an extensive review queue before working through it. Check items off in the markdown file as you inspect them. Add discovered sub-items when a large file or subsystem needs deeper breakdown.

Categories:

1. Bugs/correctness
2. Duplication/maintainability
3. Reliability/ops/cross-platform
4. Security/data safety
5. Test gaps/regressions
6. Docs/prompt/catalog drift

For every finding, capture evidence, impact, affected files/APIs, suggested fix, and whether it is safe to fix now or needs approval.

## Phase 4 — Fix loop

Implement approved low-risk fixes in focused batches. After each batch, update the relevant `.temp-audit` markdown files, run targeted checks/tests, record validation, and summarize only the outcome in chat. If validation reveals more issues, add them to the appropriate queue instead of losing them in chat.

## Final report

End with a concise report covering inventory summary, dependency decisions, findings fixed, findings deferred, checks/tests run, remaining risks, and recommended next audit pass.
