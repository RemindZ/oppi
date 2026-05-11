#!/usr/bin/env node
import { spawn, spawnSync } from "node:child_process";
import { randomBytes } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, realpathSync, rmSync, statSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { basename, dirname, isAbsolute, join, resolve } from "node:path";
import { createRequire } from "node:module";
import { createServer } from "node:http";
import { createInterface } from "node:readline";
import { fileURLToPath, pathToFileURL } from "node:url";
import {
  collectPluginDiagnostics,
  parseMarketplaceCommand,
  parsePluginCommand,
  resolveEnabledPluginSources,
  runMarketplaceCommand,
  runPluginCommand,
  type MarketplaceCommand,
  type PluginCommand,
} from "./plugins.js";

const require = createRequire(import.meta.url);
const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const HOPPI_PACKAGE_NAME = "@oppiai/hoppi-memory";
const HOPPI_LEGACY_PACKAGE_NAME = "hoppi-memory";
const HOPPI_PACKAGE_SPEC = `${HOPPI_PACKAGE_NAME}@^0.1.0`;
const RUNTIME_WORKER_MEMORY_CONTEXT_MAX_BYTES = 12_000;
const RUNTIME_WORKER_MEMORY_QUERY_MAX_CHARS = 4_000;
const OPPI_CLI_PACKAGE_NAME = "@oppiai/cli";
const OPPI_CHANGELOG_URL = "https://github.com/RemindZ/oppi/blob/main/CHANGELOG.md";
const UPDATE_CHECK_INTERVAL_MS = 24 * 60 * 60 * 1000;
const UPDATE_NOTICE_INTERVAL_MS = 24 * 60 * 60 * 1000;
const UPDATE_CHECK_TIMEOUT_MS = 1200;
const OPPI_PROTOCOL_VERSION = "0.1.0";

export type RuntimeWorkerMemoryMode = "auto" | "on" | "off";
export type RuntimeWorkerEffort = "off" | "minimal" | "low" | "medium" | "high" | "xhigh";
export type RuntimeWorkerProvider = "openai-compatible" | "openai-codex";
export type PromptVariant = "off" | "promptname_a" | "promptname_b";
type RuntimeWorkerFollowUpStatus = "queued" | "running" | "completed";
type RuntimeWorkerFollowUpChain = {
  chainId?: string;
  rootPrompt: string;
  followUps?: Array<{ id: string; text: string; status: RuntimeWorkerFollowUpStatus }>;
  currentFollowUpId?: string;
  promptVariantAppend?: string;
};
type RuntimeWorkerPermissionMode = "read-only" | "default" | "auto-review" | "full-access";

type RuntimeWorkerSandboxPolicy = {
  permissionProfile: {
    mode: RuntimeWorkerPermissionMode;
    readableRoots?: string[];
    writableRoots: string[];
    filesystemRules?: unknown[];
    protectedPatterns?: string[];
  };
  network: "disabled" | "ask" | "enabled";
  filesystem: "readOnly" | "workspaceWrite" | "unrestricted";
};

export type OppiCommand =
  | { type: "help" }
  | { type: "version" }
  | { type: "doctor"; json: boolean; agentDir?: string }
  | { type: "update"; check: boolean; json: boolean }
  | { type: "mem"; subcommand: "status" | "setup" | "install" | "dashboard" | "open"; json: boolean }
  | { type: "natives"; subcommand: "status" | "benchmark"; json: boolean }
  | { type: "sandbox"; subcommand: "status" | "setup-windows"; json: boolean; yes: boolean; account?: string; persistEnv: boolean; dryRun: boolean }
  | { type: "server"; stdio: boolean; experimental: boolean; json: boolean }
  | { type: "tui"; subcommand: "run" | "smoke" | "dogfood"; experimental: boolean; json: boolean; shellArgs: string[]; agentDir?: string }
  | { type: "resume"; threadId?: string; json: boolean; shellArgs: string[]; agentDir?: string }
  | { type: "runtime-loop"; subcommand: "smoke"; json: boolean }
  | { type: "runtime-worker"; subcommand: "smoke"; json: boolean }
  | {
    type: "runtime-worker";
    subcommand: "run";
    json: boolean;
    prompt: string;
    provider?: RuntimeWorkerProvider;
    model?: string;
    baseUrl?: string;
    apiKeyEnv?: string;
    systemPrompt?: string;
    maxOutputTokens?: number;
    effort?: RuntimeWorkerEffort;
    stream: boolean;
    mock: boolean;
    autoApprove: boolean;
    memory: RuntimeWorkerMemoryMode;
    promptVariant?: PromptVariant;
  }
  | PluginCommand
  | MarketplaceCommand
  | { type: "launch"; piArgs: string[]; agentDir?: string; withPiExtensions: boolean };

export type DiagnosticStatus = "pass" | "warn" | "fail";
export type Diagnostic = { status: DiagnosticStatus; name: string; message: string; details?: string };
export type RuntimeLoopMode = "off" | "command" | "default-with-fallback";
const DEFAULT_RUNTIME_LOOP_MODE: RuntimeLoopMode = "default-with-fallback";

function readPackageJson(path: string): any | undefined {
  try {
    return JSON.parse(readFileSync(path, "utf8"));
  } catch {
    return undefined;
  }
}

function isDirectory(path: string): boolean {
  try {
    return statSync(path).isDirectory();
  } catch {
    return false;
  }
}

function cliPackageJsonPath(): string {
  return resolve(__dirname, "..", "package.json");
}

function cliVersion(): string {
  return readPackageJson(cliPackageJsonPath())?.version ?? "0.0.0";
}

function expandHome(value: string): string {
  if (value === "~") return homedir();
  if (value.startsWith("~/") || value.startsWith("~\\")) return join(homedir(), value.slice(2));
  return value;
}

function redactText(value: string): string {
  return value
    .replace(/(?:sk-[a-zA-Z0-9_-]{12,}|[a-zA-Z0-9_-]{20,}\.[a-zA-Z0-9_-]{20,}\.[a-zA-Z0-9_-]{20,})/g, "[redacted-secret]")
    .replace(/(token|secret|password|api[_-]?key)(["'`\s:=]+)([^\s"'`,}]+)/gi, "$1$2[redacted]");
}

type Env = Record<string, string | undefined>;

type UpdateCheckCache = {
  lastCheckedAt?: string;
  latestVersion?: string;
  lastShownAt?: string;
};

export function resolveAgentDir(input?: string, env: Env = process.env): string {
  const raw = input?.trim() || env.OPPI_AGENT_DIR?.trim() || env.PI_CODING_AGENT_DIR?.trim() || join(homedir(), ".oppi", "agent");
  const expanded = expandHome(raw);
  return isAbsolute(expanded) ? resolve(expanded) : resolve(process.cwd(), expanded);
}

export function resolveDoctorAgentDir(input?: string, env: Env = process.env, cwd = process.cwd()): string {
  const raw = input?.trim() || env.OPPI_AGENT_DIR?.trim() || env.PI_CODING_AGENT_DIR?.trim() || join(cwd, ".oppi", "agent");
  const expanded = expandHome(raw);
  return isAbsolute(expanded) ? resolve(expanded) : resolve(cwd, expanded);
}

function resolveOppiHome(env: Env = process.env, cwd = process.cwd()): string {
  const raw = env.OPPI_HOME?.trim() || join(homedir(), ".oppi");
  const expanded = expandHome(raw);
  return isAbsolute(expanded) ? resolve(expanded) : resolve(cwd, expanded);
}

function managedPackagesDir(env: Env = process.env, cwd = process.cwd()): string {
  return join(resolveOppiHome(env, cwd), "packages");
}

function updateCheckCachePath(env: Env = process.env, cwd = process.cwd()): string {
  return join(resolveOppiHome(env, cwd), "update-check.json");
}

function readUpdateCheckCache(path: string): UpdateCheckCache {
  const value = readPackageJson(path);
  if (!value || typeof value !== "object") return {};
  return {
    lastCheckedAt: typeof value.lastCheckedAt === "string" ? value.lastCheckedAt : undefined,
    latestVersion: typeof value.latestVersion === "string" ? value.latestVersion : undefined,
    lastShownAt: typeof value.lastShownAt === "string" ? value.lastShownAt : undefined,
  };
}

function writeUpdateCheckCache(path: string, cache: UpdateCheckCache): void {
  try {
    mkdirSync(dirname(path), { recursive: true });
    writeFileSync(path, `${JSON.stringify(cache, null, 2)}\n`, "utf8");
  } catch {
    // Update checks must never break OPPi startup.
  }
}

function packageRootFromNodeModules(nodeModulesDir: string, packageName: string): string {
  return join(nodeModulesDir, ...packageName.split("/"));
}

function managedHoppiModulePath(env: Env = process.env, cwd = process.cwd(), packageName = HOPPI_PACKAGE_NAME): string {
  return join(packageRootFromNodeModules(join(managedPackagesDir(env, cwd), "node_modules"), packageName), "dist", "index.js");
}

function packageDirFromNodeModules(packageName: string): string | undefined {
  const searchPaths = require.resolve.paths?.(packageName) ?? [];
  const parts = packageName.split("/");
  for (const base of searchPaths) {
    const candidate = join(base, ...parts);
    if (existsSync(join(candidate, "package.json"))) return candidate;
  }
  return undefined;
}

function packageDirFromResolvedMain(packageName: string): string | undefined {
  try {
    const mainPath = require.resolve(packageName);
    // @mariozechner/pi-coding-agent resolves to dist/index.js when CommonJS resolution can use the package export.
    let dir = dirname(mainPath);
    for (let i = 0; i < 5; i += 1) {
      if (existsSync(join(dir, "package.json"))) return dir;
      dir = dirname(dir);
    }
  } catch {
    // ESM-only packages may not expose a require condition; fall back to node_modules package-root discovery.
  }
  return packageDirFromNodeModules(packageName);
}

export function resolvePiCliPath(env: Env = process.env): string | undefined {
  if (env.OPPI_PI_CLI?.trim()) return resolve(expandHome(env.OPPI_PI_CLI.trim()));
  const packageDir = packageDirFromResolvedMain("@mariozechner/pi-coding-agent");
  if (packageDir) {
    const candidate = join(packageDir, "dist", "cli.js");
    if (existsSync(candidate)) return candidate;
  }
  return undefined;
}

export function resolvePiPackagePath(env: Env = process.env, cwd = process.cwd()): string | undefined {
  const candidates: string[] = [];
  if (env.OPPI_PI_PACKAGE?.trim()) candidates.push(env.OPPI_PI_PACKAGE.trim());

  try {
    candidates.push(dirname(require.resolve("@oppiai/pi-package/package.json")));
  } catch {
    // Package may not be installed as a dependency in dev shells.
  }

  candidates.push(
    resolve(__dirname, "..", "..", "pi-package"),
    resolve(__dirname, "..", "..", "..", "packages", "pi-package"),
    resolve(cwd, "packages", "pi-package"),
  );

  for (const candidate of candidates) {
    const expanded = expandHome(candidate);
    const resolved = isAbsolute(expanded) ? resolve(expanded) : resolve(cwd, expanded);
    if (existsSync(join(resolved, "package.json"))) return resolved;
  }
  return undefined;
}

function resolvePackageEntryFile(packageDir: string): string | undefined {
  const packageJson = readPackageJson(join(packageDir, "package.json"));
  const candidates = [
    typeof packageJson?.module === "string" ? packageJson.module : undefined,
    typeof packageJson?.main === "string" ? packageJson.main : undefined,
    "dist/index.js",
    "index.js",
  ].filter((value): value is string => Boolean(value?.trim()));
  for (const candidate of candidates) {
    const resolved = resolve(packageDir, candidate);
    if (existsSync(resolved) && !isDirectory(resolved)) return resolved;
  }
  return undefined;
}

function resolveModuleCandidateFile(candidate: string, cwd: string): string | undefined {
  const expanded = expandHome(candidate);
  const resolved = isAbsolute(expanded) ? resolve(expanded) : resolve(cwd, expanded);
  if (!existsSync(resolved)) return undefined;
  if (isDirectory(resolved)) return resolvePackageEntryFile(resolved);
  return resolved;
}

function resolveHoppiModulePath(env: Env = process.env, cwd = process.cwd()): string | undefined {
  const candidates: string[] = [];
  if (env.OPPI_HOPPI_MODULE?.trim()) candidates.push(env.OPPI_HOPPI_MODULE.trim());

  candidates.push(
    managedHoppiModulePath(env, cwd, HOPPI_PACKAGE_NAME),
    managedHoppiModulePath(env, cwd, HOPPI_LEGACY_PACKAGE_NAME),
  );

  for (const packageName of [HOPPI_PACKAGE_NAME, HOPPI_LEGACY_PACKAGE_NAME]) {
    try {
      const main = require.resolve(packageName);
      candidates.push(main);
    } catch {
      // optional
    }
  }

  candidates.push(
    resolve(cwd, "..", "hoppi-memory", "dist", "index.js"),
    resolve(cwd, "hoppi-memory", "dist", "index.js"),
    resolve(__dirname, "..", "..", "..", "hoppi-memory", "dist", "index.js"),
  );

  for (const candidate of candidates) {
    const resolved = resolveModuleCandidateFile(candidate, cwd);
    if (resolved) return resolved;
  }
  return undefined;
}

function parseVersionParts(version: string): [number, number, number] {
  const match = version.match(/^(\d+)\.(\d+)\.(\d+)/);
  if (!match) return [0, 0, 0];
  return [Number(match[1]), Number(match[2]), Number(match[3])];
}

export function compareVersions(left: string, right: string): number {
  const a = parseVersionParts(left);
  const b = parseVersionParts(right);
  for (let i = 0; i < 3; i += 1) {
    if (a[i] > b[i]) return 1;
    if (a[i] < b[i]) return -1;
  }
  return 0;
}

function updateCheckDisabled(env: Env): boolean {
  const raw = (env.OPPI_UPDATE_CHECK ?? "").trim().toLowerCase();
  return env.OPPI_NO_UPDATE_CHECK === "1" || raw === "0" || raw === "false" || raw === "off" || raw === "no";
}

export function coerceRuntimeLoopMode(value: unknown): RuntimeLoopMode {
  const raw = typeof value === "string" ? value.trim().toLowerCase() : "";
  if (!raw) return DEFAULT_RUNTIME_LOOP_MODE;
  if (["0", "false", "disabled", "disable", "off", "none"].includes(raw)) return "off";
  if (["default", "default-with-fallback", "mirror", "on"].includes(raw)) return "default-with-fallback";
  if (["command", "opt-in", "manual", "runtime-loop"].includes(raw)) return "command";
  return "command";
}

function resolveRuntimeLoopMode(env: Env = process.env, cwd = process.cwd(), agentDir = resolveAgentDir(undefined, env)): RuntimeLoopMode {
  const global = readPackageJson(join(agentDir, "settings.json"))?.oppi?.runtimeLoop?.mode;
  const project = readPackageJson(join(cwd, ".pi", "settings.json"))?.oppi?.runtimeLoop?.mode;
  return coerceRuntimeLoopMode(env.OPPI_RUNTIME_LOOP_MODE ?? project ?? global ?? DEFAULT_RUNTIME_LOOP_MODE);
}

function isDue(timestamp: string | undefined, now: Date, intervalMs: number): boolean {
  if (!timestamp) return true;
  const parsed = Date.parse(timestamp);
  return !Number.isFinite(parsed) || now.getTime() - parsed >= intervalMs;
}

function npmRegistryLatestUrl(env: Env): string {
  const registry = (env.npm_config_registry || env.NPM_CONFIG_REGISTRY || "https://registry.npmjs.org/").trim() || "https://registry.npmjs.org/";
  const normalized = registry.endsWith("/") ? registry : `${registry}/`;
  return `${normalized}${encodeURIComponent(OPPI_CLI_PACKAGE_NAME).replace("%2F", "%2f")}/latest`;
}

async function fetchLatestCliVersion(env: Env, timeoutMs: number): Promise<string | undefined> {
  const forced = env.OPPI_UPDATE_CHECK_LATEST?.trim();
  if (forced) return forced;
  if (typeof fetch !== "function") return undefined;

  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), timeoutMs);
  try {
    const response = await fetch(npmRegistryLatestUrl(env), {
      signal: controller.signal,
      headers: { accept: "application/json" },
    });
    if (!response.ok) return undefined;
    const payload = await response.json();
    return payload && typeof payload === "object" && typeof (payload as Record<string, unknown>).version === "string"
      ? (payload as Record<string, string>).version
      : undefined;
  } catch {
    return undefined;
  } finally {
    clearTimeout(timeout);
  }
}

export type UpdateNoticePayload = {
  currentVersion: string;
  latestVersion: string;
  updateCommand: string;
  changelogUrl: string;
};

function formatUpdateNotice(payload: UpdateNoticePayload): string {
  return `OPPi ${payload.latestVersion} is available (installed ${payload.currentVersion}). Run ${payload.updateCommand}\nChangelog: ${payload.changelogUrl}`;
}

export async function checkForUpdateInfo(options: { env?: Env; cwd?: string; currentVersion?: string; now?: Date; timeoutMs?: number } = {}): Promise<UpdateNoticePayload | undefined> {
  const env = options.env ?? process.env;
  if (updateCheckDisabled(env)) return undefined;

  const currentVersion = options.currentVersion ?? cliVersion();
  const cwd = options.cwd ?? process.cwd();
  const now = options.now ?? new Date();
  const path = updateCheckCachePath(env, cwd);
  const cache = readUpdateCheckCache(path);
  let nextCache = { ...cache };
  let latestVersion = cache.latestVersion;

  if (isDue(cache.lastCheckedAt, now, UPDATE_CHECK_INTERVAL_MS)) {
    const fetched = await fetchLatestCliVersion(env, options.timeoutMs ?? UPDATE_CHECK_TIMEOUT_MS);
    latestVersion = fetched ?? latestVersion;
    nextCache = {
      ...nextCache,
      lastCheckedAt: now.toISOString(),
      latestVersion,
    };
    writeUpdateCheckCache(path, nextCache);
  }

  if (!latestVersion || compareVersions(latestVersion, currentVersion) <= 0) return undefined;
  if (!isDue(nextCache.lastShownAt, now, UPDATE_NOTICE_INTERVAL_MS)) return undefined;

  nextCache = { ...nextCache, lastShownAt: now.toISOString(), latestVersion };
  writeUpdateCheckCache(path, nextCache);
  return { currentVersion, latestVersion, updateCommand: "oppi update", changelogUrl: OPPI_CHANGELOG_URL };
}

export async function checkForUpdateNotice(options: { env?: Env; cwd?: string; currentVersion?: string; now?: Date; timeoutMs?: number } = {}): Promise<string | undefined> {
  const payload = await checkForUpdateInfo(options);
  return payload ? formatUpdateNotice(payload) : undefined;
}

function shouldCheckForUpdatesOnLaunch(command: Extract<OppiCommand, { type: "launch" }>): boolean {
  return !command.piArgs.includes("-p") && !command.piArgs.includes("--print");
}

async function resolveUpdateNoticeEnv(command: Extract<OppiCommand, { type: "launch" }>): Promise<Env> {
  if (!shouldCheckForUpdatesOnLaunch(command)) return {};
  try {
    const notice = await checkForUpdateInfo();
    if (!notice) return {};
    return {
      OPPI_UPDATE_CURRENT_VERSION: notice.currentVersion,
      OPPI_UPDATE_LATEST_VERSION: notice.latestVersion,
      OPPI_CHANGELOG_URL: notice.changelogUrl,
    };
  } catch {
    // Update checks are best-effort only.
    return {};
  }
}

function parsePromptVariant(value: unknown, throwOnInvalid = false): PromptVariant | undefined {
  if (typeof value !== "string") return undefined;
  const normalized = value.trim().toLowerCase();
  if (!normalized || normalized === "off" || normalized === "none" || normalized === "default") return "off";
  if (["a", "prompt-a", "promptname_a", "loop", "agentic", "normal"].includes(normalized)) return "promptname_a";
  if (["b", "prompt-b", "promptname_b", "caveman", "compressed"].includes(normalized)) return "promptname_b";
  if (throwOnInvalid) throw new Error(`Unknown prompt variant: ${value}`);
  return undefined;
}

export function parseRuntimeWorkerEffort(value: string | undefined, throwOnInvalid = false): RuntimeWorkerEffort | undefined {
  const normalized = value?.trim().toLowerCase();
  if (!normalized) return undefined;
  if (normalized === "none" || normalized === "default") return "off";
  if (normalized === "min") return "minimal";
  if (normalized === "med") return "medium";
  if (normalized === "max") return "xhigh";
  if (["off", "minimal", "low", "medium", "high", "xhigh"].includes(normalized)) return normalized as RuntimeWorkerEffort;
  if (throwOnInvalid) throw new Error(`Unknown runtime-worker effort: ${value}`);
  return undefined;
}

export function parseRuntimeWorkerProvider(value: string | undefined, throwOnInvalid = false): RuntimeWorkerProvider | undefined {
  const normalized = value?.trim().toLowerCase();
  if (!normalized) return undefined;
  if (["openai-compatible", "openai", "api", "api-key"].includes(normalized)) return "openai-compatible";
  if (["openai-codex", "codex", "chatgpt", "subscription"].includes(normalized)) return "openai-codex";
  if (throwOnInvalid) throw new Error(`Unknown runtime-worker provider: ${value}`);
  return undefined;
}

function openAiCompatibleReasoningEffort(effort: RuntimeWorkerEffort | undefined, diagnostics: string[] = []): string | undefined {
  if (!effort || effort === "off") return undefined;
  if (effort === "xhigh") {
    diagnostics.push("OPPi direct-worker effort xhigh maps to OpenAI-compatible reasoning_effort=high until provider-specific max-effort mapping lands.");
    return "high";
  }
  return effort;
}

function selectedRuntimeWorkerEffort(command: Extract<OppiCommand, { type: "runtime-worker"; subcommand: "run" }>, env: Env = process.env): RuntimeWorkerEffort | undefined {
  return command.effort ?? parseRuntimeWorkerEffort(env.OPPI_RUNTIME_WORKER_EFFORT);
}

function selectedRuntimeWorkerProvider(command: Extract<OppiCommand, { type: "runtime-worker"; subcommand: "run" }>, env: Env = process.env): RuntimeWorkerProvider {
  return command.provider ?? parseRuntimeWorkerProvider(env.OPPI_RUNTIME_WORKER_PROVIDER) ?? "openai-compatible";
}

function parseRuntimeWorkerCommand(rest: string[]): Extract<OppiCommand, { type: "runtime-worker" }> {
  let json = false;
  let provider: RuntimeWorkerProvider | undefined;
  let model: string | undefined;
  let baseUrl: string | undefined;
  let apiKeyEnv: string | undefined;
  let systemPrompt: string | undefined;
  let maxOutputTokens: number | undefined;
  let effort: RuntimeWorkerEffort | undefined;
  let stream = true;
  let mock = false;
  let autoApprove = false;
  let memory: RuntimeWorkerMemoryMode = "auto";
  let promptVariant: PromptVariant | undefined;
  const positional: string[] = [];

  const readValue = (args: string[], index: number, flag: string): { value: string; nextIndex: number } => {
    const value = args[index + 1];
    if (!value) throw new Error(`${flag} requires a value`);
    return { value, nextIndex: index + 1 };
  };

  for (let i = 0; i < rest.length; i += 1) {
    const arg = rest[i];
    if (arg === "--") {
      positional.push(...rest.slice(i + 1));
      break;
    }
    if (arg === "--json") {
      json = true;
      continue;
    }
    if (arg === "--mock") {
      mock = true;
      continue;
    }
    if (arg === "--auto-approve" || arg === "--approve-all") {
      autoApprove = true;
      continue;
    }
    if (arg === "--memory") {
      memory = "on";
      continue;
    }
    if (arg === "--no-memory") {
      memory = "off";
      continue;
    }
    if (arg === "--no-stream") {
      stream = false;
      continue;
    }
    if (arg === "--provider" || arg === "--model" || arg === "--base-url" || arg === "--api-key-env" || arg === "--system" || arg === "--system-prompt" || arg === "--max-output-tokens" || arg === "--effort" || arg === "--reasoning-effort" || arg === "--prompt-variant" || arg === "--variant") {
      const { value, nextIndex } = readValue(rest, i, arg);
      i = nextIndex;
      if (arg === "--provider") provider = parseRuntimeWorkerProvider(value, true);
      else if (arg === "--model") model = value;
      else if (arg === "--base-url") baseUrl = value;
      else if (arg === "--api-key-env") apiKeyEnv = value;
      else if (arg === "--system" || arg === "--system-prompt") systemPrompt = value;
      else if (arg === "--max-output-tokens") maxOutputTokens = Number(value);
      else if (arg === "--effort" || arg === "--reasoning-effort") effort = parseRuntimeWorkerEffort(value, true);
      else if (arg === "--prompt-variant" || arg === "--variant") promptVariant = parsePromptVariant(value, true);
      continue;
    }
    if (arg.startsWith("--model=")) {
      model = arg.slice("--model=".length);
      continue;
    }
    if (arg.startsWith("--provider=")) {
      provider = parseRuntimeWorkerProvider(arg.slice("--provider=".length), true);
      continue;
    }
    if (arg.startsWith("--base-url=")) {
      baseUrl = arg.slice("--base-url=".length);
      continue;
    }
    if (arg.startsWith("--api-key-env=")) {
      apiKeyEnv = arg.slice("--api-key-env=".length);
      continue;
    }
    if (arg.startsWith("--system=")) {
      systemPrompt = arg.slice("--system=".length);
      continue;
    }
    if (arg.startsWith("--system-prompt=")) {
      systemPrompt = arg.slice("--system-prompt=".length);
      continue;
    }
    if (arg.startsWith("--max-output-tokens=")) {
      maxOutputTokens = Number(arg.slice("--max-output-tokens=".length));
      continue;
    }
    if (arg.startsWith("--effort=")) {
      effort = parseRuntimeWorkerEffort(arg.slice("--effort=".length), true);
      continue;
    }
    if (arg.startsWith("--reasoning-effort=")) {
      effort = parseRuntimeWorkerEffort(arg.slice("--reasoning-effort=".length), true);
      continue;
    }
    if (arg.startsWith("--memory=")) {
      memory = coerceRuntimeWorkerMemoryMode(arg.slice("--memory=".length));
      continue;
    }
    if (arg.startsWith("--prompt-variant=")) {
      promptVariant = parsePromptVariant(arg.slice("--prompt-variant=".length), true);
      continue;
    }
    if (arg.startsWith("--variant=")) {
      promptVariant = parsePromptVariant(arg.slice("--variant=".length), true);
      continue;
    }
    positional.push(arg);
  }

  if (!Number.isFinite(maxOutputTokens ?? 1) || (maxOutputTokens !== undefined && maxOutputTokens <= 0)) throw new Error("--max-output-tokens must be a positive number");
  if (positional[0] === "smoke" || positional.length === 0) return { type: "runtime-worker", subcommand: "smoke", json };
  const promptParts = positional[0] === "run" ? positional.slice(1) : positional;
  const prompt = promptParts.join(" ").trim();
  if (!prompt) throw new Error("oppi runtime-worker run requires a prompt");
  return {
    type: "runtime-worker",
    subcommand: "run",
    json,
    prompt,
    ...(provider ? { provider } : {}),
    model,
    baseUrl,
    apiKeyEnv,
    systemPrompt,
    maxOutputTokens,
    stream,
    mock,
    autoApprove,
    memory,
    ...(effort ? { effort } : {}),
    ...(promptVariant ? { promptVariant } : {}),
  };
}

function parseTuiCommand(rest: string[], agentDir?: string): Extract<OppiCommand, { type: "tui" }> {
  const json = rest.includes("--json");
  const experimental = rest.includes("--experimental");
  const firstPositional = rest.find((item) => !item.startsWith("-"));
  const subcommand = firstPositional === "smoke" || firstPositional === "dogfood" ? firstPositional : "run";
  const shellArgs = rest.filter((item, index) => {
    if (item === "--experimental") return false;
    if (subcommand !== "run" && item === subcommand && rest.findIndex((candidate) => !candidate.startsWith("-")) === index) return false;
    return true;
  });
  return { type: "tui", subcommand, experimental, json, shellArgs, agentDir };
}

function parseResumeCommand(rest: string[], agentDir?: string): Extract<OppiCommand, { type: "resume" }> {
  const json = rest.includes("--json");
  const threadId = rest.find((item) => !item.startsWith("-"));
  const shellArgs = threadId ? ["--resume", threadId] : ["--list-sessions"];
  if (json) shellArgs.push("--json");
  return { type: "resume", threadId, json, shellArgs, agentDir };
}

export function parseOppiArgs(argv: string[]): OppiCommand {
  let agentDir: string | undefined;
  let withPiExtensions = false;
  const remaining: string[] = [];

  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--agent-dir") {
      const value = argv[i + 1];
      if (!value) throw new Error("--agent-dir requires a directory path");
      agentDir = value;
      i += 1;
      continue;
    }
    if (arg.startsWith("--agent-dir=")) {
      agentDir = arg.slice("--agent-dir=".length);
      continue;
    }
    if (arg === "--with-pi-extensions") {
      withPiExtensions = true;
      continue;
    }
    remaining.push(arg);
  }

  if (remaining.length === 0) return { type: "launch", piArgs: [], agentDir, withPiExtensions };

  const [first, ...rest] = remaining;
  if (first === "--help" || first === "-h") return { type: "help" };
  if (first === "--version" || first === "-v") return { type: "version" };
  if (first === "doctor") return { type: "doctor", json: rest.includes("--json"), agentDir };
  if (first === "update") return { type: "update", check: rest.includes("--check"), json: rest.includes("--json") };
  if (first === "plugin") return parsePluginCommand(rest);
  if (first === "marketplace") return parseMarketplaceCommand(rest);
  if (first === "server") {
    return {
      type: "server",
      stdio: rest.includes("--stdio") || !rest.includes("--help"),
      experimental: rest.includes("--experimental"),
      json: rest.includes("--json"),
    };
  }
  if (first === "tui" || first === "shell") return parseTuiCommand(rest, agentDir);
  if (first === "resume") return parseResumeCommand(rest, agentDir);
  if (first === "runtime-loop" || first === "runtime") {
    const json = rest.includes("--json");
    const sub = rest.find((item) => !item.startsWith("-")) ?? "smoke";
    if (sub === "smoke") return { type: "runtime-loop", subcommand: "smoke", json };
    throw new Error(`Unknown oppi runtime-loop command: ${sub}`);
  }
  if (first === "runtime-worker" || first === "worker") return parseRuntimeWorkerCommand(rest);
  if (first === "sandbox") return parseSandboxCommand(rest);
  if (first === "natives" || first === "native") {
    const json = rest.includes("--json");
    const sub = rest.find((item) => !item.startsWith("-")) ?? "status";
    if (sub === "status" || sub === "benchmark") return { type: "natives", subcommand: sub, json };
    throw new Error(`Unknown oppi natives command: ${sub}`);
  }
  if (first === "mem") {
    const json = rest.includes("--json");
    const sub = rest.find((item) => !item.startsWith("-")) ?? "status";
    if (sub === "status" || sub === "setup" || sub === "install" || sub === "dashboard" || sub === "open") {
      return { type: "mem", subcommand: sub, json };
    }
    throw new Error(`Unknown oppi mem command: ${sub}`);
  }

  return { type: "launch", piArgs: remaining, agentDir, withPiExtensions };
}

