# System prompt experiments

Use this directory for candidate prompt variants.

Recommended naming:

```text
promptname_a/
promptname_b/
YYYY-MM-DD-short-name.append.md
YYYY-MM-DD-short-name.full.md
```

- Variant directories may contain multiple system-prompt surfaces plus a `main-system-append.md` runtime entrypoint.
- `.append.md` files are intended for Pi's `--append-system-prompt` path or OPPi's `/prompt-variant` loader.
- `.full.md` files are full replacements for Pi's default system prompt and should be tested carefully because they replace built-in tool guidance.

Current runtime-selectable variants:

```text
/prompt-variant promptname_a
/prompt-variant promptname_b
/prompt-variant off
```

`promptname_a` is based on the authorized agentic-loop/system-prompt architecture writeups. `promptname_b` applies Caveman full compression to the `promptname_a` instruction prose while preserving normal OPPi user-facing style.

Suggested experiment metadata block:

```yaml
---
id: token-saver-v1
kind: append
hypothesis: Reduce response tokens while preserving task completion.
metrics:
  - average_output_tokens
  - user_corrections_per_task
  - tool_call_count
  - task_success_rate
status: draft
---
```
