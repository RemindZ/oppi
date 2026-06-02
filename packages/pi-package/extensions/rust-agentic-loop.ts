import { existsSync, readFileSync } from "node:fs";
import { platform } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { spawn } from "node:child_process";
import { createInterface } from "node:readline";
import { getAgentDir, type ExtensionAPI, type ExtensionCommandContext, type ExtensionContext } from "@mariozechner/pi-coding-agent";

const __dirname = dirname(fileURLToPath(import.meta.url));
const MAX_CAPTURE_CHARS = 24_000;
const MAX_DIAGNOSTIC_DEPTH = 5;
const MAX_DIAGNOSTIC_KEYS = 80;
const SENSITIVE_KEY_PATTERN = /(?:token|secret|password|passwd|authorization|api[_-]?key|access[_-]?key|refresh[_-]?token|cookie|session)/i;
const RUNTIME_LOOP_MODES = ["off", "command", "default-with-fallback"] as const;
const DEFAULT_RUNTIME_LOOP_MODE = "default-with-fallback" as const;
const DEFAULT_FALLBACK_WARNING_INTERVAL_MS = 5 * 60_000;

type JsonRpcResponse<T = unknown> = {
  result?: T;
  error?: { code: number; message: string; data?: unknown };
};

type RuntimeThreadStartResult = {
  thread: { id: string };
};

type RuntimeAgenticTurnResult = {
  turn: { id: string; status: string };
  events: unknown[];
  awaitingApproval?: unknown;
};

type RuntimeLoopMode = (typeof RUNTIME_LOOP_MODES)[number];

type RuntimeClient = {
  request: <R>(method: string, params: unknown) => Promise<R>;
  close: () => Promise<void>;
  kill: () => void;
};

type CapturedTool = {
  id: string;
  name: string;
  args?: unknown;
  result?: unknown;
  isError?: boolean;
  startedAt: number;
  completedAt?: number;
};

type RuntimeToolDefinition = {
  name: string;
  namespace?: string;
  description?: string;
  concurrencySafe?: boolean;
  requiresApproval?: boolean;
  capabilities?: string[];
};

type RuntimeToolResult = {
  callId: string;
  status: "ok" | "error" | "denied" | "aborted";
  output?: string;
  error?: string;
};

type Capture = {
  id: string;
  prompt: string;
  cwd: string;
  startedAt: number;
  assistantText: string;
  tools: CapturedTool[];
  toolDefinitions: RuntimeToolDefinition[];
  mode: RuntimeLoopMode;
  source: "command" | "default";
  bridgedEventCount: number;
  diagnostics: string[];
  runtime?: RuntimeClient;
  threadId?: string;
  mirrored?: RuntimeAgenticTurnResult;
  error?: string;
  cancelled?: boolean;
};

let activeCapture: Capture | undefined;
let lastCapture: Capture | undefined;
let nextCaptureId = 0;
let lastDefaultFallbackWarningAt = 0;

function resolveOppiServerBin(cwd: string): string | undefined {
  const explicit = process.env.OPPI_SERVER_BIN?.trim();
  if (explicit) return resolve(explicit);
  const exe = platform() === "win32" ? "oppi-server.exe" : "oppi-server";
  const candidates = [
    resolve(cwd, "target", "debug", exe),
    resolve(cwd, "target", "release", exe),
    resolve(__dirname, "..", "..", "..", "target", "debug", exe),
    resolve(__dirname, "..", "..", "..", "target", "release", exe),
  ];
  return candidates.find((candidate) => existsSync(candidate));
}

function readJson(path: string): Record<string, any> {
  try {
    if (!existsSync(path)) return {};
    return JSON.parse(readFileSync(path, "utf8"));
  } catch {
    return {};
  }
}

function coerceRuntimeLoopMode(value: unknown): RuntimeLoopMode {
  const raw = typeof value === "string" ? value.trim().toLowerCase() : "";
  if (!raw) return DEFAULT_RUNTIME_LOOP_MODE;
  if (["0", "false", "disabled", "disable", "off", "none"].includes(raw)) return "off";
  if (["default", "default-with-fallback", "mirror", "on"].includes(raw)) return "default-with-fallback";
  if (["command", "opt-in", "manual", "runtime-loop"].includes(raw)) return "command";
  return "command";
}

