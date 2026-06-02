fn current_provider_model(provider: &ProviderConfig) -> Option<&str> {
    match provider {
        ProviderConfig::Mock => Some(MOCK_MODEL_ID),
        ProviderConfig::OpenAiCompatible(config) => Some(config.model.as_str()),
    }
}

fn provider_name(provider: &ProviderConfig) -> &str {
    match provider {
        ProviderConfig::Mock => "mock",
        ProviderConfig::OpenAiCompatible(config) => match config.flavor {
            DirectProviderFlavor::OpenAiCompatible => {
                config.provider_id.as_deref().unwrap_or("openai-compatible")
            }
            DirectProviderFlavor::OpenAiCodex => "openai-codex",
            DirectProviderFlavor::GitHubCopilot => "github-copilot",
        },
    }
}

fn display_provider_id(provider: &ProviderConfig) -> &str {
    match provider {
        ProviderConfig::Mock => "local-test",
        ProviderConfig::OpenAiCompatible(_) => provider_name(provider),
    }
}

fn with_default_reasoning_effort(mut config: OpenAiCompatibleConfig) -> OpenAiCompatibleConfig {
    if config.reasoning_effort.is_none() {
        config.reasoning_effort = default_reasoning_effort_for_config(&config);
    }
    config
}

fn default_reasoning_effort_for_config(config: &OpenAiCompatibleConfig) -> Option<String> {
    let provider = ProviderConfig::OpenAiCompatible(config.clone());
    let level = if config.model == GPT_MAIN_DEFAULT_MODEL {
        ThinkingLevel::XHigh
    } else if is_anthropic_like_model(&provider, &config.model) {
        ThinkingLevel::High
    } else {
        recommended_effort_level_for_provider(&provider)
    };
    if level == ThinkingLevel::Off
        || !allowed_effort_levels_for_model(&provider, &config.model).contains(&level)
    {
        None
    } else {
        Some(level.as_str().to_string())
    }
}

fn normalize_thinking_level(input: &str) -> Option<ThinkingLevel> {
    match input.trim().to_ascii_lowercase().as_str() {
        "off" | "none" => Some(ThinkingLevel::Off),
        "minimal" | "min" => Some(ThinkingLevel::Minimal),
        "low" => Some(ThinkingLevel::Low),
        "medium" | "med" => Some(ThinkingLevel::Medium),
        "high" => Some(ThinkingLevel::High),
        "xhigh" | "max" => Some(ThinkingLevel::XHigh),
        _ => None,
    }
}

fn normalize_current_thinking_level(value: Option<&str>) -> ThinkingLevel {
    value
        .and_then(normalize_thinking_level)
        .unwrap_or(ThinkingLevel::Off)
}

fn effort_model_name(provider: &ProviderConfig) -> String {
    format!(
        "{}/{}",
        display_provider_id(provider),
        current_provider_model(provider)
            .map(display_model_id)
            .unwrap_or("unknown")
    )
}

fn is_anthropic_like_model(provider: &ProviderConfig, model: &str) -> bool {
    let provider_name = provider_name(provider).to_ascii_lowercase();
    let id = model.to_ascii_lowercase();
    provider_name.contains("anthropic")
        || provider_name == "meridian"
        || provider_uses_meridian(provider)
        || id.contains("claude")
        || id.contains("opus")
        || id.contains("sonnet")
}

fn provider_uses_meridian(provider: &ProviderConfig) -> bool {
    matches!(provider, ProviderConfig::OpenAiCompatible(config) if provider_uses_meridian_placeholder(config))
}

fn supports_xhigh_effort(provider: &ProviderConfig, model: &str) -> bool {
    let id = model.to_ascii_lowercase();
    id.contains("gpt-5.2")
        || id.contains("gpt-5.3")
        || id.contains("gpt-5.4")
        || id.contains("gpt-5.5")
        || id.contains("gpt-5.1-codex-max")
        || id.contains("opus-4-6")
        || id.contains("opus-4.6")
        || id.contains("opus-4-7")
        || id.contains("opus-4.7")
        || (provider_uses_meridian(provider) && (id.contains("opus") || id.contains("sonnet")))
}

fn is_reasoning_capable_model(provider: &ProviderConfig, model: &str) -> bool {
    let provider_label = provider_name(provider).to_ascii_lowercase();
    let id = model.to_ascii_lowercase();
    (provider_label.contains("copilot")
        && (id.contains("gpt-5")
            || id.contains("o3")
            || id.contains("o4")
            || id.contains("claude")
            || id.contains("sonnet")
            || id.contains("opus")
            || id.contains("gemini")))
        || id.contains("gpt-5")
        || id.contains("o3")
        || id.contains("o4")
        || is_anthropic_like_model(provider, model)
        || id.contains("gemini-2.5")
        || id.contains("gemini-3")
}

fn allowed_effort_levels_for_model(provider: &ProviderConfig, model: &str) -> Vec<ThinkingLevel> {
    if !is_reasoning_capable_model(provider, model) {
        return vec![ThinkingLevel::Off];
    }
    ALL_THINKING_LEVELS
        .into_iter()
        .filter(|level| *level != ThinkingLevel::XHigh || supports_xhigh_effort(provider, model))
        .collect()
}

fn allowed_effort_levels_for_provider(provider: &ProviderConfig) -> Vec<ThinkingLevel> {
    let model = current_provider_model(provider).unwrap_or("unknown");
    allowed_effort_levels_for_model(provider, model)
}

fn recommended_effort_level_for_provider(provider: &ProviderConfig) -> ThinkingLevel {
    let model = current_provider_model(provider).unwrap_or("unknown");
    if !is_reasoning_capable_model(provider, model) {
        return ThinkingLevel::Off;
    }
    let id = model.to_ascii_lowercase();
    let provider_label = provider_name(provider).to_ascii_lowercase();
    if id.contains("gpt-5.5") {
        return ThinkingLevel::XHigh;
    }
    if is_anthropic_like_model(provider, model) {
        return ThinkingLevel::High;
    }
    if provider_label == "openai"
        || provider_label.contains("openai")
        || provider_label.contains("copilot")
        || id.contains("gpt-5")
        || id.contains("o3")
        || id.contains("o4")
    {
        return if supports_xhigh_effort(provider, model) {
            ThinkingLevel::High
        } else {
            ThinkingLevel::Medium
        };
    }
    if provider_label.contains("google") || id.contains("gemini") {
        return ThinkingLevel::Medium;
    }
    ThinkingLevel::Medium
}

fn current_effort_level_for_provider(provider: &ProviderConfig) -> ThinkingLevel {
    match provider {
        ProviderConfig::Mock => ThinkingLevel::Off,
        ProviderConfig::OpenAiCompatible(config) => {
            normalize_current_thinking_level(config.reasoning_effort.as_deref())
        }
    }
}

fn effort_level_label_for_provider(
    provider: &ProviderConfig,
    level: ThinkingLevel,
) -> &'static str {
    let model = current_provider_model(provider).unwrap_or("unknown");
    if level == ThinkingLevel::XHigh && is_anthropic_like_model(provider, model) {
        "Max"
    } else {
        level.label()
    }
}

fn set_provider_effort_level(provider: &mut ProviderConfig, level: ThinkingLevel) {
    if let ProviderConfig::OpenAiCompatible(config) = provider {
        config.reasoning_effort = if level == ThinkingLevel::Off {
            None
        } else {
            Some(level.as_str().to_string())
        };
    }
}

fn apply_saved_reasoning_effort(provider: &mut ProviderConfig, path: &Path, explicit_effort: bool) {
    if explicit_effort {
        return;
    }
    let Some(saved_effort) = load_reasoning_effort_setting(path) else {
        return;
    };
    if allowed_effort_levels_for_provider(provider).contains(&saved_effort) {
        set_provider_effort_level(provider, saved_effort);
    }
}

fn format_effort_status(provider: &ProviderConfig) -> String {
    let current = current_effort_level_for_provider(provider);
    let recommended = recommended_effort_level_for_provider(provider);
    let allowed = allowed_effort_levels_for_provider(provider);
    let allowed_text = allowed
        .iter()
        .map(|level| level.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let max = allowed.last().copied().unwrap_or(ThinkingLevel::Off);
    let support = if allowed.as_slice() == [ThinkingLevel::Off] {
        "Current model is not marked reasoning-capable, so the rail is locked to Off.".to_string()
    } else {
        format!(
            "Current model's rail caps at {}.",
            effort_level_label_for_provider(provider, max)
        )
    };
    format!(
        "effort: {} for {}\nrecommended: {}\nallowed: {}\n{}\nuse /effort auto or /effort <level>; saved as Pi defaultThinkingLevel; bare /effort opens the native slider in the TUI",
        effort_level_label_for_provider(provider, current),
        effort_model_name(provider),
        effort_level_label_for_provider(provider, recommended),
        allowed_text,
        support
    )
}

fn effort_status_json(provider: &ProviderConfig) -> Value {
    let current = current_effort_level_for_provider(provider);
    let recommended = recommended_effort_level_for_provider(provider);
    let allowed = allowed_effort_levels_for_provider(provider);
    json!({
        "model": effort_model_name(provider),
        "current": current.as_str(),
        "currentLabel": effort_level_label_for_provider(provider, current),
        "recommended": recommended.as_str(),
        "recommendedLabel": effort_level_label_for_provider(provider, recommended),
        "allowed": allowed.iter().map(|level| level.as_str()).collect::<Vec<_>>(),
        "reasoningCapable": allowed.as_slice() != [ThinkingLevel::Off],
    })
}

fn native_model_catalog_for_provider(provider: &ProviderConfig) -> &'static [&'static str] {
    match provider {
        ProviderConfig::Mock => &[MOCK_MODEL_ID],
        ProviderConfig::OpenAiCompatible(config)
            if config.flavor == DirectProviderFlavor::OpenAiCompatible
                && provider_uses_meridian_placeholder(config) =>
        {
            MERIDIAN_MODEL_CATALOG
        }
        ProviderConfig::OpenAiCompatible(config) => match config.flavor {
            DirectProviderFlavor::OpenAiCompatible => {
                if config.base_url.is_none() {
                    OPENAI_DIRECT_MODEL_CATALOG
                } else {
                    &[]
                }
            }
            DirectProviderFlavor::OpenAiCodex => OPENAI_CODEX_MODEL_CATALOG,
            DirectProviderFlavor::GitHubCopilot => GITHUB_COPILOT_MODEL_CATALOG,
        },
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NativeModelCatalogEntry {
    id: String,
    display_name: String,
    api: Option<String>,
    base_url: Option<String>,
    reasoning: Option<bool>,
    input: Vec<String>,
    cost: Option<Value>,
    context_window: Option<u64>,
    max_tokens: Option<u64>,
    headers: Option<Value>,
    compat: Option<Value>,
}

impl NativeModelCatalogEntry {
    fn new(id: impl Into<String>, display_name: impl Into<String>) -> Self {
        let id = id.into();
        let display_name = display_name.into();
        Self {
            display_name: if display_name.trim().is_empty() {
                id.clone()
            } else {
                display_name
            },
            id,
            api: None,
            base_url: None,
            reasoning: None,
            input: Vec::new(),
            cost: None,
            context_window: None,
            max_tokens: None,
            headers: None,
            compat: None,
        }
    }

    fn apply_models_json_config(&mut self, value: &serde_json::Map<String, Value>) {
        if let Some(name) = value
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|name| !name.is_empty())
        {
            self.display_name = name.to_string();
        }
        if let Some(api) = value
            .get("api")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|api| !api.is_empty())
        {
            self.api = Some(api.to_string());
        }
        if let Some(base_url) = value
            .get("baseUrl")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|base_url| !base_url.is_empty())
        {
            self.base_url = Some(base_url.to_string());
        }
        if let Some(reasoning) = value.get("reasoning").and_then(Value::as_bool) {
            self.reasoning = Some(reasoning);
        }
        if let Some(input) = value.get("input").and_then(Value::as_array) {
            self.input = input
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|input| !input.is_empty())
                .map(ToString::to_string)
                .collect();
        }
        if let Some(context_window) = value.get("contextWindow").and_then(Value::as_u64) {
            self.context_window = Some(context_window);
        }
        if let Some(max_tokens) = value.get("maxTokens").and_then(Value::as_u64) {
            self.max_tokens = Some(max_tokens);
        }
        self.cost = merge_json_object_values(self.cost.take(), value.get("cost"));
        self.headers = merge_json_object_values(self.headers.take(), value.get("headers"));
        self.compat = merge_json_object_values(self.compat.take(), value.get("compat"));
    }
}

fn model_ref_for_provider(provider: &ProviderConfig, model: &str, role: Option<&str>) -> ModelRef {
    ModelRef {
        id: model.to_string(),
        provider: provider_name(provider).to_string(),
        display_name: model.to_string(),
        context_window: None,
        role: role.map(str::to_string),
    }
}

fn merge_json_object_values(base: Option<Value>, override_value: Option<&Value>) -> Option<Value> {
    let Some(override_value) = override_value else {
        return base;
    };
    match (base, override_value) {
        (Some(Value::Object(mut base)), Value::Object(override_object)) => {
            for (key, value) in override_object {
                let existing = base.remove(key);
                if matches!(
                    (&existing, value),
                    (Some(Value::Object(_)), Value::Object(_))
                ) {
                    base.insert(
                        key.clone(),
                        merge_json_object_values(existing, Some(value))?,
                    );
                } else {
                    base.insert(key.clone(), value.clone());
                }
            }
            Some(Value::Object(base))
        }
        (_, Value::Object(_)) => Some(override_value.clone()),
        (base, _) => base,
    }
}

fn models_json_path() -> PathBuf {
    env::var_os("OPPI_MODELS_JSON_PATH")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| default_agent_dir().join("models.json"))
}

fn provider_aliases_for_models_json(provider: &ProviderConfig) -> Vec<String> {
    match provider {
        ProviderConfig::Mock => vec!["mock".to_string()],
        ProviderConfig::OpenAiCompatible(config)
            if config.flavor == DirectProviderFlavor::OpenAiCompatible
                && config.provider_id.is_some() =>
        {
            vec![config.provider_id.clone().unwrap_or_default()]
        }
        ProviderConfig::OpenAiCompatible(config)
            if config.flavor == DirectProviderFlavor::OpenAiCompatible
                && provider_uses_meridian_placeholder(config) =>
        {
            vec!["meridian".to_string(), "anthropic".to_string()]
        }
        ProviderConfig::OpenAiCompatible(config) => match config.flavor {
            DirectProviderFlavor::OpenAiCompatible => {
                vec!["openai-compatible".to_string(), "openai".to_string()]
            }
            DirectProviderFlavor::OpenAiCodex => vec!["openai-codex".to_string()],
            DirectProviderFlavor::GitHubCopilot => vec!["github-copilot".to_string()],
        },
    }
}

fn native_models_json_builtin_model_ids(provider_name: &str) -> Option<Vec<&'static str>> {
    match provider_name {
        "mock" => Some(vec![MOCK_MODEL_ID]),
        "openai-compatible" | "openai" => Some(OPENAI_DIRECT_MODEL_CATALOG.to_vec()),
        "openai-codex" => Some(OPENAI_CODEX_MODEL_CATALOG.to_vec()),
        "github-copilot" => Some(GITHUB_COPILOT_MODEL_CATALOG.to_vec()),
        "anthropic" | "meridian" => Some(MERIDIAN_MODEL_CATALOG.to_vec()),
        _ => None,
    }
}

fn native_models_json_api_supported(api: &str) -> bool {
    matches!(api, "openai-completions")
}

fn models_json_registry_diagnostics(
    path: &Path,
    providers: &serde_json::Map<String, Value>,
) -> Vec<String> {
    let mut diagnostics = Vec::<String>::new();
    for (provider_name, provider_value) in providers {
        let Some(provider_object) = provider_value.as_object() else {
            diagnostics.push(format!(
                "models.json schema error at {}: provider {provider_name} must be an object",
                path.display()
            ));
            continue;
        };
        if let Some(api) = provider_object.get("api").and_then(Value::as_str) {
            let api = api.trim();
            if !api.is_empty() && !native_models_json_api_supported(api) {
                diagnostics.push(format!(
                    "models.json unsupported provider API at {}: provider {provider_name} api {api} is not yet supported by Rust-native OPPi",
                    path.display()
                ));
            }
        }
        if let Some(models) = provider_object.get("models").and_then(Value::as_array) {
            if native_models_json_builtin_model_ids(provider_name).is_none()
                && !models.is_empty()
                && provider_object.get("apiKey").is_none()
            {
                diagnostics.push(format!(
                    "models.json auth diagnostic at {}: provider {provider_name} apiKey is required when defining custom models",
                    path.display()
                ));
            }
            for model in models {
                let model_id = model
                    .get("id")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|id| !id.is_empty())
                    .unwrap_or("<unknown>");
                if let Some(api) = model.get("api").and_then(Value::as_str) {
                    let api = api.trim();
                    if !api.is_empty() && !native_models_json_api_supported(api) {
                        diagnostics.push(format!(
                            "models.json unsupported provider API at {}: provider {provider_name} model {model_id} api {api} is not yet supported by Rust-native OPPi",
                            path.display()
                        ));
                    }
                }
            }
        }
        if let Some(model_overrides) = provider_object
            .get("modelOverrides")
            .and_then(Value::as_object)
        {
            let known_ids = native_models_json_builtin_model_ids(provider_name);
            for model_id in model_overrides.keys() {
                let known = known_ids
                    .as_ref()
                    .is_some_and(|ids| ids.iter().any(|known| *known == model_id));
                if !known {
                    diagnostics.push(format!(
                        "models.json ignored modelOverrides at {}: provider {provider_name} has no built-in model id {model_id}",
                        path.display()
                    ));
                }
            }
        }
    }
    diagnostics
}

fn models_json_providers_from_path(
    path: &Path,
) -> Result<Option<serde_json::Map<String, Value>>, Vec<String>> {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(vec![format!(
                "models.json read error at {}: {error}",
                path.display()
            )]);
        }
    };
    let value = match serde_json::from_str::<Value>(&raw) {
        Ok(value) => value,
        Err(error) => {
            return Err(vec![format!(
                "models.json parse error at {}: {error}",
                path.display()
            )]);
        }
    };
    let Some(providers) = value.get("providers").and_then(Value::as_object) else {
        return Err(vec![format!(
            "models.json schema error at {}: expected a providers object",
            path.display()
        )]);
    };
    Ok(Some(providers.clone()))
}

