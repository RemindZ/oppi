import type { ExtensionAPI } from "@mariozechner/pi-coding-agent";

export default function exitExtension(pi: ExtensionAPI) {
  pi.registerCommand("exit", {
    description: "Exit OPPi after running shutdown cleanup, memory recap, and exit sync when enabled.",
    handler: async (_args, ctx) => {
      ctx.shutdown();
    },
  });
}
