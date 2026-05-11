import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { chmodSync, mkdtempSync, mkdirSync, readFileSync, realpathSync, writeFileSync } from "node:fs";
import { homedir, tmpdir } from "node:os";
import { join, resolve } from "node:path";
import test from "node:test";
import { buildPiArgs, checkForUpdateNotice, coerceRuntimeLoopMode, collectDoctorDiagnostics, compareVersions, generatedWindowsSandboxPassword, isMain, parseOppiArgs, resolveAgentDir, resolveOppiServerBin } from "./main.js";
import { parseMarketplaceCommand, parsePluginCommand, resolveEnabledPluginSources } from "./plugins.js";

function tempDir(name: string): string {
  return mkdtempSync(join(tmpdir(), `oppi-${name}-`));
}

function createFakeRuntimeServer(root: string): string {
  const script = join(root, "fake-runtime-server.mjs");
  writeFileSync(script, `import { createInterface } from 'node:readline';
const rl = createInterface({ input: process.stdin });
let nextTurn = 0;
const events = [];
let todoState = { todos: [], summary: '' };
function send(id, result) { console.log(JSON.stringify({ jsonrpc: '2.0', id, result })); }
function fail(id, code, message, data) { console.log(JSON.stringify({ jsonrpc: '2.0', id, error: { code, message, data } })); }
function event(type, body = {}) { const { threadId, turnId, ...kindBody } = body; const value = { kind: { type, ...kindBody } }; if (threadId) value.threadId = threadId; if (turnId) value.turnId = turnId; events.push(value); return value; }
function turn(req, status) { nextTurn += 1; return { id: 'turn-fake-' + nextTurn, threadId: req.params.threadId, status, phase: status === 'completed' ? 'await' : 'loop' }; }
rl.on('line', async (line) => {
  const req = JSON.parse(line);
  const id = req.id;
  switch (req.method) {
    case 'initialize': send(id, { protocolVersion: '0.1.0', minProtocolVersion: '0.1.0', protocolCompatible: true, serverName: 'oppi-server', serverVersion: 'fake', serverCapabilities: ['threads','turns','events','approvals','memory'], acceptedClientCapabilities: [] }); break;
    case 'thread/start': send(id, { thread: { id: 'thread-fake', project: req.params.project, status: 'active', title: req.params.title }, events: [] }); break;
    case 'pi/bridge-event': send(id, { events: [event('piAdapterEvent', { name: req.params.name, payload: req.params.payload })] }); break;
    case 'turn/run-agentic': {
      const input = String(req.params.input || '');
      if (input.includes('pairing error')) { fail(id, -32000, 'model step provided tool results without matching calls: missing-call', { code: 'tool_result_without_call', category: 'tool_pairing' }); break; }
      if (input.includes('approval')) {
        const paused = turn(req, 'waitingForApproval');
        const toolCall = req.params.modelSteps?.[0]?.toolCalls?.[0];
        send(id, { turn: paused, events: [event('approvalRequested', { turnId: paused.id, request: { id: 'approval-fake', reason: 'fake approval', risk: 'medium', toolCall } })], awaitingApproval: { id: 'approval-fake', reason: 'fake approval', risk: 'medium', toolCall } });
        break;
      }
      if (input.includes('background stream')) {
        const running = turn(req, 'running');
        const delta = event('itemDelta', { turnId: running.id, itemId: 'item-fake-stream', delta: 'streamed delta' });
        setTimeout(() => event('turnCompleted', { turnId: running.id }), 30);
        send(id, { turn: running, events: [delta] });
        break;
      }
      if (input.includes('background interrupt')) {
        const running = turn(req, 'running');
        send(id, { turn: running, events: [event('itemDelta', { turnId: running.id, itemId: 'item-fake-interrupt', delta: 'interruptible' })] });
        break;
      }
      if (input.includes('cancellation')) {
        const aborted = turn(req, 'aborted');
        send(id, { turn: aborted, events: [event('toolCallCompleted', { result: { callId: 'cancel-dry-run', status: 'aborted', error: 'dry-run cancellation' } }), event('turnAborted', { reason: 'dry-run cancellation' })] });
        break;
      }
      if (input.includes('guard abort')) {
        const aborted = turn(req, 'aborted');
        send(id, { turn: aborted, events: [event('turnAborted', { reason: 'continuation guard exceeded' })] });
        break;
      }
      if (req.params.modelProvider) {
        const provider = req.params.modelProvider;
        if (provider.kind === 'openai-codex') {
          const completed = turn(req, 'completed');
          send(id, { turn: completed, events: [event('itemDelta', { delta: 'Codex runtime-worker fake completed.' }), event('turnCompleted', { turnId: completed.id })], providerKind: provider.kind, providerModel: provider.model });
          break;
        }
        const key = process.env[provider.apiKeyEnv] || '';
        const messages = provider.systemPrompt ? [{ role: 'system', content: provider.systemPrompt }, { role: 'user', content: req.params.input }] : [{ role: 'user', content: req.params.input }];
        await fetch(provider.baseUrl + '/chat/completions', { method: 'POST', headers: { authorization: 'Bearer ' + key, 'content-type': 'application/json' }, body: JSON.stringify({ model: provider.model, messages, stream: true }) });
        await fetch(provider.baseUrl + '/chat/completions', { method: 'POST', headers: { authorization: 'Bearer ' + key, 'content-type': 'application/json' }, body: JSON.stringify({ model: provider.model, messages: [...messages, { role: 'tool', tool_call_id: 'direct-echo-smoke', content: 'direct tool output' }], stream: true }) });
        const completed = turn(req, 'completed');
        send(id, { turn: completed, events: [event('toolCallCompleted', { result: { callId: 'direct-echo-smoke', status: 'ok', output: 'direct tool output' } }), event('itemDelta', { delta: 'Rust direct provider smoke ' }), event('itemDelta', { delta: 'completed with direct tool output.' }), event('turnCompleted', { turnId: completed.id })] });
        break;
      }
      const completed = turn(req, 'completed');
      const toolEvents = (req.params.modelSteps?.[0]?.toolResults || []).map((result) => event('toolCallCompleted', { result }));
      send(id, { turn: completed, events: [...toolEvents, event('turnCompleted', { turnId: completed.id })] });
      break;
    }
    case 'turn/resume-agentic': {
      const completed = { id: req.params.turnId, threadId: req.params.threadId, status: 'completed', phase: 'await' };
      send(id, { turn: completed, events: [event('approvalResolved', { turnId: completed.id, decision: { requestId: 'approval-fake', decision: 'approved' } }), event('toolCallCompleted', { turnId: completed.id, result: { callId: 'approval-dry-run', status: 'ok', output: 'approved dry-run result' } }), event('turnCompleted', { turnId: completed.id })] });
      break;
    }
    case 'turn/interrupt': send(id, { events: [event('turnInterrupted', { turnId: req.params.turnId, reason: req.params.reason })] }); break;
    case 'memory/set': send(id, { events: [event('memoryStatusChanged', { status: req.params.status })] }); break;
    case 'memory/compact': send(id, { events: [event('handoffCompacted', { summary: req.params.summary })] }); break;
    case 'todos/list': send(id, { state: todoState }); break;
    case 'events/list': send(id, { events }); break;
    case 'debug/bundle': send(id, { schemaVersion: 1, redacted: true, metrics: { threadCount: 1, turnCount: nextTurn, eventCount: events.length, pendingApprovals: 0, pendingQuestions: 0, pluginCount: 0, mcpServerCount: 0, modelCount: 0, agentDefinitionCount: 0, agentRunCount: 0 } }); break;
    case 'server/shutdown': send(id, { shuttingDown: true }); process.exitCode = 0; rl.close(); break;
    default: fail(id, -32601, 'unknown method');
  }
});
`, "utf8");
  if (process.platform === "win32") {
    const cmd = join(root, "fake-runtime-server.cmd");
    writeFileSync(cmd, `@echo off\r\n"${process.execPath}" "${script}" %*\r\n`, "utf8");
    return cmd;
  }
  const bin = join(root, "fake-runtime-server");
  writeFileSync(bin, `#!/usr/bin/env sh\nexec "${process.execPath}" "${script}" "$@"\n`, "utf8");
  chmodSync(bin, 0o755);
  return bin;
}

function createFakeShell(root: string): string {
const script = join(root, "fake-oppi-shell.mjs");
  writeFileSync(script, `const argv = process.argv.slice(2);
if (argv.includes('--help')) { console.log('fake oppi-shell help'); process.exit(0); }
const serverIndex = argv.indexOf('--server');
const hasProvider = argv.includes('--mock') || argv.includes('--model') || argv.some((arg) => arg.startsWith('--model='));
if (!hasProvider && !argv.includes('--list-sessions')) { console.error('expected provider'); process.exit(2); }
if (serverIndex < 0 || !argv[serverIndex + 1]) { console.error('expected --server'); process.exit(3); }
console.log(JSON.stringify({ fakeShell: true, args: argv, server: argv[serverIndex + 1] }));
`, "utf8");
  if (process.platform === "win32") {
    const cmd = join(root, "fake-oppi-shell.cmd");
    writeFileSync(cmd, `@echo off\r\n"${process.execPath}" "${script}" %*\r\n`, "utf8");
    return cmd;
  }
  const bin = join(root, "fake-oppi-shell");
  writeFileSync(bin, `#!/usr/bin/env sh\nexec "${process.execPath}" "${script}" "$@"\n`, "utf8");
  chmodSync(bin, 0o755);
  return bin;
}

