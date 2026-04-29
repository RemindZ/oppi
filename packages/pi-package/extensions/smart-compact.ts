import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { complete } from "@mariozechner/pi-ai";
import type { ExtensionAPI, ExtensionContext } from "@mariozechner/pi-coding-agent";
import { convertToLlm, getAgentDir, serializeConversation } from "@mariozechner/pi-coding-agent";

const SMART_COMPACT_VERSION = 1;
const DEFAULT_SMART_COMPACT_THRESHOLD_PERCENT = 65;
const VALID_SMART_COMPACT_THRESHOLDS = [50, 55, 60, 65, 70, 75] as const;
const SMART_COMPACT_SOURCE = "oppi-smart-compact";

function continueAfterCompact(thresholdPercent: SmartCompactThreshold): string {
  return [
    `OPPi compacted the conversation around the remaining todo list at ${thresholdPercent}% context usage.`,
    "Continue the current task from the compacted summary; the compacted summary is the source of truth for pre-compaction work.",
    "Focus only on pending, in-progress, or blocked todos, and do not redo completed todos unless the remaining work requires it.",
    "On your next todo_write update, you may omit completed/cancelled todos that are already archived in the compacted summary; keep only active, blocked, and pending work visible.",
    "When the remaining todos are done, your final user-facing response must combine the archived completed todo outcomes with any post-compaction work/validation so it does not sound like only the last todo happened.",
  ].join(" ");
}

type SmartCompactThreshold = (typeof VALID_SMART_COMPACT_THRESHOLDS)[number];

type SmartCompactConfig = {
  thresholdPercent: SmartCompactThreshold;
};

type OppiSettingsFile = Record<string, any> & {
  oppi?: {
    smartCompact?: Partial<SmartCompactConfig>;
  };
};

type TodoStatus = "pending" | "in_progress" | "completed" | "blocked" | "cancelled";
type TodoPriority = "low" | "medium" | "high";

type TodoItem = {
  id: string;
  content: string;
  status: TodoStatus;
  priority?: TodoPriority;
  phase?: string;
  notes?: string;
};

type TodoOutcome = {
  id: string;
  content: string;
  status: Extract<TodoStatus, "completed" | "cancelled">;
  phase?: string;
  notes?: string;
  outcome: string;
  updatedAt?: string;
};

type SmartCompactDetails = {
  source: typeof SMART_COMPACT_SOURCE;
  version: typeof SMART_COMPACT_VERSION;
  compactedAt: string;
  compactionNumber: number;
  remainingTodos: TodoItem[];
  completedOutcomes: TodoOutcome[];
  priorOutcomeCount: number;
  trigger: "pi-auto" | "oppi-threshold" | "manual";
  thresholdPercent?: SmartCompactThreshold;
};

type TodoSnapshot = {
  todos: TodoItem[];
  remaining: TodoItem[];
  completed: TodoItem[];
  outcomes: TodoOutcome[];
  latestSummary?: string;
};

function globalSettingsPath(): string {
  return join(getAgentDir(), "settings.json");
}

function projectSettingsPath(cwd: string): string {
  return join(cwd, ".pi", "settings.json");
}

function readJson(path: string): OppiSettingsFile {
  try {
    if (!existsSync(path)) return {};
    return JSON.parse(readFileSync(path, "utf8"));
  } catch {
    return {};
  }
}

function coerceSmartCompactThreshold(value: unknown): SmartCompactThreshold {
  const numeric = Number(value);
  return VALID_SMART_COMPACT_THRESHOLDS.includes(numeric as SmartCompactThreshold)
    ? (numeric as SmartCompactThreshold)
    : DEFAULT_SMART_COMPACT_THRESHOLD_PERCENT;
}

function normalizeSmartCompactConfig(value: Partial<SmartCompactConfig> | undefined): SmartCompactConfig {
  return { thresholdPercent: coerceSmartCompactThreshold(value?.thresholdPercent) };
}

function readSmartCompactConfig(cwd: string): SmartCompactConfig {
  const global = readJson(globalSettingsPath()).oppi?.smartCompact;
  const project = readJson(projectSettingsPath(cwd)).oppi?.smartCompact;
  return normalizeSmartCompactConfig({ ...global, ...project });
}

