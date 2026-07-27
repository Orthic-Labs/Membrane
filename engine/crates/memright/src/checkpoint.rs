//! Machine-local session checkpoints. They occupy the canonical `memories` table but carry the
//! separate `session` artifact family, A0 authority, orientation influence, and a hard TTL.

use crate::store::MemoryStore;
use rusqlite::{OptionalExtension, TransactionBehavior};

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CheckpointV1 {
    pub checkpoint_id: String,
    pub client: String,
    pub session_id: String,
    pub repository_id: String,
    pub worktree_rev: String,
    pub scope_id: String,
    pub summary: String,
    pub created_at_ms: i64,
    pub expires_at_ms: i64,
    #[serde(default)]
    pub source_refs: Vec<CheckpointSourceRefV1>,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CheckpointSourceRefV1 {
    pub source_ref: String,
    pub expected_content_hash: String,
    pub anchor_id: String,
    pub label: String,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckpointSourceResolutionV1 {
    pub source_ref: String,
    pub anchor_id: String,
    pub status: String,
}

/// Resolve checkpoint links only through DocReadV1. A raw filesystem path is never returned.
pub fn resolve_source_refs(
    checkpoint: &CheckpointV1,
    workspace_root: &std::path::Path,
) -> Vec<CheckpointSourceResolutionV1> {
    checkpoint
        .source_refs
        .iter()
        .map(|reference| {
            let status = reference
                .source_ref
                .strip_prefix("doc://repo/worktree/")
                .filter(|path| !path.split('/').any(|segment| segment == ".."))
                .and_then(|path| std::fs::read_to_string(workspace_root.join(path)).ok())
                .map(|markdown| {
                    match crate::outline::read_section(
                        &reference.source_ref,
                        &markdown,
                        &reference.anchor_id,
                        &reference.expected_content_hash,
                        12_000,
                    ) {
                        Ok(_) => "ok".to_string(),
                        Err(error) => error.to_string(),
                    }
                })
                .unwrap_or_else(|| {
                    if reference.source_ref.starts_with("doc://repo/worktree/") {
                        "source_missing".into()
                    } else {
                        "deny".into()
                    }
                });
            CheckpointSourceResolutionV1 {
                source_ref: reference.source_ref.clone(),
                anchor_id: reference.anchor_id.clone(),
                status,
            }
        })
        .collect()
}

#[derive(Debug, thiserror::Error)]
pub enum CheckpointError {
    #[error("checkpoint is invalid: {0}")]
    Invalid(String),
    #[error("checkpoint does not exist: {0}")]
    Missing(String),
    #[error("checkpoint is expired: {0}")]
    Expired(String),
    #[error("checkpoint persistence failed: {0}")]
    Persist(#[from] rusqlite::Error),
    #[error("checkpoint encoding failed: {0}")]
    Encode(#[from] serde_json::Error),
}

impl CheckpointV1 {
    fn validate(&self) -> Result<(), CheckpointError> {
        for (name, value) in [
            ("checkpoint_id", &self.checkpoint_id),
            ("client", &self.client),
            ("session_id", &self.session_id),
            ("repository_id", &self.repository_id),
            ("worktree_rev", &self.worktree_rev),
            ("scope_id", &self.scope_id),
        ] {
            if value.trim().is_empty() {
                return Err(CheckpointError::Invalid(format!("{name} is required")));
            }
        }
        if self.created_at_ms < 0 || self.expires_at_ms <= self.created_at_ms {
            return Err(CheckpointError::Invalid(
                "TTL must be a positive millisecond interval".into(),
            ));
        }
        Ok(())
    }
}

impl MemoryStore {
    pub fn save_checkpoint(&self, checkpoint: &CheckpointV1) -> Result<(), CheckpointError> {
        checkpoint.validate()?;
        let content = serde_json::to_string(checkpoint)?;
        let mut conn = self.db().lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "INSERT INTO memories
             (id, tier, content, keywords, score, created_at, updated_at, access_count, scope_id,
              artifact_family, producer, record_type, authority, influence_class, lifecycle_state,
              expires_at_ms)
             VALUES (?1, 'Episodic', ?2, '[]', 0.0, ?3, ?3, 0, ?4,
                     'session', 'checkpoint', 'checkpoint', 'A0', 'orientation', 'active', ?5)
             ON CONFLICT(id) DO UPDATE SET
               content=excluded.content, updated_at=excluded.updated_at, scope_id=excluded.scope_id,
               artifact_family='session', producer='checkpoint', record_type='checkpoint',
               authority='A0', influence_class='orientation', lifecycle_state='active',
               expires_at_ms=excluded.expires_at_ms",
            rusqlite::params![
                &checkpoint.checkpoint_id,
                content,
                checkpoint.created_at_ms.to_string(),
                &checkpoint.scope_id,
                checkpoint.expires_at_ms,
            ],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn load_checkpoint(
        &self,
        checkpoint_id: &str,
        as_of_ms: i64,
    ) -> Result<CheckpointV1, CheckpointError> {
        let conn = self.db().lock();
        let row: Option<(String, i64, String)> = conn.query_row(
            "SELECT content, expires_at_ms, lifecycle_state FROM memories
             WHERE id=?1 AND artifact_family='session' AND record_type='checkpoint' AND authority='A0'",
            [checkpoint_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        ).optional()?;
        let Some((content, expires_at_ms, state)) = row else {
            return Err(CheckpointError::Missing(checkpoint_id.into()));
        };
        if state != "active" || expires_at_ms <= as_of_ms {
            return Err(CheckpointError::Expired(checkpoint_id.into()));
        }
        Ok(serde_json::from_str(&content)?)
    }

    pub fn close_checkpoint(&self, checkpoint_id: &str) -> Result<(), CheckpointError> {
        let changed = self.db().lock().execute(
            "UPDATE memories SET lifecycle_state='retired' WHERE id=?1 AND artifact_family='session' AND record_type='checkpoint'",
            [checkpoint_id],
        )?;
        if changed == 1 {
            Ok(())
        } else {
            Err(CheckpointError::Missing(checkpoint_id.into()))
        }
    }

    pub fn list_checkpoints(
        &self,
        scope_id: &str,
        as_of_ms: i64,
    ) -> Result<Vec<CheckpointV1>, CheckpointError> {
        let conn = self.db().lock();
        let mut statement = conn.prepare(
            "SELECT content FROM memories WHERE scope_id=?1 AND artifact_family='session'
             AND record_type='checkpoint' AND authority='A0' AND lifecycle_state='active'
             AND expires_at_ms > ?2 ORDER BY expires_at_ms DESC",
        )?;
        let rows = statement.query_map(rusqlite::params![scope_id, as_of_ms], |row| {
            row.get::<_, String>(0)
        })?;
        rows.map(|row| Ok(serde_json::from_str(&row?)?)).collect()
    }
}