function createFakeDogfoodShell(root: string, options: { requireAgentDir?: string; requireRuntimeStoreDirPrefix?: string; backgroundSandboxDenied?: boolean; backgroundReadNeverMatches?: boolean; backgroundReadMarkerAfterAttempts?: number } = {}): string {
  const script = join(root, "fake-oppi-dogfood-shell.mjs");
  writeFileSync(script, `import { createInterface } from 'node:readline';
const requiredAgentDir = ${JSON.stringify(options.requireAgentDir ?? "")};
const requiredRuntimeStoreDirPrefix = ${JSON.stringify(options.requireRuntimeStoreDirPrefix ?? "")};
const backgroundSandboxDenied = ${JSON.stringify(options.backgroundSandboxDenied ?? false)};
const backgroundReadNeverMatches = ${JSON.stringify(options.backgroundReadNeverMatches ?? false)};
const backgroundReadMarkerAfterAttempts = ${JSON.stringify(options.backgroundReadMarkerAfterAttempts ?? 1)};
if (requiredAgentDir && process.env.OPPI_AGENT_DIR !== requiredAgentDir) {
  console.error('expected OPPI_AGENT_DIR=' + requiredAgentDir + ', got ' + (process.env.OPPI_AGENT_DIR || ''));
  process.exit(17);
}
if (requiredRuntimeStoreDirPrefix && !(process.env.OPPI_RUNTIME_STORE_DIR || '').startsWith(requiredRuntimeStoreDirPrefix)) {
  console.error('expected OPPI_RUNTIME_STORE_DIR prefix=' + requiredRuntimeStoreDirPrefix + ', got ' + (process.env.OPPI_RUNTIME_STORE_DIR || ''));
  process.exit(18);
}
const rl = createInterface({ input: process.stdin });
const taskId = 'task-dogfood-fake';
let phase = 'permissions';
let backgroundReadAttempts = 0;
function emit(value) { console.log(JSON.stringify(value)); }
function event(type, extra = {}) { emit({ kind: { type, ...extra } }); }
rl.on('line', (line) => {
  if (line === '/permissions full-access' && phase === 'permissions') { emit({ permissions: { mode: 'full-access' } }); phase = 'approval'; return; }
  if (line === 'oppi-dogfood-repo-edit') { event('approvalRequested'); return; }
  if (line === '/approve' && phase === 'approval') { event('approvalResolved'); event('toolCallCompleted', { result: { output: 'wrote 222 bytes to docs/native-shell-dogfood.md' } }); event('turnCompleted'); phase = 'ask'; return; }
  if (line === 'oppi-dogfood-ask-user') { event('askUserRequested'); return; }
  if (line === 'oppi-dogfood-follow-up-one') { emit({ shell: 'queued follow-up #1' }); return; }
  if (line === 'oppi-dogfood-follow-up-two') { emit({ shell: 'queued follow-up #2' }); return; }
  if (line === '/answer safe') { event('askUserResolved'); event('turnCompleted'); event('turnCompleted'); event('turnCompleted'); phase = 'background'; return; }
  if (line === 'oppi-dogfood-background-start') { event('approvalRequested'); return; }
  if (line === '/approve' && phase === 'background' && backgroundSandboxDenied) { event('toolCallCompleted', { result: { status: 'denied', error: 'sandboxed background execution is unavailable on this host' } }); event('turnCompleted'); phase = 'readonly-setup'; return; }
  if (line === '/approve' && phase === 'background') { event('toolCallCompleted', { result: { output: 'background shell task started: ' + taskId } }); event('turnCompleted'); return; }
  if (line === '/background list') { emit({ background: { items: [{ id: taskId, status: 'running', cwd: '.' }] } }); return; }
  if (line.startsWith('/background read ')) {
    backgroundReadAttempts += 1;
    const output = backgroundReadNeverMatches || backgroundReadAttempts < backgroundReadMarkerAfterAttempts
      ? 'background output pending'
      : 'oppi-background-dogfood';
    emit({ backgroundRead: { task: { id: taskId, status: 'running' }, output } });
    return;
  }
  if (line === '/background kill ' + taskId) { emit({ backgroundKill: { task: { id: taskId, status: 'killed' } } }); phase = 'readonly-setup'; return; }
  if (line === '/permissions read-only') { emit({ permissions: { mode: 'read-only' } }); phase = 'readonly'; return; }
  if (line === 'oppi-dogfood-readonly-write') { event('approvalRequested'); return; }
  if (line === '/approve' && phase === 'readonly') { event('toolCallCompleted', { result: { status: 'denied', error: 'read-only mode blocks writes' } }); event('turnCompleted'); phase = 'protected-setup'; return; }
  if (line === '/permissions full-access' && phase === 'protected-setup') { emit({ permissions: { mode: 'full-access' } }); phase = 'protected'; return; }
  if (line === 'oppi-dogfood-protected-path') { event('approvalRequested'); return; }
  if (line === '/approve' && phase === 'protected') { event('toolCallCompleted', { result: { status: 'denied', error: 'protected path blocked' } }); event('turnCompleted'); phase = 'network-setup'; return; }
  if (line === '/permissions default') { emit({ permissions: { mode: 'default' } }); phase = 'network'; return; }
  if (line === 'oppi-dogfood-network-disabled') { event('approvalRequested'); return; }
  if (line === '/approve' && phase === 'network') { event('toolCallCompleted', { result: { status: 'denied', error: 'network disabled' } }); event('turnCompleted'); phase = 'image'; return; }
  if (line === 'oppi-dogfood-missing-image') { event('toolCallCompleted', { result: { status: 'error', error: 'image_gen requires a host-provided adapter result' } }); event('turnCompleted'); return; }
  if (line === '/exit') { process.exit(0); }
});
`, "utf8");
  if (process.platform === "win32") {
    const cmd = join(root, "fake-oppi-dogfood-shell.cmd");
    writeFileSync(cmd, `@echo off\r\n"${process.execPath}" "${script}" %*\r\n`, "utf8");
    return cmd;
  }
  const bin = join(root, "fake-oppi-dogfood-shell");
  writeFileSync(bin, `#!/usr/bin/env sh\nexec "${process.execPath}" "${script}" "$@"\n`, "utf8");
  chmodSync(bin, 0o755);
  return bin;
}

function createFakeHoppiModule(root: string, options: { settingsThrows?: boolean; settingsDisabled?: boolean; statusThrows?: boolean; recallThrows?: boolean; rememberThrows?: boolean; recallErrorMessage?: string; rememberErrorMessage?: string } = {}): { modulePath: string; recordsPath: string } {
  const recordsPath = join(root, "hoppi-records.json");
  const modulePath = join(root, "fake-hoppi.mjs");
  writeFileSync(recordsPath, "[]", "utf8");
  writeFileSync(modulePath, `import { readFileSync, writeFileSync } from 'node:fs';
const recordsPath = ${JSON.stringify(recordsPath)};
function record(value) {
  const records = JSON.parse(readFileSync(recordsPath, 'utf8'));
  records.push(value);
  writeFileSync(recordsPath, JSON.stringify(records, null, 2), 'utf8');
}
export function getDefaultHoppiRoot() { return ${JSON.stringify(join(root, "hoppi-root"))}; }
export function readHoppiMemorySettings() {
  ${options.settingsThrows ? "throw new Error('fake settings read failed');" : `return { enabled: ${options.settingsDisabled ? "false" : "true"}, startupRecall: true, taskStartRecall: true, turnSummaries: true, idleConsolidation: true, sync: { enabled: false } };`}
}
export function createHoppiBackend() {
  return {
    async init() { record({ type: 'init' }); },
    async status(project) { record({ type: 'status', project }); ${options.statusThrows ? "throw new Error('fake status failed');" : "return { enabled: true, initialized: true, storePath: 'fake', projectId: 'fake-project', displayName: 'fake', memoryCount: 2, pinnedCount: 1 };"} },
    async buildStartupContext(input) { record({ type: 'startup', input }); return { memoryCount: 1, contextMarkdown: '# Startup Hoppi\\nPinned runtime memory.', humanSummary: 'loaded' }; },
    async recall(input) { record({ type: 'recall', input }); ${options.recallThrows ? `throw new Error(${JSON.stringify(options.recallErrorMessage ?? "fake recall failed")});` : "return { memories: [{ id: 'memory-1' }], contextMarkdown: '# Recalled Hoppi\\nUse the remembered direct-worker context.' };"} },
    async remember(input) { record({ type: 'remember', input }); ${options.rememberThrows ? `throw new Error(${JSON.stringify(options.rememberErrorMessage ?? "fake remember failed")});` : "return { id: 'memory-saved', ...input };"} },
  };
}
`, "utf8");
  return { modulePath, recordsPath };
}

function createFakeCodexAuth(root: string): string {
  const authPath = join(root, "auth.json");
  writeFileSync(authPath, JSON.stringify({
    "openai-codex": {
      type: "oauth",
      access: "fake-codex-access-token",
      refresh: "fake-codex-refresh-token",
      expires: 4_102_444_800_000,
      accountId: "acct_fake",
    },
  }, null, 2), "utf8");
  return authPath;
}

