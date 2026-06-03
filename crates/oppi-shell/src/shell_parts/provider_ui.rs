use super::*;

fn meridian_host_port() -> Option<(String, u16)> {
    base_url_host_port(&meridian_base_url())
}

fn is_loopback_host(host: &str) -> bool {
    let host = host.trim_matches(['[', ']']).to_ascii_lowercase();
    host == "localhost" || host == "127.0.0.1" || host == "::1"
}

fn meridian_base_url_is_loopback() -> bool {
    meridian_host_port()
        .map(|(host, _)| is_loopback_host(&host))
        .unwrap_or(false)
}

fn tcp_reachable(host: &str, port: u16, timeout: Duration) -> bool {
    let Ok(addrs) = (host, port).to_socket_addrs() else {
        return false;
    };
    for addr in addrs {
        if TcpStream::connect_timeout(&addr, timeout).is_ok() {
            return true;
        }
    }
    false
}

pub(super) fn meridian_reachable() -> bool {
    meridian_host_port()
        .map(|(host, port)| tcp_reachable(&host, port, Duration::from_millis(300)))
        .unwrap_or(false)
}

pub(super) fn wait_for_meridian(timeout: Duration) -> bool {
    let started = Instant::now();
    while started.elapsed() < timeout {
        if meridian_reachable() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(300));
    }
    false
}

pub(super) fn meridian_start_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(command) = env::var_os("OPPI_MERIDIAN_COMMAND")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
    {
        candidates.push(command);
    }
    let managed = managed_meridian_bin_path();
    if managed.exists() {
        candidates.push(managed);
    }
    let local = local_meridian_bin_path();
    if local.exists() {
        candidates.push(local);
    }
    candidates.push(PathBuf::from("meridian"));
    candidates
}

pub(super) fn spawn_meridian_command(command: &Path) -> Result<Child, String> {
    let (host, port) = meridian_host_port().unwrap_or_else(|| ("127.0.0.1".to_string(), 3456));
    let mut cmd = Command::new(command);
    cmd.env(
        "MERIDIAN_DEFAULT_AGENT",
        env::var("MERIDIAN_DEFAULT_AGENT").unwrap_or_else(|_| "pi".to_string()),
    )
    .env(
        "MERIDIAN_PASSTHROUGH",
        env::var("MERIDIAN_PASSTHROUGH").unwrap_or_else(|_| "1".to_string()),
    )
    .env("MERIDIAN_HOST", host)
    .env("MERIDIAN_PORT", port.to_string())
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::null());
    cmd.spawn().map_err(|error| format!("spawn: {error}"))
}

fn claude_auth_status_summary() -> String {
    match Command::new("claude").args(["auth", "status"]).output() {
        Ok(output) if output.status.success() => {
            let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if let Ok(value) = serde_json::from_str::<Value>(&text) {
                let logged_in = value
                    .get("loggedIn")
                    .and_then(Value::as_bool)
                    .map(|value| if value { "yes" } else { "no" })
                    .unwrap_or("unknown");
                let subscription = value
                    .get("subscriptionType")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                format!("loggedIn={logged_in}; subscription={subscription}")
            } else if text.is_empty() {
                "status command succeeded".to_string()
            } else {
                compact_process_output(text.as_bytes())
            }
        }
        Ok(output) => {
            let tail = compact_process_output(&[output.stdout, output.stderr].concat());
            format!(
                "unavailable: claude auth status exited {}{}{}",
                output.status,
                if tail.is_empty() { "" } else { ": " },
                tail
            )
        }
        Err(error) => {
            format!("unavailable: {error}; run `claude login` after installing Claude Code")
        }
    }
}

pub(super) fn meridian_status_panel() -> String {
    let reachable = meridian_reachable();
    format!(
        "Meridian Claude bridge\nbaseUrl: {}\nloopbackOnly: {}\nreachable: {}\nclaudeCodeAuth: {}\nmanagedBin: {} ({})\nlocalBin: {} ({})\nmanagedModule: {} ({})\npolicy: no hidden npx/proxy/credential-helper spawning; install requires explicit approval through /login subscription claude install --yes or the picker; Claude tokens remain owned by Claude Code/Meridian\nnext: `/login subscription claude login` if needed, choose a model with `/model`, then `/login subscription claude start` or `/login subscription claude use`.",
        redacted_base_url_label(&meridian_base_url()),
        meridian_base_url_is_loopback(),
        if reachable { "yes" } else { "no" },
        claude_auth_status_summary(),
        format_path_for_display(&managed_meridian_bin_path()),
        if managed_meridian_bin_path().exists() {
            "installed"
        } else {
            "missing"
        },
        format_path_for_display(&local_meridian_bin_path()),
        if local_meridian_bin_path().exists() {
            "available"
        } else {
            "missing"
        },
        format_path_for_display(&managed_meridian_module_path()),
        if managed_meridian_module_path().exists() {
            "present"
        } else {
            "missing"
        },
    )
}

pub(super) fn meridian_provider_config(model: Option<&str>) -> OpenAiCompatibleConfig {
    with_default_reasoning_effort(OpenAiCompatibleConfig {
        flavor: DirectProviderFlavor::OpenAiCompatible,
        provider_id: None,
        model: model
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(MERIDIAN_DEFAULT_MODEL)
            .to_string(),
        base_url: Some(meridian_base_url()),
        api_key_env: Some(MERIDIAN_API_KEY_ENV.to_string()),
        system_prompt: None,
        temperature: None,
        reasoning_effort: None,
        max_output_tokens: None,
        stream: true,
    })
}

pub(super) fn provider_uses_meridian_placeholder(config: &OpenAiCompatibleConfig) -> bool {
    config.api_key_env.as_deref() == Some(MERIDIAN_API_KEY_ENV)
        && config
            .base_url
            .as_deref()
            .and_then(base_url_host_port)
            .is_some_and(|(host, _)| is_loopback_host(&host))
}

pub(super) fn redacted_base_url_label(raw: &str) -> String {
    let value = raw.trim();
    if value.is_empty() {
        return "default OpenAI-compatible endpoint".to_string();
    }
    let (scheme, rest) = value
        .split_once("://")
        .map(|(scheme, rest)| (format!("{scheme}://"), rest))
        .unwrap_or_else(|| (String::new(), value));
    let host = rest
        .split('/')
        .next()
        .unwrap_or(rest)
        .rsplit('@')
        .next()
        .unwrap_or(rest);
    format!("{scheme}{host}")
}

