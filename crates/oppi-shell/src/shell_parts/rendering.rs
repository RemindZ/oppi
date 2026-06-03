use super::*;

pub(super) fn format_approval_panel(request: &ApprovalRequest) -> String {
    let mut lines = vec![
        "approval needed".to_string(),
        format!("  reason: {}", request.reason),
        format!("  risk: {:?}", request.risk),
    ];
    if let Some(call) = &request.tool_call {
        lines.push(format!(
            "  tool: {}:{} ({})",
            call.namespace.as_deref().unwrap_or("tool"),
            call.name,
            call.id
        ));
        lines.push(format!("  args: {}", compact_json(&call.arguments)));
    }
    lines.push("  action: /approve or /deny".to_string());
    lines.join("\n")
}

pub(super) fn format_ask_user_panel(request: &AskUserRequest) -> String {
    let mut lines = vec![format!(
        "question: {}",
        request.title.as_deref().unwrap_or("user input requested")
    )];
    lines.push(format!(
        "  timeout: {}",
        format_ask_user_timeout_label(request.timeout_ms)
    ));
    for question in &request.questions {
        lines.push(format!("  {}: {}", question.id, question.question));
        for option in &question.options {
            lines.push(format!("    - {}: {}", option.id, option.label));
        }
    }
    lines.push("  action: /answer <text-or-option-id>".to_string());
    lines.join("\n")
}

pub(super) fn ask_user_input_answers(request: &AskUserRequest, answer: &str) -> Vec<AskUserAnswer> {
    request
        .questions
        .iter()
        .map(|question| {
            if let Some(option) = question.options.iter().find(|option| option.id == answer) {
                AskUserAnswer {
                    question_id: question.id.clone(),
                    option_id: Some(option.id.clone()),
                    label: Some(option.label.clone()),
                    text: None,
                    skipped: false,
                }
            } else {
                AskUserAnswer {
                    question_id: question.id.clone(),
                    option_id: None,
                    label: None,
                    text: Some(answer.to_string()),
                    skipped: false,
                }
            }
        })
        .collect()
}

pub(super) fn ask_user_timeout_answers(request: &AskUserRequest) -> Vec<AskUserAnswer> {
    request
        .questions
        .iter()
        .map(|question| {
            let default_option = question
                .default_option_id
                .as_ref()
                .and_then(|default_id| {
                    question
                        .options
                        .iter()
                        .find(|option| option.id == *default_id)
                })
                .or_else(|| question.options.first());
            if let Some(option) = default_option {
                return AskUserAnswer {
                    question_id: question.id.clone(),
                    option_id: Some(option.id.clone()),
                    label: Some(option.label.clone()),
                    text: None,
                    skipped: false,
                };
            }
            if !question.required {
                return AskUserAnswer {
                    question_id: question.id.clone(),
                    option_id: None,
                    label: None,
                    text: None,
                    skipped: true,
                };
            }
            AskUserAnswer {
                question_id: question.id.clone(),
                option_id: None,
                label: None,
                text: Some("No recommended answer was available before timeout.".to_string()),
                skipped: false,
            }
        })
        .collect()
}

pub(super) fn ask_user_deadline_at(request: &AskUserRequest) -> Option<Instant> {
    if request.timeout_ms == 0 {
        return None;
    }
    Instant::now().checked_add(Duration::from_millis(request.timeout_ms))
}

fn compact_json(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string())
}

pub(super) fn is_background_latest_alias(value: &str) -> bool {
    matches!(value.to_ascii_lowercase().as_str(), "latest" | "last" | ".")
}

pub(super) fn current_time_ms() -> Option<u64> {
    Some(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()?
            .as_millis()
            .min(u128::from(u64::MAX)) as u64,
    )
}

fn json_u64(value: &Value, key: &str) -> Option<u64> {
    value.get(key).and_then(|value| {
        value
            .as_u64()
            .or_else(|| value.as_i64().and_then(|n| n.try_into().ok()))
    })
}

fn task_started_at_ms(task: &Value) -> Option<u64> {
    json_u64(task, "startedAtMs")
}

fn task_output_bytes(task: &Value) -> Option<u64> {
    json_u64(task, "outputBytes")
}

pub(super) fn select_background_task_id(value: &Value, running_only: bool) -> Option<String> {
    let items = value.get("items")?.as_array()?;
    items
        .iter()
        .filter(|item| {
            !running_only
                || item
                    .get("status")
                    .and_then(Value::as_str)
                    .is_some_and(|status| status.eq_ignore_ascii_case("running"))
        })
        .filter_map(|item| {
            Some((
                task_started_at_ms(item).unwrap_or_default(),
                item.get("id")?.as_str()?.to_string(),
            ))
        })
        .max_by(|(left_started, left_id), (right_started, right_id)| {
            left_started
                .cmp(right_started)
                .then_with(|| left_id.cmp(right_id))
        })
        .map(|(_, id)| id)
}

pub(super) fn format_duration_ms(ms: u64) -> String {
    if ms < 1_000 {
        format!("{ms}ms")
    } else if ms < 60_000 {
        format!("{}s", ms / 1_000)
    } else {
        format!("{}m{}s", ms / 60_000, (ms % 60_000) / 1_000)
    }
}

fn format_task_elapsed(task: &Value) -> Option<String> {
    let started = task_started_at_ms(task)?;
    let status = task.get("status").and_then(Value::as_str).unwrap_or("");
    if status.eq_ignore_ascii_case("running") {
        let now = current_time_ms()?;
        return now
            .checked_sub(started)
            .map(|ms| format!("for {}", format_duration_ms(ms)));
    }
    let finished = json_u64(task, "finishedAtMs")?;
    finished
        .checked_sub(started)
        .map(|ms| format!("ran {}", format_duration_ms(ms)))
}

pub(super) fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes}B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1}KiB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1}MiB", bytes as f64 / (1024.0 * 1024.0))
    }
}

fn shorten_command(command: &str) -> String {
    const MAX: usize = 96;
    if command.chars().count() <= MAX {
        return command.to_string();
    }
    let head = command
        .chars()
        .take(MAX.saturating_sub(1))
        .collect::<String>();
    format!("{head}…")
}

fn format_background_meta(task: &Value) -> String {
    let mut meta = vec![
        task.get("status")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string(),
    ];
    if let Some(elapsed) = format_task_elapsed(task) {
        meta.push(elapsed);
    }
    if let Some(code) = task.get("exitCode").and_then(Value::as_i64) {
        meta.push(format!("exit {code}"));
    }
    if let Some(bytes) = task_output_bytes(task) {
        meta.push(format_bytes(bytes));
    }
    meta.join(", ")
}

fn format_background_counts(items: &[Value]) -> String {
    let mut counts = BTreeMap::<String, usize>::new();
    for item in items {
        let status = item
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_ascii_lowercase();
        *counts.entry(status).or_default() += 1;
    }
    counts
        .into_iter()
        .map(|(status, count)| format!("{count} {status}"))
        .collect::<Vec<_>>()
        .join(", ")
}

