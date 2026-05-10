#!/usr/bin/env node
import { existsSync, lstatSync, readdirSync, readFileSync, statSync } from "node:fs";
import { createHash } from "node:crypto";
import { basename, dirname, join, relative, resolve, sep } from "node:path";

const args = process.argv.slice(2);
const rootArg = args.find((arg) => !arg.startsWith("-"));
const evidenceRoot = resolve(rootArg || "plan50-evidence");
const json = args.includes("--json");
const REQUIRED_RUNNERS = ["Linux", "Windows", "macOS"];
const REQUIRED_MATRIX_OS = {
  Linux: "ubuntu-latest",
  Windows: "windows-latest",
  macOS: "macos-latest",
};

function walk(dir) {
  if (!existsSync(dir)) return [];
  if (!lstatSync(dir).isDirectory()) return [];
  const out = [];
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    const stat = lstatSync(path);
    if (stat.isDirectory()) out.push(...walk(path));
    else out.push(path);
  }
  return out;
}

function readText(path) {
  try {
    return readFileSync(path, "utf8");
  } catch {
    return "";
  }
}

function readJson(path) {
  try {
    return JSON.parse(readText(path));
  } catch {
    return undefined;
  }
}

function sha256File(path) {
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}

function isRegularFile(path) {
  try {
    return lstatSync(path).isFile();
  } catch {
    return false;
  }
}

function isDirectory(path) {
  try {
    return lstatSync(path).isDirectory();
  } catch {
    return false;
  }
}

function validManifestFileName(name) {
  return typeof name === "string"
    && name.length > 0
    && name.trim() === name
    && !/[\x00-\x1f\x7f]/.test(name)
    && name !== "."
    && name !== ".."
    && !/^plan50-native-shell-evidence-.+\.json$/.test(name)
    && !/[\\/]/.test(name);
}

function validManifestFileList(files) {
  const sortedFiles = Array.isArray(files) ? [...files].sort() : [];
  return Array.isArray(files)
    && files.every((name) => validManifestFileName(name))
    && new Set(files).size === files.length
    && files.every((name, index) => name === sortedFiles[index]);
}

function manifestRecords(files) {
  return files
    .filter((file) => basename(file).startsWith("plan50-native-shell-evidence-") && basename(file).endsWith(".json"))
    .map((path) => ({ path, dir: dirname(path), payload: readJson(path) }))
    .filter((record) => record.payload?.plan === "50-standalone-oppi-finish-line" && record.payload?.runnerOs);
}

function manifestRunners(manifests) {
  return manifests
    .map((record) => record.payload.runnerOs)
    .filter(Boolean)
    .sort();
}

function manifestRunnersUnique(manifests) {
  const runners = manifests.map((record) => record.payload.runnerOs).filter(Boolean);
  return runners.length > 0 && new Set(runners).size === runners.length;
}

function manifestMetadataValid(manifests) {
  const allowedRunnerOs = new Set(REQUIRED_RUNNERS);
  return manifests.length > 0 && manifests.every((record) => {
    const runnerOs = record.payload.runnerOs;
    return record.payload.schemaVersion === 1
      && allowedRunnerOs.has(runnerOs)
      && basename(record.path) === `plan50-native-shell-evidence-${runnerOs}.json`
      && record.payload.matrixOs === REQUIRED_MATRIX_OS[runnerOs]
      && record.payload.strictBackgroundExpected === (runnerOs === "Linux")
      && validManifestFileList(record.payload.files);
  });
}

function manifestArtifactFoldersValid(manifests) {
  return manifests.length > 0 && manifests.every((record) => {
    const expectedMatrixOs = REQUIRED_MATRIX_OS[record.payload.runnerOs];
    return Boolean(expectedMatrixOs)
      && record.payload.matrixOs === expectedMatrixOs
      && basename(record.dir) === `plan50-native-shell-evidence-${expectedMatrixOs}`;
  });
}

function evidenceFilesScopedToManifests(files, manifests) {
  if (!manifests.length) return false;
  return files.every((file) => {
    const resolved = resolve(file);
    return manifests.some((record) => {
      const manifestDir = resolve(record.dir);
      return resolved === resolve(record.path) || resolved.startsWith(`${manifestDir}${sep}`);
    });
  });
}

function requiredManifestFiles(manifest) {
  const runnerOs = manifest.payload.runnerOs;
  const files = [
    `terminal-cleanup-lifecycle-${runnerOs}.log`,
    `terminal-cleanup-reset-${runnerOs}.log`,
    `tui-smoke-${runnerOs}.json`,
    `tui-dogfood-${runnerOs}.json`,
  ];
  if (runnerOs === "Linux") {
    files.push("linux-bubblewrap-host-sandbox-Linux.log");
    files.push("tui-dogfood-strict-Linux.json");
  }
  return files;
}

