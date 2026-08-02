//! Multi-project scope model — a faithful Rust port of `mem.py`'s scope functions.
//!
//! A project's `scope_id` is Claude's own project-slug: each `:` `\` `/` in the cwd becomes `-`,
//! with NO collapsing, and the leading Windows drive letter normalized to uppercase so a
//! lowercase-drive cwd can't fork a project's memories into a second scope. Recall is chain-scoped
//! (self + ancestors that exist + global), never siblings.

use std::path::{Path, PathBuf};

pub const MAX_VIRTUAL_SCOPE_DEPTH: usize = 8;

/// Explicit caller scope. Legacy strings remain filesystem-only; virtual scopes must use this
/// tagged form so punctuation in an opaque id can never create path-like ancestors.
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum ScopeDescriptorV1 {
    #[serde(rename = "filesystem")]
    Filesystem { path: String },
    #[serde(rename = "virtual")]
    Virtual {
        id: String,
        #[serde(rename = "tenant_id")]
        tenant_id: String,
        #[serde(default)]
        parents: Vec<String>,
        #[serde(default, rename = "inherit_global")]
        inherit_global: bool,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ScopeDescriptorError {
    #[error("scope descriptor path is required")]
    MissingPath,
    #[error("virtual scope id and tenant_id are required")]
    MissingVirtualIdentity,
    #[error("virtual scope parent chain exceeds depth 8")]
    Depth,
    #[error("virtual scopes cannot inherit global")]
    GlobalInheritance,
    #[error("virtual scope parent is invalid")]
    InvalidParent,
    #[error("virtual scope parent chain contains a duplicate or cycle")]
    Cycle,
    #[error("virtual scope parent is not registered for this tenant")]
    UnknownParent,
}

fn virtual_scope_key(tenant_id: &str, id: &str) -> String {
    format!("virtual:{tenant_id}:{id}")
}

impl ScopeDescriptorV1 {
    pub fn filesystem(path: impl Into<String>) -> Self {
        Self::Filesystem { path: path.into() }
    }

    pub fn virtual_scope(
        id: impl Into<String>,
        tenant_id: impl Into<String>,
        parents: Vec<String>,
        inherit_global: bool,
    ) -> Self {
        Self::Virtual {
            id: id.into(),
            tenant_id: tenant_id.into(),
            parents,
            inherit_global,
        }
    }

    /// Resolve a descriptor into exact recall scope ids. Virtual parents are opaque ids in the
    /// same tenant, ordered nearest-first; they are never slugged, split, or given implicit global.
    pub fn resolve_chain(&self, existing: &[String]) -> Result<Vec<String>, ScopeDescriptorError> {
        match self {
            Self::Filesystem { path } => {
                if path.trim().is_empty() {
                    return Err(ScopeDescriptorError::MissingPath);
                }
                Ok(canonical_scope_chain(path, existing))
            }
            Self::Virtual {
                id,
                tenant_id,
                parents,
                inherit_global,
            } => {
                if id.trim().is_empty() || tenant_id.trim().is_empty() {
                    return Err(ScopeDescriptorError::MissingVirtualIdentity);
                }
                if parents.len() + 1 > MAX_VIRTUAL_SCOPE_DEPTH {
                    return Err(ScopeDescriptorError::Depth);
                }
                if *inherit_global {
                    return Err(ScopeDescriptorError::GlobalInheritance);
                }
                let own = virtual_scope_key(tenant_id, id);
                let mut seen = std::collections::HashSet::from([own.clone()]);
                let mut chain = vec![own];
                for parent in parents {
                    if parent.trim().is_empty() || parent.starts_with("virtual:") {
                        return Err(ScopeDescriptorError::InvalidParent);
                    }
                    let key = virtual_scope_key(tenant_id, parent);
                    if !seen.insert(key.clone()) {
                        return Err(ScopeDescriptorError::Cycle);
                    }
                    if !existing.iter().any(|scope| scope == &key) {
                        return Err(ScopeDescriptorError::UnknownParent);
                    }
                    chain.push(key);
                }
                Ok(chain)
            }
        }
    }
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct RightContextWorkspaceConfig {
    schema_version: u32,
    workspace_root: PathBuf,
    python_executable: PathBuf,
}

fn validated_workspace_root(value: &str) -> Option<PathBuf> {
    let path = PathBuf::from(value.trim());
    (path.is_absolute() && path.is_dir())
        .then(|| path.canonicalize().ok())
        .flatten()
}

pub(crate) fn workspace_root_from(
    explicit: Option<&str>,
    config_path: Option<&Path>,
) -> Option<PathBuf> {
    if let Some(value) = explicit.filter(|value| !value.trim().is_empty()) {
        return validated_workspace_root(value);
    }
    let payload = std::fs::read(config_path?).ok()?;
    let config: RightContextWorkspaceConfig = serde_json::from_slice(&payload).ok()?;
    if config.schema_version != 2
        || !config.python_executable.is_absolute()
        || !config.python_executable.is_file()
    {
        return None;
    }
    validated_workspace_root(config.workspace_root.to_str()?)
}

pub fn workspace_root() -> Option<PathBuf> {
    let explicit = std::env::var("WORKSPACE_ROOT").ok();
    let configured = std::env::var("RIGHTCONTEXT_WORKSPACE_CONFIG")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .or_else(|| std::env::var_os("USERPROFILE"))
                .map(PathBuf::from)
                .map(|home| home.join(".config/rightcontext/workspace.json"))
        });
    workspace_root_from(explicit.as_deref(), configured.as_deref())
}

/// Uppercase a leading Windows drive token so `d--Claude-x` and `D--Claude-x` never fork.
/// The slug form of a drive is one letter followed by `--` (from `d:` -> `d-`, joined with the next `-`).
pub fn normalize_scope(s: &str) -> String {
    let b = s.as_bytes();
    if b.len() >= 2 && b[1] == b'-' && b[0].is_ascii_alphabetic() && b[0].is_ascii_lowercase() {
        let mut out = String::with_capacity(s.len());
        out.push((b[0] as char).to_ascii_uppercase());
        out.push_str(&s[1..]);
        out
    } else {
        s.to_string()
    }
}

/// Claude's project-slug: each `:` `\` `/` -> `-`, NO collapsing, drive letter normalized.
/// `D:\Claude\myproject` and `d:\Claude\myproject` both -> `D--Claude-myproject`.
pub fn path_to_scope(cwd: &str) -> String {
    let cwd = cwd.strip_prefix(r"\\?\").unwrap_or(cwd);
    let slug: String = cwd
        .chars()
        .map(|c| {
            if c == ':' || c == '\\' || c == '/' {
                '-'
            } else {
                c
            }
        })
        .collect();
    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        return "workspace".to_string();
    }
    normalize_scope(slug)
}

/// A scope sees itself + its ancestors (path-slug prefixes that actually hold rows) + `global`,
/// never siblings. `existing` is the set of scope_ids present in the store. Self always leads, even
/// if it has no rows yet. `D--Claude-coderight` -> `[D--Claude-coderight, D--Claude, global]`.
pub fn scope_chain(scope_id: &str, existing: &[String]) -> Vec<String> {
    let parts: Vec<&str> = scope_id.split('-').collect();
    // prefixes: longest (self) down to the first segment
    let mut candidates: Vec<String> = Vec::with_capacity(parts.len() + 2);
    candidates.push(scope_id.to_string());
    for i in (1..=parts.len()).rev() {
        candidates.push(parts[..i].join("-"));
    }
    let has = |p: &str| existing.iter().any(|e| e == p);
    let mut chain: Vec<String> = Vec::new();
    for p in candidates {
        if chain.contains(&p) {
            continue;
        }
        if p == scope_id || has(&p) {
            chain.push(p);
        }
    }
    if has("global") && !chain.iter().any(|c| c == "global") {
        chain.push("global".to_string());
    }
    chain
}

/// THE one canonical entry point for a caller-supplied scope string (2026-07-16, Sol audit P0).
/// Clients send whatever they have — a filesystem path (`D:\Claude\heardright`, `/Users/x/claude`),
/// an already-slugged scope (`D--Claude`), or `global` — and every retrieval surface must resolve
/// it identically. Before this existed, the federation memory lane passed raw paths straight into
/// `recall_scored`, which matched NO project scope rows: the rich path silently recalled from the
/// global corpus only. Path detection keys on separators that can never appear in a slug.
pub fn canonical_scope_chain(raw: &str, existing: &[String]) -> Vec<String> {
    let raw = raw.trim();
    let slug = if raw.contains(':') || raw.contains('\\') || raw.contains('/') {
        path_to_scope(raw)
    } else if raw.is_empty() {
        "global".to_string()
    } else {
        normalize_scope(raw)
    };
    scope_chain(&slug, existing)
}

/// Windows drive cwd slug: `D--Claude` / `D--Claude-heardright` (drive letter + `--` + rest).
fn is_windows_drive_slug(scope: &str) -> bool {
    let b = scope.as_bytes();
    b.len() >= 4
        && b[0].is_ascii_alphabetic()
        && b[1] == b'-'
        && b[2] == b'-'
        && b[3].is_ascii_alphanumeric()
}

/// Unix-style cwd slug from path_to_scope: `Users-adrdsouza-claude`, `Volumes-D-claude`.
/// Requires a known absolute-root first segment, or ≥3 non-empty hyphen segments.
fn is_unix_path_slug(scope: &str) -> bool {
    let segments: Vec<&str> = scope.split('-').filter(|s| !s.is_empty()).collect();
    if segments.len() < 2 {
        return false;
    }
    let first = segments[0];
    let known_root = matches!(
        first,
        "Users" | "home" | "Volumes" | "private" | "var" | "tmp" | "opt" | "root"
    );
    known_root || segments.len() >= 3
}

/// Write-path scope gate (P1 scope-typo hardening).
///
/// Allows `proposed` / `global` / `workspace`, filesystem paths, Windows drive slugs
/// (`D--Claude-…`), Unix cwd slugs (`Users-…` / multi-segment path slugs), and typed
/// `virtual:…` ids. Refuses single-token hand-typed names like `heardright` that silently
/// fork the corpus (2026-07-05 mirror incident).
pub fn validate_write_scope(raw: &str) -> Result<(), String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("scope must be non-empty".into());
    }
    if matches!(raw, "proposed" | "global" | "workspace") {
        return Ok(());
    }
    if raw.starts_with("virtual:") {
        return Ok(());
    }
    // Paths always OK — callers normalize via path_to_scope / normalize_scope.
    if raw.contains('/') || raw.contains('\\') || raw.contains(':') {
        return Ok(());
    }
    let normalized = normalize_scope(raw);
    if is_windows_drive_slug(&normalized) || is_unix_path_slug(&normalized) {
        return Ok(());
    }
    Err(format!(
        "refusing hand-typed scope {raw:?}: use proposed, global, a cwd path, or a cwd slug \
         (e.g. D--Claude-heardright / Users-adrdsouza-claude); a novel single-token scope forks the corpus"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_installation() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "crypt-workspace-root-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn workspace_root_accepts_arbitrary_explicit_installation() {
        let root = temp_installation();
        assert_eq!(
            workspace_root_from(root.to_str(), None),
            Some(root.canonicalize().unwrap())
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn workspace_root_reads_only_strict_v2_config() {
        let root = temp_installation();
        let python = root.join("python-custom");
        std::fs::write(&python, b"").unwrap();
        let config_path = root.join("workspace.json");
        std::fs::write(
            &config_path,
            serde_json::json!({
                "schemaVersion": 2,
                "workspaceRoot": root,
                "pythonExecutable": python,
            })
            .to_string(),
        )
        .unwrap();
        assert_eq!(
            workspace_root_from(None, Some(&config_path)),
            Some(root.canonicalize().unwrap())
        );

        std::fs::write(
            &config_path,
            serde_json::json!({
                "schemaVersion": 2,
                "workspaceRoot": root,
                "pythonExecutable": python,
                "hostname": "must-not-be-accepted",
            })
            .to_string(),
        )
        .unwrap();
        assert_eq!(workspace_root_from(None, Some(&config_path)), None);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn canonical_chain_resolves_windows_path_to_project_scopes() {
        let existing = vec![
            "D--Claude".to_string(),
            "D--Claude-heardright".to_string(),
            "global".to_string(),
        ];
        assert_eq!(
            canonical_scope_chain("D:\\Claude", &existing),
            vec!["D--Claude".to_string(), "global".to_string()]
        );
        assert_eq!(
            canonical_scope_chain("D:\\Claude\\heardright", &existing),
            vec![
                "D--Claude-heardright".to_string(),
                "D--Claude".to_string(),
                "global".to_string()
            ]
        );
    }

    #[test]
    fn canonical_chain_passes_through_slugs_and_unix_paths() {
        let existing = vec![
            "D--Claude".to_string(),
            "Users-x-claude".to_string(),
            "global".to_string(),
        ];
        assert_eq!(
            canonical_scope_chain("D--Claude", &existing),
            vec!["D--Claude".to_string(), "global".to_string()]
        );
        assert_eq!(
            canonical_scope_chain("/Users/x/claude", &existing),
            vec!["Users-x-claude".to_string(), "global".to_string()]
        );
        assert_eq!(
            canonical_scope_chain("global", &existing),
            vec!["global".to_string()]
        );
        assert_eq!(
            canonical_scope_chain("", &existing),
            vec!["global".to_string()]
        );
    }

    #[test]
    fn drive_letter_is_normalized() {
        assert_eq!(path_to_scope(r"D:\Claude\myproject"), "D--Claude-myproject");
        assert_eq!(path_to_scope(r"d:\Claude\myproject"), "D--Claude-myproject");
        assert_eq!(
            path_to_scope(r"\\?\D:\Claude\myproject"),
            "D--Claude-myproject"
        );
        assert_eq!(
            path_to_scope("/Users/adrdsouza/claude"),
            "Users-adrdsouza-claude"
        );
        assert_eq!(path_to_scope(""), "workspace");
        assert_eq!(
            normalize_scope("d--Claude-mailright"),
            "D--Claude-mailright"
        );
        assert_eq!(normalize_scope("workspace"), "workspace");
        assert_eq!(normalize_scope("global"), "global");
    }

    #[test]
    fn write_scope_allows_reserved_paths_and_cwd_slugs() {
        assert!(validate_write_scope("proposed").is_ok());
        assert!(validate_write_scope("global").is_ok());
        assert!(validate_write_scope("workspace").is_ok());
        assert!(validate_write_scope("D--Claude-heardright").is_ok());
        assert!(validate_write_scope("d--Claude").is_ok());
        assert!(validate_write_scope("Users-adrdsouza-claude").is_ok());
        assert!(validate_write_scope("Volumes-D-claude").is_ok());
        assert!(validate_write_scope(r"D:\Claude\heardright").is_ok());
        assert!(validate_write_scope("/Users/x/claude").is_ok());
        assert!(validate_write_scope("virtual:tenant-a:thread:abc").is_ok());
    }

    #[test]
    fn write_scope_refuses_hand_typed_single_tokens() {
        let err = validate_write_scope("heardright").unwrap_err();
        assert!(err.contains("refusing hand-typed scope"), "{err}");
        assert!(validate_write_scope("mailright").is_err());
        assert!(validate_write_scope("foo").is_err());
        assert!(validate_write_scope("my-project").is_err());
        assert!(validate_write_scope("").is_err());
    }

    #[test]
    fn chain_includes_ancestors_and_global_not_siblings() {
        let existing = vec![
            "D--Claude-myproject".to_string(),
            "D--Claude".to_string(),
            "D--Claude-otherproject".to_string(),
            "global".to_string(),
        ];
        let chain = scope_chain("D--Claude-myproject", &existing);
        assert_eq!(chain, vec!["D--Claude-myproject", "D--Claude", "global"]);
        // sibling never leaks in
        assert!(!chain.iter().any(|s| s == "D--Claude-otherproject"));
    }

    #[test]
    fn self_leads_even_when_empty() {
        let existing = vec!["D--Claude".to_string(), "global".to_string()];
        let chain = scope_chain("D--Claude-newproj", &existing);
        assert_eq!(chain.first().unwrap(), "D--Claude-newproj");
        assert!(chain.iter().any(|s| s == "D--Claude"));
        assert!(chain.iter().any(|s| s == "global"));
    }

    #[test]
    fn virtual_scope_is_opaque_and_uses_only_explicit_same_tenant_parents() {
        let existing = vec![
            "virtual:tenant-a:parent-1".to_string(),
            "virtual:tenant-a:root".to_string(),
            "virtual:tenant-b:parent-1".to_string(),
            "global".to_string(),
        ];
        let descriptor = ScopeDescriptorV1::virtual_scope(
            "thread:abc-123",
            "tenant-a",
            vec!["parent-1".to_string(), "root".to_string()],
            false,
        );
        assert_eq!(
            descriptor.resolve_chain(&existing).unwrap(),
            vec![
                "virtual:tenant-a:thread:abc-123".to_string(),
                "virtual:tenant-a:parent-1".to_string(),
                "virtual:tenant-a:root".to_string(),
            ]
        );
    }

    #[test]
    fn virtual_scope_rejects_cross_tenant_unknown_duplicate_and_excessive_parents() {
        let existing = vec!["virtual:tenant-a:parent".to_string(), "global".to_string()];
        assert_eq!(
            ScopeDescriptorV1::virtual_scope(
                "thread",
                "tenant-a",
                vec!["virtual:tenant-b:parent".to_string()],
                false
            )
            .resolve_chain(&existing)
            .unwrap_err(),
            ScopeDescriptorError::InvalidParent,
        );
        assert_eq!(
            ScopeDescriptorV1::virtual_scope(
                "thread",
                "tenant-a",
                vec!["missing".to_string()],
                false
            )
            .resolve_chain(&existing)
            .unwrap_err(),
            ScopeDescriptorError::UnknownParent,
        );
        assert_eq!(
            ScopeDescriptorV1::virtual_scope(
                "thread",
                "tenant-a",
                vec!["parent".to_string(), "parent".to_string()],
                false
            )
            .resolve_chain(&existing)
            .unwrap_err(),
            ScopeDescriptorError::Cycle,
        );
        assert_eq!(
            ScopeDescriptorV1::virtual_scope(
                "thread",
                "tenant-a",
                vec!["parent".to_string(); 8],
                false
            )
            .resolve_chain(&existing)
            .unwrap_err(),
            ScopeDescriptorError::Depth,
        );
        assert_eq!(
            ScopeDescriptorV1::virtual_scope("thread", "tenant-a", vec![], true)
                .resolve_chain(&existing)
                .unwrap_err(),
            ScopeDescriptorError::GlobalInheritance,
        );
    }
}
