import type { ExtensionAPI } from "@mariozechner/pi-coding-agent";
import { Key, matchesKey } from "@mariozechner/pi-tui";

const ALT_ENTER_SEQUENCE = "\x1b\r";

export default function enterRoutingExtension(pi: ExtensionAPI) {
  pi.on("session_start", (_event, ctx) => {
    if (!ctx.hasUI) return;

    ctx.ui.onTerminalInput((data) => {
      // OPPi wants Claude Code-style message routing:
      // - Enter while idle: normal submit (let Pi's editor handle it so it clears text/history correctly)
      // - Enter while busy: follow-up queue
      // - Ctrl+Enter while busy: normal submit path, which Pi treats as steer
      // Pi's built-in app.message.followUp binding is static, so we rewrite only busy plain Enter
      // into Alt+Enter, then let Pi's own follow-up handler do command/template expansion and clearing.
      if (!matchesKey(data, Key.enter)) return undefined;
      if (ctx.isIdle()) return undefined;
      if (!ctx.ui.getEditorText().trim()) return undefined;

      return { data: ALT_ENTER_SEQUENCE };
    });
  });
}
