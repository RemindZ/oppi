# OPPi active tool prompt additions

These snippets/guidelines are registered by `packages/pi-package` tools and are included by Pi in the main system prompt when the tools are active.

## Built-in Pi tools normally active

- `read`: Read file contents
- `bash`: Execute bash commands (ls, grep, find, etc.)
- `edit`: Make precise file edits with exact text replacement, including multiple disjoint edits in one call
- `write`: Create or overwrite files
- `grep`: Search file contents for patterns (respects .gitignore)
- `find`: Find files by glob pattern (respects .gitignore)
- `ls`: List directory contents

Built-in guidelines include:

- Use read to examine files instead of cat or sed.
- Use write only for new files or complete rewrites.
- Use bash for file operations like ls, rg, find, unless grep/find/ls tools are active; then prefer them for file exploration.
- Be concise in your responses.
- Show file paths clearly when working with files.

## `image_gen`

Snippet:

> Generate or edit images using Codex native image_generation or OpenAI GPT Image models

Guidelines:

- Use image_gen when the user asks to create, generate, draw, render, or edit an image. Do not claim you cannot make images if image_gen is available.
- For ordinary image requests, call image_gen with a rich prompt and omit backend/model so it can choose the best available backend dynamically.
- image_gen Image API fallback defaults to gpt-image-2, size auto, quality medium, and png output.
- If the user explicitly asks for API/model controls or multiple images, use image_gen with backend=image_api and the requested controls; this requires an OpenAI API key.
- gpt-image-2 does not support true transparent backgrounds or input_fidelity. Ask before switching to gpt-image-1.5 for true transparency.
- When the user asks you to pick a subject yourself, choose a concrete subject and call image_gen rather than asking a follow-up question.

## `todo_write`

Snippet:

> Use todo_write to proactively maintain a concise phase/task todo list for multi-step work.

Guidelines:

- OPPi owns the visible todo list during multi-step work: create it, update it, add newly discovered tasks, and tick items off without waiting for the user to ask.
- Use todo_write for multi-step coding tasks, refactors, debugging sessions, and plans with several dependent steps.
- Do not use todo_write for tiny one-shot tasks.
- Always send the full current todo list, not just changed items.
- Keep todo content short and action-oriented.
- At most one or two todos should be in_progress unless work is genuinely parallel.
- When starting a task, mark it in_progress; when it is finished, mark it completed before moving on or giving the final answer.
- When completing a todo, put the concrete outcome in notes when useful so OPPi's scoped compactor can preserve it for the final response.
- After OPPi performs scoped compaction, completed/cancelled todo outcomes are archived in the compacted summary; future todo_write calls may omit those archived completed/cancelled items and keep only active, blocked, and pending work visible.
- When preparing a progress or final response, include completed outcomes in the user-facing message and then call todo_write with completed/cancelled items pruned (or an empty list if nothing actionable remains), unless those items are still useful context.
- If the plan changes, add new todos, mark obsolete ones cancelled, and explain the change briefly in the todo_write summary.

## `ask_user`

Snippet:

> Use ask_user to ask focused clarifying questions or request explicit user decisions.

Guidelines:

- Use ask_user when you need user input before taking action, especially for ambiguous requirements or permission overrides.
- Batch related questions into one ask_user call instead of asking one at a time.
- Provide concrete options when possible and include allowCustom when free-form input is useful.
- Keep questions short and directly actionable.
