import type { ExtensionAPI, ExtensionContext, ToolRenderResultOptions, Theme } from "@mariozechner/pi-coding-agent";
import { StringEnum } from "@mariozechner/pi-ai";
import { Text } from "@mariozechner/pi-tui";
import { Type, type Static } from "typebox";
import { randomBytes } from "node:crypto";
import { createWriteStream, existsSync, mkdirSync, readFileSync, statSync } from "node:fs";
import { mkdir, readFile, stat, unlink } from "node:fs/promises";
import { delimiter, join, resolve } from "node:path";
import { platform, tmpdir } from "node:os";
import { spawn, type ChildProcessWithoutNullStreams } from "node:child_process";
import { registerBackgroundTask, updateBackgroundTask } from "./background-tasks";

const SHELL_OUTPUT_DIR = join("output", "shelltool");
const DEFAULT_TIMEOUT_MS = 120_000;
const MAX_TIMEOUT_MS = 30 * 60_000;
const INLINE_MAX_BYTES = 30_000;
const INLINE_MAX_LINES = 1_200;
const TAIL_SAMPLE_BYTES = 16_000;
const MAX_TASKS = 80;
const PROGRESS_INTERVAL_MS = 1_000;
const TASK_STALL_AFTER_MS = 60_000;
const BG_OUTPUT_MAX_BYTES = 25 * 1024 * 1024;

const shellKindSchema = StringEnum(["auto", "bash", "powershell"] as const, {
  description: "Shell provider. auto uses PowerShell on Windows when available, otherwise Bash/POSIX shell.",
});

const shellExecSchema = Type.Object(
  {
    command: Type.String({ description: "Shell command to execute. Prefer dedicated read/search/edit/write tools for file operations." }),
    shell: Type.Optional(shellKindSchema),
    timeout: Type.Optional(Type.Number({ description: "Timeout in seconds. Defaults to 120; maximum 1800." })),
    description: Type.Optional(Type.String({ description: "Short active-voice description for UI/background task summaries." })),
    run_in_background: Type.Optional(Type.Boolean({ description: "Start the command as a background task and return immediately with a task ID." })),
    background_on_timeout: Type.Optional(Type.Boolean({ description: "If true, timeout leaves the process running as a background task instead of killing it." })),
    cwd: Type.Optional(Type.String({ description: "Optional working directory. Relative paths resolve from the session cwd/current shell cwd." })),
    dangerouslyDisableSandbox: Type.Optional(Type.Boolean({ description: "Accepted for compatibility. Stage 1 shell_exec does not provide a native sandbox boundary." })),
  },
  { additionalProperties: false },
);

type ShellExecInput = Static<typeof shellExecSchema>;
type ShellKind = "bash" | "powershell";
type ShellChoice = "auto" | ShellKind;
type TaskStatus = "running" | "backgrounded" | "completed" | "failed" | "killed" | "timed_out";

type ShellExecDetails = {
  taskId: string;
  shell: ShellKind;
  command: string;
  description?: string;
  status: TaskStatus;
  exitCode?: number | null;
  signal?: string | null;
  cwdBefore: string;
  cwdAfter?: string;
  outputPath: string;
  outputBytes: number;
  truncated: boolean;
  timedOut?: boolean;
  backgrounded?: boolean;
  runInBackground?: boolean;
  stalledHint?: string;
  startedAt: string;
  completedAt?: string;
  durationMs?: number;
};

type ShellTask = ShellExecDetails & {
  child?: ChildProcessWithoutNullStreams;
  cwdPath: string;
  outputPathAbs: string;
  lastOutputBytes: number;
  lastOutputAt: number;
};

const shellTaskSchema = Type.Object(
  {
    action: StringEnum(["list", "read", "kill"] as const, { description: "Background task action." }),
    taskId: Type.Optional(Type.String({ description: "Task ID for read or kill." })),
    maxBytes: Type.Optional(Type.Number({ description: "Maximum bytes to read from the end of task output. Defaults to 30000." })),
  },
  { additionalProperties: false },
);

