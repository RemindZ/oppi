#!/usr/bin/env node
import { mkdirSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { spawnSync } from "node:child_process";

const repoRoot = resolve(import.meta.dirname, "..");

function localRunnerOs() {
  if (process.platform === "win32") return "Windows";
  if (process.platform === "darwin") return "macOS";
  if (process.platform === "linux") return "Linux";
  return process.platform;
}

function argValue(name) {
  const inline = process.argv.find((arg) => arg.startsWith(`${name}=`));
  if (inline) return inline.slice(name.length + 1);
  const index = process.argv.indexOf(name);
  return index >= 0 ? process.argv[index + 1] : undefined;
}

function runCargoTest(testName) {
  return spawnSync("cargo", [
    "test",
    "-p",
    "oppi-shell",
    testName,
    "--",
    "--nocapture",
  ], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
}

const outputDir = resolve(argValue("--output-dir") || join(repoRoot, ".validation", "plan50-terminal-evidence"));
const runnerOs = localRunnerOs();
mkdirSync(outputDir, { recursive: true });

const cases = [
  ["lifecycle", "ratatui_lifecycle_exit_paths_share_cleanup_contract"],
  ["reset", "ratatui_terminal_cleanup_sequence_resets_and_clears"],
];

let ok = true;
for (const [kind, testName] of cases) {
  const result = runCargoTest(testName);
  const text = `${result.stdout || ""}${result.stderr || ""}`;
  writeFileSync(join(outputDir, `terminal-cleanup-${kind}-${runnerOs}.log`), text, "utf8");
  if (result.error) process.stderr.write(`${result.error.message}\n`);
  if (result.status !== 0) ok = false;
}

process.stderr.write(`Plan 50 terminal evidence written to ${outputDir}\n`);
process.exit(ok ? 0 : 1);
