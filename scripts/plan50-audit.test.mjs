#!/usr/bin/env node
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import test from "node:test";

const repoRoot = resolve(import.meta.dirname, "..");
const realWorkflowPath = join(repoRoot, ".github", "workflows", "native-shell.yml");
const realManifestWriterPath = join(repoRoot, "scripts", "plan50-write-evidence-manifest.mjs");
const defaultTestPlanDir = mkdtempSync(join(tmpdir(), "oppi-plan50-default-plan-"));
const defaultTestPlanPath = join(defaultTestPlanDir, "50-standalone-oppi-finish-line.md");
writeFileSync(defaultTestPlanPath, [
  "# Plan 50 test fixture",
  "- [ ] `/background` list/read/kill is dogfooded through native shell.",
  "- [ ] Terminal restore after abort/panic is checked on Windows and Unix.",
  "- [ ] Sandboxed background tasks instead of full-access/unrestricted background shell tasks; default promotion requires this.",
  "",
].join("\n"), "utf8");
process.env.OPPI_PLAN50_PLAN_PATH = process.env.OPPI_PLAN50_PLAN_PATH || defaultTestPlanPath;
process.env.OPPI_PLAN50_TEST_SANDBOX_READY = process.env.OPPI_PLAN50_TEST_SANDBOX_READY || "0";

function tempEvidenceRoot() {
  return mkdtempSync(join(tmpdir(), "oppi-plan50-audit-evidence-"));
}

function localRunnerOs() {
  if (process.platform === "win32") return "Windows";
  if (process.platform === "darwin") return "macOS";
  if (process.platform === "linux") return "Linux";
  return process.platform;
}

function writeLocalTerminalEvidence(root, runnerOs = localRunnerOs()) {
  mkdirSync(root, { recursive: true });
  writeFileSync(join(root, `terminal-cleanup-lifecycle-${runnerOs}.log`), "test result: ok. 1 passed; 0 failed\n", "utf8");
  writeFileSync(join(root, `terminal-cleanup-reset-${runnerOs}.log`), "test result: ok. 1 passed; 0 failed\n", "utf8");
}

function writeLocalBackgroundEvidence(root, patch = {}) {
  mkdirSync(root, { recursive: true });
  const path = join(root, "tui-dogfood-strict-local.json");
  writeFileSync(path, JSON.stringify({
    ok: true,
    strictBackgroundLifecycle: true,
    scenarios: [
      { name: "background-sandbox-execution", ok: true, status: "started, list=true, read=true, kill=true" },
    ],
    ...patch,
  }, null, 2), "utf8");
  return path;
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

function writeRunnerEvidence(root, runnerOs, { strict = false } = {}) {
  const dir = join(root, `plan50-native-shell-evidence-${matrixOsForRunner(runnerOs)}`);
  mkdirSync(dir, { recursive: true });
  const files = [
    `terminal-cleanup-lifecycle-${runnerOs}.log`,
    `terminal-cleanup-reset-${runnerOs}.log`,
    `tui-smoke-${runnerOs}.json`,
    `tui-dogfood-${runnerOs}.json`,
  ];
  writeFileSync(join(dir, `terminal-cleanup-lifecycle-${runnerOs}.log`), "test result: ok. 1 passed; 0 failed\n", "utf8");
  writeFileSync(join(dir, `terminal-cleanup-reset-${runnerOs}.log`), "test result: ok. 1 passed; 0 failed\n", "utf8");
  writeFileSync(join(dir, `tui-smoke-${runnerOs}.json`), JSON.stringify({
    ok: true,
    diagnostics: ["native shell mock smoke completed"],
  }, null, 2), "utf8");
  writeFileSync(join(dir, `tui-dogfood-${runnerOs}.json`), JSON.stringify({
    ok: true,
    scenarios: [
      { name: "background-sandbox-execution", ok: true, status: strict ? "started, list=true, read=true, kill=true" : "sandbox-unavailable-denied" },
    ],
  }, null, 2), "utf8");
  if (runnerOs === "Linux") {
    files.push("linux-bubblewrap-host-sandbox-Linux.log");
    writeFileSync(join(dir, "linux-bubblewrap-host-sandbox-Linux.log"), "test result: ok. 1 passed; 0 failed\n", "utf8");
  }
  if (strict) {
    files.push(`tui-dogfood-strict-${runnerOs}.json`);
    writeFileSync(join(dir, `tui-dogfood-strict-${runnerOs}.json`), JSON.stringify({
      ok: true,
      strictBackgroundLifecycle: true,
      scenarios: [
        { name: "background-sandbox-execution", ok: true, status: "started, list=true, read=true, kill=true" },
      ],
    }, null, 2), "utf8");
  }
  const manifestFiles = [...files].sort();
  writeFileSync(join(dir, `plan50-native-shell-evidence-${runnerOs}.json`), JSON.stringify({
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
    fileSha256: Object.fromEntries(manifestFiles.map((file) => [file, sha256File(join(dir, file))])),
  }, null, 2), "utf8");
}

test("plan50 audit separates implemented Windows background adapter from host setup gate", () => {
  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
    env: {
      ...process.env,
      OPPI_PLAN50_DEFAULT_EVIDENCE_ROOT: tempEvidenceRoot(),
      OPPI_PLAN50_TEST_UNIX_RUNNER_AVAILABLE: "1",
      OPPI_PLAN50_TEST_CI_CHANGES: "M packages/cli/src/main.ts\n?? .github/workflows/native-shell.yml\n?? crates/",
    },
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(typeof payload.githubCli?.installed, "boolean");
  assert.equal(typeof payload.githubCli?.authenticated, "boolean");
  const adapterCheck = payload.checks.find((check) => check.id === "windows-background-adapter-implemented");
  assert.equal(adapterCheck?.ok, true);
  assert.match(adapterCheck.evidence, /restricted-token background adapter/i);
  const localTerminalCheck = payload.checks.find((check) => check.id === "terminal-restore-local-platform");
  assert.equal(localTerminalCheck?.ok, false);
  assert.match(localTerminalCheck.evidence, /--local-terminal-evidence-root/);
  const unixTerminalCheck = payload.checks.find((check) => check.id === "terminal-restore-unix-local");
  assert.equal(unixTerminalCheck?.ok, false);
  assert.match(unixTerminalCheck.evidence, /requires captured Windows plus Unix terminal cleanup evidence/i);
  assert.equal(payload.closeoutChecklist?.readyToComplete, false);
  assert.deepEqual(payload.closeoutChecklist?.missing, [
    "background-native-lifecycle",
    "terminal-restore-windows-unix",
    "sandboxed-background-default-promotion",
  ]);
  assert.deepEqual(
    Object.fromEntries(payload.closeoutChecklist.successCriteria.map((criterion) => [criterion.id, criterion.planLine])),
    {
      "background-native-lifecycle": payload.unchecked.find((row) => row.text.includes("`/background` list/read/kill"))?.line,
      "terminal-restore-windows-unix": payload.unchecked.find((row) => row.text.includes("Terminal restore after abort/panic"))?.line,
      "sandboxed-background-default-promotion": payload.unchecked.find((row) => row.text.includes("Sandboxed background tasks"))?.line,
    },
  );
  assert.ok(
    payload.closeoutChecklist.routes.some((route) =>
      route.id === "multi-os-ci-artifacts"
        && route.curatedPublishSet.includes(".github/workflows/native-shell.yml")
        && route.curatedPublishSet.includes(".gitignore")
        && route.curatedPublishSet.includes("crates")
        && route.curatedPublishSet.includes("package.json")
        && route.curatedPublishSet.includes("pnpm-lock.yaml")
        && route.curatedPublishSet.includes("pnpm-workspace.yaml")
        && route.curatedPublishSet.includes("packages/cli")
        && route.curatedPublishSet.includes("packages/native")
        && route.curatedPublishSet.includes("packages/natives")
        && route.curatedPublishSet.includes("packages/pi-package/skills/graphify")
        && route.curatedPublishSet.includes("systemprompts/goals")
        && route.curatedPublishSet.includes("systemprompts/main/oppi-feature-routing-system-append.md")
        && route.curatedPublishSet.includes("systemprompts/experiments/promptname_a/oppi-feature-routing-system-append.md")
        && route.curatedPublishSet.includes("systemprompts/experiments/promptname_b/oppi-feature-routing-system-append.md")
        && route.steps.includes("gh auth status")
    ),
    JSON.stringify(payload.closeoutChecklist, null, 2),
  );
  const ciRoute = payload.closeoutChecklist.routes.find((route) => route.id === "multi-os-ci-artifacts");
  const localRoute = payload.closeoutChecklist.routes.find((route) => route.id === "local-windows-sandbox");
  assert.equal(payload.closeoutChecklist.recommendedRouteId, "multi-os-ci-artifacts");
  assert.equal(localRoute.approvalPhrase, "approve Windows sandbox setup for Plan 50");
  assert.equal(ciRoute.approvalPhrase, "approve Plan 50 CI evidence route");
  assert.equal(ciRoute.reviewDocument, ".oppi-plans/50-ci-evidence-publish-set-review.md");
  assert.equal(localRoute.requiresElevation, true);
  assert.equal(localRoute.hostMutation, true);
  assert.equal(ciRoute.requiresNetwork, true);
  assert.equal(ciRoute.requiresGitHubAuth, true);
  assert.equal(ciRoute.requiresCommitPush, true);
  assert.equal(ciRoute.localMutation, true);
  assert.equal(ciRoute.remoteMutation, true);
  assert.equal(localRoute.completionScope, "partial-on-this-host");
  assert.deepEqual(localRoute.requiresAdditionalEvidence, [
    "Unix terminal restore evidence from WSL/Unix or CI.",
  ]);
  assert.equal(ciRoute.completionScope, "all-remaining-rows");
  assert.deepEqual(ciRoute.requiresAdditionalEvidence, []);
  assert.equal(payload.closeoutChecklist.localNonApprovalCloseout?.available, false);
  assert.deepEqual(payload.closeoutChecklist.localNonApprovalCloseout?.requiredActions, [
    "unix-terminal-restore-evidence",
  ]);
  assert.ok(
    payload.closeoutChecklist.localNonApprovalCloseout?.blockedBy?.some((blocker) =>
      /Local sandbox adapter is not configured/i.test(blocker)
    ),
    JSON.stringify(payload.closeoutChecklist.localNonApprovalCloseout, null, 2),
  );
  assert.equal(payload.userApproval?.required, true);
  assert.deepEqual(payload.userApproval?.options?.map((option) => option.routeId), [
    "local-windows-sandbox",
    "multi-os-ci-artifacts",
  ]);
  assert.deepEqual(payload.userApproval?.options?.map((option) => option.approvalPhrase), [
    "approve Windows sandbox setup for Plan 50",
    "approve Plan 50 CI evidence route",
  ]);
  assert.deepEqual(payload.userApproval?.options?.map((option) => option.completionScope), [
    "partial-on-this-host",
    "all-remaining-rows",
  ]);
  const localApproval = payload.userApproval?.options?.find((option) => option.routeId === "local-windows-sandbox");
  assert.equal(localApproval?.requiresElevation, true);
  assert.equal(localApproval?.hostMutation, true);
  assert.deepEqual(localRoute.approvalGatedSteps, [
    "Open an elevated PowerShell in the repo.",
    "Run the Windows sandbox setup with --yes.",
    "Verify sandbox status before capturing strict background evidence.",
  ]);
  assert.deepEqual(localApproval?.approvalGatedSteps, localRoute.approvalGatedSteps);
  const ciApproval = payload.userApproval?.options?.find((option) => option.routeId === "multi-os-ci-artifacts");
  assert.equal(localApproval?.recommended, false);
  assert.equal(ciApproval?.recommended, true);
  assert.equal(ciApproval?.requiresNetwork, true);
  assert.equal(ciApproval?.requiresGitHubAuth, true);
  assert.equal(ciApproval?.requiresCommitPush, true);
  assert.equal(ciApproval?.localMutation, true);
  assert.equal(ciApproval?.remoteMutation, true);
  assert.equal(ciRoute.githubAuthMutation, true);
  assert.equal(ciApproval?.githubAuthMutation, true);
  assert.equal(ciApproval?.reviewDocument, ".oppi-plans/50-ci-evidence-publish-set-review.md");
  assert.deepEqual(ciRoute.approvalGatedSteps, [
    "Review the curated Plan 50 publish set.",
    "Stage only the curated Plan 50 publish set.",
    "Commit the curated Plan 50 publish set.",
    "Repair GitHub CLI auth if gh auth status is invalid.",
    "Push the selected ref before running GitHub Actions.",
  ]);
  assert.ok(
    ciRoute.blockedBy.includes("Relevant Plan 50 workflow/runtime changes must be reviewed, staged, tested, committed, and pushed after GitHub auth is valid."),
    JSON.stringify(ciRoute.blockedBy, null, 2),
  );
  assert.ok(
    !ciRoute.blockedBy.includes("Relevant Plan 50 workflow/runtime changes must be reviewed, committed, and pushed."),
    JSON.stringify(ciRoute.blockedBy, null, 2),
  );
  assert.deepEqual(ciApproval?.approvalGatedSteps, ciRoute.approvalGatedSteps);
  assert.equal(new Set(localRoute.steps).size, localRoute.steps.length, JSON.stringify(localRoute, null, 2));
  assert.ok(
    localRoute.steps.some((step) =>
      step.includes("node scripts/plan50-capture-local-background.mjs --output")
        && step.includes("plan50-background-evidence")
        && step.includes(`tui-dogfood-strict-${localRunnerOs()}.json`)
    ),
    JSON.stringify(localRoute, null, 2),
  );
  assert.equal(
    localRoute.steps.filter((step) => step === "node packages/cli/dist/main.js tui dogfood --mock --json --require-background-lifecycle").length,
    0,
    JSON.stringify(localRoute, null, 2),
  );
  const downloadIndex = ciRoute.steps.findIndex((step) => step.startsWith("gh run download "));
  const verifyIndex = ciRoute.steps.findIndex((step) => step.startsWith("node scripts/plan50-evidence-verify.mjs "));
  const applyIndex = ciRoute.steps.indexOf("node scripts/plan50-audit.mjs --evidence-root plan50-downloaded-evidence --apply-evidence --json");
  assert.ok(downloadIndex >= 0, JSON.stringify(ciRoute, null, 2));
  assert.ok(verifyIndex > downloadIndex, JSON.stringify(ciRoute, null, 2));
  assert.ok(applyIndex > verifyIndex, JSON.stringify(ciRoute, null, 2));
  const ciAction = payload.nextActions.find((action) => action.id === "github-ci-evidence-run");
  const actionDownloadIndex = ciAction.verifyAfter.findIndex((step) => step.startsWith("gh run download "));
  const actionVerifyIndex = ciAction.verifyAfter.findIndex((step) => step.includes("plan50-evidence-verify.mjs"));
  const actionApplyIndex = ciAction.verifyAfter.findIndex((step) => step.includes("--apply-evidence"));
  assert.ok(actionDownloadIndex >= 0, JSON.stringify(ciAction, null, 2));
  assert.ok(actionVerifyIndex > actionDownloadIndex, JSON.stringify(ciAction, null, 2));
  assert.ok(actionApplyIndex > actionVerifyIndex, JSON.stringify(ciAction, null, 2));
  assert.ok(
    payload.closeoutChecklist.routes.some((route) =>
        route.id === "local-windows-sandbox"
        && route.requiresExplicitUserApproval === true
        && route.steps.includes("node packages/cli/dist/main.js sandbox setup-windows --yes --json")
        && route.steps.some((step) =>
          step.includes("node scripts/plan50-capture-local-terminal.mjs --output-dir")
            && step.includes("plan50-terminal-evidence"))
        && route.blockedBy.includes("Unix terminal restore evidence is not captured on this host.")
    ),
    JSON.stringify(payload.closeoutChecklist, null, 2),
  );
  const evidenceArtifactCheck = payload.checks.find((check) => check.id === "multi-os-ci-evidence-artifacts-defined");
  assert.equal(evidenceArtifactCheck?.ok, true);
  assert.match(evidenceArtifactCheck.evidence, /retention/i);
  assert.match(evidenceArtifactCheck.evidence, /RUNNER_OS/i);
  assert.match(evidenceArtifactCheck.evidence, /schemaVersion=1/i);
  const aggregateVerifierCheck = payload.checks.find((check) => check.id === "multi-os-ci-evidence-verifier-defined");
  assert.equal(aggregateVerifierCheck?.ok, true);
  assert.match(aggregateVerifierCheck.evidence, /download all Plan 50 evidence artifacts/i);
  const workflowDispatchCheck = payload.checks.find((check) => check.id === "workflow-dispatch-defined");
  assert.equal(workflowDispatchCheck?.ok, true);
  assert.match(workflowDispatchCheck.evidence, /gh workflow run/i);
  const multiOsDogfoodCheck = payload.checks.find((check) => check.id === "multi-os-ci-dogfood-defined");
  assert.equal(multiOsDogfoodCheck?.ok, true);
  assert.ok(
    payload.nextActions.some((action) =>
      action.id === "windows-sandbox-setup-dry-run"
        && action.command === "node packages/cli/dist/main.js sandbox setup-windows --dry-run --json"
        && action.dryRun === true
        && action.hostMutation === false
        && action.requiresExplicitUserApproval === false
        && action.verifyAfter?.includes("node packages/cli/dist/main.js sandbox status --json")
    ),
    JSON.stringify(payload.nextActions, null, 2),
  );
  assert.ok(
    payload.nextActions.some((action) =>
      action.id === "verify-downloaded-ci-evidence"
        && action.routeId === "multi-os-ci-artifacts"
        && action.completionScope === "all-remaining-rows"
        && action.availableOnThisHost === false
        && action.blockedBy?.includes("Downloaded Plan 50 evidence root is not supplied.")
        && action.requiresInput?.includes("downloaded-plan50-evidence-root")
        && Array.isArray(action.requiresAdditionalEvidence)
        && action.requiresAdditionalEvidence.length === 0
        && action.command === "node scripts/plan50-evidence-verify.mjs <downloaded-plan50-evidence-root> --json"
        && action.requiresExplicitUserApproval === false
    ),
    JSON.stringify(payload.nextActions, null, 2),
  );
  assert.ok(
    payload.nextActions.some((action) =>
      action.id === "local-background-lifecycle-evidence"
        && action.routeId === "local-windows-sandbox"
        && action.completionScope === "partial-on-this-host"
        && action.availableOnThisHost === false
        && action.requiresSandboxSetup === true
        && action.blockedBy?.some((blocker) => /sandbox adapter is not configured/i.test(blocker))
        && action.requiresAdditionalEvidence?.includes("Unix terminal restore evidence from WSL/Unix or CI.")
        && action.command === "node packages/cli/dist/main.js tui dogfood --mock --json --require-background-lifecycle"
        && /tui-dogfood-strict-(Windows|Linux|macOS)\.json$/.test(action.evidencePath)
        && action.captureCommand?.includes(action.evidencePath)
        && action.captureCommand?.includes("node scripts/plan50-capture-local-background.mjs --output")
        && action.verifyAfter?.includes(`node scripts/plan50-audit.mjs --local-background-evidence ${action.evidencePath} --json`)
    ),
    JSON.stringify(payload.nextActions, null, 2),
  );
  assert.ok(
    payload.nextActions.some((action) =>
      action.id === "github-ci-evidence-run"
        && action.routeId === "multi-os-ci-artifacts"
        && action.completionScope === "all-remaining-rows"
        && action.availableOnThisHost === false
        && action.requiresNetwork === true
        && action.requiresGitHubAuth === true
        && action.requiresCommitPush === true
        && action.remoteMutation === true
        && action.blockedBy?.includes("Relevant Plan 50 workflow/runtime changes must be committed and pushed before CI can test them.")
        && action.blockedBy?.includes("GitHub CLI auth is not valid.")
        && Array.isArray(action.requiresAdditionalEvidence)
        && action.requiresAdditionalEvidence.length === 0
        && action.command.startsWith("gh workflow run native-shell.yml --ref ")
        && action.requiresExplicitUserApproval === true
        && action.approvalPhrase === "approve Plan 50 CI evidence route"
        && action.preconditions?.includes("Relevant Plan 50 workflow/runtime changes are committed and pushed to the selected ref.")
        && action.verifyAfter?.includes("node scripts/plan50-audit.mjs --evidence-root plan50-downloaded-evidence --apply-evidence --json")
    ),
    JSON.stringify(payload.nextActions, null, 2),
  );
  assert.ok(
    payload.nextActions.some((action) =>
      action.id === "github-auth-preflight"
        && action.command === "gh auth status"
        && action.requiresNetwork === true
        && action.checksGitHubAuth === true
        && typeof action.status === "string"
        && action.verifyAfter?.includes("gh auth login -h github.com")
    ),
    JSON.stringify(payload.nextActions, null, 2),
  );
  assert.ok(
    payload.nextActions.some((action) =>
      action.id === "publish-ci-evidence-inputs"
        && action.routeId === "multi-os-ci-artifacts"
        && action.completionScope === "all-remaining-rows"
        && action.reviewOnly === true
        && action.remoteMutation === false
        && action.requiresExplicitUserApproval === false
        && action.routeRequiresExplicitUserApproval === true
        && action.routeRequiresNetwork === true
        && action.routeRequiresGitHubAuth === true
        && action.routeRequiresCommitPush === true
        && action.routeLocalMutation === true
        && action.routeRemoteMutation === true
        && Array.isArray(action.requiresAdditionalEvidence)
        && action.requiresAdditionalEvidence.length === 0
        && action.reason.includes("remote workflow can only test committed and pushed content")
        && action.routeApprovalPhrase === "approve Plan 50 CI evidence route"
        && action.changes?.includes("M packages/cli/src/main.ts")
        && action.curatedPaths?.includes("packages/cli")
        && action.curatedPaths?.includes("packages/native")
        && action.curatedPaths?.includes("packages/natives")
        && action.curatedPaths?.includes("packages/pi-package/skills/graphify")
        && action.curatedPaths?.includes("systemprompts/goals")
        && action.curatedPaths?.includes("systemprompts/main/oppi-feature-routing-system-append.md")
        && action.curatedPaths?.includes("systemprompts/experiments/promptname_a/oppi-feature-routing-system-append.md")
        && action.curatedPaths?.includes("systemprompts/experiments/promptname_b/oppi-feature-routing-system-append.md")
        && action.curatedPaths?.includes(".gitignore")
        && action.curatedPaths?.includes("package.json")
        && action.curatedPaths?.includes("pnpm-lock.yaml")
        && action.curatedPaths?.includes("pnpm-workspace.yaml")
        && action.command === payload.ciEvidenceInputs?.statusCommand
        && action.reviewDocument === ".oppi-plans/50-ci-evidence-publish-set-review.md"
    ),
    JSON.stringify(payload.nextActions, null, 2),
  );
  const stageAction = payload.nextActions.find((action) => action.id === "stage-ci-evidence-inputs");
  assert.ok(stageAction, JSON.stringify(payload.nextActions, null, 2));
  assert.equal(stageAction.routeId, "multi-os-ci-artifacts");
  assert.equal(stageAction.requiresExplicitUserApproval, true);
  assert.equal(stageAction.approvalPhrase, "approve Plan 50 CI evidence route");
  assert.equal(stageAction.localMutation, true);
  assert.equal(stageAction.remoteMutation, false);
  assert.equal(stageAction.command, 'git add -- "packages/cli/src/main.ts" ".github/workflows/native-shell.yml" "crates/"');
  assert.match(stageAction.reason, /avoid `git add -A`/);
  assert.deepEqual(stageAction.stagePaths, [
    "packages/cli/src/main.ts",
    ".github/workflows/native-shell.yml",
    "crates/",
  ]);
  assert.ok(stageAction.verifyAfter?.includes('git diff --cached --name-status -- "packages/cli/src/main.ts" ".github/workflows/native-shell.yml" "crates/"'));
  assert.ok(stageAction.verifyAfter?.includes('git diff --cached --check -- "packages/cli/src/main.ts" ".github/workflows/native-shell.yml" "crates/"'));

  const localTestAction = payload.nextActions.find((action) => action.id === "verify-plan50-local-tests");
  assert.ok(localTestAction, JSON.stringify(payload.nextActions, null, 2));
  assert.equal(localTestAction.routeId, "multi-os-ci-artifacts");
  assert.equal(localTestAction.completionScope, "all-remaining-rows");
  assert.equal(localTestAction.command, "pnpm run plan50:test");
  assert.equal(localTestAction.requiresExplicitUserApproval, false);
  assert.equal(localTestAction.remoteMutation, false);
  assert.match(localTestAction.reason, /before committing/i);

  const commitAction = payload.nextActions.find((action) => action.id === "commit-ci-evidence-inputs");
  assert.ok(commitAction, JSON.stringify(payload.nextActions, null, 2));
  assert.equal(commitAction.command, 'git commit -m "Prepare Plan 50 native evidence gates"');
  assert.equal(commitAction.requiresExplicitUserApproval, true);
  assert.equal(commitAction.localMutation, true);
  assert.equal(commitAction.remoteMutation, false);
  assert.ok(commitAction.preconditions?.includes("Only curated Plan 50 paths are staged."));

  const githubAuthAction = payload.nextActions.find((action) => action.id === "github-auth-preflight");
  assert.ok(githubAuthAction, JSON.stringify(payload.nextActions, null, 2));
  assert.equal(githubAuthAction.command, "gh auth status");
  assert.equal(githubAuthAction.requiresNetwork, true);
  assert.equal(githubAuthAction.checksGitHubAuth, true);

  const githubAuthRepairAction = payload.nextActions.find((action) => action.id === "github-auth-repair");
  assert.ok(githubAuthRepairAction, JSON.stringify(payload.nextActions, null, 2));
  assert.equal(githubAuthRepairAction.command, "gh auth login -h github.com");
  assert.equal(githubAuthRepairAction.requiresExplicitUserApproval, true);
  assert.equal(githubAuthRepairAction.approvalPhrase, "approve Plan 50 CI evidence route");
  assert.equal(githubAuthRepairAction.requiresNetwork, true);
  assert.equal(githubAuthRepairAction.githubAuthMutation, true);

  const pushAction = payload.nextActions.find((action) => action.id === "push-ci-evidence-inputs");
  assert.ok(pushAction, JSON.stringify(payload.nextActions, null, 2));
  assert.match(pushAction.command, /^git push origin /);
  assert.equal(pushAction.requiresExplicitUserApproval, true);
  assert.equal(pushAction.requiresNetwork, true);
  assert.equal(pushAction.requiresGitHubAuth, true);
  assert.equal(pushAction.requiresCommitPush, true);
  assert.equal(pushAction.remoteMutation, true);
  assert.ok(pushAction.preconditions?.includes("Curated Plan 50 publish set is committed on the selected ref."));

  assert.ok(ciRoute.steps.includes(stageAction.command), JSON.stringify(ciRoute, null, 2));
  assert.ok(ciRoute.steps.includes(localTestAction.command), JSON.stringify(ciRoute, null, 2));
  assert.ok(ciRoute.steps.includes(commitAction.command), JSON.stringify(ciRoute, null, 2));
  assert.ok(ciRoute.steps.includes(githubAuthAction.command), JSON.stringify(ciRoute, null, 2));
  assert.ok(ciRoute.steps.includes(githubAuthRepairAction.command), JSON.stringify(ciRoute, null, 2));
  assert.ok(ciRoute.steps.includes(pushAction.command), JSON.stringify(ciRoute, null, 2));
  assert.ok(
    payload.nextActions.indexOf(commitAction) < payload.nextActions.indexOf(githubAuthAction),
    JSON.stringify(payload.nextActions.map((action) => action.id), null, 2),
  );
  assert.ok(
    payload.nextActions.indexOf(githubAuthAction) < payload.nextActions.indexOf(pushAction),
    JSON.stringify(payload.nextActions.map((action) => action.id), null, 2),
  );
  assert.ok(
    payload.nextActions.indexOf(githubAuthAction) < payload.nextActions.indexOf(githubAuthRepairAction),
    JSON.stringify(payload.nextActions.map((action) => action.id), null, 2),
  );
  assert.ok(
    payload.nextActions.indexOf(githubAuthRepairAction) < payload.nextActions.indexOf(pushAction),
    JSON.stringify(payload.nextActions.map((action) => action.id), null, 2),
  );
  assert.ok(
    ciRoute.steps.indexOf(localTestAction.command) < ciRoute.steps.indexOf(commitAction.command),
    JSON.stringify(ciRoute.steps, null, 2),
  );
  assert.ok(
    ciRoute.steps.indexOf(commitAction.command) < ciRoute.steps.indexOf(githubAuthAction.command),
    JSON.stringify(ciRoute.steps, null, 2),
  );
  assert.ok(
    ciRoute.steps.indexOf(githubAuthAction.command) < ciRoute.steps.indexOf(githubAuthRepairAction.command),
    JSON.stringify(ciRoute.steps, null, 2),
  );
  assert.ok(
    ciRoute.steps.indexOf(githubAuthRepairAction.command) < ciRoute.steps.indexOf(pushAction.command),
    JSON.stringify(ciRoute.steps, null, 2),
  );
  assert.deepEqual(payload.ciEvidenceInputs?.changes, [
    "M packages/cli/src/main.ts",
    "?? .github/workflows/native-shell.yml",
    "?? crates/",
  ]);
  assert.deepEqual(payload.ciEvidenceInputs?.stagePaths, [
    "packages/cli/src/main.ts",
    ".github/workflows/native-shell.yml",
    "crates/",
  ]);
  assert.equal(payload.ciEvidenceInputs?.count, 3);
  assert.equal(payload.ciEvidenceInputs?.dirty, true);
  assert.ok(payload.ciEvidenceInputs?.curatedPaths.includes("package.json"));
  assert.ok(payload.ciEvidenceInputs?.curatedPaths.includes(".gitignore"));
  assert.ok(payload.ciEvidenceInputs?.curatedPaths.includes("pnpm-lock.yaml"));
  assert.ok(payload.ciEvidenceInputs?.curatedPaths.includes("pnpm-workspace.yaml"));
  assert.ok(payload.ciEvidenceInputs?.curatedPaths.includes("packages/native"));
  assert.ok(payload.ciEvidenceInputs?.curatedPaths.includes("packages/natives"));
  assert.ok(payload.ciEvidenceInputs?.curatedPaths.includes("packages/pi-package/skills/graphify"));
  assert.ok(payload.ciEvidenceInputs?.curatedPaths.includes("systemprompts/goals"));
  assert.ok(payload.ciEvidenceInputs?.curatedPaths.includes("systemprompts/main/oppi-feature-routing-system-append.md"));
  assert.ok(payload.ciEvidenceInputs?.curatedPaths.includes("systemprompts/experiments/promptname_a/oppi-feature-routing-system-append.md"));
  assert.ok(payload.ciEvidenceInputs?.curatedPaths.includes("systemprompts/experiments/promptname_b/oppi-feature-routing-system-append.md"));
  assert.ok(payload.ciEvidenceInputs?.curatedPaths.includes("scripts/plan50-capture-local-background.mjs"));
  assert.ok(payload.ciEvidenceInputs?.curatedPaths.includes("scripts/plan50-capture-local-terminal.mjs"));
  assert.ok(payload.ciEvidenceInputs?.curatedPaths.includes("scripts/plan50-test.mjs"));
  assert.ok(payload.ciEvidenceInputs?.curatedPaths.includes("scripts/plan50-evidence-verify.mjs"));
  assert.ok(
    payload.nextActions.some((action) =>
      action.id === "windows-sandbox-setup-explicit-approval"
        && action.routeId === "local-windows-sandbox"
        && action.completionScope === "partial-on-this-host"
        && action.requiresAdditionalEvidence?.includes("Unix terminal restore evidence from WSL/Unix or CI.")
        && action.command === "node packages/cli/dist/main.js sandbox setup-windows --yes --json"
        && action.requiresExplicitUserApproval === true
        && action.requiresElevation === true
        && action.hostMutation === true
        && action.approvalPhrase === "approve Windows sandbox setup for Plan 50"
    ),
    JSON.stringify(payload.nextActions, null, 2),
  );
  assert.ok(
    payload.nextActions.some((action) =>
      action.id === "unix-terminal-restore-evidence"
        && action.routeId === "local-windows-sandbox"
        && action.completionScope === "partial-on-this-host"
        && action.availableOnThisHost === true
        && Array.isArray(action.blockedBy)
        && action.blockedBy.length === 0
        && action.requiresAdditionalEvidence?.includes("Unix terminal restore evidence from WSL/Unix or CI.")
        && action.command.includes("node scripts/plan50-capture-local-terminal.mjs --output-dir")
        && action.command.includes("plan50-terminal-evidence")
        && action.evidenceRoot?.includes("plan50-terminal-evidence")
        && action.verifyAfter?.some((step) =>
          step.includes("node scripts/plan50-audit.mjs --local-terminal-evidence-root")
            && step.includes("plan50-terminal-evidence")
            && step.includes("--json"))
    ),
    JSON.stringify(payload.nextActions, null, 2),
  );
});

test("plan50 audit honors OPPI_PLAN50_PLAN_PATH for clean CI helper runs", () => {
  const planDir = mkdtempSync(join(tmpdir(), "oppi-plan50-env-plan-"));
  const planPath = join(planDir, "50-standalone-oppi-finish-line.md");
  writeFileSync(planPath, [
    "# Plan 50 env fixture",
    "- [ ] `/background` list/read/kill is dogfooded through native shell.",
    "- [ ] Terminal restore after abort/panic is checked on Windows and Unix.",
    "- [ ] Sandboxed background tasks instead of full-access/unrestricted background shell tasks; default promotion requires this.",
    "",
  ].join("\n"), "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
    env: {
      ...process.env,
      OPPI_PLAN50_PLAN_PATH: planPath,
      OPPI_PLAN50_DEFAULT_EVIDENCE_ROOT: tempEvidenceRoot(),
      OPPI_PLAN50_TEST_UNIX_RUNNER_AVAILABLE: "1",
      OPPI_PLAN50_TEST_CI_CHANGES: "",
    },
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.planPath), planPath);
  assert.equal(payload.planPathSource, "OPPI_PLAN50_PLAN_PATH");
  assert.deepEqual(payload.unchecked.map((row) => row.line), [2, 3, 4]);
  assert.deepEqual(payload.closeoutChecklist?.missing, [
    "background-native-lifecycle",
    "terminal-restore-windows-unix",
    "sandboxed-background-default-promotion",
  ]);
});

