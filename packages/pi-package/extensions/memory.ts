import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, isAbsolute, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { execFileSync, spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { complete } from "@mariozechner/pi-ai";
import type { ExtensionAPI, ExtensionCommandContext, ExtensionContext, Theme } from "@mariozechner/pi-coding-agent";
import { getAgentDir } from "@mariozechner/pi-coding-agent";
import type { Component } from "@mariozechner/pi-tui";
import { Key, matchesKey, truncateToWidth, visibleWidth } from "@mariozechner/pi-tui";
import {
  coerceIdleMinutes,
  coerceIdleThreshold,
  coerceSmartThreshold,
  readOppiCompactConfig,
  VALID_IDLE_MINUTES,
  VALID_IDLE_THRESHOLDS,
  VALID_SMART_THRESHOLDS,
  writeGlobalOppiCompactConfig,
  type OppiCompactConfig,
} from "./idle-compact";
import {
  clearSessionPermissionState,
  coerceTimeout,
  MODES as PERMISSION_MODES,
  permissionStatusText,
  publishMode,
  readPermissionConfig,
  showPermissionHistory,
  writeGlobalPermissionConfig,
  type PermissionConfig,
} from "./permissions";
import { askUserTimeoutLabel, ASK_USER_TIMEOUT_MINUTES, coerceAskUserTimeout, readAskUserConfig, writeGlobalAskUserConfig, type AskUserConfig } from "./ask-user";
import { currentThemeName, normalizeThemeName, openThemePreview, OPPI_THEMES, setOppiTheme, themeLabel } from "./themes";
import { FOOTER_CONFIG_CHANGED_EVENT, FOOTER_HELP_SHORTCUT_LABEL, FOOTER_USAGE_DISPLAY_VALUES, footerUsageDisplayLabel, readFooterConfig, writeFooterConfig, type FooterConfig } from "./usage";

type SyncConflictMode = "keep-both" | "keep-local" | "overwrite-local";
type SyncDirection = "pull" | "push" | "both";
type BackgroundSync = "off" | "15m" | "30m" | "60m";
type EncryptionMode = "none" | "passphrase";
type PassphraseSource = "env" | "file";
type MemoryAgentModel = "auto" | "claude" | "gpt";
type DeepMemoryModel = "auto" | "sonnet" | "gpt-5.5";
type HoppiInstallOffer = "ask" | "dismissed";

type MemorySyncConfig = {
  enabled: boolean;
  provider: "github";
  repoPath?: string;
  repoUrl?: string;
  pullOnStartup: boolean;
  pushOnExit: boolean;
  backgroundSync: BackgroundSync;
  encryption: EncryptionMode;
  passphraseSource: PassphraseSource;
  passphraseEnv: string;
  passphraseFile?: string;
  conflictMode: SyncConflictMode;
};

type MemoryConfig = {
  enabled: boolean;
  agentModel: MemoryAgentModel;
  deepModel: DeepMemoryModel;
  startupRecall: boolean;
  taskStartRecall: boolean;
  turnSummaries: boolean;
  idleConsolidation: boolean;
  dashboardPort: "auto" | number;
  hoppiInstallOffer: HoppiInstallOffer;
  sync: MemorySyncConfig;
};

type OppiSettingsFile = Record<string, any> & {
  oppi?: {
    memory?: Partial<MemoryConfig> & { sync?: Partial<MemorySyncConfig> };
  };
};

type HoppiProjectRef = { cwd: string; displayName?: string };

type HoppiBackend = {
  init(options?: unknown): Promise<void>;
  status(project: HoppiProjectRef): Promise<{ memoryCount: number; pinnedCount: number; storePath: string }>;
  startDashboard(input?: { project?: HoppiProjectRef; port?: number }): Promise<{ url: string; stop(): Promise<void> }>;
  remember?(input: {
    project: HoppiProjectRef;
    content: string;
    tags?: string[];
    layer?: "buffer" | "episodic" | "semantic" | "trace";
    pinned?: boolean;
    confidence?: "verified" | "observed" | "inferred" | "stale";
    source?: string;
    sourceSessionId?: string | null;
    parents?: string[];
  }): Promise<{ id: string }>;
  recall?(input: {
    project: HoppiProjectRef;
    query: string;
    budget?: number;
    limit?: number;
    includeSuperseded?: boolean;
  }): Promise<{ memories: unknown[]; contextMarkdown: string; omittedReason?: string }>;
  list?(input: {
    project: HoppiProjectRef;
    includeSuperseded?: boolean;
    pinnedOnly?: boolean;
    limit?: number;
  }): Promise<MemoryMaintenanceEntry[]>;
  forget?(id: string): Promise<void>;
  buildStartupContext?(input: {
    project: HoppiProjectRef;
    maxMemories?: number;
    includePinned?: boolean;
  }): Promise<{ memoryCount: number; contextMarkdown: string; humanSummary: string }>;
  consolidate?(input?: { project?: HoppiProjectRef; dryRun?: boolean; budget?: number }): Promise<unknown>;
};

type HoppiModule = {
  getDefaultHoppiRoot?: () => string;
  createHoppiBackend?: (options?: { root?: string }) => HoppiBackend;
  syncGitRepository?: (root: string, repoPath: string, options?: Record<string, unknown>) => {
    imported: number;
    skipped: number;
    overwritten: number;
    keptBoth: number;
    conflicts: unknown[];
    exported: number;
    encrypted: boolean;
    pulled: boolean;
    committed: boolean;
    pushed: boolean;
    repoPath: string;
    direction: SyncDirection;
    embeddingsRebuildRecommended?: boolean;
  };
  rebuildEmbeddings?: (root: string, options?: Record<string, unknown>) => Promise<{ available: boolean; rebuilt: number; error?: string }>;
};

type SettingsAction = "close" | "install-hoppi" | "setup-sync" | "sync-now" | "pull-now" | "push-now" | "open-dashboard" | "permission-history" | "permission-clear" | "permission-reviewer-model" | "permission-status" | "theme-preview";

const HOPPI_PACKAGE_NAME = "@oppiai/hoppi-memory";
const HOPPI_LEGACY_PACKAGE_NAME = "hoppi-memory";
const HOPPI_PACKAGE_SPEC = `${HOPPI_PACKAGE_NAME}@^0.1.0`;
const DEFAULT_PASSPHRASE_ENV = "OPPI_HOPPI_SYNC_PASSPHRASE";
const DEFAULT_SYNC_REPO_NAME = "hoppi-memories";
const SETTINGS_TABS = ["⚙️ General", "🧭 Footer", "🧠 Memory", "🗜️ Compaction", "🔐 Permissions", "🎨 Theme"] as const;
const BACKGROUND_VALUES: BackgroundSync[] = ["off", "15m", "30m", "60m"];
const CONFLICT_VALUES: SyncConflictMode[] = ["keep-both", "keep-local", "overwrite-local"];
const AGENT_MODEL_VALUES: MemoryAgentModel[] = ["auto", "claude", "gpt"];
const DEEP_MODEL_VALUES: DeepMemoryModel[] = ["auto", "sonnet", "gpt-5.5"];
const HOPPI_INSTALL_OFFER_VALUES: HoppiInstallOffer[] = ["ask", "dismissed"];
const PERMISSION_TIMEOUT_SECONDS = [5, 15, 30, 45, 60, 90, 120, 180] as const;
const MEMORY_CONTEXT_TYPE = "oppi-memory-context";
const MEMORY_DISTILLER_MAX_PROMPT_CHARS = 4_000;
const MEMORY_IDLE_CHECK_MS = 60_000;
const MEMORY_IDLE_CONSOLIDATE_AFTER_MS = 90_000;

let dashboardHandle: { url: string; stop(): Promise<void> } | undefined;
let startupSyncStarted = false;
let hoppiInstallPromise: Promise<HoppiInstallResult> | undefined;

type HoppiInstallResult =
  | { ok: true; modulePath: string; output: string }
  | { ok: false; error: string; output: string };

function globalSettingsPath(): string {
  const explicit = process.env.OPPI_SETTINGS_PATH?.trim();
  return explicit ? resolveUserPath(explicit) : join(getAgentDir(), "settings.json");
}

function readJson(path: string): OppiSettingsFile {
  try {
    if (!existsSync(path)) return {};
    return JSON.parse(readFileSync(path, "utf8"));
  } catch {
    return {};
  }
}

function writeJson(path: string, data: OppiSettingsFile): void {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, `${JSON.stringify(data, null, 2)}\n`, "utf8");
}

function defaultRepoPath(): string {
  return join(getAgentDir(), "oppi", "hoppi-sync", DEFAULT_SYNC_REPO_NAME);
}

function defaultPassphraseFile(): string {
  return join(getAgentDir(), "oppi", "hoppi-sync", "passphrase");
}

function normalizeDashboardPort(value: unknown): "auto" | number {
  if (value === "auto" || value === undefined || value === null) return "auto";
  const numeric = Number(value);
  return Number.isInteger(numeric) && numeric > 0 && numeric < 65536 ? numeric : "auto";
}

function normalizeBackground(value: unknown): BackgroundSync {
  return BACKGROUND_VALUES.includes(value as BackgroundSync) ? value as BackgroundSync : "off";
}

function normalizeConflictMode(value: unknown): SyncConflictMode {
  return CONFLICT_VALUES.includes(value as SyncConflictMode) ? value as SyncConflictMode : "keep-both";
}

function normalizeAgentModel(value: unknown): MemoryAgentModel {
  return AGENT_MODEL_VALUES.includes(value as MemoryAgentModel) ? value as MemoryAgentModel : "auto";
}

function normalizeDeepModel(value: unknown): DeepMemoryModel {
  return DEEP_MODEL_VALUES.includes(value as DeepMemoryModel) ? value as DeepMemoryModel : "auto";
}

function normalizeHoppiInstallOffer(value: unknown): HoppiInstallOffer {
  return value === "dismissed" ? "dismissed" : "ask";
}

function normalizeSyncConfig(value: Partial<MemorySyncConfig> | undefined): MemorySyncConfig {
  return {
    enabled: value?.enabled === true,
    provider: "github",
    repoPath: typeof value?.repoPath === "string" && value.repoPath.trim() ? value.repoPath : undefined,
    repoUrl: typeof value?.repoUrl === "string" && value.repoUrl.trim() ? value.repoUrl : undefined,
    pullOnStartup: value?.pullOnStartup !== false,
    pushOnExit: value?.pushOnExit !== false,
    backgroundSync: normalizeBackground(value?.backgroundSync),
    encryption: value?.encryption === "passphrase" ? "passphrase" : "none",
    passphraseSource: value?.passphraseSource === "file" ? "file" : "env",
    passphraseEnv: typeof value?.passphraseEnv === "string" && value.passphraseEnv.trim() ? value.passphraseEnv : DEFAULT_PASSPHRASE_ENV,
    passphraseFile: typeof value?.passphraseFile === "string" && value.passphraseFile.trim() ? value.passphraseFile : undefined,
    conflictMode: normalizeConflictMode(value?.conflictMode),
  };
}

function normalizeMemoryConfig(value: Partial<MemoryConfig> | undefined): MemoryConfig {
  return {
    enabled: value?.enabled !== false,
    agentModel: normalizeAgentModel(value?.agentModel),
    deepModel: normalizeDeepModel(value?.deepModel),
    startupRecall: value?.startupRecall !== false,
    taskStartRecall: value?.taskStartRecall !== false,
    turnSummaries: value?.turnSummaries !== false,
    idleConsolidation: value?.idleConsolidation !== false,
    dashboardPort: normalizeDashboardPort(value?.dashboardPort),
    hoppiInstallOffer: normalizeHoppiInstallOffer(value?.hoppiInstallOffer),
    sync: normalizeSyncConfig(value?.sync),
  };
}

function readMemoryConfig(_cwd: string): MemoryConfig {
  return normalizeMemoryConfig(readJson(globalSettingsPath()).oppi?.memory);
}

function writeMemoryConfig(config: MemoryConfig): void {
  const path = globalSettingsPath();
  const data = readJson(path);
  data.oppi = data.oppi ?? {};
  const existing = data.oppi.memory ?? {};
  data.oppi.memory = normalizeMemoryConfig({
    ...existing,
    ...config,
    sync: { ...existing.sync, ...config.sync },
  });
  writeJson(path, data);
}

function expandHome(value: string): string {
  if (value === "~") return homedir();
  if (value.startsWith("~/") || value.startsWith("~\\")) return join(homedir(), value.slice(2));
  return value;
}

function resolveUserPath(value: string): string {
  const expanded = expandHome(value.trim());
  return isAbsolute(expanded) ? expanded : resolve(process.cwd(), expanded);
}

function oppiHome(): string {
  const explicit = process.env.OPPI_HOME?.trim();
  return explicit ? resolveUserPath(explicit) : join(homedir(), ".oppi");
}

function managedPackagesDir(): string {
  return join(oppiHome(), "packages");
}

function packageRootFromNodeModules(nodeModulesDir: string, packageName: string): string {
  return join(nodeModulesDir, ...packageName.split("/"));
}

function managedHoppiModulePath(packageName = HOPPI_PACKAGE_NAME): string {
  return join(packageRootFromNodeModules(join(managedPackagesDir(), "node_modules"), packageName), "dist", "index.js");
}

function ensureManagedPackageRoot(): string {
  const root = managedPackagesDir();
  mkdirSync(root, { recursive: true });
  const packageJson = join(root, "package.json");
  if (!existsSync(packageJson)) {
    writeFileSync(packageJson, `${JSON.stringify({ private: true, name: "oppi-managed-packages", description: "OPPi managed optional packages." }, null, 2)}\n`, "utf8");
  }
  return root;
}

function npmSpawnCommand(args: string[]): { command: string; args: string[] } {
  // Directly spawning npm.cmd can throw EINVAL under some Windows terminals.
  // Route through cmd.exe explicitly instead of relying on shell lookup.
  if (process.platform === "win32") return { command: "cmd.exe", args: ["/d", "/s", "/c", "npm", ...args] };
  return { command: "npm", args };
}

function formatPath(value: string | undefined): string {
  if (!value) return "not configured";
  const home = homedir();
  return value.startsWith(home) ? `~${value.slice(home.length)}` : value;
}

function extensionDir(): string {
  return dirname(fileURLToPath(import.meta.url));
}

