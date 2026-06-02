import { existsSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { basename, isAbsolute, join, relative } from "node:path";
import type { ExtensionAPI, ExtensionCommandContext } from "@mariozechner/pi-coding-agent";
import { getAgentDir } from "@mariozechner/pi-coding-agent";
import type { AutocompleteItem } from "@mariozechner/pi-tui";

const AGENT_DIR = join(".oppi", "agents");
const CLAUDE_AGENT_DIR = join(".claude", "agents");
const MAX_AGENT_FILES = 800;
const MAX_AGENT_DEPTH = 5;
const IGNORED_DIRS = new Set([".git", "node_modules", "target", "dist", "build", "coverage", ".turbo", ".cache"]);

export type AgentSource = "built-in" | "project" | "user" | "claude-project" | "claude-user";

export type AgentDefinition = {
  name: string;
  description: string;
  prompt: string;
  tools?: string[];
  disallowedTools?: string[];
  model?: string;
  effort?: string;
  permissionMode?: string;
  memory?: string;
  background?: boolean;
  isolation?: string;
  color?: string;
  skills?: string[];
  maxTurns?: number;
  source: AgentSource;
  path: string;
  filename: string;
};

export type FailedAgentFile = {
  path: string;
  source: AgentSource;
  reason: string;
};

export type ResolvedAgent = {
  active: AgentDefinition;
  shadowed: AgentDefinition[];
};

type AgentDir = { source: AgentSource; path: string; writable: boolean; label: string };

type ParsedFrontmatter = Record<string, string>;

const SOURCE_PRIORITY: Record<AgentSource, number> = {
  "built-in": 0,
  "claude-user": 1,
  user: 2,
  "claude-project": 3,
  project: 4,
};

const SOURCE_LABEL: Record<AgentSource, string> = {
  "built-in": "built-in OPPi personality",
  project: "project .oppi/agents",
  user: "personal OPPi agents",
  "claude-project": "imported project .claude/agents",
  "claude-user": "imported personal .claude/agents",
};

function toDisplayPath(cwd: string, path: string): string {
  const rel = relative(cwd, path);
  return rel && !rel.startsWith("..") && !isAbsolute(rel) ? rel.replace(/\\/g, "/") : path.replace(/\\/g, "/");
}

function userHomeAgentDir(agentDir = getAgentDir()): string {
  return join(agentDir, "oppi", "agents");
}

function builtinAgent(name: string, description: string, prompt: string, options: Partial<Omit<AgentDefinition, "name" | "description" | "prompt" | "source" | "path" | "filename">> = {}): AgentDefinition {
  return {
    name,
    description,
    prompt,
    tools: options.tools,
    disallowedTools: options.disallowedTools,
    model: options.model,
    effort: options.effort,
    permissionMode: options.permissionMode,
    memory: options.memory,
    background: options.background,
    isolation: options.isolation,
    color: options.color,
    skills: options.skills,
    maxTurns: options.maxTurns,
    source: "built-in",
    path: `builtin:${name}`,
    filename: `${name}.builtin`,
  };
}

export function builtInAgentDefinitions(): AgentDefinition[] {
  if (/^(1|true|yes|on)$/i.test(process.env.OPPI_DISABLE_BUILTIN_AGENTS ?? "")) return [];
  return [
    builtinAgent(
      "general-purpose",
      "Use for broad codebase research, multi-step investigation, uncertain searches, and general delegated work that does not fit a narrower personality.",
      "You are OPPi's general-purpose coding subagent. Complete the delegated task thoroughly, search broadly when needed, stay within the requested scope, avoid unnecessary files or documentation, and report concise results with any blockers or changed files.",
      { tools: ["*"], model: "coding", color: "cyan" },
    ),
    builtinAgent(
      "Explore",
      "Use for read-only codebase exploration: finding files, symbols, patterns, relevant implementation areas, and repository structure before edits begin.",
      "You are OPPi's read-only exploration specialist. Use fast search and safe shell diagnostics to locate relevant code and explain what you found. Do not create, edit, move, delete, or write project files. Return a concise discovery report with important paths and confidence notes.",
      { tools: ["read", "grep", "find", "ls", "shell_exec"], disallowedTools: ["edit", "write", "Agent", "todo_write"], model: "coding", permissionMode: "read-only", color: "blue" },
    ),
    builtinAgent(
      "Plan",
      "Use for read-only implementation planning: break down a feature or fix, identify critical files, call out trade-offs, and produce concrete steps before coding.",
      "You are OPPi's planning architect. Inspect the repository read-only, understand the requested outcome, design a practical implementation plan, list risks and trade-offs, and finish with the most important files to modify or inspect. Do not modify project files.",
      { tools: ["read", "grep", "find", "ls", "shell_exec"], disallowedTools: ["edit", "write", "Agent", "todo_write"], model: "strong", permissionMode: "read-only", color: "purple" },
    ),
    builtinAgent(
      "oppi-code-guide",
      "Use when the user asks how OPPi, Pi, OPPi extensions, commands, skills, themes, permissions, memory, or the runtime spine work.",
      "You are OPPi's product and documentation guide. Prefer the local OPPi repository, installed Pi documentation, and official project docs. Inspect local configuration read-only when it helps. Give direct, grounded guidance and call out when a feature belongs to a future runtime stage.",
      { tools: ["read", "grep", "find", "ls", "shell_exec"], model: "coding", permissionMode: "read-only", color: "green" },
    ),
    builtinAgent(
      "statusline-setup",
      "Use when the user wants to configure OPPi/Pi terminal status, footer, or prompt-like display behavior.",
      "You are OPPi's status and footer setup specialist. Inspect existing settings and terminal configuration, preserve unrelated settings, prefer minimal reversible edits, and explain exactly what changed. Ask for missing prompt/status details instead of guessing.",
      { tools: ["read", "edit"], model: "strong", permissionMode: "default", color: "orange" },
    ),
    builtinAgent(
      "verification",
      "Use before reporting completion for non-trivial implementation, multi-file changes, backend/API/infrastructure work, or risky changes that need independent evidence.",
      "You are OPPi's adversarial verification specialist. Verify rather than modify. Read instructions, inspect changed files, run relevant checks when available, perform at least one adversarial probe before passing, and report each check with evidence. End with exactly one terminal verdict: VERDICT: pass, VERDICT: fail, or VERDICT: partial.",
      { tools: ["read", "grep", "find", "ls", "shell_exec"], disallowedTools: ["edit", "write", "Agent", "todo_write"], model: "strong", permissionMode: "read-only", background: true, color: "red" },
    ),
  ];
}

export function agentDirectories(cwd: string, agentDir = getAgentDir()): AgentDir[] {
  return [
    { source: "claude-user", path: join(homedir(), ".claude", "agents"), writable: false, label: SOURCE_LABEL["claude-user"] },
    { source: "user", path: userHomeAgentDir(agentDir), writable: true, label: SOURCE_LABEL.user },
    { source: "claude-project", path: join(cwd, CLAUDE_AGENT_DIR), writable: false, label: SOURCE_LABEL["claude-project"] },
    { source: "project", path: join(cwd, AGENT_DIR), writable: true, label: SOURCE_LABEL.project },
  ];
}

function stripQuotes(value: string): string {
  const trimmed = value.trim();
  if ((trimmed.startsWith('"') && trimmed.endsWith('"')) || (trimmed.startsWith("'") && trimmed.endsWith("'"))) {
    const inner = trimmed.slice(1, -1);
    return trimmed.startsWith('"')
      ? inner.replace(/\\n/g, "\n").replace(/\\"/g, '"').replace(/\\\\/g, "\\")
      : inner.replace(/''/g, "'");
  }
  return trimmed.replace(/\\n/g, "\n");
}

function parseFrontmatter(text: string): { frontmatter: ParsedFrontmatter; body: string } | undefined {
  const normalized = text.replace(/\r\n/g, "\n");
  if (!normalized.startsWith("---\n")) return undefined;
  const end = normalized.indexOf("\n---", 4);
  if (end < 0) return undefined;
  const rawFrontmatter = normalized.slice(4, end);
  const body = normalized.slice(end + 4).replace(/^\n/, "").trim();
  const frontmatter: ParsedFrontmatter = {};
  for (const rawLine of rawFrontmatter.split("\n")) {
    const line = rawLine.trim();
    if (!line || line.startsWith("#")) continue;
    const match = line.match(/^([A-Za-z][A-Za-z0-9_-]*)\s*:\s*(.*)$/);
    if (!match) continue;
    frontmatter[match[1]] = stripQuotes(match[2] ?? "");
  }
  return { frontmatter, body };
}

function parseList(value: string | undefined): string[] | undefined {
  if (value === undefined) return undefined;
  const trimmed = value.trim();
  if (!trimmed) return [];
  const withoutBrackets = trimmed.startsWith("[") && trimmed.endsWith("]") ? trimmed.slice(1, -1) : trimmed;
  return withoutBrackets.split(",").map((item) => stripQuotes(item).trim()).filter(Boolean);
}

function parseBoolean(value: string | undefined): boolean | undefined {
  if (value === undefined) return undefined;
  return /^(1|true|yes|on)$/i.test(value.trim());
}

function parsePositiveInteger(value: string | undefined): number | undefined {
  if (value === undefined) return undefined;
  const parsed = Number.parseInt(value, 10);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : undefined;
}

export function parseAgentMarkdown(text: string, path: string, source: AgentSource): AgentDefinition | undefined {
  const parsed = parseFrontmatter(text);
  if (!parsed) return undefined;
  const fm = parsed.frontmatter;
  const name = stripQuotes(fm.name ?? "").trim();
  if (!name) return undefined;
  const description = stripQuotes(fm.description ?? "").trim();
  const prompt = parsed.body.trim();
  return {
    name,
    description,
    prompt,
    tools: parseList(fm.tools),
    disallowedTools: parseList(fm.disallowedTools),
    model: stripQuotes(fm.model ?? "").trim() || undefined,
    effort: stripQuotes(fm.effort ?? "").trim() || undefined,
    permissionMode: stripQuotes(fm.permissionMode ?? "").trim() || undefined,
    memory: stripQuotes(fm.memory ?? "").trim() || undefined,
    background: parseBoolean(fm.background),
    isolation: stripQuotes(fm.isolation ?? "").trim() || undefined,
    color: stripQuotes(fm.color ?? "").trim() || undefined,
    skills: parseList(fm.skills),
    maxTurns: parsePositiveInteger(fm.maxTurns),
    source,
    path,
    filename: basename(path),
  };
}

function quoteYaml(value: string): string {
  return `"${value.replace(/\\/g, "\\\\").replace(/"/g, '\\"').replace(/\n/g, "\\n")}"`;
}

function formatList(values: string[] | undefined): string | undefined {
  if (values === undefined) return undefined;
  return values.map((value) => value.includes(",") || /\s/.test(value) ? quoteYaml(value) : value).join(", ");
}

export function formatAgentMarkdown(agent: Pick<AgentDefinition, "name" | "description" | "prompt"> & Partial<AgentDefinition>): string {
  const lines = [
    "---",
    `name: ${agent.name}`,
    `description: ${quoteYaml(agent.description)}`,
  ];
  const optional: Array<[string, string | undefined]> = [
    ["tools", formatList(agent.tools)],
    ["disallowedTools", formatList(agent.disallowedTools)],
    ["model", agent.model],
    ["effort", agent.effort],
    ["permissionMode", agent.permissionMode],
    ["memory", agent.memory],
    ["background", agent.background ? "true" : undefined],
    ["isolation", agent.isolation],
    ["color", agent.color],
    ["skills", formatList(agent.skills)],
    ["maxTurns", agent.maxTurns ? String(agent.maxTurns) : undefined],
  ];
  for (const [key, value] of optional) {
    if (value !== undefined) lines.push(`${key}: ${value}`);
  }
  lines.push("---", "", agent.prompt.trim(), "");
  return lines.join("\n");
}

export function validateAgent(agent: Pick<AgentDefinition, "name" | "description" | "prompt" | "tools">): { errors: string[]; warnings: string[] } {
  const errors: string[] = [];
  const warnings: string[] = [];
  if (!agent.name.trim()) errors.push("Agent name is required.");
  if (agent.name.length < 3 || agent.name.length >= 50) errors.push("Agent name must be 3-49 characters.");
  if (!/^[A-Za-z0-9][A-Za-z0-9-]*[A-Za-z0-9]$/.test(agent.name)) errors.push("Agent name must use letters, numbers, and hyphens, and start/end alphanumeric.");
  if (!agent.description.trim()) errors.push("Description is required; it tells the main assistant when to use this agent.");
  if (agent.description.trim().length > 1_000) warnings.push("Description is unusually long; routing hints work best when concise.");
  if (!agent.prompt.trim()) errors.push("System prompt body is required.");
  if (agent.prompt.trim().length < 20) errors.push("System prompt should be at least 20 characters.");
  if (agent.prompt.length > 10_000) errors.push("System prompt must be under 10,000 characters.");
  if (agent.tools && agent.tools.length === 0) warnings.push("Tools is explicitly empty; the agent will have no selected tools when a runtime consumes it.");
  if (agent.tools === undefined) warnings.push("Tools omitted; future runtimes should treat this as all-tools after global filters.");
  return { errors, warnings };
}

function collectMarkdownFiles(dir: string, depth = 0, out: string[] = []): string[] {
  if (out.length >= MAX_AGENT_FILES || depth > MAX_AGENT_DEPTH) return out;
  if (!existsSync(dir)) return out;
  let entries: Array<{ name: string; isDirectory(): boolean; isFile(): boolean }>;
  try {
    entries = readdirSync(dir, { withFileTypes: true }) as Array<{ name: string; isDirectory(): boolean; isFile(): boolean }>;
  } catch {
    return out;
  }
  for (const entry of entries) {
    if (out.length >= MAX_AGENT_FILES) break;
    const path = join(dir, entry.name);
    if (entry.isDirectory()) {
      if (!IGNORED_DIRS.has(entry.name)) collectMarkdownFiles(path, depth + 1, out);
      continue;
    }
    if (entry.isFile() && entry.name.toLowerCase().endsWith(".md")) out.push(path);
  }
  return out;
}

export function loadAgentDefinitions(cwd: string, agentDir = getAgentDir()): { agents: AgentDefinition[]; failed: FailedAgentFile[] } {
  const agents: AgentDefinition[] = [];
  const failed: FailedAgentFile[] = [];
  for (const dir of agentDirectories(cwd, agentDir)) {
    for (const path of collectMarkdownFiles(dir.path)) {
      try {
        const text = readFileSync(path, "utf8");
        const parsed = parseAgentMarkdown(text, path, dir.source);
        if (!parsed) continue;
        const validation = validateAgent(parsed);
        if (validation.errors.length > 0) {
          failed.push({ path, source: dir.source, reason: validation.errors.join("; ") });
          continue;
        }
        agents.push(parsed);
      } catch (error) {
        failed.push({ path, source: dir.source, reason: error instanceof Error ? error.message : String(error) });
      }
    }
  }
  return { agents, failed };
}

export function resolveActiveAgents(agents: AgentDefinition[]): ResolvedAgent[] {
  const byName = new Map<string, AgentDefinition[]>();
  for (const agent of agents) {
    const list = byName.get(agent.name) ?? [];
    list.push(agent);
    byName.set(agent.name, list);
  }
  return [...byName.values()].map((list) => {
    const sorted = [...list].sort((a, b) => SOURCE_PRIORITY[b.source] - SOURCE_PRIORITY[a.source] || a.path.localeCompare(b.path));
    return { active: sorted[0], shadowed: sorted.slice(1) };
  }).sort((a, b) => a.active.name.localeCompare(b.active.name));
}

export function loadAgentIndex(cwd: string, agentDir = getAgentDir()): { all: AgentDefinition[]; active: ResolvedAgent[]; failed: FailedAgentFile[] } {
  const loaded = loadAgentDefinitions(cwd, agentDir);
  const all = [...builtInAgentDefinitions(), ...loaded.agents];
  return { all, active: resolveActiveAgents(all), failed: loaded.failed };
}

function sanitizeName(name: string): string {
  return name.trim().toLowerCase().replace(/[^a-z0-9-]+/g, "-").replace(/^-+|-+$/g, "").slice(0, 49) || "agent";
}

function editable(agent: AgentDefinition): boolean {
  return agent.source === "project" || agent.source === "user";
}

function sourceEditDescription(agent: AgentDefinition): string {
  if (agent.source === "built-in") return "no (built-in personality; override it by creating a project or personal agent with the same name)";
  if (agent.source === "claude-project" || agent.source === "claude-user") return "no (read-only compatibility import)";
  return "yes";
}

function detailText(agent: AgentDefinition, cwd: string, shadowed: AgentDefinition[] = []): string {
  const tools = agent.tools === undefined ? "all tools (field omitted)" : agent.tools.length === 0 ? "no tools" : agent.tools.join(", ");
  const lines = [
    `# ${agent.name}`,
    "",
    `Source: ${SOURCE_LABEL[agent.source]}`,
    `Path: ${toDisplayPath(cwd, agent.path)}`,
    `Editable: ${sourceEditDescription(agent)}`,
    `Description: ${agent.description}`,
    `Tools: ${tools}`,
    agent.disallowedTools?.length ? `Disallowed tools: ${agent.disallowedTools.join(", ")}` : undefined,
    agent.model ? `Model: ${agent.model}` : undefined,
    agent.effort ? `Effort: ${agent.effort}` : undefined,
    agent.permissionMode ? `Permission mode: ${agent.permissionMode}` : undefined,
    agent.memory ? `Memory: ${agent.memory}` : undefined,
    agent.background ? "Background: true" : undefined,
    agent.isolation ? `Isolation: ${agent.isolation}` : undefined,
    agent.color ? `Color: ${agent.color}` : undefined,
    agent.skills?.length ? `Skills: ${agent.skills.join(", ")}` : undefined,
    agent.maxTurns ? `Max turns: ${agent.maxTurns}` : undefined,
    shadowed.length ? `Shadowing: ${shadowed.map((item) => `${item.source}:${toDisplayPath(cwd, item.path)}`).join("; ")}` : undefined,
    "",
    "## System prompt",
    "",
    agent.prompt,
  ].filter((line): line is string => line !== undefined);
  return lines.join("\n");
}

function listText(cwd: string): string {
  const index = loadAgentIndex(cwd);
  if (index.active.length === 0 && index.failed.length === 0) {
    return `No agent definitions found. Create one with /agents create, or add markdown files under ${AGENT_DIR}.`;
  }
  const lines = ["OPPi agents", "", "Active definitions:"];
  for (const resolved of index.active) {
    const agent = resolved.active;
    const shadowed = resolved.shadowed.length ? ` (${resolved.shadowed.length} shadowed)` : "";
    const meta = [SOURCE_LABEL[agent.source], agent.model, agent.memory, agent.background ? "background" : undefined].filter(Boolean).join(" · ");
    lines.push(`- ${agent.name}${shadowed}: ${agent.description}`);
    lines.push(`  ${meta}`);
    lines.push(`  ${toDisplayPath(cwd, agent.path)}`);
  }
  if (index.failed.length > 0) {
    lines.push("", "Failed agent files:");
    for (const failure of index.failed) lines.push(`- ${toDisplayPath(cwd, failure.path)}: ${failure.reason}`);
  }
  lines.push("", "Note: /agents manages definitions and shows built-in personalities. Full AgentTool execution is part of the Rust/runtime spine work.");
  return lines.join("\n");
}

function findResolvedAgent(cwd: string, name: string): ResolvedAgent | undefined {
  const target = name.trim().toLowerCase();
  return loadAgentIndex(cwd).active.find((item) => item.active.name.toLowerCase() === target);
}

async function showList(ctx: ExtensionCommandContext): Promise<void> {
  const text = listText(ctx.cwd);
  if (ctx.hasUI) await ctx.ui.editor("OPPi agents", text);
  else ctx.ui.notify(text, "info");
}

async function showAgent(ctx: ExtensionCommandContext, name?: string): Promise<void> {
  let selected = name?.trim();
  const index = loadAgentIndex(ctx.cwd);
  if (!selected && index.active.length === 0) {
    ctx.ui.notify("No agent definitions found. Use /agents create to add one.", "info");
    return;
  }
  if (!selected) {
    const choice = await ctx.ui.select("Show agent", index.active.map((item) => `${item.active.name} — ${SOURCE_LABEL[item.active.source]}`));
    selected = choice?.split(" — ")[0];
  }
  if (!selected) return;
  const resolved = findResolvedAgent(ctx.cwd, selected);
  if (!resolved) {
    ctx.ui.notify(`Unknown agent: ${selected}`, "warning");
    return;
  }
  await ctx.ui.editor(`Agent: ${resolved.active.name}`, detailText(resolved.active, ctx.cwd, resolved.shadowed));
}

async function createAgent(ctx: ExtensionCommandContext): Promise<void> {
  const location = await ctx.ui.select("Create agent location", ["Project (.oppi/agents)", "Personal (OPPi agent dir)"]);
  if (!location) return;
  const source: AgentSource = location.startsWith("Personal") ? "user" : "project";
  const dirs = agentDirectories(ctx.cwd);
  const dir = dirs.find((item) => item.source === source);
  if (!dir) return;

  const rawName = await ctx.ui.input("Agent name", "test-runner");
  if (rawName === undefined) return;
  const name = sanitizeName(rawName);
  const description = await ctx.ui.input("Usage description", "Use this agent when...");
  if (description === undefined) return;
  const prompt = await ctx.ui.editor("System prompt", "You are a focused coding subagent. Follow the task, report concise results, and call out blockers.");
  if (prompt === undefined) return;
  const toolsInput = await ctx.ui.input("Tools (comma separated, blank = all tools)", "");
  if (toolsInput === undefined) return;
  const model = await ctx.ui.input("Model override (optional)", "");
  if (model === undefined) return;
  const color = await ctx.ui.input("Color (optional)", "cyan");
  if (color === undefined) return;

  const agent: AgentDefinition = {
    name,
    description: description.trim(),
    prompt: prompt.trim(),
    tools: toolsInput.trim() ? toolsInput.split(",").map((item) => item.trim()).filter(Boolean) : undefined,
    model: model.trim() || undefined,
    color: color.trim() || undefined,
    source,
    path: join(dir.path, `${name}.md`),
    filename: `${name}.md`,
  };
  const validation = validateAgent(agent);
  if (validation.errors.length > 0) {
    ctx.ui.notify(`Agent not saved: ${validation.errors.join(" ")}`, "error");
    return;
  }
  if (existsSync(agent.path)) {
    ctx.ui.notify(`Agent file already exists: ${toDisplayPath(ctx.cwd, agent.path)}`, "error");
    return;
  }
  mkdirSync(dir.path, { recursive: true });
  writeFileSync(agent.path, formatAgentMarkdown(agent), { encoding: "utf8", flag: "wx" });
  ctx.ui.notify(`Created ${agent.name} at ${toDisplayPath(ctx.cwd, agent.path)}.${validation.warnings.length ? ` ${validation.warnings.join(" ")}` : ""}`, "info");
}

async function editAgent(ctx: ExtensionCommandContext, name?: string): Promise<void> {
  let resolved = name ? findResolvedAgent(ctx.cwd, name) : undefined;
  const editableAgents = loadAgentIndex(ctx.cwd).active.filter((item) => editable(item.active));
  if (!resolved && editableAgents.length === 0) {
    ctx.ui.notify("No editable OPPi agents found. Use /agents create to add one.", "info");
    return;
  }
  if (!resolved) {
    const choice = await ctx.ui.select("Edit agent", editableAgents.map((item) => `${item.active.name} — ${SOURCE_LABEL[item.active.source]}`));
    const selected = choice?.split(" — ")[0];
    resolved = selected ? findResolvedAgent(ctx.cwd, selected) : undefined;
  }
  if (!resolved) return;
  const agent = resolved.active;
  if (!editable(agent)) {
    ctx.ui.notify(`${agent.name} is imported from ${SOURCE_LABEL[agent.source]} and is read-only in OPPi.`, "warning");
    return;
  }
  const edited = await ctx.ui.editor(`Edit ${agent.name}`, formatAgentMarkdown(agent));
  if (edited === undefined) return;
  const parsed = parseAgentMarkdown(edited, agent.path, agent.source);
  if (!parsed) {
    ctx.ui.notify("Edited markdown is missing agent frontmatter/name.", "error");
    return;
  }
  const validation = validateAgent(parsed);
  if (validation.errors.length > 0) {
    ctx.ui.notify(`Agent not saved: ${validation.errors.join(" ")}`, "error");
    return;
  }
  writeFileSync(agent.path, formatAgentMarkdown(parsed), "utf8");
  ctx.ui.notify(`Saved ${parsed.name}.${validation.warnings.length ? ` ${validation.warnings.join(" ")}` : ""}`, "info");
}

async function deleteAgent(ctx: ExtensionCommandContext, name?: string): Promise<void> {
  let resolved = name ? findResolvedAgent(ctx.cwd, name) : undefined;
  const editableAgents = loadAgentIndex(ctx.cwd).active.filter((item) => editable(item.active));
  if (!resolved && editableAgents.length === 0) {
    ctx.ui.notify("No editable OPPi agents found. Use /agents create to add one.", "info");
    return;
  }
  if (!resolved) {
    const choice = await ctx.ui.select("Delete agent", editableAgents.map((item) => `${item.active.name} — ${SOURCE_LABEL[item.active.source]}`));
    const selected = choice?.split(" — ")[0];
    resolved = selected ? findResolvedAgent(ctx.cwd, selected) : undefined;
  }
  if (!resolved) return;
  const agent = resolved.active;
  if (!editable(agent)) {
    ctx.ui.notify(`${agent.name} is imported from ${SOURCE_LABEL[agent.source]} and is read-only in OPPi.`, "warning");
    return;
  }
  const confirmed = await ctx.ui.confirm("Delete agent?", `${agent.name}\n${toDisplayPath(ctx.cwd, agent.path)}`);
  if (!confirmed) return;
  try {
    rmSync(agent.path, { force: true });
    ctx.ui.notify(`Deleted ${agent.name}.`, "info");
  } catch (error) {
    ctx.ui.notify(`Delete failed: ${error instanceof Error ? error.message : String(error)}`, "error");
  }
}

async function showMenu(ctx: ExtensionCommandContext): Promise<void> {
  const choice = await ctx.ui.select("Agents", ["List agents", "Create agent", "Show agent", "Edit agent", "Delete agent"]);
  if (!choice) return;
  if (choice.startsWith("List")) return showList(ctx);
  if (choice.startsWith("Create")) return createAgent(ctx);
  if (choice.startsWith("Show")) return showAgent(ctx);
  if (choice.startsWith("Edit")) return editAgent(ctx);
  if (choice.startsWith("Delete")) return deleteAgent(ctx);
}

export function getAgentsArgumentCompletions(prefix: string, cwd = process.cwd()): AutocompleteItem[] {
  const [commandRaw, secondRaw] = prefix.trimStart().split(/\s+/, 2);
  const commands = ["list", "show", "create", "edit", "delete"];
  if (!commandRaw || !prefix.trimStart().includes(" ")) {
    return commands.filter((command) => command.startsWith((commandRaw ?? "").toLowerCase())).map((command) => ({ value: command, label: command }));
  }
  const command = commandRaw.toLowerCase();
  if (!["show", "edit", "delete"].includes(command)) return [];
  const query = (secondRaw ?? "").toLowerCase();
  return loadAgentIndex(cwd).active
    .filter((item) => item.active.name.toLowerCase().includes(query))
    .map((item) => ({ value: `${command} ${item.active.name}`, label: item.active.name, description: SOURCE_LABEL[item.active.source] }));
}

async function handleAgents(args: string, ctx: ExtensionCommandContext): Promise<void> {
  const [commandRaw, ...rest] = args.trim().split(/\s+/).filter(Boolean);
  const command = commandRaw?.toLowerCase();
  const name = rest.join(" ").trim() || undefined;
  if (!command) return showMenu(ctx);
  if (command === "list" || command === "ls") return showList(ctx);
  if (command === "show" || command === "view") return showAgent(ctx, name);
  if (command === "create" || command === "new") return createAgent(ctx);
  if (command === "edit") return editAgent(ctx, name);
  if (command === "delete" || command === "remove" || command === "rm") return deleteAgent(ctx, name);
  ctx.ui.notify("Usage: /agents [list|show <name>|create|edit <name>|delete <name>]", "warning");
}

export default function agentsExtension(pi: ExtensionAPI) {
  pi.registerCommand("agents", {
    description: "Browse and manage OPPi agent definition markdown files.",
    getArgumentCompletions: (prefix) => getAgentsArgumentCompletions(prefix),
    handler: async (args, ctx) => handleAgents(args, ctx),
  });
}
