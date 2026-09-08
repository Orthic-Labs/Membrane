//! Machine-local session checkpoints. They occupy the canonical `memories` table but carry the
//! separate `session` artifact family, A0 authority, orientation influence, and a hard TTL.

use crate::store::MemoryStore;
use rusqlite::{OptionalExtension, TransactionBehavior};

/// Checkpoint payloads are intentionally small: they are session continuity
/// state, never an unbounded carrier for durable content.
pub const MAX_CHECKPOINT_BYTES: usize = 16 * 1024;

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CheckpointV1 {
    pub checkpoint_id: String,
    /// Installation identity binds this machine-local orientation record to its origin.
    pub installation_id: String,
    pub client: String,
    pub session_id: String,
    pub repository_id: String,
    pub worktree_rev: String,
    pub scope_id: String,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal_snapshot: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_snapshot: Option<String>,
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
            let status = resolve_source_ref(reference, workspace_root);
            CheckpointSourceResolutionV1 {
                source_ref: reference.source_ref.clone(),
                anchor_id: reference.anchor_id.clone(),
                status,
            }
        })
        .collect()
}

fn resolve_source_ref(
    reference: &CheckpointSourceRefV1,
    workspace_root: &std::path::Path,
) -> String {
    let Ok(source_ref) = crate::ledger::identifier::WorktreeDocRef::parse(&reference.source_ref)
    else {
        return "deny".into();
    };
    let relative = std::path::Path::new(source_ref.relative_path());
    let Ok(root) = workspace_root.canonicalize() else {
        return "deny".into();
    };
    let path = workspace_root.join(relative);
    match read_workspace_text(&root, &path) {
        Some(markdown) => {
            if crate::ledger::outline::read_section(
                &reference.source_ref,
                &markdown,
                &reference.anchor_id,
                &reference.expected_content_hash,
                12_000,
            )
            .is_ok()
            {
                "ok".into()
            } else {
                "changed".into()
            }
        }
        None if workspace_contains_hash(&root, &reference.expected_content_hash) => {
            "relocated".into()
        }
        None => "missing".into(),
    }
}

/// Read only regular, canonicalized files that remain within the granted workspace root.
fn read_workspace_text(root: &std::path::Path, path: &std::path::Path) -> Option<String> {
    let metadata = std::fs::symlink_metadata(path).ok()?;
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        return None;
    }
    let canonical = path.canonicalize().ok()?;
    if !canonical.starts_with(root) {
        return None;
    }
    std::fs::read_to_string(canonical).ok()
}

/// A missing source can be resumed only when identical content is still present in this workspace.
fn workspace_contains_hash(root: &std::path::Path, expected_hash: &str) -> bool {
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                pending.push(entry.path());
                continue;
            }
            if !file_type.is_file() {
                continue;
            }
            let Some(markdown) = read_workspace_text(root, &entry.path()) else {
                continue;
            };
            if crate::ledger::outline::build_outline(
                "doc://repo/worktree/relocated",
                &markdown,
                "comrak-0.54.0",
            )
            .content_hash
                == expected_hash
            {
                return true;
            }
        }
    }
    false
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
    #[error("checkpoint payload is corrupt: {0}")]
    Corrupt(String),
    #[error("checkpoint binding denied: {0}")]
    ScopeDenied(String),
    #[error("checkpoint id collision: {0}")]
    IdCollision(String),
    #[error("checkpoint payload exceeds {MAX_CHECKPOINT_BYTES} bytes")]
    PayloadTooLarge,
}

