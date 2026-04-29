import { StringEnum } from "@mariozechner/pi-ai";
import type { ExtensionAPI, ExtensionContext, Theme } from "@mariozechner/pi-coding-agent";
import { Container, matchesKey, Text, truncateToWidth } from "@mariozechner/pi-tui";
import { Type, type Static } from "typebox";

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

type TodoDetails = {
  todos: TodoItem[];
  summary: string;
  updatedAt: string;
};

const TodoStatusSchema = StringEnum(["pending", "in_progress", "completed", "blocked", "cancelled"] as const, {
  description: "Todo status.",
});
const TodoPrioritySchema = StringEnum(["low", "medium", "high"] as const, {
  description: "Optional priority.",
});

const TodoItemSchema = Type.Object(
  {
    id: Type.String({ description: "Stable short identifier, e.g. '1' or 'tests'." }),
    content: Type.String({ description: "Short action-oriented task text." }),
    status: TodoStatusSchema,
    priority: Type.Optional(TodoPrioritySchema),
    phase: Type.Optional(Type.String({ description: "Optional phase/group label." })),
    notes: Type.Optional(Type.String({ description: "Optional concise note, blocker, or result." })),
  },
  { additionalProperties: false },
);

const TodoWriteParams = Type.Object(
  {
    todos: Type.Array(TodoItemSchema, {
      description: "The full current todo list. Send the complete list each time, not a patch.",
    }),
    summary: Type.Optional(Type.String({ description: "Brief explanation of what changed." })),
  },
  { additionalProperties: false },
);

type TodoWriteInput = Static<typeof TodoWriteParams>;

let todos: TodoItem[] = [];

const TODO_WIDGET_KEY = "oppi-todos";

function normalizeTodos(input: TodoItem[]): TodoItem[] {
  const seen = new Set<string>();
  const result: TodoItem[] = [];
  for (let i = 0; i < input.length; i++) {
    const raw = input[i];
    const id = String(raw.id || i + 1).trim() || String(i + 1);
    const deduped = seen.has(id) ? `${id}-${i + 1}` : id;
    seen.add(deduped);
    result.push({
      id: deduped,
      content: String(raw.content || "").trim().slice(0, 300) || "Untitled task",
      status: raw.status,
      priority: raw.priority,
      phase: raw.phase?.trim() || undefined,
      notes: raw.notes?.trim().slice(0, 500) || undefined,
    });
  }
  return result;
}

function counts(items: TodoItem[]): Record<TodoStatus, number> {
  return {
    pending: items.filter((todo) => todo.status === "pending").length,
    in_progress: items.filter((todo) => todo.status === "in_progress").length,
    completed: items.filter((todo) => todo.status === "completed").length,
    blocked: items.filter((todo) => todo.status === "blocked").length,
    cancelled: items.filter((todo) => todo.status === "cancelled").length,
  };
}

function statusIcon(status: TodoStatus, theme: Theme): string {
  switch (status) {
    case "completed": return theme.fg("success", "✓");
    case "in_progress": return theme.fg("accent", "●");
    case "blocked": return theme.fg("warning", "!");
    case "cancelled": return theme.fg("dim", "×");
    default: return theme.fg("dim", "○");
  }
}

function priorityLabel(priority: TodoPriority | undefined, theme: Theme): string {
  if (!priority) return "";
  const color = priority === "high" ? "warning" : priority === "medium" ? "muted" : "dim";
  return theme.fg(color as any, priority);
}

function renderTodoLine(todo: TodoItem, theme: Theme, width: number): string {
  const phase = todo.phase ? theme.fg("dim", `[${todo.phase}] `) : "";
  const prio = priorityLabel(todo.priority, theme);
  const suffix = [prio, todo.notes ? theme.fg("dim", `— ${todo.notes}`) : ""].filter(Boolean).join(" ");
  const content = todo.status === "completed" || todo.status === "cancelled"
    ? theme.fg("dim", todo.content)
    : theme.fg("text", todo.content);
  return truncateToWidth(`  ${statusIcon(todo.status, theme)} ${theme.fg("accent", todo.id)} ${phase}${content}${suffix ? ` ${suffix}` : ""}`, width);
}

function summaryFor(items: TodoItem[]): string {
  const c = counts(items);
  const parts = [`${items.length} total`, `${c.completed} done`];
  if (c.in_progress) parts.push(`${c.in_progress} active`);
  if (c.blocked) parts.push(`${c.blocked} blocked`);
  return parts.join(", ");
}

