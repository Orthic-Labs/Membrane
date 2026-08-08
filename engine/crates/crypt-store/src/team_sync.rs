//! MBR-1007: local, durable, fail-closed record of team policy sync opt-in state.
//!
//! `membrane_runtime::team_policy::admit_team_policy_with_opt_in` decides *whether* an incoming
//! [`TeamPolicySyncV1`] is trustworthy (bounds, encryption, authorization, user-origin learning
//! scope preservation) against a caller-supplied, in-memory opt-in record and trust verifier.
//! This module is the store-side complement, mirroring how `maintenance_exec` complements
//! `membrane_supervisor::maintenance`: it owns the two things only durable local storage can
//! verify -- whether this installation has ever explicitly opted in, and whether a generation is
//! monotonically increasing across restarts -- and it persists the result as a content-free
//! audit trail. It never verifies encryption or authorization itself and never becomes the
//! second opinion the runtime layer can lean on for those; `crypt-store` has no crypto verifier
//! and none is added here.
//!
//! ## Guarantees this module makes
//!
//! - **Off by default, and only a local caller can turn it on.** [`MemDb::team_sync_opt_in_status`]
//!   returns `None` until [`MemDb::enable_team_sync_opt_in`] is called explicitly with a real
//!   tenant/team/key id. Nothing in [`TeamPolicySyncV1`] (wire data) can create, modify, or
//!   delete this row -- there is no code path from policy admission back into the opt-in table.
//! - **Fails closed on a missing or mismatched key/policy.** [`MemDb::commit_team_policy_admission`]
//!   refuses -- before opening a transaction, before writing anything -- when there is no opt-in
//!   record, when the record is disabled, when the policy's tenant/team does not match the
//!   opted-in scope, or when the envelope's `key_id` does not match the opted-in key. A key
//!   rotated on the sending side is only followed after an explicit local re-opt-in.
//! - **Never stores plaintext secrets or key material.** The schema below has no column that can
//!   hold a private key, a symmetric key, or decrypted policy content -- only opaque ids and a
//!   `sha256:` ciphertext digest already present on the wire envelope.
//! - **Crash-safe.** The one write path that mutates state runs inside a single `IMMEDIATE`
//!   transaction; a process crash between statements leaves the store exactly as it found it.
//! - **Cannot weaken local root confinement or user-origin learning boundaries.** This module
//!   never reads or writes `memories`, `scope_id`, embeddings, or any other learning-boundary
//!   table; its schema is entirely new, tenant/team/generation bookkeeping tables that no other
//!   Crypt code path reads from or writes to.
//! - **Auditable.** Every call that gets past the three fail-closed gates appends exactly one
//!   content-free row recording what was decided and why, whether or not it was admitted. Local
//!   audit export is reading this ledger back; shipping it anywhere remains an explicit
//!   integration gate, same as `docs/team/policy-sync.md` already states for transport.

use crate::MemDb;
use membrane_protocol::TeamPolicySyncV1;
use rusqlite::{OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};

const TEAM_SYNC_SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS membrane_team_sync_opt_in (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    tenant_id TEXT NOT NULL,
    team_id TEXT NOT NULL,
    key_id TEXT NOT NULL,
    enabled_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS membrane_team_sync_state (
    tenant_id TEXT NOT NULL,
    team_id TEXT NOT NULL,
    generation INTEGER NOT NULL,
    key_id TEXT NOT NULL,
    envelope_id TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (tenant_id, team_id)
);
CREATE TABLE IF NOT EXISTS membrane_team_sync_audit_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    tenant_id TEXT NOT NULL,
    team_id TEXT NOT NULL,
    generation INTEGER NOT NULL,
    policy_id TEXT NOT NULL,
    envelope_id TEXT NOT NULL,
    ciphertext_sha256 TEXT NOT NULL,
    admin_policy_sha256 TEXT NOT NULL,
    key_rotation_id TEXT NOT NULL,
    audit_export_id TEXT NOT NULL,
    offboarded_count INTEGER NOT NULL,
    admitted INTEGER NOT NULL,
    reason TEXT NOT NULL,
    recorded_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_team_sync_audit_tenant_team
    ON membrane_team_sync_audit_log(tenant_id, team_id, generation);
";

/// The local opt-in record. Its mere existence is the opt-in signal: no row means sync is off.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamSyncOptInRecord {
    pub tenant_id: String,
    pub team_id: String,
    pub key_id: String,
    pub enabled_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamSyncCommitOutcome {
    Admitted,
    Rejected,
}

/// Content-free receipt of one admission attempt. Never carries policy content or key material.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamSyncCommitReceipt {
    pub tenant_id: String,
    pub team_id: String,
    pub generation: u64,
    pub outcome: TeamSyncCommitOutcome,
    pub reason: String,
}

