import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { readFileSync } from "node:fs";
import test from "node:test";
import { fileURLToPath } from "node:url";
import ts from "typescript";

const require = createRequire(import.meta.url);

function loadUsageModule() {
  const sourcePath = fileURLToPath(new URL("../extensions/usage.ts", import.meta.url));
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
      return {
        DynamicBorder: class {},
        getAgentDir: () => process.cwd(),
      };
    }
    if (specifier === "@mariozechner/pi-tui") {
      return {
        Container: class {},
        Text: class {},
        matchesKey: () => false,
        truncateToWidth: (text) => text,
        visibleWidth: (text) => String(text).replace(/\x1b\[[0-9;]*m/g, "").length,
      };
    }
    if (specifier === "./background-tasks") {
      return { BACKGROUND_TASKS_SHORTCUT_LABEL: "Ctrl+Alt+T" };
    }
    return require(specifier);
  };
  const fn = new Function("require", "module", "exports", compiled);
  fn(mockedRequire, module, module.exports);
  return module.exports.__testUsageMath;
}

function plainTheme() {
  return {
    fg: (_color, text) => String(text),
    bold: (text) => String(text),
  };
}

function assistant(provider, model, usage, timestamp = Date.now()) {
  return {
    type: "message",
    timestamp: new Date(timestamp).toISOString(),
    message: {
      role: "assistant",
      provider,
      model,
      usage,
      timestamp,
    },
  };
}

function fakeCtx({ current, models = [current], entries = [] }) {
  return {
    model: current,
    cwd: process.cwd(),
    modelRegistry: {
      getAll: () => models,
      hasConfiguredAuth: () => true,
      isUsingOAuth: (model) => ["openai-codex", "github-copilot", "anthropic", "meridian"].includes(model?.provider),
    },
    sessionManager: {
      getEntries: () => entries,
      getSessionName: () => "",
    },
    getContextUsage: () => ({ tokens: 40_000, contextWindow: current.contextWindow ?? 200_000, percent: 20 }),
  };
}

test("usage footer keeps provider window labels with remaining percentages", () => {
  const usage = loadUsageModule();
  const snapshot = {
    fiveHour: { label: "3h", usedPercent: 15, source: "live" },
    weekly: { label: "1d", usedPercent: 5, source: "live" },
  };

  const parts = usage.usageFooterParts(snapshot, plainTheme(), 5, "session-weekly");

  assert.match(parts[0], /^3h\b/);
  assert.match(parts[0], /85% left$/);
  assert.match(parts[1], /^1d\b/);
  assert.match(parts[1], /95% left$/);
  assert.doesNotMatch(parts[1], /^weekly\b/);
});

test("usage footer uses Pi-style session stats for local provider usage", () => {
  const usage = loadUsageModule();
  const snapshot = {
    source: "local",
    auth: "api-key",
    session: {
      input: 1200,
      output: 300,
      cacheRead: 400,
      cacheWrite: 100,
      totalTokens: 2000,
      cost: 0.1234,
      turns: 1,
    },
    fiveHour: { label: "5h", usedPercent: null, source: "local" },
    weekly: { label: "7d", usedPercent: null, source: "local" },
  };

  const parts = usage.usageFooterParts(snapshot, plainTheme(), 5, "session-weekly");

  assert.equal(parts.length, 1);
  assert.match(parts[0], /^session\b/);
  assert.match(parts[0], /↑1\.2k ↓300 R400 W100 \$0\.123/);
});

test("OpenAI usage windows do not duplicate one provider limit into both footer slots", () => {
  const usage = loadUsageModule();
  const windows = usage.windowsFromOpenAISnapshot({
    limitId: "codex",
    primary: {
      usedPercent: 12,
      windowMinutes: 180,
      resetsAt: 1_800_000_000,
    },
  });

  assert.equal(windows.fiveHour.label, "3h");
  assert.equal(windows.fiveHour.usedPercent, 12);
  assert.equal(windows.weekly.label, "7d");
  assert.equal(windows.weekly.usedPercent, null);
});

