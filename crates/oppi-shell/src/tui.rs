use super::*;

const SPINNER_FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const SETTINGS_PANEL_ROW_SLOTS: usize = 11;
const SETTINGS_ROOT_ITEM_SLOTS: usize = 5;
const TREE_ENTRY_PICKER_WINDOW_LIMIT: usize = 200;

pub(super) fn retained_tui_interactive_loop(
    session: &mut ShellSession,
    provider: &mut ProviderConfig,
) -> Result<(), String> {
    let _guard = TuiTerminalGuard::enter(session.show_hardware_cursor)?;
    session.terminal_ui_active = true;
    session.print_text(
        "OPPi native TUI ready — /settings opens settings; Ctrl+L selects model; Enter submits; Shift+Enter newline; Ctrl+C twice exits.",
        false,
    )?;
    session.refresh_native_usage(provider, false);
    let mut state = NativeTuiState::default();
    let mut renderer = DifferentialRenderer::with_clear_on_shrink(session.terminal_clear_on_shrink);
    let mut running = true;
    while running {
        let (width, height) = terminal::size().unwrap_or((100, 30));
        state.spinner_index = state.spinner_index.wrapping_add(1);
        session.sync_ui_docks();
        let _ = session.refresh_model_registry_cache(provider, true);
        let frame =
            render_native_tui_frame(session, provider, &state, width as usize, height as usize);
        renderer.set_clear_on_shrink(session.terminal_clear_on_shrink);
        renderer.render(&frame)?;

        if event::poll(Duration::from_millis(40))
            .map_err(|error| format!("poll terminal input: {error}"))?
        {
            match event::read().map_err(|error| format!("read terminal input: {error}"))? {
                CrosstermEvent::Key(key) if key.kind != KeyEventKind::Release => {
                    running = handle_tui_key(session, provider, &mut state, key)?;
                    if !running {
                        session.sync_ui_docks();
                        let (width, height) = terminal::size().unwrap_or((100, 30));
                        let frame = render_native_tui_frame(
                            session,
                            provider,
                            &state,
                            width as usize,
                            height as usize,
                        );
                        renderer.set_clear_on_shrink(session.terminal_clear_on_shrink);
                        renderer.render(&frame)?;
                    }
                }
                CrosstermEvent::Resize(_, _) => renderer.invalidate(),
                _ => {}
            }
        }

        if let Some(outcome) = session.poll_turn_events_silent()? {
            if matches!(outcome, TurnOutcome::Completed) {
                session.refresh_native_usage(provider, false);
                session.start_next_queued_or_goal_continuation(provider, false)?;
            }
        } else if session.maybe_timeout_pending_question(provider, false)? {
            session.refresh_native_usage(provider, false);
        } else if !session.is_turn_running() && !session.has_pending_pause() {
            session.start_next_queued_or_goal_continuation(provider, false)?;
        }
        session.maybe_run_hoppi_background_sync(false)?;
        session.maybe_offer_compaction(provider, false)?;
    }
    session.terminal_ui_active = false;
    Ok(())
}

#[derive(Debug)]
pub(super) struct NativeTuiState {
    pub(super) editor: LineEditor,
    overlay: Option<TuiOverlay>,
    pub(super) spinner_index: usize,
    pub(super) slash_selected: usize,
    pub(super) question_selected: usize,
    pub(super) footer_hotkeys_visible: bool,
    slash_keyboard_mode: bool,
    slash_palette_suppressed: bool,
}

impl Default for NativeTuiState {
    fn default() -> Self {
        Self {
            editor: LineEditor::default(),
            overlay: None,
            spinner_index: 0,
            slash_selected: 0,
            question_selected: 0,
            footer_hotkeys_visible: true,
            slash_keyboard_mode: false,
            slash_palette_suppressed: false,
        }
    }
}

impl NativeTuiState {
    fn reset_slash_navigation(&mut self) {
        self.slash_selected = 0;
        self.slash_keyboard_mode = false;
        self.slash_palette_suppressed = false;
    }

    fn handle_chrome_key(&mut self, key: event::KeyEvent) -> bool {
        if is_alt_k_key(key) {
            self.footer_hotkeys_visible = !self.footer_hotkeys_visible;
            return true;
        }
        false
    }

    fn current_slash_palette(
        &self,
        session: &ShellSession,
        provider: &ProviderConfig,
    ) -> Option<SlashPalette> {
        if self.slash_palette_suppressed {
            return None;
        }
        slash_palette_for_buffer_with_session(
            self.editor.buffer_preview(),
            self.slash_selected,
            session,
            provider,
        )
    }

    pub(super) fn theme_preview_name(&self) -> Option<String> {
        match &self.overlay {
            Some(TuiOverlay::SettingsPanel {
                panel: SettingsSubpanel::Theme,
                selected,
                items,
                ..
            }) => items
                .get(*selected)
                .and_then(theme_preview_name_for_settings_item),
            _ => None,
        }
    }

    #[cfg(test)]
    pub(super) fn settings_overlay_selected(&self) -> Option<usize> {
        match &self.overlay {
            Some(TuiOverlay::Settings { selected, .. }) => Some(*selected),
            Some(TuiOverlay::SettingsPanel { selected, .. }) => Some(*selected),
            _ => None,
        }
    }

    pub(super) fn has_overlay(&self) -> bool {
        self.overlay.is_some()
    }

    #[cfg(test)]
    pub(super) fn has_settings_overlay(&self) -> bool {
        matches!(
            self.overlay,
            Some(TuiOverlay::Settings { .. } | TuiOverlay::SettingsPanel { .. })
        )
    }

