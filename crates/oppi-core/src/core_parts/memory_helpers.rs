use super::*;

pub(super) fn numeric_suffix(id: &str) -> Option<u64> {
    id.rsplit_once('-')?.1.parse().ok()
}

pub(super) fn normalize_thread_title(title: &str) -> Result<Option<String>, RuntimeError> {
    let title = title.trim();
    if title.is_empty() {
        return Ok(None);
    }
    if title.chars().count() > 160 {
        return Err(RuntimeError::new(
            "invalid_thread_title",
            RuntimeErrorCategory::InvalidRequest,
            "thread title must be 160 characters or fewer",
        ));
    }
    Ok(Some(title.to_string()))
}

pub(super) fn event_store_error(error: event_store::EventStoreError) -> RuntimeError {
    RuntimeError::new(
        "event_store_error",
        RuntimeErrorCategory::EventStore,
        format!("event store error: {error:?}"),
    )
}

pub(super) fn thread_item_matches_filter(
    kind: &ItemKind,
    labels: &[String],
    filter: ThreadItemFilter,
) -> bool {
    match filter {
        ThreadItemFilter::All => true,
        ThreadItemFilter::UserOnly => matches!(kind, ItemKind::UserMessage { .. }),
        ThreadItemFilter::LabeledOnly => !labels.is_empty(),
        ThreadItemFilter::Default | ThreadItemFilter::NoTools => {
            !matches!(kind, ItemKind::ToolCall(_) | ItemKind::ToolResult(_))
        }
    }
}

pub(super) fn normalize_thread_item_labels(labels: Vec<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut normalized = Vec::new();
    for label in labels {
        let label = label.trim();
        if label.is_empty() {
            continue;
        }
        if seen.insert(label.to_ascii_lowercase()) {
            normalized.push(label.to_string());
        }
    }
    normalized
}

pub(super) fn not_found(kind: &str, id: &str) -> RuntimeError {
    RuntimeError::new(
        format!("{}_not_found", kind.replace(' ', "_")),
        RuntimeErrorCategory::NotFound,
        format!("{kind} not found: {id}"),
    )
}

pub(super) fn memory_dashboard_fallback_url(url: &str) -> Option<String> {
    url.contains("hoppi.localhost")
        .then(|| url.replace("hoppi.localhost", "localhost"))
        .filter(|fallback| fallback != url)
}

pub(super) fn memory_backend_recall_stage(params: &MemoryBackendRecallParams) -> &'static str {
    match (params.include_startup, params.include_task) {
        (true, true) => "recall-startup-task",
        (true, false) => "recall-startup",
        (false, true) => "recall-task",
        (false, false) => "recall",
    }
}

pub(super) fn memory_backend_list_stage(params: &MemoryBackendListParams) -> &'static str {
    match (params.include_superseded, params.pinned_only) {
        (true, true) => "list-all-pinned",
        (true, false) => "list-all",
        (false, true) => "list-pinned",
        (false, false) => "list",
    }
}

pub(super) fn memory_backend_sync_stage(direction: MemoryBackendSyncDirection) -> &'static str {
    match direction {
        MemoryBackendSyncDirection::Pull => "sync-pull",
        MemoryBackendSyncDirection::Push => "sync-push",
        MemoryBackendSyncDirection::Both => "sync-both",
    }
}

pub(super) fn memory_backend_required_text(
    value: &str,
    field: &str,
    max_chars: usize,
) -> Result<String, RuntimeError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(RuntimeError::new(
            format!("invalid_memory_backend_{field}"),
            RuntimeErrorCategory::InvalidRequest,
            format!("memory backend {field} cannot be empty"),
        ));
    }
    Ok(value.chars().take(max_chars).collect())
}

pub(super) fn memory_backend_optional_text(
    value: Option<String>,
    max_chars: usize,
) -> Option<String> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.chars().take(max_chars).collect())
}

fn memory_backend_optional_text_vec(values: Vec<String>, max_items: usize) -> Vec<String> {
    values
        .into_iter()
        .filter_map(|value| memory_backend_optional_text(Some(value), 128))
        .take(max_items)
        .collect()
}

pub(super) fn memory_recurring_texts(
    values: Vec<String>,
    max_items: usize,
    max_chars: usize,
) -> Vec<String> {
    values
        .into_iter()
        .filter_map(|value| memory_backend_optional_text(Some(value), max_chars))
        .take(max_items)
        .collect()
}