test("plan50 audit warns about dirty workflow files outside the curated CI publish set", () => {
  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
    env: {
      ...process.env,
      OPPI_PLAN50_DEFAULT_EVIDENCE_ROOT: tempEvidenceRoot(),
      OPPI_PLAN50_TEST_UNIX_RUNNER_AVAILABLE: "1",
      OPPI_PLAN50_TEST_CI_CHANGES: [
        "M packages/cli/src/main.ts",
        "?? .github/workflows/native-shell.yml",
      ].join("\n"),
      OPPI_PLAN50_TEST_SENSITIVE_CI_CHANGES: [
        "?? .github/workflows/native-shell.yml",
        "?? .github/workflows/sandbox.yml",
      ].join("\n"),
    },
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.deepEqual(payload.ciEvidenceInputs?.excludedDirtyPaths, [
    ".github/workflows/sandbox.yml",
  ]);

  const publishAction = payload.nextActions.find((action) => action.id === "publish-ci-evidence-inputs");
  assert.ok(publishAction, JSON.stringify(payload.nextActions, null, 2));
  assert.deepEqual(publishAction.excludedDirtyPaths, [
    ".github/workflows/sandbox.yml",
  ]);

  const stageAction = payload.nextActions.find((action) => action.id === "stage-ci-evidence-inputs");
  assert.ok(stageAction, JSON.stringify(payload.nextActions, null, 2));
  assert.deepEqual(stageAction.stagePaths, [
    "packages/cli/src/main.ts",
    ".github/workflows/native-shell.yml",
  ]);
});

test("plan50 audit reports dirty paths outside the curated CI publish set", () => {
  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
    env: {
      ...process.env,
      OPPI_PLAN50_DEFAULT_EVIDENCE_ROOT: tempEvidenceRoot(),
      OPPI_PLAN50_TEST_UNIX_RUNNER_AVAILABLE: "1",
      OPPI_PLAN50_TEST_CI_CHANGES: [
        "M packages/cli/src/main.ts",
        "?? .github/workflows/native-shell.yml",
      ].join("\n"),
      OPPI_PLAN50_TEST_ALL_CHANGES: [
        "M packages/cli/src/main.ts",
        "?? .github/workflows/native-shell.yml",
        "M packages/pi-package/package.json",
        "?? docs/native-ui-pi-parity-sanity.md",
      ].join("\n"),
    },
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(payload.ciEvidenceInputs?.allDirtyCount, 4);
  assert.equal(payload.ciEvidenceInputs?.nonCuratedDirtyCount, 2);
  assert.deepEqual(payload.ciEvidenceInputs?.nonCuratedDirtyChanges, [
    "M packages/pi-package/package.json",
    "?? docs/native-ui-pi-parity-sanity.md",
  ]);

  const publishAction = payload.nextActions.find((action) => action.id === "publish-ci-evidence-inputs");
  assert.ok(publishAction, JSON.stringify(payload.nextActions, null, 2));
  assert.match(
    publishAction.reason,
    /Review, stage, test, commit, repair GitHub auth if needed, and push relevant Plan 50 workflow\/runtime changes before running GitHub CI/,
  );
  assert.doesNotMatch(
    publishAction.reason,
    /Review, commit, repair GitHub auth if needed, and push relevant Plan 50 workflow\/runtime changes before running GitHub CI/,
  );
  assert.doesNotMatch(
    publishAction.reason,
    /Review, commit, and push relevant Plan 50 workflow\/runtime changes before running GitHub CI/,
  );
  assert.equal(publishAction.nonCuratedDirtyCount, 2);
  assert.deepEqual(publishAction.nonCuratedDirtySample, [
    "M packages/pi-package/package.json",
    "?? docs/native-ui-pi-parity-sanity.md",
  ]);

  const ciRoute = payload.closeoutChecklist.routes.find((route) => route.id === "multi-os-ci-artifacts");
  assert.equal(ciRoute?.nonCuratedDirtyCount, 2);
  const ciApproval = payload.userApproval?.options?.find((option) => option.routeId === "multi-os-ci-artifacts");
  assert.equal(ciApproval?.nonCuratedDirtyCount, 2);
});