test("OpenAI usage parser accepts reset_after_seconds and 0-100 used_percent values", () => {
  const usage = loadUsageModule();
  const before = Math.floor(Date.now() / 1000);
  const snapshots = usage.openAISnapshotsFromPayload({
    plan_type: "plus",
    rate_limit: {
      primary_window: {
        used_percent: 15,
        limit_window_seconds: 10_800,
        reset_after_seconds: 120,
      },
      secondary_window: {
        used_percent: 5,
        limit_window_seconds: 86_400,
        reset_after_seconds: 3_600,
      },
    },
  });

  assert.equal(snapshots[0].primary.usedPercent, 15);
  assert.equal(snapshots[0].secondary.usedPercent, 5);
  assert.ok(snapshots[0].primary.resetsAt >= before + 119);
  assert.ok(snapshots[0].primary.resetsAt <= before + 121);
});

test("usage report includes Copilot OpenAI Anthropic and Meridian proxy providers", () => {
  const usage = loadUsageModule();
  const models = [
    { provider: "github-copilot", id: "gpt-5-mini", contextWindow: 200_000 },
    { provider: "openai", id: "gpt-5.5", contextWindow: 400_000 },
    { provider: "openai-compatible", id: "gpt-5.5-proxy", contextWindow: 400_000 },
    { provider: "anthropic", id: "claude-sonnet-4-5", contextWindow: 200_000 },
    { provider: "meridian", id: "claude-sonnet-4-6", contextWindow: 200_000 },
  ];
  const report = usage.currentUsageReport(fakeCtx({
    current: models[3],
    models,
    entries: [
      assistant("github-copilot", "gpt-5-mini", { input: 10, output: 5, cacheRead: 0, cacheWrite: 0, totalTokens: 15, cost: { total: 0 } }),
      assistant("openai", "gpt-5.5", { input: 20, output: 10, cacheRead: 5, cacheWrite: 0, totalTokens: 35, cost: { total: 0.0123 } }),
      assistant("openai-compatible", "gpt-5.5-proxy", { input: 22, output: 11, cacheRead: 6, cacheWrite: 0, totalTokens: 39, cost: { total: 0.0133 } }),
      assistant("anthropic", "claude-sonnet-4-5", { input: 30, output: 15, cacheRead: 5, cacheWrite: 2, totalTokens: 52, cost: { total: 0.0456 } }),
      assistant("meridian", "claude-sonnet-4-6", { input: 40, output: 20, cacheRead: 8, cacheWrite: 4, totalTokens: 72, cost: { total: 0 } }),
    ],
  }));

  assert.deepEqual(report.snapshots.map((snapshot) => snapshot.provider).sort(), ["anthropic", "github-copilot", "meridian", "openai", "openai-compatible"]);
  assert.equal(report.active.provider, "anthropic");
  assert.equal(report.active.session.totalTokens, 52);
  assert.equal(report.active.session.cost, 0.0456);
});

test("provider proxy headers produce live limit windows for Copilot Anthropic and Meridian", () => {
  const usage = loadUsageModule();

  assert.equal(usage.parseHeaderLimit("github-copilot", {
    "x-ratelimit-limit-tokens": "1000",
    "x-ratelimit-remaining-tokens": "750",
    "x-ratelimit-reset-tokens": "2m",
  }).usedPercent, 25);

  assert.equal(usage.parseHeaderLimit("openai", {
    "x-ratelimit-limit-requests": "500",
    "x-ratelimit-remaining-requests": "400",
  }).usedPercent, 20);

  assert.equal(usage.parseHeaderLimit("anthropic", {
    "anthropic-ratelimit-tokens-limit": "2000",
    "anthropic-ratelimit-tokens-remaining": "1000",
  }).usedPercent, 50);

  assert.equal(usage.parseHeaderLimit("meridian", {
    "anthropic-ratelimit-requests-limit": "100",
    "anthropic-ratelimit-requests-remaining": "70",
  }).usedPercent, 30);
});

test("/usage lines render Pi-style session stats for provider snapshots", () => {
  const usage = loadUsageModule();
  const model = { provider: "openai", id: "gpt-5.5", contextWindow: 400_000 };
  const report = usage.currentUsageReport(fakeCtx({
    current: model,
    models: [model],
    entries: [
      assistant("openai", "gpt-5.5", { input: 1200, output: 300, cacheRead: 400, cacheWrite: 100, totalTokens: 2000, cost: { total: 0.1234 } }),
    ],
  }));

  const text = usage.usageLines(fakeCtx({ current: model, models: [model] }), report, plainTheme()).join("\n");

  assert.match(text, /Session\s+↑1\.2k ↓300 R400 W100 \$0\.123/);
  assert.match(text, /remaining unknown/);
  assert.match(text, /ctx\s+/);
});
