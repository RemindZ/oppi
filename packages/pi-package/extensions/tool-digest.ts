import { existsSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join, normalize, sep } from "node:path";
import { pathToFileURL } from "node:url";
import { complete } from "@mariozechner/pi-ai";
import type {
  ExtensionAPI,
  ExtensionContext,
  ToolResultEvent,
  ToolRenderResultOptions,
  ToolDefinition,
  Theme,
} from "@mariozechner/pi-coding-agent";
import {
  createBashToolDefinition,
  createEditToolDefinition,
  createFindToolDefinition,
  createGrepToolDefinition,
  createLsToolDefinition,
  createReadToolDefinition,
  createWriteToolDefinition,
  SettingsManager,
} from "@mariozechner/pi-coding-agent";
import { Container, Spacer, Text, truncateToWidth } from "@mariozechner/pi-tui";

const DIGEST_DETAIL_KEY = "__oppiDigest";
const MAX_PROMPT_CHARS = 5_000;
const MAX_EXPANDED_CHARS = 12_000;
const BUILTIN_TOOLS = ["bash", "read", "edit", "write", "grep", "find", "ls"] as const;
const GROUPABLE_TOOLS = new Set<string>(["bash", "read", "edit", "write", "grep", "find", "ls"]);
const HIDDEN_TOOL_CALL_IDS_KEY = Symbol.for("oppi.toolDigest.hiddenToolCallIds");
const PERMISSION_RENDER_DECISIONS_KEY = Symbol.for("oppi.permissions.renderDecisions");
const UI_PATCHED_KEY = Symbol.for("oppi.toolDigest.uiPatched");

type BuiltinToolName = (typeof BUILTIN_TOOLS)[number];

type DigestDetails = {
  summary: string;
  generatedBy: "ai" | "fallback";
  at: string;
};

type DetailsWithDigest = Record<string, unknown> & {
  [DIGEST_DETAIL_KEY]?: DigestDetails;
};

type DigestRenderState = {
  originalCallComponent?: unknown;
  originalResultComponent?: unknown;
};

type ToolGroup = {
  id: string;
  toolName: string;
  calls: string[];
  labels: Map<string, string>;
  completed: Set<string>;
  failed: Set<string>;
  closed: boolean;
};

type PermissionRenderDecision = {
  kind: "auto-reviewing" | "auto-review-allowed" | "auto-review-cached" | "auto-review-denied" | "auto-review-circuit";
  mode: string;
  riskLevel?: "low" | "medium" | "high" | "critical" | string;
  userAuthorization?: "unknown" | "low" | "medium" | "high" | string;
  rationale?: string;
};

let groupingRunId = 0;
let groupCounter = 0;
let activeGroup: ToolGroup | undefined;
const groupByCallId = new Map<string, ToolGroup>();
const invalidateByCallId = new Map<string, () => void>();

function hiddenToolCallIds(): Set<string> {
  const globalStore = globalThis as Record<symbol, Set<string> | boolean | Map<string, PermissionRenderDecision> | undefined>;
  let ids = globalStore[HIDDEN_TOOL_CALL_IDS_KEY] as Set<string> | undefined;
  if (!ids) {
    ids = new Set<string>();
    globalStore[HIDDEN_TOOL_CALL_IDS_KEY] = ids;
  }
  return ids;
}

function permissionRenderDecision(toolCallId: string): PermissionRenderDecision | undefined {
  const globalStore = globalThis as Record<symbol, Map<string, PermissionRenderDecision> | undefined>;
  return globalStore[PERMISSION_RENDER_DECISIONS_KEY]?.get(toolCallId);
}

function groupPermissionDecision(group: ToolGroup): PermissionRenderDecision | undefined {
  for (const callId of group.calls) {
    const decision = permissionRenderDecision(callId);
    if (decision) return decision;
  }
  return undefined;
}

function themeFg(theme: Theme, color: string, text: string, fallback = "muted"): string {
  try { return theme.fg(color as any, text); } catch { return theme.fg(fallback as any, text); }
}

function themeBg(theme: Theme, color: string, text: string, fallback = "customMessageBg"): string {
  try { return theme.bg(color as any, text); } catch { return theme.bg(fallback as any, text); }
}