fn memory_recurring_tag(value: String) -> Option<String> {
    let value = value.trim();
    if value.is_empty() || value.to_ascii_lowercase().starts_with("project:") {
        return None;
    }

    let mut normalized = String::new();
    let mut previous_dash = true;
    for character in value.to_ascii_lowercase().chars() {
        let mapped = if character.is_ascii_alphanumeric() || matches!(character, ':' | '_' | '-') {
            character
        } else {
            '-'
        };
        if mapped == '-' {
            if !previous_dash {
                normalized.push('-');
                previous_dash = true;
            }
        } else {
            normalized.push(mapped);
            previous_dash = false;
        }
        if normalized.len() >= 64 {
            break;
        }
    }

    let normalized = normalized.trim_matches('-').to_string();
    (normalized.len() >= 2 && !normalized.starts_with("project:")).then_some(normalized)
}

fn recurring_task_record_id(task: &str) -> String {
    let mut slug = String::new();
    let mut previous_dash = true;
    for character in task.to_ascii_lowercase().chars() {
        if character.is_ascii_alphanumeric() {
            slug.push(character);
            previous_dash = false;
        } else if !previous_dash {
            slug.push('-');
            previous_dash = true;
        }
        if slug.len() >= 72 {
            break;
        }
    }
    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        "recurring-task-candidate".to_string()
    } else {
        format!("recurring-task-{slug}")
    }
}

fn append_recurring_task_list_section(lines: &mut Vec<String>, heading: &str, items: &[String]) {
    if items.is_empty() {
        return;
    }
    lines.push(heading.to_string());
    for item in items {
        lines.push(format!("- {item}"));
    }
}

fn recurring_task_candidate_content(
    task: &str,
    rationale: Option<&str>,
    prompt_hints: &[String],
    outcomes: &[String],
    related_memory_ids: &[String],
    source_project: Option<&str>,
    source_session_id: Option<&str>,
    recent_user_texts: &[String],
    recent_assistant_texts: &[String],
) -> String {
    let mut lines = vec![
        "Recurring task candidate".to_string(),
        format!("Task: {task}"),
    ];
    if let Some(rationale) = rationale {
        lines.push(format!("Rationale: {rationale}"));
    }
    if let Some(source_project) = source_project {
        lines.push(format!("Source project: {source_project}"));
    }
    if let Some(source_session_id) = source_session_id {
        lines.push(format!("Source session: {source_session_id}"));
    }
    lines.push(
        "Approval gate: only propose skill creation after explicit user approval.".to_string(),
    );
    append_recurring_task_list_section(&mut lines, "Prompt hints:", prompt_hints);
    append_recurring_task_list_section(&mut lines, "Recent user wording:", recent_user_texts);
    append_recurring_task_list_section(&mut lines, "Observed outcomes:", outcomes);
    append_recurring_task_list_section(
        &mut lines,
        "Recent assistant outcomes:",
        recent_assistant_texts,
    );
    append_recurring_task_list_section(&mut lines, "Related memories:", related_memory_ids);
    lines.join("\n")
}

pub(super) fn recurring_task_candidate_record(
    candidate: MemoryRecurringTaskCandidate,
    source_project: Option<&str>,
    source_session_id: Option<&str>,
    recent_user_texts: &[String],
    recent_assistant_texts: &[String],
) -> Option<MemoryRecord> {
    let task = memory_backend_optional_text(Some(candidate.task), 512)?;
    if task.chars().count() < RECURRING_TASK_MIN_TASK_CHARS {
        return None;
    }
    let rationale = memory_backend_optional_text(candidate.rationale, 2048);
    let prompt_hints = memory_recurring_texts(candidate.prompt_hints, 8, 512);
    let outcomes = memory_recurring_texts(candidate.outcomes, 8, 512);
    let related_memory_ids = memory_recurring_texts(candidate.related_memory_ids, 16, 256);
    let mut seen = BTreeSet::new();
    let mut tags = Vec::new();
    for tag in [
        "oppi-recurring-task-candidate".to_string(),
        "system-scope".to_string(),
        "skill-candidate".to_string(),
    ] {
        if seen.insert(tag.clone()) {
            tags.push(tag);
        }
    }
    for tag in candidate.tags.into_iter().filter_map(memory_recurring_tag) {
        if seen.insert(tag.clone()) {
            tags.push(tag);
        }
    }

    Some(MemoryRecord {
        id: recurring_task_record_id(&task),
        content: recurring_task_candidate_content(
            &task,
            rationale.as_deref(),
            &prompt_hints,
            &outcomes,
            &related_memory_ids,
            source_project,
            source_session_id,
            recent_user_texts,
            recent_assistant_texts,
        ),
        tags,
        layer: Some("semantic".to_string()),
        confidence: Some("observed".to_string()),
        source: Some("oppi:recurring-task-mining".to_string()),
        ..MemoryRecord::default()
    })
}

