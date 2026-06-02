fn image_api_key_env_candidates(explicit: Option<&str>) -> Result<Vec<String>, RuntimeError> {
    if let Some(name) = explicit {
        if provider_api_key_env_allowed(name) {
            return Ok(vec![name.to_string()]);
        }
        return Err(RuntimeError::new(
            "image_api_key_env_not_allowed",
            RuntimeErrorCategory::InvalidRequest,
            "image_gen apiKeyEnv must be an env-reference such as OPPI_IMAGE_API_KEY, OPPI_OPENAI_API_KEY, OPENAI_API_KEY, or OPPI_*_API_KEY",
        ));
    }
    Ok(vec![
        "OPPI_IMAGE_API_KEY".to_string(),
        "OPPI_OPENAI_API_KEY".to_string(),
        "OPENAI_API_KEY".to_string(),
    ])
}

fn image_api_base_url(call: &ToolCall) -> String {
    let base = call
        .arguments
        .get("baseUrl")
        .or_else(|| call.arguments.get("base_url"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| std::env::var("OPPI_IMAGE_API_BASE_URL").ok())
        .or_else(|| std::env::var("OPPI_OPENAI_BASE_URL").ok())
        .unwrap_or_else(|| "https://api.openai.com/v1".to_string());
    base.trim_end_matches('/').to_string()
}

fn image_api_endpoint(call: &ToolCall, path: &str) -> String {
    let base = image_api_base_url(call);
    let base = if base.ends_with("/images/generations") || base.ends_with("/images/edits") {
        base.rsplit_once("/images/")
            .map(|(api_base, _)| api_base.to_string())
            .unwrap_or(base)
    } else {
        base
    };
    format!("{base}/{path}")
}

fn image_generation_endpoint(call: &ToolCall) -> String {
    image_api_endpoint(call, "images/generations")
}

fn image_generation_request_body(
    call: &ToolCall,
    model: &str,
    prompt: &str,
    output_format: &str,
    background: Option<&str>,
    n: usize,
) -> Value {
    let mut body = json!({
        "model": model,
        "prompt": prompt,
        "n": n,
        "output_format": output_format,
    });
    for (arg, wire) in [
        ("size", "size"),
        ("quality", "quality"),
        ("moderation", "moderation"),
    ] {
        if let Some(value) = call.arguments.get(arg).and_then(Value::as_str)
            && !value.trim().is_empty()
        {
            body[wire] = json!(value.trim());
        }
    }
    if let Some(background) = background {
        body["background"] = json!(background);
    }
    if let Some(value) = call
        .arguments
        .get("outputCompression")
        .or_else(|| call.arguments.get("output_compression"))
        .and_then(Value::as_u64)
    {
        body["output_compression"] = json!(value);
    }
    body
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ImageGenBackend {
    CodexNative,
    ImageApi,
}

fn image_validation_error(message: impl Into<String>) -> (ToolResultStatus, String) {
    (
        ToolResultStatus::Error,
        format!("image_gen {}", message.into()),
    )
}

fn image_backend_arg(call: &ToolCall) -> Result<&str, (ToolResultStatus, String)> {
    let value = call
        .arguments
        .get("backend")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("auto");
    match value {
        "auto" | "codex_native" | "image_api" => Ok(value),
        _ => Err(image_validation_error(
            "backend must be auto, codex_native, or image_api",
        )),
    }
}

fn image_has_low_level_api_controls(call: &ToolCall, n: usize, has_mask: bool) -> bool {
    n != 1
        || has_mask
        || [
            "model",
            "size",
            "quality",
            "background",
            "outputCompression",
            "output_compression",
            "inputFidelity",
            "input_fidelity",
            "moderation",
        ]
        .iter()
        .any(|key| call.arguments.get(*key).is_some())
}

fn selected_image_backend(
    call: &ToolCall,
    parent_provider_config: Option<&DirectModelProviderConfig>,
    n: usize,
    has_mask: bool,
) -> Result<ImageGenBackend, (ToolResultStatus, String)> {
    match image_backend_arg(call)? {
        "codex_native" => Ok(ImageGenBackend::CodexNative),
        "image_api" => Ok(ImageGenBackend::ImageApi),
        _ if parent_provider_config
            .is_some_and(|config| config.kind == DirectModelProviderKind::OpenAiCodex)
            && !image_has_low_level_api_controls(call, n, has_mask) =>
        {
            Ok(ImageGenBackend::CodexNative)
        }
        _ => Ok(ImageGenBackend::ImageApi),
    }
}

fn image_output_format(call: &ToolCall) -> Result<String, (ToolResultStatus, String)> {
    let value = call
        .arguments
        .get("outputFormat")
        .or_else(|| call.arguments.get("output_format"))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("png")
        .trim()
        .trim_start_matches('.')
        .to_ascii_lowercase();
    match value.as_str() {
        "jpg" => Ok("jpeg".to_string()),
        "png" | "jpeg" | "webp" => Ok(value),
        _ => Err(image_validation_error(
            "outputFormat must be png, jpeg, jpg, or webp",
        )),
    }
}

fn image_output_compression(call: &ToolCall) -> Result<Option<u64>, (ToolResultStatus, String)> {
    let Some(value) = call
        .arguments
        .get("outputCompression")
        .or_else(|| call.arguments.get("output_compression"))
    else {
        return Ok(None);
    };
    if let Some(value) = value.as_i64()
        && (0..=100).contains(&value)
    {
        return Ok(Some(value as u64));
    }
    if let Some(value) = value.as_u64()
        && value <= 100
    {
        return Ok(Some(value));
    }
    Err(image_validation_error(
        "outputCompression must be between 0 and 100",
    ))
}

fn parse_image_api_size(size: &str) -> Option<(u64, u64)> {
    let (width, height) = size.split_once('x')?;
    let width = width.parse::<u64>().ok()?;
    let height = height.parse::<u64>().ok()?;
    if width == 0 || height == 0 {
        return None;
    }
    Some((width, height))
}

fn image_validate_enum(
    value: Option<&str>,
    allowed: &[&str],
    message: &str,
) -> Result<(), (ToolResultStatus, String)> {
    if let Some(value) = value
        && !allowed.contains(&value)
    {
        return Err(image_validation_error(message));
    }
    Ok(())
}

fn validate_image_api_options(
    call: &ToolCall,
    model: &str,
    output_format: &str,
    background: Option<&str>,
    _n: usize,
) -> Result<(), (ToolResultStatus, String)> {
    if !model.starts_with("gpt-image-") && model != "chatgpt-image-latest" {
        return Err(image_validation_error(
            "model must be a GPT Image model such as gpt-image-2, gpt-image-1.5, gpt-image-1, or gpt-image-1-mini",
        ));
    }
    image_validate_enum(
        image_optional_string_arg(call, &["quality"]),
        &["low", "medium", "high", "auto"],
        "quality must be low, medium, high, or auto",
    )?;
    image_validate_enum(
        background,
        &["transparent", "opaque", "auto"],
        "background must be transparent, opaque, or auto",
    )?;
    image_validate_enum(
        image_optional_string_arg(call, &["moderation"]),
        &["low", "auto"],
        "moderation must be low or auto",
    )?;
    let input_fidelity = image_optional_string_arg(call, &["inputFidelity", "input_fidelity"]);
    image_validate_enum(
        input_fidelity,
        &["low", "high"],
        "inputFidelity must be low or high",
    )?;
    image_output_compression(call)?;

    if background == Some("transparent") && !matches!(output_format, "png" | "webp") {
        return Err(image_validation_error(
            "transparent background requires outputFormat png or webp",
        ));
    }

    let size = image_optional_string_arg(call, &["size"]).unwrap_or("auto");
    if model == "gpt-image-2" {
        if input_fidelity.is_some() {
            return Err(image_validation_error(
                "inputFidelity is not supported with gpt-image-2 because image inputs always use high fidelity",
            ));
        }
        if size != "auto" {
            let (width, height) = parse_image_api_size(size).ok_or_else(|| {
                image_validation_error(
                    "gpt-image-2 size must be auto or WIDTHxHEIGHT, for example 1536x1024",
                )
            })?;
            let max_edge = width.max(height);
            let min_edge = width.min(height);
            let pixels = width.saturating_mul(height);
            if max_edge > GPT_IMAGE_2_MAX_EDGE {
                return Err(image_validation_error(
                    "gpt-image-2 size max edge must be <= 3840px",
                ));
            }
            if width % 16 != 0 || height % 16 != 0 {
                return Err(image_validation_error(
                    "gpt-image-2 size width and height must be multiples of 16px",
                ));
            }
            if max_edge as f64 / min_edge as f64 > GPT_IMAGE_2_MAX_RATIO {
                return Err(image_validation_error(
                    "gpt-image-2 size ratio must not exceed 3:1",
                ));
            }
            if !(GPT_IMAGE_2_MIN_PIXELS..=GPT_IMAGE_2_MAX_PIXELS).contains(&pixels) {
                return Err(image_validation_error(
                    "gpt-image-2 size total pixels must be between 655,360 and 8,294,400",
                ));
            }
        }
    } else if !matches!(size, "1024x1024" | "1536x1024" | "1024x1536" | "auto") {
        return Err(image_validation_error(
            "size must be one of 1024x1024, 1536x1024, 1024x1536, or auto for this GPT Image model",
        ));
    }

    Ok(())
}

fn validate_codex_native_image_options(
    n: usize,
    mask: Option<&str>,
) -> Result<(), (ToolResultStatus, String)> {
    if mask.is_some() {
        return Err(image_validation_error(
            "mask requires backend=image_api; Codex native image_generation does not expose mask controls",
        ));
    }
    if n != 1 {
        return Err(image_validation_error(
            "n>1 requires backend=image_api; Codex native image_generation returns one image per call",
        ));
    }
    Ok(())
}

fn openai_codex_responses_endpoint(config: Option<&DirectModelProviderConfig>) -> String {
    let base = config
        .and_then(|config| config.base_url.clone())
        .or_else(|| std::env::var("OPPI_OPENAI_CODEX_BASE_URL").ok())
        .or_else(|| std::env::var("OPPI_CODEX_BASE_URL").ok())
        .unwrap_or_else(|| OPENAI_CODEX_DEFAULT_BASE_URL.to_string());
    let base = base.trim_end_matches('/');
    if base.ends_with("/codex/responses") {
        base.to_string()
    } else if base.ends_with("/codex") {
        format!("{base}/responses")
    } else {
        format!("{base}/codex/responses")
    }
}

fn codex_native_image_model(config: Option<&DirectModelProviderConfig>) -> String {
    config
        .filter(|config| config.kind == DirectModelProviderKind::OpenAiCodex)
        .map(|config| config.model.clone())
        .filter(|model| !model.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_CODEX_IMAGE_MODEL.to_string())
}

fn codex_native_image_request_body(
    model: &str,
    prompt: &str,
    output_format: &str,
    source_files: &[(String, PathBuf, Vec<u8>)],
) -> Value {
    let image_prompt = if source_files.is_empty() {
        format!("Create an image. Prompt: {prompt}")
    } else {
        format!(
            "Create an edited/generated image from the attached source image(s). Prompt: {prompt}"
        )
    };
    let mut content = vec![json!({ "type": "input_text", "text": image_prompt })];
    for (_, path, bytes) in source_files {
        content.push(json!({
            "type": "input_image",
            "detail": "high",
            "image_url": format!(
                "data:{};base64,{}",
                image_mime_type_for_path(path),
                STANDARD.encode(bytes)
            ),
        }));
    }
    json!({
        "model": model,
        "store": false,
        "stream": true,
        "instructions": CODEX_NATIVE_IMAGE_INSTRUCTIONS,
        "input": [{ "role": "user", "content": content }],
        "tools": [{ "type": "image_generation", "output_format": output_format }],
        "tool_choice": "auto",
        "parallel_tool_calls": false,
    })
}

fn parse_codex_image_generation_stream<R: BufRead>(
    reader: R,
) -> Result<Vec<Value>, (ToolResultStatus, String)> {
    let mut images = Vec::new();
    for line in reader.lines() {
        let line = line.map_err(|error| {
            (
                ToolResultStatus::Error,
                format!("image_gen Codex native image_generation stream read failed: {error}"),
            )
        })?;
        let Some(data) = line.trim().strip_prefix("data:") else {
            continue;
        };
        let data = data.trim();
        if data.is_empty() || data == "[DONE]" {
            continue;
        }
        let event = serde_json::from_str::<Value>(data).map_err(|error| {
            (
                ToolResultStatus::Error,
                format!(
                    "image_gen Codex native image_generation returned invalid SSE JSON: {error}"
                ),
            )
        })?;
        match event.get("type").and_then(Value::as_str) {
            Some("error") => {
                let message = event
                    .get("message")
                    .or_else(|| event.get("code"))
                    .and_then(Value::as_str)
                    .unwrap_or("unknown error");
                return Err((
                    ToolResultStatus::Error,
                    format!("image_gen Codex native image_generation failed: {message}"),
                ));
            }
            Some("response.failed") => {
                let message = event
                    .get("response")
                    .and_then(|response| response.get("error"))
                    .and_then(|error| error.get("message"))
                    .and_then(Value::as_str)
                    .unwrap_or("response failed");
                return Err((
                    ToolResultStatus::Error,
                    format!("image_gen Codex native image_generation failed: {message}"),
                ));
            }
            Some("response.output_item.done") => {
                let Some(item) = event.get("item") else {
                    continue;
                };
                if item.get("type").and_then(Value::as_str) != Some("image_generation_call") {
                    continue;
                }
                if let Some(result) = item
                    .get("result")
                    .and_then(Value::as_str)
                    .filter(|value| !value.is_empty())
                {
                    let mut image = json!({ "b64_json": result });
                    if let Some(revised) = item.get("revised_prompt").and_then(Value::as_str) {
                        image["revised_prompt"] = json!(revised);
                    }
                    images.push(image);
                } else if item
                    .get("status")
                    .and_then(Value::as_str)
                    .is_some_and(|status| status != "completed")
                {
                    let status = item
                        .get("status")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown");
                    return Err((
                        ToolResultStatus::Error,
                        format!("image_gen Codex native image_generation_call status: {status}"),
                    ));
                }
            }
            _ => {}
        }
    }
    if images.is_empty() {
        return Err((
            ToolResultStatus::Error,
            "image_gen Codex native image_generation completed without an image result".to_string(),
        ));
    }
    Ok(images)
}

fn image_source_arguments(call: &ToolCall) -> Vec<String> {
    for key in ["images", "sourceImages", "source_images"] {
        if let Some(items) = call.arguments.get(key).and_then(Value::as_array) {
            return items
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect();
        }
        if let Some(item) = call
            .arguments
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return vec![item.to_string()];
        }
    }
    Vec::new()
}

fn image_mask_argument(call: &ToolCall) -> Option<String> {
    call.arguments
        .get("mask")
        .or_else(|| call.arguments.get("maskPath"))
        .or_else(|| call.arguments.get("mask_path"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn image_input_path(cwd: &str, raw: &str) -> Result<PathBuf, (ToolResultStatus, String)> {
    let raw = raw.trim().trim_start_matches('@');
    resolve_project_path(cwd, raw)
}

fn image_mime_type_for_path(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        _ => "image/png",
    }
}

fn multipart_push_text(body: &mut Vec<u8>, boundary: &str, name: &str, value: &str) {
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(
        format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes(),
    );
    body.extend_from_slice(value.as_bytes());
    body.extend_from_slice(b"\r\n");
}

fn multipart_push_file(
    body: &mut Vec<u8>,
    boundary: &str,
    name: &str,
    filename: &str,
    mime_type: &str,
    bytes: &[u8],
) {
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(
        format!("Content-Disposition: form-data; name=\"{name}\"; filename=\"{filename}\"\r\n")
            .as_bytes(),
    );
    body.extend_from_slice(format!("Content-Type: {mime_type}\r\n\r\n").as_bytes());
    body.extend_from_slice(bytes);
    body.extend_from_slice(b"\r\n");
}

fn image_optional_string_arg<'a>(call: &'a ToolCall, keys: &[&str]) -> Option<&'a str> {
    keys.iter().find_map(|key| {
        call.arguments
            .get(*key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
    })
}

fn image_optional_u64_arg(call: &ToolCall, keys: &[&str]) -> Option<u64> {
    keys.iter()
        .find_map(|key| call.arguments.get(*key).and_then(Value::as_u64))
}

fn image_generation_count(call: &ToolCall) -> Result<usize, (ToolResultStatus, String)> {
    let Some(value) = call.arguments.get("n") else {
        return Ok(1);
    };
    let Some(n) = value.as_u64() else {
        return Err((
            ToolResultStatus::Error,
            "image_gen n must be an integer between 1 and 10".to_string(),
        ));
    };
    if !(1..=10).contains(&n) {
        return Err((
            ToolResultStatus::Error,
            "image_gen n must be an integer between 1 and 10".to_string(),
        ));
    }
    Ok(n as usize)
}

fn default_image_output_path(prompt: &str, output_format: &str) -> String {
    let mut slug = prompt
        .chars()
        .filter_map(|ch| {
            if ch.is_ascii_alphanumeric() {
                Some(ch.to_ascii_lowercase())
            } else if ch.is_whitespace() || matches!(ch, '-' | '_' | '.') {
                Some('-')
            } else {
                None
            }
        })
        .collect::<String>();
    while slug.contains("--") {
        slug = slug.replace("--", "-");
    }
    slug = slug.trim_matches('-').chars().take(48).collect();
    if slug.is_empty() {
        slug = "image".to_string();
    }
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("output/imagegen/{stamp}-{slug}.{output_format}")
}

fn image_output_path_with_index(path: &Path, output_format: &str, index: usize) -> PathBuf {
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("image");
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or(output_format);
    let file_name = format!("{stem}-{index}.{extension}");
    path.parent()
        .map(|parent| parent.join(&file_name))
        .unwrap_or_else(|| PathBuf::from(file_name))
}

fn image_output_paths(
    cwd: &str,
    output_format: &str,
    output_path: &str,
    n: usize,
) -> Result<Vec<PathBuf>, (ToolResultStatus, String)> {
    let path = resolve_project_path(cwd, output_path)?;
    if n == 1 {
        return Ok(vec![path]);
    }
    Ok((1..=n)
        .map(|index| image_output_path_with_index(&path, output_format, index))
        .collect())
}

fn mime_type_for_image_format(format: &str) -> &'static str {
    match format {
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        _ => "image/png",
    }
}

fn image_dimensions(bytes: &[u8], format: &str) -> (Option<u32>, Option<u32>) {
    if matches!(format, "png" | "") && bytes.len() >= 24 && bytes.starts_with(b"\x89PNG\r\n\x1a\n")
    {
        let width = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
        let height = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
        return (Some(width), Some(height));
    }
    (None, None)
}

fn artifact_metadata_from_tool_result(
    call: &ToolCall,
    result: &ToolResult,
) -> Option<ArtifactMetadata> {
    if result.status != ToolResultStatus::Ok {
        return None;
    }
    let output = result.output.as_deref()?;
    let value: Value = serde_json::from_str(output).ok()?;
    let output_path = value
        .get("outputPath")
        .or_else(|| value.get("path"))
        .or_else(|| value.get("file"))
        .and_then(Value::as_str)?
        .to_string();
    let dimensions = value.get("dimensions");
    let width = value
        .get("width")
        .and_then(Value::as_u64)
        .or_else(|| {
            dimensions
                .and_then(|value| value.get("width"))
                .and_then(Value::as_u64)
        })
        .and_then(|value| u32::try_from(value).ok());
    let height = value
        .get("height")
        .and_then(Value::as_u64)
        .or_else(|| {
            dimensions
                .and_then(|value| value.get("height"))
                .and_then(Value::as_u64)
        })
        .and_then(|value| u32::try_from(value).ok());
    let source_images = call
        .arguments
        .get("images")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mask = call
        .arguments
        .get("mask")
        .and_then(Value::as_str)
        .map(str::to_string);
    let mut diagnostics = Vec::new();
    if value.get("endpoint").is_some() || value.get("durationMs").is_some() {
        let mut metadata = BTreeMap::new();
        if let Some(endpoint) = value.get("endpoint").and_then(Value::as_str) {
            metadata.insert("endpoint".to_string(), redacted_endpoint(endpoint));
        }
        if let Some(duration) = value.get("durationMs").and_then(Value::as_u64) {
            metadata.insert("durationMs".to_string(), duration.to_string());
        }
        diagnostics.push(Diagnostic {
            level: DiagnosticLevel::Info,
            message: "image artifact generated".to_string(),
            metadata,
        });
    }
    Some(ArtifactMetadata {
        id: format!("artifact-{}", call.id),
        tool_call_id: call.id.clone(),
        output_path,
        mime_type: value
            .get("mimeType")
            .and_then(Value::as_str)
            .map(str::to_string),
        width,
        height,
        source_images,
        mask,
        backend: value
            .get("backend")
            .and_then(Value::as_str)
            .map(str::to_string),
        model: value
            .get("model")
            .and_then(Value::as_str)
            .map(str::to_string),
        bytes: value.get("bytes").and_then(Value::as_u64),
        diagnostics,
    })
}

fn decode_base64(raw: &str) -> Result<Vec<u8>, String> {
    let mut buffer = 0u32;
    let mut bits = 0u8;
    let mut output = Vec::new();
    for ch in raw.chars().filter(|ch| !ch.is_whitespace()) {
        let value = match ch {
            'A'..='Z' => ch as u8 - b'A',
            'a'..='z' => ch as u8 - b'a' + 26,
            '0'..='9' => ch as u8 - b'0' + 52,
            '+' => 62,
            '/' => 63,
            '=' => break,
            _ => return Err("image_gen backend returned invalid base64 image data".to_string()),
        } as u32;
        buffer = (buffer << 6) | value;
        bits += 6;
        while bits >= 8 {
            bits -= 8;
            output.push(((buffer >> bits) & 0xff) as u8);
        }
    }
    if output.is_empty() {
        return Err("image_gen backend returned empty image data".to_string());
    }
    Ok(output)
}

fn download_image_url(url: &str) -> Result<Vec<u8>, (ToolResultStatus, String)> {
    let response = match ureq::get(url).call() {
        Ok(response) => response,
        Err(ureq::Error::Status(status, response)) => {
            let detail = response.into_string().unwrap_or_default();
            return Err((
                ToolResultStatus::Error,
                format!(
                    "image_gen generated image URL download failed with status {status}{}",
                    if detail.trim().is_empty() {
                        String::new()
                    } else {
                        format!(": {}", compact_provider_error(&detail))
                    }
                ),
            ));
        }
        Err(error) => {
            return Err((
                ToolResultStatus::Error,
                format!("image_gen generated image URL download failed: {error}"),
            ));
        }
    };
    let mut bytes = Vec::new();
    response
        .into_reader()
        .read_to_end(&mut bytes)
        .map_err(file_io_error)?;
    if bytes.is_empty() {
        return Err((
            ToolResultStatus::Error,
            "image_gen generated image URL returned an empty body".to_string(),
        ));
    }
    Ok(bytes)
}

fn tool_delay_ms(call: &ToolCall) -> Option<u64> {
    call.arguments
        .get("delayMs")
        .or_else(|| call.arguments.get("delay_ms"))
        .and_then(serde_json::Value::as_u64)
        .map(|delay| delay.min(5_000))
        .filter(|delay| *delay > 0)
}

fn file_tool_kind(definition: &ToolDefinition, call: &ToolCall) -> Option<FileToolKind> {
    match call.name.as_str() {
        "read" | "read_file" | "oppi_review_read" => return Some(FileToolKind::Read),
        "ls" | "list" | "list_files" | "oppi_review_ls" => return Some(FileToolKind::List),
        "search" | "search_files" | "grep" | "oppi_review_grep" => {
            return Some(FileToolKind::Search);
        }
        "write" | "write_file" => return Some(FileToolKind::Write),
        "edit" | "edit_file" => return Some(FileToolKind::Edit),
        _ => {}
    }
    if definition
        .capabilities
        .iter()
        .any(|capability| capability == "filesystem")
    {
        if definition
            .capabilities
            .iter()
            .any(|capability| capability == "search")
        {
            return Some(FileToolKind::Search);
        }
        if definition
            .capabilities
            .iter()
            .any(|capability| capability == "write")
        {
            return Some(FileToolKind::Write);
        }
        if definition
            .capabilities
            .iter()
            .any(|capability| capability == "edit")
        {
            return Some(FileToolKind::Edit);
        }
        if definition
            .capabilities
            .iter()
            .any(|capability| capability == "list")
        {
            return Some(FileToolKind::List);
        }
        if definition
            .capabilities
            .iter()
            .any(|capability| capability == "read")
        {
            return Some(FileToolKind::Read);
        }
    }
    None
}

fn required_string_arg<'a>(
    call: &'a ToolCall,
    key: &str,
) -> Result<&'a str, (ToolResultStatus, String)> {
    call.arguments
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            (
                ToolResultStatus::Error,
                format!("file tool requires a string {key} argument"),
            )
        })
}