/// One row of the durable, content-free local audit trail.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamSyncAuditRecord {
    pub tenant_id: String,
    pub team_id: String,
    pub generation: u64,
    pub policy_id: String,
    pub envelope_id: String,
    pub ciphertext_sha256: String,
    pub admin_policy_sha256: String,
    pub key_rotation_id: String,
    pub audit_export_id: String,
    pub offboarded_count: u32,
    pub admitted: bool,
    pub reason: String,
    pub recorded_at: String,
}

#[derive(Debug, thiserror::Error)]
pub enum TeamSyncStoreError {
    #[error("policy failed structural bounds validation; refusing to touch team sync state")]
    InvalidBounds,
    #[error(
        "team sync is not opted in on this installation; refusing to admit or persist any policy"
    )]
    NotOptedIn,
    #[error("policy tenant/team does not match the locally opted-in tenant/team")]
    TenantOrTeamMismatch,
    #[error(
        "policy envelope key_id does not match the locally opted-in key_id; rotate via explicit opt-in first"
    )]
    KeyMismatch,
    #[error("opt-in call requires a non-empty tenant_id, team_id, key_id, and enabled_at")]
    InvalidOptIn,
    #[error("team sync store: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

impl MemDb {
    fn ensure_team_sync_schema(&self) -> rusqlite::Result<()> {
        self.lock().execute_batch(TEAM_SYNC_SCHEMA)
    }

    /// Explicit local opt-in (MBR-1007). This is the *only* function that can make
    /// [`team_sync_opt_in_status`](Self::team_sync_opt_in_status) return `Some`; nothing derived
    /// from wire data reaches this path. Calling it again (e.g. after a key rotation) overwrites
    /// the single opt-in row -- there is exactly one local opt-in scope at a time, by design.
    pub fn enable_team_sync_opt_in(
        &self,
        tenant_id: &str,
        team_id: &str,
        key_id: &str,
        enabled_at: &str,
    ) -> Result<TeamSyncOptInRecord, TeamSyncStoreError> {
        if tenant_id.is_empty() || team_id.is_empty() || key_id.is_empty() || enabled_at.is_empty()
        {
            return Err(TeamSyncStoreError::InvalidOptIn);
        }
        self.ensure_team_sync_schema()?;
        self.lock().execute(
            "INSERT INTO membrane_team_sync_opt_in (id, tenant_id, team_id, key_id, enabled_at)
             VALUES (1, ?1, ?2, ?3, ?4)
             ON CONFLICT(id) DO UPDATE SET
                 tenant_id=excluded.tenant_id, team_id=excluded.team_id,
                 key_id=excluded.key_id, enabled_at=excluded.enabled_at",
            rusqlite::params![tenant_id, team_id, key_id, enabled_at],
        )?;
        Ok(TeamSyncOptInRecord {
            tenant_id: tenant_id.into(),
            team_id: team_id.into(),
            key_id: key_id.into(),
            enabled_at: enabled_at.into(),
        })
    }

    /// Explicit local opt-out. Deletes only the opt-in row; per-tenant/team generation state and
    /// the audit trail are untouched, so a later re-opt-in does not erase prior audit evidence.
    pub fn disable_team_sync_opt_in(&self) -> Result<(), TeamSyncStoreError> {
        self.ensure_team_sync_schema()?;
        self.lock()
            .execute("DELETE FROM membrane_team_sync_opt_in WHERE id = 1", [])?;
        Ok(())
    }