function riskColor(level: string | undefined): string {
  switch (level) {
    case "low": return "permissionRiskLow";
    case "medium": return "permissionRiskMedium";
    case "high": return "permissionRiskHigh";
    case "critical": return "permissionRiskCritical";
    default: return "muted";
  }
}

function authColor(level: string | undefined): string {
  switch (level) {
    case "high": return "permissionAuthHigh";
    case "medium": return "permissionAuthMedium";
    case "low": return "permissionAuthLow";
    case "unknown": return "permissionAuthUnknown";
    default: return "muted";
  }
}

function permissionBadge(decision: PermissionRenderDecision | undefined, theme: Theme): string {
  if (!decision) return "";
  const label = decision.kind === "auto-reviewing"
    ? "auto-reviewing…"
    : decision.kind === "auto-review-cached"
      ? "cached-approval"
      : decision.kind === "auto-review-circuit"
        ? "circuit-blocked"
        : decision.kind === "auto-review-allowed"
          ? "auto-approved"
          : "auto-denied";
  const labelColor = decision.kind === "auto-review-allowed" || decision.kind === "auto-review-cached"
    ? riskColor(decision.riskLevel) || "success"
    : decision.kind === "auto-reviewing"
      ? "permissionAutoReviewing"
      : "permissionRiskCritical";
  const risk = decision.riskLevel ? themeFg(theme, riskColor(decision.riskLevel), ` · risk ${decision.riskLevel}`) : "";
  const auth = decision.userAuthorization ? themeFg(theme, authColor(decision.userAuthorization), ` · auth ${decision.userAuthorization}`) : "";
  return `  ${themeFg(theme, labelColor, label)}${risk}${auth}`;
}

function permissionBackground(decision: PermissionRenderDecision | undefined, theme: Theme): ((text: string) => string) | undefined {
  if (!decision) return undefined;
  if (decision.kind === "auto-reviewing") return (text: string) => themeBg(theme, "permissionAutoReviewingBg", text, "toolPendingBg");
  if (decision.kind === "auto-review-allowed" || decision.kind === "auto-review-cached") return (text: string) => themeBg(theme, "permissionAutoApprovedBg", text, "toolSuccessBg");
  return (text: string) => themeBg(theme, "permissionAutoDeniedBg", text, "toolErrorBg");
}

function resolvePiMainPath(): string {
  const require = createRequire(import.meta.url);
  try {
    return require.resolve("@mariozechner/pi-coding-agent");
  } catch {
    // Local OPPi packages are often loaded outside Pi's own global node_modules.
    // Recover Pi's install root from the running CLI path or common npm global paths.
  }

  const needle = `${sep}node_modules${sep}@mariozechner${sep}pi-coding-agent${sep}`;
  for (const raw of process.argv) {
    const value = normalize(raw || "");
    const index = value.indexOf(needle);
    if (index >= 0) return join(value.slice(0, index + needle.length), "dist", "index.js");
  }

  const candidates = [
    process.env.APPDATA ? join(process.env.APPDATA, "npm", "node_modules", "@mariozechner", "pi-coding-agent", "dist", "index.js") : undefined,
    process.env.npm_config_prefix ? join(process.env.npm_config_prefix, "node_modules", "@mariozechner", "pi-coding-agent", "dist", "index.js") : undefined,
  ].filter(Boolean) as string[];

  for (const candidate of candidates) {
    if (existsSync(candidate)) return candidate;
  }

  throw new Error("Cannot resolve @mariozechner/pi-coding-agent internals for OPPi tool digest patch");
}

async function importPiInternal(relativePath: string): Promise<any> {
  const mainPath = resolvePiMainPath();
  return import(pathToFileURL(join(dirname(mainPath), relativePath)).href);
}

function compactAssistantMessageForDisplay(message: any, hideThinkingBlock: boolean): any {
  if (!hideThinkingBlock || !message || message.role !== "assistant" || !Array.isArray(message.content)) return message;

  const hasToolCall = message.content.some((part: any) => part?.type === "toolCall");

  if (hasToolCall) {
    const content = message.content.filter((part: any) => part?.type !== "thinking");
    return content.length === message.content.length ? message : { ...message, content };
  }

  let keptThinking = false;
  let changed = false;
  const content = message.content.filter((part: any) => {
    if (part?.type !== "thinking" || typeof part.thinking !== "string" || !part.thinking.trim()) return true;
    if (!keptThinking) {
      keptThinking = true;
      return true;
    }
    changed = true;
    return false;
  });

  return changed ? { ...message, content } : message;
}