fn usize_arg(call: &ToolCall, key: &str) -> Option<usize> {
    call.arguments
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
}

fn todo_status(value: &Value) -> Result<TodoStatus, (ToolResultStatus, String)> {
    match value.as_str().unwrap_or_default() {
        "pending" => Ok(TodoStatus::Pending),
        "in_progress" | "in-progress" | "active" => Ok(TodoStatus::InProgress),
        "completed" | "done" => Ok(TodoStatus::Completed),
        "blocked" => Ok(TodoStatus::Blocked),
        "cancelled" | "canceled" => Ok(TodoStatus::Cancelled),
        other => Err((
            ToolResultStatus::Error,
            format!(
                "todo_write invalid status '{other}'; expected pending, in_progress, completed, blocked, or cancelled"
            ),
        )),
    }
}

fn todo_priority(
    value: Option<&Value>,
) -> Result<Option<TodoPriority>, (ToolResultStatus, String)> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    match value.as_str().unwrap_or_default() {
        "low" => Ok(Some(TodoPriority::Low)),
        "medium" => Ok(Some(TodoPriority::Medium)),
        "high" => Ok(Some(TodoPriority::High)),
        other => Err((
            ToolResultStatus::Error,
            format!("todo_write invalid priority '{other}'; expected low, medium, or high"),
        )),
    }
}