type ShellTaskInput = Static<typeof shellTaskSchema>;

const tasks = new Map<string, ShellTask>();
let sessionCwd: string | undefined;

function taskId(): string {
  return `shell-${Date.now().toString(36)}-${randomBytes(3).toString("hex")}`;
}

function normalizeShellChoice(value: unknown): ShellChoice {
  return value === "bash" || value === "powershell" || value === "auto" ? value : "auto";
}

function resolveTimeoutSeconds(value: unknown): number {
  const raw = Number(value);
  if (!Number.isFinite(raw) || raw <= 0) return DEFAULT_TIMEOUT_MS / 1000;
  return Math.min(MAX_TIMEOUT_MS / 1000, Math.max(1, Math.floor(raw)));
}

function ensureOutputDir(cwd: string): string {
  const dir = resolve(cwd, SHELL_OUTPUT_DIR);
  mkdirSync(dir, { recursive: true });
  return dir;
}

function displayPath(cwd: string, absolutePath: string): string {
  const normalizedCwd = resolve(cwd);
  const normalizedPath = resolve(absolutePath);
  return normalizedPath.startsWith(normalizedCwd) ? normalizedPath.slice(normalizedCwd.length + 1).replace(/\\/g, "/") : normalizedPath;
}

function shellQuote(value: string): string {
  return `'${value.replace(/'/g, `'"'"'`)}'`;
}

function psSingleQuote(value: string): string {
  return `'${value.replace(/'/g, "''")}'`;
}

function pathCandidates(command: string): string[] {
  const names = platform() === "win32" && !/\.(?:exe|cmd|bat)$/i.test(command)
    ? [command, `${command}.exe`, `${command}.cmd`, `${command}.bat`]
    : [command];
  const paths = (process.env.PATH || "").split(delimiter).filter(Boolean);
  const candidates: string[] = [];
  for (const path of paths) for (const name of names) candidates.push(join(path, name));
  return candidates;
}

function findExecutable(names: string[], extra: string[] = []): string | undefined {
  for (const candidate of [...extra, ...names.flatMap(pathCandidates)]) {
    try {
      if (existsSync(candidate) && statSync(candidate).isFile()) return candidate;
    } catch {
      // Ignore bad PATH entries.
    }
  }
  return undefined;
}

function resolveShell(choice: ShellChoice): { kind: ShellKind; executable: string } {
  const win = platform() === "win32";
  const bashExtras = win
    ? [
        "C:/Program Files/Git/bin/bash.exe",
        "C:/Program Files/Git/usr/bin/bash.exe",
        "C:/msys64/usr/bin/bash.exe",
      ]
    : ["/bin/bash", "/usr/bin/bash", "/bin/sh", "/usr/bin/sh"];
  const psExtras = win
    ? [
        "C:/Program Files/PowerShell/7/pwsh.exe",
        "C:/Windows/System32/WindowsPowerShell/v1.0/powershell.exe",
      ]
    : ["/usr/bin/pwsh", "/usr/local/bin/pwsh", "/opt/homebrew/bin/pwsh"];

  if (choice === "powershell" || (choice === "auto" && win)) {
    const executable = findExecutable(win ? ["pwsh", "powershell"] : ["pwsh", "powershell"], psExtras);
    if (executable) return { kind: "powershell", executable };
    if (choice === "powershell") throw new Error("PowerShell was requested but pwsh/powershell was not found on PATH.");
  }

  const bashExecutable = findExecutable(["bash", "sh"], bashExtras);
  if (bashExecutable) return { kind: "bash", executable: bashExecutable };

  if (choice === "auto") {
    const psExecutable = findExecutable(win ? ["pwsh", "powershell"] : ["pwsh", "powershell"], psExtras);
    if (psExecutable) return { kind: "powershell", executable: psExecutable };
  }

  throw new Error("No supported shell found. Install Bash/Git Bash or PowerShell 7, or set PATH accordingly.");
}

function buildBashArgs(command: string, cwdPath: string): string[] {
  const wrapped = `set +e\n${command}\n__oppi_exit=$?\n(pwd -P 2>/dev/null || pwd) > ${shellQuote(cwdPath)}\nexit $__oppi_exit`;
  return ["-lc", wrapped];
}

