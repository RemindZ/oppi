#!/usr/bin/env node
import { chmodSync, copyFileSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

const __dirname = dirname(fileURLToPath(import.meta.url));
const packageRoot = resolve(__dirname, "..");

function findRepoRoot(start) {
  let current = start;
  for (;;) {
    if (existsSync(join(current, "Cargo.toml")) && existsSync(join(current, "crates"))) return current;
    const parent = dirname(current);
    if (parent === current) throw new Error(`Could not find OPPi repo root from ${start}`);
    current = parent;
  }
}

function executableName(name) {
  return process.platform === "win32" ? `${name}.exe` : name;
}

function copyBinary(repoRoot, outDir, name) {
  const exe = executableName(name);
  const source = join(repoRoot, "target", "release", exe);
  if (!existsSync(source)) throw new Error(`Missing ${source} after cargo build`);
  const destination = join(outDir, exe);
  copyFileSync(source, destination);
  try {
    chmodSync(destination, 0o755);
  } catch {
    // chmod is best-effort on Windows.
  }
  return destination;
}

if (process.env.OPPI_NATIVE_SKIP_CARGO_BUILD === "1") {
  console.log("Skipping Rust binary packaging because OPPI_NATIVE_SKIP_CARGO_BUILD=1.");
  process.exit(0);
}

const repoRoot = findRepoRoot(packageRoot);
const packageJson = JSON.parse(readFileSync(join(packageRoot, "package.json"), "utf8"));
const platformKey = `${process.platform}-${process.arch}`;
const outDir = join(packageRoot, "bin", platformKey);

const cargo = spawnSync("cargo", ["build", "--release", "-p", "oppi-server", "-p", "oppi-shell"], {
  cwd: repoRoot,
  stdio: "inherit",
  env: process.env,
});
if (cargo.error) throw cargo.error;
if (cargo.status !== 0) process.exit(cargo.status ?? 1);

rmSync(outDir, { recursive: true, force: true });
mkdirSync(outDir, { recursive: true });
const server = copyBinary(repoRoot, outDir, "oppi-server");
const shell = copyBinary(repoRoot, outDir, "oppi-shell");
writeFileSync(
  join(outDir, "manifest.json"),
  `${JSON.stringify(
    {
      packageName: packageJson.name,
      packageVersion: packageJson.version,
      platform: process.platform,
      arch: process.arch,
      binaries: [server, shell].map((path) => path.slice(outDir.length + 1)),
      builtAt: new Date().toISOString(),
    },
    null,
    2,
  )}\n`,
  "utf8",
);
console.log(`Packaged OPPi native binaries in ${outDir}`);