async function loadHoppi(): Promise<HoppiModule> {
  const candidates = [
    process.env.OPPI_HOPPI_MODULE,
    managedHoppiModulePath(HOPPI_PACKAGE_NAME),
    managedHoppiModulePath(HOPPI_LEGACY_PACKAGE_NAME),
    join(extensionDir(), "..", "..", "..", "..", "hoppi-memory", "dist", "index.js"),
  ].filter((candidate): candidate is string => Boolean(candidate));

  for (const candidate of candidates) {
    try {
      const resolved = isAbsolute(candidate) ? candidate : resolve(candidate);
      if (existsSync(resolved)) return await import(pathToFileURL(resolved).href) as HoppiModule;
    } catch {
      // Try the next candidate.
    }
  }

  const packageNames = [HOPPI_PACKAGE_NAME, HOPPI_LEGACY_PACKAGE_NAME];
  let lastError: unknown;
  for (const packageName of packageNames) {
    try {
      return await import(packageName) as HoppiModule;
    } catch (error) {
      lastError = error;
    }
  }

  throw new Error(`Hoppi package is not available. Install ${HOPPI_PACKAGE_NAME} from /settings:oppi → Memory, run \`oppi mem install\`, or set OPPI_HOPPI_MODULE. (${lastError instanceof Error ? lastError.message : String(lastError)})`);
}

function isHoppiMissingError(error: unknown): boolean {
  const message = error instanceof Error ? error.message : String(error);
  return message.includes("Hoppi package is not available")
    || message.includes("Hoppi module not found")
    || message.includes(`Cannot find module '${HOPPI_PACKAGE_NAME}'`)
    || message.includes(`Cannot find package "${HOPPI_PACKAGE_NAME}"`)
    || message.includes("Cannot find module 'hoppi-memory'")
    || message.includes('Cannot find package "hoppi-memory"');
}

function hoppiSetupMessage(): string {
  return `Hoppi memory needs setup. Install ${HOPPI_PACKAGE_NAME} from /settings:oppi → Memory, run \`oppi mem install\`, or set OPPI_HOPPI_MODULE to a built Hoppi dist/index.js.`;
}

async function isHoppiMissing(): Promise<boolean> {
  try {
    await loadHoppi();
    return false;
  } catch (error) {
    return isHoppiMissingError(error);
  }
}

async function installHoppiPackage(): Promise<HoppiInstallResult> {
  if (hoppiInstallPromise) return hoppiInstallPromise;
  hoppiInstallPromise = new Promise<HoppiInstallResult>((resolveInstall) => {
    const root = ensureManagedPackageRoot();
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
      env: { ...process.env, npm_config_loglevel: process.env.npm_config_loglevel ?? "warn" },
    });
    child.stdout?.on("data", append);
    child.stderr?.on("data", append);
    child.on("error", (error) => resolveInstall({ ok: false, error: error.message, output }));
    child.on("close", (code) => {
      const modulePath = managedHoppiModulePath(HOPPI_PACKAGE_NAME);
      if (code === 0 && existsSync(modulePath)) resolveInstall({ ok: true, modulePath, output });
      else resolveInstall({ ok: false, error: `npm install exited with code ${code ?? "unknown"}`, output });
    });
  }).finally(() => {
    hoppiInstallPromise = undefined;
  });
  return hoppiInstallPromise;
}

async function installHoppiFromUi(ctx: ExtensionContext): Promise<boolean> {
  publishStatus(ctx, "mem:install…");
  if (ctx.hasUI) ctx.ui.notify(`Installing ${HOPPI_PACKAGE_NAME} into ${formatPath(managedPackagesDir())}…`, "info");
  const result = await installHoppiPackage();
  if (result.ok === false) {
    publishStatus(ctx, "mem:setup");
    const tail = result.output.trim() ? `\n${truncateText(result.output, 1_200)}` : "";
    if (ctx.hasUI) ctx.ui.notify(`Hoppi install failed: ${result.error}${tail}`, "warning");
    return false;
  }

  const config = readMemoryConfig(ctx.cwd);
  writeMemoryConfig({ ...config, enabled: true, hoppiInstallOffer: "ask" });
  publishStatus(ctx, "mem:on");
  if (ctx.hasUI) ctx.ui.notify(`Installed Hoppi backend: ${formatPath(result.modulePath)}.`, "info");
  return true;
}

async function maybeOfferHoppiInstall(ctx: ExtensionContext): Promise<boolean> {
  const config = readMemoryConfig(ctx.cwd);
  if (!ctx.hasUI || !config.enabled || config.hoppiInstallOffer === "dismissed") return false;
  try {
    await loadHoppi();
    return false;
  } catch (error) {
    if (!isHoppiMissingError(error)) return false;
  }

  publishStatus(ctx, "mem:setup");
  const accepted = await ctx.ui.confirm(
    "Install Hoppi memory?",
    `OPPi memory is enabled, but ${HOPPI_PACKAGE_NAME} is not installed. Install it now with npm into ${formatPath(managedPackagesDir())}? Choose No to keep working; you can install later from /settings:oppi → Memory.`,
  );
  if (!accepted) {
    writeMemoryConfig({ ...config, hoppiInstallOffer: "dismissed" });
    publishStatus(ctx, "mem:setup");
    ctx.ui.notify("Okay — you can install Hoppi later from /settings:oppi → Memory.", "info");
    return false;
  }
  return installHoppiFromUi(ctx);
}

function hoppiRoot(hoppi: HoppiModule): string {
  if (hoppi.getDefaultHoppiRoot) return hoppi.getDefaultHoppiRoot();
  return process.env.HOPPI_HOME ? resolve(process.env.HOPPI_HOME) : join(homedir(), ".oppi", "hoppi");
}

function syncPassphrase(config: MemorySyncConfig): string | undefined {
  if (config.encryption !== "passphrase") return undefined;
  if (config.passphraseSource === "file") {
    const file = config.passphraseFile ? resolveUserPath(config.passphraseFile) : defaultPassphraseFile();
    if (!existsSync(file)) throw new Error(`Encrypted sync passphrase file is missing: ${file}`);
    const passphrase = readFileSync(file, "utf8").trim();
    if (!passphrase) throw new Error(`Encrypted sync passphrase file is empty: ${file}`);
    return passphrase;
  }
  const passphrase = process.env[config.passphraseEnv]?.trim();
  if (!passphrase) throw new Error(`Encrypted sync needs ${config.passphraseEnv} to be set.`);
  return passphrase;
}

function publishStatus(ctx: ExtensionContext, status: string | undefined): void {
  if (!ctx.hasUI) return;
  ctx.ui.setStatus("oppi.memory", status);
}

function syncSummary(direction: SyncDirection, result: ReturnType<NonNullable<HoppiModule["syncGitRepository"]>>): string {
  const parts = [
    direction,
    `pulled ${result.imported}`,
    result.exported ? `exported ${result.exported}` : undefined,
    result.committed ? "committed" : undefined,
    result.pushed ? "pushed" : undefined,
    result.conflicts.length ? `${result.conflicts.length} conflicts` : undefined,
  ].filter(Boolean);
  return parts.join(" · ");
}

async function runSync(ctx: ExtensionContext, direction: SyncDirection, reason: string, notify = false): Promise<void> {
  const config = readMemoryConfig(ctx.cwd);
  if (!config.enabled || !config.sync.enabled) {
    publishStatus(ctx, config.enabled ? "mem:sync off" : "mem:off");
    return;
  }
  const repoPath = resolveUserPath(config.sync.repoPath ?? defaultRepoPath());
  publishStatus(ctx, direction === "pull" ? "mem:pull…" : direction === "push" ? "mem:push…" : "mem:sync…");

  try {
    const hoppi = await loadHoppi();
    if (!hoppi.syncGitRepository) throw new Error("Installed Hoppi package does not expose syncGitRepository().");
    const result = hoppi.syncGitRepository(hoppiRoot(hoppi), repoPath, {
      direction,
      remote: config.sync.repoUrl,
      passphrase: syncPassphrase(config.sync),
      conflictMode: config.sync.conflictMode,
      message: `Hoppi memory sync (${reason})`,
    });
    if ((direction === "pull" || direction === "both") && result.embeddingsRebuildRecommended && hoppi.rebuildEmbeddings) {
      publishStatus(ctx, "mem:embed…");
      const rebuilt = await hoppi.rebuildEmbeddings(hoppiRoot(hoppi));
      if (rebuilt.error && ctx.hasUI) ctx.ui.notify(`Hoppi embedding rebuild skipped: ${rebuilt.error}`, "warning");
    }
    const summary = syncSummary(direction, result);
    publishStatus(ctx, result.conflicts.length ? `mem:conflicts ${result.conflicts.length}` : "mem:synced");
    if (notify && ctx.hasUI) ctx.ui.notify(`Hoppi sync ${summary}`, result.conflicts.length ? "warning" : "info");
  } catch (error) {
    publishStatus(ctx, "mem:sync error");
    if (ctx.hasUI) ctx.ui.notify(`Hoppi sync skipped: ${error instanceof Error ? error.message : String(error)}`, "warning");
  }
}

function openUrl(url: string): void {
  if (process.platform === "win32") {
    execFileSync("cmd", ["/c", "start", "", url], { stdio: "ignore" });
  } else if (process.platform === "darwin") {
    spawn("open", [url], { detached: true, stdio: "ignore" }).unref();
  } else {
    spawn("xdg-open", [url], { detached: true, stdio: "ignore" }).unref();
  }
}

async function openDashboard(ctx: ExtensionContext): Promise<string> {
  const config = readMemoryConfig(ctx.cwd);
  const hoppi = await loadHoppi();
  if (!hoppi.createHoppiBackend) throw new Error("Installed Hoppi package does not expose createHoppiBackend().");
  if (!dashboardHandle) {
    const backend = hoppi.createHoppiBackend({ root: hoppiRoot(hoppi) });
    await backend.init();
    dashboardHandle = await backend.startDashboard({
      project: { cwd: ctx.cwd },
      port: config.dashboardPort === "auto" ? 0 : config.dashboardPort,
    });
  }
  return dashboardHandle.url;
}

async function openMemoryDashboard(ctx: ExtensionCommandContext): Promise<void> {
  try {
    const url = await openDashboard(ctx);
    try {
      openUrl(url);
      ctx.ui.notify(`Hoppi dashboard: ${url}`, "info");
    } catch {
      ctx.ui.notify(`Hoppi dashboard: ${url}\nFallback: ${url.replace("hoppi.localhost", "localhost")}`, "info");
    }
  } catch (error) {
    ctx.ui.notify(isHoppiMissingError(error) ? hoppiSetupMessage() : `Could not open Hoppi dashboard: ${error instanceof Error ? error.message : String(error)}`, isHoppiMissingError(error) ? "info" : "warning");
  }
}

type AgentMessageLike = Record<string, any>;

type DistilledTurnMemory = {
  remember?: boolean;
  request?: string;
  completed?: string[];
  learned?: string[];
  decisions?: string[];
  next?: string[];
  files?: string[];
  tags?: string[];
};

type ModelTurnSummary =
  | { kind: "memory"; content: string; tags: string[] }
  | { kind: "skip" };

const MEMORY_DISTILLER_PROMPT = `You are OPPi's memory distiller. You observe one completed coding-agent turn and decide whether to save a compact memory for future sessions.

Record durable technical signal only:
- shipped changes, bug fixes, configuration/docs updates, tests or validation outcomes
- decisions, trade-offs, gotchas, root causes, user preferences, and concrete next steps
- specific file paths or components when they help future recall

Skip routine chatter, empty status checks, raw terminal dumps, package installs with no finding, and repeated information already obvious from the turn.

Return only JSON matching this shape:
{
  "remember": true | false,
  "request": "short user request",
  "completed": ["what changed or was delivered"],
  "learned": ["durable finding, root cause, gotcha, or behavior"],
  "decisions": ["decision or trade-off"],
  "next": ["active next step"],
  "files": ["path/or/component"],
  "tags": ["short-topic"]
}

Rules:
- Max 3 items in each array, max 18 words per item.
- Use standalone statements; avoid pronouns without a referent.
- Do not describe the act of summarizing or observing.
- If remember is false, all arrays may be empty.`;

function projectRef(ctx: ExtensionContext): HoppiProjectRef {
  return { cwd: ctx.cwd };
}

function sessionSourceId(ctx: ExtensionContext): string | null {
  try {
    return ctx.sessionManager.getSessionId() || ctx.sessionManager.getSessionFile() || null;
  } catch {
    return null;
  }
}

function compactWhitespace(value: string): string {
  return value.replace(/\s+/g, " ").trim();
}

function truncateText(value: string, max: number): string {
  const compact = compactWhitespace(value);
  return compact.length > max ? `${compact.slice(0, Math.max(0, max - 1)).trimEnd()}…` : compact;
}

function textFromContent(content: unknown, options: { includeToolCalls?: boolean } = {}): string {
  if (typeof content === "string") return content;
  if (!Array.isArray(content)) return "";
  return content.map((part: any) => {
    if (!part || typeof part !== "object") return "";
    if (part.type === "text" && typeof part.text === "string") return part.text;
    if (part.type === "toolCall") return options.includeToolCalls ? `[tool:${part.toolName ?? part.name ?? "unknown"}]` : "";
    if (part.type === "image") return "[image]";
    if (typeof part.text === "string") return part.text;
    return "";
  }).filter(Boolean).join("\n");
}

function isMemoryContextMessage(message: AgentMessageLike): boolean {
  const text = textFromContent(message.content);
  return message.customType === MEMORY_CONTEXT_TYPE
    || text.includes("OPPi memory context")
    || text.includes("relevant Hoppi memory")
    || text.includes("Memory is advisory");
}

function messageText(message: AgentMessageLike): string {
  if (isMemoryContextMessage(message)) return "";
  return textFromContent(message.content);
}

function isLowSignalMemoryText(text: string): boolean {
  const normalized = compactWhitespace(text).toLowerCase();
  return normalized.length === 0
    || normalized === "cd"
    || normalized.startsWith("current working directory:")
    || /^([a-z]:\\|\\\\|\/)[^\s]+$/i.test(normalized)
    || /^\[tool:[^\]]+\](\s+\[tool:[^\]]+\])*$/.test(normalized);
}

function messagesByRole(messages: readonly AgentMessageLike[], role: string): string[] {
  return messages
    .filter((message) => message.role === role)
    .map(messageText)
    .map((text) => truncateText(text, 900))
    .filter((text) => text.length > 0 && !isLowSignalMemoryText(text));
}

function uniqueRecentTexts(texts: readonly string[], limit: number): string[] {
  const seen = new Set<string>();
  const selected: string[] = [];
  for (const text of [...texts].reverse()) {
    const key = compactWhitespace(text).toLowerCase().slice(0, 180);
    if (seen.has(key)) continue;
    seen.add(key);
    selected.push(text);
    if (selected.length >= limit) break;
  }
  return selected.reverse();
}

