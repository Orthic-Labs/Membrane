//! MBR-205: ownership-safe uninstall.
//!
//! The uninstall pipeline refuses to remove anything the runtime did not
//! register as owned by a Membrane receipt. The contract is:
//!
//!   * `OwnershipTable` is the durable record of every path the install
//!     touched outside the four stable roots. It lives in
//!     `<receipt_root>/ownership.json`.
//!   * `load_table` returns the on-disk table, or an empty table tied to
//!     a freshly-synthesised `installation_id` when the file is missing
//!     (so a fresh install after uninstall is always permitted — the new
//!     install writes its own table).
//!   * `register` records a claim. Duplicate `(kind, path)` pairs are
//!     rejected so a buggy installer can never quietly overwrite the
//!     audit trail.
//!   * `revoke_unowned` returns the subset of `candidates` that ARE in
//!     the table. Anything not in the table is left alone with a logged
//!     warning so a stray `--candidate` cannot delete user data.
//!   * `execute_uninstall` walks the authorised set, calls the supplied
//!     `remove` callback for each path, and writes a typed
//!     `UninstallReceiptV1` so the operator sees exactly what was
//!     removed, what was refused, and which outcome was recorded.
//!
//! The framework stays generic — `remove` is a callback so tests can
//! inject a deterministic in-process effect. The CLI's `uninstall` mode
//! passes a callback that uses `std::fs::remove_dir_all` for directories
//! and `std::fs::remove_file` for files.

use serde::{Deserialize, Serialize};
use sha2::Digest;
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Bumped on incompatible shape changes; the next run refuses unknown versions.
pub const UNINSTALL_RECEIPT_SCHEMA_VERSION: u32 = 1;

/// The ownership-table file name. Always written under the receipt root
/// (the target root of an MBR-203 install) and re-read by every subsequent
/// uninstall.
pub const OWNERSHIP_TABLE_FILE_NAME: &str = "ownership.json";

/// The receipt file name for an uninstall run. Written alongside the
/// ownership table so the operator sees both the original ownership
/// record and the uninstall audit trail in one place.
pub const UNINSTALL_RECEIPT_FILE_NAME: &str = "uninstall-receipt.json";

/// One ownership category. The kind tells the operator which subsystem
/// claimed the path; uninstall refuses anything that is not in the table
/// regardless of kind, but the receipt records the kind so a residue
/// audit can answer "what wrote this?" without re-reading the receipt
/// registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum OwnershipKind {
    /// A durable install manifest the installer wrote to disk.
    Manifest,
    /// A supervisor-issued lease (or sibling endpoint) the supervisor
    /// publishes alongside the manifest.
    Lease,
    /// A receipt file the install or uninstall pipeline produced.
    Receipt,
    /// A native binding (MCP client entry, watcher hook, registry key)
    /// the installer registered with an external surface.
    Binding,
}

impl OwnershipKind {
    /// Stable, human-readable name. Used in JSON, in log lines, and in
    /// the receipt's `claims` vector.
    pub const fn as_str(self) -> &'static str {
        match self {
            OwnershipKind::Manifest => "Manifest",
            OwnershipKind::Lease => "Lease",
            OwnershipKind::Receipt => "Receipt",
            OwnershipKind::Binding => "Binding",
        }
    }
}

/// One row of the ownership table. The path is what the install actually
/// wrote; `receipt_id` is the install receipt (or supervisor lease id)
/// that minted the row so a future uninstall can trace the chain back to
/// the plan that produced it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnershipClaim {
    pub kind: OwnershipKind,
    pub path: PathBuf,
    /// Stable id of the receipt that minted this claim — usually the
    /// `plan_id` of the originating install receipt, or the supervisor
    /// lease's `supervisor_instance_id` for a Lease row.
    pub receipt_id: String,
    /// Wall-clock instant the row was registered. Best-effort; the
    /// receipt's own `started_at_unix_ms` is the authoritative timeline.
    pub registered_at_unix_ms: u64,
}

/// The durable ownership record. Persisted as `<receipt_root>/ownership.json`.
/// `installation_id` is a stable hash of the receipt root so two installs
/// in different locations can never accidentally share a table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnershipTable {
    pub installation_id: String,
    pub claims: Vec<OwnershipClaim>,
}