#[derive(Debug, Clone)]
pub(super) struct DistilledTurnMemory {
    pub(super) remember: bool,
    pub(super) request: Option<String>,
    pub(super) completed: Vec<String>,
    pub(super) learned: Vec<String>,
    pub(super) decisions: Vec<String>,
    pub(super) next: Vec<String>,
    pub(super) files: Vec<String>,
    pub(super) tags: Vec<String>,
    pub(super) recurring_tasks: Vec<MemoryRecurringTaskCandidate>,
}

pub(super) fn memory_distiller_input(user_text: &str, assistant_text: &str) -> String {
    let payload = serde_json::to_string_pretty(&json!({
        "user": user_text,
        "assistant": assistant_text,
    }))
    .unwrap_or_else(|_| "{}".to_string());
    format!("Turn input:\n{payload}")
        .chars()
        .take(MEMORY_DISTILLER_MAX_PROMPT_CHARS)
        .collect()
}

fn memory_distiller_json_object(raw: &str) -> Result<Value, RuntimeError> {
    let mut trimmed = raw.trim();
    if let Some(stripped) = trimmed.strip_prefix("```json") {
        trimmed = stripped.trim();
    } else if let Some(stripped) = trimmed.strip_prefix("```") {
        trimmed = stripped.trim();
    }
    if let Some(stripped) = trimmed.strip_suffix("```") {
        trimmed = stripped.trim();
    }
    match serde_json::from_str::<Value>(trimmed) {
        Ok(value) if value.is_object() => return Ok(value),
        Ok(_) => {
            return Err(RuntimeError::new(
                "memory_distiller_invalid_json",
                RuntimeErrorCategory::Provider,
                "memory distiller returned JSON that was not an object",
            ));
        }
        Err(_) => {}
    }
    let start = trimmed.find('{').ok_or_else(|| {
        RuntimeError::new(
            "memory_distiller_invalid_json",
            RuntimeErrorCategory::Provider,
            "memory distiller output did not contain a JSON object",
        )
    })?;
    let end = trimmed.rfind('}').ok_or_else(|| {
        RuntimeError::new(
            "memory_distiller_invalid_json",
            RuntimeErrorCategory::Provider,
            "memory distiller output did not contain a complete JSON object",
        )
    })?;
    if end <= start {
        return Err(RuntimeError::new(
            "memory_distiller_invalid_json",
            RuntimeErrorCategory::Provider,
            "memory distiller output did not contain a complete JSON object",
        ));
    }
    let value = serde_json::from_str::<Value>(&trimmed[start..=end]).map_err(|error| {
        RuntimeError::new(
            "memory_distiller_invalid_json",
            RuntimeErrorCategory::Provider,
            format!("memory distiller returned invalid JSON: {error}"),
        )
    })?;
    if value.is_object() {
        Ok(value)
    } else {
        Err(RuntimeError::new(
            "memory_distiller_invalid_json",
            RuntimeErrorCategory::Provider,
            "memory distiller returned JSON that was not an object",
        ))
    }
}

fn memory_distiller_string_field(value: &Value, keys: &[&str], max_chars: usize) -> Option<String> {
    keys.iter().find_map(|key| {
        value
            .get(*key)
            .and_then(Value::as_str)
            .and_then(|text| memory_backend_optional_text(Some(text.to_string()), max_chars))
    })
}

fn memory_distiller_string_array(
    value: &Value,
    keys: &[&str],
    max_items: usize,
    max_chars: usize,
) -> Vec<String> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_array))
        .into_iter()
        .flat_map(|values| values.iter())
        .filter_map(|item| {
            item.as_str()
                .and_then(|text| memory_backend_optional_text(Some(text.to_string()), max_chars))
        })
        .take(max_items)
        .collect()
}

fn memory_distiller_recurring_candidates(value: &Value) -> Vec<MemoryRecurringTaskCandidate> {
    value
        .get("recurringTasks")
        .or_else(|| value.get("recurringTaskCandidates"))
        .and_then(Value::as_array)
        .into_iter()
        .flat_map(|items| items.iter())
        .filter_map(|item| {
            let task = memory_distiller_string_field(item, &["task", "title", "pattern"], 140)?;
            if task.chars().count() < RECURRING_TASK_MIN_TASK_CHARS {
                return None;
            }
            Some(MemoryRecurringTaskCandidate {
                task,
                rationale: memory_distiller_string_field(
                    item,
                    &["rationale", "reason", "why"],
                    220,
                ),
                prompt_hints: memory_distiller_string_array(
                    item,
                    &["promptHints", "prompts", "triggers"],
                    4,
                    180,
                ),
                outcomes: memory_distiller_string_array(
                    item,
                    &["outcomes", "outcomeHints", "results"],
                    4,
                    180,
                ),
                related_memory_ids: memory_distiller_string_array(
                    item,
                    &["relatedMemoryIds", "memoryIds", "sources"],
                    8,
                    80,
                ),
                tags: memory_distiller_string_array(item, &["tags"], 5, 40),
            })
        })
        .take(RECURRING_TASK_MAX_CANDIDATES)
        .collect()
}