    /// `None` means team sync is off on this installation (the default).
    pub fn team_sync_opt_in_status(
        &self,
    ) -> Result<Option<TeamSyncOptInRecord>, TeamSyncStoreError> {
        self.ensure_team_sync_schema()?;
        let conn = self.lock();
        Ok(conn
            .query_row(
                "SELECT tenant_id, team_id, key_id, enabled_at
                   FROM membrane_team_sync_opt_in WHERE id = 1",
                [],
                |row| {
                    Ok(TeamSyncOptInRecord {
                        tenant_id: row.get(0)?,
                        team_id: row.get(1)?,
                        key_id: row.get(2)?,
                        enabled_at: row.get(3)?,
                    })
                },
            )
            .optional()?)
    }

    /// The fail-closed local gate for admitting a team policy (MBR-1007). `runtime_admitted` and
    /// `runtime_reason` are the outcome of `membrane_runtime::team_policy::admit_team_policy` (or
    /// `admit_team_policy_with_opt_in`) -- the only place bounds/encryption/authorization/
    /// user-origin-learning-scope trust is decided. This function does not re-derive or trust
    /// its own opinion of those; it re-derives only what durable local storage can verify itself:
    ///
    /// 1. **Opt-in.** No opt-in row, or a policy whose tenant/team does not match it, or whose
    ///    envelope `key_id` does not match it, refuses *before opening a transaction and before
    ///    writing anything* -- mirroring how `execute_bounded_maintenance` refuses on a missing
    ///    authority receipt before touching storage.
    /// 2. **Generation monotonicity.** Even if `runtime_admitted` is `true`, a generation that
    ///    does not strictly exceed this tenant/team's last locally committed generation is
    ///    rejected as a replay -- a second, durable check that outlives any single verifier call.
    ///
    /// Once past those, exactly one row is appended to the content-free audit log recording the
    /// final decision (admitted or rejected) and, only when admitted, the per-tenant/team
    /// generation state advances. Both writes share one `IMMEDIATE` transaction.
    pub fn commit_team_policy_admission(
        &self,
        policy: &TeamPolicySyncV1,
        runtime_admitted: bool,
        runtime_reason: &str,
    ) -> Result<TeamSyncCommitReceipt, TeamSyncStoreError> {
        if !policy.has_valid_bounds() {
            return Err(TeamSyncStoreError::InvalidBounds);
        }
        self.ensure_team_sync_schema()?;

        let opt_in = self
            .team_sync_opt_in_status()?
            .ok_or(TeamSyncStoreError::NotOptedIn)?;
        if opt_in.tenant_id != policy.tenant_id || opt_in.team_id != policy.team_id {
            return Err(TeamSyncStoreError::TenantOrTeamMismatch);
        }
        if opt_in.key_id != policy.envelope.key_id {
            return Err(TeamSyncStoreError::KeyMismatch);
        }

        let mut conn = self.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;

        let last_generation: Option<i64> = tx
            .query_row(
                "SELECT generation FROM membrane_team_sync_state WHERE tenant_id=?1 AND team_id=?2",
                rusqlite::params![policy.tenant_id, policy.team_id],
                |row| row.get(0),
            )
            .optional()?;
        let policy_generation = policy.generation as i64;
        let is_local_replay = last_generation.is_some_and(|last| policy_generation <= last);

        let admitted = runtime_admitted && !is_local_replay;
        let reason: String = if is_local_replay {
            "replay".into()
        } else {
            runtime_reason.into()
        };

        tx.execute(
            "INSERT INTO membrane_team_sync_audit_log
                (tenant_id, team_id, generation, policy_id, envelope_id, ciphertext_sha256,
                 admin_policy_sha256, key_rotation_id, audit_export_id, offboarded_count,
                 admitted, reason, recorded_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
            rusqlite::params![
                policy.tenant_id,
                policy.team_id,
                policy_generation,
                policy.policy_id,
                policy.envelope.envelope_id,
                policy.envelope.ciphertext_sha256,
                policy.admin_policy_sha256,
                policy.key_rotation_id,
                policy.audit_export_id,
                policy.offboarded_user_ids.len() as i64,
                admitted as i64,
                reason,
                crate::time::now_iso(),
            ],
        )?;

        if admitted {
            tx.execute(
                "INSERT INTO membrane_team_sync_state
                    (tenant_id, team_id, generation, key_id, envelope_id, updated_at)
                 VALUES (?1,?2,?3,?4,?5,?6)
                 ON CONFLICT(tenant_id, team_id) DO UPDATE SET
                     generation=excluded.generation, key_id=excluded.key_id,
                     envelope_id=excluded.envelope_id, updated_at=excluded.updated_at",
                rusqlite::params![
                    policy.tenant_id,
                    policy.team_id,
                    policy_generation,
                    policy.envelope.key_id,
                    policy.envelope.envelope_id,
                    crate::time::now_iso(),
                ],
            )?;
        }

        tx.commit()?;

        Ok(TeamSyncCommitReceipt {
            tenant_id: policy.tenant_id.clone(),
            team_id: policy.team_id.clone(),
            generation: policy.generation,
            outcome: if admitted {
                TeamSyncCommitOutcome::Admitted
            } else {
                TeamSyncCommitOutcome::Rejected
            },
            reason,
        })
    }

    /// Read-only local audit trail for a tenant/team, newest first (MBR-1007 "audit export").
    /// This returns already-durable, content-free records; the export transport itself (writing
    /// to a file, handing to an auditor's collector, etc.) remains an explicit integration gate.
    pub fn team_sync_audit_log(
        &self,
        tenant_id: &str,
        team_id: &str,
        limit: u32,
    ) -> Result<Vec<TeamSyncAuditRecord>, TeamSyncStoreError> {
        self.ensure_team_sync_schema()?;
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT tenant_id, team_id, generation, policy_id, envelope_id, ciphertext_sha256,
                    admin_policy_sha256, key_rotation_id, audit_export_id, offboarded_count,
                    admitted, reason, recorded_at
               FROM membrane_team_sync_audit_log
              WHERE tenant_id=?1 AND team_id=?2
              ORDER BY id DESC LIMIT ?3",
        )?;
        let rows = stmt.query_map(rusqlite::params![tenant_id, team_id, limit], |row| {
            Ok(TeamSyncAuditRecord {
                tenant_id: row.get(0)?,
                team_id: row.get(1)?,
                generation: row.get::<_, i64>(2)? as u64,
                policy_id: row.get(3)?,
                envelope_id: row.get(4)?,
                ciphertext_sha256: row.get(5)?,
                admin_policy_sha256: row.get(6)?,
                key_rotation_id: row.get(7)?,
                audit_export_id: row.get(8)?,
                offboarded_count: row.get::<_, i64>(9)? as u32,
                admitted: row.get::<_, i64>(10)? != 0,
                reason: row.get(11)?,
                recorded_at: row.get(12)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use membrane_protocol::{
        EncryptedReplicationEnvelopeV1, TeamPolicyScopeV1, TEAM_POLICY_SCHEMA_VERSION,
    };

    fn policy(generation: u64, key_id: &str) -> TeamPolicySyncV1 {
        TeamPolicySyncV1 {
            schema_version: TEAM_POLICY_SCHEMA_VERSION,
            policy_id: "policy-1".into(),
            tenant_id: "tenant-1".into(),
            team_id: "team-1".into(),
            user_id: "user-1".into(),
            generation,
            scopes: vec![
                TeamPolicyScopeV1::Tenant,
                TeamPolicyScopeV1::Team,
                TeamPolicyScopeV1::User,
            ],
            admin_policy_sha256: format!("sha256:{}", "b".repeat(64)),
            offboarded_user_ids: vec!["offboarded-1".into()],
            key_rotation_id: "rot-1".into(),
            audit_export_id: "audit-1".into(),
            envelope: EncryptedReplicationEnvelopeV1 {
                envelope_id: format!("env-{generation}"),
                tenant_id: "tenant-1".into(),
                team_id: "team-1".into(),
                generation,
                ciphertext_sha256: format!("sha256:{}", "a".repeat(64)),
                key_id: key_id.into(),
                replay_nonce: "0123456789abcdef".into(),
            },
        }
    }

    #[test]
    fn refuses_without_opt_in_and_writes_nothing() {
        let db = MemDb::open_in_memory();
        let error = db
            .commit_team_policy_admission(&policy(1, "key-1"), true, "accepted")
            .unwrap_err();
        assert!(matches!(error, TeamSyncStoreError::NotOptedIn));
        assert_eq!(
            db.team_sync_audit_log("tenant-1", "team-1", 10).unwrap(),
            vec![]
        );
    }

    #[test]
    fn refuses_structurally_invalid_policy_before_touching_opt_in() {
        let db = MemDb::open_in_memory();
        db.enable_team_sync_opt_in("tenant-1", "team-1", "key-1", "2026-08-08T00:00:00Z")
            .unwrap();
        let mut bad = policy(1, "key-1");
        bad.scopes.push(TeamPolicyScopeV1::LocalRoot);
        let error = db
            .commit_team_policy_admission(&bad, true, "accepted")
            .unwrap_err();
        assert!(matches!(error, TeamSyncStoreError::InvalidBounds));
    }

    #[test]
    fn refuses_when_tenant_or_team_does_not_match_opt_in() {
        let db = MemDb::open_in_memory();
        db.enable_team_sync_opt_in("tenant-1", "other-team", "key-1", "2026-08-08T00:00:00Z")
            .unwrap();
        let error = db
            .commit_team_policy_admission(&policy(1, "key-1"), true, "accepted")
            .unwrap_err();
        assert!(matches!(error, TeamSyncStoreError::TenantOrTeamMismatch));
        assert_eq!(
            db.team_sync_audit_log("tenant-1", "team-1", 10).unwrap(),
            vec![]
        );
    }

    #[test]
    fn refuses_when_key_id_does_not_match_opt_in_key() {
        let db = MemDb::open_in_memory();
        db.enable_team_sync_opt_in(
            "tenant-1",
            "team-1",
            "key-rotated-away",
            "2026-08-08T00:00:00Z",
        )
        .unwrap();
        let error = db
            .commit_team_policy_admission(&policy(1, "key-1"), true, "accepted")
            .unwrap_err();
        assert!(matches!(error, TeamSyncStoreError::KeyMismatch));
        assert_eq!(
            db.team_sync_audit_log("tenant-1", "team-1", 10).unwrap(),
            vec![]
        );
    }

    #[test]
    fn commits_and_advances_generation_when_admitted_and_matches() {
        let db = MemDb::open_in_memory();
        db.enable_team_sync_opt_in("tenant-1", "team-1", "key-1", "2026-08-08T00:00:00Z")
            .unwrap();
        let receipt = db
            .commit_team_policy_admission(&policy(1, "key-1"), true, "accepted")
            .unwrap();
        assert_eq!(receipt.outcome, TeamSyncCommitOutcome::Admitted);
        let log = db.team_sync_audit_log("tenant-1", "team-1", 10).unwrap();
        assert_eq!(log.len(), 1);
        assert!(log[0].admitted);
        assert_eq!(log[0].offboarded_count, 1);
    }

    #[test]
    fn runtime_rejection_is_recorded_but_never_advances_generation() {
        let db = MemDb::open_in_memory();
        db.enable_team_sync_opt_in("tenant-1", "team-1", "key-1", "2026-08-08T00:00:00Z")
            .unwrap();
        let receipt = db
            .commit_team_policy_admission(&policy(1, "key-1"), false, "untrusted_encryption")
            .unwrap();
        assert_eq!(receipt.outcome, TeamSyncCommitOutcome::Rejected);
        assert_eq!(receipt.reason, "untrusted_encryption");
        let log = db.team_sync_audit_log("tenant-1", "team-1", 10).unwrap();
        assert_eq!(log.len(), 1);
        assert!(!log[0].admitted);

        // Because generation never advanced, the very same generation can still be admitted next.
        let second = db
            .commit_team_policy_admission(&policy(1, "key-1"), true, "accepted")
            .unwrap();
        assert_eq!(second.outcome, TeamSyncCommitOutcome::Admitted);
    }

    #[test]
    fn local_replay_is_rejected_even_when_runtime_claims_admitted() {
        let db = MemDb::open_in_memory();
        db.enable_team_sync_opt_in("tenant-1", "team-1", "key-1", "2026-08-08T00:00:00Z")
            .unwrap();
        db.commit_team_policy_admission(&policy(5, "key-1"), true, "accepted")
            .unwrap();

        let replay = db
            .commit_team_policy_admission(&policy(5, "key-1"), true, "accepted")
            .unwrap();
        assert_eq!(replay.outcome, TeamSyncCommitOutcome::Rejected);
        assert_eq!(replay.reason, "replay");

        let stale = db
            .commit_team_policy_admission(&policy(4, "key-1"), true, "accepted")
            .unwrap();
        assert_eq!(stale.outcome, TeamSyncCommitOutcome::Rejected);
        assert_eq!(stale.reason, "replay");

        let log = db.team_sync_audit_log("tenant-1", "team-1", 10).unwrap();
        assert_eq!(log.len(), 3, "every attempt is audited, admitted or not");
    }

    #[test]
    fn opt_in_and_opt_out_roundtrip() {
        let db = MemDb::open_in_memory();
        assert_eq!(db.team_sync_opt_in_status().unwrap(), None);

        let enabled = db
            .enable_team_sync_opt_in("tenant-1", "team-1", "key-1", "2026-08-08T00:00:00Z")
            .unwrap();
        assert_eq!(db.team_sync_opt_in_status().unwrap(), Some(enabled));

        db.disable_team_sync_opt_in().unwrap();
        assert_eq!(db.team_sync_opt_in_status().unwrap(), None);

        // Opting back out fails closed for any subsequent admission.
        let error = db
            .commit_team_policy_admission(&policy(1, "key-1"), true, "accepted")
            .unwrap_err();
        assert!(matches!(error, TeamSyncStoreError::NotOptedIn));
    }

    #[test]
    fn enable_rejects_empty_fields_instead_of_persisting_a_partial_opt_in() {
        let db = MemDb::open_in_memory();
        let error = db
            .enable_team_sync_opt_in("", "team-1", "key-1", "2026-08-08T00:00:00Z")
            .unwrap_err();
        assert!(matches!(error, TeamSyncStoreError::InvalidOptIn));
        assert_eq!(db.team_sync_opt_in_status().unwrap(), None);
    }

    #[test]
    fn schema_has_no_column_that_could_hold_key_material_or_plaintext() {
        let db = MemDb::open_in_memory();
        db.ensure_team_sync_schema().unwrap();
        let conn = db.lock();
        for table in [
            "membrane_team_sync_opt_in",
            "membrane_team_sync_state",
            "membrane_team_sync_audit_log",
        ] {
            let mut stmt = conn
                .prepare(&format!("PRAGMA table_info({table})"))
                .unwrap();
            let columns: Vec<String> = stmt
                .query_map([], |row| row.get::<_, String>(1))
                .unwrap()
                .collect::<rusqlite::Result<_>>()
                .unwrap();
            for banned in ["key_material", "secret", "plaintext", "private_key"] {
                assert!(
                    !columns.iter().any(|c| c.contains(banned)),
                    "{table} must never carry a {banned} column, found columns: {columns:?}"
                );
            }
        }
    }

    #[test]
    fn audit_log_is_scoped_per_tenant_team_and_ordered_newest_first() {
        let db = MemDb::open_in_memory();
        db.enable_team_sync_opt_in("tenant-1", "team-1", "key-1", "2026-08-08T00:00:00Z")
            .unwrap();
        db.commit_team_policy_admission(&policy(1, "key-1"), true, "accepted")
            .unwrap();
        db.commit_team_policy_admission(&policy(2, "key-1"), true, "accepted")
            .unwrap();

        let log = db.team_sync_audit_log("tenant-1", "team-1", 10).unwrap();
        assert_eq!(log.len(), 2);
        assert_eq!(log[0].generation, 2, "newest first");
        assert_eq!(log[1].generation, 1);

        assert_eq!(
            db.team_sync_audit_log("tenant-1", "other-team", 10)
                .unwrap(),
            vec![]
        );
    }
}