function runtimeLoopMode(cwd: string): RuntimeLoopMode {
  const global = readJson(join(getAgentDir(), "settings.json"))?.oppi?.runtimeLoop?.mode;
  const project = readJson(join(cwd, ".pi", "settings.json"))?.oppi?.runtimeLoop?.mode;
  return coerceRuntimeLoopMode(process.env.OPPI_RUNTIME_LOOP_MODE ?? project ?? global ?? DEFAULT_RUNTIME_LOOP_MODE);
}

function redactText(value: string): string {
  return value
    .replace(/(?:sk-[a-zA-Z0-9_-]{12,}|[a-zA-Z0-9_-]{20,}\.[a-zA-Z0-9_-]{20,}\.[a-zA-Z0-9_-]{20,})/g, "[redacted-secret]")
    .replace(/(token|secret|password|api[_-]?key|authorization)(["'`\s:=]+)([^\s"'`,}]+)/gi, "$1$2[redacted]");
}

function sanitizeForDiagnostics(value: unknown, depth = 0): unknown {
  if (value == null) return value;
  if (typeof value === "string") return compactText(redactText(value));
  if (typeof value === "number" || typeof value === "boolean") return value;
  if (depth >= MAX_DIAGNOSTIC_DEPTH) return "[truncated-depth]";
  if (Array.isArray(value)) return value.slice(0, MAX_DIAGNOSTIC_KEYS).map((item) => sanitizeForDiagnostics(item, depth + 1));
  if (typeof value !== "object") return String(value);
  const output: Record<string, unknown> = {};
  let count = 0;
  for (const [key, child] of Object.entries(value as Record<string, unknown>)) {
    if (count++ >= MAX_DIAGNOSTIC_KEYS) {
      output.__truncated = true;
      break;
    }
    output[key] = SENSITIVE_KEY_PATTERN.test(key) ? "[redacted]" : sanitizeForDiagnostics(child, depth + 1);
  }
  return output;
}

function compactText(value: string): string {
  const normalized = value.replace(/\r\n/g, "\n").trim();
  if (normalized.length <= MAX_CAPTURE_CHARS) return normalized;
  return `${normalized.slice(0, MAX_CAPTURE_CHARS)}\n\n[truncated by OPPi Rust loop bridge]`;
}

function extractText(value: unknown, depth = 0): string {
  if (value == null || depth > 5) return "";
  if (typeof value === "string") return value;
  if (typeof value !== "object") return "";
  if (Array.isArray(value)) return value.map((item) => extractText(item, depth + 1)).filter(Boolean).join("\n");
  const record = value as Record<string, unknown>;
  for (const key of ["text", "content", "output", "message", "delta"]) {
    const text = extractText(record[key], depth + 1);
    if (text) return text;
  }
  return "";
}

function messageLooksAssistant(event: any): boolean {
  const role = event?.message?.role ?? event?.message?.type ?? event?.role ?? event?.type;
  return typeof role === "string" && role.toLowerCase().includes("assistant");
}

function collectRuntimeToolDefinitions(pi: ExtensionAPI): RuntimeToolDefinition[] {
  return pi.getAllTools().map((tool: any) => ({
    name: String(tool.name),
    namespace: "pi",
    description: typeof tool.description === "string" ? tool.description : undefined,
    concurrencySafe: !new Set(["bash", "shell_exec", "edit", "write"]).has(String(tool.name)),
    requiresApproval: false,
    capabilities: tool.name === "shell_exec" || tool.name === "bash" ? ["process"] : [],
  }));
}

function sanitizeToolCallId(value: string): string {
  return value.replace(/[^a-zA-Z0-9_.:-]/g, "-");
}

function makeRuntimeToolCalls(tools: CapturedTool[]): Array<{ id: string; name: string; namespace: string; arguments: unknown }> {
  return tools.map((tool, index) => ({
    id: sanitizeToolCallId(`pi-tool-${index + 1}-${tool.id || tool.name}`),
    name: tool.name,
    namespace: "pi",
    arguments: tool.args ?? {},
  }));
}

function makeRuntimeToolResults(tools: CapturedTool[]): RuntimeToolResult[] {
  return tools.map((tool, index) => {
    const callId = sanitizeToolCallId(`pi-tool-${index + 1}-${tool.id || tool.name}`);
    if (!tool.completedAt) {
      return {
        callId,
        status: "aborted",
        error: `${tool.name} did not complete before the Pi adapter capture ended`,
      };
    }
    const sanitized = sanitizeForDiagnostics(tool.result);
    const output = compactText(redactText(extractText(sanitized) || JSON.stringify(sanitized ?? {})));
    return {
      callId,
      status: tool.isError ? "error" : "ok",
      output: output || undefined,
      error: tool.isError ? output || `${tool.name} failed` : undefined,
    };
  });
}

function buildAgenticSteps(capture: Capture) {
  const assistantText = compactText(capture.assistantText || "Pi adapter completed without captured assistant text.");
  const toolCalls = makeRuntimeToolCalls(capture.tools);
  const toolResults = makeRuntimeToolResults(capture.tools);
  if (toolCalls.length === 0) {
    return [{ assistantDeltas: [assistantText], finalResponse: true }];
  }
  return [
    { assistantDeltas: [assistantText], toolCalls, toolResults, finalResponse: false },
    { assistantDeltas: [`Mirrored ${toolCalls.length} Pi tool result(s) through the Rust agentic loop.`], finalResponse: true },
  ];
}

function runtimeServerCommand(serverBin: string): { command: string; args: string[] } {
  if (platform() === "win32" && /\.(?:cmd|bat)$/i.test(serverBin)) return { command: "cmd.exe", args: ["/d", "/s", "/c", serverBin, "--stdio"] };
  return { command: serverBin, args: ["--stdio"] };
}

function withRuntimeAuth(params: unknown): unknown {
  const token = process.env.OPPI_SERVER_AUTH_TOKEN?.trim();
  if (!token || !params || typeof params !== "object" || Array.isArray(params)) return params;
  return { ...(params as Record<string, unknown>), authToken: token };
}

async function openRuntimeClient(cwd: string): Promise<RuntimeClient> {
  const serverBin = resolveOppiServerBin(cwd);
  if (!serverBin) throw new Error("oppi-server not found. Build with `cargo build -p oppi-server` or set OPPI_SERVER_BIN.");
  const serverCommand = runtimeServerCommand(serverBin);
  const child = spawn(serverCommand.command, serverCommand.args, {
    cwd,
    env: { ...process.env, OPPI_EXPERIMENTAL_RUNTIME: "1" },
    stdio: ["pipe", "pipe", "pipe"],
    windowsHide: true,
  });
  let stderr = "";
  let nextId = 0;
  let requestQueue = Promise.resolve();
  let closed = false;
  let closing = false;
  child.stderr.on("data", (chunk: Buffer) => { stderr += chunk.toString("utf8"); });
  const lines = createInterface({ input: child.stdout });
  const iterator = lines[Symbol.asyncIterator]();
  const closePromise = new Promise<number | null>((resolveClose, rejectClose) => {
    child.on("error", rejectClose);
    child.on("close", resolveClose);
  });
  const request = async <R>(method: string, params: unknown): Promise<R> => {
    const run = requestQueue.then(async () => {
      if (closed || closing) throw new Error("oppi-server runtime client is closed");
      child.stdin.write(`${JSON.stringify({ jsonrpc: "2.0", id: `runtime-loop-${Date.now()}-${++nextId}`, method, params: withRuntimeAuth(params) })}\n`);
      const line = await iterator.next();
      if (line.done || !line.value) throw new Error(`oppi-server returned no JSON-RPC response.${stderr.trim() ? ` stderr: ${redactText(stderr.trim())}` : ""}`);
      const response = JSON.parse(line.value) as JsonRpcResponse<R>;
      if (response.error) throw new Error(redactText(response.error.message || JSON.stringify(response.error)));
      if (response.result === undefined) throw new Error(`${method} returned no result`);
      return response.result;
    });
    requestQueue = run.then(() => undefined, () => undefined);
    return run;
  };
  const close = async () => {
    if (closed) return;
    closing = true;
    await requestQueue.catch(() => undefined);
    closed = true;
    child.stdin.end();
    const code = await closePromise;
    lines.close();
    if (code !== 0) throw new Error(`oppi-server exited with code ${code ?? "signal"}.${stderr.trim() ? ` stderr: ${redactText(stderr.trim())}` : ""}`);
  };
  const kill = () => {
    closed = true;
    lines.close();
    child.kill();
  };
  return { request, close, kill };
}

async function bridgeCaptureEvent(capture: Capture | undefined, name: string, payload: unknown): Promise<void> {
  if (!capture?.runtime || !capture.threadId || (capture.cancelled && name !== "capture_cancelled")) return;
  try {
    await capture.runtime.request("pi/bridge-event", {
      threadId: capture.threadId,
      name,
      payload: sanitizeForDiagnostics(payload),
    });
    capture.bridgedEventCount += 1;
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    capture.error = redactText(message);
    capture.diagnostics.push(`bridge event ${name} failed: ${capture.error}`);
  }
}

async function mirrorCaptureToRust(pi: ExtensionAPI, capture: Capture): Promise<void> {
  if (!capture.runtime || !capture.threadId) {
    capture.runtime = await openRuntimeClient(capture.cwd);
    const start = await capture.runtime.request<RuntimeThreadStartResult>("thread/start", {
      project: { id: "pi-adapter", cwd: capture.cwd, displayName: "Pi adapter capture" },
      title: `Pi bridge ${capture.id}`,
    });
    capture.threadId = start.thread.id;
  }
  await bridgeCaptureEvent(capture, "capture_finalizing", {
    toolCount: capture.tools.length,
    completedToolCount: capture.tools.filter((tool) => tool.completedAt).length,
    assistantTextBytes: capture.assistantText.length,
  });
  const result = await capture.runtime.request<RuntimeAgenticTurnResult>("turn/run-agentic", {
    threadId: capture.threadId,
    input: compactText(redactText(capture.prompt)),
    modelSteps: buildAgenticSteps(capture),
    toolDefinitions: capture.toolDefinitions,
    maxContinuations: 8,
  });
  capture.mirrored = result;
  await bridgeCaptureEvent(capture, "capture_mirrored", { turnId: result.turn.id, status: result.turn.status, eventCount: result.events.length });
  await capture.runtime.close();
  capture.runtime = undefined;
  pi.appendEntry("oppi-rust-agentic-loop", {
    id: capture.id,
    prompt: compactText(redactText(capture.prompt)),
    source: capture.source,
    mode: capture.mode,
    threadId: capture.threadId,
    turnId: result.turn.id,
    status: result.turn.status,
    eventCount: result.events.length,
    bridgedEventCount: capture.bridgedEventCount,
    toolCount: capture.tools.length,
    completedToolCount: capture.tools.filter((tool) => tool.completedAt).length,
    toolDefinitionCount: capture.toolDefinitions.length,
    diagnostics: capture.diagnostics,
    redacted: true,
    mirroredAt: new Date().toISOString(),
  });
}

function renderStatus(capture: Capture | undefined, cwd?: string): string {
  const mode = cwd ? runtimeLoopMode(cwd) : capture?.mode ?? DEFAULT_RUNTIME_LOOP_MODE;
  if (!capture) return `Rust loop mode: ${mode}. No bridge capture has completed yet.`;
  const suffix = `mode=${capture.mode}, source=${capture.source}, bridged=${capture.bridgedEventCount}, tools=${capture.tools.length}`;
  if (capture.cancelled) return `Rust loop bridge capture ${capture.id} was cancelled (${suffix}).`;
  if (capture.error) return `Last Rust loop bridge capture ${capture.id} failed: ${capture.error} (${suffix}).`;
  if (!capture.mirrored) return `Rust loop bridge capture ${capture.id} is still waiting for Pi agent completion (${suffix}).`;
  return `Last Rust loop bridge capture ${capture.id}: Rust turn ${capture.mirrored.turn.id} ${capture.mirrored.turn.status}, ${capture.mirrored.events.length} events (${suffix}).`;
}

function shouldNotifyDefaultFallback(): boolean {
  const now = Date.now();
  if (now - lastDefaultFallbackWarningAt < DEFAULT_FALLBACK_WARNING_INTERVAL_MS) return false;
  lastDefaultFallbackWarningAt = now;
  return true;
}

async function prepareCapture(pi: ExtensionAPI, ctx: Pick<ExtensionContext, "cwd" | "ui">, prompt: string, source: Capture["source"], mode: RuntimeLoopMode): Promise<Capture | undefined> {
  const capture: Capture = {
    id: `capture-${++nextCaptureId}`,
    prompt,
    cwd: ctx.cwd,
    startedAt: Date.now(),
    assistantText: "",
    tools: [],
    toolDefinitions: collectRuntimeToolDefinitions(pi),
    mode,
    source,
    bridgedEventCount: 0,
    diagnostics: [],
  };
  try {
    capture.runtime = await openRuntimeClient(ctx.cwd);
    const start = await capture.runtime.request<RuntimeThreadStartResult>("thread/start", {
      project: { id: "pi-adapter", cwd: capture.cwd, displayName: "Pi adapter capture" },
      title: `Pi bridge ${capture.id}`,
    });
    capture.threadId = start.thread.id;
    await bridgeCaptureEvent(capture, "capture_started", { source, mode, prompt: compactText(redactText(prompt)) });
    return capture;
  } catch (error) {
    capture.error = redactText(error instanceof Error ? error.message : String(error));
    capture.runtime?.kill();
    capture.runtime = undefined;
    lastCapture = capture;
    if (source === "command") ctx.ui.notify(capture.error, "error");
    else if (shouldNotifyDefaultFallback()) ctx.ui.notify(`Rust loop mirror unavailable; continuing with stable Pi runtime fallback. ${capture.error}`, "warning");
    return undefined;
  }
}

async function cancelActiveCapture(ctx: Pick<ExtensionContext, "ui">): Promise<void> {
  const capture = activeCapture;
  if (!capture) {
    ctx.ui.notify("No active Rust loop bridge capture to cancel.", "info");
    return;
  }
  activeCapture = undefined;
  capture.cancelled = true;
  capture.error = "cancelled by user";
  await bridgeCaptureEvent(capture, "capture_cancelled", { reason: "cancelled by user" });
  try { await capture.runtime?.close(); } catch { capture.runtime?.kill(); }
  capture.runtime = undefined;
  lastCapture = capture;
  ctx.ui.notify(`Cancelled Rust loop bridge capture ${capture.id}; the Pi turn may continue normally.`, "warning");
}

async function startCapture(pi: ExtensionAPI, args: string, ctx: ExtensionCommandContext): Promise<void> {
  const prompt = args.trim();
  if (!prompt) {
    ctx.ui.notify("Usage: /runtime-loop <prompt> | /runtime-loop status | /runtime-loop cancel", "warning");
    return;
  }
  const normalized = prompt.toLowerCase();
  if (normalized === "status") {
    ctx.ui.notify(renderStatus(activeCapture ?? lastCapture, ctx.cwd), activeCapture ? "info" : lastCapture?.error ? "error" : "info");
    return;
  }
  if (normalized === "cancel") {
    await cancelActiveCapture(ctx);
    return;
  }
  const mode = runtimeLoopMode(ctx.cwd);
  if (mode === "off") {
    ctx.ui.notify("Rust loop dogfood is disabled by OPPI_RUNTIME_LOOP_MODE=off or oppi.runtimeLoop.mode=off.", "warning");
    return;
  }
  if (activeCapture) {
    ctx.ui.notify("A Rust loop bridge capture is already active. Wait for it to finish, run /runtime-loop status, or /runtime-loop cancel.", "warning");
    return;
  }
  const capture = await prepareCapture(pi, ctx, prompt, "command", mode);
  if (!capture) return;
  activeCapture = capture;
  ctx.ui.notify("Routing this prompt through Pi while streaming adapter events into Rust, then finalizing the Rust 11-step loop.", "info");
  pi.sendUserMessage(prompt);
}

export default function rustAgenticLoopExtension(pi: ExtensionAPI) {
  pi.registerCommand("runtime-loop", {
    description: "Experimental: run a Pi turn, mirror it through Rust's 11-step loop, or inspect/cancel the bridge.",
    handler: async (args, ctx) => startCapture(pi, args, ctx),
  });

  pi.on("before_agent_start", async (event: any, ctx: ExtensionContext) => {
    const mode = runtimeLoopMode(ctx.cwd);
    if (mode !== "default-with-fallback" || activeCapture) return;
    const capture = await prepareCapture(pi, ctx, String(event?.prompt ?? ""), "default", mode);
    if (!capture) return;
    activeCapture = capture;
    ctx.ui.notify("Rust loop dogfood: mirroring this Pi turn into Rust with stable Pi runtime fallback.", "info");
  });

  pi.on("message_update", async (event: any) => {
    if (!activeCapture) return;
    const text = extractText(event?.assistantMessageEvent) || extractText(event?.message);
    if (text && text.length >= activeCapture.assistantText.length) activeCapture.assistantText = compactText(text);
    await bridgeCaptureEvent(activeCapture, "message_update", {
      text: text ? compactText(text) : undefined,
      assistantMessageEvent: event?.assistantMessageEvent,
    });
  });

  pi.on("message_end", async (event: any) => {
    if (!activeCapture || !messageLooksAssistant(event)) return;
    const text = extractText(event?.message);
    if (text) activeCapture.assistantText = compactText(text);
    await bridgeCaptureEvent(activeCapture, "message_end", {
      text: text ? compactText(text) : undefined,
      message: event?.message,
    });
  });

  pi.on("tool_execution_start", async (event: any) => {
    if (!activeCapture) return;
    activeCapture.tools.push({
      id: String(event.toolCallId ?? `tool-${activeCapture.tools.length + 1}`),
      name: String(event.toolName ?? "unknown"),
      args: sanitizeForDiagnostics(event.args),
      startedAt: Date.now(),
    });
    await bridgeCaptureEvent(activeCapture, "tool_execution_start", {
      toolCallId: event.toolCallId,
      toolName: event.toolName,
      args: event.args,
    });
  });

  pi.on("tool_execution_update", async (event: any) => {
    if (!activeCapture) return;
    await bridgeCaptureEvent(activeCapture, "tool_execution_update", {
      toolCallId: event.toolCallId,
      toolName: event.toolName,
      partialResult: event.partialResult,
    });
  });

  pi.on("tool_execution_end", async (event: any) => {
    if (!activeCapture) return;
    const id = String(event.toolCallId ?? "");
    const found = activeCapture.tools.find((tool) => tool.id === id) ?? activeCapture.tools[activeCapture.tools.length - 1];
    if (found) {
      found.result = sanitizeForDiagnostics(event.result);
      found.isError = Boolean(event.isError);
      found.completedAt = Date.now();
    }
    await bridgeCaptureEvent(activeCapture, "tool_execution_end", {
      toolCallId: event.toolCallId,
      toolName: event.toolName,
      result: event.result,
      isError: event.isError,
    });
  });

  pi.on("agent_end", async (_event: any) => {
    const capture = activeCapture;
    if (!capture) return;
    activeCapture = undefined;
    try {
      await mirrorCaptureToRust(pi, capture);
      lastCapture = capture;
    } catch (error) {
      capture.error = redactText(error instanceof Error ? error.message : String(error));
      capture.diagnostics.push(capture.error);
      capture.runtime?.kill();
      capture.runtime = undefined;
      lastCapture = capture;
      pi.appendEntry("oppi-rust-agentic-loop", {
        id: capture.id,
        source: capture.source,
        mode: capture.mode,
        threadId: capture.threadId,
        status: "failed",
        error: capture.error,
        bridgedEventCount: capture.bridgedEventCount,
        toolCount: capture.tools.length,
        completedToolCount: capture.tools.filter((tool) => tool.completedAt).length,
        diagnostics: capture.diagnostics,
        redacted: true,
        mirroredAt: new Date().toISOString(),
      });
    }
  });
}