pub(super) fn parse_memory_distiller_output(
    raw: &str,
) -> Result<DistilledTurnMemory, RuntimeError> {
    let value = memory_distiller_json_object(raw)?;
    Ok(DistilledTurnMemory {
        remember: value
            .get("remember")
            .and_then(Value::as_bool)
            .unwrap_or(true),
        request: memory_distiller_string_field(&value, &["request"], 220),
        completed: memory_distiller_string_array(&value, &["completed"], 3, 180),
        learned: memory_distiller_string_array(&value, &["learned"], 3, 180),
        decisions: memory_distiller_string_array(&value, &["decisions"], 2, 180),
        next: memory_distiller_string_array(&value, &["next"], 2, 160),
        files: memory_distiller_string_array(&value, &["files"], 6, 120),
        tags: memory_distiller_string_array(&value, &["tags"], 4, 40),
        recurring_tasks: memory_distiller_recurring_candidates(&value),
    })
}

fn append_distilled_turn_section(lines: &mut Vec<String>, heading: &str, items: &[String]) {
    if items.is_empty() {
        return;
    }
    lines.push(heading.to_string());
    for item in items {
        lines.push(format!("- {item}"));
    }
}

fn turn_memory_record_id(distilled: &DistilledTurnMemory) -> String {
    let seed = distilled
        .request
        .as_deref()
        .or_else(|| distilled.completed.first().map(String::as_str))
        .unwrap_or("distilled-turn");
    let mut slug = String::new();
    let mut previous_dash = true;
    for character in seed.to_ascii_lowercase().chars() {
        if character.is_ascii_alphanumeric() {
            slug.push(character);
            previous_dash = false;
        } else if !previous_dash {
            slug.push('-');
            previous_dash = true;
        }
        if slug.len() >= 72 {
            break;
        }
    }
    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        "turn-memory-distilled-turn".to_string()
    } else {
        format!("turn-memory-{slug}")
    }
}

fn memory_distilled_turn_content(
    distilled: &DistilledTurnMemory,
    source_session_id: Option<&str>,
    source_project: Option<&str>,
) -> Option<String> {
    if !distilled.remember {
        return None;
    }
    let mut lines = vec!["Turn memory".to_string()];
    if let Some(request) = &distilled.request {
        lines.push(format!("Request: {request}"));
    }
    append_distilled_turn_section(&mut lines, "Completed:", &distilled.completed);
    append_distilled_turn_section(&mut lines, "Learned:", &distilled.learned);
    append_distilled_turn_section(&mut lines, "Decisions:", &distilled.decisions);
    append_distilled_turn_section(&mut lines, "Next:", &distilled.next);
    if !distilled.files.is_empty() {
        lines.push(format!("Files: {}", distilled.files.join(", ")));
    }
    if let Some(source_project) = source_project {
        lines.push(format!("Source project: {source_project}"));
    }
    if let Some(source_session_id) = source_session_id {
        lines.push(format!("Source session: {source_session_id}"));
    }
    let content = lines.join("\n");
    (content.len() >= 80).then_some(content)
}

pub(super) fn memory_distilled_turn_record(
    distilled: &DistilledTurnMemory,
    source_session_id: Option<&str>,
    source_project: Option<&str>,
) -> Option<MemoryRecord> {
    let content = memory_distilled_turn_content(distilled, source_session_id, source_project)?;
    let mut seen = BTreeSet::new();
    let mut tags = Vec::new();
    for tag in ["oppi-turn-summary".to_string(), "agent_end".to_string()] {
        if seen.insert(tag.clone()) {
            tags.push(tag);
        }
    }
    for tag in distilled
        .tags
        .iter()
        .cloned()
        .filter_map(memory_recurring_tag)
    {
        if seen.insert(tag.clone()) {
            tags.push(tag);
        }
    }
    Some(MemoryRecord {
        id: turn_memory_record_id(distilled),
        content,
        tags,
        layer: Some("semantic".to_string()),
        confidence: Some("observed".to_string()),
        source: Some("oppi:turn-distillation".to_string()),
        ..MemoryRecord::default()
    })
}

