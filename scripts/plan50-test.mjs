#!/usr/bin/env node
import { spawnSync } from "node:child_process";
import { resolve } from "node:path";

const repoRoot = resolve(import.meta.dirname, "..");
const plan50Scripts = [
  "scripts/plan50-audit.mjs",
  "scripts/plan50-audit.test.mjs",
  "scripts/plan50-capture-local-background.mjs",
  "scripts/plan50-capture-local-terminal.mjs",
  "scripts/plan50-evidence-verify.mjs",
  "scripts/plan50-evidence-verify.test.mjs",
  "scripts/plan50-test.mjs",
  "scripts/plan50-write-evidence-manifest.mjs",
  "scripts/plan50-write-evidence-manifest.test.mjs",
];
const testFiles = [
  "scripts/plan50-audit.test.mjs",
  "scripts/plan50-evidence-verify.test.mjs",
  "scripts/plan50-write-evidence-manifest.test.mjs",
];

function run(label, args) {
  console.log(`plan50:test ${label}: ${process.execPath} ${args.join(" ")}`);
  const result = spawnSync(process.execPath, args, {
    cwd: repoRoot,
    stdio: "inherit",
    windowsHide: true,
  });
  if (result.error) {
    console.error(`plan50:test ${label} failed to start: ${result.error.message}`);
    process.exit(1);
  }
  if (result.status !== 0) process.exit(result.status ?? 1);
}

for (const script of plan50Scripts) run(`syntax ${script}`, ["--check", script]);
run("suite", ["--test", ...testFiles]);
