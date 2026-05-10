import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { benchmarkSearch, getNativeStatus } from "./index.js";

function tempDir(name: string): string {
  return mkdtempSync(join(tmpdir(), `oppi-natives-${name}-`));
}

test("getNativeStatus degrades gracefully when no native module is bundled", () => {
  const status = getNativeStatus({ env: {} });
  assert.equal(status.packageName, "@oppiai/natives");
  assert.equal(status.native.available, false);
  assert.equal(status.fallbacks.search, "js-benchmark-only");
  assert.match(status.recommendations.join("\n"), /benchmark/);
});

test("benchmarkSearch collects a bounded JS fallback baseline", async () => {
  const root = tempDir("search");
  mkdirSync(join(root, "src"), { recursive: true });
  writeFileSync(join(root, "src", "a.txt"), "hello oppi\n", "utf8");
  writeFileSync(join(root, "src", "b.txt"), "nothing to see\n", "utf8");

  const result = await benchmarkSearch({ root, query: "oppi", maxFiles: 10, includeExternalTools: false });
  assert.equal(result.root, root);
  assert.equal(result.query, "oppi");
  assert.equal(result.recommendation, "defer-native");
  assert.equal(result.runs[0].name, "js-recursive-fallback");
  assert.equal(result.runs[0].available, true);
  assert.equal(result.runs[0].fileCount, 2);
  assert.equal(result.runs[0].matchCount, 1);
});
