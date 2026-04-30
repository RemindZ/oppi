import { existsSync, mkdirSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { basename, dirname, extname, isAbsolute, join, resolve } from "node:path";

export type PluginScope = "global" | "project";
export type PluginSourceType = "local" | "npm" | "git" | "url" | "marketplace";

export type OppiPluginManifest = {
  name: string;
  version?: string;
  description?: string;
  extensions?: string[];
  skills?: string[];
  prompts?: string[];
  themes?: string[];
  permissions?: unknown[];
  capabilities?: string[];
  license?: string;
};

export type InstalledPlugin = {
  name: string;
  source: string;
  sourceType: PluginSourceType;
  enabled: boolean;
  trusted: boolean;
  scope: PluginScope;
  description?: string;
  version?: string;
  license?: string;
  capabilities?: string[];
  manifest?: OppiPluginManifest;
  marketplace?: string;
  addedAt: string;
  updatedAt: string;
};

type StoredPlugin = Omit<InstalledPlugin, "scope">;

type PluginStore = {
  version: 1;
  updatedAt?: string;
  plugins: StoredPlugin[];
};

export type MarketplaceEntry = {
  name: string;
  url: string;
  addedAt: string;
};

type MarketplaceStore = {
  version: 1;
  updatedAt?: string;
  marketplaces: MarketplaceEntry[];
};

type CatalogPlugin = {
  name: string;
  source: string;
  version?: string;
  description?: string;
  license?: string;
  capabilities?: string[];
};

type IncompatibleCatalogPlugin = {
  name: string;
  detectedAs: "claude-marketplace" | "unknown-marketplace";
  fields: string[];
  reasons: string[];
  agentHandoffPrompt: string;
};

type LoadedCatalog = {
  marketplace: MarketplaceEntry;
  name: string;
  plugins: CatalogPlugin[];
  incompatiblePlugins: IncompatibleCatalogPlugin[];
  error?: string;
};

export type PluginCommand =
  | { type: "plugin"; subcommand: "list"; json: boolean; scope?: PluginScope }
  | { type: "plugin"; subcommand: "add" | "install"; source: string; name?: string; scope: PluginScope; enable: boolean; yes: boolean; json: boolean }
  | { type: "plugin"; subcommand: "remove" | "enable" | "disable" | "doctor"; name: string; scope?: PluginScope; yes: boolean; json: boolean };

export type MarketplaceCommand =
  | { type: "marketplace"; subcommand: "list"; json: boolean }
  | { type: "marketplace"; subcommand: "add"; url: string; name?: string; json: boolean }
  | { type: "marketplace"; subcommand: "remove"; name: string; json: boolean };

type Env = Record<string, string | undefined>;

type ParsedFlags = {
  positional: string[];
  json: boolean;
  yes: boolean;
  local: boolean;
  global: boolean;
  enable: boolean;
  disable: boolean;
  name?: string;
};

function expandHome(value: string): string {
  if (value === "~") return homedir();
  if (value.startsWith("~/") || value.startsWith("~\\")) return join(homedir(), value.slice(2));
  return value;
}

function resolveOppiHome(env: Env = process.env, cwd = process.cwd()): string {
  const raw = env.OPPI_HOME?.trim() || join(homedir(), ".oppi");
  const expanded = expandHome(raw);
  return isAbsolute(expanded) ? resolve(expanded) : resolve(cwd, expanded);
}

function globalPluginStorePath(env: Env = process.env, cwd = process.cwd()): string {
  return join(resolveOppiHome(env, cwd), "plugin-lock.json");
}

function projectPluginStorePath(cwd = process.cwd()): string {
  return join(cwd, ".oppi", "plugins.json");
}

function marketplaceStorePath(env: Env = process.env, cwd = process.cwd()): string {
  return join(resolveOppiHome(env, cwd), "marketplaces.json");
}

function readJsonFile(path: string): unknown | undefined {
  try {
    return JSON.parse(readFileSync(path, "utf8"));
  } catch {
    return undefined;
  }
}

function writeJsonFile(path: string, value: unknown): void {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, `${JSON.stringify(value, null, 2)}\n`, "utf8");
}

function asStringArray(value: unknown): string[] | undefined {
  if (!Array.isArray(value)) return undefined;
  const result = value.filter((item): item is string => typeof item === "string");
  return result.length ? result : undefined;
}

function normalizeCapabilities(value: unknown): string[] | undefined {
  const items = asStringArray(value);
  return items?.map((item) => item.trim().toLowerCase()).filter(Boolean);
}

function pluginKey(name: string): string {
  return name.trim().toLowerCase();
}

function nowIso(): string {
  return new Date().toISOString();
}

class PluginCompatibilityError extends Error {
  constructor(message: string, readonly details: { name: string; reasons: string[]; agentHandoffPrompt: string }) {
    super(message);
    this.name = "PluginCompatibilityError";
  }
}

function slugifyName(name: string): string {
  return name.toLowerCase().replace(/[^a-z0-9._-]+/g, "-").replace(/^-+|-+$/g, "") || "plugin";
}

function agentHandoffPromptFor(name: string, sourceHint: string, reasons: readonly string[]): string {
  return `Port the Claude marketplace plugin '${name}' into a Pi-compatible OPPi plugin. Source/catalog: ${sourceHint}. Compatibility blockers: ${reasons.join("; ")}. Inspect it safely, create or adapt a local Pi package under .oppi/plugins/${slugifyName(name)}, then register it with oppi plugin add ./.oppi/plugins/${slugifyName(name)} --local and enable only after review.`;
}

