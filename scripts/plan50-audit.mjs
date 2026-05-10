#!/usr/bin/env node
import { existsSync, readFileSync, readdirSync, statSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { basename, join, resolve } from "node:path";
import { spawnSync } from "node:child_process";

const repoRoot = resolve(import.meta.dirname, "..");
const defaultPlanPath = join(repoRoot, ".oppi-plans", "50-standalone-oppi-finish-line.md");
const defaultWorkflowPath = join(repoRoot, ".github", "workflows", "native-shell.yml");
const defaultManifestWriterPath = join(repoRoot, "scripts", "plan50-write-evidence-manifest.mjs");
const ciEvidencePublishSetReviewPath = ".oppi-plans/50-ci-evidence-publish-set-review.md";
const sandboxLibPath = join(repoRoot, "crates", "oppi-sandbox", "src", "lib.rs");
const windowsProcessPath = join(repoRoot, "crates", "oppi-sandbox", "src", "windows_process.rs");
const ciEvidenceInputPaths = [
  ".gitignore",
  ".github/workflows/native-shell.yml",
  "scripts/plan50-audit.mjs",
  "scripts/plan50-audit.test.mjs",
  "scripts/plan50-capture-local-background.mjs",
  "scripts/plan50-capture-local-terminal.mjs",
  "scripts/plan50-evidence-verify.mjs",
  "scripts/plan50-evidence-verify.test.mjs",
  "scripts/plan50-test.mjs",
  "scripts/plan50-write-evidence-manifest.mjs",
  "scripts/plan50-write-evidence-manifest.test.mjs",
  "Cargo.toml",
  "Cargo.lock",
  "crates",
  "package.json",
  "pnpm-lock.yaml",
  "pnpm-workspace.yaml",
  "packages/cli",
  "packages/native",
  "packages/natives",
];
const ciEvidenceSensitiveReviewPaths = [
  ".github/workflows",
];
const nativeShellCriticalStepNames = [
  "Enable pnpm",
  "Install Linux sandbox dependencies",
  "Install workspace dependencies",
  "Build native shell and server",
  "Prepare Plan 50 evidence folder",
  "Test native Rust workspace",
  "Check Linux Bubblewrap host sandbox",
  "Check terminal cleanup lifecycle",
  "Build CLI wrapper",
  "Test CLI package",
  "Test native npm packages",
  "Test Plan 50 audit helpers",
  "Smoke native shell through CLI",
  "Dogfood native shell through CLI",
  "Strict sandboxed background dogfood on Linux",
  "Write Plan 50 evidence manifest",
  "Upload Plan 50 native shell evidence",
];
const plan50EvidenceCriticalStepNames = [
  "Download Plan 50 native shell evidence artifacts",
  "Verify Plan 50 evidence bundle",
  "Require native shell matrix success",
];
const requiredWorkflowJobIds = ["native-shell", "plan50-evidence"];

function argValue(name) {
  const inline = process.argv.find((arg) => arg.startsWith(`${name}=`));
  if (inline) return inline.slice(name.length + 1);
  const index = process.argv.indexOf(name);
  return index >= 0 ? process.argv[index + 1] : undefined;
}

function commandAvailable(command, args = ["--version"]) {
  const result = spawnSync(command, args, { encoding: "utf8", windowsHide: true, timeout: 5_000 });
  return !result.error && result.status === 0;
}

function currentGitRef() {
  const result = spawnSync("git", ["branch", "--show-current"], { encoding: "utf8", windowsHide: true, timeout: 5_000 });
  const branch = result.status === 0 ? result.stdout.trim() : "";
  return branch || "HEAD";
}

function gitRemoteUrl(remote = "origin") {
  const result = spawnSync("git", ["remote", "get-url", remote], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
    timeout: 5_000,
  });
  return result.status === 0 ? result.stdout.trim() : "";
}

function githubCliStatus() {
  const version = spawnSync("gh", ["--version"], { encoding: "utf8", windowsHide: true, timeout: 5_000 });
  if (version.error || version.status !== 0) {
    return {
      installed: false,
      authenticated: false,
      status: "missing",
      diagnostics: [version.error?.message || "gh --version failed"],
    };
  }
  const auth = spawnSync("gh", ["auth", "status"], { encoding: "utf8", windowsHide: true, timeout: 5_000 });
  const diagnostics = `${auth.stdout || ""}\n${auth.stderr || ""}`
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean)
    .slice(0, 12);
  return {
    installed: true,
    authenticated: auth.status === 0,
    status: auth.status === 0 ? "authenticated" : "not-authenticated",
    diagnostics,
  };
}

function ciEvidenceInputStatus() {
  const statusCommand = `git status --short -uall -- ${ciEvidenceInputPaths.join(" ")}`;
  const testChanges = process.env.OPPI_PLAN50_TEST_CI_CHANGES;
  const sensitiveStatusCommand = `git status --short -uall -- ${ciEvidenceSensitiveReviewPaths.join(" ")}`;
  const sensitiveTestChanges = process.env.OPPI_PLAN50_TEST_SENSITIVE_CI_CHANGES;
  const allDirtyStatusCommand = "git status --short -uall";
  const allDirtyTestChanges = process.env.OPPI_PLAN50_TEST_ALL_CHANGES;
  let lines = [];
  let sensitiveLines = [];
  let allDirtyLines = [];
  let statusError = "";
  let sensitiveStatusError = "";
  let allDirtyStatusError = "";

  if (testChanges !== undefined) {
    lines = testChanges.split(/\r?\n/).map((line) => line.trim()).filter(Boolean);
  } else {
    const result = spawnSync("git", ["status", "--short", "-uall", "--", ...ciEvidenceInputPaths], {
      cwd: repoRoot,
      encoding: "utf8",
      windowsHide: true,
      timeout: 5_000,
    });
    lines = result.status === 0
      ? result.stdout.split(/\r?\n/).map((line) => line.trim()).filter(Boolean)
      : [];
    statusError = result.status === 0 ? "" : (result.stderr || result.error?.message || "git status failed").trim();
  }

  if (sensitiveTestChanges !== undefined) {
    sensitiveLines = sensitiveTestChanges.split(/\r?\n/).map((line) => line.trim()).filter(Boolean);
  } else if (testChanges !== undefined) {
    sensitiveLines = lines;
  } else {
    const result = spawnSync("git", ["status", "--short", "-uall", "--", ...ciEvidenceSensitiveReviewPaths], {
      cwd: repoRoot,
      encoding: "utf8",
      windowsHide: true,
      timeout: 5_000,
    });
    sensitiveLines = result.status === 0
      ? result.stdout.split(/\r?\n/).map((line) => line.trim()).filter(Boolean)
      : [];
    sensitiveStatusError = result.status === 0 ? "" : (result.stderr || result.error?.message || "git status failed").trim();
  }

  if (allDirtyTestChanges !== undefined) {
    allDirtyLines = allDirtyTestChanges.split(/\r?\n/).map((line) => line.trim()).filter(Boolean);
  } else if (testChanges !== undefined) {
    allDirtyLines = lines;
  } else {
    const result = spawnSync("git", ["status", "--short", "-uall"], {
      cwd: repoRoot,
      encoding: "utf8",
      windowsHide: true,
      timeout: 5_000,
    });
    allDirtyLines = result.status === 0
      ? result.stdout.split(/\r?\n/).map((line) => line.trim()).filter(Boolean)
      : [];
    allDirtyStatusError = result.status === 0 ? "" : (result.stderr || result.error?.message || "git status failed").trim();
  }

  const stagePaths = gitStatusStagePaths(lines);
  const excludedDirtyPaths = gitStatusStagePaths(sensitiveLines)
    .filter((path) => !ciEvidenceCuratedPathCovers(path));
  const nonCuratedDirtyChanges = allDirtyLines
    .filter((line) => !gitStatusPayloadPaths(line).every((path) =>
      stagePaths.some((stagePath) => gitPathCovers(path, stagePath))
    ));

  return {
    dirty: lines.length > 0,
    count: lines.length,
    changes: lines,
    stagePaths,
    sample: lines.slice(0, 10),
    curatedPaths: ciEvidenceInputPaths,
    statusCommand,
    sensitiveStatusCommand,
    sensitiveReviewPaths: ciEvidenceSensitiveReviewPaths,
    sensitiveChanges: sensitiveLines,
    excludedDirtyPaths,
    allDirtyStatusCommand,
    allDirtyCount: allDirtyLines.length,
    allDirtySample: allDirtyLines.slice(0, 10),
    nonCuratedDirtyCount: nonCuratedDirtyChanges.length,
    nonCuratedDirtyChanges,
    nonCuratedDirtySample: nonCuratedDirtyChanges.slice(0, 10),
    gitRef: currentGitRef(),
    originUrl: gitRemoteUrl("origin") || null,
    statusError,
    sensitiveStatusError,
    allDirtyStatusError,
  };
}

function wslHasDistribution() {
  if (process.platform !== "win32") return undefined;
  const result = spawnSync("wsl", ["-l", "-q"], { encoding: "utf8", windowsHide: true, timeout: 5_000 });
  if (result.status !== 0) return false;
  const output = `${result.stdout}\n${result.stderr}`
    .replace(/\0/g, "")
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean)
    .filter((line) => !/Windows Subsystem for Linux/i.test(line));
  return output.length > 0;
}

function unixEvidenceRunnerAvailable() {
  if (process.env.OPPI_PLAN50_TEST_UNIX_RUNNER_AVAILABLE === "1") return true;
  return process.platform !== "win32" || wslHasDistribution() === true;
}

function unixEvidenceActionBlockedBy(unixRunnerAvailable) {
  if (unixRunnerAvailable) return [];
  if (process.platform === "win32") {
    return [
      "No installed WSL distribution is available for local Unix terminal cleanup evidence; use WSL/Unix or CI.",
    ];
  }
  return [
    "Local Unix terminal cleanup evidence runner is unavailable on this host.",
  ];
}

function localBackgroundActionBlockedBy(localSandboxReady) {
  if (localSandboxReady) return [];
  return [
    "Local sandbox adapter is not configured; run the approved sandbox setup route or use CI.",
  ];
}

function githubCiRunBlockedBy(inputStatus, githubCli) {
  return [
    ...(inputStatus.dirty ? ["Relevant Plan 50 workflow/runtime changes must be committed and pushed before CI can test them."] : []),
    ...(githubCli?.authenticated === true ? [] : ["GitHub CLI auth is not valid."]),
  ];
}

function envConfigured(name) {
  return Boolean(process.env[name]?.trim());
}

function codexAuthPath() {
  const explicit = process.env.OPPI_OPENAI_CODEX_AUTH_PATH?.trim();
  if (explicit) return resolve(explicit);
  const agentDir = process.env.OPPI_AGENT_DIR?.trim() || process.env.PI_CODING_AGENT_DIR?.trim() || join(homedir(), ".oppi", "agent");
  return join(resolve(agentDir), "auth.json");
}

function codexAuthConfigured() {
  try {
    const raw = JSON.parse(readFileSync(codexAuthPath(), "utf8"));
    const credential = raw?.["openai-codex"];
    return credential?.type === "oauth" && typeof credential.access === "string" && typeof credential.refresh === "string";
  } catch {
    return false;
  }
}

function providerConfigured() {
  const envPointer = process.env.OPPI_RUNTIME_WORKER_API_KEY_ENV?.trim();
  const allowedPointer = envPointer
    && /^[A-Z0-9_]{1,128}$/.test(envPointer)
    && (
      envPointer === "OPPI_OPENAI_API_KEY"
      || envPointer === "OPENAI_API_KEY"
      || (envPointer.startsWith("OPPI_") && envPointer.endsWith("_API_KEY"))
      || (envPointer.startsWith("OPENAI_") && envPointer.endsWith("_API_KEY"))
      || (envPointer.startsWith("AZURE_OPENAI_") && envPointer.endsWith("_API_KEY"))
    );
  const pointedKey = allowedPointer ? envConfigured(envPointer) : false;
  return pointedKey || (!envPointer && (envConfigured("OPPI_OPENAI_API_KEY") || envConfigured("OPENAI_API_KEY"))) || codexAuthConfigured();
}

function sandboxReady() {
  if (process.env.OPPI_PLAN50_TEST_SANDBOX_READY === "1") return true;
  if (process.platform === "win32") {
    return envConfigured("OPPI_WINDOWS_SANDBOX_USERNAME")
      && envConfigured("OPPI_WINDOWS_SANDBOX_PASSWORD")
      && process.env.OPPI_WINDOWS_SANDBOX_WFP_READY === "1";
  }
  if (process.platform === "linux") return commandAvailable("bwrap");
  if (process.platform === "darwin") return existsSync("/usr/bin/sandbox-exec");
  return false;
}

function walkFiles(dir) {
  if (!dir || !existsSync(dir)) return [];
  if (!statSync(dir).isDirectory()) return [];
  const out = [];
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    const stat = statSync(path);
    if (stat.isDirectory()) out.push(...walkFiles(path));
    else out.push(path);
  }
  return out;
}

function localRunnerOs() {
  if (process.platform === "win32") return "Windows";
  if (process.platform === "darwin") return "macOS";
  if (process.platform === "linux") return "Linux";
  return process.platform;
}

function defaultEvidenceRootOverride() {
  return process.env.OPPI_PLAN50_DEFAULT_EVIDENCE_ROOT?.trim();
}

function defaultLocalBackgroundEvidencePath() {
  const root = defaultEvidenceRootOverride();
  if (root) return join(resolve(root), "plan50-background-evidence", `tui-dogfood-strict-${localRunnerOs()}.json`);
  return `.validation/plan50-background-evidence/tui-dogfood-strict-${localRunnerOs()}.json`;
}

function normalizeGitPath(path) {
  return String(path || "").trim().replaceAll("\\", "/").replace(/\/+$/, "");
}

function displayGitPath(path) {
  return String(path || "").trim().replaceAll("\\", "/");
}

function gitStatusPayloadPaths(line) {
  const text = String(line || "").trim();
  const payload = text.match(/^(?:[ MADRCU?!]{1,2})\s+(.+)$/)?.[1]?.trim() || "";
  if (!payload) return [];
  if (payload.includes(" -> ")) return payload.split(" -> ").map(displayGitPath).filter(Boolean);
  return [displayGitPath(payload)].filter(Boolean);
}

function gitStatusStagePaths(lines) {
  const out = [];
  for (const line of lines || []) {
    for (const path of gitStatusPayloadPaths(line)) {
      if (path && !out.includes(path)) out.push(path);
    }
  }
  return out;
}

function gitPathCovers(path, base) {
  const candidate = normalizeGitPath(path);
  const root = normalizeGitPath(base);
  return Boolean(candidate && root && (candidate === root || candidate.startsWith(`${root}/`)));
}

function ciEvidenceCuratedPathCovers(path) {
  return ciEvidenceInputPaths.some((curatedPath) => gitPathCovers(path, curatedPath));
}

function localBackgroundCaptureCommand(path = defaultLocalBackgroundEvidencePath()) {
  return `node scripts/plan50-capture-local-background.mjs --output ${path}`;
}

