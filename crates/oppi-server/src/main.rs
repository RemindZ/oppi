use oppi_core::{
    MemoryBackendRawResult, MemoryBackendRunner, Runtime, TurnInterruptRegistry,
    event_store::{EventStore, FilesystemEventStore, StoreNamespace},
};
use oppi_memory::{
    EmbeddedListInput, EmbeddedMemoryError, EmbeddedMemoryRecord, EmbeddedMemoryStatus,
    EmbeddedMemoryStore, EmbeddedRecallInput, EmbeddedRecordPatch, EmbeddedRememberInput,
};
use oppi_protocol::*;
use oppi_sandbox::{
    SandboxManager, current_sandbox_status, execute_sandboxed,
    install_windows_wfp_filters_for_account, windows_wfp_status,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io::{self, BufRead, Read, Write};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    #[allow(dead_code)]
    jsonrpc: Option<String>,
    id: Value,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    jsonrpc: &'static str,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
struct JsonRpcError {
    code: i64,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct WindowsWfpInstallParams {
    account: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StdioAction {
    Continue,
    Shutdown,
}

const SERVER_EVENTS_LIST_DEFAULT_LIMIT: usize = 1_000;
const SERVER_EVENTS_LIST_MAX_LIMIT: usize = 5_000;
const BACKGROUND_TURN_START_TIMEOUT: Duration = Duration::from_secs(2);
const BACKGROUND_TURN_INTERRUPT_TIMEOUT: Duration = Duration::from_secs(2);
const HOPPI_PACKAGE_NAME: &str = "@oppiai/hoppi-memory";
const HOPPI_LEGACY_PACKAGE_NAME: &str = "hoppi-memory";
const HOPPI_NODE_BACKEND_NAME: &str = "hoppi-node-bridge";
const HOPPI_NODE_BACKEND_ADAPTER_KIND: &str = "node-typescript";
const HOPPI_EMBEDDED_BACKEND_NAME: &str = "hoppi-embedded";
const HOPPI_EMBEDDED_BACKEND_ADAPTER_KIND: &str = "rust-embedded";
const HOPPI_NODE_BACKEND_SCRIPT: &str = r#"
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { pathToFileURL } from "node:url";

function stringField(entry, keys) {
  for (const key of keys) {
    const value = entry && entry[key];
    if (typeof value === "string" && value.trim()) return value.trim();
  }
  return undefined;
}

function numberField(entry, keys) {
  for (const key of keys) {
    const value = Number(entry && entry[key]);
    if (Number.isFinite(value) && value >= 0) return Math.floor(value);
  }
  return undefined;
}

function stringArray(value) {
  return Array.isArray(value)
    ? value.map((item) => String(item || "").trim()).filter(Boolean)
    : [];
}

function normalizeRecord(entry, fallback = {}) {
  if (!entry || typeof entry !== "object") return undefined;
  const id = stringField(entry, ["id", "memoryId"]) || fallback.id;
  const content = stringField(entry, ["content", "text", "summary", "body"]) || fallback.content;
  if (!id || !content) return undefined;
  const tags = stringArray(entry.tags);
  return {
    id,
    content,
    tags: tags.length ? tags : (fallback.tags || []),
    pinned: Boolean(entry.pinned ?? fallback.pinned),
    starred: Boolean(entry.starred),
    layer: stringField(entry, ["layer"]) || fallback.layer,
    confidence: stringField(entry, ["confidence"]) || fallback.confidence,
    source: stringField(entry, ["source"]) || fallback.source,
    createdAt: stringField(entry, ["createdAt", "created_at", "created"]),
    updatedAt: stringField(entry, ["updatedAt", "updated_at"]),
    strength: numberField(entry, ["strength"]),
    retrievalStrength: numberField(entry, ["retrievalStrength", "retrieval_strength"]),
    retrievalCount: numberField(entry, ["retrievalCount", "retrieval_count", "useCount", "use_count"]),
    lastRetrievedAt: stringField(entry, ["lastRetrievedAt", "last_retrieved_at", "retrieved_at", "lastUsedAt", "last_used_at"]),
    supersededBy: stringField(entry, ["supersededBy", "superseded_by"]) || fallback.supersededBy,
    parents: stringArray(entry.parents),
  };
}

function normalizeStatus(status, extras = {}) {
  const merged = { ...(status || {}), ...extras };
  const memoryCount = numberField(merged, ["memoryCount", "memory_count"]);
  const pinnedCount = numberField(merged, ["pinnedCount", "pinned_count"]);
  return {
    enabled: Boolean(merged.enabled ?? true),
    initialized: Boolean(merged.initialized ?? true),
    backend: stringField(merged, ["backend"]) || "hoppi",
    scope: stringField(merged, ["scope"]) || "project",
    storePath: stringField(merged, ["storePath", "store_path"]),
    projectId: stringField(merged, ["projectId", "project_id"]),
    memoryCount: memoryCount ?? 0,
    pinnedCount: pinnedCount ?? 0,
    lastWriteAt: stringField(merged, ["lastWriteAt", "last_write_at"]),
    lastConsolidatedAt: stringField(merged, ["lastConsolidatedAt", "last_consolidated_at"]),
    queueDepth: numberField(merged, ["queueDepth", "queue_depth"]),
    dashboardUrl: stringField(merged, ["dashboardUrl", "url"]),
  };
}

async function bestEffortStatus(backend, project, extras = {}) {
  if (backend && typeof backend.status === "function") {
    try {
      return normalizeStatus(await backend.status(project), extras);
    } catch {
      // Operation results are still useful when status refresh fails.
    }
  }
  return normalizeStatus(undefined, extras);
}

const raw = fs.readFileSync(0, "utf8");
const input = JSON.parse(raw || "{}");
const modulePath = path.resolve(String(input.modulePath || ""));
const moduleUrl = pathToFileURL(modulePath).href;
const module = await import(moduleUrl);
if (typeof module.createHoppiBackend !== "function") {
  throw new Error("Hoppi module must export createHoppiBackend()");
}
const root = typeof module.getDefaultHoppiRoot === "function"
  ? module.getDefaultHoppiRoot()
  : path.join(os.homedir(), ".oppi", "hoppi");
const backend = module.createHoppiBackend({ root });
if (!backend || typeof backend !== "object") {
  throw new Error("createHoppiBackend() must return a backend object");
}
if (typeof backend.init === "function") {
  await backend.init();
}
const project = input.project || {};
const params = input.params || {};
const operation = String(input.operation || "");
let result = {};

if (operation === "status") {
  result = { status: await bestEffortStatus(backend, project) };
} else if (operation === "recall") {
  if (typeof backend.recall !== "function") throw new Error("Hoppi backend must expose recall(input)");
  const query = String(params.prompt || "").trim();
  result = query ? await backend.recall({
    project,
    query,
    budget: 900,
    limit: 5,
    includeStartup: Boolean(params.includeStartup),
    includeTask: Boolean(params.includeTask),
  }) : {};
} else if (operation === "remember") {
  if (typeof backend.remember !== "function") throw new Error("Hoppi backend must expose remember(input)");
  const rememberInput = {
    project,
    content: String(params.content || "").trim(),
    tags: Array.isArray(params.tags) ? params.tags.map((tag) => String(tag || "").trim()).filter(Boolean) : [],
    source: String(params.source || "oppi:runtime").trim(),
    sourceSessionId: params.sourceSessionId || null,
  };
  if (params.layer) rememberInput.layer = String(params.layer).trim();
  const rawRecord = await backend.remember(rememberInput);
  result = {
    saved: true,
    record: normalizeRecord(rawRecord, rememberInput),
    status: await bestEffortStatus(backend, project),
  };
} else if (operation === "list") {
  if (typeof backend.list !== "function") throw new Error("Hoppi backend must expose list(input)");
  const listInput = {
    project,
    includeSuperseded: Boolean(params.includeSuperseded),
    pinnedOnly: Boolean(params.pinnedOnly),
  };
  const limit = Number(params.limit);
  if (Number.isInteger(limit) && limit > 0) listInput.limit = Math.min(limit, 500);
  const rawRecords = await backend.list(listInput);
  const records = Array.isArray(rawRecords) ? rawRecords.map((record) => normalizeRecord(record)).filter(Boolean) : [];
  result = { records, status: await bestEffortStatus(backend, project, { memoryCount: records.length, pinnedCount: records.filter((record) => record.pinned || record.starred).length }) };
} else if (operation === "dashboard") {
  if (typeof backend.startDashboard !== "function") throw new Error("Hoppi backend must expose startDashboard(input)");
  const dashboard = await backend.startDashboard({ project, port: 0 });
  if (!dashboard || !dashboard.url) throw new Error("Hoppi backend returned no dashboard URL");
  const url = String(dashboard.url);
  result = {
    url,
    fallbackUrl: url.includes("hoppi.localhost") ? url.replace("hoppi.localhost", "localhost") : undefined,
    status: await bestEffortStatus(backend, project, { dashboardUrl: url }),
  };
  if (typeof dashboard.stop === "function") {
    try { await dashboard.stop(); } catch {}
  }
} else if (operation === "forget") {
  if (typeof backend.forget !== "function") throw new Error("Hoppi backend must expose forget(id)");
  const id = String(params.id || "").trim();
  if (!id) throw new Error("memory id is required");
  await backend.forget(id);
  result = { id, status: await bestEffortStatus(backend, project) };
} else if (operation === "pin") {
  if (typeof backend.pin !== "function") throw new Error("Hoppi backend must expose pin(id, pinned)");
  const id = String(params.id || "").trim();
  if (!id) throw new Error("memory id is required");
  const pinned = Boolean(params.pinned);
  const rawRecord = await backend.pin(id, pinned);
  result = { id, pinned, record: normalizeRecord(rawRecord, { id, pinned }), status: await bestEffortStatus(backend, project) };
} else if (operation === "update") {
  if (typeof backend.update !== "function") throw new Error("Hoppi backend must expose update(id, patch)");
  const id = String(params.id || "").trim();
  if (!id) throw new Error("memory id is required");
  const patch = params.patch && typeof params.patch === "object" ? params.patch : {};
  const rawRecord = await backend.update(id, patch);
  result = { id, record: normalizeRecord(rawRecord, { id, ...patch }), status: await bestEffortStatus(backend, project) };
} else if (operation === "supersede") {
  const id = String(params.id || "").trim();
  const supersededBy = String(params.supersededBy || "").trim();
  if (!id || !supersededBy) throw new Error("memory supersede ids are required");
  let rawRecord;
  if (typeof backend.supersede === "function") {
    rawRecord = await backend.supersede(id, { supersededBy });
  } else if (typeof backend.update === "function") {
    rawRecord = await backend.update(id, { supersededBy });
  } else {
    throw new Error("Hoppi backend must expose supersede(id, input) or update(id, patch)");
  }
  result = { id, supersededBy, record: normalizeRecord(rawRecord, { id, supersededBy }), status: await bestEffortStatus(backend, project) };
} else if (operation === "consolidate") {
  if (typeof backend.consolidate !== "function") throw new Error("Hoppi backend must expose consolidate(input)");
  const consolidateInput = {
    project,
    dryRun: Boolean(params.dryRun),
    stage: String(params.stage || "manual-maintenance").trim() || "manual-maintenance",
    source: "oppi:runtime",
    modelPreference: {
      agentModel: String(params.agentModel || "auto").trim() || "auto",
      deepModel: String(params.deepModel || "auto").trim() || "auto",
    },
  };
  if (Number.isInteger(Number(params.limit)) && Number(params.limit) > 0) consolidateInput.limit = Math.min(Number(params.limit), 500);
  if (Number.isInteger(Number(params.budget)) && Number(params.budget) > 0) consolidateInput.budget = Math.min(Number(params.budget), 16000);
  const rawResult = (await backend.consolidate(consolidateInput)) || {};
  const recordsRaw = rawResult.records || rawResult.memories || rawResult.created || rawResult.consolidated || [];
  const records = Array.isArray(recordsRaw) ? recordsRaw.map((record) => normalizeRecord(record)).filter(Boolean) : [];
  result = {
    ...rawResult,
    records,
    deletedIds: Array.isArray(rawResult.deletedIds || rawResult.deleted_ids || rawResult.deleted || rawResult.removedIds)
      ? (rawResult.deletedIds || rawResult.deleted_ids || rawResult.deleted || rawResult.removedIds).map((id) => String(id || "").trim()).filter(Boolean)
      : [],
    status: await bestEffortStatus(backend, project, rawResult.status || {}),
  };
} else if (operation === "sync") {
  if (typeof module.syncGitRepository !== "function") throw new Error("Hoppi module must expose syncGitRepository(root, repoPath, options)");
  const direction = String(params.direction || "both").trim() || "both";
  const options = {
    direction,
    conflictMode: String(params.conflictMode || "keep-both").trim() || "keep-both",
    message: String(params.message || "OPPi runtime memory sync").trim() || "OPPi runtime memory sync",
  };
  if (params.remote) options.remote = String(params.remote).trim();
  if (params.passphrase) options.passphrase = String(params.passphrase);
  const repoPath = String(params.repoPath || "").trim();
  const rawResult = await module.syncGitRepository(root, repoPath, options) || {};
  let rebuiltEmbeddings = false;
  if ((direction === "pull" || direction === "both")
      && rawResult && rawResult.embeddingsRebuildRecommended
      && typeof module.rebuildEmbeddings === "function") {
    const rebuilt = await module.rebuildEmbeddings(root);
    rebuiltEmbeddings = !(rebuilt && rebuilt.error);
  }
  result = { ...rawResult, rebuiltEmbeddings };
} else {
  throw new Error(`Unsupported Hoppi backend operation: ${operation}`);
}
console.log(JSON.stringify(result || {}));
"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MemoryBackendPreference {
    Auto,
    Node,
    Embedded,
}

fn memory_backend_preference_from_env(
    raw: Option<&str>,
) -> Result<MemoryBackendPreference, String> {
    match raw.unwrap_or("auto").trim().to_ascii_lowercase().as_str() {
        "" | "auto" => Ok(MemoryBackendPreference::Auto),
        "node" | "node-typescript" | "typescript" => Ok(MemoryBackendPreference::Node),
        "embedded" | "rust" | "rust-embedded" => Ok(MemoryBackendPreference::Embedded),
        other => Err(format!(
            "unsupported OPPI_MEMORY_BACKEND={other}; expected auto, node, or embedded"
        )),
    }
}

#[derive(Debug, Clone)]
struct HoppiNodeBackendRunner {
    module_path: PathBuf,
    timeout: Duration,
}

impl HoppiNodeBackendRunner {
    fn new(module_path: PathBuf, timeout: Duration) -> Self {
        Self {
            module_path,
            timeout,
        }
    }

    #[cfg(test)]
    fn new_for_test(module_path: PathBuf, timeout: Duration) -> Self {
        Self::new(module_path, timeout)
    }

    fn run_operation(
        &self,
        operation: &str,
        thread: &Thread,
        params: Value,
    ) -> Result<MemoryBackendRawResult, RuntimeError> {
        let input = json!({
            "operation": operation,
            "modulePath": self.module_path.to_string_lossy().to_string(),
            "threadId": &thread.id,
            "project": &thread.project,
            "params": params,
        });
        let stdout = run_hoppi_node_backend(input, self.timeout)?;
        let value = parse_hoppi_node_json(&stdout)?;
        Ok(MemoryBackendRawResult {
            backend: HOPPI_NODE_BACKEND_NAME.to_string(),
            adapter_kind: Some(HOPPI_NODE_BACKEND_ADAPTER_KIND.to_string()),
            value,
        })
    }
}

#[derive(Debug, Clone)]
struct EmbeddedHoppiBackendRunner {
    store: Arc<Mutex<EmbeddedMemoryStore>>,
}

impl EmbeddedHoppiBackendRunner {
    #[cfg(test)]
    fn new_for_test() -> Self {
        Self {
            store: Arc::new(Mutex::new(EmbeddedMemoryStore::new_for_test())),
        }
    }

    fn open(path: PathBuf) -> Result<Self, RuntimeError> {
        let store = EmbeddedMemoryStore::open(path).map_err(embedded_memory_error)?;
        Ok(Self {
            store: Arc::new(Mutex::new(store)),
        })
    }

    fn lock_store(&self) -> Result<std::sync::MutexGuard<'_, EmbeddedMemoryStore>, RuntimeError> {
        self.store.lock().map_err(|_| {
            RuntimeError::new(
                "memory_backend_embedded_store_lock_poisoned",
                RuntimeErrorCategory::Provider,
                "embedded Hoppi memory store lock was poisoned",
            )
        })
    }

    fn project_id(thread: &Thread) -> String {
        let id = thread.project.id.trim();
        if id.is_empty() {
            thread.project.cwd.clone()
        } else {
            id.to_string()
        }
    }

    fn raw(value: Value) -> MemoryBackendRawResult {
        MemoryBackendRawResult {
            backend: HOPPI_EMBEDDED_BACKEND_NAME.to_string(),
            adapter_kind: Some(HOPPI_EMBEDDED_BACKEND_ADAPTER_KIND.to_string()),
            value,
        }
    }

    fn status_value(status: &EmbeddedMemoryStatus) -> Value {
        let mut status_value = json!({
            "enabled": true,
            "initialized": true,
            "backend": HOPPI_EMBEDDED_BACKEND_NAME,
            "scope": "project",
            "projectId": status.project_id,
            "memoryCount": status.memory_count,
            "pinnedCount": status.pinned_count,
        });
        if let Some(store_path) = &status.store_path {
            status_value["storePath"] = json!(store_path.display().to_string());
        }
        status_value
    }

    fn unavailable_operation(operation: &str) -> RuntimeError {
        RuntimeError::new(
            "memory_backend_embedded_operation_unavailable",
            RuntimeErrorCategory::Unavailable,
            format!("embedded Hoppi memory backend does not yet support {operation}"),
        )
    }
}

impl MemoryBackendRunner for EmbeddedHoppiBackendRunner {
    fn adapter_info(&self) -> MemoryBackendAdapterInfo {
        MemoryBackendAdapterInfo {
            id: HOPPI_EMBEDDED_BACKEND_NAME.to_string(),
            kind: HOPPI_EMBEDDED_BACKEND_ADAPTER_KIND.to_string(),
            embedded: true,
            operations: MemoryBackendOperation::ALL.to_vec(),
            diagnostics: Vec::new(),
        }
    }

    fn status(
        &self,
        thread: &Thread,
        _params: &MemoryBackendStatusParams,
    ) -> Result<MemoryBackendRawResult, RuntimeError> {
        let store = self.lock_store()?;
        let status = store.status(Self::project_id(thread));
        Ok(Self::raw(json!({
            "status": Self::status_value(&status)
        })))
    }

    fn recall(
        &self,
        thread: &Thread,
        params: &MemoryBackendRecallParams,
    ) -> Result<MemoryBackendRawResult, RuntimeError> {
        let store = self.lock_store()?;
        let recalled = store
            .recall(EmbeddedRecallInput {
                project_id: Self::project_id(thread),
                prompt: params.prompt.clone(),
                limit: 5,
            })
            .map_err(embedded_memory_error)?;
        let status = store.status(Self::project_id(thread));
        Ok(Self::raw(json!({
            "contextMarkdown": recalled.context_markdown,
            "records": embedded_records_json(&recalled.records),
            "status": Self::status_value(&status),
            "truncated": false
        })))
    }

    fn remember(
        &self,
        thread: &Thread,
        params: &MemoryBackendRememberParams,
    ) -> Result<MemoryBackendRawResult, RuntimeError> {
        let mut store = self.lock_store()?;
        let record = store
            .remember(EmbeddedRememberInput {
                project_id: Self::project_id(thread),
                content: params.content.clone(),
                tags: params.tags.clone(),
            })
            .map_err(embedded_memory_error)?;
        let status = store.status(Self::project_id(thread));
        Ok(Self::raw(json!({
            "saved": true,
            "record": embedded_record_json(&record),
            "status": Self::status_value(&status)
        })))
    }

    fn list(
        &self,
        thread: &Thread,
        params: &MemoryBackendListParams,
    ) -> Result<MemoryBackendRawResult, RuntimeError> {
        let store = self.lock_store()?;
        let records = store
            .list(EmbeddedListInput {
                project_id: Self::project_id(thread),
                query: params.query.clone(),
                limit: params.limit.map(|limit| limit as usize),
                pinned_only: params.pinned_only,
                include_superseded: params.include_superseded,
            })
            .map_err(embedded_memory_error)?;
        let status = store.status(Self::project_id(thread));
        Ok(Self::raw(json!({
            "records": embedded_records_json(&records),
            "status": Self::status_value(&status)
        })))
    }

    fn dashboard(
        &self,
        _thread: &Thread,
        _params: &MemoryBackendDashboardParams,
    ) -> Result<MemoryBackendRawResult, RuntimeError> {
        Err(Self::unavailable_operation("dashboard"))
    }

    fn forget(
        &self,
        thread: &Thread,
        _params: &MemoryBackendForgetParams,
    ) -> Result<MemoryBackendRawResult, RuntimeError> {
        let mut store = self.lock_store()?;
        let project_id = Self::project_id(thread);
        let removed = store
            .forget(&project_id, &_params.id)
            .map_err(embedded_memory_error)?;
        let status = store.status(project_id);
        Ok(Self::raw(json!({
            "id": _params.id,
            "deleted": removed.is_some(),
            "status": Self::status_value(&status)
        })))
    }

    fn pin(
        &self,
        thread: &Thread,
        params: &MemoryBackendPinParams,
    ) -> Result<MemoryBackendRawResult, RuntimeError> {
        let mut store = self.lock_store()?;
        let project_id = Self::project_id(thread);
        let record = store
            .pin(&project_id, &params.id, params.pinned)
            .map_err(embedded_memory_error)?;
        let status = store.status(project_id);
        Ok(Self::raw(json!({
            "id": params.id,
            "record": record.as_ref().map(embedded_record_json),
            "status": Self::status_value(&status)
        })))
    }

    fn update(
        &self,
        thread: &Thread,
        params: &MemoryBackendUpdateParams,
    ) -> Result<MemoryBackendRawResult, RuntimeError> {
        let mut store = self.lock_store()?;
        let project_id = Self::project_id(thread);
        let record = store
            .update(
                &project_id,
                &params.id,
                EmbeddedRecordPatch {
                    content: params.patch.content.clone(),
                    tags: params.patch.tags.clone(),
                    pinned: params.patch.pinned,
                    superseded_by: params.patch.superseded_by.clone(),
                },
            )
            .map_err(embedded_memory_error)?;
        let status = store.status(project_id);
        Ok(Self::raw(json!({
            "id": params.id,
            "record": record.as_ref().map(embedded_record_json),
            "status": Self::status_value(&status)
        })))
    }

    fn supersede(
        &self,
        thread: &Thread,
        params: &MemoryBackendSupersedeParams,
    ) -> Result<MemoryBackendRawResult, RuntimeError> {
        let mut store = self.lock_store()?;
        let project_id = Self::project_id(thread);
        let record = store
            .supersede(&project_id, &params.id, params.superseded_by.clone())
            .map_err(embedded_memory_error)?;
        let status = store.status(project_id);
        Ok(Self::raw(json!({
            "id": params.id,
            "record": record.as_ref().map(embedded_record_json),
            "status": Self::status_value(&status)
        })))
    }

    fn consolidate(
        &self,
        thread: &Thread,
        params: &MemoryBackendConsolidateParams,
    ) -> Result<MemoryBackendRawResult, RuntimeError> {
        let store = self.lock_store()?;
        let status = store.status(Self::project_id(thread));
        Ok(Self::raw(json!({
            "dryRun": params.dry_run,
            "backendApplied": !params.dry_run,
            "summary": "Embedded memory consolidation is currently a no-op.",
            "records": [],
            "deletedIds": [],
            "status": Self::status_value(&status)
        })))
    }

    fn sync(
        &self,
        _thread: &Thread,
        params: &MemoryBackendSyncParams,
    ) -> Result<MemoryBackendRawResult, RuntimeError> {
        Ok(Self::raw(json!({
            "imported": 0,
            "exported": 0,
            "committed": false,
            "pushed": false,
            "conflicts": [],
            "direction": params.direction,
            "rebuiltEmbeddings": false
        })))
    }
}

fn embedded_memory_error(error: EmbeddedMemoryError) -> RuntimeError {
    RuntimeError::new(
        "memory_backend_embedded_store_failed",
        RuntimeErrorCategory::Provider,
        format!("embedded Hoppi memory backend failed: {}", error.message()),
    )
}

fn embedded_record_json(record: &EmbeddedMemoryRecord) -> Value {
    let mut value = serde_json::to_value(record).unwrap_or_else(|_| json!({}));
    if let Some(object) = value.as_object_mut() {
        object.remove("projectId");
        object
            .entry("source".to_string())
            .or_insert(json!("oppi:embedded"));
    }
    value
}

fn embedded_records_json(records: &[EmbeddedMemoryRecord]) -> Vec<Value> {
    records.iter().map(embedded_record_json).collect()
}

impl MemoryBackendRunner for HoppiNodeBackendRunner {
    fn adapter_info(&self) -> MemoryBackendAdapterInfo {
        MemoryBackendAdapterInfo {
            id: HOPPI_NODE_BACKEND_NAME.to_string(),
            kind: HOPPI_NODE_BACKEND_ADAPTER_KIND.to_string(),
            embedded: false,
            operations: MemoryBackendOperation::ALL.to_vec(),
            diagnostics: Vec::new(),
        }
    }

    fn status(
        &self,
        thread: &Thread,
        params: &MemoryBackendStatusParams,
    ) -> Result<MemoryBackendRawResult, RuntimeError> {
        self.run_operation("status", thread, json!({ "threadId": &params.thread_id }))
    }

    fn recall(
        &self,
        thread: &Thread,
        params: &MemoryBackendRecallParams,
    ) -> Result<MemoryBackendRawResult, RuntimeError> {
        self.run_operation(
            "recall",
            thread,
            json!({
                "threadId": &params.thread_id,
            "prompt": &params.prompt,
            "includeStartup": params.include_startup,
            "includeTask": params.include_task,
            }),
        )
    }

    fn remember(
        &self,
        thread: &Thread,
        params: &MemoryBackendRememberParams,
    ) -> Result<MemoryBackendRawResult, RuntimeError> {
        self.run_operation(
            "remember",
            thread,
            serde_json::to_value(params).unwrap_or_default(),
        )
    }

    fn list(
        &self,
        thread: &Thread,
        params: &MemoryBackendListParams,
    ) -> Result<MemoryBackendRawResult, RuntimeError> {
        self.run_operation(
            "list",
            thread,
            serde_json::to_value(params).unwrap_or_default(),
        )
    }

    fn dashboard(
        &self,
        thread: &Thread,
        params: &MemoryBackendDashboardParams,
    ) -> Result<MemoryBackendRawResult, RuntimeError> {
        self.run_operation(
            "dashboard",
            thread,
            json!({ "threadId": &params.thread_id }),
        )
    }

    fn forget(
        &self,
        thread: &Thread,
        params: &MemoryBackendForgetParams,
    ) -> Result<MemoryBackendRawResult, RuntimeError> {
        self.run_operation(
            "forget",
            thread,
            serde_json::to_value(params).unwrap_or_default(),
        )
    }

    fn pin(
        &self,
        thread: &Thread,
        params: &MemoryBackendPinParams,
    ) -> Result<MemoryBackendRawResult, RuntimeError> {
        self.run_operation(
            "pin",
            thread,
            serde_json::to_value(params).unwrap_or_default(),
        )
    }

    fn update(
        &self,
        thread: &Thread,
        params: &MemoryBackendUpdateParams,
    ) -> Result<MemoryBackendRawResult, RuntimeError> {
        self.run_operation(
            "update",
            thread,
            serde_json::to_value(params).unwrap_or_default(),
        )
    }

    fn supersede(
        &self,
        thread: &Thread,
        params: &MemoryBackendSupersedeParams,
    ) -> Result<MemoryBackendRawResult, RuntimeError> {
        self.run_operation(
            "supersede",
            thread,
            serde_json::to_value(params).unwrap_or_default(),
        )
    }

    fn consolidate(
        &self,
        thread: &Thread,
        params: &MemoryBackendConsolidateParams,
    ) -> Result<MemoryBackendRawResult, RuntimeError> {
        self.run_operation(
            "consolidate",
            thread,
            serde_json::to_value(params).unwrap_or_default(),
        )
    }

    fn sync(
        &self,
        thread: &Thread,
        params: &MemoryBackendSyncParams,
    ) -> Result<MemoryBackendRawResult, RuntimeError> {
        self.run_operation(
            "sync",
            thread,
            serde_json::to_value(params).unwrap_or_default(),
        )
    }
}

#[derive(Clone)]
struct ServerState {
    runtime: Arc<Mutex<Runtime>>,
    event_mirror: Arc<Mutex<Vec<Event>>>,
    turn_interrupts: TurnInterruptRegistry,
    persistent_store: Option<Arc<Mutex<FilesystemEventStore>>>,
    persisted_event_id: Arc<Mutex<EventId>>,
}

impl ServerState {
    fn new() -> Self {
        Self::from_env().unwrap_or_else(|error| {
            eprintln!("oppi-server persistence disabled: {error}");
            Self::without_persistence()
        })
    }

    fn without_persistence() -> Self {
        let event_mirror = Arc::new(Mutex::new(Vec::new()));
        let mut runtime = Runtime::new();
        configure_default_memory_backend_runner(&mut runtime);
        runtime.set_event_mirror(Some(event_mirror.clone()));
        let turn_interrupts = runtime.turn_interrupt_registry();
        Self {
            runtime: Arc::new(Mutex::new(runtime)),
            event_mirror,
            turn_interrupts,
            persistent_store: None,
            persisted_event_id: Arc::new(Mutex::new(0)),
        }
    }

    fn from_env() -> Result<Self, String> {
        let Some(root) = runtime_store_root_from_env() else {
            return Ok(Self::without_persistence());
        };
        let namespace = StoreNamespace::new(
            std::env::var("OPPI_RUNTIME_STORE_PROJECT_ID")
                .ok()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| "oppi-shell".to_string()),
            std::env::var("OPPI_RUNTIME_STORE_ID")
                .ok()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| "default".to_string()),
        );
        Self::with_persistence(root, namespace)
    }

    fn with_persistence(root: PathBuf, namespace: StoreNamespace) -> Result<Self, String> {
        let store = FilesystemEventStore::with_namespace(root, namespace)
            .map_err(|error| format!("open event store: {error:?}"))?;
        let events = store
            .list_all_events()
            .map_err(|error| format!("load persisted events: {error:?}"))?;
        let mut runtime = if events.is_empty() {
            Runtime::new()
        } else {
            Runtime::replay_events(&events).map_err(|error| error.message)?
        };
        configure_default_memory_backend_runner(&mut runtime);
        runtime.reserve_thread_counter(
            store
                .max_thread_counter_hint()
                .map_err(|error| format!("scan persisted thread ids: {error:?}"))?,
        );
        let event_mirror = Arc::new(Mutex::new(events.clone()));
        runtime.set_event_mirror(Some(event_mirror.clone()));
        let recover = runtime.recover_incomplete_turns("server restarted before turn completed");
        let turn_interrupts = runtime.turn_interrupt_registry();
        let last_loaded = events.iter().map(|event| event.id).max().unwrap_or(0);
        let state = Self {
            runtime: Arc::new(Mutex::new(runtime)),
            event_mirror,
            turn_interrupts,
            persistent_store: Some(Arc::new(Mutex::new(store))),
            persisted_event_id: Arc::new(Mutex::new(last_loaded)),
        };
        if !recover.events.is_empty() {
            state.persist_new_events().map_err(|error| error.message)?;
        }
        Ok(state)
    }

    fn events_after_mirror(
        &self,
        params: EventsListParams,
    ) -> Result<EventsListResult, JsonRpcError> {
        let limit = params
            .limit
            .unwrap_or(SERVER_EVENTS_LIST_DEFAULT_LIMIT)
            .min(SERVER_EVENTS_LIST_MAX_LIMIT);
        let events = self
            .event_mirror
            .lock()
            .map_err(|_| json_rpc_internal("event mirror lock poisoned"))?
            .iter()
            .filter(|event| event.thread_id == params.thread_id && event.id > params.after)
            .take(limit)
            .cloned()
            .collect();
        Ok(EventsListResult { events })
    }

    fn run_agentic_background(
        &self,
        mut params: AgenticTurnParams,
    ) -> Result<AgenticTurnResult, JsonRpcError> {
        let thread_id = params.thread_id.clone();
        let after = self.last_mirrored_event_id()?;
        params.execution_mode = ExecutionMode::Blocking;
        let runtime = self.runtime.clone();
        let result_slot: Arc<Mutex<Option<Result<AgenticTurnResult, RuntimeError>>>> =
            Arc::new(Mutex::new(None));
        let result_slot_for_thread = result_slot.clone();
        thread::spawn(move || {
            let result = match runtime.lock() {
                Ok(mut runtime) => runtime.run_agentic_turn(params),
                Err(_) => Err(RuntimeError::new(
                    "runtime_lock_poisoned",
                    RuntimeErrorCategory::EventStore,
                    "runtime lock poisoned while running background turn",
                )),
            };
            if let Ok(mut slot) = result_slot_for_thread.lock() {
                *slot = Some(result);
            }
        });

        let started = self.wait_for_background_turn_start(&thread_id, after, &result_slot)?;
        let events = self
            .events_after_mirror(EventsListParams {
                thread_id,
                after,
                limit: None,
            })?
            .events;
        Ok(agentic_result_from_background_events(started, events))
    }

    fn run_agentic_blocking_with_background_agents(
        &self,
        params: AgenticTurnParams,
    ) -> Result<AgenticTurnResult, JsonRpcError> {
        let (result, pending) = {
            let mut runtime = self
                .runtime
                .lock()
                .map_err(|_| json_rpc_internal("runtime lock poisoned"))?;
            let result = runtime.run_agentic_turn(params).map_err(runtime_error)?;
            let pending = runtime.take_pending_background_agent_requests();
            (result, pending)
        };
        for request in pending {
            self.spawn_background_agent_request(request);
        }
        Ok(result)
    }

    fn spawn_background_agent_request(&self, request: BackgroundAgentRequest) {
        let runtime = self.runtime.clone();
        thread::spawn(move || {
            let result = match runtime.lock() {
                Ok(mut runtime) => runtime.run_pending_background_agent(request),
                Err(_) => Err(RuntimeError::new(
                    "runtime_lock_poisoned",
                    RuntimeErrorCategory::EventStore,
                    "runtime lock poisoned while running background agent",
                )),
            };
            if let Err(error) = result {
                eprintln!("background agent failed: {}", error.message);
            }
        });
    }

    fn request_background_interrupt(
        &self,
        params: TurnInterruptParams,
    ) -> Result<EventsListResult, JsonRpcError> {
        let after = self.last_mirrored_event_id()?;
        self.turn_interrupts
            .request(params.turn_id.clone(), params.reason)
            .map_err(json_rpc_internal)?;
        let started = Instant::now();
        loop {
            let result = self.events_after_mirror(EventsListParams {
                thread_id: params.thread_id.clone(),
                after,
                limit: None,
            })?;
            let observed = result.events.iter().any(|event| {
                event.turn_id.as_deref() == Some(params.turn_id.as_str())
                    && matches!(
                        event.kind,
                        EventKind::TurnInterrupted { .. } | EventKind::TurnAborted { .. }
                    )
            });
            if observed || started.elapsed() >= BACKGROUND_TURN_INTERRUPT_TIMEOUT {
                return Ok(result);
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn wait_for_background_turn_start(
        &self,
        thread_id: &str,
        after: EventId,
        result_slot: &Arc<Mutex<Option<Result<AgenticTurnResult, RuntimeError>>>>,
    ) -> Result<Turn, JsonRpcError> {
        let started = Instant::now();
        loop {
            if let Some(turn) = self.find_turn_started_after(thread_id, after)? {
                return Ok(turn);
            }
            if let Some(result) = result_slot
                .lock()
                .map_err(|_| json_rpc_internal("background result lock poisoned"))?
                .as_ref()
            {
                match result {
                    Ok(result) => return Ok(result.turn.clone()),
                    Err(error) => return Err(runtime_error(error.clone())),
                }
            }
            if started.elapsed() >= BACKGROUND_TURN_START_TIMEOUT {
                return Err(JsonRpcError {
                    code: -32000,
                    message: "background turn did not start before timeout".to_string(),
                    data: Some(json!({
                        "code": "background_turn_start_timeout",
                        "category": "terminal_state"
                    })),
                });
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn find_turn_started_after(
        &self,
        thread_id: &str,
        after: EventId,
    ) -> Result<Option<Turn>, JsonRpcError> {
        Ok(self
            .event_mirror
            .lock()
            .map_err(|_| json_rpc_internal("event mirror lock poisoned"))?
            .iter()
            .filter(|event| event.thread_id == thread_id && event.id > after)
            .find_map(|event| match &event.kind {
                EventKind::TurnStarted { turn } => Some(turn.clone()),
                _ => None,
            }))
    }

    fn last_mirrored_event_id(&self) -> Result<EventId, JsonRpcError> {
        Ok(self
            .event_mirror
            .lock()
            .map_err(|_| json_rpc_internal("event mirror lock poisoned"))?
            .last()
            .map(|event| event.id)
            .unwrap_or(0))
    }

    fn persist_new_events(&self) -> Result<(), RuntimeError> {
        let Some(store) = &self.persistent_store else {
            return Ok(());
        };
        let last_persisted = *self.persisted_event_id.lock().map_err(|_| {
            RuntimeError::new(
                "persistence_lock_poisoned",
                RuntimeErrorCategory::EventStore,
                "persistence event cursor lock poisoned",
            )
        })?;
        let events = self
            .event_mirror
            .lock()
            .map_err(|_| {
                RuntimeError::new(
                    "event_mirror_lock_poisoned",
                    RuntimeErrorCategory::EventStore,
                    "event mirror lock poisoned while persisting events",
                )
            })?
            .iter()
            .filter(|event| event.id > last_persisted)
            .cloned()
            .collect::<Vec<_>>();
        if events.is_empty() {
            return Ok(());
        }
        let mut max_id = last_persisted;
        let mut store = store.lock().map_err(|_| {
            RuntimeError::new(
                "persistence_store_lock_poisoned",
                RuntimeErrorCategory::EventStore,
                "persistent event store lock poisoned",
            )
        })?;
        for event in events {
            max_id = max_id.max(event.id);
            store.append(event).map_err(|error| {
                RuntimeError::new(
                    "persistent_event_append_failed",
                    RuntimeErrorCategory::EventStore,
                    format!("persist event failed: {error:?}"),
                )
            })?;
        }
        *self.persisted_event_id.lock().map_err(|_| {
            RuntimeError::new(
                "persistence_lock_poisoned",
                RuntimeErrorCategory::EventStore,
                "persistence event cursor lock poisoned",
            )
        })? = max_id;
        Ok(())
    }
}

fn runtime_store_root_from_env() -> Option<PathBuf> {
    std::env::var("OPPI_RUNTIME_STORE_DIR")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn configure_default_memory_backend_runner(runtime: &mut Runtime) {
    match memory_backend_preference_from_env(std::env::var("OPPI_MEMORY_BACKEND").ok().as_deref()) {
        Ok(preference) => configure_memory_backend_runner_for_preference(runtime, preference),
        Err(message) => {
            eprintln!("oppi-server memory backend disabled: {message}");
        }
    }
}

fn configure_memory_backend_runner_for_preference(
    runtime: &mut Runtime,
    preference: MemoryBackendPreference,
) {
    match preference {
        MemoryBackendPreference::Auto | MemoryBackendPreference::Node => {
            if let Some(module_path) = hoppi_candidate_module_paths()
                .into_iter()
                .find(|path| path.exists())
            {
                runtime.set_memory_backend_runner(Box::new(HoppiNodeBackendRunner::new(
                    module_path,
                    Duration::from_secs(120),
                )));
            }
        }
        MemoryBackendPreference::Embedded => {
            let path = oppi_home_dir().join("memory").join("embedded-memory.json");
            match EmbeddedHoppiBackendRunner::open(path) {
                Ok(runner) => runtime.set_memory_backend_runner(Box::new(runner)),
                Err(error) => {
                    eprintln!(
                        "oppi-server embedded memory backend disabled: {}",
                        error.message
                    );
                }
            }
        }
    }
}

fn hoppi_candidate_module_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(path) = std::env::var_os("OPPI_HOPPI_MODULE")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .map(resolve_optional_module_path)
    {
        paths.push(path);
    }
    paths.push(managed_hoppi_module_path_for(HOPPI_PACKAGE_NAME));
    paths.push(managed_hoppi_module_path_for(HOPPI_LEGACY_PACKAGE_NAME));
    if let Some(path) = local_sibling_hoppi_module_path() {
        paths.push(path);
    }
    paths
}

fn managed_hoppi_module_path_for(package_name: &str) -> PathBuf {
    let mut path = oppi_home_dir().join("packages").join("node_modules");
    for part in package_name.split('/') {
        path.push(part);
    }
    path.join("dist").join("index.js")
}

fn local_sibling_hoppi_module_path() -> Option<PathBuf> {
    std::env::current_dir()
        .ok()
        .and_then(|cwd| cwd.parent().map(|parent| parent.to_path_buf()))
        .map(|parent| parent.join("hoppi-memory").join("dist").join("index.js"))
}

fn resolve_optional_module_path(path: PathBuf) -> PathBuf {
    let text = path.display().to_string();
    if let Some(stripped) = text.strip_prefix("~/").or_else(|| text.strip_prefix("~\\")) {
        return home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(stripped);
    }
    if path.is_absolute() {
        path
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    }
}

fn oppi_home_dir() -> PathBuf {
    std::env::var_os("OPPI_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".oppi")
        })
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        })
}

fn run_hoppi_node_backend(input: Value, timeout: Duration) -> Result<Vec<u8>, RuntimeError> {
    let mut child = Command::new(hoppi_node_binary())
        .arg("--input-type=module")
        .arg("-e")
        .arg(HOPPI_NODE_BACKEND_SCRIPT)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            RuntimeError::new(
                "memory_backend_node_spawn_failed",
                RuntimeErrorCategory::Provider,
                format!("failed to start Hoppi memory backend process: {error}"),
            )
        })?;

    let stdout = child.stdout.take().ok_or_else(|| {
        RuntimeError::new(
            "memory_backend_node_stdout_unavailable",
            RuntimeErrorCategory::Provider,
            "Hoppi memory backend stdout pipe was unavailable",
        )
    })?;
    let stderr = child.stderr.take().ok_or_else(|| {
        RuntimeError::new(
            "memory_backend_node_stderr_unavailable",
            RuntimeErrorCategory::Provider,
            "Hoppi memory backend stderr pipe was unavailable",
        )
    })?;
    let stdout_reader = read_child_pipe(stdout);
    let stderr_reader = read_child_pipe(stderr);

    {
        let mut stdin = child.stdin.take().ok_or_else(|| {
            RuntimeError::new(
                "memory_backend_node_stdin_unavailable",
                RuntimeErrorCategory::Provider,
                "Hoppi memory backend stdin pipe was unavailable",
            )
        })?;
        serde_json::to_writer(&mut stdin, &input).map_err(|error| {
            RuntimeError::new(
                "memory_backend_node_input_serialize_failed",
                RuntimeErrorCategory::Provider,
                format!("failed to serialize Hoppi memory backend input: {error}"),
            )
        })?;
        stdin.write_all(b"\n").map_err(|error| {
            RuntimeError::new(
                "memory_backend_node_input_write_failed",
                RuntimeErrorCategory::Provider,
                format!("failed to write Hoppi memory backend input: {error}"),
            )
        })?;
    }

    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait().map_err(|error| {
            RuntimeError::new(
                "memory_backend_node_wait_failed",
                RuntimeErrorCategory::Provider,
                format!("failed to wait for Hoppi memory backend process: {error}"),
            )
        })? {
            let stdout = join_child_pipe(stdout_reader);
            let stderr = join_child_pipe(stderr_reader);
            if status.success() {
                return Ok(stdout);
            }
            return Err(RuntimeError::new(
                "memory_backend_node_failed",
                RuntimeErrorCategory::Provider,
                format!(
                    "Hoppi memory backend process exited with status {}; stderr bytes: {}",
                    status,
                    stderr.len()
                ),
            ));
        }

        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            let _ = join_child_pipe(stdout_reader);
            let _ = join_child_pipe(stderr_reader);
            return Err(RuntimeError::new(
                "memory_backend_node_timeout",
                RuntimeErrorCategory::Provider,
                format!(
                    "Hoppi memory backend process timed out after {} ms",
                    timeout.as_millis()
                ),
            ));
        }

        thread::sleep(Duration::from_millis(10));
    }
}

