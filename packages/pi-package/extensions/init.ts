import type { ExtensionAPI } from "@mariozechner/pi-coding-agent";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";

const MAX_EXISTING_AGENTS_CHARS = 32_000;

const INIT_PROMPT = `Generate a file named AGENTS.md that serves as a contributor guide for this repository.
Your goal is to produce a clear, concise, and well-structured document with descriptive headings and actionable explanations for each section.
Follow the outline below, but adapt as needed — add sections if relevant, and omit those that do not apply to this project.

Document Requirements

- Title the document "Repository Guidelines".
- Use Markdown headings (#, ##, etc.) for structure.
- Keep the document concise. 200-400 words is optimal.
- Keep explanations short, direct, and specific to this repository.
- Provide examples where helpful (commands, directory paths, naming patterns).
- Maintain a professional, instructional tone.

Recommended Sections

Project Structure & Module Organization

- Outline the project structure, including where the source code, tests, and assets are located.

Build, Test, and Development Commands

- List key commands for building, testing, and running locally (e.g., npm test, make build).
- Briefly explain what each command does.

Coding Style & Naming Conventions

- Specify indentation rules, language-specific style preferences, and naming patterns.
- Include any formatting or linting tools used.

Testing Guidelines

- Identify testing frameworks and coverage requirements.
- State test naming conventions and how to run tests.

Commit & Pull Request Guidelines

- Summarize commit message conventions found in the project’s Git history.
- Outline pull request requirements (descriptions, linked issues, screenshots, etc.).

(Optional) Add other sections if relevant, such as Security & Configuration Tips, Architecture Overview, or Agent-Specific Instructions.

Completion Status Requirement

After creating or refreshing AGENTS.md, finish with a concise completion status for the user. Include:

- Result: Created, Updated, or No changes needed.
- Added: bullets for important new guidance, or "none".
- Changed: bullets for important revised guidance, or "none".
- Removed: bullets for stale/incorrect guidance removed, or "none".
- Validation: one sentence explaining how you checked the result against the current repository.

Do not just say "done" when the file changed; tell the user what changed.`;

function refreshPrompt(existing: string): string {
  const truncated = existing.length > MAX_EXISTING_AGENTS_CHARS;
  const visibleExisting = truncated ? `${existing.slice(0, MAX_EXISTING_AGENTS_CHARS)}\n\n[... existing AGENTS.md truncated ...]` : existing;
  return `${INIT_PROMPT}

AGENTS.md already exists, so refresh it instead of skipping.

Claude Code-style refresh workflow:

1. Inspect the repository as needed.
2. Draft a fresh AGENTS.md in your working context using the current repository state.
3. Compare that fresh draft against the existing AGENTS.md below.
4. Validate old instructions before preserving them; remove or revise stale details.
5. If important guidance is missing or stale, update AGENTS.md with a concise merged version.
6. If the existing file is already accurate, say so and leave it unchanged.
7. End with the required completion status: Result, Added, Changed, Removed, and Validation.

Existing AGENTS.md:

\`\`\`markdown
${visibleExisting}
\`\`\``;
}

export default function initExtension(pi: ExtensionAPI) {
  pi.registerCommand("init", {
    description: "Generate or refresh AGENTS.md repository guidelines.",
    handler: async (_args, ctx) => {
      const target = join(ctx.cwd, "AGENTS.md");
      if (existsSync(target)) {
        const existing = readFileSync(target, "utf8");
        await (ctx as any).sendUserMessage(refreshPrompt(existing));
        return;
      }
      await (ctx as any).sendUserMessage(INIT_PROMPT);
    },
  });
}