fn apply_models_json_merge_from_path(
    provider: &ProviderConfig,
    path: &Path,
    out: &mut Vec<NativeModelCatalogEntry>,
) -> Vec<String> {
    let providers = match models_json_providers_from_path(path) {
        Ok(Some(providers)) => providers,
        Ok(None) => return Vec::new(),
        Err(diagnostics) => return diagnostics,
    };
    let aliases = provider_aliases_for_models_json(provider);
    let mut diagnostics = models_json_registry_diagnostics(path, &providers);
    for alias in aliases {
        let Some(provider_value) = providers.get(&alias) else {
            continue;
        };
        let Some(provider_object) = provider_value.as_object() else {
            continue;
        };
        for entry in out.iter_mut() {
            entry.apply_models_json_config(provider_object);
        }
        if let Some(model_overrides) = provider_object
            .get("modelOverrides")
            .and_then(Value::as_object)
        {
            for (model_id, override_value) in model_overrides {
                let Some(override_object) = override_value.as_object() else {
                    diagnostics.push(format!(
                        "models.json schema error at {}: provider {alias} modelOverrides.{model_id} must be an object",
                        path.display()
                    ));
                    continue;
                };
                if let Some(existing) = out.iter_mut().find(|existing| existing.id == *model_id) {
                    existing.apply_models_json_config(override_object);
                }
            }
        }
        if let Some(models_value) = provider_object.get("models") {
            let Some(models) = models_value.as_array() else {
                diagnostics.push(format!(
                    "models.json schema error at {}: provider {alias} models must be an array",
                    path.display()
                ));
                continue;
            };
            for model in models {
                let Some(model_object) = model.as_object() else {
                    diagnostics.push(format!(
                        "models.json schema error at {}: provider {alias} model entries must be objects",
                        path.display()
                    ));
                    continue;
                };
                let Some(id) = model_object
                    .get("id")
                    .and_then(Value::as_str)
                    .map(str::trim)
                else {
                    diagnostics.push(format!(
                        "models.json schema error at {}: provider {alias} model entry is missing id",
                        path.display()
                    ));
                    continue;
                };
                if id.is_empty() {
                    diagnostics.push(format!(
                        "models.json schema error at {}: provider {alias} model entry has an empty id",
                        path.display()
                    ));
                    continue;
                }
                let display_name = model_object
                    .get("name")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|name| !name.is_empty())
                    .unwrap_or(id);
                let mut entry = NativeModelCatalogEntry::new(id, display_name);
                entry.apply_models_json_config(provider_object);
                entry.apply_models_json_config(model_object);
                if let Some(existing) = out.iter_mut().find(|existing| existing.id == entry.id) {
                    *existing = entry;
                } else {
                    out.push(entry);
                }
            }
        }
    }
    diagnostics
}

fn custom_model_entries_from_models_json_path(
    provider: &ProviderConfig,
    path: &Path,
) -> Vec<NativeModelCatalogEntry> {
    custom_model_entries_from_models_json_path_with_diagnostics(provider, path).0
}

fn custom_model_entries_from_models_json_path_with_diagnostics(
    provider: &ProviderConfig,
    path: &Path,
) -> (Vec<NativeModelCatalogEntry>, Vec<String>) {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return (Vec::new(), Vec::new()),
        Err(error) => {
            return (
                Vec::new(),
                vec![format!(
                    "models.json read error at {}: {error}",
                    path.display()
                )],
            );
        }
    };
    let value = match serde_json::from_str::<Value>(&raw) {
        Ok(value) => value,
        Err(error) => {
            return (
                Vec::new(),
                vec![format!(
                    "models.json parse error at {}: {error}",
                    path.display()
                )],
            );
        }
    };
    let Some(providers) = value.get("providers").and_then(Value::as_object) else {
        return (
            Vec::new(),
            vec![format!(
                "models.json schema error at {}: expected a providers object",
                path.display()
            )],
        );
    };
    let aliases = provider_aliases_for_models_json(provider);
    let mut out = Vec::<NativeModelCatalogEntry>::new();
    let mut diagnostics = models_json_registry_diagnostics(path, providers);
    for alias in aliases {
        let Some(provider_value) = providers.get(&alias) else {
            continue;
        };
        let Some(provider_object) = provider_value.as_object() else {
            diagnostics.push(format!(
                "models.json schema error at {}: provider {alias} must be an object",
                path.display()
            ));
            continue;
        };
        let Some(models_value) = provider_object.get("models") else {
            continue;
        };
        let Some(models) = models_value.as_array() else {
            diagnostics.push(format!(
                "models.json schema error at {}: provider {alias} models must be an array",
                path.display()
            ));
            continue;
        };
        for model in models {
            let Some(model_object) = model.as_object() else {
                diagnostics.push(format!(
                    "models.json schema error at {}: provider {alias} model entries must be objects",
                    path.display()
                ));
                continue;
            };
            let Some(id) = model_object
                .get("id")
                .and_then(Value::as_str)
                .map(str::trim)
            else {
                diagnostics.push(format!(
                    "models.json schema error at {}: provider {alias} model entry is missing id",
                    path.display()
                ));
                continue;
            };
            if id.is_empty() {
                diagnostics.push(format!(
                    "models.json schema error at {}: provider {alias} model entry has an empty id",
                    path.display()
                ));
                continue;
            }
            let display_name = model_object
                .get("name")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .unwrap_or(id);
            if !out.iter().any(|existing| existing.id == id) {
                out.push(NativeModelCatalogEntry::new(id, display_name));
            }
        }
    }
    (out, diagnostics)
}

fn custom_model_entries_from_models_json(
    provider: &ProviderConfig,
) -> Vec<NativeModelCatalogEntry> {
    custom_model_entries_from_models_json_path(provider, &models_json_path())
}

fn native_model_entries_for_provider_at_path(
    provider: &ProviderConfig,
    models_path: &Path,
) -> Vec<NativeModelCatalogEntry> {
    native_model_entries_for_provider_at_path_with_diagnostics(provider, models_path).0
}

fn native_model_entries_for_provider_at_path_with_diagnostics(
    provider: &ProviderConfig,
    models_path: &Path,
) -> (Vec<NativeModelCatalogEntry>, Vec<String>) {
    let mut seen = BTreeSet::<String>::new();
    let mut out = Vec::<NativeModelCatalogEntry>::new();
    for model in native_model_catalog_for_provider(provider) {
        if seen.insert((*model).to_string()) {
            out.push(NativeModelCatalogEntry::new(*model, *model));
        }
    }
    let diagnostics = apply_models_json_merge_from_path(provider, models_path, &mut out);
    (out, diagnostics)
}

fn native_model_entries_for_provider(provider: &ProviderConfig) -> Vec<NativeModelCatalogEntry> {
    native_model_entries_for_provider_with_diagnostics(provider).0
}

fn native_model_entries_for_provider_with_diagnostics(
    provider: &ProviderConfig,
) -> (Vec<NativeModelCatalogEntry>, Vec<String>) {
    native_model_entries_for_provider_at_path_with_diagnostics(provider, &models_json_path())
}

fn models_json_provider_request_auth_configured_at(
    provider: &ProviderConfig,
    models_path: &Path,
) -> bool {
    let Ok(Some(providers)) = models_json_providers_from_path(models_path) else {
        return false;
    };
    provider_aliases_for_models_json(provider)
        .into_iter()
        .filter_map(|alias| providers.get(&alias).and_then(Value::as_object))
        .any(|provider_object| {
            provider_object
                .get("apiKey")
                .and_then(Value::as_str)
                .map(str::trim)
                .is_some_and(|api_key| !api_key.is_empty())
        })
}

fn models_json_string_field<'a>(
    object: &'a serde_json::Map<String, Value>,
    key: &str,
) -> Option<&'a str> {
    object
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn models_json_bool_compat_field(
    object: &serde_json::Map<String, Value>,
    key: &str,
) -> Option<bool> {
    object
        .get("compat")
        .and_then(Value::as_object)
        .and_then(|compat| compat.get(key))
        .and_then(Value::as_bool)
}

fn models_json_supports_reasoning_effort(
    provider_object: &serde_json::Map<String, Value>,
    model_object: &serde_json::Map<String, Value>,
) -> bool {
    models_json_bool_compat_field(model_object, "supportsReasoningEffort")
        .or_else(|| models_json_bool_compat_field(provider_object, "supportsReasoningEffort"))
        .unwrap_or(true)
}

fn models_json_u32_field(object: &serde_json::Map<String, Value>, key: &str) -> Option<u32> {
    object
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
}

fn custom_provider_config_for_model_at_path(
    selector: &str,
    models_path: &Path,
) -> Result<ProviderConfig, String> {
    let (provider_id, model_id) = selector.split_once('/').ok_or_else(|| {
        "custom provider model selection requires provider/model syntax".to_string()
    })?;
    let provider_id = provider_id.trim();
    let model_id = model_id.trim();
    if provider_id.is_empty() || model_id.is_empty() {
        return Err("custom provider model selection requires provider/model syntax".to_string());
    }
    if native_models_json_builtin_model_ids(provider_id).is_some() {
        return Err(format!(
            "provider {provider_id} is a built-in native provider; select its model id directly"
        ));
    }

    let providers = models_json_providers_from_path(models_path)
        .map_err(|diagnostics| diagnostics.join("\n"))?
        .ok_or_else(|| {
            format!(
                "models.json custom provider {provider_id} was not found at {}",
                models_path.display()
            )
        })?;
    let provider_object = providers
        .get(provider_id)
        .and_then(Value::as_object)
        .ok_or_else(|| {
            format!(
                "models.json custom provider {provider_id} was not found at {}",
                models_path.display()
            )
        })?;
    let models = provider_object
        .get("models")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            format!("models.json provider {provider_id} must define a models array for selection")
        })?;
    let model_object = models
        .iter()
        .filter_map(Value::as_object)
        .find(|model| models_json_string_field(model, "id") == Some(model_id))
        .ok_or_else(|| format!("models.json provider {provider_id} has no model id {model_id}"))?;

    let api = models_json_string_field(model_object, "api")
        .or_else(|| models_json_string_field(provider_object, "api"))
        .ok_or_else(|| {
            format!("models.json provider {provider_id} model {model_id} is missing api")
        })?;
    if api != "openai-completions" {
        return Err(format!(
            "models.json provider {provider_id} model {model_id} uses unsupported api {api}; Rust-native OPPi can select only openai-completions custom providers today"
        ));
    }
    let base_url = models_json_string_field(model_object, "baseUrl")
        .or_else(|| models_json_string_field(provider_object, "baseUrl"))
        .ok_or_else(|| {
            format!("models.json provider {provider_id} model {model_id} is missing baseUrl")
        })?;
    let api_key = models_json_string_field(provider_object, "apiKey").ok_or_else(|| {
        format!("models.json provider {provider_id} model {model_id} is missing apiKey")
    })?;
    if api_key.starts_with('!') {
        return Err(format!(
            "models.json provider {provider_id} apiKey is command-backed; Rust-native custom provider selection requires a safe environment variable reference"
        ));
    }
    if !is_safe_api_key_env_name(api_key) {
        return Err(format!(
            "models.json provider {provider_id} apiKey must be a safe environment variable reference; raw or literal keys are not passed into Rust-native provider config"
        ));
    }

    let mut config = OpenAiCompatibleConfig {
        flavor: DirectProviderFlavor::OpenAiCompatible,
        provider_id: Some(provider_id.to_string()),
        model: model_id.to_string(),
        base_url: Some(base_url.to_string()),
        api_key_env: Some(api_key.to_string()),
        system_prompt: None,
        temperature: None,
        reasoning_effort: None,
        max_output_tokens: models_json_u32_field(model_object, "maxTokens"),
        stream: true,
    };
    if models_json_supports_reasoning_effort(provider_object, model_object) {
        config = with_default_reasoning_effort(config);
    }
    Ok(ProviderConfig::OpenAiCompatible(config))
}

fn native_provider_auth_configured_at_paths(
    provider: &ProviderConfig,
    models_path: &Path,
    codex_auth_path: &Path,
    copilot_auth_path: &Path,
) -> bool {
    if models_json_provider_request_auth_configured_at(provider, models_path) {
        return true;
    }
    match provider {
        ProviderConfig::Mock => true,
        ProviderConfig::OpenAiCompatible(config) => match config.flavor {
            DirectProviderFlavor::OpenAiCodex => codex_auth_present_at(codex_auth_path),
            DirectProviderFlavor::GitHubCopilot => {
                github_copilot_auth_present_at(copilot_auth_path)
            }
            DirectProviderFlavor::OpenAiCompatible => {
                configured_api_key_env(config).is_some()
                    || provider_uses_meridian_placeholder(config)
            }
        },
    }
}

fn native_provider_auth_configured(provider: &ProviderConfig, models_path: &Path) -> bool {
    native_provider_auth_configured_at_paths(
        provider,
        models_path,
        &auth_store_path(),
        &github_copilot_auth_store_path(),
    )
}

fn native_model_ids_for_provider(provider: &ProviderConfig) -> Vec<String> {
    native_model_entries_for_provider(provider)
        .into_iter()
        .map(|model| model.id)
        .collect()
}

fn native_model_refs_for_provider_at_path(
    provider: &ProviderConfig,
    models_path: &Path,
) -> Vec<ModelRef> {
    native_model_refs_for_provider_at_path_with_diagnostics(provider, models_path).0
}

fn native_model_refs_for_provider_at_path_with_diagnostics(
    provider: &ProviderConfig,
    models_path: &Path,
) -> (Vec<ModelRef>, Vec<String>) {
    let (entries, diagnostics) =
        native_model_entries_for_provider_at_path_with_diagnostics(provider, models_path);
    let refs = entries
        .into_iter()
        .map(|model| ModelRef {
            id: model.id,
            provider: provider_name(provider).to_string(),
            display_name: model.display_name,
            context_window: model.context_window,
            role: None,
        })
        .collect();
    (refs, diagnostics)
}

fn thinking_suffix_stripped(pattern: &str) -> &str {
    let Some((prefix, suffix)) = pattern.rsplit_once(':') else {
        return pattern;
    };
    match suffix {
        "off" | "minimal" | "low" | "medium" | "high" | "xhigh" => prefix,
        _ => pattern,
    }
}

fn wildcard_match(pattern: &str, value: &str) -> bool {
    fn inner(pattern: &[u8], value: &[u8]) -> bool {
        match pattern.split_first() {
            None => value.is_empty(),
            Some((&b'*', rest)) => {
                inner(rest, value) || (!value.is_empty() && inner(pattern, &value[1..]))
            }
            Some((&b'?', rest)) => !value.is_empty() && inner(rest, &value[1..]),
            Some((&p, rest)) => {
                !value.is_empty() && p.eq_ignore_ascii_case(&value[0]) && inner(rest, &value[1..])
            }
        }
    }
    inner(pattern.as_bytes(), value.as_bytes())
}

fn model_matches_scope_pattern(provider: &ProviderConfig, model: &str, pattern: &str) -> bool {
    let pattern = thinking_suffix_stripped(pattern.trim());
    if pattern.is_empty() {
        return false;
    }
    let model_lower = model.to_ascii_lowercase();
    let canonicals = provider_aliases_for_models_json(provider)
        .into_iter()
        .map(|provider| format!("{}/{model_lower}", provider.to_ascii_lowercase()))
        .collect::<Vec<_>>();
    let pattern_lower = pattern.to_ascii_lowercase();
    if pattern_lower.contains('*') || pattern_lower.contains('?') {
        wildcard_match(&pattern_lower, &model_lower)
            || canonicals
                .iter()
                .any(|canonical| wildcard_match(&pattern_lower, canonical))
    } else {
        pattern_lower == model_lower
            || canonicals
                .iter()
                .any(|canonical| pattern_lower == *canonical)
    }
}

fn scoped_model_ids_for_patterns(patterns: &[String], provider: &ProviderConfig) -> Vec<String> {
    if patterns.is_empty() {
        return Vec::new();
    }
    let catalog = native_model_ids_for_provider(provider);
    let mut seen = BTreeSet::<String>::new();
    let mut out = Vec::new();
    for pattern in patterns {
        for model in &catalog {
            if model_matches_scope_pattern(provider, model, pattern) && seen.insert(model.clone()) {
                out.push(model.clone());
            }
        }
    }
    out
}

fn scoped_model_ids_for_provider(session: &ShellSession, provider: &ProviderConfig) -> Vec<String> {
    scoped_model_ids_for_patterns(&session.scoped_model_ids, provider)
}

fn main_model_ids_for_selection(
    current: Option<&str>,
    scoped_patterns: &[String],
    provider: &ProviderConfig,
) -> Vec<String> {
    let entries = native_model_entries_for_provider(provider);
    main_model_ids_for_selection_from_entries(current, scoped_patterns, provider, &entries)
}

fn main_model_ids_for_selection_from_entries(
    current: Option<&str>,
    scoped_patterns: &[String],
    provider: &ProviderConfig,
    entries: &[NativeModelCatalogEntry],
) -> Vec<String> {
    let mut seen = BTreeSet::<String>::new();
    let mut out = Vec::<String>::new();
    let mut push_model = |model: &str| {
        let model = model.trim();
        if !model.is_empty() && seen.insert(model.to_string()) {
            out.push(model.to_string());
        }
    };
    if let Some(current) = current {
        push_model(current);
    }
    if scoped_patterns.is_empty() {
        for model in entries {
            push_model(&model.id);
        }
        return out;
    }
    let mut scoped_resolved = false;
    for pattern in scoped_patterns {
        for model in entries {
            if model_matches_scope_pattern(provider, &model.id, pattern) {
                scoped_resolved = true;
                push_model(&model.id);
            }
        }
    }
    if !scoped_resolved {
        for model in entries {
            push_model(&model.id);
        }
    }
    out
}

fn main_model_refs_from_entries(
    current: Option<&str>,
    scoped_patterns: &[String],
    provider: &ProviderConfig,
    entries: &[NativeModelCatalogEntry],
) -> Vec<ModelRef> {
    let display_names = entries
        .iter()
        .map(|model| (model.id.clone(), model.display_name.clone()))
        .collect::<BTreeMap<_, _>>();
    main_model_ids_for_selection_from_entries(current, scoped_patterns, provider, entries)
        .into_iter()
        .map(|model| {
            let context_window = entries
                .iter()
                .find(|entry| entry.id == model)
                .and_then(|entry| entry.context_window);
            ModelRef {
                display_name: display_names
                    .get(&model)
                    .cloned()
                    .unwrap_or_else(|| model.clone()),
                id: model,
                provider: provider_name(provider).to_string(),
                context_window,
                role: None,
            }
        })
        .collect()
}

fn main_model_refs_for_provider_at_path_with_auth_paths(
    current: Option<&str>,
    scoped_patterns: &[String],
    provider: &ProviderConfig,
    models_path: &Path,
    codex_auth_path: &Path,
    copilot_auth_path: &Path,
    available_only: bool,
) -> (Vec<ModelRef>, Vec<String>) {
    let (entries, diagnostics) =
        native_model_entries_for_provider_at_path_with_diagnostics(provider, models_path);
    if available_only
        && !native_provider_auth_configured_at_paths(
            provider,
            models_path,
            codex_auth_path,
            copilot_auth_path,
        )
    {
        return (Vec::new(), diagnostics);
    }
    (
        main_model_refs_from_entries(current, scoped_patterns, provider, &entries),
        diagnostics,
    )
}

fn main_model_refs_for_provider_with_diagnostics(
    session: &ShellSession,
    provider: &ProviderConfig,
    available_only: bool,
) -> (Vec<ModelRef>, Vec<String>) {
    // Match Pi selector behavior: current selection first, then either the
    // enabled/scoped order or the current provider catalog order. Do not use
    // historical known_model_ids here; those include stale prior selections and
    // role-only overrides, which is how old models leaked into the main selector.
    let models_path = models_json_path();
    main_model_refs_for_provider_at_path_with_auth_paths(
        session.session_model(provider),
        &session.scoped_model_ids,
        provider,
        &models_path,
        &auth_store_path(),
        &github_copilot_auth_store_path(),
        available_only,
    )
}

fn main_model_refs_for_provider(
    session: &ShellSession,
    provider: &ProviderConfig,
) -> Vec<ModelRef> {
    main_model_refs_for_provider_with_diagnostics(session, provider, true).0
}

fn main_model_ids_for_provider(session: &ShellSession, provider: &ProviderConfig) -> Vec<String> {
    session
        .cached_model_ids_for_provider(provider, true)
        .unwrap_or_default()
}

