//! Durable, content-free identity receipt for one resident startup.

use crate::installation_identity::{InstallationIdentity, StartupClaim};
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const RUNTIME_RECEIPT_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeReceiptV1 {
    pub schema_version: u32,
    pub workspace_root: PathBuf,
    pub crypt_db: PathBuf,
    pub catalog_db: PathBuf,
    pub telemetry_outbox_db: PathBuf,
    pub installation_id: String,
    pub startup_generation: u64,
    pub service_instance_id: String,
    pub claimed_at: String,
}

#[derive(Debug, thiserror::Error)]
pub enum RuntimeReceiptError {
    #[error("runtime binding {binding} must be absolute: {value}")]
    Relative {
        binding: &'static str,
        value: String,
    },
    #[error("workspace root cannot be derived from CRYPT_DB: {0}")]
    WorkspaceUnbound(PathBuf),
    #[error("runtime identity and startup claim do not match")]
    IdentityMismatch,
    #[error("runtime receipt {operation} {path}: {source}")]
    Io {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("runtime receipt conflict: {0}")]
    Conflict(PathBuf),
    #[error("runtime receipt path is not a regular owned path: {0}")]
    UnsafePath(PathBuf),
    #[error("encode runtime receipt: {0}")]
    Encode(String),
}

fn io_error(operation: &'static str, path: &Path, source: std::io::Error) -> RuntimeReceiptError {
    RuntimeReceiptError::Io {
        operation,
        path: path.to_path_buf(),
        source,
    }
}

fn require_absolute(
    binding: &'static str,
    path: impl AsRef<Path>,
) -> Result<PathBuf, RuntimeReceiptError> {
    let path = path.as_ref();
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Err(RuntimeReceiptError::Relative {
            binding,
            value: path.to_string_lossy().into_owned(),
        })
    }
}

pub fn resolve_workspace_root(
    configured: Option<std::ffi::OsString>,
    crypt_db: &Path,
) -> Result<PathBuf, RuntimeReceiptError> {
    if let Some(configured) = configured.filter(|value| !value.is_empty()) {
        return require_absolute("WORKSPACE_ROOT", PathBuf::from(configured));
    }
    let crypt_db = require_absolute("CRYPT_DB", crypt_db)?;
    let memory = crypt_db.parent();
    let cache = memory.and_then(Path::parent);
    let tools = cache.and_then(Path::parent);
    if memory
        .and_then(Path::file_name)
        .is_some_and(|name| name == "memory")
        && cache
            .and_then(Path::file_name)
            .is_some_and(|name| name == ".cache")
        && tools
            .and_then(Path::file_name)
            .is_some_and(|name| name == "tools")
    {
        return tools
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .ok_or_else(|| RuntimeReceiptError::WorkspaceUnbound(crypt_db));
    }
    Err(RuntimeReceiptError::WorkspaceUnbound(crypt_db))
}

pub fn resolve_telemetry_outbox_path_from(
    crypt_outbox: Option<std::ffi::OsString>,
    membrane_outbox: Option<std::ffi::OsString>,
    crypt_db: &Path,
) -> Result<PathBuf, RuntimeReceiptError> {
    if let Some(path) = crypt_outbox.filter(|value| !value.is_empty()) {
        return require_absolute("CRYPT_TELEMETRY_OUTBOX", PathBuf::from(path));
    }
    if let Some(path) = membrane_outbox.filter(|value| !value.is_empty()) {
        return require_absolute("MEMBRANE_TELEMETRY_OUTBOX", PathBuf::from(path));
    }
    let crypt_db = require_absolute("CRYPT_DB", crypt_db)?;
    Ok(crypt_db
        .parent()
        .expect("absolute database path has a parent")
        .join("context-telemetry-outbox.db"))
}

