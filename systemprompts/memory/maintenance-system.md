# OPPi memory maintenance system prompt

Runtime source: `packages/pi-package/extensions/memory.ts` (`MEMORY_MAINTENANCE_PROMPT`).

This system prompt is used by the Stage 2 Hoppi dream worker. It instructs the model to curate project memories after duplicate cleanup by preserving durable decisions, user preferences, gotchas, architecture facts, completed outcomes, retrieval-strengthened records, and current next steps while deleting only clear duplicates, noise, or superseded memories.

The response must be a single JSON object with `delete`, `consolidate`, and `notes` arrays. Pinned/starred records are protected; verified or retrieval-strengthened memories must not be deleted unless replaced by a better consolidation.

The `/prompt-variant` selector appends `memory-maintenance-system.md` when variant `a`/`promptname_a` or `b`/`promptname_b` is active.