function shellQuoteArg(value) {
  const text = String(value);
  return `"${text.replaceAll("\\", "\\\\").replaceAll('"', '\\"')}"`;
}

function ciEvidenceGitAddCommand(stagePaths = ciEvidenceInputPaths) {
  return `git add -- ${stagePaths.map(shellQuoteArg).join(" ")}`;
}

function ciEvidenceCachedDiffCommand(stagePaths = ciEvidenceInputPaths) {
  return `git diff --cached --name-status -- ${stagePaths.map(shellQuoteArg).join(" ")}`;
}

function ciEvidenceCachedDiffCheckCommand(stagePaths = ciEvidenceInputPaths) {
  return `git diff --cached --check -- ${stagePaths.map(shellQuoteArg).join(" ")}`;
}

function ciEvidenceCommitCommand() {
  return `git commit -m "Prepare Plan 50 native evidence gates"`;
}

function ciEvidencePushCommand(ref = currentGitRef()) {
  return `git push origin ${ref}`;
}

function existingDefaultLocalBackgroundEvidencePath() {
  const path = defaultLocalBackgroundEvidencePath();
  return localBackgroundEvidenceReady(path) ? path : undefined;
}

function defaultLocalTerminalEvidenceRoot() {
  const root = defaultEvidenceRootOverride();
  if (root) return join(resolve(root), "plan50-terminal-evidence");
  return ".validation/plan50-terminal-evidence";
}

function localTerminalCaptureCommand(root = defaultLocalTerminalEvidenceRoot()) {
  return `node scripts/plan50-capture-local-terminal.mjs --output-dir ${root}`;
}

function existingDefaultLocalTerminalEvidenceRoot() {
  const root = defaultLocalTerminalEvidenceRoot();
  return localTerminalEvidenceReady(root) || localWindowsUnixTerminalEvidenceReady(root) ? root : undefined;
}

function terminalLogPassed(text) {
  const failedCounts = [...text.matchAll(/\b(\d+)\s+failed\b/gi)].map((match) => Number(match[1]));
  return /test result:\s+ok\./i.test(text)
    && failedCounts.length > 0
    && failedCounts.every((count) => count === 0)
    && !/test result:\s+failed\./i.test(text);
}

function localTerminalEvidenceReady(rootArg) {
  if (!rootArg) return false;
  const root = resolve(rootArg);
  const runnerOs = localRunnerOs();
  const files = walkFiles(root);
  const lifecycle = files.find((file) => basename(file) === `terminal-cleanup-lifecycle-${runnerOs}.log`);
  const reset = files.find((file) => basename(file) === `terminal-cleanup-reset-${runnerOs}.log`);
  if (!lifecycle || !reset) return false;
  return terminalLogPassed(readFileSync(lifecycle, "utf8")) && terminalLogPassed(readFileSync(reset, "utf8"));
}

function terminalPairEvidenceReady(files, runnerOs) {
  const lifecycle = files.find((file) => basename(file) === `terminal-cleanup-lifecycle-${runnerOs}.log`);
  const reset = files.find((file) => basename(file) === `terminal-cleanup-reset-${runnerOs}.log`);
  if (!lifecycle || !reset) return false;
  return terminalLogPassed(readFileSync(lifecycle, "utf8")) && terminalLogPassed(readFileSync(reset, "utf8"));
}

function localWindowsUnixTerminalEvidenceReady(rootArg) {
  if (!rootArg) return false;
  const files = walkFiles(resolve(rootArg));
  return terminalPairEvidenceReady(files, "Windows")
    && (terminalPairEvidenceReady(files, "Linux") || terminalPairEvidenceReady(files, "macOS"));
}

function readJsonFile(pathArg) {
  if (!pathArg) return undefined;
  try {
    return JSON.parse(readFileSync(resolve(pathArg), "utf8"));
  } catch {
    return undefined;
  }
}

function backgroundScenario(payload) {
  return Array.isArray(payload?.scenarios)
    ? payload.scenarios.find((scenario) => scenario?.name === "background-sandbox-execution")
    : undefined;
}

function allScenariosPassed(payload) {
  return Array.isArray(payload?.scenarios)
    && payload.scenarios.length > 0
    && payload.scenarios.every((scenario) => scenario?.ok === true);
}

function backgroundLifecycleStatusPassed(scenario) {
  const status = String(scenario?.status ?? "").toLowerCase();
  const contradictoryFailure = /sandbox-unavailable|unavailable|denied|failed|refused|refusing/.test(status);
  return !contradictoryFailure
    && status.includes("started")
    && /list\s*=\s*true/.test(status)
    && /read\s*=\s*true/.test(status)
    && /kill\s*=\s*true/.test(status);
}

function localBackgroundEvidenceReady(pathArg) {
  const payload = readJsonFile(pathArg);
  const scenario = backgroundScenario(payload);
  return Boolean(
    payload?.ok === true
      && payload?.strictBackgroundLifecycle === true
      && allScenariosPassed(payload)
      && scenario?.ok === true
      && backgroundLifecycleStatusPassed(scenario),
  );
}

function workflowJobBlock(workflow, jobId) {
  const lines = workflow.split(/\r?\n/);
  const startPattern = new RegExp(`^(\\s*)${jobId}:\\s*$`);
  for (let index = 0; index < lines.length; index += 1) {
    const match = lines[index].match(startPattern);
    if (!match) continue;
    const indent = match[1];
    const block = [lines[index]];
    for (let cursor = index + 1; cursor < lines.length; cursor += 1) {
      const line = lines[cursor];
      if (line.trim() && line.startsWith(indent) && !line.startsWith(`${indent} `) && !line.startsWith(`${indent}\t`)) break;
      block.push(line);
    }
    return block.join("\n");
  }
  return "";
}

function workflowJobIds(workflow) {
  const jobsBlock = workflowTopLevelKeyBlock(workflow, "jobs");
  const lines = jobsBlock.split(/\r?\n/);
  const header = lines[0]?.match(/^(\s*)jobs:\s*$/);
  if (!header) return [];
  const expectedIndentLength = header[1].length + 2;
  return lines
    .slice(1)
    .map((line) => line.match(/^(\s*)([^:\s][^:]*):\s*$/))
    .filter((match) => match && match[1].length === expectedIndentLength)
    .map((match) => match[2]);
}

function workflowRequiredJobIdsUnique(workflow) {
  const jobIds = workflowJobIds(workflow);
  return requiredWorkflowJobIds.every((jobId) =>
    jobIds.filter((candidate) => candidate === jobId).length === 1
  );
}

function workflowJobHasTopLevelScalar(workflow, jobId, key, value) {
  const block = workflowJobBlock(workflow, jobId);
  const lines = block.split(/\r?\n/);
  const header = lines[0]?.match(/^(\s*)/);
  if (!header) return false;
  const expectedIndentLength = header[1].length + 2;
  const values = lines
    .map((line) => {
    const match = line.match(/^(\s*)([^:\s][^:]*):\s*(.*?)\s*$/);
      return match && match[1].length === expectedIndentLength && match[2] === key
        ? match[3]
        : undefined;
    })
    .filter((entry) => entry !== undefined);
  return values.length === 1 && values[0] === String(value);
}

function workflowJobTopLevelKeyBlock(workflow, jobId, key) {
  const block = workflowJobBlock(workflow, jobId);
  const lines = block.split(/\r?\n/);
  const header = lines[0]?.match(/^(\s*)/);
  if (!header) return "";
  const expectedIndentLength = header[1].length + 2;
  for (let index = 1; index < lines.length; index += 1) {
    const match = lines[index].match(/^(\s*)([^:\s][^:]*):\s*$/);
    if (!match || match[1].length !== expectedIndentLength || match[2] !== key) continue;
    const blockLines = [lines[index]];
    for (let cursor = index + 1; cursor < lines.length; cursor += 1) {
      const line = lines[cursor];
      const next = line.match(/^(\s*)\S/);
      if (next && next[1].length <= expectedIndentLength) break;
      blockLines.push(line);
    }
    return blockLines.join("\n");
  }
  return "";
}

function workflowKeyBlock(text, key) {
  const lines = text.split(/\r?\n/);
  const startPattern = new RegExp(`^(\\s*)${key}:\\s*$`);
  for (let index = 0; index < lines.length; index += 1) {
    const match = lines[index].match(startPattern);
    if (!match) continue;
    const indentLength = match[1].length;
    const block = [lines[index]];
    for (let cursor = index + 1; cursor < lines.length; cursor += 1) {
      const line = lines[cursor];
      const next = line.match(/^(\s*)\S/);
      if (next && next[1].length <= indentLength) break;
      block.push(line);
    }
    return block.join("\n");
  }
  return "";
}

function workflowTopLevelKeyBlock(text, key) {
  const lines = text.split(/\r?\n/);
  const startPattern = new RegExp(`^${key}:\\s*$`);
  for (let index = 0; index < lines.length; index += 1) {
    if (!startPattern.test(lines[index])) continue;
    const block = [lines[index]];
    for (let cursor = index + 1; cursor < lines.length; cursor += 1) {
      const line = lines[cursor];
      if (/^\S/.test(line)) break;
      block.push(line);
    }
    return block.join("\n");
  }
  return "";
}

const PLAN50_WORKFLOW_PATH_FILTERS = [
  "Cargo.toml",
  "Cargo.lock",
  "crates/**",
  "package.json",
  "pnpm-lock.yaml",
  "pnpm-workspace.yaml",
  "packages/cli/**",
  "packages/native/**",
  "packages/natives/**",
  "scripts/plan50-*.mjs",
  ".github/workflows/native-shell.yml",
];

function workflowTriggerPathsInclude(workflow, eventName, requiredPaths) {
  const onBlock = workflowTopLevelKeyBlock(workflow, "on");
  const pathBlock = workflowKeyBlock(workflowKeyBlock(onBlock, eventName), "paths");
  const paths = pathBlock
    .split(/\r?\n/)
    .map((line) => {
      const match = line.trim().match(/^-\s*(?:"([^"]+)"|'([^']+)'|(.+))$/);
      return match ? (match[1] || match[2] || match[3]).trim() : undefined;
    })
    .filter(Boolean);
  return requiredPaths.every((path) => paths.includes(path));
}

function workflowDefinesPlan50PathFilters(workflow) {
  return workflowTriggerPathsInclude(workflow, "push", PLAN50_WORKFLOW_PATH_FILTERS)
    && workflowTriggerPathsInclude(workflow, "pull_request", PLAN50_WORKFLOW_PATH_FILTERS);
}