pub(super) fn provider_status_panel(
    provider: &ProviderConfig,
    models: &[ModelRef],
    session: &ShellSession,
) -> String {
    match provider {
        ProviderConfig::Mock => format!(
            "provider: {MOCK_PROVIDER_DISPLAY_LABEL}\nauth: not required\nliveCalls: none\nselectedModel: {}\nregisteredModels: {}\n{}",
            session
                .selected_model
                .as_deref()
                .map(display_model_id)
                .unwrap_or(MOCK_MODEL_DISPLAY_LABEL),
            models.len(),
            format_role_profiles(
                &session.role_models,
                session
                    .selected_model
                    .as_deref()
                    .or(current_provider_model(provider)),
            )
        ),
        ProviderConfig::OpenAiCompatible(config) => {
            let auth_env = provider_api_key_env(config);
            let auth_status = provider_auth_status_label(config);
            let provider_label = provider_name(provider);
            let base_default = match config.flavor {
                DirectProviderFlavor::OpenAiCompatible => {
                    "OPPI_OPENAI_BASE_URL or https://api.openai.com/v1"
                }
                DirectProviderFlavor::OpenAiCodex => {
                    "OPPI_OPENAI_CODEX_BASE_URL or https://chatgpt.com/backend-api"
                }
                DirectProviderFlavor::GitHubCopilot => "GitHub Copilot token proxy endpoint",
            };
            let login_hint = match config.flavor {
                DirectProviderFlavor::OpenAiCompatible => {
                    "/login api openai env <ENV>, /login subscription codex, /login subscription copilot, or /login subscription claude"
                }
                DirectProviderFlavor::OpenAiCodex => {
                    "/login subscription codex [--force] refreshes browser OAuth; model stays in /model"
                }
                DirectProviderFlavor::GitHubCopilot => {
                    "/login subscription copilot [--force] [--enterprise <domain>] uses Pi's GitHub device-code flow; model stays in /model"
                }
            };
            format!(
                "provider: {provider_label}\nmodel: {}\nselectedModel: {}\nbaseUrl: {}\nauth: {} ({})\nstream: {}{}{}{}\nregisteredModels: {}\nliveCalls: none for status/validate; /provider smoke is the explicit live call\ndiagnostics: secrets redacted; only env-reference names, auth-store labels, and endpoint hosts are shown\nlogin: {login_hint}\n{}",
                config.model,
                session
                    .selected_model
                    .as_deref()
                    .unwrap_or(config.model.as_str()),
                config
                    .base_url
                    .as_deref()
                    .map(redacted_base_url_label)
                    .unwrap_or_else(|| base_default.to_string()),
                auth_env,
                auth_status,
                config.stream,
                config
                    .reasoning_effort
                    .as_ref()
                    .map(|value| format!("\nreasoningEffort: {value}"))
                    .unwrap_or_default(),
                config
                    .max_output_tokens
                    .map(|value| format!("\nmaxOutputTokens: {value}"))
                    .unwrap_or_default(),
                config
                    .temperature
                    .map(|value| format!("\ntemperature: {value}"))
                    .unwrap_or_default(),
                models.len(),
                format_role_profiles(
                    &session.role_models,
                    session
                        .selected_model
                        .as_deref()
                        .or(current_provider_model(provider)),
                )
            )
        }
    }
}

pub(super) fn provider_validation_panel(provider: &ProviderConfig) -> String {
    match provider {
        ProviderConfig::Mock => {
            "provider validation: mock provider ready; no credentials, network, or live calls used"
                .to_string()
        }
        ProviderConfig::OpenAiCompatible(config) => {
            let auth_env = provider_api_key_env(config);
            let auth_status = provider_auth_status_label(config);
            let provider_label = provider_name(provider);
            let base_default = match config.flavor {
                DirectProviderFlavor::OpenAiCompatible => {
                    "OPPI_OPENAI_BASE_URL or https://api.openai.com/v1"
                }
                DirectProviderFlavor::OpenAiCodex => {
                    "OPPI_OPENAI_CODEX_BASE_URL or https://chatgpt.com/backend-api"
                }
                DirectProviderFlavor::GitHubCopilot => "GitHub Copilot token proxy endpoint",
            };
            let auth_guidance = match config.flavor {
                DirectProviderFlavor::OpenAiCompatible => {
                    "API/env auth: use `/login api openai env <ENV>`, or use subscription routes for Codex/Copilot/Claude"
                }
                DirectProviderFlavor::OpenAiCodex => {
                    "Codex auth: use `/login subscription codex [--force]`; tokens stay in the protected auth store"
                }
                DirectProviderFlavor::GitHubCopilot => {
                    "Copilot auth: use `/login subscription copilot [--force] [--enterprise <domain>]`; tokens stay in the protected auth store"
                }
            };
            format!(
                "provider validation: {}\nprovider: {provider_label}\nmodel: {}\nbaseUrl: {}\nauth: {} ({})\nliveCalls: none during validation\n{auth_guidance}\nanthropic: use `/login subscription claude` for explicit managed Meridian setup; install asks approval and no hidden proxy spawning\nsmoke: run /provider smoke to make an explicit redacted live provider call",
                if provider_local_validation_ready(provider) {
                    "ready"
                } else {
                    "blocked"
                },
                config.model,
                config
                    .base_url
                    .as_deref()
                    .map(redacted_base_url_label)
                    .unwrap_or_else(|| base_default.to_string()),
                auth_env,
                auth_status
            )
        }
    }
}

pub(super) fn provider_local_validation_ready(provider: &ProviderConfig) -> bool {
    match provider {
        ProviderConfig::Mock => true,
        ProviderConfig::OpenAiCompatible(config) => {
            !config.model.trim().is_empty() && provider_auth_present(config)
        }
    }
}

pub(super) fn provider_for_role_config(
    provider: &ProviderConfig,
    role_models: &BTreeMap<String, String>,
    role: Option<&str>,
) -> ProviderConfig {
    provider_for_role_config_with_complexity(provider, role_models, role, false)
}

