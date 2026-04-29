import { spawn, type ChildProcess } from "node:child_process";
import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import { delimiter, join } from "node:path";
import { homedir, tmpdir } from "node:os";
import type { ExtensionAPI } from "@mariozechner/pi-coding-agent";

const DEFAULT_MERIDIAN_BASE_URL = "http://127.0.0.1:3456";

type MeridianInstance = {
  config?: { host?: string; port?: number };
  close(): Promise<void>;
};

let meridianInstance: MeridianInstance | undefined;
let meridianProcess: ChildProcess | undefined;

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
  const { startProxyServer } = await import("@rynfar/meridian");
  const { host, port } = parseHostPort();
  meridianInstance = await startProxyServer({ host, port, silent: true });
  return "embedded package";
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

  if (process.env.OPPI_MERIDIAN_DISABLE_NPX !== "1") {
    try {
      const source = await startCommandMeridian("npx", ["-y", "@rynfar/meridian"], "npx @rynfar/meridian", 60_000);
      return `Started Meridian at ${meridianBaseUrl()} (${source})`;
    } catch (error) {
      failures.push(`npx @rynfar/meridian: ${error instanceof Error ? error.message : String(error)}`);
      meridianProcess?.kill();
      meridianProcess = undefined;
    }
  }

  throw new Error(`Could not start Meridian. Install @rynfar/meridian, run meridian externally, or set OPPI_MERIDIAN_COMMAND. Tried: ${failures.join("; ")}`);
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
    description: "Manage OPPi's optional Meridian Claude subscription bridge: /meridian start|stop|status.",
    handler: async (args, ctx) => {
      const action = args.trim().split(/\s+/)[0] || "status";
      try {
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
            ctx.ui.notify(`Meridian is not reachable at ${meridianBaseUrl()}. Run /meridian start or start it externally.`, "warning");
          }
          return;
        }

        ctx.ui.notify("Usage: /meridian start | stop | status", "info");
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
