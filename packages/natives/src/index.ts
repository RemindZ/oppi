import { spawnSync } from "node:child_process";
import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join, resolve } from "node:path";
import { performance } from "node:perf_hooks";
import { fileURLToPath } from "node:url";

const require = createRequire(import.meta.url);
const __dirname = dirname(fileURLToPath(import.meta.url));
const PACKAGE_NAME = "@oppiai/natives";
const PACKAGE_VERSION = "0.2.9";
const DEFAULT_IGNORED_DIRS = new Set([".git", "node_modules", "dist", ".oppi", ".oppi-local", ".wrangler", "output"]);

export type NativeModuleStatus = {
  available: boolean;
  candidatePaths: string[];
  modulePath?: string;
  error?: string;
};

export type NativeFallbackStatus = {
  search: "js-benchmark-only" | "native-available";
  pty: "deferred" | "native-available";
  clipboard: "deferred" | "native-available";
  sandbox: "deferred" | "native-available";
};

export type OppiNativeStatus = {
  packageName: typeof PACKAGE_NAME;
  packageVersion: string;
  platform: string;
  arch: string;
  native: NativeModuleStatus;
  fallbacks: NativeFallbackStatus;
  recommendations: string[];
};

export type SearchBenchmarkOptions = {
  root?: string;
  query?: string;
  maxFiles?: number;
  maxFileBytes?: number;
  timeoutMs?: number;
  includeExternalTools?: boolean;
};

export type BenchmarkRun = {
  name: string;
  available: boolean;
  elapsedMs?: number;
  fileCount?: number;
  matchCount?: number;
  exitCode?: number | null;
  error?: string;
};

export type SearchBenchmarkResult = {
  root: string;
  query: string;
  maxFiles: number;
  runs: BenchmarkRun[];
  recommendation: "defer-native" | "investigate-native-search";
  rationale: string;
};

function nativeCandidatePaths(env: Record<string, string | undefined> = process.env): string[] {
  const candidates = [
    env.OPPI_NATIVE_MODULE?.trim(),
    join(__dirname, "oppi_natives.node"),
    join(__dirname, "..", "prebuilds", `${process.platform}-${process.arch}`, "oppi_natives.node"),
  ].filter((value): value is string => Boolean(value));
  return [...new Set(candidates.map((candidate) => resolve(candidate)))];
}

function tryLoadNative(path: string): { ok: true } | { ok: false; error: string } {
  try {
    require(path);
    return { ok: true };
  } catch (error) {
    return { ok: false, error: error instanceof Error ? error.message : String(error) };
  }
}

export function getNativeStatus(options: { env?: Record<string, string | undefined> } = {}): OppiNativeStatus {
  const candidatePaths = nativeCandidatePaths(options.env);
  let firstError: string | undefined;
  let modulePath: string | undefined;

  for (const candidate of candidatePaths) {
    if (!existsSync(candidate)) continue;
    const loaded = tryLoadNative(candidate);
    if (loaded.ok) {
      modulePath = candidate;
      break;
    }
    firstError = loaded.error;
  }

  const nativeAvailable = Boolean(modulePath);
  return {
    packageName: PACKAGE_NAME,
    packageVersion: PACKAGE_VERSION,
    platform: process.platform,
    arch: process.arch,
    native: {
      available: nativeAvailable,
      candidatePaths,
      modulePath,
      error: nativeAvailable ? undefined : firstError ?? "No bundled native module found; using JS fallbacks and benchmarks only.",
    },
    fallbacks: {
      search: nativeAvailable ? "native-available" : "js-benchmark-only",
      pty: nativeAvailable ? "native-available" : "deferred",
      clipboard: nativeAvailable ? "native-available" : "deferred",
      sandbox: nativeAvailable ? "native-available" : "deferred",
    },
    recommendations: nativeAvailable
      ? ["Native module loaded. Keep JS fallbacks for install resilience and cross-platform smoke tests."]
      : [
        "Do not add Rust/N-API code until a benchmark or missing capability justifies it.",
        "Use `oppi natives benchmark --json` to gather repository-specific search evidence.",
      ],
  };
}

function shouldIgnoreDir(name: string): boolean {
  return DEFAULT_IGNORED_DIRS.has(name);
}

