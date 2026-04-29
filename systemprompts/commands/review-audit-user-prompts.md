# Review audit user prompt templates

These prompts are sent by the `/review` command when the user chooses `Full codebase audit` or types `/review audit [focus]`.

## AUDIT_FULL_PROMPT

Run the full OPPi codebase audit workflow for this repository.

First create or update `.temp-audit/INDEX.md`, `.temp-audit/00-inventory-and-dependencies.md`, and the six category audit files. Catalogue packages, APIs, entrypoints, commands/tools/extensions, dependencies, scripts, runtimes, and current best practices. Research latest stable dependency/tooling versions, then pause with the inventory and discrepancy table before applying updates.

After I approve dependency/tooling decisions, continue through the markdown-backed queues for bugs/correctness, duplication/maintainability, reliability/ops/cross-platform, security/data safety, test gaps/regressions, and docs/prompt/catalog drift. Fix approved issues in focused batches and keep the `.temp-audit` files as the detailed source of truth.

## AUDIT_FOCUS_PROMPT

Run the full OPPi codebase audit workflow with this additional user focus:

{{focus}}

Keep the mandatory `.temp-audit` markdown queue structure, dependency freshness checkpoint, current best-practice baseline, approval gates, and batch fix loop.

## Review picker option

- Full codebase audit
