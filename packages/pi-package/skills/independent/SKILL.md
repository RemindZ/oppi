---
name: independent
description: Execute work independently from a plan document, roadmap, checklist, issue spec, or implementation plan. Use when the user asks you to follow a plan to completion, run autonomously, maintain todos, and ask only necessary clarification questions.
---

# Independent Plan Runner

Use this skill when the user wants you to carry out a plan document with minimal supervision.

## Goal

Complete the plan, not just plan the plan. Continue until every plan item is completed, explicitly deferred, or genuinely blocked.

## Inputs

The plan may be provided as:

- a file reference such as `@plan.md`, `@docs/stage-5.md`, or multiple references
- pasted Markdown/checklists
- a repo issue/spec/roadmap file
- a user description that names where the plan lives

If a reference starts with `@`, treat it as a document path to inspect with the available file-reading tools, resolving relative paths from the current working directory unless the environment says otherwise.

## Required workflow

1. **Load instructions and plan context**
   - Read the plan document completely.
   - Follow links or references that are necessary to understand the active stage.
   - Read local agent/project instructions when present, such as `AGENTS.md`, `.agents/`, `.pi/`, or other files the environment explicitly requires.
   - Check current repository state before editing; do not overwrite unrelated user changes.

2. **Create an execution todo list**
   - Use `todo_write` for multi-step work.
   - Convert the plan into concise action-oriented todos.
   - Keep one or two items `in_progress` at a time.
   - Update todos as the plan changes, when blockers appear, and after validation.

3. **Run independently**
   - Do not stop after summarizing or proposing a plan.
   - Choose reasonable defaults and continue when the plan leaves implementation details open.
   - Ask clarification questions only when needed for:
     - product decisions with multiple plausible outcomes
     - destructive or irreversible actions
     - credentials/secrets/account access
     - production deploys or package publishes
     - legal/licensing/security-sensitive choices
   - Use the environment's structured question tool when available.

4. **Implement in focused batches**
   - Keep changes aligned with the plan document.
   - Prefer small, real, composable implementation steps over throwaway scaffolding.
   - Update docs or plan checkboxes when behavior changes and the plan is meant to stay current.
   - Do not silently broaden scope beyond the plan; add newly discovered work to todos or defer it explicitly.

5. **Validate before calling work done**
   - Run the most relevant checks/tests/builds for the changed area.
   - If full validation is impossible, run targeted validation and record why full validation could not run.
   - Do not mark todos complete until the implementation and validation for that item are complete.

6. **Commit or publish only when appropriate**
   - Commit logical completed chunks only if the user request, repository instructions, or active session explicitly allows commits.
   - Ask before the first commit if permission is unclear.
   - Never publish packages, deploy to production, rotate secrets, or run destructive operations unless the user explicitly requested that action.

## Final response

End with a concise completion report:

- what was completed
- key files changed
- validation commands and results
- commits made, if any
- deferred items or blockers, if any
- how the result maps back to the plan document

If work was blocked, state the blocker, what was already done safely, and the exact decision/action needed to continue.