function formatCompatibilityMessage(details: { name: string; reasons: readonly string[]; agentHandoffPrompt: string }): string {
  return [
    `Plugin '${details.name}' does not look Pi/OPPi-compatible yet.`,
    ...details.reasons.map((reason) => `- ${reason}`),
    "",
    "OPPi can still help, but it should be an explicit porting task instead of blindly loading Claude-specific code.",
    "Suggested handoff:",
    `  oppi ${JSON.stringify(details.agentHandoffPrompt)}`,
  ].join("\n");
}

function normalizeStoredPlugin(value: unknown): StoredPlugin | undefined {
  if (!value || typeof value !== "object" || Array.isArray(value)) return undefined;
  const input = value as Record<string, unknown>;
  if (typeof input.name !== "string" || typeof input.source !== "string") return undefined;
  const sourceType = input.sourceType === "npm" || input.sourceType === "git" || input.sourceType === "url" || input.sourceType === "marketplace" || input.sourceType === "local"
    ? input.sourceType
    : classifySource(input.source).sourceType;
  return {
    name: input.name,
    source: input.source,
    sourceType,
    enabled: Boolean(input.enabled),
    trusted: Boolean(input.trusted),
    description: typeof input.description === "string" ? input.description : undefined,
    version: typeof input.version === "string" ? input.version : undefined,
    license: typeof input.license === "string" ? input.license : undefined,
    capabilities: normalizeCapabilities(input.capabilities),
    manifest: normalizeManifest(input.manifest),
    marketplace: typeof input.marketplace === "string" ? input.marketplace : undefined,
    addedAt: typeof input.addedAt === "string" ? input.addedAt : nowIso(),
    updatedAt: typeof input.updatedAt === "string" ? input.updatedAt : nowIso(),
  };
}

function readPluginStore(path: string): PluginStore {
  const parsed = readJsonFile(path);
  if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) return { version: 1, plugins: [] };
  const input = parsed as Record<string, unknown>;
  const plugins = Array.isArray(input.plugins) ? input.plugins.map(normalizeStoredPlugin).filter((item): item is StoredPlugin => Boolean(item)) : [];
  return { version: 1, updatedAt: typeof input.updatedAt === "string" ? input.updatedAt : undefined, plugins };
}

function writePluginStore(path: string, store: PluginStore): void {
  writeJsonFile(path, { ...store, version: 1, updatedAt: nowIso() });
}

function normalizeMarketplaceEntry(value: unknown): MarketplaceEntry | undefined {
  if (!value || typeof value !== "object" || Array.isArray(value)) return undefined;
  const input = value as Record<string, unknown>;
  if (typeof input.name !== "string" || typeof input.url !== "string") return undefined;
  return { name: input.name, url: input.url, addedAt: typeof input.addedAt === "string" ? input.addedAt : nowIso() };
}

function readMarketplaceStore(env: Env = process.env, cwd = process.cwd()): MarketplaceStore {
  const parsed = readJsonFile(marketplaceStorePath(env, cwd));
  if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) return { version: 1, marketplaces: [] };
  const input = parsed as Record<string, unknown>;
  const marketplaces = Array.isArray(input.marketplaces)
    ? input.marketplaces.map(normalizeMarketplaceEntry).filter((item): item is MarketplaceEntry => Boolean(item))
    : [];
  return { version: 1, updatedAt: typeof input.updatedAt === "string" ? input.updatedAt : undefined, marketplaces };
}

function writeMarketplaceStore(store: MarketplaceStore, env: Env = process.env, cwd = process.cwd()): void {
  writeJsonFile(marketplaceStorePath(env, cwd), { ...store, version: 1, updatedAt: nowIso() });
}

function parseFlags(args: string[]): ParsedFlags {
  const flags: ParsedFlags = { positional: [], json: false, yes: false, local: false, global: false, enable: false, disable: false };
  for (let i = 0; i < args.length; i += 1) {
    const arg = args[i];
    if (arg === "--json") { flags.json = true; continue; }
    if (arg === "--yes" || arg === "-y") { flags.yes = true; continue; }
    if (arg === "--local") { flags.local = true; continue; }
    if (arg === "--global") { flags.global = true; continue; }
    if (arg === "--enable" || arg === "--enabled") { flags.enable = true; continue; }
    if (arg === "--disable" || arg === "--disabled") { flags.disable = true; continue; }
    if (arg === "--name") {
      const value = args[i + 1];
      if (!value) throw new Error("--name requires a value");
      flags.name = value;
      i += 1;
      continue;
    }
    if (arg.startsWith("--name=")) { flags.name = arg.slice("--name=".length); continue; }
    flags.positional.push(arg);
  }
  if (flags.local && flags.global) throw new Error("Choose only one of --local or --global");
  if (flags.enable && flags.disable) throw new Error("Choose only one of --enable or --disable");
  return flags;
}

function flagScope(flags: ParsedFlags): PluginScope | undefined {
  if (flags.local) return "project";
  if (flags.global) return "global";
  return undefined;
}

export function parsePluginCommand(args: string[]): PluginCommand {
  const [rawSubcommand = "list", ...rest] = args;
  const subcommand = rawSubcommand === "install" ? "install" : rawSubcommand === "add" ? "add" : rawSubcommand;
  const flags = parseFlags(rest);
  if (subcommand === "list") return { type: "plugin", subcommand, json: flags.json, scope: flagScope(flags) };
  if (subcommand === "add" || subcommand === "install") {
    const source = flags.positional[0];
    if (!source) throw new Error(`oppi plugin ${subcommand} requires a source`);
    return {
      type: "plugin",
      subcommand,
      source,
      name: flags.name,
      scope: flagScope(flags) ?? "global",
      enable: flags.enable,
      yes: flags.yes,
      json: flags.json,
    };
  }
  if (subcommand === "remove" || subcommand === "enable" || subcommand === "disable" || subcommand === "doctor") {
    const name = flags.positional[0];
    if (!name) throw new Error(`oppi plugin ${subcommand} requires a plugin name`);
    return { type: "plugin", subcommand, name, scope: flagScope(flags), yes: flags.yes, json: flags.json };
  }
  throw new Error(`Unknown oppi plugin command: ${rawSubcommand}`);
}

