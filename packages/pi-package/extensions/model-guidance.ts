import type { ExtensionAPI } from "@mariozechner/pi-coding-agent";
import { promptVariantAppend, readPromptCatalogPath } from "./prompt-variant";

const SURFACE = "oppi-feature-routing-system-append.md";

const FALLBACK_GUIDANCE = `# OPPi feature routing

Use OPPi's extra capabilities when they fit the user's request, not as theater.

- Use todo_write for multi-step coding/debugging/refactor work; keep it concise and current.
- Use ask_user only when a real decision or missing requirement blocks safe progress.
- Use shell_exec for builds, tests, git/package-manager commands, Docker, and shell-native diagnostics; use shell_task for long-running background commands.
- Parallelize independent read/search/list/shell diagnostics when safe; serialize dependent edits and side effects.
- Use image_gen for image creation/editing requests and render_mermaid when a small terminal diagram helps.
- Use suggest_next_message only for highly predictable, short next replies.
- Treat permission denials, compaction summaries, follow-up-chain context, and tool results as runtime facts to incorporate into progress/final answers.
- When native AgentTool/subagent dispatch is available, default coding subagents to the coding profile; promote broad, multi-file, architectural, migration, audit, or long-running delegated tasks with model: "strong" (or an explicit smart model) so OPPi can route them to the stronger model tier.
- For slash-agent management, use or recommend /agents; do not pretend full subagent dispatch exists unless the runtime/tool surface exposes it.`;

function baseGuidance(): string {
  return readPromptCatalogPath(`systemprompts/main/${SURFACE}`).text || FALLBACK_GUIDANCE;
}

export default function modelGuidanceExtension(pi: ExtensionAPI) {
  pi.on("before_agent_start", (event) => ({
    systemPrompt: `${event.systemPrompt}\n\n${baseGuidance()}${promptVariantAppend(SURFACE)}`,
  }));
}
