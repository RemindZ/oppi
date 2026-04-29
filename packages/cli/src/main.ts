#!/usr/bin/env node
import { spawn } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, isAbsolute, join, resolve } from "node:path";
import { createRequire } from "node:module";
import { fileURLToPath, pathToFileURL } from "node:url";

const require = createRequire(import.meta.url);
const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);

export type OppiCommand =
  | { type: "help" }
  | { type: "version" }
  | { type: "doctor"; json: boolean; agentDir?: string }
  | { type: "mem"; subcommand: "status" | "setup" | "dashboard" | "open"; json: boolean }
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

export function resolveAgentDir(input?: string, env: Env = process.env): string {
  const raw = input?.trim() || env.OPPI_AGENT_DIR?.trim() || env.PI_CODING_AGENT_DIR?.trim() || join(homedir(), ".oppi", "agent");
  const expanded = expandHome(raw);
  return isAbsolute(expanded) ? resolve(expanded) : resolve(process.cwd(), expanded);
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

  try {
    const main = require.resolve("hoppi-memory");
    candidates.push(main);
  } catch {
    // optional in Stage 2
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
  if (first === "mem") {
    const json = rest.includes("--json");
    const sub = rest.find((item) => !item.startsWith("-")) ?? "status";
    if (sub === "status" || sub === "setup" || sub === "dashboard" || sub === "open") {
      return { type: "mem", subcommand: sub, json };
    }
    throw new Error(`Unknown oppi mem command: ${sub}`);
  }

  return { type: "launch", piArgs: remaining, agentDir, withPiExtensions };
}

export function buildPiArgs(command: Extract<OppiCommand, { type: "launch" }>, piPackagePath: string): string[] {
  const args: string[] = [];
  if (!command.withPiExtensions) args.push("--no-extensions");
  args.push("-e", piPackagePath, ...command.piArgs);
  return args;
}

function launchPi(command: Extract<OppiCommand, { type: "launch" }>): Promise<number> {
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

  const child = spawn(process.execPath, [piCli, ...buildPiArgs(command, piPackage)], {
    stdio: "inherit",
    env: {
      ...process.env,
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
    : { status: "warn", name: "Hoppi", message: "Hoppi module not found; set OPPI_HOPPI_MODULE or build hoppi-memory for memory features" });

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

async function importHoppiModule(): Promise<any> {
  const modulePath = resolveHoppiModulePath();
  if (!modulePath) throw new Error("Hoppi module not found. Build hoppi-memory or set OPPI_HOPPI_MODULE.");
  return import(pathToFileURL(modulePath).href);
}

async function runMemCommand(command: Extract<OppiCommand, { type: "mem" }>): Promise<number> {
  try {
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
    if (command.json) console.log(JSON.stringify({ ok: false, error: message }, null, 2));
    else console.error(`OPPi mem ${command.subcommand} failed: ${message}`);
    return 1;
  }
}

function helpText(): string {
  return `OPPi — opinionated Pi-powered coding agent\n\nUsage:\n  oppi [pi options] [@files...] [messages...]\n  oppi doctor [--json]\n  oppi mem status|setup|dashboard [--json]\n\nOPPi options:\n  --agent-dir <dir>       Use a specific OPPi/Pi agent dir for this run\n  --with-pi-extensions    Allow normal Pi extension discovery in addition to OPPi\n  --version, -v           Print OPPi CLI version\n  --help, -h              Show this help\n\nDefaults:\n  - loads the bundled/local @oppiai/pi-package\n  - disables unrelated Pi extension discovery unless --with-pi-extensions is set\n  - stores sessions/settings under OPPI_AGENT_DIR or ~/.oppi/agent\n\nExamples:\n  oppi\n  oppi "summarize this repository"\n  oppi -p "Reply ok"\n  OPPI_AGENT_DIR=/tmp/oppi-agent oppi doctor\n\nAll ordinary Pi flags not listed above are passed through unchanged.`;
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
  if (command.type === "mem") {
    return runMemCommand(command);
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