test("plan50 audit reports --plan-path source and prefers it over env plan path", () => {
  const explicitPlanDir = mkdtempSync(join(tmpdir(), "oppi-plan50-explicit-source-"));
  const explicitPlanPath = join(explicitPlanDir, "50-standalone-oppi-finish-line.md");
  writeFileSync(explicitPlanPath, [
    "# Plan 50 explicit source fixture",
    "- [ ] `/background` list/read/kill is dogfooded through native shell.",
    "- [ ] Terminal restore after abort/panic is checked on Windows and Unix.",
    "- [ ] Sandboxed background tasks instead of full-access/unrestricted background shell tasks; default promotion requires this.",
    "",
  ].join("\n"), "utf8");

  const envPlanDir = mkdtempSync(join(tmpdir(), "oppi-plan50-env-shadow-"));
  const envPlanPath = join(envPlanDir, "50-standalone-oppi-finish-line.md");
  writeFileSync(envPlanPath, [
    "# Plan 50 env shadow fixture",
    "- [ ] `/background` list/read/kill is dogfooded through native shell.",
    "",
    "",
    "- [ ] Terminal restore after abort/panic is checked on Windows and Unix.",
    "- [ ] Sandboxed background tasks instead of full-access/unrestricted background shell tasks; default promotion requires this.",
    "",
  ].join("\n"), "utf8");

  const result = spawnSync(process.execPath, [
    "scripts/plan50-audit.mjs",
    "--plan-path",
    explicitPlanPath,
    "--json",
  ], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
    env: {
      ...process.env,
      OPPI_PLAN50_PLAN_PATH: envPlanPath,
      OPPI_PLAN50_DEFAULT_EVIDENCE_ROOT: tempEvidenceRoot(),
      OPPI_PLAN50_TEST_UNIX_RUNNER_AVAILABLE: "1",
      OPPI_PLAN50_TEST_CI_CHANGES: "",
    },
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.planPath), explicitPlanPath);
  assert.equal(payload.planPathSource, "--plan-path");
  assert.deepEqual(payload.unchecked.map((row) => row.line), [2, 3, 4]);
});

test("plan50 audit apply mode rejects env-selected plan without explicit plan path", () => {
  const root = tempEvidenceRoot();
  writeRunnerEvidence(root, "Windows");
  writeRunnerEvidence(root, "Linux", { strict: true });
  writeRunnerEvidence(root, "macOS");

  const envPlanDir = mkdtempSync(join(tmpdir(), "oppi-plan50-env-apply-"));
  const envPlanPath = join(envPlanDir, "50-standalone-oppi-finish-line.md");
  writeFileSync(envPlanPath, [
    "# Plan 50 env apply fixture",
    "- [ ] `/background` list/read/kill is dogfooded through native shell.",
    "- [ ] Terminal restore after abort/panic is checked on Windows and Unix.",
    "- [ ] Sandboxed background tasks instead of full-access/unrestricted background shell tasks; default promotion requires this.",
    "",
  ].join("\n"), "utf8");

  const result = spawnSync(process.execPath, [
    "scripts/plan50-audit.mjs",
    "--evidence-root",
    root,
    "--apply-evidence",
    "--json",
  ], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
    env: {
      ...process.env,
      OPPI_PLAN50_PLAN_PATH: envPlanPath,
      OPPI_PLAN50_DEFAULT_EVIDENCE_ROOT: tempEvidenceRoot(),
    },
  });
  assert.equal(result.status, 2, result.stderr || result.stdout);
  assert.match(result.stderr, /OPPI_PLAN50_PLAN_PATH requires an explicit --plan-path/);
});

test("plan50 audit marks Unix terminal local action unavailable on Windows without WSL", () => {
  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
    env: {
      ...process.env,
      OPPI_PLAN50_DEFAULT_EVIDENCE_ROOT: tempEvidenceRoot(),
      OPPI_PLAN50_TEST_CI_CHANGES: "M packages/cli/src/main.ts\n?? .github/workflows/native-shell.yml\n?? crates/",
    },
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  const action = payload.nextActions.find((candidate) => candidate.id === "unix-terminal-restore-evidence");
  assert.ok(action, JSON.stringify(payload.nextActions, null, 2));
  if (process.platform === "win32") {
    assert.equal(action.availableOnThisHost, false);
    assert.equal(action.requiresUnixRunner, true);
    assert.ok(
      action.blockedBy?.some((blocker) => /No installed WSL distribution/i.test(blocker)),
      JSON.stringify(action, null, 2),
    );
  } else {
    assert.equal(action.availableOnThisHost, true);
    assert.deepEqual(action.blockedBy, []);
  }
});

test("plan50 audit marks local background action available after sandbox setup", () => {
  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
    env: {
      ...process.env,
      OPPI_PLAN50_DEFAULT_EVIDENCE_ROOT: tempEvidenceRoot(),
      OPPI_PLAN50_TEST_SANDBOX_READY: "1",
      OPPI_PLAN50_TEST_CI_CHANGES: "M packages/cli/src/main.ts\n?? .github/workflows/native-shell.yml\n?? crates/",
    },
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  const action = payload.nextActions.find((candidate) => candidate.id === "local-background-lifecycle-evidence");
  assert.ok(action, JSON.stringify(payload.nextActions, null, 2));
  assert.equal(action.availableOnThisHost, true);
  assert.deepEqual(action.blockedBy, []);
});

test("plan50 audit reports when local non-approval closeout actions can cover remaining rows", () => {
  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
    env: {
      ...process.env,
      OPPI_PLAN50_DEFAULT_EVIDENCE_ROOT: tempEvidenceRoot(),
      OPPI_PLAN50_TEST_SANDBOX_READY: "1",
      OPPI_PLAN50_TEST_UNIX_RUNNER_AVAILABLE: "1",
      OPPI_PLAN50_TEST_CI_CHANGES: "M packages/cli/src/main.ts\n?? .github/workflows/native-shell.yml\n?? crates/",
    },
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(payload.closeoutChecklist.localNonApprovalCloseout?.available, true);
  assert.deepEqual(payload.closeoutChecklist.localNonApprovalCloseout?.requiredActions, [
    "local-background-lifecycle-evidence",
    "unix-terminal-restore-evidence",
  ]);
  assert.deepEqual(payload.closeoutChecklist.localNonApprovalCloseout?.blockedBy, []);
});

test("plan50 audit accepts local strict background lifecycle evidence", () => {
  const root = tempEvidenceRoot();
  const backgroundPath = writeLocalBackgroundEvidence(root);

  const result = spawnSync(process.execPath, [
    "scripts/plan50-audit.mjs",
    "--local-background-evidence",
    backgroundPath,
    "--json",
  ], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  const localBackgroundCheck = payload.checks.find((check) => check.id === "local-background-lifecycle-evidence");
  assert.equal(localBackgroundCheck?.ok, true);
  assert.match(localBackgroundCheck.evidence, /started\/list\/read\/kill/);
  assert.equal(payload.checks.find((check) => check.id === "sandboxed-background-local")?.ok, true);
  assert.deepEqual(
    payload.closeoutChecklist.successCriteria.map((criterion) => [criterion.id, criterion.status]),
    [
      ["background-native-lifecycle", "evidence-ready"],
      ["terminal-restore-windows-unix", "open"],
      ["sandboxed-background-default-promotion", "evidence-ready"],
    ],
  );
});

test("plan50 audit rejects incomplete local strict background lifecycle evidence", () => {
  const root = tempEvidenceRoot();
  const backgroundPath = writeLocalBackgroundEvidence(root, {
    scenarios: [
      { name: "background-sandbox-execution", ok: true, status: "started, list=true, read=true, kill=false" },
    ],
  });

  const result = spawnSync(process.execPath, [
    "scripts/plan50-audit.mjs",
    "--local-background-evidence",
    backgroundPath,
    "--json",
  ], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(payload.checks.find((check) => check.id === "local-background-lifecycle-evidence")?.ok, false);
  assert.equal(
    payload.closeoutChecklist.successCriteria.find((criterion) => criterion.id === "background-native-lifecycle")?.status,
    "open",
  );
});

test("plan50 audit rejects contradictory local strict background lifecycle evidence", () => {
  const root = tempEvidenceRoot();
  const backgroundPath = writeLocalBackgroundEvidence(root, {
    scenarios: [
      { name: "background-sandbox-execution", ok: true, status: "sandbox-unavailable-denied; started, list=true, read=true, kill=true" },
    ],
  });

  const result = spawnSync(process.execPath, [
    "scripts/plan50-audit.mjs",
    "--local-background-evidence",
    backgroundPath,
    "--json",
  ], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(payload.checks.find((check) => check.id === "local-background-lifecycle-evidence")?.ok, false);
  assert.equal(payload.checks.find((check) => check.id === "sandboxed-background-local")?.ok, false);
  assert.deepEqual(payload.localPlanRowsReadyToCheck, []);
});

test("plan50 audit does not treat sandbox adapter readiness as background lifecycle evidence", () => {
  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
    env: {
      ...process.env,
      OPPI_PLAN50_TEST_SANDBOX_READY: "1",
      OPPI_PLAN50_TEST_CI_CHANGES: "",
    },
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(payload.checks.find((check) => check.id === "sandbox-adapter-local-ready")?.ok, true);
  assert.equal(payload.checks.find((check) => check.id === "sandboxed-background-local")?.ok, false);
  assert.equal(
    payload.closeoutChecklist.successCriteria.find((criterion) => criterion.id === "background-native-lifecycle")?.status,
    "open",
  );
  assert.equal(
    payload.closeoutChecklist.successCriteria.find((criterion) => criterion.id === "sandboxed-background-default-promotion")?.status,
    "open",
  );
});

test("plan50 audit keeps CI route explicit even when publish inputs are clean", () => {
  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
    env: {
      ...process.env,
      OPPI_PLAN50_TEST_UNIX_RUNNER_AVAILABLE: "1",
      OPPI_PLAN50_TEST_CI_CHANGES: "",
    },
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(payload.ciEvidenceInputs?.dirty, false);
  assert.ok(
    payload.closeoutChecklist.routes.some((route) =>
      route.id === "multi-os-ci-artifacts"
        && route.requiresExplicitUserApproval === true
        && !route.blockedBy.includes("Relevant Plan 50 workflow/runtime changes must be reviewed, staged, tested, committed, and pushed after GitHub auth is valid.")
        && route.steps.some((step) => step.startsWith("gh workflow run native-shell.yml --ref "))
    ),
    JSON.stringify(payload.closeoutChecklist, null, 2),
  );
});

test("plan50 audit accepts explicit local terminal cleanup evidence", () => {
  const root = tempEvidenceRoot();
  writeLocalTerminalEvidence(root);

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--local-terminal-evidence-root", root, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  const localTerminalCheck = payload.checks.find((check) => check.id === "terminal-restore-local-platform");
  assert.equal(localTerminalCheck?.ok, true);
  assert.match(localTerminalCheck.evidence, /Captured local terminal cleanup evidence passed/);
});

test("plan50 audit accepts local Windows plus Unix terminal cleanup evidence", () => {
  const root = tempEvidenceRoot();
  writeLocalTerminalEvidence(root, "Windows");
  writeLocalTerminalEvidence(root, "Linux");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--local-terminal-evidence-root", root, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  const localTerminalCheck = payload.checks.find((check) => check.id === "terminal-restore-local-platform");
  assert.equal(localTerminalCheck?.ok, true);
  assert.match(localTerminalCheck.evidence, /Windows plus Unix/);
  const unixTerminalCheck = payload.checks.find((check) => check.id === "terminal-restore-unix-local");
  assert.equal(unixTerminalCheck?.ok, true);
  assert.equal(
    payload.closeoutChecklist.successCriteria.find((criterion) => criterion.id === "terminal-restore-windows-unix")?.status,
    "evidence-ready",
  );
  const localRoute = payload.closeoutChecklist.routes.find((route) => route.id === "local-windows-sandbox");
  assert.equal(localRoute.completionScope, "all-remaining-rows");
  assert.deepEqual(localRoute.requiresAdditionalEvidence, []);
  const localBackgroundAction = payload.nextActions.find((action) => action.id === "local-background-lifecycle-evidence");
  assert.equal(localBackgroundAction.completionScope, "all-remaining-rows");
  assert.deepEqual(localBackgroundAction.requiresAdditionalEvidence, []);
  const localApproval = payload.userApproval.options.find((option) => option.routeId === "local-windows-sandbox");
  assert.equal(localApproval.completionScope, "all-remaining-rows");
  assert.deepEqual(localApproval.requiresAdditionalEvidence, []);
});

test("plan50 audit rejects workflow artifact schema drift", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replace('schemaVersion:1', 'schemaVersion:2')
    .replace('tui-dogfood-${RUNNER_OS}.json', 'tui-dogfood.json')
    .replace('if-no-files-found: error', 'if-no-files-found: ignore')
    .replace('cargo test --workspace', 'cargo test -p oppi-shell');
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const evidenceArtifactCheck = payload.checks.find((check) => check.id === "multi-os-ci-evidence-artifacts-defined");
  assert.equal(evidenceArtifactCheck?.ok, false);
  const multiOsDogfoodCheck = payload.checks.find((check) => check.id === "multi-os-ci-dogfood-defined");
  assert.equal(multiOsDogfoodCheck?.ok, false);
});

test("plan50 audit rejects workflow smoke artifact drift", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replaceAll("tui-smoke-${RUNNER_OS}.json", "tui-smoke.json");
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const evidenceArtifactCheck = payload.checks.find((check) => check.id === "multi-os-ci-evidence-artifacts-defined");
  assert.equal(evidenceArtifactCheck?.ok, false);
  assert.match(evidenceArtifactCheck.evidence, /smoke JSON/i);
});

test("plan50 audit rejects smoke artifact target outside smoke step", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replaceAll(
      'tee "plan50-evidence/tui-smoke-${RUNNER_OS}.json"',
      'tee "plan50-evidence/tui-smoke.json"',
    )
    .replace(
      "    timeout-minutes: 20",
      "    timeout-minutes: 20\n    env:\n      PLAN50_UNUSED_SMOKE_ARTIFACT: tui-smoke-${RUNNER_OS}.json",
    );
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const evidenceArtifactCheck = payload.checks.find((check) => check.id === "multi-os-ci-evidence-artifacts-defined");
  assert.equal(evidenceArtifactCheck?.ok, false);
  assert.match(evidenceArtifactCheck.evidence, /smoke JSON/i);
});

test("plan50 audit rejects artifact always-upload drift", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replace("      - name: Write Plan 50 evidence manifest\n        if: always()", "      - name: Write Plan 50 evidence manifest\n        if: success()")
    .replace("      - name: Upload Plan 50 native shell evidence\n        if: always()", "      - name: Upload Plan 50 native shell evidence\n        if: success()");
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const evidenceArtifactCheck = payload.checks.find((check) => check.id === "multi-os-ci-evidence-artifacts-defined");
  assert.equal(evidenceArtifactCheck?.ok, false);
  assert.match(evidenceArtifactCheck.evidence, /always/i);
});

test("plan50 audit rejects evidence producer continue-on-error drift", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replace(
      "      - name: Smoke native shell through CLI\n        shell: bash",
      "      - name: Smoke native shell through CLI\n        continue-on-error: true\n        shell: bash",
    );
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const evidenceArtifactCheck = payload.checks.find((check) => check.id === "multi-os-ci-evidence-artifacts-defined");
  assert.equal(evidenceArtifactCheck?.ok, false);
  assert.match(evidenceArtifactCheck.evidence, /continue-on-error/i);
});

test("plan50 audit rejects duplicate evidence producer continue-on-error overrides", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replace(
      "      - name: Smoke native shell through CLI\n        shell: bash",
      "      - name: Smoke native shell through CLI\n        continue-on-error: false\n        continue-on-error: true\n        shell: bash",
    );
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const evidenceArtifactCheck = payload.checks.find((check) => check.id === "multi-os-ci-evidence-artifacts-defined");
  assert.equal(evidenceArtifactCheck?.ok, false);
  assert.match(evidenceArtifactCheck.evidence, /continue-on-error/i);
});

test("plan50 audit rejects duplicate critical native-shell workflow step names", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replace(
      "      - name: Write Plan 50 evidence manifest\n        if: always()",
      "      - name: Smoke native shell through CLI\n        run: echo duplicate smoke step\n      - name: Write Plan 50 evidence manifest\n        if: always()",
    );
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const multiOsDogfoodCheck = payload.checks.find((check) => check.id === "multi-os-ci-dogfood-defined");
  assert.equal(multiOsDogfoodCheck?.ok, false);
  assert.match(multiOsDogfoodCheck.evidence, /unique.*step/i);
});

test("plan50 audit rejects duplicate verifier workflow step names", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replace(
      "      - name: Require native shell matrix success",
      "      - name: Verify Plan 50 evidence bundle\n        run: echo duplicate verifier step\n      - name: Require native shell matrix success",
    );
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const verifierCheck = payload.checks.find((check) => check.id === "multi-os-ci-evidence-verifier-defined");
  assert.equal(verifierCheck?.ok, false);
  assert.match(verifierCheck.evidence, /unique.*step/i);
});

test("plan50 audit rejects evidence producer pipeline without pipefail", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replace(
      "      - name: Dogfood native shell through CLI\n        shell: bash\n        run: |\n          set -euo pipefail",
      "      - name: Dogfood native shell through CLI\n        shell: bash\n        run: |\n          echo pipefail disabled",
    );
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const evidenceArtifactCheck = payload.checks.find((check) => check.id === "multi-os-ci-evidence-artifacts-defined");
  assert.equal(evidenceArtifactCheck?.ok, false);
  assert.match(evidenceArtifactCheck.evidence, /pipefail/i);
});

test("plan50 audit rejects evidence producer shell drift", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replace(
      "      - name: Smoke native shell through CLI\n        shell: bash\n        run: |",
      "      - name: Smoke native shell through CLI\n        run: |",
    );
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const evidenceArtifactCheck = payload.checks.find((check) => check.id === "multi-os-ci-evidence-artifacts-defined");
  assert.equal(evidenceArtifactCheck?.ok, false);
  assert.match(evidenceArtifactCheck.evidence, /shell: bash/i);
});

test("plan50 audit rejects evidence always guards outside direct step if keys", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replace(
      "      - name: Write Plan 50 evidence manifest\n        if: always()\n        shell: bash\n        env:\n          MATRIX_OS: ${{ matrix.os }}\n        run: |\n          node scripts/plan50-write-evidence-manifest.mjs --evidence-dir plan50-evidence",
      "      - name: Write Plan 50 evidence manifest\n        shell: bash\n        env:\n          MATRIX_OS: ${{ matrix.os }}\n        run: |\n          if: always()\n          node scripts/plan50-write-evidence-manifest.mjs --evidence-dir plan50-evidence",
    )
    .replace(
      "      - name: Upload Plan 50 native shell evidence\n        if: always()\n        uses: actions/upload-artifact@v4\n        with:\n          name: plan50-native-shell-evidence-${{ matrix.os }}\n          path: plan50-evidence/**\n          if-no-files-found: error\n          retention-days: 14",
      "      - name: Upload Plan 50 native shell evidence\n        uses: actions/upload-artifact@v4\n        with:\n          name: plan50-native-shell-evidence-${{ matrix.os }}\n          path: plan50-evidence/**\n          if-no-files-found: error\n          retention-days: 14\n          if: always()",
    );
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const evidenceArtifactCheck = payload.checks.find((check) => check.id === "multi-os-ci-evidence-artifacts-defined");
  assert.equal(evidenceArtifactCheck?.ok, false);
  assert.match(evidenceArtifactCheck.evidence, /always/i);
});

test("plan50 audit rejects missing evidence folder preparation", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replace("      - name: Prepare Plan 50 evidence folder\n        shell: bash\n        run: mkdir -p plan50-evidence\n", "");
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const evidenceArtifactCheck = payload.checks.find((check) => check.id === "multi-os-ci-evidence-artifacts-defined");
  assert.equal(evidenceArtifactCheck?.ok, false);
  assert.match(evidenceArtifactCheck.evidence, /evidence folder/i);
});

