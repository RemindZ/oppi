fn interactive_loop(
    session: &mut ShellSession,
    provider: &mut ProviderConfig,
    json: bool,
    raw: bool,
    ratatui: bool,
) -> Result<(), String> {
    if raw && !json && io::stdin().is_terminal() && io::stdout().is_terminal() {
        if ratatui {
            return ratatui_ui::ratatui_interactive_loop(session, provider);
        }
        return tui::retained_tui_interactive_loop(session, provider);
    }
    line_interactive_loop(session, provider, json)
}

fn maybe_offer_hoppi_install_on_startup(session: &mut ShellSession) -> Result<(), String> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Ok(());
    }
    if !should_offer_hoppi_first_start_install(
        &session.memory_settings,
        hoppi_backend_available_for_startup_offer(),
    ) {
        return Ok(());
    }

    session.memory_status = Some(MemoryStatus {
        enabled: true,
        initialized: false,
        backend: "setup".to_string(),
        scope: "project".to_string(),
        memory_count: 0,
        ..MemoryStatus::default()
    });
    session.print_text(&hoppi_first_start_install_prompt(), false)?;
    print!("Install Hoppi now? [y/N] ");
    io::stdout()
        .flush()
        .map_err(|error| format!("flush Hoppi install prompt: {error}"))?;

    let mut answer = String::new();
    io::stdin()
        .read_line(&mut answer)
        .map_err(|error| format!("read Hoppi install prompt: {error}"))?;
    let normalized = answer.trim().to_lowercase();
    if matches!(normalized.as_str(), "y" | "yes") {
        session.print_text("Installing Hoppi backend after explicit approval.", false)?;
        match install_hoppi_package() {
            Ok(message) => {
                session.memory_settings.enabled = true;
                session.memory_settings.hoppi_install_offer = "ask".to_string();
                save_memory_settings(&session.role_profile_path, &session.memory_settings)?;
                session.memory_status = Some(MemoryStatus {
                    enabled: true,
                    initialized: true,
                    backend: "hoppi".to_string(),
                    scope: "project".to_string(),
                    memory_count: 0,
                    ..MemoryStatus::default()
                });
                session.print_text(&message, false)
            }
            Err(error) => {
                session.memory_status = Some(MemoryStatus {
                    enabled: true,
                    initialized: false,
                    backend: "setup".to_string(),
                    scope: "project".to_string(),
                    memory_count: 0,
                    ..MemoryStatus::default()
                });
                session.print_text(&format!("Hoppi install failed: {error}"), false)
            }
        }
    } else {
        session.memory_settings.hoppi_install_offer = "dismissed".to_string();
        save_memory_settings(&session.role_profile_path, &session.memory_settings)?;
        session.print_text(
            "Okay - you can install Hoppi later with `/memory install-hoppi` or re-enable the startup offer with `/memory install-offer ask`.",
            false,
        )
    }
}

fn maybe_run_hoppi_startup_sync(
    session: &mut ShellSession,
    json_output: bool,
) -> Result<(), String> {
    maybe_run_hoppi_lifecycle_sync(session, HoppiSyncLifecycle::Startup, json_output)
}

fn maybe_run_hoppi_exit_sync(session: &mut ShellSession, json_output: bool) -> Result<(), String> {
    maybe_run_hoppi_lifecycle_sync(session, HoppiSyncLifecycle::Exit, json_output)
}

fn maybe_run_hoppi_lifecycle_sync(
    session: &mut ShellSession,
    lifecycle: HoppiSyncLifecycle,
    json_output: bool,
) -> Result<(), String> {
    let Some(direction) = hoppi_sync_lifecycle_direction(&session.memory_settings, lifecycle)
    else {
        return Ok(());
    };
    match session.run_hoppi_sync_with_job_reporting(direction, lifecycle.reason()) {
        Ok(run) => {
            let summary = format_hoppi_sync_report(direction, &run.result);
            if json_output {
                return session.print_value(
                    "memorySync",
                    json!({
                        "summary": summary,
                        "direction": direction.as_str(),
                        "reason": lifecycle.reason(),
                        "repoPath": format_path_for_display(&run.invocation.repo_path),
                        "remote": run.invocation.remote.clone(),
                        "result": hoppi_sync_result_json(&run.result),
                    }),
                    json_output,
                );
            }
            session.print_text(
                &format!("memory: {} sync {}", lifecycle.label(), summary),
                json_output,
            )
        }
        Err(error) => session.print_text(
            &format!("memory: {} sync skipped: {error}", lifecycle.label()),
            json_output,
        ),
    }
}