pub(super) fn background_summary_from_list(value: &Value) -> String {
    let Some(items) = value.get("items").and_then(Value::as_array) else {
        return "status unavailable".to_string();
    };
    if items.is_empty() {
        return "no process-local tasks".to_string();
    }
    let latest = select_background_task_id(value, false).unwrap_or_else(|| "latest".to_string());
    format!("{}; latest={latest}", format_background_counts(items))
}

pub(super) fn background_summary_from_read(value: &Value) -> String {
    let task = value.get("task").unwrap_or(&Value::Null);
    let id = task.get("id").and_then(Value::as_str).unwrap_or("task");
    let status = task
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let bytes = value
        .get("outputBytes")
        .and_then(Value::as_u64)
        .or_else(|| task.get("outputBytes").and_then(Value::as_u64))
        .unwrap_or(0);
    format!("read {id} ({status}, {})", format_bytes(bytes))
}

pub(super) fn background_summary_from_kill(value: &Value) -> String {
    let task = value.get("task").unwrap_or(&Value::Null);
    let id = task.get("id").and_then(Value::as_str).unwrap_or("task");
    let status = task
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    format!("kill requested for {id} ({status})")
}

pub(super) fn format_background_list(value: &Value) -> String {
    let Some(items) = value.get("items").and_then(Value::as_array) else {
        return format!("background: {}", compact_json(value));
    };
    if items.is_empty() {
        return "background: none (process-local; tasks reset when the server restarts)"
            .to_string();
    }
    let mut sorted = items.iter().collect::<Vec<_>>();
    sorted.sort_by(|left, right| {
        task_started_at_ms(right)
            .cmp(&task_started_at_ms(left))
            .then_with(|| {
                left.get("id")
                    .and_then(Value::as_str)
                    .cmp(&right.get("id").and_then(Value::as_str))
            })
    });
    let mut lines = vec![format!(
        "background: {} task(s) ({})",
        items.len(),
        format_background_counts(items)
    )];
    for item in sorted {
        let id = item.get("id").and_then(Value::as_str).unwrap_or("unknown");
        let command = item.get("command").and_then(Value::as_str).unwrap_or("");
        lines.push(format!(
            "  {id} [{}] {}",
            format_background_meta(item),
            shorten_command(command)
        ));
        let cwd = item.get("cwd").and_then(Value::as_str).unwrap_or("");
        let log = item.get("outputPath").and_then(Value::as_str).unwrap_or("");
        if !cwd.is_empty() || !log.is_empty() {
            lines.push(format!("    cwd: {cwd}  log: {log}"));
        }
    }
    lines.push(
        "actions: /background read [latest|id] [bytes], /background kill [latest|id]".to_string(),
    );
    lines.join("\n")
}

pub(super) fn format_background_read(value: &Value) -> String {
    let task = value.get("task").unwrap_or(&Value::Null);
    let id = task.get("id").and_then(Value::as_str).unwrap_or("unknown");
    let output = value.get("output").and_then(Value::as_str).unwrap_or("");
    let output_bytes = value
        .get("outputBytes")
        .and_then(Value::as_u64)
        .or_else(|| task_output_bytes(task));
    let max_bytes = json_u64(value, "maxBytes");
    let mut lines = vec![format!(
        "background {id} [{}]",
        format_background_meta(task)
    )];
    if let Some(log) = task.get("outputPath").and_then(Value::as_str)
        && !log.is_empty()
    {
        lines.push(format!("log: {log}"));
    }
    if output.is_empty() {
        lines.push("output: no output yet".to_string());
    } else {
        let tail_note = if value
            .get("truncated")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            match (max_bytes, output_bytes) {
                (Some(max), Some(total)) => format!(
                    "tail: last {} of {}",
                    format_bytes(max),
                    format_bytes(total)
                ),
                _ => "tail: truncated".to_string(),
            }
        } else {
            match output_bytes {
                Some(total) => format!("output: {}", format_bytes(total)),
                None => "output:".to_string(),
            }
        };
        lines.push(tail_note);
        lines.push(trim_output_tail(output, 12_000));
    }
    lines.join("\n")
}

pub(super) fn format_background_kill(value: &Value) -> String {
    let task = value.get("task").unwrap_or(&Value::Null);
    let mut lines = vec![format!(
        "background {} [{}]: {}",
        task.get("id").and_then(Value::as_str).unwrap_or("unknown"),
        format_background_meta(task),
        value
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("updated")
    )];
    if let Some(log) = task.get("outputPath").and_then(Value::as_str)
        && !log.is_empty()
    {
        lines.push(format!("log: {log}"));
    }
    lines.join("\n")
}

fn trim_output_tail(output: &str, max_chars: usize) -> String {
    if output.chars().count() <= max_chars {
        return output.to_string();
    }
    let tail = output
        .chars()
        .rev()
        .take(max_chars)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<String>();
    format!("…\n{tail}")
}

#[cfg(test)]
pub(super) fn render_event_line(event: &Event) -> Option<String> {
    render_event_line_themed(event, "plain", &BTreeMap::new())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ThemeTokens {
    pub(super) accent: &'static str,
    pub(super) dim: &'static str,
    pub(super) code: &'static str,
    pub(super) success: &'static str,
    pub(super) warning: &'static str,
    pub(super) error: &'static str,
    pub(super) reset: &'static str,
}

pub(super) fn plain_theme_tokens() -> ThemeTokens {
    ThemeTokens {
        accent: "",
        dim: "",
        code: "",
        success: "",
        warning: "",
        error: "",
        reset: "",
    }
}

pub(super) fn theme_tokens(name: &str) -> ThemeTokens {
    if !ansi_enabled() || name == "plain" {
        return plain_theme_tokens();
    }
    ansi_theme_tokens(name)
}

pub(super) fn ansi_theme_tokens(name: &str) -> ThemeTokens {
    match name {
        "light" => ThemeTokens {
            accent: "\x1b[34;1m",
            dim: "\x1b[2m",
            code: "\x1b[35m",
            success: "\x1b[32m",
            warning: "\x1b[33m",
            error: "\x1b[31m",
            reset: "\x1b[0m",
        },
        "dark" => ThemeTokens {
            accent: "\x1b[36;1m",
            dim: "\x1b[2m",
            code: "\x1b[35m",
            success: "\x1b[32m",
            warning: "\x1b[33m",
            error: "\x1b[31m",
            reset: "\x1b[0m",
        },
        _ => ThemeTokens {
            accent: "\x1b[38;5;81;1m",
            dim: "\x1b[2m",
            code: "\x1b[38;5;141m",
            success: "\x1b[38;5;114m",
            warning: "\x1b[38;5;221m",
            error: "\x1b[38;5;203m",
            reset: "\x1b[0m",
        },
    }
}

pub(super) fn line_theme_tokens_from_pi_theme_values(
    colors: &Value,
) -> Result<ThemeTokens, String> {
    let base = ansi_theme_tokens("oppi");
    Ok(ThemeTokens {
        accent: line_theme_color(colors, "accent", base.accent)?,
        dim: line_theme_color(colors, "dim", base.dim)?,
        code: line_theme_color_any(colors, &["mdCode", "syntaxString"], base.code)?,
        success: line_theme_color(colors, "success", base.success)?,
        warning: line_theme_color(colors, "warning", base.warning)?,
        error: line_theme_color(colors, "error", base.error)?,
        reset: base.reset,
    })
}

fn line_theme_color(
    colors: &Value,
    key: &str,
    fallback: &'static str,
) -> Result<&'static str, String> {
    line_theme_color_any(colors, &[key], fallback)
}