function normalizeStatus(value: unknown): TodoStatus {
  return ["pending", "in_progress", "completed", "blocked", "cancelled"].includes(String(value))
    ? (String(value) as TodoStatus)
    : "pending";
}

function normalizeTodo(raw: any, index: number): TodoItem {
  return {
    id: String(raw?.id || index + 1).trim() || String(index + 1),
    content: String(raw?.content || "Untitled task").trim().slice(0, 300),
    status: normalizeStatus(raw?.status),
    priority: ["low", "medium", "high"].includes(String(raw?.priority)) ? raw.priority : undefined,
    phase: typeof raw?.phase === "string" && raw.phase.trim() ? raw.phase.trim().slice(0, 120) : undefined,
    notes: typeof raw?.notes === "string" && raw.notes.trim() ? raw.notes.trim().slice(0, 500) : undefined,
  };
}

function todoOutcome(todo: TodoItem, updatedAt?: string): TodoOutcome | undefined {
  if (todo.status !== "completed" && todo.status !== "cancelled") return undefined;
  const note = todo.notes?.trim();
  return {
    id: todo.id,
    content: todo.content,
    status: todo.status,
    phase: todo.phase,
    notes: note,
    outcome: note || `${todo.status === "completed" ? "Completed" : "Cancelled"}: ${todo.content}`,
    updatedAt,
  };
}

function outcomeKey(outcome: Pick<TodoOutcome, "id" | "content" | "status">): string {
  return `${outcome.status}\u0000${outcome.id}\u0000${outcome.content}`;
}

function extractSmartDetails(entry: any): SmartCompactDetails | undefined {
  const details = entry?.details;
  const nested = details?.oppiSmartCompact ?? details;
  return nested?.source === SMART_COMPACT_SOURCE && Array.isArray(nested.completedOutcomes)
    ? (nested as SmartCompactDetails)
    : undefined;
}

function collectPriorOutcomes(branchEntries: readonly any[]): { outcomes: TodoOutcome[]; compactionNumber: number } {
  const outcomes = new Map<string, TodoOutcome>();
  let compactionNumber = 0;
  for (const entry of branchEntries) {
    if (entry?.type !== "compaction") continue;
    const details = extractSmartDetails(entry);
    if (!details) continue;
    compactionNumber = Math.max(compactionNumber, Number(details.compactionNumber) || 0);
    for (const outcome of details.completedOutcomes) outcomes.set(outcomeKey(outcome), outcome);
  }
  return { outcomes: [...outcomes.values()], compactionNumber };
}

function snapshotFromDetails(details: any): TodoSnapshot | undefined {
  if (!Array.isArray(details?.todos)) return undefined;
  const todos = details.todos.map((todo: any, index: number) => normalizeTodo(todo, index));
  const latestSummary = typeof details.summary === "string" ? details.summary : undefined;
  const latestUpdatedAt = typeof details.updatedAt === "string" ? details.updatedAt : undefined;
  const remaining = todos.filter((todo) => todo.status !== "completed" && todo.status !== "cancelled");
  const completed = todos.filter((todo) => todo.status === "completed" || todo.status === "cancelled");
  const outcomes = completed
    .map((todo) => todoOutcome(todo, latestUpdatedAt))
    .filter((outcome): outcome is TodoOutcome => Boolean(outcome));
  return { todos, remaining, completed, outcomes, latestSummary };
}

function readTodoSnapshot(branchEntries: readonly any[]): TodoSnapshot {
  let latest: TodoSnapshot | undefined;

  for (const entry of branchEntries) {
    if (entry?.type !== "message") continue;
    const message = entry.message;
    if (message?.role !== "toolResult" || message.toolName !== "todo_write") continue;
    latest = snapshotFromDetails(message.details) ?? latest;
  }

  return latest ?? { todos: [], remaining: [], completed: [], outcomes: [] };
}

function readTodoSnapshotFromToolResults(toolResults: readonly any[]): TodoSnapshot | undefined {
  let latest: TodoSnapshot | undefined;
  for (const message of toolResults) {
    if (message?.role !== "toolResult" || message.toolName !== "todo_write") continue;
    latest = snapshotFromDetails(message.details) ?? latest;
  }
  return latest;
}