export function parseMarketplaceCommand(args: string[]): MarketplaceCommand {
  const [subcommand = "list", ...rest] = args;
  const flags = parseFlags(rest);
  if (subcommand === "list") return { type: "marketplace", subcommand, json: flags.json };
  if (subcommand === "add") {
    const url = flags.positional[0];
    if (!url) throw new Error("oppi marketplace add requires a catalog URL or path");
    return { type: "marketplace", subcommand, url, name: flags.name, json: flags.json };
  }
  if (subcommand === "remove") {
    const name = flags.positional[0];
    if (!name) throw new Error("oppi marketplace remove requires a catalog name or URL");
    return { type: "marketplace", subcommand, name, json: flags.json };
  }
  throw new Error(`Unknown oppi marketplace command: ${subcommand}`);
}

function sourceIsUrl(value: string): boolean {
  return /^https?:\/\//i.test(value) || /^ssh:\/\//i.test(value) || /^git:\/\//i.test(value);
}

function sourceIsGit(value: string): boolean {
  return value.startsWith("git:") || /^git@[^:]+:.+/i.test(value);
}

function sourceIsNpm(value: string): boolean {
  return value.startsWith("npm:");
}

function sourceLooksLocal(value: string, cwd = process.cwd()): boolean {
  const expanded = expandHome(value);
  const resolved = isAbsolute(expanded) ? resolve(expanded) : resolve(cwd, expanded);
  return value.startsWith(".") || value.startsWith("/") || value.startsWith("~") || /^[A-Za-z]:[\\/]/.test(value) || existsSync(resolved);
}

function classifySource(source: string, cwd = process.cwd()): { source: string; sourceType: PluginSourceType } {
  if (sourceIsNpm(source)) return { source, sourceType: "npm" };
  if (sourceIsGit(source)) return { source, sourceType: "git" };
  if (sourceIsUrl(source)) return { source, sourceType: "url" };
  if (sourceLooksLocal(source, cwd)) {
    const expanded = expandHome(source);
    return { source: isAbsolute(expanded) ? resolve(expanded) : resolve(cwd, expanded), sourceType: "local" };
  }
  throw new Error(`Unknown plugin source '${source}'. Use a local path, npm:<package>, git:<repo>, URL, or a marketplace plugin name.`);
}

function normalizeManifest(value: unknown): OppiPluginManifest | undefined {
  if (!value || typeof value !== "object" || Array.isArray(value)) return undefined;
  const input = value as Record<string, unknown>;
  if (typeof input.name !== "string") return undefined;
  return {
    name: input.name,
    version: typeof input.version === "string" ? input.version : undefined,
    description: typeof input.description === "string" ? input.description : undefined,
    extensions: asStringArray(input.extensions),
    skills: asStringArray(input.skills),
    prompts: asStringArray(input.prompts),
    themes: asStringArray(input.themes),
    permissions: Array.isArray(input.permissions) ? input.permissions : undefined,
    capabilities: normalizeCapabilities(input.capabilities),
    license: typeof input.license === "string" ? input.license : undefined,
  };
}

function readPackageJson(path: string): Record<string, unknown> | undefined {
  const parsed = readJsonFile(path);
  return parsed && typeof parsed === "object" && !Array.isArray(parsed) ? parsed as Record<string, unknown> : undefined;
}

function readLocalManifest(source: string): OppiPluginManifest | undefined {
  if (!existsSync(source)) return undefined;
  const stats = statSync(source);
  const root = stats.isDirectory() ? source : dirname(source);
  const explicit = normalizeManifest(readJsonFile(join(root, "oppi-plugin.json")));
  if (explicit) return explicit;

  const packageJson = readPackageJson(join(root, "package.json"));
  if (!packageJson) {
    if (!stats.isDirectory()) {
      const fileName = basename(source);
      return { name: fileName.replace(extname(fileName), ""), version: "0.0.0", extensions: [source] };
    }
    return undefined;
  }

  const oppiPlugin = normalizeManifest(packageJson.oppiPlugin);
  if (oppiPlugin) return oppiPlugin;

  const pi = packageJson.pi && typeof packageJson.pi === "object" && !Array.isArray(packageJson.pi) ? packageJson.pi as Record<string, unknown> : undefined;
  const conventional = {
    extensions: existsSync(join(root, "extensions")) ? ["./extensions"] : undefined,
    skills: existsSync(join(root, "skills")) ? ["./skills"] : undefined,
    prompts: existsSync(join(root, "prompts")) ? ["./prompts"] : undefined,
    themes: existsSync(join(root, "themes")) ? ["./themes"] : undefined,
  };
  const name = typeof packageJson.name === "string" ? packageJson.name : basename(root);
  return {
    name,
    version: typeof packageJson.version === "string" ? packageJson.version : undefined,
    description: typeof packageJson.description === "string" ? packageJson.description : undefined,
    license: typeof packageJson.license === "string" ? packageJson.license : undefined,
    extensions: asStringArray(pi?.extensions) ?? conventional.extensions,
    skills: asStringArray(pi?.skills) ?? conventional.skills,
    prompts: asStringArray(pi?.prompts) ?? conventional.prompts,
    themes: asStringArray(pi?.themes) ?? conventional.themes,
    capabilities: normalizeCapabilities((packageJson.oppi && typeof packageJson.oppi === "object" && !Array.isArray(packageJson.oppi) ? (packageJson.oppi as Record<string, unknown>).capabilities : undefined)),
  };
}