pub(super) fn memory_distill_turn_message(remembered: bool, recurring_saved_count: u64) -> String {
    match (remembered, recurring_saved_count) {
        (true, 0) => "Distilled and recorded turn memory.".to_string(),
        (false, 0) => {
            "Distiller found no durable turn memory or recurring task candidates.".to_string()
        }
        (true, count) => format!(
            "Distilled turn memory and recorded {count} recurring task candidate{}.",
            if count == 1 { "" } else { "s" }
        ),
        (false, count) => format!(
            "Recorded {count} recurring task candidate{} from turn distillation.",
            if count == 1 { "" } else { "s" }
        ),
    }
}

pub(super) fn sanitize_memory_backend_remember_params(
    mut params: MemoryBackendRememberParams,
) -> Result<MemoryBackendRememberParams, RuntimeError> {
    params.content = memory_backend_required_text(&params.content, "content", 16_000)?;
    params.tags = memory_backend_optional_text_vec(params.tags, 32);
    params.source = memory_backend_optional_text(params.source, 512);
    params.source_session_id = memory_backend_optional_text(params.source_session_id, 512);
    params.layer = memory_backend_optional_text(params.layer, 64);
    Ok(params)
}

pub(super) fn sanitize_memory_backend_list_params(
    mut params: MemoryBackendListParams,
) -> Result<MemoryBackendListParams, RuntimeError> {
    params.query = memory_backend_optional_text(params.query, 512);
    params.limit = params.limit.map(|limit| limit.min(500));
    Ok(params)
}

pub(super) fn sanitize_memory_backend_patch(
    mut patch: MemoryRecordPatch,
) -> Result<MemoryRecordPatch, RuntimeError> {
    patch.content = memory_backend_optional_text(patch.content, 16_000);
    patch.tags = patch
        .tags
        .map(|tags| memory_backend_optional_text_vec(tags, 64));
    patch.confidence = memory_backend_optional_text(patch.confidence, 64);
    patch.source = memory_backend_optional_text(patch.source, 512);
    patch.superseded_by = memory_backend_optional_text(patch.superseded_by, 512);
    Ok(patch)
}

pub(super) fn sanitize_memory_backend_consolidate_params(
    mut params: MemoryBackendConsolidateParams,
) -> Result<MemoryBackendConsolidateParams, RuntimeError> {
    params.stage = memory_backend_optional_text(params.stage, 128);
    params.limit = params.limit.map(|limit| limit.min(500));
    params.budget = params.budget.map(|budget| budget.min(16_000));
    params.agent_model = memory_backend_optional_text(params.agent_model, 128);
    params.deep_model = memory_backend_optional_text(params.deep_model, 128);
    Ok(params)
}

pub(super) fn sanitize_memory_backend_sync_params(
    mut params: MemoryBackendSyncParams,
) -> Result<MemoryBackendSyncParams, RuntimeError> {
    params.repo_path = memory_backend_optional_text(params.repo_path, 4096);
    params.remote = memory_backend_optional_text(params.remote, 2048);
    params.conflict_mode = memory_backend_optional_text(params.conflict_mode, 128);
    params.passphrase = memory_backend_optional_text(params.passphrase, 4096);
    params.message = memory_backend_optional_text(params.message, 512);
    Ok(params)
}

fn memory_backend_string_field(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        value
            .get(*key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    })
}

fn memory_backend_u64_field(value: &Value, keys: &[&str]) -> Option<u64> {
    keys.iter().find_map(|key| {
        value.get(*key).and_then(|value| {
            value.as_u64().or_else(|| {
                value
                    .as_i64()
                    .and_then(|number| (number >= 0).then_some(number as u64))
            })
        })
    })
}

fn memory_backend_bool_field(value: &Value, keys: &[&str]) -> Option<bool> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_bool))
}

