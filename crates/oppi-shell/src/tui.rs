use super::*;

const SPINNER_FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const SETTINGS_PANEL_ROW_SLOTS: usize = 11;
const SETTINGS_ROOT_ITEM_SLOTS: usize = 5;

pub(super) fn retained_tui_interactive_loop(
    session: &mut ShellSession,
    provider: &mut ProviderConfig,
) -> Result<(), String> {
    let _guard = TuiTerminalGuard::enter()?;
    session.terminal_ui_active = true;
    session.print_text(
        "OPPi native TUI ready — /settings opens settings; Ctrl+L selects model; Enter submits; Shift+Enter newline; Ctrl+C twice exits.",
        false,
    )?;
    let mut state = NativeTuiState::default();
    let mut renderer = DifferentialRenderer::default();
    let mut running = true;
    while running {
        let (width, height) = terminal::size().unwrap_or((100, 30));
        state.spinner_index = state.spinner_index.wrapping_add(1);
        session.sync_ui_docks();
        let frame =
            render_native_tui_frame(session, provider, &state, width as usize, height as usize);
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
                        renderer.render(&frame)?;
                    }
                }
                CrosstermEvent::Resize(_, _) => renderer.invalidate(),
                _ => {}
            }
        }

        if let Some(outcome) = session.poll_turn_events_silent()? {
            if matches!(outcome, TurnOutcome::Completed) {
                session.start_next_queued_or_goal_continuation(provider, false)?;
            }
        } else if !session.is_turn_running() && !session.has_pending_pause() {
            session.start_next_queued_or_goal_continuation(provider, false)?;
        }
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
                let filtered = filtered_settings_root_items(&root_items, query);
                Some(TuiOverlayView {
                    title: if query.is_empty() {
                        "settings".to_string()
                    } else {
                        format!("settings · filter: {query}")
                    },
                    selected: filtered
                        .iter()
                        .position(|(index, _)| index == selected)
                        .unwrap_or(0),
                    items: filtered
                        .into_iter()
                        .map(|(_, item)| TuiOverlayViewItem {
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
                selected: *selected,
                items: items
                    .iter()
                    .map(|item| TuiOverlayViewItem {
                        label: item.label.clone(),
                        value: item.value.clone(),
                        detail: item.detail.clone(),
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
            Some(TuiOverlay::Effort {
                selected,
                allowed,
                current,
            }) => Some(TuiOverlayView {
                title: format!("effort · {}", effort_model_name(provider)),
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
                items,
            }) => Some(TuiOverlayView {
                title: if query.is_empty() {
                    "main model".to_string()
                } else {
                    format!("main model · filter: {query}")
                },
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
            None => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
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
    Effort {
        selected: usize,
        allowed: Vec<ThinkingLevel>,
        current: ThinkingLevel,
    },
    Models {
        selected: usize,
        query: String,
        items: Vec<ModelPickerItem>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TuiOverlayView {
    pub(super) title: String,
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

struct TuiTerminalGuard;

impl TuiTerminalGuard {
    fn enter() -> Result<Self, String> {
        terminal::enable_raw_mode()
            .map_err(|error| format!("enable raw terminal mode: {error}"))?;
        print!("\x1b[?25l");
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
}

impl DifferentialRenderer {
    fn invalidate(&mut self) {
        self.previous.clear();
        self.previous_width = 0;
    }

    fn render(&mut self, lines: &[String]) -> Result<(), String> {
        let width = lines
            .iter()
            .map(|line| tui_visible_width(line))
            .max()
            .unwrap_or(0);
        let mut out = String::new();
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
                out.push_str("\x1b[J");
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
        print!("{out}");
        io::stdout()
            .flush()
            .map_err(|error| format!("flush TUI frame: {error}"))?;
        self.previous = lines.to_vec();
        self.previous_width = width;
        Ok(())
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
        Some(TuiOverlay::Settings { selected, query }) => match input {
            EditorInput::Up => {
                let items = settings_root_items(session, provider);
                *selected = settings_root_move_visible(&items, query, *selected, -1);
            }
            EditorInput::Down => {
                let items = settings_root_items(session, provider);
                *selected = settings_root_move_visible(&items, query, *selected, 1);
            }
            EditorInput::Left | EditorInput::BackTab if query.is_empty() => {
                let items = settings_root_items(session, provider);
                *selected = settings_root_move_tab(&items, *selected, -1);
            }
            EditorInput::Right | EditorInput::Tab if query.is_empty() => {
                let items = settings_root_items(session, provider);
                *selected = settings_root_move_tab(&items, *selected, 1);
            }
            EditorInput::Home | EditorInput::PageUp => {
                let items = settings_root_items(session, provider);
                *selected = first_visible_settings_index(&items, query).unwrap_or(0);
            }
            EditorInput::End | EditorInput::PageDown => {
                let items = settings_root_items(session, provider);
                *selected = last_visible_settings_index(&items, query).unwrap_or(0);
            }
            EditorInput::Enter => {
                let selected = *selected;
                activate_settings_item(session, provider, state, selected)?;
            }
            EditorInput::Text(text) if text == " " => {
                let selected = *selected;
                activate_settings_item(session, provider, state, selected)?;
            }
            EditorInput::Text(text) => {
                query.push_str(&text);
                let items = settings_root_items(session, provider);
                *selected = first_visible_settings_index(&items, query).unwrap_or(0);
            }
            EditorInput::Backspace | EditorInput::Delete => {
                query.pop();
                let items = settings_root_items(session, provider);
                *selected = first_visible_settings_index(&items, query).unwrap_or(0);
            }
            EditorInput::Escape => state.overlay = None,
            EditorInput::CtrlC => {
                state.overlay = None;
                state.editor.arm_ctrl_c_exit();
            }
            EditorInput::CtrlD => return Ok(false),
            _ => {}
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
            if matches!(prompt, "/tree" | "/sessions" | "/resume") {
                open_sessions_overlay(session, state)?;
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
                session.print_text("restored queued follow-up into editor", false)?;
            } else {
                session.print_text("no queued follow-up to restore", false)?;
            }
            Ok(true)
        }
        EditorAction::Interrupt => {
            session.interrupt_active_turn(false)?;
            Ok(true)
        }
    }
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
    session: &ShellSession,
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
    session: &ShellSession,
    provider: &ProviderConfig,
    state: &mut NativeTuiState,
    panel: SettingsSubpanel,
    back_stack: Vec<SettingsSubpanel>,
) {
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
    session: &ShellSession,
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
        SettingsSubpanel::General => general_settings_panel_items(),
        SettingsSubpanel::Footer => footer_settings_panel_items(),
        SettingsSubpanel::Compaction => compaction_settings_panel_items(),
        SettingsSubpanel::Theme => theme_settings_panel_items(session),
        SettingsSubpanel::Permissions => permission_settings_panel_items(session),
        SettingsSubpanel::Provider => provider_settings_panel_items(provider),
        SettingsSubpanel::Login => login_settings_panel_items(provider),
        SettingsSubpanel::LoginSubscription => login_subscription_settings_panel_items(),
        SettingsSubpanel::LoginApi => login_api_settings_panel_items(provider),
        SettingsSubpanel::LoginClaude => login_claude_settings_panel_items(),
        SettingsSubpanel::Memory => memory_settings_panel_items(),
        SettingsSubpanel::RoleModels => role_model_settings_panel_items(session, provider),
        SettingsSubpanel::ScopedModels => scoped_model_settings_panel_items(session, provider),
    }
}

fn general_settings_panel_items() -> Vec<SettingsPanelItem> {
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
    ]
}

fn footer_settings_panel_items() -> Vec<SettingsPanelItem> {
    vec![
        settings_command_item(
            "Hotkey help",
            "Alt+K",
            "Alt+K toggles the live help row; /keys explains fallbacks",
            true,
            "/keys",
        ),
        settings_command_item(
            "Usage display",
            "local",
            "Show local usage/status snapshot",
            false,
            "/usage",
        ),
        settings_command_item(
            "Todos chip",
            "/todos",
            "Show active model-owned todos",
            false,
            "/todos",
        ),
        settings_command_item(
            "Suggestion dock",
            "ghost",
            "Show active ghost suggestion or empty state",
            false,
            "/suggest-next show",
        ),
    ]
}

fn compaction_settings_panel_items() -> Vec<SettingsPanelItem> {
    vec![
        settings_command_item(
            "Memory status",
            "client",
            "Show Hoppi/native memory status before compacting",
            true,
            "/memory status",
        ),
        settings_edit_item(
            "Compact summary",
            "edit",
            "Insert /memory compact <summary> into the editor",
            "/memory compact ",
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
    theme_settings_panel_items_for(&session.theme)
}

fn theme_settings_panel_items_for(current_theme: &str) -> Vec<SettingsPanelItem> {
    [
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
    .collect()
}

fn permission_settings_panel_items(session: &ShellSession) -> Vec<SettingsPanelItem> {
    permission_settings_panel_items_for(session.permission_mode.as_str())
}

fn permission_settings_panel_items_for(current_mode: &str) -> Vec<SettingsPanelItem> {
    [
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
    .collect()
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
    for model in main_model_ids_for_provider(session, provider)
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
        model_ids.insert("mock-scripted".to_string());
    }
    let inherited = session
        .session_model(provider)
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
            items.push(SettingsPanelItem {
                label: format!("{role} model"),
                value: model.clone(),
                detail: format!("Use {model} for {role} turns"),
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

fn memory_settings_panel_items() -> Vec<SettingsPanelItem> {
    vec![
        settings_command_item(
            "Status",
            "client-hosted",
            "Show Hoppi/native memory status",
            false,
            "/memory status",
        ),
        settings_command_item(
            "Enable",
            "on",
            "Enable client-hosted project memory",
            false,
            "/memory on",
        ),
        settings_command_item(
            "Disable",
            "off",
            "Disable memory for this session/project scope",
            false,
            "/memory off",
        ),
        settings_command_item(
            "Dashboard",
            "view",
            "Open client-hosted memory dashboard panel",
            false,
            "/memory dashboard",
        ),
        settings_command_item(
            "Settings",
            "view",
            "Open client-hosted memory settings panel",
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
        settings_edit_item(
            "Record compaction",
            "edit",
            "Insert /memory compact <summary> into the editor",
            "/memory compact ",
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

fn session_picker_items(session: &mut ShellSession) -> Result<Vec<SessionPickerItem>, String> {
    let value = session.rpc.request("thread/list", json!({}))?;
    let result: RuntimeListResult<Thread> = serde_json::from_value(value)
        .map_err(|error| format!("decode thread/list for session picker: {error}"))?;
    Ok(result
        .items
        .into_iter()
        .filter(|thread| thread.status != ThreadStatus::Archived)
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

fn open_models_overlay(
    session: &mut ShellSession,
    provider: &ProviderConfig,
    state: &mut NativeTuiState,
) -> Result<(), String> {
    let items = model_picker_items(session, provider)?;
    state.overlay = Some(TuiOverlay::Models {
        selected: 0,
        query: String::new(),
        items,
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

fn model_picker_items(
    session: &mut ShellSession,
    provider: &ProviderConfig,
) -> Result<Vec<ModelPickerItem>, String> {
    let session_model = session.session_model(provider).map(str::to_string);
    Ok(main_model_ids_for_provider(session, provider)
        .into_iter()
        .map(|model| ModelPickerItem {
            label: model.clone(),
            detail: "Set the main model for OPPi executor turns".to_string(),
            current: session_model.as_deref() == Some(model.as_str()),
            action: ModelPickerAction::SelectSession(model),
        })
        .collect())
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
    let models = main_model_ids_for_provider(session, provider);
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
        ModelPickerAction::SelectSession(model) => format!("main {model}"),
        ModelPickerAction::SelectRole { role, model } => format!("role {role} {model}"),
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
                &format!("role {role} model set to {model} (persisted)"),
                false,
            )?;
        }
        ModelPickerAction::ClearRole { role } => {
            session.role_models.remove(role);
            save_role_profiles(&session.role_profile_path, &session.role_models)?;
            session.print_text(
                &format!(
                    "role {role} now inherits {} (persisted)",
                    session.session_model(provider).unwrap_or("session model")
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
    let scoped_model_count = session.scoped_model_ids.len();
    settings_root_items_for_values(
        provider,
        session_model,
        scoped_model_count,
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
    permission_mode: &str,
    theme: &str,
    thread_id: &str,
    goal_status: &str,
) -> Vec<SettingsRootItem> {
    vec![
        SettingsRootItem {
            section: "General",
            label: "Status shortcuts",
            value: "/usage".to_string(),
            detail: "Usage/status, keybindings, and debug surfaces",
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
            label: "Effort",
            value: effort_level_label_for_provider(
                provider,
                current_effort_level_for_provider(provider),
            )
            .to_string(),
            detail: "Model-aware thinking slider for the current main model",
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
            label: "Context handoff",
            value: "manual".to_string(),
            detail: "Manual memory compaction and maintenance shortcuts",
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
            value: "client-hosted".to_string(),
            detail: "Recall, dashboard, and maintenance",
            action: SettingsRootAction::OpenPanel(SettingsSubpanel::Memory),
        },
    ]
}

fn filtered_settings_root_items<'a>(
    items: &'a [SettingsRootItem],
    query: &str,
) -> Vec<(usize, &'a SettingsRootItem)> {
    let query = query.trim().to_ascii_lowercase();
    items
        .iter()
        .enumerate()
        .filter(|(_, item)| {
            query.is_empty()
                || item.section.to_ascii_lowercase().contains(&query)
                || item.label.to_ascii_lowercase().contains(&query)
                || item.value.to_ascii_lowercase().contains(&query)
                || item.detail.to_ascii_lowercase().contains(&query)
        })
        .collect()
}

fn first_visible_settings_index(items: &[SettingsRootItem], query: &str) -> Option<usize> {
    filtered_settings_root_items(items, query)
        .first()
        .map(|(index, _)| *index)
}

fn last_visible_settings_index(items: &[SettingsRootItem], query: &str) -> Option<usize> {
    filtered_settings_root_items(items, query)
        .last()
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
        "General" => "⚙️ General".to_string(),
        "Pi" => "🧩 Pi".to_string(),
        "AI" => "🤖 AI".to_string(),
        "Account" => "🔑 Account".to_string(),
        "Permissions" | "Safety" => "🔐 Permissions".to_string(),
        "Theme" | "Appearance" => "🎨 Theme".to_string(),
        "Footer" => "🧭 Footer".to_string(),
        "Compaction" => "🗜️ Compaction".to_string(),
        "Memory" => "🧠 Memory".to_string(),
        "Workspace" => "🗂️ Workspace".to_string(),
        other => other.to_string(),
    }
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
    let tokens = theme_tokens(&session.theme);

    let header = render_tui_header(session, provider, state, width, tokens);
    let dock_lines = render_tui_docks(session, width, tokens);
    let overlay_lines = state
        .overlay
        .as_ref()
        .map(|overlay| render_overlay_lines(overlay, session, provider, width, height, tokens))
        .unwrap_or_default();
    let editor_lines = render_tui_editor(&state.editor, width, session.is_turn_running(), tokens);
    let slash_palette_lines = state
        .current_slash_palette(session, provider)
        .map(|palette| render_slash_palette(&palette, width, 5, tokens))
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
    let max_entries = max_entries.max(1).min(5).min(count);
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
    busy: bool,
    tokens: ThemeTokens,
) -> Vec<String> {
    let mut lines = vec![tui_rule(width, tokens)];
    let rendered_buffer = render_editor_buffer(editor, width, tokens);
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

fn render_editor_buffer(editor: &LineEditor, width: usize, _tokens: ThemeTokens) -> Vec<String> {
    let buffer = editor.buffer_preview();
    if buffer.is_empty() {
        return vec!["\x1b[7m \x1b[27m".to_string()];
    }
    let cursor = editor.cursor();
    let mut output = Vec::new();
    let mut offset = 0;
    for raw_line in buffer.split('\n') {
        let line_end = offset + raw_line.len();
        let mut line = String::new();
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
    let queued = session.follow_up_queue.len();
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
        TuiOverlay::Effort {
            selected,
            allowed,
            current,
        } => render_effort_overlay(provider, allowed, *current, *selected, width, tokens),
        TuiOverlay::Models {
            selected,
            query,
            items,
        } => render_models_overlay(items, query, *selected, width, height, tokens),
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
) -> Vec<String> {
    let filtered = filtered_model_items(items, query);
    let count = filtered.len();
    let selected = selected.min(count.saturating_sub(1));
    let max_rows = height.saturating_sub(10).clamp(4, 10).min(count.max(1));
    let start = selected.saturating_add(1).saturating_sub(max_rows);
    let end = (start + max_rows).min(count);
    let mut lines = vec![tui_rule(width, tokens)];
    lines.push(tui_pad_line(
        &paint(tokens, tokens.accent, "Main model"),
        width,
    ));
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
            let row_text = format!("{marker} {current} {label:<24} {}", item.value);
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
        .map(|item| item.detail.as_str())
        .unwrap_or("");
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
            &format!("{range} of {count} · Enter/Space to change · Esc back"),
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
    let active_section = items
        .get(selected)
        .map(|item| item.section)
        .unwrap_or("General");
    let filtered = filtered_settings_root_items(&items, query);
    let mut sections = Vec::<&str>::new();
    for item in &items {
        if sections.last().copied() != Some(item.section) {
            sections.push(item.section);
        }
    }
    let mut lines = vec![tui_rule(width, tokens)];
    lines.push(tui_pad_line(
        &paint(tokens, tokens.accent, "OPPi Settings"),
        width,
    ));
    let tabs = sections
        .iter()
        .map(|section| {
            let display = settings_section_display_label(section);
            if *section == active_section {
                paint(tokens, tokens.accent, &format!("[{display}]"))
            } else {
                paint(tokens, tokens.dim, &format!(" {display} "))
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    lines.push(tui_pad_line(&tui_truncate(&tabs, width), width));
    lines.push(tui_pad_line(
        &paint(
            tokens,
            tokens.dim,
            if query.is_empty() {
                "←/→ tabs · ↑/↓ settings · type search · Enter/Space open · Esc close"
            } else {
                "type to search · Backspace edit · ↑/↓ results · Enter open · Esc close"
            },
        ),
        width,
    ));
    let search_line = if query.is_empty() {
        paint(tokens, tokens.dim, "Search: type to filter")
    } else {
        paint(tokens, tokens.accent, &format!("Search: {query}"))
    };
    lines.push(tui_pad_line(&search_line, width));
    lines.push(tui_pad_line("", width));
    let visible_items = if query.is_empty() {
        items
            .iter()
            .enumerate()
            .filter(|(_, item)| item.section == active_section)
            .collect::<Vec<_>>()
    } else {
        filtered
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
        lines.push(tui_pad_line(
            &paint(tokens, tokens.dim, "  No matching settings"),
            width,
        ));
        lines.push(tui_pad_line("", width));
        rendered_items += 1;
    }
    for &(index, item) in visible_items
        .iter()
        .skip(start)
        .take(end.saturating_sub(start))
    {
        let marker = if index == selected { "→" } else { " " };
        let label = if index == selected {
            paint(tokens, tokens.accent, item.label)
        } else {
            item.label.to_string()
        };
        let prefix = if query.is_empty() {
            String::new()
        } else {
            format!("{} › ", item.section)
        };
        let row = format!("{marker} {prefix}{label} — {}", item.value);
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
    let summary = if query.is_empty() {
        format!(
            "{range} of {} settings · active: {}",
            visible_items.len(),
            settings_section_display_label(active_section)
        )
    } else {
        format!("{range} of {} matching settings", visible_items.len())
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
            items: Vec::new(),
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
    fn effort_overlay_renders_model_aware_slider_width_safe() {
        let provider = ProviderConfig::OpenAiCompatible(OpenAiCompatibleConfig {
            flavor: DirectProviderFlavor::OpenAiCodex,
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
        let lines = render_models_overlay(&items, "review", 0, 78, 20, theme_tokens("plain"));
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

        let permission_items = permission_settings_panel_items_for("auto-review");
        assert!(
            permission_items
                .iter()
                .any(|item| item.current && item.value == "auto-review")
        );
        assert!(permission_items.iter().any(|item| matches!(
            item.action,
            SettingsPanelAction::Command(ref command) if command == "/permissions full-access"
        )));

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
            SettingsPanelAction::OpenPanel(_) | SettingsPanelAction::Model(_) => true,
        }));

        let memory_items = memory_settings_panel_items();
        assert!(memory_items.iter().any(|item| matches!(
            item.action,
            SettingsPanelAction::Command(ref command) if command == "/memory maintenance apply"
        )));
        assert!(memory_items.iter().any(|item| matches!(
            item.action,
            SettingsPanelAction::EditCommand(ref command) if command == "/memory compact "
        )));
    }

    #[test]
    fn settings_root_navigation_uses_tabs_not_one_long_list() {
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
    fn settings_root_exposes_goal_mode_shortcut() {
        let items = settings_root_items_for_values(
            &ProviderConfig::Mock,
            "mock-scripted",
            0,
            "auto-review",
            "oppi",
            "thread-1",
            "goal active 2m",
        );

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

        let memory_items = memory_settings_panel_items();
        assert!(
            memory_items
                .iter()
                .any(|item| item.label == "Maintenance apply")
        );
        assert!(memory_items.iter().any(|item| matches!(
            item.action,
            SettingsPanelAction::EditCommand(ref command) if command == "/memory compact "
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
