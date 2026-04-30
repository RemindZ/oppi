import type { ExtensionAPI } from "@mariozechner/pi-coding-agent";

const USAGE = "Usage: /independent @plan.md [additional scope]. Runs the referenced plan independently with todos, validation, and minimal clarification questions.";

function independentPrompt(args: string): string {
  return `Use the independent skill to execute the referenced plan document(s) to completion.

Plan document / scope:
${args}

Operating mode:
- First load and follow the full \`independent\` skill instructions.
- Read the referenced plan document(s) completely. If an item starts with \`@\`, resolve it as a file path from the current working directory unless the environment says otherwise.
- Create and maintain a \`todo_write\` execution plan.
- Do not stop after planning; continue through implementation, docs, validation, and final reporting.
- Ask clarification questions only when genuinely blocked by a product decision, secret/account access, destructive operation, production deploy/publish, or irreversible architectural choice. Use the structured question tool if available.
- Choose reasonable defaults and keep working when details are underspecified.
- Run relevant validation before marking work complete.
- Commit only if the user request or project instructions allow it; never publish or deploy unless explicitly requested.

Begin now.`;
}

export default function independentExtension(pi: ExtensionAPI) {
  pi.registerCommand("independent", {
    description: "Run independently from a plan document. Usage: /independent @plan.md",
    handler: async (args, ctx) => {
      const trimmed = args.trim();
      if (!trimmed) {
        ctx.ui.notify(USAGE, "info");
        return;
      }
      pi.sendUserMessage(independentPrompt(trimmed));
    },
  });
}
