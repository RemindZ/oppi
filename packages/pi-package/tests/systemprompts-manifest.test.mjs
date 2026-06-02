import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { resolve } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(fileURLToPath(new URL("../../..", import.meta.url)));

function repoPath(path) {
  return resolve(repoRoot, path);
}

test("system prompt manifest points at existing runtime prompt artifacts", () => {
  const manifestPath = repoPath("systemprompts/manifest.json");
  const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
  for (const surface of manifest.activePromptSurfaces) {
    assert.ok(existsSync(repoPath(surface.path)), `missing prompt surface ${surface.id}: ${surface.path}`);
    assert.ok(surface.runtimeSource, `prompt surface ${surface.id} should name its runtime source`);
  }
});

test("prompt variants carry every runtime surface used by OPPi", () => {
  const manifest = JSON.parse(readFileSync(repoPath("systemprompts/manifest.json"), "utf8"));
  const expectedSurfaces = [
    "main-system-append.md",
    "oppi-feature-routing-system-append.md",
    "review-system-append.md",
    "review-audit-system-append.md",
    "permissions-auto-review-system.md",
    "permissions-auto-review-user-append.md",
    "image-gen-codex-native-adapter-instructions.md",
    "memory-distiller-user-prompt.md",
    "memory-maintenance-system.md",
    "memory-deep-maintenance-system.md",
    "init-user-prompt-append.md",
    "independent-user-prompt-append.md",
    "feedback-triage-user-prompt-append.md",
    "smart-compact-user-prompt-append.md",
    "follow-up-chain-system-append.md",
  ];

  for (const variant of manifest.variants) {
    assert.deepEqual(variant.runtimeSurfaces, expectedSurfaces, `${variant.id} runtime surface list drifted`);
    for (const surface of variant.runtimeSurfaces) {
      assert.ok(
        existsSync(repoPath(`systemprompts/experiments/${variant.id}/${surface}`)),
        `missing ${variant.id} surface ${surface}`,
      );
    }
  }
});
