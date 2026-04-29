import type { ExtensionAPI, ExtensionCommandContext, ExtensionContext, Theme } from "@mariozechner/pi-coding-agent";
import { Text, truncateToWidth, type Component } from "@mariozechner/pi-tui";

export type BackgroundTaskStatus = "running" | "backgrounded" | "completed" | "failed" | "killed" | "timed_out";

export type BackgroundTaskRecord = {
  id: string;
  kind: string;
  source: string;
  title: string;
  description?: string;
  command?: string;
  cwd?: string;
  outputPath?: string;
  outputBytes?: number;
  status: BackgroundTaskStatus;
  startedAt: string;
  completedAt?: string;
  stalledHint?: string;
  metadata?: Record<string, unknown>;
  cancel?: () => void;
  readOutput?: (maxBytes: number) => string;
};

type Notice = {
  id: string;
  text: string;
  level: "success" | "warning" | "error";
  expiresAt: number;
};

const WIDGET_KEY = "oppi.background.status";
const COMPLETION_NOTICE_MS = 10_000;
const MAX_COMPLETED_IN_PANEL = 20;
// Alt+Shift+letter is not reliably encoded by legacy terminals/VS Code's
// integrated terminal unless enhanced keyboard protocols are active. Ctrl+Alt+B
// has a legacy ESC+Ctrl-B form that Pi's key parser recognizes.
const BACKGROUND_TASKS_SHORTCUT = "ctrl+alt+b";
const BACKGROUND_TASKS_SHORTCUT_LABEL = "Ctrl+Alt+B";
const contexts = new Set<ExtensionContext>();
const tasks = new Map<string, BackgroundTaskRecord>();
const notices: Notice[] = [];
let refreshTimer: NodeJS.Timeout | undefined;

function runningTasks(): BackgroundTaskRecord[] {
  return [...tasks.values()].filter((task) => task.status === "running" || task.status === "backgrounded");
}

function completedTasks(): BackgroundTaskRecord[] {
  return [...tasks.values()]
    .filter((task) => task.status !== "running" && task.status !== "backgrounded")
    .sort((a, b) => Date.parse(b.completedAt || b.startedAt) - Date.parse(a.completedAt || a.startedAt));
}

function activeNotices(): Notice[] {
  const now = Date.now();
  for (let i = notices.length - 1; i >= 0; i--) {
    if (notices[i].expiresAt <= now) notices.splice(i, 1);
  }
  return notices;
}

function duration(task: BackgroundTaskRecord): string {
  const end = task.completedAt ? Date.parse(task.completedAt) : Date.now();
  const start = Date.parse(task.startedAt);
  const seconds = Math.max(0, Math.round((end - start) / 1000));
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  const rest = seconds % 60;
  if (minutes < 60) return `${minutes}m${rest.toString().padStart(2, "0")}s`;
  const hours = Math.floor(minutes / 60);
  return `${hours}h${(minutes % 60).toString().padStart(2, "0")}m`;
}

function statusIcon(status: BackgroundTaskStatus): string {
  switch (status) {
    case "completed": return "✓";
    case "failed": return "!";
    case "killed": return "✗";
    case "timed_out": return "⏱";
    case "backgrounded": return "↺";
    default: return "⏳";
  }
}

function statusColor(status: BackgroundTaskStatus): string {
  switch (status) {
    case "completed": return "success";
    case "running":
    case "backgrounded": return "warning";
    default: return "error";
  }
}

function shortLabel(task: BackgroundTaskRecord): string {
  return task.description || task.title || task.command || task.id;
}

class BackgroundStatusWidget implements Component {
  constructor(private theme: Theme) {}

  render(width: number): string[] {
    const running = runningTasks();
    const currentNotices = activeNotices();
    const lines: string[] = [];
    if (running.length > 0) {
      const preview = running.slice(0, 3).map((task) => `${shortLabel(task)} ${duration(task)}`).join(" · ");
      const more = running.length > 3 ? ` · +${running.length - 3}` : "";
      lines.push(truncateToWidth(`${this.theme.fg("warning", "bg:")} ${running.length} running · ${this.theme.fg("dim", `${BACKGROUND_TASKS_SHORTCUT_LABEL} open · `)}${this.theme.fg("toolOutput", preview)}${more}`, width));
    }
    for (const notice of currentNotices.slice(-2)) {
      lines.push(truncateToWidth(`${this.theme.fg(statusColor(notice.level === "success" ? "completed" : "failed") as any, "bg done:")} ${this.theme.fg("toolOutput", notice.text)}`, width));
    }
    return lines;
  }

