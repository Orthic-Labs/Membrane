//! Native Rust port of `blueprint/src/lib/path-confinement.mjs`.
//!
//! Lane LIB1 (r5 closure): this module has no prior native equivalent.
//! Ported behavior:
//!   - `resolve_physical_path`: resolve `path` to its physical location,
//!     following symlinks in the nearest existing ancestor while keeping the
//!     (possibly not-yet-existing) tail intact. Returns `None` when no
//!     existing ancestor can be resolved (e.g. a dangling symlink), so
//!     callers fail closed rather than guessing a location.
//!   - `is_confined_path`: containment check against the canonical
//!     (symlink-resolved) root. A path is confined only when its physical
//!     location stays inside the physical root. Accepts a symlinked repo
//!     root (canonicalised on both sides) while still rejecting
//!     symlink/junction targets that resolve outside the root.

use std::path::{Component, Path, PathBuf};

/// Options for [`is_confined_path`], mirroring the JS `{ allowRoot }` option.
#[derive(Debug, Clone, Copy, Default)]
pub struct ConfinementOptions {
    pub allow_root: bool,
}

/// Resolve `path` to its physical location, following symlinks in the
/// nearest existing ancestor while keeping the (possibly not-yet-existing)
/// tail intact. Mirrors `resolvePhysicalPath` in the legacy JS module.
///
/// Returns `None` when no existing ancestor can be resolved (e.g. because the
/// filesystem root itself does not exist, or symlink canonicalization
/// fails), matching the JS fail-closed behavior.
pub fn resolve_physical_path(path: &Path) -> Option<PathBuf> {
    let target = absolutize(path);

    let mut ancestor = target.clone();
    loop {
        if ancestor.exists() {
            break;
        }
        match ancestor.parent() {
            Some(parent) if parent != ancestor => {
                ancestor = parent.to_path_buf();
            }
            _ => return None,
        }
    }

    let canonical_ancestor = std::fs::canonicalize(&ancestor).ok()?;
    let tail = target.strip_prefix(&ancestor).unwrap_or(Path::new(""));
    Some(join_clean(&canonical_ancestor, tail))
}

/// Containment check against the canonical (symlink-resolved) `root`.
/// Mirrors `isConfinedPath` in the legacy JS module.
pub fn is_confined_path(root: &Path, path: &Path, options: ConfinementOptions) -> bool {
    if !path.is_absolute() {
        return false;
    }

    let canonical_root = match std::fs::canonicalize(root) {
        Ok(r) => r,
        Err(_) => return false,
    };

    let target = match resolve_physical_path(path) {
        Some(t) => t,
        None => return false,
    };

    let rel = match target.strip_prefix(&canonical_root) {
        Ok(r) => r,
        Err(_) => return false,
    };

    let rel_is_empty = rel.as_os_str().is_empty();
    if rel_is_empty {
        return options.allow_root;
    }

    // strip_prefix already guarantees `rel` does not escape via `..` when it
    // succeeds against a canonicalized root/target pair, but stay defensive
    // and reject any literal `..` component just in case of odd inputs.
    !rel.components().any(|c| c == Component::ParentDir)
}

fn absolutize(path: &Path) -> PathBuf {
    if path.is_absolute() {
        normalize_lexically(path)
    } else {
        let cwd = std::env::current_dir().unwrap_or_default();
        normalize_lexically(&cwd.join(path))
    }
}

/// Lexical normalization (no filesystem access): collapses `.` and resolves
/// `..` against preceding normal components, matching Node's `path.resolve`
/// semantics closely enough for our purposes (we never emit a leading `..`
/// for an absolute path).
fn normalize_lexically(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn join_clean(base: &Path, tail: &Path) -> PathBuf {
    if tail.as_os_str().is_empty() {
        base.to_path_buf()
    } else {
        normalize_lexically(&base.join(tail))
    }
}