fn line_theme_color_any(
    colors: &Value,
    keys: &[&str],
    fallback: &'static str,
) -> Result<&'static str, String> {
    for key in keys {
        if let Some(value) = colors.get(*key) {
            return ansi_escape_for_pi_color(value, *key, fallback);
        }
    }
    Ok(fallback)
}

fn ansi_escape_for_pi_color(
    value: &Value,
    key: &str,
    fallback: &'static str,
) -> Result<&'static str, String> {
    if let Some(index) = value.as_u64() {
        let index = u8::try_from(index)
            .map_err(|_| format!("theme token {key} ANSI index is outside 0-255"))?;
        return Ok(leak_ansi_escape(format!("\x1b[38;5;{index}m")));
    }
    let Some(raw) = value.as_str() else {
        return Err(format!("theme token {key} must be a string or ANSI index"));
    };
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok(fallback);
    }
    if let Some((red, green, blue)) = parse_hex_rgb(raw) {
        return Ok(leak_ansi_escape(format!("\x1b[38;2;{red};{green};{blue}m")));
    }
    if let Ok(index) = raw.parse::<u8>() {
        return Ok(leak_ansi_escape(format!("\x1b[38;5;{index}m")));
    }
    Err(format!(
        "theme token {key} has unsupported color value: {raw}"
    ))
}

fn parse_hex_rgb(raw: &str) -> Option<(u8, u8, u8)> {
    let hex = raw.strip_prefix('#')?;
    match hex.len() {
        3 => {
            let mut chars = hex.chars();
            let red = chars.next()?.to_digit(16)? as u8;
            let green = chars.next()?.to_digit(16)? as u8;
            let blue = chars.next()?.to_digit(16)? as u8;
            Some((red * 17, green * 17, blue * 17))
        }
        6 => {
            let red = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let green = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let blue = u8::from_str_radix(&hex[4..6], 16).ok()?;
            Some((red, green, blue))
        }
        _ => None,
    }
}

fn leak_ansi_escape(value: String) -> &'static str {
    Box::leak(value.into_boxed_str())
}

pub(super) fn ansi_enabled() -> bool {
    env::var_os("NO_COLOR").is_none() && env::var("TERM").map(|term| term != "dumb").unwrap_or(true)
}

pub(super) fn paint(tokens: ThemeTokens, color: &'static str, text: impl AsRef<str>) -> String {
    let text = text.as_ref();
    if color.is_empty() {
        text.to_string()
    } else {
        format!("{color}{text}{}", tokens.reset)
    }
}

#[cfg(test)]
fn render_event_line_themed(
    event: &Event,
    theme: &str,
    calls: &BTreeMap<String, ToolCall>,
) -> Option<String> {
    let tokens = theme_tokens(theme);
    render_event_line_with_tokens(event, tokens, calls)
}

pub(super) fn render_event_line_with_tokens(
    event: &Event,
    tokens: ThemeTokens,
    calls: &BTreeMap<String, ToolCall>,
) -> Option<String> {
    match &event.kind {
        EventKind::ThreadStarted { thread } => Some(format!(
            "thread {} started",
            paint(tokens, tokens.accent, &thread.id)
        )),
        EventKind::ThreadResumed { thread } => Some(format!(
            "thread {} resumed",
            paint(tokens, tokens.accent, &thread.id)
        )),
        EventKind::ThreadForked {
            thread,
            from_thread_id,
        } => Some(format!(
            "thread {} forked from {from_thread_id}",
            paint(tokens, tokens.accent, &thread.id)
        )),
        EventKind::ThreadUpdated { thread } => Some(format!(
            "thread {} updated ({:?})",
            paint(tokens, tokens.accent, &thread.id),
            thread.status
        )),
        EventKind::ThreadGoalUpdated { goal } => Some(format!(
            "{}: {}",
            format_goal_status_label(Some(goal)),
            goal.objective
        )),
        EventKind::ThreadGoalCleared { .. } => Some("goal cleared".to_string()),
        EventKind::TurnStarted { turn } => Some(format!(
            "turn {} started",
            paint(tokens, tokens.dim, &turn.id)
        )),
        EventKind::TurnPhaseChanged { .. } => None,
        EventKind::ItemCompleted { item } => match &item.kind {
            oppi_protocol::ItemKind::UserMessage { text } => Some(format!("user: {text}")),
            oppi_protocol::ItemKind::AssistantMessage { text } => Some(format!(
                "assistant:\n{}",
                render_markdown_with_tokens(text, tokens)
            )),
            _ => None,
        },
        EventKind::ItemDelta { .. } => None,
        EventKind::ToolCallStarted { call } => Some(format_tool_call_started(call, tokens)),
        EventKind::ToolCallCompleted { result } => Some(format_tool_result_digest(
            result,
            calls.get(&result.call_id),
            tokens,
        )),
        EventKind::ArtifactCreated { artifact } => Some(format_artifact_created(artifact, tokens)),
        EventKind::AgentStarted { run } => Some(format_agent_started(run, tokens)),
        EventKind::AgentBlocked { run_id, reason } => Some(format!(
            "agent {} blocked: {}",
            paint(tokens, tokens.warning, run_id),
            paint(tokens, tokens.warning, reason)
        )),
        EventKind::AgentCompleted { run_id, output } => Some(format!(
            "agent {} completed: {}",
            paint(tokens, tokens.success, run_id),
            render_markdown(output, "plain")
        )),
        EventKind::ApprovalRequested { request } => Some(format_approval_panel(request)),
        EventKind::PermissionReviewStarted { record }
        | EventKind::PermissionReviewCompleted { record } => {
            Some(format_permission_review_line(record, tokens))
        }
        EventKind::AskUserRequested { request } => Some(format_ask_user_panel(request)),
        EventKind::Diagnostic { .. } => None,
        EventKind::TurnCompleted { .. } => None,
        EventKind::TurnAborted { reason } => Some(format!(
            "turn aborted: {}",
            paint(tokens, tokens.error, reason)
        )),
        EventKind::TurnInterrupted { reason } => Some(format!(
            "turn interrupted: {}",
            paint(tokens, tokens.warning, reason)
        )),
        _ => None,
    }
}