impl CheckpointV1 {
    pub(crate) fn validate(&self) -> Result<(), CheckpointError> {
        for (name, value) in [
            ("checkpoint_id", &self.checkpoint_id),
            ("installation_id", &self.installation_id),
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

    pub(crate) fn canonical_bytes(&self) -> Result<Vec<u8>, CheckpointError> {
        self.validate()?;
        let bytes = serde_json::to_vec(self)?;
        if bytes.len() > MAX_CHECKPOINT_BYTES {
            return Err(CheckpointError::PayloadTooLarge);
        }
        Ok(bytes)
    }
}

impl MemoryStore {
    pub fn save_checkpoint(&self, checkpoint: &CheckpointV1) -> Result<(), CheckpointError> {
        let content = String::from_utf8(checkpoint.canonical_bytes()?)
            .map_err(|error| CheckpointError::Corrupt(error.to_string()))?;
        let mut conn = self.db().lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let existing: Option<(
            String,
            String,
            String,
            f64,
            String,
            String,
            i64,
            Option<Vec<u8>>,
            Option<Vec<u8>>,
            String,
            i64,
            Option<String>,
            Option<String>,
            String,
            String,
            String,
            String,
            String,
            String,
            String,
            Option<i64>,
        )> = tx
            .query_row(
                "SELECT tier, content, keywords, score, created_at, updated_at, access_count,
                        embedding, embedding_q, scope_id, inject_count, content_hash, embed_model,
                        source_ids, artifact_family, producer, record_type, authority,
                        influence_class, lifecycle_state, expires_at_ms
                 FROM memories WHERE id=?1",
                [&checkpoint.checkpoint_id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                        row.get(7)?,
                        row.get(8)?,
                        row.get(9)?,
                        row.get(10)?,
                        row.get(11)?,
                        row.get(12)?,
                        row.get(13)?,
                        row.get(14)?,
                        row.get(15)?,
                        row.get(16)?,
                        row.get(17)?,
                        row.get(18)?,
                        row.get(19)?,
                        row.get(20)?,
                    ))
                },
            )
            .optional()?;
        if let Some((
            tier,
            existing_content,
            keywords,
            score,
            created_at,
            updated_at,
            access_count,
            embedding,
            embedding_q,
            scope_id,
            inject_count,
            content_hash,
            embed_model,
            source_ids,
            artifact_family,
            producer,
            record_type,
            authority,
            influence_class,
            lifecycle_state,
            expires_at_ms,
        )) = existing
        {
            let exact_replay = tier == "\"Episodic\""
                && existing_content == content
                && keywords == "[]"
                && score == 0.0
                && created_at == checkpoint.created_at_ms.to_string()
                && updated_at == checkpoint.created_at_ms.to_string()
                && access_count == 0
                && embedding.is_none()
                && embedding_q.is_none()
                && scope_id == checkpoint.scope_id
                && inject_count == 0
                && content_hash.is_none()
                && embed_model.is_none()
                && source_ids == "[]"
                && artifact_family == "session"
                && producer == "checkpoint"
                && record_type == "checkpoint"
                && authority == "A0"
                && influence_class == "orientation"
                && lifecycle_state == "active"
                && expires_at_ms == Some(checkpoint.expires_at_ms);
            if exact_replay {
                tx.commit()?;
                return Ok(());
            }
            return Err(CheckpointError::IdCollision(
                checkpoint.checkpoint_id.clone(),
            ));
        }
        tx.execute(
            "INSERT INTO memories
             (id, tier, content, keywords, score, created_at, updated_at, access_count, scope_id,
              artifact_family, producer, record_type, authority, influence_class, lifecycle_state,
              expires_at_ms)
             VALUES (?1, '\"Episodic\"', ?2, '[]', 0.0, ?3, ?3, 0, ?4,
                     'session', 'checkpoint', 'checkpoint', 'A0', 'orientation', 'active', ?5)",
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
        let row: Option<(String, i64, String, String)> = conn.query_row(
            "SELECT content, expires_at_ms, lifecycle_state, scope_id FROM memories
             WHERE id=?1 AND artifact_family='session' AND record_type='checkpoint' AND authority='A0'",
            [checkpoint_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        ).optional()?;
        let Some((content, expires_at_ms, state, _scope_id)) = row else {
            return Err(CheckpointError::Missing(checkpoint_id.into()));
        };
        if state != "active" || expires_at_ms <= as_of_ms {
            return Err(CheckpointError::Expired(checkpoint_id.into()));
        }
        serde_json::from_str(&content)
            .map_err(|error| CheckpointError::Corrupt(error.to_string()))
    }

    /// Load only after row-level scope and lineage binding has been checked.
    /// The content is parsed after those checks so an out-of-scope row cannot
    /// be projected to a caller, even when its payload is malformed.
    pub fn load_checkpoint_bound(
        &self,
        checkpoint_id: &str,
        as_of_ms: i64,
        repository_id: &str,
        scope_id: &str,
        installation_id: &str,
    ) -> Result<CheckpointV1, CheckpointError> {
        let conn = self.db().lock();
        let row: Option<(String, Option<i64>, String, String, String, String, String)> = conn
            .query_row(
                "SELECT content, expires_at_ms, lifecycle_state, scope_id,
                        artifact_family, record_type, authority
                 FROM memories WHERE id=?1",
                [checkpoint_id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                    ))
                },
            )
            .optional()?;
        let Some((content, expires_at_ms, state, row_scope, family, record_type, authority)) = row
        else {
            return Err(CheckpointError::Missing(checkpoint_id.into()));
        };
        if row_scope != scope_id {
            return Err(CheckpointError::ScopeDenied(checkpoint_id.into()));
        }
        if family != "session" || record_type != "checkpoint" || authority != "A0" {
            return Err(CheckpointError::Missing(checkpoint_id.into()));
        }
        let Some(expires_at_ms) = expires_at_ms else {
            return Err(CheckpointError::Corrupt("missing expires_at_ms".into()));
        };
        if state != "active" || expires_at_ms <= as_of_ms {
            return Err(CheckpointError::Expired(checkpoint_id.into()));
        }
        let checkpoint: CheckpointV1 = serde_json::from_str(&content)
            .map_err(|error| CheckpointError::Corrupt(error.to_string()))?;
        if let Err(error) = checkpoint.validate() {
            return Err(CheckpointError::Corrupt(error.to_string()));
        }
        if checkpoint.checkpoint_id != checkpoint_id
            || checkpoint.repository_id != repository_id
            || checkpoint.scope_id != scope_id
            || checkpoint.installation_id != installation_id
        {
            return Err(CheckpointError::ScopeDenied(checkpoint_id.into()));
        }
        Ok(checkpoint)
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
