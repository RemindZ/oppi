fn default_runtime_store_dir() -> PathBuf {
    env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(".oppi")
        .join("runtime-store")
}

fn home_dir() -> Option<PathBuf> {
    env::var_os("USERPROFILE")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("HOME")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        })
}

fn default_agent_dir() -> PathBuf {
    env::var_os("OPPI_AGENT_DIR")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("PI_CODING_AGENT_DIR")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        })
        .unwrap_or_else(|| {
            home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".oppi")
                .join("agent")
        })
}

fn auth_store_path() -> PathBuf {
    env::var_os("OPPI_OPENAI_CODEX_AUTH_PATH")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| default_agent_dir().join("auth.json"))
}

fn auth_lock_path(path: &Path) -> PathBuf {
    let mut lock_path = path.as_os_str().to_os_string();
    lock_path.push(".lock");
    PathBuf::from(lock_path)
}

#[derive(Debug)]
struct CodexAuthLock {
    path: PathBuf,
}

impl CodexAuthLock {
    fn acquire(auth_path: &Path) -> Result<Self, String> {
        if let Some(parent) = auth_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("create auth dir {}: {error}", parent.display()))?;
        }
        let lock_path = auth_lock_path(auth_path);
        let mut delay = Duration::from_millis(100);
        for attempt in 0..10 {
            match fs::create_dir(&lock_path) {
                Ok(()) => return Ok(Self { path: lock_path }),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    if auth_lock_is_stale(&lock_path) {
                        let _ = remove_auth_lock_path(&lock_path);
                        continue;
                    }
                    if attempt == 9 {
                        return Err(format!(
                            "could not acquire Pi-compatible auth lock {}; another OPPi/Pi process may be refreshing credentials",
                            lock_path.display()
                        ));
                    }
                    std::thread::sleep(delay);
                    delay = (delay * 2).min(Duration::from_secs(2));
                }
                Err(error) => {
                    return Err(format!("create auth lock {}: {error}", lock_path.display()));
                }
            }
        }
        Err(format!(
            "could not acquire auth lock {}",
            lock_path.display()
        ))
    }
}

impl Drop for CodexAuthLock {
    fn drop(&mut self) {
        let _ = remove_auth_lock_path(&self.path);
    }
}

fn auth_lock_is_stale(path: &Path) -> bool {
    fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| modified.elapsed().ok())
        .is_some_and(|elapsed| elapsed > Duration::from_secs(30))
}

fn remove_auth_lock_path(path: &Path) -> std::io::Result<()> {
    fs::remove_dir(path).or_else(|dir_error| {
        if path.is_file() {
            fs::remove_file(path)
        } else {
            Err(dir_error)
        }
    })
}

fn role_profile_settings_path() -> PathBuf {
    env::var_os("OPPI_ROLE_PROFILES_PATH")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| default_agent_dir().join("settings.json"))
}

fn read_json_or_empty(path: &Path) -> Result<Value, String> {
    match fs::read_to_string(path) {
        Ok(raw) => serde_json::from_str(&raw)
            .map_err(|error| format!("read {}: invalid JSON: {error}", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(json!({})),
        Err(error) => Err(format!("read {}: {error}", path.display())),
    }
}

fn write_private_json(path: &Path, data: &Value) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("create auth dir {}: {error}", parent.display()))?;
    }
    let rendered = serde_json::to_string_pretty(data)
        .map_err(|error| format!("serialize {}: {error}", path.display()))?;
    fs::write(path, format!("{rendered}\n"))
        .map_err(|error| format!("write {}: {error}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

fn form_url_encode(input: &str) -> String {
    input
        .bytes()
        .flat_map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                vec![byte as char]
            }
            b' ' => vec!['+'],
            _ => format!("%{byte:02X}").chars().collect::<Vec<_>>(),
        })
        .collect()
}

fn form_url_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut output = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                output.push(b' ');
                index += 1;
            }
            b'%' if index + 2 < bytes.len() => {
                let hex = &input[index + 1..index + 3];
                if let Ok(value) = u8::from_str_radix(hex, 16) {
                    output.push(value);
                    index += 3;
                } else {
                    output.push(bytes[index]);
                    index += 1;
                }
            }
            byte => {
                output.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&output).to_string()
}

fn query_param(path: &str, key: &str) -> Option<String> {
    let query = path
        .split_once('?')?
        .1
        .split('#')
        .next()
        .unwrap_or_default();
    query.split('&').find_map(|pair| {
        let (raw_key, raw_value) = pair.split_once('=')?;
        (form_url_decode(raw_key) == key).then(|| form_url_decode(raw_value))
    })
}

fn codex_token_form(params: &[(&str, &str)]) -> String {
    params
        .iter()
        .map(|(key, value)| format!("{}={}", form_url_encode(key), form_url_encode(value)))
        .collect::<Vec<_>>()
        .join("&")
}

fn codex_auth_present_at(path: &Path) -> bool {
    read_json_or_empty(path).ok().is_some_and(|data| {
        let Some(credential) = data.get(OPENAI_CODEX_PROVIDER_ID) else {
            return false;
        };
        credential.get("type").and_then(Value::as_str) == Some("oauth")
            && credential
                .get("access")
                .and_then(Value::as_str)
                .is_some_and(|value| !value.trim().is_empty())
            && credential
                .get("refresh")
                .and_then(Value::as_str)
                .is_some_and(|value| !value.trim().is_empty())
            && credential
                .get("accountId")
                .and_then(Value::as_str)
                .is_some_and(|value| !value.trim().is_empty())
    })
}

fn read_codex_usage_auth_at(path: &Path) -> Result<CodexUsageAuth, String> {
    let data = read_json_or_empty(path)?;
    let credential = data.get(OPENAI_CODEX_PROVIDER_ID).ok_or_else(|| {
        format!(
            "ChatGPT/Codex auth missing in {}; run `/login subscription codex`",
            path.display()
        )
    })?;
    if credential.get("type").and_then(Value::as_str) != Some("oauth") {
        return Err(format!(
            "ChatGPT/Codex credential in {} is not an OAuth credential",
            path.display()
        ));
    }
    Ok(CodexUsageAuth {
        access_token: credential
            .get("access")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| "ChatGPT/Codex credential is missing access token".to_string())?
            .to_string(),
        refresh_token: credential
            .get("refresh")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| "ChatGPT/Codex credential is missing refresh token".to_string())?
            .to_string(),
        expires: credential
            .get("expires")
            .and_then(Value::as_i64)
            .ok_or_else(|| "ChatGPT/Codex credential is missing expiry".to_string())?,
        account_id: credential
            .get("accountId")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| "ChatGPT/Codex credential is missing account id".to_string())?
            .to_string(),
    })
}

fn read_or_refresh_codex_usage_auth(agent: &ureq::Agent) -> Result<CodexUsageAuth, String> {
    let path = auth_store_path();
    let auth = read_codex_usage_auth_at(&path)?;
    let now = current_time_ms().unwrap_or_default().min(i64::MAX as u64) as i64;
    if auth.expires > now.saturating_add(60_000) {
        return Ok(auth);
    }
    let _lock = CodexAuthLock::acquire(&path)?;
    let auth = read_codex_usage_auth_at(&path)?;
    let now = current_time_ms().unwrap_or_default().min(i64::MAX as u64) as i64;
    if auth.expires > now.saturating_add(60_000) {
        return Ok(auth);
    }
    let body = codex_token_form(&[
        ("grant_type", "refresh_token"),
        ("refresh_token", &auth.refresh_token),
        ("client_id", OPENAI_CODEX_CLIENT_ID),
    ]);
    let value = agent
        .post(OPENAI_CODEX_TOKEN_URL)
        .set("content-type", "application/x-www-form-urlencoded")
        .send_string(&body)
        .map_err(|error| match error {
            ureq::Error::Status(status, _) => {
                format!("Codex OAuth refresh failed with HTTP {status}")
            }
            other => format!("Codex OAuth refresh failed: {other}"),
        })?
        .into_json::<Value>()
        .map_err(|error| format!("Codex OAuth refresh returned invalid JSON: {error}"))?;
    let access = value
        .get("access_token")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "Codex OAuth refresh did not return access_token".to_string())?;
    let refresh = value
        .get("refresh_token")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "Codex OAuth refresh did not return refresh_token".to_string())?;
    let expires_in = value
        .get("expires_in")
        .and_then(Value::as_i64)
        .filter(|value| *value > 0)
        .ok_or_else(|| "Codex OAuth refresh did not return expires_in".to_string())?;
    let account_id = codex_account_id(access)?;
    let expires = current_time_ms()
        .unwrap_or_default()
        .saturating_add((expires_in as u64).saturating_mul(1000))
        .min(i64::MAX as u64) as i64;
    let mut data = read_json_or_empty(&path)?;
    if !data.is_object() {
        data = json!({});
    }
    data[OPENAI_CODEX_PROVIDER_ID] = json!({
        "type": "oauth",
        "access": access,
        "refresh": refresh,
        "expires": expires,
        "accountId": account_id,
    });
    write_private_json(&path, &data)?;
    read_codex_usage_auth_at(&path)
}

fn codex_usage_provider_config(provider: &ProviderConfig) -> Option<&OpenAiCompatibleConfig> {
    match provider {
        ProviderConfig::OpenAiCompatible(config)
            if config.flavor == DirectProviderFlavor::OpenAiCodex =>
        {
            Some(config)
        }
        _ => None,
    }
}

fn normalize_codex_usage_base_url(config: &OpenAiCompatibleConfig) -> String {
    config
        .base_url
        .clone()
        .or_else(|| env::var("OPPI_OPENAI_CODEX_BASE_URL").ok())
        .unwrap_or_else(|| OPENAI_CODEX_DEFAULT_BASE_URL.to_string())
        .trim()
        .trim_end_matches('/')
        .to_string()
}

