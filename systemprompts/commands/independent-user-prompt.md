# Independent command user prompt

Runtime source: `packages/pi-package/extensions/independent.ts`.

`/independent @plan.md [scope]` sends the main agent a user prompt instructing it to:

- load and follow the `independent` skill;
- read referenced plan documents completely;
- create and maintain `todo_write` todos;
- continue through implementation, docs, validation, and final reporting;
- ask only for genuine blockers or explicit user/product decisions;
- choose reasonable defaults, validate, and avoid commits/deploys unless authorized.

The `/prompt-variant` selector appends `independent-user-prompt-append.md` when variant `a`/`promptname_a` or `b`/`promptname_b` is active.
