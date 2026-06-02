fn skill_source_priority(source: SkillSource) -> u8 {
    match source {
        SkillSource::BuiltIn => 0,
        SkillSource::User => 10,
        SkillSource::Plugin => 15,
        SkillSource::Project => 20,
        SkillSource::Cli => 30,
    }
}

fn builtin_skill_candidates() -> Vec<SkillCandidate> {
    [
        ("builtin:imagegen", BUILTIN_IMAGEGEN_SKILL),
        ("builtin:independent", BUILTIN_INDEPENDENT_SKILL),
        ("builtin:mermaid-diagrams", BUILTIN_MERMAID_SKILL),
        ("builtin:graphify", BUILTIN_GRAPHIFY_SKILL),
    ]
    .into_iter()
    .filter_map(|(path, content)| parse_skill_candidate(content, path, SkillSource::BuiltIn))
    .collect()
}

fn parse_skill_candidate(content: &str, path: &str, source: SkillSource) -> Option<SkillCandidate> {
    let (name, description) = parse_skill_frontmatter(content)?;
    let priority = skill_source_priority(source);
    Some(SkillCandidate {
        reference: SkillRef {
            id: name.clone(),
            name,
            description,
            source,
            path: Some(path.to_string()),
            priority,
        },
        content: truncate_chars(content, MAX_SKILL_CONTENT_CHARS),
    })
}

fn parse_skill_frontmatter(content: &str) -> Option<(String, String)> {
    let mut lines = content.lines();
    if lines.next()?.trim() != "---" {
        return None;
    }
    let mut name = None;
    let mut description = None;
    for line in lines {
        let trimmed = line.trim();
        if trimmed == "---" {
            break;
        }
        let Some((key, value)) = trimmed.split_once(':') else {
            continue;
        };
        let value = parse_frontmatter_scalar(value.trim());
        match key.trim() {
            "name" => name = Some(value),
            "description" => description = Some(value),
            _ => {}
        }
    }
    let name = name?.trim().to_string();
    let description = description?.trim().to_string();
    if name.is_empty() || description.is_empty() {
        None
    } else {
        Some((name, description))
    }
}

fn parse_frontmatter_scalar(value: &str) -> String {
    value
        .trim_matches(|ch| ch == '"' || ch == '\'')
        .trim()
        .to_string()
}

fn prompt_template_source_priority(source: PromptTemplateSource) -> u8 {
    match source {
        PromptTemplateSource::BuiltIn => 0,
        PromptTemplateSource::User => 10,
        PromptTemplateSource::Plugin => 15,
        PromptTemplateSource::Project => 20,
        PromptTemplateSource::Cli => 30,
    }
}

fn split_prompt_template_frontmatter(content: &str) -> (BTreeMap<String, String>, String) {
    let mut metadata = BTreeMap::new();
    let mut lines = content.lines();
    if lines.next().map(str::trim) != Some("---") {
        return (metadata, content.trim().to_string());
    }

    for line in &mut lines {
        let trimmed = line.trim();
        if trimmed == "---" {
            let body = lines.collect::<Vec<_>>().join("\n").trim().to_string();
            return (metadata, body);
        }
        let Some((key, value)) = trimmed.split_once(':') else {
            continue;
        };
        metadata.insert(
            key.trim().to_ascii_lowercase(),
            parse_frontmatter_scalar(value.trim()),
        );
    }

    (BTreeMap::new(), content.trim().to_string())
}

fn parse_prompt_template_candidate(
    content: &str,
    path: &Path,
    source: PromptTemplateSource,
) -> Option<PromptTemplateCandidate> {
    let name = path
        .file_stem()
        .and_then(|value| value.to_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())?
        .to_string();
    let (metadata, body) = split_prompt_template_frontmatter(content);
    if body.trim().is_empty() {
        return None;
    }
    let description = metadata
        .get("description")
        .map(String::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            body.lines()
                .map(str::trim)
                .find(|line| !line.is_empty())
                .map(ToOwned::to_owned)
        })?;
    let argument_hint = metadata
        .get("argument-hint")
        .or_else(|| metadata.get("argument_hint"))
        .map(String::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let priority = prompt_template_source_priority(source);
    Some(PromptTemplateCandidate {
        reference: PromptTemplateRef {
            id: name.clone(),
            name,
            description,
            argument_hint,
            source,
            path: Some(display_resource_path(path)),
            priority,
        },
        body: truncate_chars(&body, MAX_PROMPT_TEMPLATE_CHARS),
    })
}

fn theme_source_priority(source: ThemeSource) -> u8 {
    match source {
        ThemeSource::BuiltIn => 0,
        ThemeSource::User => 10,
        ThemeSource::Plugin => 15,
        ThemeSource::Project => 20,
        ThemeSource::Cli => 30,
    }
}

fn theme_file_is_json(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
}

fn is_hex_theme_color(value: &str) -> bool {
    let Some(hex) = value.strip_prefix('#') else {
        return false;
    };
    hex.len() == 6 && hex.chars().all(|ch| ch.is_ascii_hexdigit())
}

fn resolve_theme_color_value(
    value: &Value,
    vars: &serde_json::Map<String, Value>,
    stack: &mut Vec<String>,
) -> Result<Value, String> {
    if let Some(number) = value.as_u64() {
        if number <= 255 {
            return Ok(json!(number));
        }
        return Err(format!("theme color index {number} is outside 0-255"));
    }
    let Some(raw) = value.as_str() else {
        return Err("theme color values must be strings, numbers, or defaults".to_string());
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(json!(""));
    }
    if is_hex_theme_color(trimmed) {
        return Ok(json!(trimmed.to_ascii_lowercase()));
    }
    if stack.iter().any(|name| name == trimmed) {
        return Err(format!("theme variable cycle includes `{trimmed}`"));
    }
    let Some(next) = vars.get(trimmed) else {
        return Err(format!("unknown theme variable `{trimmed}`"));
    };
    stack.push(trimmed.to_string());
    let resolved = resolve_theme_color_value(next, vars, stack);
    stack.pop();
    resolved
}

fn resolve_theme_value_object(
    object: &serde_json::Map<String, Value>,
    vars: &serde_json::Map<String, Value>,
) -> Result<Value, String> {
    let mut resolved = serde_json::Map::new();
    for (key, value) in object {
        resolved.insert(
            key.clone(),
            resolve_theme_color_value(value, vars, &mut Vec::new())
                .map_err(|error| format!("{key}: {error}"))?,
        );
    }
    Ok(Value::Object(resolved))
}

fn parse_theme_candidate(
    content: &str,
    path: &Path,
    source: ThemeSource,
) -> Result<ThemeCandidate, String> {
    let value = serde_json::from_str::<Value>(content)
        .map_err(|error| format!("invalid theme json: {error}"))?;
    let Some(object) = value.as_object() else {
        return Err("theme root must be a JSON object".to_string());
    };
    let name = object
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "theme name is required".to_string())?
        .to_string();
    let vars = match object.get("vars") {
        Some(value) => value
            .as_object()
            .ok_or_else(|| "theme vars must be an object".to_string())?,
        None => {
            static EMPTY: std::sync::OnceLock<serde_json::Map<String, Value>> =
                std::sync::OnceLock::new();
            EMPTY.get_or_init(serde_json::Map::new)
        }
    };
    let colors_object = object
        .get("colors")
        .and_then(Value::as_object)
        .ok_or_else(|| "theme colors object is required".to_string())?;
    for token in PI_REQUIRED_THEME_COLOR_TOKENS {
        if !colors_object.contains_key(token) {
            return Err(format!("missing required color token `{token}`"));
        }
    }
    let colors = resolve_theme_value_object(colors_object, vars)?;
    let export = match object.get("export") {
        Some(value) => Some(resolve_theme_value_object(
            value
                .as_object()
                .ok_or_else(|| "theme export must be an object".to_string())?,
            vars,
        )?),
        None => None,
    };
    let priority = theme_source_priority(source);
    Ok(ThemeCandidate {
        reference: ThemeRef {
            id: name.clone(),
            name,
            source,
            path: Some(display_resource_path(path)),
            priority,
        },
        colors,
        export,
    })
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("OPPI_HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

fn user_home_base_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

fn oppi_home_dir() -> Option<PathBuf> {
    std::env::var_os("OPPI_HOME")
        .map(PathBuf::from)
        .or_else(|| user_home_base_dir().map(|home| home.join(".oppi")))
}

fn plugin_materialization_store_path() -> Option<PathBuf> {
    oppi_home_dir().map(|home| home.join("plugins").join("materialized.json"))
}

fn plugin_materialization_cache_dir() -> Option<PathBuf> {
    oppi_home_dir().map(|home| home.join("plugins").join("cache"))
}

fn plugin_mcp_trust_store_path(cwd: &str) -> PathBuf {
    PathBuf::from(cwd).join(".oppi").join("plugin-mcp.json")
}

fn plugin_runner_store_path(cwd: &str) -> PathBuf {
    PathBuf::from(cwd).join(".oppi").join("plugin-runners.json")
}

fn collect_skill_files(
    root: &Path,
    include_root_md: bool,
    depth: usize,
    output: &mut Vec<PathBuf>,
) {
    if output.len() >= 200 || depth > 8 || !root.is_dir() {
        return;
    }
    let root_skill = root.join("SKILL.md");
    if root_skill.is_file() {
        output.push(root_skill);
    }
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        if output.len() >= 200 {
            return;
        }
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            let skill = path.join("SKILL.md");
            if skill.is_file() {
                output.push(skill);
            }
            collect_skill_files(&path, false, depth + 1, output);
        } else if include_root_md
            && depth == 0
            && path.extension().and_then(|ext| ext.to_str()) == Some("md")
        {
            output.push(path);
        }
    }
}

fn collect_prompt_template_files(root: &Path, output: &mut Vec<PathBuf>) {
    if output.len() >= 200 || !root.is_dir() {
        return;
    }
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        if output.len() >= 200 {
            return;
        }
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_file() && path.extension().and_then(|ext| ext.to_str()) == Some("md") {
            output.push(path);
        }
    }
}

fn collect_theme_files(root: &Path, output: &mut Vec<PathBuf>) {
    if output.len() >= 200 || !root.is_dir() {
        return;
    }
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        if output.len() >= 200 {
            return;
        }
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_file() && theme_file_is_json(&path) {
            output.push(path);
        }
    }
}

fn project_skill_roots(cwd: &str) -> Vec<(PathBuf, bool, SkillSource)> {
    let mut roots = Vec::new();
    let mut current = PathBuf::from(cwd);
    loop {
        roots.push((
            current.join(".pi").join("skills"),
            true,
            SkillSource::Project,
        ));
        roots.push((
            current.join(".agents").join("skills"),
            false,
            SkillSource::Project,
        ));
        if current.join(".git").exists() || !current.pop() {
            break;
        }
    }
    roots
}

fn project_prompt_roots(cwd: &str) -> Vec<(PathBuf, PromptTemplateSource)> {
    let mut roots = Vec::new();
    let mut current = PathBuf::from(cwd);
    loop {
        roots.push((
            current.join(".pi").join("prompts"),
            PromptTemplateSource::Project,
        ));
        roots.push((
            current.join(".oppi").join("prompts"),
            PromptTemplateSource::Project,
        ));
        if current.join(".git").exists() || !current.pop() {
            break;
        }
    }
    roots
}

fn project_theme_roots(cwd: &str) -> Vec<(PathBuf, ThemeSource)> {
    let mut roots = Vec::new();
    let mut current = PathBuf::from(cwd);
    loop {
        roots.push((current.join(".pi").join("themes"), ThemeSource::Project));
        roots.push((current.join(".oppi").join("themes"), ThemeSource::Project));
        if current.join(".git").exists() || !current.pop() {
            break;
        }
    }
    roots
}

fn user_skill_roots() -> Vec<(PathBuf, bool, SkillSource)> {
    let mut roots = Vec::new();
    for key in ["OPPI_USER_SKILLS_DIR", "OPPI_SKILLS_DIR"] {
        if let Some(value) = std::env::var_os(key) {
            roots.push((PathBuf::from(value), true, SkillSource::User));
        }
    }
    if let Some(home) = home_dir() {
        roots.push((
            home.join(".pi").join("agent").join("skills"),
            true,
            SkillSource::User,
        ));
        roots.push((
            home.join(".agents").join("skills"),
            false,
            SkillSource::User,
        ));
    }
    roots
}

fn user_prompt_roots() -> Vec<(PathBuf, PromptTemplateSource)> {
    let mut roots = Vec::new();
    for key in ["OPPI_USER_PROMPTS_DIR", "OPPI_PROMPTS_DIR"] {
        if let Some(value) = std::env::var_os(key) {
            roots.push((PathBuf::from(value), PromptTemplateSource::User));
        }
    }
    if let Some(home) = home_dir() {
        roots.push((
            home.join(".pi").join("agent").join("prompts"),
            PromptTemplateSource::User,
        ));
        roots.push((
            home.join(".oppi").join("prompts"),
            PromptTemplateSource::User,
        ));
    }
    roots
}

fn user_theme_roots() -> Vec<(PathBuf, ThemeSource)> {
    let mut roots = Vec::new();
    for key in ["OPPI_USER_THEMES_DIR", "OPPI_THEMES_DIR"] {
        if let Some(value) = std::env::var_os(key) {
            roots.push((PathBuf::from(value), ThemeSource::User));
        }
    }
    if let Some(home) = home_dir() {
        roots.push((
            home.join(".pi").join("agent").join("themes"),
            ThemeSource::User,
        ));
        roots.push((home.join(".oppi").join("themes"), ThemeSource::User));
    }
    roots
}

fn file_skill_candidates(roots: Vec<(PathBuf, bool, SkillSource)>) -> Vec<SkillCandidate> {
    let mut candidates = Vec::new();
    for (root, include_root_md, source) in roots {
        let mut files = Vec::new();
        collect_skill_files(&root, include_root_md, 0, &mut files);
        files.sort();
        files.dedup();
        for file in files {
            let Ok(content) = fs::read_to_string(&file) else {
                continue;
            };
            if let Some(candidate) =
                parse_skill_candidate(&content, &file.display().to_string(), source)
            {
                candidates.push(candidate);
            }
        }
    }
    candidates
}

fn file_prompt_template_candidates(
    roots: Vec<(PathBuf, PromptTemplateSource)>,
) -> Vec<PromptTemplateCandidate> {
    let mut candidates = Vec::new();
    for (root, source) in roots {
        let mut files = Vec::new();
        collect_prompt_template_files(&root, &mut files);
        files.sort();
        files.dedup();
        for file in files {
            let Ok(content) = fs::read_to_string(&file) else {
                continue;
            };
            if let Some(candidate) = parse_prompt_template_candidate(&content, &file, source) {
                candidates.push(candidate);
            }
        }
    }
    candidates
}

fn file_theme_candidates(roots: Vec<(PathBuf, ThemeSource)>) -> Vec<ThemeCandidate> {
    let mut candidates = Vec::new();
    for (root, source) in roots {
        let mut files = Vec::new();
        collect_theme_files(&root, &mut files);
        files.sort();
        files.dedup();
        for file in files {
            let Ok(content) = fs::read_to_string(&file) else {
                continue;
            };
            if let Ok(candidate) = parse_theme_candidate(&content, &file, source) {
                candidates.push(candidate);
            }
        }
    }
    candidates
}

fn read_json_value(path: &Path) -> Option<Value> {
    fs::read_to_string(path)
        .ok()
        .and_then(|content| serde_json::from_str(&content).ok())
}

fn read_plugin_mcp_trust_store(cwd: &str) -> PluginMcpTrustStore {
    let path = plugin_mcp_trust_store_path(cwd);
    let Some(value) = read_json_value(&path) else {
        return PluginMcpTrustStore {
            version: 1,
            servers: Vec::new(),
        };
    };
    serde_json::from_value(value).unwrap_or(PluginMcpTrustStore {
        version: 1,
        servers: Vec::new(),
    })
}

