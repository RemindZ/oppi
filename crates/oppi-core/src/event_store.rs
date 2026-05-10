use oppi_protocol::{Event, EventId, ThreadId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

pub const CURRENT_STORE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug)]
pub enum EventStoreError {
    Io(std::io::Error),
    Json(serde_json::Error),
    LockAlreadyHeld { path: PathBuf },
    UnsupportedSchema { found: u32, supported: u32 },
}

impl From<std::io::Error> for EventStoreError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for EventStoreError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

pub trait EventStore {
    fn append(&mut self, event: Event) -> Result<(), EventStoreError>;
    fn list_after(&self, thread_id: &str, after: EventId) -> Result<Vec<Event>, EventStoreError>;
    fn list_after_limit(
        &self,
        thread_id: &str,
        after: EventId,
        limit: usize,
    ) -> Result<Vec<Event>, EventStoreError> {
        let mut events = self.list_after(thread_id, after)?;
        events.truncate(limit);
        Ok(events)
    }
    fn list_thread(&self, thread_id: &str) -> Result<Vec<Event>, EventStoreError> {
        self.list_after(thread_id, 0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreNamespace {
    pub project_id: String,
    pub runtime_id: String,
}

impl StoreNamespace {
    pub fn new(project_id: impl Into<String>, runtime_id: impl Into<String>) -> Self {
        Self {
            project_id: project_id.into(),
            runtime_id: runtime_id.into(),
        }
    }
}

impl Default for StoreNamespace {
    fn default() -> Self {
        Self::new("default-project", "default-runtime")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreMetadata {
    pub schema_version: u32,
    pub namespace: StoreNamespace,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotEnvelope {
    pub schema_version: u32,
    pub thread_id: ThreadId,
    pub last_event_id: EventId,
    pub state: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdempotencyRecord {
    pub key: String,
    pub thread_id: ThreadId,
    pub event_ids: Vec<EventId>,
}

#[derive(Debug, Default, Clone)]
pub struct InMemoryEventStore {
    events: BTreeMap<ThreadId, Vec<Event>>,
}

impl InMemoryEventStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn threads(&self) -> Vec<ThreadId> {
        self.events.keys().cloned().collect()
    }
}

impl EventStore for InMemoryEventStore {
    fn append(&mut self, event: Event) -> Result<(), EventStoreError> {
        self.events
            .entry(event.thread_id.clone())
            .or_default()
            .push(event);
        Ok(())
    }

    fn list_after(&self, thread_id: &str, after: EventId) -> Result<Vec<Event>, EventStoreError> {
        Ok(self
            .events
            .get(thread_id)
            .map(|items| {
                items
                    .iter()
                    .filter(|event| event.id > after)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default())
    }

    fn list_after_limit(
        &self,
        thread_id: &str,
        after: EventId,
        limit: usize,
    ) -> Result<Vec<Event>, EventStoreError> {
        Ok(self
            .events
            .get(thread_id)
            .map(|items| {
                items
                    .iter()
                    .filter(|event| event.id > after)
                    .take(limit)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default())
    }
}

#[derive(Debug, Clone)]
pub struct FilesystemEventStore {
    root: PathBuf,
    store_root: PathBuf,
    namespace: StoreNamespace,
}

impl FilesystemEventStore {
    pub fn new(root: impl Into<PathBuf>) -> Result<Self, EventStoreError> {
        Self::with_namespace(root, StoreNamespace::default())
    }

    pub fn with_namespace(
        root: impl Into<PathBuf>,
        namespace: StoreNamespace,
    ) -> Result<Self, EventStoreError> {
        let root = root.into();
        let store_root = root
            .join("projects")
            .join(encode_path_component(&namespace.project_id))
            .join("runtimes")
            .join(encode_path_component(&namespace.runtime_id));
        fs::create_dir_all(store_root.join("events"))?;
        fs::create_dir_all(store_root.join("snapshots"))?;
        fs::create_dir_all(store_root.join("idempotency"))?;
        let store = Self {
            root,
            store_root,
            namespace,
        };
        store.write_metadata()?;
        Ok(store)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn store_root(&self) -> &Path {
        &self.store_root
    }

    pub fn namespace(&self) -> &StoreNamespace {
        &self.namespace
    }

    pub fn metadata(&self) -> StoreMetadata {
        StoreMetadata {
            schema_version: CURRENT_STORE_SCHEMA_VERSION,
            namespace: self.namespace.clone(),
        }
    }

    pub fn acquire_lock(&self) -> Result<AdvisoryStoreLock, EventStoreError> {
        AdvisoryStoreLock::acquire(self.store_root.join("store.lock"))
    }

    pub fn write_snapshot(&self, snapshot: &SnapshotEnvelope) -> Result<(), EventStoreError> {
        let snapshot = migrate_snapshot(snapshot.clone())?;
        fs::create_dir_all(self.store_root.join("snapshots"))?;
        let path = self.snapshot_path(&snapshot.thread_id, snapshot.last_event_id);
        let temp_path = path.with_extension("json.tmp");
        let mut file = fs::File::create(&temp_path)?;
        serde_json::to_writer_pretty(&mut file, &snapshot)?;
        writeln!(file)?;
        file.flush()?;
        fs::rename(temp_path, path)?;
        Ok(())
    }

    pub fn latest_snapshot(
        &self,
        thread_id: &str,
    ) -> Result<Option<SnapshotEnvelope>, EventStoreError> {
        let snapshot_dir = self.store_root.join("snapshots");
        if !snapshot_dir.exists() {
            return Ok(None);
        }
        let prefix = format!("{}-", encode_path_component(thread_id));
        let mut latest: Option<SnapshotEnvelope> = None;
        for entry in fs::read_dir(snapshot_dir)? {
            let entry = entry?;
            let path = entry.path();
            let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if !file_name.starts_with(&prefix) || !file_name.ends_with(".json") {
                continue;
            }
            let file = fs::File::open(path)?;
            let snapshot: SnapshotEnvelope = migrate_snapshot(serde_json::from_reader(file)?)?;
            if latest
                .as_ref()
                .is_none_or(|current| snapshot.last_event_id > current.last_event_id)
            {
                latest = Some(snapshot);
            }
        }
        Ok(latest)
    }

    pub fn record_idempotency(&self, record: &IdempotencyRecord) -> Result<(), EventStoreError> {
        fs::create_dir_all(self.store_root.join("idempotency"))?;
        let path = self.idempotency_path(&record.key);
        let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
        serde_json::to_writer_pretty(&mut file, record)?;
        writeln!(file)?;
        file.flush()?;
        Ok(())
    }

    pub fn read_idempotency(
        &self,
        key: &str,
    ) -> Result<Option<IdempotencyRecord>, EventStoreError> {
        let path = self.idempotency_path(key);
        if !path.exists() {
            return Ok(None);
        }
        Ok(Some(serde_json::from_reader(fs::File::open(path)?)?))
    }

    pub fn list_all_events(&self) -> Result<Vec<Event>, EventStoreError> {
        let events_dir = self.store_root.join("events");
        if !events_dir.exists() {
            return Ok(Vec::new());
        }
        let mut events = Vec::new();
        for entry in fs::read_dir(events_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
                continue;
            }
            match read_event_file(&path) {
                Ok(mut file_events) => events.append(&mut file_events),
                Err(EventStoreError::Json(_)) => {
                    continue;
                }
                Err(error) => return Err(error),
            }
        }
        events.sort_by_key(|event| event.id);
        Ok(events)
    }

    pub fn max_thread_counter_hint(&self) -> Result<u64, EventStoreError> {
        let events_dir = self.store_root.join("events");
        if !events_dir.exists() {
            return Ok(0);
        }
        let mut max = 0;
        for entry in fs::read_dir(events_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|name| name.to_str()) else {
                continue;
            };
            max = max.max(numeric_suffix(stem).unwrap_or(0));
        }
        Ok(max)
    }

    fn write_metadata(&self) -> Result<(), EventStoreError> {
        let mut file = fs::File::create(self.store_root.join("metadata.json"))?;
        serde_json::to_writer_pretty(&mut file, &self.metadata())?;
        writeln!(file)?;
        file.flush()?;
        Ok(())
    }

    fn event_path(&self, thread_id: &str) -> PathBuf {
        self.store_root
            .join("events")
            .join(format!("{}.jsonl", encode_path_component(thread_id)))
    }

    fn snapshot_path(&self, thread_id: &str, last_event_id: EventId) -> PathBuf {
        self.store_root.join("snapshots").join(format!(
            "{}-{last_event_id:020}.json",
            encode_path_component(thread_id)
        ))
    }

    fn idempotency_path(&self, key: &str) -> PathBuf {
        self.store_root
            .join("idempotency")
            .join(format!("{}.json", encode_path_component(key)))
    }
}

fn read_event_file(path: &Path) -> Result<Vec<Event>, EventStoreError> {
    let file = fs::File::open(path)?;
    let mut events = Vec::new();
    for line in BufReader::new(file).lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        events.push(serde_json::from_str::<Event>(&line)?);
    }
    Ok(events)
}

fn numeric_suffix(id: &str) -> Option<u64> {
    id.rsplit_once('-')?.1.parse().ok()
}

impl EventStore for FilesystemEventStore {
    fn append(&mut self, event: Event) -> Result<(), EventStoreError> {
        fs::create_dir_all(self.store_root.join("events"))?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.event_path(&event.thread_id))?;
        serde_json::to_writer(&mut file, &event)?;
        writeln!(file)?;
        file.flush()?;
        Ok(())
    }

    fn list_after(&self, thread_id: &str, after: EventId) -> Result<Vec<Event>, EventStoreError> {
        let path = self.event_path(thread_id);
        if !path.exists() {
            return Ok(Vec::new());
        }
        let file = fs::File::open(path)?;
        let mut events = Vec::new();
        for line in BufReader::new(file).lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let event: Event = serde_json::from_str(&line)?;
            if event.id > after {
                events.push(event);
            }
        }
        Ok(events)
    }

    fn list_after_limit(
        &self,
        thread_id: &str,
        after: EventId,
        limit: usize,
    ) -> Result<Vec<Event>, EventStoreError> {
        let path = self.event_path(thread_id);
        if !path.exists() {
            return Ok(Vec::new());
        }
        let file = fs::File::open(path)?;
        let mut events = Vec::new();
        for line in BufReader::new(file).lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let event: Event = serde_json::from_str(&line)?;
            if event.id > after {
                events.push(event);
                if events.len() >= limit {
                    break;
                }
            }
        }
        Ok(events)
    }
}

#[derive(Debug)]
pub struct AdvisoryStoreLock {
    path: PathBuf,
}

impl AdvisoryStoreLock {
    fn acquire(path: PathBuf) -> Result<Self, EventStoreError> {
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                writeln!(file, "pid={}", std::process::id())?;
                file.flush()?;
                Ok(Self { path })
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                Err(EventStoreError::LockAlreadyHeld { path })
            }
            Err(error) => Err(EventStoreError::Io(error)),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for AdvisoryStoreLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn migrate_snapshot(snapshot: SnapshotEnvelope) -> Result<SnapshotEnvelope, EventStoreError> {
    if snapshot.schema_version > CURRENT_STORE_SCHEMA_VERSION {
        return Err(EventStoreError::UnsupportedSchema {
            found: snapshot.schema_version,
            supported: CURRENT_STORE_SCHEMA_VERSION,
        });
    }
    // Schema v1 is the first persisted snapshot format. Future migrations should
    // transform older envelopes here before callers see them.
    Ok(SnapshotEnvelope {
        schema_version: CURRENT_STORE_SCHEMA_VERSION,
        ..snapshot
    })
}

fn encode_path_component(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        match byte {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' => encoded.push(byte as char),
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;
    use oppi_protocol::{EventKind, Thread, ThreadStatus};
    use serde_json::json;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn event(id: EventId, thread_id: &str) -> Event {
        Event {
            id,
            thread_id: thread_id.to_string(),
            turn_id: None,
            kind: EventKind::ThreadStarted {
                thread: Thread {
                    id: thread_id.to_string(),
                    project: oppi_protocol::ProjectRef {
                        id: "project".to_string(),
                        cwd: "/repo".to_string(),
                        display_name: None,
                        workspace_roots: Vec::new(),
                    },
                    status: ThreadStatus::Active,
                    title: None,
                    forked_from: None,
                },
            },
        }
    }

    fn temp_root(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("{name}-{unique}"))
    }

    #[test]
    fn in_memory_store_lists_events_after_cursor() {
        let mut store = InMemoryEventStore::new();
        store.append(event(1, "thread-1")).unwrap();
        store.append(event(2, "thread-1")).unwrap();
        assert_eq!(store.list_after("thread-1", 1).unwrap().len(), 1);
    }

    #[test]
    fn in_memory_store_lists_events_with_limit() {
        let mut store = InMemoryEventStore::new();
        store.append(event(1, "thread-1")).unwrap();
        store.append(event(2, "thread-1")).unwrap();
        store.append(event(3, "thread-1")).unwrap();
        let events = store.list_after_limit("thread-1", 1, 1).unwrap();
        assert_eq!(events, vec![event(2, "thread-1")]);
    }

    #[test]
    fn filesystem_store_appends_jsonl_and_reloads_by_thread() {
        let root = temp_root("oppi-event-store-test");
        let namespace = StoreNamespace::new("project/one", "runtime:one");
        let mut store = FilesystemEventStore::with_namespace(&root, namespace.clone()).unwrap();
        store.append(event(1, "thread/one")).unwrap();
        store.append(event(2, "thread/one")).unwrap();

        let reloaded = FilesystemEventStore::with_namespace(&root, namespace).unwrap();
        let events = reloaded.list_after("thread/one", 0).unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[1].id, 2);
        let limited = reloaded.list_after_limit("thread/one", 0, 1).unwrap();
        assert_eq!(limited, vec![event(1, "thread/one")]);
        assert!(reloaded.store_root().join("metadata.json").exists());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn filesystem_store_global_replay_skips_corrupt_thread_files() {
        let root = temp_root("oppi-event-store-corrupt-thread-test");
        let namespace = StoreNamespace::new("project", "runtime");
        let mut store = FilesystemEventStore::with_namespace(&root, namespace.clone()).unwrap();
        store.append(event(1, "thread-good")).unwrap();
        fs::write(
            store.store_root().join("events").join("thread-bad.jsonl"),
            "{{not valid json}}\n",
        )
        .unwrap();

        let reloaded = FilesystemEventStore::with_namespace(&root, namespace).unwrap();
        let events = reloaded.list_all_events().unwrap();

        assert_eq!(events, vec![event(1, "thread-good")]);
        assert!(reloaded.list_thread("thread-bad").is_err());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn filesystem_store_is_namespaced_by_project_and_runtime() {
        let root = temp_root("oppi-event-namespace-test");
        let mut first = FilesystemEventStore::with_namespace(
            &root,
            StoreNamespace::new("project", "runtime-one"),
        )
        .unwrap();
        let mut second = FilesystemEventStore::with_namespace(
            &root,
            StoreNamespace::new("project", "runtime-two"),
        )
        .unwrap();
        first.append(event(1, "thread-1")).unwrap();
        second.append(event(2, "thread-1")).unwrap();

        assert_eq!(first.list_thread("thread-1").unwrap()[0].id, 1);
        assert_eq!(second.list_thread("thread-1").unwrap()[0].id, 2);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn snapshots_round_trip_and_latest_wins() {
        let root = temp_root("oppi-snapshot-test");
        let store = FilesystemEventStore::new(&root).unwrap();
        store
            .write_snapshot(&SnapshotEnvelope {
                schema_version: CURRENT_STORE_SCHEMA_VERSION,
                thread_id: "thread-1".to_string(),
                last_event_id: 1,
                state: json!({ "turns": 1 }),
            })
            .unwrap();
        store
            .write_snapshot(&SnapshotEnvelope {
                schema_version: CURRENT_STORE_SCHEMA_VERSION,
                thread_id: "thread-1".to_string(),
                last_event_id: 3,
                state: json!({ "turns": 3 }),
            })
            .unwrap();

        let latest = store.latest_snapshot("thread-1").unwrap().unwrap();
        assert_eq!(latest.last_event_id, 3);
        assert_eq!(latest.state, json!({ "turns": 3 }));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn advisory_lock_blocks_second_holder_until_drop() {
        let root = temp_root("oppi-lock-test");
        let store = FilesystemEventStore::new(&root).unwrap();
        let lock = store.acquire_lock().unwrap();
        assert!(matches!(
            store.acquire_lock().unwrap_err(),
            EventStoreError::LockAlreadyHeld { .. }
        ));
        drop(lock);
        assert!(store.acquire_lock().is_ok());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn idempotency_records_are_create_once() {
        let root = temp_root("oppi-idempotency-test");
        let store = FilesystemEventStore::new(&root).unwrap();
        let record = IdempotencyRecord {
            key: "request-1".to_string(),
            thread_id: "thread-1".to_string(),
            event_ids: vec![1, 2],
        };
        store.record_idempotency(&record).unwrap();
        assert_eq!(store.read_idempotency("request-1").unwrap(), Some(record));
        assert!(matches!(
            store
                .record_idempotency(&IdempotencyRecord {
                    key: "request-1".to_string(),
                    thread_id: "thread-1".to_string(),
                    event_ids: vec![3],
                })
                .unwrap_err(),
            EventStoreError::Io(_)
        ));

        let _ = fs::remove_dir_all(root);
    }
}