function mergeOutcomes(...groups: TodoOutcome[][]): TodoOutcome[] {
  const merged = new Map<string, TodoOutcome>();
  for (const group of groups) {
    for (const outcome of group) merged.set(outcomeKey(outcome), { ...merged.get(outcomeKey(outcome)), ...outcome });
  }
  return [...merged.values()];
}

function formatTodo(todo: TodoItem): string {
  const phase = todo.phase ? ` [${todo.phase}]` : "";
  const prio = todo.priority ? ` priority=${todo.priority}` : "";
  const notes = todo.notes ? ` — ${todo.notes}` : "";
  return `- [${todo.status}] ${todo.id}${phase}: ${todo.content}${prio}${notes}`;
}

function formatOutcome(outcome: TodoOutcome): string {
  const phase = outcome.phase ? ` [${outcome.phase}]` : "";
  return `- [${outcome.status}] ${outcome.id}${phase}: ${outcome.content} — ${outcome.outcome}`;
}

function fileLists(fileOps: any): { readFiles: string[]; modifiedFiles: string[] } {
  const read = new Set<string>(Array.from(fileOps?.read ?? []).map(String));
  const modified = new Set<string>([
    ...Array.from(fileOps?.written ?? []).map(String),
    ...Array.from(fileOps?.edited ?? []).map(String),
  ]);
  return {
    readFiles: [...read].filter((file) => !modified.has(file)).sort(),
    modifiedFiles: [...modified].sort(),
  };
}

function truncateLedgerText(value: string | undefined, max = 12_000): string | undefined {
  const text = value?.trim();
  if (!text) return undefined;
  if (text.length <= max) return text;
  return `${text.slice(0, max).trimEnd()}\n\n[Previous summary truncated by OPPi deterministic scoped-compaction fallback: ${text.length - max} chars omitted.]`;
}

function compactTrigger(customInstructions: string | undefined): SmartCompactDetails["trigger"] {
  if (customInstructions?.includes("OPPi todo-aware scoped compaction")) return "oppi-threshold";
  if (customInstructions) return "manual";
  return "pi-auto";
}

function buildSmartDetails(ctx: ExtensionContext, snapshot: TodoSnapshot, prior: { outcomes: TodoOutcome[]; compactionNumber: number }, trigger: SmartCompactDetails["trigger"]): SmartCompactDetails {
  return {
    source: SMART_COMPACT_SOURCE,
    version: SMART_COMPACT_VERSION,
    compactedAt: new Date().toISOString(),
    compactionNumber: prior.compactionNumber + 1,
    remainingTodos: snapshot.remaining,
    completedOutcomes: mergeOutcomes(prior.outcomes, snapshot.outcomes),
    priorOutcomeCount: prior.outcomes.length,
    trigger,
    thresholdPercent: readSmartCompactConfig(ctx.cwd).thresholdPercent,
  };
}

function buildDeterministicScopedSummary(event: any, ctx: ExtensionContext, snapshot: TodoSnapshot, prior: { outcomes: TodoOutcome[]; compactionNumber: number }, reason: string) {
  const { preparation, customInstructions } = event;
  const details = buildSmartDetails(ctx, snapshot, prior, compactTrigger(customInstructions));
  const files = fileLists(preparation.fileOps);
  const previousSummary = truncateLedgerText(preparation.previousSummary);
  const operatorInstructions = truncateLedgerText(customInstructions, 2_000);
  const summary = `## Goal
Continue the current OPPi task from the remaining todo list after scoped compaction.

## Remaining Todos
${snapshot.remaining.length ? snapshot.remaining.map(formatTodo).join("\n") : "- No remaining todos. Preserve final-response context only."}

## Completed Todo Outcomes
${details.completedOutcomes.length ? details.completedOutcomes.map(formatOutcome).join("\n") : "- No completed todo outcomes recorded yet."}

## Relevant Context For Remaining Work
- OPPi used deterministic todo-aware scoped-compaction fallback because ${reason}.
- Latest todo update: ${snapshot.latestSummary ?? "none recorded"}.
${operatorInstructions ? `- Operator compaction instructions: ${operatorInstructions}` : "- No extra operator compaction instructions."}
${previousSummary ? `\n<previous-compaction-summary>\n${previousSummary}\n</previous-compaction-summary>` : "\n- No previous compaction summary was available."}

## File State
Read files:
${files.readFiles.length ? files.readFiles.map((file) => `- ${file}`).join("\n") : "- None recorded by Pi file tracking."}

Modified files:
${files.modifiedFiles.length ? files.modifiedFiles.map((file) => `- ${file}`).join("\n") : "- None recorded by Pi file tracking."}

## Next Steps
1. Continue only pending, in-progress, or blocked todos unless the user asks otherwise.
2. Treat the Completed Todo Outcomes ledger above as mandatory final-response context, not as visible active work.
3. If more detailed historical context is needed, rely on the recent kept messages after the compaction boundary.

## Final Response Notes
- At task completion, summarize both the completed-outcomes ledger and any post-compaction work/validation.
- If the ledger is long, group it concisely, but do not answer with only the final remaining todo.
- Do not redo completed/cancelled todos unless remaining work depends on them.`;

  return {
    summary,
    firstKeptEntryId: preparation.firstKeptEntryId,
    tokensBefore: preparation.tokensBefore,
    details: { oppiSmartCompact: details },
  };
}