test("parseOppiArgs handles OPPi-owned commands", () => {
  assert.deepEqual(parseOppiArgs(["--version"]), { type: "version" });
  assert.deepEqual(parseOppiArgs(["doctor", "--json"]), { type: "doctor", json: true, agentDir: undefined });
  assert.deepEqual(parseOppiArgs(["--agent-dir", "./agent", "doctor"]), { type: "doctor", json: false, agentDir: "./agent" });
  assert.deepEqual(parseOppiArgs(["update", "--check", "--json"]), { type: "update", check: true, json: true });
  assert.deepEqual(parseOppiArgs(["mem", "status", "--json"]), { type: "mem", subcommand: "status", json: true });
  assert.deepEqual(parseOppiArgs(["mem", "install", "--json"]), { type: "mem", subcommand: "install", json: true });
  assert.deepEqual(parseOppiArgs(["natives", "status", "--json"]), { type: "natives", subcommand: "status", json: true });
  assert.deepEqual(parseOppiArgs(["native", "benchmark"]), { type: "natives", subcommand: "benchmark", json: false });
  assert.deepEqual(parseOppiArgs(["sandbox", "setup-windows", "--yes", "--json", "--account", "oppi-sandbox-test"]), { type: "sandbox", subcommand: "setup-windows", json: true, yes: true, account: "oppi-sandbox-test", persistEnv: true, dryRun: false });
  assert.deepEqual(parseOppiArgs(["sandbox", "setup-windows", "--dry-run", "--json", "--no-persist-env"]), { type: "sandbox", subcommand: "setup-windows", json: true, yes: false, account: undefined, persistEnv: false, dryRun: true });
  assert.deepEqual(parseOppiArgs(["server", "--stdio", "--experimental", "--json"]), { type: "server", stdio: true, experimental: true, json: true });
  assert.deepEqual(parseOppiArgs(["tui", "--mock"]), { type: "tui", subcommand: "run", experimental: false, json: false, shellArgs: ["--mock"], agentDir: undefined });
  assert.deepEqual(parseOppiArgs(["tui", "--experimental", "--mock"]), { type: "tui", subcommand: "run", experimental: true, json: false, shellArgs: ["--mock"], agentDir: undefined });
  assert.deepEqual(parseOppiArgs(["--agent-dir", "./agent", "tui", "smoke", "--mock", "--json"]), { type: "tui", subcommand: "smoke", experimental: false, json: true, shellArgs: ["--mock", "--json"], agentDir: "./agent" });
  assert.deepEqual(parseOppiArgs(["tui", "dogfood", "--mock", "--json"]), { type: "tui", subcommand: "dogfood", experimental: false, json: true, shellArgs: ["--mock", "--json"], agentDir: undefined });
  assert.deepEqual(parseOppiArgs(["resume", "thread-7"]), { type: "resume", threadId: "thread-7", json: false, shellArgs: ["--resume", "thread-7"], agentDir: undefined });
  assert.deepEqual(parseOppiArgs(["--agent-dir", "./agent", "resume", "--json"]), { type: "resume", threadId: undefined, json: true, shellArgs: ["--list-sessions", "--json"], agentDir: "./agent" });
  assert.deepEqual(parseOppiArgs(["runtime-loop", "smoke", "--json"]), { type: "runtime-loop", subcommand: "smoke", json: true });
  assert.deepEqual(parseOppiArgs(["runtime-worker", "smoke", "--json"]), { type: "runtime-worker", subcommand: "smoke", json: true });
  assert.deepEqual(parseOppiArgs(["runtime-worker", "run", "hello", "repo", "--json", "--model", "demo", "--mock", "--auto-approve", "--memory"]), { type: "runtime-worker", subcommand: "run", json: true, prompt: "hello repo", model: "demo", baseUrl: undefined, apiKeyEnv: undefined, systemPrompt: undefined, maxOutputTokens: undefined, stream: true, mock: true, autoApprove: true, memory: "on" });
  assert.deepEqual(parseOppiArgs(["runtime-worker", "run", "hello", "--provider", "codex", "--model", "gpt-5.4"]), { type: "runtime-worker", subcommand: "run", json: false, prompt: "hello", provider: "openai-codex", model: "gpt-5.4", baseUrl: undefined, apiKeyEnv: undefined, systemPrompt: undefined, maxOutputTokens: undefined, stream: true, mock: false, autoApprove: false, memory: "auto" });
  assert.deepEqual(parseOppiArgs(["runtime-worker", "run", "hello", "--mock", "--prompt-variant", "caveman"]), { type: "runtime-worker", subcommand: "run", json: false, prompt: "hello", model: undefined, baseUrl: undefined, apiKeyEnv: undefined, systemPrompt: undefined, maxOutputTokens: undefined, stream: true, mock: true, autoApprove: false, memory: "auto", promptVariant: "promptname_b" });
  assert.deepEqual(parseOppiArgs(["runtime-worker", "run", "think", "--mock", "--effort", "max"]), { type: "runtime-worker", subcommand: "run", json: false, prompt: "think", model: undefined, baseUrl: undefined, apiKeyEnv: undefined, systemPrompt: undefined, maxOutputTokens: undefined, stream: true, mock: true, autoApprove: false, memory: "auto", effort: "xhigh" });
  assert.deepEqual(parseOppiArgs(["plugin", "list", "--json"]), { type: "plugin", subcommand: "list", json: true, scope: undefined });
  assert.deepEqual(parseOppiArgs(["marketplace", "list", "--json"]), { type: "marketplace", subcommand: "list", json: true });
});

test("parse plugin and marketplace commands", () => {
  assert.deepEqual(parsePluginCommand(["add", "./plugin", "--local", "--name", "demo"]), {
    type: "plugin",
    subcommand: "add",
    source: "./plugin",
    name: "demo",
    scope: "project",
    enable: false,
    yes: false,
    json: false,
  });
  assert.deepEqual(parsePluginCommand(["enable", "demo", "--yes", "--json"]), {
    type: "plugin",
    subcommand: "enable",
    name: "demo",
    scope: undefined,
    yes: true,
    json: true,
  });
  assert.deepEqual(parseMarketplaceCommand(["add", "./catalog.json", "--name=local"]), {
    type: "marketplace",
    subcommand: "add",
    url: "./catalog.json",
    name: "local",
    json: false,
  });
});

test("isMain tolerates resolved entrypoint paths", () => {
  const mainPath = resolve("dist", "main.js");
  assert.equal(isMain(mainPath, mainPath), true);
  assert.equal(isMain(undefined, mainPath), false);
  assert.equal(isMain(resolve("dist", "plugins.js"), mainPath), false);
  assert.equal(isMain(realpathSync(mainPath), mainPath), true);
});

test("parseOppiArgs strips OPPi flags and passes Pi args through", () => {
  assert.deepEqual(parseOppiArgs(["--agent-dir", "./tmp-agent", "--with-pi-extensions", "--model", "sonnet", "hello"]), {
    type: "launch",
    agentDir: "./tmp-agent",
    withPiExtensions: true,
    piArgs: ["--model", "sonnet", "hello"],
  });
});

test("resolveAgentDir uses documented precedence", () => {
  assert.equal(resolveAgentDir("./explicit", { OPPI_AGENT_DIR: "/oppi", PI_CODING_AGENT_DIR: "/pi" }), resolve("./explicit"));
  assert.equal(resolveAgentDir(undefined, { OPPI_AGENT_DIR: "/oppi", PI_CODING_AGENT_DIR: "/pi" }), resolve("/oppi"));
  assert.equal(resolveAgentDir(undefined, { PI_CODING_AGENT_DIR: "/pi" }), resolve("/pi"));
  assert.equal(resolveAgentDir(undefined, {}), join(homedir(), ".oppi", "agent"));
});

test("doctor defaults its agent-dir probe to the current repository", () => {
  const root = tempDir("doctor-agent-dir");
  const diagnostics = collectDoctorDiagnostics({
    cwd: root,
    env: {
      OPPI_AGENT_DIR: undefined,
      PI_CODING_AGENT_DIR: undefined,
      OPPI_SERVER_BIN: undefined,
      OPPI_SHELL_BIN: undefined,
    },
  });
  const agentDir = join(root, ".oppi", "agent");
  assert.deepEqual(
    diagnostics.find((diagnostic) => diagnostic.name === "OPPi agent dir"),
    { status: "pass", name: "OPPi agent dir", message: `${agentDir} is writable` },
  );
});

test("doctor terminal diagnostic explains degraded keybindings and color fallback", () => {
  const root = tempDir("doctor-terminal-degraded");
  const diagnostics = collectDoctorDiagnostics({
    cwd: root,
    env: {
      OPPI_AGENT_DIR: undefined,
      PI_CODING_AGENT_DIR: undefined,
      OPPI_SERVER_BIN: undefined,
      OPPI_SHELL_BIN: undefined,
      TERM: "dumb",
      NO_COLOR: "1",
      COLORTERM: undefined,
      TERM_PROGRAM: undefined,
      WT_SESSION: undefined,
    },
  });
  const terminal = diagnostics.find((diagnostic) => diagnostic.name === "Terminal");
  assert.equal(terminal?.status, "warn");
  assert.match(terminal?.message ?? "", /limited color/);
  assert.match(terminal?.details ?? "", /plain\/no-color fallback/);
  assert.match(terminal?.details ?? "", /key chords.*degrade/i);
  assert.match(terminal?.details ?? "", /\/keys/);
});

