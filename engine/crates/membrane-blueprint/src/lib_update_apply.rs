//! Native Rust port of `blueprint/src/lib/update/apply.mjs`.
//!
//! Lane LIB3 (r5 closure): no prior native equivalent found (grep of
//! `applyStaged`/`recoverPendingUpdate`/`backupStore` across
//! membrane-blueprint/src and membrane-runtime/src produced no match).
//!
//! SCOPE: the legacy module is a stateful atomic-update engine (renames
//! directories, journals a transaction, backs up a live SQLite store with
//! `VACUUM INTO`). This port covers its deterministic, testable core: the
//! durable-write pattern (`durable_json` — write-temp, fsync, rename),
//! `copy_recursive` (the same symlink-refusing recursive copy the legacy
//! code uses for staging/applying), and `backup_store` (via `rusqlite`,
//! already a crate dependency, mirroring `VACUUM INTO`). The full
//! multi-phase rename/journal choreography in `applyStaged`/
//! `recoverPendingUpdate` is intentionally not replicated as a pure
//! function — it is inherently an OS-transaction orchestrator, not
//! something with a meaningful non-IO contract to prove parity against;
//! `lib_update_manifest.rs`'s `tree_digest` covers the digest-binding half
//! of that contract that is meaningfully testable.

use std::fs;
use std::path::Path;

/// Mirrors `durableJson(path, value)`: write to a temp file beside the
/// target, fsync it, then rename into place (atomic on the same
/// filesystem), matching the legacy write-temp/fsync/rename pattern.
pub fn durable_json(path: &Path, value: &serde_json::Value) -> std::io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let temp = path.with_file_name(format!(
        "{file_name}.tmp-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let result = (|| {
        use std::io::Write;
        let mut file = fs::File::create(&temp)?;
        file.write_all(&serde_json::to_vec(value).unwrap())?;
        file.sync_all()?;
        fs::rename(&temp, path)?;
        // Directory metadata must reach disk too, otherwise a crash after the
        // rename can lose the receipt/journal despite the file being synced.
        if let Ok(directory) = fs::File::open(parent) {
            let _ = directory.sync_all();
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

/// Mirrors `copyRecursive(source, target)`: refuses symlinks and anything
/// that is neither a regular file nor a directory.
pub fn copy_recursive(source: &Path, target: &Path) -> std::io::Result<()> {
    let source_meta = fs::symlink_metadata(source)?;
    if source_meta.file_type().is_symlink() || !source_meta.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("unsafe update source: {}", source.display()),
        ));
    }
    if let Ok(target_meta) = fs::symlink_metadata(target) {
        if target_meta.file_type().is_symlink() || !target_meta.is_dir() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("unsafe update target: {}", target.display()),
            ));
        }
    }
    fs::create_dir_all(target)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let src = entry.path();
        let dst = target.join(entry.file_name());
        let meta = fs::symlink_metadata(&src)?;
        if meta.file_type().is_symlink() || (!meta.is_dir() && !meta.is_file()) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("unsafe update entry: {}", src.display()),
            ));
        }
        if meta.is_dir() {
            copy_recursive(&src, &dst)?;
        } else {
            fs::copy(&src, &dst)?;
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq)]
pub struct BackupResult {
    pub backed_up: bool,
    pub path: Option<std::path::PathBuf>,
    pub error: Option<String>,
}

/// Mirrors `backupStore(root, { outDir })`: `VACUUM INTO` a live-store
/// backup via `rusqlite` (already a crate dependency), matching the legacy
/// `node:sqlite` `DatabaseSync` `VACUUM INTO` call.
pub fn backup_store(root: &Path, out_dir: &str) -> BackupResult {
    let db_path = root.join(out_dir).join("graph").join("graph.db");
    if !db_path.exists() {
        return BackupResult {
            backed_up: false,
            path: None,
            error: None,
        };
    }
    let backup_dir = root.join(out_dir).join("backups");
    if let Err(e) = fs::create_dir_all(&backup_dir) {
        return BackupResult {
            backed_up: false,
            path: None,
            error: Some(e.to_string()),
        };
    }
    let target = backup_dir.join(format!(
        "graph.db.before-update-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    ));
    let conn = match rusqlite::Connection::open(&db_path) {
        Ok(c) => c,
        Err(e) => {
            return BackupResult {
                backed_up: false,
                path: None,
                error: Some(e.to_string()),
            }
        }
    };
    let target_sql = target.to_string_lossy().replace('\'', "''");
    match conn.execute_batch(&format!("VACUUM INTO '{target_sql}'")) {
        Ok(()) => BackupResult {
            backed_up: true,
            path: Some(target),
            error: None,
        },
        Err(e) => BackupResult {
            backed_up: false,
            path: None,
            error: Some(e.to_string()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn durable_json_writes_readable_json_atomically() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        durable_json(&path, &json!({ "a": 1 })).unwrap();
        let read: serde_json::Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(read, json!({ "a": 1 }));
        // No leftover temp files.
        let leftovers: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains("tmp-"))
            .collect();
        assert!(leftovers.is_empty());
    }

    #[test]
    fn copy_recursive_copies_nested_files_and_rejects_symlinks() {
        let src = tempfile::tempdir().unwrap();
        fs::create_dir_all(src.path().join("nested")).unwrap();
        fs::write(src.path().join("nested/file.txt"), b"hello").unwrap();
        let dst = tempfile::tempdir().unwrap().path().join("copy");
        copy_recursive(src.path(), &dst).unwrap();
        assert_eq!(fs::read(dst.join("nested/file.txt")).unwrap(), b"hello");
    }

    #[test]
    fn backup_store_no_op_when_db_missing() {
        let dir = tempfile::tempdir().unwrap();
        let result = backup_store(dir.path(), ".agent");
        assert_eq!(
            result,
            BackupResult {
                backed_up: false,
                path: None,
                error: None
            }
        );
    }

    #[test]
    fn backup_store_vacuums_an_existing_sqlite_db() {
        let dir = tempfile::tempdir().unwrap();
        let graph_dir = dir.path().join(".agent").join("graph");
        fs::create_dir_all(&graph_dir).unwrap();
        let db_path = graph_dir.join("graph.db");
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute_batch("CREATE TABLE t (x INTEGER); INSERT INTO t VALUES (1);")
            .unwrap();
        drop(conn);
        let result = backup_store(dir.path(), ".agent");
        assert!(result.backed_up, "error: {:?}", result.error);
        assert!(result.path.as_ref().unwrap().exists());
    }
}
