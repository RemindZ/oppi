import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import ts from "typescript";

const require = createRequire(import.meta.url);

function typeboxStub() {
  const Type = new Proxy({}, {
    get: () => () => ({}),
  });
  return { Type };
}

function loadAskUserModule() {
  const sourcePath = fileURLToPath(new URL("../extensions/ask-user.ts", import.meta.url));
  const source = readFileSync(sourcePath, "utf8");
  const compiled = ts.transpileModule(source, {
    compilerOptions: {
      module: ts.ModuleKind.CommonJS,
      target: ts.ScriptTarget.ES2022,
      esModuleInterop: true,
    },
    fileName: sourcePath,
  }).outputText;
  const module = { exports: {} };
  const mockedRequire = (specifier) => {
    if (specifier === "@mariozechner/pi-coding-agent") {
      return { getAgentDir: () => join(process.cwd(), ".oppi", "agent") };
    }
    if (specifier === "@mariozechner/pi-tui") {
      return {
        Editor: class {},
        Key: { escape: "\u001b", ctrl: (key) => `ctrl:${key}` },
        matchesKey: () => false,
        Text: class {
          constructor(text) { this.text = text; }
        },
        truncateToWidth: (text) => text,
      };
    }
    if (specifier === "typebox") return typeboxStub();
    return require(specifier);
  };
  const fn = new Function("require", "module", "exports", compiled);
  fn(mockedRequire, module, module.exports);
  return module.exports;
}

test("ask_user deadline helpers add thirty seconds per submitted answer", () => {
  const askUser = loadAskUserModule();
  assert.equal(askUser.ASK_USER_ANSWER_EXTENSION_MS, 30_000);
  assert.equal(typeof askUser.extendAskUserDeadline, "function");
  assert.equal(askUser.extendAskUserDeadline(100_000), 130_000);
  assert.equal(askUser.extendAskUserDeadline(130_000), 160_000);
  assert.equal(askUser.askUserRemainingMs(160_000, 120_000), 40_000);
  assert.equal(askUser.askUserRemainingMs(160_000, 180_000), 0);
});
