//! Slash-agent definition primitives for future AgentTool dispatch.

pub use oppi_protocol::{AgentDefinition, AgentSource, PermissionMode, ResolvedAgent};
use std::collections::BTreeMap;

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
        model: model.map(str::to_string),
        effort: None,
        permission_mode,
        background,
        worktree_root: None,
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

fn source_rank(source: Option<AgentSource>) -> u8 {
    match source.unwrap_or(AgentSource::User) {
        AgentSource::BuiltIn => 0,
        AgentSource::Plugin => 1,
        AgentSource::User => 2,
        AgentSource::Project => 3,
        AgentSource::Cli => 4,
        AgentSource::ManagedPolicy => 5,
    }
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
            model: None,
            effort: None,
            permission_mode: None,
            background: false,
            worktree_root: None,
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
}