fn read_plugin_runner_store(cwd: &str) -> PluginRunnerStore {
    let path = plugin_runner_store_path(cwd);
    let Some(value) = read_json_value(&path) else {
        return PluginRunnerStore {
            version: 1,
            runners: Vec::new(),
        };
    };
    serde_json::from_value(value).unwrap_or(PluginRunnerStore {
        version: 1,
        runners: Vec::new(),
    })
}

fn write_plugin_mcp_trust_store(
    cwd: &str,
    store: &PluginMcpTrustStore,
) -> Result<(), RuntimeError> {
    let path = plugin_mcp_trust_store_path(cwd);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            RuntimeError::new(
                "plugin_mcp_trust_store_write_failed",
                RuntimeErrorCategory::InvalidRequest,
                format!("failed to create plugin MCP trust store directory: {error}"),
            )
        })?;
    }
    let rendered = serde_json::to_string_pretty(store).map_err(|error| {
        RuntimeError::new(
            "plugin_mcp_trust_store_write_failed",
            RuntimeErrorCategory::InvalidRequest,
            format!("failed to encode plugin MCP trust store: {error}"),
        )
    })?;
    fs::write(&path, rendered).map_err(|error| {
        RuntimeError::new(
            "plugin_mcp_trust_store_write_failed",
            RuntimeErrorCategory::InvalidRequest,
            format!("failed to write {}: {error}", path.display()),
        )
    })
}

fn plugin_mcp_trust_record<'a>(
    records: &'a [StoredPluginMcpTrustRecord],
    plugin_id: &str,
    server_id: &str,
) -> Option<&'a StoredPluginMcpTrustRecord> {
    records
        .iter()
        .find(|record| record.plugin_id == plugin_id && record.server_id == server_id)
}

fn plugin_runner_record<'a>(
    records: &'a [StoredPluginRunnerRecord],
    plugin_id: &str,
) -> Option<&'a StoredPluginRunnerRecord> {
    records.iter().find(|record| record.plugin_id == plugin_id)
}

fn string_array_field(value: Option<&Value>) -> Option<Vec<String>> {
    let values = value?.as_array()?;
    let result = values
        .iter()
        .filter_map(|item| item.as_str())
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    (!result.is_empty()).then_some(result)
}

fn string_field(value: Option<&Value>) -> Option<String> {
    value?
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn plugin_manifest_object<'a>(package_json: &'a Value, key: &str) -> Option<&'a Value> {
    package_json.get(key).filter(|value| value.is_object())
}

fn plugin_manifest_entries(root: &Path, key: &str, conventional_dir: &str) -> Vec<String> {
    if let Some(explicit) = read_json_value(&root.join("oppi-plugin.json")) {
        return string_array_field(explicit.get(key)).unwrap_or_default();
    }

    let Some(package_json) = read_json_value(&root.join("package.json")) else {
        return if root.join(conventional_dir).is_dir() {
            vec![conventional_dir.to_string()]
        } else {
            Vec::new()
        };
    };

    if let Some(oppi_plugin) = plugin_manifest_object(&package_json, "oppiPlugin") {
        return string_array_field(oppi_plugin.get(key)).unwrap_or_default();
    }

    string_array_field(package_json.get("pi").and_then(|pi| pi.get(key))).unwrap_or_else(|| {
        if root.join(conventional_dir).is_dir() {
            vec![conventional_dir.to_string()]
        } else {
            Vec::new()
        }
    })
}

fn plugin_manifest_skill_entries(root: &Path) -> Vec<String> {
    plugin_manifest_entries(root, "skills", "skills")
}

fn plugin_manifest_extension_entries(root: &Path) -> Vec<String> {
    plugin_manifest_entries(root, "extensions", "extensions")
}

fn plugin_manifest_prompt_entries(root: &Path) -> Vec<String> {
    plugin_manifest_entries(root, "prompts", "prompts")
}

fn plugin_manifest_theme_entries(root: &Path) -> Vec<String> {
    plugin_manifest_entries(root, "themes", "themes")
}

fn plugin_manifest_metadata(root: &Path) -> (Option<String>, Option<String>, Option<String>) {
    if let Some(explicit) = read_json_value(&root.join("oppi-plugin.json")) {
        return (
            string_field(explicit.get("name")),
            string_field(explicit.get("version")),
            string_field(explicit.get("description")),
        );
    }
    let Some(package_json) = read_json_value(&root.join("package.json")) else {
        return (None, None, None);
    };
    if let Some(oppi_plugin) = plugin_manifest_object(&package_json, "oppiPlugin") {
        let name = string_field(oppi_plugin.get("name"))
            .or_else(|| string_field(package_json.get("name")));
        let version = string_field(oppi_plugin.get("version"))
            .or_else(|| string_field(package_json.get("version")));
        let description = string_field(oppi_plugin.get("description"))
            .or_else(|| string_field(package_json.get("description")));
        return (name, version, description);
    }
    (
        string_field(package_json.get("name")),
        string_field(package_json.get("version")),
        string_field(package_json.get("description")),
    )
}

fn resource_entry_is_supported_path(entry: &str) -> bool {
    let trimmed = entry.trim();
    !trimmed.is_empty()
        && !trimmed.starts_with('!')
        && !trimmed.starts_with('+')
        && !trimmed.starts_with('-')
        && !trimmed.contains('*')
        && !trimmed.contains('?')
}

fn plugin_skill_resource_paths(root: &Path) -> Vec<PathBuf> {
    plugin_manifest_skill_entries(root)
        .into_iter()
        .filter(|entry| resource_entry_is_supported_path(entry))
        .map(|entry| root.join(entry))
        .collect()
}

fn plugin_prompt_resource_paths(root: &Path) -> Vec<PathBuf> {
    plugin_manifest_prompt_entries(root)
        .into_iter()
        .filter(|entry| resource_entry_is_supported_path(entry))
        .map(|entry| root.join(entry))
        .collect()
}

fn plugin_theme_resource_paths(root: &Path) -> Vec<PathBuf> {
    plugin_manifest_theme_entries(root)
        .into_iter()
        .filter(|entry| resource_entry_is_supported_path(entry))
        .map(|entry| root.join(entry))
        .collect()
}

fn display_resource_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn plugin_skill_resource_refs(root: &Path) -> Vec<PluginResourceRef> {
    let mut resources = Vec::new();
    for entry in plugin_manifest_skill_entries(root) {
        if !resource_entry_is_supported_path(&entry) {
            resources.push(PluginResourceRef {
                kind: PluginResourceKind::Skill,
                path: entry,
                status: PluginResourceStatus::Unsupported,
                reason: Some(
                    "glob and force-include/exclude filters are not loaded by Rust native yet"
                        .to_string(),
                ),
            });
            continue;
        }
        let path = root.join(&entry);
        if path.is_file() {
            resources.push(PluginResourceRef {
                kind: PluginResourceKind::Skill,
                path: display_resource_path(&path),
                status: PluginResourceStatus::Loaded,
                reason: None,
            });
            continue;
        }
        if path.is_dir() {
            let mut files = Vec::new();
            collect_skill_files(&path, true, 0, &mut files);
            files.sort();
            files.dedup();
            if files.is_empty() {
                resources.push(PluginResourceRef {
                    kind: PluginResourceKind::Skill,
                    path: display_resource_path(&path),
                    status: PluginResourceStatus::Missing,
                    reason: Some(
                        "skill directory contains no loadable markdown skill files".to_string(),
                    ),
                });
            } else {
                resources.extend(files.into_iter().map(|file| PluginResourceRef {
                    kind: PluginResourceKind::Skill,
                    path: display_resource_path(&file),
                    status: PluginResourceStatus::Loaded,
                    reason: None,
                }));
            }
            continue;
        }
        resources.push(PluginResourceRef {
            kind: PluginResourceKind::Skill,
            path: display_resource_path(&path),
            status: PluginResourceStatus::Missing,
            reason: Some("skill resource path does not exist".to_string()),
        });
    }
    resources
}

fn plugin_prompt_resource_refs(root: &Path) -> Vec<PluginResourceRef> {
    let mut resources = Vec::new();
    for entry in plugin_manifest_prompt_entries(root) {
        if !resource_entry_is_supported_path(&entry) {
            resources.push(PluginResourceRef {
                kind: PluginResourceKind::Prompt,
                path: entry,
                status: PluginResourceStatus::Unsupported,
                reason: Some(
                    "glob and force-include/exclude filters are not loaded by Rust native yet"
                        .to_string(),
                ),
            });
            continue;
        }
        let path = root.join(&entry);
        if path.is_file() {
            let status = if path.extension().and_then(|ext| ext.to_str()) == Some("md") {
                PluginResourceStatus::Loaded
            } else {
                PluginResourceStatus::Unsupported
            };
            resources.push(PluginResourceRef {
                kind: PluginResourceKind::Prompt,
                path: display_resource_path(&path),
                status,
                reason: (status == PluginResourceStatus::Unsupported)
                    .then(|| "prompt template files must be markdown".to_string()),
            });
            continue;
        }
        if path.is_dir() {
            let mut files = Vec::new();
            collect_prompt_template_files(&path, &mut files);
            files.sort();
            files.dedup();
            if files.is_empty() {
                resources.push(PluginResourceRef {
                    kind: PluginResourceKind::Prompt,
                    path: display_resource_path(&path),
                    status: PluginResourceStatus::Missing,
                    reason: Some(
                        "prompt directory contains no loadable markdown template files".to_string(),
                    ),
                });
            } else {
                resources.extend(files.into_iter().map(|file| PluginResourceRef {
                    kind: PluginResourceKind::Prompt,
                    path: display_resource_path(&file),
                    status: PluginResourceStatus::Loaded,
                    reason: None,
                }));
            }
            continue;
        }
        resources.push(PluginResourceRef {
            kind: PluginResourceKind::Prompt,
            path: display_resource_path(&path),
            status: PluginResourceStatus::Missing,
            reason: Some("prompt resource path does not exist".to_string()),
        });
    }
    resources
}

fn theme_resource_ref_for_file(path: &Path) -> PluginResourceRef {
    if !theme_file_is_json(path) {
        return PluginResourceRef {
            kind: PluginResourceKind::Theme,
            path: display_resource_path(path),
            status: PluginResourceStatus::Unsupported,
            reason: Some("theme files must be json".to_string()),
        };
    }
    let status_reason = match fs::read_to_string(path) {
        Ok(content) => parse_theme_candidate(&content, path, ThemeSource::Plugin)
            .err()
            .map(|error| format!("invalid theme: {error}")),
        Err(error) => Some(format!("read theme resource failed: {error}")),
    };
    PluginResourceRef {
        kind: PluginResourceKind::Theme,
        path: display_resource_path(path),
        status: if status_reason.is_some() {
            PluginResourceStatus::Unsupported
        } else {
            PluginResourceStatus::Loaded
        },
        reason: status_reason,
    }
}

fn plugin_theme_resource_refs(root: &Path) -> Vec<PluginResourceRef> {
    let mut resources = Vec::new();
    for entry in plugin_manifest_theme_entries(root) {
        if !resource_entry_is_supported_path(&entry) {
            resources.push(PluginResourceRef {
                kind: PluginResourceKind::Theme,
                path: entry,
                status: PluginResourceStatus::Unsupported,
                reason: Some(
                    "glob and force-include/exclude filters are not loaded by Rust native yet"
                        .to_string(),
                ),
            });
            continue;
        }
        let path = root.join(&entry);
        if path.is_file() {
            resources.push(theme_resource_ref_for_file(&path));
            continue;
        }
        if path.is_dir() {
            let mut files = Vec::new();
            collect_theme_files(&path, &mut files);
            files.sort();
            files.dedup();
            if files.is_empty() {
                resources.push(PluginResourceRef {
                    kind: PluginResourceKind::Theme,
                    path: display_resource_path(&path),
                    status: PluginResourceStatus::Missing,
                    reason: Some(
                        "theme directory contains no loadable json theme files".to_string(),
                    ),
                });
            } else {
                resources.extend(
                    files
                        .into_iter()
                        .map(|file| theme_resource_ref_for_file(&file)),
                );
            }
            continue;
        }
        resources.push(PluginResourceRef {
            kind: PluginResourceKind::Theme,
            path: display_resource_path(&path),
            status: PluginResourceStatus::Missing,
            reason: Some("theme resource path does not exist".to_string()),
        });
    }
    resources
}

fn unsupported_plugin_resource_refs(
    root: &Path,
    kind: PluginResourceKind,
    entries: Vec<String>,
    reason: &str,
) -> Vec<PluginResourceRef> {
    entries
        .into_iter()
        .map(|entry| {
            let path = if resource_entry_is_supported_path(&entry) {
                display_resource_path(&root.join(&entry))
            } else {
                entry
            };
            PluginResourceRef {
                kind,
                path,
                status: PluginResourceStatus::Unsupported,
                reason: Some(reason.to_string()),
            }
        })
        .collect()
}

fn plugin_resource_refs_from_root(root: &Path) -> Vec<PluginResourceRef> {
    let mut resources = plugin_skill_resource_refs(root);
    resources.extend(plugin_prompt_resource_refs(root));
    resources.extend(plugin_theme_resource_refs(root));
    resources.extend(unsupported_plugin_resource_refs(
        root,
        PluginResourceKind::Extension,
        plugin_manifest_extension_entries(root),
        "Pi extensions are not executed by Rust native",
    ));
    resources
}

fn plugin_resource_diagnostics(resources: &[PluginResourceRef]) -> Vec<PluginDiagnostic> {
    let mut diagnostics = Vec::new();
    if resources.iter().any(|resource| {
        resource.kind == PluginResourceKind::Extension
            && resource.status == PluginResourceStatus::Unsupported
    }) {
        diagnostics.push(PluginDiagnostic {
            code: "pi_extension_unsupported".to_string(),
            message: "Pi extension resources are visible but not executed by Rust native"
                .to_string(),
            severity: Some("warning".to_string()),
        });
    }
    if resources.iter().any(|resource| {
        matches!(
            resource.kind,
            PluginResourceKind::Prompt | PluginResourceKind::Theme
        ) && resource.status == PluginResourceStatus::Unsupported
    }) {
        diagnostics.push(PluginDiagnostic {
            code: "plugin_resource_unsupported".to_string(),
            message: "Some plugin resources are unsupported or invalid in Rust native".to_string(),
            severity: Some("info".to_string()),
        });
    }
    if resources
        .iter()
        .any(|resource| resource.status == PluginResourceStatus::Missing)
    {
        diagnostics.push(PluginDiagnostic {
            code: "plugin_resource_missing".to_string(),
            message: "One or more plugin resource paths could not be resolved".to_string(),
            severity: Some("warning".to_string()),
        });
    }
    diagnostics
}

fn normalized_plugin_capabilities(entry: &StoredPluginEntry) -> BTreeSet<String> {
    let mut capabilities = BTreeSet::new();
    for capability in &entry.capabilities {
        let value = capability.trim().to_ascii_lowercase();
        if !value.is_empty() {
            capabilities.insert(value);
        }
    }
    if let Some(manifest) = &entry.manifest
        && let Some(values) = string_array_field(manifest.get("capabilities").or_else(|| {
            manifest
                .get("oppi")
                .and_then(|oppi| oppi.get("capabilities"))
        }))
    {
        for capability in values {
            let value = capability.trim().to_ascii_lowercase();
            if !value.is_empty() {
                capabilities.insert(value);
            }
        }
    }
    capabilities
}

