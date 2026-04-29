# OPPi System Prompts

This folder catalogs the prompts OPPi/Pi sends to language models so they can be reviewed, versioned, and prompt-engineered for token savings and UX.

## What is cataloged

`manifest.json` is the index. Current prompt surfaces:

| ID | Kind | File | Runtime source |
| --- | --- | --- | --- |
| `pi-default-main` | Main system prompt template | `main/pi-default-system-prompt.template.md` | Pi `buildSystemPrompt()` |
| `oppi-tool-additions` | Tool snippets and guidelines injected into the main prompt | `main/oppi-active-tool-snippets-and-guidelines.md` | OPPi tool definitions |
| `review-system-append` | `/review` turn-specific system append | `review/codex-review-system-append.md` | `packages/pi-package/extensions/review.ts` |
| `review-audit-system-append` | `/review` full-audit turn-specific system append | `audit/codebase-audit-system-append.md` | `packages/pi-package/extensions/review.ts` |
| `permissions-auto-review-system` | Isolated auto-reviewer system prompt | `permissions/auto-review-system.md` | `packages/pi-package/extensions/permissions.ts` |
| `permissions-auto-review-user-template` | Auto-reviewer user prompt template | `permissions/auto-review-user-prompt.template.md` | `packages/pi-package/extensions/permissions.ts` |
| `image-gen-codex-native-adapter` | OpenAI Responses `instructions` for native image generation | `image-gen/codex-native-adapter-instructions.md` | `packages/pi-package/extensions/image-gen.ts` |
| `init-command-user-prompt` | `/init` user prompt | `commands/init-user-prompt.md` | `packages/pi-package/extensions/init.ts` |
| `review-command-user-prompts` | `/review` user prompt templates | `commands/review-user-prompts.md` | `packages/pi-package/extensions/review.ts` |
| `review-audit-user-prompts` | `/review` full-audit user prompt templates | `commands/review-audit-user-prompts.md` | `packages/pi-package/extensions/review.ts` |

The command user prompt rows are not system prompts, but they are included because they materially affect LLM behavior.

## How the main prompt is assembled

Pi builds the main prompt dynamically from:

1. the default template in `main/pi-default-system-prompt.template.md`, unless a full custom system prompt is configured;
2. selected tool snippets and active tool guidelines;
3. optional `--append-system-prompt` / configured append text;
4. project context files such as `AGENTS.md` and `CLAUDE.md`;
5. loaded skills, when `read` is available;
6. current date and current working directory.

So the template here is not a byte-for-byte runtime snapshot. It is the stable shape of the prompt with placeholders for dynamic values.

## A/B testing prompt variants

Use `experiments/` for candidate variants.

Recommended workflow today:

```bash
PI_SKIP_VERSION_CHECK=1 pi \
  --no-extensions -e ./packages/pi-package \
  --append-system-prompt systemprompts/experiments/2026-04-27-token-saver.append.md
```

For non-interactive comparisons, keep the task, model, enabled tools, and context files fixed:

```bash
PI_SKIP_VERSION_CHECK=1 OPPI_TOOL_DIGEST_AI=0 pi \
  --no-extensions -e ./packages/pi-package \
  --no-prompt-templates \
  --append-system-prompt systemprompts/experiments/variant-a.append.md \
  -p "<fixed benchmark task>"
```

Suggested metrics:

- output tokens / approximate response length;
- tool call count;
- number of clarification turns;
- user corrections per task;
- task success rate;
- subjective polish/noise rating.

Current A/B variants:

- `promptname_a` — agentic-loop and prompt-architecture overlay based on the authorized writeups.
- `promptname_b` — Caveman-full compressed counterpart to `promptname_a`; this compresses system prompt prose only and preserves normal OPPi user-facing style.

Use the runtime selector inside OPPi. It appends the selected main prompt overlay and lets OPPi extensions read matching variant files for review, permission-reviewer, and image-generation adapter surfaces where wired:

```text
/prompt-variant
/prompt-variant promptname_a
/prompt-variant promptname_b
/prompt-variant off
```

For non-interactive comparisons, set the same selector through the environment:

```bash
OPPI_SYSTEM_PROMPT_VARIANT=promptname_a pi --no-extensions -e ./packages/pi-package -p "<fixed benchmark task>"
OPPI_SYSTEM_PROMPT_VARIANT=promptname_b pi --no-extensions -e ./packages/pi-package -p "<fixed benchmark task>"
```

Current prompt backups live under `backups/2026-04-27/`.

Future OPPi work can add richer telemetry around active variants in `/usage`.

## Prompt strategy references

[Caveman](https://github.com/juliusbrussee/caveman) by Julius Brussee is an allowed MIT-licensed reference for system-prompt strategy experiments. If OPPi adopts ideas from it, keep the concrete prompt changes in this catalog/manifest and preserve attribution in `ATTRIBUTIONS.md`.

## Editing rule

When a runtime prompt changes, update both:

1. the source file that sends the prompt; and
2. this catalog plus `manifest.json`.

That keeps prompt engineering reviewable in normal diffs.