fn fetch_openai_codex_live_usage(
    config: &OpenAiCompatibleConfig,
) -> Result<NativeLiveUsage, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout(USAGE_FETCH_TIMEOUT)
        .build();
    let auth = read_or_refresh_codex_usage_auth(&agent)?;
    let base = normalize_codex_usage_base_url(config);
    let urls = [
        format!("{base}/wham/usage"),
        format!("{base}/api/codex/usage"),
    ];
    let mut last_error = None;
    for url in urls {
        let response = agent
            .get(&url)
            .set("authorization", &format!("Bearer {}", auth.access_token))
            .set("chatgpt-account-id", &auth.account_id)
            .set("originator", "pi")
            .set("accept", "application/json")
            .set("user-agent", "oppi-shell-usage/0.1.0")
            .call();
        match response {
            Ok(response) => {
                let payload = response
                    .into_json::<Value>()
                    .map_err(|error| format!("usage response was invalid JSON: {error}"))?;
                if let Some(snapshot) = openai_codex_usage_from_payload(&payload, &config.model) {
                    return Ok(snapshot);
                }
                last_error = Some("usage response did not include rate limit windows".to_string());
            }
            Err(ureq::Error::Status(status, response)) => {
                let body = response.into_string().unwrap_or_default();
                last_error = Some(format!("HTTP {status}: {}", compact_usage_error(&body)));
            }
            Err(error) => {
                last_error = Some(error.to_string());
            }
        }
    }
    Err(last_error.unwrap_or_else(|| "could not fetch ChatGPT/Codex usage".to_string()))
}

fn openai_codex_usage_from_payload(payload: &Value, model: &str) -> Option<NativeLiveUsage> {
    let plan_type = payload
        .get("plan_type")
        .and_then(Value::as_str)
        .map(str::to_string);
    let mut snapshots = Vec::new();
    snapshots.push((Some("codex".to_string()), None, payload.get("rate_limit")));
    if let Some(additional) = payload
        .get("additional_rate_limits")
        .and_then(Value::as_array)
    {
        for item in additional {
            snapshots.push((
                item.get("metered_feature")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                item.get("limit_name")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                item.get("rate_limit"),
            ));
        }
    }
    let selected = snapshots
        .iter()
        .find(|(id, name, raw)| {
            raw.is_some() && usage_limit_matches_model(id.as_deref(), name.as_deref(), model)
        })
        .or_else(|| {
            snapshots
                .iter()
                .find(|(id, _, raw)| raw.is_some() && id.as_deref() == Some("codex"))
        })
        .or_else(|| snapshots.iter().find(|(_, _, raw)| raw.is_some()))?;
    let windows = [
        selected.2?.get("primary_window"),
        selected.2?.get("secondary_window"),
    ]
    .into_iter()
    .flatten()
    .filter_map(parse_openai_usage_window)
    .collect::<Vec<_>>();
    if windows.is_empty() {
        return None;
    }
    let five_hour = windows
        .iter()
        .position(|window| window.label == "5h")
        .or(Some(0))
        .and_then(|index| windows.get(index).cloned());
    let weekly = windows
        .iter()
        .find(|window| window.label == "week")
        .or_else(|| {
            windows
                .iter()
                .rev()
                .find(|window| Some(*window) != five_hour.as_ref())
        })
        .cloned();
    Some(NativeLiveUsage {
        source: "openai-codex".to_string(),
        plan_type,
        five_hour,
        weekly,
        fetched_at_ms: current_time_ms().unwrap_or_default(),
    })
}

fn parse_openai_usage_window(raw: &Value) -> Option<NativeUsageWindow> {
    if raw.is_null() {
        return None;
    }
    let seconds = number_f64(raw.get("limit_window_seconds"));
    let label = match seconds {
        Some(value) if (value - 18_000.0).abs() <= 300.0 => "5h".to_string(),
        Some(value) if (value - 604_800.0).abs() <= 1_800.0 => "week".to_string(),
        Some(value) if value < 3_600.0 => format!("{}m", (value / 60.0).round() as i64),
        Some(value) if value < 86_400.0 => format!("{}h", (value / 3_600.0).round() as i64),
        Some(value) => format!("{}d", (value / 86_400.0).round() as i64),
        None => "limit".to_string(),
    };
    let resets_at = raw.get("reset_at").and_then(Value::as_u64).or_else(|| {
        let reset_after = number_f64(raw.get("reset_after_seconds"))?;
        let now = current_time_ms()? / 1000;
        Some(now.saturating_add(reset_after.max(0.0).round() as u64))
    });
    Some(NativeUsageWindow {
        label,
        used_percent: number_f64(raw.get("used_percent")).map(|value| value.clamp(0.0, 100.0)),
        resets_at,
    })
}

fn number_f64(value: Option<&Value>) -> Option<f64> {
    value.and_then(|value| {
        value
            .as_f64()
            .or_else(|| value.as_i64().map(|n| n as f64))
            .or_else(|| value.as_u64().map(|n| n as f64))
    })
}

fn usage_limit_matches_model(id: Option<&str>, name: Option<&str>, model: &str) -> bool {
    let model = normalize_usage_search(model);
    let haystack = normalize_usage_search(&format!(
        "{} {}",
        id.unwrap_or_default(),
        name.unwrap_or_default()
    ));
    !haystack.is_empty() && (haystack.contains(&model) || model.contains(&haystack))
}

