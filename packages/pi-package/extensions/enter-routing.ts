import type { ExtensionAPI, ExtensionContext } from "@mariozechner/pi-coding-agent";
import { Key, matchesKey } from "@mariozechner/pi-tui";

const ALT_ENTER_SEQUENCE = "\x1b\r";
const FOLLOW_UP_CONTEXT_TYPE = "oppi-follow-up-chain-context";
const FOLLOW_UP_ENTRY_TYPE = "oppi-follow-up-chain";

type FollowUpStatus = "queued" | "running" | "completed";

type FollowUpItem = {
  id: number;
  text: string;
  status: FollowUpStatus;
  queuedAt: string;
  startedAt?: string;
  completedAt?: string;
};

type FollowUpChain = {
  id: string;
  rootPrompt: string;
  rootStartedAt: string;
  followUps: FollowUpItem[];
};

type ActivePrompt =
  | { kind: "standalone"; text: string }
  | { kind: "follow-up"; chainId: string; followUpId: number; text: string };

let activePrompt: ActivePrompt | undefined;
let activeChain: FollowUpChain | undefined;
let currentRunningPrompt = "";
let nextFollowUpId = 1;

function compactWhitespace(value: string): string {
  return value.replace(/\s+/g, " ").trim();
}

function truncate(value: string, max = 500): string {
  const compact = compactWhitespace(value);
  return compact.length > max ? `${compact.slice(0, Math.max(0, max - 1)).trimEnd()}…` : compact;
}

function nowIso(): string {
  return new Date().toISOString();
}

function chainOpen(chain: FollowUpChain | undefined): boolean {
  return Boolean(chain && chain.followUps.some((followUp) => followUp.status !== "completed"));
}

function publishFollowUpStatus(ctx: ExtensionContext): void {
  if (!ctx.hasUI) return;
  const queued = activeChain?.followUps.filter((followUp) => followUp.status === "queued").length ?? 0;
  const running = activeChain?.followUps.some((followUp) => followUp.status === "running") ?? false;
  ctx.ui.setStatus("oppi.followup", queued > 0 ? `follow:${queued}` : running ? "follow:run" : undefined);
}

function logChain(pi: ExtensionAPI, event: string, chain: FollowUpChain): void {
  pi.appendEntry(FOLLOW_UP_ENTRY_TYPE, {
    event,
    chainId: chain.id,
    rootPrompt: chain.rootPrompt,
    rootStartedAt: chain.rootStartedAt,
    followUps: chain.followUps.map((followUp) => ({ ...followUp })),
    updatedAt: nowIso(),
  });
}

function ensureChain(pi: ExtensionAPI): FollowUpChain {
  if (activeChain && chainOpen(activeChain)) return activeChain;

  const rootPrompt = currentRunningPrompt || activeChain?.rootPrompt || "Unknown initial request";
  activeChain = {
    id: `fup_${Date.now().toString(36)}_${Math.random().toString(36).slice(2, 8)}`,
    rootPrompt,
    rootStartedAt: nowIso(),
    followUps: [],
  };
  logChain(pi, "created", activeChain);
  return activeChain;
}

function queueFollowUp(pi: ExtensionAPI, ctx: ExtensionContext, text: string): void {
  const content = text.trim();
  if (!content) return;
  const chain = ensureChain(pi);
  chain.followUps.push({
    id: nextFollowUpId++,
    text: content,
    status: "queued",
    queuedAt: nowIso(),
  });
  logChain(pi, "queued", chain);
  publishFollowUpStatus(ctx);
}

function takeQueuedFollowUp(prompt: string): FollowUpItem | undefined {
  const chain = activeChain;
  if (!chain) return undefined;
  const normalizedPrompt = compactWhitespace(prompt);
  return chain.followUps.find((followUp) => followUp.status === "queued" && compactWhitespace(followUp.text) === normalizedPrompt)
    ?? chain.followUps.find((followUp) => followUp.status === "queued");
}

function followUpLedger(chain: FollowUpChain, current: FollowUpItem): string {
  const rows = chain.followUps.map((followUp) => {
    const status = followUp.id === current.id ? "current" : followUp.status;
    return `- ${status}: ${truncate(followUp.text, 300)}`;
  });
  return rows.length ? rows.join("\n") : "- current: no follow-up text recorded";
}

