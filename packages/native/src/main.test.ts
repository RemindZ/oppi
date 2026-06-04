import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { chmodSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import {
  expectedNativePackageFiles,
  getNativePackageStatus,
  isMain,
  parseNativeArgs,
  resolveNativeBinaries,
  verifyNativePackageFileList,
} from "./main.js";

const __dirname = dirname(fileURLToPath(import.meta.url));

function tempDir(name: string): string {
  return mkdtempSync(join(tmpdir(), `oppi-native-${name}-`));
}

function executableName(name: string): string {
  return process.platform === "win32" ? `${name}.exe` : name;
}

function createFakeBinary(path: string): void {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, "fake", "utf8");
  try {
    chmodSync(path, 0o755);
  } catch {
    // Best effort on Windows.
  }
}

function createFakeCommand(root: string, name: string, body: string): string {
  const script = join(root, `${name}.mjs`);
  writeFileSync(script, body, "utf8");
  if (process.platform === "win32") {
    const cmd = join(root, `${name}.cmd`);
    writeFileSync(cmd, `@echo off\r\n"${process.execPath}" "${script}" %*\r\n`, "utf8");
    return cmd;
  }
  const bin = join(root, name);
  writeFileSync(bin, `#!/usr/bin/env sh\nexec "${process.execPath}" "${script}" "$@"\n`, "utf8");
  chmodSync(bin, 0o755);
  return bin;
}

function createFakeNodeEntrypoint(root: string, name: string, body: string): string {
  const script = join(root, `${name}.mjs`);
  writeFileSync(script, body, "utf8");
  return script;
}

test("package metadata preserves the legacy oppi bin for @oppiai/cli", () => {
  const nativePackage = JSON.parse(readFileSync(resolve(__dirname, "..", "package.json"), "utf8"));
  const cliPackage = JSON.parse(readFileSync(resolve(__dirname, "..", "..", "cli", "package.json"), "utf8"));

  assert.equal(nativePackage.name, "@oppiai/native");
  assert.deepEqual(Object.keys(nativePackage.bin).sort(), ["oppi-native", "oppi-rs"]);
  assert.equal(Object.prototype.hasOwnProperty.call(nativePackage.bin, "oppi"), false);
  assert.equal(nativePackage.scripts["verify:pack"], "node scripts/verify-pack.mjs");
  assert.deepEqual(cliPackage.bin, { oppi: "./dist/main.js" });
});

test("parseNativeArgs routes preview commands and defaults to shell passthrough", () => {
  assert.deepEqual(parseNativeArgs(["--help"]), { type: "help" });
  assert.deepEqual(parseNativeArgs(["doctor", "--json"]), { type: "doctor", json: true });
  assert.deepEqual(parseNativeArgs(["status"]), { type: "doctor", json: false });
  assert.deepEqual(parseNativeArgs(["smoke", "--json"]), { type: "smoke", json: true, mock: false, shellArgs: [] });
  assert.deepEqual(parseNativeArgs(["smoke", "--mock", "--json"]), { type: "smoke", json: true, mock: true, shellArgs: [] });
  assert.deepEqual(parseNativeArgs(["server"]), { type: "server", args: ["--stdio"] });
  assert.deepEqual(parseNativeArgs(["--mock", "hello"]), { type: "shell", args: ["--mock", "hello"] });
});

test("parseNativeArgs routes management commands to the CLI bridge", () => {
  for (const argv of [
    ["update", "--check", "--json"],
    ["mem", "status", "--json"],
    ["natives", "status", "--json"],
    ["sandbox", "status", "--json"],
    ["resume", "thread-1", "--json"],
    ["runtime-loop", "smoke", "--json"],
    ["runtime-worker", "smoke", "--json"],
    ["plugin", "list", "--json"],
    ["plugin", "materialize", "plugin-pack", "--local", "--yes"],
    ["marketplace", "list", "--json"],
  ]) {
    assert.deepEqual(parseNativeArgs(argv), { type: "managed", args: argv });
  }
});

test("resolveNativeBinaries prefers env overrides and bundled package binaries", () => {
  const root = tempDir("resolution");
  const envShell = join(root, process.platform === "win32" ? "shell.cmd" : "shell");
  const envServer = join(root, process.platform === "win32" ? "server.cmd" : "server");
  createFakeBinary(envShell);
  createFakeBinary(envServer);

  const explicit = resolveNativeBinaries({ env: { OPPI_NATIVE_SHELL_BIN: envShell, OPPI_NATIVE_SERVER_BIN: envServer }, cwd: root, packageRoot: root, includeCargoTarget: false });
  assert.equal(explicit.shell.path, envShell);
  assert.equal(explicit.shell.source, "env");
  assert.equal(explicit.server.path, envServer);
  assert.equal(explicit.server.source, "env");

  const platformDir = join(root, "bin", `${process.platform}-${process.arch}`);
  const bundledShell = join(platformDir, executableName("oppi-shell"));
  const bundledServer = join(platformDir, executableName("oppi-server"));
  createFakeBinary(bundledShell);
  createFakeBinary(bundledServer);
  const bundled = resolveNativeBinaries({ env: {}, cwd: root, packageRoot: root, includeCargoTarget: false });
  assert.equal(bundled.shell.path, bundledShell);
  assert.equal(bundled.shell.source, "bundled");
  assert.equal(bundled.server.path, bundledServer);
  assert.equal(bundled.server.source, "bundled");
});