impl RuntimeReceiptV1 {
    pub fn new(
        workspace_root: &Path,
        crypt_db: &Path,
        catalog_db: &Path,
        telemetry_outbox_db: &Path,
        identity: &InstallationIdentity,
        claim: &StartupClaim,
    ) -> Result<Self, RuntimeReceiptError> {
        if identity.installation_id != claim.installation_id
            || identity.startup_generation != claim.startup_generation
            || identity.current_service_instance_id.as_deref()
                != Some(claim.service_instance_id.as_str())
            || identity.current_claimed_at.as_deref() != Some(claim.claimed_at.as_str())
            || claim.startup_generation == 0
        {
            return Err(RuntimeReceiptError::IdentityMismatch);
        }
        Ok(Self {
            schema_version: RUNTIME_RECEIPT_SCHEMA_VERSION,
            workspace_root: require_absolute("WORKSPACE_ROOT", workspace_root)?,
            crypt_db: require_absolute("CRYPT_DB", crypt_db)?,
            catalog_db: require_absolute("RIGHTCONTEXT_CATALOG", catalog_db)?,
            telemetry_outbox_db: require_absolute("CRYPT_TELEMETRY_OUTBOX", telemetry_outbox_db)?,
            installation_id: identity.installation_id.clone(),
            startup_generation: claim.startup_generation,
            service_instance_id: claim.service_instance_id.clone(),
            claimed_at: claim.claimed_at.clone(),
        })
    }

    pub fn path(&self) -> PathBuf {
        self.workspace_root
            .join("tools/.cache/memory/runtime-receipts")
            .join(format!(
                "{:020}-{}.json",
                self.startup_generation, self.service_instance_id
            ))
    }

    pub fn persist(&self) -> Result<PathBuf, RuntimeReceiptError> {
        let final_path = self.path();
        let parent = final_path
            .parent()
            .expect("runtime receipt path always has a parent");
        fs::create_dir_all(parent).map_err(|error| io_error("create directory", parent, error))?;
        let parent_metadata = fs::symlink_metadata(parent)
            .map_err(|error| io_error("inspect directory", parent, error))?;
        if !parent_metadata.is_dir() || parent_metadata.file_type().is_symlink() {
            return Err(RuntimeReceiptError::UnsafePath(parent.to_path_buf()));
        }
        let mut bytes = serde_json::to_vec_pretty(self)
            .map_err(|error| RuntimeReceiptError::Encode(error.to_string()))?;
        bytes.push(b'\n');
        if let Ok(metadata) = fs::symlink_metadata(&final_path) {
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                return Err(RuntimeReceiptError::UnsafePath(final_path));
            }
            let existing =
                fs::read(&final_path).map_err(|error| io_error("read", &final_path, error))?;
            return if existing == bytes {
                Ok(final_path)
            } else {
                Err(RuntimeReceiptError::Conflict(final_path))
            };
        }
        let temporary = parent.join(format!(
            ".{}.{}.tmp",
            final_path
                .file_name()
                .expect("runtime receipt path has a file name")
                .to_string_lossy(),
            std::process::id()
        ));
        let result = (|| {
            let mut file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&temporary)
                .map_err(|error| io_error("create temporary", &temporary, error))?;
            file.write_all(&bytes)
                .and_then(|_| file.sync_all())
                .map_err(|error| io_error("write temporary", &temporary, error))?;
            match fs::hard_link(&temporary, &final_path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    let metadata = fs::symlink_metadata(&final_path)
                        .map_err(|error| io_error("inspect", &final_path, error))?;
                    if !metadata.is_file() || metadata.file_type().is_symlink() {
                        return Err(RuntimeReceiptError::UnsafePath(final_path.clone()));
                    }
                    let existing = fs::read(&final_path)
                        .map_err(|error| io_error("read", &final_path, error))?;
                    if existing != bytes {
                        return Err(RuntimeReceiptError::Conflict(final_path.clone()));
                    }
                }
                Err(error) => return Err(io_error("publish", &final_path, error)),
            }
            #[cfg(unix)]
            fs::File::open(parent)
                .and_then(|directory| directory.sync_all())
                .map_err(|error| io_error("sync directory", parent, error))?;
            Ok(final_path.clone())
        })();
        let _ = fs::remove_file(&temporary);
        result
    }
}

static CURRENT_RECEIPT: Mutex<Option<RuntimeReceiptV1>> = Mutex::new(None);

pub fn publish_current(receipt: RuntimeReceiptV1) {
    *CURRENT_RECEIPT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(receipt);
}