function manifestHasPiResources(manifest: OppiPluginManifest | undefined): boolean {
  return Boolean(manifest?.extensions?.length || manifest?.skills?.length || manifest?.prompts?.length || manifest?.themes?.length);
}

function claudeSignalFields(input: Record<string, unknown>): string[] {
  const signals = [
    "mcpServers",
    "mcp_servers",
    "server",
    "servers",
    "slashCommands",
    "slash_commands",
    "commands",
    "agents",
    "hooks",
    "claude",
    "claudePlugin",
    "claude-plugin",
    "claude_desktop_config",
    "claudeDesktopConfig",
  ];
  return signals.filter((field) => input[field] !== undefined);
}

function localCompatibilityError(source: string, manifest: OppiPluginManifest | undefined): PluginCompatibilityError | undefined {
  if (manifestHasPiResources(manifest)) return undefined;
  if (!existsSync(source)) return undefined;
  const root = statSync(source).isDirectory() ? source : dirname(source);
  const packageJson = readPackageJson(join(root, "package.json"));
  const claudeManifest = readJsonFile(join(root, "claude-plugin.json"))
    ?? readJsonFile(join(root, ".claude-plugin.json"))
    ?? readJsonFile(join(root, "claude.json"));
  const signals = [
    ...(packageJson ? claudeSignalFields(packageJson) : []),
    ...(claudeManifest && typeof claudeManifest === "object" && !Array.isArray(claudeManifest) ? claudeSignalFields(claudeManifest as Record<string, unknown>) : []),
  ];
  if (signals.length === 0 && (manifest?.name || packageJson)) {
    const reasons = ["Local package has no Pi manifest resources and no conventional extensions/skills/prompts/themes directories."];
    throw new PluginCompatibilityError(formatCompatibilityMessage({
      name: manifest?.name ?? (typeof packageJson?.name === "string" ? packageJson.name : basename(root)),
      reasons,
      agentHandoffPrompt: agentHandoffPromptFor(manifest?.name ?? (typeof packageJson?.name === "string" ? packageJson.name : basename(root)), source, reasons),
    }), {
      name: manifest?.name ?? (typeof packageJson?.name === "string" ? packageJson.name : basename(root)),
      reasons,
      agentHandoffPrompt: agentHandoffPromptFor(manifest?.name ?? (typeof packageJson?.name === "string" ? packageJson.name : basename(root)), source, reasons),
    });
  }
  if (signals.length === 0) return undefined;
  const pluginName = manifest?.name ?? (typeof packageJson?.name === "string" ? packageJson.name : basename(root));
  const reasons = [
    "Detected Claude-specific plugin fields instead of Pi package resources.",
    `Claude fields: ${[...new Set(signals)].join(", ")}.`,
    "OPPi cannot safely load this until it is adapted into Pi extensions/skills/prompts/themes.",
  ];
  return new PluginCompatibilityError(formatCompatibilityMessage({
    name: pluginName,
    reasons,
    agentHandoffPrompt: agentHandoffPromptFor(pluginName, source, reasons),
  }), {
    name: pluginName,
    reasons,
    agentHandoffPrompt: agentHandoffPromptFor(pluginName, source, reasons),
  });
}

function npmNameFromSource(source: string): string {
  const spec = source.slice("npm:".length);
  if (spec.startsWith("@")) {
    const parts = spec.split("@");
    return parts.length > 2 ? `@${parts[1] ?? "plugin"}` : spec;
  }
  return spec.split("@")[0] ?? spec;
}