fn memory_backend_string_array_field(value: &Value, keys: &[&str]) -> Vec<String> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_array))
        .into_iter()
        .flat_map(|values| values.iter())
        .filter_map(|value| {
            value
                .as_str()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
        .collect()
}

fn memory_backend_status_candidate(value: &Value, current: &MemoryStatus) -> Option<MemoryStatus> {
    if let Ok(status) = serde_json::from_value::<MemoryStatus>(value.clone()) {
        return Some(status);
    }
    let mut status = current.clone();
    let mut touched = false;
    if let Some(enabled) = memory_backend_bool_field(value, &["enabled"]) {
        status.enabled = enabled;
        touched = true;
    }
    if let Some(initialized) = memory_backend_bool_field(value, &["initialized"]) {
        status.initialized = initialized;
        touched = true;
    }
    if let Some(backend) = memory_backend_string_field(value, &["backend"]) {
        status.backend = backend;
        touched = true;
    }
    if let Some(scope) = memory_backend_string_field(value, &["scope"]) {
        status.scope = scope;
        touched = true;
    }
    if let Some(store_path) = memory_backend_string_field(value, &["storePath", "store_path"]) {
        status.store_path = Some(store_path);
        touched = true;
    }
    if let Some(project_id) = memory_backend_string_field(value, &["projectId", "project_id"]) {
        status.project_id = Some(project_id);
        touched = true;
    }
    if let Some(memory_count) = memory_backend_u64_field(value, &["memoryCount", "memory_count"]) {
        status.memory_count = memory_count;
        touched = true;
    }
    if let Some(pinned_count) = memory_backend_u64_field(value, &["pinnedCount", "pinned_count"]) {
        status.pinned_count = pinned_count;
        touched = true;
    }
    if let Some(last_write_at) =
        memory_backend_string_field(value, &["lastWriteAt", "last_write_at"])
    {
        status.last_write_at = Some(last_write_at);
        touched = true;
    }
    if let Some(last_consolidated_at) =
        memory_backend_string_field(value, &["lastConsolidatedAt", "last_consolidated_at"])
    {
        status.last_consolidated_at = Some(last_consolidated_at);
        touched = true;
    }
    if let Some(queue_depth) = memory_backend_u64_field(value, &["queueDepth", "queue_depth"]) {
        status.queue_depth = Some(queue_depth);
        touched = true;
    }
    if let Some(dashboard_url) = memory_backend_string_field(value, &["dashboardUrl", "url"]) {
        status.dashboard_url = Some(dashboard_url);
        touched = true;
    }
    if let Some(execution_owner) =
        memory_backend_string_field(value, &["executionOwner", "execution_owner"])
    {
        status.execution_owner = Some(execution_owner);
        touched = true;
    }
    if let Some(execution_adapter) =
        memory_backend_string_field(value, &["executionAdapter", "execution_adapter"])
    {
        status.execution_adapter = Some(execution_adapter);
        touched = true;
    }
    if let Some(execution_adapter_kind) = memory_backend_string_field(
        value,
        &[
            "executionAdapterKind",
            "execution_adapter_kind",
            "adapterKind",
        ],
    ) {
        status.execution_adapter_kind = Some(execution_adapter_kind);
        touched = true;
    }
    if touched {
        if status.backend == "none" {
            status.backend = "hoppi".to_string();
        }
        if status.scope.is_empty() {
            status.scope = "project".to_string();
        }
        Some(status)
    } else {
        None
    }
}

fn annotate_memory_backend_status(
    mut status: MemoryStatus,
    raw: &MemoryBackendRawResult,
) -> MemoryStatus {
    if status
        .execution_owner
        .as_deref()
        .map(str::trim)
        .is_none_or(str::is_empty)
    {
        status.execution_owner = Some("runtime".to_string());
    }
    if status
        .execution_adapter
        .as_deref()
        .map(str::trim)
        .is_none_or(str::is_empty)
    {
        status.execution_adapter = Some(raw.backend.clone());
    }
    if status
        .execution_adapter_kind
        .as_deref()
        .map(str::trim)
        .is_none_or(str::is_empty)
    {
        status.execution_adapter_kind = raw.adapter_kind.clone();
    }
    status
}

pub(super) fn memory_backend_optional_status_from_raw(
    raw: &MemoryBackendRawResult,
    current: &MemoryStatus,
) -> Option<MemoryStatus> {
    raw.value
        .get("status")
        .and_then(|value| memory_backend_status_candidate(value, current))
        .or_else(|| memory_backend_status_candidate(&raw.value, current))
        .map(|status| annotate_memory_backend_status(status, raw))
}

pub(super) fn memory_backend_status_from_raw(
    raw: &MemoryBackendRawResult,
    current: &MemoryStatus,
) -> MemoryStatus {
    memory_backend_optional_status_from_raw(raw, current).unwrap_or_else(|| current.clone())
}

pub(super) fn memory_backend_records_from_raw(raw: &MemoryBackendRawResult) -> Vec<MemoryRecord> {
    raw.value
        .get("records")
        .or_else(|| raw.value.get("memories"))
        .or_else(|| raw.value.get("items"))
        .or_else(|| raw.value.as_array().map(|_| &raw.value))
        .cloned()
        .and_then(|value| serde_json::from_value::<Vec<MemoryRecord>>(value).ok())
        .unwrap_or_default()
}

pub(super) fn memory_backend_record_from_raw(raw: &MemoryBackendRawResult) -> Option<MemoryRecord> {
    raw.value
        .get("record")
        .or_else(|| raw.value.get("memory"))
        .or_else(|| {
            (raw.value.get("id").is_some() && raw.value.get("content").is_some())
                .then_some(&raw.value)
        })
        .cloned()
        .and_then(|value| serde_json::from_value::<MemoryRecord>(value).ok())
}

pub(super) fn memory_remember_params_from_backend_raw(
    params: &MemoryBackendRememberParams,
    raw: &MemoryBackendRawResult,
) -> MemoryRememberParams {
    MemoryRememberParams {
        thread_id: params.thread_id.clone(),
        content: params.content.clone(),
        tags: params.tags.clone(),
        layer: params.layer.clone(),
        confidence: memory_backend_string_field(&raw.value, &["confidence"]),
        source_session_id: params.source_session_id.clone(),
        backend_applied: memory_backend_bool_field(&raw.value, &["saved", "backendApplied"])
            .unwrap_or(true),
        record: memory_backend_record_from_raw(raw),
        status: memory_backend_optional_status_from_raw(raw, &MemoryStatus::default()),
        reason: memory_backend_string_field(&raw.value, &["reason"]),
        source: params.source.clone().or_else(|| Some(raw.backend.clone())),
    }
}

pub(super) fn memory_consolidate_params_from_backend_raw(
    params: &MemoryBackendConsolidateParams,
    raw: &MemoryBackendRawResult,
) -> MemoryConsolidateParams {
    MemoryConsolidateParams {
        thread_id: params.thread_id.clone(),
        stage: params.stage.clone(),
        summary: memory_backend_string_field(&raw.value, &["summary", "message"]),
        model_used: memory_backend_string_field(&raw.value, &["modelUsed", "model_used", "model"]),
        dry_run: memory_backend_bool_field(&raw.value, &["dryRun", "dry_run"])
            .unwrap_or(params.dry_run),
        backend_applied: memory_backend_bool_field(&raw.value, &["backendApplied"])
            .unwrap_or(!params.dry_run),
        records: memory_backend_records_from_raw(raw),
        deleted_ids: memory_backend_string_array_field(
            &raw.value,
            &["deletedIds", "deleted_ids", "deleted", "removedIds"],
        ),
        status: memory_backend_optional_status_from_raw(raw, &MemoryStatus::default()),
        source: Some(raw.backend.clone()),
    }
}

pub(super) fn memory_backend_sync_record_count(value: &Value) -> u64 {
    memory_backend_u64_field(value, &["imported"]).unwrap_or(0)
        + memory_backend_u64_field(value, &["exported"]).unwrap_or(0)
}

pub(super) fn memory_recall_params_from_backend_raw(
    thread_id: &str,
    prompt: &str,
    raw: &MemoryBackendRawResult,
) -> MemoryRecallParams {
    let records = raw
        .value
        .get("records")
        .or_else(|| raw.value.get("memories"))
        .cloned()
        .and_then(|value| serde_json::from_value::<Vec<MemoryRecord>>(value).ok())
        .unwrap_or_default();
    let context_markdown = raw
        .value
        .get("contextMarkdown")
        .or_else(|| raw.value.get("context"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let omitted_reason = raw
        .value
        .get("omittedReason")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let status = raw
        .value
        .get("status")
        .cloned()
        .and_then(|value| serde_json::from_value::<MemoryStatus>(value).ok());
    let truncated = raw
        .value
        .get("truncated")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    MemoryRecallParams {
        thread_id: thread_id.to_string(),
        query: Some(prompt.to_string()),
        context_markdown,
        omitted_reason,
        records,
        backend_queried: true,
        status,
        truncated,
        source: Some(raw.backend.clone()),
    }
}

pub(super) fn memory_record_matches_query(record: &MemoryRecord, query: &str) -> bool {
    let query = query.trim().to_ascii_lowercase();
    if query.is_empty() {
        return true;
    }
    record.id.to_ascii_lowercase().contains(&query)
        || record.content.to_ascii_lowercase().contains(&query)
        || record
            .tags
            .iter()
            .any(|tag| tag.to_ascii_lowercase().contains(&query))
        || record
            .source
            .as_deref()
            .is_some_and(|source| source.to_ascii_lowercase().contains(&query))
        || record
            .confidence
            .as_deref()
            .is_some_and(|confidence| confidence.to_ascii_lowercase().contains(&query))
}

pub(super) fn memory_record_patch_is_empty(patch: &MemoryRecordPatch) -> bool {
    patch.content.is_none()
        && patch.tags.is_none()
        && patch.pinned.is_none()
        && patch.starred.is_none()
        && patch.confidence.is_none()
        && patch.source.is_none()
        && patch.superseded_by.is_none()
}

pub(super) fn apply_memory_record_patch(record: &mut MemoryRecord, patch: &MemoryRecordPatch) {
    if let Some(content) = &patch.content {
        record.content = content.clone();
    }
    if let Some(tags) = &patch.tags {
        record.tags = tags.clone();
    }
    if let Some(pinned) = patch.pinned {
        record.pinned = pinned;
    }
    if let Some(starred) = patch.starred {
        record.starred = starred;
    }
    if let Some(confidence) = &patch.confidence {
        record.confidence = Some(confidence.clone());
    }
    if let Some(source) = &patch.source {
        record.source = Some(source.clone());
    }
    if let Some(superseded_by) = &patch.superseded_by {
        record.superseded_by = Some(superseded_by.clone());
    }
}

pub(super) fn already_exists(kind: &str, id: &str) -> RuntimeError {
    RuntimeError::new(
        format!("{}_already_exists", kind.replace(' ', "_")),
        RuntimeErrorCategory::AlreadyExists,
        format!("{kind} already exists: {id}"),
    )
}

pub(super) fn already_resolved(kind: &str, id: &str) -> RuntimeError {
    RuntimeError::new(
        format!("{}_already_resolved", kind.replace(' ', "_")),
        RuntimeErrorCategory::AlreadyResolved,
        format!("{kind} already resolved: {id}"),
    )
}

pub(super) fn agent_name_slug(name: &str) -> Result<String, RuntimeError> {
    let mut slug = String::new();
    let mut pending_dash = false;
    for character in name.trim().chars() {
        if character.is_ascii_alphanumeric() {
            if pending_dash && !slug.is_empty() {
                slug.push('-');
            }
            slug.push(character.to_ascii_lowercase());
            pending_dash = false;
        } else {
            pending_dash = true;
        }
    }

    if slug.is_empty() {
        return Err(RuntimeError::new(
            "invalid_agent_name",
            RuntimeErrorCategory::InvalidRequest,
            "agent name must contain at least one ASCII letter or digit",
        ));
    }

    Ok(slug)
}

pub(super) fn invalid_goal_request(message: String) -> RuntimeError {
    RuntimeError::new(
        "invalid_goal",
        RuntimeErrorCategory::InvalidRequest,
        message,
    )
}

pub(super) fn thread_goal_status_name(status: ThreadGoalStatus) -> &'static str {
    match status {
        ThreadGoalStatus::Active => "active",
        ThreadGoalStatus::Paused => "paused",
        ThreadGoalStatus::BudgetLimited => "budget-limited",
        ThreadGoalStatus::Complete => "complete",
    }
}

pub(super) fn turn_status_name(status: TurnStatus) -> &'static str {
    match status {
        TurnStatus::Queued => "queued",
        TurnStatus::Running => "running",
        TurnStatus::WaitingForApproval => "waitingForApproval",
        TurnStatus::WaitingForUser => "waitingForUser",
        TurnStatus::Completed => "completed",
        TurnStatus::Aborted => "aborted",
        TurnStatus::Failed => "failed",
    }
}

pub(super) fn turn_is_terminal(turn: &Turn) -> bool {
    matches!(
        turn.status,
        TurnStatus::Completed | TurnStatus::Aborted | TurnStatus::Failed
    )
}

pub(super) fn ensure_turn_mutable(turn: &Turn) -> Result<(), RuntimeError> {
    if turn_is_terminal(turn) {
        return Err(RuntimeError::new(
            "turn_not_mutable",
            RuntimeErrorCategory::TerminalState,
            format!("turn {} is terminal and cannot be mutated", turn.id),
        ));
    }
    Ok(())
}

pub(super) fn validate_tool_pairing(tools: &[SimulatedToolUse]) -> Result<(), RuntimeError> {
    let mut tracker = ToolPairingTracker::new();
    for simulated in tools {
        if simulated.result.call_id != simulated.call.id {
            return Err(RuntimeError::new(
                "tool_pair_mismatch",
                RuntimeErrorCategory::ToolPairing,
                format!(
                    "tool result {} does not match tool call {}",
                    simulated.result.call_id, simulated.call.id
                ),
            ));
        }
        tracker
            .record_call(simulated.call.clone())
            .map_err(tool_error)?;
    }
    for simulated in tools {
        tracker
            .record_result(simulated.result.clone())
            .map_err(tool_error)?;
    }
    tracker.finish().map_err(tool_error)
}

fn tool_error(error: oppi_tools::ToolPairingError) -> RuntimeError {
    RuntimeError::new(
        "tool_pairing_error",
        RuntimeErrorCategory::ToolPairing,
        format!("{error:?}"),
    )
}