fn normalize_usage_search(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn compact_usage_error(raw: &str) -> String {
    let compact = raw
        .replace(['\r', '\n', '\t'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if compact.len() > 220 {
        format!("{}...", &compact[..220])
    } else {
        compact
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GitHubCopilotStoredAuth {
    access_token: String,
    refresh_token: String,
    expires: i64,
    enterprise_domain: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GitHubCopilotDeviceFlow {
    device_code: String,
    user_code: String,
    verification_uri: String,
    interval_seconds: u64,
    expires_in_seconds: u64,
}

fn github_copilot_auth_store_path() -> PathBuf {
    env::var_os("OPPI_GITHUB_COPILOT_AUTH_PATH")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(auth_store_path)
}

fn read_github_copilot_auth_at(path: &Path) -> Result<GitHubCopilotStoredAuth, String> {
    let data = read_json_or_empty(path)?;
    let credential = data.get(GITHUB_COPILOT_PROVIDER_ID).ok_or_else(|| {
        format!(
            "GitHub Copilot auth missing in {}; run `/login subscription copilot`",
            path.display()
        )
    })?;
    if credential.get("type").and_then(Value::as_str) != Some("oauth") {
        return Err(format!(
            "GitHub Copilot credential in {} is not an OAuth credential",
            path.display()
        ));
    }
    Ok(GitHubCopilotStoredAuth {
        access_token: credential
            .get("access")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| "GitHub Copilot credential is missing access token".to_string())?
            .to_string(),
        refresh_token: credential
            .get("refresh")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| "GitHub Copilot credential is missing refresh token".to_string())?
            .to_string(),
        expires: credential
            .get("expires")
            .and_then(Value::as_i64)
            .ok_or_else(|| "GitHub Copilot credential is missing expiry".to_string())?,
        enterprise_domain: credential
            .get("enterpriseUrl")
            .or_else(|| credential.get("enterpriseDomain"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
    })
}

fn github_copilot_auth_present_at(path: &Path) -> bool {
    read_github_copilot_auth_at(path).is_ok()
}

fn github_copilot_base_url_from_token(token: &str) -> Option<String> {
    token.split(';').find_map(|part| {
        let (key, value) = part.split_once('=')?;
        if key.trim() != "proxy-ep" {
            return None;
        }
        let api_host = value.trim().strip_prefix("proxy.").unwrap_or(value.trim());
        Some(format!("https://api.{api_host}"))
    })
}

fn github_copilot_base_url(auth: &GitHubCopilotStoredAuth) -> String {
    github_copilot_base_url_from_token(&auth.access_token)
        .or_else(|| {
            auth.enterprise_domain
                .as_ref()
                .map(|domain| format!("https://copilot-api.{domain}"))
        })
        .unwrap_or_else(|| GITHUB_COPILOT_DEFAULT_BASE_URL.to_string())
}

fn normalize_github_enterprise_domain(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }
    let without_scheme = trimmed
        .strip_prefix("https://")
        .or_else(|| trimmed.strip_prefix("http://"))
        .unwrap_or(trimmed);
    let host = without_scheme
        .split('/')
        .next()
        .unwrap_or(without_scheme)
        .trim()
        .trim_matches('.');
    if host.is_empty()
        || !host
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '.')
    {
        return None;
    }
    Some(host.to_ascii_lowercase())
}

fn github_copilot_enterprise_domain_from_flags(flags: &[&str]) -> Result<Option<String>, String> {
    let mut index = 0;
    while index < flags.len() {
        let flag = flags[index].trim();
        if let Some(value) = flag
            .strip_prefix("--enterprise=")
            .or_else(|| flag.strip_prefix("--domain="))
        {
            return normalize_github_enterprise_domain(value)
                .map(Some)
                .ok_or_else(|| "invalid GitHub Enterprise domain".to_string());
        }
        if matches!(flag, "--enterprise" | "enterprise" | "--domain" | "domain") {
            let Some(value) = flags.get(index + 1) else {
                return Err("GitHub Copilot enterprise login requires a domain".to_string());
            };
            return normalize_github_enterprise_domain(value)
                .map(Some)
                .ok_or_else(|| "invalid GitHub Enterprise domain".to_string());
        }
        index += 1;
    }
    Ok(None)
}

fn github_copilot_urls(domain: &str) -> (String, String, String) {
    (
        format!("https://{domain}/login/device/code"),
        format!("https://{domain}/login/oauth/access_token"),
        format!("https://api.{domain}/copilot_internal/v2/token"),
    )
}

fn start_github_copilot_device_flow(domain: &str) -> Result<GitHubCopilotDeviceFlow, String> {
    let (device_url, _, _) = github_copilot_urls(domain);
    let body = codex_token_form(&[
        ("client_id", GITHUB_COPILOT_CLIENT_ID),
        ("scope", "read:user"),
    ]);
    let value = ureq::post(&device_url)
        .set("accept", "application/json")
        .set("content-type", "application/x-www-form-urlencoded")
        .set("User-Agent", "GitHubCopilotChat/0.35.0")
        .send_string(&body)
        .map_err(|error| format!("GitHub Copilot device-code request failed: {error}"))?
        .into_json::<Value>()
        .map_err(|error| {
            format!("GitHub Copilot device-code response was invalid JSON: {error}")
        })?;
    Ok(GitHubCopilotDeviceFlow {
        device_code: value
            .get("device_code")
            .and_then(Value::as_str)
            .ok_or_else(|| "GitHub Copilot device-code response missing device_code".to_string())?
            .to_string(),
        user_code: value
            .get("user_code")
            .and_then(Value::as_str)
            .ok_or_else(|| "GitHub Copilot device-code response missing user_code".to_string())?
            .to_string(),
        verification_uri: value
            .get("verification_uri")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                "GitHub Copilot device-code response missing verification_uri".to_string()
            })?
            .to_string(),
        interval_seconds: value
            .get("interval")
            .and_then(Value::as_u64)
            .filter(|value| *value > 0)
            .ok_or_else(|| "GitHub Copilot device-code response missing interval".to_string())?,
        expires_in_seconds: value
            .get("expires_in")
            .and_then(Value::as_u64)
            .filter(|value| *value > 0)
            .ok_or_else(|| "GitHub Copilot device-code response missing expires_in".to_string())?,
    })
}

fn poll_github_copilot_device_flow(
    domain: &str,
    device: &GitHubCopilotDeviceFlow,
) -> Result<String, String> {
    let (_, token_url, _) = github_copilot_urls(domain);
    let deadline = Instant::now() + Duration::from_secs(device.expires_in_seconds);
    let mut interval = Duration::from_secs(device.interval_seconds).max(Duration::from_secs(1));
    let mut multiplier = 1.2f64;
    let mut slow_downs = 0u32;
    while Instant::now() < deadline {
        let wait = interval.mul_f64(multiplier).min(Duration::from_secs(30));
        std::thread::sleep(wait);
        let body = codex_token_form(&[
            ("client_id", GITHUB_COPILOT_CLIENT_ID),
            ("device_code", &device.device_code),
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
        ]);
        let value = match ureq::post(&token_url)
            .set("accept", "application/json")
            .set("content-type", "application/x-www-form-urlencoded")
            .set("User-Agent", "GitHubCopilotChat/0.35.0")
            .send_string(&body)
        {
            Ok(response) => response.into_json::<Value>().map_err(|error| {
                format!("GitHub Copilot device-token response was invalid JSON: {error}")
            })?,
            Err(ureq::Error::Status(_, response)) => {
                response.into_json::<Value>().unwrap_or_else(|_| json!({}))
            }
            Err(error) => {
                return Err(format!(
                    "GitHub Copilot device-token request failed: {error}"
                ));
            }
        };
        if let Some(access) = value
            .get("access_token")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
        {
            return Ok(access.to_string());
        }
        match value.get("error").and_then(Value::as_str) {
            Some("authorization_pending") => continue,
            Some("slow_down") => {
                slow_downs += 1;
                interval = value
                    .get("interval")
                    .and_then(Value::as_u64)
                    .filter(|value| *value > 0)
                    .map(Duration::from_secs)
                    .unwrap_or_else(|| interval + Duration::from_secs(5));
                multiplier = 1.4;
            }
            Some(error) => {
                let description = value
                    .get("error_description")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                return Err(format!(
                    "GitHub Copilot device flow failed: {error}{}{}",
                    if description.is_empty() { "" } else { ": " },
                    description
                ));
            }
            None => {
                return Err("GitHub Copilot device-token response missing access_token".to_string());
            }
        }
    }
    if slow_downs > 0 {
        Err("GitHub Copilot device flow timed out after slow_down responses; check local clock drift and try again.".to_string())
    } else {
        Err("GitHub Copilot device flow timed out".to_string())
    }
}

fn refresh_github_copilot_token(
    github_access_token: &str,
    enterprise_domain: Option<&str>,
) -> Result<Value, String> {
    let domain = enterprise_domain.unwrap_or(GITHUB_COPILOT_DEFAULT_DOMAIN);
    let (_, _, token_url) = github_copilot_urls(domain);
    ureq::get(&token_url)
        .set("accept", "application/json")
        .set("authorization", &format!("Bearer {github_access_token}"))
        .set("User-Agent", "GitHubCopilotChat/0.35.0")
        .set("Editor-Version", "vscode/1.107.0")
        .set("Editor-Plugin-Version", "copilot-chat/0.35.0")
        .set("Copilot-Integration-Id", "vscode-chat")
        .call()
        .map_err(|error| format!("GitHub Copilot token refresh failed: {error}"))?
        .into_json::<Value>()
        .map_err(|error| format!("GitHub Copilot token refresh returned invalid JSON: {error}"))
}

fn persist_github_copilot_oauth(
    path: &Path,
    github_access_token: &str,
    enterprise_domain: Option<&str>,
    token: Value,
) -> Result<(), String> {
    let access = token
        .get("token")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "GitHub Copilot token refresh did not return token".to_string())?;
    let expires_at = token
        .get("expires_at")
        .and_then(Value::as_i64)
        .filter(|value| *value > 0)
        .ok_or_else(|| "GitHub Copilot token refresh did not return expires_at".to_string())?;
    let expires = expires_at
        .saturating_mul(1000)
        .saturating_sub(5 * 60 * 1000);
    let _lock = CodexAuthLock::acquire(path)?;
    let mut data = read_json_or_empty(path)?;
    if !data.is_object() {
        data = json!({});
    }
    let mut credential = json!({
        "type": "oauth",
        "access": access,
        "refresh": github_access_token,
        "expires": expires,
    });
    if let Some(domain) = enterprise_domain {
        credential["enterpriseUrl"] = json!(domain);
    }
    data[GITHUB_COPILOT_PROVIDER_ID] = credential;
    write_private_json(path, &data)
}

fn oauth_random_bytes<const N: usize>() -> Result<[u8; N], String> {
    let mut bytes = [0u8; N];
    getrandom::getrandom(&mut bytes)
        .map_err(|error| format!("generate OAuth random bytes: {error}"))?;
    Ok(bytes)
}

fn create_codex_oauth_flow() -> Result<(String, String, String), String> {
    let verifier = URL_SAFE_NO_PAD.encode(oauth_random_bytes::<32>()?);
    let digest = ring::digest::digest(&ring::digest::SHA256, verifier.as_bytes());
    let challenge = URL_SAFE_NO_PAD.encode(digest.as_ref());
    let state = oauth_random_bytes::<16>()?
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let params = [
        ("response_type", "code"),
        ("client_id", OPENAI_CODEX_CLIENT_ID),
        ("redirect_uri", OPENAI_CODEX_REDIRECT_URI),
        ("scope", OPENAI_CODEX_SCOPE),
        ("code_challenge", challenge.as_str()),
        ("code_challenge_method", "S256"),
        ("state", state.as_str()),
        ("id_token_add_organizations", "true"),
        ("codex_cli_simplified_flow", "true"),
        ("originator", "pi"),
    ];
    let query = params
        .iter()
        .map(|(key, value)| format!("{}={}", form_url_encode(key), form_url_encode(value)))
        .collect::<Vec<_>>()
        .join("&");
    Ok((
        verifier,
        state,
        format!("{OPENAI_CODEX_AUTHORIZE_URL}?{query}"),
    ))
}

fn open_browser(url: &str) -> Result<(), String> {
    let mut command = if cfg!(windows) {
        let mut command = Command::new("cmd");
        command.args(["/C", "start", "", url]);
        command
    } else if cfg!(target_os = "macos") {
        let mut command = Command::new("open");
        command.arg(url);
        command
    } else {
        let mut command = Command::new("xdg-open");
        command.arg(url);
        command
    };
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("open browser for Codex login: {error}"))
}

fn oauth_response_html(message: &str) -> String {
    format!(
        "<!doctype html><html><body><h1>OPPi Codex login</h1><p>{}</p></body></html>",
        message
    )
}

fn read_http_request_path(stream: &mut TcpStream) -> Result<String, String> {
    let mut reader = BufReader::new(stream.try_clone().map_err(|error| error.to_string())?);
    let mut request_line = String::new();
    reader
        .read_line(&mut request_line)
        .map_err(|error| format!("read OAuth callback: {error}"))?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let path = parts.next().unwrap_or_default();
    if method != "GET" || path.is_empty() {
        return Err("OAuth callback did not send a GET request".to_string());
    }
    Ok(path.to_string())
}

fn write_oauth_http_response(stream: &mut TcpStream, status: &str, message: &str) {
    let body = oauth_response_html(message);
    let response = format!(
        "HTTP/1.1 {status}\r\ncontent-type: text/html; charset=utf-8\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

struct CodexOAuthListener {
    listener: TcpListener,
    host: String,
}

impl CodexOAuthListener {
    fn bind() -> Result<Self, String> {
        let host = env::var("PI_OAUTH_CALLBACK_HOST")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "127.0.0.1".to_string());
        let listener = TcpListener::bind((host.as_str(), 1455))
            .map_err(|error| format!("bind OAuth callback at {host}:1455: {error}"))?;
        listener
            .set_nonblocking(true)
            .map_err(|error| format!("configure OAuth callback listener: {error}"))?;
        Ok(Self { listener, host })
    }

    fn wait_for_code(&self, state: &str, timeout: Duration) -> Result<String, String> {
        let started = Instant::now();
        while started.elapsed() < timeout {
            match self.listener.accept() {
                Ok((mut stream, _addr)) => {
                    let path = match read_http_request_path(&mut stream) {
                        Ok(path) => path,
                        Err(error) => {
                            write_oauth_http_response(&mut stream, "400 Bad Request", &error);
                            return Err(error);
                        }
                    };
                    if !path.starts_with("/auth/callback") {
                        write_oauth_http_response(
                            &mut stream,
                            "404 Not Found",
                            "Callback route not found.",
                        );
                        continue;
                    }
                    let Some(code) = query_param(&path, "code") else {
                        write_oauth_http_response(
                            &mut stream,
                            "400 Bad Request",
                            "Missing authorization code.",
                        );
                        return Err("OAuth callback was missing the authorization code".to_string());
                    };
                    if query_param(&path, "state").as_deref() != Some(state) {
                        write_oauth_http_response(
                            &mut stream,
                            "400 Bad Request",
                            "State mismatch.",
                        );
                        return Err("OAuth callback state mismatch".to_string());
                    }
                    write_oauth_http_response(
                        &mut stream,
                        "200 OK",
                        "OpenAI authentication completed. You can close this window.",
                    );
                    return Ok(code);
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(100));
                }
                Err(error) => return Err(format!("accept OAuth callback: {error}")),
            }
        }
        Err(format!(
            "Timed out waiting for the browser OAuth callback from ChatGPT/Codex at http://{}:1455/auth/callback.",
            self.host
        ))
    }
}

fn decode_jwt_payload(token: &str) -> Result<Value, String> {
    let payload = token
        .split('.')
        .nth(1)
        .ok_or_else(|| "OAuth access token is not a JWT".to_string())?;
    let decoded = URL_SAFE_NO_PAD
        .decode(payload.as_bytes())
        .map_err(|error| format!("decode OAuth access token payload: {error}"))?;
    serde_json::from_slice(&decoded)
        .map_err(|error| format!("parse OAuth access token payload: {error}"))
}