function stripAnsi(value: string): string {
  return value.replace(/\u001B\[[0-?]*[ -/]*[@-~]/g, "").replace(/\u001B\][^\u0007]*(?:\u0007|\u001B\\)/g, "");
}

function compactRenderedToolLines(lines: string[], expanded: boolean): string[] {
  if (expanded || process.env.OPPI_TOOL_DIGEST_SPACING === "loose") return lines;
  let start = 0;
  let end = lines.length;
  while (start < end && stripAnsi(lines[start] ?? "").trim() === "") start++;
  while (end > start && stripAnsi(lines[end - 1] ?? "").trim() === "") end--;
  return lines.slice(start, end);
}

async function installUiPatches(): Promise<void> {
  const globalStore = globalThis as Record<symbol, Set<string> | boolean | undefined>;
  if (globalStore[UI_PATCHED_KEY]) return;
  globalStore[UI_PATCHED_KEY] = true;

  try {
    const { ToolExecutionComponent } = await importPiInternal("modes/interactive/components/tool-execution.js");
    const proto = ToolExecutionComponent?.prototype;
    if (proto && !proto.__oppiDigestRenderPatched) {
      const originalRender = proto.render;
      proto.render = function renderWithOppiHiddenTools(width: number) {
        if (hiddenToolCallIds().has(this.toolCallId)) return [];
        return compactRenderedToolLines(originalRender.call(this, width), Boolean(this.expanded));
      };
      proto.__oppiDigestRenderPatched = true;
    }
  } catch {
    // Internal UI patch is best-effort; digest renderers still work without it.
  }

  try {
    const { AssistantMessageComponent } = await importPiInternal("modes/interactive/components/assistant-message.js");
    const proto = AssistantMessageComponent?.prototype;
    if (proto && !proto.__oppiThinkingCompactPatched) {
      const originalUpdateContent = proto.updateContent;
      proto.updateContent = function updateContentWithCompactThinking(message: any) {
        return originalUpdateContent.call(this, compactAssistantMessageForDisplay(message, Boolean(this.hideThinkingBlock)));
      };
      proto.__oppiThinkingCompactPatched = true;
    }
  } catch {
    // Same: best-effort UI polish only.
  }
}

type ToolRenderContext<TState = any, TArgs = any> = {
  args: TArgs;
  toolCallId: string;
  invalidate: () => void;
  lastComponent?: unknown;
  state: TState;
  cwd: string;
  executionStarted: boolean;
  argsComplete: boolean;
  isPartial: boolean;
  expanded: boolean;
  showImages: boolean;
  isError: boolean;
};

function isBuiltinTool(name: string): name is BuiltinToolName {
  return (BUILTIN_TOOLS as readonly string[]).includes(name);
}

function clamp(value: string | undefined, max = MAX_PROMPT_CHARS): string {
  if (!value) return "";
  const normalized = value.replace(/\r\n/g, "\n").trim();
  return normalized.length > max ? `${normalized.slice(0, max)}\n… [truncated]` : normalized;
}

function textContent(content: Array<{ type: string; text?: string }> | undefined, max = MAX_PROMPT_CHARS): string {
  const text = (content ?? [])
    .filter((item) => item.type === "text" && typeof item.text === "string")
    .map((item) => item.text ?? "")
    .join("\n")
    .trim();
  return clamp(text, max);
}

function compactJson(value: unknown, max = 2_000): string {
  try {
    return clamp(JSON.stringify(value, null, 2), max);
  } catch {
    return clamp(String(value), max);
  }
}

function visibleOneLine(value: string, width: number): string {
  return truncateToWidth(value.replace(/[\r\n\t]+/g, " ").replace(/ +/g, " ").trim(), width, "…");
}

function pathFromArgs(args: Record<string, unknown> | undefined): string | undefined {
  const path = args?.path ?? args?.file_path;
  return typeof path === "string" && path.trim() ? path.trim() : undefined;
}