function memorySnippet(text: string | undefined, max: number): string {
  if (!text) return "";
  return truncateText(text.replace(/\x1b\[[0-9;]*m/g, ""), max);
}

function buildTurnSummary(messages: readonly AgentMessageLike[]): string | undefined {
  const userTexts = messagesByRole(messages, "user");
  const assistantTexts = messagesByRole(messages, "assistant");
  if (userTexts.length === 0 || assistantTexts.length === 0) return undefined;

  const user = memorySnippet(userTexts.at(-1), 360);
  const assistant = memorySnippet(assistantTexts.at(-1), 520);
  if (`${user}${assistant}`.length < 80) return undefined;

  const lines = [
    `Turn summary (${new Date().toISOString()})`,
    user ? `User asked: ${user}` : undefined,
    assistant ? `Assistant outcome: ${assistant}` : undefined,
  ].filter(Boolean) as string[];

  return lines.join("\n");
}

function buildSessionRecap(ctx: ExtensionContext): string | undefined {
  const branch = ctx.sessionManager.getBranch();
  const messages = branch
    .filter((entry: any) => entry.type === "message" && entry.message)
    .map((entry: any) => entry.message as AgentMessageLike)
    .filter((message) => !isMemoryContextMessage(message));
  const userTexts = uniqueRecentTexts(messagesByRole(messages, "user"), 4);
  const assistantTexts = uniqueRecentTexts(messagesByRole(messages, "assistant"), 4);
  if (userTexts.length === 0 && assistantTexts.length === 0) return undefined;

  const lines = [
    `Session recap (${new Date().toISOString()})`,
    "Recent user goals:",
    ...userTexts.map((text) => `- ${memorySnippet(text, 180)}`),
    "Recent assistant outcomes:",
    ...assistantTexts.map((text) => `- ${memorySnippet(text, 220)}`),
  ];
  const recap = lines.join("\n");
  return recap.length >= 90 ? recap : undefined;
}

function parseJsonObject(text: string): unknown {
  const trimmed = text.trim().replace(/^```(?:json)?\s*/i, "").replace(/```$/i, "").trim();
  try {
    return JSON.parse(trimmed);
  } catch {
    const start = trimmed.indexOf("{");
    const end = trimmed.lastIndexOf("}");
    if (start >= 0 && end > start) return JSON.parse(trimmed.slice(start, end + 1));
    throw new Error("No JSON object found");
  }
}

function stringArray(value: unknown, limit: number, maxChars: number): string[] {
  if (!Array.isArray(value)) return [];
  return value
    .filter((item): item is string => typeof item === "string")
    .map((item) => memorySnippet(item, maxChars))
    .filter(Boolean)
    .slice(0, limit);
}

function distillerTags(memory: DistilledTurnMemory): string[] {
  return stringArray(memory.tags, 4, 40)
    .map((tag) => tag.toLowerCase().replace(/[^a-z0-9:_-]+/g, "-").replace(/^-+|-+$/g, ""))
    .filter((tag) => tag.length >= 2 && !tag.startsWith("project:"));
}

function distilledTurnContent(memory: DistilledTurnMemory): string | undefined {
  if (memory.remember === false) return undefined;
  const request = memorySnippet(memory.request, 220);
  const completed = stringArray(memory.completed, 3, 180);
  const learned = stringArray(memory.learned, 3, 180);
  const decisions = stringArray(memory.decisions, 2, 180);
  const next = stringArray(memory.next, 2, 160);
  const files = stringArray(memory.files, 6, 120);
  if (!request && completed.length === 0 && learned.length === 0 && decisions.length === 0 && next.length === 0) return undefined;

  const lines = [
    `Turn memory (${new Date().toISOString()})`,
    request ? `Request: ${request}` : undefined,
    completed.length ? "Completed:" : undefined,
    ...completed.map((item) => `- ${item}`),
    learned.length ? "Learned:" : undefined,
    ...learned.map((item) => `- ${item}`),
    decisions.length ? "Decisions:" : undefined,
    ...decisions.map((item) => `- ${item}`),
    next.length ? "Next:" : undefined,
    ...next.map((item) => `- ${item}`),
    files.length ? `Files: ${files.join(", ")}` : undefined,
  ].filter(Boolean) as string[];

  const content = lines.join("\n");
  return content.length >= 80 ? content : undefined;
}

function shouldUseModelDistiller(config: MemoryConfig): boolean {
  const explicit = process.env.OPPI_MEMORY_DISTILL_AI?.trim().toLowerCase();
  if (explicit === "1" || explicit === "true" || explicit === "yes") return true;
  if (explicit === "0" || explicit === "false" || explicit === "no") return false;
  return config.agentModel !== "auto";
}

async function buildModelTurnSummary(messages: readonly AgentMessageLike[], ctx: ExtensionContext, config: MemoryConfig): Promise<ModelTurnSummary | undefined> {
  if (!shouldUseModelDistiller(config)) return undefined;
  const model = ctx.model;
  if (!model) return undefined;
  const auth = await ctx.modelRegistry.getApiKeyAndHeaders(model).catch(() => undefined);
  if (!auth?.ok || !auth.apiKey) return undefined;

  const userTexts = messagesByRole(messages, "user");
  const assistantTexts = messagesByRole(messages, "assistant");
  const user = memorySnippet(userTexts.at(-1), 1_200);
  const assistant = memorySnippet(assistantTexts.at(-1), 2_000);
  if (!user || !assistant) return undefined;

  const prompt = [
    MEMORY_DISTILLER_PROMPT,
    "",
    "Turn input:",
    JSON.stringify({ user, assistant }, null, 2),
  ].join("\n");

  const response = await complete(
    model,
    {
      messages: [
        {
          role: "user" as const,
          content: [{ type: "text" as const, text: truncateText(prompt, MEMORY_DISTILLER_MAX_PROMPT_CHARS) }],
          timestamp: Date.now(),
        },
      ],
    },
    {
      apiKey: auth.apiKey,
      headers: auth.headers,
      maxTokens: 420,
      reasoningEffort: "minimal",
      signal: ctx.signal,
    },
  );

  const raw = response.content
    .filter((part): part is { type: "text"; text: string } => part.type === "text")
    .map((part) => part.text)
    .join("\n");
  const parsed = parseJsonObject(raw);
  if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) return undefined;
  const distilled = parsed as DistilledTurnMemory;
  if (distilled.remember === false) return { kind: "skip" };
  const content = distilledTurnContent(distilled);
  return content ? { kind: "memory", content, tags: distillerTags(distilled) } : undefined;
}

type MemoryMaintenanceEntry = {
  id: string;
  content: string;
  tags?: string[];
  pinned?: boolean;
  starred?: boolean;
  layer?: string;
  confidence?: string;
  source?: string;
  created?: string;
  created_at?: string;
  updated_at?: string;
  strength?: number;
  superseded_by?: string | null;
  parents?: string[];
};

type MemoryMaintenanceConsolidation = {
  title?: string;
  content: string;
  sourceIds: string[];
  tags: string[];
};

type MemoryMaintenancePlan = {
  deleteIds: string[];
  deleteReasons: Map<string, string>;
  consolidate: MemoryMaintenanceConsolidation[];
  notes: string[];
};

type MemoryMaintenanceArgs = {
  dryRun: boolean;
  yes: boolean;
  help: boolean;
  limit: number;
};

type MaintenanceModelCandidate = {
  model: any;
  label: string;
  reason: string;
  rank: number;
};

type MaintenancePlanResult = {
  plan: MemoryMaintenancePlan;
  modelLabel: string;
  modelReason: string;
  attempts: string[];
};

type MemoryMaintenanceApplyResult = {
  created: string[];
  deleted: string[];
  skippedPinned: string[];
  skippedVerified: string[];
  consolidateResult?: unknown;
};

type DreamProjectState = {
  dreamCount: number;
  lastDreamAt?: string;
  lastSummary?: string;
};

type DreamStateFile = {
  projects?: Record<string, DreamProjectState>;
};

type DreamStageResult = {
  created: string[];
  deleted: string[];
  skippedPinned: string[];
  skippedVerified: string[];
  notes: string[];
};

type DreamRunResult = {
  beforeCount: number;
  afterCount?: number;
  dreamCount: number;
  stage1: DreamStageResult;
  stage2?: DreamStageResult & { modelLabel: string; modelReason: string };
  stage3?: (DreamStageResult & { modelLabel: string; modelReason: string }) | { skipped: true; reason: string };
};

const MEMORY_MAINTENANCE_DEFAULT_LIMIT = 220;
const MEMORY_MAINTENANCE_HARD_LIMIT = 400;
const MEMORY_MAINTENANCE_CONTENT_CHARS = 520;
const MEMORY_MAINTENANCE_MAX_PROMPT_CHARS = 90_000;
const MEMORY_MAINTENANCE_MAX_OUTPUT_TOKENS = 4_000;
const MEMORY_DEEP_MAINTENANCE_MAX_PROMPT_CHARS = 220_000;
const MEMORY_DEEP_MAINTENANCE_MAX_OUTPUT_TOKENS = 8_000;
const MEMORY_DREAM_TRIGGER_COUNT = 60;
const MEMORY_DREAM_TARGET_COUNT = 50;
const MEMORY_DREAM_DEEP_EVERY = 5;
const MEMORY_DREAM_MIN_INTERVAL_MS = 12 * 60 * 60 * 1_000;
const MEMORY_MAINTENANCE_MODEL_SPECS = [
  "openai-codex/gpt-5.4-mini",
  "openai/gpt-5.4-mini",
  "azure-openai-responses/gpt-5.4-mini",
  "opencode/gpt-5.4-mini",
];
const MEMORY_DEEP_GPT_MODEL_SPECS = [
  "openai-codex/gpt-5.5",
  "openai/gpt-5.5",
  "azure-openai-responses/gpt-5.5",
  "opencode/gpt-5.5",
];
const MEMORY_DEEP_SONNET_MODEL_SPECS = [
  "meridian/claude-sonnet-4-6",
  "anthropic/claude-sonnet-4-6",
  "anthropic/claude-sonnet-4-5-20250929-v1:0",
  "openrouter/anthropic/claude-sonnet-4-6",
];

const MEMORY_MAINTENANCE_PROMPT = `You are OPPi's Stage 2 Hoppi dream worker: age-aware memory curation after duplicate cleanup.

Goal:
- preserve durable decisions, user preferences, gotchas, architecture facts, release/package status, completed outcomes, and current next steps
- slowly compress older nitty-gritty code details into durable "why/what changed" facts
- keep recent implementation details when they are still likely useful for active work
- delete only irrelevant/noisy memories or memories fully replaced by a better consolidation

Age-aware policy:
- Newer memories may keep changed files, exact commands, and implementation details when useful.
- Middle-aged memories should keep decisions, changed areas, and gotchas; compress tool logs and step-by-step chatter.
- Older memories should keep architecture intent, product decisions, user preferences, recurring gotchas, release state, and roadmap context; drop low-level code churn unless it still explains current architecture.
- If two memories conflict about the same subsystem, prefer newer/current facts, but preserve the older fact only if it explains historical context.

Hard safety rules:
- Pinned/starred records are protected: never put them in delete, and only cite them as sourceIds when the source should remain.
- Never delete verified memories unless they are sourceIds of a better verified consolidation you create in this response.
- Auto-delete only obvious duplicates, noise, stale inferred summaries, or memories fully superseded by your consolidation.
- Do not invent facts. If uncertain, keep.

Return only one valid JSON object. The first character must be an opening brace and the last character must be a closing brace. No Markdown, no commentary, no code fences:
{
  "delete": [{"id": "mem_or_sem_id", "reason": "short reason"}],
  "consolidate": [{"title": "short title", "content": "durable memory text", "sourceIds": ["id1", "id2"], "tags": ["short-topic"]}],
  "notes": ["short operational note"]
}

Limits:
- At most 80 delete items.
- At most 12 consolidated memories.
- Each consolidated memory should be 2-6 bullets or <=120 words.
- Use only IDs from the input.`;

const MEMORY_DEEP_MAINTENANCE_PROMPT = `You are OPPi's Stage 3 deep Hoppi dream worker. This is an occasional whole-store reconciliation pass using a larger model.

Goal:
- load the whole project memory set mentally and eliminate superseded old information
- when two memories describe the same system and conflict, newer memories win unless the older memory captures important historical rationale
- produce a small set of durable verified semantic consolidations that preserve architecture decisions, product direction, user preferences, release state, recurring gotchas, and current roadmap
- remove older implementation details once they are no longer needed, especially repeated file lists, tool logs, and stale turn/session summaries

Hard safety rules:
- Pinned/starred records are protected: never put them in delete.
- Never delete verified memories unless they are sourceIds of a better verified consolidation you create in this response.
- Do not delete merely because a memory is old. Delete because it is superseded, duplicated, noisy, or replaced.
- Newer facts win over older facts for the same subsystem.
- Do not invent facts. If uncertain, keep.

Return only one valid JSON object. The first character must be an opening brace and the last character must be a closing brace. No Markdown, no commentary, no code fences:
{
  "delete": [{"id": "mem_or_sem_id", "reason": "superseded by newer memory/consolidation"}],
  "consolidate": [{"title": "short title", "content": "durable memory text", "sourceIds": ["id1", "id2"], "tags": ["short-topic"]}],
  "notes": ["short operational note"]
}

Limits:
- At most 120 delete items.
- At most 16 consolidated memories.
- Each consolidated memory should be 2-7 bullets or <=160 words.
- Use only IDs from the input.`;

function parseMemoryMaintenanceArgs(args: string | undefined): MemoryMaintenanceArgs {
  const tokens = (args ?? "").trim().split(/\s+/).filter(Boolean);
  const lower = tokens.map((token) => token.toLowerCase());
  const help = lower.includes("help") || lower.includes("--help") || lower.includes("-h");
  const dryRun = lower.includes("dry-run") || lower.includes("dryrun") || lower.includes("preview") || lower.includes("--dry-run");
  const yes = lower.includes("--yes") || lower.includes("-y");
  let limit = MEMORY_MAINTENANCE_DEFAULT_LIMIT;
  for (let i = 0; i < tokens.length; i += 1) {
    const token = tokens[i]!;
    const value = token.startsWith("--limit=") ? token.slice("--limit=".length) : token === "--limit" ? tokens[i + 1] : undefined;
    if (value !== undefined) {
      const parsed = Number(value);
      if (Number.isInteger(parsed) && parsed > 0) limit = Math.min(parsed, MEMORY_MAINTENANCE_HARD_LIMIT);
      if (token === "--limit") i += 1;
    }
  }
  return { dryRun, yes, help, limit };
}

