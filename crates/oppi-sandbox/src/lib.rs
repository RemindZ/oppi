//! Sandbox policy primitives.
//!
//! This crate is the Rust-owned security-boundary planning layer for OPPi. It
//! resolves high-level permission profiles into concrete sandbox execution
//! plans, enforces conservative preflight policy checks, and fails closed when a
//! caller requires OS sandboxing that is not available yet. OS-specific adapters
//! will attach below this API; adapter mistakes should not be able to bypass the
//! policy decisions made here.

use oppi_protocol::{
    AdditionalPermissionProfile, ConcreteSandboxPolicy, Diagnostic, DiagnosticLevel,
    FilesystemAccess, FilesystemPolicy, FilesystemRoot, FilesystemRule, ManagedNetworkConfig,
    NetworkPolicy, PermissionMode, PermissionProfile, RiskLevel, SandboxAuditRecord,
    SandboxEnforcement, SandboxExecParams, SandboxExecPlan, SandboxExecRequest, SandboxExecResult,
    SandboxPlanResult, SandboxPolicy, SandboxPolicyDecision, SandboxPreference, SandboxStatus,
    SandboxType, SandboxUserConfig, ToolPermissionManifest,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(windows)]
mod windows_process;
#[cfg(windows)]
mod windows_restricted;
#[cfg(windows)]
mod windows_wfp;

#[cfg(windows)]
pub use windows_restricted::{
    WindowsRestrictedToken, WindowsRestrictedTokenStatus,
    create_restricted_low_integrity_primary_token,
    create_restricted_low_integrity_primary_token_for_credentials,
    create_restricted_low_integrity_primary_token_from, windows_restricted_token_status,
};
#[cfg(windows)]
pub use windows_wfp::{
    WindowsWfpStatus, install_windows_wfp_filters_for_account, windows_wfp_filter_count,
    windows_wfp_status,
};

#[cfg(not(windows))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsRestrictedTokenStatus {
    pub available: bool,
    pub low_integrity_supported: bool,
    pub message: String,
}

#[cfg(not(windows))]
pub fn windows_restricted_token_status() -> WindowsRestrictedTokenStatus {
    WindowsRestrictedTokenStatus {
        available: false,
        low_integrity_supported: false,
        message: "Windows restricted tokens are only available on Windows".to_string(),
    }
}

#[cfg(not(windows))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsRestrictedToken;

#[cfg(not(windows))]
pub fn create_restricted_low_integrity_primary_token() -> Result<WindowsRestrictedToken, String> {
    Err("Windows restricted tokens are only available on Windows".to_string())
}

#[cfg(not(windows))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsWfpStatus {
    pub available: bool,
    pub filter_count: usize,
    pub message: String,
}

#[cfg(not(windows))]
pub fn windows_wfp_status() -> WindowsWfpStatus {
    WindowsWfpStatus {
        available: false,
        filter_count: 0,
        message: "Windows Filtering Platform is only available on Windows".to_string(),
    }
}

#[cfg(not(windows))]
pub fn windows_wfp_filter_count() -> usize {
    0
}

#[cfg(not(windows))]
pub fn install_windows_wfp_filters_for_account(_account: &str) -> Result<usize, String> {
    Err("Windows Filtering Platform is only available on Windows".to_string())
}

const PROTECTED_WORKSPACE_METADATA_DIRS: &[&str] = &[".git", ".agents", ".codex"];

const DEFAULT_PROTECTED_PATTERNS: &[&str] = &[
    ".env*",
    ".ssh/",
    "*.pem",
    "*.key",
    ".git/",
    ".agents/",
    ".codex/",
    ".git/config",
    ".git/hooks/",
    ".npmrc",
    ".pypirc",
    ".mcp.json",
    ".claude.json",
    ".oppi/auth*",
    ".oppi/token*",
    ".oppi/credentials*",
    ".oppi/memory/secrets*",
];