fn plugin_store_metadata_diagnostics(
    entry: &StoredPluginEntry,
    materialized: bool,
) -> Vec<PluginDiagnostic> {
    let mut diagnostics = Vec::new();
    let _license = entry
        .license
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let source_type = entry
        .source_type
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("local");
    if !materialized
        && (source_type.eq_ignore_ascii_case("marketplace") || entry.marketplace.is_some())
    {
        diagnostics.push(PluginDiagnostic {
            code: "marketplace_plugin_not_materialized".to_string(),
            message:
                "Marketplace plugin is configured, but Rust native does not install or materialize marketplace packages yet"
                    .to_string(),
            severity: Some("warning".to_string()),
        });
    }
    let capabilities = normalized_plugin_capabilities(entry);
    if capabilities.contains("shell") || capabilities.contains("exec") {
        diagnostics.push(PluginDiagnostic {
            code: "plugin_risk_shell_exec".to_string(),
            message: "Plugin declares shell or exec capability".to_string(),
            severity: Some("warning".to_string()),
        });
    }
    if capabilities.contains("network") {
        diagnostics.push(PluginDiagnostic {
            code: "plugin_risk_network".to_string(),
            message: "Plugin declares network capability".to_string(),
            severity: Some("warning".to_string()),
        });
    }
    if capabilities.contains("native") {
        diagnostics.push(PluginDiagnostic {
            code: "plugin_risk_native".to_string(),
            message: "Plugin declares native-code capability".to_string(),
            severity: Some("warning".to_string()),
        });
    }
    diagnostics
}

fn claude_signal_fields(value: &Value) -> Vec<String> {
    let Some(object) = value.as_object() else {
        return Vec::new();
    };
    [
        "mcpServers",
        "mcp_servers",
        "server",
        "servers",
        "slashCommands",
        "slash_commands",
        "commands",
        "agents",
        "hooks",
        "claude",
        "claudePlugin",
        "claude-plugin",
        "claude_desktop_config",
        "claudeDesktopConfig",
    ]
    .into_iter()
    .filter(|field| object.contains_key(*field))
    .map(ToOwned::to_owned)
    .collect()
}

fn manifest_has_pi_resources(root: &Path) -> bool {
    !plugin_manifest_skill_entries(root).is_empty()
        || !plugin_manifest_extension_entries(root).is_empty()
        || !plugin_manifest_prompt_entries(root).is_empty()
        || !plugin_manifest_theme_entries(root).is_empty()
}

fn claude_plugin_diagnostics(root: &Path) -> Vec<PluginDiagnostic> {
    if manifest_has_pi_resources(root) {
        return Vec::new();
    }
    let mut signals = Vec::new();
    if let Some(package_json) = read_json_value(&root.join("package.json")) {
        signals.extend(claude_signal_fields(&package_json));
    }
    for manifest_name in ["claude-plugin.json", ".claude-plugin.json", "claude.json"] {
        if let Some(manifest) = read_json_value(&root.join(manifest_name)) {
            signals.extend(claude_signal_fields(&manifest));
        }
    }
    signals.sort();
    signals.dedup();
    if signals.is_empty() {
        return Vec::new();
    }
    vec![PluginDiagnostic {
        code: "claude_plugin_requires_port".to_string(),
        message: format!(
            "Detected Claude-specific plugin fields ({}) without Pi resources. Port this package into Pi/OPPi skills, extensions, prompts, or themes before loading it in Rust native.",
            signals.join(", ")
        ),
        severity: Some("warning".to_string()),
    }]
}

fn local_package_risk_diagnostics(root: &Path) -> Vec<PluginDiagnostic> {
    let Some(package_json) = read_json_value(&root.join("package.json")) else {
        return Vec::new();
    };
    let mut diagnostics = Vec::new();
    if let Some(scripts) = package_json
        .get("scripts")
        .and_then(|value| value.as_object())
    {
        for script in ["preinstall", "install", "postinstall", "prepare"] {
            if scripts
                .get(script)
                .and_then(|value| value.as_str())
                .is_some()
            {
                diagnostics.push(PluginDiagnostic {
                    code: "plugin_risk_install_script".to_string(),
                    message: format!("package.json has {script} script"),
                    severity: Some("warning".to_string()),
                });
            }
        }
    }
    if package_json
        .get("gypfile")
        .and_then(|value| value.as_bool())
        == Some(true)
    {
        diagnostics.push(PluginDiagnostic {
            code: "plugin_risk_native_build".to_string(),
            message: "package.json declares gypfile/native build".to_string(),
            severity: Some("warning".to_string()),
        });
    }
    diagnostics
}

fn plugin_skill_candidates_from_root(root: &Path) -> Vec<SkillCandidate> {
    let mut candidates = Vec::new();
    for path in plugin_skill_resource_paths(root) {
        if path.is_file() {
            if let Ok(content) = fs::read_to_string(&path)
                && let Some(candidate) = parse_skill_candidate(
                    &content,
                    &path.display().to_string(),
                    SkillSource::Plugin,
                )
            {
                candidates.push(candidate);
            }
            continue;
        }
        let mut files = Vec::new();
        collect_skill_files(&path, true, 0, &mut files);
        files.sort();
        files.dedup();
        for file in files {
            let Ok(content) = fs::read_to_string(&file) else {
                continue;
            };
            if let Some(candidate) =
                parse_skill_candidate(&content, &file.display().to_string(), SkillSource::Plugin)
            {
                candidates.push(candidate);
            }
        }
    }
    candidates
}

fn plugin_prompt_template_candidates_from_root(root: &Path) -> Vec<PromptTemplateCandidate> {
    let mut candidates = Vec::new();
    for path in plugin_prompt_resource_paths(root) {
        if path.is_file() {
            if path.extension().and_then(|ext| ext.to_str()) != Some("md") {
                continue;
            }
            if let Ok(content) = fs::read_to_string(&path)
                && let Some(candidate) =
                    parse_prompt_template_candidate(&content, &path, PromptTemplateSource::Plugin)
            {
                candidates.push(candidate);
            }
            continue;
        }
        let mut files = Vec::new();
        collect_prompt_template_files(&path, &mut files);
        files.sort();
        files.dedup();
        for file in files {
            let Ok(content) = fs::read_to_string(&file) else {
                continue;
            };
            if let Some(candidate) =
                parse_prompt_template_candidate(&content, &file, PromptTemplateSource::Plugin)
            {
                candidates.push(candidate);
            }
        }
    }
    candidates
}

fn plugin_theme_candidates_from_root(root: &Path) -> Vec<ThemeCandidate> {
    let mut candidates = Vec::new();
    for path in plugin_theme_resource_paths(root) {
        if path.is_file() {
            if !theme_file_is_json(&path) {
                continue;
            }
            if let Ok(content) = fs::read_to_string(&path)
                && let Ok(candidate) = parse_theme_candidate(&content, &path, ThemeSource::Plugin)
            {
                candidates.push(candidate);
            }
            continue;
        }
        let mut files = Vec::new();
        collect_theme_files(&path, &mut files);
        files.sort();
        files.dedup();
        for file in files {
            let Ok(content) = fs::read_to_string(&file) else {
                continue;
            };
            if let Ok(candidate) = parse_theme_candidate(&content, &file, ThemeSource::Plugin) {
                candidates.push(candidate);
            }
        }
    }
    candidates
}

fn expand_tilde_path(source: &str) -> PathBuf {
    if source == "~" {
        return user_home_base_dir().unwrap_or_else(|| PathBuf::from(source));
    }
    for prefix in ["~/", "~\\"] {
        if let Some(rest) = source.strip_prefix(prefix) {
            return user_home_base_dir()
                .map(|home| home.join(rest))
                .unwrap_or_else(|| PathBuf::from(source));
        }
    }
    PathBuf::from(source)
}

fn resolve_local_plugin_root(source: &str, cwd: Option<&str>) -> Option<PathBuf> {
    let expanded = expand_tilde_path(source);
    let path = if expanded.is_absolute() {
        expanded
    } else if let Some(cwd) = cwd {
        PathBuf::from(cwd).join(expanded)
    } else {
        std::env::current_dir().ok()?.join(expanded)
    };
    let metadata = fs::metadata(&path).ok()?;
    if metadata.is_dir() {
        Some(path)
    } else if metadata.is_file() {
        path.parent().map(Path::to_path_buf)
    } else {
        None
    }
}

fn plugin_store_paths(cwd: Option<&str>) -> Vec<(PluginScope, PathBuf)> {
    let mut paths = Vec::new();
    if let Some(home) = oppi_home_dir() {
        paths.push((PluginScope::Global, home.join("plugin-lock.json")));
    }
    if let Some(cwd) = cwd {
        paths.push((
            PluginScope::Project,
            PathBuf::from(cwd).join(".oppi").join("plugins.json"),
        ));
    }
    paths
}

fn read_plugin_store(path: &Path) -> Vec<StoredPluginEntry> {
    let Ok(content) = fs::read_to_string(path) else {
        return Vec::new();
    };
    serde_json::from_str::<StoredPluginStore>(&content)
        .map(|store| store.plugins)
        .unwrap_or_default()
}

fn read_plugin_materialization_records() -> Vec<StoredPluginMaterializationRecord> {
    let Some(path) = plugin_materialization_store_path() else {
        return Vec::new();
    };
    let Ok(content) = fs::read_to_string(path) else {
        return Vec::new();
    };
    serde_json::from_str::<PluginMaterializationStore>(&content)
        .map(|store| store.records)
        .unwrap_or_default()
}

fn store_entry_allows_native_local_loading(plugin: &StoredPluginEntry) -> bool {
    if !plugin.enabled || !plugin.trusted || plugin.source.trim().is_empty() {
        return false;
    }
    plugin
        .source_type
        .as_deref()
        .map(str::trim)
        .filter(|source_type| !source_type.is_empty())
        .map(|source_type| source_type.eq_ignore_ascii_case("local"))
        .unwrap_or(true)
}

fn plugin_source_type(value: Option<&str>) -> PluginSourceType {
    match value
        .unwrap_or("local")
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "local" => PluginSourceType::Local,
        "npm" => PluginSourceType::Npm,
        "git" => PluginSourceType::Git,
        "url" => PluginSourceType::Url,
        "marketplace" => PluginSourceType::Marketplace,
        _ => PluginSourceType::Unknown,
    }
}

fn plugin_entry_source_type(entry: &StoredPluginEntry) -> PluginSourceType {
    if entry
        .source_type
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_none()
        && entry.marketplace.is_some()
    {
        return PluginSourceType::Marketplace;
    }
    plugin_source_type(entry.source_type.as_deref())
}

fn materialization_record_for_entry<'a>(
    entry: &StoredPluginEntry,
    scope: PluginScope,
    records: &'a [StoredPluginMaterializationRecord],
) -> Option<&'a StoredPluginMaterializationRecord> {
    let source_type = plugin_entry_source_type(entry);
    records.iter().find(|record| {
        record.scope == scope
            && record.source == entry.source
            && plugin_source_type(Some(&record.source_type)) == source_type
            && Path::new(&record.cache_path).exists()
    })
}

fn materialized_plugin_root_for_entry(
    entry: &StoredPluginEntry,
    scope: PluginScope,
    records: &[StoredPluginMaterializationRecord],
) -> Option<PathBuf> {
    if !entry.enabled || !entry.trusted || entry.source.trim().is_empty() {
        return None;
    }
    materialization_record_for_entry(entry, scope, records)
        .map(|record| PathBuf::from(&record.cache_path))
}

fn materialization_ref_for_entry(
    entry: &StoredPluginEntry,
    local_root: Option<&Path>,
    record: Option<&StoredPluginMaterializationRecord>,
) -> PluginMaterializationRef {
    let source_type = plugin_entry_source_type(entry);
    if source_type == PluginSourceType::Local && local_root.is_some() {
        return PluginMaterializationRef {
            status: PluginMaterializationStatus::NotRequired,
            source_type,
            cache_path: None,
            manifest_path: Some("package.json".to_string()),
            checksum: None,
            materialized_at: None,
            provenance: None,
            diagnostics: vec![PluginDiagnostic {
                code: "plugin_static_local_source".to_string(),
                message: "Local trusted plugin can contribute static resources without cache materialization."
                    .to_string(),
                severity: Some("info".to_string()),
            }],
        };
    }

    if let Some(record) = record {
        return PluginMaterializationRef {
            status: PluginMaterializationStatus::Materialized,
            source_type,
            cache_path: Some(record.cache_path.clone()),
            manifest_path: Some("package.json".to_string()),
            checksum: Some(record.checksum.clone()),
            materialized_at: Some(record.materialized_at.clone()),
            provenance: record.provenance.clone().or_else(|| {
                Some(PluginProvenance {
                    marketplace: entry.marketplace.clone(),
                    catalog_url: None,
                    resolved_source: record.source.clone(),
                    resolved_version: record.manifest_version.clone(),
                })
            }),
            diagnostics: vec![PluginDiagnostic {
                code: "plugin_materialized_static_only".to_string(),
                message: "Materialized package can contribute static resources; executable handlers require separate enablement."
                    .to_string(),
                severity: Some("info".to_string()),
            }],
        };
    }

    let cache_hint = plugin_materialization_cache_dir()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "OPPI_HOME/plugins/cache".to_string());
    match source_type {
        PluginSourceType::Local => PluginMaterializationRef {
            status: PluginMaterializationStatus::Failed,
            source_type,
            cache_path: None,
            manifest_path: None,
            checksum: None,
            materialized_at: None,
            provenance: None,
            diagnostics: vec![PluginDiagnostic {
                code: "plugin_source_missing".to_string(),
                message: "Local plugin source path could not be resolved.".to_string(),
                severity: Some("warning".to_string()),
            }],
        },
        PluginSourceType::Unknown => PluginMaterializationRef {
            status: PluginMaterializationStatus::Unsupported,
            source_type,
            cache_path: None,
            manifest_path: None,
            checksum: None,
            materialized_at: None,
            provenance: Some(PluginProvenance {
                marketplace: entry.marketplace.clone(),
                catalog_url: None,
                resolved_source: entry.source.clone(),
                resolved_version: entry.version.clone(),
            }),
            diagnostics: vec![PluginDiagnostic {
                code: "plugin_source_type_unsupported".to_string(),
                message: "Plugin source type is not supported by native materialization."
                    .to_string(),
                severity: Some("warning".to_string()),
            }],
        },
        _ => PluginMaterializationRef {
            status: PluginMaterializationStatus::NotMaterialized,
            source_type,
            cache_path: None,
            manifest_path: None,
            checksum: None,
            materialized_at: None,
            provenance: Some(PluginProvenance {
                marketplace: entry.marketplace.clone(),
                catalog_url: None,
                resolved_source: entry.source.clone(),
                resolved_version: entry.version.clone(),
            }),
            diagnostics: vec![PluginDiagnostic {
                code: "plugin_materialization_required".to_string(),
                message: format!(
                    "Plugin source must be materialized into the native cache at {cache_hint} before Rust can load static resources."
                ),
                severity: Some("warning".to_string()),
            }],
        },
    }
}

fn plugin_runner_diagnostic(
    code: impl Into<String>,
    message: impl Into<String>,
    severity: impl Into<Option<String>>,
) -> PluginDiagnostic {
    PluginDiagnostic {
        code: code.into(),
        message: message.into(),
        severity: severity.into(),
    }
}

fn plugin_runtime_manifest_value(package_json: &Value) -> Option<&Value> {
    package_json
        .get("oppi")
        .and_then(|oppi| oppi.get("runtime"))
        .or_else(|| {
            package_json
                .get("oppiPlugin")
                .and_then(|oppi| oppi.get("runtime"))
        })
}

fn plugin_runner_is_bare_executable(value: &str) -> bool {
    let trimmed = value.trim();
    !trimmed.is_empty()
        && !trimmed.contains('/')
        && !trimmed.contains('\\')
        && !trimmed.contains(':')
        && !trimmed.starts_with('.')
        && !Path::new(trimmed).is_absolute()
}

fn plugin_runner_path_stays_inside_root(root: &Path, value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return false;
    }
    let path = Path::new(trimmed);
    let resolved = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    lexical_normalize(&resolved).starts_with(lexical_normalize(root))
}

fn plugin_runner_command_is_trusted(root: &Path, command: &str) -> bool {
    plugin_runner_is_bare_executable(command) || plugin_runner_path_stays_inside_root(root, command)
}