export function buildPiArgs(command: Extract<OppiCommand, { type: "launch" }>, piPackagePath: string, pluginSources: string[] = []): string[] {
  const args: string[] = [];
  if (!command.withPiExtensions) args.push("--no-extensions");
  args.push("-e", piPackagePath);
  for (const source of pluginSources) args.push("-e", source);
  args.push(...command.piArgs);
  return args;
}

async function launchPi(command: Extract<OppiCommand, { type: "launch" }>): Promise<number> {
  const piCli = resolvePiCliPath();
  const piPackage = resolvePiPackagePath();
  if (!piCli) {
    console.error("OPPi could not resolve Pi's CLI. Reinstall dependencies or set OPPI_PI_CLI.");
    return Promise.resolve(1);
  }
  if (!piPackage) {
    console.error("OPPi could not resolve @oppiai/pi-package. Run from the monorepo or set OPPI_PI_PACKAGE.");
    return Promise.resolve(1);
  }

  const agentDir = resolveAgentDir(command.agentDir);
  mkdirSync(agentDir, { recursive: true });

  const updateNoticeEnv = await resolveUpdateNoticeEnv(command);

  const pluginSources = resolveEnabledPluginSources();
  const child = spawn(process.execPath, [piCli, ...buildPiArgs(command, piPackage, pluginSources)], {
    stdio: "inherit",
    env: {
      ...process.env,
      ...updateNoticeEnv,
      OPPI_CLI: "1",
      OPPI_AGENT_DIR: agentDir,
      PI_CODING_AGENT_DIR: agentDir,
    },
  });

  return new Promise((resolveExit) => {
    child.on("error", (error: Error) => {
      console.error(`OPPi failed to start Pi: ${error.message}`);
      resolveExit(1);
    });
    child.on("exit", (code: number | null, signal: string | null) => {
      if (signal) {
        const signalNumber = signal === "SIGINT" ? 130 : signal === "SIGTERM" ? 143 : 1;
        resolveExit(signalNumber);
      } else {
        resolveExit(code ?? 0);
      }
    });
  });
}

function checkWritableDir(path: string): { ok: boolean; error?: string } {
  try {
    mkdirSync(path, { recursive: true });
    const file = join(path, ".oppi-doctor-write-test");
    writeFileSync(file, "ok", "utf8");
    rmSync(file, { force: true });
    return { ok: true };
  } catch (error) {
    return { ok: false, error: error instanceof Error ? error.message : String(error) };
  }
}

function nodeVersionAtLeast(major: number, minor: number): boolean {
  const [actualMajor, actualMinor] = process.versions.node.split(".").map((part: string) => Number(part));
  return actualMajor > major || (actualMajor === major && actualMinor >= minor);
}

function collectRustRuntimeDiagnostic(env: Env = process.env, cwd = process.cwd()): Diagnostic {
  const serverBin = resolveOppiServerBin(env, cwd);
  if (!serverBin) {
    return {
      status: "warn",
      name: "Rust runtime",
      message: "oppi-server not found; build with `cargo build -p oppi-server` or set OPPI_SERVER_BIN for experimental runtime/sandbox diagnostics",
    };
  }
  const version = spawnSync(serverBin, ["--version"], {
    encoding: "utf8",
    timeout: 2_000,
    windowsHide: true,
  });
  if (version.error) {
    return {
      status: "warn",
      name: "Rust runtime",
      message: `${serverBin} found but --version failed`,
      details: version.error.message,
    };
  }
  if (version.status !== 0) {
    return {
      status: "warn",
      name: "Rust runtime",
      message: `${serverBin} found but --version exited ${version.status}`,
      details: [version.stderr, version.stdout].filter(Boolean).join("\n"),
    };
  }
  return {
    status: "pass",
    name: "Rust runtime",
    message: `oppi-server ${version.stdout.trim() || "available"} at ${serverBin}`,
  };
}

function collectRustProtocolSandboxDiagnostic(env: Env = process.env, cwd = process.cwd()): Diagnostic {
  const serverBin = resolveOppiServerBin(env, cwd);
  if (!serverBin) {
    return {
      status: "warn",
      name: "Rust protocol/sandbox",
      message: "oppi-server not found; protocol and sandbox probes are unavailable",
    };
  }
  const authToken = env.OPPI_SERVER_AUTH_TOKEN?.trim();
  const authedParams = authToken ? { authToken } : {};
  const requests = [
    { jsonrpc: "2.0", id: 1, method: "initialize", params: { clientName: "oppi-doctor", clientVersion: cliVersion(), protocolVersion: OPPI_PROTOCOL_VERSION, clientCapabilities: ["shell", "sandbox"] } },
    { jsonrpc: "2.0", id: 2, method: "sandbox/status", params: authedParams },
    { jsonrpc: "2.0", id: 3, method: "server/shutdown", params: authedParams },
  ].map((request) => JSON.stringify(request)).join("\n") + "\n";
  const target = windowsCmdShim(serverBin, ["--stdio"]);
  const result = spawnSync(target.command, target.args, {
    input: requests,
    encoding: "utf8",
    timeout: 3_000,
    windowsHide: true,
    maxBuffer: 1024 * 1024,
    env: { ...env, OPPI_EXPERIMENTAL_RUNTIME: "1" },
    cwd,
  });
  if (result.error) {
    return {
      status: "warn",
      name: "Rust protocol/sandbox",
      message: `${serverBin} probe failed`,
      details: result.error.message,
    };
  }
  const responses = String(result.stdout ?? "")
    .split(/\r?\n/)
    .filter(Boolean)
    .map((line) => {
      try {
        return JSON.parse(line) as any;
      } catch {
        return undefined;
      }
    })
    .filter(Boolean);
  const initialize = responses.find((response) => response.id === 1)?.result;
  const sandbox = responses.find((response) => response.id === 2)?.result;
  const errors = responses.filter((response) => response.error).map((response) => response.error?.message).filter(Boolean);
  if (!initialize || errors.length > 0) {
    return {
      status: "warn",
      name: "Rust protocol/sandbox",
      message: `${serverBin} did not complete protocol/sandbox probe`,
      details: redactText([errors.join("\n"), result.stderr, result.stdout].filter(Boolean).join("\n")).trim() || undefined,
    };
  }
  const compatible = initialize.protocolCompatible !== false;
  const sandboxSummary = sandbox
    ? `${sandbox.enforcement ?? "unknown"} on ${sandbox.platform ?? "unknown"}${sandbox.supported === false ? " (degraded)" : ""}`
    : "not reported";
  return {
    status: compatible && sandbox ? "pass" : "warn",
    name: "Rust protocol/sandbox",
    message: `protocol ${initialize.protocolVersion ?? "unknown"} ${compatible ? "compatible" : "incompatible"}; sandbox ${sandboxSummary}`,
    details: [
      `server: ${initialize.serverName ?? "oppi-server"} ${initialize.serverVersion ?? "unknown"}`,
      `capabilities: ${(initialize.serverCapabilities ?? []).join(", ") || "unknown"}`,
      sandbox?.message ? `sandbox: ${sandbox.message}` : undefined,
    ].filter(Boolean).join("\n"),
  };
}

function directWorkerApiKeyEnvName(env: Env = process.env): string | undefined {
  const configured = env.OPPI_RUNTIME_WORKER_API_KEY_ENV?.trim();
  return configured || undefined;
}

function isDirectWorkerApiKeyEnvAllowed(name: string | undefined): boolean {
  if (!name) return true;
  if (name.length > 128 || !/^[A-Z0-9_]+$/.test(name)) return false;
  return name === "OPPI_OPENAI_API_KEY" || name === "OPENAI_API_KEY"
    || (name.startsWith("OPPI_") && name.endsWith("_API_KEY"))
    || (name.startsWith("OPENAI_") && name.endsWith("_API_KEY"))
    || (name.startsWith("AZURE_OPENAI_") && name.endsWith("_API_KEY"));
}

function hasDirectWorkerProviderAuth(env: Env = process.env, apiKeyEnv = directWorkerApiKeyEnvName(env)): boolean {
  if (apiKeyEnv) return isDirectWorkerApiKeyEnvAllowed(apiKeyEnv) && Boolean(env[apiKeyEnv]?.trim());
  return Boolean(env.OPPI_OPENAI_API_KEY?.trim() || env.OPENAI_API_KEY?.trim());
}

function runtimeWorkerCodexAuthPath(env: Env = process.env): string {
  const explicit = env.OPPI_OPENAI_CODEX_AUTH_PATH?.trim();
  return explicit ? resolve(expandHome(explicit)) : join(resolveAgentDir(undefined, env), "auth.json");
}

function hasRuntimeWorkerCodexAuth(env: Env = process.env): boolean {
  const path = runtimeWorkerCodexAuthPath(env);
  try {
    const raw = JSON.parse(readFileSync(path, "utf8"));
    const credential = raw?.["openai-codex"];
    return credential?.type === "oauth" && typeof credential.access === "string" && typeof credential.refresh === "string";
  } catch {
    return false;
  }
}

function runtimeWorkerFallbackCommand(prompt?: string): string {
  if (!prompt) return "oppi \"<prompt>\"";
  return `oppi ${JSON.stringify(prompt)}`;
}

function coerceRuntimeWorkerPermissionMode(value: unknown): RuntimeWorkerPermissionMode {
  const raw = typeof value === "string" ? value.trim().toLowerCase() : "";
  if (raw === "read-only" || raw === ":read-only") return "read-only";
  if (raw === "default") return "default";
  if (raw === "full-access" || raw === ":danger-no-sandbox") return "full-access";
  return "auto-review";
}

function runtimeWorkerPermissionSettingsPath(env: Env = process.env, cwd = process.cwd()): string {
  const explicit = env.OPPI_SETTINGS_PATH?.trim();
  if (explicit) {
    const expanded = expandHome(explicit);
    return isAbsolute(expanded) ? resolve(expanded) : resolve(cwd, expanded);
  }
  return join(resolveAgentDir(undefined, env), "settings.json");
}

function selectedRuntimeWorkerPermissionMode(env: Env = process.env, cwd = process.cwd()): RuntimeWorkerPermissionMode {
  if (env.OPPI_RUNTIME_WORKER_PERMISSION_MODE !== undefined) return coerceRuntimeWorkerPermissionMode(env.OPPI_RUNTIME_WORKER_PERMISSION_MODE);
  const globalMode = readPackageJson(runtimeWorkerPermissionSettingsPath(env, cwd))?.oppi?.permissions?.mode;
  const projectMode = readPackageJson(join(cwd, ".pi", "settings.json"))?.oppi?.permissions?.mode;
  return coerceRuntimeWorkerPermissionMode(projectMode ?? globalMode ?? "auto-review");
}

function runtimeWorkerSandboxPolicy(mode: RuntimeWorkerPermissionMode, cwd: string): RuntimeWorkerSandboxPolicy {
  const writable = mode === "read-only" ? [] : [cwd];
  return {
    permissionProfile: {
      mode,
      readableRoots: [cwd],
      writableRoots: writable,
      filesystemRules: [],
      protectedPatterns: [".env*", ".ssh/", "*.pem", "*.key", ".git/config", ".git/hooks/", ".npmrc", ".pypirc", ".mcp.json", ".claude.json"],
    },
    network: mode === "full-access" ? "enabled" : "disabled",
    filesystem: mode === "read-only" ? "readOnly" : mode === "full-access" ? "unrestricted" : "workspaceWrite",
  };
}

function coerceRuntimeWorkerMemoryMode(value: unknown): RuntimeWorkerMemoryMode {
  const raw = typeof value === "string" ? value.trim().toLowerCase() : "";
  if (!raw || raw === "auto" || raw === "default") return "auto";
  if (["1", "true", "yes", "on", "enabled", "enable"].includes(raw)) return "on";
  if (["0", "false", "no", "off", "disabled", "disable", "none"].includes(raw)) return "off";
  return "auto";
}

function runtimeWorkerMemorySettingsPath(env: Env = process.env, cwd = process.cwd()): string {
  const explicit = env.OPPI_SETTINGS_PATH?.trim();
  if (explicit) {
    const expanded = expandHome(explicit);
    return isAbsolute(expanded) ? resolve(expanded) : resolve(cwd, expanded);
  }
  return join(resolveAgentDir(undefined, env), "settings.json");
}

type RuntimeWorkerPromptVariantSelection = {
  variant: PromptVariant;
  applied: boolean;
  text?: string;
  path?: string;
  diagnostics: string[];
};

type RuntimeWorkerFeatureGuidanceSelection = {
  applied: boolean;
  text?: string;
  path?: string;
  variantApplied: boolean;
  variantText?: string;
  variantPath?: string;
  diagnostics: string[];
};

const RUNTIME_WORKER_FEATURE_GUIDANCE_FALLBACK = `# OPPi feature routing

Use OPPi's extra capabilities when they fit the user's request, not as theater.

- Use todo_write for multi-step coding/debugging/refactor work; keep it concise and current.
- Use ask_user only when a real decision or missing requirement blocks safe progress.
- Use shell_exec for builds, tests, git/package-manager commands, Docker, and shell-native diagnostics; use shell_task for long-running background commands.
- Parallelize independent read/search/list/shell diagnostics when safe; serialize dependent edits and side effects.
- Use image_gen for image creation/editing requests and render_mermaid when a small terminal diagram helps.
- Use suggest_next_message only for highly predictable, short next replies.
- Treat permission denials, compaction summaries, follow-up-chain context, and tool results as runtime facts to incorporate into progress/final answers.
- For slash-agent management, use or recommend /agents; do not pretend full subagent dispatch exists unless the runtime/tool surface exposes it.`;

function promptCatalogPathCandidates(relativePath: string, env: Env = process.env, cwd = process.cwd()): string[] {
  const explicitBase = env.OPPI_PROMPT_CATALOG_DIR?.trim();
  const bases = [
    explicitBase,
    cwd,
    resolve(__dirname, "..", "..", ".."),
  ].filter((value): value is string => Boolean(value));
  const candidates: string[] = [];
  for (const base of bases) {
    const expanded = expandHome(base);
    const resolvedBase = isAbsolute(expanded) ? resolve(expanded) : resolve(cwd, expanded);
    candidates.push(resolve(resolvedBase, relativePath));
    if (relativePath.startsWith("systemprompts/")) {
      candidates.push(resolve(resolvedBase, relativePath.slice("systemprompts/".length)));
    }
  }
  return [...new Set(candidates)];
}

function readPromptCatalogFile(relativePath: string, env: Env = process.env, cwd = process.cwd()): { text?: string; path?: string; error?: string } {
  const path = promptCatalogPathCandidates(relativePath, env, cwd).find((candidate) => existsSync(candidate) && !isDirectory(candidate));
  if (!path) return { error: `Prompt variant file not found: ${relativePath}` };
  try {
    const text = readFileSync(path, "utf8").trim();
    return text ? { text, path } : { path, error: `Prompt variant file is empty: ${relativePath}` };
  } catch (error) {
    return { path, error: error instanceof Error ? error.message : String(error) };
  }
}