fn provider_api_key_env(config: &OpenAiCompatibleConfig) -> &str {
    if config.flavor == DirectProviderFlavor::OpenAiCodex {
        return "Pi/OPPi auth store (openai-codex)";
    }
    if config.flavor == DirectProviderFlavor::GitHubCopilot {
        return "Pi/OPPi auth store (github-copilot)";
    }
    config
        .api_key_env
        .as_deref()
        .unwrap_or("OPPI_OPENAI_API_KEY or OPENAI_API_KEY")
}

fn configured_api_key_env(config: &OpenAiCompatibleConfig) -> Option<String> {
    if let Some(name) = config.api_key_env.as_deref() {
        return env::var(name)
            .ok()
            .filter(|value| !value.trim().is_empty())
            .map(|_| name.to_string());
    }
    if env::var("OPPI_OPENAI_API_KEY")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .is_some()
    {
        return Some("OPPI_OPENAI_API_KEY".to_string());
    }
    if env::var("OPENAI_API_KEY")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .is_some()
    {
        return Some("OPENAI_API_KEY".to_string());
    }
    None
}

fn provider_auth_present(config: &OpenAiCompatibleConfig) -> bool {
    if config.flavor == DirectProviderFlavor::OpenAiCodex {
        return codex_auth_present_at(&auth_store_path());
    }
    if config.flavor == DirectProviderFlavor::GitHubCopilot {
        return github_copilot_auth_present_at(&github_copilot_auth_store_path());
    }
    configured_api_key_env(config).is_some() || provider_uses_meridian_placeholder(config)
}

fn provider_auth_status_label(config: &OpenAiCompatibleConfig) -> &'static str {
    if config.flavor == DirectProviderFlavor::OpenAiCodex {
        return if codex_auth_present_at(&auth_store_path()) {
            "stored oauth"
        } else {
            "missing"
        };
    }
    if config.flavor == DirectProviderFlavor::GitHubCopilot {
        return if github_copilot_auth_present_at(&github_copilot_auth_store_path()) {
            "stored oauth"
        } else {
            "missing"
        };
    }
    if configured_api_key_env(config).is_some() {
        "present"
    } else if provider_uses_meridian_placeholder(config) {
        "loopback placeholder"
    } else {
        "missing"
    }
}

fn default_authenticated_provider_config() -> Option<ProviderConfig> {
    let codex_present = codex_auth_present_at(&auth_store_path());
    let copilot_auth = read_github_copilot_auth_at(&github_copilot_auth_store_path()).ok();
    default_authenticated_provider_config_from_state(
        codex_present,
        copilot_auth.as_ref(),
        meridian_reachable(),
    )
}

fn default_authenticated_provider_config_from_state(
    codex_present: bool,
    copilot_auth: Option<&GitHubCopilotStoredAuth>,
    claude_bridge_reachable: bool,
) -> Option<ProviderConfig> {
    if codex_present {
        return Some(ProviderConfig::OpenAiCompatible(
            with_default_reasoning_effort(OpenAiCompatibleConfig {
                flavor: DirectProviderFlavor::OpenAiCodex,
                provider_id: None,
                model: OPENAI_CODEX_DEFAULT_MODEL.to_string(),
                base_url: None,
                api_key_env: None,
                system_prompt: None,
                temperature: None,
                reasoning_effort: None,
                max_output_tokens: None,
                stream: true,
            }),
        ));
    }

    if let Some(auth) = copilot_auth {
        return Some(ProviderConfig::OpenAiCompatible(
            with_default_reasoning_effort(OpenAiCompatibleConfig {
                flavor: DirectProviderFlavor::GitHubCopilot,
                provider_id: None,
                model: GITHUB_COPILOT_DEFAULT_MODEL.to_string(),
                base_url: Some(github_copilot_base_url(auth)),
                api_key_env: None,
                system_prompt: None,
                temperature: None,
                reasoning_effort: None,
                max_output_tokens: None,
                stream: true,
            }),
        ));
    }

    if claude_bridge_reachable {
        return Some(ProviderConfig::OpenAiCompatible(meridian_provider_config(
            None,
        )));
    }

    None
}

fn is_safe_api_key_env_name(name: &str) -> bool {
    if name.is_empty() || name.len() > 128 {
        return false;
    }
    if !name
        .chars()
        .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_')
    {
        return false;
    }
    name == "OPPI_OPENAI_API_KEY"
        || name == "OPENAI_API_KEY"
        || (name.starts_with("OPPI_") && name.ends_with("_API_KEY"))
        || (name.starts_with("OPENAI_") && name.ends_with("_API_KEY"))
        || (name.starts_with("AZURE_OPENAI_") && name.ends_with("_API_KEY"))
}

fn meridian_base_url() -> String {
    env::var("OPPI_MERIDIAN_BASE_URL")
        .ok()
        .or_else(|| env::var("MERIDIAN_BASE_URL").ok())
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| MERIDIAN_DEFAULT_BASE_URL.to_string())
}

fn meridian_package_spec() -> String {
    env::var("OPPI_MERIDIAN_PACKAGE_SPEC")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| format!("{MERIDIAN_PACKAGE_NAME}@latest"))
}

fn hoppi_package_spec() -> String {
    env::var("OPPI_HOPPI_PACKAGE_SPEC")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| format!("{HOPPI_PACKAGE_NAME}@^0.1.0"))
}

fn oppi_home_dir() -> PathBuf {
    env::var_os("OPPI_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".oppi")
        })
}

fn managed_packages_dir() -> PathBuf {
    oppi_home_dir().join("packages")
}

fn meridian_bin_name() -> String {
    format!("meridian{}", if cfg!(windows) { ".cmd" } else { "" })
}

fn managed_meridian_bin_path() -> PathBuf {
    managed_packages_dir()
        .join("node_modules")
        .join(".bin")
        .join(meridian_bin_name())
}

fn local_meridian_bin_path() -> PathBuf {
    env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("node_modules")
        .join(".bin")
        .join(meridian_bin_name())
}

fn managed_meridian_module_path() -> PathBuf {
    managed_packages_dir()
        .join("node_modules")
        .join("@rynfar")
        .join("meridian")
        .join("dist")
        .join("server.js")
}

fn managed_hoppi_module_path_for(package_name: &str) -> PathBuf {
    let mut path = managed_packages_dir().join("node_modules");
    for part in package_name.split('/') {
        path.push(part);
    }
    path.join("dist").join("index.js")
}

fn managed_hoppi_module_path() -> PathBuf {
    managed_hoppi_module_path_for(HOPPI_PACKAGE_NAME)
}

fn resolve_optional_module_path(path: PathBuf) -> PathBuf {
    let text = path.display().to_string();
    if let Some(stripped) = text.strip_prefix("~/").or_else(|| text.strip_prefix("~\\")) {
        return home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(stripped);
    }
    if path.is_absolute() {
        path
    } else {
        env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    }
}

fn local_sibling_hoppi_module_path() -> Option<PathBuf> {
    env::current_dir()
        .ok()
        .and_then(|cwd| cwd.parent().map(Path::to_path_buf))
        .map(|parent| parent.join("hoppi-memory").join("dist").join("index.js"))
}

fn hoppi_candidate_module_paths_for_startup_offer() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(path) = env::var_os("OPPI_HOPPI_MODULE")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .map(resolve_optional_module_path)
    {
        paths.push(path);
    }
    paths.push(managed_hoppi_module_path_for(HOPPI_PACKAGE_NAME));
    paths.push(managed_hoppi_module_path_for(HOPPI_LEGACY_PACKAGE_NAME));
    if let Some(path) = local_sibling_hoppi_module_path() {
        paths.push(path);
    }
    paths
}

fn hoppi_backend_available_for_startup_offer() -> bool {
    hoppi_candidate_module_paths_for_startup_offer()
        .into_iter()
        .any(|path| path.exists())
}

fn should_offer_hoppi_first_start_install(
    settings: &MemorySettings,
    backend_available: bool,
) -> bool {
    settings.enabled && settings.hoppi_install_offer != "dismissed" && !backend_available
}

fn hoppi_first_start_install_prompt() -> String {
    format!(
        "Install Hoppi memory?\n\
         OPPi memory is enabled, but {HOPPI_PACKAGE_NAME} is not installed. Install it now with npm into {}?\n\
         Choose No to keep working. You can install later with `/memory install-hoppi`, approve directly with `/memory install-hoppi --yes`, or dismiss future startup offers with `/memory install-offer dismissed`. OPPi performs no hidden install.",
        format_path_for_display(&managed_packages_dir())
    )
}

fn format_path_for_display(path: &Path) -> String {
    let text = path.display().to_string();
    if let Some(home) = home_dir() {
        let home = home.display().to_string();
        if text.starts_with(&home) {
            return format!("~{}", &text[home.len()..]);
        }
    }
    text
}

fn ensure_managed_packages_root() -> Result<PathBuf, String> {
    let root = managed_packages_dir();
    fs::create_dir_all(&root)
        .map_err(|error| format!("create managed packages dir {}: {error}", root.display()))?;
    let package_json = root.join("package.json");
    if !package_json.exists() {
        fs::write(
            &package_json,
            "{\n  \"private\": true,\n  \"name\": \"oppi-managed-packages\",\n  \"description\": \"OPPi managed optional packages.\"\n}\n",
        )
        .map_err(|error| format!("write {}: {error}", package_json.display()))?;
    }
    Ok(root)
}

fn compact_process_output(output: &[u8]) -> String {
    let text = String::from_utf8_lossy(output).replace(['\r', '\n'], " ");
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.chars().count() > 800 {
        text.chars()
            .rev()
            .take(800)
            .collect::<String>()
            .chars()
            .rev()
            .collect()
    } else {
        text
    }
}

fn install_meridian_package() -> Result<String, String> {
    let root = ensure_managed_packages_root()?;
    let spec = meridian_package_spec();
    let output = Command::new(if cfg!(windows) { "npm.cmd" } else { "npm" })
        .args([
            "install",
            spec.as_str(),
            "--save-exact",
            "--no-audit",
            "--no-fund",
        ])
        .current_dir(&root)
        .output()
        .map_err(|error| format!("spawn npm install: {error}"))?;
    if output.status.success() && managed_meridian_bin_path().exists() {
        Ok(format!(
            "Installed Meridian bridge {} into {}. Next: run `claude login` if needed, choose a model with `/model`, then `/login subscription claude start`.",
            spec,
            format_path_for_display(&root)
        ))
    } else {
        let tail = compact_process_output(&[output.stdout, output.stderr].concat());
        Err(format!(
            "Meridian install failed with status {}{}{}",
            output.status,
            if tail.is_empty() { "" } else { ": " },
            tail
        ))
    }
}

fn default_hoppi_sync_repo_path() -> PathBuf {
    oppi_home_dir()
        .join("oppi")
        .join("hoppi-sync")
        .join("hoppi-memories")
}

fn default_hoppi_sync_passphrase_file() -> PathBuf {
    oppi_home_dir()
        .join("oppi")
        .join("hoppi-sync")
        .join("passphrase")
}

fn resolve_hoppi_sync_path(value: &str) -> PathBuf {
    resolve_optional_module_path(PathBuf::from(value))
}

fn hoppi_module_path_for_sync() -> Result<PathBuf, String> {
    hoppi_candidate_module_paths_for_startup_offer()
        .into_iter()
        .find(|path| path.exists())
        .ok_or_else(|| {
            format!(
                "Hoppi sync skipped: Hoppi backend is not installed. Run `/memory install-hoppi --yes` or set OPPI_HOPPI_MODULE."
            )
        })
}

fn hoppi_module_path_for_recall() -> Result<PathBuf, String> {
    hoppi_candidate_module_paths_for_startup_offer()
        .into_iter()
        .find(|path| path.exists())
        .ok_or_else(|| {
            format!(
                "Hoppi recall skipped: Hoppi backend is not installed. Run `/memory install-hoppi --yes` or set OPPI_HOPPI_MODULE."
            )
        })
}

fn hoppi_module_path_for_remember() -> Result<PathBuf, String> {
    hoppi_candidate_module_paths_for_startup_offer()
        .into_iter()
        .find(|path| path.exists())
        .ok_or_else(|| {
            format!(
                "Hoppi remember skipped: Hoppi backend is not installed. Run `/memory install-hoppi --yes` or set OPPI_HOPPI_MODULE."
            )
        })
}

fn hoppi_module_path_for_dashboard() -> Result<PathBuf, String> {
    hoppi_candidate_module_paths_for_startup_offer()
        .into_iter()
        .find(|path| path.exists())
        .ok_or_else(|| {
            format!(
                "Hoppi dashboard skipped: Hoppi backend is not installed. Run `/memory install-hoppi --yes` or set OPPI_HOPPI_MODULE."
            )
        })
}

fn hoppi_module_path_for_list() -> Result<PathBuf, String> {
    hoppi_candidate_module_paths_for_startup_offer()
        .into_iter()
        .find(|path| path.exists())
        .ok_or_else(|| {
            format!(
                "Hoppi list skipped: Hoppi backend is not installed. Run `/memory install-hoppi --yes` or set OPPI_HOPPI_MODULE."
            )
        })
}

fn hoppi_module_path_for_forget() -> Result<PathBuf, String> {
    hoppi_candidate_module_paths_for_startup_offer()
        .into_iter()
        .find(|path| path.exists())
        .ok_or_else(|| {
            format!(
                "Hoppi forget skipped: Hoppi backend is not installed. Run `/memory install-hoppi --yes` or set OPPI_HOPPI_MODULE."
            )
        })
}

fn hoppi_module_path_for_update() -> Result<PathBuf, String> {
    hoppi_candidate_module_paths_for_startup_offer()
        .into_iter()
        .find(|path| path.exists())
        .ok_or_else(|| {
            format!(
                "Hoppi update skipped: Hoppi backend is not installed. Run `/memory install-hoppi --yes` or set OPPI_HOPPI_MODULE."
            )
        })
}

fn hoppi_module_path_for_consolidate() -> Result<PathBuf, String> {
    hoppi_candidate_module_paths_for_startup_offer()
        .into_iter()
        .find(|path| path.exists())
        .ok_or_else(|| {
            format!(
                "Hoppi maintenance skipped: Hoppi backend is not installed. Run `/memory install-hoppi --yes` or set OPPI_HOPPI_MODULE."
            )
        })
}

fn memory_backend_recall_params_from_settings(
    settings: &MemorySettings,
    thread_id: &str,
    prompt: &str,
) -> Result<Value, String> {
    if !settings.enabled {
        return Err("Hoppi recall skipped: memory disabled".to_string());
    }
    let prompt = prompt.trim();
    let include_startup = settings.startup_recall;
    let include_task = settings.task_start_recall && !prompt.is_empty();
    if !include_startup && !include_task {
        return Err("Hoppi recall skipped: recall disabled".to_string());
    }
    Ok(json!({
        "threadId": thread_id,
        "prompt": prompt.chars().take(HOPPI_MEMORY_QUERY_MAX_CHARS).collect::<String>(),
        "includeStartup": include_startup,
        "includeTask": include_task,
    }))
}

fn memory_backend_dashboard_params_from_settings(
    settings: &MemorySettings,
    thread_id: &str,
) -> Result<Value, String> {
    if !settings.enabled {
        return Err("Hoppi dashboard skipped: memory disabled".to_string());
    }
    Ok(json!({ "threadId": thread_id }))
}

fn memory_backend_list_params_from_settings(
    settings: &MemorySettings,
    thread_id: &str,
    include_superseded: bool,
    pinned_only: bool,
    limit: Option<u64>,
) -> Result<Value, String> {
    if !settings.enabled {
        return Err("Hoppi list skipped: memory disabled".to_string());
    }
    let mut params = json!({
        "threadId": thread_id,
        "includeSuperseded": include_superseded,
        "pinnedOnly": pinned_only,
    });
    if let Some(limit) = limit {
        params["limit"] = json!(limit);
    }
    Ok(params)
}

fn memory_backend_forget_params_from_settings(
    settings: &MemorySettings,
    thread_id: &str,
    id: &str,
) -> Result<Value, String> {
    if !settings.enabled {
        return Err("Hoppi forget skipped: memory disabled".to_string());
    }
    let id = id.trim();
    if id.is_empty() {
        return Err("Hoppi forget skipped: memory id is required".to_string());
    }
    Ok(json!({ "threadId": thread_id, "id": id }))
}

fn memory_backend_pin_params_from_settings(
    settings: &MemorySettings,
    thread_id: &str,
    id: &str,
    pinned: bool,
) -> Result<Value, String> {
    if !settings.enabled {
        return Err("Hoppi pin skipped: memory disabled".to_string());
    }
    let id = id.trim();
    if id.is_empty() {
        return Err("Hoppi pin skipped: memory id is required".to_string());
    }
    Ok(json!({ "threadId": thread_id, "id": id, "pinned": pinned }))
}

fn memory_backend_update_params_from_settings(
    settings: &MemorySettings,
    thread_id: &str,
    id: &str,
    patch: Value,
) -> Result<Value, String> {
    if !settings.enabled {
        return Err("Hoppi update skipped: memory disabled".to_string());
    }
    let id = id.trim();
    if id.is_empty() {
        return Err("Hoppi update skipped: memory id is required".to_string());
    }
    let Some(object) = patch.as_object() else {
        return Err("Hoppi update skipped: update patch must be an object".to_string());
    };
    if object.is_empty() {
        return Err("Hoppi update skipped: update patch cannot be empty".to_string());
    }
    Ok(json!({ "threadId": thread_id, "id": id, "patch": patch }))
}

fn memory_backend_supersede_params_from_settings(
    settings: &MemorySettings,
    thread_id: &str,
    id: &str,
    superseded_by: &str,
) -> Result<Value, String> {
    if !settings.enabled {
        return Err("Hoppi supersede skipped: memory disabled".to_string());
    }
    let id = id.trim();
    let superseded_by = superseded_by.trim();
    if id.is_empty() || superseded_by.is_empty() || id == superseded_by {
        return Err(memory_settings_usage().to_string());
    }
    Ok(json!({
        "threadId": thread_id,
        "id": id,
        "supersededBy": superseded_by,
    }))
}

fn memory_backend_consolidate_params_from_settings(
    settings: &MemorySettings,
    thread_id: &str,
    dry_run: bool,
) -> Result<Value, String> {
    if !settings.enabled {
        return Err("Hoppi maintenance skipped: memory disabled".to_string());
    }
    Ok(json!({
        "threadId": thread_id,
        "stage": "manual-maintenance",
        "dryRun": dry_run,
        "limit": 50,
        "agentModel": memory_model_preference_or_auto(&settings.agent_model),
        "deepModel": memory_model_preference_or_auto(&settings.deep_model),
    }))
}

fn memory_backend_idle_consolidate_params_from_settings(
    settings: &MemorySettings,
    thread_id: &str,
) -> Result<Value, String> {
    if !settings.enabled {
        return Err("Hoppi idle consolidation skipped: memory disabled".to_string());
    }
    if !settings.idle_consolidation {
        return Err("Hoppi idle consolidation skipped: idle consolidation disabled".to_string());
    }
    Ok(json!({
        "threadId": thread_id,
        "stage": "idle-dream",
        "dryRun": false,
        "budget": HOPPI_IDLE_CONSOLIDATE_BUDGET,
        "agentModel": memory_model_preference_or_auto(&settings.agent_model),
        "deepModel": memory_model_preference_or_auto(&settings.deep_model),
    }))
}