test("doctor terminal diagnostic describes capable terminal key chords", () => {
  const root = tempDir("doctor-terminal-capable");
  const diagnostics = collectDoctorDiagnostics({
    cwd: root,
    env: {
      OPPI_AGENT_DIR: undefined,
      PI_CODING_AGENT_DIR: undefined,
      OPPI_SERVER_BIN: undefined,
      OPPI_SHELL_BIN: undefined,
      TERM: "xterm-256color",
      COLORTERM: "truecolor",
      TERM_PROGRAM: undefined,
      WT_SESSION: "1",
    },
  });
  const terminal = diagnostics.find((diagnostic) => diagnostic.name === "Terminal");
  assert.equal(terminal?.status, "pass");
  assert.match(terminal?.message ?? "", /Windows Terminal; color capable/);
  assert.match(terminal?.details ?? "", /Alt\+Enter/);
  assert.match(terminal?.details ?? "", /Ctrl\+Enter/);
  assert.match(terminal?.details ?? "", /truecolor|256-color/);
});

test("doctor terminal diagnostic warns for unknown limited terminals", () => {
  const root = tempDir("doctor-terminal-unknown");
  const diagnostics = collectDoctorDiagnostics({
    cwd: root,
    env: {
      OPPI_AGENT_DIR: undefined,
      PI_CODING_AGENT_DIR: undefined,
      OPPI_SERVER_BIN: undefined,
      OPPI_SHELL_BIN: undefined,
      TERM: undefined,
      COLORTERM: undefined,
      TERM_PROGRAM: undefined,
      WT_SESSION: undefined,
      NO_COLOR: undefined,
    },
  });
  const terminal = diagnostics.find((diagnostic) => diagnostic.name === "Terminal");
  assert.equal(terminal?.status, "warn");
  assert.match(terminal?.message ?? "", /unknown terminal; limited color/);
  assert.match(terminal?.details ?? "", /key chords.*degrade/i);
  assert.match(terminal?.details ?? "", /plain fallback/);
});

test("Pi AuthStorage writes under the OPPi-provided agent dir", async () => {
  const agentDir = tempDir("pi-auth");
  const previousPiAgentDir = process.env.PI_CODING_AGENT_DIR;
  process.env.PI_CODING_AGENT_DIR = agentDir;

  try {
    const { AuthStorage, getAgentDir } = await import("@mariozechner/pi-coding-agent");
    assert.equal(resolve(getAgentDir()), resolve(agentDir));

    const authStorage = AuthStorage.create();
    authStorage.set("openai", { type: "api_key", key: "OPENAI_API_KEY" });
    authStorage.set("openai-codex", { type: "oauth", access: "codex-access", refresh: "codex-refresh", expires: 4_102_444_800_000 });
    authStorage.set("google-gemini-cli", { type: "oauth", access: "gemini-access", refresh: "gemini-refresh", expires: 4_102_444_800_000 });
    authStorage.set("anthropic", { type: "oauth", access: "anthropic-access", refresh: "anthropic-refresh", expires: 4_102_444_800_000 });

    const stored = JSON.parse(readFileSync(join(agentDir, "auth.json"), "utf8"));
    assert.deepEqual(stored.openai, { type: "api_key", key: "OPENAI_API_KEY" });
    assert.deepEqual(stored["openai-codex"], { type: "oauth", access: "codex-access", refresh: "codex-refresh", expires: 4_102_444_800_000 });
    assert.deepEqual(stored["google-gemini-cli"], { type: "oauth", access: "gemini-access", refresh: "gemini-refresh", expires: 4_102_444_800_000 });
    assert.deepEqual(stored.anthropic, { type: "oauth", access: "anthropic-access", refresh: "anthropic-refresh", expires: 4_102_444_800_000 });
  } finally {
    if (previousPiAgentDir === undefined) delete process.env.PI_CODING_AGENT_DIR;
    else process.env.PI_CODING_AGENT_DIR = previousPiAgentDir;
  }
});

test("update check notices newer npm versions and throttles daily", async () => {
  const root = tempDir("update-check");
  const env = { OPPI_HOME: join(root, "oppi-home"), OPPI_UPDATE_CHECK_LATEST: "9.9.9" };
  const now = new Date("2026-01-01T00:00:00.000Z");

  assert.equal(compareVersions("0.2.6", "0.2.5"), 1);
  assert.equal(compareVersions("0.2.5", "0.2.5"), 0);
  assert.equal(compareVersions("0.2.4", "0.2.5"), -1);

  const first = await checkForUpdateNotice({ env, cwd: root, currentVersion: "0.2.5", now, timeoutMs: 1 });
  assert.match(first ?? "", /OPPi 9\.9\.9 is available/);
  assert.match(first ?? "", /Run oppi update/);
  assert.match(first ?? "", /Changelog: https:\/\/github\.com\/RemindZ\/oppi\/blob\/main\/CHANGELOG\.md/);

  const throttled = await checkForUpdateNotice({ env, cwd: root, currentVersion: "0.2.5", now, timeoutMs: 1 });
  assert.equal(throttled, undefined);

  const disabled = await checkForUpdateNotice({ env: { ...env, OPPI_UPDATE_CHECK: "0" }, cwd: root, currentVersion: "0.2.5", now: new Date("2026-01-02T00:00:01.000Z"), timeoutMs: 1 });
  assert.equal(disabled, undefined);
});

test("buildPiArgs isolates extension loading by default", () => {
  const isolated = buildPiArgs({ type: "launch", piArgs: ["-p", "ok"], withPiExtensions: false }, "/pkg");
  assert.deepEqual(isolated, ["--no-extensions", "-e", "/pkg", "-p", "ok"]);

  const withPlugins = buildPiArgs({ type: "launch", piArgs: ["-p", "ok"], withPiExtensions: false }, "/pkg", ["/plugin-a", "npm:@scope/plugin"]);
  assert.deepEqual(withPlugins, ["--no-extensions", "-e", "/pkg", "-e", "/plugin-a", "-e", "npm:@scope/plugin", "-p", "ok"]);

  const withExtensions = buildPiArgs({ type: "launch", piArgs: [], withPiExtensions: true }, "/pkg");
  assert.deepEqual(withExtensions, ["-e", "/pkg"]);
});

test("plugin command smoke stores disabled-by-default plugins and explicit trust enables them", () => {
  const root = tempDir("plugins");
  const oppiHome = join(root, "oppi-home");
  const pluginDir = join(root, "demo-plugin");
  mkdirSync(pluginDir, { recursive: true });
  writeFileSync(join(pluginDir, "package.json"), JSON.stringify({
    name: "demo-plugin",
    version: "1.0.0",
    description: "Demo plugin",
    pi: { extensions: ["./extensions/demo.ts"] },
  }), "utf8");

  const baseEnv = { ...process.env, OPPI_HOME: oppiHome };
  const add = spawnSync(process.execPath, [resolve("dist", "main.js"), "plugin", "add", pluginDir, "--json"], { encoding: "utf8", env: baseEnv });
  assert.equal(add.status, 0, add.stderr);
  assert.equal(JSON.parse(add.stdout).plugin.enabled, false);

  const blocked = spawnSync(process.execPath, [resolve("dist", "main.js"), "plugin", "enable", "demo-plugin", "--json"], { encoding: "utf8", env: baseEnv });
  assert.equal(blocked.status, 1);
  assert.match(JSON.parse(blocked.stdout).error, /without explicit trust/);

  const enabled = spawnSync(process.execPath, [resolve("dist", "main.js"), "plugin", "enable", "demo-plugin", "--yes", "--json"], { encoding: "utf8", env: baseEnv });
  assert.equal(enabled.status, 0, enabled.stderr);
  assert.deepEqual(resolveEnabledPluginSources({ env: { OPPI_HOME: oppiHome }, cwd: root }), [pluginDir]);
});

test("Claude-style marketplace entries fail with agent handoff prompt", () => {
  const root = tempDir("claude-marketplace");
  const oppiHome = join(root, "oppi-home");
  const catalog = join(root, "claude-catalog.json");
  writeFileSync(catalog, JSON.stringify({
    name: "claude-store-smoke",
    plugins: [{
      name: "claude-mcp-demo",
      description: "Claude MCP plugin shape",
      mcpServers: { demo: { command: "node", args: ["server.js"] } },
    }],
  }), "utf8");

  const baseEnv = { ...process.env, OPPI_HOME: oppiHome };
  const addMarketplace = spawnSync(process.execPath, [resolve("dist", "main.js"), "marketplace", "add", catalog, "--json"], { encoding: "utf8", env: baseEnv });
  assert.equal(addMarketplace.status, 0, addMarketplace.stderr);
  assert.equal(JSON.parse(addMarketplace.stdout).incompatiblePlugins, 1);

  const addPlugin = spawnSync(process.execPath, [resolve("dist", "main.js"), "plugin", "add", "claude-mcp-demo", "--json"], { encoding: "utf8", env: baseEnv });
  assert.equal(addPlugin.status, 1);
  const parsed = JSON.parse(addPlugin.stdout);
  assert.match(parsed.error, /does not look Pi\/OPPi-compatible/);
  assert.match(parsed.compatibility.agentHandoffPrompt, /Port the Claude marketplace plugin/);
});

test("runtime loop mode coercion defaults to fallback mirroring", () => {
  assert.equal(coerceRuntimeLoopMode(undefined), "default-with-fallback");
  assert.equal(coerceRuntimeLoopMode(""), "default-with-fallback");
  assert.equal(coerceRuntimeLoopMode("off"), "off");
  assert.equal(coerceRuntimeLoopMode("command"), "command");
  assert.equal(coerceRuntimeLoopMode("default"), "default-with-fallback");
  assert.equal(coerceRuntimeLoopMode("default-with-fallback"), "default-with-fallback");
  assert.equal(coerceRuntimeLoopMode("surprise"), "command");
});