fn format_permission_review_line(
    record: &oppi_protocol::GuardianReviewRecord,
    tokens: ThemeTokens,
) -> String {
    let status = guardian_review_status_label(record.status);
    let color = match record.status {
        oppi_protocol::GuardianReviewStatus::Approved
        | oppi_protocol::GuardianReviewStatus::Cached => tokens.success,
        oppi_protocol::GuardianReviewStatus::Denied
        | oppi_protocol::GuardianReviewStatus::FailedClosed
        | oppi_protocol::GuardianReviewStatus::CircuitBlocked => tokens.warning,
        oppi_protocol::GuardianReviewStatus::Reviewing => tokens.dim,
    };
    let mut details = vec![
        format!("risk={}", guardian_review_risk_label(record)),
        format!("auth={}", guardian_review_auth_label(record)),
    ];
    if let Some(scope) = record
        .decision
        .as_ref()
        .and_then(|decision| decision.cache_scope)
    {
        details.push(format!("cache={}", guardian_cache_scope_label(scope)));
    }
    format!(
        "guardian review {}: {} {}",
        paint(tokens, color, status),
        paint(tokens, tokens.code, record.tool_name.trim()),
        paint(tokens, tokens.dim, details.join(" "))
    )
}

fn guardian_review_status_label(status: oppi_protocol::GuardianReviewStatus) -> &'static str {
    match status {
        oppi_protocol::GuardianReviewStatus::Reviewing => "reviewing",
        oppi_protocol::GuardianReviewStatus::Approved => "auto-approved",
        oppi_protocol::GuardianReviewStatus::Denied => "denied",
        oppi_protocol::GuardianReviewStatus::FailedClosed => "fail-closed",
        oppi_protocol::GuardianReviewStatus::Cached => "cached",
        oppi_protocol::GuardianReviewStatus::CircuitBlocked => "circuit-blocked",
    }
}

fn guardian_review_risk_label(record: &oppi_protocol::GuardianReviewRecord) -> String {
    record
        .decision
        .as_ref()
        .map(|decision| risk_level_label(decision.risk_level).to_string())
        .unwrap_or_else(|| {
            let category = record.risk.category.trim();
            if category.is_empty() {
                "unknown".to_string()
            } else {
                category.to_string()
            }
        })
}

fn risk_level_label(level: oppi_protocol::RiskLevel) -> &'static str {
    match level {
        oppi_protocol::RiskLevel::Low => "low",
        oppi_protocol::RiskLevel::Medium => "medium",
        oppi_protocol::RiskLevel::High => "high",
        oppi_protocol::RiskLevel::Critical => "critical",
    }
}

fn guardian_review_auth_label(record: &oppi_protocol::GuardianReviewRecord) -> &'static str {
    record
        .decision
        .as_ref()
        .map(|decision| guardian_authorization_label(decision.user_authorization))
        .unwrap_or("unknown")
}

fn guardian_authorization_label(auth: oppi_protocol::GuardianAuthorizationLevel) -> &'static str {
    match auth {
        oppi_protocol::GuardianAuthorizationLevel::Unknown => "unknown",
        oppi_protocol::GuardianAuthorizationLevel::Low => "low",
        oppi_protocol::GuardianAuthorizationLevel::Medium => "medium",
        oppi_protocol::GuardianAuthorizationLevel::High => "high",
    }
}

fn guardian_cache_scope_label(scope: oppi_protocol::GuardianCacheScope) -> &'static str {
    match scope {
        oppi_protocol::GuardianCacheScope::None => "none",
        oppi_protocol::GuardianCacheScope::Exact => "exact",
    }
}

pub(super) fn render_markdown(text: &str, theme: &str) -> String {
    let tokens = theme_tokens(theme);
    render_markdown_with_tokens(text, tokens)
}

fn render_markdown_with_tokens(text: &str, tokens: ThemeTokens) -> String {
    let mut in_code = false;
    let mut lines = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            in_code = !in_code;
            lines.push(paint(tokens, tokens.dim, line));
        } else if in_code {
            let color = if trimmed.starts_with('+') {
                tokens.success
            } else if trimmed.starts_with('-') {
                tokens.error
            } else if trimmed.starts_with('@') {
                tokens.warning
            } else {
                tokens.code
            };
            lines.push(paint(tokens, color, line));
        } else if trimmed.starts_with('#') {
            lines.push(paint(tokens, tokens.accent, line));
        } else if trimmed.starts_with('>') {
            lines.push(paint(tokens, tokens.dim, line));
        } else if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
            lines.push(format!("  • {}", trimmed[2..].trim_start()));
        } else {
            lines.push(line.to_string());
        }
    }
    if lines.is_empty() {
        text.to_string()
    } else {
        lines.join("\n")
    }
}

fn format_tool_call_started(call: &ToolCall, tokens: ThemeTokens) -> String {
    let namespace = call.namespace.as_deref().unwrap_or("tool");
    format!(
        "tool {} started {}",
        paint(tokens, tokens.accent, format!("{namespace}:{}", call.name)),
        paint(tokens, tokens.dim, format!("({})", call.id))
    )
}

pub(super) fn format_tool_result_digest(
    result: &oppi_protocol::ToolResult,
    call: Option<&ToolCall>,
    tokens: ThemeTokens,
) -> String {
    if let Some(line) = format_running_agent_tool_result(result, call) {
        return line;
    }
    let status = match result.status {
        oppi_protocol::ToolResultStatus::Ok => paint(tokens, tokens.success, "ok"),
        oppi_protocol::ToolResultStatus::Denied => paint(tokens, tokens.warning, "denied"),
        oppi_protocol::ToolResultStatus::Error => paint(tokens, tokens.error, "error"),
        oppi_protocol::ToolResultStatus::Aborted => paint(tokens, tokens.warning, "aborted"),
    };
    let tool = call
        .map(|call| {
            format!(
                "{}:{}",
                call.namespace.as_deref().unwrap_or("tool"),
                call.name
            )
        })
        .unwrap_or_else(|| result.call_id.clone());
    let mut line = format!("tool {tool} → {status}");
    if let Some(output) = result
        .output
        .as_deref()
        .filter(|output| !output.trim().is_empty())
    {
        let preview = trim_output_tail(output.trim(), 600);
        line.push_str(&format!("\n{}", paint(tokens, tokens.dim, preview)));
        if let Some(artifact) = artifact_hint(output) {
            line.push_str(&format!(
                "\nartifact: {}",
                paint(tokens, tokens.accent, artifact)
            ));
        }
    }
    if let Some(error) = result.error.as_deref() {
        line.push_str(&format!("\n{}", paint(tokens, tokens.error, error)));
    }
    line
}