function parseYamlValueList(value) {
  return value
    .split(",")
    .map((item) => item.trim().replace(/^["']|["']$/g, ""))
    .filter(Boolean);
}

function workflowMatrixOsList(workflow) {
  const strategy = workflowJobTopLevelKeyBlock(workflow, "native-shell", "strategy");
  const matrix = workflowKeyBlock(strategy, "matrix");
  const lines = matrix.split(/\r?\n/);
  for (let index = 0; index < lines.length; index += 1) {
    const match = lines[index].match(/^(\s*)os:\s*(.*)\s*$/);
    if (!match) continue;
    const indentLength = match[1].length;
    const value = match[2].trim();
    const inlineList = value.match(/^\[(.*)\]$/);
    if (inlineList) return parseYamlValueList(inlineList[1]);
    const items = [];
    for (let cursor = index + 1; cursor < lines.length; cursor += 1) {
      const line = lines[cursor];
      const next = line.match(/^(\s*)\S/);
      if (next && next[1].length <= indentLength) break;
      const item = line.trim().match(/^-\s*(?:"([^"]+)"|'([^']+)'|(.+))$/);
      if (item) items.push((item[1] || item[2] || item[3]).trim());
    }
    return items;
  }
  return [];
}

function workflowNativeShellMatrixIncludesAllOs(workflow) {
  const matrixOs = workflowMatrixOsList(workflow);
  return ["ubuntu-latest", "macos-latest", "windows-latest"].every((os) => matrixOs.includes(os));
}

function workflowNativeShellRunsOnMatrixOs(workflow) {
  return workflowJobHasTopLevelScalar(workflow, "native-shell", "runs-on", "${{ matrix.os }}");
}

function workflowDefinesStrictLinuxBackgroundDogfood(workflow) {
  const stepName = "Strict sandboxed background dogfood on Linux";
  return workflowNativeShellStepScalarMatches(workflow, stepName, "if", /^runner\.os == 'Linux'$/)
    && workflowNativeShellStepRunBodyIncludes(workflow, stepName, /tui dogfood --mock --json --require-background-lifecycle/)
    && workflowNativeShellStepRunBodyIncludes(workflow, stepName, /plan50-evidence\/tui-dogfood-strict-\$\{RUNNER_OS\}\.json/);
}

function workflowDefinesLinuxHostSandboxCheck(workflow) {
  const stepName = "Check Linux Bubblewrap host sandbox";
  return workflowNativeShellStepScalarMatches(workflow, stepName, "if", /^runner\.os == 'Linux'$/)
    && workflowNativeShellStepRunBodyIncludes(workflow, stepName, /host_linux_bubblewrap_blocks_network_when_disabled -- --ignored --nocapture/)
    && workflowNativeShellStepRunBodyIncludes(workflow, stepName, /linux-bubblewrap-host-sandbox-\$\{RUNNER_OS\}\.log/);
}

function workflowDefinesTerminalCleanupChecks(workflow) {
  const stepName = "Check terminal cleanup lifecycle";
  return workflowNativeShellStepRunBodyIncludes(workflow, stepName, /ratatui_lifecycle_exit_paths_share_cleanup_contract -- --nocapture/)
    && workflowNativeShellStepRunBodyIncludes(workflow, stepName, /terminal-cleanup-lifecycle-\$\{RUNNER_OS\}\.log/)
    && workflowNativeShellStepRunBodyIncludes(workflow, stepName, /ratatui_terminal_cleanup_sequence_resets_and_clears -- --nocapture/)
    && workflowNativeShellStepRunBodyIncludes(workflow, stepName, /terminal-cleanup-reset-\$\{RUNNER_OS\}\.log/);
}

function workflowDefinesNativeShellCliSmokeAndDogfood(workflow) {
  return workflowNativeShellStepRunBodyIncludes(
    workflow,
    "Smoke native shell through CLI",
    /node packages\/cli\/dist\/main\.js tui smoke --mock --json \| tee "plan50-evidence\/tui-smoke-\$\{RUNNER_OS\}\.json"/,
  )
    && workflowNativeShellStepRunBodyIncludes(
      workflow,
      "Dogfood native shell through CLI",
      /node packages\/cli\/dist\/main\.js tui dogfood --mock --json \| tee "plan50-evidence\/tui-dogfood-\$\{RUNNER_OS\}\.json"/,
    );
}

function workflowDefinesCliNativeBinaryTargets(workflow, stepName) {
  return workflowNativeShellStepRunBodyIncludes(
    workflow,
    stepName,
    /^[ \t]*OPPI_SERVER_BIN="\$\{root\}\\\\target\\\\debug\\\\oppi-server\.exe"\s*\\\s*$/,
  )
    && workflowNativeShellStepRunBodyIncludes(
      workflow,
      stepName,
      /^[ \t]*OPPI_SHELL_BIN="\$\{root\}\\\\target\\\\debug\\\\oppi-shell\.exe"\s*\\\s*$/,
    )
    && workflowNativeShellStepRunBodyIncludes(
      workflow,
      stepName,
      /^[ \t]*OPPI_SERVER_BIN="\$\{PWD\}\/target\/debug\/oppi-server"\s*\\\s*$/,
    )
    && workflowNativeShellStepRunBodyIncludes(
      workflow,
      stepName,
      /^[ \t]*OPPI_SHELL_BIN="\$\{PWD\}\/target\/debug\/oppi-shell"\s*\\\s*$/,
    );
}

function workflowDefinesStrictNativeBinaryTargets(workflow) {
  const stepName = "Strict sandboxed background dogfood on Linux";
  return workflowNativeShellStepRunBodyIncludes(
    workflow,
    stepName,
    /^[ \t]*OPPI_SERVER_BIN="\$\{PWD\}\/target\/debug\/oppi-server"\s*\\\s*$/,
  )
    && workflowNativeShellStepRunBodyIncludes(
      workflow,
      stepName,
      /^[ \t]*OPPI_SHELL_BIN="\$\{PWD\}\/target\/debug\/oppi-shell"\s*\\\s*$/,
    );
}

function workflowDefinesNativeShellCliBinaryTargets(workflow) {
  return workflowDefinesCliNativeBinaryTargets(workflow, "Smoke native shell through CLI")
    && workflowDefinesCliNativeBinaryTargets(workflow, "Dogfood native shell through CLI")
    && workflowDefinesStrictNativeBinaryTargets(workflow);
}

function workflowDefinesRustWorkspaceTest(workflow) {
  return workflowNativeShellStepScalarMatches(
    workflow,
    "Test native Rust workspace",
    "run",
    /^cargo test --workspace$/,
  );
}

function workflowDefinesCliPackageTest(workflow) {
  return workflowNativeShellStepScalarMatches(
    workflow,
    "Test CLI package",
    "run",
    /^pnpm --filter @oppiai\/cli test$/,
  );
}

function workflowDefinesCliWrapperBuild(workflow) {
  return workflowNativeShellStepScalarMatches(
    workflow,
    "Build CLI wrapper",
    "run",
    /^pnpm --filter @oppiai\/cli build$/,
  );
}

function workflowDefinesNativeShellBinaryBuild(workflow) {
  return workflowNativeShellStepScalarMatches(
    workflow,
    "Build native shell and server",
    "run",
    /^cargo build -p oppi-server -p oppi-shell$/,
  );
}

function workflowDefinesNativeShellBinaryBuildOrder(workflow) {
  return workflowJobStepOrderMatches(workflow, "native-shell", [
    (step) => step.kind === "name" && step.value === "Install workspace dependencies",
    (step) => step.kind === "name" && step.value === "Build native shell and server",
    (step) => step.kind === "name" && step.value === "Smoke native shell through CLI",
    (step) => step.kind === "name" && step.value === "Dogfood native shell through CLI",
    (step) => step.kind === "name" && step.value === "Strict sandboxed background dogfood on Linux",
  ]);
}

function workflowDefinesNativeNpmPackageTests(workflow) {
  const stepName = "Test native npm packages";
  return workflowNativeShellStepRunBodyIncludes(workflow, stepName, /^[ \t]*pnpm --filter @oppiai\/native test\s*$/)
    && workflowNativeShellStepRunBodyIncludes(workflow, stepName, /^[ \t]*pnpm --filter @oppiai\/natives test\s*$/);
}

function workflowDefinesPlan50HelperTests(workflow) {
  return workflowNativeShellStepScalarMatches(
    workflow,
    "Test Plan 50 audit helpers",
    "run",
    /^pnpm run plan50:test$/,
  );
}

function workflowDefinesNativeShellSetupOrder(workflow) {
  return workflowJobStepOrderMatches(workflow, "native-shell", [
    (step) => step.kind === "uses" && /^actions\/checkout@v4$/.test(step.value),
    (step) => step.kind === "uses" && /^dtolnay\/rust-toolchain@stable$/.test(step.value),
    (step) => step.kind === "uses" && /^actions\/setup-node@v4$/.test(step.value),
    (step) => step.kind === "name" && step.value === "Enable pnpm",
    (step) => step.kind === "name" && step.value === "Install Linux sandbox dependencies",
    (step) => step.kind === "name" && step.value === "Install workspace dependencies",
  ]);
}

function workflowDefinesNativeShellValidationOrder(workflow) {
  return workflowJobStepOrderMatches(workflow, "native-shell", [
    (step) => step.kind === "name" && step.value === "Install workspace dependencies",
    (step) => step.kind === "name" && step.value === "Build native shell and server",
    (step) => step.kind === "name" && step.value === "Test native Rust workspace",
    (step) => step.kind === "name" && step.value === "Build CLI wrapper",
    (step) => step.kind === "name" && step.value === "Test CLI package",
    (step) => step.kind === "name" && step.value === "Test native npm packages",
    (step) => step.kind === "name" && step.value === "Test Plan 50 audit helpers",
    (step) => step.kind === "name" && step.value === "Smoke native shell through CLI",
    (step) => step.kind === "name" && step.value === "Dogfood native shell through CLI",
    (step) => step.kind === "name" && step.value === "Strict sandboxed background dogfood on Linux",
  ]);
}

function workflowDefinesWorkspaceDependencyInstall(workflow) {
  return workflowNativeShellStepScalarMatches(
    workflow,
    "Install workspace dependencies",
    "run",
    /^pnpm install --frozen-lockfile$/,
  );
}

function workflowDefinesPnpmEnable(workflow) {
  return workflowNativeShellStepScalarMatches(
    workflow,
    "Enable pnpm",
    "run",
    /^corepack enable$/,
  );
}

function workflowStepDirectChildBlocks(lines, stepIndex, itemIndent, key) {
  const expectedIndentLength = itemIndent.length + 2;
  const blocks = [];
  for (let cursor = stepIndex + 1; cursor < lines.length; cursor += 1) {
    const line = lines[cursor];
    if (line.startsWith(itemIndent) && line.slice(itemIndent.length).startsWith("- ")) break;
    const match = line.match(/^(\s*)([^:\s][^:]*):\s*$/);
    if (!match || match[1].length !== expectedIndentLength || match[2] !== key) continue;
    const block = [line];
    for (let inner = cursor + 1; inner < lines.length; inner += 1) {
      const innerLine = lines[inner];
      if (innerLine.startsWith(itemIndent) && innerLine.slice(itemIndent.length).startsWith("- ")) break;
      const next = innerLine.match(/^(\s*)\S/);
      if (next && next[1].length <= expectedIndentLength) break;
      block.push(innerLine);
    }
    blocks.push(block.join("\n"));
  }
  return blocks;
}

function workflowStepDirectChildBlock(lines, stepIndex, itemIndent, key) {
  const blocks = workflowStepDirectChildBlocks(lines, stepIndex, itemIndent, key);
  return blocks.length === 1 ? blocks[0] : "";
}

function workflowJobStepUsesWithIncludes(workflow, jobId, usesPattern, expectedPattern) {
  const lines = workflowJobBlock(workflow, jobId).split(/\r?\n/);
  for (let index = 0; index < lines.length; index += 1) {
    const match = lines[index].match(/^(\s*)-\s+uses:\s*(.+?)\s*$/);
    if (!match || !usesPattern.test(match[2])) continue;
    return workflowStepDirectChildBlock(lines, index, match[1], "with")
      .split(/\r?\n/)
      .some((line) => expectedPattern.test(line));
  }
  return false;
}

function workflowJobStepIndex(workflow, jobId, matcher) {
  const lines = workflowJobBlock(workflow, jobId).split(/\r?\n/);
  for (let index = 0; index < lines.length; index += 1) {
    const match = lines[index].match(/^(\s*)-\s+(name|uses):\s*(.+?)\s*$/);
    if (!match) continue;
    if (matcher({ kind: match[2], value: match[3].trim() })) return index;
  }
  return -1;
}

function workflowJobStepOrderMatches(workflow, jobId, matchers) {
  let previousIndex = -1;
  for (const matcher of matchers) {
    const index = workflowJobStepIndex(workflow, jobId, matcher);
    if (index <= previousIndex) return false;
    previousIndex = index;
  }
  return true;
}

function workflowNamedStepUsesWithIncludes(workflow, stepName, usesPattern, expectedPatterns) {
  const lines = workflow.split(/\r?\n/);
  const patterns = Array.isArray(expectedPatterns) ? expectedPatterns : [expectedPatterns];
  for (let index = 0; index < lines.length; index += 1) {
    const match = lines[index].match(/^(\s*)-\s+name:\s*(.+?)\s*$/);
    if (!match || match[2] !== stepName) continue;
    const itemIndent = match[1];
    let usesMatches = false;
    for (let cursor = index + 1; cursor < lines.length; cursor += 1) {
      const line = lines[cursor];
      if (line.startsWith(itemIndent) && line.slice(itemIndent.length).startsWith("- ")) break;
      const uses = line.match(/^(\s*)uses:\s*(.+?)\s*$/);
      if (uses && uses[1].length === itemIndent.length + 2 && usesPattern.test(uses[2])) {
        usesMatches = true;
        break;
      }
    }
    if (!usesMatches) return false;
    const withLines = workflowStepDirectChildBlock(lines, index, itemIndent, "with").split(/\r?\n/);
    return patterns.every((pattern) => withLines.some((line) => pattern.test(line)));
  }
  return false;
}

function workflowNamedStepDirectChildBlockIncludes(workflow, stepName, key, expectedPattern) {
  const lines = workflow.split(/\r?\n/);
  for (let index = 0; index < lines.length; index += 1) {
    const match = lines[index].match(/^(\s*)-\s+name:\s*(.+?)\s*$/);
    if (!match || match[2] !== stepName) continue;
    return workflowStepDirectChildBlock(lines, index, match[1], key)
      .split(/\r?\n/)
      .some((line) => expectedPattern.test(line));
  }
  return false;
}

function workflowNamedStepDirectChildScalarValues(workflow, stepName, key) {
  const lines = workflow.split(/\r?\n/);
  for (let index = 0; index < lines.length; index += 1) {
    const match = lines[index].match(/^(\s*)-\s+name:\s*(.+?)\s*$/);
    if (!match || match[2] !== stepName) continue;
    const itemIndent = match[1];
    const expectedIndentLength = itemIndent.length + 2;
    const values = [];
    for (let cursor = index + 1; cursor < lines.length; cursor += 1) {
      const line = lines[cursor];
      if (line.startsWith(itemIndent) && line.slice(itemIndent.length).startsWith("- ")) break;
      const scalar = line.match(/^(\s*)([^:\s][^:]*):\s*(.*?)\s*$/);
      if (
        scalar
        && scalar[1].length === expectedIndentLength
        && scalar[2] === key
      ) {
        values.push(scalar[3].trim());
      }
    }
    return values;
  }
  return [];
}

function workflowNamedStepDirectChildScalarMatches(workflow, stepName, key, expectedPattern) {
  const values = workflowNamedStepDirectChildScalarValues(workflow, stepName, key);
  return values.length === 1 && expectedPattern.test(values[0]);
}

function workflowNamedStepDirectChildScalarValue(workflow, stepName, key) {
  const values = workflowNamedStepDirectChildScalarValues(workflow, stepName, key);
  if (values.length === 0) return null;
  if (values.length === 1) return values[0];
  return "__duplicate__";
}

function workflowJobNamedStepsUnique(workflow, jobId, stepNames) {
  const counts = new Map(stepNames.map((stepName) => [stepName, 0]));
  const lines = workflowJobBlock(workflow, jobId).split(/\r?\n/);
  for (const line of lines) {
    const match = line.match(/^\s*-\s+name:\s*(.+?)\s*$/);
    if (!match || !counts.has(match[1])) continue;
    counts.set(match[1], counts.get(match[1]) + 1);
  }
  return [...counts.values()].every((count) => count === 1);
}

function workflowNamedStepFailsClosed(workflow, stepName) {
  const continueOnError = workflowNamedStepDirectChildScalarValue(workflow, stepName, "continue-on-error");
  return continueOnError === null || /^false$/.test(continueOnError);
}

function workflowNamedStepsFailClosed(workflow, stepNames) {
  return stepNames.every((stepName) => workflowNamedStepFailsClosed(workflow, stepName));
}

function workflowNamedStepsUseBashPipefail(workflow, stepNames) {
  return stepNames.every((stepName) => workflowNamedStepRunBodyIncludes(
    workflow,
    stepName,
    /^[ \t]*set -euo pipefail\s*$/,
  ));
}

function workflowNamedStepsUseBashShell(workflow, stepNames) {
  return stepNames.every((stepName) => workflowNamedStepDirectChildScalarMatches(
    workflow,
    stepName,
    "shell",
    /^bash$/,
  ));
}

function workflowNamedStepRunBodyIncludes(workflow, stepName, expectedPattern) {
  const lines = workflow.split(/\r?\n/);
  for (let index = 0; index < lines.length; index += 1) {
    const match = lines[index].match(/^(\s*)-\s+name:\s*(.+?)\s*$/);
    if (!match || match[2] !== stepName) continue;
    const itemIndent = match[1];
    const expectedIndentLength = itemIndent.length + 2;
    const runBodies = [];
    let directScalarRun = false;
    for (let cursor = index + 1; cursor < lines.length; cursor += 1) {
      const line = lines[cursor];
      if (line.startsWith(itemIndent) && line.slice(itemIndent.length).startsWith("- ")) break;
      const scalarRun = line.match(/^(\s*)run:\s+(?!\|\s*$).+?\s*$/);
      if (scalarRun && scalarRun[1].length === expectedIndentLength) {
        directScalarRun = true;
      }
      const run = line.match(/^(\s*)run:\s*\|\s*$/);
      if (!run || run[1].length !== expectedIndentLength) continue;
      const body = [];
      for (let inner = cursor + 1; inner < lines.length; inner += 1) {
        const bodyLine = lines[inner];
        if (bodyLine.startsWith(itemIndent) && bodyLine.slice(itemIndent.length).startsWith("- ")) break;
        const next = bodyLine.match(/^(\s*)\S/);
        if (next && next[1].length <= expectedIndentLength) break;
        body.push(bodyLine);
      }
      runBodies.push(body);
    }
    return runBodies.length === 1
      && directScalarRun === false
      && runBodies[0].some((bodyLine) => expectedPattern.test(bodyLine));
  }
  return false;
}

function workflowNativeShellStepEnvIncludes(workflow, stepName, expectedPattern) {
  return workflowNamedStepDirectChildBlockIncludes(workflowJobBlock(workflow, "native-shell"), stepName, "env", expectedPattern);
}

function workflowNativeShellStepScalarMatches(workflow, stepName, key, expectedPattern) {
  return workflowNamedStepDirectChildScalarMatches(workflowJobBlock(workflow, "native-shell"), stepName, key, expectedPattern);
}

function workflowNativeShellStepRunBodyIncludes(workflow, stepName, expectedPattern) {
  return workflowNamedStepRunBodyIncludes(workflowJobBlock(workflow, "native-shell"), stepName, expectedPattern);
}

function workflowNativeShellStepUsesWithIncludes(workflow, stepName, usesPattern, expectedPatterns) {
  return workflowNamedStepUsesWithIncludes(workflowJobBlock(workflow, "native-shell"), stepName, usesPattern, expectedPatterns);
}

function workflowPlan50EvidenceStepUsesWithIncludes(workflow, stepName, usesPattern, expectedPatterns) {
  return workflowNamedStepUsesWithIncludes(workflowJobBlock(workflow, "plan50-evidence"), stepName, usesPattern, expectedPatterns);
}

function workflowPlan50EvidenceStepScalarMatches(workflow, stepName, key, expectedPattern) {
  return workflowNamedStepDirectChildScalarMatches(workflowJobBlock(workflow, "plan50-evidence"), stepName, key, expectedPattern);
}

function workflowPlan50EvidenceStepRunBodyIncludes(workflow, stepName, expectedPattern) {
  return workflowNamedStepRunBodyIncludes(workflowJobBlock(workflow, "plan50-evidence"), stepName, expectedPattern);
}

function workflowDefinesNativeShellNodeSetup(workflow) {
  return workflowJobStepUsesWithIncludes(
    workflow,
    "native-shell",
    /^actions\/setup-node@v4$/,
    /^[ \t]*node-version:\s*20\s*$/,
  );
}

function workflowDefinesNativeShellRustToolchainSetup(workflow) {
  return workflowJobStepIndex(
    workflow,
    "native-shell",
    (step) => step.kind === "uses" && /^dtolnay\/rust-toolchain@stable$/.test(step.value),
  ) >= 0;
}

function workflowDefinesNativeShellRustToolchainOrder(workflow) {
  return workflowJobStepOrderMatches(workflow, "native-shell", [
    (step) => step.kind === "uses" && /^dtolnay\/rust-toolchain@stable$/.test(step.value),
    (step) => step.kind === "name" && step.value === "Build native shell and server",
    (step) => step.kind === "name" && step.value === "Test native Rust workspace",
  ]);
}

function workflowDefinesLinuxSandboxDependencyInstall(workflow) {
  const stepName = "Install Linux sandbox dependencies";
  return workflowNativeShellStepScalarMatches(workflow, stepName, "if", /^runner\.os == 'Linux'$/)
    && workflowNativeShellStepScalarMatches(workflow, stepName, "run", /^sudo apt-get update && sudo apt-get install -y bubblewrap$/);
}

function workflowDefinesLinuxSandboxDependencyOrder(workflow) {
  return workflowJobStepOrderMatches(workflow, "native-shell", [
    (step) => step.kind === "name" && step.value === "Install Linux sandbox dependencies",
    (step) => step.kind === "name" && step.value === "Check Linux Bubblewrap host sandbox",
    (step) => step.kind === "name" && step.value === "Strict sandboxed background dogfood on Linux",
  ]);
}

function workflowDefinesPlan50EvidenceNodeSetup(workflow) {
  return workflowJobStepUsesWithIncludes(
    workflow,
    "plan50-evidence",
    /^actions\/setup-node@v4$/,
    /^[ \t]*node-version:\s*20\s*$/,
  );
}

function workflowDefinesPlan50EvidenceStepOrder(workflow) {
  return workflowJobStepOrderMatches(workflow, "plan50-evidence", [
    (step) => step.kind === "uses" && /^actions\/checkout@v4$/.test(step.value),
    (step) => step.kind === "uses" && /^actions\/setup-node@v4$/.test(step.value),
    (step) => step.kind === "name" && step.value === "Download Plan 50 native shell evidence artifacts",
    (step) => step.kind === "name" && step.value === "Verify Plan 50 evidence bundle",
    (step) => step.kind === "name" && step.value === "Require native shell matrix success",
  ]);
}

function workflowNativeShellStrategyDisablesFailFast(workflow) {
  return /^[ \t]*fail-fast:\s*false\s*$/m.test(workflowJobTopLevelKeyBlock(workflow, "native-shell", "strategy"));
}

function workflowCoversMultiOsDogfood(workflow) {
  return workflowRequiredJobIdsUnique(workflow)
    && workflowJobNamedStepsUnique(workflow, "native-shell", nativeShellCriticalStepNames)
    && workflowNativeShellMatrixIncludesAllOs(workflow)
    && workflowNativeShellRunsOnMatrixOs(workflow)
    && workflowNativeShellStrategyDisablesFailFast(workflow)
    && workflowDefinesPlan50PathFilters(workflow)
    && workflowDefinesNativeShellNodeSetup(workflow)
    && workflowDefinesNativeShellRustToolchainSetup(workflow)
    && workflowDefinesNativeShellRustToolchainOrder(workflow)
    && workflowDefinesPnpmEnable(workflow)
    && workflowDefinesLinuxSandboxDependencyInstall(workflow)
    && workflowDefinesLinuxSandboxDependencyOrder(workflow)
    && workflowDefinesWorkspaceDependencyInstall(workflow)
    && workflowDefinesNativeShellSetupOrder(workflow)
    && workflowDefinesNativeShellBinaryBuild(workflow)
    && workflowDefinesNativeShellBinaryBuildOrder(workflow)
    && workflowDefinesCliWrapperBuild(workflow)
    && workflowDefinesCliPackageTest(workflow)
    && workflowDefinesNativeNpmPackageTests(workflow)
    && workflowDefinesPlan50HelperTests(workflow)
    && workflowDefinesNativeShellValidationOrder(workflow)
    && workflowDefinesNativeShellCliSmokeAndDogfood(workflow)
    && workflowDefinesNativeShellCliBinaryTargets(workflow)
    && workflowDefinesStrictLinuxBackgroundDogfood(workflow)
    && workflowDefinesRustWorkspaceTest(workflow)
    && workflowDefinesLinuxHostSandboxCheck(workflow)
    && workflowDefinesTerminalCleanupChecks(workflow);
}

function manifestWriterDefinesEvidenceArtifacts(script) {
  return script.includes('const PLAN_ID = "50-standalone-oppi-finish-line"')
    && script.includes("schemaVersion: 1")
    && script.includes("runnerOs")
    && script.includes("matrixOs")
    && script.includes("strictBackgroundExpected")
    && script.includes("gitSha")
    && script.includes("githubRunId")
    && script.includes("githubRunAttempt")
    && script.includes("githubRefName")
    && script.includes('createHash("sha256")')
    && script.includes("fileSha256")
    && script.includes("plan50-native-shell-evidence-${runnerOs}.json");
}

function workflowDefinesEvidenceProducerTargets(workflow) {
  return workflowDefinesNativeShellCliSmokeAndDogfood(workflow)
    && workflowDefinesStrictLinuxBackgroundDogfood(workflow)
    && workflowDefinesLinuxHostSandboxCheck(workflow)
    && workflowDefinesTerminalCleanupChecks(workflow);
}

function workflowDefinesEvidenceArtifactStepOrder(workflow) {
  return workflowJobStepOrderMatches(workflow, "native-shell", [
    (step) => step.kind === "name" && step.value === "Prepare Plan 50 evidence folder",
    (step) => step.kind === "name" && step.value === "Check Linux Bubblewrap host sandbox",
    (step) => step.kind === "name" && step.value === "Check terminal cleanup lifecycle",
    (step) => step.kind === "name" && step.value === "Smoke native shell through CLI",
    (step) => step.kind === "name" && step.value === "Dogfood native shell through CLI",
    (step) => step.kind === "name" && step.value === "Strict sandboxed background dogfood on Linux",
    (step) => step.kind === "name" && step.value === "Write Plan 50 evidence manifest",
    (step) => step.kind === "name" && step.value === "Upload Plan 50 native shell evidence",
  ]);
}

function workflowDefinesEvidenceStepsFailClosed(workflow) {
  return workflowNamedStepsFailClosed(workflowJobBlock(workflow, "native-shell"), [
    "Prepare Plan 50 evidence folder",
    "Check Linux Bubblewrap host sandbox",
    "Check terminal cleanup lifecycle",
    "Smoke native shell through CLI",
    "Dogfood native shell through CLI",
    "Strict sandboxed background dogfood on Linux",
    "Write Plan 50 evidence manifest",
    "Upload Plan 50 native shell evidence",
  ]);
}

function workflowDefinesEvidencePipelinesFailClosed(workflow) {
  return workflowNamedStepsUseBashPipefail(workflowJobBlock(workflow, "native-shell"), [
    "Check Linux Bubblewrap host sandbox",
    "Check terminal cleanup lifecycle",
    "Smoke native shell through CLI",
    "Dogfood native shell through CLI",
    "Strict sandboxed background dogfood on Linux",
  ]);
}

function workflowDefinesEvidenceStepsUseBashShell(workflow) {
  return workflowNamedStepsUseBashShell(workflowJobBlock(workflow, "native-shell"), [
    "Prepare Plan 50 evidence folder",
    "Check Linux Bubblewrap host sandbox",
    "Check terminal cleanup lifecycle",
    "Smoke native shell through CLI",
    "Dogfood native shell through CLI",
    "Strict sandboxed background dogfood on Linux",
    "Write Plan 50 evidence manifest",
  ]);
}

function workflowDefinesEvidenceArtifacts(workflow, manifestWriter) {
  const prepareStepName = "Prepare Plan 50 evidence folder";
  const writeStepName = "Write Plan 50 evidence manifest";
  const uploadStepName = "Upload Plan 50 native shell evidence";
  return manifestWriterDefinesEvidenceArtifacts(manifestWriter)
    && workflowRequiredJobIdsUnique(workflow)
    && workflowJobNamedStepsUnique(workflow, "native-shell", nativeShellCriticalStepNames)
    && workflowDefinesEvidenceProducerTargets(workflow)
    && workflowDefinesEvidenceArtifactStepOrder(workflow)
    && workflowDefinesEvidenceStepsFailClosed(workflow)
    && workflowDefinesEvidencePipelinesFailClosed(workflow)
    && workflowDefinesEvidenceStepsUseBashShell(workflow)
    && workflowNativeShellStepScalarMatches(workflow, prepareStepName, "run", /^mkdir -p plan50-evidence$/)
    && workflowNativeShellStepScalarMatches(workflow, writeStepName, "if", /^always\(\)$/)
    && workflowNativeShellStepEnvIncludes(workflow, writeStepName, /^[ \t]*MATRIX_OS:\s*\$\{\{ matrix\.os \}\}\s*$/)
    && workflowNativeShellStepRunBodyIncludes(workflow, writeStepName, /^[ \t]*node scripts\/plan50-write-evidence-manifest\.mjs --evidence-dir plan50-evidence\s*$/)
    && workflowNativeShellStepScalarMatches(workflow, uploadStepName, "if", /^always\(\)$/)
    && workflowNativeShellStepUsesWithIncludes(workflow, uploadStepName, /^actions\/upload-artifact@v4$/, [
      /^[ \t]*name:\s*plan50-native-shell-evidence-\$\{\{ matrix\.os \}\}\s*$/,
      /^[ \t]*path:\s*plan50-evidence\/\*\*\s*$/,
      /^[ \t]*if-no-files-found:\s*error\s*$/,
      /^[ \t]*retention-days:\s*14\s*$/,
    ]);
}

function workflowDefinesVerifierStepsFailClosed(workflow) {
  return workflowNamedStepsFailClosed(workflowJobBlock(workflow, "plan50-evidence"), [
    "Download Plan 50 native shell evidence artifacts",
    "Verify Plan 50 evidence bundle",
    "Require native shell matrix success",
  ]);
}

function workflowDefinesEvidenceVerifier(workflow) {
  const downloadStepName = "Download Plan 50 native shell evidence artifacts";
  const verifyStepName = "Verify Plan 50 evidence bundle";
  const guardStepName = "Require native shell matrix success";
  return workflowRequiredJobIdsUnique(workflow)
    && workflowJobHasTopLevelScalar(workflow, "plan50-evidence", "name", "Plan 50 evidence bundle verifier")
    && workflowJobNamedStepsUnique(workflow, "plan50-evidence", plan50EvidenceCriticalStepNames)
    && workflowJobHasTopLevelScalar(workflow, "plan50-evidence", "runs-on", "ubuntu-latest")
    && workflowJobHasTopLevelScalar(workflow, "plan50-evidence", "needs", "native-shell")
    && workflowJobHasTopLevelScalar(workflow, "plan50-evidence", "if", "always()")
    && workflowDefinesPlan50EvidenceNodeSetup(workflow)
    && workflowDefinesPlan50EvidenceStepOrder(workflow)
    && workflowDefinesVerifierStepsFailClosed(workflow)
    && workflowPlan50EvidenceStepUsesWithIncludes(workflow, downloadStepName, /^actions\/download-artifact@v4$/, [
      /^[ \t]*pattern:\s*plan50-native-shell-evidence-\*\s*$/,
      /^[ \t]*path:\s*plan50-downloaded-evidence\s*$/,
      /^[ \t]*merge-multiple:\s*false\s*$/,
    ])
    && workflowPlan50EvidenceStepScalarMatches(
      workflow,
      verifyStepName,
      "run",
      /^node scripts\/plan50-evidence-verify\.mjs plan50-downloaded-evidence --json$/,
    )
    && workflowPlan50EvidenceStepScalarMatches(
      workflow,
      guardStepName,
      "if",
      /^\$\{\{ always\(\) && needs\.native-shell\.result != 'success' \}\}$/,
    )
    && workflowPlan50EvidenceStepRunBodyIncludes(
      workflow,
      guardStepName,
      /^[ \t]*echo "native-shell matrix result was \$\{\{ needs\.native-shell\.result \}\}"\s*$/,
    )
    && workflowPlan50EvidenceStepRunBodyIncludes(workflow, guardStepName, /^[ \t]*exit 1\s*$/);
}

function workflowJobDisablesCheckoutCredentialPersistence(workflow, jobId) {
  const lines = workflowJobBlock(workflow, jobId).split(/\r?\n/);
  let checkoutCount = 0;
  for (let index = 0; index < lines.length; index += 1) {
    const match = lines[index].match(/^(\s*)-\s+uses:\s*actions\/checkout@v4\s*$/);
    if (!match) continue;
    checkoutCount += 1;
    const withBlock = workflowStepDirectChildBlock(lines, index, match[1], "with");
    if (!/^\s*persist-credentials:\s*false\s*$/m.test(withBlock)) return false;
  }
  return checkoutCount > 0;
}

function workflowDisablesCheckoutCredentialPersistence(workflow) {
  return workflowJobDisablesCheckoutCredentialPersistence(workflow, "native-shell")
    && workflowJobDisablesCheckoutCredentialPersistence(workflow, "plan50-evidence");
}

function workflowDefinesLeastPrivilegePermissions(workflow) {
  const permissions = workflowTopLevelKeyBlock(workflow, "permissions");
  const lines = permissions
    .split(/\r?\n/)
    .filter((line) => line.trim().length > 0);
  if (lines[0] !== "permissions:") return false;
  const entries = lines.slice(1).map((line) => line.match(/^  ([^:\s][^:]*):\s*(.*?)\s*$/));
  return entries.length === 1
    && entries.every(Boolean)
    && entries[0][1] === "contents"
    && entries[0][2] === "read";
}

function workflowDefinesNoJobScopedPermissions(workflow) {
  const jobIds = workflowJobIds(workflow);
  return jobIds.length > 0
    && jobIds
    .every((jobId) => workflowJobTopLevelKeyBlock(workflow, jobId, "permissions") === "");
}

function workflowDefinesCiSafetyControls(workflow) {
  return workflowDefinesLeastPrivilegePermissions(workflow)
    && workflowDefinesNoJobScopedPermissions(workflow)
    && workflowJobHasTopLevelScalar(workflow, "native-shell", "timeout-minutes", "45")
    && workflowJobHasTopLevelScalar(workflow, "plan50-evidence", "timeout-minutes", "20")
    && workflowDisablesCheckoutCredentialPersistence(workflow);
}

function workflowDispatchDefined(workflow) {
  const onBlock = workflowTopLevelKeyBlock(workflow, "on");
  return /^[ \t]*workflow_dispatch:\s*$/m.test(onBlock);
}

function uncheckedPlanRows(plan) {
  return plan
    .split(/\r?\n/)
    .map((line, index) => ({ line: index + 1, text: line }))
    .filter((entry) => entry.text.includes("- [ ]"));
}

function normalizePlanRow(text) {
  return text
    .replace(/^- \[[ x]\]\s*/i, "")
    .replace(/[`*_]/g, "")
    .replace(/\s+/g, " ")
    .trim()
    .replace(/[.]+$/, "")
    .toLowerCase();
}

const PLAN50_BACKGROUND_LIFECYCLE_ROW = "/background list/read/kill is dogfooded through native shell";
const PLAN50_TERMINAL_RESTORE_ROW = "Terminal restore after abort/panic is checked on Windows and Unix";
const PLAN50_SANDBOXED_BACKGROUND_ROW = "Sandboxed background tasks instead of full-access/unrestricted background shell tasks";
const PLAN50_REQUIRED_ROWS = [
  PLAN50_BACKGROUND_LIFECYCLE_ROW,
  PLAN50_TERMINAL_RESTORE_ROW,
  PLAN50_SANDBOXED_BACKGROUND_ROW,
];
const LOCAL_WINDOWS_SANDBOX_ROUTE = {
  id: "local-windows-sandbox",
  completionScope: "partial-on-this-host",
  requiresAdditionalEvidence: [
    "Unix terminal restore evidence from WSL/Unix or CI.",
  ],
};
const MULTI_OS_CI_ROUTE = {
  id: "multi-os-ci-artifacts",
  completionScope: "all-remaining-rows",
  requiresAdditionalEvidence: [],
};

function evidenceBundleCovers(evidenceBundle, requiredRows) {
  if (!evidenceBundle?.ok) return false;
  const readyRows = evidenceBundle.payload?.planRowsReadyToCheck || [];
  return requiredRows.every((required) =>
    readyRows.some((ready) => normalizePlanRow(ready) === normalizePlanRow(required))
  );
}

function uniqueRows(rows) {
  const seen = new Set();
  const out = [];
  for (const row of rows) {
    const normalized = normalizePlanRow(row);
    if (seen.has(normalized)) continue;
    seen.add(normalized);
    out.push(row);
  }
  return out;
}

function applyVerifiedEvidenceRows(path, plan, rows) {
  const targets = rows.map((row) => ({ row, normalized: normalizePlanRow(row) }));
  const applied = [];
  const updated = plan.split(/\r?\n/).map((line) => {
    if (!line.includes("- [ ]")) return line;
    const normalized = normalizePlanRow(line);
    const match = targets.find((target) =>
      normalized === target.normalized
        || normalized.startsWith(`${target.normalized};`)
        || normalized.startsWith(`${target.normalized}.`)
    );
    if (!match) return line;
    const row = match.row;
    if (row && !applied.includes(row)) applied.push(row);
    return line.replace("- [ ]", "- [x]");
  }).join("\n");
  if (applied.length) writeFileSync(path, updated, "utf8");
  return applied;
}

function fileIncludes(path, pattern) {
  try {
    return readFileSync(path, "utf8").includes(pattern);
  } catch {
    return false;
  }
}

function windowsBackgroundAdapterImplemented() {
  return fileIncludes(windowsProcessPath, "spawn_windows_restricted_background_plan")
    && fileIncludes(windowsProcessPath, "WindowsRestrictedBackgroundProcess")
    && fileIncludes(sandboxLibPath, "spawn_windows_restricted_background_plan")
    && fileIncludes(sandboxLibPath, "windows_dedicated_sandbox_credentials_configured");
}

function verifyEvidenceBundle(evidenceRoot) {
  if (!evidenceRoot) return undefined;
  const resolvedRoot = resolve(evidenceRoot);
  const result = spawnSync(process.execPath, [join(repoRoot, "scripts", "plan50-evidence-verify.mjs"), resolvedRoot, "--json"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
    timeout: 30_000,
  });
  let payload;
  try {
    payload = JSON.parse(result.stdout);
  } catch {
    payload = undefined;
  }
  return {
    ok: result.status === 0 && payload?.ok === true,
    evidenceRoot: resolvedRoot,
    status: result.status,
    error: result.error?.message || result.stderr?.trim() || undefined,
    payload,
  };
}

function nextActions(checks, githubCli, inputStatus = ciEvidenceInputStatus(), context = {}) {
  const actions = [];
  const sandboxAdapterLocal = checks.find((check) => check.id === "sandbox-adapter-local-ready");
  const sandboxLocal = checks.find((check) => check.id === "sandboxed-background-local");
  const unixTerminal = checks.find((check) => check.id === "terminal-restore-unix-local");
  const unixRunnerAvailable = unixEvidenceRunnerAvailable();
  const localSandboxAvailable = sandboxReady();
  const localRouteNeedsUnixEvidence = context.localRouteNeedsUnixEvidence ?? (unixTerminal && !unixTerminal.ok);
  const localRouteCompletionScope = localRouteNeedsUnixEvidence
    ? LOCAL_WINDOWS_SANDBOX_ROUTE.completionScope
    : MULTI_OS_CI_ROUTE.completionScope;
  const localRouteRequiresAdditionalEvidence = localRouteNeedsUnixEvidence
    ? LOCAL_WINDOWS_SANDBOX_ROUTE.requiresAdditionalEvidence
    : [];
  const ciArtifacts = checks.find((check) => check.id === "multi-os-ci-evidence-artifacts-defined");
  if (process.platform === "win32" && sandboxAdapterLocal && !sandboxAdapterLocal.ok) {
    actions.push({
      id: "windows-sandbox-setup-dry-run",
      routeId: LOCAL_WINDOWS_SANDBOX_ROUTE.id,
      completionScope: localRouteCompletionScope,
      requiresAdditionalEvidence: localRouteRequiresAdditionalEvidence,
      command: "node packages/cli/dist/main.js sandbox setup-windows --dry-run --json",
      requiresExplicitUserApproval: false,
      dryRun: true,
      hostMutation: false,
      reason: "Previews the local sandbox account, WFP filter, and OPPI_WINDOWS_SANDBOX_* env changes without mutating the host.",
      verifyAfter: [
        "node packages/cli/dist/main.js sandbox status --json",
      ],
    });
    actions.push({
      id: "windows-sandbox-setup-explicit-approval",
      routeId: LOCAL_WINDOWS_SANDBOX_ROUTE.id,
      completionScope: localRouteCompletionScope,
      requiresAdditionalEvidence: localRouteRequiresAdditionalEvidence,
      command: "node packages/cli/dist/main.js sandbox setup-windows --yes --json",
      requiresExplicitUserApproval: true,
      requiresElevation: true,
      hostMutation: true,
      approvalPhrase: "approve Windows sandbox setup for Plan 50",
      reason: "Creates or updates the local OPPi Windows sandbox account, installs WFP filters, and persists OPPI_WINDOWS_SANDBOX_* env vars.",
      verifyAfter: [
        "node packages/cli/dist/main.js sandbox status --json",
        "node packages/cli/dist/main.js tui dogfood --mock --json --require-background-lifecycle",
      ],
    });
  }
  if (unixTerminal && !unixTerminal.ok) {
    if (ciArtifacts?.ok) {
      const ref = currentGitRef();
      const githubAuthAction = {
        id: "github-auth-preflight",
        routeId: MULTI_OS_CI_ROUTE.id,
        completionScope: MULTI_OS_CI_ROUTE.completionScope,
        requiresAdditionalEvidence: MULTI_OS_CI_ROUTE.requiresAdditionalEvidence,
        requiresNetwork: true,
        checksGitHubAuth: true,
        command: "gh auth status",
        requiresExplicitUserApproval: false,
        reason: "Verify GitHub CLI auth before triggering or downloading Plan 50 evidence artifacts.",
        status: githubCli?.status || "unknown",
        authenticated: githubCli?.authenticated === true,
        verifyAfter: [
          "gh auth login -h github.com",
        ],
      };
      const githubAuthRepairAction = githubCli?.authenticated === false
        ? {
            id: "github-auth-repair",
            routeId: MULTI_OS_CI_ROUTE.id,
            completionScope: MULTI_OS_CI_ROUTE.completionScope,
            requiresAdditionalEvidence: MULTI_OS_CI_ROUTE.requiresAdditionalEvidence,
            command: "gh auth login -h github.com",
            requiresExplicitUserApproval: true,
            approvalPhrase: "approve Plan 50 CI evidence route",
            requiresNetwork: true,
            githubAuthMutation: true,
            reason: "Repair GitHub CLI auth before pushing or triggering Plan 50 CI evidence collection.",
            preconditions: [
              "GitHub CLI auth status is invalid.",
              "The CI evidence route has explicit user approval.",
            ],
            verifyAfter: [
              "gh auth status",
            ],
          }
        : undefined;
      if (inputStatus.dirty) {
        actions.push({
          id: "publish-ci-evidence-inputs",
          routeId: MULTI_OS_CI_ROUTE.id,
          completionScope: MULTI_OS_CI_ROUTE.completionScope,
          requiresAdditionalEvidence: MULTI_OS_CI_ROUTE.requiresAdditionalEvidence,
          reviewOnly: true,
          remoteMutation: false,
          routeRequiresExplicitUserApproval: true,
          routeRequiresNetwork: true,
          routeRequiresGitHubAuth: true,
          routeRequiresCommitPush: true,
          routeLocalMutation: true,
          routeRemoteMutation: true,
          command: inputStatus.statusCommand,
          requiresExplicitUserApproval: false,
          routeApprovalPhrase: "approve Plan 50 CI evidence route",
          reason: `Review, stage, test, commit, repair GitHub auth if needed, and push relevant Plan 50 workflow/runtime changes before running GitHub CI; the remote workflow can only test committed and pushed content. Local relevant changes detected: ${inputStatus.count}.`,
          changes: inputStatus.changes,
          curatedPaths: inputStatus.curatedPaths,
          excludedDirtyPaths: inputStatus.excludedDirtyPaths,
          nonCuratedDirtyCount: inputStatus.nonCuratedDirtyCount,
          nonCuratedDirtySample: inputStatus.nonCuratedDirtySample,
          reviewDocument: ciEvidencePublishSetReviewPath,
          gitRef: inputStatus.gitRef,
          originUrl: inputStatus.originUrl,
          sample: inputStatus.sample,
        });
        actions.push({
          id: "stage-ci-evidence-inputs",
          routeId: MULTI_OS_CI_ROUTE.id,
          completionScope: MULTI_OS_CI_ROUTE.completionScope,
          requiresAdditionalEvidence: MULTI_OS_CI_ROUTE.requiresAdditionalEvidence,
          command: ciEvidenceGitAddCommand(inputStatus.stagePaths?.length ? inputStatus.stagePaths : inputStatus.curatedPaths),
          requiresExplicitUserApproval: true,
          approvalPhrase: "approve Plan 50 CI evidence route",
          localMutation: true,
          remoteMutation: false,
          reason: "Stage only the curated Plan 50 CI evidence publish set; avoid `git add -A` because the worktree contains broader non-Plan-50 changes.",
          curatedPaths: inputStatus.curatedPaths,
          stagePaths: inputStatus.stagePaths,
          verifyAfter: [
            ciEvidenceCachedDiffCommand(inputStatus.stagePaths?.length ? inputStatus.stagePaths : inputStatus.curatedPaths),
            ciEvidenceCachedDiffCheckCommand(inputStatus.stagePaths?.length ? inputStatus.stagePaths : inputStatus.curatedPaths),
          ],
        });
        actions.push({
          id: "verify-plan50-local-tests",
          routeId: MULTI_OS_CI_ROUTE.id,
          completionScope: MULTI_OS_CI_ROUTE.completionScope,
          requiresAdditionalEvidence: MULTI_OS_CI_ROUTE.requiresAdditionalEvidence,
          command: "pnpm run plan50:test",
          requiresExplicitUserApproval: false,
          remoteMutation: false,
          reason: "Run the local Plan 50 helper suite before committing the curated CI evidence publish set.",
        });
        actions.push({
          id: "commit-ci-evidence-inputs",
          routeId: MULTI_OS_CI_ROUTE.id,
          completionScope: MULTI_OS_CI_ROUTE.completionScope,
          requiresAdditionalEvidence: MULTI_OS_CI_ROUTE.requiresAdditionalEvidence,
          command: ciEvidenceCommitCommand(),
          requiresExplicitUserApproval: true,
          approvalPhrase: "approve Plan 50 CI evidence route",
          localMutation: true,
          remoteMutation: false,
          reason: "Commit the reviewed curated Plan 50 CI evidence publish set before asking GitHub Actions to verify it.",
          preconditions: [
            "Only curated Plan 50 paths are staged.",
            "Local Plan 50 tests pass.",
          ],
          verifyAfter: [
            "git status --short",
          ],
        });
        actions.push(githubAuthAction);
        if (githubAuthRepairAction) actions.push(githubAuthRepairAction);
        actions.push({
          id: "push-ci-evidence-inputs",
          routeId: MULTI_OS_CI_ROUTE.id,
          completionScope: MULTI_OS_CI_ROUTE.completionScope,
          requiresAdditionalEvidence: MULTI_OS_CI_ROUTE.requiresAdditionalEvidence,
          command: ciEvidencePushCommand(ref),
          requiresExplicitUserApproval: true,
          approvalPhrase: "approve Plan 50 CI evidence route",
          requiresNetwork: true,
          requiresGitHubAuth: true,
          requiresCommitPush: true,
          remoteMutation: true,
          reason: "Push the committed Plan 50 evidence route inputs so the remote native-shell workflow tests the actual content.",
          preconditions: [
            "Curated Plan 50 publish set is committed on the selected ref.",
            "GitHub CLI/auth can push to origin.",
          ],
          verifyAfter: [
            `git status --short -- ${ciEvidenceInputPaths.join(" ")}`,
          ],
        });
      } else {
        actions.push(githubAuthAction);
        if (githubAuthRepairAction) actions.push(githubAuthRepairAction);
      }
      actions.push({
        id: "github-ci-evidence-run",
        routeId: MULTI_OS_CI_ROUTE.id,
        completionScope: MULTI_OS_CI_ROUTE.completionScope,
        requiresAdditionalEvidence: MULTI_OS_CI_ROUTE.requiresAdditionalEvidence,
        availableOnThisHost: githubCiRunBlockedBy(inputStatus, githubCli).length === 0,
        blockedBy: githubCiRunBlockedBy(inputStatus, githubCli),
        requiresNetwork: true,
        requiresGitHubAuth: true,
        requiresCommitPush: true,
        remoteMutation: true,
        command: `gh workflow run native-shell.yml --ref ${ref}`,
        requiresExplicitUserApproval: true,
        approvalPhrase: "approve Plan 50 CI evidence route",
        reason: "Runs the native-shell GitHub Actions matrix so Plan 50 can collect Windows plus Unix terminal/background evidence.",
        preconditions: [
          "Relevant Plan 50 workflow/runtime changes are committed and pushed to the selected ref.",
          "GitHub CLI auth is valid for the repository.",
        ],
        verifyAfter: [
          `gh run list --workflow native-shell.yml --branch ${ref} --limit 1`,
          "gh run watch <run-id> --exit-status",
          "gh run download <run-id> --pattern plan50-native-shell-evidence-* --dir plan50-downloaded-evidence",
          "node scripts/plan50-evidence-verify.mjs plan50-downloaded-evidence --json",
          "node scripts/plan50-audit.mjs --evidence-root plan50-downloaded-evidence --apply-evidence --json",
        ],
      });
      actions.push({
        id: "verify-downloaded-ci-evidence",
        routeId: MULTI_OS_CI_ROUTE.id,
        completionScope: MULTI_OS_CI_ROUTE.completionScope,
        requiresAdditionalEvidence: MULTI_OS_CI_ROUTE.requiresAdditionalEvidence,
        availableOnThisHost: false,
        blockedBy: ["Downloaded Plan 50 evidence root is not supplied."],
        requiresInput: ["downloaded-plan50-evidence-root"],
        command: "node scripts/plan50-evidence-verify.mjs <downloaded-plan50-evidence-root> --json",
        requiresExplicitUserApproval: false,
        reason: "After a successful native-shell workflow run, download the plan50-native-shell-evidence-* artifacts and verify they prove strict Linux background lifecycle plus Windows/Unix terminal restore.",
      });
    }
    actions.push({
      id: "unix-terminal-restore-evidence",
      routeId: LOCAL_WINDOWS_SANDBOX_ROUTE.id,
      completionScope: localRouteCompletionScope,
      requiresAdditionalEvidence: localRouteRequiresAdditionalEvidence,
      availableOnThisHost: unixRunnerAvailable,
      requiresUnixRunner: !unixRunnerAvailable,
      blockedBy: unixEvidenceActionBlockedBy(unixRunnerAvailable),
      command: localTerminalCaptureCommand(),
      requiresExplicitUserApproval: false,
      reason: process.platform === "win32"
        ? "Run inside an installed WSL distribution or rely on a completed multi-OS CI run for Unix terminal restore evidence."
        : "Run on this Unix-like host to refresh terminal restore evidence.",
      evidenceRoot: defaultLocalTerminalEvidenceRoot(),
      verifyAfter: [
        `node scripts/plan50-audit.mjs --local-terminal-evidence-root ${defaultLocalTerminalEvidenceRoot()} --json`,
      ],
    });
  }
  if (sandboxLocal && !sandboxLocal.ok) {
    const backgroundEvidencePath = defaultLocalBackgroundEvidencePath();
    actions.push({
      id: "local-background-lifecycle-evidence",
      routeId: LOCAL_WINDOWS_SANDBOX_ROUTE.id,
      completionScope: localRouteCompletionScope,
      requiresAdditionalEvidence: localRouteRequiresAdditionalEvidence,
      availableOnThisHost: localSandboxAvailable,
      requiresSandboxSetup: !localSandboxAvailable,
      blockedBy: localBackgroundActionBlockedBy(localSandboxAvailable),
      command: "node packages/cli/dist/main.js tui dogfood --mock --json --require-background-lifecycle",
      captureCommand: localBackgroundCaptureCommand(backgroundEvidencePath),
      requiresExplicitUserApproval: false,
      reason: "After a supported sandbox adapter is configured, capture strict native-shell background lifecycle JSON so the audit can verify start/list/read/kill locally.",
      evidencePath: backgroundEvidencePath,
      verifyAfter: [
        `node scripts/plan50-audit.mjs --local-background-evidence ${backgroundEvidencePath} --json`,
      ],
    });
  }
  return actions;
}

function actionById(actions, id) {
  return actions.find((action) => action.id === id);
}

function actionCommand(action) {
  return action?.captureCommand || action?.command;
}

function uniqueNonEmpty(items) {
  return [...new Set(items.filter(Boolean))];
}

function checkOk(checks, id) {
  return checks.find((check) => check.id === id)?.ok === true;
}

function rowChecked(unchecked, row) {
  const normalized = normalizePlanRow(row);
  return !unchecked.some((entry) => normalizePlanRow(entry.text).startsWith(normalized));
}

function planLineForRow(plan, row) {
  const normalized = normalizePlanRow(row);
  const match = plan
    .split(/\r?\n/)
    .map((text, index) => ({ line: index + 1, text }))
    .find((entry) => normalizePlanRow(entry.text).startsWith(normalized));
  return match?.line;
}

function planRowDetails(plan, rows) {
  return (rows || []).map((row) => ({
    planLine: planLineForRow(plan, row) ?? null,
    planRow: row,
  }));
}

function samePlanRow(left, right) {
  return normalizePlanRow(left) === normalizePlanRow(right);
}

function nonApprovalActionAvailable(action) {
  return Boolean(
    action
      && action.requiresExplicitUserApproval !== true
      && action.availableOnThisHost !== false
      && (!Array.isArray(action.requiresInput) || action.requiresInput.length === 0)
      && (!Array.isArray(action.blockedBy) || action.blockedBy.length === 0),
  );
}

function localNonApprovalCloseoutState(successCriteria, actions) {
  const requiredActions = [];
  const blockedBy = [];
  const openCriteria = successCriteria
    .filter((criterion) => criterion.status === "open")
    .map((criterion) => criterion.id);

  const addRequirement = (criteriaIds, action, fallbackBlocker) => {
    if (!criteriaIds.some((id) => openCriteria.includes(id))) return;
    if (nonApprovalActionAvailable(action)) {
      requiredActions.push(action.id);
      return;
    }
    blockedBy.push(...(action?.blockedBy?.length ? action.blockedBy : [fallbackBlocker]));
  };

  addRequirement(
    ["background-native-lifecycle", "sandboxed-background-default-promotion"],
    actionById(actions, "local-background-lifecycle-evidence"),
    "Strict sandboxed background lifecycle evidence is not available without approved sandbox setup.",
  );
  addRequirement(
    ["terminal-restore-windows-unix"],
    actionById(actions, "unix-terminal-restore-evidence"),
    "Unix terminal restore evidence is not available on this host.",
  );

  return {
    available: openCriteria.length > 0 && blockedBy.length === 0 && requiredActions.length > 0,
    requiredActions: uniqueNonEmpty(requiredActions),
    blockedBy: uniqueNonEmpty(blockedBy),
  };
}

function closeoutChecklist(checks, plan, unchecked, actions, evidenceBundle) {
  const sandboxAdapterReady = checkOk(checks, "sandbox-adapter-local-ready");
  const sandboxEvidenceReady = checkOk(checks, "sandboxed-background-local");
  const unixTerminalReady = checkOk(checks, "terminal-restore-unix-local");
  const localBackgroundReady = checkOk(checks, "local-background-lifecycle-evidence");
  const evidenceRowsReady = evidenceBundle?.payload?.planRowsReadyToCheck || [];
  const evidenceReadyFor = (row) =>
    evidenceRowsReady.some((ready) => normalizePlanRow(ready) === normalizePlanRow(row));
  const statusForRow = (row, evidenceReady) => {
    if (planLineForRow(plan, row) === undefined) return "missing-from-plan";
    if (rowChecked(unchecked, row)) return "checked";
    return evidenceReady ? "evidence-ready" : "open";
  };

  const successCriteria = [
    {
      id: "background-native-lifecycle",
      planRow: PLAN50_BACKGROUND_LIFECYCLE_ROW,
      planLine: planLineForRow(plan, PLAN50_BACKGROUND_LIFECYCLE_ROW),
      status: statusForRow(
        PLAN50_BACKGROUND_LIFECYCLE_ROW,
        evidenceReadyFor(PLAN50_BACKGROUND_LIFECYCLE_ROW) || localBackgroundReady,
      ),
      evidenceRequired: "Native-shell dogfood must prove /background start/list/read/kill through a sandboxed background task, not merely the degraded sandbox-unavailable path.",
      acceptingEvidence: [
        "node packages/cli/dist/main.js tui dogfood --mock --json --require-background-lifecycle",
        `node scripts/plan50-audit.mjs --local-background-evidence ${defaultLocalBackgroundEvidencePath()} --json`,
        "or verified plan50-native-shell-evidence-* artifacts containing passing strict Linux dogfood JSON",
      ],
    },
    {
      id: "terminal-restore-windows-unix",
      planRow: PLAN50_TERMINAL_RESTORE_ROW,
      planLine: planLineForRow(plan, PLAN50_TERMINAL_RESTORE_ROW),
      status: statusForRow(
        PLAN50_TERMINAL_RESTORE_ROW,
        evidenceReadyFor(PLAN50_TERMINAL_RESTORE_ROW) || unixTerminalReady,
      ),
      evidenceRequired: "Terminal cleanup lifecycle and reset tests must pass on Windows and at least one Unix runner.",
      acceptingEvidence: [
        `node scripts/plan50-capture-local-terminal.mjs --output-dir ${defaultLocalTerminalEvidenceRoot()}`,
        "cargo test -p oppi-shell ratatui_lifecycle_exit_paths_share_cleanup_contract -- --nocapture",
        "cargo test -p oppi-shell ratatui_terminal_cleanup_sequence_resets_and_clears -- --nocapture",
        "plus WSL/Unix local evidence or verified multi-OS CI artifacts",
      ],
    },
    {
      id: "sandboxed-background-default-promotion",
      planRow: PLAN50_SANDBOXED_BACKGROUND_ROW,
      planLine: planLineForRow(plan, PLAN50_SANDBOXED_BACKGROUND_ROW),
      status: statusForRow(
        PLAN50_SANDBOXED_BACKGROUND_ROW,
        evidenceReadyFor(PLAN50_SANDBOXED_BACKGROUND_ROW) || localBackgroundReady,
      ),
      evidenceRequired: "Default promotion requires sandboxed background execution; unrestricted/full-access background shell tasks do not satisfy this row.",
      acceptingEvidence: [
        "passing local strict native-shell background lifecycle JSON",
        "configured Windows sandbox account/WFP with strict native dogfood",
        "or verified Linux CI strict background lifecycle evidence",
      ],
    },
  ];

  const windowsSetup = actionById(actions, "windows-sandbox-setup-explicit-approval");
  const windowsDryRun = actionById(actions, "windows-sandbox-setup-dry-run");
  const publishInputs = actionById(actions, "publish-ci-evidence-inputs");
  const stageInputs = actionById(actions, "stage-ci-evidence-inputs");
  const localTests = actionById(actions, "verify-plan50-local-tests");
  const commitInputs = actionById(actions, "commit-ci-evidence-inputs");
  const pushInputs = actionById(actions, "push-ci-evidence-inputs");
  const githubAuth = actionById(actions, "github-auth-preflight");
  const githubAuthRepair = actionById(actions, "github-auth-repair");
  const githubRun = actionById(actions, "github-ci-evidence-run");
  const verifyEvidence = actionById(actions, "verify-downloaded-ci-evidence");
  const localBackground = actionById(actions, "local-background-lifecycle-evidence");
  const unixEvidence = actionById(actions, "unix-terminal-restore-evidence");
  const githubRunVerifySteps = githubRun?.verifyAfter || [];
  const applyEvidenceStep = githubRunVerifySteps.find((step) => step.includes("--apply-evidence"));
  const preApplyGithubRunSteps = githubRunVerifySteps.filter((step) => step !== applyEvidenceStep);
  const specificVerifyStep = preApplyGithubRunSteps.find((step) => step.includes("plan50-evidence-verify.mjs"));
  const localBackgroundCommand = actionCommand(localBackground);
  const windowsSetupVerifySteps = (windowsSetup?.verifyAfter || [])
    .filter((step) => !localBackgroundCommand || step !== localBackground?.command);
  const localWindowsRouteNeedsUnixEvidence = !unixTerminalReady && !rowChecked(unchecked, PLAN50_TERMINAL_RESTORE_ROW);
  const localWindowsRouteCompletionScope = localWindowsRouteNeedsUnixEvidence
    ? LOCAL_WINDOWS_SANDBOX_ROUTE.completionScope
    : MULTI_OS_CI_ROUTE.completionScope;
  const localWindowsRouteAdditionalEvidence = localWindowsRouteNeedsUnixEvidence
    ? LOCAL_WINDOWS_SANDBOX_ROUTE.requiresAdditionalEvidence
    : [];
  const recommendedRouteId = process.platform === "win32"
    && localWindowsRouteCompletionScope === MULTI_OS_CI_ROUTE.completionScope
    ? LOCAL_WINDOWS_SANDBOX_ROUTE.id
    : MULTI_OS_CI_ROUTE.id;

  return {
    objective: "Close Plan 50 native/Rust standalone OPPi evidence gates.",
    readyToComplete: checks.every((check) => check.ok),
    recommendedRouteId,
    missing: successCriteria
      .filter((criterion) => criterion.status !== "checked")
      .map((criterion) => criterion.id),
    successCriteria,
    missingPlanRows: successCriteria
      .filter((criterion) => criterion.status === "missing-from-plan")
      .map((criterion) => criterion.planRow),
    localNonApprovalCloseout: localNonApprovalCloseoutState(successCriteria, actions),
    routes: [
      {
        id: "local-windows-sandbox",
        completionScope: localWindowsRouteCompletionScope,
        requiresAdditionalEvidence: localWindowsRouteAdditionalEvidence,
        availableOnThisHost: process.platform === "win32",
        requiresExplicitUserApproval: true,
        requiresElevation: true,
        hostMutation: true,
        approvalPhrase: "approve Windows sandbox setup for Plan 50",
        approvalGatedSteps: [
          "Open an elevated PowerShell in the repo.",
          "Run the Windows sandbox setup with --yes.",
          "Verify sandbox status before capturing strict background evidence.",
        ],
        blockedBy: [
          ...(sandboxAdapterReady ? [] : ["OPPI Windows sandbox account/WFP/env setup is not configured."]),
          ...(sandboxEvidenceReady ? [] : ["Strict sandboxed background lifecycle evidence is not captured."]),
          ...(unixTerminalReady ? [] : ["Unix terminal restore evidence is not captured on this host."]),
        ],
        steps: uniqueNonEmpty([
          windowsDryRun?.command,
          windowsSetup?.command,
          ...windowsSetupVerifySteps,
          localBackgroundCommand,
          ...(localBackground?.verifyAfter || []),
          unixEvidence?.command,
          ...(unixEvidence?.verifyAfter || []),
        ]),
      },
      {
        id: "multi-os-ci-artifacts",
        completionScope: MULTI_OS_CI_ROUTE.completionScope,
        requiresAdditionalEvidence: MULTI_OS_CI_ROUTE.requiresAdditionalEvidence,
        requiresExplicitUserApproval: Boolean(publishInputs || githubRun),
        requiresNetwork: true,
        requiresGitHubAuth: true,
        requiresCommitPush: true,
        localMutation: Boolean(stageInputs || commitInputs),
        githubAuthMutation: Boolean(githubAuthRepair),
        remoteMutation: true,
        approvalPhrase: "approve Plan 50 CI evidence route",
        approvalGatedSteps: [
          "Review the curated Plan 50 publish set.",
          "Stage only the curated Plan 50 publish set.",
          "Commit the curated Plan 50 publish set.",
          "Repair GitHub CLI auth if gh auth status is invalid.",
          "Push the selected ref before running GitHub Actions.",
        ],
        blockedBy: [
          ...(publishInputs ? ["Relevant Plan 50 workflow/runtime changes must be reviewed, staged, tested, committed, and pushed after GitHub auth is valid."] : []),
          ...(githubAuth?.authenticated === false ? ["GitHub CLI auth is not valid."] : []),
        ],
        excludedDirtyPaths: publishInputs?.excludedDirtyPaths || [],
        nonCuratedDirtyCount: publishInputs?.nonCuratedDirtyCount || 0,
        nonCuratedDirtySample: publishInputs?.nonCuratedDirtySample || [],
        curatedPublishSet: ciEvidenceInputPaths,
        reviewDocument: ciEvidencePublishSetReviewPath,
        steps: uniqueNonEmpty([
          publishInputs?.command,
          stageInputs?.command,
          ...(stageInputs?.verifyAfter || []),
          localTests?.command,
          commitInputs?.command,
          ...(commitInputs?.verifyAfter || []),
          githubAuth?.command,
          ...(githubAuth?.authenticated === false ? (githubAuth.verifyAfter || []) : []),
          githubAuthRepair?.command,
          ...(githubAuthRepair?.verifyAfter || []),
          pushInputs?.command,
          ...(pushInputs?.verifyAfter || []),
          githubRun?.command,
          ...preApplyGithubRunSteps,
          specificVerifyStep ? undefined : verifyEvidence?.command,
          applyEvidenceStep,
        ]),
      },
    ],
  };
}

function formatAuditSummary(payload) {
  const checklist = payload.closeoutChecklist || {};
  const lines = [
    `Plan 50 audit: ${payload.ok ? "passed" : "blocked"}`,
    `Objective: ${checklist.objective || "Close Plan 50 native/Rust standalone OPPi evidence gates."}`,
    `Plan file: ${payload.planPath} (${payload.planPathSource || "unknown"})`,
  ];

  const missing = checklist.missing || [];
  if (missing.length) lines.push(`Missing criteria: ${missing.join(", ")}`);

  const unchecked = payload.unchecked || [];
  if (unchecked.length) {
    lines.push("", `Open rows (${unchecked.length}):`);
    for (const row of unchecked) lines.push(`- line ${row.line}: ${row.text.replace(/^- \[ \]\s*/, "")}`);
  }

  const blockingChecks = (payload.checks || []).filter((check) => !check.ok);
  if (blockingChecks.length) {
    lines.push("", "Blocking checks:");
    for (const check of blockingChecks) lines.push(`- ${check.id}: ${check.evidence}`);
  }

  const recognizedEvidence = (payload.checks || []).filter((check) => check.ok && [
    "downloaded-ci-evidence-bundle",
    "local-background-lifecycle-evidence",
    "terminal-restore-local-platform",
    "terminal-restore-unix-local",
  ].includes(check.id));
  if (recognizedEvidence.length) {
    lines.push("", "Recognized evidence:");
    for (const check of recognizedEvidence) lines.push(`- ${check.id}: ${check.evidence}`);
  }

  if ((payload.unappliedPlanRowDetails || []).length) {
    lines.push("", "Unapplied accepted evidence rows:");
    for (const detail of payload.unappliedPlanRowDetails) {
      const location = detail.planLine == null ? "missing" : `line ${detail.planLine}`;
      lines.push(`- ${location}: ${detail.planRow}`);
    }
  }

  if (!payload.ok && checklist.localNonApprovalCloseout) {
    const localCloseout = checklist.localNonApprovalCloseout;
    lines.push("", `Local non-approval closeout: ${localCloseout.available ? "available" : "unavailable"}`);
    for (const action of localCloseout.requiredActions || []) lines.push(`  action: ${action}`);
    for (const blocker of localCloseout.blockedBy || []) lines.push(`  blocked: ${blocker}`);
  }

  const routes = checklist.routes || [];
  if (routes.length) {
    lines.push("", "Valid closeout routes:");
    for (const route of routes) {
      if (route.id === "local-windows-sandbox") {
        const partial = route.completionScope === "partial-on-this-host";
        lines.push(`- Local Windows sandbox${partial ? " (partial on this host)" : ""}: approve Windows sandbox setup for Plan 50, then run the setup/status/strict-dogfood evidence commands.`);
        if (route.id === checklist.recommendedRouteId) lines.push("  recommended: yes");
        if (partial) {
          lines.push("  note: still needs separate Unix terminal restore evidence from WSL/Unix or CI before Plan 50 can complete.");
        }
        if (route.requiresElevation) lines.push("  requires elevation: yes");
        if (route.hostMutation) lines.push("  mutates host: yes");
        for (const step of route.approvalGatedSteps || []) lines.push(`  gated step: ${step}`);
        lines.push("  exact user approval phrase: \"approve Windows sandbox setup for Plan 50\"");
      } else if (route.id === "multi-os-ci-artifacts") {
        lines.push("- CI artifact route: review/stage/test/commit the curated Plan 50 set, fix gh auth, push, run the workflow, download artifacts, verify, then apply evidence.");
        if (route.id === checklist.recommendedRouteId) lines.push("  recommended: yes");
        if (route.reviewDocument) lines.push(`  review doc: ${route.reviewDocument}`);
        if (route.requiresNetwork) lines.push("  requires network: yes");
      if (route.requiresGitHubAuth) lines.push("  requires GitHub auth: yes");
      if (route.requiresCommitPush) lines.push("  requires commit/push: yes");
      if (route.localMutation) lines.push("  mutates local repo: yes");
      if (route.githubAuthMutation) lines.push("  mutates GitHub auth: yes");
      if (route.remoteMutation) lines.push("  mutates remote: yes");
        for (const step of route.approvalGatedSteps || []) lines.push(`  gated step: ${step}`);
        lines.push("  exact user approval phrase: \"approve Plan 50 CI evidence route\"");
      } else {
        lines.push(`- ${route.id}`);
      }
      for (const blocker of route.blockedBy || []) lines.push(`  blocked: ${blocker}`);
    }
  }

  if (payload.githubCli?.authenticated === false) {
    lines.push("", "GitHub CLI auth is not valid; run: gh auth login -h github.com");
  }

  const actions = payload.nextActions || [];
  if (actions.length) {
    lines.push("", "Next commands:");
    for (const action of actions) lines.push(formatNextAction(action));
  }

  return `${lines.join("\n")}\n`;
}

function nextActionLabels(action) {
  return [
    action.dryRun ? "dry run" : undefined,
    action.reviewOnly ? "review only" : undefined,
    action.requiresExplicitUserApproval ? "approval required" : undefined,
    action.requiresElevation ? "requires elevation" : undefined,
    action.hostMutation ? "mutates host" : undefined,
    action.hostMutation === false ? "no host mutation" : undefined,
    action.localMutation ? "mutates local repo" : undefined,
    action.requiresNetwork ? "requires network" : undefined,
    action.requiresGitHubAuth ? "requires GitHub auth" : undefined,
    action.checksGitHubAuth ? "checks GitHub auth" : undefined,
    action.githubAuthMutation ? "mutates GitHub auth" : undefined,
    action.requiresCommitPush ? "requires commit/push" : undefined,
    action.remoteMutation ? "mutates remote" : undefined,
    action.remoteMutation === false ? "no remote mutation" : undefined,
    action.availableOnThisHost === false ? "unavailable here" : undefined,
    action.requiresUnixRunner ? "requires WSL/Unix or CI" : undefined,
    action.requiresSandboxSetup ? "requires sandbox setup" : undefined,
    Array.isArray(action.requiresInput) && action.requiresInput.length > 0 ? "requires input" : undefined,
  ].filter(Boolean);
}

function formatNextAction(action) {
  const labels = nextActionLabels(action);
  const suffix = labels.length ? ` [${labels.join(", ")}]` : "";
  const lines = [`- ${action.id}${suffix}: ${actionCommand(action)}`];
  if (action.reviewDocument) lines.push(`  review doc: ${action.reviewDocument}`);
  if (Array.isArray(action.excludedDirtyPaths) && action.excludedDirtyPaths.length > 0) {
    lines.push(`  warning: dirty sensitive paths outside curated Plan 50 publish set: ${action.excludedDirtyPaths.join(", ")}`);
  }
  if (Number(action.nonCuratedDirtyCount) > 0) {
    lines.push(`  warning: worktree has ${action.nonCuratedDirtyCount} dirty path(s) outside the curated Plan 50 publish set; use the exact stage command, not git add -A.`);
  }
  if (["stage-ci-evidence-inputs", "windows-sandbox-setup-dry-run", "github-auth-repair"].includes(action.id) && Array.isArray(action.verifyAfter)) {
    for (const step of action.verifyAfter) lines.push(`  verify: ${step}`);
  }
  return lines.join("\n");
}

function userApprovalState(ok, checklist) {
  const missingPlanRows = checklist.missingPlanRows || [];
  if (!ok && missingPlanRows.length > 0) {
    return {
      required: false,
      blocked: true,
      blockedBy: [
        `Required Plan 50 rows are missing from the plan: ${missingPlanRows.join("; ")}.`,
      ],
      options: [],
    };
  }
  const options = (checklist.routes || [])
    .filter((route) => route.requiresExplicitUserApproval && route.approvalPhrase)
    .map((route) => ({
      routeId: route.id,
      approvalPhrase: route.approvalPhrase,
      recommended: route.id === checklist.recommendedRouteId,
      completionScope: route.completionScope,
      requiresAdditionalEvidence: route.requiresAdditionalEvidence || [],
      requiresElevation: route.requiresElevation === true,
      hostMutation: route.hostMutation === true,
      localMutation: route.localMutation === true,
      requiresNetwork: route.requiresNetwork === true,
      requiresGitHubAuth: route.requiresGitHubAuth === true,
      requiresCommitPush: route.requiresCommitPush === true,
      githubAuthMutation: route.githubAuthMutation === true,
      remoteMutation: route.remoteMutation === true,
      approvalGatedSteps: route.approvalGatedSteps || [],
      blockedBy: route.blockedBy || [],
      excludedDirtyPaths: route.excludedDirtyPaths || [],
      nonCuratedDirtyCount: route.nonCuratedDirtyCount || 0,
      nonCuratedDirtySample: route.nonCuratedDirtySample || [],
      reviewDocument: route.reviewDocument,
      steps: route.steps || [],
    }));
  return {
    required: !ok && options.length > 0,
    blocked: false,
    blockedBy: [],
    options,
  };
}

function formatApprovalSummary(payload) {
  const approval = payload.userApproval || { required: false, options: [] };
  if (approval.blocked) {
    const lines = ["Plan 50 approval blocked:"];
    for (const blocker of approval.blockedBy || []) lines.push(`- ${blocker}`);
    return `${lines.join("\n")}\n`;
  }
  if (!approval.required) return "Plan 50 approval not required.\n";
  const lines = ["Plan 50 approval required:"];
  if (payload.planPath) lines.push(`Plan file: ${payload.planPath} (${payload.planPathSource || "unknown"})`);
  const recommended = (approval.options || []).find((option) => option.recommended);
  if (recommended) {
    lines.push(`Recommended route: ${recommended.approvalPhrase} (${recommended.completionScope || recommended.routeId})`);
  }
  for (const option of approval.options || []) {
    lines.push(`- ${option.approvalPhrase}`);
    lines.push(`  route: ${option.routeId}`);
    if (option.recommended) lines.push("  recommended: yes");
    if (option.completionScope) lines.push(`  scope: ${option.completionScope}`);
    if (option.requiresElevation) lines.push("  requires elevation: yes");
    if (option.hostMutation) lines.push("  mutates host: yes");
    if (option.localMutation) lines.push("  mutates local repo: yes");
    if (option.requiresNetwork) lines.push("  requires network: yes");
    if (option.requiresGitHubAuth) lines.push("  requires GitHub auth: yes");
    if (option.requiresCommitPush) lines.push("  requires commit/push: yes");
    if (option.githubAuthMutation) lines.push("  mutates GitHub auth: yes");
    if (option.remoteMutation) lines.push("  mutates remote: yes");
    if (option.reviewDocument) lines.push(`  review doc: ${option.reviewDocument}`);
    if (Array.isArray(option.excludedDirtyPaths) && option.excludedDirtyPaths.length > 0) {
      lines.push(`  warning: dirty sensitive paths outside curated Plan 50 publish set: ${option.excludedDirtyPaths.join(", ")}`);
    }
    if (Number(option.nonCuratedDirtyCount) > 0) {
      lines.push(`  warning: worktree has ${option.nonCuratedDirtyCount} dirty path(s) outside the curated Plan 50 publish set; use the exact stage command, not git add -A.`);
    }
    for (const step of option.approvalGatedSteps || []) lines.push(`  gated step: ${step}`);
    if (option.routeId === "local-windows-sandbox" && option.completionScope === "partial-on-this-host") {
      lines.push("  note: partial on this host; still needs Unix terminal restore evidence from WSL/Unix or CI.");
    }
    for (const blocker of option.blockedBy || []) lines.push(`  blocked: ${blocker}`);
  }
  return `${lines.join("\n")}\n`;
}

function main() {
  const planPathArg = argValue("--plan-path");
  const planPathEnv = process.env.OPPI_PLAN50_PLAN_PATH?.trim();
  const planPathSource = planPathArg ? "--plan-path" : planPathEnv ? "OPPI_PLAN50_PLAN_PATH" : "default";
  const planPath = resolve(planPathArg || planPathEnv || defaultPlanPath);
  const workflowPath = resolve(argValue("--workflow-path") || defaultWorkflowPath);
  const manifestWriterPath = resolve(argValue("--manifest-writer-path") || defaultManifestWriterPath);
  const evidenceRootArg = argValue("--evidence-root");
  const explicitLocalTerminalEvidenceRootArg = argValue("--local-terminal-evidence-root");
  const explicitLocalBackgroundEvidenceArg = argValue("--local-background-evidence");
  const applyEvidence = process.argv.includes("--apply-evidence");
  if (applyEvidence && !evidenceRootArg && !explicitLocalTerminalEvidenceRootArg && !explicitLocalBackgroundEvidenceArg) {
    console.error("--apply-evidence requires --evidence-root <downloaded-plan50-evidence-root> or local evidence arguments.");
    process.exit(2);
  }
  if (applyEvidence && planPathSource === "OPPI_PLAN50_PLAN_PATH") {
    console.error("--apply-evidence with OPPI_PLAN50_PLAN_PATH requires an explicit --plan-path so evidence cannot be applied to an env-selected fixture by accident.");
    process.exit(2);
  }
  const localTerminalEvidenceRootArg = explicitLocalTerminalEvidenceRootArg || (applyEvidence ? undefined : existingDefaultLocalTerminalEvidenceRoot());
  const localBackgroundEvidenceArg = explicitLocalBackgroundEvidenceArg || (applyEvidence ? undefined : existingDefaultLocalBackgroundEvidencePath());
  if (!existsSync(planPath)) {
    console.error(`Plan 50 not found: ${planPath}`);
    process.exit(2);
  }

  const workflow = existsSync(workflowPath) ? readFileSync(workflowPath, "utf8") : "";
  const manifestWriter = existsSync(manifestWriterPath) ? readFileSync(manifestWriterPath, "utf8") : "";
  const githubCli = githubCliStatus();
  const evidenceBundle = verifyEvidenceBundle(evidenceRootArg);
  const sandboxEvidenceReady = evidenceBundleCovers(evidenceBundle, [
    PLAN50_BACKGROUND_LIFECYCLE_ROW,
    PLAN50_SANDBOXED_BACKGROUND_ROW,
  ]);
  const terminalEvidenceReady = evidenceBundleCovers(evidenceBundle, [
    PLAN50_TERMINAL_RESTORE_ROW,
  ]);
  let plan = readFileSync(planPath, "utf8");
  const localWindowsUnixTerminalReady = localWindowsUnixTerminalEvidenceReady(localTerminalEvidenceRootArg);
  const localPlatformTerminalReady = terminalEvidenceReady || localWindowsUnixTerminalReady || localTerminalEvidenceReady(localTerminalEvidenceRootArg);
  const localBackgroundReady = localBackgroundEvidenceReady(localBackgroundEvidenceArg);
  const requiredPlanRowsMissing = PLAN50_REQUIRED_ROWS.filter((row) => planLineForRow(plan, row) === undefined);
  const localPlanRowsReadyToCheck = [
    ...(localBackgroundReady ? [
      PLAN50_BACKGROUND_LIFECYCLE_ROW,
      PLAN50_SANDBOXED_BACKGROUND_ROW,
    ] : []),
    ...(localWindowsUnixTerminalReady ? [
      PLAN50_TERMINAL_RESTORE_ROW,
    ] : []),
  ];
  const planRowsReadyToApply = uniqueRows([
    ...(evidenceBundle?.ok ? (evidenceBundle.payload?.planRowsReadyToCheck || []) : []),
    ...localPlanRowsReadyToCheck,
  ]);
  const localPlanRowsReadyToCheckDetails = planRowDetails(plan, localPlanRowsReadyToCheck);
  const planRowsReadyToApplyDetails = planRowDetails(plan, planRowsReadyToApply);
  const localSandboxReady = sandboxReady();
  const unixRunnerAvailable = unixEvidenceRunnerAvailable();
  const appliedPlanRows = applyEvidence && planRowsReadyToApply.length
    ? applyVerifiedEvidenceRows(planPath, plan, planRowsReadyToApply)
    : undefined;
  const appliedPlanRowDetails = appliedPlanRows ? planRowDetails(plan, appliedPlanRows) : undefined;
  const unappliedPlanRows = applyEvidence && planRowsReadyToApply.length
    ? uniqueRows(planRowsReadyToApply.filter((row) =>
        !(appliedPlanRows || []).some((applied) => samePlanRow(applied, row))
      ))
    : undefined;
  const unappliedPlanRowDetails = unappliedPlanRows ? planRowDetails(plan, unappliedPlanRows) : undefined;
  if (appliedPlanRows?.length) plan = readFileSync(planPath, "utf8");
  const unchecked = uncheckedPlanRows(plan);
  const checks = [
    {
      id: "live-provider-smoke",
      ok: providerConfigured(),
      evidence: `Requires OPPI_RUNTIME_WORKER_API_KEY_ENV pointing to an allowed configured API-key env, OPPI_OPENAI_API_KEY, OPENAI_API_KEY, or openai-codex OAuth in ${codexAuthPath()}.`,
    },
    {
      id: "sandbox-adapter-local-ready",
      ok: localSandboxReady || sandboxEvidenceReady || localBackgroundReady,
      evidence: localBackgroundReady
        ? "Local strict background lifecycle evidence proves a sandbox adapter was exercised."
        : sandboxEvidenceReady
          ? "Verified external CI evidence proves sandboxed background lifecycle; local adapter setup is not required for this audit run."
          : localSandboxReady
            ? (process.platform === "win32"
                ? "Local Windows sandbox env/WFP is configured."
                : process.platform === "linux"
                  ? "Local bubblewrap (`bwrap`) is available for OS-enforced sandbox dogfood."
                  : process.platform === "darwin"
                    ? "Local /usr/bin/sandbox-exec is available for sandbox dogfood."
                    : `Local sandbox adapter is ready on ${process.platform}.`)
            : process.platform === "win32"
              ? "Requires OPPI_WINDOWS_SANDBOX_USERNAME, OPPI_WINDOWS_SANDBOX_PASSWORD, and OPPI_WINDOWS_SANDBOX_WFP_READY=1."
              : process.platform === "linux"
                ? "Requires bubblewrap (`bwrap`) for OS-enforced sandbox dogfood."
                : process.platform === "darwin"
                  ? "Requires /usr/bin/sandbox-exec for sandbox dogfood."
                  : `Unsupported local platform: ${process.platform}.`,
    },
    {
      id: "sandboxed-background-local",
      ok: sandboxEvidenceReady || localBackgroundReady,
      evidence: localBackgroundReady
        ? `Captured local strict background lifecycle evidence passed from ${resolve(localBackgroundEvidenceArg)}.`
        : sandboxEvidenceReady
          ? "Verified external CI evidence proves strict sandboxed background lifecycle."
          : "Requires strict native-shell background lifecycle evidence via --local-background-evidence or verified external CI evidence.",
    },
    ...(localBackgroundEvidenceArg ? [
      {
        id: "local-background-lifecycle-evidence",
        ok: localBackgroundReady,
        evidence: localBackgroundReady
          ? `Local strict native-shell background lifecycle JSON proves started/list/read/kill: ${resolve(localBackgroundEvidenceArg)}.`
          : `Requires strict dogfood JSON with ok=true, strictBackgroundLifecycle=true, and background-sandbox-execution status proving started/list/read/kill: ${resolve(localBackgroundEvidenceArg)}.`,
      },
    ] : []),
    {
      id: "windows-background-adapter-implemented",
      ok: windowsBackgroundAdapterImplemented(),
      evidence: "Rust sandbox should include the Windows restricted-token background adapter path and dedicated sandbox-account detection, separate from local account/WFP setup.",
    },
    {
      id: "terminal-restore-local-platform",
      ok: localPlatformTerminalReady,
      evidence: localPlatformTerminalReady
        ? (terminalEvidenceReady
            ? "Verified external evidence includes terminal restore logs for Windows plus Unix runners."
            : localWindowsUnixTerminalReady
              ? `Captured local terminal cleanup evidence passed for Windows plus Unix from ${resolve(localTerminalEvidenceRootArg)}.`
            : `Captured local terminal cleanup evidence passed for ${localRunnerOs()} from ${resolve(localTerminalEvidenceRootArg)}.`)
        : `Requires captured terminal cleanup logs for ${localRunnerOs()} via --local-terminal-evidence-root, or verified external CI evidence.`,
    },
    {
      id: "terminal-restore-unix-local",
      ok: terminalEvidenceReady || localWindowsUnixTerminalReady,
      evidence: terminalEvidenceReady
        ? "Verified external evidence proves Windows plus Unix terminal restore."
        : localWindowsUnixTerminalReady
          ? `Captured local terminal cleanup evidence proves Windows plus Unix terminal restore from ${resolve(localTerminalEvidenceRootArg)}.`
        : unixRunnerAvailable
          ? "A Unix runner is available, but the audit still requires captured Windows plus Unix terminal cleanup evidence via --local-terminal-evidence-root or --evidence-root."
          : "Windows host needs an installed WSL distribution or verified external CI evidence to prove Unix terminal restore.",
    },
    {
      id: "multi-os-ci-dogfood-defined",
      ok: workflowCoversMultiOsDogfood(workflow),
      evidence: ".github/workflows/native-shell.yml should run native shell smoke/dogfood CLI steps against built native shell binary targets, include unique native-shell/plan50-evidence job ids, push/pull_request path filters for Rust/native/CLI/Plan 50 workflow inputs, native-shell Node setup, native-shell setup order, Rust toolchain setup before cargo build/test, pnpm dependency setup before install, Linux sandbox dependencies before Linux evidence, the native shell binary build step before smoke/dogfood evidence, the Rust workspace test step, CLI wrapper build before smoke/dogfood evidence, unique critical native-shell step names, and terminal cleanup checks on ubuntu-latest, macos-latest, and windows-latest in the native-shell matrix with native-shell runs-on bound directly to matrix.os within the native-shell job with matrix fail-fast disabled, plus CLI package tests, native npm package tests, Plan 50 helper tests before evidence, Linux Bubblewrap host-sandbox evidence, and strict Linux background lifecycle dogfood.",
    },
    {
      id: "multi-os-ci-evidence-artifacts-defined",
      ok: workflowDefinesEvidenceArtifacts(workflow, manifestWriter),
      evidence: ".github/workflows/native-shell.yml should prepare the evidence folder, then always write and upload downloadable Plan 50 evidence artifacts from the native-shell job with unique native-shell/plan50-evidence job ids, unique critical native-shell step names, direct shell: bash on bash-dependent evidence steps, no evidence step continue-on-error drift, bash pipefail on tee-backed evidence producers, RUNNER_OS-named terminal cleanup logs, smoke JSON, normal dogfood JSON, strict Linux background dogfood JSON, and call scripts/plan50-write-evidence-manifest.mjs to produce schemaVersion=1 manifests with matrix identity, GitHub run identity, SHA-256 file hashes, missing-artifact failure, and explicit artifact retention.",
    },
    {
      id: "multi-os-ci-evidence-verifier-defined",
      ok: workflowDefinesEvidenceVerifier(workflow),
      evidence: ".github/workflows/native-shell.yml should use the plan50-evidence job on ubuntu-latest with unique native-shell/plan50-evidence job ids to download all Plan 50 evidence artifacts into separate runner artifact folders with unique verifier step names, no verifier continue-on-error drift, execute scripts/plan50-evidence-verify.mjs against the combined bundle, and fail if the native-shell matrix result was not success.",
    },
    {
      id: "workflow-dispatch-defined",
      ok: workflowDispatchDefined(workflow),
      evidence: ".github/workflows/native-shell.yml should define workflow_dispatch so the audit's gh workflow run command can trigger evidence collection.",
    },
    {
      id: "workflow-ci-safety-controls-defined",
      ok: workflowDefinesCiSafetyControls(workflow),
      evidence: ".github/workflows/native-shell.yml should use least-privilege token permissions (`contents: read`), disable persisted checkout credentials, and set explicit job timeouts for the matrix and verifier jobs before its evidence artifacts are trusted.",
    },
    ...(evidenceBundle ? [
      {
        id: "downloaded-ci-evidence-bundle",
        ok: evidenceBundle.ok,
        evidence: evidenceBundle.ok
          ? `Verified external Plan 50 evidence bundle: ${evidenceBundle.evidenceRoot}.`
          : `External evidence bundle did not pass: ${evidenceBundle.error || evidenceBundle.evidenceRoot}.`,
      },
    ] : []),
    {
      id: "plan50-all-rows-checked",
      ok: unchecked.length === 0,
      evidence: unchecked.length ? `${unchecked.length} unchecked Plan 50 row(s) remain.` : "No unchecked Plan 50 rows remain.",
    },
    {
      id: "plan50-required-rows-present",
      ok: requiredPlanRowsMissing.length === 0,
      evidence: requiredPlanRowsMissing.length
        ? `Missing required Plan 50 row(s): ${requiredPlanRowsMissing.join("; ")}.`
        : "All required Plan 50 evidence rows are present in the plan.",
    },
  ];

  const ok = checks.every((check) => check.ok);
  const ciEvidenceInputs = ciEvidenceInputStatus();
  const actions = nextActions(checks, githubCli, ciEvidenceInputs, {
    localRouteNeedsUnixEvidence: !terminalEvidenceReady && !localWindowsUnixTerminalReady,
  });
  const checklist = closeoutChecklist(checks, plan, unchecked, actions, evidenceBundle);
  const userApproval = userApprovalState(ok, checklist);
  const payload = {
    ok,
    planPath,
    planPathSource,
    workflowPath,
    platform: process.platform,
    unchecked,
    checks,
    nextActions: actions,
    closeoutChecklist: checklist,
    userApproval,
    githubCli,
    ciEvidenceInputs,
    appliedPlanRows,
    appliedPlanRowDetails,
    unappliedPlanRows,
    unappliedPlanRowDetails,
    localPlanRowsReadyToCheck,
    localPlanRowsReadyToCheckDetails,
    planRowsReadyToApply,
    planRowsReadyToApplyDetails,
    evidenceBundle: evidenceBundle ? {
      ok: evidenceBundle.ok,
      evidenceRoot: evidenceBundle.evidenceRoot,
      status: evidenceBundle.status,
      checks: evidenceBundle.payload?.checks || [],
      planRowsReadyToCheck: evidenceBundle.payload?.planRowsReadyToCheck || [],
    } : undefined,
  };

  const json = process.argv.includes("--json");
  const summary = process.argv.includes("--summary");
  const approval = process.argv.includes("--approval");
  if (json) {
    console.log(JSON.stringify(payload, null, 2));
  } else if (summary) {
    process.stdout.write(formatAuditSummary(payload));
  } else if (approval) {
    process.stdout.write(formatApprovalSummary(payload));
  } else {
    console.log("Plan 50 audit");
    for (const check of checks) console.log(`${check.ok ? "pass" : "block"} ${check.id}: ${check.evidence}`);
    if (unchecked.length) {
      console.log("\nUnchecked rows:");
      for (const row of unchecked) console.log(`${row.line}: ${row.text}`);
    }
    if (actions.length) {
      console.log("\nNext actions:");
      for (const action of actions) console.log(`${action.id}: ${action.command}`);
    }
    if (evidenceBundle?.payload?.planRowsReadyToCheck?.length) {
      console.log("\nRows ready to check from evidence bundle:");
      for (const row of evidenceBundle.payload.planRowsReadyToCheck) console.log(`- ${row}`);
    }
  }
  process.exit(ok ? 0 : 1);
}

main();