function walkFiles(root: string, maxFiles: number): string[] {
  const files: string[] = [];
  const stack = [root];
  while (stack.length > 0 && files.length < maxFiles) {
    const dir = stack.pop() as string;
    let entries: any[];
    try {
      entries = readdirSync(dir, { withFileTypes: true });
    } catch {
      continue;
    }

    for (const entry of entries) {
      const fullPath = join(dir, entry.name);
      if (entry.isDirectory?.()) {
        if (!shouldIgnoreDir(entry.name)) stack.push(fullPath);
        continue;
      }
      if (entry.isFile?.()) {
        files.push(fullPath);
        if (files.length >= maxFiles) break;
      }
    }
  }
  return files;
}

function runJsSearch(root: string, query: string, maxFiles: number, maxFileBytes: number): BenchmarkRun {
  const started = performance.now();
  const files = walkFiles(root, maxFiles);
  let matchCount = 0;

  for (const file of files) {
    try {
      const stat = statSync(file);
      if (!stat.isFile() || stat.size > maxFileBytes) continue;
      const text = readFileSync(file, { encoding: "utf8" });
      if (typeof text === "string" && text.includes(query)) matchCount += 1;
    } catch {
      // Ignore unreadable/binary/transient files; this benchmark is a native-decision signal, not a production search API.
    }
  }

  return {
    name: "js-recursive-fallback",
    available: true,
    elapsedMs: Math.round((performance.now() - started) * 100) / 100,
    fileCount: files.length,
    matchCount,
  };
}

function runRipgrep(root: string, query: string, timeoutMs: number): BenchmarkRun {
  const started = performance.now();
  const result = spawnSync("rg", ["--fixed-strings", "--files-with-matches", "--hidden", "--glob", "!.git/**", "--glob", "!node_modules/**", "--glob", "!dist/**", query, root], {
    encoding: "utf8",
    timeout: timeoutMs,
    maxBuffer: 1024 * 1024,
  });
  const elapsedMs = Math.round((performance.now() - started) * 100) / 100;
  if (result.error) {
    return { name: "rg-external-native", available: false, elapsedMs, error: result.error.message };
  }
  const stdout = typeof result.stdout === "string" ? result.stdout.trim() : "";
  return {
    name: "rg-external-native",
    available: true,
    elapsedMs,
    matchCount: stdout ? stdout.split(/\r?\n/).filter(Boolean).length : 0,
    exitCode: result.status,
    error: result.status && result.status > 1 ? String(result.stderr || `rg exited ${result.status}`) : undefined,
  };
}

function decideSearchRecommendation(runs: BenchmarkRun[]): Pick<SearchBenchmarkResult, "recommendation" | "rationale"> {
  const js = runs.find((run) => run.name === "js-recursive-fallback" && run.available && typeof run.elapsedMs === "number");
  const rg = runs.find((run) => run.name === "rg-external-native" && run.available && typeof run.elapsedMs === "number" && !run.error);
  if (!js) return { recommendation: "defer-native", rationale: "No JS baseline was collected, so native work is not justified yet." };
  if (!rg) return { recommendation: "defer-native", rationale: "No external native search baseline was available; keep using existing shell/search tools and JS fallbacks." };

  const speedup = (js.elapsedMs as number) / Math.max(rg.elapsedMs as number, 1);
  if ((js.elapsedMs as number) >= 100 && speedup >= 2) {
    return {
      recommendation: "investigate-native-search",
      rationale: `External native search was ${speedup.toFixed(1)}x faster than the JS fallback on this repository. A Rust search helper may be worth a focused design spike.`,
    };
  }
  return {
    recommendation: "defer-native",
    rationale: `Search did not clear the native threshold (JS ${js.elapsedMs}ms vs rg ${rg.elapsedMs}ms). Do not add Rust search yet.`,
  };
}

export async function benchmarkSearch(options: SearchBenchmarkOptions = {}): Promise<SearchBenchmarkResult> {
  const root = resolve(options.root ?? process.cwd());
  const query = options.query ?? "oppi";
  const maxFiles = options.maxFiles ?? 2_000;
  const maxFileBytes = options.maxFileBytes ?? 512 * 1024;
  const timeoutMs = options.timeoutMs ?? 5_000;
  const includeExternalTools = options.includeExternalTools ?? true;
  const runs = [runJsSearch(root, query, maxFiles, maxFileBytes)];
  if (includeExternalTools) runs.push(runRipgrep(root, query, timeoutMs));
  const decision = decideSearchRecommendation(runs);
  return { root, query, maxFiles, runs, ...decision };
}