test("plan50 audit rejects evidence upload before manifest write", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const writeStep = "      - name: Write Plan 50 evidence manifest\n        if: always()\n        shell: bash\n        env:\n          MATRIX_OS: ${{ matrix.os }}\n        run: |\n          node scripts/plan50-write-evidence-manifest.mjs --evidence-dir plan50-evidence\n";
  const uploadStep = "      - name: Upload Plan 50 native shell evidence\n        if: always()\n        uses: actions/upload-artifact@v4\n        with:\n          name: plan50-native-shell-evidence-${{ matrix.os }}\n          path: plan50-evidence/**\n          if-no-files-found: error\n          retention-days: 14\n";
  const originalWorkflow = readFileSync(realWorkflowPath, "utf8");
  assert.ok(originalWorkflow.includes(`${writeStep}${uploadStep}`), "expected manifest write before upload in workflow fixture");
  const workflow = originalWorkflow.replace(`${writeStep}${uploadStep}`, `${uploadStep}${writeStep}`);
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const evidenceArtifactCheck = payload.checks.find((check) => check.id === "multi-os-ci-evidence-artifacts-defined");
  assert.equal(evidenceArtifactCheck?.ok, false);
  assert.match(evidenceArtifactCheck.evidence, /manifest/i);
});

test("plan50 audit rejects evidence manifest before producer steps", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const writeStep = "      - name: Write Plan 50 evidence manifest\n        if: always()\n        shell: bash\n        env:\n          MATRIX_OS: ${{ matrix.os }}\n        run: |\n          node scripts/plan50-write-evidence-manifest.mjs --evidence-dir plan50-evidence\n";
  const uploadStep = "      - name: Upload Plan 50 native shell evidence\n        if: always()\n        uses: actions/upload-artifact@v4\n        with:\n          name: plan50-native-shell-evidence-${{ matrix.os }}\n          path: plan50-evidence/**\n          if-no-files-found: error\n          retention-days: 14\n";
  const marker = "      - name: Test native Rust workspace\n        run: cargo test --workspace\n";
  const originalWorkflow = readFileSync(realWorkflowPath, "utf8");
  assert.ok(originalWorkflow.includes(`${writeStep}${uploadStep}`), "expected manifest write before upload in workflow fixture");
  assert.ok(originalWorkflow.includes(marker), "expected native Rust test marker in workflow fixture");
  const workflowWithoutArtifactSteps = originalWorkflow.replace(`${writeStep}${uploadStep}`, "");
  const workflow = workflowWithoutArtifactSteps.replace(marker, `${writeStep}${uploadStep}${marker}`);
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const evidenceArtifactCheck = payload.checks.find((check) => check.id === "multi-os-ci-evidence-artifacts-defined");
  assert.equal(evidenceArtifactCheck?.ok, false);
  assert.match(evidenceArtifactCheck.evidence, /manifest/i);
});

test("plan50 audit rejects evidence artifact steps in verifier job", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replace(
      "      - name: Write Plan 50 evidence manifest\n        if: always()",
      "      - name: Write Plan 50 evidence manifest moved out\n        if: success()",
    )
    .replace(
      "      - name: Upload Plan 50 native shell evidence\n        if: always()",
      "      - name: Upload Plan 50 native shell evidence moved out\n        if: success()",
    )
    .replace(
      "      - name: Download Plan 50 native shell evidence artifacts",
      "      - name: Write Plan 50 evidence manifest\n        if: always()\n        run: echo moved\n      - name: Upload Plan 50 native shell evidence\n        if: always()\n        uses: actions/upload-artifact@v4\n      - name: Download Plan 50 native shell evidence artifacts",
    );
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const evidenceArtifactCheck = payload.checks.find((check) => check.id === "multi-os-ci-evidence-artifacts-defined");
  assert.equal(evidenceArtifactCheck?.ok, false);
  assert.match(evidenceArtifactCheck.evidence, /native-shell job/i);
});

test("plan50 audit rejects evidence artifact content outside native-shell steps", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replace(
      "        run: node scripts/plan50-write-evidence-manifest.mjs --evidence-dir plan50-evidence",
      "        run: echo moved",
    )
    .replace("        uses: actions/upload-artifact@v4", "        uses: actions/cache@v4")
    .replace(
      "      - name: Download Plan 50 native shell evidence artifacts",
      "      - name: Moved Plan 50 evidence content\n        run: node scripts/plan50-write-evidence-manifest.mjs --evidence-dir plan50-evidence\n      - uses: actions/upload-artifact@v4\n      - name: Download Plan 50 native shell evidence artifacts",
    );
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const evidenceArtifactCheck = payload.checks.find((check) => check.id === "multi-os-ci-evidence-artifacts-defined");
  assert.equal(evidenceArtifactCheck?.ok, false);
  assert.match(evidenceArtifactCheck.evidence, /native-shell job/i);
});

test("plan50 audit rejects upload artifact settings outside with block", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replace(
      "        with:\n          name: plan50-native-shell-evidence-${{ matrix.os }}\n          path: plan50-evidence/**\n          if-no-files-found: error\n          retention-days: 14",
      "        env:\n          name: plan50-native-shell-evidence-${{ matrix.os }}\n          path: plan50-evidence/**\n          if-no-files-found: error\n          retention-days: 14",
    );
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const evidenceArtifactCheck = payload.checks.find((check) => check.id === "multi-os-ci-evidence-artifacts-defined");
  assert.equal(evidenceArtifactCheck?.ok, false);
  assert.match(evidenceArtifactCheck.evidence, /native-shell job/i);
});

test("plan50 audit rejects duplicate upload artifact with overrides", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replace(
      "        with:\n          name: plan50-native-shell-evidence-${{ matrix.os }}\n          path: plan50-evidence/**\n          if-no-files-found: error\n          retention-days: 14",
      "        with:\n          name: plan50-native-shell-evidence-${{ matrix.os }}\n          path: plan50-evidence/**\n          if-no-files-found: error\n          retention-days: 14\n        with:\n          name: overwritten-evidence\n          path: missing-evidence/**\n          if-no-files-found: ignore\n          retention-days: 1",
    );
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const evidenceArtifactCheck = payload.checks.find((check) => check.id === "multi-os-ci-evidence-artifacts-defined");
  assert.equal(evidenceArtifactCheck?.ok, false);
  assert.match(evidenceArtifactCheck.evidence, /native-shell job/i);
});

test("plan50 audit rejects manifest matrix identity outside env block", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replace(
      "        env:\n          MATRIX_OS: ${{ matrix.os }}\n        run: |\n          node scripts/plan50-write-evidence-manifest.mjs --evidence-dir plan50-evidence",
      "        run: |\n          MATRIX_OS: ${{ matrix.os }}\n          node scripts/plan50-write-evidence-manifest.mjs --evidence-dir plan50-evidence",
    );
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const evidenceArtifactCheck = payload.checks.find((check) => check.id === "multi-os-ci-evidence-artifacts-defined");
  assert.equal(evidenceArtifactCheck?.ok, false);
  assert.match(evidenceArtifactCheck.evidence, /matrix/i);
});

test("plan50 audit rejects manifest writer command outside run body", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replace(
      "        env:\n          MATRIX_OS: ${{ matrix.os }}\n        run: |\n          node scripts/plan50-write-evidence-manifest.mjs --evidence-dir plan50-evidence",
      "        env:\n          MATRIX_OS: ${{ matrix.os }}\n          PLAN50_UNUSED_MANIFEST_COMMAND: |\n            node scripts/plan50-write-evidence-manifest.mjs --evidence-dir plan50-evidence\n        run: |\n          echo manifest skipped",
    );
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const evidenceArtifactCheck = payload.checks.find((check) => check.id === "multi-os-ci-evidence-artifacts-defined");
  assert.equal(evidenceArtifactCheck?.ok, false);
  assert.match(evidenceArtifactCheck.evidence, /manifest/i);
});

test("plan50 audit rejects workflow manifest hash drift", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const manifestWriterPath = join(dir, "plan50-write-evidence-manifest.mjs");
  writeFileSync(workflowPath, readFileSync(realWorkflowPath, "utf8"), "utf8");
  const manifestWriter = readFileSync(realManifestWriterPath, "utf8")
    .replaceAll('createHash("sha256")', 'createHash("sha1")')
    .replaceAll("fileSha256", "fileHashes");
  writeFileSync(manifestWriterPath, manifestWriter, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--manifest-writer-path", manifestWriterPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const evidenceArtifactCheck = payload.checks.find((check) => check.id === "multi-os-ci-evidence-artifacts-defined");
  assert.equal(evidenceArtifactCheck?.ok, false);
  assert.match(evidenceArtifactCheck.evidence, /SHA-256/i);
});

test("plan50 audit rejects workflow manifest run identity drift", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const manifestWriterPath = join(dir, "plan50-write-evidence-manifest.mjs");
  writeFileSync(workflowPath, readFileSync(realWorkflowPath, "utf8"), "utf8");
  const manifestWriter = readFileSync(realManifestWriterPath, "utf8")
    .replaceAll("gitSha", "commitSha")
    .replaceAll("githubRunId", "runId")
    .replaceAll("githubRunAttempt", "runAttempt")
    .replaceAll("githubRefName", "refName");
  writeFileSync(manifestWriterPath, manifestWriter, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--manifest-writer-path", manifestWriterPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const evidenceArtifactCheck = payload.checks.find((check) => check.id === "multi-os-ci-evidence-artifacts-defined");
  assert.equal(evidenceArtifactCheck?.ok, false);
  assert.match(evidenceArtifactCheck.evidence, /run identity/i);
});

test("plan50 audit rejects workflow manifest matrix identity drift", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replace("matrixOs:process.env.MATRIX_OS, ", "")
    .replace("MATRIX_OS: ${{ matrix.os }}", "MATRIX_OS: unknown");
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const evidenceArtifactCheck = payload.checks.find((check) => check.id === "multi-os-ci-evidence-artifacts-defined");
  assert.equal(evidenceArtifactCheck?.ok, false);
  assert.match(evidenceArtifactCheck.evidence, /matrix/i);
});

test("plan50 audit rejects matrix fail-fast drift", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replace("      fail-fast: false", "      fail-fast: true");
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const multiOsDogfoodCheck = payload.checks.find((check) => check.id === "multi-os-ci-dogfood-defined");
  assert.equal(multiOsDogfoodCheck?.ok, false);
  assert.match(multiOsDogfoodCheck.evidence, /fail-fast/i);
});

test("plan50 audit rejects fail-fast false outside native matrix", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replace("      fail-fast: false", "      fail-fast: true")
    .replace("    timeout-minutes: 20", "    timeout-minutes: 20\n    strategy:\n      fail-fast: false");
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const multiOsDogfoodCheck = payload.checks.find((check) => check.id === "multi-os-ci-dogfood-defined");
  assert.equal(multiOsDogfoodCheck?.ok, false);
  assert.match(multiOsDogfoodCheck.evidence, /fail-fast/i);
});

test("plan50 audit rejects fail-fast false outside native strategy", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replace("      fail-fast: false", "      fail-fast: true")
    .replace("    timeout-minutes: 45\n", "    timeout-minutes: 45\n    env:\n      fail-fast: false\n");
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const multiOsDogfoodCheck = payload.checks.find((check) => check.id === "multi-os-ci-dogfood-defined");
  assert.equal(multiOsDogfoodCheck?.ok, false);
  assert.match(multiOsDogfoodCheck.evidence, /fail-fast/i);
});

test("plan50 audit rejects OS labels outside native matrix", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replace("        os: [ubuntu-latest, macos-latest, windows-latest]", "        os: [ubuntu-latest, macos-latest]")
    .replace("    timeout-minutes: 20", "    timeout-minutes: 20\n    env:\n      PLAN50_UNUSED_OS_LABEL: windows-latest");
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });

  assert.equal(result.status, 1, result.stdout + result.stderr);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const multiOsDogfoodCheck = payload.checks.find((check) => check.id === "multi-os-ci-dogfood-defined");
  assert.equal(multiOsDogfoodCheck?.ok, false);
  assert.match(multiOsDogfoodCheck.evidence, /native-shell matrix/i);
});

test("plan50 audit rejects OS labels outside native matrix os list", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replace(
      "        os: [ubuntu-latest, macos-latest, windows-latest]",
      "        os: [ubuntu-latest, macos-latest]\n        PLAN50_UNUSED_OS_LABEL: windows-latest",
    );
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });

  assert.equal(result.status, 1, result.stdout + result.stderr);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const multiOsDogfoodCheck = payload.checks.find((check) => check.id === "multi-os-ci-dogfood-defined");
  assert.equal(multiOsDogfoodCheck?.ok, false);
  assert.match(multiOsDogfoodCheck.evidence, /native-shell matrix/i);
});

test("plan50 audit rejects OS labels outside native strategy matrix", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replace("        os: [ubuntu-latest, macos-latest, windows-latest]", "        os: [ubuntu-latest]")
    .replace(
      "    timeout-minutes: 45\n",
      "    timeout-minutes: 45\n    env:\n      matrix:\n        os: [ubuntu-latest, macos-latest, windows-latest]\n",
    );
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });

  assert.equal(result.status, 1, result.stdout + result.stderr);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const multiOsDogfoodCheck = payload.checks.find((check) => check.id === "multi-os-ci-dogfood-defined");
  assert.equal(multiOsDogfoodCheck?.ok, false);
  assert.match(multiOsDogfoodCheck.evidence, /native-shell matrix/i);
});

test("plan50 audit rejects native-shell runner detached from matrix os", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replace("    runs-on: ${{ matrix.os }}", "    runs-on: ubuntu-latest");
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });

  assert.equal(result.status, 1, result.stdout + result.stderr);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const multiOsDogfoodCheck = payload.checks.find((check) => check.id === "multi-os-ci-dogfood-defined");
  assert.equal(multiOsDogfoodCheck?.ok, false);
  assert.match(multiOsDogfoodCheck.evidence, /runs-on/i);
});

test("plan50 audit rejects workflow dependency setup drift", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replace("corepack enable", "node --version")
    .replace("pnpm install --frozen-lockfile", "pnpm install")
    .replace("pnpm --filter @oppiai/cli build", "pnpm --version");
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const multiOsDogfoodCheck = payload.checks.find((check) => check.id === "multi-os-ci-dogfood-defined");
  assert.equal(multiOsDogfoodCheck?.ok, false);
});

test("plan50 audit rejects Node setup outside native-shell job", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replace("      - uses: actions/setup-node@v4", "      - uses: actions/setup-node@v3");
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });

  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const multiOsDogfoodCheck = payload.checks.find((check) => check.id === "multi-os-ci-dogfood-defined");
  assert.equal(multiOsDogfoodCheck?.ok, false);
  assert.match(multiOsDogfoodCheck.evidence, /native-shell Node setup/i);
});

test("plan50 audit rejects Node version outside setup-node with block", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replace("        with:\n          node-version: 20", "        env:\n          node-version: 20");
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });

  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const multiOsDogfoodCheck = payload.checks.find((check) => check.id === "multi-os-ci-dogfood-defined");
  assert.equal(multiOsDogfoodCheck?.ok, false);
  assert.match(multiOsDogfoodCheck.evidence, /native-shell Node setup/i);
});

test("plan50 audit rejects Node setup after dependency install", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const nodeSetupBlock = "      - uses: actions/setup-node@v4\n        with:\n          node-version: 20\n";
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replace(nodeSetupBlock, "")
    .replace(
      "      - name: Install workspace dependencies\n        run: pnpm install --frozen-lockfile\n",
      "      - name: Install workspace dependencies\n        run: pnpm install --frozen-lockfile\n" + nodeSetupBlock,
    );
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });

  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const multiOsDogfoodCheck = payload.checks.find((check) => check.id === "multi-os-ci-dogfood-defined");
  assert.equal(multiOsDogfoodCheck?.ok, false);
  assert.match(multiOsDogfoodCheck.evidence, /native-shell setup order/i);
});

test("plan50 audit rejects missing Rust toolchain setup", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replace("      - uses: dtolnay/rust-toolchain@stable\n", "");
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });

  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const multiOsDogfoodCheck = payload.checks.find((check) => check.id === "multi-os-ci-dogfood-defined");
  assert.equal(multiOsDogfoodCheck?.ok, false);
  assert.match(multiOsDogfoodCheck.evidence, /Rust toolchain/i);
});

test("plan50 audit rejects Linux sandbox dependency setup drift", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replace(
      "      - name: Install Linux sandbox dependencies\n        if: runner.os == 'Linux'\n        run: sudo apt-get update && sudo apt-get install -y bubblewrap\n",
      "      - name: Install Linux sandbox dependencies\n        if: runner.os == 'Linux'\n        run: echo skipped\n",
    )
    .replace(
      "    timeout-minutes: 20",
      "    timeout-minutes: 20\n    env:\n      PLAN50_UNUSED_LINUX_SANDBOX_DEPS: sudo apt-get update && sudo apt-get install -y bubblewrap",
    );
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });

  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const multiOsDogfoodCheck = payload.checks.find((check) => check.id === "multi-os-ci-dogfood-defined");
  assert.equal(multiOsDogfoodCheck?.ok, false);
  assert.match(multiOsDogfoodCheck.evidence, /Linux sandbox dependencies/i);
});

test("plan50 audit rejects pnpm enable after dependency install", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const pnpmEnableStep = "      - name: Enable pnpm\n        run: corepack enable\n";
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replace(pnpmEnableStep, "")
    .replace(
      "      - name: Install workspace dependencies\n        run: pnpm install --frozen-lockfile\n",
      "      - name: Install workspace dependencies\n        run: pnpm install --frozen-lockfile\n" + pnpmEnableStep,
    );
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });

  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const multiOsDogfoodCheck = payload.checks.find((check) => check.id === "multi-os-ci-dogfood-defined");
  assert.equal(multiOsDogfoodCheck?.ok, false);
  assert.match(multiOsDogfoodCheck.evidence, /native-shell setup order/i);
});

test("plan50 audit rejects Rust toolchain setup after native build", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const rustToolchainStep = "      - uses: dtolnay/rust-toolchain@stable\n";
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replace(rustToolchainStep, "")
    .replace(
      "      - name: Build native shell and server\n        run: cargo build -p oppi-server -p oppi-shell\n",
      "      - name: Build native shell and server\n        run: cargo build -p oppi-server -p oppi-shell\n" + rustToolchainStep,
    );
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });

  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const multiOsDogfoodCheck = payload.checks.find((check) => check.id === "multi-os-ci-dogfood-defined");
  assert.equal(multiOsDogfoodCheck?.ok, false);
  assert.match(multiOsDogfoodCheck.evidence, /Rust toolchain/i);
});

test("plan50 audit rejects Linux sandbox dependencies after host sandbox evidence", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const linuxSandboxDepsStep = "      - name: Install Linux sandbox dependencies\n        if: runner.os == 'Linux'\n        run: sudo apt-get update && sudo apt-get install -y bubblewrap\n";
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replace(linuxSandboxDepsStep, "")
    .replace(
      "          cargo test -p oppi-sandbox host_linux_bubblewrap_blocks_network_when_disabled -- --ignored --nocapture 2>&1 | tee \"plan50-evidence/linux-bubblewrap-host-sandbox-${RUNNER_OS}.log\"\n",
      "          cargo test -p oppi-sandbox host_linux_bubblewrap_blocks_network_when_disabled -- --ignored --nocapture 2>&1 | tee \"plan50-evidence/linux-bubblewrap-host-sandbox-${RUNNER_OS}.log\"\n" + linuxSandboxDepsStep,
    );
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });

  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const multiOsDogfoodCheck = payload.checks.find((check) => check.id === "multi-os-ci-dogfood-defined");
  assert.equal(multiOsDogfoodCheck?.ok, false);
  assert.match(multiOsDogfoodCheck.evidence, /Linux sandbox dependencies/i);
});