function manifestFilePath(manifests, runnerOs, name) {
  const manifest = manifests.find((record) => record.payload.runnerOs === runnerOs);
  if (!manifest || !Array.isArray(manifest.payload.files) || !manifest.payload.files.includes(name)) return undefined;
  const path = join(manifest.dir, name);
  return existsSync(path) ? path : undefined;
}

function manifestFileListsComplete(manifests) {
  return manifests.every((manifest) => {
    const listedFiles = Array.isArray(manifest.payload.files) ? manifest.payload.files : [];
    const listedFilesUnique = new Set(listedFiles).size === listedFiles.length;
    const listed = new Set(listedFiles);
    const actualArtifactFiles = walk(manifest.dir)
      .map((file) => relative(manifest.dir, file).replaceAll("\\", "/"))
      .filter((name) => name !== basename(manifest.path));
    const noUnlistedFiles = actualArtifactFiles.every((name) => listed.has(name));
    const listedFilesExist = listedFiles.every((name) => isRegularFile(join(manifest.dir, name)));
    const requiredFilesExist = requiredManifestFiles(manifest).every((name) => manifestFilePath(manifests, manifest.payload.runnerOs, name));
    return listedFilesUnique && noUnlistedFiles && listedFilesExist && requiredFilesExist;
  });
}

function manifestFileHashesValid(manifests) {
  return manifests.length > 0 && manifests.every((manifest) => {
    const listedFiles = Array.isArray(manifest.payload.files) ? manifest.payload.files : [];
    const hashes = manifest.payload.fileSha256;
    if (!hashes || typeof hashes !== "object" || Array.isArray(hashes)) return false;
    const hashNames = Object.keys(hashes);
    const listed = new Set(listedFiles);
    const hashKeysMatchManifest = hashNames.length === listedFiles.length && hashNames.every((name) => listed.has(name));
    const listedHashesMatch = listedFiles.every((name) => {
      const hash = hashes[name];
      const path = join(manifest.dir, name);
      return typeof hash === "string"
        && /^[a-f0-9]{64}$/i.test(hash)
        && isRegularFile(path)
        && sha256File(path) === hash.toLowerCase();
    });
    return hashKeysMatchManifest && listedHashesMatch;
  });
}

function validGithubRefName(value) {
  return typeof value === "string"
    && value.length > 0
    && value.trim() === value
    && !/[\r\n]/.test(value);
}

function validGitSha(value) {
  return typeof value === "string"
    && /^[a-f0-9]{40}$/i.test(value)
    && !/^0{40}$/.test(value);
}

function validPositiveDecimal(value) {
  return typeof value === "string" && /^[1-9]\d*$/.test(value);
}

function manifestRunIdentityConsistent(manifests) {
  if (!manifests.length) return false;
  const first = manifests[0].payload;
  const validIdentity = (payload) => validGitSha(payload.gitSha)
    && validPositiveDecimal(payload.githubRunId)
    && validPositiveDecimal(payload.githubRunAttempt)
    && validGithubRefName(payload.githubRefName);
  return validIdentity(first) && manifests.every((manifest) => {
    const payload = manifest.payload;
    return validIdentity(payload)
      && payload.gitSha === first.gitSha
      && payload.githubRunId === first.githubRunId
      && payload.githubRunAttempt === first.githubRunAttempt
      && payload.githubRefName === first.githubRefName;
  });
}

function logPassed(manifests, runnerOs, kind) {
  const path = manifestFilePath(manifests, runnerOs, `terminal-cleanup-${kind}-${runnerOs}.log`);
  const text = path ? readText(path) : "";
  const failedCounts = [...text.matchAll(/\b(\d+)\s+failed\b/gi)].map((match) => Number(match[1]));
  return Boolean(path)
    && /test result:\s+ok\./i.test(text)
    && failedCounts.length > 0
    && failedCounts.every((count) => count === 0)
    && !/test result:\s+failed\./i.test(text);
}

function terminalPairPassed(manifests, runnerOs) {
  return logPassed(manifests, runnerOs, "lifecycle") && logPassed(manifests, runnerOs, "reset");
}

function listedLogPassed(manifests, runnerOs, name) {
  const path = manifestFilePath(manifests, runnerOs, name);
  const text = path ? readText(path) : "";
  const failedCounts = [...text.matchAll(/\b(\d+)\s+failed\b/gi)].map((match) => Number(match[1]));
  return Boolean(path)
    && /test result:\s+ok\./i.test(text)
    && failedCounts.length > 0
    && failedCounts.every((count) => count === 0)
    && !/test result:\s+failed\./i.test(text);
}

