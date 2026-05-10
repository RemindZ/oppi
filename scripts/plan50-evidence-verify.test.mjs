#!/usr/bin/env node
import assert from "node:assert/strict";
import { existsSync, mkdtempSync, mkdirSync, readFileSync, symlinkSync, writeFileSync } from "node:fs";
import { createHash } from "node:crypto";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";

const repoRoot = resolve(import.meta.dirname, "..");

function tempEvidenceRoot() {
  return mkdtempSync(join(tmpdir(), "oppi-plan50-evidence-"));
}

function sha256File(path) {
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}

function matrixOsForRunner(runnerOs) {
  if (runnerOs === "Linux") return "ubuntu-latest";
  if (runnerOs === "Windows") return "windows-latest";
  if (runnerOs === "macOS") return "macos-latest";
  return runnerOs.toLowerCase();
}

function writeRunnerEvidence(root, runnerOs, { strict = false, omitFromManifest = [], extraManifestFiles = [], manifestPatch = {}, artifactName, manifestName, strictStatus, logPatch = {}, smokePatch = {}, dogfoodPatch = {} } = {}) {
  const dir = join(root, artifactName || `plan50-native-shell-evidence-${matrixOsForRunner(runnerOs)}`);
  mkdirSync(dir, { recursive: true });
  const files = [
    `terminal-cleanup-lifecycle-${runnerOs}.log`,
    `terminal-cleanup-reset-${runnerOs}.log`,
    `tui-smoke-${runnerOs}.json`,
    `tui-dogfood-${runnerOs}.json`,
  ];
  writeFileSync(join(dir, `terminal-cleanup-lifecycle-${runnerOs}.log`), logPatch.lifecycle || "test result: ok. 1 passed; 0 failed\n", "utf8");
  writeFileSync(join(dir, `terminal-cleanup-reset-${runnerOs}.log`), logPatch.reset || "test result: ok. 1 passed; 0 failed\n", "utf8");
  writeFileSync(join(dir, `tui-smoke-${runnerOs}.json`), JSON.stringify({
    ok: true,
    diagnostics: ["native shell mock smoke completed"],
    ...smokePatch,
  }, null, 2), "utf8");
  writeFileSync(join(dir, `tui-dogfood-${runnerOs}.json`), JSON.stringify({
    ok: true,
    scenarios: [
      { name: "background-sandbox-execution", ok: true, status: strict ? "started, list=true, read=true, kill=true" : "sandbox-unavailable-denied" },
    ],
    ...dogfoodPatch,
  }, null, 2), "utf8");
  if (runnerOs === "Linux") {
    files.push("linux-bubblewrap-host-sandbox-Linux.log");
    writeFileSync(join(dir, "linux-bubblewrap-host-sandbox-Linux.log"), logPatch.linuxHostSandbox || "test result: ok. 1 passed; 0 failed\n", "utf8");
  }
  if (strict) {
    files.push(`tui-dogfood-strict-${runnerOs}.json`);
    writeFileSync(join(dir, `tui-dogfood-strict-${runnerOs}.json`), JSON.stringify({
      ok: true,
      strictBackgroundLifecycle: true,
      scenarios: [
        { name: "background-sandbox-execution", ok: true, status: strictStatus || "started, list=true, read=true, kill=true" },
      ],
    }, null, 2), "utf8");
  }
  const manifestFiles = files.filter((file) => !omitFromManifest.includes(file)).concat(extraManifestFiles).sort();
  const fileSha256 = Object.fromEntries(
    manifestFiles
      .filter((file) => existsSync(join(dir, file)))
      .map((file) => [file, sha256File(join(dir, file))]),
  );
  writeFileSync(join(dir, manifestName || `plan50-native-shell-evidence-${runnerOs}.json`), JSON.stringify({
    schemaVersion: 1,
    plan: "50-standalone-oppi-finish-line",
    runnerOs,
    matrixOs: matrixOsForRunner(runnerOs),
    strictBackgroundExpected: runnerOs === "Linux",
    gitSha: "0123456789abcdef0123456789abcdef01234567",
    githubRunId: "123456789",
    githubRunAttempt: "1",
    githubRefName: "main",
    files: manifestFiles,
    fileSha256,
    ...manifestPatch,
  }, null, 2), "utf8");
}