test("plan50 audit rejects native-shell test steps in verifier job", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replace(
      "      - name: Test CLI package\n        run: pnpm --filter @oppiai/cli test",
      "      - name: Test CLI package moved out\n        run: node --version",
    )
    .replace(
      "      - name: Download Plan 50 native shell evidence artifacts",
      "      - name: Test CLI package\n        run: pnpm --filter @oppiai/cli test\n      - name: Download Plan 50 native shell evidence artifacts",
    );
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });

  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const multiOsDogfoodCheck = payload.checks.find((check) => check.id === "multi-os-ci-dogfood-defined");
  assert.equal(multiOsDogfoodCheck?.ok, false);
  assert.match(multiOsDogfoodCheck.evidence, /native-shell job/i);
});

test("plan50 audit rejects frozen install command outside dependency install step", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replace("pnpm install --frozen-lockfile", "pnpm install")
    .replace(
      "    timeout-minutes: 20",
      "    timeout-minutes: 20\n    env:\n      PLAN50_UNUSED_INSTALL_COMMAND: pnpm install --frozen-lockfile",
    );
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });

  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const multiOsDogfoodCheck = payload.checks.find((check) => check.id === "multi-os-ci-dogfood-defined");
  assert.equal(multiOsDogfoodCheck?.ok, false);
  assert.match(multiOsDogfoodCheck.evidence, /dependency setup/i);
});

test("plan50 audit rejects corepack command outside pnpm enable step", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replace("corepack enable", "node --version")
    .replace(
      "    timeout-minutes: 20",
      "    timeout-minutes: 20\n    env:\n      PLAN50_UNUSED_COREPACK_COMMAND: corepack enable",
    );
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });

  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const multiOsDogfoodCheck = payload.checks.find((check) => check.id === "multi-os-ci-dogfood-defined");
  assert.equal(multiOsDogfoodCheck?.ok, false);
  assert.match(multiOsDogfoodCheck.evidence, /dependency setup/i);
});

test("plan50 audit rejects missing native binary build step", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replace(
      "      - name: Build native shell and server\n        run: cargo build -p oppi-server -p oppi-shell\n",
      "",
    );
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });

  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const multiOsDogfoodCheck = payload.checks.find((check) => check.id === "multi-os-ci-dogfood-defined");
  assert.equal(multiOsDogfoodCheck?.ok, false);
  assert.match(multiOsDogfoodCheck.evidence, /native shell binary build/i);
});

test("plan50 audit rejects native binary build after evidence producers", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const buildStep = "      - name: Build native shell and server\n        run: cargo build -p oppi-server -p oppi-shell\n";
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replace(buildStep, "")
    .replace(
      "          node packages/cli/dist/main.js tui dogfood --mock --json --require-background-lifecycle | tee \"plan50-evidence/tui-dogfood-strict-${RUNNER_OS}.json\"\n",
      "          node packages/cli/dist/main.js tui dogfood --mock --json --require-background-lifecycle | tee \"plan50-evidence/tui-dogfood-strict-${RUNNER_OS}.json\"\n" + buildStep,
    );
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });

  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const multiOsDogfoodCheck = payload.checks.find((check) => check.id === "multi-os-ci-dogfood-defined");
  assert.equal(multiOsDogfoodCheck?.ok, false);
  assert.match(multiOsDogfoodCheck.evidence, /native shell binary build/i);
});

test("plan50 audit rejects CLI wrapper build after smoke evidence", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const buildStep = "      - name: Build CLI wrapper\n        run: pnpm --filter @oppiai/cli build\n";
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replace(buildStep, "")
    .replace(
      "            node packages/cli/dist/main.js tui smoke --mock --json | tee \"plan50-evidence/tui-smoke-${RUNNER_OS}.json\"\n",
      "            node packages/cli/dist/main.js tui smoke --mock --json | tee \"plan50-evidence/tui-smoke-${RUNNER_OS}.json\"\n" + buildStep,
    );
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });

  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const multiOsDogfoodCheck = payload.checks.find((check) => check.id === "multi-os-ci-dogfood-defined");
  assert.equal(multiOsDogfoodCheck?.ok, false);
  assert.match(multiOsDogfoodCheck.evidence, /CLI wrapper build/i);
});

test("plan50 audit rejects helper tests after native evidence producers", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const helperStep = "      - name: Test Plan 50 audit helpers\n        run: pnpm run plan50:test\n";
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replace(helperStep, "")
    .replace(
      "            node packages/cli/dist/main.js tui dogfood --mock --json | tee \"plan50-evidence/tui-dogfood-${RUNNER_OS}.json\"\n",
      "            node packages/cli/dist/main.js tui dogfood --mock --json | tee \"plan50-evidence/tui-dogfood-${RUNNER_OS}.json\"\n" + helperStep,
    );
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });

  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const multiOsDogfoodCheck = payload.checks.find((check) => check.id === "multi-os-ci-dogfood-defined");
  assert.equal(multiOsDogfoodCheck?.ok, false);
  assert.match(multiOsDogfoodCheck.evidence, /Plan 50 helper tests/i);
});

test("plan50 audit rejects workflow native package test drift", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replace("pnpm --filter @oppiai/native test", "pnpm --filter @oppiai/native build")
    .replace("pnpm --filter @oppiai/natives test", "pnpm --filter @oppiai/natives build");
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const multiOsDogfoodCheck = payload.checks.find((check) => check.id === "multi-os-ci-dogfood-defined");
  assert.equal(multiOsDogfoodCheck?.ok, false);
  assert.match(multiOsDogfoodCheck.evidence, /native npm package tests/i);
});

test("plan50 audit rejects native package test commands outside native package step", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replace("pnpm --filter @oppiai/native test", "pnpm --filter @oppiai/native build")
    .replace("pnpm --filter @oppiai/natives test", "pnpm --filter @oppiai/natives build")
    .replace(
      "    timeout-minutes: 20",
      "    timeout-minutes: 20\n    env:\n      PLAN50_UNUSED_NATIVE_TEST_COMMANDS: pnpm --filter @oppiai/native test pnpm --filter @oppiai/natives test",
    );
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });

  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const multiOsDogfoodCheck = payload.checks.find((check) => check.id === "multi-os-ci-dogfood-defined");
  assert.equal(multiOsDogfoodCheck?.ok, false);
  assert.match(multiOsDogfoodCheck.evidence, /native npm package tests/i);
});

test("plan50 audit rejects native package test commands outside run body", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replace(
      "      - name: Test native npm packages\n        run: |",
      "      - name: Test native npm packages\n        env:\n          PLAN50_UNUSED_NATIVE_PACKAGE_TESTS: |\n            pnpm --filter @oppiai/native test\n            pnpm --filter @oppiai/natives test\n        run: |",
    )
    .replace(
      "          pnpm --filter @oppiai/native test\n          pnpm --filter @oppiai/natives test",
      "          pnpm --filter @oppiai/cli --version",
    );
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });

  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const multiOsDogfoodCheck = payload.checks.find((check) => check.id === "multi-os-ci-dogfood-defined");
  assert.equal(multiOsDogfoodCheck?.ok, false);
  assert.match(multiOsDogfoodCheck.evidence, /native npm package tests/i);
});

test("plan50 audit rejects workflow CLI package test drift", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replace("pnpm --filter @oppiai/cli test", "pnpm --filter @oppiai/cli build");
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const multiOsDogfoodCheck = payload.checks.find((check) => check.id === "multi-os-ci-dogfood-defined");
  assert.equal(multiOsDogfoodCheck?.ok, false);
  assert.match(multiOsDogfoodCheck.evidence, /CLI package tests/i);
});

test("plan50 audit rejects CLI build command outside CLI build step", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replace("pnpm --filter @oppiai/cli build", "pnpm --filter @oppiai/cli test")
    .replace(
      "    timeout-minutes: 20",
      "    timeout-minutes: 20\n    env:\n      PLAN50_UNUSED_CLI_BUILD_COMMAND: pnpm --filter @oppiai/cli build",
    );
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });

  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const multiOsDogfoodCheck = payload.checks.find((check) => check.id === "multi-os-ci-dogfood-defined");
  assert.equal(multiOsDogfoodCheck?.ok, false);
  assert.match(multiOsDogfoodCheck.evidence, /CLI wrapper build/i);
});

test("plan50 audit rejects CLI package test command outside CLI test step", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replace("pnpm --filter @oppiai/cli test", "pnpm --filter @oppiai/cli build")
    .replace(
      "    timeout-minutes: 20",
      "    timeout-minutes: 20\n    env:\n      PLAN50_UNUSED_CLI_TEST_COMMAND: pnpm --filter @oppiai/cli test",
    );
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });

  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const multiOsDogfoodCheck = payload.checks.find((check) => check.id === "multi-os-ci-dogfood-defined");
  assert.equal(multiOsDogfoodCheck?.ok, false);
  assert.match(multiOsDogfoodCheck.evidence, /CLI package tests/i);
});

test("plan50 audit rejects workflow Plan 50 helper test drift", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replace("pnpm run plan50:test", "pnpm run plan50:summary");
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const multiOsDogfoodCheck = payload.checks.find((check) => check.id === "multi-os-ci-dogfood-defined");
  assert.equal(multiOsDogfoodCheck?.ok, false);
  assert.match(multiOsDogfoodCheck.evidence, /Plan 50 helper tests/i);
});

test("plan50 audit rejects Plan 50 helper command outside helper test step", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replace("pnpm run plan50:test", "pnpm run plan50:summary")
    .replace(
      "    timeout-minutes: 20",
      "    timeout-minutes: 20\n    env:\n      PLAN50_UNUSED_HELPER_TEST_COMMAND: pnpm run plan50:test",
    );
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });

  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const multiOsDogfoodCheck = payload.checks.find((check) => check.id === "multi-os-ci-dogfood-defined");
  assert.equal(multiOsDogfoodCheck?.ok, false);
  assert.match(multiOsDogfoodCheck.evidence, /Plan 50 helper tests/i);
});

test("plan50 audit rejects Rust workspace command outside Rust test step", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replace("cargo test --workspace", "cargo test -p oppi-shell")
    .replace(
      "    timeout-minutes: 20",
      "    timeout-minutes: 20\n    env:\n      PLAN50_UNUSED_RUST_WORKSPACE_COMMAND: cargo test --workspace",
    );
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });

  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const multiOsDogfoodCheck = payload.checks.find((check) => check.id === "multi-os-ci-dogfood-defined");
  assert.equal(multiOsDogfoodCheck?.ok, false);
  assert.match(multiOsDogfoodCheck.evidence, /Rust workspace test/i);
});

test("plan50 audit rejects scalar run commands outside direct step run keys", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replace(
      "      - name: Enable pnpm\n        run: corepack enable",
      "      - name: Enable pnpm\n        env:\n          PLAN50_UNUSED_COREPACK_SCALAR: |\n            run: corepack enable\n        run: node --version",
    )
    .replace(
      "      - name: Install workspace dependencies\n        run: pnpm install --frozen-lockfile",
      "      - name: Install workspace dependencies\n        env:\n          PLAN50_UNUSED_INSTALL_SCALAR: |\n            run: pnpm install --frozen-lockfile\n        run: pnpm install",
    )
    .replace(
      "      - name: Test native Rust workspace\n        run: cargo test --workspace",
      "      - name: Test native Rust workspace\n        env:\n          PLAN50_UNUSED_RUST_SCALAR: |\n            run: cargo test --workspace\n        run: cargo test -p oppi-shell",
    )
    .replace(
      "      - name: Build CLI wrapper\n        run: pnpm --filter @oppiai/cli build",
      "      - name: Build CLI wrapper\n        env:\n          PLAN50_UNUSED_CLI_BUILD_SCALAR: |\n            run: pnpm --filter @oppiai/cli build\n        run: pnpm --filter @oppiai/cli test",
    )
    .replace(
      "      - name: Test CLI package\n        run: pnpm --filter @oppiai/cli test",
      "      - name: Test CLI package\n        env:\n          PLAN50_UNUSED_CLI_TEST_SCALAR: |\n            run: pnpm --filter @oppiai/cli test\n        run: pnpm --filter @oppiai/cli build",
    )
    .replace(
      "      - name: Test Plan 50 audit helpers\n        run: pnpm run plan50:test",
      "      - name: Test Plan 50 audit helpers\n        env:\n          PLAN50_UNUSED_HELPER_SCALAR: |\n            run: pnpm run plan50:test\n        run: pnpm run plan50:summary",
    );
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });

  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const multiOsDogfoodCheck = payload.checks.find((check) => check.id === "multi-os-ci-dogfood-defined");
  assert.equal(multiOsDogfoodCheck?.ok, false);
  assert.match(multiOsDogfoodCheck.evidence, /dependency setup|CLI wrapper build|CLI package tests|Plan 50 helper tests|Rust workspace test/i);
});

test("plan50 audit rejects strict background command outside strict Linux step", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replace(
      "node packages/cli/dist/main.js tui dogfood --mock --json --require-background-lifecycle | tee \"plan50-evidence/tui-dogfood-strict-${RUNNER_OS}.json\"",
      "node packages/cli/dist/main.js tui dogfood --mock --json | tee \"plan50-evidence/tui-dogfood-strict-${RUNNER_OS}.json\"",
    )
    .replace(
      "    timeout-minutes: 20",
      "    timeout-minutes: 20\n    env:\n      PLAN50_UNUSED_STRICT_COMMAND: tui dogfood --mock --json --require-background-lifecycle",
    );
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });

  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const multiOsDogfoodCheck = payload.checks.find((check) => check.id === "multi-os-ci-dogfood-defined");
  assert.equal(multiOsDogfoodCheck?.ok, false);
  assert.match(multiOsDogfoodCheck.evidence, /strict Linux background lifecycle dogfood/i);
});

test("plan50 audit rejects strict background artifact target outside strict Linux step", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replace(
      'tee "plan50-evidence/tui-dogfood-strict-${RUNNER_OS}.json"',
      'tee "plan50-evidence/tui-dogfood-strict.json"',
    )
    .replace(
      "    timeout-minutes: 20",
      "    timeout-minutes: 20\n    env:\n      PLAN50_UNUSED_STRICT_ARTIFACT: tui-dogfood-strict-${RUNNER_OS}.json",
    );
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });

  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const multiOsDogfoodCheck = payload.checks.find((check) => check.id === "multi-os-ci-dogfood-defined");
  assert.equal(multiOsDogfoodCheck?.ok, false);
  assert.match(multiOsDogfoodCheck.evidence, /strict Linux background lifecycle dogfood/i);
});

test("plan50 audit rejects Linux host-sandbox command outside host-sandbox step", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replace(
      "cargo test -p oppi-sandbox host_linux_bubblewrap_blocks_network_when_disabled -- --ignored --nocapture 2>&1 | tee \"plan50-evidence/linux-bubblewrap-host-sandbox-${RUNNER_OS}.log\"",
      "cargo test -p oppi-sandbox -- --nocapture 2>&1 | tee \"plan50-evidence/linux-bubblewrap-host-sandbox-${RUNNER_OS}.log\"",
    )
    .replace(
      "    timeout-minutes: 20",
      "    timeout-minutes: 20\n    env:\n      PLAN50_UNUSED_HOST_SANDBOX_COMMAND: host_linux_bubblewrap_blocks_network_when_disabled -- --ignored --nocapture",
    );
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });

  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const multiOsDogfoodCheck = payload.checks.find((check) => check.id === "multi-os-ci-dogfood-defined");
  assert.equal(multiOsDogfoodCheck?.ok, false);
  assert.match(multiOsDogfoodCheck.evidence, /Linux Bubblewrap host-sandbox/i);
});

test("plan50 audit rejects Linux-only evidence conditions outside direct step if keys", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replace(
      "      - name: Check Linux Bubblewrap host sandbox\n        if: runner.os == 'Linux'\n        shell: bash\n        run: |",
      "      - name: Check Linux Bubblewrap host sandbox\n        shell: bash\n        run: |\n          if: runner.os == 'Linux'",
    )
    .replace(
      "      - name: Strict sandboxed background dogfood on Linux\n        if: runner.os == 'Linux'\n        shell: bash\n        run: |",
      "      - name: Strict sandboxed background dogfood on Linux\n        shell: bash\n        run: |\n          if: runner.os == 'Linux'",
    );
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });

  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const multiOsDogfoodCheck = payload.checks.find((check) => check.id === "multi-os-ci-dogfood-defined");
  assert.equal(multiOsDogfoodCheck?.ok, false);
  assert.match(multiOsDogfoodCheck.evidence, /Linux/i);
});

test("plan50 audit rejects Linux evidence commands outside run bodies", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const hostSandboxCommand = 'cargo test -p oppi-sandbox host_linux_bubblewrap_blocks_network_when_disabled -- --ignored --nocapture 2>&1 | tee "plan50-evidence/linux-bubblewrap-host-sandbox-${RUNNER_OS}.log"';
  const strictDogfoodCommand = 'node packages/cli/dist/main.js tui dogfood --mock --json --require-background-lifecycle | tee "plan50-evidence/tui-dogfood-strict-${RUNNER_OS}.json"';
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replace(
      "      - name: Check Linux Bubblewrap host sandbox\n        if: runner.os == 'Linux'\n        shell: bash\n        run: |\n          set -euo pipefail\n          cargo test -p oppi-sandbox host_linux_bubblewrap_blocks_network_when_disabled -- --ignored --nocapture 2>&1 | tee \"plan50-evidence/linux-bubblewrap-host-sandbox-${RUNNER_OS}.log\"",
      `      - name: Check Linux Bubblewrap host sandbox\n        if: runner.os == 'Linux'\n        shell: bash\n        env:\n          PLAN50_UNUSED_HOST_SANDBOX_COMMAND: ${hostSandboxCommand}\n        run: |\n          set -euo pipefail\n          echo skipped`,
    )
    .replace(
      "      - name: Strict sandboxed background dogfood on Linux\n        if: runner.os == 'Linux'\n        shell: bash\n        run: |\n          set -euo pipefail\n          OPPI_SERVER_BIN=\"${PWD}/target/debug/oppi-server\" \\\n          OPPI_SHELL_BIN=\"${PWD}/target/debug/oppi-shell\" \\\n          node packages/cli/dist/main.js tui dogfood --mock --json --require-background-lifecycle | tee \"plan50-evidence/tui-dogfood-strict-${RUNNER_OS}.json\"",
      `      - name: Strict sandboxed background dogfood on Linux\n        if: runner.os == 'Linux'\n        shell: bash\n        env:\n          PLAN50_UNUSED_STRICT_DOGFOOD_COMMAND: ${strictDogfoodCommand}\n        run: |\n          set -euo pipefail\n          echo skipped`,
    );
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });

  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const multiOsDogfoodCheck = payload.checks.find((check) => check.id === "multi-os-ci-dogfood-defined");
  assert.equal(multiOsDogfoodCheck?.ok, false);
  assert.match(multiOsDogfoodCheck.evidence, /strict Linux background lifecycle dogfood|Linux Bubblewrap host-sandbox/i);
});

test("plan50 audit rejects terminal cleanup tests outside cleanup step", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replace(
      "cargo test -p oppi-shell ratatui_lifecycle_exit_paths_share_cleanup_contract -- --nocapture 2>&1 | tee \"plan50-evidence/terminal-cleanup-lifecycle-${RUNNER_OS}.log\"",
      "cargo test -p oppi-shell -- --nocapture 2>&1 | tee \"plan50-evidence/terminal-cleanup-lifecycle-${RUNNER_OS}.log\"",
    )
    .replace(
      "cargo test -p oppi-shell ratatui_terminal_cleanup_sequence_resets_and_clears -- --nocapture 2>&1 | tee \"plan50-evidence/terminal-cleanup-reset-${RUNNER_OS}.log\"",
      "cargo test -p oppi-shell -- --nocapture 2>&1 | tee \"plan50-evidence/terminal-cleanup-reset-${RUNNER_OS}.log\"",
    )
    .replace(
      "    timeout-minutes: 20",
      "    timeout-minutes: 20\n    env:\n      PLAN50_UNUSED_TERMINAL_TESTS: ratatui_lifecycle_exit_paths_share_cleanup_contract ratatui_terminal_cleanup_sequence_resets_and_clears",
    );
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });

  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const multiOsDogfoodCheck = payload.checks.find((check) => check.id === "multi-os-ci-dogfood-defined");
  assert.equal(multiOsDogfoodCheck?.ok, false);
  assert.match(multiOsDogfoodCheck.evidence, /terminal cleanup checks/i);
});

