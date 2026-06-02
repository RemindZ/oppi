import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(fileURLToPath(new URL("../../..", import.meta.url)));

function repoPath(path) {
  return resolve(repoRoot, path);
}

function source(path) {
  return readFileSync(repoPath(path), "utf8");
}

test("memory distiller keeps recurring task candidates separate from project memories", () => {
  const memory = source("packages/pi-package/extensions/memory.ts");
  assert.match(memory, /"recurringTasks"/, "distiller output shape should include recurringTasks");
  assert.match(memory, /oppi-recurring-task-candidate/, "recurring candidates should have a stable system-scope tag");
  assert.match(memory, /recurringTaskProjectRef/, "recurring candidates should use a separate project ref");
  assert.match(memory, /\.oppi", "system", "recurring-task-candidates"/, "recurring candidates should not be saved into the active project cwd");
  assert.match(memory, /ask the user before creating or updating any skill/, "skill creation must stay user-approved");

  const distillerDoc = source("systemprompts/memory/distiller-user-prompt.md");
  assert.match(distillerDoc, /recurringTasks/, "prompt artifact should document recurring task output");
  assert.match(distillerDoc, /future user approval/, "prompt artifact should preserve the approval gate");
});

test("memory cleanup protects retrieval-strengthened memories", () => {
  const memory = source("packages/pi-package/extensions/memory.ts");
  assert.match(memory, /function isRetrievalStrengthenedMemory/, "retrieval-strength helper should exist");
  assert.match(memory, /skippedRetrieval/, "cleanup should report retrieval-skipped records");
  assert.match(memory, /isRetrievalStrengthenedMemory\(entry\) && !consolidatedSources\.has\(id\)/, "deletion set should protect retrieved memories unless consolidated");
  assert.match(memory, /retrievalTelemetryRecord/, "maintenance prompt records should expose retrieval telemetry");
  assert.match(memory, /retrievalUseCount\(entry\) \* 80/, "dream duplicate keeper scoring should bias toward repeated retrieval");
  assert.match(memory, /isProtectedMemory\(entry\) \|\| isVerifiedMemory\(entry\) \|\| isRetrievalStrengthenedMemory\(entry\)/, "obvious-noise cleanup should not prune retrieval-strengthened records");

  const maintenanceDoc = source("systemprompts/memory/maintenance-system.md");
  const deepDoc = source("systemprompts/memory/deep-maintenance-system.md");
  assert.match(maintenanceDoc, /retrieval-strengthened/, "maintenance docs should explain retrieval protection");
  assert.match(deepDoc, /retrieval-strengthened/, "deep maintenance docs should explain retrieval protection");
});
