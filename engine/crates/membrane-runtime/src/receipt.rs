//! Receipt of files the runtime wrote outside the four stable roots.
//!
//! MBR-106: standardize stable config, data, cache, and log roots.
//!
//! The four roots in [`crate::paths`] hold everything durable the runtime
//! owns by default. Anything the runtime writes **outside** those roots
//! (a temp lease file, a sandbox scratch directory, a registry key) is
//! potentially hostile to a later uninstall — it must be tracked so
//! `membrane uninstall` removes only what the runtime itself placed there
//! and never touches user data.
//!
//! Each entry is a typed [`ReceiptOwnedFile`]; the audit trail is the
//! in-memory list returned by [`snapshot`] so uninstall can write a
//! stable JSON receipt of exactly what it removed.

use crate::paths::{config_root, Roots};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// One file (or directory) the runtime owns outside the four stable
/// roots. The receipt is the audit trail uninstall uses to know what is
/// safe to delete.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReceiptOwnedFile {
    /// Absolute path the runtime wrote to. The receipt records what the
    /// caller claimed; receipt never resolves symlinks on the caller's
    /// behalf so callers can install a sandboxed scratch tree without
    /// the receipt trying to canonicalize it.
    pub path: PathBuf,
    /// Stable owner label, e.g. `"lease-file"`, `"sandbox-scratch"`. The
    /// label survives across runs and is what uninstall echoes in its
    /// log so operators can audit a residue by category.
    pub owner: String,
    /// True when this row points at a directory the runtime owns
    /// (recursive delete). False for a single file.
    pub directory: bool,
}

impl ReceiptOwnedFile {
    /// Convenience constructor for a single file.
    pub fn file<P: AsRef<Path>>(path: P, owner: impl Into<String>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
            owner: owner.into(),
            directory: false,
        }
    }

    /// Convenience constructor for a directory the runtime owns.
    pub fn dir<P: AsRef<Path>>(path: P, owner: impl Into<String>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
            owner: owner.into(),
            directory: true,
        }
    }
}

/// In-memory registry of every file the runtime has registered as
/// "owned, safe to remove on uninstall". Backed by a process-wide
/// `Mutex<BTreeMap>` so the receipt is stable across concurrent
/// callers within one resident but not persisted between runs.
///
/// Persistence is the installer's job: it reads [`snapshot`] before
/// uninstall and writes the result to a JSON receipt alongside the
/// uninstall marker.
static REGISTRY: Mutex<Option<BTreeMap<PathBuf, ReceiptOwnedFile>>> = Mutex::new(None);

fn with_registry<F, R>(operation: F) -> R
where
    F: FnOnce(&mut BTreeMap<PathBuf, ReceiptOwnedFile>) -> R,
{
    let mut guard = REGISTRY
        .lock()
        .expect("receipt registry poisoned; a previous call panicked while holding the lock");
    let map = guard.get_or_insert_with(BTreeMap::new);
    operation(map)
}

/// Register one file the runtime wrote outside the four stable roots.
/// Re-registering the same path updates the owner label so the most
/// recent owner wins (latest write wins, no double counting).
///
/// Returns `Err(ReceiptError::InsideStableRoot)` when the caller tries
/// to register a path that already lives under one of the four roots —
/// receipt-owned is for residue only; stable-root writes are tracked by
/// the roots themselves.
pub fn register_receipt_owned(entry: ReceiptOwnedFile) -> Result<(), ReceiptError> {
    let roots = Roots::resolve();
    if roots.contains(&entry.path) {
        return Err(ReceiptError::InsideStableRoot(entry.path));
    }
    with_registry(|map| {
        map.insert(entry.path.clone(), entry);
    });
    Ok(())
}

/// Convenience wrapper matching the spec's `register_receipt_owned(path)`.
/// Records a single file owned by the runtime at the given path, owned
/// by the runtime's supervisor lease writer. Caller may overwrite the
/// owner later via [`register_receipt_owned`].
pub fn register_receipt_owned_path<P: AsRef<Path>>(path: P) -> Result<(), ReceiptError> {
    register_receipt_owned(ReceiptOwnedFile::file(path, "membrane-runtime"))
}

/// Returns `true` when `path` is currently registered. Used by the
/// installer and uninstall to skip files that aren't part of this
/// runtime's footprint.
pub fn is_receipt_owned(path: &Path) -> bool {
    with_registry(|map| map.contains_key(path))
}

/// All receipt-owned entries in a stable order (BTreeMap by absolute
/// path). Uninstall calls this to drive its deletion list and to write
/// a typed JSON audit receipt.
pub fn snapshot() -> Vec<ReceiptOwnedFile> {
    with_registry(|map| map.values().cloned().collect())
}