    pub(super) fn overlay_view(
        &self,
        session: &ShellSession,
        provider: &ProviderConfig,
    ) -> Option<TuiOverlayView> {
        match &self.overlay {
            Some(TuiOverlay::Settings { selected, query }) => {
                let root_items = settings_root_items(session, provider);
                Some(TuiOverlayView {
                    title: "settings".to_string(),
                    query: query.clone(),
                    selected: *selected,
                    items: root_items
                        .into_iter()
                        .map(|item| TuiOverlayViewItem {
                            label: format!("{} › {}", item.section, item.label),
                            value: item.value.clone(),
                            detail: item.detail.to_string(),
                        })
                        .collect(),
                })
            }
            Some(TuiOverlay::SettingsPanel {
                panel,
                selected,
                items,
                ..
            }) => Some(TuiOverlayView {
                title: format!("settings › {}", settings_subpanel_title(*panel)),
                query: String::new(),
                selected: *selected,
                items: items
                    .iter()
                    .enumerate()
                    .map(|(row, item)| TuiOverlayViewItem {
                        label: item.label.clone(),
                        value: settings_panel_item_value_for_render(*panel, item, row, *selected),
                        detail: settings_panel_item_detail_for_render(*panel, item, row, *selected),
                    })
                    .collect(),
            }),
            Some(TuiOverlay::Sessions {
                selected,
                query,
                items,
            }) => Some(TuiOverlayView {
                title: if query.is_empty() {
                    "sessions".to_string()
                } else {
                    format!("sessions · filter: {query}")
                },
                query: query.clone(),
                selected: *selected,
                items: filtered_session_items(items, query)
                    .into_iter()
                    .map(|(_, item)| TuiOverlayViewItem {
                        label: if item.current {
                            format!("● {}", item.title)
                        } else {
                            item.title.clone()
                        },
                        value: item.status.clone(),
                        detail: format!(
                            "{}{}",
                            item.cwd,
                            item.forked_from
                                .as_ref()
                                .map(|from| format!(" · forked from {from}"))
                                .unwrap_or_default()
                        ),
                    })
                    .collect(),
            }),
            Some(TuiOverlay::TreeEntries {
                selected,
                query,
                entries,
                branch,
                filter_mode,
                after,
                total_count,
                ..
            }) => Some(TuiOverlayView {
                title: tree_entries_overlay_title(query, *filter_mode),
                query: query.clone(),
                selected: *selected,
                items: filtered_tree_entry_items(entries, query)
                    .into_iter()
                    .map(|(_, entry)| TuiOverlayViewItem {
                        label: tree_entry_picker_label(entry),
                        value: format!("event {}", entry.event_id),
                        detail: tree_entry_picker_detail(
                            entry,
                            branch.as_ref(),
                            *after,
                            *total_count,
                        ),
                    })
                    .collect(),
            }),
            Some(TuiOverlay::TreeEntryLabelInput {
                event_id,
                input,
                preview,
            }) => Some(TuiOverlayView {
                title: format!("label event {event_id}"),
                query: input.clone(),
                selected: 0,
                items: vec![TuiOverlayViewItem {
                    label: format!("event {event_id}"),
                    value: input.clone(),
                    detail: preview.clone(),
                }],
            }),
            Some(TuiOverlay::TreeEntryTextInput {
                action,
                event_id,
                input,
                preview,
            }) => Some(TuiOverlayView {
                title: format!("{} event {event_id}", tree_entry_text_input_title(*action)),
                query: input.clone(),
                selected: 0,
                items: vec![TuiOverlayViewItem {
                    label: format!("event {event_id}"),
                    value: input.clone(),
                    detail: preview.clone(),
                }],
            }),
            Some(TuiOverlay::Effort {
                selected,
                allowed,
                current,
            }) => Some(TuiOverlayView {
                title: format!("effort · {}", effort_model_name(provider)),
                query: String::new(),
                selected: *selected,
                items: allowed
                    .iter()
                    .enumerate()
                    .map(|(index, level)| TuiOverlayViewItem {
                        label: if *level == *current {
                            format!("● {}", effort_level_label_for_provider(provider, *level))
                        } else {
                            effort_level_label_for_provider(provider, *level).to_string()
                        },
                        value: if index == *selected {
                            "selected".to_string()
                        } else {
                            level.as_str().to_string()
                        },
                        detail: level.description().to_string(),
                    })
                    .collect(),
            }),
            Some(TuiOverlay::Models {
                selected,
                query,
                scope,
                items,
                ..
            }) => Some(TuiOverlayView {
                title: if query.is_empty() {
                    format!("main model · {}", model_picker_scope_name(*scope))
                } else {
                    format!(
                        "main model · {} · filter: {query}",
                        model_picker_scope_name(*scope)
                    )
                },
                query: query.clone(),
                selected: *selected,
                items: filtered_model_items(items, query)
                    .into_iter()
                    .map(|(_, item)| TuiOverlayViewItem {
                        label: if item.current {
                            format!("● {}", item.label)
                        } else {
                            item.label.clone()
                        },
                        value: model_picker_action_text(&item.action),
                        detail: item.detail.clone(),
                    })
                    .collect(),
            }),
            Some(TuiOverlay::SandboxDirectories {
                selected,
                query,
                kind,
                edit,
                cwd,
                items,
            }) => Some(TuiOverlayView {
                title: if query.is_empty() {
                    format!(
                        "sandbox {} root {} · {}",
                        sandbox_root_kind_label(*kind),
                        sandbox_root_edit_mode_label(*edit),
                        cwd.display()
                    )
                } else {
                    format!(
                        "sandbox {} root {} · filter: {query}",
                        sandbox_root_kind_label(*kind),
                        sandbox_root_edit_mode_label(*edit)
                    )
                },
                query: query.clone(),
                selected: *selected,
                items: filtered_sandbox_directory_items(items, query)
                    .into_iter()
                    .map(|(_, item)| TuiOverlayViewItem {
                        label: item.label.clone(),
                        value: sandbox_directory_item_value(&item.action),
                        detail: sandbox_directory_item_detail_for_render(item),
                    })
                    .collect(),
            }),
            Some(TuiOverlay::SandboxRootEntries {
                selected,
                query,
                kind,
                items,
                selected_roots,
            }) => Some(TuiOverlayView {
                title: if query.is_empty() {
                    format!("sandbox {} root remove", sandbox_root_kind_label(*kind))
                } else {
                    format!(
                        "sandbox {} root remove · filter: {query}",
                        sandbox_root_kind_label(*kind)
                    )
                },
                query: query.clone(),
                selected: *selected,
                items: filtered_sandbox_root_entry_items(items, query)
                    .into_iter()
                    .map(|(_, item)| TuiOverlayViewItem {
                        label: item.label.clone(),
                        value: sandbox_root_entry_action_label(item, selected_roots),
                        detail: item.detail.clone(),
                    })
                    .collect(),
            }),
            None => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum TuiOverlay {
    Settings {
        selected: usize,
        query: String,
    },
    SettingsPanel {
        panel: SettingsSubpanel,
        selected: usize,
        items: Vec<SettingsPanelItem>,
        back_stack: Vec<SettingsSubpanel>,
    },
    Sessions {
        selected: usize,
        query: String,
        items: Vec<SessionPickerItem>,
    },
    TreeEntries {
        selected: usize,
        query: String,
        entries: Vec<ThreadItemEntry>,
        branch: Option<ThreadItemBranch>,
        filter_mode: TreeFilterMode,
        after: u64,
        total_count: u64,
        previous_afters: Vec<u64>,
    },
    TreeEntryLabelInput {
        event_id: u64,
        input: String,
        preview: String,
    },
    TreeEntryTextInput {
        action: TreeEntryTextInputAction,
        event_id: u64,
        input: String,
        preview: String,
    },
    Effort {
        selected: usize,
        allowed: Vec<ThinkingLevel>,
        current: ThinkingLevel,
    },
    Models {
        selected: usize,
        query: String,
        scope: ModelPickerScope,
        items: Vec<ModelPickerItem>,
        all_items: Vec<ModelPickerItem>,
        scoped_items: Vec<ModelPickerItem>,
    },
    SandboxDirectories {
        selected: usize,
        query: String,
        kind: SandboxRootKind,
        edit: SandboxRootEditMode,
        cwd: PathBuf,
        items: Vec<SandboxDirectoryItem>,
    },
    SandboxRootEntries {
        selected: usize,
        query: String,
        kind: SandboxRootKind,
        items: Vec<SandboxRootEntryItem>,
        selected_roots: Vec<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ModelPickerScope {
    All,
    Scoped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TuiPromptPickerRoute {
    TreeEntries,
    Sessions,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TreeEntryPickerAction {
    Fork,
    Edit,
    Resubmit,
    Label,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TreeEntryBranchNavigation {
    Parent,
    Child,
    PreviousSibling,
    NextSibling,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TreeEntryTextInputAction {
    Edit,
    Resubmit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TreeEntryPickerEffect {
    RunCommand(String),
    DraftCommand(String),
    OpenTextInput {
        action: TreeEntryTextInputAction,
        event_id: u64,
        input: String,
        preview: String,
    },
    OpenLabelInput {
        event_id: u64,
        input: String,
        preview: String,
    },
}

fn model_picker_scope_name(scope: ModelPickerScope) -> &'static str {
    match scope {
        ModelPickerScope::All => "all",
        ModelPickerScope::Scoped => "scoped",
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TuiOverlayView {
    pub(super) title: String,
    pub(super) query: String,
    pub(super) selected: usize,
    pub(super) items: Vec<TuiOverlayViewItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TuiOverlayViewItem {
    pub(super) label: String,
    pub(super) value: String,
    pub(super) detail: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SettingsSubpanel {
    General,
    Footer,
    Images,
    Compaction,
    Theme,
    Permissions,
    Provider,
    Login,
    LoginSubscription,
    LoginApi,
    LoginClaude,
    Memory,
    RoleModels,
    ScopedModels,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SettingsRootAction {
    OpenPanel(SettingsSubpanel),
    OpenMainModel,
    OpenEffort,
    OpenSessions,
    InsertCommand(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SettingsRootKeyAction {
    MoveRow(isize),
    MoveTab(isize),
    First,
    Last,
    Activate,
    Search(String),
    SearchBackspace,
    Close,
    ArmCtrlCExit,
    Exit,
    Ignore,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SettingsRootItem {
    section: &'static str,
    label: &'static str,
    value: String,
    detail: &'static str,
    action: SettingsRootAction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SettingsPanelItem {
    label: String,
    value: String,
    detail: String,
    current: bool,
    action: SettingsPanelAction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SettingsPanelAction {
    Command(String),
    EditCommand(String),
    OpenPanel(SettingsSubpanel),
    OpenSandboxDirectoryPicker {
        kind: SandboxRootKind,
        edit: SandboxRootEditMode,
    },
    OpenSandboxRootRemovePicker(SandboxRootKind),
    Model(ModelPickerAction),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SessionPickerItem {
    id: String,
    title: String,
    status: String,
    cwd: String,
    forked_from: Option<String>,
    current: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ModelPickerItem {
    label: String,
    detail: String,
    action: ModelPickerAction,
    current: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ModelPickerAction {
    SelectSession(String),
    SelectRole { role: String, model: String },
    ClearRole { role: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SandboxRootKind {
    Project,
    Read,
    Write,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SandboxRootEditMode {
    Set,
    Add,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SandboxDirectoryItem {
    label: String,
    detail: String,
    path: PathBuf,
    action: SandboxDirectoryAction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SandboxDirectoryAction {
    Choose(PathBuf),
    Open(PathBuf),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SandboxRootEntryItem {
    label: String,
    detail: String,
    root: String,
}

struct TuiTerminalGuard;

impl TuiTerminalGuard {
    fn enter(show_hardware_cursor: bool) -> Result<Self, String> {
        terminal::enable_raw_mode()
            .map_err(|error| format!("enable raw terminal mode: {error}"))?;
        print!("{}", terminal_cursor_setup_sequence(show_hardware_cursor));
        io::stdout()
            .flush()
            .map_err(|error| format!("flush terminal setup: {error}"))?;
        Ok(Self)
    }
}

impl Drop for TuiTerminalGuard {
    fn drop(&mut self) {
        print!("\x1b[?25h\x1b[0m\r\n");
        let _ = io::stdout().flush();
        let _ = terminal::disable_raw_mode();
    }
}

#[derive(Debug, Default)]
struct DifferentialRenderer {
    previous: Vec<String>,
    previous_width: usize,
    max_lines_rendered: usize,
    clear_on_shrink: bool,
}

impl DifferentialRenderer {
    fn with_clear_on_shrink(clear_on_shrink: bool) -> Self {
        Self {
            clear_on_shrink,
            ..Self::default()
        }
    }

    fn set_clear_on_shrink(&mut self, clear_on_shrink: bool) {
        self.clear_on_shrink = clear_on_shrink;
    }

    fn invalidate(&mut self) {
        self.previous.clear();
        self.previous_width = 0;
        self.max_lines_rendered = 0;
    }

    fn render(&mut self, lines: &[String]) -> Result<(), String> {
        let out = self.render_to_string(lines);
        print!("{out}");
        io::stdout()
            .flush()
            .map_err(|error| format!("flush TUI frame: {error}"))
    }

    fn render_to_string(&mut self, lines: &[String]) -> String {
        let width = lines
            .iter()
            .map(|line| tui_visible_width(line))
            .max()
            .unwrap_or(0);
        let mut out = String::new();
        let content_shrunk = lines.len() < self.max_lines_rendered;
        out.push_str("\x1b[?2026h");
        if self.previous.is_empty() || self.previous_width != width {
            if !self.previous.is_empty() {
                out.push('\r');
                if self.previous.len() > 1 {
                    out.push_str(&format!("\x1b[{}A", self.previous.len() - 1));
                }
                out.push_str("\x1b[J");
            }
            for (index, line) in lines.iter().enumerate() {
                if index > 0 {
                    out.push_str("\r\n");
                }
                out.push_str("\x1b[2K");
                out.push_str(line);
            }
        } else {
            let max_len = self.previous.len().max(lines.len());
            let first_changed =
                (0..max_len).find(|index| self.previous.get(*index) != lines.get(*index));
            if let Some(first) = first_changed {
                out.push('\r');
                if self.previous.len() > 1 {
                    out.push_str(&format!("\x1b[{}A", self.previous.len() - 1));
                }
                if first > 0 {
                    out.push_str(&format!("\x1b[{}B", first));
                }
                if !content_shrunk || self.clear_on_shrink {
                    out.push_str("\x1b[J");
                }
                for (offset, line) in lines.iter().enumerate().skip(first) {
                    if offset > first {
                        out.push_str("\r\n");
                    }
                    out.push_str("\x1b[2K");
                    out.push_str(line);
                }
            }
        }
        out.push_str("\x1b[?2026l");
        self.previous = lines.to_vec();
        self.previous_width = width;
        self.max_lines_rendered = self.max_lines_rendered.max(lines.len());
        out
    }
}

pub(super) fn handle_tui_key(
    session: &mut ShellSession,
    provider: &mut ProviderConfig,
    state: &mut NativeTuiState,
    key: event::KeyEvent,
) -> Result<bool, String> {
    if state.handle_chrome_key(key) {
        return Ok(true);
    }
    if is_ctrl_c_key(key) && state.editor.ctrl_c_exit_armed() {
        let action = state.editor.handle(EditorInput::CtrlC);
        return handle_tui_editor_action(session, provider, state, action);
    }
    if state.overlay.is_some() {
        return handle_tui_overlay_key(session, provider, state, key);
    }
    if handle_tui_question_key(session, provider, state, key)? {
        return Ok(true);
    }
    if handle_tui_live_dock_key(session, provider, state, key)? {
        return Ok(true);
    }
    if handle_tui_app_key(session, provider, state, key)? {
        return Ok(true);
    }
    let Some(input) = editor_input_from_key(key) else {
        return Ok(true);
    };
    if let Some(keep_running) = handle_tui_slash_palette_input(session, provider, state, &input)? {
        return Ok(keep_running);
    }
    let action = state.editor.handle(input.clone());
    if matches!(
        input,
        EditorInput::Text(_)
            | EditorInput::Backspace
            | EditorInput::CtrlBackspace
            | EditorInput::AltBackspace
            | EditorInput::Delete
    ) {
        state.reset_slash_navigation();
    }
    handle_tui_editor_action(session, provider, state, action)
}

fn handle_tui_question_key(
    session: &mut ShellSession,
    provider: &mut ProviderConfig,
    state: &mut NativeTuiState,
    key: event::KeyEvent,
) -> Result<bool, String> {
    if session.pending_question.is_none() {
        return Ok(false);
    }
    let Some(input) = editor_input_from_key(key) else {
        return Ok(true);
    };
    let choices = pending_question_answer_choices(session);
    match input {
        EditorInput::Up => {
            state.question_selected = state.question_selected.saturating_sub(1);
            Ok(true)
        }
        EditorInput::Down => {
            state.question_selected =
                (state.question_selected + 1).min(choices.len().saturating_sub(1));
            Ok(true)
        }
        EditorInput::Home | EditorInput::PageUp => {
            state.question_selected = 0;
            Ok(true)
        }
        EditorInput::End | EditorInput::PageDown => {
            state.question_selected = choices.len().saturating_sub(1);
            Ok(true)
        }
        EditorInput::Enter => {
            let typed = state.editor.buffer_preview().trim().to_string();
            if !typed.is_empty() && !typed.starts_with('/') {
                state.editor.replace_buffer(String::new());
                session.answer_pending_question(&typed, provider, false)?;
                state.question_selected = 0;
                return Ok(true);
            }
            let selected = state.question_selected.min(choices.len().saturating_sub(1));
            if let Some(choice) = choices
                .get(selected)
                .and_then(|choice| choice.answer.clone())
            {
                session.answer_pending_question(&choice, provider, false)?;
                state.question_selected = 0;
                return Ok(true);
            }
            session.print_text(
                "type a custom answer in the editor, then press Enter",
                false,
            )?;
            Ok(true)
        }
        _ => Ok(false),
    }
}

fn handle_tui_live_dock_key(
    session: &mut ShellSession,
    provider: &mut ProviderConfig,
    state: &mut NativeTuiState,
    key: event::KeyEvent,
) -> Result<bool, String> {
    let Some(input) = editor_input_from_key(key) else {
        return Ok(false);
    };

    if session.pending_approval.is_some() {
        match input {
            EditorInput::Text(text) if text.eq_ignore_ascii_case("a") => {
                session.resume_pending_approval(provider, false)?;
                return Ok(true);
            }
            EditorInput::Text(text) if text.eq_ignore_ascii_case("d") => {
                session.deny_pending_approval(false)?;
                return Ok(true);
            }
            EditorInput::CtrlC => {
                state.editor.arm_ctrl_c_exit();
                return Ok(true);
            }
            EditorInput::CtrlD => return Ok(false),
            _ => {}
        }
    }

    if session.background_summary.is_some() {
        match input {
            EditorInput::Text(text) if text.eq_ignore_ascii_case("l") => {
                session.handle_command("/background list", provider, false)?;
                return Ok(true);
            }
            EditorInput::Text(text) if text.eq_ignore_ascii_case("r") => {
                state
                    .editor
                    .replace_buffer("/background read latest".to_string());
                return Ok(true);
            }
            EditorInput::Text(text) if text.eq_ignore_ascii_case("k") => {
                state
                    .editor
                    .replace_buffer("/background kill latest".to_string());
                return Ok(true);
            }
            _ => {}
        }
    }

    if let Some(suggestion) = session.suggestion.as_ref() {
        match input {
            EditorInput::Tab => {
                state.editor.replace_buffer(suggestion.message.clone());
                session.suggestion = None;
                return Ok(true);
            }
            EditorInput::Escape => {
                session.suggestion = None;
                return Ok(true);
            }
            _ => {}
        }
    }

    Ok(false)
}

fn handle_tui_app_key(
    session: &mut ShellSession,
    provider: &mut ProviderConfig,
    state: &mut NativeTuiState,
    key: event::KeyEvent,
) -> Result<bool, String> {
    let modifiers = key.modifiers;
    match key.code {
        KeyCode::Char('l') if modifiers.contains(KeyModifiers::CONTROL) => {
            open_models_overlay(session, provider, state)?;
            Ok(true)
        }
        KeyCode::Char('t')
            if modifiers.contains(KeyModifiers::CONTROL)
                && modifiers.contains(KeyModifiers::ALT) =>
        {
            session.handle_command("/background list", provider, false)?;
            Ok(true)
        }
        KeyCode::Char('p')
            if modifiers.contains(KeyModifiers::CONTROL)
                && modifiers.contains(KeyModifiers::SHIFT) =>
        {
            cycle_session_model(session, provider, true)?;
            Ok(true)
        }
        KeyCode::Char('P') if modifiers.contains(KeyModifiers::CONTROL) => {
            cycle_session_model(session, provider, true)?;
            Ok(true)
        }
        KeyCode::Char('p') if modifiers.contains(KeyModifiers::CONTROL) => {
            cycle_session_model(session, provider, false)?;
            Ok(true)
        }
        KeyCode::BackTab => {
            cycle_reasoning_effort(session, provider)?;
            Ok(true)
        }
        _ => Ok(false),
    }
}

fn handle_tui_slash_palette_input(
    session: &mut ShellSession,
    provider: &mut ProviderConfig,
    state: &mut NativeTuiState,
    input: &EditorInput,
) -> Result<Option<bool>, String> {
    let buffer = state.editor.buffer_preview().to_string();
    let Some(palette) = state.current_slash_palette(session, provider) else {
        if !buffer.trim_start().starts_with('/') {
            state.reset_slash_navigation();
        }
        return Ok(None);
    };
    state.slash_selected = palette.selected;
    let count = palette.items.len();
    match input {
        EditorInput::Up => {
            state.slash_selected = state.slash_selected.saturating_sub(1);
            state.slash_keyboard_mode = true;
            Ok(Some(true))
        }
        EditorInput::Down => {
            state.slash_selected = (state.slash_selected + 1).min(count.saturating_sub(1));
            state.slash_keyboard_mode = true;
            Ok(Some(true))
        }
        EditorInput::PageUp => {
            state.slash_selected = state.slash_selected.saturating_sub(8);
            state.slash_keyboard_mode = true;
            Ok(Some(true))
        }
        EditorInput::PageDown => {
            state.slash_selected = (state.slash_selected + 8).min(count.saturating_sub(1));
            state.slash_keyboard_mode = true;
            Ok(Some(true))
        }
        EditorInput::Home => {
            state.slash_selected = 0;
            state.slash_keyboard_mode = true;
            Ok(Some(true))
        }
        EditorInput::End => {
            state.slash_selected = count.saturating_sub(1);
            state.slash_keyboard_mode = true;
            Ok(Some(true))
        }
        EditorInput::Tab | EditorInput::Enter => {
            let submit_allowed = matches!(input, EditorInput::Enter);
            match slash_palette_accept_from_palette(
                &buffer,
                &palette,
                state.slash_keyboard_mode,
                submit_allowed,
            ) {
                Some(SlashPaletteAccept::Insert(replacement)) => {
                    state.editor.replace_buffer(replacement);
                    state.reset_slash_navigation();
                    Ok(Some(true))
                }
                Some(SlashPaletteAccept::Submit(command)) => {
                    state.editor.replace_buffer(command);
                    let action = state.editor.handle(EditorInput::Enter);
                    state.reset_slash_navigation();
                    handle_tui_editor_action(session, provider, state, action).map(Some)
                }
                None => Ok(None),
            }
        }
        EditorInput::Escape => {
            state.slash_palette_suppressed = true;
            state.slash_keyboard_mode = false;
            Ok(Some(true))
        }
        EditorInput::CtrlC => {
            state.slash_palette_suppressed = true;
            state.slash_keyboard_mode = false;
            state.editor.arm_ctrl_c_exit();
            Ok(Some(true))
        }
        _ => Ok(None),
    }
}

fn handle_tui_overlay_key(
    session: &mut ShellSession,
    provider: &mut ProviderConfig,
    state: &mut NativeTuiState,
    key: event::KeyEvent,
) -> Result<bool, String> {
    let Some(input) = editor_input_from_key(key) else {
        return Ok(true);
    };
    match state.overlay.as_mut() {
        Some(TuiOverlay::Settings { selected, query }) => match settings_root_key_action(&input) {
            SettingsRootKeyAction::MoveRow(delta) => {
                let items = settings_root_items(session, provider);
                *selected = settings_root_move_visible(&items, query, *selected, delta);
            }
            SettingsRootKeyAction::MoveTab(delta) => {
                let items = settings_root_items(session, provider);
                *selected = if query.trim().is_empty() {
                    settings_root_move_tab(&items, *selected, delta)
                } else {
                    settings_root_move_visible(&items, query, *selected, delta)
                };
            }
            SettingsRootKeyAction::First => {
                let items = settings_root_items(session, provider);
                *selected = first_visible_settings_index(&items, query).unwrap_or(0);
            }
            SettingsRootKeyAction::Last => {
                let items = settings_root_items(session, provider);
                *selected = filtered_settings_root_items(&items, query)
                    .last()
                    .map(|(index, _)| *index)
                    .unwrap_or_else(|| items.len().saturating_sub(1));
            }
            SettingsRootKeyAction::Activate => {
                let items = settings_root_items(session, provider);
                if !query.trim().is_empty()
                    && !filtered_settings_root_items(&items, query)
                        .iter()
                        .any(|(index, _)| *index == *selected)
                {
                    return Ok(true);
                }
                let selected = *selected;
                activate_settings_item(session, provider, state, selected)?;
            }
            SettingsRootKeyAction::Search(text) => {
                query.push_str(&text);
                let items = settings_root_items(session, provider);
                *selected = first_visible_settings_index(&items, query).unwrap_or(0);
            }
            SettingsRootKeyAction::SearchBackspace => {
                if !query.is_empty() {
                    query.pop();
                    let items = settings_root_items(session, provider);
                    *selected = first_visible_settings_index(&items, query).unwrap_or(0);
                }
            }
            SettingsRootKeyAction::Close => state.overlay = None,
            SettingsRootKeyAction::ArmCtrlCExit => {
                state.overlay = None;
                state.editor.arm_ctrl_c_exit();
            }
            SettingsRootKeyAction::Exit => return Ok(false),
            SettingsRootKeyAction::Ignore => {}
        },
        Some(TuiOverlay::SettingsPanel {
            panel,
            selected,
            items,
            back_stack,
        }) => match input {
            EditorInput::Up => *selected = selected.saturating_sub(1),
            EditorInput::Down => *selected = (*selected + 1).min(items.len().saturating_sub(1)),
            EditorInput::Home | EditorInput::PageUp => *selected = 0,
            EditorInput::End | EditorInput::PageDown => *selected = items.len().saturating_sub(1),
            EditorInput::Enter => {
                let chosen = items.get(*selected).cloned();
                if let Some(chosen) = chosen {
                    let panel = *panel;
                    let back_stack = back_stack.clone();
                    activate_settings_panel_item(
                        session,
                        provider,
                        state,
                        panel,
                        &back_stack,
                        &chosen,
                    )?;
                }
            }
            EditorInput::Text(text) if text == " " => {
                let chosen = items.get(*selected).cloned();
                if let Some(chosen) = chosen {
                    let panel = *panel;
                    let back_stack = back_stack.clone();
                    activate_settings_panel_item(
                        session,
                        provider,
                        state,
                        panel,
                        &back_stack,
                        &chosen,
                    )?;
                }
            }
            EditorInput::Text(text) => {
                if let Some(target) = settings_panel_numeric_preview_target(*panel, items, &text) {
                    *selected = target;
                }
            }
            EditorInput::Backspace | EditorInput::BackTab | EditorInput::Left => {
                let back_stack = back_stack.clone();
                return_to_previous_settings_screen(session, provider, state, &back_stack);
            }
            EditorInput::Escape => {
                let back_stack = back_stack.clone();
                return_to_previous_settings_screen(session, provider, state, &back_stack);
            }
            EditorInput::CtrlC => {
                state.overlay = None;
                state.editor.arm_ctrl_c_exit();
            }
            EditorInput::CtrlD => return Ok(false),
            _ => {}
        },
        Some(TuiOverlay::Sessions {
            selected,
            query,
            items,
        }) => match input {
            EditorInput::Text(text) => {
                query.push_str(&text);
                *selected = 0;
            }
            EditorInput::Backspace | EditorInput::Delete => {
                query.pop();
                *selected = 0;
            }
            EditorInput::Up => *selected = selected.saturating_sub(1),
            EditorInput::Down => {
                *selected = (*selected + 1)
                    .min(filtered_session_items(items, query).len().saturating_sub(1));
            }
            EditorInput::PageUp => *selected = selected.saturating_sub(8),
            EditorInput::PageDown => {
                *selected = (*selected + 8)
                    .min(filtered_session_items(items, query).len().saturating_sub(1));
            }
            EditorInput::Home => *selected = 0,
            EditorInput::End => {
                *selected = filtered_session_items(items, query).len().saturating_sub(1);
            }
            EditorInput::Enter => {
                let chosen = selected_session_item(items, query, *selected).cloned();
                state.overlay = None;
                if let Some(chosen) = chosen {
                    resume_session_picker_item(session, &chosen)?;
                }
            }
            EditorInput::Escape => state.overlay = None,
            EditorInput::CtrlC => {
                state.overlay = None;
                state.editor.arm_ctrl_c_exit();
            }
            EditorInput::CtrlD => return Ok(false),
            _ => {}
        },
        Some(TuiOverlay::TreeEntries {
            selected,
            query,
            entries,
            branch,
            filter_mode,
            after,
            total_count,
            previous_afters,
        }) => match input {
            EditorInput::Text(text) if matches!(text.as_str(), "e" | "E") => {
                if let Some(effect) = selected_tree_entry_item(entries, query, *selected)
                    .map(|entry| tree_entry_picker_effect(TreeEntryPickerAction::Edit, entry))
                {
                    state.overlay = None;
                    match effect {
                        TreeEntryPickerEffect::RunCommand(command) => {
                            let _ = session.handle_command(&command, provider, false)?;
                        }
                        TreeEntryPickerEffect::DraftCommand(command) => {
                            state.editor.replace_buffer(command);
                        }
                        TreeEntryPickerEffect::OpenTextInput {
                            action,
                            event_id,
                            input,
                            preview,
                        } => {
                            state.overlay = Some(TuiOverlay::TreeEntryTextInput {
                                action,
                                event_id,
                                input,
                                preview,
                            });
                        }
                        TreeEntryPickerEffect::OpenLabelInput {
                            event_id,
                            input,
                            preview,
                        } => {
                            state.overlay = Some(TuiOverlay::TreeEntryLabelInput {
                                event_id,
                                input,
                                preview,
                            });
                        }
                    }
                }
            }
            EditorInput::Text(text) if matches!(text.as_str(), "r" | "R") => {
                if let Some(effect) = selected_tree_entry_item(entries, query, *selected)
                    .map(|entry| tree_entry_picker_effect(TreeEntryPickerAction::Resubmit, entry))
                {
                    state.overlay = None;
                    match effect {
                        TreeEntryPickerEffect::RunCommand(command) => {
                            let _ = session.handle_command(&command, provider, false)?;
                        }
                        TreeEntryPickerEffect::DraftCommand(command) => {
                            state.editor.replace_buffer(command);
                        }
                        TreeEntryPickerEffect::OpenTextInput {
                            action,
                            event_id,
                            input,
                            preview,
                        } => {
                            state.overlay = Some(TuiOverlay::TreeEntryTextInput {
                                action,
                                event_id,
                                input,
                                preview,
                            });
                        }
                        TreeEntryPickerEffect::OpenLabelInput {
                            event_id,
                            input,
                            preview,
                        } => {
                            state.overlay = Some(TuiOverlay::TreeEntryLabelInput {
                                event_id,
                                input,
                                preview,
                            });
                        }
                    }
                }
            }
            EditorInput::Text(text) if matches!(text.as_str(), "l" | "L") => {
                if let Some(effect) = selected_tree_entry_item(entries, query, *selected)
                    .map(|entry| tree_entry_picker_effect(TreeEntryPickerAction::Label, entry))
                {
                    state.overlay = None;
                    match effect {
                        TreeEntryPickerEffect::RunCommand(command) => {
                            let _ = session.handle_command(&command, provider, false)?;
                        }
                        TreeEntryPickerEffect::DraftCommand(command) => {
                            state.editor.replace_buffer(command);
                        }
                        TreeEntryPickerEffect::OpenTextInput {
                            action,
                            event_id,
                            input,
                            preview,
                        } => {
                            state.overlay = Some(TuiOverlay::TreeEntryTextInput {
                                action,
                                event_id,
                                input,
                                preview,
                            });
                        }
                        TreeEntryPickerEffect::OpenLabelInput {
                            event_id,
                            input,
                            preview,
                        } => {
                            state.overlay = Some(TuiOverlay::TreeEntryLabelInput {
                                event_id,
                                input,
                                preview,
                            });
                        }
                    }
                }
            }
            EditorInput::Text(text)
                if tree_entry_branch_navigation_key(text.as_str()).is_some() =>
            {
                let navigation = tree_entry_branch_navigation_key(text.as_str()).unwrap();
                if let Some(target_thread_id) =
                    tree_entry_branch_navigation_target(branch.as_ref(), navigation)
                {
                    let value = session
                        .rpc
                        .request("thread/resume", json!({ "threadId": target_thread_id }))?;
                    session.switch_to_thread(value, false, "resumed")?;
                    let result = load_tree_entries_overlay_window(session, *filter_mode, 0)?;
                    *entries = result.entries;
                    *branch = Some(result.branch);
                    *after = result.after;
                    *total_count = result.total_count;
                    previous_afters.clear();
                    query.clear();
                    *selected = 0;
                }
            }
            EditorInput::Text(text) => {
                query.push_str(&text);
                *selected = 0;
            }
            EditorInput::Backspace | EditorInput::Delete => {
                query.pop();
                *selected = 0;
            }
            EditorInput::Left => {
                if let Some(previous_after) =
                    tree_entry_picker_previous_window_after(previous_afters)
                {
                    let result =
                        load_tree_entries_overlay_window(session, *filter_mode, previous_after)?;
                    previous_afters.pop();
                    *entries = result.entries;
                    *branch = Some(result.branch);
                    *after = result.after;
                    *total_count = result.total_count;
                    query.clear();
                    *selected = 0;
                }
            }
            EditorInput::Right => {
                if let Some(next_after) = tree_entry_picker_next_window_after(entries, *total_count)
                {
                    let current_after = *after;
                    let result =
                        load_tree_entries_overlay_window(session, *filter_mode, next_after)?;
                    previous_afters.push(current_after);
                    *entries = result.entries;
                    *branch = Some(result.branch);
                    *after = result.after;
                    *total_count = result.total_count;
                    query.clear();
                    *selected = 0;
                }
            }
            EditorInput::Up => *selected = selected.saturating_sub(1),
            EditorInput::Down => {
                *selected = (*selected + 1).min(
                    filtered_tree_entry_items(entries, query)
                        .len()
                        .saturating_sub(1),
                );
            }
            EditorInput::PageUp => *selected = selected.saturating_sub(8),
            EditorInput::PageDown => {
                *selected = (*selected + 8).min(
                    filtered_tree_entry_items(entries, query)
                        .len()
                        .saturating_sub(1),
                );
            }
            EditorInput::Home => *selected = 0,
            EditorInput::End => {
                *selected = filtered_tree_entry_items(entries, query)
                    .len()
                    .saturating_sub(1);
            }
            EditorInput::Enter => {
                if let Some(effect) = selected_tree_entry_item(entries, query, *selected)
                    .map(|entry| tree_entry_picker_effect(TreeEntryPickerAction::Fork, entry))
                {
                    state.overlay = None;
                    match effect {
                        TreeEntryPickerEffect::RunCommand(command) => {
                            let _ = session.handle_command(&command, provider, false)?;
                        }
                        TreeEntryPickerEffect::DraftCommand(command) => {
                            state.editor.replace_buffer(command);
                        }
                        TreeEntryPickerEffect::OpenTextInput {
                            action,
                            event_id,
                            input,
                            preview,
                        } => {
                            state.overlay = Some(TuiOverlay::TreeEntryTextInput {
                                action,
                                event_id,
                                input,
                                preview,
                            });
                        }
                        TreeEntryPickerEffect::OpenLabelInput {
                            event_id,
                            input,
                            preview,
                        } => {
                            state.overlay = Some(TuiOverlay::TreeEntryLabelInput {
                                event_id,
                                input,
                                preview,
                            });
                        }
                    }
                }
            }
            EditorInput::Escape => state.overlay = None,
            EditorInput::CtrlC => {
                state.overlay = None;
                state.editor.arm_ctrl_c_exit();
            }
            EditorInput::CtrlD => return Ok(false),
            _ => {}
        },
        Some(TuiOverlay::TreeEntryLabelInput {
            event_id,
            input: label_input,
            ..
        }) => match input {
            EditorInput::Text(text) => {
                label_input.push_str(&text);
            }
            EditorInput::Backspace | EditorInput::Delete => {
                label_input.pop();
            }
            EditorInput::Enter => {
                let command = tree_entry_label_input_command(*event_id, label_input);
                state.overlay = None;
                let _ = session.handle_command(&command, provider, false)?;
            }
            EditorInput::Escape => state.overlay = None,
            EditorInput::CtrlC => {
                state.overlay = None;
                state.editor.arm_ctrl_c_exit();
            }
            EditorInput::CtrlD => return Ok(false),
            _ => {}
        },
        Some(TuiOverlay::TreeEntryTextInput {
            action,
            event_id,
            input: text_input,
            ..
        }) => match input {
            EditorInput::Text(text) => {
                text_input.push_str(&text);
            }
            EditorInput::Backspace | EditorInput::Delete => {
                text_input.pop();
            }
            EditorInput::Enter => {
                if let Some(command) = tree_entry_text_input_command(*action, *event_id, text_input)
                {
                    state.overlay = None;
                    let _ = session.handle_command(&command, provider, false)?;
                }
            }
            EditorInput::Escape => state.overlay = None,
            EditorInput::CtrlC => {
                state.overlay = None;
                state.editor.arm_ctrl_c_exit();
            }
            EditorInput::CtrlD => return Ok(false),
            _ => {}
        },
        Some(TuiOverlay::Effort {
            selected,
            allowed,
            current: _,
        }) => match input {
            EditorInput::Left | EditorInput::Up => {
                *selected = selected.saturating_sub(1);
            }
            EditorInput::Text(text) if matches!(text.as_str(), "h" | "k") => {
                *selected = selected.saturating_sub(1);
            }
            EditorInput::Right | EditorInput::Down => {
                *selected = (*selected + 1).min(allowed.len().saturating_sub(1));
            }
            EditorInput::Text(text) if matches!(text.as_str(), "l" | "j") => {
                *selected = (*selected + 1).min(allowed.len().saturating_sub(1));
            }
            EditorInput::Text(text) if text == "a" => {
                let recommended = recommended_effort_level_for_provider(provider);
                *selected = allowed
                    .iter()
                    .position(|level| *level == recommended)
                    .unwrap_or_else(|| allowed.len().saturating_sub(1));
            }
            EditorInput::Home | EditorInput::PageUp => {
                *selected = 0;
            }
            EditorInput::Text(text) if text == "0" => {
                *selected = 0;
            }
            EditorInput::End | EditorInput::PageDown => {
                *selected = allowed.len().saturating_sub(1);
            }
            EditorInput::Text(text) => {
                if let Ok(index) = text.parse::<usize>()
                    && index >= 1
                    && index <= allowed.len()
                {
                    *selected = index - 1;
                }
            }
            EditorInput::Enter => {
                let level = allowed
                    .get(*selected)
                    .copied()
                    .unwrap_or(ThinkingLevel::Off);
                state.overlay = None;
                set_provider_effort_level(provider, level);
                save_reasoning_effort_setting(&session.role_profile_path, level)?;
                session.print_text(
                    &format!(
                        "Effort set to {} for {}.",
                        effort_level_label_for_provider(provider, level),
                        effort_model_name(provider)
                    ),
                    false,
                )?;
            }
            EditorInput::Escape => state.overlay = None,
            EditorInput::CtrlC => {
                state.overlay = None;
                state.editor.arm_ctrl_c_exit();
            }
            EditorInput::CtrlD => return Ok(false),
            _ => {}
        },
        Some(TuiOverlay::Models {
            selected,
            query,
            scope,
            items,
            all_items,
            scoped_items,
        }) => match input {
            EditorInput::Text(text) => {
                query.push_str(&text);
                *selected = 0;
            }
            EditorInput::Backspace | EditorInput::Delete => {
                query.pop();
                *selected = 0;
            }
            EditorInput::Up => *selected = selected.saturating_sub(1),
            EditorInput::Down => {
                *selected =
                    (*selected + 1).min(filtered_model_items(items, query).len().saturating_sub(1));
            }
            EditorInput::PageUp => *selected = selected.saturating_sub(8),
            EditorInput::PageDown => {
                *selected =
                    (*selected + 8).min(filtered_model_items(items, query).len().saturating_sub(1));
            }
            EditorInput::Home => *selected = 0,
            EditorInput::End => {
                *selected = filtered_model_items(items, query).len().saturating_sub(1);
            }
            EditorInput::Tab => {
                toggle_model_picker_scope_fields(
                    selected,
                    query,
                    scope,
                    items,
                    all_items,
                    scoped_items,
                );
            }
            EditorInput::Enter => {
                let chosen = selected_model_item(items, query, *selected).cloned();
                state.overlay = None;
                if let Some(chosen) = chosen {
                    activate_model_picker_item(session, provider, &chosen)?;
                }
            }
            EditorInput::Escape => state.overlay = None,
            EditorInput::CtrlC => {
                state.overlay = None;
                state.editor.arm_ctrl_c_exit();
            }
            EditorInput::CtrlD => return Ok(false),
            _ => {}
        },
        Some(TuiOverlay::SandboxDirectories {
            selected,
            query,
            kind,
            edit,
            cwd,
            items,
        }) => match input {
            EditorInput::Text(text) => {
                query.push_str(&text);
                *selected = 0;
            }
            EditorInput::Left => {
                move_sandbox_directory_picker_to_parent(cwd, items, query, selected);
            }
            EditorInput::Backspace if query.is_empty() => {
                move_sandbox_directory_picker_to_parent(cwd, items, query, selected);
            }
            EditorInput::Backspace | EditorInput::Delete => {
                query.pop();
                *selected = 0;
            }
            EditorInput::Up => *selected = selected.saturating_sub(1),
            EditorInput::Down => {
                *selected = (*selected + 1).min(
                    filtered_sandbox_directory_items(items, query)
                        .len()
                        .saturating_sub(1),
                );
            }
            EditorInput::PageUp => *selected = selected.saturating_sub(8),
            EditorInput::PageDown => {
                *selected = (*selected + 8).min(
                    filtered_sandbox_directory_items(items, query)
                        .len()
                        .saturating_sub(1),
                );
            }
            EditorInput::Home => *selected = 0,
            EditorInput::End => {
                *selected = filtered_sandbox_directory_items(items, query)
                    .len()
                    .saturating_sub(1);
            }
            EditorInput::Enter => {
                let chosen = selected_sandbox_directory_item(items, query, *selected).cloned();
                if let Some(chosen) = chosen {
                    match chosen.action {
                        SandboxDirectoryAction::Open(path) => {
                            if let Ok(next_items) = sandbox_directory_picker_items(&path) {
                                *cwd = path;
                                *items = next_items;
                                query.clear();
                                *selected = 0;
                            }
                        }
                        SandboxDirectoryAction::Choose(path) => {
                            let command = sandbox_directory_picker_command(*kind, *edit, &path);
                            state.overlay = None;
                            session.handle_command(&command, provider, false)?;
                        }
                    }
                }
            }
            EditorInput::Escape => state.overlay = None,
            EditorInput::CtrlC => {
                state.overlay = None;
                state.editor.arm_ctrl_c_exit();
            }
            EditorInput::CtrlD => return Ok(false),
            _ => {}
        },
        Some(TuiOverlay::SandboxRootEntries {
            selected,
            query,
            kind,
            items,
            selected_roots,
        }) => match input {
            EditorInput::Text(text) if text == " " => {
                if let Some(chosen) =
                    selected_sandbox_root_entry_item(items, query, *selected).cloned()
                {
                    *selected_roots =
                        toggle_sandbox_root_entry_selection(items, selected_roots, &chosen);
                }
            }
            EditorInput::Text(text) => {
                query.push_str(&text);
                *selected = 0;
            }
            EditorInput::Backspace | EditorInput::Delete => {
                query.pop();
                *selected = 0;
            }
            EditorInput::Up => *selected = selected.saturating_sub(1),
            EditorInput::Down => {
                *selected = (*selected + 1).min(
                    filtered_sandbox_root_entry_items(items, query)
                        .len()
                        .saturating_sub(1),
                );
            }
            EditorInput::PageUp => *selected = selected.saturating_sub(8),
            EditorInput::PageDown => {
                *selected = (*selected + 8).min(
                    filtered_sandbox_root_entry_items(items, query)
                        .len()
                        .saturating_sub(1),
                );
            }
            EditorInput::Home => *selected = 0,
            EditorInput::End => {
                *selected = filtered_sandbox_root_entry_items(items, query)
                    .len()
                    .saturating_sub(1);
            }
            EditorInput::Enter => {
                let chosen = selected_sandbox_root_entry_item(items, query, *selected).cloned();
                if let Some(chosen) = chosen {
                    let command = sandbox_root_entries_command(*kind, &chosen, selected_roots);
                    state.overlay = None;
                    session.handle_command(&command, provider, false)?;
                }
            }
            EditorInput::Escape => state.overlay = None,
            EditorInput::CtrlC => {
                state.overlay = None;
                state.editor.arm_ctrl_c_exit();
            }
            EditorInput::CtrlD => return Ok(false),
            _ => {}
        },
        None => state.overlay = None,
    }
    Ok(true)
}

fn handle_tui_editor_action(
    session: &mut ShellSession,
    provider: &mut ProviderConfig,
    state: &mut NativeTuiState,
    action: EditorAction,
) -> Result<bool, String> {
    match action {
        EditorAction::None => Ok(true),
        EditorAction::Cleared => {
            session.print_text("editor cleared", false)?;
            Ok(true)
        }
        EditorAction::OpenSettings => {
            state.overlay = Some(TuiOverlay::Settings {
                selected: 0,
                query: String::new(),
            });
            Ok(true)
        }
        EditorAction::OpenTree => {
            open_tree_entries_overlay(session, state)?;
            Ok(true)
        }
        EditorAction::OpenFork => {
            state.editor.replace_buffer("/fork ".to_string());
            session.print_text(
                "double escape: /fork ready; add a title or press Enter",
                false,
            )?;
            Ok(true)
        }
        EditorAction::EmptyEscape => handle_tui_empty_escape_action(session, provider, state),
        EditorAction::Exit => {
            if session.is_turn_running() {
                session.print_text(
                    "turn still running; press Escape/Ctrl+C to interrupt before Ctrl+D exit",
                    false,
                )?;
                Ok(true)
            } else {
                Ok(false)
            }
        }
        EditorAction::Submit(prompt) => {
            let prompt = prompt.trim();
            if prompt.is_empty() {
                return Ok(true);
            }
            if matches!(prompt, "/settings" | "/prefs" | "/preferences") {
                state.overlay = Some(TuiOverlay::Settings {
                    selected: 0,
                    query: String::new(),
                });
                return Ok(true);
            }
            if let Some(route) = tui_prompt_picker_route(prompt) {
                match route {
                    TuiPromptPickerRoute::TreeEntries => {
                        open_tree_entries_overlay(session, state)?;
                    }
                    TuiPromptPickerRoute::Sessions => {
                        open_sessions_overlay(session, state)?;
                    }
                }
                return Ok(true);
            }
            if matches!(prompt, "/model" | "/models" | "/roles" | "/role-model") {
                open_models_overlay(session, provider, state)?;
                return Ok(true);
            }
            if prompt == "/effort" {
                open_effort_overlay(provider, state);
                return Ok(true);
            }
            match prompt {
                "/theme" | "/themes" => {
                    open_settings_subpanel(session, provider, state, SettingsSubpanel::Theme);
                    return Ok(true);
                }
                "/permissions" | "/sandbox" => {
                    open_settings_subpanel(session, provider, state, SettingsSubpanel::Permissions);
                    return Ok(true);
                }
                "/provider" => {
                    open_settings_subpanel(session, provider, state, SettingsSubpanel::Provider);
                    return Ok(true);
                }
                "/login" => {
                    open_settings_subpanel(session, provider, state, SettingsSubpanel::Login);
                    return Ok(true);
                }
                "/memory" | "/mem" => {
                    open_settings_subpanel(session, provider, state, SettingsSubpanel::Memory);
                    return Ok(true);
                }
                _ => {}
            }
            if prompt == "/suggest-next accept" {
                if let Some(suggestion) = session.suggestion.take() {
                    state.editor.replace_buffer(suggestion.message);
                } else {
                    session.print_text("suggest-next: no active ghost suggestion", false)?;
                }
                return Ok(true);
            }
            if prompt.starts_with('/') {
                return session.handle_command(prompt, provider, false);
            }
            if session.handle_login_picker_input(prompt, provider, false)? {
                return Ok(true);
            }
            if session.is_turn_running() || session.has_pending_pause() {
                session.queue_follow_up(prompt, false)?;
            } else {
                session.start_turn_for_role(prompt, provider, false, Some("executor"))?;
            }
            Ok(true)
        }
        EditorAction::SubmitFollowUp(prompt) => {
            session.queue_follow_up(prompt.trim(), false)?;
            if !session.is_turn_running() && !session.has_pending_pause() {
                session.start_next_queued_or_goal_continuation(provider, false)?;
            }
            Ok(true)
        }
        EditorAction::Steer(prompt) => {
            session.steer_active_turn(prompt.trim(), false)?;
            Ok(true)
        }
        EditorAction::RestoreQueued => {
            if let Some(restored) = session.restore_latest_follow_up() {
                state.editor.replace_buffer(restored);
                session.print_text("restored queued message into editor", false)?;
            } else {
                session.print_text("no queued message to restore", false)?;
            }
            Ok(true)
        }
        EditorAction::Interrupt => {
            session.interrupt_active_turn(false)?;
            Ok(true)
        }
    }
}

fn handle_tui_empty_escape_action(
    session: &mut ShellSession,
    provider: &mut ProviderConfig,
    state: &mut NativeTuiState,
) -> Result<bool, String> {
    if session.is_turn_running() {
        session.interrupt_active_turn(false)?;
        return Ok(true);
    }
    let action = state
        .editor
        .resolve_empty_escape(session.double_escape_action, Instant::now());
    handle_tui_editor_action(session, provider, state, action)
}

fn is_alt_k_key(key: event::KeyEvent) -> bool {
    matches!(key.code, KeyCode::Char('k') | KeyCode::Char('K'))
        && key.modifiers.contains(KeyModifiers::ALT)
}

fn is_ctrl_c_key(key: event::KeyEvent) -> bool {
    matches!(key.code, KeyCode::Char('c') | KeyCode::Char('C'))
        && key.modifiers.contains(KeyModifiers::CONTROL)
}

fn editor_input_from_key(key: event::KeyEvent) -> Option<EditorInput> {
    let modifiers = key.modifiers;
    match key.code {
        KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => Some(EditorInput::CtrlC),
        KeyCode::Char('d') if modifiers.contains(KeyModifiers::CONTROL) => Some(EditorInput::CtrlD),
        KeyCode::Char('p') if modifiers.contains(KeyModifiers::CONTROL) => Some(EditorInput::CtrlP),
        KeyCode::Char('\t') => Some(EditorInput::Tab),
        KeyCode::Char(ch) => Some(EditorInput::Text(ch.to_string())),
        KeyCode::BackTab => Some(EditorInput::BackTab),
        KeyCode::Tab => Some(EditorInput::Tab),
        KeyCode::Enter if modifiers.contains(KeyModifiers::SHIFT) => Some(EditorInput::ShiftEnter),
        KeyCode::Enter if modifiers.contains(KeyModifiers::CONTROL) => Some(EditorInput::CtrlEnter),
        KeyCode::Enter if modifiers.contains(KeyModifiers::ALT) => Some(EditorInput::AltEnter),
        KeyCode::Enter => Some(EditorInput::Enter),
        KeyCode::Esc => Some(EditorInput::Escape),
        KeyCode::Backspace if modifiers.contains(KeyModifiers::ALT) => {
            Some(EditorInput::AltBackspace)
        }
        KeyCode::Backspace if modifiers.contains(KeyModifiers::CONTROL) => {
            Some(EditorInput::CtrlBackspace)
        }
        KeyCode::Backspace => Some(EditorInput::Backspace),
        KeyCode::Delete => Some(EditorInput::Delete),
        KeyCode::Left => Some(EditorInput::Left),
        KeyCode::Right => Some(EditorInput::Right),
        KeyCode::Home => Some(EditorInput::Home),
        KeyCode::End => Some(EditorInput::End),
        KeyCode::PageUp => Some(EditorInput::PageUp),
        KeyCode::PageDown => Some(EditorInput::PageDown),
        KeyCode::Up if modifiers.contains(KeyModifiers::ALT) => Some(EditorInput::AltUp),
        KeyCode::Up => Some(EditorInput::Up),
        KeyCode::Down => Some(EditorInput::Down),
        _ => None,
    }
}

fn activate_settings_item(
    session: &mut ShellSession,
    provider: &mut ProviderConfig,
    state: &mut NativeTuiState,
    selected: usize,
) -> Result<(), String> {
    let Some(item) = settings_root_items(session, provider)
        .get(selected)
        .cloned()
    else {
        return Ok(());
    };
    match item.action {
        SettingsRootAction::OpenPanel(panel) => {
            open_settings_subpanel(session, provider, state, panel)
        }
        SettingsRootAction::OpenMainModel => open_models_overlay(session, provider, state)?,
        SettingsRootAction::OpenEffort => open_effort_overlay(provider, state),
        SettingsRootAction::OpenSessions => open_sessions_overlay(session, state)?,
        SettingsRootAction::InsertCommand(command) => {
            state.overlay = None;
            state.editor.replace_buffer(command);
        }
    }
    session.sync_ui_docks();
    Ok(())
}

fn open_settings_subpanel(
    session: &mut ShellSession,
    provider: &ProviderConfig,
    state: &mut NativeTuiState,
    panel: SettingsSubpanel,
) {
    let back_stack = match &state.overlay {
        Some(TuiOverlay::SettingsPanel {
            panel: current,
            back_stack,
            ..
        }) => {
            let mut next = back_stack.clone();
            next.push(*current);
            next
        }
        _ => Vec::new(),
    };
    open_settings_subpanel_with_back_stack(session, provider, state, panel, back_stack);
}

fn open_settings_subpanel_with_back_stack(
    session: &mut ShellSession,
    provider: &ProviderConfig,
    state: &mut NativeTuiState,
    panel: SettingsSubpanel,
    back_stack: Vec<SettingsSubpanel>,
) {
    if matches!(panel, SettingsSubpanel::ScopedModels) {
        let _ = session.refresh_model_registry_cache(provider, false);
    }
    if matches!(panel, SettingsSubpanel::Theme) {
        session.refresh_theme_settings_cache();
    }
    let items = settings_panel_items(session, provider, panel);
    let selected = items
        .iter()
        .position(|item| item.current)
        .unwrap_or_default();
    state.overlay = Some(TuiOverlay::SettingsPanel {
        panel,
        selected,
        items,
        back_stack,
    });
}

fn settings_previous_overlay_for_stack(back_stack: &[SettingsSubpanel]) -> TuiOverlay {
    if let Some((panel, remaining)) = back_stack.split_last() {
        TuiOverlay::SettingsPanel {
            panel: *panel,
            selected: 0,
            items: Vec::new(),
            back_stack: remaining.to_vec(),
        }
    } else {
        TuiOverlay::Settings {
            selected: 0,
            query: String::new(),
        }
    }
}

fn return_to_previous_settings_screen(
    session: &mut ShellSession,
    provider: &ProviderConfig,
    state: &mut NativeTuiState,
    back_stack: &[SettingsSubpanel],
) {
    match settings_previous_overlay_for_stack(back_stack) {
        TuiOverlay::SettingsPanel {
            panel, back_stack, ..
        } => open_settings_subpanel_with_back_stack(session, provider, state, panel, back_stack),
        TuiOverlay::Settings { selected, query } => {
            state.overlay = Some(TuiOverlay::Settings { selected, query });
        }
        _ => unreachable!("settings stack only returns settings overlays"),
    }
}

fn settings_panel_items(
    session: &ShellSession,
    provider: &ProviderConfig,
    panel: SettingsSubpanel,
) -> Vec<SettingsPanelItem> {
    match panel {
        SettingsSubpanel::General => general_settings_panel_items(session),
        SettingsSubpanel::Footer => {
            footer_settings_panel_items_for_settings(&session.footer_settings)
        }
        SettingsSubpanel::Images => {
            image_settings_panel_items_for_settings(&session.image_settings)
        }
        SettingsSubpanel::Compaction => {
            compaction_settings_panel_items_for_settings(&session.compact_settings)
        }
        SettingsSubpanel::Theme => theme_settings_panel_items(session),
        SettingsSubpanel::Permissions => permission_settings_panel_items(session),
        SettingsSubpanel::Provider => provider_settings_panel_items(provider),
        SettingsSubpanel::Login => login_settings_panel_items(provider),
        SettingsSubpanel::LoginSubscription => login_subscription_settings_panel_items(),
        SettingsSubpanel::LoginApi => login_api_settings_panel_items(provider),
        SettingsSubpanel::LoginClaude => login_claude_settings_panel_items(),
        SettingsSubpanel::Memory => {
            memory_settings_panel_items_for_settings(&session.memory_settings)
        }
        SettingsSubpanel::RoleModels => role_model_settings_panel_items(session, provider),
        SettingsSubpanel::ScopedModels => scoped_model_settings_panel_items(session, provider),
    }
}

fn general_settings_panel_items(session: &ShellSession) -> Vec<SettingsPanelItem> {
    vec![
        settings_command_item(
            "Usage/status",
            "/usage",
            "Show native local provider/model/thread/todo status",
            true,
            "/usage",
        ),
        settings_command_item(
            "Keybindings",
            "/keys",
            "Show terminal capability and degraded-keybinding notes",
            false,
            "/keys",
        ),
        settings_command_item(
            "Debug bundle",
            "/debug",
            "Print redacted native debug state",
            false,
            "/debug",
        ),
        settings_command_item(
            "Suggestion",
            "/suggest-next",
            "Show or clear the current ghost next-message suggestion",
            false,
            "/suggest-next show",
        ),
        settings_edit_item(
            "Question timeout",
            &format_ask_user_timeout_label(session.ask_user_settings.timeout_ms),
            "ask_user waits this long before recommended/default answers",
            "/ask-user timeout ",
        ),
    ]
}

fn footer_settings_panel_items_for_settings(settings: &FooterSettings) -> Vec<SettingsPanelItem> {
    vec![
        settings_command_item(
            "Workspace path",
            footer_visibility_label(settings.show_path),
            "Show current project path in the footer area",
            settings.show_path,
            format!("/footer path {}", footer_toggle_arg(settings.show_path)),
        ),
        settings_command_item(
            "Hotkey help",
            footer_visibility_label(settings.show_help_bar),
            "Show the Alt+K help row below the footer",
            settings.show_help_bar,
            format!("/footer help {}", footer_toggle_arg(settings.show_help_bar)),
        ),
        settings_edit_item(
            "Usage display",
            settings.usage_display.label(),
            "Session and weekly usage chip visibility",
            "/footer usage ",
        ),
        settings_command_item(
            "Model chip",
            footer_visibility_label(settings.show_model),
            "Show active provider/model in footer",
            settings.show_model,
            format!("/footer model {}", footer_toggle_arg(settings.show_model)),
        ),
        settings_command_item(
            "Permission chip",
            footer_visibility_label(settings.show_permission),
            "Show active permission mode in footer",
            settings.show_permission,
            format!(
                "/footer permission {}",
                footer_toggle_arg(settings.show_permission)
            ),
        ),
        settings_command_item(
            "Memory chip",
            footer_visibility_label(settings.show_memory),
            "Show memory status in footer",
            settings.show_memory,
            format!("/footer memory {}", footer_toggle_arg(settings.show_memory)),
        ),
        settings_command_item(
            "Context chip",
            footer_visibility_label(settings.show_context),
            "Show context usage in footer",
            settings.show_context,
            format!(
                "/footer context {}",
                footer_toggle_arg(settings.show_context)
            ),
        ),
    ]
}

fn image_settings_panel_items_for_settings(settings: &ImageSettings) -> Vec<SettingsPanelItem> {
    vec![
        settings_command_item(
            "Show images",
            image_visibility_label(settings.show_images),
            "Render supported images inline in capable terminals",
            settings.show_images,
            format!(
                "/images show {}",
                image_show_toggle_arg(settings.show_images)
            ),
        ),
        settings_edit_item(
            "Image width",
            &settings.image_width_cells.to_string(),
            "Preferred inline image width in terminal cells",
            "/images width ",
        ),
        settings_command_item(
            "Auto-resize images",
            enabled_label(settings.auto_resize_images),
            "Resize large images before sending them to model providers",
            settings.auto_resize_images,
            format!(
                "/images auto-resize {}",
                image_enabled_toggle_arg(settings.auto_resize_images)
            ),
        ),
        settings_command_item(
            "Block images",
            image_block_label(settings.block_images),
            "Prevent images from being sent to model providers",
            settings.block_images,
            format!(
                "/images block {}",
                image_block_toggle_arg(settings.block_images)
            ),
        ),
        settings_edit_item(
            "Preview protocol",
            settings.preview_protocol.as_str(),
            "Override terminal inline image protocol detection",
            "/images protocol ",
        ),
        settings_command_item(
            "Preview capability",
            "detect",
            "Show terminal image preview support",
            false,
            "/images capabilities",
        ),
    ]
}

fn compaction_settings_panel_items_for_settings(
    settings: &OppiCompactSettings,
) -> Vec<SettingsPanelItem> {
    vec![
        settings_command_item(
            "Idle compaction",
            compaction_enabled_label(settings.idle_enabled),
            "Compact after idle time and context threshold",
            settings.idle_enabled,
            format!(
                "/idle-compact {}",
                compaction_toggle_arg(settings.idle_enabled)
            ),
        ),
        settings_edit_item(
            "Idle time",
            &format_compaction_minutes_label(settings.idle_minutes),
            "Minutes idle before automatic compaction",
            "/idle-compact idle-time ",
        ),
        settings_edit_item(
            "Idle context threshold",
            &format_compaction_threshold_label(settings.idle_threshold_percent),
            "Context usage required for idle compaction",
            "/idle-compact idle-threshold ",
        ),
        settings_edit_item(
            "Smart compact threshold",
            &format_compaction_threshold_label(settings.smart_threshold_percent),
            "Todo checkpoint context threshold",
            "/idle-compact smart-threshold ",
        ),
        settings_command_item(
            "Memory status",
            "client",
            "Show Hoppi/native memory status before compacting",
            true,
            "/memory status",
        ),
        settings_command_item(
            "Compact now",
            "handoff",
            "Create a todo-aware memory handoff now",
            false,
            "/idle-compact compact",
        ),
        settings_command_item(
            "Maintenance dry-run",
            "preview",
            "Preview explicit memory maintenance where supported",
            false,
            "/memory maintenance dry-run",
        ),
        settings_command_item(
            "Maintenance apply",
            "manual",
            "Run explicit memory maintenance where supported",
            false,
            "/memory maintenance apply",
        ),
    ]
}

fn theme_settings_panel_items(session: &ShellSession) -> Vec<SettingsPanelItem> {
    theme_settings_panel_items_for_list(&session.theme, session.theme_list_cache.as_ref())
}

fn theme_settings_panel_items_for(current_theme: &str) -> Vec<SettingsPanelItem> {
    theme_settings_panel_items_for_list(current_theme, None)
}

fn theme_settings_panel_items_for_list(
    current_theme: &str,
    list: Option<&Value>,
) -> Vec<SettingsPanelItem> {
    let mut items = [
        ("OPPi", "oppi", "Default OPPi accent theme"),
        ("Dark", "dark", "Low-contrast dark terminal theme"),
        ("Light", "light", "Light terminal theme"),
        ("Plain", "plain", "No-color terminal-safe theme"),
        ("Reload file", "reload", "Reload .oppi/theme.txt if present"),
    ]
    .into_iter()
    .map(|(label, theme, detail)| {
        settings_command_item(
            label,
            theme,
            detail,
            current_theme == theme,
            format!("/theme {theme}"),
        )
    })
    .collect::<Vec<_>>();
    if let Some(list) = list {
        for entry in active_theme_entries_from_theme_list(list) {
            if normalize_theme_name(&entry.name).is_some() {
                continue;
            }
            let detail = match entry.path.as_deref() {
                Some(path) => format!("Discovered {} theme · {path}", entry.source),
                None => format!("Discovered {} theme", entry.source),
            };
            items.push(settings_command_item(
                entry.name.clone(),
                entry.source,
                detail,
                current_theme == entry.name,
                format!("/theme {}", entry.name),
            ));
        }
    }
    items.push(settings_command_item(
        "Preview picker",
        "open",
        "Preview available themes before applying one",
        false,
        "/theme preview",
    ));
    items
}

fn theme_preview_name_for_settings_item(item: &SettingsPanelItem) -> Option<String> {
    let SettingsPanelAction::Command(command) = &item.action else {
        return None;
    };
    let name = command.strip_prefix("/theme ")?.trim();
    if name.is_empty() || matches!(name, "preview" | "reload") {
        return None;
    }
    normalize_theme_name(name).or_else(|| Some(name.to_string()))
}

fn settings_panel_numeric_preview_target(
    panel: SettingsSubpanel,
    items: &[SettingsPanelItem],
    text: &str,
) -> Option<usize> {
    if !matches!(panel, SettingsSubpanel::Theme) || text.len() != 1 {
        return None;
    }
    let digit = text.as_bytes().first().copied()?;
    if !(b'1'..=b'9').contains(&digit) {
        return None;
    }
    let target = usize::from(digit - b'1');
    items
        .get(target)
        .and_then(theme_preview_name_for_settings_item)
        .map(|_| target)
}

fn settings_panel_item_value_for_render(
    panel: SettingsSubpanel,
    item: &SettingsPanelItem,
    row: usize,
    selected: usize,
) -> String {
    if matches!(panel, SettingsSubpanel::Theme) {
        if item.current {
            return "current".to_string();
        }
        if row == selected && theme_preview_name_for_settings_item(item).is_some() {
            return "preview".to_string();
        }
    }
    item.value.clone()
}

fn settings_panel_item_detail_for_render(
    panel: SettingsSubpanel,
    item: &SettingsPanelItem,
    row: usize,
    selected: usize,
) -> String {
    if matches!(panel, SettingsSubpanel::Theme)
        && row == selected
        && theme_preview_name_for_settings_item(item).is_some()
    {
        return format!(
            "{} · Live preview; Enter/Space applies; Esc restores saved theme",
            item.detail
        );
    }
    item.detail.clone()
}

fn settings_panel_footer_hint(panel: SettingsSubpanel, range: &str, count: usize) -> String {
    if matches!(panel, SettingsSubpanel::Theme) {
        format!("{range} of {count} · Up/Down/1-9 preview live · Enter/Space apply · Esc restore")
    } else {
        format!("{range} of {count} · Enter/Space to change · Esc back")
    }
}

fn theme_name_for_tui_frame(current_theme: &str, state: &NativeTuiState) -> String {
    state
        .theme_preview_name()
        .unwrap_or_else(|| current_theme.to_string())
}

fn theme_tokens_for_tui_frame(
    current_theme: &str,
    loaded_theme: Option<&LoadedShellTheme>,
    state: &NativeTuiState,
    preview_cache: &BTreeMap<String, LoadedShellTheme>,
) -> ThemeTokens {
    if let Some(preview_theme) = state.theme_preview_name() {
        if let Some(loaded) = preview_cache.get(&preview_theme) {
            return loaded.line_tokens;
        }
        return theme_tokens(&preview_theme);
    }
    loaded_theme
        .map(|theme| theme.line_tokens)
        .unwrap_or_else(|| theme_tokens(current_theme))
}

fn permission_settings_panel_items(session: &ShellSession) -> Vec<SettingsPanelItem> {
    permission_settings_panel_items_for_settings(
        session.permission_mode.as_str(),
        &session.permission_review_settings,
        &session.permission_sandbox_settings,
    )
}

fn permission_settings_panel_items_for_settings(
    current_mode: &str,
    review_settings: &GuardianReviewSettings,
    sandbox_settings: &PermissionSandboxSettings,
) -> Vec<SettingsPanelItem> {
    let mut items = [
        (
            "Read only",
            "read-only",
            "Allow reads and block write/network actions",
        ),
        ("Default", "default", "Balanced local development policy"),
        (
            "Auto review",
            "auto-review",
            "Review risky actions before execution",
        ),
        (
            "Full access",
            "full-access",
            "Allow unrestricted local tool actions",
        ),
    ]
    .into_iter()
    .map(|(label, mode, detail)| {
        settings_command_item(
            label,
            mode,
            detail,
            current_mode == mode,
            format!("/permissions {mode}"),
        )
    })
    .collect::<Vec<_>>();
    items.push(settings_edit_item(
        "Review timeout",
        &format_review_timeout_label(review_settings.timeout_ms),
        "Guardian reviewer timeout before fail-closed",
        "/permissions timeout ",
    ));
    items.push(settings_edit_item(
        "Reviewer model",
        review_settings.reviewer_model.as_deref().unwrap_or("auto"),
        "Non-Meridian reviewer model or auto",
        "/permissions reviewer-model ",
    ));
    items.push(settings_command_item(
        "Validate sandbox",
        "check",
        "Preview effective roots and path issues",
        false,
        "/permissions sandbox validate",
    ));
    items.push(settings_command_item(
        "Use workspace roots",
        "current",
        "Set project/read/write roots to the current workspace",
        false,
        "/permissions sandbox roots here",
    ));
    items.push(settings_command_item(
        "Reset sandbox roots",
        "default",
        "Restore project/read/write root fallback behavior",
        false,
        "/permissions sandbox roots default",
    ));
    items.push(settings_sandbox_directory_picker_item(
        "Browse project root",
        "Choose a project root from directories on disk",
        SandboxRootKind::Project,
        SandboxRootEditMode::Set,
    ));
    items.push(settings_sandbox_directory_picker_item(
        "Add project root",
        "Append a project root from directories on disk",
        SandboxRootKind::Project,
        SandboxRootEditMode::Add,
    ));
    items.push(settings_sandbox_root_remove_picker_item(
        "Remove project root",
        "Remove a configured project root",
        SandboxRootKind::Project,
    ));
    items.push(settings_sandbox_directory_picker_item(
        "Browse readable root",
        "Choose a readable root from directories on disk",
        SandboxRootKind::Read,
        SandboxRootEditMode::Set,
    ));
    items.push(settings_sandbox_directory_picker_item(
        "Add readable root",
        "Append a readable root from directories on disk",
        SandboxRootKind::Read,
        SandboxRootEditMode::Add,
    ));
    items.push(settings_sandbox_root_remove_picker_item(
        "Remove readable root",
        "Remove a configured readable root",
        SandboxRootKind::Read,
    ));
    items.push(settings_sandbox_directory_picker_item(
        "Browse writable root",
        "Choose a writable root from directories on disk",
        SandboxRootKind::Write,
        SandboxRootEditMode::Set,
    ));
    items.push(settings_sandbox_directory_picker_item(
        "Add writable root",
        "Append a writable root from directories on disk",
        SandboxRootKind::Write,
        SandboxRootEditMode::Add,
    ));
    items.push(settings_sandbox_root_remove_picker_item(
        "Remove writable root",
        "Remove a configured writable root",
        SandboxRootKind::Write,
    ));
    items.push(settings_command_item(
        "Sandbox internet",
        sandbox_flag_label(sandbox_settings.allow_internet),
        "Network access for sandboxed tools",
        sandbox_settings.allow_internet,
        format!(
            "/permissions sandbox internet {}",
            sandbox_toggle_arg(sandbox_settings.allow_internet)
        ),
    ));
    items.push(settings_command_item(
        "Sandbox SSH",
        sandbox_flag_label(sandbox_settings.allow_ssh),
        "SSH/network access for sandboxed tools",
        sandbox_settings.allow_ssh,
        format!(
            "/permissions sandbox ssh {}",
            sandbox_toggle_arg(sandbox_settings.allow_ssh)
        ),
    ));
    items.push(settings_edit_item(
        "Project roots",
        &format_sandbox_roots_label(&sandbox_settings.project_roots, "default"),
        "Default root set for sandbox read/write",
        format_sandbox_edit_command(
            "/permissions sandbox project ",
            &sandbox_settings.project_roots,
        ),
    ));
    items.push(settings_edit_item(
        "Readable roots",
        &format_sandbox_roots_label(&sandbox_settings.readable_roots, "project roots"),
        "Sandbox paths readable by tools",
        format_sandbox_edit_command(
            "/permissions sandbox read ",
            &sandbox_settings.readable_roots,
        ),
    ));
    items.push(settings_edit_item(
        "Writable roots",
        &format_sandbox_roots_label(&sandbox_settings.writable_roots, "project roots"),
        "Sandbox paths writable by tools",
        format_sandbox_edit_command(
            "/permissions sandbox write ",
            &sandbox_settings.writable_roots,
        ),
    ));
    items.push(settings_edit_item(
        "Protected patterns",
        &format_protected_patterns_label(&sandbox_settings.protected_patterns),
        "Extra secret/path patterns blocked by sandbox policy",
        format_sandbox_edit_command(
            "/permissions sandbox protected ",
            &sandbox_settings.protected_patterns,
        ),
    ));
    items
}

fn settings_sandbox_directory_picker_item(
    label: impl Into<String>,
    detail: impl Into<String>,
    kind: SandboxRootKind,
    edit: SandboxRootEditMode,
) -> SettingsPanelItem {
    let value = match edit {
        SandboxRootEditMode::Set => "picker",
        SandboxRootEditMode::Add => "add",
    };
    SettingsPanelItem {
        label: label.into(),
        value: value.to_string(),
        detail: detail.into(),
        current: false,
        action: SettingsPanelAction::OpenSandboxDirectoryPicker { kind, edit },
    }
}

fn settings_sandbox_root_remove_picker_item(
    label: impl Into<String>,
    detail: impl Into<String>,
    kind: SandboxRootKind,
) -> SettingsPanelItem {
    SettingsPanelItem {
        label: label.into(),
        value: "remove".to_string(),
        detail: detail.into(),
        current: false,
        action: SettingsPanelAction::OpenSandboxRootRemovePicker(kind),
    }
}

fn sandbox_flag_label(allowed: bool) -> &'static str {
    if allowed { "allowed" } else { "blocked" }
}

fn sandbox_toggle_arg(allowed: bool) -> &'static str {
    if allowed { "off" } else { "on" }
}

fn format_sandbox_roots_label(roots: &[String], fallback: &str) -> String {
    let roots = normalize_string_slice(roots);
    if roots.is_empty() {
        fallback.to_string()
    } else {
        roots.join(", ")
    }
}

fn format_sandbox_edit_command(prefix: &str, values: &[String]) -> String {
    let values = normalize_string_slice(values);
    if values.is_empty() {
        format!("{prefix}default")
    } else {
        format!("{prefix}{}", values.join(", "))
    }
}

fn format_review_timeout_label(timeout_ms: u64) -> String {
    if timeout_ms > 0 && timeout_ms % 1_000 == 0 {
        format!("{}s", timeout_ms / 1_000)
    } else {
        format!("{timeout_ms}ms")
    }
}

fn provider_settings_panel_items(provider: &ProviderConfig) -> Vec<SettingsPanelItem> {
    let openai_env = match provider {
        ProviderConfig::OpenAiCompatible(config) => config.api_key_env.as_deref(),
        ProviderConfig::Mock => None,
    };
    let mut items = vec![
        settings_command_item(
            "Status",
            provider.label(),
            "Show redacted provider/model/auth status",
            true,
            "/provider status",
        ),
        settings_command_item(
            "Validate",
            "local",
            "Run redacted local readiness checks (no model call)",
            false,
            "/provider validate",
        ),
        settings_command_item(
            "Policy",
            "safety",
            "Explain provider auth/network policy",
            false,
            "/provider policy",
        ),
        settings_command_item(
            "Anthropic bridge",
            "Meridian",
            "Show explicit Claude/Meridian compatibility notes",
            false,
            "/provider anthropic",
        ),
    ];
    for env_name in ["OPPI_OPENAI_API_KEY", "OPENAI_API_KEY"] {
        items.push(settings_command_item(
            "Use OpenAI env",
            env_name,
            "Configure by env-reference only; raw keys are rejected",
            openai_env == Some(env_name),
            format!("/login api openai env {env_name}"),
        ));
    }
    items.push(settings_edit_item(
        "Custom API env",
        "edit",
        "Insert /login api openai env <ENV> into the editor",
        "/login api openai env ",
    ));
    items.push(settings_edit_item(
        "Base URL",
        "edit",
        "Insert /provider base-url <url> into the editor",
        "/provider base-url ",
    ));
    items.push(settings_edit_item(
        "Smoke",
        "live",
        "Insert live provider smoke command for explicit review",
        "/provider smoke ",
    ));
    items.push(settings_command_item(
        "Logout provider",
        "session",
        "Clear direct-provider settings for this shell session",
        false,
        "/logout provider",
    ));
    items
}

fn login_settings_panel_items(provider: &ProviderConfig) -> Vec<SettingsPanelItem> {
    vec![
        settings_panel_link_item(
            "Subscription auth",
            "providers",
            "Codex, Claude/Meridian, or Copilot guidance",
            SettingsSubpanel::LoginSubscription,
            true,
        ),
        settings_panel_link_item(
            "API auth",
            "env-ref",
            "OpenAI-compatible setup with environment references only",
            SettingsSubpanel::LoginApi,
            false,
        ),
        settings_command_item(
            "Status",
            provider.label(),
            "Show the current login/provider status",
            false,
            "/provider status",
        ),
        settings_command_item(
            "Logout provider",
            "session",
            "Clear native direct-provider settings for this session",
            false,
            "/logout provider",
        ),
    ]
}

fn login_subscription_settings_panel_items() -> Vec<SettingsPanelItem> {
    vec![
        settings_command_item(
            "Codex (ChatGPT)",
            "native OAuth",
            "Open browser sign-in and save redacted auth-store token",
            false,
            "/login subscription codex",
        ),
        settings_panel_link_item(
            "Claude (Anthropic)",
            "Meridian",
            "Explicit managed loopback bridge; no hidden install/start",
            SettingsSubpanel::LoginClaude,
            true,
        ),
        settings_command_item(
            "Copilot (Microsoft)",
            "device OAuth",
            "Pi-compatible GitHub device-code sign-in; saves redacted auth-store token",
            false,
            "/login subscription copilot",
        ),
    ]
}

fn login_api_settings_panel_items(provider: &ProviderConfig) -> Vec<SettingsPanelItem> {
    let openai_env = match provider {
        ProviderConfig::OpenAiCompatible(config) => config.api_key_env.as_deref(),
        ProviderConfig::Mock => None,
    };
    vec![
        settings_command_item(
            "OpenAI guidance",
            "env-ref",
            "Show safe API-key environment-variable setup",
            false,
            "/login api openai",
        ),
        settings_command_item(
            "Use OPPI_OPENAI_API_KEY",
            "env-ref",
            "Configure without storing a raw secret",
            openai_env == Some("OPPI_OPENAI_API_KEY"),
            "/login api openai env OPPI_OPENAI_API_KEY",
        ),
        settings_command_item(
            "Use OPENAI_API_KEY",
            "env-ref",
            "Configure without storing a raw secret",
            openai_env == Some("OPENAI_API_KEY"),
            "/login api openai env OPENAI_API_KEY",
        ),
        settings_edit_item(
            "Custom API env",
            "edit",
            "Insert /login api openai env <ENV> into the editor",
            "/login api openai env ",
        ),
    ]
}

fn login_claude_settings_panel_items() -> Vec<SettingsPanelItem> {
    vec![
        settings_command_item(
            "Status",
            "Meridian",
            "Check explicit managed bridge status",
            true,
            "/login subscription claude status",
        ),
        settings_command_item(
            "Claude Code login",
            "explicit",
            "Run/guide `claude login`; OPPi never extracts Claude tokens",
            false,
            "/login subscription claude login",
        ),
        settings_command_item(
            "Use bridge",
            "loopback",
            "Configure explicit bridge if already running",
            false,
            "/login subscription claude use",
        ),
        settings_command_item(
            "Start bridge",
            "start",
            "Try visible user-selected Meridian startup candidates",
            false,
            "/login subscription claude start",
        ),
        settings_command_item(
            "Install bridge",
            "approval",
            "Ask before installing managed Meridian dependency",
            false,
            "/login subscription claude install",
        ),
        settings_command_item(
            "Stop bridge",
            "stop",
            "Stop only the bridge started by this shell session",
            false,
            "/login subscription claude stop",
        ),
    ]
}

fn scoped_model_settings_panel_items(
    session: &ShellSession,
    provider: &ProviderConfig,
) -> Vec<SettingsPanelItem> {
    let mut items = vec![settings_command_item(
        "List scope",
        if session.scoped_model_ids.is_empty() {
            "all"
        } else {
            "scoped"
        },
        "Show enabled model patterns and resolved current-provider scope",
        true,
        "/scoped-models list",
    )];
    if let Some(current) = session.session_model(provider) {
        items.push(settings_command_item(
            "Enable current",
            current,
            "Add the current main model to the model cycling scope",
            session.scoped_model_ids.iter().any(|item| item == current),
            format!("/scoped-models enable {current}"),
        ));
    }
    for model in session
        .cached_model_ids_for_provider(provider, false)
        .unwrap_or_default()
        .into_iter()
        .take(12)
    {
        items.push(settings_command_item(
            "Enable model",
            model.clone(),
            "Add this model to scoped /model and Ctrl+P cycling",
            session
                .scoped_model_ids
                .iter()
                .any(|item| item == &model || model_matches_scope_pattern(provider, &model, item)),
            format!("/scoped-models enable {model}"),
        ));
    }
    items.push(settings_edit_item(
        "Enable pattern",
        "edit",
        "Insert /scoped-models enable <model|provider/model|glob>",
        "/scoped-models enable ",
    ));
    items.push(settings_edit_item(
        "Disable pattern",
        "edit",
        "Insert /scoped-models disable <pattern>",
        "/scoped-models disable ",
    ));
    items.push(settings_command_item(
        "Clear scope",
        "all models",
        "Use all current provider models again",
        session.scoped_model_ids.is_empty(),
        "/scoped-models clear",
    ));
    items
}

fn role_model_settings_panel_items(
    session: &ShellSession,
    provider: &ProviderConfig,
) -> Vec<SettingsPanelItem> {
    let mut model_ids = session.known_model_ids.clone();
    if let Some(model) = session.session_model(provider) {
        model_ids.insert(model.to_string());
    }
    for model in session.role_models.values() {
        model_ids.insert(model.clone());
    }
    if model_ids.is_empty() {
        model_ids.insert(MOCK_MODEL_ID.to_string());
    }
    let inherited = session
        .session_model(provider)
        .map(display_model_id)
        .unwrap_or("main model")
        .to_string();
    let mut items = Vec::new();
    for role in ROLE_NAMES {
        items.push(SettingsPanelItem {
            label: format!("{role} inherit"),
            value: inherited.clone(),
            detail: "Use the main OPPi model for this task role".to_string(),
            current: !session.role_models.contains_key(role),
            action: SettingsPanelAction::Model(ModelPickerAction::ClearRole {
                role: role.to_string(),
            }),
        });
        for model in &model_ids {
            let model_label = display_model_id(model).to_string();
            items.push(SettingsPanelItem {
                label: format!("{role} model"),
                value: model_label.clone(),
                detail: format!("Use {model_label} for {role} turns"),
                current: session
                    .role_models
                    .get(role)
                    .is_some_and(|current| current == model),
                action: SettingsPanelAction::Model(ModelPickerAction::SelectRole {
                    role: role.to_string(),
                    model: model.clone(),
                }),
            });
        }
    }
    items
}

fn memory_settings_panel_items_for_settings(settings: &MemorySettings) -> Vec<SettingsPanelItem> {
    vec![
        settings_command_item(
            "Status",
            enabled_label(settings.enabled),
            "Show Hoppi/native memory status",
            false,
            "/memory status",
        ),
        settings_command_item(
            "Memory",
            enabled_label(settings.enabled),
            "Enable runtime-backed project memory recall/write behavior",
            false,
            format!("/memory {}", memory_setting_toggle_arg(settings.enabled)),
        ),
        settings_command_item(
            "Backend package",
            HOPPI_PACKAGE_NAME,
            "Install/update Hoppi from npm after explicit approval",
            false,
            "/memory install-hoppi",
        ),
        settings_edit_item(
            "Memory agent model",
            settings.agent_model.clone(),
            "auto | claude | gpt for Hoppi turn-memory distillation",
            "/memory agent-model ",
        ),
        settings_edit_item(
            "Deep dream model",
            settings.deep_model.clone(),
            "auto | sonnet | gpt-5.5 for deep memory reconciliation",
            "/memory deep-model ",
        ),
        settings_command_item(
            "Startup recall",
            enabled_label(settings.startup_recall),
            "Inject bounded Hoppi context when a native session starts",
            false,
            format!(
                "/memory startup-recall {}",
                memory_setting_toggle_arg(settings.startup_recall)
            ),
        ),
        settings_command_item(
            "Task-start recall",
            enabled_label(settings.task_start_recall),
            "Run high-precision recall at new task boundaries",
            false,
            format!(
                "/memory task-start-recall {}",
                memory_setting_toggle_arg(settings.task_start_recall)
            ),
        ),
        settings_command_item(
            "Turn summaries",
            enabled_label(settings.turn_summaries),
            "Save bounded Hoppi turn summaries after completed turns",
            false,
            format!(
                "/memory turn-summaries {}",
                memory_setting_toggle_arg(settings.turn_summaries)
            ),
        ),
        settings_command_item(
            "Idle dream mode",
            enabled_label(settings.idle_consolidation),
            "Consolidate dirty Hoppi memory during idle windows",
            false,
            format!(
                "/memory idle-dream {}",
                memory_setting_toggle_arg(settings.idle_consolidation)
            ),
        ),
        settings_command_item(
            "First-start install offer",
            settings.hoppi_install_offer.clone(),
            "Ask before installing Hoppi when the backend package is missing",
            false,
            format!(
                "/memory install-offer {}",
                hoppi_install_offer_toggle_arg(&settings.hoppi_install_offer)
            ),
        ),
        settings_command_item(
            "Sync",
            enabled_label(settings.sync_enabled),
            "Enable opt-in Hoppi sync",
            false,
            format!(
                "/memory sync {}",
                memory_setting_toggle_arg(settings.sync_enabled)
            ),
        ),
        settings_command_item(
            "Sync now",
            "manual",
            "Run Hoppi pull + push sync now",
            false,
            "/memory sync now",
        ),
        settings_command_item(
            "Pull now",
            "manual",
            "Pull shared Hoppi memory now",
            false,
            "/memory sync pull",
        ),
        settings_command_item(
            "Push now",
            "manual",
            "Push shared Hoppi memory now",
            false,
            "/memory sync push",
        ),
        settings_edit_item(
            "Sync repo",
            optional_memory_setting_label(settings.sync_repo_path.as_deref()),
            "Local Hoppi sync repository path",
            "/memory sync repo ",
        ),
        settings_edit_item(
            "Sync remote",
            optional_memory_setting_label(settings.sync_repo_url.as_deref()),
            "Git remote URL for shared Hoppi memory",
            "/memory sync remote ",
        ),
        settings_command_item(
            "Sync startup pull",
            enabled_label(settings.sync_pull_on_startup),
            "Pull shared memory when native OPPi starts",
            false,
            format!(
                "/memory sync pull-startup {}",
                memory_setting_toggle_arg(settings.sync_pull_on_startup)
            ),
        ),
        settings_command_item(
            "Sync exit push",
            enabled_label(settings.sync_push_on_exit),
            "Push shared memory when native OPPi exits",
            false,
            format!(
                "/memory sync push-exit {}",
                memory_setting_toggle_arg(settings.sync_push_on_exit)
            ),
        ),
        settings_edit_item(
            "Sync background",
            settings.sync_background.clone(),
            "off | 15m | 30m | 60m",
            "/memory sync background ",
        ),
        settings_edit_item(
            "Sync encryption",
            format!(
                "{}/{}",
                settings.sync_encryption, settings.sync_passphrase_source
            ),
            "none | env | file",
            "/memory sync encryption ",
        ),
        settings_edit_item(
            "Sync passphrase env",
            settings.sync_passphrase_env.clone(),
            "Environment variable for sync passphrase",
            "/memory sync passphrase-env ",
        ),
        settings_edit_item(
            "Sync passphrase file",
            optional_memory_setting_label(settings.sync_passphrase_file.as_deref()),
            "File path for sync passphrase",
            "/memory sync passphrase-file ",
        ),
        settings_edit_item(
            "Sync conflict mode",
            settings.sync_conflict_mode.clone(),
            "keep-both | keep-local | overwrite-local",
            "/memory sync conflict ",
        ),
        settings_command_item(
            "Dashboard",
            "view",
            "Open runtime-backed memory dashboard panel",
            false,
            "/memory dashboard",
        ),
        settings_command_item(
            "Settings",
            "view",
            "Open runtime-backed memory settings panel",
            false,
            "/memory settings",
        ),
        settings_command_item(
            "Maintenance preview",
            "dry-run",
            "Preview memory maintenance without applying changes",
            false,
            "/memory maintenance dry-run",
        ),
        settings_command_item(
            "Maintenance apply",
            "explicit",
            "Apply memory maintenance only after this explicit selection",
            false,
            "/memory maintenance apply",
        ),
        settings_command_item(
            "Record compaction",
            "handoff",
            "Create a todo-aware memory handoff now",
            false,
            "/idle-compact compact",
        ),
    ]
}

fn settings_command_item(
    label: impl Into<String>,
    value: impl Into<String>,
    detail: impl Into<String>,
    current: bool,
    command: impl Into<String>,
) -> SettingsPanelItem {
    SettingsPanelItem {
        label: label.into(),
        value: value.into(),
        detail: detail.into(),
        current,
        action: SettingsPanelAction::Command(command.into()),
    }
}

fn settings_edit_item(
    label: impl Into<String>,
    value: impl Into<String>,
    detail: impl Into<String>,
    command: impl Into<String>,
) -> SettingsPanelItem {
    SettingsPanelItem {
        label: label.into(),
        value: value.into(),
        detail: detail.into(),
        current: false,
        action: SettingsPanelAction::EditCommand(command.into()),
    }
}

fn settings_panel_link_item(
    label: impl Into<String>,
    value: impl Into<String>,
    detail: impl Into<String>,
    panel: SettingsSubpanel,
    current: bool,
) -> SettingsPanelItem {
    SettingsPanelItem {
        label: label.into(),
        value: value.into(),
        detail: detail.into(),
        current,
        action: SettingsPanelAction::OpenPanel(panel),
    }
}

fn activate_settings_panel_item(
    session: &mut ShellSession,
    provider: &mut ProviderConfig,
    state: &mut NativeTuiState,
    current_panel: SettingsSubpanel,
    back_stack: &[SettingsSubpanel],
    item: &SettingsPanelItem,
) -> Result<(), String> {
    match &item.action {
        SettingsPanelAction::Command(command) => {
            let _ = session.handle_command(command, provider, false)?;
            return_to_previous_settings_screen(session, provider, state, back_stack);
        }
        SettingsPanelAction::EditCommand(command) => {
            state.editor.replace_buffer(command.clone());
            return_to_previous_settings_screen(session, provider, state, back_stack);
        }
        SettingsPanelAction::OpenPanel(panel) => {
            let mut next_stack = back_stack.to_vec();
            next_stack.push(current_panel);
            open_settings_subpanel_with_back_stack(session, provider, state, *panel, next_stack);
        }
        SettingsPanelAction::OpenSandboxDirectoryPicker { kind, edit } => {
            open_sandbox_directory_overlay(session, state, *kind, *edit)?;
        }
        SettingsPanelAction::OpenSandboxRootRemovePicker(kind) => {
            open_sandbox_root_remove_overlay(session, state, *kind);
        }
        SettingsPanelAction::Model(action) => {
            let item = ModelPickerItem {
                label: item.label.clone(),
                detail: item.detail.clone(),
                current: item.current,
                action: action.clone(),
            };
            activate_model_picker_item(session, provider, &item)?;
            return_to_previous_settings_screen(session, provider, state, back_stack);
        }
    }
    session.sync_ui_docks();
    Ok(())
}

fn settings_subpanel_title(panel: SettingsSubpanel) -> &'static str {
    match panel {
        SettingsSubpanel::General => "general",
        SettingsSubpanel::Footer => "footer",
        SettingsSubpanel::Images => "images",
        SettingsSubpanel::Compaction => "compaction",
        SettingsSubpanel::Theme => "theme",
        SettingsSubpanel::Permissions => "permissions",
        SettingsSubpanel::Provider => "provider",
        SettingsSubpanel::Login => "login",
        SettingsSubpanel::LoginSubscription => "login › subscription",
        SettingsSubpanel::LoginApi => "login › api",
        SettingsSubpanel::LoginClaude => "login › claude",
        SettingsSubpanel::Memory => "memory",
        SettingsSubpanel::RoleModels => "role models",
        SettingsSubpanel::ScopedModels => "scoped models",
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct QuestionAnswerChoice {
    label: String,
    answer: Option<String>,
}

fn pending_question_answer_choices(session: &ShellSession) -> Vec<QuestionAnswerChoice> {
    session
        .pending_question
        .as_ref()
        .map(|pending| question_answer_choices_for_request(&pending.request))
        .unwrap_or_default()
}

fn question_answer_choices_for_request(request: &AskUserRequest) -> Vec<QuestionAnswerChoice> {
    let Some(question) = request.questions.first() else {
        return Vec::new();
    };
    let mut choices = question
        .options
        .iter()
        .map(|option| QuestionAnswerChoice {
            label: format!("{}. {}", option.id, option.label),
            answer: Some(option.id.clone()),
        })
        .collect::<Vec<_>>();
    if choices.is_empty() || question.allow_custom.unwrap_or(false) {
        choices.push(QuestionAnswerChoice {
            label: "custom answer".to_string(),
            answer: None,
        });
    }
    choices
}

fn open_sessions_overlay(
    session: &mut ShellSession,
    state: &mut NativeTuiState,
) -> Result<(), String> {
    let items = session_picker_items(session)?;
    let selected = items
        .iter()
        .position(|item| item.current)
        .unwrap_or_default();
    state.overlay = Some(TuiOverlay::Sessions {
        selected,
        query: String::new(),
        items,
    });
    Ok(())
}

fn open_tree_entries_overlay(
    session: &mut ShellSession,
    state: &mut NativeTuiState,
) -> Result<(), String> {
    let result = load_tree_entries_overlay_window(session, session.tree_filter_mode, 0)?;
    state.overlay = Some(TuiOverlay::TreeEntries {
        selected: 0,
        query: String::new(),
        entries: result.entries,
        branch: Some(result.branch),
        filter_mode: session.tree_filter_mode,
        after: result.after,
        total_count: result.total_count,
        previous_afters: Vec::new(),
    });
    Ok(())
}

fn load_tree_entries_overlay_window(
    session: &mut ShellSession,
    filter_mode: TreeFilterMode,
    after: u64,
) -> Result<ThreadItemsResult, String> {
    let value = session.rpc.request(
        "thread/items",
        json!({
            "threadId": session.thread_id,
            "after": after,
            "filter": filter_mode.as_str(),
            "limit": TREE_ENTRY_PICKER_WINDOW_LIMIT
        }),
    )?;
    serde_json::from_value(value)
        .map_err(|error| format!("decode thread/items for tree picker: {error}"))
}

fn tui_prompt_picker_route(prompt: &str) -> Option<TuiPromptPickerRoute> {
    let parts = prompt.split_whitespace().collect::<Vec<_>>();
    match parts.as_slice() {
        ["/tree"] | ["/tree", "entries" | "messages" | "items"] => {
            Some(TuiPromptPickerRoute::TreeEntries)
        }
        ["/sessions"] | ["/resume"] | ["/tree", "sessions" | "threads"] => {
            Some(TuiPromptPickerRoute::Sessions)
        }
        _ => None,
    }
}

fn session_picker_items(session: &mut ShellSession) -> Result<Vec<SessionPickerItem>, String> {
    let value = session.rpc.request("thread/list", json!({}))?;
    let result: RuntimeListResult<Thread> = serde_json::from_value(value)
        .map_err(|error| format!("decode thread/list for session picker: {error}"))?;
    Ok(result
        .items
        .into_iter()
        .filter(|thread| tree_thread_visible(thread, &session.thread_id, session.tree_filter_mode))
        .map(|thread| {
            let cwd = thread.project.cwd;
            SessionPickerItem {
                current: thread.id == session.thread_id,
                id: thread.id,
                title: thread
                    .title
                    .unwrap_or_else(|| "Untitled session".to_string()),
                status: format!("{:?}", thread.status),
                cwd,
                forked_from: thread.forked_from,
            }
        })
        .collect())
}

fn filtered_session_items<'a>(
    items: &'a [SessionPickerItem],
    query: &str,
) -> Vec<(usize, &'a SessionPickerItem)> {
    let query = query.trim().to_ascii_lowercase();
    items
        .iter()
        .enumerate()
        .filter(|(_, item)| {
            query.is_empty()
                || item.id.to_ascii_lowercase().contains(&query)
                || item.title.to_ascii_lowercase().contains(&query)
                || item.cwd.to_ascii_lowercase().contains(&query)
                || item.status.to_ascii_lowercase().contains(&query)
                || item
                    .forked_from
                    .as_deref()
                    .unwrap_or_default()
                    .to_ascii_lowercase()
                    .contains(&query)
        })
        .collect()
}

fn selected_session_item<'a>(
    items: &'a [SessionPickerItem],
    query: &str,
    selected: usize,
) -> Option<&'a SessionPickerItem> {
    filtered_session_items(items, query)
        .get(selected)
        .map(|(_, item)| *item)
}

fn resume_session_picker_item(
    session: &mut ShellSession,
    item: &SessionPickerItem,
) -> Result<(), String> {
    if item.current || item.id == session.thread_id {
        return session.print_text(&format!("already on session {}", item.id), false);
    }
    let value = session
        .rpc
        .request("thread/resume", json!({ "threadId": item.id }))?;
    session.switch_to_thread(value, false, "resumed")
}

fn filtered_tree_entry_items<'a>(
    entries: &'a [ThreadItemEntry],
    query: &str,
) -> Vec<(usize, &'a ThreadItemEntry)> {
    let query = query.trim().to_ascii_lowercase();
    entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| {
            if query.is_empty() {
                return true;
            }
            let item = &entry.item;
            entry.event_id.to_string().contains(&query)
                || item.id.to_ascii_lowercase().contains(&query)
                || item.turn_id.to_ascii_lowercase().contains(&query)
                || tree_item_kind_name(&item.kind)
                    .to_ascii_lowercase()
                    .contains(&query)
                || tree_item_label(&item.kind)
                    .to_ascii_lowercase()
                    .contains(&query)
                || tree_item_preview(&item.kind)
                    .to_ascii_lowercase()
                    .contains(&query)
                || entry
                    .labels
                    .iter()
                    .any(|label| label.to_ascii_lowercase().contains(&query))
        })
        .collect()
}

fn selected_tree_entry_item<'a>(
    entries: &'a [ThreadItemEntry],
    query: &str,
    selected: usize,
) -> Option<&'a ThreadItemEntry> {
    filtered_tree_entry_items(entries, query)
        .get(selected)
        .map(|(_, entry)| *entry)
}

fn tree_entry_picker_next_window_after(
    entries: &[ThreadItemEntry],
    total_count: u64,
) -> Option<u64> {
    thread_item_tree_next_after(entries, total_count)
}

fn tree_entry_picker_previous_window_after(previous_afters: &[u64]) -> Option<u64> {
    previous_afters.last().copied()
}

fn tree_entry_branch_navigation_key(key: &str) -> Option<TreeEntryBranchNavigation> {
    match key {
        "{" => Some(TreeEntryBranchNavigation::Parent),
        "}" => Some(TreeEntryBranchNavigation::Child),
        "[" => Some(TreeEntryBranchNavigation::PreviousSibling),
        "]" => Some(TreeEntryBranchNavigation::NextSibling),
        _ => None,
    }
}

fn tree_entry_branch_navigation_target(
    branch: Option<&ThreadItemBranch>,
    navigation: TreeEntryBranchNavigation,
) -> Option<String> {
    let branch = branch?;
    match navigation {
        TreeEntryBranchNavigation::Parent => branch.parent_thread_id.clone(),
        TreeEntryBranchNavigation::Child => branch.child_thread_ids.first().cloned(),
        TreeEntryBranchNavigation::PreviousSibling => branch.sibling_thread_ids.last().cloned(),
        TreeEntryBranchNavigation::NextSibling => branch.sibling_thread_ids.first().cloned(),
    }
}

fn tree_entry_branch_navigation_hint(branch: Option<&ThreadItemBranch>) -> String {
    let mut parts = Vec::new();
    if tree_entry_branch_navigation_target(branch, TreeEntryBranchNavigation::Parent).is_some() {
        parts.push("{ parent");
    }
    if tree_entry_branch_navigation_target(branch, TreeEntryBranchNavigation::PreviousSibling)
        .is_some()
        || tree_entry_branch_navigation_target(branch, TreeEntryBranchNavigation::NextSibling)
            .is_some()
    {
        parts.push("[/] siblings");
    }
    if tree_entry_branch_navigation_target(branch, TreeEntryBranchNavigation::Child).is_some() {
        parts.push("} child");
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!(" · {}", parts.join(" · "))
    }
}

fn tree_entry_picker_effect(
    action: TreeEntryPickerAction,
    entry: &ThreadItemEntry,
) -> TreeEntryPickerEffect {
    match action {
        TreeEntryPickerAction::Fork => {
            TreeEntryPickerEffect::RunCommand(format!("/tree fork {}", entry.event_id))
        }
        TreeEntryPickerAction::Edit => tree_entry_text_input_initial_value(entry)
            .map(|input| TreeEntryPickerEffect::OpenTextInput {
                action: TreeEntryTextInputAction::Edit,
                event_id: entry.event_id,
                input: input.to_string(),
                preview: tree_item_preview(&entry.item.kind),
            })
            .unwrap_or_else(|| {
                TreeEntryPickerEffect::DraftCommand(format!("/tree edit {} ", entry.event_id))
            }),
        TreeEntryPickerAction::Resubmit => tree_entry_text_input_initial_value(entry)
            .map(|input| TreeEntryPickerEffect::OpenTextInput {
                action: TreeEntryTextInputAction::Resubmit,
                event_id: entry.event_id,
                input: input.to_string(),
                preview: tree_item_preview(&entry.item.kind),
            })
            .unwrap_or_else(|| {
                TreeEntryPickerEffect::DraftCommand(format!("/tree resubmit {} ", entry.event_id))
            }),
        TreeEntryPickerAction::Label => TreeEntryPickerEffect::OpenLabelInput {
            event_id: entry.event_id,
            input: tree_entry_label_input_initial_value(entry),
            preview: tree_item_preview(&entry.item.kind),
        },
    }
}

fn tree_entry_text_input_initial_value(entry: &ThreadItemEntry) -> Option<&str> {
    match &entry.item.kind {
        ItemKind::UserMessage { text } => Some(text),
        _ => None,
    }
}

fn tree_entry_text_input_command(
    action: TreeEntryTextInputAction,
    event_id: u64,
    input: &str,
) -> Option<String> {
    let input = input.trim();
    if input.is_empty() {
        return None;
    }
    Some(format!(
        "/tree {} {event_id} {input}",
        tree_entry_text_input_command_name(action)
    ))
}

fn tree_entry_text_input_command_name(action: TreeEntryTextInputAction) -> &'static str {
    match action {
        TreeEntryTextInputAction::Edit => "edit",
        TreeEntryTextInputAction::Resubmit => "resubmit",
    }
}

fn tree_entry_text_input_title(action: TreeEntryTextInputAction) -> &'static str {
    match action {
        TreeEntryTextInputAction::Edit => "Edit",
        TreeEntryTextInputAction::Resubmit => "Resubmit",
    }
}

fn tree_entry_label_input_initial_value(entry: &ThreadItemEntry) -> String {
    entry.labels.join(", ")
}

fn tree_entry_label_input_command(event_id: u64, input: &str) -> String {
    let input = input.trim();
    if input.is_empty() {
        format!("/tree unlabel {event_id}")
    } else {
        format!("/tree label {event_id} {input}")
    }
}

fn tree_entries_overlay_title(query: &str, filter_mode: TreeFilterMode) -> String {
    if query.is_empty() {
        format!("tree entries · filter {}", filter_mode.as_str())
    } else {
        format!(
            "tree entries · filter {} · search: {query}",
            filter_mode.as_str()
        )
    }
}

fn tree_entry_picker_label(entry: &ThreadItemEntry) -> String {
    let item = &entry.item;
    let preview = tree_item_preview(&item.kind);
    if preview.is_empty() {
        format!(
            "{} event {} · {}",
            tree_item_label(&item.kind),
            entry.event_id,
            item.id
        )
    } else {
        format!(
            "{} event {} · {}",
            tree_item_label(&item.kind),
            entry.event_id,
            preview
        )
    }
}

fn tree_entry_picker_detail(
    entry: &ThreadItemEntry,
    branch: Option<&ThreadItemBranch>,
    after: u64,
    total_count: u64,
) -> String {
    let item = &entry.item;
    let mut parts = vec![
        item.turn_id.clone(),
        format!("item {}", item.id),
        format!("shown in {total_count}"),
    ];
    if after > 0 {
        parts.push(format!("after {after}"));
    }
    if let Some(parent) = entry.parent_event_id {
        parts.push(format!("parent {parent}"));
    }
    if !entry.child_event_ids.is_empty() {
        let children = entry
            .child_event_ids
            .iter()
            .map(u64::to_string)
            .collect::<Vec<_>>()
            .join(",");
        parts.push(format!("children {children}"));
    }
    if !entry.labels.is_empty() {
        parts.push(format!("labels {}", entry.labels.join(",")));
    }
    if let Some(branch) = branch {
        parts.push(format!("branch depth {}", branch.depth));
    }
    parts.join(" · ")
}

fn open_sandbox_directory_overlay(
    session: &ShellSession,
    state: &mut NativeTuiState,
    kind: SandboxRootKind,
    edit: SandboxRootEditMode,
) -> Result<(), String> {
    let cwd = sandbox_directory_picker_start_dir(session, kind);
    let items = sandbox_directory_picker_items(&cwd)?;
    state.overlay = Some(TuiOverlay::SandboxDirectories {
        selected: 0,
        query: String::new(),
        kind,
        edit,
        cwd,
        items,
    });
    Ok(())
}

fn open_sandbox_root_remove_overlay(
    session: &ShellSession,
    state: &mut NativeTuiState,
    kind: SandboxRootKind,
) {
    let roots = match kind {
        SandboxRootKind::Project => &session.permission_sandbox_settings.project_roots,
        SandboxRootKind::Read => &session.permission_sandbox_settings.readable_roots,
        SandboxRootKind::Write => &session.permission_sandbox_settings.writable_roots,
    };
    state.overlay = Some(TuiOverlay::SandboxRootEntries {
        selected: 0,
        query: String::new(),
        kind,
        items: sandbox_root_remove_picker_items(roots),
        selected_roots: Vec::new(),
    });
}

fn sandbox_directory_picker_start_dir(session: &ShellSession, kind: SandboxRootKind) -> PathBuf {
    let configured = match kind {
        SandboxRootKind::Project => session.permission_sandbox_settings.project_roots.first(),
        SandboxRootKind::Read => session.permission_sandbox_settings.readable_roots.first(),
        SandboxRootKind::Write => session.permission_sandbox_settings.writable_roots.first(),
    };
    configured
        .map(PathBuf::from)
        .filter(|path| path.is_dir())
        .unwrap_or_else(|| PathBuf::from(&session.cwd))
}

fn sandbox_directory_picker_items(root: &Path) -> Result<Vec<SandboxDirectoryItem>, String> {
    let mut items = Vec::new();
    items.push(SandboxDirectoryItem {
        label: "Use this directory".to_string(),
        detail: root.display().to_string(),
        path: root.to_path_buf(),
        action: SandboxDirectoryAction::Choose(root.to_path_buf()),
    });
    if let Some(parent) = root.parent() {
        items.push(SandboxDirectoryItem {
            label: "..".to_string(),
            detail: "parent directory".to_string(),
            path: parent.to_path_buf(),
            action: SandboxDirectoryAction::Open(parent.to_path_buf()),
        });
    }
    let mut directories = fs::read_dir(root)
        .map_err(|error| {
            format!(
                "read sandbox directory picker root {}: {error}",
                root.display()
            )
        })?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let file_type = entry.file_type().ok()?;
            file_type.is_dir().then_some(entry)
        })
        .map(|entry| {
            let path = entry.path();
            let label = entry.file_name().to_string_lossy().to_string();
            SandboxDirectoryItem {
                label,
                detail: "directory".to_string(),
                path: path.clone(),
                action: SandboxDirectoryAction::Open(path),
            }
        })
        .collect::<Vec<_>>();
    directories.sort_by_key(|item| item.label.to_ascii_lowercase());
    items.extend(directories);
    Ok(items)
}

fn filtered_sandbox_directory_items<'a>(
    items: &'a [SandboxDirectoryItem],
    query: &str,
) -> Vec<(usize, &'a SandboxDirectoryItem)> {
    let query = query.trim().to_ascii_lowercase();
    items
        .iter()
        .enumerate()
        .filter(|(_, item)| {
            query.is_empty()
                || item.label.to_ascii_lowercase().contains(&query)
                || item.detail.to_ascii_lowercase().contains(&query)
                || item
                    .path
                    .display()
                    .to_string()
                    .to_ascii_lowercase()
                    .contains(&query)
        })
        .collect()
}

fn selected_sandbox_directory_item<'a>(
    items: &'a [SandboxDirectoryItem],
    query: &str,
    selected: usize,
) -> Option<&'a SandboxDirectoryItem> {
    filtered_sandbox_directory_items(items, query)
        .get(selected)
        .map(|(_, item)| *item)
}

fn sandbox_directory_parent_items(
    cwd: &Path,
) -> Result<Option<(PathBuf, Vec<SandboxDirectoryItem>)>, String> {
    let Some(parent) = cwd.parent().map(Path::to_path_buf) else {
        return Ok(None);
    };
    let items = sandbox_directory_picker_items(&parent)?;
    Ok(Some((parent, items)))
}

fn move_sandbox_directory_picker_to_parent(
    cwd: &mut PathBuf,
    items: &mut Vec<SandboxDirectoryItem>,
    query: &mut String,
    selected: &mut usize,
) {
    if let Ok(Some((parent, next_items))) = sandbox_directory_parent_items(cwd) {
        *cwd = parent;
        *items = next_items;
        query.clear();
        *selected = 0;
    }
}

fn sandbox_root_remove_picker_items(roots: &[String]) -> Vec<SandboxRootEntryItem> {
    let roots = normalize_string_slice(roots);
    if roots.is_empty() {
        return Vec::new();
    }
    let mut items = vec![SandboxRootEntryItem {
        label: "All configured roots".to_string(),
        detail: "clear all configured roots".to_string(),
        root: "*".to_string(),
    }];
    items.extend(roots.into_iter().map(|root| SandboxRootEntryItem {
        label: root.clone(),
        detail: "configured root".to_string(),
        root,
    }));
    items
}

fn filtered_sandbox_root_entry_items<'a>(
    items: &'a [SandboxRootEntryItem],
    query: &str,
) -> Vec<(usize, &'a SandboxRootEntryItem)> {
    let query = query.trim().to_ascii_lowercase();
    items
        .iter()
        .enumerate()
        .filter(|(_, item)| {
            query.is_empty()
                || item.label.to_ascii_lowercase().contains(&query)
                || item.detail.to_ascii_lowercase().contains(&query)
                || item.root.to_ascii_lowercase().contains(&query)
        })
        .collect()
}

fn sandbox_root_entry_configured_roots(items: &[SandboxRootEntryItem]) -> Vec<String> {
    normalize_string_slice(
        &items
            .iter()
            .filter(|item| item.root != "*")
            .map(|item| item.root.clone())
            .collect::<Vec<_>>(),
    )
}

fn ordered_sandbox_root_selection(
    items: &[SandboxRootEntryItem],
    selected_roots: &[String],
) -> Vec<String> {
    let selected_roots = normalize_string_slice(selected_roots);
    let mut ordered = Vec::new();
    for item in items {
        if item.root != "*" && selected_roots.contains(&item.root) && !ordered.contains(&item.root)
        {
            ordered.push(item.root.clone());
        }
    }
    for root in selected_roots {
        if root != "*" && !ordered.contains(&root) {
            ordered.push(root);
        }
    }
    ordered
}

fn toggle_sandbox_root_entry_selection(
    items: &[SandboxRootEntryItem],
    selected_roots: &[String],
    item: &SandboxRootEntryItem,
) -> Vec<String> {
    let all_roots = sandbox_root_entry_configured_roots(items);
    let mut selected_roots = ordered_sandbox_root_selection(items, selected_roots);
    if item.root == "*" {
        if !all_roots.is_empty() && all_roots.iter().all(|root| selected_roots.contains(root)) {
            return Vec::new();
        }
        return all_roots;
    }
    if selected_roots.contains(&item.root) {
        selected_roots.retain(|root| root != &item.root);
    } else {
        selected_roots.push(item.root.clone());
    }
    ordered_sandbox_root_selection(items, &selected_roots)
}

fn selected_sandbox_root_entry_item<'a>(
    items: &'a [SandboxRootEntryItem],
    query: &str,
    selected: usize,
) -> Option<&'a SandboxRootEntryItem> {
    filtered_sandbox_root_entry_items(items, query)
        .get(selected)
        .map(|(_, item)| *item)
}

fn sandbox_directory_picker_command(
    kind: SandboxRootKind,
    edit: SandboxRootEditMode,
    path: &Path,
) -> String {
    format!(
        "{}{}{}",
        sandbox_root_kind_command_prefix(kind),
        sandbox_root_edit_command_infix(edit),
        path.display()
    )
}

fn sandbox_root_remove_command(kind: SandboxRootKind, root: &str) -> String {
    format!("{}remove {}", sandbox_root_kind_command_prefix(kind), root)
}

fn sandbox_root_entry_command(kind: SandboxRootKind, item: &SandboxRootEntryItem) -> String {
    if item.root == "*" {
        format!("{}default", sandbox_root_kind_command_prefix(kind))
    } else {
        sandbox_root_remove_command(kind, &item.root)
    }
}

fn sandbox_root_entries_command(
    kind: SandboxRootKind,
    item: &SandboxRootEntryItem,
    selected_roots: &[String],
) -> String {
    if item.root == "*" {
        return format!("{}default", sandbox_root_kind_command_prefix(kind));
    }
    let selected_roots = normalize_string_slice(
        &selected_roots
            .iter()
            .filter(|root| root.as_str() != "*")
            .cloned()
            .collect::<Vec<_>>(),
    );
    if selected_roots.is_empty() {
        sandbox_root_entry_command(kind, item)
    } else {
        format!(
            "{}remove {}",
            sandbox_root_kind_command_prefix(kind),
            selected_roots.join(",")
        )
    }
}

fn sandbox_root_entry_action_label(
    item: &SandboxRootEntryItem,
    selected_roots: &[String],
) -> String {
    if item.root == "*" {
        return "clear all".to_string();
    }
    if selected_roots.contains(&item.root) {
        "selected".to_string()
    } else {
        "remove".to_string()
    }
}

fn sandbox_root_kind_command_prefix(kind: SandboxRootKind) -> &'static str {
    match kind {
        SandboxRootKind::Project => "/permissions sandbox project ",
        SandboxRootKind::Read => "/permissions sandbox read ",
        SandboxRootKind::Write => "/permissions sandbox write ",
    }
}

fn sandbox_root_kind_label(kind: SandboxRootKind) -> &'static str {
    match kind {
        SandboxRootKind::Project => "project",
        SandboxRootKind::Read => "readable",
        SandboxRootKind::Write => "writable",
    }
}

fn sandbox_root_edit_command_infix(edit: SandboxRootEditMode) -> &'static str {
    match edit {
        SandboxRootEditMode::Set => "",
        SandboxRootEditMode::Add => "add ",
    }
}

fn sandbox_root_edit_mode_label(edit: SandboxRootEditMode) -> &'static str {
    match edit {
        SandboxRootEditMode::Set => "picker",
        SandboxRootEditMode::Add => "add",
    }
}

fn sandbox_directory_item_value(action: &SandboxDirectoryAction) -> String {
    match action {
        SandboxDirectoryAction::Choose(path) => format!("choose {}", path.display()),
        SandboxDirectoryAction::Open(path) => format!("open {}", path.display()),
    }
}

fn sandbox_directory_item_detail_for_render(item: &SandboxDirectoryItem) -> String {
    match &item.action {
        SandboxDirectoryAction::Choose(path) => {
            format!("Choose current directory: {}", path.display())
        }
        SandboxDirectoryAction::Open(path) if item.label == ".." => {
            format!("Open parent directory: {}", path.display())
        }
        SandboxDirectoryAction::Open(path) => format!("Open directory: {}", path.display()),
    }
}

fn open_models_overlay(
    session: &mut ShellSession,
    provider: &ProviderConfig,
    state: &mut NativeTuiState,
) -> Result<(), String> {
    let groups = model_picker_item_groups(session, provider)?;
    state.overlay = Some(TuiOverlay::Models {
        selected: model_picker_selected_index_after_scope(&groups.items, ""),
        query: String::new(),
        scope: groups.scope,
        items: groups.items,
        all_items: groups.all_items,
        scoped_items: groups.scoped_items,
    });
    Ok(())
}

fn open_effort_overlay(provider: &ProviderConfig, state: &mut NativeTuiState) {
    let allowed = allowed_effort_levels_for_provider(provider);
    let current = current_effort_level_for_provider(provider);
    let selected_level = if allowed.contains(&current) {
        current
    } else {
        allowed.last().copied().unwrap_or(ThinkingLevel::Off)
    };
    let selected = allowed
        .iter()
        .position(|level| *level == selected_level)
        .unwrap_or_default();
    state.overlay = Some(TuiOverlay::Effort {
        selected,
        allowed,
        current,
    });
}

struct ModelPickerItemGroups {
    scope: ModelPickerScope,
    items: Vec<ModelPickerItem>,
    all_items: Vec<ModelPickerItem>,
    scoped_items: Vec<ModelPickerItem>,
}

fn model_picker_item_groups(
    session: &mut ShellSession,
    provider: &ProviderConfig,
) -> Result<ModelPickerItemGroups, String> {
    let session_model = session.session_model(provider).map(str::to_string);
    let all_refs =
        session.runtime_model_refs_for_provider_with_scope(provider, true, Vec::new())?;
    let scoped_refs = if session.scoped_model_ids.is_empty() {
        Vec::new()
    } else {
        session.runtime_model_refs_for_provider(provider, true)?
    };
    let all_items = model_picker_items_from_refs(&all_refs, session_model.as_deref());
    let scoped_items = model_picker_items_from_refs(&scoped_refs, session_model.as_deref());
    let scope = if scoped_items.is_empty() {
        ModelPickerScope::All
    } else {
        ModelPickerScope::Scoped
    };
    let items = model_picker_items_for_scope(scope, &all_items, &scoped_items);
    Ok(ModelPickerItemGroups {
        scope,
        items,
        all_items,
        scoped_items,
    })
}

fn model_picker_items_for_scope(
    scope: ModelPickerScope,
    all_items: &[ModelPickerItem],
    scoped_items: &[ModelPickerItem],
) -> Vec<ModelPickerItem> {
    match scope {
        ModelPickerScope::All => all_items.to_vec(),
        ModelPickerScope::Scoped => scoped_items.to_vec(),
    }
}

fn model_picker_selected_index_after_scope(items: &[ModelPickerItem], query: &str) -> usize {
    let filtered = filtered_model_items(items, query);
    if filtered.is_empty() {
        return 0;
    }
    if let Some(current_index) = items.iter().position(|item| item.current)
        && let Some(filtered_index) = filtered
            .iter()
            .position(|(index, _)| *index == current_index)
    {
        return filtered_index;
    }
    0
}

fn set_model_picker_scope_fields(
    selected: &mut usize,
    query: &str,
    scope: &mut ModelPickerScope,
    items: &mut Vec<ModelPickerItem>,
    all_items: &[ModelPickerItem],
    scoped_items: &[ModelPickerItem],
    next_scope: ModelPickerScope,
) -> bool {
    if *scope == next_scope || scoped_items.is_empty() {
        return false;
    }
    *scope = next_scope;
    *items = model_picker_items_for_scope(*scope, all_items, scoped_items);
    *selected = model_picker_selected_index_after_scope(items, query);
    true
}

fn toggle_model_picker_scope_fields(
    selected: &mut usize,
    query: &str,
    scope: &mut ModelPickerScope,
    items: &mut Vec<ModelPickerItem>,
    all_items: &[ModelPickerItem],
    scoped_items: &[ModelPickerItem],
) -> bool {
    let next_scope = match *scope {
        ModelPickerScope::All => ModelPickerScope::Scoped,
        ModelPickerScope::Scoped => ModelPickerScope::All,
    };
    set_model_picker_scope_fields(
        selected,
        query,
        scope,
        items,
        all_items,
        scoped_items,
        next_scope,
    )
}

#[cfg(test)]
fn toggle_model_picker_scope(overlay: &mut TuiOverlay) -> bool {
    match overlay {
        TuiOverlay::Models {
            selected,
            query,
            scope,
            items,
            all_items,
            scoped_items,
        } => {
            toggle_model_picker_scope_fields(selected, query, scope, items, all_items, scoped_items)
        }
        _ => false,
    }
}

fn model_picker_items_from_refs(
    models: &[ModelRef],
    session_model: Option<&str>,
) -> Vec<ModelPickerItem> {
    models
        .iter()
        .map(|model| {
            let label = if model.display_name.trim().is_empty() {
                display_model_id(&model.id).to_string()
            } else {
                model.display_name.clone()
            };
            ModelPickerItem {
                label,
                detail: format!(
                    "Set {} for OPPi executor turns",
                    display_model_id(&model.id)
                ),
                current: session_model == Some(model.id.as_str()),
                action: ModelPickerAction::SelectSession(model.id.clone()),
            }
        })
        .collect()
}

fn filtered_model_items<'a>(
    items: &'a [ModelPickerItem],
    query: &str,
) -> Vec<(usize, &'a ModelPickerItem)> {
    let query = query.trim().to_ascii_lowercase();
    items
        .iter()
        .enumerate()
        .filter(|(_, item)| {
            query.is_empty()
                || item.label.to_ascii_lowercase().contains(&query)
                || item.detail.to_ascii_lowercase().contains(&query)
                || model_picker_action_text(&item.action)
                    .to_ascii_lowercase()
                    .contains(&query)
        })
        .collect()
}

fn selected_model_item<'a>(
    items: &'a [ModelPickerItem],
    query: &str,
    selected: usize,
) -> Option<&'a ModelPickerItem> {
    filtered_model_items(items, query)
        .get(selected)
        .map(|(_, item)| *item)
}

fn cycle_session_model(
    session: &mut ShellSession,
    provider: &mut ProviderConfig,
    reverse: bool,
) -> Result<(), String> {
    let models = session.runtime_model_ids_for_provider(provider, true)?;
    if models.is_empty() {
        return session.print_text("no models available; use /login or /model", false);
    }
    let current = session.session_model(provider).unwrap_or_default();
    let current_index = models
        .iter()
        .position(|model| model == current)
        .unwrap_or(0);
    let next_index = if reverse {
        (current_index + models.len() - 1) % models.len()
    } else {
        (current_index + 1) % models.len()
    };
    session.select_session_model(provider, &models[next_index], false)
}

fn cycle_reasoning_effort(
    session: &mut ShellSession,
    provider: &mut ProviderConfig,
) -> Result<(), String> {
    let levels = allowed_effort_levels_for_provider(provider);
    if levels.len() <= 1 {
        return session.print_text(&format_effort_status(provider), false);
    }
    let current = current_effort_level_for_provider(provider);
    let current_index = levels
        .iter()
        .position(|level| *level == current)
        .unwrap_or(0);
    let next = levels[(current_index + 1) % levels.len()];
    session.handle_command(&format!("/effort {}", next.as_str()), provider, false)?;
    Ok(())
}

fn model_picker_action_text(action: &ModelPickerAction) -> String {
    match action {
        ModelPickerAction::SelectSession(model) => format!("main {}", display_model_id(model)),
        ModelPickerAction::SelectRole { role, model } => {
            format!("role {role} {}", display_model_id(model))
        }
        ModelPickerAction::ClearRole { role } => format!("role {role} inherit"),
    }
}

fn activate_model_picker_item(
    session: &mut ShellSession,
    provider: &mut ProviderConfig,
    item: &ModelPickerItem,
) -> Result<(), String> {
    match &item.action {
        ModelPickerAction::SelectSession(model) => {
            session.select_session_model(provider, model, false)?;
        }
        ModelPickerAction::SelectRole { role, model } => {
            session.role_models.insert(role.clone(), model.clone());
            save_role_profiles(&session.role_profile_path, &session.role_models)?;
            session.register_model_ref(model, provider_name(provider), Some(role))?;
            session.print_text(
                &format!(
                    "role {role} model set to {} (persisted)",
                    display_model_id(model)
                ),
                false,
            )?;
        }
        ModelPickerAction::ClearRole { role } => {
            session.role_models.remove(role);
            save_role_profiles(&session.role_profile_path, &session.role_models)?;
            session.print_text(
                &format!(
                    "role {role} now inherits {} (persisted)",
                    session
                        .session_model(provider)
                        .map(display_model_id)
                        .unwrap_or("session model")
                ),
                false,
            )?;
        }
    }
    session.sync_ui_docks();
    Ok(())
}

fn settings_root_items(session: &ShellSession, provider: &ProviderConfig) -> Vec<SettingsRootItem> {
    let session_model = session.session_model(provider).unwrap_or("none");
    let session_model = display_model_id(session_model);
    let scoped_model_count = session.scoped_model_ids.len();
    settings_root_items_for_values(
        provider,
        session_model,
        scoped_model_count,
        session.follow_up_mode,
        session.steering_mode,
        session.quiet_startup,
        session.double_escape_action,
        session.tree_filter_mode,
        session.enable_skill_commands,
        session.autocomplete_max_visible,
        session.show_hardware_cursor,
        session.editor_padding_x,
        session.show_terminal_progress,
        session.terminal_clear_on_shrink,
        session.permission_mode.as_str(),
        &session.theme,
        &session.thread_id,
        &session.goal_status_label(),
    )
}

fn settings_root_items_for_values(
    provider: &ProviderConfig,
    session_model: &str,
    scoped_model_count: usize,
    follow_up_mode: FollowUpMode,
    steering_mode: SteeringMode,
    quiet_startup: bool,
    double_escape_action: DoubleEscapeAction,
    tree_filter_mode: TreeFilterMode,
    enable_skill_commands: bool,
    autocomplete_max_visible: usize,
    show_hardware_cursor: bool,
    editor_padding_x: usize,
    show_terminal_progress: bool,
    terminal_clear_on_shrink: bool,
    permission_mode: &str,
    theme: &str,
    thread_id: &str,
    goal_status: &str,
) -> Vec<SettingsRootItem> {
    vec![
        SettingsRootItem {
            section: "General",
            label: "Status & usage",
            value: "/usage".to_string(),
            detail: "Usage/status, footer, and debug surfaces",
            action: SettingsRootAction::OpenPanel(SettingsSubpanel::General),
        },
        SettingsRootItem {
            section: "General",
            label: "Goal mode",
            value: goal_status.to_string(),
            detail: "Track and continue one thread objective",
            action: SettingsRootAction::InsertCommand("/goal".to_string()),
        },
        SettingsRootItem {
            section: "Pi",
            label: "Main model",
            value: session_model.to_string(),
            detail: "Select the default model for OPPi turns",
            action: SettingsRootAction::OpenMainModel,
        },
        SettingsRootItem {
            section: "Pi",
            label: "Default thinking",
            value: effort_level_label_for_provider(
                provider,
                current_effort_level_for_provider(provider),
            )
            .to_string(),
            detail: "Pi defaultThinkingLevel for the current main model",
            action: SettingsRootAction::OpenEffort,
        },
        SettingsRootItem {
            section: "Pi",
            label: "Scoped models",
            value: if scoped_model_count == 0 {
                "all".to_string()
            } else {
                format!("{scoped_model_count} patterns")
            },
            detail: "Limit /model and Ctrl+P cycling to selected models",
            action: SettingsRootAction::OpenPanel(SettingsSubpanel::ScopedModels),
        },
        SettingsRootItem {
            section: "Pi",
            label: "Role models",
            value: "advanced".to_string(),
            detail: "Per-task model overrides live here",
            action: SettingsRootAction::OpenPanel(SettingsSubpanel::RoleModels),
        },
        SettingsRootItem {
            section: "Pi",
            label: "Follow-up mode",
            value: follow_up_mode.as_str().to_string(),
            detail: "Alt+Enter queue delivery mode",
            action: SettingsRootAction::InsertCommand(format!(
                "/follow-up mode {}",
                follow_up_mode_toggle_arg(follow_up_mode)
            )),
        },
        SettingsRootItem {
            section: "Pi",
            label: "Steering mode",
            value: steering_mode.as_str().to_string(),
            detail: "Ctrl+Enter active-turn delivery mode",
            action: SettingsRootAction::InsertCommand(format!(
                "/steer mode {}",
                steering_mode_toggle_arg(steering_mode)
            )),
        },
        SettingsRootItem {
            section: "Pi",
            label: "Quiet startup",
            value: enabled_label(quiet_startup).to_string(),
            detail: "Suppress the non-critical ready banner",
            action: SettingsRootAction::InsertCommand(format!(
                "/startup quiet {}",
                quiet_startup_toggle_arg(quiet_startup)
            )),
        },
        SettingsRootItem {
            section: "Pi",
            label: "Double escape",
            value: double_escape_action.as_str().to_string(),
            detail: "Action when Escape is pressed twice with an empty editor",
            action: SettingsRootAction::InsertCommand(format!(
                "/editor double-escape {}",
                double_escape_action_toggle_arg(double_escape_action)
            )),
        },
        SettingsRootItem {
            section: "Pi",
            label: "Tree filter",
            value: tree_filter_mode.as_str().to_string(),
            detail: "Default filter for transcript entries and sessions",
            action: SettingsRootAction::InsertCommand(format!(
                "/tree filter {}",
                tree_filter_mode_toggle_arg(tree_filter_mode)
            )),
        },
        SettingsRootItem {
            section: "Pi",
            label: "Skill commands",
            value: enabled_label(enable_skill_commands).to_string(),
            detail: "Register skills as /skill:name commands",
            action: SettingsRootAction::InsertCommand(format!(
                "/skills commands {}",
                skill_commands_toggle_arg(enable_skill_commands)
            )),
        },
        SettingsRootItem {
            section: "Pi",
            label: "Autocomplete items",
            value: autocomplete_max_visible.to_string(),
            detail: "Maximum visible command suggestions",
            action: SettingsRootAction::InsertCommand(format!(
                "/autocomplete items {}",
                autocomplete_max_visible_toggle_arg(autocomplete_max_visible)
            )),
        },
        SettingsRootItem {
            section: "Pi",
            label: "Hardware cursor",
            value: enabled_label(show_hardware_cursor).to_string(),
            detail: "Keep the terminal cursor visible for IME support",
            action: SettingsRootAction::InsertCommand(format!(
                "/cursor hardware {}",
                hardware_cursor_toggle_arg(show_hardware_cursor)
            )),
        },
        SettingsRootItem {
            section: "Pi",
            label: "Editor padding",
            value: editor_padding_x.to_string(),
            detail: "Horizontal input padding",
            action: SettingsRootAction::InsertCommand(format!(
                "/editor padding {}",
                editor_padding_x_toggle_arg(editor_padding_x)
            )),
        },
        SettingsRootItem {
            section: "Pi",
            label: "Terminal progress",
            value: enabled_label(show_terminal_progress).to_string(),
            detail: "OSC 9;4 turn progress indicator",
            action: SettingsRootAction::InsertCommand(format!(
                "/terminal progress {}",
                terminal_progress_toggle_arg(show_terminal_progress)
            )),
        },
        SettingsRootItem {
            section: "Pi",
            label: "Clear on shrink",
            value: enabled_label(terminal_clear_on_shrink).to_string(),
            detail: "Clear stale terminal rows after content shrinks",
            action: SettingsRootAction::InsertCommand(format!(
                "/terminal clear-on-shrink {}",
                terminal_clear_on_shrink_toggle_arg(terminal_clear_on_shrink)
            )),
        },
        SettingsRootItem {
            section: "Pi",
            label: "Provider",
            value: provider.label().to_string(),
            detail: "Provider status, validation, and base URL",
            action: SettingsRootAction::OpenPanel(SettingsSubpanel::Provider),
        },
        SettingsRootItem {
            section: "Pi",
            label: "Login",
            value: "subscription/api".to_string(),
            detail: "Subscription and API authentication",
            action: SettingsRootAction::OpenPanel(SettingsSubpanel::Login),
        },
        SettingsRootItem {
            section: "Pi",
            label: "Images",
            value: "preview".to_string(),
            detail: "Terminal image preview and provider image handling",
            action: SettingsRootAction::OpenPanel(SettingsSubpanel::Images),
        },
        SettingsRootItem {
            section: "Permissions",
            label: "Mode",
            value: permission_mode.to_string(),
            detail: "Read/write/network approval policy",
            action: SettingsRootAction::OpenPanel(SettingsSubpanel::Permissions),
        },
        SettingsRootItem {
            section: "Footer",
            label: "Status bar",
            value: "live".to_string(),
            detail: "Footer help, usage, todos, model, permission, and memory chips",
            action: SettingsRootAction::OpenPanel(SettingsSubpanel::Footer),
        },
        SettingsRootItem {
            section: "Compaction",
            label: "Compact now",
            value: "handoff".to_string(),
            detail: "Create a todo-aware memory handoff now",
            action: SettingsRootAction::OpenPanel(SettingsSubpanel::Compaction),
        },
        SettingsRootItem {
            section: "Theme",
            label: "OPPi theme",
            value: theme.to_string(),
            detail: "Colors and terminal-safe rendering",
            action: SettingsRootAction::OpenPanel(SettingsSubpanel::Theme),
        },
        SettingsRootItem {
            section: "Workspace",
            label: "Sessions",
            value: thread_id.to_string(),
            detail: "Browse and resume prior sessions",
            action: SettingsRootAction::OpenSessions,
        },
        SettingsRootItem {
            section: "Memory",
            label: "Hoppi",
            value: "runtime-backed".to_string(),
            detail: "Recall, dashboard, and maintenance",
            action: SettingsRootAction::OpenPanel(SettingsSubpanel::Memory),
        },
    ]
}

fn settings_root_key_action(input: &EditorInput) -> SettingsRootKeyAction {
    match input {
        EditorInput::Up => SettingsRootKeyAction::MoveRow(-1),
        EditorInput::Down => SettingsRootKeyAction::MoveRow(1),
        EditorInput::Left => SettingsRootKeyAction::MoveTab(-1),
        EditorInput::Right => SettingsRootKeyAction::MoveTab(1),
        EditorInput::Home | EditorInput::PageUp => SettingsRootKeyAction::First,
        EditorInput::End | EditorInput::PageDown => SettingsRootKeyAction::Last,
        EditorInput::Enter => SettingsRootKeyAction::Activate,
        EditorInput::Text(text) if text == " " => SettingsRootKeyAction::Activate,
        EditorInput::Text(text) => SettingsRootKeyAction::Search(text.clone()),
        EditorInput::Backspace | EditorInput::Delete => SettingsRootKeyAction::SearchBackspace,
        EditorInput::Escape => SettingsRootKeyAction::Close,
        EditorInput::CtrlC => SettingsRootKeyAction::ArmCtrlCExit,
        EditorInput::CtrlD => SettingsRootKeyAction::Exit,
        _ => SettingsRootKeyAction::Ignore,
    }
}

fn filtered_settings_root_items<'a>(
    items: &'a [SettingsRootItem],
    query: &str,
) -> Vec<(usize, &'a SettingsRootItem)> {
    let query = query.trim().to_ascii_lowercase();
    items
        .iter()
        .enumerate()
        .filter(|(_, item)| settings_root_item_matches_query(item, &query))
        .collect()
}

fn settings_root_item_matches_query(item: &SettingsRootItem, query: &str) -> bool {
    if query.is_empty() {
        return true;
    }
    let haystack = format!(
        "{} {} {} {}",
        item.section, item.label, item.value, item.detail
    )
    .to_ascii_lowercase();
    query.split_whitespace().all(|term| haystack.contains(term))
}

fn first_visible_settings_index(items: &[SettingsRootItem], query: &str) -> Option<usize> {
    filtered_settings_root_items(items, query)
        .first()
        .map(|(index, _)| *index)
}

fn settings_root_move_visible(
    items: &[SettingsRootItem],
    query: &str,
    selected: usize,
    delta: isize,
) -> usize {
    if query.trim().is_empty() {
        return settings_root_move_row(items, selected, delta);
    }
    let visible = filtered_settings_root_items(items, query);
    if visible.is_empty() {
        return 0;
    }
    let pos = visible
        .iter()
        .position(|(index, _)| *index == selected)
        .unwrap_or(0) as isize;
    let next = (pos + delta).rem_euclid(visible.len() as isize) as usize;
    visible[next].0
}

fn settings_root_move_row(items: &[SettingsRootItem], selected: usize, delta: isize) -> usize {
    if items.is_empty() {
        return 0;
    }
    let selected = selected.min(items.len().saturating_sub(1));
    let section = items[selected].section;
    let section_indices = items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| (item.section == section).then_some(index))
        .collect::<Vec<_>>();
    if section_indices.is_empty() {
        return selected;
    }
    let pos = section_indices
        .iter()
        .position(|index| *index == selected)
        .unwrap_or(0) as isize;
    let next = (pos + delta).rem_euclid(section_indices.len() as isize) as usize;
    section_indices[next]
}

fn settings_root_move_tab(items: &[SettingsRootItem], selected: usize, delta: isize) -> usize {
    if items.is_empty() {
        return 0;
    }
    let selected = selected.min(items.len().saturating_sub(1));
    let mut sections = Vec::<&'static str>::new();
    for item in items {
        if sections.last().copied() != Some(item.section) {
            sections.push(item.section);
        }
    }
    let current = items[selected].section;
    let pos = sections
        .iter()
        .position(|section| *section == current)
        .unwrap_or(0) as isize;
    let next = (pos + delta).rem_euclid(sections.len() as isize) as usize;
    items
        .iter()
        .position(|item| item.section == sections[next])
        .unwrap_or(selected)
}

fn settings_section_display_label(section: &str) -> String {
    match section {
        "Safety" => "Permissions".to_string(),
        "Appearance" => "Theme".to_string(),
        other => other.to_string(),
    }
}

fn settings_section_bar(items: &[SettingsRootItem], active_section: &str) -> String {
    let mut sections = Vec::<&'static str>::new();
    for item in items {
        if sections.last().copied() != Some(item.section) {
            sections.push(item.section);
        }
    }
    sections
        .into_iter()
        .map(|section| {
            let label = settings_section_display_label(section);
            if section == active_section {
                format!("[{label}]")
            } else {
                label
            }
        })
        .collect::<Vec<_>>()
        .join("  ")
}

fn render_native_tui_frame(
    session: &ShellSession,
    provider: &ProviderConfig,
    state: &NativeTuiState,
    width: usize,
    height: usize,
) -> Vec<String> {
    let width = width.max(50);
    let height = height.max(14);
    let tokens = theme_tokens_for_tui_frame(
        &session.theme,
        session.loaded_theme.as_ref(),
        state,
        &session.theme_preview_cache,
    );

    let header = render_tui_header(session, provider, state, width, tokens);
    let dock_lines = render_tui_docks(session, width, tokens);
    let overlay_lines = state
        .overlay
        .as_ref()
        .map(|overlay| render_overlay_lines(overlay, session, provider, width, height, tokens))
        .unwrap_or_default();
    let editor_lines = render_tui_editor(
        &state.editor,
        width,
        session.editor_padding_x,
        session.is_turn_running(),
        tokens,
    );
    let slash_palette_lines = state
        .current_slash_palette(session, provider)
        .map(|palette| {
            render_slash_palette(&palette, width, session.autocomplete_max_visible, tokens)
        })
        .unwrap_or_default();
    let footer_lines = render_tui_footer(session, provider, state, width, tokens);

    let reserved = 1
        + dock_lines.len()
        + overlay_lines.len()
        + editor_lines.len()
        + slash_palette_lines.len()
        + footer_lines.len();
    let transcript_height = height.saturating_sub(reserved).max(3);

    let mut lines = Vec::new();
    lines.push(header);
    lines.extend(render_tui_transcript(
        session,
        width,
        transcript_height,
        tokens,
    ));
    lines.extend(dock_lines);
    lines.extend(overlay_lines);
    lines.extend(editor_lines);
    lines.extend(slash_palette_lines);
    lines.extend(footer_lines);

    while lines.len() < height {
        lines.push(tui_pad_line("", width));
    }
    lines.truncate(height);
    lines
        .into_iter()
        .map(|line| tui_pad_line(&line, width))
        .collect()
}

fn render_tui_header(
    session: &ShellSession,
    provider: &ProviderConfig,
    state: &NativeTuiState,
    width: usize,
    tokens: ThemeTokens,
) -> String {
    let spinner = if session.is_turn_running() {
        SPINNER_FRAMES[state.spinner_index % SPINNER_FRAMES.len()]
    } else if session.has_pending_pause() {
        "⏸"
    } else {
        "●"
    };
    let status = if session.is_turn_running() {
        "running"
    } else if session.has_pending_pause() {
        "waiting"
    } else {
        "ready"
    };
    let model = session.session_model(provider).unwrap_or("none");
    let model = display_model_id(model);
    let goal = session
        .goal_header_label()
        .map(|goal| format!(" · ◎ {}", tui_truncate(&goal, 28)))
        .unwrap_or_default();
    let title = format!(
        " {spinner} OPPi · {}/{} · perms: {} · {status}{goal} ",
        provider.label(),
        model,
        session.permission_mode.as_str()
    );
    let painted = paint(tokens, tokens.accent, title);
    tui_box_line('╭', '─', '╮', Some(&painted), width)
}

fn render_tui_transcript(
    session: &ShellSession,
    width: usize,
    max_lines: usize,
    tokens: ThemeTokens,
) -> Vec<String> {
    let inner = width.saturating_sub(4).max(10);
    let mut collected = Vec::new();
    for entry in session.ui.scrollback.iter().rev() {
        for line in entry.lines().rev() {
            collected.push(format!(
                "│ {} │",
                tui_pad_line(&tui_truncate(line, inner), inner)
            ));
            if collected.len() >= max_lines {
                break;
            }
        }
        if collected.len() >= max_lines {
            break;
        }
    }
    if collected.is_empty() {
        collected.push(format!(
            "│ {} │",
            tui_pad_line(
                &paint(
                    tokens,
                    tokens.dim,
                    "No transcript yet — type a prompt or /settings."
                ),
                inner
            )
        ));
    }
    collected.reverse();
    while collected.len() < max_lines {
        collected.insert(0, format!("│ {} │", tui_pad_line("", inner)));
    }
    collected
}

fn render_tui_docks(session: &ShellSession, width: usize, tokens: ThemeTokens) -> Vec<String> {
    let inner = width.saturating_sub(4).max(10);
    let dock_lines = session.ui.dock_lines(inner);
    if dock_lines.is_empty() {
        return vec![format!(
            "├{}┤",
            tui_pad_line(
                &paint(tokens, tokens.dim, " docks: idle "),
                width.saturating_sub(2)
            )
        )];
    }
    let mut lines = vec![format!(
        "├{}┤",
        tui_pad_line(
            &paint(tokens, tokens.dim, " docks "),
            width.saturating_sub(2)
        )
    )];
    for line in dock_lines.into_iter().take(5) {
        lines.push(format!(
            "│ {} │",
            tui_pad_line(&tui_truncate(&line, inner), inner)
        ));
    }
    lines
}

fn render_slash_palette(
    palette: &SlashPalette,
    width: usize,
    max_entries: usize,
    tokens: ThemeTokens,
) -> Vec<String> {
    let count = palette.items.len();
    if count == 0 {
        return Vec::new();
    }
    let max_entries = max_entries
        .max(1)
        .min(MAX_AUTOCOMPLETE_MAX_VISIBLE)
        .min(count);
    let selected = palette.selected.min(count.saturating_sub(1));
    let start = selected.saturating_add(1).saturating_sub(max_entries);
    let end = (start + max_entries).min(count);
    let mut lines = Vec::new();
    for (index, item) in palette
        .items
        .iter()
        .enumerate()
        .skip(start)
        .take(end.saturating_sub(start))
    {
        let marker = if index == selected { "→" } else { " " };
        let label = if index == selected {
            paint(tokens, tokens.accent, &item.label)
        } else {
            item.label.clone()
        };
        let detail = paint(tokens, tokens.dim, &item.detail);
        let row = if width >= 72 {
            let label_width = 34usize.min(width.saturating_sub(20));
            let label_cell = tui_pad_line(&tui_truncate(&label, label_width), label_width);
            format!("{marker} {label_cell} {detail}")
        } else {
            format!("{marker} {label} {detail}")
        };
        lines.push(tui_pad_line(&tui_truncate(&row, width), width));
    }
    if count > max_entries {
        lines.push(tui_pad_line(
            &paint(
                tokens,
                tokens.dim,
                &format!(
                    "  {}-{} of {} • ↑↓ navigate • Enter select",
                    start + 1,
                    end,
                    count
                ),
            ),
            width,
        ));
    }
    lines
}

fn render_tui_editor(
    editor: &LineEditor,
    width: usize,
    padding_x: usize,
    busy: bool,
    tokens: ThemeTokens,
) -> Vec<String> {
    let mut lines = vec![tui_rule(width, tokens)];
    let rendered_buffer = render_editor_buffer(editor, width, padding_x, tokens);
    for line in rendered_buffer.into_iter().take(5) {
        lines.push(tui_pad_line(&line, width));
    }
    if busy {
        lines.push(tui_pad_line(
            &paint(
                tokens,
                tokens.dim,
                "Enter queues follow-up while the turn runs",
            ),
            width,
        ));
    }
    lines.push(tui_rule(width, tokens));
    lines
}

fn render_editor_buffer(
    editor: &LineEditor,
    width: usize,
    padding_x: usize,
    _tokens: ThemeTokens,
) -> Vec<String> {
    let padding = " ".repeat(padding_x.min(width.saturating_sub(1)));
    let buffer = editor.buffer_preview();
    if buffer.is_empty() {
        return vec![format!("{padding}\x1b[7m \x1b[27m")];
    }
    let cursor = editor.cursor();
    let mut output = Vec::new();
    let mut offset = 0;
    for raw_line in buffer.split('\n') {
        let line_end = offset + raw_line.len();
        let mut line = padding.clone();
        if cursor >= offset && cursor <= line_end {
            let local = cursor - offset;
            let before = &raw_line[..local];
            let after = &raw_line[local..];
            let (cursor_char, rest) = split_first_char(after);
            line.push_str(before);
            line.push_str("\x1b[7m");
            line.push_str(cursor_char.unwrap_or(" "));
            line.push_str("\x1b[27m");
            line.push_str(rest);
        } else {
            line.push_str(raw_line);
        }
        output.push(tui_truncate(&line, width));
        offset = line_end + 1;
    }
    output
}

fn render_tui_footer(
    session: &ShellSession,
    provider: &ProviderConfig,
    state: &NativeTuiState,
    width: usize,
    tokens: ThemeTokens,
) -> Vec<String> {
    let queued = session.follow_up_queue.len() + session.steering_queue.len();
    let todos = active_todos(&session.todo_state).len();
    let status = if session.is_turn_running() {
        "running"
    } else if session.has_pending_pause() {
        "waiting"
    } else {
        "ready"
    };
    let goal = if session.current_goal.is_some() {
        format!(" · {}", session.goal_status_label())
    } else {
        String::new()
    };
    let left = format!(
        " {} · {} · {} · {} · {}t · {}q{} ",
        status,
        provider.label(),
        session.session_model(provider).unwrap_or("none"),
        session.permission_mode.as_str(),
        todos,
        queued,
        goal
    );
    let right = if state.footer_hotkeys_visible {
        " Alt+K Hide help "
    } else {
        " Alt+K Show help "
    };
    let gap = width.saturating_sub(tui_visible_width(&left) + tui_visible_width(right));
    let key_hints =
        " Ctrl+L Model  Ctrl+P Next model  Ctrl+Shift+P Prev model  Shift+Tab Thinking ";
    let mut lines = vec![tui_pad_line(
        &format!(
            "{}{}{}",
            paint(tokens, tokens.dim, &left),
            " ".repeat(gap),
            paint(tokens, tokens.dim, right)
        ),
        width,
    )];
    if state.footer_hotkeys_visible {
        lines.push(tui_pad_line(&paint(tokens, tokens.dim, key_hints), width));
    }
    lines
}

fn render_overlay_lines(
    overlay: &TuiOverlay,
    session: &ShellSession,
    provider: &ProviderConfig,
    width: usize,
    height: usize,
    tokens: ThemeTokens,
) -> Vec<String> {
    match overlay {
        TuiOverlay::Settings { selected, query } => {
            render_settings_overlay(session, provider, *selected, query, width, height, tokens)
        }
        TuiOverlay::SettingsPanel {
            panel,
            selected,
            items,
            ..
        } => render_settings_subpanel_overlay(*panel, items, *selected, width, height, tokens),
        TuiOverlay::Sessions {
            selected,
            query,
            items,
        } => render_sessions_overlay(items, query, *selected, width, height, tokens),
        TuiOverlay::TreeEntries {
            selected,
            query,
            entries,
            branch,
            filter_mode,
            after,
            total_count,
            previous_afters,
        } => render_tree_entries_overlay_with_counts(
            entries,
            branch.as_ref(),
            *filter_mode,
            query,
            *selected,
            width,
            height,
            tokens,
            *after,
            *total_count,
            tree_entry_picker_previous_window_after(previous_afters).is_some(),
        ),
        TuiOverlay::TreeEntryLabelInput {
            event_id,
            input,
            preview,
        } => render_tree_entry_label_input_overlay(*event_id, input, preview, width, tokens),
        TuiOverlay::TreeEntryTextInput {
            action,
            event_id,
            input,
            preview,
        } => {
            render_tree_entry_text_input_overlay(*action, *event_id, input, preview, width, tokens)
        }
        TuiOverlay::Effort {
            selected,
            allowed,
            current,
        } => render_effort_overlay(provider, allowed, *current, *selected, width, tokens),
        TuiOverlay::Models {
            selected,
            query,
            scope,
            items,
            scoped_items,
            ..
        } => render_models_overlay(
            items,
            query,
            *selected,
            width,
            height,
            tokens,
            *scope,
            !scoped_items.is_empty(),
        ),
        TuiOverlay::SandboxDirectories {
            selected,
            query,
            kind,
            edit,
            cwd,
            items,
        } => render_sandbox_directories_overlay(
            items, query, *selected, *kind, *edit, cwd, width, height, tokens,
        ),
        TuiOverlay::SandboxRootEntries {
            selected,
            query,
            kind,
            items,
            selected_roots,
        } => render_sandbox_root_entries_overlay(
            items,
            query,
            *selected,
            *kind,
            selected_roots,
            width,
            height,
            tokens,
        ),
    }
}

fn tui_rule(width: usize, tokens: ThemeTokens) -> String {
    paint(tokens, tokens.dim, &"─".repeat(width))
}

fn render_sessions_overlay(
    items: &[SessionPickerItem],
    query: &str,
    selected: usize,
    width: usize,
    height: usize,
    tokens: ThemeTokens,
) -> Vec<String> {
    let filtered = filtered_session_items(items, query);
    let count = filtered.len();
    let selected = selected.min(count.saturating_sub(1));
    let max_rows = height.saturating_sub(10).clamp(3, 10).min(count.max(1));
    let start = selected.saturating_add(1).saturating_sub(max_rows);
    let end = (start + max_rows).min(count);
    let mut lines = vec![tui_rule(width, tokens)];
    lines.push(tui_pad_line(
        &paint(tokens, tokens.accent, "Sessions"),
        width,
    ));
    let search = if query.trim().is_empty() {
        "Search sessions by title, id, path, or status".to_string()
    } else {
        format!("Search: {query}")
    };
    lines.push(tui_pad_line(&paint(tokens, tokens.dim, &search), width));
    if filtered.is_empty() {
        lines.push(tui_pad_line(
            &paint(tokens, tokens.warning, "No matching sessions"),
            width,
        ));
    } else {
        for (row, (_, item)) in filtered
            .iter()
            .enumerate()
            .skip(start)
            .take(end.saturating_sub(start))
        {
            let marker = if row == selected { "→" } else { " " };
            let current = if item.current { "●" } else { " " };
            let title = if row == selected {
                paint(tokens, tokens.accent, &item.title)
            } else {
                item.title.clone()
            };
            let fork = item
                .forked_from
                .as_deref()
                .map(|fork| format!(" forked={fork}"))
                .unwrap_or_default();
            let row_text = format!(
                "{marker} {current} {title}  {}  {}{}",
                item.status, item.cwd, fork
            );
            lines.push(tui_pad_line(&tui_truncate(&row_text, width), width));
        }
    }
    lines.push(tui_pad_line(
        &paint(
            tokens,
            tokens.dim,
            &format!(
                "{}-{} of {} · Enter to select · Esc to go back",
                start + 1,
                end,
                count
            ),
        ),
        width,
    ));
    lines.push(tui_rule(width, tokens));
    lines
}

fn render_tree_entries_overlay(
    entries: &[ThreadItemEntry],
    branch: Option<&ThreadItemBranch>,
    filter_mode: TreeFilterMode,
    query: &str,
    selected: usize,
    width: usize,
    height: usize,
    tokens: ThemeTokens,
) -> Vec<String> {
    render_tree_entries_overlay_with_counts(
        entries,
        branch,
        filter_mode,
        query,
        selected,
        width,
        height,
        tokens,
        0,
        entries.len() as u64,
        false,
    )
}

fn render_tree_entry_label_input_overlay(
    event_id: u64,
    input: &str,
    preview: &str,
    width: usize,
    tokens: ThemeTokens,
) -> Vec<String> {
    let mut lines = vec![tui_rule(width, tokens)];
    lines.push(tui_pad_line(
        &paint(tokens, tokens.accent, &format!("Label event {event_id}")),
        width,
    ));
    let preview = if preview.trim().is_empty() {
        "No preview"
    } else {
        preview
    };
    lines.push(tui_pad_line(
        &paint(tokens, tokens.dim, &tui_truncate(preview, width)),
        width,
    ));
    let labels = if input.is_empty() { " " } else { input };
    lines.push(tui_pad_line(&format!("Labels: {labels}"), width));
    lines.push(tui_pad_line(
        &paint(
            tokens,
            tokens.dim,
            "Comma-separated labels · blank clears · Enter save · Esc cancel",
        ),
        width,
    ));
    lines.push(tui_rule(width, tokens));
    lines
}

fn render_tree_entry_text_input_overlay(
    action: TreeEntryTextInputAction,
    event_id: u64,
    input: &str,
    preview: &str,
    width: usize,
    tokens: ThemeTokens,
) -> Vec<String> {
    let mut lines = vec![tui_rule(width, tokens)];
    lines.push(tui_pad_line(
        &paint(
            tokens,
            tokens.accent,
            &format!("{} event {event_id}", tree_entry_text_input_title(action)),
        ),
        width,
    ));
    let preview = if preview.trim().is_empty() {
        "No preview"
    } else {
        preview
    };
    lines.push(tui_pad_line(
        &paint(tokens, tokens.dim, &tui_truncate(preview, width)),
        width,
    ));
    let text = if input.is_empty() { " " } else { input };
    lines.push(tui_pad_line(&format!("Text: {text}"), width));
    lines.push(tui_pad_line(
        &paint(
            tokens,
            tokens.dim,
            "Edit replacement text · Enter submit · blank stays open · Esc cancel",
        ),
        width,
    ));
    lines.push(tui_rule(width, tokens));
    lines
}

fn render_tree_entries_overlay_with_counts(
    entries: &[ThreadItemEntry],
    branch: Option<&ThreadItemBranch>,
    filter_mode: TreeFilterMode,
    query: &str,
    selected: usize,
    width: usize,
    height: usize,
    tokens: ThemeTokens,
    after: u64,
    total_count: u64,
    has_previous_window: bool,
) -> Vec<String> {
    let filtered = filtered_tree_entry_items(entries, query);
    let count = filtered.len();
    let selected = selected.min(count.saturating_sub(1));
    let max_rows = height.saturating_sub(11).clamp(3, 10).min(count.max(1));
    let start = selected.saturating_add(1).saturating_sub(max_rows);
    let end = (start + max_rows).min(count);
    let mut lines = vec![tui_rule(width, tokens)];
    lines.push(tui_pad_line(
        &paint(tokens, tokens.accent, "Transcript entries"),
        width,
    ));
    let search = if query.trim().is_empty() {
        format!(
            "Filter {} · search event, label, kind, or text",
            filter_mode.as_str()
        )
    } else {
        format!("Filter {} · Search: {query}", filter_mode.as_str())
    };
    lines.push(tui_pad_line(&paint(tokens, tokens.dim, &search), width));
    if let Some(branch) = branch {
        lines.push(tui_pad_line(
            &paint(tokens, tokens.dim, &format_thread_item_branch(branch)),
            width,
        ));
    }
    if filtered.is_empty() {
        lines.push(tui_pad_line(
            &paint(tokens, tokens.warning, "No matching transcript entries"),
            width,
        ));
    } else {
        for (row, (_, entry)) in filtered
            .iter()
            .enumerate()
            .skip(start)
            .take(end.saturating_sub(start))
        {
            let marker = if row == selected { "→" } else { " " };
            let label = tree_entry_picker_label(entry);
            let labels = if entry.labels.is_empty() {
                String::new()
            } else {
                format!(" [{}]", entry.labels.join(","))
            };
            let detail = tree_entry_picker_detail(entry, branch, after, total_count);
            let label = if row == selected {
                paint(tokens, tokens.accent, &label)
            } else {
                label
            };
            let row_text = format!("{marker} {label}{labels}  {detail}");
            lines.push(tui_pad_line(&tui_truncate(&row_text, width), width));
        }
    }
    let next_after = tree_entry_picker_next_window_after(entries, total_count);
    let next = next_after
        .map(|event_id| format!(" · next /tree entries after {event_id}"))
        .unwrap_or_default();
    let window_keys = if has_previous_window || next_after.is_some() {
        " · ←/→ windows"
    } else {
        ""
    };
    let branch_keys = tree_entry_branch_navigation_hint(branch);
    lines.push(tui_pad_line(
        &paint(
            tokens,
            tokens.dim,
            &format!(
                "{}-{} of {} shown/{total_count} total · Enter fork · e edit · r resubmit · l label{}{} · Esc close{}",
                start + 1,
                end,
                count,
                window_keys,
                branch_keys,
                next
            ),
        ),
        width,
    ));
    lines.push(tui_rule(width, tokens));
    lines
}

fn render_effort_overlay(
    provider: &ProviderConfig,
    allowed: &[ThinkingLevel],
    current: ThinkingLevel,
    selected: usize,
    width: usize,
    tokens: ThemeTokens,
) -> Vec<String> {
    let selected = selected.min(allowed.len().saturating_sub(1));
    let level = allowed.get(selected).copied().unwrap_or(ThinkingLevel::Off);
    let recommended = recommended_effort_level_for_provider(provider);
    let model = effort_model_name(provider);
    let percent = if allowed.len() <= 1 {
        0
    } else {
        ((selected * 100) / (allowed.len() - 1)).min(100)
    };
    let panel_width = width.max(1);
    let inner = panel_width.saturating_sub(2).max(1);
    let rail_prefix = "Effort ";
    let bar_width = inner.saturating_sub(18).clamp(10, 52).min(inner);
    let labels = format!(
        "{}{}",
        " ".repeat(tui_visible_width(rail_prefix)),
        effort_progress_labels(provider, allowed, selected, bar_width, tokens)
    );
    let rail = effort_progress_rail(allowed, selected, bar_width, tokens);
    let support = if allowed == [ThinkingLevel::Off] {
        "Current model is not marked reasoning-capable, so the rail is locked to Off.".to_string()
    } else {
        format!(
            "Current model's rail caps at {}. Switch models with /model.",
            effort_level_label_for_provider(
                provider,
                allowed.last().copied().unwrap_or(ThinkingLevel::Off)
            )
        )
    };
    vec![
        paint(
            tokens,
            tokens.accent,
            tui_box_line(
                '╭',
                '─',
                '╮',
                Some(&format!("─ Effort · {model} ")),
                panel_width,
            ),
        ),
        effort_panel_line(
            tokens,
            &format!(
                "{}{}",
                paint(tokens, tokens.dim, "Current effort "),
                paint(
                    tokens,
                    effort_level_color(tokens, current),
                    effort_level_label_for_provider(provider, current)
                )
            ),
            panel_width,
        ),
        effort_panel_line(tokens, &labels, panel_width),
        effort_panel_line(
            tokens,
            &format!(
                "{}{} {}{}{}",
                paint(tokens, tokens.dim, rail_prefix),
                rail,
                paint(
                    tokens,
                    effort_level_color(tokens, level),
                    effort_level_label_for_provider(provider, level)
                ),
                paint(tokens, tokens.dim, format!("  {percent}%")),
                if level == recommended {
                    paint(tokens, tokens.accent, "  ★")
                } else {
                    String::new()
                }
            ),
            panel_width,
        ),
        effort_panel_line(
            tokens,
            &paint(
                tokens,
                effort_level_color(tokens, level),
                level.description(),
            ),
            panel_width,
        ),
        effort_panel_line(tokens, &paint(tokens, tokens.dim, support), panel_width),
        effort_panel_line(
            tokens,
            &format!(
                "{}  {} {}  {} {}  {} {}",
                paint(tokens, tokens.dim, "←/→ or h/l adjust"),
                paint(tokens, tokens.accent, "a"),
                paint(tokens, tokens.dim, "auto"),
                paint(tokens, tokens.accent, "Enter"),
                paint(tokens, tokens.dim, "apply"),
                paint(tokens, tokens.accent, "Esc"),
                paint(tokens, tokens.dim, "cancel")
            ),
            panel_width,
        ),
        paint(
            tokens,
            tokens.accent,
            tui_box_line('╰', '─', '╯', None, panel_width),
        ),
    ]
}

fn effort_level_color(tokens: ThemeTokens, level: ThinkingLevel) -> &'static str {
    match level {
        ThinkingLevel::Off => tokens.dim,
        ThinkingLevel::Minimal => tokens.code,
        ThinkingLevel::Low => tokens.success,
        ThinkingLevel::Medium => tokens.accent,
        ThinkingLevel::High => tokens.warning,
        ThinkingLevel::XHigh => tokens.error,
    }
}

