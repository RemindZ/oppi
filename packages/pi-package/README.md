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

The package registers OPPi defaults, docked UI, `/effort`, `/permissions`, `/review`, `/init`, `/exit`, `todo_write`, `ask_user`, feedback intake, `image_gen`, `render_mermaid`, themes, terminal setup, usage footer, Hoppi memory hooks, and compaction helpers.