pub(super) fn provider_for_role_config_with_complexity(
    provider: &ProviderConfig,
    role_models: &BTreeMap<String, String>,
    role: Option<&str>,
    promote_complex_subagent: bool,
) -> ProviderConfig {
    let Some(role) = role.and_then(normalize_role) else {
        return provider.clone();
    };
    let explicit_model = role_models.get(role).cloned();
    let model = explicit_model.clone().or_else(|| {
        default_model_for_role(provider, role, promote_complex_subagent).map(str::to_string)
    });
    let Some(model) = model else {
        return provider.clone();
    };
    match provider {
        ProviderConfig::Mock => ProviderConfig::Mock,
        ProviderConfig::OpenAiCompatible(config) => {
            let mut config = config.clone();
            config.model = model.clone();
            if explicit_model.is_none() {
                config.reasoning_effort =
                    default_effort_for_role_model(provider, role, &model, promote_complex_subagent);
            }
            ProviderConfig::OpenAiCompatible(config)
        }
    }
}

pub(super) fn default_model_for_role(
    provider: &ProviderConfig,
    role: &str,
    promote_complex_subagent: bool,
) -> Option<&'static str> {
    let role = normalize_role(role)?;
    if role != "subagent" {
        return None;
    }
    let strong_role = promote_complex_subagent;
    match provider {
        ProviderConfig::Mock => None,
        ProviderConfig::OpenAiCompatible(config)
            if provider_uses_meridian_placeholder(config)
                || (config.flavor == DirectProviderFlavor::OpenAiCompatible
                    && is_anthropic_like_model(provider, &config.model)) =>
        {
            Some(if strong_role {
                CLAUDE_MAIN_DEFAULT_MODEL
            } else {
                CLAUDE_CODING_SUBAGENT_DEFAULT_MODEL
            })
        }
        ProviderConfig::OpenAiCompatible(config)
            if matches!(
                config.flavor,
                DirectProviderFlavor::OpenAiCodex | DirectProviderFlavor::GitHubCopilot
            ) || (config.flavor == DirectProviderFlavor::OpenAiCompatible
                && (config.base_url.is_none() || config.model.starts_with("gpt-"))) =>
        {
            Some(if strong_role {
                GPT_MAIN_DEFAULT_MODEL
            } else {
                GPT_CODING_SUBAGENT_DEFAULT_MODEL
            })
        }
        ProviderConfig::OpenAiCompatible(_) => None,
    }
}

fn default_effort_for_role_model(
    provider: &ProviderConfig,
    role: &str,
    model: &str,
    promote_complex_subagent: bool,
) -> Option<String> {
    let role = normalize_role(role)?;
    let role_provider = provider_with_model(provider, model)?;
    let level = if is_anthropic_like_model(&role_provider, model) {
        ThinkingLevel::High
    } else if role == "subagent" && !promote_complex_subagent {
        ThinkingLevel::High
    } else if model == GPT_MAIN_DEFAULT_MODEL {
        ThinkingLevel::XHigh
    } else {
        recommended_effort_level_for_provider(&role_provider)
    };
    if level == ThinkingLevel::Off
        || !allowed_effort_levels_for_model(&role_provider, model).contains(&level)
    {
        None
    } else {
        Some(level.as_str().to_string())
    }
}

fn provider_with_model(provider: &ProviderConfig, model: &str) -> Option<ProviderConfig> {
    match provider {
        ProviderConfig::Mock => None,
        ProviderConfig::OpenAiCompatible(config) => {
            let mut config = config.clone();
            config.model = model.to_string();
            Some(ProviderConfig::OpenAiCompatible(config))
        }
    }
}

pub(super) fn complex_subagent_task(task: &str) -> bool {
    let lower = task.to_ascii_lowercase();
    let word_count = lower.split_whitespace().count();
    word_count >= 48
        || lower.len() >= 280
        || [
            "multi-file",
            "multiple files",
            "whole codebase",
            "architecture",
            "refactor",
            "migration",
            "complex",
            "lengthy",
            "deep",
            "audit",
            "plan",
        ]
        .iter()
        .any(|needle| lower.contains(needle))
}

pub(super) fn provider_policy_text() -> &'static str {
    "provider policy: native shell owns mock, OpenAI-compatible direct API, ChatGPT/Codex OAuth, and GitHub Copilot device-code OAuth today, with /login as the primary setup UX. /login is a picker for Subscription vs API; model choice belongs to /model or future /settings. Status/validate are local and redacted; /provider smoke is the only provider command that makes a live model call. Codex and Copilot OAuth open the browser after explicit selection and store tokens only in the protected auth store. Anthropic/Claude subscription support uses an explicit user-selected managed Meridian bridge over the Claude Code SDK. OPPi must not spawn Meridian, npx, subscription proxies, external brokers, or credential helper processes implicitly; /login subscription claude install requires explicit user approval and /login subscription claude login is the explicit Claude Code auth step."
}

pub(super) fn anthropic_provider_evaluation() -> &'static str {
    "anthropic evaluation: native /login exposes the subscription-auth path through an explicit managed Meridian bridge over the Claude Code SDK. Meridian runs on loopback with visible install/login/start/stop/use lifecycle, forces client-owned passthrough tools for OPPi, avoids implicit npx or hidden proxy spawning, and asks before managed install. Claude OAuth tokens stay in Claude Code/Meridian; OPPi does not extract or store them. Model selection stays in /model or future /settings."
}

pub(super) fn normalize_role(raw: &str) -> Option<&'static str> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "planner" | "plan" | "planning" => Some("planner"),
        "thinking" | "thinker" | "think" => Some("thinking"),
        "reviewer" | "review" | "audit" => Some("reviewer"),
        "orchestrator" | "orchestration" | "orchestrate" => Some("orchestrator"),
        "executor" | "execute" | "coding" | "coder" => Some("executor"),
        "subagent" | "subagents" | "agent" | "agents" => Some("subagent"),
        _ => None,
    }
}

pub(super) fn role_usage() -> &'static str {
    "usage: /model role <planner|thinking|reviewer|orchestrator|executor|subagent> <model-id|inherit>"
}

pub(super) fn role_for_command(command: &str) -> &'static str {
    match command {
        "/review" | "/audit" => "reviewer",
        "/init" => "planner",
        "/independent" => "orchestrator",
        "/bug-report" | "/feature-request" => "planner",
        _ => "executor",
    }
}

pub(super) fn format_suggestion_summary(suggestion: &SuggestedNextMessage) -> String {
    format!(
        "suggest-next: {}\nconfidence: {}%\nuse: Tab in suggestion dock, or /suggest-next clear",
        suggestion.message,
        (suggestion.confidence * 100.0).round() as u32
    )
}

