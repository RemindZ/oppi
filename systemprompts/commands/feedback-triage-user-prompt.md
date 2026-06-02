# Feedback triage user prompt

Runtime source: `packages/pi-package/extensions/feedback.ts`.

`/bug-report` and `/feature-request` send a triage prompt that asks the main agent to gather enough structured context, ask concise follow-up questions when important information is missing, and then call `oppi_feedback_submit` with validated fields.

Important runtime rules:

- Prefer diagnostics by default, but include logs only with user agreement or clear need.
- Do not include secrets or raw private conversation history.
- Submit only when the issue/request has enough context for maintainers to act.

The `/prompt-variant` selector appends `feedback-triage-user-prompt-append.md` when variant `a`/`promptname_a` or `b`/`promptname_b` is active.