function followUpGuidance(chain: FollowUpChain, current: FollowUpItem): string {
  const pending = chain.followUps.filter((followUp) => followUp.status === "queued" && followUp.id !== current.id);
  const finalInstruction = pending.length > 0
    ? `There ${pending.length === 1 ? "is" : "are"} ${pending.length} additional queued follow-up${pending.length === 1 ? "" : "s"} after this one. Keep this response operational; the last follow-up should provide the combined final answer.`
    : "No further follow-ups were queued when this turn started. When this turn is complete, provide the combined final answer for the initial request and every follow-up in this chain.";

  return [
    "OPPi follow-up chain context:",
    "This user prompt is a follow-up queued while an earlier answer was still running. Treat it as part of the same user-visible task, not as an unrelated standalone request.",
    `Initial standalone request: ${truncate(chain.rootPrompt, 700)}`,
    "Follow-up ledger:",
    followUpLedger(chain, current),
    finalInstruction,
    "Do not dump this ledger verbatim. Use it to make the final user-facing answer cover the initial request plus all completed follow-ups.",
  ].join("\n");
}

function resetForNewStandalone(prompt: string): void {
  activePrompt = { kind: "standalone", text: prompt };
  currentRunningPrompt = prompt;
  if (!chainOpen(activeChain)) activeChain = undefined;
}

export default function enterRoutingExtension(pi: ExtensionAPI) {
  pi.on("session_start", (_event, ctx) => {
    activePrompt = undefined;
    activeChain = undefined;
    currentRunningPrompt = "";
    nextFollowUpId = 1;
    publishFollowUpStatus(ctx);

    if (!ctx.hasUI) return;

    ctx.ui.onTerminalInput((data) => {
      // OPPi wants Claude Code-style message routing:
      // - Enter while idle: normal submit (let Pi's editor handle it so it clears text/history correctly)
      // - Enter while busy: follow-up queue
      // - Ctrl+Enter while busy: normal submit path, which Pi treats as steer
      // Pi's built-in app.message.followUp binding is static, so we rewrite only busy plain Enter
      // into Alt+Enter, then let Pi's own follow-up handler do command/template expansion and clearing.
      const isPlainEnter = matchesKey(data, Key.enter);
      const isAltEnter = data === ALT_ENTER_SEQUENCE;
      if (!isPlainEnter && !isAltEnter) return undefined;
      if (ctx.isIdle()) return undefined;

      const text = ctx.ui.getEditorText().trim();
      if (!text) return undefined;
      // Pi rejects queued extension commands; route the key normally but do not
      // record a durable follow-up chain entry for a prompt that will not run.
      if (!text.startsWith("/")) queueFollowUp(pi, ctx, text);

      if (isPlainEnter) return { data: ALT_ENTER_SEQUENCE };
      return undefined;
    });
  });

  pi.on("before_agent_start", async (event, ctx) => {
    const queuedFollowUp = takeQueuedFollowUp(event.prompt);
    if (!queuedFollowUp || !activeChain) {
      resetForNewStandalone(event.prompt);
      publishFollowUpStatus(ctx);
      return;
    }

    queuedFollowUp.status = "running";
    queuedFollowUp.startedAt = nowIso();
    activePrompt = { kind: "follow-up", chainId: activeChain.id, followUpId: queuedFollowUp.id, text: event.prompt };
    currentRunningPrompt = event.prompt;
    logChain(pi, "started", activeChain);
    publishFollowUpStatus(ctx);

    const guidance = followUpGuidance(activeChain, queuedFollowUp);
    return {
      message: {
        customType: FOLLOW_UP_CONTEXT_TYPE,
        content: guidance,
        display: false,
        details: {
          chainId: activeChain.id,
          followUpId: queuedFollowUp.id,
          pendingFollowUps: activeChain.followUps.filter((followUp) => followUp.status === "queued" && followUp.id !== queuedFollowUp.id).length,
          createdAt: nowIso(),
        },
      },
      systemPrompt: `${event.systemPrompt}\n\n${guidance}`,
    };
  });

  pi.on("agent_end", async (_event, ctx) => {
    if (activePrompt?.kind === "follow-up" && activeChain?.id === activePrompt.chainId) {
      const followUp = activeChain.followUps.find((item) => item.id === activePrompt?.followUpId);
      if (followUp) {
        followUp.status = "completed";
        followUp.completedAt = nowIso();
        logChain(pi, "completed", activeChain);
      }
      if (!chainOpen(activeChain)) activeChain = undefined;
    }

    activePrompt = undefined;
    currentRunningPrompt = "";
    publishFollowUpStatus(ctx);
  });
}
