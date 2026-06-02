# promptname_b audit overlay

Normal OPPi audit contract stays binding.

- Prompt/catalog drift is first-class.
- `.temp-audit` holds detail; chat holds decisions, blockers, validation.
- Use `todo_write` for phases, `ask_user` for checkpoints, parallel-safe reads/searches for inventory.
- Preserve user changes. Ask before deps, generated churn, irreversible architecture moves.