pub type ExecRequest = SandboxExecRequest;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyDecision {
    Allow,
    Ask { risk: RiskLevel, reason: String },
    Deny { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxTransform {
    pub decision: PolicyDecision,
    pub plan: Option<SandboxExecPlan>,
}

impl From<SandboxTransform> for SandboxPlanResult {
    fn from(value: SandboxTransform) -> Self {
        Self {
            decision: value.decision.into(),
            plan: value.plan,
        }
    }
}

impl From<PolicyDecision> for SandboxPolicyDecision {
    fn from(value: PolicyDecision) -> Self {
        match value {
            PolicyDecision::Allow => SandboxPolicyDecision::Allow,
            PolicyDecision::Ask { risk, reason } => SandboxPolicyDecision::Ask { risk, reason },
            PolicyDecision::Deny { reason } => SandboxPolicyDecision::Deny { reason },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxManager {
    platform: String,
    available_types: Vec<SandboxType>,
}

impl Default for SandboxManager {
    fn default() -> Self {
        Self::detect()
    }
}

impl SandboxManager {
    pub fn detect() -> Self {
        Self {
            platform: std::env::consts::OS.to_string(),
            available_types: detect_available_sandbox_types(),
        }
    }

    pub fn with_capabilities(
        platform: impl Into<String>,
        available_types: Vec<SandboxType>,
    ) -> Self {
        Self {
            platform: platform.into(),
            available_types,
        }
    }

    pub fn select_initial(&self, preference: SandboxPreference) -> SandboxStatus {
        match (preference, self.available_types.first().copied()) {
            (SandboxPreference::Forbid, _) => SandboxStatus {
                supported: true,
                enforcement: SandboxEnforcement::None,
                platform: self.platform.clone(),
                message: "OS sandboxing was explicitly forbidden for this execution".to_string(),
                available_sandbox_types: self.available_types.clone(),
            },
            (SandboxPreference::Require, Some(kind)) => SandboxStatus {
                supported: true,
                enforcement: SandboxEnforcement::OsSandbox,
                platform: self.platform.clone(),
                message: format!("required OS sandbox is available: {kind:?}"),
                available_sandbox_types: self.available_types.clone(),
            },
            (SandboxPreference::Require, None) => SandboxStatus {
                supported: false,
                enforcement: SandboxEnforcement::None,
                platform: self.platform.clone(),
                message: "required OS sandbox is not available; refusing to plan execution"
                    .to_string(),
                available_sandbox_types: Vec::new(),
            },
            (SandboxPreference::Auto, Some(kind)) => SandboxStatus {
                supported: true,
                enforcement: SandboxEnforcement::OsSandbox,
                platform: self.platform.clone(),
                message: format!("OS sandbox is available: {kind:?}"),
                available_sandbox_types: self.available_types.clone(),
            },
            (SandboxPreference::Auto, None) => SandboxStatus {
                supported: false,
                enforcement: SandboxEnforcement::ReviewOnly,
                platform: self.platform.clone(),
                message: "OS sandbox adapters are not available; using review-layer policy only"
                    .to_string(),
                available_sandbox_types: Vec::new(),
            },
        }
    }

    pub fn transform(
        &self,
        policy: &SandboxPolicy,
        request: &ExecRequest,
        preference: SandboxPreference,
    ) -> SandboxTransform {
        let decision = evaluate_exec(policy, request);
        if matches!(decision, PolicyDecision::Deny { .. }) {
            return SandboxTransform {
                decision,
                plan: None,
            };
        }

        let status = self.select_initial(preference);
        if preference == SandboxPreference::Require
            && status.enforcement != SandboxEnforcement::OsSandbox
        {
            return SandboxTransform {
                decision: PolicyDecision::Deny {
                    reason: status.message,
                },
                plan: None,
            };
        }

        let sandbox_type = if status.enforcement == SandboxEnforcement::OsSandbox {
            self.available_types
                .first()
                .copied()
                .unwrap_or(SandboxType::None)
        } else {
            SandboxType::None
        };
        let concrete = concrete_policy(policy, preference, sandbox_type);
        let readable_roots =
            effective_readable_roots_for_request(&policy.permission_profile, &request.cwd);
        let writable_roots = if policy.filesystem == FilesystemPolicy::ReadOnly {
            Vec::new()
        } else {
            effective_writable_roots_for_request(&policy.permission_profile, &request.cwd)
        };
        let filesystem_rules = resolved_filesystem_rules_for_request(
            &concrete.filesystem_rules,
            &policy.permission_profile,
            &request.cwd,
        );
        SandboxTransform {
            decision,
            plan: Some(SandboxExecPlan {
                command: request.command.clone(),
                cwd: request.cwd.clone(),
                sandbox_type: concrete.sandbox_type,
                enforcement: status.enforcement,
                filesystem: concrete.filesystem,
                network: concrete.network,
                managed_network: None,
                readable_roots,
                writable_roots,
                filesystem_rules,
                protected_patterns: concrete.protected_patterns,
                diagnostics: vec![sandbox_status_diagnostic(&status, preference)],
            }),
        }
    }
}

fn sandbox_status_diagnostic(status: &SandboxStatus, preference: SandboxPreference) -> Diagnostic {
    let unavailable = status.enforcement != SandboxEnforcement::OsSandbox;
    let fail_closed = preference == SandboxPreference::Require && unavailable;
    let mut metadata = BTreeMap::from([
        ("component".to_string(), "sandbox-adapter".to_string()),
        ("platform".to_string(), status.platform.clone()),
        (
            "enforcement".to_string(),
            format!("{:?}", status.enforcement),
        ),
        ("supported".to_string(), status.supported.to_string()),
        ("preference".to_string(), format!("{:?}", preference)),
        ("failClosed".to_string(), fail_closed.to_string()),
        (
            "availableAdapters".to_string(),
            status
                .available_sandbox_types
                .iter()
                .map(|kind| format!("{kind:?}"))
                .collect::<Vec<_>>()
                .join(","),
        ),
    ]);
    if unavailable {
        metadata.insert(
            "requiredAction".to_string(),
            match status.platform.as_str() {
                "windows" => {
                    "run `oppi sandbox setup-windows --yes` from elevated PowerShell, then restart the terminal"
                }
                "linux" => "install bubblewrap (`bwrap`) or choose a less restrictive policy",
                "macos" => "ensure `/usr/bin/sandbox-exec` is available or choose a less restrictive policy",
                _ => "configure a supported OS sandbox adapter or choose a less restrictive policy",
            }
            .to_string(),
        );
    }
    Diagnostic {
        level: if unavailable {
            DiagnosticLevel::Warning
        } else {
            DiagnosticLevel::Info
        },
        message: status.message.clone(),
        metadata,
    }
}

pub fn default_policy(profile: PermissionProfile) -> SandboxPolicy {
    let filesystem = match profile.mode {
        PermissionMode::ReadOnly => FilesystemPolicy::ReadOnly,
        PermissionMode::Default | PermissionMode::AutoReview => FilesystemPolicy::WorkspaceWrite,
        PermissionMode::FullAccess => FilesystemPolicy::Unrestricted,
    };
    let network = match profile.mode {
        PermissionMode::FullAccess => NetworkPolicy::Enabled,
        PermissionMode::ReadOnly | PermissionMode::Default | PermissionMode::AutoReview => {
            NetworkPolicy::Ask
        }
    };
    SandboxPolicy {
        permission_profile: profile,
        network,
        filesystem,
    }
}

pub fn concrete_policy(
    policy: &SandboxPolicy,
    sandbox_preference: SandboxPreference,
    sandbox_type: SandboxType,
) -> ConcreteSandboxPolicy {
    let protected_patterns = effective_protected_patterns(&policy.permission_profile);
    let readable_roots = effective_readable_roots(&policy.permission_profile);
    let writable_roots = effective_writable_roots(&policy.permission_profile);
    let mut filesystem_rules = readable_roots
        .iter()
        .map(|root| FilesystemRule {
            root: FilesystemRoot::Path { path: root.clone() },
            access: FilesystemAccess::Read,
        })
        .collect::<Vec<_>>();
    if filesystem_rules.is_empty() {
        filesystem_rules.push(FilesystemRule {
            root: FilesystemRoot::Cwd,
            access: FilesystemAccess::Read,
        });
    }
    if policy.filesystem != FilesystemPolicy::ReadOnly {
        for root in &writable_roots {
            filesystem_rules.push(FilesystemRule {
                root: FilesystemRoot::Path { path: root.clone() },
                access: FilesystemAccess::Write,
            });
        }
    }
    filesystem_rules.extend(policy.permission_profile.filesystem_rules.clone());
    for pattern in &protected_patterns {
        filesystem_rules.push(FilesystemRule {
            root: FilesystemRoot::Path {
                path: pattern.clone(),
            },
            access: FilesystemAccess::None,
        });
    }
    if policy.filesystem != FilesystemPolicy::ReadOnly {
        add_protected_metadata_create_rules(&mut filesystem_rules, &writable_roots);
    }

    ConcreteSandboxPolicy {
        sandbox_preference,
        sandbox_type,
        filesystem: policy.filesystem,
        network: policy.network,
        writable_roots,
        protected_patterns,
        filesystem_rules,
    }
}

fn add_protected_metadata_create_rules(
    filesystem_rules: &mut Vec<FilesystemRule>,
    writable_roots: &[String],
) {
    let mut existing: BTreeSet<String> = filesystem_rules
        .iter()
        .filter(|rule| rule.access == FilesystemAccess::None)
        .filter_map(|rule| match &rule.root {
            FilesystemRoot::Path { path } => Some(path.clone()),
            _ => None,
        })
        .collect();
    for root in writable_roots {
        for name in PROTECTED_WORKSPACE_METADATA_DIRS {
            let path = join_policy_path(root, name);
            if existing.insert(path.clone()) {
                filesystem_rules.push(FilesystemRule {
                    root: FilesystemRoot::Path { path },
                    access: FilesystemAccess::None,
                });
            }
        }
    }
}

pub fn evaluate_exec(policy: &SandboxPolicy, request: &ExecRequest) -> PolicyDecision {
    if request.touches_protected_path || touches_protected_pattern(policy, request) {
        return PolicyDecision::Ask {
            risk: RiskLevel::High,
            reason: "command touches a protected path".to_string(),
        };
    }

    if request_touches_denied_filesystem_rule(policy, request) {
        return PolicyDecision::Deny {
            reason: "filesystem policy explicitly denies this path".to_string(),
        };
    }

    match policy.filesystem {
        FilesystemPolicy::ReadOnly if request.writes_files => PolicyDecision::Deny {
            reason: "read-only permission profile blocks file writes".to_string(),
        },
        FilesystemPolicy::WorkspaceWrite
        | FilesystemPolicy::Unrestricted
        | FilesystemPolicy::ReadOnly => {
            if request.writes_files
                && policy.filesystem == FilesystemPolicy::WorkspaceWrite
                && !request_write_scope_is_within_roots(request, &policy.permission_profile)
            {
                return PolicyDecision::Deny {
                    reason: "workspace-write permission profile blocks writes outside configured writable roots".to_string(),
                };
            }
            if request.uses_network {
                match policy.network {
                    NetworkPolicy::Disabled => {
                        return PolicyDecision::Deny {
                            reason: "network access is disabled by policy".to_string(),
                        };
                    }
                    NetworkPolicy::Ask => {
                        return PolicyDecision::Ask {
                            risk: RiskLevel::Medium,
                            reason: "command may access the network".to_string(),
                        };
                    }
                    NetworkPolicy::Enabled => {}
                }
            }
            match policy.permission_profile.mode {
                PermissionMode::AutoReview if request.writes_files => PolicyDecision::Ask {
                    risk: RiskLevel::Medium,
                    reason: "auto-review mode asks before command-driven writes".to_string(),
                },
                _ => PolicyDecision::Allow,
            }
        }
    }
}

fn detect_available_sandbox_types() -> Vec<SandboxType> {
    match std::env::consts::OS {
        "macos" if Path::new("/usr/bin/sandbox-exec").exists() => {
            vec![SandboxType::MacosSeatbelt]
        }
        "linux" => {
            let mut available = Vec::new();
            if command_exists("bwrap") {
                available.push(SandboxType::LinuxBubblewrap);
            }
            available
        }
        "windows" => {
            if windows_restricted_token_status().available
                || windows_dedicated_sandbox_credentials_configured()
            {
                vec![SandboxType::WindowsRestrictedToken]
            } else {
                Vec::new()
            }
        }
        _ => Vec::new(),
    }
}

fn command_exists(command: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths).any(|path| {
            let candidate = path.join(command);
            candidate.is_file() || candidate.with_extension("exe").is_file()
        })
    })
}

#[cfg(windows)]
fn windows_dedicated_sandbox_credentials_configured() -> bool {
    std::env::var("OPPI_WINDOWS_SANDBOX_USERNAME").is_ok_and(|value| !value.trim().is_empty())
        && std::env::var("OPPI_WINDOWS_SANDBOX_PASSWORD")
            .is_ok_and(|value| !value.trim().is_empty())
}

#[cfg(not(windows))]
fn windows_dedicated_sandbox_credentials_configured() -> bool {
    false
}

fn request_write_scope_is_within_roots(request: &ExecRequest, profile: &PermissionProfile) -> bool {
    let roots = effective_writable_roots_for_request(profile, &request.cwd);
    if roots.is_empty() {
        return false;
    }
    let cwd = Path::new(&request.cwd);
    path_is_within_any_root(cwd, &roots, None)
        && request
            .touched_paths
            .iter()
            .all(|path| path_is_within_any_root(Path::new(path), &roots, Some(cwd)))
}

fn request_touches_denied_filesystem_rule(policy: &SandboxPolicy, request: &ExecRequest) -> bool {
    let cwd = Path::new(&request.cwd);
    policy
        .permission_profile
        .filesystem_rules
        .iter()
        .filter(|rule| filesystem_access_denies(rule.access))
        .flat_map(|rule| filesystem_rule_paths(rule, &policy.permission_profile, &request.cwd))
        .any(|root| {
            path_is_within_any_root(cwd, std::slice::from_ref(&root), None)
                || request.touched_paths.iter().any(|path| {
                    path_is_within_any_root(Path::new(path), std::slice::from_ref(&root), Some(cwd))
                })
        })
}

fn path_is_within_any_root(path: &Path, roots: &[String], cwd: Option<&Path>) -> bool {
    let path = normalize_path_for_policy(path, cwd);
    roots.iter().any(|root| {
        let root = normalize_path_for_policy(Path::new(root), cwd);
        path.starts_with(root)
    })
}

fn touches_protected_pattern(policy: &SandboxPolicy, request: &ExecRequest) -> bool {
    let protected = effective_protected_patterns(&policy.permission_profile);
    request.touched_paths.iter().any(|path| {
        protected
            .iter()
            .any(|pattern| path_matches_pattern(path, pattern))
    }) || protected
        .iter()
        .any(|pattern| command_mentions_pattern(&request.command, pattern))
}

fn effective_readable_roots(profile: &PermissionProfile) -> Vec<String> {
    if profile.readable_roots.is_empty() {
        profile.writable_roots.clone()
    } else {
        profile.readable_roots.clone()
    }
}

fn effective_writable_roots(profile: &PermissionProfile) -> Vec<String> {
    let mut roots: BTreeSet<String> = profile.writable_roots.iter().cloned().collect();
    for rule in &profile.filesystem_rules {
        if filesystem_access_grants_write(rule.access) {
            for path in filesystem_rule_paths(rule, profile, ".") {
                roots.insert(path);
            }
        }
    }
    roots.into_iter().collect()
}

fn effective_readable_roots_for_request(profile: &PermissionProfile, cwd: &str) -> Vec<String> {
    let mut roots: BTreeSet<String> = effective_readable_roots(profile).into_iter().collect();
    for rule in &profile.filesystem_rules {
        if filesystem_access_grants_read(rule.access) {
            for path in filesystem_rule_paths(rule, profile, cwd) {
                roots.insert(path);
            }
        }
    }
    roots.into_iter().collect()
}

fn effective_writable_roots_for_request(profile: &PermissionProfile, cwd: &str) -> Vec<String> {
    let mut roots: BTreeSet<String> = profile.writable_roots.iter().cloned().collect();
    for rule in &profile.filesystem_rules {
        if filesystem_access_grants_write(rule.access) {
            for path in filesystem_rule_paths(rule, profile, cwd) {
                roots.insert(path);
            }
        }
    }
    roots.into_iter().collect()
}

fn resolved_filesystem_rules_for_request(
    rules: &[FilesystemRule],
    profile: &PermissionProfile,
    cwd: &str,
) -> Vec<FilesystemRule> {
    rules
        .iter()
        .flat_map(|rule| {
            let paths = filesystem_rule_paths(rule, profile, cwd);
            if paths.is_empty() {
                vec![rule.clone()]
            } else {
                paths
                    .into_iter()
                    .map(|path| FilesystemRule {
                        root: FilesystemRoot::Path { path },
                        access: rule.access,
                    })
                    .collect()
            }
        })
        .collect()
}

fn filesystem_rule_paths(
    rule: &FilesystemRule,
    profile: &PermissionProfile,
    cwd: &str,
) -> Vec<String> {
    match &rule.root {
        FilesystemRoot::Cwd => vec![cwd.to_string()],
        FilesystemRoot::Workspace => workspace_roots(profile),
        FilesystemRoot::ProjectRoots { subpath } => workspace_roots(profile)
            .into_iter()
            .map(|root| match subpath {
                Some(subpath) => join_policy_path(&root, subpath),
                None => root,
            })
            .collect(),
        FilesystemRoot::Home => home_dir().into_iter().collect(),
        FilesystemRoot::Temp => vec![std::env::temp_dir().to_string_lossy().to_string()],
        FilesystemRoot::PlatformDefaults | FilesystemRoot::Unknown { .. } => Vec::new(),
        FilesystemRoot::Path { path } => vec![path.clone()],
    }
}

fn workspace_roots(profile: &PermissionProfile) -> Vec<String> {
    let mut roots: BTreeSet<String> = effective_readable_roots(profile).into_iter().collect();
    roots.extend(profile.writable_roots.iter().cloned());
    roots.into_iter().collect()
}

fn join_policy_path(root: &str, subpath: &str) -> String {
    if root.starts_with('/') {
        lexical_clean_slash_path(&format!("{}/{}", root.trim_end_matches('/'), subpath))
    } else {
        Path::new(root).join(subpath).to_string_lossy().to_string()
    }
}

fn home_dir() -> Option<String> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(|path| PathBuf::from(path).to_string_lossy().to_string())
}

fn filesystem_access_grants_read(access: FilesystemAccess) -> bool {
    matches!(
        access,
        FilesystemAccess::Read
            | FilesystemAccess::Write
            | FilesystemAccess::ReadOnly
            | FilesystemAccess::ReadWrite
    )
}

fn filesystem_access_grants_write(access: FilesystemAccess) -> bool {
    matches!(
        access,
        FilesystemAccess::Write | FilesystemAccess::ReadWrite
    )
}

fn filesystem_access_denies(access: FilesystemAccess) -> bool {
    matches!(access, FilesystemAccess::None | FilesystemAccess::DenyRead)
}

fn effective_protected_patterns(profile: &PermissionProfile) -> Vec<String> {
    let mut patterns = BTreeSet::new();
    for pattern in DEFAULT_PROTECTED_PATTERNS {
        patterns.insert((*pattern).to_string());
    }
    for pattern in &profile.protected_patterns {
        patterns.insert(pattern.clone());
    }
    patterns.into_iter().collect()
}

fn command_mentions_pattern(command: &str, pattern: &str) -> bool {
    let needle = pattern
        .trim_matches('*')
        .trim_end_matches('/')
        .replace('\\', "/");
    !needle.is_empty() && command.replace('\\', "/").contains(&needle)
}

fn path_matches_pattern(path: &str, pattern: &str) -> bool {
    let normalized = normalize_path_for_policy(Path::new(path), None);
    let path = normalized.to_string_lossy().replace('\\', "/");
    let file_name = normalized
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default();
    glob_like_matches(&path, pattern) || glob_like_matches(&file_name, pattern)
}

fn glob_like_matches(value: &str, pattern: &str) -> bool {
    let pattern = pattern.replace('\\', "/");
    if pattern == "*" {
        return true;
    }
    if pattern.ends_with('/') {
        let component = pattern.trim_end_matches('/');
        return value
            .split('/')
            .any(|part| part.eq_ignore_ascii_case(component))
            || value.contains(&format!("/{component}/"));
    }
    match (pattern.strip_prefix('*'), pattern.strip_suffix('*')) {
        (Some(suffix), _) => value.ends_with(suffix),
        (_, Some(prefix)) => value.starts_with(prefix) || file_name_starts_with(value, prefix),
        _ => value == pattern || value.ends_with(&format!("/{pattern}")),
    }
}

fn file_name_starts_with(value: &str, prefix: &str) -> bool {
    value
        .rsplit('/')
        .next()
        .map(|name| name.starts_with(prefix))
        .unwrap_or(false)
}

fn normalize_path_for_policy(path: &Path, cwd: Option<&Path>) -> PathBuf {
    let has_root = matches!(path.components().next(), Some(Component::RootDir));
    let absolute = if path.is_absolute() || has_root {
        path.to_path_buf()
    } else {
        cwd.unwrap_or_else(|| Path::new(".")).join(path)
    };
    let cleaned = lexical_clean(&absolute);
    let canonical = canonicalize_existing_prefix(&cleaned).unwrap_or(cleaned);
    if cfg!(windows) {
        PathBuf::from(canonical.to_string_lossy().to_lowercase())
    } else {
        canonical
    }
}

fn canonicalize_existing_prefix(path: &Path) -> Option<PathBuf> {
    let mut probe = path.to_path_buf();
    let mut suffix = Vec::new();
    loop {
        if probe.exists() {
            let mut canonical = fs::canonicalize(&probe).ok()?;
            for component in suffix.iter().rev() {
                canonical.push(component);
            }
            return Some(lexical_clean(&canonical));
        }
        let file_name = probe.file_name()?.to_owned();
        suffix.push(file_name);
        if !probe.pop() {
            return None;
        }
    }
}

fn lexical_clean(path: &Path) -> PathBuf {
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

pub fn current_sandbox_status() -> SandboxStatus {
    SandboxManager::detect().select_initial(SandboxPreference::Auto)
}

pub fn policy_from_user_config(config: &SandboxUserConfig) -> SandboxPolicy {
    let project_roots = config.project_roots.clone();
    let readable_roots = if config.readable_roots.is_empty() {
        project_roots.clone()
    } else {
        config.readable_roots.clone()
    };
    let writable_roots = if config.writable_roots.is_empty() {
        project_roots
    } else {
        config.writable_roots.clone()
    };
    SandboxPolicy {
        permission_profile: PermissionProfile {
            mode: PermissionMode::AutoReview,
            readable_roots,
            writable_roots,
            filesystem_rules: config.filesystem_rules.clone(),
            protected_patterns: config.protected_patterns.clone(),
        },
        network: if config.allow_internet || config.allow_ssh {
            NetworkPolicy::Enabled
        } else {
            NetworkPolicy::Disabled
        },
        filesystem: FilesystemPolicy::WorkspaceWrite,
    }
}

pub fn effective_policy_for_tool(
    base: &SandboxPolicy,
    manifest: &ToolPermissionManifest,
) -> SandboxPolicy {
    SandboxPolicy {
        permission_profile: merge_permission_profile(
            &base.permission_profile,
            &manifest.additional_permissions,
        ),
        filesystem: base.filesystem,
        network: merge_network_policy(base.network, manifest.additional_permissions.network),
    }
}

fn merge_permission_profile(
    base: &PermissionProfile,
    additional: &AdditionalPermissionProfile,
) -> PermissionProfile {
    let base_readable_roots = effective_readable_roots(base);
    let readable_roots = if additional.readable_roots.is_empty() {
        base_readable_roots.clone()
    } else {
        additional
            .readable_roots
            .iter()
            .filter(|root| path_is_within_any_root(Path::new(root), &base_readable_roots, None))
            .cloned()
            .collect()
    };
    let writable_roots = additional
        .writable_roots
        .iter()
        .filter(|root| path_is_within_any_root(Path::new(root), &base.writable_roots, None))
        .cloned()
        .collect();
    let mut filesystem_rules = base.filesystem_rules.clone();
    filesystem_rules.extend(
        additional
            .filesystem_rules
            .iter()
            .filter(|rule| requested_rule_is_within_base(rule, base))
            .cloned(),
    );
    let mut protected_patterns = base.protected_patterns.clone();
    protected_patterns.extend(additional.protected_patterns.clone());
    PermissionProfile {
        mode: base.mode,
        readable_roots,
        writable_roots,
        filesystem_rules,
        protected_patterns,
    }
}

fn requested_rule_is_within_base(rule: &FilesystemRule, base: &PermissionProfile) -> bool {
    if filesystem_access_denies(rule.access) {
        return true;
    }
    let allowed_roots = if filesystem_access_grants_write(rule.access) {
        base.writable_roots.clone()
    } else {
        effective_readable_roots(base)
    };
    if allowed_roots.is_empty() {
        return false;
    }
    filesystem_rule_paths(rule, base, ".")
        .iter()
        .all(|path| path_is_within_any_root(Path::new(path), &allowed_roots, None))
}

fn merge_network_policy(base: NetworkPolicy, requested: Option<NetworkPolicy>) -> NetworkPolicy {
    match (base, requested.unwrap_or(base)) {
        (NetworkPolicy::Disabled, _) | (_, NetworkPolicy::Disabled) => NetworkPolicy::Disabled,
        (NetworkPolicy::Ask, _) | (_, NetworkPolicy::Ask) => NetworkPolicy::Ask,
        (NetworkPolicy::Enabled, NetworkPolicy::Enabled) => NetworkPolicy::Enabled,
    }
}

pub fn spawn_sandboxed_background(
    params: SandboxBackgroundSpawnParams,
) -> Result<(SandboxedBackgroundProcess, SandboxExecPlan), String> {
    spawn_sandboxed_background_with_manager(params, None)
}

fn spawn_sandboxed_background_with_manager(
    params: SandboxBackgroundSpawnParams,
    manager_override: Option<SandboxManager>,
) -> Result<(SandboxedBackgroundProcess, SandboxExecPlan), String> {
    let manager = if let Some(manager) = manager_override {
        manager
    } else if params.preference == SandboxPreference::Forbid {
        SandboxManager::with_capabilities(std::env::consts::OS, Vec::new())
    } else {
        SandboxManager::detect()
    };
    let transform = manager.transform(&params.policy, &params.request, params.preference);
    match transform.decision.clone() {
        PolicyDecision::Deny { reason } => return Err(reason),
        PolicyDecision::Ask { reason, .. } if !params.approval_granted => {
            return Err(format!(
                "execution requires approval before running: {reason}"
            ));
        }
        PolicyDecision::Ask { .. } | PolicyDecision::Allow => {}
    }
    let Some(mut plan) = transform.plan.clone() else {
        return Err("sandbox transform did not produce an execution plan".to_string());
    };
    plan.managed_network = params.managed_network.clone();
    if requires_os_enforcement(&params.policy) && plan.enforcement != SandboxEnforcement::OsSandbox
    {
        return Err(
            "policy requires OS enforcement; refusing review-only background execution".to_string(),
        );
    }
    if let Err(reason) = validate_managed_network_enforcement(&plan) {
        return Err(reason);
    }
    #[cfg(windows)]
    if plan.sandbox_type == SandboxType::WindowsRestrictedToken {
        let process = windows_process::spawn_windows_restricted_background_plan(
            &plan,
            params.stdout,
            params.stderr,
        )?;
        return Ok((
            SandboxedBackgroundProcess {
                inner: SandboxedBackgroundProcessInner::WindowsRestricted(process),
            },
            plan,
        ));
    }
    #[cfg(not(windows))]
    if plan.sandbox_type == SandboxType::WindowsRestrictedToken {
        return Err(
            "Windows restricted-token background adapter is only available on Windows".to_string(),
        );
    }
    let mut command = command_for_plan(&plan)?;
    command.current_dir(&plan.cwd);
    apply_network_environment(&mut command, &plan);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::from(params.stdout))
        .stderr(Stdio::from(params.stderr));
    let child = command.spawn().map_err(|error| error.to_string())?;
    let containment = attach_process_containment(&plan, &child)?;
    Ok((
        SandboxedBackgroundProcess {
            inner: SandboxedBackgroundProcessInner::Standard {
                child,
                _containment: containment,
            },
        },
        plan,
    ))
}

pub fn execute_sandboxed(params: SandboxExecParams) -> SandboxExecResult {
    let manager = if params.preference == SandboxPreference::Forbid {
        SandboxManager::with_capabilities(std::env::consts::OS, Vec::new())
    } else {
        SandboxManager::detect()
    };
    let transform = manager.transform(&params.policy, &params.request, params.preference);
    let decision = transform.decision.clone();
    let mut audit = vec![audit_record(
        "plan",
        &decision,
        &params.request.command,
        transform.plan.as_ref(),
    )];

    if let PolicyDecision::Deny { reason } = decision.clone() {
        audit.push(audit_record_with_reason(
            "deny",
            &decision,
            &params.request.command,
            transform.plan.as_ref(),
            Some(reason),
        ));
        return SandboxExecResult {
            decision: decision.into(),
            plan: transform.plan,
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            timed_out: false,
            audit,
        };
    }

    if let PolicyDecision::Ask { reason, .. } = decision.clone() {
        if !params.approval_granted {
            let denied = PolicyDecision::Deny {
                reason: format!("execution requires approval before running: {reason}"),
            };
            audit.push(audit_record_with_reason(
                "approval-required",
                &denied,
                &params.request.command,
                transform.plan.as_ref(),
                Some(reason),
            ));
            return SandboxExecResult {
                decision: denied.into(),
                plan: transform.plan,
                exit_code: None,
                stdout: String::new(),
                stderr: String::new(),
                timed_out: false,
                audit,
            };
        }
        audit.push(audit_record_with_reason(
            "approved",
            &PolicyDecision::Allow,
            &params.request.command,
            transform.plan.as_ref(),
            Some(reason),
        ));
    }

    let Some(mut plan) = transform.plan.clone() else {
        let denied = PolicyDecision::Deny {
            reason: "sandbox transform did not produce an execution plan".to_string(),
        };
        return SandboxExecResult {
            decision: denied.into(),
            plan: None,
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            timed_out: false,
            audit,
        };
    };

    plan.managed_network = params.managed_network.clone();

    if requires_os_enforcement(&params.policy) && plan.enforcement != SandboxEnforcement::OsSandbox
    {
        let denied = PolicyDecision::Deny {
            reason: "policy requires OS enforcement; refusing review-only execution".to_string(),
        };
        audit.push(audit_record_with_reason(
            "fail-closed",
            &denied,
            &params.request.command,
            Some(&plan),
            Some("review-only execution cannot enforce restrictive policy".to_string()),
        ));
        return SandboxExecResult {
            decision: denied.into(),
            plan: Some(plan),
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            timed_out: false,
            audit,
        };
    }

    if let Err(reason) = validate_managed_network_enforcement(&plan) {
        let denied = PolicyDecision::Deny {
            reason: reason.clone(),
        };
        audit.push(audit_record_with_reason(
            "fail-closed",
            &denied,
            &params.request.command,
            Some(&plan),
            Some(reason),
        ));
        return SandboxExecResult {
            decision: denied.into(),
            plan: Some(plan),
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            timed_out: false,
            audit,
        };
    }

    let execution = run_command_with_plan(
        &plan,
        params.timeout_ms.unwrap_or(30_000),
        params.max_output_bytes.unwrap_or(64 * 1024),
    );
    audit.push(audit_record(
        "execute",
        &PolicyDecision::Allow,
        &params.request.command,
        Some(&plan),
    ));

    match execution {
        Ok(output) => SandboxExecResult {
            decision: SandboxPolicyDecision::Allow,
            plan: Some(plan),
            exit_code: output.exit_code,
            stdout: output.stdout,
            stderr: output.stderr,
            timed_out: output.timed_out,
            audit,
        },
        Err(error) => {
            let denied = PolicyDecision::Deny {
                reason: error.clone(),
            };
            audit.push(audit_record_with_reason(
                "error",
                &denied,
                &params.request.command,
                Some(&plan),
                Some(error),
            ));
            SandboxExecResult {
                decision: denied.into(),
                plan: Some(plan),
                exit_code: None,
                stdout: String::new(),
                stderr: String::new(),
                timed_out: false,
                audit,
            }
        }
    }
}

fn requires_os_enforcement(policy: &SandboxPolicy) -> bool {
    policy.filesystem != FilesystemPolicy::Unrestricted || policy.network != NetworkPolicy::Enabled
}

fn validate_managed_network_enforcement(plan: &SandboxExecPlan) -> Result<(), String> {
    if plan.managed_network.is_none() {
        return Ok(());
    }
    if plan.enforcement != SandboxEnforcement::OsSandbox {
        return Err("managed-network proxy configuration requires OS-enforced proxy-only routing; refusing review-only execution with broad network access".to_string());
    }
    match plan.sandbox_type {
        SandboxType::LinuxBubblewrap
        | SandboxType::MacosSeatbelt
        | SandboxType::WindowsRestrictedToken => Err(format!(
            "managed-network proxy-only routing is not enforced by {:?} yet; refusing broad network execution",
            plan.sandbox_type
        )),
        SandboxType::None | SandboxType::LinuxLandlock => Err(format!(
            "managed-network proxy-only routing is not available for {:?}",
            plan.sandbox_type
        )),
    }
}

#[derive(Debug)]
pub struct SandboxedBackgroundProcess {
    inner: SandboxedBackgroundProcessInner,
}

#[derive(Debug)]
enum SandboxedBackgroundProcessInner {
    Standard {
        child: Child,
        _containment: ProcessContainmentGuard,
    },
    #[cfg(windows)]
    WindowsRestricted(windows_process::WindowsRestrictedBackgroundProcess),
}

impl SandboxedBackgroundProcess {
    pub fn try_wait(&mut self) -> std::io::Result<Option<ExitStatus>> {
        match &mut self.inner {
            SandboxedBackgroundProcessInner::Standard { child, .. } => child.try_wait(),
            #[cfg(windows)]
            SandboxedBackgroundProcessInner::WindowsRestricted(process) => process.try_wait(),
        }
    }

    pub fn wait(&mut self) -> std::io::Result<ExitStatus> {
        match &mut self.inner {
            SandboxedBackgroundProcessInner::Standard { child, .. } => child.wait(),
            #[cfg(windows)]
            SandboxedBackgroundProcessInner::WindowsRestricted(process) => process.wait(),
        }
    }

    pub fn kill(&mut self) -> std::io::Result<()> {
        match &mut self.inner {
            SandboxedBackgroundProcessInner::Standard { child, .. } => {
                kill_process_tree(child);
                Ok(())
            }
            #[cfg(windows)]
            SandboxedBackgroundProcessInner::WindowsRestricted(process) => process.kill(),
        }
    }
}

pub struct SandboxBackgroundSpawnParams {
    pub policy: SandboxPolicy,
    pub request: SandboxExecRequest,
    pub preference: SandboxPreference,
    pub approval_granted: bool,
    pub stdout: File,
    pub stderr: File,
    pub managed_network: Option<ManagedNetworkConfig>,
}

#[derive(Debug)]
struct CapturedOutput {
    exit_code: Option<i32>,
    stdout: String,
    stderr: String,
    timed_out: bool,
}

fn run_command_with_plan(
    plan: &SandboxExecPlan,
    timeout_ms: u64,
    max_output_bytes: usize,
) -> Result<CapturedOutput, String> {
    #[cfg(windows)]
    if plan.sandbox_type == SandboxType::WindowsRestrictedToken {
        return windows_process::run_windows_restricted_plan(plan, timeout_ms, max_output_bytes);
    }

    let mut command = command_for_plan(plan)?;
    command.current_dir(&plan.cwd);
    apply_network_environment(&mut command, plan);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|error| error.to_string())?;
    let _containment = attach_process_containment(plan, &child)?;
    let mut stdout = child.stdout.take().expect("stdout was piped");
    let mut stderr = child.stderr.take().expect("stderr was piped");
    let stdout_reader = thread::spawn(move || read_bounded(&mut stdout, max_output_bytes));
    let stderr_reader = thread::spawn(move || read_bounded(&mut stderr, max_output_bytes));
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    let mut timed_out = false;
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
            break status;
        }
        if Instant::now() >= deadline {
            timed_out = true;
            kill_process_tree(&mut child);
            break child.wait().map_err(|error| error.to_string())?;
        }
        thread::sleep(Duration::from_millis(10));
    };
    let stdout = stdout_reader
        .join()
        .map_err(|_| "stdout reader panicked".to_string())?
        .map_err(|error| error.to_string())?;
    let stderr = stderr_reader
        .join()
        .map_err(|_| "stderr reader panicked".to_string())?
        .map_err(|error| error.to_string())?;
    Ok(CapturedOutput {
        exit_code: status.code(),
        stdout,
        stderr,
        timed_out,
    })
}