pub fn current_snapshot() -> Option<RuntimeReceiptV1> {
    CURRENT_RECEIPT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity() -> (InstallationIdentity, StartupClaim) {
        let installation_id = "00000000-0000-4000-8000-000000000111".to_string();
        let service_instance_id = "00000000-0000-4000-8000-000000000222".to_string();
        let claimed_at = "2026-08-12T00:00:00Z".to_string();
        (
            InstallationIdentity {
                schema_version: 2,
                installation_id: installation_id.clone(),
                created_at: claimed_at.clone(),
                startup_generation: 7,
                legacy_labels: Vec::new(),
                lineage: Vec::new(),
                current_service_instance_id: Some(service_instance_id.clone()),
                current_claimed_at: Some(claimed_at.clone()),
            },
            StartupClaim {
                schema_version: 1,
                installation_id,
                startup_generation: 7,
                service_instance_id,
                claimed_at,
            },
        )
    }

    #[test]
    fn resolvers_require_absolute_bindings_and_use_canonical_sibling() {
        let root = std::env::temp_dir().join("runtime-receipt-resolver");
        let db = root.join("tools/.cache/memory/crypt-engine.db");
        assert_eq!(resolve_workspace_root(None, &db).unwrap(), root);
        assert_eq!(
            resolve_telemetry_outbox_path_from(None, None, &db).unwrap(),
            db.parent().unwrap().join("context-telemetry-outbox.db")
        );
        assert!(matches!(
            resolve_telemetry_outbox_path_from(Some("relative.db".into()), None, &db),
            Err(RuntimeReceiptError::Relative { .. })
        ));
        assert!(matches!(
            resolve_workspace_root(None, Path::new("relative.db")),
            Err(RuntimeReceiptError::Relative { .. })
        ));
    }

    #[test]
    fn receipt_is_immutable_and_binds_every_runtime_path() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let crypt_db = root.join("tools/.cache/memory/crypt-engine.db");
        let catalog_db = root.join("tools/.cache/memory/catalog.db");
        let outbox_db = root.join("tools/.cache/memory/context-telemetry-outbox.db");
        let (identity, claim) = identity();
        let receipt =
            RuntimeReceiptV1::new(root, &crypt_db, &catalog_db, &outbox_db, &identity, &claim)
                .unwrap();
        let path = receipt.persist().unwrap();
        assert_eq!(receipt.persist().unwrap(), path);
        let decoded: RuntimeReceiptV1 = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(decoded, receipt);
        assert!(path.is_file());
        assert!(fs::read_dir(path.parent().unwrap())
            .unwrap()
            .all(|entry| !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".tmp")));
    }

    #[test]
    fn mismatched_startup_claim_is_rejected_before_receipt_io() {
        let temp = tempfile::tempdir().unwrap();
        let (identity, mut claim) = identity();
        claim.startup_generation += 1;
        let result = RuntimeReceiptV1::new(
            temp.path(),
            &temp.path().join("crypt.db"),
            &temp.path().join("catalog.db"),
            &temp.path().join("outbox.db"),
            &identity,
            &claim,
        );
        assert!(matches!(result, Err(RuntimeReceiptError::IdentityMismatch)));
        assert!(!temp
            .path()
            .join("tools/.cache/memory/runtime-receipts")
            .exists());
    }

    #[cfg(unix)]
    #[test]
    fn receipt_refuses_symlink_target_without_following_it() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let (identity, claim) = identity();
        let receipt = RuntimeReceiptV1::new(
            root,
            &root.join("tools/.cache/memory/crypt-engine.db"),
            &root.join("tools/.cache/memory/catalog.db"),
            &root.join("tools/.cache/memory/context-telemetry-outbox.db"),
            &identity,
            &claim,
        )
        .unwrap();
        let path = receipt.path();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let outside = root.join("outside.json");
        fs::write(&outside, b"outside").unwrap();
        symlink(&outside, &path).unwrap();

        assert!(matches!(
            receipt.persist(),
            Err(RuntimeReceiptError::UnsafePath(_))
        ));
        assert_eq!(fs::read(&outside).unwrap(), b"outside");
    }
}