fn optional_todo_string(value: Option<&Value>, max_chars: usize) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| truncate_chars(value, max_chars))
}

fn todo_summary_for(todos: &[TodoItem]) -> String {
    let completed = todos
        .iter()
        .filter(|todo| todo.status == TodoStatus::Completed)
        .count();
    let active = todos
        .iter()
        .filter(|todo| todo.status == TodoStatus::InProgress)
        .count();
    let blocked = todos
        .iter()
        .filter(|todo| todo.status == TodoStatus::Blocked)
        .count();
    let mut parts = vec![
        format!("{} total", todos.len()),
        format!("{completed} done"),
    ];
    if active > 0 {
        parts.push(format!("{active} active"));
    }
    if blocked > 0 {
        parts.push(format!("{blocked} blocked"));
    }
    parts.join(", ")
}

fn clean_suggestion_text(raw: &str, max_chars: usize) -> Option<String> {
    let mut text = raw
        .trim()
        .trim_matches(|ch: char| "[](){}\"'`".contains(ch) || ch.is_whitespace())
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if text.is_empty() {
        return None;
    }
    if text.chars().count() > max_chars {
        text = truncate_chars(&text, max_chars.saturating_sub(1));
    }
    Some(text)
}

fn normalize_suggest_next(
    call: &ToolCall,
) -> Result<Option<SuggestedNextMessage>, (ToolResultStatus, String)> {
    let Some(raw_message) = call.arguments.get("message").and_then(Value::as_str) else {
        return Err((
            ToolResultStatus::Error,
            "suggest_next_message requires a message string".to_string(),
        ));
    };
    let Some(message) = clean_suggestion_text(raw_message, 180) else {
        return Ok(None);
    };
    let confidence = call
        .arguments
        .get("confidence")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    if !confidence.is_finite() {
        return Ok(None);
    }
    if confidence < 0.7 {
        return Ok(None);
    }
    let reason = call
        .arguments
        .get("reason")
        .and_then(Value::as_str)
        .and_then(|value| clean_suggestion_text(value, 240));
    Ok(Some(SuggestedNextMessage {
        message,
        confidence: confidence.min(1.0).max(0.0) as f32,
        reason,
    }))
}

fn normalize_ask_user_request(
    request_id: QuestionId,
    call: &ToolCall,
    settings: &AskUserSettings,
) -> Result<AskUserRequest, (ToolResultStatus, String)> {
    let title = call
        .arguments
        .get("title")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| truncate_chars(value, 160));
    let questions = call
        .arguments
        .get("questions")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            (
                ToolResultStatus::Error,
                "ask_user requires a non-empty questions array".to_string(),
            )
        })?;
    if questions.is_empty() {
        return Err((
            ToolResultStatus::Error,
            "ask_user requires at least one question".to_string(),
        ));
    }
    if questions.len() > 12 {
        return Err((
            ToolResultStatus::Error,
            "ask_user accepts at most 12 questions in one request".to_string(),
        ));
    }

    let mut normalized = Vec::new();
    for (index, raw) in questions.iter().enumerate() {
        let object = raw.as_object().ok_or_else(|| {
            (
                ToolResultStatus::Error,
                format!("ask_user question {} must be an object", index + 1),
            )
        })?;
        let id = object
            .get("id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| truncate_chars(value, 64))
            .unwrap_or_else(|| format!("q{}", index + 1));
        let question = object
            .get("question")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| truncate_chars(value, 500))
            .ok_or_else(|| {
                (
                    ToolResultStatus::Error,
                    format!("ask_user question {} requires a question string", index + 1),
                )
            })?;
        let options = object
            .get("options")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .enumerate()
                    .map(|(option_index, raw_option)| {
                        let option = raw_option.as_object().ok_or_else(|| {
                            (
                                ToolResultStatus::Error,
                                format!(
                                    "ask_user question {} option {} must be an object",
                                    index + 1,
                                    option_index + 1
                                ),
                            )
                        })?;
                        let option_id = option
                            .get("id")
                            .and_then(Value::as_str)
                            .map(str::trim)
                            .filter(|value| !value.is_empty())
                            .map(|value| truncate_chars(value, 64))
                            .unwrap_or_else(|| format!("option-{}", option_index + 1));
                        let label = option
                            .get("label")
                            .and_then(Value::as_str)
                            .map(str::trim)
                            .filter(|value| !value.is_empty())
                            .map(|value| truncate_chars(value, 200))
                            .unwrap_or_else(|| option_id.clone());
                        let description = option
                            .get("description")
                            .and_then(Value::as_str)
                            .map(str::trim)
                            .filter(|value| !value.is_empty())
                            .map(|value| truncate_chars(value, 400));
                        Ok(AskUserOption {
                            id: option_id,
                            label,
                            description,
                        })
                    })
                    .collect::<Result<Vec<_>, (ToolResultStatus, String)>>()
            })
            .transpose()?
            .unwrap_or_default();
        normalized.push(AskUserQuestion {
            id,
            question,
            options,
            allow_custom: object.get("allowCustom").and_then(Value::as_bool),
            default_option_id: object
                .get("defaultOptionId")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(|value| truncate_chars(value, 64)),
            required: object
                .get("required")
                .and_then(Value::as_bool)
                .unwrap_or(true),
        });
    }

    Ok(AskUserRequest {
        id: request_id,
        title,
        questions: normalized,
        tool_call_id: Some(call.id.clone()),
        timeout_ms: settings.timeout_ms,
    })
}

fn ask_user_answer_text(answer: &AskUserAnswer) -> String {
    if answer.skipped {
        return "skipped".to_string();
    }
    if let Some(text) = answer.text.as_ref().filter(|text| !text.trim().is_empty()) {
        return format!("custom: {}", truncate_chars(text.trim(), 500));
    }
    if let Some(option_id) = answer.option_id.as_ref() {
        if let Some(label) = answer
            .label
            .as_ref()
            .filter(|label| !label.trim().is_empty())
        {
            return format!("{} ({})", option_id, truncate_chars(label.trim(), 200));
        }
        return option_id.clone();
    }
    answer
        .label
        .as_ref()
        .map(|label| truncate_chars(label.trim(), 200))
        .filter(|label| !label.is_empty())
        .unwrap_or_else(|| "answered".to_string())
}

fn ask_user_response_text(response: &AskUserResponse) -> String {
    if response.cancelled {
        return "User cancelled the questionnaire.".to_string();
    }
    let prefix = if response.timed_out {
        "Questionnaire timed out; unanswered questions used recommended/default answers.\n"
    } else {
        ""
    };
    let answers = response
        .answers
        .iter()
        .map(|answer| format!("{}: {}", answer.question_id, ask_user_answer_text(answer)))
        .collect::<Vec<_>>()
        .join("\n");
    format!("{}{}", prefix, answers)
}

fn parse_mermaid_node_label(token: &str) -> String {
    let trimmed = token
        .trim()
        .trim_matches(|ch: char| ch == ';' || ch == ',')
        .trim();
    for (open, close) in [('[', ']'), ('(', ')'), ('{', '}')] {
        if let Some(start) = trimmed.find(open)
            && let Some(end) = trimmed[start + 1..].find(close)
        {
            let label = trimmed[start + 1..start + 1 + end].trim();
            if !label.is_empty() {
                return label.to_string();
            }
        }
    }
    trimmed
        .trim_matches(|ch: char| ch == '-' || ch == '>' || ch == '<' || ch == '|')
        .trim()
        .to_string()
}

fn render_mermaid_flowchart(lines: &[&str]) -> Option<String> {
    let mut edges = Vec::new();
    let mut standalone = Vec::new();
    let arrows = ["-->", "==>", "-.->", "---"];
    for line in lines {
        let cleaned = line.split("%%").next().unwrap_or_default().trim();
        if cleaned.is_empty()
            || cleaned.starts_with("flowchart")
            || cleaned.starts_with("graph")
            || cleaned.starts_with("subgraph")
            || cleaned == "end"
        {
            continue;
        }
        if let Some((arrow, index)) = arrows
            .iter()
            .filter_map(|arrow| cleaned.find(arrow).map(|index| (*arrow, index)))
            .next()
        {
            let from = parse_mermaid_node_label(&cleaned[..index]);
            let to = parse_mermaid_node_label(&cleaned[index + arrow.len()..]);
            if !from.is_empty() && !to.is_empty() {
                edges.push(format!("  {from} -> {to}"));
            }
        } else {
            let node = parse_mermaid_node_label(cleaned);
            if !node.is_empty() {
                standalone.push(format!("  [{node}]"));
            }
        }
    }
    if edges.is_empty() && standalone.is_empty() {
        None
    } else {
        Some(
            [
                vec![
                    "Mermaid flowchart (simple ASCII fallback)".to_string(),
                    String::new(),
                ],
                edges,
                standalone,
            ]
            .concat()
            .join("\n"),
        )
    }
}