function buildPowerShellArgs(command: string, cwdPath: string): string[] {
  const script = `$global:LASTEXITCODE = $null\n$oppiSuccess = $true\ntry {\n& {\n${command}\n}\n$oppiSuccess = $?\n} catch {\nWrite-Error $_\n$oppiSuccess = $false\n}\ntry {\n(Get-Location).ProviderPath | Set-Content -LiteralPath ${psSingleQuote(cwdPath)} -NoNewline -Encoding UTF8\n} catch {}\nif ($global:LASTEXITCODE -is [int]) { exit $global:LASTEXITCODE }\nif ($oppiSuccess) { exit 0 } else { exit 1 }`;
  const encoded = Buffer.from(script, "utf16le").toString("base64");
  return platform() === "win32"
    ? ["-NoLogo", "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-EncodedCommand", encoded]
    : ["-NoLogo", "-NoProfile", "-NonInteractive", "-EncodedCommand", encoded];
}

function readTail(path: string, maxBytes = INLINE_MAX_BYTES): string {
  try {
    const stats = statSync(path);
    const bytes = Math.min(Math.max(1, maxBytes), stats.size);
    const fd = readFileSync(path);
    if (fd.length <= bytes) return fd.toString("utf8");
    return fd.subarray(fd.length - bytes).toString("utf8");
  } catch {
    return "";
  }
}

function truncateForModel(text: string, totalBytes: number): { content: string; truncated: boolean } {
  const lines = text.split(/\r?\n/);
  let content = text;
  let truncated = totalBytes > INLINE_MAX_BYTES || lines.length > INLINE_MAX_LINES;
  if (lines.length > INLINE_MAX_LINES) content = lines.slice(-INLINE_MAX_LINES).join("\n");
  while (Buffer.byteLength(content, "utf8") > INLINE_MAX_BYTES) {
    content = content.slice(Math.max(1, Math.floor(content.length * 0.15)));
    truncated = true;
  }
  return { content, truncated };
}

async function readOutputPreview(path: string): Promise<{ content: string; bytes: number; truncated: boolean }> {
  let bytes = 0;
  try { bytes = (await stat(path)).size; } catch { return { content: "", bytes: 0, truncated: false }; }
  const raw = bytes <= INLINE_MAX_BYTES ? await readFile(path, "utf8") : readTail(path, Math.max(INLINE_MAX_BYTES * 2, TAIL_SAMPLE_BYTES));
  const truncated = truncateForModel(raw, bytes);
  return { content: truncated.content.trimEnd(), bytes, truncated: truncated.truncated || bytes > Buffer.byteLength(raw, "utf8") };
}

function classifyStall(tail: string): string | undefined {
  const lowered = tail.toLowerCase();
  if (/\b(y\/n|yes\/no|are you sure|continue\?|press enter|overwrite|password:|passphrase:)\b/i.test(lowered)) {
    return "Output looks like an interactive prompt; kill and rerun non-interactively if it stalls.";
  }
  return undefined;
}

function trimTasks(): void {
  while (tasks.size > MAX_TASKS) {
    const first = tasks.keys().next().value as string | undefined;
    if (!first) return;
    const task = tasks.get(first);
    if (task?.status === "running" || task?.status === "backgrounded") return;
    tasks.delete(first);
  }
}

function killProcessTree(task: ShellTask, signal: NodeJS.Signals = "SIGTERM"): void {
  const child = task.child;
  if (!child?.pid) return;
  try {
    if (platform() === "win32") {
      spawn("taskkill", ["/pid", String(child.pid), "/t", "/f"], { stdio: "ignore", windowsHide: true });
    } else {
      try { process.kill(-child.pid, signal); }
      catch { process.kill(child.pid, signal); }
    }
  } catch {
    // Best effort kill.
  }
}

function resolveRunCwd(ctx: ExtensionContext, requested?: string): string {
  const base = sessionCwd || ctx.cwd;
  return requested ? resolve(base, requested) : base;
}