function linuxHostSandboxPassed(manifests) {
  return listedLogPassed(manifests, "Linux", "linux-bubblewrap-host-sandbox-Linux.log");
}

function smokePassedForRunner(manifests, runnerOs) {
  const path = manifestFilePath(manifests, runnerOs, `tui-smoke-${runnerOs}.json`);
  const payload = path ? readJson(path) : undefined;
  return Boolean(path && payload?.ok === true);
}

function smokePassed(manifests) {
  const runners = manifestRunners(manifests);
  return runners.length > 0 && runners.every((runnerOs) => smokePassedForRunner(manifests, runnerOs));
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

function normalDogfoodPassedForRunner(manifests, runnerOs) {
  const path = manifestFilePath(manifests, runnerOs, `tui-dogfood-${runnerOs}.json`);
  const payload = path ? readJson(path) : undefined;
  return Boolean(
    path
      && payload?.ok === true
      && allScenariosPassed(payload)
      && backgroundScenario(payload)?.ok === true,
  );
}

function normalDogfoodPassed(manifests) {
  const runners = manifestRunners(manifests);
  return runners.length > 0 && runners.every((runnerOs) => normalDogfoodPassedForRunner(manifests, runnerOs));
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

function strictLinuxBackgroundPassed(manifests) {
  const path = manifestFilePath(manifests, "Linux", "tui-dogfood-strict-Linux.json");
  const payload = path ? readJson(path) : undefined;
  const scenario = backgroundScenario(payload);
  return Boolean(
    path
      && payload?.ok === true
      && payload?.strictBackgroundLifecycle === true
      && allScenariosPassed(payload)
      && scenario?.ok === true
      && backgroundLifecycleStatusPassed(scenario),
  );
}

function buildPayload() {
  const files = walk(evidenceRoot);
  const evidenceRootIsDirectory = isDirectory(evidenceRoot);
  const manifests = manifestRecords(files);
  const runners = manifestRunners(manifests);
  const requiredRunnersPresent = REQUIRED_RUNNERS.every((runnerOs) => runners.includes(runnerOs));
  const runnersUnique = manifestRunnersUnique(manifests);
  const metadataValid = manifestMetadataValid(manifests);
  const artifactFoldersValid = manifestArtifactFoldersValid(manifests);
  const scopedFiles = evidenceFilesScopedToManifests(files, manifests);
  const manifestFilesComplete = manifests.length > 0 && manifestFileListsComplete(manifests);
  const manifestFileHashes = manifestFileHashesValid(manifests);
  const manifestRunIdentity = manifestRunIdentityConsistent(manifests);
  const nativeShellSmoke = smokePassed(manifests);
  const normalDogfood = normalDogfoodPassed(manifests);
  const linuxHostSandbox = linuxHostSandboxPassed(manifests);
  const strictBackground = strictLinuxBackgroundPassed(manifests);
  const windowsTerminal = terminalPairPassed(manifests, "Windows");
  const unixTerminal = terminalPairPassed(manifests, "Linux") || terminalPairPassed(manifests, "macOS");
  const terminalRestore = windowsTerminal && unixTerminal;
  const checks = [
    {
      id: "evidence-root",
      ok: evidenceRootIsDirectory,
      evidence: evidenceRootIsDirectory
        ? evidenceRoot
        : `${evidenceRoot} is not a readable evidence directory.`,
    },
    {
      id: "manifests-present",
      ok: requiredRunnersPresent,
      evidence: runners.length
        ? `runner manifests: ${runners.join(", ")}; required: ${REQUIRED_RUNNERS.join(", ")}`
        : "no Plan 50 evidence manifests found",
    },
    {
      id: "manifest-runners-unique",
      ok: runnersUnique,
      evidence: runnersUnique
        ? "Each runner has exactly one Plan 50 evidence manifest."
        : "Each runner must have exactly one Plan 50 evidence manifest; duplicate runnerOs entries are ambiguous.",
    },
    {
      id: "manifest-metadata-valid",
      ok: metadataValid,
      evidence: metadataValid
        ? "Runner manifests use schemaVersion=1, known runnerOs labels, runner-matched manifest filenames, matching matrixOs labels, matching strictBackgroundExpected, and sorted unique trimmed printable basename-only file entries."
        : "Runner manifests must use schemaVersion=1, runnerOs in Windows/Linux/macOS, runner-matched manifest filenames, matching matrixOs labels, strictBackgroundExpected matching Linux only, and sorted unique trimmed printable basename-only file entries.",
    },
    {
      id: "manifest-artifact-folders-valid",
      ok: artifactFoldersValid,
      evidence: artifactFoldersValid
        ? "Runner manifests are stored under the downloaded artifact folder matching their matrix OS."
        : "Each runner manifest must live under its matching downloaded artifact folder: Linux=plan50-native-shell-evidence-ubuntu-latest, Windows=plan50-native-shell-evidence-windows-latest, macOS=plan50-native-shell-evidence-macos-latest.",
    },
    {
      id: "evidence-files-scoped-to-manifests",
      ok: scopedFiles,
      evidence: scopedFiles
        ? "Every file in the evidence root is inside a recognized runner artifact folder."
        : "Every file in the evidence root must be inside a recognized runner artifact folder so stray unmanifested files are rejected.",
    },
    {
      id: "manifest-file-lists-complete",
      ok: manifestFilesComplete,
      evidence: manifestFilesComplete
        ? "Runner manifests list every non-manifest artifact file, every required evidence file, and each listed file exists in the artifact folder."
        : "Each runner manifest must list every non-manifest artifact file, terminal cleanup logs, smoke JSON, normal dogfood JSON, Linux strict dogfood JSON when runnerOs=Linux, and Linux host-sandbox evidence when runnerOs=Linux.",
    },
    {
      id: "manifest-file-hashes-valid",
      ok: manifestFileHashes,
      evidence: manifestFileHashes
        ? "Runner manifests include a SHA-256 hash for every listed artifact file, and each hash matches the downloaded file contents."
        : "Each runner manifest must include fileSha256 entries for exactly the listed artifact files, and each SHA-256 must match the downloaded file contents.",
    },
    {
      id: "manifest-run-identity-consistent",
      ok: manifestRunIdentity,
      evidence: manifestRunIdentity
        ? "Runner manifests share the same nonzero Git commit, positive GitHub run id, positive run attempt, and ref name."
        : "Each runner manifest must include matching nonzero gitSha, positive githubRunId, positive githubRunAttempt, and githubRefName values so mixed-run or fabricated evidence bundles are rejected.",
    },
    {
      id: "normal-native-dogfood-passed",
      ok: normalDogfood,
      evidence: normalDogfood
        ? "Each runner's normal native-shell dogfood JSON reports ok=true, has all scenarios passing, and includes a passing background-sandbox-execution scenario."
        : "Each runner must include tui-dogfood-<runner>.json with ok=true, all scenarios passing, and a passing background-sandbox-execution scenario.",
    },
    {
      id: "native-shell-smoke-passed",
      ok: nativeShellSmoke,
      evidence: nativeShellSmoke
        ? "Each runner's native-shell smoke JSON is manifest-listed and reports ok=true."
        : "Each runner must include tui-smoke-<runner>.json with ok=true before the evidence bundle is accepted.",
    },
    {
      id: "linux-host-sandbox-passed",
      ok: linuxHostSandbox,
      evidence: linuxHostSandbox
        ? "Linux Bubblewrap host integration test log is manifest-listed and reports test result ok."
        : "Linux runner must include linux-bubblewrap-host-sandbox-Linux.log from the ignored Bubblewrap host integration test with test result ok.",
    },
    {
      id: "strict-linux-background-lifecycle",
      ok: strictBackground,
      evidence: "Requires tui-dogfood-strict-Linux.json with ok=true, strictBackgroundLifecycle=true, background-sandbox-execution.ok=true, and status proving started/list=true/read=true/kill=true.",
    },
    {
      id: "terminal-restore-windows-and-unix",
      ok: terminalRestore,
      evidence: `Windows=${windowsTerminal}; Unix=${unixTerminal}. Requires lifecycle and reset logs with test result ok.`,
    },
  ];
  const planRowsReadyToCheck = requiredRunnersPresent && runnersUnique && metadataValid && artifactFoldersValid && scopedFiles && manifestFilesComplete && manifestFileHashes && manifestRunIdentity && nativeShellSmoke && normalDogfood && linuxHostSandbox && strictBackground && terminalRestore
    ? [
        "/background list/read/kill is dogfooded through native shell",
        "Terminal restore after abort/panic is checked on Windows and Unix",
        "Sandboxed background tasks instead of full-access/unrestricted background shell tasks",
      ]
    : [];
  return {
    ok: checks.every((check) => check.ok),
    evidenceRoot,
    fileCount: files.length,
    runners,
    checks,
    planRowsReadyToCheck,
  };
}

const payload = buildPayload();
if (json) {
  console.log(JSON.stringify(payload, null, 2));
} else {
  console.log("Plan 50 evidence verification");
  for (const check of payload.checks) console.log(`${check.ok ? "pass" : "block"} ${check.id}: ${check.evidence}`);
  if (payload.planRowsReadyToCheck.length) {
    console.log("\nRows ready to check:");
    for (const row of payload.planRowsReadyToCheck) console.log(`- ${row}`);
  }
}
process.exit(payload.ok ? 0 : 1);