fn render_mermaid_sequence(lines: &[&str]) -> Option<String> {
    let mut participants = BTreeSet::new();
    let mut messages = Vec::new();
    for line in lines {
        let cleaned = line.split("%%").next().unwrap_or_default().trim();
        if cleaned.is_empty() || cleaned.starts_with("sequenceDiagram") {
            continue;
        }
        if let Some(rest) = cleaned.strip_prefix("participant ") {
            let label = rest
                .split_once(" as ")
                .map(|(_, label)| label)
                .unwrap_or(rest)
                .trim();
            if !label.is_empty() {
                participants.insert(label.to_string());
            }
            continue;
        }
        let Some((left, body)) = cleaned.split_once(':') else {
            continue;
        };
        let arrow = ["-->>", "->>", "-->", "->"]
            .iter()
            .find_map(|arrow| left.find(arrow).map(|index| (*arrow, index)));
        if let Some((arrow, index)) = arrow {
            let from = left[..index].trim();
            let to = left[index + arrow.len()..]
                .trim_matches(|ch| ch == '+' || ch == '-')
                .trim();
            if !from.is_empty() && !to.is_empty() {
                participants.insert(from.to_string());
                participants.insert(to.to_string());
                messages.push(format!("  {from} -> {to}: {}", body.trim()));
            }
        }
    }
    if messages.is_empty() {
        None
    } else {
        Some(
            [
                vec![
                    "Mermaid sequence diagram (simple ASCII fallback)".to_string(),
                    format!(
                        "Participants: {}",
                        participants.into_iter().collect::<Vec<_>>().join(", ")
                    ),
                    String::new(),
                ],
                messages,
            ]
            .concat()
            .join("\n"),
        )
    }
}

fn feedback_clamp(value: Option<&Value>, max_chars: usize) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| truncate_chars(value, max_chars))
}

fn feedback_sanitize(text: &str) -> String {
    text.split_whitespace()
        .map(|token| {
            let lower = token.to_lowercase();
            if lower.starts_with("sk-")
                || lower.starts_with("ghp_")
                || lower.starts_with("gho_")
                || lower.contains("token=")
                || lower.contains("password=")
                || lower.contains("secret=")
                || lower.contains("api_key=")
            {
                "<redacted>".to_string()
            } else {
                token.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn feedback_section(title: &str, body: Option<String>) -> String {
    let Some(body) = body else {
        return String::new();
    };
    let body = feedback_sanitize(&body);
    if body.trim().is_empty() {
        String::new()
    } else {
        format!("## {title}\n\n{body}\n\n")
    }
}

fn feedback_list_section(title: &str, value: Option<&Value>) -> String {
    let items = value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str())
                .map(|item| feedback_sanitize(&truncate_chars(item.trim(), 500)))
                .filter(|item| !item.is_empty())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if items.is_empty() {
        String::new()
    } else {
        format!(
            "## {title}\n\n{}\n\n",
            items
                .iter()
                .map(|item| format!("- {item}"))
                .collect::<Vec<_>>()
                .join("\n")
        )
    }
}

fn feedback_title(kind: &str, summary: &str) -> String {
    let prefix = if kind == "bug-report" {
        "[Bug]"
    } else {
        "[Feature]"
    };
    truncate_chars(&format!("{prefix} {summary}"), 140)
}

fn feedback_body(
    kind: &str,
    call: &ToolCall,
    cwd: &str,
) -> Result<(String, String, String), (ToolResultStatus, String)> {
    let summary = feedback_clamp(call.arguments.get("summary"), 160).ok_or_else(|| {
        (
            ToolResultStatus::Error,
            "oppi_feedback_submit requires a summary".to_string(),
        )
    })?;
    let mut missing = Vec::new();
    let mut body = String::new();
    body.push_str(&feedback_section("Summary", Some(summary.clone())));
    if kind == "bug-report" {
        let what = feedback_clamp(call.arguments.get("whatHappened"), 12_000)
            .or_else(|| feedback_clamp(call.arguments.get("description"), 12_000));
        let expected = feedback_clamp(call.arguments.get("expectedBehavior"), 12_000);
        let repro = feedback_clamp(call.arguments.get("reproduction"), 12_000);
        if what.is_none() {
            missing.push("what happened");
        }
        if expected.is_none() {
            missing.push("expected behavior");
        }
        if repro.is_none() {
            missing.push("reproduction/context");
        }
        body.push_str(&feedback_section("What happened", what));
        body.push_str(&feedback_section("Expected behavior", expected));
        body.push_str(&feedback_section("Reproduction / context", repro));
        body.push_str(&feedback_section(
            "Impact",
            feedback_clamp(call.arguments.get("impact"), 12_000),
        ));
    } else {
        let requested = feedback_clamp(call.arguments.get("requestedBehavior"), 12_000)
            .or_else(|| feedback_clamp(call.arguments.get("description"), 12_000));
        let value = feedback_clamp(call.arguments.get("userValue"), 12_000);
        let workflow = feedback_clamp(call.arguments.get("exampleWorkflow"), 12_000);
        let criteria = feedback_list_section(
            "Acceptance criteria",
            call.arguments.get("acceptanceCriteria"),
        );
        if requested.is_none() {
            missing.push("requested behavior");
        }
        if value.is_none() {
            missing.push("why it matters / workflow value");
        }
        if workflow.is_none() && criteria.is_empty() {
            missing.push("example workflow or acceptance criteria");
        }
        body.push_str(&feedback_section("Requested behavior", requested));
        body.push_str(&feedback_section("Why this matters", value));
        body.push_str(&feedback_section("Example workflow", workflow));
        body.push_str(&criteria);
    }
    if !missing.is_empty() {
        return Err((
            ToolResultStatus::Error,
            format!(
                "oppi_feedback_submit needs more context: {}",
                missing.join(", ")
            ),
        ));
    }
    if call
        .arguments
        .get("includeDiagnostics")
        .and_then(Value::as_bool)
        .unwrap_or(true)
    {
        body.push_str(&format!(
            "## Sanitized diagnostics\n\n- cwd: {}\n- platform: {}\n\n",
            feedback_sanitize(cwd),
            std::env::consts::OS
        ));
    }
    body.push_str("---\n\nCreated from OPPi feedback intake. Sensitive values are redacted client-side and again by the intake worker when submitted.\n");
    let repo = feedback_clamp(call.arguments.get("repo"), 200)
        .unwrap_or_else(|| "RemindZ/oppi".to_string());
    Ok((feedback_title(kind, &summary), body, repo))
}

fn render_mermaid_fallback(source: &str) -> Result<String, (ToolResultStatus, String)> {
    let source = source.replace("\r\n", "\n").replace('\r', "\n");
    let lines = source.lines().collect::<Vec<_>>();
    let first = lines
        .iter()
        .map(|line| line.trim())
        .find(|line| !line.is_empty() && !line.starts_with("%%"))
        .unwrap_or_default();
    if first.starts_with("flowchart") || first.starts_with("graph") {
        return render_mermaid_flowchart(&lines).ok_or_else(|| {
            (
                ToolResultStatus::Error,
                "render_mermaid fallback could not parse any flowchart nodes or edges".to_string(),
            )
        });
    }
    if first.starts_with("sequenceDiagram") {
        return render_mermaid_sequence(&lines).ok_or_else(|| {
            (
                ToolResultStatus::Error,
                "render_mermaid fallback could not parse any sequence messages".to_string(),
            )
        });
    }
    Err((
        ToolResultStatus::Error,
        "render_mermaid fallback supports simple flowchart/graph and sequenceDiagram inputs"
            .to_string(),
    ))
}

fn normalize_todo_write_state(call: &ToolCall) -> Result<TodoState, (ToolResultStatus, String)> {
    let items = call
        .arguments
        .get("todos")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            (
                ToolResultStatus::Error,
                "todo_write requires a todos array containing the full current list".to_string(),
            )
        })?;
    let mut seen = BTreeSet::new();
    let mut todos = Vec::new();
    for (index, item) in items.iter().enumerate() {
        let object = item.as_object().ok_or_else(|| {
            (
                ToolResultStatus::Error,
                format!("todo_write item {} must be an object", index + 1),
            )
        })?;
        let raw_id = object
            .get("id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| truncate_chars(value, 64))
            .unwrap_or_else(|| (index + 1).to_string());
        let id = if seen.insert(raw_id.clone()) {
            raw_id
        } else {
            let deduped = format!("{}-{}", raw_id, index + 1);
            seen.insert(deduped.clone());
            deduped
        };
        let content = object
            .get("content")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| truncate_chars(value, 300))
            .unwrap_or_else(|| "Untitled task".to_string());
        let status = todo_status(object.get("status").unwrap_or(&Value::Null))?;
        todos.push(TodoItem {
            id,
            content,
            status,
            priority: todo_priority(object.get("priority"))?,
            phase: optional_todo_string(object.get("phase"), 80),
            notes: optional_todo_string(object.get("notes"), 500),
        });
    }
    let summary = call
        .arguments
        .get("summary")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| truncate_chars(value, 500))
        .unwrap_or_else(|| todo_summary_for(&todos));
    Ok(TodoState { todos, summary })
}

fn todo_write_output(state: &TodoState) -> String {
    let mut lines = vec![format!("Updated todos: {}", state.summary)];
    for todo in &state.todos {
        lines.push(format!(
            "- [{}] {}: {}",
            todo.status.as_str(),
            todo.id,
            todo.content
        ));
    }
    lines.join("\n")
}

fn read_task_tail(path: &Path, max_bytes: usize) -> String {
    let bytes = fs::read(path).unwrap_or_default();
    if bytes.len() <= max_bytes {
        return String::from_utf8_lossy(&bytes).to_string();
    }
    String::from_utf8_lossy(&bytes[bytes.len() - max_bytes..]).to_string()
}

fn file_io_error(error: std::io::Error) -> (ToolResultStatus, String) {
    (
        ToolResultStatus::Error,
        format!("file tool I/O error: {error}"),
    )
}

fn resolve_project_path(cwd: &str, raw: &str) -> Result<PathBuf, (ToolResultStatus, String)> {
    let cwd = lexical_normalize(Path::new(cwd));
    let raw_path = Path::new(raw);
    let path = if raw_path.is_absolute() {
        raw_path.to_path_buf()
    } else {
        cwd.join(raw_path)
    };
    let normalized = lexical_normalize(&path);
    if !normalized.starts_with(&cwd) {
        return Err((
            ToolResultStatus::Denied,
            "path escapes the project working directory".to_string(),
        ));
    }
    Ok(normalized)
}

fn resolve_child_worktree_root(
    parent_root: &Path,
    requested: &str,
) -> Result<PathBuf, (ToolResultStatus, String)> {
    let requested = requested.trim();
    if requested.is_empty() {
        return Err((
            ToolResultStatus::Error,
            "worktreeRoot must not be empty".to_string(),
        ));
    }
    let parent_root = lexical_normalize(parent_root);
    let requested_path = Path::new(requested);
    let resolved = if requested_path.is_absolute() {
        requested_path.to_path_buf()
    } else {
        parent_root.join(requested_path)
    };
    let resolved = lexical_normalize(&resolved);
    if !resolved.starts_with(&parent_root) {
        return Err((
            ToolResultStatus::Denied,
            format!("worktreeRoot must resolve inside the parent root: {requested}"),
        ));
    }
    if !resolved.is_dir() {
        return Err((
            ToolResultStatus::Error,
            format!(
                "worktreeRoot directory does not exist: {requested} (resolved to {})",
                resolved.display()
            ),
        ));
    }
    Ok(resolved)
}

fn lexical_normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::Normal(part) => normalized.push(part),
        }
    }
    normalized
}

fn default_file_tool_policy(cwd: &str, writes: bool) -> SandboxPolicy {
    default_policy(PermissionProfile {
        mode: if writes {
            PermissionMode::Default
        } else {
            PermissionMode::ReadOnly
        },
        readable_roots: vec![cwd.to_string()],
        writable_roots: if writes {
            vec![cwd.to_string()]
        } else {
            Vec::new()
        },
        filesystem_rules: Vec::new(),
        protected_patterns: Vec::new(),
    })
}