fn format_running_agent_tool_result(
    result: &oppi_protocol::ToolResult,
    call: Option<&ToolCall>,
) -> Option<String> {
    if !matches!(result.status, oppi_protocol::ToolResultStatus::Ok) {
        return None;
    }
    let call = call?;
    if call.name != "AgentTool" {
        return None;
    }
    let value: Value = serde_json::from_str(result.output.as_deref()?.trim()).ok()?;
    if value.get("status").and_then(Value::as_str) != Some("running") {
        return None;
    }
    let agent_name = value
        .get("agentName")
        .and_then(Value::as_str)
        .or_else(|| call.arguments.get("agent").and_then(Value::as_str))?;
    let run_id = value.get("runId").and_then(Value::as_str)?;
    let execution_root = value.get("executionRoot").and_then(Value::as_str)?;
    Some(format!(
        "agent {agent_name} running as {run_id} in {execution_root}"
    ))
}

fn format_agent_started(run: &AgentRun, tokens: ThemeTokens) -> String {
    let mut meta = Vec::new();
    if run.background {
        meta.push("background=true".to_string());
    }
    if let Some(role) = run.role.as_deref() {
        meta.push(format!("role={role}"));
    }
    if let Some(model) = run.model.as_deref() {
        meta.push(format!("model={model}"));
    }
    if let Some(effort) = run.effort.as_deref() {
        meta.push(format!("effort={effort}"));
    }
    if let Some(mode) = run.permission_mode {
        meta.push(format!("permissions={}", mode.as_str()));
    }
    if let Some(memory) = run.memory_mode.as_deref() {
        meta.push(format!("memory={memory}"));
    }
    if !run.tool_allowlist.is_empty() {
        meta.push(format!("tools={}", run.tool_allowlist.join(",")));
    }
    if !run.tool_denylist.is_empty() {
        meta.push(format!("deny={}", run.tool_denylist.join(",")));
    }
    if let Some(max_turns) = run.max_turns {
        meta.push(format!("maxTurns={max_turns}"));
    }
    let suffix = if meta.is_empty() {
        String::new()
    } else {
        format!(" ({})", meta.join("; "))
    };
    format!(
        "agent {} started {}: {}{}",
        paint(tokens, tokens.accent, &run.id),
        paint(tokens, tokens.accent, &run.agent_name),
        run.task,
        suffix
    )
}

pub(super) fn format_agent_poll_result(result: &AgentRunPollResult) -> String {
    let run = &result.run;
    let execution_root = run
        .execution_root
        .as_deref()
        .unwrap_or("unknown execution root");
    match run.status {
        AgentRunStatus::Running => {
            format!(
                "agent {} running as {} in {}",
                run.agent_name, run.id, execution_root
            )
        }
        AgentRunStatus::Blocked => {
            format!(
                "agent {} blocked as {} in {}",
                run.agent_name, run.id, execution_root
            )
        }
        AgentRunStatus::Completed => {
            let suffix = result
                .output
                .as_deref()
                .filter(|output| !output.trim().is_empty())
                .map(|output| format!(": {}", trim_output_tail(output.trim(), 600)))
                .unwrap_or_default();
            format!(
                "agent {} completed as {} in {}{}",
                run.agent_name, run.id, execution_root, suffix
            )
        }
        AgentRunStatus::Failed => {
            format!(
                "agent {} failed as {} in {}",
                run.agent_name, run.id, execution_root
            )
        }
    }
}

pub(super) fn format_side_question_result(value: &Value) -> Result<String, String> {
    let side_thread_id = value
        .get("sideThreadId")
        .and_then(Value::as_str)
        .ok_or_else(|| "side-question/ask returned no sideThreadId".to_string())?;
    let answer = value
        .get("answer")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if answer.is_empty() {
        Ok(format!("btw {side_thread_id}"))
    } else {
        Ok(format!("btw {side_thread_id}\n{answer}"))
    }
}

pub(super) fn format_artifact_created(artifact: &ArtifactMetadata, tokens: ThemeTokens) -> String {
    format_artifact_created_inner(artifact, tokens, None, None)
}

pub(super) fn format_artifact_created_with_image_settings(
    artifact: &ArtifactMetadata,
    tokens: ThemeTokens,
    settings: &ImageSettings,
) -> String {
    format_artifact_created_with_image_settings_and_protocol(
        artifact,
        tokens,
        settings,
        Some(TerminalImageProtocol::KittyLocalFile),
    )
}

pub(super) fn format_artifact_created_with_image_settings_and_protocol(
    artifact: &ArtifactMetadata,
    tokens: ThemeTokens,
    settings: &ImageSettings,
    protocol: Option<TerminalImageProtocol>,
) -> String {
    format_artifact_created_inner(artifact, tokens, Some(settings), protocol)
}

