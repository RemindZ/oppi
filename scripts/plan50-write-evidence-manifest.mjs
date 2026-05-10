#!/usr/bin/env node
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";

const PLAN_ID = "50-standalone-oppi-finish-line";
const REQUIRED_ENV = [
  "RUNNER_OS",
  "MATRIX_OS",
  "GITHUB_SHA",
  "GITHUB_RUN_ID",
  "GITHUB_RUN_ATTEMPT",
  "GITHUB_REF_NAME",
];

function argValue(name, fallback) {
  const inline = process.argv.find((arg) => arg.startsWith(`${name}=`));
  if (inline) return inline.slice(name.length + 1);
  const index = process.argv.indexOf(name);
  return index >= 0 ? process.argv[index + 1] : fallback;
}

function fail(message) {
  console.error(message);
  process.exit(1);
}

function requireEnv(name) {
  const value = process.env[name];
  if (!value) fail(`Missing required environment variable: ${name}`);
  return value;
}

function sha256File(path) {
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}

function evidenceFiles(evidenceDir) {
  if (!existsSync(evidenceDir)) mkdirSync(evidenceDir, { recursive: true });
  return readdirSync(evidenceDir)
    .filter((name) => statSync(join(evidenceDir, name)).isFile())
    .filter((name) => !/^plan50-native-shell-evidence-.+\.json$/.test(name))
    .sort();
}

const evidenceDir = resolve(argValue("--evidence-dir", "plan50-evidence"));
const runnerOs = requireEnv("RUNNER_OS");
const matrixOs = requireEnv("MATRIX_OS");
const gitSha = requireEnv("GITHUB_SHA");
const githubRunId = requireEnv("GITHUB_RUN_ID");
const githubRunAttempt = requireEnv("GITHUB_RUN_ATTEMPT");
const githubRefName = requireEnv("GITHUB_REF_NAME");
const files = evidenceFiles(evidenceDir);
const fileSha256 = Object.fromEntries(files.map((name) => [name, sha256File(join(evidenceDir, name))]));
const payload = {
  schemaVersion: 1,
  plan: PLAN_ID,
  runnerOs,
  matrixOs,
  strictBackgroundExpected: runnerOs === "Linux",
  gitSha,
  githubRunId,
  githubRunAttempt,
  githubRefName,
  files,
  fileSha256,
};
const manifestPath = join(evidenceDir, `plan50-native-shell-evidence-${runnerOs}.json`);
writeFileSync(manifestPath, JSON.stringify(payload, null, 2), "utf8");
console.log(manifestPath);