function fallbackSummary(event: Pick<ToolResultEvent, "toolName" | "input" | "content" | "details" | "isError">): string {
  const args = event.input ?? {};
  const path = pathFromArgs(args);
  const output = textContent(event.content, 1_000);
  switch (event.toolName) {
    case "bash": {
      const command = typeof args.command === "string" ? args.command.trim() : "command";
      const codeMatch = output.match(/Command exited with code (\d+)/i);
      if (event.isError) return `Failed: command ${codeMatch ? `exited ${codeMatch[1]}` : "errored"}: ${command}`;
      return `Ran ${command || "bash command"}`;
    }
    case "read":
      return event.isError ? `Failed: could not read ${path ?? "file"}` : `Read ${path ?? "file"}`;
    case "edit": {
      const edits = Array.isArray((args as any).edits) ? (args as any).edits.length : 1;
      return event.isError ? `Failed: could not edit ${path ?? "file"}` : `Edited ${path ?? "file"} (${edits} block${edits === 1 ? "" : "s"})`;
    }
    case "write":
      return event.isError ? `Failed: could not write ${path ?? "file"}` : `Wrote ${path ?? "file"}`;
    case "grep": {
      const pattern = typeof args.pattern === "string" ? args.pattern : "pattern";
      if (/No matches found/i.test(output)) return `Searched for ${pattern}; no matches`;
      return event.isError ? `Failed: grep failed for ${pattern}` : `Searched for ${pattern}`;
    }
    case "find": {
      const pattern = typeof args.pattern === "string" ? args.pattern : "files";
      if (/No files found/i.test(output)) return `Found no files for ${pattern}`;
      return event.isError ? `Failed: find failed for ${pattern}` : `Found files for ${pattern}`;
    }
    case "ls":
      return event.isError ? `Failed: could not list ${path ?? "."}` : `Listed ${path ?? "."}`;
    default:
      return event.isError ? `Failed: ${event.toolName} failed` : `Used ${event.toolName}`;
  }
}