fn default_shell_policy(cwd: &str) -> SandboxPolicy {
    default_policy(PermissionProfile {
        mode: PermissionMode::ReadOnly,
        readable_roots: vec![cwd.to_string()],
        writable_roots: Vec::new(),
        filesystem_rules: Vec::new(),
        protected_patterns: Vec::new(),
    })
}

fn shell_sandbox_preference_from_call(
    call: &ToolCall,
    fallback: SandboxPreference,
) -> SandboxPreference {
    call.arguments
        .get("sandboxPreference")
        .or_else(|| call.arguments.get("sandbox_preference"))
        .or_else(|| call.arguments.get("preference"))
        .cloned()
        .and_then(|value| serde_json::from_value::<SandboxPreference>(value).ok())
        .unwrap_or(fallback)
}

fn policy_with_cwd_defaults(mut policy: SandboxPolicy, cwd: &str) -> SandboxPolicy {
    if policy.permission_profile.readable_roots.is_empty() {
        policy
            .permission_profile
            .readable_roots
            .push(cwd.to_string());
    }
    if policy.filesystem != FilesystemPolicy::ReadOnly
        && policy.permission_profile.writable_roots.is_empty()
        && policy.filesystem != FilesystemPolicy::Unrestricted
    {
        policy
            .permission_profile
            .writable_roots
            .push(cwd.to_string());
    }
    policy
}

fn file_path_allowed_by_roots(
    path: &Path,
    cwd: &str,
    policy: &SandboxPolicy,
    writes: bool,
) -> bool {
    if policy.filesystem == FilesystemPolicy::Unrestricted {
        return true;
    }
    if writes && policy.filesystem == FilesystemPolicy::ReadOnly {
        return false;
    }
    let mut roots = if writes {
        policy.permission_profile.writable_roots.clone()
    } else if policy.permission_profile.readable_roots.is_empty() {
        policy.permission_profile.writable_roots.clone()
    } else {
        policy.permission_profile.readable_roots.clone()
    };
    if roots.is_empty() {
        roots.push(cwd.to_string());
    }
    let path = lexical_normalize(path);
    roots.iter().any(|root| {
        let root = Path::new(root);
        let root = if root.is_absolute() {
            lexical_normalize(root)
        } else {
            lexical_normalize(&Path::new(cwd).join(root))
        };
        path.starts_with(root)
    })
}

fn protected_path_preflight_denial(
    policy: &SandboxPolicy,
    request: &SandboxExecRequest,
) -> Option<String> {
    match evaluate_exec(policy, request) {
        PolicyDecision::Ask { reason, .. } if reason.contains("protected path") => Some(reason),
        _ => None,
    }
}

fn guardian_tool_signature(call: &ToolCall) -> String {
    let namespace = call.namespace.as_deref().unwrap_or("default");
    let args = serde_json::to_string(&call.arguments).unwrap_or_else(|_| "{}".to_string());
    format!("{namespace}::{}\n{args}", call.name)
}

fn guardian_circuit_key(call: &ToolCall, risk: &GuardianRiskSummary) -> String {
    let namespace = call.namespace.as_deref().unwrap_or("default");
    let paths = risk.paths.join("|");
    format!("{namespace}::{}:{}:{paths}", call.name, risk.category)
}

fn guardian_can_cache(record: &GuardianReviewRecord) -> bool {
    let Some(decision) = record.decision.as_ref() else {
        return false;
    };
    decision.outcome == GuardianReviewOutcome::Allow
        && decision.risk_level == RiskLevel::Low
        && matches!(
            decision.user_authorization,
            GuardianAuthorizationLevel::Medium | GuardianAuthorizationLevel::High
        )
        && decision.cache_scope == Some(GuardianCacheScope::Exact)
        && record.risk.protected_hits.is_empty()
}

fn guardian_review_counts_as_denial(record: &GuardianReviewRecord) -> bool {
    matches!(
        record.status,
        GuardianReviewStatus::Denied
            | GuardianReviewStatus::FailedClosed
            | GuardianReviewStatus::CircuitBlocked
    ) || record
        .decision
        .as_ref()
        .is_some_and(|decision| decision.outcome == GuardianReviewOutcome::Deny)
}

fn guardian_reviewer_provider_is_meridian(config: &DirectModelProviderConfig) -> bool {
    config.api_key_env.as_deref() == Some(MERIDIAN_API_KEY_ENV)
        || config.model.to_ascii_lowercase().starts_with("claude-")
}

fn guardian_reviewer_config_from_setting(
    setting: &str,
) -> Result<DirectModelProviderConfig, RuntimeError> {
    let (provider, model) = setting.trim().split_once('/').ok_or_else(|| {
        RuntimeError::new(
            "provider_auth_missing",
            RuntimeErrorCategory::Provider,
            "auto-review reviewer model must be provider/model, for example openai-compatible/gpt-5.5",
        )
    })?;
    let model = model.trim();
    if model.is_empty() {
        return Err(RuntimeError::new(
            "provider_auth_missing",
            RuntimeErrorCategory::Provider,
            "auto-review reviewer model is missing the model name after provider/",
        ));
    }
    let kind = match provider.trim() {
        "openai-compatible" | "openai" => DirectModelProviderKind::OpenAiCompatible,
        "openai-codex" => DirectModelProviderKind::OpenAiCodex,
        other => {
            return Err(RuntimeError::new(
                "provider_auth_missing",
                RuntimeErrorCategory::Provider,
                format!(
                    "no non-Meridian reviewer provider is available for reviewer model provider `{other}`"
                ),
            ));
        }
    };
    Ok(DirectModelProviderConfig {
        kind,
        model: model.to_string(),
        base_url: None,
        api_key_env: None,
        system_prompt: Some(GUARDIAN_REVIEWER_SYSTEM_PROMPT.to_string()),
        temperature: None,
        reasoning_effort: None,
        max_output_tokens: Some(GUARDIAN_REVIEWER_MAX_OUTPUT_TOKENS),
        stream: false,
    })
}

fn guardian_reviewer_provider_config(
    parent: Option<&DirectModelProviderConfig>,
    settings: &GuardianReviewSettings,
) -> Result<DirectModelProviderConfig, RuntimeError> {
    if let Some(setting) = settings
        .reviewer_model
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return guardian_reviewer_config_from_setting(setting);
    }
    let Some(parent) = parent else {
        return Err(RuntimeError::new(
            "provider_auth_missing",
            RuntimeErrorCategory::Provider,
            "auto-review failed closed: no non-Meridian reviewer provider is available",
        ));
    };
    if guardian_reviewer_provider_is_meridian(parent)
        || !matches!(
            parent.kind,
            DirectModelProviderKind::OpenAiCompatible | DirectModelProviderKind::OpenAiCodex
        )
    {
        return Err(RuntimeError::new(
            "provider_auth_missing",
            RuntimeErrorCategory::Provider,
            "auto-review failed closed: no non-Meridian reviewer provider is available",
        ));
    }
    let mut config = parent.clone();
    config.system_prompt = Some(GUARDIAN_REVIEWER_SYSTEM_PROMPT.to_string());
    config.temperature = None;
    config.max_output_tokens = Some(GUARDIAN_REVIEWER_MAX_OUTPUT_TOKENS);
    config.stream = false;
    Ok(config)
}

fn guardian_json_object_candidates(text: &str) -> Vec<String> {
    let mut candidates = Vec::new();
    let mut start = None;
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (index, ch) in text.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '{' => {
                if depth == 0 {
                    start = Some(index);
                }
                depth += 1;
            }
            '}' if depth > 0 => {
                depth -= 1;
                if depth == 0
                    && let Some(start) = start.take()
                {
                    candidates.push(text[start..=index].to_string());
                }
            }
            _ => {}
        }
    }
    candidates
}

fn parse_guardian_review_outcome(value: &Value) -> Result<GuardianReviewOutcome, String> {
    match value.as_str().unwrap_or_default() {
        "allow" => Ok(GuardianReviewOutcome::Allow),
        "deny" => Ok(GuardianReviewOutcome::Deny),
        other => Err(format!("unknown outcome `{other}`")),
    }
}

fn parse_guardian_review_risk(value: &Value) -> Result<RiskLevel, String> {
    match value.as_str().unwrap_or_default() {
        "low" => Ok(RiskLevel::Low),
        "medium" => Ok(RiskLevel::Medium),
        "high" => Ok(RiskLevel::High),
        "critical" => Ok(RiskLevel::Critical),
        other => Err(format!("unknown risk_level `{other}`")),
    }
}

fn parse_guardian_authorization(value: &Value) -> Result<GuardianAuthorizationLevel, String> {
    match value.as_str().unwrap_or_default() {
        "unknown" => Ok(GuardianAuthorizationLevel::Unknown),
        "low" => Ok(GuardianAuthorizationLevel::Low),
        "medium" => Ok(GuardianAuthorizationLevel::Medium),
        "high" => Ok(GuardianAuthorizationLevel::High),
        other => Err(format!("unknown user_authorization `{other}`")),
    }
}

fn parse_guardian_cache_scope(value: Option<&Value>) -> Result<GuardianCacheScope, String> {
    match value.and_then(Value::as_str).unwrap_or("none") {
        "none" => Ok(GuardianCacheScope::None),
        "exact" => Ok(GuardianCacheScope::Exact),
        other => Err(format!("unknown cache_scope `{other}`")),
    }
}

fn parse_guardian_review_decision(
    text: &str,
) -> Result<(oppi_protocol::GuardianReviewDecision, String), String> {
    let candidates = guardian_json_object_candidates(text);
    if candidates.len() != 1 {
        return Err(format!(
            "expected exactly one JSON object, found {}",
            candidates.len()
        ));
    }
    let strict_json = candidates[0].clone();
    let value: Value = serde_json::from_str(&strict_json)
        .map_err(|error| format!("reviewer returned invalid JSON: {error}"))?;
    let object = value
        .as_object()
        .ok_or_else(|| "reviewer decision must be a JSON object".to_string())?;
    let outcome = parse_guardian_review_outcome(
        object
            .get("outcome")
            .ok_or_else(|| "reviewer decision missing outcome".to_string())?,
    )?;
    let risk_level = parse_guardian_review_risk(
        object
            .get("risk_level")
            .ok_or_else(|| "reviewer decision missing risk_level".to_string())?,
    )?;
    let user_authorization = parse_guardian_authorization(
        object
            .get("user_authorization")
            .ok_or_else(|| "reviewer decision missing user_authorization".to_string())?,
    )?;
    let rationale = object
        .get("rationale")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "reviewer decision missing non-empty rationale".to_string())?
        .to_string();
    let cache_scope = parse_guardian_cache_scope(object.get("cache_scope"))?;
    Ok((
        oppi_protocol::GuardianReviewDecision {
            outcome,
            risk_level,
            user_authorization,
            rationale,
            cache_scope: Some(cache_scope),
        },
        strict_json,
    ))
}

fn risk_level_name(risk: RiskLevel) -> &'static str {
    match risk {
        RiskLevel::Low => "low",
        RiskLevel::Medium => "medium",
        RiskLevel::High => "high",
        RiskLevel::Critical => "critical",
    }
}

fn guardian_decision_name(decision: GuardianReviewDecisionKind) -> &'static str {
    match decision {
        GuardianReviewDecisionKind::Allow => "allow",
        GuardianReviewDecisionKind::Deny => "deny",
    }
}