fn plugin_runner_arg_looks_path_like(value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return false;
    }
    trimmed.contains('/')
        || trimmed.contains('\\')
        || trimmed.starts_with('.')
        || Path::new(trimmed).is_absolute()
        || (!trimmed.starts_with('-') && Path::new(trimmed).extension().is_some())
}

fn plugin_runner_arg_is_trusted(root: &Path, arg: &str) -> bool {
    let trimmed = arg.trim();
    if let Some((_, value)) = trimmed.split_once('=')
        && plugin_runner_arg_looks_path_like(value)
    {
        return plugin_runner_path_stays_inside_root(root, value);
    }
    !plugin_runner_arg_looks_path_like(trimmed)
        || plugin_runner_path_stays_inside_root(root, trimmed)
}

fn plugin_runner_record_enabled_and_trusted(
    records: &[StoredPluginRunnerRecord],
    plugin_id: &str,
) -> bool {
    plugin_runner_record(records, plugin_id).is_some_and(|record| record.enabled && record.trusted)
}

fn plugin_executable_boundary_from_root(
    plugin_id: &str,
    root: &Path,
    runner_records: &[StoredPluginRunnerRecord],
) -> Option<PluginExecutableBoundaryRef> {
    let package_json = read_json_value(&root.join("package.json"))?;
    let runtime = plugin_runtime_manifest_value(&package_json)?;
    let Some(runtime) = runtime.as_object() else {
        return Some(PluginExecutableBoundaryRef {
            kind: PluginExecutableBoundaryKind::StaticResourcesOnly,
            status: PluginExecutableBoundaryStatus::Unsupported,
            command: None,
            args: Vec::new(),
            capabilities: Vec::new(),
            diagnostics: vec![plugin_runner_diagnostic(
                "plugin_runner_manifest_invalid",
                "Plugin runtime manifest must be an object.",
                Some("warning".to_string()),
            )],
        });
    };
    let kind = string_field(runtime.get("kind")).unwrap_or_default();
    if kind != "jsonRpcSubprocess" {
        return Some(PluginExecutableBoundaryRef {
            kind: PluginExecutableBoundaryKind::StaticResourcesOnly,
            status: PluginExecutableBoundaryStatus::Unsupported,
            command: None,
            args: Vec::new(),
            capabilities: Vec::new(),
            diagnostics: vec![plugin_runner_diagnostic(
                "plugin_runner_kind_unsupported",
                format!("Plugin runner kind `{kind}` is not supported by Rust native."),
                Some("warning".to_string()),
            )],
        });
    }
    let Some(command) = string_field(runtime.get("command")) else {
        return Some(PluginExecutableBoundaryRef {
            kind: PluginExecutableBoundaryKind::JsonRpcSubprocess,
            status: PluginExecutableBoundaryStatus::Failed,
            command: None,
            args: Vec::new(),
            capabilities: string_vec_field(runtime.get("capabilities")),
            diagnostics: vec![plugin_runner_diagnostic(
                "plugin_runner_command_missing",
                "JSON-RPC subprocess plugin runner manifest omitted command.",
                Some("error".to_string()),
            )],
        });
    };
    let args = string_vec_field(runtime.get("args"));
    let capabilities = string_vec_field(runtime.get("capabilities"));
    if !plugin_runner_command_is_trusted(root, &command)
        || args
            .iter()
            .any(|arg| !plugin_runner_arg_is_trusted(root, arg))
    {
        return Some(PluginExecutableBoundaryRef {
            kind: PluginExecutableBoundaryKind::JsonRpcSubprocess,
            status: PluginExecutableBoundaryStatus::Failed,
            command: Some(command),
            args,
            capabilities,
            diagnostics: vec![plugin_runner_diagnostic(
                "plugin_runner_path_untrusted",
                "Plugin runner command or argument path escapes the local/materialized plugin root.",
                Some("error".to_string()),
            )],
        });
    }

    let enabled = plugin_runner_record_enabled_and_trusted(runner_records, plugin_id);
    let diagnostics = if enabled {
        Vec::new()
    } else {
        vec![plugin_runner_diagnostic(
            "plugin_runner_requires_enablement",
            "Executable plugin runner is disabled until explicitly enabled and trusted.",
            Some("warning".to_string()),
        )]
    };
    Some(PluginExecutableBoundaryRef {
        kind: PluginExecutableBoundaryKind::JsonRpcSubprocess,
        status: if enabled {
            PluginExecutableBoundaryStatus::Enabled
        } else {
            PluginExecutableBoundaryStatus::Disabled
        },
        command: Some(command),
        args,
        capabilities,
        diagnostics,
    })
}

fn configured_local_plugin_roots(cwd: Option<&str>) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let mut seen = BTreeSet::new();
    let records = read_plugin_materialization_records();
    for (scope, store_path) in plugin_store_paths(cwd) {
        for plugin in read_plugin_store(&store_path) {
            let root = if store_entry_allows_native_local_loading(&plugin) {
                resolve_local_plugin_root(&plugin.source, cwd)
            } else {
                materialized_plugin_root_for_entry(&plugin, scope, &records)
            };
            let Some(root) = root else {
                continue;
            };
            let key = root.to_string_lossy().to_ascii_lowercase();
            if seen.insert(key) {
                roots.push(root);
            }
        }
    }

    if let Ok(raw) = std::env::var("OPPI_PLUGIN_SOURCES") {
        for source in raw
            .split([';', ','])
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            let Some(root) = resolve_local_plugin_root(source, cwd) else {
                continue;
            };
            let key = root.to_string_lossy().to_ascii_lowercase();
            if seen.insert(key) {
                roots.push(root);
            }
        }
    }

    roots
}

fn configured_plugin_mcp_roots(cwd: Option<&str>) -> Vec<ConfiguredPluginMcpRoot> {
    let mut roots = Vec::new();
    let mut seen = BTreeSet::new();
    let records = read_plugin_materialization_records();
    for (scope, store_path) in plugin_store_paths(cwd) {
        for plugin in read_plugin_store(&store_path) {
            let root = if store_entry_allows_native_local_loading(&plugin) {
                resolve_local_plugin_root(&plugin.source, cwd)
            } else {
                materialized_plugin_root_for_entry(&plugin, scope, &records)
            };
            let Some(root) = root else {
                continue;
            };
            let manifest_name = plugin_manifest_metadata(&root).0;
            let plugin_id = plugin
                .name
                .clone()
                .or(manifest_name)
                .unwrap_or_else(|| plugin_id_from_source(&plugin.source));
            let key = format!(
                "{}::{}",
                plugin_id.to_ascii_lowercase(),
                root.to_string_lossy().to_ascii_lowercase()
            );
            if seen.insert(key) {
                roots.push(ConfiguredPluginMcpRoot { plugin_id, root });
            }
        }
    }

    if let Ok(raw) = std::env::var("OPPI_PLUGIN_SOURCES") {
        for source in raw
            .split([';', ','])
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            let Some(root) = resolve_local_plugin_root(source, cwd) else {
                continue;
            };
            let manifest_name = plugin_manifest_metadata(&root).0;
            let plugin_id = manifest_name.unwrap_or_else(|| plugin_id_from_source(source));
            let key = format!(
                "{}::{}",
                plugin_id.to_ascii_lowercase(),
                root.to_string_lossy().to_ascii_lowercase()
            );
            if seen.insert(key) {
                roots.push(ConfiguredPluginMcpRoot { plugin_id, root });
            }
        }
    }

    roots
}

fn configured_plugin_runner_entries(cwd: &str) -> Vec<ConfiguredPluginRunner> {
    let mut runners = Vec::new();
    let mut seen = BTreeSet::new();
    let materialization_records = read_plugin_materialization_records();
    let runner_records = read_plugin_runner_store(cwd).runners;
    for (scope, store_path) in plugin_store_paths(Some(cwd)) {
        for plugin in read_plugin_store(&store_path) {
            let root = if store_entry_allows_native_local_loading(&plugin) {
                resolve_local_plugin_root(&plugin.source, Some(cwd))
            } else {
                materialized_plugin_root_for_entry(&plugin, scope, &materialization_records)
            };
            let Some(root) = root else {
                continue;
            };
            let manifest_name = plugin_manifest_metadata(&root).0;
            let plugin_id = plugin
                .name
                .clone()
                .or(manifest_name)
                .unwrap_or_else(|| plugin_id_from_source(&plugin.source));
            let Some(boundary) =
                plugin_executable_boundary_from_root(&plugin_id, &root, &runner_records)
            else {
                continue;
            };
            if boundary.status != PluginExecutableBoundaryStatus::Enabled {
                continue;
            }
            let Some(command) = boundary.command.clone() else {
                continue;
            };
            let key = format!(
                "{}::{}",
                plugin_id.to_ascii_lowercase(),
                root.to_string_lossy().to_ascii_lowercase()
            );
            if seen.insert(key) {
                runners.push(ConfiguredPluginRunner {
                    plugin_id,
                    root,
                    command,
                    args: boundary.args,
                    capabilities: boundary.capabilities,
                });
            }
        }
    }

    if let Ok(raw) = std::env::var("OPPI_PLUGIN_SOURCES") {
        for source in raw
            .split([';', ','])
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            let Some(root) = resolve_local_plugin_root(source, Some(cwd)) else {
                continue;
            };
            let manifest_name = plugin_manifest_metadata(&root).0;
            let plugin_id = manifest_name.unwrap_or_else(|| plugin_id_from_source(source));
            let Some(boundary) =
                plugin_executable_boundary_from_root(&plugin_id, &root, &runner_records)
            else {
                continue;
            };
            if boundary.status != PluginExecutableBoundaryStatus::Enabled {
                continue;
            }
            let Some(command) = boundary.command.clone() else {
                continue;
            };
            let key = format!(
                "{}::{}",
                plugin_id.to_ascii_lowercase(),
                root.to_string_lossy().to_ascii_lowercase()
            );
            if seen.insert(key) {
                runners.push(ConfiguredPluginRunner {
                    plugin_id,
                    root,
                    command,
                    args: boundary.args,
                    capabilities: boundary.capabilities,
                });
            }
        }
    }

    runners
}