#[derive(Debug)]
struct ProcessContainmentGuard {
    #[cfg(windows)]
    job: Option<windows_sys::Win32::Foundation::HANDLE>,
}

// The guard owns a process/job handle and only closes it on drop. HANDLE values are
// process-wide kernel handles, so transferring ownership between worker threads is
// safe as long as the guard remains single-owner.
unsafe impl Send for ProcessContainmentGuard {}

fn attach_process_containment(
    plan: &SandboxExecPlan,
    child: &std::process::Child,
) -> Result<ProcessContainmentGuard, String> {
    #[cfg(windows)]
    {
        if plan.sandbox_type == SandboxType::WindowsRestrictedToken {
            return attach_windows_job_object(child);
        }
        Ok(ProcessContainmentGuard { job: None })
    }
    #[cfg(not(windows))]
    {
        let _ = (plan, child);
        Ok(ProcessContainmentGuard {})
    }
}

#[cfg(windows)]
fn attach_windows_job_object(
    child: &std::process::Child,
) -> Result<ProcessContainmentGuard, String> {
    use std::mem::{size_of, zeroed};
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOB_OBJECT_UILIMIT_DESKTOP, JOB_OBJECT_UILIMIT_DISPLAYSETTINGS,
        JOB_OBJECT_UILIMIT_EXITWINDOWS, JOB_OBJECT_UILIMIT_GLOBALATOMS, JOB_OBJECT_UILIMIT_HANDLES,
        JOB_OBJECT_UILIMIT_READCLIPBOARD, JOB_OBJECT_UILIMIT_SYSTEMPARAMETERS,
        JOB_OBJECT_UILIMIT_WRITECLIPBOARD, JOBOBJECT_BASIC_UI_RESTRICTIONS,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectBasicUIRestrictions,
        JobObjectExtendedLimitInformation, SetInformationJobObject,
    };

    unsafe {
        let job = CreateJobObjectW(std::ptr::null_mut(), std::ptr::null());
        if job.is_null() {
            return Err("CreateJobObjectW failed".to_string());
        }
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = zeroed();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let configured = SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &info as *const _ as *const _,
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        );
        if configured == 0 {
            CloseHandle(job);
            return Err("SetInformationJobObject failed".to_string());
        }
        let ui_limits = JOBOBJECT_BASIC_UI_RESTRICTIONS {
            UIRestrictionsClass: JOB_OBJECT_UILIMIT_DESKTOP
                | JOB_OBJECT_UILIMIT_DISPLAYSETTINGS
                | JOB_OBJECT_UILIMIT_EXITWINDOWS
                | JOB_OBJECT_UILIMIT_GLOBALATOMS
                | JOB_OBJECT_UILIMIT_HANDLES
                | JOB_OBJECT_UILIMIT_READCLIPBOARD
                | JOB_OBJECT_UILIMIT_SYSTEMPARAMETERS
                | JOB_OBJECT_UILIMIT_WRITECLIPBOARD,
        };
        let ui_configured = SetInformationJobObject(
            job,
            JobObjectBasicUIRestrictions,
            &ui_limits as *const _ as *const _,
            size_of::<JOBOBJECT_BASIC_UI_RESTRICTIONS>() as u32,
        );
        if ui_configured == 0 {
            CloseHandle(job);
            return Err("SetInformationJobObject UI restrictions failed".to_string());
        }
        let process: HANDLE = child.as_raw_handle() as HANDLE;
        if AssignProcessToJobObject(job, process) == 0 {
            CloseHandle(job);
            return Err("AssignProcessToJobObject failed".to_string());
        }
        Ok(ProcessContainmentGuard { job: Some(job) })
    }
}