test("plan50 evidence verifier accepts downloaded CI artifacts proving strict background and terminal restore", () => {
  const root = tempEvidenceRoot();
  writeRunnerEvidence(root, "Windows");
  writeRunnerEvidence(root, "Linux", { strict: true });
  writeRunnerEvidence(root, "macOS");

  const result = spawnSync(process.execPath, ["scripts/plan50-evidence-verify.mjs", root, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 0, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(payload.ok, true);
  assert.equal(payload.checks.find((check) => check.id === "manifest-metadata-valid")?.ok, true);
  assert.equal(payload.checks.find((check) => check.id === "manifest-runners-unique")?.ok, true);
  assert.equal(payload.checks.find((check) => check.id === "manifest-artifact-folders-valid")?.ok, true);
  assert.equal(payload.checks.find((check) => check.id === "manifest-file-hashes-valid")?.ok, true);
  assert.equal(payload.checks.find((check) => check.id === "manifest-run-identity-consistent")?.ok, true);
  assert.equal(payload.checks.find((check) => check.id === "native-shell-smoke-passed")?.ok, true);
  assert.equal(payload.checks.find((check) => check.id === "normal-native-dogfood-passed")?.ok, true);
  assert.equal(payload.checks.find((check) => check.id === "linux-host-sandbox-passed")?.ok, true);
  assert.equal(payload.checks.find((check) => check.id === "strict-linux-background-lifecycle")?.ok, true);
  assert.equal(payload.checks.find((check) => check.id === "terminal-restore-windows-and-unix")?.ok, true);
  assert.deepEqual(payload.planRowsReadyToCheck, [
    "/background list/read/kill is dogfooded through native shell",
    "Terminal restore after abort/panic is checked on Windows and Unix",
    "Sandboxed background tasks instead of full-access/unrestricted background shell tasks",
  ]);
});

test("plan50 evidence verifier rejects partial matrix bundles", () => {
  const root = tempEvidenceRoot();
  writeRunnerEvidence(root, "Windows");
  writeRunnerEvidence(root, "Linux", { strict: true });

  const result = spawnSync(process.execPath, ["scripts/plan50-evidence-verify.mjs", root, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(payload.ok, false);
  assert.equal(payload.checks.find((check) => check.id === "manifests-present")?.ok, false);
  assert.deepEqual(payload.planRowsReadyToCheck, []);
});

test("plan50 evidence verifier rejects file paths as evidence roots", () => {
  const root = tempEvidenceRoot();
  const evidenceRoot = join(root, "not-a-directory.json");
  writeFileSync(evidenceRoot, "{}\n", "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-evidence-verify.mjs", evidenceRoot, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(payload.ok, false);
  assert.equal(payload.checks.find((check) => check.id === "evidence-root")?.ok, false);
  assert.match(payload.checks.find((check) => check.id === "evidence-root")?.evidence, /directory/);
  assert.deepEqual(payload.planRowsReadyToCheck, []);
});

test("plan50 evidence verifier rejects degraded Linux background evidence", () => {
  const root = tempEvidenceRoot();
  writeRunnerEvidence(root, "Windows");
  writeRunnerEvidence(root, "Linux");
  writeRunnerEvidence(root, "macOS");

  const result = spawnSync(process.execPath, ["scripts/plan50-evidence-verify.mjs", root, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(payload.ok, false);
  assert.equal(payload.checks.find((check) => check.id === "strict-linux-background-lifecycle")?.ok, false);
});

test("plan50 evidence verifier rejects failed normal native dogfood evidence", () => {
  const root = tempEvidenceRoot();
  writeRunnerEvidence(root, "Windows", {
    dogfoodPatch: {
      ok: false,
      scenarios: [
        { name: "background-sandbox-execution", ok: false, status: "dogfood failed before background read" },
      ],
    },
  });
  writeRunnerEvidence(root, "Linux", { strict: true });
  writeRunnerEvidence(root, "macOS");

  const result = spawnSync(process.execPath, ["scripts/plan50-evidence-verify.mjs", root, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(payload.ok, false);
  assert.equal(payload.checks.find((check) => check.id === "normal-native-dogfood-passed")?.ok, false);
  assert.deepEqual(payload.planRowsReadyToCheck, []);
});

test("plan50 evidence verifier rejects missing native shell smoke evidence", () => {
  const root = tempEvidenceRoot();
  writeRunnerEvidence(root, "Windows", { omitFromManifest: ["tui-smoke-Windows.json"] });
  writeRunnerEvidence(root, "Linux", { strict: true });
  writeRunnerEvidence(root, "macOS");

  const result = spawnSync(process.execPath, ["scripts/plan50-evidence-verify.mjs", root, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(payload.ok, false);
  assert.equal(payload.checks.find((check) => check.id === "native-shell-smoke-passed")?.ok, false);
  assert.deepEqual(payload.planRowsReadyToCheck, []);
});

test("plan50 evidence verifier rejects failed native shell smoke evidence", () => {
  const root = tempEvidenceRoot();
  writeRunnerEvidence(root, "Windows", { smokePatch: { ok: false, diagnostics: ["shell exited 1"] } });
  writeRunnerEvidence(root, "Linux", { strict: true });
  writeRunnerEvidence(root, "macOS");

  const result = spawnSync(process.execPath, ["scripts/plan50-evidence-verify.mjs", root, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(payload.ok, false);
  assert.equal(payload.checks.find((check) => check.id === "manifest-file-lists-complete")?.ok, true);
  assert.equal(payload.checks.find((check) => check.id === "native-shell-smoke-passed")?.ok, false);
  assert.deepEqual(payload.planRowsReadyToCheck, []);
});

test("plan50 evidence verifier rejects manifest file hash mismatches", () => {
  const root = tempEvidenceRoot();
  writeRunnerEvidence(root, "Windows", {
    manifestPatch: {
      fileSha256: {
        "tui-smoke-Windows.json": "0".repeat(64),
      },
    },
  });
  writeRunnerEvidence(root, "Linux", { strict: true });
  writeRunnerEvidence(root, "macOS");

  const result = spawnSync(process.execPath, ["scripts/plan50-evidence-verify.mjs", root, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(payload.ok, false);
  assert.equal(payload.checks.find((check) => check.id === "manifest-file-hashes-valid")?.ok, false);
  assert.deepEqual(payload.planRowsReadyToCheck, []);
});

test("plan50 evidence verifier rejects runner artifacts mixed from different CI runs", () => {
  const root = tempEvidenceRoot();
  writeRunnerEvidence(root, "Windows");
  writeRunnerEvidence(root, "Linux", {
    strict: true,
    manifestPatch: {
      githubRunId: "987654321",
    },
  });
  writeRunnerEvidence(root, "macOS");

  const result = spawnSync(process.execPath, ["scripts/plan50-evidence-verify.mjs", root, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(payload.ok, false);
  assert.equal(payload.checks.find((check) => check.id === "manifest-run-identity-consistent")?.ok, false);
  assert.deepEqual(payload.planRowsReadyToCheck, []);
});

test("plan50 evidence verifier rejects malformed shared CI ref names", () => {
  const root = tempEvidenceRoot();
  const manifestPatch = {
    githubRefName: "main\n",
  };
  writeRunnerEvidence(root, "Windows", { manifestPatch });
  writeRunnerEvidence(root, "Linux", { strict: true, manifestPatch });
  writeRunnerEvidence(root, "macOS", { manifestPatch });

  const result = spawnSync(process.execPath, ["scripts/plan50-evidence-verify.mjs", root, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(payload.ok, false);
  assert.equal(payload.checks.find((check) => check.id === "manifest-run-identity-consistent")?.ok, false);
  assert.deepEqual(payload.planRowsReadyToCheck, []);
});

test("plan50 evidence verifier rejects non-positive CI run identity values", () => {
  const root = tempEvidenceRoot();
  const manifestPatch = {
    githubRunId: "0",
    githubRunAttempt: "0",
  };
  writeRunnerEvidence(root, "Windows", { manifestPatch });
  writeRunnerEvidence(root, "Linux", { strict: true, manifestPatch });
  writeRunnerEvidence(root, "macOS", { manifestPatch });

  const result = spawnSync(process.execPath, ["scripts/plan50-evidence-verify.mjs", root, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(payload.ok, false);
  assert.equal(payload.checks.find((check) => check.id === "manifest-run-identity-consistent")?.ok, false);
  assert.deepEqual(payload.planRowsReadyToCheck, []);
});

test("plan50 evidence verifier rejects all-zero CI git SHA values", () => {
  const root = tempEvidenceRoot();
  const manifestPatch = {
    gitSha: "0".repeat(40),
  };
  writeRunnerEvidence(root, "Windows", { manifestPatch });
  writeRunnerEvidence(root, "Linux", { strict: true, manifestPatch });
  writeRunnerEvidence(root, "macOS", { manifestPatch });

  const result = spawnSync(process.execPath, ["scripts/plan50-evidence-verify.mjs", root, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(payload.ok, false);
  assert.equal(payload.checks.find((check) => check.id === "manifest-run-identity-consistent")?.ok, false);
  assert.deepEqual(payload.planRowsReadyToCheck, []);
});

test("plan50 evidence verifier rejects runner manifests in the wrong matrix artifact folder", () => {
  const root = tempEvidenceRoot();
  writeRunnerEvidence(root, "Windows", {
    artifactName: "plan50-native-shell-evidence-ubuntu-latest",
  });
  writeRunnerEvidence(root, "Linux", {
    strict: true,
    artifactName: "plan50-native-shell-evidence-windows-latest",
  });
  writeRunnerEvidence(root, "macOS");

  const result = spawnSync(process.execPath, ["scripts/plan50-evidence-verify.mjs", root, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(payload.ok, false);
  assert.equal(payload.checks.find((check) => check.id === "manifest-artifact-folders-valid")?.ok, false);
  assert.deepEqual(payload.planRowsReadyToCheck, []);
});

test("plan50 evidence verifier rejects runner manifests with the wrong filename", () => {
  const root = tempEvidenceRoot();
  writeRunnerEvidence(root, "Windows", {
    manifestName: "plan50-native-shell-evidence-Linux.json",
  });
  writeRunnerEvidence(root, "Linux", { strict: true });
  writeRunnerEvidence(root, "macOS");

  const result = spawnSync(process.execPath, ["scripts/plan50-evidence-verify.mjs", root, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(payload.ok, false);
  assert.equal(payload.checks.find((check) => check.id === "manifest-metadata-valid")?.ok, false);
  assert.deepEqual(payload.planRowsReadyToCheck, []);
});

test("plan50 evidence verifier rejects dogfood payloads with failed scenario details", () => {
  const root = tempEvidenceRoot();
  writeRunnerEvidence(root, "Windows", {
    dogfoodPatch: {
      ok: true,
      scenarios: [
        { name: "background-sandbox-execution", ok: true, status: "sandbox-unavailable-denied" },
        { name: "failure-read-only-write", ok: false, status: "not-denied" },
      ],
    },
  });
  writeRunnerEvidence(root, "Linux", { strict: true });
  writeRunnerEvidence(root, "macOS");

  const result = spawnSync(process.execPath, ["scripts/plan50-evidence-verify.mjs", root, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(payload.ok, false);
  assert.equal(payload.checks.find((check) => check.id === "normal-native-dogfood-passed")?.ok, false);
  assert.deepEqual(payload.planRowsReadyToCheck, []);
});

test("plan50 evidence verifier rejects incomplete strict background lifecycle status", () => {
  const root = tempEvidenceRoot();
  writeRunnerEvidence(root, "Windows");
  writeRunnerEvidence(root, "Linux", {
    strict: true,
    strictStatus: "started, list=true, read=true, kill=false",
  });
  writeRunnerEvidence(root, "macOS");

  const result = spawnSync(process.execPath, ["scripts/plan50-evidence-verify.mjs", root, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(payload.ok, false);
  assert.equal(payload.checks.find((check) => check.id === "strict-linux-background-lifecycle")?.ok, false);
  assert.deepEqual(payload.planRowsReadyToCheck, []);
});

test("plan50 evidence verifier rejects contradictory strict background lifecycle status", () => {
  const root = tempEvidenceRoot();
  writeRunnerEvidence(root, "Windows");
  writeRunnerEvidence(root, "Linux", {
    strict: true,
    strictStatus: "sandbox-unavailable-denied; started, list=true, read=true, kill=true",
  });
  writeRunnerEvidence(root, "macOS");

  const result = spawnSync(process.execPath, ["scripts/plan50-evidence-verify.mjs", root, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(payload.ok, false);
  assert.equal(payload.checks.find((check) => check.id === "strict-linux-background-lifecycle")?.ok, false);
  assert.deepEqual(payload.planRowsReadyToCheck, []);
});

test("plan50 evidence verifier rejects missing Linux host sandbox evidence", () => {
  const root = tempEvidenceRoot();
  writeRunnerEvidence(root, "Windows");
  writeRunnerEvidence(root, "Linux", {
    strict: true,
    omitFromManifest: ["linux-bubblewrap-host-sandbox-Linux.log"],
  });
  writeRunnerEvidence(root, "macOS");

  const result = spawnSync(process.execPath, ["scripts/plan50-evidence-verify.mjs", root, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(payload.ok, false);
  assert.equal(payload.checks.find((check) => check.id === "manifest-file-lists-complete")?.ok, false);
  assert.equal(payload.checks.find((check) => check.id === "linux-host-sandbox-passed")?.ok, false);
  assert.deepEqual(payload.planRowsReadyToCheck, []);
});

test("plan50 evidence verifier rejects failed Linux host sandbox evidence", () => {
  const root = tempEvidenceRoot();
  writeRunnerEvidence(root, "Windows");
  writeRunnerEvidence(root, "Linux", {
    strict: true,
    logPatch: {
      linuxHostSandbox: "test result: FAILED. 0 passed; 1 failed\n",
    },
  });
  writeRunnerEvidence(root, "macOS");

  const result = spawnSync(process.execPath, ["scripts/plan50-evidence-verify.mjs", root, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(payload.ok, false);
  assert.equal(payload.checks.find((check) => check.id === "manifest-file-lists-complete")?.ok, true);
  assert.equal(payload.checks.find((check) => check.id === "linux-host-sandbox-passed")?.ok, false);
  assert.deepEqual(payload.planRowsReadyToCheck, []);
});

test("plan50 evidence verifier rejects mixed terminal cleanup pass and fail logs", () => {
  const root = tempEvidenceRoot();
  writeRunnerEvidence(root, "Windows", {
    logPatch: {
      lifecycle: [
        "test result: ok. 1 passed; 0 failed",
        "test result: FAILED. 0 passed; 1 failed",
      ].join("\n"),
    },
  });
  writeRunnerEvidence(root, "Linux", { strict: true });
  writeRunnerEvidence(root, "macOS");

  const result = spawnSync(process.execPath, ["scripts/plan50-evidence-verify.mjs", root, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(payload.ok, false);
  assert.equal(payload.checks.find((check) => check.id === "terminal-restore-windows-and-unix")?.ok, false);
  assert.deepEqual(payload.planRowsReadyToCheck, []);
});

test("plan50 evidence verifier rejects required files omitted from runner manifest", () => {
  const root = tempEvidenceRoot();
  writeRunnerEvidence(root, "Windows");
  writeRunnerEvidence(root, "Linux", {
    strict: true,
    omitFromManifest: ["tui-dogfood-strict-Linux.json"],
  });
  writeRunnerEvidence(root, "macOS");

  const result = spawnSync(process.execPath, ["scripts/plan50-evidence-verify.mjs", root, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(payload.ok, false);
  assert.equal(payload.checks.find((check) => check.id === "manifest-file-lists-complete")?.ok, false);
  assert.equal(payload.checks.find((check) => check.id === "strict-linux-background-lifecycle")?.ok, false);
});

test("plan50 evidence verifier rejects extra artifact files not listed by runner manifest", () => {
  const root = tempEvidenceRoot();
  writeRunnerEvidence(root, "Windows");
  writeRunnerEvidence(root, "Linux", { strict: true });
  writeRunnerEvidence(root, "macOS");
  writeFileSync(join(root, "plan50-native-shell-evidence-windows-latest", "unexpected-extra.log"), "not listed\n", "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-evidence-verify.mjs", root, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(payload.ok, false);
  assert.equal(payload.checks.find((check) => check.id === "manifest-file-lists-complete")?.ok, false);
  assert.deepEqual(payload.planRowsReadyToCheck, []);
});

test("plan50 evidence verifier rejects evidence files outside runner artifact folders", () => {
  const root = tempEvidenceRoot();
  writeRunnerEvidence(root, "Windows");
  writeRunnerEvidence(root, "Linux", { strict: true });
  writeRunnerEvidence(root, "macOS");
  writeFileSync(join(root, "unexpected-root-file.log"), "not scoped to a runner artifact\n", "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-evidence-verify.mjs", root, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(payload.ok, false);
  assert.equal(payload.checks.find((check) => check.id === "evidence-files-scoped-to-manifests")?.ok, false);
  assert.deepEqual(payload.planRowsReadyToCheck, []);
});

test("plan50 evidence verifier rejects symlinked runner artifact folders", () => {
  const sourceRoot = tempEvidenceRoot();
  writeRunnerEvidence(sourceRoot, "Windows");
  writeRunnerEvidence(sourceRoot, "Linux", { strict: true });
  writeRunnerEvidence(sourceRoot, "macOS");

  const root = tempEvidenceRoot();
  for (const artifact of [
    "plan50-native-shell-evidence-windows-latest",
    "plan50-native-shell-evidence-ubuntu-latest",
    "plan50-native-shell-evidence-macos-latest",
  ]) {
    symlinkSync(join(sourceRoot, artifact), join(root, artifact), "junction");
  }

  const result = spawnSync(process.execPath, ["scripts/plan50-evidence-verify.mjs", root, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(payload.ok, false);
  assert.equal(payload.checks.find((check) => check.id === "manifests-present")?.ok, false);
  assert.deepEqual(payload.planRowsReadyToCheck, []);
});

test("plan50 evidence verifier rejects manifest-listed files that are missing", () => {
  const root = tempEvidenceRoot();
  writeRunnerEvidence(root, "Windows", {
    extraManifestFiles: ["missing-extra-evidence.log"],
  });
  writeRunnerEvidence(root, "Linux", { strict: true });
  writeRunnerEvidence(root, "macOS");

  const result = spawnSync(process.execPath, ["scripts/plan50-evidence-verify.mjs", root, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(payload.ok, false);
  assert.equal(payload.checks.find((check) => check.id === "manifest-file-lists-complete")?.ok, false);
  assert.deepEqual(payload.planRowsReadyToCheck, []);
});

test("plan50 evidence verifier rejects duplicate manifest file entries", () => {
  const root = tempEvidenceRoot();
  writeRunnerEvidence(root, "Windows", {
    manifestPatch: {
      files: [
        "terminal-cleanup-lifecycle-Windows.log",
        "terminal-cleanup-lifecycle-Windows.log",
        "terminal-cleanup-reset-Windows.log",
        "tui-smoke-Windows.json",
        "tui-dogfood-Windows.json",
      ],
    },
  });
  writeRunnerEvidence(root, "Linux", { strict: true });
  writeRunnerEvidence(root, "macOS");

  const result = spawnSync(process.execPath, ["scripts/plan50-evidence-verify.mjs", root, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(payload.ok, false);
  assert.equal(payload.checks.find((check) => check.id === "manifest-metadata-valid")?.ok, false);
  assert.equal(payload.checks.find((check) => check.id === "manifest-file-lists-complete")?.ok, false);
  assert.deepEqual(payload.planRowsReadyToCheck, []);
});

test("plan50 evidence verifier rejects unsorted manifest file entries", () => {
  const root = tempEvidenceRoot();
  writeRunnerEvidence(root, "Windows", {
    manifestPatch: {
      files: [
        "terminal-cleanup-lifecycle-Windows.log",
        "terminal-cleanup-reset-Windows.log",
        "tui-smoke-Windows.json",
        "tui-dogfood-Windows.json",
      ],
    },
  });
  writeRunnerEvidence(root, "Linux", { strict: true });
  writeRunnerEvidence(root, "macOS");

  const result = spawnSync(process.execPath, ["scripts/plan50-evidence-verify.mjs", root, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(payload.ok, false);
  assert.equal(payload.checks.find((check) => check.id === "manifest-metadata-valid")?.ok, false);
  assert.deepEqual(payload.planRowsReadyToCheck, []);
});

test("plan50 evidence verifier rejects whitespace-padded manifest file entries", () => {
  const root = tempEvidenceRoot();
  writeRunnerEvidence(root, "Windows");
  writeRunnerEvidence(root, "Linux", { strict: true });
  writeRunnerEvidence(root, "macOS");
  const dir = join(root, "plan50-native-shell-evidence-windows-latest");
  const paddedName = " leading-space-artifact.log";
  writeFileSync(join(dir, paddedName), "ambiguous artifact name\n", "utf8");
  const manifestPath = join(dir, "plan50-native-shell-evidence-Windows.json");
  const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
  manifest.files = manifest.files.concat(paddedName).sort();
  manifest.fileSha256[paddedName] = sha256File(join(dir, paddedName));
  writeFileSync(manifestPath, JSON.stringify(manifest, null, 2), "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-evidence-verify.mjs", root, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(payload.ok, false);
  assert.equal(payload.checks.find((check) => check.id === "manifest-metadata-valid")?.ok, false);
  assert.deepEqual(payload.planRowsReadyToCheck, []);
});

test("plan50 evidence verifier rejects manifest-looking files listed as artifacts", () => {
  const root = tempEvidenceRoot();
  writeRunnerEvidence(root, "Windows");
  writeRunnerEvidence(root, "Linux", { strict: true });
  writeRunnerEvidence(root, "macOS");
  const dir = join(root, "plan50-native-shell-evidence-windows-latest");
  const extraManifestName = "plan50-native-shell-evidence-extra.json";
  writeFileSync(join(dir, extraManifestName), "{}\n", "utf8");
  const manifestPath = join(dir, "plan50-native-shell-evidence-Windows.json");
  const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
  manifest.files.push(extraManifestName);
  manifest.fileSha256[extraManifestName] = sha256File(join(dir, extraManifestName));
  writeFileSync(manifestPath, JSON.stringify(manifest, null, 2), "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-evidence-verify.mjs", root, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(payload.ok, false);
  assert.equal(payload.checks.find((check) => check.id === "manifest-metadata-valid")?.ok, false);
  assert.deepEqual(payload.planRowsReadyToCheck, []);
});

test("plan50 evidence verifier rejects unsafe manifest file entries without crashing", () => {
  const root = tempEvidenceRoot();
  writeRunnerEvidence(root, "Windows", {
    manifestPatch: {
      files: [".."],
      fileSha256: {
        "..": "0".repeat(64),
      },
    },
  });
  writeRunnerEvidence(root, "Linux", { strict: true });
  writeRunnerEvidence(root, "macOS");

  const result = spawnSync(process.execPath, ["scripts/plan50-evidence-verify.mjs", root, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(payload.ok, false);
  assert.equal(payload.checks.find((check) => check.id === "manifest-metadata-valid")?.ok, false);
  assert.deepEqual(payload.planRowsReadyToCheck, []);
});

test("plan50 evidence verifier rejects stale or malformed manifest metadata", () => {
  const root = tempEvidenceRoot();
  writeRunnerEvidence(root, "Windows");
  writeRunnerEvidence(root, "Linux", {
    strict: true,
    manifestPatch: { schemaVersion: 0, strictBackgroundExpected: false },
  });
  writeRunnerEvidence(root, "macOS");

  const result = spawnSync(process.execPath, ["scripts/plan50-evidence-verify.mjs", root, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(payload.ok, false);
  assert.equal(payload.checks.find((check) => check.id === "manifest-metadata-valid")?.ok, false);
  assert.deepEqual(payload.planRowsReadyToCheck, []);
});

test("plan50 evidence verifier rejects duplicate runner manifests", () => {
  const root = tempEvidenceRoot();
  writeRunnerEvidence(root, "Windows");
  writeRunnerEvidence(root, "Linux", { strict: true });
  writeRunnerEvidence(root, "macOS");
  writeRunnerEvidence(root, "Linux", {
    strict: true,
    artifactName: "plan50-native-shell-evidence-linux-duplicate",
  });

  const result = spawnSync(process.execPath, ["scripts/plan50-evidence-verify.mjs", root, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(payload.ok, false);
  assert.equal(payload.checks.find((check) => check.id === "manifest-runners-unique")?.ok, false);
  assert.deepEqual(payload.planRowsReadyToCheck, []);
});