fn memory_backend_turn_remember_params_from_settings(
    settings: &MemorySettings,
    summary: &str,
    thread_id: &str,
    turn_id: &str,
) -> Result<Value, String> {
    if !settings.enabled {
        return Err("Hoppi remember skipped: memory disabled".to_string());
    }
    if !settings.turn_summaries {
        return Err("Hoppi remember skipped: turn summaries disabled".to_string());
    }
    let summary = summary.trim();
    if summary.is_empty() {
        return Err("Hoppi remember skipped: empty summary".to_string());
    }
    Ok(json!({
        "threadId": thread_id,
        "content": summary,
        "tags": ["oppi-native-shell", "oppi-turn-summary", "agent_end"],
        "source": "oppi:native-shell",
        "sourceSessionId": format!("{thread_id}:{turn_id}"),
        "layer": "buffer",
    }))
}

fn memory_backend_session_recap_params_from_settings(
    settings: &MemorySettings,
    recap: &str,
    thread_id: &str,
    reason: HoppiSessionRecapReason,
) -> Result<Value, String> {
    if !settings.enabled {
        return Err(format!(
            "Hoppi {} recap skipped: memory disabled",
            reason.as_str()
        ));
    }
    let recap = recap.trim();
    if recap.is_empty() {
        return Err(format!(
            "Hoppi {} recap skipped: empty recap",
            reason.as_str()
        ));
    }
    Ok(json!({
        "threadId": thread_id,
        "content": recap,
        "tags": ["oppi-native-shell", reason.tag(), "session-recap"],
        "source": format!("oppi:{}", reason.as_str()),
        "sourceSessionId": thread_id,
    }))
}

fn memory_backend_sync_params_from_settings(
    settings: &MemorySettings,
    thread_id: &str,
    direction: HoppiSyncDirection,
    reason: &str,
) -> Result<Value, String> {
    if !settings.enabled {
        return Err("Hoppi sync skipped: memory disabled".to_string());
    }
    if !settings.sync_enabled {
        return Err("Hoppi sync skipped: sync disabled".to_string());
    }
    let repo_path = settings
        .sync_repo_path
        .as_deref()
        .map(resolve_hoppi_sync_path)
        .unwrap_or_else(default_hoppi_sync_repo_path);
    let mut params = json!({
        "threadId": thread_id,
        "direction": direction.as_str(),
        "repoPath": repo_path.display().to_string(),
        "conflictMode": settings.sync_conflict_mode,
        "message": format!("Hoppi memory sync ({reason})"),
    });
    if let Some(remote) = settings.sync_repo_url.as_ref() {
        params["remote"] = json!(remote);
    }
    if let Some(passphrase) = hoppi_sync_passphrase(settings)? {
        params["passphrase"] = json!(passphrase);
    }
    Ok(params)
}

fn hoppi_sync_display_invocation_from_settings(
    settings: &MemorySettings,
    direction: HoppiSyncDirection,
    reason: &str,
) -> Result<HoppiSyncInvocation, String> {
    if !settings.enabled {
        return Err("Hoppi sync skipped: memory disabled".to_string());
    }
    if !settings.sync_enabled {
        return Err("Hoppi sync skipped: sync disabled".to_string());
    }
    let repo_path = settings
        .sync_repo_path
        .as_deref()
        .map(resolve_hoppi_sync_path)
        .unwrap_or_else(default_hoppi_sync_repo_path);
    Ok(HoppiSyncInvocation {
        module_path: PathBuf::new(),
        repo_path,
        direction,
        remote: settings.sync_repo_url.clone(),
        conflict_mode: settings.sync_conflict_mode.clone(),
        passphrase: None,
        message: format!("Hoppi memory sync ({reason})"),
    })
}

fn memory_backend_normalized_value(value: Value, method: &str) -> Result<Value, String> {
    value
        .get("normalized")
        .cloned()
        .ok_or_else(|| format!("{method} response missing normalized result"))
}

fn hoppi_recall_invocation_from_settings(
    settings: &MemorySettings,
    cwd: &Path,
    prompt: &str,
    module_path: PathBuf,
) -> Result<HoppiRecallInvocation, String> {
    if !settings.enabled {
        return Err("Hoppi recall skipped: memory disabled".to_string());
    }
    let prompt = prompt.trim();
    let include_startup = settings.startup_recall;
    let include_task = settings.task_start_recall && !prompt.is_empty();
    if !include_startup && !include_task {
        return Err("Hoppi recall skipped: recall disabled".to_string());
    }
    Ok(HoppiRecallInvocation {
        module_path,
        cwd: cwd.to_path_buf(),
        prompt: prompt.chars().take(HOPPI_MEMORY_QUERY_MAX_CHARS).collect(),
        include_startup,
        include_task,
    })
}

fn hoppi_dashboard_invocation_from_settings(
    settings: &MemorySettings,
    cwd: &Path,
    module_path: PathBuf,
) -> Result<HoppiDashboardInvocation, String> {
    if !settings.enabled {
        return Err("Hoppi dashboard skipped: memory disabled".to_string());
    }
    Ok(HoppiDashboardInvocation {
        module_path,
        cwd: cwd.to_path_buf(),
        port: settings.dashboard_port,
    })
}

fn hoppi_list_invocation_from_settings(
    settings: &MemorySettings,
    cwd: &Path,
    module_path: PathBuf,
    include_superseded: bool,
    pinned_only: bool,
    limit: Option<u64>,
) -> Result<HoppiListInvocation, String> {
    if !settings.enabled {
        return Err("Hoppi list skipped: memory disabled".to_string());
    }
    Ok(HoppiListInvocation {
        module_path,
        cwd: cwd.to_path_buf(),
        include_superseded,
        pinned_only,
        limit,
    })
}

fn hoppi_forget_invocation_from_settings(
    settings: &MemorySettings,
    cwd: &Path,
    module_path: PathBuf,
    id: &str,
) -> Result<HoppiForgetInvocation, String> {
    if !settings.enabled {
        return Err("Hoppi forget skipped: memory disabled".to_string());
    }
    let id = id.trim();
    if id.is_empty() {
        return Err("Hoppi forget skipped: memory id is required".to_string());
    }
    Ok(HoppiForgetInvocation {
        module_path,
        cwd: cwd.to_path_buf(),
        id: id.to_string(),
    })
}

fn hoppi_pin_invocation_from_settings(
    settings: &MemorySettings,
    cwd: &Path,
    module_path: PathBuf,
    id: &str,
    pinned: bool,
) -> Result<HoppiPinInvocation, String> {
    if !settings.enabled {
        return Err("Hoppi pin skipped: memory disabled".to_string());
    }
    let id = id.trim();
    if id.is_empty() {
        return Err("Hoppi pin skipped: memory id is required".to_string());
    }
    Ok(HoppiPinInvocation {
        module_path,
        cwd: cwd.to_path_buf(),
        id: id.to_string(),
        pinned,
    })
}

fn hoppi_update_invocation_from_settings(
    settings: &MemorySettings,
    cwd: &Path,
    module_path: PathBuf,
    id: &str,
    patch: Value,
) -> Result<HoppiUpdateInvocation, String> {
    if !settings.enabled {
        return Err("Hoppi update skipped: memory disabled".to_string());
    }
    let id = id.trim();
    if id.is_empty() {
        return Err("Hoppi update skipped: memory id is required".to_string());
    }
    let Some(object) = patch.as_object() else {
        return Err("Hoppi update skipped: update patch must be an object".to_string());
    };
    if object.is_empty() {
        return Err("Hoppi update skipped: update patch cannot be empty".to_string());
    }
    Ok(HoppiUpdateInvocation {
        module_path,
        cwd: cwd.to_path_buf(),
        id: id.to_string(),
        patch,
    })
}

fn memory_model_preference_or_auto(value: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        "auto".to_string()
    } else {
        value.to_string()
    }
}

fn hoppi_consolidate_invocation_from_settings(
    settings: &MemorySettings,
    cwd: &Path,
    module_path: PathBuf,
    dry_run: bool,
) -> Result<HoppiConsolidateInvocation, String> {
    if !settings.enabled {
        return Err("Hoppi maintenance skipped: memory disabled".to_string());
    }
    Ok(HoppiConsolidateInvocation {
        module_path,
        cwd: cwd.to_path_buf(),
        dry_run,
        stage: "manual-maintenance".to_string(),
        limit: Some(50),
        budget: None,
        agent_model: memory_model_preference_or_auto(&settings.agent_model),
        deep_model: memory_model_preference_or_auto(&settings.deep_model),
    })
}

fn hoppi_idle_consolidate_invocation_from_settings(
    settings: &MemorySettings,
    cwd: &Path,
    module_path: PathBuf,
) -> Result<HoppiConsolidateInvocation, String> {
    if !settings.enabled {
        return Err("Hoppi idle consolidation skipped: memory disabled".to_string());
    }
    if !settings.idle_consolidation {
        return Err("Hoppi idle consolidation skipped: idle consolidation disabled".to_string());
    }
    Ok(HoppiConsolidateInvocation {
        module_path,
        cwd: cwd.to_path_buf(),
        dry_run: false,
        stage: "idle-dream".to_string(),
        limit: None,
        budget: Some(HOPPI_IDLE_CONSOLIDATE_BUDGET),
        agent_model: memory_model_preference_or_auto(&settings.agent_model),
        deep_model: memory_model_preference_or_auto(&settings.deep_model),
    })
}

fn hoppi_remember_invocation_from_settings(
    settings: &MemorySettings,
    cwd: &Path,
    summary: &str,
    thread_id: &str,
    turn_id: &str,
    module_path: PathBuf,
) -> Result<HoppiRememberInvocation, String> {
    if !settings.enabled {
        return Err("Hoppi remember skipped: memory disabled".to_string());
    }
    if !settings.turn_summaries {
        return Err("Hoppi remember skipped: turn summaries disabled".to_string());
    }
    let summary = summary.trim();
    if summary.is_empty() {
        return Err("Hoppi remember skipped: empty summary".to_string());
    }
    Ok(HoppiRememberInvocation {
        module_path,
        cwd: cwd.to_path_buf(),
        summary: summary.to_string(),
        tags: vec![
            "oppi-native-shell".to_string(),
            "oppi-turn-summary".to_string(),
            "agent_end".to_string(),
        ],
        source: "oppi:native-shell".to_string(),
        source_session_id: format!("{thread_id}:{turn_id}"),
        layer: Some("buffer".to_string()),
    })
}

fn hoppi_exit_recap_invocation_from_settings(
    settings: &MemorySettings,
    cwd: &Path,
    recap: &str,
    thread_id: &str,
    module_path: PathBuf,
) -> Result<HoppiRememberInvocation, String> {
    hoppi_session_recap_invocation_from_settings(
        settings,
        cwd,
        recap,
        thread_id,
        module_path,
        HoppiSessionRecapReason::Exit,
    )
}

fn hoppi_session_recap_invocation_from_settings(
    settings: &MemorySettings,
    cwd: &Path,
    recap: &str,
    thread_id: &str,
    module_path: PathBuf,
    reason: HoppiSessionRecapReason,
) -> Result<HoppiRememberInvocation, String> {
    if !settings.enabled {
        return Err(format!(
            "Hoppi {} recap skipped: memory disabled",
            reason.as_str()
        ));
    }
    let recap = recap.trim();
    if recap.is_empty() {
        return Err(format!(
            "Hoppi {} recap skipped: empty recap",
            reason.as_str()
        ));
    }
    Ok(HoppiRememberInvocation {
        module_path,
        cwd: cwd.to_path_buf(),
        summary: recap.to_string(),
        tags: vec![
            "oppi-native-shell".to_string(),
            reason.tag().to_string(),
            "session-recap".to_string(),
        ],
        source: format!("oppi:{}", reason.as_str()),
        source_session_id: thread_id.to_string(),
        layer: None,
    })
}

fn hoppi_sync_passphrase(settings: &MemorySettings) -> Result<Option<String>, String> {
    if settings.sync_encryption != "passphrase" {
        return Ok(None);
    }
    if settings.sync_passphrase_source == "file" {
        let file = settings
            .sync_passphrase_file
            .as_deref()
            .map(resolve_hoppi_sync_path)
            .unwrap_or_else(default_hoppi_sync_passphrase_file);
        let value = fs::read_to_string(&file)
            .map_err(|error| {
                format!(
                    "Encrypted sync passphrase file is missing: {}: {error}",
                    file.display()
                )
            })?
            .trim()
            .to_string();
        if value.is_empty() {
            return Err(format!(
                "Encrypted sync passphrase file is empty: {}",
                file.display()
            ));
        }
        return Ok(Some(value));
    }
    let value = env::var(&settings.sync_passphrase_env)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            format!(
                "Encrypted sync needs {} to be set.",
                settings.sync_passphrase_env
            )
        })?;
    Ok(Some(value))
}

fn hoppi_sync_invocation_from_settings(
    settings: &MemorySettings,
    direction: HoppiSyncDirection,
    reason: &str,
    module_path: PathBuf,
) -> Result<HoppiSyncInvocation, String> {
    if !settings.enabled {
        return Err("Hoppi sync skipped: memory disabled".to_string());
    }
    if !settings.sync_enabled {
        return Err("Hoppi sync skipped: sync disabled".to_string());
    }
    let repo_path = settings
        .sync_repo_path
        .as_deref()
        .map(resolve_hoppi_sync_path)
        .unwrap_or_else(default_hoppi_sync_repo_path);
    Ok(HoppiSyncInvocation {
        module_path,
        repo_path,
        direction,
        remote: settings.sync_repo_url.clone(),
        conflict_mode: settings.sync_conflict_mode.clone(),
        passphrase: hoppi_sync_passphrase(settings)?,
        message: format!("Hoppi memory sync ({reason})"),
    })
}

fn hoppi_sync_lifecycle_direction(
    settings: &MemorySettings,
    lifecycle: HoppiSyncLifecycle,
) -> Option<HoppiSyncDirection> {
    if !settings.enabled || !settings.sync_enabled {
        return None;
    }
    match lifecycle {
        HoppiSyncLifecycle::Startup if settings.sync_pull_on_startup => {
            Some(HoppiSyncDirection::Pull)
        }
        HoppiSyncLifecycle::Exit if settings.sync_push_on_exit => Some(HoppiSyncDirection::Push),
        HoppiSyncLifecycle::Clear if settings.sync_push_on_exit => Some(HoppiSyncDirection::Push),
        _ => None,
    }
}

fn hoppi_background_sync_interval(settings: &MemorySettings) -> Option<Duration> {
    if !settings.enabled || !settings.sync_enabled {
        return None;
    }
    match settings.sync_background.as_str() {
        "15m" => Some(Duration::from_secs(15 * 60)),
        "30m" => Some(Duration::from_secs(30 * 60)),
        "60m" => Some(Duration::from_secs(60 * 60)),
        _ => None,
    }
}

fn hoppi_next_background_sync_at(settings: &MemorySettings, now: Instant) -> Option<Instant> {
    hoppi_background_sync_interval(settings).map(|interval| now + interval)
}

fn hoppi_background_sync_due(
    settings: &MemorySettings,
    next_due: Option<Instant>,
    idle: bool,
    now: Instant,
) -> bool {
    idle && hoppi_background_sync_interval(settings).is_some()
        && next_due.is_some_and(|due| now >= due)
}

fn hoppi_idle_consolidation_due(
    settings: &MemorySettings,
    dirty_version: u64,
    consolidated_version: u64,
    dirty_at: Option<Instant>,
    idle: bool,
    now: Instant,
) -> bool {
    if !settings.enabled || !settings.idle_consolidation || !idle {
        return false;
    }
    if dirty_version <= consolidated_version {
        return false;
    }
    let Some(dirty_at) = dirty_at else {
        return false;
    };
    now.checked_duration_since(dirty_at)
        .is_some_and(|elapsed| elapsed >= HOPPI_IDLE_CONSOLIDATE_AFTER)
}

fn hoppi_turn_summary_timestamp() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    format!("unix:{seconds}")
}