fn codex_account_id(access_token: &str) -> Result<String, String> {
    decode_jwt_payload(access_token)?
        .get("https://api.openai.com/auth")
        .and_then(|auth| auth.get("chatgpt_account_id"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "OAuth access token did not include a ChatGPT account id".to_string())
}

fn exchange_codex_oauth_code(code: &str, verifier: &str) -> Result<Value, String> {
    let body = [
        ("grant_type", "authorization_code"),
        ("client_id", OPENAI_CODEX_CLIENT_ID),
        ("code", code),
        ("code_verifier", verifier),
        ("redirect_uri", OPENAI_CODEX_REDIRECT_URI),
    ]
    .iter()
    .map(|(key, value)| format!("{}={}", form_url_encode(key), form_url_encode(value)))
    .collect::<Vec<_>>()
    .join("&");
    let response = ureq::post(OPENAI_CODEX_TOKEN_URL)
        .set("content-type", "application/x-www-form-urlencoded")
        .send_string(&body)
        .map_err(|error| match error {
            ureq::Error::Status(status, _) => {
                format!("Codex OAuth token exchange failed with HTTP {status}")
            }
            other => format!("Codex OAuth token exchange failed: {other}"),
        })?;
    response
        .into_json::<Value>()
        .map_err(|error| format!("Codex OAuth token exchange returned invalid JSON: {error}"))
}

fn persist_codex_oauth(path: &Path, token: Value) -> Result<(), String> {
    let access = token
        .get("access_token")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "Codex OAuth token exchange did not return access_token".to_string())?;
    let refresh = token
        .get("refresh_token")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "Codex OAuth token exchange did not return refresh_token".to_string())?;
    let expires_in = token
        .get("expires_in")
        .and_then(Value::as_i64)
        .filter(|value| *value > 0)
        .ok_or_else(|| "Codex OAuth token exchange did not return expires_in".to_string())?;
    let account_id = codex_account_id(access)?;
    let expires = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .saturating_add((expires_in as u128).saturating_mul(1000))
        .min(i64::MAX as u128) as i64;
    let _lock = CodexAuthLock::acquire(path)?;
    let mut data = read_json_or_empty(path)?;
    if !data.is_object() {
        data = json!({});
    }
    data[OPENAI_CODEX_PROVIDER_ID] = json!({
        "type": "oauth",
        "access": access,
        "refresh": refresh,
        "expires": expires,
        "accountId": account_id,
    });
    write_private_json(path, &data)
}