fn hoppi_node_binary() -> &'static str {
    if cfg!(windows) { "node.exe" } else { "node" }
}

fn read_child_pipe<R>(mut pipe: R) -> thread::JoinHandle<io::Result<Vec<u8>>>
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let mut output = Vec::new();
        pipe.read_to_end(&mut output)?;
        Ok(output)
    })
}

fn join_child_pipe(handle: thread::JoinHandle<io::Result<Vec<u8>>>) -> Vec<u8> {
    handle.join().ok().and_then(Result::ok).unwrap_or_default()
}

fn parse_hoppi_node_json(stdout: &[u8]) -> Result<Value, RuntimeError> {
    let stdout = std::str::from_utf8(stdout).map_err(|error| {
        RuntimeError::new(
            "memory_backend_node_stdout_utf8_failed",
            RuntimeErrorCategory::Provider,
            format!("Hoppi memory backend returned non-UTF-8 output: {error}"),
        )
    })?;
    let line = stdout
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .ok_or_else(|| {
            RuntimeError::new(
                "memory_backend_node_stdout_empty",
                RuntimeErrorCategory::Provider,
                "Hoppi memory backend returned no JSON output",
            )
        })?;
    serde_json::from_str(line).map_err(|error| {
        RuntimeError::new(
            "memory_backend_node_stdout_parse_failed",
            RuntimeErrorCategory::Provider,
            format!("Hoppi memory backend returned invalid JSON output: {error}"),
        )
    })
}

fn agentic_result_from_background_events(mut turn: Turn, events: Vec<Event>) -> AgenticTurnResult {
    let mut awaiting_approval = None;
    let mut awaiting_question = None;
    for event in &events {
        if event.turn_id.as_deref() != Some(turn.id.as_str()) {
            continue;
        }
        match &event.kind {
            EventKind::TurnPhaseChanged { phase } => turn.phase = *phase,
            EventKind::TurnCompleted { .. } => turn.status = TurnStatus::Completed,
            EventKind::TurnInterrupted { .. } | EventKind::TurnAborted { .. } => {
                turn.status = TurnStatus::Aborted;
            }
            EventKind::ApprovalRequested { request } => {
                turn.status = TurnStatus::WaitingForApproval;
                awaiting_approval = Some(request.clone());
            }
            EventKind::AskUserRequested { request } => {
                turn.status = TurnStatus::WaitingForUser;
                awaiting_question = Some(request.clone());
            }
            _ => {}
        }
    }
    AgenticTurnResult {
        turn,
        events,
        awaiting_approval,
        awaiting_question,
    }
}

fn json_rpc_internal(message: impl Into<String>) -> JsonRpcError {
    JsonRpcError {
        code: -32603,
        message: message.into(),
        data: Some(json!({ "code": "internal_error" })),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!(
            "oppi-server --stdio\n\nExperimental OPPi JSON-RPC server. Reads one JSON-RPC request per stdin line and writes one response per stdout line."
        );
        return;
    }
    if args.iter().any(|arg| arg == "--version" || arg == "-V") {
        println!("{}", env!("CARGO_PKG_VERSION"));
        return;
    }
    if !args.is_empty() && !args.iter().any(|arg| arg == "--stdio") {
        eprintln!("unknown arguments: {}", args.join(" "));
        std::process::exit(2);
    }

    if let Err(error) = run_stdio() {
        eprintln!("oppi-server failed: {error}");
        std::process::exit(1);
    }
}

fn run_stdio() -> io::Result<()> {
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    let state = ServerState::new();

    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let (response, action) = response_for_line_with_state_action(&state, &line);
        writeln!(
            stdout,
            "{}",
            serde_json::to_string(&response).expect("serialize JSON-RPC response")
        )?;
        stdout.flush()?;
        if action == StdioAction::Shutdown {
            break;
        }
    }

    Ok(())
}

#[cfg(test)]
fn response_for_line(runtime: &mut Runtime, line: &str) -> JsonRpcResponse {
    response_for_line_with_action(runtime, line).0
}

#[cfg(test)]
fn response_for_line_with_state(state: &ServerState, line: &str) -> JsonRpcResponse {
    response_for_line_with_state_action(state, line).0
}

fn response_for_line_with_state_action(
    state: &ServerState,
    line: &str,
) -> (JsonRpcResponse, StdioAction) {
    match serde_json::from_str::<JsonRpcRequest>(line) {
        Ok(request) => {
            let shutdown_requested = request.method == "server/shutdown";
            let request_id = request.id.clone();
            let mut response = handle_state_request(state, request);
            if response.error.is_none()
                && let Err(error) = state.persist_new_events()
            {
                response = JsonRpcResponse {
                    jsonrpc: "2.0",
                    id: request_id,
                    result: None,
                    error: Some(runtime_error(error)),
                };
            }
            let action = if shutdown_requested && response.error.is_none() {
                StdioAction::Shutdown
            } else {
                StdioAction::Continue
            };
            (response, action)
        }
        Err(error) => (
            JsonRpcResponse {
                jsonrpc: "2.0",
                id: Value::Null,
                result: None,
                error: Some(JsonRpcError {
                    code: -32700,
                    message: "parse error".to_string(),
                    data: Some(json!({ "detail": error.to_string() })),
                }),
            },
            StdioAction::Continue,
        ),
    }
}

#[cfg(test)]
fn response_for_line_with_action(
    runtime: &mut Runtime,
    line: &str,
) -> (JsonRpcResponse, StdioAction) {
    match serde_json::from_str::<JsonRpcRequest>(line) {
        Ok(request) => {
            let shutdown_requested = request.method == "server/shutdown";
            let response = handle_request(runtime, request);
            let action = if shutdown_requested && response.error.is_none() {
                StdioAction::Shutdown
            } else {
                StdioAction::Continue
            };
            (response, action)
        }
        Err(error) => (
            JsonRpcResponse {
                jsonrpc: "2.0",
                id: Value::Null,
                result: None,
                error: Some(JsonRpcError {
                    code: -32700,
                    message: "parse error".to_string(),
                    data: Some(json!({ "detail": error.to_string() })),
                }),
            },
            StdioAction::Continue,
        ),
    }
}

fn handle_state_request(state: &ServerState, request: JsonRpcRequest) -> JsonRpcResponse {
    let required_token = std::env::var("OPPI_SERVER_AUTH_TOKEN")
        .ok()
        .filter(|token| !token.is_empty());
    handle_state_request_with_auth(state, request, required_token.as_deref())
}

fn handle_state_request_with_auth(
    state: &ServerState,
    request: JsonRpcRequest,
    required_token: Option<&str>,
) -> JsonRpcResponse {
    let id = request.id.clone();
    match catch_unwind(AssertUnwindSafe(|| {
        handle_state_request_with_auth_inner(state, request, required_token)
    })) {
        Ok(response) => response,
        Err(_) => JsonRpcResponse {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(JsonRpcError {
                code: -32603,
                message: "internal error".to_string(),
                data: Some(json!({ "code": "internal_panic" })),
            }),
        },
    }
}

fn handle_state_request_with_auth_inner(
    state: &ServerState,
    request: JsonRpcRequest,
    required_token: Option<&str>,
) -> JsonRpcResponse {
    let id = request.id.clone();
    if matches!(
        request.method.as_str(),
        "sandbox/exec" | "sandbox/windows-wfp-install"
    ) && required_token.is_none()
    {
        return JsonRpcResponse {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(JsonRpcError {
                code: -32001,
                message: format!("{} requires OPPI_SERVER_AUTH_TOKEN", request.method),
                data: Some(json!({ "code": "auth_required" })),
            }),
        };
    }
    if let Some(token) = required_token
        && request.method != "initialize"
        && !request_has_valid_auth_token(&request.params, token)
    {
        return JsonRpcResponse {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(JsonRpcError {
                code: -32001,
                message: "authentication required".to_string(),
                data: Some(json!({ "code": "auth_required" })),
            }),
        };
    }

    let result = match request.method.as_str() {
        "events/list" => decode::<EventsListParams>(request.params)
            .and_then(|params| match state.runtime.try_lock() {
                Ok(runtime) => runtime.events_after(params).map_err(runtime_error),
                Err(std::sync::TryLockError::WouldBlock) => state.events_after_mirror(params),
                Err(std::sync::TryLockError::Poisoned(_)) => {
                    Err(json_rpc_internal("runtime lock poisoned"))
                }
            })
            .map(|result| json!(result)),
        "turn/run-agentic" => decode::<AgenticTurnParams>(request.params).and_then(|params| {
            if params.execution_mode == ExecutionMode::Background {
                state
                    .run_agentic_background(params)
                    .map(|result| json!(result))
            } else {
                state
                    .run_agentic_blocking_with_background_agents(params)
                    .map(|result| json!(result))
            }
        }),
        "turn/interrupt" => decode::<TurnInterruptParams>(request.params).and_then(|params| {
            match state.runtime.try_lock() {
                Ok(mut runtime) => runtime
                    .interrupt_turn(&params.thread_id, &params.turn_id, params.reason)
                    .map(|result| json!(result))
                    .map_err(runtime_error),
                Err(std::sync::TryLockError::WouldBlock) => state
                    .request_background_interrupt(params)
                    .map(|result| json!(result)),
                Err(std::sync::TryLockError::Poisoned(_)) => {
                    Err(json_rpc_internal("runtime lock poisoned"))
                }
            }
        }),
        _ => match state.runtime.lock() {
            Ok(mut runtime) => {
                return handle_request_with_auth_inner(&mut runtime, request, required_token);
            }
            Err(_) => Err(json_rpc_internal("runtime lock poisoned")),
        },
    };

    match result {
        Ok(value) => JsonRpcResponse {
            jsonrpc: "2.0",
            id,
            result: Some(value),
            error: None,
        },
        Err(error) => JsonRpcResponse {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(error),
        },
    }
}

#[cfg(test)]
fn handle_request(runtime: &mut Runtime, request: JsonRpcRequest) -> JsonRpcResponse {
    let required_token = std::env::var("OPPI_SERVER_AUTH_TOKEN")
        .ok()
        .filter(|token| !token.is_empty());
    handle_request_with_auth(runtime, request, required_token.as_deref())
}

#[cfg(test)]
fn handle_request_with_auth(
    runtime: &mut Runtime,
    request: JsonRpcRequest,
    required_token: Option<&str>,
) -> JsonRpcResponse {
    let id = request.id.clone();
    match catch_unwind(AssertUnwindSafe(|| {
        handle_request_with_auth_inner(runtime, request, required_token)
    })) {
        Ok(response) => response,
        Err(_) => JsonRpcResponse {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(JsonRpcError {
                code: -32603,
                message: "internal error".to_string(),
                data: Some(json!({ "code": "internal_panic" })),
            }),
        },
    }
}