impl Drop for ProcessContainmentGuard {
    fn drop(&mut self) {
        #[cfg(windows)]
        if let Some(job) = self.job.take() {
            unsafe {
                windows_sys::Win32::Foundation::CloseHandle(job);
            }
        }
    }
}

fn kill_process_tree(child: &mut std::process::Child) {
    let pid = child.id().to_string();
    if cfg!(windows) {
        let taskkill = Command::new("taskkill")
            .args(["/PID", &pid, "/T", "/F"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        if !matches!(taskkill, Ok(status) if status.success()) {
            let _ = child.kill();
        }
    } else {
        let _ = Command::new("pkill").args(["-TERM", "-P", &pid]).status();
        let _ = child.kill();
    }
}

fn apply_network_environment(command: &mut Command, plan: &SandboxExecPlan) {
    if plan.network == NetworkPolicy::Disabled {
        for key in [
            "HTTP_PROXY",
            "HTTPS_PROXY",
            "ALL_PROXY",
            "NO_PROXY",
            "http_proxy",
            "https_proxy",
            "all_proxy",
            "no_proxy",
        ] {
            command.env_remove(key);
        }
        return;
    }
    if let Some(config) = &plan.managed_network {
        apply_proxy_env(command, config);
    }
}

fn apply_proxy_env(command: &mut Command, config: &ManagedNetworkConfig) {
    if let Some(proxy) = &config.http_proxy {
        command.env("HTTP_PROXY", proxy).env("http_proxy", proxy);
    }
    if let Some(proxy) = &config.https_proxy {
        command.env("HTTPS_PROXY", proxy).env("https_proxy", proxy);
    }
    if let Some(proxy) = &config.all_proxy {
        command.env("ALL_PROXY", proxy).env("all_proxy", proxy);
    }
    if !config.no_proxy.is_empty() || config.allow_loopback {
        let mut entries = config.no_proxy.clone();
        if config.allow_loopback {
            entries.extend([
                "localhost".to_string(),
                "127.0.0.1".to_string(),
                "::1".to_string(),
            ]);
        }
        let no_proxy = entries.join(",");
        command.env("NO_PROXY", &no_proxy).env("no_proxy", no_proxy);
    }
}

fn command_for_plan(plan: &SandboxExecPlan) -> Result<Command, String> {
    match plan.sandbox_type {
        SandboxType::None => shell_command(&plan.command),
        SandboxType::MacosSeatbelt => {
            let mut command = Command::new("/usr/bin/sandbox-exec");
            command.arg("-p").arg(macos_seatbelt_profile(plan));
            append_shell_args(&mut command, &plan.command);
            Ok(command)
        }
        SandboxType::LinuxBubblewrap => {
            let mut command = Command::new("bwrap");
            command.args(["--ro-bind", "/", "/", "--dev", "/dev", "--proc", "/proc"]);
            for root in &plan.writable_roots {
                command.args(["--bind", root, root]);
            }
            for path in deny_mount_paths(plan) {
                command.arg("--tmpfs").arg(path);
            }
            if plan.network == NetworkPolicy::Disabled {
                command.arg("--unshare-net");
            }
            command.arg("--chdir").arg(&plan.cwd);
            append_shell_args(&mut command, &plan.command);
            Ok(command)
        }
        SandboxType::LinuxLandlock => Err(format!(
            "sandbox adapter {:?} is not executable in this build",
            plan.sandbox_type
        )),
        SandboxType::WindowsRestrictedToken => {
            if cfg!(windows) {
                shell_command(&plan.command)
            } else {
                Err(
                    "Windows restricted-token/job-object adapter is only available on Windows"
                        .to_string(),
                )
            }
        }
    }
}

fn shell_command(command: &str) -> Result<Command, String> {
    if cfg!(windows) {
        let mut cmd = Command::new("cmd");
        cmd.args(["/C", command]);
        Ok(cmd)
    } else {
        let mut cmd = Command::new("sh");
        cmd.args(["-c", command]);
        Ok(cmd)
    }
}

fn append_shell_args(command: &mut Command, script: &str) {
    command.args(["/bin/sh", "-c", script]);
}

fn macos_seatbelt_profile(plan: &SandboxExecPlan) -> String {
    let mut profile =
        String::from("(version 1)\n(deny default)\n(allow process*)\n(allow file-read*)\n");
    if plan.filesystem != FilesystemPolicy::ReadOnly {
        for root in &plan.writable_roots {
            profile.push_str(&format!(
                "(allow file-write* (subpath \"{}\"))\n",
                seatbelt_escape(root)
            ));
        }
    }
    for path in deny_mount_paths(plan) {
        profile.push_str(&format!(
            "(deny file-read* file-write* (subpath \"{}\"))\n",
            seatbelt_escape(&path)
        ));
    }
    if plan.network == NetworkPolicy::Enabled {
        profile.push_str("(allow network*)\n");
    }
    profile
}

fn deny_mount_paths(plan: &SandboxExecPlan) -> Vec<String> {
    let mut paths = BTreeSet::new();
    for rule in &plan.filesystem_rules {
        if !filesystem_access_denies(rule.access) {
            continue;
        }
        let Some(path) = concrete_rule_path_for_adapter(rule, &plan.cwd) else {
            continue;
        };
        if path == "/" || path.is_empty() {
            continue;
        }
        paths.insert(path);
    }
    paths.into_iter().collect()
}

fn concrete_rule_path_for_adapter(rule: &FilesystemRule, cwd: &str) -> Option<String> {
    let FilesystemRoot::Path { path } = &rule.root else {
        return None;
    };
    if path_has_glob_chars(path) {
        return None;
    }
    let path = path.trim_end_matches(['/', '\\']);
    if path.is_empty() {
        return None;
    }
    if cwd.starts_with('/') {
        let resolved = if path.starts_with('/') {
            path.to_string()
        } else {
            format!("{}/{}", cwd.trim_end_matches('/'), path)
        };
        return Some(lexical_clean_slash_path(&resolved));
    }
    let resolved = if Path::new(path).is_absolute() {
        PathBuf::from(path)
    } else {
        Path::new(cwd).join(path)
    };
    Some(lexical_clean(&resolved).to_string_lossy().to_string())
}

fn lexical_clean_slash_path(path: &str) -> String {
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            part => parts.push(part),
        }
    }
    if path.starts_with('/') {
        format!("/{}", parts.join("/"))
    } else {
        parts.join("/")
    }
}