fn compact_hoppi_summary_text(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn truncate_hoppi_summary_part(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

fn build_hoppi_turn_summary(
    prompt: &str,
    assistant_text: &str,
    tool_result_statuses: &[String],
    thread_id: &str,
    turn_id: &str,
    timestamp: &str,
) -> Option<String> {
    let user = compact_hoppi_summary_text(prompt);
    let assistant = compact_hoppi_summary_text(assistant_text);
    if user.is_empty() || assistant.is_empty() || user.len() + assistant.len() < 80 {
        return None;
    }
    Some(
        [
            Some(format!("Native shell turn summary ({timestamp})")),
            Some(format!(
                "User asked: {}",
                truncate_hoppi_summary_part(&user, 500)
            )),
            Some(format!(
                "Assistant outcome: {}",
                truncate_hoppi_summary_part(&assistant, 700)
            )),
            (!tool_result_statuses.is_empty())
                .then(|| format!("Tool results: {}", tool_result_statuses.join(", "))),
            Some(format!("Runtime turn: {thread_id}/{turn_id}")),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("\n"),
    )
}

fn build_hoppi_turn_summary_from_events(
    events: &[Event],
    thread_id: &str,
    turn_id: &str,
    fallback_prompt: Option<&str>,
    timestamp: &str,
) -> Option<String> {
    let mut user_text = fallback_prompt.unwrap_or_default().to_string();
    let mut assistant_parts = Vec::new();
    let mut tool_result_statuses = Vec::new();
    for event in events
        .iter()
        .filter(|event| event.thread_id == thread_id && event.turn_id.as_deref() == Some(turn_id))
    {
        match &event.kind {
            EventKind::ItemCompleted { item } if item.turn_id == turn_id => match &item.kind {
                ItemKind::UserMessage { text } => user_text = text.clone(),
                ItemKind::AssistantMessage { text } => assistant_parts.push(text.clone()),
                _ => {}
            },
            EventKind::ToolCallCompleted { result } => {
                tool_result_statuses.push(tool_result_status_label(result.status).to_string());
            }
            _ => {}
        }
    }
    build_hoppi_turn_summary(
        &user_text,
        &assistant_parts.join("\n\n"),
        &tool_result_statuses,
        thread_id,
        turn_id,
        timestamp,
    )
}

fn is_low_signal_hoppi_memory_text(text: &str) -> bool {
    let normalized = compact_hoppi_summary_text(text).to_lowercase();
    normalized.is_empty()
        || normalized == "cd"
        || normalized.starts_with("current working directory:")
        || normalized
            .strip_prefix("[tool:")
            .is_some_and(|rest| rest.ends_with(']'))
}

fn recent_unique_hoppi_texts(texts: &[String], limit: usize, max_chars: usize) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut selected = Vec::new();
    for text in texts.iter().rev() {
        let compact = compact_hoppi_summary_text(text);
        if is_low_signal_hoppi_memory_text(&compact) {
            continue;
        }
        let key = compact.to_lowercase().chars().take(180).collect::<String>();
        if !seen.insert(key) {
            continue;
        }
        selected.push(truncate_hoppi_summary_part(&compact, max_chars));
        if selected.len() >= limit {
            break;
        }
    }
    selected.reverse();
    selected
}

fn build_hoppi_session_recap_from_events(
    events: &[Event],
    thread_id: &str,
    timestamp: &str,
) -> Option<String> {
    let mut user_texts = Vec::new();
    let mut assistant_texts = Vec::new();
    for event in events.iter().filter(|event| event.thread_id == thread_id) {
        let EventKind::ItemCompleted { item } = &event.kind else {
            continue;
        };
        if item.thread_id != thread_id {
            continue;
        }
        match &item.kind {
            ItemKind::UserMessage { text } => user_texts.push(text.clone()),
            ItemKind::AssistantMessage { text } => assistant_texts.push(text.clone()),
            _ => {}
        }
    }

    let recent_user_goals = recent_unique_hoppi_texts(&user_texts, 4, 180);
    let recent_assistant_outcomes = recent_unique_hoppi_texts(&assistant_texts, 4, 220);
    if recent_user_goals.is_empty() && recent_assistant_outcomes.is_empty() {
        return None;
    }

    let mut lines = vec![
        format!("Session recap ({timestamp})"),
        "Recent user goals:".to_string(),
    ];
    lines.extend(recent_user_goals.iter().map(|text| format!("- {text}")));
    lines.push("Recent assistant outcomes:".to_string());
    lines.extend(
        recent_assistant_outcomes
            .iter()
            .map(|text| format!("- {text}")),
    );
    let recap = lines.join("\n");
    (recap.len() >= 90).then_some(recap)
}

const HOPPI_RECALL_NODE_SCRIPT: &str = r#"
const path = require("node:path");
const os = require("node:os");
const { pathToFileURL } = require("node:url");

const CONTEXT_MAX_BYTES = Number(process.env.OPPI_HOPPI_MEMORY_CONTEXT_MAX_BYTES || 12000);

function coerceMemoryCount(value) {
  const numeric = Number(value);
  return Number.isFinite(numeric) && numeric > 0 ? Math.floor(numeric) : 0;
}

function truncateUtf8Text(value, maxBytes, suffix = "\n\n[Hoppi memory context truncated]") {
  const text = String(value || "").trim();
  if (Buffer.byteLength(text, "utf8") <= maxBytes) return { text, truncated: false };
  const suffixBytes = Buffer.byteLength(suffix, "utf8");
  const limit = Math.max(0, maxBytes - suffixBytes);
  const truncated = Buffer.from(text, "utf8").subarray(0, limit).toString("utf8").replace(/\uFFFD+$/g, "").trimEnd();
  return { text: `${truncated}${suffix}`, truncated: true };
}

function stringField(entry, keys) {
  for (const key of keys) {
    const value = entry && entry[key];
    if (typeof value === "string" && value.trim()) return value.trim();
  }
  return undefined;
}

function numberField(entry, keys) {
  for (const key of keys) {
    const value = Number(entry && entry[key]);
    if (Number.isFinite(value)) return value;
  }
  return undefined;
}

function stringArray(value) {
  return Array.isArray(value)
    ? value.map((item) => String(item || "").trim()).filter(Boolean)
    : [];
}

function modelField(value) {
  return typeof value === "string" && value.trim() ? value.trim() : "auto";
}

function normalizeRecord(entry) {
  const id = stringField(entry, ["id", "memoryId"]);
  const content = stringField(entry, ["content", "text", "summary", "body"]);
  if (!id || !content) return undefined;
  return {
    id,
    content,
    tags: stringArray(entry.tags),
    pinned: Boolean(entry.pinned),
    starred: Boolean(entry.starred),
    layer: stringField(entry, ["layer"]),
    confidence: stringField(entry, ["confidence"]),
    source: stringField(entry, ["source"]),
    createdAt: stringField(entry, ["createdAt", "created_at", "created"]),
    updatedAt: stringField(entry, ["updatedAt", "updated_at"]),
    strength: numberField(entry, ["strength"]),
    retrievalStrength: numberField(entry, ["retrievalStrength", "retrieval_strength"]),
    retrievalCount: coerceMemoryCount(entry.retrievalCount ?? entry.retrieval_count ?? entry.useCount ?? entry.use_count),
    lastRetrievedAt: stringField(entry, ["lastRetrievedAt", "last_retrieved_at", "retrieved_at", "lastUsedAt", "last_used_at"]),
    supersededBy: stringField(entry, ["supersededBy", "superseded_by"]),
    parents: stringArray(entry.parents),
  };
}

function mergeRecords(target, rawRecords) {
  if (!Array.isArray(rawRecords)) return;
  for (const rawRecord of rawRecords) {
    const record = normalizeRecord(rawRecord);
    if (!record) continue;
    const existing = target.findIndex((item) => item.id === record.id);
    if (existing >= 0) target[existing] = record;
    else target.push(record);
  }
}

function buildMemoryContext(parts) {
  const unique = [...new Set(parts.map((part) => String(part || "").trim()).filter(Boolean))];
  if (!unique.length) return { contextBytes: 0, truncated: false };
  const advisory = "\n\nMemory is advisory: prefer current user instructions and current files when they conflict.";
  const bodyLimit = Math.max(1, CONTEXT_MAX_BYTES - Buffer.byteLength(advisory, "utf8"));
  const body = truncateUtf8Text(unique.join("\n\n---\n\n"), bodyLimit);
  const contextMarkdown = `${body.text}${advisory}`;
  return { contextMarkdown, contextBytes: Buffer.byteLength(contextMarkdown, "utf8"), truncated: body.truncated };
}

(async () => {
  const [modulePath, cwd, prompt, includeStartupRaw, includeTaskRaw] = process.argv.slice(1);
  const includeStartup = includeStartupRaw === "true";
  const includeTask = includeTaskRaw === "true";
  const hoppi = await import(pathToFileURL(path.resolve(modulePath)).href);
  if (typeof hoppi.createHoppiBackend !== "function") {
    throw new Error("Installed Hoppi package does not expose createHoppiBackend().");
  }
  const root = typeof hoppi.getDefaultHoppiRoot === "function"
    ? hoppi.getDefaultHoppiRoot()
    : (process.env.HOPPI_HOME ? path.resolve(process.env.HOPPI_HOME) : path.join(os.homedir(), ".oppi", "hoppi"));
  const backend = hoppi.createHoppiBackend({ root });
  if (!backend || typeof backend !== "object") {
    throw new Error("Installed Hoppi package returned an invalid backend.");
  }
  if (typeof backend.init === "function") await backend.init();
  const resolvedCwd = path.resolve(cwd);
  const project = { cwd: resolvedCwd, displayName: path.basename(resolvedCwd) || undefined };
  const parts = [];
  const records = [];
  let memoryCount = 0;
  let pinnedCount = 0;
  let storePath;

  if (typeof backend.status === "function") {
    try {
      const status = await backend.status(project);
      memoryCount = Math.max(memoryCount, coerceMemoryCount(status && status.memoryCount));
      pinnedCount = Math.max(pinnedCount, coerceMemoryCount(status && status.pinnedCount));
      if (status && status.storePath) storePath = String(status.storePath);
    } catch {
      // Status is informational; recall should still be able to provide useful context.
    }
  }

  if (includeStartup && typeof backend.buildStartupContext === "function") {
    try {
      const startup = await backend.buildStartupContext({ project, maxMemories: 8, includePinned: true });
      if (startup && startup.contextMarkdown) parts.push(startup.contextMarkdown);
      memoryCount = Math.max(memoryCount, coerceMemoryCount(startup && startup.memoryCount));
      if (startup && Array.isArray(startup.memories)) {
        memoryCount = Math.max(memoryCount, startup.memories.length);
        mergeRecords(records, startup.memories);
      }
    } catch {
      // Startup recall is additive; task recall may still succeed.
    }
  }

  if (includeTask && String(prompt || "").trim() && typeof backend.recall === "function") {
    try {
      const recall = await backend.recall({ project, query: String(prompt || "").trim(), budget: 900, limit: 5 });
      if (recall && recall.contextMarkdown) parts.push(recall.contextMarkdown);
      if (recall && Array.isArray(recall.memories)) memoryCount = Math.max(memoryCount, recall.memories.length);
      if (recall && Array.isArray(recall.memories)) mergeRecords(records, recall.memories);
      memoryCount = Math.max(memoryCount, coerceMemoryCount(recall && recall.memoryCount));
    } catch {
      // The shell treats missing recall as no context instead of aborting the turn.
    }
  }

  pinnedCount = Math.max(pinnedCount, records.filter((record) => record.pinned || record.starred).length);
  console.log(JSON.stringify({ ...buildMemoryContext(parts), memoryCount, pinnedCount, storePath, records }));
})().catch((error) => {
  console.error(error && error.stack ? error.stack : String(error));
  process.exit(1);
});
"#;

const HOPPI_DASHBOARD_NODE_SCRIPT: &str = r#"
const path = require("node:path");
const os = require("node:os");
const { pathToFileURL } = require("node:url");

function coerceMemoryCount(value) {
  const numeric = Number(value);
  return Number.isFinite(numeric) && numeric > 0 ? Math.floor(numeric) : 0;
}

(async () => {
  const [modulePath, cwd, portRaw] = process.argv.slice(1);
  const hoppi = await import(pathToFileURL(path.resolve(modulePath)).href);
  if (typeof hoppi.createHoppiBackend !== "function") {
    throw new Error("Installed Hoppi package does not expose createHoppiBackend().");
  }
  const root = typeof hoppi.getDefaultHoppiRoot === "function"
    ? hoppi.getDefaultHoppiRoot()
    : (process.env.HOPPI_HOME ? path.resolve(process.env.HOPPI_HOME) : path.join(os.homedir(), ".oppi", "hoppi"));
  const backend = hoppi.createHoppiBackend({ root });
  if (!backend || typeof backend !== "object") {
    throw new Error("Installed Hoppi package returned an invalid backend.");
  }
  if (typeof backend.init === "function") await backend.init();
  if (typeof backend.startDashboard !== "function") {
    throw new Error("Installed Hoppi package does not expose backend.startDashboard().");
  }
  const resolvedCwd = path.resolve(cwd);
  const project = { cwd: resolvedCwd, displayName: path.basename(resolvedCwd) || undefined };
  let memoryCount = 0;
  let pinnedCount = 0;
  let storePath;
  if (typeof backend.status === "function") {
    try {
      const status = await backend.status(project);
      memoryCount = coerceMemoryCount(status && status.memoryCount);
      pinnedCount = coerceMemoryCount(status && status.pinnedCount);
      if (status && status.storePath) storePath = String(status.storePath);
    } catch {
      // Dashboard startup is still useful when status is unavailable.
    }
  }
  const requestedPort = portRaw && portRaw !== "auto" ? Number(portRaw) : 0;
  const dashboard = await backend.startDashboard({
    project,
    port: Number.isInteger(requestedPort) && requestedPort > 0 && requestedPort < 65536 ? requestedPort : 0,
  });
  if (!dashboard || !dashboard.url) {
    throw new Error("Installed Hoppi package returned no dashboard URL.");
  }
  const url = String(dashboard.url);
  const fallbackUrl = url.includes("hoppi.localhost") ? url.replace("hoppi.localhost", "localhost") : undefined;
  console.log(JSON.stringify({ url, fallbackUrl, memoryCount, pinnedCount, storePath }));

  let stopping = false;
  async function stopAndExit() {
    if (stopping) return;
    stopping = true;
    if (dashboard && typeof dashboard.stop === "function") {
      try { await dashboard.stop(); } catch {}
    }
    process.exit(0);
  }
  process.on("SIGTERM", () => { void stopAndExit(); });
  process.on("SIGINT", () => { void stopAndExit(); });
  process.stdin.resume();
  setInterval(() => {}, 2147483647);
})().catch((error) => {
  console.error(error && error.stack ? error.stack : String(error));
  process.exit(1);
});
"#;

const HOPPI_LIST_NODE_SCRIPT: &str = r#"
const path = require("node:path");
const os = require("node:os");
const { pathToFileURL } = require("node:url");

function coerceMemoryCount(value) {
  const numeric = Number(value);
  return Number.isFinite(numeric) && numeric > 0 ? Math.floor(numeric) : 0;
}

function stringField(entry, keys) {
  for (const key of keys) {
    const value = entry && entry[key];
    if (typeof value === "string" && value.trim()) return value.trim();
  }
  return undefined;
}

function numberField(entry, keys) {
  for (const key of keys) {
    const value = Number(entry && entry[key]);
    if (Number.isFinite(value)) return value;
  }
  return undefined;
}

function stringArray(value) {
  return Array.isArray(value)
    ? value.map((item) => String(item || "").trim()).filter(Boolean)
    : [];
}

function normalizeRecord(entry) {
  const id = stringField(entry, ["id", "memoryId"]);
  const content = stringField(entry, ["content", "text", "summary", "body"]);
  if (!id || !content) return undefined;
  return {
    id,
    content,
    tags: stringArray(entry.tags),
    pinned: Boolean(entry.pinned),
    starred: Boolean(entry.starred),
    layer: stringField(entry, ["layer"]),
    confidence: stringField(entry, ["confidence"]),
    source: stringField(entry, ["source"]),
    createdAt: stringField(entry, ["createdAt", "created_at", "created"]),
    updatedAt: stringField(entry, ["updatedAt", "updated_at"]),
    strength: numberField(entry, ["strength"]),
    retrievalStrength: numberField(entry, ["retrievalStrength", "retrieval_strength"]),
    retrievalCount: coerceMemoryCount(entry.retrievalCount ?? entry.retrieval_count ?? entry.useCount ?? entry.use_count),
    lastRetrievedAt: stringField(entry, ["lastRetrievedAt", "last_retrieved_at", "retrieved_at", "lastUsedAt", "last_used_at"]),
    supersededBy: stringField(entry, ["supersededBy", "superseded_by"]),
    parents: stringArray(entry.parents),
  };
}

(async () => {
  const [modulePath, cwd, includeSupersededRaw, pinnedOnlyRaw, limitRaw] = process.argv.slice(1);
  const hoppi = await import(pathToFileURL(path.resolve(modulePath)).href);
  if (typeof hoppi.createHoppiBackend !== "function") {
    throw new Error("Installed Hoppi package does not expose createHoppiBackend().");
  }
  const root = typeof hoppi.getDefaultHoppiRoot === "function"
    ? hoppi.getDefaultHoppiRoot()
    : (process.env.HOPPI_HOME ? path.resolve(process.env.HOPPI_HOME) : path.join(os.homedir(), ".oppi", "hoppi"));
  const backend = hoppi.createHoppiBackend({ root });
  if (!backend || typeof backend !== "object") {
    throw new Error("Installed Hoppi package returned an invalid backend.");
  }
  if (typeof backend.init === "function") await backend.init();
  if (typeof backend.list !== "function") {
    throw new Error("Installed Hoppi package does not expose backend.list().");
  }
  const resolvedCwd = path.resolve(cwd);
  const project = { cwd: resolvedCwd, displayName: path.basename(resolvedCwd) || undefined };
  const limitNumber = Number(limitRaw);
  const input = {
    project,
    includeSuperseded: includeSupersededRaw === "true",
    pinnedOnly: pinnedOnlyRaw === "true",
  };
  if (Number.isInteger(limitNumber) && limitNumber > 0) input.limit = Math.min(limitNumber, 500);
  const rawRecords = await backend.list(input);
  const records = Array.isArray(rawRecords)
    ? rawRecords.map(normalizeRecord).filter(Boolean)
    : [];
  let memoryCount = records.length;
  let pinnedCount = records.filter((record) => record.pinned || record.starred).length;
  let storePath;
  if (typeof backend.status === "function") {
    try {
      const status = await backend.status(project);
      memoryCount = Math.max(memoryCount, coerceMemoryCount(status && status.memoryCount));
      pinnedCount = Math.max(pinnedCount, coerceMemoryCount(status && status.pinnedCount));
      if (status && status.storePath) storePath = String(status.storePath);
    } catch {
      // List records are useful even when status is unavailable.
    }
  }
  console.log(JSON.stringify({ records, memoryCount, pinnedCount, storePath }));
})().catch((error) => {
  console.error(error && error.stack ? error.stack : String(error));
  process.exit(1);
});
"#;

const HOPPI_FORGET_NODE_SCRIPT: &str = r#"
const path = require("node:path");
const os = require("node:os");
const { pathToFileURL } = require("node:url");

function coerceMemoryCount(value) {
  const numeric = Number(value);
  return Number.isFinite(numeric) && numeric >= 0 ? Math.floor(numeric) : undefined;
}

(async () => {
  const [modulePath, cwd, id] = process.argv.slice(1);
  const memoryId = String(id || "").trim();
  if (!memoryId) throw new Error("memory id is required");
  const hoppi = await import(pathToFileURL(path.resolve(modulePath)).href);
  if (typeof hoppi.createHoppiBackend !== "function") {
    throw new Error("Installed Hoppi package does not expose createHoppiBackend().");
  }
  const root = typeof hoppi.getDefaultHoppiRoot === "function"
    ? hoppi.getDefaultHoppiRoot()
    : (process.env.HOPPI_HOME ? path.resolve(process.env.HOPPI_HOME) : path.join(os.homedir(), ".oppi", "hoppi"));
  const backend = hoppi.createHoppiBackend({ root });
  if (!backend || typeof backend !== "object") {
    throw new Error("Installed Hoppi package returned an invalid backend.");
  }
  if (typeof backend.init === "function") await backend.init();
  if (typeof backend.forget !== "function") {
    throw new Error("Installed Hoppi package does not expose backend.forget().");
  }
  const resolvedCwd = path.resolve(cwd);
  const project = { cwd: resolvedCwd, displayName: path.basename(resolvedCwd) || undefined };
  await backend.forget(memoryId);
  let memoryCount;
  let pinnedCount;
  let storePath;
  if (typeof backend.status === "function") {
    try {
      const status = await backend.status(project);
      memoryCount = coerceMemoryCount(status && status.memoryCount);
      pinnedCount = coerceMemoryCount(status && status.pinnedCount);
      if (status && status.storePath) storePath = String(status.storePath);
    } catch {
      // Forget already succeeded; status refresh is best-effort.
    }
  }
  console.log(JSON.stringify({ id: memoryId, memoryCount, pinnedCount, storePath }));
})().catch((error) => {
  console.error(error && error.stack ? error.stack : String(error));
  process.exit(1);
});
"#;

const HOPPI_PIN_NODE_SCRIPT: &str = r#"
const path = require("node:path");
const os = require("node:os");
const { pathToFileURL } = require("node:url");

function coerceMemoryCount(value) {
  const numeric = Number(value);
  return Number.isFinite(numeric) && numeric >= 0 ? Math.floor(numeric) : undefined;
}

function stringField(entry, keys) {
  for (const key of keys) {
    const value = entry && entry[key];
    if (typeof value === "string" && value.trim()) return value.trim();
  }
  return undefined;
}

function numberField(entry, keys) {
  for (const key of keys) {
    const value = Number(entry && entry[key]);
    if (Number.isFinite(value)) return value;
  }
  return undefined;
}

function stringArray(value) {
  return Array.isArray(value)
    ? value.map((item) => String(item || "").trim()).filter(Boolean)
    : [];
}

function normalizeRecord(entry, fallbackId, pinned) {
  if (!entry || typeof entry !== "object") return undefined;
  const id = stringField(entry, ["id", "memoryId"]) || fallbackId;
  const content = stringField(entry, ["content", "text", "summary", "body"]);
  if (!id || !content) return undefined;
  return {
    id,
    content,
    tags: stringArray(entry.tags),
    pinned: Boolean(entry.pinned ?? pinned),
    starred: Boolean(entry.starred),
    layer: stringField(entry, ["layer"]),
    confidence: stringField(entry, ["confidence"]),
    source: stringField(entry, ["source"]),
    createdAt: stringField(entry, ["createdAt", "created_at", "created"]),
    updatedAt: stringField(entry, ["updatedAt", "updated_at"]),
    strength: numberField(entry, ["strength"]),
    retrievalStrength: numberField(entry, ["retrievalStrength", "retrieval_strength"]),
    retrievalCount: coerceMemoryCount(entry.retrievalCount ?? entry.retrieval_count ?? entry.useCount ?? entry.use_count),
    lastRetrievedAt: stringField(entry, ["lastRetrievedAt", "last_retrieved_at", "retrieved_at", "lastUsedAt", "last_used_at"]),
    supersededBy: stringField(entry, ["supersededBy", "superseded_by"]),
    parents: stringArray(entry.parents),
  };
}

(async () => {
  const [modulePath, cwd, id, pinnedRaw] = process.argv.slice(1);
  const memoryId = String(id || "").trim();
  if (!memoryId) throw new Error("memory id is required");
  const pinned = pinnedRaw === "true";
  const hoppi = await import(pathToFileURL(path.resolve(modulePath)).href);
  if (typeof hoppi.createHoppiBackend !== "function") {
    throw new Error("Installed Hoppi package does not expose createHoppiBackend().");
  }
  const root = typeof hoppi.getDefaultHoppiRoot === "function"
    ? hoppi.getDefaultHoppiRoot()
    : (process.env.HOPPI_HOME ? path.resolve(process.env.HOPPI_HOME) : path.join(os.homedir(), ".oppi", "hoppi"));
  const backend = hoppi.createHoppiBackend({ root });
  if (!backend || typeof backend !== "object") {
    throw new Error("Installed Hoppi package returned an invalid backend.");
  }
  if (typeof backend.init === "function") await backend.init();
  if (typeof backend.pin !== "function") {
    throw new Error("Installed Hoppi package does not expose backend.pin().");
  }
  const resolvedCwd = path.resolve(cwd);
  const project = { cwd: resolvedCwd, displayName: path.basename(resolvedCwd) || undefined };
  const rawRecord = await backend.pin(memoryId, pinned);
  const record = normalizeRecord(rawRecord, memoryId, pinned);
  let memoryCount;
  let pinnedCount;
  let storePath;
  if (typeof backend.status === "function") {
    try {
      const status = await backend.status(project);
      memoryCount = coerceMemoryCount(status && status.memoryCount);
      pinnedCount = coerceMemoryCount(status && status.pinnedCount);
      if (status && status.storePath) storePath = String(status.storePath);
    } catch {
      // Pin already succeeded; status refresh is best-effort.
    }
  }
  console.log(JSON.stringify({ id: memoryId, pinned, record, memoryCount, pinnedCount, storePath }));
})().catch((error) => {
  console.error(error && error.stack ? error.stack : String(error));
  process.exit(1);
});
"#;

const HOPPI_UPDATE_NODE_SCRIPT: &str = r#"
const path = require("node:path");
const os = require("node:os");
const { pathToFileURL } = require("node:url");

function coerceMemoryCount(value) {
  const numeric = Number(value);
  return Number.isFinite(numeric) && numeric >= 0 ? Math.floor(numeric) : undefined;
}

function stringField(entry, keys) {
  for (const key of keys) {
    const value = entry && entry[key];
    if (typeof value === "string" && value.trim()) return value.trim();
  }
  return undefined;
}

function numberField(entry, keys) {
  for (const key of keys) {
    const value = Number(entry && entry[key]);
    if (Number.isFinite(value)) return value;
  }
  return undefined;
}

function stringArray(value) {
  return Array.isArray(value)
    ? value.map((item) => String(item || "").trim()).filter(Boolean)
    : [];
}

function normalizeRecord(entry, fallbackId) {
  if (!entry || typeof entry !== "object") return undefined;
  const id = stringField(entry, ["id", "memoryId"]) || fallbackId;
  const content = stringField(entry, ["content", "text", "summary", "body"]);
  if (!id || !content) return undefined;
  return {
    id,
    content,
    tags: stringArray(entry.tags),
    pinned: Boolean(entry.pinned),
    starred: Boolean(entry.starred),
    layer: stringField(entry, ["layer"]),
    confidence: stringField(entry, ["confidence"]),
    source: stringField(entry, ["source"]),
    createdAt: stringField(entry, ["createdAt", "created_at", "created"]),
    updatedAt: stringField(entry, ["updatedAt", "updated_at"]),
    strength: numberField(entry, ["strength"]),
    retrievalStrength: numberField(entry, ["retrievalStrength", "retrieval_strength"]),
    retrievalCount: coerceMemoryCount(entry.retrievalCount ?? entry.retrieval_count ?? entry.useCount ?? entry.use_count),
    lastRetrievedAt: stringField(entry, ["lastRetrievedAt", "last_retrieved_at", "retrieved_at", "lastUsedAt", "last_used_at"]),
    supersededBy: stringField(entry, ["supersededBy", "superseded_by"]),
    parents: stringArray(entry.parents),
  };
}

(async () => {
  const [modulePath, cwd, id, patchRaw] = process.argv.slice(1);
  const memoryId = String(id || "").trim();
  if (!memoryId) throw new Error("memory id is required");
  const patch = JSON.parse(String(patchRaw || "{}"));
  if (!patch || typeof patch !== "object" || Array.isArray(patch) || Object.keys(patch).length === 0) {
    throw new Error("memory update patch must be a non-empty object");
  }
  const hoppi = await import(pathToFileURL(path.resolve(modulePath)).href);
  if (typeof hoppi.createHoppiBackend !== "function") {
    throw new Error("Installed Hoppi package does not expose createHoppiBackend().");
  }
  const root = typeof hoppi.getDefaultHoppiRoot === "function"
    ? hoppi.getDefaultHoppiRoot()
    : (process.env.HOPPI_HOME ? path.resolve(process.env.HOPPI_HOME) : path.join(os.homedir(), ".oppi", "hoppi"));
  const backend = hoppi.createHoppiBackend({ root });
  if (!backend || typeof backend !== "object") {
    throw new Error("Installed Hoppi package returned an invalid backend.");
  }
  if (typeof backend.init === "function") await backend.init();
  if (typeof backend.update !== "function") {
    throw new Error("Installed Hoppi package does not expose backend.update().");
  }
  const resolvedCwd = path.resolve(cwd);
  const project = { cwd: resolvedCwd, displayName: path.basename(resolvedCwd) || undefined };
  const rawRecord = await backend.update(memoryId, patch);
  const record = normalizeRecord(rawRecord, memoryId);
  let memoryCount;
  let pinnedCount;
  let storePath;
  if (typeof backend.status === "function") {
    try {
      const status = await backend.status(project);
      memoryCount = coerceMemoryCount(status && status.memoryCount);
      pinnedCount = coerceMemoryCount(status && status.pinnedCount);
      if (status && status.storePath) storePath = String(status.storePath);
    } catch {
      // Update already succeeded; status refresh is best-effort.
    }
  }
  console.log(JSON.stringify({ id: memoryId, record, memoryCount, pinnedCount, storePath }));
})().catch((error) => {
  console.error(error && error.stack ? error.stack : String(error));
  process.exit(1);
});
"#;

const HOPPI_SYNC_NODE_SCRIPT: &str = r#"
const path = require("node:path");
const os = require("node:os");
const { pathToFileURL } = require("node:url");

(async () => {
  const [modulePath, repoPath, direction, conflictMode, message, remote] = process.argv.slice(1);
  const hoppi = await import(pathToFileURL(path.resolve(modulePath)).href);
  if (typeof hoppi.syncGitRepository !== "function") {
    throw new Error("Installed Hoppi package does not expose syncGitRepository().");
  }
  const root = typeof hoppi.getDefaultHoppiRoot === "function"
    ? hoppi.getDefaultHoppiRoot()
    : (process.env.HOPPI_HOME ? path.resolve(process.env.HOPPI_HOME) : path.join(os.homedir(), ".oppi", "hoppi"));
  const options = { direction, conflictMode, message };
  if (remote) options.remote = remote;
  if (process.env.OPPI_HOPPI_SYNC_RUNTIME_PASSPHRASE) {
    options.passphrase = process.env.OPPI_HOPPI_SYNC_RUNTIME_PASSPHRASE;
  }
  const result = await hoppi.syncGitRepository(root, repoPath, options);
  let rebuiltEmbeddings = false;
  if ((direction === "pull" || direction === "both")
      && result && result.embeddingsRebuildRecommended
      && typeof hoppi.rebuildEmbeddings === "function") {
    const rebuilt = await hoppi.rebuildEmbeddings(root);
    rebuiltEmbeddings = !(rebuilt && rebuilt.error);
  }
  console.log(JSON.stringify({ ...(result || {}), rebuiltEmbeddings }));
})().catch((error) => {
  console.error(error && error.stack ? error.stack : String(error));
  process.exit(1);
});
"#;

fn format_hoppi_bridge_timeout(timeout: Duration) -> String {
    let millis = timeout.as_millis();
    if millis >= 1_000 && millis % 1_000 == 0 {
        format!("{}s", millis / 1_000)
    } else {
        format!("{millis}ms")
    }
}

fn read_process_pipe_async<R: Read + Send + 'static>(mut pipe: R) -> mpsc::Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = pipe.read_to_end(&mut bytes);
        let _ = tx.send(bytes);
    });
    rx
}