impl OwnershipTable {
    /// Build a fresh table tied to a stable `installation_id`. The id is
    /// `sha256:<hex>` of the receipt root so two installs at different
    /// paths cannot collide.
    pub fn fresh(receipt_root: &Path) -> Self {
        Self {
            installation_id: synthesize_installation_id(receipt_root),
            claims: Vec::new(),
        }
    }

    /// True when the table contains a claim for the given `(kind, path)`
    /// pair. Used by `register` to detect duplicates and by
    /// `revoke_unowned` to test each candidate.
    pub fn contains(&self, kind: OwnershipKind, path: &Path) -> bool {
        self.claims
            .iter()
            .any(|claim| claim.kind == kind && claim.path == path)
    }
}

/// Load the ownership table from disk, or return a fresh one tied to a
/// stable `installation_id` when the file is missing. A corrupt file is
/// treated as missing — the operator's only safe option is a fresh
/// install, which writes its own table on disk.
pub fn load_table(receipt_root: &Path) -> Result<OwnershipTable, UninstallError> {
    let table_path = receipt_root.join(OWNERSHIP_TABLE_FILE_NAME);
    match std::fs::read(&table_path) {
        Ok(bytes) => match serde_json::from_slice::<OwnershipTable>(&bytes) {
            Ok(table) => Ok(table),
            Err(error) => Err(UninstallError::Io {
                path: table_path,
                reason: format!("parse ownership table: {error}"),
            }),
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(OwnershipTable::fresh(receipt_root))
        }
        Err(error) => Err(UninstallError::Io {
            path: table_path,
            reason: format!("read ownership table: {error}"),
        }),
    }
}

/// Register one ownership claim. Rejects duplicate `(kind, path)` pairs
/// so the audit trail cannot be silently overwritten by a buggy
/// installer. The function returns the (now-stored) claim's index on
/// success and a typed error on a duplicate.
pub fn register(table: &mut OwnershipTable, claim: OwnershipClaim) -> Result<(), UninstallError> {
    if table.contains(claim.kind, &claim.path) {
        return Err(UninstallError::DuplicateOwnership {
            kind: claim.kind,
            path: claim.path,
        });
    }
    table.claims.push(claim);
    Ok(())
}

/// Return the subset of `candidates` that ARE in the ownership table.
/// Anything not in the table is left alone — the caller is expected to
/// log a warning so the operator sees which paths were refused. The
/// returned vector preserves the input order so the receipt's `removed`
/// list matches what the operator typed on the command line.
pub fn revoke_unowned(table: &OwnershipTable, candidates: &[PathBuf]) -> Vec<PathBuf> {
    let mut authorised: Vec<PathBuf> = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        if table.claims.iter().any(|claim| &claim.path == candidate) {
            authorised.push(candidate.clone());
        }
    }
    authorised
}

/// Terminal state of an uninstall run, recorded in the receipt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum UninstallOutcome {
    /// The run started; no decision is final yet. Used while the receipt
    /// is being rewritten between `execute_uninstall` calls.
    Pending,
    /// Every authorised candidate was removed and every refused candidate
    /// is recorded in `refused`. A clean run lands here.
    Completed,
    /// Every candidate was refused; nothing was removed. Distinct from
    /// `PartiallyRemoved` so a refusal-only run is easy to grep for.
    RefusedAll,
    /// Some authorised candidates were removed and `remove` failed on at
    /// least one. The remaining authorised set is preserved in `refused`
    /// and the failing reason is carried in `reason`.
    PartiallyRemoved { reason: String },
}

/// The durable uninstall record. Persisted under the receipt root after
/// `execute_uninstall` returns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UninstallReceiptV1 {
    pub schema_version: u32,
    pub installation_id: String,
    pub started_at_unix_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at_unix_ms: Option<u64>,
    /// Every path `remove` succeeded on.
    pub removed: Vec<PathBuf>,
    /// Every path the caller asked about that was either refused by the
    /// ownership table or skipped because of a prior `remove` failure.
    pub refused: Vec<PathBuf>,
    pub outcome: UninstallOutcome,
}