fn effort_panel_line(tokens: ThemeTokens, content: &str, width: usize) -> String {
    if width <= 1 {
        return tui_truncate(content, width);
    }
    let inner = width.saturating_sub(2);
    format!(
        "{}{}{}",
        paint(tokens, tokens.accent, "│"),
        tui_pad_line(content, inner),
        paint(tokens, tokens.accent, "│")
    )
}

fn effort_level_positions(count: usize, width: usize) -> Vec<usize> {
    let size = width.max(1);
    if count <= 1 {
        return vec![0];
    }
    (0..count)
        .map(|index| (index * (size - 1) + (count - 1) / 2) / (count - 1))
        .collect()
}

fn effort_progress_labels(
    provider: &ProviderConfig,
    allowed: &[ThinkingLevel],
    selected: usize,
    width: usize,
    tokens: ThemeTokens,
) -> String {
    let width = width.max(1);
    let positions = effort_level_positions(allowed.len(), width);
    let mut cursor = 0usize;
    let mut output = String::new();
    for (index, level) in allowed.iter().enumerate() {
        let label = effort_level_label_for_provider(provider, *level);
        let label_width = tui_visible_width(label);
        if label_width >= width && index > 0 {
            continue;
        }
        let centered = positions
            .get(index)
            .copied()
            .unwrap_or_default()
            .saturating_sub(label_width / 2);
        let clamped = centered.min(width.saturating_sub(label_width));
        let target = clamped.max(cursor + usize::from(index > 0));
        if target >= width {
            continue;
        }
        output.push_str(&" ".repeat(target.saturating_sub(cursor)));
        let colored = paint(tokens, effort_level_color(tokens, *level), label);
        if index == selected {
            output.push_str(&colored);
        } else {
            output.push_str(&colored);
        }
        cursor = target.saturating_add(label_width);
    }
    tui_pad_line(&output, width)
}

