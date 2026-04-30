#!/usr/bin/env node
import { spawn } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, isAbsolute, join, resolve } from "node:path";
import { createRequire } from "node:module";
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
const OPPI_CLI_PACKAGE_NAME = "@oppiai/cli";
const OPPI_CHANGELOG_URL = "https://github.com/RemindZ/oppi/blob/main/CHANGELOG.md";
const UPDATE_CHECK_INTERVAL_MS = 24 * 60 * 60 * 1000;
const UPDATE_NOTICE_INTERVAL_MS = 24 * 60 * 60 * 1000;
const UPDATE_CHECK_TIMEOUT_MS = 1200;

export type OppiCommand =
  | { type: "help" }
  | { type: "version" }
  | { type: "doctor"; json: boolean; agentDir?: string }
  | { type: "update"; check: boolean; json: boolean }
  | { type: "mem"; subcommand: "status" | "setup" | "install" | "dashboard" | "open"; json: boolean }
  | PluginCommand
  | MarketplaceCommand
  | { type: "launch"; piArgs: string[]; agentDir?: string; withPiExtensions: boolean };

export type DiagnosticStatus = "pass" | "warn" | "fail";
export type Diagnostic = { status: DiagnosticStatus; name: string; message: string; details?: string };

function readPackageJson(path: string): any | undefined {
  try {
    return JSON.parse(readFileSync(path, "utf8"));
  } catch {
    return undefined;
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
    const expanded = expandHome(candidate);
    const resolved = isAbsolute(expanded) ? resolve(expanded) : resolve(cwd, expanded);
    if (existsSync(resolved)) return resolved;
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

export function collectDoctorDiagnostics(options: { agentDir?: string } = {}): Diagnostic[] {
  const diagnostics: Diagnostic[] = [];
  const agentDir = resolveAgentDir(options.agentDir);
  const piCli = resolvePiCliPath();
  const piPackage = resolvePiPackagePath();
  const writable = checkWritableDir(agentDir);
  const hoppiModule = resolveHoppiModulePath();
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

  diagnostics.push(existsSync(feedbackConfig)
    ? { status: "pass", name: "Feedback", message: `${feedbackConfig} exists (secrets not printed)` }
    : { status: "warn", name: "Feedback", message: "No ~/.oppi/feedback.json; feedback commands will use defaults or local drafts" });

  if (process.env.TERM_PROGRAM?.toLowerCase().includes("vscode")) {
    diagnostics.push({ status: "pass", name: "Terminal", message: "VS Code/Cursor terminal detected" });
  } else {
    diagnostics.push({ status: "warn", name: "Terminal", message: "Not running under VS Code/Cursor terminal, or TERM_PROGRAM is unset" });
  }

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

async function importHoppiModule(): Promise<any> {
  const modulePath = resolveHoppiModulePath();
  if (!modulePath) throw new Error(`Hoppi module not found. Run \`oppi mem install\`, install ${HOPPI_PACKAGE_NAME} from /settings:oppi → Memory, or set OPPI_HOPPI_MODULE.`);
  return import(pathToFileURL(modulePath).href);
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
  return `OPPi — opinionated Pi-powered coding agent\n\nUsage:\n  oppi [pi options] [@files...] [messages...]\n  oppi doctor [--json]\n  oppi update [--check] [--json]\n  oppi mem status|setup|install|dashboard [--json]\n  oppi plugin list|add|install|enable|disable|remove|doctor [--json]\n  oppi marketplace list|add|remove [--json]\n\nOPPi options:\n  --agent-dir <dir>       Use a specific OPPi/Pi agent dir for this run\n  --with-pi-extensions    Allow normal Pi extension discovery in addition to OPPi\n  --version, -v           Print OPPi CLI version\n  --help, -h              Show this help\n\nEnvironment:\n  OPPI_UPDATE_CHECK=0     Disable the daily npm update banner\n\nUpdates:\n  oppi update             Install the latest @oppiai/cli from npm\n  oppi update --check     Check npm and print the OPPi changelog link\n\nPlugin examples:\n  oppi plugin add ./my-pi-package --local\n  oppi plugin enable my-pi-package --yes\n  oppi marketplace add ./catalog.json\n\nDefaults:\n  - loads the bundled/local @oppiai/pi-package\n  - loads enabled OPPi plugins as additional Pi packages with -e\n  - disables unrelated Pi extension discovery unless --with-pi-extensions is set\n  - stores sessions/settings under OPPI_AGENT_DIR or ~/.oppi/agent\n\nExamples:\n  oppi\n  oppi "summarize this repository"\n  oppi -p "Reply ok"\n  OPPI_AGENT_DIR=/tmp/oppi-agent oppi doctor\n\nAll ordinary Pi flags not listed above are passed through unchanged.`;
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
  if (command.type === "plugin") {
    return runPluginCommand(command);
  }
  if (command.type === "marketplace") {
    return runMarketplaceCommand(command);
  }
  return launchPi(command);
}

function isMain(): boolean {
  return process.argv[1] ? resolve(process.argv[1]) === resolve(__filename) : false;
}

if (isMain()) {
  run(process.argv.slice(2)).then((code) => {
    process.exitCode = code;
  }).catch((error) => {
    console.error(error instanceof Error ? error.stack || error.message : String(error));
    process.exitCode = 1;
  });
}
