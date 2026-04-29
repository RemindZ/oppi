import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { homedir, tmpdir } from "node:os";
import { join, resolve } from "node:path";
import test from "node:test";
import { buildPiArgs, parseOppiArgs, resolveAgentDir } from "./main.js";

function tempDir(name: string): string {
  return mkdtempSync(join(tmpdir(), `oppi-${name}-`));
}

test("parseOppiArgs handles OPPi-owned commands", () => {
  assert.deepEqual(parseOppiArgs(["--version"]), { type: "version" });
  assert.deepEqual(parseOppiArgs(["doctor", "--json"]), { type: "doctor", json: true, agentDir: undefined });
  assert.deepEqual(parseOppiArgs(["--agent-dir", "./agent", "doctor"]), { type: "doctor", json: false, agentDir: "./agent" });
  assert.deepEqual(parseOppiArgs(["mem", "status", "--json"]), { type: "mem", subcommand: "status", json: true });
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

  const withExtensions = buildPiArgs({ type: "launch", piArgs: [], withPiExtensions: true }, "/pkg");
  assert.deepEqual(withExtensions, ["-e", "/pkg"]);
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
  writeFileSync(join(fakePackage, "package.json"), "{\"name\":\"@oppi/pi-package\"}\n", "utf8");

  const result = spawnSync(process.execPath, [resolve("dist", "main.js"), "doctor", "--json"], {
    encoding: "utf8",
    env: {
      ...process.env,
      OPPI_AGENT_DIR: agentDir,
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
  writeFileSync(join(fakePackage, "package.json"), "{\"name\":\"@oppi/pi-package\"}\n", "utf8");
  writeFileSync(fakePi, `import { writeFileSync } from "node:fs";\nwriteFileSync(process.env.OPPI_FAKE_PI_CAPTURE, JSON.stringify({ argv: process.argv.slice(2), OPPI_AGENT_DIR: process.env.OPPI_AGENT_DIR, PI_CODING_AGENT_DIR: process.env.PI_CODING_AGENT_DIR }));\n`, "utf8");

  const result = spawnSync(process.execPath, [resolve("dist", "main.js"), "--agent-dir", agentDir, "-p", "Reply ok"], {
    encoding: "utf8",
    env: {
      ...process.env,
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