fn effort_level_for_rail_position(
    allowed: &[ThinkingLevel],
    positions: &[usize],
    index: usize,
) -> ThinkingLevel {
    let next = positions.iter().position(|position| index <= *position);
    match next {
        Some(0) | None => allowed.first().copied().unwrap_or(ThinkingLevel::Off),
        Some(next) => allowed
            .get(next)
            .copied()
            .unwrap_or_else(|| allowed.last().copied().unwrap_or(ThinkingLevel::Off)),
    }
}

fn effort_progress_rail(
    allowed: &[ThinkingLevel],
    selected: usize,
    width: usize,
    tokens: ThemeTokens,
) -> String {
    let width = width.max(1);
    let positions = effort_level_positions(allowed.len(), width);
    let selected_position = positions.get(selected).copied().unwrap_or(0);
    let mut output = String::new();
    for index in 0..width {
        let tick = positions.iter().position(|position| *position == index);
        let level = tick
            .and_then(|tick| allowed.get(tick).copied())
            .unwrap_or_else(|| effort_level_for_rail_position(allowed, &positions, index));
        let glyph = match tick {
            Some(tick) if tick == selected => "◆",
            Some(tick) if tick < selected => "●",
            Some(_) => "○",
            None if index < selected_position => "━",
            None => "─",
        };
        output.push_str(&paint(tokens, effort_level_color(tokens, level), glyph));
    }
    output
}