fn plugin_id_from_source(source: &str) -> String {
    Path::new(source)
        .file_stem()
        .or_else(|| Path::new(source).file_name())
        .and_then(|name| name.to_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("plugin")
        .to_string()
}

fn plugin_ref_from_store_entry(
    entry: StoredPluginEntry,
    scope: PluginScope,
    cwd: Option<&str>,
    records: &[StoredPluginMaterializationRecord],
    runner_records: &[StoredPluginRunnerRecord],
) -> PluginRef {
    let local_allowed = store_entry_allows_native_local_loading(&entry);
    let local_root = local_allowed
        .then(|| resolve_local_plugin_root(&entry.source, cwd))
        .flatten();
    let materialization_record = materialization_record_for_entry(&entry, scope, records);
    let materialized_root = materialization_record.map(|record| PathBuf::from(&record.cache_path));
    let root = local_root.clone().or(materialized_root.clone());
    let materialized = materialized_root.is_some();
    let materialization =
        materialization_ref_for_entry(&entry, local_root.as_deref(), materialization_record);
    let (manifest_name, manifest_version, manifest_description) = root
        .as_deref()
        .map(plugin_manifest_metadata)
        .unwrap_or((None, None, None));
    let resources = root
        .as_deref()
        .map(plugin_resource_refs_from_root)
        .unwrap_or_default();
    let mut diagnostics = plugin_resource_diagnostics(&resources);
    diagnostics.extend(plugin_store_metadata_diagnostics(&entry, materialized));
    if let Some(root) = root.as_deref() {
        diagnostics.extend(local_package_risk_diagnostics(root));
        diagnostics.extend(claude_plugin_diagnostics(root));
    }
    if !entry.enabled {
        diagnostics.push(PluginDiagnostic {
            code: "plugin_disabled".to_string(),
            message: "Plugin is configured but disabled".to_string(),
            severity: Some("info".to_string()),
        });
    }
    if entry.enabled && !entry.trusted {
        diagnostics.push(PluginDiagnostic {
            code: "plugin_untrusted".to_string(),
            message: "Plugin is enabled but not trusted for native loading".to_string(),
            severity: Some("warning".to_string()),
        });
    }
    if entry.enabled && entry.trusted && !local_allowed && !materialized {
        diagnostics.push(PluginDiagnostic {
            code: "non_local_plugin_unsupported".to_string(),
            message: "Rust native only loads local plugin resources in this slice".to_string(),
            severity: Some("warning".to_string()),
        });
    }
    if local_allowed && root.is_none() {
        diagnostics.push(PluginDiagnostic {
            code: "plugin_source_missing".to_string(),
            message: "Local plugin source path could not be resolved".to_string(),
            severity: Some("warning".to_string()),
        });
    }
    let source_type = plugin_entry_source_type(&entry);
    let name = entry.name.clone().or(manifest_name);
    let id = name
        .clone()
        .unwrap_or_else(|| plugin_id_from_source(&entry.source));
    let executable_boundary = root
        .as_deref()
        .and_then(|root| plugin_executable_boundary_from_root(&id, root, runner_records));
    let name = entry.name.or(name);
    let version = entry.version.or(manifest_version);
    let description = entry.description.or(manifest_description);
    PluginRef {
        id,
        source: entry.source,
        name,
        version,
        description,
        enabled: entry.enabled,
        trusted: Some(entry.trusted),
        scope: Some(scope),
        source_type: Some(source_type),
        materialization: Some(materialization),
        executable_boundary,
        resources,
        diagnostics,
    }
}

fn configured_plugin_refs(cwd: Option<&str>) -> Vec<PluginRef> {
    let mut refs = Vec::new();
    let records = read_plugin_materialization_records();
    let runner_records = cwd
        .map(read_plugin_runner_store)
        .map(|store| store.runners)
        .unwrap_or_default();
    for (scope, store_path) in plugin_store_paths(cwd) {
        refs.extend(read_plugin_store(&store_path).into_iter().map(|entry| {
            plugin_ref_from_store_entry(entry, scope, cwd, &records, &runner_records)
        }));
    }
    if let Ok(raw) = std::env::var("OPPI_PLUGIN_SOURCES") {
        refs.extend(
            raw.split([';', ','])
                .map(str::trim)
                .filter(|source| !source.is_empty())
                .map(|source| StoredPluginEntry {
                    name: None,
                    source: source.to_string(),
                    source_type: Some("local".to_string()),
                    enabled: true,
                    trusted: true,
                    description: None,
                    version: None,
                    license: None,
                    capabilities: Vec::new(),
                    manifest: None,
                    marketplace: None,
                })
                .map(|entry| {
                    plugin_ref_from_store_entry(
                        entry,
                        PluginScope::Env,
                        cwd,
                        &records,
                        &runner_records,
                    )
                }),
        );
    }
    refs.sort_by(|left, right| {
        left.scope
            .cmp(&right.scope)
            .then_with(|| left.id.cmp(&right.id))
            .then_with(|| left.source.cmp(&right.source))
    });
    refs
}

fn plugin_skill_candidates(cwd: Option<&str>) -> Vec<SkillCandidate> {
    configured_local_plugin_roots(cwd)
        .into_iter()
        .flat_map(|root| plugin_skill_candidates_from_root(&root))
        .collect()
}

fn plugin_prompt_template_candidates(cwd: Option<&str>) -> Vec<PromptTemplateCandidate> {
    configured_local_plugin_roots(cwd)
        .into_iter()
        .flat_map(|root| plugin_prompt_template_candidates_from_root(&root))
        .collect()
}

fn plugin_theme_candidates(cwd: Option<&str>) -> Vec<ThemeCandidate> {
    configured_local_plugin_roots(cwd)
        .into_iter()
        .flat_map(|root| plugin_theme_candidates_from_root(&root))
        .collect()
}

fn mcp_diagnostic(
    code: impl Into<String>,
    message: impl Into<String>,
    severity: impl Into<Option<String>>,
) -> McpDiagnostic {
    McpDiagnostic {
        code: code.into(),
        message: message.into(),
        severity: severity.into(),
    }
}

fn mcp_servers_object(value: &Value) -> Option<&serde_json::Map<String, Value>> {
    value
        .get("mcpServers")
        .or_else(|| value.get("mcp_servers"))
        .and_then(Value::as_object)
}

fn string_vec_field(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(|value| value.as_str().map(str::trim))
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn mcp_env_value_is_reference(value: &str) -> bool {
    let value = value.trim();
    value.starts_with("env:")
        || (value.starts_with("${") && value.ends_with('}'))
        || (value.starts_with('$') && value.len() > 1)
        || (value.starts_with('%') && value.ends_with('%') && value.len() > 2)
}

fn mcp_env_key_looks_secret(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    key.contains("token")
        || key.contains("secret")
        || key.contains("password")
        || key.contains("passwd")
        || key.contains("api_key")
        || key.contains("apikey")
        || key.contains("private_key")
}

fn mcp_env_map_field(
    value: Option<&Value>,
    diagnostics: &mut Vec<McpDiagnostic>,
) -> BTreeMap<String, String> {
    value
        .and_then(Value::as_object)
        .map(|values| {
            values
                .iter()
                .filter_map(|(key, value)| {
                    let value = value
                        .as_str()
                        .map(str::trim)
                        .filter(|value| !value.is_empty())?;
                    if mcp_env_key_looks_secret(key) && !mcp_env_value_is_reference(value) {
                        diagnostics.push(mcp_diagnostic(
                            "mcp_env_secret_redacted",
                            format!(
                                "MCP env value for {key} was omitted because it looks like a raw secret; use an env reference or authRef instead."
                            ),
                            Some("warning".to_string()),
                        ));
                        None
                    } else {
                        Some((key.clone(), value.to_string()))
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

fn mcp_auth_ref(value: &Value) -> Option<String> {
    string_field(value.get("authRef")).or_else(|| string_field(value.get("auth_ref")))
}

fn streamable_http_requires_diagnostic(endpoint: &str, auth_ref: Option<&str>) -> bool {
    let endpoint = endpoint.trim().to_ascii_lowercase();
    let local = endpoint.starts_with("http://localhost")
        || endpoint.starts_with("http://127.0.0.1")
        || endpoint.starts_with("http://[::1]");
    let https = endpoint.starts_with("https://");
    !(local || https) || auth_ref.is_none()
}

fn mcp_ref_from_config_entry(
    id: &str,
    entry: &Value,
    source: McpServerSource,
    default_cwd: Option<&Path>,
) -> Option<McpServerRef> {
    let object = entry.as_object()?;
    let name = string_field(object.get("name")).unwrap_or_else(|| id.to_string());
    let description = string_field(object.get("description"));
    let when_to_use =
        string_field(object.get("whenToUse")).or_else(|| string_field(object.get("when_to_use")));
    let mut diagnostics = vec![mcp_diagnostic(
        "mcp_requires_trust",
        "MCP server imported as disabled until trusted and explicitly enabled.",
        Some("info".to_string()),
    )];
    let (transport, status) = if let Some(command) = string_field(object.get("command")) {
        (
            Some(McpTransportConfig::Stdio {
                command,
                args: string_vec_field(object.get("args")),
                env: mcp_env_map_field(object.get("env"), &mut diagnostics),
                cwd: string_field(object.get("cwd"))
                    .or_else(|| default_cwd.map(|path| path.to_string_lossy().to_string())),
            }),
            McpServerStatus::Disabled,
        )
    } else if let Some(endpoint) =
        string_field(object.get("endpoint")).or_else(|| string_field(object.get("url")))
    {
        let auth_ref = mcp_auth_ref(entry);
        if streamable_http_requires_diagnostic(&endpoint, auth_ref.as_deref()) {
            diagnostics.push(mcp_diagnostic(
                "mcp_transport_untrusted",
                "Streamable HTTP MCP endpoints require localhost or https plus authRef before they can be trusted.",
                Some("warning".to_string()),
            ));
        }
        (
            Some(McpTransportConfig::StreamableHttp { endpoint, auth_ref }),
            McpServerStatus::Disabled,
        )
    } else {
        diagnostics.push(mcp_diagnostic(
            "mcp_transport_missing",
            "MCP server entry has no supported stdio command or Streamable HTTP endpoint.",
            Some("warning".to_string()),
        ));
        (None, McpServerStatus::Failed)
    };

    Some(McpServerRef {
        id: id.to_string(),
        name,
        status,
        source: Some(source),
        transport,
        trusted: false,
        description,
        tools: Vec::new(),
        tool_refs: Vec::new(),
        resource_subscriptions: Vec::new(),
        when_to_use,
        capabilities: McpServerCapabilities::default(),
        list_refresh_hints: McpListRefreshHints::default(),
        diagnostics,
    })
}

fn mcp_refs_from_config_file(
    path: &Path,
    source: McpServerSource,
    default_cwd: Option<&Path>,
) -> Vec<McpServerRef> {
    let Some(value) = read_json_value(path) else {
        return Vec::new();
    };
    mcp_refs_from_config_value(&value, source, default_cwd)
}

fn mcp_refs_from_config_value(
    value: &Value,
    source: McpServerSource,
    default_cwd: Option<&Path>,
) -> Vec<McpServerRef> {
    let Some(servers) = mcp_servers_object(value) else {
        return Vec::new();
    };
    servers
        .iter()
        .filter_map(|(id, entry)| {
            let id = id.trim();
            (!id.is_empty())
                .then(|| mcp_ref_from_config_entry(id, entry, source, default_cwd))
                .flatten()
        })
        .collect()
}

fn apply_plugin_mcp_trust(
    plugin_id: &str,
    mut server: McpServerRef,
    trust_records: &[StoredPluginMcpTrustRecord],
) -> McpServerRef {
    let trusted = plugin_mcp_trust_record(trust_records, plugin_id, &server.id)
        .is_some_and(|record| record.enabled && record.trusted);
    if trusted {
        server.trusted = true;
        if server.status == McpServerStatus::Disabled && server.transport.is_some() {
            server.status = McpServerStatus::Disconnected;
        }
        server.diagnostics.retain(|diagnostic| {
            diagnostic.code != "mcp_requires_trust" && diagnostic.code != "mcp_plugin_metadata_only"
        });
    }
    server
}

fn plugin_mcp_refs_from_root(
    root: &Path,
    plugin_id: &str,
    trust_records: &[StoredPluginMcpTrustRecord],
) -> Vec<McpServerRef> {
    if let Some(value) = read_json_value(&root.join("oppi-plugin.json")) {
        return mcp_refs_from_config_value(&value, McpServerSource::Plugin, Some(root))
            .into_iter()
            .map(|mut server| {
                server.diagnostics.push(mcp_diagnostic(
                    "mcp_plugin_metadata_only",
                    "Plugin MCP server metadata is visible but not executed by Rust native until explicitly enabled through the MCP broker.",
                    Some("info".to_string()),
                ));
                apply_plugin_mcp_trust(plugin_id, server, trust_records)
            })
            .collect();
    }

    let Some(package_json) = read_json_value(&root.join("package.json")) else {
        return Vec::new();
    };
    let mut candidates = vec![&package_json];
    if let Some(oppi_plugin) = package_json.get("oppiPlugin") {
        candidates.push(oppi_plugin);
    }
    if let Some(pi) = package_json.get("pi") {
        candidates.push(pi);
    }
    candidates
        .into_iter()
        .flat_map(|value| mcp_refs_from_config_value(value, McpServerSource::Plugin, Some(root)))
        .map(|mut server| {
            server.diagnostics.push(mcp_diagnostic(
                "mcp_plugin_metadata_only",
                "Plugin MCP server metadata is visible but not executed by Rust native until explicitly enabled through the MCP broker.",
                Some("info".to_string()),
            ));
            apply_plugin_mcp_trust(plugin_id, server, trust_records)
        })
        .collect()
}

fn configured_plugin_mcp_refs(cwd: Option<&str>) -> Vec<McpServerRef> {
    let trust_records = cwd
        .map(read_plugin_mcp_trust_store)
        .map(|store| store.servers)
        .unwrap_or_default();
    configured_plugin_mcp_roots(cwd)
        .into_iter()
        .flat_map(|entry| plugin_mcp_refs_from_root(&entry.root, &entry.plugin_id, &trust_records))
        .collect()
}

fn configured_cli_mcp_refs(cwd: Option<&str>) -> Vec<McpServerRef> {
    let Ok(raw) = std::env::var("OPPI_MCP_CONFIG") else {
        return Vec::new();
    };
    let default_cwd = cwd.map(PathBuf::from);
    raw.split([';', ','])
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .flat_map(|path| {
            mcp_refs_from_config_file(&path, McpServerSource::Cli, default_cwd.as_deref())
        })
        .collect()
}

fn configured_mcp_refs(cwd: Option<&str>) -> Vec<McpServerRef> {
    let mut refs = BTreeMap::<McpServerId, McpServerRef>::new();
    for server in configured_plugin_mcp_refs(cwd) {
        refs.insert(server.id.clone(), server);
    }
    if let Some(home) = oppi_home_dir() {
        for server in mcp_refs_from_config_file(&home.join("mcp.json"), McpServerSource::User, None)
        {
            refs.insert(server.id.clone(), server);
        }
    }
    if let Some(cwd) = cwd {
        let cwd = PathBuf::from(cwd);
        for server in
            mcp_refs_from_config_file(&cwd.join(".mcp.json"), McpServerSource::Project, Some(&cwd))
        {
            refs.insert(server.id.clone(), server);
        }
    }
    for server in configured_cli_mcp_refs(cwd) {
        refs.insert(server.id.clone(), server);
    }
    refs.into_values().collect()
}

fn mcp_error(code: &str, message: impl Into<String>) -> RuntimeError {
    RuntimeError::new(code, RuntimeErrorCategory::Provider, message)
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct McpHttpSessionState {
    session_id: Option<String>,
    protocol_version: Option<String>,
}

fn mcp_http_auth_header_from_ref(auth_ref: Option<&str>) -> Result<String, RuntimeError> {
    let Some(auth_ref) = auth_ref.map(str::trim).filter(|value| !value.is_empty()) else {
        return Err(mcp_error(
            "mcp_http_auth_missing",
            "Streamable HTTP MCP transport requires authRef",
        ));
    };
    let Some(env_name) = auth_ref.strip_prefix("env:") else {
        return Err(mcp_error(
            "mcp_http_auth_unsupported",
            "Streamable HTTP MCP authRef must be an env:VAR reference",
        ));
    };
    let env_name = env_name.trim();
    if env_name.is_empty()
        || !env_name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    {
        return Err(mcp_error(
            "mcp_http_auth_invalid",
            "Streamable HTTP MCP authRef contains an invalid env var name",
        ));
    }
    let token = std::env::var(env_name).map_err(|_| {
        mcp_error(
            "mcp_http_auth_missing",
            format!("Streamable HTTP MCP auth env var {env_name} is not set"),
        )
    })?;
    let token = token.trim();
    if token.is_empty() {
        return Err(mcp_error(
            "mcp_http_auth_missing",
            format!("Streamable HTTP MCP auth env var {env_name} is empty"),
        ));
    }
    Ok(format!("Bearer {token}"))
}

fn mcp_auth_ref_env_name(auth_ref: &str) -> Option<&str> {
    let env_name = auth_ref.trim().strip_prefix("env:")?.trim();
    if env_name.is_empty()
        || !env_name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    {
        return None;
    }
    Some(env_name)
}

fn mcp_auth_ref_env_state_for_server(server: &McpServerRef) -> Option<(String, String)> {
    let Some(McpTransportConfig::StreamableHttp {
        auth_ref: Some(auth_ref),
        ..
    }) = server.transport.as_ref()
    else {
        return None;
    };
    let env_name = mcp_auth_ref_env_name(auth_ref)?;
    let state = match std::env::var(env_name) {
        Ok(value) if !value.trim().is_empty() => "present",
        Ok(_) => "empty",
        Err(_) => "missing",
    };
    Some((env_name.to_string(), state.to_string()))
}

fn mcp_auth_guidance_for_server(server: &McpServerRef) -> String {
    const PREFIX: &str = "MCP OAuth/browser auth is deferred in Rust-native OPPi";
    let Some(McpTransportConfig::StreamableHttp { auth_ref, .. }) = server.transport.as_ref()
    else {
        return format!(
            "{PREFIX}; configure an env-backed authRef such as authRef: \"env:MCP_TOKEN\" in the MCP server config, then run /mcp connect."
        );
    };
    let Some(auth_ref) = auth_ref
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return format!(
            "{PREFIX}; Streamable HTTP MCP auth uses env-backed authRef values. Add authRef: \"env:MCP_TOKEN\" in the MCP server config, then run /mcp connect."
        );
    };
    let Some(env_name) = mcp_auth_ref_env_name(auth_ref) else {
        return format!(
            "{PREFIX}; Streamable HTTP MCP authRef {auth_ref:?} is unsupported. Use authRef: \"env:MCP_TOKEN\" in the MCP server config, then run /mcp connect."
        );
    };
    match std::env::var(env_name) {
        Ok(value) if !value.trim().is_empty() => format!(
            "{PREFIX}; this server is configured with authRef: \"env:{env_name}\" and environment variable {env_name} is set. Run /mcp connect to use it."
        ),
        Ok(_) => format!(
            "{PREFIX}; this server is configured with authRef: \"env:{env_name}\", but environment variable {env_name} is empty. Set it in the oppi-server environment, then run /mcp connect."
        ),
        Err(_) => format!(
            "{PREFIX}; this server is configured with authRef: \"env:{env_name}\", but environment variable {env_name} is not set. Set it in the oppi-server environment, then run /mcp connect."
        ),
    }
}

fn mcp_jsonrpc_result_from_response(
    method: &str,
    id: u64,
    response: Value,
) -> Result<Value, RuntimeError> {
    if response.get("id").and_then(Value::as_u64) != Some(id) {
        return Err(mcp_error(
            "mcp_http_response_id_mismatch",
            format!("MCP Streamable HTTP response id did not match request for {method}"),
        ));
    }
    if let Some(error) = response.get("error") {
        return Err(mcp_error(
            "mcp_jsonrpc_error",
            format!("MCP {method} returned error: {error}"),
        ));
    }
    response
        .get("result")
        .cloned()
        .ok_or_else(|| mcp_error("mcp_result_missing", "MCP response omitted result"))
}

fn mcp_http_json_response_result(method: &str, id: u64, body: &str) -> Result<Value, RuntimeError> {
    let response = serde_json::from_str::<Value>(body).map_err(|error| {
        mcp_error(
            "mcp_http_response_invalid_json",
            format!("MCP Streamable HTTP response was not valid JSON: {error}"),
        )
    })?;
    mcp_jsonrpc_result_from_response(method, id, response)
}

fn mcp_transport_notification_method(message: &Value) -> Option<&str> {
    if message.get("id").is_some() {
        return None;
    }
    message
        .get("method")
        .and_then(Value::as_str)
        .filter(|method| method.starts_with("notifications/"))
}

#[cfg(test)]
fn mcp_http_sse_response_result(method: &str, id: u64, body: &str) -> Result<Value, RuntimeError> {
    mcp_http_sse_response_result_with_notifications(method, id, body).map(|result| result.0)
}

fn mcp_http_sse_response_result_with_notifications(
    method: &str,
    id: u64,
    body: &str,
) -> Result<(Value, Vec<Value>), RuntimeError> {
    let mut notifications = Vec::new();
    for event in body.split("\n\n") {
        let data = event
            .lines()
            .filter_map(|line| {
                let line = line.trim_end_matches('\r');
                line.strip_prefix("data:")
                    .map(|data| data.strip_prefix(' ').unwrap_or(data))
            })
            .collect::<Vec<_>>()
            .join("\n");
        let data = data.trim();
        if data.is_empty() || data == "[DONE]" {
            continue;
        }
        let response = serde_json::from_str::<Value>(data).map_err(|error| {
            mcp_error(
                "mcp_http_sse_invalid_json",
                format!("MCP Streamable HTTP SSE event was not valid JSON: {error}"),
            )
        })?;
        if mcp_transport_notification_method(&response).is_some() {
            notifications.push(response);
            continue;
        }
        if response.get("id").and_then(Value::as_u64) == Some(id) {
            return mcp_jsonrpc_result_from_response(method, id, response)
                .map(|result| (result, notifications));
        }
    }
    Err(mcp_error(
        "mcp_http_sse_response_missing",
        format!("MCP Streamable HTTP SSE stream omitted response for {method}"),
    ))
}

trait McpTransportSession {
    fn request(&mut self, method: &str, params: Value) -> Result<Value, RuntimeError>;
    fn notify(&mut self, method: &str, params: Value) -> Result<(), RuntimeError>;
    fn take_notifications(&mut self) -> Vec<Value> {
        Vec::new()
    }
    fn session_state(&self) -> Option<McpHttpSessionState> {
        None
    }
    fn shutdown(&mut self);
}

#[cfg(test)]
struct MockMcpTransport {
    responses: VecDeque<Value>,
    notifications: Vec<Value>,
}

#[cfg(test)]
impl MockMcpTransport {
    fn new(responses: VecDeque<Value>) -> Self {
        Self {
            responses,
            notifications: Vec::new(),
        }
    }
}

#[cfg(test)]
impl McpTransportSession for MockMcpTransport {
    fn request(&mut self, method: &str, _params: Value) -> Result<Value, RuntimeError> {
        loop {
            let response = self.responses.pop_front().ok_or_else(|| {
                mcp_error(
                    "mcp_mock_response_missing",
                    format!("mock MCP transport has no response for {method}"),
                )
            })?;
            if mcp_transport_notification_method(&response).is_some() {
                self.notifications.push(response);
                continue;
            }
            return Ok(response);
        }
    }

    fn notify(&mut self, _method: &str, _params: Value) -> Result<(), RuntimeError> {
        Ok(())
    }

    fn take_notifications(&mut self) -> Vec<Value> {
        std::mem::take(&mut self.notifications)
    }

    fn shutdown(&mut self) {}
}

struct StdioMcpTransport {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<std::process::ChildStdout>,
    next_id: u64,
    notifications: Vec<Value>,
}

impl StdioMcpTransport {
    fn spawn(config: &McpTransportConfig) -> Result<Self, RuntimeError> {
        let McpTransportConfig::Stdio {
            command,
            args,
            env,
            cwd,
        } = config
        else {
            return Err(mcp_error(
                "mcp_transport_unsupported",
                "only stdio MCP transport is supported by the native broker slice",
            ));
        };
        let mut command_builder = Command::new(command);
        command_builder
            .args(args)
            .envs(env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(cwd) = cwd {
            command_builder.current_dir(cwd);
        }
        let mut child = command_builder.spawn().map_err(|error| {
            mcp_error(
                "mcp_stdio_spawn_failed",
                format!("failed to start MCP stdio server {command}: {error}"),
            )
        })?;
        let stdin = child.stdin.take().ok_or_else(|| {
            mcp_error(
                "mcp_stdio_stdin_unavailable",
                "MCP stdio child did not expose stdin",
            )
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            mcp_error(
                "mcp_stdio_stdout_unavailable",
                "MCP stdio child did not expose stdout",
            )
        })?;
        Ok(Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
            next_id: 1,
            notifications: Vec::new(),
        })
    }

    fn write_message(&mut self, message: Value) -> Result<(), RuntimeError> {
        serde_json::to_writer(&mut self.stdin, &message).map_err(|error| {
            mcp_error(
                "mcp_stdio_encode_failed",
                format!("failed to encode MCP JSON-RPC message: {error}"),
            )
        })?;
        self.stdin.write_all(b"\n").map_err(|error| {
            mcp_error(
                "mcp_stdio_write_failed",
                format!("failed to write MCP JSON-RPC message: {error}"),
            )
        })?;
        self.stdin.flush().map_err(|error| {
            mcp_error(
                "mcp_stdio_flush_failed",
                format!("failed to flush MCP JSON-RPC message: {error}"),
            )
        })
    }
}

impl McpTransportSession for StdioMcpTransport {
    fn request(&mut self, method: &str, params: Value) -> Result<Value, RuntimeError> {
        let id = self.next_id;
        self.next_id += 1;
        self.write_message(json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        }))?;
        let mut line = String::new();
        loop {
            line.clear();
            let read = self.stdout.read_line(&mut line).map_err(|error| {
                mcp_error(
                    "mcp_stdio_read_failed",
                    format!("failed to read MCP JSON-RPC response: {error}"),
                )
            })?;
            if read == 0 {
                return Err(mcp_error(
                    "mcp_stdio_eof",
                    format!("MCP stdio server closed stdout before responding to {method}"),
                ));
            }
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let response: Value = serde_json::from_str(trimmed).map_err(|error| {
                mcp_error(
                    "mcp_stdio_response_invalid_json",
                    format!("MCP response was not valid JSON: {error}"),
                )
            })?;
            if mcp_transport_notification_method(&response).is_some() {
                self.notifications.push(response);
                continue;
            }
            if response.get("id").and_then(Value::as_u64) != Some(id) {
                continue;
            }
            if let Some(error) = response.get("error") {
                return Err(mcp_error(
                    "mcp_jsonrpc_error",
                    format!("MCP {method} returned error: {error}"),
                ));
            }
            return response
                .get("result")
                .cloned()
                .ok_or_else(|| mcp_error("mcp_result_missing", "MCP response omitted result"));
        }
    }

    fn notify(&mut self, method: &str, params: Value) -> Result<(), RuntimeError> {
        self.write_message(json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        }))
    }

    fn take_notifications(&mut self) -> Vec<Value> {
        std::mem::take(&mut self.notifications)
    }

    fn shutdown(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct StreamableHttpMcpTransport {
    endpoint: String,
    auth_header: String,
    agent: ureq::Agent,
    next_id: u64,
    session: McpHttpSessionState,
    notifications: Vec<Value>,
}

impl StreamableHttpMcpTransport {
    fn new(
        endpoint: String,
        auth_ref: Option<&str>,
        session: McpHttpSessionState,
    ) -> Result<Self, RuntimeError> {
        if streamable_http_requires_diagnostic(&endpoint, auth_ref) {
            return Err(mcp_error(
                "mcp_http_transport_untrusted",
                "Streamable HTTP MCP endpoints require localhost or https plus authRef",
            ));
        }
        Ok(Self {
            endpoint,
            auth_header: mcp_http_auth_header_from_ref(auth_ref)?,
            agent: ureq::AgentBuilder::new()
                .timeout(Duration::from_secs(30))
                .build(),
            next_id: 1,
            session,
            notifications: Vec::new(),
        })
    }

    fn post_request(&self) -> ureq::Request {
        let mut request = self
            .agent
            .post(&self.endpoint)
            .set("accept", "application/json, text/event-stream")
            .set("content-type", "application/json")
            .set("authorization", &self.auth_header);
        if let Some(session_id) = self.session.session_id.as_deref() {
            request = request.set("Mcp-Session-Id", session_id);
        }
        if let Some(protocol_version) = self.session.protocol_version.as_deref() {
            request = request.set("MCP-Protocol-Version", protocol_version);
        }
        request
    }

    fn update_session_from_response(&mut self, response: &ureq::Response) {
        if let Some(session_id) = response
            .header("Mcp-Session-Id")
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            self.session.session_id = Some(session_id.to_string());
        }
    }

    fn response_body(response: ureq::Response) -> Result<String, RuntimeError> {
        response.into_string().map_err(|error| {
            mcp_error(
                "mcp_http_response_read_failed",
                format!("failed to read MCP Streamable HTTP response body: {error}"),
            )
        })
    }

    fn http_error(method: &str, status: u16, response: ureq::Response) -> RuntimeError {
        let detail = response.into_string().unwrap_or_default();
        mcp_error(
            "mcp_http_error",
            format!(
                "MCP Streamable HTTP {method} failed with status {status}{}",
                if detail.trim().is_empty() {
                    String::new()
                } else {
                    format!(": {}", compact_provider_error(&detail))
                }
            ),
        )
    }
}

impl McpTransportSession for StreamableHttpMcpTransport {
    fn request(&mut self, method: &str, params: Value) -> Result<Value, RuntimeError> {
        let id = self.next_id;
        self.next_id += 1;
        let response = self.post_request().send_json(json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        }));
        let response = match response {
            Ok(response) => response,
            Err(ureq::Error::Status(status, response)) => {
                return Err(Self::http_error(method, status, response));
            }
            Err(error) => {
                return Err(mcp_error(
                    "mcp_http_transport_error",
                    format!("MCP Streamable HTTP {method} transport error: {error}"),
                ));
            }
        };
        self.update_session_from_response(&response);
        let content_type = response
            .header("content-type")
            .unwrap_or_default()
            .to_ascii_lowercase();
        let body = Self::response_body(response)?;
        if body.trim().is_empty() {
            return Err(mcp_error(
                "mcp_http_response_empty",
                format!("MCP Streamable HTTP {method} returned an empty response"),
            ));
        }
        let result = if content_type.contains("text/event-stream") {
            let (result, notifications) =
                mcp_http_sse_response_result_with_notifications(method, id, &body)?;
            self.notifications.extend(notifications);
            result
        } else {
            mcp_http_json_response_result(method, id, &body)?
        };
        if method == "initialize" {
            if let Some(protocol_version) = result
                .get("protocolVersion")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                self.session.protocol_version = Some(protocol_version.to_string());
            }
        }
        Ok(result)
    }

    fn notify(&mut self, method: &str, params: Value) -> Result<(), RuntimeError> {
        let response = self.post_request().send_json(json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        }));
        match response {
            Ok(response) => {
                self.update_session_from_response(&response);
                Ok(())
            }
            Err(ureq::Error::Status(status, response)) => {
                Err(Self::http_error(method, status, response))
            }
            Err(error) => Err(mcp_error(
                "mcp_http_transport_error",
                format!("MCP Streamable HTTP {method} transport error: {error}"),
            )),
        }
    }

    fn take_notifications(&mut self) -> Vec<Value> {
        std::mem::take(&mut self.notifications)
    }

    fn session_state(&self) -> Option<McpHttpSessionState> {
        Some(self.session.clone())
    }

    fn shutdown(&mut self) {}
}

