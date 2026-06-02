#!/usr/bin/env node
import { readFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

export const REQUIRED_PLAN72_STEP_NAME = "Verify native npm package artifacts";
export const REQUIRED_NATIVE_SHELL_MATRIX_JOBS = [
  "ubuntu-latest native shell",
  "macos-latest native shell",
  "windows-latest native shell",
];

const GH_RUN_FIELDS = [
  "databaseId",
  "workflowName",
  "headBranch",
  "headSha",
  "status",
  "conclusion",
  "createdAt",
  "updatedAt",
  "url",
  "jobs",
].join(",");

function check(id, ok, evidence, missing = []) {
  return { id, ok, evidence, missing };
}

function successfulStep(job, name) {
  return (job.steps || []).some((step) =>
    step.name === name
      && step.status === "completed"
      && step.conclusion === "success"
  );
}

export function verifyPlan72NativeShellRun(run) {
  const jobs = Array.isArray(run?.jobs) ? run.jobs : [];
  const matrixJobs = REQUIRED_NATIVE_SHELL_MATRIX_JOBS.map((name) => ({
    name,
    job: jobs.find((job) => job.name === name),
  }));
  const missingJobs = matrixJobs
    .filter(({ job }) => !job)
    .map(({ name }) => `missing matrix job ${name}`);
  const failedJobs = matrixJobs
    .filter(({ job }) => job && (job.status !== "completed" || job.conclusion !== "success"))
    .map(({ name, job }) => `${name} not successful (${job.status || "unknown"}/${job.conclusion || "unknown"})`);
  const missingArtifactSteps = matrixJobs
    .filter(({ job }) => job && !successfulStep(job, REQUIRED_PLAN72_STEP_NAME))
    .map(({ name }) => `${name} missing successful step ${REQUIRED_PLAN72_STEP_NAME}`);

  const checks = [
    check(
      "workflow-success",
      run?.workflowName === "Native shell validation"
        && run?.status === "completed"
        && run?.conclusion === "success",
      `workflow=${run?.workflowName || "unknown"} status=${run?.status || "unknown"} conclusion=${run?.conclusion || "unknown"}`,
      run?.workflowName === "Native shell validation"
        && run?.status === "completed"
        && run?.conclusion === "success"
        ? []
        : ["native-shell workflow run is not completed successfully"],
    ),
    check(
      "native-shell-matrix-jobs",
      missingJobs.length === 0 && failedJobs.length === 0,
      `matrix jobs found=${matrixJobs.filter(({ job }) => job).length}/${REQUIRED_NATIVE_SHELL_MATRIX_JOBS.length}`,
      [...missingJobs, ...failedJobs],
    ),
    check(
      "native-package-artifact-step",
      missingArtifactSteps.length === 0,
      `required step=${REQUIRED_PLAN72_STEP_NAME}`,
      missingArtifactSteps,
    ),
  ];
  const missing = checks.flatMap((item) => item.missing);
  return {
    ok: checks.every((item) => item.ok),
    runId: run?.databaseId ?? null,
    workflowName: run?.workflowName ?? null,
    headBranch: run?.headBranch ?? null,
    headSha: run?.headSha ?? null,
    status: run?.status ?? null,
    conclusion: run?.conclusion ?? null,
    createdAt: run?.createdAt ?? null,
    updatedAt: run?.updatedAt ?? null,
    url: run?.url ?? null,
    matrixJobs: matrixJobs
      .filter(({ job }) => job)
      .map(({ name, job }) => ({
        name,
        status: job.status ?? null,
        conclusion: job.conclusion ?? null,
        verifyPackStep: successfulStep(job, REQUIRED_PLAN72_STEP_NAME),
      })),
    checks,
    missing,
  };
}

function argValue(name) {
  const inline = process.argv.find((arg) => arg.startsWith(`${name}=`));
  if (inline) return inline.slice(name.length + 1);
  const index = process.argv.indexOf(name);
  return index >= 0 ? process.argv[index + 1] : undefined;
}

function runGh(args) {
  const result = spawnSync("gh", args, {
    cwd: resolve(import.meta.dirname, ".."),
    encoding: "utf8",
    windowsHide: true,
    timeout: 30_000,
  });
  if (result.error || result.status !== 0) {
    const detail = (result.stderr || result.error?.message || "gh command failed").trim();
    throw new Error(detail);
  }
  return result.stdout;
}

function latestRunId() {
  const output = runGh([
    "run",
    "list",
    "--workflow",
    "native-shell.yml",
    "--limit",
    "1",
    "--json",
    "databaseId",
  ]);
  const runs = JSON.parse(output);
  const id = runs?.[0]?.databaseId;
  if (!id) throw new Error("gh run list did not return a native-shell run id");
  return String(id);
}

function runJsonForId(runId) {
  return JSON.parse(runGh(["run", "view", runId, "--json", GH_RUN_FIELDS]));
}

function loadRunPayload() {
  const runJson = argValue("--run-json");
  if (runJson) {
    const raw = runJson === "-" ? readFileSync(0, "utf8") : readFileSync(runJson, "utf8");
    return JSON.parse(raw);
  }
  const runId = argValue("--run-id") || (process.argv.includes("--latest") ? latestRunId() : undefined);
  if (!runId) {
    throw new Error("Provide --run-id <id>, --latest, or --run-json <path>");
  }
  return runJsonForId(runId);
}

function printSummary(report) {
  const label = report.ok ? "pass" : "blocked";
  console.log(`Plan 72 CI evidence: ${label}`);
  console.log(`Run: ${report.runId || "unknown"} ${report.url || ""}`.trim());
  for (const check of report.checks) {
    console.log(`- ${check.ok ? "ok" : "missing"} ${check.id}: ${check.evidence}`);
    for (const missing of check.missing) console.log(`  - ${missing}`);
  }
}

function main() {
  let report;
  try {
    report = verifyPlan72NativeShellRun(loadRunPayload());
  } catch (error) {
    console.error(`Plan 72 CI evidence error: ${error.message}`);
    process.exit(2);
  }
  if (process.argv.includes("--json")) {
    console.log(JSON.stringify(report, null, 2));
  } else {
    printSummary(report);
  }
  process.exit(report.ok ? 0 : 1);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main();
}