fn guardian_review_result(
    decision: GuardianReviewDecisionKind,
    risk: RiskLevel,
    reason: impl Into<String>,
    call: &ToolCall,
) -> GuardianReviewResult {
    let reason = reason.into();
    let strict_json = json!({
        "outcome": guardian_decision_name(decision),
        "risk_level": risk_level_name(risk),
        "user_authorization": "unknown",
        "rationale": reason,
        "cache_scope": "none",
        "tool": {
            "id": &call.id,
            "namespace": &call.namespace,
            "name": &call.name,
        },
        "reviewer": {
            "kind": "guardian-local",
            "isolated": true,
            "boundedReadOnly": true,
            "toolAllowlist": ["oppi_review_read", "oppi_review_ls", "oppi_review_grep"],
            "strictJson": true,
            "timeoutMs": 750,
            "failClosed": decision == GuardianReviewDecisionKind::Deny,
        }
    })
    .to_string();
    GuardianReviewResult {
        decision,
        risk,
        reason,
        strict_json,
    }
}

fn guardian_call_paths(call: &ToolCall) -> Vec<String> {
    let mut paths = BTreeSet::new();
    for key in ["path", "root", "cwd"] {
        if let Some(path) = first_string_arg(&call.arguments, &[key]) {
            paths.insert(path);
        }
    }
    if let Some(items) = call.arguments.get("touchedPaths").and_then(Value::as_array) {
        for item in items {
            if let Some(path) = item.as_str().map(str::to_string) {
                paths.insert(path);
            }
        }
    }
    paths.into_iter().collect()
}

fn guardian_risk_category(call: &ToolCall) -> String {
    if call.name.contains("shell") || call.name == "execute" {
        "shell".to_string()
    } else if matches!(call.name.as_str(), "write_file" | "edit_file") {
        "workspace-write".to_string()
    } else if matches!(
        call.name.as_str(),
        "read_file" | "search_files" | "list_files" | "oppi_review_read" | "oppi_review_ls"
    ) {
        "workspace-read".to_string()
    } else {
        "tool-call".to_string()
    }
}

fn guardian_risk_summary(call: &ToolCall, review: &GuardianReviewResult) -> GuardianRiskSummary {
    let paths = guardian_call_paths(call);
    let protected_hits = if review
        .reason
        .to_ascii_lowercase()
        .contains("protected path")
    {
        paths.clone()
    } else {
        Vec::new()
    };
    GuardianRiskSummary {
        category: guardian_risk_category(call),
        reason: review.reason.clone(),
        protected_hits,
        paths,
        hints: vec![format!(
            "local preflight outcome={} risk={}",
            guardian_decision_name(review.decision),
            risk_level_name(review.risk)
        )],
    }
}

fn guardian_approved_tool_call(call: &ToolCall) -> ToolCall {
    let mut approved = call.clone();
    match &mut approved.arguments {
        Value::Object(arguments) => {
            arguments.insert("approvalGranted".to_string(), Value::Bool(true));
        }
        _ => {
            approved.arguments = json!({ "approvalGranted": true });
        }
    }
    approved
}

fn value_contains_raw_secret(value: &Value) -> bool {
    match value {
        Value::String(text) => string_contains_raw_secret(text),
        Value::Array(items) => items.iter().any(value_contains_raw_secret),
        Value::Object(map) => map.values().any(value_contains_raw_secret),
        _ => false,
    }
}

fn string_contains_raw_secret(text: &str) -> bool {
    text.split_whitespace().any(|token| {
        let lower = token.to_ascii_lowercase();
        lower.starts_with("sk-")
            || lower.starts_with("ghp_")
            || lower.starts_with("gho_")
            || lower.starts_with("xoxb-")
            || lower.starts_with("bearer sk-")
            || lower.contains("token=")
            || lower.contains("password=")
            || lower.contains("secret=")
            || lower.contains("api_key=sk-")
    })
}

fn file_tool_writes(kind: FileToolKind) -> bool {
    matches!(kind, FileToolKind::Write | FileToolKind::Edit)
}

fn guardian_file_tool_request(
    call: &ToolCall,
    kind: FileToolKind,
    cwd: &str,
) -> Result<SandboxExecRequest, String> {
    let Some(raw_path) = first_string_arg(&call.arguments, &["path"]) else {
        return Err("file tool path is missing; auto-review fails closed".to_string());
    };
    let path = resolve_project_path(cwd, &raw_path).map_err(|(_, error)| error)?;
    Ok(SandboxExecRequest {
        command: format!("file-tool:{}", call.name),
        cwd: cwd.to_string(),
        writes_files: file_tool_writes(kind),
        uses_network: false,
        touches_protected_path: false,
        touched_paths: vec![path.to_string_lossy().to_string()],
    })
}

fn guardian_shell_tool_request(call: &ToolCall, cwd: &str) -> Result<SandboxExecRequest, String> {
    let command = first_string_arg(&call.arguments, &["command"])
        .ok_or_else(|| "shell command is missing; auto-review fails closed".to_string())?;
    Ok(SandboxExecRequest {
        command,
        cwd: first_string_arg(&call.arguments, &["cwd"]).unwrap_or_else(|| cwd.to_string()),
        writes_files: call
            .arguments
            .get("writesFiles")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        uses_network: call
            .arguments
            .get("usesNetwork")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        touches_protected_path: call
            .arguments
            .get("touchesProtectedPath")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        touched_paths: call
            .arguments
            .get("touchedPaths")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
    })
}

fn guardian_mcp_tool_request(
    definition: &ToolDefinition,
    call: &ToolCall,
    cwd: &str,
) -> Result<SandboxExecRequest, String> {
    let Some(target) = mcp_tool_target(definition, call) else {
        return Err("MCP tool metadata is missing; auto-review fails closed".to_string());
    };
    let arguments = mcp_call_arguments(&call.arguments);
    Ok(SandboxExecRequest {
        command: format!("mcp-tool:{}::{}", target.server_id, target.tool_name),
        cwd: cwd.to_string(),
        writes_files: definition.requires_approval
            || definition
                .annotations
                .get("destructiveHint")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        uses_network: definition
            .annotations
            .get("openWorldHint")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        touches_protected_path: call
            .arguments
            .get("touchesProtectedPath")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        touched_paths: mcp_tool_argument_paths(cwd, &arguments),
    })
}

fn guardian_plugin_runner_tool_request(
    definition: &ToolDefinition,
    call: &ToolCall,
    cwd: &str,
) -> Result<SandboxExecRequest, String> {
    let Some(target) = plugin_runner_tool_target(definition, call) else {
        return Err("plugin runner tool metadata is missing; auto-review fails closed".to_string());
    };
    let arguments = mcp_call_arguments(&call.arguments);
    Ok(SandboxExecRequest {
        command: format!("plugin-runner:{}::{}", target.plugin_id, target.tool_name),
        cwd: cwd.to_string(),
        writes_files: definition.requires_approval,
        uses_network: definition
            .annotations
            .get("openWorldHint")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        touches_protected_path: call
            .arguments
            .get("touchesProtectedPath")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        touched_paths: mcp_tool_argument_paths(cwd, &arguments),
    })
}

fn guardian_review_request_for_tool(
    definition: &ToolDefinition,
    call: &ToolCall,
    cwd: &str,
) -> Result<Option<(SandboxPolicy, SandboxExecRequest)>, String> {
    if is_shell_tool(definition, call) {
        return Ok(Some((
            default_shell_policy(cwd),
            guardian_shell_tool_request(call, cwd)?,
        )));
    }
    if let Some(kind) = file_tool_kind(definition, call) {
        return Ok(Some((
            default_file_tool_policy(cwd, file_tool_writes(kind)),
            guardian_file_tool_request(call, kind, cwd)?,
        )));
    }
    if mcp_tool_target(definition, call).is_some() {
        return Ok(Some((
            default_file_tool_policy(cwd, definition.requires_approval),
            guardian_mcp_tool_request(definition, call, cwd)?,
        )));
    }
    if plugin_runner_tool_target(definition, call).is_some() {
        return Ok(Some((
            default_file_tool_policy(cwd, definition.requires_approval),
            guardian_plugin_runner_tool_request(definition, call, cwd)?,
        )));
    }
    Ok(None)
}

#[allow(clippy::too_many_arguments)]
fn search_files_under(
    runtime: &Runtime,
    turn: &Turn,
    call: &ToolCall,
    cwd: &str,
    root: &Path,
    query: &str,
    max_results: usize,
    max_bytes_per_file: usize,
    results: &mut Vec<String>,
) {
    if results.len() >= max_results {
        return;
    }
    if root.is_file() {
        search_one_file(
            runtime,
            turn,
            call,
            cwd,
            root,
            query,
            max_results,
            max_bytes_per_file,
            results,
        );
        return;
    }
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        if results.len() >= max_results {
            return;
        }
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            search_files_under(
                runtime,
                turn,
                call,
                cwd,
                &path,
                query,
                max_results,
                max_bytes_per_file,
                results,
            );
        } else if file_type.is_file() {
            search_one_file(
                runtime,
                turn,
                call,
                cwd,
                &path,
                query,
                max_results,
                max_bytes_per_file,
                results,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn search_one_file(
    runtime: &Runtime,
    turn: &Turn,
    call: &ToolCall,
    cwd: &str,
    path: &Path,
    query: &str,
    max_results: usize,
    max_bytes_per_file: usize,
    results: &mut Vec<String>,
) {
    if results.len() >= max_results
        || runtime
            .preflight_file_access(turn, call, cwd, path, false)
            .is_err()
    {
        return;
    }
    let display = path.display().to_string();
    if display.contains(query) {
        results.push(display.clone());
        if results.len() >= max_results {
            return;
        }
    }
    let Ok(bytes) = fs::read(path) else {
        return;
    };
    let text = String::from_utf8_lossy(&bytes[..bytes.len().min(max_bytes_per_file)]);
    for (index, line) in text.lines().enumerate() {
        if line.contains(query) {
            results.push(format!(
                "{}:{}:{}",
                display,
                index + 1,
                truncate_chars(line.trim(), 240)
            ));
            if results.len() >= max_results {
                return;
            }
        }
    }
}

fn tool_results_by_call_id(
    step: &ScriptedModelStep,
) -> Result<BTreeMap<ToolCallId, ToolResult>, RuntimeError> {
    let mut results = BTreeMap::new();
    for result in &step.tool_results {
        if results
            .insert(result.call_id.clone(), result.clone())
            .is_some()
        {
            return Err(RuntimeError::new(
                "duplicate_tool_result",
                RuntimeErrorCategory::ToolPairing,
                format!("duplicate tool result for call {}", result.call_id),
            ));
        }
    }
    Ok(results)
}

fn cancellation_before_model_reason(
    cancellation: &Option<AgenticCancellation>,
    continuation: u32,
) -> Option<String> {
    let cancellation = cancellation.as_ref()?;
    if cancellation.before_model_continuation == Some(continuation) {
        Some(cancellation.reason.clone())
    } else {
        None
    }
}

fn cancellation_tool_reason(
    cancellation: &Option<AgenticCancellation>,
    call_id: &str,
) -> Option<String> {
    let cancellation = cancellation.as_ref()?;
    if cancellation.tool_call_ids.iter().any(|id| id == call_id) {
        Some(cancellation.reason.clone())
    } else {
        None
    }
}

fn command_context_string<'a>(context: &'a Value, key: &str) -> Option<&'a str> {
    context
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
}

fn render_command_template(template: &str, vars: &[(&str, String)]) -> String {
    vars.iter().fold(template.to_string(), |acc, (key, value)| {
        acc.replace(&format!("{{{{{key}}}}}"), value)
    })
}

fn split_prompt_template_args(args: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for ch in args.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if let Some(active_quote) = quote {
            if ch == active_quote {
                quote = None;
            } else {
                current.push(ch);
            }
            continue;
        }
        if ch == '"' || ch == '\'' {
            quote = Some(ch);
            continue;
        }
        if ch.is_whitespace() {
            if !current.is_empty() {
                values.push(std::mem::take(&mut current));
            }
            continue;
        }
        current.push(ch);
    }
    if escaped {
        current.push('\\');
    }
    if !current.is_empty() {
        values.push(current);
    }
    values
}