function nameFromSource(source: string, sourceType: PluginSourceType): string {
  if (sourceType === "npm") return npmNameFromSource(source);
  const clean = source.replace(/[#?].*$/, "").replace(/\.git$/i, "");
  const tail = basename(clean.replace(/[\\/]+$/, ""));
  return tail || source;
}

function pluginRisk(plugin: Pick<InstalledPlugin, "sourceType" | "source" | "capabilities" | "manifest">): string[] {
  const risks: string[] = [];
  const capabilities = new Set([...(plugin.capabilities ?? []), ...(plugin.manifest?.capabilities ?? [])].map((item) => item.toLowerCase()));
  if (plugin.sourceType !== "local") risks.push("third-party package source can execute extension code");
  if (capabilities.has("shell") || capabilities.has("exec")) risks.push("declares shell/exec capability");
  if (capabilities.has("network")) risks.push("declares network capability");
  if (capabilities.has("native")) risks.push("declares native capability");

  if (plugin.sourceType === "local" && existsSync(plugin.source)) {
    const root = statSync(plugin.source).isDirectory() ? plugin.source : dirname(plugin.source);
    const packageJson = readPackageJson(join(root, "package.json"));
    const scripts = packageJson?.scripts && typeof packageJson.scripts === "object" && !Array.isArray(packageJson.scripts) ? packageJson.scripts as Record<string, unknown> : undefined;
    for (const script of ["preinstall", "install", "postinstall", "prepare"]) {
      if (typeof scripts?.[script] === "string") risks.push(`package.json has ${script} script`);
    }
    if (packageJson?.gypfile === true) risks.push("package.json declares gypfile/native build");
  }

  return [...new Set(risks)];
}

function loadPluginsForScope(scope: PluginScope, env: Env = process.env, cwd = process.cwd()): InstalledPlugin[] {
  const path = scope === "global" ? globalPluginStorePath(env, cwd) : projectPluginStorePath(cwd);
  return readPluginStore(path).plugins.map((plugin) => ({ ...plugin, scope }));
}

export function loadAllPlugins(env: Env = process.env, cwd = process.cwd()): InstalledPlugin[] {
  const byName = new Map<string, InstalledPlugin>();
  for (const plugin of loadPluginsForScope("global", env, cwd)) byName.set(pluginKey(plugin.name), plugin);
  for (const plugin of loadPluginsForScope("project", env, cwd)) byName.set(pluginKey(plugin.name), plugin);
  return [...byName.values()].sort((a, b) => a.name.localeCompare(b.name));
}

export function resolveEnabledPluginSources(options: { env?: Env; cwd?: string } = {}): string[] {
  const env = options.env ?? process.env;
  const cwd = options.cwd ?? process.cwd();
  const sources: string[] = [];
  const seen = new Set<string>();
  const extra = env.OPPI_PLUGIN_SOURCES?.split(/[;,]/).map((item) => item.trim()).filter(Boolean) ?? [];
  for (const source of extra) {
    const key = source.toLowerCase();
    if (!seen.has(key)) {
      seen.add(key);
      sources.push(source);
    }
  }
  for (const plugin of loadAllPlugins(env, cwd)) {
    if (!plugin.enabled) continue;
    const key = plugin.source.toLowerCase();
    if (seen.has(key)) continue;
    seen.add(key);
    sources.push(plugin.source);
  }
  return sources;
}

function getStoreForWrite(scope: PluginScope, env: Env = process.env, cwd = process.cwd()): { path: string; store: PluginStore } {
  const path = scope === "global" ? globalPluginStorePath(env, cwd) : projectPluginStorePath(cwd);
  return { path, store: readPluginStore(path) };
}

function upsertPlugin(plugin: InstalledPlugin, env: Env = process.env, cwd = process.cwd()): void {
  const { path, store } = getStoreForWrite(plugin.scope, env, cwd);
  const without = store.plugins.filter((item) => pluginKey(item.name) !== pluginKey(plugin.name));
  const { scope: _scope, ...stored } = plugin;
  writePluginStore(path, { version: 1, plugins: [...without, stored].sort((a, b) => a.name.localeCompare(b.name)) });
}

function removePlugin(name: string, scope: PluginScope, env: Env = process.env, cwd = process.cwd()): boolean {
  const { path, store } = getStoreForWrite(scope, env, cwd);
  const before = store.plugins.length;
  const plugins = store.plugins.filter((item) => pluginKey(item.name) !== pluginKey(name));
  if (plugins.length === before) return false;
  writePluginStore(path, { version: 1, plugins });
  return true;
}

function findPlugin(name: string, scope: PluginScope | undefined, env: Env = process.env, cwd = process.cwd()): InstalledPlugin | undefined {
  const scopes: PluginScope[] = scope ? [scope] : ["project", "global"];
  for (const candidateScope of scopes) {
    const found = loadPluginsForScope(candidateScope, env, cwd).find((plugin) => pluginKey(plugin.name) === pluginKey(name));
    if (found) return found;
  }
  return undefined;
}

async function readTextFromUrlOrPath(url: string, cwd = process.cwd()): Promise<string> {
  if (/^https?:\/\//i.test(url)) {
    const response = await fetch(url);
    if (!response.ok) throw new Error(`HTTP ${response.status} ${response.statusText}`);
    return response.text();
  }
  const expanded = expandHome(url);
  const resolved = isAbsolute(expanded) ? resolve(expanded) : resolve(cwd, expanded);
  return readFileSync(resolved, "utf8");
}

function catalogItems(value: unknown): unknown[] {
  if (Array.isArray(value)) return value;
  if (!value || typeof value !== "object") return [];
  const root = value as Record<string, unknown>;
  const arrays = [root.plugins, root.items, root.extensions, root.tools, root.agents, root.commands]
    .filter((item): item is unknown[] => Array.isArray(item));
  const fromArrays = arrays.flat();
  const mcpServers = root.mcpServers ?? root.mcp_servers;
  if (mcpServers && typeof mcpServers === "object" && !Array.isArray(mcpServers)) {
    for (const [name, server] of Object.entries(mcpServers as Record<string, unknown>)) {
      fromArrays.push(server && typeof server === "object" && !Array.isArray(server) ? { name, ...(server as Record<string, unknown>) } : { name, server });
    }
  }
  return fromArrays;
}

function catalogSourceFromEntry(input: Record<string, unknown>): string | undefined {
  for (const key of ["source", "piPackage", "oppiPackage", "package", "npm", "git", "repo", "repository"]) {
    const value = input[key];
    if (typeof value === "string" && value.trim()) return value.trim();
    if (value && typeof value === "object" && !Array.isArray(value)) {
      const nested = value as Record<string, unknown>;
      if (typeof nested.url === "string" && nested.url.trim()) return nested.url.trim();
    }
  }
  return undefined;
}

function catalogEntryName(input: Record<string, unknown>, fallback: string): string {
  for (const key of ["name", "id", "title", "slug"]) {
    const value = input[key];
    if (typeof value === "string" && value.trim()) return value.trim();
  }
  return fallback;
}

function incompatibleCatalogEntry(input: Record<string, unknown>, name: string, catalogHint: string): IncompatibleCatalogPlugin {
  const fields = Object.keys(input).sort();
  const claudeFields = claudeSignalFields(input);
  const detectedAs = claudeFields.length > 0 ? "claude-marketplace" : "unknown-marketplace";
  const reasons = detectedAs === "claude-marketplace"
    ? [
      "Catalog entry looks Claude-specific and does not provide a Pi/OPPi package source.",
      `Claude fields: ${claudeFields.join(", ")}.`,
      "A compatibility adapter or manual port must translate it into Pi extensions/skills/prompts/themes first.",
    ]
    : [
      "Catalog entry does not provide a Pi/OPPi package source.",
      "Expected one of: source, piPackage, oppiPackage, package, npm, git, repo, or repository.",
    ];
  return {
    name,
    detectedAs,
    fields,
    reasons,
    agentHandoffPrompt: agentHandoffPromptFor(name, catalogHint, reasons),
  };
}

function catalogFromJson(value: unknown, fallbackName: string, catalogHint = fallbackName): { name: string; plugins: CatalogPlugin[]; incompatiblePlugins: IncompatibleCatalogPlugin[] } {
  const root = value && typeof value === "object" && !Array.isArray(value) ? value as Record<string, unknown> : undefined;
  const plugins: CatalogPlugin[] = [];
  const incompatiblePlugins: IncompatibleCatalogPlugin[] = [];
  let index = 0;
  for (const item of catalogItems(value)) {
    index += 1;
    if (!item || typeof item !== "object" || Array.isArray(item)) continue;
    const input = item as Record<string, unknown>;
    const name = catalogEntryName(input, `entry-${index}`);
    const source = catalogSourceFromEntry(input);
    if (source) {
      plugins.push({
        name,
        source,
        version: typeof input.version === "string" ? input.version : undefined,
        description: typeof input.description === "string" ? input.description : typeof input.summary === "string" ? input.summary : undefined,
        license: typeof input.license === "string" ? input.license : undefined,
        capabilities: normalizeCapabilities(input.capabilities),
      });
      continue;
    }
    incompatiblePlugins.push(incompatibleCatalogEntry(input, name, catalogHint));
  }
  return { name: typeof root?.name === "string" ? root.name : fallbackName, plugins, incompatiblePlugins };
}

function shouldResolveCatalogSourceRelativeToCatalog(source: string): boolean {
  if (sourceIsNpm(source) || sourceIsGit(source) || sourceIsUrl(source)) return false;
  if (source.startsWith("~") || /^[A-Za-z]:[\\/]/.test(source)) return false;
  return !isAbsolute(expandHome(source));
}

function localCatalogBaseDir(url: string, cwd: string): string | undefined {
  if (/^https?:\/\//i.test(url)) return undefined;
  return dirname(normalizeMarketplaceUrl(url, cwd));
}

async function loadCatalog(marketplace: MarketplaceEntry, cwd = process.cwd()): Promise<LoadedCatalog> {
  try {
    const text = await readTextFromUrlOrPath(marketplace.url, cwd);
    const parsed = JSON.parse(text);
    const catalog = catalogFromJson(parsed, marketplace.name, marketplace.url);
    const baseDir = localCatalogBaseDir(marketplace.url, cwd);
    const plugins = baseDir
      ? catalog.plugins.map((plugin) => ({
        ...plugin,
        source: shouldResolveCatalogSourceRelativeToCatalog(plugin.source) ? resolve(baseDir, plugin.source) : plugin.source,
      }))
      : catalog.plugins;
    return { marketplace, name: catalog.name, plugins, incompatiblePlugins: catalog.incompatiblePlugins };
  } catch (error) {
    return { marketplace, name: marketplace.name, plugins: [], incompatiblePlugins: [], error: error instanceof Error ? error.message : String(error) };
  }
}

async function resolveMarketplacePlugin(name: string, env: Env = process.env, cwd = process.cwd()): Promise<{ kind: "compatible"; plugin: CatalogPlugin; marketplace: MarketplaceEntry } | { kind: "incompatible"; plugin: IncompatibleCatalogPlugin; marketplace: MarketplaceEntry } | undefined> {
  const store = readMarketplaceStore(env, cwd);
  for (const marketplace of store.marketplaces) {
    const catalog = await loadCatalog(marketplace, cwd);
    const plugin = catalog.plugins.find((item) => pluginKey(item.name) === pluginKey(name));
    if (plugin) return { kind: "compatible", plugin, marketplace };
    const incompatible = catalog.incompatiblePlugins.find((item) => pluginKey(item.name) === pluginKey(name));
    if (incompatible) return { kind: "incompatible", plugin: incompatible, marketplace };
  }
  return undefined;
}

async function pluginFromAddCommand(command: Extract<PluginCommand, { subcommand: "add" | "install" }>, env: Env, cwd: string): Promise<InstalledPlugin> {
  const marketplaceMatch = await resolveMarketplacePlugin(command.source, env, cwd);
  if (marketplaceMatch?.kind === "incompatible") {
    throw new PluginCompatibilityError(formatCompatibilityMessage(marketplaceMatch.plugin), {
      name: marketplaceMatch.plugin.name,
      reasons: marketplaceMatch.plugin.reasons,
      agentHandoffPrompt: marketplaceMatch.plugin.agentHandoffPrompt,
    });
  }
  const source = marketplaceMatch?.kind === "compatible" ? marketplaceMatch.plugin.source : command.source;
  const classified = classifySource(source, cwd);
  const manifest = classified.sourceType === "local" ? readLocalManifest(classified.source) : undefined;
  const localError = classified.sourceType === "local" ? localCompatibilityError(classified.source, manifest) : undefined;
  if (localError) throw localError;
  const name = command.name ?? (marketplaceMatch?.kind === "compatible" ? marketplaceMatch.plugin.name : undefined) ?? manifest?.name ?? nameFromSource(classified.source, classified.sourceType);
  const timestamp = nowIso();
  const plugin: InstalledPlugin = {
    name,
    source: classified.source,
    sourceType: marketplaceMatch?.kind === "compatible" ? "marketplace" : classified.sourceType,
    enabled: command.enable,
    trusted: command.enable && command.yes,
    scope: command.scope,
    description: (marketplaceMatch?.kind === "compatible" ? marketplaceMatch.plugin.description : undefined) ?? manifest?.description,
    version: (marketplaceMatch?.kind === "compatible" ? marketplaceMatch.plugin.version : undefined) ?? manifest?.version,
    license: (marketplaceMatch?.kind === "compatible" ? marketplaceMatch.plugin.license : undefined) ?? manifest?.license,
    capabilities: (marketplaceMatch?.kind === "compatible" ? marketplaceMatch.plugin.capabilities : undefined) ?? manifest?.capabilities,
    manifest,
    marketplace: marketplaceMatch?.kind === "compatible" ? marketplaceMatch.marketplace.name : undefined,
    addedAt: timestamp,
    updatedAt: timestamp,
  };
  if (plugin.enabled && !command.yes) {
    throw new Error(`Refusing to enable '${plugin.name}' without explicit trust. Re-run with --enable --yes after reviewing the plugin source.`);
  }
  return plugin;
}

function pluginSummary(plugin: InstalledPlugin): Record<string, unknown> {
  return {
    name: plugin.name,
    source: plugin.source,
    sourceType: plugin.sourceType,
    scope: plugin.scope,
    enabled: plugin.enabled,
    trusted: plugin.trusted,
    version: plugin.version,
    description: plugin.description,
    license: plugin.license,
    capabilities: plugin.capabilities,
    marketplace: plugin.marketplace,
    risks: pluginRisk(plugin),
  };
}

function printPluginReview(plugin: InstalledPlugin): void {
  const risks = pluginRisk(plugin);
  console.log(`Plugin: ${plugin.name}`);
  if (plugin.description) console.log(`description: ${plugin.description}`);
  if (plugin.version) console.log(`version: ${plugin.version}`);
  if (plugin.license) console.log(`license: ${plugin.license}`);
  console.log(`source: ${plugin.source}`);
  console.log(`scope: ${plugin.scope}`);
  console.log(`enabled: ${plugin.enabled ? "yes" : "no"}`);
  console.log(`trusted: ${plugin.trusted ? "yes" : "no"}`);
  if (plugin.capabilities?.length) console.log(`capabilities: ${plugin.capabilities.join(", ")}`);
  if (risks.length) {
    console.log("risk notes:");
    for (const risk of risks) console.log(`  - ${risk}`);
  } else {
    console.log("risk notes: no declared elevated capabilities found; still review source before enabling.");
  }
}

function printPluginList(plugins: InstalledPlugin[]): void {
  console.log("OPPi plugins");
  if (plugins.length === 0) {
    console.log("No plugins configured. Add one with `oppi plugin add <source>`.");
    return;
  }
  for (const plugin of plugins) {
    const state = plugin.enabled ? "enabled" : "disabled";
    const trust = plugin.trusted ? "trusted" : "untrusted";
    console.log(`- ${plugin.name} [${state}, ${trust}, ${plugin.scope}]`);
    console.log(`  source: ${plugin.source}`);
    if (plugin.description) console.log(`  ${plugin.description}`);
  }
}

export async function runPluginCommand(command: PluginCommand, env: Env = process.env, cwd = process.cwd()): Promise<number> {
  try {
    if (command.subcommand === "list") {
      const all = loadAllPlugins(env, cwd);
      const plugins = command.scope ? all.filter((plugin) => plugin.scope === command.scope) : all;
      if (command.json) console.log(JSON.stringify({ ok: true, plugins: plugins.map(pluginSummary) }, null, 2));
      else printPluginList(plugins);
      return 0;
    }

    if (command.subcommand === "add" || command.subcommand === "install") {
      const plugin = await pluginFromAddCommand(command, env, cwd);
      upsertPlugin(plugin, env, cwd);
      if (command.json) console.log(JSON.stringify({ ok: true, plugin: pluginSummary(plugin) }, null, 2));
      else {
        console.log(`${command.subcommand === "install" ? "Installed" : "Added"} plugin '${plugin.name}' (${plugin.enabled ? "enabled" : "disabled"}).`);
        printPluginReview(plugin);
        if (!plugin.enabled) console.log(`Next: review the source, then run \`oppi plugin enable ${plugin.name} --yes\`.`);
      }
      return 0;
    }

    if (!("name" in command) || !command.name) throw new Error(`oppi plugin ${command.subcommand} requires a plugin name`);
    const plugin = findPlugin(command.name, command.scope, env, cwd);
    if (!plugin) throw new Error(`Plugin not found: ${command.name}`);

    if (command.subcommand === "doctor") {
      const exists = plugin.sourceType !== "local" || existsSync(plugin.source);
      const risks = pluginRisk(plugin);
      const payload = { ok: exists, plugin: pluginSummary(plugin), checks: [{ name: "source", ok: exists, message: exists ? "source is resolvable" : "local source path is missing" }] };
      if (command.json) console.log(JSON.stringify(payload, null, 2));
      else {
        printPluginReview(plugin);
        console.log(`source check: ${exists ? "ok" : "missing"}`);
        console.log(`launch behavior: ${plugin.enabled ? "will be passed to Pi with -e" : "disabled; not loaded"}`);
        if (risks.length && !plugin.trusted) console.log("warning: plugin has risk notes and is not trusted/enabled yet.");
      }
      return exists ? 0 : 1;
    }

    if (command.subcommand === "remove") {
      const removed = removePlugin(plugin.name, plugin.scope, env, cwd);
      if (command.json) console.log(JSON.stringify({ ok: removed, removed: plugin.name, scope: plugin.scope }, null, 2));
      else console.log(removed ? `Removed plugin '${plugin.name}' from ${plugin.scope} config.` : `Plugin '${plugin.name}' was not configured.`);
      return removed ? 0 : 1;
    }

    const next: InstalledPlugin = {
      ...plugin,
      enabled: command.subcommand === "enable",
      trusted: command.subcommand === "enable" ? (plugin.trusted || command.yes) : plugin.trusted,
      updatedAt: nowIso(),
    };
    if (command.subcommand === "enable" && !next.trusted) {
      throw new Error(`Refusing to enable '${plugin.name}' without explicit trust. Re-run with --yes after reviewing the plugin source.`);
    }
    upsertPlugin(next, env, cwd);
    if (command.json) console.log(JSON.stringify({ ok: true, plugin: pluginSummary(next) }, null, 2));
    else console.log(`${command.subcommand === "enable" ? "Enabled" : "Disabled"} plugin '${plugin.name}'.`);
    return 0;
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    if (command.json) {
      const details = error instanceof PluginCompatibilityError ? error.details : undefined;
      console.log(JSON.stringify({ ok: false, error: message, compatibility: details }, null, 2));
    } else {
      console.error(`OPPi plugin ${command.subcommand} failed: ${message}`);
    }
    return 1;
  }
}

function marketplaceNameFromUrl(url: string): string {
  const clean = url.replace(/[#?].*$/, "").replace(/\/+$/, "");
  const tail = basename(clean).replace(/\.json$/i, "");
  return tail || "marketplace";
}

function normalizeMarketplaceUrl(url: string, cwd = process.cwd()): string {
  if (/^https?:\/\//i.test(url)) return url;
  const expanded = expandHome(url);
  return isAbsolute(expanded) ? resolve(expanded) : resolve(cwd, expanded);
}

function printMarketplaceList(items: MarketplaceEntry[]): void {
  console.log("OPPi marketplaces");
  if (items.length === 0) {
    console.log("No marketplaces configured. Add one with `oppi marketplace add <catalog.json>`.");
    return;
  }
  for (const item of items) {
    console.log(`- ${item.name}`);
    console.log(`  ${item.url}`);
  }
}

export async function runMarketplaceCommand(command: MarketplaceCommand, env: Env = process.env, cwd = process.cwd()): Promise<number> {
  try {
    const store = readMarketplaceStore(env, cwd);
    if (command.subcommand === "list") {
      const catalogs = await Promise.all(store.marketplaces.map((item) => loadCatalog(item, cwd)));
      if (command.json) console.log(JSON.stringify({ ok: true, marketplaces: store.marketplaces, catalogs }, null, 2));
      else {
        printMarketplaceList(store.marketplaces);
        for (const catalog of catalogs) {
          if (catalog.error) console.log(`  ! ${catalog.marketplace.name}: ${catalog.error}`);
          else console.log(`  ${catalog.marketplace.name}: ${catalog.plugins.length} compatible plugin(s), ${catalog.incompatiblePlugins.length} incompatible entr${catalog.incompatiblePlugins.length === 1 ? "y" : "ies"}`);
        }
      }
      return 0;
    }

    if (command.subcommand === "add") {
      const url = normalizeMarketplaceUrl(command.url, cwd);
      if (!/^https?:\/\//i.test(url) && !existsSync(url)) throw new Error(`Marketplace catalog not found: ${url}`);
      let name = command.name ?? marketplaceNameFromUrl(url);
      const entry: MarketplaceEntry = { name, url, addedAt: nowIso() };
      const loaded = await loadCatalog(entry, cwd);
      if (!loaded.error && loaded.name) name = command.name ?? loaded.name;
      const nextEntry: MarketplaceEntry = { ...entry, name };
      const marketplaces = [...store.marketplaces.filter((item) => pluginKey(item.name) !== pluginKey(name) && item.url !== url), nextEntry]
        .sort((a, b) => a.name.localeCompare(b.name));
      writeMarketplaceStore({ version: 1, marketplaces }, env, cwd);
      if (command.json) console.log(JSON.stringify({ ok: true, marketplace: nextEntry, warning: loaded.error, compatiblePlugins: loaded.plugins.length, incompatiblePlugins: loaded.incompatiblePlugins.length }, null, 2));
      else {
        console.log(`Added marketplace '${name}': ${url}`);
        if (loaded.error) console.log(`Warning: catalog could not be read now (${loaded.error}); it remains registered for later.`);
        else {
          console.log(`Catalog has ${loaded.plugins.length} compatible plugin(s) and ${loaded.incompatiblePlugins.length} incompatible entr${loaded.incompatiblePlugins.length === 1 ? "y" : "ies"}.`);
          if (loaded.incompatiblePlugins.length > 0) console.log("If an incompatible Claude entry is worth using, run `oppi plugin add <name>` to get an agent handoff prompt for porting it.");
        }
      }
      return 0;
    }

    const before = store.marketplaces.length;
    const marketplaces = store.marketplaces.filter((item) => pluginKey(item.name) !== pluginKey(command.name) && item.url !== command.name);
    const removed = marketplaces.length !== before;
    if (removed) writeMarketplaceStore({ version: 1, marketplaces }, env, cwd);
    if (command.json) console.log(JSON.stringify({ ok: removed, removed: command.name }, null, 2));
    else console.log(removed ? `Removed marketplace '${command.name}'.` : `Marketplace '${command.name}' was not configured.`);
    return removed ? 0 : 1;
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    if (command.json) console.log(JSON.stringify({ ok: false, error: message }, null, 2));
    else console.error(`OPPi marketplace ${command.subcommand} failed: ${message}`);
    return 1;
  }
}

export function collectPluginDiagnostics(env: Env = process.env, cwd = process.cwd()): { enabled: number; configured: number; sources: string[]; warnings: string[] } {
  const plugins = loadAllPlugins(env, cwd);
  const sources = resolveEnabledPluginSources({ env, cwd });
  const warnings: string[] = [];
  for (const plugin of plugins) {
    if (plugin.enabled && plugin.sourceType === "local" && !existsSync(plugin.source)) warnings.push(`Enabled plugin '${plugin.name}' points to a missing path.`);
    if (plugin.enabled && !plugin.trusted) warnings.push(`Enabled plugin '${plugin.name}' is not marked trusted.`);
  }
  return { enabled: plugins.filter((plugin) => plugin.enabled).length, configured: plugins.length, sources, warnings };
}