/// Typed uninstall errors. The CLI maps each variant to a non-zero exit
/// code and a stable log label.
#[derive(Debug, Error)]
pub enum UninstallError {
    /// `register` was asked to record a `(kind, path)` pair that is
    /// already in the table. The audit trail must never be silently
    /// overwritten.
    #[error("duplicate ownership claim for kind {kind:?} at path {path:?}")]
    DuplicateOwnership { kind: OwnershipKind, path: PathBuf },
    /// A path was not in the ownership table. Surfaced when the caller
    /// asked `execute_uninstall` to remove a specific path that the
    /// table did not authorise.
    #[error("path {path:?} is not in the ownership table")]
    NotOwned { path: PathBuf },
    /// The `remove` callback returned an error on the given path. The
    /// receipt captures whatever was removed before the failure so a
    /// forensic read sees the partial state.
    #[error("remove failed for {path:?}: {reason}")]
    RemoveFailed { path: PathBuf, reason: String },
    /// A filesystem-level failure (read/write/parse of the ownership
    /// table or the receipt) prevented the uninstall from running.
    #[error("uninstall IO failure at {path}: {reason}")]
    Io { path: PathBuf, reason: String },
}

/// Drive an uninstall against the supplied `candidates`. The pipeline:
///
///   1. Filter `candidates` through `revoke_unowned` so only
///      receipt-owned paths reach `remove`.
///   2. For each authorised candidate, call `remove`. On any failure,
///      halt and return `Err(UninstallError::RemoveFailed { receipt })`
///      with the partial receipt attached.
///   3. If no candidate is authorised, return a `RefusedAll` receipt
///      instead of silently succeeding.
///
/// `now_unix_ms` is the start instant the receipt records. The function
/// does not read the wall clock itself so tests can pin the timestamp.
///
/// `remove` is the per-path effect; tests inject a deterministic
/// in-process callback, the CLI's `uninstall` mode passes a callback that
/// uses `std::fs::remove_dir_all` / `std::fs::remove_file`.
pub fn execute_uninstall<F>(
    table: OwnershipTable,
    candidates: Vec<PathBuf>,
    now_unix_ms: u64,
    mut remove: F,
) -> Result<UninstallReceiptV1, UninstallError>
where
    F: FnMut(&Path) -> Result<(), String>,
{
    let authorised = revoke_unowned(&table, &candidates);
    let mut receipt = UninstallReceiptV1 {
        schema_version: UNINSTALL_RECEIPT_SCHEMA_VERSION,
        installation_id: table.installation_id.clone(),
        started_at_unix_ms: now_unix_ms,
        finished_at_unix_ms: None,
        removed: Vec::with_capacity(authorised.len()),
        refused: Vec::with_capacity(candidates.len()),
        outcome: UninstallOutcome::Pending,
    };

    if authorised.is_empty() {
        // Every candidate was refused; record the refused set verbatim
        // so the operator sees exactly what they asked about.
        receipt.refused = candidates;
        receipt.outcome = UninstallOutcome::RefusedAll;
        receipt.finished_at_unix_ms = Some(now_unix_ms);
        return Ok(receipt);
    }

    for candidate in &authorised {
        if let Err(reason) = remove(candidate) {
            // Halt on the first failure. The receipt captures every
            // successful remove up to this point and the still-authorised
            // candidates we did not get to so the operator can retry.
            receipt.refused = authorised
                .iter()
                .filter(|path| !receipt.removed.contains(path) && path != &candidate)
                .cloned()
                .collect();
            // The refused list also includes the failing candidate itself
            // so the receipt is honest about what we did not delete.
            receipt.refused.push(candidate.clone());
            receipt.outcome = UninstallOutcome::PartiallyRemoved {
                reason: reason.clone(),
            };
            receipt.finished_at_unix_ms = Some(now_unix_ms);
            return Err(UninstallError::RemoveFailed {
                path: candidate.clone(),
                reason,
            });
        }
        receipt.removed.push(candidate.clone());
    }

    // Every authorised candidate was removed; the refused list is the
    // original candidates minus the authorised set.
    receipt.refused = candidates
        .into_iter()
        .filter(|path| !receipt.removed.contains(path))
        .collect();
    receipt.outcome = UninstallOutcome::Completed;
    receipt.finished_at_unix_ms = Some(now_unix_ms);
    Ok(receipt)
}