test("resolveOppiServerBin honors explicit experimental server path", () => {
  const root = tempDir("server-bin");
  const fakeServer = join(root, process.platform === "win32" ? "oppi-server.exe" : "oppi-server");
  writeFileSync(fakeServer, "fake", "utf8");
  assert.equal(resolveOppiServerBin({ OPPI_SERVER_BIN: fakeServer }, root), fakeServer);
});

test("CLI smoke: server command requires explicit experimental flag", () => {
  const result = spawnSync(process.execPath, [resolve("dist", "main.js"), "server", "--stdio", "--json"], { encoding: "utf8" });
  assert.equal(result.status, 1);
  const parsed = JSON.parse(result.stdout);
  assert.match(parsed.error, /experimental/);
});

test("CLI smoke: sandbox setup-windows dry-run previews host changes without server", () => {
  const result = spawnSync(process.execPath, [resolve("dist", "main.js"), "sandbox", "setup-windows", "--dry-run", "--json", "--account", "oppi-dry-run", "--no-persist-env"], {
    encoding: "utf8",
    env: { ...process.env, OPPI_SERVER_BIN: join(tempDir("missing-server"), "missing-oppi-server.exe") },
  });
  assert.equal(result.status, 0, result.stderr || result.stdout);
  const parsed = JSON.parse(result.stdout);
  assert.equal(parsed.ok, true);
  assert.equal(parsed.action, "planned");
  assert.equal(parsed.dryRun, true);
  assert.equal(parsed.account, ".\\oppi-dry-run");
  assert.equal(parsed.persistedEnv, false);
  assert.deepEqual(parsed.wouldSetEnv, ["OPPI_WINDOWS_SANDBOX_USERNAME", "OPPI_WINDOWS_SANDBOX_PASSWORD", "OPPI_WINDOWS_SANDBOX_WFP_READY"]);
  assert.ok(parsed.plannedActions.some((action: string) => action.includes("create or update")));
  assert.ok(parsed.diagnostics.join("\n").includes("No changes were made"));
});

test("Windows sandbox generated password avoids net user legacy prompt", () => {
  for (let index = 0; index < 16; index += 1) {
    const password = generatedWindowsSandboxPassword();
    assert.ok(password.length <= 14, password);
    assert.match(password, /[a-z]/);
    assert.match(password, /[A-Z]/);
    assert.match(password, /\d/);
    assert.match(password, /[^A-Za-z0-9]/);
  }
});

test("CLI smoke: tui command launches Rust UI and wraps shell smoke", () => {
  const root = tempDir("tui-smoke");
  const fakeServer = createFakeRuntimeServer(root);
  const fakeShell = createFakeShell(root);

  const launched = spawnSync(process.execPath, [resolve("dist", "main.js"), "tui", "--mock"], {
    encoding: "utf8",
    env: { ...process.env, OPPI_SERVER_BIN: fakeServer, OPPI_SHELL_BIN: fakeShell },
  });
  assert.equal(launched.status, 0, launched.stderr);
  assert.match(launched.stdout, /fakeShell/);

  const result = spawnSync(process.execPath, [resolve("dist", "main.js"), "tui", "smoke", "--mock", "--json"], {
    encoding: "utf8",
    env: { ...process.env, OPPI_SERVER_BIN: fakeServer, OPPI_SHELL_BIN: fakeShell, OPPI_SERVER_AUTH_TOKEN: "super-secret-tui-token", OPPI_AGENT_DIR: join(root, "agent") },
  });
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.stdout.includes("super-secret-tui-token"), false);
  const parsed = JSON.parse(result.stdout);
  assert.equal(parsed.ok, true);
  assert.equal(parsed.shellBin, fakeShell);
  assert.equal(parsed.serverBin, fakeServer);
});

test("CLI smoke: resume forwards to Rust shell resume routes", () => {
  const root = tempDir("resume-smoke");
  const fakeServer = createFakeRuntimeServer(root);
  const fakeShell = createFakeShell(root);

  const direct = spawnSync(process.execPath, [resolve("dist", "main.js"), "resume", "thread-7"], {
    encoding: "utf8",
    env: { ...process.env, OPPI_SERVER_BIN: fakeServer, OPPI_SHELL_BIN: fakeShell },
  });
  assert.equal(direct.status, 0, direct.stderr);
  const directParsed = JSON.parse(direct.stdout);
  assert.equal(directParsed.args[directParsed.args.indexOf("--resume") + 1], "thread-7");

  const list = spawnSync(process.execPath, [resolve("dist", "main.js"), "resume", "--json"], {
    encoding: "utf8",
    env: { ...process.env, OPPI_SERVER_BIN: fakeServer, OPPI_SHELL_BIN: fakeShell },
  });
  assert.equal(list.status, 0, list.stderr);
  const listParsed = JSON.parse(list.stdout);
  assert.ok(listParsed.args.includes("--list-sessions"));
  assert.equal(listParsed.args.includes("--mock"), false);
  assert.equal(listParsed.args.includes("--model"), false);
  assert.ok(listParsed.args.includes("--json"));
});

test("CLI smoke: tui dogfood drives shell repo-edit approval ask_user follow-up and background scenarios", () => {
  const root = tempDir("tui-dogfood");
  const fakeServer = createFakeRuntimeServer(root);
  const fakeShell = createFakeDogfoodShell(root);

  const result = spawnSync(process.execPath, [resolve("dist", "main.js"), "tui", "dogfood", "--mock", "--json"], {
    encoding: "utf8",
    env: { ...process.env, OPPI_SERVER_BIN: fakeServer, OPPI_SHELL_BIN: fakeShell, OPPI_SERVER_AUTH_TOKEN: "super-secret-tui-dogfood-token", OPPI_AGENT_DIR: join(root, "agent") },
  });
  assert.equal(result.status, 0, result.stderr || result.stdout);
  assert.equal(result.stdout.includes("super-secret-tui-dogfood-token"), false);
  const parsed = JSON.parse(result.stdout);
  assert.equal(parsed.ok, true);
  assert.deepEqual(parsed.scenarios.map((scenario: any) => [scenario.name, scenario.ok]), [
    ["repo-edit-approval", true],
    ["ask-user-follow-up-queue", true],
    ["background-sandbox-execution", true],
    ["failure-read-only-write", true],
    ["failure-protected-path", true],
    ["failure-network-disabled", true],
    ["failure-missing-image-backend", true],
  ]);
  assert.equal(parsed.backgroundTaskId, "task-dogfood-fake");
});

test("CLI smoke: tui dogfood strict background lifecycle rejects sandbox degradation", () => {
  const root = tempDir("tui-dogfood-strict-background");
  const fakeServer = createFakeRuntimeServer(root);
  const fakeShell = createFakeDogfoodShell(root, { backgroundSandboxDenied: true });

  const result = spawnSync(process.execPath, [resolve("dist", "main.js"), "tui", "dogfood", "--mock", "--json", "--require-background-lifecycle"], {
    encoding: "utf8",
    env: { ...process.env, OPPI_SERVER_BIN: fakeServer, OPPI_SHELL_BIN: fakeShell, OPPI_SERVER_AUTH_TOKEN: "super-secret-tui-dogfood-strict-token", OPPI_AGENT_DIR: join(root, "agent") },
  });
  assert.equal(result.stdout.includes("super-secret-tui-dogfood-strict-token"), false);
  const parsed = JSON.parse(result.stdout);
  const backgroundScenario = parsed.scenarios.find((scenario: any) => scenario.name === "background-sandbox-execution");
  assert.equal(result.status, 1, result.stderr || result.stdout);
  assert.equal(parsed.ok, false);
  assert.equal(backgroundScenario.ok, false);
  assert.equal(backgroundScenario.status, "sandbox-unavailable-denied");
  assert.match(parsed.diagnostics.join("\n"), /strict background lifecycle/i);
  assert.match(backgroundScenario.diagnostics.join("\n"), /sandboxed background execution is unavailable on this host/);
});

test("CLI smoke: tui dogfood strict background lifecycle retries delayed output", () => {
  const root = tempDir("tui-dogfood-strict-background-delayed-read");
  const fakeServer = createFakeRuntimeServer(root);
  const fakeShell = createFakeDogfoodShell(root, { backgroundReadMarkerAfterAttempts: 3 });

  const result = spawnSync(process.execPath, [resolve("dist", "main.js"), "tui", "dogfood", "--mock", "--json", "--require-background-lifecycle"], {
    encoding: "utf8",
    timeout: 10_000,
    env: { ...process.env, OPPI_SERVER_BIN: fakeServer, OPPI_SHELL_BIN: fakeShell, OPPI_SERVER_AUTH_TOKEN: "super-secret-tui-dogfood-delayed-read-token", OPPI_AGENT_DIR: join(root, "agent") },
  });
  assert.equal(result.status, 0, result.stderr || result.stdout);
  assert.equal(result.stdout.includes("super-secret-tui-dogfood-delayed-read-token"), false);
  const parsed = JSON.parse(result.stdout);
  const backgroundScenario = parsed.scenarios.find((scenario: any) => scenario.name === "background-sandbox-execution");
  assert.equal(parsed.ok, true);
  assert.equal(backgroundScenario.ok, true);
  assert.equal(backgroundScenario.status, "started, list=true, read=true, kill=true");
});

