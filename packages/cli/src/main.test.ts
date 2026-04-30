import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { homedir, tmpdir } from "node:os";
import { join, resolve } from "node:path";
import test from "node:test";
import { buildPiArgs, parseOppiArgs, resolveAgentDir } from "./main.js";
import { parseMarketplaceCommand, parsePluginCommand, resolveEnabledPluginSources } from "./plugins.js";

function tempDir(name: string): string {
  return mkdtempSync(join(tmpdir(), `oppi-${name}-`));
}

test("parseOppiArgs handles OPPi-owned commands", () => {
  assert.deepEqual(parseOppiArgs(["--version"]), { type: "version" });
  assert.deepEqual(parseOppiArgs(["doctor", "--json"]), { type: "doctor", json: true, agentDir: undefined });
  assert.deepEqual(parseOppiArgs(["--agent-dir", "./agent", "doctor"]), { type: "doctor", json: false, agentDir: "./agent" });
  assert.deepEqual(parseOppiArgs(["mem", "status", "--json"]), { type: "mem", subcommand: "status", json: true });
  assert.deepEqual(parseOppiArgs(["mem", "install", "--json"]), { type: "mem", subcommand: "install", json: true });
  assert.deepEqual(parseOppiArgs(["plugin", "list", "--json"]), { type: "plugin", subcommand: "list", json: true, scope: undefined });
  assert.deepEqual(parseOppiArgs(["marketplace", "list", "--json"]), { type: "marketplace", subcommand: "list", json: true });
});

test("parse plugin and marketplace commands", () => {
  assert.deepEqual(parsePluginCommand(["add", "./plugin", "--local", "--name", "demo"]), {
    type: "plugin",
    subcommand: "add",
    source: "./plugin",
    name: "demo",
    scope: "project",
    enable: false,
    yes: false,
    json: false,
  });
  assert.deepEqual(parsePluginCommand(["enable", "demo", "--yes", "--json"]), {
    type: "plugin",
    subcommand: "enable",
    name: "demo",
    scope: undefined,
    yes: true,
    json: true,
  });
  assert.deepEqual(parseMarketplaceCommand(["add", "./catalog.json", "--name=local"]), {
    type: "marketplace",
    subcommand: "add",
    url: "./catalog.json",
    name: "local",
    json: false,
  });
});

test("parseOppiArgs strips OPPi flags and passes Pi args through", () => {
  assert.deepEqual(parseOppiArgs(["--agent-dir", "./tmp-agent", "--with-pi-extensions", "--model", "sonnet", "hello"]), {
    type: "launch",
    agentDir: "./tmp-agent",
    withPiExtensions: true,
    piArgs: ["--model", "sonnet", "hello"],
  });
});

test("resolveAgentDir uses documented precedence", () => {
  assert.equal(resolveAgentDir("./explicit", { OPPI_AGENT_DIR: "/oppi", PI_CODING_AGENT_DIR: "/pi" }), resolve("./explicit"));
  assert.equal(resolveAgentDir(undefined, { OPPI_AGENT_DIR: "/oppi", PI_CODING_AGENT_DIR: "/pi" }), resolve("/oppi"));
  assert.equal(resolveAgentDir(undefined, { PI_CODING_AGENT_DIR: "/pi" }), resolve("/pi"));
  assert.equal(resolveAgentDir(undefined, {}), join(homedir(), ".oppi", "agent"));
});

test("buildPiArgs isolates extension loading by default", () => {
  const isolated = buildPiArgs({ type: "launch", piArgs: ["-p", "ok"], withPiExtensions: false }, "/pkg");
  assert.deepEqual(isolated, ["--no-extensions", "-e", "/pkg", "-p", "ok"]);

  const withPlugins = buildPiArgs({ type: "launch", piArgs: ["-p", "ok"], withPiExtensions: false }, "/pkg", ["/plugin-a", "npm:@scope/plugin"]);
  assert.deepEqual(withPlugins, ["--no-extensions", "-e", "/pkg", "-e", "/plugin-a", "-e", "npm:@scope/plugin", "-p", "ok"]);

  const withExtensions = buildPiArgs({ type: "launch", piArgs: [], withPiExtensions: true }, "/pkg");
  assert.deepEqual(withExtensions, ["-e", "/pkg"]);
});