fn mcp_transport_should_initialize(
    server: &McpServerRef,
    http_sessions: &BTreeMap<McpServerId, McpHttpSessionState>,
) -> bool {
    match server.transport.as_ref() {
        Some(McpTransportConfig::StreamableHttp { .. }) => !http_sessions.contains_key(&server.id),
        _ => true,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum McpListChangedKind {
    Tools,
    Resources,
    Prompts,
}

impl McpListChangedKind {
    fn label(self) -> &'static str {
        match self {
            Self::Tools => "tools",
            Self::Resources => "resources",
            Self::Prompts => "prompts",
        }
    }

    fn code(self) -> &'static str {
        match self {
            Self::Tools => "mcp_tools_list_changed",
            Self::Resources => "mcp_resources_list_changed",
            Self::Prompts => "mcp_prompts_list_changed",
        }
    }

    fn refresh_hint_slot(self, hints: &mut McpListRefreshHints) -> &mut Option<McpListRefreshHint> {
        match self {
            Self::Tools => &mut hints.tools,
            Self::Resources => &mut hints.resources,
            Self::Prompts => &mut hints.prompts,
        }
    }
}

fn mcp_list_changed_kind_from_transport_notification(
    notification: &Value,
) -> Option<McpListChangedKind> {
    match mcp_transport_notification_method(notification)? {
        "notifications/tools/list_changed" | "notifications/tools/listChanged" => {
            Some(McpListChangedKind::Tools)
        }
        "notifications/resources/list_changed" | "notifications/resources/listChanged" => {
            Some(McpListChangedKind::Resources)
        }
        "notifications/prompts/list_changed" | "notifications/prompts/listChanged" => {
            Some(McpListChangedKind::Prompts)
        }
        _ => None,
    }
}

fn mcp_resource_update_from_transport_notification(
    thread_id: &str,
    server_id: &str,
    notification: &Value,
) -> Option<McpResourceUpdatedParams> {
    if mcp_transport_notification_method(notification)? != "notifications/resources/updated" {
        return None;
    }
    let uri = notification
        .get("params")
        .and_then(|params| params.get("uri"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|uri| !uri.is_empty())?;
    Some(McpResourceUpdatedParams {
        thread_id: thread_id.to_string(),
        server_id: server_id.to_string(),
        uri: uri.to_string(),
        received_at: Some(now_millis().to_string()),
    })
}

fn mcp_initialize_payload(capability: &str) -> Value {
    let mut capabilities = serde_json::Map::new();
    capabilities.insert(capability.to_string(), json!({}));
    json!({
        "protocolVersion": "2025-11-25",
        "clientInfo": {
            "name": "oppi-native",
            "version": env!("CARGO_PKG_VERSION")
        },
        "capabilities": Value::Object(capabilities)
    })
}

fn initialize_mcp_transport(
    transport: &mut dyn McpTransportSession,
    capability: &str,
) -> Result<(), RuntimeError> {
    transport.request("initialize", mcp_initialize_payload(capability))?;
    transport.notify("notifications/initialized", json!({}))
}

fn mcp_bool_annotation(value: &Value, camel: &str, snake: &str) -> Option<bool> {
    value
        .get(camel)
        .or_else(|| value.get(snake))
        .and_then(Value::as_bool)
}

fn mcp_capability_json(capabilities: &serde_json::Map<String, Value>, key: &str) -> Option<Value> {
    capabilities
        .get(key)
        .filter(|value| !value.is_null())
        .cloned()
}

fn mcp_server_capabilities_from_initialize(initialize: &Value) -> McpServerCapabilities {
    let Some(capabilities) = initialize.get("capabilities").and_then(Value::as_object) else {
        return McpServerCapabilities::default();
    };
    let tools = capabilities.get("tools").map(|value| McpToolsCapability {
        list_changed: mcp_bool_annotation(value, "listChanged", "list_changed"),
    });
    let resources = capabilities
        .get("resources")
        .map(|value| McpResourcesCapability {
            subscribe: mcp_bool_annotation(value, "subscribe", "subscribe"),
            list_changed: mcp_bool_annotation(value, "listChanged", "list_changed"),
        });
    let prompts = capabilities
        .get("prompts")
        .map(|value| McpPromptsCapability {
            list_changed: mcp_bool_annotation(value, "listChanged", "list_changed"),
        });

    McpServerCapabilities {
        tools,
        resources,
        prompts,
        sampling: mcp_capability_json(capabilities, "sampling"),
        elicitation: mcp_capability_json(capabilities, "elicitation"),
        tasks: mcp_capability_json(capabilities, "tasks"),
    }
}

fn mcp_server_has_notification_capabilities(capabilities: &McpServerCapabilities) -> bool {
    capabilities
        .tools
        .as_ref()
        .and_then(|tools| tools.list_changed)
        .unwrap_or(false)
        || capabilities
            .resources
            .as_ref()
            .map(|resources| {
                resources.subscribe.unwrap_or(false) || resources.list_changed.unwrap_or(false)
            })
            .unwrap_or(false)
        || capabilities
            .prompts
            .as_ref()
            .and_then(|prompts| prompts.list_changed)
            .unwrap_or(false)
}

fn sync_mcp_notification_listener_diagnostic(server: &mut McpServerRef) {
    const CODE: &str = "mcp_notification_listener_deferred";
    server
        .diagnostics
        .retain(|diagnostic| diagnostic.code != CODE);
    if server.status == McpServerStatus::Enabled
        && mcp_server_has_notification_capabilities(&server.capabilities)
    {
        server.diagnostics.push(mcp_diagnostic(
            CODE.to_string(),
            "MCP notifications are captured during active MCP requests only; durable background listeners are not running yet."
                .to_string(),
            Some("info".to_string()),
        ));
    }
}

fn mcp_tool_ref_from_value(server_id: &str, value: &Value) -> Option<McpToolRef> {
    let name = string_field(value.get("name"))?;
    let annotations = value.get("annotations").unwrap_or(&Value::Null);
    Some(McpToolRef {
        server_id: server_id.to_string(),
        name,
        title: string_field(value.get("title")),
        description: string_field(value.get("description")),
        input_schema: value
            .get("inputSchema")
            .or_else(|| value.get("input_schema"))
            .cloned()
            .unwrap_or_else(|| json!({ "type": "object", "additionalProperties": true })),
        output_schema: value
            .get("outputSchema")
            .or_else(|| value.get("output_schema"))
            .cloned(),
        annotations: McpToolAnnotations {
            read_only_hint: mcp_bool_annotation(annotations, "readOnlyHint", "read_only_hint"),
            destructive_hint: mcp_bool_annotation(
                annotations,
                "destructiveHint",
                "destructive_hint",
            ),
            idempotent_hint: mcp_bool_annotation(annotations, "idempotentHint", "idempotent_hint"),
            open_world_hint: mcp_bool_annotation(annotations, "openWorldHint", "open_world_hint"),
        },
    })
}

fn mcp_tool_refs_from_tools_list(
    server_id: &str,
    result: &Value,
) -> Result<Vec<McpToolRef>, RuntimeError> {
    let tools = result
        .get("tools")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            mcp_error(
                "mcp_tools_list_malformed",
                "MCP tools/list result omitted tools array",
            )
        })?;
    Ok(tools
        .iter()
        .filter_map(|value| mcp_tool_ref_from_value(server_id, value))
        .collect())
}