fn prompt_template_arg_slice(args: &[String], start: usize, len: Option<usize>) -> String {
    if start == 0 || start > args.len() {
        return String::new();
    }
    let iter = args.iter().skip(start - 1);
    match len {
        Some(len) => iter.take(len).cloned().collect::<Vec<_>>().join(" "),
        None => iter.cloned().collect::<Vec<_>>().join(" "),
    }
}

fn render_prompt_template_body(template: &str, args: &[String]) -> String {
    let all_args = args.join(" ");
    let mut rendered = String::new();
    let chars = template.chars().collect::<Vec<_>>();
    let mut index = 0;
    while index < chars.len() {
        if chars[index] != '$' {
            rendered.push(chars[index]);
            index += 1;
            continue;
        }

        if chars.get(index + 1) == Some(&'@') {
            rendered.push_str(&all_args);
            index += 2;
            continue;
        }
        if chars
            .get(index + 1..index + 10)
            .map(|slice| slice.iter().collect::<String>() == "ARGUMENTS")
            .unwrap_or(false)
        {
            rendered.push_str(&all_args);
            index += 10;
            continue;
        }
        if chars.get(index + 1) == Some(&'{') {
            let mut end = index + 2;
            while end < chars.len() && chars[end] != '}' {
                end += 1;
            }
            if end < chars.len() {
                let inner = chars[index + 2..end].iter().collect::<String>();
                if let Some(rest) = inner.strip_prefix("@:") {
                    let parts = rest.split(':').collect::<Vec<_>>();
                    if let Some(start) = parts.first().and_then(|value| value.parse().ok()) {
                        let len = parts.get(1).and_then(|value| value.parse().ok());
                        rendered.push_str(&prompt_template_arg_slice(args, start, len));
                        index = end + 1;
                        continue;
                    }
                }
            }
        }
        if chars
            .get(index + 1)
            .map(|ch| ch.is_ascii_digit())
            .unwrap_or(false)
        {
            let mut end = index + 1;
            while end < chars.len() && chars[end].is_ascii_digit() {
                end += 1;
            }
            let value = chars[index + 1..end].iter().collect::<String>();
            if let Ok(position) = value.parse::<usize>() {
                if position > 0 && position <= args.len() {
                    rendered.push_str(&args[position - 1]);
                }
                index = end;
                continue;
            }
        }

        rendered.push('$');
        index += 1;
    }
    rendered
}

fn command_variant_append(params: &CommandPrepareParams, _surface: &str) -> String {
    params
        .prompt_variant_append
        .as_deref()
        .filter(|append| !append.trim().is_empty())
        .map(|append| format!("\n\n{append}"))
        .unwrap_or_default()
}

fn normalize_runtime_command(command: &str) -> String {
    command
        .trim()
        .trim_start_matches('/')
        .to_ascii_lowercase()
        .replace('_', "-")
}

fn prepare_independent_command(
    params: &CommandPrepareParams,
) -> Result<CommandPrepareResult, RuntimeError> {
    let args = params.args.trim();
    if args.is_empty() {
        return Err(RuntimeError::new(
            "command_prepare_missing_args",
            RuntimeErrorCategory::InvalidRequest,
            "/independent requires a plan path or scope, for example /independent @plan.md",
        ));
    }
    let input = format!(
        "Use the independent skill to execute the referenced plan document(s) to completion.\n\nPlan document / scope:\n{args}\n\nOperating mode:\n- First load and follow the full `independent` skill instructions.\n- Read the referenced plan document(s) completely. If an item starts with `@`, resolve it as a file path from the current working directory unless the environment says otherwise.\n- Create and maintain a `todo_write` execution plan.\n- Do not stop after planning; continue through implementation, docs, validation, and final reporting.\n- Ask clarification questions only when genuinely blocked by a product decision, secret/account access, destructive operation, production deploy/publish, or irreversible architectural choice. Use the structured question tool if available.\n- Choose reasonable defaults and keep working when details are underspecified.\n- Run relevant validation before marking work complete.\n- Commit only if the user request or project instructions allow it; never publish or deploy unless explicitly requested.\n\nBegin now.{}",
        command_variant_append(params, "independent-user-prompt-append.md")
    );
    Ok(CommandPrepareResult {
        command: "independent".to_string(),
        input,
        system_prompt_profile: None,
        notes: vec![
            "Host UI may still own @path autocomplete; Rust owns the generated user prompt."
                .to_string(),
        ],
    })
}

fn prepare_init_command(params: &CommandPrepareParams) -> CommandPrepareResult {
    let mut input = INIT_USER_PROMPT.trim().to_string();
    if let Some(existing) = command_context_string(&params.context, "existingAgentsMd") {
        let truncated = truncate_chars(existing, MAX_EXISTING_AGENTS_CHARS);
        input = format!(
            "{input}\n\nAGENTS.md already exists, so refresh it instead of skipping.\n\nClaude Code-style refresh workflow:\n\n1. Inspect the repository as needed.\n2. Draft a fresh AGENTS.md in your working context using the current repository state.\n3. Compare that fresh draft against the existing AGENTS.md below.\n4. Validate old instructions before preserving them; remove or revise stale details.\n5. If important guidance is missing or stale, update AGENTS.md with a concise merged version.\n6. If the existing file is already accurate, say so and leave it unchanged.\n7. End with the required completion status: Result, Added, Changed, Removed, and Validation.\n\nExisting AGENTS.md:\n\n```markdown\n{truncated}\n```"
        );
    }
    input.push_str(&command_variant_append(
        params,
        "init-user-prompt-append.md",
    ));
    CommandPrepareResult {
        command: "init".to_string(),
        input,
        system_prompt_profile: None,
        notes: vec![
            "Host may pass context.existingAgentsMd when refreshing an existing AGENTS.md."
                .to_string(),
        ],
    }
}

fn prepare_feedback_command(params: &CommandPrepareParams, kind: &str) -> CommandPrepareResult {
    let command_name = if kind == "bug-report" {
        "/bug-report"
    } else {
        "/feature-request"
    };
    let required = if kind == "bug-report" {
        "summary, what happened, expected behavior, reproduction/context, and whether diagnostics/logs may be included"
    } else {
        "summary, requested behavior, why it matters, example workflow or acceptance criteria, and whether diagnostics may be included"
    };
    let starter = if params.args.trim().is_empty() {
        format!("The user started {command_name} without a description.")
    } else {
        format!(
            "The user started {command_name} with this description:\n\n{}",
            params.args.trim()
        )
    };
    CommandPrepareResult {
        command: kind.to_string(),
        input: format!(
            "{starter}\n\nYour job: help create a high-quality OPPi {}.\n\nVerify enough context before submitting. Required context: {required}. If anything important is missing, ask concise follow-up questions first. Once you have enough context, call the `oppi_feedback_submit` tool with structured fields. Prefer includeDiagnostics=true. Only set includeLogs=true if the user agrees or logs are clearly needed. Do not include secrets or raw private conversation history.{}",
            if kind == "bug-report" {
                "bug report"
            } else {
                "feature request"
            },
            command_variant_append(params, "feedback-triage-user-prompt-append.md")
        ),
        system_prompt_profile: None,
        notes: vec![
            "Feedback submission itself is handled by the oppi_feedback_submit tool.".to_string(),
        ],
    }
}

fn prepare_review_command(params: &CommandPrepareParams) -> CommandPrepareResult {
    let mode = command_context_string(&params.context, "mode")
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    let args = params.args.trim();
    if mode == "audit"
        || args.eq_ignore_ascii_case("audit")
        || args.to_ascii_lowercase().starts_with("audit ")
    {
        let focus = command_context_string(&params.context, "focus")
            .map(str::to_string)
            .or_else(|| {
                args.strip_prefix("audit")
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
            });
        let input = if let Some(focus) = focus {
            format!(
                "Run the full OPPi codebase audit workflow with this additional user focus:\n\n{focus}\n\nKeep the mandatory `.temp-audit` markdown queue structure, dependency freshness checkpoint, current best-practice baseline, approval gates, and batch fix loop."
            )
        } else {
            "Run the full OPPi codebase audit workflow for this repository.\n\nFirst create or update `.temp-audit/INDEX.md`, `.temp-audit/00-inventory-and-dependencies.md`, and the six category audit files. Catalogue packages, APIs, entrypoints, commands/tools/extensions, dependencies, scripts, runtimes, and current best practices. Research latest stable dependency/tooling versions, then pause with the inventory and discrepancy table before applying updates.\n\nAfter I approve dependency/tooling decisions, continue through the markdown-backed queues for bugs/correctness, duplication/maintainability, reliability/ops/cross-platform, security/data safety, test gaps/regressions, and docs/prompt/catalog drift. Fix approved issues in focused batches and keep the `.temp-audit` files as the detailed source of truth.".to_string()
        };
        return CommandPrepareResult {
            command: "review".to_string(),
            input,
            system_prompt_profile: Some("audit".to_string()),
            notes: vec![
                "Host should append the audit system-prompt profile before the agent turn."
                    .to_string(),
            ],
        };
    }

    let input = if !args.is_empty() && mode.is_empty() {
        args.to_string()
    } else if mode == "base-branch"
        || command_context_string(&params.context, "baseBranch").is_some()
    {
        let branch =
            command_context_string(&params.context, "baseBranch").unwrap_or("<base_branch>");
        if let Some(merge_base) = command_context_string(&params.context, "mergeBaseSha") {
            render_command_template(
                "Review the code changes against the base branch '{{base_branch}}'. The merge base commit for this comparison is {{merge_base_sha}}. Run `git diff {{merge_base_sha}}` to inspect the changes relative to {{base_branch}}. Provide prioritized, actionable findings.",
                &[
                    ("base_branch", branch.to_string()),
                    ("merge_base_sha", merge_base.to_string()),
                ],
            )
        } else {
            render_command_template(
                "Review the code changes against the base branch '{{branch}}'. Start by finding the merge diff between the current branch and {{branch}}'s upstream e.g. (`git merge-base HEAD \"$(git rev-parse --abbrev-ref \"{{branch}}@{upstream}\")\"`), then run `git diff` against that SHA to see what changes we would merge into the {{branch}} branch. Provide prioritized, actionable findings.",
                &[("branch", branch.to_string())],
            )
        }
    } else if mode == "commit" || command_context_string(&params.context, "sha").is_some() {
        let sha = command_context_string(&params.context, "sha").unwrap_or("<sha>");
        if let Some(title) = command_context_string(&params.context, "title") {
            render_command_template(
                "Review the code changes introduced by commit {{sha}} (\"{{title}}\"). Provide prioritized, actionable findings.",
                &[("sha", sha.to_string()), ("title", title.to_string())],
            )
        } else {
            render_command_template(
                "Review the code changes introduced by commit {{sha}}. Provide prioritized, actionable findings.",
                &[("sha", sha.to_string())],
            )
        }
    } else {
        "Review the current code changes (staged, unstaged, and untracked files) and provide prioritized findings.".to_string()
    };

    CommandPrepareResult {
        command: "review".to_string(),
        input,
        system_prompt_profile: Some("review".to_string()),
        notes: vec![
            "Host should append the review system-prompt profile before the agent turn."
                .to_string(),
        ],
    }
}

fn prepare_runtime_command(
    params: CommandPrepareParams,
) -> Result<CommandPrepareResult, RuntimeError> {
    match normalize_runtime_command(&params.command).as_str() {
        "independent" => prepare_independent_command(&params),
        "init" => Ok(prepare_init_command(&params)),
        "bug-report" => Ok(prepare_feedback_command(&params, "bug-report")),
        "feature-request" => Ok(prepare_feedback_command(&params, "feature-request")),
        "review" | "audit" => Ok(prepare_review_command(&CommandPrepareParams {
            thread_id: params.thread_id.clone(),
            command: "review".to_string(),
            args: if normalize_runtime_command(&params.command) == "audit"
                && !params.args.trim_start().starts_with("audit")
            {
                format!("audit {}", params.args).trim().to_string()
            } else {
                params.args.clone()
            },
            context: params.context.clone(),
            prompt_variant_append: params.prompt_variant_append.clone(),
        })),
        other => Err(RuntimeError::new(
            "command_prepare_unknown",
            RuntimeErrorCategory::InvalidRequest,
            format!("command/prepare does not support /{other} yet"),
        )),
    }
}