async function buildScopedSummary(event: any, ctx: ExtensionContext, snapshot: TodoSnapshot, prior: { outcomes: TodoOutcome[]; compactionNumber: number }) {
  if (!ctx.model) return undefined;

  const auth = await ctx.modelRegistry.getApiKeyAndHeaders(ctx.model);
  if (!auth.ok || !auth.apiKey) return undefined;

  const { preparation, customInstructions, signal } = event;
  const allMessages = [...preparation.messagesToSummarize, ...preparation.turnPrefixMessages];
  const conversationText = serializeConversation(convertToLlm(allMessages));
  const completedOutcomes = mergeOutcomes(prior.outcomes, snapshot.outcomes);
  const previousSummary = preparation.previousSummary?.trim()
    ? `\n\n<previous-compaction-summary>\n${preparation.previousSummary.trim()}\n</previous-compaction-summary>`
    : "";
  const userFocus = customInstructions?.trim()
    ? `\n\nAdditional user/operator compaction instructions:\n${customInstructions.trim()}`
    : "";

  const files = fileLists(preparation.fileOps);

  const prompt = `You are OPPi's todo-aware scoped compactor. Summarize only what is needed to continue the remaining todo list and to write the final user-facing response later.

Rules:
- Keep context relevant to pending, in-progress, or blocked todos.
- Preserve concrete user requirements, constraints, paths, commands, decisions, errors, and file state needed for remaining todos.
- Do not carry irrelevant chatter or details for completed todos, except for concise completed-todo outcomes.
- Completed todo outcomes may have survived prior compactions; merge them and preserve them so the final answer can mention all completed work.
- Once completed/cancelled todo outcomes are archived here, future todo_write calls may omit those completed/cancelled items to keep the visible todo list focused on remaining work.
- If no todos remain, preserve a concise final-response ledger and any verification/blocker information.
- Under the ## Final Response Notes section, explicitly tell the next model to combine the archived completed outcomes with any post-compaction work/validation when the task is complete.
- Be compact, factual, and operational. Do not invent results.

Return markdown with these exact sections:
## Goal
## Remaining Todos
## Completed Todo Outcomes
## Relevant Context For Remaining Work
## File State
## Next Steps
## Final Response Notes

Current todo list summary: ${snapshot.latestSummary ?? "none"}

<remaining-todos>
${snapshot.remaining.length ? snapshot.remaining.map(formatTodo).join("\n") : "No remaining todos. Preserve final-response context only."}
</remaining-todos>

<completed-outcomes-ledger>
${completedOutcomes.length ? completedOutcomes.map(formatOutcome).join("\n") : "No completed todo outcomes recorded yet."}
</completed-outcomes-ledger>

<read-files>
${files.readFiles.join("\n")}
</read-files>

<modified-files>
${files.modifiedFiles.join("\n")}
</modified-files>${previousSummary}${userFocus}

<conversation-being-compacted>
${conversationText}
</conversation-being-compacted>`;

  const response = await complete(
    ctx.model,
    {
      messages: [
        {
          role: "user" as const,
          content: [{ type: "text" as const, text: prompt }],
          timestamp: Date.now(),
        },
      ],
    },
    {
      apiKey: auth.apiKey,
      headers: auth.headers,
      maxTokens: 8192,
      signal,
    },
  );

  const summary = response.content
    .filter((part): part is { type: "text"; text: string } => part.type === "text")
    .map((part) => part.text)
    .join("\n")
    .trim();

  if (!summary || signal.aborted) return undefined;

  const details = buildSmartDetails(ctx, snapshot, prior, compactTrigger(customInstructions));

  return {
    summary,
    firstKeptEntryId: preparation.firstKeptEntryId,
    tokensBefore: preparation.tokensBefore,
    details: { oppiSmartCompact: details },
  };
}