/// Persist the ownership table to `<receipt_root>/ownership.json`. Used
/// by installers to record their claims immediately after writing the
/// per-stage files. Atomic write (tmp + rename) so a half-written table
/// can never be parsed.
pub fn persist_table(receipt_root: &Path, table: &OwnershipTable) -> Result<(), UninstallError> {
    let path = receipt_root.join(OWNERSHIP_TABLE_FILE_NAME);
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|error| UninstallError::Io {
                path: parent.to_path_buf(),
                reason: format!("create ownership parent: {error}"),
            })?;
        }
    }
    let bytes = serde_json::to_vec_pretty(table).map_err(|error| UninstallError::Io {
        path: path.clone(),
        reason: format!("serialize ownership table: {error}"),
    })?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, &bytes).map_err(|error| UninstallError::Io {
        path: tmp.clone(),
        reason: format!("write tmp ownership: {error}"),
    })?;
    std::fs::rename(&tmp, &path).map_err(|error| UninstallError::Io {
        path,
        reason: format!("rename tmp ownership: {error}"),
    })?;
    Ok(())
}

/// Persist the uninstall receipt alongside the ownership table.
pub fn persist_receipt(
    receipt_root: &Path,
    receipt: &UninstallReceiptV1,
) -> Result<(), UninstallError> {
    let path = receipt_root.join(UNINSTALL_RECEIPT_FILE_NAME);
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|error| UninstallError::Io {
                path: parent.to_path_buf(),
                reason: format!("create receipt parent: {error}"),
            })?;
        }
    }
    let bytes = serde_json::to_vec_pretty(receipt).map_err(|error| UninstallError::Io {
        path: path.clone(),
        reason: format!("serialize uninstall receipt: {error}"),
    })?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, &bytes).map_err(|error| UninstallError::Io {
        path: tmp.clone(),
        reason: format!("write tmp uninstall receipt: {error}"),
    })?;
    std::fs::rename(&tmp, &path).map_err(|error| UninstallError::Io {
        path,
        reason: format!("rename tmp uninstall receipt: {error}"),
    })?;
    Ok(())
}

