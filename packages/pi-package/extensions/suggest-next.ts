import type { ExtensionAPI, ExtensionContext } from "@mariozechner/pi-coding-agent";
import { CustomEditor } from "@mariozechner/pi-coding-agent";
import { Type } from "typebox";
import { Key, matchesKey, parseKey, truncateToWidth, visibleWidth, type EditorTheme, type TUI } from "@mariozechner/pi-tui";
import type { KeybindingsManager } from "@mariozechner/pi-coding-agent";

const MIN_CONFIDENCE = 0.7;
const MAX_SUGGESTION_CHARS = 180;
const STATUS_KEY = "oppi.suggestNext";

const suggestNextSchema = Type.Object(
  {
    message: Type.String({
      description: "The exact short next user message to show as a ghost suggestion. Use the user's likely phrasing.",
    }),
    confidence: Type.Number({
      minimum: 0,
      maximum: 1,
      description: "Your confidence from 0 to 1. OPPi only shows suggestions at 0.7 or above.",
    }),
    reason: Type.Optional(Type.String({
      description: "Brief private reason for why this next message is likely. Used for diagnostics only.",
    })),
  },
  { additionalProperties: false },
);

type SuggestNextParams = {
  message: string;
  confidence: number;
  reason?: string;
};

type Suggestion = {
  message: string;
  confidence: number;
  reason?: string;
  createdAt: number;
};

