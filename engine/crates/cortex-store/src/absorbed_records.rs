//! Durable sessions, append-only events, tasks, and artifacts.
//!
//! The hot append path is SQLite-only: no embedding, model, or network call is
//! made.  Event rows and their high-water cursor are committed together.

use crate::absorbed_migrations::ensure_absorbed_schema;
use crate::MemDb;
use rusqlite::{params, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const ABSORBED_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProvenanceRef {
    pub source: String,
    #[serde(default)]
    pub source_event_ids: Vec<String>,
    #[serde(default)]
    pub producer: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionRecord {
    pub schema_version: u32,
    pub session_id: String,
    pub scope_id: String,
    pub workspace_root: Option<String>,
    pub permission_mode: Option<String>,
    pub model: Option<String>,
    pub provider: Option<String>,
    pub status: String,
    pub title: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub authority: String,
    pub influence_class: String,
    pub lifecycle: String,
    pub retention: String,
    #[serde(default)]
    pub provenance: Vec<ProvenanceRef>,
    pub content_hash: String,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub started_at_ms: Option<u64>,
    pub ended_at_ms: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionEvent {
    pub schema_version: u32,
    pub session_id: String,
    pub seq: u64,
    pub event_id: String,
    #[serde(rename = "type")]
    pub event_type: String,
    pub payload: serde_json::Value,
    pub scope_id: String,
    pub authority: String,
    pub influence_class: String,
    pub lifecycle: String,
    pub retention: String,
    #[serde(default)]
    pub provenance: Vec<ProvenanceRef>,
    pub content_hash: String,
    pub occurred_at_ms: u64,
    pub recorded_at_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskRecord {
    pub schema_version: u32,
    pub task_id: String,
    pub session_id: String,
    pub scope_id: String,
    pub status: String,
    pub title: String,
    pub goal: Option<String>,
    pub authority: String,
    pub influence_class: String,
    pub lifecycle: String,
    pub retention: String,
    #[serde(default)]
    pub provenance: Vec<ProvenanceRef>,
    pub content_hash: String,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub completed_at_ms: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArtifactRecord {
    pub schema_version: u32,
    pub artifact_id: String,
    pub session_id: Option<String>,
    pub task_id: Option<String>,
    pub handle: String,
    pub content_hash: String,
    pub media_type: String,
    pub byte_length: u64,
    pub scope_id: String,
    pub authority: String,
    pub influence_class: String,
    pub lifecycle: String,
    pub retention: String,
    #[serde(default)]
    pub provenance: Vec<ProvenanceRef>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EventCursor {
    pub session_id: String,
    pub last_seq: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AppendOutcome {
    Inserted(EventCursor),
    AlreadyPresent(EventCursor),
}

#[derive(Debug, thiserror::Error)]
pub enum AbsorbedStoreError {
    #[error("absorbed storage sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("absorbed storage serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("unsupported absorbed schema version")]
    SchemaVersion,
    #[error("empty absorbed record identity")]
    EmptyIdentity,
    #[error("empty absorbed governance field")]
    EmptyGovernance,
    #[error("invalid event sequence")]
    InvalidSequence,
    #[error("event sequence gap: expected {expected}, got {actual}")]
    SequenceGap { expected: u64, actual: u64 },
    #[error("event sequence duplicate: {0}")]
    DuplicateSequence(u64),
    #[error("event sequence reordered")]
    Reordered,
    #[error("event id already belongs to another event: {0}")]
    EventIdConflict(String),
    #[error("event identity does not match session")]
    SessionMismatch,
    #[error("event does not exist: {session_id}:{seq}")]
    EventNotFound { session_id: String, seq: u64 },
    #[error("event tombstone prevents sequence reuse: {session_id}:{seq}")]
    Tombstoned { session_id: String, seq: u64 },
    #[error("event payload must be an object")]
    InvalidPayload,
}

fn required(value: &str) -> bool {
    !value.trim().is_empty()
}

fn validate_provenance(value: &[ProvenanceRef]) -> Result<(), AbsorbedStoreError> {
    if value.iter().any(|item| !required(&item.source)) {
        return Err(AbsorbedStoreError::EmptyGovernance);
    }
    Ok(())
}

fn validate_common(
    schema_version: u32,
    id: &str,
    scope_id: &str,
    authority: &str,
    influence_class: &str,
    lifecycle: &str,
    retention: &str,
    content_hash: &str,
) -> Result<(), AbsorbedStoreError> {
    if schema_version != ABSORBED_SCHEMA_VERSION {
        return Err(AbsorbedStoreError::SchemaVersion);
    }
    if !required(id) {
        return Err(AbsorbedStoreError::EmptyIdentity);
    }
    if ![scope_id, authority, influence_class, lifecycle, retention, content_hash]
        .iter()
        .all(|value| required(value))
    {
        return Err(AbsorbedStoreError::EmptyGovernance);
    }
    Ok(())
}

fn validate_session(value: &SessionRecord) -> Result<(), AbsorbedStoreError> {
    validate_common(
        value.schema_version,
        &value.session_id,
        &value.scope_id,
        &value.authority,
        &value.influence_class,
        &value.lifecycle,
        &value.retention,
        &value.content_hash,
    )?;
    validate_provenance(&value.provenance)
}

fn validate_event(value: &SessionEvent) -> Result<(), AbsorbedStoreError> {
    validate_common(
        value.schema_version,
        &value.event_id,
        &value.scope_id,
        &value.authority,
        &value.influence_class,
        &value.lifecycle,
        &value.retention,
        &value.content_hash,
    )?;
    if !required(&value.session_id) || !required(&value.event_type) {
        return Err(AbsorbedStoreError::EmptyIdentity);
    }
    if value.seq == 0 {
        return Err(AbsorbedStoreError::InvalidSequence);
    }
    if !value.payload.is_object() {
        return Err(AbsorbedStoreError::InvalidPayload);
    }
    validate_provenance(&value.provenance)
}

fn validate_task(value: &TaskRecord) -> Result<(), AbsorbedStoreError> {
    validate_common(
        value.schema_version,
        &value.task_id,
        &value.scope_id,
        &value.authority,
        &value.influence_class,
        &value.lifecycle,
        &value.retention,
        &value.content_hash,
    )?;
    if !required(&value.session_id) || !required(&value.title) {
        return Err(AbsorbedStoreError::EmptyIdentity);
    }
    validate_provenance(&value.provenance)
}

fn validate_artifact(value: &ArtifactRecord) -> Result<(), AbsorbedStoreError> {
    validate_common(
        value.schema_version,
        &value.artifact_id,
        &value.scope_id,
        &value.authority,
        &value.influence_class,
        &value.lifecycle,
        &value.retention,
        &value.content_hash,
    )?;
    if !required(&value.handle) || !required(&value.media_type) {
        return Err(AbsorbedStoreError::EmptyIdentity);
    }
    validate_provenance(&value.provenance)
}

fn json<T: Serialize>(value: &T) -> Result<String, AbsorbedStoreError> {
    Ok(serde_json::to_string(value)?)
}

fn provenance<T: for<'de> Deserialize<'de>>(value: String) -> Result<T, rusqlite::Error> {
    serde_json::from_str(&value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            Box::new(error),
        )
    })
}

pub struct AbsorbedStore {
    db: MemDb,
}

impl AbsorbedStore {
    pub fn new(db: MemDb) -> Result<Self, AbsorbedStoreError> {
        {
            let conn = db.lock();
            ensure_absorbed_schema(&conn)?;
        }
        Ok(Self { db })
    }

    pub fn database(&self) -> MemDb {
        self.db.clone()
    }

    pub fn put_session(&self, value: &SessionRecord) -> Result<(), AbsorbedStoreError> {
        validate_session(value)?;
        let payload = json(value)?;
        let tags = json(&value.tags)?;
        let provenance = json(&value.provenance)?;
        let conn = self.db.lock();
        ensure_absorbed_schema(&conn)?;
        conn.execute(
            "INSERT INTO absorbed_sessions(session_id,scope_id,workspace_root,permission_mode,model,provider,status,title,tags_json,authority,influence_class,lifecycle,retention,provenance_json,content_hash,created_at_ms,updated_at_ms,started_at_ms,ended_at_ms,payload_json)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20)
             ON CONFLICT(session_id) DO UPDATE SET scope_id=excluded.scope_id,workspace_root=excluded.workspace_root,permission_mode=excluded.permission_mode,model=excluded.model,provider=excluded.provider,status=excluded.status,title=excluded.title,tags_json=excluded.tags_json,authority=excluded.authority,influence_class=excluded.influence_class,lifecycle=excluded.lifecycle,retention=excluded.retention,provenance_json=excluded.provenance_json,content_hash=excluded.content_hash,updated_at_ms=excluded.updated_at_ms,started_at_ms=excluded.started_at_ms,ended_at_ms=excluded.ended_at_ms,payload_json=excluded.payload_json",
            params![value.session_id,value.scope_id,value.workspace_root,value.permission_mode,value.model,value.provider,value.status,value.title,tags,value.authority,value.influence_class,value.lifecycle,value.retention,provenance,value.content_hash,value.created_at_ms as i64,value.updated_at_ms as i64,value.started_at_ms.map(|v|v as i64),value.ended_at_ms.map(|v|v as i64),payload],
        )?;
        Ok(())
    }

    pub fn insert_session(&self, value: &SessionRecord) -> Result<(), AbsorbedStoreError> {
        self.put_session(value)
    }

    pub fn update_session(&self, value: &SessionRecord) -> Result<(), AbsorbedStoreError> {
        self.put_session(value)
    }

    pub fn get_session(&self, session_id: &str) -> Result<Option<SessionRecord>, AbsorbedStoreError> {
        let conn = self.db.lock();
        ensure_absorbed_schema(&conn)?;
        conn.query_row("SELECT payload_json FROM absorbed_sessions WHERE session_id=?1", [session_id], |row| row.get::<_, String>(0)).optional()?.map(|payload| serde_json::from_str(&payload).map_err(AbsorbedStoreError::from)).transpose()
    }

    pub fn delete_session(&self, session_id: &str) -> Result<bool, AbsorbedStoreError> {
        let conn = self.db.lock();
        ensure_absorbed_schema(&conn)?;
        let tx = conn.unchecked_transaction()?;
        let event_rows: Vec<(i64, String)> = {
            let mut statement = tx.prepare("SELECT seq,event_id FROM absorbed_events WHERE session_id=?1")?;
            let mut rows = statement.query([session_id])?;
            let mut result = Vec::new();
            while let Some(row) = rows.next()? {
                result.push((row.get(0)?, row.get(1)?));
            }
            result
        };
        for (seq, event_id) in event_rows {
            tx.execute(
                "INSERT OR IGNORE INTO absorbed_event_tombstones(session_id,seq,event_id,tombstoned_at_ms) VALUES (?1,?2,?3,?4)",
                params![session_id, seq, event_id, 0i64],
            )?;
        }
        tx.execute("DELETE FROM absorbed_events WHERE session_id=?1", [session_id])?;
        tx.execute("DELETE FROM absorbed_tasks WHERE session_id=?1", [session_id])?;
        tx.execute("DELETE FROM absorbed_artifacts WHERE session_id=?1", [session_id])?;
        let changed = tx.execute("DELETE FROM absorbed_sessions WHERE session_id=?1", [session_id])?;
        tx.commit()?;
        Ok(changed != 0)
    }

    pub fn append_event(&self, value: &SessionEvent) -> Result<AppendOutcome, AbsorbedStoreError> {
        validate_event(value)?;
        let payload = json(value)?;
        let provenance = json(&value.provenance)?;
        let conn = self.db.lock();
        ensure_absorbed_schema(&conn)?;
        let tx = conn.unchecked_transaction()?;
        let outcome = Self::append_event_in_tx(&tx, value, &payload, &provenance)?;
        tx.commit()?;
        Ok(outcome)
    }

    fn append_event_in_tx(
        tx: &Transaction<'_>,
        value: &SessionEvent,
        payload: &str,
        provenance: &str,
    ) -> Result<AppendOutcome, AbsorbedStoreError> {
        let sequence = i64::try_from(value.seq).map_err(|_| AbsorbedStoreError::InvalidSequence)?;
        let existing: Option<(String, String, i64)> = tx.query_row("SELECT payload_json,session_id,seq FROM absorbed_events WHERE event_id=?1", [&value.event_id], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?))).optional()?;
        if let Some((stored, session_id, seq)) = existing {
            if stored == payload && session_id == value.session_id && seq == sequence {
                let last = cursor_in_tx(&tx, &value.session_id)?;
                return Ok(AppendOutcome::AlreadyPresent(EventCursor { session_id: value.session_id.clone(), last_seq: last }));
            }
            return Err(AbsorbedStoreError::EventIdConflict(value.event_id.clone()));
        }
        if tx.query_row("SELECT 1 FROM absorbed_event_tombstones WHERE session_id=?1 AND seq=?2", params![value.session_id,sequence], |_| Ok(())).optional()?.is_some() {
            return Err(AbsorbedStoreError::Tombstoned { session_id: value.session_id.clone(), seq: value.seq });
        }
        let last = cursor_in_tx(&tx, &value.session_id)?;
        let expected = last.saturating_add(1);
        if value.seq != expected {
            return Err(if value.seq < expected { AbsorbedStoreError::DuplicateSequence(value.seq) } else { AbsorbedStoreError::SequenceGap { expected, actual: value.seq } });
        }
        tx.execute("INSERT INTO absorbed_events(session_id,seq,event_id,event_type,payload_json,scope_id,authority,influence_class,lifecycle,retention,provenance_json,content_hash,occurred_at_ms,recorded_at_ms) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)", params![value.session_id,sequence,value.event_id,value.event_type,payload,value.scope_id,value.authority,value.influence_class,value.lifecycle,value.retention,provenance,value.content_hash,value.occurred_at_ms as i64,value.recorded_at_ms as i64])?;
        tx.execute("INSERT INTO absorbed_event_cursors(session_id,last_seq) VALUES (?1,?2) ON CONFLICT(session_id) DO UPDATE SET last_seq=excluded.last_seq", params![value.session_id,sequence])?;
        Ok(AppendOutcome::Inserted(EventCursor { session_id: value.session_id.clone(), last_seq: value.seq }))
    }

    pub fn append(&self, value: &SessionEvent) -> Result<AppendOutcome, AbsorbedStoreError> {
        self.append_event(value)
    }

    pub fn cursor(&self, session_id: &str) -> Result<EventCursor, AbsorbedStoreError> {
        let conn = self.db.lock();
        ensure_absorbed_schema(&conn)?;
        Ok(EventCursor { session_id: session_id.to_string(), last_seq: conn.query_row("SELECT last_seq FROM absorbed_event_cursors WHERE session_id=?1", [session_id], |row| row.get::<_, i64>(0)).optional()?.unwrap_or(0).max(0) as u64 })
    }

    pub fn events_range(&self, session_id: &str, start_seq: u64, end_seq: u64) -> Result<Vec<SessionEvent>, AbsorbedStoreError> {
        let conn = self.db.lock();
        ensure_absorbed_schema(&conn)?;
        let mut statement = conn.prepare("SELECT payload_json FROM absorbed_events WHERE session_id=?1 AND seq>=?2 AND seq<?3 AND tombstoned=0 ORDER BY seq ASC")?;
        let rows = statement.query_map(params![session_id,start_seq as i64,end_seq as i64], |row| row.get::<_, String>(0))?;
        let mut result = Vec::new();
        for row in rows { result.push(serde_json::from_str(&row?).map_err(AbsorbedStoreError::from)?); }
        Ok(result)
    }

    pub fn range(&self, session_id: &str, start_seq: u64, end_seq: u64) -> Result<Vec<SessionEvent>, AbsorbedStoreError> {
        self.events_range(session_id, start_seq, end_seq)
    }

    pub fn tombstone_event(&self, session_id: &str, seq: u64, tombstoned_at_ms: u64) -> Result<(), AbsorbedStoreError> {
        let conn = self.db.lock();
        ensure_absorbed_schema(&conn)?;
        let tx = conn.unchecked_transaction()?;
        let event_id: String = tx.query_row("SELECT event_id FROM absorbed_events WHERE session_id=?1 AND seq=?2", params![session_id,seq as i64], |row| row.get(0)).optional()?.ok_or_else(|| AbsorbedStoreError::EventNotFound { session_id: session_id.to_string(), seq })?;
        tx.execute("UPDATE absorbed_events SET tombstoned=1 WHERE session_id=?1 AND seq=?2", params![session_id,seq as i64])?;
        tx.execute("INSERT OR IGNORE INTO absorbed_event_tombstones(session_id,seq,event_id,tombstoned_at_ms) VALUES (?1,?2,?3,?4)", params![session_id,seq as i64,event_id,tombstoned_at_ms as i64])?;
        tx.commit()?;
        Ok(())
    }

    pub fn validate_import(events: &[SessionEvent]) -> Result<(), AbsorbedStoreError> {
        let mut ids = HashSet::with_capacity(events.len());
        let mut expected = 1u64;
        let session = events.first().map(|event| event.session_id.as_str());
        for event in events {
            validate_event(event)?;
            if Some(event.session_id.as_str()) != session { return Err(AbsorbedStoreError::SessionMismatch); }
            if !ids.insert(event.event_id.as_str()) { return Err(AbsorbedStoreError::EventIdConflict(event.event_id.clone())); }
            if event.seq != expected { return Err(if event.seq < expected { AbsorbedStoreError::Reordered } else { AbsorbedStoreError::SequenceGap { expected, actual: event.seq } }); }
            expected = expected.saturating_add(1);
        }
        Ok(())
    }

    pub fn import_events(&self, events: &[SessionEvent]) -> Result<usize, AbsorbedStoreError> {
        Self::validate_import(events)?;
        let conn = self.db.lock();
        ensure_absorbed_schema(&conn)?;
        let tx = conn.unchecked_transaction()?;
        let mut inserted = 0;
        for event in events {
            let payload = json(event)?;
            let provenance = json(&event.provenance)?;
            if matches!(Self::append_event_in_tx(&tx, event, &payload, &provenance)?, AppendOutcome::Inserted(_)) {
                inserted += 1;
            }
        }
        tx.commit()?;
        Ok(inserted)
    }

    pub fn put_task(&self, value: &TaskRecord) -> Result<(), AbsorbedStoreError> {
        validate_task(value)?;
        let payload=json(value)?; let provenance=json(&value.provenance)?; let conn=self.db.lock(); ensure_absorbed_schema(&conn)?;
        conn.execute("INSERT INTO absorbed_tasks(task_id,session_id,scope_id,status,title,goal,authority,influence_class,lifecycle,retention,provenance_json,content_hash,created_at_ms,updated_at_ms,completed_at_ms,payload_json) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16) ON CONFLICT(task_id) DO UPDATE SET session_id=excluded.session_id,scope_id=excluded.scope_id,status=excluded.status,title=excluded.title,goal=excluded.goal,authority=excluded.authority,influence_class=excluded.influence_class,lifecycle=excluded.lifecycle,retention=excluded.retention,provenance_json=excluded.provenance_json,content_hash=excluded.content_hash,updated_at_ms=excluded.updated_at_ms,completed_at_ms=excluded.completed_at_ms,payload_json=excluded.payload_json", params![value.task_id,value.session_id,value.scope_id,value.status,value.title,value.goal,value.authority,value.influence_class,value.lifecycle,value.retention,provenance,value.content_hash,value.created_at_ms as i64,value.updated_at_ms as i64,value.completed_at_ms.map(|v|v as i64),payload])?; Ok(())
    }

    pub fn get_task(&self, task_id: &str) -> Result<Option<TaskRecord>, AbsorbedStoreError> { let conn=self.db.lock(); ensure_absorbed_schema(&conn)?; conn.query_row("SELECT payload_json FROM absorbed_tasks WHERE task_id=?1", [task_id], |row| row.get::<_,String>(0)).optional()?.map(|p|serde_json::from_str(&p).map_err(AbsorbedStoreError::from)).transpose() }
    pub fn insert_task(&self, value: &TaskRecord) -> Result<(), AbsorbedStoreError> { self.put_task(value) }
    pub fn update_task(&self, value: &TaskRecord) -> Result<(), AbsorbedStoreError> { self.put_task(value) }
    pub fn delete_task(&self, task_id: &str) -> Result<bool, AbsorbedStoreError> { let conn=self.db.lock(); ensure_absorbed_schema(&conn)?; Ok(conn.execute("DELETE FROM absorbed_tasks WHERE task_id=?1", [task_id])? != 0) }
    pub fn put_artifact(&self, value: &ArtifactRecord) -> Result<(), AbsorbedStoreError> { validate_artifact(value)?; let payload=json(value)?; let provenance=json(&value.provenance)?; let conn=self.db.lock(); ensure_absorbed_schema(&conn)?; conn.execute("INSERT INTO absorbed_artifacts(artifact_id,session_id,task_id,handle,content_hash,media_type,byte_length,scope_id,authority,influence_class,lifecycle,retention,provenance_json,created_at_ms,updated_at_ms,payload_json) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16) ON CONFLICT(artifact_id) DO UPDATE SET session_id=excluded.session_id,task_id=excluded.task_id,handle=excluded.handle,content_hash=excluded.content_hash,media_type=excluded.media_type,byte_length=excluded.byte_length,scope_id=excluded.scope_id,authority=excluded.authority,influence_class=excluded.influence_class,lifecycle=excluded.lifecycle,retention=excluded.retention,provenance_json=excluded.provenance_json,created_at_ms=excluded.created_at_ms,updated_at_ms=excluded.updated_at_ms,payload_json=excluded.payload_json", params![value.artifact_id,value.session_id,value.task_id,value.handle,value.content_hash,value.media_type,value.byte_length as i64,value.scope_id,value.authority,value.influence_class,value.lifecycle,value.retention,provenance,value.created_at_ms as i64,value.updated_at_ms as i64,payload])?; Ok(()) }
    pub fn get_artifact(&self, artifact_id: &str) -> Result<Option<ArtifactRecord>, AbsorbedStoreError> { let conn=self.db.lock(); ensure_absorbed_schema(&conn)?; conn.query_row("SELECT payload_json FROM absorbed_artifacts WHERE artifact_id=?1", [artifact_id], |row| row.get::<_,String>(0)).optional()?.map(|p|serde_json::from_str(&p).map_err(AbsorbedStoreError::from)).transpose() }
    pub fn insert_artifact(&self, value: &ArtifactRecord) -> Result<(), AbsorbedStoreError> { self.put_artifact(value) }
    pub fn update_artifact(&self, value: &ArtifactRecord) -> Result<(), AbsorbedStoreError> { self.put_artifact(value) }
    pub fn delete_artifact(&self, artifact_id: &str) -> Result<bool, AbsorbedStoreError> { let conn=self.db.lock(); ensure_absorbed_schema(&conn)?; Ok(conn.execute("DELETE FROM absorbed_artifacts WHERE artifact_id=?1", [artifact_id])? != 0) }
}

fn cursor_in_tx(tx: &Transaction<'_>, session_id: &str) -> Result<u64, AbsorbedStoreError> {
    Ok(tx.query_row("SELECT last_seq FROM absorbed_event_cursors WHERE session_id=?1", [session_id], |row| row.get::<_, i64>(0)).optional()?.unwrap_or(0).max(0) as u64)
}