test("plan50 audit rejects terminal cleanup commands outside run body", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const lifecycleCommand = 'cargo test -p oppi-shell ratatui_lifecycle_exit_paths_share_cleanup_contract -- --nocapture 2>&1 | tee "plan50-evidence/terminal-cleanup-lifecycle-${RUNNER_OS}.log"';
  const resetCommand = 'cargo test -p oppi-shell ratatui_terminal_cleanup_sequence_resets_and_clears -- --nocapture 2>&1 | tee "plan50-evidence/terminal-cleanup-reset-${RUNNER_OS}.log"';
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replace(
      "      - name: Check terminal cleanup lifecycle\n        shell: bash\n        run: |\n          set -euo pipefail\n          cargo test -p oppi-shell ratatui_lifecycle_exit_paths_share_cleanup_contract -- --nocapture 2>&1 | tee \"plan50-evidence/terminal-cleanup-lifecycle-${RUNNER_OS}.log\"\n          cargo test -p oppi-shell ratatui_terminal_cleanup_sequence_resets_and_clears -- --nocapture 2>&1 | tee \"plan50-evidence/terminal-cleanup-reset-${RUNNER_OS}.log\"",
      `      - name: Check terminal cleanup lifecycle\n        shell: bash\n        env:\n          PLAN50_UNUSED_TERMINAL_LIFECYCLE_COMMAND: ${lifecycleCommand}\n          PLAN50_UNUSED_TERMINAL_RESET_COMMAND: ${resetCommand}\n        run: |\n          set -euo pipefail\n          echo skipped`,
    );
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });

  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const multiOsDogfoodCheck = payload.checks.find((check) => check.id === "multi-os-ci-dogfood-defined");
  assert.equal(multiOsDogfoodCheck?.ok, false);
  assert.match(multiOsDogfoodCheck.evidence, /terminal cleanup checks/i);
});

test("plan50 audit rejects smoke and dogfood commands outside CLI steps", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replaceAll(
      "node packages/cli/dist/main.js tui smoke --mock --json | tee \"plan50-evidence/tui-smoke-${RUNNER_OS}.json\"",
      "node packages/cli/dist/main.js --version | tee \"plan50-evidence/tui-smoke-${RUNNER_OS}.json\"",
    )
    .replaceAll(
      "node packages/cli/dist/main.js tui dogfood --mock --json | tee \"plan50-evidence/tui-dogfood-${RUNNER_OS}.json\"",
      "node packages/cli/dist/main.js --version | tee \"plan50-evidence/tui-dogfood-${RUNNER_OS}.json\"",
    )
    .replace(
      "    timeout-minutes: 20",
      "    timeout-minutes: 20\n    env:\n      PLAN50_UNUSED_CLI_COMMANDS: tui smoke --mock --json tui dogfood --mock --json",
    );
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });

  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const multiOsDogfoodCheck = payload.checks.find((check) => check.id === "multi-os-ci-dogfood-defined");
  assert.equal(multiOsDogfoodCheck?.ok, false);
  assert.match(multiOsDogfoodCheck.evidence, /smoke\/dogfood CLI steps/i);
});

test("plan50 audit rejects smoke and dogfood commands outside run bodies", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const smokeCommand = 'node packages/cli/dist/main.js tui smoke --mock --json | tee "plan50-evidence/tui-smoke-${RUNNER_OS}.json"';
  const dogfoodCommand = 'node packages/cli/dist/main.js tui dogfood --mock --json | tee "plan50-evidence/tui-dogfood-${RUNNER_OS}.json"';
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replace(
      "      - name: Smoke native shell through CLI\n        shell: bash\n        run: |",
      `      - name: Smoke native shell through CLI\n        shell: bash\n        env:\n          PLAN50_UNUSED_SMOKE_COMMAND: ${smokeCommand}\n        run: |`,
    )
    .replace(
      "            node packages/cli/dist/main.js tui smoke --mock --json | tee \"plan50-evidence/tui-smoke-${RUNNER_OS}.json\"",
      "            node packages/cli/dist/main.js --version",
    )
    .replace(
      "            node packages/cli/dist/main.js tui smoke --mock --json | tee \"plan50-evidence/tui-smoke-${RUNNER_OS}.json\"",
      "            node packages/cli/dist/main.js --version",
    )
    .replace(
      "      - name: Dogfood native shell through CLI\n        shell: bash\n        run: |",
      `      - name: Dogfood native shell through CLI\n        shell: bash\n        env:\n          PLAN50_UNUSED_DOGFOOD_COMMAND: ${dogfoodCommand}\n        run: |`,
    )
    .replace(
      "            node packages/cli/dist/main.js tui dogfood --mock --json | tee \"plan50-evidence/tui-dogfood-${RUNNER_OS}.json\"",
      "            node packages/cli/dist/main.js --version",
    )
    .replace(
      "            node packages/cli/dist/main.js tui dogfood --mock --json | tee \"plan50-evidence/tui-dogfood-${RUNNER_OS}.json\"",
      "            node packages/cli/dist/main.js --version",
    );
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });

  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const multiOsDogfoodCheck = payload.checks.find((check) => check.id === "multi-os-ci-dogfood-defined");
  assert.equal(multiOsDogfoodCheck?.ok, false);
  assert.match(multiOsDogfoodCheck.evidence, /smoke\/dogfood CLI steps/i);
});

test("plan50 audit rejects duplicate evidence producer run overrides", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replace(
      "      - name: Dogfood native shell through CLI\n",
      "        run: echo skipped\n      - name: Dogfood native shell through CLI\n",
    );
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });

  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const multiOsDogfoodCheck = payload.checks.find((check) => check.id === "multi-os-ci-dogfood-defined");
  assert.equal(multiOsDogfoodCheck?.ok, false);
  assert.match(multiOsDogfoodCheck.evidence, /smoke\/dogfood CLI steps/i);
});

test("plan50 audit rejects native shell evidence without built binary targets", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replace(/^\s*OPPI_(SERVER|SHELL)_BIN=.*\\\r?\n/gm, "")
    .replace(
      "    timeout-minutes: 20",
      "    timeout-minutes: 20\n    env:\n      PLAN50_UNUSED_NATIVE_BIN_TARGETS: OPPI_SERVER_BIN=\"${PWD}/target/debug/oppi-server\" OPPI_SHELL_BIN=\"${PWD}/target/debug/oppi-shell\"",
    );
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });

  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const multiOsDogfoodCheck = payload.checks.find((check) => check.id === "multi-os-ci-dogfood-defined");
  assert.equal(multiOsDogfoodCheck?.ok, false);
  assert.match(multiOsDogfoodCheck.evidence, /native shell binary targets/i);
});

test("plan50 audit rejects path filters outside workflow triggers", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replaceAll('      - "packages/native/**"\n', "")
    .replace(
      "    timeout-minutes: 20",
      "    timeout-minutes: 20\n    env:\n      PLAN50_UNUSED_PATH_FILTER: packages/native/**",
    );
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });

  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const multiOsDogfoodCheck = payload.checks.find((check) => check.id === "multi-os-ci-dogfood-defined");
  assert.equal(multiOsDogfoodCheck?.ok, false);
  assert.match(multiOsDogfoodCheck.evidence, /push\/pull_request path filters/i);
});

test("plan50 audit rejects missing embedded prompt asset path filters", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replaceAll('      - "packages/pi-package/skills/graphify/**"\n', "")
    .replaceAll('      - "systemprompts/goals/**"\n', "")
    .replaceAll('      - "systemprompts/main/oppi-feature-routing-system-append.md"\n', "")
    .replaceAll('      - "systemprompts/experiments/promptname_a/oppi-feature-routing-system-append.md"\n', "")
    .replaceAll('      - "systemprompts/experiments/promptname_b/oppi-feature-routing-system-append.md"\n', "")
    .replace(
      "    timeout-minutes: 20",
      "    timeout-minutes: 20\n    env:\n      PLAN50_UNUSED_PATH_FILTER: systemprompts/goals/**\n      PLAN50_UNUSED_GRAPHIFY_FILTER: packages/pi-package/skills/graphify/**\n      PLAN50_UNUSED_ROUTING_FILTER: systemprompts/main/oppi-feature-routing-system-append.md",
    );
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });

  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const multiOsDogfoodCheck = payload.checks.find((check) => check.id === "multi-os-ci-dogfood-defined");
  assert.equal(multiOsDogfoodCheck?.ok, false);
  assert.match(multiOsDogfoodCheck.evidence, /push\/pull_request path filters/i);
});

test("plan50 audit rejects path filter strings outside path list items", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replaceAll('      - "packages/native/**"\n', "")
    .replaceAll(
      "    paths:\n",
      '    paths:\n      PLAN50_UNUSED_PATH_FILTER: "packages/native/**"\n',
    );
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });

  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const multiOsDogfoodCheck = payload.checks.find((check) => check.id === "multi-os-ci-dogfood-defined");
  assert.equal(multiOsDogfoodCheck?.ok, false);
  assert.match(multiOsDogfoodCheck.evidence, /push\/pull_request path filters/i);
});

test("plan50 audit rejects path filters outside the top-level on block", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const triggerPaths = [
    '      - "Cargo.toml"',
    '      - "Cargo.lock"',
    '      - "crates/**"',
    '      - "package.json"',
    '      - "pnpm-lock.yaml"',
    '      - "pnpm-workspace.yaml"',
    '      - "packages/cli/**"',
    '      - "packages/native/**"',
    '      - "packages/natives/**"',
    '      - "packages/pi-package/skills/graphify/**"',
    '      - "systemprompts/goals/**"',
    '      - "systemprompts/main/oppi-feature-routing-system-append.md"',
    '      - "systemprompts/experiments/promptname_a/oppi-feature-routing-system-append.md"',
    '      - "systemprompts/experiments/promptname_b/oppi-feature-routing-system-append.md"',
    '      - "scripts/plan50-*.mjs"',
    '      - ".github/workflows/native-shell.yml"',
  ].join("\n");
  const fakeTriggerPaths = [
    '    - "Cargo.toml"',
    '    - "Cargo.lock"',
    '    - "crates/**"',
    '    - "package.json"',
    '    - "pnpm-lock.yaml"',
    '    - "pnpm-workspace.yaml"',
    '    - "packages/cli/**"',
    '    - "packages/native/**"',
    '    - "packages/natives/**"',
    '    - "packages/pi-package/skills/graphify/**"',
    '    - "systemprompts/goals/**"',
    '    - "systemprompts/main/oppi-feature-routing-system-append.md"',
    '    - "systemprompts/experiments/promptname_a/oppi-feature-routing-system-append.md"',
    '    - "systemprompts/experiments/promptname_b/oppi-feature-routing-system-append.md"',
    '    - "scripts/plan50-*.mjs"',
    '    - ".github/workflows/native-shell.yml"',
  ].join("\n");
  const originalWorkflow = readFileSync(realWorkflowPath, "utf8");
  const workflow = originalWorkflow
    .replace(
      `on:
  push:
    paths:
${triggerPaths}
  pull_request:
    paths:
${triggerPaths}
  workflow_dispatch:

permissions:`,
      `on:
  workflow_dispatch:

push:
  paths:
${fakeTriggerPaths}
pull_request:
  paths:
${fakeTriggerPaths}

permissions:`,
    );
  assert.notEqual(workflow, originalWorkflow, "expected workflow fixture to move trigger path blocks");
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });

  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const multiOsDogfoodCheck = payload.checks.find((check) => check.id === "multi-os-ci-dogfood-defined");
  assert.equal(multiOsDogfoodCheck?.ok, false);
  assert.match(multiOsDogfoodCheck.evidence, /push\/pull_request path filters/i);
});

test("plan50 audit rejects workflow dispatch outside workflow triggers", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replace("  workflow_dispatch:\n\npermissions:", "permissions:")
    .replace(
      "    timeout-minutes: 20",
      "    timeout-minutes: 20\n    env:\n      workflow_dispatch:",
    );
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });

  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const workflowDispatchCheck = payload.checks.find((check) => check.id === "workflow-dispatch-defined");
  assert.equal(workflowDispatchCheck?.ok, false);
  assert.match(workflowDispatchCheck.evidence, /workflow_dispatch/i);
});

test("plan50 audit rejects workflow dispatch outside the top-level on block", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replace("  workflow_dispatch:\n\npermissions:", "permissions:")
    .replace(
      "name: Native shell validation\n\non:",
      "name: Native shell validation\n\nenv:\n  on:\n    workflow_dispatch:\n\non:",
    );
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });

  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const workflowDispatchCheck = payload.checks.find((check) => check.id === "workflow-dispatch-defined");
  assert.equal(workflowDispatchCheck?.ok, false);
  assert.match(workflowDispatchCheck.evidence, /workflow_dispatch/i);
});

test("plan50 audit rejects duplicate native-shell workflow job ids", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const workflow = `${readFileSync(realWorkflowPath, "utf8")}
  native-shell:
    name: duplicate native shell job
    runs-on: ubuntu-latest
    steps:
      - run: echo duplicate native-shell
`;
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });

  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const multiOsDogfoodCheck = payload.checks.find((check) => check.id === "multi-os-ci-dogfood-defined");
  assert.equal(multiOsDogfoodCheck?.ok, false);
  assert.match(multiOsDogfoodCheck.evidence, /unique.*job/i);
});

test("plan50 audit rejects duplicate plan50-evidence workflow job ids", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const workflow = `${readFileSync(realWorkflowPath, "utf8")}
  plan50-evidence:
    name: duplicate verifier job
    runs-on: ubuntu-latest
    steps:
      - run: echo duplicate verifier
`;
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });

  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const verifierCheck = payload.checks.find((check) => check.id === "multi-os-ci-evidence-verifier-defined");
  assert.equal(verifierCheck?.ok, false);
  assert.match(verifierCheck.evidence, /unique.*job/i);
});

test("plan50 audit rejects workflow safety-control drift", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replace("permissions:\n  contents: read\n\n", "")
    .replace("    timeout-minutes: 45\n", "")
    .replace("    timeout-minutes: 20\n", "");
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const safetyCheck = payload.checks.find((check) => check.id === "workflow-ci-safety-controls-defined");
  assert.equal(safetyCheck?.ok, false);
  assert.match(safetyCheck.evidence, /least-privilege token permissions/i);
  assert.match(safetyCheck.evidence, /timeout/i);
});

test("plan50 audit rejects workflow permissions moved into a job", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replace("permissions:\n  contents: read\n\n", "")
    .replace("  plan50-evidence:\n", "  plan50-evidence:\n    permissions:\n      contents: read\n");
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const safetyCheck = payload.checks.find((check) => check.id === "workflow-ci-safety-controls-defined");
  assert.equal(safetyCheck?.ok, false);
  assert.match(safetyCheck.evidence, /least-privilege token permissions/i);
});

test("plan50 audit rejects extra top-level workflow permissions", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replace(
      "permissions:\n  contents: read\n\n",
      "permissions:\n  contents: read\n  actions: write\n\n",
    );
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const safetyCheck = payload.checks.find((check) => check.id === "workflow-ci-safety-controls-defined");
  assert.equal(safetyCheck?.ok, false);
  assert.match(safetyCheck.evidence, /least-privilege token permissions/i);
});

test("plan50 audit rejects job-level workflow permissions overrides", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replace("  native-shell:\n", "  native-shell:\n    permissions:\n      actions: write\n");
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const safetyCheck = payload.checks.find((check) => check.id === "workflow-ci-safety-controls-defined");
  assert.equal(safetyCheck?.ok, false);
  assert.match(safetyCheck.evidence, /least-privilege token permissions/i);
});

test("plan50 audit rejects permissions overrides in unrelated workflow jobs", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const workflow = `${readFileSync(realWorkflowPath, "utf8")}
  unrelated-token-job:
    runs-on: ubuntu-latest
    permissions:
      contents: write
    steps:
      - run: echo "not Plan 50 evidence"
`;
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const safetyCheck = payload.checks.find((check) => check.id === "workflow-ci-safety-controls-defined");
  assert.equal(safetyCheck?.ok, false);
  assert.match(safetyCheck.evidence, /least-privilege token permissions/i);
});

test("plan50 audit rejects workflow timeouts moved into env blocks", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replace("    timeout-minutes: 45\n", "    env:\n      timeout-minutes: 45\n")
    .replace("    timeout-minutes: 20\n", "    env:\n      timeout-minutes: 20\n");
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const safetyCheck = payload.checks.find((check) => check.id === "workflow-ci-safety-controls-defined");
  assert.equal(safetyCheck?.ok, false);
  assert.match(safetyCheck.evidence, /timeout/i);
});

test("plan50 audit rejects duplicate workflow timeout overrides", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replace("    timeout-minutes: 45\n", "    timeout-minutes: 45\n    timeout-minutes: 999\n");
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const safetyCheck = payload.checks.find((check) => check.id === "workflow-ci-safety-controls-defined");
  assert.equal(safetyCheck?.ok, false);
  assert.match(safetyCheck.evidence, /timeout/i);
});

test("plan50 audit rejects checkout credential persistence drift", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replaceAll("          persist-credentials: false\n", "");
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const safetyCheck = payload.checks.find((check) => check.id === "workflow-ci-safety-controls-defined");
  assert.equal(safetyCheck?.ok, false);
  assert.match(safetyCheck.evidence, /checkout credentials/i);
});

test("plan50 audit rejects checkout credential persistence outside with blocks", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replaceAll("        with:\n          persist-credentials: false\n", "        env:\n          persist-credentials: false\n");
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const safetyCheck = payload.checks.find((check) => check.id === "workflow-ci-safety-controls-defined");
  assert.equal(safetyCheck?.ok, false);
  assert.match(safetyCheck.evidence, /checkout credentials/i);
});

test("plan50 audit rejects unpaired checkout credential persistence", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const checkoutBlock = "      - uses: actions/checkout@v4\n        with:\n          persist-credentials: false\n";
  const duplicatedFirstCheckout = readFileSync(realWorkflowPath, "utf8")
    .replace(checkoutBlock, "      - uses: actions/checkout@v4\n        with:\n          persist-credentials: false\n          persist-credentials: false\n");
  const secondCheckoutIndex = duplicatedFirstCheckout.lastIndexOf(checkoutBlock);
  assert.ok(secondCheckoutIndex > 0, "expected a second checkout block in the workflow fixture");
  const workflow = `${duplicatedFirstCheckout.slice(0, secondCheckoutIndex)}      - uses: actions/checkout@v4\n${duplicatedFirstCheckout.slice(secondCheckoutIndex + checkoutBlock.length)}`;
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const safetyCheck = payload.checks.find((check) => check.id === "workflow-ci-safety-controls-defined");
  assert.equal(safetyCheck?.ok, false);
  assert.match(safetyCheck.evidence, /checkout credentials/i);
});

