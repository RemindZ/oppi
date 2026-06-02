#!/usr/bin/env node
import { spawnSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { verifyNativePackageFileList } from "../dist/main.js";

const __dirname = dirname(fileURLToPath(import.meta.url));
const packageRoot = resolve(__dirname, "..");

function optionValue(args, name) {
  const index = args.indexOf(name);
  if (index >= 0) return args[index + 1];
  const prefix = `${name}=`;
  const found = args.find((arg) => arg.startsWith(prefix));
  return found ? found.slice(prefix.length) : undefined;
}

function extractPackJson(stdout) {
  const text = String(stdout ?? "").trim();
  if (!text) throw new Error("npm pack --json produced no output");
  try {
    return JSON.parse(text);
  } catch {
    const start = text.indexOf("[");
    const end = text.lastIndexOf("]");
    if (start >= 0 && end > start) return JSON.parse(text.slice(start, end + 1));
    throw new Error(`could not parse npm pack JSON output: ${text.slice(0, 500)}`);
  }
}

function npmPackDryRun() {
  const npmArgs = ["pack", "--dry-run", "--json", "--ignore-scripts"];
  const command = process.platform === "win32" ? "cmd.exe" : "npm";
  const args = process.platform === "win32" ? ["/d", "/s", "/c", "npm", ...npmArgs] : npmArgs;
  return spawnSync(
    command,
    args,
    {
      cwd: packageRoot,
      encoding: "utf8",
      maxBuffer: 10 * 1024 * 1024,
    },
  );
}

function readPackRecord(args) {
  const packJsonPath = optionValue(args, "--pack-json");
  if (packJsonPath) {
    const resolved = resolve(process.cwd(), packJsonPath);
    if (!existsSync(resolved)) throw new Error(`--pack-json file does not exist: ${resolved}`);
    const parsed = JSON.parse(readFileSync(resolved, "utf8"));
    return Array.isArray(parsed) ? parsed[0] : parsed;
  }

  const result = npmPackDryRun();
  if (result.error) throw result.error;
  if (result.status !== 0) {
    throw new Error(String(result.stderr || result.stdout || `npm pack exited ${result.status}`));
  }
  const parsed = extractPackJson(result.stdout);
  if (!Array.isArray(parsed) || !parsed[0]) throw new Error("npm pack JSON did not contain a package record");
  return parsed[0];
}

function printText(report, packRecord) {
  const key = `${report.platform}-${report.arch}`;
  if (report.ok) {
    console.log(`@oppiai/native pack includes required ${key} artifacts.`);
    console.log(`files checked: ${report.files.length}`);
    console.log(`tarball: ${packRecord.filename ?? "(dry-run)"}`);
    return;
  }
  console.error(`@oppiai/native pack is missing required ${key} artifacts:`);
  for (const file of report.missingFiles) console.error(`- ${file}`);
}

try {
  const args = process.argv.slice(2);
  const json = args.includes("--json");
  const platform = optionValue(args, "--platform") ?? process.platform;
  const arch = optionValue(args, "--arch") ?? process.arch;
  const packRecord = readPackRecord(args);
  const report = verifyNativePackageFileList(packRecord.files ?? [], { platform, arch });
  if (json) {
    console.log(JSON.stringify({ ...report, package: packRecord.name, version: packRecord.version }, null, 2));
  } else {
    printText(report, packRecord);
  }
  process.exitCode = report.ok ? 0 : 1;
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error));
  process.exitCode = 1;
}