function selectedRuntimeWorkerPromptVariant(command: Extract<OppiCommand, { type: "runtime-worker"; subcommand: "run" }>, env: Env = process.env, cwd = process.cwd()): PromptVariant {
  if (command.promptVariant) return command.promptVariant;
  if (env.OPPI_SYSTEM_PROMPT_VARIANT !== undefined) return parsePromptVariant(env.OPPI_SYSTEM_PROMPT_VARIANT) ?? "off";
  return parsePromptVariant(readPackageJson(runtimeWorkerMemorySettingsPath(env, cwd))?.oppi?.promptVariant) ?? "off";
}

function prepareRuntimeWorkerPromptVariant(command: Extract<OppiCommand, { type: "runtime-worker"; subcommand: "run" }>, env: Env = process.env, cwd = process.cwd()): RuntimeWorkerPromptVariantSelection {
  const diagnostics: string[] = [];
  const variant = selectedRuntimeWorkerPromptVariant(command, env, cwd);
  if (variant === "off") return { variant, applied: false, diagnostics };
  const relativePath = `systemprompts/experiments/${variant}/main-system-append.md`;
  const read = readPromptCatalogFile(relativePath, env, cwd);
  if (!read.text) {
    diagnostics.push(`OPPi prompt variant ${variant} skipped for direct worker: ${read.error ?? "empty variant file"}`);
    return { variant, applied: false, path: read.path, diagnostics };
  }
  diagnostics.push(`OPPi prompt variant ${variant} applied to the direct-worker provider prompt.`);
  return { variant, applied: true, text: read.text, path: read.path, diagnostics };
}

function appendPromptVariantToSystemPrompt(systemPrompt: unknown, selection: RuntimeWorkerPromptVariantSelection): string | undefined {
  const base = typeof systemPrompt === "string" ? systemPrompt.trim() : "";
  if (!selection.applied || !selection.text) return base || undefined;
  return [base, `<!-- OPPi prompt variant: ${selection.variant} (${selection.path ?? "unknown path"}) -->\n\n${selection.text}`].filter(Boolean).join("\n\n");
}

function prepareRuntimeWorkerFeatureGuidance(promptVariant: RuntimeWorkerPromptVariantSelection, env: Env = process.env, cwd = process.cwd()): RuntimeWorkerFeatureGuidanceSelection {
  const diagnostics: string[] = [];
  const base = readPromptCatalogFile("systemprompts/main/oppi-feature-routing-system-append.md", env, cwd);
  const text = base.text ?? RUNTIME_WORKER_FEATURE_GUIDANCE_FALLBACK;
  if (!base.text) diagnostics.push(`OPPi feature-routing guidance used built-in fallback for direct worker: ${base.error ?? "empty guidance file"}`);
  const selection: RuntimeWorkerFeatureGuidanceSelection = {
    applied: true,
    text,
    path: base.path,
    variantApplied: false,
    diagnostics,
  };
  if (promptVariant.variant !== "off") {
    const variant = readPromptCatalogFile(`systemprompts/experiments/${promptVariant.variant}/oppi-feature-routing-system-append.md`, env, cwd);
    if (variant.text) {
      selection.variantApplied = true;
      selection.variantText = variant.text;
      selection.variantPath = variant.path;
    } else {
      diagnostics.push(`OPPi feature-routing variant ${promptVariant.variant} skipped for direct worker: ${variant.error ?? "empty variant file"}`);
    }
  }
  return selection;
}

function appendFeatureGuidanceToSystemPrompt(systemPrompt: unknown, selection: RuntimeWorkerFeatureGuidanceSelection): string | undefined {
  const base = typeof systemPrompt === "string" ? systemPrompt.trim() : "";
  const parts = [base];
  if (selection.applied && selection.text) {
    parts.push(`<!-- OPPi feature routing (${selection.path ?? "built-in fallback"}) -->\n\n${selection.text}`);
  }
  if (selection.variantApplied && selection.variantText) {
    parts.push(`<!-- OPPi feature routing variant (${selection.variantPath ?? "unknown path"}) -->\n\n${selection.variantText}`);
  }
  return parts.filter(Boolean).join("\n\n") || undefined;
}

function normalizeRuntimeWorkerFollowUpStatus(value: unknown): RuntimeWorkerFollowUpStatus {
  return value === "running" || value === "completed" ? value : "queued";
}

function trimRuntimeWorkerString(value: unknown, max: number): string | undefined {
  if (typeof value !== "string") return undefined;
  const trimmed = value.trim();
  if (!trimmed) return undefined;
  return trimmed.length > max ? `${trimmed.slice(0, Math.max(0, max - 1)).trimEnd()}…` : trimmed;
}

function prepareRuntimeWorkerFollowUpChain(promptVariant: RuntimeWorkerPromptVariantSelection, env: Env = process.env, cwd = process.cwd(), diagnostics: string[] = []): RuntimeWorkerFollowUpChain | undefined {
  const raw = env.OPPI_RUNTIME_WORKER_FOLLOW_UP_JSON?.trim();
  if (!raw) return undefined;
  try {
    const parsed = JSON.parse(raw);
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) throw new Error("follow-up context must be a JSON object");
    const rootPrompt = trimRuntimeWorkerString((parsed as any).rootPrompt, 2_000);
    if (!rootPrompt) throw new Error("follow-up context requires rootPrompt");
    const followUps = Array.isArray((parsed as any).followUps)
      ? (parsed as any).followUps
        .map((item: any, index: number) => {
          const text = trimRuntimeWorkerString(item?.text, 1_000);
          if (!text) return undefined;
          return {
            id: trimRuntimeWorkerString(item?.id, 120) ?? String(index + 1),
            text,
            status: normalizeRuntimeWorkerFollowUpStatus(item?.status),
          };
        })
        .filter(Boolean)
      : undefined;
    const chain: RuntimeWorkerFollowUpChain = {
      rootPrompt,
      ...(trimRuntimeWorkerString((parsed as any).chainId, 120) ? { chainId: trimRuntimeWorkerString((parsed as any).chainId, 120) } : {}),
      ...(followUps?.length ? { followUps } : {}),
      ...(trimRuntimeWorkerString((parsed as any).currentFollowUpId, 120) ? { currentFollowUpId: trimRuntimeWorkerString((parsed as any).currentFollowUpId, 120) } : {}),
      ...(trimRuntimeWorkerString((parsed as any).promptVariantAppend, 2_000) ? { promptVariantAppend: trimRuntimeWorkerString((parsed as any).promptVariantAppend, 2_000) } : {}),
    };
    if (promptVariant.variant !== "off") {
      const variant = readPromptCatalogFile(`systemprompts/experiments/${promptVariant.variant}/follow-up-chain-system-append.md`, env, cwd);
      if (variant.text) {
        chain.promptVariantAppend = [chain.promptVariantAppend, variant.text].filter(Boolean).join("\n\n");
      } else {
        diagnostics.push(`OPPi follow-up variant ${promptVariant.variant} skipped for direct worker: ${variant.error ?? "empty variant file"}`);
      }
    }
    diagnostics.push("OPPi follow-up chain context will be delegated to Rust for direct-worker prompt routing.");
    return chain;
  } catch (error) {
    diagnostics.push(`Ignored invalid OPPI_RUNTIME_WORKER_FOLLOW_UP_JSON: ${redactText(error instanceof Error ? error.message : String(error))}`);
    return undefined;
  }
}

function collectRuntimeLoopDiagnostic(env: Env = process.env, cwd = process.cwd(), agentDir = resolveAgentDir(undefined, env)): Diagnostic {
  const mode = resolveRuntimeLoopMode(env, cwd, agentDir);
  const serverBin = resolveOppiServerBin(env, cwd);
  if (mode === "off") {
    return {
      status: "warn",
      name: "Rust loop dogfood",
      message: "OPPI_RUNTIME_LOOP_MODE=off; /runtime-loop and automatic Rust-loop mirroring are disabled",
      details: "Unset OPPI_RUNTIME_LOOP_MODE for default automatic mirroring, or use OPPI_RUNTIME_LOOP_MODE=command for opt-in /runtime-loop only.",
    };
  }
  if (!serverBin) {
    return {
      status: "warn",
      name: "Rust loop dogfood",
      message: `${mode}; oppi-server not found, so /runtime-loop cannot mirror turns until the server is built`,
      details: "Run `cargo build -p oppi-server`, set OPPI_SERVER_BIN, then verify with `oppi runtime-loop smoke --json`.",
    };
  }
  const fallback = mode === "default-with-fallback"
    ? "automatic Pi-turn mirroring enabled with stable Pi runtime fallback"
    : "opt-in /runtime-loop command mode";
  return {
    status: "pass",
    name: "Rust loop dogfood",
    message: `${fallback}; smoke with 'oppi runtime-loop smoke --json'`,
  };
}

function parseSandboxCommand(rest: string[]): Extract<OppiCommand, { type: "sandbox" }> {
  const json = rest.includes("--json");
  const yes = rest.includes("--yes") || rest.includes("-y");
  const persistEnv = !rest.includes("--no-persist-env");
  const dryRun = rest.includes("--dry-run") || rest.includes("--plan");
  const sub = rest.find((item) => !item.startsWith("-")) ?? "status";
  let account: string | undefined;
  for (let index = 0; index < rest.length; index += 1) {
    const item = rest[index];
    if (item === "--account") account = rest[index + 1];
    else if (item.startsWith("--account=")) account = item.slice("--account=".length);
  }
  if (sub === "status") return { type: "sandbox", subcommand: "status", json, yes, account, persistEnv, dryRun };
  if (sub === "setup" || sub === "setup-windows" || sub === "windows-setup") {
    return { type: "sandbox", subcommand: "setup-windows", json, yes, account, persistEnv, dryRun };
  }
  throw new Error(`Unknown oppi sandbox command: ${sub}`);
}

function collectWindowsSandboxDiagnostic(env: Env = process.env): Diagnostic | undefined {
  if (process.platform !== "win32") return undefined;
  const username = env.OPPI_WINDOWS_SANDBOX_USERNAME?.trim();
  const password = env.OPPI_WINDOWS_SANDBOX_PASSWORD?.trim();
  const wfpReady = env.OPPI_WINDOWS_SANDBOX_WFP_READY === "1";
  if (username && password && wfpReady) {
    return {
      status: "pass",
      name: "Windows sandbox account",
      message: `${username} configured with WFP readiness marker`,
    };
  }
  return {
    status: "warn",
    name: "Windows sandbox account",
    message: "disabled-network sandbox runs need a dedicated Windows account and WFP filters",
    details: "Run an elevated PowerShell once: `oppi sandbox setup-windows --yes`. OPPi will generate the password automatically and persist the required env vars for future terminals.",
  };
}

function collectNativeShellDiagnostic(env: Env = process.env, cwd = process.cwd()): Diagnostic {
  const serverBin = resolveOppiServerBin(env, cwd);
  const shellBin = resolveOppiShellBin(env, cwd);
  if (!serverBin || !shellBin) {
    return {
      status: "warn",
      name: "Native Rust shell",
      message: "oppi-shell native client is not ready yet",
      details: [
        serverBin ? undefined : "Build or configure oppi-server with `cargo build -p oppi-server` or OPPI_SERVER_BIN.",
        shellBin ? undefined : "Build or configure oppi-shell with `cargo build -p oppi-shell` or OPPI_SHELL_BIN.",
      ].filter(Boolean).join("\n"),
    };
  }
  const help = spawnSync(shellBin, ["--help"], {
    encoding: "utf8",
    timeout: 2_000,
    windowsHide: true,
  });
  if (help.error) {
    return {
      status: "warn",
      name: "Native Rust shell",
      message: `${shellBin} found but --help failed`,
      details: help.error.message,
    };
  }
  if (help.status !== 0) {
    return {
      status: "warn",
      name: "Native Rust shell",
      message: `${shellBin} found but --help exited ${help.status}`,
      details: [help.stderr, help.stdout].filter(Boolean).join("\n"),
    };
  }
  return {
    status: "pass",
    name: "Native Rust shell",
    message: `oppi-shell available at ${shellBin}; server ${serverBin}`,
  };
}

function collectTerminalDiagnostic(env: Env = process.env): Diagnostic {
  const termProgram = env.TERM_PROGRAM?.toLowerCase() ?? "";
  const term = env.TERM?.toLowerCase() ?? "";
  const colorTerm = env.COLORTERM?.toLowerCase() ?? "";
  const detected = env.WT_SESSION ? "Windows Terminal"
    : termProgram.includes("vscode") ? "VS Code/Cursor terminal"
      : termProgram.includes("wezterm") || env.WEZTERM_EXECUTABLE ? "WezTerm"
        : env.KITTY_WINDOW_ID ? "Kitty"
          : termProgram.includes("iterm") ? "iTerm2"
            : termProgram.includes("ghostty") ? "Ghostty"
              : term || termProgram || "unknown terminal";
  const supportsColor = !env.NO_COLOR && (colorTerm.includes("truecolor") || term.includes("256color") || detected !== "unknown terminal");
  const limited = term === "dumb" || env.NO_COLOR === "1";
  const degraded = limited || !supportsColor || detected === "unknown terminal";
  const colorDetails = degraded
    ? "Color: plain fallback is active (plain/no-color fallback); unset NO_COLOR and use a 256-color/truecolor terminal for themed output, or keep /theme plain intentionally."
    : `Color: ${colorTerm.includes("truecolor") ? "truecolor" : term.includes("256color") ? "256-color" : "ANSI"} output is available; set NO_COLOR=1 or /theme plain for the plain fallback.`;
  const keyDetails = degraded
    ? "Keybindings: basic input and slash commands still work, but advanced key chords may degrade to line-mode commands; use /keys for the local map, /again for follow-ups, /steer for steering, and /interrupt for interruption."
    : "Keybindings: slash palette, arrows, PgUp/PgDn, Home/End, Tab, Enter, Esc, Ctrl+C, Ctrl+D, Shift+Enter, Alt+Enter, Ctrl+Enter, and Alt+Up are expected when the terminal emits standard escape sequences; use /keys if a chord degrades.";
  return {
    status: degraded ? "warn" : "pass",
    name: "Terminal",
    message: `${detected}; ${supportsColor && !limited ? "color capable" : "limited color"}`,
    details: [
      degraded
        ? "Native shell rendering will fall back to plain output. For best results use Windows Terminal, VS Code/Cursor, Ghostty, WezTerm, Kitty, or iTerm2 with color enabled."
        : "Native shell rendering supports concise status, themed markdown, and tool/background digests.",
      keyDetails,
      colorDetails,
    ].join("\n"),
  };
}

function collectRuntimeWorkerDiagnostic(env: Env = process.env, cwd = process.cwd()): Diagnostic {
  const serverBin = resolveOppiServerBin(env, cwd);
  const apiKeyEnv = directWorkerApiKeyEnvName(env);
  const providerReady = hasDirectWorkerProviderAuth(env, apiKeyEnv);
  if (!serverBin) {
    return {
      status: "warn",
      name: "Rust direct worker",
      message: "oppi-server not found; direct provider worker commands remain unavailable until the server is built",
      details: "Run `cargo build -p oppi-server`, set OPPI_SERVER_BIN, then verify with `oppi runtime-worker smoke --json`.",
    };
  }
  if (apiKeyEnv && !isDirectWorkerApiKeyEnvAllowed(apiKeyEnv)) {
    return {
      status: "warn",
      name: "Rust direct worker",
      message: "configured direct-provider API key env name is not allowed by the Rust safety boundary",
      details: "Use OPPI_OPENAI_API_KEY, OPENAI_API_KEY, or an OPPI_*_API_KEY variable; raw keys are never sent over JSON-RPC.",
    };
  }
  if (!providerReady) {
    return {
      status: "warn",
      name: "Rust direct worker",
      message: "oppi-server is available, but direct-provider auth is not configured; `oppi runtime-worker <prompt>` will show stable Pi fallback guidance",
      details: apiKeyEnv
        ? `Set ${apiKeyEnv} or choose another OPPI_RUNTIME_WORKER_API_KEY_ENV. Local regression path: \`oppi runtime-worker smoke --json\`.`
        : "Set OPPI_OPENAI_API_KEY / OPENAI_API_KEY, or OPPI_RUNTIME_WORKER_API_KEY_ENV naming an OPPI_*_API_KEY variable. Local regression path: `oppi runtime-worker smoke --json`.",
    };
  }
  return {
    status: "pass",
    name: "Rust direct worker",
    message: `opt-in provider worker ready via oppi-server at ${serverBin}; smoke with 'oppi runtime-worker smoke --json'`,
  };
}

export function collectDoctorDiagnostics(options: { agentDir?: string; env?: Env; cwd?: string } = {}): Diagnostic[] {
  const env = options.env ?? process.env;
  const cwd = options.cwd ?? process.cwd();
  const diagnostics: Diagnostic[] = [];
  const agentDir = resolveDoctorAgentDir(options.agentDir, env, cwd);
  const piCli = resolvePiCliPath();
  const piPackage = resolvePiPackagePath();
  const writable = checkWritableDir(agentDir);
  const hoppiModule = resolveHoppiModulePath(env, cwd);
  const nativesPackage = packageDirFromResolvedMain("@oppiai/natives");
  const feedbackConfig = join(homedir(), ".oppi", "feedback.json");

  diagnostics.push(nodeVersionAtLeast(20, 6)
    ? { status: "pass", name: "Node.js", message: `Node ${process.versions.node}` }
    : { status: "fail", name: "Node.js", message: `Node ${process.versions.node}; Pi requires >=20.6.0` });

  diagnostics.push(piCli
    ? { status: "pass", name: "Pi CLI", message: piCli }
    : { status: "fail", name: "Pi CLI", message: "Could not resolve @mariozechner/pi-coding-agent/dist/cli.js" });

  diagnostics.push(piPackage
    ? { status: "pass", name: "OPPi Pi package", message: piPackage }
    : { status: "fail", name: "OPPi Pi package", message: "Could not find packages/pi-package or @oppiai/pi-package" });

  const plugins = collectPluginDiagnostics();
  diagnostics.push(plugins.warnings.length === 0
    ? { status: "pass", name: "Plugins", message: `${plugins.enabled}/${plugins.configured} enabled; ${plugins.sources.length} launch source(s)` }
    : { status: "warn", name: "Plugins", message: `${plugins.enabled}/${plugins.configured} enabled with warnings`, details: plugins.warnings.join("\n") });

  diagnostics.push(writable.ok
    ? { status: "pass", name: "OPPi agent dir", message: `${agentDir} is writable` }
    : { status: "fail", name: "OPPi agent dir", message: `${agentDir} is not writable`, details: writable.error });

  diagnostics.push(existsSync(join(agentDir, "settings.json"))
    ? { status: "pass", name: "Settings", message: "settings.json exists" }
    : { status: "warn", name: "Settings", message: "settings.json not found yet; it will be created when settings are changed" });

  diagnostics.push(existsSync(join(agentDir, "auth.json"))
    ? { status: "pass", name: "Auth", message: "auth.json exists (contents not inspected)" }
    : { status: "warn", name: "Auth", message: "auth.json not found; configure provider auth if needed" });

  diagnostics.push(hoppiModule
    ? { status: "pass", name: "Hoppi", message: hoppiModule }
    : { status: "warn", name: "Hoppi", message: `Hoppi module not found; memory is optional. Run \`oppi mem install\`, accept the first-start prompt, install from /settings:oppi → Memory, or set OPPI_HOPPI_MODULE.` });

  diagnostics.push(nativesPackage
    ? { status: "pass", name: "Natives", message: `${nativesPackage} available; run \`oppi natives status\` for Rust/N-API probe details` }
    : { status: "warn", name: "Natives", message: "Optional @oppiai/natives package not found; native helpers are disabled and JS fallbacks remain in use" });

  diagnostics.push(collectRustRuntimeDiagnostic(env, cwd));
  diagnostics.push(collectRustProtocolSandboxDiagnostic(env, cwd));
  const windowsSandbox = collectWindowsSandboxDiagnostic(env);
  if (windowsSandbox) diagnostics.push(windowsSandbox);
  diagnostics.push(collectRuntimeLoopDiagnostic(env, cwd, agentDir));
  diagnostics.push(collectRuntimeWorkerDiagnostic(env, cwd));
  diagnostics.push(collectNativeShellDiagnostic(env, cwd));

  diagnostics.push(existsSync(feedbackConfig)
    ? { status: "pass", name: "Feedback", message: `${feedbackConfig} exists (secrets not printed)` }
    : { status: "warn", name: "Feedback", message: "No ~/.oppi/feedback.json; feedback commands will use defaults or local drafts" });

  diagnostics.push(collectTerminalDiagnostic(env));

  return diagnostics;
}

function printDiagnostics(diagnostics: Diagnostic[], json: boolean): number {
  const hardFailures = diagnostics.filter((item) => item.status === "fail");
  if (json) {
    console.log(JSON.stringify({ ok: hardFailures.length === 0, diagnostics }, null, 2));
  } else {
    console.log("OPPi doctor");
    for (const diagnostic of diagnostics) {
      const icon = diagnostic.status === "pass" ? "✓" : diagnostic.status === "warn" ? "!" : "✗";
      console.log(`${icon} ${diagnostic.name}: ${diagnostic.message}`);
      if (diagnostic.details) console.log(`  ${diagnostic.details}`);
    }
    if (hardFailures.length === 0) console.log("\nOPPi looks usable. Tiny spaceship cleared for launch.");
    else console.log("\nOPPi has hard failures. Fix the ✗ items above, then rerun `oppi doctor`.");
  }
  return hardFailures.length === 0 ? 0 : 1;
}

function hoppiSetupInstructions(): string[] {
  return [
    `Hoppi memory uses the optional ${HOPPI_PACKAGE_NAME} npm package.`,
    "OPPi never installs it silently; accept the first-start prompt, install it from `/settings:oppi` → Memory, or run:",
    "",
    "  oppi mem install",
    "",
    "For local development without npm publishing:",
    "  cd ..\\hoppi-memory",
    "  npm run build:all",
    "  setx OPPI_HOPPI_MODULE \"%CD%\\dist\\index.js\"",
    "",
    "After installation, run `oppi mem setup` again to initialize the store.",
  ];
}

function ensureManagedPackageRoot(env: Env = process.env, cwd = process.cwd()): string {
  const root = managedPackagesDir(env, cwd);
  mkdirSync(root, { recursive: true });
  const packageJson = join(root, "package.json");
  if (!existsSync(packageJson)) {
    writeFileSync(packageJson, `${JSON.stringify({ private: true, name: "oppi-managed-packages", description: "OPPi managed optional packages." }, null, 2)}\n`, "utf8");
  }
  return root;
}

function windowsCmdShim(command: string, args: string[]): { command: string; args: string[] } {
  if (process.platform === "win32" && /\.(?:cmd|bat)$/i.test(command)) {
    return { command: "cmd.exe", args: ["/d", "/s", "/c", command, ...args] };
  }
  return { command, args };
}