test("doctor status advertises side-by-side install posture", () => {
  const root = tempDir("doctor");
  const platformDir = join(root, "bin", `${process.platform}-${process.arch}`);
  createFakeBinary(join(platformDir, executableName("oppi-shell")));
  createFakeBinary(join(platformDir, executableName("oppi-server")));
  writeFileSync(join(root, "package.json"), JSON.stringify({ name: "@oppiai/native", version: "9.9.9" }), "utf8");

  const status = getNativePackageStatus({ env: {}, cwd: root, packageRoot: root, includeCargoTarget: false });
  assert.equal(status.ok, true);
  assert.equal(status.packageName, "@oppiai/native");
  assert.equal(status.packageVersion, "9.9.9");
  assert.deepEqual(status.bins, ["oppi-native", "oppi-rs"]);
  assert.deepEqual(status.legacyCli, { packageName: "@oppiai/cli", bin: "oppi", preserved: true });
  assert.match(status.installExamples.join("\n"), /npm install -g @oppiai\/cli/);
});

test("native pack verifier requires platform binaries and manifest", () => {
  const required = expectedNativePackageFiles({ platform: "win32", arch: "x64" });
  assert.deepEqual(required, [
    "package.json",
    "README.md",
    "dist/main.js",
    "dist/main.d.ts",
    "bin/win32-x64/manifest.json",
    "bin/win32-x64/oppi-server.exe",
    "bin/win32-x64/oppi-shell.exe",
  ]);

  const missingShell = verifyNativePackageFileList(
    required
      .filter((path) => path !== "bin/win32-x64/oppi-shell.exe")
      .map((path) => ({ path: `package/${path}` })),
    { platform: "win32", arch: "x64" },
  );

  assert.equal(missingShell.ok, false);
  assert.deepEqual(missingShell.missingFiles, ["bin/win32-x64/oppi-shell.exe"]);
  assert.match(missingShell.diagnostics.join("\n"), /missing required native package files/);

  const complete = verifyNativePackageFileList(required, { platform: "win32", arch: "x64" });
  assert.equal(complete.ok, true);
  assert.deepEqual(complete.missingFiles, []);
});

test("oppi-native smoke runs the configured shell without leaking tokens", () => {
  const root = tempDir("smoke");
  const fakeServer = createFakeCommand(root, "fake-oppi-server", "console.log('fake server should not be launched in smoke');\n");
  const fakeShell = createFakeCommand(
    root,
    "fake-oppi-shell",
    `const argv = process.argv.slice(2);\nif (!argv.includes('--mock')) { console.error('missing --mock'); process.exit(2); }\nconst serverIndex = argv.indexOf('--server');\nif (serverIndex < 0 || argv[serverIndex + 1] !== ${JSON.stringify(fakeServer)}) { console.error('missing expected server'); process.exit(3); }\nconsole.log(JSON.stringify({ fakeShell: true, args: argv }));\n`,
  );

  const result = spawnSync(process.execPath, [resolve(__dirname, "main.js"), "smoke", "--mock", "--json"], {
    encoding: "utf8",
    env: { ...process.env, OPPI_NATIVE_SHELL_BIN: fakeShell, OPPI_NATIVE_SERVER_BIN: fakeServer, OPPI_SERVER_AUTH_TOKEN: "super-secret-native-token" },
  });
  assert.equal(result.status, 0, result.stderr || result.stdout);
  assert.equal(result.stdout.includes("super-secret-native-token"), false);
  const parsed = JSON.parse(result.stdout);
  assert.equal(parsed.ok, true);
  assert.equal(parsed.packageName, "@oppiai/native");
  assert.equal(parsed.shellBin, fakeShell);
  assert.equal(parsed.serverBin, fakeServer);
  assert.match(parsed.diagnostics.join("\n"), /mock smoke completed/);
});

test("oppi-native smoke defaults to live Codex provider", () => {
  const root = tempDir("smoke-live");
  const fakeServer = createFakeCommand(root, "fake-oppi-server", "console.error('server should not be launched directly'); process.exit(9);\n");
  const fakeShell = createFakeCommand(
    root,
    "fake-oppi-shell",
    `const argv = process.argv.slice(2);\nif (argv.includes('--mock')) { console.error('unexpected --mock'); process.exit(2); }\nif (argv[argv.indexOf('--provider') + 1] !== 'openai-codex') { console.error('missing openai-codex provider'); process.exit(3); }\nif (argv[argv.indexOf('--model') + 1] !== 'gpt-5.5') { console.error('missing gpt-5.5 model'); process.exit(4); }\nconst serverIndex = argv.indexOf('--server');\nif (serverIndex < 0 || argv[serverIndex + 1] !== ${JSON.stringify(fakeServer)}) { console.error('missing expected server'); process.exit(5); }\nconsole.log(JSON.stringify({ fakeShell: true, args: argv }));\n`,
  );

  const result = spawnSync(process.execPath, [resolve(__dirname, "main.js"), "smoke", "--json"], {
    encoding: "utf8",
    env: { ...process.env, OPPI_NATIVE_SHELL_BIN: fakeShell, OPPI_NATIVE_SERVER_BIN: fakeServer, OPPI_RUNTIME_WORKER_PROVIDER: "", OPPI_RUNTIME_WORKER_MODEL: "" },
  });
  assert.equal(result.status, 0, result.stderr || result.stdout);
  const parsed = JSON.parse(result.stdout);
  assert.equal(parsed.ok, true);
  assert.match(parsed.diagnostics.join("\n"), /live smoke completed/);
});

