#[cfg(test)]
mod tests {
    use super::*;

    fn fake_codex_access_token(account_id: &str) -> String {
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"none"}"#);
        let payload = URL_SAFE_NO_PAD.encode(
            format!(r#"{{"https://api.openai.com/auth":{{"chatgpt_account_id":"{account_id}"}}}}"#)
                .as_bytes(),
        );
        format!("{header}.{payload}.sig")
    }

    #[test]
    fn parses_mock_prompt() {
        let parsed = parse_args(vec!["--mock".into(), "hello".into(), "world".into()]).unwrap();
        assert_eq!(parsed.initial_prompt.as_deref(), Some("hello world"));
        assert!(matches!(parsed.provider, ProviderConfig::Mock));
        assert!(!parsed.json);
        assert!(!parsed.interactive);
    }

    #[test]
    fn parses_interactive_without_prompt() {
        let parsed = parse_args(vec!["--mock".into()]).unwrap();
        assert_eq!(parsed.initial_prompt, None);
        assert!(parsed.interactive);
    }

    #[test]
    fn parses_raw_mode_flag() {
        let parsed = parse_args(vec!["--mock".into(), "--raw".into()]).unwrap();
        assert!(parsed.raw);
        assert!(parsed.interactive);
    }

    #[test]
    fn parses_ratatui_as_default_ui() {
        let parsed = parse_args(vec!["--mock".into()]).unwrap();
        assert!(parsed.ratatui);
    }

    #[test]
    fn parses_ratatui_preview_flag() {
        let parsed = parse_args(vec!["--mock".into(), "--ratatui".into()]).unwrap();
        assert!(parsed.raw);
        assert!(parsed.ratatui);
        assert!(parsed.interactive);
    }

    #[test]
    fn parses_no_ratatui_fallback_flag() {
        let parsed = parse_args(vec!["--mock".into(), "--no-ratatui".into()]).unwrap();
        assert!(!parsed.ratatui);
    }

    #[test]
    fn parses_resume_thread_flag() {
        let parsed =
            parse_args(vec!["--mock".into(), "--resume".into(), "thread-7".into()]).unwrap();
        assert_eq!(parsed.resume_thread.as_deref(), Some("thread-7"));
    }

    #[test]
    fn parses_session_list_flag_as_non_interactive() {
        let parsed = parse_args(vec!["--mock".into(), "--list-sessions".into()]).unwrap();
        assert!(parsed.list_sessions);
        assert!(!parsed.interactive);
    }

    #[test]
    fn parses_session_list_flag_defaults_to_mock_when_no_provider_flag() {
        let parsed = parse_args(vec!["--list-sessions".into()]).unwrap();
        assert!(parsed.list_sessions);
        assert!(!parsed.interactive);
        assert!(matches!(parsed.provider, ProviderConfig::Mock));
    }

    #[test]
    fn dogfood_background_command_uses_portable_shell_marker() {
        let command = dogfood_background_command();
        assert!(command.contains("oppi-background-dogfood"));
        assert!(!command.contains("node -e"));
        if cfg!(windows) {
            assert!(command.contains("ping -n"));
        } else {
            assert!(command.starts_with("/bin/echo "));
            assert!(command.contains("; sleep "));
        }
    }

    #[test]
    fn parses_real_provider_flags() {
        let parsed = parse_args(vec![
            "--model".into(),
            "gpt-test".into(),
            "--base-url".into(),
            "http://127.0.0.1:3000/v1".into(),
            "--api-key-env".into(),
            "OPPI_TEST_API_KEY".into(),
            "--no-stream".into(),
            "hello".into(),
        ])
        .unwrap();
        match parsed.provider {
            ProviderConfig::OpenAiCompatible(config) => {
                assert_eq!(config.model, "gpt-test");
                assert_eq!(config.base_url.as_deref(), Some("http://127.0.0.1:3000/v1"));
                assert_eq!(config.api_key_env.as_deref(), Some("OPPI_TEST_API_KEY"));
                assert!(!config.stream);
            }
            ProviderConfig::Mock => panic!("expected real provider"),
        }
        assert_eq!(parsed.initial_prompt.as_deref(), Some("hello"));
    }

    #[test]
    fn rejects_provider_without_model() {
        let error = parse_args(vec!["--api-key-env".into(), "OPPI_TEST_API_KEY".into()])
            .expect_err("missing model should fail");
        assert!(error.contains("--model"));
    }

    #[test]
    fn renders_core_events() {
        let event = Event {
            id: 1,
            thread_id: "thread-1".to_string(),
            turn_id: Some("turn-1".to_string()),
            created_at_ms: 1,
            kind: EventKind::ItemDelta {
                item_id: "item-1".to_string(),
                delta: "hi".to_string(),
            },
        };
        assert_eq!(render_event_line(&event), None);

        let agent_started = Event {
            id: 2,
            thread_id: "thread-1".to_string(),
            turn_id: Some("turn-1".to_string()),
            created_at_ms: 2,
            kind: EventKind::AgentStarted {
                run: AgentRun {
                    id: "agent-run-1".to_string(),
                    thread_id: "thread-1".to_string(),
                    parent_thread_id: None,
                    child_thread_id: None,
                    child_turn_id: None,
                    agent_name: "general-purpose".to_string(),
                    status: oppi_protocol::AgentRunStatus::Running,
                    task: "inspect".to_string(),
                    worktree_root: None,
                    execution_root: None,
                    background: false,
                    role: Some("subagent".to_string()),
                    model: Some("gpt-sub".to_string()),
                    effort: Some("medium".to_string()),
                    permission_mode: Some(PermissionMode::ReadOnly),
                    memory_mode: Some("disabled".to_string()),
                    tool_allowlist: vec!["read_file".to_string()],
                    tool_denylist: vec!["shell_exec".to_string()],
                    isolation: Some("thread".to_string()),
                    color: Some("cyan".to_string()),
                    skills: vec!["independent".to_string()],
                    max_turns: Some(2),
                },
            },
        };
        let rendered = render_event_line(&agent_started).unwrap();
        assert!(rendered.contains("agent-run-1"));
        assert!(rendered.contains("role=subagent"));
        assert!(rendered.contains("permissions=read-only"));
        assert!(rendered.contains("tools=read_file"));

        let memory_panel = format_memory_control(&json!({
            "title": "Hoppi memory dashboard",
            "summary": "runtime-owned backend broker; default adapter is bounded Node/TypeScript",
            "status": {
                "enabled": true,
                "initialized": true,
                "backend": "hoppi",
                "scope": "project",
                "memoryCount": 3,
                "pinnedCount": 1,
                "storePath": "C:/Users/matth/.oppi/hoppi",
                "dashboardUrl": "http://hoppi.localhost:49152",
                "executionOwner": "runtime",
                "executionAdapter": "hoppi-node-bridge"
            },
            "controls": [
                { "id": "maintenance", "label": "Preview maintenance", "command": "/memory maintenance dry-run", "description": "Explicit only" }
            ]
        }));
        assert!(memory_panel.contains("Hoppi memory dashboard"));
        assert!(memory_panel.contains("status: enabled backend=hoppi"));
        assert!(memory_panel.contains("owner=runtime"));
        assert!(memory_panel.contains("adapter=hoppi-node-bridge"));
        assert!(memory_panel.contains("initialized"));
        assert!(memory_panel.contains("pinned=1"));
        assert!(memory_panel.contains("store=C:/Users/matth/.oppi/hoppi"));
        assert!(memory_panel.contains("dashboard=http://hoppi.localhost:49152"));
        assert!(memory_panel.contains("/memory maintenance dry-run"));

        let dashboard_panel = format_memory_dashboard(&json!({
            "available": true,
            "url": "http://hoppi.localhost:49152",
            "fallbackUrl": "http://localhost:49152",
            "message": "Hoppi dashboard is available from runtime memory status.",
            "status": {
                "memoryCount": 3,
                "pinnedCount": 1,
                "storePath": "C:/Users/matth/.oppi/hoppi"
            }
        }));
        assert!(dashboard_panel.contains("Hoppi dashboard: http://hoppi.localhost:49152"));
        assert!(dashboard_panel.contains("Fallback: http://localhost:49152"));
        assert!(dashboard_panel.contains("memories=3 pinned=1"));
        assert!(dashboard_panel.contains("store: C:/Users/matth/.oppi/hoppi"));

        let status_summary = format_memory_status_summary(&json!({
            "enabled": true,
            "backend": "hoppi",
            "scope": "project",
            "memoryCount": 3,
            "executionOwner": "runtime",
            "executionAdapter": "hoppi-node-bridge"
        }));
        assert!(status_summary.contains("owner runtime"));
        assert!(status_summary.contains("adapter hoppi-node-bridge"));
    }

    #[test]
    fn tool_result_renderer_summarizes_running_agent_handle() {
        let call = ToolCall {
            id: "call-agent-1".to_string(),
            name: "AgentTool".to_string(),
            namespace: Some("oppi".to_string()),
            arguments: json!({
                "agent": "verification",
                "task": "verify slowly",
                "background": true
            }),
        };
        let result = oppi_protocol::ToolResult {
            call_id: call.id.clone(),
            status: ToolResultStatus::Ok,
            output: Some(
                json!({
                    "status": "running",
                    "agentName": "verification",
                    "runId": "agent-run-7",
                    "executionRoot": "E:\\RepositoryV2\\oppi\\worktrees\\verification"
                })
                .to_string(),
            ),
            error: None,
        };

        let rendered = format_tool_result_digest(&result, Some(&call), theme_tokens("plain"));

        assert_eq!(
            rendered,
            "agent verification running as agent-run-7 in E:\\RepositoryV2\\oppi\\worktrees\\verification"
        );
    }

    #[test]
    fn formats_btw_side_question_result_with_side_thread_id() {
        let rendered = format_side_question_result(&json!({
            "sideThreadId": "thread-side-7",
            "sideTurnId": "turn-side-9",
            "answer": "Guardian sees bounded context."
        }))
        .unwrap();

        assert_eq!(
            rendered,
            "btw thread-side-7\nGuardian sees bounded context."
        );
    }

    #[test]
    fn btw_request_params_include_provider_sandbox_and_blocking_mode() {
        let provider = ProviderConfig::OpenAiCompatible(OpenAiCompatibleConfig {
            flavor: DirectProviderFlavor::OpenAiCodex,
            provider_id: None,
            model: "gpt-test".to_string(),
            base_url: None,
            api_key_env: Some("OPPI_TEST_API_KEY".to_string()),
            system_prompt: None,
            temperature: None,
            reasoning_effort: Some("medium".to_string()),
            max_output_tokens: None,
            stream: true,
        });

        let params = side_question_request_params(
            "thread-1",
            "What context does Guardian get?",
            &provider,
            "E:\\RepositoryV2\\oppi",
            PermissionMode::AutoReview,
            &PermissionSandboxSettings {
                allow_internet: true,
                project_roots: vec!["E:\\RepositoryV2\\oppi".to_string()],
                readable_roots: vec![
                    "E:\\RepositoryV2\\oppi".to_string(),
                    "E:\\RepositoryV2\\shared-read".to_string(),
                ],
                ..PermissionSandboxSettings::default()
            },
            "off",
        );

        assert_eq!(params["threadId"], json!("thread-1"));
        assert_eq!(params["question"], json!("What context does Guardian get?"));
        assert_eq!(params["executionMode"], json!("blocking"));
        assert_eq!(
            params["sandboxPolicy"]["permissionProfile"]["mode"],
            json!("auto-review")
        );
        assert_eq!(params["sandboxPolicy"]["network"], json!("enabled"));
        assert_eq!(
            params["sandboxPolicy"]["permissionProfile"]["readableRoots"],
            json!(["E:\\RepositoryV2\\oppi", "E:\\RepositoryV2\\shared-read"])
        );
        assert_eq!(
            params["sandboxPolicy"]["permissionProfile"]["writableRoots"],
            json!(["E:\\RepositoryV2\\oppi"])
        );
        assert_eq!(params["modelProvider"]["model"], json!("gpt-test"));
        assert_eq!(params["maxContinuations"], json!(8));
    }

    #[test]
    fn editor_handles_submit_multiline_escape_and_ctrl_d() {
        let mut editor = LineEditor::default();
        assert_eq!(
            editor.handle(EditorInput::Text("hello".to_string())),
            EditorAction::None
        );
        assert_eq!(editor.handle(EditorInput::ShiftEnter), EditorAction::None);
        assert_eq!(
            editor.handle(EditorInput::Text("world".to_string())),
            EditorAction::None
        );
        assert_eq!(
            editor.handle(EditorInput::Enter),
            EditorAction::Submit("hello\nworld".to_string())
        );
        assert_eq!(
            editor.handle(EditorInput::Text("draft".to_string())),
            EditorAction::None
        );
        assert_eq!(editor.handle(EditorInput::Escape), EditorAction::Cleared);
        assert_eq!(
            editor.handle(EditorInput::Escape),
            EditorAction::EmptyEscape
        );
        let now = Instant::now();
        assert_eq!(
            editor.resolve_empty_escape(DoubleEscapeAction::Tree, now),
            EditorAction::None
        );
        assert_eq!(
            editor.resolve_empty_escape(DoubleEscapeAction::Tree, now + Duration::from_millis(100)),
            EditorAction::OpenTree
        );
        assert_eq!(
            editor.handle(EditorInput::Escape),
            EditorAction::EmptyEscape
        );
        assert_eq!(
            editor.resolve_empty_escape(DoubleEscapeAction::Fork, now + Duration::from_secs(1)),
            EditorAction::None
        );
        assert_eq!(
            editor.handle(EditorInput::Escape),
            EditorAction::EmptyEscape
        );
        assert_eq!(
            editor.resolve_empty_escape(
                DoubleEscapeAction::Fork,
                now + Duration::from_secs(1) + Duration::from_millis(100)
            ),
            EditorAction::OpenFork
        );
        assert_eq!(
            editor.handle(EditorInput::Escape),
            EditorAction::EmptyEscape
        );
        assert_eq!(
            editor.resolve_empty_escape(DoubleEscapeAction::None, now + Duration::from_secs(2)),
            EditorAction::None
        );
        assert_eq!(editor.handle(EditorInput::CtrlD), EditorAction::Exit);

        assert_eq!(
            editor.handle(EditorInput::Text("queue me".to_string())),
            EditorAction::None
        );
        assert_eq!(
            editor.handle(EditorInput::AltEnter),
            EditorAction::SubmitFollowUp("queue me".to_string())
        );
        assert_eq!(
            editor.handle(EditorInput::Text("steer me".to_string())),
            EditorAction::None
        );
        assert_eq!(
            editor.handle(EditorInput::CtrlEnter),
            EditorAction::Steer("steer me".to_string())
        );
        assert_eq!(
            editor.handle(EditorInput::AltUp),
            EditorAction::RestoreQueued
        );
        assert_eq!(editor.handle(EditorInput::CtrlC), EditorAction::Interrupt);
        assert_eq!(
            editor.handle(EditorInput::CtrlC),
            EditorAction::Submit("/exit".to_string())
        );

        let mut editor = LineEditor::default();
        assert_eq!(
            editor.handle(EditorInput::Text("draft".to_string())),
            EditorAction::None
        );
        assert_eq!(editor.handle(EditorInput::CtrlC), EditorAction::Cleared);
        assert_eq!(
            editor.handle(EditorInput::CtrlC),
            EditorAction::Submit("/exit".to_string())
        );

        let mut editor = LineEditor::default();
        assert_eq!(editor.handle(EditorInput::CtrlC), EditorAction::Interrupt);
        assert_eq!(
            editor.handle(EditorInput::Text("keep running".to_string())),
            EditorAction::None
        );
        assert_eq!(editor.handle(EditorInput::CtrlC), EditorAction::Cleared);
    }

    #[test]
    fn editor_ctrl_backspace_deletes_word_and_alt_backspace_deletes_line() {
        let mut editor = LineEditor::default();
        assert_eq!(
            editor.handle(EditorInput::Text("hello brave world".to_string())),
            EditorAction::None
        );
        assert_eq!(
            editor.handle(EditorInput::CtrlBackspace),
            EditorAction::None
        );
        assert_eq!(editor.buffer_preview(), "hello brave ");
        assert_eq!(
            editor.handle(EditorInput::CtrlBackspace),
            EditorAction::None
        );
        assert_eq!(editor.buffer_preview(), "hello ");
        assert_eq!(editor.handle(EditorInput::AltBackspace), EditorAction::None);
        assert_eq!(editor.buffer_preview(), "");

        assert_eq!(
            editor.handle(EditorInput::Text("one\ntwo three\nfour".to_string())),
            EditorAction::None
        );
        editor.move_start();
        assert_eq!(editor.handle(EditorInput::AltBackspace), EditorAction::None);
        assert_eq!(editor.buffer_preview(), "one\ntwo three\n");
    }

    #[test]
    fn raw_key_parser_maps_supported_sequences() {
        let mut parser = RawInputParser::default();
        let inputs = b"hi\x1b\rthere\x1b[13;2u\x1b[13;5u\x1b[1;3A\x17\x1b\x7f\x03\x04"
            .iter()
            .flat_map(|byte| parser.push(*byte))
            .collect::<Vec<_>>();
        assert!(inputs.contains(&EditorInput::Text("h".to_string())));
        assert!(inputs.contains(&EditorInput::AltEnter));
        assert!(inputs.contains(&EditorInput::ShiftEnter));
        assert!(inputs.contains(&EditorInput::CtrlEnter));
        assert!(inputs.contains(&EditorInput::AltUp));
        assert!(inputs.contains(&EditorInput::CtrlBackspace));
        assert!(inputs.contains(&EditorInput::AltBackspace));
        assert!(inputs.contains(&EditorInput::CtrlC));
        assert!(inputs.contains(&EditorInput::CtrlD));
        assert_eq!(RawInputParser::default().push(0x09), vec![EditorInput::Tab]);
        assert_eq!(parser.finish(), Vec::<EditorInput>::new());
    }

    fn test_skill(
        name: &str,
        description: &str,
        source: oppi_protocol::SkillSource,
    ) -> oppi_protocol::ResolvedSkill {
        oppi_protocol::ResolvedSkill {
            active: oppi_protocol::SkillRef {
                id: format!("skill:{name}"),
                name: name.to_string(),
                description: description.to_string(),
                source,
                path: None,
                priority: 0,
            },
            shadowed: Vec::new(),
        }
    }

