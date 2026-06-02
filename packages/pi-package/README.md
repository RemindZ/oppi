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

The package registers OPPi defaults, model-facing feature routing guidance, docked UI, `/prompt-variant` A/B prompt selection, `/effort`, `/permissions`, `/review`, `/init`, `/agents`, `/runtime-loop`, `/independent @plan.md`, `/exit`, `/clear`/`/reset`, `todo_write`, `ask_user`, `suggest_next_message`, `shell_exec`/`shell_task`, feedback intake, `image_gen`, `render_mermaid`, themes, terminal setup, configurable usage/footer bars, follow-up chain context, Hoppi memory hooks, explicit `@oppiai/hoppi-memory` install prompts/settings, legacy `/memory-maintenance` fallback cleanup (superseded by automatic dreaming when enabled), and compaction helpers.

`/agents` browses built-in personalities (`general-purpose`, `Explore`, `Plan`, `oppi-code-guide`, `statusline-setup`, `verification`) and manages OPPi agent definition markdown files in `.oppi/agents` and the personal OPPi agent dir, while also listing compatible `.claude/agents` files as read-only imports. Project/personal agents can override built-ins by name. It manages definitions only; full AgentTool runtime dispatch belongs to the Rust runtime spine.

The Rust-loop bridge now defaults to `default-with-fallback`: normal Pi turns keep using Pi as the primary runtime while redacted lifecycle events are mirrored into `oppi-server` and finalized through Rust's 11-phase `turn/run-agentic` path when the server is available. `/runtime-loop <prompt>` still forces a single mirrored turn, `/runtime-loop status` reports the last capture, and `/runtime-loop cancel` stops the bridge capture while leaving Pi's stable runtime fallback alone. Set `OPPI_RUNTIME_LOOP_MODE=command` for opt-in-only mirroring or `off` to disable the bridge. Separately, the CLI-level `oppi runtime-worker smoke --json` exercises the direct Rust provider worker path with a local OpenAI-compatible mock, including streaming chunks, compact transcript seeding, a provider-requested Rust tool call, Rust-owned text file tools, and approval-resume support. `oppi runtime-worker <prompt>` is now the opt-in direct-worker dogfood command for credentialed provider runs; when Hoppi is installed and memory is enabled, the CLI bridges bounded Hoppi recall into the direct-provider context and saves a bounded Hoppi turn summary after completion. The direct worker now also honors the same prompt A/B variant setting (`--prompt-variant`, `OPPI_SYSTEM_PROMPT_VARIANT`, stored `oppi.promptVariant`), Rust owns `oppi__todo_write` state with `todos/list` for the backend half of `/todos` parity, and Rust applies host-supplied follow-up chain context to direct-provider prompts while Pi/native clients own the visible queue UI. It reports stable Pi fallback guidance when provider auth or `oppi-server` is unavailable, and supports `--no-memory` / `OPPI_RUNTIME_WORKER_MEMORY=off` for memory-free dogfood. Precise tool schemas, a generalized Rust memory broker for non-CLI clients, native `/todos` UI, native follow-up queue UI, and persisted provider snapshots remain future direct-worker slices.

`/independent @plan.md` starts a reusable plan-runner workflow: it loads the `independent` skill, reads the referenced plan document, maintains todos, asks only blocking clarification questions, and continues through implementation plus validation instead of stopping at a proposal.

`shell_exec` gives the model a bounded Bash/PowerShell execution tool with cwd tracking, timeouts, output logs under `output/shelltool`, and optional background execution. Use `shell_task` to list, read, or kill background shell tasks.