  invalidate(): void {}
}

class BackgroundPanel implements Component {
  private selected = 0;
  private expanded = new Set<string>();

  constructor(private theme: Theme, private done: () => void) {}

  private ordered(): BackgroundTaskRecord[] {
    return [...runningTasks(), ...completedTasks().slice(0, MAX_COMPLETED_IN_PANEL)];
  }

  render(width: number): string[] {
    const ordered = this.ordered();
    if (this.selected >= ordered.length) this.selected = Math.max(0, ordered.length - 1);
    const lines = [
      truncateToWidth(`${this.theme.fg("borderAccent", "╭─")} ${this.theme.fg("accent", this.theme.bold("Background tasks"))} ${this.theme.fg("dim", `${BACKGROUND_TASKS_SHORTCUT_LABEL} · c cancel · Enter expand · x clear done · Esc close`)}`, width),
    ];

    const running = runningTasks();
    const completed = completedTasks().slice(0, MAX_COMPLETED_IN_PANEL);
    const sections: Array<[string, BackgroundTaskRecord[]]> = [["Running", running], ["Completed", completed]];
    let globalIndex = 0;
    for (const [label, group] of sections) {
      if (group.length === 0 && label === "Completed") continue;
      lines.push(truncateToWidth(`${this.theme.fg("border", "│")} ${this.theme.fg("dim", label)}`, width));
      if (group.length === 0) {
        lines.push(truncateToWidth(`${this.theme.fg("border", "│")}   ${this.theme.fg("dim", "No running tasks")}`, width));
      }
      for (const task of group) {
        const active = globalIndex === this.selected;
        const marker = active ? this.theme.fg("accent", "›") : " ";
        const icon = this.theme.fg(statusColor(task.status) as any, statusIcon(task.status));
        const labelText = `${marker} ${icon} ${task.kind}:${shortLabel(task)} ${this.theme.fg("dim", `${task.status} · ${duration(task)}`)}`;
        lines.push(truncateToWidth(`${this.theme.fg("border", "│")} ${labelText}`, width));
        if (this.expanded.has(task.id)) {
          if (task.command) lines.push(truncateToWidth(`${this.theme.fg("border", "│")}   ${this.theme.fg("dim", "cmd:")} ${this.theme.fg("toolOutput", task.command)}`, width));
          if (task.outputPath) lines.push(truncateToWidth(`${this.theme.fg("border", "│")}   ${this.theme.fg("dim", "out:")} ${task.outputPath}`, width));
          if (task.stalledHint) lines.push(truncateToWidth(`${this.theme.fg("border", "│")}   ${this.theme.fg("warning", task.stalledHint)}`, width));
          const output = task.readOutput?.(8_000)?.trimEnd();
          if (output) {
            for (const line of output.split(/\r?\n/).slice(-8)) {
              lines.push(truncateToWidth(`${this.theme.fg("border", "│")}   ${this.theme.fg("toolOutput", line)}`, width));
            }
          }
        }
        globalIndex += 1;
      }
    }
    lines.push(truncateToWidth(`${this.theme.fg("borderAccent", "╰─")} ${this.theme.fg("dim", `${running.length} running · ${completed.length} completed shown`)}`, width));
    return lines.slice(0, 18);
  }

  handleInput(data: string): void {
    const ordered = this.ordered();
    if (data === "\u001b" || data === "q") {
      this.done();
      return;
    }
    if (data === "j" || data === "\u001b[B") {
      this.selected = Math.min(ordered.length - 1, this.selected + 1);
      return;
    }
    if (data === "k" || data === "\u001b[A") {
      this.selected = Math.max(0, this.selected - 1);
      return;
    }
    if (data === "\r" || data === "\n") {
      const task = ordered[this.selected];
      if (!task) return;
      if (this.expanded.has(task.id)) this.expanded.delete(task.id);
      else this.expanded.add(task.id);
      return;
    }
    if (data === "c") {
      const task = ordered[this.selected];
      if (task) cancelBackgroundTask(task.id);
      return;
    }
    if (data === "x") {
      clearCompletedBackgroundTasks();
      this.selected = 0;
    }
  }

