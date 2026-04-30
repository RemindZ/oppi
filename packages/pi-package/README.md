# @oppiai/pi-package

OPPi's Pi package: extensions, skills, prompts, and themes that make stock Pi feel like OPPi.

Most users should install the CLI instead:

```bash
npm install -g @oppiai/cli
oppi doctor
oppi
```

For direct Pi debugging or package development:

```bash
pi --no-extensions -e ./packages/pi-package
```

The package registers OPPi defaults, docked UI, `/effort`, `/permissions`, `/review`, `/init`, `/independent @plan.md`, `/exit`, `/clear`/`/reset`, `todo_write`, `ask_user`, `suggest_next_message`, feedback intake, `image_gen`, `render_mermaid`, themes, terminal setup, configurable usage/footer bars, follow-up chain context, Hoppi memory hooks, explicit `@oppiai/hoppi-memory` install prompts/settings, legacy `/memory-maintenance` fallback cleanup (superseded by automatic dreaming when enabled), and compaction helpers.

`/independent @plan.md` starts a reusable plan-runner workflow: it loads the `independent` skill, reads the referenced plan document, maintains todos, asks only blocking clarification questions, and continues through implementation plus validation instead of stopping at a proposal.