fn mcp_resource_annotations(value: &Value) -> BTreeMap<String, Value> {
    value
        .as_object()
        .map(|object| {
            object
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect()
        })
        .unwrap_or_default()
}

fn mcp_resource_ref_from_value(server_id: &str, value: &Value) -> Option<McpResourceRef> {
    let uri = string_field(value.get("uri"))?;
    let name = string_field(value.get("name"))?;
    Some(McpResourceRef {
        server_id: server_id.to_string(),
        uri,
        name,
        title: string_field(value.get("title")),
        description: string_field(value.get("description")),
        mime_type: string_field(value.get("mimeType").or_else(|| value.get("mime_type"))),
        annotations: mcp_resource_annotations(value.get("annotations").unwrap_or(&Value::Null)),
        size: value.get("size").and_then(Value::as_u64),
    })
}

fn mcp_resource_refs_from_resources_list(
    server_id: &str,
    result: &Value,
) -> Result<(Vec<McpResourceRef>, Option<String>), RuntimeError> {
    let resources = result
        .get("resources")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            mcp_error(
                "mcp_resources_list_malformed",
                "MCP resources/list result omitted resources array",
            )
        })?;
    let next_cursor = result
        .get("nextCursor")
        .or_else(|| result.get("next_cursor"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    Ok((
        resources
            .iter()
            .filter_map(|value| mcp_resource_ref_from_value(server_id, value))
            .collect(),
        next_cursor,
    ))
}

fn mcp_resource_template_ref_from_value(
    server_id: &str,
    value: &Value,
) -> Option<McpResourceTemplateRef> {
    let uri_template = string_field(
        value
            .get("uriTemplate")
            .or_else(|| value.get("uri_template")),
    )?;
    let name = string_field(value.get("name"))?;
    Some(McpResourceTemplateRef {
        server_id: server_id.to_string(),
        uri_template,
        name,
        title: string_field(value.get("title")),
        description: string_field(value.get("description")),
        mime_type: string_field(value.get("mimeType").or_else(|| value.get("mime_type"))),
        annotations: mcp_resource_annotations(value.get("annotations").unwrap_or(&Value::Null)),
    })
}

fn mcp_resource_template_refs_from_templates_list(
    server_id: &str,
    result: &Value,
) -> Result<(Vec<McpResourceTemplateRef>, Option<String>), RuntimeError> {
    let templates = result
        .get("resourceTemplates")
        .or_else(|| result.get("resource_templates"))
        .and_then(Value::as_array)
        .ok_or_else(|| {
            mcp_error(
                "mcp_resource_templates_list_malformed",
                "MCP resources/templates/list result omitted resourceTemplates array",
            )
        })?;
    let next_cursor = result
        .get("nextCursor")
        .or_else(|| result.get("next_cursor"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    Ok((
        templates
            .iter()
            .filter_map(|value| mcp_resource_template_ref_from_value(server_id, value))
            .collect(),
        next_cursor,
    ))
}

fn mcp_prompt_argument_ref_from_value(value: &Value) -> Option<McpPromptArgumentRef> {
    let name = string_field(value.get("name"))?;
    Some(McpPromptArgumentRef {
        name,
        title: string_field(value.get("title")),
        description: string_field(value.get("description")),
        required: value.get("required").and_then(Value::as_bool),
    })
}

fn mcp_prompt_ref_from_value(server_id: &str, value: &Value) -> Option<McpPromptRef> {
    let name = string_field(value.get("name"))?;
    let arguments = value
        .get("arguments")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(mcp_prompt_argument_ref_from_value)
                .collect()
        })
        .unwrap_or_default();
    Some(McpPromptRef {
        server_id: server_id.to_string(),
        name,
        title: string_field(value.get("title")),
        description: string_field(value.get("description")),
        arguments,
    })
}

fn mcp_prompt_refs_from_prompts_list(
    server_id: &str,
    result: &Value,
) -> Result<(Vec<McpPromptRef>, Option<String>), RuntimeError> {
    let prompts = result
        .get("prompts")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            mcp_error(
                "mcp_prompts_list_malformed",
                "MCP prompts/list result omitted prompts array",
            )
        })?;
    let next_cursor = result
        .get("nextCursor")
        .or_else(|| result.get("next_cursor"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    Ok((
        prompts
            .iter()
            .filter_map(|value| mcp_prompt_ref_from_value(server_id, value))
            .collect(),
        next_cursor,
    ))
}

fn mcp_prompt_messages_from_get_result(
    result: &Value,
) -> Result<(Option<String>, Vec<Value>), RuntimeError> {
    let messages = result
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            mcp_error(
                "mcp_prompt_get_malformed",
                "MCP prompts/get result omitted messages array",
            )
        })?
        .clone();
    Ok((string_field(result.get("description")), messages))
}

fn mcp_resource_content_from_value(value: &Value) -> Option<McpResourceContent> {
    let uri = string_field(value.get("uri"))?;
    Some(McpResourceContent {
        uri,
        mime_type: string_field(value.get("mimeType").or_else(|| value.get("mime_type"))),
        text: string_field(value.get("text")),
        blob: string_field(value.get("blob")),
    })
}

fn mcp_resource_contents_from_read_result(
    result: &Value,
) -> Result<Vec<McpResourceContent>, RuntimeError> {
    let contents = result
        .get("contents")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            mcp_error(
                "mcp_resources_read_malformed",
                "MCP resources/read result omitted contents array",
            )
        })?;
    Ok(contents
        .iter()
        .filter_map(mcp_resource_content_from_value)
        .collect())
}

fn sanitize_mcp_tool_identifier(value: &str, fallback: &str) -> String {
    let sanitized = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches('_')
        .to_string();
    if sanitized.is_empty() {
        fallback.to_string()
    } else {
        sanitized
    }
}

fn mcp_tool_annotations_to_tool_annotations(tool: &McpToolRef) -> BTreeMap<String, Value> {
    let mut annotations = BTreeMap::from([
        ("mcp.serverId".to_string(), json!(tool.server_id)),
        ("mcp.toolName".to_string(), json!(tool.name)),
    ]);
    if let Some(value) = tool.annotations.read_only_hint {
        annotations.insert("readOnlyHint".to_string(), json!(value));
    }
    if let Some(value) = tool.annotations.destructive_hint {
        annotations.insert("destructiveHint".to_string(), json!(value));
    }
    if let Some(value) = tool.annotations.idempotent_hint {
        annotations.insert("idempotentHint".to_string(), json!(value));
    }
    if let Some(value) = tool.annotations.open_world_hint {
        annotations.insert("openWorldHint".to_string(), json!(value));
    }
    annotations
}

fn mcp_tool_definition(tool: &McpToolRef) -> ToolDefinition {
    let read_only = tool.annotations.read_only_hint.unwrap_or(false);
    ToolDefinition {
        namespace: Some(format!(
            "mcp_{}",
            sanitize_mcp_tool_identifier(&tool.server_id, "server")
        )),
        name: sanitize_mcp_tool_identifier(&tool.name, "tool"),
        description: tool.description.clone().or_else(|| tool.title.clone()),
        concurrency_safe: read_only,
        requires_approval: !read_only,
        capabilities: vec!["mcp".to_string(), format!("mcp:{}", tool.server_id)],
        input_schema: Some(tool.input_schema.clone()),
        output_schema: tool.output_schema.clone(),
        annotations: mcp_tool_annotations_to_tool_annotations(tool),
    }
}

fn mcp_tool_uses_network(definition: &ToolDefinition) -> bool {
    definition
        .annotations
        .get("openWorldHint")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

#[derive(Debug, Clone)]
struct McpToolTarget {
    server_id: String,
    tool_name: String,
    input_schema: Option<Value>,
}

fn mcp_tool_target(definition: &ToolDefinition, call: &ToolCall) -> Option<McpToolTarget> {
    if !definition
        .capabilities
        .iter()
        .any(|capability| capability == "mcp")
    {
        return None;
    }
    let capability_server = definition
        .capabilities
        .iter()
        .find_map(|capability| capability.strip_prefix("mcp:"))
        .map(str::to_string);
    let annotation_server = definition
        .annotations
        .get("mcp.serverId")
        .and_then(Value::as_str)
        .map(str::to_string);
    let server_id = annotation_server.or(capability_server)?;
    let tool_name = definition
        .annotations
        .get("mcp.toolName")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| call.name.clone());
    Some(McpToolTarget {
        server_id,
        tool_name,
        input_schema: definition.input_schema.clone(),
    })
}

fn plugin_runner_tool_ref_from_value(
    plugin_id: &str,
    value: &Value,
) -> Option<PluginRunnerToolRef> {
    let name = string_field(value.get("name"))?;
    Some(PluginRunnerToolRef {
        plugin_id: plugin_id.to_string(),
        name,
        description: string_field(value.get("description")),
        input_schema: value
            .get("inputSchema")
            .or_else(|| value.get("input_schema"))
            .cloned()
            .unwrap_or_else(|| json!({ "type": "object", "additionalProperties": true })),
        read_only: value
            .get("readOnly")
            .or_else(|| value.get("read_only"))
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}

fn plugin_runner_tool_refs_from_tools_list(
    plugin_id: &str,
    result: &Value,
) -> Result<Vec<PluginRunnerToolRef>, RuntimeError> {
    let tools = result
        .get("tools")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            mcp_error(
                "plugin_runner_tools_list_malformed",
                "Plugin runner tools/list result omitted tools array",
            )
        })?;
    Ok(tools
        .iter()
        .filter_map(|value| plugin_runner_tool_ref_from_value(plugin_id, value))
        .collect())
}

fn plugin_runner_tool_definition(tool: &PluginRunnerToolRef) -> ToolDefinition {
    let mut annotations = BTreeMap::from([
        ("pluginRunner.pluginId".to_string(), json!(tool.plugin_id)),
        ("pluginRunner.toolName".to_string(), json!(tool.name)),
        ("readOnlyHint".to_string(), json!(tool.read_only)),
    ]);
    annotations.insert(
        "pluginRunner.boundary".to_string(),
        json!("jsonRpcSubprocess"),
    );
    ToolDefinition {
        namespace: Some(format!(
            "plugin_{}",
            sanitize_mcp_tool_identifier(&tool.plugin_id, "plugin")
        )),
        name: sanitize_mcp_tool_identifier(&tool.name, "tool"),
        description: tool.description.clone(),
        concurrency_safe: tool.read_only,
        requires_approval: !tool.read_only,
        capabilities: vec![
            "plugin-runner".to_string(),
            format!("plugin:{}", tool.plugin_id),
        ],
        input_schema: Some(tool.input_schema.clone()),
        output_schema: None,
        annotations,
    }
}

#[derive(Debug, Clone)]
struct PluginRunnerToolTarget {
    plugin_id: String,
    tool_name: String,
    input_schema: Option<Value>,
}

fn plugin_runner_tool_target(
    definition: &ToolDefinition,
    call: &ToolCall,
) -> Option<PluginRunnerToolTarget> {
    if !definition
        .capabilities
        .iter()
        .any(|capability| capability == "plugin-runner")
    {
        return None;
    }
    let capability_plugin = definition
        .capabilities
        .iter()
        .find_map(|capability| capability.strip_prefix("plugin:"))
        .map(str::to_string);
    let annotation_plugin = definition
        .annotations
        .get("pluginRunner.pluginId")
        .and_then(Value::as_str)
        .map(str::to_string);
    let plugin_id = annotation_plugin.or(capability_plugin)?;
    let tool_name = definition
        .annotations
        .get("pluginRunner.toolName")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| call.name.clone());
    Some(PluginRunnerToolTarget {
        plugin_id,
        tool_name,
        input_schema: definition.input_schema.clone(),
    })
}

fn mcp_call_arguments(arguments: &Value) -> Value {
    let mut arguments = arguments.clone();
    if let Value::Object(map) = &mut arguments {
        map.remove("approvalGranted");
        map.remove("policy");
        map.remove("requireApproval");
        map.remove("sandboxPreference");
        map.remove("sandbox_preference");
    }
    arguments
}

fn validate_mcp_tool_arguments(schema: Option<&Value>, arguments: &Value) -> Result<(), String> {
    let Some(schema) = schema else {
        return Ok(());
    };
    let schema_type = schema.get("type").and_then(Value::as_str);
    let required = schema
        .get("required")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if schema_type == Some("object") || !required.is_empty() {
        let Some(map) = arguments.as_object() else {
            return Err("MCP tool arguments must be an object".to_string());
        };
        if let Some(missing) = required
            .iter()
            .find(|field| !map.contains_key(field.as_str()))
            .cloned()
        {
            return Err(format!(
                "MCP tool arguments missing required field {missing}"
            ));
        }
    }
    Ok(())
}

fn mcp_call_result_output(result: &Value) -> String {
    let mut parts = Vec::new();
    if let Some(content) = result.get("content").and_then(Value::as_array) {
        for item in content {
            if let Some(text) = item.get("text").and_then(Value::as_str) {
                parts.push(text.to_string());
            } else if item != &Value::Null {
                parts.push(item.to_string());
            }
        }
    }
    if let Some(structured) = result.get("structuredContent") {
        parts.push(structured.to_string());
    }
    if parts.is_empty() {
        result.to_string()
    } else {
        parts.join("\n")
    }
}

fn mcp_call_tool_result_from_value(
    server_id: String,
    tool_name: String,
    result: Value,
) -> McpCallToolResult {
    let status = if result
        .get("isError")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        ToolResultStatus::Error
    } else {
        ToolResultStatus::Ok
    };
    let content = result
        .get("content")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let structured_content = result.get("structuredContent").cloned();
    let error = (status == ToolResultStatus::Error).then(|| {
        result
            .get("error")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| mcp_call_result_output(&result))
    });
    McpCallToolResult {
        server_id,
        tool_name,
        status,
        content,
        structured_content,
        error,
    }
}

fn mcp_tool_argument_paths(cwd: &str, arguments: &Value) -> Vec<String> {
    let mut paths = Vec::new();
    if let Value::Object(map) = arguments {
        for key in ["path", "root", "cwd"] {
            if let Some(path) = map.get(key).and_then(Value::as_str) {
                let resolved = resolve_project_path(cwd, path)
                    .map(|path| path.to_string_lossy().to_string())
                    .unwrap_or_else(|_| path.to_string());
                paths.push(resolved);
            }
        }
        if let Some(items) = map.get("touchedPaths").and_then(Value::as_array) {
            for item in items {
                if let Some(path) = item.as_str() {
                    paths.push(path.to_string());
                }
            }
        }
    }
    paths.sort();
    paths.dedup();
    paths
}