function modelLabelFor(model: any): string {
  return `${model?.provider ?? "unknown"}/${model?.id ?? model?.name ?? "unknown"}`;
}

function findModelSpec(ctx: ExtensionContext, spec: string): any | undefined {
  const [provider, ...idParts] = spec.split("/");
  const id = idParts.join("/");
  if (!provider || !id) return undefined;
  try {
    return ctx.modelRegistry.find(provider, id) as any;
  } catch {
    return undefined;
  }
}

function modelCandidatesFromSpecs(ctx: ExtensionContext, specs: readonly string[], reason: string): MaintenanceModelCandidate[] {
  return specs.flatMap((spec, index) => {
    const model = findModelSpec(ctx, spec);
    return model ? [{ model, label: modelLabelFor(model), reason, rank: index }] : [];
  });
}

function preferredMaintenanceModels(ctx: ExtensionContext): MaintenanceModelCandidate[] {
  return modelCandidatesFromSpecs(ctx, MEMORY_MAINTENANCE_MODEL_SPECS, "default GPT-5.4 mini maintenance model");
}

function maintenanceModelCandidates(ctx: ExtensionContext, _config: MemoryConfig): MaintenanceModelCandidate[] {
  const override = process.env.OPPI_MEMORY_MAINTENANCE_MODEL?.trim();
  if (override) {
    const model = findModelSpec(ctx, override);
    return model ? [{ model, label: modelLabelFor(model), reason: `OPPI_MEMORY_MAINTENANCE_MODEL=${override}`, rank: 0 }] : [];
  }

  return preferredMaintenanceModels(ctx);
}

function deepMaintenanceModelCandidates(ctx: ExtensionContext, config: MemoryConfig): MaintenanceModelCandidate[] {
  const override = process.env.OPPI_MEMORY_DEEP_MAINTENANCE_MODEL?.trim() || process.env.OPPI_MEMORY_MAINTENANCE_DEEP_MODEL?.trim();
  if (override) {
    const model = findModelSpec(ctx, override);
    return model ? [{ model, label: modelLabelFor(model), reason: `OPPI_MEMORY_DEEP_MAINTENANCE_MODEL=${override}`, rank: 0 }] : [];
  }
  if (config.deepModel === "gpt-5.5") return modelCandidatesFromSpecs(ctx, MEMORY_DEEP_GPT_MODEL_SPECS, "deep dream GPT-5.5 model");
  if (config.deepModel === "sonnet") return modelCandidatesFromSpecs(ctx, MEMORY_DEEP_SONNET_MODEL_SPECS, "deep dream Sonnet model");
  return [];
}

function stripProjectTags(tags: readonly string[] | undefined): string[] {
  return (tags ?? [])
    .filter((tag) => !tag.startsWith("project:") && !tag.startsWith("project-alias:") && !tag.startsWith("project-name:"))
    .slice(0, 14);
}

function compactMaintenanceContent(content: string): string {
  return content
    .replace(/\x1b\[[0-9;]*m/g, "")
    .replace(/\[tool:[^\]]+\]/g, " ")
    .replace(/^\s*Tools used:.*$/gim, " ")
    .replace(/\n{3,}/g, "\n\n")
    .replace(/[ \t]+/g, " ")
    .trim();
}

function memoryCreated(entry: MemoryMaintenanceEntry): string | undefined {
  return entry.created ?? entry.created_at;
}

function memoryCreatedMs(entry: MemoryMaintenanceEntry): number {
  const raw = memoryCreated(entry);
  const parsed = raw ? Date.parse(raw) : NaN;
  return Number.isFinite(parsed) ? parsed : 0;
}

function isProtectedMemory(entry: MemoryMaintenanceEntry): boolean {
  return Boolean(entry.pinned || entry.starred);
}

function isVerifiedMemory(entry: MemoryMaintenanceEntry): boolean {
  return entry.confidence === "verified" || entry.source === "oppi:memory-maintenance";
}

function maintenanceRecord(entry: MemoryMaintenanceEntry, contentChars = MEMORY_MAINTENANCE_CONTENT_CHARS): Record<string, unknown> {
  return {
    id: entry.id,
    protected: isProtectedMemory(entry),
    layer: entry.layer,
    confidence: entry.confidence,
    source: entry.source,
    strength: typeof entry.strength === "number" ? Number(entry.strength.toFixed(3)) : undefined,
    created: memoryCreated(entry)?.slice(0, 10),
    superseded: Boolean(entry.superseded_by),
    tags: stripProjectTags(entry.tags),
    content: memorySnippet(compactMaintenanceContent(entry.content), contentChars),
  };
}

function buildMemoryMaintenancePrompt(ctx: ExtensionContext, memories: readonly MemoryMaintenanceEntry[], maxChars = MEMORY_MAINTENANCE_MAX_PROMPT_CHARS, contentChars = MEMORY_MAINTENANCE_CONTENT_CHARS): string {
  const records = memories.map((entry) => maintenanceRecord(entry, contentChars));
  const prompt = [
    "Project:",
    JSON.stringify({ cwd: ctx.cwd, recordCount: records.length, now: new Date().toISOString() }, null, 2),
    "",
    "Memory records:",
    JSON.stringify(records, null, 2),
  ].join("\n");
  return prompt.length > maxChars
    ? `${prompt.slice(0, maxChars)}\n\n[Input truncated by OPPi; operate only on complete IDs visible above.]`
    : prompt;
}

function isObject(value: unknown): value is Record<string, unknown> {
  return Boolean(value) && typeof value === "object" && !Array.isArray(value);
}

function sanitizedMaintenanceTags(value: unknown): string[] {
  return stringArray(value, 6, 40)
    .map((tag) => tag.toLowerCase().replace(/[^a-z0-9:_-]+/g, "-").replace(/^-+|-+$/g, ""))
    .filter((tag) => tag.length >= 2 && !tag.startsWith("project:"));
}