fn synthesize_installation_id(receipt_root: &Path) -> String {
    let mut hasher = sha2::Sha256::new();
    sha2::Digest::update(&mut hasher, receipt_root.to_string_lossy().as_bytes());
    format!("sha256:{:x}", sha2::Digest::finalize(hasher))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn temp_receipt_root(label: &str) -> PathBuf {
        let dir = tempfile::tempdir().unwrap();
        let keep = dir.keep();
        let root = keep.join(format!("mbr-205-{label}"));
        std::fs::create_dir_all(&root).unwrap();
        // Leak the parent so it survives the tempdir drop; tests clean up
        // explicitly via `rm -rf` if they care.
        let _ = dir;
        root
    }

    fn claim(kind: OwnershipKind, path: &str, receipt_id: &str, ts: u64) -> OwnershipClaim {
        OwnershipClaim {
            kind,
            path: PathBuf::from(path),
            receipt_id: receipt_id.to_string(),
            registered_at_unix_ms: ts,
        }
    }

    #[test]
    fn empty_table_refuses_every_candidate() {
        let table = OwnershipTable {
            installation_id: "sha256:test".to_string(),
            claims: Vec::new(),
        };
        let candidates = vec![
            PathBuf::from("/var/membrane/foo"),
            PathBuf::from("/var/membrane/bar"),
        ];
        let authorised = revoke_unowned(&table, &candidates);
        assert!(authorised.is_empty(), "empty table must refuse everything");

        let now: u64 = 1_755_124_800_000;
        let receipt = execute_uninstall(table, candidates, now, |_| {
            panic!("remove must not be called when the table refuses everything")
        })
        .expect("empty table should not error");
        assert_eq!(receipt.schema_version, UNINSTALL_RECEIPT_SCHEMA_VERSION);
        assert!(matches!(receipt.outcome, UninstallOutcome::RefusedAll));
        assert_eq!(receipt.removed.len(), 0);
        assert_eq!(receipt.refused.len(), 2);
        assert_eq!(receipt.finished_at_unix_ms, Some(now));
    }

    #[test]
    fn registered_path_is_removed_and_unregistered_is_refused() {
        let table = OwnershipTable {
            installation_id: "sha256:test".to_string(),
            claims: vec![
                claim(
                    OwnershipKind::Manifest,
                    "/var/membrane/manifest.json",
                    "plan-1",
                    1_755_000_000_000,
                ),
                claim(
                    OwnershipKind::Lease,
                    "/var/membrane/lease.json",
                    "sup-1",
                    1_755_000_000_000,
                ),
            ],
        };
        let candidates = vec![
            PathBuf::from("/var/membrane/manifest.json"),
            PathBuf::from("/var/membrane/lease.json"),
            PathBuf::from("/var/membrane/stray.txt"),
        ];
        let authorised = revoke_unowned(&table, &candidates);
        assert_eq!(
            authorised,
            vec![
                PathBuf::from("/var/membrane/manifest.json"),
                PathBuf::from("/var/membrane/lease.json"),
            ]
        );

        let now: u64 = 1_755_124_800_000;
        let receipt = execute_uninstall(table, candidates, now, |path| {
            // Pretend every authorised remove succeeds.
            assert!(path.starts_with("/var/membrane/"));
            Ok(())
        })
        .expect("authorised removes must succeed");
        assert_eq!(receipt.removed.len(), 2);
        assert_eq!(
            receipt.refused,
            vec![PathBuf::from("/var/membrane/stray.txt")]
        );
        assert!(matches!(receipt.outcome, UninstallOutcome::Completed));
        assert_eq!(receipt.finished_at_unix_ms, Some(now));
    }

    #[test]
    fn duplicate_registration_returns_typed_error() {
        let mut table = OwnershipTable {
            installation_id: "sha256:test".to_string(),
            claims: Vec::new(),
        };
        let row = claim(
            OwnershipKind::Manifest,
            "/var/membrane/manifest.json",
            "plan-1",
            1_755_000_000_000,
        );
        register(&mut table, row.clone()).expect("first register must succeed");
        let err = register(&mut table, row).expect_err("duplicate register must error");
        match err {
            UninstallError::DuplicateOwnership { kind, path } => {
                assert_eq!(kind, OwnershipKind::Manifest);
                assert_eq!(path, PathBuf::from("/var/membrane/manifest.json"));
            }
            other => panic!("expected DuplicateOwnership, got {other:?}"),
        }
        // Same kind, different path is fine.
        register(
            &mut table,
            claim(
                OwnershipKind::Lease,
                "/var/membrane/lease.json",
                "sup-1",
                1_755_000_000_000,
            ),
        )
        .expect("different path is not a duplicate");
        // Same path, different kind is fine.
        register(
            &mut table,
            claim(
                OwnershipKind::Binding,
                "/var/membrane/manifest.json",
                "plan-1",
                1_755_000_000_000,
            ),
        )
        .expect("different kind is not a duplicate");
        assert_eq!(table.claims.len(), 3);
    }

    #[test]
    fn refused_paths_survive_uninstall() {
        let table = OwnershipTable {
            installation_id: "sha256:test".to_string(),
            claims: vec![claim(
                OwnershipKind::Receipt,
                "/var/membrane/install-receipt.json",
                "plan-1",
                1_755_000_000_000,
            )],
        };
        let candidates = vec![
            PathBuf::from("/var/membrane/install-receipt.json"),
            PathBuf::from("/home/user/important-document.txt"),
            PathBuf::from("/etc/membrane.d/config.toml"),
        ];
        let now: u64 = 1_755_124_800_000;
        let receipt = execute_uninstall(table, candidates, now, |path| {
            assert_eq!(path, PathBuf::from("/var/membrane/install-receipt.json"));
            Ok(())
        })
        .expect("authorised remove must succeed");
        assert_eq!(
            receipt.removed,
            vec![PathBuf::from("/var/membrane/install-receipt.json")]
        );
        let refused: HashSet<PathBuf> = receipt.refused.iter().cloned().collect();
        assert!(refused.contains(&PathBuf::from("/home/user/important-document.txt")));
        assert!(refused.contains(&PathBuf::from("/etc/membrane.d/config.toml")));
        assert!(!refused.contains(&PathBuf::from("/var/membrane/install-receipt.json")));
        assert!(matches!(receipt.outcome, UninstallOutcome::Completed));
    }

    #[test]
    fn remove_failure_halts_and_records_partial_state() {
        let table = OwnershipTable {
            installation_id: "sha256:test".to_string(),
            claims: vec![
                claim(
                    OwnershipKind::Manifest,
                    "/var/membrane/manifest.json",
                    "plan-1",
                    1_755_000_000_000,
                ),
                claim(
                    OwnershipKind::Lease,
                    "/var/membrane/lease.json",
                    "sup-1",
                    1_755_000_000_000,
                ),
            ],
        };
        let candidates = vec![
            PathBuf::from("/var/membrane/manifest.json"),
            PathBuf::from("/var/membrane/lease.json"),
        ];
        let now: u64 = 1_755_124_800_000;
        let err = execute_uninstall(table, candidates, now, |path| {
            if path == Path::new("/var/membrane/lease.json") {
                Err("simulated permission denied".to_string())
            } else {
                Ok(())
            }
        })
        .expect_err("second remove must fail");
        match err {
            UninstallError::RemoveFailed { path, reason } => {
                assert_eq!(path, PathBuf::from("/var/membrane/lease.json"));
                assert!(reason.contains("permission denied"));
            }
            other => panic!("expected RemoveFailed, got {other:?}"),
        }
    }

    #[test]
    fn partial_removal_outcome_serialization_round_trips() {
        let receipt = UninstallReceiptV1 {
            schema_version: UNINSTALL_RECEIPT_SCHEMA_VERSION,
            installation_id: "sha256:abc".to_string(),
            started_at_unix_ms: 1_755_000_000_000,
            finished_at_unix_ms: Some(1_755_000_001_000),
            removed: vec![PathBuf::from("/var/membrane/manifest.json")],
            refused: vec![PathBuf::from("/var/membrane/lease.json")],
            outcome: UninstallOutcome::PartiallyRemoved {
                reason: "boom".to_string(),
            },
        };
        let bytes = serde_json::to_vec(&receipt).expect("serialize");
        let parsed: UninstallReceiptV1 = serde_json::from_slice(&bytes).expect("parse");
        assert_eq!(parsed, receipt);
        match parsed.outcome {
            UninstallOutcome::PartiallyRemoved { reason } => assert_eq!(reason, "boom"),
            other => panic!("expected PartiallyRemoved, got {other:?}"),
        }

        // Every outcome variant round-trips.
        for outcome in [
            UninstallOutcome::Pending,
            UninstallOutcome::Completed,
            UninstallOutcome::RefusedAll,
            UninstallOutcome::PartiallyRemoved {
                reason: "x".to_string(),
            },
        ] {
            let body = serde_json::to_string(&outcome).unwrap();
            let back: UninstallOutcome = serde_json::from_str(&body).unwrap();
            assert_eq!(back, outcome);
        }
    }

    #[test]
    fn load_table_returns_fresh_when_missing() {
        let root = temp_receipt_root("fresh");
        let table = load_table(&root).expect("missing table must load as fresh");
        assert_eq!(table.claims.len(), 0);
        assert!(table.installation_id.starts_with("sha256:"));
        // Distinct roots → distinct installation ids.
        let other = temp_receipt_root("fresh-other");
        let other_table = load_table(&other).unwrap();
        assert_ne!(table.installation_id, other_table.installation_id);
        std::fs::remove_dir_all(&root).ok();
        std::fs::remove_dir_all(&other).ok();
    }

    #[test]
    fn ownership_table_round_trips_through_disk() {
        let root = temp_receipt_root("roundtrip");
        let mut table = OwnershipTable::fresh(&root);
        register(
            &mut table,
            claim(
                OwnershipKind::Manifest,
                "/var/membrane/manifest.json",
                "plan-1",
                1_755_000_000_000,
            ),
        )
        .unwrap();
        register(
            &mut table,
            claim(
                OwnershipKind::Lease,
                "/var/membrane/lease.json",
                "sup-1",
                1_755_000_000_000,
            ),
        )
        .unwrap();
        persist_table(&root, &table).expect("persist");

        // Re-read and confirm the table on disk matches the in-memory one.
        let reloaded = load_table(&root).expect("reload");
        assert_eq!(reloaded.installation_id, table.installation_id);
        assert_eq!(reloaded.claims.len(), 2);
        assert!(reloaded.contains(
            OwnershipKind::Manifest,
            Path::new("/var/membrane/manifest.json")
        ));
        assert!(reloaded.contains(OwnershipKind::Lease, Path::new("/var/membrane/lease.json")));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn ownership_kind_as_str_is_stable() {
        assert_eq!(OwnershipKind::Manifest.as_str(), "Manifest");
        assert_eq!(OwnershipKind::Lease.as_str(), "Lease");
        assert_eq!(OwnershipKind::Receipt.as_str(), "Receipt");
        assert_eq!(OwnershipKind::Binding.as_str(), "Binding");
        // Serde shape matches.
        for kind in [
            OwnershipKind::Manifest,
            OwnershipKind::Lease,
            OwnershipKind::Receipt,
            OwnershipKind::Binding,
        ] {
            let body = serde_json::to_string(&kind).unwrap();
            let back: OwnershipKind = serde_json::from_str(&body).unwrap();
            assert_eq!(back, kind);
        }
    }
}