fn resolve_skill_candidates(
    candidates: Vec<SkillCandidate>,
) -> Vec<(SkillCandidate, Vec<SkillRef>)> {
    let mut by_name: BTreeMap<String, Vec<SkillCandidate>> = BTreeMap::new();
    for candidate in candidates {
        by_name
            .entry(candidate.reference.name.clone())
            .or_default()
            .push(candidate);
    }
    let mut resolved = by_name
        .into_values()
        .filter_map(|mut candidates| {
            candidates.sort_by(|left, right| {
                right
                    .reference
                    .priority
                    .cmp(&left.reference.priority)
                    .then_with(|| left.reference.path.cmp(&right.reference.path))
            });
            let active = candidates.remove(0);
            let shadowed = candidates
                .into_iter()
                .map(|candidate| candidate.reference)
                .collect::<Vec<_>>();
            Some((active, shadowed))
        })
        .collect::<Vec<_>>();
    resolved.sort_by(|left, right| left.0.reference.name.cmp(&right.0.reference.name));
    resolved
}

fn resolve_prompt_template_candidates(
    candidates: Vec<PromptTemplateCandidate>,
) -> Vec<(PromptTemplateCandidate, Vec<PromptTemplateRef>)> {
    let mut by_name: BTreeMap<String, Vec<PromptTemplateCandidate>> = BTreeMap::new();
    for candidate in candidates {
        by_name
            .entry(candidate.reference.name.to_ascii_lowercase())
            .or_default()
            .push(candidate);
    }
    let mut resolved = by_name
        .into_values()
        .filter_map(|mut candidates| {
            candidates.sort_by(|left, right| {
                right
                    .reference
                    .priority
                    .cmp(&left.reference.priority)
                    .then_with(|| left.reference.path.cmp(&right.reference.path))
            });
            let active = candidates.remove(0);
            let shadowed = candidates
                .into_iter()
                .map(|candidate| candidate.reference)
                .collect::<Vec<_>>();
            Some((active, shadowed))
        })
        .collect::<Vec<_>>();
    resolved.sort_by(|left, right| left.0.reference.name.cmp(&right.0.reference.name));
    resolved
}

fn canonical_builtin_theme_name(name: &str) -> String {
    match name.trim().to_ascii_lowercase().as_str() {
        "oppi-cyan-dark" => "oppi-cyan".to_string(),
        other => other.to_string(),
    }
}

fn builtin_theme_colors(name: &str) -> (Value, Option<Value>) {
    let canonical = canonical_builtin_theme_name(name);
    let name = canonical.as_str();
    let mut colors = serde_json::Map::new();
    let base = match name {
        "light" | "oppi-cyan-light" => "#16252f",
        "oppi-cyan" => "#d9e6ea",
        "plain" => "",
        _ => "#d8e2ea",
    };
    for token in PI_REQUIRED_THEME_COLOR_TOKENS {
        colors.insert(token.to_string(), json!(base));
    }
    match name {
        "oppi-cyan-light" => {
            colors.insert("accent".to_string(), json!("#006d7d"));
            colors.insert("border".to_string(), json!("#008fa3"));
            colors.insert("borderAccent".to_string(), json!("#006d7d"));
            colors.insert("borderMuted".to_string(), json!("#c7d8df"));
            colors.insert("success".to_string(), json!("#2e7d32"));
            colors.insert("error".to_string(), json!("#c62828"));
            colors.insert("warning".to_string(), json!("#a66a00"));
            colors.insert("muted".to_string(), json!("#526c78"));
            colors.insert("dim".to_string(), json!("#7c8c96"));
            colors.insert("text".to_string(), json!("#16252f"));
            colors.insert("selectedBg".to_string(), json!("#d7f4f8"));
            colors.insert("userMessageBg".to_string(), json!("#f8fbf8"));
            colors.insert("customMessageBg".to_string(), json!("#def3f4"));
            colors.insert("toolPendingBg".to_string(), json!("#e7f1ef"));
            colors.insert("toolSuccessBg".to_string(), json!("#edf8ef"));
            colors.insert("toolErrorBg".to_string(), json!("#fff0f0"));
            colors.insert("mdLink".to_string(), json!("#3267c9"));
            colors.insert("bashMode".to_string(), json!("#a66a00"));
            colors.insert("syntaxNumber".to_string(), json!("#7b3db8"));
            colors.insert("permissionAutoReviewingBg".to_string(), json!("#e0f8fc"));
            colors.insert("permissionAutoApprovedBg".to_string(), json!("#e8f6ea"));
            colors.insert("permissionAutoDeniedBg".to_string(), json!("#ffe7e7"));
            (
                Value::Object(colors),
                Some(json!({
                    "pageBg": "#eef7f4",
                    "cardBg": "#f8fbf8",
                    "infoBg": "#def3f4"
                })),
            )
        }
        "light" => {
            colors.insert("accent".to_string(), json!("#006d7d"));
            colors.insert("border".to_string(), json!("#009fb4"));
            colors.insert("borderAccent".to_string(), json!("#008fa3"));
            colors.insert("borderMuted".to_string(), json!("#c7d8df"));
            colors.insert("success".to_string(), json!("#2e7d32"));
            colors.insert("error".to_string(), json!("#c62828"));
            colors.insert("warning".to_string(), json!("#a66a00"));
            colors.insert("muted".to_string(), json!("#526c78"));
            colors.insert("dim".to_string(), json!("#7c8c96"));
            colors.insert("selectedBg".to_string(), json!("#d7f4f8"));
            colors.insert("userMessageBg".to_string(), json!("#f1f7f6"));
            colors.insert("customMessageBg".to_string(), json!("#e7f1ef"));
            colors.insert("toolPendingBg".to_string(), json!("#e7f1ef"));
            colors.insert("toolSuccessBg".to_string(), json!("#edf8ef"));
            colors.insert("toolErrorBg".to_string(), json!("#fff0f0"));
            colors.insert("mdLink".to_string(), json!("#3267c9"));
            colors.insert("bashMode".to_string(), json!("#d06a00"));
            colors.insert("syntaxNumber".to_string(), json!("#7b3db8"));
            colors.insert("permissionAutoReviewingBg".to_string(), json!("#e0f8fc"));
            colors.insert("permissionAutoApprovedBg".to_string(), json!("#e8f6ea"));
            colors.insert("permissionAutoDeniedBg".to_string(), json!("#ffe7e7"));
            (
                Value::Object(colors),
                Some(json!({ "pageBg": "#f8fbf8", "cardBg": "#f1f7f6" })),
            )
        }
        "plain" => {
            colors.insert("permissionAutoReviewingBg".to_string(), json!(""));
            colors.insert("permissionAutoApprovedBg".to_string(), json!(""));
            colors.insert("permissionAutoDeniedBg".to_string(), json!(""));
            (Value::Object(colors), Some(json!({ "pageBg": "" })))
        }
        "oppi-cyan" => {
            colors.insert("accent".to_string(), json!("#39d7e5"));
            colors.insert("border".to_string(), json!("#3b8f99"));
            colors.insert("borderAccent".to_string(), json!("#8be9f0"));
            colors.insert("borderMuted".to_string(), json!("#2f3b46"));
            colors.insert("success".to_string(), json!("#9ece6a"));
            colors.insert("error".to_string(), json!("#f7768e"));
            colors.insert("warning".to_string(), json!("#e0af68"));
            colors.insert("muted".to_string(), json!("#9aa7b2"));
            colors.insert("dim".to_string(), json!("#66717d"));
            colors.insert("text".to_string(), json!(""));
            colors.insert("selectedBg".to_string(), json!("#15323a"));
            colors.insert("userMessageBg".to_string(), json!("#17242c"));
            colors.insert("customMessageBg".to_string(), json!("#182832"));
            colors.insert("toolPendingBg".to_string(), json!("#13242b"));
            colors.insert("toolSuccessBg".to_string(), json!("#172a22"));
            colors.insert("toolErrorBg".to_string(), json!("#321b24"));
            colors.insert("mdLink".to_string(), json!("#7aa2f7"));
            colors.insert("bashMode".to_string(), json!("#e0af68"));
            colors.insert("syntaxNumber".to_string(), json!("#bb9af7"));
            colors.insert("permissionAutoReviewingBg".to_string(), json!("#102b35"));
            colors.insert("permissionAutoApprovedBg".to_string(), json!("#122d22"));
            colors.insert("permissionAutoDeniedBg".to_string(), json!("#351924"));
            (
                Value::Object(colors),
                Some(json!({
                    "pageBg": "#0f1419",
                    "cardBg": "#151f27",
                    "infoBg": "#172a30"
                })),
            )
        }
        _ => {
            colors.insert("accent".to_string(), json!("#39d7e5"));
            colors.insert("border".to_string(), json!("#3b8f99"));
            colors.insert("borderAccent".to_string(), json!("#8be9f0"));
            colors.insert("borderMuted".to_string(), json!("#2f3b46"));
            colors.insert("success".to_string(), json!("#9ece6a"));
            colors.insert("error".to_string(), json!("#f7768e"));
            colors.insert("warning".to_string(), json!("#e0af68"));
            colors.insert("muted".to_string(), json!("#9aa7b2"));
            colors.insert("dim".to_string(), json!("#66717d"));
            colors.insert("selectedBg".to_string(), json!("#15323a"));
            colors.insert("userMessageBg".to_string(), json!("#17242c"));
            colors.insert("customMessageBg".to_string(), json!("#182832"));
            colors.insert("toolPendingBg".to_string(), json!("#13242b"));
            colors.insert("toolSuccessBg".to_string(), json!("#172a22"));
            colors.insert("toolErrorBg".to_string(), json!("#321b24"));
            colors.insert("mdLink".to_string(), json!("#7aa2f7"));
            colors.insert("bashMode".to_string(), json!("#ff9e64"));
            colors.insert("syntaxNumber".to_string(), json!("#bb9af7"));
            colors.insert("permissionAutoReviewingBg".to_string(), json!("#102b35"));
            colors.insert("permissionAutoApprovedBg".to_string(), json!("#122d22"));
            colors.insert("permissionAutoDeniedBg".to_string(), json!("#351924"));
            (
                Value::Object(colors),
                Some(json!({ "pageBg": "#0c1318", "cardBg": "#17242c" })),
            )
        }
    }
}

fn builtin_theme_candidates() -> Vec<ThemeCandidate> {
    [
        "oppi",
        "oppi-cyan",
        "oppi-cyan-light",
        "dark",
        "light",
        "plain",
    ]
    .into_iter()
    .map(|name| {
        let (colors, export) = builtin_theme_colors(name);
        ThemeCandidate {
            reference: ThemeRef {
                id: name.to_string(),
                name: name.to_string(),
                source: ThemeSource::BuiltIn,
                path: None,
                priority: theme_source_priority(ThemeSource::BuiltIn),
            },
            colors,
            export,
        }
    })
    .collect()
}

fn resolve_theme_candidates(
    candidates: Vec<ThemeCandidate>,
) -> Vec<(ThemeCandidate, Vec<ThemeRef>)> {
    let mut by_name: BTreeMap<String, Vec<ThemeCandidate>> = BTreeMap::new();
    for candidate in candidates {
        by_name
            .entry(candidate.reference.name.to_ascii_lowercase())
            .or_default()
            .push(candidate);
    }
    let mut resolved = by_name
        .into_values()
        .filter_map(|mut candidates| {
            candidates.sort_by(|left, right| {
                right
                    .reference
                    .priority
                    .cmp(&left.reference.priority)
                    .then_with(|| left.reference.path.cmp(&right.reference.path))
            });
            let active = candidates.remove(0);
            let shadowed = candidates
                .into_iter()
                .map(|candidate| candidate.reference)
                .collect::<Vec<_>>();
            Some((active, shadowed))
        })
        .collect::<Vec<_>>();
    resolved.sort_by(|left, right| left.0.reference.name.cmp(&right.0.reference.name));
    resolved
}

fn skill_relevant(skill: &SkillRef, input: &str, explicit_names: &[String]) -> bool {
    let name = skill.name.to_ascii_lowercase();
    if explicit_names
        .iter()
        .any(|value| value.eq_ignore_ascii_case(&skill.name))
    {
        return true;
    }
    let input = input.to_ascii_lowercase();
    match name.as_str() {
        "imagegen" => [
            "image",
            "draw",
            "generate",
            "render",
            "picture",
            "logo",
            "illustration",
            "edit image",
        ]
        .iter()
        .any(|needle| input.contains(needle)),
        "mermaid-diagrams" => [
            "mermaid",
            "diagram",
            "flowchart",
            "sequence diagram",
            "state machine",
            "architecture map",
        ]
        .iter()
        .any(|needle| input.contains(needle)),
        "independent" => [
            "plan",
            "roadmap",
            "checklist",
            "todo",
            "autonomous",
            "continue",
            "work through",
            "finish line",
        ]
        .iter()
        .any(|needle| input.contains(needle)),
        "graphify" => [
            "graphify",
            "codebase graph",
            "knowledge graph",
            "architecture",
            "dependency",
            "dependencies",
            "cross-module",
            "repo-wide",
            "blast radius",
            "impact analysis",
            "god node",
            "community",
            "module map",
            "call graph",
            "why connected",
        ]
        .iter()
        .any(|needle| input.contains(needle)),
        _ => skill
            .description
            .split(|ch: char| !ch.is_alphanumeric())
            .filter(|word| word.len() >= 6)
            .any(|word| input.contains(&word.to_ascii_lowercase())),
    }
}

fn skill_injection_section(skill: &SkillCandidate) -> String {
    format!(
        "## Skill: {}\nSource: {:?}{}\nDescription: {}\n\n{}\n",
        skill.reference.name,
        skill.reference.source,
        skill
            .reference
            .path
            .as_deref()
            .map(|path| format!(" ({path})"))
            .unwrap_or_default(),
        skill.reference.description,
        skill.content
    )
}

fn parse_skill_command_input(input: &str) -> Option<(&str, &str)> {
    let trimmed = input.trim_start();
    let rest = trimmed.strip_prefix("/skill:")?;
    let (name, args) = if let Some(index) = rest.find(char::is_whitespace) {
        (&rest[..index], rest[index..].trim())
    } else {
        (rest, "")
    };
    let name = name.trim();
    if name.is_empty() {
        None
    } else {
        Some((name, args))
    }
}

fn strip_skill_frontmatter(content: &str) -> &str {
    let mut offset = 0;
    for (index, line) in content.split_inclusive('\n').enumerate() {
        let trimmed = line.trim_end_matches('\n').trim_end_matches('\r').trim();
        if index == 0 && trimmed != "---" {
            return content.trim();
        }
        offset += line.len();
        if index > 0 && trimmed == "---" {
            return content[offset..].trim();
        }
    }
    content.trim()
}

fn xml_attr_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn skill_location(skill: &SkillCandidate) -> String {
    skill
        .reference
        .path
        .clone()
        .unwrap_or_else(|| skill.reference.name.clone())
}

fn skill_base_dir(location: &str) -> String {
    if location.starts_with("builtin:") {
        return "builtin".to_string();
    }
    Path::new(location)
        .parent()
        .map(|path| path.display().to_string())
        .filter(|path| !path.is_empty())
        .unwrap_or_else(|| ".".to_string())
}

fn expand_skill_command_text(skill: &SkillCandidate, args: &str) -> String {
    let location = skill_location(skill);
    let base_dir = skill_base_dir(&location);
    let body = strip_skill_frontmatter(&skill.content);
    let skill_block = format!(
        "<skill name=\"{}\" location=\"{}\">\nReferences are relative to {}.\n\n{}\n</skill>",
        xml_attr_escape(&skill.reference.name),
        xml_attr_escape(&location),
        base_dir,
        body
    );
    if args.is_empty() {
        skill_block
    } else {
        format!("{skill_block}\n\n{args}")
    }
}