test("plugin command smoke stores disabled-by-default plugins and explicit trust enables them", () => {
  const root = tempDir("plugins");
  const oppiHome = join(root, "oppi-home");
  const pluginDir = join(root, "demo-plugin");
  mkdirSync(pluginDir, { recursive: true });
  writeFileSync(join(pluginDir, "package.json"), JSON.stringify({
    name: "demo-plugin",
    version: "1.0.0",
    description: "Demo plugin",
    pi: { extensions: ["./extensions/demo.ts"] },
  }), "utf8");

  const baseEnv = { ...process.env, OPPI_HOME: oppiHome };
  const add = spawnSync(process.execPath, [resolve("dist", "main.js"), "plugin", "add", pluginDir, "--json"], { encoding: "utf8", env: baseEnv });
  assert.equal(add.status, 0, add.stderr);
  assert.equal(JSON.parse(add.stdout).plugin.enabled, false);

  const blocked = spawnSync(process.execPath, [resolve("dist", "main.js"), "plugin", "enable", "demo-plugin", "--json"], { encoding: "utf8", env: baseEnv });
  assert.equal(blocked.status, 1);
  assert.match(JSON.parse(blocked.stdout).error, /without explicit trust/);

  const enabled = spawnSync(process.execPath, [resolve("dist", "main.js"), "plugin", "enable", "demo-plugin", "--yes", "--json"], { encoding: "utf8", env: baseEnv });
  assert.equal(enabled.status, 0, enabled.stderr);
  assert.deepEqual(resolveEnabledPluginSources({ env: { OPPI_HOME: oppiHome }, cwd: root }), [pluginDir]);
});

test("Claude-style marketplace entries fail with agent handoff prompt", () => {
  const root = tempDir("claude-marketplace");
  const oppiHome = join(root, "oppi-home");
  const catalog = join(root, "claude-catalog.json");
  writeFileSync(catalog, JSON.stringify({
    name: "claude-store-smoke",
    plugins: [{
      name: "claude-mcp-demo",
      description: "Claude MCP plugin shape",
      mcpServers: { demo: { command: "node", args: ["server.js"] } },
    }],
  }), "utf8");

  const baseEnv = { ...process.env, OPPI_HOME: oppiHome };
  const addMarketplace = spawnSync(process.execPath, [resolve("dist", "main.js"), "marketplace", "add", catalog, "--json"], { encoding: "utf8", env: baseEnv });
  assert.equal(addMarketplace.status, 0, addMarketplace.stderr);
  assert.equal(JSON.parse(addMarketplace.stdout).incompatiblePlugins, 1);

  const addPlugin = spawnSync(process.execPath, [resolve("dist", "main.js"), "plugin", "add", "claude-mcp-demo", "--json"], { encoding: "utf8", env: baseEnv });
  assert.equal(addPlugin.status, 1);
  const parsed = JSON.parse(addPlugin.stdout);
  assert.match(parsed.error, /does not look Pi\/OPPi-compatible/);
  assert.match(parsed.compatibility.agentHandoffPrompt, /Port the Claude marketplace plugin/);
});