pub(super) fn format_suggestion_debug(suggestion: &SuggestedNextMessage) -> String {
    let reason = suggestion
        .reason
        .as_deref()
        .filter(|reason| !reason.trim().is_empty())
        .unwrap_or("not provided");
    format!(
        "suggest-next debug:\nmessage: {}\nconfidence: {:.2}\nreason: {}",
        suggestion.message, suggestion.confidence, reason
    )
}

pub(super) fn native_usage_status_note() -> &'static str {
    "provider-neutral/local only; do not claim subscription remaining limits; native shell turns do not automatically inject recalled memory yet"
}

pub(super) fn format_usage_panel(session: &ShellSession, provider: &ProviderConfig) -> String {
    let model = session
        .session_model(provider)
        .map(display_model_id)
        .unwrap_or("none");
    let todos_total = session.todo_state.todos.len();
    let todos_active = session
        .todo_state
        .todos
        .iter()
        .filter(|todo| !matches!(todo.status.as_str(), "completed" | "cancelled"))
        .count();
    let suggestion = session
        .suggestion
        .as_ref()
        .map(|suggestion| format!("{}%", (suggestion.confidence * 100.0).round() as u32))
        .unwrap_or_else(|| "none".to_string());
    let usage = &session.usage_state;
    let context = usage.context_footer_label();
    let memory = session
        .memory_status
        .as_ref()
        .map(|status| {
            format!(
                "{} backend={} scope={} memories={}",
                if status.enabled {
                    "enabled"
                } else {
                    "disabled"
                },
                status.backend,
                status.scope,
                status.memory_count
            )
        })
        .unwrap_or_else(|| "unknown".to_string());
    let footer = usage
        .weekly_footer_label()
        .map(|weekly| {
            format!(
                "{} · {} · {}",
                usage.session_footer_label(),
                weekly,
                context
            )
        })
        .unwrap_or_else(|| format!("{} · {}", usage.session_footer_label(), context));
    format!(
        "usage/status:\nprovider: {}\nmodel: {}\nthread: {}\npermission: {} ({})\nsession tokens: total={} input={} output={} providerCalls={}\ncontext: {}\n{}\nfooter: {}\nmemory: {}\ngoal: {}\ntodos: {}/{} active\nqueued follow-ups: {}\nqueued steering: {}\nsuggestion: {}{}",
        display_provider_id(provider),
        model,
        session.thread_id,
        session.permission_mode.as_str(),
        session.permission_mode_source,
        usage.session.total_tokens,
        usage.session.input_tokens,
        usage.session.output_tokens,
        usage.session.provider_calls,
        context,
        usage.panel_limits_line(),
        footer,
        memory,
        session.goal_status_label(),
        todos_active,
        todos_total,
        session.follow_up_queue.len(),
        session.steering_queue.len(),
        suggestion,
        native_usage_status_note()
    )
}

pub(super) fn format_role_profiles(
    role_models: &BTreeMap<String, String>,
    session_model: Option<&str>,
) -> String {
    let inherited = session_model.unwrap_or("session model");
    let mut lines = vec!["roleModels:".to_string()];
    for role in ROLE_NAMES {
        let model = role_models
            .get(role)
            .map(String::as_str)
            .unwrap_or(inherited);
        let model = display_model_id(model);
        let suffix = if role_models.contains_key(role) {
            ""
        } else {
            " (inherit)"
        };
        lines.push(format!("  {role}: {model}{suffix}"));
    }
    lines.join("\n")
}

pub(super) fn parse_models_command_args(args: &str) -> (bool, String) {
    let mut show_all = false;
    let mut filter = Vec::<&str>::new();
    for part in args.split_whitespace() {
        match part {
            "--all" | "-a" => show_all = true,
            _ => filter.push(part),
        }
    }
    (show_all, filter.join(" "))
}

pub(super) fn filter_models(models: &[ModelRef], filter: &str) -> Vec<ModelRef> {
    let needle = filter.trim().to_ascii_lowercase();
    models
        .iter()
        .filter(|model| {
            needle.is_empty()
                || model.id.to_ascii_lowercase().contains(&needle)
                || model.provider.to_ascii_lowercase().contains(&needle)
                || model.display_name.to_ascii_lowercase().contains(&needle)
                || model
                    .role
                    .as_deref()
                    .unwrap_or_default()
                    .to_ascii_lowercase()
                    .contains(&needle)
        })
        .cloned()
        .collect()
}

#[cfg(test)]
pub(super) fn format_model_list(
    models: &[ModelRef],
    selected: Option<&str>,
    filter: &str,
    role_models: &BTreeMap<String, String>,
) -> String {
    format_model_list_with_diagnostics(models, selected, filter, role_models, &[])
}

pub(super) fn format_model_list_with_diagnostics(
    models: &[ModelRef],
    selected: Option<&str>,
    filter: &str,
    role_models: &BTreeMap<String, String>,
    diagnostics: &[String],
) -> String {
    let filtered = filter_models(models, filter);
    let mut lines = vec![format!(
        "models: {}{}",
        filtered.len(),
        if filter.trim().is_empty() {
            String::new()
        } else {
            format!(" filtered by {:?}", filter.trim())
        }
    )];
    if filtered.is_empty() {
        lines.push("  none".to_string());
    } else {
        for model in filtered {
            let marker = if selected == Some(model.id.as_str()) {
                "*"
            } else {
                " "
            };
            let role = model
                .role
                .map(|role| format!(" role={role}"))
                .unwrap_or_default();
            let display = if model.display_name != model.id {
                format!(" ({})", model.display_name)
            } else {
                String::new()
            };
            lines.push(format!(
                "{marker} {}{} [{}]{}",
                model.id, display, model.provider, role
            ));
        }
    }
    lines.push(format_role_profiles(role_models, selected));
    if !diagnostics.is_empty() {
        lines.push("diagnostics:".to_string());
        for diagnostic in diagnostics {
            lines.push(format!("  - {diagnostic}"));
        }
    }
    lines.join("\n")
}