fn process_pipe_bytes(rx: &mpsc::Receiver<Vec<u8>>) -> Vec<u8> {
    rx.recv_timeout(Duration::from_secs(1)).unwrap_or_default()
}

fn run_hoppi_node_output(
    command: &mut Command,
    label: &str,
    timeout: Duration,
) -> Result<std::process::Output, String> {
    let timeout = std::cmp::max(timeout, Duration::from_millis(1));
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|error| format!("spawn node {label}: {error}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| format!("{label} stdout was not captured"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| format!("{label} stderr was not captured"))?;
    let stdout_rx = read_process_pipe_async(stdout);
    let stderr_rx = read_process_pipe_async(stderr);
    let started = Instant::now();

    loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|error| format!("wait for node {label}: {error}"))?
        {
            return Ok(std::process::Output {
                status,
                stdout: process_pipe_bytes(&stdout_rx),
                stderr: process_pipe_bytes(&stderr_rx),
            });
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            let stdout = process_pipe_bytes(&stdout_rx);
            let stderr = process_pipe_bytes(&stderr_rx);
            let tail = compact_process_output(&[stdout, stderr].concat());
            return Err(format!(
                "{label} timed out after {}{}{}",
                format_hoppi_bridge_timeout(timeout),
                if tail.is_empty() { "" } else { ": " },
                tail
            ));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn run_hoppi_dashboard(
    settings: &MemorySettings,
    cwd: &Path,
    module_path: PathBuf,
) -> Result<HoppiDashboardProcess, String> {
    let invocation = hoppi_dashboard_invocation_from_settings(settings, cwd, module_path)?;
    run_hoppi_dashboard_invocation(&invocation)
}

fn run_hoppi_dashboard_invocation(
    invocation: &HoppiDashboardInvocation,
) -> Result<HoppiDashboardProcess, String> {
    let mut child = Command::new(if cfg!(windows) { "node.exe" } else { "node" })
        .arg("-e")
        .arg(HOPPI_DASHBOARD_NODE_SCRIPT)
        .arg(&invocation.module_path)
        .arg(&invocation.cwd)
        .arg(
            invocation
                .port
                .map(|port| port.to_string())
                .unwrap_or_else(|| "auto".to_string()),
        )
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("spawn node Hoppi dashboard: {error}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Hoppi dashboard stdout was not captured".to_string())?;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        let read = reader.read_line(&mut line);
        let _ = tx.send((read, line));
    });

    let (read, line) = match rx.recv_timeout(Duration::from_secs(10)) {
        Ok(value) => value,
        Err(mpsc::RecvTimeoutError::Timeout) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Hoppi dashboard did not report a URL within 10s".to_string());
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Hoppi dashboard exited before reporting a URL".to_string());
        }
    };
    match read {
        Ok(0) => {
            let tail = child
                .stderr
                .take()
                .and_then(|mut stderr| {
                    let mut text = Vec::new();
                    stderr.read_to_end(&mut text).ok().map(|_| text)
                })
                .map(|text| compact_process_output(&text))
                .unwrap_or_default();
            let status = child.try_wait().ok().flatten();
            let _ = child.kill();
            let _ = child.wait();
            Err(format!(
                "Hoppi dashboard returned no JSON output{}{}{}",
                status
                    .map(|status| format!(" ({status})"))
                    .unwrap_or_default(),
                if tail.is_empty() { "" } else { ": " },
                tail
            ))
        }
        Ok(_) => {
            let result = parse_hoppi_dashboard_output(&line)?;
            Ok(HoppiDashboardProcess { result, child })
        }
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            Err(format!("read Hoppi dashboard output: {error}"))
        }
    }
}

fn parse_hoppi_dashboard_output(output: &str) -> Result<HoppiDashboardCommandResult, String> {
    let line = output
        .lines()
        .find(|line| !line.trim().is_empty())
        .ok_or_else(|| "Hoppi dashboard returned no JSON output".to_string())?;
    let value: Value = serde_json::from_str(line.trim())
        .map_err(|error| format!("parse Hoppi dashboard output: {error}"))?;
    let url = value
        .get("url")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|url| !url.is_empty())
        .ok_or_else(|| "Hoppi dashboard output missing url".to_string())?
        .to_string();
    Ok(HoppiDashboardCommandResult {
        fallback_url: value
            .get("fallbackUrl")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|url| !url.is_empty())
            .map(str::to_string),
        url,
        memory_count: value
            .get("memoryCount")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        pinned_count: value
            .get("pinnedCount")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        store_path: value
            .get("storePath")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|path| !path.is_empty())
            .map(str::to_string),
    })
}

fn run_hoppi_list_invocation(
    invocation: &HoppiListInvocation,
) -> Result<HoppiListCommandResult, String> {
    run_hoppi_list_invocation_with_timeout(invocation, HOPPI_BRIDGE_TIMEOUT)
}