test("plan50 audit rejects verifier job without checkout", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const checkoutBlock = "      - uses: actions/checkout@v4\n        with:\n          persist-credentials: false\n";
  const workflowWithBothCheckouts = readFileSync(realWorkflowPath, "utf8");
  const verifierCheckoutIndex = workflowWithBothCheckouts.lastIndexOf(checkoutBlock);
  assert.ok(verifierCheckoutIndex > 0, "expected a verifier checkout block in the workflow fixture");
  const workflow = `${workflowWithBothCheckouts.slice(0, verifierCheckoutIndex)}${workflowWithBothCheckouts.slice(verifierCheckoutIndex + checkoutBlock.length)}`;
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const safetyCheck = payload.checks.find((check) => check.id === "workflow-ci-safety-controls-defined");
  assert.equal(safetyCheck?.ok, false);
  assert.match(safetyCheck.evidence, /checkout credentials/i);
});

test("plan50 audit rejects verifier job without Node setup", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const nodeSetupBlock = "      - uses: actions/setup-node@v4\n        with:\n          node-version: 20\n";
  const workflowWithBothNodeSetups = readFileSync(realWorkflowPath, "utf8");
  const verifierNodeSetupIndex = workflowWithBothNodeSetups.lastIndexOf(nodeSetupBlock);
  assert.ok(verifierNodeSetupIndex > 0, "expected a verifier setup-node block in the workflow fixture");
  const workflow = `${workflowWithBothNodeSetups.slice(0, verifierNodeSetupIndex)}${workflowWithBothNodeSetups.slice(verifierNodeSetupIndex + nodeSetupBlock.length)}`;
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const verifierCheck = payload.checks.find((check) => check.id === "multi-os-ci-evidence-verifier-defined");
  assert.equal(verifierCheck?.ok, false);
  assert.match(verifierCheck.evidence, /plan50-evidence job/i);
});

test("plan50 audit rejects verifier steps in unsafe order", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const downloadStep = "      - name: Download Plan 50 native shell evidence artifacts\n        uses: actions/download-artifact@v4\n        with:\n          pattern: plan50-native-shell-evidence-*\n          path: plan50-downloaded-evidence\n          merge-multiple: false\n";
  const verifyStep = "      - name: Verify Plan 50 evidence bundle\n        run: node scripts/plan50-evidence-verify.mjs plan50-downloaded-evidence --json\n";
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replace(`${downloadStep}${verifyStep}`, `${verifyStep}${downloadStep}`);
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const verifierCheck = payload.checks.find((check) => check.id === "multi-os-ci-evidence-verifier-defined");
  assert.equal(verifierCheck?.ok, false);
  assert.match(verifierCheck.evidence, /plan50-evidence job/i);
});

test("plan50 audit rejects verifier job controls outside direct job keys", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replace(
      "  plan50-evidence:\n    name: Plan 50 evidence bundle verifier\n    runs-on: ubuntu-latest\n    needs: native-shell\n    if: always()\n    timeout-minutes: 20",
      "  plan50-evidence:\n    name: Plan 50 evidence verifier spoof holder\n    runs-on: ubuntu-latest\n    env:\n      PLAN50_UNUSED_JOB_CONTROLS: |\n        name: Plan 50 evidence bundle verifier\n        needs: native-shell\n        if: always()\n    needs: []\n    if: ${{ failure() }}\n    timeout-minutes: 20",
    );
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const verifierCheck = payload.checks.find((check) => check.id === "multi-os-ci-evidence-verifier-defined");
  assert.equal(verifierCheck?.ok, false);
  assert.match(verifierCheck.evidence, /plan50-evidence job/i);
});

test("plan50 audit rejects verifier runner drift", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replace(
      "  plan50-evidence:\n    name: Plan 50 evidence bundle verifier\n    runs-on: ubuntu-latest",
      "  plan50-evidence:\n    name: Plan 50 evidence bundle verifier\n    runs-on: windows-latest\n    env:\n      PLAN50_UNUSED_VERIFIER_RUNNER: ubuntu-latest",
    );
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const verifierCheck = payload.checks.find((check) => check.id === "multi-os-ci-evidence-verifier-defined");
  assert.equal(verifierCheck?.ok, false);
  assert.match(verifierCheck.evidence, /ubuntu-latest/i);
});

test("plan50 audit rejects verifier job matrix-result drift", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replace("      - name: Require native shell matrix success\n        if: ${{ always() && needs.native-shell.result != 'success' }}\n        run: |\n          echo \"native-shell matrix result was ${{ needs.native-shell.result }}\"\n          exit 1\n", "");
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const verifierCheck = payload.checks.find((check) => check.id === "multi-os-ci-evidence-verifier-defined");
  assert.equal(verifierCheck?.ok, false);
  assert.match(verifierCheck.evidence, /native-shell matrix result/i);
});

test("plan50 audit rejects verifier continue-on-error drift", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replace(
      "      - name: Verify Plan 50 evidence bundle\n        run: node scripts/plan50-evidence-verify.mjs plan50-downloaded-evidence --json",
      "      - name: Verify Plan 50 evidence bundle\n        continue-on-error: true\n        run: node scripts/plan50-evidence-verify.mjs plan50-downloaded-evidence --json",
    );
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const verifierCheck = payload.checks.find((check) => check.id === "multi-os-ci-evidence-verifier-defined");
  assert.equal(verifierCheck?.ok, false);
  assert.match(verifierCheck.evidence, /continue-on-error/i);
});

test("plan50 audit rejects verifier artifact download layout drift", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replace("          path: plan50-downloaded-evidence\n          merge-multiple: false", "          path: plan50-downloaded-evidence\n          merge-multiple: true");
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const verifierCheck = payload.checks.find((check) => check.id === "multi-os-ci-evidence-verifier-defined");
  assert.equal(verifierCheck?.ok, false);
  assert.match(verifierCheck.evidence, /separate runner artifact folders/i);
});

test("plan50 audit rejects download artifact settings outside with block", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replace(
      "        with:\n          pattern: plan50-native-shell-evidence-*\n          path: plan50-downloaded-evidence\n          merge-multiple: false",
      "        env:\n          pattern: plan50-native-shell-evidence-*\n          path: plan50-downloaded-evidence\n          merge-multiple: false",
    );
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const verifierCheck = payload.checks.find((check) => check.id === "multi-os-ci-evidence-verifier-defined");
  assert.equal(verifierCheck?.ok, false);
  assert.match(verifierCheck.evidence, /separate runner artifact folders/i);
});

test("plan50 audit rejects verifier command and matrix guard outside direct step keys", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replace(
      "      - name: Verify Plan 50 evidence bundle\n        run: node scripts/plan50-evidence-verify.mjs plan50-downloaded-evidence --json",
      "      - name: Verify Plan 50 evidence bundle\n        env:\n          PLAN50_UNUSED_VERIFY_COMMAND: |\n            run: node scripts/plan50-evidence-verify.mjs plan50-downloaded-evidence --json\n        run: node --version",
    )
    .replace(
      "      - name: Require native shell matrix success\n        if: ${{ always() && needs.native-shell.result != 'success' }}\n        run: |\n          echo \"native-shell matrix result was ${{ needs.native-shell.result }}\"\n          exit 1",
      "      - name: Require native shell matrix success\n        env:\n          PLAN50_UNUSED_MATRIX_GUARD: |\n            if: ${{ always() && needs.native-shell.result != 'success' }}\n            echo \"native-shell matrix result was ${{ needs.native-shell.result }}\"\n            exit 1\n        if: always()\n        run: |\n          echo skipped",
    );
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const verifierCheck = payload.checks.find((check) => check.id === "multi-os-ci-evidence-verifier-defined");
  assert.equal(verifierCheck?.ok, false);
  assert.match(verifierCheck.evidence, /native-shell matrix result/i);
});

test("plan50 audit rejects verifier steps outside plan50-evidence job", () => {
  const dir = mkdtempSync(join(tmpdir(), "oppi-plan50-workflow-"));
  const workflowPath = join(dir, "native-shell.yml");
  const verifierSteps = "      - name: Download Plan 50 native shell evidence artifacts\n        uses: actions/download-artifact@v4\n        with:\n          pattern: plan50-native-shell-evidence-*\n          path: plan50-downloaded-evidence\n          merge-multiple: false\n      - name: Verify Plan 50 evidence bundle\n        run: node scripts/plan50-evidence-verify.mjs plan50-downloaded-evidence --json\n      - name: Require native shell matrix success\n        if: ${{ always() && needs.native-shell.result != 'success' }}\n        run: |\n          echo \"native-shell matrix result was ${{ needs.native-shell.result }}\"\n          exit 1";
  const workflow = readFileSync(realWorkflowPath, "utf8")
    .replace(
      "      - name: Download Plan 50 native shell evidence artifacts\n        uses: actions/download-artifact@v4\n        with:\n          pattern: plan50-native-shell-evidence-*\n          path: plan50-downloaded-evidence\n          merge-multiple: false",
      "      - name: Download Plan 50 native shell evidence artifacts moved out\n        run: echo moved",
    )
    .replace(
      "      - name: Verify Plan 50 evidence bundle\n        run: node scripts/plan50-evidence-verify.mjs plan50-downloaded-evidence --json",
      "      - name: Verify Plan 50 evidence bundle moved out\n        run: echo moved",
    )
    .replace(
      "      - name: Require native shell matrix success\n        if: ${{ always() && needs.native-shell.result != 'success' }}\n        run: |\n          echo \"native-shell matrix result was ${{ needs.native-shell.result }}\"\n          exit 1",
      "      - name: Require native shell matrix success moved out\n        run: echo moved",
    )
    .replace(
      "      - name: Write Plan 50 evidence manifest",
      `${verifierSteps}\n      - name: Write Plan 50 evidence manifest`,
    );
  writeFileSync(workflowPath, workflow, "utf8");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--workflow-path", workflowPath, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(resolve(payload.workflowPath), workflowPath);
  const verifierCheck = payload.checks.find((check) => check.id === "multi-os-ci-evidence-verifier-defined");
  assert.equal(verifierCheck?.ok, false);
  assert.match(verifierCheck.evidence, /plan50-evidence job/i);
});

test("plan50 audit folds in verified external evidence bundle without hiding unchecked rows", () => {
  const root = tempEvidenceRoot();
  writeRunnerEvidence(root, "Windows");
  writeRunnerEvidence(root, "Linux", { strict: true });
  writeRunnerEvidence(root, "macOS");

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--evidence-root", root, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  const evidenceCheck = payload.checks.find((check) => check.id === "downloaded-ci-evidence-bundle");
  assert.equal(evidenceCheck?.ok, true);
  assert.deepEqual(
    payload.closeoutChecklist.successCriteria.map((criterion) => criterion.status),
    ["evidence-ready", "evidence-ready", "evidence-ready"],
  );
  assert.deepEqual(payload.evidenceBundle.planRowsReadyToCheck, [
    "/background list/read/kill is dogfooded through native shell",
    "Terminal restore after abort/panic is checked on Windows and Unix",
    "Sandboxed background tasks instead of full-access/unrestricted background shell tasks",
  ]);
  assert.ok(payload.unchecked.length > 0, "audit should still expose unchecked plan rows until the plan is updated");
});

