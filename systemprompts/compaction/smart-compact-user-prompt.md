# Smart compaction user prompt

Runtime source: `packages/pi-package/extensions/smart-compact.ts`.

The smart compactor sends an isolated model prompt asking for a todo-aware scoped summary with exact sections:

- `## Goal`
- `## Remaining Todos`
- `## Completed Todo Outcomes`
- `## Relevant Context For Remaining Work`
- `## File State`
- `## Next Steps`
- `## Final Response Notes`

Core behavior:

- preserve only context needed for pending/in-progress/blocked todos and later final reporting;
- merge completed todo outcomes across compactions;
- preserve concrete requirements, paths, commands, decisions, errors, file state, validation, and blockers;
- remind the next model to combine archived outcomes with post-compaction work in the final response.

The `/prompt-variant` selector appends `smart-compact-user-prompt-append.md` when variant `a`/`promptname_a` or `b`/`promptname_b` is active.