fn run_hoppi_list_invocation_with_timeout(
    invocation: &HoppiListInvocation,
    timeout: Duration,
) -> Result<HoppiListCommandResult, String> {
    let mut command = Command::new(if cfg!(windows) { "node.exe" } else { "node" });
    command
        .arg("-e")
        .arg(HOPPI_LIST_NODE_SCRIPT)
        .arg(&invocation.module_path)
        .arg(&invocation.cwd)
        .arg(if invocation.include_superseded {
            "true"
        } else {
            "false"
        })
        .arg(if invocation.pinned_only {
            "true"
        } else {
            "false"
        })
        .arg(
            invocation
                .limit
                .map(|limit| limit.to_string())
                .unwrap_or_else(|| "0".to_string()),
        );
    let output = run_hoppi_node_output(&mut command, "Hoppi list", timeout)?;
    if !output.status.success() {
        let tail = compact_process_output(&[output.stdout, output.stderr].concat());
        return Err(format!(
            "Hoppi list failed with status {}{}{}",
            output.status,
            if tail.is_empty() { "" } else { ": " },
            tail
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_hoppi_list_output(&stdout)
}

fn parse_hoppi_list_output(output: &str) -> Result<HoppiListCommandResult, String> {
    let line = output
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .ok_or_else(|| "Hoppi list returned no JSON output".to_string())?;
    let value: Value = serde_json::from_str(line.trim())
        .map_err(|error| format!("parse Hoppi list output: {error}"))?;
    let records = value
        .get("records")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|record| {
            serde_json::from_value::<MemoryRecord>(record)
                .map_err(|error| format!("parse Hoppi memory record: {error}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(HoppiListCommandResult {
        records,
        memory_count: value
            .get("memoryCount")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        pinned_count: value
            .get("pinnedCount")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        store_path: value
            .get("storePath")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|path| !path.is_empty())
            .map(str::to_string),
    })
}

fn run_hoppi_forget_invocation(
    invocation: &HoppiForgetInvocation,
) -> Result<HoppiForgetCommandResult, String> {
    let mut command = Command::new(if cfg!(windows) { "node.exe" } else { "node" });
    command
        .arg("-e")
        .arg(HOPPI_FORGET_NODE_SCRIPT)
        .arg(&invocation.module_path)
        .arg(&invocation.cwd)
        .arg(&invocation.id);
    let output = run_hoppi_node_output(&mut command, "Hoppi forget", HOPPI_BRIDGE_TIMEOUT)?;
    if !output.status.success() {
        let tail = compact_process_output(&[output.stdout, output.stderr].concat());
        return Err(format!(
            "Hoppi forget failed with status {}{}{}",
            output.status,
            if tail.is_empty() { "" } else { ": " },
            tail
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_hoppi_forget_output(&stdout)
}

fn parse_hoppi_forget_output(output: &str) -> Result<HoppiForgetCommandResult, String> {
    let line = output
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .ok_or_else(|| "Hoppi forget returned no JSON output".to_string())?;
    let value: Value = serde_json::from_str(line.trim())
        .map_err(|error| format!("parse Hoppi forget output: {error}"))?;
    let id = value
        .get("id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .ok_or_else(|| "Hoppi forget output missing id".to_string())?
        .to_string();
    Ok(HoppiForgetCommandResult {
        id,
        memory_count: value.get("memoryCount").and_then(Value::as_u64),
        pinned_count: value.get("pinnedCount").and_then(Value::as_u64),
        store_path: value
            .get("storePath")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|path| !path.is_empty())
            .map(str::to_string),
    })
}

fn run_hoppi_pin_invocation(
    invocation: &HoppiPinInvocation,
) -> Result<HoppiPinCommandResult, String> {
    let mut command = Command::new(if cfg!(windows) { "node.exe" } else { "node" });
    command
        .arg("-e")
        .arg(HOPPI_PIN_NODE_SCRIPT)
        .arg(&invocation.module_path)
        .arg(&invocation.cwd)
        .arg(&invocation.id)
        .arg(if invocation.pinned { "true" } else { "false" });
    let output = run_hoppi_node_output(&mut command, "Hoppi pin", HOPPI_BRIDGE_TIMEOUT)?;
    if !output.status.success() {
        let tail = compact_process_output(&[output.stdout, output.stderr].concat());
        return Err(format!(
            "Hoppi pin failed with status {}{}{}",
            output.status,
            if tail.is_empty() { "" } else { ": " },
            tail
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_hoppi_pin_output(&stdout)
}

fn parse_hoppi_pin_output(output: &str) -> Result<HoppiPinCommandResult, String> {
    let line = output
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .ok_or_else(|| "Hoppi pin returned no JSON output".to_string())?;
    let value: Value = serde_json::from_str(line.trim())
        .map_err(|error| format!("parse Hoppi pin output: {error}"))?;
    let id = value
        .get("id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .ok_or_else(|| "Hoppi pin output missing id".to_string())?
        .to_string();
    let record = value
        .get("record")
        .cloned()
        .filter(|value| !value.is_null())
        .map(|record| {
            serde_json::from_value::<MemoryRecord>(record)
                .map_err(|error| format!("parse Hoppi pinned memory record: {error}"))
        })
        .transpose()?;
    Ok(HoppiPinCommandResult {
        id,
        pinned: value
            .get("pinned")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        record,
        memory_count: value.get("memoryCount").and_then(Value::as_u64),
        pinned_count: value.get("pinnedCount").and_then(Value::as_u64),
        store_path: value
            .get("storePath")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|path| !path.is_empty())
            .map(str::to_string),
    })
}

fn run_hoppi_update_invocation(
    invocation: &HoppiUpdateInvocation,
) -> Result<HoppiUpdateCommandResult, String> {
    let patch_json = serde_json::to_string(&invocation.patch)
        .map_err(|error| format!("serialize Hoppi update patch: {error}"))?;
    let mut command = Command::new(if cfg!(windows) { "node.exe" } else { "node" });
    command
        .arg("-e")
        .arg(HOPPI_UPDATE_NODE_SCRIPT)
        .arg(&invocation.module_path)
        .arg(&invocation.cwd)
        .arg(&invocation.id)
        .arg(patch_json);
    let output = run_hoppi_node_output(&mut command, "Hoppi update", HOPPI_BRIDGE_TIMEOUT)?;
    if !output.status.success() {
        let tail = compact_process_output(&[output.stdout, output.stderr].concat());
        return Err(format!(
            "Hoppi update failed with status {}{}{}",
            output.status,
            if tail.is_empty() { "" } else { ": " },
            tail
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_hoppi_update_output(&stdout)
}

fn parse_hoppi_update_output(output: &str) -> Result<HoppiUpdateCommandResult, String> {
    let line = output
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .ok_or_else(|| "Hoppi update returned no JSON output".to_string())?;
    let value: Value = serde_json::from_str(line.trim())
        .map_err(|error| format!("parse Hoppi update output: {error}"))?;
    let id = value
        .get("id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .ok_or_else(|| "Hoppi update output missing id".to_string())?
        .to_string();
    let record = value
        .get("record")
        .cloned()
        .filter(|value| !value.is_null())
        .map(|record| {
            serde_json::from_value::<MemoryRecord>(record)
                .map_err(|error| format!("parse Hoppi updated memory record: {error}"))
        })
        .transpose()?;
    Ok(HoppiUpdateCommandResult {
        id,
        record,
        memory_count: value.get("memoryCount").and_then(Value::as_u64),
        pinned_count: value.get("pinnedCount").and_then(Value::as_u64),
        store_path: value
            .get("storePath")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|path| !path.is_empty())
            .map(str::to_string),
    })
}

const HOPPI_CONSOLIDATE_NODE_SCRIPT: &str = r#"
const path = require("node:path");
const os = require("node:os");
const { pathToFileURL } = require("node:url");

function coerceCount(value) {
  const numeric = Number(value);
  return Number.isFinite(numeric) && numeric >= 0 ? Math.floor(numeric) : undefined;
}

function stringField(entry, keys) {
  for (const key of keys) {
    const value = entry && entry[key];
    if (typeof value === "string" && value.trim()) return value.trim();
  }
  return undefined;
}

function numberField(entry, keys) {
  for (const key of keys) {
    const value = Number(entry && entry[key]);
    if (Number.isFinite(value)) return value;
  }
  return undefined;
}

function stringArray(value) {
  return Array.isArray(value)
    ? value.map((item) => String(item || "").trim()).filter(Boolean)
    : [];
}

function normalizeRecord(entry) {
  const id = stringField(entry, ["id", "memoryId"]);
  const content = stringField(entry, ["content", "text", "summary", "body"]);
  if (!id || !content) return undefined;
  return {
    id,
    content,
    tags: stringArray(entry && entry.tags),
    pinned: Boolean(entry && entry.pinned),
    starred: Boolean(entry && entry.starred),
    layer: stringField(entry, ["layer"]),
    confidence: stringField(entry, ["confidence"]),
    source: stringField(entry, ["source"]),
    createdAt: stringField(entry, ["createdAt", "created_at", "created"]),
    updatedAt: stringField(entry, ["updatedAt", "updated_at"]),
    strength: numberField(entry, ["strength"]),
    retrievalStrength: numberField(entry, ["retrievalStrength", "retrieval_strength"]),
    retrievalCount: coerceCount(entry && (entry.retrievalCount ?? entry.retrieval_count ?? entry.useCount ?? entry.use_count)),
    lastRetrievedAt: stringField(entry, ["lastRetrievedAt", "last_retrieved_at", "retrieved_at", "lastUsedAt", "last_used_at"]),
    supersededBy: stringField(entry, ["supersededBy", "superseded_by"]),
    parents: stringArray(entry && entry.parents),
  };
}

function firstArray(value, keys) {
  for (const key of keys) {
    if (Array.isArray(value && value[key])) return value[key];
  }
  return [];
}

(async () => {
  const [modulePath, cwd, dryRunRaw, stageRaw, limitRaw, budgetRaw, agentModelRaw, deepModelRaw] = process.argv.slice(1);
  const dryRun = dryRunRaw === "true";
  const stage = String(stageRaw || "manual-maintenance").trim() || "manual-maintenance";
  const limit = coerceCount(limitRaw);
  const budget = coerceCount(budgetRaw);
  const modelPreference = {
    agentModel: typeof agentModelRaw === "string" && agentModelRaw.trim() ? agentModelRaw.trim() : "auto",
    deepModel: typeof deepModelRaw === "string" && deepModelRaw.trim() ? deepModelRaw.trim() : "auto"
  };
  const hoppi = await import(pathToFileURL(path.resolve(modulePath)).href);
  if (typeof hoppi.createHoppiBackend !== "function") {
    throw new Error("Installed Hoppi package does not expose createHoppiBackend().");
  }
  const root = typeof hoppi.getDefaultHoppiRoot === "function"
    ? hoppi.getDefaultHoppiRoot()
    : (process.env.HOPPI_HOME ? path.resolve(process.env.HOPPI_HOME) : path.join(os.homedir(), ".oppi", "hoppi"));
  const backend = hoppi.createHoppiBackend({ root });
  if (!backend || typeof backend !== "object") {
    throw new Error("Installed Hoppi package returned an invalid backend.");
  }
  if (typeof backend.init === "function") await backend.init();
  if (typeof backend.consolidate !== "function") {
    throw new Error("Installed Hoppi package does not expose backend.consolidate().");
  }
  const resolvedCwd = path.resolve(cwd);
  const project = { cwd: resolvedCwd, displayName: path.basename(resolvedCwd) || undefined };
  const input = { project, dryRun, stage, source: "oppi:native-shell", modelPreference };
  if (limit !== undefined && limit > 0) input.limit = limit;
  if (budget !== undefined && budget > 0) input.budget = budget;
  const result = (await backend.consolidate(input)) || {};
  let status = result.status && typeof result.status === "object" ? result.status : undefined;
  if (typeof backend.status === "function") {
    try {
      status = { ...(status || {}), ...((await backend.status(project)) || {}) };
    } catch {
      // Consolidation is the main result; status is only used for observability.
    }
  }
  const records = firstArray(result, ["records", "memories", "created", "consolidated"])
    .map(normalizeRecord)
    .filter(Boolean);
  const deletedIds = stringArray(result.deletedIds || result.deleted_ids || result.deleted || result.removedIds);
  console.log(JSON.stringify({
    dryRun: Boolean(result.dryRun ?? dryRun),
    modelUsed: stringField(result, ["modelUsed", "model_used", "model"]) || stringField(status, ["modelUsed", "model_used", "model"]),
    summary: stringField(result, ["summary", "message"]),
    records,
    deletedIds,
    memoryCount: coerceCount(result.memoryCount ?? result.memory_count ?? (status && (status.memoryCount ?? status.memory_count))) ?? records.length,
    pinnedCount: coerceCount(result.pinnedCount ?? result.pinned_count ?? (status && (status.pinnedCount ?? status.pinned_count))),
    storePath: stringField(result, ["storePath", "store_path"]) || stringField(status, ["storePath", "store_path"]),
    lastConsolidatedAt: stringField(result, ["lastConsolidatedAt", "last_consolidated_at"]) || stringField(status, ["lastConsolidatedAt", "last_consolidated_at"]),
    queueDepth: coerceCount(result.queueDepth ?? result.queue_depth ?? (status && (status.queueDepth ?? status.queue_depth))),
  }));
})().catch((error) => {
  console.error(error && error.stack ? error.stack : String(error));
  process.exit(1);
});
"#;

fn sanitize_memory_job_component(input: &str) -> String {
    let mut output = String::new();
    let mut last_dash = false;
    for character in input.chars() {
        let next = if character.is_ascii_alphanumeric() {
            last_dash = false;
            Some(character.to_ascii_lowercase())
        } else if !last_dash {
            last_dash = true;
            Some('-')
        } else {
            None
        };
        if let Some(next) = next {
            output.push(next);
        }
    }
    let output = output.trim_matches('-');
    if output.is_empty() {
        "job".to_string()
    } else {
        output.to_string()
    }
}

fn next_native_memory_job_id(prefix: &str, stage: &str) -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    format!(
        "native-{}-{}-{}-{}",
        sanitize_memory_job_component(prefix),
        sanitize_memory_job_component(stage),
        std::process::id(),
        timestamp
    )
}

fn hoppi_dashboard_job_stage() -> &'static str {
    "dashboard-start"
}

fn hoppi_dashboard_job_started_params(
    thread_id: &str,
    job_id: &str,
    _invocation: &HoppiDashboardInvocation,
) -> Value {
    json!({
        "threadId": thread_id,
        "jobId": job_id,
        "kind": "dashboard",
        "stage": hoppi_dashboard_job_stage(),
        "status": "running",
        "source": "hoppi-dashboard",
        "message": "Starting Hoppi dashboard.",
        "backendApplied": false,
    })
}

fn hoppi_dashboard_job_finished_params(
    thread_id: &str,
    job_id: &str,
    _invocation: &HoppiDashboardInvocation,
    result: &HoppiDashboardCommandResult,
) -> Value {
    json!({
        "threadId": thread_id,
        "jobId": job_id,
        "kind": "dashboard",
        "stage": hoppi_dashboard_job_stage(),
        "status": "succeeded",
        "source": "hoppi-dashboard",
        "message": format!("Hoppi dashboard ready: {}.", result.url),
        "recordCount": result.memory_count,
        "backendApplied": true,
    })
}

fn hoppi_dashboard_job_failed_params(
    thread_id: &str,
    job_id: &str,
    _invocation: &HoppiDashboardInvocation,
    error: &str,
) -> Value {
    json!({
        "threadId": thread_id,
        "jobId": job_id,
        "kind": "dashboard",
        "stage": hoppi_dashboard_job_stage(),
        "status": "failed",
        "source": "hoppi-dashboard",
        "message": format!("Hoppi dashboard failed: {error}"),
        "backendApplied": false,
    })
}

fn hoppi_list_job_stage(invocation: &HoppiListInvocation) -> String {
    match (invocation.include_superseded, invocation.pinned_only) {
        (true, true) => "list-pinned-all".to_string(),
        (true, false) => "list-all".to_string(),
        (false, true) => "list-pinned".to_string(),
        (false, false) => "list".to_string(),
    }
}

fn hoppi_list_job_started_params(
    thread_id: &str,
    job_id: &str,
    invocation: &HoppiListInvocation,
) -> Value {
    let stage = hoppi_list_job_stage(invocation);
    json!({
        "threadId": thread_id,
        "jobId": job_id,
        "kind": "list",
        "stage": stage,
        "status": "running",
        "source": "hoppi-list",
        "message": format!("Loading Hoppi {stage}."),
        "backendApplied": false,
    })
}

fn hoppi_list_job_finished_params(
    thread_id: &str,
    job_id: &str,
    invocation: &HoppiListInvocation,
    result: &HoppiListCommandResult,
) -> Value {
    let stage = hoppi_list_job_stage(invocation);
    json!({
        "threadId": thread_id,
        "jobId": job_id,
        "kind": "list",
        "stage": stage,
        "status": "succeeded",
        "source": "hoppi-list",
        "message": format!("Hoppi {stage} loaded {} memories.", result.records.len()),
        "recordCount": result.records.len(),
        "backendApplied": true,
    })
}

fn hoppi_list_job_failed_params(
    thread_id: &str,
    job_id: &str,
    invocation: &HoppiListInvocation,
    error: &str,
) -> Value {
    let stage = hoppi_list_job_stage(invocation);
    json!({
        "threadId": thread_id,
        "jobId": job_id,
        "kind": "list",
        "stage": stage,
        "status": "failed",
        "source": "hoppi-list",
        "message": format!("Hoppi {stage} failed: {error}"),
        "backendApplied": false,
    })
}

fn hoppi_sync_job_stage(invocation: &HoppiSyncInvocation) -> String {
    format!("sync-{}", invocation.direction.as_str())
}

fn hoppi_sync_job_source(reason: &str) -> String {
    format!("hoppi-sync-{}", sanitize_memory_job_component(reason))
}

fn hoppi_sync_job_started_params(
    thread_id: &str,
    job_id: &str,
    invocation: &HoppiSyncInvocation,
    reason: &str,
) -> Value {
    json!({
        "threadId": thread_id,
        "jobId": job_id,
        "kind": "sync",
        "stage": hoppi_sync_job_stage(invocation),
        "status": "running",
        "source": hoppi_sync_job_source(reason),
        "message": invocation.message,
        "backendApplied": false,
    })
}

fn hoppi_sync_job_finished_params(
    thread_id: &str,
    job_id: &str,
    invocation: &HoppiSyncInvocation,
    result: &HoppiSyncCommandResult,
    reason: &str,
) -> Value {
    json!({
        "threadId": thread_id,
        "jobId": job_id,
        "kind": "sync",
        "stage": hoppi_sync_job_stage(invocation),
        "status": "succeeded",
        "source": hoppi_sync_job_source(reason),
        "message": format_hoppi_sync_report(invocation.direction, result),
        "recordCount": result.imported + result.exported,
        "backendApplied": true,
    })
}

fn hoppi_sync_job_failed_params(
    thread_id: &str,
    job_id: &str,
    invocation: &HoppiSyncInvocation,
    error: &str,
    reason: &str,
) -> Value {
    json!({
        "threadId": thread_id,
        "jobId": job_id,
        "kind": "sync",
        "stage": hoppi_sync_job_stage(invocation),
        "status": "failed",
        "source": hoppi_sync_job_source(reason),
        "message": format!("{} failed: {error}", invocation.message),
        "backendApplied": false,
    })
}

fn hoppi_recall_job_stage(invocation: &HoppiRecallInvocation) -> String {
    match (invocation.include_startup, invocation.include_task) {
        (true, true) => "recall-startup-task".to_string(),
        (true, false) => "recall-startup".to_string(),
        (false, true) => "recall-task".to_string(),
        (false, false) => "recall".to_string(),
    }
}

fn hoppi_recall_job_started_params(
    thread_id: &str,
    job_id: &str,
    invocation: &HoppiRecallInvocation,
) -> Value {
    let stage = hoppi_recall_job_stage(invocation);
    json!({
        "threadId": thread_id,
        "jobId": job_id,
        "kind": "recall",
        "stage": stage,
        "status": "running",
        "source": "hoppi-recall",
        "message": format!("Running Hoppi {stage}."),
        "backendApplied": false,
    })
}

fn hoppi_recall_job_finished_params(
    thread_id: &str,
    job_id: &str,
    invocation: &HoppiRecallInvocation,
    result: &HoppiRecallCommandResult,
) -> Value {
    let stage = hoppi_recall_job_stage(invocation);
    json!({
        "threadId": thread_id,
        "jobId": job_id,
        "kind": "recall",
        "stage": stage,
        "status": "succeeded",
        "source": "hoppi-recall",
        "message": format!("Hoppi {stage} finished: {} recalled, {} bytes.", result.records.len(), result.context_bytes),
        "recordCount": result.records.len(),
        "backendApplied": true,
    })
}

fn hoppi_recall_job_failed_params(
    thread_id: &str,
    job_id: &str,
    invocation: &HoppiRecallInvocation,
    error: &str,
) -> Value {
    let stage = hoppi_recall_job_stage(invocation);
    json!({
        "threadId": thread_id,
        "jobId": job_id,
        "kind": "recall",
        "stage": stage,
        "status": "failed",
        "source": "hoppi-recall",
        "message": format!("Hoppi {stage} failed: {error}"),
        "backendApplied": false,
    })
}

fn hoppi_remember_job_stage(reason: &str) -> String {
    sanitize_memory_job_component(reason)
}

fn hoppi_remember_job_source(reason: &str) -> String {
    format!("hoppi-remember-{}", hoppi_remember_job_stage(reason))
}

fn hoppi_remember_job_started_params(
    thread_id: &str,
    job_id: &str,
    _invocation: &HoppiRememberInvocation,
    reason: &str,
) -> Value {
    let stage = hoppi_remember_job_stage(reason);
    json!({
        "threadId": thread_id,
        "jobId": job_id,
        "kind": "remember",
        "stage": stage,
        "status": "running",
        "source": hoppi_remember_job_source(reason),
        "message": format!("Saving Hoppi memory ({stage})."),
        "backendApplied": false,
    })
}

fn hoppi_remember_job_finished_params(
    thread_id: &str,
    job_id: &str,
    _invocation: &HoppiRememberInvocation,
    result: &HoppiRememberCommandResult,
    reason: &str,
) -> Value {
    let stage = hoppi_remember_job_stage(reason);
    let record_count = if result.saved {
        result.record.as_ref().map(|_| 1).unwrap_or(1)
    } else {
        0
    };
    json!({
        "threadId": thread_id,
        "jobId": job_id,
        "kind": "remember",
        "stage": stage,
        "status": "succeeded",
        "source": hoppi_remember_job_source(reason),
        "message": format!("Hoppi remember {stage} saved {record_count} memory."),
        "recordCount": record_count,
        "backendApplied": result.saved,
    })
}

fn hoppi_remember_job_failed_params(
    thread_id: &str,
    job_id: &str,
    _invocation: &HoppiRememberInvocation,
    error: &str,
    reason: &str,
) -> Value {
    let stage = hoppi_remember_job_stage(reason);
    json!({
        "threadId": thread_id,
        "jobId": job_id,
        "kind": "remember",
        "stage": stage,
        "status": "failed",
        "source": hoppi_remember_job_source(reason),
        "message": format!("Hoppi remember {stage} failed: {error}"),
        "backendApplied": false,
    })
}

fn hoppi_record_mutation_job_stage(stage: &str) -> String {
    sanitize_memory_job_component(stage)
}

fn hoppi_record_mutation_job_started_params(
    thread_id: &str,
    job_id: &str,
    stage: &str,
    source: &str,
) -> Value {
    let stage = hoppi_record_mutation_job_stage(stage);
    json!({
        "threadId": thread_id,
        "jobId": job_id,
        "kind": "recordMutation",
        "stage": stage,
        "status": "running",
        "source": source,
        "message": format!("Running Hoppi record {stage}."),
        "backendApplied": false,
    })
}

fn hoppi_record_mutation_job_finished_params(
    thread_id: &str,
    job_id: &str,
    stage: &str,
    source: &str,
    record_count: u64,
    deleted_count: u64,
) -> Value {
    let stage = hoppi_record_mutation_job_stage(stage);
    json!({
        "threadId": thread_id,
        "jobId": job_id,
        "kind": "recordMutation",
        "stage": stage,
        "status": "succeeded",
        "source": source,
        "message": format!("Hoppi record {stage} finished."),
        "recordCount": record_count,
        "deletedCount": deleted_count,
        "backendApplied": true,
    })
}

fn hoppi_record_mutation_job_failed_params(
    thread_id: &str,
    job_id: &str,
    stage: &str,
    source: &str,
    error: &str,
) -> Value {
    let stage = hoppi_record_mutation_job_stage(stage);
    json!({
        "threadId": thread_id,
        "jobId": job_id,
        "kind": "recordMutation",
        "stage": stage,
        "status": "failed",
        "source": source,
        "message": format!("Hoppi record {stage} failed: {error}"),
        "backendApplied": false,
    })
}

fn hoppi_consolidate_job_started_params(
    thread_id: &str,
    job_id: &str,
    invocation: &HoppiConsolidateInvocation,
    source: &str,
) -> Value {
    json!({
        "threadId": thread_id,
        "jobId": job_id,
        "kind": "consolidate",
        "stage": invocation.stage,
        "status": "running",
        "source": source,
        "message": format!("Running Hoppi {}.", invocation.stage),
        "dryRun": invocation.dry_run,
        "backendApplied": false,
    })
}

fn hoppi_consolidate_job_finished_params(
    thread_id: &str,
    job_id: &str,
    invocation: &HoppiConsolidateInvocation,
    result: &HoppiConsolidateCommandResult,
    source: &str,
) -> Value {
    json!({
        "threadId": thread_id,
        "jobId": job_id,
        "kind": "consolidate",
        "stage": invocation.stage,
        "status": "succeeded",
        "source": source,
        "message": result.summary.clone().unwrap_or_else(|| format!("Hoppi {} finished.", invocation.stage)),
        "modelUsed": result.model_used.clone(),
        "recordCount": result.records.len(),
        "deletedCount": result.deleted_ids.len(),
        "dryRun": result.dry_run,
        "backendApplied": !result.dry_run,
    })
}

fn hoppi_consolidate_job_failed_params(
    thread_id: &str,
    job_id: &str,
    invocation: &HoppiConsolidateInvocation,
    error: &str,
    source: &str,
) -> Value {
    json!({
        "threadId": thread_id,
        "jobId": job_id,
        "kind": "consolidate",
        "stage": invocation.stage,
        "status": "failed",
        "source": source,
        "message": format!("Hoppi {} failed: {error}", invocation.stage),
        "dryRun": invocation.dry_run,
        "backendApplied": false,
    })
}

fn run_hoppi_consolidate_invocation(
    invocation: &HoppiConsolidateInvocation,
) -> Result<HoppiConsolidateCommandResult, String> {
    let output = Command::new(if cfg!(windows) { "node.exe" } else { "node" })
        .arg("-e")
        .arg(HOPPI_CONSOLIDATE_NODE_SCRIPT)
        .arg(&invocation.module_path)
        .arg(&invocation.cwd)
        .arg(if invocation.dry_run { "true" } else { "false" })
        .arg(&invocation.stage)
        .arg(
            invocation
                .limit
                .map(|limit| limit.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .arg(
            invocation
                .budget
                .map(|budget| budget.to_string())
                .unwrap_or_else(|| "0".to_string()),
        )
        .arg(&invocation.agent_model)
        .arg(&invocation.deep_model)
        .output()
        .map_err(|error| format!("spawn node Hoppi maintenance: {error}"))?;
    if !output.status.success() {
        let tail = compact_process_output(&[output.stdout, output.stderr].concat());
        return Err(format!(
            "Hoppi maintenance failed with status {}{}{}",
            output.status,
            if tail.is_empty() { "" } else { ": " },
            tail
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_hoppi_consolidate_output(&stdout)
}

fn parse_hoppi_consolidate_output(output: &str) -> Result<HoppiConsolidateCommandResult, String> {
    let line = output
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .ok_or_else(|| "Hoppi maintenance returned no JSON output".to_string())?;
    let value: Value = serde_json::from_str(line.trim())
        .map_err(|error| format!("parse Hoppi maintenance output: {error}"))?;
    let records = value
        .get("records")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|record| {
            serde_json::from_value::<MemoryRecord>(record)
                .map_err(|error| format!("parse Hoppi consolidated memory record: {error}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let deleted_ids = value
        .get("deletedIds")
        .and_then(Value::as_array)
        .map(|ids| {
            ids.iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Ok(HoppiConsolidateCommandResult {
        dry_run: value
            .get("dryRun")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        model_used: value
            .get("modelUsed")
            .or_else(|| value.get("model"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|model| !model.is_empty())
            .map(str::to_string),
        summary: value
            .get("summary")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|summary| !summary.is_empty())
            .map(str::to_string),
        records,
        deleted_ids,
        memory_count: value
            .get("memoryCount")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        pinned_count: value.get("pinnedCount").and_then(Value::as_u64),
        store_path: value
            .get("storePath")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|path| !path.is_empty())
            .map(str::to_string),
        last_consolidated_at: value
            .get("lastConsolidatedAt")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        queue_depth: value.get("queueDepth").and_then(Value::as_u64),
    })
}

fn run_hoppi_recall_invocation(
    invocation: &HoppiRecallInvocation,
) -> Result<HoppiRecallCommandResult, String> {
    let mut command = Command::new(if cfg!(windows) { "node.exe" } else { "node" });
    command
        .arg("-e")
        .arg(HOPPI_RECALL_NODE_SCRIPT)
        .arg(&invocation.module_path)
        .arg(&invocation.cwd)
        .arg(&invocation.prompt)
        .arg(if invocation.include_startup {
            "true"
        } else {
            "false"
        })
        .arg(if invocation.include_task {
            "true"
        } else {
            "false"
        })
        .env(
            "OPPI_HOPPI_MEMORY_CONTEXT_MAX_BYTES",
            HOPPI_MEMORY_CONTEXT_MAX_BYTES.to_string(),
        );
    let output = run_hoppi_node_output(&mut command, "Hoppi recall", HOPPI_BRIDGE_TIMEOUT)?;
    if !output.status.success() {
        let tail = compact_process_output(&[output.stdout, output.stderr].concat());
        return Err(format!(
            "Hoppi recall failed with status {}{}{}",
            output.status,
            if tail.is_empty() { "" } else { ": " },
            tail
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_hoppi_recall_output(&stdout)
}

fn parse_hoppi_recall_output(output: &str) -> Result<HoppiRecallCommandResult, String> {
    let line = output
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .ok_or_else(|| "Hoppi recall returned no JSON output".to_string())?;
    let value: Value = serde_json::from_str(line.trim())
        .map_err(|error| format!("parse Hoppi recall output: {error}"))?;
    let context_markdown = value
        .get("contextMarkdown")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string);
    let context_bytes = value
        .get("contextBytes")
        .and_then(Value::as_u64)
        .unwrap_or_else(|| {
            context_markdown
                .as_ref()
                .map(|text| text.len() as u64)
                .unwrap_or(0)
        });
    let records = value
        .get("records")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|record| {
            serde_json::from_value::<MemoryRecord>(record)
                .map_err(|error| format!("parse Hoppi recalled memory record: {error}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(HoppiRecallCommandResult {
        context_markdown,
        context_bytes,
        memory_count: value
            .get("memoryCount")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        pinned_count: value.get("pinnedCount").and_then(Value::as_u64),
        store_path: value
            .get("storePath")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|path| !path.is_empty())
            .map(str::to_string),
        records,
        omitted_reason: value
            .get("omittedReason")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|reason| !reason.is_empty())
            .map(str::to_string),
        truncated: value
            .get("truncated")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}

const HOPPI_REMEMBER_NODE_SCRIPT: &str = r#"
const path = require("node:path");
const os = require("node:os");
const { pathToFileURL } = require("node:url");

function coerceMemoryCount(value) {
  const numeric = Number(value);
  return Number.isFinite(numeric) && numeric > 0 ? Math.floor(numeric) : 0;
}

function parseTags(raw) {
  try {
    const tags = JSON.parse(raw || "[]");
    if (Array.isArray(tags)) return tags.map((tag) => String(tag || "").trim()).filter(Boolean);
  } catch {
    // Fall through to the conservative default.
  }
  return ["oppi-native-shell"];
}

function stringField(entry, keys) {
  for (const key of keys) {
    const value = entry && entry[key];
    if (typeof value === "string" && value.trim()) return value.trim();
  }
  return undefined;
}

function numberField(entry, keys) {
  for (const key of keys) {
    const value = Number(entry && entry[key]);
    if (Number.isFinite(value)) return value;
  }
  return undefined;
}

function stringArray(value) {
  return Array.isArray(value)
    ? value.map((item) => String(item || "").trim()).filter(Boolean)
    : [];
}

function normalizeRecord(entry, fallback) {
  const id = stringField(entry, ["id", "memoryId"]);
  const content = stringField(entry, ["content", "text", "summary", "body"]) || fallback.content;
  if (!id || !content) return undefined;
  const tags = stringArray(entry && entry.tags);
  return {
    id,
    content,
    tags: tags.length ? tags : fallback.tags,
    pinned: Boolean(entry && entry.pinned),
    starred: Boolean(entry && entry.starred),
    layer: stringField(entry, ["layer"]) || fallback.layer,
    confidence: stringField(entry, ["confidence"]) || fallback.confidence,
    source: stringField(entry, ["source"]) || fallback.source,
    createdAt: stringField(entry, ["createdAt", "created_at", "created"]),
    updatedAt: stringField(entry, ["updatedAt", "updated_at"]),
    strength: numberField(entry, ["strength"]),
    retrievalStrength: numberField(entry, ["retrievalStrength", "retrieval_strength"]),
    retrievalCount: coerceMemoryCount(entry && (entry.retrievalCount ?? entry.retrieval_count ?? entry.useCount ?? entry.use_count)),
    lastRetrievedAt: stringField(entry, ["lastRetrievedAt", "last_retrieved_at", "retrieved_at", "lastUsedAt", "last_used_at"]),
    supersededBy: stringField(entry, ["supersededBy", "superseded_by"]),
    parents: stringArray(entry && entry.parents),
  };
}

(async () => {
  const [modulePath, cwd] = process.argv.slice(1);
  const summary = process.env.OPPI_HOPPI_REMEMBER_SUMMARY || "";
  const tags = parseTags(process.env.OPPI_HOPPI_REMEMBER_TAGS_JSON);
  const source = String(process.env.OPPI_HOPPI_REMEMBER_SOURCE || "oppi:native-shell").trim();
  const sourceSessionId = String(process.env.OPPI_HOPPI_REMEMBER_SOURCE_SESSION_ID || "").trim() || null;
  const layer = String(process.env.OPPI_HOPPI_REMEMBER_LAYER || "").trim();
  const hoppi = await import(pathToFileURL(path.resolve(modulePath)).href);
  if (typeof hoppi.createHoppiBackend !== "function") {
    throw new Error("Installed Hoppi package does not expose createHoppiBackend().");
  }
  const root = typeof hoppi.getDefaultHoppiRoot === "function"
    ? hoppi.getDefaultHoppiRoot()
    : (process.env.HOPPI_HOME ? path.resolve(process.env.HOPPI_HOME) : path.join(os.homedir(), ".oppi", "hoppi"));
  const backend = hoppi.createHoppiBackend({ root });
  if (!backend || typeof backend !== "object") {
    throw new Error("Installed Hoppi package returned an invalid backend.");
  }
  if (typeof backend.init === "function") await backend.init();
  if (typeof backend.remember !== "function") {
    throw new Error("Installed Hoppi package does not expose backend.remember().");
  }
  const resolvedCwd = path.resolve(cwd);
  const project = { cwd: resolvedCwd, displayName: path.basename(resolvedCwd) || undefined };
  const input = {
    project,
    content: String(summary || "").trim(),
    tags,
    confidence: "observed",
    source,
    sourceSessionId,
  };
  if (layer) input.layer = layer;
  const rawRecord = await backend.remember(input);
  const record = normalizeRecord(rawRecord, input);
  let memoryCount = record ? 1 : 0;
  let pinnedCount = record && (record.pinned || record.starred) ? 1 : 0;
  let storePath;
  if (typeof backend.status === "function") {
    try {
      const status = await backend.status(project);
      memoryCount = Math.max(memoryCount, coerceMemoryCount(status && status.memoryCount));
      pinnedCount = Math.max(pinnedCount, coerceMemoryCount(status && status.pinnedCount));
      if (status && status.storePath) storePath = String(status.storePath);
    } catch {
      // The write succeeded; status is only used for footer observability.
    }
  }
  console.log(JSON.stringify({ saved: true, memoryCount, pinnedCount, storePath, record }));
})().catch((error) => {
  console.error(error && error.stack ? error.stack : String(error));
  process.exit(1);
});
"#;

fn run_hoppi_remember_invocation(
    invocation: &HoppiRememberInvocation,
) -> Result<HoppiRememberCommandResult, String> {
    let mut command = Command::new(if cfg!(windows) { "node.exe" } else { "node" });
    command
        .arg("-e")
        .arg(HOPPI_REMEMBER_NODE_SCRIPT)
        .arg(&invocation.module_path)
        .arg(&invocation.cwd)
        .env("OPPI_HOPPI_REMEMBER_SUMMARY", &invocation.summary)
        .env(
            "OPPI_HOPPI_REMEMBER_TAGS_JSON",
            serde_json::to_string(&invocation.tags).map_err(|error| error.to_string())?,
        )
        .env("OPPI_HOPPI_REMEMBER_SOURCE", &invocation.source)
        .env(
            "OPPI_HOPPI_REMEMBER_SOURCE_SESSION_ID",
            &invocation.source_session_id,
        )
        .env(
            "OPPI_HOPPI_REMEMBER_LAYER",
            invocation.layer.as_deref().unwrap_or_default(),
        );
    let output = run_hoppi_node_output(&mut command, "Hoppi remember", HOPPI_BRIDGE_TIMEOUT)?;
    if !output.status.success() {
        let tail = compact_process_output(&[output.stdout, output.stderr].concat());
        return Err(format!(
            "Hoppi remember failed with status {}{}{}",
            output.status,
            if tail.is_empty() { "" } else { ": " },
            tail
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_hoppi_remember_output(&stdout)
}

fn parse_hoppi_remember_output(output: &str) -> Result<HoppiRememberCommandResult, String> {
    let line = output
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .ok_or_else(|| "Hoppi remember returned no JSON output".to_string())?;
    let value: Value = serde_json::from_str(line.trim())
        .map_err(|error| format!("parse Hoppi remember output: {error}"))?;
    let record = value
        .get("record")
        .filter(|record| !record.is_null())
        .cloned()
        .map(|record| {
            serde_json::from_value::<MemoryRecord>(record)
                .map_err(|error| format!("parse Hoppi remembered memory record: {error}"))
        })
        .transpose()?;
    Ok(HoppiRememberCommandResult {
        saved: value.get("saved").and_then(Value::as_bool).unwrap_or(false),
        memory_count: value
            .get("memoryCount")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        pinned_count: value.get("pinnedCount").and_then(Value::as_u64),
        store_path: value
            .get("storePath")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|path| !path.is_empty())
            .map(str::to_string),
        record,
    })
}

fn run_hoppi_sync(
    settings: &MemorySettings,
    direction: HoppiSyncDirection,
    reason: &str,
) -> Result<HoppiSyncRunResult, String> {
    let invocation = hoppi_sync_invocation_from_settings(
        settings,
        direction,
        reason,
        hoppi_module_path_for_sync()?,
    )?;
    let result = run_hoppi_sync_invocation(&invocation)?;
    Ok(HoppiSyncRunResult { invocation, result })
}

fn run_hoppi_sync_invocation(
    invocation: &HoppiSyncInvocation,
) -> Result<HoppiSyncCommandResult, String> {
    let mut command = Command::new(if cfg!(windows) { "node.exe" } else { "node" });
    command
        .arg("-e")
        .arg(HOPPI_SYNC_NODE_SCRIPT)
        .arg(&invocation.module_path)
        .arg(&invocation.repo_path)
        .arg(invocation.direction.as_str())
        .arg(&invocation.conflict_mode)
        .arg(&invocation.message)
        .arg(invocation.remote.as_deref().unwrap_or(""));
    if let Some(passphrase) = &invocation.passphrase {
        command.env("OPPI_HOPPI_SYNC_RUNTIME_PASSPHRASE", passphrase);
    } else {
        command.env_remove("OPPI_HOPPI_SYNC_RUNTIME_PASSPHRASE");
    }
    let output = run_hoppi_node_output(&mut command, "Hoppi sync", HOPPI_BRIDGE_TIMEOUT)?;
    if !output.status.success() {
        let tail = compact_process_output(&[output.stdout, output.stderr].concat());
        return Err(format!(
            "Hoppi sync failed with status {}{}{}",
            output.status,
            if tail.is_empty() { "" } else { ": " },
            tail
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let result = parse_hoppi_sync_output(&stdout)?;
    Ok(result)
}

fn parse_hoppi_sync_output(output: &str) -> Result<HoppiSyncCommandResult, String> {
    let line = output
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .ok_or_else(|| "Hoppi sync returned no JSON output".to_string())?;
    let value: Value = serde_json::from_str(line.trim())
        .map_err(|error| format!("parse Hoppi sync output: {error}"))?;
    let conflicts = value
        .get("conflicts")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(|item| {
                    item.as_str()
                        .map(str::to_string)
                        .unwrap_or_else(|| item.to_string())
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Ok(HoppiSyncCommandResult {
        imported: value.get("imported").and_then(Value::as_u64).unwrap_or(0),
        exported: value.get("exported").and_then(Value::as_u64).unwrap_or(0),
        committed: value
            .get("committed")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        pushed: value
            .get("pushed")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        conflicts,
        embeddings_rebuild_recommended: value
            .get("embeddingsRebuildRecommended")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        rebuilt_embeddings: value
            .get("rebuiltEmbeddings")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}

fn hoppi_sync_result_from_value(value: &Value) -> HoppiSyncCommandResult {
    let conflicts = value
        .get("conflicts")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(|item| {
                    item.as_str()
                        .map(str::to_string)
                        .unwrap_or_else(|| item.to_string())
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    HoppiSyncCommandResult {
        imported: value.get("imported").and_then(Value::as_u64).unwrap_or(0),
        exported: value.get("exported").and_then(Value::as_u64).unwrap_or(0),
        committed: value
            .get("committed")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        pushed: value
            .get("pushed")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        conflicts,
        embeddings_rebuild_recommended: value
            .get("embeddingsRebuildRecommended")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        rebuilt_embeddings: value
            .get("rebuiltEmbeddings")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    }
}

fn hoppi_sync_result_json(result: &HoppiSyncCommandResult) -> Value {
    json!({
        "imported": result.imported,
        "exported": result.exported,
        "committed": result.committed,
        "pushed": result.pushed,
        "conflicts": result.conflicts.clone(),
        "embeddingsRebuildRecommended": result.embeddings_rebuild_recommended,
        "rebuiltEmbeddings": result.rebuilt_embeddings,
    })
}

fn format_hoppi_sync_report(
    direction: HoppiSyncDirection,
    result: &HoppiSyncCommandResult,
) -> String {
    let mut parts = vec![
        format!("Hoppi sync {}", direction.as_str()),
        format!("pulled {}", result.imported),
    ];
    if result.exported > 0 {
        parts.push(format!("exported {}", result.exported));
    }
    if result.committed {
        parts.push("committed".to_string());
    }
    if result.pushed {
        parts.push("pushed".to_string());
    }
    if !result.conflicts.is_empty() {
        parts.push(format!("{} conflicts", result.conflicts.len()));
    }
    if result.embeddings_rebuild_recommended && result.rebuilt_embeddings {
        parts.push("embeddings rebuilt".to_string());
    }
    parts.join(" · ")
}

fn base_url_host_port(base: &str) -> Option<(String, u16)> {
    let (scheme, rest) = base
        .split_once("://")
        .map(|(scheme, rest)| (scheme, rest))
        .unwrap_or(("http", base));
    let authority = rest.split('/').next()?.rsplit('@').next()?.trim();
    let (host, port) = if authority.starts_with('[') {
        let end = authority.find(']')?;
        let host = authority[1..end].to_string();
        let port = authority[end + 1..]
            .strip_prefix(':')
            .and_then(|raw| raw.parse().ok());
        (host, port)
    } else if let Some((host, port)) = authority.rsplit_once(':') {
        (host.to_string(), port.parse().ok())
    } else {
        (authority.to_string(), None)
    };
    let default_port = if scheme.eq_ignore_ascii_case("https") {
        443
    } else {
        80
    };
    Some((host, port.unwrap_or(default_port)))
}