function npmSpawnCommand(args: string[]): { command: string; args: string[] } {
  // Directly spawning npm.cmd can throw EINVAL under some Windows terminals.
  // Route through cmd.exe explicitly instead of relying on shell lookup.
  if (process.platform === "win32") return { command: "cmd.exe", args: ["/d", "/s", "/c", "npm", ...args] };
  return { command: "npm", args };
}

async function runUpdateCommand(command: Extract<OppiCommand, { type: "update" }>): Promise<number> {
  const currentVersion = cliVersion();
  const latestVersion = await fetchLatestCliVersion(process.env, 10_000);
  const updateAvailable = latestVersion ? compareVersions(latestVersion, currentVersion) > 0 : null;
  const payload = {
    currentVersion,
    latestVersion,
    updateAvailable,
    updateCommand: "oppi update",
    changelogUrl: OPPI_CHANGELOG_URL,
  };

  if (command.json) {
    console.log(JSON.stringify(payload, null, 2));
    if (command.check) return 0;
  }

  if (command.check) {
    if (!latestVersion) console.log(`Could not check the npm registry for OPPi updates.\nChangelog: ${OPPI_CHANGELOG_URL}`);
    else if (updateAvailable) console.log(formatUpdateNotice({ currentVersion, latestVersion, updateCommand: "oppi update", changelogUrl: OPPI_CHANGELOG_URL }));
    else console.log(`OPPi ${currentVersion} is current.\nChangelog: ${OPPI_CHANGELOG_URL}`);
    return 0;
  }

  if (latestVersion && updateAvailable === false) {
    console.log(`OPPi ${currentVersion} is already current.\nChangelog: ${OPPI_CHANGELOG_URL}`);
    return 0;
  }

  console.log(latestVersion
    ? `Updating OPPi from ${currentVersion} to ${latestVersion}...`
    : `Updating OPPi ${currentVersion} to the latest npm release...`);
  console.log(`Changelog: ${OPPI_CHANGELOG_URL}`);

  const npm = npmSpawnCommand(["install", "-g", `${OPPI_CLI_PACKAGE_NAME}@latest`]);
  return new Promise((resolveUpdate) => {
    const child = spawn(npm.command, npm.args, { stdio: "inherit", env: process.env });
    child.on("error", (error: Error) => {
      console.error(`OPPi update failed to start npm: ${error.message}`);
      resolveUpdate(1);
    });
    child.on("close", (code: number | null) => {
      if (code === 0) console.log("OPPi update completed. Restart oppi to use the new version.");
      resolveUpdate(code ?? 1);
    });
  });
}

type HoppiInstallResult =
  | { ok: true; modulePath: string; output: string }
  | { ok: false; error: string; output: string };

function installHoppiPackage(env: Env = process.env, cwd = process.cwd()): Promise<HoppiInstallResult> {
  return new Promise<HoppiInstallResult>((resolveInstall) => {
    const root = ensureManagedPackageRoot(env, cwd);
    const args = ["install", HOPPI_PACKAGE_SPEC, "--save-exact", "--no-audit", "--no-fund"];
    let output = "";
    const append = (chunk: unknown) => {
      output += String(chunk);
      if (output.length > 12_000) output = output.slice(-12_000);
    };
    const npm = npmSpawnCommand(args);
    const child = spawn(npm.command, npm.args, {
      cwd: root,
      stdio: ["ignore", "pipe", "pipe"],
      env: { ...process.env, ...env, npm_config_loglevel: env.npm_config_loglevel ?? "warn" },
    });
    child.stdout?.on("data", append);
    child.stderr?.on("data", append);
    child.on("error", (error: Error) => resolveInstall({ ok: false, error: error.message, output }));
    child.on("close", (code: number | null) => {
      const modulePath = managedHoppiModulePath(env, cwd, HOPPI_PACKAGE_NAME);
      if (code === 0 && existsSync(modulePath)) resolveInstall({ ok: true, modulePath, output });
      else resolveInstall({ ok: false, error: `npm install exited with code ${code ?? "unknown"}`, output });
    });
  });
}

function isHoppiMissingMessage(message: string): boolean {
  return message.includes("Hoppi module not found")
    || message.includes(`Cannot find module '${HOPPI_PACKAGE_NAME}'`)
    || message.includes(`Cannot find package "${HOPPI_PACKAGE_NAME}"`)
    || message.includes("Cannot find module 'hoppi-memory'")
    || message.includes('Cannot find package "hoppi-memory"');
}

async function importHoppiModule(env: Env = process.env, cwd = process.cwd()): Promise<any> {
  const modulePath = resolveHoppiModulePath(env, cwd);
  if (!modulePath) throw new Error(`Hoppi module not found. Run \`oppi mem install\`, install ${HOPPI_PACKAGE_NAME} from /settings:oppi → Memory, or set OPPI_HOPPI_MODULE.`);
  return import(pathToFileURL(modulePath).href);
}

async function importNativesModule(env: Env = process.env): Promise<any> {
  if (env.OPPI_DISABLE_NATIVES?.trim() === "1") {
    throw new Error("Optional @oppiai/natives package disabled by OPPI_DISABLE_NATIVES.");
  }
  try {
    return await import("@oppiai/natives");
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    throw new Error(`Optional @oppiai/natives package not available: ${message}`);
  }
}

function unavailableNativeStatus(error: string) {
  return {
    packageName: "@oppiai/natives",
    packageVersion: "unavailable",
    platform: process.platform,
    arch: process.arch,
    native: {
      available: false,
      candidatePaths: [],
      error,
    },
    fallbacks: {
      search: "js-benchmark-only",
      pty: "deferred",
      clipboard: "deferred",
      sandbox: "deferred",
    },
    recommendations: [
      "Optional @oppiai/natives package is not installed for this platform; JS fallbacks remain in use.",
      "Run `oppi natives benchmark --json` only after installing a platform-compatible native package.",
    ],
  };
}

export function resolveOppiServerBin(env: Env = process.env, cwd = process.cwd()): string | undefined {
  if (env.OPPI_SERVER_BIN?.trim()) return resolve(expandHome(env.OPPI_SERVER_BIN.trim()));
  return resolveCargoBin("oppi-server", env, cwd);
}

export function resolveOppiShellBin(env: Env = process.env, cwd = process.cwd()): string | undefined {
  if (env.OPPI_SHELL_BIN?.trim()) return resolve(expandHome(env.OPPI_SHELL_BIN.trim()));
  return resolveCargoBin("oppi-shell", env, cwd);
}

function resolveCargoBin(name: string, _env: Env = process.env, cwd = process.cwd()): string | undefined {
  const exe = process.platform === "win32" ? `${name}.exe` : name;
  const candidates = [
    resolve(cwd, "target", "debug", exe),
    resolve(cwd, "target", "release", exe),
    resolve(__dirname, "..", "..", "..", "target", "debug", exe),
    resolve(__dirname, "..", "..", "..", "target", "release", exe),
  ];
  return candidates.find((candidate) => existsSync(candidate));
}

type RuntimeLoopSmokeScenario = {
  name: string;
  ok: boolean;
  status?: string;
  eventCount?: number;
  expectedErrorCode?: string;
  diagnostics?: string[];
};

type RuntimeLoopSmokeResult = {
  ok: boolean;
  mode: RuntimeLoopMode;
  serverBin?: string;
  threadId?: string;
  turnId?: string;
  turnStatus?: string;
  eventCount?: number;
  bridgedEventCount?: number;
  toolResultStatuses?: string[];
  debugBundleRedacted?: boolean;
  bridgeClean?: boolean;
  bridgeCleanReason?: string;
  scenarios?: RuntimeLoopSmokeScenario[];
  durationMs: number;
  diagnostics: string[];
};

type RuntimeWorkerSmokeResult = {
  ok: boolean;
  serverBin?: string;
  threadId?: string;
  turnId?: string;
  turnStatus?: string;
  eventCount?: number;
  providerRequestCount?: number;
  providerAuthorized?: boolean;
  providerStreamed?: boolean;
  assistantDeltaCount?: number;
  toolResultStatuses?: string[];
  assistantText?: string;
  debugBundleRedacted?: boolean;
  workerClean?: boolean;
  workerCleanReason?: string;
  durationMs: number;
  diagnostics: string[];
};

type RuntimeWorkerRunResult = {
  ok: boolean;
  serverBin?: string;
  threadId?: string;
  turnId?: string;
  turnStatus?: string;
  eventCount?: number;
  providerConfigured?: boolean;
  provider?: RuntimeWorkerProvider;
  providerRequestCount?: number;
  providerStreamed?: boolean;
  assistantDeltaCount?: number;
  approvalsAutoApproved?: number;
  awaitingApproval?: boolean;
  toolResultStatuses?: string[];
  assistantText?: string;
  todos?: any[];
  todoSummary?: string;
  debugBundleRedacted?: boolean;
  promptVariant?: PromptVariant;
  promptVariantApplied?: boolean;
  promptVariantProviderPromptIncluded?: boolean;
  featureGuidanceApplied?: boolean;
  featureGuidanceProviderPromptIncluded?: boolean;
  permissionMode?: RuntimeWorkerPermissionMode;
  effort?: RuntimeWorkerEffort;
  providerReasoningEffort?: string;
  followUpApplied?: boolean;
  followUpProviderPromptIncluded?: boolean;
  memoryEnabled?: boolean;
  memoryAvailable?: boolean;
  memoryLoaded?: boolean;
  memoryContextBytes?: number;
  memoryCount?: number;
  memorySaved?: boolean;
  memoryProviderPromptIncluded?: boolean;
  fallbackAvailable?: boolean;
  fallbackCommand?: string;
  durationMs: number;
  diagnostics: string[];
};

type JsonRpcResponse<T = unknown> = {
  result?: T;
  error?: { code: number; message: string; data?: unknown };
};

class RuntimeLoopSmokeRpcError extends Error {
  rpcCode: number;
  data?: unknown;

  constructor(error: { code: number; message: string; data?: unknown }) {
    super(redactText(error.message || JSON.stringify(error)));
    this.name = "RuntimeLoopSmokeRpcError";
    this.rpcCode = error.code;
    this.data = error.data;
  }
}

function runtimeErrorDataCode(error: unknown): string | undefined {
  const data = error instanceof RuntimeLoopSmokeRpcError ? error.data : undefined;
  return typeof (data as any)?.code === "string" ? (data as any).code : undefined;
}

function eventKindType(event: unknown): string | undefined {
  const kind = (event as any)?.kind;
  if (typeof kind?.type === "string") return kind.type;
  if (kind && typeof kind === "object") return Object.keys(kind)[0];
  return undefined;
}

function hasEventType(events: unknown[] | undefined, type: string): boolean {
  return Array.isArray(events) && events.some((event) => eventKindType(event) === type);
}

function toolStatuses(events: unknown[] | undefined): string[] {
  if (!Array.isArray(events)) return [];
  return events
    .filter((event) => eventKindType(event) === "toolCallCompleted")
    .map((event) => String((event as any)?.kind?.result?.status ?? (event as any)?.kind?.toolCallCompleted?.result?.status ?? "unknown"));
}

function assistantDeltaEvents(events: unknown[] | undefined): unknown[] {
  return Array.isArray(events) ? events.filter((event) => eventKindType(event) === "itemDelta") : [];
}

function assistantTextFromEvents(events: unknown[] | undefined): string {
  return assistantDeltaEvents(events)
    .map((event) => String((event as any)?.kind?.delta ?? (event as any)?.kind?.itemDelta?.delta ?? ""))
    .join("");
}

type RuntimeWorkerMemorySettings = {
  enabled: boolean;
  startupRecall: boolean;
  taskStartRecall: boolean;
  turnSummaries: boolean;
};

type RuntimeWorkerMemoryBridge = {
  enabled: boolean;
  available: boolean;
  loaded: boolean;
  saved: boolean;
  contextMarkdown?: string;
  contextBytes?: number;
  memoryCount?: number;
  backend?: any;
  project?: { cwd: string; displayName?: string };
  settings?: RuntimeWorkerMemorySettings;
  diagnostics: string[];
};

function normalizeRuntimeWorkerMemorySettings(value: any): RuntimeWorkerMemorySettings {
  return {
    enabled: value?.enabled !== false,
    startupRecall: value?.startupRecall !== false,
    taskStartRecall: value?.taskStartRecall !== false,
    turnSummaries: value?.turnSummaries !== false,
  };
}

function readRuntimeWorkerMemorySettings(hoppi: any, settingsPath: string, diagnostics: string[]): RuntimeWorkerMemorySettings {
  if (typeof hoppi.readHoppiMemorySettings === "function") {
    try {
      return normalizeRuntimeWorkerMemorySettings(hoppi.readHoppiMemorySettings({ settingsPath }));
    } catch (error) {
      diagnostics.push(`Hoppi memory settings read failed; using safe defaults. ${redactText(error instanceof Error ? error.message : String(error))}`);
    }
  }
  return normalizeRuntimeWorkerMemorySettings(readPackageJson(settingsPath)?.oppi?.memory);
}

function coerceMemoryCount(value: unknown): number {
  const numeric = Number(value);
  return Number.isFinite(numeric) && numeric > 0 ? Math.floor(numeric) : 0;
}

function truncateUtf8Text(value: string, maxBytes: number, suffix = "\n\n[Hoppi memory context truncated]"): { text: string; truncated: boolean } {
  const text = value.trim();
  if (Buffer.byteLength(text, "utf8") <= maxBytes) return { text, truncated: false };
  const suffixBytes = Buffer.byteLength(suffix, "utf8");
  const limit = Math.max(0, maxBytes - suffixBytes);
  const truncated = Buffer.from(text, "utf8").subarray(0, limit).toString("utf8").replace(/\uFFFD+$/g, "").trimEnd();
  return { text: `${truncated}${suffix}`, truncated: true };
}

function buildRuntimeWorkerMemoryContext(parts: readonly string[]): { contextMarkdown?: string; contextBytes: number; truncated: boolean } {
  const unique = [...new Set(parts.map((part) => part.trim()).filter(Boolean))];
  if (!unique.length) return { contextBytes: 0, truncated: false };
  const advisory = "\n\nMemory is advisory: prefer current user instructions and current files when they conflict.";
  const bodyLimit = Math.max(1, RUNTIME_WORKER_MEMORY_CONTEXT_MAX_BYTES - Buffer.byteLength(advisory, "utf8"));
  const body = truncateUtf8Text(unique.join("\n\n---\n\n"), bodyLimit);
  const contextMarkdown = `${body.text}${advisory}`;
  return { contextMarkdown, contextBytes: Buffer.byteLength(contextMarkdown, "utf8"), truncated: body.truncated };
}

function providerPromptContainsText(requests: readonly any[] | undefined, text: string | undefined): boolean | undefined {
  if (!text?.trim() || !requests) return undefined;
  const probe = text.split("\n").map((line) => line.trim()).find((line) => line.length >= 24) ?? text.trim().slice(0, 80);
  if (!probe) return undefined;
  return requests.some((request) => {
    try {
      const body = JSON.parse(String(request?.body ?? "{}"));
      const messages = Array.isArray(body.messages) ? body.messages : [];
      return messages.some((message: any) => message?.role === "system" && typeof message.content === "string" && message.content.includes(probe));
    } catch {
      return false;
    }
  });
}

function providerPromptContainsMemory(requests: readonly any[] | undefined, contextMarkdown: string | undefined): boolean | undefined {
  return providerPromptContainsText(requests, contextMarkdown);
}

function runtimeWorkerMemoryMode(command: Extract<OppiCommand, { type: "runtime-worker"; subcommand: "run" }>, env: Env): RuntimeWorkerMemoryMode {
  if (command.memory !== "auto") return command.memory;
  return coerceRuntimeWorkerMemoryMode(env.OPPI_RUNTIME_WORKER_MEMORY);
}

function runtimeWorkerProjectRef(cwd: string): { cwd: string; displayName?: string } {
  return { cwd, displayName: basename(cwd) || undefined };
}

function appendMemoryToSystemPrompt(systemPrompt: unknown, context: string | undefined): string | undefined {
  const base = typeof systemPrompt === "string" ? systemPrompt.trim() : "";
  if (!context?.trim()) return base || undefined;
  return [base, context.trim()].filter(Boolean).join("\n\n");
}

function buildRuntimeWorkerMemorySummary(prompt: string, assistantText: string, toolResultStatuses: readonly string[], threadId: string | undefined, turnId: string | undefined): string | undefined {
  const user = prompt.replace(/\s+/g, " ").trim();
  const assistant = assistantText.replace(/\s+/g, " ").trim();
  if (!user || !assistant || `${user}${assistant}`.length < 80) return undefined;
  return [
    `Runtime-worker turn summary (${new Date().toISOString()})`,
    `User asked: ${user.slice(0, 500)}`,
    `Assistant outcome: ${assistant.slice(0, 700)}`,
    toolResultStatuses.length ? `Tool results: ${toolResultStatuses.join(", ")}` : undefined,
    threadId && turnId ? `Runtime turn: ${threadId}/${turnId}` : undefined,
  ].filter(Boolean).join("\n");
}

async function prepareRuntimeWorkerMemoryBridge(command: Extract<OppiCommand, { type: "runtime-worker"; subcommand: "run" }>, env: Env = process.env, cwd = process.cwd()): Promise<RuntimeWorkerMemoryBridge> {
  const diagnostics: string[] = [];
  const mode = runtimeWorkerMemoryMode(command, env);
  if (mode === "off" || (mode === "auto" && command.mock)) {
    return { enabled: false, available: false, loaded: false, saved: false, diagnostics };
  }

  try {
    const hoppi = await importHoppiModule(env, cwd);
    const settingsPath = runtimeWorkerMemorySettingsPath(env, cwd);
    const settings = readRuntimeWorkerMemorySettings(hoppi, settingsPath, diagnostics);
    const enabled = mode === "on" || settings.enabled;
    if (!enabled) {
      diagnostics.push("Hoppi memory is disabled by settings for the direct worker; continuing without memory.");
      return { enabled: false, available: true, loaded: false, saved: false, settings, diagnostics };
    }

    if (typeof hoppi.createHoppiBackend !== "function") throw new Error("Installed Hoppi module does not expose createHoppiBackend().");
    const root = typeof hoppi.getDefaultHoppiRoot === "function" ? hoppi.getDefaultHoppiRoot() : join(homedir(), ".oppi", "hoppi");
    const backend = hoppi.createHoppiBackend({ root });
    if (!backend || typeof backend !== "object") throw new Error("Installed Hoppi module returned an invalid backend.");
    if (typeof backend.init === "function") await backend.init();
    const project = runtimeWorkerProjectRef(cwd);
    const parts: string[] = [];
    let memoryCount = 0;

    if (typeof backend.status === "function") {
      try {
        const status = await backend.status(project);
        memoryCount = Math.max(memoryCount, coerceMemoryCount(status?.memoryCount));
      } catch (error) {
        diagnostics.push(`Hoppi memory status unavailable; continuing recall. ${redactText(error instanceof Error ? error.message : String(error))}`);
      }
    }

    if (settings.startupRecall && typeof backend.buildStartupContext === "function") {
      try {
        const startup = await backend.buildStartupContext({ project, maxMemories: 8, includePinned: true });
        if (startup?.contextMarkdown?.trim()) parts.push(startup.contextMarkdown.trim());
        memoryCount = Math.max(memoryCount, coerceMemoryCount(startup?.memoryCount));
      } catch (error) {
        diagnostics.push(`Hoppi startup recall failed; continuing with task recall. ${redactText(error instanceof Error ? error.message : String(error))}`);
      }
    }

    if (settings.taskStartRecall && command.prompt.trim() && typeof backend.recall === "function") {
      try {
        const query = command.prompt.trim().slice(0, RUNTIME_WORKER_MEMORY_QUERY_MAX_CHARS);
        const recall = await backend.recall({ project, query, budget: 900, limit: 5 });
        if (recall?.contextMarkdown?.trim()) parts.push(recall.contextMarkdown.trim());
        if (Array.isArray(recall?.memories)) memoryCount = Math.max(memoryCount, recall.memories.length);
      } catch (error) {
        diagnostics.push(`Hoppi task recall failed; continuing without recalled task context. ${redactText(error instanceof Error ? error.message : String(error))}`);
      }
    }

    const { contextMarkdown, contextBytes, truncated } = buildRuntimeWorkerMemoryContext(parts);
    if (truncated) diagnostics.push(`Hoppi memory context was truncated to ${contextBytes} bytes for provider prompt safety.`);
    diagnostics.push(contextMarkdown
      ? `Hoppi memory loaded ${memoryCount} project memor${memoryCount === 1 ? "y" : "ies"} into the direct-worker context (${contextBytes} bytes).`
      : `Hoppi memory is enabled for the direct worker; no relevant project context was recalled.`);
    return {
      enabled: true,
      available: true,
      loaded: Boolean(contextMarkdown),
      saved: false,
      contextMarkdown,
      contextBytes,
      memoryCount,
      backend,
      project,
      settings,
      diagnostics,
    };
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    diagnostics.push(isHoppiMissingMessage(message)
      ? "Hoppi memory is enabled but the optional Hoppi package is unavailable; continuing without memory."
      : `Hoppi memory unavailable for direct worker: ${redactText(message)}`);
    return { enabled: true, available: false, loaded: false, saved: false, diagnostics };
  }
}

async function rememberRuntimeWorkerTurn(memory: RuntimeWorkerMemoryBridge, prompt: string, assistantText: string, toolResultStatuses: readonly string[], threadId: string | undefined, turnId: string | undefined): Promise<boolean> {
  if (!memory.enabled || !memory.available || !memory.settings?.turnSummaries || !memory.backend?.remember || !memory.project) return false;
  const summary = buildRuntimeWorkerMemorySummary(prompt, assistantText, toolResultStatuses, threadId, turnId);
  if (!summary) return false;
  try {
    await memory.backend.remember({
      project: memory.project,
      content: summary,
      tags: ["oppi-runtime-worker", "oppi-turn-summary", "agent_end"],
      layer: "buffer",
      confidence: "observed",
      source: "oppi:runtime-worker",
      sourceSessionId: threadId && turnId ? `${threadId}:${turnId}` : undefined,
    });
    memory.saved = true;
    memory.diagnostics.push("Hoppi memory saved a bounded direct-worker turn summary.");
    return true;
  } catch (error) {
    memory.diagnostics.push(`Hoppi memory summary save failed: ${redactText(error instanceof Error ? error.message : String(error))}`);
    return false;
  }
}

function longCompactionSummary(): string {
  const chunks = Array.from({ length: 32 }, (_, index) => `compaction dry-run chunk ${index + 1}: preserve remaining todo context, archived outcomes, file state, validation notes, and final-response ledger.`);
  return chunks.join("\n");
}

