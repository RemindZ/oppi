//! Slash-agent definition primitives for future AgentTool dispatch.

pub use oppi_protocol::{
    AgentDefinition, AgentSource, FailedAgentFile, PermissionMode, ResolvedAgent,
};
use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
};

pub const MAX_AGENT_FILES: usize = 800;
pub const MAX_AGENT_DEPTH: usize = 5;

#[derive(Debug, Clone)]
pub struct AgentIndexOptions {
    pub project_root: PathBuf,
    pub home_dir: PathBuf,
    pub agent_dir: PathBuf,
    pub built_ins: Vec<AgentDefinition>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentIndex {
    pub items: Vec<ResolvedAgent>,
    pub failed: Vec<FailedAgentFile>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentDirectory {
    pub root: PathBuf,
    pub source: AgentSource,
    pub read_only: bool,
}

pub fn built_in_agent_definitions() -> Vec<AgentDefinition> {
    vec![
        built_in(
            "general-purpose",
            "Use for broad codebase research, multi-step investigation, uncertain searches, and general delegated work that does not fit a narrower personality.",
            vec!["*"],
            None,
            Some("coding"),
            false,
            "You are OPPi's general-purpose coding subagent. Complete the delegated task thoroughly, search broadly when needed, stay within the requested scope, avoid unnecessary files or documentation, and report concise results with any blockers or changed files.",
        ),
        built_in(
            "Explore",
            "Use for read-only codebase exploration: finding files, symbols, patterns, relevant implementation areas, and repository structure before edits begin.",
            vec!["read", "grep", "find", "ls", "shell_exec"],
            Some(PermissionMode::ReadOnly),
            Some("coding"),
            false,
            "You are OPPi's read-only exploration specialist. Use fast search and safe shell diagnostics to locate relevant code and explain what you found. Do not create, edit, move, delete, or write project files. Return a concise discovery report with important paths and confidence notes.",
        ),
        built_in(
            "Plan",
            "Use for read-only implementation planning: break down a feature or fix, identify critical files, call out trade-offs, and produce concrete steps before coding.",
            vec!["read", "grep", "find", "ls", "shell_exec"],
            Some(PermissionMode::ReadOnly),
            Some("strong"),
            false,
            "You are OPPi's planning architect. Inspect the repository read-only, understand the requested outcome, design a practical implementation plan, list risks and trade-offs, and finish with the most important files to modify or inspect. Do not modify project files.",
        ),
        built_in(
            "oppi-code-guide",
            "Use when the user asks how OPPi, Pi, OPPi extensions, commands, skills, themes, permissions, memory, or the runtime spine work.",
            vec!["read", "grep", "find", "ls", "shell_exec"],
            Some(PermissionMode::ReadOnly),
            Some("coding"),
            false,
            "You are OPPi's product and documentation guide. Prefer the local OPPi repository, installed Pi documentation, and official project docs. Inspect local configuration read-only when it helps. Give direct, grounded guidance and call out when a feature belongs to a future runtime stage.",
        ),
        built_in(
            "statusline-setup",
            "Use when the user wants to configure OPPi/Pi terminal status, footer, or prompt-like display behavior.",
            vec!["read", "edit"],
            Some(PermissionMode::Default),
            Some("strong"),
            false,
            "You are OPPi's status and footer setup specialist. Inspect existing settings and terminal configuration, preserve unrelated settings, prefer minimal reversible edits, and explain exactly what changed. Ask for missing prompt/status details instead of guessing.",
        ),
        built_in(
            "verification",
            "Use before reporting completion for non-trivial implementation, multi-file changes, backend/API/infrastructure work, or risky changes that need independent evidence.",
            vec!["read", "grep", "find", "ls", "shell_exec"],
            Some(PermissionMode::ReadOnly),
            Some("strong"),
            true,
            "You are OPPi's adversarial verification specialist. Verify rather than modify. Read instructions, inspect changed files, run relevant checks when available, perform at least one adversarial probe before passing, and report each check with evidence. End with exactly one terminal verdict: VERDICT: pass, VERDICT: fail, or VERDICT: partial.",
        ),
    ]
}

fn built_in(
    name: &str,
    description: &str,
    tools: Vec<&str>,
    permission_mode: Option<PermissionMode>,
    model: Option<&str>,
    background: bool,
    instructions: &str,
) -> AgentDefinition {
    AgentDefinition {
        name: name.to_string(),
        description: description.to_string(),
        source: Some(AgentSource::BuiltIn),
        tools: tools.into_iter().map(str::to_string).collect(),
        tool_denylist: Vec::new(),
        model: model.map(str::to_string),
        effort: None,
        permission_mode,
        memory_mode: None,
        background,
        worktree_root: None,
        isolation: None,
        color: None,
        skills: Vec::new(),
        max_turns: None,
        path: None,
        filename: None,
        read_only: true,
        instructions: instructions.to_string(),
    }
}

pub fn resolve_active_agents(definitions: Vec<AgentDefinition>) -> Vec<ResolvedAgent> {
    let mut by_name: BTreeMap<String, Vec<AgentDefinition>> = BTreeMap::new();
    for definition in definitions {
        by_name
            .entry(definition.name.clone())
            .or_default()
            .push(definition);
    }

    by_name
        .into_values()
        .map(|mut definitions| {
            definitions.sort_by_key(|definition| source_rank(definition.source));
            definitions.reverse();
            let active = definitions.remove(0);
            ResolvedAgent {
                active,
                shadowed: definitions,
            }
        })
        .collect()
}

pub fn agent_directories(
    project_root: &Path,
    home_dir: &Path,
    agent_dir: &Path,
) -> Vec<AgentDirectory> {
    vec![
        AgentDirectory {
            root: home_dir.join(".claude").join("agents"),
            source: AgentSource::ClaudeUser,
            read_only: true,
        },
        AgentDirectory {
            root: agent_dir.join("oppi").join("agents"),
            source: AgentSource::User,
            read_only: false,
        },
        AgentDirectory {
            root: project_root.join(".claude").join("agents"),
            source: AgentSource::ClaudeProject,
            read_only: true,
        },
        AgentDirectory {
            root: project_root.join(".oppi").join("agents"),
            source: AgentSource::Project,
            read_only: false,
        },
    ]
}

pub fn load_agent_index(options: AgentIndexOptions) -> io::Result<AgentIndex> {
    let mut definitions = options.built_ins;
    let mut failed = Vec::new();
    let mut files_seen = 0usize;

    for directory in agent_directories(&options.project_root, &options.home_dir, &options.agent_dir)
    {
        scan_agent_directory(
            &directory,
            &directory.root,
            0,
            &mut files_seen,
            &mut definitions,
            &mut failed,
        )?;
        if files_seen >= MAX_AGENT_FILES {
            break;
        }
    }

    Ok(AgentIndex {
        items: resolve_active_agents(definitions),
        failed,
    })
}

pub fn parse_agent_markdown(
    markdown: &str,
    source: AgentSource,
    path: Option<String>,
    filename: Option<String>,
    read_only: bool,
) -> Result<AgentDefinition, String> {
    let (frontmatter, body) = split_agent_frontmatter(markdown)
        .ok_or_else(|| "agent markdown requires YAML frontmatter".to_string())?;
    let fields = parse_agent_frontmatter(frontmatter);
    let name = required_agent_field(&fields, "name")?;
    let description = required_agent_field(&fields, "description")?;
    let instructions = body.trim().to_string();
    if instructions.is_empty() {
        return Err("agent markdown requires instruction body".to_string());
    }

    Ok(AgentDefinition {
        name,
        description,
        source: Some(source),
        tools: parse_agent_list_field(fields.get("tools").map(String::as_str)),
        tool_denylist: parse_agent_list_field(
            fields
                .get("disallowedTools")
                .or_else(|| fields.get("disallowed_tools"))
                .map(String::as_str),
        ),
        model: optional_agent_field(&fields, "model"),
        effort: optional_agent_field(&fields, "effort"),
        permission_mode: optional_agent_field(&fields, "permissionMode")
            .or_else(|| optional_agent_field(&fields, "permission_mode"))
            .map(|value| parse_agent_permission_mode(&value))
            .transpose()?,
        memory_mode: optional_agent_field(&fields, "memory")
            .or_else(|| optional_agent_field(&fields, "memoryMode"))
            .or_else(|| optional_agent_field(&fields, "memory_mode")),
        background: optional_agent_field(&fields, "background").is_some_and(|value| {
            matches!(
                value.to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        }),
        worktree_root: optional_agent_field(&fields, "worktreeRoot")
            .or_else(|| optional_agent_field(&fields, "worktree_root")),
        isolation: optional_agent_field(&fields, "isolation"),
        color: optional_agent_field(&fields, "color"),
        skills: parse_agent_list_field(fields.get("skills").map(String::as_str)),
        max_turns: optional_agent_field(&fields, "maxTurns")
            .or_else(|| optional_agent_field(&fields, "max_turns"))
            .map(|value| {
                value
                    .parse::<u32>()
                    .map_err(|_| format!("invalid maxTurns value: {value}"))
            })
            .transpose()?,
        path,
        filename,
        read_only,
        instructions,
    })
}

pub fn format_agent_markdown(definition: &AgentDefinition) -> String {
    let mut lines = vec![
        "---".to_string(),
        format!("name: {}", definition.name),
        format!("description: {}", quote_agent_yaml(&definition.description)),
    ];
    if !definition.tools.is_empty() {
        lines.push(format!("tools: {}", definition.tools.join(", ")));
    }
    if !definition.tool_denylist.is_empty() {
        lines.push(format!(
            "disallowedTools: {}",
            definition.tool_denylist.join(", ")
        ));
    }
    if let Some(model) = definition.model.as_deref() {
        lines.push(format!("model: {model}"));
    }
    if let Some(effort) = definition.effort.as_deref() {
        lines.push(format!("effort: {effort}"));
    }
    if let Some(mode) = definition.permission_mode {
        lines.push(format!("permissionMode: {}", mode.as_str()));
    }
    if let Some(memory) = definition.memory_mode.as_deref() {
        lines.push(format!("memory: {memory}"));
    }
    if definition.background {
        lines.push("background: true".to_string());
    }
    if let Some(root) = definition.worktree_root.as_deref() {
        lines.push(format!("worktreeRoot: {root}"));
    }
    if let Some(isolation) = definition.isolation.as_deref() {
        lines.push(format!("isolation: {isolation}"));
    }
    if let Some(color) = definition.color.as_deref() {
        lines.push(format!("color: {color}"));
    }
    if !definition.skills.is_empty() {
        lines.push(format!("skills: {}", definition.skills.join(", ")));
    }
    if let Some(max_turns) = definition.max_turns {
        lines.push(format!("maxTurns: {max_turns}"));
    }
    lines.push("---".to_string());
    lines.push(String::new());
    lines.push(definition.instructions.trim().to_string());
    lines.push(String::new());
    lines.join("\n")
}

fn source_rank(source: Option<AgentSource>) -> u8 {
    match source.unwrap_or(AgentSource::User) {
        AgentSource::BuiltIn => 0,
        AgentSource::ClaudeUser => 1,
        AgentSource::User => 2,
        AgentSource::ClaudeProject => 3,
        AgentSource::Project => 4,
        AgentSource::Plugin => 5,
        AgentSource::Cli => 6,
        AgentSource::ManagedPolicy => 7,
    }
}

fn scan_agent_directory(
    directory: &AgentDirectory,
    current: &Path,
    depth: usize,
    files_seen: &mut usize,
    definitions: &mut Vec<AgentDefinition>,
    failed: &mut Vec<FailedAgentFile>,
) -> io::Result<()> {
    if *files_seen >= MAX_AGENT_FILES || !current.exists() {
        return Ok(());
    }
    if depth > MAX_AGENT_DEPTH {
        return Ok(());
    }

    let mut entries = fs::read_dir(current)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.path());

    for entry in entries {
        if *files_seen >= MAX_AGENT_FILES {
            break;
        }
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            if depth < MAX_AGENT_DEPTH && should_scan_agent_subdirectory(&path) {
                scan_agent_directory(directory, &path, depth + 1, files_seen, definitions, failed)?;
            }
            continue;
        }

        if !file_type.is_file() || path.extension().and_then(|ext| ext.to_str()) != Some("md") {
            continue;
        }

        *files_seen += 1;
        let path_string = path.display().to_string();
        let filename = path
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_string);
        match fs::read_to_string(&path) {
            Ok(contents) => match parse_agent_markdown(
                &contents,
                directory.source,
                Some(path_string.clone()),
                filename,
                directory.read_only,
            ) {
                Ok(definition) => definitions.push(definition),
                Err(message) => failed.push(FailedAgentFile {
                    path: path_string,
                    source: directory.source,
                    message,
                }),
            },
            Err(error) => failed.push(FailedAgentFile {
                path: path_string,
                source: directory.source,
                message: format!("failed to read agent markdown: {error}"),
            }),
        }
    }