class TodoAwareAutoCompactor {
  private running = false;
  private lastTriggeredTokens: number | null | undefined;
  private lastTriggerAt = 0;

  reset(): void {
    this.running = false;
    this.lastTriggeredTokens = undefined;
    this.lastTriggerAt = 0;
  }

  maybeTrigger(ctx: ExtensionContext, pi: ExtensionAPI, checkpointSnapshot?: TodoSnapshot): void {
    if (this.running || ctx.hasPendingMessages()) return;

    const branch = ctx.sessionManager.getBranch();
    const snapshot = checkpointSnapshot ?? readTodoSnapshot(branch);
    if (snapshot.remaining.length === 0) return;

    const { thresholdPercent } = readSmartCompactConfig(ctx.cwd);
    const usage = ctx.getContextUsage();
    const percent = usage?.percent;
    if (percent === null || percent === undefined || percent < thresholdPercent) return;

    const now = Date.now();
    const tokens = usage?.tokens ?? null;
    if (tokens !== null && tokens === this.lastTriggeredTokens && now - this.lastTriggerAt < 30 * 60_000) return;

    this.running = true;
    this.lastTriggeredTokens = tokens;
    this.lastTriggerAt = now;
    ctx.ui.notify(`OPPi scoped compacting around remaining todos (${Math.round(percent)}% ≥ ${thresholdPercent}%).`, "info");
    ctx.compact({
      customInstructions: `OPPi todo-aware scoped compaction at ${thresholdPercent}%. Keep context relevant to remaining todos, preserve completed todo outcomes for the final response, and ensure the final reply summarizes archived outcomes plus post-compaction work.`,
      onComplete: () => {
        this.running = false;
        try {
          pi.sendUserMessage(continueAfterCompact(thresholdPercent));
        } catch (error) {
          ctx.ui.notify(error instanceof Error ? error.message : String(error), "warning");
        }
      },
      onError: (error) => {
        this.running = false;
        ctx.ui.notify(`OPPi scoped compaction failed: ${error.message}`, "warning");
      },
    });
  }
}

export default function smartCompactExtension(pi: ExtensionAPI) {
  const auto = new TodoAwareAutoCompactor();

  pi.on("session_start", async () => {
    auto.reset();
  });

  pi.on("session_before_compact", async (event, ctx) => {
    const snapshot = readTodoSnapshot(event.branchEntries);
    const prior = collectPriorOutcomes(event.branchEntries);
    const hasTodoContext = snapshot.todos.length > 0 || prior.outcomes.length > 0;
    if (!hasTodoContext) return;

    try {
      const compaction = await buildScopedSummary(event, ctx, snapshot, prior);
      if (compaction) return { compaction };
      ctx.ui.notify("OPPi scoped compaction is preserving todos with deterministic fallback because no model/API key is available.", "warning");
      return { compaction: buildDeterministicScopedSummary(event, ctx, snapshot, prior, "no active model/API key was available") };
    } catch (error) {
      if (event.signal.aborted) return;
      const reason = error instanceof Error ? error.message : String(error);
      ctx.ui.notify(`OPPi scoped compaction model path failed; preserving todos with deterministic fallback (${reason}).`, "warning");
      return { compaction: buildDeterministicScopedSummary(event, ctx, snapshot, prior, reason) };
    }
  });

  pi.on("turn_end", async (event, ctx) => {
    if (!event.toolResults || event.toolResults.length === 0) return;
    const checkpointSnapshot = readTodoSnapshotFromToolResults(event.toolResults);
    if (!checkpointSnapshot) return;
    auto.maybeTrigger(ctx, pi, checkpointSnapshot);
  });
}