async function runRuntimeLoopSmoke(env: Env = process.env, cwd = process.cwd()): Promise<RuntimeLoopSmokeResult> {
  const started = Date.now();
  const mode = resolveRuntimeLoopMode(env, cwd);
  const diagnostics: string[] = [];
  const serverBin = resolveOppiServerBin(env, cwd);
  if (!serverBin) {
    return {
      ok: false,
      mode,
      durationMs: Date.now() - started,
      diagnostics: ["oppi-server not found; build with `cargo build -p oppi-server` or set OPPI_SERVER_BIN"],
    };
  }

  const serverCommand = process.platform === "win32" && /\.(?:cmd|bat)$/i.test(serverBin)
    ? { command: "cmd.exe", args: ["/d", "/s", "/c", serverBin, "--stdio"] }
    : { command: serverBin, args: ["--stdio"] };
  const child = spawn(serverCommand.command, serverCommand.args, {
    cwd,
    env: { ...process.env, ...env, OPPI_EXPERIMENTAL_RUNTIME: "1" },
    stdio: ["pipe", "pipe", "pipe"],
    windowsHide: true,
  });
  let stderr = "";
  let nextId = 0;
  child.stderr?.on("data", (chunk: Buffer) => { stderr += chunk.toString("utf8"); });
  const lines = createInterface({ input: child.stdout });
  const iterator = lines[Symbol.asyncIterator]();
  const closePromise = new Promise<number | null>((resolveClose, rejectClose) => {
    child.on("error", rejectClose);
    child.on("close", resolveClose);
  });
  const withAuth = (params: Record<string, unknown> = {}) => {
    const token = env.OPPI_SERVER_AUTH_TOKEN?.trim();
    return token ? { ...params, authToken: token } : params;
  };
  const request = async <T>(method: string, params: Record<string, unknown> = {}): Promise<T> => {
    const id = `runtime-loop-smoke-${++nextId}`;
    child.stdin.write(`${JSON.stringify({ jsonrpc: "2.0", id, method, params: withAuth(params) })}\n`);
    const line = await iterator.next();
    if (line.done || !line.value) throw new Error(`oppi-server returned no JSON-RPC response.${stderr.trim() ? ` stderr: ${redactText(stderr.trim())}` : ""}`);
    const response = JSON.parse(line.value) as JsonRpcResponse<T>;
    if (response.error) throw new RuntimeLoopSmokeRpcError(response.error);
    if (response.result === undefined) throw new Error(`${method} returned no result`);
    return response.result;
  };

  try {
    const scenarios: RuntimeLoopSmokeScenario[] = [];
    const init = await request<any>("initialize", {
      clientName: "oppi-runtime-loop-smoke",
      clientVersion: cliVersion(),
      protocolVersion: "0.1.0",
      clientCapabilities: ["threads", "turns", "events", "approvals", "memory"],
    });
    if (init.protocolCompatible === false) diagnostics.push(`protocol compatibility warning: server ${init.protocolVersion}`);
    const start = await request<any>("thread/start", {
      project: { id: "runtime-loop-smoke", cwd, displayName: "Runtime loop smoke" },
      title: "Runtime loop smoke",
    });
    const threadId = String(start.thread.id);
    const turnEvents = (events: unknown[] | undefined, turnId: string) => Array.isArray(events) ? events.filter((event: any) => event?.turnId === turnId) : [];
    const pollEvents = async (predicate: (events: unknown[]) => boolean, attempts = 60): Promise<unknown[]> => {
      let events: unknown[] = [];
      for (let attempt = 0; attempt < attempts; attempt += 1) {
        const listed = await request<any>("events/list", { threadId, after: 0, limit: 5000 });
        events = Array.isArray(listed.events) ? listed.events : [];
        if (predicate(events)) return events;
        await new Promise((resolvePoll) => setTimeout(resolvePoll, 10));
      }
      return events;
    };

    await request("pi/bridge-event", { threadId, name: "smoke/message_update", payload: { text: "bridge event smoke" } });
    const run = await request<any>("turn/run-agentic", {
      threadId,
      input: "runtime loop smoke",
      toolDefinitions: [
        { name: "read", namespace: "pi", description: "Smoke read tool", concurrencySafe: true, capabilities: ["filesystem"] },
        { name: "shell_exec", namespace: "pi", description: "Smoke shell tool", concurrencySafe: false, capabilities: ["process"] },
      ],
      modelSteps: [{
        assistantDeltas: ["Running adapter smoke."],
        toolCalls: [
          { id: "pi-read-smoke", name: "read", namespace: "pi", arguments: { path: "README.md" } },
          { id: "pi-shell-smoke", name: "shell_exec", namespace: "pi", arguments: { command: "echo smoke" } },
        ],
        toolResults: [
          { callId: "pi-read-smoke", status: "ok", output: "read result smoke" },
          { callId: "pi-shell-smoke", status: "error", error: "simulated tool failure smoke" },
        ],
        finalResponse: true,
      }],
      maxContinuations: 4,
    });
    const bridgeToolStatuses = toolStatuses(run?.events);
    scenarios.push({
      name: "bridge-smoke",
      ok: run?.turn?.status === "completed" && bridgeToolStatuses.includes("ok") && bridgeToolStatuses.includes("error"),
      status: run?.turn?.status,
      eventCount: Array.isArray(run?.events) ? run.events.length : undefined,
      diagnostics: [`tool statuses: ${bridgeToolStatuses.join(", ") || "none"}`],
    });

    const approvalCall = { id: "approval-dry-run", name: "write", namespace: "pi", arguments: { path: "dry-run.txt" } };
    const approvalTool = { name: "write", namespace: "pi", description: "Dry-run write approval", concurrencySafe: false, requiresApproval: true, capabilities: ["filesystem"] };
    const paused = await request<any>("turn/run-agentic", {
      threadId,
      input: "approval dry-run",
      toolDefinitions: [approvalTool],
      modelSteps: [{
        assistantDeltas: ["Need approval before writing."],
        toolCalls: [approvalCall],
        finalResponse: false,
      }],
      maxContinuations: 4,
    });
    const resumed = await request<any>("turn/resume-agentic", {
      threadId,
      turnId: paused?.turn?.id,
      toolDefinitions: [approvalTool],
      approvedToolCallIds: [approvalCall.id],
      modelSteps: [
        {
          assistantDeltas: [],
          toolCalls: [approvalCall],
          toolResults: [{ callId: approvalCall.id, status: "ok", output: "approved dry-run result" }],
          finalResponse: false,
        },
        { assistantDeltas: ["Approved write dry-run completed."], finalResponse: true },
      ],
      maxContinuations: 4,
    });
    scenarios.push({
      name: "approval-resume",
      ok: paused?.turn?.status === "waitingForApproval" && Boolean(paused?.awaitingApproval) && resumed?.turn?.status === "completed" && hasEventType(resumed?.events, "approvalResolved"),
      status: `${paused?.turn?.status ?? "unknown"}->${resumed?.turn?.status ?? "unknown"}`,
      eventCount: (Array.isArray(paused?.events) ? paused.events.length : 0) + (Array.isArray(resumed?.events) ? resumed.events.length : 0),
    });

    const backgroundStream = await request<any>("turn/run-agentic", {
      threadId,
      input: "background stream dry-run",
      executionMode: "background",
      modelSteps: [
        {
          assistantDeltas: ["streamed ", "delta"],
          toolCalls: [{ id: "background-stream-echo", name: "echo", namespace: "oppi", arguments: { output: "slow", delayMs: 250 } }],
          finalResponse: false,
        },
        { assistantDeltas: [" after slow tool"], finalResponse: true },
      ],
      maxContinuations: 2,
    });
    const backgroundStreamTurnId = String(backgroundStream?.turn?.id ?? "");
    let sawBackgroundDeltaBeforeCompletion = false;
    const backgroundStreamEvents = await pollEvents((events) => {
      const eventsForTurn = turnEvents(events, backgroundStreamTurnId);
      const sawDelta = hasEventType(eventsForTurn, "itemDelta");
      const sawCompleted = hasEventType(eventsForTurn, "turnCompleted");
      if (sawDelta && !sawCompleted) {
        sawBackgroundDeltaBeforeCompletion = true;
        return true;
      }
      return false;
    });
    const backgroundStreamFinalEvents = await pollEvents((events) => hasEventType(turnEvents(events, backgroundStreamTurnId), "turnCompleted"));
    scenarios.push({
      name: "background-stream",
      ok: Boolean(backgroundStreamTurnId) && sawBackgroundDeltaBeforeCompletion && hasEventType(turnEvents(backgroundStreamFinalEvents, backgroundStreamTurnId), "turnCompleted"),
      status: sawBackgroundDeltaBeforeCompletion ? "delta-before-complete" : "missing-early-delta",
      eventCount: turnEvents(backgroundStreamEvents, backgroundStreamTurnId).length,
    });

    const backgroundInterrupt = await request<any>("turn/run-agentic", {
      threadId,
      input: "background interrupt dry-run",
      executionMode: "background",
      modelSteps: [{
        assistantDeltas: ["interruptible"],
        toolCalls: [{ id: "background-interrupt-echo", name: "echo", namespace: "oppi", arguments: { output: "slow", delayMs: 250 } }],
        finalResponse: false,
      }],
      maxContinuations: 2,
    });
    const backgroundInterruptTurnId = String(backgroundInterrupt?.turn?.id ?? "");
    const backgroundInterrupted = await request<any>("turn/interrupt", {
      threadId,
      turnId: backgroundInterruptTurnId,
      reason: "runtime-loop smoke interrupt",
    });
    const backgroundInterruptEvents = turnEvents(backgroundInterrupted?.events, backgroundInterruptTurnId);
    scenarios.push({
      name: "background-interrupt",
      ok: Boolean(backgroundInterruptTurnId) && hasEventType(backgroundInterruptEvents, "turnInterrupted") && !hasEventType(backgroundInterruptEvents, "turnCompleted"),
      status: hasEventType(backgroundInterruptEvents, "turnInterrupted") ? "interrupted" : "missing-interrupt",
      eventCount: backgroundInterruptEvents.length,
    });

    const backgroundApprovalCall = { id: "background-approval-dry-run", name: "write", namespace: "pi", arguments: { path: "background-dry-run.txt" } };
    const backgroundPaused = await request<any>("turn/run-agentic", {
      threadId,
      input: "background approval dry-run",
      executionMode: "background",
      toolDefinitions: [approvalTool],
      modelSteps: [{
        assistantDeltas: ["Need background approval."],
        toolCalls: [backgroundApprovalCall],
        finalResponse: false,
      }],
      maxContinuations: 4,
    });
    const backgroundApprovalTurnId = String(backgroundPaused?.turn?.id ?? "");
    const backgroundApprovalEvents = await pollEvents((events) => hasEventType(turnEvents(events, backgroundApprovalTurnId), "approvalRequested"));
    const backgroundResumed = await request<any>("turn/resume-agentic", {
      threadId,
      turnId: backgroundApprovalTurnId,
      toolDefinitions: [approvalTool],
      approvedToolCallIds: [backgroundApprovalCall.id],
      modelSteps: [
        {
          assistantDeltas: [],
          toolCalls: [backgroundApprovalCall],
          toolResults: [{ callId: backgroundApprovalCall.id, status: "ok", output: "background approved dry-run result" }],
          finalResponse: false,
        },
        { assistantDeltas: ["Background approval dry-run completed."], finalResponse: true },
      ],
      maxContinuations: 4,
    });
    scenarios.push({
      name: "background-resume",
      ok: Boolean(backgroundApprovalTurnId) && hasEventType(turnEvents(backgroundApprovalEvents, backgroundApprovalTurnId), "approvalRequested") && backgroundResumed?.turn?.status === "completed" && hasEventType(backgroundResumed?.events, "approvalResolved"),
      status: `${backgroundPaused?.turn?.status ?? "unknown"}->${backgroundResumed?.turn?.status ?? "unknown"}`,
      eventCount: turnEvents(backgroundApprovalEvents, backgroundApprovalTurnId).length + (Array.isArray(backgroundResumed?.events) ? backgroundResumed.events.length : 0),
    });

    const cancelled = await request<any>("turn/run-agentic", {
      threadId,
      input: "cancellation dry-run",
      modelSteps: [{
        assistantDeltas: ["Starting cancellable tool."],
        toolCalls: [{ id: "cancel-dry-run", name: "read", namespace: "pi", arguments: { path: "README.md" } }],
        finalResponse: false,
      }],
      cancellation: { reason: "dry-run cancellation", toolCallIds: ["cancel-dry-run"] },
      maxContinuations: 2,
    });
    scenarios.push({
      name: "cancellation",
      ok: cancelled?.turn?.status === "aborted" && toolStatuses(cancelled?.events).includes("aborted") && hasEventType(cancelled?.events, "turnAborted"),
      status: cancelled?.turn?.status,
      eventCount: Array.isArray(cancelled?.events) ? cancelled.events.length : undefined,
    });

    try {
      await request("turn/run-agentic", {
        threadId,
        input: "pairing error dry-run",
        modelSteps: [{
          assistantDeltas: ["This step returns an unmatched tool result."],
          toolResults: [{ callId: "missing-call", status: "ok", output: "orphan" }],
          finalResponse: false,
        }],
        maxContinuations: 1,
      });
      scenarios.push({ name: "pairing-error", ok: false, status: "unexpected-success" });
    } catch (error) {
      if (!(error instanceof RuntimeLoopSmokeRpcError)) throw error;
      const expectedErrorCode = runtimeErrorDataCode(error);
      scenarios.push({
        name: "pairing-error",
        ok: expectedErrorCode === "tool_result_without_call",
        status: "expected-error",
        expectedErrorCode,
        diagnostics: [error.message],
      });
    }

    const guarded = await request<any>("turn/run-agentic", {
      threadId,
      input: "guard abort dry-run",
      modelSteps: [{
        assistantDeltas: ["Looping once."],
        toolCalls: [{ id: "guard-dry-run", name: "echo", namespace: "oppi", arguments: { output: "again" } }],
        finalResponse: false,
      }],
      maxContinuations: 0,
    });
    scenarios.push({
      name: "guard-abort",
      ok: guarded?.turn?.status === "aborted" && hasEventType(guarded?.events, "turnAborted"),
      status: guarded?.turn?.status,
      eventCount: Array.isArray(guarded?.events) ? guarded.events.length : undefined,
    });

    const compactionSummary = longCompactionSummary();
    const compact = await request<any>("memory/compact", { threadId, summary: compactionSummary });
    scenarios.push({
      name: "long-compaction",
      ok: hasEventType(compact?.events, "handoffCompacted"),
      status: hasEventType(compact?.events, "handoffCompacted") ? "compacted" : "missing-event",
      eventCount: Array.isArray(compact?.events) ? compact.events.length : undefined,
      diagnostics: [`summary bytes: ${Buffer.byteLength(compactionSummary, "utf8")}`],
    });

    const listed = await request<any>("events/list", { threadId, limit: 5000 });
    const debug = await request<any>("debug/bundle", {});
    const allEvents = Array.isArray(listed.events) ? listed.events : [];
    const encodedEvents = JSON.stringify(allEvents);
    const bridgedEventCount = (encodedEvents.match(/piAdapterEvent/g) ?? []).length || (encodedEvents.match(/smoke\/message_update/g) ?? []).length;
    diagnostics.push(`server capabilities: ${(init.serverCapabilities ?? []).join(", ") || "unknown"}`);
    diagnostics.push(`debug bundle metrics: ${JSON.stringify(debug.metrics ?? {})}`);
    await request("server/shutdown", {});
    child.stdin.end();
    const code = await closePromise;
    if (code !== 0) diagnostics.push(`oppi-server exited with code ${code ?? "signal"}`);
    const completed = run?.turn?.status === "completed";
    const debugBundleRedacted = debug.redacted === true;
    const scenariosClean = scenarios.every((scenario) => scenario.ok);
    const bridgeClean = completed && code === 0 && bridgedEventCount > 0 && debugBundleRedacted && scenariosClean;
    const failedScenarios = scenarios.filter((scenario) => !scenario.ok).map((scenario) => scenario.name);
    const bridgeCleanReason = bridgeClean
      ? "turn completed, bridge events persisted, dry-run hardening scenarios passed, debug bundle was redacted, and server shut down cleanly"
      : `bridge did not satisfy completion/event/scenario/redaction/shutdown checks${failedScenarios.length ? `; failed scenarios: ${failedScenarios.join(", ")}` : ""}`;
    return {
      ok: completed && code === 0 && scenariosClean,
      mode,
      serverBin,
      threadId,
      turnId: run?.turn?.id,
      turnStatus: run?.turn?.status,
      eventCount: allEvents.length,
      bridgedEventCount,
      toolResultStatuses: bridgeToolStatuses,
      debugBundleRedacted,
      bridgeClean,
      bridgeCleanReason,
      scenarios,
      durationMs: Date.now() - started,
      diagnostics,
    };
  } catch (error) {
    child.kill();
    return {
      ok: false,
      mode,
      serverBin,
      durationMs: Date.now() - started,
      diagnostics: [redactText(error instanceof Error ? error.message : String(error))],
    };
  } finally {
    lines.close();
  }
}

type MockOpenAiToolCall = { id: string; name: string; arguments: Record<string, unknown> };

type MockOpenAiServerOptions = {
  toolCall?: MockOpenAiToolCall;
  finalText?: string;
};

function defaultMockOpenAiToolCall(): MockOpenAiToolCall {
  return { id: "direct-echo-smoke", name: "oppi__echo", arguments: { output: "direct tool output" } };
}

function parseMockOpenAiToolCall(value: string | undefined, diagnostics: string[]): MockOpenAiToolCall | undefined {
  if (!value?.trim()) return undefined;
  try {
    const parsed = JSON.parse(value);
    const id = typeof parsed.id === "string" && parsed.id.trim() ? parsed.id.trim() : "runtime-worker-mock-tool";
    const name = typeof parsed.name === "string" && parsed.name.trim() ? parsed.name.trim() : undefined;
    const args = parsed.arguments && typeof parsed.arguments === "object" && !Array.isArray(parsed.arguments) ? parsed.arguments : undefined;
    if (!name || !args) throw new Error("mock tool call must include string name and object arguments");
    return { id, name, arguments: args };
  } catch (error) {
    diagnostics.push(`ignored invalid OPPI_RUNTIME_WORKER_MOCK_TOOL_CALL: ${redactText(error instanceof Error ? error.message : String(error))}`);
    return undefined;
  }
}

async function startMockOpenAiServer(options: MockOpenAiServerOptions = {}): Promise<{ baseUrl: string; requests: any[]; close: () => Promise<void> }> {
  const requests: any[] = [];
  const configuredToolCall = options.toolCall ?? defaultMockOpenAiToolCall();
  const server = createServer((req: any, res: any) => {
    let body = "";
    req.setEncoding?.("utf8");
    req.on("data", (chunk: string) => { body += chunk; });
    req.on("end", () => {
      requests.push({ method: req.method, url: req.url, headers: req.headers ?? {}, body });
      const parsed = (() => { try { return JSON.parse(body); } catch { return {}; } })();
      const messages = Array.isArray(parsed.messages) ? parsed.messages : [];
      const toolMessage = messages.find((message: any) => message?.role === "tool");
      res.statusCode = 200;
      const sendSse = (chunks: any[]) => {
        res.setHeader("content-type", "text/event-stream");
        res.end(`${chunks.map((chunk) => `data: ${JSON.stringify(chunk)}\n\n`).join("")}data: [DONE]\n\n`);
      };
      if (!toolMessage) {
        const argumentText = JSON.stringify(configuredToolCall.arguments);
        const splitAt = Math.max(1, Math.floor(argumentText.length / 2));
        if (parsed.stream === true) {
          sendSse([
            { choices: [{ delta: { tool_calls: [{ index: 0, id: configuredToolCall.id, type: "function", function: { name: configuredToolCall.name, arguments: argumentText.slice(0, splitAt) } }] } }] },
            { choices: [{ delta: { tool_calls: [{ index: 0, function: { arguments: argumentText.slice(splitAt) } }] } }] },
          ]);
          return;
        }
        res.setHeader("content-type", "application/json");
        res.end(JSON.stringify({
          id: "chatcmpl-oppi-smoke-tool",
          object: "chat.completion",
          choices: [{
            index: 0,
            message: {
              role: "assistant",
              content: null,
              tool_calls: [{
                id: configuredToolCall.id,
                type: "function",
                function: { name: configuredToolCall.name, arguments: argumentText },
              }],
            },
            finish_reason: "tool_calls",
          }],
        }));
        return;
      }
      const finalText = options.finalText ?? `Rust direct provider smoke completed with ${toolMessage.content}.`;
      if (parsed.stream === true) {
        const splitAt = Math.max(1, Math.floor(finalText.length / 2));
        sendSse([
          { choices: [{ delta: { content: finalText.slice(0, splitAt) } }] },
          { choices: [{ delta: { content: finalText.slice(splitAt) } }] },
        ]);
        return;
      }
      res.setHeader("content-type", "application/json");
      res.end(JSON.stringify({
        id: "chatcmpl-oppi-smoke-final",
        object: "chat.completion",
        choices: [{
          index: 0,
          message: { role: "assistant", content: finalText },
          finish_reason: "stop",
        }],
      }));
    });
  });
  await new Promise<void>((resolveListen, rejectListen) => {
    server.once("error", rejectListen);
    server.listen(0, "127.0.0.1", () => {
      server.off?.("error", rejectListen);
      resolveListen();
    });
  });
  const address = server.address() as any;
  return {
    baseUrl: `http://127.0.0.1:${address.port}/v1`,
    requests,
    close: () => new Promise<void>((resolveClose, rejectClose) => server.close((error: Error | undefined) => error ? rejectClose(error) : resolveClose())),
  };
}