fn render_models_overlay(
    items: &[ModelPickerItem],
    query: &str,
    selected: usize,
    width: usize,
    height: usize,
    tokens: ThemeTokens,
    scope: ModelPickerScope,
    has_scoped_models: bool,
) -> Vec<String> {
    let filtered = filtered_model_items(items, query);
    let count = filtered.len();
    let selected = selected.min(count.saturating_sub(1));
    let chrome_rows = if has_scoped_models { 12 } else { 10 };
    let max_rows = height
        .saturating_sub(chrome_rows)
        .clamp(4, 10)
        .min(count.max(1));
    let start = selected.saturating_add(1).saturating_sub(max_rows);
    let end = (start + max_rows).min(count);
    let mut lines = vec![tui_rule(width, tokens)];
    lines.push(tui_pad_line(
        &paint(tokens, tokens.accent, "Main model"),
        width,
    ));
    if has_scoped_models {
        let all_label = if scope == ModelPickerScope::All {
            paint(tokens, tokens.accent, "all")
        } else {
            paint(tokens, tokens.dim, "all")
        };
        let scoped_label = if scope == ModelPickerScope::Scoped {
            paint(tokens, tokens.accent, "scoped")
        } else {
            paint(tokens, tokens.dim, "scoped")
        };
        lines.push(tui_pad_line(
            &format!(
                "{}{}{}{}",
                paint(tokens, tokens.dim, "Scope: "),
                all_label,
                paint(tokens, tokens.dim, " | "),
                scoped_label
            ),
            width,
        ));
        lines.push(tui_pad_line(
            &paint(tokens, tokens.dim, "Tab scope (all/scoped)"),
            width,
        ));
    }
    let search = if query.trim().is_empty() {
        "Search main OPPi models".to_string()
    } else {
        format!("Search: {query}")
    };
    lines.push(tui_pad_line(&paint(tokens, tokens.dim, &search), width));
    if filtered.is_empty() {
        lines.push(tui_pad_line(
            &paint(tokens, tokens.warning, "No matching models"),
            width,
        ));
    } else {
        for (row, (_, item)) in filtered
            .iter()
            .enumerate()
            .skip(start)
            .take(end.saturating_sub(start))
        {
            let marker = if row == selected { "→" } else { " " };
            let current = if item.current { "●" } else { " " };
            let label = if row == selected {
                paint(tokens, tokens.accent, &item.label)
            } else {
                item.label.clone()
            };
            let row_text = format!("{marker} {current} {label}  {}", item.detail);
            lines.push(tui_pad_line(&tui_truncate(&row_text, width), width));
        }
    }
    lines.push(tui_pad_line(
        &paint(
            tokens,
            tokens.dim,
            &format!(
                "{}-{} of {} · Enter selects main model · Esc to go back",
                start + 1,
                end,
                count
            ),
        ),
        width,
    ));
    lines.push(tui_rule(width, tokens));
    lines
}

