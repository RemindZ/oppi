#!/usr/bin/env node
import { spawn, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, realpathSync } from "node:fs";
import { createRequire } from "node:module";
import { homedir } from "node:os";
import { dirname, isAbsolute, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const require = createRequire(import.meta.url);
const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const PACKAGE_NAME = "@oppiai/native";
const FALLBACK_PACKAGE_VERSION = "0.2.11";
const LEGACY_CLI_PACKAGE_NAME = "@oppiai/cli";
const LEGACY_CLI_BIN = "oppi";
const NATIVE_BINS = ["oppi-native", "oppi-rs"] as const;

type Env = Record<string, string | undefined>;
export type NativeBinaryName = "oppi-server" | "oppi-shell";
export type NativeBinarySource = "env" | "bin-dir" | "bundled" | "optional-package" | "cargo-target";

export type NativeBinaryResolution = {
  name: NativeBinaryName;
  available: boolean;
  path?: string;
  source?: NativeBinarySource;
  candidatePaths: string[];
  diagnostic?: string;
};

export type NativePackageStatus = {
  ok: boolean;
  packageName: typeof PACKAGE_NAME;
  packageVersion: string;
  platform: string;
  arch: string;
  bins: typeof NATIVE_BINS;
  legacyCli: {
    packageName: typeof LEGACY_CLI_PACKAGE_NAME;
    bin: typeof LEGACY_CLI_BIN;
    preserved: true;
  };
  server: NativeBinaryResolution;
  shell: NativeBinaryResolution;
  installExamples: string[];
};

type ResolveOptions = {
  env?: Env;
  cwd?: string;
  packageRoot?: string;
  platform?: string;
  arch?: string;
  includeCargoTarget?: boolean;
};

type ParsedCommand =
  | { type: "help" }
  | { type: "version"; json: boolean }
  | { type: "doctor"; json: boolean }
  | { type: "smoke"; json: boolean; mock: boolean; shellArgs: string[] }
  | { type: "server"; args: string[] }
  | { type: "shell"; args: string[] };

function packageRootFromDist(): string {
  return resolve(__dirname, "..");
}

function readPackageVersion(packageRoot = packageRootFromDist()): string {
  try {
    const value = JSON.parse(readFileSync(join(packageRoot, "package.json"), "utf8"));
    return typeof value.version === "string" ? value.version : FALLBACK_PACKAGE_VERSION;
  } catch {
    return FALLBACK_PACKAGE_VERSION;
  }
}

function expandHome(value: string): string {
  if (value === "~") return homedir();
  if (value.startsWith("~/") || value.startsWith("~\\")) return join(homedir(), value.slice(2));
  return value;
}

function resolvePath(value: string, cwd: string): string {
  const expanded = expandHome(value.trim());
  return isAbsolute(expanded) ? resolve(expanded) : resolve(cwd, expanded);
}

function executableName(name: NativeBinaryName, platform = process.platform): string {
  return platform === "win32" ? `${name}.exe` : name;
}

function platformKey(platform = process.platform, arch = process.arch): string {
  return `${platform}-${arch}`;
}

function optionalPlatformPackageName(platform = process.platform, arch = process.arch): string {
  return `@oppiai/native-${platform}-${arch}`;
}

function optionalPlatformPackageRoot(platform: string, arch: string): string | undefined {
  try {
    return dirname(require.resolve(`${optionalPlatformPackageName(platform, arch)}/package.json`));
  } catch {
    return undefined;
  }
}

function cargoTargetCandidates(name: NativeBinaryName, cwd: string, packageRoot: string, platform: string): string[] {
  const exe = executableName(name, platform);
  const repoRootFromPackage = resolve(packageRoot, "..", "..");
  return [
    resolve(cwd, "target", "release", exe),
    resolve(cwd, "target", "debug", exe),
    resolve(repoRootFromPackage, "target", "release", exe),
    resolve(repoRootFromPackage, "target", "debug", exe),
  ];
}

export function resolveNativeBinary(name: NativeBinaryName, options: ResolveOptions = {}): NativeBinaryResolution {
  const env = options.env ?? process.env;
  const cwd = options.cwd ?? process.cwd();
  const packageRoot = options.packageRoot ?? packageRootFromDist();
  const platform = options.platform ?? process.platform;
  const arch = options.arch ?? process.arch;
  const exe = executableName(name, platform);
  const key = platformKey(platform, arch);
  const directEnvName = name === "oppi-server" ? "OPPI_NATIVE_SERVER_BIN" : "OPPI_NATIVE_SHELL_BIN";
  const legacyEnvName = name === "oppi-server" ? "OPPI_SERVER_BIN" : "OPPI_SHELL_BIN";
  const directOverride = env[directEnvName]?.trim() || env[legacyEnvName]?.trim();

  if (directOverride) {
    const candidate = resolvePath(directOverride, cwd);
    return existsSync(candidate)
      ? { name, available: true, path: candidate, source: "env", candidatePaths: [candidate] }
      : {
        name,
        available: false,
        candidatePaths: [candidate],
        diagnostic: `${directEnvName}/${legacyEnvName} points to a missing ${name} binary: ${candidate}`,
      };
  }

  const candidates: Array<{ path: string; source: NativeBinarySource }> = [];
  const add = (path: string | undefined, source: NativeBinarySource) => {
    if (path) candidates.push({ path, source });
  };
  const binDir = env.OPPI_NATIVE_BIN_DIR?.trim();
  if (binDir) add(join(resolvePath(binDir, cwd), exe), "bin-dir");

  for (const dir of [join(packageRoot, "bin", key), join(packageRoot, "prebuilds", key)]) {
    add(join(dir, exe), "bundled");
  }

  const optionalRoot = optionalPlatformPackageRoot(platform, arch);
  if (optionalRoot) {
    for (const dir of [join(optionalRoot, "bin"), join(optionalRoot, "bin", key), join(optionalRoot, "prebuilds", key)]) {
      add(join(dir, exe), "optional-package");
    }
  }

  if (options.includeCargoTarget ?? true) {
    for (const candidate of cargoTargetCandidates(name, cwd, packageRoot, platform)) add(candidate, "cargo-target");
  }

  const uniqueCandidates = candidates.filter((candidate, index) => candidates.findIndex((other) => other.path === candidate.path) === index);
  const found = uniqueCandidates.find((candidate) => existsSync(candidate.path));
  if (found) {
    return {
      name,
      available: true,
      path: found.path,
      source: found.source,
      candidatePaths: uniqueCandidates.map((candidate) => candidate.path),
    };
  }
  return {
    name,
    available: false,
    candidatePaths: uniqueCandidates.map((candidate) => candidate.path),
    diagnostic: `No ${name} binary found for ${key}. Install a package tarball with bundled binaries, set ${directEnvName}, or build locally with \`cargo build -p ${name}\`.`,
  };
}

export function resolveNativeBinaries(options: ResolveOptions = {}): Pick<NativePackageStatus, "server" | "shell"> {
  return {
    server: resolveNativeBinary("oppi-server", options),
    shell: resolveNativeBinary("oppi-shell", options),
  };
}

export function getNativePackageStatus(options: ResolveOptions = {}): NativePackageStatus {
  const packageRoot = options.packageRoot ?? packageRootFromDist();
  const platform = options.platform ?? process.platform;
  const arch = options.arch ?? process.arch;
  const binaries = resolveNativeBinaries({ ...options, packageRoot, platform, arch });
  return {
    ok: binaries.server.available && binaries.shell.available,
    packageName: PACKAGE_NAME,
    packageVersion: readPackageVersion(packageRoot),
    platform,
    arch,
    bins: NATIVE_BINS,
    legacyCli: {
      packageName: LEGACY_CLI_PACKAGE_NAME,
      bin: LEGACY_CLI_BIN,
      preserved: true,
    },
    ...binaries,
    installExamples: [
      "npm install -g @oppiai/cli      # stable legacy Pi-backed `oppi`",
      "npm install -g @oppiai/native   # Rust-first preview `oppi-native`",
    ],
  };
}

function redactText(value: string): string {
  return value
    .replace(/(?:sk-[a-zA-Z0-9_-]{12,}|[a-zA-Z0-9_-]{20,}\.[a-zA-Z0-9_-]{20,}\.[a-zA-Z0-9_-]{20,})/g, "[redacted-secret]")
    .replace(/(token|secret|password|api[_-]?key)(["'`\s:=]+)([^\s"'`,}]+)/gi, "$1$2[redacted]");
}

function printJson(value: unknown): void {
  console.log(JSON.stringify(value, null, 2));
}

function formatBinary(binary: NativeBinaryResolution): string {
  if (binary.available) return `✓ ${binary.name}: ${binary.path} (${binary.source})`;
  return `✗ ${binary.name}: ${binary.diagnostic ?? "missing"}`;
}

function printDoctor(status: NativePackageStatus, json: boolean): void {
  if (json) {
    printJson(status);
    return;
  }
  console.log("OPPi native package preview");
  console.log(`package: ${status.packageName}@${status.packageVersion}`);
  console.log(`bins: ${status.bins.join(", ")} (legacy ${status.legacyCli.packageName} keeps \`${status.legacyCli.bin}\`)`);
  console.log(`platform: ${status.platform}-${status.arch}`);
  console.log(formatBinary(status.server));
  console.log(formatBinary(status.shell));
  if (!status.ok) {
    console.log("\nInstall/build guidance:");
    for (const example of status.installExamples) console.log(`  ${example}`);
    console.log("  cargo build -p oppi-server -p oppi-shell  # local development fallback");
  }
}

function nativeShellEnv(env: Env = process.env): Env {
  const agentDir = resolveAgentDir(undefined, env);
  mkdirSync(agentDir, { recursive: true });
  return {
    ...env,
    OPPI_EXPERIMENTAL_RUNTIME: "1",
    OPPI_AGENT_DIR: agentDir,
    PI_CODING_AGENT_DIR: agentDir,
  };
}

export function resolveAgentDir(input?: string, env: Env = process.env): string {
  const raw = input?.trim() || env.OPPI_AGENT_DIR?.trim() || env.PI_CODING_AGENT_DIR?.trim() || join(homedir(), ".oppi", "agent");
  return resolvePath(raw, process.cwd());
}

function hasOption(args: string[], name: string): boolean {
  return args.some((arg) => arg === name || arg.startsWith(`${name}=`));
}

function withShellDefaults(args: string[], serverBin: string): string[] {
  const next = [...args];
  if (!hasOption(next, "--server")) next.unshift("--server", serverBin);
  return next;
}

function windowsCmdShim(command: string, args: string[]): { command: string; args: string[] } {
  if (process.platform === "win32" && /\.(?:cmd|bat)$/i.test(command)) {
    return { command: "cmd.exe", args: ["/d", "/s", "/c", command, ...args] };
  }
  return { command, args };
}

function missingBinaryMessage(status: NativePackageStatus): string {
  const missing = [status.server, status.shell].filter((binary) => !binary.available).map((binary) => binary.name).join(", ");
  return `OPPi native package is missing required binaries (${missing}). Run \`oppi-native doctor\` for resolution details.`;
}

function parseBooleanFlag(args: string[], name: string): boolean {
  return args.includes(name);
}

export function parseNativeArgs(argv: string[]): ParsedCommand {
  const [first, ...rest] = argv;
  if (!first || first === "shell" || first === "run" || first === "tui") return { type: "shell", args: first ? rest : [] };
  if (first === "--help" || first === "-h" || first === "help") return { type: "help" };
  if (first === "--version" || first === "-v" || first === "version") return { type: "version", json: parseBooleanFlag(rest, "--json") };
  if (first === "doctor" || first === "status") return { type: "doctor", json: parseBooleanFlag(rest, "--json") };
  if (first === "smoke") {
    const json = parseBooleanFlag(rest, "--json");
    const mock = !rest.includes("--live");
    const shellArgs = rest.filter((arg) => !["--json", "--mock", "--live"].includes(arg));
    return { type: "smoke", json, mock, shellArgs };
  }
  if (first === "server") return { type: "server", args: rest.length > 0 ? rest : ["--stdio"] };
  return { type: "shell", args: argv };
}

function versionPayload(): { packageName: typeof PACKAGE_NAME; version: string; bins: typeof NATIVE_BINS; legacyCliBin: typeof LEGACY_CLI_BIN } {
  return { packageName: PACKAGE_NAME, version: readPackageVersion(), bins: NATIVE_BINS, legacyCliBin: LEGACY_CLI_BIN };
}

function printHelp(): void {
  console.log(`OPPi native — Rust/Ratatui UI launcher

Usage:
  oppi-native [oppi-shell options] [prompt]
  oppi-native shell [oppi-shell options]
  oppi-native server [--stdio]
  oppi-native doctor [--json]
  oppi-native smoke --mock [--json]
  oppi-native --version

Side-by-side install:
  npm install -g @oppiai/cli      # legacy Pi-backed package; owns \`oppi\`
  npm install -g @oppiai/native   # Rust/Ratatui native UI; owns \`oppi-native\` and \`oppi-rs\`

Binary overrides:
  OPPI_NATIVE_SHELL_BIN=/path/to/oppi-shell
  OPPI_NATIVE_SERVER_BIN=/path/to/oppi-server
  OPPI_NATIVE_BIN_DIR=/path/to/platform-bin-dir
`);
}

async function runShell(command: Extract<ParsedCommand, { type: "shell" }>, status: NativePackageStatus, env: Env): Promise<number> {
  if (!status.ok || !status.shell.path || !status.server.path) {
    console.error(missingBinaryMessage(status));
    return 1;
  }
  const target = windowsCmdShim(status.shell.path, withShellDefaults(command.args, status.server.path));
  const child = spawn(target.command, target.args, {
    stdio: "inherit",
    env: nativeShellEnv(env),
  });
  return new Promise((resolveExit) => {
    child.on("error", (error: Error) => {
      console.error(`OPPi failed to start oppi-shell: ${error.message}`);
      resolveExit(1);
    });
    child.on("exit", (code: number | null, signal: string | null) => {
      if (signal) resolveExit(signal === "SIGINT" ? 130 : signal === "SIGTERM" ? 143 : 1);
      else resolveExit(code ?? 0);
    });
  });
}

async function runServer(command: Extract<ParsedCommand, { type: "server" }>, status: NativePackageStatus, env: Env): Promise<number> {
  if (!status.server.available || !status.server.path) {
    console.error(status.server.diagnostic ?? "Missing oppi-server binary.");
    return 1;
  }
  const target = windowsCmdShim(status.server.path, command.args);
  const child = spawn(target.command, target.args, { stdio: "inherit", env: { ...env, OPPI_EXPERIMENTAL_RUNTIME: "1" } });
  return new Promise((resolveExit) => {
    child.on("error", (error: Error) => {
      console.error(`OPPi failed to start oppi-server: ${error.message}`);
      resolveExit(1);
    });
    child.on("exit", (code: number | null, signal: string | null) => {
      if (signal) resolveExit(signal === "SIGINT" ? 130 : signal === "SIGTERM" ? 143 : 1);
      else resolveExit(code ?? 0);
    });
  });
}

function runSmoke(command: Extract<ParsedCommand, { type: "smoke" }>, status: NativePackageStatus, env: Env): number {
  const started = Date.now();
  if (!status.ok || !status.shell.path || !status.server.path) {
    const payload = { ok: false, shellBin: status.shell.path, serverBin: status.server.path, diagnostics: [missingBinaryMessage(status)] };
    if (command.json) printJson(payload);
    else console.error(payload.diagnostics[0]);
    return 1;
  }
  const args = [
    ...(command.mock ? ["--mock"] : []),
    "--json",
    "--server",
    status.server.path,
    ...command.shellArgs,
    "OPPi native package smoke: reply ok.",
  ];
  const target = windowsCmdShim(status.shell.path, args);
  const result = spawnSync(target.command, target.args, {
    encoding: "utf8",
    timeout: 15_000,
    windowsHide: true,
    maxBuffer: 1024 * 1024,
    env: nativeShellEnv(env),
  });
  const stdout = redactText(String(result.stdout ?? ""));
  const stderr = redactText(String(result.stderr ?? ""));
  const ok = !result.error && result.status === 0;
  const payload = {
    ok,
    packageName: PACKAGE_NAME,
    shellBin: status.shell.path,
    serverBin: status.server.path,
    exitCode: result.status,
    durationMs: Date.now() - started,
    stdoutLineCount: stdout.split(/\r?\n/).filter(Boolean).length,
    diagnostics: [ok ? "oppi-native mock smoke completed" : result.error?.message || stderr.trim() || `oppi-shell exited ${result.status ?? "without a code"}`],
    stderr: stderr.trim() || undefined,
  };
  if (command.json) printJson(payload);
  else {
    console.log("OPPi native package smoke");
    console.log(`${ok ? "✓" : "✗"} ${payload.diagnostics[0]}`);
    console.log(`shell: ${status.shell.path}`);
    console.log(`server: ${status.server.path}`);
    if (!ok && stdout.trim()) console.log(stdout.trim().slice(-4_000));
    if (payload.stderr) console.error(payload.stderr);
  }
  return ok ? 0 : 1;
}

export async function run(argv = process.argv.slice(2), env: Env = process.env, cwd = process.cwd()): Promise<number> {
  const command = parseNativeArgs(argv);
  if (command.type === "help") {
    printHelp();
    return 0;
  }
  if (command.type === "version") {
    if (command.json) printJson(versionPayload());
    else console.log(`${PACKAGE_NAME}@${readPackageVersion()} (${NATIVE_BINS.join(", ")}; legacy ${LEGACY_CLI_BIN} preserved)`);
    return 0;
  }

  const status = getNativePackageStatus({ env, cwd });
  if (command.type === "doctor") {
    printDoctor(status, command.json);
    return status.ok ? 0 : 1;
  }
  if (command.type === "smoke") return runSmoke(command, status, env);
  if (command.type === "server") return runServer(command, status, env);
  return runShell(command, status, env);
}

export function isMain(argv1 = process.argv[1], mainFile = __filename): boolean {
  if (!argv1) return false;
  try {
    const left = realpathSync(argv1);
    const right = realpathSync(mainFile);
    return process.platform === "win32" ? left.toLowerCase() === right.toLowerCase() : left === right;
  } catch {
    return false;
  }
}

if (isMain()) {
  run().then((code) => {
    process.exitCode = code;
  }).catch((error: Error) => {
    console.error(error.message);
    process.exitCode = 1;
  });
}