async function runRuntimeWorkerSmoke(env: Env = process.env, cwd = process.cwd()): Promise<RuntimeWorkerSmokeResult> {
  const started = Date.now();
  const diagnostics: string[] = [];
  const serverBin = resolveOppiServerBin(env, cwd);
  if (!serverBin) {
    return {
      ok: false,
      durationMs: Date.now() - started,
      diagnostics: ["oppi-server not found; build with `cargo build -p oppi-server` or set OPPI_SERVER_BIN"],
    };
  }

  const mock = await startMockOpenAiServer();
  const smokeKey = "oppi-runtime-worker-smoke-key";
  const serverCommand = process.platform === "win32" && /\.(?:cmd|bat)$/i.test(serverBin)
    ? { command: "cmd.exe", args: ["/d", "/s", "/c", serverBin, "--stdio"] }
    : { command: serverBin, args: ["--stdio"] };
  const child = spawn(serverCommand.command, serverCommand.args, {
    cwd,
    env: { ...process.env, ...env, OPPI_EXPERIMENTAL_RUNTIME: "1", OPPI_DIRECT_PROVIDER_SMOKE_API_KEY: smokeKey },
    stdio: ["pipe", "pipe", "pipe"],
    windowsHide: true,
  });
  let stderr = "";
  let nextId = 0;
  child.stderr?.on("data", (chunk: Buffer) => { stderr += chunk.toString("utf8"); });
  const lines = createInterface({ input: child.stdout });
  const iterator = lines[Symbol.asyncIterator]();
  const closePromise = new Promise<number | null>((resolveClose, rejectClose) => {
    child.on("error", rejectClose);
    child.on("close", resolveClose);
  });
  const withAuth = (params: Record<string, unknown> = {}) => {
    const token = env.OPPI_SERVER_AUTH_TOKEN?.trim();
    return token ? { ...params, authToken: token } : params;
  };
  const request = async <T>(method: string, params: Record<string, unknown> = {}): Promise<T> => {
    const id = `runtime-worker-smoke-${++nextId}`;
    child.stdin.write(`${JSON.stringify({ jsonrpc: "2.0", id, method, params: withAuth(params) })}\n`);
    const line = await iterator.next();
    if (line.done || !line.value) throw new Error(`oppi-server returned no JSON-RPC response.${stderr.trim() ? ` stderr: ${redactText(stderr.trim())}` : ""}`);
    const response = JSON.parse(line.value) as JsonRpcResponse<T>;
    if (response.error) throw new RuntimeLoopSmokeRpcError(response.error);
    if (response.result === undefined) throw new Error(`${method} returned no result`);
    return response.result;
  };

  try {
    const init = await request<any>("initialize", {
      clientName: "oppi-runtime-worker-smoke",
      clientVersion: cliVersion(),
      protocolVersion: "0.1.0",
      clientCapabilities: ["threads", "turns", "events", "models"],
    });
    if (init.protocolCompatible === false) diagnostics.push(`protocol compatibility warning: server ${init.protocolVersion}`);
    const start = await request<any>("thread/start", {
      project: { id: "runtime-worker-smoke", cwd, displayName: "Runtime worker smoke" },
      title: "Runtime worker smoke",
    });
    const threadId = String(start.thread.id);
    const run = await request<any>("turn/run-agentic", {
      threadId,
      input: "Complete the Rust direct provider smoke in one short sentence.",
      modelProvider: {
        kind: "openai-compatible",
        model: "oppi-smoke-model",
        baseUrl: mock.baseUrl,
        apiKeyEnv: "OPPI_DIRECT_PROVIDER_SMOKE_API_KEY",
        systemPrompt: "You are the OPPi runtime-worker smoke mock.",
        maxOutputTokens: 64,
        stream: true,
      },
      maxContinuations: 1,
    });
    const listed = await request<any>("events/list", { threadId, limit: 5000 });
    const debug = await request<any>("debug/bundle", {});
    await request("server/shutdown", {});
    child.stdin.end();
    const code = await closePromise;
    if (code !== 0) diagnostics.push(`oppi-server exited with code ${code ?? "signal"}`);
    const allEvents = Array.isArray(listed.events) ? listed.events : [];
    const assistantText = assistantTextFromEvents(run?.events) || assistantTextFromEvents(allEvents);
    const providerAuthorized = mock.requests.some((request) => String(request.headers?.authorization ?? request.headers?.Authorization ?? "") === `Bearer ${smokeKey}`);
    const providerRequestCount = mock.requests.length;
    const providerStreamed = mock.requests.some((request) => { try { return JSON.parse(request.body || "{}").stream === true; } catch { return false; } });
    const assistantDeltaCount = assistantDeltaEvents(run?.events).length;
    const directToolStatuses = toolStatuses(run?.events);
    const completed = run?.turn?.status === "completed";
    const debugBundleRedacted = debug.redacted === true;
    const workerClean = completed && code === 0 && providerRequestCount === 2 && providerAuthorized && providerStreamed && assistantDeltaCount >= 2 && directToolStatuses.includes("ok") && assistantText.includes("Rust direct provider smoke") && debugBundleRedacted;
    const workerCleanReason = workerClean
      ? "Rust called the OpenAI-compatible provider directly, executed a provider-requested tool, continued the provider turn, emitted assistant events, redacted debug data, and shut down cleanly"
      : "direct worker smoke did not satisfy provider-call/tool/completion/redaction/shutdown checks";
    diagnostics.push(`server capabilities: ${(init.serverCapabilities ?? []).join(", ") || "unknown"}`);
    return {
      ok: workerClean,
      serverBin,
      threadId,
      turnId: run?.turn?.id,
      turnStatus: run?.turn?.status,
      eventCount: allEvents.length,
      providerRequestCount,
      providerAuthorized,
      providerStreamed,
      assistantDeltaCount,
      toolResultStatuses: directToolStatuses,
      assistantText,
      debugBundleRedacted,
      workerClean,
      workerCleanReason,
      durationMs: Date.now() - started,
      diagnostics,
    };
  } catch (error) {
    child.kill();
    return {
      ok: false,
      serverBin,
      providerRequestCount: mock.requests.length,
      durationMs: Date.now() - started,
      diagnostics: [redactText(error instanceof Error ? error.message : String(error))],
    };
  } finally {
    lines.close();
    await mock.close().catch(() => undefined);
  }
}

async function runRuntimeWorkerPrompt(command: Extract<OppiCommand, { type: "runtime-worker"; subcommand: "run" }>, env: Env = process.env, cwd = process.cwd()): Promise<RuntimeWorkerRunResult> {
  const started = Date.now();
  const diagnostics: string[] = [];
  const fallbackCommand = runtimeWorkerFallbackCommand(command.prompt);
  const serverBin = resolveOppiServerBin(env, cwd);
  if (!serverBin) {
    return {
      ok: false,
      fallbackAvailable: true,
      fallbackCommand,
      durationMs: Date.now() - started,
      diagnostics: ["oppi-server not found; build with `cargo build -p oppi-server` or set OPPI_SERVER_BIN", `Stable Pi fallback remains available: ${fallbackCommand}`],
    };
  }

  let mock: Awaited<ReturnType<typeof startMockOpenAiServer>> | undefined;
  const provider = selectedRuntimeWorkerProvider(command, env);
  const providerKeyEnv = command.apiKeyEnv?.trim() || directWorkerApiKeyEnvName(env);
  if (provider === "openai-codex" && (providerKeyEnv || command.baseUrl)) {
    return {
      ok: false,
      serverBin,
      providerConfigured: false,
      provider,
      fallbackAvailable: true,
      fallbackCommand,
      durationMs: Date.now() - started,
      diagnostics: ["Codex subscription provider uses the protected auth store; do not pass --api-key-env or --base-url.", `Stable Pi fallback remains available: ${fallbackCommand}`],
    };
  }
  if (provider === "openai-compatible" && providerKeyEnv && !isDirectWorkerApiKeyEnvAllowed(providerKeyEnv)) {
    return {
      ok: false,
      serverBin,
      providerConfigured: false,
      provider,
      fallbackAvailable: true,
      fallbackCommand,
      durationMs: Date.now() - started,
      diagnostics: [`Direct-provider apiKeyEnv is not allowed: ${providerKeyEnv}. Use OPPI_OPENAI_API_KEY, OPENAI_API_KEY, or an OPPI_*_API_KEY variable.`, `Stable Pi fallback remains available: ${fallbackCommand}`],
    };
  }
  const providerConfigured = command.mock
    || (provider === "openai-codex" ? hasRuntimeWorkerCodexAuth(env) : hasDirectWorkerProviderAuth(env, providerKeyEnv));
  if (!providerConfigured) {
    const authHint = provider === "openai-codex"
      ? `Run /login subscription codex, or set OPPI_OPENAI_CODEX_AUTH_PATH to an auth.json containing openai-codex OAuth credentials. Checked ${runtimeWorkerCodexAuthPath(env)}.`
      : providerKeyEnv
        ? `Set ${providerKeyEnv} or choose another --api-key-env.`
        : "Set OPPI_OPENAI_API_KEY / OPENAI_API_KEY, or pass --api-key-env naming a configured OPPI_*_API_KEY variable.";
    return {
      ok: false,
      serverBin,
      providerConfigured: false,
      provider,
      fallbackAvailable: true,
      fallbackCommand,
      durationMs: Date.now() - started,
      diagnostics: [`Direct-provider auth is not configured. ${authHint}`, `Stable Pi fallback remains available: ${fallbackCommand}`],
    };
  }

  const childEnv: Env = { ...process.env, ...env, OPPI_EXPERIMENTAL_RUNTIME: "1" };
  const effort = selectedRuntimeWorkerEffort(command, env);
  const providerReasoningEffort = openAiCompatibleReasoningEffort(effort, diagnostics);
  if (effort) diagnostics.push(`OPPi direct-worker effort: ${effort}${providerReasoningEffort ? ` (reasoning_effort=${providerReasoningEffort})` : ""}`);
  let modelProvider: Record<string, unknown>;
  if (command.mock) {
    const mockKey = "oppi-runtime-worker-mock-key";
    const mockToolCall = parseMockOpenAiToolCall(env.OPPI_RUNTIME_WORKER_MOCK_TOOL_CALL, diagnostics) ?? defaultMockOpenAiToolCall();
    mock = await startMockOpenAiServer({
      toolCall: mockToolCall,
      finalText: env.OPPI_RUNTIME_WORKER_MOCK_FINAL || `Rust direct provider run completed with ${mockToolCall.id}.`,
    });
    childEnv.OPPI_DIRECT_PROVIDER_MOCK_API_KEY = mockKey;
    modelProvider = {
      kind: "openai-compatible",
      model: command.model || "oppi-mock-model",
      baseUrl: mock.baseUrl,
      apiKeyEnv: "OPPI_DIRECT_PROVIDER_MOCK_API_KEY",
      systemPrompt: command.systemPrompt || "You are the OPPi runtime-worker local mock.",
      ...(providerReasoningEffort ? { reasoningEffort: providerReasoningEffort } : {}),
      maxOutputTokens: command.maxOutputTokens ?? 512,
      stream: command.stream,
    };
  } else {
    if (provider === "openai-codex") {
      modelProvider = {
        kind: "openai-codex",
        model: command.model || env.OPPI_RUNTIME_WORKER_MODEL?.trim() || "gpt-5.4",
        systemPrompt: command.systemPrompt || env.OPPI_RUNTIME_WORKER_SYSTEM_PROMPT?.trim() || "You are OPPi's experimental direct Rust worker. Be concise, use tools only when useful, and surface uncertainty.",
        ...(effort && effort !== "off" ? { reasoningEffort: effort } : {}),
        maxOutputTokens: command.maxOutputTokens ?? 2048,
        stream: command.stream,
      };
      diagnostics.push(`OPPi direct-worker provider: openai-codex auth-store=${runtimeWorkerCodexAuthPath(env)}`);
    } else {
      modelProvider = {
        kind: "openai-compatible",
        model: command.model || env.OPPI_RUNTIME_WORKER_MODEL?.trim() || "gpt-4.1-mini",
        ...(command.baseUrl || env.OPPI_RUNTIME_WORKER_BASE_URL?.trim() ? { baseUrl: command.baseUrl || env.OPPI_RUNTIME_WORKER_BASE_URL?.trim() } : {}),
        ...(providerKeyEnv ? { apiKeyEnv: providerKeyEnv } : {}),
        systemPrompt: command.systemPrompt || env.OPPI_RUNTIME_WORKER_SYSTEM_PROMPT?.trim() || "You are OPPi's experimental direct Rust worker. Be concise, use tools only when useful, and surface uncertainty.",
        ...(providerReasoningEffort ? { reasoningEffort: providerReasoningEffort } : {}),
        maxOutputTokens: command.maxOutputTokens ?? 2048,
        stream: command.stream,
      };
    }
  }

  const permissionMode = selectedRuntimeWorkerPermissionMode(env, cwd);
  const sandboxPolicy = runtimeWorkerSandboxPolicy(permissionMode, cwd);
  diagnostics.push(`OPPi direct-worker permission mode: ${permissionMode}`);

  const promptVariant = prepareRuntimeWorkerPromptVariant(command, env, cwd);
  diagnostics.push(...promptVariant.diagnostics);
  modelProvider.systemPrompt = appendPromptVariantToSystemPrompt(modelProvider.systemPrompt, promptVariant);

  const featureGuidance = prepareRuntimeWorkerFeatureGuidance(promptVariant, env, cwd);
  diagnostics.push(...featureGuidance.diagnostics);
  modelProvider.systemPrompt = appendFeatureGuidanceToSystemPrompt(modelProvider.systemPrompt, featureGuidance);

  const followUp = prepareRuntimeWorkerFollowUpChain(promptVariant, env, cwd, diagnostics);

  const memory = await prepareRuntimeWorkerMemoryBridge(command, env, cwd);
  diagnostics.push(...memory.diagnostics);
  modelProvider.systemPrompt = appendMemoryToSystemPrompt(modelProvider.systemPrompt, memory.contextMarkdown);

  const serverCommand = process.platform === "win32" && /\.(?:cmd|bat)$/i.test(serverBin)
    ? { command: "cmd.exe", args: ["/d", "/s", "/c", serverBin, "--stdio"] }
    : { command: serverBin, args: ["--stdio"] };
  const child = spawn(serverCommand.command, serverCommand.args, {
    cwd,
    env: childEnv as NodeJS.ProcessEnv,
    stdio: ["pipe", "pipe", "pipe"],
    windowsHide: true,
  });
  let stderr = "";
  let nextId = 0;
  child.stderr?.on("data", (chunk: Buffer) => { stderr += chunk.toString("utf8"); });
  const lines = createInterface({ input: child.stdout });
  const iterator = lines[Symbol.asyncIterator]();
  const closePromise = new Promise<number | null>((resolveClose, rejectClose) => {
    child.on("error", rejectClose);
    child.on("close", resolveClose);
  });
  const withAuth = (params: Record<string, unknown> = {}) => {
    const token = env.OPPI_SERVER_AUTH_TOKEN?.trim();
    return token ? { ...params, authToken: token } : params;
  };
  const request = async <T>(method: string, params: Record<string, unknown> = {}): Promise<T> => {
    const id = `runtime-worker-run-${++nextId}`;
    child.stdin.write(`${JSON.stringify({ jsonrpc: "2.0", id, method, params: withAuth(params) })}\n`);
    const line = await iterator.next();
    if (line.done || !line.value) throw new Error(`oppi-server returned no JSON-RPC response.${stderr.trim() ? ` stderr: ${redactText(stderr.trim())}` : ""}`);
    const response = JSON.parse(line.value) as JsonRpcResponse<T>;
    if (response.error) throw new RuntimeLoopSmokeRpcError(response.error);
    if (response.result === undefined) throw new Error(`${method} returned no result`);
    return response.result;
  };

  try {
    const init = await request<any>("initialize", {
      clientName: "oppi-runtime-worker",
      clientVersion: cliVersion(),
      protocolVersion: "0.1.0",
      clientCapabilities: ["threads", "turns", "events", "models", "approvals"],
    });
    if (init.protocolCompatible === false) diagnostics.push(`protocol compatibility warning: server ${init.protocolVersion}`);
    const start = await request<any>("thread/start", {
      project: { id: "runtime-worker", cwd, displayName: "Runtime worker" },
      title: command.prompt.slice(0, 80) || "Runtime worker",
    });
    const threadId = String(start.thread.id);
    if (memory.enabled) {
      await request("memory/set", {
        threadId,
        status: {
          enabled: memory.available,
          backend: "hoppi",
          scope: "project",
          memoryCount: memory.memoryCount ?? 0,
        },
      }).catch((error) => diagnostics.push(`Rust memory status update failed: ${redactText(error instanceof Error ? error.message : String(error))}`));
    }
    let run = await request<any>("turn/run-agentic", {
      threadId,
      input: command.prompt,
      ...(followUp ? { followUp } : {}),
      sandboxPolicy,
      modelProvider,
      maxContinuations: 8,
    });
    let approvalsAutoApproved = 0;
    while (run?.awaitingApproval && command.autoApprove && approvalsAutoApproved < 4) {
      const callId = String(run.awaitingApproval?.toolCall?.id ?? "");
      if (!callId) break;
      approvalsAutoApproved += 1;
      run = await request<any>("turn/resume-agentic", {
        threadId,
        turnId: run.turn.id,
        approvedToolCallIds: [callId],
        sandboxPolicy,
        modelProvider,
        maxContinuations: 8,
      });
    }
    if (run?.awaitingApproval && !command.autoApprove) diagnostics.push("A tool approval is required; rerun with --auto-approve/--approve-all only if this experimental direct-worker action is acceptable.");
    if (approvalsAutoApproved > 0) diagnostics.push(`auto-approved ${approvalsAutoApproved} direct-worker tool call(s) for this explicit run`);

    const listed = await request<any>("events/list", { threadId, limit: 5000 });
    const todoState = await request<any>("todos/list", {}).catch((error) => {
      diagnostics.push(`Rust todo state unavailable: ${redactText(error instanceof Error ? error.message : String(error))}`);
      return undefined;
    });
    const debug = await request<any>("debug/bundle", {});
    await request("server/shutdown", {});
    child.stdin.end();
    const code = await closePromise;
    if (code !== 0) diagnostics.push(`oppi-server exited with code ${code ?? "signal"}`);

    const allEvents = Array.isArray(listed.events) ? listed.events : [];
    const assistantText = assistantTextFromEvents(run?.events) || assistantTextFromEvents(allEvents);
    const directToolStatuses = toolStatuses(run?.events);
    const assistantDeltaCount = assistantDeltaEvents(run?.events).length;
    const providerStreamed = command.mock
      ? mock?.requests.some((request) => { try { return JSON.parse(request.body || "{}").stream === true; } catch { return false; } })
      : command.stream;
    const completed = run?.turn?.status === "completed";
    const awaitingApproval = Boolean(run?.awaitingApproval);
    const debugBundleRedacted = debug.redacted === true;
    const promptVariantProviderPromptIncluded = command.mock ? providerPromptContainsText(mock?.requests, promptVariant.text) : undefined;
    if (promptVariant.applied && promptVariantProviderPromptIncluded === false) diagnostics.push("OPPi prompt variant was applied but did not reach the mock provider system prompt.");
    const featureGuidanceProviderPromptIncluded = command.mock ? providerPromptContainsText(mock?.requests, featureGuidance.text) : undefined;
    if (featureGuidance.applied && featureGuidanceProviderPromptIncluded === false) diagnostics.push("OPPi feature-routing guidance was applied but did not reach the mock provider system prompt.");
    const followUpProviderPromptIncluded = command.mock && followUp ? providerPromptContainsText(mock?.requests, followUp.rootPrompt) : undefined;
    if (followUp && followUpProviderPromptIncluded === false) diagnostics.push("OPPi follow-up context was delegated to Rust but did not reach the mock provider system prompt.");
    const memoryProviderPromptIncluded = command.mock ? providerPromptContainsMemory(mock?.requests, memory.contextMarkdown) : undefined;
    if (memory.loaded && memoryProviderPromptIncluded === false) diagnostics.push("Hoppi memory was loaded but did not reach the mock provider system prompt.");
    const memorySaved = await rememberRuntimeWorkerTurn(memory, command.prompt, assistantText, directToolStatuses, threadId, run?.turn?.id);
    for (const diagnostic of memory.diagnostics) {
      if (!diagnostics.includes(diagnostic)) diagnostics.push(diagnostic);
    }
    diagnostics.push(`server capabilities: ${(init.serverCapabilities ?? []).join(", ") || "unknown"}`);
    if (awaitingApproval) diagnostics.push(`Stable Pi fallback remains available: ${fallbackCommand}`);
    return {
      ok: completed && code === 0 && !awaitingApproval && promptVariantProviderPromptIncluded !== false && featureGuidanceProviderPromptIncluded !== false && followUpProviderPromptIncluded !== false && memoryProviderPromptIncluded !== false,
      serverBin,
      threadId,
      turnId: run?.turn?.id,
      turnStatus: run?.turn?.status,
      eventCount: allEvents.length,
      providerConfigured: true,
      provider,
      providerRequestCount: mock?.requests.length,
      providerStreamed,
      assistantDeltaCount,
      approvalsAutoApproved,
      awaitingApproval,
      toolResultStatuses: directToolStatuses,
      assistantText,
      todos: Array.isArray(todoState?.state?.todos) ? todoState.state.todos : undefined,
      todoSummary: typeof todoState?.state?.summary === "string" && todoState.state.summary ? todoState.state.summary : undefined,
      debugBundleRedacted,
      promptVariant: promptVariant.variant,
      promptVariantApplied: promptVariant.applied,
      promptVariantProviderPromptIncluded,
      featureGuidanceApplied: featureGuidance.applied,
      featureGuidanceProviderPromptIncluded,
      permissionMode,
      effort,
      providerReasoningEffort,
      followUpApplied: Boolean(followUp),
      followUpProviderPromptIncluded,
      memoryEnabled: memory.enabled,
      memoryAvailable: memory.available,
      memoryLoaded: memory.loaded,
      memoryContextBytes: memory.contextBytes,
      memoryCount: memory.memoryCount,
      memorySaved,
      memoryProviderPromptIncluded,
      fallbackAvailable: awaitingApproval,
      fallbackCommand: awaitingApproval ? fallbackCommand : undefined,
      durationMs: Date.now() - started,
      diagnostics,
    };
  } catch (error) {
    child.kill();
    return {
      ok: false,
      serverBin,
      providerConfigured: true,
      provider,
      providerRequestCount: mock?.requests.length,
      fallbackAvailable: true,
      fallbackCommand,
      durationMs: Date.now() - started,
      diagnostics: [redactText(error instanceof Error ? error.message : String(error)), `Stable Pi fallback remains available: ${fallbackCommand}`],
    };
  } finally {
    lines.close();
    await mock?.close().catch(() => undefined);
  }
}

type WindowsSandboxSetupResult = {
  ok: boolean;
  platform: string;
  account?: string;
  action?: "status" | "planned" | "created" | "updated";
  dryRun?: boolean;
  persistedEnv?: boolean;
  wouldSetEnv?: string[];
  plannedActions?: string[];
  installedFilters?: number;
  restartRequired?: boolean;
  diagnostics: string[];
};

export function generatedWindowsSandboxPassword(): string {
  // Windows `net user` prompts for confirmation above 14 chars; keep setup noninteractive.
  return `${randomBytes(7).toString("base64url")}aA1!`;
}

function runCheckedWindowsCommand(command: string, args: string[], redactedArgs: string[] = args): void {
  const result = spawnSync(command, args, { encoding: "utf8", windowsHide: true });
  if (result.status !== 0) {
    const output = [result.stdout, result.stderr].filter(Boolean).join("\n").trim();
    throw new Error(`${command} ${redactedArgs.join(" ")} failed${output ? `: ${redactText(output)}` : ""}`);
  }
}

function callSandboxSetupRpc(serverBin: string, account: string): number {
  const token = randomBytes(16).toString("hex");
  const id = "windows-wfp-install";
  const request = {
    jsonrpc: "2.0",
    id,
    method: "sandbox/windows-wfp-install",
    params: { authToken: token, account },
  };
  const result = spawnSync(serverBin, ["--stdio"], {
    input: `${JSON.stringify(request)}\n`,
    encoding: "utf8",
    env: { ...process.env, OPPI_SERVER_AUTH_TOKEN: token },
    windowsHide: true,
    timeout: 30_000,
  });
  if (result.status !== 0) {
    throw new Error(`oppi-server WFP setup failed${result.stderr ? `: ${redactText(result.stderr)}` : ""}`);
  }
  const line = String(result.stdout).split(/\r?\n/).find((item: string) => item.trim());
  if (!line) throw new Error("oppi-server WFP setup returned no JSON-RPC response");
  const response = JSON.parse(line) as { result?: { installed?: number }; error?: { message?: string } };
  if (response.error) throw new Error(response.error.message ?? JSON.stringify(response.error));
  return Number(response.result?.installed ?? 0);
}