function reconstructState(ctx: ExtensionContext): void {
  todos = [];
  for (const entry of ctx.sessionManager.getBranch()) {
    if (entry.type !== "message") continue;
    const msg = entry.message as any;
    if (msg.role !== "toolResult" || msg.toolName !== "todo_write") continue;
    const details = msg.details as TodoDetails | undefined;
    if (Array.isArray(details?.todos)) todos = details.todos;
  }
}

class TodoListComponent {
  constructor(private theme: Theme, private onClose: () => void) {}

  handleInput(data: string): void {
    if (matchesKey(data, "escape") || matchesKey(data, "ctrl+c")) this.onClose();
  }

  render(width: number): string[] {
    const theme = this.theme;
    const lines = [
      theme.fg("accent", "─".repeat(width)),
      truncateToWidth(`${theme.fg("accent", theme.bold("OPPi todos"))} ${theme.fg("dim", summaryFor(todos))}`, width),
      "",
    ];

    if (todos.length === 0) {
      lines.push(truncateToWidth(`  ${theme.fg("dim", "No todos yet. For multi-step work, OPPi will use todo_write.")}`, width));
    } else {
      for (const todo of todos) lines.push(renderTodoLine(todo, theme, width));
    }

    lines.push("", theme.fg("dim", "Esc close • /todos clear • /todos done <id>"), theme.fg("accent", "─".repeat(width)));
    return lines.map((line) => truncateToWidth(line, width));
  }

  invalidate(): void {}
}

function visibleDockTodos(): TodoItem[] {
  const active = todos.filter((todo) => todo.status === "in_progress");
  const blocked = todos.filter((todo) => todo.status === "blocked");
  const pending = todos.filter((todo) => todo.status === "pending");
  const completed = todos.filter((todo) => todo.status === "completed");
  const cancelled = todos.filter((todo) => todo.status === "cancelled");
  return [...active, ...blocked, ...pending, ...completed, ...cancelled].slice(0, 4);
}

class TodoDockComponent {
  constructor(private theme: Theme) {}

  render(width: number): string[] {
    if (todos.length === 0) return [];

    const theme = this.theme;
    const c = counts(todos);
    const remaining = c.pending + c.in_progress + c.blocked;
    const status = [
      c.in_progress ? `${c.in_progress} cooking` : undefined,
      c.pending ? `${c.pending} left` : undefined,
      c.blocked ? `${c.blocked} blocked` : undefined,
      c.completed ? `${c.completed} cooked` : undefined,
    ].filter(Boolean).join(" · ") || "all cooked";

    const visible = visibleDockTodos();
    const visibleRemaining = visible.filter((todo) => todo.status !== "completed" && todo.status !== "cancelled").length;
    const lines = [
      truncateToWidth(`${theme.fg("borderAccent", "╭─")} ${theme.fg("accent", theme.bold("OPPi cooking"))} ${theme.fg("dim", status)}`, width),
    ];

    for (const todo of visible) {
      const icon = statusIcon(todo.status, theme);
      const phase = todo.phase ? theme.fg("dim", `[${todo.phase}] `) : "";
      const content = todo.status === "completed" || todo.status === "cancelled"
        ? theme.fg("dim", todo.content)
        : theme.fg("text", todo.content);
      const label = `${theme.fg("border", "│")} ${icon} ${theme.fg("accent", todo.id)} ${phase}${content}`;
      lines.push(truncateToWidth(label, width));
    }

    if (remaining > visibleRemaining) {
      lines.push(truncateToWidth(`${theme.fg("border", "│")} ${theme.fg("dim", `… ${remaining - visibleRemaining} more active/left`)}`, width));
    }

    lines.push(truncateToWidth(`${theme.fg("borderAccent", "╰─")} ${theme.fg("dim", "/todos for details")}`, width));
    return lines.slice(0, 7);
  }

  invalidate(): void {}
}

function updateTodoWidget(ctx: ExtensionContext): void {
  if (!ctx.hasUI) return;
  if (todos.length === 0) {
    ctx.ui.setWidget(TODO_WIDGET_KEY, undefined);
    return;
  }
  ctx.ui.setWidget(TODO_WIDGET_KEY, (_tui, theme) => new TodoDockComponent(theme), { placement: "aboveEditor" });
}