  invalidate(): void {}
}

function refreshWidgets(): void {
  for (const ctx of contexts) {
    if (!ctx.hasUI) continue;
    if (runningTasks().length === 0 && activeNotices().length === 0) {
      ctx.ui.setWidget(WIDGET_KEY, undefined, { placement: "belowEditor" });
    } else {
      ctx.ui.setWidget(WIDGET_KEY, (_tui, theme) => new BackgroundStatusWidget(theme), { placement: "belowEditor" });
    }
  }
}

function scheduleRefresh(): void {
  refreshWidgets();
  if (refreshTimer) clearTimeout(refreshTimer);
  if (activeNotices().length > 0) {
    refreshTimer = setTimeout(() => {
      refreshTimer = undefined;
      refreshWidgets();
    }, Math.max(250, Math.min(...notices.map((notice) => notice.expiresAt - Date.now()).filter((ms) => ms > 0))));
  }
}

function addCompletionNotice(task: BackgroundTaskRecord): void {
  const level = task.status === "completed" ? "success" : task.status === "killed" ? "warning" : "error";
  notices.push({
    id: `${task.id}:${Date.now()}`,
    text: `${task.kind}:${shortLabel(task)} ${task.status}`,
    level,
    expiresAt: Date.now() + COMPLETION_NOTICE_MS,
  });
  while (notices.length > 5) notices.shift();
}

export function registerBackgroundTask(task: BackgroundTaskRecord): BackgroundTaskRecord {
  tasks.set(task.id, task);
  scheduleRefresh();
  return task;
}

export function updateBackgroundTask(id: string, patch: Partial<BackgroundTaskRecord>): BackgroundTaskRecord | undefined {
  const existing = tasks.get(id);
  if (!existing) return undefined;
  const previousStatus = existing.status;
  Object.assign(existing, patch);
  const terminal = existing.status !== "running" && existing.status !== "backgrounded";
  if (terminal && (previousStatus === "running" || previousStatus === "backgrounded")) {
    existing.completedAt ??= new Date().toISOString();
    addCompletionNotice(existing);
  }
  scheduleRefresh();
  return existing;
}

export function getBackgroundTask(id: string): BackgroundTaskRecord | undefined {
  return tasks.get(id);
}

export function getBackgroundTasks(): BackgroundTaskRecord[] {
  return [...tasks.values()];
}

export function cancelBackgroundTask(id: string): boolean {
  const task = tasks.get(id);
  if (!task) return false;
  if (task.status !== "running" && task.status !== "backgrounded") return false;
  try { task.cancel?.(); } catch { /* best effort */ }
  updateBackgroundTask(id, { status: "killed", completedAt: new Date().toISOString() });
  return true;
}

export function clearCompletedBackgroundTasks(): void {
  for (const [id, task] of tasks) {
    if (task.status !== "running" && task.status !== "backgrounded") tasks.delete(id);
  }
  scheduleRefresh();
}

async function showBackgroundPanel(ctx: ExtensionCommandContext | ExtensionContext): Promise<void> {
  if (!ctx.hasUI) {
    const lines = getBackgroundTasks().map((task) => `${task.id}: ${task.status} ${task.kind}:${shortLabel(task)}`);
    ctx.ui.notify(lines.join("\n") || "No background tasks recorded.", "info");
    return;
  }
  await ctx.ui.custom<void>((_tui, theme, _keybindings, done) => new BackgroundPanel(theme, () => done()), { overlay: false });
}

export default function backgroundTasksExtension(pi: ExtensionAPI) {
  pi.on("session_start", (_event, ctx) => {
    contexts.add(ctx);
    scheduleRefresh();
  });

  pi.on("session_shutdown", (_event, ctx) => {
    contexts.delete(ctx);
  });

  pi.registerCommand("background", {
    description: "Show OPPi background tasks docked above the input bar.",
    handler: async (_args, ctx) => {
      contexts.add(ctx);
      await showBackgroundPanel(ctx);
    },
  });

  pi.registerShortcut(BACKGROUND_TASKS_SHORTCUT, {
    description: "Open OPPi background tasks",
    handler: async (ctx) => {
      contexts.add(ctx);
      await showBackgroundPanel(ctx);
    },
  });
}
