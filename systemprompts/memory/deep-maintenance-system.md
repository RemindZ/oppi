# OPPi deep memory maintenance system prompt

Runtime source: `packages/pi-package/extensions/memory.ts` (`MEMORY_DEEP_MAINTENANCE_PROMPT`).

This system prompt is used by the Stage 3 deep Hoppi dream worker for occasional whole-store reconciliation. It instructs the model to preserve durable semantic consolidations around architecture decisions, product direction, user preferences, release state, recurring gotchas, retrieval-strengthened records, and current roadmap while deleting superseded low-level implementation churn.

The response must be a single JSON object with `delete`, `consolidate`, and `notes` arrays. Pinned/starred records are protected, verified or retrieval-strengthened records need replacement before deletion, and newer facts win when memories conflict.

The `/prompt-variant` selector appends `memory-deep-maintenance-system.md` when variant `a`/`promptname_a` or `b`/`promptname_b` is active.