fn handle_request_with_auth_inner(
    runtime: &mut Runtime,
    request: JsonRpcRequest,
    required_token: Option<&str>,
) -> JsonRpcResponse {
    let id = request.id;
    if matches!(
        request.method.as_str(),
        "sandbox/exec" | "sandbox/windows-wfp-install"
    ) && required_token.is_none()
    {
        return JsonRpcResponse {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(JsonRpcError {
                code: -32001,
                message: format!("{} requires OPPI_SERVER_AUTH_TOKEN", request.method),
                data: Some(json!({ "code": "auth_required" })),
            }),
        };
    }
    if let Some(token) = required_token
        && request.method != "initialize"
        && !request_has_valid_auth_token(&request.params, token)
    {
        return JsonRpcResponse {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(JsonRpcError {
                code: -32001,
                message: "authentication required".to_string(),
                data: Some(json!({ "code": "auth_required" })),
            }),
        };
    }
    let result = match request.method.as_str() {
        #[cfg(test)]
        "__test/panic" => panic!("intentional JSON-RPC panic-boundary test"),
        "initialize" => decode::<InitializeParams>(request.params)
            .map(|params| json!(runtime.initialize(params))),
        "server/shutdown" => Ok(json!({ "shuttingDown": true })),
        "runtime/metrics" => Ok(json!(runtime.metrics())),
        "debug/bundle" => {
            let sandbox = current_sandbox_status();
            let diagnostic = Diagnostic {
                level: DiagnosticLevel::Info,
                message: sandbox.message.clone(),
                metadata: BTreeMap::from([
                    ("component".to_string(), "sandbox".to_string()),
                    ("platform".to_string(), sandbox.platform),
                    (
                        "enforcement".to_string(),
                        serde_json::to_value(sandbox.enforcement)
                            .ok()
                            .and_then(|value| value.as_str().map(str::to_string))
                            .unwrap_or_else(|| "unknown".to_string()),
                    ),
                    ("supported".to_string(), sandbox.supported.to_string()),
                ]),
            };
            Ok(json!(runtime.debug_bundle(vec![diagnostic])))
        }
        "thread/list" => Ok(json!(runtime.list_threads())),
        "thread/items" => decode::<ThreadItemsParams>(request.params)
            .and_then(|params| runtime.list_thread_items(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "thread/item-labels/set" => decode::<ThreadItemLabelsUpdateParams>(request.params)
            .and_then(|params| {
                runtime
                    .set_thread_item_labels(params)
                    .map_err(runtime_error)
            })
            .map(|result| json!(result)),
        "thread/start" => decode::<ThreadStartParams>(request.params)
            .map(|params| json!(runtime.start_thread(params))),
        "thread/resume" => decode_thread_id(request.params)
            .and_then(|thread_id| runtime.resume_thread(&thread_id).map_err(runtime_error))
            .map(|result| json!(result)),
        "thread/fork" => decode::<ThreadForkParams>(request.params)
            .and_then(|params| {
                runtime
                    .fork_thread(&params.thread_id, params.title)
                    .map_err(runtime_error)
            })
            .map(|result| json!(result)),
        "thread/item-fork" => decode::<ThreadItemForkParams>(request.params)
            .and_then(|params| runtime.fork_thread_from_item(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "thread/item-edit-fork" => decode::<ThreadItemEditForkParams>(request.params)
            .and_then(|params| {
                runtime
                    .fork_thread_from_item_edit(params)
                    .map_err(runtime_error)
            })
            .map(|result| json!(result)),
        "thread/rename" => decode::<ThreadRenameParams>(request.params)
            .and_then(|params| {
                runtime
                    .rename_thread(&params.thread_id, params.title)
                    .map_err(runtime_error)
            })
            .map(|result| json!(result)),
        "thread/archive" | "thread/delete" => decode_thread_id(request.params)
            .and_then(|thread_id| runtime.archive_thread(&thread_id).map_err(runtime_error))
            .map(|result| json!(result)),
        "thread/goal/get" => decode::<ThreadGoalGetParams>(request.params)
            .and_then(|params| runtime.get_thread_goal(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "thread/goal/set" => decode::<ThreadGoalSetParams>(request.params)
            .and_then(|params| runtime.set_thread_goal(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "thread/goal/clear" => decode::<ThreadGoalClearParams>(request.params)
            .and_then(|params| runtime.clear_thread_goal(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "thread/goal/continuation" => decode::<ThreadGoalContinuationParams>(request.params)
            .and_then(|params| {
                runtime
                    .next_thread_goal_continuation(params)
                    .map_err(runtime_error)
            })
            .map(|result| json!(result)),
        "turn/start" => decode::<TurnStartParams>(request.params)
            .and_then(|params| runtime.start_turn(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "turn/run-agentic" => decode::<AgenticTurnParams>(request.params)
            .and_then(|params| runtime.run_agentic_turn(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "turn/resume-agentic" => decode::<AgenticTurnResumeParams>(request.params)
            .and_then(|params| runtime.resume_agentic_turn(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "turn/steer" => decode::<TurnSteerParams>(request.params)
            .and_then(|params| {
                runtime
                    .steer_turn(&params.thread_id, &params.turn_id, params.input)
                    .map_err(runtime_error)
            })
            .map(|result| json!(result)),
        "turn/interrupt" => decode::<TurnInterruptParams>(request.params)
            .and_then(|params| {
                runtime
                    .interrupt_turn(&params.thread_id, &params.turn_id, params.reason)
                    .map_err(runtime_error)
            })
            .map(|result| json!(result)),
        "tool/record" => decode::<ToolRecordParams>(request.params)
            .and_then(|params| runtime.record_tool(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "tool/batch" => decode::<ToolBatchRecordParams>(request.params)
            .and_then(|params| runtime.record_tool_batch(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "command/prepare" => decode::<CommandPrepareParams>(request.params)
            .and_then(|params| runtime.prepare_command(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "sandbox/status" => Ok(json!(current_sandbox_status())),
        "sandbox/windows-wfp-status" => {
            let status = windows_wfp_status();
            Ok(json!({
                "available": status.available,
                "filterCount": status.filter_count,
                "message": status.message,
            }))
        }
        "sandbox/windows-wfp-install" => decode::<WindowsWfpInstallParams>(request.params)
            .and_then(|params| {
                install_windows_wfp_filters_for_account(&params.account)
                    .map(|installed| json!({ "installed": installed, "account": params.account }))
                    .map_err(|message| JsonRpcError {
                        code: -32020,
                        message,
                        data: Some(json!({ "code": "windows_wfp_install_failed" })),
                    })
            }),
        "sandbox/plan" => decode::<SandboxPlanParams>(request.params).map(|params| {
            let manager = SandboxManager::detect();
            let transform = manager.transform(&params.policy, &params.request, params.preference);
            json!(SandboxPlanResult::from(transform))
        }),
        "sandbox/exec" => decode::<SandboxExecParams>(request.params)
            .map(|params| json!(execute_sandboxed(params))),
        "approval/request" => decode::<ApprovalRequestParams>(request.params)
            .and_then(|params| {
                runtime
                    .request_approval(&params.thread_id, params.request)
                    .map_err(runtime_error)
            })
            .map(|result| json!(result)),
        "approval/respond" => decode::<ApprovalRespondParams>(request.params)
            .and_then(|params| runtime.respond_approval(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "question/request" => decode::<QuestionRequestParams>(request.params)
            .and_then(|params| {
                runtime
                    .request_question(&params.thread_id, params.request)
                    .map_err(runtime_error)
            })
            .map(|result| json!(result)),
        "question/respond" => decode::<QuestionRespondParams>(request.params)
            .and_then(|params| runtime.respond_question(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "plugin/register" => decode::<PluginRegisterParams>(request.params)
            .and_then(|params| {
                runtime
                    .register_plugin(&params.thread_id, params.plugin)
                    .map_err(runtime_error)
            })
            .map(|result| json!(result)),
        "plugin/list" => {
            let params = if request.params.is_null() {
                Ok(PluginListParams::default())
            } else {
                decode::<PluginListParams>(request.params)
            };
            params
                .and_then(|params| runtime.list_plugins(params).map_err(runtime_error))
                .map(|result| json!(result))
        }
        "plugin/materialize" => decode::<PluginMaterializeParams>(request.params)
            .and_then(|params| {
                runtime
                    .materialize_plugin_status(params)
                    .map_err(runtime_error)
            })
            .map(|result| json!(result)),
        "background/list" => Ok(json!(runtime.list_background_tasks())),
        "background/read" => decode::<BackgroundReadParams>(request.params)
            .and_then(|params| runtime.read_background_task(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "background/kill" => decode::<BackgroundKillParams>(request.params)
            .and_then(|params| runtime.kill_background_task(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "memory/status" => Ok(json!(runtime.memory_status())),
        "memory/jobs/list" => {
            let params = if request.params.is_null() {
                Ok(MemoryJobsListParams {
                    thread_id: None,
                    limit: None,
                    include_finished: false,
                })
            } else {
                decode::<MemoryJobsListParams>(request.params)
            };
            params
                .and_then(|params| runtime.list_memory_jobs(params).map_err(runtime_error))
                .map(|result| json!(result))
        }
        "memory/jobs/update" => decode::<MemoryJobUpdateParams>(request.params)
            .and_then(|params| runtime.update_memory_job(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "memory/jobs/cancel" => decode::<MemoryJobCancelParams>(request.params)
            .and_then(|params| runtime.cancel_memory_job(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "memory/list" => decode::<MemoryListParams>(request.params)
            .and_then(|params| runtime.memory_list(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "memory/recall" => decode::<MemoryRecallParams>(request.params)
            .and_then(|params| runtime.memory_recall(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "memory/backend/status" => decode::<MemoryBackendStatusParams>(request.params)
            .and_then(|params| runtime.memory_backend_status(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "memory/backend/recall" => decode::<MemoryBackendRecallParams>(request.params)
            .and_then(|params| runtime.memory_backend_recall(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "memory/backend/remember" => decode::<MemoryBackendRememberParams>(request.params)
            .and_then(|params| {
                runtime
                    .memory_backend_remember(params)
                    .map_err(runtime_error)
            })
            .map(|result| json!(result)),
        "memory/backend/list" => decode::<MemoryBackendListParams>(request.params)
            .and_then(|params| runtime.memory_backend_list(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "memory/backend/dashboard" => decode::<MemoryBackendDashboardParams>(request.params)
            .and_then(|params| {
                runtime
                    .memory_backend_dashboard(params)
                    .map_err(runtime_error)
            })
            .map(|result| json!(result)),
        "memory/backend/forget" => decode::<MemoryBackendForgetParams>(request.params)
            .and_then(|params| runtime.memory_backend_forget(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "memory/backend/pin" => decode::<MemoryBackendPinParams>(request.params)
            .and_then(|params| runtime.memory_backend_pin(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "memory/backend/update" => decode::<MemoryBackendUpdateParams>(request.params)
            .and_then(|params| runtime.memory_backend_update(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "memory/backend/supersede" => decode::<MemoryBackendSupersedeParams>(request.params)
            .and_then(|params| {
                runtime
                    .memory_backend_supersede(params)
                    .map_err(runtime_error)
            })
            .map(|result| json!(result)),
        "memory/backend/consolidate" => decode::<MemoryBackendConsolidateParams>(request.params)
            .and_then(|params| {
                runtime
                    .memory_backend_consolidate(params)
                    .map_err(runtime_error)
            })
            .map(|result| json!(result)),
        "memory/backend/sync" => decode::<MemoryBackendSyncParams>(request.params)
            .and_then(|params| runtime.memory_backend_sync(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "memory/remember" => decode::<MemoryRememberParams>(request.params)
            .and_then(|params| runtime.memory_remember(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "memory/distill-turn" => decode::<MemoryDistillTurnParams>(request.params)
            .and_then(|params| runtime.memory_distill_turn(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "memory/recurring-candidates" => decode::<MemoryRecurringCandidatesParams>(request.params)
            .and_then(|params| {
                runtime
                    .memory_recurring_candidates(params)
                    .map_err(runtime_error)
            })
            .map(|result| json!(result)),
        "memory/consolidate" => decode::<MemoryConsolidateParams>(request.params)
            .and_then(|params| runtime.memory_consolidate(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "memory/records/set" => decode::<MemoryRecordsSetParams>(request.params)
            .and_then(|params| runtime.set_memory_records(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "memory/record/forget" => decode::<MemoryRecordForgetParams>(request.params)
            .and_then(|params| runtime.forget_memory_record(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "memory/record/pin" => decode::<MemoryRecordPinParams>(request.params)
            .and_then(|params| runtime.pin_memory_record(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "memory/record/update" => decode::<MemoryRecordUpdateParams>(request.params)
            .and_then(|params| runtime.update_memory_record(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "memory/record/supersede" => decode::<MemoryRecordSupersedeParams>(request.params)
            .and_then(|params| {
                runtime
                    .supersede_memory_record(params)
                    .map_err(runtime_error)
            })
            .map(|result| json!(result)),
        "memory/dashboard" => decode::<MemoryDashboardParams>(request.params)
            .and_then(|params| runtime.memory_dashboard(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "todos/list" => Ok(json!(runtime.todos_state())),
        "todos/client-action" => decode::<TodoClientActionParams>(request.params)
            .and_then(|params| {
                runtime
                    .apply_todo_client_action(params)
                    .map_err(runtime_error)
            })
            .map(|result| json!(result)),
        "memory/set" => decode::<MemorySetParams>(request.params)
            .and_then(|params| runtime.set_memory_status(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "memory/control" => decode::<MemoryControlParams>(request.params)
            .and_then(|params| runtime.memory_control(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "memory/compact" => decode::<HandoffCompactParams>(request.params)
            .and_then(|params| runtime.compact_handoff(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "permissions/history" => decode::<PermissionReviewHistoryParams>(request.params)
            .and_then(|params| {
                runtime
                    .permission_review_history(params)
                    .map_err(runtime_error)
            })
            .map(|result| json!(result)),
        "permissions/context" | "permissions/review-context" => {
            decode::<PermissionReviewContextParams>(request.params)
                .and_then(|params| {
                    runtime
                        .permission_review_context(params)
                        .map_err(runtime_error)
                })
                .map(|result| json!(result))
        }
        "permissions/clear-session" => decode::<PermissionReviewClearSessionParams>(request.params)
            .and_then(|params| {
                runtime
                    .clear_permission_review_session(params)
                    .map_err(runtime_error)
            })
            .map(|result| json!(result)),
        "permissions/settings" => Ok(json!(runtime.permission_review_settings())),
        "permissions/settings/update" => {
            decode::<PermissionReviewSettingsUpdateParams>(request.params)
                .and_then(|params| {
                    runtime
                        .update_permission_review_settings(params)
                        .map_err(runtime_error)
                })
                .map(|result| json!(result))
        }
        "ask-user/settings" => Ok(json!(runtime.ask_user_settings())),
        "ask-user/settings/update" => decode::<AskUserSettingsUpdateParams>(request.params)
            .and_then(|params| {
                runtime
                    .update_ask_user_settings(params)
                    .map_err(runtime_error)
            })
            .map(|result| json!(result)),
        "mcp/register" => decode::<McpRegisterParams>(request.params)
            .and_then(|params| {
                runtime
                    .register_mcp(&params.thread_id, params.server)
                    .map_err(runtime_error)
            })
            .map(|result| json!(result)),
        "mcp/list" => {
            let params = if request.params.is_null() {
                Ok(McpListParams::default())
            } else {
                decode::<McpListParams>(request.params)
            };
            params
                .and_then(|params| runtime.list_mcp(params).map_err(runtime_error))
                .map(|result| json!(result))
        }
        "plugin/mcp/trust" => decode::<PluginMcpTrustParams>(request.params)
            .and_then(|params| runtime.trust_plugin_mcp(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "mcp/action" => decode::<McpActionParams>(request.params)
            .and_then(|params| runtime.mcp_action(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "mcp/connect" => decode::<McpConnectParams>(request.params)
            .and_then(|params| runtime.connect_mcp(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "mcp/call-tool" => decode::<McpCallToolParams>(request.params)
            .and_then(|params| runtime.call_mcp_tool(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "mcp/resources/list" => decode::<McpListResourcesParams>(request.params)
            .and_then(|params| runtime.list_mcp_resources(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "mcp/resources/templates/list" => decode::<McpListResourceTemplatesParams>(request.params)
            .and_then(|params| {
                runtime
                    .list_mcp_resource_templates(params)
                    .map_err(runtime_error)
            })
            .map(|result| json!(result)),
        "mcp/prompts/list" => decode::<McpListPromptsParams>(request.params)
            .and_then(|params| runtime.list_mcp_prompts(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "mcp/prompts/get" => decode::<McpGetPromptParams>(request.params)
            .and_then(|params| runtime.get_mcp_prompt(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "mcp/resources/read" => decode::<McpReadResourceParams>(request.params)
            .and_then(|params| runtime.read_mcp_resource(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "mcp/resources/subscribe" => decode::<McpResourceSubscriptionParams>(request.params)
            .and_then(|params| {
                runtime
                    .subscribe_mcp_resource(params)
                    .map_err(runtime_error)
            })
            .map(|result| json!(result)),
        "mcp/resources/unsubscribe" => decode::<McpResourceSubscriptionParams>(request.params)
            .and_then(|params| {
                runtime
                    .unsubscribe_mcp_resource(params)
                    .map_err(runtime_error)
            })
            .map(|result| json!(result)),
        "mcp/resources/updated" => decode::<McpResourceUpdatedParams>(request.params)
            .and_then(|params| runtime.mcp_resource_updated(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "mcp/reload" => decode::<McpServerActionParams>(request.params)
            .map(|params| McpActionParams {
                thread_id: params.thread_id,
                server_id: params.server_id,
                action: McpAction::Reload,
            })
            .and_then(|params| runtime.mcp_action(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "mcp/test" => decode::<McpServerActionParams>(request.params)
            .map(|params| McpActionParams {
                thread_id: params.thread_id,
                server_id: params.server_id,
                action: McpAction::Test,
            })
            .and_then(|params| runtime.mcp_action(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "mcp/auth" => decode::<McpServerActionParams>(request.params)
            .map(|params| McpActionParams {
                thread_id: params.thread_id,
                server_id: params.server_id,
                action: McpAction::Auth,
            })
            .and_then(|params| runtime.mcp_action(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "model/register" => decode::<ModelRegisterParams>(request.params)
            .and_then(|params| {
                runtime
                    .register_model(&params.thread_id, params.model)
                    .map_err(runtime_error)
            })
            .map(|result| json!(result)),
        "model/list" => Ok(json!(runtime.list_models())),
        "model/registry" => decode_object::<ModelRegistryListParams>(request.params)
            .map(|params| runtime.list_model_registry(params))
            .map(|result| json!(result)),
        "model/select" => decode::<ModelSelectParams>(request.params)
            .and_then(|params| runtime.select_model(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "agents/register" => decode::<AgentRegisterParams>(request.params)
            .and_then(|params| {
                runtime
                    .register_agent(&params.thread_id, params.agent)
                    .map_err(runtime_error)
            })
            .map(|result| json!(result)),
        "agents/list" => decode::<AgentListParams>(request.params)
            .and_then(|params| runtime.list_agents(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "agents/show" => decode::<AgentShowParams>(request.params)
            .and_then(|params| runtime.show_agent(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "agents/create" => decode::<AgentWriteParams>(request.params)
            .map(|mut params| {
                params.overwrite = false;
                params
            })
            .and_then(|params| runtime.write_agent(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "agents/update" => decode::<AgentWriteParams>(request.params)
            .map(|mut params| {
                params.overwrite = true;
                params
            })
            .and_then(|params| runtime.write_agent(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "agents/delete" => decode::<AgentDeleteParams>(request.params)
            .and_then(|params| runtime.delete_agent(params).map_err(runtime_error))
            .map(|_| json!({ "ok": true })),
        "skills/list" => decode::<SkillListParams>(request.params)
            .and_then(|params| runtime.list_skills(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "skills/expand" => decode::<SkillExpandParams>(request.params)
            .and_then(|params| runtime.expand_skill_command(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "prompt-template/list" => decode::<PromptTemplateListParams>(request.params)
            .and_then(|params| runtime.list_prompt_templates(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "prompt-template/expand" => decode::<PromptTemplateExpandParams>(request.params)
            .and_then(|params| {
                runtime
                    .expand_prompt_template(params)
                    .map_err(runtime_error)
            })
            .map(|result| json!(result)),
        "theme/list" => decode::<ThemeListParams>(request.params)
            .and_then(|params| runtime.list_themes(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "theme/load" => decode::<ThemeLoadParams>(request.params)
            .and_then(|params| runtime.load_theme(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "agents/dispatch" => decode::<AgentDispatchParams>(request.params)
            .and_then(|params| runtime.dispatch_agent(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "agents/poll" => decode::<AgentRunPollParams>(request.params)
            .and_then(|params| runtime.poll_agent_run(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "agents/resume" => decode::<AgentRunResumeParams>(request.params)
            .and_then(|params| runtime.resume_agent_run(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "agents/block" => decode::<AgentBlockParams>(request.params)
            .and_then(|params| runtime.block_agent(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "agents/complete" => decode::<AgentCompleteParams>(request.params)
            .and_then(|params| runtime.complete_agent(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "side-question/ask" => decode::<SideQuestionParams>(request.params)
            .and_then(|params| runtime.side_question(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "pi/bridge-event" => decode::<PiBridgeEventParams>(request.params)
            .and_then(|params| runtime.bridge_pi_event(params).map_err(runtime_error))
            .map(|result| json!(result)),
        "events/list" => decode::<EventsListParams>(request.params)
            .and_then(|params| runtime.events_after(params).map_err(runtime_error))
            .map(|result| json!(result)),
        _ => Err(JsonRpcError {
            code: -32601,
            message: format!("method not found: {}", request.method),
            data: None,
        }),
    };

    match result {
        Ok(result) => JsonRpcResponse {
            jsonrpc: "2.0",
            id,
            result: Some(result),
            error: None,
        },
        Err(error) => JsonRpcResponse {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(error),
        },
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ThreadForkParams {
    thread_id: String,
    #[serde(default)]
    title: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ThreadRenameParams {
    thread_id: String,
    title: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TurnSteerParams {
    thread_id: String,
    turn_id: String,
    input: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TurnInterruptParams {
    thread_id: String,
    turn_id: String,
    reason: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApprovalRequestParams {
    thread_id: String,
    request: ApprovalRequest,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct QuestionRequestParams {
    thread_id: String,
    request: QuestionRequest,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PluginRegisterParams {
    thread_id: String,
    plugin: PluginRef,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct McpRegisterParams {
    thread_id: String,
    server: McpServerRef,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct McpServerActionParams {
    thread_id: String,
    server_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModelRegisterParams {
    thread_id: String,
    model: ModelRef,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentRegisterParams {
    thread_id: String,
    agent: AgentDefinition,
}

fn request_has_valid_auth_token(params: &Value, required_token: &str) -> bool {
    params
        .get("authToken")
        .and_then(Value::as_str)
        .is_some_and(|token| token == required_token)
}

fn decode<T>(value: Value) -> Result<T, JsonRpcError>
where
    T: for<'de> Deserialize<'de>,
{
    serde_json::from_value(value).map_err(|error| JsonRpcError {
        code: -32602,
        message: "invalid params".to_string(),
        data: Some(json!({ "detail": error.to_string() })),
    })
}

fn decode_object<T>(value: Value) -> Result<T, JsonRpcError>
where
    T: for<'de> Deserialize<'de>,
{
    if !value.is_object() {
        return Err(JsonRpcError {
            code: -32602,
            message: "invalid params".to_string(),
            data: Some(json!({ "detail": "expected object params" })),
        });
    }
    decode(value)
}

fn decode_thread_id(value: Value) -> Result<String, JsonRpcError> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Params {
        thread_id: String,
    }
    decode::<Params>(value).map(|params| params.thread_id)
}

fn runtime_error(error: RuntimeError) -> JsonRpcError {
    JsonRpcError {
        code: -32000,
        message: error.message,
        data: Some(json!({
            "code": error.code,
            "category": error.category.as_str(),
        })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store_root(name: &str) -> PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("{name}-{}-{unique}", std::process::id()))
    }

    fn server_valid_theme_json(name: &str) -> String {
        let mut colors = serde_json::Map::new();
        for token in [
            "accent",
            "border",
            "borderAccent",
            "borderMuted",
            "success",
            "error",
            "warning",
            "muted",
            "dim",
            "text",
            "thinkingText",
            "selectedBg",
            "userMessageBg",
            "userMessageText",
            "customMessageBg",
            "customMessageText",
            "customMessageLabel",
            "toolPendingBg",
            "toolSuccessBg",
            "toolErrorBg",
            "toolTitle",
            "toolOutput",
            "mdHeading",
            "mdLink",
            "mdLinkUrl",
            "mdCode",
            "mdCodeBlock",
            "mdCodeBlockBorder",
            "mdQuote",
            "mdQuoteBorder",
            "mdHr",
            "mdListBullet",
            "toolDiffAdded",
            "toolDiffRemoved",
            "toolDiffContext",
            "syntaxComment",
            "syntaxKeyword",
            "syntaxFunction",
            "syntaxVariable",
            "syntaxString",
            "syntaxNumber",
            "syntaxType",
            "syntaxOperator",
            "syntaxPunctuation",
            "thinkingOff",
            "thinkingMinimal",
            "thinkingLow",
            "thinkingMedium",
            "thinkingHigh",
            "thinkingXhigh",
            "bashMode",
        ] {
            colors.insert(token.to_string(), json!("#223344"));
        }
        colors.insert("accent".to_string(), json!("brand"));
        colors.insert("muted".to_string(), json!(242));
        colors.insert("text".to_string(), json!(""));
        json!({
            "name": name,
            "vars": {
                "brand": "#123abc",
                "page": "#010203"
            },
            "colors": colors,
            "export": { "pageBg": "page" }
        })
        .to_string()
    }

    fn write_prompt_theme_plugin(root: &std::path::Path) -> PathBuf {
        let plugin = root.join("plugin-pack");
        std::fs::create_dir_all(plugin.join("prompts")).unwrap();
        std::fs::create_dir_all(plugin.join("themes")).unwrap();
        std::fs::create_dir_all(root.join(".oppi")).unwrap();
        std::fs::write(
            plugin.join("package.json"),
            r#"{"name":"plugin-pack","version":"1.0.0","pi":{"prompts":["./prompts"],"themes":["./themes"]}}"#,
        )
        .unwrap();
        std::fs::write(
            plugin.join("prompts/component.md"),
            "---\ndescription: Create a component\nargument-hint: \"<name>\"\n---\nCreate component $1.\n",
        )
        .unwrap();
        std::fs::write(
            plugin.join("themes/acme.json"),
            server_valid_theme_json("acme"),
        )
        .unwrap();
        std::fs::write(
            root.join(".oppi/plugins.json"),
            format!(
                r#"{{
  "version": 1,
  "plugins": [
    {{
      "name": "plugin-pack",
      "source": {},
      "sourceType": "local",
      "enabled": true,
      "trusted": true
    }}
  ]
}}"#,
                serde_json::to_string(&plugin.display().to_string()).unwrap()
            ),
        )
        .unwrap();
        plugin
    }

    fn start_thread_for_root(runtime: &mut Runtime, root: &std::path::Path) {
        let start = handle_request(
            runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(1),
                method: "thread/start".to_string(),
                params: json!({ "project": { "id": "project", "cwd": root.display().to_string() } }),
            },
        );
        assert!(start.error.is_none(), "{:?}", start.error);
    }

    fn read_http_request(stream: &mut std::net::TcpStream) -> String {
        use std::io::Read;
        let mut bytes = Vec::new();
        let mut buffer = [0u8; 1024];
        loop {
            let read = stream.read(&mut buffer).unwrap();
            if read == 0 {
                break;
            }
            bytes.extend_from_slice(&buffer[..read]);
            if bytes.windows(4).any(|window| window == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&bytes).to_string();
                let content_length = headers
                    .lines()
                    .find_map(|line| line.strip_prefix("Content-Length:"))
                    .and_then(|value| value.trim().parse::<usize>().ok())
                    .unwrap_or(0);
                let header_end = bytes
                    .windows(4)
                    .position(|window| window == b"\r\n\r\n")
                    .map(|index| index + 4)
                    .unwrap_or(bytes.len());
                while bytes.len().saturating_sub(header_end) < content_length {
                    let read = stream.read(&mut buffer).unwrap();
                    if read == 0 {
                        break;
                    }
                    bytes.extend_from_slice(&buffer[..read]);
                }
                break;
            }
        }
        String::from_utf8_lossy(&bytes).to_string()
    }

    fn start_mock_openai_server(
        responses: Vec<String>,
    ) -> (String, std::thread::JoinHandle<Vec<String>>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            let mut requests = Vec::new();
            for response in responses {
                let (mut stream, _) = listener.accept().unwrap();
                requests.push(read_http_request(&mut stream));
                let reply = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    response.len(),
                    response
                );
                stream.write_all(reply.as_bytes()).unwrap();
            }
            requests
        });
        (format!("http://{address}/v1"), handle)
    }

    #[test]
    fn initialize_returns_protocol_metadata() {
        let mut runtime = Runtime::new();
        let request = JsonRpcRequest {
            jsonrpc: Some("2.0".to_string()),
            id: json!(1),
            method: "initialize".to_string(),
            params: json!({
                "clientName": "test",
                "protocolVersion": "0.1.0",
                "clientCapabilities": ["sandbox", "unknown-future-capability"]
            }),
        };
        let response = handle_request(&mut runtime, request);
        assert!(response.error.is_none());
        let result = response.result.unwrap();
        assert_eq!(result["serverName"], json!("oppi-server"));
        assert_eq!(result["protocolCompatible"], json!(true));
        assert_eq!(result["acceptedClientCapabilities"], json!(["sandbox"]));
    }

    #[test]
    fn initialize_reports_incompatible_future_major_protocol() {
        let mut runtime = Runtime::new();
        let request = JsonRpcRequest {
            jsonrpc: Some("2.0".to_string()),
            id: json!(1),
            method: "initialize".to_string(),
            params: json!({ "clientName": "test", "protocolVersion": "99.0.0" }),
        };
        let response = handle_request(&mut runtime, request);
        assert!(response.error.is_none());
        assert_eq!(response.result.unwrap()["protocolCompatible"], json!(false));
    }

    #[test]
    fn thread_goal_rpc_set_get_clear_round_trips() {
        let mut runtime = Runtime::new();
        let start = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(1),
                method: "thread/start".to_string(),
                params: json!({ "project": { "id": "project", "cwd": "/repo" } }),
            },
        );
        let thread_id = start.result.unwrap()["thread"]["id"]
            .as_str()
            .unwrap()
            .to_string();

        let set = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(2),
                method: "thread/goal/set".to_string(),
                params: json!({
                    "threadId": thread_id,
                    "objective": "Ship /goal",
                    "status": "active",
                    "tokenBudget": 10_000
                }),
            },
        );
        assert!(set.error.is_none(), "{:?}", set.error);
        assert_eq!(
            set.result.unwrap()["goal"]["objective"],
            json!("Ship /goal")
        );

        let get = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(3),
                method: "thread/goal/get".to_string(),
                params: json!({ "threadId": thread_id }),
            },
        );
        assert_eq!(get.result.unwrap()["goal"]["tokenBudget"], json!(10_000));

        let clear = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(4),
                method: "thread/goal/clear".to_string(),
                params: json!({ "threadId": thread_id }),
            },
        );
        assert_eq!(clear.result.unwrap()["cleared"], json!(true));
    }

    #[test]
    fn memory_dashboard_route_returns_runtime_dashboard_state() {
        let mut runtime = Runtime::new();
        let start = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(1),
                method: "thread/start".to_string(),
                params: json!({ "project": { "id": "project", "cwd": "/repo" } }),
            },
        );
        let thread_id = start.result.unwrap()["thread"]["id"]
            .as_str()
            .unwrap()
            .to_string();

        let set = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(2),
                method: "memory/set".to_string(),
                params: json!({
                    "threadId": thread_id,
                    "status": {
                        "enabled": true,
                        "initialized": true,
                        "backend": "hoppi",
                        "scope": "project",
                        "memoryCount": 12,
                        "pinnedCount": 3,
                        "dashboardUrl": "http://hoppi.localhost:49152"
                    }
                }),
            },
        );
        assert!(set.error.is_none(), "{:?}", set.error);

        let dashboard = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(3),
                method: "memory/dashboard".to_string(),
                params: json!({ "threadId": thread_id }),
            },
        );
        assert!(dashboard.error.is_none(), "{:?}", dashboard.error);
        let result = dashboard.result.unwrap();
        assert_eq!(result["available"], json!(true));
        assert_eq!(result["url"], json!("http://hoppi.localhost:49152"));
        assert_eq!(result["fallbackUrl"], json!("http://localhost:49152"));
        assert_eq!(result["status"]["pinnedCount"], json!(3));
    }

    #[test]
    fn memory_list_route_returns_runtime_cached_records() {
        let mut runtime = Runtime::new();
        let start = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(1),
                method: "thread/start".to_string(),
                params: json!({ "project": { "id": "project", "cwd": "/repo" } }),
            },
        );
        let thread_id = start.result.unwrap()["thread"]["id"]
            .as_str()
            .unwrap()
            .to_string();

        let set = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(2),
                method: "memory/records/set".to_string(),
                params: json!({
                    "threadId": thread_id,
                    "source": "hoppi-list",
                    "status": {
                        "enabled": true,
                        "initialized": true,
                        "backend": "hoppi",
                        "scope": "project",
                        "memoryCount": 1,
                        "pinnedCount": 1
                    },
                    "records": [
                        {
                            "id": "mem-1",
                            "content": "Native memory list route is cached in Rust.",
                            "tags": ["oppi-native-shell"],
                            "pinned": true,
                            "confidence": "verified"
                        }
                    ]
                }),
            },
        );
        assert!(set.error.is_none(), "{:?}", set.error);

        let listed = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(3),
                method: "memory/list".to_string(),
                params: json!({
                    "threadId": thread_id,
                    "query": "route",
                    "pinnedOnly": true,
                    "includeSuperseded": false,
                    "limit": 5
                }),
            },
        );
        assert!(listed.error.is_none(), "{:?}", listed.error);
        let result = listed.result.unwrap();
        assert_eq!(result["available"], json!(true));
        assert_eq!(result["totalCount"], json!(1));
        assert_eq!(result["items"][0]["id"], json!("mem-1"));
        assert_eq!(result["status"]["pinnedCount"], json!(1));
    }

    #[test]
    fn memory_recall_route_records_runtime_context_and_records() {
        let mut runtime = Runtime::new();
        let start = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(1),
                method: "thread/start".to_string(),
                params: json!({ "project": { "id": "project", "cwd": "/repo" } }),
            },
        );
        let thread_id = start.result.unwrap()["thread"]["id"]
            .as_str()
            .unwrap()
            .to_string();

        let recalled = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(2),
                method: "memory/recall".to_string(),
                params: json!({
                    "threadId": thread_id,
                    "query": "ship native recall route",
                    "contextMarkdown": "Native recall context from Hoppi.",
                    "backendQueried": true,
                    "source": "hoppi-recall",
                    "status": {
                        "enabled": true,
                        "initialized": true,
                        "backend": "hoppi",
                        "scope": "project",
                        "memoryCount": 1,
                        "pinnedCount": 1
                    },
                    "records": [
                        {
                            "id": "mem-recall",
                            "content": "Route should cache recalled records.",
                            "pinned": true,
                            "retrievalStrength": 0.86
                        }
                    ]
                }),
            },
        );
        assert!(recalled.error.is_none(), "{:?}", recalled.error);
        let result = recalled.result.unwrap();
        assert_eq!(result["available"], json!(true));
        assert_eq!(result["backendQueried"], json!(true));
        assert_eq!(result["cacheUpdated"], json!(true));
        assert_eq!(
            result["contextMarkdown"],
            json!("Native recall context from Hoppi.")
        );
        assert_eq!(result["items"][0]["id"], json!("mem-recall"));
        assert_eq!(result["status"]["pinnedCount"], json!(1));

        let listed = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(3),
                method: "memory/list".to_string(),
                params: json!({ "threadId": thread_id, "pinnedOnly": true }),
            },
        );
        assert!(listed.error.is_none(), "{:?}", listed.error);
        let list = listed.result.unwrap();
        assert_eq!(list["totalCount"], json!(1));
        assert_eq!(list["items"][0]["id"], json!("mem-recall"));
    }

    #[test]
    fn memory_backend_recall_runs_hoppi_adapter_over_json_rpc() {
        let root = temp_store_root("oppi-hoppi-backend-recall");
        std::fs::create_dir_all(&root).unwrap();
        let fixture = root.join("hoppi-fixture.mjs");
        std::fs::write(
            &fixture,
            r#"
export function createHoppiBackend() {
  return {
    async init() {},
    async recall(input) {
      return {
        contextMarkdown: `remember ${input.query}`,
        memories: [{ id: "mem-1", content: "Rust owns the broker now." }]
      };
    }
  };
}
"#,
        )
        .unwrap();

        let mut runtime = Runtime::new();
        runtime.set_memory_backend_runner(Box::new(HoppiNodeBackendRunner::new_for_test(
            fixture,
            Duration::from_secs(5),
        )));
        let start = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(1),
                method: "thread/start".to_string(),
                params: json!({ "project": { "id": "project", "cwd": root.display().to_string() } }),
            },
        );
        let thread_id = start.result.unwrap()["thread"]["id"]
            .as_str()
            .unwrap()
            .to_string();

        let response = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(2),
                method: "memory/backend/recall".to_string(),
                params: json!({
                    "threadId": thread_id,
                    "prompt": "current gap",
                    "includeStartup": false,
                    "includeTask": true
                }),
            },
        );

        assert!(response.error.is_none(), "{:?}", response.error);
        let result = response.result.unwrap();
        assert_eq!(
            result["normalized"]["contextMarkdown"],
            json!("remember current gap")
        );
        assert_eq!(result["backend"], json!("hoppi-node-bridge"));
        assert_eq!(result["adapterKind"], json!("node-typescript"));
        assert_eq!(result["operation"], json!("recall"));
        assert_eq!(result["normalized"]["items"][0]["id"], json!("mem-1"));
    }

    #[test]
    fn memory_backend_routes_run_hoppi_adapter_operations_over_json_rpc() {
        let root = temp_store_root("oppi-hoppi-backend-routes");
        std::fs::create_dir_all(&root).unwrap();
        let fixture = root.join("hoppi-fixture.mjs");
        std::fs::write(
            &fixture,
            r#"
const status = {
  enabled: true,
  initialized: true,
  backend: "hoppi",
  scope: "project",
  memoryCount: 2,
  pinnedCount: 1,
  storePath: "C:/Users/matth/.oppi/hoppi",
  dashboardUrl: "http://hoppi.localhost:49152"
};

export function createHoppiBackend() {
  return {
    async init() {},
    async status() {
      return status;
    },
    async remember(input) {
      return {
        id: "mem-remember",
        content: input.content,
        tags: input.tags,
        layer: input.layer,
        confidence: "observed",
        source: input.source
      };
    },
    async list(input) {
      return [
        { id: "mem-list", content: "Backend route list item.", pinned: true },
        { id: "mem-other", content: "Other memory.", pinned: false }
      ].filter((record) => !input.pinnedOnly || record.pinned);
    },
    async startDashboard() {
      return { url: "http://hoppi.localhost:49152", async stop() {} };
    },
    async forget(id) {
      return { id };
    },
    async pin(id, pinned) {
      return { id, content: "Pinned by backend route.", pinned };
    },
    async update(id, patch) {
      return { id, content: patch.content || "Updated by backend route.", tags: patch.tags || [] , supersededBy: patch.supersededBy };
    },
    async consolidate(input) {
      return {
        dryRun: input.dryRun,
        summary: "Merged backend route memories.",
        modelUsed: "gpt-5.5",
        records: [{ id: "mem-consolidated", content: "Consolidated backend route memory." }],
        deletedIds: ["mem-other"],
        status
      };
    }
  };
}

export async function syncGitRepository(root, repoPath, options) {
  if (!repoPath.endsWith("oppi-hoppi-sync")) throw new Error(`bad repo path ${repoPath}`);
  if (options.remote !== "git@example.com:memories.git") throw new Error("missing sync remote");
  if (options.conflictMode !== "keep-local") throw new Error(`bad conflict mode ${options.conflictMode}`);
  if (options.passphrase !== "secret-passphrase") throw new Error("missing sync passphrase");
  if (options.message !== "Hoppi memory sync (manual)") throw new Error(`bad sync message ${options.message}`);
  return {
    imported: 2,
    exported: 1,
    committed: true,
    pushed: options.direction !== "pull",
    conflicts: [],
    direction: options.direction,
    embeddingsRebuildRecommended: true
  };
}

export async function rebuildEmbeddings(root) {
  return { ok: true };
}
"#,
        )
        .unwrap();

        let mut runtime = Runtime::new();
        runtime.set_memory_backend_runner(Box::new(HoppiNodeBackendRunner::new_for_test(
            fixture,
            Duration::from_secs(5),
        )));
        let adapter_info = HoppiNodeBackendRunner::new_for_test(
            root.join("hoppi-fixture.mjs"),
            Duration::from_secs(5),
        )
        .adapter_info();
        assert_eq!(adapter_info.id, "hoppi-node-bridge");
        assert_eq!(adapter_info.kind, "node-typescript");
        assert!(!adapter_info.embedded);
        assert_eq!(
            adapter_info.operations,
            MemoryBackendOperation::ALL.to_vec()
        );
        let start = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(1),
                method: "thread/start".to_string(),
                params: json!({ "project": { "id": "project", "cwd": root.display().to_string() } }),
            },
        );
        let thread_id = start.result.unwrap()["thread"]["id"]
            .as_str()
            .unwrap()
            .to_string();

        let status = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(2),
                method: "memory/backend/status".to_string(),
                params: json!({ "threadId": thread_id }),
            },
        );
        assert!(status.error.is_none(), "{:?}", status.error);
        let result = status.result.unwrap();
        assert_eq!(result["operation"], json!("status"));
        assert_eq!(result["adapterKind"], json!("node-typescript"));
        assert_eq!(result["normalized"]["memoryCount"], json!(2));
        assert_eq!(
            result["normalized"]["executionAdapterKind"],
            json!("node-typescript")
        );

        let remembered = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(3),
                method: "memory/backend/remember".to_string(),
                params: json!({
                    "threadId": thread_id,
                    "content": "Remember through backend route.",
                    "tags": ["backend-route"],
                    "source": "oppi:test",
                    "sourceSessionId": "thread-1:turn-1",
                    "layer": "buffer"
                }),
            },
        );
        assert!(remembered.error.is_none(), "{:?}", remembered.error);
        let result = remembered.result.unwrap();
        assert_eq!(result["operation"], json!("remember"));
        assert_eq!(result["normalized"]["record"]["id"], json!("mem-remember"));
        assert_eq!(result["normalized"]["backendApplied"], json!(true));

        let listed = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(4),
                method: "memory/backend/list".to_string(),
                params: json!({
                    "threadId": thread_id,
                    "query": "backend route",
                    "pinnedOnly": true,
                    "includeSuperseded": false,
                    "limit": 5
                }),
            },
        );
        assert!(listed.error.is_none(), "{:?}", listed.error);
        let result = listed.result.unwrap();
        assert_eq!(result["operation"], json!("list"));
        assert_eq!(result["normalized"]["items"][0]["id"], json!("mem-list"));

        let dashboard = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(5),
                method: "memory/backend/dashboard".to_string(),
                params: json!({ "threadId": thread_id }),
            },
        );
        assert!(dashboard.error.is_none(), "{:?}", dashboard.error);
        assert_eq!(
            dashboard.result.unwrap()["normalized"]["url"],
            json!("http://hoppi.localhost:49152")
        );

        let forgotten = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(6),
                method: "memory/backend/forget".to_string(),
                params: json!({ "threadId": thread_id, "id": "mem-list" }),
            },
        );
        assert!(forgotten.error.is_none(), "{:?}", forgotten.error);
        assert_eq!(
            forgotten.result.unwrap()["normalized"]["action"],
            json!("forget")
        );

        let pinned = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(7),
                method: "memory/backend/pin".to_string(),
                params: json!({ "threadId": thread_id, "id": "mem-remember", "pinned": true }),
            },
        );
        assert!(pinned.error.is_none(), "{:?}", pinned.error);
        assert_eq!(
            pinned.result.unwrap()["normalized"]["record"]["pinned"],
            json!(true)
        );

        let updated = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(8),
                method: "memory/backend/update".to_string(),
                params: json!({
                    "threadId": thread_id,
                    "id": "mem-remember",
                    "patch": { "content": "Updated through backend route.", "tags": ["updated"] }
                }),
            },
        );
        assert!(updated.error.is_none(), "{:?}", updated.error);
        assert_eq!(
            updated.result.unwrap()["normalized"]["record"]["content"],
            json!("Updated through backend route.")
        );

        let superseded = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(9),
                method: "memory/backend/supersede".to_string(),
                params: json!({ "threadId": thread_id, "id": "mem-remember", "supersededBy": "mem-new" }),
            },
        );
        assert!(superseded.error.is_none(), "{:?}", superseded.error);
        assert_eq!(
            superseded.result.unwrap()["normalized"]["record"]["supersededBy"],
            json!("mem-new")
        );

        let consolidated = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(10),
                method: "memory/backend/consolidate".to_string(),
                params: json!({
                    "threadId": thread_id,
                    "stage": "idle-dream",
                    "dryRun": false,
                    "limit": 50,
                    "budget": 1500,
                    "agentModel": "auto",
                    "deepModel": "gpt-5.5"
                }),
            },
        );
        assert!(consolidated.error.is_none(), "{:?}", consolidated.error);
        assert_eq!(
            consolidated.result.unwrap()["normalized"]["createdCount"],
            json!(1)
        );

        let synced = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(11),
                method: "memory/backend/sync".to_string(),
                params: json!({
                    "threadId": thread_id,
                    "direction": "both",
                    "repoPath": root.join("oppi-hoppi-sync").display().to_string(),
                    "remote": "git@example.com:memories.git",
                    "conflictMode": "keep-local",
                    "passphrase": "secret-passphrase",
                    "message": "Hoppi memory sync (manual)"
                }),
            },
        );
        assert!(synced.error.is_none(), "{:?}", synced.error);
        let result = synced.result.unwrap();
        assert_eq!(result["operation"], json!("sync"));
        assert_eq!(result["normalized"]["imported"], json!(2));
        assert_eq!(result["normalized"]["exported"], json!(1));
        assert_eq!(result["normalized"]["rebuiltEmbeddings"], json!(true));
    }

    #[test]
    fn memory_backend_timeout_marks_job_failed_and_queue_idle() {
        let root = temp_store_root("oppi-hoppi-backend-timeout");
        std::fs::create_dir_all(&root).unwrap();
        let fixture = root.join("hoppi-timeout-fixture.mjs");
        std::fs::write(
            &fixture,
            r#"
export function createHoppiBackend() {
  return {
    async init() {},
    async recall() {
      setInterval(() => {}, 1000);
      await new Promise(() => {});
    }
  };
}
"#,
        )
        .unwrap();

        let mut runtime = Runtime::new();
        runtime.set_memory_backend_runner(Box::new(HoppiNodeBackendRunner::new_for_test(
            fixture,
            Duration::from_millis(120),
        )));
        let start = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(1),
                method: "thread/start".to_string(),
                params: json!({ "project": { "id": "project", "cwd": root.display().to_string() } }),
            },
        );
        let thread_id = start.result.unwrap()["thread"]["id"]
            .as_str()
            .unwrap()
            .to_string();

        let response = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(2),
                method: "memory/backend/recall".to_string(),
                params: json!({
                    "threadId": thread_id,
                    "prompt": "timeout coverage",
                    "includeStartup": false,
                    "includeTask": true
                }),
            },
        );

        let error = response.error.unwrap();
        assert_eq!(
            error.data.as_ref().unwrap()["code"],
            json!("memory_backend_node_timeout")
        );

        let jobs = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(3),
                method: "memory/jobs/list".to_string(),
                params: json!({
                    "threadId": thread_id,
                    "includeFinished": true,
                    "limit": 10
                }),
            },
        );
        assert!(jobs.error.is_none(), "{:?}", jobs.error);
        let result = jobs.result.unwrap();
        assert_eq!(result["queueDepth"], json!(0));
        let job = result["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["kind"] == json!("recall"))
            .unwrap();
        assert_eq!(job["status"], json!("failed"));
        assert_ne!(job["backendApplied"], json!(true));
        assert!(
            job["message"]
                .as_str()
                .unwrap()
                .contains("timed out after 120 ms")
        );
    }

    #[test]
    fn memory_backend_preference_parses_env_values_and_rejects_unknown() {
        assert_eq!(
            memory_backend_preference_from_env(Some("embedded")).unwrap(),
            MemoryBackendPreference::Embedded
        );
        assert_eq!(
            memory_backend_preference_from_env(Some("rust-embedded")).unwrap(),
            MemoryBackendPreference::Embedded
        );
        assert_eq!(
            memory_backend_preference_from_env(Some("node")).unwrap(),
            MemoryBackendPreference::Node
        );
        assert_eq!(
            memory_backend_preference_from_env(Some("node-typescript")).unwrap(),
            MemoryBackendPreference::Node
        );
        assert_eq!(
            memory_backend_preference_from_env(Some("")).unwrap(),
            MemoryBackendPreference::Auto
        );
        assert!(memory_backend_preference_from_env(Some("banana")).is_err());
    }

    #[test]
    fn memory_backend_embedded_runner_remembers_and_recalls_over_json_rpc() {
        let mut runtime = Runtime::new();
        runtime.set_memory_backend_runner(Box::new(EmbeddedHoppiBackendRunner::new_for_test()));
        let start = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(1),
                method: "thread/start".to_string(),
                params: json!({
                    "project": {
                        "id": "project",
                        "cwd": temp_store_root("oppi-embedded-backend").display().to_string()
                    }
                }),
            },
        );
        assert!(start.error.is_none(), "{:?}", start.error);
        let thread_id = start.result.unwrap()["thread"]["id"]
            .as_str()
            .unwrap()
            .to_string();

        let remembered = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(2),
                method: "memory/backend/remember".to_string(),
                params: json!({
                    "threadId": thread_id,
                    "content": "OPPi uses runtime-owned embedded memory backend routes.",
                    "tags": ["oppi", "embedded"]
                }),
            },
        );
        assert!(remembered.error.is_none(), "{:?}", remembered.error);
        let result = remembered.result.unwrap();
        assert_eq!(result["backend"], json!("hoppi-embedded"));
        assert_eq!(result["adapterKind"], json!("rust-embedded"));
        assert_eq!(result["normalized"]["record"]["id"], json!("mem-000001"));
        assert_eq!(result["normalized"]["status"]["memoryCount"], json!(1));

        let recalled = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(3),
                method: "memory/backend/recall".to_string(),
                params: json!({
                    "threadId": thread_id,
                    "prompt": "embedded memory backend routes",
                    "includeStartup": false,
                    "includeTask": true
                }),
            },
        );
        assert!(recalled.error.is_none(), "{:?}", recalled.error);
        let result = recalled.result.unwrap();
        assert!(
            result["normalized"]["contextMarkdown"]
                .as_str()
                .unwrap()
                .contains("runtime-owned embedded memory backend routes")
        );
        assert_eq!(result["normalized"]["items"][0]["id"], json!("mem-000001"));
    }

    #[test]
    fn memory_remember_route_records_backend_write_and_cached_record() {
        let mut runtime = Runtime::new();
        let start = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(1),
                method: "thread/start".to_string(),
                params: json!({ "project": { "id": "project", "cwd": "/repo" } }),
            },
        );
        let thread_id = start.result.unwrap()["thread"]["id"]
            .as_str()
            .unwrap()
            .to_string();

        let remembered = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(2),
                method: "memory/remember".to_string(),
                params: json!({
                    "threadId": thread_id,
                    "content": "Route should cache saved Hoppi remember records.",
                    "tags": ["oppi-native-shell", "oppi-turn-summary"],
                    "layer": "buffer",
                    "confidence": "observed",
                    "sourceSessionId": "thread-1:turn-1",
                    "backendApplied": true,
                    "source": "hoppi-remember",
                    "reason": "turn-summary",
                    "status": {
                        "enabled": true,
                        "initialized": true,
                        "backend": "hoppi",
                        "scope": "project",
                        "memoryCount": 1,
                        "pinnedCount": 0
                    },
                    "record": {
                        "id": "mem-remember",
                        "content": "Route should cache saved Hoppi remember records.",
                        "tags": ["oppi-native-shell", "oppi-turn-summary"],
                        "layer": "buffer",
                        "confidence": "observed",
                        "source": "oppi:native-shell"
                    }
                }),
            },
        );
        assert!(remembered.error.is_none(), "{:?}", remembered.error);
        let result = remembered.result.unwrap();
        assert_eq!(result["available"], json!(true));
        assert_eq!(result["saved"], json!(true));
        assert_eq!(result["backendApplied"], json!(true));
        assert_eq!(result["cacheUpdated"], json!(true));
        assert_eq!(result["record"]["id"], json!("mem-remember"));
        assert_eq!(result["status"]["memoryCount"], json!(1));

        let listed = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(3),
                method: "memory/list".to_string(),
                params: json!({ "threadId": thread_id, "query": "saved Hoppi remember" }),
            },
        );
        assert!(listed.error.is_none(), "{:?}", listed.error);
        let list = listed.result.unwrap();
        assert_eq!(list["totalCount"], json!(1));
        assert_eq!(list["items"][0]["id"], json!("mem-remember"));
    }

    #[test]
    fn memory_recurring_candidates_route_records_candidates_over_json_rpc() {
        let mut runtime = Runtime::new();
        let start = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(1),
                method: "thread/start".to_string(),
                params: json!({ "project": { "id": "project", "cwd": "/repo" } }),
            },
        );
        let thread_id = start.result.unwrap()["thread"]["id"]
            .as_str()
            .unwrap()
            .to_string();

        let recorded = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(2),
                method: "memory/recurring-candidates".to_string(),
                params: json!({
                    "threadId": thread_id,
                    "sourceProject": "/repo",
                    "sourceSessionId": "session-1",
                    "recentUserTexts": ["continue with the plan"],
                    "recentAssistantTexts": ["Plan 75 evidence updated."],
                    "candidates": [{
                        "task": "Audit native memory parity after each backend slice",
                        "rationale": "This work repeats across standalone parity passes.",
                        "promptHints": ["continue with the plan"],
                        "outcomes": ["Plan 61 gets current verification evidence"],
                        "relatedMemoryIds": ["mem-1"],
                        "tags": ["plan75", "project:private"]
                    }]
                }),
            },
        );
        assert!(recorded.error.is_none(), "{:?}", recorded.error);
        let result = recorded.result.unwrap();
        assert_eq!(result["available"], json!(true));
        assert_eq!(result["savedCount"], json!(1));
        assert_eq!(
            result["records"][0]["source"],
            json!("oppi:recurring-task-mining")
        );
        assert!(
            result["records"][0]["content"]
                .as_str()
                .unwrap()
                .contains("Approval gate: only propose skill creation")
        );
        assert_eq!(
            result["records"][0]["tags"],
            json!([
                "oppi-recurring-task-candidate",
                "system-scope",
                "skill-candidate",
                "plan75"
            ])
        );

        let jobs = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(3),
                method: "memory/jobs/list".to_string(),
                params: json!({ "threadId": thread_id, "includeFinished": true }),
            },
        );
        assert!(jobs.error.is_none(), "{:?}", jobs.error);
        assert!(
            jobs.result.unwrap()["items"]
                .as_array()
                .unwrap()
                .iter()
                .any(|job| {
                    job["kind"] == json!("remember")
                        && job["stage"] == json!("recurring-task-mining")
                        && job["source"] == json!("memory-recurring-task-mining")
                        && job["recordCount"] == json!(1)
                })
        );
    }

    #[test]
    fn memory_distill_turn_route_runs_model_and_records_candidates_over_json_rpc() {
        let response = json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": serde_json::to_string(&json!({
                        "remember": true,
                        "request": "Continue Plan 75",
                        "completed": ["Added runtime memory distillation route"],
                        "learned": ["Rust owns recurring candidate visibility"],
                        "decisions": ["Keep skill creation approval gated"],
                        "next": ["Wire native shell to distiller"],
                        "files": ["crates/oppi-server/src/main.rs"],
                        "tags": ["plan75"],
                        "recurringTasks": [{
                            "task": "Audit native memory parity after backend slices",
                            "rationale": "Plan alignment repeats across standalone work",
                            "promptHints": ["continue with the plan"],
                            "outcomes": ["Plan 61 receives fresh evidence"],
                            "relatedMemoryIds": ["mem-1"],
                            "tags": ["plan75", "project:private"]
                        }]
                    })).unwrap()
                }
            }]
        })
        .to_string();
        let (base_url, server) = start_mock_openai_server(vec![response]);
        let key_name = format!(
            "OPPI_MEMORY_DISTILL_SERVER_TEST_{}_API_KEY",
            std::process::id()
        );
        unsafe { std::env::set_var(&key_name, "test-key") };
        let mut runtime = Runtime::new();
        let start = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(1),
                method: "thread/start".to_string(),
                params: json!({ "project": { "id": "project", "cwd": "/repo" } }),
            },
        );
        let thread_id = start.result.unwrap()["thread"]["id"]
            .as_str()
            .unwrap()
            .to_string();

        let distilled = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(2),
                method: "memory/distill-turn".to_string(),
                params: json!({
                    "threadId": thread_id,
                    "userText": "continue with the plan",
                    "assistantText": "Added a runtime memory route and tests.",
                    "sourceProject": "/repo",
                    "sourceSessionId": "session-1",
                    "recentUserTexts": ["continue with the plan"],
                    "recentAssistantTexts": ["Plan 75 evidence updated."],
                    "agentModel": "gpt",
                    "modelProvider": {
                        "kind": "openai-compatible",
                        "model": "mock-memory-model",
                        "baseUrl": base_url,
                        "apiKeyEnv": key_name,
                        "temperature": 0.2
                    }
                }),
            },
        );
        unsafe { std::env::remove_var(&key_name) };

        assert!(distilled.error.is_none(), "{:?}", distilled.error);
        let requests = server.join().unwrap();
        let result = distilled.result.unwrap();
        assert_eq!(result["remembered"], json!(true));
        assert_eq!(result["modelUsed"], json!("mock-memory-model"));
        assert_eq!(result["recurringSavedCount"], json!(1));
        assert_eq!(result["record"]["source"], json!("oppi:turn-distillation"));
        assert_eq!(
            result["recurringRecords"][0]["source"],
            json!("oppi:recurring-task-mining")
        );
        assert_eq!(
            result["recurringRecords"][0]["tags"],
            json!([
                "oppi-recurring-task-candidate",
                "system-scope",
                "skill-candidate",
                "plan75"
            ])
        );
        assert!(requests[0].contains("OPPi's memory distiller"));

        let jobs = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(3),
                method: "memory/jobs/list".to_string(),
                params: json!({ "threadId": thread_id, "includeFinished": true }),
            },
        );
        assert!(jobs.error.is_none(), "{:?}", jobs.error);
        assert!(
            jobs.result.unwrap()["items"]
                .as_array()
                .unwrap()
                .iter()
                .any(|job| {
                    job["kind"] == json!("remember")
                        && job["stage"] == json!("turn-distillation")
                        && job["source"] == json!("memory-distiller")
                        && job["modelUsed"] == json!("mock-memory-model")
                        && job["recordCount"] == json!(2)
                })
        );
    }

    #[test]
    fn memory_forget_route_removes_runtime_cached_record() {
        let mut runtime = Runtime::new();
        let start = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(1),
                method: "thread/start".to_string(),
                params: json!({ "project": { "id": "project", "cwd": "/repo" } }),
            },
        );
        let thread_id = start.result.unwrap()["thread"]["id"]
            .as_str()
            .unwrap()
            .to_string();

        let set = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(2),
                method: "memory/records/set".to_string(),
                params: json!({
                    "threadId": thread_id,
                    "source": "hoppi-list",
                    "status": {
                        "enabled": true,
                        "initialized": true,
                        "backend": "hoppi",
                        "scope": "project",
                        "memoryCount": 2,
                        "pinnedCount": 1
                    },
                    "records": [
                        { "id": "mem-1", "content": "Delete this record.", "pinned": true },
                        { "id": "mem-2", "content": "Keep this record." }
                    ]
                }),
            },
        );
        assert!(set.error.is_none(), "{:?}", set.error);

        let forgotten = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(3),
                method: "memory/record/forget".to_string(),
                params: json!({
                    "threadId": thread_id,
                    "id": "mem-1",
                    "backendApplied": true,
                    "source": "hoppi-forget"
                }),
            },
        );
        assert!(forgotten.error.is_none(), "{:?}", forgotten.error);
        let result = forgotten.result.unwrap();
        assert_eq!(result["action"], json!("forget"));
        assert_eq!(result["id"], json!("mem-1"));
        assert_eq!(result["backendApplied"], json!(true));
        assert_eq!(result["cacheUpdated"], json!(true));
        assert_eq!(result["status"]["memoryCount"], json!(1));
        assert!(result["status"].get("pinnedCount").is_none());

        let listed = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(4),
                method: "memory/list".to_string(),
                params: json!({ "threadId": thread_id, "includeSuperseded": true }),
            },
        );
        assert!(listed.error.is_none(), "{:?}", listed.error);
        let list = listed.result.unwrap();
        assert_eq!(list["totalCount"], json!(1));
        assert_eq!(list["items"][0]["id"], json!("mem-2"));
    }

    #[test]
    fn memory_pin_route_updates_runtime_cached_record() {
        let mut runtime = Runtime::new();
        let start = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(1),
                method: "thread/start".to_string(),
                params: json!({ "project": { "id": "project", "cwd": "/repo" } }),
            },
        );
        let thread_id = start.result.unwrap()["thread"]["id"]
            .as_str()
            .unwrap()
            .to_string();

        let set = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(2),
                method: "memory/records/set".to_string(),
                params: json!({
                    "threadId": thread_id,
                    "source": "hoppi-list",
                    "status": {
                        "enabled": true,
                        "initialized": true,
                        "backend": "hoppi",
                        "scope": "project",
                        "memoryCount": 1,
                        "pinnedCount": 0
                    },
                    "records": [
                        { "id": "mem-1", "content": "Pin this record.", "pinned": false }
                    ]
                }),
            },
        );
        assert!(set.error.is_none(), "{:?}", set.error);

        let pinned = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(3),
                method: "memory/record/pin".to_string(),
                params: json!({
                    "threadId": thread_id,
                    "id": "mem-1",
                    "pinned": true,
                    "backendApplied": true,
                    "source": "hoppi-pin",
                    "record": { "id": "mem-1", "content": "Pin this record.", "pinned": true }
                }),
            },
        );
        assert!(pinned.error.is_none(), "{:?}", pinned.error);
        let result = pinned.result.unwrap();
        assert_eq!(result["action"], json!("pin"));
        assert_eq!(result["id"], json!("mem-1"));
        assert_eq!(result["backendApplied"], json!(true));
        assert_eq!(result["cacheUpdated"], json!(true));
        assert_eq!(result["record"]["pinned"], json!(true));
        assert_eq!(result["status"]["pinnedCount"], json!(1));

        let listed = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(4),
                method: "memory/list".to_string(),
                params: json!({ "threadId": thread_id, "pinnedOnly": true }),
            },
        );
        assert!(listed.error.is_none(), "{:?}", listed.error);
        let list = listed.result.unwrap();
        assert_eq!(list["totalCount"], json!(1));
        assert_eq!(list["items"][0]["id"], json!("mem-1"));
        assert_eq!(list["items"][0]["pinned"], json!(true));
    }

    #[test]
    fn memory_update_route_updates_runtime_cached_record() {
        let mut runtime = Runtime::new();
        let start = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(1),
                method: "thread/start".to_string(),
                params: json!({ "project": { "id": "project", "cwd": "/repo" } }),
            },
        );
        let thread_id = start.result.unwrap()["thread"]["id"]
            .as_str()
            .unwrap()
            .to_string();

        let set = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(2),
                method: "memory/records/set".to_string(),
                params: json!({
                    "threadId": thread_id,
                    "source": "hoppi-list",
                    "status": {
                        "enabled": true,
                        "initialized": true,
                        "backend": "hoppi",
                        "scope": "project",
                        "memoryCount": 1,
                        "pinnedCount": 0
                    },
                    "records": [
                        {
                            "id": "mem-1",
                            "content": "Edit this record.",
                            "tags": ["draft"],
                            "confidence": "observed"
                        }
                    ]
                }),
            },
        );
        assert!(set.error.is_none(), "{:?}", set.error);

        let updated = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(3),
                method: "memory/record/update".to_string(),
                params: json!({
                    "threadId": thread_id,
                    "id": "mem-1",
                    "backendApplied": true,
                    "source": "hoppi-update",
                    "patch": {
                        "content": "Edited through runtime update.",
                        "tags": ["verified", "native"],
                        "confidence": "verified"
                    },
                    "record": {
                        "id": "mem-1",
                        "content": "Edited through runtime update.",
                        "tags": ["verified", "native"],
                        "confidence": "verified"
                    }
                }),
            },
        );
        assert!(updated.error.is_none(), "{:?}", updated.error);
        let result = updated.result.unwrap();
        assert_eq!(result["action"], json!("update"));
        assert_eq!(result["id"], json!("mem-1"));
        assert_eq!(result["backendApplied"], json!(true));
        assert_eq!(result["cacheUpdated"], json!(true));
        assert_eq!(
            result["record"]["content"],
            json!("Edited through runtime update.")
        );
        assert_eq!(result["record"]["tags"], json!(["verified", "native"]));

        let listed = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(4),
                method: "memory/list".to_string(),
                params: json!({ "threadId": thread_id, "query": "runtime" }),
            },
        );
        assert!(listed.error.is_none(), "{:?}", listed.error);
        let list = listed.result.unwrap();
        assert_eq!(list["totalCount"], json!(1));
        assert_eq!(list["items"][0]["id"], json!("mem-1"));
        assert_eq!(
            list["items"][0]["content"],
            json!("Edited through runtime update.")
        );
    }

    #[test]
    fn memory_supersede_route_marks_runtime_cached_record() {
        let mut runtime = Runtime::new();
        let start = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(1),
                method: "thread/start".to_string(),
                params: json!({ "project": { "id": "project", "cwd": "/repo" } }),
            },
        );
        let thread_id = start.result.unwrap()["thread"]["id"]
            .as_str()
            .unwrap()
            .to_string();

        let set = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(2),
                method: "memory/records/set".to_string(),
                params: json!({
                    "threadId": thread_id,
                    "source": "hoppi-list",
                    "records": [
                        { "id": "mem-old", "content": "Older memory." },
                        { "id": "mem-new", "content": "Newer memory." }
                    ]
                }),
            },
        );
        assert!(set.error.is_none(), "{:?}", set.error);

        let superseded = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(3),
                method: "memory/record/supersede".to_string(),
                params: json!({
                    "threadId": thread_id,
                    "id": "mem-old",
                    "supersededBy": "mem-new",
                    "backendApplied": true,
                    "source": "hoppi-supersede"
                }),
            },
        );
        assert!(superseded.error.is_none(), "{:?}", superseded.error);
        let result = superseded.result.unwrap();
        assert_eq!(result["action"], json!("supersede"));
        assert_eq!(result["id"], json!("mem-old"));
        assert_eq!(result["backendApplied"], json!(true));
        assert_eq!(result["cacheUpdated"], json!(true));
        assert_eq!(result["record"]["supersededBy"], json!("mem-new"));

        let listed = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(4),
                method: "memory/list".to_string(),
                params: json!({ "threadId": thread_id }),
            },
        );
        assert!(listed.error.is_none(), "{:?}", listed.error);
        let list = listed.result.unwrap();
        assert_eq!(list["totalCount"], json!(1));
        assert_eq!(list["items"][0]["id"], json!("mem-new"));
    }

    #[test]
    fn memory_consolidate_route_records_backend_result() {
        let mut runtime = Runtime::new();
        let start = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(1),
                method: "thread/start".to_string(),
                params: json!({ "project": { "id": "project", "cwd": "/repo" } }),
            },
        );
        let thread_id = start.result.unwrap()["thread"]["id"]
            .as_str()
            .unwrap()
            .to_string();

        let set = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(2),
                method: "memory/records/set".to_string(),
                params: json!({
                    "threadId": thread_id,
                    "source": "hoppi-list",
                    "records": [
                        { "id": "mem-noise", "content": "Noisy duplicate detail." },
                        { "id": "mem-keep", "content": "Keep this memory.", "pinned": true }
                    ],
                    "status": {
                        "enabled": true,
                        "initialized": true,
                        "backend": "hoppi",
                        "scope": "project",
                        "memoryCount": 2,
                        "pinnedCount": 1
                    }
                }),
            },
        );
        assert!(set.error.is_none(), "{:?}", set.error);

        let consolidated = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(3),
                method: "memory/consolidate".to_string(),
                params: json!({
                    "threadId": thread_id,
                    "stage": "idle-dream",
                    "summary": "Merged duplicate details.",
                    "backendApplied": true,
                    "records": [
                        {
                            "id": "mem-consolidated",
                            "content": "Consolidated implementation memory.",
                            "tags": ["consolidated"],
                            "parents": ["mem-noise"]
                        }
                    ],
                    "deletedIds": ["mem-noise"],
                    "status": {
                        "enabled": true,
                        "initialized": true,
                        "backend": "hoppi",
                        "scope": "project",
                        "memoryCount": 2,
                        "pinnedCount": 1,
                        "lastConsolidatedAt": "2026-05-31T10:00:00Z",
                        "queueDepth": 0
                    },
                    "source": "hoppi-consolidate"
                }),
            },
        );
        assert!(consolidated.error.is_none(), "{:?}", consolidated.error);
        let result = consolidated.result.unwrap();
        assert_eq!(result["backendApplied"], json!(true));
        assert_eq!(result["cacheUpdated"], json!(true));
        assert_eq!(result["createdCount"], json!(1));
        assert_eq!(result["deletedCount"], json!(1));
        assert_eq!(result["deletedIds"], json!(["mem-noise"]));
        assert_eq!(result["status"]["queueDepth"], json!(0));

        let listed = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(4),
                method: "memory/list".to_string(),
                params: json!({ "threadId": thread_id, "includeSuperseded": true }),
            },
        );
        assert!(listed.error.is_none(), "{:?}", listed.error);
        let list = listed.result.unwrap();
        assert_eq!(list["totalCount"], json!(2));
        assert_eq!(list["items"][0]["id"], json!("mem-keep"));
        assert_eq!(list["items"][1]["id"], json!("mem-consolidated"));
    }

    #[test]
    fn memory_jobs_routes_round_trip_queue_state() {
        let mut runtime = Runtime::new();
        let start = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(1),
                method: "thread/start".to_string(),
                params: json!({ "project": { "id": "project", "cwd": "/repo" } }),
            },
        );
        let thread_id = start.result.unwrap()["thread"]["id"]
            .as_str()
            .unwrap()
            .to_string();

        let running = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(2),
                method: "memory/jobs/update".to_string(),
                params: json!({
                    "threadId": thread_id,
                    "jobId": "mem-job-1",
                    "kind": "consolidate",
                    "stage": "idle-dream",
                    "status": "running",
                    "source": "native-shell",
                    "message": "Running idle Hoppi consolidation.",
                    "startedAt": "2026-05-31T12:00:01Z"
                }),
            },
        );
        assert!(running.error.is_none(), "{:?}", running.error);
        let result = running.result.unwrap();
        assert_eq!(result["queueDepth"], json!(1));
        assert_eq!(result["status"]["queueDepth"], json!(1));
        assert_eq!(result["items"][0]["id"], json!("mem-job-1"));
        assert_eq!(result["items"][0]["status"], json!("running"));

        let cancelled = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(22),
                method: "memory/jobs/cancel".to_string(),
                params: json!({
                    "threadId": thread_id,
                    "jobId": "mem-job-1",
                    "reason": "user requested cancellation",
                    "finishedAt": "2026-05-31T12:00:05Z"
                }),
            },
        );
        assert!(cancelled.error.is_none(), "{:?}", cancelled.error);
        let result = cancelled.result.unwrap();
        assert_eq!(result["queueDepth"], json!(0));
        assert_eq!(result["status"]["queueDepth"], json!(0));
        assert_eq!(result["items"][0]["id"], json!("mem-job-1"));
        assert_eq!(result["items"][0]["status"], json!("cancelled"));
        assert_eq!(
            result["items"][0]["message"],
            json!("user requested cancellation")
        );

        let finished = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(3),
                method: "memory/jobs/update".to_string(),
                params: json!({
                    "threadId": thread_id,
                    "jobId": "mem-job-2",
                    "kind": "consolidate",
                    "stage": "idle-dream",
                    "status": "succeeded",
                    "source": "native-shell",
                    "message": "Applied idle Hoppi consolidation.",
                    "finishedAt": "2026-05-31T12:00:10Z",
                    "recordCount": 2,
                    "deletedCount": 1,
                    "backendApplied": true
                }),
            },
        );
        assert!(finished.error.is_none(), "{:?}", finished.error);
        assert_eq!(finished.result.unwrap()["queueDepth"], json!(0));

        let listed = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(4),
                method: "memory/jobs/list".to_string(),
                params: json!({
                    "threadId": thread_id,
                    "includeFinished": true
                }),
            },
        );
        assert!(listed.error.is_none(), "{:?}", listed.error);
        let result = listed.result.unwrap();
        assert_eq!(result["queueDepth"], json!(0));
        assert_eq!(result["totalCount"], json!(2));
        assert!(
            result["items"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item["status"] == json!("cancelled"))
        );
        let succeeded = result["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["status"] == json!("succeeded"))
            .unwrap();
        assert_eq!(succeeded["recordCount"], json!(2));
        assert_eq!(succeeded["deletedCount"], json!(1));
    }

    #[test]
    fn permissions_rpc_settings_history_and_clear_session_round_trip() {
        let mut runtime = Runtime::new();
        let start = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(1),
                method: "thread/start".to_string(),
                params: json!({ "project": { "id": "project", "cwd": "/repo" } }),
            },
        );
        let thread_id = start.result.unwrap()["thread"]["id"]
            .as_str()
            .unwrap()
            .to_string();

        let update = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(2),
                method: "permissions/settings/update".to_string(),
                params: json!({
                    "threadId": thread_id,
                    "settings": {
                        "timeoutMs": 30000,
                        "reviewerModel": "openai-codex/gpt-5.5"
                    }
                }),
            },
        );
        assert!(update.error.is_none(), "{:?}", update.error);
        assert_eq!(
            update.result.unwrap()["settings"]["timeoutMs"],
            json!(30000)
        );

        let settings = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(3),
                method: "permissions/settings".to_string(),
                params: json!({}),
            },
        );
        assert!(settings.error.is_none(), "{:?}", settings.error);
        assert_eq!(
            settings.result.unwrap()["settings"]["reviewerModel"],
            json!("openai-codex/gpt-5.5")
        );

        let history = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(4),
                method: "permissions/history".to_string(),
                params: json!({ "threadId": thread_id, "limit": 10 }),
            },
        );
        assert!(history.error.is_none(), "{:?}", history.error);
        assert_eq!(history.result.unwrap()["records"], json!([]));

        let context = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(5),
                method: "permissions/context".to_string(),
                params: json!({ "threadId": thread_id }),
            },
        );
        assert!(context.error.is_none(), "{:?}", context.error);
        let context = context.result.unwrap();
        assert_eq!(
            context["settings"]["reviewerModel"],
            json!("openai-codex/gpt-5.5")
        );
        assert!(
            context["promptContext"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item.as_str().unwrap().contains("recent Guardian"))
        );
        assert_eq!(context["reviewerTools"][0], json!("oppi_review_read"));

        let cleared = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(6),
                method: "permissions/clear-session".to_string(),
                params: json!({ "threadId": thread_id }),
            },
        );
        assert!(cleared.error.is_none(), "{:?}", cleared.error);
        assert_eq!(cleared.result.unwrap()["clearedReviews"], json!(0));
    }

    #[test]
    fn ask_user_rpc_settings_round_trip() {
        let mut runtime = Runtime::new();
        let start = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(1),
                method: "thread/start".to_string(),
                params: json!({ "project": { "id": "project", "cwd": "/repo" } }),
            },
        );
        let thread_id = start.result.unwrap()["thread"]["id"]
            .as_str()
            .unwrap()
            .to_string();

        let update = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(2),
                method: "ask-user/settings/update".to_string(),
                params: json!({
                    "threadId": thread_id,
                    "settings": { "timeoutMs": 120000 }
                }),
            },
        );
        assert!(update.error.is_none(), "{:?}", update.error);
        assert_eq!(
            update.result.unwrap()["settings"]["timeoutMs"],
            json!(120000)
        );

        let settings = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(3),
                method: "ask-user/settings".to_string(),
                params: json!({}),
            },
        );
        assert!(settings.error.is_none(), "{:?}", settings.error);
        assert_eq!(
            settings.result.unwrap()["settings"]["timeoutMs"],
            json!(120000)
        );
    }

    #[test]
    fn side_question_route_returns_side_thread_and_answer() {
        let mut runtime = Runtime::new();
        let start = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(1),
                method: "thread/start".to_string(),
                params: json!({
                    "project": { "id": "project", "cwd": "/repo" },
                    "title": "main"
                }),
            },
        );
        let thread_id = start.result.unwrap()["thread"]["id"]
            .as_str()
            .unwrap()
            .to_string();

        let response = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(2),
                method: "side-question/ask".to_string(),
                params: json!({
                    "threadId": thread_id,
                    "question": "What context does Guardian get?",
                    "modelSteps": [{
                        "assistantDeltas": ["Guardian sees bounded side context."],
                        "finalResponse": true
                    }],
                    "maxContinuations": 1
                }),
            },
        );

        let result = response.result.expect("side-question result");
        assert_eq!(result["parentThreadId"], json!(thread_id));
        assert_ne!(result["sideThreadId"], result["parentThreadId"]);
        assert_eq!(result["status"], json!("completed"));
        assert!(result["sideTurnId"].as_str().is_some());
        assert_eq!(
            result["answer"],
            json!("Guardian sees bounded side context.")
        );
        assert!(result["events"].as_array().unwrap().iter().any(|event| {
            event["kind"]["type"] == json!("sideQuestionAnswered")
                && event["kind"]["parentThreadId"] == json!(thread_id)
                && event["kind"]["sideThreadId"] == result["sideThreadId"]
                && event["kind"]["sideTurnId"] == result["sideTurnId"]
        }));
    }

    #[test]
    fn thread_rename_and_archive_round_trip_over_json_rpc() {
        let mut runtime = Runtime::new();
        let start = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(1),
                method: "thread/start".to_string(),
                params: json!({
                    "project": { "id": "project", "cwd": "/repo" },
                    "title": "Original"
                }),
            },
        );
        let thread_id = start.result.unwrap()["thread"]["id"]
            .as_str()
            .unwrap()
            .to_string();

        let renamed = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(2),
                method: "thread/rename".to_string(),
                params: json!({ "threadId": thread_id, "title": "Renamed" }),
            },
        );
        assert!(renamed.error.is_none(), "{:?}", renamed.error);
        assert_eq!(renamed.result.unwrap()["thread"]["title"], json!("Renamed"));

        let archived = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(3),
                method: "thread/archive".to_string(),
                params: json!({ "threadId": thread_id }),
            },
        );
        assert!(archived.error.is_none(), "{:?}", archived.error);
        assert_eq!(
            archived.result.unwrap()["thread"]["status"],
            json!("archived")
        );
    }

    #[test]
    fn thread_goal_continuation_rpc_returns_prompt_and_guard() {
        let mut runtime = Runtime::new();
        let start = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(1),
                method: "thread/start".to_string(),
                params: json!({ "project": { "id": "project", "cwd": "/repo" } }),
            },
        );
        let thread_id = start.result.unwrap()["thread"]["id"]
            .as_str()
            .unwrap()
            .to_string();
        let _ = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(2),
                method: "thread/goal/set".to_string(),
                params: json!({
                    "threadId": thread_id,
                    "objective": "Ship continuation",
                    "status": "active"
                }),
            },
        );

        let first = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(3),
                method: "thread/goal/continuation".to_string(),
                params: json!({ "threadId": thread_id, "maxContinuations": 1 }),
            },
        );
        let first = first.result.unwrap();
        assert_eq!(first["continuation"], json!(1));
        assert!(
            first["prompt"]
                .as_str()
                .unwrap()
                .contains("Ship continuation")
        );

        let blocked = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(4),
                method: "thread/goal/continuation".to_string(),
                params: json!({ "threadId": thread_id, "maxContinuations": 1 }),
            },
        );
        let blocked = blocked.result.unwrap();
        assert!(blocked["prompt"].is_null());
        assert_eq!(blocked["goal"]["status"], json!("paused"));
        assert!(blocked["blockedReason"].as_str().unwrap().contains("guard"));
    }

    #[test]
    fn auth_token_gate_rejects_unauthenticated_non_initialize_requests() {
        let mut runtime = Runtime::new();
        let response = handle_request_with_auth(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(1),
                method: "thread/start".to_string(),
                params: json!({ "project": { "id": "project", "cwd": "/repo" } }),
            },
            Some("secret"),
        );
        assert_eq!(response.error.unwrap().code, -32001);
    }

    #[test]
    fn auth_token_gate_accepts_matching_capability_token() {
        let mut runtime = Runtime::new();
        let response = handle_request_with_auth(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(1),
                method: "thread/start".to_string(),
                params: json!({
                    "authToken": "secret",
                    "project": { "id": "project", "cwd": "/repo" }
                }),
            },
            Some("secret"),
        );
        assert!(response.error.is_none(), "{:?}", response.error);
        assert_eq!(response.result.unwrap()["thread"]["id"], json!("thread-1"));
    }

    #[test]
    fn runtime_errors_include_stable_code_and_category() {
        let mut runtime = Runtime::new();
        let response = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(1),
                method: "events/list".to_string(),
                params: json!({ "threadId": "missing-thread" }),
            },
        );
        let error = response.error.expect("runtime error");
        assert_eq!(error.code, -32000);
        assert_eq!(
            error.data.as_ref().unwrap()["code"],
            json!("thread_not_found")
        );
        assert_eq!(error.data.unwrap()["category"], json!("not_found"));
    }

    #[test]
    fn background_agentic_turn_mirrors_events_for_polling() {
        let state = ServerState::new();
        let start = response_for_line_with_state(
            &state,
            &json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "thread/start",
                "params": { "project": { "id": "project", "cwd": "/repo" } }
            })
            .to_string(),
        );
        assert!(start.error.is_none(), "{:?}", start.error);
        let thread_id = start.result.unwrap()["thread"]["id"]
            .as_str()
            .unwrap()
            .to_string();

        let run = response_for_line_with_state(
            &state,
            &json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "turn/run-agentic",
                "params": {
                    "threadId": thread_id,
                    "input": "ask before continuing",
                    "executionMode": "background",
                    "modelSteps": [{
                        "toolCalls": [{
                            "id": "ask-1",
                            "namespace": "oppi",
                            "name": "ask_user",
                            "arguments": {
                                "title": "Choose",
                                "questions": [{
                                    "id": "q1",
                                    "question": "Proceed?",
                                    "options": [{ "id": "yes", "label": "Yes" }]
                                }]
                            }
                        }]
                    }]
                }
            })
            .to_string(),
        );
        assert!(run.error.is_none(), "{:?}", run.error);
        let result = run.result.unwrap();
        let turn_id = result["turn"]["id"].as_str().unwrap().to_string();
        assert!(
            result["turn"]["status"] == json!("running")
                || result["turn"]["status"] == json!("waitingForUser")
        );
        assert!(
            result["events"]
                .as_array()
                .unwrap()
                .iter()
                .any(|event| { event["kind"]["type"] == json!("turnStarted") })
        );

        let mut listed = Vec::new();
        for _ in 0..50 {
            let response = response_for_line_with_state(
                &state,
                &json!({
                    "jsonrpc": "2.0",
                    "id": 3,
                    "method": "events/list",
                    "params": { "threadId": thread_id, "after": 0 }
                })
                .to_string(),
            );
            assert!(response.error.is_none(), "{:?}", response.error);
            listed = response.result.unwrap()["events"]
                .as_array()
                .unwrap()
                .clone();
            if listed
                .iter()
                .any(|event| event["kind"]["type"] == json!("askUserRequested"))
            {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert!(listed.iter().any(|event| {
            event["turnId"] == json!(turn_id) && event["kind"]["type"] == json!("askUserRequested")
        }));
    }

    #[test]
    fn background_agentic_turn_streams_deltas_before_completion() {
        let state = ServerState::new();
        let start = response_for_line_with_state(
            &state,
            &json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "thread/start",
                "params": { "project": { "id": "project", "cwd": "/repo" } }
            })
            .to_string(),
        );
        assert!(start.error.is_none(), "{:?}", start.error);
        let thread_id = start.result.unwrap()["thread"]["id"]
            .as_str()
            .unwrap()
            .to_string();

        let run = response_for_line_with_state(
            &state,
            &json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "turn/run-agentic",
                "params": {
                    "threadId": thread_id,
                    "input": "stream while slow tool runs",
                    "executionMode": "background",
                    "modelSteps": [
                        {
                            "assistantDeltas": ["streamed ", "delta"],
                            "toolCalls": [{
                                "id": "slow-echo",
                                "namespace": "oppi",
                                "name": "echo",
                                "arguments": { "output": "slow", "delayMs": 250 }
                            }]
                        },
                        { "assistantDeltas": ["done"], "finalResponse": true }
                    ],
                    "maxContinuations": 2
                }
            })
            .to_string(),
        );
        assert!(run.error.is_none(), "{:?}", run.error);
        let turn_id = run.result.unwrap()["turn"]["id"]
            .as_str()
            .unwrap()
            .to_string();

        let mut listed = Vec::new();
        let mut saw_delta_before_completion = false;
        for _ in 0..50 {
            let response = response_for_line_with_state(
                &state,
                &json!({
                    "jsonrpc": "2.0",
                    "id": 3,
                    "method": "events/list",
                    "params": { "threadId": thread_id, "after": 0 }
                })
                .to_string(),
            );
            assert!(response.error.is_none(), "{:?}", response.error);
            listed = response.result.unwrap()["events"]
                .as_array()
                .unwrap()
                .clone();
            let saw_delta = listed.iter().any(|event| {
                event["turnId"] == json!(turn_id) && event["kind"]["type"] == json!("itemDelta")
            });
            let saw_completed = listed.iter().any(|event| {
                event["turnId"] == json!(turn_id) && event["kind"]["type"] == json!("turnCompleted")
            });
            if saw_delta && !saw_completed {
                saw_delta_before_completion = true;
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert!(saw_delta_before_completion, "events: {listed:?}");
    }

    #[test]
    fn background_agentic_turn_can_be_interrupted_while_runtime_busy() {
        let state = ServerState::new();
        let start = response_for_line_with_state(
            &state,
            &json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "thread/start",
                "params": { "project": { "id": "project", "cwd": "/repo" } }
            })
            .to_string(),
        );
        assert!(start.error.is_none(), "{:?}", start.error);
        let thread_id = start.result.unwrap()["thread"]["id"]
            .as_str()
            .unwrap()
            .to_string();

        let run = response_for_line_with_state(
            &state,
            &json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "turn/run-agentic",
                "params": {
                    "threadId": thread_id,
                    "input": "interrupt slow tool",
                    "executionMode": "background",
                    "modelSteps": [{
                        "assistantDeltas": ["working"],
                        "toolCalls": [{
                            "id": "slow-echo",
                            "namespace": "oppi",
                            "name": "echo",
                            "arguments": { "output": "slow", "delayMs": 250 }
                        }]
                    }],
                    "maxContinuations": 2
                }
            })
            .to_string(),
        );
        assert!(run.error.is_none(), "{:?}", run.error);
        let turn_id = run.result.unwrap()["turn"]["id"]
            .as_str()
            .unwrap()
            .to_string();

        let interrupted = response_for_line_with_state(
            &state,
            &json!({
                "jsonrpc": "2.0",
                "id": 3,
                "method": "turn/interrupt",
                "params": { "threadId": thread_id, "turnId": turn_id, "reason": "test interrupt" }
            })
            .to_string(),
        );
        assert!(interrupted.error.is_none(), "{:?}", interrupted.error);
        let events = interrupted.result.unwrap()["events"]
            .as_array()
            .unwrap()
            .clone();
        assert!(
            events.iter().any(|event| {
                event["turnId"] == json!(turn_id)
                    && event["kind"]["type"] == json!("turnInterrupted")
            }),
            "events: {events:?}"
        );

        let listed = response_for_line_with_state(
            &state,
            &json!({
                "jsonrpc": "2.0",
                "id": 4,
                "method": "events/list",
                "params": { "threadId": thread_id, "after": 0 }
            })
            .to_string(),
        );
        assert!(listed.error.is_none(), "{:?}", listed.error);
        let all_events = listed.result.unwrap()["events"].as_array().unwrap().clone();
        assert!(
            !all_events.iter().any(|event| {
                event["turnId"] == json!(turn_id) && event["kind"]["type"] == json!("turnCompleted")
            }),
            "events: {all_events:?}"
        );
    }

    #[test]
    fn background_direct_provider_approval_resume_preserves_tool_transcript() {
        let root = std::env::temp_dir().join(format!(
            "oppi-server-bg-resume-{}-{}",
            std::process::id(),
            1
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let first = json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "write-approved",
                        "type": "function",
                        "function": {
                            "name": "oppi__write_file",
                            "arguments": "{\"path\":\"approved.txt\",\"content\":\"approved\"}"
                        }
                    }]
                }
            }]
        })
        .to_string();
        let second = json!({
            "choices": [{
                "message": { "role": "assistant", "content": "continued after approval" }
            }]
        })
        .to_string();
        let (base_url, server) = start_mock_openai_server(vec![first, second]);
        let key_name = "OPPI_SERVER_BG_RESUME_API_KEY";
        unsafe { std::env::set_var(key_name, "test-key") };
        let state = ServerState::new();
        let start = response_for_line_with_state(
            &state,
            &json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "thread/start",
                "params": { "project": { "id": "project", "cwd": root.display().to_string() } }
            })
            .to_string(),
        );
        assert!(start.error.is_none(), "{:?}", start.error);
        let thread_id = start.result.unwrap()["thread"]["id"]
            .as_str()
            .unwrap()
            .to_string();

        let run = response_for_line_with_state(
            &state,
            &json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "turn/run-agentic",
                "params": {
                    "threadId": thread_id,
                    "input": "write with approval",
                    "executionMode": "background",
                    "modelProvider": {
                        "kind": "openai-compatible",
                        "model": "mock",
                        "baseUrl": base_url,
                        "apiKeyEnv": key_name,
                        "stream": false
                    },
                    "maxContinuations": 2
                }
            })
            .to_string(),
        );
        assert!(run.error.is_none(), "{:?}", run.error);
        let turn_id = run.result.unwrap()["turn"]["id"]
            .as_str()
            .unwrap()
            .to_string();

        let mut listed = Vec::new();
        for _ in 0..50 {
            let response = response_for_line_with_state(
                &state,
                &json!({
                    "jsonrpc": "2.0",
                    "id": 3,
                    "method": "events/list",
                    "params": { "threadId": thread_id, "after": 0 }
                })
                .to_string(),
            );
            assert!(response.error.is_none(), "{:?}", response.error);
            listed = response.result.unwrap()["events"]
                .as_array()
                .unwrap()
                .clone();
            if listed.iter().any(|event| {
                event["turnId"] == json!(turn_id)
                    && event["kind"]["type"] == json!("approvalRequested")
            }) {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert!(
            listed.iter().any(|event| {
                event["turnId"] == json!(turn_id)
                    && event["kind"]["type"] == json!("approvalRequested")
            }),
            "events: {listed:?}"
        );

        let resumed = response_for_line_with_state(
            &state,
            &json!({
                "jsonrpc": "2.0",
                "id": 4,
                "method": "turn/resume-agentic",
                "params": {
                    "threadId": thread_id,
                    "turnId": turn_id,
                    "approvedToolCallIds": ["write-approved"],
                    "maxContinuations": 2
                }
            })
            .to_string(),
        );
        unsafe { std::env::remove_var(key_name) };
        assert!(resumed.error.is_none(), "{:?}", resumed.error);
        assert_eq!(
            resumed.result.as_ref().unwrap()["turn"]["status"],
            json!("completed")
        );
        assert_eq!(
            std::fs::read_to_string(root.join("approved.txt")).unwrap(),
            "approved"
        );
        let requests = server.join().unwrap();
        assert!(
            requests[1].contains("\"role\":\"tool\""),
            "request: {}",
            requests[1]
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn lists_background_tasks_over_json_rpc() {
        let mut runtime = Runtime::new();
        let response = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(1),
                method: "background/list".to_string(),
                params: json!({}),
            },
        );
        assert!(response.error.is_none(), "{:?}", response.error);
        assert_eq!(response.result.unwrap()["items"], json!([]));
    }

    #[test]
    fn runs_agentic_turn_over_json_rpc() {
        let mut runtime = Runtime::new();
        let start = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(1),
                method: "thread/start".to_string(),
                params: json!({ "project": { "id": "project", "cwd": "/repo" } }),
            },
        );
        assert!(start.error.is_none(), "{:?}", start.error);
        let run = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(2),
                method: "turn/run-agentic".to_string(),
                params: json!({
                    "threadId": "thread-1",
                    "input": "call echo",
                    "modelSteps": [
                        {
                            "assistantDeltas": ["Calling echo."],
                            "toolCalls": [
                                {
                                    "id": "echo-1",
                                    "name": "echo",
                                    "namespace": "oppi",
                                    "arguments": { "output": "hello" }
                                }
                            ]
                        },
                        {
                            "assistantDeltas": ["Done."],
                            "finalResponse": true
                        }
                    ]
                }),
            },
        );
        assert!(run.error.is_none(), "{:?}", run.error);
        let result = run.result.unwrap();
        assert_eq!(result["turn"]["status"], json!("completed"));
        assert!(
            result["events"]
                .as_array()
                .unwrap()
                .iter()
                .any(|event| { event["kind"]["type"] == json!("toolCallCompleted") })
        );
    }

    #[test]
    fn runtime_metrics_and_debug_bundle_are_available_over_json_rpc() {
        let mut runtime = Runtime::new();
        let start = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(1),
                method: "thread/start".to_string(),
                params: json!({ "project": { "id": "project", "cwd": "/repo" } }),
            },
        );
        assert!(start.error.is_none(), "{:?}", start.error);

        let metrics = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(2),
                method: "runtime/metrics".to_string(),
                params: json!({}),
            },
        );
        assert!(metrics.error.is_none(), "{:?}", metrics.error);
        assert_eq!(metrics.result.unwrap()["threadCount"], json!(1));

        let bundle = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(3),
                method: "debug/bundle".to_string(),
                params: json!({}),
            },
        );
        assert!(bundle.error.is_none(), "{:?}", bundle.error);
        let result = bundle.result.unwrap();
        assert_eq!(result["redacted"], json!(true));
        assert_eq!(result["metrics"]["threadCount"], json!(1));
        assert!(
            result["diagnostics"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| { item["metadata"]["component"] == json!("sandbox") })
        );
    }

    #[test]
    fn server_shutdown_returns_ack_and_stdio_shutdown_action() {
        let mut runtime = Runtime::new();
        let (response, action) = response_for_line_with_action(
            &mut runtime,
            r#"{"jsonrpc":"2.0","id":"shutdown","method":"server/shutdown","params":{}}"#,
        );
        assert_eq!(action, StdioAction::Shutdown);
        assert!(response.error.is_none(), "{:?}", response.error);
        assert_eq!(response.id, json!("shutdown"));
        assert_eq!(response.result.unwrap()["shuttingDown"], json!(true));
    }

    #[test]
    fn persistent_state_replays_threads_and_events_across_server_restart() {
        let root = temp_store_root("oppi-server-persist");
        let namespace = StoreNamespace::new("project", "runtime");
        let state = ServerState::with_persistence(root.clone(), namespace.clone()).unwrap();
        let start = response_for_line_with_state(
            &state,
            &json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "thread/start",
                "params": { "project": { "id": "project", "cwd": "/repo" }, "title": "Persisted" }
            })
            .to_string(),
        );
        assert!(start.error.is_none(), "{:?}", start.error);
        let thread_id = start.result.as_ref().unwrap()["thread"]["id"]
            .as_str()
            .unwrap();
        assert_eq!(thread_id, "thread-1");
        drop(state);

        let restarted = ServerState::with_persistence(root.clone(), namespace).unwrap();
        let list = response_for_line_with_state(
            &restarted,
            &json!({ "jsonrpc": "2.0", "id": 2, "method": "thread/list", "params": {} })
                .to_string(),
        );
        assert!(list.error.is_none(), "{:?}", list.error);
        assert_eq!(
            list.result.as_ref().unwrap()["items"][0]["id"],
            json!("thread-1")
        );
        let events = response_for_line_with_state(
            &restarted,
            &json!({ "jsonrpc": "2.0", "id": 3, "method": "events/list", "params": { "threadId": "thread-1", "after": 0 } }).to_string(),
        );
        assert!(events.error.is_none(), "{:?}", events.error);
        assert!(events.result.unwrap()["events"].as_array().unwrap().len() >= 1);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn persistent_state_skips_corrupt_threads_without_reusing_their_ids() {
        let root = temp_store_root("oppi-server-corrupt-thread");
        let namespace = StoreNamespace::new("project", "runtime");
        let state = ServerState::with_persistence(root.clone(), namespace.clone()).unwrap();
        let start = response_for_line_with_state(
            &state,
            &json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "thread/start",
                "params": { "project": { "id": "project", "cwd": "/repo" }, "title": "Persisted" }
            })
            .to_string(),
        );
        assert!(start.error.is_none(), "{:?}", start.error);
        drop(state);

        let corrupt_path = root
            .join("projects")
            .join("project")
            .join("runtimes")
            .join("runtime")
            .join("events")
            .join("thread-9.jsonl");
        std::fs::write(corrupt_path, "{{not valid json}}\n").unwrap();

        let restarted = ServerState::with_persistence(root.clone(), namespace).unwrap();
        let list = response_for_line_with_state(
            &restarted,
            &json!({ "jsonrpc": "2.0", "id": 2, "method": "thread/list", "params": {} })
                .to_string(),
        );
        assert!(list.error.is_none(), "{:?}", list.error);
        assert_eq!(
            list.result.as_ref().unwrap()["items"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            list.result.as_ref().unwrap()["items"][0]["id"],
            json!("thread-1")
        );

        let next = response_for_line_with_state(
            &restarted,
            &json!({
                "jsonrpc": "2.0",
                "id": 3,
                "method": "thread/start",
                "params": { "project": { "id": "project", "cwd": "/repo" }, "title": "Next" }
            })
            .to_string(),
        );
        assert!(next.error.is_none(), "{:?}", next.error);
        assert_eq!(
            next.result.as_ref().unwrap()["thread"]["id"],
            json!("thread-10")
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn todo_client_actions_mark_done_and_clear_with_events() {
        let state = ServerState::without_persistence();
        let start = response_for_line_with_state(
            &state,
            &json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "thread/start",
                "params": { "project": { "id": "project", "cwd": "/repo" }, "title": "Todos" }
            })
            .to_string(),
        );
        assert!(start.error.is_none(), "{:?}", start.error);
        let thread_id = start.result.as_ref().unwrap()["thread"]["id"]
            .as_str()
            .unwrap();

        let seed = response_for_line_with_state(
            &state,
            &json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "turn/run-agentic",
                "params": {
                    "threadId": thread_id,
                    "input": "seed todos",
                    "modelSteps": [{
                        "toolCalls": [{
                            "id": "todo-seed",
                            "name": "todo_write",
                            "arguments": {
                                "summary": "Seeded todos",
                                "todos": [
                                    { "id": "impl", "content": "Implement todo actions", "status": "in_progress", "priority": "high" },
                                    { "id": "verify", "content": "Verify todo actions", "status": "pending", "priority": "medium" }
                                ]
                            }
                        }],
                        "finalResponse": true
                    }]
                }
            })
            .to_string(),
        );
        assert!(seed.error.is_none(), "{:?}", seed.error);

        let done = response_for_line_with_state(
            &state,
            &json!({
                "jsonrpc": "2.0",
                "id": 3,
                "method": "todos/client-action",
                "params": { "threadId": thread_id, "action": "done", "id": "impl" }
            })
            .to_string(),
        );
        assert!(done.error.is_none(), "{:?}", done.error);
        assert_eq!(
            done.result.as_ref().unwrap()["state"]["todos"][0]["status"],
            json!("completed")
        );
        assert_eq!(
            done.result.as_ref().unwrap()["state"]["todos"][1]["status"],
            json!("pending")
        );
        assert!(
            done.result.as_ref().unwrap()["events"]
                .as_array()
                .unwrap()
                .iter()
                .any(|event| event["kind"]["type"] == json!("todosUpdated"))
        );

        let clear = response_for_line_with_state(
            &state,
            &json!({
                "jsonrpc": "2.0",
                "id": 4,
                "method": "todos/client-action",
                "params": { "threadId": thread_id, "action": "clear" }
            })
            .to_string(),
        );
        assert!(clear.error.is_none(), "{:?}", clear.error);
        assert_eq!(clear.result.as_ref().unwrap()["state"]["todos"], json!([]));
        assert!(
            clear.result.as_ref().unwrap()["events"]
                .as_array()
                .unwrap()
                .iter()
                .any(|event| event["kind"]["type"] == json!("todosUpdated"))
        );
    }

    #[test]
    fn malformed_json_lines_return_parse_errors_without_mutation() {
        let mut runtime = Runtime::new();
        let malformed_lines = [
            "{",
            "not json",
            "[]",
            "null",
            "{\"jsonrpc\":\"2.0\",\"id\":1}",
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":42}",
        ];

        for line in malformed_lines {
            let response = response_for_line(&mut runtime, line);
            assert_eq!(response.id, Value::Null, "line: {line}");
            assert_eq!(
                response.error.as_ref().unwrap().code,
                -32700,
                "line: {line}"
            );
            serde_json::to_string(&response).expect("response remains JSON serializable");
        }

        let follow_up = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(1),
                method: "thread/start".to_string(),
                params: json!({ "project": { "id": "project", "cwd": "/repo" } }),
            },
        );
        assert!(follow_up.error.is_none(), "{:?}", follow_up.error);
        assert_eq!(follow_up.result.unwrap()["thread"]["id"], json!("thread-1"));
    }

    #[test]
    fn malformed_protocol_params_do_not_panic_or_mutate_runtime() {
        let mut runtime = Runtime::new();
        let malformed_params = [
            json!(null),
            json!([]),
            json!(42),
            json!("string"),
            json!({}),
            json!({ "threadId": 42, "turnId": [], "project": { "id": null } }),
            json!({ "threadId": "missing", "turnId": "missing", "input": { "nested": true } }),
        ];
        let methods = [
            "initialize",
            "thread/start",
            "thread/items",
            "thread/resume",
            "thread/fork",
            "thread/item-fork",
            "thread/item-edit-fork",
            "turn/start",
            "turn/steer",
            "turn/interrupt",
            "tool/record",
            "tool/batch",
            "approval/request",
            "approval/respond",
            "question/request",
            "question/respond",
            "plugin/register",
            "memory/set",
            "memory/jobs/update",
            "memory/list",
            "memory/recall",
            "memory/backend/status",
            "memory/backend/recall",
            "memory/backend/remember",
            "memory/backend/list",
            "memory/backend/dashboard",
            "memory/backend/forget",
            "memory/backend/pin",
            "memory/backend/update",
            "memory/backend/supersede",
            "memory/backend/consolidate",
            "memory/backend/sync",
            "memory/consolidate",
            "memory/distill-turn",
            "memory/recurring-candidates",
            "memory/records/set",
            "memory/record/forget",
            "memory/record/pin",
            "memory/record/update",
            "memory/record/supersede",
            "memory/dashboard",
            "memory/control",
            "memory/compact",
            "mcp/register",
            "mcp/action",
            "model/register",
            "model/registry",
            "model/select",
            "agents/register",
            "agents/dispatch",
            "agents/block",
            "agents/complete",
            "side-question/ask",
            "pi/bridge-event",
            "events/list",
            "sandbox/plan",
        ];

        let mut id = 0;
        for method in methods {
            for params in &malformed_params {
                id += 1;
                let response = handle_request(
                    &mut runtime,
                    JsonRpcRequest {
                        jsonrpc: Some("2.0".to_string()),
                        id: json!(id),
                        method: method.to_string(),
                        params: params.clone(),
                    },
                );
                assert!(
                    response.result.is_none(),
                    "{method} unexpectedly succeeded for {params}"
                );
                assert!(
                    response.error.is_some(),
                    "{method} returned neither result nor error"
                );
                serde_json::to_string(&response).expect("response remains JSON serializable");
            }
        }

        let follow_up = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!("after-fuzz"),
                method: "thread/start".to_string(),
                params: json!({ "project": { "id": "project", "cwd": "/repo" } }),
            },
        );
        assert!(follow_up.error.is_none(), "{:?}", follow_up.error);
        assert_eq!(follow_up.result.unwrap()["thread"]["id"], json!("thread-1"));
    }

    #[test]
    fn model_registry_route_returns_structured_catalog_result() {
        let mut runtime = Runtime::new();
        let response = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!("model-registry"),
                method: "model/registry".to_string(),
                params: json!({
                    "provider": "openai-codex",
                    "selectedModelId": "gpt-5.5",
                    "scope": ["openai-codex/gpt-5.4"],
                    "availableOnly": false
                }),
            },
        );

        assert!(response.error.is_none(), "{:?}", response.error);
        let result = response.result.unwrap();
        assert_eq!(result["provider"], json!("openai-codex"));
        assert_eq!(result["availableOnly"], json!(false));
        assert_eq!(result["selectedModel"]["id"], json!("gpt-5.5"));
        assert_eq!(result["items"][0]["id"], json!("gpt-5.5"));
        assert!(result["items"].as_array().unwrap().iter().any(|item| {
            item["id"] == json!("gpt-5.4") && item["provider"] == json!("openai-codex")
        }));
    }

    #[test]
    fn panic_boundary_returns_internal_error_and_keeps_server_usable() {
        let mut runtime = Runtime::new();
        let panic_response = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!("panic-id"),
                method: "__test/panic".to_string(),
                params: json!({}),
            },
        );
        let error = panic_response.error.expect("panic response error");
        assert_eq!(panic_response.id, json!("panic-id"));
        assert_eq!(error.code, -32603);
        assert_eq!(error.data.unwrap()["code"], json!("internal_panic"));

        let follow_up = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(2),
                method: "thread/start".to_string(),
                params: json!({ "project": { "id": "project", "cwd": "/repo" } }),
            },
        );
        assert!(follow_up.error.is_none(), "{:?}", follow_up.error);
        assert_eq!(follow_up.result.unwrap()["thread"]["id"], json!("thread-1"));
    }

    #[test]
    fn invalid_params_return_json_rpc_invalid_params_without_mutation() {
        let mut runtime = Runtime::new();
        let bad = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(1),
                method: "thread/start".to_string(),
                params: json!({ "project": { "id": 42 } }),
            },
        );
        assert_eq!(bad.error.as_ref().unwrap().code, -32602);

        let good = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(2),
                method: "thread/start".to_string(),
                params: json!({ "project": { "id": "project", "cwd": "/repo" } }),
            },
        );
        assert!(good.error.is_none(), "{:?}", good.error);
        assert_eq!(good.result.unwrap()["thread"]["id"], json!("thread-1"));
    }

    #[test]
    fn returns_sandbox_plan_over_json_rpc() {
        let mut runtime = Runtime::new();
        let response = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(1),
                method: "sandbox/plan".to_string(),
                params: json!({
                    "preference": "auto",
                    "policy": {
                        "permissionProfile": {
                            "mode": "default",
                            "writableRoots": ["/repo"],
                            "protectedPatterns": []
                        },
                        "network": "disabled",
                        "filesystem": "workspaceWrite"
                    },
                    "request": {
                        "command": "echo hi",
                        "cwd": "/repo",
                        "writesFiles": false,
                        "usesNetwork": false
                    }
                }),
            },
        );
        assert!(response.error.is_none(), "{:?}", response.error);
        let result = response.result.unwrap();
        assert_eq!(result["decision"]["type"], json!("allow"));
        assert!(
            matches!(
                result["plan"]["enforcement"].as_str(),
                Some("reviewOnly" | "osSandbox")
            ),
            "unexpected enforcement: {}",
            result["plan"]["enforcement"]
        );
    }

    #[test]
    fn sandbox_exec_requires_configured_auth_token() {
        let mut runtime = Runtime::new();
        let response = handle_request_with_auth(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(1),
                method: "sandbox/exec".to_string(),
                params: json!({}),
            },
            None,
        );
        assert_eq!(response.error.unwrap().code, -32001);
    }

    #[test]
    fn windows_wfp_install_requires_configured_auth_token() {
        let mut runtime = Runtime::new();
        let response = handle_request_with_auth(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(1),
                method: "sandbox/windows-wfp-install".to_string(),
                params: json!({ "account": "OPPiSandbox" }),
            },
            None,
        );
        assert_eq!(response.error.unwrap().code, -32001);
    }

    #[test]
    fn windows_wfp_status_returns_json_shape() {
        let mut runtime = Runtime::new();
        let response = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(1),
                method: "sandbox/windows-wfp-status".to_string(),
                params: json!({}),
            },
        );
        assert!(response.error.is_none(), "{:?}", response.error);
        let result = response.result.unwrap();
        assert!(result.get("available").is_some());
        assert!(result.get("filterCount").is_some());
        assert!(result.get("message").is_some());
    }

    #[test]
    fn sandbox_exec_returns_policy_denial_over_json_rpc() {
        let mut runtime = Runtime::new();
        let response = handle_request_with_auth(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(1),
                method: "sandbox/exec".to_string(),
                params: json!({
                    "authToken": "secret",
                    "preference": "auto",
                    "policy": {
                        "permissionProfile": {
                            "mode": "read-only",
                            "writableRoots": ["/repo"],
                            "protectedPatterns": []
                        },
                        "network": "ask",
                        "filesystem": "readOnly"
                    },
                    "request": {
                        "command": "echo hi > file",
                        "cwd": "/repo",
                        "writesFiles": true,
                        "usesNetwork": false
                    }
                }),
            },
            Some("secret"),
        );
        assert!(response.error.is_none(), "{:?}", response.error);
        assert_eq!(response.result.unwrap()["decision"]["type"], json!("deny"));
    }

    #[test]
    fn records_parallel_tool_batch_over_json_rpc() {
        let mut runtime = Runtime::new();
        let start = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(1),
                method: "thread/start".to_string(),
                params: json!({ "project": { "id": "project", "cwd": "/repo" } }),
            },
        );
        assert!(start.error.is_none());
        let turn = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(2),
                method: "turn/start".to_string(),
                params: json!({ "threadId": "thread-1", "input": "inspect", "deferCompletion": true }),
            },
        );
        assert!(turn.error.is_none());
        let batch = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(3),
                method: "tool/batch".to_string(),
                params: json!({
                    "threadId": "thread-1",
                    "turnId": "turn-1",
                    "maxConcurrency": 10,
                    "tools": [
                        {
                            "concurrencySafe": true,
                            "call": { "id": "read-1", "name": "read", "arguments": { "path": "README.md" } },
                            "result": { "callId": "read-1", "status": "ok", "output": "read" }
                        },
                        {
                            "concurrencySafe": true,
                            "call": { "id": "grep-1", "name": "grep", "arguments": { "pattern": "OPPi" } },
                            "result": { "callId": "grep-1", "status": "ok", "output": "grep" }
                        },
                        {
                            "call": { "id": "edit-1", "name": "edit", "arguments": {} },
                            "result": { "callId": "edit-1", "status": "ok", "output": "edited" }
                        }
                    ]
                }),
            },
        );
        assert!(batch.error.is_none(), "{:?}", batch.error);
        let result = batch.result.unwrap();
        assert_eq!(result["batches"].as_array().unwrap().len(), 2);
        assert_eq!(result["batches"][0]["execution"], json!("concurrent"));
        assert_eq!(result["batches"][1]["execution"], json!("exclusive"));
    }

    #[test]
    fn lists_thread_items_over_json_rpc_with_runtime_filters() {
        let mut runtime = Runtime::new();
        let start = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(1),
                method: "thread/start".to_string(),
                params: json!({ "project": { "id": "project", "cwd": "/repo" } }),
            },
        );
        assert!(start.error.is_none(), "{:?}", start.error);
        let turn = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(2),
                method: "turn/start".to_string(),
                params: json!({
                    "threadId": "thread-1",
                    "input": "inspect",
                    "assistantResponse": "done",
                    "simulatedTool": {
                        "concurrencySafe": true,
                        "call": { "id": "read-1", "name": "read", "arguments": { "path": "README.md" } },
                        "result": { "callId": "read-1", "status": "ok", "output": "read" }
                    }
                }),
            },
        );
        assert!(turn.error.is_none(), "{:?}", turn.error);

        let default_response = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(3),
                method: "thread/items".to_string(),
                params: json!({ "threadId": "thread-1" }),
            },
        );
        assert!(
            default_response.error.is_none(),
            "{:?}",
            default_response.error
        );
        let default_result = default_response.result.unwrap();
        assert_eq!(default_result["filter"], json!("default"));
        assert_eq!(default_result["totalCount"], json!(2));
        assert_eq!(default_result["items"].as_array().unwrap().len(), 2);
        assert!(
            default_result["items"]
                .as_array()
                .unwrap()
                .iter()
                .all(|item| {
                    item["kind"]["type"] != json!("toolCall")
                        && item["kind"]["type"] != json!("toolResult")
                })
        );

        let all_response = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(4),
                method: "thread/items".to_string(),
                params: json!({ "threadId": "thread-1", "filter": "all", "limit": 3 }),
            },
        );
        assert!(all_response.error.is_none(), "{:?}", all_response.error);
        let all_result = all_response.result.unwrap();
        assert_eq!(all_result["filter"], json!("all"));
        assert_eq!(all_result["after"], json!(0));
        assert_eq!(all_result["totalCount"], json!(4));
        assert_eq!(all_result["entries"].as_array().unwrap().len(), 3);
        assert!(all_result["entries"][0]["completedAtMs"].as_u64().unwrap() > 0);
        assert_eq!(all_result["entries"][0]["parentEventId"], Value::Null);
        assert_eq!(
            all_result["entries"][1]["parentEventId"],
            all_result["entries"][0]["eventId"]
        );
        assert_eq!(
            all_result["entries"][0]["childEventIds"],
            json!([all_result["entries"][1]["eventId"].as_u64().unwrap()])
        );
        assert!(
            all_result["entries"][2]["childEventIds"][0]
                .as_u64()
                .unwrap()
                > all_result["entries"][2]["eventId"].as_u64().unwrap()
        );
        assert_eq!(
            all_result["entries"][0]["item"]["id"],
            all_result["items"][0]["id"]
        );
        assert_eq!(all_result["items"].as_array().unwrap().len(), 3);
        assert!(
            all_result["items"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| { item["kind"]["type"] == json!("toolCall") })
        );

        let after_first = all_result["entries"][0]["eventId"].as_u64().unwrap();
        let label_response = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(5),
                method: "thread/item-labels/set".to_string(),
                params: json!({
                    "threadId": "thread-1",
                    "eventId": after_first,
                    "labels": ["checkpoint"]
                }),
            },
        );
        assert!(label_response.error.is_none(), "{:?}", label_response.error);
        let label_result = label_response.result.unwrap();
        assert_eq!(label_result["eventId"], json!(after_first));
        assert_eq!(label_result["labels"], json!(["checkpoint"]));
        assert_eq!(
            label_result["events"][0]["kind"]["type"],
            json!("threadItemLabelsUpdated")
        );
        assert_eq!(
            label_result["events"][0]["kind"]["targetEventId"],
            json!(after_first)
        );

        let labeled_response = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(6),
                method: "thread/items".to_string(),
                params: json!({ "threadId": "thread-1", "filter": "labeled-only" }),
            },
        );
        assert!(
            labeled_response.error.is_none(),
            "{:?}",
            labeled_response.error
        );
        let labeled_result = labeled_response.result.unwrap();
        assert_eq!(labeled_result["filter"], json!("labeled-only"));
        assert_eq!(labeled_result["totalCount"], json!(1));
        assert_eq!(labeled_result["entries"][0]["eventId"], json!(after_first));
        assert_eq!(
            labeled_result["entries"][0]["labels"],
            json!(["checkpoint"])
        );

        let cursor_response = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(7),
                method: "thread/items".to_string(),
                params: json!({ "threadId": "thread-1", "filter": "all", "after": after_first }),
            },
        );
        assert!(
            cursor_response.error.is_none(),
            "{:?}",
            cursor_response.error
        );
        let cursor_result = cursor_response.result.unwrap();
        assert_eq!(cursor_result["after"], json!(after_first));
        assert_eq!(cursor_result["totalCount"], json!(3));
        assert!(
            cursor_result["entries"]
                .as_array()
                .unwrap()
                .iter()
                .all(|entry| { entry["eventId"].as_u64().unwrap() > after_first })
        );
        assert_eq!(
            cursor_result["entries"][0]["parentEventId"],
            json!(after_first)
        );
        assert_eq!(
            cursor_result["entries"][0]["childEventIds"],
            json!([cursor_result["entries"][1]["eventId"].as_u64().unwrap()])
        );
    }

    #[test]
    fn lists_thread_item_branch_metadata_over_json_rpc() {
        let mut runtime = Runtime::new();
        let start = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(1),
                method: "thread/start".to_string(),
                params: json!({ "project": { "id": "project", "cwd": "/repo" } }),
            },
        );
        assert!(start.error.is_none(), "{:?}", start.error);
        for (id, parent, title) in [
            (2, "thread-1", "first child"),
            (3, "thread-1", "second child"),
            (4, "thread-2", "grandchild"),
        ] {
            let response = handle_request(
                &mut runtime,
                JsonRpcRequest {
                    jsonrpc: Some("2.0".to_string()),
                    id: json!(id),
                    method: "thread/fork".to_string(),
                    params: json!({ "threadId": parent, "title": title }),
                },
            );
            assert!(response.error.is_none(), "{:?}", response.error);
        }

        let root_response = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(5),
                method: "thread/items".to_string(),
                params: json!({ "threadId": "thread-1" }),
            },
        );
        assert!(root_response.error.is_none(), "{:?}", root_response.error);
        let root_result = root_response.result.unwrap();
        assert_eq!(root_result["branch"]["threadId"], json!("thread-1"));
        assert_eq!(root_result["branch"]["parentThreadId"], Value::Null);
        assert_eq!(
            root_result["branch"]["childThreadIds"],
            json!(["thread-2", "thread-3"])
        );
        assert_eq!(root_result["branch"]["siblingThreadIds"], json!([]));
        assert_eq!(root_result["branch"]["path"], json!(["thread-1"]));
        assert_eq!(root_result["branch"]["depth"], json!(0));

        let grandchild_response = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(6),
                method: "thread/items".to_string(),
                params: json!({ "threadId": "thread-4" }),
            },
        );
        assert!(
            grandchild_response.error.is_none(),
            "{:?}",
            grandchild_response.error
        );
        let grandchild_result = grandchild_response.result.unwrap();
        assert_eq!(grandchild_result["branch"]["threadId"], json!("thread-4"));
        assert_eq!(
            grandchild_result["branch"]["parentThreadId"],
            json!("thread-2")
        );
        assert_eq!(grandchild_result["branch"]["childThreadIds"], json!([]));
        assert_eq!(grandchild_result["branch"]["siblingThreadIds"], json!([]));
        assert_eq!(
            grandchild_result["branch"]["path"],
            json!(["thread-1", "thread-2", "thread-4"])
        );
        assert_eq!(grandchild_result["branch"]["depth"], json!(2));
    }

    #[test]
    fn forks_thread_from_item_over_json_rpc_and_reports_branch_point() {
        let mut runtime = Runtime::new();
        let start = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(1),
                method: "thread/start".to_string(),
                params: json!({ "project": { "id": "project", "cwd": "/repo" } }),
            },
        );
        assert!(start.error.is_none(), "{:?}", start.error);
        let turn = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(2),
                method: "turn/start".to_string(),
                params: json!({
                    "threadId": "thread-1",
                    "input": "inspect",
                    "assistantResponse": "done"
                }),
            },
        );
        assert!(turn.error.is_none(), "{:?}", turn.error);
        let turn_result = turn.result.unwrap();
        let assistant_event_id = turn_result["events"]
            .as_array()
            .unwrap()
            .iter()
            .find_map(|event| {
                (event["kind"]["type"] == json!("itemCompleted")
                    && event["kind"]["item"]["kind"]["type"] == json!("assistantMessage"))
                .then(|| event["id"].as_u64().unwrap())
            })
            .expect("assistant item event");

        let fork = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(3),
                method: "thread/item-fork".to_string(),
                params: json!({
                    "threadId": "thread-1",
                    "eventId": assistant_event_id,
                    "title": "rewrite from inspect"
                }),
            },
        );
        assert!(fork.error.is_none(), "{:?}", fork.error);
        let fork_result = fork.result.unwrap();
        assert_eq!(fork_result["thread"]["id"], json!("thread-2"));
        assert_eq!(fork_result["thread"]["forkedFrom"], json!("thread-1"));
        assert!(
            fork_result["events"]
                .as_array()
                .unwrap()
                .iter()
                .any(|event| {
                    event["kind"]["type"] == json!("threadForkPointSet")
                        && event["kind"]["sourceEventId"] == json!(assistant_event_id)
                })
        );

        let items = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(4),
                method: "thread/items".to_string(),
                params: json!({ "threadId": "thread-2" }),
            },
        );
        assert!(items.error.is_none(), "{:?}", items.error);
        let items_result = items.result.unwrap();
        assert_eq!(items_result["branch"]["parentThreadId"], json!("thread-1"));
        assert_eq!(
            items_result["branch"]["forkEventId"],
            json!(assistant_event_id)
        );
        assert_eq!(
            items_result["branch"]["path"],
            json!(["thread-1", "thread-2"])
        );
    }

    #[test]
    fn edit_forks_user_item_over_json_rpc_with_history_cutoff() {
        let mut runtime = Runtime::new();
        let start = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(1),
                method: "thread/start".to_string(),
                params: json!({ "project": { "id": "project", "cwd": "/repo" } }),
            },
        );
        assert!(start.error.is_none(), "{:?}", start.error);
        let setup = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(2),
                method: "turn/start".to_string(),
                params: json!({
                    "threadId": "thread-1",
                    "input": "setup question",
                    "assistantResponse": "setup answer"
                }),
            },
        );
        assert!(setup.error.is_none(), "{:?}", setup.error);
        let old = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(3),
                method: "turn/start".to_string(),
                params: json!({
                    "threadId": "thread-1",
                    "input": "old request",
                    "assistantResponse": "old answer"
                }),
            },
        );
        assert!(old.error.is_none(), "{:?}", old.error);
        let old_result = old.result.unwrap();
        let old_user_event_id = old_result["events"]
            .as_array()
            .unwrap()
            .iter()
            .find_map(|event| {
                let kind = &event["kind"];
                (kind["type"] == json!("itemCompleted")
                    && kind["item"]["kind"]["type"] == json!("userMessage"))
                .then(|| event["id"].as_u64())
                .flatten()
            })
            .expect("old user event id");
        let old_assistant_event_id = old_result["events"]
            .as_array()
            .unwrap()
            .iter()
            .find_map(|event| {
                let kind = &event["kind"];
                (kind["type"] == json!("itemCompleted")
                    && kind["item"]["kind"]["type"] == json!("assistantMessage"))
                .then(|| event["id"].as_u64())
                .flatten()
            })
            .expect("old assistant event id");

        let edited = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(4),
                method: "thread/item-edit-fork".to_string(),
                params: json!({
                    "threadId": "thread-1",
                    "eventId": old_user_event_id,
                    "input": "edited request",
                    "title": "edit old request"
                }),
            },
        );
        assert!(edited.error.is_none(), "{:?}", edited.error);
        let edited_result = edited.result.unwrap();
        assert_eq!(edited_result["thread"]["id"], json!("thread-2"));
        assert_eq!(edited_result["input"], json!("edited request"));
        assert_eq!(edited_result["sourceEventId"], json!(old_user_event_id));
        assert!(edited_result["historyCutoffEventId"].as_u64().unwrap() < old_user_event_id);
        assert!(
            edited_result["events"]
                .as_array()
                .unwrap()
                .iter()
                .any(|event| {
                    event["kind"]["type"] == json!("threadForkPointSet")
                        && event["kind"]["sourceEventId"] == json!(old_user_event_id)
                        && event["kind"]["historyCutoffEventId"].as_u64().unwrap()
                            < old_user_event_id
                })
        );

        let assistant_rejected = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(5),
                method: "thread/item-edit-fork".to_string(),
                params: json!({
                    "threadId": "thread-1",
                    "eventId": old_assistant_event_id,
                    "input": "edited request"
                }),
            },
        );
        assert!(assistant_rejected.error.is_some());
        assert_eq!(
            assistant_rejected.error.unwrap().data.unwrap()["code"],
            json!("thread_item_not_user_message")
        );
    }

    #[test]
    fn lists_skills_over_json_rpc() {
        let mut runtime = Runtime::new();
        let start = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(1),
                method: "thread/start".to_string(),
                params: json!({ "project": { "id": "project", "cwd": "/repo" } }),
            },
        );
        assert!(start.error.is_none());
        let response = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(2),
                method: "skills/list".to_string(),
                params: json!({ "threadId": "thread-1" }),
            },
        );
        assert!(response.error.is_none());
        let result = response.result.unwrap();
        assert!(result["items"].as_array().unwrap().iter().any(|item| {
            item["active"]["name"] == json!("imagegen")
                && item["active"]["source"] == json!("builtIn")
        }));
    }

    #[test]
    fn expands_skill_command_over_json_rpc() {
        let mut runtime = Runtime::new();
        let start = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(1),
                method: "thread/start".to_string(),
                params: json!({ "project": { "id": "project", "cwd": "/repo" } }),
            },
        );
        assert!(start.error.is_none());
        let response = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(2),
                method: "skills/expand".to_string(),
                params: json!({
                    "threadId": "thread-1",
                    "input": "/skill:imagegen draw a launch badge"
                }),
            },
        );
        assert!(response.error.is_none(), "{:?}", response.error);
        let result = response.result.unwrap();
        assert_eq!(result["skill"]["name"], json!("imagegen"));
        assert!(
            result["input"]
                .as_str()
                .unwrap()
                .contains("<skill name=\"imagegen\"")
        );
        assert!(
            result["input"]
                .as_str()
                .unwrap()
                .ends_with("draw a launch badge")
        );
    }

    #[test]
    fn lists_plugin_prompt_templates_over_json_rpc() {
        let root = temp_store_root("oppi-server-prompt-template-list");
        let _ = std::fs::remove_dir_all(&root);
        write_prompt_theme_plugin(&root);
        let mut runtime = Runtime::new();
        start_thread_for_root(&mut runtime, &root);

        let response = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(2),
                method: "prompt-template/list".to_string(),
                params: json!({ "threadId": "thread-1" }),
            },
        );
        assert!(response.error.is_none(), "{:?}", response.error);
        let result = response.result.unwrap();
        assert!(result["items"].as_array().unwrap().iter().any(|item| {
            item["active"]["name"] == json!("component")
                && item["active"]["source"] == json!("plugin")
                && item["active"]["argumentHint"] == json!("<name>")
        }));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn expands_plugin_prompt_template_over_json_rpc() {
        let root = temp_store_root("oppi-server-prompt-template-expand");
        let _ = std::fs::remove_dir_all(&root);
        write_prompt_theme_plugin(&root);
        let mut runtime = Runtime::new();
        start_thread_for_root(&mut runtime, &root);

        let expanded = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(2),
                method: "prompt-template/expand".to_string(),
                params: json!({
                    "threadId": "thread-1",
                    "name": "component",
                    "args": "Button"
                }),
            },
        );
        assert!(expanded.error.is_none(), "{:?}", expanded.error);
        let result = expanded.result.unwrap();
        assert_eq!(result["name"], json!("component"));
        assert!(
            result["input"]
                .as_str()
                .unwrap_or_default()
                .contains("Create component Button.")
        );

        let prepared = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(3),
                method: "command/prepare".to_string(),
                params: json!({
                    "threadId": "thread-1",
                    "command": "component",
                    "args": "Button",
                    "context": {}
                }),
            },
        );
        assert!(prepared.error.is_none(), "{:?}", prepared.error);
        let result = prepared.result.unwrap();
        assert_eq!(result["command"], json!("component"));
        assert!(
            result["input"]
                .as_str()
                .unwrap_or_default()
                .contains("Create component Button.")
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn lists_plugin_themes_over_json_rpc() {
        let root = temp_store_root("oppi-server-theme-list");
        let _ = std::fs::remove_dir_all(&root);
        write_prompt_theme_plugin(&root);
        let mut runtime = Runtime::new();
        start_thread_for_root(&mut runtime, &root);

        let response = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(2),
                method: "theme/list".to_string(),
                params: json!({ "threadId": "thread-1" }),
            },
        );
        assert!(response.error.is_none(), "{:?}", response.error);
        let result = response.result.unwrap();
        assert!(result["items"].as_array().unwrap().iter().any(|item| {
            item["active"]["name"] == json!("acme") && item["active"]["source"] == json!("plugin")
        }));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn loads_plugin_theme_over_json_rpc() {
        let root = temp_store_root("oppi-server-theme-load");
        let _ = std::fs::remove_dir_all(&root);
        write_prompt_theme_plugin(&root);
        let mut runtime = Runtime::new();
        start_thread_for_root(&mut runtime, &root);

        let response = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(2),
                method: "theme/load".to_string(),
                params: json!({ "threadId": "thread-1", "name": "acme" }),
            },
        );
        assert!(response.error.is_none(), "{:?}", response.error);
        let result = response.result.unwrap();
        assert_eq!(result["theme"]["name"], json!("acme"));
        assert_eq!(result["colors"]["accent"], json!("#123abc"));
        assert_eq!(result["colors"]["muted"], json!(242));
        assert_eq!(result["colors"]["text"], json!(""));
        assert_eq!(result["export"]["pageBg"], json!("#010203"));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn lists_configured_plugin_resources_over_json_rpc() {
        let root =
            std::env::temp_dir().join(format!("oppi-server-plugin-list-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let plugin = root.join("plugin-pack");
        std::fs::create_dir_all(plugin.join("skills/plugin-scout")).unwrap();
        std::fs::create_dir_all(plugin.join("extensions")).unwrap();
        std::fs::create_dir_all(root.join(".oppi")).unwrap();
        std::fs::write(
            plugin.join("package.json"),
            r#"{"name":"plugin-pack","version":"1.0.0","pi":{"skills":["./skills"],"extensions":["./extensions/index.ts"]}}"#,
        )
        .unwrap();
        std::fs::write(
            plugin.join("skills/plugin-scout/SKILL.md"),
            "---\nname: plugin-scout\ndescription: Inspect plugin resources.\n---\n\n# Plugin Scout\n",
        )
        .unwrap();
        std::fs::write(plugin.join("extensions/index.ts"), "export default {};\n").unwrap();
        std::fs::write(
            root.join(".oppi/plugins.json"),
            format!(
                r#"{{
  "version": 1,
  "plugins": [
    {{
      "name": "plugin-pack",
      "source": {},
      "sourceType": "local",
      "enabled": true,
      "trusted": true
    }}
  ]
}}"#,
                serde_json::to_string(&plugin.display().to_string()).unwrap()
            ),
        )
        .unwrap();

        let mut runtime = Runtime::new();
        let start = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(1),
                method: "thread/start".to_string(),
                params: json!({ "project": { "id": "project", "cwd": root.display().to_string() } }),
            },
        );
        assert!(start.error.is_none(), "{:?}", start.error);

        let response = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(2),
                method: "plugin/list".to_string(),
                params: json!({ "threadId": "thread-1" }),
            },
        );
        assert!(response.error.is_none(), "{:?}", response.error);
        let result = response.result.unwrap();
        let items = result["items"].as_array().unwrap();
        let plugin = items
            .iter()
            .find(|item| item["id"] == json!("plugin-pack"))
            .expect("configured plugin should be listed");
        assert_eq!(plugin["scope"], json!("project"));
        assert_eq!(plugin["trusted"], json!(true));
        assert!(
            plugin["resources"]
                .as_array()
                .unwrap()
                .iter()
                .any(|resource| {
                    resource["kind"] == json!("skill")
                        && resource["status"] == json!("loaded")
                        && resource["path"]
                            .as_str()
                            .unwrap_or_default()
                            .replace('\\', "/")
                            .ends_with("skills/plugin-scout/SKILL.md")
                })
        );
        assert!(
            plugin["resources"]
                .as_array()
                .unwrap()
                .iter()
                .any(|resource| {
                    resource["kind"] == json!("extension")
                        && resource["status"] == json!("unsupported")
                        && resource["path"]
                            .as_str()
                            .unwrap_or_default()
                            .replace('\\', "/")
                            .ends_with("extensions/index.ts")
                })
        );
        assert!(
            plugin["diagnostics"]
                .as_array()
                .unwrap()
                .iter()
                .any(|diagnostic| {
                    diagnostic["code"] == json!("pi_extension_unsupported")
                        && diagnostic["message"]
                            .as_str()
                            .unwrap_or_default()
                            .contains("not executed by Rust native")
                })
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn lists_marketplace_plugin_diagnostics_over_json_rpc() {
        let root = std::env::temp_dir().join(format!(
            "oppi-server-marketplace-plugin-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".oppi")).unwrap();
        std::fs::write(
            root.join(".oppi/plugins.json"),
            r#"{
  "version": 1,
  "plugins": [
    {
      "name": "market-plugin",
      "source": "marketplace:danger-plugin",
      "sourceType": "marketplace",
      "enabled": true,
      "trusted": true,
      "marketplace": "demo-market",
      "capabilities": ["shell"]
    }
  ]
}"#,
        )
        .unwrap();

        let mut runtime = Runtime::new();
        let start = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(1),
                method: "thread/start".to_string(),
                params: json!({ "project": { "id": "project", "cwd": root.display().to_string() } }),
            },
        );
        assert!(start.error.is_none(), "{:?}", start.error);

        let response = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(2),
                method: "plugin/list".to_string(),
                params: json!({ "threadId": "thread-1" }),
            },
        );
        assert!(response.error.is_none(), "{:?}", response.error);
        let result = response.result.unwrap();
        let items = result["items"].as_array().unwrap();
        let plugin = items
            .iter()
            .find(|item| item["id"] == json!("market-plugin"))
            .expect("configured marketplace plugin should be listed");
        let diagnostics = plugin["diagnostics"].as_array().unwrap();
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic["code"] == json!("marketplace_plugin_not_materialized")
        }));
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| { diagnostic["code"] == json!("plugin_risk_shell_exec") })
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn plugin_materialize_route_fails_closed_for_runtime_mutation() {
        let root = std::env::temp_dir().join(format!(
            "oppi-server-plugin-materialize-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let plugin = root.join("plugin-pack");
        std::fs::create_dir_all(&plugin).unwrap();
        std::fs::create_dir_all(root.join(".oppi")).unwrap();
        std::fs::write(
            plugin.join("package.json"),
            r#"{"name":"plugin-pack","version":"1.0.0","pi":{"skills":["./skills"]}}"#,
        )
        .unwrap();
        std::fs::write(
            root.join(".oppi/plugins.json"),
            format!(
                r#"{{
  "version": 1,
  "plugins": [
    {{
      "name": "plugin-pack",
      "source": {},
      "sourceType": "local",
      "enabled": true,
      "trusted": true
    }}
  ]
}}"#,
                serde_json::to_string(&plugin.display().to_string()).unwrap()
            ),
        )
        .unwrap();

        let mut runtime = Runtime::new();
        let start = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(1),
                method: "thread/start".to_string(),
                params: json!({ "project": { "id": "project", "cwd": root.display().to_string() } }),
            },
        );
        assert!(start.error.is_none(), "{:?}", start.error);

        let response = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(2),
                method: "plugin/materialize".to_string(),
                params: json!({
                    "threadId": "thread-1",
                    "pluginId": "plugin-pack",
                    "scope": "project",
                    "yes": true
                }),
            },
        );
        let error = response.error.expect("runtime route should fail closed");
        assert_eq!(
            error.data.unwrap()["code"],
            json!("method_not_available_from_runtime")
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn lists_thread_scoped_mcp_config_over_json_rpc() {
        let root =
            std::env::temp_dir().join(format!("oppi-server-mcp-config-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join(".mcp.json"),
            r#"{
  "mcpServers": {
    "fs": {
      "command": "node",
      "args": ["server.js"]
    }
  }
}"#,
        )
        .unwrap();

        let mut runtime = Runtime::new();
        let start = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(1),
                method: "thread/start".to_string(),
                params: json!({ "project": { "id": "project", "cwd": root.display().to_string() } }),
            },
        );
        assert!(start.error.is_none(), "{:?}", start.error);

        let response = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(2),
                method: "mcp/list".to_string(),
                params: json!({ "threadId": "thread-1" }),
            },
        );
        assert!(response.error.is_none(), "{:?}", response.error);
        let result = response.result.unwrap();
        let items = result["items"].as_array().unwrap();
        let server = items
            .iter()
            .find(|item| item["id"] == json!("fs"))
            .expect("project MCP server should be listed");
        assert_eq!(server["source"], json!("project"));
        assert_eq!(server["status"], json!("disabled"));
        assert_eq!(server["transport"]["kind"], json!("stdio"));
        assert!(
            server["diagnostics"]
                .as_array()
                .unwrap()
                .iter()
                .any(|diagnostic| diagnostic["code"] == json!("mcp_requires_trust"))
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn trusts_plugin_mcp_server_over_json_rpc() {
        let root = temp_store_root("jsonrpc-plugin-mcp-trust");
        let plugin = root.join("plugin-pack");
        std::fs::create_dir_all(root.join(".oppi")).unwrap();
        std::fs::create_dir_all(&plugin).unwrap();
        std::fs::write(
            plugin.join("package.json"),
            r#"{
  "name": "plugin-pack",
  "version": "1.0.0",
  "mcpServers": {
    "demo": {
      "command": "node",
      "args": ["server.js"]
    }
  }
}"#,
        )
        .unwrap();
        std::fs::write(
            root.join(".oppi/plugins.json"),
            format!(
                r#"{{
  "version": 1,
  "plugins": [
    {{
      "name": "plugin-pack",
      "source": {},
      "sourceType": "local",
      "enabled": true,
      "trusted": true
    }}
  ]
}}"#,
                serde_json::to_string(&plugin.display().to_string()).unwrap()
            ),
        )
        .unwrap();

        let mut runtime = Runtime::new();
        let start = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(1),
                method: "thread/start".to_string(),
                params: json!({ "project": { "id": "project", "cwd": root.display().to_string() } }),
            },
        );
        assert!(start.error.is_none(), "{:?}", start.error);

        let response = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(2),
                method: "plugin/mcp/trust".to_string(),
                params: json!({
                    "threadId": "thread-1",
                    "pluginId": "plugin-pack",
                    "serverId": "demo",
                    "enabled": true,
                    "trusted": true
                }),
            },
        );
        assert!(response.error.is_none(), "{:?}", response.error);
        let result = response.result.unwrap();
        let server = result["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["id"] == json!("demo"))
            .expect("trusted plugin MCP server should be returned");
        assert_eq!(server["status"], json!("disconnected"));
        assert_eq!(server["trusted"], json!(true));
        assert!(
            !server
                .get("diagnostics")
                .and_then(Value::as_array)
                .map(|diagnostics| diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic["code"] == json!("mcp_plugin_metadata_only")))
                .unwrap_or(false)
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    fn node_mcp_test_server(root: &std::path::Path) -> Option<PathBuf> {
        if std::process::Command::new("node")
            .arg("--version")
            .output()
            .is_err()
        {
            eprintln!("skipping stdio MCP JSON-RPC test because node is unavailable");
            return None;
        }
        let script = root.join("mcp-server.js");
        std::fs::write(
            &script,
            r#"
const readline = require('readline');
const rl = readline.createInterface({ input: process.stdin, crlfDelay: Infinity });
let initialized = false;
function send(id, result) {
  process.stdout.write(JSON.stringify({ jsonrpc: '2.0', id, result }) + '\n');
}
function notifyResourceUpdated(uri) {
  process.stdout.write(JSON.stringify({
    jsonrpc: '2.0',
    method: 'notifications/resources/updated',
    params: { uri }
  }) + '\n');
}
rl.on('line', (line) => {
  const msg = JSON.parse(line);
  if (msg.method === 'notifications/initialized') return;
    if (msg.method === 'initialize') {
    initialized = true;
    send(msg.id, {
      protocolVersion: '2025-11-25',
      serverInfo: { name: 'node-test-mcp', version: '1.0.0' },
      capabilities: { tools: {}, resources: { subscribe: true } }
    });
  } else if (!initialized) {
    send(msg.id, { isError: true, content: [{ type: 'text', text: `${msg.method} before initialize` }] });
  } else if (msg.method === 'tools/list') {
    send(msg.id, { tools: [{
      name: 'read_file',
      description: 'Read a file',
      inputSchema: {
        type: 'object',
        properties: { path: { type: 'string' } },
        required: ['path']
      },
      annotations: { readOnlyHint: true }
    }]});
  } else if (msg.method === 'resources/list') {
    send(msg.id, {
      resources: [{
        uri: 'file:///repo/README.md',
        name: 'readme',
        title: 'Project README',
        description: 'Human-facing project overview',
        mimeType: 'text/markdown',
        size: 2048
      }],
      nextCursor: 'page-2'
    });
  } else if (msg.method === 'resources/templates/list') {
    send(msg.id, {
      resourceTemplates: [{
        uriTemplate: 'file:///repo/{path}',
        name: 'repo-file',
        title: 'Repository file',
        description: 'Read a repository file by path',
        mimeType: 'text/plain'
      }],
      nextCursor: 'template-page-2'
    });
  } else if (msg.method === 'prompts/list') {
    send(msg.id, {
      prompts: [{
        name: 'review-change',
        title: 'Review change',
        description: 'Review a diff with a specific focus',
        arguments: [{
          name: 'focus',
          title: 'Focus',
          description: 'Review focus',
          required: true
        }]
      }],
      nextCursor: 'prompt-page-2'
    });
  } else if (msg.method === 'prompts/get') {
    send(msg.id, {
      description: 'Review prompt',
      messages: [{
        role: 'user',
        content: {
          type: 'text',
          text: `Review this change for ${msg.params.arguments.focus}.`
        }
      }]
    });
  } else if (msg.method === 'resources/read') {
    if (msg.params.uri === 'file:///repo/notify.md') {
      notifyResourceUpdated('file:///repo/README.md');
    }
    send(msg.id, {
      contents: [{
        uri: msg.params.uri,
        mimeType: 'text/markdown',
        text: `# README\nRead ${msg.params.uri}`
      }]
    });
  } else if (msg.method === 'resources/subscribe') {
    send(msg.id, {});
  } else if (msg.method === 'resources/unsubscribe') {
    send(msg.id, {});
  } else if (msg.method === 'tools/call') {
    send(msg.id, { content: [{ type: 'text', text: `MCP read ${msg.params.arguments.path}` }] });
  } else {
    send(msg.id, { isError: true, content: [{ type: 'text', text: `unknown ${msg.method}` }] });
  }
});
"#,
        )
        .unwrap();
        Some(script)
    }

    #[test]
    fn mcp_connect_missing_stdio_command_returns_visible_diagnostic_over_json_rpc() {
        let root = temp_store_root("oppi-server-mcp-spawn-fail");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let missing_command = format!("oppi-missing-mcp-command-{}", std::process::id());
        let mut runtime = Runtime::new();
        let start = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(1),
                method: "thread/start".to_string(),
                params: json!({ "project": { "id": "project", "cwd": root.display().to_string() } }),
            },
        );
        assert!(start.error.is_none(), "{:?}", start.error);

        let register = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(2),
                method: "mcp/register".to_string(),
                params: json!({
                    "threadId": "thread-1",
                    "server": {
                        "id": "cli-missing",
                        "name": "Missing CLI MCP",
                        "status": "enabled",
                        "source": "cli",
                        "transport": {
                            "kind": "stdio",
                            "command": missing_command
                        },
                        "trusted": true
                    }
                }),
            },
        );
        assert!(register.error.is_none(), "{:?}", register.error);

        let connected = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(3),
                method: "mcp/connect".to_string(),
                params: json!({ "threadId": "thread-1", "serverId": "cli-missing" }),
            },
        );
        assert!(connected.error.is_none(), "{:?}", connected.error);
        let events = connected.result.as_ref().unwrap()["events"]
            .as_array()
            .unwrap();
        let server = events
            .iter()
            .find_map(|event| {
                (event["kind"]["type"] == json!("mcpServerRegistered")
                    && event["kind"]["server"]["id"] == json!("cli-missing"))
                .then(|| &event["kind"]["server"])
            })
            .expect("failed MCP server event");
        assert_eq!(server["status"], json!("failed"));
        assert_eq!(server["source"], json!("cli"));
        assert!(server["diagnostics"].as_array().unwrap().iter().any(
            |diagnostic| diagnostic["code"] == json!("mcp_stdio_spawn_failed")
                && diagnostic["severity"] == json!("error")
        ));
        assert!(events.iter().any(|event| {
            event["kind"]["type"] == json!("diagnostic")
                && event["kind"]["diagnostic"]["metadata"]["component"] == json!("mcp")
                && event["kind"]["diagnostic"]["metadata"]["code"]
                    == json!("mcp_stdio_spawn_failed")
                && event["kind"]["diagnostic"]["metadata"]["serverId"] == json!("cli-missing")
        }));

        let listed = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(4),
                method: "mcp/list".to_string(),
                params: json!({ "threadId": "thread-1" }),
            },
        );
        assert!(listed.error.is_none(), "{:?}", listed.error);
        let result = listed.result.unwrap();
        let listed_server = result["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|server| server["id"] == json!("cli-missing"))
            .expect("listed failed MCP server");
        assert_eq!(listed_server["status"], json!("failed"));
        assert!(
            listed_server["diagnostics"]
                .as_array()
                .unwrap()
                .iter()
                .any(|diagnostic| diagnostic["code"] == json!("mcp_stdio_spawn_failed"))
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn mcp_connect_and_call_tool_over_json_rpc() {
        let root = temp_store_root("oppi-server-mcp-connect-call");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let Some(script) = node_mcp_test_server(&root) else {
            return;
        };
        let mut runtime = Runtime::new();
        let start = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(1),
                method: "thread/start".to_string(),
                params: json!({ "project": { "id": "project", "cwd": root.display().to_string() } }),
            },
        );
        assert!(start.error.is_none(), "{:?}", start.error);

        let register = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(2),
                method: "mcp/register".to_string(),
                params: json!({
                    "threadId": "thread-1",
                    "server": {
                        "id": "fs",
                        "name": "filesystem",
                        "status": "enabled",
                        "source": "runtime",
                        "transport": {
                            "kind": "stdio",
                            "command": "node",
                            "args": [script.display().to_string()]
                        },
                        "trusted": true
                    }
                }),
            },
        );
        assert!(register.error.is_none(), "{:?}", register.error);

        let connected = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(3),
                method: "mcp/connect".to_string(),
                params: json!({ "threadId": "thread-1", "serverId": "fs" }),
            },
        );
        assert!(connected.error.is_none(), "{:?}", connected.error);
        let events = connected.result.as_ref().unwrap()["events"]
            .as_array()
            .unwrap();
        assert!(events.iter().any(|event| {
            event["kind"]["type"] == json!("mcpServerRegistered")
                && event["kind"]["server"]["toolRefs"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|tool| tool["name"] == json!("read_file"))
        }));

        let called = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(4),
                method: "mcp/call-tool".to_string(),
                params: json!({
                    "threadId": "thread-1",
                    "serverId": "fs",
                    "toolName": "read_file",
                    "arguments": { "path": "README.md" }
                }),
            },
        );
        assert!(called.error.is_none(), "{:?}", called.error);
        let result = called.result.unwrap();
        assert_eq!(result["serverId"], json!("fs"));
        assert_eq!(result["toolName"], json!("read_file"));
        assert_eq!(result["status"], json!("ok"));
        assert!(
            result["content"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item["text"] == json!("MCP read README.md"))
        );

        let resources = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(5),
                method: "mcp/resources/list".to_string(),
                params: json!({
                    "threadId": "thread-1",
                    "serverId": "fs"
                }),
            },
        );
        assert!(resources.error.is_none(), "{:?}", resources.error);
        let result = resources.result.unwrap();
        assert_eq!(result["serverId"], json!("fs"));
        assert_eq!(result["status"], json!("ok"));
        assert_eq!(result["nextCursor"], json!("page-2"));
        assert_eq!(
            result["resources"][0]["uri"],
            json!("file:///repo/README.md")
        );
        assert_eq!(result["resources"][0]["mimeType"], json!("text/markdown"));

        let templates = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(6),
                method: "mcp/resources/templates/list".to_string(),
                params: json!({
                    "threadId": "thread-1",
                    "serverId": "fs"
                }),
            },
        );
        assert!(templates.error.is_none(), "{:?}", templates.error);
        let result = templates.result.expect("templates result");
        assert_eq!(result["serverId"], json!("fs"));
        assert_eq!(result["status"], json!("ok"));
        assert_eq!(
            result["resourceTemplates"][0]["uriTemplate"],
            json!("file:///repo/{path}")
        );
        assert_eq!(result["nextCursor"], json!("template-page-2"));

        let prompts = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(7),
                method: "mcp/prompts/list".to_string(),
                params: json!({
                    "threadId": "thread-1",
                    "serverId": "fs"
                }),
            },
        );
        assert!(prompts.error.is_none(), "{:?}", prompts.error);
        let result = prompts.result.expect("prompts result");
        assert_eq!(result["serverId"], json!("fs"));
        assert_eq!(result["status"], json!("ok"));
        assert_eq!(result["prompts"][0]["name"], json!("review-change"));
        assert_eq!(result["nextCursor"], json!("prompt-page-2"));

        let prompt = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(8),
                method: "mcp/prompts/get".to_string(),
                params: json!({
                    "threadId": "thread-1",
                    "serverId": "fs",
                    "name": "review-change",
                    "arguments": { "focus": "security" }
                }),
            },
        );
        assert!(prompt.error.is_none(), "{:?}", prompt.error);
        let result = prompt.result.expect("prompt result");
        assert_eq!(result["serverId"], json!("fs"));
        assert_eq!(result["status"], json!("ok"));
        assert_eq!(result["name"], json!("review-change"));
        assert_eq!(
            result["messages"][0]["content"]["text"],
            json!("Review this change for security.")
        );

        let resource_read = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(9),
                method: "mcp/resources/read".to_string(),
                params: json!({
                    "threadId": "thread-1",
                    "serverId": "fs",
                    "uri": "file:///repo/README.md"
                }),
            },
        );
        assert!(resource_read.error.is_none(), "{:?}", resource_read.error);
        let result = resource_read.result.unwrap();
        assert_eq!(result["serverId"], json!("fs"));
        assert_eq!(result["uri"], json!("file:///repo/README.md"));
        assert_eq!(result["status"], json!("ok"));
        assert_eq!(result["contents"][0]["mimeType"], json!("text/markdown"));
        assert_eq!(
            result["contents"][0]["text"],
            json!("# README\nRead file:///repo/README.md")
        );

        let subscribed = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(10),
                method: "mcp/resources/subscribe".to_string(),
                params: json!({
                    "threadId": "thread-1",
                    "serverId": "fs",
                    "uri": "file:///repo/README.md"
                }),
            },
        );
        assert!(subscribed.error.is_none(), "{:?}", subscribed.error);
        let result = subscribed.result.unwrap();
        assert_eq!(result["serverId"], json!("fs"));
        assert_eq!(result["uri"], json!("file:///repo/README.md"));
        assert_eq!(result["status"], json!("ok"));
        assert_eq!(result["subscribed"], json!(true));

        let updated = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(11),
                method: "mcp/resources/updated".to_string(),
                params: json!({
                    "threadId": "thread-1",
                    "serverId": "fs",
                    "uri": "file:///repo/README.md",
                    "receivedAt": "2026-05-31T12:00:00Z"
                }),
            },
        );
        assert!(updated.error.is_none(), "{:?}", updated.error);
        let result = updated.result.unwrap();
        assert_eq!(result["serverId"], json!("fs"));
        assert_eq!(result["uri"], json!("file:///repo/README.md"));
        assert_eq!(result["status"], json!("ok"));
        assert_eq!(result["updated"], json!(true));
        assert_eq!(result["updateCount"], json!(1));

        let notify_read = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(12),
                method: "mcp/resources/read".to_string(),
                params: json!({
                    "threadId": "thread-1",
                    "serverId": "fs",
                    "uri": "file:///repo/notify.md"
                }),
            },
        );
        assert!(notify_read.error.is_none(), "{:?}", notify_read.error);
        let result = notify_read.result.unwrap();
        assert_eq!(result["serverId"], json!("fs"));
        assert_eq!(result["uri"], json!("file:///repo/notify.md"));
        assert_eq!(result["status"], json!("ok"));

        let listed = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(13),
                method: "mcp/list".to_string(),
                params: json!({ "threadId": "thread-1" }),
            },
        );
        assert!(listed.error.is_none(), "{:?}", listed.error);
        let result = listed.result.unwrap();
        let fs = result["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|server| server["id"] == json!("fs"))
            .expect("fs server");
        assert_eq!(
            fs["resourceSubscriptions"][0]["uri"],
            json!("file:///repo/README.md")
        );
        assert_eq!(fs["resourceSubscriptions"][0]["updateCount"], json!(2));
        assert!(fs["resourceSubscriptions"][0]["lastUpdatedAt"].is_string());

        let unsubscribed = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(14),
                method: "mcp/resources/unsubscribe".to_string(),
                params: json!({
                    "threadId": "thread-1",
                    "serverId": "fs",
                    "uri": "file:///repo/README.md"
                }),
            },
        );
        assert!(unsubscribed.error.is_none(), "{:?}", unsubscribed.error);
        let result = unsubscribed.result.unwrap();
        assert_eq!(result["serverId"], json!("fs"));
        assert_eq!(result["uri"], json!("file:///repo/README.md"));
        assert_eq!(result["status"], json!("ok"));
        assert_eq!(result["subscribed"], json!(false));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn dispatches_agent_over_json_rpc() {
        let mut runtime = Runtime::new();
        let start = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(1),
                method: "thread/start".to_string(),
                params: json!({ "project": { "id": "project", "cwd": "/repo" } }),
            },
        );
        assert!(start.error.is_none());
        let register = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(2),
                method: "agents/register".to_string(),
                params: json!({
                    "threadId": "thread-1",
                    "agent": {
                        "name": "reviewer",
                        "description": "review code",
                        "instructions": "Review carefully.",
                        "background": true,
                        "worktreeRoot": "/repo-wt"
                    }
                }),
            },
        );
        assert!(register.error.is_none());
        let dispatch = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(3),
                method: "agents/dispatch".to_string(),
                params: json!({ "threadId": "thread-1", "agentName": "reviewer", "task": "audit", "role": "subagent", "model": "gpt-sub", "effort": "high" }),
            },
        );
        assert!(dispatch.error.is_none());
        let result = dispatch.result.unwrap();
        assert_eq!(result["run"]["worktreeRoot"], json!("/repo-wt"));
        assert_eq!(result["run"]["role"], json!("subagent"));
        assert_eq!(result["run"]["model"], json!("gpt-sub"));
        assert_eq!(result["run"]["effort"], json!("high"));
    }

    #[test]
    fn background_agent_poll_route_drains_pending_agent_run() {
        let mut runtime = Runtime::new();
        let start = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(1),
                method: "thread/start".to_string(),
                params: json!({ "project": { "id": "project", "cwd": "/repo" } }),
            },
        );
        assert!(start.error.is_none(), "{:?}", start.error);

        let turn = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(2),
                method: "turn/run-agentic".to_string(),
                params: json!({
                    "threadId": "thread-1",
                    "input": "delegate background agent",
                    "modelSteps": [{
                        "toolCalls": [{
                            "id": "agent-bg",
                            "name": "AgentTool",
                            "namespace": "oppi",
                            "arguments": {
                                "agentName": "general-purpose",
                                "task": "finish later",
                                "background": true,
                                "modelSteps": [{
                                    "assistantDeltas": ["background child done"],
                                    "finalResponse": true
                                }]
                            }
                        }],
                        "finalResponse": true
                    }]
                }),
            },
        );
        assert!(turn.error.is_none(), "{:?}", turn.error);
        let turn_result = turn.result.unwrap();
        let agent_output = turn_result["events"]
            .as_array()
            .unwrap()
            .iter()
            .find_map(|event| {
                let kind = &event["kind"];
                (kind["type"] == json!("toolCallCompleted")
                    && kind["result"]["callId"] == json!("agent-bg"))
                .then(|| kind["result"]["output"].as_str())
                .flatten()
            })
            .expect("AgentTool output should be present");
        let agent_output: Value = serde_json::from_str(agent_output).unwrap();
        assert_eq!(agent_output["status"], json!("running"));
        let run_id = agent_output["runId"].as_str().unwrap();

        let poll = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(3),
                method: "agents/poll".to_string(),
                params: json!({ "threadId": "thread-1", "runId": run_id }),
            },
        );
        assert!(poll.error.is_none(), "{:?}", poll.error);
        let result = poll.result.unwrap();
        assert_eq!(result["run"]["status"], json!("completed"));
        assert_eq!(result["output"], json!("background child done"));
        assert!(result["run"]["childTurnId"].as_str().is_some());
        assert!(result["events"].as_array().unwrap().iter().any(|event| {
            event["kind"]["type"] == json!("agentCompleted")
                && event["kind"]["runId"] == json!(run_id)
        }));
    }

    #[test]
    fn background_agent_resume_route_completes_blocked_child_run() {
        let mut runtime = Runtime::new();
        let start = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(1),
                method: "thread/start".to_string(),
                params: json!({ "project": { "id": "project", "cwd": "/repo" } }),
            },
        );
        assert!(start.error.is_none(), "{:?}", start.error);

        let turn = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(2),
                method: "turn/run-agentic".to_string(),
                params: json!({
                    "threadId": "thread-1",
                    "input": "delegate background approval",
                    "modelSteps": [{
                        "toolCalls": [{
                            "id": "agent-bg",
                            "name": "AgentTool",
                            "namespace": "oppi",
                            "arguments": {
                                "agentName": "general-purpose",
                                "task": "wait for approval",
                                "background": true,
                                "modelSteps": [{
                                    "toolCalls": [{
                                        "id": "echo-approval",
                                        "name": "echo",
                                        "namespace": "oppi",
                                        "arguments": {
                                            "output": "approved child output",
                                            "requireApproval": true
                                        }
                                    }],
                                    "finalResponse": false
                                }]
                            }
                        }],
                        "finalResponse": true
                    }]
                }),
            },
        );
        assert!(turn.error.is_none(), "{:?}", turn.error);
        let turn_result = turn.result.unwrap();
        let agent_output = turn_result["events"]
            .as_array()
            .unwrap()
            .iter()
            .find_map(|event| {
                let kind = &event["kind"];
                (kind["type"] == json!("toolCallCompleted")
                    && kind["result"]["callId"] == json!("agent-bg"))
                .then(|| kind["result"]["output"].as_str())
                .flatten()
            })
            .expect("AgentTool output should be present");
        let agent_output: Value = serde_json::from_str(agent_output).unwrap();
        let run_id = agent_output["runId"].as_str().unwrap();

        let poll = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(3),
                method: "agents/poll".to_string(),
                params: json!({ "threadId": "thread-1", "runId": run_id }),
            },
        );
        assert!(poll.error.is_none(), "{:?}", poll.error);
        assert_eq!(
            poll.result.as_ref().unwrap()["run"]["status"],
            json!("blocked")
        );
        assert!(
            poll.result.unwrap()["events"]
                .as_array()
                .unwrap()
                .iter()
                .any(|event| event["kind"]["type"] == json!("approvalRequested"))
        );

        let resume = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(4),
                method: "agents/resume".to_string(),
                params: json!({
                    "threadId": "thread-1",
                    "runId": run_id,
                    "approvedToolCallIds": ["echo-approval"],
                    "modelSteps": [
                        {
                            "toolCalls": [{
                                "id": "echo-approval",
                                "name": "echo",
                                "namespace": "oppi",
                                "arguments": {
                                    "output": "approved child output",
                                    "requireApproval": true
                                }
                            }],
                            "finalResponse": false
                        },
                        {
                            "assistantDeltas": ["Approved background child done."],
                            "finalResponse": true
                        }
                    ],
                    "maxContinuations": 4
                }),
            },
        );
        assert!(resume.error.is_none(), "{:?}", resume.error);
        let result = resume.result.unwrap();
        assert_eq!(result["run"]["status"], json!("completed"));
        assert_eq!(result["output"], json!("Approved background child done."));
        assert!(
            result["events"]
                .as_array()
                .unwrap()
                .iter()
                .any(|event| { event["kind"]["type"] == json!("approvalResolved") })
        );
        assert!(result["events"].as_array().unwrap().iter().any(|event| {
            event["kind"]["type"] == json!("agentCompleted")
                && event["kind"]["runId"] == json!(run_id)
        }));
    }

    #[test]
    fn background_agent_tool_state_worker_completes_without_poll_draining() {
        let state = ServerState::new();
        let start = response_for_line_with_state(
            &state,
            &json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "thread/start",
                "params": { "project": { "id": "project", "cwd": "/repo" } }
            })
            .to_string(),
        );
        assert!(start.error.is_none(), "{:?}", start.error);
        let thread_id = start.result.unwrap()["thread"]["id"]
            .as_str()
            .unwrap()
            .to_string();

        let turn = response_for_line_with_state(
            &state,
            &json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "turn/run-agentic",
                "params": {
                    "threadId": thread_id,
                    "input": "delegate background agent",
                    "modelSteps": [{
                        "toolCalls": [{
                            "id": "agent-bg",
                            "name": "AgentTool",
                            "namespace": "oppi",
                            "arguments": {
                                "agentName": "general-purpose",
                                "task": "finish later",
                                "background": true,
                                "modelSteps": [{
                                    "assistantDeltas": ["background child done"],
                                    "finalResponse": true
                                }]
                            }
                        }],
                        "finalResponse": true
                    }]
                }
            })
            .to_string(),
        );
        assert!(turn.error.is_none(), "{:?}", turn.error);
        let turn_result = turn.result.unwrap();
        let agent_output = turn_result["events"]
            .as_array()
            .unwrap()
            .iter()
            .find_map(|event| {
                let kind = &event["kind"];
                (kind["type"] == json!("toolCallCompleted")
                    && kind["result"]["callId"] == json!("agent-bg"))
                .then(|| kind["result"]["output"].as_str())
                .flatten()
            })
            .expect("AgentTool output should be present");
        let agent_output: Value = serde_json::from_str(agent_output).unwrap();
        assert_eq!(agent_output["status"], json!("running"));
        let run_id = agent_output["runId"].as_str().unwrap().to_string();

        let mut polled = Value::Null;
        for _ in 0..50 {
            let poll = response_for_line_with_state(
                &state,
                &json!({
                    "jsonrpc": "2.0",
                    "id": 3,
                    "method": "agents/poll",
                    "params": { "threadId": thread_id, "runId": run_id }
                })
                .to_string(),
            );
            assert!(poll.error.is_none(), "{:?}", poll.error);
            polled = poll.result.unwrap();
            if polled["run"]["status"] == json!("completed") {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(polled["run"]["status"], json!("completed"));
        assert_eq!(polled["output"], json!("background child done"));
    }

    #[test]
    fn agents_crud_routes_use_filesystem_agent_definitions() {
        let root = temp_store_root("oppi-server-agent-crud");
        std::fs::create_dir_all(&root).unwrap();
        let mut runtime = Runtime::new();
        start_thread_for_root(&mut runtime, &root);

        let create = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(2),
                method: "agents/create".to_string(),
                params: json!({
                    "threadId": "thread-1",
                    "source": "project",
                    "definition": {
                        "name": "reviewer",
                        "description": "review code",
                        "instructions": "Review carefully."
                    }
                }),
            },
        );
        assert!(create.error.is_none(), "{:?}", create.error);
        assert!(
            root.join(".oppi")
                .join("agents")
                .join("reviewer.md")
                .exists()
        );

        let list = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(3),
                method: "agents/list".to_string(),
                params: json!({ "threadId": "thread-1" }),
            },
        );
        assert!(list.error.is_none(), "{:?}", list.error);
        let list_result = list.result.unwrap();
        assert!(
            list_result["items"]
                .as_array()
                .unwrap()
                .iter()
                .any(|agent| {
                    agent["active"]["name"] == json!("reviewer")
                        && agent["active"]["source"] == json!("project")
                })
        );

        let show = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(4),
                method: "agents/show".to_string(),
                params: json!({ "threadId": "thread-1", "name": "reviewer" }),
            },
        );
        assert!(show.error.is_none(), "{:?}", show.error);
        assert_eq!(
            show.result.unwrap()["active"]["description"],
            json!("review code")
        );

        let update = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(5),
                method: "agents/update".to_string(),
                params: json!({
                    "threadId": "thread-1",
                    "source": "project",
                    "overwrite": true,
                    "definition": {
                        "name": "reviewer",
                        "description": "updated review code",
                        "instructions": "Review even more carefully."
                    }
                }),
            },
        );
        assert!(update.error.is_none(), "{:?}", update.error);

        let delete = handle_request(
            &mut runtime,
            JsonRpcRequest {
                jsonrpc: Some("2.0".to_string()),
                id: json!(6),
                method: "agents/delete".to_string(),
                params: json!({
                    "threadId": "thread-1",
                    "source": "project",
                    "name": "reviewer"
                }),
            },
        );
        assert!(delete.error.is_none(), "{:?}", delete.error);
        assert!(
            !root
                .join(".oppi")
                .join("agents")
                .join("reviewer.md")
                .exists()
        );

        let _ = std::fs::remove_dir_all(&root);
    }
}