fn model_registry_models_json_path() -> PathBuf {
    std::env::var_os("OPPI_MODELS_JSON_PATH")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| default_oppi_agent_dir().join("models.json"))
}

fn model_registry_diagnostic(
    level: DiagnosticLevel,
    message: impl Into<String>,
    metadata: impl IntoIterator<Item = (String, String)>,
) -> Diagnostic {
    Diagnostic {
        level,
        message: message.into(),
        metadata: metadata.into_iter().collect(),
    }
}

fn model_registry_providers_from_path(
    path: &Path,
) -> (Option<serde_json::Map<String, Value>>, Vec<Diagnostic>) {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return (None, Vec::new()),
        Err(error) => {
            return (
                None,
                vec![model_registry_diagnostic(
                    DiagnosticLevel::Warning,
                    format!("models.json read error at {}: {error}", path.display()),
                    [("path".to_string(), path.display().to_string())],
                )],
            );
        }
    };
    let value = match serde_json::from_str::<Value>(&raw) {
        Ok(value) => value,
        Err(error) => {
            return (
                None,
                vec![model_registry_diagnostic(
                    DiagnosticLevel::Warning,
                    format!("models.json parse error at {}: {error}", path.display()),
                    [("path".to_string(), path.display().to_string())],
                )],
            );
        }
    };
    let Some(providers) = value.get("providers").and_then(Value::as_object) else {
        return (
            None,
            vec![model_registry_diagnostic(
                DiagnosticLevel::Warning,
                format!(
                    "models.json schema error at {}: expected a providers object",
                    path.display()
                ),
                [("path".to_string(), path.display().to_string())],
            )],
        );
    };
    (Some(providers.clone()), Vec::new())
}

fn model_registry_provider_aliases(provider: &str) -> Vec<String> {
    match provider {
        "openai-compatible" | "openai" => {
            vec!["openai-compatible".to_string(), "openai".to_string()]
        }
        "openai-codex" => vec!["openai-codex".to_string()],
        "github-copilot" => vec!["github-copilot".to_string()],
        "anthropic" | "meridian" => vec!["meridian".to_string(), "anthropic".to_string()],
        other => vec![other.to_string()],
    }
}

fn model_registry_builtin_model_ids(provider: &str) -> &'static [&'static str] {
    match provider {
        "mock" => &["local-test"],
        "openai-compatible" | "openai" => OPENAI_DIRECT_MODEL_CATALOG,
        "openai-codex" => OPENAI_CODEX_MODEL_CATALOG,
        "github-copilot" => GITHUB_COPILOT_MODEL_CATALOG,
        "anthropic" | "meridian" => MERIDIAN_MODEL_CATALOG,
        _ => &[],
    }
}

fn model_registry_api_supported(api: &str) -> bool {
    matches!(api, "openai-completions")
}

fn model_registry_string_field<'a>(
    object: &'a serde_json::Map<String, Value>,
    key: &str,
) -> Option<&'a str> {
    object
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn model_registry_u64_field(object: &serde_json::Map<String, Value>, key: &str) -> Option<u64> {
    object.get(key).and_then(Value::as_u64)
}

fn model_registry_registered_auth_configured(
    provider: &str,
    providers: &serde_json::Map<String, Value>,
) -> bool {
    model_registry_provider_aliases(provider)
        .into_iter()
        .filter_map(|alias| providers.get(&alias).and_then(Value::as_object))
        .any(|provider| model_registry_string_field(provider, "apiKey").is_some())
}

fn model_registry_builtin_auth_configured(
    provider: &str,
    providers: Option<&serde_json::Map<String, Value>>,
) -> bool {
    if providers
        .is_some_and(|providers| model_registry_registered_auth_configured(provider, providers))
    {
        return true;
    }
    match provider {
        "mock" => true,
        "openai-codex" => read_codex_auth_json(&codex_auth_path())
            .ok()
            .and_then(|raw| parse_codex_auth(&raw))
            .is_some(),
        "github-copilot" => read_github_copilot_auth_json(&github_copilot_auth_path())
            .ok()
            .and_then(|raw| parse_github_copilot_auth(&raw))
            .is_some(),
        "openai-compatible" | "openai" => ["OPPI_OPENAI_API_KEY", "OPENAI_API_KEY"]
            .into_iter()
            .any(|name| {
                std::env::var(name)
                    .ok()
                    .is_some_and(|value| !value.trim().is_empty())
            }),
        _ => false,
    }
}

fn model_registry_upsert(entries: &mut Vec<ModelRef>, model: ModelRef) {
    if let Some(existing) = entries
        .iter_mut()
        .find(|existing| existing.id == model.id && existing.provider == model.provider)
    {
        *existing = model;
    } else {
        entries.push(model);
    }
}

fn model_registry_scan_diagnostics(
    path: &Path,
    providers: &serde_json::Map<String, Value>,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for (provider_name, provider_value) in providers {
        let Some(provider_object) = provider_value.as_object() else {
            diagnostics.push(model_registry_diagnostic(
                DiagnosticLevel::Warning,
                format!(
                    "models.json schema error at {}: provider {provider_name} must be an object",
                    path.display()
                ),
                [("provider".to_string(), provider_name.clone())],
            ));
            continue;
        };
        if let Some(api) = model_registry_string_field(provider_object, "api")
            && !model_registry_api_supported(api)
        {
            diagnostics.push(model_registry_diagnostic(
                DiagnosticLevel::Warning,
                format!(
                    "models.json unsupported provider API at {}: provider {provider_name} api {api} is not yet supported by Rust-native OPPi",
                    path.display()
                ),
                [
                    ("provider".to_string(), provider_name.clone()),
                    ("api".to_string(), api.to_string()),
                ],
            ));
        }
        if let Some(models) = provider_object.get("models").and_then(Value::as_array) {
            for model in models {
                let model_id = model
                    .get("id")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|id| !id.is_empty())
                    .unwrap_or("<unknown>");
                if let Some(api) = model
                    .as_object()
                    .and_then(|model| model_registry_string_field(model, "api"))
                    && !model_registry_api_supported(api)
                {
                    diagnostics.push(model_registry_diagnostic(
                        DiagnosticLevel::Warning,
                        format!(
                            "models.json unsupported provider API at {}: provider {provider_name} model {model_id} api {api} is not yet supported by Rust-native OPPi",
                            path.display()
                        ),
                        [
                            ("provider".to_string(), provider_name.clone()),
                            ("model".to_string(), model_id.to_string()),
                            ("api".to_string(), api.to_string()),
                        ],
                    ));
                }
            }
        }
    }
    diagnostics
}

fn model_registry_apply_models_json(
    provider: &str,
    path: &Path,
    providers: &serde_json::Map<String, Value>,
    entries: &mut Vec<ModelRef>,
) -> Vec<Diagnostic> {
    let mut diagnostics = model_registry_scan_diagnostics(path, providers);
    for alias in model_registry_provider_aliases(provider) {
        let Some(provider_object) = providers.get(&alias).and_then(Value::as_object) else {
            continue;
        };
        if let Some(model_overrides) = provider_object
            .get("modelOverrides")
            .and_then(Value::as_object)
        {
            for (model_id, override_value) in model_overrides {
                let Some(override_object) = override_value.as_object() else {
                    diagnostics.push(model_registry_diagnostic(
                        DiagnosticLevel::Warning,
                        format!(
                            "models.json schema error at {}: provider {alias} modelOverrides.{model_id} must be an object",
                            path.display()
                        ),
                        [
                            ("provider".to_string(), alias.clone()),
                            ("model".to_string(), model_id.clone()),
                        ],
                    ));
                    continue;
                };
                if let Some(name) = model_registry_string_field(override_object, "name")
                    && let Some(existing) = entries
                        .iter_mut()
                        .find(|model| model.provider == provider && model.id == *model_id)
                {
                    existing.display_name = name.to_string();
                }
                if let Some(context_window) =
                    model_registry_u64_field(override_object, "contextWindow")
                    && let Some(existing) = entries
                        .iter_mut()
                        .find(|model| model.provider == provider && model.id == *model_id)
                {
                    existing.context_window = Some(context_window);
                }
            }
        }
        if let Some(models_value) = provider_object.get("models") {
            let Some(models) = models_value.as_array() else {
                diagnostics.push(model_registry_diagnostic(
                    DiagnosticLevel::Warning,
                    format!(
                        "models.json schema error at {}: provider {alias} models must be an array",
                        path.display()
                    ),
                    [("provider".to_string(), alias.clone())],
                ));
                continue;
            };
            for model in models {
                let Some(model_object) = model.as_object() else {
                    diagnostics.push(model_registry_diagnostic(
                        DiagnosticLevel::Warning,
                        format!(
                            "models.json schema error at {}: provider {alias} model entries must be objects",
                            path.display()
                        ),
                        [("provider".to_string(), alias.clone())],
                    ));
                    continue;
                };
                let Some(id) = model_registry_string_field(model_object, "id") else {
                    diagnostics.push(model_registry_diagnostic(
                        DiagnosticLevel::Warning,
                        format!(
                            "models.json schema error at {}: provider {alias} model entry is missing id",
                            path.display()
                        ),
                        [("provider".to_string(), alias.clone())],
                    ));
                    continue;
                };
                let display_name = model_registry_string_field(model_object, "name").unwrap_or(id);
                model_registry_upsert(
                    entries,
                    ModelRef {
                        id: id.to_string(),
                        provider: provider.to_string(),
                        display_name: display_name.to_string(),
                        context_window: model_registry_u64_field(model_object, "contextWindow"),
                        role: None,
                    },
                );
            }
        }
    }
    diagnostics
}

fn model_registry_wildcard_match(pattern: &str, value: &str) -> bool {
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

fn model_registry_matches_scope(provider: &str, model_id: &str, pattern: &str) -> bool {
    let pattern = pattern.trim();
    if pattern.is_empty() {
        return false;
    }
    let model_lower = model_id.to_ascii_lowercase();
    let pattern_lower = pattern.to_ascii_lowercase();
    let provider_model = format!("{}/{}", provider.to_ascii_lowercase(), model_lower);
    if pattern_lower.contains('*') || pattern_lower.contains('?') {
        model_registry_wildcard_match(&pattern_lower, &model_lower)
            || model_registry_wildcard_match(&pattern_lower, &provider_model)
    } else {
        pattern_lower == model_lower || pattern_lower == provider_model
    }
}

fn model_registry_order_models(
    provider: &str,
    entries: &[ModelRef],
    selected_model_id: Option<&str>,
    scope: &[String],
) -> Vec<ModelRef> {
    let mut seen = BTreeSet::<String>::new();
    let mut out = Vec::new();
    let mut push = |model: &ModelRef| {
        let key = format!("{}/{}", model.provider, model.id);
        if seen.insert(key) {
            out.push(model.clone());
        }
    };
    if let Some(selected) = selected_model_id
        && let Some(model) = entries.iter().find(|model| model.id == selected)
    {
        push(model);
    }
    if scope.is_empty() {
        for model in entries {
            push(model);
        }
        return out;
    }
    let mut matched_scope = false;
    for pattern in scope {
        for model in entries {
            if model_registry_matches_scope(provider, &model.id, pattern) {
                matched_scope = true;
                push(model);
            }
        }
    }
    if !matched_scope {
        for model in entries {
            push(model);
        }
    }
    out
}