test("CLI smoke: tui dogfood strict background lifecycle fails fast when output marker is missing", () => {
  const root = tempDir("tui-dogfood-strict-background-missing-marker");
  const fakeServer = createFakeRuntimeServer(root);
  const fakeShell = createFakeDogfoodShell(root, { backgroundReadNeverMatches: true });

  const result = spawnSync(process.execPath, [resolve("dist", "main.js"), "tui", "dogfood", "--mock", "--json", "--require-background-lifecycle"], {
    encoding: "utf8",
    timeout: 10_000,
    env: { ...process.env, OPPI_SERVER_BIN: fakeServer, OPPI_SHELL_BIN: fakeShell, OPPI_SERVER_AUTH_TOKEN: "super-secret-tui-dogfood-missing-marker-token", OPPI_AGENT_DIR: join(root, "agent") },
  });
  assert.equal(result.stdout.includes("super-secret-tui-dogfood-missing-marker-token"), false);
  const parsed = JSON.parse(result.stdout);
  const backgroundScenario = parsed.scenarios.find((scenario: any) => scenario.name === "background-sandbox-execution");
  assert.equal(result.status, 1, result.stderr || result.stdout);
  assert.equal(parsed.ok, false);
  assert.equal(parsed.exitCode, 0);
  assert.equal(backgroundScenario.ok, false);
  assert.equal(backgroundScenario.status, "started, list=true, read=false, kill=true");
  assert.equal(parsed.scenarios.find((scenario: any) => scenario.name === "failure-read-only-write")?.ok, true);
  assert.doesNotMatch(parsed.diagnostics.join("\n"), /timeout/i);
});

test("CLI smoke: tui dogfood defaults agent dir to the current repository", () => {
  const root = tempDir("tui-dogfood-agent-dir");
  const fakeServer = createFakeRuntimeServer(root);
  const expectedAgentDir = resolve(".oppi", "agent");
  const fakeShell = createFakeDogfoodShell(root, {
    requireAgentDir: expectedAgentDir,
    requireRuntimeStoreDirPrefix: join(expectedAgentDir, "runtime-store", "tui-dogfood-"),
  });

  const result = spawnSync(process.execPath, [resolve("dist", "main.js"), "tui", "dogfood", "--mock", "--json"], {
    encoding: "utf8",
    env: {
      ...process.env,
      OPPI_SERVER_BIN: fakeServer,
      OPPI_SHELL_BIN: fakeShell,
      OPPI_SERVER_AUTH_TOKEN: "super-secret-tui-dogfood-agent-dir-token",
      OPPI_AGENT_DIR: "",
      PI_CODING_AGENT_DIR: "",
    },
  });
  assert.equal(result.status, 0, result.stderr || result.stdout);
  assert.equal(result.stdout.includes("super-secret-tui-dogfood-agent-dir-token"), false);
  const parsed = JSON.parse(result.stdout);
  assert.equal(parsed.ok, true);
});

test("CLI smoke: runtime-loop smoke exercises configured server without printing secrets", () => {
  const root = tempDir("runtime-loop-smoke");
  const fakeServer = createFakeRuntimeServer(root);
  const result = spawnSync(process.execPath, [resolve("dist", "main.js"), "runtime-loop", "smoke", "--json"], {
    encoding: "utf8",
    env: {
      ...process.env,
      OPPI_SERVER_BIN: fakeServer,
      OPPI_SERVER_AUTH_TOKEN: "super-secret-runtime-token",
      OPPI_RUNTIME_LOOP_MODE: "default-with-fallback",
    },
  });
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.stdout.includes("super-secret-runtime-token"), false);
  const parsed = JSON.parse(result.stdout);
  assert.equal(parsed.ok, true);
  assert.equal(parsed.mode, "default-with-fallback");
  assert.equal(parsed.threadId, "thread-fake");
  assert.equal(parsed.turnStatus, "completed");
  assert.equal(parsed.debugBundleRedacted, true);
  assert.equal(parsed.bridgeClean, true);
  assert.match(parsed.bridgeCleanReason, /dry-run hardening scenarios passed/);
  assert.deepEqual(parsed.scenarios.map((scenario: any) => [scenario.name, scenario.ok]), [
    ["bridge-smoke", true],
    ["approval-resume", true],
    ["background-stream", true],
    ["background-interrupt", true],
    ["background-resume", true],
    ["cancellation", true],
    ["pairing-error", true],
    ["guard-abort", true],
    ["long-compaction", true],
  ]);
});

test("CLI smoke: runtime-worker smoke exercises direct-worker command without printing secrets", () => {
  const root = tempDir("runtime-worker-smoke");
  const fakeServer = createFakeRuntimeServer(root);
  const result = spawnSync(process.execPath, [resolve("dist", "main.js"), "runtime-worker", "smoke", "--json"], {
    encoding: "utf8",
    env: {
      ...process.env,
      OPPI_SERVER_BIN: fakeServer,
      OPPI_SERVER_AUTH_TOKEN: "super-secret-runtime-worker-token",
    },
  });
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.stdout.includes("super-secret-runtime-worker-token"), false);
  assert.equal(result.stdout.includes("oppi-runtime-worker-smoke-key"), false);
  const parsed = JSON.parse(result.stdout);
  assert.equal(parsed.ok, true);
  assert.equal(parsed.threadId, "thread-fake");
  assert.equal(parsed.turnStatus, "completed");
  assert.equal(parsed.providerRequestCount, 2);
  assert.equal(parsed.providerStreamed, true);
  assert.equal(parsed.assistantDeltaCount, 2);
  assert.deepEqual(parsed.toolResultStatuses, ["ok"]);
  assert.equal(parsed.workerClean, true);
  assert.match(parsed.workerCleanReason, /executed a provider-requested tool/);
});

test("CLI smoke: runtime-worker prompt uses mock direct provider without printing secrets", () => {
  const root = tempDir("runtime-worker-run");
  const fakeServer = createFakeRuntimeServer(root);
  const result = spawnSync(process.execPath, [resolve("dist", "main.js"), "runtime-worker", "run", "Summarize the direct worker", "--mock", "--json"], {
    encoding: "utf8",
    env: {
      ...process.env,
      OPPI_SERVER_BIN: fakeServer,
      OPPI_SERVER_AUTH_TOKEN: "super-secret-runtime-worker-run-token",
    },
  });
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.stdout.includes("super-secret-runtime-worker-run-token"), false);
  assert.equal(result.stdout.includes("oppi-runtime-worker-mock-key"), false);
  const parsed = JSON.parse(result.stdout);
  assert.equal(parsed.ok, true);
  assert.equal(parsed.threadId, "thread-fake");
  assert.equal(parsed.turnStatus, "completed");
  assert.equal(parsed.providerConfigured, true);
  assert.equal(parsed.providerRequestCount, 2);
  assert.equal(parsed.providerStreamed, true);
  assert.equal(parsed.featureGuidanceApplied, true);
  assert.equal(parsed.featureGuidanceProviderPromptIncluded, true);
  assert.match(parsed.assistantText, /Rust direct provider smoke completed|Rust direct provider run completed/);
});

test("CLI smoke: runtime-worker prompt maps effort to provider reasoning_effort", () => {
  const root = tempDir("runtime-worker-effort");
  const fakeServer = createFakeRuntimeServer(root);
  const result = spawnSync(process.execPath, [resolve("dist", "main.js"), "runtime-worker", "run", "Use high effort", "--mock", "--effort", "xhigh", "--json"], {
    encoding: "utf8",
    env: {
      ...process.env,
      OPPI_SERVER_BIN: fakeServer,
      OPPI_SERVER_AUTH_TOKEN: "super-secret-runtime-worker-effort-token",
    },
  });
  assert.equal(result.status, 0, result.stderr);
  const parsed = JSON.parse(result.stdout);
  assert.equal(parsed.ok, true);
  assert.equal(parsed.effort, "xhigh");
  assert.equal(parsed.providerReasoningEffort, "high");
  assert.match(parsed.diagnostics.join("\n"), /reasoning_effort=high/);
});

test("CLI smoke: runtime-worker prompt applies caveman prompt variant to provider", () => {
  const root = tempDir("runtime-worker-prompt-variant");
  const fakeServer = createFakeRuntimeServer(root);
  const result = spawnSync(process.execPath, [resolve("dist", "main.js"), "runtime-worker", "run", "Use the caveman prompt variant", "--mock", "--prompt-variant", "caveman", "--json"], {
    encoding: "utf8",
    env: {
      ...process.env,
      OPPI_SERVER_BIN: fakeServer,
      OPPI_SERVER_AUTH_TOKEN: "super-secret-runtime-worker-variant-token",
    },
  });
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.stdout.includes("super-secret-runtime-worker-variant-token"), false);
  const parsed = JSON.parse(result.stdout);
  assert.equal(parsed.ok, true);
  assert.equal(parsed.promptVariant, "promptname_b");
  assert.equal(parsed.promptVariantApplied, true);
  assert.equal(parsed.promptVariantProviderPromptIncluded, true);
  assert.match(parsed.diagnostics.join("\n"), /prompt variant promptname_b applied/);
});