test("plan50 audit applies verified evidence only to matching plan rows", () => {
  const root = tempEvidenceRoot();
  writeRunnerEvidence(root, "Windows");
  writeRunnerEvidence(root, "Linux", { strict: true });
  writeRunnerEvidence(root, "macOS");
  const planDir = mkdtempSync(join(tmpdir(), "oppi-plan50-plan-"));
  const planPath = join(planDir, "50-standalone-oppi-finish-line.md");
  writeFileSync(planPath, [
    "# Plan 50 test",
    "- [ ] `/background` list/read/kill is dogfooded through native shell.",
    "- [ ] Terminal restore after abort/panic is checked on Windows and Unix.",
    "- [ ] Sandboxed background tasks instead of full-access/unrestricted background shell tasks; default promotion requires this.",
    "- [x] Something already done.",
    "",
  ].join("\n"), "utf8");

  const result = spawnSync(process.execPath, [
    "scripts/plan50-audit.mjs",
    "--plan-path",
    planPath,
    "--evidence-root",
    root,
    "--apply-evidence",
    "--json",
  ], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 0, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(payload.ok, true);
  assert.equal(payload.checks.find((check) => check.id === "sandboxed-background-local")?.ok, true);
  assert.equal(payload.checks.find((check) => check.id === "terminal-restore-unix-local")?.ok, true);
  assert.equal(payload.checks.find((check) => check.id === "plan50-all-rows-checked")?.ok, true);
  assert.deepEqual(payload.appliedPlanRows, [
    "/background list/read/kill is dogfooded through native shell",
    "Terminal restore after abort/panic is checked on Windows and Unix",
    "Sandboxed background tasks instead of full-access/unrestricted background shell tasks",
  ]);
  assert.deepEqual(payload.planRowsReadyToApplyDetails, [
    { planLine: 2, planRow: "/background list/read/kill is dogfooded through native shell" },
    { planLine: 3, planRow: "Terminal restore after abort/panic is checked on Windows and Unix" },
    { planLine: 4, planRow: "Sandboxed background tasks instead of full-access/unrestricted background shell tasks" },
  ]);
  assert.deepEqual(payload.appliedPlanRowDetails, [
    { planLine: 2, planRow: "/background list/read/kill is dogfooded through native shell" },
    { planLine: 3, planRow: "Terminal restore after abort/panic is checked on Windows and Unix" },
    { planLine: 4, planRow: "Sandboxed background tasks instead of full-access/unrestricted background shell tasks" },
  ]);
  const updated = readFileSync(planPath, "utf8");
  assert.match(updated, /- \[x\] `\/background` list\/read\/kill is dogfooded through native shell\./);
  assert.match(updated, /- \[x\] Terminal restore after abort\/panic is checked on Windows and Unix\./);
  assert.match(updated, /- \[x\] Sandboxed background tasks instead of full-access\/unrestricted background shell tasks; default promotion requires this\./);
});

test("plan50 audit reports accepted evidence rows that are absent from the plan", () => {
  const root = tempEvidenceRoot();
  writeRunnerEvidence(root, "Windows");
  writeRunnerEvidence(root, "Linux", { strict: true });
  writeRunnerEvidence(root, "macOS");
  const planDir = mkdtempSync(join(tmpdir(), "oppi-plan50-missing-row-plan-"));
  const planPath = join(planDir, "50-standalone-oppi-finish-line.md");
  writeFileSync(planPath, [
    "# Plan 50 missing-row test",
    "- [ ] `/background` list/read/kill is dogfooded through native shell.",
    "- [ ] Sandboxed background tasks instead of full-access/unrestricted background shell tasks; default promotion requires this.",
    "",
  ].join("\n"), "utf8");

  const result = spawnSync(process.execPath, [
    "scripts/plan50-audit.mjs",
    "--plan-path",
    planPath,
    "--evidence-root",
    root,
    "--apply-evidence",
    "--json",
  ], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.equal(
    payload.checks.find((check) => check.id === "plan50-required-rows-present")?.ok,
    false,
  );
  assert.equal(
    payload.closeoutChecklist.successCriteria.find((criterion) => criterion.id === "terminal-restore-windows-unix")?.status,
    "missing-from-plan",
  );
  assert.deepEqual(payload.unappliedPlanRowDetails, [
    { planLine: null, planRow: "Terminal restore after abort/panic is checked on Windows and Unix" },
  ]);
  assert.equal(payload.userApproval.required, false);
  assert.equal(payload.userApproval.blocked, true);
  assert.ok(
    payload.userApproval.blockedBy?.some((blocker) => /Required Plan 50 rows are missing/i.test(blocker)),
    JSON.stringify(payload.userApproval, null, 2),
  );
  assert.deepEqual(payload.appliedPlanRows, [
    "/background list/read/kill is dogfooded through native shell",
    "Sandboxed background tasks instead of full-access/unrestricted background shell tasks",
  ]);

  const summaryPlanPath = join(planDir, "50-standalone-oppi-finish-line-summary.md");
  writeFileSync(summaryPlanPath, [
    "# Plan 50 missing-row summary test",
    "- [ ] `/background` list/read/kill is dogfooded through native shell.",
    "- [ ] Sandboxed background tasks instead of full-access/unrestricted background shell tasks; default promotion requires this.",
    "",
  ].join("\n"), "utf8");
  const summary = spawnSync(process.execPath, [
    "scripts/plan50-audit.mjs",
    "--plan-path",
    summaryPlanPath,
    "--evidence-root",
    root,
    "--apply-evidence",
    "--summary",
  ], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(summary.status, 1, summary.stderr || summary.stdout);
  assert.match(summary.stdout, /Unapplied accepted evidence rows:/);
  assert.match(summary.stdout, /missing: Terminal restore after abort\/panic is checked on Windows and Unix/);
});

test("plan50 audit applies verified local evidence only to rows it proves", () => {
  const root = tempEvidenceRoot();
  const backgroundPath = writeLocalBackgroundEvidence(root);
  writeLocalTerminalEvidence(root, "Windows");
  writeLocalTerminalEvidence(root, "Linux");
  const planDir = mkdtempSync(join(tmpdir(), "oppi-plan50-local-plan-"));
  const planPath = join(planDir, "50-standalone-oppi-finish-line.md");
  writeFileSync(planPath, [
    "# Plan 50 local test",
    "- [ ] `/background` list/read/kill is dogfooded through native shell.",
    "- [ ] Terminal restore after abort/panic is checked on Windows and Unix.",
    "- [ ] Sandboxed background tasks instead of full-access/unrestricted background shell tasks; default promotion requires this.",
    "",
  ].join("\n"), "utf8");

  const result = spawnSync(process.execPath, [
    "scripts/plan50-audit.mjs",
    "--plan-path",
    planPath,
    "--local-background-evidence",
    backgroundPath,
    "--local-terminal-evidence-root",
    root,
    "--apply-evidence",
    "--json",
  ], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 0, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.deepEqual(payload.localPlanRowsReadyToCheck, [
    "/background list/read/kill is dogfooded through native shell",
    "Sandboxed background tasks instead of full-access/unrestricted background shell tasks",
    "Terminal restore after abort/panic is checked on Windows and Unix",
  ]);
  assert.deepEqual(payload.localPlanRowsReadyToCheckDetails, [
    { planLine: 2, planRow: "/background list/read/kill is dogfooded through native shell" },
    { planLine: 4, planRow: "Sandboxed background tasks instead of full-access/unrestricted background shell tasks" },
    { planLine: 3, planRow: "Terminal restore after abort/panic is checked on Windows and Unix" },
  ]);
  assert.deepEqual(payload.appliedPlanRows, [
    "/background list/read/kill is dogfooded through native shell",
    "Terminal restore after abort/panic is checked on Windows and Unix",
    "Sandboxed background tasks instead of full-access/unrestricted background shell tasks",
  ]);
  assert.deepEqual(payload.appliedPlanRowDetails, [
    { planLine: 2, planRow: "/background list/read/kill is dogfooded through native shell" },
    { planLine: 3, planRow: "Terminal restore after abort/panic is checked on Windows and Unix" },
    { planLine: 4, planRow: "Sandboxed background tasks instead of full-access/unrestricted background shell tasks" },
  ]);
  const updated = readFileSync(planPath, "utf8");
  assert.match(updated, /- \[x\] `\/background` list\/read\/kill is dogfooded through native shell\./);
  assert.match(updated, /- \[x\] Terminal restore after abort\/panic is checked on Windows and Unix\./);
  assert.match(updated, /- \[x\] Sandboxed background tasks instead of full-access\/unrestricted background shell tasks; default promotion requires this\./);
});

test("plan50 audit apply mode with local partial evidence leaves unproved rows open", () => {
  const root = tempEvidenceRoot();
  const backgroundPath = writeLocalBackgroundEvidence(root);
  const planDir = mkdtempSync(join(tmpdir(), "oppi-plan50-local-partial-plan-"));
  const planPath = join(planDir, "50-standalone-oppi-finish-line.md");
  writeFileSync(planPath, [
    "# Plan 50 local partial test",
    "- [ ] `/background` list/read/kill is dogfooded through native shell.",
    "- [ ] Terminal restore after abort/panic is checked on Windows and Unix.",
    "- [ ] Sandboxed background tasks instead of full-access/unrestricted background shell tasks; default promotion requires this.",
    "",
  ].join("\n"), "utf8");

  const result = spawnSync(process.execPath, [
    "scripts/plan50-audit.mjs",
    "--plan-path",
    planPath,
    "--local-background-evidence",
    backgroundPath,
    "--apply-evidence",
    "--json",
  ], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.deepEqual(payload.appliedPlanRows, [
    "/background list/read/kill is dogfooded through native shell",
    "Sandboxed background tasks instead of full-access/unrestricted background shell tasks",
  ]);
  const updated = readFileSync(planPath, "utf8");
  assert.match(updated, /- \[x\] `\/background` list\/read\/kill is dogfooded through native shell\./);
  assert.match(updated, /- \[ \] Terminal restore after abort\/panic is checked on Windows and Unix\./);
  assert.match(updated, /- \[x\] Sandboxed background tasks instead of full-access\/unrestricted background shell tasks; default promotion requires this\./);
});

test("plan50 audit apply mode ignores auto-detected default local evidence", () => {
  const defaultRoot = tempEvidenceRoot();
  const defaultBackgroundDir = join(defaultRoot, "plan50-background-evidence");
  mkdirSync(defaultBackgroundDir, { recursive: true });
  writeLocalBackgroundEvidence(defaultBackgroundDir, {});
  writeFileSync(
    join(defaultBackgroundDir, `tui-dogfood-strict-${localRunnerOs()}.json`),
    readFileSync(join(defaultBackgroundDir, "tui-dogfood-strict-local.json"), "utf8"),
    "utf8",
  );

  const explicitTerminalRoot = tempEvidenceRoot();
  writeLocalTerminalEvidence(explicitTerminalRoot, "Windows");
  writeLocalTerminalEvidence(explicitTerminalRoot, "Linux");

  const planDir = mkdtempSync(join(tmpdir(), "oppi-plan50-explicit-local-plan-"));
  const planPath = join(planDir, "50-standalone-oppi-finish-line.md");
  writeFileSync(planPath, [
    "# Plan 50 explicit local test",
    "- [ ] `/background` list/read/kill is dogfooded through native shell.",
    "- [ ] Terminal restore after abort/panic is checked on Windows and Unix.",
    "- [ ] Sandboxed background tasks instead of full-access/unrestricted background shell tasks; default promotion requires this.",
    "",
  ].join("\n"), "utf8");

  const result = spawnSync(process.execPath, [
    "scripts/plan50-audit.mjs",
    "--plan-path",
    planPath,
    "--local-terminal-evidence-root",
    explicitTerminalRoot,
    "--apply-evidence",
    "--json",
  ], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
    env: {
      ...process.env,
      OPPI_PLAN50_DEFAULT_EVIDENCE_ROOT: defaultRoot,
    },
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  const payload = JSON.parse(result.stdout);
  assert.deepEqual(payload.localPlanRowsReadyToCheck, [
    "Terminal restore after abort/panic is checked on Windows and Unix",
  ]);
  assert.deepEqual(payload.appliedPlanRows, [
    "Terminal restore after abort/panic is checked on Windows and Unix",
  ]);
  const updated = readFileSync(planPath, "utf8");
  assert.match(updated, /- \[ \] `\/background` list\/read\/kill is dogfooded through native shell\./);
  assert.match(updated, /- \[x\] Terminal restore after abort\/panic is checked on Windows and Unix\./);
  assert.match(updated, /- \[ \] Sandboxed background tasks instead of full-access\/unrestricted background shell tasks; default promotion requires this\./);
});

test("plan50 audit rejects apply mode without evidence root", () => {
  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--apply-evidence", "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  assert.equal(result.status, 2, result.stderr || result.stdout);
  assert.match(result.stderr, /--apply-evidence requires --evidence-root .* or local evidence arguments/);
});

test("plan50 audit summary prints concise closeout routes", () => {
  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--summary"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
    env: {
      ...process.env,
      OPPI_PLAN50_TEST_CI_CHANGES: "M packages/cli/src/main.ts\n?? .github/workflows/native-shell.yml\n?? crates/",
      OPPI_PLAN50_TEST_SENSITIVE_CI_CHANGES: "?? .github/workflows/native-shell.yml\n?? .github/workflows/sandbox.yml",
      OPPI_PLAN50_TEST_ALL_CHANGES: [
        "M packages/cli/src/main.ts",
        "?? .github/workflows/native-shell.yml",
        "?? crates/",
        "M packages/pi-package/package.json",
        "?? docs/native-ui-pi-parity-sanity.md",
      ].join("\n"),
    },
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  assert.match(result.stdout, /Plan 50 audit: blocked/);
  assert.match(result.stdout, /Plan file: .*OPPI_PLAN50_PLAN_PATH/);
  assert.match(result.stdout, /Open rows \(3\):/);
  assert.match(result.stdout, /Valid closeout routes:/);
  assert.match(result.stdout, /Local Windows sandbox \(partial on this host\)/);
  assert.match(result.stdout, /still needs separate Unix terminal restore evidence/);
  assert.match(result.stdout, /requires elevation: yes/);
  assert.match(result.stdout, /mutates host: yes/);
  assert.match(result.stdout, /approve Windows sandbox setup/);
  assert.match(result.stdout, /gated step: Open an elevated PowerShell in the repo\./);
  assert.match(result.stdout, /gated step: Run the Windows sandbox setup with --yes\./);
  assert.match(result.stdout, /gated step: Verify sandbox status before capturing strict background evidence\./);
  assert.match(result.stdout, /exact user approval phrase: "approve Windows sandbox setup for Plan 50"/);
  assert.match(result.stdout, /CI artifact route: review\/stage\/test\/commit the curated Plan 50 set, fix gh auth, push, run the workflow, download artifacts, verify, then apply evidence\./);
  assert.doesNotMatch(result.stdout, /CI artifact route: review\/stage\/test\/commit\/push the curated Plan 50 set, fix gh auth/);
  assert.match(result.stdout, /CI artifact route[\s\S]*recommended: yes/);
  assert.match(result.stdout, /review doc: \.oppi-plans\/50-ci-evidence-publish-set-review\.md/);
  assert.match(result.stdout, /requires network: yes/);
  assert.match(result.stdout, /requires GitHub auth: yes/);
  assert.match(result.stdout, /requires commit\/push: yes/);
  assert.match(result.stdout, /mutates local repo: yes/);
  assert.match(result.stdout, /mutates GitHub auth: yes/);
  assert.match(result.stdout, /mutates remote: yes/);
  assert.match(result.stdout, /gated step: Review the curated Plan 50 publish set\./);
  assert.match(result.stdout, /gated step: Stage only the curated Plan 50 publish set\./);
  assert.match(result.stdout, /gated step: Commit the curated Plan 50 publish set\./);
  assert.match(
    result.stdout,
    /gated step: Commit the curated Plan 50 publish set\.[\s\S]*gated step: Repair GitHub CLI auth if gh auth status is invalid\.[\s\S]*gated step: Push the selected ref before running GitHub Actions\./,
  );
  assert.match(result.stdout, /exact user approval phrase: "approve Plan 50 CI evidence route"/);
  assert.match(result.stdout, /blocked: Relevant Plan 50 workflow\/runtime changes must be reviewed, staged, tested, committed, and pushed after GitHub auth is valid\./);
  assert.doesNotMatch(result.stdout, /blocked: Relevant Plan 50 workflow\/runtime changes must be reviewed, committed, and pushed\./);
  assert.match(result.stdout, /GitHub CLI auth is not valid/);
  assert.match(result.stdout, /Local non-approval closeout: unavailable/);
  assert.match(result.stdout, /blocked: Local sandbox adapter is not configured; run the approved sandbox setup route or use CI\./);
  assert.match(result.stdout, /windows-sandbox-setup-dry-run \[dry run, no host mutation\]: node packages\/cli\/dist\/main\.js sandbox setup-windows --dry-run --json/);
  assert.match(result.stdout, /windows-sandbox-setup-explicit-approval \[approval required, requires elevation, mutates host\]: node packages\/cli\/dist\/main\.js sandbox setup-windows --yes --json/);
  assert.match(result.stdout, /publish-ci-evidence-inputs \[review only, no remote mutation\]: git status --short/);
  assert.match(result.stdout, /publish-ci-evidence-inputs[\s\S]*review doc: \.oppi-plans\/50-ci-evidence-publish-set-review\.md/);
  assert.match(result.stdout, /publish-ci-evidence-inputs[\s\S]*warning: dirty sensitive paths outside curated Plan 50 publish set: \.github\/workflows\/sandbox\.yml/);
  assert.match(result.stdout, /publish-ci-evidence-inputs[\s\S]*warning: worktree has 2 dirty path\(s\) outside the curated Plan 50 publish set; use the exact stage command, not git add -A\./);
  assert.match(result.stdout, /stage-ci-evidence-inputs \[approval required, mutates local repo, no remote mutation\]: git add --/);
  assert.match(result.stdout, /  verify: git diff --cached --name-status --/);
  assert.match(result.stdout, /  verify: git diff --cached --check --/);
  assert.match(result.stdout, /verify-plan50-local-tests \[no remote mutation\]: pnpm run plan50:test/);
  assert.match(result.stdout, /commit-ci-evidence-inputs \[approval required, mutates local repo, no remote mutation\]: git commit -m "Prepare Plan 50 native evidence gates"/);
  assert.match(result.stdout, /github-auth-preflight \[requires network, checks GitHub auth\]: gh auth status/);
  assert.match(result.stdout, /github-auth-repair \[approval required, requires network, mutates GitHub auth\]: gh auth login -h github\.com/);
  assert.match(result.stdout, /github-auth-repair[\s\S]*\n  verify: gh auth status/);
  assert.match(
    result.stdout,
    /commit-ci-evidence-inputs \[approval required, mutates local repo, no remote mutation\]: git commit -m "Prepare Plan 50 native evidence gates"[\s\S]*github-auth-preflight \[requires network, checks GitHub auth\]: gh auth status[\s\S]*github-auth-repair \[approval required, requires network, mutates GitHub auth\]: gh auth login -h github\.com[\s\S]*push-ci-evidence-inputs \[approval required, requires network, requires GitHub auth, requires commit\/push, mutates remote\]: git push origin/,
  );
  assert.match(result.stdout, /github-ci-evidence-run \[approval required, requires network, requires GitHub auth, requires commit\/push, mutates remote, unavailable here\]: gh workflow run native-shell\.yml --ref /);
  assert.match(result.stdout, /verify-downloaded-ci-evidence \[unavailable here, requires input\]: node scripts\/plan50-evidence-verify\.mjs <downloaded-plan50-evidence-root> --json/);
  assert.match(result.stdout, /unix-terminal-restore-evidence \[unavailable here, requires WSL\/Unix or CI\]: node scripts\/plan50-capture-local-terminal\.mjs --output-dir/);
  assert.match(result.stdout, /local-background-lifecycle-evidence \[unavailable here, requires sandbox setup\]: node scripts\/plan50-capture-local-background\.mjs --output .*tui-dogfood-strict-(Windows|Linux|macOS)\.json/);
  assert.doesNotMatch(result.stdout, /local-background-lifecycle-evidence \[unavailable here, requires sandbox setup\]: node packages\/cli\/dist\/main\.js tui dogfood --mock --json --require-background-lifecycle/);
  assert.match(result.stdout, /windows-sandbox-setup-dry-run[\s\S]*\n  verify: node packages\/cli\/dist\/main\.js sandbox status --json/);
  assert.doesNotMatch(result.stdout, /^\{/);
});

test("plan50 audit approval view prints only approval options", () => {
  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--approval"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
    env: {
      ...process.env,
      OPPI_PLAN50_TEST_CI_CHANGES: "M packages/cli/src/main.ts\n?? .github/workflows/native-shell.yml\n?? crates/",
      OPPI_PLAN50_TEST_SENSITIVE_CI_CHANGES: "?? .github/workflows/native-shell.yml\n?? .github/workflows/sandbox.yml",
      OPPI_PLAN50_TEST_ALL_CHANGES: [
        "M packages/cli/src/main.ts",
        "?? .github/workflows/native-shell.yml",
        "?? crates/",
        "M packages/pi-package/package.json",
        "?? docs/native-ui-pi-parity-sanity.md",
      ].join("\n"),
    },
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  assert.match(result.stdout, /Plan 50 approval required/);
  assert.match(result.stdout, /Plan file: .*OPPI_PLAN50_PLAN_PATH/);
  assert.match(result.stdout, /Recommended route: approve Plan 50 CI evidence route \(all-remaining-rows\)/);
  assert.match(result.stdout, /local-windows-sandbox/);
  assert.match(result.stdout, /scope: partial-on-this-host/);
  assert.match(result.stdout, /partial on this host/);
  assert.match(result.stdout, /requires elevation: yes/);
  assert.match(result.stdout, /mutates host: yes/);
  assert.match(result.stdout, /gated step: Open an elevated PowerShell in the repo\./);
  assert.match(result.stdout, /gated step: Run the Windows sandbox setup with --yes\./);
  assert.match(result.stdout, /gated step: Verify sandbox status before capturing strict background evidence\./);
  assert.match(result.stdout, /still needs Unix terminal restore evidence/);
  assert.match(result.stdout, /approve Windows sandbox setup for Plan 50/);
  assert.match(result.stdout, /multi-os-ci-artifacts/);
  assert.match(result.stdout, /multi-os-ci-artifacts[\s\S]*recommended: yes/);
  assert.match(result.stdout, /scope: all-remaining-rows/);
  assert.match(result.stdout, /requires network: yes/);
  assert.match(result.stdout, /requires GitHub auth: yes/);
  assert.match(result.stdout, /requires commit\/push: yes/);
  assert.match(result.stdout, /mutates local repo: yes/);
  assert.match(result.stdout, /mutates remote: yes/);
  assert.match(result.stdout, /review doc: \.oppi-plans\/50-ci-evidence-publish-set-review\.md/);
  assert.match(result.stdout, /warning: dirty sensitive paths outside curated Plan 50 publish set: \.github\/workflows\/sandbox\.yml/);
  assert.match(result.stdout, /warning: worktree has 2 dirty path\(s\) outside the curated Plan 50 publish set; use the exact stage command, not git add -A\./);
  assert.match(result.stdout, /gated step: Review the curated Plan 50 publish set\./);
  assert.match(result.stdout, /gated step: Stage only the curated Plan 50 publish set\./);
  assert.match(result.stdout, /gated step: Commit the curated Plan 50 publish set\./);
  assert.match(
    result.stdout,
    /gated step: Commit the curated Plan 50 publish set\.[\s\S]*gated step: Repair GitHub CLI auth if gh auth status is invalid\.[\s\S]*gated step: Push the selected ref before running GitHub Actions\./,
  );
  assert.match(result.stdout, /approve Plan 50 CI evidence route/);
  assert.match(result.stdout, /OPPI Windows sandbox account\/WFP\/env setup is not configured/);
  assert.match(result.stdout, /GitHub CLI auth is not valid/);
  assert.doesNotMatch(result.stdout, /Next commands:/);
  assert.doesNotMatch(result.stdout, /^\{/);
});

test("plan50 audit summary and approval do not call local route partial after Unix evidence exists", () => {
  const root = tempEvidenceRoot();
  writeLocalTerminalEvidence(root, "Windows");
  writeLocalTerminalEvidence(root, "Linux");

  const common = {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
    env: {
      ...process.env,
      OPPI_PLAN50_TEST_CI_CHANGES: "M packages/cli/src/main.ts\n?? .github/workflows/native-shell.yml\n?? crates/",
    },
  };
  const summary = spawnSync(process.execPath, [
    "scripts/plan50-audit.mjs",
    "--local-terminal-evidence-root",
    root,
    "--summary",
  ], common);
  assert.equal(summary.status, 1, summary.stderr || summary.stdout);
  assert.match(summary.stdout, /Local Windows sandbox: approve Windows sandbox setup for Plan 50/);
  assert.doesNotMatch(summary.stdout, /Local Windows sandbox \(partial on this host\)/);
  assert.doesNotMatch(summary.stdout, /still needs separate Unix terminal restore evidence/);
  if (process.platform === "win32") {
    assert.match(summary.stdout, /Local Windows sandbox:[\s\S]*recommended: yes/);
    assert.doesNotMatch(summary.stdout, /CI artifact route[\s\S]*recommended: yes/);
  } else {
    assert.match(summary.stdout, /CI artifact route[\s\S]*recommended: yes/);
  }

  const approval = spawnSync(process.execPath, [
    "scripts/plan50-audit.mjs",
    "--local-terminal-evidence-root",
    root,
    "--approval",
  ], common);
  assert.equal(approval.status, 1, approval.stderr || approval.stdout);
  assert.match(approval.stdout, /route: local-windows-sandbox/);
  assert.match(approval.stdout, /scope: all-remaining-rows/);
  assert.doesNotMatch(approval.stdout, /partial on this host/);
  assert.doesNotMatch(approval.stdout, /still needs Unix terminal restore evidence/);
  if (process.platform === "win32") {
    assert.match(approval.stdout, /Recommended route: approve Windows sandbox setup for Plan 50 \(all-remaining-rows\)/);
    assert.match(approval.stdout, /route: local-windows-sandbox[\s\S]*recommended: yes/);
    assert.doesNotMatch(approval.stdout, /route: multi-os-ci-artifacts[\s\S]*recommended: yes/);
  } else {
    assert.match(approval.stdout, /Recommended route: approve Plan 50 CI evidence route \(all-remaining-rows\)/);
    assert.match(approval.stdout, /route: multi-os-ci-artifacts[\s\S]*recommended: yes/);
  }
});

test("plan50 audit auto-detects default local terminal evidence", () => {
  const root = tempEvidenceRoot();
  const terminalRoot = join(root, "plan50-terminal-evidence");
  writeLocalTerminalEvidence(terminalRoot, localRunnerOs());

  const result = spawnSync(process.execPath, ["scripts/plan50-audit.mjs", "--summary"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
    env: {
      ...process.env,
      OPPI_PLAN50_DEFAULT_EVIDENCE_ROOT: root,
    },
  });
  assert.equal(result.status, 1, result.stderr || result.stdout);
  assert.match(result.stdout, new RegExp(`Captured local terminal cleanup evidence passed for ${localRunnerOs()}`));
  assert.doesNotMatch(result.stdout, /terminal-restore-local-platform: Requires captured terminal cleanup logs/);
});

test("root package exposes Plan 50 helper scripts", () => {
  const rootPackage = JSON.parse(readFileSync(join(repoRoot, "package.json"), "utf8"));
  assert.equal(rootPackage.scripts["plan50:audit"], "node scripts/plan50-audit.mjs --json");
  assert.equal(rootPackage.scripts["plan50:summary"], "node scripts/plan50-audit.mjs --summary");
  assert.equal(rootPackage.scripts["plan50:approval"], "node scripts/plan50-audit.mjs --approval");
  assert.equal(rootPackage.scripts["plan50:test"], "node scripts/plan50-test.mjs");
  const runner = readFileSync(join(repoRoot, "scripts", "plan50-test.mjs"), "utf8");
  for (const script of [
    "scripts/plan50-audit.mjs",
    "scripts/plan50-audit.test.mjs",
    "scripts/plan50-capture-local-background.mjs",
    "scripts/plan50-capture-local-terminal.mjs",
    "scripts/plan50-evidence-verify.mjs",
    "scripts/plan50-evidence-verify.test.mjs",
    "scripts/plan50-test.mjs",
  ]) {
    assert.ok(runner.includes(script), `${script} missing from plan50-test runner`);
  }
});