    Ok(())
}

fn should_scan_agent_subdirectory(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    if matches!(name, ".git" | "node_modules" | "target") {
        return false;
    }
    !name.starts_with('.')
}

fn split_agent_frontmatter(text: &str) -> Option<(&str, &str)> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text).trim_start();
    let rest = text.strip_prefix("---")?;
    let rest = rest
        .strip_prefix("\r\n")
        .or_else(|| rest.strip_prefix('\n'))
        .unwrap_or(rest);
    let marker = rest.find("\n---").or_else(|| rest.find("\r\n---"))?;
    let (frontmatter, after) = rest.split_at(marker);
    let body = after
        .trim_start_matches("\r\n")
        .trim_start_matches('\n')
        .trim_start_matches("---")
        .trim_start_matches("\r\n")
        .trim_start_matches('\n');
    Some((frontmatter, body))
}

fn parse_agent_frontmatter(frontmatter: &str) -> BTreeMap<String, String> {
    let mut fields = BTreeMap::new();
    for line in frontmatter.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once(':') {
            fields.insert(
                key.trim().to_string(),
                strip_agent_quotes(value.trim()).to_string(),
            );
        }
    }
    fields
}

fn required_agent_field(fields: &BTreeMap<String, String>, key: &str) -> Result<String, String> {
    optional_agent_field(fields, key).ok_or_else(|| format!("agent markdown requires {key}"))
}