function startShellCommand(ctx: ExtensionContext, params: ShellExecInput, choice: ShellChoice, runCwd: string): ShellTask {
  const shell = resolveShell(choice);
  const id = taskId();
  const outputDir = ensureOutputDir(ctx.cwd);
  const outputPathAbs = join(outputDir, `${id}.log`);
  const cwdPath = join(tmpdir(), `${id}.cwd`);
  const output = createWriteStream(outputPathAbs, { flags: "a" });
  const args = shell.kind === "powershell" ? buildPowerShellArgs(params.command, cwdPath) : buildBashArgs(params.command, cwdPath);
  const child = spawn(shell.executable, args, {
    cwd: runCwd,
    detached: true,
    env: {
      ...process.env,
      CI: process.env.CI ?? "1",
      GIT_EDITOR: process.env.GIT_EDITOR ?? "true",
      CLAUDECODE: process.env.CLAUDECODE ?? "1",
      OPPI: "1",
    },
    stdio: ["ignore", "pipe", "pipe"],
    windowsHide: true,
  });

  const now = new Date();
  const task: ShellTask = {
    taskId: id,
    shell: shell.kind,
    command: params.command,
    description: params.description,
    status: "running",
    cwdBefore: runCwd,
    outputPath: displayPath(ctx.cwd, outputPathAbs),
    outputPathAbs,
    outputBytes: 0,
    truncated: false,
    startedAt: now.toISOString(),
    child,
    cwdPath,
    lastOutputBytes: 0,
    lastOutputAt: Date.now(),
  };

  tasks.set(id, task);
  registerBackgroundTask({
    id,
    kind: "shell",
    source: "shell_exec",
    title: params.description || params.command,
    description: params.description,
    command: params.command,
    cwd: runCwd,
    outputPath: task.outputPath,
    outputBytes: 0,
    status: task.status,
    startedAt: task.startedAt,
    cancel: () => killProcessTree(task),
    readOutput: (maxBytes) => readTail(outputPathAbs, maxBytes),
  });
  trimTasks();

  child.stdout.on("data", (chunk: Buffer) => output.write(chunk));
  child.stderr.on("data", (chunk: Buffer) => output.write(chunk));
  child.on("error", (error) => {
    task.status = "failed";
    task.exitCode = null;
    task.completedAt = new Date().toISOString();
    task.durationMs = Date.parse(task.completedAt) - Date.parse(task.startedAt);
    output.write(`\n[spawn error] ${error instanceof Error ? error.message : String(error)}\n`);
    output.end();
    updateBackgroundTask(id, {
      status: task.status,
      completedAt: task.completedAt,
      outputBytes: task.outputBytes,
      metadata: { exitCode: null, error: error instanceof Error ? error.message : String(error) },
    });
  });
  child.on("exit", async (code, signal) => {
    output.end();
    task.child = undefined;
    task.exitCode = code;
    task.signal = signal;
    task.completedAt = new Date().toISOString();
    task.durationMs = Date.parse(task.completedAt) - Date.parse(task.startedAt);
    try { task.outputBytes = (await stat(outputPathAbs)).size; } catch { task.outputBytes = 0; }
    task.truncated = task.outputBytes > INLINE_MAX_BYTES;
    if (task.status === "killed" || task.status === "timed_out") {
      // Preserve explicit terminal state.
    } else if (code === 0) {
      task.status = "completed";
    } else {
      task.status = "failed";
    }
    if (task.status !== "backgrounded") {
      try {
        const cwdAfter = (await readFile(cwdPath, "utf8")).trim();
        if (cwdAfter) {
          task.cwdAfter = cwdAfter;
          if (!params.run_in_background && !task.backgrounded) sessionCwd = cwdAfter;
        }
      } catch {
        // Command may have exited before cwd tracking, e.g. explicit exit.
      }
    }
    try { await unlink(cwdPath); } catch {}
    updateBackgroundTask(id, {
      status: task.status,
      completedAt: task.completedAt,
      outputBytes: task.outputBytes,
      outputPath: task.outputPath,
      cwd: task.cwdAfter || task.cwdBefore,
      stalledHint: task.stalledHint,
      metadata: { exitCode: code, signal, durationMs: task.durationMs },
    });
  });

  child.unref?.();
  return task;
}