function setUserEnv(name: string, value: string): void {
  runCheckedWindowsCommand("setx", [name, value], [name, name.includes("PASSWORD") ? "[generated-password]" : value]);
}

function windowsSandboxSetupPlan(envAccount: string, persistEnv: boolean): Pick<WindowsSandboxSetupResult, "plannedActions" | "wouldSetEnv" | "diagnostics"> {
  const plannedActions = [
    `create or update dedicated local Windows sandbox account ${envAccount}`,
    "generate a new random password for the sandbox account",
    `install persistent OPPi WFP network-denial filters for ${envAccount}`,
  ];
  if (persistEnv) plannedActions.push("persist OPPI_WINDOWS_SANDBOX_* env vars with setx");
  return {
    plannedActions,
    wouldSetEnv: [
      "OPPI_WINDOWS_SANDBOX_USERNAME",
      "OPPI_WINDOWS_SANDBOX_PASSWORD",
      "OPPI_WINDOWS_SANDBOX_WFP_READY",
    ],
    diagnostics: [
      "Setup will create or update a dedicated local Windows sandbox account, generate a new password, install OPPi WFP filters for that account, and persist OPPI_WINDOWS_SANDBOX_* env vars when enabled.",
      "No changes were made. Re-run from an elevated PowerShell with `oppi sandbox setup-windows --yes` to apply it.",
    ],
  };
}

async function runSandboxCommand(command: Extract<OppiCommand, { type: "sandbox" }>): Promise<number> {
  const diagnostics: string[] = [];
  const accountName = (command.account?.trim() || "oppi-sandbox").replace(/^\.\\/, "");
  const envAccount = `.\\${accountName}`;
  if (command.subcommand === "status") {
    const diagnostic = collectWindowsSandboxDiagnostic(process.env);
    const result: WindowsSandboxSetupResult = {
      ok: !diagnostic || diagnostic.status === "pass",
      platform: process.platform,
      account: process.env.OPPI_WINDOWS_SANDBOX_USERNAME,
      action: "status",
      persistedEnv: Boolean(process.env.OPPI_WINDOWS_SANDBOX_USERNAME && process.env.OPPI_WINDOWS_SANDBOX_PASSWORD),
      diagnostics: diagnostic ? [diagnostic.message, diagnostic.details].filter(Boolean) as string[] : ["Windows sandbox account setup is not required on this platform."],
    };
    if (command.json) console.log(JSON.stringify(result, null, 2));
    else {
      console.log("OPPi sandbox status");
      console.log(`${result.ok ? "✓" : "!"} ${result.diagnostics.join("\n  ")}`);
    }
    return result.ok ? 0 : 1;
  }

  if (process.platform !== "win32") {
    const plan = command.dryRun
      ? windowsSandboxSetupPlan(envAccount, command.persistEnv)
      : { diagnostics: ["Windows sandbox account setup is only needed on Windows."] };
    const result: WindowsSandboxSetupResult = {
      ok: command.dryRun,
      platform: process.platform,
      account: envAccount,
      action: "planned",
      dryRun: command.dryRun,
      persistedEnv: false,
      restartRequired: false,
      ...plan,
      diagnostics: command.dryRun
        ? ["Windows sandbox account setup is only needed on Windows. No changes were made.", ...plan.diagnostics]
        : plan.diagnostics,
    };
    if (command.json) console.log(JSON.stringify(result, null, 2));
    else console.error(result.diagnostics[0]);
    return command.dryRun ? 0 : 1;
  }

  if (command.dryRun || !command.yes) {
    const plan = windowsSandboxSetupPlan(envAccount, command.persistEnv);
    const result: WindowsSandboxSetupResult = {
      ok: command.dryRun,
      platform: process.platform,
      account: envAccount,
      action: "planned",
      dryRun: command.dryRun,
      persistedEnv: false,
      restartRequired: false,
      ...plan,
    };
    if (command.json) console.log(JSON.stringify(result, null, 2));
    else console.log(result.diagnostics.join("\n"));
    return command.dryRun ? 0 : 1;
  }

  const serverBin = resolveOppiServerBin();
  if (!serverBin) throw new Error("oppi-server not found. Build `cargo build -p oppi-server` or set OPPI_SERVER_BIN before sandbox setup.");

  const password = generatedWindowsSandboxPassword();
  const existed = spawnSync("net", ["user", accountName], { encoding: "utf8", windowsHide: true }).status === 0;
  const netArgs = existed
    ? ["user", accountName, password, "/active:yes", "/passwordchg:no", "/expires:never"]
    : ["user", accountName, password, "/add", "/active:yes", "/passwordchg:no", "/expires:never"];
  runCheckedWindowsCommand("net", netArgs, netArgs.map((arg) => arg === password ? "[generated-password]" : arg));
  diagnostics.push(`${existed ? "updated" : "created"} local sandbox account ${envAccount}`);
  const installedFilters = callSandboxSetupRpc(serverBin, envAccount);
  diagnostics.push(`installed ${installedFilters} OPPi WFP filter(s) for ${envAccount}`);
  if (command.persistEnv) {
    setUserEnv("OPPI_WINDOWS_SANDBOX_USERNAME", envAccount);
    setUserEnv("OPPI_WINDOWS_SANDBOX_PASSWORD", password);
    setUserEnv("OPPI_WINDOWS_SANDBOX_WFP_READY", "1");
    process.env.OPPI_WINDOWS_SANDBOX_USERNAME = envAccount;
    process.env.OPPI_WINDOWS_SANDBOX_PASSWORD = password;
    process.env.OPPI_WINDOWS_SANDBOX_WFP_READY = "1";
    diagnostics.push("persisted OPPI_WINDOWS_SANDBOX_* env vars with setx; restart existing terminals to inherit them");
  }
  const result: WindowsSandboxSetupResult = {
    ok: true,
    platform: process.platform,
    account: envAccount,
    action: existed ? "updated" : "created",
    persistedEnv: command.persistEnv,
    installedFilters,
    restartRequired: command.persistEnv,
    diagnostics,
  };
  if (command.json) console.log(JSON.stringify(result, null, 2));
  else {
    console.log("OPPi Windows sandbox setup");
    console.log(`✓ ${diagnostics.join("\n✓ ")}`);
  }
  return 0;
}

async function runRuntimeWorkerCommand(command: Extract<OppiCommand, { type: "runtime-worker" }>): Promise<number> {
  if (command.subcommand === "smoke") {
    const result = await runRuntimeWorkerSmoke();
    if (command.json) console.log(JSON.stringify(result, null, 2));
    else {
      console.log("OPPi runtime-worker smoke");
      console.log(`${result.ok ? "✓" : "✗"} ${result.turnStatus ?? "not completed"} in ${result.durationMs}ms`);
      if (result.serverBin) console.log(`server: ${result.serverBin}`);
      if (result.threadId) console.log(`thread: ${result.threadId}`);
      if (result.turnId) console.log(`turn: ${result.turnId}`);
      if (typeof result.eventCount === "number") console.log(`events: ${result.eventCount}`);
      if (typeof result.providerRequestCount === "number") console.log(`provider requests: ${result.providerRequestCount}${result.providerAuthorized ? " authorized" : ""}${result.providerStreamed ? ", streamed" : ""}`);
      if (typeof result.assistantDeltaCount === "number") console.log(`assistant deltas: ${result.assistantDeltaCount}`);
      if (result.toolResultStatuses?.length) console.log(`tool results: ${result.toolResultStatuses.join(", ")}`);
      if (result.assistantText) console.log(`assistant: ${result.assistantText}`);
      if (typeof result.workerClean === "boolean") console.log(`worker: ${result.workerClean ? "clean" : "dirty"}${result.workerCleanReason ? ` — ${result.workerCleanReason}` : ""}`);
      for (const diagnostic of result.diagnostics) console.log(`- ${diagnostic}`);
    }
    return result.ok ? 0 : 1;
  }

  const result = await runRuntimeWorkerPrompt(command);
  if (command.json) console.log(JSON.stringify(result, null, 2));
  else {
    console.log("OPPi runtime-worker");
    console.log(`${result.ok ? "✓" : "✗"} ${result.turnStatus ?? "not completed"} in ${result.durationMs}ms`);
    if (result.serverBin) console.log(`server: ${result.serverBin}`);
    if (result.threadId) console.log(`thread: ${result.threadId}`);
    if (result.turnId) console.log(`turn: ${result.turnId}`);
    if (typeof result.eventCount === "number") console.log(`events: ${result.eventCount}`);
    if (typeof result.providerRequestCount === "number") console.log(`provider requests: ${result.providerRequestCount}${result.providerStreamed ? ", streamed" : ""}`);
    if (typeof result.assistantDeltaCount === "number") console.log(`assistant deltas: ${result.assistantDeltaCount}`);
    if (typeof result.approvalsAutoApproved === "number" && result.approvalsAutoApproved > 0) console.log(`auto-approved tools: ${result.approvalsAutoApproved}`);
    if (result.promptVariant && result.promptVariant !== "off") console.log(`prompt variant: ${result.promptVariant}${result.promptVariantApplied ? " applied" : ""}`);
    if (result.featureGuidanceApplied) console.log("feature guidance: applied");
    if (result.effort) console.log(`effort: ${result.effort}${result.providerReasoningEffort ? ` (reasoning_effort=${result.providerReasoningEffort})` : ""}`);
    if (result.permissionMode) console.log(`permissions: ${result.permissionMode}`);
    if (result.followUpApplied) console.log("follow-up context: applied");
    if (typeof result.memoryEnabled === "boolean") console.log(`memory: ${result.memoryEnabled ? (result.memoryLoaded ? `loaded${result.memorySaved ? ", saved" : ""}` : result.memoryAvailable ? "on, no recall" : "unavailable") : "off"}`);
    if (result.todoSummary || result.todos?.length) console.log(`todos: ${result.todoSummary ?? `${result.todos?.length ?? 0} item(s)`}`);
    if (result.toolResultStatuses?.length) console.log(`tool results: ${result.toolResultStatuses.join(", ")}`);
    if (result.assistantText) console.log(`assistant: ${result.assistantText}`);
    if (result.fallbackAvailable && result.fallbackCommand) console.log(`fallback: ${result.fallbackCommand}`);
    for (const diagnostic of result.diagnostics) console.log(`- ${diagnostic}`);
  }
  return result.ok ? 0 : 1;
}

async function runRuntimeLoopCommand(command: Extract<OppiCommand, { type: "runtime-loop" }>): Promise<number> {
  const result = await runRuntimeLoopSmoke();
  if (command.json) console.log(JSON.stringify(result, null, 2));
  else {
    console.log("OPPi runtime-loop smoke");
    console.log(`${result.ok ? "✓" : "✗"} ${result.turnStatus ?? "not completed"} in ${result.durationMs}ms`);
    console.log(`mode: ${result.mode}`);
    if (result.serverBin) console.log(`server: ${result.serverBin}`);
    if (result.threadId) console.log(`thread: ${result.threadId}`);
    if (result.turnId) console.log(`turn: ${result.turnId}`);
    if (typeof result.eventCount === "number") console.log(`events: ${result.eventCount}, bridged: ${result.bridgedEventCount ?? 0}`);
    if (typeof result.bridgeClean === "boolean") console.log(`bridge: ${result.bridgeClean ? "clean" : "dirty"}${result.bridgeCleanReason ? ` — ${result.bridgeCleanReason}` : ""}`);
    for (const scenario of result.scenarios ?? []) console.log(`${scenario.ok ? "✓" : "✗"} scenario ${scenario.name}: ${scenario.status ?? "n/a"}`);
    for (const diagnostic of result.diagnostics) console.log(`- ${diagnostic}`);
  }
  return result.ok ? 0 : 1;
}

function shellArgsHaveOption(args: string[], name: string): boolean {
  return args.includes(name) || args.some((arg) => arg.startsWith(`${name}=`));
}

function shellArgsHaveProvider(args: string[]): boolean {
  return shellArgsHaveOption(args, "--mock") || shellArgsHaveOption(args, "--provider") || shellArgsHaveOption(args, "--model");
}

function shellArgsOnlyListSessions(args: string[]): boolean {
  return shellArgsHaveOption(args, "--list-sessions") || args.includes("--sessions");
}

function withNativeShellDefaults(args: string[], serverBin: string, env: Env = process.env): string[] {
  const next = [...args];
  if (!shellArgsHaveOption(next, "--server")) next.push("--server", serverBin);
  if (!shellArgsOnlyListSessions(next) && !shellArgsHaveProvider(next)) {
    next.unshift("--model", env.OPPI_RUNTIME_WORKER_MODEL?.trim() || "gpt-4.1-mini");
    const baseUrl = env.OPPI_RUNTIME_WORKER_BASE_URL?.trim();
    if (baseUrl && !shellArgsHaveOption(next, "--base-url")) next.push("--base-url", baseUrl);
    const apiKeyEnv = directWorkerApiKeyEnvName(env);
    if (apiKeyEnv && isDirectWorkerApiKeyEnvAllowed(apiKeyEnv) && !shellArgsHaveOption(next, "--api-key-env")) next.push("--api-key-env", apiKeyEnv);
  }
  return next;
}

function nativeShellEnv(command: Extract<OppiCommand, { type: "tui" }>, projectLocalDefault = false): Env {
  const agentDir = projectLocalDefault
    ? resolveDoctorAgentDir(command.agentDir)
    : resolveAgentDir(command.agentDir);
  mkdirSync(agentDir, { recursive: true });
  const runtimeStoreDir = projectLocalDefault && !process.env.OPPI_RUNTIME_STORE_DIR?.trim()
    ? join(agentDir, "runtime-store", `tui-${command.subcommand}-${process.pid}-${Date.now()}-${randomBytes(4).toString("hex")}`)
    : process.env.OPPI_RUNTIME_STORE_DIR;
  return {
    ...process.env,
    OPPI_EXPERIMENTAL_RUNTIME: "1",
    OPPI_AGENT_DIR: agentDir,
    PI_CODING_AGENT_DIR: agentDir,
    ...(runtimeStoreDir ? { OPPI_RUNTIME_STORE_DIR: runtimeStoreDir } : {}),
  };
}

function nativeShellUnavailableMessage(shellBin: string | undefined, serverBin: string | undefined): string {
  const hints = [
    shellBin ? undefined : "Build or configure oppi-shell with `cargo build -p oppi-shell` or OPPI_SHELL_BIN.",
    serverBin ? undefined : "Build or configure oppi-server with `cargo build -p oppi-server` or OPPI_SERVER_BIN.",
  ].filter(Boolean);
  return `Native Rust shell is not ready. ${hints.join(" ")}`.trim();
}

function runNativeShellSmoke(command: Extract<OppiCommand, { type: "tui" }>): number {
  const started = Date.now();
  const shellBin = resolveOppiShellBin();
  const serverBin = resolveOppiServerBin();
  if (!shellBin || !serverBin) {
    const payload = { ok: false, shellBin, serverBin, diagnostics: [nativeShellUnavailableMessage(shellBin, serverBin)] };
    if (command.json) console.log(JSON.stringify(payload, null, 2));
    else console.error(payload.diagnostics[0]);
    return 1;
  }
  const target = windowsCmdShim(shellBin, ["--mock", "--json", "--server", serverBin, "OPPi native shell smoke: reply ok."]);
  const result = spawnSync(target.command, target.args, {
    encoding: "utf8",
    timeout: 15_000,
    windowsHide: true,
    maxBuffer: 1024 * 1024,
    env: nativeShellEnv(command, true),
  });
  const stdout = redactText(String(result.stdout ?? ""));
  const stderr = redactText(String(result.stderr ?? ""));
  const stdoutLines = stdout.split(/\r?\n/).filter(Boolean);
  const ok = !result.error && result.status === 0;
  const payload = {
    ok,
    shellBin,
    serverBin,
    exitCode: result.status,
    durationMs: Date.now() - started,
    stdoutLineCount: stdoutLines.length,
    diagnostics: [
      ok ? "native shell mock smoke completed" : result.error?.message || stderr.trim() || `oppi-shell exited ${result.status ?? "without a code"}`,
    ],
    stderr: stderr.trim() || undefined,
  };
  if (command.json) console.log(JSON.stringify(payload, null, 2));
  else {
    console.log("OPPi native shell smoke");
    console.log(`${ok ? "✓" : "✗"} ${payload.diagnostics[0]}`);
    console.log(`shell: ${shellBin}`);
    console.log(`server: ${serverBin}`);
    if (!ok && stdout.trim()) console.log(stdout.trim().slice(-4_000));
    if (payload.stderr) console.error(payload.stderr);
  }
  return ok ? 0 : 1;
}

type NativeShellDogfoodScenario = { name: string; ok: boolean; status?: string; diagnostics?: string[] };

type NativeShellDogfoodResult = {
  ok: boolean;
  shellBin?: string;
  serverBin?: string;
  exitCode?: number | null;
  durationMs: number;
  stdoutLineCount: number;
  scenarios: NativeShellDogfoodScenario[];
  diagnostics: string[];
  backgroundTaskId?: string;
  strictBackgroundLifecycle?: boolean;
  stderr?: string;
};

function nativeShellDogfoodRequiresBackgroundLifecycle(command: Extract<OppiCommand, { type: "tui" }>): boolean {
  return command.shellArgs.includes("--require-background-lifecycle");
}

function shellEventKind(value: unknown): string | undefined {
  if (!value || typeof value !== "object") return undefined;
  const kind = (value as any).kind;
  if (!kind || typeof kind !== "object") return undefined;
  return typeof kind.type === "string" ? kind.type : undefined;
}

function shellToolOutput(value: unknown): string | undefined {
  if (!value || typeof value !== "object") return undefined;
  const kind = (value as any).kind;
  if (!kind || typeof kind !== "object" || kind.type !== "toolCallCompleted") return undefined;
  const result = kind.result;
  return typeof result?.output === "string" ? result.output : undefined;
}

function shellToolStatus(value: unknown): string | undefined {
  if (!value || typeof value !== "object") return undefined;
  const kind = (value as any).kind;
  if (!kind || typeof kind !== "object" || kind.type !== "toolCallCompleted") return undefined;
  const result = kind.result;
  return typeof result?.status === "string" ? result.status : undefined;
}

function shellToolError(value: unknown): string | undefined {
  if (!value || typeof value !== "object") return undefined;
  const kind = (value as any).kind;
  if (!kind || typeof kind !== "object" || kind.type !== "toolCallCompleted") return undefined;
  const result = kind.result;
  return typeof result?.error === "string" ? result.error : undefined;
}

function extractBackgroundTaskId(output: string): string | undefined {
  return output.split(/\r?\n/).find((line) => line.startsWith("background shell task started: "))?.slice("background shell task started: ".length).trim();
}