fn render_sandbox_directories_overlay(
    items: &[SandboxDirectoryItem],
    query: &str,
    selected: usize,
    kind: SandboxRootKind,
    edit: SandboxRootEditMode,
    cwd: &Path,
    width: usize,
    height: usize,
    tokens: ThemeTokens,
) -> Vec<String> {
    let filtered = filtered_sandbox_directory_items(items, query);
    let count = filtered.len();
    let selected = selected.min(count.saturating_sub(1));
    let max_rows = height.saturating_sub(11).clamp(4, 10).min(count.max(1));
    let start = selected.saturating_add(1).saturating_sub(max_rows);
    let end = (start + max_rows).min(count);
    let mut lines = vec![tui_rule(width, tokens)];
    lines.push(tui_pad_line(
        &paint(
            tokens,
            tokens.accent,
            &format!(
                "Sandbox {} root {}",
                sandbox_root_kind_label(kind),
                sandbox_root_edit_mode_label(edit)
            ),
        ),
        width,
    ));
    lines.push(tui_pad_line(
        &paint(tokens, tokens.dim, &format!("Current: {}", cwd.display())),
        width,
    ));
    let search = if query.trim().is_empty() {
        "Type to filter directories; Enter opens or chooses selected row".to_string()
    } else {
        format!("Search: {query}")
    };
    lines.push(tui_pad_line(&paint(tokens, tokens.dim, &search), width));
    if filtered.is_empty() {
        lines.push(tui_pad_line(
            &paint(tokens, tokens.warning, "No matching directories"),
            width,
        ));
    } else {
        for (row, (_, item)) in filtered
            .iter()
            .enumerate()
            .skip(start)
            .take(end.saturating_sub(start))
        {
            let marker = if row == selected { "→" } else { " " };
            let label = if row == selected {
                paint(tokens, tokens.accent, &item.label)
            } else {
                item.label.clone()
            };
            let row_text = format!(
                "{marker} {label:<24} {}",
                sandbox_directory_item_value(&item.action)
            );
            lines.push(tui_pad_line(&tui_truncate(&row_text, width), width));
        }
    }
    let detail = filtered
        .get(selected)
        .map(|(_, item)| sandbox_directory_item_detail_for_render(item))
        .unwrap_or_else(|| "No directory selected".to_string());
    lines.push(tui_pad_line(&paint(tokens, tokens.dim, detail), width));
    lines.push(tui_pad_line(
        &paint(
            tokens,
            tokens.dim,
            &format!(
                "{}-{} of {} · Enter open/choose · Backspace/Left parent · Esc back",
                start + 1,
                end,
                count
            ),
        ),
        width,
    ));
    lines.push(tui_rule(width, tokens));
    lines
}

fn render_sandbox_root_entries_overlay(
    items: &[SandboxRootEntryItem],
    query: &str,
    selected: usize,
    kind: SandboxRootKind,
    selected_roots: &[String],
    width: usize,
    height: usize,
    tokens: ThemeTokens,
) -> Vec<String> {
    let filtered = filtered_sandbox_root_entry_items(items, query);
    let all_roots = sandbox_root_entry_configured_roots(items);
    let selected_roots = ordered_sandbox_root_selection(items, selected_roots);
    let count = filtered.len();
    let selected = selected.min(count.saturating_sub(1));
    let max_rows = height.saturating_sub(10).clamp(4, 10).min(count.max(1));
    let start = if count == 0 {
        0
    } else {
        selected.saturating_add(1).saturating_sub(max_rows)
    };
    let end = (start + max_rows).min(count);
    let mut lines = vec![tui_rule(width, tokens)];
    lines.push(tui_pad_line(
        &paint(
            tokens,
            tokens.accent,
            &format!("Sandbox {} root remove", sandbox_root_kind_label(kind)),
        ),
        width,
    ));
    let search = if query.trim().is_empty() {
        "Type to filter configured roots; Space toggles rows; Enter removes selection".to_string()
    } else {
        format!("Search: {query}")
    };
    lines.push(tui_pad_line(&paint(tokens, tokens.dim, &search), width));
    if items.is_empty() {
        lines.push(tui_pad_line(
            &paint(
                tokens,
                tokens.warning,
                &format!(
                    "No configured {} roots to remove",
                    sandbox_root_kind_label(kind)
                ),
            ),
            width,
        ));
    } else if filtered.is_empty() {
        lines.push(tui_pad_line(
            &paint(tokens, tokens.warning, "No matching configured roots"),
            width,
        ));
    } else {
        for (row, (_, item)) in filtered
            .iter()
            .enumerate()
            .skip(start)
            .take(end.saturating_sub(start))
        {
            let marker = if row == selected { "→" } else { " " };
            let item_selected = if item.root == "*" {
                !all_roots.is_empty() && all_roots.iter().all(|root| selected_roots.contains(root))
            } else {
                selected_roots.contains(&item.root)
            };
            let checkbox = if item_selected { "[x]" } else { "[ ]" };
            let label = if row == selected {
                paint(tokens, tokens.accent, &item.label)
            } else {
                item.label.clone()
            };
            let action = if item.root == "*" {
                if item_selected {
                    "all selected"
                } else {
                    "clear all"
                }
            } else if item_selected {
                "selected"
            } else {
                "remove"
            };
            let row_text = format!("{marker} {checkbox} {label:<28} {action}");
            lines.push(tui_pad_line(&tui_truncate(&row_text, width), width));
        }
    }
    let range = if count == 0 {
        "0-0".to_string()
    } else {
        format!("{}-{}", start + 1, end)
    };
    lines.push(tui_pad_line(
        &paint(
            tokens,
            tokens.dim,
            &if selected_roots.is_empty() {
                format!("{range} of {count} · Space select · Enter remove/clear · Esc to go back")
            } else {
                format!(
                    "{range} of {count} · {} selected · Space toggle · Enter remove selected · Esc to go back",
                    selected_roots.len()
                )
            },
        ),
        width,
    ));
    lines.push(tui_rule(width, tokens));
    lines
}

fn render_settings_subpanel_overlay(
    panel: SettingsSubpanel,
    items: &[SettingsPanelItem],
    selected: usize,
    width: usize,
    height: usize,
    tokens: ThemeTokens,
) -> Vec<String> {
    let count = items.len();
    let selected = selected.min(count.saturating_sub(1));
    let row_slots = settings_panel_row_slots(height);
    let visible_rows = row_slots.min(count.max(1));
    let start = selected.saturating_add(1).saturating_sub(visible_rows);
    let end = (start + visible_rows).min(count);
    let mut lines = vec![tui_rule(width, tokens)];
    lines.push(tui_pad_line(
        &paint(
            tokens,
            tokens.accent,
            &format!("Settings › {}", settings_subpanel_title(panel)),
        ),
        width,
    ));
    let mut rendered_rows = 0usize;
    if items.is_empty() {
        lines.push(tui_pad_line(
            &paint(tokens, tokens.warning, "No settings available"),
            width,
        ));
        rendered_rows += 1;
    } else {
        for (row, item) in items
            .iter()
            .enumerate()
            .skip(start)
            .take(end.saturating_sub(start))
        {
            let marker = if row == selected { "→" } else { " " };
            let current = if item.current { "●" } else { " " };
            let label = if row == selected {
                paint(tokens, tokens.accent, &item.label)
            } else {
                item.label.clone()
            };
            let value = settings_panel_item_value_for_render(panel, item, row, selected);
            let row_text = format!("{marker} {current} {label:<24} {value}");
            lines.push(tui_pad_line(&tui_truncate(&row_text, width), width));
            rendered_rows += 1;
        }
    }
    while rendered_rows < row_slots {
        lines.push(tui_pad_line("", width));
        rendered_rows += 1;
    }
    let detail = items
        .get(selected)
        .map(|item| settings_panel_item_detail_for_render(panel, item, selected, selected))
        .unwrap_or_default();
    lines.push(tui_pad_line(&paint(tokens, tokens.dim, detail), width));
    let range = if count == 0 {
        "0-0".to_string()
    } else {
        format!("{}-{}", start + 1, end)
    };
    lines.push(tui_pad_line(
        &paint(
            tokens,
            tokens.dim,
            &settings_panel_footer_hint(panel, &range, count),
        ),
        width,
    ));
    lines.push(tui_rule(width, tokens));
    lines
}

fn settings_panel_row_slots(height: usize) -> usize {
    height.saturating_sub(9).clamp(4, SETTINGS_PANEL_ROW_SLOTS)
}

fn settings_root_item_slots(height: usize) -> usize {
    height.saturating_sub(10).clamp(2, SETTINGS_ROOT_ITEM_SLOTS)
}

fn render_settings_overlay(
    session: &ShellSession,
    provider: &ProviderConfig,
    selected: usize,
    query: &str,
    width: usize,
    height: usize,
    tokens: ThemeTokens,
) -> Vec<String> {
    let items = settings_root_items(session, provider);
    let selected = selected.min(items.len().saturating_sub(1));
    let query = query.trim();
    let has_query = !query.is_empty();
    let active_section = items
        .get(selected)
        .map(|item| item.section)
        .unwrap_or("General");
    let mut lines = vec![tui_rule(width, tokens)];
    lines.push(tui_pad_line(
        &paint(tokens, tokens.accent, "OPPi Settings"),
        width,
    ));
    let navigation_line = if has_query {
        format!("Search: {query}")
    } else {
        settings_section_bar(&items, active_section)
    };
    lines.push(tui_pad_line(
        &paint(
            tokens,
            tokens.accent,
            &tui_truncate(&navigation_line, width),
        ),
        width,
    ));
    let hint = if has_query {
        "Type to search · Backspace edit · ↑/↓ results · Enter/Space open · Esc close"
    } else {
        "←/→ sections · ↑/↓ settings · Type to search · Enter/Space open · Esc close"
    };
    lines.push(tui_pad_line(&paint(tokens, tokens.dim, hint), width));
    lines.push(tui_pad_line("", width));
    let visible_items = if has_query {
        filtered_settings_root_items(&items, query)
    } else {
        items
            .iter()
            .enumerate()
            .filter(|(_, item)| item.section == active_section)
            .collect::<Vec<_>>()
    };
    let item_slots = settings_root_item_slots(height);
    let selected_visible = visible_items
        .iter()
        .position(|(index, _)| *index == selected)
        .unwrap_or(0);
    let start = selected_visible
        .saturating_add(1)
        .saturating_sub(item_slots);
    let end = (start + item_slots).min(visible_items.len());
    let mut rendered_items = 0usize;
    if visible_items.is_empty() {
        let empty = if has_query {
            format!("  No settings match \"{query}\"")
        } else {
            "  No matching settings".to_string()
        };
        lines.push(tui_pad_line(&paint(tokens, tokens.dim, &empty), width));
        lines.push(tui_pad_line("", width));
        rendered_items += 1;
    }
    for &(index, item) in visible_items
        .iter()
        .skip(start)
        .take(end.saturating_sub(start))
    {
        let marker = if index == selected { "→" } else { " " };
        let base_label = if has_query {
            format!(
                "{} › {}",
                settings_section_display_label(item.section),
                item.label
            )
        } else {
            item.label.to_string()
        };
        let label = if index == selected {
            paint(tokens, tokens.accent, &base_label)
        } else {
            base_label
        };
        let row = format!("{marker} {label} — {}", item.value);
        lines.push(tui_pad_line(&tui_truncate(&row, width), width));
        lines.push(tui_pad_line(
            &paint(tokens, tokens.dim, &format!("    {}", item.detail)),
            width,
        ));
        rendered_items += 1;
    }
    while rendered_items < item_slots {
        lines.push(tui_pad_line("", width));
        lines.push(tui_pad_line("", width));
        rendered_items += 1;
    }
    let range = if visible_items.is_empty() {
        "0-0".to_string()
    } else {
        format!("{}-{}", start + 1, end)
    };
    let summary = if has_query {
        format!(
            "{range} of {} matches ({} settings) · filter: {query}",
            visible_items.len(),
            items.len()
        )
    } else {
        format!(
            "{range} of {} settings · active: {}",
            visible_items.len(),
            settings_section_display_label(active_section)
        )
    };
    lines.push(tui_pad_line(&paint(tokens, tokens.dim, &summary), width));
    lines.push(tui_rule(width, tokens));
    lines
}

fn tui_box_line(left: char, fill: char, right: char, label: Option<&str>, width: usize) -> String {
    let width = width.max(2);
    let inner = width.saturating_sub(2);
    let mut body = fill.to_string().repeat(inner);
    if let Some(label) = label {
        let label = tui_truncate(label, inner);
        let label_width = tui_visible_width(&label);
        if label_width < inner {
            let start = 1.min(inner.saturating_sub(label_width));
            body = format!(
                "{}{}{}",
                fill.to_string().repeat(start),
                label,
                fill.to_string()
                    .repeat(inner.saturating_sub(start + label_width))
            );
        }
    }
    format!("{left}{body}{right}")
}

fn split_first_char(text: &str) -> (Option<&str>, &str) {
    let mut chars = text.char_indices();
    let Some((_, _)) = chars.next() else {
        return (None, "");
    };
    if let Some((next, _)) = chars.next() {
        (Some(&text[..next]), &text[next..])
    } else {
        (Some(text), "")
    }
}

fn tui_pad_line(line: &str, width: usize) -> String {
    let truncated = tui_truncate(line, width);
    let visible = tui_visible_width(&truncated);
    if visible >= width {
        truncated
    } else {
        format!("{}{}", truncated, " ".repeat(width - visible))
    }
}

fn tui_truncate(line: &str, width: usize) -> String {
    if tui_visible_width(line) <= width {
        return line.to_string();
    }
    let limit = width.saturating_sub(1);
    let mut output = String::new();
    let mut visible = 0;
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            output.push(ch);
            for next in chars.by_ref() {
                output.push(next);
                if next.is_ascii_alphabetic() || next == 'm' {
                    break;
                }
            }
            continue;
        }
        if visible >= limit {
            break;
        }
        output.push(ch);
        visible += 1;
    }
    output.push('…');
    if line.contains("\x1b[") {
        output.push_str("\x1b[0m");
    }
    output
}

fn tui_visible_width(line: &str) -> usize {
    let mut visible = 0;
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            for next in chars.by_ref() {
                if next.is_ascii_alphabetic() || next == 'm' {
                    break;
                }
            }
        } else {
            visible += 1;
        }
    }
    visible
}

#[cfg(test)]
mod tests {
    use super::*;
    use oppi_protocol::{AskUserOption, AskUserQuestion};

    #[test]
    fn native_tui_state_exposes_settings_overlay_for_ratatui_renderer() {
        let mut state = NativeTuiState::default();
        assert!(!state.has_settings_overlay());
        assert_eq!(state.settings_overlay_selected(), None);

        state.overlay = Some(TuiOverlay::Settings {
            selected: 2,
            query: String::new(),
        });
        assert!(state.has_settings_overlay());
        assert_eq!(state.settings_overlay_selected(), Some(2));

        state.overlay = Some(TuiOverlay::SettingsPanel {
            panel: SettingsSubpanel::Theme,
            selected: 3,
            items: Vec::new(),
            back_stack: Vec::new(),
        });
        assert!(state.has_settings_overlay());
        assert_eq!(state.settings_overlay_selected(), Some(3));

        state.overlay = Some(TuiOverlay::Models {
            selected: 0,
            query: String::new(),
            scope: ModelPickerScope::All,
            items: Vec::new(),
            all_items: Vec::new(),
            scoped_items: Vec::new(),
        });
        assert!(!state.has_settings_overlay());
        assert_eq!(state.settings_overlay_selected(), None);
    }

    #[test]
    fn tab_character_key_maps_to_tab_completion_input() {
        let tab = event::KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE);
        assert_eq!(editor_input_from_key(tab), Some(EditorInput::Tab));