function formatTaskLine(task: ShellTask): string {
  const elapsed = task.completedAt ? `${Math.max(0, Math.round((Date.parse(task.completedAt) - Date.parse(task.startedAt)) / 1000))}s` : `${Math.max(0, Math.round((Date.now() - Date.parse(task.startedAt)) / 1000))}s`;
  const code = task.exitCode === undefined ? "" : ` code=${task.exitCode}`;
  const desc = task.description ? ` — ${task.description}` : "";
  return `- ${task.taskId}: ${task.status} ${task.shell}${code} ${elapsed}${desc}\n  ${task.command}\n  output: ${task.outputPath}`;
}

function buildResultText(task: ShellTask, preview: { content: string; bytes: number; truncated: boolean }): string {
  const header = [
    `shell_exec ${task.status} (${task.shell})`,
    `task: ${task.taskId}`,
    `cwd: ${task.cwdBefore}${task.cwdAfter && task.cwdAfter !== task.cwdBefore ? ` → ${task.cwdAfter}` : ""}`,
    `output: ${task.outputPath}`,
    task.exitCode !== undefined ? `exit: ${task.exitCode}${task.signal ? ` signal ${task.signal}` : ""}` : undefined,
  ].filter(Boolean).join("\n");
  const body = preview.content || "(no output)";
  const truncation = preview.truncated ? `\n\n[Output truncated: showing tail of ${preview.bytes} bytes. Full output saved to ${task.outputPath}]` : "";
  const stall = task.stalledHint ? `\n\n[Stall hint: ${task.stalledHint}]` : "";
  return `${header}\n\n${body}${truncation}${stall}`;
}

function renderShellCall(args: Partial<ShellExecInput> | undefined, theme: Theme): Text {
  const shell = args?.shell || "auto";
  const background = args?.run_in_background ? " bg" : "";
  const timeout = args?.timeout ? ` timeout ${args.timeout}s` : "";
  const command = typeof args?.command === "string" ? args.command : "...";
  return new Text(`${theme.fg("toolTitle", theme.bold(`shell_exec ${shell}${background}${timeout}`))} ${theme.fg("toolOutput", command)}`, 0, 0);
}

function renderShellResult(result: { content?: Array<{ text?: string }>; details?: ShellExecDetails }, options: ToolRenderResultOptions, theme: Theme): Text {
  const details = result.details;
  if (!details) return new Text(theme.fg("muted", "shell_exec"), 0, 0);
  const statusColor = details.status === "completed" ? "success" : details.status === "running" || details.status === "backgrounded" ? "warning" : "error";
  const lines = [
    `${theme.fg(statusColor as any, details.status)} ${theme.fg("muted", details.taskId)} ${theme.fg("toolOutput", details.shell)}`,
    `${theme.fg("muted", "output:")} ${details.outputPath}`,
  ];
  if (details.exitCode !== undefined) lines.push(`${theme.fg("muted", "exit:")} ${details.exitCode}`);
  if (details.truncated) lines.push(theme.fg("warning", "output truncated; full log saved"));
  const text = result.content?.map((item) => item.text || "").join("\n") || "";
  if (options.expanded && text) lines.push("", theme.fg("toolOutput", text));
  return new Text(lines.join("\n"), 0, 0);
}

