# Follow-up chain system append

Runtime source: `packages/pi-package/extensions/enter-routing.ts`.

When the user queues follow-up text while an answer is still running, OPPi appends hidden routing context to the next agent turn. The prompt tells the model to treat the message as part of the same user-visible task, preserve the original ask and follow-up ledger, avoid dumping the ledger verbatim, and provide one combined final answer when the chain is complete.

The `/prompt-variant` selector appends `follow-up-chain-system-append.md` when variant `a`/`promptname_a` or `b`/`promptname_b` is active.
