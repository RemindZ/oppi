import { readdirSync } from "node:fs";
import { basename, join, relative } from "node:path";
import type { ExtensionAPI } from "@mariozechner/pi-coding-agent";
import type { AutocompleteItem } from "@mariozechner/pi-tui";
import { promptVariantAppend } from "./prompt-variant";

const USAGE = "Usage: /independent @plan.md [additional scope]. Runs the referenced plan independently with todos, validation, and minimal clarification questions.";
const MAX_COMPLETIONS = 40;
const MAX_WALK_ENTRIES = 4_000;
const IGNORED_DIRS = new Set([".git", "node_modules", "target", "dist", "build", "coverage", ".turbo", ".cache"]);

function toDisplayPath(path: string): string {
  return path.replace(/\\/g, "/");
}

function quoteAtPath(path: string): string {
  return /\s/.test(path) ? `@"${path.replace(/"/g, '\\"')}"` : `@${path}`;
}

function currentTokenStart(text: string): number {
  for (let i = text.length - 1; i >= 0; i -= 1) {
    if (/\s/.test(text[i] ?? "")) return i + 1;
  }
  return 0;
}

function scorePath(path: string, query: string): number {
  const lowerPath = path.toLowerCase();
  const lowerBase = basename(path).toLowerCase();
  const lowerQuery = query.toLowerCase();
  if (!lowerQuery) return path.startsWith(".oppi-plans/") ? 90 : path.endsWith(".md") ? 70 : 20;
  if (lowerBase === lowerQuery) return 120;
  if (lowerBase.startsWith(lowerQuery)) return 100;
  if (lowerBase.includes(lowerQuery)) return 80;
  if (lowerPath.includes(lowerQuery)) return 50;
  let cursor = 0;
  for (const ch of lowerQuery) {
    cursor = lowerPath.indexOf(ch, cursor);
    if (cursor === -1) return 0;
    cursor += 1;
  }
  return 25;
}

function collectProjectFiles(cwd: string): string[] {
  const files: string[] = [];
  let visited = 0;
  const walk = (dir: string) => {
    if (visited > MAX_WALK_ENTRIES) return;
    let entries: Array<{ name: string; isDirectory(): boolean; isFile(): boolean }>;
    try {
      entries = readdirSync(dir, { withFileTypes: true }) as Array<{ name: string; isDirectory(): boolean; isFile(): boolean }>;
    } catch {
      return;
    }
    for (const entry of entries) {
      if (visited > MAX_WALK_ENTRIES) return;
      if (entry.isDirectory() && IGNORED_DIRS.has(entry.name)) continue;
      const fullPath = join(dir, entry.name);
      visited += 1;
      if (entry.isDirectory()) {
        walk(fullPath);
        continue;
      }
      if (!entry.isFile()) continue;
      const rel = toDisplayPath(relative(cwd, fullPath));
      if (!rel.startsWith("..")) files.push(rel);
    }
  };
  walk(cwd);
  return files;
}

export function getIndependentArgumentCompletions(argumentText: string, cwd = process.cwd()): AutocompleteItem[] | null {
  const tokenStart = currentTokenStart(argumentText);
  const beforeToken = argumentText.slice(0, tokenStart);
  const token = argumentText.slice(tokenStart);
  if (!token.startsWith("@")) return null;

  const query = token.startsWith('@"') ? token.slice(2).replace(/"$/, "") : token.slice(1);
  const files = collectProjectFiles(cwd)
    .map((path) => ({ path, score: scorePath(path, query) }))
    .filter((entry) => entry.score > 0)
    .sort((a, b) => b.score - a.score || a.path.localeCompare(b.path))
    .slice(0, MAX_COMPLETIONS);

  if (files.length === 0) return null;
  return files.map(({ path }) => ({
    value: `${beforeToken}${quoteAtPath(path)}`,
    label: basename(path),
    description: path,
  }));
}

function independentPrompt(args: string): string {
  return `Use the independent skill to execute the referenced plan document(s) to completion.

Plan document / scope:
${args}

Operating mode:
- First load and follow the full \`independent\` skill instructions.
- Read the referenced plan document(s) completely. If an item starts with \`@\`, resolve it as a file path from the current working directory unless the environment says otherwise.
- Create and maintain a \`todo_write\` execution plan.
- Do not stop after planning; continue through implementation, docs, validation, and final reporting.
- Ask clarification questions only when genuinely blocked by a product decision, secret/account access, destructive operation, production deploy/publish, or irreversible architectural choice. Use the structured question tool if available.
- Choose reasonable defaults and keep working when details are underspecified.
- Run relevant validation before marking work complete.
- Commit only if the user request or project instructions allow it; never publish or deploy unless explicitly requested.

Begin now.${promptVariantAppend("independent-user-prompt-append.md")}`;
}

export default function independentExtension(pi: ExtensionAPI) {
  pi.registerCommand("independent", {
    description: "Run independently from a plan document. Usage: /independent @plan.md",
    getArgumentCompletions: (prefix) => getIndependentArgumentCompletions(prefix),
    handler: async (args, ctx) => {
      const trimmed = args.trim();
      if (!trimmed) {
        ctx.ui.notify(USAGE, "info");
        return;
      }
      pi.sendUserMessage(independentPrompt(trimmed));
    },
  });
}