export default function todoWriteExtension(pi: ExtensionAPI) {
  pi.on("session_start", async (_event, ctx) => {
    reconstructState(ctx);
    updateTodoWidget(ctx);
  });
  pi.on("session_tree", async (_event, ctx) => {
    reconstructState(ctx);
    updateTodoWidget(ctx);
  });

  pi.registerTool({
    name: "todo_write",
    label: "todo_write",
    description: "Create or update the current task todo list. Always provide the full current list. OPPi should maintain this proactively as work evolves.",
    promptSnippet: "Use todo_write to proactively maintain a concise phase/task todo list for multi-step work.",
    promptGuidelines: [
      "OPPi owns the visible todo list during multi-step work: create it, update it, add newly discovered tasks, and tick items off without waiting for the user to ask.",
      "Use todo_write for multi-step coding tasks, refactors, debugging sessions, and plans with several dependent steps.",
      "Do not use todo_write for tiny one-shot tasks.",
      "Always send the full current todo list, not just changed items.",
      "Keep todo content short and action-oriented.",
      "At most one or two todos should be in_progress unless work is genuinely parallel.",
      "When starting a task, mark it in_progress; when it is finished, mark it completed before moving on or giving the final answer.",
      "When completing a todo, put the concrete outcome in notes when useful so OPPi's scoped compactor can preserve it for the final response.",
      "After OPPi performs scoped compaction, completed/cancelled todo outcomes are archived in the compacted summary; future todo_write calls may omit those archived completed/cancelled items and keep only active, blocked, and pending work visible.",
      "When preparing a progress or final response, include completed outcomes in the user-facing message and then call todo_write with completed/cancelled items pruned (or an empty list if nothing actionable remains), unless those items are still useful context.",
      "If the plan changes, add new todos, mark obsolete ones cancelled, and explain the change briefly in the todo_write summary.",
    ],
    parameters: TodoWriteParams,
    renderShell: "self",
    async execute(_toolCallId, params: TodoWriteInput, _signal, _onUpdate, ctx) {
      todos = normalizeTodos(params.todos as TodoItem[]);
      updateTodoWidget(ctx);
      const details: TodoDetails = {
        todos,
        summary: params.summary?.trim() || summaryFor(todos),
        updatedAt: new Date().toISOString(),
      };
      const lines = [`Updated todos: ${details.summary}`];
      for (const todo of todos) lines.push(`- [${todo.status}] ${todo.id}: ${todo.content}`);
      return { content: [{ type: "text", text: lines.join("\n") }], details };
    },
    renderCall(args, theme, context) {
      if (!context.isPartial || context.expanded) return new Container();
      const count = Array.isArray(args.todos) ? args.todos.length : 0;
      const summary = typeof args.summary === "string" && args.summary.trim() ? ` — ${args.summary.trim()}` : "";
      return new Text(`${theme.fg("toolTitle", theme.bold("Updating plan"))} ${theme.fg("muted", `${count} item${count === 1 ? "" : "s"}${summary}`)}`, 0, 0);
    },
    renderResult(result, { expanded }, theme) {
      const details = result.details as TodoDetails | undefined;
      if (!details) return new Text(`${theme.fg("success", "✓")} ${theme.fg("toolOutput", "Plan updated")}`, 0, 0);
      const summary = details.summary ? `Plan updated: ${details.summary}` : "Plan updated";
      if (!expanded) return new Text(`${theme.fg("success", "✓")} ${theme.fg("toolOutput", summary)}`, 0, 0);
      const lines = [theme.fg("success", `✓ ${summary}`), ...details.todos.map((todo) => renderTodoLine(todo, theme, 120))];
      return new Text(lines.join("\n"), 0, 0);
    },
  });

  pi.registerCommand("todos", {
    description: "Show or edit current OPPi todos. Usage: /todos [clear|done <id>]",
    handler: async (args, ctx) => {
      const [action, id] = args.trim().split(/\s+/);

      if (action === "clear") {
        if (todos.length === 0) {
          ctx.ui.notify("No todos to clear.", "info");
          return;
        }
        if (ctx.hasUI && !(await ctx.ui.confirm("Clear todos?", `Clear ${todos.length} current todo(s)?`))) return;
        todos = [];
        updateTodoWidget(ctx);
        ctx.ui.notify("Cleared todos for this session view.", "info");
        return;
      }

      if (action === "done") {
        const todo = todos.find((item) => item.id === id);
        if (!todo) {
          ctx.ui.notify(`Todo '${id ?? ""}' not found.`, "warning");
          return;
        }
        todo.status = "completed";
        updateTodoWidget(ctx);
        ctx.ui.notify(`Marked ${todo.id} done.`, "info");
        return;
      }

      if (!ctx.hasUI) {
        ctx.ui.notify(todos.length ? todos.map((todo) => `${todo.status} ${todo.id}: ${todo.content}`).join("\n") : "No todos.", "info");
        return;
      }
      await ctx.ui.custom<void>((_tui, theme, _kb, done) => new TodoListComponent(theme, () => done()));
    },
  });
}
