#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EventOutputMode {
    Text,
    Json,
    Silent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TurnOutcome {
    Completed,
    Paused,
    Aborted(String),
    Interrupted(String),
}

impl TurnOutcome {
    fn into_result(self) -> Result<(), String> {
        match self {
            Self::Completed | Self::Paused => Ok(()),
            Self::Aborted(reason) => Err(format!("turn aborted: {reason}")),
            Self::Interrupted(reason) => Err(format!("turn interrupted: {reason}")),
        }
    }
}

#[derive(Debug, Clone)]
struct PendingApproval {
    turn_id: String,
    request_id: String,
    tool_call_id: Option<String>,
    tool_call: Option<ToolCall>,
}

#[derive(Debug, Clone)]
struct PendingQuestion {
    turn_id: String,
    request: AskUserRequest,
    requested_at: Instant,
    deadline_at: Option<Instant>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum UiComponent {
    Transcript,
    Todos,
    Approval,
    Question,
    Background,
    Suggestion,
    Footer,
}

impl UiComponent {
    fn label(self) -> &'static str {
        match self {
            Self::Transcript => "transcript",
            Self::Todos => "todos",
            Self::Approval => "approval",
            Self::Question => "question",
            Self::Background => "background",
            Self::Suggestion => "suggestion",
            Self::Footer => "footer",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct DockState {
    todos: Vec<String>,
    approval: Option<String>,
    question: Option<String>,
    background: Option<String>,
    suggestion: Option<String>,
    footer: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TranscriptEntryKind {
    Info,
    User,
    Assistant,
    ToolRead,
    ToolWrite,
    ToolRun,
    Diff,
    Artifact,
    Denied,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TranscriptEntry {
    kind: TranscriptEntryKind,
    label: String,
    body: String,
    event_id: u64,
    turn_id: Option<String>,
    item_id: Option<String>,
    tool_call_id: Option<String>,
    artifact_id: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct NativeTokenUsage {
    input_tokens: i64,
    output_tokens: i64,
    total_tokens: i64,
    provider_calls: usize,
}

#[derive(Debug, Clone, Default)]
struct NativeUsageState {
    session: NativeTokenUsage,
    latest_context_tokens: Option<i64>,
    latest_context_window: Option<u64>,
    live_limits: Option<NativeLiveUsage>,
    live_error: Option<String>,
    last_refresh: Option<Instant>,
}

#[derive(Debug, Clone, PartialEq)]
struct NativeLiveUsage {
    source: String,
    plan_type: Option<String>,
    five_hour: Option<NativeUsageWindow>,
    weekly: Option<NativeUsageWindow>,
    fetched_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq)]
struct NativeUsageWindow {
    label: String,
    used_percent: Option<f64>,
    resets_at: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CodexUsageAuth {
    access_token: String,
    refresh_token: String,
    expires: i64,
    account_id: String,
}

#[derive(Debug, Clone)]
struct RetainedTui {
    scrollback: VecDeque<String>,
    typed_scrollback: VecDeque<TranscriptEntry>,
    max_scrollback: usize,
    docks: DockState,
    dirty: BTreeSet<UiComponent>,
}

impl RetainedTui {
    fn new(max_scrollback: usize) -> Self {
        let mut dirty = BTreeSet::new();
        dirty.insert(UiComponent::Transcript);
        dirty.insert(UiComponent::Footer);
        Self {
            scrollback: VecDeque::new(),
            typed_scrollback: VecDeque::new(),
            max_scrollback,
            docks: DockState::default(),
            dirty,
        }
    }

    fn clear(&mut self) {
        self.scrollback.clear();
        self.typed_scrollback.clear();
        self.docks = DockState::default();
        self.dirty = [
            UiComponent::Transcript,
            UiComponent::Todos,
            UiComponent::Approval,
            UiComponent::Question,
            UiComponent::Background,
            UiComponent::Suggestion,
            UiComponent::Footer,
        ]
        .into_iter()
        .collect();
    }

    fn push_transcript(&mut self, line: impl Into<String>) {
        let line = line.into();
        if line.trim().is_empty() {
            return;
        }
        self.push_transcript_entry(TranscriptEntry {
            kind: TranscriptEntryKind::Info,
            label: "log".to_string(),
            body: line,
            event_id: 0,
            turn_id: None,
            item_id: None,
            tool_call_id: None,
            artifact_id: None,
        });
    }

    fn push_event_transcript(
        &mut self,
        event: &Event,
        rendered: String,
        calls: &BTreeMap<String, ToolCall>,
    ) {
        if rendered.trim().is_empty() {
            return;
        }
        let entry = transcript_entry_from_event(event, redact_render_text(&rendered), calls);
        self.push_transcript_entry(entry);
    }

    fn push_transcript_entry(&mut self, mut entry: TranscriptEntry) {
        entry.body = redact_render_text(&entry.body);
        if entry.body.trim().is_empty() {
            return;
        }
        self.scrollback.push_back(entry.body.clone());
        self.typed_scrollback.push_back(entry);
        while self.scrollback.len() > self.max_scrollback {
            self.scrollback.pop_front();
        }
        while self.typed_scrollback.len() > self.max_scrollback {
            self.typed_scrollback.pop_front();
        }
        self.dirty.insert(UiComponent::Transcript);
    }

    fn update_docks(&mut self, next: DockState) {
        if self.docks.todos != next.todos {
            self.dirty.insert(UiComponent::Todos);
        }
        if self.docks.approval != next.approval {
            self.dirty.insert(UiComponent::Approval);
        }
        if self.docks.question != next.question {
            self.dirty.insert(UiComponent::Question);
        }
        if self.docks.background != next.background {
            self.dirty.insert(UiComponent::Background);
        }
        if self.docks.suggestion != next.suggestion {
            self.dirty.insert(UiComponent::Suggestion);
        }
        if self.docks.footer != next.footer {
            self.dirty.insert(UiComponent::Footer);
        }
        self.docks = next;
    }

    fn dirty_components(&self) -> Vec<&'static str> {
        self.dirty.iter().copied().map(UiComponent::label).collect()
    }

    fn clear_dirty(&mut self) {
        self.dirty.clear();
    }

    fn render_docks(&self, width: usize) -> String {
        let mut lines = self.dock_lines(width);
        if lines.is_empty() {
            return String::new();
        }
        lines.insert(
            0,
            width_safe_line("─ OPPi docked panels (line-mode fallback) ─", width),
        );
        lines.join("\n")
    }

    fn render_frame(&self, width: usize, height: usize) -> String {
        let width = width.max(24);
        let height = height.max(6);
        let dock_lines = self.dock_lines(width);
        let reserved = dock_lines.len().saturating_add(3);
        let transcript_height = height.saturating_sub(reserved).max(1);
        let mut lines = vec![width_safe_line(
            "╭─ OPPi retained scrollback (foundation) ─",
            width,
        )];
        let mut rendered_transcript = Vec::new();
        for entry in self.scrollback.iter().rev().take(transcript_height).rev() {
            for line in entry.lines() {
                rendered_transcript.push(width_safe_line(line, width));
            }
        }
        if rendered_transcript.is_empty() {
            rendered_transcript.push(width_safe_line("scrollback: empty", width));
        }
        lines.extend(
            rendered_transcript
                .into_iter()
                .rev()
                .take(transcript_height)
                .collect::<Vec<_>>()
                .into_iter()
                .rev(),
        );
        lines.push(width_safe_line("├─ docks ─", width));
        if dock_lines.is_empty() {
            lines.push(width_safe_line("docks: idle", width));
        } else {
            lines.extend(dock_lines);
        }
        lines.push(width_safe_line(
            "╰─ line-mode fallback remains available ─",
            width,
        ));
        lines.join("\n")
    }

    fn dock_lines(&self, width: usize) -> Vec<String> {
        let mut lines = Vec::new();
        if !self.docks.todos.is_empty() {
            lines.push(width_safe_line(
                &format!("todos: {}", self.docks.todos.join(" | ")),
                width,
            ));
        }
        if let Some(approval) = &self.docks.approval {
            lines.push(width_safe_line(&format!("approval: {approval}"), width));
        }
        if let Some(question) = &self.docks.question {
            lines.push(width_safe_line(&format!("question: {question}"), width));
        }
        if let Some(background) = &self.docks.background {
            lines.push(width_safe_line(&format!("background: {background}"), width));
        }
        if let Some(suggestion) = &self.docks.suggestion {
            lines.push(width_safe_line(&format!("suggestion: {suggestion}"), width));
        }
        if !self.docks.footer.trim().is_empty() {
            lines.push(width_safe_line(&self.docks.footer, width));
        }
        lines
    }
}

impl NativeUsageState {
    fn observe_provider_diagnostic(&mut self, diagnostic: &Diagnostic) {
        if diagnostic.metadata.get("component").map(String::as_str) != Some("provider") {
            return;
        }
        let input = diagnostic_i64(diagnostic, "inputTokens").unwrap_or(0);
        let output = diagnostic_i64(diagnostic, "outputTokens").unwrap_or(0);
        let total = diagnostic_i64(diagnostic, "totalTokens")
            .unwrap_or_else(|| input.saturating_add(output));
        if input <= 0 && output <= 0 && total <= 0 {
            return;
        }
        self.session.input_tokens = self.session.input_tokens.saturating_add(input);
        self.session.output_tokens = self.session.output_tokens.saturating_add(output);
        self.session.total_tokens = self.session.total_tokens.saturating_add(total);
        self.session.provider_calls = self.session.provider_calls.saturating_add(1);
        if input > 0 {
            self.latest_context_tokens = Some(input);
        }
        if let Some(context_window) = diagnostic_u64(diagnostic, "contextWindow")
            .or_else(|| diagnostic_u64(diagnostic, "contextWindowTokens"))
            .filter(|value| *value > 0)
        {
            self.latest_context_window = Some(context_window);
        }
    }

    fn session_footer_label(&self) -> String {
        if let Some(window) = self
            .live_limits
            .as_ref()
            .and_then(|limits| limits.five_hour.as_ref())
            && let Some(remaining) = window.remaining_percent()
        {
            return format!("usage {} {}% left", window.label, remaining.round() as i64);
        }
        format!(
            "usage session {}",
            format_compact_tokens(self.session.total_tokens)
        )
    }

    fn weekly_footer_label(&self) -> Option<String> {
        let window = self.live_limits.as_ref()?.weekly.as_ref()?;
        let remaining = window.remaining_percent()?;
        Some(format!("week {}% left", remaining.round() as i64))
    }

    fn context_footer_label(&self) -> String {
        self.latest_context_tokens
            .map(|tokens| format!("ctx {}", format_compact_tokens(tokens)))
            .unwrap_or_else(|| "ctx pending".to_string())
    }

    fn panel_limits_line(&self) -> String {
        if let Some(limits) = &self.live_limits {
            let mut parts = Vec::new();
            if let Some(window) = &limits.five_hour {
                parts.push(window.panel_label());
            }
            if let Some(window) = &limits.weekly {
                parts.push(window.panel_label());
            }
            let details = if parts.is_empty() {
                "no provider windows returned".to_string()
            } else {
                parts.join("; ")
            };
            let plan = limits
                .plan_type
                .as_deref()
                .map(|plan| format!(" plan={plan}"))
                .unwrap_or_default();
            return format!("limits: {details} (source={}{} )", limits.source, plan)
                .replace(" )", ")");
        }
        if let Some(error) = &self.live_error {
            return format!("limits: live refresh failed ({error})");
        }
        "limits: pending live refresh".to_string()
    }
}

impl NativeUsageWindow {
    fn remaining_percent(&self) -> Option<f64> {
        self.used_percent
            .map(|used| (100.0 - used).clamp(0.0, 100.0))
    }

    fn panel_label(&self) -> String {
        let pct = self
            .remaining_percent()
            .map(|remaining| format!("{}% left", remaining.round() as i64))
            .unwrap_or_else(|| "unknown remaining".to_string());
        let reset = self
            .resets_at
            .map(format_reset_epoch)
            .map(|value| format!(" reset={value}"))
            .unwrap_or_default();
        format!("{} {pct}{reset}", self.label)
    }
}

fn diagnostic_i64(diagnostic: &Diagnostic, key: &str) -> Option<i64> {
    diagnostic
        .metadata
        .get(key)
        .and_then(|value| value.parse::<i64>().ok())
}

fn diagnostic_u64(diagnostic: &Diagnostic, key: &str) -> Option<u64> {
    diagnostic
        .metadata
        .get(key)
        .and_then(|value| value.parse::<u64>().ok())
}

fn redact_render_text(text: &str) -> String {
    text.lines()
        .map(|line| {
            line.split_whitespace()
                .map(|token| {
                    let lower = token.to_ascii_lowercase();
                    if token.starts_with("sk-")
                        || lower == "bearer"
                        || lower.starts_with("bearer ")
                        || lower.starts_with("oppi_openai_api_key=")
                        || lower.starts_with("openai_api_key=")
                        || lower.contains("api_key=")
                        || lower.contains("apikey=")
                    {
                        "[REDACTED]".to_string()
                    } else {
                        token.to_string()
                    }
                })
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn transcript_entry_from_event(
    event: &Event,
    rendered: String,
    calls: &BTreeMap<String, ToolCall>,
) -> TranscriptEntry {
    let mut entry = TranscriptEntry {
        kind: TranscriptEntryKind::Info,
        label: "info".to_string(),
        body: rendered,
        event_id: event.id,
        turn_id: event.turn_id.clone(),
        item_id: None,
        tool_call_id: None,
        artifact_id: None,
    };
    match &event.kind {
        EventKind::ItemStarted { item } | EventKind::ItemCompleted { item } => {
            entry.item_id = Some(item.id.clone());
            entry.turn_id = Some(item.turn_id.clone());
            match &item.kind {
                ItemKind::UserMessage { text } => {
                    entry.kind = TranscriptEntryKind::User;
                    entry.label = "user".to_string();
                    entry.body = text.clone();
                }
                ItemKind::AssistantMessage { text } | ItemKind::Reasoning { text } => {
                    entry.kind = TranscriptEntryKind::Assistant;
                    entry.label = "oppi".to_string();
                    entry.body = text.clone();
                }
                ItemKind::ToolCall(call) => {
                    entry.kind = tool_transcript_kind(call);
                    entry.label = tool_display_kind_name_main(&call.name).to_string();
                    entry.tool_call_id = Some(call.id.clone());
                }
                ItemKind::ToolResult(result) => {
                    entry.kind = if matches!(result.status, ToolResultStatus::Error) {
                        TranscriptEntryKind::Error
                    } else {
                        calls
                            .get(&result.call_id)
                            .map(tool_transcript_kind)
                            .unwrap_or(TranscriptEntryKind::ToolRead)
                    };
                    entry.label = calls
                        .get(&result.call_id)
                        .map(|call| tool_display_kind_name_main(&call.name).to_string())
                        .unwrap_or_else(|| "tool".to_string());
                    entry.tool_call_id = Some(result.call_id.clone());
                }
                ItemKind::Diagnostic(_) => {
                    entry.kind = TranscriptEntryKind::Error;
                    entry.label = "diag".to_string();
                }
                ItemKind::HandoffSummary { summary } => {
                    entry.kind = TranscriptEntryKind::Info;
                    entry.label = "compact".to_string();
                    entry.body = summary.clone();
                }
                _ => {}
            }
        }
        EventKind::ItemDelta { item_id, delta } => {
            entry.item_id = Some(item_id.clone());
            entry.kind = if delta.trim_start().starts_with(['+', '-']) {
                TranscriptEntryKind::Diff
            } else {
                TranscriptEntryKind::Assistant
            };
            entry.label = "oppi".to_string();
            entry.body = delta.clone();
        }
        EventKind::ToolCallStarted { call } => {
            entry.kind = tool_transcript_kind(call);
            entry.label = tool_display_kind_name_main(&call.name).to_string();
            entry.tool_call_id = Some(call.id.clone());
        }
        EventKind::ToolCallCompleted { result } => {
            entry.kind = if matches!(result.status, ToolResultStatus::Error) {
                TranscriptEntryKind::Error
            } else {
                calls
                    .get(&result.call_id)
                    .map(tool_transcript_kind)
                    .unwrap_or(TranscriptEntryKind::ToolRead)
            };
            entry.label = calls
                .get(&result.call_id)
                .map(|call| tool_display_kind_name_main(&call.name).to_string())
                .unwrap_or_else(|| "tool".to_string());
            entry.tool_call_id = Some(result.call_id.clone());
        }
        EventKind::ArtifactCreated { artifact } => {
            entry.kind = TranscriptEntryKind::Artifact;
            entry.label = "artifact".to_string();
            entry.tool_call_id = Some(artifact.tool_call_id.clone());
            entry.artifact_id = Some(artifact.id.clone());
            entry.body = artifact_transcript_body(artifact);
        }
        EventKind::ApprovalRequested { request } => {
            entry.kind = TranscriptEntryKind::Denied;
            entry.label = "approval".to_string();
            entry.tool_call_id = request.tool_call.as_ref().map(tool_call_id);
        }
        EventKind::PermissionReviewStarted { record }
        | EventKind::PermissionReviewCompleted { record } => {
            entry.kind = permission_review_transcript_kind(record.status);
            entry.label = "review".to_string();
            entry.tool_call_id = Some(record.tool_call_id.clone());
        }
        EventKind::AskUserRequested { .. } => {
            entry.kind = TranscriptEntryKind::Info;
            entry.label = "question".to_string();
        }
        EventKind::Diagnostic { .. } | EventKind::TurnAborted { .. } => {
            entry.kind = TranscriptEntryKind::Error;
            entry.label = "error".to_string();
        }
        _ => {}
    }
    entry
}

fn permission_review_transcript_kind(
    status: oppi_protocol::GuardianReviewStatus,
) -> TranscriptEntryKind {
    match status {
        oppi_protocol::GuardianReviewStatus::Denied
        | oppi_protocol::GuardianReviewStatus::FailedClosed
        | oppi_protocol::GuardianReviewStatus::CircuitBlocked => TranscriptEntryKind::Denied,
        _ => TranscriptEntryKind::Info,
    }
}

fn tool_display_kind_name_main(name: &str) -> &'static str {
    if tool_name_suggests_write_main(name) {
        "write"
    } else if tool_name_suggests_run_main(name) {
        "run"
    } else {
        "read"
    }
}

fn tool_transcript_kind(call: &ToolCall) -> TranscriptEntryKind {
    if tool_name_suggests_write_main(&call.name) {
        TranscriptEntryKind::ToolWrite
    } else if tool_name_suggests_run_main(&call.name) {
        TranscriptEntryKind::ToolRun
    } else {
        TranscriptEntryKind::ToolRead
    }
}

fn tool_name_suggests_write_main(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.contains("write")
        || lower.contains("edit")
        || lower.contains("delete")
        || lower.contains("patch")
}

fn tool_name_suggests_run_main(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.contains("shell") || lower.contains("bash") || lower.contains("exec")
}

fn artifact_transcript_body(artifact: &ArtifactMetadata) -> String {
    let mut parts = vec![format!("artifact://{}", artifact.output_path)];
    if let Some(mime) = artifact.mime_type.as_deref() {
        parts.push(mime.to_string());
    }
    if let Some(bytes) = artifact.bytes {
        parts.push(format_bytes(bytes));
    }
    if let (Some(width), Some(height)) = (artifact.width, artifact.height) {
        parts.push(format!("{width}x{height}"));
    }
    parts.push(format!("id={}", artifact.id));
    parts.push(format!("tool={}", artifact.tool_call_id));
    parts.join(" · ")
}

#[derive(Debug, Clone)]
struct LoadedShellTheme {
    name: String,
    source: String,
    colors: Value,
    export: Option<Value>,
    line_tokens: ThemeTokens,
}

#[derive(Debug, Clone)]
struct ModelRegistryCache {
    provider: String,
    selected_model: Option<String>,
    scope: Vec<String>,
    available_only: bool,
    items: Vec<ModelRef>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct PermissionSandboxSettings {
    allow_internet: bool,
    allow_ssh: bool,
    project_roots: Vec<String>,
    readable_roots: Vec<String>,
    writable_roots: Vec<String>,
    protected_patterns: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OppiCompactSettings {
    idle_enabled: bool,
    idle_minutes: u64,
    idle_threshold_percent: u64,
    smart_threshold_percent: u64,
}

impl Default for OppiCompactSettings {
    fn default() -> Self {
        Self {
            idle_enabled: true,
            idle_minutes: 5,
            idle_threshold_percent: 70,
            smart_threshold_percent: 65,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FollowUpMode {
    All,
    OneAtATime,
}

impl Default for FollowUpMode {
    fn default() -> Self {
        Self::All
    }
}

impl FollowUpMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::OneAtATime => "one-at-a-time",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::OneAtATime => "one at a time",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SteeringMode {
    All,
    OneAtATime,
}

impl Default for SteeringMode {
    fn default() -> Self {
        Self::All
    }
}

impl SteeringMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::OneAtATime => "one-at-a-time",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FooterUsageDisplay {
    SessionWeekly,
    Session,
    Weekly,
    Off,
}

impl Default for FooterUsageDisplay {
    fn default() -> Self {
        Self::SessionWeekly
    }
}

impl FooterUsageDisplay {
    fn as_str(self) -> &'static str {
        match self {
            Self::SessionWeekly => "session-weekly",
            Self::Session => "session",
            Self::Weekly => "weekly",
            Self::Off => "off",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::SessionWeekly => "session + weekly",
            Self::Session => "session",
            Self::Weekly => "weekly",
            Self::Off => "off",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FooterSettings {
    show_path: bool,
    show_help_bar: bool,
    usage_display: FooterUsageDisplay,
    show_model: bool,
    show_permission: bool,
    show_memory: bool,
    show_context: bool,
}

impl Default for FooterSettings {
    fn default() -> Self {
        Self {
            show_path: true,
            show_help_bar: true,
            usage_display: FooterUsageDisplay::SessionWeekly,
            show_model: true,
            show_permission: true,
            show_memory: true,
            show_context: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ImagePreviewProtocolOverride {
    Auto,
    Kitty,
    Wezterm,
    Iterm2,
    Sixel,
    Off,
}

impl ImagePreviewProtocolOverride {
    fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Kitty => "kitty",
            Self::Wezterm => "wezterm",
            Self::Iterm2 => "iterm2",
            Self::Sixel => "sixel",
            Self::Off => "off",
        }
    }
}

impl Default for ImagePreviewProtocolOverride {
    fn default() -> Self {
        Self::Auto
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ImageSettings {
    show_images: bool,
    image_width_cells: u64,
    auto_resize_images: bool,
    block_images: bool,
    preview_protocol: ImagePreviewProtocolOverride,
}

impl Default for ImageSettings {
    fn default() -> Self {
        Self {
            show_images: true,
            image_width_cells: 80,
            auto_resize_images: true,
            block_images: false,
            preview_protocol: ImagePreviewProtocolOverride::Auto,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MemorySettings {
    enabled: bool,
    agent_model: String,
    deep_model: String,
    startup_recall: bool,
    task_start_recall: bool,
    turn_summaries: bool,
    idle_consolidation: bool,
    dashboard_port: Option<u16>,
    hoppi_install_offer: String,
    sync_enabled: bool,
    sync_repo_path: Option<String>,
    sync_repo_url: Option<String>,
    sync_pull_on_startup: bool,
    sync_push_on_exit: bool,
    sync_background: String,
    sync_encryption: String,
    sync_passphrase_source: String,
    sync_passphrase_env: String,
    sync_passphrase_file: Option<String>,
    sync_conflict_mode: String,
}

impl Default for MemorySettings {
    fn default() -> Self {
        Self {
            enabled: true,
            agent_model: "auto".to_string(),
            deep_model: "auto".to_string(),
            startup_recall: true,
            task_start_recall: true,
            turn_summaries: true,
            idle_consolidation: true,
            dashboard_port: None,
            hoppi_install_offer: "ask".to_string(),
            sync_enabled: false,
            sync_repo_path: None,
            sync_repo_url: None,
            sync_pull_on_startup: true,
            sync_push_on_exit: true,
            sync_background: "off".to_string(),
            sync_encryption: "none".to_string(),
            sync_passphrase_source: "env".to_string(),
            sync_passphrase_env: DEFAULT_SYNC_PASSPHRASE_ENV.to_string(),
            sync_passphrase_file: None,
            sync_conflict_mode: "keep-both".to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HoppiSyncDirection {
    Pull,
    Push,
    Both,
}

impl HoppiSyncDirection {
    fn as_str(self) -> &'static str {
        match self {
            Self::Pull => "pull",
            Self::Push => "push",
            Self::Both => "both",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HoppiSyncLifecycle {
    Startup,
    Exit,
    Clear,
}

impl HoppiSyncLifecycle {
    fn reason(self) -> &'static str {
        match self {
            Self::Startup => "startup",
            Self::Exit => "exit",
            Self::Clear => "clear",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Startup => "startup",
            Self::Exit => "exit",
            Self::Clear => "clear",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HoppiSessionRecapReason {
    Exit,
    Clear,
}

impl HoppiSessionRecapReason {
    fn as_str(self) -> &'static str {
        match self {
            Self::Exit => "exit",
            Self::Clear => "clear",
        }
    }

    fn job_reason(self) -> &'static str {
        match self {
            Self::Exit => "exit-recap",
            Self::Clear => "clear-recap",
        }
    }

    fn tag(self) -> &'static str {
        match self {
            Self::Exit => "oppi-exit-recap",
            Self::Clear => "oppi-clear-recap",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HoppiSyncInvocation {
    module_path: PathBuf,
    repo_path: PathBuf,
    direction: HoppiSyncDirection,
    remote: Option<String>,
    conflict_mode: String,
    passphrase: Option<String>,
    message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HoppiSyncCommandResult {
    imported: u64,
    exported: u64,
    committed: bool,
    pushed: bool,
    conflicts: Vec<String>,
    embeddings_rebuild_recommended: bool,
    rebuilt_embeddings: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HoppiSyncRunResult {
    invocation: HoppiSyncInvocation,
    result: HoppiSyncCommandResult,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HoppiRecallInvocation {
    module_path: PathBuf,
    cwd: PathBuf,
    prompt: String,
    include_startup: bool,
    include_task: bool,
}

#[derive(Debug, Clone, PartialEq)]
struct HoppiRecallCommandResult {
    context_markdown: Option<String>,
    context_bytes: u64,
    memory_count: u64,
    pinned_count: Option<u64>,
    store_path: Option<String>,
    records: Vec<MemoryRecord>,
    omitted_reason: Option<String>,
    truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HoppiDashboardInvocation {
    module_path: PathBuf,
    cwd: PathBuf,
    port: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HoppiDashboardCommandResult {
    url: String,
    fallback_url: Option<String>,
    memory_count: u64,
    pinned_count: u64,
    store_path: Option<String>,
}

struct HoppiDashboardProcess {
    result: HoppiDashboardCommandResult,
    child: Child,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HoppiListInvocation {
    module_path: PathBuf,
    cwd: PathBuf,
    include_superseded: bool,
    pinned_only: bool,
    limit: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
struct HoppiListCommandResult {
    records: Vec<MemoryRecord>,
    memory_count: u64,
    pinned_count: u64,
    store_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HoppiForgetInvocation {
    module_path: PathBuf,
    cwd: PathBuf,
    id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HoppiForgetCommandResult {
    id: String,
    memory_count: Option<u64>,
    pinned_count: Option<u64>,
    store_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HoppiPinInvocation {
    module_path: PathBuf,
    cwd: PathBuf,
    id: String,
    pinned: bool,
}

#[derive(Debug, Clone, PartialEq)]
struct HoppiPinCommandResult {
    id: String,
    pinned: bool,
    record: Option<MemoryRecord>,
    memory_count: Option<u64>,
    pinned_count: Option<u64>,
    store_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
struct HoppiUpdateInvocation {
    module_path: PathBuf,
    cwd: PathBuf,
    id: String,
    patch: Value,
}

#[derive(Debug, Clone, PartialEq)]
struct HoppiUpdateCommandResult {
    id: String,
    record: Option<MemoryRecord>,
    memory_count: Option<u64>,
    pinned_count: Option<u64>,
    store_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HoppiConsolidateInvocation {
    module_path: PathBuf,
    cwd: PathBuf,
    dry_run: bool,
    stage: String,
    limit: Option<u64>,
    budget: Option<u64>,
    agent_model: String,
    deep_model: String,
}

#[derive(Debug, Clone, PartialEq)]
struct HoppiConsolidateCommandResult {
    dry_run: bool,
    model_used: Option<String>,
    summary: Option<String>,
    records: Vec<MemoryRecord>,
    deleted_ids: Vec<String>,
    memory_count: u64,
    pinned_count: Option<u64>,
    store_path: Option<String>,
    last_consolidated_at: Option<String>,
    queue_depth: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HoppiRememberInvocation {
    module_path: PathBuf,
    cwd: PathBuf,
    summary: String,
    tags: Vec<String>,
    source: String,
    source_session_id: String,
    layer: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
struct HoppiRememberCommandResult {
    saved: bool,
    memory_count: u64,
    pinned_count: Option<u64>,
    store_path: Option<String>,
    record: Option<MemoryRecord>,
}

struct ShellSession {
    rpc: RpcClient,
    thread_id: String,
    cwd: String,
    cursor: u64,
    pending_approval: Option<PendingApproval>,
    pending_question: Option<PendingQuestion>,
    active_turn_id: Option<String>,
    active_turn_model_provider: Option<OpenAiCompatibleConfig>,
    todo_state: TodoState,
    current_goal: Option<ThreadGoal>,
    usage_state: NativeUsageState,
    memory_status: Option<MemoryStatus>,
    suggestion: Option<SuggestedNextMessage>,
    permission_mode: PermissionMode,
    permission_mode_source: String,
    permission_review_settings: GuardianReviewSettings,
    permission_sandbox_settings: PermissionSandboxSettings,
    compact_settings: OppiCompactSettings,
    ask_user_settings: AskUserSettings,
    follow_up_mode: FollowUpMode,
    steering_mode: SteeringMode,
    quiet_startup: bool,
    enable_skill_commands: bool,
    autocomplete_max_visible: usize,
    show_hardware_cursor: bool,
    double_escape_action: DoubleEscapeAction,
    tree_filter_mode: TreeFilterMode,
    show_terminal_progress: bool,
    terminal_clear_on_shrink: bool,
    editor_padding_x: usize,
    skill_command_cache: Vec<ResolvedSkill>,
    footer_settings: FooterSettings,
    image_settings: ImageSettings,
    memory_settings: MemorySettings,
    hoppi_dashboard_process: Option<HoppiDashboardProcess>,
    next_hoppi_background_sync_at: Option<Instant>,
    hoppi_dirty_version: u64,
    hoppi_consolidated_version: u64,
    hoppi_idle_consolidation_attempted_version: u64,
    hoppi_dirty_at: Option<Instant>,
    hoppi_idle_consolidating: bool,
    compaction_idle_since: Option<Instant>,
    last_compaction_offer_key: Option<String>,
    mcp_summary: Option<String>,
    follow_up_queue: VecDeque<String>,
    steering_queue: VecDeque<String>,
    steered_turns: BTreeSet<String>,
    last_prompt: Option<String>,
    prompt_variant: String,
    theme: String,
    loaded_theme: Option<LoadedShellTheme>,
    theme_list_cache: Option<Value>,
    theme_preview_cache: BTreeMap<String, LoadedShellTheme>,
    selected_model: Option<String>,
    model_registry_cache: Option<ModelRegistryCache>,
    known_model_ids: BTreeSet<String>,
    scoped_model_ids: Vec<String>,
    role_models: BTreeMap<String, String>,
    role_profile_path: PathBuf,
    tool_calls: BTreeMap<String, ToolCall>,
    background_summary: Option<String>,
    folded_threads: BTreeSet<String>,
    pending_login_picker: Option<LoginPicker>,
    meridian_child: Option<Child>,
    ui: RetainedTui,
    terminal_ui_active: bool,
}

fn take_follow_up_batch(queue: &mut VecDeque<String>, mode: FollowUpMode) -> Vec<String> {
    let Some(first) = queue.pop_front() else {
        return Vec::new();
    };
    let mut batch = vec![first];
    if mode == FollowUpMode::All {
        batch.extend(queue.drain(..));
    }
    batch
}

fn format_follow_up_batch_prompt(prompts: &[String]) -> String {
    match prompts {
        [] => String::new(),
        [prompt] => prompt.clone(),
        _ => {
            let mut lines = Vec::with_capacity(prompts.len() + 1);
            lines.push("Queued follow-ups:".to_string());
            for (index, prompt) in prompts.iter().enumerate() {
                lines.push(format!("{}. {}", index + 1, prompt.trim()));
            }
            lines.join("\n")
        }
    }
}

fn format_follow_up_run_message(prompts: &[String]) -> String {
    match prompts {
        [] => "running queued follow-up".to_string(),
        [prompt] => format!("running queued follow-up: {prompt}"),
        _ => format!("running {} queued follow-ups", prompts.len()),
    }
}

fn take_steering_batch(queue: &mut VecDeque<String>, mode: SteeringMode) -> Vec<String> {
    let Some(first) = queue.pop_front() else {
        return Vec::new();
    };
    let mut batch = vec![first];
    if mode == SteeringMode::All {
        batch.extend(queue.drain(..));
    }
    batch
}

fn format_steering_batch_prompt(prompts: &[String]) -> String {
    match prompts {
        [] => String::new(),
        [prompt] => prompt.clone(),
        _ => {
            let mut lines = Vec::with_capacity(prompts.len() + 1);
            lines.push("Queued steering messages:".to_string());
            for (index, prompt) in prompts.iter().enumerate() {
                lines.push(format!("{}. {}", index + 1, prompt.trim()));
            }
            lines.join("\n")
        }
    }
}

fn format_steering_run_message(prompts: &[String]) -> String {
    match prompts {
        [] => "running queued steering".to_string(),
        [prompt] => format!("running queued steering: {prompt}"),
        _ => format!("running {} queued steering messages", prompts.len()),
    }
}

impl ShellSession {
    fn refresh_native_usage(&mut self, provider: &ProviderConfig, force: bool) {
        let Some(config) = codex_usage_provider_config(provider) else {
            return;
        };
        if !force
            && self
                .usage_state
                .last_refresh
                .is_some_and(|last| last.elapsed() < USAGE_REFRESH_INTERVAL)
        {
            return;
        }
        self.usage_state.last_refresh = Some(Instant::now());
        match fetch_openai_codex_live_usage(config) {
            Ok(snapshot) => {
                self.usage_state.live_limits = Some(snapshot);
                self.usage_state.live_error = None;
            }
            Err(error) => {
                self.usage_state.live_error = Some(compact_usage_error(&error));
            }
        }
        self.sync_ui_docks();
    }

    fn memory_footer_label(&self) -> String {
        self.memory_status
            .as_ref()
            .map(|status| {
                if status.enabled {
                    format!("{}:{}", status.backend, status.memory_count)
                } else {
                    "off".to_string()
                }
            })
            .unwrap_or_else(|| "unknown".to_string())
    }

    fn connect(server_path: PathBuf, resume_thread: Option<String>) -> Result<Self, String> {
        let mut rpc = RpcClient::spawn(server_path)?;
        initialize_rpc_client(&mut rpc)?;

        let cwd = env::current_dir().map_err(|error| format!("read cwd: {error}"))?;
        let cwd = cwd.display().to_string();
        let thread = if let Some(thread_id) = resume_thread {
            rpc.request("thread/resume", json!({ "threadId": thread_id }))?
        } else {
            rpc.request(
                "thread/start",
                json!({
                    "project": {
                        "id": "oppi-shell",
                        "cwd": cwd.clone()
                    },
                    "title": "OPPi shell"
                }),
            )?
        };
        let thread_id = thread["thread"]["id"]
            .as_str()
            .ok_or_else(|| "thread start/resume returned no thread id".to_string())?
            .to_string();

        let role_profile_path = role_profile_settings_path();
        let role_models = load_role_profiles(&role_profile_path);
        let scoped_model_ids = load_enabled_model_scope(&role_profile_path);
        let (permission_mode, permission_mode_source) =
            load_permission_mode_setting(&role_profile_path);
        let permission_review_settings = load_permission_review_settings(&role_profile_path);
        let permission_sandbox_settings = load_permission_sandbox_settings(&role_profile_path);
        let compact_settings = load_oppi_compact_settings(&role_profile_path);
        let ask_user_settings = load_ask_user_settings(&role_profile_path);
        let follow_up_mode = load_follow_up_mode_setting(&role_profile_path);
        let steering_mode = load_steering_mode_setting(&role_profile_path);
        let quiet_startup = load_quiet_startup_setting(&role_profile_path);
        let enable_skill_commands = load_skill_commands_setting(&role_profile_path);
        let autocomplete_max_visible = load_autocomplete_max_visible_setting(&role_profile_path);
        let show_hardware_cursor = load_show_hardware_cursor_setting(&role_profile_path);
        let double_escape_action = load_double_escape_action_setting(&role_profile_path);
        let tree_filter_mode = load_tree_filter_mode_setting(&role_profile_path);
        let show_terminal_progress = load_show_terminal_progress_setting(&role_profile_path);
        let terminal_clear_on_shrink = load_terminal_clear_on_shrink_setting(&role_profile_path);
        let editor_padding_x = load_editor_padding_x_setting(&role_profile_path);
        let footer_settings = load_footer_settings(&role_profile_path);
        let image_settings = load_image_settings(&role_profile_path);
        let memory_settings = load_memory_settings(&role_profile_path);
        let next_hoppi_background_sync_at =
            hoppi_next_background_sync_at(&memory_settings, Instant::now());
        let prompt_variant = load_prompt_variant_setting(&role_profile_path);
        let mut session = Self {
            rpc,
            thread_id,
            cwd,
            cursor: 0,
            pending_approval: None,
            pending_question: None,
            active_turn_id: None,
            active_turn_model_provider: None,
            todo_state: TodoState::default(),
            current_goal: None,
            usage_state: NativeUsageState::default(),
            memory_status: None,
            suggestion: None,
            permission_mode,
            permission_mode_source,
            permission_review_settings,
            permission_sandbox_settings,
            compact_settings,
            ask_user_settings,
            follow_up_mode,
            steering_mode,
            quiet_startup,
            enable_skill_commands,
            autocomplete_max_visible,
            show_hardware_cursor,
            double_escape_action,
            tree_filter_mode,
            show_terminal_progress,
            terminal_clear_on_shrink,
            editor_padding_x,
            skill_command_cache: Vec::new(),
            footer_settings,
            image_settings,
            memory_settings,
            hoppi_dashboard_process: None,
            next_hoppi_background_sync_at,
            hoppi_dirty_version: 0,
            hoppi_consolidated_version: 0,
            hoppi_idle_consolidation_attempted_version: 0,
            hoppi_dirty_at: None,
            hoppi_idle_consolidating: false,
            compaction_idle_since: None,
            last_compaction_offer_key: None,
            mcp_summary: None,
            follow_up_queue: VecDeque::new(),
            steering_queue: VecDeque::new(),
            steered_turns: BTreeSet::new(),
            last_prompt: None,
            prompt_variant,
            theme: "oppi".to_string(),
            loaded_theme: None,
            theme_list_cache: None,
            theme_preview_cache: BTreeMap::new(),
            selected_model: None,
            model_registry_cache: None,
            known_model_ids: BTreeSet::new(),
            scoped_model_ids,
            role_models,
            role_profile_path,
            tool_calls: BTreeMap::new(),
            background_summary: None,
            folded_threads: BTreeSet::new(),
            pending_login_picker: None,
            meridian_child: None,
            ui: RetainedTui::new(2_000),
            terminal_ui_active: false,
        };
        session.sync_permission_review_settings()?;
        session.sync_ask_user_settings()?;
        session.load_thread_state(false)?;
        session.refresh_skill_command_cache();
        Ok(session)
    }

    fn line_theme_tokens(&self) -> ThemeTokens {
        if !ansi_enabled() {
            return plain_theme_tokens();
        }
        self.loaded_theme
            .as_ref()
            .map(|theme| theme.line_tokens)
            .unwrap_or_else(|| theme_tokens(&self.theme))
    }

    fn refresh_skill_command_cache(&mut self) {
        let Ok(value) = self
            .rpc
            .request("skills/list", json!({ "threadId": self.thread_id }))
        else {
            self.skill_command_cache.clear();
            return;
        };
        let Ok(result) = serde_json::from_value::<RuntimeListResult<ResolvedSkill>>(value) else {
            self.skill_command_cache.clear();
            return;
        };
        self.skill_command_cache = result.items;
    }

    fn refresh_theme_settings_cache(&mut self) {
        let Ok(list) = self
            .rpc
            .request("theme/list", json!({ "threadId": self.thread_id }))
        else {
            self.theme_list_cache = None;
            self.theme_preview_cache.clear();
            return;
        };
        let entries = active_theme_entries_from_theme_list(&list);
        self.theme_list_cache = Some(list);
        self.theme_preview_cache.clear();
        for entry in entries
            .into_iter()
            .filter(|entry| normalize_theme_name(&entry.name).is_none())
            .take(32)
        {
            let Ok(value) = self.rpc.request(
                "theme/load",
                theme_load_params_json(&self.thread_id, &entry.name),
            ) else {
                continue;
            };
            let Ok(loaded) = loaded_shell_theme_from_load_result(&value, &entry.name) else {
                continue;
            };
            self.theme_preview_cache
                .insert(entry.name.clone(), loaded.clone());
            self.theme_preview_cache.insert(loaded.name.clone(), loaded);
        }
    }

    fn render_event_line(&self, event: &Event) -> Option<String> {
        match &event.kind {
            EventKind::ArtifactCreated { artifact } => {
                Some(format_artifact_created_with_image_settings(
                    artifact,
                    self.line_theme_tokens(),
                    &self.image_settings,
                ))
            }
            _ => render_event_line_with_tokens(event, self.line_theme_tokens(), &self.tool_calls),
        }
    }

    fn start_turn_for_role(
        &mut self,
        prompt: &str,
        provider: &ProviderConfig,
        json_output: bool,
        role: Option<&str>,
    ) -> Result<String, String> {
        self.start_turn_for_role_with_system_append(prompt, provider, json_output, role, None)
    }

    fn start_turn_for_role_with_system_append(
        &mut self,
        prompt: &str,
        provider: &ProviderConfig,
        json_output: bool,
        role: Option<&str>,
        system_append: Option<&str>,
    ) -> Result<String, String> {
        self.compaction_idle_since = None;
        let prompt = self.expand_skill_command_input(prompt)?;
        let prompt = prompt.as_str();
        self.last_prompt = Some(prompt.to_string());
        let effective_provider = self.provider_for_role(provider, role);
        if let Some(role_name) = role
            && let Some(model) = self.effective_model_for_role(role_name, provider)
        {
            let _ = self.register_model_ref(
                &model,
                provider_name(&effective_provider),
                Some(role_name),
            );
        }
        let mut params = json!({
            "threadId": self.thread_id,
            "input": prompt,
            "executionMode": "background",
            "sandboxPolicy": sandbox_policy_json_with_settings(
                self.permission_mode,
                &self.cwd,
                &self.permission_sandbox_settings,
            ),
        });
        let mut active_turn_model_provider = None;
        match &effective_provider {
            ProviderConfig::Mock => {
                params["modelSteps"] = mock_model_steps_for_prompt(prompt, &self.cwd);
                if let Some(tool_definitions) = mock_tool_definitions_for_prompt(prompt) {
                    params["toolDefinitions"] = tool_definitions;
                }
            }
            ProviderConfig::OpenAiCompatible(config) => {
                active_turn_model_provider = Some(config.clone());
                let hoppi_recall = self.prepare_hoppi_recall_for_prompt(prompt);
                let mut provider_json = openai_provider_json(config);
                apply_feature_routing_to_provider(&mut provider_json, &self.prompt_variant);
                apply_prompt_variant_to_provider(&mut provider_json, &self.prompt_variant);
                if let Some(context) = hoppi_recall
                    .as_ref()
                    .and_then(|result| result.context_markdown.as_deref())
                {
                    append_hoppi_memory_context_to_provider(&mut provider_json, context);
                }
                if let Some(system_append) = system_append {
                    append_system_prompt_to_provider(
                        &mut provider_json,
                        GOAL_CONTINUATION_SYSTEM_HEADING,
                        system_append,
                    );
                }
                params["modelProvider"] = provider_json;
                params["maxContinuations"] = json!(8);
            }
        }

        let value = self.rpc.request("turn/run-agentic", params)?;
        let turn_id = value["turn"]["id"]
            .as_str()
            .ok_or_else(|| "turn/run-agentic returned no turn id".to_string())?
            .to_string();
        self.active_turn_id = Some(turn_id.clone());
        self.active_turn_model_provider = active_turn_model_provider;
        self.emit_terminal_progress(true, json_output);
        self.sync_ui_docks();
        Ok(turn_id)
    }

    fn emit_terminal_progress(&self, active: bool, json_output: bool) {
        if let Some(sequence) =
            terminal_progress_output(self.show_terminal_progress, active, json_output)
        {
            print!("{sequence}");
            let _ = io::stdout().flush();
        }
    }

    fn expand_skill_command_input(&mut self, prompt: &str) -> Result<String, String> {
        if !self.enable_skill_commands || !prompt.trim_start().starts_with("/skill:") {
            return Ok(prompt.to_string());
        }
        let value = self.rpc.request(
            "skills/expand",
            json!({ "threadId": self.thread_id, "input": prompt }),
        )?;
        Ok(value
            .get("input")
            .and_then(Value::as_str)
            .unwrap_or(prompt)
            .to_string())
    }

    fn run_turn_for_role(
        &mut self,
        prompt: &str,
        provider: &ProviderConfig,
        json: bool,
        role: Option<&str>,
    ) -> Result<TurnOutcome, String> {
        self.start_turn_for_role(prompt, provider, json, role)?;
        self.poll_until_turn_boundary(json)
    }

    fn run_turn_for_role_with_system_append(
        &mut self,
        prompt: &str,
        provider: &ProviderConfig,
        json: bool,
        role: Option<&str>,
        system_append: Option<&str>,
    ) -> Result<TurnOutcome, String> {
        self.start_turn_for_role_with_system_append(prompt, provider, json, role, system_append)?;
        self.poll_until_turn_boundary(json)
    }

    fn handle_command(
        &mut self,
        line: &str,
        provider: &mut ProviderConfig,
        json: bool,
    ) -> Result<bool, String> {
        let mut parts = line.split_whitespace();
        let command = parts.next().unwrap_or_default();
        if command.starts_with("/skill:") && self.enable_skill_commands {
            if self.is_turn_running() || self.has_pending_pause() {
                self.queue_follow_up(line.trim(), json)?;
            } else {
                self.start_turn_for_role(line.trim(), provider, json, Some("executor"))?;
            }
            return Ok(true);
        }
        match command {
            "/exit" | "/quit" => {
                self.print_exit_requested(json)?;
                Ok(false)
            }
            "/help" | "/commands" => {
                self.print_text(&slash_command_help_text(), json)?;
                Ok(true)
            }
            "/settings" | "/prefs" | "/preferences" | "/settings:oppi" | "/oppi-settings" => {
                let args = parts.collect::<Vec<_>>().join(" ");
                self.print_text(&format_settings_command_panel(args.trim())?, json)?;
                Ok(true)
            }
            "/login" | "/logout" => {
                self.handle_login_command(command, provider, parts.collect::<Vec<_>>(), json)?;
                Ok(true)
            }
            "/provider" => {
                self.handle_provider_command(provider, parts.collect::<Vec<_>>(), json)?;
                Ok(true)
            }
            "/usage" | "/stats" => {
                self.refresh_native_usage(provider, true);
                self.print_text(&format_usage_panel(self, provider), json)?;
                Ok(true)
            }
            "/footer" => {
                let args = parts.collect::<Vec<_>>().join(" ");
                self.handle_footer_command(args.trim(), json)?;
                Ok(true)
            }
            "/images" | "/image" | "/image-settings" => {
                let args = parts.collect::<Vec<_>>().join(" ");
                self.handle_images_command(args.trim(), json)?;
                Ok(true)
            }
            "/suggest-next" => {
                self.handle_suggest_next_command(parts.collect::<Vec<_>>(), json)?;
                Ok(true)
            }
            "/todos" => {
                self.handle_todos_command(parts.collect::<Vec<_>>(), json)?;
                Ok(true)
            }
            "/goal" => {
                let args = parts.collect::<Vec<_>>().join(" ");
                self.handle_goal_command(args.trim(), json)?;
                Ok(true)
            }
            "/memory" | "/mem" => {
                self.handle_memory_command(provider, parts.collect::<Vec<_>>(), json)?;
                Ok(true)
            }
            "/memory-maintenance" => {
                let args = parts.collect::<Vec<_>>();
                let mut mapped = vec!["maintenance"];
                match args.as_slice() {
                    [] | ["dry-run"] | ["status"] => mapped.push("dry-run"),
                    ["apply", ..] => mapped.push("apply"),
                    ["help"] => mapped.clear(),
                    _ => mapped.push("dry-run"),
                }
                self.handle_memory_command(provider, mapped, json)?;
                Ok(true)
            }
            "/idle-compact" => {
                let args = parts.collect::<Vec<_>>().join(" ");
                self.handle_compaction_command(provider, args.trim(), json)?;
                Ok(true)
            }
            "/sandbox" | "/permissions" => {
                let args = parts.collect::<Vec<_>>().join(" ");
                self.handle_permissions_command(args.trim(), json)?;
                Ok(true)
            }
            "/ask-user" | "/ask_user" | "/questions" => {
                let args = parts.collect::<Vec<_>>().join(" ");
                self.handle_ask_user_command(args.trim(), json)?;
                Ok(true)
            }
            "/mcp" => {
                self.handle_mcp_command(parts.collect::<Vec<_>>(), json)?;
                Ok(true)
            }
            "/models" => {
                let args = parts.collect::<Vec<_>>().join(" ");
                self.handle_models_command(provider, args.trim(), json)?;
                Ok(true)
            }
            "/scoped-models" => {
                self.handle_scoped_models_command(provider, parts.collect::<Vec<_>>(), json)?;
                Ok(true)
            }
            "/agents" => {
                self.handle_agents_command(provider, parts.collect::<Vec<_>>(), json)?;
                Ok(true)
            }
            "/skills" => {
                self.handle_skills_command(parts.collect::<Vec<_>>(), json)?;
                Ok(true)
            }
            "/prompts" | "/prompt-templates" => {
                self.handle_prompt_templates_command(json)?;
                Ok(true)
            }
            "/graphify" => {
                self.handle_graphify_command(parts.collect::<Vec<_>>(), json)?;
                Ok(true)
            }
            "/prompt-variant" | "/variant" => {
                let args = parts.collect::<Vec<_>>().join(" ");
                self.handle_prompt_variant_command(args.trim(), json)?;
                Ok(true)
            }
            "/theme" | "/themes" => {
                let args = parts.collect::<Vec<_>>().join(" ");
                self.handle_theme_command(args.trim(), json)?;
                Ok(true)
            }
            "/model" => {
                self.handle_model_command(provider, parts.collect::<Vec<_>>(), json)?;
                Ok(true)
            }
            "/resume" => {
                self.handle_resume_command(parts.collect::<Vec<_>>(), json)?;
                Ok(true)
            }
            "/new" => {
                self.handle_new_thread_command(
                    parts.collect::<Vec<_>>(),
                    json,
                    NewThreadMode::New,
                )?;
                Ok(true)
            }
            "/clear" | "/reset" => {
                self.handle_new_thread_command(
                    parts.collect::<Vec<_>>(),
                    json,
                    NewThreadMode::Clear,
                )?;
                Ok(true)
            }
            "/fork" => {
                self.handle_fork_command(parts.collect::<Vec<_>>(), json)?;
                Ok(true)
            }
            "/tree" | "/sessions" => {
                self.handle_tree_command(command, parts.collect::<Vec<_>>(), Some(provider), json)?;
                Ok(true)
            }
            "/debug" => {
                self.handle_debug_command(json)?;
                Ok(true)
            }
            "/ui" | "/frame" => {
                self.handle_ui_command(provider, parts.collect::<Vec<_>>(), json)?;
                Ok(true)
            }
            "/keys" | "/keybindings" | "/oppi-terminal-setup" => {
                self.print_text(terminal_capability_summary(false), json)?;
                Ok(true)
            }
            "/follow-up" | "/followups" => {
                let args = parts.collect::<Vec<_>>().join(" ");
                self.handle_follow_up_command(args.trim(), json)?;
                Ok(true)
            }
            "/startup" | "/quiet-startup" => {
                let args = parts.collect::<Vec<_>>().join(" ");
                self.handle_startup_command(args.trim(), json)?;
                Ok(true)
            }
            "/autocomplete" | "/autocomplete-max-visible" => {
                let args = parts.collect::<Vec<_>>().join(" ");
                self.handle_autocomplete_command(args.trim(), json)?;
                Ok(true)
            }
            "/cursor" | "/hardware-cursor" => {
                let args = parts.collect::<Vec<_>>().join(" ");
                self.handle_cursor_command(args.trim(), json)?;
                Ok(true)
            }
            "/terminal" | "/terminal-progress" => {
                let args = parts.collect::<Vec<_>>().join(" ");
                self.handle_terminal_command(args.trim(), json)?;
                Ok(true)
            }
            "/editor" | "/input" => {
                let args = parts.collect::<Vec<_>>().join(" ");
                self.handle_editor_command(args.trim(), json)?;
                Ok(true)
            }
            "/steer" => {
                let input = parts.collect::<Vec<_>>().join(" ");
                self.handle_steer_command(input.trim(), json)?;
                Ok(true)
            }
            "/interrupt" | "/cancel" => {
                self.interrupt_active_turn(json)?;
                Ok(true)
            }
            "/roles" | "/role-model" => {
                self.handle_role_model_command(provider, parts.collect::<Vec<_>>(), json)?;
                Ok(true)
            }
            "/effort" => {
                let args = parts.collect::<Vec<_>>().join(" ");
                self.handle_effort_command(provider, args.trim(), json)?;
                Ok(true)
            }
            "/background" => {
                self.handle_background_command(parts.collect::<Vec<_>>(), json)?;
                Ok(true)
            }
            "/meridian" => {
                let mut mapped = vec!["meridian"];
                let args = parts.collect::<Vec<_>>();
                if args.is_empty() {
                    mapped.push("status");
                } else {
                    mapped.extend(args);
                }
                self.handle_login_command("/login", provider, mapped, json)?;
                Ok(true)
            }
            "/runtime-loop" => {
                let debug = self.rpc.request("debug/bundle", json!({}))?;
                if json {
                    self.print_value(
                        "runtimeLoop",
                        json!({
                            "mode": "native-rust-owned",
                            "threadId": self.thread_id,
                            "server": "oppi-server",
                            "turnLoop": "turn/run-agentic",
                            "fallback": "stable Pi remains available through normal `oppi`",
                            "serverMetrics": debug.get("metrics").cloned().unwrap_or(Value::Null),
                        }),
                        json,
                    )?;
                } else {
                    self.print_text(
                        "runtime-loop: native shell already runs turns through Rust `turn/run-agentic`; stable Pi remains the fallback through normal `oppi`. Use `oppi runtime-loop smoke --json` from the CLI for the mirror smoke matrix.",
                        json,
                    )?;
                }
                Ok(true)
            }
            "/review" | "/audit" | "/init" | "/independent" | "/bug-report"
            | "/feature-request" => {
                let args = parts.collect::<Vec<_>>().join(" ");
                self.prepare_and_run_command(command, &args, provider, json)?;
                Ok(true)
            }
            "/approve" => {
                self.resume_pending_approval(provider, json)?;
                Ok(true)
            }
            "/deny" => {
                self.deny_pending_approval(json)?;
                Ok(true)
            }
            "/answer" => {
                let answer = parts.collect::<Vec<_>>().join(" ");
                self.answer_pending_question(answer.trim(), provider, json)?;
                Ok(true)
            }
            "/btw" => {
                let question = parts.collect::<Vec<_>>().join(" ");
                self.handle_btw_command(question.trim(), provider, json)?;
                Ok(true)
            }
            "/again" => {
                let prompt = self
                    .last_prompt
                    .clone()
                    .ok_or_else(|| "no previous prompt".to_string())?;
                if self.is_turn_running() || self.has_pending_pause() {
                    self.queue_follow_up(&prompt, json)?;
                } else {
                    let _ = self.run_turn_for_role(&prompt, provider, json, Some("executor"))?;
                }
                Ok(true)
            }
            other => {
                let args = parts.collect::<Vec<_>>().join(" ");
                match self.prepare_and_run_command(other, &args, provider, json) {
                    Ok(()) => {}
                    Err(error) if is_command_prepare_unknown_error(&error) => {
                        self.print_text(&format!("unknown command: {other}. Try /help."), json)?;
                    }
                    Err(error) => return Err(error),
                }
                Ok(true)
            }
        }
    }

    fn handle_login_picker_input(
        &mut self,
        input: &str,
        provider: &mut ProviderConfig,
        json_output: bool,
    ) -> Result<bool, String> {
        let Some(picker) = self.pending_login_picker else {
            return Ok(false);
        };
        let choice = normalize_login_choice(input);
        if matches!(choice.as_str(), "q" | "quit" | "cancel" | "back") {
            self.pending_login_picker = None;
            self.print_text("login picker closed", json_output)?;
            return Ok(true);
        }
        match picker {
            LoginPicker::Root => match choice.as_str() {
                "1" | "subscription" | "subscriptions" => {
                    self.pending_login_picker = Some(LoginPicker::Subscription);
                    self.print_text(&login_subscription_picker_panel(), json_output)?;
                }
                "2" | "api" | "apikey" | "api-key" => {
                    self.pending_login_picker = Some(LoginPicker::Api);
                    self.print_text(&login_api_picker_panel(), json_output)?;
                }
                _ => self.print_text(&format!("unknown login choice `{input}`\n{}", login_root_picker_panel(provider)), json_output)?,
            },
            LoginPicker::Subscription => match choice.as_str() {
                "1" | "codex" | "chatgpt" | "chat-gpt" => {
                    self.pending_login_picker = None;
                    self.configure_codex_login(provider, &[], json_output)?;
                }
                "2" | "claude" | "anthropic" | "meridian" => {
                    self.pending_login_picker = Some(LoginPicker::Claude);
                    self.print_text(&login_claude_picker_panel(), json_output)?;
                }
                "3" | "copilot" | "microsoft" | "github" => {
                    self.pending_login_picker = None;
                    self.configure_github_copilot_login(provider, &[], json_output)?;
                }
                _ => self.print_text(&format!("unknown subscription choice `{input}`\n{}", login_subscription_picker_panel()), json_output)?,
            },
            LoginPicker::Api => match choice.as_str() {
                "1" | "openai" | "openai-compatible" | "env" => {
                    self.pending_login_picker = None;
                    self.print_text(login_openai_instructions(), json_output)?;
                }
                _ => self.print_text(&format!("unknown API choice `{input}`\n{}", login_api_picker_panel()), json_output)?,
            },
            LoginPicker::Claude => match choice.as_str() {
                "1" | "status" => self.print_text(&meridian_status_panel(), json_output)?,
                "2" | "login" | "auth" => {
                    self.pending_login_picker = None;
                    self.run_claude_code_login(json_output)?;
                }
                "3" | "install" => {
                    self.pending_login_picker = Some(LoginPicker::MeridianInstallApproval);
                    self.print_text(&login_meridian_install_approval_panel(), json_output)?;
                }
                "4" | "start" => {
                    let result = self.start_meridian_bridge()?;
                    self.print_text(&result, json_output)?;
                    self.configure_meridian_login(provider, json_output)?;
                    self.pending_login_picker = None;
                }
                "5" | "use" | "configure" => {
                    self.configure_meridian_login(provider, json_output)?;
                    self.pending_login_picker = None;
                }
                "6" | "stop" => {
                    self.stop_meridian_bridge(json_output)?;
                    self.pending_login_picker = None;
                }
                _ => self.print_text(&format!("unknown Claude login choice `{input}`\n{}", login_claude_picker_panel()), json_output)?,
            },
            LoginPicker::MeridianInstallApproval => match choice.as_str() {
                "yes" | "y" | "approve" | "install" => {
                    self.pending_login_picker = None;
                    self.print_text("Installing managed Meridian bridge after explicit approval.", json_output)?;
                    let result = install_meridian_package()?;
                    self.print_text(&result, json_output)?;
                }
                "no" | "n" | "deny" => {
                    self.pending_login_picker = Some(LoginPicker::Claude);
                    self.print_text("Meridian install cancelled. Returning to Claude login options.", json_output)?;
                    self.print_text(&login_claude_picker_panel(), json_output)?;
                }
                _ => self.print_text(&format!("Please type `yes` to approve installing {MERIDIAN_PACKAGE_NAME}, or `no` to cancel."), json_output)?,
            },
        }
        Ok(true)
    }

    fn handle_login_command(
        &mut self,
        command: &str,
        provider: &mut ProviderConfig,
        args: Vec<&str>,
        json_output: bool,
    ) -> Result<(), String> {
        if command == "/logout" {
            return self.handle_logout_command(provider, args, json_output);
        }
        match args.as_slice() {
            [] | ["status"] => {
                self.pending_login_picker = Some(LoginPicker::Root);
                self.print_text(&login_root_picker_panel(provider), json_output)
            }
            ["subscription"] | ["subscriptions"] => {
                self.pending_login_picker = Some(LoginPicker::Subscription);
                self.print_text(&login_subscription_picker_panel(), json_output)
            }
            ["subscription", "codex" | "chatgpt", flags @ ..]
            | ["chatgpt" | "codex", flags @ ..] => {
                self.pending_login_picker = None;
                self.configure_codex_login(provider, flags, json_output)
            }
            ["subscription", "copilot" | "microsoft" | "github", flags @ ..]
            | ["copilot" | "microsoft" | "github", flags @ ..] => {
                self.pending_login_picker = None;
                self.configure_github_copilot_login(provider, flags, json_output)
            }
            ["api"] => {
                self.pending_login_picker = Some(LoginPicker::Api);
                self.print_text(&login_api_picker_panel(), json_output)
            }
            ["api", "openai"] | ["api", "openai", "status"] | ["openai"]
            | ["openai", "status"] => self.print_text(login_openai_instructions(), json_output),
            ["api", "openai", "env", env_name]
            | ["api", "openai", "auth-env", env_name]
            | ["openai", "env", env_name]
            | ["openai", "auth-env", env_name] => {
                self.pending_login_picker = None;
                self.configure_openai_env_login(provider, env_name, json_output)
            }
            ["subscription", "claude" | "anthropic" | "meridian"]
            | ["anthropic"]
            | ["claude"]
            | ["meridian"] => {
                self.pending_login_picker = Some(LoginPicker::Claude);
                self.print_text(&login_claude_picker_panel(), json_output)
            }
            ["subscription", "claude" | "anthropic" | "meridian", "status"]
            | ["anthropic" | "claude" | "meridian", "status"] => {
                self.print_text(&meridian_status_panel(), json_output)
            }
            ["subscription", "claude" | "anthropic" | "meridian", "login"]
            | ["anthropic" | "claude" | "meridian", "login"]
            | ["subscription", "claude" | "anthropic" | "meridian", "auth"]
            | ["anthropic" | "claude" | "meridian", "auth"] => {
                self.pending_login_picker = None;
                self.run_claude_code_login(json_output)
            }
            ["subscription", "claude" | "anthropic" | "meridian", "install", flags @ ..]
            | ["anthropic" | "claude" | "meridian", "install", flags @ ..] => {
                if !login_action_approved(flags) {
                    self.pending_login_picker = Some(LoginPicker::MeridianInstallApproval);
                    return self.print_text(&login_meridian_install_approval_panel(), json_output);
                }
                self.pending_login_picker = None;
                self.print_text("Installing managed Meridian bridge after explicit approval.", json_output)?;
                let result = install_meridian_package()?;
                self.print_text(&result, json_output)
            }
            ["subscription", "claude" | "anthropic" | "meridian", "start"]
            | ["anthropic" | "claude" | "meridian", "start"] => {
                self.pending_login_picker = None;
                let result = self.start_meridian_bridge()?;
                self.print_text(&result, json_output)?;
                self.configure_meridian_login(provider, json_output)
            }
            ["subscription", "claude" | "anthropic" | "meridian", "use"]
            | ["anthropic" | "claude" | "meridian", "use"] => {
                self.pending_login_picker = None;
                self.configure_meridian_login(provider, json_output)
            }
            ["subscription", "claude" | "anthropic" | "meridian", "use", _model]
            | ["anthropic" | "claude" | "meridian", "use", _model] => self.print_text(
                "login Claude no longer accepts a model argument. Use `/model <model-id>` or `/model role <role> <model-id>` first, then `/login subscription claude use`.",
                json_output,
            ),
            ["subscription", "claude" | "anthropic" | "meridian", "stop"]
            | ["anthropic" | "claude" | "meridian", "stop"] => {
                self.pending_login_picker = None;
                self.stop_meridian_bridge(json_output)
            }
            ["gemini" | "antigravity"] => {
                self.pending_login_picker = None;
                self.print_text(login_delegated_provider_text(args[0]), json_output)
            }
            _ => self.print_text(login_usage(), json_output),
        }
    }

    fn handle_logout_command(
        &mut self,
        provider: &mut ProviderConfig,
        args: Vec<&str>,
        json_output: bool,
    ) -> Result<(), String> {
        match args.as_slice() {
            [] | ["provider"] => {
                *provider = ProviderConfig::Mock;
                self.register_provider_model(provider)?;
                self.print_text("logout: native direct-provider settings cleared for this shell session; persistent Pi OAuth/API credentials are not touched. Use stable `oppi` /logout for Pi-managed credentials.", json_output)
            }
            ["anthropic" | "claude" | "meridian"] => self.stop_meridian_bridge(json_output),
            _ => self.print_text("usage: /logout [provider|anthropic]", json_output),
        }
    }

    fn configure_openai_env_login(
        &mut self,
        provider: &mut ProviderConfig,
        env_name: &str,
        json_output: bool,
    ) -> Result<(), String> {
        let env_name = env_name.trim();
        if !is_safe_api_key_env_name(env_name) {
            return Err("/login openai env accepts only env-reference names such as OPPI_OPENAI_API_KEY or OPENAI_API_KEY; raw keys are rejected".to_string());
        }
        let model = self
            .session_model(provider)
            .map(str::to_string)
            .or_else(default_model_from_env)
            .unwrap_or_else(|| OPENAI_DIRECT_DEFAULT_MODEL.to_string());
        *provider = ProviderConfig::OpenAiCompatible(with_default_reasoning_effort(
            OpenAiCompatibleConfig {
                flavor: DirectProviderFlavor::OpenAiCompatible,
                provider_id: None,
                model,
                base_url: None,
                api_key_env: Some(env_name.to_string()),
                system_prompt: None,
                temperature: None,
                reasoning_effort: None,
                max_output_tokens: None,
                stream: true,
            },
        ));
        self.register_provider_model(provider)?;
        self.print_text(
            &format!(
                "login openai: configured direct OpenAI-compatible provider with env reference {env_name} ({}). Raw secrets were not stored.",
                if env::var(env_name).ok().filter(|value| !value.trim().is_empty()).is_some() { "present" } else { "missing" }
            ),
            json_output,
        )
    }

    fn print_memory_settings_value_update(
        &mut self,
        label: &str,
        value: &str,
        json_output: bool,
    ) -> Result<(), String> {
        if json_output {
            return self.print_value(
                "memory",
                json!({ "settings": memory_settings_json(&self.memory_settings) }),
                json_output,
            );
        }
        self.print_text(&format!("memory: {label} {value}"), json_output)
    }

    fn print_memory_settings_text_update(
        &mut self,
        label: &str,
        value: String,
        json_output: bool,
    ) -> Result<(), String> {
        if json_output {
            return self.print_value(
                "memory",
                json!({ "settings": memory_settings_json(&self.memory_settings) }),
                json_output,
            );
        }
        self.print_text(&format!("memory: {label} {value}"), json_output)
    }

    fn configure_codex_login(
        &mut self,
        provider: &mut ProviderConfig,
        flags: &[&str],
        json_output: bool,
    ) -> Result<(), String> {
        let force = login_action_approved(flags)
            || flags.iter().any(|flag| {
                matches!(
                    flag.trim().to_ascii_lowercase().as_str(),
                    "--force" | "force" | "relogin"
                )
            });
        let auth_path = auth_store_path();
        if force || !codex_auth_present_at(&auth_path) {
            let (verifier, state, url) = create_codex_oauth_flow()?;
            let listener = CodexOAuthListener::bind()?;
            self.print_text(
                &format!(
                    "login Codex: opening browser for ChatGPT/Codex OAuth. If it does not open, paste this URL into your browser:\n{url}\nWaiting for local callback at {OPENAI_CODEX_REDIRECT_URI}..."
                ),
                json_output,
            )?;
            if let Err(error) = open_browser(&url) {
                self.print_text(
                    &format!("login Codex: could not open browser automatically ({error}); open the URL above manually."),
                    json_output,
                )?;
            }
            let code = listener.wait_for_code(&state, Duration::from_secs(300))?;
            let token = exchange_codex_oauth_code(&code, &verifier)?;
            persist_codex_oauth(&auth_path, token)?;
            self.print_text(
                &format!(
                    "login Codex: OAuth credential saved to {} (tokens redacted).",
                    format_path_for_display(&auth_path)
                ),
                json_output,
            )?;
        }

        let model = self
            .session_model(provider)
            .filter(|model| *model != MOCK_MODEL_ID)
            .map(str::to_string)
            .or_else(|| env::var("OPPI_OPENAI_CODEX_MODEL").ok())
            .or_else(default_model_from_env)
            .unwrap_or_else(|| OPENAI_CODEX_DEFAULT_MODEL.to_string());
        *provider = ProviderConfig::OpenAiCompatible(with_default_reasoning_effort(
            OpenAiCompatibleConfig {
                flavor: DirectProviderFlavor::OpenAiCodex,
                provider_id: None,
                model,
                base_url: env::var("OPPI_OPENAI_CODEX_BASE_URL")
                    .ok()
                    .or_else(|| env::var("OPPI_CODEX_BASE_URL").ok()),
                api_key_env: None,
                system_prompt: None,
                temperature: None,
                reasoning_effort: None,
                max_output_tokens: None,
                stream: true,
            },
        ));
        self.register_provider_model(provider)?;
        self.print_text(
            "login Codex: configured ChatGPT/Codex subscription provider for this shell session. Run `/provider validate`, then `/provider smoke` for an explicit live call. Model choice stays in `/model`.",
            json_output,
        )
    }

    fn configure_github_copilot_login(
        &mut self,
        provider: &mut ProviderConfig,
        flags: &[&str],
        json_output: bool,
    ) -> Result<(), String> {
        let force = login_action_approved(flags)
            || flags.iter().any(|flag| {
                matches!(
                    flag.trim().to_ascii_lowercase().as_str(),
                    "--force" | "force" | "relogin"
                )
            });
        let enterprise_domain = github_copilot_enterprise_domain_from_flags(flags)?;
        let auth_path = github_copilot_auth_store_path();
        if force || !github_copilot_auth_present_at(&auth_path) {
            let domain = enterprise_domain
                .as_deref()
                .unwrap_or(GITHUB_COPILOT_DEFAULT_DOMAIN);
            let device = start_github_copilot_device_flow(domain)?;
            self.print_text(
                &format!(
                    "login Copilot: open {} and enter code {}. Waiting for GitHub device authorization...",
                    device.verification_uri, device.user_code
                ),
                json_output,
            )?;
            if let Err(error) = open_browser(&device.verification_uri) {
                self.print_text(
                    &format!("login Copilot: could not open browser automatically ({error}); open the URL above manually."),
                    json_output,
                )?;
            }
            let github_access = poll_github_copilot_device_flow(domain, &device)?;
            let token = refresh_github_copilot_token(&github_access, enterprise_domain.as_deref())?;
            persist_github_copilot_oauth(
                &auth_path,
                &github_access,
                enterprise_domain.as_deref(),
                token,
            )?;
            self.print_text(
                &format!(
                    "login Copilot: OAuth credential saved to {} (tokens redacted).",
                    format_path_for_display(&auth_path)
                ),
                json_output,
            )?;
        }

        let auth = read_github_copilot_auth_at(&auth_path)?;
        let base_url = github_copilot_base_url(&auth);
        let model = self
            .session_model(provider)
            .filter(|model| *model != "mock-scripted")
            .map(str::to_string)
            .or_else(|| env::var("OPPI_GITHUB_COPILOT_MODEL").ok())
            .or_else(default_model_from_env)
            .unwrap_or_else(|| GITHUB_COPILOT_DEFAULT_MODEL.to_string());
        *provider = ProviderConfig::OpenAiCompatible(with_default_reasoning_effort(
            OpenAiCompatibleConfig {
                flavor: DirectProviderFlavor::GitHubCopilot,
                provider_id: None,
                model,
                base_url: Some(base_url),
                api_key_env: None,
                system_prompt: None,
                temperature: None,
                reasoning_effort: None,
                max_output_tokens: None,
                stream: true,
            },
        ));
        self.register_provider_model(provider)?;
        self.print_text(
            "login Copilot: configured GitHub Copilot subscription provider for this shell session. Use `/model <openai-compatible-copilot-model>` for model choice; `/provider smoke` is the explicit live call.",
            json_output,
        )
    }

    fn configure_meridian_login(
        &mut self,
        provider: &mut ProviderConfig,
        json_output: bool,
    ) -> Result<(), String> {
        let selected_model = self
            .session_model(provider)
            .filter(|model| *model != MOCK_MODEL_ID)
            .unwrap_or(MERIDIAN_DEFAULT_MODEL)
            .to_string();
        let config = meridian_provider_config(Some(&selected_model));
        let model = config.model.clone();
        let base_url = config.base_url.clone().unwrap_or_else(meridian_base_url);
        *provider = ProviderConfig::OpenAiCompatible(config);
        self.register_provider_model(provider)?;
        self.print_text(
            &format!(
                "login Claude: configured explicit Meridian bridge at {}. Model is {model}; change it with `/model <model-id>` or `/model role <role> <model-id>`, not `/login`. Authentication stays in Claude Code/Meridian; OPPi sends only placeholder auth to the loopback bridge.",
                redacted_base_url_label(&base_url)
            ),
            json_output,
        )
    }

    fn start_meridian_bridge(&mut self) -> Result<String, String> {
        if meridian_reachable() {
            return Ok(format!(
                "Meridian already reachable at {}",
                meridian_base_url()
            ));
        }
        let mut failures = Vec::new();
        for command in meridian_start_candidates() {
            match spawn_meridian_command(&command) {
                Ok(mut child) => {
                    if wait_for_meridian(Duration::from_secs(12)) {
                        self.meridian_child = Some(child);
                        return Ok(format!(
                            "Started Meridian at {} via {}",
                            meridian_base_url(),
                            command.display()
                        ));
                    }
                    let _ = child.kill();
                    failures.push(format!("{}: did not become reachable", command.display()));
                }
                Err(error) => failures.push(format!("{}: {error}", command.display())),
            }
        }
        Err(format!(
            "Could not start Meridian without hidden fallback. Run `/login anthropic install`, run `claude login`, start `meridian` yourself, or set OPPI_MERIDIAN_COMMAND. Tried: {}",
            failures.join("; ")
        ))
    }

    fn stop_meridian_bridge(&mut self, json_output: bool) -> Result<(), String> {
        if let Some(mut child) = self.meridian_child.take() {
            let _ = child.kill();
            return self.print_text(
                "Stopped Meridian process started by this OPPi shell session.",
                json_output,
            );
        }
        self.print_text("Meridian was not started by this OPPi shell session; if you run it externally, stop that process directly.", json_output)
    }

    fn run_claude_code_login(&mut self, json_output: bool) -> Result<(), String> {
        if json_output {
            return self.print_text(
                "login Claude: run `claude login` in an interactive terminal, then `/login subscription claude start` or `/login subscription claude use`. Meridian owns token refresh through the Claude Code SDK; OPPi does not extract Claude tokens.",
                json_output,
            );
        }
        if self.terminal_ui_active {
            return self.print_text(
                "login Claude: leave the retained TUI or use --no-tui, run `claude login` in the terminal, then return to `/login subscription claude start`. OPPi will not run an interactive Claude Code login while raw TUI mode owns the terminal.",
                json_output,
            );
        }
        self.print_text(
            "login Claude: launching explicit `claude login` in this terminal. Tokens stay with Claude Code/Meridian; OPPi does not read or store them.",
            json_output,
        )?;
        let status = Command::new("claude")
            .arg("login")
            .status()
            .map_err(|error| {
                format!("spawn `claude login`: {error}. Install Claude Code first, then retry.")
            })?;
        if status.success() {
            self.print_text(
                "login Claude: Claude Code login finished. Next: `/login subscription claude start` or `/login subscription claude use`.",
                json_output,
            )
        } else {
            Err(format!("`claude login` exited with status {status}"))
        }
    }

    fn handle_provider_command(
        &mut self,
        provider: &mut ProviderConfig,
        args: Vec<&str>,
        json_output: bool,
    ) -> Result<(), String> {
        match args.as_slice() {
            [] | ["status"] => {
                let models = self.list_models().unwrap_or_default();
                self.print_text(&provider_status_panel(provider, &models, self), json_output)
            }
            ["auth-env" | "key-env", env_name] => match provider {
                ProviderConfig::Mock => self.print_text(
                    "provider auth-env: mock provider does not use credentials",
                    json_output,
                ),
                ProviderConfig::OpenAiCompatible(config) => {
                    let env_name = env_name.trim();
                    if !is_safe_api_key_env_name(env_name) {
                        return Err("credential setup accepts only env-reference names such as OPPI_OPENAI_API_KEY or OPENAI_API_KEY; raw keys are rejected".to_string());
                    }
                    config.api_key_env = Some(env_name.to_string());
                    self.print_text(
                        &format!(
                            "provider api key env set to {env_name} ({})",
                            if env::var(env_name).ok().filter(|value| !value.trim().is_empty()).is_some() { "present" } else { "missing" }
                        ),
                        json_output,
                    )
                }
            },
            ["base-url", url] => match provider {
                ProviderConfig::Mock => self.print_text(
                    "provider base-url: mock provider does not use a network endpoint",
                    json_output,
                ),
                ProviderConfig::OpenAiCompatible(config) => {
                    config.base_url = Some((*url).to_string());
                    self.print_text(
                        &format!("provider base URL set to {}", redacted_base_url_label(url)),
                        json_output,
                    )
                }
            },
            ["validate"] => self.print_text(&provider_validation_panel(provider), json_output),
            ["smoke", prompt @ ..] => {
                let prompt = if prompt.is_empty() {
                    "Reply with OPPi provider validation OK.".to_string()
                } else {
                    prompt.join(" ")
                };
                if !provider_local_validation_ready(provider) {
                    return Err("provider smoke requires a configured model and present API key env; run /provider validate for redacted diagnostics".to_string());
                }
                let _ = self.run_turn_for_role(&prompt, provider, json_output, Some("executor"))?;
                Ok(())
            }
            ["policy"] => self.print_text(provider_policy_text(), json_output),
            ["anthropic" | "anthropic-status" | "compatibility"] => {
                self.print_text(anthropic_provider_evaluation(), json_output)
            }
            _ => self.print_text(
                "usage: /provider [status|auth-env <ENV>|base-url <url>|validate|smoke [prompt]|policy|anthropic]",
                json_output,
            ),
        }
    }

    fn handle_todos_command(&mut self, args: Vec<&str>, json_output: bool) -> Result<(), String> {
        match parse_todos_command_args(&args)? {
            TodoCommand::List => {
                let value = self.rpc.request("todos/list", json!({}))?;
                let result: TodoListResult = serde_json::from_value(value.clone())
                    .map_err(|error| format!("decode todos/list: {error}"))?;
                self.todo_state = result.state;
                if json_output {
                    self.print_value("todos", value, json_output)
                } else {
                    self.print_text(&format_todos(&self.todo_state), json_output)
                }
            }
            TodoCommand::ClientAction { action, id } => {
                let mut params = json!({
                    "threadId": self.thread_id,
                    "action": todo_client_action_name(action),
                });
                if let Some(id) = id.as_deref() {
                    params["id"] = json!(id);
                }
                let value = self.rpc.request("todos/client-action", params)?;
                let result: TodoClientActionResult = serde_json::from_value(value.clone())
                    .map_err(|error| format!("decode todos/client-action: {error}"))?;
                self.todo_state = result.state;
                if json_output {
                    self.print_value("todos", value, json_output)
                } else {
                    self.print_text(&format_todos_with_summary(&self.todo_state), json_output)
                }
            }
        }
    }

    fn handle_models_command(
        &mut self,
        provider: &ProviderConfig,
        args: &str,
        json_output: bool,
    ) -> Result<(), String> {
        let (show_all, filter) = parse_models_command_args(args);
        let available_only = !show_all;
        let result = self.model_registry_result(provider, available_only)?;
        let models = result.items;
        let diagnostic_messages = result
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.clone())
            .collect::<Vec<_>>();
        if json_output {
            self.print_value(
                "models",
                json!({
                    "items": filter_models(&models, &filter),
                    "diagnostics": result.diagnostics,
                    "availableOnly": result.available_only,
                    "selectedModel": result.selected_model,
                    "provider": result.provider,
                    "scope": result.scope,
                    "filter": if filter.is_empty() { Value::Null } else { json!(filter) },
                    "roleModels": self.role_models,
                    "roleProfilePath": self.role_profile_path.display().to_string(),
                    "source": if available_only { "runtime native registry (available)" } else { "runtime native registry (all)" },
                }),
                json_output,
            )
        } else {
            let selected_model = result
                .selected_model
                .as_ref()
                .map(|model| model.id.as_str())
                .or_else(|| self.selected_model.as_deref());
            let rendered = format_model_list_with_diagnostics(
                &models,
                selected_model,
                &filter,
                &self.role_models,
                &diagnostic_messages,
            );
            self.print_text(&rendered, json_output)
        }
    }

    fn model_registry_result(
        &mut self,
        provider: &ProviderConfig,
        available_only: bool,
    ) -> Result<ModelRegistryListResult, String> {
        self.model_registry_result_with_scope(
            provider,
            available_only,
            self.scoped_model_ids.clone(),
        )
    }

    fn model_registry_result_with_scope(
        &mut self,
        provider: &ProviderConfig,
        available_only: bool,
        scope: Vec<String>,
    ) -> Result<ModelRegistryListResult, String> {
        let value = self.rpc.request(
            "model/registry",
            json!({
                "threadId": self.thread_id,
                "provider": provider_name(provider),
                "selectedModelId": self.session_model(provider),
                "scope": scope.clone(),
                "availableOnly": available_only,
            }),
        )?;
        let result: ModelRegistryListResult = serde_json::from_value(value)
            .map_err(|error| format!("decode model/registry: {error}"))?;
        if scope == self.scoped_model_ids {
            self.model_registry_cache = Some(ModelRegistryCache {
                provider: result.provider.clone(),
                selected_model: result.selected_model.as_ref().map(|model| model.id.clone()),
                scope: result.scope.clone(),
                available_only: result.available_only,
                items: result.items.clone(),
            });
        }
        Ok(result)
    }

    fn refresh_model_registry_cache(
        &mut self,
        provider: &ProviderConfig,
        available_only: bool,
    ) -> Result<(), String> {
        let provider_name = provider_name(provider);
        let selected_model = self.session_model(provider).map(str::to_string);
        if self.model_registry_cache.as_ref().is_some_and(|cache| {
            cache.provider == provider_name
                && cache.selected_model == selected_model
                && cache.scope == self.scoped_model_ids
                && cache.available_only == available_only
        }) {
            return Ok(());
        }
        self.model_registry_result(provider, available_only)
            .map(|_| ())
    }

    fn cached_model_refs_for_provider(
        &self,
        provider: &ProviderConfig,
        available_only: bool,
    ) -> Option<&[ModelRef]> {
        let provider_name = provider_name(provider);
        self.model_registry_cache
            .as_ref()
            .filter(|cache| {
                cache.provider == provider_name && cache.available_only == available_only
            })
            .map(|cache| cache.items.as_slice())
    }

    fn current_model_context_window(&self, provider: &ProviderConfig) -> Option<u64> {
        let provider_name = provider_name(provider);
        let selected = self.session_model(provider)?;
        self.model_registry_cache
            .as_ref()
            .filter(|cache| {
                cache.provider == provider_name
                    && cache.selected_model.as_deref() == Some(selected)
                    && cache.scope == self.scoped_model_ids
            })
            .and_then(|cache| {
                cache
                    .items
                    .iter()
                    .find(|model| model.provider == provider_name && model.id == selected)
                    .and_then(|model| model.context_window)
            })
    }

    fn compaction_trigger_telemetry_for_provider(
        &mut self,
        provider: &ProviderConfig,
    ) -> CompactionTriggerTelemetry {
        let _ = self.refresh_model_registry_cache(provider, false);
        let context_window = self
            .usage_state
            .latest_context_window
            .or_else(|| self.current_model_context_window(provider));
        compaction_trigger_telemetry(
            &self.compact_settings,
            self.usage_state.latest_context_tokens,
            context_window,
            active_todos(&self.todo_state).len(),
        )
    }

    fn cached_model_ids_for_provider(
        &self,
        provider: &ProviderConfig,
        available_only: bool,
    ) -> Option<Vec<String>> {
        self.cached_model_refs_for_provider(provider, available_only)
            .map(|items| items.iter().map(|model| model.id.clone()).collect())
    }

    fn runtime_model_refs_for_provider(
        &mut self,
        provider: &ProviderConfig,
        available_only: bool,
    ) -> Result<Vec<ModelRef>, String> {
        Ok(self.model_registry_result(provider, available_only)?.items)
    }

    fn runtime_model_refs_for_provider_with_scope(
        &mut self,
        provider: &ProviderConfig,
        available_only: bool,
        scope: Vec<String>,
    ) -> Result<Vec<ModelRef>, String> {
        Ok(self
            .model_registry_result_with_scope(provider, available_only, scope)?
            .items)
    }

    fn runtime_model_ids_for_provider(
        &mut self,
        provider: &ProviderConfig,
        available_only: bool,
    ) -> Result<Vec<String>, String> {
        Ok(self
            .runtime_model_refs_for_provider(provider, available_only)?
            .into_iter()
            .map(|model| model.id)
            .collect())
    }

    fn handle_suggest_next_command(
        &mut self,
        args: Vec<&str>,
        json_output: bool,
    ) -> Result<(), String> {
        match args.as_slice() {
            [] | ["show"] => {
                let message = self
                    .suggestion
                    .as_ref()
                    .map(format_suggestion_summary)
                    .unwrap_or_else(|| "suggest-next: no active ghost suggestion".to_string());
                self.print_text(&message, json_output)
            }
            ["debug"] => {
                let message = self
                    .suggestion
                    .as_ref()
                    .map(format_suggestion_debug)
                    .unwrap_or_else(|| "suggest-next: no active ghost suggestion".to_string());
                self.print_text(&message, json_output)
            }
            ["clear" | "ignore"] => {
                self.suggestion = None;
                self.print_text("suggest-next: cleared", json_output)
            }
            ["accept"] => self.print_text(
                "suggest-next: in native TUI, Tab accepts the ghost suggestion into the editor; /suggest-next accept is handled before command dispatch when typed in the editor",
                json_output,
            ),
            _ => self.print_text(
                "usage: /suggest-next [show|accept|clear|debug]",
                json_output,
            ),
        }
    }

    fn handle_goal_command(&mut self, args: &str, json_output: bool) -> Result<(), String> {
        let route = match goal_command_route(args) {
            Ok(route) => route,
            Err(message) if message.starts_with("usage:") => {
                return self.print_text(&message, json_output);
            }
            Err(message) => return Err(message),
        };

        let value = match route {
            GoalCommandRoute::Get => self
                .rpc
                .request("thread/goal/get", json!({ "threadId": self.thread_id }))?,
            GoalCommandRoute::Clear => self.rpc.request(
                "thread/goal/clear",
                json!({
                    "threadId": self.thread_id,
                }),
            )?,
            GoalCommandRoute::Set {
                objective,
                status,
                token_budget,
            } => self.rpc.request(
                "thread/goal/set",
                goal_set_params(&self.thread_id, objective, status, token_budget),
            )?,
            GoalCommandRoute::CreateObjective(objective) => {
                let current = self
                    .rpc
                    .request("thread/goal/get", json!({ "threadId": self.thread_id }))?;
                self.apply_goal_response(&current)?;
                if let Some(goal) = decode_goal_from_response(&current)?
                    && goal.status != ThreadGoalStatus::Complete
                {
                    return self.print_text(
                        "Goal already active. Use /goal replace <objective> to replace it.",
                        json_output,
                    );
                }
                self.rpc.request(
                    "thread/goal/set",
                    json!({
                        "threadId": self.thread_id,
                        "objective": objective,
                        "status": "active",
                    }),
                )?
            }
        };
        self.apply_goal_response(&value)?;

        if json_output {
            self.print_value("goal", value, json_output)
        } else if value.get("cleared").is_some() {
            let cleared = value
                .get("cleared")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            self.print_text(
                if cleared {
                    "Goal cleared"
                } else {
                    "Goal already clear"
                },
                json_output,
            )
        } else {
            self.print_text(&format_goal_response(&value), json_output)
        }
    }

    fn apply_goal_response(&mut self, value: &Value) -> Result<(), String> {
        if value.get("goal").is_some() {
            self.current_goal = decode_goal_from_response(value)?;
            self.sync_ui_docks();
        } else if value.get("cleared").is_some() {
            self.current_goal = None;
            self.sync_ui_docks();
        }
        Ok(())
    }

    fn handle_scoped_models_command(
        &mut self,
        provider: &ProviderConfig,
        args: Vec<&str>,
        json_output: bool,
    ) -> Result<(), String> {
        match args.as_slice() {
            [] | ["list"] => {
                let registry = self.model_registry_result(provider, false)?;
                let rendered = format_scoped_models_from_registry(provider, &registry);
                self.print_text(&rendered, json_output)
            }
            ["enable", model] | ["add", model] => {
                let model = model.trim();
                if model.is_empty() {
                    return Err(
                        "usage: /scoped-models enable <model-id|provider/model-id>".to_string()
                    );
                }
                if !self.scoped_model_ids.iter().any(|item| item == model) {
                    self.scoped_model_ids.push(model.to_string());
                    save_enabled_model_scope(
                        &self.role_profile_path,
                        Some(self.scoped_model_ids.as_slice()),
                    )?;
                }
                let registry = self.model_registry_result(provider, false)?;
                self.print_text(
                    &format_scoped_models_from_registry(provider, &registry),
                    json_output,
                )
            }
            ["disable" | "remove", model] => {
                let model = model.trim();
                self.scoped_model_ids.retain(|item| item != model);
                save_enabled_model_scope(
                    &self.role_profile_path,
                    Some(self.scoped_model_ids.as_slice()),
                )?;
                let registry = self.model_registry_result(provider, false)?;
                self.print_text(
                    &format_scoped_models_from_registry(provider, &registry),
                    json_output,
                )
            }
            ["clear" | "all"] => {
                self.scoped_model_ids.clear();
                save_enabled_model_scope(&self.role_profile_path, None)?;
                self.print_text(
                    "scoped models cleared; /model and Ctrl+P use all current provider models",
                    json_output,
                )
            }
            _ => self.print_text(
                "usage: /scoped-models [list|enable <model>|disable <model>|clear]",
                json_output,
            ),
        }
    }

    fn handle_model_command(
        &mut self,
        provider: &mut ProviderConfig,
        args: Vec<&str>,
        json_output: bool,
    ) -> Result<(), String> {
        match args.as_slice() {
            [] => self.print_text(&self.model_status(provider), json_output),
            ["role", role_args @ ..] => {
                self.handle_role_model_command(provider, role_args.to_vec(), json_output)
            }
            [model] => self.select_session_model(provider, model, json_output),
            _ => self.print_text(
                "usage: /model [model-id|role <role> <model-id|inherit>]",
                json_output,
            ),
        }
    }

    fn handle_role_model_command(
        &mut self,
        provider: &mut ProviderConfig,
        args: Vec<&str>,
        json_output: bool,
    ) -> Result<(), String> {
        match args.as_slice() {
            [] => self.print_text(
                &format!(
                    "{}\nsource: {}",
                    format_role_profiles(&self.role_models, self.session_model(provider)),
                    self.role_profile_path.display()
                ),
                json_output,
            ),
            [role] => {
                let role = normalize_role(role).ok_or_else(|| role_usage().to_string())?;
                let value = self
                    .role_models
                    .get(role)
                    .map(String::as_str)
                    .unwrap_or("inherit");
                self.print_text(&format!("role {role}: {value}"), json_output)
            }
            [role, "inherit" | "off" | "unset"] => {
                let role = normalize_role(role).ok_or_else(|| role_usage().to_string())?;
                self.role_models.remove(role);
                save_role_profiles(&self.role_profile_path, &self.role_models)?;
                self.print_text(
                    &format!(
                        "role {role} now inherits {} (persisted)",
                        self.session_model(provider).unwrap_or("session model")
                    ),
                    json_output,
                )
            }
            [role, model] => {
                let role = normalize_role(role).ok_or_else(|| role_usage().to_string())?;
                self.role_models
                    .insert(role.to_string(), (*model).to_string());
                save_role_profiles(&self.role_profile_path, &self.role_models)?;
                self.register_model_ref(model, provider_name(provider), Some(role))?;
                self.print_text(
                    &format!("role {role} model set to {model} (persisted)"),
                    json_output,
                )
            }
            _ => self.print_text(role_usage(), json_output),
        }
    }

    fn select_session_model(
        &mut self,
        provider: &mut ProviderConfig,
        model: &str,
        json_output: bool,
    ) -> Result<(), String> {
        if model.contains('/') {
            let custom_provider =
                custom_provider_config_for_model_at_path(model, &models_json_path())?;
            let selected_model = current_provider_model(&custom_provider)
                .unwrap_or(model)
                .to_string();
            *provider = custom_provider;
            self.register_model_ref(&selected_model, provider_name(provider), None)?;
            let value = self.rpc.request(
                "model/select",
                json!({ "threadId": self.thread_id, "modelId": selected_model }),
            )?;
            self.selected_model = Some(selected_model.clone());
            let selected_label = format!("{}/{}", provider_name(provider), selected_model);
            return if json_output {
                self.print_value("modelSelected", value, json_output)
            } else {
                self.print_text(&format!("model set to {selected_label}"), json_output)
            };
        }
        self.register_model_ref(model, provider_name(provider), None)?;
        let value = self.rpc.request(
            "model/select",
            json!({ "threadId": self.thread_id, "modelId": model }),
        )?;
        self.selected_model = Some(model.to_string());
        if let ProviderConfig::OpenAiCompatible(config) = provider {
            config.model = model.to_string();
        }
        if json_output {
            self.print_value("modelSelected", value, json_output)
        } else {
            self.print_text(&format!("model set to {model}"), json_output)
        }
    }

    fn session_model<'a>(&'a self, provider: &'a ProviderConfig) -> Option<&'a str> {
        self.selected_model
            .as_deref()
            .or_else(|| current_provider_model(provider))
    }

    fn model_status(&self, provider: &ProviderConfig) -> String {
        let session_model = self.session_model(provider).unwrap_or("none");
        let session_model_display = display_model_id(session_model);
        format!(
            "model: {session_model_display}\nselected: {}\n{}\nroleProfileSource: {}",
            self.selected_model
                .as_deref()
                .map(display_model_id)
                .unwrap_or("none"),
            format_role_profiles(&self.role_models, self.session_model(provider)),
            self.role_profile_path.display()
        )
    }

    fn register_provider_model(&mut self, provider: &ProviderConfig) -> Result<(), String> {
        match provider {
            ProviderConfig::Mock => {
                self.register_model_ref(MOCK_MODEL_ID, "mock", None)?;
                self.selected_model = Some(MOCK_MODEL_ID.to_string());
                let _ = self.rpc.request(
                    "model/select",
                    json!({ "threadId": self.thread_id, "modelId": MOCK_MODEL_ID }),
                )?;
            }
            ProviderConfig::OpenAiCompatible(config) => {
                self.register_model_ref(&config.model, "openai-compatible", None)?;
                self.selected_model = Some(config.model.clone());
                let _ = self.rpc.request(
                    "model/select",
                    json!({ "threadId": self.thread_id, "modelId": config.model }),
                )?;
            }
        }
        self.register_role_model_refs(provider)
    }

    fn register_role_model_refs(&mut self, provider: &ProviderConfig) -> Result<(), String> {
        let provider = provider_name(provider);
        for (role, model) in self.role_models.clone() {
            self.register_model_ref(&model, provider, Some(&role))?;
        }
        Ok(())
    }

    fn register_model_ref(
        &mut self,
        model: &str,
        provider: &str,
        role: Option<&str>,
    ) -> Result<(), String> {
        let model = model.trim();
        if model.is_empty() {
            return Err("model id cannot be empty".to_string());
        }
        self.known_model_ids.insert(model.to_string());
        self.rpc.request(
            "model/register",
            json!({
                "threadId": self.thread_id,
                "model": {
                    "id": model,
                    "provider": provider,
                    "displayName": model,
                    "contextWindow": null,
                    "role": role,
                }
            }),
        )?;
        Ok(())
    }

    fn list_models(&mut self) -> Result<Vec<ModelRef>, String> {
        let value = self.rpc.request("model/list", json!({}))?;
        let result: RuntimeListResult<ModelRef> =
            serde_json::from_value(value).map_err(|error| format!("decode model/list: {error}"))?;
        for model in &result.items {
            self.known_model_ids.insert(model.id.clone());
        }
        Ok(result.items)
    }

    fn provider_for_role(&self, provider: &ProviderConfig, role: Option<&str>) -> ProviderConfig {
        provider_for_role_config(provider, &self.role_models, role)
    }

    fn provider_for_role_with_complexity(
        &self,
        provider: &ProviderConfig,
        role: Option<&str>,
        promote_complex_subagent: bool,
    ) -> ProviderConfig {
        provider_for_role_config_with_complexity(
            provider,
            &self.role_models,
            role,
            promote_complex_subagent,
        )
    }

    fn effective_model_for_role(&self, role: &str, provider: &ProviderConfig) -> Option<String> {
        let role = normalize_role(role)?;
        let effective = self.provider_for_role(provider, Some(role));
        current_provider_model(&effective).map(str::to_string)
    }

    fn effective_effort_for_role(&self, role: &str, provider: &ProviderConfig) -> Option<String> {
        let effective = self.provider_for_role(provider, Some(role));
        match effective {
            ProviderConfig::OpenAiCompatible(config) => config.reasoning_effort,
            ProviderConfig::Mock => None,
        }
    }

    fn role_execution_profile(&self, role: &str, provider: &ProviderConfig) -> String {
        let normalized = normalize_role(role).unwrap_or("executor");
        let model = self
            .effective_model_for_role(normalized, provider)
            .unwrap_or_else(|| "provider default".to_string());
        let model_source = if self.role_models.contains_key(normalized) {
            "profile"
        } else if default_model_for_role(provider, normalized, false).is_some() {
            "default"
        } else {
            "inherit"
        };
        let effort = self
            .effective_effort_for_role(normalized, provider)
            .unwrap_or_else(|| "off".to_string());
        format!(
            "role={normalized} model={} ({model_source}) effort={effort}",
            display_model_id(&model)
        )
    }

    fn handle_resume_command(&mut self, args: Vec<&str>, json_output: bool) -> Result<(), String> {
        let Some(thread_id) = args.first().copied() else {
            let value = self.rpc.request("thread/list", json!({}))?;
            let result: RuntimeListResult<Thread> = serde_json::from_value(value)
                .map_err(|error| format!("decode thread/list: {error}"))?;
            if json_output {
                let items: Vec<Thread> = result
                    .items
                    .into_iter()
                    .filter(|thread| same_project_cwd(&thread.project.cwd, &self.cwd))
                    .collect();
                return self.print_value(
                    "resume",
                    json!({
                        "currentThread": &self.thread_id,
                        "projectCwd": &self.cwd,
                        "items": items,
                    }),
                    json_output,
                );
            }
            let rendered = format_resume_session_list(
                &result.items,
                &self.thread_id,
                &self.cwd,
                &self.folded_threads,
            );
            return self.print_text(&rendered, json_output);
        };
        let value = self
            .rpc
            .request("thread/resume", json!({ "threadId": thread_id }))?;
        self.switch_to_thread(value, json_output, "resumed")
    }

    fn handle_new_thread_command(
        &mut self,
        args: Vec<&str>,
        json_output: bool,
        mode: NewThreadMode,
    ) -> Result<(), String> {
        if mode == NewThreadMode::Clear {
            self.maybe_remember_hoppi_clear_recap();
            self.maybe_drain_memory_jobs(json_output)?;
            maybe_run_hoppi_lifecycle_sync(self, HoppiSyncLifecycle::Clear, json_output)?;
        }
        let title = if args.is_empty() {
            "OPPi shell".to_string()
        } else {
            args.join(" ")
        };
        let value = self.rpc.request(
            "thread/start",
            json!({
                "project": { "id": "oppi-shell", "cwd": self.cwd.clone() },
                "title": title,
            }),
        )?;
        self.switch_to_thread(value, json_output, "started")
    }

    fn handle_fork_command(&mut self, args: Vec<&str>, json_output: bool) -> Result<(), String> {
        let title = if args.is_empty() {
            format!("Fork of {}", self.thread_id)
        } else {
            args.join(" ")
        };
        let value = self.rpc.request(
            "thread/fork",
            json!({ "threadId": self.thread_id, "title": title }),
        )?;
        self.switch_to_thread(value, json_output, "forked")
    }

    fn handle_tree_command(
        &mut self,
        command: &str,
        args: Vec<&str>,
        provider: Option<&ProviderConfig>,
        json_output: bool,
    ) -> Result<(), String> {
        match tree_command_route_for_command(command, &args)? {
            TreeCommandRoute::Entries { after, limit } => {
                let limit = limit.unwrap_or(TREE_ENTRY_DEFAULT_LIMIT);
                let value = self.rpc.request(
                    "thread/items",
                    json!({
                        "threadId": self.thread_id,
                        "after": after,
                        "filter": self.tree_filter_mode.as_str(),
                        "limit": limit
                    }),
                )?;
                let result: ThreadItemsResult = serde_json::from_value(value)
                    .map_err(|error| format!("decode thread/items: {error}"))?;
                let returned_count = result.entries.len();
                let next_after = thread_item_tree_next_after(&result.entries, result.total_count);
                if json_output {
                    return self.print_value(
                        "treeEntries",
                        json!({
                            "threadId": self.thread_id,
                            "after": result.after,
                            "limit": limit,
                            "treeFilterMode": self.tree_filter_mode.as_str(),
                            "branch": &result.branch,
                            "returnedCount": returned_count,
                            "totalCount": result.total_count,
                            "nextAfter": next_after,
                            "items": thread_item_tree_json_from_entries(&result.entries),
                        }),
                        json_output,
                    );
                }
                return self.print_text(
                    &format_thread_item_tree_from_entries(
                        &result.entries,
                        Some(&result.branch),
                        self.tree_filter_mode,
                        result.after,
                        result.total_count,
                        next_after,
                    ),
                    json_output,
                );
            }
            TreeCommandRoute::Label { event_id, labels } => {
                let value = self.rpc.request(
                    "thread/item-labels/set",
                    json!({
                        "threadId": self.thread_id,
                        "eventId": event_id,
                        "labels": labels
                    }),
                )?;
                if json_output {
                    return self.print_value("treeItemLabels", value, json_output);
                }
                let result: ThreadItemLabelsResult = serde_json::from_value(value)
                    .map_err(|error| format!("decode thread/item-labels/set: {error}"))?;
                return self.print_text(&format_thread_item_labels_result(&result), json_output);
            }
            TreeCommandRoute::Fork { event_id, title } => {
                let value = self.rpc.request(
                    "thread/item-fork",
                    json!({
                        "threadId": self.thread_id,
                        "eventId": event_id,
                        "title": title
                    }),
                )?;
                return self.switch_to_thread(value, json_output, "forked");
            }
            TreeCommandRoute::Edit { event_id, input } => {
                let provider = provider.ok_or_else(|| {
                    "tree edit requires an interactive provider context".to_string()
                })?;
                let title = format!("Edit from event {event_id}");
                let value = self.rpc.request(
                    "thread/item-edit-fork",
                    json!({
                        "threadId": self.thread_id,
                        "eventId": event_id,
                        "input": input.clone(),
                        "title": title
                    }),
                )?;
                let input = value
                    .get("input")
                    .and_then(Value::as_str)
                    .unwrap_or(input.as_str())
                    .to_string();
                self.switch_to_thread(value, json_output, "forked")?;
                self.start_turn_for_role(&input, provider, json_output, Some("executor"))?;
                return Ok(());
            }
            TreeCommandRoute::Fold(thread_id) => {
                self.folded_threads.insert(thread_id.clone());
                return self.print_text(&format!("session {thread_id} folded"), json_output);
            }
            TreeCommandRoute::Unfold(thread_id) => {
                self.folded_threads.remove(&thread_id);
                return self.print_text(&format!("session {thread_id} unfolded"), json_output);
            }
            TreeCommandRoute::Toggle(thread_id) => {
                let state = if self.folded_threads.remove(&thread_id) {
                    "unfolded"
                } else {
                    self.folded_threads.insert(thread_id.clone());
                    "folded"
                };
                return self.print_text(&format!("session {thread_id} {state}"), json_output);
            }
            TreeCommandRoute::Rename { thread_id, title } => {
                let value = self.rpc.request(
                    "thread/rename",
                    json!({ "threadId": thread_id, "title": title }),
                )?;
                return self.print_value("session", value, json_output);
            }
            TreeCommandRoute::Archive(thread_id) => {
                let value = self
                    .rpc
                    .request("thread/archive", json!({ "threadId": thread_id }))?;
                return self.print_value("session", value, json_output);
            }
            TreeCommandRoute::SetFilter(mode) => {
                self.tree_filter_mode = mode;
                save_tree_filter_mode_setting(&self.role_profile_path, mode)?;
                if json_output {
                    return self.print_value(
                        "tree",
                        json!({ "treeFilterMode": self.tree_filter_mode.as_str() }),
                        json_output,
                    );
                }
                return self.print_text(&format_tree_status(self.tree_filter_mode), json_output);
            }
            TreeCommandRoute::Status => {
                if json_output {
                    return self.print_value(
                        "tree",
                        json!({ "treeFilterMode": self.tree_filter_mode.as_str() }),
                        json_output,
                    );
                }
                return self.print_text(&format_tree_status(self.tree_filter_mode), json_output);
            }
            TreeCommandRoute::Show => {}
        }
        let value = self.rpc.request("thread/list", json!({}))?;
        if json_output {
            let mut value = value;
            value["treeFilterMode"] = json!(self.tree_filter_mode.as_str());
            return self.print_value("tree", value, json_output);
        }
        let result: RuntimeListResult<Thread> = serde_json::from_value(value)
            .map_err(|error| format!("decode thread/list: {error}"))?;
        let mut rendered = format_thread_tree_with_filter(
            &result.items,
            &self.thread_id,
            &self.folded_threads,
            self.tree_filter_mode,
        );
        if let Some(summary) = self.latest_handoff_summary()? {
            rendered.push_str(&format!("\nsummary: {summary}"));
        }
        rendered.push_str(
            "\nnote: use /resume <thread-id>, /sessions rename <thread-id> <title>, /sessions delete <thread-id>, or /tree fold <thread-id>",
        );
        self.print_text(&rendered, json_output)
    }

    fn handle_debug_command(&mut self, json_output: bool) -> Result<(), String> {
        let mut value = self.rpc.request("debug/bundle", json!({}))?;
        value["client"] = json!({
            "name": "oppi-shell",
            "version": env!("CARGO_PKG_VERSION"),
            "threadId": self.thread_id,
            "lastPrompt": self.last_prompt.as_deref().map(redact_debug_text),
            "currentGoal": self.current_goal.as_ref(),
            "followUpQueue": self.follow_up_queue.len(),
            "followUpMode": self.follow_up_mode.as_str(),
            "steeringQueue": self.steering_queue.len(),
            "steeringMode": self.steering_mode.as_str(),
            "quietStartup": self.quiet_startup,
            "enableSkillCommands": self.enable_skill_commands,
            "autocompleteMaxVisible": self.autocomplete_max_visible,
            "showHardwareCursor": self.show_hardware_cursor,
            "doubleEscapeAction": self.double_escape_action.as_str(),
            "treeFilterMode": self.tree_filter_mode.as_str(),
            "showTerminalProgress": self.show_terminal_progress,
            "terminalClearOnShrink": self.terminal_clear_on_shrink,
            "editorPaddingX": self.editor_padding_x,
            "cursor": self.cursor,
            "activeTurnId": self.active_turn_id,
            "roleModels": self.role_models,
            "roleProfilePath": self.role_profile_path.display().to_string(),
            "retainedUi": {
                "scrollbackLines": self.ui.scrollback.len(),
                "dirtyComponents": self.ui.dirty_components(),
            },
        });
        self.print_value("debug", value, json_output)
    }

    fn handle_ui_command(
        &mut self,
        provider: &ProviderConfig,
        args: Vec<&str>,
        json_output: bool,
    ) -> Result<(), String> {
        let ratatui = args
            .first()
            .is_some_and(|value| matches!(*value, "ratatui" | "rt"));
        let size_args = if ratatui { &args[1..] } else { args.as_slice() };
        let width = size_args
            .first()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or_else(terminal_width);
        let height = size_args
            .get(1)
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(24);
        self.sync_ui_docks();
        if ratatui {
            let mode = size_args
                .get(2)
                .map(|value| match *value {
                    "running" | "run" => ratatui_ui::RatatuiFrameMode::Running,
                    "tools" | "tool" | "artifact" | "denial" => ratatui_ui::RatatuiFrameMode::Tools,
                    "question" | "ask" | "ask_user" => ratatui_ui::RatatuiFrameMode::Question,
                    "background" | "bg" => ratatui_ui::RatatuiFrameMode::Background,
                    "todos" | "todo" => ratatui_ui::RatatuiFrameMode::Todos,
                    "slash" | "commands" => ratatui_ui::RatatuiFrameMode::Slash,
                    "settings" | "overlay" => ratatui_ui::RatatuiFrameMode::Settings,
                    _ => ratatui_ui::RatatuiFrameMode::Idle,
                })
                .unwrap_or(ratatui_ui::RatatuiFrameMode::Idle);
            let view = ratatui_ui::ratatui_view_model(self, provider);
            let frame =
                ratatui_ui::render_ratatui_preview(&view, width as u16, height as u16, mode)?;
            if json_output {
                self.print_value(
                    "ui",
                    json!({
                        "renderer": "ratatui",
                        "width": width,
                        "height": height,
                        "mode": format!("{mode:?}"),
                        "frame": frame,
                    }),
                    json_output,
                )?;
            } else {
                self.print_text(&frame, json_output)?;
            }
            return Ok(());
        }
        if json_output {
            self.print_value(
                "ui",
                json!({
                    "renderer": "retained",
                    "width": width,
                    "height": height,
                    "dirtyComponents": self.ui.dirty_components(),
                    "scrollbackLines": self.ui.scrollback.len(),
                    "frame": self.ui.render_frame(width, height),
                }),
                json_output,
            )?;
        } else {
            let mut rendered = self.ui.render_frame(width, height);
            let dirty = self.ui.dirty_components();
            if !dirty.is_empty() {
                rendered.push_str(&format!("\ninvalidated: {}", dirty.join(", ")));
            }
            self.print_text(&rendered, json_output)?;
        }
        self.ui.clear_dirty();
        Ok(())
    }

    fn switch_to_thread(
        &mut self,
        value: Value,
        json_output: bool,
        verb: &str,
    ) -> Result<(), String> {
        let thread_id = value["thread"]["id"]
            .as_str()
            .ok_or_else(|| "thread response returned no thread id".to_string())?
            .to_string();
        self.thread_id = thread_id.clone();
        self.cursor = 0;
        self.pending_approval = None;
        self.pending_question = None;
        self.active_turn_id = None;
        self.active_turn_model_provider = None;
        self.current_goal = None;
        self.tool_calls.clear();
        self.background_summary = None;
        self.follow_up_queue.clear();
        self.steering_queue.clear();
        self.steered_turns.clear();
        self.ui.clear();
        self.load_thread_state(json_output)?;
        if json_output {
            self.print_value("thread", value, json_output)
        } else {
            let mut rendered = format!("{verb} thread {thread_id}");
            if let Some(summary) = self.latest_handoff_summary()? {
                rendered.push_str(&format!("\nlatest handoff: {summary}"));
            }
            self.print_text(&rendered, json_output)
        }
    }

    fn load_thread_state(&mut self, render_pauses: bool) -> Result<(), String> {
        self.current_goal = None;
        self.usage_state = NativeUsageState::default();
        self.memory_status = None;
        let listed = self.rpc.request(
            "events/list",
            json!({ "threadId": self.thread_id, "after": 0, "limit": 5000 }),
        )?;
        let events = listed["events"]
            .as_array()
            .ok_or_else(|| "events/list returned invalid events".to_string())?;
        for raw in events {
            let event: Event = serde_json::from_value(raw.clone())
                .map_err(|error| format!("decode event: {error}"))?;
            self.cursor = self.cursor.max(event.id);
            match &event.kind {
                EventKind::ToolCallStarted { call } => {
                    self.tool_calls.insert(call.id.clone(), call.clone());
                }
                EventKind::ThreadGoalUpdated { goal } => {
                    self.current_goal = Some(goal.clone());
                }
                EventKind::ThreadGoalCleared { .. } => {
                    self.current_goal = None;
                }
                EventKind::Diagnostic { diagnostic } => {
                    self.usage_state.observe_provider_diagnostic(diagnostic);
                }
                EventKind::MemoryStatusChanged { status } => {
                    self.memory_status = Some(status.clone());
                }
                EventKind::TodosUpdated { state } => self.todo_state = state.clone(),
                EventKind::SuggestionOffered { suggestion } if suggestion.confidence >= 0.70 => {
                    self.suggestion = Some(suggestion.clone());
                }
                EventKind::ApprovalRequested { request } => {
                    self.pending_approval = Some(PendingApproval {
                        turn_id: event.turn_id.clone().unwrap_or_default(),
                        request_id: request.id.clone(),
                        tool_call_id: request.tool_call.as_ref().map(tool_call_id),
                        tool_call: request.tool_call.clone(),
                    });
                    if render_pauses {
                        println!("{}", format_approval_panel(request));
                    }
                }
                EventKind::ApprovalResolved { decision } => {
                    if self
                        .pending_approval
                        .as_ref()
                        .is_some_and(|pending| pending.request_id == decision.request_id)
                    {
                        self.pending_approval = None;
                    }
                }
                EventKind::AskUserRequested { request } => {
                    self.pending_question = Some(PendingQuestion {
                        turn_id: event.turn_id.clone().unwrap_or_default(),
                        request: request.clone(),
                        requested_at: Instant::now(),
                        deadline_at: ask_user_deadline_at(request),
                    });
                    if render_pauses {
                        println!("{}", format_ask_user_panel(request));
                    }
                }
                EventKind::AskUserResolved { response } => {
                    if self
                        .pending_question
                        .as_ref()
                        .is_some_and(|pending| pending.request.id == response.request_id)
                    {
                        self.pending_question = None;
                    }
                }
                EventKind::TurnCompleted { .. }
                | EventKind::TurnAborted { .. }
                | EventKind::TurnInterrupted { .. } => {
                    if let Some(turn_id) = event.turn_id.as_ref() {
                        self.steered_turns.remove(turn_id);
                    }
                    if self.pending_approval.as_ref().is_some_and(|pending| {
                        Some(pending.turn_id.as_str()) == event.turn_id.as_deref()
                    }) {
                        self.pending_approval = None;
                    }
                    if self.pending_question.as_ref().is_some_and(|pending| {
                        Some(pending.turn_id.as_str()) == event.turn_id.as_deref()
                    }) {
                        self.pending_question = None;
                    }
                }
                _ => {}
            }
            if let Some(line) = self.render_event_line(&event) {
                self.ui
                    .push_event_transcript(&event, line, &self.tool_calls);
            }
            self.sync_ui_docks();
        }
        Ok(())
    }

    fn latest_handoff_summary(&mut self) -> Result<Option<String>, String> {
        let listed = self.rpc.request(
            "events/list",
            json!({ "threadId": self.thread_id, "after": 0, "limit": 5000 }),
        )?;
        let events = listed["events"]
            .as_array()
            .ok_or_else(|| "events/list returned invalid events".to_string())?;
        let mut latest = None;
        for raw in events {
            let event: Event = serde_json::from_value(raw.clone())
                .map_err(|error| format!("decode event: {error}"))?;
            if let EventKind::HandoffCompacted { summary, details } = event.kind {
                let mut line = summary;
                if let Some(details) = details {
                    line.push_str(&format!(
                        " ({} remaining, {} completed outcomes)",
                        details.remaining_todos.len(),
                        details.completed_outcomes.len()
                    ));
                }
                latest = Some(line);
            }
        }
        Ok(latest)
    }

    fn handle_effort_command(
        &mut self,
        provider: &mut ProviderConfig,
        effort: &str,
        json_output: bool,
    ) -> Result<(), String> {
        let requested = effort.trim();
        if requested.is_empty() {
            if json_output {
                return self.print_value("effort", effort_status_json(provider), json_output);
            }
            return self.print_text(&format_effort_status(provider), json_output);
        }

        let level = if requested.eq_ignore_ascii_case("auto") {
            recommended_effort_level_for_provider(provider)
        } else {
            normalize_thinking_level(requested).ok_or_else(|| {
                "Unknown effort. Use off, minimal, low, medium, high, xhigh, or auto.".to_string()
            })?
        };
        let allowed = allowed_effort_levels_for_provider(provider);
        if !allowed.contains(&level) {
            let allowed_text = allowed
                .iter()
                .map(|level| level.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            return self.print_text(
                &format!(
                    "{} does not expose '{}' effort in this slider. Allowed: {}.",
                    effort_model_name(provider),
                    level.as_str(),
                    allowed_text
                ),
                json_output,
            );
        }

        set_provider_effort_level(provider, level);
        save_reasoning_effort_setting(&self.role_profile_path, level)?;
        if json_output {
            return self.print_value("effort", effort_status_json(provider), json_output);
        }
        self.print_text(
            &format!(
                "Effort set to {} for {}.",
                effort_level_label_for_provider(provider, level),
                effort_model_name(provider)
            ),
            json_output,
        )
    }

    fn sync_permission_review_settings(&mut self) -> Result<(), String> {
        self.rpc.request(
            "permissions/settings/update",
            json!({
                "threadId": self.thread_id,
                "settings": self.permission_review_settings,
            }),
        )?;
        Ok(())
    }

    fn sync_ask_user_settings(&mut self) -> Result<(), String> {
        self.rpc.request(
            "ask-user/settings/update",
            json!({
                "threadId": self.thread_id,
                "settings": self.ask_user_settings,
            }),
        )?;
        Ok(())
    }

    fn handle_ask_user_command(&mut self, args: &str, json_output: bool) -> Result<(), String> {
        match ask_user_command_route(args)? {
            AskUserCommandRoute::Status => {
                if json_output {
                    return self.print_value(
                        "askUser",
                        json!({ "settings": self.ask_user_settings }),
                        json_output,
                    );
                }
                self.print_text(
                    &format!(
                        "ask_user: question timeout {}",
                        format_ask_user_timeout_label(self.ask_user_settings.timeout_ms)
                    ),
                    json_output,
                )
            }
            AskUserCommandRoute::SetTimeoutMs(timeout_ms) => {
                self.ask_user_settings.timeout_ms = timeout_ms;
                save_ask_user_settings(&self.role_profile_path, &self.ask_user_settings)?;
                self.sync_ask_user_settings()?;
                if json_output {
                    return self.print_value(
                        "askUser",
                        json!({ "settings": self.ask_user_settings }),
                        json_output,
                    );
                }
                self.print_text(
                    &format!(
                        "ask_user: question timeout set to {}",
                        format_ask_user_timeout_label(timeout_ms)
                    ),
                    json_output,
                )
            }
        }
    }

    fn handle_follow_up_command(&mut self, args: &str, json_output: bool) -> Result<(), String> {
        match follow_up_command_route(args)? {
            FollowUpCommandRoute::Status => {
                if json_output {
                    return self.print_value(
                        "followUp",
                        json!({
                            "mode": self.follow_up_mode.as_str(),
                            "queued": self.follow_up_queue.len(),
                        }),
                        json_output,
                    );
                }
                self.print_text(
                    &format_follow_up_status(self.follow_up_mode, self.follow_up_queue.len()),
                    json_output,
                )
            }
            FollowUpCommandRoute::SetMode(mode) => {
                self.follow_up_mode = mode;
                save_follow_up_mode_setting(&self.role_profile_path, mode)?;
                if json_output {
                    return self.print_value(
                        "followUp",
                        json!({
                            "mode": self.follow_up_mode.as_str(),
                            "queued": self.follow_up_queue.len(),
                        }),
                        json_output,
                    );
                }
                self.print_text(
                    &format!("follow-up: mode set to {}", mode.as_str()),
                    json_output,
                )
            }
        }
    }

    fn handle_steer_command(&mut self, args: &str, json_output: bool) -> Result<(), String> {
        match steer_command_route(args)? {
            SteerCommandRoute::Status => {
                if json_output {
                    return self.print_value(
                        "steer",
                        json!({
                            "mode": self.steering_mode.as_str(),
                            "queued": self.steering_queue.len(),
                        }),
                        json_output,
                    );
                }
                self.print_text(
                    &format_steering_status(self.steering_mode, self.steering_queue.len()),
                    json_output,
                )
            }
            SteerCommandRoute::SetMode(mode) => {
                self.steering_mode = mode;
                save_steering_mode_setting(&self.role_profile_path, mode)?;
                if json_output {
                    return self.print_value(
                        "steer",
                        json!({
                            "mode": self.steering_mode.as_str(),
                            "queued": self.steering_queue.len(),
                        }),
                        json_output,
                    );
                }
                self.print_text(
                    &format!("steer: mode set to {}", mode.as_str()),
                    json_output,
                )
            }
            SteerCommandRoute::Send(input) => self.steer_active_turn(&input, json_output),
        }
    }

    fn handle_startup_command(&mut self, args: &str, json_output: bool) -> Result<(), String> {
        match startup_command_route(args)? {
            StartupCommandRoute::Status => {
                if json_output {
                    return self.print_value(
                        "startup",
                        json!({ "quietStartup": self.quiet_startup }),
                        json_output,
                    );
                }
                self.print_text(&format_startup_status(self.quiet_startup), json_output)
            }
            StartupCommandRoute::SetQuietStartup(enabled) => {
                self.quiet_startup = enabled;
                save_quiet_startup_setting(&self.role_profile_path, enabled)?;
                if json_output {
                    return self.print_value(
                        "startup",
                        json!({ "quietStartup": self.quiet_startup }),
                        json_output,
                    );
                }
                self.print_text(
                    &format!("startup: quiet {}", enabled_label(enabled)),
                    json_output,
                )
            }
        }
    }

    fn handle_autocomplete_command(&mut self, args: &str, json_output: bool) -> Result<(), String> {
        match autocomplete_command_route(args)? {
            AutocompleteCommandRoute::Status => {
                if json_output {
                    return self.print_value(
                        "autocomplete",
                        json!({ "autocompleteMaxVisible": self.autocomplete_max_visible }),
                        json_output,
                    );
                }
                self.print_text(
                    &format_autocomplete_status(self.autocomplete_max_visible),
                    json_output,
                )
            }
            AutocompleteCommandRoute::SetMaxVisible(max_visible) => {
                self.autocomplete_max_visible = max_visible;
                save_autocomplete_max_visible_setting(&self.role_profile_path, max_visible)?;
                if json_output {
                    return self.print_value(
                        "autocomplete",
                        json!({ "autocompleteMaxVisible": self.autocomplete_max_visible }),
                        json_output,
                    );
                }
                self.print_text(
                    &format!("autocomplete: visible items set to {max_visible}"),
                    json_output,
                )
            }
        }
    }

    fn handle_cursor_command(&mut self, args: &str, json_output: bool) -> Result<(), String> {
        match cursor_command_route(args)? {
            CursorCommandRoute::Status => {
                if json_output {
                    return self.print_value(
                        "cursor",
                        json!({ "showHardwareCursor": self.show_hardware_cursor }),
                        json_output,
                    );
                }
                self.print_text(
                    &format_cursor_status(self.show_hardware_cursor),
                    json_output,
                )
            }
            CursorCommandRoute::SetHardwareCursor(enabled) => {
                self.show_hardware_cursor = enabled;
                save_show_hardware_cursor_setting(&self.role_profile_path, enabled)?;
                if json_output {
                    return self.print_value(
                        "cursor",
                        json!({ "showHardwareCursor": self.show_hardware_cursor }),
                        json_output,
                    );
                }
                self.print_text(
                    &format!("cursor: hardware {}", enabled_label(enabled)),
                    json_output,
                )
            }
        }
    }

    fn handle_editor_command(&mut self, args: &str, json_output: bool) -> Result<(), String> {
        match editor_command_route(args)? {
            EditorCommandRoute::Status => {
                if json_output {
                    return self.print_value(
                        "editor",
                        json!({
                            "editorPaddingX": self.editor_padding_x,
                            "doubleEscapeAction": self.double_escape_action.as_str(),
                        }),
                        json_output,
                    );
                }
                self.print_text(
                    &format_editor_status(self.editor_padding_x, self.double_escape_action),
                    json_output,
                )
            }
            EditorCommandRoute::SetPaddingX(padding_x) => {
                self.editor_padding_x = padding_x;
                save_editor_padding_x_setting(&self.role_profile_path, padding_x)?;
                if json_output {
                    return self.print_value(
                        "editor",
                        json!({ "editorPaddingX": self.editor_padding_x }),
                        json_output,
                    );
                }
                self.print_text(&format!("editor: padding {padding_x}"), json_output)
            }
            EditorCommandRoute::SetDoubleEscapeAction(action) => {
                self.double_escape_action = action;
                save_double_escape_action_setting(&self.role_profile_path, action)?;
                if json_output {
                    return self.print_value(
                        "editor",
                        json!({ "doubleEscapeAction": self.double_escape_action.as_str() }),
                        json_output,
                    );
                }
                self.print_text(
                    &format!("editor: double-escape {}", action.as_str()),
                    json_output,
                )
            }
        }
    }

    fn handle_terminal_command(&mut self, args: &str, json_output: bool) -> Result<(), String> {
        match terminal_command_route(args)? {
            TerminalCommandRoute::Status => {
                if json_output {
                    return self.print_value(
                        "terminal",
                        json!({
                            "showTerminalProgress": self.show_terminal_progress,
                            "clearOnShrink": self.terminal_clear_on_shrink,
                        }),
                        json_output,
                    );
                }
                self.print_text(
                    &format_terminal_status(
                        self.show_terminal_progress,
                        self.terminal_clear_on_shrink,
                    ),
                    json_output,
                )
            }
            TerminalCommandRoute::SetProgress(enabled) => {
                self.show_terminal_progress = enabled;
                save_show_terminal_progress_setting(&self.role_profile_path, enabled)?;
                if json_output {
                    return self.print_value(
                        "terminal",
                        json!({ "showTerminalProgress": self.show_terminal_progress }),
                        json_output,
                    );
                }
                self.print_text(
                    &format!("terminal: progress {}", enabled_label(enabled)),
                    json_output,
                )
            }
            TerminalCommandRoute::SetClearOnShrink(enabled) => {
                self.terminal_clear_on_shrink = enabled;
                save_terminal_clear_on_shrink_setting(&self.role_profile_path, enabled)?;
                if json_output {
                    return self.print_value(
                        "terminal",
                        json!({ "clearOnShrink": self.terminal_clear_on_shrink }),
                        json_output,
                    );
                }
                self.print_text(
                    &format!("terminal: clear-on-shrink {}", enabled_label(enabled)),
                    json_output,
                )
            }
        }
    }

    fn handle_footer_command(&mut self, args: &str, json_output: bool) -> Result<(), String> {
        match footer_command_route(args)? {
            FooterCommandRoute::Status => {
                if json_output {
                    return self.print_value(
                        "footer",
                        json!({ "settings": footer_settings_json(&self.footer_settings) }),
                        json_output,
                    );
                }
                self.print_text(
                    &format_footer_settings_status(&self.footer_settings),
                    json_output,
                )
            }
            FooterCommandRoute::SetBool(setting, shown) => {
                setting.apply(&mut self.footer_settings, shown);
                save_footer_settings(&self.role_profile_path, &self.footer_settings)?;
                self.sync_ui_docks();
                if json_output {
                    return self.print_value(
                        "footer",
                        json!({ "settings": footer_settings_json(&self.footer_settings) }),
                        json_output,
                    );
                }
                self.print_text(
                    &format!(
                        "footer: {} {}",
                        setting.label(),
                        footer_visibility_label(shown)
                    ),
                    json_output,
                )
            }
            FooterCommandRoute::SetUsageDisplay(display) => {
                self.footer_settings.usage_display = display;
                save_footer_settings(&self.role_profile_path, &self.footer_settings)?;
                self.sync_ui_docks();
                if json_output {
                    return self.print_value(
                        "footer",
                        json!({ "settings": footer_settings_json(&self.footer_settings) }),
                        json_output,
                    );
                }
                self.print_text(
                    &format!("footer: usage display {}", display.label()),
                    json_output,
                )
            }
        }
    }

    fn handle_images_command(&mut self, args: &str, json_output: bool) -> Result<(), String> {
        match image_settings_command_route(args)? {
            ImageSettingsCommandRoute::Status => {
                if json_output {
                    return self.print_value(
                        "images",
                        json!({ "settings": image_settings_json(&self.image_settings) }),
                        json_output,
                    );
                }
                self.print_text(
                    &format_image_settings_status(&self.image_settings),
                    json_output,
                )
            }
            ImageSettingsCommandRoute::Capabilities => {
                let capabilities = image_preview_capabilities_from_env(&self.image_settings);
                if json_output {
                    return self.print_value(
                        "images",
                        json!({
                            "settings": image_settings_json(&self.image_settings),
                            "capabilities": capabilities,
                        }),
                        json_output,
                    );
                }
                self.print_text(
                    &format_image_preview_capabilities(&capabilities),
                    json_output,
                )
            }
            ImageSettingsCommandRoute::SetShowImages(shown) => {
                self.image_settings.show_images = shown;
                save_image_settings(&self.role_profile_path, &self.image_settings)?;
                if json_output {
                    return self.print_value(
                        "images",
                        json!({ "settings": image_settings_json(&self.image_settings) }),
                        json_output,
                    );
                }
                self.print_text(
                    &format!("images: previews {}", image_visibility_label(shown)),
                    json_output,
                )
            }
            ImageSettingsCommandRoute::SetImageWidthCells(width) => {
                self.image_settings.image_width_cells = width;
                save_image_settings(&self.role_profile_path, &self.image_settings)?;
                if json_output {
                    return self.print_value(
                        "images",
                        json!({ "settings": image_settings_json(&self.image_settings) }),
                        json_output,
                    );
                }
                self.print_text(&format!("images: width {width} cells"), json_output)
            }
            ImageSettingsCommandRoute::SetAutoResizeImages(enabled) => {
                self.image_settings.auto_resize_images = enabled;
                save_image_settings(&self.role_profile_path, &self.image_settings)?;
                if json_output {
                    return self.print_value(
                        "images",
                        json!({ "settings": image_settings_json(&self.image_settings) }),
                        json_output,
                    );
                }
                self.print_text(
                    &format!("images: auto-resize {}", enabled_label(enabled)),
                    json_output,
                )
            }
            ImageSettingsCommandRoute::SetBlockImages(blocked) => {
                self.image_settings.block_images = blocked;
                save_image_settings(&self.role_profile_path, &self.image_settings)?;
                if json_output {
                    return self.print_value(
                        "images",
                        json!({ "settings": image_settings_json(&self.image_settings) }),
                        json_output,
                    );
                }
                self.print_text(
                    &format!(
                        "images: provider image sending {}",
                        image_block_label(blocked)
                    ),
                    json_output,
                )
            }
            ImageSettingsCommandRoute::SetPreviewProtocol(protocol) => {
                self.image_settings.preview_protocol = protocol;
                save_image_settings(&self.role_profile_path, &self.image_settings)?;
                if json_output {
                    return self.print_value(
                        "images",
                        json!({ "settings": image_settings_json(&self.image_settings) }),
                        json_output,
                    );
                }
                self.print_text(
                    &format!("images: preview protocol {}", protocol.as_str()),
                    json_output,
                )
            }
        }
    }

    fn handle_compaction_command(
        &mut self,
        provider: &ProviderConfig,
        args: &str,
        json_output: bool,
    ) -> Result<(), String> {
        match compaction_command_route(args)? {
            CompactionCommandRoute::Status => {
                let telemetry = self.compaction_trigger_telemetry_for_provider(provider);
                if json_output {
                    return self.print_value(
                        "compaction",
                        json!({
                            "settings": oppi_compact_settings_json(&self.compact_settings),
                            "telemetry": compaction_trigger_telemetry_json(&telemetry),
                        }),
                        json_output,
                    );
                }
                self.print_text(
                    &format_oppi_compact_settings_status(&self.compact_settings, Some(&telemetry)),
                    json_output,
                )
            }
            CompactionCommandRoute::CompactNow => self.handle_memory_compact(&[], json_output),
            CompactionCommandRoute::SetIdleEnabled(enabled) => {
                self.compact_settings.idle_enabled = enabled;
                save_oppi_compact_settings(&self.role_profile_path, &self.compact_settings)?;
                self.sync_ui_docks();
                if json_output {
                    return self.print_value(
                        "compaction",
                        json!({ "settings": oppi_compact_settings_json(&self.compact_settings) }),
                        json_output,
                    );
                }
                self.print_text(
                    &format!("compaction: idle {}", compaction_enabled_label(enabled)),
                    json_output,
                )
            }
            CompactionCommandRoute::SetIdleMinutes(minutes) => {
                self.compact_settings.idle_minutes = minutes;
                save_oppi_compact_settings(&self.role_profile_path, &self.compact_settings)?;
                self.sync_ui_docks();
                if json_output {
                    return self.print_value(
                        "compaction",
                        json!({ "settings": oppi_compact_settings_json(&self.compact_settings) }),
                        json_output,
                    );
                }
                self.print_text(&format!("compaction: idle time {}m", minutes), json_output)
            }
            CompactionCommandRoute::SetIdleThresholdPercent(percent) => {
                self.compact_settings.idle_threshold_percent = percent;
                save_oppi_compact_settings(&self.role_profile_path, &self.compact_settings)?;
                self.sync_ui_docks();
                if json_output {
                    return self.print_value(
                        "compaction",
                        json!({ "settings": oppi_compact_settings_json(&self.compact_settings) }),
                        json_output,
                    );
                }
                self.print_text(
                    &format!("compaction: idle threshold {}%", percent),
                    json_output,
                )
            }
            CompactionCommandRoute::SetSmartThresholdPercent(percent) => {
                self.compact_settings.smart_threshold_percent = percent;
                save_oppi_compact_settings(&self.role_profile_path, &self.compact_settings)?;
                self.sync_ui_docks();
                if json_output {
                    return self.print_value(
                        "compaction",
                        json!({ "settings": oppi_compact_settings_json(&self.compact_settings) }),
                        json_output,
                    );
                }
                self.print_text(
                    &format!("compaction: smart threshold {}%", percent),
                    json_output,
                )
            }
        }
    }

    fn handle_permissions_command(&mut self, args: &str, json_output: bool) -> Result<(), String> {
        match permissions_command_route(args)? {
            PermissionCommandRoute::Status => {}
            PermissionCommandRoute::History => {
                let value = self.rpc.request(
                    "permissions/history",
                    json!({
                        "threadId": self.thread_id,
                        "limit": 20,
                    }),
                )?;
                if json_output {
                    return self.print_value("permissions", value, json_output);
                }
                return self.print_text(&format_permission_review_history(&value), json_output);
            }
            PermissionCommandRoute::ReviewContext => {
                let value = self.rpc.request(
                    "permissions/context",
                    json!({
                        "threadId": self.thread_id,
                    }),
                )?;
                if json_output {
                    return self.print_value("permissions", value, json_output);
                }
                return self.print_text(&format_permission_review_context(&value), json_output);
            }
            PermissionCommandRoute::ClearSession => {
                let value = self.rpc.request(
                    "permissions/clear-session",
                    json!({ "threadId": self.thread_id }),
                )?;
                if json_output {
                    return self.print_value("permissions", value, json_output);
                }
                return self.print_text(
                    value
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("permission review session state cleared"),
                    json_output,
                );
            }
            PermissionCommandRoute::SetReviewTimeoutMs(timeout_ms) => {
                self.permission_review_settings.timeout_ms = timeout_ms;
                save_permission_review_settings(
                    &self.role_profile_path,
                    &self.permission_review_settings,
                )?;
                self.sync_permission_review_settings()?;
                if json_output {
                    return self.print_value(
                        "permissions",
                        json!({ "settings": self.permission_review_settings }),
                        json_output,
                    );
                }
                return self.print_text(
                    &format!(
                        "permissions: reviewer timeout set to {}s",
                        timeout_ms / 1_000
                    ),
                    json_output,
                );
            }
            PermissionCommandRoute::SetReviewerModel(reviewer_model) => {
                self.permission_review_settings.reviewer_model = reviewer_model;
                save_permission_review_settings(
                    &self.role_profile_path,
                    &self.permission_review_settings,
                )?;
                self.sync_permission_review_settings()?;
                if json_output {
                    return self.print_value(
                        "permissions",
                        json!({ "settings": self.permission_review_settings }),
                        json_output,
                    );
                }
                return self.print_text(
                    &format!(
                        "permissions: reviewer model set to {}",
                        self.permission_review_settings
                            .reviewer_model
                            .as_deref()
                            .unwrap_or("auto")
                    ),
                    json_output,
                );
            }
            PermissionCommandRoute::ValidateSandbox => {
                let validation = permission_sandbox_validation_value(
                    self.permission_mode,
                    &self.cwd,
                    &self.permission_sandbox_settings,
                );
                if json_output {
                    return self.print_value("permissions", validation, json_output);
                }
                return self.print_text(
                    &format_permission_sandbox_validation(&validation),
                    json_output,
                );
            }
            PermissionCommandRoute::SetProtectedPatterns(protected_patterns) => {
                self.permission_sandbox_settings.protected_patterns = protected_patterns;
                save_permission_sandbox_settings(
                    &self.role_profile_path,
                    &self.permission_sandbox_settings,
                )?;
                if json_output {
                    return self.print_value(
                        "permissions",
                        json!({
                            "sandboxSettings": permission_sandbox_settings_json(
                                &self.permission_sandbox_settings
                            )
                        }),
                        json_output,
                    );
                }
                return self.print_text(
                    &format!(
                        "permissions: protected patterns set to {}",
                        format_protected_patterns_label(
                            &self.permission_sandbox_settings.protected_patterns
                        )
                    ),
                    json_output,
                );
            }
            PermissionCommandRoute::SetSandboxInternet(allowed) => {
                self.permission_sandbox_settings.allow_internet = allowed;
                save_permission_sandbox_settings(
                    &self.role_profile_path,
                    &self.permission_sandbox_settings,
                )?;
                if json_output {
                    return self.print_value(
                        "permissions",
                        json!({
                            "sandboxSettings": permission_sandbox_settings_json(
                                &self.permission_sandbox_settings
                            )
                        }),
                        json_output,
                    );
                }
                return self.print_text(
                    &format!(
                        "permissions: sandbox internet {}",
                        if allowed { "allowed" } else { "blocked" }
                    ),
                    json_output,
                );
            }
            PermissionCommandRoute::SetSandboxSsh(allowed) => {
                self.permission_sandbox_settings.allow_ssh = allowed;
                save_permission_sandbox_settings(
                    &self.role_profile_path,
                    &self.permission_sandbox_settings,
                )?;
                if json_output {
                    return self.print_value(
                        "permissions",
                        json!({
                            "sandboxSettings": permission_sandbox_settings_json(
                                &self.permission_sandbox_settings
                            )
                        }),
                        json_output,
                    );
                }
                return self.print_text(
                    &format!(
                        "permissions: sandbox ssh {}",
                        if allowed { "allowed" } else { "blocked" }
                    ),
                    json_output,
                );
            }
            PermissionCommandRoute::SetSandboxRootsHere => {
                apply_permission_sandbox_root_preset(
                    &mut self.permission_sandbox_settings,
                    PermissionSandboxRootPreset::Here,
                    &self.cwd,
                );
                save_permission_sandbox_settings(
                    &self.role_profile_path,
                    &self.permission_sandbox_settings,
                )?;
                if json_output {
                    return self.print_value(
                        "permissions",
                        json!({
                            "sandboxSettings": permission_sandbox_settings_json(
                                &self.permission_sandbox_settings
                            )
                        }),
                        json_output,
                    );
                }
                return self.print_text(
                    &format!(
                        "permissions: sandbox roots set to current workspace {}",
                        self.cwd
                    ),
                    json_output,
                );
            }
            PermissionCommandRoute::SetSandboxRootsDefault => {
                apply_permission_sandbox_root_preset(
                    &mut self.permission_sandbox_settings,
                    PermissionSandboxRootPreset::Default,
                    &self.cwd,
                );
                save_permission_sandbox_settings(
                    &self.role_profile_path,
                    &self.permission_sandbox_settings,
                )?;
                if json_output {
                    return self.print_value(
                        "permissions",
                        json!({
                            "sandboxSettings": permission_sandbox_settings_json(
                                &self.permission_sandbox_settings
                            )
                        }),
                        json_output,
                    );
                }
                return self.print_text(
                    "permissions: sandbox roots reset to default fallback behavior",
                    json_output,
                );
            }
            PermissionCommandRoute::SetSandboxProjectRoots(roots) => {
                self.permission_sandbox_settings.project_roots = roots;
                save_permission_sandbox_settings(
                    &self.role_profile_path,
                    &self.permission_sandbox_settings,
                )?;
                if json_output {
                    return self.print_value(
                        "permissions",
                        json!({
                            "sandboxSettings": permission_sandbox_settings_json(
                                &self.permission_sandbox_settings
                            )
                        }),
                        json_output,
                    );
                }
                return self.print_text(
                    &format!(
                        "permissions: sandbox project roots set to {}",
                        format_sandbox_roots_label(
                            &self.permission_sandbox_settings.project_roots,
                            "default"
                        )
                    ),
                    json_output,
                );
            }
            PermissionCommandRoute::SetSandboxReadableRoots(roots) => {
                self.permission_sandbox_settings.readable_roots = roots;
                save_permission_sandbox_settings(
                    &self.role_profile_path,
                    &self.permission_sandbox_settings,
                )?;
                if json_output {
                    return self.print_value(
                        "permissions",
                        json!({
                            "sandboxSettings": permission_sandbox_settings_json(
                                &self.permission_sandbox_settings
                            )
                        }),
                        json_output,
                    );
                }
                return self.print_text(
                    &format!(
                        "permissions: sandbox readable roots set to {}",
                        format_sandbox_roots_label(
                            &self.permission_sandbox_settings.readable_roots,
                            "project roots"
                        )
                    ),
                    json_output,
                );
            }
            PermissionCommandRoute::SetSandboxWritableRoots(roots) => {
                self.permission_sandbox_settings.writable_roots = roots;
                save_permission_sandbox_settings(
                    &self.role_profile_path,
                    &self.permission_sandbox_settings,
                )?;
                if json_output {
                    return self.print_value(
                        "permissions",
                        json!({
                            "sandboxSettings": permission_sandbox_settings_json(
                                &self.permission_sandbox_settings
                            )
                        }),
                        json_output,
                    );
                }
                return self.print_text(
                    &format!(
                        "permissions: sandbox writable roots set to {}",
                        format_sandbox_roots_label(
                            &self.permission_sandbox_settings.writable_roots,
                            "project roots"
                        )
                    ),
                    json_output,
                );
            }
            PermissionCommandRoute::EditSandboxRoots { kind, edit, roots } => {
                let (label, fallback) = permission_sandbox_root_list_labels(kind);
                let changed = apply_permission_sandbox_root_list_edit(
                    permission_sandbox_root_list_mut(&mut self.permission_sandbox_settings, kind),
                    edit,
                    &roots,
                );
                save_permission_sandbox_settings(
                    &self.role_profile_path,
                    &self.permission_sandbox_settings,
                )?;
                if json_output {
                    return self.print_value(
                        "permissions",
                        json!({
                            "changed": changed,
                            "sandboxSettings": permission_sandbox_settings_json(
                                &self.permission_sandbox_settings
                            )
                        }),
                        json_output,
                    );
                }
                let roots_label = format_sandbox_roots_label(
                    permission_sandbox_root_list_mut(&mut self.permission_sandbox_settings, kind),
                    fallback,
                );
                return self.print_text(
                    &format!(
                        "permissions: sandbox {label} roots {} {}; now {}",
                        match edit {
                            PermissionSandboxRootListEdit::Add => "added",
                            PermissionSandboxRootListEdit::Remove => "removed",
                        },
                        format_sandbox_roots_label(&roots, "nothing"),
                        roots_label
                    ),
                    json_output,
                );
            }
            PermissionCommandRoute::SetMode(mode) => {
                self.permission_mode = mode;
                self.permission_mode_source = "session/settings".to_string();
                save_permission_mode_setting(&self.role_profile_path, mode)?;
            }
        }
        let sandbox = self.rpc.request("sandbox/status", json!({}))?;
        if json_output {
            self.print_value(
                "permissions",
                json!({
                    "mode": self.permission_mode,
                    "source": self.permission_mode_source.as_str(),
                    "policy": sandbox_policy_json_with_settings(
                        self.permission_mode,
                        &self.cwd,
                        &self.permission_sandbox_settings,
                    ),
                    "reviewSettings": self.permission_review_settings,
                    "sandboxSettings": permission_sandbox_settings_json(
                        &self.permission_sandbox_settings
                    ),
                    "sandbox": sandbox,
                }),
                json_output,
            )?;
        } else {
            let rendered = format!(
                "permissions: {} (source: {})\npolicy: filesystem={}, network={}\nreviewer: timeout={}s, model={}\nsandbox flags: internet={}, ssh={}\nsandbox roots: project={}, read={}, write={}\nprotected patterns: {}\nsandbox: {}",
                self.permission_mode.as_str(),
                self.permission_mode_source,
                filesystem_policy_for_mode(self.permission_mode),
                network_policy_label_for_settings(
                    self.permission_mode,
                    &self.permission_sandbox_settings
                ),
                self.permission_review_settings.timeout_ms / 1_000,
                self.permission_review_settings
                    .reviewer_model
                    .as_deref()
                    .unwrap_or("auto"),
                format_sandbox_flag_label(self.permission_sandbox_settings.allow_internet),
                format_sandbox_flag_label(self.permission_sandbox_settings.allow_ssh),
                format_sandbox_roots_label(
                    &self.permission_sandbox_settings.project_roots,
                    "default"
                ),
                format_sandbox_roots_label(
                    &self.permission_sandbox_settings.readable_roots,
                    "project roots"
                ),
                format_sandbox_roots_label(
                    &self.permission_sandbox_settings.writable_roots,
                    "project roots"
                ),
                format_protected_patterns_label(
                    &self.permission_sandbox_settings.protected_patterns
                ),
                sandbox
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("status unavailable")
            );
            self.print_text(&rendered, json_output)?;
        }
        Ok(())
    }

    fn mcp_list_value(&mut self) -> Result<Value, String> {
        self.rpc
            .request("mcp/list", json!({ "threadId": self.thread_id }))
    }

    fn update_mcp_summary(&mut self, value: &Value) {
        self.mcp_summary = Some(format_mcp_status_summary(value));
        self.sync_ui_docks();
    }

    fn handle_mcp_action(
        &mut self,
        action: &str,
        server_id: &str,
        json_output: bool,
    ) -> Result<(), String> {
        let method = match action {
            "connect" => "mcp/connect",
            "test" => "mcp/test",
            "reload" => "mcp/reload",
            "auth" => "mcp/auth",
            _ => return self.print_text(mcp_command_usage(), json_output),
        };
        let value = self.rpc.request(
            method,
            json!({
                "threadId": self.thread_id,
                "serverId": server_id,
            }),
        )?;
        if json_output {
            return self.print_value("mcp", value, json_output);
        }
        let listed = self.mcp_list_value()?;
        self.update_mcp_summary(&listed);
        let mut rendered = format_mcp_action_result(action, server_id, &value);
        let status = format_mcp_status_list(&listed);
        if !status.trim().is_empty() {
            rendered.push('\n');
            rendered.push_str(&status);
        }
        self.print_text(&rendered, json_output)
    }

    fn handle_mcp_resources(&mut self, server_id: &str, json_output: bool) -> Result<(), String> {
        let value = self.rpc.request(
            "mcp/resources/list",
            json!({
                "threadId": self.thread_id,
                "serverId": server_id,
            }),
        )?;
        if json_output {
            self.print_value("mcp-resources", value, json_output)
        } else {
            self.print_text(&format_mcp_resources_list(&value), json_output)
        }
    }

    fn handle_mcp_resource_templates(
        &mut self,
        server_id: &str,
        cursor: Option<&str>,
        json_output: bool,
    ) -> Result<(), String> {
        let value = self.rpc.request(
            "mcp/resources/templates/list",
            mcp_list_request_params_json(&self.thread_id, server_id, cursor),
        )?;
        if json_output {
            self.print_value("mcp-resource-templates", value, json_output)
        } else {
            self.print_text(&format_mcp_resource_templates_list(&value), json_output)
        }
    }

    fn handle_mcp_prompts(
        &mut self,
        server_id: &str,
        cursor: Option<&str>,
        json_output: bool,
    ) -> Result<(), String> {
        let value = self.rpc.request(
            "mcp/prompts/list",
            mcp_list_request_params_json(&self.thread_id, server_id, cursor),
        )?;
        if json_output {
            self.print_value("mcp-prompts", value, json_output)
        } else {
            self.print_text(&format_mcp_prompts_list(&value), json_output)
        }
    }

    fn handle_mcp_prompt_get(
        &mut self,
        server_id: &str,
        name: &str,
        rest: &[&str],
        json_output: bool,
    ) -> Result<(), String> {
        let arguments = parse_mcp_prompt_arguments(rest)?;
        let value = self.rpc.request(
            "mcp/prompts/get",
            json!({
                "threadId": self.thread_id,
                "serverId": server_id,
                "name": name,
                "arguments": arguments,
            }),
        )?;
        if json_output {
            self.print_value("mcp-prompt", value, json_output)
        } else {
            self.print_text(&format_mcp_prompt_get_result(&value), json_output)
        }
    }

    fn handle_mcp_resource_read(
        &mut self,
        server_id: &str,
        uri: &str,
        json_output: bool,
    ) -> Result<(), String> {
        let value = self.rpc.request(
            "mcp/resources/read",
            json!({
                "threadId": self.thread_id,
                "serverId": server_id,
                "uri": uri,
            }),
        )?;
        if json_output {
            self.print_value("mcp-resource", value, json_output)
        } else {
            self.print_text(&format_mcp_resource_read_result(&value), json_output)
        }
    }

    fn handle_mcp_resource_subscription(
        &mut self,
        action: &str,
        server_id: &str,
        uri: &str,
        json_output: bool,
    ) -> Result<(), String> {
        let method = match action {
            "subscribe" => "mcp/resources/subscribe",
            "unsubscribe" => "mcp/resources/unsubscribe",
            _ => return self.print_text(mcp_command_usage(), json_output),
        };
        let value = self.rpc.request(
            method,
            json!({
                "threadId": self.thread_id,
                "serverId": server_id,
                "uri": uri,
            }),
        )?;
        if json_output {
            self.print_value("mcp-resource-subscription", value, json_output)
        } else {
            self.print_text(
                &format_mcp_resource_subscription_result(action, &value),
                json_output,
            )
        }
    }

    fn handle_plugin_mcp_trust(
        &mut self,
        plugin_id: &str,
        server_id: &str,
        enabled: bool,
        trusted: bool,
        json_output: bool,
    ) -> Result<(), String> {
        let value = self.rpc.request(
            "plugin/mcp/trust",
            json!({
                "threadId": self.thread_id,
                "pluginId": plugin_id,
                "serverId": server_id,
                "enabled": enabled,
                "trusted": trusted,
            }),
        )?;
        if json_output {
            return self.print_value("mcp", value, json_output);
        }
        self.update_mcp_summary(&value);
        let action = if enabled && trusted {
            "trust-plugin"
        } else {
            "disable-plugin"
        };
        let mut rendered = format!("mcp {action} {plugin_id}/{server_id}: ok");
        let status = format_mcp_status_list(&value);
        if !status.trim().is_empty() {
            rendered.push('\n');
            rendered.push_str(&status);
        }
        self.print_text(&rendered, json_output)
    }

    fn handle_mcp_command(&mut self, args: Vec<&str>, json_output: bool) -> Result<(), String> {
        match args.as_slice() {
            [] | ["list"] | ["status"] => {
                let value = self.mcp_list_value()?;
                self.update_mcp_summary(&value);
                if json_output {
                    self.print_value("mcp", value, json_output)
                } else {
                    self.print_text(&format_mcp_status_list(&value), json_output)
                }
            }
            [action @ ("connect" | "test" | "reload" | "auth"), server_id] => {
                self.handle_mcp_action(action, server_id, json_output)
            }
            ["resources", server_id] => self.handle_mcp_resources(server_id, json_output),
            ["templates", server_id] => {
                self.handle_mcp_resource_templates(server_id, None, json_output)
            }
            ["prompts", server_id] => self.handle_mcp_prompts(server_id, None, json_output),
            ["prompt", server_id, name, rest @ ..] => {
                self.handle_mcp_prompt_get(server_id, name, rest, json_output)
            }
            ["read", server_id, uri @ ..] if !uri.is_empty() => {
                let uri = uri.join(" ");
                self.handle_mcp_resource_read(server_id, &uri, json_output)
            }
            [action @ ("subscribe" | "unsubscribe"), server_id, uri @ ..] if !uri.is_empty() => {
                let uri = uri.join(" ");
                self.handle_mcp_resource_subscription(action, server_id, &uri, json_output)
            }
            ["trust-plugin", plugin_id, server_id] => {
                self.handle_plugin_mcp_trust(plugin_id, server_id, true, true, json_output)
            }
            ["disable-plugin", plugin_id, server_id] => {
                self.handle_plugin_mcp_trust(plugin_id, server_id, false, false, json_output)
            }
            ["help"] | ["--help"] | ["-h"] => self.print_text(mcp_command_usage(), json_output),
            _ => self.print_text(mcp_command_usage(), json_output),
        }
    }

    fn handle_memory_command(
        &mut self,
        provider: &ProviderConfig,
        args: Vec<&str>,
        json_output: bool,
    ) -> Result<(), String> {
        match args.as_slice() {
            [] | ["status"] => {
                let value = self.rpc.request("memory/status", json!({}))?;
                if json_output {
                    self.print_value(
                        "memory",
                        json!({ "runtime": value, "settings": memory_settings_json(&self.memory_settings) }),
                        json_output,
                    )
                } else {
                    let rendered = format!(
                        "{}\n{}",
                        format_memory_status_summary(&value),
                        format_memory_settings_status(&self.memory_settings)
                    );
                    self.print_text(&rendered, json_output)
                }
            }
            ["on"] | ["off"] => {
                let enabled = args[0] == "on";
                self.memory_settings.enabled = enabled;
                self.reset_hoppi_background_sync_timer();
                save_memory_settings(&self.role_profile_path, &self.memory_settings)?;
                let value = self.rpc.request(
                    "memory/set",
                    json!({
                        "threadId": self.thread_id,
                        "status": { "enabled": enabled, "backend": if enabled { "client" } else { "none" }, "scope": "project", "memoryCount": 0 }
                    }),
                )?;
                if json_output {
                    self.print_value(
                        "memory",
                        json!({ "runtime": value, "settings": memory_settings_json(&self.memory_settings) }),
                        json_output,
                    )
                } else {
                    self.print_text(&format!("memory: {}", enabled_label(enabled)), json_output)
                }
            }
            ["startup-recall", value] | ["startup", "recall", value] => {
                let enabled = parse_memory_settings_bool(value)?;
                self.memory_settings.startup_recall = enabled;
                save_memory_settings(&self.role_profile_path, &self.memory_settings)?;
                self.print_memory_settings_update("startup recall", enabled, json_output)
            }
            ["task-start-recall", value] | ["task", "start", "recall", value] => {
                let enabled = parse_memory_settings_bool(value)?;
                self.memory_settings.task_start_recall = enabled;
                save_memory_settings(&self.role_profile_path, &self.memory_settings)?;
                self.print_memory_settings_update("task-start recall", enabled, json_output)
            }
            ["turn-summaries", value] | ["turn-summary", value] | ["summaries", value] => {
                let enabled = parse_memory_settings_bool(value)?;
                self.memory_settings.turn_summaries = enabled;
                save_memory_settings(&self.role_profile_path, &self.memory_settings)?;
                self.print_memory_settings_update("turn summaries", enabled, json_output)
            }
            ["agent-model", value] | ["memory-agent-model", value] => {
                let model = parse_memory_agent_model(value)?;
                self.memory_settings.agent_model = model.to_string();
                save_memory_settings(&self.role_profile_path, &self.memory_settings)?;
                self.print_memory_settings_value_update("agent model", model, json_output)
            }
            ["deep-model", value] | ["deep-dream-model", value] => {
                let model = parse_deep_memory_model(value)?;
                self.memory_settings.deep_model = model.to_string();
                save_memory_settings(&self.role_profile_path, &self.memory_settings)?;
                self.print_memory_settings_value_update("deep dream model", model, json_output)
            }
            ["idle-dream", value] | ["idle-consolidation", value] => {
                let enabled = parse_memory_settings_bool(value)?;
                self.memory_settings.idle_consolidation = enabled;
                save_memory_settings(&self.role_profile_path, &self.memory_settings)?;
                self.print_memory_settings_update("idle dream mode", enabled, json_output)
            }
            ["install-offer", value] | ["first-start-install-offer", value] => {
                let offer = parse_hoppi_install_offer(value)?;
                self.memory_settings.hoppi_install_offer = offer.to_string();
                save_memory_settings(&self.role_profile_path, &self.memory_settings)?;
                self.print_memory_settings_value_update(
                    "first-start install offer",
                    offer,
                    json_output,
                )
            }
            ["sync", value] => {
                let enabled = parse_memory_settings_bool(value)?;
                self.memory_settings.sync_enabled = enabled;
                self.reset_hoppi_background_sync_timer();
                save_memory_settings(&self.role_profile_path, &self.memory_settings)?;
                self.print_memory_settings_update("sync", enabled, json_output)
            }
            ["sync", "repo", rest @ ..] | ["sync-repo", rest @ ..] => {
                self.memory_settings.sync_repo_path = parse_memory_sync_optional_value(rest)?;
                save_memory_settings(&self.role_profile_path, &self.memory_settings)?;
                self.print_memory_settings_text_update(
                    "sync repo",
                    optional_memory_setting_label(self.memory_settings.sync_repo_path.as_deref()),
                    json_output,
                )
            }
            ["sync", "remote", rest @ ..]
            | ["sync", "url", rest @ ..]
            | ["sync-remote", rest @ ..]
            | ["sync-url", rest @ ..] => {
                self.memory_settings.sync_repo_url = parse_memory_sync_optional_value(rest)?;
                save_memory_settings(&self.role_profile_path, &self.memory_settings)?;
                self.print_memory_settings_text_update(
                    "sync remote",
                    optional_memory_setting_label(self.memory_settings.sync_repo_url.as_deref()),
                    json_output,
                )
            }
            ["sync", "pull-startup", value] | ["sync-pull-startup", value] => {
                let enabled = parse_memory_settings_bool(value)?;
                self.memory_settings.sync_pull_on_startup = enabled;
                save_memory_settings(&self.role_profile_path, &self.memory_settings)?;
                self.print_memory_settings_update("sync startup pull", enabled, json_output)
            }
            ["sync", "push-exit", value] | ["sync-push-exit", value] => {
                let enabled = parse_memory_settings_bool(value)?;
                self.memory_settings.sync_push_on_exit = enabled;
                save_memory_settings(&self.role_profile_path, &self.memory_settings)?;
                self.print_memory_settings_update("sync exit push", enabled, json_output)
            }
            ["sync", "background", value] | ["sync-background", value] => {
                let value = parse_sync_background(value)?;
                self.memory_settings.sync_background = value.to_string();
                self.reset_hoppi_background_sync_timer();
                save_memory_settings(&self.role_profile_path, &self.memory_settings)?;
                self.print_memory_settings_value_update("sync background", value, json_output)
            }
            ["sync", "encryption", value] | ["sync-encryption", value] => {
                let (encryption, source) = parse_sync_encryption(value)?;
                self.memory_settings.sync_encryption = encryption.to_string();
                self.memory_settings.sync_passphrase_source = source.to_string();
                save_memory_settings(&self.role_profile_path, &self.memory_settings)?;
                self.print_memory_settings_text_update(
                    "sync encryption",
                    format!("{encryption}/{source}"),
                    json_output,
                )
            }
            ["sync", "passphrase-env", value] | ["sync-passphrase-env", value] => {
                let value = parse_sync_passphrase_env(value)?;
                self.memory_settings.sync_encryption = "passphrase".to_string();
                self.memory_settings.sync_passphrase_source = "env".to_string();
                self.memory_settings.sync_passphrase_env = value.to_string();
                save_memory_settings(&self.role_profile_path, &self.memory_settings)?;
                self.print_memory_settings_value_update("sync passphrase env", value, json_output)
            }
            ["sync", "passphrase-file", rest @ ..] | ["sync-passphrase-file", rest @ ..] => {
                self.memory_settings.sync_passphrase_file = parse_memory_sync_optional_value(rest)?;
                if self.memory_settings.sync_passphrase_file.is_some() {
                    self.memory_settings.sync_encryption = "passphrase".to_string();
                    self.memory_settings.sync_passphrase_source = "file".to_string();
                }
                save_memory_settings(&self.role_profile_path, &self.memory_settings)?;
                self.print_memory_settings_text_update(
                    "sync passphrase file",
                    optional_memory_setting_label(
                        self.memory_settings.sync_passphrase_file.as_deref(),
                    ),
                    json_output,
                )
            }
            ["sync", "conflict", value] | ["sync-conflict", value] => {
                let value = parse_sync_conflict_mode(value)?;
                self.memory_settings.sync_conflict_mode = value.to_string();
                save_memory_settings(&self.role_profile_path, &self.memory_settings)?;
                self.print_memory_settings_value_update("sync conflict mode", value, json_output)
            }
            ["list"] | ["memories"] => self.handle_memory_list(false, false, json_output),
            ["list", "pinned"] | ["memories", "pinned"] => {
                self.handle_memory_list(false, true, json_output)
            }
            ["list", "all"] | ["memories", "all"] => {
                self.handle_memory_list(true, false, json_output)
            }
            ["jobs"] | ["queue"] => self.handle_memory_jobs(false, json_output),
            ["jobs", "all"] | ["queue", "all"] => self.handle_memory_jobs(true, json_output),
            ["jobs", "cancel", id] | ["queue", "cancel", id] => {
                self.handle_memory_job_cancel(id, json_output)
            }
            ["forget", id, "--yes"] | ["delete", id, "--yes"] => {
                self.handle_memory_forget(id, json_output)
            }
            ["forget", id] | ["delete", id] => self.print_text(
                &format!(
                    "Forgetting a memory requires explicit confirmation: /memory forget {id} --yes"
                ),
                json_output,
            ),
            ["pin", id, "--yes"] => self.handle_memory_pin(id, true, json_output),
            ["unpin", id, "--yes"] => self.handle_memory_pin(id, false, json_output),
            ["pin", id] => self.print_text(
                &format!("Pinning a memory requires explicit confirmation: /memory pin {id} --yes"),
                json_output,
            ),
            ["unpin", id] => self.print_text(
                &format!(
                    "Unpinning a memory requires explicit confirmation: /memory unpin {id} --yes"
                ),
                json_output,
            ),
            ["dashboard"] => self.handle_memory_dashboard(json_output),
            ["settings"] => self.handle_memory_control("settings", false, json_output),
            ["maintain"]
            | ["maintenance"]
            | ["maintenance", "dry-run"]
            | ["maintain", "dry-run"] => {
                self.handle_memory_control("maintenance", false, json_output)
            }
            ["maintenance", "apply"] | ["maintain", "apply"] => {
                self.handle_memory_control("maintenance", true, json_output)
            }
            ["compaction", rest @ ..] | ["compact-settings", rest @ ..] => {
                self.handle_compaction_command(provider, &rest.join(" "), json_output)
            }
            ["compact", rest @ ..] => self.handle_memory_compact(rest, json_output),
            _ => self.print_text(memory_settings_usage(), json_output),
        }
    }

    fn handle_memory_compact(&mut self, rest: &[&str], json_output: bool) -> Result<(), String> {
        let summary = memory_compact_summary_arg(rest);
        let value = self.rpc.request(
            "memory/compact",
            json!({ "threadId": self.thread_id, "summary": summary }),
        )?;
        self.print_value("memoryCompact", value, json_output)
    }

    fn handle_memory_dashboard(&mut self, json_output: bool) -> Result<(), String> {
        let params =
            memory_backend_dashboard_params_from_settings(&self.memory_settings, &self.thread_id)?;
        let value = self.request_memory_backend_normalized("memory/backend/dashboard", params)?;
        if json_output {
            self.print_value("memoryDashboard", value, json_output)
        } else {
            let rendered = format_memory_dashboard(&value);
            self.print_text(&rendered, json_output)
        }
    }

    fn handle_memory_list(
        &mut self,
        include_superseded: bool,
        pinned_only: bool,
        json_output: bool,
    ) -> Result<(), String> {
        let params = memory_backend_list_params_from_settings(
            &self.memory_settings,
            &self.thread_id,
            include_superseded,
            pinned_only,
            Some(50),
        )?;
        let value = self.request_memory_backend_normalized("memory/backend/list", params)?;
        if json_output {
            self.print_value("memoryList", value, json_output)
        } else {
            let rendered = format_memory_list(&value);
            self.print_text(&rendered, json_output)
        }
    }

    fn handle_memory_jobs(
        &mut self,
        include_finished: bool,
        json_output: bool,
    ) -> Result<(), String> {
        let value = self.memory_jobs_list(include_finished)?;
        if json_output {
            self.print_value("memoryJobs", value, json_output)
        } else {
            let rendered = format_memory_jobs(&value);
            self.print_text(&rendered, json_output)
        }
    }

    fn handle_memory_job_cancel(&mut self, id: &str, json_output: bool) -> Result<(), String> {
        let id = id.trim();
        if id.is_empty() {
            return Err(memory_settings_usage().to_string());
        }
        let value = self.rpc.request(
            "memory/jobs/cancel",
            json!({
                "threadId": self.thread_id,
                "jobId": id,
                "reason": "user requested cancellation from native shell"
            }),
        )?;
        if json_output {
            self.print_value("memoryJobs", value, json_output)
        } else {
            let rendered = format_memory_jobs(&value);
            self.print_text(&rendered, json_output)
        }
    }

    fn handle_memory_forget(&mut self, id: &str, json_output: bool) -> Result<(), String> {
        let params =
            memory_backend_forget_params_from_settings(&self.memory_settings, &self.thread_id, id)?;
        let value = self.request_memory_backend_normalized("memory/backend/forget", params)?;
        if json_output {
            self.print_value("memoryForget", value, json_output)
        } else {
            let rendered = format_memory_forget(&value);
            self.print_text(&rendered, json_output)
        }
    }

    fn handle_memory_pin(
        &mut self,
        id: &str,
        pinned: bool,
        json_output: bool,
    ) -> Result<(), String> {
        let params = memory_backend_pin_params_from_settings(
            &self.memory_settings,
            &self.thread_id,
            id,
            pinned,
        )?;
        let value = self.request_memory_backend_normalized("memory/backend/pin", params)?;
        if json_output {
            self.print_value("memoryPin", value, json_output)
        } else {
            let rendered = format_memory_pin(&value);
            self.print_text(&rendered, json_output)
        }
    }

    fn run_hoppi_sync_with_job_reporting(
        &mut self,
        direction: HoppiSyncDirection,
        reason: &str,
    ) -> Result<HoppiSyncRunResult, String> {
        let invocation =
            hoppi_sync_display_invocation_from_settings(&self.memory_settings, direction, reason)?;
        let params = memory_backend_sync_params_from_settings(
            &self.memory_settings,
            &self.thread_id,
            direction,
            reason,
        )?;
        let value = self.request_memory_backend_normalized("memory/backend/sync", params)?;
        let result = hoppi_sync_result_from_value(&value);
        Ok(HoppiSyncRunResult { invocation, result })
    }

    fn handle_memory_sync(
        &mut self,
        direction: HoppiSyncDirection,
        json_output: bool,
    ) -> Result<(), String> {
        let run = self.run_hoppi_sync_with_job_reporting(direction, "manual")?;
        if json_output {
            return self.print_value(
                "memorySync",
                json!({
                    "summary": format_hoppi_sync_report(direction, &run.result),
                    "direction": direction.as_str(),
                    "repoPath": format_path_for_display(&run.invocation.repo_path),
                    "remote": run.invocation.remote.clone(),
                    "result": hoppi_sync_result_json(&run.result),
                }),
                json_output,
            );
        }
        self.print_text(
            &format_hoppi_sync_report(direction, &run.result),
            json_output,
        )
    }

    fn handle_memory_maintenance(
        &mut self,
        dry_run: bool,
        json_output: bool,
    ) -> Result<(), String> {
        let params = memory_backend_consolidate_params_from_settings(
            &self.memory_settings,
            &self.thread_id,
            dry_run,
        )?;
        let value = self.request_memory_backend_normalized("memory/backend/consolidate", params)?;
        if json_output {
            self.print_value("memoryConsolidate", value, json_output)
        } else {
            let rendered = format_memory_consolidate(&value);
            self.print_text(&rendered, json_output)
        }
    }

    fn request_memory_backend_normalized(
        &mut self,
        method: &str,
        params: Value,
    ) -> Result<Value, String> {
        let value = self.rpc.request(method, params)?;
        let normalized = memory_backend_normalized_value(value, method)?;
        self.record_memory_status_from_value(&normalized);
        Ok(normalized)
    }

    fn record_memory_status_from_value(&mut self, value: &Value) {
        if let Some(status) = value
            .get("status")
            .cloned()
            .and_then(|status| serde_json::from_value::<MemoryStatus>(status).ok())
        {
            self.memory_status = Some(status);
            self.sync_ui_docks();
        }
    }

    fn record_memory_job_update(&mut self, params: Value) {
        if let Ok(value) = self.rpc.request("memory/jobs/update", params)
            && let Some(status) = value
                .get("status")
                .cloned()
                .and_then(|status| serde_json::from_value::<MemoryStatus>(status).ok())
        {
            self.memory_status = Some(status);
            self.sync_ui_docks();
        }
    }

    fn mark_hoppi_memory_dirty(&mut self) {
        self.hoppi_dirty_version = self.hoppi_dirty_version.saturating_add(1);
        self.hoppi_dirty_at = Some(Instant::now());
    }

    fn mark_hoppi_memory_consolidated(&mut self) {
        self.hoppi_consolidated_version = self.hoppi_dirty_version;
    }

    fn maybe_drain_memory_jobs(&mut self, json_output: bool) -> Result<(), String> {
        match self.wait_for_memory_jobs_idle(HOPPI_MEMORY_DRAIN_TIMEOUT) {
            Ok(None) => Ok(()),
            Ok(Some(message)) => self.print_text(&message, json_output),
            Err(error) => self.print_text(&format!("memory: drain skipped: {error}"), json_output),
        }
    }

    fn wait_for_memory_jobs_idle(&mut self, timeout: Duration) -> Result<Option<String>, String> {
        let started = Instant::now();
        let mut last = self.memory_jobs_list(false)?;
        loop {
            if memory_jobs_active_count(&last) == 0 {
                return Ok(None);
            }
            if started.elapsed() >= timeout {
                return Ok(Some(format_memory_drain_timeout(&last, timeout)));
            }
            std::thread::sleep(HOPPI_MEMORY_DRAIN_POLL);
            last = self.memory_jobs_list(false)?;
        }
    }

    fn memory_jobs_list(&mut self, include_finished: bool) -> Result<Value, String> {
        self.rpc.request(
            "memory/jobs/list",
            json!({
                "threadId": self.thread_id,
                "includeFinished": include_finished,
                "limit": 50
            }),
        )
    }

    fn print_memory_settings_update(
        &mut self,
        label: &str,
        enabled: bool,
        json_output: bool,
    ) -> Result<(), String> {
        if json_output {
            return self.print_value(
                "memory",
                json!({ "settings": memory_settings_json(&self.memory_settings) }),
                json_output,
            );
        }
        self.print_text(
            &format!("memory: {label} {}", enabled_label(enabled)),
            json_output,
        )
    }

    fn handle_memory_control(
        &mut self,
        action: &str,
        apply: bool,
        json_output: bool,
    ) -> Result<(), String> {
        let value = self.rpc.request(
            "memory/control",
            json!({ "threadId": self.thread_id, "action": action, "apply": apply }),
        )?;
        if json_output {
            self.print_value("memoryControl", value, json_output)
        } else {
            let rendered = format_memory_control(&value);
            self.print_text(&rendered, json_output)
        }
    }

    fn handle_skills_command(&mut self, args: Vec<&str>, json_output: bool) -> Result<(), String> {
        let route = skills_command_route(&args.join(" "))?;
        match route {
            SkillsCommandRoute::Status => {
                if json_output {
                    return self.print_value(
                        "skills",
                        json!({ "enableSkillCommands": self.enable_skill_commands }),
                        json_output,
                    );
                }
                return self.print_text(
                    &format_skills_status(self.enable_skill_commands),
                    json_output,
                );
            }
            SkillsCommandRoute::SetCommands(enabled) => {
                self.enable_skill_commands = enabled;
                save_skill_commands_setting(&self.role_profile_path, enabled)?;
                if enabled && self.skill_command_cache.is_empty() {
                    self.refresh_skill_command_cache();
                }
                if json_output {
                    return self.print_value(
                        "skills",
                        json!({ "enableSkillCommands": self.enable_skill_commands }),
                        json_output,
                    );
                }
                return self.print_text(
                    &format_skills_status(self.enable_skill_commands),
                    json_output,
                );
            }
            SkillsCommandRoute::List => {}
        }
        let value = self
            .rpc
            .request("skills/list", json!({ "threadId": self.thread_id }))?;
        let result: RuntimeListResult<ResolvedSkill> = serde_json::from_value(value.clone())
            .map_err(|error| format!("decode skills/list result: {error}"))?;
        self.skill_command_cache = result.items.clone();
        if json_output {
            let mut value = value;
            value["enableSkillCommands"] = json!(self.enable_skill_commands);
            return self.print_value("skills", value, json_output);
        }
        if result.items.is_empty() {
            return self.print_text("no skills discovered", json_output);
        }
        let mut rendered = format!(
            "skills (active definitions; project > user > built-in):\n{}",
            format_skills_status(self.enable_skill_commands)
        );
        for item in result.items {
            let name = item.active.name;
            let source = skill_source_label(item.active.source);
            let description = item.active.description;
            let shadowed = item.shadowed.len();
            rendered.push_str(&format!("\n- {name} [{source}]: {description}"));
            if shadowed > 0 {
                rendered.push_str(&format!(
                    "\n  shadows {shadowed} lower-priority definition(s)"
                ));
            }
        }
        self.print_text(&rendered, json_output)
    }

    fn handle_prompt_templates_command(&mut self, json_output: bool) -> Result<(), String> {
        let value = self.rpc.request(
            "prompt-template/list",
            json!({ "threadId": self.thread_id }),
        )?;
        if json_output {
            return self.print_value("prompts", value, json_output);
        }
        self.print_text(&format_prompt_templates(&value), json_output)
    }

    fn handle_graphify_command(
        &mut self,
        args: Vec<&str>,
        json_output: bool,
    ) -> Result<(), String> {
        match args.as_slice() {
            [] | ["status"] => {
                let status = graphify_status(&self.cwd);
                if json_output {
                    self.print_value("graphify", graphify_status_json(&status), json_output)
                } else {
                    self.print_text(&format_graphify_status(&status), json_output)
                }
            }
            ["install" | "setup"] => self.print_text(graphify_install_guidance(), json_output),
            ["commands" | "help"] => self.print_text(graphify_command_guidance(), json_output),
            _ => self.print_text("usage: /graphify [status|install|commands]", json_output),
        }
    }

    fn handle_agents_command(
        &mut self,
        provider: &ProviderConfig,
        args: Vec<&str>,
        json_output: bool,
    ) -> Result<(), String> {
        let agent_dir = default_agent_dir();
        let route = parse_agents_route_parts(&args, Path::new(&self.cwd), &agent_dir)?;
        match route {
            AgentsRoute::List => {
                let value = self
                    .rpc
                    .request("agents/list", json!({ "threadId": self.thread_id }))?;
                if !json_output {
                    self.print_text(
                        &format!(
                            "agent dispatch profile: {}",
                            self.role_execution_profile("subagent", provider)
                        ),
                        json_output,
                    )?;
                }
                if json_output {
                    self.print_value("agents", value, json_output)
                } else {
                    let rendered = format_agents_list(&value)?;
                    self.print_text(&rendered, json_output)
                }
            }
            AgentsRoute::Show { name } => {
                let value = self.rpc.request(
                    "agents/show",
                    json!({ "threadId": self.thread_id, "name": name }),
                )?;
                if json_output {
                    self.print_value("agent", value, json_output)
                } else {
                    let resolved: ResolvedAgent = serde_json::from_value(value)
                        .map_err(|error| format!("decode agents/show: {error}"))?;
                    self.print_text(&format_agent_show(&resolved), json_output)
                }
            }
            AgentsRoute::Create {
                source,
                name,
                description,
            } => {
                let value = self.rpc.request(
                    "agents/create",
                    json!({
                        "threadId": self.thread_id,
                        "source": source,
                        "definition": {
                            "name": name,
                            "description": description,
                            "instructions": STARTER_AGENT_INSTRUCTIONS,
                        }
                    }),
                )?;
                if json_output {
                    self.print_value("agentCreate", value, json_output)
                } else {
                    let created: AgentDefinition = serde_json::from_value(value)
                        .map_err(|error| format!("decode agents/create: {error}"))?;
                    let path = created
                        .path
                        .as_deref()
                        .map(|path| format!(" at {path}"))
                        .unwrap_or_default();
                    self.print_text(
                        &format!("created agent {}{}", created.name, path),
                        json_output,
                    )
                }
            }
            AgentsRoute::Edit { name } => {
                let value = self.rpc.request(
                    "agents/show",
                    json!({ "threadId": self.thread_id, "name": name }),
                )?;
                let resolved: ResolvedAgent = serde_json::from_value(value)
                    .map_err(|error| format!("decode agents/show: {error}"))?;
                let agent = resolved.active;
                if agent.read_only || !agent_source_is_writable(agent.source) {
                    return self.print_text(
                        &format!("agent is read-only [{}]", agent_source_label(agent.source)),
                        json_output,
                    );
                }
                let Some(path) = agent.path.as_deref().filter(|path| !path.trim().is_empty())
                else {
                    return self.print_text("agent has no editable path", json_output);
                };
                if let Some(editor) = configured_agent_editor() {
                    launch_agent_editor(&editor, Path::new(path))?;
                    self.print_text(
                        &format!("opened agent {} in {}", agent.name, editor),
                        json_output,
                    )
                } else {
                    let markdown = oppi_agents::format_agent_markdown(&agent);
                    self.print_text(&format!("agent path: {path}\n\n{markdown}"), json_output)
                }
            }
            AgentsRoute::Delete { name, confirmed } => {
                if !confirmed {
                    return self.print_text(
                        &format!(
                            "Refusing to delete without confirmation. Run `/agents delete {name} --yes`."
                        ),
                        json_output,
                    );
                }
                let value = self.rpc.request(
                    "agents/show",
                    json!({ "threadId": self.thread_id, "name": name }),
                )?;
                let resolved: ResolvedAgent = serde_json::from_value(value)
                    .map_err(|error| format!("decode agents/show: {error}"))?;
                let source = resolved
                    .active
                    .source
                    .ok_or_else(|| format!("agent {} has no source", resolved.active.name))?;
                let value = self.rpc.request(
                    "agents/delete",
                    json!({
                        "threadId": self.thread_id,
                        "source": source,
                        "name": resolved.active.name,
                    }),
                )?;
                if json_output {
                    self.print_value("agentDelete", value, json_output)
                } else {
                    self.print_text(&format!("deleted agent {name}"), json_output)
                }
            }
            AgentsRoute::Dispatch { name, task } => {
                let role = "subagent";
                let promoted = complex_subagent_task(&task);
                let effective_provider =
                    self.provider_for_role_with_complexity(provider, Some(role), promoted);
                let model = current_provider_model(&effective_provider).map(str::to_string);
                let effort = match effective_provider {
                    ProviderConfig::OpenAiCompatible(config) => config.reasoning_effort,
                    ProviderConfig::Mock => None,
                };
                if !json_output {
                    let source = if self.role_models.contains_key(role) {
                        "profile"
                    } else if promoted {
                        "promoted"
                    } else if default_model_for_role(provider, role, false).is_some() {
                        "default"
                    } else {
                        "inherit"
                    };
                    self.print_text(
                        &format!(
                            "agent dispatch profile: role={role} model={} ({source}) effort={}",
                            model
                                .as_deref()
                                .map(display_model_id)
                                .unwrap_or("provider default"),
                            effort.as_deref().unwrap_or("off")
                        ),
                        json_output,
                    )?;
                }
                let value = self.rpc.request(
                    "agents/dispatch",
                    json!({
                        "threadId": self.thread_id,
                        "agentName": name,
                        "task": task,
                        "background": false,
                        "role": role,
                        "model": model,
                        "effort": effort,
                    }),
                )?;
                self.print_value("agentDispatch", value, json_output)
            }
            AgentsRoute::Poll { run_id } => {
                let value = self.rpc.request(
                    "agents/poll",
                    json!({ "threadId": self.thread_id, "runId": run_id }),
                )?;
                if json_output {
                    self.print_value("agentPoll", value, json_output)
                } else {
                    let result: AgentRunPollResult = serde_json::from_value(value)
                        .map_err(|error| format!("decode agents/poll: {error}"))?;
                    self.print_text(&format_agent_poll_result(&result), json_output)
                }
            }
            AgentsRoute::Block { run_id, reason } => {
                let value = self.rpc.request(
                    "agents/block",
                    json!({ "threadId": self.thread_id, "runId": run_id, "reason": reason }),
                )?;
                self.print_value("agentBlock", value, json_output)
            }
            AgentsRoute::Complete { run_id, output } => {
                let value = self.rpc.request(
                    "agents/complete",
                    json!({ "threadId": self.thread_id, "runId": run_id, "output": output }),
                )?;
                self.print_value("agentComplete", value, json_output)
            }
            AgentsRoute::Import(route) => {
                let raw = fs::read_to_string(&route.path).map_err(|error| {
                    format!("read agent markdown {}: {error}", route.path.display())
                })?;
                let agent = parse_agent_markdown(&raw, route.source)?;
                let value = self.rpc.request(
                    "agents/register",
                    json!({ "threadId": self.thread_id, "agent": agent }),
                )?;
                if json_output {
                    self.print_value(
                        "agentImport",
                        json!({ "path": route.path.display().to_string(), "source": route.source, "result": value }),
                        json_output,
                    )
                } else {
                    self.print_text(
                        &format!("imported agent markdown from {}", route.path.display()),
                        json_output,
                    )
                }
            }
            AgentsRoute::Export(route) => {
                let value = self
                    .rpc
                    .request("agents/list", json!({ "threadId": self.thread_id }))?;
                let agent = active_agent_from_list(value, &route.agent_name)?;
                let markdown = format_agent_markdown(&agent);
                if let Some(parent) = route.path.parent() {
                    fs::create_dir_all(parent).map_err(|error| {
                        format!("create agent markdown dir {}: {error}", parent.display())
                    })?;
                }
                fs::write(&route.path, markdown).map_err(|error| {
                    format!("write agent markdown {}: {error}", route.path.display())
                })?;
                if json_output {
                    self.print_value(
                        "agentExport",
                        json!({ "agent": route.agent_name, "path": route.path.display().to_string() }),
                        json_output,
                    )
                } else {
                    self.print_text(
                        &format!(
                            "exported agent {} to {}",
                            route.agent_name,
                            route.path.display()
                        ),
                        json_output,
                    )
                }
            }
        }
    }

    fn handle_prompt_variant_command(
        &mut self,
        variant: &str,
        json_output: bool,
    ) -> Result<(), String> {
        if !variant.is_empty() {
            let normalized = normalize_prompt_variant(variant)
                .ok_or_else(|| "usage: /prompt-variant [off|a|b|caveman]".to_string())?;
            save_prompt_variant_setting(&self.role_profile_path, &normalized)?;
            self.prompt_variant = normalized;
        }
        self.print_text(
            &format!("prompt variant: {}", self.prompt_variant),
            json_output,
        )
    }

    fn handle_theme_command(&mut self, theme: &str, json_output: bool) -> Result<(), String> {
        let trimmed = theme.trim();
        if trimmed == "preview" || trimmed.starts_with("preview ") {
            let preview = trimmed.strip_prefix("preview").unwrap_or_default();
            return self.handle_theme_preview_command(preview.trim(), json_output);
        }
        if theme.trim().is_empty() {
            let list = self
                .rpc
                .request("theme/list", json!({ "threadId": self.thread_id }))?;
            let rendered =
                format_theme_status(&self.theme, self.loaded_theme.as_ref(), Some(&list));
            return self.print_text(&rendered, json_output);
        }
        if !theme.is_empty() {
            let selection = if theme_preview_index_arg(trimmed) {
                let list = self
                    .rpc
                    .request("theme/list", json!({ "threadId": self.thread_id }))?;
                theme_preview_index_selection(Some(&list), trimmed).ok_or_else(|| {
                    format!("theme index {trimmed} is out of range; run /theme preview")
                })?
            } else {
                trimmed.to_string()
            };
            if theme.trim().eq_ignore_ascii_case("reload") {
                if let Some(loaded) = load_theme_file()? {
                    self.apply_theme_selection(&loaded)?;
                }
            } else {
                self.apply_theme_selection(&selection)?;
            }
        }
        self.print_text(
            &format_theme_status(&self.theme, self.loaded_theme.as_ref(), None),
            json_output,
        )
    }

    fn handle_theme_preview_command(
        &mut self,
        theme: &str,
        json_output: bool,
    ) -> Result<(), String> {
        let theme = theme.trim();
        if theme.is_empty() {
            let list = self
                .rpc
                .request("theme/list", json!({ "threadId": self.thread_id }))?;
            return self.print_text(
                &format_theme_preview_index(&self.theme, Some(&list)),
                json_output,
            );
        }
        let theme = if theme_preview_index_arg(theme) {
            let list = self
                .rpc
                .request("theme/list", json!({ "threadId": self.thread_id }))?;
            theme_preview_index_selection(Some(&list), theme)
                .ok_or_else(|| format!("theme index {theme} is out of range; run /theme preview"))?
        } else {
            theme.to_string()
        };
        if let Some(normalized) = normalize_theme_name(&theme) {
            return self.print_text(
                &format_theme_preview_detail(&normalized, "builtIn", theme_tokens(&normalized)),
                json_output,
            );
        }
        let value = self.rpc.request(
            "theme/load",
            theme_load_params_json(&self.thread_id, &theme),
        )?;
        let loaded = loaded_shell_theme_from_load_result(&value, &theme)?;
        self.print_text(
            &format_theme_preview_detail(&loaded.name, &loaded.source, loaded.line_tokens),
            json_output,
        )
    }

    fn apply_theme_selection(&mut self, theme: &str) -> Result<(), String> {
        if let Some(normalized) = normalize_theme_name(theme) {
            self.theme = normalized;
            self.loaded_theme = None;
            return Ok(());
        }
        let value = self
            .rpc
            .request("theme/load", theme_load_params_json(&self.thread_id, theme))?;
        let loaded = loaded_shell_theme_from_load_result(&value, theme)?;
        self.theme = loaded.name.clone();
        self.loaded_theme = Some(loaded);
        Ok(())
    }

    fn prepare_and_run_command(
        &mut self,
        command: &str,
        args: &str,
        provider: &ProviderConfig,
        json_output: bool,
    ) -> Result<(), String> {
        let role = role_for_command(command);
        let role_model = self.effective_model_for_role(role, provider);
        let role_effort = self.effective_effort_for_role(role, provider);
        let prepared = self.rpc.request(
            "command/prepare",
            command_prepare_params_json(
                &self.thread_id,
                command,
                args,
                role,
                role_model,
                role_effort,
                prompt_variant_append(&self.prompt_variant),
            ),
        )?;
        if !json_output {
            let mut rendered = format!(
                "command profile: {}",
                self.role_execution_profile(role, provider)
            );
            if let Some(notes) = prepared.get("notes").and_then(Value::as_array) {
                for note in notes.iter().filter_map(Value::as_str) {
                    rendered.push_str(&format!("\nnote: {note}"));
                }
            }
            self.print_text(&rendered, json_output)?;
        }
        let input = prepared["input"]
            .as_str()
            .ok_or_else(|| "command/prepare returned no input".to_string())?
            .to_string();
        let _ = self.run_turn_for_role(&input, provider, json_output, Some(role))?;
        Ok(())
    }

    fn resolve_background_task_id(
        &mut self,
        requested: Option<&str>,
        running_only: bool,
    ) -> Result<String, String> {
        if let Some(task_id) = requested
            && !is_background_latest_alias(task_id)
        {
            return Ok(task_id.to_string());
        }
        let value = self.rpc.request("background/list", json!({}))?;
        select_background_task_id(&value, running_only).ok_or_else(|| {
            if running_only {
                "no running background task; pass an explicit task id or run /background list".to_string()
            } else {
                "no background tasks; background shell tasks are process-local and reset when the server restarts".to_string()
            }
        })
    }

    fn print_background_read(
        &mut self,
        task_id: &str,
        max_bytes: usize,
        json: bool,
    ) -> Result<(), String> {
        let value = self.rpc.request(
            "background/read",
            json!({ "taskId": task_id, "maxBytes": max_bytes }),
        )?;
        self.background_summary = Some(background_summary_from_read(&value));
        self.sync_ui_docks();
        if json {
            self.print_value("backgroundRead", value, json)
        } else {
            let rendered = format_background_read(&value);
            self.print_text(&rendered, json)
        }
    }

    fn print_background_kill(&mut self, task_id: &str, json: bool) -> Result<(), String> {
        let value = self
            .rpc
            .request("background/kill", json!({ "taskId": task_id }))?;
        self.background_summary = Some(background_summary_from_kill(&value));
        self.sync_ui_docks();
        if json {
            self.print_value("backgroundKill", value, json)
        } else {
            let rendered = format_background_kill(&value);
            self.print_text(&rendered, json)
        }
    }

    fn handle_background_command(&mut self, args: Vec<&str>, json: bool) -> Result<(), String> {
        match args.as_slice() {
            [] | ["list"] => {
                let value = self.rpc.request("background/list", json!({}))?;
                self.background_summary = Some(background_summary_from_list(&value));
                self.sync_ui_docks();
                if json {
                    self.print_value("background", value, json)
                } else {
                    let rendered = format_background_list(&value);
                    self.print_text(&rendered, json)
                }
            }
            ["read"] => {
                let task_id = self.resolve_background_task_id(None, false)?;
                self.print_background_read(&task_id, 30_000, json)
            }
            ["read", task_id] => {
                let task_id = self.resolve_background_task_id(Some(task_id), false)?;
                self.print_background_read(&task_id, 30_000, json)
            }
            ["read", task_id, max_bytes] => {
                let max_bytes = max_bytes
                    .parse::<usize>()
                    .map_err(|error| format!("invalid max bytes: {error}"))?;
                let task_id = self.resolve_background_task_id(Some(task_id), false)?;
                self.print_background_read(&task_id, max_bytes, json)
            }
            ["kill"] => {
                let task_id = self.resolve_background_task_id(None, true)?;
                self.print_background_kill(&task_id, json)
            }
            ["kill", task_id] => {
                let task_id = self.resolve_background_task_id(Some(task_id), true)?;
                self.print_background_kill(&task_id, json)
            }
            _ => self.print_text(
                "usage: /background [list|read [task-id|latest] [max-bytes]|kill [task-id|latest]]",
                json,
            ),
        }
    }

    fn handle_btw_command(
        &mut self,
        question: &str,
        provider: &ProviderConfig,
        json_output: bool,
    ) -> Result<(), String> {
        if question.is_empty() {
            return self.print_text("usage: /btw <question>", json_output);
        }
        let value = self.rpc.request(
            "side-question/ask",
            side_question_request_params(
                &self.thread_id,
                question,
                provider,
                &self.cwd,
                self.permission_mode,
                &self.permission_sandbox_settings,
                &self.prompt_variant,
            ),
        )?;
        if json_output {
            self.print_value("sideQuestion", value, json_output)
        } else {
            let rendered = format_side_question_result(&value)?;
            self.print_text(&rendered, json_output)
        }
    }

    fn resume_pending_approval(
        &mut self,
        provider: &ProviderConfig,
        json_output: bool,
    ) -> Result<(), String> {
        let pending = self
            .pending_approval
            .take()
            .ok_or_else(|| "no pending approval".to_string())?;
        let approved_tool_call_ids = pending
            .tool_call_id
            .as_ref()
            .map(|id| vec![id.clone()])
            .unwrap_or_default();
        let mut params = json!({
            "threadId": self.thread_id,
            "turnId": pending.turn_id,
            "approvedToolCallIds": approved_tool_call_ids,
        });
        let mut active_turn_model_provider = None;
        match provider {
            ProviderConfig::Mock => {
                params["modelSteps"] = mock_model_steps_after_approval(pending.tool_call.as_ref());
            }
            ProviderConfig::OpenAiCompatible(config) => {
                active_turn_model_provider = Some(config.clone());
                let mut provider_json = openai_provider_json(config);
                apply_feature_routing_to_provider(&mut provider_json, &self.prompt_variant);
                apply_prompt_variant_to_provider(&mut provider_json, &self.prompt_variant);
                params["modelProvider"] = provider_json;
                params["maxContinuations"] = json!(8);
            }
        }
        self.rpc.request("turn/resume-agentic", params)?;
        self.active_turn_model_provider = active_turn_model_provider;
        self.print_text(&format!("approved {}", pending.request_id), json_output)?;
        let _ = self.poll_until_turn_boundary(json_output)?;
        Ok(())
    }

    fn deny_pending_approval(&mut self, json_output: bool) -> Result<(), String> {
        let pending = self
            .pending_approval
            .take()
            .ok_or_else(|| "no pending approval".to_string())?;
        self.rpc.request(
            "turn/interrupt",
            json!({
                "threadId": self.thread_id,
                "turnId": pending.turn_id,
                "reason": format!("approval denied: {}", pending.request_id),
            }),
        )?;
        self.print_text(&format!("denied {}", pending.request_id), json_output)?;
        Ok(())
    }

    fn answer_pending_question(
        &mut self,
        answer: &str,
        provider: &ProviderConfig,
        json_output: bool,
    ) -> Result<(), String> {
        if answer.is_empty() {
            return Err("usage: /answer <text-or-option-id>".to_string());
        }
        let pending = self
            .pending_question
            .take()
            .ok_or_else(|| "no pending ask_user question".to_string())?;
        let answers = ask_user_input_answers(&pending.request, answer);
        self.resume_pending_question(pending, answers, false, provider)?;
        self.print_text("answered pending question", json_output)?;
        let _ = self.poll_until_turn_boundary(json_output)?;
        Ok(())
    }

    fn resume_pending_question(
        &mut self,
        pending: PendingQuestion,
        answers: Vec<AskUserAnswer>,
        timed_out: bool,
        provider: &ProviderConfig,
    ) -> Result<(), String> {
        let mut params = json!({
            "threadId": self.thread_id,
            "turnId": pending.turn_id,
            "askUserResponse": {
                "requestId": pending.request.id,
                "answers": answers,
                "timedOut": timed_out,
            },
        });
        let mut active_turn_model_provider = None;
        match provider {
            ProviderConfig::Mock => {
                params["modelSteps"] = mock_model_steps_after_answer();
            }
            ProviderConfig::OpenAiCompatible(config) => {
                active_turn_model_provider = Some(config.clone());
                let mut provider_json = openai_provider_json(config);
                apply_feature_routing_to_provider(&mut provider_json, &self.prompt_variant);
                apply_prompt_variant_to_provider(&mut provider_json, &self.prompt_variant);
                params["modelProvider"] = provider_json;
                params["maxContinuations"] = json!(8);
            }
        }
        self.rpc.request("turn/resume-agentic", params)?;
        self.active_turn_model_provider = active_turn_model_provider;
        Ok(())
    }

    fn maybe_timeout_pending_question(
        &mut self,
        provider: &ProviderConfig,
        json_output: bool,
    ) -> Result<bool, String> {
        let expired = self
            .pending_question
            .as_ref()
            .and_then(|pending| pending.deadline_at)
            .is_some_and(|deadline| Instant::now() >= deadline);
        if !expired {
            return Ok(false);
        }
        let pending = self
            .pending_question
            .take()
            .ok_or_else(|| "no pending ask_user question".to_string())?;
        let answers = ask_user_timeout_answers(&pending.request);
        self.resume_pending_question(pending, answers, true, provider)?;
        self.print_text(
            "question timed out; used recommended/default answers",
            json_output,
        )?;
        let _ = self.poll_until_turn_boundary(json_output)?;
        Ok(true)
    }

    fn poll_turn_events(&mut self, json_output: bool) -> Result<Option<TurnOutcome>, String> {
        self.poll_turn_events_with_output(if json_output {
            EventOutputMode::Json
        } else {
            EventOutputMode::Text
        })
    }

    fn poll_turn_events_silent(&mut self) -> Result<Option<TurnOutcome>, String> {
        self.poll_turn_events_with_output(EventOutputMode::Silent)
    }

    fn poll_turn_events_with_output(
        &mut self,
        output: EventOutputMode,
    ) -> Result<Option<TurnOutcome>, String> {
        let listed = self.rpc.request(
            "events/list",
            json!({
                "threadId": self.thread_id,
                "after": self.cursor,
                "limit": 1000
            }),
        )?;
        let events = listed["events"]
            .as_array()
            .ok_or_else(|| "events/list returned invalid events".to_string())?;
        let mut outcome = None;
        for raw in events {
            let event: Event = serde_json::from_value(raw.clone())
                .map_err(|error| format!("decode event: {error}"))?;
            self.cursor = self.cursor.max(event.id);
            let rendered = self.render_event_line(&event);
            match output {
                EventOutputMode::Json => println!(
                    "{}",
                    serde_json::to_string(&event).map_err(|error| error.to_string())?
                ),
                EventOutputMode::Text => {
                    if let Some(line) = &rendered {
                        println!("{line}");
                    }
                }
                EventOutputMode::Silent => {}
            }
            if let Some(line) = rendered {
                self.ui
                    .push_event_transcript(&event, line, &self.tool_calls);
            }
            match &event.kind {
                EventKind::TurnStarted { turn } => {
                    self.active_turn_id = Some(turn.id.clone());
                    self.emit_terminal_progress(true, matches!(output, EventOutputMode::Json));
                }
                EventKind::TurnCompleted { turn_id } => {
                    self.maybe_remember_hoppi_turn(turn_id);
                    self.steered_turns.remove(turn_id);
                    self.active_turn_id = None;
                    self.active_turn_model_provider = None;
                    self.emit_terminal_progress(false, matches!(output, EventOutputMode::Json));
                    outcome = Some(TurnOutcome::Completed);
                }
                EventKind::TurnAborted { reason } => {
                    if let Some(turn_id) = event.turn_id.as_ref() {
                        self.steered_turns.remove(turn_id);
                    }
                    self.active_turn_id = None;
                    self.active_turn_model_provider = None;
                    self.emit_terminal_progress(false, matches!(output, EventOutputMode::Json));
                    outcome = Some(TurnOutcome::Aborted(reason.clone()));
                }
                EventKind::TurnInterrupted { reason } => {
                    if let Some(turn_id) = event.turn_id.as_ref() {
                        self.steered_turns.remove(turn_id);
                    }
                    self.active_turn_id = None;
                    self.active_turn_model_provider = None;
                    self.emit_terminal_progress(false, matches!(output, EventOutputMode::Json));
                    outcome = Some(TurnOutcome::Interrupted(reason.clone()));
                }
                EventKind::ApprovalRequested { request } => {
                    self.pending_approval = Some(PendingApproval {
                        turn_id: event.turn_id.clone().unwrap_or_default(),
                        request_id: request.id.clone(),
                        tool_call_id: request.tool_call.as_ref().map(tool_call_id),
                        tool_call: request.tool_call.clone(),
                    });
                    self.active_turn_id = None;
                    self.emit_terminal_progress(false, matches!(output, EventOutputMode::Json));
                    outcome = Some(TurnOutcome::Paused);
                }
                EventKind::AskUserRequested { request } => {
                    self.pending_question = Some(PendingQuestion {
                        turn_id: event.turn_id.clone().unwrap_or_default(),
                        request: request.clone(),
                        requested_at: Instant::now(),
                        deadline_at: ask_user_deadline_at(request),
                    });
                    self.active_turn_id = None;
                    outcome = Some(TurnOutcome::Paused);
                }
                EventKind::ToolCallStarted { call } => {
                    self.tool_calls.insert(call.id.clone(), call.clone());
                }
                EventKind::ThreadGoalUpdated { goal } => {
                    self.current_goal = Some(goal.clone());
                }
                EventKind::ThreadGoalCleared { .. } => {
                    self.current_goal = None;
                }
                EventKind::TodosUpdated { state } => {
                    self.todo_state = state.clone();
                }
                EventKind::Diagnostic { diagnostic } => {
                    self.usage_state.observe_provider_diagnostic(diagnostic);
                }
                EventKind::MemoryStatusChanged { status } => {
                    self.memory_status = Some(status.clone());
                }
                EventKind::SuggestionOffered { suggestion } => {
                    if suggestion.confidence >= 0.70 {
                        self.suggestion = Some(suggestion.clone());
                    }
                }
                _ => {}
            }
            self.sync_ui_docks();
        }
        Ok(outcome)
    }

    fn poll_until_turn_boundary(&mut self, json_output: bool) -> Result<TurnOutcome, String> {
        let started = Instant::now();
        loop {
            if let Some(outcome) = self.poll_turn_events(json_output)? {
                return Ok(outcome);
            }
            if started.elapsed() > TURN_TIMEOUT {
                return Err("timed out waiting for turn event boundary".to_string());
            }
            std::thread::sleep(POLL_INTERVAL);
        }
    }

    fn has_pending_pause(&self) -> bool {
        self.pending_approval.is_some() || self.pending_question.is_some()
    }

    fn is_turn_running(&self) -> bool {
        self.active_turn_id.is_some()
    }

    fn mutable_turn_id(&self) -> Option<String> {
        self.active_turn_id
            .clone()
            .or_else(|| {
                self.pending_approval
                    .as_ref()
                    .map(|pending| pending.turn_id.clone())
            })
            .or_else(|| {
                self.pending_question
                    .as_ref()
                    .map(|pending| pending.turn_id.clone())
            })
    }

    fn steer_active_turn(&mut self, input: &str, json_output: bool) -> Result<(), String> {
        if input.trim().is_empty() {
            return Err(steer_usage().to_string());
        }
        let turn_id = self
            .mutable_turn_id()
            .ok_or_else(|| "no active or paused turn to steer".to_string())?;
        if self.steering_mode == SteeringMode::OneAtATime && self.steered_turns.contains(&turn_id) {
            return self.queue_steering(input, json_output);
        }
        let value = self.rpc.request(
            "turn/steer",
            json!({ "threadId": self.thread_id, "turnId": turn_id, "input": input }),
        )?;
        self.steered_turns.insert(turn_id);
        if json_output {
            self.print_value("steer", value, json_output)
        } else {
            self.print_text("steering input sent to active turn", json_output)
        }
    }

    fn interrupt_active_turn(&mut self, json_output: bool) -> Result<(), String> {
        let Some(turn_id) = self.mutable_turn_id() else {
            return self.print_text("no active turn to interrupt", json_output);
        };
        let value = self.rpc.request(
            "turn/interrupt",
            json!({
                "threadId": self.thread_id,
                "turnId": turn_id,
                "reason": "user interrupt from native shell",
            }),
        )?;
        self.active_turn_id = None;
        self.active_turn_model_provider = None;
        self.pending_approval = None;
        self.pending_question = None;
        self.emit_terminal_progress(false, json_output);
        self.sync_ui_docks();
        if json_output {
            self.print_value("interrupt", value, json_output)
        } else {
            self.print_text("interrupt requested", json_output)
        }
    }

    fn restore_latest_follow_up(&mut self) -> Option<String> {
        let restored = self
            .steering_queue
            .pop_back()
            .or_else(|| self.follow_up_queue.pop_back());
        self.sync_ui_docks();
        restored
    }

    fn queue_steering(&mut self, prompt: &str, json_output: bool) -> Result<(), String> {
        let prompt = prompt.trim();
        if prompt.is_empty() {
            return Ok(());
        }
        self.steering_queue.push_back(prompt.to_string());
        self.sync_ui_docks();
        self.print_text(
            &format!("queued steering #{}", self.steering_queue.len()),
            json_output,
        )
    }

    fn queue_follow_up(&mut self, prompt: &str, json_output: bool) -> Result<(), String> {
        let prompt = prompt.trim();
        if prompt.is_empty() {
            return Ok(());
        }
        self.follow_up_queue.push_back(prompt.to_string());
        self.sync_ui_docks();
        self.print_text(
            &format!("queued follow-up #{}", self.follow_up_queue.len()),
            json_output,
        )
    }

    fn start_next_queued_steering(
        &mut self,
        provider: &ProviderConfig,
        json_output: bool,
    ) -> Result<bool, String> {
        if self.is_turn_running() || self.has_pending_pause() {
            return Ok(false);
        }
        let prompts = take_steering_batch(&mut self.steering_queue, self.steering_mode);
        if prompts.is_empty() {
            self.sync_ui_docks();
            return Ok(false);
        };
        let prompt = format_steering_batch_prompt(&prompts);
        self.sync_ui_docks();
        self.print_text(&format_steering_run_message(&prompts), json_output)?;
        self.start_turn_for_role(&prompt, provider, json_output, Some("executor"))?;
        Ok(true)
    }

    fn start_next_queued_follow_up(
        &mut self,
        provider: &ProviderConfig,
        json_output: bool,
    ) -> Result<bool, String> {
        if self.is_turn_running() || self.has_pending_pause() {
            return Ok(false);
        }
        let prompts = take_follow_up_batch(&mut self.follow_up_queue, self.follow_up_mode);
        if prompts.is_empty() {
            self.sync_ui_docks();
            return Ok(false);
        };
        let prompt = format_follow_up_batch_prompt(&prompts);
        self.sync_ui_docks();
        self.print_text(&format_follow_up_run_message(&prompts), json_output)?;
        self.start_turn_for_role(&prompt, provider, json_output, Some("executor"))?;
        Ok(true)
    }

    fn start_next_queued_or_goal_continuation(
        &mut self,
        provider: &ProviderConfig,
        json_output: bool,
    ) -> Result<(), String> {
        if self.start_next_queued_steering(provider, json_output)? {
            return Ok(());
        }
        if self.start_next_queued_follow_up(provider, json_output)? {
            return Ok(());
        }
        self.start_goal_continuation_turn(provider, json_output)
            .map(|_| ())
    }

    fn drain_follow_ups(
        &mut self,
        provider: &ProviderConfig,
        json_output: bool,
    ) -> Result<(), String> {
        while !self.has_pending_pause() {
            let prompts = take_steering_batch(&mut self.steering_queue, self.steering_mode);
            if prompts.is_empty() {
                break;
            };
            let prompt = format_steering_batch_prompt(&prompts);
            self.sync_ui_docks();
            self.print_text(&format_steering_run_message(&prompts), json_output)?;
            let outcome =
                self.run_turn_for_role(&prompt, provider, json_output, Some("executor"))?;
            if !matches!(outcome, TurnOutcome::Completed) {
                break;
            }
        }
        while !self.has_pending_pause() {
            let prompts = take_follow_up_batch(&mut self.follow_up_queue, self.follow_up_mode);
            if prompts.is_empty() {
                break;
            };
            let prompt = format_follow_up_batch_prompt(&prompts);
            self.sync_ui_docks();
            self.print_text(&format_follow_up_run_message(&prompts), json_output)?;
            let outcome =
                self.run_turn_for_role(&prompt, provider, json_output, Some("executor"))?;
            if !matches!(outcome, TurnOutcome::Completed) {
                break;
            }
        }
        while !self.has_pending_pause()
            && !self.is_turn_running()
            && self.steering_queue.is_empty()
            && self.follow_up_queue.is_empty()
        {
            let Some((prompt, continuation)) = self.claim_goal_continuation_prompt(json_output)?
            else {
                break;
            };
            self.print_text(&format!("continuing goal #{continuation}"), json_output)?;
            let outcome = self.run_turn_for_role_with_system_append(
                GOAL_CONTINUATION_INPUT,
                provider,
                json_output,
                Some("executor"),
                Some(&prompt),
            )?;
            if !matches!(outcome, TurnOutcome::Completed) {
                break;
            }
        }
        while !self.has_pending_pause()
            && !self.is_turn_running()
            && self.follow_up_queue.is_empty()
        {
            let Some((prompt, continuation)) = self.claim_goal_continuation_prompt(json_output)?
            else {
                break;
            };
            self.print_text(&format!("continuing goal #{continuation}"), json_output)?;
            let outcome = self.run_turn_for_role_with_system_append(
                GOAL_CONTINUATION_INPUT,
                provider,
                json_output,
                Some("executor"),
                Some(&prompt),
            )?;
            if !matches!(outcome, TurnOutcome::Completed) {
                break;
            }
        }
        Ok(())
    }

    fn start_goal_continuation_turn(
        &mut self,
        provider: &ProviderConfig,
        json_output: bool,
    ) -> Result<bool, String> {
        let Some((prompt, continuation)) = self.claim_goal_continuation_prompt(json_output)? else {
            return Ok(false);
        };
        self.print_text(&format!("continuing goal #{continuation}"), json_output)?;
        self.start_turn_for_role_with_system_append(
            GOAL_CONTINUATION_INPUT,
            provider,
            json_output,
            Some("executor"),
            Some(&prompt),
        )?;
        Ok(true)
    }

    fn claim_goal_continuation_prompt(
        &mut self,
        json_output: bool,
    ) -> Result<Option<(String, u32)>, String> {
        if self.is_turn_running()
            || self.has_pending_pause()
            || !self.steering_queue.is_empty()
            || !self.follow_up_queue.is_empty()
            || !self
                .current_goal
                .as_ref()
                .is_some_and(|goal| goal.status == ThreadGoalStatus::Active)
        {
            return Ok(None);
        }
        let value = self.rpc.request(
            "thread/goal/continuation",
            json!({
                "threadId": self.thread_id,
                "maxContinuations": GOAL_CONTINUATION_CAP,
            }),
        )?;
        self.apply_goal_response(&value)?;
        if let Some(reason) = value.get("blockedReason").and_then(Value::as_str) {
            self.print_text(&format!("goal continuation stopped: {reason}"), json_output)?;
            return Ok(None);
        }
        let Some(prompt) = value.get("prompt").and_then(Value::as_str) else {
            return Ok(None);
        };
        let continuation = value
            .get("continuation")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            .min(u32::MAX as u64) as u32;
        Ok(Some((prompt.to_string(), continuation)))
    }

    fn sync_ui_docks(&mut self) {
        let active = active_todos(&self.todo_state);
        let approval = self.pending_approval.as_ref().map(|pending| {
            format!(
                "{} pending{} — /approve or /deny",
                pending.request_id,
                pending
                    .tool_call_id
                    .as_ref()
                    .map(|id| format!(" for {id}"))
                    .unwrap_or_default()
            )
        });
        let question = self.pending_question.as_ref().map(|pending| {
            format!(
                "{} — /answer <text-or-option-id>",
                pending
                    .request
                    .title
                    .as_deref()
                    .unwrap_or("user input requested")
            )
        });
        let suggestion = self
            .suggestion
            .as_ref()
            .map(|suggestion| suggestion.message.clone());
        let active_todo_count = active.len();
        let goal = self.goal_status_label();
        let usage = self.usage_state.session_footer_label();
        let weekly = self.usage_state.weekly_footer_label();
        let context = self.usage_state.context_footer_label();
        let memory = self.memory_footer_label();
        let model = self
            .selected_model
            .as_deref()
            .map(display_model_id)
            .unwrap_or("none");
        let footer = format_footer_status_line(FooterStatusParts {
            settings: &self.footer_settings,
            cwd: &self.cwd,
            model,
            permission: self.permission_mode.as_str(),
            usage: &usage,
            weekly: weekly.as_deref(),
            context: &context,
            memory: &memory,
            todos: active_todo_count,
            queued: self.follow_up_queue.len() + self.steering_queue.len(),
            goal: &goal,
            variant: &self.prompt_variant,
            theme: &self.theme,
            thread: &self.thread_id,
        });
        self.ui.update_docks(DockState {
            todos: active,
            approval,
            question,
            background: self.background_summary.clone(),
            suggestion,
            footer,
        });
    }

    fn goal_status_label(&self) -> String {
        format_goal_status_label(self.current_goal.as_ref())
    }

    fn goal_header_label(&self) -> Option<String> {
        if let Some(steering) = self.steering_queue.front() {
            return Some(steering.clone());
        }
        if let Some(follow_up) = self.follow_up_queue.front() {
            return Some(follow_up.clone());
        }
        self.current_goal.as_ref().and_then(|goal| {
            (!matches!(goal.status, ThreadGoalStatus::Complete)).then(|| goal.objective.clone())
        })
    }

    fn render_docks(&mut self) {
        self.sync_ui_docks();
        let rendered = self.ui.render_docks(terminal_width());
        if !rendered.trim().is_empty() {
            println!("{rendered}");
        }
        self.ui.clear_dirty();
    }

    fn prepare_hoppi_recall_for_prompt(
        &mut self,
        prompt: &str,
    ) -> Option<HoppiRecallCommandResult> {
        let params = memory_backend_recall_params_from_settings(
            &self.memory_settings,
            &self.thread_id,
            prompt,
        )
        .ok()?;
        let value = self
            .request_memory_backend_normalized("memory/backend/recall", params)
            .ok()?;
        let recalled = serde_json::from_value::<MemoryRecallResult>(value).ok()?;
        Some(HoppiRecallCommandResult {
            context_markdown: recalled.context_markdown,
            context_bytes: recalled.context_bytes,
            memory_count: recalled.status.memory_count.max(recalled.total_count),
            pinned_count: Some(recalled.status.pinned_count),
            store_path: recalled.status.store_path,
            records: recalled.items,
            omitted_reason: recalled.omitted_reason,
            truncated: recalled.truncated,
        })
    }

    fn hoppi_thread_events(&mut self) -> Option<Vec<Event>> {
        let listed = match self.rpc.request(
            "events/list",
            json!({ "threadId": self.thread_id, "after": 0, "limit": 5000 }),
        ) {
            Ok(value) => value,
            Err(_error) => return None,
        };
        let Some(raw_events) = listed["events"].as_array() else {
            return None;
        };
        Some(
            raw_events
                .iter()
                .filter_map(|raw| serde_json::from_value::<Event>(raw.clone()).ok())
                .collect::<Vec<_>>(),
        )
    }

    fn maybe_remember_hoppi_turn(&mut self, turn_id: &str) {
        let Some(events) = self.hoppi_thread_events() else {
            return;
        };
        if self.maybe_distill_hoppi_turn(&events, turn_id) {
            return;
        }
        let Some(summary) = build_hoppi_turn_summary_from_events(
            &events,
            &self.thread_id,
            turn_id,
            self.last_prompt.as_deref(),
            &hoppi_turn_summary_timestamp(),
        ) else {
            return;
        };
        let params = memory_backend_turn_remember_params_from_settings(
            &self.memory_settings,
            &summary,
            &self.thread_id,
            turn_id,
        );
        if let Ok(params) = params
            && let Ok(value) =
                self.request_memory_backend_normalized("memory/backend/remember", params)
            && let Ok(remembered) = serde_json::from_value::<MemoryRememberResult>(value)
            && remembered.backend_applied
        {
            self.mark_hoppi_memory_dirty();
        }
    }

    fn maybe_distill_hoppi_turn(&mut self, events: &[Event], turn_id: &str) -> bool {
        let params = {
            let Some(provider) = self.active_turn_model_provider.as_ref() else {
                return false;
            };
            match memory_distill_turn_params_from_events(
                &self.memory_settings,
                events,
                Path::new(&self.cwd),
                &self.thread_id,
                turn_id,
                self.last_prompt.as_deref(),
                provider,
            ) {
                Ok(params) => params,
                Err(_) => return false,
            }
        };
        let Ok(value) = self.rpc.request("memory/distill-turn", params) else {
            return true;
        };
        self.record_memory_status_from_value(&value);
        let remembered = value
            .get("remembered")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let recurring_saved = value
            .get("recurringSavedCount")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        if remembered || recurring_saved > 0 {
            self.mark_hoppi_memory_dirty();
        }
        true
    }

    fn maybe_remember_hoppi_exit_recap(&mut self) {
        let _ = self.maybe_remember_hoppi_session_recap(HoppiSessionRecapReason::Exit);
    }

    fn maybe_remember_hoppi_clear_recap(&mut self) -> bool {
        self.maybe_remember_hoppi_session_recap(HoppiSessionRecapReason::Clear)
    }

    fn maybe_remember_hoppi_session_recap(&mut self, reason: HoppiSessionRecapReason) -> bool {
        let Some(events) = self.hoppi_thread_events() else {
            return false;
        };
        let Some(recap) = build_hoppi_session_recap_from_events(
            &events,
            &self.thread_id,
            &hoppi_turn_summary_timestamp(),
        ) else {
            return false;
        };
        let params = memory_backend_session_recap_params_from_settings(
            &self.memory_settings,
            &recap,
            &self.thread_id,
            reason,
        );
        if let Ok(params) = params
            && let Ok(value) =
                self.request_memory_backend_normalized("memory/backend/remember", params)
            && let Ok(remembered) = serde_json::from_value::<MemoryRememberResult>(value)
            && remembered.backend_applied
        {
            self.mark_hoppi_memory_dirty();
            return true;
        }
        false
    }

    fn record_hoppi_memory_status(&mut self, enabled: bool, memory_count: u64) {
        let status = MemoryStatus {
            enabled,
            initialized: enabled,
            backend: "hoppi".to_string(),
            scope: "project".to_string(),
            memory_count,
            ..MemoryStatus::default()
        };
        self.memory_status = Some(status.clone());
        let _ = self.rpc.request(
            "memory/set",
            json!({
                "threadId": self.thread_id,
                "status": status,
            }),
        );
        self.sync_ui_docks();
    }

    fn record_hoppi_dashboard_status(&mut self, result: &HoppiDashboardCommandResult) {
        let status = MemoryStatus {
            enabled: true,
            initialized: true,
            backend: "hoppi".to_string(),
            scope: "project".to_string(),
            store_path: result.store_path.clone(),
            memory_count: result.memory_count,
            pinned_count: result.pinned_count,
            dashboard_url: Some(result.url.clone()),
            ..MemoryStatus::default()
        };
        self.memory_status = Some(status.clone());
        let _ = self.rpc.request(
            "memory/set",
            json!({
                "threadId": self.thread_id,
                "status": status,
            }),
        );
        self.sync_ui_docks();
    }

    fn record_hoppi_list_records(&mut self, result: &HoppiListCommandResult) {
        let status = MemoryStatus {
            enabled: true,
            initialized: true,
            backend: "hoppi".to_string(),
            scope: "project".to_string(),
            store_path: result.store_path.clone(),
            memory_count: result.memory_count.max(result.records.len() as u64),
            pinned_count: result.pinned_count.max(
                result
                    .records
                    .iter()
                    .filter(|record| record.pinned || record.starred)
                    .count() as u64,
            ),
            ..MemoryStatus::default()
        };
        self.memory_status = Some(status.clone());
        let _ = self.rpc.request(
            "memory/records/set",
            json!({
                "threadId": self.thread_id,
                "source": "hoppi-list",
                "status": status,
                "records": result.records,
            }),
        );
        self.sync_ui_docks();
    }

    fn reset_hoppi_background_sync_timer(&mut self) {
        self.next_hoppi_background_sync_at =
            hoppi_next_background_sync_at(&self.memory_settings, Instant::now());
    }

    fn is_idle_for_hoppi_background_sync(&self) -> bool {
        !self.is_turn_running() && !self.has_pending_pause() && self.follow_up_queue.is_empty()
    }

    fn is_idle_for_compaction_offer(&self) -> bool {
        !self.is_turn_running() && !self.has_pending_pause() && self.follow_up_queue.is_empty()
    }

    fn refresh_compaction_idle_timer(&mut self) {
        if self.is_idle_for_compaction_offer() {
            if self.compaction_idle_since.is_none() {
                self.compaction_idle_since = Some(Instant::now());
            }
        } else {
            self.compaction_idle_since = None;
        }
    }

    fn maybe_offer_compaction(
        &mut self,
        provider: &ProviderConfig,
        json_output: bool,
    ) -> Result<(), String> {
        self.refresh_compaction_idle_timer();
        if !self.is_idle_for_compaction_offer() {
            return Ok(());
        }
        let idle_elapsed = self.compaction_idle_since.map(|started| started.elapsed());
        let telemetry = self.compaction_trigger_telemetry_for_provider(provider);
        let Some(offer) = compaction_offer_plan(&self.compact_settings, &telemetry, idle_elapsed)
        else {
            return Ok(());
        };
        if self.last_compaction_offer_key.as_deref() == Some(offer.key.as_str()) {
            return Ok(());
        }
        self.last_compaction_offer_key = Some(offer.key);
        self.print_text(&offer.message, json_output)
    }

    fn maybe_run_hoppi_background_sync(&mut self, json_output: bool) -> Result<(), String> {
        let now = Instant::now();
        if self.next_hoppi_background_sync_at.is_none() {
            self.reset_hoppi_background_sync_timer();
        }
        if !hoppi_background_sync_due(
            &self.memory_settings,
            self.next_hoppi_background_sync_at,
            self.is_idle_for_hoppi_background_sync(),
            now,
        ) {
            return Ok(());
        }

        self.next_hoppi_background_sync_at =
            hoppi_next_background_sync_at(&self.memory_settings, now);
        match self.run_hoppi_sync_with_job_reporting(HoppiSyncDirection::Both, "idle") {
            Ok(run) => self.print_text(
                &format!(
                    "memory: background sync {}",
                    format_hoppi_sync_report(HoppiSyncDirection::Both, &run.result)
                ),
                json_output,
            ),
            Err(error) => self.print_text(
                &format!("memory: background sync skipped: {error}"),
                json_output,
            ),
        }
    }

    fn maybe_run_hoppi_idle_consolidation(&mut self, json_output: bool) -> Result<(), String> {
        let now = Instant::now();
        if self.hoppi_idle_consolidating
            || self.hoppi_idle_consolidation_attempted_version == self.hoppi_dirty_version
            || !hoppi_idle_consolidation_due(
                &self.memory_settings,
                self.hoppi_dirty_version,
                self.hoppi_consolidated_version,
                self.hoppi_dirty_at,
                self.is_idle_for_hoppi_background_sync(),
                now,
            )
        {
            return Ok(());
        }

        self.hoppi_idle_consolidation_attempted_version = self.hoppi_dirty_version;
        self.hoppi_idle_consolidating = true;
        let params = memory_backend_idle_consolidate_params_from_settings(
            &self.memory_settings,
            &self.thread_id,
        );
        let result = match params.and_then(|params| {
            self.request_memory_backend_normalized("memory/backend/consolidate", params)
        }) {
            Ok(value) => match serde_json::from_value::<MemoryConsolidateResult>(value.clone()) {
                Ok(consolidated) => {
                    if !consolidated.dry_run && consolidated.backend_applied {
                        self.mark_hoppi_memory_consolidated();
                    }
                    if json_output {
                        self.print_value("memoryConsolidate", json!(consolidated), json_output)
                    } else {
                        self.print_text(
                            &format!(
                                "memory: idle consolidation\n{}",
                                format_memory_consolidate(&json!(consolidated))
                            ),
                            json_output,
                        )
                    }
                }
                Err(error) => Err(format!("decode memory/backend/consolidate: {error}")),
            },
            Err(error) => self.print_text(
                &format!("memory: idle consolidation skipped: {error}"),
                json_output,
            ),
        };
        self.hoppi_idle_consolidating = false;
        result
    }

    fn print_exit_requested(&mut self, json_output: bool) -> Result<(), String> {
        let resume_command = exit_resume_command(&self.thread_id);
        if self.terminal_ui_active && !json_output {
            self.ui.push_transcript(EXIT_COMMAND_ECHO_TEXT.to_string());
            self.ui.push_transcript(EXIT_REQUESTED_TEXT.to_string());
            self.ui.push_transcript(resume_command);
            self.sync_ui_docks();
            return Ok(());
        }
        self.print_text(
            &format!("{EXIT_REQUESTED_TEXT}\n{resume_command}"),
            json_output,
        )
    }

    fn print_text(&mut self, text: &str, json_output: bool) -> Result<(), String> {
        if self.terminal_ui_active && !json_output {
            self.ui.push_transcript(text.to_string());
            self.sync_ui_docks();
            return Ok(());
        }
        if json_output {
            println!("{}", json!({ "shell": text }));
        } else {
            println!("{text}");
        }
        Ok(())
    }

    fn print_value(&mut self, label: &str, value: Value, json_output: bool) -> Result<(), String> {
        if json_output {
            println!("{}", json!({ label: value }));
        } else {
            let rendered = format!(
                "{label}: {}",
                serde_json::to_string_pretty(&value).map_err(|error| error.to_string())?
            );
            if self.terminal_ui_active {
                self.ui.push_transcript(rendered);
                self.sync_ui_docks();
            } else {
                println!("{rendered}");
            }
        }
        Ok(())
    }

    fn shutdown(&mut self) {
        if let Some(mut process) = self.hoppi_dashboard_process.take() {
            let _ = process.child.kill();
            let _ = process.child.wait();
        }
        if let Some(mut child) = self.meridian_child.take() {
            let _ = child.kill();
        }
        let _ = self.rpc.request("server/shutdown", json!({}));
    }
}

fn initialize_rpc_client(rpc: &mut RpcClient) -> Result<(), String> {
    let init = rpc.request(
        "initialize",
        json!({
            "clientName": "oppi-shell",
            "clientVersion": env!("CARGO_PKG_VERSION"),
            "protocolVersion": oppi_protocol::OPPI_PROTOCOL_VERSION,
            "clientCapabilities": ["threads", "turns", "events", "background-turns"]
        }),
    )?;
    validate_initialize_response(&init)
}

fn list_sessions_command(server_path: PathBuf, json_output: bool) -> Result<(), String> {
    let mut rpc = RpcClient::spawn(server_path)?;
    initialize_rpc_client(&mut rpc)?;
    let cwd = env::current_dir().map_err(|error| format!("read cwd: {error}"))?;
    let cwd = cwd.display().to_string();
    let value = rpc.request("thread/list", json!({}))?;
    let result: RuntimeListResult<Thread> =
        serde_json::from_value(value).map_err(|error| format!("decode thread/list: {error}"))?;
    if json_output {
        let items: Vec<Thread> = result
            .items
            .into_iter()
            .filter(|thread| same_project_cwd(&thread.project.cwd, &cwd))
            .collect();
        println!(
            "{}",
            json!({
                "resume": {
                    "projectCwd": cwd,
                    "items": items,
                }
            })
        );
    } else {
        println!(
            "{}",
            format_resume_session_list(&result.items, "", &cwd, &BTreeSet::new())
        );
    }
    let _ = rpc.request("server/shutdown", json!({}));
    Ok(())
}

fn tool_call_id(call: &ToolCall) -> String {
    call.id.clone()
}

fn validate_initialize_response(init: &Value) -> Result<(), String> {
    if init["protocolCompatible"] == json!(false) {
        return Err(format!(
            "oppi-server protocol is incompatible: server={} min={}",
            init["protocolVersion"], init["minProtocolVersion"]
        ));
    }
    if init
        .get("protocolVersion")
        .and_then(Value::as_str)
        .is_none()
    {
        return Err("initialize response missing protocolVersion".to_string());
    }
    Ok(())
}

fn permission_mode_from_env() -> Option<PermissionMode> {
    env::var("OPPI_RUNTIME_WORKER_PERMISSION_MODE")
        .ok()
        .as_deref()
        .and_then(parse_permission_mode)
}