fn format_artifact_created_inner(
    artifact: &ArtifactMetadata,
    tokens: ThemeTokens,
    image_settings: Option<&ImageSettings>,
    image_protocol: Option<TerminalImageProtocol>,
) -> String {
    let mut line = format!(
        "artifact {} created: {}",
        paint(tokens, tokens.accent, &artifact.id),
        paint(tokens, tokens.accent, &artifact.output_path)
    );
    let mut meta = Vec::new();
    if let Some(mime) = artifact.mime_type.as_deref() {
        meta.push(mime.to_string());
    }
    if let (Some(width), Some(height)) = (artifact.width, artifact.height) {
        meta.push(format!("{width}x{height}"));
    }
    if let Some(bytes) = artifact.bytes {
        meta.push(format_bytes(bytes));
    }
    if let Some(backend) = artifact.backend.as_deref() {
        meta.push(format!("backend={backend}"));
    }
    if let Some(model) = artifact.model.as_deref() {
        meta.push(format!("model={model}"));
    }
    if !meta.is_empty() {
        line.push_str(&format!(" ({})", meta.join(", ")));
    }
    if !artifact.source_images.is_empty() {
        line.push_str(&format!("\nsources: {}", artifact.source_images.join(", ")));
    }
    if let Some(mask) = artifact.mask.as_deref() {
        line.push_str(&format!("\nmask: {mask}"));
    }
    if let Some(settings) = image_settings {
        if is_image_artifact(artifact) {
            if settings.show_images {
                if let Some(protocol) = image_protocol {
                    if let Some(sequence) = terminal_image_preview_sequence(
                        artifact,
                        settings.image_width_cells,
                        protocol,
                    ) {
                        line.push_str("\nimage preview:");
                        line.push('\n');
                        line.push_str(&sequence);
                    } else {
                        line.push_str(
                            "\nimage preview: unsupported image format for native inline preview",
                        );
                    }
                } else {
                    line.push_str(
                        "\nimage preview: terminal does not advertise native inline image support",
                    );
                }
            } else {
                line.push_str("\nEnable terminal images to preview inline.");
            }
        }
    }
    line
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TerminalImageProtocol {
    KittyLocalFile,
    Iterm2InlineFile,
    Sixel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct TerminalImageProtocolDetection {
    pub(super) protocol: Option<TerminalImageProtocol>,
    pub(super) source: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TerminalSixelDetection {
    detected: bool,
    source: Option<String>,
    status: &'static str,
}

const PREVIEW_IMAGE_FORMATS: [&str; 5] = ["png", "jpeg", "jpg", "webp", "gif"];
const IMAGE_PREVIEW_PROTOCOL_ENV: &str = "OPPI_IMAGE_PREVIEW_PROTOCOL";

fn terminal_image_protocol_from_env() -> Option<TerminalImageProtocol> {
    terminal_image_protocol_detection_from_env().protocol
}

fn terminal_image_protocol_detection_from_env() -> TerminalImageProtocolDetection {
    let override_value = env::var(IMAGE_PREVIEW_PROTOCOL_ENV).ok();
    let term = env::var("TERM").ok();
    let term_program = env::var("TERM_PROGRAM").ok();
    let kitty_window_id = env::var("KITTY_WINDOW_ID").ok();
    let wezterm_executable = env::var("WEZTERM_EXECUTABLE").ok();
    terminal_image_protocol_detection_from_env_values_with_override(
        override_value.as_deref(),
        term.as_deref(),
        term_program.as_deref(),
        kitty_window_id.as_deref(),
        wezterm_executable.as_deref(),
    )
}

pub(super) fn terminal_image_protocol_from_env_values(
    term: Option<&str>,
    term_program: Option<&str>,
    kitty_window_id: Option<&str>,
    wezterm_executable: Option<&str>,
) -> Option<TerminalImageProtocol> {
    terminal_image_protocol_detection_from_env_values_with_override(
        None,
        term,
        term_program,
        kitty_window_id,
        wezterm_executable,
    )
    .protocol
}

pub(super) fn terminal_image_protocol_from_env_values_with_override(
    override_value: Option<&str>,
    term: Option<&str>,
    term_program: Option<&str>,
    kitty_window_id: Option<&str>,
    wezterm_executable: Option<&str>,
) -> Option<TerminalImageProtocol> {
    terminal_image_protocol_detection_from_env_values_with_override(
        override_value,
        term,
        term_program,
        kitty_window_id,
        wezterm_executable,
    )
    .protocol
}

fn terminal_image_protocol_detection_from_env_values(
    term: Option<&str>,
    term_program: Option<&str>,
    kitty_window_id: Option<&str>,
    wezterm_executable: Option<&str>,
) -> TerminalImageProtocolDetection {
    terminal_image_protocol_detection_from_env_values_with_override(
        None,
        term,
        term_program,
        kitty_window_id,
        wezterm_executable,
    )
}

pub(super) fn terminal_image_protocol_detection_from_env_values_with_override(
    override_value: Option<&str>,
    term: Option<&str>,
    term_program: Option<&str>,
    kitty_window_id: Option<&str>,
    wezterm_executable: Option<&str>,
) -> TerminalImageProtocolDetection {
    terminal_image_protocol_detection_from_env_values_with_override_and_setting(
        override_value,
        ImagePreviewProtocolOverride::Auto,
        term,
        term_program,
        kitty_window_id,
        wezterm_executable,
    )
}

fn terminal_image_protocol_detection_from_env_values_with_override_and_setting(
    override_value: Option<&str>,
    preview_protocol: ImagePreviewProtocolOverride,
    term: Option<&str>,
    term_program: Option<&str>,
    kitty_window_id: Option<&str>,
    wezterm_executable: Option<&str>,
) -> TerminalImageProtocolDetection {
    if let Some(detection) = terminal_image_protocol_override_detection(override_value) {
        return detection;
    }
    if let Some(detection) = terminal_image_protocol_setting_detection(preview_protocol) {
        return detection;
    }

    if non_empty_env_value(kitty_window_id) {
        TerminalImageProtocolDetection {
            protocol: Some(TerminalImageProtocol::KittyLocalFile),
            source: "KITTY_WINDOW_ID",
        }
    } else if non_empty_env_value(wezterm_executable) {
        TerminalImageProtocolDetection {
            protocol: Some(TerminalImageProtocol::KittyLocalFile),
            source: "WEZTERM_EXECUTABLE",
        }
    } else if env_value_contains(term, "xterm-kitty") {
        TerminalImageProtocolDetection {
            protocol: Some(TerminalImageProtocol::KittyLocalFile),
            source: "TERM=xterm-kitty",
        }
    } else if env_value_contains(term, "ghostty") {
        TerminalImageProtocolDetection {
            protocol: Some(TerminalImageProtocol::KittyLocalFile),
            source: "TERM=xterm-ghostty",
        }
    } else if env_value_contains(term_program, "ghostty") {
        TerminalImageProtocolDetection {
            protocol: Some(TerminalImageProtocol::KittyLocalFile),
            source: "TERM_PROGRAM=ghostty",
        }
    } else if env_value_contains(term_program, "wezterm") {
        TerminalImageProtocolDetection {
            protocol: Some(TerminalImageProtocol::KittyLocalFile),
            source: "TERM_PROGRAM=WezTerm",
        }
    } else if env_value_contains(term_program, "iterm") {
        TerminalImageProtocolDetection {
            protocol: Some(TerminalImageProtocol::Iterm2InlineFile),
            source: "TERM_PROGRAM=iTerm",
        }
    } else if terminal_sixel_detection_from_env_values(term).detected {
        TerminalImageProtocolDetection {
            protocol: Some(TerminalImageProtocol::Sixel),
            source: "TERM=sixel",
        }
    } else {
        TerminalImageProtocolDetection {
            protocol: None,
            source: "none",
        }
    }
}

fn terminal_image_protocol_override_detection(
    override_value: Option<&str>,
) -> Option<TerminalImageProtocolDetection> {
    let value = override_value
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let lower = value.to_ascii_lowercase();
    Some(match lower.as_str() {
        "kitty" => TerminalImageProtocolDetection {
            protocol: Some(TerminalImageProtocol::KittyLocalFile),
            source: "OPPI_IMAGE_PREVIEW_PROTOCOL=kitty",
        },
        "kitty-local-file" => TerminalImageProtocolDetection {
            protocol: Some(TerminalImageProtocol::KittyLocalFile),
            source: "OPPI_IMAGE_PREVIEW_PROTOCOL=kitty-local-file",
        },
        "wezterm" => TerminalImageProtocolDetection {
            protocol: Some(TerminalImageProtocol::KittyLocalFile),
            source: "OPPI_IMAGE_PREVIEW_PROTOCOL=wezterm",
        },
        "iterm" => TerminalImageProtocolDetection {
            protocol: Some(TerminalImageProtocol::Iterm2InlineFile),
            source: "OPPI_IMAGE_PREVIEW_PROTOCOL=iterm",
        },
        "iterm2" => TerminalImageProtocolDetection {
            protocol: Some(TerminalImageProtocol::Iterm2InlineFile),
            source: "OPPI_IMAGE_PREVIEW_PROTOCOL=iterm2",
        },
        "iterm2-inline-file" => TerminalImageProtocolDetection {
            protocol: Some(TerminalImageProtocol::Iterm2InlineFile),
            source: "OPPI_IMAGE_PREVIEW_PROTOCOL=iterm2-inline-file",
        },
        "sixel" => TerminalImageProtocolDetection {
            protocol: Some(TerminalImageProtocol::Sixel),
            source: "OPPI_IMAGE_PREVIEW_PROTOCOL=sixel",
        },
        "off" => TerminalImageProtocolDetection {
            protocol: None,
            source: "OPPI_IMAGE_PREVIEW_PROTOCOL=off",
        },
        "none" => TerminalImageProtocolDetection {
            protocol: None,
            source: "OPPI_IMAGE_PREVIEW_PROTOCOL=none",
        },
        "disabled" | "disable" => TerminalImageProtocolDetection {
            protocol: None,
            source: "OPPI_IMAGE_PREVIEW_PROTOCOL=disabled",
        },
        _ => TerminalImageProtocolDetection {
            protocol: None,
            source: "OPPI_IMAGE_PREVIEW_PROTOCOL=unsupported",
        },
    })
}

fn terminal_image_protocol_setting_detection(
    preview_protocol: ImagePreviewProtocolOverride,
) -> Option<TerminalImageProtocolDetection> {
    Some(match preview_protocol {
        ImagePreviewProtocolOverride::Auto => return None,
        ImagePreviewProtocolOverride::Kitty => TerminalImageProtocolDetection {
            protocol: Some(TerminalImageProtocol::KittyLocalFile),
            source: "oppi.images.previewProtocol=kitty",
        },
        ImagePreviewProtocolOverride::Wezterm => TerminalImageProtocolDetection {
            protocol: Some(TerminalImageProtocol::KittyLocalFile),
            source: "oppi.images.previewProtocol=wezterm",
        },
        ImagePreviewProtocolOverride::Iterm2 => TerminalImageProtocolDetection {
            protocol: Some(TerminalImageProtocol::Iterm2InlineFile),
            source: "oppi.images.previewProtocol=iterm2",
        },
        ImagePreviewProtocolOverride::Sixel => TerminalImageProtocolDetection {
            protocol: Some(TerminalImageProtocol::Sixel),
            source: "oppi.images.previewProtocol=sixel",
        },
        ImagePreviewProtocolOverride::Off => TerminalImageProtocolDetection {
            protocol: None,
            source: "oppi.images.previewProtocol=off",
        },
    })
}

fn terminal_image_protocol_label(protocol: TerminalImageProtocol) -> &'static str {
    match protocol {
        TerminalImageProtocol::KittyLocalFile => "kitty-local-file",
        TerminalImageProtocol::Iterm2InlineFile => "iterm2-inline-file",
        TerminalImageProtocol::Sixel => "sixel",
    }
}

fn terminal_sixel_detection_from_env_values(term: Option<&str>) -> TerminalSixelDetection {
    if let Some(term) = term.map(str::trim).filter(|term| !term.is_empty()) {
        if term.to_ascii_lowercase().contains("sixel") {
            return TerminalSixelDetection {
                detected: true,
                source: Some(format!("TERM={term}")),
                status: "renderer-enabled",
            };
        }
    }
    TerminalSixelDetection {
        detected: false,
        source: None,
        status: "not-detected",
    }
}

pub(super) fn image_preview_capabilities_from_env(settings: &ImageSettings) -> Value {
    let override_value = env::var(IMAGE_PREVIEW_PROTOCOL_ENV).ok();
    let term = env::var("TERM").ok();
    let term_program = env::var("TERM_PROGRAM").ok();
    let kitty_window_id = env::var("KITTY_WINDOW_ID").ok();
    let wezterm_executable = env::var("WEZTERM_EXECUTABLE").ok();
    image_preview_capabilities_from_env_values_with_override(
        settings,
        override_value.as_deref(),
        term.as_deref(),
        term_program.as_deref(),
        kitty_window_id.as_deref(),
        wezterm_executable.as_deref(),
    )
}

pub(super) fn image_preview_capabilities_from_env_values(
    settings: &ImageSettings,
    term: Option<&str>,
    term_program: Option<&str>,
    kitty_window_id: Option<&str>,
    wezterm_executable: Option<&str>,
) -> Value {
    image_preview_capabilities_from_env_values_with_override(
        settings,
        None,
        term,
        term_program,
        kitty_window_id,
        wezterm_executable,
    )
}

fn image_preview_capabilities_from_env_values_with_override(
    settings: &ImageSettings,
    override_value: Option<&str>,
    term: Option<&str>,
    term_program: Option<&str>,
    kitty_window_id: Option<&str>,
    wezterm_executable: Option<&str>,
) -> Value {
    let detection = terminal_image_protocol_detection_from_env_values_with_override_and_setting(
        override_value,
        settings.preview_protocol,
        term,
        term_program,
        kitty_window_id,
        wezterm_executable,
    );
    let protocol = detection.protocol.map(terminal_image_protocol_label);
    let max_inline_bytes = match detection.protocol {
        Some(TerminalImageProtocol::Iterm2InlineFile) => Some(ITERM2_INLINE_IMAGE_MAX_BYTES),
        Some(TerminalImageProtocol::Sixel) => Some(SIXEL_INLINE_IMAGE_MAX_BYTES),
        _ => None,
    };
    let sixel_detection = if detection.protocol == Some(TerminalImageProtocol::Sixel)
        && explicit_image_protocol_source(detection.source)
    {
        TerminalSixelDetection {
            detected: true,
            source: Some(detection.source.to_string()),
            status: "renderer-enabled",
        }
    } else {
        terminal_sixel_detection_from_env_values(term)
    };
    json!({
        "showImages": settings.show_images,
        "imageWidthCells": settings.image_width_cells,
        "autoResizeImages": settings.auto_resize_images,
        "blockImages": settings.block_images,
        "previewProtocol": settings.preview_protocol.as_str(),
        "protocol": protocol,
        "source": detection.source,
        "protocolAvailable": detection.protocol.is_some(),
        "inlinePreviews": settings.show_images && detection.protocol.is_some(),
        "supportedFormats": PREVIEW_IMAGE_FORMATS,
        "sixelDetected": sixel_detection.detected,
        "sixelSource": sixel_detection.source,
        "sixelSupported": sixel_detection.detected,
        "sixelStatus": sixel_detection.status,
        "activeProbePerformed": false,
        "activeProbeStatus": "not-attempted",
        "activeProbeReason": "transcript-safe-env-and-settings-only",
        "maxInlineBytes": max_inline_bytes,
    })
}

pub(super) fn format_image_preview_capabilities(capabilities: &Value) -> String {
    let show_images = capabilities
        .get("showImages")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let inline_previews = capabilities
        .get("inlinePreviews")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let protocol = capabilities.get("protocol").and_then(Value::as_str);
    let source = capabilities
        .get("source")
        .and_then(Value::as_str)
        .unwrap_or("none");
    let width = capabilities
        .get("imageWidthCells")
        .and_then(Value::as_u64)
        .unwrap_or(VALID_IMAGE_WIDTH_CELLS[1]);
    let formats = capabilities
        .get("supportedFormats")
        .and_then(Value::as_array)
        .map(|formats| {
            formats
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join("/")
        })
        .filter(|formats| !formats.is_empty())
        .unwrap_or_else(|| PREVIEW_IMAGE_FORMATS.join("/"));
    let max_inline = capabilities.get("maxInlineBytes").and_then(Value::as_u64);
    let sixel_detected = capabilities
        .get("sixelDetected")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let sixel_source = capabilities
        .get("sixelSource")
        .and_then(Value::as_str)
        .unwrap_or("unknown source");
    let sixel_supported = capabilities
        .get("sixelSupported")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let mut parts = Vec::new();
    if show_images {
        if explicit_image_protocol_source(source) && protocol.is_none() {
            parts.push(format!(
                "inline image protocol override disabled ({source})"
            ));
        } else if inline_previews {
            let protocol = protocol.unwrap_or("unknown");
            parts.push(format!("inline previews enabled via {protocol} ({source})"));
        } else {
            parts.push("terminal does not advertise native inline image support".to_string());
        }
    } else {
        parts.push("previews disabled in settings".to_string());
        if let Some(protocol) = protocol {
            parts.push(format!("detected {protocol} ({source})"));
        } else {
            parts.push("terminal does not advertise native inline image support".to_string());
        }
    }
    parts.push(format!("width {width} cells"));
    parts.push(format!("formats {formats}"));
    if sixel_detected {
        if sixel_supported {
            parts.push(format!("sixel renderer enabled via {sixel_source}"));
        } else {
            parts.push(format!(
                "sixel detected via {sixel_source}; renderer unavailable"
            ));
        }
    } else {
        parts.push("sixel unsupported".to_string());
    }
    if let Some(max_inline) = max_inline {
        parts.push(format!("inline byte cap {}", format_bytes(max_inline)));
    }
    if capabilities
        .get("activeProbePerformed")
        .and_then(Value::as_bool)
        == Some(false)
    {
        let reason = capabilities
            .get("activeProbeReason")
            .and_then(Value::as_str)
            .unwrap_or("not-attempted");
        parts.push(format!("active probe not attempted ({reason})"));
    }

    format!("image preview capabilities: {}", parts.join("; "))
}

fn explicit_image_protocol_source(source: &str) -> bool {
    source.starts_with(IMAGE_PREVIEW_PROTOCOL_ENV)
        || source.starts_with("oppi.images.previewProtocol=")
}

fn non_empty_env_value(value: Option<&str>) -> bool {
    value.is_some_and(|value| !value.trim().is_empty())
}

fn env_value_contains(value: Option<&str>, needle: &str) -> bool {
    value.is_some_and(|value| value.to_ascii_lowercase().contains(needle))
}

fn is_image_artifact(artifact: &ArtifactMetadata) -> bool {
    artifact
        .mime_type
        .as_deref()
        .is_some_and(|mime| mime.to_ascii_lowercase().starts_with("image/"))
        || artifact_extension(artifact)
            .as_deref()
            .is_some_and(|extension| matches!(extension, "png" | "jpg" | "jpeg" | "webp" | "gif"))
}

fn terminal_image_preview_sequence(
    artifact: &ArtifactMetadata,
    width_cells: u64,
    protocol: TerminalImageProtocol,
) -> Option<String> {
    match protocol {
        TerminalImageProtocol::KittyLocalFile => is_local_file_preview_image_artifact(artifact)
            .then(|| {
                let encoded_path = STANDARD.encode(artifact.output_path.as_bytes());
                format!("\x1b_Ga=T,f=100,t=f,c={width_cells},q=2;{encoded_path}\x1b\\")
            }),
        TerminalImageProtocol::Iterm2InlineFile => {
            if !is_local_file_preview_image_artifact(artifact) {
                return None;
            }
            if artifact
                .bytes
                .is_some_and(|bytes| bytes > ITERM2_INLINE_IMAGE_MAX_BYTES)
            {
                return None;
            }
            let bytes = fs::read(&artifact.output_path).ok()?;
            if bytes.len() as u64 > ITERM2_INLINE_IMAGE_MAX_BYTES {
                return None;
            }
            let encoded = STANDARD.encode(bytes);
            Some(format!(
                "\x1b]1337;File=inline=1;width={width_cells};preserveAspectRatio=1:{encoded}\x07"
            ))
        }
        TerminalImageProtocol::Sixel => None,
    }
}

fn is_local_file_preview_image_artifact(artifact: &ArtifactMetadata) -> bool {
    artifact.mime_type.as_deref().is_some_and(|mime| {
        matches!(
            mime.to_ascii_lowercase().as_str(),
            "image/png" | "image/jpeg" | "image/jpg" | "image/webp" | "image/gif"
        )
    }) || artifact_extension(artifact)
        .as_deref()
        .is_some_and(|extension| matches!(extension, "png" | "jpg" | "jpeg" | "webp" | "gif"))
}

fn artifact_extension(artifact: &ArtifactMetadata) -> Option<String> {
    Path::new(&artifact.output_path)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.trim_start_matches('.').to_ascii_lowercase())
        .filter(|extension| !extension.is_empty())
}

pub(super) fn artifact_hint(output: &str) -> Option<String> {
    let value: Value = serde_json::from_str(output).ok()?;
    for key in ["outputPath", "path", "file"] {
        if let Some(path) = value.get(key).and_then(Value::as_str) {
            return Some(path.to_string());
        }
    }
    None
}

pub(super) struct RpcClient {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<std::process::ChildStdout>,
    next_id: u64,
}

impl RpcClient {
    pub(super) fn spawn(server: PathBuf) -> Result<Self, String> {
        let mut command = Command::new(&server);
        command
            .arg("--stdio")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        if env::var("OPPI_RUNTIME_STORE_DIR")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .is_none()
        {
            command.env("OPPI_RUNTIME_STORE_DIR", default_runtime_store_dir());
        }
        let mut child = command
            .spawn()
            .map_err(|error| format!("spawn {}: {error}", server.display()))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| "oppi-server stdin unavailable".to_string())?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "oppi-server stdout unavailable".to_string())?;
        Ok(Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
            next_id: 1,
        })
    }

    pub(super) fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        let request = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        writeln!(self.stdin, "{request}").map_err(|error| format!("write request: {error}"))?;
        self.stdin
            .flush()
            .map_err(|error| format!("flush request: {error}"))?;
        let mut line = String::new();
        self.stdout
            .read_line(&mut line)
            .map_err(|error| format!("read response: {error}"))?;
        if line.trim().is_empty() {
            return Err("oppi-server closed stdout".to_string());
        }
        let response: Value =
            serde_json::from_str(&line).map_err(|error| format!("decode response: {error}"))?;
        if let Some(error) = response.get("error").filter(|error| !error.is_null()) {
            return Err(error.to_string());
        }
        response
            .get("result")
            .cloned()
            .ok_or_else(|| "JSON-RPC response missing result".to_string())
    }
}

impl Drop for RpcClient {
    fn drop(&mut self) {
        let _ = self.child.try_wait();
        let _ = self.child.kill();
    }
}
