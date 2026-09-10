//! Native port of `blueprint/src/graph/atomic-store-adoption.mjs`.
//!
//! Replaces one closed file with another, retaining an immediately
//! recoverable prior file until a caller-supplied validation step passes.
//! On non-Windows platforms the prior file is preserved via a hardlink
//! (`target` can be renamed into place while the backup inode survives);
//! on Windows, where a file that is still open cannot be hardlinked over
//! reliably in the same way the legacy implementation relied on, the prior
//! file is renamed aside instead. Either way, on any failure the backup is
//! restored to `target` before the error propagates, and the backup path
//! is removed once adoption is decided (success or restored failure).

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug)]
pub struct AtomicAdoptError {
    pub message: String,
}

impl fmt::Display for AtomicAdoptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for AtomicAdoptError {}

impl AtomicAdoptError {
    fn new(message: impl Into<String>) -> Self {
        Self { message: message.into() }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdoptionOutcome {
    pub target: PathBuf,
    pub replaced: bool,
}

fn backup_path_for(target: &Path) -> PathBuf {
    let pid = std::process::id();
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let suffix = format!("previous-{pid}-{nanos}");
    let mut name = target.as_os_str().to_owned();
    name.push(".");
    name.push(suffix);
    PathBuf::from(name)
}

/// Whether the current platform should preserve the prior file via a
/// hardlink (`false`) or a temporary rename (`true`, Windows-style).
pub fn platform_uses_rename_backup(platform: &str) -> bool {
    platform == "win32" || platform == "windows"
}

/// Replace `target` with `source`, invoking `validate(target)` after the
/// swap and before the prior file is discarded. On any failure — including
/// a failing `validate` — the prior file (if one existed) is restored.
///
/// `platform` selects the backup strategy (see [`platform_uses_rename_backup`]);
/// pass `std::env::consts::OS` in production.
pub fn adopt_file_atomically(
    source: &Path,
    target: &Path,
    platform: &str,
    validate: impl FnOnce(&Path) -> Result<(), AtomicAdoptError>,
) -> Result<AdoptionOutcome, AtomicAdoptError> {
    let backup = backup_path_for(target);
    let had_target = target.exists();
    let mut target_moved = false;
    let mut backup_created = false;

    let result = (|| -> Result<AdoptionOutcome, AtomicAdoptError> {
        if had_target {
            if platform_uses_rename_backup(platform) {
                fs::rename(target, &backup).map_err(|e| AtomicAdoptError::new(format!("rename target to backup failed: {e}")))?;
                target_moved = true;
            } else {
                fs::hard_link(target, &backup).map_err(|e| AtomicAdoptError::new(format!("hardlink backup failed: {e}")))?;
                backup_created = true;
            }
        }
        fs::rename(source, target).map_err(|e| AtomicAdoptError::new(format!("rename source into target failed: {e}")))?;
        validate(target)?;
        if had_target {
            let _ = fs::remove_file(&backup);
        }
        Ok(AdoptionOutcome { target: target.to_path_buf(), replaced: had_target })
    })();

    if result.is_err() && had_target && (backup_created || target_moved) && backup.exists() {
        if target.exists() {
            let _ = fs::remove_file(target);
        }
        let _ = fs::rename(&backup, target);
    }
    if backup.exists() {
        let _ = fs::remove_file(&backup);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_store_replaces_prior_file_only_after_validation() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("graph.db");
        let source = dir.path().join("fresh.db");
        fs::write(&target, "old").unwrap();
        fs::write(&source, "fresh").unwrap();
        let outcome = adopt_file_atomically(&source, &target, "linux", |path| {
            assert_eq!(fs::read_to_string(path).unwrap(), "fresh");
            Ok(())
        })
        .unwrap();
        assert!(outcome.replaced);
        assert_eq!(fs::read_to_string(&target).unwrap(), "fresh");
        assert!(!source.exists());
    }

    #[test]
    fn failed_validation_restores_prior_file() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("graph.db");
        let source = dir.path().join("fresh.db");
        fs::write(&target, "old").unwrap();
        fs::write(&source, "broken").unwrap();
        let err = adopt_file_atomically(&source, &target, "linux", |_| Err(AtomicAdoptError::new("invalid"))).unwrap_err();
        assert!(err.message.contains("invalid"));
        assert_eq!(fs::read_to_string(&target).unwrap(), "old");
    }

    #[test]
    fn windows_backup_strategy_also_restores_on_failure() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("graph.db");
        let source = dir.path().join("fresh.db");
        fs::write(&target, "old").unwrap();
        fs::write(&source, "broken").unwrap();
        let err = adopt_file_atomically(&source, &target, "win32", |_| Err(AtomicAdoptError::new("invalid"))).unwrap_err();
        assert!(err.message.contains("invalid"));
        assert_eq!(fs::read_to_string(&target).unwrap(), "old");
    }

    #[test]
    fn no_prior_target_is_a_plain_move() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("graph.db");
        let source = dir.path().join("fresh.db");
        fs::write(&source, "fresh").unwrap();
        let outcome = adopt_file_atomically(&source, &target, "linux", |_| Ok(())).unwrap();
        assert!(!outcome.replaced);
        assert_eq!(fs::read_to_string(&target).unwrap(), "fresh");
    }
}