/// Remove every registered entry that `predicate` agrees to delete and
/// return what was removed, in path order. The predicate is uninstall's
/// guard: it can decline a row if the path was clobbered by another
/// tool between the install that wrote it and the uninstall reading
/// it.
///
/// Rows not matched by the predicate are left in the registry so a
/// subsequent `remove_receipt_owned` call can retry them.
pub fn remove_receipt_owned<F>(predicate: F) -> Vec<ReceiptOwnedFile>
where
    F: Fn(&ReceiptOwnedFile) -> bool,
{
    let mut removed: Vec<ReceiptOwnedFile> = Vec::new();
    with_registry(|map| {
        let to_remove: Vec<PathBuf> = map
            .iter()
            .filter_map(|(path, entry)| {
                if predicate(entry) {
                    Some(path.clone())
                } else {
                    None
                }
            })
            .collect();
        for path in to_remove {
            if let Some(entry) = map.remove(&path) {
                removed.push(entry);
            }
        }
    });
    removed
}

/// Clear the entire registry. Used by tests and by `membrane uninstall`
/// after it has captured the receipt it intends to honor.
pub fn clear_receipt_registry() {
    with_registry(|map| map.clear());
}

/// Errors the receipt module surfaces to callers.
#[derive(Debug, thiserror::Error)]
pub enum ReceiptError {
    /// The caller tried to register a path that already lives under one
    /// of the four stable roots. Receipt is for residue only.
    #[error("path {0:?} lives inside a stable root; receipt tracking is for residue files only")]
    InsideStableRoot(PathBuf),
}

/// Convenience: a typed audit receipt. The installer writes this JSON
/// blob next to the uninstall marker so residue audits are reproducible
/// from disk without re-running the runtime.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UninstallReceipt {
    pub schema_version: u32,
    pub config_root: PathBuf,
    pub owned_files: Vec<ReceiptOwnedFile>,
}

impl UninstallReceipt {
    /// Capture the current receipt snapshot anchored at the active
    /// config root. The installer calls this immediately before it
    /// unlinks anything so uninstall logs always include the four
    /// roots.
    pub fn capture() -> Self {
        Self {
            schema_version: 1,
            config_root: config_root(),
            owned_files: snapshot(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::Mutex;

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn outside_path(label: &str) -> PathBuf {
        // Use a path under `/tmp` (or the platform equivalent) so the
        // path is guaranteed not to live under any of the four roots
        // even when the process has a custom MEMBRANE_* override.
        let mut p = std::env::temp_dir();
        p.push(format!("membrane-receipt-test-{label}"));
        p
    }

    #[test]
    fn registering_then_querying_returns_true() {
        let _guard = TEST_LOCK.lock().unwrap();
        clear_receipt_registry();
        let path = outside_path("basic");
        register_receipt_owned_path(&path).expect("outside root");
        assert!(is_receipt_owned(&path));
        let snap = snapshot();
        assert!(snap.iter().any(|entry| entry.path == path));
    }

    #[test]
    fn registering_inside_stable_root_is_rejected() {
        let _guard = TEST_LOCK.lock().unwrap();
        clear_receipt_registry();
        let inside = config_root().join("receipt-should-not-track-this");
        let err = register_receipt_owned_path(&inside).unwrap_err();
        assert!(matches!(err, ReceiptError::InsideStableRoot(_)));
        assert!(!is_receipt_owned(&inside));
    }

    #[test]
    fn remove_receipt_owned_only_removes_receipt_owned_paths() {
        let _guard = TEST_LOCK.lock().unwrap();
        clear_receipt_registry();
        let keep = outside_path("keep");
        let drop = outside_path("drop");
        register_receipt_owned_path(&keep).unwrap();
        register_receipt_owned_path(&drop).unwrap();
        let removed = remove_receipt_owned(|entry| entry.path == drop);
        assert_eq!(removed.len(), 1);
        assert_eq!(removed[0].path, drop);
        assert!(!is_receipt_owned(&drop));
        assert!(is_receipt_owned(&keep));
    }

    #[test]
    fn uninstall_receipt_captures_anchored_snapshot() {
        let _guard = TEST_LOCK.lock().unwrap();
        clear_receipt_registry();
        let a = outside_path("audit-a");
        let b = outside_path("audit-b");
        register_receipt_owned_path(&a).unwrap();
        register_receipt_owned_path(&b).unwrap();
        let receipt = UninstallReceipt::capture();
        assert_eq!(receipt.schema_version, 1);
        assert_eq!(receipt.config_root, config_root());
        assert_eq!(receipt.owned_files.len(), 2);
    }
}