fn format_scoped_models(session: &ShellSession, provider: &ProviderConfig) -> String {
    let catalog = native_model_ids_for_provider(provider);
    let resolved = scoped_model_ids_for_provider(session, provider);
    let mut lines = vec!["scoped models:".to_string()];
    if session.scoped_model_ids.is_empty() {
        lines.push("  all current provider models (no scope configured)".to_string());
    } else {
        lines.push(format!(
            "  patterns: {}",
            session.scoped_model_ids.join(", ")
        ));
        if resolved.is_empty() {
            lines.push("  resolved: none for current provider".to_string());
        } else {
            lines.push(format!("  resolved: {}", resolved.join(", ")));
        }
    }
    lines.push(format!("  current provider: {}", provider_name(provider)));
    lines.push(format!("  available: {}", catalog.join(", ")));
    lines.push("usage: /scoped-models enable <model|provider/model|glob>, /scoped-models disable <pattern>, /scoped-models clear".to_string());
    lines.join("\n")
}

pub(super) fn format_scoped_models_from_registry(
    provider: &ProviderConfig,
    registry: &ModelRegistryListResult,
) -> String {
    let resolved = scoped_model_refs_from_registry(provider, registry)
        .into_iter()
        .map(|model| model.id.clone())
        .collect::<Vec<_>>();
    let mut lines = vec!["scoped models:".to_string()];
    if registry.scope.is_empty() {
        lines.push("  all current provider models (no scope configured)".to_string());
    } else {
        lines.push(format!("  patterns: {}", registry.scope.join(", ")));
        if resolved.is_empty() {
            lines.push("  resolved: none for current provider".to_string());
        } else {
            lines.push(format!("  resolved: {}", resolved.join(", ")));
        }
    }
    lines.push(format!("  current provider: {}", registry.provider));
    lines.push(format!(
        "  registry: {}",
        registry
            .items
            .iter()
            .map(format_model_ref_for_scoped_registry)
            .collect::<Vec<_>>()
            .join(", ")
    ));
    if !registry.diagnostics.is_empty() {
        lines.push("diagnostics:".to_string());
        for diagnostic in &registry.diagnostics {
            lines.push(format!("  - {}", diagnostic.message));
        }
    }
    lines.push("usage: /scoped-models enable <model|provider/model|glob>, /scoped-models disable <pattern>, /scoped-models clear".to_string());
    lines.join("\n").replace(
        &format!("current provider: {}", registry.provider),
        &format!("current provider: {}", provider_name(provider)),
    )
}

fn scoped_model_refs_from_registry<'a>(
    provider: &ProviderConfig,
    registry: &'a ModelRegistryListResult,
) -> Vec<&'a ModelRef> {
    if registry.scope.is_empty() {
        return Vec::new();
    }
    let mut seen = BTreeSet::<String>::new();
    let mut out = Vec::new();
    for pattern in &registry.scope {
        for model in &registry.items {
            if model_matches_scope_pattern(provider, &model.id, pattern)
                && seen.insert(model.id.clone())
            {
                out.push(model);
            }
        }
    }
    out
}

fn format_model_ref_for_scoped_registry(model: &ModelRef) -> String {
    if model.display_name != model.id {
        format!("{} ({})", model.id, model.display_name)
    } else {
        model.id.clone()
    }
}

#[cfg(test)]
pub(super) fn format_thread_tree(threads: &[Thread], active_thread_id: &str) -> String {
    format_thread_tree_with_folded(threads, active_thread_id, &BTreeSet::new())
}

pub(super) fn format_thread_tree_with_filter(
    threads: &[Thread],
    active_thread_id: &str,
    folded_threads: &BTreeSet<String>,
    filter_mode: TreeFilterMode,
) -> String {
    let filtered = threads
        .iter()
        .filter(|thread| tree_thread_visible(thread, active_thread_id, filter_mode))
        .cloned()
        .collect::<Vec<_>>();
    let tree = format_thread_tree_with_folded(&filtered, active_thread_id, folded_threads);
    tree.replacen(
        "threads:",
        &format!("threads:\nfilter: {}", filter_mode.as_str()),
        1,
    )
}

pub(super) fn tree_thread_visible(
    thread: &Thread,
    active_thread_id: &str,
    filter_mode: TreeFilterMode,
) -> bool {
    if thread.id == active_thread_id {
        return true;
    }
    match filter_mode {
        TreeFilterMode::All => true,
        TreeFilterMode::LabeledOnly => thread
            .title
            .as_deref()
            .is_some_and(|title| !title.trim().is_empty()),
        TreeFilterMode::Default | TreeFilterMode::NoTools | TreeFilterMode::UserOnly => {
            thread.status != ThreadStatus::Archived
        }
    }
}

pub(super) fn format_thread_tree_with_folded(
    threads: &[Thread],
    active_thread_id: &str,
    folded_threads: &BTreeSet<String>,
) -> String {
    let mut children: BTreeMap<Option<String>, Vec<&Thread>> = BTreeMap::new();
    for thread in threads {
        children
            .entry(thread.forked_from.clone())
            .or_default()
            .push(thread);
    }
    for group in children.values_mut() {
        group.sort_by(|left, right| left.id.cmp(&right.id));
    }
    let mut lines = vec!["threads:".to_string()];
    let mut visited = BTreeMap::<String, bool>::new();
    fn mark_descendants(
        thread: &Thread,
        children: &BTreeMap<Option<String>, Vec<&Thread>>,
        visited: &mut BTreeMap<String, bool>,
    ) {
        if let Some(kids) = children.get(&Some(thread.id.clone())) {
            for child in kids {
                visited.insert(child.id.clone(), true);
                mark_descendants(child, children, visited);
            }
        }
    }
    fn render(
        thread: &Thread,
        children: &BTreeMap<Option<String>, Vec<&Thread>>,
        active_thread_id: &str,
        depth: usize,
        lines: &mut Vec<String>,
        visited: &mut BTreeMap<String, bool>,
        folded_threads: &BTreeSet<String>,
    ) {
        if visited.insert(thread.id.clone(), true).is_some() {
            return;
        }
        let marker = if thread.id == active_thread_id {
            "*"
        } else {
            " "
        };
        let title = thread.title.as_deref().unwrap_or("untitled");
        let fork = thread
            .forked_from
            .as_deref()
            .map(|parent| format!(" forkedFrom={parent}"))
            .unwrap_or_default();
        let archived = if thread.status == ThreadStatus::Archived {
            " [archived]"
        } else {
            ""
        };
        let child_count = children
            .get(&Some(thread.id.clone()))
            .map(Vec::len)
            .unwrap_or(0);
        let fold = if child_count > 0 && folded_threads.contains(&thread.id) {
            format!(" [+{child_count}]")
        } else if child_count > 0 {
            " [-]".to_string()
        } else {
            String::new()
        };
        lines.push(format!(
            "{}{} {}{} — {}{}{}",
            "  ".repeat(depth),
            marker,
            thread.id,
            fold,
            title,
            archived,
            fork
        ));
        if folded_threads.contains(&thread.id) {
            mark_descendants(thread, children, visited);
        } else if let Some(kids) = children.get(&Some(thread.id.clone())) {
            for child in kids {
                render(
                    child,
                    children,
                    active_thread_id,
                    depth + 1,
                    lines,
                    visited,
                    folded_threads,
                );
            }
        }
    }
    if let Some(roots) = children.get(&None) {
        for root in roots {
            render(
                root,
                &children,
                active_thread_id,
                0,
                &mut lines,
                &mut visited,
                folded_threads,
            );
        }
    }
    for thread in threads {
        if !visited.contains_key(&thread.id) {
            render(
                thread,
                &children,
                active_thread_id,
                0,
                &mut lines,
                &mut visited,
                folded_threads,
            );
        }
    }
    if threads.is_empty() {
        lines.push("  none".to_string());
    }
    lines.join("\n")
}