test("CLI smoke: runtime-worker prompt bridges Hoppi memory when explicitly enabled", () => {
  const root = tempDir("runtime-worker-hoppi");
  const fakeServer = createFakeRuntimeServer(root);
  const fakeHoppi = createFakeHoppiModule(root);
  const result = spawnSync(process.execPath, [resolve("dist", "main.js"), "runtime-worker", "run", "Use remembered context in the Rust direct worker", "--mock", "--memory", "--json"], {
    encoding: "utf8",
    env: {
      ...process.env,
      OPPI_SERVER_BIN: fakeServer,
      OPPI_HOPPI_MODULE: fakeHoppi.modulePath,
      OPPI_SERVER_AUTH_TOKEN: "super-secret-runtime-worker-hoppi-token",
    },
  });
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.stdout.includes("super-secret-runtime-worker-hoppi-token"), false);
  const parsed = JSON.parse(result.stdout);
  assert.equal(parsed.ok, true);
  assert.equal(parsed.memoryEnabled, true);
  assert.equal(parsed.memoryAvailable, true);
  assert.equal(parsed.memoryLoaded, true);
  assert.equal(parsed.memorySaved, true);
  assert.equal(parsed.memoryProviderPromptIncluded, true);
  assert.equal(parsed.memoryCount, 2);
  assert.ok(parsed.memoryContextBytes > 0);
  assert.match(parsed.diagnostics.join("\n"), /Hoppi memory loaded/);

  const records = JSON.parse(readFileSync(fakeHoppi.recordsPath, "utf8"));
  assert.ok(records.some((record: any) => record.type === "recall" && record.input.query.includes("remembered context")));
  const remembered = records.find((record: any) => record.type === "remember");
  assert.ok(remembered?.input?.content.includes("Runtime-worker turn summary"));
  assert.ok(remembered?.input?.content.includes("Use remembered context"));
});

test("CLI smoke: runtime-worker prompt can use Codex subscription auth-store provider", () => {
  const root = tempDir("runtime-worker-codex-provider");
  const fakeServer = createFakeRuntimeServer(root);
  const authPath = createFakeCodexAuth(root);
  const result = spawnSync(process.execPath, [resolve("dist", "main.js"), "runtime-worker", "run", "Use Codex subscription provider", "--provider", "openai-codex", "--model", "gpt-5.4", "--json", "--no-memory"], {
    encoding: "utf8",
    env: {
      ...process.env,
      OPPI_SERVER_BIN: fakeServer,
      OPPI_OPENAI_CODEX_AUTH_PATH: authPath,
      OPPI_SERVER_AUTH_TOKEN: "super-secret-runtime-worker-codex-token",
    },
  });
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.stdout.includes("super-secret-runtime-worker-codex-token"), false);
  assert.equal(result.stdout.includes("fake-codex-refresh-token"), false);
  const parsed = JSON.parse(result.stdout);
  assert.equal(parsed.ok, true);
  assert.equal(parsed.providerConfigured, true);
  assert.equal(parsed.provider, "openai-codex");
  assert.equal(parsed.assistantText, "Codex runtime-worker fake completed.");
  assert.match(parsed.diagnostics.join("\n"), /provider: openai-codex/);
});

test("CLI smoke: runtime-worker Codex provider fails closed without auth-store secrets", () => {
  const root = tempDir("runtime-worker-codex-missing");
  const fakeServer = createFakeRuntimeServer(root);
  const missingAuthPath = join(root, "missing-auth.json");
  const result = spawnSync(process.execPath, [resolve("dist", "main.js"), "runtime-worker", "run", "Use missing Codex auth", "--provider", "codex", "--json"], {
    encoding: "utf8",
    env: {
      ...process.env,
      OPPI_SERVER_BIN: fakeServer,
      OPPI_OPENAI_CODEX_AUTH_PATH: missingAuthPath,
      OPPI_SERVER_AUTH_TOKEN: "super-secret-runtime-worker-codex-missing-token",
    },
  });
  assert.equal(result.status, 1);
  assert.equal(result.stdout.includes("super-secret-runtime-worker-codex-missing-token"), false);
  const parsed = JSON.parse(result.stdout);
  assert.equal(parsed.ok, false);
  assert.equal(parsed.providerConfigured, false);
  assert.equal(parsed.provider, "openai-codex");
  assert.match(parsed.diagnostics.join("\n"), /Run \/login subscription codex/);
  assert.match(parsed.diagnostics.join("\n"), /Stable Pi fallback remains available/);
});

test("CLI smoke: runtime-worker Hoppi bridge degrades when Hoppi is missing", () => {
  const root = tempDir("runtime-worker-hoppi-missing");
  const fakeServer = createFakeRuntimeServer(root);
  const result = spawnSync(process.execPath, [resolve("dist", "main.js"), "runtime-worker", "run", "Continue without Hoppi", "--mock", "--memory", "--json"], {
    encoding: "utf8",
    env: {
      ...process.env,
      OPPI_HOME: join(root, "oppi-home"),
      OPPI_HOPPI_MODULE: join(root, "missing-hoppi.mjs"),
      OPPI_SERVER_BIN: fakeServer,
      OPPI_SERVER_AUTH_TOKEN: "super-secret-runtime-worker-hoppi-missing-token",
    },
  });
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.stdout.includes("super-secret-runtime-worker-hoppi-missing-token"), false);
  const parsed = JSON.parse(result.stdout);
  assert.equal(parsed.ok, true);
  assert.equal(parsed.memoryEnabled, true);
  assert.equal(parsed.memoryAvailable, false);
  assert.equal(parsed.memoryLoaded, false);
  assert.match(parsed.diagnostics.join("\n"), /optional Hoppi package is unavailable|continuing without memory/);
});

test("CLI smoke: runtime-worker Hoppi bridge falls back when settings read fails", () => {
  const root = tempDir("runtime-worker-hoppi-settings");
  const fakeServer = createFakeRuntimeServer(root);
  const fakeHoppi = createFakeHoppiModule(root, { settingsThrows: true });
  const result = spawnSync(process.execPath, [resolve("dist", "main.js"), "runtime-worker", "run", "Use remembered context despite settings read failure", "--mock", "--memory", "--json"], {
    encoding: "utf8",
    env: {
      ...process.env,
      OPPI_SERVER_BIN: fakeServer,
      OPPI_HOPPI_MODULE: fakeHoppi.modulePath,
      OPPI_SERVER_AUTH_TOKEN: "super-secret-runtime-worker-hoppi-settings-token",
    },
  });
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.stdout.includes("super-secret-runtime-worker-hoppi-settings-token"), false);
  const parsed = JSON.parse(result.stdout);
  assert.equal(parsed.ok, true);
  assert.equal(parsed.memoryLoaded, true);
  assert.equal(parsed.memoryProviderPromptIncluded, true);
  assert.match(parsed.diagnostics.join("\n"), /settings read failed; using safe defaults/);
});

test("CLI smoke: runtime-worker Hoppi bridge stays off when memory is disabled", () => {
  const root = tempDir("runtime-worker-hoppi-disabled");
  const fakeServer = createFakeRuntimeServer(root);
  const fakeHoppi = createFakeHoppiModule(root);
  const result = spawnSync(process.execPath, [resolve("dist", "main.js"), "runtime-worker", "run", "Do not load memory", "--mock", "--no-memory", "--json"], {
    encoding: "utf8",
    env: {
      ...process.env,
      OPPI_SERVER_BIN: fakeServer,
      OPPI_HOPPI_MODULE: fakeHoppi.modulePath,
      OPPI_SERVER_AUTH_TOKEN: "super-secret-runtime-worker-hoppi-disabled-token",
    },
  });
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.stdout.includes("super-secret-runtime-worker-hoppi-disabled-token"), false);
  const parsed = JSON.parse(result.stdout);
  assert.equal(parsed.ok, true);
  assert.equal(parsed.memoryEnabled, false);
  assert.equal(parsed.memoryAvailable, false);
  assert.equal(parsed.memoryLoaded, false);
  assert.equal(parsed.memorySaved, false);
  const records = JSON.parse(readFileSync(fakeHoppi.recordsPath, "utf8"));
  assert.deepEqual(records, []);
});

test("CLI smoke: runtime-worker Hoppi bridge reports status and recall failures without aborting", () => {
  const root = tempDir("runtime-worker-hoppi-recall-failure");
  const fakeServer = createFakeRuntimeServer(root);
  const fakeHoppi = createFakeHoppiModule(root, { statusThrows: true, recallThrows: true });
  const result = spawnSync(process.execPath, [resolve("dist", "main.js"), "runtime-worker", "run", "Continue after recall trouble", "--mock", "--memory", "--json"], {
    encoding: "utf8",
    env: {
      ...process.env,
      OPPI_SERVER_BIN: fakeServer,
      OPPI_HOPPI_MODULE: fakeHoppi.modulePath,
      OPPI_SERVER_AUTH_TOKEN: "super-secret-runtime-worker-hoppi-recall-token",
    },
  });
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.stdout.includes("super-secret-runtime-worker-hoppi-recall-token"), false);
  const parsed = JSON.parse(result.stdout);
  assert.equal(parsed.ok, true);
  assert.equal(parsed.memoryEnabled, true);
  assert.equal(parsed.memoryAvailable, true);
  assert.equal(parsed.memoryLoaded, true);
  assert.equal(parsed.memorySaved, true);
  assert.equal(parsed.memoryCount, 1);
  assert.match(parsed.diagnostics.join("\n"), /status unavailable; continuing recall/);
  assert.match(parsed.diagnostics.join("\n"), /task recall failed; continuing without recalled task context/);
  const records = JSON.parse(readFileSync(fakeHoppi.recordsPath, "utf8"));
  assert.ok(records.some((record: any) => record.type === "status"));
  assert.ok(records.some((record: any) => record.type === "recall"));
});

