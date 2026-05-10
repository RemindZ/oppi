#!/usr/bin/env node
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
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

const outputArg = argValue("--output");
const outputPath = resolve(outputArg || join(
  repoRoot,
  ".validation",
  "plan50-background-evidence",
  `tui-dogfood-strict-${localRunnerOs()}.json`,
));

mkdirSync(dirname(outputPath), { recursive: true });

const result = spawnSync(process.execPath, [
  join(repoRoot, "packages", "cli", "dist", "main.js"),
  "tui",
  "dogfood",
  "--mock",
  "--json",
  "--require-background-lifecycle",
], {
  cwd: repoRoot,
  encoding: "utf8",
  windowsHide: true,
});

writeFileSync(outputPath, result.stdout || "", "utf8");
if (result.stderr) process.stderr.write(result.stderr);
if (result.error) process.stderr.write(`${result.error.message}\n`);
process.stderr.write(`Plan 50 strict background evidence written to ${outputPath}\n`);
process.exit(result.status ?? 1);