fn path_has_glob_chars(path: &str) -> bool {
    path.contains('*') || path.contains('?') || path.contains('[') || path.contains(']')
}

fn seatbelt_escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "")
}

fn read_bounded(reader: &mut impl Read, max_bytes: usize) -> std::io::Result<String> {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 4096];
    loop {
        let read = reader.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        let remaining = max_bytes.saturating_sub(buffer.len());
        if remaining == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read.min(remaining)]);
    }
    Ok(String::from_utf8_lossy(&buffer).to_string())
}

fn audit_record(
    action: &str,
    decision: &PolicyDecision,
    command: &str,
    plan: Option<&SandboxExecPlan>,
) -> SandboxAuditRecord {
    audit_record_with_reason(action, decision, command, plan, None)
}

fn audit_record_with_reason(
    action: &str,
    decision: &PolicyDecision,
    command: &str,
    plan: Option<&SandboxExecPlan>,
    reason: Option<String>,
) -> SandboxAuditRecord {
    SandboxAuditRecord {
        action: action.to_string(),
        decision: decision.clone().into(),
        command: redact_sensitive(command),
        reason,
        enforcement: plan.map(|plan| plan.enforcement),
        sandbox_type: plan.map(|plan| plan.sandbox_type),
    }
}

fn redact_sensitive(value: &str) -> String {
    let mut redacted = value.to_string();
    for marker in [
        "OPENAI_API_KEY=",
        "ANTHROPIC_API_KEY=",
        "GITHUB_TOKEN=",
        "AUTH_TOKEN=",
        "TOKEN=",
    ] {
        let mut search_from = 0;
        while let Some(relative_start) = redacted[search_from..].find(marker) {
            let start = search_from + relative_start;
            let value_start = start + marker.len();
            let value_end = redacted[value_start..]
                .find(char::is_whitespace)
                .map(|offset| value_start + offset)
                .unwrap_or(redacted.len());
            if &redacted[value_start..value_end] != "[REDACTED]" {
                redacted.replace_range(value_start..value_end, "[REDACTED]");
                search_from = value_start + "[REDACTED]".len();
            } else {
                search_from = value_end;
            }
        }
    }
    for pattern in DEFAULT_PROTECTED_PATTERNS {
        let needle = pattern.trim_matches('*').trim_end_matches('/');
        if !needle.is_empty() {
            redacted = redacted.replace(needle, "[PROTECTED]");
        }
    }
    redacted
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn profile(mode: PermissionMode) -> PermissionProfile {
        PermissionProfile {
            mode,
            readable_roots: vec!["/repo".to_string()],
            writable_roots: vec!["/repo".to_string()],
            filesystem_rules: Vec::new(),
            protected_patterns: vec![".env*".to_string()],
        }
    }

    fn request(cwd: &str) -> ExecRequest {
        ExecRequest {
            command: "echo hi".to_string(),
            cwd: cwd.to_string(),
            writes_files: false,
            uses_network: false,
            touches_protected_path: false,
            touched_paths: Vec::new(),
        }
    }

    #[test]
    fn read_only_denies_writes() {
        let policy = default_policy(profile(PermissionMode::ReadOnly));
        let decision = evaluate_exec(
            &policy,
            &ExecRequest {
                command: "echo hi > file".to_string(),
                cwd: "/repo".to_string(),
                writes_files: true,
                uses_network: false,
                touches_protected_path: false,
                touched_paths: Vec::new(),
            },
        );
        assert_eq!(
            decision,
            PolicyDecision::Deny {
                reason: "read-only permission profile blocks file writes".to_string()
            }
        );
    }

    #[test]
    fn workspace_write_denies_writes_outside_writable_roots() {
        let policy = default_policy(profile(PermissionMode::Default));
        let decision = evaluate_exec(
            &policy,
            &ExecRequest {
                command: "echo hi > file".to_string(),
                cwd: "/tmp".to_string(),
                writes_files: true,
                uses_network: false,
                touches_protected_path: false,
                touched_paths: Vec::new(),
            },
        );
        assert_eq!(
            decision,
            PolicyDecision::Deny {
                reason: "workspace-write permission profile blocks writes outside configured writable roots".to_string()
            }
        );
    }

    #[test]
    fn workspace_write_allows_writes_inside_writable_roots() {
        let policy = default_policy(profile(PermissionMode::Default));
        let decision = evaluate_exec(
            &policy,
            &ExecRequest {
                command: "echo hi > file".to_string(),
                cwd: "/repo/src".to_string(),
                writes_files: true,
                uses_network: false,
                touches_protected_path: false,
                touched_paths: Vec::new(),
            },
        );
        assert_eq!(decision, PolicyDecision::Allow);
    }

    #[test]
    fn workspace_write_checks_touched_paths_not_just_cwd() {
        let policy = default_policy(profile(PermissionMode::Default));
        let decision = evaluate_exec(
            &policy,
            &ExecRequest {
                command: "write outside".to_string(),
                cwd: "/repo".to_string(),
                writes_files: true,
                uses_network: false,
                touches_protected_path: false,
                touched_paths: vec!["/tmp/outside.txt".to_string()],
            },
        );
        assert!(matches!(decision, PolicyDecision::Deny { .. }));
    }

    #[test]
    fn root_matching_is_component_bounded() {
        let roots = vec!["/repo".to_string()];
        assert!(path_is_within_any_root(
            Path::new("/repo/src"),
            &roots,
            None
        ));
        assert!(!path_is_within_any_root(
            Path::new("/repo2/src"),
            &roots,
            None
        ));
    }

    #[test]
    fn dot_dot_paths_are_normalized_before_root_checks() {
        let roots = vec!["/repo/work".to_string()];
        assert!(path_is_within_any_root(
            Path::new("/repo/work/src/../file.txt"),
            &roots,
            None
        ));
        assert!(!path_is_within_any_root(
            Path::new("/repo/work/../../escape.txt"),
            &roots,
            None
        ));
    }

    #[cfg(windows)]
    #[test]
    fn windows_drive_paths_are_case_folded_and_component_bounded() {
        let roots = vec!["C:\\Repo".to_string()];
        assert!(path_is_within_any_root(
            Path::new("c:\\repo\\src"),
            &roots,
            None
        ));
        assert!(!path_is_within_any_root(
            Path::new("c:\\repo2\\src"),
            &roots,
            None
        ));
    }

    #[cfg(windows)]
    #[test]
    fn windows_unc_paths_are_component_bounded() {
        let roots = vec!["\\\\server\\share\\repo".to_string()];
        assert!(path_is_within_any_root(
            Path::new("\\\\server\\share\\repo\\src"),
            &roots,
            None
        ));
        assert!(!path_is_within_any_root(
            Path::new("\\\\server\\share\\repo2\\src"),
            &roots,
            None
        ));
    }

    #[cfg(windows)]
    #[test]
    fn windows_junction_escape_is_rejected_by_existing_prefix_canonicalization() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let base = std::env::temp_dir().join(format!("oppi-sandbox-junction-test-{unique}"));
        let repo = base.join("repo");
        let outside = base.join("outside");
        let junction = repo.join("link-out");
        let secret = junction.join("secret.txt");
        fs::create_dir_all(&repo).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("secret.txt"), "secret").unwrap();
        let status = Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&junction)
            .arg(&outside)
            .status()
            .unwrap();
        assert!(status.success(), "failed to create junction for test");

        let roots = vec![repo.to_string_lossy().to_string()];
        assert!(!path_is_within_any_root(&secret, &roots, None));

        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn disabled_network_denies_instead_of_asking() {
        let mut policy = default_policy(profile(PermissionMode::Default));
        policy.network = NetworkPolicy::Disabled;
        let decision = evaluate_exec(
            &policy,
            &ExecRequest {
                command: "curl example.com".to_string(),
                cwd: "/repo".to_string(),
                writes_files: false,
                uses_network: true,
                touches_protected_path: false,
                touched_paths: Vec::new(),
            },
        );
        assert_eq!(
            decision,
            PolicyDecision::Deny {
                reason: "network access is disabled by policy".to_string()
            }
        );
    }

    #[test]
    fn protected_paths_ask_even_in_full_access() {
        let policy = default_policy(profile(PermissionMode::FullAccess));
        let decision = evaluate_exec(
            &policy,
            &ExecRequest {
                command: "cat .env".to_string(),
                cwd: "/repo".to_string(),
                writes_files: false,
                uses_network: false,
                touches_protected_path: true,
                touched_paths: Vec::new(),
            },
        );
        assert!(matches!(
            decision,
            PolicyDecision::Ask {
                risk: RiskLevel::High,
                ..
            }
        ));
    }

    #[test]
    fn default_protected_patterns_are_enforced_by_policy_layer() {
        let mut custom = profile(PermissionMode::FullAccess);
        custom.protected_patterns.clear();
        let policy = default_policy(custom);
        let decision = evaluate_exec(
            &policy,
            &ExecRequest {
                command: "cat safe.txt".to_string(),
                cwd: "/repo".to_string(),
                writes_files: false,
                uses_network: false,
                touches_protected_path: false,
                touched_paths: vec!["/repo/.ssh/id_rsa".to_string()],
            },
        );
        assert!(matches!(
            decision,
            PolicyDecision::Ask {
                risk: RiskLevel::High,
                ..
            }
        ));
    }

    #[test]
    fn protected_patterns_are_enforced_by_policy_layer() {
        let policy = default_policy(profile(PermissionMode::FullAccess));
        let decision = evaluate_exec(
            &policy,
            &ExecRequest {
                command: "cat safe.txt".to_string(),
                cwd: "/repo".to_string(),
                writes_files: false,
                uses_network: false,
                touches_protected_path: false,
                touched_paths: vec!["/repo/.env.local".to_string()],
            },
        );
        assert!(matches!(
            decision,
            PolicyDecision::Ask {
                risk: RiskLevel::High,
                ..
            }
        ));
    }

    #[test]
    fn sandbox_manager_fails_closed_when_required_without_adapter() {
        let manager = SandboxManager::with_capabilities("test", Vec::new());
        let policy = default_policy(profile(PermissionMode::Default));
        let transform = manager.transform(&policy, &request("/repo"), SandboxPreference::Require);
        assert!(matches!(transform.decision, PolicyDecision::Deny { .. }));
        assert!(transform.plan.is_none());
    }

    #[test]
    fn sandbox_manager_returns_review_only_plan_for_auto_without_adapter() {
        let manager = SandboxManager::with_capabilities("linux", Vec::new());
        let policy = default_policy(profile(PermissionMode::Default));
        let transform = manager.transform(&policy, &request("/repo"), SandboxPreference::Auto);
        assert_eq!(transform.decision, PolicyDecision::Allow);
        let plan = transform.plan.unwrap();
        assert_eq!(plan.enforcement, SandboxEnforcement::ReviewOnly);
        assert_eq!(plan.sandbox_type, SandboxType::None);
        assert!(!plan.protected_patterns.is_empty());
        let diagnostic = plan.diagnostics.first().expect("sandbox diagnostic");
        assert_eq!(
            diagnostic.metadata.get("component").map(String::as_str),
            Some("sandbox-adapter")
        );
        assert_eq!(
            diagnostic
                .metadata
                .get("requiredAction")
                .map(String::as_str),
            Some("install bubblewrap (`bwrap`) or choose a less restrictive policy")
        );
        assert_eq!(
            diagnostic.metadata.get("failClosed").map(String::as_str),
            Some("false")
        );
    }

    #[test]
    fn concrete_policy_contains_none_rules_for_protected_patterns_and_metadata() {
        let policy = default_policy(profile(PermissionMode::Default));
        let concrete = concrete_policy(&policy, SandboxPreference::Auto, SandboxType::None);
        let mut expected_rules = vec![
            ".env*".to_string(),
            ".git/".to_string(),
            ".agents/".to_string(),
            ".codex/".to_string(),
        ];
        expected_rules.extend(
            [".git", ".agents", ".codex"]
                .into_iter()
                .map(|name| join_policy_path("/repo", name)),
        );
        for expected in expected_rules {
            assert!(
                concrete.filesystem_rules.iter().any(|rule| {
                    rule.access == FilesystemAccess::None
                        && matches!(&rule.root, FilesystemRoot::Path { path } if path == &expected)
                }),
                "missing protected rule for {expected}"
            );
        }
    }

    #[test]
    fn transform_resolves_missing_metadata_create_protection_under_writable_roots() {
        let policy = default_policy(profile(PermissionMode::Default));
        let manager =
            SandboxManager::with_capabilities("linux", vec![SandboxType::LinuxBubblewrap]);
        let transform = manager.transform(&policy, &request("/repo"), SandboxPreference::Require);
        let plan = transform.plan.unwrap();
        for expected in [".git", ".agents", ".codex"]
            .into_iter()
            .map(|name| join_policy_path("/repo", name))
        {
            assert!(
                plan.filesystem_rules.iter().any(|rule| {
                    rule.access == FilesystemAccess::None
                        && matches!(&rule.root, FilesystemRoot::Path { path } if path == &expected)
                }),
                "missing metadata create-protection rule for {expected}"
            );
        }
    }

    #[test]
    fn explicit_filesystem_write_entry_expands_workspace_scope() {
        let mut profile = profile(PermissionMode::Default);
        profile.filesystem_rules.push(FilesystemRule {
            root: FilesystemRoot::Path {
                path: "/tmp/generated".to_string(),
            },
            access: FilesystemAccess::Write,
        });
        let policy = default_policy(profile);
        let decision = evaluate_exec(
            &policy,
            &ExecRequest {
                command: "write generated".to_string(),
                cwd: "/tmp/generated".to_string(),
                writes_files: true,
                uses_network: false,
                touches_protected_path: false,
                touched_paths: vec!["/tmp/generated/out.txt".to_string()],
            },
        );
        assert_eq!(decision, PolicyDecision::Allow);
    }

    #[test]
    fn explicit_none_entry_denies_even_inside_writable_root() {
        let mut profile = profile(PermissionMode::Default);
        profile.filesystem_rules.push(FilesystemRule {
            root: FilesystemRoot::Path {
                path: "/repo/locked".to_string(),
            },
            access: FilesystemAccess::None,
        });
        let policy = default_policy(profile);
        let decision = evaluate_exec(
            &policy,
            &ExecRequest {
                command: "write locked area".to_string(),
                cwd: "/repo".to_string(),
                writes_files: true,
                uses_network: false,
                touches_protected_path: false,
                touched_paths: vec!["/repo/locked/config.json".to_string()],
            },
        );
        assert_eq!(
            decision,
            PolicyDecision::Deny {
                reason: "filesystem policy explicitly denies this path".to_string()
            }
        );
    }

    #[test]
    fn transform_resolves_special_filesystem_roots_into_plan() {
        let mut profile = profile(PermissionMode::Default);
        profile.filesystem_rules.push(FilesystemRule {
            root: FilesystemRoot::ProjectRoots {
                subpath: Some("docs".to_string()),
            },
            access: FilesystemAccess::Read,
        });
        profile.filesystem_rules.push(FilesystemRule {
            root: FilesystemRoot::Cwd,
            access: FilesystemAccess::Write,
        });
        let policy = default_policy(profile);
        let manager = SandboxManager::with_capabilities("test", Vec::new());
        let transform = manager.transform(&policy, &request("/repo/work"), SandboxPreference::Auto);
        let plan = transform.plan.unwrap();
        assert!(plan.readable_roots.contains(&"/repo/docs".to_string()));
        assert!(plan.writable_roots.contains(&"/repo/work".to_string()));
        assert!(plan.filesystem_rules.iter().any(|rule| {
            rule.access == FilesystemAccess::Read
                && matches!(&rule.root, FilesystemRoot::Path { path } if path == "/repo/docs")
        }));
    }

    #[test]
    fn tool_permission_manifest_intersects_with_base_policy() {
        let base = default_policy(profile(PermissionMode::Default));
        let manifest = ToolPermissionManifest {
            tool_name: "shell".to_string(),
            additional_permissions: AdditionalPermissionProfile {
                readable_roots: vec!["/repo/subdir".to_string(), "/etc".to_string()],
                writable_roots: vec!["/repo/subdir".to_string(), "/tmp".to_string()],
                filesystem_rules: vec![FilesystemRule {
                    root: FilesystemRoot::ProjectRoots {
                        subpath: Some("subdir".to_string()),
                    },
                    access: FilesystemAccess::Read,
                }],
                network: Some(NetworkPolicy::Enabled),
                protected_patterns: vec!["secret.json".to_string()],
            },
            sandbox_preference: SandboxPreference::Require,
        };
        let effective = effective_policy_for_tool(&base, &manifest);
        assert_eq!(
            effective.permission_profile.readable_roots,
            vec!["/repo/subdir"]
        );
        assert_eq!(
            effective.permission_profile.writable_roots,
            vec!["/repo/subdir"]
        );
        assert_eq!(effective.network, NetworkPolicy::Ask);
        assert!(
            effective
                .permission_profile
                .protected_patterns
                .contains(&"secret.json".to_string())
        );
    }

    #[test]
    fn user_config_defaults_read_and_write_to_project_roots_but_allows_read_expansion() {
        let default_config = SandboxUserConfig {
            project_roots: vec!["/repo".to_string()],
            readable_roots: Vec::new(),
            writable_roots: Vec::new(),
            filesystem_rules: Vec::new(),
            allow_internet: false,
            allow_ssh: false,
            managed_network: None,
            protected_patterns: vec![".env*".to_string()],
            sandbox_preference: SandboxPreference::Auto,
        };
        let default_policy = policy_from_user_config(&default_config);
        assert_eq!(
            default_policy.permission_profile.readable_roots,
            vec!["/repo"]
        );
        assert_eq!(
            default_policy.permission_profile.writable_roots,
            vec!["/repo"]
        );
        assert_eq!(default_policy.network, NetworkPolicy::Disabled);

        let expanded = SandboxUserConfig {
            readable_roots: vec!["/repo".to_string(), "/docs".to_string()],
            writable_roots: vec!["/repo".to_string()],
            allow_internet: true,
            ..default_config
        };
        let expanded_policy = policy_from_user_config(&expanded);
        assert_eq!(
            expanded_policy.permission_profile.readable_roots,
            vec!["/repo", "/docs"]
        );
        assert_eq!(
            expanded_policy.permission_profile.writable_roots,
            vec!["/repo"]
        );
        assert_eq!(expanded_policy.network, NetworkPolicy::Enabled);
    }

    #[test]
    fn restrictive_policy_refuses_review_only_transform_when_no_adapter_exists() {
        let policy = default_policy(profile(PermissionMode::ReadOnly));
        let manager = SandboxManager::with_capabilities("test", Vec::new());
        let transform = manager.transform(&policy, &request("/repo"), SandboxPreference::Require);
        assert!(matches!(transform.decision, PolicyDecision::Deny { .. }));
        assert!(transform.plan.is_none());
    }

    fn exec_plan(sandbox_type: SandboxType, network: NetworkPolicy) -> SandboxExecPlan {
        SandboxExecPlan {
            command: "echo hi".to_string(),
            cwd: "/repo".to_string(),
            sandbox_type,
            enforcement: if sandbox_type == SandboxType::None {
                SandboxEnforcement::ReviewOnly
            } else {
                SandboxEnforcement::OsSandbox
            },
            filesystem: FilesystemPolicy::WorkspaceWrite,
            network,
            managed_network: None,
            readable_roots: vec!["/repo".to_string()],
            writable_roots: vec!["/repo".to_string()],
            filesystem_rules: Vec::new(),
            protected_patterns: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_restricted_token_adapter_is_detected_and_runs_under_job_object() {
        let status = current_sandbox_status();
        if !status
            .available_sandbox_types
            .contains(&SandboxType::WindowsRestrictedToken)
        {
            eprintln!(
                "skipping Windows restricted-token execution smoke: {}",
                status.message
            );
            return;
        }

        let mut request = request(".");
        request.command = "echo windows-sandbox".to_string();
        let result = execute_sandboxed(SandboxExecParams {
            policy: SandboxPolicy {
                permission_profile: PermissionProfile {
                    mode: PermissionMode::FullAccess,
                    readable_roots: vec![".".to_string()],
                    writable_roots: vec![".".to_string()],
                    filesystem_rules: Vec::new(),
                    protected_patterns: Vec::new(),
                },
                network: NetworkPolicy::Enabled,
                filesystem: FilesystemPolicy::Unrestricted,
            },
            request,
            preference: SandboxPreference::Require,
            approval_granted: false,
            managed_network: None,
            timeout_ms: Some(5_000),
            max_output_bytes: Some(4_000),
        });
        assert!(
            matches!(result.decision, SandboxPolicyDecision::Allow),
            "{:?}",
            result
        );
        assert_eq!(
            result.plan.as_ref().unwrap().sandbox_type,
            SandboxType::WindowsRestrictedToken
        );
        assert!(result.stdout.contains("windows-sandbox"), "{result:?}");
    }

    #[cfg(windows)]
    #[test]
    fn windows_disabled_network_execution_fails_closed_without_dedicated_wfp() {
        if std::env::var("OPPI_WINDOWS_SANDBOX_USERNAME").is_ok()
            && std::env::var("OPPI_WINDOWS_SANDBOX_PASSWORD").is_ok()
            && std::env::var("OPPI_WINDOWS_SANDBOX_WFP_READY").as_deref() == Ok("1")
        {
            return;
        }
        let mut request = request(".");
        request.command = "echo no-network".to_string();
        let result = execute_sandboxed(SandboxExecParams {
            policy: SandboxPolicy {
                permission_profile: PermissionProfile {
                    mode: PermissionMode::FullAccess,
                    readable_roots: vec![".".to_string()],
                    writable_roots: vec![".".to_string()],
                    filesystem_rules: Vec::new(),
                    protected_patterns: Vec::new(),
                },
                network: NetworkPolicy::Disabled,
                filesystem: FilesystemPolicy::Unrestricted,
            },
            request,
            preference: SandboxPreference::Require,
            approval_granted: false,
            managed_network: None,
            timeout_ms: Some(5_000),
            max_output_bytes: Some(4_000),
        });
        assert!(
            matches!(result.decision, SandboxPolicyDecision::Deny { .. }),
            "{:?}",
            result
        );
        let deny_reason = match &result.decision {
            SandboxPolicyDecision::Deny { reason } => reason.as_str(),
            _ => "",
        };
        assert!(
            deny_reason.contains("refusing")
                || deny_reason.contains("fails closed")
                || deny_reason.contains("network access is disabled"),
            "{:?}",
            result
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_restricted_token_background_adapter_runs_and_captures_output() {
        let status = current_sandbox_status();
        if !status
            .available_sandbox_types
            .contains(&SandboxType::WindowsRestrictedToken)
        {
            eprintln!(
                "skipping Windows restricted-token background smoke: {}",
                status.message
            );
            return;
        }

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("oppi-background-sandbox-{unique}"));
        fs::create_dir_all(&root).unwrap();
        let output = root.join("background.log");
        let stdout = fs::File::create(&output).unwrap();
        let stderr = stdout.try_clone().unwrap();
        let mut request = request(&root.display().to_string());
        request.command = "echo windows-background-sandbox".to_string();

        let (mut child, plan) = spawn_sandboxed_background(SandboxBackgroundSpawnParams {
            policy: SandboxPolicy {
                permission_profile: PermissionProfile {
                    mode: PermissionMode::FullAccess,
                    readable_roots: vec![root.display().to_string()],
                    writable_roots: vec![root.display().to_string()],
                    filesystem_rules: Vec::new(),
                    protected_patterns: Vec::new(),
                },
                network: NetworkPolicy::Enabled,
                filesystem: FilesystemPolicy::Unrestricted,
            },
            request,
            preference: SandboxPreference::Require,
            approval_granted: true,
            stdout,
            stderr,
            managed_network: None,
        })
        .unwrap();

        assert_eq!(plan.sandbox_type, SandboxType::WindowsRestrictedToken);
        let status = child.wait().unwrap();
        assert!(status.success());
        let captured = fs::read_to_string(&output).unwrap();
        assert!(captured.contains("windows-background-sandbox"));
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(windows)]
    #[test]
    fn windows_restricted_token_background_path_has_real_adapter() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("oppi-background-adapter-{unique}"));
        fs::create_dir_all(&root).unwrap();
        let output = root.join("background.log");
        let stdout = fs::File::create(&output).unwrap();
        let stderr = stdout.try_clone().unwrap();
        let mut request = request(&root.display().to_string());
        request.command = "echo windows-background-adapter".to_string();

        let result = spawn_sandboxed_background_with_manager(
            SandboxBackgroundSpawnParams {
                policy: SandboxPolicy {
                    permission_profile: PermissionProfile {
                        mode: PermissionMode::FullAccess,
                        readable_roots: vec![root.display().to_string()],
                        writable_roots: vec![root.display().to_string()],
                        filesystem_rules: Vec::new(),
                        protected_patterns: Vec::new(),
                    },
                    network: NetworkPolicy::Enabled,
                    filesystem: FilesystemPolicy::Unrestricted,
                },
                request,
                preference: SandboxPreference::Require,
                approval_granted: true,
                stdout,
                stderr,
                managed_network: None,
            },
            Some(SandboxManager::with_capabilities(
                "windows",
                vec![SandboxType::WindowsRestrictedToken],
            )),
        );

        match result {
            Ok((mut child, plan)) => {
                assert_eq!(plan.sandbox_type, SandboxType::WindowsRestrictedToken);
                let status = child.wait().unwrap();
                assert!(status.success());
            }
            Err(error) => {
                assert!(
                    !error.contains("background adapter"),
                    "Windows restricted-token background path must attempt the real adapter instead of returning the placeholder denial: {error}"
                );
            }
        }
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn linux_bubblewrap_transform_uses_readonly_root_writable_roots_and_network_namespace() {
        let plan = exec_plan(SandboxType::LinuxBubblewrap, NetworkPolicy::Disabled);
        let command = command_for_plan(&plan).unwrap();
        assert_eq!(command.get_program().to_string_lossy(), "bwrap");
        let args: Vec<String> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect();
        assert!(
            args.windows(3)
                .any(|window| window == ["--ro-bind", "/", "/"])
        );
        assert!(
            args.windows(3)
                .any(|window| window == ["--bind", "/repo", "/repo"])
        );
        assert!(args.iter().any(|arg| arg == "--unshare-net"));
        assert!(args.windows(2).any(|window| window == ["/bin/sh", "-c"]));
    }

    #[test]
    fn linux_bubblewrap_masks_explicit_none_rules_after_writable_binds() {
        let mut plan = exec_plan(SandboxType::LinuxBubblewrap, NetworkPolicy::Disabled);
        plan.filesystem_rules.push(FilesystemRule {
            root: FilesystemRoot::Path {
                path: "/repo/.git".to_string(),
            },
            access: FilesystemAccess::None,
        });
        plan.filesystem_rules.push(FilesystemRule {
            root: FilesystemRoot::Path {
                path: ".agents".to_string(),
            },
            access: FilesystemAccess::None,
        });
        plan.filesystem_rules.push(FilesystemRule {
            root: FilesystemRoot::Path {
                path: ".env*".to_string(),
            },
            access: FilesystemAccess::None,
        });
        let command = command_for_plan(&plan).unwrap();
        let args: Vec<String> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect();
        let bind_pos = args
            .windows(3)
            .position(|window| window == ["--bind", "/repo", "/repo"])
            .expect("writable bind present");
        let git_mask_pos = args
            .windows(2)
            .position(|window| window == ["--tmpfs", "/repo/.git"])
            .expect(".git mask present");
        assert!(
            git_mask_pos > bind_pos,
            "deny mask should override writable bind"
        );
        assert!(
            args.windows(2)
                .any(|window| window == ["--tmpfs", "/repo/.agents"])
        );
        assert!(
            !args
                .windows(2)
                .any(|window| window == ["--tmpfs", "/repo/.env*"])
        );
    }

    #[test]
    fn macos_seatbelt_transform_generates_deny_default_profile() {
        let plan = exec_plan(SandboxType::MacosSeatbelt, NetworkPolicy::Disabled);
        let profile = macos_seatbelt_profile(&plan);
        assert!(profile.contains("(deny default)"));
        assert!(profile.contains("(allow file-read*)"));
        assert!(!profile.contains("(allow network*)"));
    }

    #[test]
    fn macos_seatbelt_profile_denies_explicit_none_paths() {
        let mut plan = exec_plan(SandboxType::MacosSeatbelt, NetworkPolicy::Disabled);
        plan.filesystem_rules.push(FilesystemRule {
            root: FilesystemRoot::Path {
                path: "/repo/.codex".to_string(),
            },
            access: FilesystemAccess::None,
        });
        let profile = macos_seatbelt_profile(&plan);
        assert!(profile.contains("(deny file-read* file-write* (subpath \"/repo/.codex\"))"));
    }

    #[test]
    fn managed_network_proxy_environment_is_applied_when_network_is_allowed() {
        let mut plan = exec_plan(SandboxType::None, NetworkPolicy::Enabled);
        plan.managed_network = Some(ManagedNetworkConfig {
            http_proxy: Some("http://127.0.0.1:8080".to_string()),
            https_proxy: None,
            all_proxy: None,
            no_proxy: vec!["example.local".to_string()],
            allow_loopback: true,
        });
        let mut command = shell_command("echo hi").unwrap();
        apply_network_environment(&mut command, &plan);
        let env: Vec<(String, String)> = command
            .get_envs()
            .filter_map(|(key, value)| {
                value.map(|value| {
                    (
                        key.to_string_lossy().to_string(),
                        value.to_string_lossy().to_string(),
                    )
                })
            })
            .collect();
        assert!(
            env.iter()
                .any(|(key, value)| key == "HTTP_PROXY" && value == "http://127.0.0.1:8080")
        );
        assert!(
            env.iter()
                .any(|(key, value)| key == "NO_PROXY" && value.contains("localhost"))
        );
    }

    #[test]
    fn disabled_network_removes_proxy_environment_from_command() {
        let plan = exec_plan(SandboxType::None, NetworkPolicy::Disabled);
        let mut command = shell_command("echo hi").unwrap();
        command.env("HTTP_PROXY", "http://proxy");
        apply_network_environment(&mut command, &plan);
        assert!(
            command
                .get_envs()
                .any(|(key, value)| key == "HTTP_PROXY" && value.is_none())
        );
    }

    #[test]
    fn managed_network_fails_closed_without_proxy_only_enforcement() {
        let mut request = request(".");
        request.command = if cfg!(windows) {
            "echo managed".to_string()
        } else {
            "printf managed".to_string()
        };
        let result = execute_sandboxed(SandboxExecParams {
            policy: SandboxPolicy {
                permission_profile: PermissionProfile {
                    mode: PermissionMode::FullAccess,
                    readable_roots: vec![".".to_string()],
                    writable_roots: vec![".".to_string()],
                    filesystem_rules: Vec::new(),
                    protected_patterns: Vec::new(),
                },
                network: NetworkPolicy::Enabled,
                filesystem: FilesystemPolicy::Unrestricted,
            },
            request,
            preference: SandboxPreference::Forbid,
            approval_granted: false,
            managed_network: Some(ManagedNetworkConfig {
                http_proxy: Some("http://127.0.0.1:8080".to_string()),
                https_proxy: None,
                all_proxy: None,
                no_proxy: Vec::new(),
                allow_loopback: true,
            }),
            timeout_ms: Some(5_000),
            max_output_bytes: Some(1_000),
        });
        assert!(
            matches!(result.decision, SandboxPolicyDecision::Deny { .. }),
            "{result:?}"
        );
        assert!(result.audit.iter().any(|record| {
            record.action == "fail-closed"
                && record
                    .reason
                    .as_deref()
                    .is_some_and(|reason| reason.contains("proxy-only"))
        }));
        assert!(result.stdout.is_empty());
    }

    #[test]
    fn approval_granted_allows_ask_decision_to_execute() {
        let mut request = request(".");
        request.command = if cfg!(windows) {
            "echo approved".to_string()
        } else {
            "printf approved".to_string()
        };
        request.writes_files = true;
        let result = execute_sandboxed(SandboxExecParams {
            policy: SandboxPolicy {
                permission_profile: PermissionProfile {
                    mode: PermissionMode::AutoReview,
                    readable_roots: vec![".".to_string()],
                    writable_roots: vec![".".to_string()],
                    filesystem_rules: Vec::new(),
                    protected_patterns: Vec::new(),
                },
                network: NetworkPolicy::Enabled,
                filesystem: FilesystemPolicy::Unrestricted,
            },
            request,
            preference: SandboxPreference::Forbid,
            approval_granted: true,
            managed_network: None,
            timeout_ms: Some(5_000),
            max_output_bytes: Some(1_000),
        });
        assert!(
            matches!(result.decision, SandboxPolicyDecision::Allow),
            "{:?}",
            result
        );
        assert!(result.stdout.contains("approved"));
        assert!(
            result
                .audit
                .iter()
                .any(|record| record.action == "approved")
        );
    }

    #[test]
    fn ask_decision_without_approval_is_not_executed() {
        let mut request = request(".");
        request.command = if cfg!(windows) {
            "echo blocked".to_string()
        } else {
            "printf blocked".to_string()
        };
        request.writes_files = true;
        let result = execute_sandboxed(SandboxExecParams {
            policy: SandboxPolicy {
                permission_profile: PermissionProfile {
                    mode: PermissionMode::AutoReview,
                    readable_roots: vec![".".to_string()],
                    writable_roots: vec![".".to_string()],
                    filesystem_rules: Vec::new(),
                    protected_patterns: Vec::new(),
                },
                network: NetworkPolicy::Enabled,
                filesystem: FilesystemPolicy::Unrestricted,
            },
            request,
            preference: SandboxPreference::Forbid,
            approval_granted: false,
            managed_network: None,
            timeout_ms: Some(5_000),
            max_output_bytes: Some(1_000),
        });
        assert!(
            matches!(result.decision, SandboxPolicyDecision::Deny { .. }),
            "{:?}",
            result
        );
        assert!(!result.stdout.contains("blocked"));
    }

    #[test]
    fn full_access_exec_captures_output_with_bounded_audit() {
        let mut request = request(".");
        request.command = if cfg!(windows) {
            "echo hello TOKEN=super-secret".to_string()
        } else {
            "printf 'hello TOKEN=super-secret'".to_string()
        };
        let result = execute_sandboxed(SandboxExecParams {
            policy: default_policy(profile(PermissionMode::FullAccess)),
            request,
            preference: SandboxPreference::Forbid,
            approval_granted: false,
            managed_network: None,
            timeout_ms: Some(5_000),
            max_output_bytes: Some(1_000),
        });
        assert!(matches!(result.decision, SandboxPolicyDecision::Allow));
        assert!(result.stdout.contains("hello"));
        assert!(
            result
                .audit
                .iter()
                .all(|record| !record.command.contains("super-secret"))
        );
    }

    #[test]
    #[ignore = "host integration: requires Linux with bubblewrap installed"]
    fn host_linux_bubblewrap_blocks_network_when_disabled() {
        if std::env::consts::OS != "linux" || !command_exists("bwrap") {
            return;
        }
        let root = std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .to_string_lossy()
            .to_string();
        let mut request = request(&root);
        request.command = "printf 'oppi-bwrap-host-ok\\n'".to_string();
        let result = execute_sandboxed(SandboxExecParams {
            policy: SandboxPolicy {
                permission_profile: PermissionProfile {
                    mode: PermissionMode::FullAccess,
                    readable_roots: vec![root.clone()],
                    writable_roots: vec![root],
                    filesystem_rules: Vec::new(),
                    protected_patterns: Vec::new(),
                },
                network: NetworkPolicy::Disabled,
                filesystem: FilesystemPolicy::Unrestricted,
            },
            request,
            preference: SandboxPreference::Require,
            approval_granted: false,
            managed_network: None,
            timeout_ms: Some(5_000),
            max_output_bytes: Some(1_000),
        });
        assert!(matches!(result.decision, SandboxPolicyDecision::Allow));
        assert_eq!(
            result.plan.unwrap().sandbox_type,
            SandboxType::LinuxBubblewrap
        );
        assert_eq!(result.exit_code, Some(0), "stderr: {}", result.stderr);
        assert!(!result.timed_out);
        assert!(result.stdout.contains("oppi-bwrap-host-ok"));
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "host integration: requires configured OPPI_WINDOWS_SANDBOX_USERNAME/PASSWORD and WFP_READY=1"]
    fn host_windows_restricted_token_wfp_smoke_runs_with_configured_sandbox_account() {
        if std::env::var("OPPI_WINDOWS_SANDBOX_USERNAME").is_err()
            || std::env::var("OPPI_WINDOWS_SANDBOX_PASSWORD").is_err()
            || std::env::var("OPPI_WINDOWS_SANDBOX_WFP_READY").as_deref() != Ok("1")
        {
            return;
        }
        let mut request = request(".");
        request.command = "echo windows-wfp-smoke".to_string();
        let result = execute_sandboxed(SandboxExecParams {
            policy: SandboxPolicy {
                permission_profile: PermissionProfile {
                    mode: PermissionMode::FullAccess,
                    readable_roots: vec![".".to_string()],
                    writable_roots: vec![".".to_string()],
                    filesystem_rules: Vec::new(),
                    protected_patterns: Vec::new(),
                },
                network: NetworkPolicy::Disabled,
                filesystem: FilesystemPolicy::Unrestricted,
            },
            request,
            preference: SandboxPreference::Require,
            approval_granted: false,
            managed_network: None,
            timeout_ms: Some(10_000),
            max_output_bytes: Some(4_000),
        });
        assert!(
            matches!(result.decision, SandboxPolicyDecision::Allow),
            "{:?}",
            result
        );
        assert!(result.stdout.contains("windows-wfp-smoke"));
    }

    #[test]
    #[ignore = "host integration: requires macOS with sandbox-exec available"]
    fn host_macos_seatbelt_executes_basic_command() {
        if std::env::consts::OS != "macos" || !Path::new("/usr/bin/sandbox-exec").exists() {
            return;
        }
        let mut request = request(".");
        request.command = "echo seatbelt".to_string();
        let result = execute_sandboxed(SandboxExecParams {
            policy: SandboxPolicy {
                permission_profile: PermissionProfile {
                    mode: PermissionMode::FullAccess,
                    readable_roots: vec![".".to_string()],
                    writable_roots: vec![".".to_string()],
                    filesystem_rules: Vec::new(),
                    protected_patterns: Vec::new(),
                },
                network: NetworkPolicy::Enabled,
                filesystem: FilesystemPolicy::Unrestricted,
            },
            request,
            preference: SandboxPreference::Require,
            approval_granted: false,
            managed_network: None,
            timeout_ms: Some(5_000),
            max_output_bytes: Some(1_000),
        });
        assert!(matches!(result.decision, SandboxPolicyDecision::Allow));
        assert!(result.stdout.contains("seatbelt"));
    }

    #[cfg(unix)]
    #[test]
    fn symlink_escape_is_rejected_by_existing_prefix_canonicalization() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let base = std::env::temp_dir().join(format!("oppi-sandbox-test-{unique}"));
        let repo = base.join("repo");
        let outside = base.join("outside");
        fs::create_dir_all(&repo).unwrap();
        fs::create_dir_all(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, repo.join("link-out")).unwrap();

        let roots = vec![repo.to_string_lossy().to_string()];
        assert!(!path_is_within_any_root(
            &repo.join("link-out/secret.txt"),
            &roots,
            None
        ));

        let _ = fs::remove_dir_all(base);
    }
}
