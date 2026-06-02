#!/usr/bin/env node
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import test from "node:test";
import {
  REQUIRED_NATIVE_SHELL_MATRIX_JOBS,
  REQUIRED_PLAN72_STEP_NAME,
  verifyPlan72NativeShellRun,
} from "./plan72-ci-evidence.mjs";

const repoRoot = resolve(import.meta.dirname, "..");

function step(name, conclusion = "success") {
  return {
    name,
    status: "completed",
    conclusion,
  };
}

function matrixJob(name, steps = [step(REQUIRED_PLAN72_STEP_NAME)]) {
  return {
    name,
    status: "completed",
    conclusion: "success",
    steps,
  };
}

function nativeShellRun(jobs) {
  return {
    databaseId: 123456,
    workflowName: "Native shell validation",
    status: "completed",
    conclusion: "success",
    url: "https://github.com/RemindZ/oppi/actions/runs/123456",
    jobs,
  };
}

test("Plan 72 CI evidence passes only when every native-shell matrix job ran verify:pack", () => {
  const report = verifyPlan72NativeShellRun(
    nativeShellRun([
      ...REQUIRED_NATIVE_SHELL_MATRIX_JOBS.map((name) => matrixJob(name)),
      matrixJob("Plan 50 evidence bundle verifier", [step("Verify Plan 50 evidence bundle")]),
    ]),
  );

  assert.equal(report.ok, true);
  assert.deepEqual(report.missing, []);
  assert.equal(report.matrixJobs.length, 3);
  assert.equal(
    report.checks.find((check) => check.id === "native-package-artifact-step")?.ok,
    true,
  );
});

test("Plan 72 CI evidence rejects stale successful runs that predate the package verifier step", () => {
  const report = verifyPlan72NativeShellRun(
    nativeShellRun([
      matrixJob("ubuntu-latest native shell", [step("Test native npm packages")]),
      matrixJob("macos-latest native shell", [step("Test native npm packages")]),
      matrixJob("windows-latest native shell", [step("Test native npm packages")]),
    ]),
  );

  assert.equal(report.ok, false);
  assert.deepEqual(report.missing, [
    "ubuntu-latest native shell missing successful step Verify native npm package artifacts",
    "macos-latest native shell missing successful step Verify native npm package artifacts",
    "windows-latest native shell missing successful step Verify native npm package artifacts",
  ]);
  assert.equal(
    report.checks.find((check) => check.id === "native-package-artifact-step")?.ok,
    false,
  );
});

test("root package exposes Plan 72 CI evidence helper scripts", () => {
  const rootPackage = JSON.parse(readFileSync(join(repoRoot, "package.json"), "utf8"));

  assert.equal(rootPackage.scripts["plan72:ci-evidence"], "node scripts/plan72-ci-evidence.mjs --latest --summary");
  assert.equal(rootPackage.scripts["plan72:test"], "node --test scripts/plan72-ci-evidence.test.mjs");
});
