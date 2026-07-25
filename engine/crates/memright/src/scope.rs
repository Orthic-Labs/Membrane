//! Multi-project scope model — a faithful Rust port of `mem.py`'s scope functions.
//!
//! A project's `scope_id` is Claude's own project-slug: each `:` `\` `/` in the cwd becomes `-`,
//! with NO collapsing, and the leading Windows drive letter normalized to uppercase so a
//! lowercase-drive cwd can't fork a project's memories into a second scope. Recall is chain-scoped
//! (self + ancestors that exist + global), never siblings.

use std::path::{Path, PathBuf};

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

pub(crate) fn workspace_root() -> Option<PathBuf> {
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
            "memright-workspace-root-{}-{unique}",
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
}
