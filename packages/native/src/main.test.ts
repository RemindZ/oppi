import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { chmodSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { getNativePackageStatus, isMain, parseNativeArgs, resolveNativeBinaries } from "./main.js";

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

test("package metadata preserves the legacy oppi bin for @oppiai/cli", () => {
  const nativePackage = JSON.parse(readFileSync(resolve(__dirname, "..", "package.json"), "utf8"));
  const cliPackage = JSON.parse(readFileSync(resolve(__dirname, "..", "..", "cli", "package.json"), "utf8"));

  assert.equal(nativePackage.name, "@oppiai/native");
  assert.deepEqual(Object.keys(nativePackage.bin).sort(), ["oppi-native", "oppi-rs"]);
  assert.equal(Object.prototype.hasOwnProperty.call(nativePackage.bin, "oppi"), false);
  assert.deepEqual(cliPackage.bin, { oppi: "./dist/main.js" });
});

test("parseNativeArgs routes preview commands and defaults to shell passthrough", () => {
  assert.deepEqual(parseNativeArgs(["--help"]), { type: "help" });
  assert.deepEqual(parseNativeArgs(["doctor", "--json"]), { type: "doctor", json: true });
  assert.deepEqual(parseNativeArgs(["status"]), { type: "doctor", json: false });
  assert.deepEqual(parseNativeArgs(["smoke", "--mock", "--json"]), { type: "smoke", json: true, mock: true, shellArgs: [] });
  assert.deepEqual(parseNativeArgs(["server"]), { type: "server", args: ["--stdio"] });
  assert.deepEqual(parseNativeArgs(["--mock", "hello"]), { type: "shell", args: ["--mock", "hello"] });
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

test("isMain tolerates realpath entrypoints", () => {
  const mainPath = resolve(__dirname, "main.js");
  assert.equal(isMain(mainPath, mainPath), true);
  assert.equal(isMain(undefined, mainPath), false);
});