fn line_interactive_loop(
    session: &mut ShellSession,
    provider: &mut ProviderConfig,
    json: bool,
) -> Result<(), String> {
    let stdin = io::stdin();
    let mut input = String::new();
    let mut editor = LineEditor::default();
    loop {
        session.maybe_run_hoppi_background_sync(json)?;
        session.maybe_run_hoppi_idle_consolidation(json)?;
        session.maybe_offer_compaction(provider, json)?;
        if !json {
            session.render_docks();
            print!("\noppi> ");
            io::stdout()
                .flush()
                .map_err(|error| format!("flush prompt: {error}"))?;
        }
        input.clear();
        let bytes = stdin
            .read_line(&mut input)
            .map_err(|error| format!("read prompt: {error}"))?;
        let action = if bytes == 0 {
            editor.handle(EditorInput::CtrlD)
        } else {
            editor.handle(EditorInput::Text(
                input.trim_end_matches(['\r', '\n']).to_string(),
            ));
            editor.handle(EditorInput::Enter)
        };
        match action {
            EditorAction::Exit => break,
            EditorAction::Submit(prompt) => {
                let prompt = prompt.trim();
                if prompt.is_empty() {
                    continue;
                }
                if prompt.starts_with('/') {
                    if !session.handle_command(prompt, provider, json)? {
                        break;
                    }
                    session.drain_follow_ups(provider, json)?;
                    session.maybe_offer_compaction(provider, json)?;
                    continue;
                }
                if session.handle_login_picker_input(prompt, provider, json)? {
                    continue;
                }
                if session.is_turn_running() || session.has_pending_pause() {
                    session.queue_follow_up(prompt, json)?;
                    continue;
                }
                let _ = session.run_turn_for_role(prompt, provider, json, Some("executor"))?;
                session.drain_follow_ups(provider, json)?;
                session.maybe_offer_compaction(provider, json)?;
            }
            EditorAction::SubmitFollowUp(prompt) => {
                session.queue_follow_up(prompt.trim(), json)?;
                session.drain_follow_ups(provider, json)?;
                session.maybe_offer_compaction(provider, json)?;
            }
            EditorAction::Steer(prompt) => {
                session.steer_active_turn(prompt.trim(), json)?;
            }
            EditorAction::RestoreQueued => {
                if let Some(restored) = session.restore_latest_follow_up() {
                    editor.replace_buffer(restored);
                    session.print_text("restored queued message into editor", json)?;
                } else {
                    session.print_text("no queued message to restore", json)?;
                }
            }
            EditorAction::OpenSettings => {
                session.print_text("settings subpanels require the retained TUI; run without --no-tui or use /theme, /permissions, /model, /provider, /login, /tree, and /memory commands directly", json)?;
            }
            EditorAction::OpenTree => {
                session.handle_tree_command("/tree", Vec::new(), Some(provider), json)?;
            }
            EditorAction::OpenFork => {
                editor.replace_buffer("/fork ".to_string());
                session.print_text(
                    "double escape: /fork ready; add a title or press Enter",
                    json,
                )?;
            }
            EditorAction::EmptyEscape => {
                handle_raw_empty_escape_action(session, provider, json, &mut editor)?;
            }
            EditorAction::Interrupt => {
                session.interrupt_active_turn(json)?;
            }
            EditorAction::None | EditorAction::Cleared => continue,
        }
    }
    Ok(())
}