fn optional_agent_field(fields: &BTreeMap<String, String>, key: &str) -> Option<String> {
    fields
        .get(key)
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn parse_agent_list_field(value: Option<&str>) -> Vec<String> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Vec::new();
    };
    let value = value
        .strip_prefix('[')
        .and_then(|inner| inner.strip_suffix(']'))
        .unwrap_or(value);
    value
        .split(',')
        .map(|item| strip_agent_quotes(item.trim()).trim().to_string())
        .filter(|item| !item.is_empty())
        .collect()
}

fn parse_agent_permission_mode(value: &str) -> Result<PermissionMode, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "read-only" | "readonly" | "ro" => Ok(PermissionMode::ReadOnly),
        "default" => Ok(PermissionMode::Default),
        "auto-review" | "autoreview" | "auto" => Ok(PermissionMode::AutoReview),
        "full-access" | "full" | "danger" => Ok(PermissionMode::FullAccess),
        other => Err(format!("unknown agent permissionMode: {other}")),
    }
}

fn strip_agent_quotes(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .or_else(|| {
            value
                .strip_prefix('\'')
                .and_then(|value| value.strip_suffix('\''))
        })
        .unwrap_or(value)
}

fn quote_agent_yaml(value: &str) -> String {
    format!(
        "\"{}\"",
        value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agent(name: &str, source: AgentSource, instructions: &str) -> AgentDefinition {
        AgentDefinition {
            name: name.to_string(),
            description: "agent".to_string(),
            source: Some(source),
            tools: Vec::new(),
            tool_denylist: Vec::new(),
            model: None,
            effort: None,
            permission_mode: None,
            memory_mode: None,
            background: false,
            worktree_root: None,
            isolation: None,
            color: None,
            skills: Vec::new(),
            max_turns: None,
            path: None,
            filename: None,
            read_only: false,
            instructions: instructions.to_string(),
        }
    }

    #[test]
    fn built_ins_include_core_personalities() {
        let built_ins = built_in_agent_definitions();
        assert!(
            built_ins
                .iter()
                .any(|agent| agent.name == "general-purpose")
        );
        assert!(built_ins.iter().any(|agent| agent.name == "Explore"));
        assert!(built_ins.iter().any(|agent| agent.name == "Plan"));
        assert!(
            built_ins
                .iter()
                .any(|agent| agent.name == "verification" && agent.background)
        );
    }

    #[test]
    fn resolves_highest_priority_agent_and_keeps_shadowed_definitions() {
        let resolved = resolve_active_agents(vec![
            agent("reviewer", AgentSource::BuiltIn, "built-in"),
            agent("reviewer", AgentSource::Project, "project"),
        ]);
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].active.instructions, "project");
        assert_eq!(resolved[0].shadowed.len(), 1);
    }

    #[test]
    fn loads_project_user_and_claude_agents_with_priority_and_failed_files() {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("project");
        let home = temp.path().join("home");
        let agent_home = temp.path().join("oppi-home");
        std::fs::create_dir_all(project.join(".oppi/agents")).unwrap();
        std::fs::create_dir_all(project.join(".claude/agents")).unwrap();
        std::fs::create_dir_all(agent_home.join("oppi/agents")).unwrap();
        std::fs::create_dir_all(home.join(".claude/agents")).unwrap();

        std::fs::write(
            project.join(".claude/agents/reviewer.md"),
            "---\nname: reviewer\ndescription: Claude project reviewer\n---\nRead-only prompt.\n",
        )
        .unwrap();
        std::fs::write(
            project.join(".oppi/agents/reviewer.md"),
            "---\nname: reviewer\ndescription: Project reviewer\ntools: read, shell_exec\ndisallowedTools: shell_exec\nbackground: true\n---\nProject prompt.\n",
        )
        .unwrap();
        std::fs::write(
            project.join(".oppi/agents/bad.md"),
            "---\ndescription: Missing name\n---\nBad prompt.\n",
        )
        .unwrap();

        let index = load_agent_index(AgentIndexOptions {
            project_root: project.clone(),
            home_dir: home.clone(),
            agent_dir: agent_home.clone(),
            built_ins: Vec::new(),
        })
        .unwrap();

        assert_eq!(index.failed.len(), 1);
        assert_eq!(index.failed[0].source, AgentSource::Project);
        let reviewer = index
            .items
            .iter()
            .find(|item| item.active.name == "reviewer")
            .unwrap();
        assert_eq!(reviewer.active.source, Some(AgentSource::Project));
        assert_eq!(reviewer.active.tool_denylist, vec!["shell_exec"]);
        assert!(reviewer.active.background);
        assert_eq!(
            reviewer.shadowed[0].source,
            Some(AgentSource::ClaudeProject)
        );
        assert!(reviewer.shadowed[0].read_only);
    }
}