    #[test]
    fn skill_slash_suggestions_include_active_skills_when_enabled() {
        let skills = vec![
            test_skill(
                "imagegen",
                "Generate or edit images",
                oppi_protocol::SkillSource::BuiltIn,
            ),
            test_skill(
                "mermaid-diagrams",
                "Create concise Mermaid diagrams",
                oppi_protocol::SkillSource::Plugin,
            ),
        ];

        let items = skill_slash_command_items(&skills, true, "/skill:i");

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].label, "/skill:imagegen");
        assert_eq!(items[0].insert, "/skill:imagegen ");
        assert!(items[0].detail.contains("built-in"));
        assert!(items[0].detail.contains("Generate or edit images"));
        assert!(skill_slash_command_items(&skills, false, "/skill:").is_empty());
    }

    #[test]
    fn slash_palette_lists_dynamic_skill_commands_from_runtime_cache() {
        let skills = vec![
            test_skill(
                "imagegen",
                "Generate or edit images",
                oppi_protocol::SkillSource::BuiltIn,
            ),
            test_skill(
                "mermaid-diagrams",
                "Create concise Mermaid diagrams",
                oppi_protocol::SkillSource::Project,
            ),
        ];

        let palette =
            slash_palette_for_buffer_with_skills("/skill:m", 0, &skills, true).expect("palette");

        assert_eq!(palette.mode, SlashPaletteMode::Commands);
        assert_eq!(palette.items.len(), 1);
        assert_eq!(palette.items[0].insert, "/skill:mermaid-diagrams ");
        assert!(palette.items[0].detail.contains("project"));
        assert!(
            palette.items[0]
                .detail
                .contains("Create concise Mermaid diagrams")
        );
    }

    #[test]
    fn slash_palette_lists_commands_filters_and_accepts_arguments() {
        let all = slash_palette_for_buffer("/", 0).expect("slash palette should open");
        assert_eq!(all.mode, SlashPaletteMode::Commands);
        assert_eq!(all.items.len(), visible_slash_command_specs().len());
        assert!(all.items.iter().any(|item| item.insert == "/permissions"));
        assert!(all.items.iter().any(|item| item.insert == "/audit "));
        assert!(all.items.iter().any(|item| item.insert == "/btw "));
        assert!(!all.items.iter().any(|item| item.insert == "/provider"));
        assert!(
            all.items
                .iter()
                .any(|item| item.insert == "/feature-request ")
        );

        let filtered = slash_palette_for_buffer("/per", 0).expect("filtered palette");
        assert!(
            filtered
                .items
                .iter()
                .any(|item| item.insert == "/permissions")
        );
        assert_eq!(
            slash_palette_accept("/per", 0, false, true),
            Some(SlashPaletteAccept::Submit("/permissions".to_string()))
        );
        assert_eq!(
            slash_palette_accept("/per", 0, false, false),
            Some(SlashPaletteAccept::Insert("/permissions".to_string()))
        );

        let provider_args = slash_palette_for_buffer("/provider ", 1).expect("provider args");
        assert_eq!(provider_args.mode, SlashPaletteMode::Arguments);
        assert!(
            provider_args
                .items
                .iter()
                .any(|item| item.insert == "/provider validate")
        );
        assert_eq!(
            slash_palette_accept("/provider v", 0, false, true),
            Some(SlashPaletteAccept::Submit("/provider validate".to_string()))
        );
        assert_eq!(
            slash_palette_accept("/provider v", 0, false, false),
            Some(SlashPaletteAccept::Insert("/provider validate".to_string()))
        );
        assert_eq!(
            slash_palette_accept("/provider validate", 0, false, true),
            None
        );
        assert_eq!(
            slash_palette_accept("/effor", 0, false, false),
            Some(SlashPaletteAccept::Insert("/effort".to_string()))
        );
        assert_eq!(slash_palette_accept("/effort", 0, false, true), None);
        assert!(
            slash_palette_for_buffer("/auth", 0)
                .expect("fuzzy auth search")
                .items
                .iter()
                .any(|item| item.insert == "/login")
        );

        let help = slash_command_help_text();
        assert!(help.contains("/login [subscription|api]"));
        assert!(
            help.contains(
                "/permissions [status|history|timeout|reviewer-model|clear-session|mode]"
            )
        );
        assert!(!help.lines().any(|line| line.starts_with("- /commands")));
        assert!(help.contains("Tab completes"));
    }

    #[test]
    fn sandbox_is_compatibility_route_not_visible_slash_surface() {
        assert!(
            find_slash_command_spec("/sandbox").is_none(),
            "/sandbox should not be registered as a top-level slash command"
        );
        assert!(
            !visible_slash_command_specs()
                .iter()
                .any(|spec| spec.command == "/sandbox"
                    || spec.aliases.iter().any(|alias| *alias == "/sandbox"))
        );
        assert!(
            slash_arg_suggestions("/sandbox", "").is_empty(),
            "/sandbox should not advertise its own argument completions"
        );

        let all = slash_palette_for_buffer("/", 0).expect("slash palette should open");
        assert!(!all.items.iter().any(|item| item.insert == "/sandbox"));
        let help = slash_command_help_text();
        assert!(!help.contains("/sandbox"));
    }

    #[test]
    fn slash_palette_includes_pi_package_compatibility_commands() {
        let idle_compact_args = slash_arg_suggestions("/idle-compact", "");
        assert!(
            idle_compact_args
                .iter()
                .any(|item| item.insert == "/idle-compact compact"),
            "/idle-compact should expose a one-command compaction action"
        );
        let permissions_args = slash_arg_suggestions("/permissions", "sandbox read");
        assert!(
            permissions_args
                .iter()
                .any(|item| item.insert == "/permissions sandbox read add "),
            "/permissions should expose sandbox readable-root append"
        );
        assert!(
            permissions_args
                .iter()
                .any(|item| item.insert == "/permissions sandbox read remove "),
            "/permissions should expose sandbox readable-root removal"
        );

        let aliases = [
            ("/effort", "/effort"),
            ("/exit", "/exit"),
            ("/bug-report", "/bug-report"),
            ("/feature-request", "/feature-request"),
            ("/background", "/background"),
            ("/agents", "/agents"),
            ("/independent", "/independent"),
            ("/init", "/init"),
            ("/settings:oppi", "/settings"),
            ("/oppi-settings", "/settings"),
            ("/memory", "/memory"),
            ("/memory-maintenance", "/memory"),
            ("/idle-compact", "/memory"),
            ("/meridian", "/meridian"),
            ("/permissions", "/permissions"),
            ("/prompt-variant", "/prompt-variant"),
            ("/prompts", "/prompts"),
            ("/prompt-templates", "/prompts"),
            ("/runtime-loop", "/runtime-loop"),
            ("/review", "/review"),
            ("/suggest-next", "/suggest-next"),
            ("/oppi-terminal-setup", "/keys"),
            ("/todos", "/todos"),
            ("/usage", "/usage"),
            ("/stats", "/usage"),
            ("/theme", "/theme"),
            ("/themes", "/theme"),
            ("/clear", "/new"),
            ("/reset", "/new"),
            ("/btw", "/btw"),
            ("/mcp", "/mcp"),
        ];
        for (alias, canonical) in aliases {
            let spec = find_slash_command_spec(alias).expect(alias);
            assert_eq!(
                spec.command, canonical,
                "{alias} should route to {canonical}"
            );
        }

        assert_eq!(
            find_slash_command_spec("/meridian")
                .expect("meridian command")
                .command,
            "/meridian"
        );
        assert_eq!(
            find_slash_command_spec("/runtime-loop")
                .expect("runtime-loop command")
                .command,
            "/runtime-loop"
        );

        let all = slash_palette_for_buffer("/", 0).expect("slash palette should open");
        assert!(all.items.iter().any(|item| item.insert == "/meridian"));
        assert!(all.items.iter().any(|item| item.insert == "/runtime-loop"));

        let help = slash_command_help_text();
        assert!(help.contains("/meridian"));
        assert!(help.contains("/runtime-loop"));
        assert!(help.contains("/settings:oppi"));
        assert!(help.contains("/oppi-settings"));
        assert!(help.contains("/memory-maintenance"));
        assert!(help.contains("/idle-compact"));
        assert!(help.contains("/oppi-terminal-setup"));
        assert!(help.contains("/btw <question>"));
        assert!(help.contains("/prompts"));
        assert!(help.contains("/mcp [list|connect|test|reload|auth|trust-plugin|disable-plugin]"));
    }

    #[test]
    fn mcp_status_formatter_shows_state_without_secret_values() {
        let rendered = format_mcp_status_list(&json!({
            "items": [{
                "id": "fs",
                "source": "project",
                "status": "enabled",
                "transport": {
                    "kind": "stdio",
                    "command": "node",
                    "args": ["server.js"],
                    "env": { "API_TOKEN": "should-not-render" }
                },
                "trusted": true,
                "tools": ["read_file", "write_file", "search", "stat", "list", "extra"],
                "resourceSubscriptions": [
                    {
                        "uri": "file:///repo/README.md",
                        "updateCount": 2,
                        "lastUpdatedAt": "2026-05-31T12:00:00Z"
                    }
                ],
                "diagnostics": [{
                    "code": "mcp_connected",
                    "message": "MCP server initialized."
                }]
            }]
        }));

        assert!(rendered.contains("fs [project stdio enabled] tools=6"));
        assert!(rendered.contains("read_file, write_file, search, stat, list (+1 more)"));
        assert!(rendered.contains("subscriptions=1"));
        assert!(rendered.contains("file:///repo/README.md (updates=2, last=2026-05-31T12:00:00Z)"));
        assert!(rendered.contains("mcp_connected: MCP server initialized."));
        assert!(!rendered.contains("should-not-render"));
        assert!(!rendered.contains("API_TOKEN"));
    }

    #[test]
    fn mcp_resources_formatter_shows_resource_metadata_without_secret_values() {
        let rendered = format_mcp_resources_list(&json!({
            "serverId": "docs",
            "status": "ok",
            "resources": [{
                "uri": "file:///repo/README.md",
                "name": "readme",
                "title": "Project README",
                "description": "Human-facing project overview with token should-not-render",
                "mimeType": "text/markdown",
                "size": 2048,
                "annotations": {
                    "secret": "should-not-render"
                }
            }],
            "nextCursor": "page-2"
        }));

        assert!(rendered.contains("mcp resources docs: 1 resource"));
        assert!(rendered.contains("Project README <file:///repo/README.md>"));
        assert!(rendered.contains("text/markdown, 2048 bytes"));
        assert!(rendered.contains("next cursor: page-2"));
        assert!(!rendered.contains("secret"));
        assert!(!rendered.contains("should-not-render"));
    }

    #[test]
    fn mcp_status_formatter_shows_server_capabilities() {
        let rendered = format_mcp_status_list(&json!({
            "items": [{
                "id": "docs",
                "source": "project",
                "status": "enabled",
                "transport": { "kind": "stdio" },
                "tools": ["read"],
                "capabilities": {
                    "tools": { "listChanged": true },
                    "resources": { "subscribe": true, "listChanged": true },
                    "prompts": { "listChanged": false },
                    "sampling": {},
                    "elicitation": {},
                    "tasks": {}
                }
            }]
        }));

        assert!(rendered.contains("capabilities=tools.listChanged, resources.subscribe, resources.listChanged, prompts.listChanged=false, sampling(deferred), elicitation(deferred), tasks(deferred)"));
    }

    #[test]
    fn mcp_status_formatter_shows_list_refresh_hints() {
        let rendered = format_mcp_status_list(&json!({
            "items": [{
                "id": "docs",
                "source": "project",
                "status": "enabled",
                "transport": { "kind": "stdio" },
                "tools": ["read"],
                "listRefreshHints": {
                    "resources": { "refreshCount": 1 },
                    "prompts": { "refreshCount": 2 }
                }
            }]
        }));

        assert!(rendered.contains("refresh=resources:1, prompts:2"));
    }

    #[test]
    fn mcp_action_formatter_shows_auth_required_without_ok_headline() {
        let rendered = format_mcp_action_result(
            "auth",
            "remote",
            &json!({
                "events": [
                    {
                        "kind": {
                            "type": "mcpServerRegistered",
                            "server": {
                                "id": "remote",
                                "status": "authRequired",
                                "tools": []
                            }
                        }
                    },
                    {
                        "kind": {
                            "type": "diagnostic",
                            "diagnostic": {
                                "message": "MCP auth required for remote: MCP OAuth/browser auth is deferred in Rust-native OPPi; configure an env-backed authRef such as authRef: \"env:MCP_TOKEN\" in the MCP server config, then run /mcp connect."
                            }
                        }
                    }
                ]
            }),
        );

        let first_line = rendered.lines().next().unwrap_or_default();
        assert_eq!(first_line, "mcp auth remote: authRequired");
        assert!(rendered.contains("OAuth/browser auth is deferred"));
        assert!(rendered.contains("authRef"));
    }

    #[test]
    fn mcp_resource_read_formatter_shows_text_and_blob_summaries() {
        let rendered = format_mcp_resource_read_result(&json!({
            "serverId": "docs",
            "uri": "file:///repo/README.md",
            "status": "ok",
            "contents": [
                {
                    "uri": "file:///repo/README.md",
                    "mimeType": "text/markdown",
                    "text": "# README\nNative MCP resources can be read."
                },
                {
                    "uri": "file:///repo/logo.png",
                    "mimeType": "image/png",
                    "blob": "aW1hZ2U="
                }
            ]
        }));

        assert!(rendered.contains("mcp read docs: file:///repo/README.md"));
        assert!(rendered.contains("text/markdown text"));
        assert!(rendered.contains("# README"));
        assert!(rendered.contains("image/png blob, 8 base64 chars"));
        assert!(!rendered.contains("aW1hZ2U="));
    }

    #[test]
    fn mcp_resource_subscription_formatter_shows_compact_state() {
        let subscribed = format_mcp_resource_subscription_result(
            "subscribe",
            &json!({
                "serverId": "docs",
                "uri": "file:///repo/README.md",
                "status": "ok",
                "subscribed": true
            }),
        );
        assert!(subscribed.contains("mcp subscribe docs: file:///repo/README.md"));
        assert!(subscribed.contains("subscribed"));

        let denied = format_mcp_resource_subscription_result(
            "subscribe",
            &json!({
                "serverId": "docs",
                "uri": "file:///repo/secret.md",
                "status": "denied",
                "subscribed": false,
                "error": "MCP server does not advertise resources.subscribe"
            }),
        );
        assert!(denied.contains("mcp subscribe docs: denied"));
        assert!(denied.contains("resources.subscribe"));
        assert!(!denied.contains("secret.md"));
    }

    #[test]
    fn mcp_resource_templates_formatter_shows_template_metadata_without_secret_values() {
        let rendered = format_mcp_resource_templates_list(&json!({
            "serverId": "docs",
            "status": "ok",
            "resourceTemplates": [{
                "uriTemplate": "file:///repo/{path}",
                "name": "repo-file",
                "title": "Repository file",
                "description": "Read a repository file by path with token should-not-render",
                "mimeType": "text/plain",
                "annotations": {
                    "secret": "should-not-render"
                }
            }],
            "nextCursor": "template-page-2"
        }));

        assert!(rendered.contains("mcp templates docs: 1 template"));
        assert!(rendered.contains("Repository file <file:///repo/{path}>"));
        assert!(rendered.contains("text/plain"));
        assert!(rendered.contains("next cursor: template-page-2"));
        assert!(!rendered.contains("secret"));
        assert!(!rendered.contains("should-not-render"));
    }

    #[test]
    fn mcp_prompts_formatters_show_prompt_metadata_and_bounded_messages() {
        let listed = format_mcp_prompts_list(&json!({
            "serverId": "docs",
            "status": "ok",
            "prompts": [{
                "name": "review-change",
                "title": "Review change",
                "description": "Review a diff with token should-not-render",
                "arguments": [{
                    "name": "focus",
                    "title": "Focus",
                    "description": "Do not print this secret should-not-render",
                    "required": true
                }]
            }],
            "nextCursor": "page-2"
        }));

        assert!(listed.contains("mcp prompts docs: 1 prompt"));
        assert!(listed.contains("Review change <review-change>"));
        assert!(listed.contains("args: focus required"));
        assert!(listed.contains("next cursor: page-2"));
        assert!(!listed.contains("should-not-render"));

        let fetched = format_mcp_prompt_get_result(&json!({
            "serverId": "docs",
            "name": "review-change",
            "status": "ok",
            "description": "Review prompt",
            "messages": [{
                "role": "user",
                "content": {
                    "type": "text",
                    "text": "Review this change for security.\nPrefer concrete findings."
                }
            }, {
                "role": "assistant",
                "content": {
                    "type": "resource",
                    "resource": {
                        "uri": "file:///repo/README.md",
                        "mimeType": "text/markdown",
                        "text": "resource body should-not-render"
                    }
                }
            }]
        }));

        assert!(fetched.contains("mcp prompt docs/review-change: ok"));
        assert!(fetched.contains("Review prompt"));
        assert!(fetched.contains("user text"));
        assert!(fetched.contains("Review this change for security."));
        assert!(fetched.contains("assistant resource file:///repo/README.md"));
        assert!(!fetched.contains("resource body should-not-render"));
    }

    #[test]
    fn mcp_list_cursor_args_accept_next_page_tokens() {
        assert_eq!(parse_mcp_list_cursor_arg("resources", &[]).unwrap(), None);
        assert_eq!(
            parse_mcp_list_cursor_arg("resources", &["page-2"])
                .unwrap()
                .as_deref(),
            Some("page-2")
        );
        assert_eq!(
            parse_mcp_list_cursor_arg("templates", &["--cursor", "template-page-2"])
                .unwrap()
                .as_deref(),
            Some("template-page-2")
        );
        assert_eq!(
            parse_mcp_list_cursor_arg("prompts", &["cursor", "prompt-page-2"])
                .unwrap()
                .as_deref(),
            Some("prompt-page-2")
        );
        assert!(
            parse_mcp_list_cursor_arg("resources", &["--cursor"])
                .unwrap_err()
                .contains("usage: /mcp resources <id> [cursor]")
        );
        assert!(
            parse_mcp_list_cursor_arg("prompts", &["page-2", "extra"])
                .unwrap_err()
                .contains("usage: /mcp prompts <id> [cursor]")
        );
    }

    #[test]
    fn mcp_list_request_params_include_cursor_only_when_present() {
        let first_page = mcp_list_request_params_json("thread-1", "docs", None);
        assert_eq!(first_page["threadId"], "thread-1");
        assert_eq!(first_page["serverId"], "docs");
        assert!(first_page.get("cursor").is_none());

        let next_page = mcp_list_request_params_json("thread-1", "docs", Some("page-2"));
        assert_eq!(next_page["threadId"], "thread-1");
        assert_eq!(next_page["serverId"], "docs");
        assert_eq!(next_page["cursor"], "page-2");
    }

    #[test]
    fn mcp_help_includes_plugin_trust_commands() {
        let usage = mcp_command_usage();
        assert!(usage.contains("trust-plugin"));
        assert!(usage.contains("disable-plugin"));
        assert!(usage.contains("resources <id> [cursor]"));
        assert!(usage.contains("templates <id> [cursor]"));
        assert!(usage.contains("read <id> <uri>"));
        assert!(usage.contains("prompts <id> [cursor]"));
        assert!(usage.contains("prompt <id> <name>"));

        let args = slash_palette_for_buffer("/mcp ", 0).expect("mcp args");
        assert!(args.items.iter().any(|item| item.label == "trust-plugin"));
        assert!(args.items.iter().any(|item| item.label == "disable-plugin"));
        assert!(args.items.iter().any(|item| item.label == "resources"));
        assert!(args.items.iter().any(|item| item.label == "templates"));
        assert!(args.items.iter().any(|item| item.label == "read"));
        assert!(args.items.iter().any(|item| item.label == "prompts"));
        assert!(args.items.iter().any(|item| item.label == "prompt"));
    }

    #[test]
    fn slash_palette_includes_goal_command_and_arguments() {
        let all = slash_palette_for_buffer("/", 0).expect("slash palette");
        assert!(all.items.iter().any(|item| item.label.starts_with("/goal")));

        let goal_args = slash_palette_for_buffer("/goal ", 0).expect("goal args");
        assert!(
            goal_args
                .items
                .iter()
                .any(|item| item.insert == "/goal pause")
        );
        assert!(
            goal_args
                .items
                .iter()
                .any(|item| item.insert == "/goal resume")
        );
        assert!(
            goal_args
                .items
                .iter()
                .any(|item| item.insert == "/goal clear")
        );
    }

    #[test]
    fn goal_command_route_maps_actions_to_rpc_intent() {
        assert!(matches!(
            goal_command_route(" ").unwrap(),
            GoalCommandRoute::Get
        ));
        assert!(matches!(
            goal_command_route("clear").unwrap(),
            GoalCommandRoute::Clear
        ));
        assert_eq!(
            goal_command_route("pause").unwrap(),
            GoalCommandRoute::Set {
                objective: None,
                status: Some(ThreadGoalStatus::Paused),
                token_budget: GoalBudgetRoute::Unchanged
            }
        );
        assert_eq!(
            goal_command_route("budget 12500").unwrap(),
            GoalCommandRoute::Set {
                objective: None,
                status: None,
                token_budget: GoalBudgetRoute::Set(12_500)
            }
        );
        assert_eq!(
            goal_command_route("budget clear").unwrap(),
            GoalCommandRoute::Set {
                objective: None,
                status: None,
                token_budget: GoalBudgetRoute::Clear
            }
        );
        assert_eq!(
            goal_command_route("replace Ship native goal mode").unwrap(),
            GoalCommandRoute::Set {
                objective: Some("Ship native goal mode".to_string()),
                status: Some(ThreadGoalStatus::Active),
                token_budget: GoalBudgetRoute::Unchanged
            }
        );
        assert_eq!(
            goal_command_route("Ship native goal mode").unwrap(),
            GoalCommandRoute::CreateObjective("Ship native goal mode".to_string())
        );
    }

    #[test]
    fn rejects_protocol_mismatch_response() {
        let error = validate_initialize_response(&json!({
            "protocolVersion": "99.0.0",
            "minProtocolVersion": "99.0.0",
            "protocolCompatible": false
        }))
        .expect_err("mismatched protocol should fail");
        assert!(error.contains("incompatible"));
    }

    #[test]
    fn missing_server_reports_spawn_error() {
        let error = match RpcClient::spawn(PathBuf::from("definitely-missing-oppi-server-for-test"))
        {
            Ok(_) => panic!("missing server should fail"),
            Err(error) => error,
        };
        assert!(error.contains("spawn"));
    }

    #[test]
    fn permission_modes_map_to_turn_sandbox_policy() {
        let read_only = sandbox_policy_json(PermissionMode::ReadOnly, "/repo");
        assert_eq!(read_only["permissionProfile"]["mode"], json!("read-only"));
        assert_eq!(read_only["permissionProfile"]["writableRoots"], json!([]));
        assert_eq!(read_only["filesystem"], json!("readOnly"));
        assert_eq!(read_only["network"], json!("disabled"));

        let full = sandbox_policy_json(PermissionMode::FullAccess, "/repo");
        assert_eq!(full["permissionProfile"]["mode"], json!("full-access"));
        assert_eq!(full["filesystem"], json!("unrestricted"));
        assert_eq!(full["network"], json!("enabled"));
    }

    #[test]
    fn permission_sandbox_protected_patterns_flow_into_turn_policy() {
        let policy = sandbox_policy_json_with_protected_patterns(
            PermissionMode::AutoReview,
            "/repo",
            &["secrets/*.json".to_string(), ".env.local".to_string()],
        );

        assert_eq!(
            policy["permissionProfile"]["protectedPatterns"],
            json!([
                ".env*",
                ".ssh/",
                "*.pem",
                "*.key",
                ".git/config",
                ".git/hooks/",
                ".npmrc",
                ".pypirc",
                ".mcp.json",
                ".claude.json",
                "secrets/*.json",
                ".env.local"
            ])
        );
    }

    #[test]
    fn permission_sandbox_network_flags_flow_into_turn_policy() {
        let default_settings = PermissionSandboxSettings::default();
        let default_policy = sandbox_policy_json_with_settings(
            PermissionMode::AutoReview,
            "/repo",
            &default_settings,
        );
        assert_eq!(default_policy["network"], json!("disabled"));

        let internet_policy = sandbox_policy_json_with_settings(
            PermissionMode::AutoReview,
            "/repo",
            &PermissionSandboxSettings {
                allow_internet: true,
                ..PermissionSandboxSettings::default()
            },
        );
        assert_eq!(internet_policy["network"], json!("enabled"));

        let ssh_policy = sandbox_policy_json_with_settings(
            PermissionMode::ReadOnly,
            "/repo",
            &PermissionSandboxSettings {
                allow_ssh: true,
                ..PermissionSandboxSettings::default()
            },
        );
        assert_eq!(ssh_policy["filesystem"], json!("readOnly"));
        assert_eq!(ssh_policy["network"], json!("enabled"));
    }

    #[test]
    fn permission_sandbox_root_lists_flow_into_turn_policy() {
        let default_policy = sandbox_policy_json_with_settings(
            PermissionMode::AutoReview,
            "/repo",
            &PermissionSandboxSettings::default(),
        );
        assert_eq!(
            default_policy["permissionProfile"]["readableRoots"],
            json!(["/repo"])
        );
        assert_eq!(
            default_policy["permissionProfile"]["writableRoots"],
            json!(["/repo"])
        );

        let project_policy = sandbox_policy_json_with_settings(
            PermissionMode::Default,
            "/repo",
            &PermissionSandboxSettings {
                project_roots: vec!["/workspace".to_string()],
                ..PermissionSandboxSettings::default()
            },
        );
        assert_eq!(
            project_policy["permissionProfile"]["readableRoots"],
            json!(["/workspace"])
        );
        assert_eq!(
            project_policy["permissionProfile"]["writableRoots"],
            json!(["/workspace"])
        );

        let explicit_policy = sandbox_policy_json_with_settings(
            PermissionMode::AutoReview,
            "/repo",
            &PermissionSandboxSettings {
                project_roots: vec!["/workspace".to_string()],
                readable_roots: vec!["/workspace".to_string(), "/shared-read".to_string()],
                writable_roots: vec!["/workspace/out".to_string()],
                ..PermissionSandboxSettings::default()
            },
        );
        assert_eq!(
            explicit_policy["permissionProfile"]["readableRoots"],
            json!(["/workspace", "/shared-read"])
        );
        assert_eq!(
            explicit_policy["permissionProfile"]["writableRoots"],
            json!(["/workspace/out"])
        );

        let read_only_policy = sandbox_policy_json_with_settings(
            PermissionMode::ReadOnly,
            "/repo",
            &PermissionSandboxSettings {
                writable_roots: vec!["/workspace/out".to_string()],
                ..PermissionSandboxSettings::default()
            },
        );
        assert_eq!(
            read_only_policy["permissionProfile"]["writableRoots"],
            json!([])
        );
    }

    #[test]
    fn permissions_command_parses_history_timeout_reviewer_and_clear_session() {
        assert_eq!(
            permissions_command_route("").unwrap(),
            PermissionCommandRoute::Status
        );
        assert_eq!(
            permissions_command_route("status").unwrap(),
            PermissionCommandRoute::Status
        );
        assert_eq!(
            permissions_command_route("history").unwrap(),
            PermissionCommandRoute::History
        );
        assert_eq!(
            permissions_command_route("context").unwrap(),
            PermissionCommandRoute::ReviewContext
        );
        assert_eq!(
            permissions_command_route("review-context").unwrap(),
            PermissionCommandRoute::ReviewContext
        );
        assert_eq!(
            permissions_command_route("timeout 30").unwrap(),
            PermissionCommandRoute::SetReviewTimeoutMs(30_000)
        );
        assert_eq!(
            permissions_command_route("reviewer-model openai-codex/gpt-5.5").unwrap(),
            PermissionCommandRoute::SetReviewerModel(Some("openai-codex/gpt-5.5".to_string()))
        );
        assert_eq!(
            permissions_command_route("reviewer-model auto").unwrap(),
            PermissionCommandRoute::SetReviewerModel(None)
        );
        assert_eq!(
            permissions_command_route("clear-session").unwrap(),
            PermissionCommandRoute::ClearSession
        );
        assert_eq!(
            permissions_command_route("auto-review").unwrap(),
            PermissionCommandRoute::SetMode(PermissionMode::AutoReview)
        );
        assert_eq!(
            permissions_command_route("sandbox protected secrets/*.json, .env.local").unwrap(),
            PermissionCommandRoute::SetProtectedPatterns(vec![
                "secrets/*.json".to_string(),
                ".env.local".to_string()
            ])
        );
        assert_eq!(
            permissions_command_route("sandbox protected default").unwrap(),
            PermissionCommandRoute::SetProtectedPatterns(Vec::new())
        );
        assert_eq!(
            permissions_command_route("sandbox internet on").unwrap(),
            PermissionCommandRoute::SetSandboxInternet(true)
        );
        assert_eq!(
            permissions_command_route("sandbox internet blocked").unwrap(),
            PermissionCommandRoute::SetSandboxInternet(false)
        );
        assert_eq!(
            permissions_command_route("sandbox ssh allow").unwrap(),
            PermissionCommandRoute::SetSandboxSsh(true)
        );
        assert_eq!(
            permissions_command_route("sandbox ssh off").unwrap(),
            PermissionCommandRoute::SetSandboxSsh(false)
        );
        assert_eq!(
            permissions_command_route("sandbox project E:/repo, E:/shared").unwrap(),
            PermissionCommandRoute::SetSandboxProjectRoots(vec![
                "E:/repo".to_string(),
                "E:/shared".to_string()
            ])
        );
        assert_eq!(
            permissions_command_route("sandbox read E:/repo, E:/readonly").unwrap(),
            PermissionCommandRoute::SetSandboxReadableRoots(vec![
                "E:/repo".to_string(),
                "E:/readonly".to_string()
            ])
        );
        assert_eq!(
            permissions_command_route("sandbox read add E:/shared E:/repo").unwrap(),
            PermissionCommandRoute::EditSandboxRoots {
                kind: PermissionSandboxRootListKind::Readable,
                edit: PermissionSandboxRootListEdit::Add,
                roots: vec!["E:/shared".to_string(), "E:/repo".to_string()]
            }
        );
        assert_eq!(
            permissions_command_route("sandbox write remove E:/repo/out").unwrap(),
            PermissionCommandRoute::EditSandboxRoots {
                kind: PermissionSandboxRootListKind::Writable,
                edit: PermissionSandboxRootListEdit::Remove,
                roots: vec!["E:/repo/out".to_string()]
            }
        );
        assert_eq!(
            permissions_command_route("sandbox write default").unwrap(),
            PermissionCommandRoute::SetSandboxWritableRoots(Vec::new())
        );
        assert_eq!(
            format!(
                "{:?}",
                permissions_command_route("sandbox validate").unwrap()
            ),
            "ValidateSandbox"
        );
        assert!(
            permissions_command_route("wat")
                .unwrap_err()
                .contains("/permissions")
        );
    }

    #[test]
    fn permission_sandbox_root_presets_apply_workspace_or_default_roots() {
        let mut settings = PermissionSandboxSettings {
            project_roots: vec!["E:/old-project".to_string()],
            readable_roots: vec!["E:/old-read".to_string()],
            writable_roots: vec!["E:/old-write".to_string()],
            ..PermissionSandboxSettings::default()
        };

        apply_permission_sandbox_root_preset(
            &mut settings,
            PermissionSandboxRootPreset::Here,
            "E:/repo/oppi",
        );
        assert_eq!(settings.project_roots, vec!["E:/repo/oppi"]);
        assert_eq!(settings.readable_roots, vec!["E:/repo/oppi"]);
        assert_eq!(settings.writable_roots, vec!["E:/repo/oppi"]);

        apply_permission_sandbox_root_preset(
            &mut settings,
            PermissionSandboxRootPreset::Default,
            "E:/repo/oppi",
        );
        assert!(settings.project_roots.is_empty());
        assert!(settings.readable_roots.is_empty());
        assert!(settings.writable_roots.is_empty());
    }

    #[test]
    fn permission_sandbox_root_list_edits_append_dedupe_and_remove_paths() {
        let mut roots = vec![
            "E:/repo".to_string(),
            "E:/repo".to_string(),
            " ".to_string(),
        ];

        let changed = apply_permission_sandbox_root_list_edit(
            &mut roots,
            PermissionSandboxRootListEdit::Add,
            &["E:/repo".to_string(), "E:/shared".to_string()],
        );
        assert!(changed);
        assert_eq!(roots, vec!["E:/repo".to_string(), "E:/shared".to_string()]);

        let changed = apply_permission_sandbox_root_list_edit(
            &mut roots,
            PermissionSandboxRootListEdit::Remove,
            &["E:/missing".to_string()],
        );
        assert!(!changed);
        assert_eq!(roots, vec!["E:/repo".to_string(), "E:/shared".to_string()]);

        let changed = apply_permission_sandbox_root_list_edit(
            &mut roots,
            PermissionSandboxRootListEdit::Remove,
            &["E:/repo".to_string()],
        );
        assert!(changed);
        assert_eq!(roots, vec!["E:/shared".to_string()]);
    }

    #[test]
    fn permission_sandbox_validation_reports_effective_roots_and_path_issues() {
        let root = env::temp_dir().join(format!(
            "oppi-permission-sandbox-validation-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        let project = root.join("project");
        let missing_read = root.join("missing-read");
        let file_write = root.join("file-write.txt");
        fs::create_dir_all(&project).unwrap();
        fs::write(&file_write, "not a directory").unwrap();

        let project = project.to_string_lossy().to_string();
        let missing_read = missing_read.to_string_lossy().to_string();
        let file_write = file_write.to_string_lossy().to_string();
        let validation = permission_sandbox_validation_value(
            PermissionMode::AutoReview,
            &project,
            &PermissionSandboxSettings {
                readable_roots: vec![project.clone(), missing_read.clone()],
                writable_roots: vec![file_write.clone()],
                protected_patterns: vec!["secrets/*.json".to_string()],
                ..PermissionSandboxSettings::default()
            },
        );

        assert_eq!(validation["ok"], json!(false));
        assert_eq!(validation["issueCount"], json!(2));
        assert_eq!(
            validation["roots"]["project"][0],
            json!({
                "path": project,
                "status": "ok",
                "source": "default"
            })
        );
        assert_eq!(
            validation["roots"]["readable"][1],
            json!({
                "path": missing_read,
                "status": "missing",
                "source": "configured"
            })
        );
        assert_eq!(
            validation["roots"]["writable"][0],
            json!({
                "path": file_write,
                "status": "not-directory",
                "source": "configured"
            })
        );
        assert_eq!(
            validation["protectedPatterns"],
            json!([
                ".env*",
                ".ssh/",
                "*.pem",
                "*.key",
                ".git/config",
                ".git/hooks/",
                ".npmrc",
                ".pypirc",
                ".mcp.json",
                ".claude.json",
                "secrets/*.json"
            ])
        );

        let rendered = format_permission_sandbox_validation(&validation);
        assert!(rendered.contains("sandbox validation: 2 issues"));
        assert!(rendered.contains("effective policy: filesystem=workspaceWrite, network=disabled"));
        assert!(rendered.contains("project roots:"));
        assert!(rendered.contains("readable roots:"));
        assert!(rendered.contains("writable roots:"));
        assert!(rendered.contains("missing"));
        assert!(rendered.contains("not-directory"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn permissions_settings_persist_reviewer_timeout_without_losing_permission_mode() {
        let root = env::temp_dir().join(format!(
            "oppi-permission-review-settings-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let path = root.join("settings.json");
        fs::write(
            &path,
            r#"{"oppi":{"permissionMode":"auto-review"},"kept":true}"#,
        )
        .unwrap();

        let mut settings = load_permission_review_settings(&path);
        settings.timeout_ms = 30_000;
        settings.reviewer_model = Some("openai-codex/gpt-5.5".to_string());
        save_permission_review_settings(&path, &settings).unwrap();

        let raw: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(raw["oppi"]["permissionMode"], json!("auto-review"));
        assert_eq!(raw["oppi"]["permissions"]["reviewTimeoutMs"], json!(30_000));
        assert_eq!(
            raw["oppi"]["permissions"]["reviewerModel"],
            json!("openai-codex/gpt-5.5")
        );
        assert_eq!(raw["kept"], json!(true));

        let loaded = load_permission_review_settings(&path);
        assert_eq!(loaded.timeout_ms, 30_000);
        assert_eq!(
            loaded.reviewer_model.as_deref(),
            Some("openai-codex/gpt-5.5")
        );

        save_permission_review_settings(
            &path,
            &oppi_protocol::GuardianReviewSettings {
                timeout_ms: 45_000,
                reviewer_model: None,
            },
        )
        .unwrap();
        let raw: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(raw["oppi"]["permissions"]["reviewerModel"], json!("auto"));
        assert_eq!(raw["oppi"]["permissionMode"], json!("auto-review"));
        assert_eq!(raw["kept"], json!(true));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn footer_command_parses_visibility_and_usage_settings() {
        assert_eq!(
            footer_command_route("").unwrap(),
            FooterCommandRoute::Status
        );
        assert_eq!(
            footer_command_route("help off").unwrap(),
            FooterCommandRoute::SetBool(FooterSettingBool::ShowHelpBar, false)
        );
        assert_eq!(
            footer_command_route("path on").unwrap(),
            FooterCommandRoute::SetBool(FooterSettingBool::ShowPath, true)
        );
        assert_eq!(
            footer_command_route("model false").unwrap(),
            FooterCommandRoute::SetBool(FooterSettingBool::ShowModel, false)
        );
        assert_eq!(
            footer_command_route("permission enabled").unwrap(),
            FooterCommandRoute::SetBool(FooterSettingBool::ShowPermission, true)
        );
        assert_eq!(
            footer_command_route("memory blocked").unwrap(),
            FooterCommandRoute::SetBool(FooterSettingBool::ShowMemory, false)
        );
        assert_eq!(
            footer_command_route("context allow").unwrap(),
            FooterCommandRoute::SetBool(FooterSettingBool::ShowContext, true)
        );
        assert_eq!(
            footer_command_route("usage weekly").unwrap(),
            FooterCommandRoute::SetUsageDisplay(FooterUsageDisplay::Weekly)
        );
        assert!(
            footer_command_route("usage never")
                .unwrap_err()
                .contains("session-weekly")
        );
    }

    #[test]
    fn compaction_command_parses_pi_compatible_settings() {
        assert_eq!(
            compaction_command_route("").unwrap(),
            CompactionCommandRoute::Status
        );
        assert_eq!(
            compaction_command_route("status").unwrap(),
            CompactionCommandRoute::Status
        );
        assert_eq!(
            compaction_command_route("off").unwrap(),
            CompactionCommandRoute::SetIdleEnabled(false)
        );
        assert_eq!(
            compaction_command_route("on").unwrap(),
            CompactionCommandRoute::SetIdleEnabled(true)
        );
        assert_eq!(
            compaction_command_route("idle-time 10").unwrap(),
            CompactionCommandRoute::SetIdleMinutes(10)
        );
        assert_eq!(
            compaction_command_route("idle-threshold 80").unwrap(),
            CompactionCommandRoute::SetIdleThresholdPercent(80)
        );
        assert_eq!(
            compaction_command_route("smart-threshold 55").unwrap(),
            CompactionCommandRoute::SetSmartThresholdPercent(55)
        );
        assert!(compaction_command_route("compact").is_ok());
        assert!(compaction_command_route("compact-now").is_ok());
        assert!(
            compaction_command_route("idle-time 3")
                .unwrap_err()
                .contains("2|5|10")
        );
        assert!(
            compaction_command_route("smart-threshold 80")
                .unwrap_err()
                .contains("50|55|60|65|70|75")
        );
    }

    #[test]
    fn memory_compact_summary_allows_empty_runtime_fallback() {
        assert_eq!(memory_compact_summary_arg(&[]), "");
        assert_eq!(
            memory_compact_summary_arg(&["manual", "handoff"]),
            "manual handoff"
        );
    }

    #[test]
    fn memory_settings_command_route_parses_pi_memory_controls() {
        assert_eq!(
            memory_settings_command_route("").unwrap(),
            MemorySettingsCommandRoute::Status
        );
        assert_eq!(
            memory_settings_command_route("status").unwrap(),
            MemorySettingsCommandRoute::Status
        );
        assert_eq!(
            memory_settings_command_route("on").unwrap(),
            MemorySettingsCommandRoute::SetEnabled(true)
        );
        assert_eq!(
            memory_settings_command_route("off").unwrap(),
            MemorySettingsCommandRoute::SetEnabled(false)
        );
        assert_eq!(
            memory_settings_command_route("startup-recall off").unwrap(),
            MemorySettingsCommandRoute::SetStartupRecall(false)
        );
        assert_eq!(
            memory_settings_command_route("task-start-recall on").unwrap(),
            MemorySettingsCommandRoute::SetTaskStartRecall(true)
        );
        assert_eq!(
            memory_settings_command_route("turn-summaries off").unwrap(),
            MemorySettingsCommandRoute::SetTurnSummaries(false)
        );
        assert_eq!(
            memory_settings_command_route("agent-model gpt").unwrap(),
            MemorySettingsCommandRoute::SetAgentModel("gpt")
        );
        assert_eq!(
            memory_settings_command_route("deep-model sonnet").unwrap(),
            MemorySettingsCommandRoute::SetDeepModel("sonnet")
        );
        assert_eq!(
            memory_settings_command_route("idle-dream off").unwrap(),
            MemorySettingsCommandRoute::SetIdleConsolidation(false)
        );
        assert_eq!(
            memory_settings_command_route("sync on").unwrap(),
            MemorySettingsCommandRoute::SetSyncEnabled(true)
        );
        assert_eq!(
            memory_settings_command_route("sync repo ~/hoppi-sync").unwrap(),
            MemorySettingsCommandRoute::SetSyncRepoPath(Some("~/hoppi-sync".to_string()))
        );
        assert_eq!(
            memory_settings_command_route("sync remote none").unwrap(),
            MemorySettingsCommandRoute::SetSyncRepoUrl(None)
        );
        assert_eq!(
            memory_settings_command_route("sync pull-startup off").unwrap(),
            MemorySettingsCommandRoute::SetSyncPullOnStartup(false)
        );
        assert_eq!(
            memory_settings_command_route("sync push-exit on").unwrap(),
            MemorySettingsCommandRoute::SetSyncPushOnExit(true)
        );
        assert_eq!(
            memory_settings_command_route("sync background 30m").unwrap(),
            MemorySettingsCommandRoute::SetSyncBackground("30m")
        );
        assert_eq!(
            memory_settings_command_route("sync encryption file").unwrap(),
            MemorySettingsCommandRoute::SetSyncEncryption {
                encryption: "passphrase",
                passphrase_source: "file"
            }
        );
        assert_eq!(
            memory_settings_command_route("sync passphrase-env OPPI_SECRET").unwrap(),
            MemorySettingsCommandRoute::SetSyncPassphraseEnv("OPPI_SECRET".to_string())
        );
        assert_eq!(
            memory_settings_command_route("sync passphrase-file ~/hoppi/passphrase").unwrap(),
            MemorySettingsCommandRoute::SetSyncPassphraseFile(Some(
                "~/hoppi/passphrase".to_string()
            ))
        );
        assert_eq!(
            memory_settings_command_route("sync conflict keep-local").unwrap(),
            MemorySettingsCommandRoute::SetSyncConflictMode("keep-local")
        );
        assert_eq!(
            memory_settings_command_route("sync now").unwrap(),
            MemorySettingsCommandRoute::RunSync(HoppiSyncDirection::Both)
        );
        assert_eq!(
            memory_settings_command_route("sync pull").unwrap(),
            MemorySettingsCommandRoute::RunSync(HoppiSyncDirection::Pull)
        );
        assert_eq!(
            memory_settings_command_route("sync push").unwrap(),
            MemorySettingsCommandRoute::RunSync(HoppiSyncDirection::Push)
        );
        assert_eq!(
            memory_settings_command_route("install-offer dismissed").unwrap(),
            MemorySettingsCommandRoute::SetHoppiInstallOffer("dismissed")
        );
        assert_eq!(
            memory_settings_command_route("install-hoppi").unwrap(),
            MemorySettingsCommandRoute::InstallHoppi(false)
        );
        assert_eq!(
            memory_settings_command_route("install --yes").unwrap(),
            MemorySettingsCommandRoute::InstallHoppi(true)
        );
        assert!(memory_settings_command_route("startup-recall maybe").is_err());
        assert!(memory_settings_command_route("agent-model llama").is_err());
        assert!(memory_settings_command_route("deep-model opus").is_err());
        assert!(memory_settings_command_route("install-offer never").is_err());
        assert!(memory_settings_command_route("sync background 5m").is_err());
        assert!(memory_settings_command_route("sync conflict merge").is_err());
    }

    #[test]
    fn memory_install_hoppi_approval_panel_is_explicit() {
        let text = hoppi_install_approval_panel();
        assert!(text.contains("Approval required"));
        assert!(text.contains("@oppiai/hoppi-memory"));
        assert!(text.contains("/memory install-hoppi --yes"));
        assert!(text.contains("npm/network"));
        assert!(text.contains("no hidden install"));
    }

    #[test]
    fn memory_backend_native_shell_recall_builds_runtime_backend_request() {
        let params = memory_backend_recall_params_from_settings(
            &MemorySettings {
                enabled: true,
                startup_recall: true,
                task_start_recall: true,
                ..MemorySettings::default()
            },
            "thread-1",
            "continue the plan",
        )
        .unwrap();

        assert_eq!(params["threadId"], "thread-1");
        assert_eq!(params["prompt"], "continue the plan");
        assert_eq!(params["includeStartup"], true);
        assert_eq!(params["includeTask"], true);
    }

    #[test]
    fn memory_backend_native_shell_builds_route_catalog_requests() {
        let settings = MemorySettings {
            enabled: true,
            sync_enabled: true,
            turn_summaries: true,
            idle_consolidation: true,
            agent_model: "gpt".to_string(),
            deep_model: "sonnet".to_string(),
            ..MemorySettings::default()
        };

        let list =
            memory_backend_list_params_from_settings(&settings, "thread-1", true, false, Some(25))
                .unwrap();
        assert_eq!(list["threadId"], "thread-1");
        assert_eq!(list["includeSuperseded"], true);
        assert_eq!(list["pinnedOnly"], false);
        assert_eq!(list["limit"], 25);

        let remember = memory_backend_turn_remember_params_from_settings(
            &settings,
            "Native shell saved a turn summary",
            "thread-1",
            "turn-1",
        )
        .unwrap();
        assert_eq!(remember["threadId"], "thread-1");
        assert_eq!(remember["content"], "Native shell saved a turn summary");
        assert_eq!(remember["source"], "oppi:native-shell");
        assert_eq!(remember["sourceSessionId"], "thread-1:turn-1");
        assert_eq!(remember["layer"], "buffer");

        let recap = memory_backend_session_recap_params_from_settings(
            &settings,
            "Session recap with useful handoff context",
            "thread-1",
            HoppiSessionRecapReason::Clear,
        )
        .unwrap();
        assert_eq!(recap["source"], "oppi:clear");
        assert_eq!(recap["sourceSessionId"], "thread-1");

        let update = memory_backend_update_params_from_settings(
            &settings,
            "thread-1",
            "mem-1",
            json!({ "content": "edited", "tags": ["native"] }),
        )
        .unwrap();
        assert_eq!(update["threadId"], "thread-1");
        assert_eq!(update["id"], "mem-1");
        assert_eq!(update["patch"]["content"], "edited");

        let supersede = memory_backend_supersede_params_from_settings(
            &settings, "thread-1", "mem-old", "mem-new",
        )
        .unwrap();
        assert_eq!(supersede["id"], "mem-old");
        assert_eq!(supersede["supersededBy"], "mem-new");

        let consolidate =
            memory_backend_consolidate_params_from_settings(&settings, "thread-1", false).unwrap();
        assert_eq!(consolidate["stage"], "manual-maintenance");
        assert_eq!(consolidate["dryRun"], false);
        assert_eq!(consolidate["agentModel"], "gpt");
        assert_eq!(consolidate["deepModel"], "sonnet");

        let idle =
            memory_backend_idle_consolidate_params_from_settings(&settings, "thread-1").unwrap();
        assert_eq!(idle["stage"], "idle-dream");
        assert_eq!(idle["budget"], HOPPI_IDLE_CONSOLIDATE_BUDGET);

        let sync = memory_backend_sync_params_from_settings(
            &settings,
            "thread-1",
            HoppiSyncDirection::Push,
            "test",
        )
        .unwrap();
        assert_eq!(sync["threadId"], "thread-1");
        assert_eq!(sync["direction"], "push");
    }

    #[test]
    fn hoppi_sync_invocation_uses_shared_settings_and_passphrase_file() {
        let root = env::temp_dir().join(format!("oppi-hoppi-sync-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let passphrase_file = root.join("passphrase.txt");
        fs::write(&passphrase_file, "secret-passphrase\n").unwrap();

        let settings = MemorySettings {
            enabled: true,
            sync_enabled: true,
            sync_repo_url: Some("git@github.com:user/hoppi-memories.git".to_string()),
            sync_encryption: "passphrase".to_string(),
            sync_passphrase_source: "file".to_string(),
            sync_passphrase_file: Some(passphrase_file.display().to_string()),
            sync_conflict_mode: "keep-local".to_string(),
            ..MemorySettings::default()
        };

        let invocation = hoppi_sync_invocation_from_settings(
            &settings,
            HoppiSyncDirection::Pull,
            "manual",
            PathBuf::from("hoppi.js"),
        )
        .unwrap();

        assert_eq!(invocation.module_path, PathBuf::from("hoppi.js"));
        assert_eq!(invocation.repo_path, default_hoppi_sync_repo_path());
        assert_eq!(invocation.direction, HoppiSyncDirection::Pull);
        assert_eq!(
            invocation.remote.as_deref(),
            Some("git@github.com:user/hoppi-memories.git")
        );
        assert_eq!(invocation.conflict_mode, "keep-local");
        assert_eq!(invocation.passphrase.as_deref(), Some("secret-passphrase"));
        assert_eq!(invocation.message, "Hoppi memory sync (manual)");

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn hoppi_sync_invocation_refuses_disabled_memory_or_sync() {
        let mut settings = MemorySettings {
            enabled: false,
            sync_enabled: true,
            ..MemorySettings::default()
        };
        assert!(
            hoppi_sync_invocation_from_settings(
                &settings,
                HoppiSyncDirection::Both,
                "manual",
                PathBuf::from("hoppi.js"),
            )
            .unwrap_err()
            .contains("memory disabled")
        );

        settings.enabled = true;
        settings.sync_enabled = false;
        assert!(
            hoppi_sync_invocation_from_settings(
                &settings,
                HoppiSyncDirection::Both,
                "manual",
                PathBuf::from("hoppi.js"),
            )
            .unwrap_err()
            .contains("sync disabled")
        );
    }

    #[test]
    fn hoppi_sync_lifecycle_plans_startup_exit_and_background_from_settings() {
        let settings = MemorySettings {
            enabled: true,
            sync_enabled: true,
            sync_background: "30m".to_string(),
            ..MemorySettings::default()
        };

        assert_eq!(
            hoppi_sync_lifecycle_direction(&settings, HoppiSyncLifecycle::Startup),
            Some(HoppiSyncDirection::Pull)
        );
        assert_eq!(
            hoppi_sync_lifecycle_direction(&settings, HoppiSyncLifecycle::Exit),
            Some(HoppiSyncDirection::Push)
        );
        assert_eq!(
            hoppi_sync_lifecycle_direction(&settings, HoppiSyncLifecycle::Clear),
            Some(HoppiSyncDirection::Push)
        );
        assert_eq!(
            hoppi_background_sync_interval(&settings),
            Some(Duration::from_secs(30 * 60))
        );

        let mut disabled = settings.clone();
        disabled.enabled = false;
        assert_eq!(
            hoppi_sync_lifecycle_direction(&disabled, HoppiSyncLifecycle::Startup),
            None
        );
        assert_eq!(
            hoppi_sync_lifecycle_direction(&disabled, HoppiSyncLifecycle::Exit),
            None
        );
        assert_eq!(
            hoppi_sync_lifecycle_direction(&disabled, HoppiSyncLifecycle::Clear),
            None
        );
        assert_eq!(hoppi_background_sync_interval(&disabled), None);

        let mut sync_disabled = settings.clone();
        sync_disabled.sync_enabled = false;
        assert_eq!(
            hoppi_sync_lifecycle_direction(&sync_disabled, HoppiSyncLifecycle::Startup),
            None
        );
        assert_eq!(
            hoppi_sync_lifecycle_direction(&sync_disabled, HoppiSyncLifecycle::Exit),
            None
        );
        assert_eq!(
            hoppi_sync_lifecycle_direction(&sync_disabled, HoppiSyncLifecycle::Clear),
            None
        );
        assert_eq!(hoppi_background_sync_interval(&sync_disabled), None);

        let mut lifecycle_disabled = settings.clone();
        lifecycle_disabled.sync_pull_on_startup = false;
        lifecycle_disabled.sync_push_on_exit = false;
        lifecycle_disabled.sync_background = "off".to_string();
        assert_eq!(
            hoppi_sync_lifecycle_direction(&lifecycle_disabled, HoppiSyncLifecycle::Startup),
            None
        );
        assert_eq!(
            hoppi_sync_lifecycle_direction(&lifecycle_disabled, HoppiSyncLifecycle::Exit),
            None
        );
        assert_eq!(
            hoppi_sync_lifecycle_direction(&lifecycle_disabled, HoppiSyncLifecycle::Clear),
            None
        );
        assert_eq!(hoppi_background_sync_interval(&lifecycle_disabled), None);
    }

    #[test]
    fn hoppi_background_sync_scheduler_waits_for_interval_and_idle() {
        let settings = MemorySettings {
            enabled: true,
            sync_enabled: true,
            sync_background: "15m".to_string(),
            ..MemorySettings::default()
        };
        let now = Instant::now();
        let due_at = hoppi_next_background_sync_at(&settings, now).unwrap();

        assert_eq!(due_at.duration_since(now), Duration::from_secs(15 * 60));
        assert!(!hoppi_background_sync_due(
            &settings,
            Some(due_at),
            true,
            now + Duration::from_secs(14 * 60)
        ));
        assert!(!hoppi_background_sync_due(
            &settings,
            Some(due_at),
            false,
            due_at
        ));
        assert!(hoppi_background_sync_due(
            &settings,
            Some(due_at),
            true,
            due_at
        ));

        let disabled = MemorySettings {
            sync_background: "off".to_string(),
            ..settings
        };
        assert_eq!(hoppi_next_background_sync_at(&disabled, now), None);
        assert!(!hoppi_background_sync_due(
            &disabled,
            Some(due_at),
            true,
            due_at
        ));
    }

    #[test]
    fn hoppi_sync_result_summary_matches_pi_manual_sync_surface() {
        let result = hoppi_sync_result_from_value(&json!({
            "imported": 2,
            "exported": 3,
            "committed": true,
            "pushed": true,
            "conflicts": ["a", "b"],
            "embeddingsRebuildRecommended": true,
            "rebuiltEmbeddings": true,
        }));

        assert_eq!(
            format_hoppi_sync_report(HoppiSyncDirection::Both, &result),
            "Hoppi sync both · pulled 2 · exported 3 · committed · pushed · 2 conflicts · embeddings rebuilt"
        );
    }

    #[test]
    fn hoppi_sync_job_update_params_track_running_and_terminal_states() {
        let invocation = HoppiSyncInvocation {
            module_path: PathBuf::from("hoppi.js"),
            repo_path: PathBuf::from("E:\\RepositoryV2\\oppi\\hoppi-memories"),
            direction: HoppiSyncDirection::Both,
            remote: Some("git@example.com:private/hoppi-memories.git".to_string()),
            conflict_mode: "keep-both".to_string(),
            passphrase: Some("secret-passphrase".to_string()),
            message: "Hoppi memory sync (manual)".to_string(),
        };
        let result = HoppiSyncCommandResult {
            imported: 4,
            exported: 5,
            committed: true,
            pushed: true,
            conflicts: vec!["conflict-a".to_string()],
            embeddings_rebuild_recommended: true,
            rebuilt_embeddings: true,
        };

        let started =
            hoppi_sync_job_started_params("thread-1", "native-sync-job-1", &invocation, "manual");
        assert_eq!(started["threadId"], "thread-1");
        assert_eq!(started["jobId"], "native-sync-job-1");
        assert_eq!(started["kind"], "sync");
        assert_eq!(started["stage"], "sync-both");
        assert_eq!(started["status"], "running");
        assert_eq!(started["source"], "hoppi-sync-manual");
        assert_eq!(started["backendApplied"], false);
        assert!(
            started["message"]
                .as_str()
                .unwrap()
                .contains("Hoppi memory sync (manual)")
        );
        assert!(
            !started["message"]
                .as_str()
                .unwrap()
                .contains("secret-passphrase")
        );

        let finished = hoppi_sync_job_finished_params(
            "thread-1",
            "native-sync-job-1",
            &invocation,
            &result,
            "manual",
        );
        assert_eq!(finished["status"], "succeeded");
        assert_eq!(finished["recordCount"], 9);
        assert_eq!(finished["backendApplied"], true);
        assert!(finished["message"].as_str().unwrap().contains("pulled 4"));
        assert!(finished["message"].as_str().unwrap().contains("exported 5"));
        assert!(
            !finished["message"]
                .as_str()
                .unwrap()
                .contains("git@example.com")
        );

        let failed = hoppi_sync_job_failed_params(
            "thread-1",
            "native-sync-job-1",
            &invocation,
            "sync bridge failed",
            "manual",
        );
        assert_eq!(failed["status"], "failed");
        assert_eq!(failed["backendApplied"], false);
        assert!(
            failed["message"]
                .as_str()
                .unwrap()
                .contains("sync bridge failed")
        );
    }

    #[test]
    fn hoppi_sync_node_bridge_runs_installed_module_contract() {
        let root = env::temp_dir().join(format!("oppi-hoppi-sync-node-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let module_path = root.join("hoppi-sync-fixture.mjs");
        let repo_path = root.join("repo");
        fs::write(
            &module_path,
            r#"
export function getDefaultHoppiRoot() {
  return "fixture-root";
}

export async function syncGitRepository(root, repoPath, options) {
  if (root !== "fixture-root") throw new Error(`bad root ${root}`);
  if (!repoPath.endsWith("repo")) throw new Error(`bad repo ${repoPath}`);
  if (options.direction !== "both") throw new Error(`bad direction ${options.direction}`);
  if (options.remote !== "git@example.com:memories.git") throw new Error("missing remote");
  if (options.conflictMode !== "keep-both") throw new Error("bad conflict mode");
  if (options.passphrase !== "secret") throw new Error("missing passphrase");
  if (options.message !== "Hoppi memory sync (test)") throw new Error("bad message");
  return {
    imported: 4,
    exported: 5,
    committed: true,
    pushed: true,
    conflicts: [],
    embeddingsRebuildRecommended: true
  };
}

export async function rebuildEmbeddings(root) {
  if (root !== "fixture-root") throw new Error(`bad rebuild root ${root}`);
  return { rebuilt: 1 };
}
"#,
        )
        .unwrap();

        let invocation = HoppiSyncInvocation {
            module_path,
            repo_path,
            direction: HoppiSyncDirection::Both,
            remote: Some("git@example.com:memories.git".to_string()),
            conflict_mode: "keep-both".to_string(),
            passphrase: Some("secret".to_string()),
            message: "Hoppi memory sync (test)".to_string(),
        };

        let result = run_hoppi_sync_invocation(&invocation).unwrap();
        assert_eq!(result.imported, 4);
        assert_eq!(result.exported, 5);
        assert!(result.committed);
        assert!(result.pushed);
        assert!(result.rebuilt_embeddings);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn hoppi_consolidate_invocation_uses_explicit_maintenance_settings() {
        let settings = MemorySettings {
            enabled: true,
            agent_model: "gpt".to_string(),
            deep_model: "gpt-5.5".to_string(),
            ..MemorySettings::default()
        };

        let invocation = hoppi_consolidate_invocation_from_settings(
            &settings,
            Path::new("E:\\RepositoryV2\\oppi"),
            PathBuf::from("hoppi.js"),
            true,
        )
        .unwrap();

        assert_eq!(invocation.module_path, PathBuf::from("hoppi.js"));
        assert_eq!(invocation.cwd, PathBuf::from("E:\\RepositoryV2\\oppi"));
        assert!(invocation.dry_run);
        assert_eq!(invocation.stage, "manual-maintenance");
        assert_eq!(invocation.limit, Some(50));
        assert_eq!(invocation.budget, None);
        assert_eq!(invocation.agent_model, "gpt");
        assert_eq!(invocation.deep_model, "gpt-5.5");

        let disabled = MemorySettings {
            enabled: false,
            ..settings
        };
        assert!(
            hoppi_consolidate_invocation_from_settings(
                &disabled,
                Path::new("E:\\RepositoryV2\\oppi"),
                PathBuf::from("hoppi.js"),
                false,
            )
            .unwrap_err()
            .contains("memory disabled")
        );
    }

    #[test]
    fn hoppi_idle_consolidation_scheduler_waits_for_dirty_idle_memory() {
        let settings = MemorySettings {
            enabled: true,
            idle_consolidation: true,
            ..MemorySettings::default()
        };
        let now = Instant::now();
        let dirty_at = now - HOPPI_IDLE_CONSOLIDATE_AFTER;

        assert!(hoppi_idle_consolidation_due(
            &settings,
            2,
            1,
            Some(dirty_at),
            true,
            now
        ));
        assert!(!hoppi_idle_consolidation_due(
            &settings,
            2,
            2,
            Some(dirty_at),
            true,
            now
        ));
        assert!(!hoppi_idle_consolidation_due(
            &settings,
            2,
            1,
            Some(now - Duration::from_secs(30)),
            true,
            now
        ));
        assert!(!hoppi_idle_consolidation_due(
            &settings,
            2,
            1,
            Some(dirty_at),
            false,
            now
        ));

        let disabled = MemorySettings {
            idle_consolidation: false,
            ..settings
        };
        assert!(!hoppi_idle_consolidation_due(
            &disabled,
            2,
            1,
            Some(dirty_at),
            true,
            now
        ));
    }

    #[test]
    fn hoppi_idle_consolidate_invocation_uses_idle_stage_and_budget() {
        let settings = MemorySettings {
            enabled: true,
            idle_consolidation: true,
            agent_model: "claude".to_string(),
            deep_model: "sonnet".to_string(),
            ..MemorySettings::default()
        };

        let invocation = hoppi_idle_consolidate_invocation_from_settings(
            &settings,
            Path::new("E:\\RepositoryV2\\oppi"),
            PathBuf::from("hoppi.js"),
        )
        .unwrap();

        assert_eq!(invocation.module_path, PathBuf::from("hoppi.js"));
        assert_eq!(invocation.cwd, PathBuf::from("E:\\RepositoryV2\\oppi"));
        assert!(!invocation.dry_run);
        assert_eq!(invocation.stage, "idle-dream");
        assert_eq!(invocation.limit, None);
        assert_eq!(invocation.budget, Some(1_500));
        assert_eq!(invocation.agent_model, "claude");
        assert_eq!(invocation.deep_model, "sonnet");

        let disabled = MemorySettings {
            idle_consolidation: false,
            ..settings
        };
        assert!(
            hoppi_idle_consolidate_invocation_from_settings(
                &disabled,
                Path::new("E:\\RepositoryV2\\oppi"),
                PathBuf::from("hoppi.js"),
            )
            .unwrap_err()
            .contains("idle consolidation disabled")
        );
    }

    #[test]
    fn hoppi_consolidate_node_bridge_runs_installed_backend_contract() {
        let root = env::temp_dir().join(format!(
            "oppi-hoppi-consolidate-node-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let module_path = root.join("hoppi-consolidate-fixture.mjs");
        let workspace = root.join("workspace");
        fs::create_dir_all(&workspace).unwrap();
        fs::write(
            &module_path,
            r#"
export function getDefaultHoppiRoot() {
  return "fixture-root";
}

export function createHoppiBackend(options) {
  if (options.root !== "fixture-root") throw new Error(`bad root ${options.root}`);
  return {
    async init() {},
    async status(project) {
      if (!project.cwd.endsWith("workspace")) throw new Error(`bad cwd ${project.cwd}`);
      return {
        memoryCount: 4,
        pinnedCount: 1,
        storePath: "fixture-store",
        lastConsolidatedAt: "2026-05-31T10:00:00Z",
        queueDepth: 0
      };
    },
    async consolidate(input) {
      if (!input.project.cwd.endsWith("workspace")) throw new Error(`bad consolidate cwd ${input.project.cwd}`);
      if (input.dryRun !== false) throw new Error(`bad dryRun ${input.dryRun}`);
      if (input.stage !== "manual-maintenance") throw new Error(`bad stage ${input.stage}`);
      if (input.limit !== 50) throw new Error(`bad limit ${input.limit}`);
      if (!input.modelPreference) throw new Error("missing modelPreference");
      if (input.modelPreference.agentModel !== "gpt") throw new Error(`bad agent model ${input.modelPreference.agentModel}`);
      if (input.modelPreference.deepModel !== "gpt-5.5") throw new Error(`bad deep model ${input.modelPreference.deepModel}`);
      return {
        modelUsed: "gpt-5.4-mini",
        summary: "Merged duplicate implementation memories.",
        records: [
          {
            id: "mem-consolidated",
            content: "Consolidated native maintenance memory",
            tags: ["consolidated", "verified"],
            confidence: "verified",
            source: "hoppi:maintenance",
            parents: ["mem-noise"]
          }
        ],
        deletedIds: ["mem-noise"],
        memoryCount: 4,
        pinnedCount: 1,
        storePath: "fixture-store",
        lastConsolidatedAt: "2026-05-31T10:00:00Z",
        queueDepth: 0
      };
    }
  };
}
"#,
        )
        .unwrap();

        let result = run_hoppi_consolidate_invocation(&HoppiConsolidateInvocation {
            module_path,
            cwd: workspace,
            dry_run: false,
            stage: "manual-maintenance".to_string(),
            limit: Some(50),
            budget: None,
            agent_model: "gpt".to_string(),
            deep_model: "gpt-5.5".to_string(),
        })
        .unwrap();

        assert!(!result.dry_run);
        assert_eq!(
            result.summary.as_deref(),
            Some("Merged duplicate implementation memories.")
        );
        assert_eq!(result.memory_count, 4);
        assert_eq!(result.pinned_count, Some(1));
        assert_eq!(result.model_used.as_deref(), Some("gpt-5.4-mini"));
        assert_eq!(result.store_path.as_deref(), Some("fixture-store"));
        assert_eq!(
            result.last_consolidated_at.as_deref(),
            Some("2026-05-31T10:00:00Z")
        );
        assert_eq!(result.queue_depth, Some(0));
        assert_eq!(result.deleted_ids, vec!["mem-noise"]);
        assert_eq!(result.records[0].id, "mem-consolidated");
        assert_eq!(result.records[0].parents, vec!["mem-noise".to_string()]);

        let rendered = format_memory_consolidate(&json!({
            "available": true,
            "stage": "manual-maintenance",
            "modelUsed": result.model_used,
            "summary": result.summary,
            "dryRun": false,
            "backendApplied": true,
            "cacheUpdated": true,
            "createdCount": result.records.len(),
            "deletedCount": result.deleted_ids.len(),
            "totalCount": 4,
            "deletedIds": result.deleted_ids,
            "status": { "memoryCount": 4, "pinnedCount": 1, "queueDepth": 0 },
            "message": "Applied Hoppi consolidation and updated Rust cache."
        }));
        assert!(rendered.contains("Hoppi maintenance: applied"));
        assert!(rendered.contains("stage=manual-maintenance"));
        assert!(rendered.contains("model=gpt-5.4-mini"));
        assert!(rendered.contains("created=1 deleted=1"));
        assert!(rendered.contains("queue=0"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn hoppi_consolidate_job_update_params_track_running_and_terminal_states() {
        let invocation = HoppiConsolidateInvocation {
            module_path: PathBuf::from("hoppi.js"),
            cwd: PathBuf::from("E:\\RepositoryV2\\oppi"),
            dry_run: false,
            stage: "idle-dream".to_string(),
            limit: None,
            budget: Some(1_500),
            agent_model: "claude".to_string(),
            deep_model: "sonnet".to_string(),
        };
        let result = HoppiConsolidateCommandResult {
            dry_run: false,
            model_used: Some("claude/haiku".to_string()),
            summary: Some("Idle Hoppi consolidation finished.".to_string()),
            records: vec![
                MemoryRecord {
                    id: "mem-a".to_string(),
                    content: "first".to_string(),
                    ..MemoryRecord::default()
                },
                MemoryRecord {
                    id: "mem-b".to_string(),
                    content: "second".to_string(),
                    ..MemoryRecord::default()
                },
            ],
            deleted_ids: vec!["mem-old".to_string()],
            memory_count: 7,
            pinned_count: Some(1),
            store_path: Some("fixture-store".to_string()),
            last_consolidated_at: Some("2026-05-31T12:00:00Z".to_string()),
            queue_depth: Some(0),
        };

        let started = hoppi_consolidate_job_started_params(
            "thread-1",
            "native-memory-job-1",
            &invocation,
            "hoppi-idle-consolidate",
        );
        assert_eq!(started["threadId"], "thread-1");
        assert_eq!(started["jobId"], "native-memory-job-1");
        assert_eq!(started["kind"], "consolidate");
        assert_eq!(started["stage"], "idle-dream");
        assert_eq!(started["status"], "running");
        assert_eq!(started["source"], "hoppi-idle-consolidate");
        assert_eq!(started["dryRun"], false);
        assert_eq!(started["backendApplied"], false);

        let finished = hoppi_consolidate_job_finished_params(
            "thread-1",
            "native-memory-job-1",
            &invocation,
            &result,
            "hoppi-idle-consolidate",
        );
        assert_eq!(finished["status"], "succeeded");
        assert_eq!(finished["recordCount"], 2);
        assert_eq!(finished["deletedCount"], 1);
        assert_eq!(finished["backendApplied"], true);
        assert!(
            finished["message"]
                .as_str()
                .unwrap()
                .contains("Idle Hoppi consolidation finished.")
        );

        let failed = hoppi_consolidate_job_failed_params(
            "thread-1",
            "native-memory-job-1",
            &invocation,
            "bridge crashed",
            "hoppi-idle-consolidate",
        );
        assert_eq!(failed["status"], "failed");
        assert_eq!(failed["backendApplied"], false);
        assert!(
            failed["message"]
                .as_str()
                .unwrap()
                .contains("bridge crashed")
        );
    }

    #[test]
    fn memory_jobs_formatter_reports_active_and_recent_jobs() {
        let rendered = format_memory_jobs(&json!({
            "queueDepth": 1,
            "totalCount": 2,
            "items": [
                {
                    "id": "mem-job-running",
                    "kind": "consolidate",
                    "stage": "idle-dream",
                    "status": "running",
                    "source": "native-shell",
                    "message": "Running idle Hoppi consolidation."
                },
                {
                    "id": "mem-job-done",
                    "kind": "remember",
                    "status": "succeeded",
                    "recordCount": 1,
                    "finishedAt": "2026-05-31T12:00:10Z"
                }
            ],
            "message": "Runtime memory worker has 1 active job."
        }));

        assert!(rendered.contains("memory jobs: active=1 total=2"));
        assert!(rendered.contains("mem-job-running"));
        assert!(rendered.contains("consolidate"));
        assert!(rendered.contains("idle-dream"));
        assert!(rendered.contains("running"));
        assert!(rendered.contains("mem-job-done"));
        assert!(rendered.contains("records=1"));
    }

    #[test]
    fn hoppi_idle_consolidate_node_bridge_passes_budget_contract() {
        let root = env::temp_dir().join(format!(
            "oppi-hoppi-idle-consolidate-node-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let module_path = root.join("hoppi-idle-consolidate-fixture.mjs");
        let workspace = root.join("workspace");
        fs::create_dir_all(&workspace).unwrap();
        fs::write(
            &module_path,
            r#"
export function createHoppiBackend() {
  return {
    async init() {},
    async consolidate(input) {
      if (!input.project.cwd.endsWith("workspace")) throw new Error(`bad cwd ${input.project.cwd}`);
      if (input.dryRun !== false) throw new Error(`bad dryRun ${input.dryRun}`);
      if (input.stage !== "idle-dream") throw new Error(`bad stage ${input.stage}`);
      if ("limit" in input) throw new Error(`unexpected limit ${input.limit}`);
      if (input.budget !== 1500) throw new Error(`bad budget ${input.budget}`);
      if (!input.modelPreference) throw new Error("missing modelPreference");
      return {
        dryRun: false,
        modelUsed: "gpt-5.4-mini",
        summary: "Idle Hoppi consolidation finished.",
        semanticCreated: 1,
        removed: 0,
        memoryCount: 7,
        queueDepth: 0
      };
    }
  };
}
"#,
        )
        .unwrap();

        let result = run_hoppi_consolidate_invocation(&HoppiConsolidateInvocation {
            module_path,
            cwd: workspace,
            dry_run: false,
            stage: "idle-dream".to_string(),
            limit: None,
            budget: Some(1_500),
            agent_model: "auto".to_string(),
            deep_model: "auto".to_string(),
        })
        .unwrap();

        assert!(!result.dry_run);
        assert_eq!(
            result.summary.as_deref(),
            Some("Idle Hoppi consolidation finished.")
        );
        assert_eq!(result.memory_count, 7);
        assert_eq!(result.queue_depth, Some(0));
        assert_eq!(result.model_used.as_deref(), Some("gpt-5.4-mini"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn hoppi_dashboard_invocation_uses_memory_settings_and_port() {
        let settings = MemorySettings {
            enabled: true,
            dashboard_port: Some(49152),
            ..MemorySettings::default()
        };

        let invocation = hoppi_dashboard_invocation_from_settings(
            &settings,
            Path::new("E:\\RepositoryV2\\oppi"),
            PathBuf::from("hoppi.js"),
        )
        .unwrap();

        assert_eq!(invocation.module_path, PathBuf::from("hoppi.js"));
        assert_eq!(invocation.cwd, PathBuf::from("E:\\RepositoryV2\\oppi"));
        assert_eq!(invocation.port, Some(49152));

        let disabled = MemorySettings {
            enabled: false,
            ..settings
        };
        assert!(
            hoppi_dashboard_invocation_from_settings(
                &disabled,
                Path::new("E:\\RepositoryV2\\oppi"),
                PathBuf::from("hoppi.js"),
            )
            .unwrap_err()
            .contains("memory disabled")
        );
    }

    #[test]
    fn hoppi_dashboard_node_bridge_starts_installed_backend_contract() {
        let root =
            env::temp_dir().join(format!("oppi-hoppi-dashboard-node-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let module_path = root.join("hoppi-dashboard-fixture.mjs");
        let workspace = root.join("workspace");
        fs::create_dir_all(&workspace).unwrap();
        fs::write(
            &module_path,
            r#"
let stopCalled = false;

export function getDefaultHoppiRoot() {
  return "fixture-root";
}

export function createHoppiBackend(options) {
  if (options.root !== "fixture-root") throw new Error(`bad root ${options.root}`);
  return {
    async init() {},
    async status(project) {
      if (!project.cwd.endsWith("workspace")) throw new Error(`bad cwd ${project.cwd}`);
      return { memoryCount: 7, pinnedCount: 2, storePath: "fixture-store" };
    },
    async startDashboard(input) {
      if (!input.project.cwd.endsWith("workspace")) throw new Error(`bad dashboard cwd ${input.project.cwd}`);
      if (input.port !== 49152) throw new Error(`bad port ${input.port}`);
      return {
        url: "http://hoppi.localhost:49152",
        async stop() { stopCalled = true; }
      };
    }
  };
}
"#,
        )
        .unwrap();

        let invocation = HoppiDashboardInvocation {
            module_path,
            cwd: workspace,
            port: Some(49152),
        };

        let mut process = run_hoppi_dashboard_invocation(&invocation).unwrap();
        assert_eq!(process.result.url, "http://hoppi.localhost:49152");
        assert_eq!(
            process.result.fallback_url.as_deref(),
            Some("http://localhost:49152")
        );
        assert_eq!(process.result.memory_count, 7);
        assert_eq!(process.result.pinned_count, 2);
        assert_eq!(process.result.store_path.as_deref(), Some("fixture-store"));
        let _ = process.child.kill();
        let _ = process.child.wait();

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn hoppi_list_node_bridge_returns_normalized_records() {
        let root = env::temp_dir().join(format!("oppi-hoppi-list-node-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let module_path = root.join("hoppi-list-fixture.mjs");
        let workspace = root.join("workspace");
        fs::create_dir_all(&workspace).unwrap();
        fs::write(
            &module_path,
            r#"
export function getDefaultHoppiRoot() {
  return "fixture-root";
}

export function createHoppiBackend(options) {
  if (options.root !== "fixture-root") throw new Error(`bad root ${options.root}`);
  return {
    async init() {},
    async status(project) {
      if (!project.cwd.endsWith("workspace")) throw new Error(`bad cwd ${project.cwd}`);
      return { memoryCount: 2, pinnedCount: 2, storePath: "fixture-store" };
    },
    async list(input) {
      if (!input.project.cwd.endsWith("workspace")) throw new Error(`bad list cwd ${input.project.cwd}`);
      if (input.includeSuperseded !== false) throw new Error("includeSuperseded should be false");
      if (input.pinnedOnly !== true) throw new Error("pinnedOnly should be true");
      if (input.limit !== 5) throw new Error(`bad limit ${input.limit}`);
      return [
        {
          id: "mem-1",
          content: "Pinned native list memory",
          tags: ["oppi-native-shell", "verified"],
          pinned: true,
          layer: "semantic",
          confidence: "verified",
          source: "oppi:native-shell",
          created_at: "2026-05-29T12:00:00Z",
          retrievalStrength: 0.8,
          retrievalCount: 3,
          parents: ["mem-parent"]
        },
        { id: "mem-2", content: "Second memory", starred: true }
      ];
    }
  };
}
"#,
        )
        .unwrap();

        let result = run_hoppi_list_invocation(&HoppiListInvocation {
            module_path,
            cwd: workspace,
            include_superseded: false,
            pinned_only: true,
            limit: Some(5),
        })
        .unwrap();

        assert_eq!(result.memory_count, 2);
        assert_eq!(result.pinned_count, 2);
        assert_eq!(result.store_path.as_deref(), Some("fixture-store"));
        assert_eq!(result.records[0].id, "mem-1");
        assert!(result.records[0].pinned);
        assert_eq!(result.records[0].retrieval_strength, Some(0.8));
        assert_eq!(result.records[0].parents, vec!["mem-parent".to_string()]);

        let rendered = format_memory_list(&json!({
            "available": true,
            "totalCount": 2,
            "items": result.records,
            "message": "Runtime memory list contains 2 cached records."
        }));
        assert!(rendered.contains("Hoppi memories: 2"));
        assert!(rendered.contains("[pinned] mem-1"));
        assert!(rendered.contains("Pinned native list memory"));
        assert!(rendered.contains("tags=oppi-native-shell, verified"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn hoppi_list_node_bridge_times_out_hung_backend_calls() {
        let root = env::temp_dir().join(format!("oppi-hoppi-list-timeout-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let module_path = root.join("hoppi-list-timeout-fixture.mjs");
        let workspace = root.join("workspace");
        fs::create_dir_all(&workspace).unwrap();
        fs::write(
            &module_path,
            r#"
export function createHoppiBackend() {
  return {
    async init() {},
    async list() {
      setInterval(() => {}, 1000);
      await new Promise(() => {});
    }
  };
}
"#,
        )
        .unwrap();

        let started = Instant::now();
        let error = run_hoppi_list_invocation_with_timeout(
            &HoppiListInvocation {
                module_path,
                cwd: workspace,
                include_superseded: false,
                pinned_only: false,
                limit: Some(50),
            },
            Duration::from_millis(150),
        )
        .unwrap_err();

        assert!(
            started.elapsed() < Duration::from_secs(2),
            "hung Hoppi bridge should be killed instead of blocking indefinitely"
        );
        assert!(error.contains("Hoppi list timed out after 150ms"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn hoppi_forget_node_bridge_runs_installed_backend_contract() {
        let root = env::temp_dir().join(format!("oppi-hoppi-forget-node-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let module_path = root.join("hoppi-forget-fixture.mjs");
        let workspace = root.join("workspace");
        fs::create_dir_all(&workspace).unwrap();
        fs::write(
            &module_path,
            r#"
export function getDefaultHoppiRoot() {
  return "fixture-root";
}

export function createHoppiBackend(options) {
  if (options.root !== "fixture-root") throw new Error(`bad root ${options.root}`);
  return {
    async init() {},
    async status(project) {
      if (!project.cwd.endsWith("workspace")) throw new Error(`bad cwd ${project.cwd}`);
      return { memoryCount: 1, pinnedCount: 0, storePath: "fixture-store" };
    },
    async forget(id) {
      if (id !== "mem-1") throw new Error(`bad id ${id}`);
    }
  };
}
"#,
        )
        .unwrap();

        let result = run_hoppi_forget_invocation(&HoppiForgetInvocation {
            module_path,
            cwd: workspace,
            id: "mem-1".to_string(),
        })
        .unwrap();

        assert_eq!(result.id, "mem-1");
        assert_eq!(result.memory_count, Some(1));
        assert_eq!(result.pinned_count, Some(0));
        assert_eq!(result.store_path.as_deref(), Some("fixture-store"));

        let rendered = format_memory_forget(&json!({
            "action": "forget",
            "id": "mem-1",
            "backendApplied": true,
            "cacheUpdated": true,
            "message": "Forgot memory record mem-1 and removed it from Rust cache.",
            "status": { "memoryCount": 1, "pinnedCount": 0 }
        }));
        assert!(rendered.contains("Forgot memory: mem-1"));
        assert!(rendered.contains("cache updated"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn hoppi_pin_node_bridge_runs_installed_backend_contract() {
        let root = env::temp_dir().join(format!("oppi-hoppi-pin-node-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let module_path = root.join("hoppi-pin-fixture.mjs");
        let workspace = root.join("workspace");
        fs::create_dir_all(&workspace).unwrap();
        fs::write(
            &module_path,
            r#"
export function getDefaultHoppiRoot() {
  return "fixture-root";
}

export function createHoppiBackend(options) {
  if (options.root !== "fixture-root") throw new Error(`bad root ${options.root}`);
  return {
    async init() {},
    async status(project) {
      if (!project.cwd.endsWith("workspace")) throw new Error(`bad cwd ${project.cwd}`);
      return { memoryCount: 1, pinnedCount: 1, storePath: "fixture-store" };
    },
    async pin(id, pinned) {
      if (id !== "mem-1") throw new Error(`bad id ${id}`);
      if (pinned !== true) throw new Error(`bad pinned ${pinned}`);
      return { id, content: "Pinned through native shell", pinned, tags: ["verified"] };
    }
  };
}
"#,
        )
        .unwrap();

        let result = run_hoppi_pin_invocation(&HoppiPinInvocation {
            module_path,
            cwd: workspace,
            id: "mem-1".to_string(),
            pinned: true,
        })
        .unwrap();

        assert_eq!(result.id, "mem-1");
        assert_eq!(result.memory_count, Some(1));
        assert_eq!(result.pinned_count, Some(1));
        assert_eq!(result.store_path.as_deref(), Some("fixture-store"));
        assert_eq!(
            result.record.as_ref().map(|record| record.pinned),
            Some(true)
        );

        let rendered = format_memory_pin(&json!({
            "action": "pin",
            "id": "mem-1",
            "backendApplied": true,
            "cacheUpdated": true,
            "message": "Pinned memory record mem-1 and updated Rust cache.",
            "status": { "memoryCount": 1, "pinnedCount": 1 }
        }));
        assert!(rendered.contains("Pinned memory: mem-1"));
        assert!(rendered.contains("cache updated"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn hoppi_update_node_bridge_runs_installed_backend_contract() {
        let root = env::temp_dir().join(format!("oppi-hoppi-update-node-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let module_path = root.join("hoppi-update-fixture.mjs");
        let workspace = root.join("workspace");
        fs::create_dir_all(&workspace).unwrap();
        fs::write(
            &module_path,
            r#"
export function getDefaultHoppiRoot() {
  return "fixture-root";
}

export function createHoppiBackend(options) {
  if (options.root !== "fixture-root") throw new Error(`bad root ${options.root}`);
  return {
    async init() {},
    async status(project) {
      if (!project.cwd.endsWith("workspace")) throw new Error(`bad cwd ${project.cwd}`);
      return { memoryCount: 1, pinnedCount: 0, storePath: "fixture-store" };
    },
    async update(id, patch) {
      if (id !== "mem-1") throw new Error(`bad id ${id}`);
      if (patch.content !== "Edited through native shell") throw new Error(`bad content ${patch.content}`);
      if (!Array.isArray(patch.tags) || patch.tags.join(",") !== "verified,native") throw new Error(`bad tags ${patch.tags}`);
      return { id, content: patch.content, tags: patch.tags, confidence: patch.confidence, source: "fixture" };
    }
  };
}
"#,
        )
        .unwrap();

        let result = run_hoppi_update_invocation(&HoppiUpdateInvocation {
            module_path,
            cwd: workspace,
            id: "mem-1".to_string(),
            patch: json!({
                "content": "Edited through native shell",
                "tags": ["verified", "native"],
                "confidence": "verified"
            }),
        })
        .unwrap();

        assert_eq!(result.id, "mem-1");
        assert_eq!(result.memory_count, Some(1));
        assert_eq!(result.pinned_count, Some(0));
        assert_eq!(result.store_path.as_deref(), Some("fixture-store"));
        let record = result.record.as_ref().unwrap();
        assert_eq!(record.content, "Edited through native shell");
        assert_eq!(record.tags, vec!["verified", "native"]);
        assert_eq!(record.confidence.as_deref(), Some("verified"));

        let rendered = format_memory_update(&json!({
            "action": "update",
            "id": "mem-1",
            "backendApplied": true,
            "cacheUpdated": true,
            "message": "Updated memory record mem-1 and updated Rust cache.",
            "status": { "memoryCount": 1, "pinnedCount": 0 }
        }));
        assert!(rendered.contains("Updated memory: mem-1"));
        assert!(rendered.contains("cache updated"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn hoppi_update_bridge_and_formatter_cover_memory_supersede() {
        let patch = parse_memory_update_patch_args(&["supersededBy=mem-new"]).unwrap();
        assert_eq!(patch["supersededBy"], json!("mem-new"));

        let rendered = format_memory_supersede(&json!({
            "action": "supersede",
            "id": "mem-old",
            "backendApplied": true,
            "cacheUpdated": true,
            "record": {
                "id": "mem-old",
                "content": "Older memory.",
                "supersededBy": "mem-new"
            },
            "status": { "memoryCount": 2, "pinnedCount": 0 },
            "message": "Marked memory record mem-old as superseded by mem-new and updated Rust cache."
        }));
        assert!(rendered.contains("Superseded memory: mem-old -> mem-new"));
        assert!(rendered.contains("backend=applied cache updated"));
    }

    #[test]
    fn hoppi_recall_invocation_plans_startup_and_task_recall_from_settings() {
        let settings = MemorySettings {
            enabled: true,
            startup_recall: true,
            task_start_recall: true,
            ..MemorySettings::default()
        };

        let invocation = hoppi_recall_invocation_from_settings(
            &settings,
            Path::new("E:\\RepositoryV2\\oppi"),
            "review native memory recall",
            PathBuf::from("hoppi.js"),
        )
        .unwrap();

        assert_eq!(invocation.module_path, PathBuf::from("hoppi.js"));
        assert_eq!(invocation.cwd, PathBuf::from("E:\\RepositoryV2\\oppi"));
        assert_eq!(invocation.prompt, "review native memory recall");
        assert!(invocation.include_startup);
        assert!(invocation.include_task);

        let mut disabled = settings.clone();
        disabled.enabled = false;
        assert!(
            hoppi_recall_invocation_from_settings(
                &disabled,
                Path::new("E:\\RepositoryV2\\oppi"),
                "review native memory recall",
                PathBuf::from("hoppi.js"),
            )
            .unwrap_err()
            .contains("memory disabled")
        );

        let recall_disabled = MemorySettings {
            startup_recall: false,
            task_start_recall: false,
            ..settings
        };
        assert!(
            hoppi_recall_invocation_from_settings(
                &recall_disabled,
                Path::new("E:\\RepositoryV2\\oppi"),
                "review native memory recall",
                PathBuf::from("hoppi.js"),
            )
            .unwrap_err()
            .contains("recall disabled")
        );
    }

    #[test]
    fn hoppi_recall_node_bridge_runs_installed_backend_contract() {
        let root = env::temp_dir().join(format!("oppi-hoppi-recall-node-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let module_path = root.join("hoppi-recall-fixture.mjs");
        let workspace = root.join("workspace");
        fs::create_dir_all(&workspace).unwrap();
        fs::write(
            &module_path,
            r#"
export function getDefaultHoppiRoot() {
  return "fixture-root";
}

export function createHoppiBackend(options) {
  if (options.root !== "fixture-root") throw new Error(`bad root ${options.root}`);
  return {
    async init() {},
    async status(project) {
      if (!project.cwd.endsWith("workspace")) throw new Error(`bad cwd ${project.cwd}`);
      return { memoryCount: 3 };
    },
    async buildStartupContext(request) {
      if (request.maxMemories !== 8) throw new Error("bad startup max");
      if (request.includePinned !== true) throw new Error("pinned startup memories required");
      return {
        contextMarkdown: "Startup memory\n- prefer native shell ownership",
        memoryCount: 2
      };
    },
    async recall(request) {
      if (request.query !== "ship task recall") throw new Error(`bad query ${request.query}`);
      if (request.budget !== 900) throw new Error(`bad budget ${request.budget}`);
      if (request.limit !== 5) throw new Error(`bad limit ${request.limit}`);
      return {
        contextMarkdown: "Task memory\n- inject recalled context into provider prompt",
        memories: [
          {
            id: "a",
            content: "Recall records should flow through the Rust runtime boundary.",
            tags: ["oppi-native-shell", "recall"],
            pinned: true,
            retrievalStrength: 0.84,
            retrievalCount: 2
          },
          { id: "b", content: "Keep direct-provider prompts grounded in recalled memory." },
          { id: "c", content: "Use current files when they conflict with memory." },
          { id: "d", content: "Expose recall records to non-shell clients." }
        ]
      };
    }
  };
}
"#,
        )
        .unwrap();

        let invocation = HoppiRecallInvocation {
            module_path,
            cwd: workspace,
            prompt: "ship task recall".to_string(),
            include_startup: true,
            include_task: true,
        };

        let result = run_hoppi_recall_invocation(&invocation).unwrap();
        let context = result.context_markdown.unwrap();
        assert!(context.contains("Startup memory"));
        assert!(context.contains("Task memory"));
        assert!(context.contains("Memory is advisory"));
        assert!(result.context_bytes > 0);
        assert_eq!(result.memory_count, 4);
        assert_eq!(result.records.len(), 4);
        assert_eq!(result.records[0].id, "a");
        assert_eq!(
            result.records[0].content,
            "Recall records should flow through the Rust runtime boundary."
        );
        assert!(result.records[0].pinned);
        assert_eq!(result.records[0].retrieval_strength, Some(0.84));
        assert!(!result.truncated);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn hoppi_recall_node_bridge_continues_when_status_or_startup_fails() {
        let root = env::temp_dir().join(format!(
            "oppi-hoppi-recall-soft-fail-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let module_path = root.join("hoppi-recall-soft-fail-fixture.mjs");
        let workspace = root.join("workspace");
        fs::create_dir_all(&workspace).unwrap();
        fs::write(
            &module_path,
            r#"
export function createHoppiBackend() {
  return {
    async init() {},
    async status() {
      throw new Error("status offline");
    },
    async buildStartupContext() {
      throw new Error("startup offline");
    },
    async recall() {
      return {
        contextMarkdown: "Task memory survived soft failures",
        memories: [{ id: "task" }]
      };
    }
  };
}
"#,
        )
        .unwrap();

        let invocation = HoppiRecallInvocation {
            module_path,
            cwd: workspace,
            prompt: "ship task recall".to_string(),
            include_startup: true,
            include_task: true,
        };

        let result = run_hoppi_recall_invocation(&invocation).unwrap();
        assert!(
            result
                .context_markdown
                .unwrap()
                .contains("Task memory survived soft failures")
        );
        assert_eq!(result.memory_count, 1);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn hoppi_recall_job_update_params_track_running_and_terminal_states() {
        let invocation = HoppiRecallInvocation {
            module_path: PathBuf::from("hoppi.js"),
            cwd: PathBuf::from("E:\\RepositoryV2\\oppi"),
            prompt: "sensitive current user prompt".to_string(),
            include_startup: true,
            include_task: true,
        };
        let result = HoppiRecallCommandResult {
            context_markdown: Some("Recalled context".to_string()),
            context_bytes: 128,
            memory_count: 3,
            pinned_count: Some(1),
            store_path: Some("fixture-store".to_string()),
            records: vec![
                MemoryRecord {
                    id: "mem-a".to_string(),
                    content: "first".to_string(),
                    ..MemoryRecord::default()
                },
                MemoryRecord {
                    id: "mem-b".to_string(),
                    content: "second".to_string(),
                    ..MemoryRecord::default()
                },
            ],
            omitted_reason: None,
            truncated: false,
        };

        let started =
            hoppi_recall_job_started_params("thread-1", "native-recall-job-1", &invocation);
        assert_eq!(started["threadId"], "thread-1");
        assert_eq!(started["jobId"], "native-recall-job-1");
        assert_eq!(started["kind"], "recall");
        assert_eq!(started["stage"], "recall-startup-task");
        assert_eq!(started["status"], "running");
        assert_eq!(started["source"], "hoppi-recall");
        assert_eq!(started["backendApplied"], false);
        assert!(
            !started["message"]
                .as_str()
                .unwrap()
                .contains("sensitive current user prompt")
        );

        let finished = hoppi_recall_job_finished_params(
            "thread-1",
            "native-recall-job-1",
            &invocation,
            &result,
        );
        assert_eq!(finished["status"], "succeeded");
        assert_eq!(finished["recordCount"], 2);
        assert_eq!(finished["backendApplied"], true);
        assert!(finished["message"].as_str().unwrap().contains("2 recalled"));

        let failed = hoppi_recall_job_failed_params(
            "thread-1",
            "native-recall-job-1",
            &invocation,
            "recall bridge failed",
        );
        assert_eq!(failed["status"], "failed");
        assert_eq!(failed["backendApplied"], false);
        assert!(
            failed["message"]
                .as_str()
                .unwrap()
                .contains("recall bridge failed")
        );
    }

    #[test]
    fn hoppi_memory_context_appends_to_provider_system_prompt_once() {
        let config = OpenAiCompatibleConfig {
            flavor: DirectProviderFlavor::OpenAiCompatible,
            provider_id: None,
            model: "mock".to_string(),
            base_url: None,
            api_key_env: None,
            system_prompt: Some("Base system".to_string()),
            temperature: None,
            reasoning_effort: None,
            max_output_tokens: None,
            stream: true,
        };
        let mut provider = openai_provider_json(&config);

        append_hoppi_memory_context_to_provider(
            &mut provider,
            "Remember the native shell owns Hoppi recall.",
        );
        append_hoppi_memory_context_to_provider(
            &mut provider,
            "Remember the native shell owns Hoppi recall.",
        );

        let system = provider["systemPrompt"].as_str().unwrap();
        assert!(system.contains("Base system"));
        assert!(system.contains("OPPi Hoppi memory"));
        assert!(system.contains("native shell owns Hoppi recall"));
        assert_eq!(system.matches("OPPi Hoppi memory:").count(), 1);
    }

    #[test]
    fn hoppi_turn_summary_from_events_builds_bounded_agent_end_memory() {
        let turn_id = "turn-native-memory";
        let events = vec![
            Event {
                id: 1,
                thread_id: "thread-1".to_string(),
                turn_id: Some(turn_id.to_string()),
                created_at_ms: 1,
                kind: EventKind::ItemCompleted {
                    item: oppi_protocol::Item {
                        id: "user-1".to_string(),
                        thread_id: "thread-1".to_string(),
                        turn_id: turn_id.to_string(),
                        kind: ItemKind::UserMessage {
                            text: "Please wire native Hoppi turn summary writes with tests first."
                                .to_string(),
                        },
                    },
                },
            },
            Event {
                id: 2,
                thread_id: "thread-1".to_string(),
                turn_id: Some(turn_id.to_string()),
                created_at_ms: 2,
                kind: EventKind::ToolCallCompleted {
                    result: oppi_protocol::ToolResult {
                        call_id: "tool-1".to_string(),
                        status: ToolResultStatus::Ok,
                        output: Some("patched".to_string()),
                        error: None,
                    },
                },
            },
            Event {
                id: 3,
                thread_id: "thread-1".to_string(),
                turn_id: Some(turn_id.to_string()),
                created_at_ms: 3,
                kind: EventKind::ItemCompleted {
                    item: oppi_protocol::Item {
                        id: "assistant-1".to_string(),
                        thread_id: "thread-1".to_string(),
                        turn_id: turn_id.to_string(),
                        kind: ItemKind::AssistantMessage {
                            text: "Implemented the native Hoppi remember bridge and kept the turn non-blocking.".to_string(),
                        },
                    },
                },
            },
        ];

        let summary = build_hoppi_turn_summary_from_events(
            &events,
            "thread-1",
            turn_id,
            Some("fallback prompt"),
            "2026-05-27T12:00:00Z",
        )
        .unwrap();

        assert!(summary.contains("Native shell turn summary (2026-05-27T12:00:00Z)"));
        assert!(summary.contains("User asked: Please wire native Hoppi turn summary writes"));
        assert!(
            summary.contains("Assistant outcome: Implemented the native Hoppi remember bridge")
        );
        assert!(summary.contains("Tool results: ok"));
        assert!(summary.contains("Runtime turn: thread-1/turn-native-memory"));
    }

    #[test]
    fn memory_distill_turn_from_events_builds_model_distiller_request() {
        let events = vec![
            Event {
                id: 1,
                thread_id: "thread-1".to_string(),
                turn_id: Some("turn-1".to_string()),
                created_at_ms: 1,
                kind: EventKind::ItemCompleted {
                    item: oppi_protocol::Item {
                        id: "user-1".to_string(),
                        thread_id: "thread-1".to_string(),
                        turn_id: "turn-1".to_string(),
                        kind: ItemKind::UserMessage {
                            text: "Please compare Rust-native OPPi memory against Plan 75."
                                .to_string(),
                        },
                    },
                },
            },
            Event {
                id: 2,
                thread_id: "thread-1".to_string(),
                turn_id: Some("turn-1".to_string()),
                created_at_ms: 2,
                kind: EventKind::ItemCompleted {
                    item: oppi_protocol::Item {
                        id: "assistant-1".to_string(),
                        thread_id: "thread-1".to_string(),
                        turn_id: "turn-1".to_string(),
                        kind: ItemKind::AssistantMessage {
                            text: "Mapped the remaining native memory lifecycle gaps.".to_string(),
                        },
                    },
                },
            },
            Event {
                id: 3,
                thread_id: "thread-1".to_string(),
                turn_id: Some("turn-2".to_string()),
                created_at_ms: 3,
                kind: EventKind::ItemCompleted {
                    item: oppi_protocol::Item {
                        id: "user-2".to_string(),
                        thread_id: "thread-1".to_string(),
                        turn_id: "turn-2".to_string(),
                        kind: ItemKind::UserMessage {
                            text: "Wire memory/distill-turn into the native shell lifecycle."
                                .to_string(),
                        },
                    },
                },
            },
            Event {
                id: 4,
                thread_id: "thread-1".to_string(),
                turn_id: Some("turn-2".to_string()),
                created_at_ms: 4,
                kind: EventKind::ItemCompleted {
                    item: oppi_protocol::Item {
                        id: "assistant-2".to_string(),
                        thread_id: "thread-1".to_string(),
                        turn_id: "turn-2".to_string(),
                        kind: ItemKind::AssistantMessage {
                            text: "Added a focused request builder test before touching the lifecycle hook."
                                .to_string(),
                        },
                    },
                },
            },
            Event {
                id: 5,
                thread_id: "other-thread".to_string(),
                turn_id: Some("turn-2".to_string()),
                created_at_ms: 5,
                kind: EventKind::ItemCompleted {
                    item: oppi_protocol::Item {
                        id: "other-assistant".to_string(),
                        thread_id: "other-thread".to_string(),
                        turn_id: "turn-2".to_string(),
                        kind: ItemKind::AssistantMessage {
                            text: "This should not leak into the distiller request.".to_string(),
                        },
                    },
                },
            },
        ];
        let settings = MemorySettings {
            enabled: true,
            turn_summaries: true,
            agent_model: "gpt".to_string(),
            ..MemorySettings::default()
        };
        let provider = OpenAiCompatibleConfig {
            flavor: DirectProviderFlavor::OpenAiCompatible,
            provider_id: Some("fixture-provider".to_string()),
            model: "fixture-memory-model".to_string(),
            base_url: Some("http://127.0.0.1:4321".to_string()),
            api_key_env: Some("OPPI_TEST_MEMORY_MODEL_KEY".to_string()),
            system_prompt: Some("Base system".to_string()),
            temperature: Some(0.2),
            reasoning_effort: Some("medium".to_string()),
            max_output_tokens: Some(2048),
            stream: true,
        };

        let params = memory_distill_turn_params_from_events(
            &settings,
            &events,
            Path::new("E:\\RepositoryV2\\oppi"),
            "thread-1",
            "turn-2",
            Some("fallback prompt must not override event user text"),
            &provider,
        )
        .unwrap();

        assert_eq!(params["threadId"], "thread-1");
        assert_eq!(
            params["userText"],
            "Wire memory/distill-turn into the native shell lifecycle."
        );
        assert_eq!(
            params["assistantText"],
            "Added a focused request builder test before touching the lifecycle hook."
        );
        assert_eq!(params["sourceProject"], "E:\\RepositoryV2\\oppi");
        assert_eq!(params["sourceSessionId"], "thread-1:turn-2");
        assert_eq!(params["agentModel"], "gpt");
        assert_eq!(
            params["recentUserTexts"],
            json!([
                "Please compare Rust-native OPPi memory against Plan 75.",
                "Wire memory/distill-turn into the native shell lifecycle."
            ])
        );
        assert_eq!(
            params["recentAssistantTexts"],
            json!([
                "Mapped the remaining native memory lifecycle gaps.",
                "Added a focused request builder test before touching the lifecycle hook."
            ])
        );
        assert_eq!(params["modelProvider"]["kind"], "openai-compatible");
        assert_eq!(params["modelProvider"]["model"], "fixture-memory-model");
        assert_eq!(params["modelProvider"]["baseUrl"], "http://127.0.0.1:4321");
        assert_eq!(
            params["modelProvider"]["apiKeyEnv"],
            "OPPI_TEST_MEMORY_MODEL_KEY"
        );
        assert_eq!(params["modelProvider"]["stream"], true);
        assert!(
            !params["assistantText"]
                .as_str()
                .unwrap()
                .contains("other thread")
        );
    }

    #[test]
    fn hoppi_session_recap_from_events_builds_recent_exit_memory() {
        let events = vec![
            Event {
                id: 1,
                thread_id: "thread-1".to_string(),
                turn_id: Some("turn-1".to_string()),
                created_at_ms: 1,
                kind: EventKind::ItemCompleted {
                    item: oppi_protocol::Item {
                        id: "user-1".to_string(),
                        thread_id: "thread-1".to_string(),
                        turn_id: "turn-1".to_string(),
                        kind: ItemKind::UserMessage {
                            text: "Please compare Rust native OPPi against the memory plan and keep the audit actionable.".to_string(),
                        },
                    },
                },
            },
            Event {
                id: 2,
                thread_id: "thread-1".to_string(),
                turn_id: Some("turn-1".to_string()),
                created_at_ms: 2,
                kind: EventKind::ItemCompleted {
                    item: oppi_protocol::Item {
                        id: "assistant-1".to_string(),
                        thread_id: "thread-1".to_string(),
                        turn_id: "turn-1".to_string(),
                        kind: ItemKind::AssistantMessage {
                            text: "Identified that /exit recap writes were still missing after normal turn summaries landed.".to_string(),
                        },
                    },
                },
            },
            Event {
                id: 3,
                thread_id: "thread-1".to_string(),
                turn_id: Some("turn-2".to_string()),
                created_at_ms: 3,
                kind: EventKind::ItemCompleted {
                    item: oppi_protocol::Item {
                        id: "assistant-2".to_string(),
                        thread_id: "other-thread".to_string(),
                        turn_id: "turn-2".to_string(),
                        kind: ItemKind::AssistantMessage {
                            text: "This other thread must not leak into the recap.".to_string(),
                        },
                    },
                },
            },
            Event {
                id: 4,
                thread_id: "thread-1".to_string(),
                turn_id: Some("turn-3".to_string()),
                created_at_ms: 4,
                kind: EventKind::ItemCompleted {
                    item: oppi_protocol::Item {
                        id: "user-2".to_string(),
                        thread_id: "thread-1".to_string(),
                        turn_id: "turn-3".to_string(),
                        kind: ItemKind::UserMessage {
                            text: "Now implement the native /exit recap write before sync shutdown.".to_string(),
                        },
                    },
                },
            },
            Event {
                id: 5,
                thread_id: "thread-1".to_string(),
                turn_id: Some("turn-3".to_string()),
                created_at_ms: 5,
                kind: EventKind::ItemCompleted {
                    item: oppi_protocol::Item {
                        id: "assistant-3".to_string(),
                        thread_id: "thread-1".to_string(),
                        turn_id: "turn-3".to_string(),
                        kind: ItemKind::AssistantMessage {
                            text: "Added a bounded session recap builder and an exit-specific Hoppi remember invocation.".to_string(),
                        },
                    },
                },
            },
        ];

        let recap =
            build_hoppi_session_recap_from_events(&events, "thread-1", "2026-05-27T13:00:00Z")
                .unwrap();

        assert!(recap.contains("Session recap (2026-05-27T13:00:00Z)"));
        assert!(recap.contains("Recent user goals:"));
        assert!(
            recap.contains("- Now implement the native /exit recap write before sync shutdown.")
        );
        assert!(recap.contains("Recent assistant outcomes:"));
        assert!(recap.contains("- Added a bounded session recap builder"));
        assert!(!recap.contains("other thread"));
    }

    #[test]
    fn hoppi_remember_invocation_refuses_disabled_memory_or_turn_summaries() {
        let mut settings = MemorySettings {
            enabled: false,
            turn_summaries: true,
            ..MemorySettings::default()
        };
        assert!(
            hoppi_remember_invocation_from_settings(
                &settings,
                Path::new("E:\\RepositoryV2\\oppi"),
                "summary",
                "thread-1",
                "turn-1",
                PathBuf::from("hoppi.js"),
            )
            .unwrap_err()
            .contains("memory disabled")
        );

        settings.enabled = true;
        settings.turn_summaries = false;
        assert!(
            hoppi_remember_invocation_from_settings(
                &settings,
                Path::new("E:\\RepositoryV2\\oppi"),
                "summary",
                "thread-1",
                "turn-1",
                PathBuf::from("hoppi.js"),
            )
            .unwrap_err()
            .contains("turn summaries disabled")
        );
    }

    #[test]
    fn hoppi_remember_node_bridge_runs_installed_backend_contract() {
        let root = env::temp_dir().join(format!("oppi-hoppi-remember-node-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let module_path = root.join("hoppi-remember-fixture.mjs");
        let workspace = root.join("workspace");
        fs::create_dir_all(&workspace).unwrap();
        fs::write(
            &module_path,
            r#"
export function getDefaultHoppiRoot() {
  return "fixture-root";
}

export function createHoppiBackend(options) {
  if (options.root !== "fixture-root") throw new Error(`bad root ${options.root}`);
  return {
    async init() {},
    async remember(input) {
      if (!input.project.cwd.endsWith("workspace")) throw new Error(`bad cwd ${input.project.cwd}`);
      if (input.content !== "Native shell turn summary body") throw new Error(`bad content ${input.content}`);
      if (!input.tags.includes("oppi-native-shell")) throw new Error("missing native tag");
      if (!input.tags.includes("oppi-turn-summary")) throw new Error("missing summary tag");
      if (input.layer !== "buffer") throw new Error(`bad layer ${input.layer}`);
      if (input.confidence !== "observed") throw new Error(`bad confidence ${input.confidence}`);
      if (input.source !== "oppi:native-shell") throw new Error(`bad source ${input.source}`);
      if (input.sourceSessionId !== "thread-1:turn-1") throw new Error(`bad session ${input.sourceSessionId}`);
      return {
        id: "memory-1",
        content: input.content,
        tags: input.tags,
        layer: input.layer,
        confidence: input.confidence,
        source: input.source
      };
    },
    async status() {
      return { memoryCount: 9 };
    }
  };
}
"#,
        )
        .unwrap();

        let invocation = HoppiRememberInvocation {
            module_path,
            cwd: workspace,
            summary: "Native shell turn summary body".to_string(),
            tags: vec![
                "oppi-native-shell".to_string(),
                "oppi-turn-summary".to_string(),
                "agent_end".to_string(),
            ],
            source: "oppi:native-shell".to_string(),
            source_session_id: "thread-1:turn-1".to_string(),
            layer: Some("buffer".to_string()),
        };

        let result = run_hoppi_remember_invocation(&invocation).unwrap();
        assert!(result.saved);
        assert_eq!(result.memory_count, 9);
        let record = result.record.as_ref().unwrap();
        assert_eq!(record.id, "memory-1");
        assert_eq!(record.content, "Native shell turn summary body");
        assert_eq!(
            record.tags,
            vec!["oppi-native-shell", "oppi-turn-summary", "agent_end"]
        );
        assert_eq!(record.layer.as_deref(), Some("buffer"));
        assert_eq!(record.confidence.as_deref(), Some("observed"));
        assert_eq!(record.source.as_deref(), Some("oppi:native-shell"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn hoppi_remember_node_bridge_runs_exit_recap_contract() {
        let root =
            env::temp_dir().join(format!("oppi-hoppi-exit-recap-node-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let module_path = root.join("hoppi-exit-recap-fixture.mjs");
        let workspace = root.join("workspace");
        fs::create_dir_all(&workspace).unwrap();
        fs::write(
            &module_path,
            r#"
export function createHoppiBackend() {
  return {
    async init() {},
    async remember(input) {
      if (!input.project.cwd.endsWith("workspace")) throw new Error(`bad cwd ${input.project.cwd}`);
      if (input.content !== "Native shell exit recap body") throw new Error(`bad content ${input.content}`);
      if (!input.tags.includes("oppi-exit-recap")) throw new Error("missing exit tag");
      if (!input.tags.includes("session-recap")) throw new Error("missing session tag");
      if ("layer" in input) throw new Error(`unexpected layer ${input.layer}`);
      if (input.confidence !== "observed") throw new Error(`bad confidence ${input.confidence}`);
      if (input.source !== "oppi:exit") throw new Error(`bad source ${input.source}`);
      if (input.sourceSessionId !== "thread-1") throw new Error(`bad session ${input.sourceSessionId}`);
      return {
        id: "memory-exit",
        content: input.content,
        tags: input.tags,
        confidence: input.confidence,
        source: input.source
      };
    },
    async status() {
      return { memoryCount: 12 };
    }
  };
}
"#,
        )
        .unwrap();

        let invocation = HoppiRememberInvocation {
            module_path,
            cwd: workspace,
            summary: "Native shell exit recap body".to_string(),
            tags: vec![
                "oppi-native-shell".to_string(),
                "oppi-exit-recap".to_string(),
                "session-recap".to_string(),
            ],
            source: "oppi:exit".to_string(),
            source_session_id: "thread-1".to_string(),
            layer: None,
        };

        let result = run_hoppi_remember_invocation(&invocation).unwrap();
        assert!(result.saved);
        assert_eq!(result.memory_count, 12);
        let record = result.record.as_ref().unwrap();
        assert_eq!(record.id, "memory-exit");
        assert_eq!(record.content, "Native shell exit recap body");
        assert_eq!(
            record.tags,
            vec!["oppi-native-shell", "oppi-exit-recap", "session-recap"]
        );
        assert_eq!(record.layer, None);
        assert_eq!(record.confidence.as_deref(), Some("observed"));
        assert_eq!(record.source.as_deref(), Some("oppi:exit"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn hoppi_first_start_offer_only_prompts_when_memory_enabled_ask_and_missing() {
        let mut settings = MemorySettings::default();

        assert!(should_offer_hoppi_first_start_install(&settings, false));
        assert!(!should_offer_hoppi_first_start_install(&settings, true));

        settings.enabled = false;
        assert!(!should_offer_hoppi_first_start_install(&settings, false));

        settings.enabled = true;
        settings.hoppi_install_offer = "dismissed".to_string();
        assert!(!should_offer_hoppi_first_start_install(&settings, false));
    }

    #[test]
    fn hoppi_first_start_install_prompt_is_explicit_and_dismissible() {
        let text = hoppi_first_start_install_prompt();

        assert!(text.contains("Install Hoppi memory?"));
        assert!(text.contains("@oppiai/hoppi-memory"));
        assert!(text.contains("npm"));
        assert!(text.contains("/memory install-hoppi --yes"));
        assert!(text.contains("/memory install-offer dismissed"));
        assert!(text.contains("no hidden install"));
    }

    #[test]
    fn memory_settings_persist_pi_memory_config_without_losing_other_settings() {
        let root = env::temp_dir().join(format!("oppi-memory-settings-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let path = root.join("settings.json");
        fs::write(
            &path,
            r#"{"oppi":{"permissionMode":"auto-review","memory":{"enabled":false,"agentModel":"claude","deepModel":"sonnet","startupRecall":false,"taskStartRecall":true,"turnSummaries":false,"idleConsolidation":false,"dashboardPort":41234,"hoppiInstallOffer":"dismissed","sync":{"enabled":true,"provider":"github","repoPath":"~/hoppi-sync","repoUrl":"git@github.com:user/hoppi-memories.git","pullOnStartup":false,"pushOnExit":true,"backgroundSync":"30m","encryption":"passphrase","passphraseSource":"file","passphraseEnv":"OPPI_OLD","passphraseFile":"~/hoppi-passphrase","conflictMode":"keep-local"}}},"kept":true}"#,
        )
        .unwrap();

        let mut settings = load_memory_settings(&path);
        assert!(!settings.enabled);
        assert_eq!(settings.agent_model, "claude");
        assert_eq!(settings.deep_model, "sonnet");
        assert!(!settings.startup_recall);
        assert!(settings.task_start_recall);
        assert!(!settings.turn_summaries);
        assert!(!settings.idle_consolidation);
        assert_eq!(settings.dashboard_port, Some(41234));
        assert_eq!(settings.hoppi_install_offer, "dismissed");
        assert!(settings.sync_enabled);
        assert_eq!(settings.sync_repo_path.as_deref(), Some("~/hoppi-sync"));
        assert_eq!(
            settings.sync_repo_url.as_deref(),
            Some("git@github.com:user/hoppi-memories.git")
        );
        assert!(!settings.sync_pull_on_startup);
        assert!(settings.sync_push_on_exit);
        assert_eq!(settings.sync_background, "30m");
        assert_eq!(settings.sync_encryption, "passphrase");
        assert_eq!(settings.sync_passphrase_source, "file");
        assert_eq!(settings.sync_passphrase_env, "OPPI_OLD");
        assert_eq!(
            settings.sync_passphrase_file.as_deref(),
            Some("~/hoppi-passphrase")
        );
        assert_eq!(settings.sync_conflict_mode, "keep-local");

        settings.enabled = true;
        settings.agent_model = "gpt".to_string();
        settings.deep_model = "gpt-5.5".to_string();
        settings.startup_recall = true;
        settings.task_start_recall = false;
        settings.turn_summaries = true;
        settings.idle_consolidation = true;
        settings.dashboard_port = None;
        settings.hoppi_install_offer = "ask".to_string();
        settings.sync_enabled = false;
        settings.sync_repo_path = Some("~/new-hoppi-sync".to_string());
        settings.sync_repo_url = None;
        settings.sync_pull_on_startup = true;
        settings.sync_push_on_exit = false;
        settings.sync_background = "60m".to_string();
        settings.sync_encryption = "passphrase".to_string();
        settings.sync_passphrase_source = "env".to_string();
        settings.sync_passphrase_env = "OPPI_HOPPI_SYNC_PASSPHRASE".to_string();
        settings.sync_passphrase_file = None;
        settings.sync_conflict_mode = "overwrite-local".to_string();
        save_memory_settings(&path, &settings).unwrap();

        let raw: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(raw["oppi"]["permissionMode"], json!("auto-review"));
        assert_eq!(raw["oppi"]["memory"]["enabled"], json!(true));
        assert_eq!(raw["oppi"]["memory"]["agentModel"], json!("gpt"));
        assert_eq!(raw["oppi"]["memory"]["deepModel"], json!("gpt-5.5"));
        assert_eq!(raw["oppi"]["memory"]["startupRecall"], json!(true));
        assert_eq!(raw["oppi"]["memory"]["taskStartRecall"], json!(false));
        assert_eq!(raw["oppi"]["memory"]["turnSummaries"], json!(true));
        assert_eq!(raw["oppi"]["memory"]["idleConsolidation"], json!(true));
        assert_eq!(raw["oppi"]["memory"]["dashboardPort"], json!("auto"));
        assert_eq!(raw["oppi"]["memory"]["hoppiInstallOffer"], json!("ask"));
        assert_eq!(raw["oppi"]["memory"]["sync"]["enabled"], json!(false));
        assert_eq!(raw["oppi"]["memory"]["sync"]["provider"], json!("github"));
        assert_eq!(
            raw["oppi"]["memory"]["sync"]["repoPath"],
            json!("~/new-hoppi-sync")
        );
        assert_eq!(raw["kept"], json!(true));

        let loaded = load_memory_settings(&path);
        assert!(loaded.enabled);
        assert_eq!(loaded.agent_model, "gpt");
        assert_eq!(loaded.deep_model, "gpt-5.5");
        assert!(loaded.startup_recall);
        assert!(!loaded.task_start_recall);
        assert!(loaded.turn_summaries);
        assert!(loaded.idle_consolidation);
        assert_eq!(loaded.dashboard_port, None);
        assert_eq!(loaded.hoppi_install_offer, "ask");
        assert!(!loaded.sync_enabled);
        assert_eq!(loaded.sync_repo_path.as_deref(), Some("~/new-hoppi-sync"));
        assert_eq!(loaded.sync_repo_url, None);
        assert!(loaded.sync_pull_on_startup);
        assert!(!loaded.sync_push_on_exit);
        assert_eq!(loaded.sync_background, "60m");
        assert_eq!(loaded.sync_encryption, "passphrase");
        assert_eq!(loaded.sync_passphrase_source, "env");
        assert_eq!(loaded.sync_passphrase_env, "OPPI_HOPPI_SYNC_PASSPHRASE");
        assert_eq!(loaded.sync_passphrase_file, None);
        assert_eq!(loaded.sync_conflict_mode, "overwrite-local");
        let status = format_memory_settings_status(&loaded);
        assert!(status.contains("sync disabled"));
        assert!(status.contains("repo ~/new-hoppi-sync"));
        assert!(status.contains("background 60m"));
        assert!(status.contains("encryption passphrase/env"));
        assert!(status.contains("conflict overwrite-local"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn compaction_settings_persist_pi_idle_and_smart_config_without_losing_other_settings() {
        let root = env::temp_dir().join(format!("oppi-compact-settings-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let path = root.join("settings.json");
        fs::write(
            &path,
            r#"{"oppi":{"permissionMode":"auto-review","idleCompact":{"idleMinutes":99},"smartCompact":{"thresholdPercent":999}},"kept":true}"#,
        )
        .unwrap();

        let mut settings = load_oppi_compact_settings(&path);
        assert!(settings.idle_enabled);
        assert_eq!(settings.idle_minutes, 5);
        assert_eq!(settings.idle_threshold_percent, 70);
        assert_eq!(settings.smart_threshold_percent, 65);

        settings.idle_enabled = false;
        settings.idle_minutes = 10;
        settings.idle_threshold_percent = 80;
        settings.smart_threshold_percent = 55;
        save_oppi_compact_settings(&path, &settings).unwrap();

        let raw: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(raw["oppi"]["permissionMode"], json!("auto-review"));
        assert_eq!(raw["oppi"]["idleCompact"]["enabled"], json!(false));
        assert_eq!(raw["oppi"]["idleCompact"]["idleMinutes"], json!(10));
        assert_eq!(raw["oppi"]["idleCompact"]["thresholdPercent"], json!(80));
        assert_eq!(raw["oppi"]["smartCompact"]["thresholdPercent"], json!(55));
        assert_eq!(raw["kept"], json!(true));

        let loaded = load_oppi_compact_settings(&path);
        assert!(!loaded.idle_enabled);
        assert_eq!(loaded.idle_minutes, 10);
        assert_eq!(loaded.idle_threshold_percent, 80);
        assert_eq!(loaded.smart_threshold_percent, 55);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn compaction_trigger_telemetry_reports_ready_when_context_thresholds_are_met() {
        let settings = OppiCompactSettings {
            idle_enabled: true,
            idle_minutes: 5,
            idle_threshold_percent: 70,
            smart_threshold_percent: 65,
        };

        let telemetry = compaction_trigger_telemetry(&settings, Some(140_000), Some(200_000), 3);
        let rendered = format_compaction_trigger_telemetry(&telemetry);

        assert!(rendered.contains("context 140k/200k (70%)"));
        assert!(rendered.contains("idle ready"));
        assert!(rendered.contains("smart ready"));
        assert!(rendered.contains("3 active todos"));
        assert!(
            format_oppi_compact_settings_status(&settings, Some(&telemetry))
                .contains("trigger: context 140k/200k (70%)")
        );
        assert_eq!(
            compaction_trigger_telemetry_json(&telemetry)["contextPercent"],
            json!(70)
        );
    }

    #[test]
    fn compaction_offer_planner_prefers_smart_offer_when_todos_are_ready() {
        let settings = OppiCompactSettings {
            idle_enabled: true,
            idle_minutes: 5,
            idle_threshold_percent: 70,
            smart_threshold_percent: 65,
        };
        let telemetry = compaction_trigger_telemetry(&settings, Some(140_000), Some(200_000), 3);

        let offer = compaction_offer_plan(&settings, &telemetry, Some(Duration::from_secs(5 * 60)))
            .expect("smart offer");

        assert_eq!(offer.kind, CompactionOfferKind::Smart);
        assert!(offer.key.contains("smart"));
        assert!(offer.message.contains("smart compact ready"));
        assert!(offer.message.contains("3 active todos"));
        assert!(offer.message.contains("/idle-compact compact"));
    }

    #[test]
    fn compaction_offer_planner_waits_for_idle_minutes_before_idle_offer() {
        let settings = OppiCompactSettings {
            idle_enabled: true,
            idle_minutes: 5,
            idle_threshold_percent: 70,
            smart_threshold_percent: 65,
        };
        let telemetry = compaction_trigger_telemetry(&settings, Some(140_000), Some(200_000), 0);

        assert_eq!(
            compaction_offer_plan(&settings, &telemetry, Some(Duration::from_secs(4 * 60)),),
            None
        );
        let offer = compaction_offer_plan(&settings, &telemetry, Some(Duration::from_secs(5 * 60)))
            .expect("idle offer");

        assert_eq!(offer.kind, CompactionOfferKind::Idle);
        assert!(offer.key.contains("idle"));
        assert!(offer.message.contains("idle compact ready"));
        assert!(offer.message.contains("5m idle"));
        assert!(offer.message.contains("/idle-compact compact"));
    }

    #[test]
    fn footer_settings_persist_pi_footer_config_without_losing_other_settings() {
        let root = env::temp_dir().join(format!("oppi-footer-settings-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let path = root.join("settings.json");
        fs::write(
            &path,
            r#"{"oppi":{"permissionMode":"auto-review","footer":{"showModel":true}},"kept":true}"#,
        )
        .unwrap();

        let mut settings = load_footer_settings(&path);
        assert!(settings.show_model);
        settings.show_path = false;
        settings.show_help_bar = false;
        settings.usage_display = FooterUsageDisplay::Weekly;
        settings.show_model = false;
        settings.show_permission = false;
        settings.show_memory = false;
        settings.show_context = false;
        save_footer_settings(&path, &settings).unwrap();

        let raw: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(raw["oppi"]["permissionMode"], json!("auto-review"));
        assert_eq!(raw["oppi"]["footer"]["showPath"], json!(false));
        assert_eq!(raw["oppi"]["footer"]["showHelpBar"], json!(false));
        assert_eq!(raw["oppi"]["footer"]["usageDisplay"], json!("weekly"));
        assert_eq!(raw["oppi"]["footer"]["showModel"], json!(false));
        assert_eq!(raw["oppi"]["footer"]["showPermission"], json!(false));
        assert_eq!(raw["oppi"]["footer"]["showMemory"], json!(false));
        assert_eq!(raw["oppi"]["footer"]["showContext"], json!(false));
        assert_eq!(raw["kept"], json!(true));

        let loaded = load_footer_settings(&path);
        assert!(!loaded.show_path);
        assert!(!loaded.show_help_bar);
        assert_eq!(loaded.usage_display, FooterUsageDisplay::Weekly);
        assert!(!loaded.show_model);
        assert!(!loaded.show_permission);
        assert!(!loaded.show_memory);
        assert!(!loaded.show_context);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn image_settings_persist_pi_image_config_without_losing_other_settings() {
        let root = env::temp_dir().join(format!("oppi-image-settings-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let path = root.join("settings.json");
        fs::write(
            &path,
            r#"{"oppi":{"permissionMode":"auto-review","images":{"showImages":false,"imageWidthCells":999,"autoResizeImages":false,"blockImages":true}},"kept":true}"#,
        )
        .unwrap();

        let mut settings = load_image_settings(&path);
        assert!(!settings.show_images);
        assert_eq!(settings.image_width_cells, 80);
        assert!(!settings.auto_resize_images);
        assert!(settings.block_images);

        settings.show_images = true;
        settings.image_width_cells = 120;
        settings.auto_resize_images = true;
        settings.block_images = false;
        save_image_settings(&path, &settings).unwrap();

        let raw: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(raw["oppi"]["permissionMode"], json!("auto-review"));
        assert_eq!(raw["oppi"]["images"]["showImages"], json!(true));
        assert_eq!(raw["oppi"]["images"]["imageWidthCells"], json!(120));
        assert_eq!(raw["oppi"]["images"]["autoResizeImages"], json!(true));
        assert_eq!(raw["oppi"]["images"]["blockImages"], json!(false));
        assert_eq!(raw["kept"], json!(true));

        let loaded = load_image_settings(&path);
        assert!(loaded.show_images);
        assert_eq!(loaded.image_width_cells, 120);
        assert!(loaded.auto_resize_images);
        assert!(!loaded.block_images);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn image_settings_preview_protocol_override_drives_capabilities() {
        let root = env::temp_dir().join(format!(
            "oppi-image-settings-protocol-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let path = root.join("settings.json");
        fs::write(
            &path,
            r#"{"oppi":{"images":{"previewProtocol":"sixel","showImages":true,"imageWidthCells":80,"autoResizeImages":true,"blockImages":false}}}"#,
        )
        .unwrap();

        let settings = load_image_settings(&path);
        let capabilities = image_preview_capabilities_from_env_values(
            &settings,
            Some("dumb"),
            Some("Apple_Terminal"),
            None,
            None,
        );

        assert_eq!(capabilities["protocol"], json!("sixel"));
        assert_eq!(
            capabilities["source"],
            json!("oppi.images.previewProtocol=sixel")
        );
        assert_eq!(capabilities["protocolAvailable"], json!(true));
        assert_eq!(capabilities["inlinePreviews"], json!(true));
        assert_eq!(capabilities["sixelSupported"], json!(true));
        assert!(format_image_settings_status(&settings).contains("preview protocol sixel"));
        assert!(
            format_image_preview_capabilities(&capabilities)
                .contains("inline previews enabled via sixel")
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn image_settings_command_route_parses_pi_image_controls() {
        assert_eq!(
            image_settings_command_route("").unwrap(),
            ImageSettingsCommandRoute::Status
        );
        assert_eq!(
            image_settings_command_route("show off").unwrap(),
            ImageSettingsCommandRoute::SetShowImages(false)
        );
        assert_eq!(
            image_settings_command_route("width 120").unwrap(),
            ImageSettingsCommandRoute::SetImageWidthCells(120)
        );
        assert_eq!(
            image_settings_command_route("auto-resize on").unwrap(),
            ImageSettingsCommandRoute::SetAutoResizeImages(true)
        );
        assert_eq!(
            image_settings_command_route("block on").unwrap(),
            ImageSettingsCommandRoute::SetBlockImages(true)
        );
        assert_eq!(
            format!(
                "{:?}",
                image_settings_command_route("capabilities").unwrap()
            ),
            "Capabilities"
        );
        assert_eq!(
            format!(
                "{:?}",
                image_settings_command_route("protocol sixel").unwrap()
            ),
            "SetPreviewProtocol(Sixel)"
        );
        assert_eq!(
            format!(
                "{:?}",
                image_settings_command_route("preview-protocol auto").unwrap()
            ),
            "SetPreviewProtocol(Auto)"
        );
        assert!(
            image_settings_command_route("protocol nope")
                .unwrap_err()
                .contains("auto|kitty|wezterm|iterm2|sixel|off")
        );
        assert!(
            image_settings_command_route("width 90")
                .unwrap_err()
                .contains("60|80|120")
        );
    }

    #[test]
    fn follow_up_mode_persists_pi_root_setting_without_losing_other_settings() {
        let root = env::temp_dir().join(format!("oppi-follow-up-mode-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let path = root.join("settings.json");
        fs::write(
            &path,
            r#"{"followUpMode":"one-at-a-time","oppi":{"permissionMode":"auto-review"},"kept":true}"#,
        )
        .unwrap();

        assert_eq!(load_follow_up_mode_setting(&path), FollowUpMode::OneAtATime);
        save_follow_up_mode_setting(&path, FollowUpMode::All).unwrap();

        let raw: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(raw["followUpMode"], json!("all"));
        assert_eq!(raw["oppi"]["permissionMode"], json!("auto-review"));
        assert_eq!(raw["kept"], json!(true));
        assert_eq!(load_follow_up_mode_setting(&path), FollowUpMode::All);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn steering_mode_persists_pi_root_setting_without_losing_other_settings() {
        let root = env::temp_dir().join(format!("oppi-steering-mode-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let path = root.join("settings.json");
        fs::write(
            &path,
            r#"{"steeringMode":"one-at-a-time","oppi":{"permissionMode":"auto-review"},"kept":true}"#,
        )
        .unwrap();

        assert_eq!(load_steering_mode_setting(&path), SteeringMode::OneAtATime);
        save_steering_mode_setting(&path, SteeringMode::All).unwrap();

        let raw: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(raw["steeringMode"], json!("all"));
        assert_eq!(raw["oppi"]["permissionMode"], json!("auto-review"));
        assert_eq!(raw["kept"], json!(true));
        assert_eq!(load_steering_mode_setting(&path), SteeringMode::All);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn steering_mode_defaults_to_oppi_all_and_reads_legacy_queue_mode() {
        let root =
            env::temp_dir().join(format!("oppi-steering-mode-default-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let path = root.join("settings.json");

        fs::write(&path, r#"{"queueMode":"one-at-a-time"}"#).unwrap();
        assert_eq!(load_steering_mode_setting(&path), SteeringMode::OneAtATime);

        fs::write(&path, r#"{}"#).unwrap();
        assert_eq!(load_steering_mode_setting(&path), SteeringMode::All);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn reasoning_effort_persists_pi_default_thinking_without_losing_legacy_settings() {
        let root = env::temp_dir().join(format!("oppi-default-thinking-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let path = root.join("settings.json");
        fs::write(
            &path,
            r#"{"defaultThinkingLevel":"medium","oppi":{"reasoningEffort":"low","permissionMode":"auto-review"},"kept":true}"#,
        )
        .unwrap();

        assert_eq!(
            load_reasoning_effort_setting(&path),
            Some(ThinkingLevel::Medium)
        );
        save_reasoning_effort_setting(&path, ThinkingLevel::High).unwrap();

        let raw: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(raw["defaultThinkingLevel"], json!("high"));
        assert_eq!(raw["oppi"]["reasoningEffort"], json!("high"));
        assert_eq!(raw["oppi"]["permissionMode"], json!("auto-review"));
        assert_eq!(raw["kept"], json!(true));
        assert_eq!(
            load_reasoning_effort_setting(&path),
            Some(ThinkingLevel::High)
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn tree_filter_mode_persists_pi_root_setting_without_losing_other_settings() {
        let root = env::temp_dir().join(format!("oppi-tree-filter-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let path = root.join("settings.json");
        fs::write(
            &path,
            r#"{"treeFilterMode":"labeled-only","doubleEscapeAction":"fork","oppi":{"permissionMode":"auto-review"},"kept":true}"#,
        )
        .unwrap();

        assert_eq!(
            load_tree_filter_mode_setting(&path),
            TreeFilterMode::LabeledOnly
        );
        save_tree_filter_mode_setting(&path, TreeFilterMode::All).unwrap();

        let raw: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(raw["treeFilterMode"], json!("all"));
        assert_eq!(raw["doubleEscapeAction"], json!("fork"));
        assert_eq!(raw["oppi"]["permissionMode"], json!("auto-review"));
        assert_eq!(raw["kept"], json!(true));
        assert_eq!(load_tree_filter_mode_setting(&path), TreeFilterMode::All);

        fs::write(&path, r#"{"treeFilterMode":"banana"}"#).unwrap();
        assert_eq!(
            load_tree_filter_mode_setting(&path),
            TreeFilterMode::Default
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn saved_default_thinking_overrides_model_default_but_not_explicit_cli_effort() {
        let root = env::temp_dir().join(format!(
            "oppi-default-thinking-apply-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let path = root.join("settings.json");
        fs::write(&path, r#"{"defaultThinkingLevel":"medium"}"#).unwrap();

        let mut provider = ProviderConfig::OpenAiCompatible(with_default_reasoning_effort(
            OpenAiCompatibleConfig {
                flavor: DirectProviderFlavor::OpenAiCodex,
                provider_id: None,
                model: GPT_MAIN_DEFAULT_MODEL.to_string(),
                base_url: None,
                api_key_env: None,
                system_prompt: None,
                temperature: None,
                reasoning_effort: None,
                max_output_tokens: None,
                stream: true,
            },
        ));
        assert_eq!(
            current_effort_level_for_provider(&provider),
            ThinkingLevel::XHigh
        );

        apply_saved_reasoning_effort(&mut provider, &path, false);
        assert_eq!(
            current_effort_level_for_provider(&provider),
            ThinkingLevel::Medium
        );

        set_provider_effort_level(&mut provider, ThinkingLevel::High);
        apply_saved_reasoning_effort(&mut provider, &path, true);
        assert_eq!(
            current_effort_level_for_provider(&provider),
            ThinkingLevel::High
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn quiet_startup_persists_pi_root_setting_without_losing_other_settings() {
        let root = env::temp_dir().join(format!("oppi-quiet-startup-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let path = root.join("settings.json");
        fs::write(
            &path,
            r#"{"quietStartup":true,"oppi":{"permissionMode":"auto-review"},"kept":true}"#,
        )
        .unwrap();

        assert!(load_quiet_startup_setting(&path));
        save_quiet_startup_setting(&path, false).unwrap();

        let raw: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(raw["quietStartup"], json!(false));
        assert_eq!(raw["oppi"]["permissionMode"], json!("auto-review"));
        assert_eq!(raw["kept"], json!(true));
        assert!(!load_quiet_startup_setting(&path));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn startup_command_route_parses_quiet_startup_controls() {
        assert_eq!(
            startup_command_route("").unwrap(),
            StartupCommandRoute::Status
        );
        assert_eq!(
            startup_command_route("status").unwrap(),
            StartupCommandRoute::Status
        );
        assert_eq!(
            startup_command_route("quiet on").unwrap(),
            StartupCommandRoute::SetQuietStartup(true)
        );
        assert_eq!(
            startup_command_route("quiet off").unwrap(),
            StartupCommandRoute::SetQuietStartup(false)
        );
        assert!(
            startup_command_route("quiet maybe")
                .unwrap_err()
                .contains("usage: /startup")
        );
    }

    #[test]
    fn cursor_command_route_parses_hardware_cursor_controls() {
        assert_eq!(
            cursor_command_route("").unwrap(),
            CursorCommandRoute::Status
        );
        assert_eq!(
            cursor_command_route("status").unwrap(),
            CursorCommandRoute::Status
        );
        assert_eq!(
            cursor_command_route("hardware on").unwrap(),
            CursorCommandRoute::SetHardwareCursor(true)
        );
        assert_eq!(
            cursor_command_route("hardware-cursor off").unwrap(),
            CursorCommandRoute::SetHardwareCursor(false)
        );
        assert!(
            cursor_command_route("hardware maybe")
                .unwrap_err()
                .contains("usage: /cursor")
        );
    }

    #[test]
    fn terminal_cursor_setup_sequence_respects_hardware_cursor_setting() {
        assert_eq!(terminal_cursor_setup_sequence(false), "\x1b[?25l");
        assert_eq!(terminal_cursor_setup_sequence(true), "");
    }

    #[test]
    fn editor_command_route_parses_pi_padding_and_double_escape_controls() {
        assert_eq!(
            editor_command_route("").unwrap(),
            EditorCommandRoute::Status
        );
        assert_eq!(
            editor_command_route("status").unwrap(),
            EditorCommandRoute::Status
        );
        assert_eq!(
            editor_command_route("padding 2").unwrap(),
            EditorCommandRoute::SetPaddingX(2)
        );
        assert_eq!(
            editor_command_route("padding-x 3").unwrap(),
            EditorCommandRoute::SetPaddingX(3)
        );
        assert_eq!(
            editor_command_route("double-escape fork").unwrap(),
            EditorCommandRoute::SetDoubleEscapeAction(DoubleEscapeAction::Fork)
        );
        assert_eq!(
            editor_command_route("escape none").unwrap(),
            EditorCommandRoute::SetDoubleEscapeAction(DoubleEscapeAction::None)
        );
        assert!(
            editor_command_route("padding 4")
                .unwrap_err()
                .contains("usage: /editor")
        );
        assert!(
            editor_command_route("double-escape banana")
                .unwrap_err()
                .contains("usage: /editor")
        );
    }

    #[test]
    fn tree_command_route_parses_pi_filter_controls() {
        assert_eq!(tree_command_route(&[]).unwrap(), TreeCommandRoute::Show);
        assert_eq!(
            tree_command_route(&["status"]).unwrap(),
            TreeCommandRoute::Status
        );
        assert_eq!(
            tree_command_route(&["entries"]).unwrap(),
            TreeCommandRoute::Entries {
                after: 0,
                limit: None
            }
        );
        assert_eq!(
            tree_command_route(&["messages"]).unwrap(),
            TreeCommandRoute::Entries {
                after: 0,
                limit: None
            }
        );
        assert_eq!(
            tree_command_route(&["entries", "after", "42"]).unwrap(),
            TreeCommandRoute::Entries {
                after: 42,
                limit: None
            }
        );
        assert_eq!(
            tree_command_route(&["messages", "--after", "99"]).unwrap(),
            TreeCommandRoute::Entries {
                after: 99,
                limit: None
            }
        );
        assert_eq!(
            tree_command_route(&["entries", "limit", "25"]).unwrap(),
            TreeCommandRoute::Entries {
                after: 0,
                limit: Some(25)
            }
        );
        assert_eq!(
            tree_command_route(&["messages", "--after", "42", "--limit", "10"]).unwrap(),
            TreeCommandRoute::Entries {
                after: 42,
                limit: Some(10)
            }
        );
        assert_eq!(
            tree_command_route(&["items", "--limit", "10", "--after", "42"]).unwrap(),
            TreeCommandRoute::Entries {
                after: 42,
                limit: Some(10)
            }
        );
        assert_eq!(
            tree_command_route(&["fork", "42"]).unwrap(),
            TreeCommandRoute::Fork {
                event_id: 42,
                title: None
            }
        );
        assert_eq!(
            tree_command_route(&["branch", "42", "rewrite", "from", "here"]).unwrap(),
            TreeCommandRoute::Fork {
                event_id: 42,
                title: Some("rewrite from here".to_string())
            }
        );
        assert_eq!(
            tree_command_route(&["edit", "42", "edited", "request"]).unwrap(),
            TreeCommandRoute::Edit {
                event_id: 42,
                input: "edited request".to_string()
            }
        );
        assert_eq!(
            tree_command_route(&["resubmit", "42", "edited", "request"]).unwrap(),
            TreeCommandRoute::Edit {
                event_id: 42,
                input: "edited request".to_string()
            }
        );
        assert!(
            tree_command_route(&["items", "--limit", "0"])
                .unwrap_err()
                .contains("usage: /tree")
        );
        assert_eq!(
            tree_command_route(&["filter", "labeled-only"]).unwrap(),
            TreeCommandRoute::SetFilter(TreeFilterMode::LabeledOnly)
        );
        assert_eq!(
            tree_command_route(&["tree-filter", "no-tools"]).unwrap(),
            TreeCommandRoute::SetFilter(TreeFilterMode::NoTools)
        );
        assert_eq!(
            tree_command_route(&["label", "42", "checkpoint"]).unwrap(),
            TreeCommandRoute::Label {
                event_id: 42,
                labels: vec!["checkpoint".to_string()]
            }
        );
        assert_eq!(
            tree_command_route(&["unlabel", "42"]).unwrap(),
            TreeCommandRoute::Label {
                event_id: 42,
                labels: Vec::new()
            }
        );
        assert!(
            tree_command_route(&["filter", "banana"])
                .unwrap_err()
                .contains("usage: /tree")
        );
        assert!(
            tree_command_route(&["entries", "after", "banana"])
                .unwrap_err()
                .contains("usage: /tree")
        );
    }

    #[test]
    fn terminal_command_route_parses_pi_progress_and_clear_on_shrink_controls() {
        assert_eq!(
            terminal_command_route("").unwrap(),
            TerminalCommandRoute::Status
        );
        assert_eq!(
            terminal_command_route("status").unwrap(),
            TerminalCommandRoute::Status
        );
        assert_eq!(
            terminal_command_route("progress on").unwrap(),
            TerminalCommandRoute::SetProgress(true)
        );
        assert_eq!(
            terminal_command_route("terminal-progress off").unwrap(),
            TerminalCommandRoute::SetProgress(false)
        );
        assert_eq!(
            terminal_command_route("clear-on-shrink on").unwrap(),
            TerminalCommandRoute::SetClearOnShrink(true)
        );
        assert_eq!(
            terminal_command_route("shrink off").unwrap(),
            TerminalCommandRoute::SetClearOnShrink(false)
        );
        assert!(
            terminal_command_route("progress maybe")
                .unwrap_err()
                .contains("usage: /terminal")
        );
        assert!(
            terminal_command_route("clear-on-shrink maybe")
                .unwrap_err()
                .contains("usage: /terminal")
        );
    }

    #[test]
    fn terminal_progress_sequences_match_pi_osc94_contract() {
        assert_eq!(terminal_progress_sequence(true), "\x1b]9;4;3\x07");
        assert_eq!(terminal_progress_sequence(false), "\x1b]9;4;0;\x07");
        assert_eq!(
            terminal_progress_output(true, true, false),
            Some("\x1b]9;4;3\x07")
        );
        assert_eq!(
            terminal_progress_output(true, false, false),
            Some("\x1b]9;4;0;\x07")
        );
        assert_eq!(terminal_progress_output(false, true, false), None);
        assert_eq!(terminal_progress_output(true, true, true), None);
    }

    #[test]
    fn autocomplete_command_route_parses_pi_visible_item_controls() {
        assert_eq!(
            autocomplete_command_route("").unwrap(),
            AutocompleteCommandRoute::Status
        );
        assert_eq!(
            autocomplete_command_route("status").unwrap(),
            AutocompleteCommandRoute::Status
        );
        assert_eq!(
            autocomplete_command_route("items 10").unwrap(),
            AutocompleteCommandRoute::SetMaxVisible(10)
        );
        assert_eq!(
            autocomplete_command_route("max-visible 15").unwrap(),
            AutocompleteCommandRoute::SetMaxVisible(15)
        );
        assert_eq!(
            autocomplete_command_route("visible 3").unwrap(),
            AutocompleteCommandRoute::SetMaxVisible(3)
        );
        assert!(
            autocomplete_command_route("items 21")
                .unwrap_err()
                .contains("usage: /autocomplete")
        );
    }

    #[test]
    fn skills_command_route_parses_command_toggle_controls() {
        assert_eq!(skills_command_route("").unwrap(), SkillsCommandRoute::List);
        assert_eq!(
            skills_command_route("status").unwrap(),
            SkillsCommandRoute::Status
        );
        assert_eq!(
            skills_command_route("commands").unwrap(),
            SkillsCommandRoute::Status
        );
        assert_eq!(
            skills_command_route("commands on").unwrap(),
            SkillsCommandRoute::SetCommands(true)
        );
        assert_eq!(
            skills_command_route("commands off").unwrap(),
            SkillsCommandRoute::SetCommands(false)
        );
        assert!(
            skills_command_route("commands maybe")
                .unwrap_err()
                .contains("usage: /skills")
        );
    }

    #[test]
    fn startup_banner_respects_json_and_quiet_startup() {
        assert!(should_print_startup_banner(false, false));
        assert!(!should_print_startup_banner(true, false));
        assert!(!should_print_startup_banner(false, true));
    }

    #[test]
    fn follow_up_command_route_parses_pi_delivery_mode() {
        assert_eq!(
            follow_up_command_route("").unwrap(),
            FollowUpCommandRoute::Status
        );
        assert_eq!(
            follow_up_command_route("status").unwrap(),
            FollowUpCommandRoute::Status
        );
        assert_eq!(
            follow_up_command_route("mode all").unwrap(),
            FollowUpCommandRoute::SetMode(FollowUpMode::All)
        );
        assert_eq!(
            follow_up_command_route("mode one").unwrap(),
            FollowUpCommandRoute::SetMode(FollowUpMode::OneAtATime)
        );
        assert!(
            follow_up_command_route("mode later")
                .unwrap_err()
                .contains("usage: /follow-up")
        );
    }

    #[test]
    fn follow_up_batches_match_pi_all_vs_one_at_a_time_modes() {
        let mut queue = VecDeque::from([
            "first follow-up".to_string(),
            "second follow-up".to_string(),
        ]);
        let all_batch = take_follow_up_batch(&mut queue, FollowUpMode::All);
        assert_eq!(
            all_batch,
            vec![
                "first follow-up".to_string(),
                "second follow-up".to_string()
            ]
        );
        assert!(queue.is_empty());
        let all_prompt = format_follow_up_batch_prompt(&all_batch);
        assert!(all_prompt.contains("Queued follow-ups"));
        assert!(all_prompt.contains("1. first follow-up"));
        assert!(all_prompt.contains("2. second follow-up"));

        let mut queue = VecDeque::from([
            "first follow-up".to_string(),
            "second follow-up".to_string(),
        ]);
        let one_batch = take_follow_up_batch(&mut queue, FollowUpMode::OneAtATime);
        assert_eq!(one_batch, vec!["first follow-up".to_string()]);
        assert_eq!(queue, VecDeque::from(["second follow-up".to_string()]));
        assert_eq!(format_follow_up_batch_prompt(&one_batch), "first follow-up");
    }

    #[test]
    fn footer_visibility_settings_shape_status_line_and_usage_summary() {
        let settings = FooterSettings {
            show_path: false,
            show_help_bar: false,
            usage_display: FooterUsageDisplay::Off,
            show_model: false,
            show_permission: false,
            show_memory: false,
            show_context: false,
        };
        let rendered = format_footer_status_line(FooterStatusParts {
            settings: &settings,
            cwd: "E:/repo/oppi",
            model: "gpt-5.5",
            permission: "auto-review",
            usage: "usage session 12k",
            weekly: Some("week 80% left"),
            context: "ctx 8k",
            memory: "hoppi:4",
            todos: 2,
            queued: 1,
            goal: "goal active",
            variant: "off",
            theme: "oppi",
            thread: "thread-1",
        });

        assert!(rendered.starts_with("status: role=executor"));
        assert!(!rendered.contains("model="));
        assert!(!rendered.contains("permissions="));
        assert!(!rendered.contains("usage "));
        assert!(!rendered.contains("week "));
        assert!(!rendered.contains("ctx "));
        assert!(!rendered.contains("memory="));
        assert!(!rendered.contains("workspace="));
        assert!(rendered.contains("todos=2"));
        assert!(rendered.contains("queued=1"));
        assert!(rendered.contains("thread=thread-1"));

        let usage_footer = format_usage_footer_line(
            &settings,
            "usage session 12k",
            Some("week 80% left"),
            "ctx 8k",
        );
        assert_eq!(usage_footer, "off");
    }

    #[test]
    fn permissions_sandbox_settings_persist_flags_and_patterns_without_losing_review_settings() {
        let root = env::temp_dir().join(format!(
            "oppi-permission-sandbox-settings-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let path = root.join("settings.json");
        fs::write(
            &path,
            r#"{"oppi":{"permissionMode":"auto-review","permissions":{"reviewTimeoutMs":30000,"reviewerModel":"openai-codex/gpt-5.5"}},"kept":true}"#,
        )
        .unwrap();

        let mut sandbox = load_permission_sandbox_settings(&path);
        sandbox.allow_internet = true;
        sandbox.allow_ssh = true;
        sandbox.project_roots = vec!["E:/repo".to_string()];
        sandbox.readable_roots = vec!["E:/repo".to_string(), "E:/readonly".to_string()];
        sandbox.writable_roots = vec!["E:/repo/out".to_string()];
        sandbox.protected_patterns = vec!["secrets/*.json".to_string(), ".env.local".to_string()];
        save_permission_sandbox_settings(&path, &sandbox).unwrap();

        let raw: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(raw["oppi"]["permissionMode"], json!("auto-review"));
        assert_eq!(raw["oppi"]["permissions"]["reviewTimeoutMs"], json!(30_000));
        assert_eq!(
            raw["oppi"]["permissions"]["reviewerModel"],
            json!("openai-codex/gpt-5.5")
        );
        assert_eq!(
            raw["oppi"]["permissions"]["sandbox"]["protectedPatterns"],
            json!(["secrets/*.json", ".env.local"])
        );
        assert_eq!(
            raw["oppi"]["permissions"]["sandbox"]["allowInternet"],
            json!(true)
        );
        assert_eq!(
            raw["oppi"]["permissions"]["sandbox"]["allowSsh"],
            json!(true)
        );
        assert_eq!(
            raw["oppi"]["permissions"]["sandbox"]["projectRoots"],
            json!(["E:/repo"])
        );
        assert_eq!(
            raw["oppi"]["permissions"]["sandbox"]["readableRoots"],
            json!(["E:/repo", "E:/readonly"])
        );
        assert_eq!(
            raw["oppi"]["permissions"]["sandbox"]["writableRoots"],
            json!(["E:/repo/out"])
        );
        assert_eq!(raw["kept"], json!(true));

        let loaded = load_permission_sandbox_settings(&path);
        assert!(loaded.allow_internet);
        assert!(loaded.allow_ssh);
        assert_eq!(loaded.project_roots, vec!["E:/repo".to_string()]);
        assert_eq!(
            loaded.readable_roots,
            vec!["E:/repo".to_string(), "E:/readonly".to_string()]
        );
        assert_eq!(loaded.writable_roots, vec!["E:/repo/out".to_string()]);
        assert_eq!(
            loaded.protected_patterns,
            vec!["secrets/*.json".to_string(), ".env.local".to_string()]
        );

        let mut cleared = loaded;
        cleared.allow_internet = false;
        cleared.allow_ssh = false;
        cleared.project_roots.clear();
        cleared.readable_roots.clear();
        cleared.writable_roots.clear();
        cleared.protected_patterns.clear();
        save_permission_sandbox_settings(&path, &cleared).unwrap();
        let raw: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            raw["oppi"]["permissions"]["sandbox"]["protectedPatterns"],
            json!([])
        );
        assert_eq!(
            raw["oppi"]["permissions"]["sandbox"]["allowInternet"],
            json!(false)
        );
        assert_eq!(
            raw["oppi"]["permissions"]["sandbox"]["allowSsh"],
            json!(false)
        );
        assert_eq!(
            raw["oppi"]["permissions"]["sandbox"]["projectRoots"],
            json!([])
        );
        assert_eq!(
            raw["oppi"]["permissions"]["sandbox"]["readableRoots"],
            json!([])
        );
        assert_eq!(
            raw["oppi"]["permissions"]["sandbox"]["writableRoots"],
            json!([])
        );
        assert_eq!(raw["oppi"]["permissionMode"], json!("auto-review"));
        assert_eq!(raw["kept"], json!(true));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn ask_user_command_parses_timeout_choices() {
        assert_eq!(
            ask_user_command_route("").unwrap(),
            AskUserCommandRoute::Status
        );
        assert_eq!(
            ask_user_command_route("status").unwrap(),
            AskUserCommandRoute::Status
        );
        assert_eq!(
            ask_user_command_route("timeout off").unwrap(),
            AskUserCommandRoute::SetTimeoutMs(0)
        );
        assert_eq!(
            ask_user_command_route("timeout 2").unwrap(),
            AskUserCommandRoute::SetTimeoutMs(120_000)
        );
        assert!(
            ask_user_command_route("timeout 3")
                .unwrap_err()
                .contains("off|1|2|5|10")
        );
    }

    #[test]
    fn ask_user_settings_persist_pi_timeout_minutes_without_losing_other_settings() {
        let root = env::temp_dir().join(format!("oppi-ask-user-settings-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let path = root.join("settings.json");
        fs::write(
            &path,
            r#"{"oppi":{"permissionMode":"auto-review"},"kept":true}"#,
        )
        .unwrap();

        save_ask_user_settings(
            &path,
            &oppi_protocol::AskUserSettings {
                timeout_ms: 120_000,
            },
        )
        .unwrap();

        let raw: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(raw["oppi"]["permissionMode"], json!("auto-review"));
        assert_eq!(raw["oppi"]["askUser"]["timeoutMinutes"], json!(2));
        assert_eq!(raw["kept"], json!(true));

        let loaded = load_ask_user_settings(&path);
        assert_eq!(loaded.timeout_ms, 120_000);

        save_ask_user_settings(&path, &oppi_protocol::AskUserSettings { timeout_ms: 0 }).unwrap();
        let raw: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(raw["oppi"]["askUser"]["timeoutMinutes"], json!(0));
        assert_eq!(raw["oppi"]["permissionMode"], json!("auto-review"));
        assert_eq!(raw["kept"], json!(true));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn ask_user_timeout_answers_use_recommended_defaults() {
        let request = serde_json::from_value::<AskUserRequest>(json!({
            "id": "question-1",
            "questions": [
                {
                    "id": "scope",
                    "question": "Which scope?",
                    "options": [
                        { "id": "all", "label": "Whole repo" },
                        { "id": "changed", "label": "Changed package" }
                    ],
                    "defaultOptionId": "changed"
                },
                {
                    "id": "notes",
                    "question": "Any notes?",
                    "allowCustom": true
                },
                {
                    "id": "optional",
                    "question": "Optional detail?",
                    "required": false
                }
            ],
            "timeoutMs": 120000
        }))
        .unwrap();

        let answers = ask_user_timeout_answers(&request);
        assert_eq!(answers[0].question_id, "scope");
        assert_eq!(answers[0].option_id.as_deref(), Some("changed"));
        assert_eq!(answers[0].label.as_deref(), Some("Changed package"));
        assert_eq!(answers[1].question_id, "notes");
        assert_eq!(
            answers[1].text.as_deref(),
            Some("No recommended answer was available before timeout.")
        );
        assert!(answers[2].skipped);
    }

    #[test]
    fn todo_dock_renders_only_active_todos() {
        let state = serde_json::from_value::<TodoState>(json!({
            "summary": "working",
            "todos": [
                { "id": "a", "content": "Active", "status": "in_progress" },
                { "id": "b", "content": "Done", "status": "completed" }
            ]
        }))
        .unwrap();
        let rendered = format_todos(&state);
        assert!(rendered.contains("a:InProgress"));
        assert!(!rendered.contains("b:"));
        assert!(rendered.contains("summary: working"));
    }

    #[test]
    fn parses_todos_client_actions() {
        assert_eq!(parse_todos_command_args(&[]).unwrap(), TodoCommand::List);
        assert_eq!(
            parse_todos_command_args(&["clear"]).unwrap(),
            TodoCommand::ClientAction {
                action: TodoClientAction::Clear,
                id: None,
            }
        );
        assert_eq!(
            parse_todos_command_args(&["done"]).unwrap(),
            TodoCommand::ClientAction {
                action: TodoClientAction::Done,
                id: None,
            }
        );
        assert_eq!(
            parse_todos_command_args(&["done", "impl"]).unwrap(),
            TodoCommand::ClientAction {
                action: TodoClientAction::Done,
                id: Some("impl".to_string()),
            }
        );
        assert_eq!(
            parse_todos_command_args(&["complete", "verify"]).unwrap(),
            TodoCommand::ClientAction {
                action: TodoClientAction::Done,
                id: Some("verify".to_string()),
            }
        );
        assert!(parse_todos_command_args(&["wat"]).is_err());
    }

    #[test]
    fn agent_markdown_round_trips_native_definition() {
        let markdown = r#"---
name: repo-helper
description: "Helps with repo maintenance"
tools: read_file, write_file
disallowedTools: shell_exec
model: gpt-5.3-codex
effort: high
permissionMode: auto-review
memory: client-hosted
background: true
worktreeRoot: .worktrees/repo-helper
isolation: worktree
color: cyan
skills: graphify, mermaid-diagrams
maxTurns: 6
---

You are a careful repo helper.
"#;
        let agent = parse_agent_markdown(markdown, AgentSource::Project).unwrap();
        assert_eq!(agent.name, "repo-helper");
        assert_eq!(agent.description, "Helps with repo maintenance");
        assert_eq!(agent.tools, vec!["read_file", "write_file"]);
        assert_eq!(agent.tool_denylist, vec!["shell_exec"]);
        assert_eq!(agent.model.as_deref(), Some("gpt-5.3-codex"));
        assert_eq!(agent.effort.as_deref(), Some("high"));
        assert_eq!(agent.permission_mode, Some(PermissionMode::AutoReview));
        assert_eq!(agent.memory_mode.as_deref(), Some("client-hosted"));
        assert!(agent.background);
        assert_eq!(
            agent.worktree_root.as_deref(),
            Some(".worktrees/repo-helper")
        );
        assert_eq!(agent.isolation.as_deref(), Some("worktree"));
        assert_eq!(agent.color.as_deref(), Some("cyan"));
        assert_eq!(agent.skills, vec!["graphify", "mermaid-diagrams"]);
        assert_eq!(agent.max_turns, Some(6));
        assert_eq!(agent.instructions, "You are a careful repo helper.");

        let exported = format_agent_markdown(&agent);
        assert!(exported.contains("name: repo-helper"));
        assert!(exported.contains("description: \"Helps with repo maintenance\""));
        assert!(exported.contains("tools: read_file, write_file"));
        assert!(exported.contains("disallowedTools: shell_exec"));
        assert!(exported.contains("permissionMode: auto-review"));
        assert!(exported.contains("memory: client-hosted"));
        assert!(exported.contains("worktreeRoot: .worktrees/repo-helper"));
        assert!(exported.contains("isolation: worktree"));
        assert!(exported.contains("color: cyan"));
        assert!(exported.contains("skills: graphify, mermaid-diagrams"));
        assert!(exported.contains("maxTurns: 6"));
        assert!(exported.contains("You are a careful repo helper."));
    }

    #[test]
    fn agent_markdown_targets_project_and_user_locations() {
        let cwd = Path::new("C:/repo");
        let agent_dir = Path::new("C:/oppi-agent");
        assert_eq!(
            default_agent_markdown_path(
                cwd,
                agent_dir,
                AgentMarkdownTarget::Project,
                "Repo Helper"
            )
            .unwrap(),
            PathBuf::from("C:/repo/.oppi/agents/repo-helper.md")
        );
        assert_eq!(
            default_agent_markdown_path(cwd, agent_dir, AgentMarkdownTarget::User, "Repo Helper")
                .unwrap(),
            PathBuf::from("C:/oppi-agent/oppi/agents/repo-helper.md")
        );
        assert!(
            default_agent_markdown_path(cwd, agent_dir, AgentMarkdownTarget::Project, "!!!")
                .is_err()
        );
    }

    #[test]
    fn parses_agent_markdown_import_export_routes() {
        let cwd = Path::new("C:/repo");
        let agent_dir = Path::new("C:/oppi-agent");
        let imported = parse_agent_import_route(&["user", "agents/repo-helper.md"], cwd).unwrap();
        assert_eq!(imported.source, AgentSource::User);
        assert_eq!(
            imported.path,
            PathBuf::from("C:/repo/agents/repo-helper.md")
        );

        let exported = parse_agent_export_route(&["repo-helper", "user"], cwd, agent_dir).unwrap();
        assert_eq!(exported.agent_name, "repo-helper");
        assert_eq!(
            exported.path,
            PathBuf::from("C:/oppi-agent/oppi/agents/repo-helper.md")
        );

        let explicit =
            parse_agent_export_route(&["repo-helper", "exports/helper.md"], cwd, agent_dir)
                .unwrap();
        assert_eq!(explicit.path, PathBuf::from("C:/repo/exports/helper.md"));
    }

    #[test]
    fn parses_agents_create_show_edit_delete_routes() {
        assert_eq!(
            parse_agents_route("show reviewer").unwrap(),
            AgentsRoute::Show {
                name: "reviewer".to_string()
            }
        );
        assert_eq!(
            parse_agents_route("create reviewer Reviews code").unwrap(),
            AgentsRoute::Create {
                source: AgentSource::Project,
                name: "reviewer".to_string(),
                description: "Reviews code".to_string()
            }
        );
        assert_eq!(
            parse_agents_route("create user reviewer Reviews code").unwrap(),
            AgentsRoute::Create {
                source: AgentSource::User,
                name: "reviewer".to_string(),
                description: "Reviews code".to_string()
            }
        );
        assert_eq!(
            parse_agents_route("edit reviewer").unwrap(),
            AgentsRoute::Edit {
                name: "reviewer".to_string()
            }
        );
        assert_eq!(
            parse_agents_route("delete reviewer --yes").unwrap(),
            AgentsRoute::Delete {
                name: "reviewer".to_string(),
                confirmed: true
            }
        );
        assert_eq!(
            parse_agents_route("delete reviewer").unwrap(),
            AgentsRoute::Delete {
                name: "reviewer".to_string(),
                confirmed: false
            }
        );
        assert_eq!(
            parse_agents_route("poll agent-run-1").unwrap(),
            AgentsRoute::Poll {
                run_id: "agent-run-1".to_string()
            }
        );
        assert_eq!(
            parse_agents_route("status agent-run-1").unwrap(),
            AgentsRoute::Poll {
                run_id: "agent-run-1".to_string()
            }
        );
    }

    #[test]
    fn finds_active_agent_from_list_for_markdown_export() {
        let value = json!({
            "items": [
                {
                    "active": {
                        "name": "repo-helper",
                        "description": "Helps with repo maintenance",
                        "source": "project",
                        "instructions": "You are careful."
                    },
                    "shadowed": []
                }
            ]
        });
        let agent = active_agent_from_list(value, "repo-helper").unwrap();
        assert_eq!(agent.name, "repo-helper");
        assert_eq!(agent.source, Some(AgentSource::Project));

        let missing = active_agent_from_list(json!({ "items": [] }), "missing").unwrap_err();
        assert!(missing.contains("unknown agent missing"));
    }

    #[test]
    fn formats_agent_list_with_shadowed_read_only_and_failed_files() {
        let rendered = format_agents_list(&json!({
            "items": [
                {
                    "active": {
                        "name": "reviewer",
                        "description": "Project reviewer",
                        "source": "project",
                        "instructions": "Review carefully."
                    },
                    "shadowed": [
                        {
                            "name": "reviewer",
                            "description": "Claude reviewer",
                            "source": "claudeProject",
                            "readOnly": true,
                            "instructions": "Read only."
                        }
                    ]
                },
                {
                    "active": {
                        "name": "verification",
                        "description": "Verify work",
                        "source": "builtIn",
                        "readOnly": true,
                        "instructions": "Verify."
                    },
                    "shadowed": []
                }
            ],
            "failed": [
                {
                    "path": "C:/repo/.oppi/agents/bad.md",
                    "source": "project",
                    "message": "agent markdown requires description"
                }
            ]
        }))
        .unwrap();

        assert!(rendered.contains("- reviewer [project]: Project reviewer"));
        assert!(rendered.contains("shadows 1 lower-priority definition(s)"));
        assert!(rendered.contains("- verification [built-in] read-only: Verify work"));
        assert!(
            rendered
                .contains("failed agent files:\n- project C:/repo/.oppi/agents/bad.md: agent markdown requires description")
        );
    }

    #[test]
    fn formats_agent_show_with_metadata_shadowed_and_instructions() {
        let resolved: ResolvedAgent = serde_json::from_value(json!({
            "active": {
                "name": "reviewer",
                "description": "Project reviewer",
                "source": "project",
                "tools": ["read_file", "shell_exec"],
                "toolDenylist": ["shell_exec"],
                "path": "C:/repo/.oppi/agents/reviewer.md",
                "instructions": "Inspect the diff."
            },
            "shadowed": [
                {
                    "name": "reviewer",
                    "description": "Claude reviewer",
                    "source": "claudeProject",
                    "readOnly": true,
                    "path": "C:/repo/.claude/agents/reviewer.md",
                    "instructions": "Read only."
                }
            ]
        }))
        .unwrap();
        let rendered = format_agent_show(&resolved);

        assert!(rendered.contains("reviewer [project]"));
        assert!(rendered.contains("Description: Project reviewer"));
        assert!(rendered.contains("Tools: read_file, shell_exec"));
        assert!(rendered.contains("Disallowed tools: shell_exec"));
        assert!(rendered.contains("Path: C:/repo/.oppi/agents/reviewer.md"));
        assert!(rendered.contains("Inspect the diff."));
        assert!(rendered.contains("Shadowed:\n- reviewer [claude-project] read-only"));
    }

    #[test]
    fn renderer_handles_markdown_background_and_artifacts() {
        let rendered = render_markdown("# Title\n- one\n```diff\n+ add\n- del\n```", "plain");
        assert!(rendered.contains("# Title"));
        assert!(rendered.contains("• one"));
        assert!(rendered.contains("+ add"));

        let background = format_background_read(&json!({
            "task": { "id": "task-1", "status": "completed", "outputBytes": 4, "outputPath": "out.log" },
            "output": "done",
            "outputBytes": 4,
            "maxBytes": 30000
        }));
        assert!(background.contains("task-1"));
        assert!(background.contains("done"));
        assert!(background.contains("out.log"));

        let now = current_time_ms().unwrap_or(2_000);
        let background_list = json!({
            "items": [
                { "id": "old", "status": "completed", "command": "echo old", "cwd": "/repo", "outputPath": "old.log", "startedAtMs": now.saturating_sub(5_000), "finishedAtMs": now.saturating_sub(3_000), "exitCode": 0, "outputBytes": 3 },
                { "id": "new", "status": "running", "command": "echo new", "cwd": "/repo", "outputPath": "new.log", "startedAtMs": now.saturating_sub(1_000), "outputBytes": 7 }
            ]
        });
        let rendered_list = format_background_list(&background_list);
        assert!(rendered_list.contains("1 running"));
        assert!(rendered_list.contains("1 completed"));
        assert!(rendered_list.contains("actions: /background read"));
        assert_eq!(
            select_background_task_id(&background_list, false).as_deref(),
            Some("new")
        );
        assert_eq!(
            select_background_task_id(&background_list, true).as_deref(),
            Some("new")
        );
        assert!(background_summary_from_list(&background_list).contains("latest=new"));

        assert_eq!(
            artifact_hint(r#"{"outputPath":"output/image.png"}"#).as_deref(),
            Some("output/image.png")
        );

        let artifact_line = format_artifact_created(
            &ArtifactMetadata {
                id: "artifact-image-1".to_string(),
                tool_call_id: "image-1".to_string(),
                output_path: "output/image.png".to_string(),
                mime_type: Some("image/png".to_string()),
                width: Some(64),
                height: Some(32),
                source_images: vec!["input.png".to_string()],
                mask: Some("mask.png".to_string()),
                backend: Some("openai-images".to_string()),
                model: Some("gpt-image-2".to_string()),
                bytes: Some(1024),
                diagnostics: Vec::new(),
            },
            theme_tokens("plain"),
        );
        assert!(artifact_line.contains("output/image.png"));
        assert!(artifact_line.contains("64x32"));
        assert!(artifact_line.contains("backend=openai-images"));
        assert!(artifact_line.contains("sources: input.png"));
        assert!(artifact_line.contains("mask.png"));
    }

    #[test]
    fn renderer_hides_internal_phase_and_diagnostics() {
        let phase_event = Event {
            id: 1,
            thread_id: "thread-1".to_string(),
            turn_id: Some("turn-1".to_string()),
            created_at_ms: 1,
            kind: EventKind::TurnPhaseChanged {
                phase: oppi_protocol::TurnPhase::Tools,
            },
        };
        assert_eq!(render_event_line(&phase_event), None);

        let diagnostic_event = Event {
            id: 2,
            thread_id: "thread-1".to_string(),
            turn_id: Some("turn-1".to_string()),
            created_at_ms: 2,
            kind: EventKind::Diagnostic {
                diagnostic: Diagnostic {
                    level: oppi_protocol::DiagnosticLevel::Info,
                    message: "agentic turn reached final response".to_string(),
                    metadata: BTreeMap::new(),
                },
            },
        };
        assert_eq!(render_event_line(&diagnostic_event), None);

        for message in [
            "sandbox policy attached to turn",
            "direct provider request completed",
        ] {
            let internal_diagnostic_event = Event {
                id: 6,
                thread_id: "thread-1".to_string(),
                turn_id: Some("turn-1".to_string()),
                created_at_ms: 6,
                kind: EventKind::Diagnostic {
                    diagnostic: Diagnostic {
                        level: oppi_protocol::DiagnosticLevel::Info,
                        message: message.to_string(),
                        metadata: BTreeMap::new(),
                    },
                },
            };
            assert_eq!(render_event_line(&internal_diagnostic_event), None);
        }

        let delta_event = Event {
            id: 3,
            thread_id: "thread-1".to_string(),
            turn_id: Some("turn-1".to_string()),
            created_at_ms: 3,
            kind: EventKind::ItemDelta {
                item_id: "item-1".to_string(),
                delta: "partial token".to_string(),
            },
        };
        assert_eq!(render_event_line(&delta_event), None);

        let completion_event = Event {
            id: 4,
            thread_id: "thread-1".to_string(),
            turn_id: Some("turn-1".to_string()),
            created_at_ms: 4,
            kind: EventKind::TurnCompleted {
                turn_id: "turn-1".to_string(),
            },
        };
        assert_eq!(render_event_line(&completion_event), None);

        let final_output_event = Event {
            id: 5,
            thread_id: "thread-1".to_string(),
            turn_id: Some("turn-1".to_string()),
            created_at_ms: 5,
            kind: EventKind::ItemCompleted {
                item: oppi_protocol::Item {
                    id: "item-2".to_string(),
                    thread_id: "thread-1".to_string(),
                    turn_id: "turn-1".to_string(),
                    kind: ItemKind::AssistantMessage {
                        text: "Final answer.".to_string(),
                    },
                },
            },
        };
        assert!(
            render_event_line(&final_output_event)
                .unwrap()
                .contains("Final answer.")
        );
    }

    fn test_guardian_review_record(
        status: oppi_protocol::GuardianReviewStatus,
    ) -> oppi_protocol::GuardianReviewRecord {
        oppi_protocol::GuardianReviewRecord {
            id: "review-1".to_string(),
            timestamp: "2026-05-26T12:00:00Z".to_string(),
            status,
            mode: PermissionMode::AutoReview,
            tool_name: "write_file".to_string(),
            tool_call_id: "write-env".to_string(),
            signature: "oppi::write_file\n{}".to_string(),
            circuit_key: "oppi::write_file:workspace-write:.env".to_string(),
            summary: "blocked protected path C:\\repo\\.env".to_string(),
            risk: oppi_protocol::GuardianRiskSummary {
                category: "workspace-write".to_string(),
                reason: "blocked protected path C:\\repo\\.env".to_string(),
                protected_hits: vec!["C:\\repo\\.env".to_string()],
                paths: vec!["C:\\repo\\.env".to_string()],
                hints: Vec::new(),
            },
            decision: Some(oppi_protocol::GuardianReviewDecision {
                outcome: oppi_protocol::GuardianReviewOutcome::Deny,
                risk_level: oppi_protocol::RiskLevel::High,
                user_authorization: oppi_protocol::GuardianAuthorizationLevel::Unknown,
                rationale: "blocked protected path C:\\repo\\.env".to_string(),
                cache_scope: Some(oppi_protocol::GuardianCacheScope::None),
            }),
            reviewer_model: Some("mock-reviewer".to_string()),
            cached_from: None,
            strict_json: None,
        }
    }

    #[test]
    fn permission_review_events_render_visible_transcript_without_path_leak() {
        let started = Event {
            id: 10,
            thread_id: "thread-1".to_string(),
            turn_id: Some("turn-1".to_string()),
            created_at_ms: 10,
            kind: EventKind::PermissionReviewStarted {
                record: test_guardian_review_record(oppi_protocol::GuardianReviewStatus::Reviewing),
            },
        };
        let started_line = render_event_line(&started).expect("started review line");
        assert!(started_line.contains("guardian review reviewing"));
        assert!(started_line.contains("write_file"));
        assert!(!started_line.contains(".env"));
        assert!(!started_line.contains("C:\\repo"));

        let completed = Event {
            id: 11,
            thread_id: "thread-1".to_string(),
            turn_id: Some("turn-1".to_string()),
            created_at_ms: 11,
            kind: EventKind::PermissionReviewCompleted {
                record: test_guardian_review_record(oppi_protocol::GuardianReviewStatus::Denied),
            },
        };
        let completed_line = render_event_line(&completed).expect("completed review line");
        assert!(completed_line.contains("guardian review denied"));
        assert!(completed_line.contains("risk=high"));
        assert!(completed_line.contains("auth=unknown"));
        assert!(!completed_line.contains(".env"));
        assert!(!completed_line.contains("C:\\repo"));

        let entry = transcript_entry_from_event(
            &completed,
            completed_line,
            &BTreeMap::<String, ToolCall>::new(),
        );
        assert_eq!(entry.kind, TranscriptEntryKind::Denied);
        assert_eq!(entry.label, "review");
        assert_eq!(entry.tool_call_id.as_deref(), Some("write-env"));
    }

    #[test]
    fn retained_tui_keeps_scrollback_docks_and_dirty_components() {
        let mut ui = RetainedTui::new(3);
        ui.push_transcript("one");
        ui.push_transcript("two");
        ui.push_transcript("three");
        ui.push_transcript("four");
        ui.push_transcript(EXIT_COMMAND_ECHO_TEXT);
        ui.push_transcript(EXIT_REQUESTED_TEXT);
        assert_eq!(ui.scrollback.len(), 3);
        assert!(!ui.scrollback.iter().any(|line| line == "one"));
        assert!(ui.dirty_components().contains(&"transcript"));
        ui.clear_dirty();

        ui.update_docks(DockState {
            todos: vec!["50-12:InProgress".to_string()],
            approval: Some("approval-1 pending — /approve or /deny".to_string()),
            question: None,
            background: Some("1 running; latest=task-1".to_string()),
            suggestion: Some("ship it".to_string()),
            footer: "status: role=executor model=mock permissions=auto-review memory=client-hosted todos=1 queued=0 diagnostics=line-mode/raw-deferred variant=off theme=plain thread=thread-1".to_string(),
        });
        let dirty = ui.dirty_components();
        assert!(dirty.contains(&"todos"));
        assert!(dirty.contains(&"approval"));
        assert!(dirty.contains(&"background"));
        assert!(dirty.contains(&"footer"));

        let frame = ui.render_frame(72, 12);
        assert!(frame.contains("OPPi retained scrollback"));
        assert!(frame.contains("four"));
        assert!(frame.contains(EXIT_COMMAND_ECHO_TEXT));
        assert!(frame.contains(EXIT_REQUESTED_TEXT));
        assert!(frame.contains("50-12:InProgress"));
        assert!(frame.contains("line-mode fallback"));
        assert!(!frame.contains("one"));
        for line in frame.lines() {
            assert!(line.chars().count() <= 72, "too wide: {line}");
        }
    }

    #[test]
    fn retained_tui_redacts_credentials_before_render_storage() {
        let mut ui = RetainedTui::new(4);
        ui.push_transcript("OPENAI_API_KEY=sk-secret Bearer sk-token api_key=raw");
        let line = ui.scrollback.back().expect("redacted line");
        assert!(!line.contains("sk-secret"));
        assert!(!line.contains("sk-token"));
        assert!(!line.contains("api_key=raw"));
        assert!(line.contains("[REDACTED]"));
    }

    #[test]
    fn retained_tui_preserves_typed_event_metadata_for_ratatui() {
        let mut ui = RetainedTui::new(8);
        let call = ToolCall {
            id: "tool-1".to_string(),
            name: "write_file".to_string(),
            namespace: Some("oppi".to_string()),
            arguments: json!({"path":"docs/out.md"}),
        };
        let mut calls = BTreeMap::new();
        calls.insert(call.id.clone(), call.clone());
        let tool_event = Event {
            id: 41,
            thread_id: "thread-1".to_string(),
            turn_id: Some("turn-1".to_string()),
            created_at_ms: 41,
            kind: EventKind::ToolCallStarted { call },
        };
        ui.push_event_transcript(&tool_event, "tool started".to_string(), &calls);
        let artifact_event = Event {
            id: 42,
            thread_id: "thread-1".to_string(),
            turn_id: Some("turn-1".to_string()),
            created_at_ms: 42,
            kind: EventKind::ArtifactCreated {
                artifact: ArtifactMetadata {
                    id: "artifact-1".to_string(),
                    tool_call_id: "tool-1".to_string(),
                    output_path: "docs/out.md".to_string(),
                    mime_type: Some("text/markdown".to_string()),
                    width: None,
                    height: None,
                    source_images: Vec::new(),
                    mask: None,
                    backend: None,
                    model: None,
                    bytes: Some(2048),
                    diagnostics: Vec::new(),
                },
            },
        };
        ui.push_event_transcript(&artifact_event, "artifact created".to_string(), &calls);

        assert_eq!(ui.typed_scrollback.len(), 2);
        assert_eq!(ui.typed_scrollback[0].kind, TranscriptEntryKind::ToolWrite);
        assert_eq!(
            ui.typed_scrollback[0].tool_call_id.as_deref(),
            Some("tool-1")
        );
        assert_eq!(ui.typed_scrollback[1].kind, TranscriptEntryKind::Artifact);
        assert_eq!(ui.typed_scrollback[1].event_id, 42);
        assert_eq!(
            ui.typed_scrollback[1].artifact_id.as_deref(),
            Some("artifact-1")
        );
        assert!(
            ui.typed_scrollback[1]
                .body
                .contains("artifact://docs/out.md")
        );
        assert!(ui.typed_scrollback[1].body.contains("text/markdown"));
        assert!(ui.typed_scrollback[1].body.contains("2.0KiB"));
    }

    #[test]
    fn image_artifact_rendering_emits_terminal_preview_when_enabled() {
        let artifact = ArtifactMetadata {
            id: "artifact-image-1".to_string(),
            tool_call_id: "image-1".to_string(),
            output_path: "output/image.png".to_string(),
            mime_type: Some("image/png".to_string()),
            width: Some(1024),
            height: Some(768),
            source_images: Vec::new(),
            mask: None,
            backend: Some("openai-images".to_string()),
            model: Some("gpt-image-2".to_string()),
            bytes: Some(4096),
            diagnostics: Vec::new(),
        };
        let settings = ImageSettings {
            show_images: true,
            image_width_cells: 120,
            auto_resize_images: true,
            block_images: false,
            preview_protocol: ImagePreviewProtocolOverride::Auto,
        };

        let rendered = format_artifact_created_with_image_settings(
            &artifact,
            theme_tokens("plain"),
            &settings,
        );

        assert!(rendered.contains("artifact artifact-image-1 created"));
        assert!(rendered.contains("output/image.png"));
        assert!(rendered.contains("\x1b_G"));
        assert!(rendered.contains("a=T"));
        assert!(rendered.contains("t=f"));
        assert!(rendered.contains("c=120"));
        assert!(rendered.contains("b3V0cHV0L2ltYWdlLnBuZw=="));
        assert!(rendered.contains("\x1b\\"));
    }

    #[test]
    fn image_artifact_rendering_supports_common_non_png_image_previews() {
        let settings = ImageSettings {
            show_images: true,
            image_width_cells: 80,
            auto_resize_images: true,
            block_images: false,
            preview_protocol: ImagePreviewProtocolOverride::Auto,
        };
        for (path, mime, encoded_path) in [
            ("output/photo.jpg", "image/jpeg", "b3V0cHV0L3Bob3RvLmpwZw=="),
            (
                "output/render.webp",
                "image/webp",
                "b3V0cHV0L3JlbmRlci53ZWJw",
            ),
        ] {
            let artifact = ArtifactMetadata {
                id: format!("artifact-{path}"),
                tool_call_id: "image-1".to_string(),
                output_path: path.to_string(),
                mime_type: Some(mime.to_string()),
                width: None,
                height: None,
                source_images: Vec::new(),
                mask: None,
                backend: None,
                model: None,
                bytes: None,
                diagnostics: Vec::new(),
            };

            let rendered = format_artifact_created_with_image_settings(
                &artifact,
                theme_tokens("plain"),
                &settings,
            );

            assert!(
                rendered.contains("\x1b_G"),
                "{path} should render an inline preview"
            );
            assert!(rendered.contains("t=f"));
            assert!(rendered.contains("c=80"));
            assert!(rendered.contains(encoded_path));
        }
    }

    #[test]
    fn image_artifact_rendering_skips_escape_sequence_without_supported_terminal() {
        let artifact = ArtifactMetadata {
            id: "artifact-image-1".to_string(),
            tool_call_id: "image-1".to_string(),
            output_path: "output/image.png".to_string(),
            mime_type: Some("image/png".to_string()),
            width: None,
            height: None,
            source_images: Vec::new(),
            mask: None,
            backend: None,
            model: None,
            bytes: None,
            diagnostics: Vec::new(),
        };
        let settings = ImageSettings {
            show_images: true,
            image_width_cells: 80,
            auto_resize_images: true,
            block_images: false,
            preview_protocol: ImagePreviewProtocolOverride::Auto,
        };

        let rendered = format_artifact_created_with_image_settings_and_protocol(
            &artifact,
            theme_tokens("plain"),
            &settings,
            None,
        );

        assert!(!rendered.contains("\x1b_G"));
        assert!(
            rendered.contains("terminal does not advertise native inline image support"),
            "{rendered}"
        );
    }

    #[test]
    fn terminal_image_protocol_detection_recognizes_kitty_and_wezterm() {
        assert_eq!(
            terminal_image_protocol_from_env_values(Some("xterm-kitty"), None, None, None),
            Some(TerminalImageProtocol::KittyLocalFile)
        );
        assert_eq!(
            terminal_image_protocol_from_env_values(
                Some("xterm-256color"),
                Some("WezTerm"),
                None,
                None
            ),
            Some(TerminalImageProtocol::KittyLocalFile)
        );
        assert_eq!(
            terminal_image_protocol_from_env_values(Some("dumb"), None, Some("12"), None),
            Some(TerminalImageProtocol::KittyLocalFile)
        );
        assert_eq!(
            terminal_image_protocol_from_env_values(
                Some("xterm-256color"),
                None,
                None,
                Some("wezterm.exe")
            ),
            Some(TerminalImageProtocol::KittyLocalFile)
        );
        assert_eq!(
            terminal_image_protocol_from_env_values(
                Some("dumb"),
                Some("Apple_Terminal"),
                None,
                None
            ),
            None
        );
    }

    #[test]
    fn terminal_image_protocol_detection_recognizes_iterm2() {
        assert_eq!(
            terminal_image_protocol_from_env_values(
                Some("xterm-256color"),
                Some("iTerm.app"),
                None,
                None
            ),
            Some(TerminalImageProtocol::Iterm2InlineFile)
        );
        assert_eq!(
            terminal_image_protocol_from_env_values(
                Some("xterm-256color"),
                Some("iTerm2"),
                None,
                None
            ),
            Some(TerminalImageProtocol::Iterm2InlineFile)
        );
    }

    #[test]
    fn terminal_image_protocol_override_beats_incomplete_terminal_env() {
        assert_eq!(
            terminal_image_protocol_from_env_values_with_override(
                Some("sixel"),
                Some("dumb"),
                Some("Apple_Terminal"),
                None,
                None
            ),
            Some(TerminalImageProtocol::Sixel)
        );
        assert_eq!(
            terminal_image_protocol_detection_from_env_values_with_override(
                Some("iterm2"),
                Some("xterm-kitty"),
                None,
                Some("12"),
                Some("wezterm.exe")
            ),
            TerminalImageProtocolDetection {
                protocol: Some(TerminalImageProtocol::Iterm2InlineFile),
                source: "OPPI_IMAGE_PREVIEW_PROTOCOL=iterm2",
            }
        );
        assert_eq!(
            terminal_image_protocol_detection_from_env_values_with_override(
                Some("off"),
                Some("xterm-kitty"),
                None,
                Some("12"),
                None
            ),
            TerminalImageProtocolDetection {
                protocol: None,
                source: "OPPI_IMAGE_PREVIEW_PROTOCOL=off",
            }
        );
    }

    #[test]
    fn image_preview_capabilities_report_protocol_source_and_fallbacks() {
        let settings = ImageSettings {
            show_images: true,
            image_width_cells: 120,
            auto_resize_images: true,
            block_images: false,
            preview_protocol: ImagePreviewProtocolOverride::Auto,
        };
        let kitty = image_preview_capabilities_from_env_values(
            &settings,
            Some("xterm-kitty"),
            None,
            None,
            None,
        );

        assert_eq!(kitty["showImages"], json!(true));
        assert_eq!(kitty["imageWidthCells"], json!(120));
        assert_eq!(kitty["autoResizeImages"], json!(true));
        assert_eq!(kitty["blockImages"], json!(false));
        assert_eq!(kitty["protocol"], json!("kitty-local-file"));
        assert_eq!(kitty["source"], json!("TERM=xterm-kitty"));
        assert_eq!(kitty["inlinePreviews"], json!(true));
        assert_eq!(kitty["activeProbePerformed"], json!(false));
        assert_eq!(kitty["activeProbeStatus"], json!("not-attempted"));
        assert_eq!(
            kitty["activeProbeReason"],
            json!("transcript-safe-env-and-settings-only")
        );
        assert_eq!(kitty["sixelSupported"], json!(false));
        assert_eq!(
            kitty["supportedFormats"],
            json!(["png", "jpeg", "jpg", "webp", "gif"])
        );
        assert_eq!(kitty["maxInlineBytes"], Value::Null);

        let kitty_text = format_image_preview_capabilities(&kitty);
        assert!(kitty_text.contains("kitty-local-file"));
        assert!(kitty_text.contains("TERM=xterm-kitty"));
        assert!(kitty_text.contains("active probe not attempted"));
        assert!(kitty_text.contains("sixel unsupported"));

        let disabled = ImageSettings {
            show_images: false,
            image_width_cells: 80,
            auto_resize_images: true,
            block_images: false,
            preview_protocol: ImagePreviewProtocolOverride::Auto,
        };
        let unsupported = image_preview_capabilities_from_env_values(
            &disabled,
            Some("dumb"),
            Some("Apple_Terminal"),
            None,
            None,
        );

        assert_eq!(unsupported["protocol"], Value::Null);
        assert_eq!(unsupported["source"], json!("none"));
        assert_eq!(unsupported["inlinePreviews"], json!(false));
        let unsupported_text = format_image_preview_capabilities(&unsupported);
        assert!(unsupported_text.contains("previews disabled in settings"));
        assert!(
            unsupported_text.contains("terminal does not advertise native inline image support")
        );
    }

    #[test]
    fn image_artifact_rendering_skips_unbounded_iterm2_payloads() {
        let root = env::temp_dir().join(format!("oppi-iterm-large-image-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let image_path = root.join("large.png");
        fs::write(
            &image_path,
            vec![b'x'; ITERM2_INLINE_IMAGE_MAX_BYTES as usize + 1],
        )
        .unwrap();
        let artifact = ArtifactMetadata {
            id: "artifact-image-1".to_string(),
            tool_call_id: "image-1".to_string(),
            output_path: image_path.display().to_string(),
            mime_type: Some("image/png".to_string()),
            width: None,
            height: None,
            source_images: Vec::new(),
            mask: None,
            backend: None,
            model: None,
            bytes: Some(ITERM2_INLINE_IMAGE_MAX_BYTES + 1),
            diagnostics: Vec::new(),
        };
        let settings = ImageSettings {
            show_images: true,
            image_width_cells: 80,
            auto_resize_images: true,
            block_images: false,
            preview_protocol: ImagePreviewProtocolOverride::Auto,
        };

        let rendered = format_artifact_created_with_image_settings_and_protocol(
            &artifact,
            theme_tokens("plain"),
            &settings,
            Some(TerminalImageProtocol::Iterm2InlineFile),
        );

        assert!(!rendered.contains("\x1b]1337;File="));
        assert!(rendered.contains("unsupported image format for native inline preview"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn image_artifact_rendering_prompts_when_preview_disabled() {
        let artifact = ArtifactMetadata {
            id: "artifact-image-1".to_string(),
            tool_call_id: "image-1".to_string(),
            output_path: "output/image.png".to_string(),
            mime_type: Some("image/png".to_string()),
            width: None,
            height: None,
            source_images: Vec::new(),
            mask: None,
            backend: None,
            model: None,
            bytes: None,
            diagnostics: Vec::new(),
        };
        let settings = ImageSettings {
            show_images: false,
            image_width_cells: 80,
            auto_resize_images: true,
            block_images: false,
            preview_protocol: ImagePreviewProtocolOverride::Auto,
        };

        let rendered = format_artifact_created_with_image_settings(
            &artifact,
            theme_tokens("plain"),
            &settings,
        );

        assert!(!rendered.contains("\x1b_G"));
        assert!(rendered.contains("Enable terminal images to preview inline."));
    }

    fn sample_retained_ui_snapshot() -> RetainedTui {
        let mut ui = RetainedTui::new(20);
        ui.push_transcript("assistant:\n# Plan\n  • one\n```diff\n+ add\n- del\n```");
        ui.push_transcript("tool oppi:write_file started (write-1)");
        ui.push_transcript(
            "tool oppi:write_file → ok\n{\"outputPath\":\"docs/native-shell-dogfood.md\"}\nartifact: docs/native-shell-dogfood.md",
        );
        ui.push_transcript(format_artifact_created(
            &ArtifactMetadata {
                id: "artifact-image-1".to_string(),
                tool_call_id: "image-1".to_string(),
                output_path: "output/image.png".to_string(),
                mime_type: Some("image/png".to_string()),
                width: Some(64),
                height: Some(64),
                source_images: Vec::new(),
                mask: None,
                backend: Some("openai-images".to_string()),
                model: Some("gpt-image-2".to_string()),
                bytes: Some(1024),
                diagnostics: Vec::new(),
            },
            theme_tokens("plain"),
        ));
        ui.update_docks(DockState {
            todos: vec!["50-14:InProgress".to_string(), "50-15:Pending".to_string()],
            approval: Some("approval-1 pending for write-1 — /approve or /deny".to_string()),
            question: Some("Choose path — /answer <text-or-option-id>".to_string()),
            background: Some("1 running, 1 completed; latest=task-2".to_string()),
            suggestion: Some("Run snapshots".to_string()),
            footer: format!("status: role=executor model={MOCK_MODEL_DISPLAY_LABEL} permissions=auto-review memory=client-hosted todos=2 queued=1 diagnostics=line-mode/raw-deferred variant=off theme=plain thread=thread-1"),
        });
        ui
    }

    fn assert_or_update_snapshot(name: &str, actual: &str) {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("snapshots")
            .join(name);
        let actual_with_newline = if actual.ends_with('\n') {
            actual.to_string()
        } else {
            format!("{actual}\n")
        };
        let actual = normalize_snapshot_newlines(&actual_with_newline);
        if env::var("OPPI_UPDATE_SNAPSHOTS").ok().as_deref() == Some("1") {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, actual).unwrap();
            return;
        }
        let expected = normalize_snapshot_newlines(
            &fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("read snapshot {}: {error}", path.display())),
        );
        assert_eq!(actual, expected, "snapshot {} changed", path.display());
    }

    fn normalize_snapshot_newlines(value: &str) -> String {
        value.replace("\r\n", "\n").replace('\r', "\n")
    }

    fn theme_palette_snapshot() -> String {
        ["plain", "light", "dark", "oppi"]
            .iter()
            .map(|theme| {
                let tokens = if *theme == "plain" {
                    plain_theme_tokens()
                } else {
                    ansi_theme_tokens(theme)
                };
                let sample = format!(
                    "{}# Title{} | {}code{} | {}ok{} | {}warn{} | {}err{}",
                    tokens.accent,
                    tokens.reset,
                    tokens.code,
                    tokens.reset,
                    tokens.success,
                    tokens.reset,
                    tokens.warning,
                    tokens.reset,
                    tokens.error,
                    tokens.reset
                );
                format!(
                    "{theme}: accent={:?} dim={:?} code={:?} success={:?} warning={:?} error={:?} sample={:?}",
                    tokens.accent,
                    tokens.dim,
                    tokens.code,
                    tokens.success,
                    tokens.warning,
                    tokens.error,
                    sample
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn ui_snapshots_cover_widths_and_themes() {
        let ui = sample_retained_ui_snapshot();
        assert_or_update_snapshot("ui-frame-44.snap", &ui.render_frame(44, 18));
        assert_or_update_snapshot("ui-frame-72.snap", &ui.render_frame(72, 18));
        assert_or_update_snapshot("ui-frame-110.snap", &ui.render_frame(110, 18));
        assert_or_update_snapshot("theme-palette.snap", &theme_palette_snapshot());
    }

    #[test]
    fn thread_tree_marks_active_and_forks() {
        let project = oppi_protocol::ProjectRef {
            id: "project".to_string(),
            cwd: "/repo".to_string(),
            display_name: None,
            workspace_roots: Vec::new(),
        };
        let threads = vec![
            Thread {
                id: "thread-1".to_string(),
                project: project.clone(),
                status: oppi_protocol::ThreadStatus::Active,
                title: Some("Root".to_string()),
                forked_from: None,
            },
            Thread {
                id: "thread-2".to_string(),
                project,
                status: oppi_protocol::ThreadStatus::Active,
                title: Some("Branch".to_string()),
                forked_from: Some("thread-1".to_string()),
            },
        ];
        let rendered = format_thread_tree(&threads, "thread-2");
        assert!(rendered.contains("thread-1"));
        assert!(rendered.contains("* thread-2"));
        assert!(rendered.contains("forkedFrom=thread-1"));
    }

    #[test]
    fn thread_tree_can_fold_subtrees_and_marks_archived_sessions() {
        let project = oppi_protocol::ProjectRef {
            id: "project".to_string(),
            cwd: "/repo".to_string(),
            display_name: None,
            workspace_roots: Vec::new(),
        };
        let threads = vec![
            Thread {
                id: "thread-1".to_string(),
                project: project.clone(),
                status: oppi_protocol::ThreadStatus::Active,
                title: Some("Root".to_string()),
                forked_from: None,
            },
            Thread {
                id: "thread-2".to_string(),
                project: project.clone(),
                status: oppi_protocol::ThreadStatus::Archived,
                title: Some("Old branch".to_string()),
                forked_from: Some("thread-1".to_string()),
            },
            Thread {
                id: "thread-3".to_string(),
                project,
                status: oppi_protocol::ThreadStatus::Active,
                title: Some("Leaf".to_string()),
                forked_from: Some("thread-2".to_string()),
            },
        ];
        let open = format_thread_tree(&threads, "thread-1");
        assert!(open.contains("thread-2"));
        assert!(open.contains("[archived]"));

        let folded = BTreeSet::from(["thread-1".to_string()]);
        let rendered = format_thread_tree_with_folded(&threads, "thread-1", &folded);
        assert!(rendered.contains("thread-1 [+1]"));
        assert!(!rendered.contains("thread-2"));
    }

    #[test]
    fn thread_tree_filter_modes_cover_native_session_tree_visibility() {
        let project = oppi_protocol::ProjectRef {
            id: "project".to_string(),
            cwd: "/repo".to_string(),
            display_name: None,
            workspace_roots: Vec::new(),
        };
        let threads = vec![
            Thread {
                id: "thread-1".to_string(),
                project: project.clone(),
                status: oppi_protocol::ThreadStatus::Active,
                title: Some("Root".to_string()),
                forked_from: None,
            },
            Thread {
                id: "thread-2".to_string(),
                project: project.clone(),
                status: oppi_protocol::ThreadStatus::Archived,
                title: Some("Archived branch".to_string()),
                forked_from: Some("thread-1".to_string()),
            },
            Thread {
                id: "thread-3".to_string(),
                project,
                status: oppi_protocol::ThreadStatus::Active,
                title: None,
                forked_from: Some("thread-1".to_string()),
            },
        ];

        let default = format_thread_tree_with_filter(
            &threads,
            "thread-1",
            &BTreeSet::new(),
            TreeFilterMode::Default,
        );
        assert!(default.contains("thread-1"));
        assert!(default.contains("thread-3"));
        assert!(!default.contains("thread-2"));
        assert!(default.contains("filter: default"));

        let all = format_thread_tree_with_filter(
            &threads,
            "thread-1",
            &BTreeSet::new(),
            TreeFilterMode::All,
        );
        assert!(all.contains("thread-2"));
        assert!(all.contains("[archived]"));
        assert!(all.contains("filter: all"));

        let labeled = format_thread_tree_with_filter(
            &threads,
            "thread-1",
            &BTreeSet::new(),
            TreeFilterMode::LabeledOnly,
        );
        assert!(labeled.contains("thread-1"));
        assert!(labeled.contains("thread-2"));
        assert!(!labeled.contains("thread-3"));
        assert!(labeled.contains("filter: labeled-only"));
    }

    #[test]
    fn exit_resume_command_is_the_last_exit_instruction() {
        let output = format!(
            "{EXIT_REQUESTED_TEXT}\n{}",
            exit_resume_command("019e0965-e833-7093-88ea-79a2baf0fc48")
        );
        assert_eq!(
            output.lines().last(),
            Some("oppi resume 019e0965-e833-7093-88ea-79a2baf0fc48")
        );
    }

    #[test]
    fn bare_resume_lists_current_project_sessions() {
        let current_project = oppi_protocol::ProjectRef {
            id: "project".to_string(),
            cwd: "/repo".to_string(),
            display_name: None,
            workspace_roots: Vec::new(),
        };
        let other_project = oppi_protocol::ProjectRef {
            id: "other".to_string(),
            cwd: "/other".to_string(),
            display_name: None,
            workspace_roots: Vec::new(),
        };
        let threads = vec![
            Thread {
                id: "thread-1".to_string(),
                project: current_project.clone(),
                status: oppi_protocol::ThreadStatus::Active,
                title: Some("Root".to_string()),
                forked_from: None,
            },
            Thread {
                id: "thread-2".to_string(),
                project: current_project,
                status: oppi_protocol::ThreadStatus::Active,
                title: Some("Branch".to_string()),
                forked_from: Some("thread-1".to_string()),
            },
            Thread {
                id: "thread-3".to_string(),
                project: other_project,
                status: oppi_protocol::ThreadStatus::Active,
                title: Some("Elsewhere".to_string()),
                forked_from: None,
            },
        ];
        let rendered = format_resume_session_list(&threads, "thread-2", "/repo", &BTreeSet::new());
        assert!(rendered.contains("current thread: thread-2"));
        assert!(rendered.contains("recent sessions in this project:"));
        assert!(rendered.contains("thread-1"));
        assert!(rendered.contains("* thread-2"));
        assert!(!rendered.contains("thread-3"));
        assert!(rendered.contains("oppi resume <thread-id>"));
    }

    #[test]
    fn role_profiles_persist_to_settings_json() {
        let root = env::temp_dir().join(format!("oppi-role-profiles-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let path = root.join("settings.json");
        fs::write(
            &path,
            r#"{"oppi":{"promptVariant":"a","roleModels":{"reviewer":"old-review"}},"kept":true}"#,
        )
        .unwrap();

        let mut role_models = BTreeMap::new();
        role_models.insert("reviewer".to_string(), "gpt-review".to_string());
        role_models.insert("executor".to_string(), "gpt-exec".to_string());
        save_role_profiles(&path, &role_models).unwrap();
        save_permission_mode_setting(&path, PermissionMode::ReadOnly).unwrap();
        save_reasoning_effort_setting(&path, ThinkingLevel::High).unwrap();
        save_prompt_variant_setting(&path, "caveman").unwrap();

        let loaded = load_role_profiles(&path);
        assert_eq!(
            loaded.get("reviewer").map(String::as_str),
            Some("gpt-review")
        );
        assert_eq!(loaded.get("executor").map(String::as_str), Some("gpt-exec"));
        let raw: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(raw["oppi"]["promptVariant"], json!("caveman"));
        assert_eq!(raw["oppi"]["permissionMode"], json!("read-only"));
        assert_eq!(raw["oppi"]["reasoningEffort"], json!("high"));
        assert_eq!(load_prompt_variant_setting(&path), "caveman");
        assert_eq!(
            load_permission_mode_setting(&path).0,
            PermissionMode::ReadOnly
        );
        assert_eq!(
            load_reasoning_effort_setting(&path),
            Some(ThinkingLevel::High)
        );
        assert_eq!(raw["kept"], json!(true));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn role_profiles_route_openai_provider_models() {
        let mut role_models = BTreeMap::new();
        role_models.insert("reviewer".to_string(), "gpt-review".to_string());
        let provider = ProviderConfig::OpenAiCompatible(OpenAiCompatibleConfig {
            flavor: DirectProviderFlavor::OpenAiCompatible,
            provider_id: None,
            model: "gpt-exec".to_string(),
            base_url: None,
            api_key_env: Some("OPPI_OPENAI_API_KEY".to_string()),
            system_prompt: None,
            temperature: None,
            reasoning_effort: None,
            max_output_tokens: None,
            stream: true,
        });
        match provider_for_role_config(&provider, &role_models, Some("reviewer")) {
            ProviderConfig::OpenAiCompatible(config) => assert_eq!(config.model, "gpt-review"),
            ProviderConfig::Mock => panic!("expected openai-compatible provider"),
        }
        match provider_for_role_config(&provider, &role_models, Some("executor")) {
            ProviderConfig::OpenAiCompatible(config) => assert_eq!(config.model, "gpt-exec"),
            ProviderConfig::Mock => panic!("expected openai-compatible provider"),
        }
        assert_eq!(role_for_command("/review"), "reviewer");
        assert_eq!(role_for_command("/independent"), "orchestrator");
    }

    #[test]
    fn default_role_models_use_gpt_for_main_and_codex_for_coding_subagents() {
        let role_models = BTreeMap::new();
        let provider = ProviderConfig::OpenAiCompatible(OpenAiCompatibleConfig {
            flavor: DirectProviderFlavor::OpenAiCodex,
            provider_id: None,
            model: OPENAI_CODEX_DEFAULT_MODEL.to_string(),
            base_url: None,
            api_key_env: None,
            system_prompt: None,
            temperature: None,
            reasoning_effort: Some("xhigh".to_string()),
            max_output_tokens: None,
            stream: true,
        });

        match provider_for_role_config(&provider, &role_models, Some("executor")) {
            ProviderConfig::OpenAiCompatible(config) => {
                assert_eq!(config.model, GPT_MAIN_DEFAULT_MODEL);
                assert_eq!(config.reasoning_effort.as_deref(), Some("xhigh"));
            }
            ProviderConfig::Mock => panic!("expected openai-compatible provider"),
        }
        match provider_for_role_config(&provider, &role_models, Some("subagent")) {
            ProviderConfig::OpenAiCompatible(config) => {
                assert_eq!(config.model, GPT_CODING_SUBAGENT_DEFAULT_MODEL);
                assert_eq!(config.reasoning_effort.as_deref(), Some("high"));
            }
            ProviderConfig::Mock => panic!("expected openai-compatible provider"),
        }
        match provider_for_role_config_with_complexity(
            &provider,
            &role_models,
            Some("subagent"),
            true,
        ) {
            ProviderConfig::OpenAiCompatible(config) => {
                assert_eq!(config.model, GPT_MAIN_DEFAULT_MODEL);
                assert_eq!(config.reasoning_effort.as_deref(), Some("xhigh"));
            }
            ProviderConfig::Mock => panic!("expected openai-compatible provider"),
        }
    }

    #[test]
    fn default_role_models_use_opus_and_sonnet_for_claude_only_provider() {
        let role_models = BTreeMap::new();
        let provider = ProviderConfig::OpenAiCompatible(meridian_provider_config(None));

        match provider_for_role_config(&provider, &role_models, Some("executor")) {
            ProviderConfig::OpenAiCompatible(config) => {
                assert_eq!(config.model, CLAUDE_MAIN_DEFAULT_MODEL);
                assert_eq!(config.reasoning_effort.as_deref(), Some("high"));
            }
            ProviderConfig::Mock => panic!("expected openai-compatible provider"),
        }
        match provider_for_role_config(&provider, &role_models, Some("subagent")) {
            ProviderConfig::OpenAiCompatible(config) => {
                assert_eq!(config.model, CLAUDE_CODING_SUBAGENT_DEFAULT_MODEL);
                assert_eq!(config.reasoning_effort.as_deref(), Some("high"));
            }
            ProviderConfig::Mock => panic!("expected openai-compatible provider"),
        }
    }

    #[test]
    fn authenticated_default_provider_prefers_gpt_subscription_over_claude_bridge() {
        let copilot = GitHubCopilotStoredAuth {
            access_token: "proxy-ep=proxy.individual.githubcopilot.com".to_string(),
            refresh_token: "refresh".to_string(),
            expires: 1,
            enterprise_domain: None,
        };

        match default_authenticated_provider_config_from_state(true, Some(&copilot), true)
            .expect("codex default")
        {
            ProviderConfig::OpenAiCompatible(config) => {
                assert_eq!(config.flavor, DirectProviderFlavor::OpenAiCodex);
                assert_eq!(config.model, GPT_MAIN_DEFAULT_MODEL);
                assert_eq!(config.reasoning_effort.as_deref(), Some("xhigh"));
            }
            ProviderConfig::Mock => panic!("expected openai-compatible provider"),
        }

        match default_authenticated_provider_config_from_state(false, Some(&copilot), true)
            .expect("copilot default")
        {
            ProviderConfig::OpenAiCompatible(config) => {
                assert_eq!(config.flavor, DirectProviderFlavor::GitHubCopilot);
                assert_eq!(config.model, GPT_MAIN_DEFAULT_MODEL);
                assert_eq!(config.reasoning_effort.as_deref(), Some("xhigh"));
            }
            ProviderConfig::Mock => panic!("expected openai-compatible provider"),
        }

        match default_authenticated_provider_config_from_state(false, None, true)
            .expect("claude-only default")
        {
            ProviderConfig::OpenAiCompatible(config) => {
                assert_eq!(config.model, CLAUDE_MAIN_DEFAULT_MODEL);
                assert_eq!(config.reasoning_effort.as_deref(), Some("high"));
            }
            ProviderConfig::Mock => panic!("expected openai-compatible provider"),
        }
    }

    #[test]
    fn codex_oauth_persist_uses_pi_auth_lock_and_preserves_other_credentials() {
        let root = env::temp_dir().join(format!("oppi-codex-auth-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let path = root.join("auth.json");
        fs::write(
            &path,
            r#"{
  "other-provider": { "type": "api_key", "key": "keep-me" }
}
"#,
        )
        .unwrap();

        persist_codex_oauth(
            &path,
            json!({
                "access_token": fake_codex_access_token("acct_test"),
                "refresh_token": "refresh_test",
                "expires_in": 3600,
            }),
        )
        .unwrap();

        let stored = read_json_or_empty(&path).unwrap();
        assert_eq!(stored["other-provider"]["key"], json!("keep-me"));
        assert_eq!(stored[OPENAI_CODEX_PROVIDER_ID]["type"], json!("oauth"));
        assert_eq!(
            stored[OPENAI_CODEX_PROVIDER_ID]["refresh"],
            json!("refresh_test")
        );
        assert_eq!(
            stored[OPENAI_CODEX_PROVIDER_ID]["accountId"],
            json!("acct_test")
        );
        assert!(!auth_lock_path(&path).exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn copilot_oauth_persist_uses_pi_auth_store_and_derives_base_url() {
        let root = env::temp_dir().join(format!("oppi-copilot-auth-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let path = root.join("auth.json");
        fs::write(&path, r#"{"other":{"type":"api_key","key":"keep"}}"#).unwrap();

        persist_github_copilot_oauth(
            &path,
            "github-refresh-token",
            Some("ghe.example.com"),
            json!({
                "token": "tid=1;proxy-ep=proxy.enterprise.githubcopilot.com;exp=4102444800;",
                "expires_at": 4_102_444_800i64,
            }),
        )
        .unwrap();

        let stored = read_json_or_empty(&path).unwrap();
        assert_eq!(stored["other"]["key"], json!("keep"));
        assert_eq!(stored[GITHUB_COPILOT_PROVIDER_ID]["type"], json!("oauth"));
        assert_eq!(
            stored[GITHUB_COPILOT_PROVIDER_ID]["refresh"],
            json!("github-refresh-token")
        );
        assert_eq!(
            stored[GITHUB_COPILOT_PROVIDER_ID]["enterpriseUrl"],
            json!("ghe.example.com")
        );
        let auth = read_github_copilot_auth_at(&path).unwrap();
        assert_eq!(
            github_copilot_base_url(&auth),
            "https://api.enterprise.githubcopilot.com"
        );
        assert_eq!(
            normalize_github_enterprise_domain("https://GHE.EXAMPLE.com/org"),
            Some("ghe.example.com".to_string())
        );
        assert!(!auth_lock_path(&path).exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn suggestion_command_formatters_show_debug_without_secrets() {
        let suggestion = SuggestedNextMessage {
            message: "run the smoke test".to_string(),
            confidence: 0.82,
            reason: Some("short likely next step".to_string()),
        };
        let summary = format_suggestion_summary(&suggestion);
        assert!(summary.contains("run the smoke test"));
        assert!(summary.contains("82%"));
        let debug = format_suggestion_debug(&suggestion);
        assert!(debug.contains("confidence: 0.82"));
        assert!(debug.contains("short likely next step"));
    }

    #[test]
    fn effort_levels_are_model_dependent_like_pi_oppi() {
        let gpt54 = ProviderConfig::OpenAiCompatible(OpenAiCompatibleConfig {
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
        let allowed = allowed_effort_levels_for_provider(&gpt54);
        assert_eq!(allowed.first(), Some(&ThinkingLevel::Off));
        assert_eq!(allowed.last(), Some(&ThinkingLevel::XHigh));
        assert_eq!(
            recommended_effort_level_for_provider(&gpt54),
            ThinkingLevel::High
        );
        assert!(format_effort_status(&gpt54).contains("recommended: High"));

        let gpt41 = ProviderConfig::OpenAiCompatible(OpenAiCompatibleConfig {
            flavor: DirectProviderFlavor::OpenAiCompatible,
            provider_id: None,
            model: "gpt-4.1".to_string(),
            base_url: None,
            api_key_env: Some("OPPI_OPENAI_API_KEY".to_string()),
            system_prompt: None,
            temperature: None,
            reasoning_effort: None,
            max_output_tokens: None,
            stream: true,
        });
        assert_eq!(
            allowed_effort_levels_for_provider(&gpt41),
            vec![ThinkingLevel::Off]
        );
        assert_eq!(
            recommended_effort_level_for_provider(&gpt41),
            ThinkingLevel::Off
        );
        assert!(format_effort_status(&gpt41).contains("locked to Off"));

        let claude = ProviderConfig::OpenAiCompatible(OpenAiCompatibleConfig {
            flavor: DirectProviderFlavor::OpenAiCompatible,
            provider_id: None,
            model: "claude-opus-4.7".to_string(),
            base_url: Some("http://127.0.0.1:3456".to_string()),
            api_key_env: Some(MERIDIAN_API_KEY_ENV.to_string()),
            system_prompt: None,
            temperature: None,
            reasoning_effort: Some("xhigh".to_string()),
            max_output_tokens: None,
            stream: true,
        });
        assert_eq!(
            effort_level_label_for_provider(&claude, ThinkingLevel::XHigh),
            "Max"
        );
        assert_eq!(normalize_thinking_level("max"), Some(ThinkingLevel::XHigh));
        assert_eq!(normalize_thinking_level("none"), Some(ThinkingLevel::Off));
    }

    #[test]
    fn effort_argument_suggestions_follow_model_limits() {
        let gpt51 = ProviderConfig::OpenAiCompatible(OpenAiCompatibleConfig {
            flavor: DirectProviderFlavor::OpenAiCodex,
            provider_id: None,
            model: "gpt-5.1".to_string(),
            base_url: None,
            api_key_env: None,
            system_prompt: None,
            temperature: None,
            reasoning_effort: Some("medium".to_string()),
            max_output_tokens: None,
            stream: true,
        });
        let limited = effort_arg_suggestions(&gpt51, "");
        assert!(limited.iter().any(|item| item.insert == "/effort auto"));
        assert!(limited.iter().any(|item| item.insert == "/effort high"));
        assert!(!limited.iter().any(|item| item.insert == "/effort xhigh"));

        let gpt54 = ProviderConfig::OpenAiCompatible(OpenAiCompatibleConfig {
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
        let full = effort_arg_suggestions(&gpt54, "x");
        assert_eq!(
            full.iter()
                .map(|item| item.insert.as_str())
                .collect::<Vec<_>>(),
            vec!["/effort xhigh"]
        );
    }

    #[test]
    fn native_model_catalog_tracks_current_pi_defaults_and_provider_order() {
        let codex = ProviderConfig::OpenAiCompatible(OpenAiCompatibleConfig {
            flavor: DirectProviderFlavor::OpenAiCodex,
            provider_id: None,
            model: OPENAI_CODEX_DEFAULT_MODEL.to_string(),
            base_url: None,
            api_key_env: None,
            system_prompt: None,
            temperature: None,
            reasoning_effort: Some("medium".to_string()),
            max_output_tokens: None,
            stream: true,
        });
        let codex_catalog = native_model_catalog_for_provider(&codex);
        assert_eq!(
            codex_catalog.first().copied(),
            Some(OPENAI_CODEX_DEFAULT_MODEL)
        );
        assert!(codex_catalog.contains(&"gpt-5.4"));
        assert!(!matches!(codex_catalog.first(), Some(&"gpt-5.1-codex")));

        let copilot = ProviderConfig::OpenAiCompatible(OpenAiCompatibleConfig {
            flavor: DirectProviderFlavor::GitHubCopilot,
            provider_id: None,
            model: GITHUB_COPILOT_DEFAULT_MODEL.to_string(),
            base_url: Some(GITHUB_COPILOT_DEFAULT_BASE_URL.to_string()),
            api_key_env: None,
            system_prompt: None,
            temperature: None,
            reasoning_effort: None,
            max_output_tokens: None,
            stream: true,
        });
        let copilot_catalog = native_model_catalog_for_provider(&copilot);
        assert_eq!(
            copilot_catalog.first().copied(),
            Some(GITHUB_COPILOT_DEFAULT_MODEL)
        );
        assert!(copilot_catalog.contains(&"claude-opus-4.7"));
    }

    #[test]
    fn custom_models_json_display_names_filter_like_pi_registry() {
        let provider = ProviderConfig::OpenAiCompatible(OpenAiCompatibleConfig {
            flavor: DirectProviderFlavor::OpenAiCodex,
            provider_id: None,
            model: OPENAI_CODEX_DEFAULT_MODEL.to_string(),
            base_url: None,
            api_key_env: None,
            system_prompt: None,
            temperature: None,
            reasoning_effort: Some("medium".to_string()),
            max_output_tokens: None,
            stream: true,
        });
        let root = env::temp_dir().join(format!("oppi-custom-models-json-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let path = root.join("models.json");
        fs::write(
            &path,
            r#"{
              "providers": {
                "openai-codex": {
                  "models": [
                    { "id": "gpt-local-coder", "name": "Local Coder" }
                  ]
                }
              }
            }"#,
        )
        .unwrap();

        let refs = native_model_refs_for_provider_at_path(&provider, &path);
        let filtered = filter_models(&refs, "Local");
        let rendered = format_model_list(&refs, None, "Local", &BTreeMap::new());

        assert!(
            filtered.iter().any(|model| {
                model.id == "gpt-local-coder" && model.display_name == "Local Coder"
            })
        );
        assert!(rendered.contains("gpt-local-coder"));
        assert!(rendered.contains("Local Coder"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn models_json_diagnostics_report_invalid_json_without_dropping_builtins() {
        let provider = ProviderConfig::OpenAiCompatible(OpenAiCompatibleConfig {
            flavor: DirectProviderFlavor::OpenAiCodex,
            provider_id: None,
            model: OPENAI_CODEX_DEFAULT_MODEL.to_string(),
            base_url: None,
            api_key_env: None,
            system_prompt: None,
            temperature: None,
            reasoning_effort: Some("medium".to_string()),
            max_output_tokens: None,
            stream: true,
        });
        let root = env::temp_dir().join(format!(
            "oppi-models-json-diagnostics-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let path = root.join("models.json");
        fs::write(&path, r#"{ "providers": { "openai-codex": "#).unwrap();

        let (refs, diagnostics) =
            native_model_refs_for_provider_at_path_with_diagnostics(&provider, &path);
        let rendered =
            format_model_list_with_diagnostics(&refs, None, "", &BTreeMap::new(), &diagnostics);

        assert!(
            refs.iter()
                .any(|model| model.id == OPENAI_CODEX_DEFAULT_MODEL)
        );
        assert!(
            diagnostics
                .iter()
                .any(|message| message.contains("models.json") && message.contains("parse"))
        );
        assert!(rendered.contains("diagnostics:"));
        assert!(rendered.contains("models.json"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn models_json_diagnostics_report_unsupported_api_unknown_override_and_missing_auth() {
        let provider = ProviderConfig::OpenAiCompatible(OpenAiCompatibleConfig {
            flavor: DirectProviderFlavor::OpenAiCodex,
            provider_id: None,
            model: OPENAI_CODEX_DEFAULT_MODEL.to_string(),
            base_url: None,
            api_key_env: None,
            system_prompt: None,
            temperature: None,
            reasoning_effort: Some("medium".to_string()),
            max_output_tokens: None,
            stream: true,
        });
        let root = env::temp_dir().join(format!(
            "oppi-models-json-diagnostics-rest-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let path = root.join("models.json");
        fs::write(
            &path,
            r#"{
              "providers": {
                "openai-codex": {
                  "modelOverrides": {
                    "gpt-ghost": { "name": "Ghost Route" }
                  },
                  "models": [
                    {
                      "id": "codex-anthropic-route",
                      "name": "Codex Anthropic Route",
                      "api": "anthropic-messages"
                    }
                  ]
                },
                "local-no-auth": {
                  "baseUrl": "http://localhost:11434/v1",
                  "api": "openai-completions",
                  "models": [
                    { "id": "llama3.1:8b" }
                  ]
                },
                "gemini-proxy": {
                  "baseUrl": "https://generativelanguage.googleapis.com/v1beta",
                  "api": "google-generative-ai",
                  "apiKey": "GEMINI_API_KEY",
                  "models": [
                    { "id": "gemma-4-31b-it" }
                  ]
                }
              }
            }"#,
        )
        .unwrap();

        let (_, diagnostics) =
            native_model_refs_for_provider_at_path_with_diagnostics(&provider, &path);

        assert!(diagnostics.iter().any(|message| {
            message.contains("unsupported provider API")
                && message.contains("anthropic-messages")
                && message.contains("codex-anthropic-route")
        }));
        assert!(diagnostics.iter().any(|message| {
            message.contains("unsupported provider API")
                && message.contains("google-generative-ai")
                && message.contains("gemini-proxy")
        }));
        assert!(diagnostics.iter().any(|message| {
            message.contains("ignored modelOverrides")
                && message.contains("openai-codex")
                && message.contains("gpt-ghost")
        }));
        assert!(diagnostics.iter().any(|message| {
            message.contains("apiKey")
                && message.contains("required")
                && message.contains("local-no-auth")
        }));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn models_json_merge_applies_overrides_and_custom_upserts_like_pi_registry() {
        let provider = ProviderConfig::OpenAiCompatible(OpenAiCompatibleConfig {
            flavor: DirectProviderFlavor::OpenAiCodex,
            provider_id: None,
            model: OPENAI_CODEX_DEFAULT_MODEL.to_string(),
            base_url: None,
            api_key_env: None,
            system_prompt: None,
            temperature: None,
            reasoning_effort: Some("medium".to_string()),
            max_output_tokens: None,
            stream: true,
        });
        let root = env::temp_dir().join(format!("oppi-models-json-merge-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let path = root.join("models.json");
        fs::write(
            &path,
            r#"{
              "providers": {
                "openai-codex": {
                  "baseUrl": "https://proxy.example.com/backend-api",
                  "api": "openai-completions",
                  "headers": { "x-provider-route": "codex" },
                  "compat": { "supportsReasoningEffort": false },
                  "modelOverrides": {
                    "gpt-5.3-codex": {
                      "name": "GPT 5.3 Codex Routed",
                      "reasoning": false,
                      "input": ["text", "image"],
                      "cost": { "input": 1.25 },
                      "contextWindow": 111000,
                      "maxTokens": 22222,
                      "headers": { "x-model-route": "codex-53" },
                      "compat": { "supportsDeveloperRole": false }
                    }
                  },
                  "models": [
                    {
                      "id": "gpt-5.4",
                      "name": "GPT 5.4 Custom Replacement",
                      "reasoning": true,
                      "input": ["text"],
                      "contextWindow": 333000,
                      "maxTokens": 44444,
                      "headers": { "x-custom-route": "replacement" },
                      "compat": { "supportsReasoningEffort": true }
                    },
                    {
                      "id": "gpt-local-coder",
                      "name": "Local Coder",
                      "api": "openai-completions",
                      "baseUrl": "http://localhost:11434/v1",
                      "headers": { "x-local-route": "1" }
                    }
                  ]
                }
              }
            }"#,
        )
        .unwrap();

        let (entries, diagnostics) =
            native_model_entries_for_provider_at_path_with_diagnostics(&provider, &path);
        assert_eq!(diagnostics, Vec::<String>::new());

        let overridden = entries
            .iter()
            .find(|model| model.id == "gpt-5.3-codex")
            .unwrap();
        assert_eq!(overridden.display_name, "GPT 5.3 Codex Routed");
        assert_eq!(
            overridden.base_url.as_deref(),
            Some("https://proxy.example.com/backend-api")
        );
        assert_eq!(overridden.reasoning, Some(false));
        assert_eq!(overridden.input, vec!["text", "image"]);
        assert_eq!(overridden.context_window, Some(111000));
        assert_eq!(overridden.max_tokens, Some(22222));
        assert_eq!(
            overridden.headers.as_ref().unwrap()["x-provider-route"],
            json!("codex")
        );
        assert_eq!(
            overridden.headers.as_ref().unwrap()["x-model-route"],
            json!("codex-53")
        );
        assert_eq!(
            overridden.compat.as_ref().unwrap()["supportsReasoningEffort"],
            json!(false)
        );
        assert_eq!(
            overridden.compat.as_ref().unwrap()["supportsDeveloperRole"],
            json!(false)
        );
        assert_eq!(overridden.cost.as_ref().unwrap()["input"], json!(1.25));

        let replaced = entries.iter().find(|model| model.id == "gpt-5.4").unwrap();
        assert_eq!(replaced.display_name, "GPT 5.4 Custom Replacement");
        assert_eq!(replaced.context_window, Some(333000));
        assert_eq!(replaced.max_tokens, Some(44444));
        assert_eq!(replaced.reasoning, Some(true));
        assert_eq!(
            replaced.compat.as_ref().unwrap()["supportsReasoningEffort"],
            json!(true)
        );
        assert_eq!(
            entries.iter().filter(|model| model.id == "gpt-5.4").count(),
            1
        );

        let appended = entries
            .iter()
            .find(|model| model.id == "gpt-local-coder")
            .unwrap();
        assert_eq!(appended.display_name, "Local Coder");
        assert_eq!(appended.api.as_deref(), Some("openai-completions"));
        assert_eq!(
            appended.base_url.as_deref(),
            Some("http://localhost:11434/v1")
        );
        assert!(
            entries
                .iter()
                .position(|model| model.id == "gpt-local-coder")
                > entries.iter().position(|model| model.id == "gpt-5.4")
        );

        let refs = native_model_refs_for_provider_at_path(&provider, &path);
        let rendered = format_model_list(&refs, None, "Routed", &BTreeMap::new());
        assert!(rendered.contains("gpt-5.3-codex"));
        assert!(rendered.contains("GPT 5.3 Codex Routed"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn model_auth_availability_filters_unconfigured_provider_and_allows_all_registry_entries() {
        let provider = ProviderConfig::OpenAiCompatible(OpenAiCompatibleConfig {
            flavor: DirectProviderFlavor::OpenAiCodex,
            provider_id: None,
            model: OPENAI_CODEX_DEFAULT_MODEL.to_string(),
            base_url: None,
            api_key_env: None,
            system_prompt: None,
            temperature: None,
            reasoning_effort: Some("medium".to_string()),
            max_output_tokens: None,
            stream: true,
        });
        let root = env::temp_dir().join(format!(
            "oppi-model-auth-availability-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let models_path = root.join("models.json");
        let codex_auth_path = root.join("auth.json");
        let copilot_auth_path = root.join("copilot-auth.json");

        let (available_without_auth, diagnostics) =
            main_model_refs_for_provider_at_path_with_auth_paths(
                None,
                &[],
                &provider,
                &models_path,
                &codex_auth_path,
                &copilot_auth_path,
                true,
            );
        let (all_without_auth, _) = main_model_refs_for_provider_at_path_with_auth_paths(
            None,
            &[],
            &provider,
            &models_path,
            &codex_auth_path,
            &copilot_auth_path,
            false,
        );

        assert_eq!(diagnostics, Vec::<String>::new());
        assert!(available_without_auth.is_empty());
        assert!(
            all_without_auth
                .iter()
                .any(|model| model.id == OPENAI_CODEX_DEFAULT_MODEL)
        );

        fs::write(
            &models_path,
            r#"{
              "providers": {
                "openai-codex": {
                  "apiKey": "!do-not-run-during-listing",
                  "models": [
                    { "id": "gpt-local-coder", "name": "Local Coder" }
                  ]
                }
              }
            }"#,
        )
        .unwrap();

        let (available_from_models_json_auth, _) =
            main_model_refs_for_provider_at_path_with_auth_paths(
                None,
                &[],
                &provider,
                &models_path,
                &codex_auth_path,
                &copilot_auth_path,
                true,
            );
        assert!(
            available_from_models_json_auth
                .iter()
                .any(|model| model.id == OPENAI_CODEX_DEFAULT_MODEL)
        );
        assert!(
            available_from_models_json_auth
                .iter()
                .any(|model| model.id == "gpt-local-coder" && model.display_name == "Local Coder")
        );

        let (show_all, filter) = parse_models_command_args("--all Local");
        assert!(show_all);
        assert_eq!(filter, "Local");

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn model_auth_availability_reads_codex_and_copilot_auth_store_status() {
        let root = env::temp_dir().join(format!("oppi-model-auth-stores-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let models_path = root.join("missing-models.json");
        let codex_auth_path = root.join("codex-auth.json");
        let copilot_auth_path = root.join("copilot-auth.json");
        fs::write(
            &codex_auth_path,
            r#"{
              "openai-codex": {
                "type": "oauth",
                "access": "codex-access",
                "refresh": "codex-refresh",
                "accountId": "acct_123"
              }
            }"#,
        )
        .unwrap();
        fs::write(
            &copilot_auth_path,
            r#"{
              "github-copilot": {
                "type": "oauth",
                "access": "copilot-access;proxy-ep=proxy.individual.githubcopilot.com",
                "refresh": "copilot-refresh",
                "expires": 4102444800
              }
            }"#,
        )
        .unwrap();

        let codex_provider = ProviderConfig::OpenAiCompatible(OpenAiCompatibleConfig {
            flavor: DirectProviderFlavor::OpenAiCodex,
            provider_id: None,
            model: OPENAI_CODEX_DEFAULT_MODEL.to_string(),
            base_url: None,
            api_key_env: None,
            system_prompt: None,
            temperature: None,
            reasoning_effort: Some("medium".to_string()),
            max_output_tokens: None,
            stream: true,
        });
        let copilot_provider = ProviderConfig::OpenAiCompatible(OpenAiCompatibleConfig {
            flavor: DirectProviderFlavor::GitHubCopilot,
            provider_id: None,
            model: GITHUB_COPILOT_DEFAULT_MODEL.to_string(),
            base_url: None,
            api_key_env: None,
            system_prompt: None,
            temperature: None,
            reasoning_effort: Some("medium".to_string()),
            max_output_tokens: None,
            stream: true,
        });

        let (codex_models, _) = main_model_refs_for_provider_at_path_with_auth_paths(
            None,
            &[],
            &codex_provider,
            &models_path,
            &codex_auth_path,
            &copilot_auth_path,
            true,
        );
        let (copilot_models, _) = main_model_refs_for_provider_at_path_with_auth_paths(
            None,
            &[],
            &copilot_provider,
            &models_path,
            &codex_auth_path,
            &copilot_auth_path,
            true,
        );

        assert!(codex_models.iter().any(|model| model.id == "gpt-5.5"));
        assert!(
            copilot_models
                .iter()
                .any(|model| model.id == GITHUB_COPILOT_DEFAULT_MODEL)
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn custom_provider_model_selection_builds_openai_compatible_config_from_env_ref() {
        let root = env::temp_dir().join(format!(
            "oppi-custom-provider-selection-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let path = root.join("models.json");
        fs::write(
            &path,
            r#"{
              "providers": {
                "ollama": {
                  "baseUrl": "http://localhost:11434/v1",
                  "api": "openai-completions",
                  "apiKey": "OPPI_OLLAMA_API_KEY",
                  "models": [
                    {
                      "id": "qwen2.5-coder:7b",
                      "name": "Qwen Coder Local",
                      "maxTokens": 12345,
                      "compat": { "supportsReasoningEffort": false }
                    }
                  ]
                }
              }
            }"#,
        )
        .unwrap();

        let provider =
            custom_provider_config_for_model_at_path("ollama/qwen2.5-coder:7b", &path).unwrap();

        assert_eq!(provider_name(&provider), "ollama");
        match provider {
            ProviderConfig::OpenAiCompatible(config) => {
                assert_eq!(config.flavor, DirectProviderFlavor::OpenAiCompatible);
                assert_eq!(config.model, "qwen2.5-coder:7b");
                assert_eq!(
                    config.base_url.as_deref(),
                    Some("http://localhost:11434/v1")
                );
                assert_eq!(config.api_key_env.as_deref(), Some("OPPI_OLLAMA_API_KEY"));
                assert_eq!(config.max_output_tokens, Some(12345));
                assert_eq!(config.reasoning_effort, None);
                assert!(config.stream);
            }
            ProviderConfig::Mock => panic!("expected OpenAI-compatible custom provider"),
        }

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn custom_provider_model_selection_rejects_unsupported_apis_and_raw_keys() {
        let root = env::temp_dir().join(format!(
            "oppi-custom-provider-rejections-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let path = root.join("models.json");
        fs::write(
            &path,
            r#"{
              "providers": {
                "anthropic-proxy": {
                  "baseUrl": "https://proxy.example.com/v1",
                  "api": "anthropic-messages",
                  "apiKey": "OPPI_ANTHROPIC_PROXY_API_KEY",
                  "models": [
                    { "id": "claude-proxy" }
                  ]
                },
                "google-proxy": {
                  "baseUrl": "https://generativelanguage.googleapis.com/v1beta",
                  "api": "google-generative-ai",
                  "apiKey": "OPPI_GOOGLE_PROXY_API_KEY",
                  "models": [
                    { "id": "gemma-4-31b-it" }
                  ]
                },
                "literal-local": {
                  "baseUrl": "http://localhost:11434/v1",
                  "api": "openai-completions",
                  "apiKey": "ollama",
                  "models": [
                    { "id": "llama3.1:8b" }
                  ]
                },
                "command-local": {
                  "baseUrl": "http://localhost:11434/v1",
                  "api": "openai-completions",
                  "apiKey": "!op read op://vault/item/key",
                  "models": [
                    { "id": "qwen2.5-coder:7b" }
                  ]
                }
              }
            }"#,
        )
        .unwrap();

        let anthropic =
            custom_provider_config_for_model_at_path("anthropic-proxy/claude-proxy", &path)
                .expect_err("anthropic custom API should be rejected");
        assert!(anthropic.contains("unsupported api anthropic-messages"));

        let google = custom_provider_config_for_model_at_path("google-proxy/gemma-4-31b-it", &path)
            .expect_err("google custom API should be rejected");
        assert!(google.contains("unsupported api google-generative-ai"));

        let literal = custom_provider_config_for_model_at_path("literal-local/llama3.1:8b", &path)
            .expect_err("literal apiKey should be rejected");
        assert!(literal.contains("safe environment variable reference"));
        assert!(literal.contains("raw or literal keys"));

        let command =
            custom_provider_config_for_model_at_path("command-local/qwen2.5-coder:7b", &path)
                .expect_err("command apiKey should be rejected");
        assert!(command.contains("command-backed"));
        assert!(command.contains("safe environment variable reference"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn scoped_model_scope_preserves_order_and_pi_settings_key() {
        let provider = ProviderConfig::OpenAiCompatible(OpenAiCompatibleConfig {
            flavor: DirectProviderFlavor::OpenAiCodex,
            provider_id: None,
            model: OPENAI_CODEX_DEFAULT_MODEL.to_string(),
            base_url: None,
            api_key_env: None,
            system_prompt: None,
            temperature: None,
            reasoning_effort: Some("medium".to_string()),
            max_output_tokens: None,
            stream: true,
        });
        let patterns = vec!["openai-codex/gpt-5.4".to_string(), "*mini".to_string()];
        let scoped = scoped_model_ids_for_patterns(&patterns, &provider);
        assert_eq!(scoped[0], "gpt-5.4");
        assert!(scoped.iter().any(|model| model == "gpt-5.4-mini"));
        let ordered = main_model_ids_for_selection(Some("gpt-5.5"), &patterns, &provider);
        assert_eq!(ordered[0], "gpt-5.5");
        assert_eq!(ordered[1], "gpt-5.4");

        let root = env::temp_dir().join(format!("oppi-enabled-models-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let path = root.join("settings.json");
        fs::write(&path, r#"{"oppi":{"promptVariant":"a"},"kept":true}"#).unwrap();
        save_enabled_model_scope(&path, Some(patterns.as_slice())).unwrap();
        assert_eq!(load_enabled_model_scope(&path), patterns);
        let raw: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(raw["enabledModels"][0], json!("openai-codex/gpt-5.4"));
        assert_eq!(raw["oppi"]["promptVariant"], json!("a"));
        save_enabled_model_scope(&path, None).unwrap();
        let raw: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert!(raw.get("enabledModels").is_none());
        assert_eq!(raw["kept"], json!(true));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn scoped_model_list_renders_runtime_registry_models_and_scope_order() {
        let provider = ProviderConfig::OpenAiCompatible(OpenAiCompatibleConfig {
            flavor: DirectProviderFlavor::OpenAiCodex,
            provider_id: None,
            model: OPENAI_CODEX_DEFAULT_MODEL.to_string(),
            base_url: None,
            api_key_env: None,
            system_prompt: None,
            temperature: None,
            reasoning_effort: Some("medium".to_string()),
            max_output_tokens: None,
            stream: true,
        });
        let registry = ModelRegistryListResult {
            provider: "openai-codex".to_string(),
            scope: vec![
                "openai-codex/gpt-local-coder".to_string(),
                "gpt-5.4-mini".to_string(),
            ],
            available_only: false,
            selected_model: None,
            items: vec![
                ModelRef {
                    id: "gpt-5.4-mini".to_string(),
                    provider: "openai-codex".to_string(),
                    display_name: "GPT 5.4 Mini".to_string(),
                    context_window: None,
                    role: None,
                },
                ModelRef {
                    id: "gpt-local-coder".to_string(),
                    provider: "openai-codex".to_string(),
                    display_name: "Local Coder".to_string(),
                    context_window: None,
                    role: None,
                },
            ],
            diagnostics: Vec::new(),
        };

        let rendered = format_scoped_models_from_registry(&provider, &registry);

        assert!(rendered.contains("patterns: openai-codex/gpt-local-coder, gpt-5.4-mini"));
        assert!(rendered.contains("resolved: gpt-local-coder, gpt-5.4-mini"));
        assert!(rendered.contains("gpt-local-coder (Local Coder)"));
    }

    #[test]
    fn provider_status_redacts_credentials_and_roles_filter_models() {
        let mut role_models = BTreeMap::new();
        role_models.insert("reviewer".to_string(), "gpt-review".to_string());
        let roles = format_role_profiles(&role_models, Some("gpt-main"));
        assert!(roles.contains("reviewer: gpt-review"));
        assert!(roles.contains("executor: gpt-main (inherit)"));

        let models = vec![
            ModelRef {
                id: "gpt-main".to_string(),
                provider: "openai-compatible".to_string(),
                display_name: "Main".to_string(),
                context_window: None,
                role: None,
            },
            ModelRef {
                id: "gpt-review".to_string(),
                provider: "openai-compatible".to_string(),
                display_name: "Reviewer".to_string(),
                context_window: None,
                role: Some("reviewer".to_string()),
            },
        ];
        let rendered = format_model_list(&models, Some("gpt-main"), "review", &role_models);
        assert!(rendered.contains("gpt-review"));
        assert!(!rendered.contains("gpt-main ["));

        assert!(is_safe_api_key_env_name("OPPI_OPENAI_API_KEY"));
        assert!(!is_safe_api_key_env_name("sk-secret"));
        assert_eq!(
            redacted_base_url_label("https://user:secret@example.com/v1"),
            "https://example.com"
        );
        let provider = ProviderConfig::OpenAiCompatible(OpenAiCompatibleConfig {
            flavor: DirectProviderFlavor::OpenAiCompatible,
            provider_id: None,
            model: "gpt-main".to_string(),
            base_url: Some("https://user:secret@example.com/v1".to_string()),
            api_key_env: Some("OPPI_OPENAI_API_KEY".to_string()),
            system_prompt: None,
            temperature: None,
            reasoning_effort: Some("medium".to_string()),
            max_output_tokens: None,
            stream: true,
        });
        let validation = provider_validation_panel(&provider);
        assert!(validation.contains("liveCalls: none"));
        assert!(validation.contains("/login subscription claude"));
        assert!(!validation.contains("user:secret"));
        assert!(provider_policy_text().contains("/login as the primary"));
        assert!(provider_policy_text().contains("must not spawn Meridian"));
        assert!(anthropic_provider_evaluation().contains("managed Meridian bridge"));

        let meridian = meridian_provider_config(Some("claude-opus-4-6"));
        assert_eq!(meridian.model, "claude-opus-4-6");
        assert_eq!(meridian.api_key_env.as_deref(), Some(MERIDIAN_API_KEY_ENV));
        let loopback_meridian = OpenAiCompatibleConfig {
            base_url: Some("http://127.0.0.1:3456".to_string()),
            ..meridian.clone()
        };
        assert!(provider_auth_present(&loopback_meridian));
        assert!(login_root_picker_panel(&ProviderConfig::Mock).contains("Subscription"));
        assert!(login_subscription_picker_panel().contains("Codex (ChatGPT)"));
        assert!(login_subscription_picker_panel().contains("native browser OAuth"));
        let codex = OpenAiCompatibleConfig {
            flavor: DirectProviderFlavor::OpenAiCodex,
            provider_id: None,
            model: OPENAI_CODEX_DEFAULT_MODEL.to_string(),
            base_url: None,
            api_key_env: None,
            system_prompt: None,
            temperature: None,
            reasoning_effort: Some("medium".to_string()),
            max_output_tokens: None,
            stream: true,
        };
        let codex_json = openai_provider_json(&codex);
        assert_eq!(codex_json["kind"], json!("openai-codex"));
        assert_eq!(
            provider_name(&ProviderConfig::OpenAiCompatible(codex)),
            "openai-codex"
        );
        let copilot = OpenAiCompatibleConfig {
            flavor: DirectProviderFlavor::GitHubCopilot,
            provider_id: None,
            model: GITHUB_COPILOT_DEFAULT_MODEL.to_string(),
            base_url: Some(GITHUB_COPILOT_DEFAULT_BASE_URL.to_string()),
            api_key_env: None,
            system_prompt: None,
            temperature: None,
            reasoning_effort: None,
            max_output_tokens: None,
            stream: true,
        };
        assert_eq!(
            openai_provider_json(&copilot)["kind"],
            json!("github-copilot")
        );
        assert_eq!(
            provider_name(&ProviderConfig::OpenAiCompatible(copilot)),
            "github-copilot"
        );
        assert!(login_subscription_picker_panel().contains("device-code OAuth"));
        assert!(login_claude_picker_panel().contains("Claude Code login"));
        assert!(login_claude_picker_panel().contains("Model selection stays in `/model`"));
        assert!(login_meridian_install_approval_panel().contains("Approval required"));
        assert!(login_action_approved(&["--yes"]));
        assert!(!login_action_approved(&[]));
        assert!(meridian_status_panel().contains("no hidden npx"));
    }

    #[test]
    fn native_usage_status_note_stays_local_without_punting_to_pi_usage() {
        let note = native_usage_status_note();
        assert!(note.contains("provider-neutral/local only"));
        assert!(note.contains("do not claim subscription remaining limits"));
        assert!(
            note.contains("native shell turns do not automatically inject recalled memory yet")
        );
        assert!(!note.contains("use stable Pi /usage"));
    }

    #[test]
    fn graphify_status_detects_artifacts_and_guidance() {
        let root = env::temp_dir().join(format!("oppi-graphify-status-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".graphify/wiki")).unwrap();
        fs::write(root.join(".graphify/GRAPH_REPORT.md"), "# Graph").unwrap();
        fs::write(root.join(".graphify/wiki/index.md"), "# Wiki").unwrap();
        fs::write(root.join(".graphify/graph.json"), "{}").unwrap();
        fs::write(root.join(".graphify/needs_update"), "").unwrap();
        fs::write(root.join("graphify.yaml"), "inputs:\n  scope: auto\n").unwrap();

        let status = graphify_status(&root.display().to_string());
        assert!(status.configured());
        assert!(status.needs_update);
        assert!(status.report_path.is_some());
        assert!(status.wiki_index_path.is_some());
        let rendered = format_graphify_status(&status);
        assert!(rendered.contains("Graphify codebase graph"));
        assert!(rendered.contains("needsUpdate: true"));
        assert!(graphify_install_guidance().contains("npm install -g graphifyy"));
        assert!(graphify_command_guidance().contains("graphify query"));
        let payload = graphify_status_json(&status);
        assert_eq!(payload["configured"], json!(true));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn prompt_variant_appends_to_provider_system_prompt() {
        let config = OpenAiCompatibleConfig {
            flavor: DirectProviderFlavor::OpenAiCompatible,
            provider_id: None,
            model: "mock".to_string(),
            base_url: None,
            api_key_env: None,
            system_prompt: Some("Base system".to_string()),
            temperature: None,
            reasoning_effort: None,
            max_output_tokens: None,
            stream: true,
        };
        let mut provider = openai_provider_json(&config);
        apply_prompt_variant_to_provider(&mut provider, "caveman");
        assert!(
            provider["systemPrompt"]
                .as_str()
                .unwrap()
                .contains("Base system")
        );
        assert!(
            provider["systemPrompt"]
                .as_str()
                .unwrap()
                .contains("Caveman")
        );
        assert_eq!(
            normalize_prompt_variant("promptname_a").as_deref(),
            Some("a")
        );
    }

    #[test]
    fn feature_routing_appends_to_native_provider_without_duplicates() {
        let config = OpenAiCompatibleConfig {
            flavor: DirectProviderFlavor::OpenAiCompatible,
            provider_id: None,
            model: "mock".to_string(),
            base_url: None,
            api_key_env: None,
            system_prompt: Some("Base system".to_string()),
            temperature: None,
            reasoning_effort: None,
            max_output_tokens: None,
            stream: true,
        };
        let mut provider = openai_provider_json(&config);
        apply_feature_routing_to_provider(&mut provider, "caveman");
        apply_feature_routing_to_provider(&mut provider, "caveman");

        let system = provider["systemPrompt"].as_str().unwrap();
        assert!(system.contains("Base system"));
        assert!(system.contains("OPPi feature routing"));
        assert!(system.contains("Use OPPi's extra capabilities"));
        assert!(system.contains("promptname_b"));
        assert_eq!(system.matches("OPPi feature routing:").count(), 1);
        assert_eq!(system.matches("OPPi feature routing variant:").count(), 1);
    }

    #[test]
    fn goal_continuation_appends_to_provider_system_prompt() {
        let config = OpenAiCompatibleConfig {
            flavor: DirectProviderFlavor::OpenAiCompatible,
            provider_id: None,
            model: "mock".to_string(),
            base_url: None,
            api_key_env: None,
            system_prompt: Some("Base system".to_string()),
            temperature: None,
            reasoning_effort: None,
            max_output_tokens: None,
            stream: true,
        };
        let mut provider = openai_provider_json(&config);

        append_system_prompt_to_provider(
            &mut provider,
            GOAL_CONTINUATION_SYSTEM_HEADING,
            "Continue <safely>.",
        );

        let system = provider["systemPrompt"].as_str().unwrap();
        assert!(system.contains("Base system"));
        assert!(system.contains(GOAL_CONTINUATION_SYSTEM_HEADING));
        assert!(system.contains("Continue <safely>."));
    }

    #[test]
    fn approval_and_question_panels_include_resume_actions() {
        let approval = serde_json::from_value::<ApprovalRequest>(json!({
            "id": "approval-1",
            "reason": "write requires approval",
            "risk": "medium",
            "toolCall": { "id": "write-1", "name": "write_file", "namespace": "oppi", "arguments": { "path": "x.txt" } }
        }))
        .unwrap();
        let panel = format_approval_panel(&approval);
        assert!(panel.contains("risk: Medium"));
        assert!(panel.contains("oppi:write_file"));
        assert!(panel.contains("/approve"));
        assert!(panel.contains("/deny"));

        let question = serde_json::from_value::<AskUserRequest>(json!({
            "id": "question-1",
            "title": "Choose",
            "questions": [{ "id": "q1", "question": "Proceed?", "options": [{ "id": "yes", "label": "Yes" }] }]
        }))
        .unwrap();
        let panel = format_ask_user_panel(&question);
        assert!(panel.contains("q1: Proceed?"));
        assert!(panel.contains("yes: Yes"));
        assert!(panel.contains("/answer"));
    }
}