function trimMaintenanceText(value: string, max: number): string {
  const cleaned = value
    .replace(/\x1b\[[0-9;]*m/g, "")
    .replace(/\r\n/g, "\n")
    .replace(/[ \t]+/g, " ")
    .replace(/\n{3,}/g, "\n\n")
    .trim();
  return cleaned.length > max ? `${cleaned.slice(0, Math.max(0, max - 1)).trimEnd()}…` : cleaned;
}

function maintenanceResponseText(response: any): string {
  return (response?.content ?? [])
    .flatMap((part: any) => {
      if (!part || typeof part !== "object") return [];
      if (part.type === "text" && typeof part.text === "string") return [part.text];
      return [];
    })
    .join("\n")
    .trim();
}

function maintenanceResponseDiagnostic(response: any, raw: string): string {
  const parts = [
    response?.stopReason ? `stop=${response.stopReason}` : undefined,
    response?.errorMessage ? `error=${memorySnippet(String(response.errorMessage), 180)}` : undefined,
    Array.isArray(response?.content) ? `content=${response.content.map((part: any) => part?.type ?? "unknown").join(",") || "empty"}` : undefined,
    raw ? `raw=${memorySnippet(raw, 220)}` : "raw=empty",
  ].filter(Boolean);
  return parts.length ? ` (${parts.join("; ")})` : "";
}

function maintenanceJsonPayload(payload: unknown): unknown | undefined {
  if (!isObject(payload)) return undefined;
  const next: Record<string, unknown> = { ...payload };
  const text = isObject(next.text) ? { ...next.text } : {};
  text.verbosity = "low";
  text.format = { type: "json_object" };
  next.text = text;
  delete next.reasoning;
  return next;
}

function normalizeMaintenancePlan(parsed: unknown, memories: readonly MemoryMaintenanceEntry[]): MemoryMaintenancePlan {
  if (!isObject(parsed)) throw new Error("Maintenance model returned non-object JSON.");
  const known = new Map(memories.map((entry) => [entry.id, entry]));
  const deleteReasons = new Map<string, string>();
  const deleteIds: string[] = [];
  const addDelete = (id: unknown, reason: unknown) => {
    if (typeof id !== "string" || !known.has(id) || deleteReasons.has(id)) return;
    const entry = known.get(id)!;
    if (entry.pinned || entry.starred) return;
    deleteReasons.set(id, typeof reason === "string" ? memorySnippet(reason, 120) : "low-value or replaced by consolidation");
    deleteIds.push(id);
  };

  const rawDelete = Array.isArray(parsed.delete) ? parsed.delete : Array.isArray(parsed.deleteIds) ? parsed.deleteIds : [];
  for (const item of rawDelete.slice(0, 100)) {
    if (typeof item === "string") addDelete(item, "model selected for deletion");
    else if (isObject(item)) addDelete(item.id, item.reason);
  }

  const rawConsolidate = Array.isArray(parsed.consolidate) ? parsed.consolidate : Array.isArray(parsed.consolidated) ? parsed.consolidated : [];
  const consolidate: MemoryMaintenanceConsolidation[] = [];
  for (const item of rawConsolidate.slice(0, 12)) {
    if (!isObject(item)) continue;
    const sourceIds = stringArray(item.sourceIds ?? item.sources, 24, 80).filter((id) => known.has(id));
    const content = typeof item.content === "string" ? trimMaintenanceText(item.content, 1_200) : "";
    if (sourceIds.length < 2 || content.length < 40) continue;
    consolidate.push({
      title: typeof item.title === "string" ? memorySnippet(item.title, 80) : undefined,
      content,
      sourceIds,
      tags: sanitizedMaintenanceTags(item.tags),
    });
  }

  return {
    deleteIds,
    deleteReasons,
    consolidate,
    notes: stringArray(parsed.notes, 5, 160),
  };
}

function memoryMaintenanceDeletionSet(plan: MemoryMaintenancePlan, memories: readonly MemoryMaintenanceEntry[]): { ids: string[]; skippedPinned: string[]; skippedVerified: string[] } {
  const known = new Map(memories.map((entry) => [entry.id, entry]));
  const consolidatedSources = new Set<string>();
  const ids = new Set(plan.deleteIds);
  for (const item of plan.consolidate) {
    for (const id of item.sourceIds) {
      ids.add(id);
      consolidatedSources.add(id);
    }
  }
  const allowed: string[] = [];
  const skippedPinned: string[] = [];
  const skippedVerified: string[] = [];
  for (const id of ids) {
    const entry = known.get(id);
    if (!entry) continue;
    if (isProtectedMemory(entry)) skippedPinned.push(id);
    else if (isVerifiedMemory(entry) && !consolidatedSources.has(id)) skippedVerified.push(id);
    else allowed.push(id);
  }
  return { ids: allowed, skippedPinned, skippedVerified };
}

async function buildMaintenancePlanWithCandidates(input: {
  ctx: ExtensionContext;
  memories: readonly MemoryMaintenanceEntry[];
  candidates: MaintenanceModelCandidate[];
  systemPrompt: string;
  maxPromptChars: number;
  maxOutputTokens: number;
  contentChars?: number;
}): Promise<MaintenancePlanResult> {
  const { ctx, memories, candidates, systemPrompt, maxPromptChars, maxOutputTokens, contentChars } = input;
  if (candidates.length === 0) throw new Error("No memory maintenance model candidates are configured.");

  const prompt = buildMemoryMaintenancePrompt(ctx, memories, maxPromptChars, contentChars);
  const attempts: string[] = [];
  for (const candidate of candidates) {
    const auth = await ctx.modelRegistry.getApiKeyAndHeaders(candidate.model).catch((error: unknown) => {
      attempts.push(`${candidate.label}: auth failed (${error instanceof Error ? error.message : String(error)})`);
      return undefined;
    });
    if (!auth?.ok || !auth.apiKey) {
      attempts.push(`${candidate.label}: no configured auth`);
      continue;
    }

    const modes: Array<{ label: string; jsonMode: boolean }> = [
      { label: "json-mode", jsonMode: true },
      { label: "plain", jsonMode: false },
    ];

    for (const mode of modes) {
      try {
        const response = await complete(
          candidate.model,
          {
            systemPrompt,
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
            maxTokens: maxOutputTokens,
            textVerbosity: "low",
            ...(mode.jsonMode ? { onPayload: maintenanceJsonPayload } : {}),
            signal: ctx.signal,
          },
        );
        const raw = maintenanceResponseText(response);
        try {
          const parsed = parseJsonObject(raw);
          return {
            plan: normalizeMaintenancePlan(parsed, memories),
            modelLabel: candidate.label,
            modelReason: `${candidate.reason}${mode.jsonMode ? " · JSON mode" : ""}`,
            attempts,
          };
        } catch (error) {
          throw new Error(`${error instanceof Error ? error.message : String(error)}${maintenanceResponseDiagnostic(response, raw)}`);
        }
      } catch (error) {
        attempts.push(`${candidate.label} ${mode.label}: ${error instanceof Error ? error.message : String(error)}`);
      }
    }
  }

  throw new Error(`No memory maintenance model succeeded. Tried: ${attempts.join("; ")}`);
}

async function buildMaintenancePlanWithModel(ctx: ExtensionContext, config: MemoryConfig, memories: readonly MemoryMaintenanceEntry[]): Promise<MaintenancePlanResult> {
  const candidates = maintenanceModelCandidates(ctx, config);
  if (candidates.length === 0) {
    const override = process.env.OPPI_MEMORY_MAINTENANCE_MODEL?.trim();
    throw new Error(override
      ? `Configured memory maintenance model was not found: ${override}`
      : "No GPT-5.4 mini maintenance model is configured. Configure openai-codex/openai gpt-5.4-mini, or set OPPI_MEMORY_MAINTENANCE_MODEL=provider/model.");
  }
  return buildMaintenancePlanWithCandidates({
    ctx,
    memories,
    candidates,
    systemPrompt: MEMORY_MAINTENANCE_PROMPT,
    maxPromptChars: MEMORY_MAINTENANCE_MAX_PROMPT_CHARS,
    maxOutputTokens: MEMORY_MAINTENANCE_MAX_OUTPUT_TOKENS,
  });
}

async function buildDeepMaintenancePlanWithModel(ctx: ExtensionContext, config: MemoryConfig, memories: readonly MemoryMaintenanceEntry[]): Promise<MaintenancePlanResult | undefined> {
  const candidates = deepMaintenanceModelCandidates(ctx, config);
  if (candidates.length === 0) return undefined;
  return buildMaintenancePlanWithCandidates({
    ctx,
    memories,
    candidates,
    systemPrompt: MEMORY_DEEP_MAINTENANCE_PROMPT,
    maxPromptChars: MEMORY_DEEP_MAINTENANCE_MAX_PROMPT_CHARS,
    maxOutputTokens: MEMORY_DEEP_MAINTENANCE_MAX_OUTPUT_TOKENS,
    contentChars: 1_800,
  });
}

function consolidationContent(item: MemoryMaintenanceConsolidation): string {
  const title = item.title ? `Memory maintenance consolidation: ${item.title}` : `Memory maintenance consolidation (${new Date().toISOString()})`;
  const sources = item.sourceIds.length ? `\n\nSource memories: ${item.sourceIds.join(", ")}` : "";
  return `${title}\n\n${item.content}${sources}`;
}

async function applyMemoryMaintenancePlan(backend: HoppiBackend, project: HoppiProjectRef, plan: MemoryMaintenancePlan, memories: readonly MemoryMaintenanceEntry[]): Promise<MemoryMaintenanceApplyResult> {
  if (!backend.remember || !backend.forget) throw new Error("Installed Hoppi package does not expose remember()/forget().");
  const created: string[] = [];
  for (const item of plan.consolidate) {
    const memory = await backend.remember({
      project,
      content: consolidationContent(item),
      tags: Array.from(new Set(["memory-maintenance", "consolidated", ...item.tags])),
      layer: "semantic",
      confidence: "verified",
      source: "oppi:memory-maintenance",
      sourceSessionId: null,
      parents: item.sourceIds,
    });
    created.push(memory.id);
  }

  const deletion = memoryMaintenanceDeletionSet(plan, memories);
  const deleted: string[] = [];
  for (const id of deletion.ids) {
    await backend.forget(id);
    deleted.push(id);
  }

  let consolidateResult: unknown;
  if (backend.consolidate) consolidateResult = await backend.consolidate({ project, dryRun: false, budget: 1_500 });
  return { created, deleted, skippedPinned: deletion.skippedPinned, skippedVerified: deletion.skippedVerified, consolidateResult };
}

function numericResult(value: unknown, key: string): number | undefined {
  return isObject(value) && typeof value[key] === "number" ? value[key] as number : undefined;
}

function formatMaintenanceSummary(input: {
  dryRun: boolean;
  scanned: number;
  total: number;
  limit: number;
  modelLabel: string;
  modelReason: string;
  plan: MemoryMaintenancePlan;
  memories: readonly MemoryMaintenanceEntry[];
  apply?: MemoryMaintenanceApplyResult;
}): string {
  const deletion = memoryMaintenanceDeletionSet(input.plan, input.memories);
  const plannedDeleteIds = input.apply?.deleted ?? deletion.ids;
  const lines = [
    input.dryRun ? "🧠 Memory maintenance dry run" : "🧠 Memory maintenance complete",
    `Model used: ${input.modelLabel} (${input.modelReason})`,
    `Scanned: ${input.scanned}${input.total > input.scanned ? ` of ${input.total} (limit ${input.limit})` : ""}`,
    input.dryRun
      ? `Planned: create ${input.plan.consolidate.length}, delete ${plannedDeleteIds.length}${deletion.skippedVerified.length ? `, verified skipped ${deletion.skippedVerified.length}` : ""}`
      : `Applied: created ${input.apply?.created.length ?? 0}, deleted ${input.apply?.deleted.length ?? 0}, protected skipped ${input.apply?.skippedPinned.length ?? 0}${input.apply?.skippedVerified.length ? `, verified skipped ${input.apply.skippedVerified.length}` : ""}`,
  ];

  const semanticCreated = numericResult(input.apply?.consolidateResult, "semanticCreated");
  const dagCreated = numericResult(input.apply?.consolidateResult, "dagSummariesCreated");
  const removed = numericResult(input.apply?.consolidateResult, "removed");
  if (!input.dryRun && (semanticCreated !== undefined || dagCreated !== undefined || removed !== undefined)) {
    lines.push(`Hoppi consolidate: semantic ${semanticCreated ?? 0}, DAG ${dagCreated ?? 0}, removed ${removed ?? 0}`);
  }

  if (input.plan.consolidate.length) {
    lines.push("", "Consolidations:");
    for (const item of input.plan.consolidate.slice(0, 6)) {
      lines.push(`- ${item.title ?? "untitled"} (${item.sourceIds.length} sources)`);
    }
    if (input.plan.consolidate.length > 6) lines.push(`- …${input.plan.consolidate.length - 6} more`);
  }

  if (plannedDeleteIds.length) {
    lines.push("", input.dryRun ? "Delete preview:" : "Deleted/replaced:");
    for (const id of plannedDeleteIds.slice(0, 10)) {
      const reason = input.plan.deleteReasons.get(id);
      lines.push(`- ${id}${reason ? ` — ${reason}` : ""}`);
    }
    if (plannedDeleteIds.length > 10) lines.push(`- …${plannedDeleteIds.length - 10} more`);
  }

  if (input.plan.notes.length) {
    lines.push("", "Notes:", ...input.plan.notes.map((note) => `- ${note}`));
  }
  if (input.dryRun) lines.push("", "Run `/memory-maintenance apply --yes` to apply this periodically without prompts.");

  return lines.join("\n").slice(0, 3_600);
}

function memoryMaintenanceHelp(): string {
  return `Usage: /memory-maintenance [dry-run|apply] [--yes] [--limit N]\n\nTemporary Hoppi cleanup pass. It defaults to GPT-5.4 mini and does not try Claude/Meridian. The final summary always shows the model ultimately used. Override only with OPPI_MEMORY_MAINTENANCE_MODEL=provider/model.\n\nExamples:\n- /memory-maintenance dry-run\n- /memory-maintenance apply\n- /memory-maintenance apply --yes`;
}

async function runMemoryMaintenance(ctx: ExtensionCommandContext, args: string | undefined): Promise<void> {
  const parsed = parseMemoryMaintenanceArgs(args);
  if (parsed.help) {
    ctx.ui.notify(memoryMaintenanceHelp(), "info");
    return;
  }

  const config = readMemoryConfig(ctx.cwd);
  if (!config.enabled) {
    publishStatus(ctx, "mem:off");
    ctx.ui.notify("Memory is off. Enable it in /settings:oppi → Memory before running maintenance.", "info");
    return;
  }

  publishStatus(ctx, "mem:maint…");
  try {
    const hoppi = await loadHoppi();
    if (!hoppi.createHoppiBackend) throw new Error("Installed Hoppi package does not expose createHoppiBackend().");
    const backend = hoppi.createHoppiBackend({ root: hoppiRoot(hoppi) });
    await backend.init();
    if (!backend.list || !backend.remember || !backend.forget) throw new Error("Installed Hoppi package does not expose maintenance APIs (list/remember/forget). Update Hoppi.");

    const project = projectRef(ctx);
    const totalMemories = await backend.list({ project, includeSuperseded: true });
    const memories = totalMemories.slice(0, parsed.limit);
    if (memories.length === 0) {
      publishStatus(ctx, "mem:on");
      ctx.ui.notify("No Hoppi memories found for this project.", "info");
      return;
    }

    ctx.ui.notify(`Planning memory maintenance for ${memories.length} memories with GPT-5.4 mini…`, "info");
    const planResult = await buildMaintenancePlanWithModel(ctx, config, memories);
    const plannedDelete = memoryMaintenanceDeletionSet(planResult.plan, memories);
    const preview = formatMaintenanceSummary({
      dryRun: true,
      scanned: memories.length,
      total: totalMemories.length,
      limit: parsed.limit,
      modelLabel: planResult.modelLabel,
      modelReason: planResult.modelReason,
      plan: planResult.plan,
      memories,
    });

    if (parsed.dryRun) {
      publishStatus(ctx, "mem:maint dry");
      ctx.ui.notify(preview, "info");
      return;
    }

    if (!parsed.yes) {
      const accepted = await ctx.ui.confirm(
        "Apply memory maintenance?",
        `Model: ${planResult.modelLabel}\nCreate ${planResult.plan.consolidate.length} consolidated memories and delete/replace ${plannedDelete.ids.length} memories. Protected skipped: ${plannedDelete.skippedPinned.length}. Verified skipped: ${plannedDelete.skippedVerified.length}.`,
      );
      if (!accepted) {
        publishStatus(ctx, "mem:on");
        ctx.ui.notify(`Memory maintenance cancelled.\n\n${preview}`, "info");
        return;
      }
    }

    const applied = await applyMemoryMaintenancePlan(backend, project, planResult.plan, memories);
    const status = await backend.status(project).catch(() => undefined);
    publishStatus(ctx, status ? `mem:${status.memoryCount}` : "mem:maint done");
    ctx.ui.notify(formatMaintenanceSummary({
      dryRun: false,
      scanned: memories.length,
      total: totalMemories.length,
      limit: parsed.limit,
      modelLabel: planResult.modelLabel,
      modelReason: planResult.modelReason,
      plan: planResult.plan,
      memories,
      apply: applied,
    }), "info");
  } catch (error) {
    publishStatus(ctx, isHoppiMissingError(error) ? "mem:setup" : "mem:error");
    ctx.ui.notify(isHoppiMissingError(error) ? hoppiSetupMessage() : `Memory maintenance failed: ${error instanceof Error ? error.message : String(error)}`, isHoppiMissingError(error) ? "info" : "warning");
  }
}

function emptyDreamStageResult(): DreamStageResult {
  return { created: [], deleted: [], skippedPinned: [], skippedVerified: [], notes: [] };
}

function dreamStatePath(): string {
  return join(getAgentDir(), "oppi", "memory-dream-state.json");
}

function readDreamState(): DreamStateFile {
  try {
    const path = dreamStatePath();
    if (!existsSync(path)) return { projects: {} };
    const parsed = JSON.parse(readFileSync(path, "utf8"));
    return isObject(parsed) ? parsed as DreamStateFile : { projects: {} };
  } catch {
    return { projects: {} };
  }
}

function writeDreamState(state: DreamStateFile): void {
  const path = dreamStatePath();
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, `${JSON.stringify({ projects: state.projects ?? {} }, null, 2)}\n`, "utf8");
}

function dreamProjectKey(project: HoppiProjectRef): string {
  return createHash("sha256").update(resolve(project.cwd).toLowerCase()).digest("hex").slice(0, 16);
}

function readDreamProjectState(project: HoppiProjectRef): DreamProjectState {
  const state = readDreamState();
  return state.projects?.[dreamProjectKey(project)] ?? { dreamCount: 0 };
}

function writeDreamProjectState(project: HoppiProjectRef, projectState: DreamProjectState): void {
  const state = readDreamState();
  state.projects = state.projects ?? {};
  state.projects[dreamProjectKey(project)] = projectState;
  writeDreamState(state);
}

function dreamLastRunMs(project: HoppiProjectRef): number {
  const last = readDreamProjectState(project).lastDreamAt;
  const parsed = last ? Date.parse(last) : NaN;
  return Number.isFinite(parsed) ? parsed : 0;
}