test("CLI smoke: runtime-worker Hoppi bridge redacts write failures and continues", () => {
  const root = tempDir("runtime-worker-hoppi-write-failure");
  const fakeServer = createFakeRuntimeServer(root);
  const fakeHoppi = createFakeHoppiModule(root, {
    rememberThrows: true,
    rememberErrorMessage: "fake secret: super-secret-hoppi-write-token",
  });
  const result = spawnSync(process.execPath, [resolve("dist", "main.js"), "runtime-worker", "run", "Continue after write trouble", "--mock", "--memory", "--json"], {
    encoding: "utf8",
    env: {
      ...process.env,
      OPPI_SERVER_BIN: fakeServer,
      OPPI_HOPPI_MODULE: fakeHoppi.modulePath,
      OPPI_SERVER_AUTH_TOKEN: "super-secret-runtime-worker-hoppi-write-token",
    },
  });
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.stdout.includes("super-secret-runtime-worker-hoppi-write-token"), false);
  assert.equal(result.stdout.includes("super-secret-hoppi-write-token"), false);
  const parsed = JSON.parse(result.stdout);
  assert.equal(parsed.ok, true);
  assert.equal(parsed.memoryEnabled, true);
  assert.equal(parsed.memoryAvailable, true);
  assert.equal(parsed.memoryLoaded, true);
  assert.equal(parsed.memorySaved, false);
  assert.match(parsed.diagnostics.join("\n"), /summary save failed/);
  assert.match(parsed.diagnostics.join("\n"), /fake secret: \[redacted\]/);
});

test("CLI smoke: runtime-worker prompt shows stable Pi fallback when provider auth is missing", () => {
  const root = tempDir("runtime-worker-fallback");
  const fakeServer = createFakeRuntimeServer(root);
  const result = spawnSync(process.execPath, [resolve("dist", "main.js"), "runtime-worker", "run", "Summarize fallback", "--json"], {
    encoding: "utf8",
    env: {
      ...process.env,
      OPPI_SERVER_BIN: fakeServer,
      OPPI_SERVER_AUTH_TOKEN: "super-secret-runtime-worker-fallback-token",
      OPPI_OPENAI_API_KEY: "",
      OPENAI_API_KEY: "",
      OPPI_RUNTIME_WORKER_API_KEY_ENV: "OPPI_TEST_DIRECT_WORKER_KEY",
      OPPI_TEST_DIRECT_WORKER_KEY: "",
    },
  });
  assert.equal(result.status, 1);
  assert.equal(result.stdout.includes("super-secret-runtime-worker-fallback-token"), false);
  const parsed = JSON.parse(result.stdout);
  assert.equal(parsed.ok, false);
  assert.equal(parsed.providerConfigured, false);
  assert.equal(parsed.fallbackAvailable, true);
  assert.match(parsed.fallbackCommand, /oppi/);
});

test("CLI smoke: natives status --json reports optional fallback state", () => {
  const result = spawnSync(process.execPath, [resolve("dist", "main.js"), "natives", "status", "--json"], {
    encoding: "utf8",
    env: { ...process.env, OPPI_DISABLE_NATIVES: "1" },
  });
  assert.equal(result.status, 0, result.stderr);
  const parsed = JSON.parse(result.stdout);
  assert.equal(parsed.packageName, "@oppiai/natives");
  assert.equal(parsed.native.available, false);
  assert.match(parsed.native.error, /OPPI_DISABLE_NATIVES/);
});

test("CLI smoke: --version prints package version", () => {
  const result = spawnSync(process.execPath, [resolve("dist", "main.js"), "--version"], { encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout.trim(), /^\d+\.\d+\.\d+/);
});

test("CLI smoke: doctor --json uses configured paths without printing secrets", () => {
  const root = tempDir("doctor");
  const agentDir = join(root, "agent");
  const fakePi = join(root, "fake-pi.mjs");
  const fakePackage = join(root, "pi-package");
  mkdirSync(fakePackage, { recursive: true });
  writeFileSync(fakePi, "process.exit(0);\n", "utf8");
  writeFileSync(join(fakePackage, "package.json"), "{\"name\":\"@oppiai/pi-package\"}\n", "utf8");

  const result = spawnSync(process.execPath, [resolve("dist", "main.js"), "doctor", "--json"], {
    encoding: "utf8",
    env: {
      ...process.env,
      OPPI_AGENT_DIR: agentDir,
      OPPI_HOME: join(root, "oppi-home"),
      OPPI_PI_CLI: fakePi,
      OPPI_PI_PACKAGE: fakePackage,
      OPPI_SERVER_BIN: process.execPath,
      OPPI_SHELL_BIN: process.execPath,
      OPPI_FEEDBACK_TOKEN: "super-secret-token-for-test",
      OPPI_OPENAI_API_KEY: "",
      OPENAI_API_KEY: "",
      OPPI_RUNTIME_WORKER_API_KEY_ENV: "OPPI_TEST_DIRECT_WORKER_KEY",
      OPPI_TEST_DIRECT_WORKER_KEY: "",
    },
  });

  assert.equal(result.status, 0, result.stderr);
  const parsed = JSON.parse(result.stdout);
  assert.equal(parsed.ok, true);
  assert.equal(result.stdout.includes("super-secret-token-for-test"), false);
  assert.equal(parsed.diagnostics.some((item: any) => item.name === "Rust runtime" && item.status === "pass"), true);
  assert.equal(parsed.diagnostics.some((item: any) => item.name === "Rust protocol/sandbox"), true);
  assert.equal(parsed.diagnostics.some((item: any) => item.name === "Rust direct worker" && item.status === "warn"), true);
  assert.equal(parsed.diagnostics.some((item: any) => item.name === "Native Rust shell"), true);
});

test("CLI smoke: print-mode launch forwards args and isolated env to Pi", () => {
  const root = tempDir("launch");
  const agentDir = join(root, "agent");
  const capture = join(root, "capture.json");
  const fakePi = join(root, "fake-pi.mjs");
  const fakePackage = join(root, "pi-package");
  mkdirSync(fakePackage, { recursive: true });
  writeFileSync(join(fakePackage, "package.json"), "{\"name\":\"@oppiai/pi-package\"}\n", "utf8");
  writeFileSync(fakePi, `import { writeFileSync } from "node:fs";\nwriteFileSync(process.env.OPPI_FAKE_PI_CAPTURE, JSON.stringify({ argv: process.argv.slice(2), OPPI_AGENT_DIR: process.env.OPPI_AGENT_DIR, PI_CODING_AGENT_DIR: process.env.PI_CODING_AGENT_DIR }));\n`, "utf8");

  const result = spawnSync(process.execPath, [resolve("dist", "main.js"), "--agent-dir", agentDir, "-p", "Reply ok"], {
    encoding: "utf8",
    env: {
      ...process.env,
      OPPI_HOME: join(root, "oppi-home"),
      OPPI_PI_CLI: fakePi,
      OPPI_PI_PACKAGE: fakePackage,
      OPPI_FAKE_PI_CAPTURE: capture,
    },
  });

  assert.equal(result.status, 0, result.stderr);
  const captured = JSON.parse(readFileSync(capture, "utf8"));
  assert.deepEqual(captured.argv, ["--no-extensions", "-e", fakePackage, "-p", "Reply ok"]);
  assert.equal(captured.OPPI_AGENT_DIR, agentDir);
  assert.equal(captured.PI_CODING_AGENT_DIR, agentDir);
});

test("CLI smoke: launch appends enabled OPPi plugin sources", () => {
  const root = tempDir("launch-plugin");
  const agentDir = join(root, "agent");
  const oppiHome = join(root, "oppi-home");
  const capture = join(root, "capture.json");
  const fakePi = join(root, "fake-pi.mjs");
  const fakePackage = join(root, "pi-package");
  const pluginDir = join(root, "plugin-package");
  mkdirSync(fakePackage, { recursive: true });
  mkdirSync(pluginDir, { recursive: true });
  mkdirSync(oppiHome, { recursive: true });
  writeFileSync(join(fakePackage, "package.json"), "{\"name\":\"@oppiai/pi-package\"}\n", "utf8");
  writeFileSync(join(pluginDir, "package.json"), "{\"name\":\"plugin-package\",\"pi\":{\"extensions\":[\"./index.ts\"]}}\n", "utf8");
  writeFileSync(join(oppiHome, "plugin-lock.json"), JSON.stringify({
    version: 1,
    plugins: [{
      name: "plugin-package",
      source: pluginDir,
      sourceType: "local",
      enabled: true,
      trusted: true,
      addedAt: new Date().toISOString(),
      updatedAt: new Date().toISOString(),
    }],
  }), "utf8");
  writeFileSync(fakePi, `import { writeFileSync } from "node:fs";\nwriteFileSync(process.env.OPPI_FAKE_PI_CAPTURE, JSON.stringify({ argv: process.argv.slice(2) }));\n`, "utf8");

  const result = spawnSync(process.execPath, [resolve("dist", "main.js"), "--agent-dir", agentDir, "-p", "Reply ok"], {
    encoding: "utf8",
    env: {
      ...process.env,
      OPPI_HOME: oppiHome,
      OPPI_PI_CLI: fakePi,
      OPPI_PI_PACKAGE: fakePackage,
      OPPI_FAKE_PI_CAPTURE: capture,
    },
  });

  assert.equal(result.status, 0, result.stderr);
  const captured = JSON.parse(readFileSync(capture, "utf8"));
  assert.deepEqual(captured.argv, ["--no-extensions", "-e", fakePackage, "-e", pluginDir, "-p", "Reply ok"]);
});