test("CLI smoke: --version prints package version", () => {
  const result = spawnSync(process.execPath, [resolve("dist", "main.js"), "--version"], { encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout.trim(), /^\d+\.\d+\.\d+/);
});

test("CLI smoke: doctor --json uses configured paths without printing secrets", () => {
  const root = tempDir("doctor");
  const agentDir = join(root, "agent");
  const fakePi = join(root, "fake-pi.mjs");
  const fakePackage = join(root, "pi-package");
  mkdirSync(fakePackage, { recursive: true });
  writeFileSync(fakePi, "process.exit(0);\n", "utf8");
  writeFileSync(join(fakePackage, "package.json"), "{\"name\":\"@oppiai/pi-package\"}\n", "utf8");

  const result = spawnSync(process.execPath, [resolve("dist", "main.js"), "doctor", "--json"], {
    encoding: "utf8",
    env: {
      ...process.env,
      OPPI_AGENT_DIR: agentDir,
      OPPI_HOME: join(root, "oppi-home"),
      OPPI_PI_CLI: fakePi,
      OPPI_PI_PACKAGE: fakePackage,
      OPPI_FEEDBACK_TOKEN: "super-secret-token-for-test",
    },
  });

  assert.equal(result.status, 0, result.stderr);
  const parsed = JSON.parse(result.stdout);
  assert.equal(parsed.ok, true);
  assert.equal(result.stdout.includes("super-secret-token-for-test"), false);
});

test("CLI smoke: print-mode launch forwards args and isolated env to Pi", () => {
  const root = tempDir("launch");
  const agentDir = join(root, "agent");
  const capture = join(root, "capture.json");
  const fakePi = join(root, "fake-pi.mjs");
  const fakePackage = join(root, "pi-package");
  mkdirSync(fakePackage, { recursive: true });
  writeFileSync(join(fakePackage, "package.json"), "{\"name\":\"@oppiai/pi-package\"}\n", "utf8");
  writeFileSync(fakePi, `import { writeFileSync } from "node:fs";\nwriteFileSync(process.env.OPPI_FAKE_PI_CAPTURE, JSON.stringify({ argv: process.argv.slice(2), OPPI_AGENT_DIR: process.env.OPPI_AGENT_DIR, PI_CODING_AGENT_DIR: process.env.PI_CODING_AGENT_DIR }));\n`, "utf8");

  const result = spawnSync(process.execPath, [resolve("dist", "main.js"), "--agent-dir", agentDir, "-p", "Reply ok"], {
    encoding: "utf8",
    env: {
      ...process.env,
      OPPI_HOME: join(root, "oppi-home"),
      OPPI_PI_CLI: fakePi,
      OPPI_PI_PACKAGE: fakePackage,
      OPPI_FAKE_PI_CAPTURE: capture,
    },
  });

  assert.equal(result.status, 0, result.stderr);
  const captured = JSON.parse(readFileSync(capture, "utf8"));
  assert.deepEqual(captured.argv, ["--no-extensions", "-e", fakePackage, "-p", "Reply ok"]);
  assert.equal(captured.OPPI_AGENT_DIR, agentDir);
  assert.equal(captured.PI_CODING_AGENT_DIR, agentDir);
});

test("CLI smoke: launch appends enabled OPPi plugin sources", () => {
  const root = tempDir("launch-plugin");
  const agentDir = join(root, "agent");
  const oppiHome = join(root, "oppi-home");
  const capture = join(root, "capture.json");
  const fakePi = join(root, "fake-pi.mjs");
  const fakePackage = join(root, "pi-package");
  const pluginDir = join(root, "plugin-package");
  mkdirSync(fakePackage, { recursive: true });
  mkdirSync(pluginDir, { recursive: true });
  mkdirSync(oppiHome, { recursive: true });
  writeFileSync(join(fakePackage, "package.json"), "{\"name\":\"@oppiai/pi-package\"}\n", "utf8");
  writeFileSync(join(pluginDir, "package.json"), "{\"name\":\"plugin-package\",\"pi\":{\"extensions\":[\"./index.ts\"]}}\n", "utf8");
  writeFileSync(join(oppiHome, "plugin-lock.json"), JSON.stringify({
    version: 1,
    plugins: [{
      name: "plugin-package",
      source: pluginDir,
      sourceType: "local",
      enabled: true,
      trusted: true,
      addedAt: new Date().toISOString(),
      updatedAt: new Date().toISOString(),
    }],
  }), "utf8");
  writeFileSync(fakePi, `import { writeFileSync } from "node:fs";\nwriteFileSync(process.env.OPPI_FAKE_PI_CAPTURE, JSON.stringify({ argv: process.argv.slice(2) }));\n`, "utf8");

  const result = spawnSync(process.execPath, [resolve("dist", "main.js"), "--agent-dir", agentDir, "-p", "Reply ok"], {
    encoding: "utf8",
    env: {
      ...process.env,
      OPPI_HOME: oppiHome,
      OPPI_PI_CLI: fakePi,
      OPPI_PI_PACKAGE: fakePackage,
      OPPI_FAKE_PI_CAPTURE: capture,
    },
  });

  assert.equal(result.status, 0, result.stderr);
  const captured = JSON.parse(readFileSync(capture, "utf8"));
  assert.deepEqual(captured.argv, ["--no-extensions", "-e", fakePackage, "-e", pluginDir, "-p", "Reply ok"]);
});