function duplicateFingerprint(entry: MemoryMaintenanceEntry): string | undefined {
  const text = compactMaintenanceContent(entry.content)
    .toLowerCase()
    .replace(/\b(?:mem|sem)_[a-f0-9]+\b/g, "<id>")
    .replace(/\b\d{4}-\d{2}-\d{2}t\d{2}:\d{2}:\d{2}(?:\.\d+)?z\b/g, "<ts>")
    .replace(/session:\s*[a-f0-9-]{12,}/g, "session:<id>")
    .replace(/project:\s*[a-z]:[\\/][^\n]+/gi, "project:<path>")
    .replace(/source memories:\s*(?:<id>[,\s]*)+/g, "source memories:<ids>")
    .replace(/[^a-z0-9_./:+@#-]+/g, " ")
    .replace(/\s+/g, " ")
    .trim();
  if (text.length < 120) return undefined;
  return createHash("sha256").update(text).digest("hex");
}

function isObviousNoiseMemory(entry: MemoryMaintenanceEntry): boolean {
  if (isProtectedMemory(entry) || isVerifiedMemory(entry)) return false;
  const text = compactMaintenanceContent(entry.content).toLowerCase();
  if (text.length <= 320 && /\b(cd|cwd|current working directory|working directory)\b/.test(text) && !/changed|implemented|decision|published|fixed|added|removed/.test(text)) return true;
  if (text.length <= 260 && /^\[?tool[:\]]/.test(text)) return true;
  return false;
}

function dreamMemoryScore(entry: MemoryMaintenanceEntry): number {
  let score = 0;
  if (entry.pinned) score += 10_000;
  if (entry.starred) score += 9_000;
  if (isVerifiedMemory(entry)) score += 1_000;
  if (entry.layer === "semantic") score += 200;
  if (entry.source === "oppi:memory-maintenance") score += 200;
  if (entry.confidence === "observed") score += 40;
  if (entry.confidence === "inferred") score -= 40;
  score += Math.min(200, compactMaintenanceContent(entry.content).length / 20);
  score += Math.min(200, memoryCreatedMs(entry) / 10_000_000_000);
  return score;
}

function chooseDuplicateKeeper(group: readonly MemoryMaintenanceEntry[]): MemoryMaintenanceEntry {
  return group.slice().sort((a, b) => dreamMemoryScore(b) - dreamMemoryScore(a))[0]!;
}

async function runDreamStage1Duplicates(backend: HoppiBackend, memories: readonly MemoryMaintenanceEntry[]): Promise<DreamStageResult> {
  if (!backend.forget) throw new Error("Installed Hoppi package does not expose forget().");
  const result = emptyDreamStageResult();
  const deleted = new Set<string>();
  const deleteOne = async (entry: MemoryMaintenanceEntry, reason: string, verifiedKeeper = false) => {
    if (deleted.has(entry.id)) return;
    if (isProtectedMemory(entry)) {
      result.skippedPinned.push(entry.id);
      return;
    }
    if (isVerifiedMemory(entry) && !verifiedKeeper) {
      result.skippedVerified.push(entry.id);
      return;
    }
    await backend.forget!(entry.id);
    deleted.add(entry.id);
    result.deleted.push(entry.id);
    if (result.notes.length < 8) result.notes.push(`${entry.id}: ${reason}`);
  };

  for (const entry of memories) {
    if (isObviousNoiseMemory(entry)) await deleteOne(entry, "obvious cwd/tool noise");
  }

  const groups = new Map<string, MemoryMaintenanceEntry[]>();
  for (const entry of memories) {
    const key = duplicateFingerprint(entry);
    if (!key) continue;
    const group = groups.get(key) ?? [];
    group.push(entry);
    groups.set(key, group);
  }

  let duplicateGroups = 0;
  for (const group of groups.values()) {
    if (group.length < 2) continue;
    duplicateGroups += 1;
    const keeper = chooseDuplicateKeeper(group);
    const keeperVerified = isVerifiedMemory(keeper);
    for (const entry of group) {
      if (entry.id === keeper.id) continue;
      await deleteOne(entry, `duplicate of ${keeper.id}`, keeperVerified);
    }
  }
  if (duplicateGroups > 0) result.notes.unshift(`duplicate groups: ${duplicateGroups}`);
  return result;
}

function stageFromMaintenance(planResult: MaintenancePlanResult, applied: MemoryMaintenanceApplyResult): DreamStageResult & { modelLabel: string; modelReason: string } {
  return {
    created: applied.created,
    deleted: applied.deleted,
    skippedPinned: applied.skippedPinned,
    skippedVerified: applied.skippedVerified,
    notes: planResult.plan.notes,
    modelLabel: planResult.modelLabel,
    modelReason: planResult.modelReason,
  };
}

function formatDreamStageLine(label: string, stage: DreamStageResult & { modelLabel?: string; modelReason?: string }): string {
  const model = stage.modelLabel ? ` · ${stage.modelLabel}` : "";
  const skipped = stage.skippedPinned.length || stage.skippedVerified.length
    ? ` · skipped protected ${stage.skippedPinned.length}, verified ${stage.skippedVerified.length}`
    : "";
  return `${label}: created ${stage.created.length}, deleted ${stage.deleted.length}${skipped}${model}`;
}

function formatDreamSummary(result: DreamRunResult): string {
  const lines = [
    `🧠 Hoppi dream #${result.dreamCount} complete`,
    `Memories: ${result.beforeCount}${result.afterCount !== undefined ? ` → ${result.afterCount}` : ""}`,
    formatDreamStageLine("Stage 1 duplicate cleanup", result.stage1),
  ];
  if (result.stage2) lines.push(formatDreamStageLine("Stage 2 age-aware curation", result.stage2));
  if (result.stage3) {
    if ("skipped" in result.stage3) lines.push(`Stage 3 deep reconciliation: skipped (${result.stage3.reason})`);
    else lines.push(formatDreamStageLine("Stage 3 deep reconciliation", result.stage3));
  }
  const notes = [
    ...result.stage1.notes,
    ...(result.stage2?.notes ?? []),
    ...(result.stage3 && !("skipped" in result.stage3) ? result.stage3.notes : []),
  ].slice(0, 8);
  if (notes.length) lines.push("", "Notes:", ...notes.map((note) => `- ${note}`));
  return lines.join("\n").slice(0, 3_600);
}

async function runDreamMaintenance(ctx: ExtensionContext, backend: HoppiBackend, config: MemoryConfig): Promise<DreamRunResult | undefined> {
  if (!backend.list || !backend.forget || !backend.remember) throw new Error("Installed Hoppi package does not expose dream maintenance APIs (list/remember/forget).");
  const project = projectRef(ctx);
  const before = await backend.list({ project, includeSuperseded: false, limit: MEMORY_MAINTENANCE_HARD_LIMIT });
  if (before.length <= MEMORY_DREAM_TRIGGER_COUNT) return undefined;

  const state = readDreamProjectState(project);
  const dreamCount = state.dreamCount + 1;
  const result: DreamRunResult = {
    beforeCount: before.length,
    dreamCount,
    stage1: emptyDreamStageResult(),
  };

  result.stage1 = await runDreamStage1Duplicates(backend, before);
  const afterStage1 = await backend.list({ project, includeSuperseded: false, limit: MEMORY_MAINTENANCE_HARD_LIMIT });

  if (afterStage1.length > MEMORY_DREAM_TARGET_COUNT || result.stage1.deleted.length > 0) {
    try {
      const stage2Plan = await buildMaintenancePlanWithModel(ctx, config, afterStage1);
      const stage2Applied = await applyMemoryMaintenancePlan(backend, project, stage2Plan.plan, afterStage1);
      result.stage2 = stageFromMaintenance(stage2Plan, stage2Applied);
    } catch (error) {
      result.stage2 = { ...emptyDreamStageResult(), modelLabel: "unavailable", modelReason: "Stage 2 skipped", notes: [`stage2: ${error instanceof Error ? error.message : String(error)}`] };
    }
  }

  if (dreamCount % MEMORY_DREAM_DEEP_EVERY === 0) {
    const beforeStage3 = await backend.list({ project, includeSuperseded: false, limit: MEMORY_MAINTENANCE_HARD_LIMIT });
    const stage3Plan = await buildDeepMaintenancePlanWithModel(ctx, config, beforeStage3).catch((error: unknown) => {
      result.stage3 = { skipped: true, reason: error instanceof Error ? error.message : String(error) };
      return undefined;
    });
    if (!stage3Plan && !result.stage3) {
      result.stage3 = { skipped: true, reason: "set Memory deep model to sonnet/gpt-5.5 or OPPI_MEMORY_DEEP_MAINTENANCE_MODEL" };
    } else if (stage3Plan) {
      const stage3Applied = await applyMemoryMaintenancePlan(backend, project, stage3Plan.plan, beforeStage3);
      result.stage3 = stageFromMaintenance(stage3Plan, stage3Applied);
    }
  }

  const status = await backend.status(project).catch(() => undefined);
  result.afterCount = status?.memoryCount;
  const summary = formatDreamSummary(result);
  writeDreamProjectState(project, { dreamCount, lastDreamAt: new Date().toISOString(), lastSummary: summary });
  return result;
}

class HoppiMemoryWorker {
  private backendPromise: Promise<HoppiBackend> | undefined;
  private queue: Promise<void> = Promise.resolve();
  private startupInjected = new Set<string>();
  private lastTurnSummary: string | undefined;
  private dirtyVersion = 0;
  private consolidatedVersion = 0;
  private dirtyAt = 0;
  private idleTimer: NodeJS.Timeout | undefined;
  private consolidating = false;
  private warnedUnavailable = false;

  start(ctx: ExtensionContext): void {
    this.stop();
    this.idleTimer = setInterval(() => void this.tickIdle(ctx), MEMORY_IDLE_CHECK_MS);
    this.idleTimer.unref?.();
    void this.refreshStatus(ctx);
  }

  stop(): void {
    if (this.idleTimer) clearInterval(this.idleTimer);
    this.idleTimer = undefined;
  }

  async buildPromptContext(prompt: string, ctx: ExtensionContext): Promise<string | undefined> {
    const config = readMemoryConfig(ctx.cwd);
    if (!config.enabled) {
      publishStatus(ctx, "mem:off");
      return undefined;
    }

    try {
      const backend = await this.backend(ctx);
      const project = projectRef(ctx);
      const parts: string[] = [];
      const sessionKey = sessionSourceId(ctx) ?? ctx.cwd;

      if (config.startupRecall && !this.startupInjected.has(sessionKey) && backend.buildStartupContext) {
        this.startupInjected.add(sessionKey);
        const startup = await backend.buildStartupContext({ project, maxMemories: 8, includePinned: true });
        if (startup.contextMarkdown.trim()) parts.push(startup.contextMarkdown.trim());
        publishStatus(ctx, startup.memoryCount > 0 ? `mem:${startup.memoryCount}` : "mem:on");
      }

      if (config.taskStartRecall && prompt.trim() && backend.recall) {
        const recall = await backend.recall({ project, query: prompt, budget: 900, limit: 5 });
        if (recall.contextMarkdown.trim()) parts.push(recall.contextMarkdown.trim());
      }

      const unique = [...new Set(parts)];
      if (unique.length === 0) return undefined;
      return `${unique.join("\n\n---\n\n")}\n\nMemory is advisory: prefer current user instructions and current files when they conflict.`;
    } catch (error) {
      this.warnUnavailable(ctx, error);
      return undefined;
    }
  }

  enqueueTurnSummary(messages: readonly AgentMessageLike[], ctx: ExtensionContext): void {
    const config = readMemoryConfig(ctx.cwd);
    if (!config.enabled || !config.turnSummaries) return;
    const fallbackSummary = buildTurnSummary(messages);
    if (!fallbackSummary || fallbackSummary === this.lastTurnSummary) return;
    this.lastTurnSummary = fallbackSummary;
    this.enqueue(ctx, async (backend) => {
      if (!backend.remember) return;
      const distilled = await buildModelTurnSummary(messages, ctx, config).catch(() => undefined);
      if (distilled?.kind === "skip") return;
      const summary = distilled?.kind === "memory" ? distilled.content : fallbackSummary;
      if (!summary) return;
      this.lastTurnSummary = summary;
      await backend.remember({
        project: projectRef(ctx),
        content: summary,
        tags: ["oppi-turn-summary", "agent_end", ...(distilled?.kind === "memory" ? distilled.tags : [])],
        layer: "buffer",
        confidence: "observed",
        source: "oppi:agent_end",
        sourceSessionId: sessionSourceId(ctx),
      });
      this.markDirty(ctx);
    });
  }

  async rememberExitRecap(ctx: ExtensionContext): Promise<void> {
    const config = readMemoryConfig(ctx.cwd);
    if (!config.enabled) return;
    const recap = buildSessionRecap(ctx);
    if (!recap) return;
    await this.enqueueAndWait(ctx, async (backend) => {
      if (!backend.remember) return;
      await backend.remember({
        project: projectRef(ctx),
        content: recap,
        tags: ["oppi-exit-recap", "session-recap"],
        confidence: "observed",
        source: "oppi:exit",
        sourceSessionId: sessionSourceId(ctx),
      });
      this.markDirty(ctx);
    });
  }

  async drain(): Promise<void> {
    await this.queue.catch(() => undefined);
  }

  private async refreshStatus(ctx: ExtensionContext): Promise<void> {
    const config = readMemoryConfig(ctx.cwd);
    if (!config.enabled) return publishStatus(ctx, "mem:off");
    try {
      const backend = await this.backend(ctx);
      const status = await backend.status(projectRef(ctx));
      publishStatus(ctx, status.memoryCount > 0 ? `mem:${status.memoryCount}` : "mem:on");
    } catch (error) {
      this.warnUnavailable(ctx, error);
    }
  }

  private enqueue(ctx: ExtensionContext, job: (backend: HoppiBackend) => Promise<void>): Promise<void> {
    const run = this.queue
      .catch(() => undefined)
      .then(() => this.runJob(ctx, job));
    this.queue = run;
    return run;
  }

  private async enqueueAndWait(ctx: ExtensionContext, job: (backend: HoppiBackend) => Promise<void>): Promise<void> {
    await this.enqueue(ctx, job);
  }

  private async runJob(ctx: ExtensionContext, job: (backend: HoppiBackend) => Promise<void>): Promise<void> {
    try {
      const backend = await this.backend(ctx);
      await job(backend);
    } catch (error) {
      this.warnUnavailable(ctx, error);
    }
  }

  private async backend(ctx: ExtensionContext): Promise<HoppiBackend> {
    if (!this.backendPromise) {
      this.backendPromise = (async () => {
        const hoppi = await loadHoppi();
        if (!hoppi.createHoppiBackend) throw new Error("Installed Hoppi package does not expose createHoppiBackend().");
        const backend = hoppi.createHoppiBackend({ root: hoppiRoot(hoppi) });
        await backend.init();
        return backend;
      })();
    }
    return this.backendPromise;
  }

  private markDirty(ctx: ExtensionContext): void {
    this.dirtyVersion += 1;
    this.dirtyAt = Date.now();
    publishStatus(ctx, "mem:saved");
  }

  private async tickIdle(ctx: ExtensionContext): Promise<void> {
    const config = readMemoryConfig(ctx.cwd);
    if (!config.enabled || !config.idleConsolidation || !ctx.isIdle()) return;
    if (this.consolidating) return;

    const project = projectRef(ctx);
    const hasDirtyMemory = this.dirtyVersion > this.consolidatedVersion && Date.now() - this.dirtyAt >= MEMORY_IDLE_CONSOLIDATE_AFTER_MS;
    const dreamIntervalElapsed = Date.now() - dreamLastRunMs(project) >= MEMORY_DREAM_MIN_INTERVAL_MS;
    if (!hasDirtyMemory && !dreamIntervalElapsed) return;

    this.consolidating = true;
    const run = this.enqueue(ctx, async (backend) => {
      const status = await backend.status(project).catch(() => undefined);
      const shouldDream = dreamIntervalElapsed && (status?.memoryCount ?? 0) > MEMORY_DREAM_TRIGGER_COUNT;
      if (shouldDream) {
        publishStatus(ctx, "mem:dream…");
        const dream = await runDreamMaintenance(ctx, backend, config);
        if (dream && ctx.hasUI) ctx.ui.notify(formatDreamSummary(dream), "info");
        const nextStatus = await backend.status(project).catch(() => undefined);
        publishStatus(ctx, nextStatus ? `mem:${nextStatus.memoryCount}` : "mem:dream done");
      } else if (hasDirtyMemory && backend.consolidate) {
        publishStatus(ctx, "mem:consolidate…");
        await backend.consolidate({ project, dryRun: false, budget: 1_500 });
        publishStatus(ctx, "mem:consolidated");
      }
      if (hasDirtyMemory) this.consolidatedVersion = this.dirtyVersion;
    });
    void run.finally(() => {
      this.consolidating = false;
    });
  }

  private warnUnavailable(ctx: ExtensionContext, error: unknown): void {
    const missing = isHoppiMissingError(error);
    publishStatus(ctx, missing ? "mem:setup" : "mem:error");
    if (!ctx.hasUI || this.warnedUnavailable) return;
    this.warnedUnavailable = true;
    ctx.ui.notify(missing ? hoppiSetupMessage() : `Hoppi memory unavailable: ${error instanceof Error ? error.message : String(error)}`, missing ? "info" : "warning");
  }
}

function bool(value: boolean): "on" | "off" {
  return value ? "on" : "off";
}

function nextValue<T extends string | number>(values: readonly T[], current: T): T {
  const index = values.indexOf(current);
  return values[(index + 1) % values.length] ?? values[0]!;
}

const ANSI_PATTERN = /\x1B\[[0-?]*[ -/]*[@-~]/g;

function isZeroWidthCodePoint(code: number): boolean {
  return (code >= 0x0300 && code <= 0x036f) || (code >= 0xfe00 && code <= 0xfe0f) || code === 0x200d;
}

function isWideCodePoint(code: number): boolean {
  return code >= 0x1f000
    || (code >= 0x2600 && code <= 0x27bf)
    || (code >= 0x2e80 && code <= 0xa4cf)
    || (code >= 0xac00 && code <= 0xd7a3)
    || (code >= 0xf900 && code <= 0xfaff)
    || (code >= 0xff01 && code <= 0xff60);
}

function settingsVisibleWidth(value: string): number {
  let width = 0;
  for (const char of value.replace(ANSI_PATTERN, "")) {
    const code = char.codePointAt(0) ?? 0;
    if (isZeroWidthCodePoint(code)) continue;
    width += isWideCodePoint(code) ? 2 : 1;
  }
  return width;
}

type SettingRow = {
  label: string;
  value?: string;
  description: string;
  action?: SettingsAction;
  cycle?: () => void;
};

class OppiSettingsComponent implements Component {
  private tab = 1;
  private selected = 0;
  private cachedWidth?: number;
  private cachedLines?: string[];

  constructor(
    private readonly theme: Theme,
    initialTab: number,
    private readonly getMemoryConfig: () => MemoryConfig,
    private readonly saveMemoryConfig: (config: MemoryConfig) => void,
    private readonly getAskUserConfig: () => AskUserConfig,
    private readonly saveAskUserConfig: (config: AskUserConfig) => void,
    private readonly getFooterConfig: () => FooterConfig,
    private readonly saveFooterConfig: (config: FooterConfig) => void,
    private readonly getCompactConfig: () => OppiCompactConfig,
    private readonly saveCompactConfig: (config: OppiCompactConfig) => void,
    private readonly getPermissionConfig: () => PermissionConfig,
    private readonly savePermissionConfig: (config: PermissionConfig) => void,
    private readonly getThemeName: () => string,
    private readonly setThemeName: (name: string) => void,
    private readonly done: (action: SettingsAction | undefined) => void,
  ) {
    this.tab = Math.max(0, Math.min(SETTINGS_TABS.length - 1, initialTab));
  }

  handleInput(data: string): void {
    if (matchesKey(data, Key.escape) || matchesKey(data, Key.ctrl("c"))) return this.done(undefined);
    if (matchesKey(data, Key.left) || data === "h") return this.moveTab(-1);
    if (matchesKey(data, Key.right) || data === "l") return this.moveTab(1);
    const rows = this.rows();
    if (matchesKey(data, Key.up) || data === "k") return this.moveRow(-1, rows.length);
    if (matchesKey(data, Key.down) || data === "j") return this.moveRow(1, rows.length);
    if (matchesKey(data, Key.enter) || matchesKey(data, Key.space)) {
      const row = rows[this.selected];
      if (!row) return;
      if (row.action) return this.done(row.action);
      if (row.cycle) {
        row.cycle();
        this.invalidate();
      }
    }
  }

  render(width: number): string[] {
    if (this.cachedWidth === width && this.cachedLines) return this.cachedLines;
    const panelWidth = Math.max(48, Math.min(width, 96));
    const inner = panelWidth - 2;
    const t = this.theme;
    const rows = this.rows();
    const lines: string[] = [];

    lines.push(this.topBorder("OPPi settings", panelWidth));
    lines.push(this.boxed(this.renderTabs(inner), inner));
    lines.push(this.boxed(t.fg("dim", "←/→ tabs · ↑/↓ settings · Space/Enter change or run · Esc close"), inner));
    lines.push(this.boxed("", inner));

    if (rows.length === 0) {
      lines.push(this.boxed(` ${t.fg("muted", "No settings in this tab yet.")}`, inner));
    } else {
      rows.forEach((row, index) => {
        const active = index === this.selected;
        const cursor = active ? t.fg("accent", "›") : " ";
        const label = active ? t.bold(t.fg("accent", row.label)) : t.fg("toolOutput", row.label);
        const value = row.value ? (active ? t.fg("success", row.value) : t.fg("muted", row.value)) : t.fg("dim", row.action ? "open" : "");
        const available = Math.max(12, inner - 5 - visibleWidth(row.label));
        lines.push(this.boxed(` ${cursor} ${label} ${t.fg("dim", "—")} ${truncateToWidth(value, available, "…")}`, inner));
        lines.push(this.boxed(`     ${t.fg("dim", row.description)}`, inner));
      });
    }

    lines.push(this.boxed("", inner));
    lines.push(this.boxed(this.footerText(), inner));
    lines.push(this.bottomBorder(panelWidth));
    this.cachedWidth = width;
    this.cachedLines = lines.map((line) => truncateToWidth(line, width, ""));
    return this.cachedLines;
  }

  invalidate(): void {
    this.cachedWidth = undefined;
    this.cachedLines = undefined;
  }

  private moveTab(delta: number): void {
    this.tab = (this.tab + delta + SETTINGS_TABS.length) % SETTINGS_TABS.length;
    this.selected = 0;
    this.invalidate();
  }

  private moveRow(delta: number, count: number): void {
    if (count === 0) return;
    this.selected = (this.selected + delta + count) % count;
    this.invalidate();
  }

  private rows(): SettingRow[] {
    if (this.tab === 0) {
      return [
        { label: "Settings command", value: "/settings:oppi", description: "Stage 1 uses this namespaced command because Pi owns the built-in /settings command." },
        { label: "Future wrapper", value: "/settings", description: "The OPPi wrapper should route /settings to this unified surface and embed/delegate Pi native settings." },
        { label: "Memory shortcut", value: "/memory", description: "Opens Hoppi dashboard; CLI keeps only core toggles and Hoppi owns detailed settings." },
        {
          label: "Question timeout",
          value: askUserTimeoutLabel(this.getAskUserConfig().timeoutMinutes),
          description: "When ask_user waits too long, keep answered questions and fill unanswered ones with recommended/default choices.",
          cycle: () => this.updateAskUser({ timeoutMinutes: coerceAskUserTimeout(nextValue(ASK_USER_TIMEOUT_MINUTES, this.getAskUserConfig().timeoutMinutes)) }),
        },
        { label: "Usage", value: "/usage", description: "Usage and cost status stays separate from product settings." },
        { label: "Footer settings", value: "Footer tab", description: "Customize the main bottom bar and the second hotkey-help bar." },
      ];
    }

    if (this.tab === 1) {
      const config = this.getFooterConfig();
      return [
        {
          label: "Hotkey help bar",
          value: bool(config.showHelpBar),
          description: `Show the second bottom bar with keyboard hints. ${FOOTER_HELP_SHORTCUT_LABEL} toggles it from anywhere.`,
          cycle: () => this.updateFooter({ ...config, showHelpBar: !config.showHelpBar }),
        },
        {
          label: "Usage display",
          value: footerUsageDisplayLabel(config.usageDisplay),
          description: "Choose which usage meters appear in the main bottom bar.",
          cycle: () => this.updateFooter({ ...config, usageDisplay: nextValue(FOOTER_USAGE_DISPLAY_VALUES, config.usageDisplay) }),
        },
        {
          label: "Workspace line",
          value: bool(config.showPath),
          description: "Show the cwd, git branch, and session name line above the main bottom bar.",
          cycle: () => this.updateFooter({ ...config, showPath: !config.showPath }),
        },
        {
          label: "Model + effort",
          value: bool(config.showModel),
          description: "Show the selected model and current reasoning effort.",
          cycle: () => this.updateFooter({ ...config, showModel: !config.showModel }),
        },
        {
          label: "Permission mode",
          value: bool(config.showPermission),
          description: "Show read-only/default/auto-review/full-access state.",
          cycle: () => this.updateFooter({ ...config, showPermission: !config.showPermission }),
        },
        {
          label: "Memory status",
          value: bool(config.showMemory),
          description: "Show compact Hoppi memory status when memory is active.",
          cycle: () => this.updateFooter({ ...config, showMemory: !config.showMemory }),
        },
        {
          label: "Context usage",
          value: bool(config.showContext),
          description: "Show active context-window usage.",
          cycle: () => this.updateFooter({ ...config, showContext: !config.showContext }),
        },
      ];
    }

    if (this.tab === 2) {
      const config = this.getMemoryConfig();
      return [
        {
          label: "Memory",
          value: bool(config.enabled),
          description: "Master switch for Hoppi-backed memory in OPPi.",
          cycle: () => this.updateMemory({ ...config, enabled: !config.enabled }),
        },
        {
          label: "Backend package",
          value: HOPPI_PACKAGE_NAME,
          description: `Install/update Hoppi from npm into ${formatPath(managedPackagesDir())}. OPPi asks first; no hidden install runs.`,
          action: "install-hoppi",
        },
        {
          label: "First-start install offer",
          value: config.hoppiInstallOffer === "dismissed" ? "dismissed" : "ask",
          description: "Ask once on interactive startup when Memory is on but the Hoppi backend package is missing.",
          cycle: () => this.updateMemory({ ...config, hoppiInstallOffer: nextValue(HOPPI_INSTALL_OFFER_VALUES, config.hoppiInstallOffer) }),
        },
        {
          label: "Startup recall",
          value: bool(config.startupRecall),
          description: "Load a small relevant memory context when OPPi starts.",
          cycle: () => this.updateMemory({ ...config, startupRecall: !config.startupRecall }),
        },
        {
          label: "Task-start recall",
          value: bool(config.taskStartRecall),
          description: "Run high-precision recall at new task boundaries; it may return nothing.",
          cycle: () => this.updateMemory({ ...config, taskStartRecall: !config.taskStartRecall }),
        },
        {
          label: "Turn summaries",
          value: bool(config.turnSummaries),
          description: "Save compact durable notes at the end of useful agent turns.",
          cycle: () => this.updateMemory({ ...config, turnSummaries: !config.turnSummaries }),
        },
        {
          label: "Memory agent model",
          value: config.agentModel,
          description: "auto uses safe defaults; claude/gpt opt into model-backed turn-memory distillation when auth is available.",
          cycle: () => this.updateMemory({ ...config, agentModel: nextValue(AGENT_MODEL_VALUES, config.agentModel) }),
        },
        {
          label: "Deep dream model",
          value: config.deepModel,
          description: "auto skips costly Stage 3; sonnet/gpt-5.5 enables every-5th-dream whole-store reconciliation.",
          cycle: () => this.updateMemory({ ...config, deepModel: nextValue(DEEP_MODEL_VALUES, config.deepModel) }),
        },
        {
          label: "Idle dream mode",
          value: bool(config.idleConsolidation),
          description: "Consolidate dirty memory and auto-dream stores over 60 memories with protected/verified safety rules.",
          cycle: () => this.updateMemory({ ...config, idleConsolidation: !config.idleConsolidation }),
        },
        {
          label: "Sync",
          value: bool(config.sync.enabled),
          description: "Enable opt-in Hoppi sync; repo, encryption, conflicts, and cadence live in the dashboard.",
          cycle: () => this.updateMemory({ ...config, sync: { ...config.sync, enabled: !config.sync.enabled } }),
        },
        { label: "Detailed settings", value: "dashboard", description: "Open Hoppi for models, port, sync repo, encryption, conflicts, and advanced tuning.", action: "open-dashboard" },
      ];
    }

    if (this.tab === 3) {
      const config = this.getCompactConfig();
      return [
        {
          label: "Idle compaction",
          value: bool(config.idleCompact.enabled),
          description: "Compact only after OPPi is idle long enough and context is full enough.",
          cycle: () => this.updateCompact({ ...config, idleCompact: { ...config.idleCompact, enabled: !config.idleCompact.enabled } }),
        },
        {
          label: "Idle time",
          value: `${config.idleCompact.idleMinutes}m`,
          description: "How long OPPi waits after the agent becomes idle before compacting.",
          cycle: () => this.updateCompact({ ...config, idleCompact: { ...config.idleCompact, idleMinutes: coerceIdleMinutes(nextValue(VALID_IDLE_MINUTES, config.idleCompact.idleMinutes)) } }),
        },
        {
          label: "Idle context threshold",
          value: `${config.idleCompact.thresholdPercent}%`,
          description: "Only idle-compact when context usage is at or above this percentage.",
          cycle: () => this.updateCompact({ ...config, idleCompact: { ...config.idleCompact, thresholdPercent: coerceIdleThreshold(nextValue(VALID_IDLE_THRESHOLDS, config.idleCompact.thresholdPercent)) } }),
        },
        {
          label: "Smart compact threshold",
          value: `${config.smartCompact.thresholdPercent}%`,
          description: "During todo-driven work, compact after todo_write checkpoints at or above this usage.",
          cycle: () => this.updateCompact({ ...config, smartCompact: { thresholdPercent: coerceSmartThreshold(nextValue(VALID_SMART_THRESHOLDS, config.smartCompact.thresholdPercent)) } }),
        },
      ];
    }

    if (this.tab === 4) {
      const config = this.getPermissionConfig();
      const timeout = `${Math.round(config.reviewTimeoutMs / 1000)}s`;
      const timeoutValues = PERMISSION_TIMEOUT_SECONDS.map(String);
      return [
        {
          label: "Permission mode",
          value: config.mode,
          description: "Controls read-only/default/auto-review/full-access behavior for risky tool calls.",
          cycle: () => this.updatePermission({ ...config, mode: nextValue(PERMISSION_MODES, config.mode) }),
        },
        {
          label: "Auto-review timeout",
          value: timeout,
          description: "Maximum time the isolated OPPi Guardian reviewer can spend on one risky call.",
          cycle: () => this.updatePermission({ ...config, reviewTimeoutMs: coerceTimeout(Number(nextValue(timeoutValues, String(Math.round(config.reviewTimeoutMs / 1000)))) * 1000) }),
        },
        {
          label: "Reviewer model",
          value: config.reviewerModel || "auto",
          description: "Model used by the isolated OPPi Guardian reviewer. Use /permissions reviewer-model provider/model for exact control.",
          action: "permission-reviewer-model",
        },
        { label: "Review history", value: "open", description: "Show recent auto-review decisions and cached approvals.", action: "permission-history" },
        { label: "Clear session allowances", value: "clear", description: "Reset manual allowances, auto-review cache, and denial circuit breakers.", action: "permission-clear" },
        { label: "Status", value: "show", description: "Show current permission mode, timeout, cache, and review count.", action: "permission-status" },
      ];
    }

    const themeName = normalizeThemeName(this.getThemeName());
    return [
      {
        label: "OPPi theme",
        value: themeName,
        description: themeLabel(themeName),
        cycle: () => this.updateTheme(nextValue(OPPI_THEMES.map((item) => item.name), themeName)),
      },
      { label: "Preview picker", value: "open", description: "Open the richer live-preview picker for OPPi themes.", action: "theme-preview" },
    ];
  }

  private updateMemory(config: MemoryConfig): void {
    this.saveMemoryConfig(config);
  }

  private updateAskUser(config: AskUserConfig): void {
    this.saveAskUserConfig(config);
  }

  private updateFooter(config: FooterConfig): void {
    this.saveFooterConfig(config);
  }

  private updateCompact(config: OppiCompactConfig): void {
    this.saveCompactConfig(config);
  }

  private updatePermission(config: PermissionConfig): void {
    this.savePermissionConfig(config);
  }

  private updateTheme(name: string): void {
    this.setThemeName(name);
  }

  private renderTabs(width: number): string {
    const t = this.theme;
    const rendered = SETTINGS_TABS.map((tab, index) => index === this.tab ? t.bg("selectedBg", t.fg("accent", ` ${tab} `)) : t.fg("muted", ` ${tab} `)).join(t.fg("dim", " "));
    return truncateToWidth(rendered, width, "…");
  }

  private footerText(): string {
    const memory = this.getMemoryConfig();
    const footer = this.getFooterConfig();
    const compact = this.getCompactConfig();
    const permissions = this.getPermissionConfig();
    return this.theme.fg("dim", `Memory ${bool(memory.enabled)} · footer ${footerUsageDisplayLabel(footer.usageDisplay)} / help ${bool(footer.showHelpBar)} · idle compact ${bool(compact.idleCompact.enabled)} @ ${compact.idleCompact.thresholdPercent}% · perm ${permissions.mode} · theme ${normalizeThemeName(this.getThemeName())}`);
  }

  private boxed(content: string, inner: number): string {
    const fitted = truncateToWidth(content, inner, "…");
    const pad = Math.max(0, inner - settingsVisibleWidth(fitted));
    return `${this.theme.fg("border", "│")}${fitted}${" ".repeat(pad)}${this.theme.fg("border", "│")}`;
  }

  private topBorder(title: string, width: number): string {
    const safe = ` ${title} `;
    const remaining = Math.max(0, width - 2 - visibleWidth(safe));
    return `${this.theme.fg("borderAccent", "╭")}${this.theme.fg("borderAccent", "─")}${this.theme.fg("accent", safe)}${this.theme.fg("borderAccent", "─".repeat(Math.max(0, remaining - 1)))}${this.theme.fg("borderAccent", "╮")}`;
  }

  private bottomBorder(width: number): string {
    return this.theme.fg("borderAccent", `╰${"─".repeat(Math.max(0, width - 2))}╯`);
  }
}

function initialSettingsTab(args: string | undefined): number {
  const normalized = (args ?? "").trim().toLowerCase();
  if (normalized.startsWith("footer") || normalized.startsWith("bottom") || normalized.startsWith("status")) return 1;
  if (normalized.startsWith("memory")) return 2;
  if (normalized.startsWith("compact") || normalized.startsWith("idle")) return 3;
  if (normalized.startsWith("permission") || normalized.startsWith("perm")) return 4;
  if (normalized.startsWith("theme")) return 5;
  return 0;
}

async function showSettings(pi: ExtensionAPI, ctx: ExtensionCommandContext, args?: string): Promise<void> {
  const initialTab = initialSettingsTab(args);
  while (true) {
    const action = await ctx.ui.custom<SettingsAction | undefined>((tui, theme, _kb, done) => {
      const component = new OppiSettingsComponent(
        theme,
        initialTab,
        () => readMemoryConfig(ctx.cwd),
        (config) => {
          writeMemoryConfig(config);
          tui.requestRender();
        },
        () => readAskUserConfig(ctx.cwd),
        (config) => {
          writeGlobalAskUserConfig(config);
          tui.requestRender();
        },
        () => readFooterConfig(ctx.cwd),
        (config) => {
          writeFooterConfig(ctx.cwd, config);
          pi.events.emit(FOOTER_CONFIG_CHANGED_EVENT, { cwd: ctx.cwd, config });
          tui.requestRender(true);
        },
        () => readOppiCompactConfig(ctx.cwd),
        (config) => {
          writeGlobalOppiCompactConfig(config);
          tui.requestRender();
        },
        () => readPermissionConfig(ctx.cwd),
        (config) => {
          writeGlobalPermissionConfig(config);
          publishMode(pi, ctx, config.mode);
          tui.requestRender();
        },
        () => currentThemeName(ctx),
        (name) => {
          const result = setOppiTheme(ctx, name);
          ctx.ui.notify(result.success ? `Theme set to ${name}.` : result.error ?? `Could not set theme ${name}.`, result.success ? "info" : "error");
          tui.requestRender();
        },
        done,
      );
      return {
        render: (width: number) => component.render(width),
        invalidate: () => component.invalidate(),
        handleInput: (data: string) => {
          component.handleInput(data);
          tui.requestRender();
        },
      };
    });

    if (!action || action === "close") return;
    if (action === "install-hoppi") await installHoppiFromUi(ctx);
    if (action === "setup-sync") await runSyncSetupWizard(ctx);
    if (action === "sync-now") await runSync(ctx, "both", "settings", true);
    if (action === "pull-now") await runSync(ctx, "pull", "settings", true);
    if (action === "push-now") await runSync(ctx, "push", "settings", true);
    if (action === "open-dashboard") await openMemoryDashboard(ctx);
    if (action === "permission-history") await showPermissionHistory(ctx);
    if (action === "permission-clear") {
      clearSessionPermissionState();
      ctx.ui.notify("Cleared session permission allowances, auto-review cache, and circuit breakers.", "info");
    }
    if (action === "permission-reviewer-model") {
      const config = readPermissionConfig(ctx.cwd);
      const value = await ctx.ui.input("Auto-review model (auto or provider/model)", config.reviewerModel || "auto");
      if (value !== undefined) {
        writeGlobalPermissionConfig({ ...config, reviewerModel: value });
        ctx.ui.notify(`Auto-review reviewer set to ${readPermissionConfig(ctx.cwd).reviewerModel || "auto"}.`, "info");
      }
    }
    if (action === "permission-status") ctx.ui.notify(permissionStatusText(readPermissionConfig(ctx.cwd), ctx), "info");
    if (action === "theme-preview") {
      const selected = await openThemePreview(ctx);
      if (selected) {
        const result = setOppiTheme(ctx, selected);
        ctx.ui.notify(result.success ? `Theme set to ${selected}.` : result.error ?? `Could not set theme ${selected}.`, result.success ? "info" : "error");
      }
    }
  }
}

function ghAvailable(): boolean {
  try {
    execFileSync("gh", ["--version"], { stdio: "ignore" });
    return true;
  } catch {
    return false;
  }
}

function runGhRepoCreate(repo: string, repoPath: string): string | undefined {
  const output = execFileSync("gh", ["repo", "create", repo, "--private"], {
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
  });
  execFileSync("gh", ["repo", "clone", repo, repoPath], { stdio: "ignore" });
  return output.trim().split(/\r?\n/).find((line) => line.includes("github.com"));
}

async function runSyncSetupWizard(ctx: ExtensionCommandContext): Promise<void> {
  const current = readMemoryConfig(ctx.cwd);
  const useGh = await ctx.ui.confirm(
    "Hoppi sync setup",
    "Create/clone a private GitHub repository with the GitHub CLI? Choose No to configure an existing local repo path instead.",
  );

  let repoPath = current.sync.repoPath ?? defaultRepoPath();
  let repoUrl = current.sync.repoUrl;

  if (useGh) {
    if (!ghAvailable()) {
      ctx.ui.notify("GitHub CLI `gh` is not available. Install/login with gh or configure an existing repo path.", "warning");
      return;
    }
    const repo = await ctx.ui.input("Private GitHub repo", DEFAULT_SYNC_REPO_NAME) || DEFAULT_SYNC_REPO_NAME;
    const pathInput = await ctx.ui.input("Local clone path", repoPath) || repoPath;
    repoPath = resolveUserPath(pathInput);
    mkdirSync(dirname(repoPath), { recursive: true });
    try {
      const created = runGhRepoCreate(repo, repoPath);
      repoUrl = created ?? repoUrl;
      ctx.ui.notify(`Created private Hoppi sync repo at ${repoPath}.`, "info");
    } catch (error) {
      ctx.ui.notify(`GitHub repo setup failed: ${error instanceof Error ? error.message : String(error)}`, "warning");
      return;
    }
  } else {
    const pathInput = await ctx.ui.input("Local sync repo path", repoPath) || repoPath;
    repoPath = resolveUserPath(pathInput);
    const remoteInput = await ctx.ui.input("Git remote URL (optional)", repoUrl ?? "") || "";
    repoUrl = remoteInput.trim() || undefined;
  }

  const encryption = await ctx.ui.select("Encryption", [
    "none — rely on private repository permissions",
    "env passphrase — encrypted payload, passphrase from environment",
    "local passphrase file — encrypted payload, stored on this device",
  ]) ?? "none — rely on private repository permissions";

  let nextSync: MemorySyncConfig = {
    ...current.sync,
    enabled: true,
    repoPath,
    repoUrl,
    pullOnStartup: true,
    pushOnExit: true,
  };

  if (encryption.startsWith("env")) {
    const envName = await ctx.ui.input("Passphrase environment variable", current.sync.passphraseEnv || DEFAULT_PASSPHRASE_ENV) || DEFAULT_PASSPHRASE_ENV;
    nextSync = { ...nextSync, encryption: "passphrase", passphraseSource: "env", passphraseEnv: envName };
    ctx.ui.notify(`Set ${envName} before startup/exit sync runs.`, "info");
  } else if (encryption.startsWith("local")) {
    const passphrase = await ctx.ui.input("Passphrase (stored locally on this device)", "") || "";
    if (!passphrase.trim()) {
      ctx.ui.notify("Passphrase was empty; leaving encryption disabled.", "warning");
      nextSync = { ...nextSync, encryption: "none" };
    } else {
      const file = defaultPassphraseFile();
      mkdirSync(dirname(file), { recursive: true });
      writeFileSync(file, `${passphrase.trim()}\n`, { encoding: "utf8", mode: 0o600 });
      nextSync = { ...nextSync, encryption: "passphrase", passphraseSource: "file", passphraseFile: file };
    }
  } else {
    nextSync = { ...nextSync, encryption: "none" };
  }

  const next = { ...current, enabled: true, sync: nextSync };
  writeMemoryConfig(next);
  ctx.ui.notify("Hoppi sync is configured. Startup pull and /exit push are enabled.", "info");

  const initial = await ctx.ui.confirm("Initial sync", "Run an initial pull + push now?");
  if (initial) await runSync(ctx, "both", "setup", true);
}

class BackgroundSyncRunner {
  private timer: NodeJS.Timeout | undefined;
  private running = false;

  start(ctx: ExtensionContext): void {
    this.stop();
    const config = readMemoryConfig(ctx.cwd);
    const interval = this.intervalMs(config.sync.backgroundSync);
    if (!config.enabled || !config.sync.enabled || interval === 0) return;
    this.timer = setInterval(() => {
      if (this.running || !ctx.isIdle()) return;
      this.running = true;
      void runSync(ctx, "both", "idle", false).finally(() => { this.running = false; });
    }, interval);
    this.timer.unref?.();
  }

  stop(): void {
    if (this.timer) clearInterval(this.timer);
    this.timer = undefined;
    this.running = false;
  }

  private intervalMs(value: BackgroundSync): number {
    if (value === "15m") return 15 * 60_000;
    if (value === "30m") return 30 * 60_000;
    if (value === "60m") return 60 * 60_000;
    return 0;
  }
}

export default function memoryExtension(pi: ExtensionAPI) {
  const backgroundSync = new BackgroundSyncRunner();
  const memoryWorker = new HoppiMemoryWorker();

  pi.on("session_start", async (_event, ctx) => {
    await maybeOfferHoppiInstall(ctx);
    const config = readMemoryConfig(ctx.cwd);
    const missingHoppi = config.enabled ? await isHoppiMissing() : false;
    publishStatus(ctx, config.enabled ? (missingHoppi ? "mem:setup" : config.sync.enabled ? "mem:sync on" : "mem:on") : "mem:off");
    if (missingHoppi) return;
    memoryWorker.start(ctx);
    backgroundSync.start(ctx);
    if (!startupSyncStarted && config.enabled && config.sync.enabled && config.sync.pullOnStartup) {
      startupSyncStarted = true;
      void runSync(ctx, "pull", "startup", true);
    }
  });

  pi.on("before_agent_start", async (event, ctx) => {
    const content = await memoryWorker.buildPromptContext(event.prompt, ctx);
    if (!content) return;
    return {
      message: {
        customType: MEMORY_CONTEXT_TYPE,
        content,
        // Keep routine recall out of the visible transcript; the footer carries
        // memory status, and summaries explicitly ignore this custom message.
        display: false,
        details: { source: "hoppi", kind: "recall", createdAt: new Date().toISOString() },
      },
    };
  });

  pi.on("agent_end", async (event, ctx) => {
    memoryWorker.enqueueTurnSummary(event.messages as AgentMessageLike[], ctx);
  });

  pi.on("session_shutdown", async (event, ctx) => {
    backgroundSync.stop();
    memoryWorker.stop();
    if (event.reason === "quit") await memoryWorker.rememberExitRecap(ctx);
    await memoryWorker.drain();

    const config = readMemoryConfig(ctx.cwd);
    if (event.reason === "quit" && config.enabled && config.sync.enabled && config.sync.pushOnExit) {
      await runSync(ctx, "push", "exit", true);
    }
    if (dashboardHandle) {
      try { await dashboardHandle.stop(); } catch { /* ignore */ }
      dashboardHandle = undefined;
    }
  });

  pi.registerCommand("settings:oppi", {
    description: "Open unified OPPi settings (General, Memory, Compaction, Permissions, Theme).",
    getArgumentCompletions: (prefix: string) => ["general", "memory", "compaction", "permissions", "theme"]
      .filter((value) => value.startsWith(prefix.toLowerCase()))
      .map((value) => ({ value, label: value })),
    handler: async (args, ctx) => showSettings(pi, ctx, args),
  });

  pi.registerCommand("memory", {
    description: "Open the Hoppi memory dashboard.",
    handler: async (_args, ctx) => openMemoryDashboard(ctx),
  });

  pi.registerCommand("memory-maintenance", {
    description: "Temporary Hoppi cleanup pass using GPT-5.4 mini. Usage: /memory-maintenance [dry-run|apply] [--yes] [--limit N]",
    getArgumentCompletions: (prefix: string) => ["dry-run", "apply", "apply --yes", "help"]
      .filter((value) => value.startsWith(prefix.toLowerCase()))
      .map((value) => ({ value, label: value })),
    handler: async (args, ctx) => runMemoryMaintenance(ctx, args),
  });
}