fn read_settings_json(path: &Path) -> Result<Value, String> {
    match fs::read_to_string(path) {
        Ok(raw) => serde_json::from_str(&raw)
            .map_err(|error| format!("read settings {}: invalid JSON: {error}", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(json!({})),
        Err(error) => Err(format!("read settings {}: {error}", path.display())),
    }
}

fn load_role_profiles(path: &Path) -> BTreeMap<String, String> {
    let Ok(data) = read_settings_json(path) else {
        return BTreeMap::new();
    };
    let Some(raw) = data
        .get("oppi")
        .and_then(|oppi| oppi.get("roleModels"))
        .and_then(Value::as_object)
    else {
        return BTreeMap::new();
    };
    raw.iter()
        .filter_map(|(role, model)| {
            let role = normalize_role(role)?;
            let model = model.as_str()?.trim();
            (!model.is_empty()).then(|| (role.to_string(), model.to_string()))
        })
        .collect()
}

fn save_role_profiles(path: &Path, role_models: &BTreeMap<String, String>) -> Result<(), String> {
    let mut data = read_settings_json(path)?;
    if !data.is_object() {
        data = json!({});
    }
    if !data.get("oppi").is_some_and(Value::is_object) {
        data["oppi"] = json!({});
    }
    data["oppi"]["roleModels"] = json!(role_models);
    write_settings_json(path, &data)
}

fn load_permission_mode_setting(path: &Path) -> (PermissionMode, String) {
    if let Some(mode) = permission_mode_from_env() {
        return (mode, "env:OPPI_RUNTIME_WORKER_PERMISSION_MODE".to_string());
    }
    if let Some(mode) = read_settings_json(path).ok().and_then(|data| {
        data.get("oppi")
            .and_then(|oppi| oppi.get("permissionMode"))
            .and_then(Value::as_str)
            .and_then(parse_permission_mode)
    }) {
        return (mode, "settings".to_string());
    }
    (PermissionMode::AutoReview, "default".to_string())
}

fn save_permission_mode_setting(path: &Path, mode: PermissionMode) -> Result<(), String> {
    let mut data = read_settings_json(path)?;
    if !data.is_object() {
        data = json!({});
    }
    if !data.get("oppi").is_some_and(Value::is_object) {
        data["oppi"] = json!({});
    }
    data["oppi"]["permissionMode"] = json!(mode.as_str());
    write_settings_json(path, &data)
}

fn load_permission_review_settings(path: &Path) -> GuardianReviewSettings {
    let default = GuardianReviewSettings {
        timeout_ms: DEFAULT_PERMISSION_REVIEW_TIMEOUT_MS,
        reviewer_model: None,
    };
    let Ok(data) = read_settings_json(path) else {
        return default;
    };
    let Some(permissions) = data
        .get("oppi")
        .and_then(|oppi| oppi.get("permissions"))
        .and_then(Value::as_object)
    else {
        return default;
    };
    let timeout_ms = permissions
        .get("reviewTimeoutMs")
        .and_then(Value::as_u64)
        .filter(|value| *value > 0)
        .unwrap_or(default.timeout_ms);
    let reviewer_model = permissions
        .get("reviewerModel")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty() && !value.eq_ignore_ascii_case("auto"))
        .map(str::to_string);
    GuardianReviewSettings {
        timeout_ms,
        reviewer_model,
    }
}

fn save_permission_review_settings(
    path: &Path,
    settings: &GuardianReviewSettings,
) -> Result<(), String> {
    let mut data = read_settings_json(path)?;
    if !data.is_object() {
        data = json!({});
    }
    if !data.get("oppi").is_some_and(Value::is_object) {
        data["oppi"] = json!({});
    }
    if !data["oppi"]
        .get("permissions")
        .is_some_and(Value::is_object)
    {
        data["oppi"]["permissions"] = json!({});
    }
    data["oppi"]["permissions"]["reviewTimeoutMs"] = json!(settings.timeout_ms);
    data["oppi"]["permissions"]["reviewerModel"] =
        json!(settings.reviewer_model.as_deref().unwrap_or("auto"));
    write_settings_json(path, &data)
}

fn load_permission_sandbox_settings(path: &Path) -> PermissionSandboxSettings {
    let Ok(data) = read_settings_json(path) else {
        return PermissionSandboxSettings::default();
    };
    let Some(permissions) = data
        .get("oppi")
        .and_then(|oppi| oppi.get("permissions"))
        .and_then(Value::as_object)
    else {
        return PermissionSandboxSettings::default();
    };
    let protected_patterns = permissions
        .get("sandbox")
        .and_then(|sandbox| sandbox.get("protectedPatterns"))
        .or_else(|| permissions.get("protectedPatterns"))
        .map(normalize_string_array)
        .unwrap_or_default();
    let project_roots = permissions
        .get("sandbox")
        .and_then(|sandbox| sandbox.get("projectRoots"))
        .or_else(|| permissions.get("projectRoots"))
        .map(normalize_string_array)
        .unwrap_or_default();
    let readable_roots = permissions
        .get("sandbox")
        .and_then(|sandbox| sandbox.get("readableRoots"))
        .or_else(|| permissions.get("readableRoots"))
        .map(normalize_string_array)
        .unwrap_or_default();
    let writable_roots = permissions
        .get("sandbox")
        .and_then(|sandbox| sandbox.get("writableRoots"))
        .or_else(|| permissions.get("writableRoots"))
        .map(normalize_string_array)
        .unwrap_or_default();
    let allow_internet = permissions
        .get("sandbox")
        .and_then(|sandbox| sandbox.get("allowInternet"))
        .or_else(|| permissions.get("allowInternet"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let allow_ssh = permissions
        .get("sandbox")
        .and_then(|sandbox| sandbox.get("allowSsh"))
        .or_else(|| permissions.get("allowSsh"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    PermissionSandboxSettings {
        allow_internet,
        allow_ssh,
        project_roots,
        readable_roots,
        writable_roots,
        protected_patterns,
    }
}

fn save_permission_sandbox_settings(
    path: &Path,
    settings: &PermissionSandboxSettings,
) -> Result<(), String> {
    let mut data = read_settings_json(path)?;
    if !data.is_object() {
        data = json!({});
    }
    if !data.get("oppi").is_some_and(Value::is_object) {
        data["oppi"] = json!({});
    }
    if !data["oppi"]
        .get("permissions")
        .is_some_and(Value::is_object)
    {
        data["oppi"]["permissions"] = json!({});
    }
    if !data["oppi"]["permissions"]
        .get("sandbox")
        .is_some_and(Value::is_object)
    {
        data["oppi"]["permissions"]["sandbox"] = json!({});
    }
    data["oppi"]["permissions"]["sandbox"]["allowInternet"] = json!(settings.allow_internet);
    data["oppi"]["permissions"]["sandbox"]["allowSsh"] = json!(settings.allow_ssh);
    data["oppi"]["permissions"]["sandbox"]["projectRoots"] =
        json!(normalize_string_slice(&settings.project_roots));
    data["oppi"]["permissions"]["sandbox"]["readableRoots"] =
        json!(normalize_string_slice(&settings.readable_roots));
    data["oppi"]["permissions"]["sandbox"]["writableRoots"] =
        json!(normalize_string_slice(&settings.writable_roots));
    data["oppi"]["permissions"]["sandbox"]["protectedPatterns"] =
        json!(normalize_string_slice(&settings.protected_patterns));
    write_settings_json(path, &data)
}

fn load_oppi_compact_settings(path: &Path) -> OppiCompactSettings {
    let default = OppiCompactSettings::default();
    let Ok(data) = read_settings_json(path) else {
        return default;
    };
    let Some(oppi) = data.get("oppi").and_then(Value::as_object) else {
        return default;
    };
    let idle = oppi.get("idleCompact");
    let smart = oppi.get("smartCompact");
    OppiCompactSettings {
        idle_enabled: idle
            .and_then(|value| value.get("enabled"))
            .and_then(Value::as_bool)
            .unwrap_or(default.idle_enabled),
        idle_minutes: idle
            .and_then(|value| value.get("idleMinutes"))
            .and_then(Value::as_u64)
            .filter(|value| VALID_IDLE_COMPACT_MINUTES.contains(value))
            .unwrap_or(default.idle_minutes),
        idle_threshold_percent: idle
            .and_then(|value| value.get("thresholdPercent"))
            .and_then(Value::as_u64)
            .filter(|value| VALID_IDLE_COMPACT_THRESHOLDS.contains(value))
            .unwrap_or(default.idle_threshold_percent),
        smart_threshold_percent: smart
            .and_then(|value| value.get("thresholdPercent"))
            .and_then(Value::as_u64)
            .filter(|value| VALID_SMART_COMPACT_THRESHOLDS.contains(value))
            .unwrap_or(default.smart_threshold_percent),
    }
}

fn save_oppi_compact_settings(path: &Path, settings: &OppiCompactSettings) -> Result<(), String> {
    let mut data = read_settings_json(path)?;
    if !data.is_object() {
        data = json!({});
    }
    if !data.get("oppi").is_some_and(Value::is_object) {
        data["oppi"] = json!({});
    }
    data["oppi"]["idleCompact"] = json!({
        "enabled": settings.idle_enabled,
        "idleMinutes": settings.idle_minutes,
        "thresholdPercent": settings.idle_threshold_percent,
    });
    data["oppi"]["smartCompact"] = json!({
        "thresholdPercent": settings.smart_threshold_percent,
    });
    write_settings_json(path, &data)
}

fn oppi_compact_settings_json(settings: &OppiCompactSettings) -> Value {
    json!({
        "idleCompact": {
            "enabled": settings.idle_enabled,
            "idleMinutes": settings.idle_minutes,
            "thresholdPercent": settings.idle_threshold_percent,
        },
        "smartCompact": {
            "thresholdPercent": settings.smart_threshold_percent,
        },
    })
}

fn compaction_enabled_label(enabled: bool) -> &'static str {
    if enabled { "enabled" } else { "disabled" }
}

fn compaction_toggle_arg(enabled: bool) -> &'static str {
    if enabled { "off" } else { "on" }
}

fn format_compaction_minutes_label(minutes: u64) -> String {
    format!("{minutes}m")
}

fn format_compaction_threshold_label(percent: u64) -> String {
    format!("{percent}%")
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CompactionTriggerTelemetry {
    context_tokens: Option<i64>,
    context_window: Option<u64>,
    context_percent: Option<u64>,
    idle_state: String,
    smart_state: String,
    active_todos: usize,
}

fn compaction_context_percent(
    context_tokens: Option<i64>,
    context_window: Option<u64>,
) -> Option<u64> {
    let tokens = context_tokens.filter(|tokens| *tokens > 0)? as u128;
    let window = context_window.filter(|window| *window > 0)? as u128;
    Some((((tokens * 100) + (window / 2)) / window).min(999) as u64)
}

fn compaction_context_meets_threshold(
    context_tokens: Option<i64>,
    context_window: Option<u64>,
    threshold_percent: u64,
) -> bool {
    let Some(tokens) = context_tokens.filter(|tokens| *tokens > 0) else {
        return false;
    };
    let Some(window) = context_window.filter(|window| *window > 0) else {
        return false;
    };
    (tokens as u128).saturating_mul(100)
        >= (threshold_percent as u128).saturating_mul(window as u128)
}

fn compaction_trigger_telemetry(
    settings: &OppiCompactSettings,
    context_tokens: Option<i64>,
    context_window: Option<u64>,
    active_todos: usize,
) -> CompactionTriggerTelemetry {
    let context_percent = compaction_context_percent(context_tokens, context_window);
    let idle_state = if !settings.idle_enabled {
        "idle disabled".to_string()
    } else if context_tokens.is_none() {
        "idle waiting for provider usage".to_string()
    } else if context_window.is_none() {
        "idle waiting for context window".to_string()
    } else if compaction_context_meets_threshold(
        context_tokens,
        context_window,
        settings.idle_threshold_percent,
    ) {
        format!(
            "idle ready at {} threshold after {} idle",
            format_compaction_threshold_label(settings.idle_threshold_percent),
            format_compaction_minutes_label(settings.idle_minutes)
        )
    } else {
        format!(
            "idle waiting for {} threshold",
            format_compaction_threshold_label(settings.idle_threshold_percent)
        )
    };
    let smart_state = if active_todos == 0 {
        "smart waiting for active todos".to_string()
    } else if context_tokens.is_none() {
        "smart waiting for provider usage".to_string()
    } else if context_window.is_none() {
        "smart waiting for context window".to_string()
    } else if compaction_context_meets_threshold(
        context_tokens,
        context_window,
        settings.smart_threshold_percent,
    ) {
        format!(
            "smart ready at {} threshold with {} active todos",
            format_compaction_threshold_label(settings.smart_threshold_percent),
            active_todos
        )
    } else {
        format!(
            "smart waiting for {} threshold with {} active todos",
            format_compaction_threshold_label(settings.smart_threshold_percent),
            active_todos
        )
    };
    CompactionTriggerTelemetry {
        context_tokens,
        context_window,
        context_percent,
        idle_state,
        smart_state,
        active_todos,
    }
}

fn format_compaction_context_label(telemetry: &CompactionTriggerTelemetry) -> String {
    match (
        telemetry.context_tokens,
        telemetry.context_window,
        telemetry.context_percent,
    ) {
        (Some(tokens), Some(window), Some(percent)) => format!(
            "context {}/{} ({}%)",
            format_compact_tokens(tokens),
            format_compact_tokens(window.min(i64::MAX as u64) as i64),
            percent
        ),
        (Some(tokens), None, _) => {
            format!("context {} (window unknown)", format_compact_tokens(tokens))
        }
        _ => "context pending".to_string(),
    }
}

fn format_compaction_trigger_telemetry(telemetry: &CompactionTriggerTelemetry) -> String {
    format!(
        "trigger: {}; {}; {}",
        format_compaction_context_label(telemetry),
        telemetry.idle_state,
        telemetry.smart_state
    )
}

fn compaction_trigger_state_kind(state: &str) -> &'static str {
    if state.contains("ready") {
        "ready"
    } else if state.contains("disabled") {
        "disabled"
    } else {
        "waiting"
    }
}

fn compaction_trigger_telemetry_json(telemetry: &CompactionTriggerTelemetry) -> Value {
    json!({
        "contextTokens": telemetry.context_tokens,
        "contextWindow": telemetry.context_window,
        "contextPercent": telemetry.context_percent,
        "activeTodos": telemetry.active_todos,
        "idle": {
            "state": compaction_trigger_state_kind(&telemetry.idle_state),
            "detail": telemetry.idle_state,
        },
        "smart": {
            "state": compaction_trigger_state_kind(&telemetry.smart_state),
            "detail": telemetry.smart_state,
        },
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CompactionOfferKind {
    Smart,
    Idle,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CompactionOffer {
    kind: CompactionOfferKind,
    key: String,
    message: String,
}

fn compaction_offer_key(
    kind: CompactionOfferKind,
    settings: &OppiCompactSettings,
    telemetry: &CompactionTriggerTelemetry,
) -> String {
    let kind = match kind {
        CompactionOfferKind::Smart => "smart",
        CompactionOfferKind::Idle => "idle",
    };
    format!(
        "{kind}:tokens={}:window={}:percent={}:todos={}:idle={}:idle_threshold={}:smart_threshold={}",
        telemetry.context_tokens.unwrap_or(-1),
        telemetry.context_window.unwrap_or(0),
        telemetry.context_percent.unwrap_or(0),
        telemetry.active_todos,
        settings.idle_minutes,
        settings.idle_threshold_percent,
        settings.smart_threshold_percent
    )
}

fn compaction_offer_plan(
    settings: &OppiCompactSettings,
    telemetry: &CompactionTriggerTelemetry,
    idle_elapsed: Option<Duration>,
) -> Option<CompactionOffer> {
    if telemetry.active_todos > 0
        && compaction_context_meets_threshold(
            telemetry.context_tokens,
            telemetry.context_window,
            settings.smart_threshold_percent,
        )
    {
        return Some(CompactionOffer {
            kind: CompactionOfferKind::Smart,
            key: compaction_offer_key(CompactionOfferKind::Smart, settings, telemetry),
            message: format!(
                "smart compact ready: {} with {} active todos. Run `/idle-compact compact` to create a todo-aware handoff now.",
                format_compaction_context_label(telemetry),
                telemetry.active_todos
            ),
        });
    }

    let idle_required = Duration::from_secs(settings.idle_minutes.saturating_mul(60));
    if settings.idle_enabled
        && idle_elapsed.is_some_and(|elapsed| elapsed >= idle_required)
        && compaction_context_meets_threshold(
            telemetry.context_tokens,
            telemetry.context_window,
            settings.idle_threshold_percent,
        )
    {
        return Some(CompactionOffer {
            kind: CompactionOfferKind::Idle,
            key: compaction_offer_key(CompactionOfferKind::Idle, settings, telemetry),
            message: format!(
                "idle compact ready: {} after {} idle. Run `/idle-compact compact` to create a handoff now.",
                format_compaction_context_label(telemetry),
                format_compaction_minutes_label(settings.idle_minutes)
            ),
        });
    }

    None
}

fn format_oppi_compact_settings_status(
    settings: &OppiCompactSettings,
    telemetry: Option<&CompactionTriggerTelemetry>,
) -> String {
    let mut status = format!(
        "compaction: idle {}; idle time {}; idle threshold {}; smart threshold {}",
        compaction_enabled_label(settings.idle_enabled),
        format_compaction_minutes_label(settings.idle_minutes),
        format_compaction_threshold_label(settings.idle_threshold_percent),
        format_compaction_threshold_label(settings.smart_threshold_percent)
    );
    if let Some(telemetry) = telemetry {
        status.push('\n');
        status.push_str(&format_compaction_trigger_telemetry(telemetry));
    }
    status
}

fn load_footer_settings(path: &Path) -> FooterSettings {
    let default = FooterSettings::default();
    let Ok(data) = read_settings_json(path) else {
        return default;
    };
    let Some(footer) = data
        .get("oppi")
        .and_then(|oppi| oppi.get("footer"))
        .and_then(Value::as_object)
    else {
        return default;
    };
    FooterSettings {
        show_path: footer_bool_setting(footer.get("showPath"), default.show_path),
        show_help_bar: footer_bool_setting(footer.get("showHelpBar"), default.show_help_bar),
        usage_display: footer
            .get("usageDisplay")
            .and_then(Value::as_str)
            .and_then(|value| parse_footer_usage_display(value).ok())
            .unwrap_or(default.usage_display),
        show_model: footer_bool_setting(footer.get("showModel"), default.show_model),
        show_permission: footer_bool_setting(footer.get("showPermission"), default.show_permission),
        show_memory: footer_bool_setting(footer.get("showMemory"), default.show_memory),
        show_context: footer_bool_setting(footer.get("showContext"), default.show_context),
    }
}

fn footer_bool_setting(value: Option<&Value>, default: bool) -> bool {
    value.and_then(Value::as_bool).unwrap_or(default)
}

fn save_footer_settings(path: &Path, settings: &FooterSettings) -> Result<(), String> {
    let mut data = read_settings_json(path)?;
    if !data.is_object() {
        data = json!({});
    }
    if !data.get("oppi").is_some_and(Value::is_object) {
        data["oppi"] = json!({});
    }
    if !data["oppi"].get("footer").is_some_and(Value::is_object) {
        data["oppi"]["footer"] = json!({});
    }
    data["oppi"]["footer"]["showPath"] = json!(settings.show_path);
    data["oppi"]["footer"]["showHelpBar"] = json!(settings.show_help_bar);
    data["oppi"]["footer"]["usageDisplay"] = json!(settings.usage_display.as_str());
    data["oppi"]["footer"]["showModel"] = json!(settings.show_model);
    data["oppi"]["footer"]["showPermission"] = json!(settings.show_permission);
    data["oppi"]["footer"]["showMemory"] = json!(settings.show_memory);
    data["oppi"]["footer"]["showContext"] = json!(settings.show_context);
    write_settings_json(path, &data)
}

fn footer_settings_json(settings: &FooterSettings) -> Value {
    json!({
        "showPath": settings.show_path,
        "showHelpBar": settings.show_help_bar,
        "usageDisplay": settings.usage_display.as_str(),
        "showModel": settings.show_model,
        "showPermission": settings.show_permission,
        "showMemory": settings.show_memory,
        "showContext": settings.show_context,
    })
}

fn footer_visibility_label(shown: bool) -> &'static str {
    if shown { "shown" } else { "hidden" }
}

fn footer_toggle_arg(shown: bool) -> &'static str {
    if shown { "off" } else { "on" }
}

fn format_footer_settings_status(settings: &FooterSettings) -> String {
    format!(
        "footer: path {}; help {}; usage {}; model {}; permission {}; memory {}; context {}",
        footer_visibility_label(settings.show_path),
        footer_visibility_label(settings.show_help_bar),
        settings.usage_display.label(),
        footer_visibility_label(settings.show_model),
        footer_visibility_label(settings.show_permission),
        footer_visibility_label(settings.show_memory),
        footer_visibility_label(settings.show_context)
    )
}

fn load_image_settings(path: &Path) -> ImageSettings {
    let default = ImageSettings::default();
    let Ok(data) = read_settings_json(path) else {
        return default;
    };
    let Some(oppi) = data.get("oppi").and_then(Value::as_object) else {
        return default;
    };
    let images = oppi.get("images").and_then(Value::as_object);
    let get = |key: &str| {
        images
            .and_then(|images| images.get(key))
            .or_else(|| oppi.get(key))
    };
    ImageSettings {
        show_images: get("showImages")
            .and_then(Value::as_bool)
            .unwrap_or(default.show_images),
        image_width_cells: get("imageWidthCells")
            .and_then(Value::as_u64)
            .filter(|value| VALID_IMAGE_WIDTH_CELLS.contains(value))
            .unwrap_or(default.image_width_cells),
        auto_resize_images: get("autoResizeImages")
            .and_then(Value::as_bool)
            .unwrap_or(default.auto_resize_images),
        block_images: get("blockImages")
            .and_then(Value::as_bool)
            .unwrap_or(default.block_images),
        preview_protocol: get("previewProtocol")
            .and_then(Value::as_str)
            .and_then(|value| parse_image_preview_protocol_override(value).ok())
            .unwrap_or(default.preview_protocol),
    }
}

fn save_image_settings(path: &Path, settings: &ImageSettings) -> Result<(), String> {
    let mut data = read_settings_json(path)?;
    if !data.is_object() {
        data = json!({});
    }
    if !data.get("oppi").is_some_and(Value::is_object) {
        data["oppi"] = json!({});
    }
    if !data["oppi"].get("images").is_some_and(Value::is_object) {
        data["oppi"]["images"] = json!({});
    }
    data["oppi"]["images"]["showImages"] = json!(settings.show_images);
    data["oppi"]["images"]["imageWidthCells"] = json!(settings.image_width_cells);
    data["oppi"]["images"]["autoResizeImages"] = json!(settings.auto_resize_images);
    data["oppi"]["images"]["blockImages"] = json!(settings.block_images);
    data["oppi"]["images"]["previewProtocol"] = json!(settings.preview_protocol.as_str());
    write_settings_json(path, &data)
}

fn image_settings_json(settings: &ImageSettings) -> Value {
    json!({
        "showImages": settings.show_images,
        "imageWidthCells": settings.image_width_cells,
        "autoResizeImages": settings.auto_resize_images,
        "blockImages": settings.block_images,
        "previewProtocol": settings.preview_protocol.as_str(),
    })
}

fn image_visibility_label(shown: bool) -> &'static str {
    if shown { "shown" } else { "hidden" }
}

fn enabled_label(enabled: bool) -> &'static str {
    if enabled { "enabled" } else { "disabled" }
}

fn image_block_label(blocked: bool) -> &'static str {
    if blocked { "blocked" } else { "allowed" }
}

fn image_show_toggle_arg(shown: bool) -> &'static str {
    if shown { "off" } else { "on" }
}

fn image_enabled_toggle_arg(enabled: bool) -> &'static str {
    if enabled { "off" } else { "on" }
}

