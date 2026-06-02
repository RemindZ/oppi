# promptname_a audit overlay

Apply the normal OPPi audit contract. Additionally:

- Treat prompt/catalog drift as a first-class audit category when prompts or runtime instructions changed.
- Keep `.temp-audit` as the detailed state store; chat should carry phase decisions, blockers, and validation only.
- Prefer OPPi-native workflow tools: `todo_write` for phases, `ask_user` at approval checkpoints, and parallel-safe reads/searches for broad inventory.
- Preserve user changes and ask before dependency upgrades, generated churn, or irreversible architecture shifts.