pub(super) fn exit_resume_command(thread_id: &str) -> String {
    format!("oppi resume {thread_id}")
}

pub(super) fn same_project_cwd(left: &str, right: &str) -> bool {
    let normalize = |value: &str| value.trim().replace('\\', "/");
    let left = normalize(left);
    let right = normalize(right);
    if cfg!(windows) {
        left.eq_ignore_ascii_case(&right)
    } else {
        left == right
    }
}

pub(super) fn format_resume_session_list(
    threads: &[Thread],
    active_thread_id: &str,
    cwd: &str,
    folded_threads: &BTreeSet<String>,
) -> String {
    let project_threads: Vec<Thread> = threads
        .iter()
        .filter(|thread| same_project_cwd(&thread.project.cwd, cwd))
        .cloned()
        .collect();
    let tree = format_thread_tree_with_folded(&project_threads, active_thread_id, folded_threads)
        .replacen("threads:", "recent sessions in this project:", 1);
    let current_thread = if active_thread_id.is_empty() {
        "current thread: none".to_string()
    } else {
        format!("current thread: {active_thread_id}")
    };
    format!("{current_thread}\n{tree}\nnote: use /resume <thread-id> or oppi resume <thread-id>")
}

pub(super) fn normalize_theme_name(raw: &str) -> Option<String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "oppi-cyan-dark" => Some("oppi-cyan".to_string()),
        "oppi" | "oppi-cyan" | "oppi-cyan-light" | "dark" | "light" | "plain" => {
            Some(raw.trim().to_ascii_lowercase())
        }
        _ => None,
    }
}

pub(super) fn load_theme_file() -> Result<Option<String>, String> {
    let path = env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(".oppi")
        .join("theme.txt");
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&path)
        .map_err(|error| format!("read {}: {error}", path.display()))?;
    let theme = raw.lines().next().unwrap_or_default().trim();
    if theme.is_empty() {
        return Ok(None);
    }
    Ok(Some(theme.to_string()))
}

pub(super) fn loaded_shell_theme_from_load_result(
    value: &Value,
    requested_name: &str,
) -> Result<LoadedShellTheme, String> {
    let theme = value.get("theme").cloned().unwrap_or_else(|| json!({}));
    let name = theme
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| !name.trim().is_empty())
        .unwrap_or(requested_name.trim())
        .to_string();
    let source = theme
        .get("source")
        .and_then(Value::as_str)
        .filter(|source| !source.trim().is_empty())
        .unwrap_or("custom")
        .to_string();
    let colors = value
        .get("colors")
        .cloned()
        .ok_or_else(|| "theme/load returned no colors".to_string())?;
    let export = value
        .get("export")
        .filter(|export| !export.is_null())
        .cloned();
    let line_tokens = line_theme_tokens_from_pi_theme_values(&colors)?;
    Ok(LoadedShellTheme {
        name,
        source,
        colors,
        export,
        line_tokens,
    })
}

pub(super) fn normalize_prompt_variant(raw: &str) -> Option<String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "off" | "none" => Some("off".to_string()),
        "a" | "promptname_a" | "prompt-name-a" => Some("a".to_string()),
        "b" | "promptname_b" | "prompt-name-b" => Some("b".to_string()),
        "caveman" => Some("caveman".to_string()),
        _ => None,
    }
}

pub(super) fn prompt_variant_append(variant: &str) -> Option<String> {
    match variant {
        "a" => Some("Prompt variant A: be concise, autonomy-forward, and explicit about OPPi safety surfaces.".to_string()),
        "b" => Some("Prompt variant B: use a more deliberate planning cadence and preserve visible task state.".to_string()),
        "caveman" => Some("Caveman variant: short words, direct actions, no fluff; still follow OPPi safety rules.".to_string()),
        _ => None,
    }
}

pub(super) fn command_prepare_params_json(
    thread_id: &str,
    command: &str,
    args: &str,
    role: &str,
    role_model: Option<String>,
    role_effort: Option<String>,
    prompt_variant_append: Option<String>,
) -> Value {
    json!({
        "threadId": thread_id,
        "command": command.trim_start_matches('/'),
        "args": args,
        "context": {
            "role": role,
            "roleModel": role_model,
            "roleEffort": role_effort,
        },
        "promptVariantAppend": prompt_variant_append,
    })
}

pub(super) fn is_command_prepare_unknown_error(error: &str) -> bool {
    error.contains("command_prepare_unknown") || error.contains("prompt_template_not_found")
}

pub(super) fn theme_load_params_json(thread_id: &str, name: &str) -> Value {
    json!({
        "threadId": thread_id,
        "name": name.trim(),
    })
}

fn prompt_variant_feature_routing_append(variant: &str) -> Option<&'static str> {
    match variant {
        "a" => Some(OPPI_FEATURE_ROUTING_VARIANT_A_APPEND),
        "b" | "caveman" => Some(OPPI_FEATURE_ROUTING_VARIANT_B_APPEND),
        _ => None,
    }
}

pub(super) fn apply_feature_routing_to_provider(provider: &mut Value, variant: &str) {
    append_system_prompt_to_provider_once(
        provider,
        "OPPi feature routing",
        OPPI_FEATURE_ROUTING_SYSTEM_APPEND,
    );
    if let Some(append) = prompt_variant_feature_routing_append(variant) {
        append_system_prompt_to_provider_once(provider, "OPPi feature routing variant", append);
    }
}