#[allow(dead_code)]
fn raw_interactive_loop(
    session: &mut ShellSession,
    provider: &mut ProviderConfig,
    json: bool,
) -> Result<(), String> {
    if !json {
        println!(
            "raw key mode: Enter submits/queues while busy, Shift+Enter newline, Alt+Enter queues, Ctrl+Enter steers, Alt+Up restores queued follow-up, Escape/Ctrl+C interrupt-or-clear, Ctrl+C twice runs /exit, Ctrl+D exits. {}",
            terminal_capability_summary(true)
        );
    }
    let (tx, rx) = mpsc::channel::<EditorInput>();
    spawn_raw_input_thread(tx);
    let mut editor = LineEditor::default();
    let mut input_open = true;
    session.refresh_native_usage(provider, false);
    loop {
        match rx.recv_timeout(POLL_INTERVAL) {
            Ok(input) => {
                let action = editor.handle(input);
                if !handle_raw_editor_action(session, provider, json, &mut editor, action)? {
                    break;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => input_open = false,
        }

        if let Some(outcome) = session.poll_turn_events(json)? {
            if matches!(outcome, TurnOutcome::Completed) {
                session.refresh_native_usage(provider, false);
                session.start_next_queued_or_goal_continuation(provider, json)?;
            }
        } else if session.maybe_timeout_pending_question(provider, json)? {
            session.refresh_native_usage(provider, false);
        } else if !session.is_turn_running() && !session.has_pending_pause() {
            session.start_next_queued_or_goal_continuation(provider, json)?;
        }
        session.maybe_run_hoppi_background_sync(json)?;
        session.maybe_offer_compaction(provider, json)?;

        if !input_open && !session.is_turn_running() {
            break;
        }
    }
    Ok(())
}

#[allow(dead_code)]
fn handle_raw_editor_action(
    session: &mut ShellSession,
    provider: &mut ProviderConfig,
    json: bool,
    editor: &mut LineEditor,
    action: EditorAction,
) -> Result<bool, String> {
    match action {
        EditorAction::None => Ok(true),
        EditorAction::Cleared => {
            session.print_text("editor cleared", json)?;
            Ok(true)
        }
        EditorAction::Exit => {
            if session.is_turn_running() {
                session.print_text(
                    "turn still running; press Ctrl+C/Escape to interrupt before Ctrl+D exit",
                    json,
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
            if prompt.starts_with('/') {
                return session.handle_command(prompt, provider, json);
            }
            if session.handle_login_picker_input(prompt, provider, json)? {
                return Ok(true);
            }
            if session.is_turn_running() || session.has_pending_pause() {
                session.queue_follow_up(prompt, json)?;
            } else {
                session.start_turn_for_role(prompt, provider, json, Some("executor"))?;
            }
            Ok(true)
        }
        EditorAction::SubmitFollowUp(prompt) => {
            session.queue_follow_up(prompt.trim(), json)?;
            if !session.is_turn_running() && !session.has_pending_pause() {
                session.start_next_queued_or_goal_continuation(provider, json)?;
            }
            Ok(true)
        }
        EditorAction::Steer(prompt) => {
            session.steer_active_turn(prompt.trim(), json)?;
            Ok(true)
        }
        EditorAction::RestoreQueued => {
            if let Some(restored) = session.restore_latest_follow_up() {
                editor.replace_buffer(restored);
                session.print_text(
                    &format!(
                        "restored queued message into editor: {}",
                        editor.buffer_preview()
                    ),
                    json,
                )?;
            } else {
                session.print_text("no queued message to restore", json)?;
            }
            Ok(true)
        }
        EditorAction::OpenSettings => {
            session.print_text(
                "settings panel is available in the retained TUI; use /settings or Ctrl+P there",
                json,
            )?;
            Ok(true)
        }
        EditorAction::OpenTree => {
            session.handle_tree_command("/tree", Vec::new(), Some(provider), json)?;
            Ok(true)
        }
        EditorAction::OpenFork => {
            editor.replace_buffer("/fork ".to_string());
            session.print_text(
                "double escape: /fork ready; add a title or press Enter",
                json,
            )?;
            Ok(true)
        }
        EditorAction::EmptyEscape => {
            handle_raw_empty_escape_action(session, provider, json, editor)?;
            Ok(true)
        }
        EditorAction::Interrupt => {
            session.interrupt_active_turn(json)?;
            Ok(true)
        }
    }
}

fn handle_raw_empty_escape_action(
    session: &mut ShellSession,
    provider: &mut ProviderConfig,
    json: bool,
    editor: &mut LineEditor,
) -> Result<(), String> {
    if session.is_turn_running() {
        return session.interrupt_active_turn(json);
    }
    match editor.resolve_empty_escape(session.double_escape_action, Instant::now()) {
        EditorAction::None => Ok(()),
        EditorAction::OpenTree => {
            session.handle_tree_command("/tree", Vec::new(), Some(provider), json)
        }
        EditorAction::OpenFork => {
            editor.replace_buffer("/fork ".to_string());
            session.print_text(
                "double escape: /fork ready; add a title or press Enter",
                json,
            )
        }
        other => handle_raw_editor_action(session, provider, json, editor, other).map(|_| ()),
    }
}

#[allow(dead_code)]
fn spawn_raw_input_thread(tx: mpsc::Sender<EditorInput>) {
    std::thread::spawn(move || {
        let stdin = io::stdin();
        let mut parser = RawInputParser::default();
        for byte in stdin.lock().bytes() {
            let Ok(byte) = byte else {
                break;
            };
            for input in parser.push(byte) {
                if tx.send(input).is_err() {
                    return;
                }
            }
        }
        for input in parser.finish() {
            if tx.send(input).is_err() {
                return;
            }
        }
    });
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
enum EditorInput {
    Text(String),
    Enter,
    ShiftEnter,
    AltEnter,
    CtrlEnter,
    AltUp,
    Escape,
    CtrlC,
    CtrlD,
    Backspace,
    CtrlBackspace,
    AltBackspace,
    Delete,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
    Tab,
    BackTab,
    CtrlP,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum EditorAction {
    None,
    Submit(String),
    SubmitFollowUp(String),
    Steer(String),
    RestoreQueued,
    OpenSettings,
    OpenTree,
    OpenFork,
    Interrupt,
    Cleared,
    EmptyEscape,
    Exit,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct LineEditor {
    buffer: String,
    cursor: usize,
    history: Vec<String>,
    history_index: Option<usize>,
    ctrl_c_armed: bool,
    last_empty_escape_at: Option<Instant>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EditorActionKind {
    Normal,
    FollowUp,
    Steer,
}

#[derive(Debug, Default)]
struct RawInputParser {
    escape: Vec<u8>,
}

impl RawInputParser {
    fn push(&mut self, byte: u8) -> Vec<EditorInput> {
        if !self.escape.is_empty() {
            self.escape.push(byte);
            if let Some(input) = parse_escape_sequence(&self.escape) {
                self.escape.clear();
                return vec![input];
            }
            if !escape_sequence_may_continue(&self.escape) {
                let mut output = vec![EditorInput::Escape];
                let replay = self.escape.split_off(1);
                self.escape.clear();
                for byte in replay {
                    output.extend(self.push(byte));
                }
                return output;
            }
            return Vec::new();
        }

        match byte {
            0x03 => vec![EditorInput::CtrlC],
            0x04 => vec![EditorInput::CtrlD],
            0x17 => vec![EditorInput::CtrlBackspace],
            0x08 | 0x7f => vec![EditorInput::Backspace],
            0x09 => vec![EditorInput::Tab],
            0x0d | 0x0a => vec![EditorInput::Enter],
            0x1b => {
                self.escape.push(byte);
                Vec::new()
            }
            byte if byte.is_ascii_control() => Vec::new(),
            byte => vec![EditorInput::Text((byte as char).to_string())],
        }
    }

    fn finish(&mut self) -> Vec<EditorInput> {
        if self.escape.is_empty() {
            Vec::new()
        } else {
            self.escape.clear();
            vec![EditorInput::Escape]
        }
    }
}

fn parse_escape_sequence(bytes: &[u8]) -> Option<EditorInput> {
    match bytes {
        [0x1b, 0x0d] | [0x1b, 0x0a] => Some(EditorInput::AltEnter),
        [0x1b, b'[', b'A'] => Some(EditorInput::Up),
        [0x1b, b'[', b'B'] => Some(EditorInput::Down),
        [0x1b, b'[', b'C'] => Some(EditorInput::Right),
        [0x1b, b'[', b'D'] => Some(EditorInput::Left),
        [0x1b, b'[', b'H'] | [0x1b, b'O', b'H'] => Some(EditorInput::Home),
        [0x1b, b'[', b'F'] | [0x1b, b'O', b'F'] => Some(EditorInput::End),
        [0x1b, 0x08] | [0x1b, 0x7f] => Some(EditorInput::AltBackspace),
        [0x1b, b'[', b'1', b'2', b'7', b';', b'5', b'u'] | [0x1b, b'[', b'8', b';', b'5', b'u'] => {
            Some(EditorInput::CtrlBackspace)
        }
        [0x1b, b'[', b'3', b'~'] => Some(EditorInput::Delete),
        [0x1b, b'[', b'5', b'~'] => Some(EditorInput::PageUp),
        [0x1b, b'[', b'6', b'~'] => Some(EditorInput::PageDown),
        [0x1b, b'[', b'Z'] => Some(EditorInput::BackTab),
        [0x1b, 0x1b, b'[', b'A'] | [0x1b, b'[', b'1', b';', b'3', b'A'] => Some(EditorInput::AltUp),
        [0x1b, b'[', b'1', b'3', b';', b'2', b'u']
        | [0x1b, b'[', b'2', b'7', b';', b'2', b';', b'1', b'3', b'~'] => {
            Some(EditorInput::ShiftEnter)
        }
        [0x1b, b'[', b'1', b'3', b';', b'5', b'u']
        | [0x1b, b'[', b'2', b'7', b';', b'5', b';', b'1', b'3', b'~'] => {
            Some(EditorInput::CtrlEnter)
        }
        _ => None,
    }
}

fn escape_sequence_may_continue(bytes: &[u8]) -> bool {
    const CANDIDATES: [&[u8]; 23] = [
        &[0x1b, 0x0d],
        &[0x1b, 0x0a],
        &[0x1b, b'[', b'A'],
        &[0x1b, b'[', b'B'],
        &[0x1b, b'[', b'C'],
        &[0x1b, b'[', b'D'],
        &[0x1b, b'[', b'H'],
        &[0x1b, b'[', b'F'],
        &[0x1b, b'O', b'H'],
        &[0x1b, b'O', b'F'],
        &[0x1b, 0x08],
        &[0x1b, 0x7f],
        &[0x1b, b'[', b'1', b'2', b'7', b';', b'5', b'u'],
        &[0x1b, b'[', b'8', b';', b'5', b'u'],
        &[0x1b, b'[', b'3', b'~'],
        &[0x1b, b'[', b'5', b'~'],
        &[0x1b, b'[', b'6', b'~'],
        &[0x1b, b'[', b'Z'],
        &[0x1b, 0x1b, b'[', b'A'],
        &[0x1b, b'[', b'1', b';', b'3', b'A'],
        &[0x1b, b'[', b'1', b'3', b';', b'2', b'u'],
        &[0x1b, b'[', b'1', b'3', b';', b'5', b'u'],
        &[0x1b, b'[', b'2', b'7'],
    ];
    CANDIDATES
        .iter()
        .any(|candidate| candidate.starts_with(bytes))
        || matches!(
            bytes,
            [0x1b, b'[', b'2']
                | [0x1b, b'[', b'2', b'7']
                | [0x1b, b'[', b'2', b'7', b';']
                | [0x1b, b'[', b'2', b'7', b';', b'2']
                | [0x1b, b'[', b'2', b'7', b';', b'5']
                | [0x1b, b'[', b'2', b'7', b';', b'2', b';']
                | [0x1b, b'[', b'2', b'7', b';', b'5', b';']
                | [0x1b, b'[', b'2', b'7', b';', b'2', b';', b'1']
                | [0x1b, b'[', b'2', b'7', b';', b'5', b';', b'1']
                | [0x1b, b'[', b'2', b'7', b';', b'2', b';', b'1', b'3']
                | [0x1b, b'[', b'2', b'7', b';', b'5', b';', b'1', b'3']
        )
}

impl LineEditor {
    fn handle(&mut self, input: EditorInput) -> EditorAction {
        if !matches!(input, EditorInput::CtrlC) {
            self.ctrl_c_armed = false;
        }
        if !matches!(input, EditorInput::Escape) {
            self.last_empty_escape_at = None;
        }
        match input {
            EditorInput::Text(text) => {
                self.insert_text(&text);
                EditorAction::None
            }
            EditorInput::ShiftEnter => {
                self.insert_text("\n");
                EditorAction::None
            }
            EditorInput::Enter => self.submit_as(EditorActionKind::Normal),
            EditorInput::AltEnter => self.submit_as(EditorActionKind::FollowUp),
            EditorInput::CtrlEnter => self.submit_as(EditorActionKind::Steer),
            EditorInput::AltUp => EditorAction::RestoreQueued,
            EditorInput::CtrlP => EditorAction::OpenSettings,
            EditorInput::Backspace => {
                self.delete_backward();
                EditorAction::None
            }
            EditorInput::CtrlBackspace => {
                self.delete_word_backward();
                EditorAction::None
            }
            EditorInput::AltBackspace => {
                self.delete_current_line();
                EditorAction::None
            }
            EditorInput::Delete => {
                self.delete_forward();
                EditorAction::None
            }
            EditorInput::Left => {
                self.move_left();
                EditorAction::None
            }
            EditorInput::Right => {
                self.move_right();
                EditorAction::None
            }
            EditorInput::Home => {
                self.move_start();
                EditorAction::None
            }
            EditorInput::End => {
                self.move_end();
                EditorAction::None
            }
            EditorInput::Up | EditorInput::PageUp => {
                self.history_prev();
                EditorAction::None
            }
            EditorInput::Down | EditorInput::PageDown => {
                self.history_next();
                EditorAction::None
            }
            EditorInput::Tab | EditorInput::BackTab => EditorAction::None,
            EditorInput::Escape => {
                if self.buffer.is_empty() {
                    EditorAction::EmptyEscape
                } else {
                    self.clear();
                    self.last_empty_escape_at = None;
                    EditorAction::Cleared
                }
            }
            EditorInput::CtrlC => {
                if self.ctrl_c_armed {
                    self.clear();
                    EditorAction::Submit("/exit".to_string())
                } else {
                    self.ctrl_c_armed = true;
                    if self.buffer.is_empty() {
                        EditorAction::Interrupt
                    } else {
                        self.clear();
                        self.ctrl_c_armed = true;
                        EditorAction::Cleared
                    }
                }
            }
            EditorInput::CtrlD => {
                if self.buffer.is_empty() {
                    EditorAction::Exit
                } else {
                    self.clear();
                    EditorAction::Cleared
                }
            }
        }
    }

    fn resolve_empty_escape(&mut self, action: DoubleEscapeAction, now: Instant) -> EditorAction {
        if action == DoubleEscapeAction::None {
            self.last_empty_escape_at = None;
            return EditorAction::None;
        }
        let is_double = self
            .last_empty_escape_at
            .is_some_and(|last| now.duration_since(last) < DOUBLE_ESCAPE_WINDOW);
        if is_double {
            self.last_empty_escape_at = None;
            match action {
                DoubleEscapeAction::Tree => EditorAction::OpenTree,
                DoubleEscapeAction::Fork => EditorAction::OpenFork,
                DoubleEscapeAction::None => EditorAction::None,
            }
        } else {
            self.last_empty_escape_at = Some(now);
            EditorAction::None
        }
    }

    fn submit_as(&mut self, kind: EditorActionKind) -> EditorAction {
        let submitted = self.buffer.trim_end().to_string();
        self.clear();
        if submitted.trim().is_empty() {
            return EditorAction::None;
        }
        self.push_history(&submitted);
        match kind {
            EditorActionKind::Normal => EditorAction::Submit(submitted),
            EditorActionKind::FollowUp => EditorAction::SubmitFollowUp(submitted),
            EditorActionKind::Steer => EditorAction::Steer(submitted),
        }
    }

    fn insert_text(&mut self, text: &str) {
        self.cursor = self.cursor.min(self.buffer.len());
        self.buffer.insert_str(self.cursor, text);
        self.cursor += text.len();
        self.history_index = None;
    }

    fn delete_backward(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let previous = previous_char_boundary(&self.buffer, self.cursor);
        self.buffer.drain(previous..self.cursor);
        self.cursor = previous;
        self.history_index = None;
    }

    fn delete_forward(&mut self) {
        if self.cursor >= self.buffer.len() {
            return;
        }
        let next = next_char_boundary(&self.buffer, self.cursor);
        self.buffer.drain(self.cursor..next);
        self.history_index = None;
    }

    fn delete_word_backward(&mut self) {
        let cursor = self.cursor.min(self.buffer.len());
        if cursor == 0 {
            return;
        }
        let mut start = cursor;
        while start > 0 {
            let previous = previous_char_boundary(&self.buffer, start);
            let ch = self.buffer[previous..start].chars().next().unwrap_or(' ');
            if ch.is_whitespace() {
                start = previous;
            } else {
                break;
            }
        }
        while start > 0 {
            let previous = previous_char_boundary(&self.buffer, start);
            let ch = self.buffer[previous..start].chars().next().unwrap_or(' ');
            if ch.is_whitespace() {
                break;
            }
            start = previous;
        }
        self.buffer.drain(start..cursor);
        self.cursor = start;
        self.history_index = None;
    }

    fn delete_current_line(&mut self) {
        if self.buffer.is_empty() {
            return;
        }
        let cursor = self.cursor.min(self.buffer.len());
        let start = self.buffer[..cursor]
            .rfind('\n')
            .map(|index| index + 1)
            .unwrap_or(0);
        let end = self.buffer[cursor..]
            .find('\n')
            .map(|offset| cursor + offset)
            .unwrap_or(self.buffer.len());
        self.buffer.drain(start..end);
        self.cursor = start;
        self.history_index = None;
    }

    fn move_left(&mut self) {
        self.cursor = previous_char_boundary(&self.buffer, self.cursor);
    }

    fn move_right(&mut self) {
        self.cursor = next_char_boundary(&self.buffer, self.cursor);
    }

    fn move_start(&mut self) {
        self.cursor = self.buffer[..self.cursor.min(self.buffer.len())]
            .rfind('\n')
            .map(|index| index + 1)
            .unwrap_or(0);
    }

    fn move_end(&mut self) {
        let cursor = self.cursor.min(self.buffer.len());
        self.cursor = self.buffer[cursor..]
            .find('\n')
            .map(|offset| cursor + offset)
            .unwrap_or(self.buffer.len());
    }

    fn history_prev(&mut self) {
        if self.history.is_empty() {
            return;
        }
        let index = self
            .history_index
            .map(|index| index.saturating_sub(1))
            .unwrap_or_else(|| self.history.len().saturating_sub(1));
        self.history_index = Some(index);
        self.replace_buffer(self.history[index].clone());
    }

    fn history_next(&mut self) {
        let Some(index) = self.history_index else {
            return;
        };
        if index + 1 >= self.history.len() {
            self.history_index = None;
            self.replace_buffer(String::new());
        } else {
            let next = index + 1;
            self.history_index = Some(next);
            self.replace_buffer(self.history[next].clone());
        }
    }

    fn push_history(&mut self, submitted: &str) {
        if self.history.last().is_some_and(|last| last == submitted) {
            self.history_index = None;
            return;
        }
        self.history.push(submitted.to_string());
        if self.history.len() > 200 {
            self.history.remove(0);
        }
        self.history_index = None;
    }

    fn clear(&mut self) {
        self.buffer.clear();
        self.cursor = 0;
        self.history_index = None;
        self.ctrl_c_armed = false;
    }

    fn arm_ctrl_c_exit(&mut self) {
        self.ctrl_c_armed = true;
    }

    fn ctrl_c_exit_armed(&self) -> bool {
        self.ctrl_c_armed
    }

    fn replace_buffer(&mut self, text: String) {
        self.buffer = text;
        self.cursor = self.buffer.len();
        self.history_index = None;
    }

    fn buffer_preview(&self) -> &str {
        &self.buffer
    }

    fn cursor(&self) -> usize {
        self.cursor.min(self.buffer.len())
    }
}

fn previous_char_boundary(text: &str, cursor: usize) -> usize {
    let cursor = cursor.min(text.len());
    if cursor == 0 {
        return 0;
    }
    text[..cursor]
        .char_indices()
        .last()
        .map(|(index, _)| index)
        .unwrap_or(0)
}

fn next_char_boundary(text: &str, cursor: usize) -> usize {
    let cursor = cursor.min(text.len());
    if cursor >= text.len() {
        return text.len();
    }
    text[cursor..]
        .char_indices()
        .nth(1)
        .map(|(offset, _)| cursor + offset)
        .unwrap_or(text.len())
}

