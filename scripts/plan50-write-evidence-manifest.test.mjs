#!/usr/bin/env node
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { existsSync, mkdtempSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";

const repoRoot = resolve(import.meta.dirname, "..");

function tempEvidenceDir() {
  return mkdtempSync(join(tmpdir(), "oppi-plan50-manifest-"));
}

function sha256Text(text) {
  return createHash("sha256").update(text).digest("hex");
}

function manifestEnv(patch = {}) {
  return {
    ...process.env,
    RUNNER_OS: "Windows",
    MATRIX_OS: "windows-latest",
    GITHUB_SHA: "0123456789abcdef0123456789abcdef01234567",
    GITHUB_RUN_ID: "123456789",
    GITHUB_RUN_ATTEMPT: "1",
    GITHUB_REF_NAME: "main",
    ...patch,
  };
}

test("plan50 manifest writer emits sorted hashed evidence manifest", () => {
  const evidenceDir = tempEvidenceDir();
  writeFileSync(join(evidenceDir, "z-last.log"), "z\n", "utf8");
  writeFileSync(join(evidenceDir, "a-first.json"), "{\"ok\":true}\n", "utf8");
  writeFileSync(join(evidenceDir, "plan50-native-shell-evidence-Windows.json"), "{\"stale\":true}\n", "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-write-evidence-manifest.mjs", "--evidence-dir", evidenceDir], {
    cwd: repoRoot,
    encoding: "utf8",
    env: manifestEnv(),
    windowsHide: true,
  });

  assert.equal(result.status, 0, result.stderr || result.stdout);
  const manifestPath = join(evidenceDir, "plan50-native-shell-evidence-Windows.json");
  assert.equal(existsSync(manifestPath), true);
  const payload = JSON.parse(readFileSync(manifestPath, "utf8"));
  assert.equal(payload.schemaVersion, 1);
  assert.equal(payload.plan, "50-standalone-oppi-finish-line");
  assert.equal(payload.runnerOs, "Windows");
  assert.equal(payload.matrixOs, "windows-latest");
  assert.equal(payload.strictBackgroundExpected, false);
  assert.equal(payload.gitSha, "0123456789abcdef0123456789abcdef01234567");
  assert.equal(payload.githubRunId, "123456789");
  assert.equal(payload.githubRunAttempt, "1");
  assert.equal(payload.githubRefName, "main");
  assert.deepEqual(payload.files, ["a-first.json", "z-last.log"]);
  assert.deepEqual(payload.fileSha256, {
    "a-first.json": sha256Text("{\"ok\":true}\n"),
    "z-last.log": sha256Text("z\n"),
  });
});

test("plan50 manifest writer fails closed when required CI identity is missing", () => {
  const evidenceDir = tempEvidenceDir();
  writeFileSync(join(evidenceDir, "tui-smoke-Windows.json"), "{\"ok\":true}\n", "utf8");
  const env = manifestEnv({ GITHUB_RUN_ID: "" });

  const result = spawnSync(process.execPath, ["scripts/plan50-write-evidence-manifest.mjs", "--evidence-dir", evidenceDir], {
    cwd: repoRoot,
    encoding: "utf8",
    env,
    windowsHide: true,
  });

  assert.equal(result.status, 1);
  assert.match(result.stderr, /GITHUB_RUN_ID/);
});