pub(super) fn apply_prompt_variant_to_provider(provider: &mut Value, variant: &str) {
    let Some(append) = prompt_variant_append(variant) else {
        return;
    };
    append_system_prompt_to_provider(provider, "OPPi prompt variant", append.as_str());
}

pub(super) fn append_hoppi_memory_context_to_provider(provider: &mut Value, context: &str) {
    append_system_prompt_to_provider_once(provider, HOPPI_MEMORY_SYSTEM_HEADING, context);
}

fn append_system_prompt_to_provider_once(provider: &mut Value, heading: &str, append: &str) {
    let existing = provider
        .get("systemPrompt")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if existing.contains(&format!("{heading}:")) {
        return;
    }
    append_system_prompt_to_provider(provider, heading, append);
}

pub(super) fn append_system_prompt_to_provider(provider: &mut Value, heading: &str, append: &str) {
    let append = append.trim();
    if append.is_empty() {
        return;
    }
    let block = format!("{heading}:\n{append}");
    let system = provider
        .get("systemPrompt")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(|value| format!("{value}\n\n{block}"))
        .unwrap_or(block);
    provider["systemPrompt"] = json!(system);
}

pub(super) fn openai_provider_json(config: &OpenAiCompatibleConfig) -> Value {
    let kind = match config.flavor {
        DirectProviderFlavor::OpenAiCompatible => "openai-compatible",
        DirectProviderFlavor::OpenAiCodex => "openai-codex",
        DirectProviderFlavor::GitHubCopilot => "github-copilot",
    };
    let mut value = json!({
        "kind": kind,
        "model": config.model,
        "stream": config.stream,
    });
    if let Some(base_url) = &config.base_url {
        value["baseUrl"] = json!(base_url);
    }
    if let Some(api_key_env) = &config.api_key_env {
        value["apiKeyEnv"] = json!(api_key_env);
    }
    if let Some(system_prompt) = &config.system_prompt {
        value["systemPrompt"] = json!(system_prompt);
    }
    if let Some(temperature) = config.temperature {
        value["temperature"] = json!(temperature);
    }
    if let Some(reasoning_effort) = &config.reasoning_effort {
        value["reasoningEffort"] = json!(reasoning_effort);
    }
    if let Some(max_output_tokens) = config.max_output_tokens {
        value["maxOutputTokens"] = json!(max_output_tokens);
    }
    value
}