        let tab_char = event::KeyEvent::new(KeyCode::Char('\t'), KeyModifiers::NONE);
        assert_eq!(editor_input_from_key(tab_char), Some(EditorInput::Tab));
    }

    #[test]
    fn differential_renderer_clear_on_shrink_controls_full_clear() {
        let initial = vec!["one".to_string(), "two".to_string(), "six".to_string()];
        let shrunk = vec!["one".to_string()];

        let mut renderer = DifferentialRenderer::with_clear_on_shrink(false);
        let _ = renderer.render_to_string(&initial);
        let off = renderer.render_to_string(&shrunk);
        assert!(
            !off.contains("\x1b[J"),
            "clearOnShrink=false should avoid clearing stale trailing rows"
        );

        let mut renderer = DifferentialRenderer::with_clear_on_shrink(true);
        let _ = renderer.render_to_string(&initial);
        let on = renderer.render_to_string(&shrunk);
        assert!(
            on.contains("\x1b[J"),
            "clearOnShrink=true should request Pi-style stale-row cleanup"
        );
    }

    #[test]
    fn alt_k_toggles_footer_help_before_editor_text_input() {
        let mut state = NativeTuiState::default();
        state.editor.replace_buffer(String::new());
        let alt_k = event::KeyEvent::new(KeyCode::Char('k'), KeyModifiers::ALT);

        assert!(state.footer_hotkeys_visible);
        assert!(state.handle_chrome_key(alt_k));
        assert!(!state.footer_hotkeys_visible);
        assert_eq!(state.editor.buffer_preview(), "");

        assert!(state.handle_chrome_key(event::KeyEvent::new(
            KeyCode::Char('K'),
            KeyModifiers::ALT | KeyModifiers::SHIFT,
        )));
        assert!(state.footer_hotkeys_visible);
        assert_eq!(state.editor.buffer_preview(), "");
    }

    #[test]
    fn ctrl_c_detection_catches_armed_uppercase_and_lowercase_exit_keys() {
        let lower = event::KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        let upper = event::KeyEvent::new(
            KeyCode::Char('C'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        );
        assert!(is_ctrl_c_key(lower));
        assert!(is_ctrl_c_key(upper));

        let mut editor = LineEditor::default();
        editor.arm_ctrl_c_exit();
        assert!(editor.ctrl_c_exit_armed());
        assert_eq!(
            editor.handle(EditorInput::CtrlC),
            EditorAction::Submit("/exit".to_string())
        );
    }

    #[test]
    fn slash_palette_renders_scrollable_command_list() {
        let palette = slash_palette_for_buffer("/", 20).expect("slash palette");
        let lines = render_slash_palette(&palette, 72, 5, theme_tokens("plain"));
        assert!(lines.iter().any(|line| line.contains("/")));
        assert!(lines.iter().any(|line| line.contains("of")));
        assert!(lines.iter().any(|line| line.contains("Enter")));
        assert!(lines.len() <= 6);
        for line in lines {
            assert!(tui_visible_width(&line) <= 72, "too wide: {line}");
        }
    }

    #[test]
    fn slash_palette_respects_pi_autocomplete_max_visible_above_default() {
        let palette = slash_palette_for_buffer("/", 0).expect("slash palette");
        let lines = render_slash_palette(&palette, 96, 10, theme_tokens("plain"));
        assert!(
            lines.iter().any(|line| line.contains("1-10 of")),
            "expected ten command rows plus range footer, got: {lines:?}"
        );
        assert!(lines.len() >= 11, "expected at least ten rows: {lines:?}");
        for line in lines {
            assert!(tui_visible_width(&line) <= 96, "too wide: {line}");
        }
    }

    #[test]
    fn retained_editor_buffer_respects_pi_editor_padding_x() {
        let mut editor = LineEditor::default();
        assert_eq!(
            render_editor_buffer(&editor, 40, 2, theme_tokens("plain")),
            vec!["  \x1b[7m \x1b[27m".to_string()]
        );

        assert_eq!(
            editor.handle(EditorInput::Text("hello".to_string())),
            EditorAction::None
        );
        let lines = render_editor_buffer(&editor, 40, 2, theme_tokens("plain"));
        assert_eq!(lines[0], "  hello\x1b[7m \x1b[27m");
    }

    #[test]
    fn effort_overlay_renders_model_aware_slider_width_safe() {
        let provider = ProviderConfig::OpenAiCompatible(OpenAiCompatibleConfig {
            flavor: DirectProviderFlavor::OpenAiCodex,
            provider_id: None,
            model: "gpt-5.4".to_string(),
            base_url: None,
            api_key_env: None,
            system_prompt: None,
            temperature: None,
            reasoning_effort: Some("medium".to_string()),
            max_output_tokens: None,
            stream: true,
        });
        let allowed = allowed_effort_levels_for_provider(&provider);
        let lines = render_effort_overlay(
            &provider,
            &allowed,
            current_effort_level_for_provider(&provider),
            allowed
                .iter()
                .position(|level| *level == ThinkingLevel::High)
                .unwrap(),
            82,
            theme_tokens("plain"),
        );
        assert!(
            lines
                .iter()
                .any(|line| line.contains("Effort · openai-codex/gpt-5.4"))
        );
        assert!(lines.first().is_some_and(|line| line.starts_with('╭')));
        assert!(lines.last().is_some_and(|line| line.starts_with('╰')));
        assert!(lines.iter().any(|line| line.contains("Current effort")));
        assert!(lines.iter().any(|line| line.contains("XHigh")));
        assert!(lines.iter().any(|line| line.contains("★")));
        assert!(lines.iter().any(|line| line.contains("Enter apply")));
        for line in lines {
            assert!(tui_visible_width(&line) <= 82, "too wide: {line}");
        }
    }

    #[test]
    fn question_answer_choices_include_options_and_custom_path() {
        let request = AskUserRequest {
            id: "ask-1".to_string(),
            title: Some("Choose path".to_string()),
            questions: vec![AskUserQuestion {
                id: "q1".to_string(),
                question: "Proceed?".to_string(),
                options: vec![AskUserOption {
                    id: "safe".to_string(),
                    label: "Safe path".to_string(),
                    description: Some("recommended".to_string()),
                }],
                allow_custom: Some(true),
                default_option_id: Some("safe".to_string()),
                required: true,
            }],
            tool_call_id: None,
            timeout_ms: DEFAULT_ASK_USER_TIMEOUT_MS,
        };

        let choices = question_answer_choices_for_request(&request);
        assert_eq!(choices.len(), 2);
        assert_eq!(choices[0].label, "safe. Safe path");
        assert_eq!(choices[0].answer.as_deref(), Some("safe"));
        assert_eq!(choices[1].label, "custom answer");
        assert_eq!(choices[1].answer, None);
    }

    #[test]
    fn session_picker_filters_and_renders_width_safe_rows() {
        let items = vec![
            SessionPickerItem {
                id: "thread-1".to_string(),
                title: "Root work".to_string(),
                status: "Active".to_string(),
                cwd: "/repo".to_string(),
                forked_from: None,
                current: false,
            },
            SessionPickerItem {
                id: "thread-2".to_string(),
                title: "Feature branch".to_string(),
                status: "Active".to_string(),
                cwd: "/repo/packages/native".to_string(),
                forked_from: Some("thread-1".to_string()),
                current: true,
            },
        ];
        let filtered = filtered_session_items(&items, "native");
        assert_eq!(filtered.len(), 1);
        assert_eq!(
            selected_session_item(&items, "native", 0).unwrap().id,
            "thread-2"
        );
        let lines = render_sessions_overlay(&items, "native", 0, 74, 20, theme_tokens("plain"));
        assert!(lines.iter().any(|line| line.contains("Sessions")));
        assert!(lines.iter().any(|line| line.contains("Feature branch")));
        assert!(lines.iter().any(|line| line.contains("Enter to select")));
        for line in lines {
            assert!(tui_visible_width(&line) <= 74, "too wide: {line}");
        }
    }

    #[test]
    fn retained_tui_routes_tree_prompt_to_entries_and_sessions_to_sessions() {
        assert_eq!(
            tui_prompt_picker_route("/tree"),
            Some(TuiPromptPickerRoute::TreeEntries)
        );
        assert_eq!(
            tui_prompt_picker_route("/tree entries"),
            Some(TuiPromptPickerRoute::TreeEntries)
        );
        assert_eq!(
            tui_prompt_picker_route("/tree messages"),
            Some(TuiPromptPickerRoute::TreeEntries)
        );
        assert_eq!(
            tui_prompt_picker_route("/sessions"),
            Some(TuiPromptPickerRoute::Sessions)
        );
        assert_eq!(
            tui_prompt_picker_route("/resume"),
            Some(TuiPromptPickerRoute::Sessions)
        );
        assert_eq!(
            tui_prompt_picker_route("/tree sessions"),
            Some(TuiPromptPickerRoute::Sessions)
        );
        assert_eq!(tui_prompt_picker_route("/tree fork 42"), None);
    }

    #[test]
    fn tree_entry_picker_filters_renders_and_builds_action_effects() {
        let entries = vec![
            ThreadItemEntry {
                event_id: 42,
                parent_event_id: None,
                child_event_ids: vec![43],
                completed_at_ms: 123,
                labels: vec!["checkpoint".to_string()],
                item: oppi_protocol::Item {
                    id: "item-1".to_string(),
                    thread_id: "thread-1".to_string(),
                    turn_id: "turn-1".to_string(),
                    kind: ItemKind::UserMessage {
                        text: "Hello native tree".to_string(),
                    },
                },
            },
            ThreadItemEntry {
                event_id: 43,
                parent_event_id: Some(42),
                child_event_ids: Vec::new(),
                completed_at_ms: 124,
                labels: Vec::new(),
                item: oppi_protocol::Item {
                    id: "item-2".to_string(),
                    thread_id: "thread-1".to_string(),
                    turn_id: "turn-1".to_string(),
                    kind: ItemKind::AssistantMessage {
                        text: "Working on it".to_string(),
                    },
                },
            },
        ];
        let branch = ThreadItemBranch {
            thread_id: "thread-1".to_string(),
            parent_thread_id: Some("thread-root".to_string()),
            fork_event_id: Some(7),
            child_thread_ids: vec!["thread-child".to_string()],
            sibling_thread_ids: Vec::new(),
            path: vec!["thread-root".to_string(), "thread-1".to_string()],
            depth: 1,
        };

        let filtered = filtered_tree_entry_items(&entries, "hello");
        assert_eq!(filtered.len(), 1);
        assert_eq!(
            selected_tree_entry_item(&entries, "hello", 0)
                .unwrap()
                .event_id,
            42
        );
        assert_eq!(
            tree_entry_picker_effect(TreeEntryPickerAction::Fork, &entries[0]),
            TreeEntryPickerEffect::RunCommand("/tree fork 42".to_string())
        );
        assert_eq!(
            tree_entry_picker_effect(TreeEntryPickerAction::Edit, &entries[0]),
            TreeEntryPickerEffect::OpenTextInput {
                action: TreeEntryTextInputAction::Edit,
                event_id: 42,
                input: "Hello native tree".to_string(),
                preview: "Hello native tree".to_string(),
            }
        );
        assert_eq!(
            tree_entry_picker_effect(TreeEntryPickerAction::Resubmit, &entries[0]),
            TreeEntryPickerEffect::OpenTextInput {
                action: TreeEntryTextInputAction::Resubmit,
                event_id: 42,
                input: "Hello native tree".to_string(),
                preview: "Hello native tree".to_string(),
            }
        );
        assert_eq!(
            tree_entry_picker_effect(TreeEntryPickerAction::Label, &entries[0]),
            TreeEntryPickerEffect::OpenLabelInput {
                event_id: 42,
                input: "checkpoint".to_string(),
                preview: "Hello native tree".to_string(),
            }
        );

        let lines = render_tree_entries_overlay(
            &entries,
            Some(&branch),
            TreeFilterMode::Default,
            "",
            0,
            80,
            20,
            theme_tokens("plain"),
        );
        assert!(lines.iter().any(|line| line.contains("Transcript entries")));
        assert!(lines.iter().any(|line| line.contains("Hello native tree")));
        assert!(lines.iter().any(|line| line.contains("checkpoint")));
        assert!(lines.iter().any(|line| line.contains("Enter fork")));
        for line in lines {
            assert!(tui_visible_width(&line) <= 80, "too wide: {line}");
        }
    }

    #[test]
    fn tree_entry_text_input_builds_edit_and_resubmit_commands() {
        let entry = ThreadItemEntry {
            event_id: 42,
            parent_event_id: None,
            child_event_ids: Vec::new(),
            completed_at_ms: 123,
            labels: Vec::new(),
            item: oppi_protocol::Item {
                id: "item-1".to_string(),
                thread_id: "thread-1".to_string(),
                turn_id: "turn-1".to_string(),
                kind: ItemKind::UserMessage {
                    text: "Original request".to_string(),
                },
            },
        };

        assert_eq!(
            tree_entry_text_input_initial_value(&entry),
            Some("Original request")
        );
        assert_eq!(
            tree_entry_text_input_command(TreeEntryTextInputAction::Edit, 42, "Better request"),
            Some("/tree edit 42 Better request".to_string())
        );
        assert_eq!(
            tree_entry_text_input_command(TreeEntryTextInputAction::Resubmit, 42, "Try again"),
            Some("/tree resubmit 42 Try again".to_string())
        );
        assert_eq!(
            tree_entry_text_input_command(TreeEntryTextInputAction::Edit, 42, "   "),
            None
        );

        let lines = render_tree_entry_text_input_overlay(
            TreeEntryTextInputAction::Edit,
            42,
            "Better request",
            "Original request",
            88,
            theme_tokens("plain"),
        );
        assert!(lines.iter().any(|line| line.contains("Edit event 42")));
        assert!(lines.iter().any(|line| line.contains("Better request")));
        assert!(lines.iter().any(|line| line.contains("Enter submit")));
        for line in lines {
            assert!(tui_visible_width(&line) <= 88, "too wide: {line}");
        }
    }

    #[test]
    fn tree_entry_label_input_builds_label_and_unlabel_commands() {
        let entry = ThreadItemEntry {
            event_id: 42,
            parent_event_id: None,
            child_event_ids: Vec::new(),
            completed_at_ms: 123,
            labels: vec!["checkpoint".to_string(), "review later".to_string()],
            item: oppi_protocol::Item {
                id: "item-1".to_string(),
                thread_id: "thread-1".to_string(),
                turn_id: "turn-1".to_string(),
                kind: ItemKind::UserMessage {
                    text: "Label this entry".to_string(),
                },
            },
        };

        assert_eq!(
            tree_entry_label_input_initial_value(&entry),
            "checkpoint, review later"
        );
        assert_eq!(
            tree_entry_label_input_command(42, "checkpoint, ship it"),
            "/tree label 42 checkpoint, ship it"
        );
        assert_eq!(
            tree_entry_label_input_command(42, "   "),
            "/tree unlabel 42"
        );

        let lines = render_tree_entry_label_input_overlay(
            42,
            "checkpoint, ship it",
            "Label this entry",
            80,
            theme_tokens("plain"),
        );
        assert!(lines.iter().any(|line| line.contains("Label event 42")));
        assert!(
            lines
                .iter()
                .any(|line| line.contains("checkpoint, ship it"))
        );
        assert!(lines.iter().any(|line| line.contains("Enter save")));
        for line in lines {
            assert!(tui_visible_width(&line) <= 80, "too wide: {line}");
        }
    }

    #[test]
    fn tree_entry_picker_window_navigation_exposes_cursor_keys() {
        let entries = vec![
            ThreadItemEntry {
                event_id: 42,
                parent_event_id: None,
                child_event_ids: vec![43],
                completed_at_ms: 123,
                labels: Vec::new(),
                item: oppi_protocol::Item {
                    id: "item-1".to_string(),
                    thread_id: "thread-1".to_string(),
                    turn_id: "turn-1".to_string(),
                    kind: ItemKind::UserMessage {
                        text: "First window".to_string(),
                    },
                },
            },
            ThreadItemEntry {
                event_id: 43,
                parent_event_id: Some(42),
                child_event_ids: Vec::new(),
                completed_at_ms: 124,
                labels: Vec::new(),
                item: oppi_protocol::Item {
                    id: "item-2".to_string(),
                    thread_id: "thread-1".to_string(),
                    turn_id: "turn-1".to_string(),
                    kind: ItemKind::AssistantMessage {
                        text: "Still in first window".to_string(),
                    },
                },
            },
        ];

        assert_eq!(tree_entry_picker_next_window_after(&entries, 3), Some(43));
        assert_eq!(tree_entry_picker_next_window_after(&entries, 2), None);
        assert_eq!(tree_entry_picker_previous_window_after(&[0, 17]), Some(17));
        assert_eq!(tree_entry_picker_previous_window_after(&[]), None);

        let lines = render_tree_entries_overlay_with_counts(
            &entries,
            None,
            TreeFilterMode::Default,
            "",
            0,
            140,
            20,
            theme_tokens("plain"),
            17,
            3,
            true,
        );
        assert!(lines.iter().any(|line| line.contains("←/→ windows")));
        assert!(
            lines
                .iter()
                .any(|line| line.contains("next /tree entries after 43"))
        );
    }

    #[test]
    fn tree_entry_picker_branch_navigation_targets_and_hints() {
        let branch = ThreadItemBranch {
            thread_id: "thread-current".to_string(),
            parent_thread_id: Some("thread-parent".to_string()),
            fork_event_id: Some(7),
            child_thread_ids: vec!["thread-child".to_string()],
            sibling_thread_ids: vec!["thread-next".to_string(), "thread-previous".to_string()],
            path: vec!["thread-parent".to_string(), "thread-current".to_string()],
            depth: 1,
        };

        assert_eq!(
            tree_entry_branch_navigation_target(Some(&branch), TreeEntryBranchNavigation::Parent),
            Some("thread-parent".to_string())
        );
        assert_eq!(
            tree_entry_branch_navigation_target(Some(&branch), TreeEntryBranchNavigation::Child),
            Some("thread-child".to_string())
        );
        assert_eq!(
            tree_entry_branch_navigation_target(
                Some(&branch),
                TreeEntryBranchNavigation::NextSibling
            ),
            Some("thread-next".to_string())
        );
        assert_eq!(
            tree_entry_branch_navigation_target(
                Some(&branch),
                TreeEntryBranchNavigation::PreviousSibling
            ),
            Some("thread-previous".to_string())
        );
        assert_eq!(
            tree_entry_branch_navigation_target(None, TreeEntryBranchNavigation::Child),
            None
        );

        let entries = vec![ThreadItemEntry {
            event_id: 42,
            parent_event_id: None,
            child_event_ids: Vec::new(),
            completed_at_ms: 123,
            labels: Vec::new(),
            item: oppi_protocol::Item {
                id: "item-1".to_string(),
                thread_id: "thread-current".to_string(),
                turn_id: "turn-1".to_string(),
                kind: ItemKind::UserMessage {
                    text: "Current branch entry".to_string(),
                },
            },
        }];
        let lines = render_tree_entries_overlay_with_counts(
            &entries,
            Some(&branch),
            TreeFilterMode::Default,
            "",
            0,
            140,
            20,
            theme_tokens("plain"),
            0,
            entries.len() as u64,
            false,
        );
        assert!(lines.iter().any(|line| line.contains("{ parent")));
        assert!(lines.iter().any(|line| line.contains("[/] siblings")));
        assert!(lines.iter().any(|line| line.contains("} child")));
    }

    #[test]
    fn model_picker_filters_main_models_and_keeps_roles_out() {
        let items = vec![
            ModelPickerItem {
                label: "gpt-fast".to_string(),
                detail: "Set the main model for OPPi executor turns".to_string(),
                action: ModelPickerAction::SelectSession("gpt-fast".to_string()),
                current: true,
            },
            ModelPickerItem {
                label: "gpt-review".to_string(),
                detail: "Set the main model for OPPi executor turns".to_string(),
                action: ModelPickerAction::SelectSession("gpt-review".to_string()),
                current: false,
            },
        ];
        let filtered = filtered_model_items(&items, "review");
        assert_eq!(filtered.len(), 1);
        assert!(matches!(
            selected_model_item(&items, "gpt-review", 0).unwrap().action,
            ModelPickerAction::SelectSession(_)
        ));
        let lines = render_models_overlay(
            &items,
            "review",
            0,
            78,
            20,
            theme_tokens("plain"),
            ModelPickerScope::All,
            false,
        );
        assert!(lines.iter().any(|line| line.contains("Main model")));
        assert!(lines.iter().any(|line| line.contains("gpt-review")));
        assert!(!lines.iter().any(|line| line.contains("role reviewer")));
        assert!(
            lines
                .iter()
                .any(|line| line.contains("Enter selects main model"))
        );
        for line in lines {
            assert!(tui_visible_width(&line) <= 78, "too wide: {line}");
        }
    }

    #[test]
    fn model_picker_items_from_runtime_refs_preserve_display_labels_and_action_ids() {
        let refs = vec![ModelRef {
            id: "gpt-local-coder".to_string(),
            provider: "openai-codex".to_string(),
            display_name: "Local Coder".to_string(),
            context_window: None,
            role: None,
        }];

        let items = model_picker_items_from_refs(&refs, Some("gpt-local-coder"));

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].label, "Local Coder");
        assert!(items[0].current);
        assert!(matches!(
            &items[0].action,
            ModelPickerAction::SelectSession(model) if model == "gpt-local-coder"
        ));
    }

    fn test_model_picker_item(label: &str, model: &str, current: bool) -> ModelPickerItem {
        ModelPickerItem {
            label: label.to_string(),
            detail: format!("Set {model} for OPPi executor turns"),
            action: ModelPickerAction::SelectSession(model.to_string()),
            current,
        }
    }

    #[test]
    fn model_picker_scope_toggle_switches_between_scoped_and_all_items() {
        let all_items = vec![
            test_model_picker_item("Current", "gpt-current", true),
            test_model_picker_item("Fast", "gpt-fast", false),
        ];
        let scoped_items = vec![test_model_picker_item("Current", "gpt-current", true)];
        let mut overlay = TuiOverlay::Models {
            selected: 0,
            query: "cur".to_string(),
            scope: ModelPickerScope::Scoped,
            items: scoped_items.clone(),
            all_items: all_items.clone(),
            scoped_items: scoped_items.clone(),
        };

        assert!(toggle_model_picker_scope(&mut overlay));
        match &overlay {
            TuiOverlay::Models {
                selected,
                query,
                scope,
                items,
                ..
            } => {
                assert_eq!(*selected, 0);
                assert_eq!(query, "cur");
                assert_eq!(*scope, ModelPickerScope::All);
                assert_eq!(items, &all_items);
            }
            _ => panic!("expected model picker overlay"),
        }

        assert!(toggle_model_picker_scope(&mut overlay));
        match &overlay {
            TuiOverlay::Models { scope, items, .. } => {
                assert_eq!(*scope, ModelPickerScope::Scoped);
                assert_eq!(items, &scoped_items);
            }
            _ => panic!("expected model picker overlay"),
        }
    }

    #[test]
    fn model_picker_render_shows_scope_toggle_when_scoped_models_exist() {
        let all_items = vec![
            test_model_picker_item("Current", "gpt-current", true),
            test_model_picker_item("Fast", "gpt-fast", false),
        ];
        let scoped_items = vec![test_model_picker_item("Current", "gpt-current", true)];

        let lines = render_models_overlay(
            &scoped_items,
            "",
            0,
            78,
            20,
            theme_tokens("plain"),
            ModelPickerScope::Scoped,
            !all_items.is_empty() && !scoped_items.is_empty(),
        );

        assert!(lines.iter().any(|line| line.contains("Scope:")));
        assert!(lines.iter().any(|line| line.contains("all")));
        assert!(lines.iter().any(|line| line.contains("scoped")));
        assert!(lines.iter().any(|line| line.contains("Tab")));
        for line in lines {
            assert!(tui_visible_width(&line) <= 78, "too wide: {line}");
        }
    }

    #[test]
    fn settings_subpanel_actions_route_to_safe_existing_commands() {
        let theme_items = theme_settings_panel_items_for("dark");
        assert!(
            theme_items
                .iter()
                .any(|item| item.current && item.value == "dark")
        );
        assert!(theme_items.iter().any(|item| matches!(
            item.action,
            SettingsPanelAction::Command(ref command) if command == "/theme light"
        )));
        assert!(theme_items.iter().any(|item| matches!(
            item.action,
            SettingsPanelAction::Command(ref command) if command == "/theme reload"
        )));
        assert!(theme_items.iter().any(|item| {
            item.label == "Preview picker"
                && item.value == "open"
                && matches!(
                    item.action,
                    SettingsPanelAction::Command(ref command) if command == "/theme preview"
                )
        }));

        let light_index = theme_items
            .iter()
            .position(|item| item.value == "light")
            .expect("light theme row");
        let state = NativeTuiState {
            overlay: Some(TuiOverlay::SettingsPanel {
                panel: SettingsSubpanel::Theme,
                selected: light_index,
                items: theme_items.clone(),
                back_stack: Vec::new(),
            }),
            ..NativeTuiState::default()
        };
        assert_eq!(theme_name_for_tui_frame("dark", &state), "light");

        let preview_index = theme_items
            .iter()
            .position(|item| item.value == "open")
            .expect("preview picker row");
        let state = NativeTuiState {
            overlay: Some(TuiOverlay::SettingsPanel {
                panel: SettingsSubpanel::Theme,
                selected: preview_index,
                items: theme_items,
                back_stack: Vec::new(),
            }),
            ..NativeTuiState::default()
        };
        assert_eq!(theme_name_for_tui_frame("dark", &state), "dark");

        let footer_items = footer_settings_panel_items_for_settings(&FooterSettings {
            show_path: true,
            show_help_bar: false,
            usage_display: FooterUsageDisplay::Session,
            show_model: true,
            show_permission: false,
            show_memory: true,
            show_context: false,
        });
        assert!(footer_items.iter().any(|item| {
            item.label == "Workspace path"
                && item.value == "shown"
                && matches!(
                    item.action,
                    SettingsPanelAction::Command(ref command) if command == "/footer path off"
                )
        }));
        assert!(footer_items.iter().any(|item| {
            item.label == "Hotkey help"
                && item.value == "hidden"
                && matches!(
                    item.action,
                    SettingsPanelAction::Command(ref command) if command == "/footer help on"
                )
        }));
        assert!(footer_items.iter().any(|item| {
            item.label == "Usage display"
                && item.value == "session"
                && matches!(
                    item.action,
                    SettingsPanelAction::EditCommand(ref command) if command == "/footer usage "
                )
        }));
        assert!(footer_items.iter().any(|item| {
            item.label == "Model chip"
                && item.value == "shown"
                && matches!(
                    item.action,
                    SettingsPanelAction::Command(ref command) if command == "/footer model off"
                )
        }));
        assert!(footer_items.iter().any(|item| {
            item.label == "Permission chip"
                && item.value == "hidden"
                && matches!(
                    item.action,
                    SettingsPanelAction::Command(ref command) if command == "/footer permission on"
                )
        }));
        assert!(footer_items.iter().any(|item| {
            item.label == "Memory chip"
                && item.value == "shown"
                && matches!(
                    item.action,
                    SettingsPanelAction::Command(ref command) if command == "/footer memory off"
                )
        }));
        assert!(footer_items.iter().any(|item| {
            item.label == "Context chip"
                && item.value == "hidden"
                && matches!(
                    item.action,
                    SettingsPanelAction::Command(ref command) if command == "/footer context on"
                )
        }));

        let image_items = image_settings_panel_items_for_settings(&ImageSettings {
            show_images: false,
            image_width_cells: 120,
            auto_resize_images: true,
            block_images: true,
            preview_protocol: ImagePreviewProtocolOverride::Sixel,
        });
        assert!(image_items.iter().any(|item| {
            item.label == "Show images"
                && item.value == "hidden"
                && matches!(
                    item.action,
                    SettingsPanelAction::Command(ref command) if command == "/images show on"
                )
        }));
        assert!(image_items.iter().any(|item| {
            item.label == "Image width"
                && item.value == "120"
                && matches!(
                    item.action,
                    SettingsPanelAction::EditCommand(ref command) if command == "/images width "
                )
        }));
        assert!(image_items.iter().any(|item| {
            item.label == "Auto-resize images"
                && item.value == "enabled"
                && matches!(
                    item.action,
                    SettingsPanelAction::Command(ref command) if command == "/images auto-resize off"
                )
        }));
        assert!(image_items.iter().any(|item| {
            item.label == "Block images"
                && item.value == "blocked"
                && matches!(
                    item.action,
                    SettingsPanelAction::Command(ref command) if command == "/images block off"
                )
        }));
        assert!(image_items.iter().any(|item| {
            item.label == "Preview protocol"
                && item.value == "sixel"
                && matches!(
                    item.action,
                    SettingsPanelAction::EditCommand(ref command) if command == "/images protocol "
                )
        }));
        assert!(image_items.iter().any(|item| {
            item.label == "Preview capability"
                && item.value == "detect"
                && matches!(
                    item.action,
                    SettingsPanelAction::Command(ref command) if command == "/images capabilities"
                )
        }));

        let permission_items = permission_settings_panel_items_for_settings(
            "auto-review",
            &GuardianReviewSettings {
                timeout_ms: DEFAULT_PERMISSION_REVIEW_TIMEOUT_MS,
                reviewer_model: None,
            },
            &PermissionSandboxSettings {
                allow_internet: true,
                allow_ssh: false,
                project_roots: vec!["E:/repo".to_string()],
                readable_roots: vec!["E:/repo".to_string(), "E:/readonly".to_string()],
                writable_roots: vec!["E:/repo/out".to_string()],
                protected_patterns: vec!["secrets/*.json".to_string(), ".env.local".to_string()],
                ..PermissionSandboxSettings::default()
            },
        );
        assert!(
            permission_items
                .iter()
                .any(|item| item.current && item.value == "auto-review")
        );
        assert!(permission_items.iter().any(|item| matches!(
            item.action,
            SettingsPanelAction::Command(ref command) if command == "/permissions full-access"
        )));
        assert!(permission_items.iter().any(|item| {
            item.label == "Review timeout"
                && item.value == "45s"
                && matches!(
                    item.action,
                    SettingsPanelAction::EditCommand(ref command) if command == "/permissions timeout "
                )
        }));
        assert!(permission_items.iter().any(|item| {
            item.label == "Reviewer model"
                && item.value == "auto"
                && matches!(
                    item.action,
                    SettingsPanelAction::EditCommand(ref command) if command == "/permissions reviewer-model "
                )
        }));
        assert!(permission_items.iter().any(|item| {
            item.label == "Validate sandbox"
                && item.value == "check"
                && matches!(
                    item.action,
                    SettingsPanelAction::Command(ref command) if command == "/permissions sandbox validate"
                )
        }));
        assert!(permission_items.iter().any(|item| {
            item.label == "Use workspace roots"
                && item.value == "current"
                && matches!(
                    item.action,
                    SettingsPanelAction::Command(ref command) if command == "/permissions sandbox roots here"
                )
        }));
        assert!(permission_items.iter().any(|item| {
            item.label == "Reset sandbox roots"
                && item.value == "default"
                && matches!(
                    item.action,
                    SettingsPanelAction::Command(ref command) if command == "/permissions sandbox roots default"
                )
        }));
        assert!(permission_items.iter().any(|item| {
            item.label == "Browse project root"
                && item.value == "picker"
                && matches!(
                    item.action,
                    SettingsPanelAction::OpenSandboxDirectoryPicker {
                        kind: SandboxRootKind::Project,
                        edit: SandboxRootEditMode::Set,
                    }
                )
        }));
        assert!(permission_items.iter().any(|item| {
            item.label == "Add project root"
                && item.value == "add"
                && matches!(
                    item.action,
                    SettingsPanelAction::OpenSandboxDirectoryPicker {
                        kind: SandboxRootKind::Project,
                        edit: SandboxRootEditMode::Add,
                    }
                )
        }));
        assert!(permission_items.iter().any(|item| {
            item.label == "Remove project root"
                && item.value == "remove"
                && matches!(
                    item.action,
                    SettingsPanelAction::OpenSandboxRootRemovePicker(SandboxRootKind::Project)
                )
        }));
        assert!(permission_items.iter().any(|item| {
            item.label == "Browse readable root"
                && item.value == "picker"
                && matches!(
                    item.action,
                    SettingsPanelAction::OpenSandboxDirectoryPicker {
                        kind: SandboxRootKind::Read,
                        edit: SandboxRootEditMode::Set,
                    }
                )
        }));
        assert!(permission_items.iter().any(|item| {
            item.label == "Add readable root"
                && item.value == "add"
                && matches!(
                    item.action,
                    SettingsPanelAction::OpenSandboxDirectoryPicker {
                        kind: SandboxRootKind::Read,
                        edit: SandboxRootEditMode::Add,
                    }
                )
        }));
        assert!(permission_items.iter().any(|item| {
            item.label == "Remove readable root"
                && item.value == "remove"
                && matches!(
                    item.action,
                    SettingsPanelAction::OpenSandboxRootRemovePicker(SandboxRootKind::Read)
                )
        }));
        assert!(permission_items.iter().any(|item| {
            item.label == "Browse writable root"
                && item.value == "picker"
                && matches!(
                    item.action,
                    SettingsPanelAction::OpenSandboxDirectoryPicker {
                        kind: SandboxRootKind::Write,
                        edit: SandboxRootEditMode::Set,
                    }
                )
        }));
        assert!(permission_items.iter().any(|item| {
            item.label == "Add writable root"
                && item.value == "add"
                && matches!(
                    item.action,
                    SettingsPanelAction::OpenSandboxDirectoryPicker {
                        kind: SandboxRootKind::Write,
                        edit: SandboxRootEditMode::Add,
                    }
                )
        }));
        assert!(permission_items.iter().any(|item| {
            item.label == "Remove writable root"
                && item.value == "remove"
                && matches!(
                    item.action,
                    SettingsPanelAction::OpenSandboxRootRemovePicker(SandboxRootKind::Write)
                )
        }));
        assert!(permission_items.iter().any(|item| {
            item.label == "Protected patterns"
                && item.value == "secrets/*.json, .env.local"
                && matches!(
                    item.action,
                    SettingsPanelAction::EditCommand(ref command) if command == "/permissions sandbox protected secrets/*.json, .env.local"
                )
        }));
        assert!(permission_items.iter().any(|item| {
            item.label == "Sandbox internet"
                && item.value == "allowed"
                && matches!(
                    item.action,
                    SettingsPanelAction::Command(ref command) if command == "/permissions sandbox internet off"
                )
        }));
        assert!(permission_items.iter().any(|item| {
            item.label == "Sandbox SSH"
                && item.value == "blocked"
                && matches!(
                    item.action,
                    SettingsPanelAction::Command(ref command) if command == "/permissions sandbox ssh on"
                )
        }));
        assert!(permission_items.iter().any(|item| {
            item.label == "Project roots"
                && item.value == "E:/repo"
                && matches!(
                    item.action,
                    SettingsPanelAction::EditCommand(ref command) if command == "/permissions sandbox project E:/repo"
                )
        }));
        assert!(permission_items.iter().any(|item| {
            item.label == "Readable roots"
                && item.value == "E:/repo, E:/readonly"
                && matches!(
                    item.action,
                    SettingsPanelAction::EditCommand(ref command) if command == "/permissions sandbox read E:/repo, E:/readonly"
                )
        }));
        assert!(permission_items.iter().any(|item| {
            item.label == "Writable roots"
                && item.value == "E:/repo/out"
                && matches!(
                    item.action,
                    SettingsPanelAction::EditCommand(ref command) if command == "/permissions sandbox write E:/repo/out"
                )
        }));

        let root = env::temp_dir().join(format!(
            "oppi-sandbox-directory-picker-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("alpha")).unwrap();
        fs::create_dir_all(root.join("beta")).unwrap();
        fs::write(root.join("note.txt"), b"not a directory").unwrap();
        let items = sandbox_directory_picker_items(&root).unwrap();
        assert!(
            items.iter().any(|item| {
                item.label == "alpha" && item.detail == "directory" && item.path.ends_with("alpha")
            }),
            "{items:?}"
        );
        assert!(
            items.iter().any(|item| {
                item.label == "beta" && item.detail == "directory" && item.path.ends_with("beta")
            }),
            "{items:?}"
        );
        assert!(
            !items.iter().any(|item| item.label == "note.txt"),
            "{items:?}"
        );
        let rendered_directory_picker = render_sandbox_directories_overlay(
            &items,
            "",
            0,
            SandboxRootKind::Project,
            SandboxRootEditMode::Set,
            &root,
            96,
            18,
            theme_tokens("plain"),
        )
        .join("\n");
        assert!(
            rendered_directory_picker.contains("Choose current directory:"),
            "{rendered_directory_picker}"
        );
        assert!(
            rendered_directory_picker.contains("Backspace/Left parent"),
            "{rendered_directory_picker}"
        );
        let (parent_path, parent_items) = sandbox_directory_parent_items(&root.join("alpha"))
            .unwrap()
            .expect("child directory should have parent picker items");
        assert_eq!(parent_path, root);
        assert!(
            parent_items
                .iter()
                .any(|item| item.label == "Use this directory"),
            "{parent_items:?}"
        );
        assert_eq!(
            sandbox_directory_picker_command(
                SandboxRootKind::Project,
                SandboxRootEditMode::Set,
                &root.join("alpha")
            ),
            format!(
                "/permissions sandbox project {}",
                root.join("alpha").display()
            )
        );
        assert_eq!(
            sandbox_directory_picker_command(
                SandboxRootKind::Read,
                SandboxRootEditMode::Set,
                &root.join("beta")
            ),
            format!("/permissions sandbox read {}", root.join("beta").display())
        );
        assert_eq!(
            sandbox_directory_picker_command(
                SandboxRootKind::Write,
                SandboxRootEditMode::Set,
                &root.join("beta")
            ),
            format!("/permissions sandbox write {}", root.join("beta").display())
        );
        assert_eq!(
            sandbox_directory_picker_command(
                SandboxRootKind::Project,
                SandboxRootEditMode::Add,
                &root.join("alpha")
            ),
            format!(
                "/permissions sandbox project add {}",
                root.join("alpha").display()
            )
        );
        assert_eq!(
            sandbox_directory_picker_command(
                SandboxRootKind::Read,
                SandboxRootEditMode::Add,
                &root.join("beta")
            ),
            format!(
                "/permissions sandbox read add {}",
                root.join("beta").display()
            )
        );
        assert_eq!(
            sandbox_directory_picker_command(
                SandboxRootKind::Write,
                SandboxRootEditMode::Add,
                &root.join("beta")
            ),
            format!(
                "/permissions sandbox write add {}",
                root.join("beta").display()
            )
        );
        let remove_items = sandbox_root_remove_picker_items(&[
            "E:/repo".to_string(),
            "".to_string(),
            "E:/repo".to_string(),
            "E:/readonly".to_string(),
        ]);
        assert_eq!(
            remove_items
                .iter()
                .map(|item| item.root.as_str())
                .collect::<Vec<_>>(),
            vec!["*", "E:/repo", "E:/readonly"]
        );
        assert_eq!(remove_items[0].label, "All configured roots");
        assert_eq!(remove_items[0].detail, "clear all configured roots");
        assert_eq!(remove_items[1].label, "E:/repo");
        assert_eq!(remove_items[1].detail, "configured root");
        assert_eq!(
            selected_sandbox_root_entry_item(&remove_items, "all", 0)
                .unwrap()
                .root,
            "*"
        );
        assert_eq!(
            selected_sandbox_root_entry_item(&remove_items, "readonly", 0)
                .unwrap()
                .root,
            "E:/readonly"
        );
        assert_eq!(
            sandbox_root_entry_command(SandboxRootKind::Project, &remove_items[0]),
            "/permissions sandbox project default"
        );
        assert_eq!(
            sandbox_root_entry_command(SandboxRootKind::Project, &remove_items[1]),
            "/permissions sandbox project remove E:/repo"
        );
        let selected_roots =
            toggle_sandbox_root_entry_selection(&remove_items, &[], &remove_items[1]);
        assert_eq!(selected_roots, vec!["E:/repo".to_string()]);
        let selected_roots =
            toggle_sandbox_root_entry_selection(&remove_items, &selected_roots, &remove_items[2]);
        assert_eq!(
            selected_roots,
            vec!["E:/repo".to_string(), "E:/readonly".to_string()]
        );
        assert_eq!(
            sandbox_root_entries_command(
                SandboxRootKind::Project,
                &remove_items[1],
                &selected_roots
            ),
            "/permissions sandbox project remove E:/repo,E:/readonly"
        );
        assert_eq!(
            sandbox_root_entries_command(
                SandboxRootKind::Project,
                &remove_items[0],
                &selected_roots
            ),
            "/permissions sandbox project default"
        );
        let all_selected =
            toggle_sandbox_root_entry_selection(&remove_items, &[], &remove_items[0]);
        assert_eq!(
            all_selected,
            vec!["E:/repo".to_string(), "E:/readonly".to_string()]
        );
        let cleared =
            toggle_sandbox_root_entry_selection(&remove_items, &all_selected, &remove_items[0]);
        assert!(cleared.is_empty());
        let read_item = SandboxRootEntryItem {
            label: "E:/readonly".to_string(),
            detail: "configured root".to_string(),
            root: "E:/readonly".to_string(),
        };
        let write_item = SandboxRootEntryItem {
            label: "E:/repo/out".to_string(),
            detail: "configured root".to_string(),
            root: "E:/repo/out".to_string(),
        };
        assert_eq!(
            sandbox_root_entry_command(SandboxRootKind::Read, &read_item),
            "/permissions sandbox read remove E:/readonly"
        );
        assert_eq!(
            sandbox_root_entry_command(SandboxRootKind::Write, &write_item),
            "/permissions sandbox write remove E:/repo/out"
        );
        let _ = fs::remove_dir_all(&root);

        let provider_items = provider_settings_panel_items(&ProviderConfig::Mock);
        assert!(provider_items.iter().any(|item| matches!(
            item.action,
            SettingsPanelAction::Command(ref command) if command == "/provider validate"
        )));
        assert!(provider_items.iter().any(|item| matches!(
            item.action,
            SettingsPanelAction::EditCommand(ref command) if command == "/login api openai env "
        )));

        let login_items = login_settings_panel_items(&ProviderConfig::Mock);
        assert!(login_items.iter().any(|item| matches!(
            item.action,
            SettingsPanelAction::OpenPanel(SettingsSubpanel::LoginSubscription)
        )));
        let login_api_items = login_api_settings_panel_items(&ProviderConfig::Mock);
        assert!(login_api_items.iter().any(|item| matches!(
            item.action,
            SettingsPanelAction::Command(ref command) if command == "/login api openai env OPPI_OPENAI_API_KEY"
        )));
        assert!(login_api_items.iter().all(|item| match &item.action {
            SettingsPanelAction::Command(command) | SettingsPanelAction::EditCommand(command) => {
                !command.contains("sk-") && !command.contains("Bearer ")
            }
            SettingsPanelAction::OpenPanel(_)
            | SettingsPanelAction::OpenSandboxDirectoryPicker { .. }
            | SettingsPanelAction::OpenSandboxRootRemovePicker(_)
            | SettingsPanelAction::Model(_) => true,
        }));

        let memory_items = memory_settings_panel_items_for_settings(&MemorySettings {
            enabled: true,
            agent_model: "gpt".to_string(),
            deep_model: "sonnet".to_string(),
            startup_recall: false,
            task_start_recall: true,
            turn_summaries: false,
            idle_consolidation: false,
            hoppi_install_offer: "dismissed".to_string(),
            sync_enabled: true,
            ..MemorySettings::default()
        });
        assert!(memory_items.iter().any(|item| matches!(
            item.action,
            SettingsPanelAction::Command(ref command) if command == "/memory maintenance apply"
        )));
        assert!(
            memory_items
                .iter()
                .any(|item| { item.label == "Memory" && item.detail.contains("runtime-backed") })
        );
        assert!(memory_items.iter().any(|item| matches!(
            item.action,
            SettingsPanelAction::Command(ref command) if command == "/idle-compact compact"
        )));
        assert!(memory_items.iter().any(|item| {
            item.label == "Backend package"
                && item.value == "@oppiai/hoppi-memory"
                && matches!(
                    item.action,
                    SettingsPanelAction::Command(ref command) if command == "/memory install-hoppi"
                )
        }));
        assert!(memory_items.iter().any(|item| {
            item.label == "Startup recall"
                && item.value == "disabled"
                && matches!(
                    item.action,
                    SettingsPanelAction::Command(ref command) if command == "/memory startup-recall on"
                )
        }));
        assert!(memory_items.iter().any(|item| {
            item.label == "Task-start recall"
                && item.value == "enabled"
                && matches!(
                    item.action,
                    SettingsPanelAction::Command(ref command) if command == "/memory task-start-recall off"
                )
        }));
        assert!(memory_items.iter().any(|item| {
            item.label == "Turn summaries"
                && item.value == "disabled"
                && matches!(
                    item.action,
                    SettingsPanelAction::Command(ref command) if command == "/memory turn-summaries on"
                )
        }));
        assert!(memory_items.iter().any(|item| {
            item.label == "Memory agent model"
                && item.value == "gpt"
                && matches!(
                    item.action,
                    SettingsPanelAction::EditCommand(ref command) if command == "/memory agent-model "
                )
        }));
        assert!(memory_items.iter().any(|item| {
            item.label == "Deep dream model"
                && item.value == "sonnet"
                && matches!(
                    item.action,
                    SettingsPanelAction::EditCommand(ref command) if command == "/memory deep-model "
                )
        }));
        assert!(memory_items.iter().any(|item| {
            item.label == "Idle dream mode"
                && item.value == "disabled"
                && matches!(
                    item.action,
                    SettingsPanelAction::Command(ref command) if command == "/memory idle-dream on"
                )
        }));
        assert!(memory_items.iter().any(|item| {
            item.label == "First-start install offer"
                && item.value == "dismissed"
                && matches!(
                    item.action,
                    SettingsPanelAction::Command(ref command) if command == "/memory install-offer ask"
                )
        }));
        assert!(memory_items.iter().any(|item| {
            item.label == "Sync"
                && item.value == "enabled"
                && matches!(
                    item.action,
                    SettingsPanelAction::Command(ref command) if command == "/memory sync off"
                )
        }));
        assert!(memory_items.iter().any(|item| {
            item.label == "Sync repo"
                && item.value == "not configured"
                && matches!(
                    item.action,
                    SettingsPanelAction::EditCommand(ref command) if command == "/memory sync repo "
                )
        }));
        assert!(memory_items.iter().any(|item| {
            item.label == "Sync remote"
                && item.value == "not configured"
                && matches!(
                    item.action,
                    SettingsPanelAction::EditCommand(ref command) if command == "/memory sync remote "
                )
        }));
        assert!(memory_items.iter().any(|item| {
            item.label == "Sync background"
                && item.value == "off"
                && matches!(
                    item.action,
                    SettingsPanelAction::EditCommand(ref command) if command == "/memory sync background "
                )
        }));
        assert!(memory_items.iter().any(|item| {
            item.label == "Sync encryption"
                && item.value == "none/env"
                && matches!(
                    item.action,
                    SettingsPanelAction::EditCommand(ref command) if command == "/memory sync encryption "
                )
        }));
        assert!(memory_items.iter().any(|item| {
            item.label == "Sync conflict mode"
                && item.value == "keep-both"
                && matches!(
                    item.action,
                    SettingsPanelAction::EditCommand(ref command) if command == "/memory sync conflict "
                )
        }));
        assert!(memory_items.iter().any(|item| {
            item.label == "Sync now"
                && item.value == "manual"
                && matches!(
                    item.action,
                    SettingsPanelAction::Command(ref command) if command == "/memory sync now"
                )
        }));
        assert!(memory_items.iter().any(|item| {
            item.label == "Pull now"
                && item.value == "manual"
                && matches!(
                    item.action,
                    SettingsPanelAction::Command(ref command) if command == "/memory sync pull"
                )
        }));
        assert!(memory_items.iter().any(|item| {
            item.label == "Push now"
                && item.value == "manual"
                && matches!(
                    item.action,
                    SettingsPanelAction::Command(ref command) if command == "/memory sync push"
                )
        }));

        let compaction_items = compaction_settings_panel_items_for_settings(&OppiCompactSettings {
            idle_enabled: false,
            idle_minutes: 10,
            idle_threshold_percent: 80,
            smart_threshold_percent: 55,
        });
        assert!(compaction_items.iter().any(|item| {
            item.label == "Idle compaction"
                && item.value == "disabled"
                && matches!(
                    item.action,
                    SettingsPanelAction::Command(ref command) if command == "/idle-compact on"
                )
        }));
        assert!(compaction_items.iter().any(|item| {
            item.label == "Idle time"
                && item.value == "10m"
                && matches!(
                    item.action,
                    SettingsPanelAction::EditCommand(ref command) if command == "/idle-compact idle-time "
                )
        }));
        assert!(compaction_items.iter().any(|item| {
            item.label == "Idle context threshold"
                && item.value == "80%"
                && matches!(
                    item.action,
                    SettingsPanelAction::EditCommand(ref command) if command == "/idle-compact idle-threshold "
                )
        }));
        assert!(compaction_items.iter().any(|item| {
            item.label == "Smart compact threshold"
                && item.value == "55%"
                && matches!(
                    item.action,
                    SettingsPanelAction::EditCommand(ref command) if command == "/idle-compact smart-threshold "
                )
        }));
        assert!(compaction_items.iter().any(|item| {
            item.label == "Compact now"
                && item.value == "handoff"
                && matches!(
                    item.action,
                    SettingsPanelAction::Command(ref command) if command == "/idle-compact compact"
                )
        }));
    }

    #[test]
    fn theme_settings_panel_includes_discovered_themes_for_live_preview() {
        let theme_items = theme_settings_panel_items_for_list(
            "oppi",
            Some(&json!({
                "items": [
                    { "active": { "name": "oppi", "source": "builtIn" } },
                    { "active": { "name": "solarized-pro", "source": "plugin", "path": "plugins/theme-pack/themes/solarized-pro.json" } }
                ]
            })),
        );
        let plugin_index = theme_items
            .iter()
            .position(|item| item.label == "solarized-pro")
            .expect("plugin theme row");
        let plugin_item = &theme_items[plugin_index];

        assert_eq!(plugin_item.value, "plugin");
        assert!(
            plugin_item
                .detail
                .contains("plugins/theme-pack/themes/solarized-pro.json")
        );
        assert!(matches!(
            plugin_item.action,
            SettingsPanelAction::Command(ref command) if command == "/theme solarized-pro"
        ));
        assert_eq!(
            theme_preview_name_for_settings_item(plugin_item).as_deref(),
            Some("solarized-pro")
        );
        let light_index = theme_items
            .iter()
            .position(|item| item.label == "Light")
            .expect("light theme row");
        assert_eq!(
            settings_panel_item_value_for_render(
                SettingsSubpanel::Theme,
                &theme_items[0],
                0,
                light_index
            ),
            "current"
        );
        assert_eq!(
            settings_panel_item_value_for_render(
                SettingsSubpanel::Theme,
                &theme_items[light_index],
                light_index,
                light_index
            ),
            "preview"
        );
        assert_eq!(
            settings_panel_item_value_for_render(
                SettingsSubpanel::Theme,
                plugin_item,
                plugin_index,
                plugin_index
            ),
            "preview"
        );
        assert!(
            settings_panel_item_detail_for_render(
                SettingsSubpanel::Theme,
                &theme_items[light_index],
                light_index,
                light_index
            )
            .contains("Esc restores saved theme")
        );
        assert!(
            settings_panel_footer_hint(SettingsSubpanel::Theme, "1-6", 6)
                .contains("Up/Down/1-9 preview live")
        );
        assert_eq!(
            settings_panel_numeric_preview_target(SettingsSubpanel::Theme, &theme_items, "2"),
            Some(1)
        );
        assert_eq!(
            settings_panel_numeric_preview_target(SettingsSubpanel::Theme, &theme_items, "5"),
            None
        );
        assert_eq!(
            settings_panel_numeric_preview_target(
                SettingsSubpanel::Theme,
                &theme_items,
                &theme_items.len().to_string(),
            ),
            None
        );
        assert_eq!(
            settings_panel_numeric_preview_target(SettingsSubpanel::Theme, &theme_items, "0"),
            None
        );
        assert_eq!(
            settings_panel_numeric_preview_target(SettingsSubpanel::General, &theme_items, "2"),
            None
        );

        let state = NativeTuiState {
            overlay: Some(TuiOverlay::SettingsPanel {
                panel: SettingsSubpanel::Theme,
                selected: plugin_index,
                items: theme_items,
                back_stack: Vec::new(),
            }),
            ..NativeTuiState::default()
        };
        assert_eq!(theme_name_for_tui_frame("oppi", &state), "solarized-pro");
    }

    #[test]
    fn theme_preview_tokens_use_loaded_discovered_theme_colors() {
        let mut preview_cache = BTreeMap::new();
        preview_cache.insert(
            "solarized-pro".to_string(),
            LoadedShellTheme {
                name: "solarized-pro".to_string(),
                source: "plugin".to_string(),
                colors: json!({ "accent": "#123abc" }),
                export: None,
                line_tokens: ThemeTokens {
                    accent: "\x1b[38;5;200m",
                    dim: "",
                    code: "",
                    success: "",
                    warning: "",
                    error: "",
                    reset: "",
                },
            },
        );
        let state = NativeTuiState {
            overlay: Some(TuiOverlay::SettingsPanel {
                panel: SettingsSubpanel::Theme,
                selected: 0,
                items: vec![SettingsPanelItem {
                    label: "solarized-pro".to_string(),
                    value: "plugin".to_string(),
                    detail: "Discovered plugin theme".to_string(),
                    current: false,
                    action: SettingsPanelAction::Command("/theme solarized-pro".to_string()),
                }],
                back_stack: Vec::new(),
            }),
            ..NativeTuiState::default()
        };

        let tokens = theme_tokens_for_tui_frame("oppi", None, &state, &preview_cache);

        assert_eq!(tokens.accent, "\x1b[38;5;200m");
    }

    #[test]
    fn settings_root_navigation_uses_sections_not_one_long_list() {
        let items = vec![
            SettingsRootItem {
                section: "AI",
                label: "Main model",
                value: "gpt".to_string(),
                detail: "main",
                action: SettingsRootAction::OpenMainModel,
            },
            SettingsRootItem {
                section: "AI",
                label: "Role models",
                value: "advanced".to_string(),
                detail: "roles",
                action: SettingsRootAction::OpenPanel(SettingsSubpanel::RoleModels),
            },
            SettingsRootItem {
                section: "Safety",
                label: "Permissions",
                value: "default".to_string(),
                detail: "perms",
                action: SettingsRootAction::OpenPanel(SettingsSubpanel::Permissions),
            },
        ];
        assert_eq!(settings_root_move_row(&items, 0, 1), 1);
        assert_eq!(settings_root_move_row(&items, 1, 1), 0);
        assert_eq!(settings_root_move_tab(&items, 0, 1), 2);
        assert_eq!(settings_root_move_tab(&items, 2, -1), 0);
        let filtered = filtered_settings_root_items(&items, "perm");
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].0, 2);
        assert_eq!(settings_root_move_visible(&items, "advanced", 0, 1), 1);
        assert_eq!(first_visible_settings_index(&items, "safety"), Some(2));
    }

    #[test]
    fn settings_root_key_actions_support_navigation_and_search() {
        assert_eq!(
            settings_root_key_action(&EditorInput::Up),
            SettingsRootKeyAction::MoveRow(-1)
        );
        assert_eq!(
            settings_root_key_action(&EditorInput::Down),
            SettingsRootKeyAction::MoveRow(1)
        );
        assert_eq!(
            settings_root_key_action(&EditorInput::Left),
            SettingsRootKeyAction::MoveTab(-1)
        );
        assert_eq!(
            settings_root_key_action(&EditorInput::Right),
            SettingsRootKeyAction::MoveTab(1)
        );
        assert_eq!(
            settings_root_key_action(&EditorInput::Enter),
            SettingsRootKeyAction::Activate
        );
        assert_eq!(
            settings_root_key_action(&EditorInput::Text(" ".to_string())),
            SettingsRootKeyAction::Activate
        );
        assert_eq!(
            settings_root_key_action(&EditorInput::Text("p".to_string())),
            SettingsRootKeyAction::Search("p".to_string())
        );
        assert_eq!(
            settings_root_key_action(&EditorInput::Backspace),
            SettingsRootKeyAction::SearchBackspace
        );
        assert_eq!(
            settings_root_key_action(&EditorInput::Tab),
            SettingsRootKeyAction::Ignore
        );
        assert_eq!(
            settings_root_key_action(&EditorInput::BackTab),
            SettingsRootKeyAction::Ignore
        );
    }

    #[test]
    fn settings_root_search_matches_terms_and_keeps_original_indices() {
        let items = vec![
            SettingsRootItem {
                section: "Pi",
                label: "Main model",
                value: "gpt".to_string(),
                detail: "Select the default model for OPPi executor turns",
                action: SettingsRootAction::OpenMainModel,
            },
            SettingsRootItem {
                section: "Pi",
                label: "Images",
                value: "preview".to_string(),
                detail: "Terminal image preview and provider image handling",
                action: SettingsRootAction::OpenPanel(SettingsSubpanel::Images),
            },
            SettingsRootItem {
                section: "Footer",
                label: "Status bar",
                value: "live".to_string(),
                detail: "Footer help and context chips",
                action: SettingsRootAction::OpenPanel(SettingsSubpanel::Footer),
            },
        ];

        let filtered = filtered_settings_root_items(&items, "image preview");
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].0, 1);
        assert_eq!(first_visible_settings_index(&items, "footer"), Some(2));
        assert_eq!(settings_root_move_visible(&items, "pi", 0, 1), 1);
    }

    #[test]
    fn settings_root_exposes_goal_mode_shortcut() {
        let items = settings_root_items_for_values(
            &ProviderConfig::Mock,
            MOCK_MODEL_DISPLAY_LABEL,
            0,
            FollowUpMode::All,
            SteeringMode::All,
            true,
            DoubleEscapeAction::Fork,
            TreeFilterMode::LabeledOnly,
            true,
            10,
            true,
            2,
            true,
            true,
            "auto-review",
            "oppi",
            "thread-1",
            "goal active 2m",
        );

        let item = items
            .iter()
            .find(|item| item.section == "General" && item.label == "Status & usage")
            .expect("settings root should expose status and usage");
        assert_eq!(item.value, "/usage");
        assert_eq!(item.detail, "Usage/status, footer, and debug surfaces");
        assert!(matches!(
            item.action,
            SettingsRootAction::OpenPanel(SettingsSubpanel::General)
        ));

        let item = items
            .iter()
            .find(|item| item.section == "General" && item.label == "Goal mode")
            .expect("settings root should expose goal mode");
        assert_eq!(item.value, "goal active 2m");
        assert_eq!(item.detail, "Track and continue one thread objective");
        assert!(matches!(
            item.action,
            SettingsRootAction::InsertCommand(ref command) if command == "/goal"
        ));

        let item = items
            .iter()
            .find(|item| item.section == "Pi" && item.label == "Follow-up mode")
            .expect("settings root should expose follow-up mode");
        assert_eq!(item.value, "all");
        assert_eq!(item.detail, "Alt+Enter queue delivery mode");
        assert!(matches!(
            item.action,
            SettingsRootAction::InsertCommand(ref command)
                if command == "/follow-up mode one-at-a-time"
        ));

        let item = items
            .iter()
            .find(|item| item.section == "Pi" && item.label == "Steering mode")
            .expect("settings root should expose steering mode");
        assert_eq!(item.value, "all");
        assert_eq!(item.detail, "Ctrl+Enter active-turn delivery mode");
        assert!(matches!(
            item.action,
            SettingsRootAction::InsertCommand(ref command)
                if command == "/steer mode one-at-a-time"
        ));

        let item = items
            .iter()
            .find(|item| item.section == "Pi" && item.label == "Quiet startup")
            .expect("settings root should expose quiet startup");
        assert_eq!(item.value, "enabled");
        assert_eq!(item.detail, "Suppress the non-critical ready banner");
        assert!(matches!(
            item.action,
            SettingsRootAction::InsertCommand(ref command)
                if command == "/startup quiet off"
        ));

        let item = items
            .iter()
            .find(|item| item.section == "Pi" && item.label == "Double escape")
            .expect("settings root should expose double-escape action");
        assert_eq!(item.value, "fork");
        assert_eq!(
            item.detail,
            "Action when Escape is pressed twice with an empty editor"
        );
        assert!(matches!(
            item.action,
            SettingsRootAction::InsertCommand(ref command)
                if command == "/editor double-escape none"
        ));

        let item = items
            .iter()
            .find(|item| item.section == "Pi" && item.label == "Tree filter")
            .expect("settings root should expose Pi tree filter mode");
        assert_eq!(item.value, "labeled-only");
        assert_eq!(
            item.detail,
            "Default filter for transcript entries and sessions"
        );
        assert!(matches!(
            item.action,
            SettingsRootAction::InsertCommand(ref command) if command == "/tree filter all"
        ));

        let item = items
            .iter()
            .find(|item| item.section == "Pi" && item.label == "Skill commands")
            .expect("settings root should expose skill commands");
        assert_eq!(item.value, "enabled");
        assert_eq!(item.detail, "Register skills as /skill:name commands");
        assert!(matches!(
            item.action,
            SettingsRootAction::InsertCommand(ref command)
                if command == "/skills commands off"
        ));

        let item = items
            .iter()
            .find(|item| item.section == "Pi" && item.label == "Autocomplete items")
            .expect("settings root should expose autocomplete item count");
        assert_eq!(item.value, "10");
        assert_eq!(item.detail, "Maximum visible command suggestions");
        assert!(matches!(
            item.action,
            SettingsRootAction::InsertCommand(ref command)
                if command == "/autocomplete items 15"
        ));

        let item = items
            .iter()
            .find(|item| item.section == "Pi" && item.label == "Hardware cursor")
            .expect("settings root should expose hardware cursor");
        assert_eq!(item.value, "enabled");
        assert_eq!(
            item.detail,
            "Keep the terminal cursor visible for IME support"
        );
        assert!(matches!(
            item.action,
            SettingsRootAction::InsertCommand(ref command)
                if command == "/cursor hardware off"
        ));

        let item = items
            .iter()
            .find(|item| item.section == "Pi" && item.label == "Editor padding")
            .expect("settings root should expose editor padding");
        assert_eq!(item.value, "2");
        assert_eq!(item.detail, "Horizontal input padding");
        assert!(matches!(
            item.action,
            SettingsRootAction::InsertCommand(ref command)
                if command == "/editor padding 3"
        ));

        let item = items
            .iter()
            .find(|item| item.section == "Pi" && item.label == "Terminal progress")
            .expect("settings root should expose terminal progress");
        assert_eq!(item.value, "enabled");
        assert_eq!(item.detail, "OSC 9;4 turn progress indicator");
        assert!(matches!(
            item.action,
            SettingsRootAction::InsertCommand(ref command)
                if command == "/terminal progress off"
        ));

        let item = items
            .iter()
            .find(|item| item.section == "Pi" && item.label == "Clear on shrink")
            .expect("settings root should expose terminal clear-on-shrink");
        assert_eq!(item.value, "enabled");
        assert_eq!(
            item.detail,
            "Clear stale terminal rows after content shrinks"
        );
        assert!(matches!(
            item.action,
            SettingsRootAction::InsertCommand(ref command)
                if command == "/terminal clear-on-shrink off"
        ));
    }

    #[test]
    fn settings_section_labels_are_plain_terminal_text() {
        assert_eq!(settings_section_display_label("General"), "General");
        assert_eq!(settings_section_display_label("Pi"), "Pi");
        assert_eq!(settings_section_display_label("Footer"), "Footer");
        let items = vec![
            SettingsRootItem {
                section: "General",
                label: "Status",
                value: "/usage".to_string(),
                detail: "status",
                action: SettingsRootAction::OpenPanel(SettingsSubpanel::General),
            },
            SettingsRootItem {
                section: "Pi",
                label: "Model",
                value: "gpt-5.5".to_string(),
                detail: "model",
                action: SettingsRootAction::OpenMainModel,
            },
            SettingsRootItem {
                section: "Footer",
                label: "Status bar",
                value: "local".to_string(),
                detail: "footer",
                action: SettingsRootAction::OpenPanel(SettingsSubpanel::Footer),
            },
        ];
        assert_eq!(settings_section_bar(&items, "Pi"), "General  [Pi]  Footer");
    }

    #[test]
    fn settings_subpanel_renders_nested_actions_width_safe() {
        let provider_items = provider_settings_panel_items(&ProviderConfig::Mock);
        assert!(provider_items.iter().any(|item| matches!(
            item.action,
            SettingsPanelAction::EditCommand(ref command) if command == "/provider smoke "
        )));
        assert!(provider_items.iter().any(|item| matches!(
            item.action,
            SettingsPanelAction::Command(ref command) if command == "/provider validate"
        )));
        let lines = render_settings_subpanel_overlay(
            SettingsSubpanel::Provider,
            &provider_items,
            0,
            80,
            20,
            theme_tokens("plain"),
        );
        assert!(
            lines
                .iter()
                .any(|line| line.contains("Settings › provider"))
        );
        assert!(lines.iter().any(|line| line.contains("Custom API env")));
        assert!(
            lines
                .iter()
                .any(|line| line.contains("Enter/Space to change"))
        );
        for line in lines {
            assert!(tui_visible_width(&line) <= 80, "too wide: {line}");
        }

        let memory_items = memory_settings_panel_items_for_settings(&MemorySettings::default());
        assert!(
            memory_items.iter().any(|item| {
                item.label == "Dashboard" && item.detail.contains("runtime-backed")
            })
        );
        assert!(
            memory_items
                .iter()
                .any(|item| item.label == "Maintenance apply")
        );
        assert!(memory_items.iter().any(|item| matches!(
            item.action,
            SettingsPanelAction::Command(ref command) if command == "/idle-compact compact"
        )));
    }

    #[test]
    fn settings_action_returns_to_previous_settings_screen() {
        assert!(matches!(
            settings_overlay_after_panel_action(SettingsSubpanel::Theme, None, false),
            TuiOverlay::Settings { .. }
        ));
        assert!(matches!(
            settings_overlay_after_panel_action(
                SettingsSubpanel::LoginClaude,
                Some(SettingsSubpanel::LoginSubscription),
                false
            ),
            TuiOverlay::SettingsPanel {
                panel: SettingsSubpanel::LoginSubscription,
                ..
            }
        ));
    }

    fn settings_overlay_after_panel_action(
        _panel: SettingsSubpanel,
        previous: Option<SettingsSubpanel>,
        _escape: bool,
    ) -> TuiOverlay {
        let back_stack = previous.into_iter().collect::<Vec<_>>();
        settings_previous_overlay_for_stack(&back_stack)
    }

    #[test]
    fn settings_overlays_keep_a_stable_common_height() {
        let provider_items = provider_settings_panel_items(&ProviderConfig::Mock);
        let login_items = login_settings_panel_items(&ProviderConfig::Mock);
        let provider_lines = render_settings_subpanel_overlay(
            SettingsSubpanel::Provider,
            &provider_items,
            0,
            80,
            30,
            theme_tokens("plain"),
        );
        let login_lines = render_settings_subpanel_overlay(
            SettingsSubpanel::Login,
            &login_items,
            0,
            80,
            30,
            theme_tokens("plain"),
        );

        assert_eq!(provider_lines.len(), login_lines.len());
        assert!(provider_lines.len() >= 16);
        for line in provider_lines.into_iter().chain(login_lines) {
            assert!(tui_visible_width(&line) <= 80, "too wide: {line}");
        }
    }
}