test("oppi-native shell defaults to real Codex provider instead of mock", () => {
  const root = tempDir("shell-default-provider");
  const fakeServer = createFakeCommand(root, "fake-oppi-server", "console.error('server should not be launched directly'); process.exit(9);\n");
  const fakeShell = createFakeCommand(
    root,
    "fake-oppi-shell",
    "console.log(JSON.stringify({ args: process.argv.slice(2) }));\n",
  );

  const result = spawnSync(process.execPath, [resolve(__dirname, "main.js")], {
    encoding: "utf8",
    env: { ...process.env, OPPI_NATIVE_SHELL_BIN: fakeShell, OPPI_NATIVE_SERVER_BIN: fakeServer, OPPI_RUNTIME_WORKER_PROVIDER: "", OPPI_RUNTIME_WORKER_MODEL: "" },
  });
  assert.equal(result.status, 0, result.stderr || result.stdout);
  const parsed = JSON.parse(result.stdout);
  assert.equal(parsed.args.includes("--mock"), false);
  assert.equal(parsed.args[parsed.args.indexOf("--provider") + 1], "openai-codex");
  assert.equal(parsed.args[parsed.args.indexOf("--model") + 1], "gpt-5.5");
});

test("oppi-native delegates management commands to the configured CLI bridge", () => {
  const root = tempDir("managed-cli");
  const bridge = createFakeNodeEntrypoint(
    root,
    "fake-oppi-cli",
    "console.log(JSON.stringify({ delegated: true, args: process.argv.slice(2), cwd: process.cwd() }));\n",
  );

  const result = spawnSync(process.execPath, [resolve(__dirname, "main.js"), "plugin", "list", "--json"], {
    encoding: "utf8",
    cwd: root,
    env: { ...process.env, OPPI_CLI_BIN: bridge },
  });

  assert.equal(result.status, 0, result.stderr || result.stdout);
  const parsed = JSON.parse(result.stdout);
  assert.deepEqual(
    { delegated: parsed.delegated, args: parsed.args },
    { delegated: true, args: ["plugin", "list", "--json"] },
  );
  assert.equal(realpathSync(parsed.cwd), realpathSync(root));
});

test("oppi-native management commands fail closed when the CLI bridge is unavailable", () => {
  const root = tempDir("missing-managed-cli");
  const missing = join(root, process.platform === "win32" ? "missing.cmd" : "missing");
  const result = spawnSync(process.execPath, [resolve(__dirname, "main.js"), "plugin", "list"], {
    encoding: "utf8",
    cwd: root,
    env: { ...process.env, OPPI_CLI_BIN: missing },
  });

  assert.equal(result.status, 1);
  assert.match(result.stderr, /OPPi native management command requires @oppiai\/cli/);
  assert.equal(result.stdout.trim(), "");
});

test("oppi-native plugin command does not become a shell prompt", () => {
  const root = tempDir("native-plugin-bridge");
  const bridge = createFakeNodeEntrypoint(
    root,
    "fake-oppi-cli",
    [
      "const args = process.argv.slice(2);",
      "if (args[0] !== 'plugin' || args[1] !== 'enable' || !args.includes('--yes')) process.exit(7);",
      "console.log(JSON.stringify({ ok: true, command: args }));",
    ].join("\n"),
  );
  const fakeServer = createFakeCommand(root, "fake-oppi-server", "process.exit(9);\n");
  const fakeShell = createFakeCommand(root, "fake-oppi-shell", "process.exit(8);\n");

  const result = spawnSync(process.execPath, [resolve(__dirname, "main.js"), "plugin", "enable", "demo", "--yes", "--json"], {
    encoding: "utf8",
    cwd: root,
    env: {
      ...process.env,
      OPPI_CLI_BIN: bridge,
      OPPI_NATIVE_SHELL_BIN: fakeShell,
      OPPI_NATIVE_SERVER_BIN: fakeServer,
    },
  });

  assert.equal(result.status, 0, result.stderr || result.stdout);
  assert.deepEqual(JSON.parse(result.stdout).command, ["plugin", "enable", "demo", "--yes", "--json"]);
});

test("isMain tolerates realpath entrypoints", () => {
  const mainPath = resolve(__dirname, "main.js");
  assert.equal(isMain(mainPath, mainPath), true);
  assert.equal(isMain(undefined, mainPath), false);
});