async function runNativeShellDogfood(command: Extract<OppiCommand, { type: "tui" }>): Promise<number> {
  const started = Date.now();
  const strictBackgroundLifecycle = nativeShellDogfoodRequiresBackgroundLifecycle(command);
  const shellBin = resolveOppiShellBin();
  const serverBin = resolveOppiServerBin();
  if (!shellBin || !serverBin) {
    const payload: NativeShellDogfoodResult = {
      ok: false,
      shellBin,
      serverBin,
      durationMs: Date.now() - started,
      stdoutLineCount: 0,
      scenarios: [],
      diagnostics: [nativeShellUnavailableMessage(shellBin, serverBin)],
      strictBackgroundLifecycle,
    };
    if (command.json) console.log(JSON.stringify(payload, null, 2));
    else console.error(payload.diagnostics[0]);
    return 1;
  }

  const target = windowsCmdShim(shellBin, ["--mock", "--json", "--server", serverBin]);
  const child = spawn(target.command, target.args, {
    stdio: ["pipe", "pipe", "pipe"],
    env: nativeShellEnv(command, true),
    windowsHide: true,
  });

  let stdout = "";
  let stderr = "";
  let lineBuffer = "";
  let stdoutLineCount = 0;
  let phase: "permissions" | "approval" | "ask" | "background" | "readonly-setup" | "readonly" | "protected-setup" | "protected" | "network-setup" | "network" | "image" | "done" = "permissions";
  let approvalPaused = false;
  let approvalResolved = false;
  let approvalCompleted = false;
  let repoEditCompleted = false;
  let askPaused = false;
  let askResolved = false;
  let askTurnCompletions = 0;
  let queuedFollowUps = 0;
  let backgroundApprovalPaused = false;
  let backgroundTaskId: string | undefined;
  let backgroundListed = false;
  let backgroundRead = false;
  let backgroundKilled = false;
  let backgroundSandboxDenied = false;
  let backgroundSandboxDeniedError: string | undefined;
  let backgroundReadAttempts = 0;
  let backgroundListSent = false;
  let backgroundReadSent = false;
  let backgroundKillSent = false;
  let readOnlyDenied = false;
  let protectedDenied = false;
  let networkDenied = false;
  let missingImageFailedClosed = false;
  let finished = false;

  const send = (line: string) => {
    if (!child.stdin.destroyed && child.stdin.writable) child.stdin.write(`${line}\n`);
  };

  const finish = (exitCode: number | null, signal?: string | null): number => {
    if (finished) return 1;
    finished = true;
    clearTimeout(timeout);
    const backgroundLifecycleOk = Boolean(backgroundTaskId) && backgroundListed && backgroundRead && backgroundKilled;
    const scenarios: NativeShellDogfoodScenario[] = [
      {
        name: "repo-edit-approval",
        ok: approvalPaused && approvalResolved && approvalCompleted && repoEditCompleted,
        status: `${approvalPaused ? "paused" : "missing-pause"}->${approvalCompleted ? "completed" : "not-completed"}, repoEdit=${repoEditCompleted}`,
      },
      {
        name: "ask-user-follow-up-queue",
        ok: askPaused && askResolved && queuedFollowUps >= 2 && askTurnCompletions >= 3,
        status: `${askPaused ? "paused" : "missing-pause"}->${askTurnCompletions} completions, ${queuedFollowUps} queued`,
      },
      {
        name: "background-sandbox-execution",
        ok: backgroundLifecycleOk || (!strictBackgroundLifecycle && backgroundSandboxDenied),
        status: backgroundSandboxDenied
          ? "sandbox-unavailable-denied"
          : `${backgroundTaskId ? "started" : "not-started"}, list=${backgroundListed}, read=${backgroundRead}, kill=${backgroundKilled}`,
        diagnostics: strictBackgroundLifecycle && backgroundSandboxDenied && !backgroundLifecycleOk
          ? [
              "Strict background lifecycle dogfood requires a real sandboxed /background list/read/kill run; sandbox degradation is not enough for default promotion.",
              backgroundSandboxDeniedError ? `Sandbox denial: ${redactText(backgroundSandboxDeniedError)}` : undefined,
            ].filter((message): message is string => Boolean(message))
          : undefined,
      },
      {
        name: "failure-read-only-write",
        ok: readOnlyDenied,
        status: readOnlyDenied ? "denied" : "not-denied",
      },
      {
        name: "failure-protected-path",
        ok: protectedDenied,
        status: protectedDenied ? "denied" : "not-denied",
      },
      {
        name: "failure-network-disabled",
        ok: networkDenied,
        status: networkDenied ? "denied" : "not-denied",
      },
      {
        name: "failure-missing-image-backend",
        ok: missingImageFailedClosed,
        status: missingImageFailedClosed ? "failed-closed" : "not-observed",
      },
    ];
    const ok = scenarios.every((scenario) => scenario.ok) && exitCode === 0;
    const strictBackgroundFailure = strictBackgroundLifecycle && backgroundSandboxDenied && !backgroundLifecycleOk;
    const payload: NativeShellDogfoodResult = {
      ok,
      shellBin,
      serverBin,
      exitCode,
      durationMs: Date.now() - started,
      stdoutLineCount,
      scenarios,
      diagnostics: [
        ok
          ? strictBackgroundLifecycle
            ? "native shell dogfood completed repo-edit approval, ask_user/follow-up, strict background lifecycle, and core failure-mode scenarios"
            : "native shell dogfood completed repo-edit approval, ask_user/follow-up, background sandbox execution/degrade, and core failure-mode scenarios"
          : strictBackgroundFailure
            ? "strict background lifecycle dogfood requires a real sandboxed /background list/read/kill run; sandbox degradation is not sufficient for default promotion"
          : signal
            ? `oppi-shell exited by ${signal}`
            : `native shell dogfood did not complete all scenarios; exit=${exitCode}`,
      ],
      backgroundTaskId,
      strictBackgroundLifecycle,
      stderr: redactText(stderr).trim() || undefined,
    };
    if (command.json) console.log(JSON.stringify(payload, null, 2));
    else {
      console.log("OPPi native shell dogfood");
      for (const scenario of scenarios) console.log(`${scenario.ok ? "✓" : "✗"} ${scenario.name}: ${scenario.status ?? "n/a"}`);
      console.log(`${ok ? "✓" : "✗"} ${payload.diagnostics[0]}`);
      console.log(`shell: ${shellBin}`);
      console.log(`server: ${serverBin}`);
      if (payload.stderr) console.error(payload.stderr);
    }
    return ok ? 0 : 1;
  };

  const maybeRetryBackgroundRead = () => {
    if (!backgroundTaskId || backgroundRead || backgroundReadAttempts >= 6) return;
    backgroundReadAttempts += 1;
    setTimeout(() => send(`/background read ${backgroundTaskId} 30000`), 80);
  };

  const handleJsonLine = (value: any) => {
    if (value?.permissions) {
      if (phase === "permissions") {
        phase = "approval";
        send("oppi-dogfood-repo-edit");
        return;
      }
      if (phase === "readonly-setup") {
        phase = "readonly";
        send("oppi-dogfood-readonly-write");
        return;
      }
      if (phase === "protected-setup") {
        phase = "protected";
        send("oppi-dogfood-protected-path");
        return;
      }
      if (phase === "network-setup") {
        phase = "network";
        send("oppi-dogfood-network-disabled");
        return;
      }
    }
    if (typeof value?.shell === "string") {
      const text = value.shell;
      const queued = /queued follow-up #(\d+)/.exec(text);
      if (queued) queuedFollowUps = Math.max(queuedFollowUps, Number(queued[1]));
      return;
    }
    if (value?.background && backgroundTaskId && !backgroundListed) {
      const items = Array.isArray(value.background.items) ? value.background.items : [];
      backgroundListed = items.some((item: any) => item?.id === backgroundTaskId);
      if (backgroundListed && !backgroundReadSent) {
        backgroundReadSent = true;
        send(`/background read ${backgroundTaskId} 30000`);
      }
      return;
    }
    if (value?.backgroundRead && backgroundTaskId) {
      const output = String(value.backgroundRead.output ?? "");
      if (output.includes("oppi-background-dogfood")) {
        backgroundRead = true;
        if (!backgroundKillSent) {
          backgroundKillSent = true;
          send(`/background kill ${backgroundTaskId}`);
        }
      } else {
        maybeRetryBackgroundRead();
      }
      return;
    }
    if (value?.backgroundKill && backgroundTaskId) {
      backgroundKilled = value.backgroundKill.task?.id === backgroundTaskId;
      if (backgroundKilled) {
        phase = "readonly-setup";
        send("/permissions read-only");
      }
      return;
    }

    const kind = shellEventKind(value);
    if (kind === "approvalRequested") {
      if (phase === "approval") {
        approvalPaused = true;
        send("/approve");
      } else if (phase === "background") {
        backgroundApprovalPaused = true;
        send("/approve");
      } else if (phase === "readonly" || phase === "protected" || phase === "network") {
        send("/approve");
      }
      return;
    }
    if (kind === "approvalResolved" && phase === "approval") {
      approvalResolved = true;
      return;
    }
    if (kind === "askUserRequested" && phase === "ask") {
      askPaused = true;
      send("oppi-dogfood-follow-up-one");
      send("oppi-dogfood-follow-up-two");
      send("/answer safe");
      return;
    }
    if (kind === "askUserResolved" && phase === "ask") {
      askResolved = true;
      return;
    }
    if (kind === "toolCallCompleted") {
      const output = shellToolOutput(value);
      const status = shellToolStatus(value);
      const error = shellToolError(value);
      if (phase === "approval" && output?.includes("native-shell-dogfood.md")) repoEditCompleted = true;
      if (phase === "background") {
        const taskId = output ? extractBackgroundTaskId(output) : undefined;
        if (taskId) backgroundTaskId = taskId;
        if (status === "denied" && error?.includes("sandboxed background execution is unavailable")) {
          backgroundSandboxDenied = true;
          backgroundSandboxDeniedError = error;
        }
      }
      if (phase === "readonly" && (status === "denied" || error?.toLowerCase().includes("read-only"))) readOnlyDenied = true;
      if (phase === "protected" && (status === "denied" || error?.toLowerCase().includes("protected"))) protectedDenied = true;
      if (phase === "network" && (status === "denied" || error?.toLowerCase().includes("network"))) networkDenied = true;
      if (phase === "image" && status === "error" && error?.includes("image_gen requires")) missingImageFailedClosed = true;
      return;
    }
    if (kind === "turnCompleted") {
      if (phase === "approval" && approvalResolved) {
        approvalCompleted = true;
        phase = "ask";
        send("oppi-dogfood-ask-user");
      } else if (phase === "ask" && askResolved) {
        askTurnCompletions += 1;
        if (queuedFollowUps >= 2 && askTurnCompletions >= 3) {
          phase = "background";
          send("oppi-dogfood-background-start");
        }
      } else if (phase === "background" && backgroundApprovalPaused && backgroundSandboxDenied) {
        phase = "readonly-setup";
        send("/permissions read-only");
      } else if (phase === "background" && backgroundApprovalPaused && backgroundTaskId && !backgroundListSent) {
        backgroundListSent = true;
        send("/background list");
      } else if (phase === "readonly" && readOnlyDenied) {
        phase = "protected-setup";
        send("/permissions full-access");
      } else if (phase === "protected" && protectedDenied) {
        phase = "network-setup";
        send("/permissions default");
      } else if (phase === "network" && networkDenied) {
        phase = "image";
        send("oppi-dogfood-missing-image");
      } else if (phase === "image" && missingImageFailedClosed) {
        phase = "done";
        send("/exit");
      }
    }
  };

  const handleOutputChunk = (chunk: Buffer) => {
    const text = chunk.toString("utf8");
    stdout += text;
    lineBuffer += text;
    for (;;) {
      const newlineIndex = lineBuffer.search(/\r?\n/);
      if (newlineIndex < 0) break;
      const line = lineBuffer.slice(0, newlineIndex).trim();
      lineBuffer = lineBuffer.slice(lineBuffer[newlineIndex] === "\r" ? newlineIndex + 2 : newlineIndex + 1);
      if (!line) continue;
      stdoutLineCount += 1;
      try {
        handleJsonLine(JSON.parse(line));
      } catch {
        // Keep non-JSON lines for diagnostics, but the dogfood protocol is JSON-only.
      }
    }
  };

  const timeout = setTimeout(() => {
    if (!finished) {
      try { child.kill(); } catch {}
      finish(null, "timeout");
    }
  }, 30_000);

  child.stdout?.on("data", handleOutputChunk);
  child.stderr?.on("data", (chunk: Buffer) => { stderr += chunk.toString("utf8"); });
  const result = await new Promise<number>((resolveExit) => {
    child.on("error", (error: Error) => {
      stderr += error.message;
      resolveExit(finish(1));
    });
    child.on("exit", (code: number | null, signal: string | null) => {
      resolveExit(finish(code, signal));
    });
    send("/permissions full-access");
  });
  stdout = redactText(stdout);
  void stdout;
  return result;
}

async function runNativeShellCommand(command: Extract<OppiCommand, { type: "tui" }>): Promise<number> {
  if (command.subcommand === "smoke") return runNativeShellSmoke(command);
  if (command.subcommand === "dogfood") return runNativeShellDogfood(command);
  const shellBin = resolveOppiShellBin();
  const serverBin = resolveOppiServerBin();
  if (!shellBin || !serverBin) {
    const message = nativeShellUnavailableMessage(shellBin, serverBin);
    if (command.json) console.log(JSON.stringify({ ok: false, error: message, shellBin, serverBin }, null, 2));
    else console.error(message);
    return 1;
  }
  const shellArgs = withNativeShellDefaults(command.shellArgs, serverBin);
  const target = windowsCmdShim(shellBin, shellArgs);
  const child = spawn(target.command, target.args, {
    stdio: "inherit",
    env: nativeShellEnv(command),
  });
  return new Promise((resolveExit) => {
    child.on("error", (error: Error) => {
      console.error(`OPPi failed to start oppi-shell: ${error.message}`);
      resolveExit(1);
    });
    child.on("exit", (code: number | null, signal: string | null) => {
      if (signal) {
        const signalNumber = signal === "SIGINT" ? 130 : signal === "SIGTERM" ? 143 : 1;
        resolveExit(signalNumber);
      } else {
        resolveExit(code ?? 0);
      }
    });
  });
}

async function runResumeCommand(command: Extract<OppiCommand, { type: "resume" }>): Promise<number> {
  return runNativeShellCommand({
    type: "tui",
    subcommand: "run",
    experimental: false,
    json: command.json,
    shellArgs: command.shellArgs,
    agentDir: command.agentDir,
  });
}

async function runServerCommand(command: Extract<OppiCommand, { type: "server" }>): Promise<number> {
  if (!command.experimental) {
    const message = "OPPi Rust server is experimental. Re-run with `oppi server --stdio --experimental`.";
    if (command.json) console.log(JSON.stringify({ ok: false, error: message }, null, 2));
    else console.error(message);
    return 1;
  }

  const serverBin = resolveOppiServerBin();
  if (!serverBin) {
    const message = "Could not find oppi-server. Build it with `cargo build -p oppi-server` or set OPPI_SERVER_BIN.";
    if (command.json) console.log(JSON.stringify({ ok: false, error: message }, null, 2));
    else console.error(message);
    return 1;
  }

  const args = command.stdio ? ["--stdio"] : [];
  const child = spawn(serverBin, args, { stdio: "inherit", env: { ...process.env, OPPI_EXPERIMENTAL_RUNTIME: "1" } });
  return new Promise((resolveExit) => {
    child.on("error", (error: Error) => {
      console.error(`OPPi failed to start oppi-server: ${error.message}`);
      resolveExit(1);
    });
    child.on("exit", (code: number | null, signal: string | null) => {
      if (signal) {
        const signalNumber = signal === "SIGINT" ? 130 : signal === "SIGTERM" ? 143 : 1;
        resolveExit(signalNumber);
      } else {
        resolveExit(code ?? 0);
      }
    });
  });
}

async function runNativesCommand(command: Extract<OppiCommand, { type: "natives" }>): Promise<number> {
  let natives: any;
  try {
    natives = await importNativesModule();
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    if (command.subcommand === "status") {
      const status = unavailableNativeStatus(message);
      if (command.json) console.log(JSON.stringify(status, null, 2));
      else {
        console.log("OPPi natives");
        console.log(`package: ${status.packageName}@${status.packageVersion}`);
        console.log(`platform: ${status.platform}/${status.arch}`);
        console.log(`native module: not loaded`);
        console.log(`fallback reason: ${status.native.error}`);
        for (const recommendation of status.recommendations) console.log(`- ${recommendation}`);
      }
      return 0;
    }
    if (command.json) console.log(JSON.stringify({ ok: false, error: message }, null, 2));
    else console.error(`OPPi natives ${command.subcommand} failed: ${message}`);
    return 1;
  }

  try {
    if (command.subcommand === "benchmark") {
      const result = await natives.benchmarkSearch?.({ root: process.cwd() });
      if (!result) throw new Error("Installed @oppiai/natives does not expose benchmarkSearch().");
      if (command.json) console.log(JSON.stringify(result, null, 2));
      else {
        console.log("OPPi native benchmark");
        console.log(`root: ${result.root}`);
        console.log(`query: ${result.query}`);
        for (const run of result.runs ?? []) {
          const timing = typeof run.elapsedMs === "number" ? `${run.elapsedMs}ms` : "n/a";
          const matches = typeof run.matchCount === "number" ? `, matches ${run.matchCount}` : "";
          const status = run.available ? "✓" : "!";
          console.log(`${status} ${run.name}: ${timing}${matches}${run.error ? ` (${run.error})` : ""}`);
        }
        console.log(`${result.recommendation}: ${result.rationale}`);
      }
      return 0;
    }

    const status = natives.getNativeStatus?.();
    if (!status) throw new Error("Installed @oppiai/natives does not expose getNativeStatus().");
    if (command.json) console.log(JSON.stringify(status, null, 2));
    else {
      console.log("OPPi natives");
      console.log(`package: ${status.packageName}@${status.packageVersion}`);
      console.log(`platform: ${status.platform}/${status.arch}`);
      console.log(`native module: ${status.native?.available ? status.native.modulePath : "not loaded"}`);
      if (status.native?.error) console.log(`fallback reason: ${status.native.error}`);
      for (const recommendation of status.recommendations ?? []) console.log(`- ${recommendation}`);
    }
    return 0;
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    if (command.json) console.log(JSON.stringify({ ok: false, error: message }, null, 2));
    else console.error(`OPPi natives ${command.subcommand} failed: ${message}`);
    return 1;
  }
}

async function runMemCommand(command: Extract<OppiCommand, { type: "mem" }>): Promise<number> {
  try {
    if (command.subcommand === "install") {
      const result = await installHoppiPackage();
      if (command.json) console.log(JSON.stringify(result, null, 2));
      else if (result.ok) console.log(`Installed Hoppi backend: ${result.modulePath}`);
      else console.error(`Hoppi install failed: ${result.error}${result.output.trim() ? `\n${result.output.trim()}` : ""}`);
      return result.ok ? 0 : 1;
    }

    const hoppi = await importHoppiModule();
    const root = hoppi.getDefaultHoppiRoot?.() ?? join(homedir(), ".oppi", "hoppi");
    const backend = hoppi.createHoppiBackend?.({ root });
    if (!backend) throw new Error("Installed Hoppi module does not expose createHoppiBackend().");
    await backend.init();

    if (command.subcommand === "setup") {
      const payload = { ok: true, root, message: "Hoppi store initialized" };
      if (command.json) console.log(JSON.stringify(payload, null, 2));
      else console.log(`Hoppi store initialized: ${root}`);
      return 0;
    }

    if (command.subcommand === "dashboard" || command.subcommand === "open") {
      const handle = await backend.startDashboard?.({ project: { cwd: process.cwd() }, port: 0 });
      if (!handle) throw new Error("Hoppi backend does not expose startDashboard().");
      const payload = { ok: true, root, url: handle.url };
      if (command.json) console.log(JSON.stringify(payload, null, 2));
      else console.log(`Hoppi dashboard: ${handle.url}`);
      return 0;
    }

    const status = await backend.status({ cwd: process.cwd() });
    const payload = { ok: true, root, project: process.cwd(), status };
    if (command.json) console.log(JSON.stringify(payload, null, 2));
    else {
      console.log("Hoppi memory status");
      console.log(`root: ${root}`);
      console.log(`project: ${process.cwd()}`);
      console.log(`memories: ${status.memoryCount ?? 0}`);
      console.log(`pinned: ${status.pinnedCount ?? 0}`);
      console.log(`store: ${status.storePath ?? root}`);
    }
    return 0;
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    if (command.subcommand === "setup" && isHoppiMissingMessage(message)) {
      const instructions = hoppiSetupInstructions();
      if (command.json) console.log(JSON.stringify({ ok: false, needsInstall: true, error: message, instructions }, null, 2));
      else console.log(instructions.join("\n"));
      return 1;
    }
    if (command.json) console.log(JSON.stringify({ ok: false, error: message }, null, 2));
    else console.error(`OPPi mem ${command.subcommand} failed: ${message}\nRun \`oppi mem install\` to install Hoppi, or \`oppi mem setup\` for setup instructions.`);
    return 1;
  }
}

function helpText(): string {
  return `OPPi — opinionated Pi-powered coding agent

Usage:
  oppi [pi options] [@files...] [messages...]
  oppi doctor [--json]
  oppi update [--check] [--json]
  oppi mem status|setup|install|dashboard [--json]
  oppi natives status|benchmark [--json]
  oppi sandbox status|setup-windows [--yes|--dry-run] [--json]
  oppi resume [thread-id] [--json]
  oppi runtime-loop smoke [--json]
  oppi runtime-worker smoke [--json]
  oppi runtime-worker [run] <prompt> [--json] [--provider openai-compatible|openai-codex] [--model <id>] [--base-url <url>] [--api-key-env <name>] [--auto-approve] [--mock] [--memory|--no-memory] [--effort off|minimal|low|medium|high|xhigh] [--prompt-variant off|a|b|caveman]
  oppi tui [oppi-shell options]
  oppi tui smoke --mock [--json]
  oppi tui dogfood --mock [--json] [--require-background-lifecycle]
  oppi server --stdio --experimental
  oppi plugin list|add|install|enable|disable|remove|doctor [--json]
  oppi marketplace list|add|remove [--json]

OPPi options:
  --agent-dir <dir>       Use a specific OPPi/Pi agent dir for this run
  --with-pi-extensions    Allow normal Pi extension discovery in addition to OPPi
  --version, -v           Print OPPi CLI version
  --help, -h              Show this help

Environment:
  OPPI_UPDATE_CHECK=0      Disable the daily npm update banner
  OPPI_RUNTIME_LOOP_MODE   off | command | default-with-fallback (default)
  OPPI_RUNTIME_WORKER_PROVIDER openai-compatible (default) or openai-codex auth-store provider
  OPPI_RUNTIME_WORKER_MODEL Direct-worker model id (default: gpt-4.1-mini; gpt-5.4 for openai-codex)
  OPPI_RUNTIME_WORKER_API_KEY_ENV Direct-worker key env; must be OPPI_*_API_KEY, OPPI_OPENAI_API_KEY, or OPENAI_API_KEY
  OPPI_RUNTIME_WORKER_MEMORY auto | on | off for direct-worker Hoppi recall/write
  OPPI_RUNTIME_WORKER_EFFORT off | minimal | low | medium | high | xhigh for provider reasoning effort
  OPPI_SYSTEM_PROMPT_VARIANT off | a | b | caveman for direct-worker prompt A/B

Updates:
  oppi update             Install the latest @oppiai/cli from npm
  oppi update --check     Check npm and print the OPPi changelog link

Native helper examples:
  oppi natives status
  oppi natives benchmark --json
  oppi sandbox setup-windows --dry-run --json
  oppi sandbox setup-windows --yes

Experimental Rust runtime:
  cargo build -p oppi-server -p oppi-shell
  oppi runtime-loop smoke --json
  oppi runtime-worker smoke --json
  oppi runtime-worker "summarize this repository" --json
  oppi runtime-worker "summarize this repository" --no-memory
  oppi tui --experimental
  oppi tui smoke --mock --json
  oppi tui dogfood --mock --json
  oppi tui dogfood --mock --json --require-background-lifecycle
  oppi resume 019e0965-e833-7093-88ea-79a2baf0fc48
  oppi server --stdio --experimental

Plugin examples:
  oppi plugin add ./my-pi-package --local
  oppi plugin enable my-pi-package --yes
  oppi marketplace add ./catalog.json

Defaults:
  - loads the bundled/local @oppiai/pi-package
  - loads enabled OPPi plugins as additional Pi packages with -e
  - disables unrelated Pi extension discovery unless --with-pi-extensions is set
  - stores sessions/settings under OPPI_AGENT_DIR or ~/.oppi/agent
  - runs doctor probes against OPPI_AGENT_DIR or the current repo's .oppi/agent

Examples:
  oppi
  oppi "summarize this repository"
  oppi -p "Reply ok"
  OPPI_AGENT_DIR=/tmp/oppi-agent oppi doctor

All ordinary Pi flags not listed above are passed through unchanged.`;
}

export async function run(argv: string[]): Promise<number> {
  let command: OppiCommand;
  try {
    command = parseOppiArgs(argv);
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    return 1;
  }

  if (command.type === "help") {
    console.log(helpText());
    return 0;
  }
  if (command.type === "version") {
    console.log(cliVersion());
    return 0;
  }
  if (command.type === "doctor") {
    return printDiagnostics(collectDoctorDiagnostics({ agentDir: command.agentDir }), command.json);
  }
  if (command.type === "update") {
    return runUpdateCommand(command);
  }
  if (command.type === "mem") {
    return runMemCommand(command);
  }
  if (command.type === "natives") {
    return runNativesCommand(command);
  }
  if (command.type === "sandbox") {
    return runSandboxCommand(command);
  }
  if (command.type === "server") {
    return runServerCommand(command);
  }
  if (command.type === "tui") {
    return runNativeShellCommand(command);
  }
  if (command.type === "resume") {
    return runResumeCommand(command);
  }
  if (command.type === "runtime-loop") {
    return runRuntimeLoopCommand(command);
  }
  if (command.type === "runtime-worker") {
    return runRuntimeWorkerCommand(command);
  }
  if (command.type === "plugin") {
    return runPluginCommand(command);
  }
  if (command.type === "marketplace") {
    return runMarketplaceCommand(command);
  }
  return launchPi(command);
}

function canonicalMainPath(path: string): string {
  let resolved: string;
  try {
    resolved = realpathSync(path);
  } catch {
    resolved = resolve(path);
  }
  return process.platform === "win32" ? resolved.toLowerCase() : resolved;
}

export function isMain(argv1 = process.argv[1], filename = __filename): boolean {
  return argv1 ? canonicalMainPath(argv1) === canonicalMainPath(filename) : false;
}

if (isMain()) {
  run(process.argv.slice(2)).then((code) => {
    process.exitCode = code;
  }).catch((error) => {
    console.error(error instanceof Error ? error.stack || error.message : String(error));
    process.exitCode = 1;
  });
}