async function aiSummary(event: ToolResultEvent, ctx: ExtensionContext): Promise<string | undefined> {
  if (process.env.OPPI_TOOL_DIGEST_AI !== "1") return undefined;

  const model = ctx.model;
  if (!model) return undefined;

  const auth = await ctx.modelRegistry.getApiKeyAndHeaders(model).catch(() => undefined);
  if (!auth?.ok || !auth.apiKey) return undefined;

  const prompt = [
    "Write a terse, past-tense one-line recap of this coding-agent tool result.",
    "Rules: return only the recap; max 12 words; no markdown; no quotes; include critical errors.",
    "",
    `Tool: ${event.toolName}`,
    `Errored: ${event.isError ? "yes" : "no"}`,
    "Arguments:",
    compactJson(event.input, 1_500),
    "Result text:",
    textContent(event.content, 2_500),
  ].join("\n");

  try {
    const response = await complete(
      model,
      {
        messages: [
          {
            role: "user" as const,
            content: [{ type: "text" as const, text: clamp(prompt, MAX_PROMPT_CHARS) }],
            timestamp: Date.now(),
          },
        ],
      },
      {
        apiKey: auth.apiKey,
        headers: auth.headers,
        maxTokens: 64,
        reasoningEffort: "minimal",
        signal: ctx.signal,
      },
    );

    const summary = response.content
      .filter((part): part is { type: "text"; text: string } => part.type === "text")
      .map((part) => part.text)
      .join(" ")
      .replace(/[\r\n]+/g, " ")
      .replace(/^[-*\s]+/, "")
      .replace(/^['\"]|['\"]$/g, "")
      .trim();

    return summary ? clamp(summary, 160) : undefined;
  } catch {
    return undefined;
  }
}

function mergeDigest(details: unknown, digest: DigestDetails): DetailsWithDigest {
  const base = details && typeof details === "object" && !Array.isArray(details) ? { ...(details as Record<string, unknown>) } : {};
  base[DIGEST_DETAIL_KEY] = digest;
  return base as DetailsWithDigest;
}

function getDigest(details: unknown): DigestDetails | undefined {
  if (!details || typeof details !== "object") return undefined;
  const maybe = (details as DetailsWithDigest)[DIGEST_DETAIL_KEY];
  if (!maybe || typeof maybe !== "object") return undefined;
  return typeof maybe.summary === "string" ? maybe : undefined;
}

function toolLabel(toolName: string, args: Record<string, unknown> | undefined): string {
  const path = pathFromArgs(args);
  switch (toolName) {
    case "bash": {
      const command = typeof args?.command === "string" ? args.command : "";
      return `$ ${command || "..."}`;
    }
    case "read":
      return `read ${path ?? "..."}`;
    case "edit":
      return `edit ${path ?? "..."}`;
    case "write":
      return `write ${path ?? "..."}`;
    case "grep":
      return `grep ${typeof args?.pattern === "string" ? `/${args.pattern}/` : "..."}${path ? ` in ${path}` : ""}`;
    case "find":
      return `find ${typeof args?.pattern === "string" ? args.pattern : "..."}${path ? ` in ${path}` : ""}`;
    case "ls":
      return `ls ${path ?? "."}`;
    default:
      return toolName;
  }
}

function groupItemLabel(toolName: string, args: Record<string, unknown> | undefined): string {
  const path = pathFromArgs(args);
  switch (toolName) {
    case "read":
      return path ?? "file";
    case "ls":
      return path ?? ".";
    case "grep": {
      const pattern = typeof args?.pattern === "string" ? `/${args.pattern}/` : "pattern";
      const scope = path && path !== "." ? ` in ${path}` : "";
      return `${pattern}${scope}`;
    }
    case "find": {
      const pattern = typeof args?.pattern === "string" ? args.pattern : "files";
      const scope = path && path !== "." ? ` in ${path}` : "";
      return `${pattern}${scope}`;
    }
    case "edit":
    case "write":
      return path ?? "file";
    default:
      return toolLabel(toolName, args);
  }
}

function noteToolStarted(toolName: string, toolCallId: string, args: Record<string, unknown> | undefined): void {
  if (!GROUPABLE_TOOLS.has(toolName)) {
    activeGroup = undefined;
    return;
  }

  if (!activeGroup || activeGroup.toolName !== toolName || activeGroup.closed) {
    activeGroup = {
      id: `${groupingRunId}:${++groupCounter}`,
      toolName,
      calls: [],
      labels: new Map(),
      completed: new Set(),
      failed: new Set(),
      closed: false,
    };
  }

  if (!groupByCallId.has(toolCallId)) {
    activeGroup.calls.push(toolCallId);
    groupByCallId.set(toolCallId, activeGroup);
  }
  if (pathFromArgs(args) || toolName !== "read") {
    activeGroup.labels.set(toolCallId, groupItemLabel(toolName, args));
  }
}

function hasUsefulGroupArgs(toolName: string, args: Record<string, unknown> | undefined): boolean {
  if (!args) return false;
  if (toolName === "read") return Boolean(pathFromArgs(args));
  if (toolName === "grep" || toolName === "find") return typeof args.pattern === "string" && args.pattern.trim().length > 0;
  return true;
}

function noteToolCompleted(toolName: string, toolCallId: string, args: Record<string, unknown> | undefined, isError: boolean): void {
  if (!GROUPABLE_TOOLS.has(toolName)) return;

  let group = groupByCallId.get(toolCallId);
  if (!group) {
    group = {
      id: `${groupingRunId}:${++groupCounter}`,
      toolName,
      calls: [toolCallId],
      labels: new Map(),
      completed: new Set(),
      failed: new Set(),
      closed: true,
    };
    groupByCallId.set(toolCallId, group);
  }

  if (hasUsefulGroupArgs(toolName, args) || !group.labels.has(toolCallId)) {
    group.labels.set(toolCallId, groupItemLabel(toolName, args));
  }
  group.completed.add(toolCallId);
  if (isError) group.failed.add(toolCallId);
  if (isCompletedGroup(group)) invalidateGroup(group);
}

function rememberInvalidate(context: ToolRenderContext<DigestRenderState, any>): void {
  invalidateByCallId.set(context.toolCallId, context.invalidate);
}

function invalidateGroup(group: ToolGroup): void {
  for (const callId of group.calls) invalidateByCallId.get(callId)?.();
}

function isCompletedGroup(group: ToolGroup): boolean {
  return group.calls.length > 1 && group.completed.size >= group.calls.length;
}

function groupSummary(group: ToolGroup): string {
  const labels = group.calls.map((id) => group.labels.get(id) ?? "item");
  const uniqueLabels = [...new Set(labels)];
  const shown = uniqueLabels.slice(0, 6).join(", ");
  const suffix = uniqueLabels.length > 6 ? `, +${uniqueLabels.length - 6} more` : "";
  const target = `${shown}${suffix}`;
  const prefix = group.failed.size > 0 ? `Finished with ${group.failed.size} error${group.failed.size === 1 ? "" : "s"}: ` : "";
  switch (group.toolName) {
    case "bash": return `${prefix}Ran ${target}`;
    case "read": return `${prefix}Read ${target}`;
    case "edit": return `${prefix}Edited ${target}`;
    case "write": return `${prefix}Wrote ${target}`;
    case "grep": return `${prefix}Searched ${target}`;
    case "find": return `${prefix}Found files for ${target}`;
    case "ls": return `${prefix}Listed ${target}`;
    default: return `${prefix}Used ${group.toolName} on ${target}`;
  }
}

function emptyRenderComponent(context: ToolRenderContext<DigestRenderState, any>): Container {
  const container = context.lastComponent instanceof Container ? context.lastComponent : new Container();
  container.clear();
  return container;
}

function diffFromDetails(details: unknown): string | undefined {
  if (!details || typeof details !== "object") return undefined;
  const diff = (details as Record<string, unknown>).diff;
  return typeof diff === "string" && diff.trim() ? diff : undefined;
}

function expandedBody(result: { content: Array<{ type: string; text?: string }>; details?: unknown }, context: ToolRenderContext<DigestRenderState, any>): string {
  const chunks: string[] = [];
  const diff = diffFromDetails(result.details);
  if (diff) chunks.push(diff);

  const output = textContent(result.content, MAX_EXPANDED_CHARS);
  if (output && !chunks.some((chunk) => chunk.includes(output))) chunks.push(output);

  if (context.args && (context.args.content || context.args.edits)) {
    const argsPreview = compactJson(context.args, 4_000);
    if (argsPreview) chunks.push(`Arguments:\n${argsPreview}`);
  }

  return clamp(chunks.join("\n\n"), MAX_EXPANDED_CHARS);
}

function renderCompactCall(toolName: string, args: Record<string, unknown>, theme: Theme, context: ToolRenderContext<DigestRenderState, any>) {
  rememberInvalidate(context);
  if (!context.isPartial && !context.expanded) return new Container();
  const text = context.lastComponent instanceof Text ? context.lastComponent : new Text("", 0, 0);
  const label = visibleOneLine(toolLabel(toolName, args), 120);
  text.setText(theme.fg("toolTitle", theme.bold(label)));
  return text;
}

function renderDigestResult(
  toolName: string,
  result: { content: Array<{ type: string; text?: string }>; details?: unknown },
  options: ToolRenderResultOptions,
  theme: Theme,
  context: ToolRenderContext<DigestRenderState, any>,
) {
  rememberInvalidate(context);
  hiddenToolCallIds().delete(context.toolCallId);
  const group = groupByCallId.get(context.toolCallId);
  if (group && isCompletedGroup(group) && !options.expanded && !options.isPartial) {
    if (context.toolCallId !== group.calls[0]) {
      hiddenToolCallIds().add(context.toolCallId);
      return emptyRenderComponent(context);
    }
    const text = context.lastComponent instanceof Text ? context.lastComponent : new Text("", 0, 0);
    const hasErrors = group.failed.size > 0;
    const icon = hasErrors ? "✗" : "✓";
    const color = hasErrors ? "error" : "success";
    const permission = groupPermissionDecision(group);
    text.setCustomBgFn(permissionBackground(permission, theme));
    text.setText(`${theme.fg(color, icon)} ${theme.fg("toolOutput", visibleOneLine(groupSummary(group), 180))}${permissionBadge(permission, theme)}`);
    return text;
  }

  const digest = getDigest(result.details);
  const summary = digest?.summary || fallbackSummary({ toolName, input: context.args ?? {}, content: result.content as any, details: result.details, isError: context.isError });
  const icon = options.isPartial ? "…" : context.isError ? "✗" : "✓";
  const color = options.isPartial ? "warning" : context.isError ? "error" : "success";
  const source = digest?.generatedBy === "ai" ? theme.fg("dim", " ai") : "";
  const permission = permissionRenderDecision(context.toolCallId);
  const summaryLine = `${theme.fg(color, icon)} ${theme.fg("toolOutput", visibleOneLine(summary, 180))}${source}${permissionBadge(permission, theme)}`;

  if (!options.expanded) {
    const text = context.lastComponent instanceof Text ? context.lastComponent : new Text("", 0, 0);
    text.setCustomBgFn(permissionBackground(permission, theme));
    text.setText(summaryLine);
    return text;
  }

  const container = context.lastComponent instanceof Container ? context.lastComponent : new Container();
  container.clear();
  container.addChild(new Text(summaryLine, 0, 0, permissionBackground(permission, theme)));

  const body = expandedBody(result, context);
  if (body) {
    container.addChild(new Spacer(1));
    container.addChild(new Text(theme.fg("toolOutput", body), 0, 0));
  }
  return container;
}

function wrapToolDefinition(definition: ToolDefinition<any, any, DigestRenderState>): ToolDefinition<any, any, DigestRenderState> {
  return {
    ...definition,
    renderShell: "self",
    renderCall(args, theme, context) {
      return renderCompactCall(definition.name, args as Record<string, unknown>, theme, context);
    },
    renderResult(result, options, theme, context) {
      return renderDigestResult(definition.name, result as any, options, theme, context);
    },
  };
}

function registerDigestRenderers(pi: ExtensionAPI, ctx: ExtensionContext): void {
  const settings = SettingsManager.create(ctx.cwd);
  const definitions: ToolDefinition<any, any, DigestRenderState>[] = [
    createBashToolDefinition(ctx.cwd, {
      shellPath: settings.getShellPath(),
      commandPrefix: settings.getShellCommandPrefix(),
    }) as ToolDefinition<any, any, DigestRenderState>,
    createReadToolDefinition(ctx.cwd, {
      autoResizeImages: settings.getImageAutoResize(),
    }) as ToolDefinition<any, any, DigestRenderState>,
    createEditToolDefinition(ctx.cwd) as ToolDefinition<any, any, DigestRenderState>,
    createWriteToolDefinition(ctx.cwd) as ToolDefinition<any, any, DigestRenderState>,
    createGrepToolDefinition(ctx.cwd) as ToolDefinition<any, any, DigestRenderState>,
    createFindToolDefinition(ctx.cwd) as ToolDefinition<any, any, DigestRenderState>,
    createLsToolDefinition(ctx.cwd) as ToolDefinition<any, any, DigestRenderState>,
  ];

  const activeTools = pi.getActiveTools();
  for (const definition of definitions) {
    pi.registerTool(wrapToolDefinition(definition));
  }
  pi.setActiveTools(activeTools);
}

export default async function toolDigestExtension(pi: ExtensionAPI) {
  let renderersRegistered = false;
  await installUiPatches();

  pi.on("session_start", async (_event, ctx) => {
    groupingRunId++;
    activeGroup = undefined;
    hiddenToolCallIds().clear();
    ctx.ui.setToolsExpanded(false);
    if (!renderersRegistered) {
      registerDigestRenderers(pi, ctx);
      renderersRegistered = true;
    }
  });

  pi.on("agent_start", async () => {
    groupingRunId++;
    activeGroup = undefined;
    hiddenToolCallIds().clear();
  });

  pi.on("tool_execution_start", async (event) => {
    noteToolStarted(event.toolName, event.toolCallId, event.args);
  });

  pi.on("tool_execution_end", async (event) => {
    noteToolCompleted(event.toolName, event.toolCallId, event.args, event.isError);
  });

  pi.on("agent_end", async () => {
    if (activeGroup) activeGroup.closed = true;
    activeGroup = undefined;
  });

  pi.on("tool_result", async (event, ctx) => {
    if (!isBuiltinTool(event.toolName)) return;

    noteToolCompleted(event.toolName, event.toolCallId, event.input, event.isError);
    const fallback = fallbackSummary(event);
    const summary = (await aiSummary(event, ctx)) || fallback;
    return {
      details: mergeDigest(event.details, {
        summary,
        generatedBy: summary === fallback ? "fallback" : "ai",
        at: new Date().toISOString(),
      }),
    };
  });
}