fn image_block_toggle_arg(blocked: bool) -> &'static str {
    if blocked { "off" } else { "on" }
}

fn format_image_settings_status(settings: &ImageSettings) -> String {
    format!(
        "images: previews {}; width {}; auto-resize {}; provider image sending {}; preview protocol {}",
        image_visibility_label(settings.show_images),
        settings.image_width_cells,
        enabled_label(settings.auto_resize_images),
        image_block_label(settings.block_images),
        settings.preview_protocol.as_str()
    )
}

fn load_memory_settings(path: &Path) -> MemorySettings {
    let default = MemorySettings::default();
    let Ok(data) = read_settings_json(path) else {
        return default;
    };
    let Some(memory) = data
        .get("oppi")
        .and_then(|oppi| oppi.get("memory"))
        .and_then(Value::as_object)
    else {
        return default;
    };
    let sync = memory.get("sync");
    MemorySettings {
        enabled: memory
            .get("enabled")
            .and_then(Value::as_bool)
            .unwrap_or(default.enabled),
        agent_model: memory
            .get("agentModel")
            .and_then(Value::as_str)
            .filter(|value| VALID_MEMORY_AGENT_MODELS.contains(value))
            .unwrap_or(&default.agent_model)
            .to_string(),
        deep_model: memory
            .get("deepModel")
            .and_then(Value::as_str)
            .filter(|value| VALID_DEEP_MEMORY_MODELS.contains(value))
            .unwrap_or(&default.deep_model)
            .to_string(),
        startup_recall: memory
            .get("startupRecall")
            .and_then(Value::as_bool)
            .unwrap_or(default.startup_recall),
        task_start_recall: memory
            .get("taskStartRecall")
            .and_then(Value::as_bool)
            .unwrap_or(default.task_start_recall),
        turn_summaries: memory
            .get("turnSummaries")
            .and_then(Value::as_bool)
            .unwrap_or(default.turn_summaries),
        idle_consolidation: memory
            .get("idleConsolidation")
            .and_then(Value::as_bool)
            .unwrap_or(default.idle_consolidation),
        dashboard_port: memory
            .get("dashboardPort")
            .and_then(|value| {
                if value.as_str() == Some("auto") {
                    Some(None)
                } else {
                    value
                        .as_u64()
                        .and_then(|port| u16::try_from(port).ok())
                        .filter(|port| *port > 0)
                        .map(Some)
                }
            })
            .unwrap_or(default.dashboard_port),
        hoppi_install_offer: memory
            .get("hoppiInstallOffer")
            .and_then(Value::as_str)
            .filter(|value| VALID_HOPPI_INSTALL_OFFERS.contains(value))
            .unwrap_or(&default.hoppi_install_offer)
            .to_string(),
        sync_enabled: sync
            .and_then(|sync| sync.get("enabled"))
            .and_then(Value::as_bool)
            .unwrap_or(default.sync_enabled),
        sync_repo_path: sync
            .and_then(|sync| sync.get("repoPath"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        sync_repo_url: sync
            .and_then(|sync| sync.get("repoUrl"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        sync_pull_on_startup: sync
            .and_then(|sync| sync.get("pullOnStartup"))
            .and_then(Value::as_bool)
            .unwrap_or(default.sync_pull_on_startup),
        sync_push_on_exit: sync
            .and_then(|sync| sync.get("pushOnExit"))
            .and_then(Value::as_bool)
            .unwrap_or(default.sync_push_on_exit),
        sync_background: sync
            .and_then(|sync| sync.get("backgroundSync"))
            .and_then(Value::as_str)
            .filter(|value| VALID_SYNC_BACKGROUND.contains(value))
            .unwrap_or(&default.sync_background)
            .to_string(),
        sync_encryption: sync
            .and_then(|sync| sync.get("encryption"))
            .and_then(Value::as_str)
            .filter(|value| matches!(*value, "none" | "passphrase"))
            .unwrap_or(&default.sync_encryption)
            .to_string(),
        sync_passphrase_source: sync
            .and_then(|sync| sync.get("passphraseSource"))
            .and_then(Value::as_str)
            .filter(|value| matches!(*value, "env" | "file"))
            .unwrap_or(&default.sync_passphrase_source)
            .to_string(),
        sync_passphrase_env: sync
            .and_then(|sync| sync.get("passphraseEnv"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(&default.sync_passphrase_env)
            .to_string(),
        sync_passphrase_file: sync
            .and_then(|sync| sync.get("passphraseFile"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        sync_conflict_mode: sync
            .and_then(|sync| sync.get("conflictMode"))
            .and_then(Value::as_str)
            .filter(|value| VALID_SYNC_CONFLICT_MODES.contains(value))
            .unwrap_or(&default.sync_conflict_mode)
            .to_string(),
    }
}

fn set_optional_json_string(target: &mut Value, key: &str, value: Option<&str>) {
    if let Some(object) = target.as_object_mut() {
        if let Some(value) = value {
            object.insert(key.to_string(), json!(value));
        } else {
            object.remove(key);
        }
    }
}

fn save_memory_settings(path: &Path, settings: &MemorySettings) -> Result<(), String> {
    let mut data = read_settings_json(path)?;
    if !data.is_object() {
        data = json!({});
    }
    if !data.get("oppi").is_some_and(Value::is_object) {
        data["oppi"] = json!({});
    }
    if !data["oppi"].get("memory").is_some_and(Value::is_object) {
        data["oppi"]["memory"] = json!({});
    }
    if !data["oppi"]["memory"]
        .get("sync")
        .is_some_and(Value::is_object)
    {
        data["oppi"]["memory"]["sync"] = json!({});
    }
    data["oppi"]["memory"]["enabled"] = json!(settings.enabled);
    data["oppi"]["memory"]["agentModel"] = json!(settings.agent_model);
    data["oppi"]["memory"]["deepModel"] = json!(settings.deep_model);
    data["oppi"]["memory"]["startupRecall"] = json!(settings.startup_recall);
    data["oppi"]["memory"]["taskStartRecall"] = json!(settings.task_start_recall);
    data["oppi"]["memory"]["turnSummaries"] = json!(settings.turn_summaries);
    data["oppi"]["memory"]["idleConsolidation"] = json!(settings.idle_consolidation);
    data["oppi"]["memory"]["dashboardPort"] = settings
        .dashboard_port
        .map(|port| json!(port))
        .unwrap_or_else(|| json!("auto"));
    data["oppi"]["memory"]["hoppiInstallOffer"] = json!(settings.hoppi_install_offer);
    let sync = &mut data["oppi"]["memory"]["sync"];
    sync["enabled"] = json!(settings.sync_enabled);
    sync["provider"] = json!("github");
    set_optional_json_string(sync, "repoPath", settings.sync_repo_path.as_deref());
    set_optional_json_string(sync, "repoUrl", settings.sync_repo_url.as_deref());
    sync["pullOnStartup"] = json!(settings.sync_pull_on_startup);
    sync["pushOnExit"] = json!(settings.sync_push_on_exit);
    sync["backgroundSync"] = json!(settings.sync_background);
    sync["encryption"] = json!(settings.sync_encryption);
    sync["passphraseSource"] = json!(settings.sync_passphrase_source);
    sync["passphraseEnv"] = json!(settings.sync_passphrase_env);
    set_optional_json_string(
        sync,
        "passphraseFile",
        settings.sync_passphrase_file.as_deref(),
    );
    sync["conflictMode"] = json!(settings.sync_conflict_mode);
    write_settings_json(path, &data)
}

fn memory_settings_json(settings: &MemorySettings) -> Value {
    json!({
        "enabled": settings.enabled,
        "agentModel": settings.agent_model,
        "deepModel": settings.deep_model,
        "startupRecall": settings.startup_recall,
        "taskStartRecall": settings.task_start_recall,
        "turnSummaries": settings.turn_summaries,
        "idleConsolidation": settings.idle_consolidation,
        "dashboardPort": settings.dashboard_port.map(|port| json!(port)).unwrap_or_else(|| json!("auto")),
        "hoppiInstallOffer": settings.hoppi_install_offer,
        "sync": {
            "enabled": settings.sync_enabled,
            "provider": "github",
            "repoPath": settings.sync_repo_path,
            "repoUrl": settings.sync_repo_url,
            "pullOnStartup": settings.sync_pull_on_startup,
            "pushOnExit": settings.sync_push_on_exit,
            "backgroundSync": settings.sync_background,
            "encryption": settings.sync_encryption,
            "passphraseSource": settings.sync_passphrase_source,
            "passphraseEnv": settings.sync_passphrase_env,
            "passphraseFile": settings.sync_passphrase_file,
            "conflictMode": settings.sync_conflict_mode,
        },
    })
}

fn optional_memory_setting_label(value: Option<&str>) -> String {
    value.unwrap_or("not configured").to_string()
}

fn memory_setting_toggle_arg(enabled: bool) -> &'static str {
    if enabled { "off" } else { "on" }
}

fn hoppi_install_offer_toggle_arg(value: &str) -> &'static str {
    if value == "dismissed" {
        "ask"
    } else {
        "dismissed"
    }
}

fn format_memory_settings_status(settings: &MemorySettings) -> String {
    let repo = optional_memory_setting_label(settings.sync_repo_path.as_deref());
    let remote = optional_memory_setting_label(settings.sync_repo_url.as_deref());
    let passphrase = if settings.sync_encryption == "passphrase" {
        if settings.sync_passphrase_source == "file" {
            optional_memory_setting_label(settings.sync_passphrase_file.as_deref())
        } else {
            settings.sync_passphrase_env.clone()
        }
    } else {
        "not used".to_string()
    };
    format!(
        "memory settings: memory {}; agent model {}; deep dream model {}; startup recall {}; task-start recall {}; turn summaries {}; idle dream {}; dashboard port {}; first-start install offer {}; sync {}; repo {}; remote {}; pull startup {}; push exit {}; background {}; encryption {}/{}; passphrase {}; conflict {}",
        enabled_label(settings.enabled),
        settings.agent_model,
        settings.deep_model,
        enabled_label(settings.startup_recall),
        enabled_label(settings.task_start_recall),
        enabled_label(settings.turn_summaries),
        enabled_label(settings.idle_consolidation),
        settings
            .dashboard_port
            .map(|port| port.to_string())
            .unwrap_or_else(|| "auto".to_string()),
        settings.hoppi_install_offer,
        enabled_label(settings.sync_enabled),
        repo,
        remote,
        enabled_label(settings.sync_pull_on_startup),
        enabled_label(settings.sync_push_on_exit),
        settings.sync_background,
        settings.sync_encryption,
        settings.sync_passphrase_source,
        passphrase,
        settings.sync_conflict_mode
    )
}

struct FooterStatusParts<'a> {
    settings: &'a FooterSettings,
    cwd: &'a str,
    model: &'a str,
    permission: &'a str,
    usage: &'a str,
    weekly: Option<&'a str>,
    context: &'a str,
    memory: &'a str,
    todos: usize,
    queued: usize,
    goal: &'a str,
    variant: &'a str,
    theme: &'a str,
    thread: &'a str,
}

fn format_footer_status_line(parts: FooterStatusParts<'_>) -> String {
    let mut segments = vec!["status: role=executor".to_string()];
    if parts.settings.show_path {
        segments.push(format!("workspace={}", parts.cwd));
    }
    if parts.settings.show_model {
        segments.push(format!("model={}", parts.model));
    }
    if parts.settings.show_permission {
        segments.push(format!("permissions={}", parts.permission));
    }
    let usage = format_usage_footer_line(parts.settings, parts.usage, parts.weekly, parts.context);
    if usage != "off" {
        segments.push(usage);
    }
    if parts.settings.show_memory {
        segments.push(format!("memory={}", parts.memory));
    }
    segments.push(format!("todos={}", parts.todos));
    segments.push(format!("queued={}", parts.queued));
    segments.push(format!("goal={}", parts.goal));
    segments.push("diagnostics=line-mode/raw-deferred".to_string());
    segments.push(format!("variant={}", parts.variant));
    segments.push(format!("theme={}", parts.theme));
    segments.push(format!("thread={}", parts.thread));
    segments.join(" ")
}

fn format_usage_footer_line(
    settings: &FooterSettings,
    usage: &str,
    weekly: Option<&str>,
    context: &str,
) -> String {
    let mut parts = Vec::new();
    match settings.usage_display {
        FooterUsageDisplay::SessionWeekly => {
            parts.push(usage.to_string());
            if let Some(weekly) = weekly {
                parts.push(weekly.to_string());
            }
        }
        FooterUsageDisplay::Session => parts.push(usage.to_string()),
        FooterUsageDisplay::Weekly => {
            if let Some(weekly) = weekly {
                parts.push(weekly.to_string());
            }
        }
        FooterUsageDisplay::Off => {}
    }
    if settings.show_context {
        parts.push(context.to_string());
    }
    if parts.is_empty() {
        "off".to_string()
    } else {
        parts.join(" · ")
    }
}

fn normalize_string_array(value: &Value) -> Vec<String> {
    value
        .as_array()
        .map(|items| {
            normalize_string_slice(
                &items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect::<Vec<_>>(),
            )
        })
        .unwrap_or_default()
}

fn normalize_string_slice(values: &[String]) -> Vec<String> {
    let mut normalized = Vec::new();
    for value in values {
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        let value = value.to_string();
        if !normalized.contains(&value) {
            normalized.push(value);
        }
    }
    normalized
}

fn load_ask_user_settings(path: &Path) -> AskUserSettings {
    let default = AskUserSettings {
        timeout_ms: DEFAULT_ASK_USER_TIMEOUT_MS,
    };
    let Ok(data) = read_settings_json(path) else {
        return default;
    };
    let Some(ask_user) = data
        .get("oppi")
        .and_then(|oppi| oppi.get("askUser"))
        .and_then(Value::as_object)
    else {
        return default;
    };
    let timeout_ms = ask_user
        .get("timeoutMinutes")
        .and_then(Value::as_u64)
        .and_then(ask_user_timeout_minutes_to_ms)
        .or_else(|| {
            ask_user
                .get("timeoutMs")
                .and_then(Value::as_u64)
                .filter(|value| ask_user_timeout_ms_to_minutes(*value).is_some())
        })
        .unwrap_or(default.timeout_ms);
    AskUserSettings { timeout_ms }
}

fn save_ask_user_settings(path: &Path, settings: &AskUserSettings) -> Result<(), String> {
    let timeout_minutes = ask_user_timeout_ms_to_minutes(settings.timeout_ms)
        .ok_or_else(|| "ask_user timeout must be off, 1, 2, 5, or 10 minutes".to_string())?;
    let mut data = read_settings_json(path)?;
    if !data.is_object() {
        data = json!({});
    }
    if !data.get("oppi").is_some_and(Value::is_object) {
        data["oppi"] = json!({});
    }
    if !data["oppi"].get("askUser").is_some_and(Value::is_object) {
        data["oppi"]["askUser"] = json!({});
    }
    data["oppi"]["askUser"]["timeoutMinutes"] = json!(timeout_minutes);
    write_settings_json(path, &data)
}

fn load_follow_up_mode_setting(path: &Path) -> FollowUpMode {
    read_settings_json(path)
        .ok()
        .and_then(|data| {
            data.get("followUpMode")
                .and_then(Value::as_str)
                .and_then(parse_follow_up_mode)
        })
        .unwrap_or_default()
}

fn save_follow_up_mode_setting(path: &Path, mode: FollowUpMode) -> Result<(), String> {
    let mut data = read_settings_json(path)?;
    if !data.is_object() {
        data = json!({});
    }
    data["followUpMode"] = json!(mode.as_str());
    write_settings_json(path, &data)
}

fn load_steering_mode_setting(path: &Path) -> SteeringMode {
    read_settings_json(path)
        .ok()
        .and_then(|data| {
            data.get("steeringMode")
                .and_then(Value::as_str)
                .and_then(parse_steering_mode)
                .or_else(|| {
                    data.get("queueMode")
                        .and_then(Value::as_str)
                        .and_then(parse_steering_mode)
                })
        })
        .unwrap_or_default()
}

fn save_steering_mode_setting(path: &Path, mode: SteeringMode) -> Result<(), String> {
    let mut data = read_settings_json(path)?;
    if !data.is_object() {
        data = json!({});
    }
    data["steeringMode"] = json!(mode.as_str());
    write_settings_json(path, &data)
}

fn load_quiet_startup_setting(path: &Path) -> bool {
    read_settings_json(path)
        .ok()
        .and_then(|data| data.get("quietStartup").and_then(Value::as_bool))
        .unwrap_or(false)
}

fn save_quiet_startup_setting(path: &Path, quiet_startup: bool) -> Result<(), String> {
    let mut data = read_settings_json(path)?;
    if !data.is_object() {
        data = json!({});
    }
    data["quietStartup"] = json!(quiet_startup);
    write_settings_json(path, &data)
}

fn load_show_hardware_cursor_setting(path: &Path) -> bool {
    read_settings_json(path)
        .ok()
        .and_then(|data| data.get("showHardwareCursor").and_then(Value::as_bool))
        .unwrap_or_else(default_show_hardware_cursor)
}

fn save_show_hardware_cursor_setting(path: &Path, enabled: bool) -> Result<(), String> {
    let mut data = read_settings_json(path)?;
    if !data.is_object() {
        data = json!({});
    }
    data["showHardwareCursor"] = json!(enabled);
    write_settings_json(path, &data)
}

fn default_show_hardware_cursor() -> bool {
    env::var("PI_HARDWARE_CURSOR")
        .ok()
        .is_some_and(|value| value.trim() == "1")
}

fn load_editor_padding_x_setting(path: &Path) -> usize {
    read_settings_json(path)
        .ok()
        .and_then(|data| {
            data.get("editorPaddingX")
                .and_then(Value::as_u64)
                .and_then(|value| usize::try_from(value).ok())
                .and_then(normalize_editor_padding_x)
        })
        .unwrap_or(DEFAULT_EDITOR_PADDING_X)
}

fn save_editor_padding_x_setting(path: &Path, padding_x: usize) -> Result<(), String> {
    let padding_x =
        normalize_editor_padding_x(padding_x).ok_or_else(|| editor_usage().to_string())?;
    let mut data = read_settings_json(path)?;
    if !data.is_object() {
        data = json!({});
    }
    data["editorPaddingX"] = json!(padding_x);
    write_settings_json(path, &data)
}

fn load_double_escape_action_setting(path: &Path) -> DoubleEscapeAction {
    read_settings_json(path)
        .ok()
        .and_then(|data| {
            data.get("doubleEscapeAction")
                .and_then(Value::as_str)
                .and_then(|raw| parse_double_escape_action(raw).ok())
        })
        .unwrap_or(DoubleEscapeAction::Tree)
}

fn save_double_escape_action_setting(
    path: &Path,
    action: DoubleEscapeAction,
) -> Result<(), String> {
    let mut data = read_settings_json(path)?;
    if !data.is_object() {
        data = json!({});
    }
    data["doubleEscapeAction"] = json!(action.as_str());
    write_settings_json(path, &data)
}

fn load_tree_filter_mode_setting(path: &Path) -> TreeFilterMode {
    read_settings_json(path)
        .ok()
        .and_then(|data| {
            data.get("treeFilterMode")
                .and_then(Value::as_str)
                .and_then(|raw| parse_tree_filter_mode(raw).ok())
        })
        .unwrap_or(TreeFilterMode::Default)
}

fn save_tree_filter_mode_setting(path: &Path, mode: TreeFilterMode) -> Result<(), String> {
    let mut data = read_settings_json(path)?;
    if !data.is_object() {
        data = json!({});
    }
    data["treeFilterMode"] = json!(mode.as_str());
    write_settings_json(path, &data)
}

fn load_show_terminal_progress_setting(path: &Path) -> bool {
    read_settings_json(path)
        .ok()
        .and_then(|data| data.get("showTerminalProgress").and_then(Value::as_bool))
        .unwrap_or(true)
}

fn save_show_terminal_progress_setting(path: &Path, enabled: bool) -> Result<(), String> {
    let mut data = read_settings_json(path)?;
    if !data.is_object() {
        data = json!({});
    }
    data["showTerminalProgress"] = json!(enabled);
    write_settings_json(path, &data)
}

fn load_terminal_clear_on_shrink_setting(path: &Path) -> bool {
    read_settings_json(path)
        .ok()
        .and_then(|data| data.get("terminalClearOnShrink").and_then(Value::as_bool))
        .unwrap_or(true)
}

fn save_terminal_clear_on_shrink_setting(path: &Path, enabled: bool) -> Result<(), String> {
    let mut data = read_settings_json(path)?;
    if !data.is_object() {
        data = json!({});
    }
    data["terminalClearOnShrink"] = json!(enabled);
    write_settings_json(path, &data)
}

fn load_skill_commands_setting(path: &Path) -> bool {
    read_settings_json(path)
        .ok()
        .and_then(|data| data.get("enableSkillCommands").and_then(Value::as_bool))
        .unwrap_or(true)
}

fn save_skill_commands_setting(path: &Path, enabled: bool) -> Result<(), String> {
    let mut data = read_settings_json(path)?;
    if !data.is_object() {
        data = json!({});
    }
    data["enableSkillCommands"] = json!(enabled);
    write_settings_json(path, &data)
}

fn load_autocomplete_max_visible_setting(path: &Path) -> usize {
    read_settings_json(path)
        .ok()
        .and_then(|data| {
            data.get("autocompleteMaxVisible")
                .and_then(Value::as_u64)
                .and_then(|value| usize::try_from(value).ok())
                .and_then(normalize_autocomplete_max_visible)
        })
        .unwrap_or(DEFAULT_AUTOCOMPLETE_MAX_VISIBLE)
}

fn save_autocomplete_max_visible_setting(path: &Path, max_visible: usize) -> Result<(), String> {
    let max_visible = normalize_autocomplete_max_visible(max_visible)
        .ok_or_else(|| autocomplete_usage().to_string())?;
    let mut data = read_settings_json(path)?;
    if !data.is_object() {
        data = json!({});
    }
    data["autocompleteMaxVisible"] = json!(max_visible);
    write_settings_json(path, &data)
}

fn load_reasoning_effort_setting(path: &Path) -> Option<ThinkingLevel> {
    read_settings_json(path).ok().and_then(|data| {
        data.get("defaultThinkingLevel")
            .and_then(Value::as_str)
            .and_then(normalize_thinking_level)
            .or_else(|| {
                data.get("oppi")
                    .and_then(|oppi| oppi.get("reasoningEffort"))
                    .and_then(Value::as_str)
                    .and_then(normalize_thinking_level)
            })
    })
}

fn save_reasoning_effort_setting(path: &Path, level: ThinkingLevel) -> Result<(), String> {
    let mut data = read_settings_json(path)?;
    if !data.is_object() {
        data = json!({});
    }
    if !data.get("oppi").is_some_and(Value::is_object) {
        data["oppi"] = json!({});
    }
    data["defaultThinkingLevel"] = json!(level.as_str());
    data["oppi"]["reasoningEffort"] = json!(level.as_str());
    write_settings_json(path, &data)
}

fn load_prompt_variant_setting(path: &Path) -> String {
    read_settings_json(path)
        .ok()
        .and_then(|data| {
            data.get("oppi")
                .and_then(|oppi| oppi.get("promptVariant"))
                .and_then(Value::as_str)
                .and_then(normalize_prompt_variant)
        })
        .unwrap_or_else(|| "off".to_string())
}

fn save_prompt_variant_setting(path: &Path, variant: &str) -> Result<(), String> {
    let normalized = normalize_prompt_variant(variant)
        .ok_or_else(|| "usage: /prompt-variant [off|a|b|caveman]".to_string())?;
    let mut data = read_settings_json(path)?;
    if !data.is_object() {
        data = json!({});
    }
    if !data.get("oppi").is_some_and(Value::is_object) {
        data["oppi"] = json!({});
    }
    data["oppi"]["promptVariant"] = json!(normalized);
    write_settings_json(path, &data)
}

fn load_enabled_model_scope(path: &Path) -> Vec<String> {
    let Ok(data) = read_settings_json(path) else {
        return Vec::new();
    };
    data.get("enabledModels")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn save_enabled_model_scope(path: &Path, models: Option<&[String]>) -> Result<(), String> {
    let mut data = read_settings_json(path)?;
    if !data.is_object() {
        data = json!({});
    }
    if let Some(models) = models.filter(|models| !models.is_empty()) {
        data["enabledModels"] = json!(models);
    } else if let Some(object) = data.as_object_mut() {
        object.remove("enabledModels");
    }
    write_settings_json(path, &data)
}

fn write_settings_json(path: &Path, data: &Value) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("create settings dir {}: {error}", parent.display()))?;
    }
    let rendered = serde_json::to_string_pretty(data)
        .map_err(|error| format!("serialize settings {}: {error}", path.display()))?;
    fs::write(path, format!("{rendered}\n"))
        .map_err(|error| format!("write settings {}: {error}", path.display()))
}

fn default_server_path() -> PathBuf {
    if let Ok(path) = env::var("OPPI_SERVER_BIN") {
        let path = path.trim();
        if !path.is_empty() {
            return PathBuf::from(path);
        }
    }
    if let Ok(exe) = env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let candidate = dir.join(format!("oppi-server{}", env::consts::EXE_SUFFIX));
        if candidate.exists() {
            return candidate;
        }
    }
    PathBuf::from(format!("oppi-server{}", env::consts::EXE_SUFFIX))
}
