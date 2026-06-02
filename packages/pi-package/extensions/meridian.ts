import { spawn, type ChildProcess } from "node:child_process";
import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import { delimiter, isAbsolute, join, resolve } from "node:path";
import { homedir, tmpdir } from "node:os";
import { pathToFileURL } from "node:url";
import type { ExtensionAPI } from "@mariozechner/pi-coding-agent";

const DEFAULT_MERIDIAN_BASE_URL = "http://127.0.0.1:3456";
const MERIDIAN_PACKAGE_NAME = "@rynfar/meridian";
const MERIDIAN_PACKAGE_SPEC = process.env.OPPI_MERIDIAN_PACKAGE_SPEC || `${MERIDIAN_PACKAGE_NAME}@latest`;

type MeridianInstance = {
  config?: { host?: string; port?: number };
  close(): Promise<void>;
};

type MeridianModule = {
  startProxyServer: (options: { host: string; port: number; silent: boolean }) => Promise<MeridianInstance>;
};

type MeridianInstallResult =
  | { ok: true; modulePath: string; output: string }
  | { ok: false; error: string; output: string };

let meridianInstance: MeridianInstance | undefined;
let meridianProcess: ChildProcess | undefined;
let meridianInstallPromise: Promise<MeridianInstallResult> | undefined;

function meridianBaseUrl(): string {
  return (process.env.OPPI_MERIDIAN_BASE_URL || process.env.MERIDIAN_BASE_URL || DEFAULT_MERIDIAN_BASE_URL).replace(/\/+$/, "");
}

function meridianApiKey(): string {
  return process.env.OPPI_MERIDIAN_API_KEY || process.env.MERIDIAN_API_KEY || "x";
}

