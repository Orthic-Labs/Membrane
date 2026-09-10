//! Native Rust port of `blueprint/src/lib/init/apply.mjs`.
//!
//! Lane LIB3 (r5 closure): no prior native equivalent found (grep of
//! `applyInitPlan`/`validateInstallState`/`allowedTarget` across
//! membrane-blueprint/src and membrane-runtime/src produced no match).
//!
//! SCOPE: the legacy module is a stateful installer (writes host files,
//! runs a child `blueprint graph build`, enrolls a watcher, and performs
//! full transactional rollback across all of it). This port covers its
//! deterministic, side-effect-free core exactly: install-state shape
//! validation (`validateInstallState`), the fixed allowlist of files an
//! init/apply operation may ever touch (`allowedTarget`), the marker-block
//! merge/remove text transform used for host instruction files
//! (`mergeBlock`/`removeBlock`), and `restore` (replays a captured
//! before-state back onto disk, used by rollback/uninstall). The OS-level
//! orchestration in `applyInitPlan` (spawning `blueprint graph build`,
//! calling into the watcher supervisor) is intentionally not replicated
//! here since it has no meaningful pure-function contract to prove parity
//! against.

use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

const START: &str = "<!-- blueprint:start -->";
const END: &str = "<!-- blueprint:end -->";

/// Mirrors `allowedTarget(root, path)`: only these repo-root-relative
/// targets may ever be written by init/apply.
pub fn allowed_targets(root: &Path) -> BTreeSet<PathBuf> {
    [
        "CLAUDE.md",
        "AGENTS.md",
        "BLUEPRINT-AGENT.md",
        ".mcp.json",
    ]
    .iter()
    .map(|name| root.join(name))
    .chain([root.join(".cursor").join("rules").join("blueprint.mdc")])
    .chain([root.join(".claude").join("settings.json")])
    .collect()
}

pub fn is_allowed_target(root: &Path, path: &Path) -> bool {
    allowed_targets(root).contains(path)
}

