use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddedMemoryError {
    message: String,
}

impl EmbeddedMemoryError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl std::fmt::Display for EmbeddedMemoryError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.message.fmt(formatter)
    }
}

impl std::error::Error for EmbeddedMemoryError {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddedMemoryRecord {
    pub id: String,
    pub project_id: String,
    pub content: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub superseded_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddedRememberInput {
    pub project_id: String,
    pub content: String,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddedRecallInput {
    pub project_id: String,
    pub prompt: String,
    pub limit: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddedListInput {
    pub project_id: String,
    pub query: Option<String>,
    pub limit: Option<usize>,
    pub pinned_only: bool,
    pub include_superseded: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EmbeddedRecordPatch {
    pub content: Option<String>,
    pub tags: Option<Vec<String>>,
    pub pinned: Option<bool>,
    pub superseded_by: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddedRecallResult {
    pub context_markdown: String,
    pub records: Vec<EmbeddedMemoryRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddedMemoryStatus {
    pub project_id: String,
    pub memory_count: u64,
    pub pinned_count: u64,
    pub store_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EmbeddedMemoryState {
    next_id: u64,
    #[serde(default)]
    records: Vec<EmbeddedMemoryRecord>,
}

#[derive(Debug, Clone)]
pub struct EmbeddedMemoryStore {
    state: EmbeddedMemoryState,
    path: Option<PathBuf>,
}

impl EmbeddedMemoryStore {
    pub fn new_for_test() -> Self {
        Self {
            state: EmbeddedMemoryState::default(),
            path: None,
        }
    }

    pub fn open(path: impl Into<PathBuf>) -> Result<Self, EmbeddedMemoryError> {
        let path = path.into();
        let state = if path.exists() {
            let text = fs::read_to_string(&path).map_err(|error| {
                EmbeddedMemoryError::new(format!("read embedded memory store: {error}"))
            })?;
            if text.trim().is_empty() {
                EmbeddedMemoryState::default()
            } else {
                serde_json::from_str(&text).map_err(|error| {
                    EmbeddedMemoryError::new(format!("parse embedded memory store: {error}"))
                })?
            }
        } else {
            EmbeddedMemoryState::default()
        };
        Ok(Self {
            state,
            path: Some(path),
        })
    }

    pub fn store_path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn remember(
        &mut self,
        input: EmbeddedRememberInput,
    ) -> Result<EmbeddedMemoryRecord, EmbeddedMemoryError> {
        let project_id = normalize_required(input.project_id, "project id")?;
        let content = normalize_required(input.content, "memory content")?;
        let tags = normalize_tags(input.tags);
        let id = self.next_record_id();
        let record = EmbeddedMemoryRecord {
            id,
            project_id,
            content,
            tags,
            pinned: false,
            superseded_by: None,
            source: Some("oppi:embedded".to_string()),
        };
        self.state.records.push(record.clone());
        self.persist()?;
        Ok(record)
    }

    pub fn recall(
        &self,
        input: EmbeddedRecallInput,
    ) -> Result<EmbeddedRecallResult, EmbeddedMemoryError> {
        let project_id = normalize_required(input.project_id, "project id")?;
        let prompt = normalize_required(input.prompt, "recall prompt")?;
        let limit = input.limit.max(1);
        let mut scored = self
            .project_records(&project_id, false)
            .into_iter()
            .filter_map(|record| {
                let score = lexical_score(&prompt, record);
                (score > 0).then_some((score, record.clone()))
            })
            .collect::<Vec<_>>();
        scored.sort_by(|left, right| {
            right
                .0
                .cmp(&left.0)
                .then_with(|| left.1.id.cmp(&right.1.id))
        });
        let records = scored
            .into_iter()
            .take(limit)
            .map(|(_, record)| record)
            .collect::<Vec<_>>();
        let context_markdown = records
            .iter()
            .map(|record| format!("- {}", record.content))
            .collect::<Vec<_>>()
            .join("\n");
        Ok(EmbeddedRecallResult {
            context_markdown,
            records,
        })
    }

    pub fn list(
        &self,
        input: EmbeddedListInput,
    ) -> Result<Vec<EmbeddedMemoryRecord>, EmbeddedMemoryError> {
        let project_id = normalize_required(input.project_id, "project id")?;
        let query = input
            .query
            .as_deref()
            .map(str::trim)
            .filter(|query| !query.is_empty())
            .map(str::to_string);
        let mut records = self
            .project_records(&project_id, input.include_superseded)
            .into_iter()
            .filter(|record| !input.pinned_only || record.pinned)
            .filter(|record| {
                query
                    .as_ref()
                    .is_none_or(|query| lexical_score(query, record) > 0)
            })
            .cloned()
            .collect::<Vec<_>>();
        records.sort_by(|left, right| left.id.cmp(&right.id));
        if let Some(limit) = input.limit {
            records.truncate(limit);
        }
        Ok(records)
    }

    pub fn status(&self, project_id: impl Into<String>) -> EmbeddedMemoryStatus {
        let project_id = project_id.into();
        let records = self.project_records(&project_id, false);
        EmbeddedMemoryStatus {
            project_id,
            memory_count: records.len() as u64,
            pinned_count: records.iter().filter(|record| record.pinned).count() as u64,
            store_path: self.path.clone(),
        }
    }

    pub fn forget(
        &mut self,
        project_id: &str,
        id: &str,
    ) -> Result<Option<EmbeddedMemoryRecord>, EmbeddedMemoryError> {
        let index = self
            .state
            .records
            .iter()
            .position(|record| record.project_id == project_id && record.id == id);
        let removed = index.map(|index| self.state.records.remove(index));
        if removed.is_some() {
            self.persist()?;
        }
        Ok(removed)
    }

    pub fn pin(
        &mut self,
        project_id: &str,
        id: &str,
        pinned: bool,
    ) -> Result<Option<EmbeddedMemoryRecord>, EmbeddedMemoryError> {
        let updated = self.with_record_mut(project_id, id, |record| {
            record.pinned = pinned;
        });
        if updated.is_some() {
            self.persist()?;
        }
        Ok(updated)
    }

    pub fn update(
        &mut self,
        project_id: &str,
        id: &str,
        patch: EmbeddedRecordPatch,
    ) -> Result<Option<EmbeddedMemoryRecord>, EmbeddedMemoryError> {
        let updated = self.with_record_mut(project_id, id, |record| {
            if let Some(content) = patch.content {
                let content = content.trim().to_string();
                if !content.is_empty() {
                    record.content = content;
                }
            }
            if let Some(tags) = patch.tags {
                record.tags = normalize_tags(tags);
            }
            if let Some(pinned) = patch.pinned {
                record.pinned = pinned;
            }
            if let Some(superseded_by) = patch.superseded_by {
                record.superseded_by = Some(superseded_by);
            }
        });
        if updated.is_some() {
            self.persist()?;
        }
        Ok(updated)
    }

    pub fn supersede(
        &mut self,
        project_id: &str,
        id: &str,
        superseded_by: String,
    ) -> Result<Option<EmbeddedMemoryRecord>, EmbeddedMemoryError> {
        self.update(
            project_id,
            id,
            EmbeddedRecordPatch {
                superseded_by: Some(superseded_by),
                ..EmbeddedRecordPatch::default()
            },
        )
    }

    fn project_records(
        &self,
        project_id: &str,
        include_superseded: bool,
    ) -> Vec<&EmbeddedMemoryRecord> {
        self.state
            .records
            .iter()
            .filter(|record| record.project_id == project_id)
            .filter(|record| include_superseded || record.superseded_by.is_none())
            .collect()
    }

    fn with_record_mut(
        &mut self,
        project_id: &str,
        id: &str,
        update: impl FnOnce(&mut EmbeddedMemoryRecord),
    ) -> Option<EmbeddedMemoryRecord> {
        let record = self
            .state
            .records
            .iter_mut()
            .find(|record| record.project_id == project_id && record.id == id)?;
        update(record);
        Some(record.clone())
    }

    fn next_record_id(&mut self) -> String {
        self.state.next_id = self.state.next_id.saturating_add(1);
        format!("mem-{:06}", self.state.next_id)
    }

    fn persist(&self) -> Result<(), EmbeddedMemoryError> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                EmbeddedMemoryError::new(format!("create embedded memory store directory: {error}"))
            })?;
        }
        let text = serde_json::to_string_pretty(&self.state).map_err(|error| {
            EmbeddedMemoryError::new(format!("serialize embedded memory store: {error}"))
        })?;
        fs::write(path, text).map_err(|error| {
            EmbeddedMemoryError::new(format!("write embedded memory store: {error}"))
        })
    }
}

fn normalize_required(value: String, label: &str) -> Result<String, EmbeddedMemoryError> {
    let value = value.trim().to_string();
    if value.is_empty() {
        return Err(EmbeddedMemoryError::new(format!("{label} cannot be empty")));
    }
    Ok(value)
}

fn normalize_tags(tags: Vec<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    tags.into_iter()
        .map(|tag| tag.trim().to_string())
        .filter(|tag| !tag.is_empty())
        .filter(|tag| seen.insert(tag.clone()))
        .collect()
}

fn lexical_score(query: &str, record: &EmbeddedMemoryRecord) -> usize {
    let haystack =
        format!("{} {} {}", record.id, record.content, record.tags.join(" ")).to_ascii_lowercase();
    tokenize(query)
        .into_iter()
        .filter(|token| haystack.contains(token))
        .count()
}

fn tokenize(text: &str) -> Vec<String> {
    text.split(|character: char| !character.is_alphanumeric())
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(str::to_ascii_lowercase)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_memory_store_remembers_and_recalls_project_records() {
        let mut store = EmbeddedMemoryStore::new_for_test();
        let saved = store
            .remember(EmbeddedRememberInput {
                project_id: "oppi".to_string(),
                content: "OPPi uses runtime-owned memory backend routes.".to_string(),
                tags: vec!["oppi".to_string()],
            })
            .unwrap();

        let recalled = store
            .recall(EmbeddedRecallInput {
                project_id: "oppi".to_string(),
                prompt: "memory backend routes".to_string(),
                limit: 5,
            })
            .unwrap();

        assert_eq!(saved.project_id, "oppi");
        assert_eq!(recalled.records.len(), 1);
        assert!(
            recalled
                .context_markdown
                .contains("runtime-owned memory backend routes")
        );
    }

    #[test]
    fn embedded_memory_store_scopes_search_by_project() {
        let mut store = EmbeddedMemoryStore::new_for_test();
        store
            .remember(EmbeddedRememberInput {
                project_id: "oppi".to_string(),
                content: "OPPi uses embedded memory.".to_string(),
                tags: vec!["oppi".to_string()],
            })
            .unwrap();
        store
            .remember(EmbeddedRememberInput {
                project_id: "other".to_string(),
                content: "Other project also says embedded memory.".to_string(),
                tags: vec!["other".to_string()],
            })
            .unwrap();

        let recalled = store
            .recall(EmbeddedRecallInput {
                project_id: "oppi".to_string(),
                prompt: "embedded memory".to_string(),
                limit: 5,
            })
            .unwrap();

        assert_eq!(recalled.records.len(), 1);
        assert_eq!(recalled.records[0].project_id, "oppi");
    }

    #[test]
    fn embedded_memory_store_persists_file_backed_records() {
        let path = std::env::temp_dir().join(format!(
            "oppi-memory-store-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        {
            let mut store = EmbeddedMemoryStore::open(&path).unwrap();
            store
                .remember(EmbeddedRememberInput {
                    project_id: "oppi".to_string(),
                    content: "File backed embedded memory persists.".to_string(),
                    tags: vec!["persist".to_string()],
                })
                .unwrap();
        }

        let reopened = EmbeddedMemoryStore::open(&path).unwrap();
        let recalled = reopened
            .recall(EmbeddedRecallInput {
                project_id: "oppi".to_string(),
                prompt: "file backed persists".to_string(),
                limit: 5,
            })
            .unwrap();

        assert_eq!(recalled.records.len(), 1);
        let _ = fs::remove_file(path);
    }
}