pub(super) fn parse_args(args: Vec<String>) -> Result<ShellCommand, String> {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_help();
        std::process::exit(0);
    }
    let mut prompt_parts = Vec::new();
    let mut server = None;
    let mut resume_thread = None;
    let mut list_sessions = false;
    let mut json = false;
    let mut mock = false;
    let mut provider_requested = false;
    let mut provider_flavor = DirectProviderFlavor::OpenAiCompatible;
    let mut interactive = false;
    let mut raw = default_retained_tui_enabled();
    let mut ratatui = default_ratatui_enabled();
    let mut model: Option<String> = None;
    let mut base_url = None;
    let mut api_key_env = None;
    let mut system_prompt = None;
    let mut temperature = None;
    let mut reasoning_effort = None;
    let mut reasoning_effort_explicit = false;
    let mut max_output_tokens = None;
    let mut stream = true;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--json" => json = true,
            "--mock" => mock = true,
            "--interactive" | "-i" => interactive = true,
            "--raw" => {
                raw = true;
                interactive = true;
            }
            "--ratatui" => {
                ratatui = true;
                raw = true;
                interactive = true;
            }
            "--no-ratatui" => ratatui = false,
            "--line" | "--no-raw" | "--no-tui" => raw = false,
            "--no-stream" => {
                provider_requested = true;
                stream = false;
            }
            "--provider" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| "--provider requires mock or openai-compatible".to_string())?;
                match value.as_str() {
                    "mock" => mock = true,
                    "openai-compatible" | "openai" => provider_requested = true,
                    "openai-codex" | "codex" | "chatgpt" => {
                        provider_requested = true;
                        provider_flavor = DirectProviderFlavor::OpenAiCodex;
                    }
                    "github-copilot" | "copilot" | "github" => {
                        provider_requested = true;
                        provider_flavor = DirectProviderFlavor::GitHubCopilot;
                    }
                    other => return Err(format!("unsupported provider: {other}")),
                }
            }
            "--server" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| "--server requires a path".to_string())?;
                server = Some(PathBuf::from(value));
            }
            "--resume" => {
                index += 1;
                resume_thread = Some(required_option_value(&args, index, "--resume")?);
            }
            "--list-sessions" => list_sessions = true,
            "--model" => {
                index += 1;
                model = Some(required_option_value(&args, index, "--model")?);
                provider_requested = true;
            }
            "--base-url" => {
                index += 1;
                base_url = Some(required_option_value(&args, index, "--base-url")?);
                provider_requested = true;
            }
            "--api-key-env" => {
                index += 1;
                api_key_env = Some(required_option_value(&args, index, "--api-key-env")?);
                provider_requested = true;
            }
            "--system" => {
                index += 1;
                system_prompt = Some(required_option_value(&args, index, "--system")?);
                provider_requested = true;
            }
            "--temperature" => {
                index += 1;
                let raw = required_option_value(&args, index, "--temperature")?;
                temperature = Some(
                    raw.parse::<f32>()
                        .map_err(|error| format!("invalid --temperature: {error}"))?,
                );
                provider_requested = true;
            }
            "--effort" | "--reasoning-effort" => {
                index += 1;
                reasoning_effort = Some(required_option_value(
                    &args,
                    index,
                    args[index - 1].as_str(),
                )?);
                reasoning_effort_explicit = true;
                provider_requested = true;
            }
            "--max-output-tokens" => {
                index += 1;
                let raw = required_option_value(&args, index, "--max-output-tokens")?;
                max_output_tokens = Some(
                    raw.parse::<u32>()
                        .map_err(|error| format!("invalid --max-output-tokens: {error}"))?,
                );
                provider_requested = true;
            }
            arg if arg.starts_with("--server=") => {
                server = Some(PathBuf::from(arg.trim_start_matches("--server=")));
            }
            arg if arg.starts_with("--resume=") => {
                resume_thread = Some(arg.trim_start_matches("--resume=").to_string());
            }
            arg if arg == "--sessions" => list_sessions = true,
            arg if arg.starts_with("--model=") => {
                model = Some(arg.trim_start_matches("--model=").to_string());
                provider_requested = true;
            }
            arg if arg.starts_with("--base-url=") => {
                base_url = Some(arg.trim_start_matches("--base-url=").to_string());
                provider_requested = true;
            }
            arg if arg.starts_with("--api-key-env=") => {
                api_key_env = Some(arg.trim_start_matches("--api-key-env=").to_string());
                provider_requested = true;
            }
            arg if arg.starts_with("--system=") => {
                system_prompt = Some(arg.trim_start_matches("--system=").to_string());
                provider_requested = true;
            }
            arg if arg.starts_with("--effort=") => {
                reasoning_effort = Some(arg.trim_start_matches("--effort=").to_string());
                reasoning_effort_explicit = true;
                provider_requested = true;
            }
            arg if arg.starts_with("--reasoning-effort=") => {
                reasoning_effort = Some(arg.trim_start_matches("--reasoning-effort=").to_string());
                reasoning_effort_explicit = true;
                provider_requested = true;
            }
            arg if arg.starts_with("--max-output-tokens=") => {
                let raw = arg.trim_start_matches("--max-output-tokens=");
                max_output_tokens = Some(
                    raw.parse::<u32>()
                        .map_err(|error| format!("invalid --max-output-tokens: {error}"))?,
                );
                provider_requested = true;
            }
            arg if arg.starts_with("--temperature=") => {
                let raw = arg.trim_start_matches("--temperature=");
                temperature = Some(
                    raw.parse::<f32>()
                        .map_err(|error| format!("invalid --temperature: {error}"))?,
                );
                provider_requested = true;
            }
            arg if arg.starts_with('-') => return Err(format!("unknown option: {arg}")),
            value => prompt_parts.push(value.to_string()),
        }
        index += 1;
    }

    if mock && provider_requested && !list_sessions {
        return Err("choose either --mock or an OpenAI-compatible provider, not both".to_string());
    }

    let can_use_authenticated_default = !provider_requested
        && model.is_none()
        && base_url.is_none()
        && api_key_env.is_none()
        && system_prompt.is_none()
        && temperature.is_none()
        && reasoning_effort.is_none()
        && max_output_tokens.is_none();
    let authenticated_default = can_use_authenticated_default
        .then(default_authenticated_provider_config)
        .flatten();

    let provider = if list_sessions || mock {
        ProviderConfig::Mock
    } else if let Some(provider) = authenticated_default {
        provider
    } else {
        let model = model
            .or_else(default_model_from_env)
            .or_else(|| match provider_flavor {
                DirectProviderFlavor::OpenAiCodex => Some(OPENAI_CODEX_DEFAULT_MODEL.to_string()),
                DirectProviderFlavor::GitHubCopilot => {
                    Some(GITHUB_COPILOT_DEFAULT_MODEL.to_string())
                }
                DirectProviderFlavor::OpenAiCompatible => None,
            });
        if let Some(model) = model {
            ProviderConfig::OpenAiCompatible(with_default_reasoning_effort(
                OpenAiCompatibleConfig {
                    flavor: provider_flavor,
                    provider_id: None,
                    model,
                    base_url,
                    api_key_env: if matches!(
                        provider_flavor,
                        DirectProviderFlavor::OpenAiCodex | DirectProviderFlavor::GitHubCopilot
                    ) {
                        None
                    } else {
                        api_key_env
                    },
                    system_prompt,
                    temperature,
                    reasoning_effort,
                    max_output_tokens,
                    stream,
                },
            ))
        } else {
            return Err(
                "choose --mock for the scripted shell or pass --model/OPPI_RUNTIME_WORKER_MODEL for a real OpenAI-compatible provider"
                    .to_string(),
            );
        }
    };

    let prompt = prompt_parts.join(" ").trim().to_string();
    let initial_prompt = if prompt.is_empty() {
        None
    } else {
        Some(prompt)
    };
    if list_sessions {
        interactive = false;
    } else {
        interactive |= initial_prompt.is_none();
    }
    Ok(ShellCommand {
        initial_prompt,
        server,
        resume_thread,
        list_sessions,
        json,
        interactive,
        raw,
        ratatui,
        reasoning_effort_explicit,
        provider,
    })
}

fn required_option_value(args: &[String], index: usize, option: &str) -> Result<String, String> {
    args.get(index)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("{option} requires a value"))
}

fn default_retained_tui_enabled() -> bool {
    if let Ok(value) = env::var("OPPI_SHELL_RAW").or_else(|_| env::var("OPPI_SHELL_TUI")) {
        return matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        );
    }
    if env::var("OPPI_SHELL_LINE").ok().is_some_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    }) {
        return false;
    }
    io::stdin().is_terminal() && io::stdout().is_terminal()
}

fn default_ratatui_enabled() -> bool {
    if let Ok(value) = env::var("OPPI_SHELL_RATATUI") {
        return !matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "0" | "false" | "no" | "off"
        );
    }
    true
}

pub(super) fn default_model_from_env() -> Option<String> {
    [
        "OPPI_RUNTIME_WORKER_MODEL",
        "OPPI_OPENAI_MODEL",
        "OPENAI_MODEL",
    ]
    .iter()
    .find_map(|name| {
        env::var(name)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    })
}

fn print_help() {
    println!(
        "oppi-shell --mock [--interactive] [--raw|--no-tui] [--ratatui] [--json] [--server <path>] [--resume <thread-id>] [prompt]\n\
         oppi-shell --mock --list-sessions [--json] [--server <path>]\n\
         oppi-shell --model <model> [--provider openai-compatible|openai-codex] [--base-url <url>] [--api-key-env <ENV>] [--resume <thread-id>] [--interactive] [--raw|--no-tui] [--ratatui] [prompt]\n\n\
         Experimental native shell for oppi-server --stdio. In an interactive terminal it opens the Ratatui Rust UI by default: structured layout bands, semantic transcript rows, dock trays, editor, slash palette, settings overlay, and adaptive narrow/tiny breakpoints. Use --no-ratatui for the retained ANSI fallback and --no-tui/--line for plain line mode. Provider/model commands include /login, /provider, /models, /model, /roles, and explicit /provider smoke. Real provider mode uses OpenAI-compatible API env refs or native ChatGPT/Codex OAuth; /login anthropic uses an explicit managed Meridian loopback bridge."
    );
}