function meridianHeaders(): Record<string, string> {
  const headers: Record<string, string> = { "x-meridian-agent": "pi" };
  const profile = process.env.OPPI_MERIDIAN_PROFILE || process.env.MERIDIAN_DEFAULT_PROFILE;
  if (profile) headers["x-meridian-profile"] = profile;
  return headers;
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

function managedMeridianModulePath(packageName = MERIDIAN_PACKAGE_NAME): string {
  return join(packageRootFromNodeModules(join(managedPackagesDir(), "node_modules"), packageName), "dist", "server.js");
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
  if (process.platform === "win32") return { command: "cmd.exe", args: ["/d", "/s", "/c", "npm", ...args] };
  return { command: "npm", args };
}

function formatPath(value: string | undefined): string {
  if (!value) return "not configured";
  const home = homedir();
  return value.startsWith(home) ? `~${value.slice(home.length)}` : value;
}

async function loadMeridian(): Promise<{ module: MeridianModule; source: string }> {
  const candidates = [process.env.OPPI_MERIDIAN_MODULE, managedMeridianModulePath()]
    .filter((candidate): candidate is string => Boolean(candidate));

  for (const candidate of candidates) {
    try {
      const resolved = isAbsolute(candidate) ? candidate : resolve(candidate);
      if (existsSync(resolved)) {
        return { module: await import(pathToFileURL(resolved).href) as MeridianModule, source: formatPath(resolved) };
      }
    } catch {
      // Try the next managed/import candidate.
    }
  }

  return { module: await import(MERIDIAN_PACKAGE_NAME) as MeridianModule, source: MERIDIAN_PACKAGE_NAME };
}

async function installMeridianPackage(): Promise<MeridianInstallResult> {
  if (meridianInstallPromise) return meridianInstallPromise;
  meridianInstallPromise = new Promise<MeridianInstallResult>((resolveInstall) => {
    const root = ensureManagedPackageRoot();
    const args = ["install", MERIDIAN_PACKAGE_SPEC, "--save-exact", "--no-audit", "--no-fund"];
    let output = "";
    const append = (chunk: unknown) => {
      output += String(chunk);
      if (output.length > 12_000) output = output.slice(-12_000);
    };
    const npm = npmSpawnCommand(args);
    const child = spawn(npm.command, npm.args, {
      cwd: root,
      stdio: ["ignore", "pipe", "pipe"],
      windowsHide: true,
      env: { ...process.env, npm_config_loglevel: process.env.npm_config_loglevel ?? "warn" },
    });
    child.stdout?.on("data", append);
    child.stderr?.on("data", append);
    child.on("error", (error) => resolveInstall({ ok: false, error: error.message, output }));
    child.on("close", (code) => {
      const modulePath = managedMeridianModulePath();
      if (code === 0 && existsSync(modulePath)) resolveInstall({ ok: true, modulePath, output });
      else resolveInstall({ ok: false, error: `npm install exited with code ${code ?? "unknown"}`, output });
    });
  }).finally(() => {
    meridianInstallPromise = undefined;
  });
  return meridianInstallPromise;
}

async function installMeridianFromUi(ctx: any): Promise<void> {
  ctx.ui.notify(`Installing ${MERIDIAN_PACKAGE_SPEC} into ${formatPath(managedPackagesDir())}…`, "info");
  const result = await installMeridianPackage();
  if (!result.ok) {
    const tail = result.output.trim() ? `\n${result.output.trim().slice(-1_200)}` : "";
    ctx.ui.notify(`Meridian install failed: ${result.error}${tail}`, "warning");
    return;
  }
  ctx.ui.notify(`Installed Meridian bridge: ${formatPath(result.modulePath)}. Run /meridian start, then choose a Meridian Claude model.`, "info");
}

function registerMeridianProvider(pi: ExtensionAPI): void {
  const baseUrl = meridianBaseUrl();
  pi.registerProvider("meridian", {
    baseUrl,
    apiKey: meridianApiKey(),
    api: "anthropic-messages",
    headers: meridianHeaders(),
    models: [
      {
        id: "claude-sonnet-4-6",
        name: "Claude Sonnet 4.6 (Meridian)",
        reasoning: true,
        input: ["text", "image"],
        cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
        contextWindow: 200_000,
        maxTokens: 64_000,
      },
      {
        id: "claude-opus-4-6",
        name: "Claude Opus 4.6 (Meridian)",
        reasoning: true,
        input: ["text", "image"],
        cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
        contextWindow: 1_000_000,
        maxTokens: 32_768,
      },
      {
        id: "claude-haiku-4-5",
        name: "Claude Haiku 4.5 (Meridian)",
        reasoning: true,
        input: ["text", "image"],
        cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
        contextWindow: 200_000,
        maxTokens: 16_384,
      },
    ],
  });
}

function parseHostPort(): { host: string; port: number } {
  const url = new URL(meridianBaseUrl());
  return {
    host: url.hostname || "127.0.0.1",
    port: Number(url.port || (url.protocol === "https:" ? 443 : 80)),
  };
}

async function fetchHealth(): Promise<{ ok: boolean; text: string }> {
  const response = await fetch(`${meridianBaseUrl()}/health`, {
    headers: {
      authorization: `Bearer ${meridianApiKey()}`,
      "x-api-key": meridianApiKey(),
    },
  });
  const text = await response.text();
  return { ok: response.ok, text };
}

async function isReachable(): Promise<boolean> {
  try {
    return (await fetchHealth()).ok;
  } catch {
    return false;
  }
}

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function outputTail(output?: { stdout: string; stderr: string }): string {
  const combined = `${output?.stdout || ""}\n${output?.stderr || ""}`.trim();
  if (!combined) return "";
  const compact = combined.replace(/\s+/g, " ").trim();
  return `: ${compact.slice(-500)}`;
}

async function waitForMeridian(child: ChildProcess | undefined, timeoutMs = 12_000, output?: { stdout: string; stderr: string }): Promise<void> {
  const started = Date.now();
  let exit: { code: number | null; signal: NodeJS.Signals | null } | undefined;
  child?.once("exit", (code, signal) => {
    exit = { code, signal };
  });

  while (Date.now() - started < timeoutMs) {
    if (await isReachable()) return;
    if (exit) throw new Error(`Meridian process exited early (${exit.code ?? exit.signal ?? "unknown"})${outputTail(output)}`);
    await delay(400);
  }

  throw new Error(`Timed out waiting for Meridian at ${meridianBaseUrl()}${outputTail(output)}`);
}

async function startEmbeddedMeridian(): Promise<string> {
  process.env.MERIDIAN_DEFAULT_AGENT ||= "pi";
  process.env.MERIDIAN_PASSTHROUGH ||= "1";
  const { module, source } = await loadMeridian();
  const { host, port } = parseHostPort();
  meridianInstance = await module.startProxyServer({ host, port, silent: true });
  return `embedded managed bridge: ${source}`;
}

function windowsWhichShimDir(): string | undefined {
  if (process.platform !== "win32") return undefined;

  const dir = join(tmpdir(), "oppi-meridian-bin");
  const shim = join(dir, "which.cmd");
  try {
    mkdirSync(dir, { recursive: true });
    if (!existsSync(shim)) {
      writeFileSync(
        shim,
        [
          "@echo off",
          "for /f \"delims=\" %%I in ('where %* 2^>nul') do (",
          "  echo %%I",
          "  exit /b 0",
          ")",
          "exit /b 1",
          "",
        ].join("\r\n"),
        "utf8",
      );
    }
    return dir;
  } catch {
    return undefined;
  }
}

function augmentedPath(): string {
  const extras = [
    windowsWhichShimDir(),
    join(homedir(), ".local", "bin"),
    join(homedir(), "AppData", "Roaming", "npm"),
    "C:\\Program Files\\Git\\usr\\bin",
  ].filter(Boolean) as string[];
  return [...extras, process.env.PATH || ""].join(delimiter);
}

async function startCommandMeridian(command: string, args: string[], label: string, timeoutMs = 12_000): Promise<string> {
  const { host, port } = parseHostPort();
  const env = {
    ...process.env,
    PATH: augmentedPath(),
    MERIDIAN_DEFAULT_AGENT: process.env.MERIDIAN_DEFAULT_AGENT || "pi",
    MERIDIAN_PASSTHROUGH: process.env.MERIDIAN_PASSTHROUGH || "1",
    MERIDIAN_HOST: host,
    MERIDIAN_PORT: String(port),
  };

  const output = { stdout: "", stderr: "" };
  meridianProcess = spawn(command, args, {
    env,
    stdio: ["ignore", "pipe", "pipe"],
    shell: process.platform === "win32",
    windowsHide: true,
  });
  meridianProcess.stdout?.on("data", (chunk) => {
    output.stdout = `${output.stdout}${String(chunk)}`.slice(-4_000);
  });
  meridianProcess.stderr?.on("data", (chunk) => {
    output.stderr = `${output.stderr}${String(chunk)}`.slice(-4_000);
  });
  meridianProcess.unref?.();
  await waitForMeridian(meridianProcess, timeoutMs, output);
  return label;
}

async function startMeridian(): Promise<string> {
  if (meridianInstance || meridianProcess) return `Meridian is already managed by this session at ${meridianBaseUrl()}`;
  if (await isReachable()) return `Meridian is already reachable at ${meridianBaseUrl()}`;

  process.env.MERIDIAN_DEFAULT_AGENT ||= "pi";

  const failures: string[] = [];
  try {
    const source = await startEmbeddedMeridian();
    await waitForMeridian(undefined, 2_000).catch(() => undefined);
    return `Started Meridian at ${meridianBaseUrl()} (${source})`;
  } catch (error) {
    failures.push(`package import: ${error instanceof Error ? error.message : String(error)}`);
    meridianInstance = undefined;
  }

  const command = process.env.OPPI_MERIDIAN_COMMAND || "meridian";
  try {
    const source = await startCommandMeridian(command, [], command);
    return `Started Meridian at ${meridianBaseUrl()} (${source})`;
  } catch (error) {
    failures.push(`${command}: ${error instanceof Error ? error.message : String(error)}`);
    meridianProcess?.kill();
    meridianProcess = undefined;
  }

  if (process.env.OPPI_MERIDIAN_ALLOW_NPX === "1") {
    try {
      const source = await startCommandMeridian("npx", ["-y", MERIDIAN_PACKAGE_NAME], `npx ${MERIDIAN_PACKAGE_NAME}`, 60_000);
      return `Started Meridian at ${meridianBaseUrl()} (${source})`;
    } catch (error) {
      failures.push(`npx ${MERIDIAN_PACKAGE_NAME}: ${error instanceof Error ? error.message : String(error)}`);
      meridianProcess?.kill();
      meridianProcess = undefined;
    }
  } else {
    failures.push("npx fallback disabled by default; run /meridian install or set OPPI_MERIDIAN_ALLOW_NPX=1 for an explicit one-off fallback");
  }

  throw new Error(`Could not start Meridian. Install with /meridian install, run meridian externally, or set OPPI_MERIDIAN_COMMAND/OPPI_MERIDIAN_MODULE. Tried: ${failures.join("; ")}`);
}

async function stopMeridian(): Promise<string> {
  if (meridianInstance) {
    await meridianInstance.close();
    meridianInstance = undefined;
    return "Stopped embedded Meridian.";
  }
  if (meridianProcess) {
    meridianProcess.kill();
    meridianProcess = undefined;
    return "Stopped Meridian process started by OPPi.";
  }
  return "Meridian was not started by this OPPi session.";
}

export default function meridianExtension(pi: ExtensionAPI) {
  registerMeridianProvider(pi);

  pi.registerCommand("meridian", {
    description: "Manage OPPi's optional Meridian Claude subscription bridge: /meridian install|start|stop|status.",
    handler: async (args, ctx) => {
      const action = args.trim().split(/\s+/)[0] || "status";
      try {
        if (action === "install") {
          const accepted = await ctx.ui.confirm(
            "Install Meridian Claude bridge?",
            `Install ${MERIDIAN_PACKAGE_SPEC} into ${formatPath(managedPackagesDir())}? This enables the managed Claude subscription bridge without spawning npx automatically.`,
          );
          if (!accepted) {
            ctx.ui.notify("Okay — Meridian was not installed. You can run /meridian install later.", "info");
            return;
          }
          await installMeridianFromUi(ctx);
          return;
        }
        if (action === "start") {
          ctx.ui.notify(await startMeridian(), "info");
          return;
        }
        if (action === "stop") {
          ctx.ui.notify(await stopMeridian(), "info");
          return;
        }
        if (action === "status") {
          try {
            const health = await fetchHealth();
            ctx.ui.notify(health.ok ? `Meridian is reachable at ${meridianBaseUrl()}` : `Meridian returned ${health.text.slice(0, 160)}`, health.ok ? "info" : "warning");
          } catch {
            ctx.ui.notify(`Meridian is not reachable at ${meridianBaseUrl()}. Run /meridian install, /meridian start, or start it externally.`, "warning");
          }
          return;
        }

        ctx.ui.notify("Usage: /meridian install | start | stop | status", "info");
      } catch (error) {
        ctx.ui.notify(error instanceof Error ? error.message : String(error), "error");
      }
    },
  });

  pi.on("session_shutdown", async () => {
    if (meridianInstance) await meridianInstance.close().catch(() => undefined);
    meridianInstance = undefined;
    meridianProcess?.kill();
    meridianProcess = undefined;
  });
}