function cleanSuggestion(raw: string): string | undefined {
  let text = raw.trim();
  if (!text) return undefined;
  text = text.replace(/^[[({"'`\s]+|[\])}"'`\s]+$/g, "").trim();
  text = text.replace(/\s+/g, " ").trim();
  if (!text) return undefined;
  return text.length > MAX_SUGGESTION_CHARS ? `${text.slice(0, MAX_SUGGESTION_CHARS - 1).trimEnd()}…` : text;
}

function dim(text: string): string {
  return `\x1b[2m${text}\x1b[22m`;
}

function isLikelyTextEntry(data: string): boolean {
  if (!data) return false;
  if (data.includes("\x1b[200~")) return true; // bracketed paste
  const parsed = parseKey(data);
  if (parsed === "space") return true;
  if (parsed && parsed.length === 1) return true;
  if (parsed?.startsWith("shift+")) {
    const shifted = parsed.slice("shift+".length);
    if (shifted === "space" || shifted.length === 1) return true;
  }
  return data.charCodeAt(0) >= 32 && data.charCodeAt(0) !== 127 && !data.startsWith("\x1b");
}

class SuggestionController {
  private active: Suggestion | undefined;
  private pending: Suggestion | undefined;
  private contexts = new Set<ExtensionContext>();
  private requestRender: (() => void) | undefined;

  bind(ctx: ExtensionContext): void {
    this.contexts.add(ctx);
    this.refreshStatus();
  }

  unbind(ctx: ExtensionContext): void {
    this.contexts.delete(ctx);
  }

  setRequestRender(requestRender: () => void): void {
    this.requestRender = requestRender;
  }

  getActiveMessage(): string | undefined {
    return this.active?.message;
  }

  setPending(params: SuggestNextParams): { shown: boolean; message?: string; reason: string } {
    const message = cleanSuggestion(params.message);
    const confidence = Number(params.confidence);
    if (!message) {
      this.pending = undefined;
      return { shown: false, reason: "empty-suggestion" };
    }
    if (!Number.isFinite(confidence) || confidence < MIN_CONFIDENCE) {
      this.pending = undefined;
      return { shown: false, message, reason: `confidence ${Number.isFinite(confidence) ? confidence.toFixed(2) : "?"} is below ${MIN_CONFIDENCE}` };
    }
    this.pending = { message, confidence, reason: cleanSuggestion(params.reason ?? ""), createdAt: Date.now() };
    return { shown: false, message, reason: "queued-until-agent-end" };
  }

  activatePending(): void {
    if (!this.pending) {
      this.clear();
      return;
    }
    this.active = this.pending;
    this.pending = undefined;
    this.refreshStatus();
  }

  clear(): void {
    this.pending = undefined;
    if (!this.active) return;
    this.active = undefined;
    this.refreshStatus();
  }

  clearAll(): void {
    this.pending = undefined;
    this.active = undefined;
    this.refreshStatus();
  }

  private refreshStatus(): void {
    const text = this.active ? "suggest" : undefined;
    for (const ctx of this.contexts) {
      if (!ctx.hasUI) continue;
      try {
        ctx.ui.setStatus(STATUS_KEY, text);
      } catch {
        // Ignore stale contexts during session replacement/reload.
      }
    }
    this.requestRender?.();
  }
}

class SuggestedNextEditor extends CustomEditor {
  constructor(
    tui: TUI,
    theme: EditorTheme,
    private readonly appKeybindings: KeybindingsManager,
    private readonly controller: SuggestionController,
  ) {
    super(tui, theme, appKeybindings);
    this.controller.setRequestRender(() => tui.requestRender());
  }

  override handleInput(data: string): void {
    const suggestion = this.controller.getActiveMessage();
    if (suggestion && this.getText().length === 0) {
      if (matchesKey(data, Key.right)) {
        this.setText(suggestion);
        this.controller.clear();
        return;
      }
      if (this.appKeybindings.matches(data, "tui.input.submit") || matchesKey(data, Key.enter)) {
        this.controller.clear();
        this.onSubmit?.(suggestion);
        return;
      }
      if (isLikelyTextEntry(data)) {
        this.controller.clear();
        super.handleInput(data);
        return;
      }
    }
    super.handleInput(data);
  }

  override setText(text: string): void {
    if (text.trim()) this.controller.clear();
    super.setText(text);
  }

  override insertTextAtCursor(text: string): void {
    if (text) this.controller.clear();
    super.insertTextAtCursor(text);
  }

  override render(width: number): string[] {
    const lines = super.render(width);
    const suggestion = this.controller.getActiveMessage();
    if (!suggestion || this.getText().length > 0 || this.isShowingAutocomplete()) return lines;

    const cursor = "\x1b[7m \x1b[0m";
    const lineIndex = lines.findIndex((line, index) => index > 0 && line.includes(cursor));
    if (lineIndex < 0) return lines;

    const line = lines[lineIndex];
    const cursorIndex = line.indexOf(cursor);
    const prefix = line.slice(0, cursorIndex + cursor.length);
    const available = Math.max(0, width - visibleWidth(prefix));
    if (available <= 0) return lines;

    const ghost = dim(truncateToWidth(suggestion, available, "…"));
    const replacement = prefix + ghost;
    lines[lineIndex] = replacement + " ".repeat(Math.max(0, width - visibleWidth(replacement)));
    return lines;
  }
}

const controller = new SuggestionController();

export default function suggestedNextExtension(pi: ExtensionAPI) {
  pi.registerTool({
    name: "suggest_next_message",
    label: "Suggest next message",
    description: "Show a grey ghost suggestion for the user's likely next message. Use only when you are at least 70% confident you know the exact short response the user will want to send next.",
    promptSnippet: "Suggest a short next user message only when you are at least 70% confident.",
    promptGuidelines: [
      "Use suggest_next_message sparingly, only when the user's next reply is highly predictable (confidence >= 0.7).",
      "Suggested messages should be short, concrete, and written in the user's likely wording.",
      "Do not suggest generic replies like 'ok' unless that exact response is clearly the likely next step.",
    ],
    parameters: suggestNextSchema,
    async execute(_toolCallId, params: SuggestNextParams, _signal, _onUpdate, ctx) {
      const result = controller.setPending(params);
      const shown = Boolean(result.message && ctx.isIdle());
      if (shown) controller.activatePending();
      const state = shown ? "shown" : result.message ? "queued" : "not shown";
      return {
        content: [{ type: "text", text: `Suggestion ${state}: ${result.reason}` }],
        details: { ...result, shown, threshold: MIN_CONFIDENCE },
      };
    },
  });

  pi.registerCommand("suggest-next", {
    description: "Set or clear the ghost suggested next message",
    handler: async (args, ctx) => {
      const text = args.trim();
      if (!text || text === "clear" || text === "off") {
        controller.clearAll();
        ctx.ui.notify("Suggested next message cleared", "info");
        return;
      }
      controller.setPending({ message: text, confidence: 1 });
      controller.activatePending();
      ctx.ui.notify("Suggested next message set", "info");
    },
  });

  pi.on("session_start", (_event, ctx) => {
    if (!ctx.hasUI) return;
    controller.bind(ctx);
    ctx.ui.setEditorComponent((tui, theme, keybindings) => new SuggestedNextEditor(tui, theme, keybindings, controller));
  });

  pi.on("agent_start", () => {
    controller.clearAll();
  });

  pi.on("agent_end", () => {
    controller.activatePending();
  });

  pi.on("input", () => {
    controller.clearAll();
  });

  pi.on("session_shutdown", (_event, ctx) => {
    if (ctx.hasUI) {
      try {
        ctx.ui.setEditorComponent(undefined);
        ctx.ui.setStatus(STATUS_KEY, undefined);
      } catch {
        // Ignore stale UI during teardown.
      }
    }
    controller.unbind(ctx);
  });
}