export default function shellToolExtension(pi: ExtensionAPI) {
  pi.registerTool({
    name: "shell_exec",
    label: "Shell",
    description:
      "Execute a Bash/POSIX or PowerShell command with bounded output, timeout handling, cwd tracking, and optional backgrounding. Use dedicated read/search/edit/write tools for ordinary file operations.",
    promptSnippet: "Execute Bash or PowerShell commands for builds, tests, git, package managers, Docker, and shell-native diagnostics.",
    promptGuidelines: [
      "Use shell_exec for terminal-native operations such as builds, tests, git, package managers, Docker, and shell diagnostics; prefer read, grep, find, ls, edit, and write for ordinary file operations.",
      "When using shell_exec, provide a concise active-voice description for nontrivial commands and set run_in_background only when the immediate result is not needed.",
      "When using shell_exec on Windows, choose shell=powershell for PowerShell syntax and shell=bash only for Git Bash/POSIX syntax; quote paths with spaces.",
      "Do not use shell_exec for interactive prompts, password entry, polling sleep loops, destructive git operations, or production deploys unless the user explicitly requested them.",
    ],
    parameters: shellExecSchema,
    async execute(_toolCallId, params: ShellExecInput, signal, onUpdate, ctx) {
      const choice = normalizeShellChoice(params.shell);
      const timeoutSeconds = resolveTimeoutSeconds(params.timeout);
      const runCwd = resolveRunCwd(ctx, params.cwd);
      if (!existsSync(runCwd)) throw new Error(`Working directory does not exist: ${runCwd}`);
      if (params.dangerouslyDisableSandbox) {
        ctx.ui?.notify?.("shell_exec: dangerouslyDisableSandbox was requested; Stage 1 has no native sandbox boundary.", "warning");
      }

      const task = startShellCommand(ctx, params, choice, runCwd);
      onUpdate?.({ content: [{ type: "text", text: `Started ${task.shell} task ${task.taskId}` }], details: { ...task, child: undefined, outputPathAbs: undefined, cwdPath: undefined } });

      if (params.run_in_background) {
        task.status = "backgrounded";
        task.backgrounded = true;
        updateBackgroundTask(task.taskId, { status: "backgrounded", metadata: { runInBackground: true } });
        return {
          content: [{ type: "text", text: `Started background shell task ${task.taskId}. Output: ${task.outputPath}` }],
          details: { ...task, child: undefined, outputPathAbs: undefined, cwdPath: undefined },
        };
      }

      let progressTimer: NodeJS.Timeout | undefined;
      let timeoutTimer: NodeJS.Timeout | undefined;
      const cleanup = () => {
        if (progressTimer) clearInterval(progressTimer);
        if (timeoutTimer) clearTimeout(timeoutTimer);
        signal?.removeEventListener("abort", onAbort);
      };
      const onAbort = () => {
        task.status = "killed";
        killProcessTree(task);
      };
      signal?.addEventListener("abort", onAbort, { once: true });

      progressTimer = setInterval(async () => {
        try {
          const size = (await stat(task.outputPathAbs)).size;
          task.outputBytes = size;
          if (size !== task.lastOutputBytes) {
            task.lastOutputBytes = size;
            task.lastOutputAt = Date.now();
          } else if (Date.now() - task.lastOutputAt > TASK_STALL_AFTER_MS) {
            task.stalledHint = classifyStall(readTail(task.outputPathAbs, TAIL_SAMPLE_BYTES));
          }
          if (size > BG_OUTPUT_MAX_BYTES) {
            task.status = "killed";
            updateBackgroundTask(task.taskId, { status: "killed", outputBytes: size, stalledHint: "Output exceeded background safety limit." });
            killProcessTree(task);
          }
          updateBackgroundTask(task.taskId, { status: task.status, outputBytes: size, stalledHint: task.stalledHint });
          const tail = readTail(task.outputPathAbs, TAIL_SAMPLE_BYTES).trimEnd();
          onUpdate?.({
            content: [{ type: "text", text: tail || `Running ${task.taskId}...` }],
            details: { ...task, child: undefined, outputPathAbs: undefined, cwdPath: undefined },
          });
        } catch {
          // Best effort progress only.
        }
      }, PROGRESS_INTERVAL_MS);

      let resolveForeground: (() => void) | undefined;
      const foregroundDone = new Promise<void>((resolveDone) => {
        resolveForeground = resolveDone;
        task.child?.once("exit", () => resolveDone());
        task.child?.once("error", () => resolveDone());
      });

      timeoutTimer = setTimeout(() => {
        if (task.status !== "running") return;
        task.timedOut = true;
        if (params.background_on_timeout) {
          task.status = "backgrounded";
          task.backgrounded = true;
          updateBackgroundTask(task.taskId, { status: "backgrounded", metadata: { backgroundedOnTimeout: true, timeoutSeconds } });
          resolveForeground?.();
        } else {
          task.status = "timed_out";
          updateBackgroundTask(task.taskId, { status: "timed_out", completedAt: new Date().toISOString(), metadata: { timeoutSeconds } });
          killProcessTree(task);
        }
      }, timeoutSeconds * 1000);

      await foregroundDone;
      cleanup();

      if (signal?.aborted && task.status !== "killed") task.status = "killed";
      const preview = await readOutputPreview(task.outputPathAbs);
      task.outputBytes = preview.bytes;
      task.truncated = preview.truncated;
      const safeDetails = { ...task, child: undefined, outputPathAbs: undefined, cwdPath: undefined };

      if (task.status === "backgrounded") {
        return {
          content: [{ type: "text", text: `Command timed out after ${timeoutSeconds}s and was backgrounded as ${task.taskId}. Output: ${task.outputPath}` }],
          details: safeDetails,
        };
      }

      const text = buildResultText(task, preview);
      if (task.status === "failed" || task.status === "timed_out" || task.status === "killed") {
        throw new Error(text);
      }
      return { content: [{ type: "text", text }], details: safeDetails };
    },
    renderCall(args, theme) {
      return renderShellCall(args as Partial<ShellExecInput>, theme);
    },
    renderResult(result, options, theme) {
      return renderShellResult(result as any, options, theme);
    },
  });

  pi.registerTool({
    name: "shell_task",
    label: "Shell Task",
    description: "List, read, or kill background shell_exec tasks.",
    promptSnippet: "Inspect or kill background shell_exec tasks.",
    promptGuidelines: [
      "Use shell_task read to inspect a background shell_exec task's output instead of polling with repeated shell commands.",
      "Use shell_task kill when a background shell_exec task is stalled, interactive, or no longer needed.",
    ],
    parameters: shellTaskSchema,
    async execute(_toolCallId, params: ShellTaskInput) {
      if (params.action === "list") {
        const list = [...tasks.values()].slice(-20).map(formatTaskLine).join("\n");
        return { content: [{ type: "text", text: list || "No shell tasks recorded." }], details: { count: tasks.size } };
      }

      const id = params.taskId;
      if (!id) throw new Error(`taskId is required for shell_task ${params.action}.`);
      const task = tasks.get(id);
      if (!task) throw new Error(`Unknown shell task: ${id}`);

      if (params.action === "kill") {
        if (task.status !== "running" && task.status !== "backgrounded") {
          return { content: [{ type: "text", text: `Task ${id} is already ${task.status}.` }], details: { taskId: id, status: task.status } };
        }
        task.status = "killed";
        updateBackgroundTask(id, { status: "killed", completedAt: new Date().toISOString() });
        killProcessTree(task);
        return { content: [{ type: "text", text: `Kill requested for shell task ${id}.` }], details: { taskId: id, status: task.status } };
      }

      const maxBytes = Math.min(100_000, Math.max(1_000, Math.floor(Number(params.maxBytes ?? INLINE_MAX_BYTES))));
      const preview = await readOutputPreview(task.outputPathAbs);
      const tail = readTail(task.outputPathAbs, maxBytes).trimEnd();
      const details = { ...task, child: undefined, outputPathAbs: undefined, cwdPath: undefined };
      return {
        content: [{ type: "text", text: `${formatTaskLine(task)}\n\n${tail || "(no output)"}${preview.truncated ? `\n\n[Full output: ${task.outputPath}]` : ""}` }],
        details,
      };
    },
  });

  pi.on("session_start", (_event, ctx) => {
    sessionCwd = ctx.cwd;
  });

  pi.on("session_shutdown", () => {
    for (const task of tasks.values()) {
      if (task.status === "running" || task.status === "backgrounded") {
        task.status = "killed";
        killProcessTree(task);
      }
    }
  });
}