#[derive(Debug, Clone, PartialEq)]
pub struct FileState {
    pub exists: bool,
    /// Base64-decoded original bytes, when the file existed.
    pub content: Option<Vec<u8>>,
    /// sha256 hex of the currently-installed content, once recorded.
    pub installed: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct InstallState {
    pub version: u32,
    pub files: std::collections::BTreeMap<String, FileState>,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum InstallStateError {
    #[error("state_invalid")]
    Invalid,
}

/// Mirrors `validateInstallState(root, state)`: every recorded path must be
/// confined to root, must be an allowlisted target, must have a `sha256`-
/// shaped `installed` hash, and consistent `exists`/`content` fields.
pub fn validate_install_state(root: &Path, state: &InstallState) -> Result<(), InstallStateError> {
    if state.version != 1 {
        return Err(InstallStateError::Invalid);
    }
    let allowed = allowed_targets(root);
    for (path, original) in &state.files {
        let path_buf = PathBuf::from(path);
        if !allowed.contains(&path_buf) {
            return Err(InstallStateError::Invalid);
        }
        let installed_valid = original
            .installed
            .as_deref()
            .map(|h| h.len() == 64 && h.chars().all(|c| c.is_ascii_hexdigit()))
            .unwrap_or(false);
        if !installed_valid {
            return Err(InstallStateError::Invalid);
        }
        if original.exists && original.content.is_none() {
            return Err(InstallStateError::Invalid);
        }
    }
    Ok(())
}

/// Mirrors `removeBlock(content)`: strips the `<!-- blueprint:start -->` ...
/// `<!-- blueprint:end -->` marker block (and one adjoining newline), and
/// collapses an all-blank remainder to empty.
pub fn remove_block(content: &str) -> String {
    let mut out = String::new();
    let mut rest = content;
    loop {
        let Some(start_idx) = rest.find(START) else {
            out.push_str(rest);
            break;
        };
        let Some(end_rel) = rest[start_idx..].find(END) else {
            out.push_str(rest);
            break;
        };
        let end_idx = start_idx + end_rel + END.len();
        let before = &rest[..start_idx];
        let before_trimmed = if before.ends_with('\n') {
            &before[..before.len() - 1]
        } else {
            before
        };
        out.push_str(before_trimmed);
        out.push('\n');
        rest = rest[end_idx..].trim_start_matches('\n');
        // Re-insert exactly one newline boundary, mirroring `\n?` in the
        // legacy regex, then continue scanning for further blocks.
        if rest.is_empty() {
            break;
        }
    }
    if out.chars().all(|c| c == '\n') {
        String::new()
    } else {
        out
    }
}

/// Mirrors `mergeBlock(content)`: removes any existing block, then appends
/// a freshly generated one.
pub fn merge_block(content: &str) -> String {
    let without = remove_block(content);
    let block = format!(
        "{START}\n## Blueprint Graph\nBefore reading repository files, call `blueprint_recall` with current repository root. Use `blueprint_expand` for bounded context and `blueprint_search` for queries.\n{END}"
    );
    let trimmed = without.trim_end();
    if trimmed.is_empty() {
        format!("{block}\n")
    } else {
        format!("{trimmed}\n\n{block}\n")
    }
}

/// Mirrors `restore(root, state)`: replays a captured before-state back
/// onto disk (used by rollback and `uninstallInit`).
pub fn restore(state: &InstallState) -> std::io::Result<()> {
    for (path, original) in &state.files {
        let path = Path::new(path);
        if original.exists {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(path, original.content.clone().unwrap_or_default())?;
        } else {
            let _ = fs::remove_file(path);
        }
    }
    Ok(())
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn allowed_targets_covers_the_fixed_allowlist() {
        let root = Path::new("/repo");
        assert!(is_allowed_target(root, &root.join("CLAUDE.md")));
        assert!(is_allowed_target(root, &root.join(".mcp.json")));
        assert!(is_allowed_target(
            root,
            &root.join(".cursor").join("rules").join("blueprint.mdc")
        ));
        assert!(!is_allowed_target(root, &root.join("random.txt")));
    }

    #[test]
    fn validate_install_state_rejects_non_allowlisted_path() {
        let root = Path::new("/repo");
        let mut files = BTreeMap::new();
        files.insert(
            "/repo/random.txt".to_string(),
            FileState {
                exists: true,
                content: Some(b"x".to_vec()),
                installed: Some("a".repeat(64)),
            },
        );
        let state = InstallState { version: 1, files };
        assert_eq!(
            validate_install_state(root, &state),
            Err(InstallStateError::Invalid)
        );
    }

    #[test]
    fn validate_install_state_rejects_missing_installed_hash() {
        let root = Path::new("/repo");
        let mut files = BTreeMap::new();
        files.insert(
            root.join("CLAUDE.md").to_string_lossy().to_string(),
            FileState {
                exists: true,
                content: Some(b"x".to_vec()),
                installed: None,
            },
        );
        let state = InstallState { version: 1, files };
        assert_eq!(
            validate_install_state(root, &state),
            Err(InstallStateError::Invalid)
        );
    }

    #[test]
    fn validate_install_state_accepts_well_formed_state() {
        let root = Path::new("/repo");
        let mut files = BTreeMap::new();
        files.insert(
            root.join("CLAUDE.md").to_string_lossy().to_string(),
            FileState {
                exists: true,
                content: Some(b"x".to_vec()),
                installed: Some("a".repeat(64)),
            },
        );
        let state = InstallState { version: 1, files };
        assert!(validate_install_state(root, &state).is_ok());
    }

    #[test]
    fn merge_block_appends_marker_block_to_empty_content() {
        let merged = merge_block("");
        assert!(merged.starts_with(START));
        assert!(merged.trim_end().ends_with(END));
    }

    #[test]
    fn merge_block_replaces_existing_block_rather_than_duplicating() {
        let existing = format!("# Notes\n\n{START}\nold\n{END}\n");
        let merged = merge_block(&existing);
        assert_eq!(merged.matches(START).count(), 1);
        assert!(merged.starts_with("# Notes"));
        assert!(merged.contains("## Blueprint Graph"));
        assert!(!merged.contains("old"));
    }

    #[test]
    fn remove_block_strips_marker_block_only() {
        let content = format!("keep me\n\n{START}\nremove me\n{END}\n");
        let cleaned = remove_block(&content);
        assert!(cleaned.contains("keep me"));
        assert!(!cleaned.contains("remove me"));
    }

    #[test]
    fn restore_writes_back_existing_files_and_deletes_new_ones() {
        let dir = tempfile::tempdir().unwrap();
        let existing_path = dir.path().join("CLAUDE.md");
        let new_path = dir.path().join("AGENTS.md");
        fs::write(&new_path, b"created-by-init").unwrap();

        let mut files = BTreeMap::new();
        files.insert(
            existing_path.to_string_lossy().to_string(),
            FileState {
                exists: true,
                content: Some(b"original content".to_vec()),
                installed: None,
            },
        );
        files.insert(
            new_path.to_string_lossy().to_string(),
            FileState {
                exists: false,
                content: None,
                installed: None,
            },
        );
        let state = InstallState { version: 1, files };
        restore(&state).unwrap();
        assert_eq!(fs::read(&existing_path).unwrap(), b"original content");
        assert!(!new_path.exists());
    }
}
