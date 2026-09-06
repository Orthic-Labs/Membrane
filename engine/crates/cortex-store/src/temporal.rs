//! Cortex temporal validity and supersession.
//!
//! The store keeps the historical `TemporalFact` API for existing callers, but
//! both that API and the durable `memories` lifecycle columns are normalized to
//! [`TemporalValidityV1`].  Normalization is deliberately in-place: the old
//! columns remain as compatibility aliases and no record is copied to (or
//! removed from) a second temporal table.

use crate::MemDb;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub const TEMPORAL_VALIDITY_SCHEMA_VERSION: u32 = 1;
const UNKNOWN_AUTHORED_TIME: &str = "authored_time_unavailable";
const UNKNOWN_RECORDED_TIME: &str = "recorded_time_unavailable";

/// A timestamp whose absence is explicit and typed.  In particular,
/// `Unavailable` is never filled with the ingest timestamp.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", tag = "status")]
pub enum TemporalInstantV1 {
    Known { value: String },
    Unavailable { reason: String },
}

impl TemporalInstantV1 {
    pub fn known(value: impl Into<String>) -> Self {
        Self::Known {
            value: value.into(),
        }
    }

    pub fn unavailable(reason: impl Into<String>) -> Self {
        Self::Unavailable {
            reason: reason.into(),
        }
    }

    fn value(&self) -> Option<&str> {
        match self {
            Self::Known { value } => Some(value),
            Self::Unavailable { .. } => None,
        }
    }

    fn reason(&self, default: &'static str) -> Option<&str> {
        match self {
            Self::Known { .. } => None,
            Self::Unavailable { reason } if !reason.trim().is_empty() => Some(reason),
            Self::Unavailable { .. } => Some(default),
        }
    }
}

/// Unified temporal vocabulary for both durable-memory and temporal-fact rows.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TemporalValidityV1 {
    pub record_id: String,
    pub subject: String,
    pub predicate: String,
    pub object: serde_json::Value,
    pub scope_id: String,
    pub authority: String,
    pub valid_at: TemporalInstantV1,
    pub recorded_at: TemporalInstantV1,
    pub invalid_at: Option<String>,
    pub superseded_by: Option<String>,
    pub revoked: bool,
    pub independently_verified: bool,
    pub expires_at: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TemporalTransitionV1 {
    pub from_record_id: String,
    pub to_record_id: String,
    pub invalid_at: String,
    pub transition_sha256: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TemporalValidityReceiptV1 {
    pub schema_version: u32,
    pub record_id: String,
    pub payload_sha256: String,
    pub transitions: Vec<TemporalTransitionV1>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TemporalConflictV1 {
    pub reason: String,
    pub record_ids: Vec<String>,
}

/// A resolved read or an explicit conflict.  A conflict is not represented as
/// several records in `records`, which prevents callers from accidentally
/// blending incompatible values.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TemporalQueryOutcomeV1 {
    pub records: Vec<TemporalValidityV1>,
    pub conflict: Option<TemporalConflictV1>,
}

impl TemporalQueryOutcomeV1 {
    fn resolved(record: TemporalValidityV1) -> Self {
        Self {
            records: vec![record],
            conflict: None,
        }
    }

    fn conflict(record_ids: Vec<String>) -> Self {
        Self {
            records: Vec::new(),
            conflict: Some(TemporalConflictV1 {
                reason: "unresolved_temporal_conflict".into(),
                record_ids,
            }),
        }
    }
}

/// Compatibility shape used by the existing MCP temporal-fact operation.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TemporalFact {
    pub fact_id: String,
    pub subject: String,
    pub predicate: String,
    pub object: serde_json::Value,
    pub scope_id: String,
    pub authority: String,
    pub veracity: String,
    pub observed_at: String,
    pub valid_from: String,
    pub valid_until: Option<String>,
    pub expires_at: Option<String>,
    pub supersedes: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TemporalTransition {
    pub from_fact_id: String,
    pub to_fact_id: String,
    pub effective_at: String,
    pub transition_sha256: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TemporalFactReceipt {
    pub fact_id: String,
    pub payload_sha256: String,
    pub transitions: Vec<TemporalTransition>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TemporalFactQuery {
    pub scope_chain: Vec<String>,
    pub subject: String,
    pub predicate: String,
    pub as_of: String,
}

fn valid_instant(value: &str) -> bool {
    let b = value.as_bytes();
    if b.len() < 20
        || b[4] != b'-'
        || b[7] != b'-'
        || b[10] != b'T'
        || b[13] != b':'
        || b[16] != b':'
        || !b.ends_with(b"Z")
    {
        return false;
    }
    for (i, c) in b.iter().enumerate() {
        if matches!(i, 4 | 7 | 10 | 13 | 16) || i == b.len() - 1 {
            continue;
        }
        if i == 19 && *c == b'.' {
            continue;
        }
        if !c.is_ascii_digit() {
            return false;
        }
    }
    true
}

fn digest<T: Serialize>(value: &T) -> Result<String, String> {
    let bytes = serde_json::to_vec(value).map_err(|e| e.to_string())?;
    Ok(format!("sha256:{}", hex::encode(Sha256::digest(bytes))))
}

fn authority_rank(value: &str) -> u8 {
    value
        .strip_prefix('A')
        .and_then(|n| n.parse::<u8>().ok())
        .unwrap_or(0)
}

fn timestamp_from_ms(value: i64) -> String {
    let seconds = value.div_euclid(1_000);
    let millis = value.rem_euclid(1_000);
    let days = seconds.div_euclid(86_400);
    let rem = seconds.rem_euclid(86_400);
    let (hour, minute, second) = (rem / 3_600, (rem % 3_600) / 60, rem % 60);
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { y + 1 } else { y };
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis:03}Z")
}

fn ensure_column(conn: &rusqlite::Connection, table: &str, name: &str, definition: &str) -> Result<(), String> {
    let exists: bool = conn
        .query_row(
            &format!("SELECT COUNT(*) > 0 FROM pragma_table_info('{table}') WHERE name=?1"),
            [name],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    if !exists {
        conn.execute(
            &format!("ALTER TABLE {table} ADD COLUMN {name} {definition}"),
            [],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Add the canonical columns and migrate both legacy representations in place.
/// This is intentionally called by every temporal operation, since `MemDb`'s
/// schema migrator is outside this lane's allowed ownership boundary.
fn ensure_temporal_schema(conn: &rusqlite::Connection) -> Result<(), String> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS membrane_temporal_fact (
            fact_id TEXT PRIMARY KEY, subject TEXT NOT NULL, predicate TEXT NOT NULL,
            object_json TEXT NOT NULL, scope_id TEXT NOT NULL, authority TEXT NOT NULL,
            veracity TEXT NOT NULL, observed_at TEXT NOT NULL, valid_from TEXT NOT NULL,
            valid_until TEXT, expires_at TEXT, supersedes TEXT,
            payload_sha256 TEXT NOT NULL, transition_sha256 TEXT NOT NULL
        )",
    )
    .map_err(|e| e.to_string())?;

    for (name, definition) in [
        ("valid_at", "TEXT"),
        ("valid_at_unavailable_reason", "TEXT"),
        ("recorded_at", "TEXT"),
        ("recorded_at_unavailable_reason", "TEXT"),
        ("invalid_at", "TEXT"),
        ("superseded_by", "TEXT"),
        ("revoked", "INTEGER NOT NULL DEFAULT 0"),
        ("independently_verified", "INTEGER NOT NULL DEFAULT 0"),
    ] {
        ensure_column(conn, "membrane_temporal_fact", name, definition)?;
    }

    // The memories table already owns these lifecycle values.  The canonical
    // columns are added beside them so old rows and old writers remain valid.
    for (name, definition) in [
        ("valid_at", "TEXT"),
        ("valid_at_unavailable_reason", "TEXT"),
        ("recorded_at", "TEXT"),
        ("recorded_at_unavailable_reason", "TEXT"),
        ("invalid_at", "TEXT"),
        ("superseded_by", "TEXT"),
        ("revoked", "INTEGER NOT NULL DEFAULT 0"),
        ("independently_verified", "INTEGER NOT NULL DEFAULT 0"),
    ] {
        ensure_column(conn, "memories", name, definition)?;
    }
    let has_quarantine: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='memory_quarantine'",
            [],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    if has_quarantine {
        for (name, definition) in [
            ("valid_at", "TEXT"),
            ("valid_at_unavailable_reason", "TEXT"),
            ("recorded_at", "TEXT"),
            ("recorded_at_unavailable_reason", "TEXT"),
            ("invalid_at", "TEXT"),
            ("superseded_by", "TEXT"),
            ("revoked", "INTEGER NOT NULL DEFAULT 0"),
            ("independently_verified", "INTEGER NOT NULL DEFAULT 0"),
        ] {
            ensure_column(conn, "memory_quarantine", name, definition)?;
        }
    }

    migrate_temporal_fact_rows(conn)?;
    migrate_memory_rows(conn, "memories")?;
    if has_quarantine {
        migrate_memory_rows(conn, "memory_quarantine")?;
    }
    Ok(())
}

fn migrate_temporal_fact_rows(conn: &rusqlite::Connection) -> Result<(), String> {
    conn.execute(
        "UPDATE membrane_temporal_fact SET valid_at=valid_from
         WHERE valid_at IS NULL AND valid_from IS NOT NULL AND valid_from <> ''",
        [],
    )
    .map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE membrane_temporal_fact SET recorded_at=observed_at
         WHERE recorded_at IS NULL AND observed_at IS NOT NULL AND observed_at <> ''",
        [],
    )
    .map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE membrane_temporal_fact SET invalid_at=valid_until
         WHERE invalid_at IS NULL AND valid_until IS NOT NULL",
        [],
    )
    .map_err(|e| e.to_string())?;

    let rows: Vec<(String, Option<String>, String)> = conn
        .prepare("SELECT fact_id,supersedes,valid_from FROM membrane_temporal_fact")
        .map_err(|e| e.to_string())?
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .map_err(|e| e.to_string())?
        .collect::<rusqlite::Result<_>>()
        .map_err(|e| e.to_string())?;
    let times: BTreeMap<String, String> = rows
        .iter()
        .map(|(id, _, valid_from)| (id.clone(), valid_from.clone()))
        .collect();
    for (id, supersedes, valid_from) in rows {
        let successor = supersedes.and_then(|value| {
            value.split(',').map(str::trim).find_map(|candidate| {
                times
                    .get(candidate)
                    .filter(|candidate_time| *candidate_time > &valid_from)
                    .map(|_| candidate.to_string())
            })
        });
        if let Some(successor) = successor {
            conn.execute(
                "UPDATE membrane_temporal_fact SET superseded_by=?1
                 WHERE fact_id=?2 AND superseded_by IS NULL",
                rusqlite::params![successor, id],
            )
            .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn migrate_memory_rows(conn: &rusqlite::Connection, table: &str) -> Result<(), String> {
    let sql = format!(
        "SELECT id,created_at,effective_from_ms,effective_until_ms FROM {table}"
    );
    let rows: Vec<(String, String, Option<i64>, Option<i64>)> = conn
        .prepare(&sql)
        .map_err(|e| e.to_string())?
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)))
        .map_err(|e| e.to_string())?
        .collect::<rusqlite::Result<_>>()
        .map_err(|e| e.to_string())?;
    for (id, created_at, valid_at, invalid_at) in rows {
        let valid_at = valid_at.map(timestamp_from_ms);
        let invalid_at = invalid_at.map(timestamp_from_ms);
        conn.execute(
            &format!(
                "UPDATE {table} SET valid_at=COALESCE(valid_at,?1),
                 invalid_at=COALESCE(invalid_at,?2), recorded_at=COALESCE(recorded_at,?3)
                 WHERE id=?4"
            ),
            rusqlite::params![valid_at, invalid_at, created_at, id],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

impl TemporalValidityV1 {
    fn validate(&self) -> Result<(), String> {
        if self.record_id.trim().is_empty()
            || self.subject.trim().is_empty()
            || self.predicate.trim().is_empty()
            || self.scope_id.trim().is_empty()
        {
            return Err("temporal_validity_identity_invalid".into());
        }
        for instant in [&self.valid_at, &self.recorded_at] {
            if let Some(value) = instant.value() {
                if !valid_instant(value) {
                    return Err("temporal_validity_timestamp_invalid".into());
                }
            } else if instant.reason(UNKNOWN_AUTHORED_TIME).is_none() {
                return Err("temporal_validity_unavailable_reason_missing".into());
            }
        }
        if self
            .invalid_at
            .as_deref()
            .is_some_and(|value| !valid_instant(value))
            || self
                .expires_at
                .as_deref()
                .is_some_and(|value| !valid_instant(value))
        {
            return Err("temporal_validity_timestamp_invalid".into());
        }
        if let (Some(valid_at), Some(invalid_at)) = (self.valid_at.value(), self.invalid_at.as_deref())
        {
            if invalid_at <= valid_at {
                return Err("temporal_validity_interval_invalid".into());
            }
        }
        if !matches!(self.authority.as_str(), "A0" | "A1" | "A2" | "A3" | "A4" | "A5") {
            return Err("temporal_validity_authority_invalid".into());
        }
        Ok(())
    }
}

pub struct TemporalFactStore {
    pub(crate) db: MemDb,
}

impl TemporalFactStore {
    pub fn new(db: MemDb) -> Self {
        // Construction is the normal lifecycle boundary for this store.  Keep
        // the existing infallible API, while operations below still retry and
        // surface any migration error instead of hiding it.
        {
            let conn = db.lock();
            let _ = ensure_temporal_schema(&conn);
        }
        Self { db }
    }

    /// Admit a record using the unified V1 vocabulary.  A single-valued key
    /// supersedes open predecessors; it never deletes them.
    pub fn record_validity(
        &self,
        record: TemporalValidityV1,
        single_valued: bool,
    ) -> Result<TemporalValidityReceiptV1, String> {
        record.validate()?;
        let mut conn = self.db.lock();
        ensure_temporal_schema(&conn)?;
        let tx = conn
            .unchecked_transaction()
            .map_err(|e| e.to_string())?;
        let payload_sha256 = digest(&record)?;

        if let Some(existing) = tx
            .query_row(
                "SELECT payload_sha256 FROM membrane_temporal_fact WHERE fact_id=?1",
                [&record.record_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|e| e.to_string())?
        {
            if existing != payload_sha256 {
                return Err("temporal_validity_conflict".into());
            }
            return Ok(TemporalValidityReceiptV1 {
                schema_version: TEMPORAL_VALIDITY_SCHEMA_VERSION,
                record_id: record.record_id,
                payload_sha256,
                transitions: Vec::new(),
            });
        }

        let valid_at = record.valid_at.value().map(str::to_owned);
        let mut transitions = Vec::new();
        if single_valued && valid_at.is_some() {
            let valid_at = valid_at.as_deref().expect("valid_at checked above");
            let mut stmt = tx
                .prepare(
                    "SELECT fact_id,valid_at FROM membrane_temporal_fact
                     WHERE scope_id=?1 AND subject=?2 AND predicate=?3
                       AND revoked=0 AND veracity <> 'revoked'
                       AND superseded_by IS NULL AND invalid_at IS NULL
                       AND valid_at IS NOT NULL ORDER BY valid_at,fact_id",
                )
                .map_err(|e| e.to_string())?;
            let current: Vec<(String, String)> = stmt
                .query_map(
                    rusqlite::params![record.scope_id, record.subject, record.predicate],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .map_err(|e| e.to_string())?
                .collect::<rusqlite::Result<_>>()
                .map_err(|e| e.to_string())?;
            if current
                .iter()
                .any(|(_, old_valid_at)| old_valid_at.as_str() >= valid_at)
            {
                return Err("temporal_validity_conflict".into());
            }
            for (from_record_id, _) in current {
                let transition_sha256 = digest(&(
                    from_record_id.clone(),
                    record.record_id.clone(),
                    valid_at.to_string(),
                ))?;
                tx.execute(
                    "UPDATE membrane_temporal_fact
                     SET invalid_at=?1, superseded_by=?2, valid_until=?1,
                         supersedes=CASE WHEN supersedes IS NULL THEN ?2 ELSE supersedes END,
                         transition_sha256=?3
                     WHERE fact_id=?4",
                    rusqlite::params![valid_at, record.record_id, transition_sha256, from_record_id],
                )
                .map_err(|e| e.to_string())?;
                transitions.push(TemporalTransitionV1 {
                    from_record_id,
                    to_record_id: record.record_id.clone(),
                    invalid_at: valid_at.to_string(),
                    transition_sha256,
                });
            }
        }

        let valid_at_value = record.valid_at.value().map(str::to_owned);
        let recorded_at_value = record.recorded_at.value().map(str::to_owned);
        let valid_at_reason = record
            .valid_at
            .reason(UNKNOWN_AUTHORED_TIME)
            .map(str::to_owned);
        let recorded_at_reason = record
            .recorded_at
            .reason(UNKNOWN_RECORDED_TIME)
            .map(str::to_owned);
        let object_json = serde_json::to_string(&record.object).map_err(|e| e.to_string())?;
        tx.execute(
            "INSERT INTO membrane_temporal_fact
             (fact_id,subject,predicate,object_json,scope_id,authority,veracity,
              observed_at,valid_from,valid_until,expires_at,supersedes,payload_sha256,
              transition_sha256,valid_at,valid_at_unavailable_reason,recorded_at,
              recorded_at_unavailable_reason,invalid_at,superseded_by,revoked,
              independently_verified)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,NULL,?12,'',
                     ?19,?13,?14,?15,?10,?16,?17,?18)",
            rusqlite::params![
                record.record_id,
                record.subject,
                record.predicate,
                object_json,
                record.scope_id,
                record.authority,
                if record.revoked { "revoked" } else { "supported" },
                recorded_at_value.as_deref().unwrap_or(""),
                valid_at_value.as_deref().unwrap_or(""),
                record.invalid_at,
                record.expires_at,
                payload_sha256,
                valid_at_reason,
                recorded_at_value.as_deref(),
                recorded_at_reason,
                record.superseded_by,
                record.revoked as i64,
                record.independently_verified as i64,
                valid_at_value.as_deref(),
            ],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(TemporalValidityReceiptV1 {
            schema_version: TEMPORAL_VALIDITY_SCHEMA_VERSION,
            record_id: record.record_id,
            payload_sha256,
            transitions,
        })
    }

    /// Reconstruct canonical receipt after response loss. Transition rows are
    /// durable predecessors pointing at this record, so replay remains exact.
    pub fn validity_receipt(
        &self,
        record_id: &str,
    ) -> Result<Option<TemporalValidityReceiptV1>, String> {
        let conn = self.db.lock();
        ensure_temporal_schema(&conn)?;
        let Some(payload_sha256) = conn
            .query_row(
                "SELECT payload_sha256 FROM membrane_temporal_fact WHERE fact_id=?1",
                [record_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| error.to_string())?
        else {
            return Ok(None);
        };
        let mut statement = conn
            .prepare(
                "SELECT fact_id,invalid_at,transition_sha256 FROM membrane_temporal_fact
                 WHERE superseded_by=?1 ORDER BY fact_id",
            )
            .map_err(|error| error.to_string())?;
        let transitions = statement
            .query_map([record_id], |row| {
                Ok(TemporalTransitionV1 {
                    from_record_id: row.get(0)?,
                    to_record_id: record_id.to_owned(),
                    invalid_at: row.get(1)?,
                    transition_sha256: row.get(2)?,
                })
            })
            .map_err(|error| error.to_string())?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|error| error.to_string())?;
        Ok(Some(TemporalValidityReceiptV1 {
            schema_version: TEMPORAL_VALIDITY_SCHEMA_VERSION,
            record_id: record_id.to_owned(),
            payload_sha256,
            transitions,
        }))
    }

    /// Alias named for the Cortex admission boundary.
    pub fn admit(
        &self,
        record: TemporalValidityV1,
        single_valued: bool,
    ) -> Result<TemporalValidityReceiptV1, String> {
        self.record_validity(record, single_valued)
    }

    /// Read with deterministic conflict ordering.  The returned `Err` is a
    /// typed compatibility error; callers needing structured detail should use
    /// [`query_validity`].
    pub fn query_validity(
        &self,
        scope_chain: Vec<String>,
        subject: String,
        predicate: String,
        as_of: String,
    ) -> Result<TemporalQueryOutcomeV1, String> {
        if scope_chain.is_empty()
            || subject.trim().is_empty()
            || predicate.trim().is_empty()
            || !valid_instant(&as_of)
        {
            return Err("temporal_validity_query_invalid".into());
        }
        let mut conn = self.db.lock();
        ensure_temporal_schema(&conn)?;
        for scope in scope_chain {
            let candidates = load_candidates(&conn, &scope, &subject, &predicate)?;
            if candidates.is_empty() {
                continue;
            }
            let outcome = resolve_candidates(candidates, &as_of);
            if outcome.conflict.is_some() {
                return Ok(outcome);
            }
            if !outcome.records.is_empty() {
                return Ok(outcome);
            }
        }
        Ok(TemporalQueryOutcomeV1 {
            records: Vec::new(),
            conflict: None,
        })
    }

    /// Short alias for callers that already have an as-of query boundary.
    pub fn query_as_of(
        &self,
        scope_chain: Vec<String>,
        subject: String,
        predicate: String,
        as_of: String,
    ) -> Result<TemporalQueryOutcomeV1, String> {
        self.query_validity(scope_chain, subject, predicate, as_of)
    }

    /// Read a durable `memories` row through the same canonical vocabulary.
    /// This is the compatibility read for rows that predate TemporalValidityV1.
    pub fn memory_validity(&self, record_id: &str) -> Result<Option<TemporalValidityV1>, String> {
        if record_id.trim().is_empty() {
            return Err("temporal_validity_identity_invalid".into());
        }
        let conn = self.db.lock();
        ensure_temporal_schema(&conn)?;
        load_memory_record(&conn, record_id)
    }

    /// Return the complete predecessor/successor chain for one temporal fact.
    pub fn lineage(&self, record_id: &str) -> Result<Vec<TemporalValidityV1>, String> {
        if record_id.trim().is_empty() {
            return Err("temporal_validity_identity_invalid".into());
        }
        let mut conn = self.db.lock();
        ensure_temporal_schema(&conn)?;
        if load_record(&conn, record_id)?.is_none() {
            return memory_lineage(&conn, record_id);
        }
        let mut current = record_id.to_string();
        let mut chain = Vec::new();
        while let Some(record) = load_record(&conn, &current)? {
            current = match conn
                .query_row(
                    "SELECT fact_id FROM membrane_temporal_fact WHERE superseded_by=?1
                     ORDER BY valid_at,fact_id LIMIT 1",
                    [&current],
                    |row| row.get::<_, String>(0),
                )
                .optional()
                .map_err(|e| e.to_string())?
            {
                Some(predecessor) => predecessor,
                None => String::new(),
            };
            chain.push(record);
            if current.is_empty() || chain.len() > 10_000 {
                break;
            }
        }
        chain.reverse();
        let mut successor = chain.last().and_then(|r| r.superseded_by.clone());
        while let Some(id) = successor {
            let Some(record) = load_record(&conn, &id)? else { break };
            successor = record.superseded_by.clone();
            chain.push(record);
            if chain.len() > 10_000 {
                break;
            }
        }
        Ok(chain)
    }

    /// Existing temporal-fact write boundary, retained as an adapter over the
    /// unified storage columns.
    pub fn record(
        &self,
        fact: TemporalFact,
        single_valued: bool,
    ) -> Result<TemporalFactReceipt, String> {
        validate_fact(&fact)?;
        let mut conn = self.db.lock();
        ensure_temporal_schema(&conn)?;
        let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
        let mut fact = fact;
        if let Some((existing, stored_supersedes)) = tx
            .query_row(
                "SELECT payload_sha256,supersedes FROM membrane_temporal_fact WHERE fact_id=?1",
                [&fact.fact_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
            )
            .optional()
            .map_err(|e| e.to_string())?
        {
            fact.supersedes = stored_supersedes;
            if existing != digest(&fact)? {
                return Err("temporal_fact_conflict".into());
            }
            return Ok(TemporalFactReceipt {
                fact_id: fact.fact_id,
                payload_sha256: existing,
                transitions: Vec::new(),
            });
        }
        let mut transitions = Vec::new();
        if single_valued {
            let current: Vec<(String, String)> = tx
                .prepare(
                    "SELECT fact_id,valid_from FROM membrane_temporal_fact
                     WHERE scope_id=?1 AND subject=?2 AND predicate=?3
                       AND veracity='supported' AND valid_until IS NULL
                     ORDER BY valid_from,fact_id",
                )
                .map_err(|e| e.to_string())?
                .query_map(
                    rusqlite::params![fact.scope_id, fact.subject, fact.predicate],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .map_err(|e| e.to_string())?
                .collect::<rusqlite::Result<_>>()
                .map_err(|e| e.to_string())?;
            if !current.is_empty() && fact.supersedes.is_none() {
                fact.supersedes = Some(
                    current
                        .iter()
                        .map(|(id, _)| id.clone())
                        .collect::<Vec<_>>()
                        .join(","),
                );
            }
            if current.iter().any(|(_, from)| from >= &fact.valid_from) {
                return Err("temporal_fact_conflict".into());
            }
            for (from_id, _) in current {
                let transition_sha256 = digest(&(
                    from_id.clone(),
                    fact.fact_id.clone(),
                    fact.valid_from.clone(),
                ))?;
                tx.execute(
                    "UPDATE membrane_temporal_fact SET valid_until=?1,invalid_at=?1,
                     superseded_by=?2,supersedes=CASE WHEN supersedes IS NULL THEN ?2 ELSE supersedes END,
                     transition_sha256=?3 WHERE fact_id=?4",
                    rusqlite::params![fact.valid_from, fact.fact_id, transition_sha256, from_id],
                )
                .map_err(|e| e.to_string())?;
                transitions.push(TemporalTransition {
                    from_fact_id: from_id,
                    to_fact_id: fact.fact_id.clone(),
                    effective_at: fact.valid_from.clone(),
                    transition_sha256,
                });
            }
        }
        let payload_sha256 = digest(&fact)?;
        tx.execute(
            "INSERT INTO membrane_temporal_fact
             (fact_id,subject,predicate,object_json,scope_id,authority,veracity,observed_at,
              valid_from,valid_until,expires_at,supersedes,payload_sha256,transition_sha256,
              valid_at,recorded_at,invalid_at,superseded_by,revoked,independently_verified)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,'',?9,?8,?10,NULL,?14,?15)",
            rusqlite::params![
                fact.fact_id,
                fact.subject,
                fact.predicate,
                serde_json::to_string(&fact.object).map_err(|e| e.to_string())?,
                fact.scope_id,
                fact.authority,
                fact.veracity,
                fact.observed_at,
                fact.valid_from,
                fact.valid_until,
                fact.expires_at,
                fact.supersedes,
                payload_sha256,
                (fact.veracity == "revoked") as i64,
                (fact.veracity == "independently_verified") as i64,
            ],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(TemporalFactReceipt {
            fact_id: fact.fact_id,
            payload_sha256,
            transitions,
        })
    }

    /// Compatibility query.  It uses the V1 resolver and therefore refuses to
    /// return an unresolved conflicting blend.
    pub fn query(&self, query: TemporalFactQuery) -> Result<Vec<TemporalFact>, String> {
        let outcome = self
            .query_validity(
                query.scope_chain,
                query.subject,
                query.predicate,
                query.as_of,
            )
            .map_err(|error| {
                if error == "temporal_validity_query_invalid" {
                    "temporal_fact_query_invalid".to_string()
                } else {
                    error
                }
            })?;
        if let Some(conflict) = outcome.conflict {
            return Err(format!("temporal_conflict_unresolved:{}", conflict.record_ids.join(",")));
        }
        // CTX-009 keeps observed, valid, recorded and expiry distinct.
        // `TemporalValidityV1` is the *validity* view and carries only valid /
        // recorded, so `TemporalFact::try_from` has to fall back to recorded
        // time for `observed_at`. Since recorded time became the admission
        // clock (rather than a copy of observed time), that fallback would
        // report the admission instant as the observation instant on this
        // compatibility surface. Re-read the durably stored observation from
        // `membrane_temporal_fact` so the fact-shaped read stays truthful;
        // rows with no fact-table entry (memory-derived validity) keep the
        // recorded-time fallback.
        let conn = self.db.lock();
        outcome
            .records
            .into_iter()
            .map(|record| {
                let observed = observed_at_for(&conn, &record.record_id);
                TemporalFact::try_from(record).map(|mut fact| {
                    if let Some(observed) = observed {
                        fact.observed_at = observed;
                    }
                    fact
                })
            })
            .collect()
    }
}

/// Durably stored observation instant for a fact id, if the fact table has one.
fn observed_at_for(conn: &rusqlite::Connection, fact_id: &str) -> Option<String> {
    conn.query_row(
        "SELECT observed_at FROM membrane_temporal_fact WHERE fact_id=?1",
        rusqlite::params![fact_id],
        |row| row.get::<_, String>(0),
    )
    .optional()
    .ok()
    .flatten()
    .filter(|value| !value.trim().is_empty())
}

fn load_candidates(
    conn: &rusqlite::Connection,
    scope: &str,
    subject: &str,
    predicate: &str,
) -> Result<Vec<TemporalValidityV1>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT fact_id,subject,predicate,object_json,scope_id,authority,veracity,
                    valid_at,valid_at_unavailable_reason,recorded_at,
                    recorded_at_unavailable_reason,invalid_at,superseded_by,expires_at,
                    revoked,independently_verified
             FROM membrane_temporal_fact
             WHERE scope_id=?1 AND subject=?2 AND predicate=?3
             ORDER BY fact_id",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(rusqlite::params![scope, subject, predicate], |row| {
            let veracity: String = row.get(6)?;
            let valid_at: Option<String> = row.get(7)?;
            let valid_at = valid_at.filter(|value| !value.is_empty());
            let valid_at_reason: Option<String> = row.get(8)?;
            let recorded_at: Option<String> = row.get(9)?;
            let recorded_at = recorded_at.filter(|value| !value.is_empty());
            let recorded_at_reason: Option<String> = row.get(10)?;
            Ok(TemporalValidityV1 {
                record_id: row.get(0)?,
                subject: row.get(1)?,
                predicate: row.get(2)?,
                object: serde_json::from_str(&row.get::<_, String>(3)?).unwrap_or(serde_json::Value::Null),
                scope_id: row.get(4)?,
                authority: row.get(5)?,
                valid_at: valid_at
                    .map(|value| TemporalInstantV1::known(value))
                    .unwrap_or_else(|| TemporalInstantV1::unavailable(valid_at_reason.unwrap_or_else(|| UNKNOWN_AUTHORED_TIME.into()))),
                recorded_at: recorded_at
                    .map(|value| TemporalInstantV1::known(value))
                    .unwrap_or_else(|| TemporalInstantV1::unavailable(recorded_at_reason.unwrap_or_else(|| UNKNOWN_RECORDED_TIME.into()))),
                invalid_at: row.get(11)?,
                superseded_by: row.get(12)?,
                revoked: row.get::<_, i64>(14)? != 0 || veracity == "revoked",
                independently_verified: row.get::<_, i64>(15)? != 0 || veracity == "independently_verified",
                expires_at: row.get(13)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<rusqlite::Result<_>>().map_err(|e| e.to_string())
}

fn valid_at_requested_time(record: &TemporalValidityV1, as_of: &str) -> bool {
    let Some(valid_at) = record.valid_at.value() else { return false };
    valid_at <= as_of
        && record
            .invalid_at
            .as_deref()
            .map_or(true, |invalid_at| as_of < invalid_at)
        && record
            .expires_at
            .as_deref()
            .map_or(true, |expires_at| as_of < expires_at)
}

fn resolve_candidates(mut candidates: Vec<TemporalValidityV1>, as_of: &str) -> TemporalQueryOutcomeV1 {
    // The stages are intentionally separate.  In particular, authority is not
    // allowed to rescue a record that is not valid at the requested instant.
    candidates.retain(|record| !record.revoked);
    if candidates.is_empty() {
        return TemporalQueryOutcomeV1 {
            records: Vec::new(),
            conflict: None,
        };
    }
    let known_valid_times = candidates.iter().any(|record| record.valid_at.value().is_some());
    let valid_now: Vec<_> = candidates
        .iter()
        .filter(|record| valid_at_requested_time(record, as_of))
        .cloned()
        .collect();
    if !valid_now.is_empty() {
        candidates = valid_now;
    } else if known_valid_times {
        // A future or already-invalid known record is not a valid fallback for
        // an as-of read.  Unknown authored time remains representable only when
        // there is no known temporal candidate at all.
        return TemporalQueryOutcomeV1 {
            records: Vec::new(),
            conflict: None,
        };
    }
    let highest_authority = candidates
        .iter()
        .map(|record| authority_rank(&record.authority))
        .max()
        .unwrap_or(0);
    candidates.retain(|record| authority_rank(&record.authority) == highest_authority);
    let independently_verified = candidates
        .iter()
        .any(|record| record.independently_verified);
    if independently_verified {
        candidates.retain(|record| record.independently_verified);
    }

    let first_object = candidates.first().map(|record| &record.object);
    if candidates
        .iter()
        .any(|record| Some(&record.object) != first_object)
    {
        candidates.sort_by(|left, right| left.record_id.cmp(&right.record_id));
        return TemporalQueryOutcomeV1::conflict(
            candidates.into_iter().map(|record| record.record_id).collect(),
        );
    }
    candidates.sort_by(|left, right| left.record_id.cmp(&right.record_id));
    TemporalQueryOutcomeV1::resolved(candidates.remove(0))
}

fn load_memory_record(
    conn: &rusqlite::Connection,
    record_id: &str,
) -> Result<Option<TemporalValidityV1>, String> {
    let row = conn
        .query_row(
            "SELECT id,content,scope_id,authority,valid_at,valid_at_unavailable_reason,
                    recorded_at,recorded_at_unavailable_reason,invalid_at,superseded_by,
                    expires_at_ms,revoked,independently_verified,lifecycle_state
             FROM memories WHERE id=?1",
            [record_id],
            |row| {
                let lifecycle: String = row.get(13)?;
                let valid_at: Option<String> = row.get(4)?;
                let valid_at_reason: Option<String> = row.get(5)?;
                let recorded_at: Option<String> = row.get(6)?;
                let recorded_at_reason: Option<String> = row.get(7)?;
                Ok(TemporalValidityV1 {
                    record_id: row.get(0)?,
                    subject: format!("memory:{}", record_id),
                    predicate: "content".into(),
                    object: serde_json::Value::String(row.get(1)?),
                    scope_id: row.get(2)?,
                    authority: row.get(3)?,
                    valid_at: valid_at
                        .filter(|value| !value.is_empty())
                        .map(|value| TemporalInstantV1::known(value))
                        .unwrap_or_else(|| TemporalInstantV1::unavailable(valid_at_reason.unwrap_or_else(|| UNKNOWN_AUTHORED_TIME.into()))),
                    recorded_at: recorded_at
                        .filter(|value| !value.is_empty())
                        .map(|value| TemporalInstantV1::known(value))
                        .unwrap_or_else(|| TemporalInstantV1::unavailable(recorded_at_reason.unwrap_or_else(|| UNKNOWN_RECORDED_TIME.into()))),
                    invalid_at: row.get(8)?,
                    superseded_by: row.get(9)?,
                    expires_at: row.get::<_, Option<i64>>(10)?.map(timestamp_from_ms),
                    revoked: row.get::<_, i64>(11)? != 0 || lifecycle == "revoked",
                    independently_verified: row.get::<_, i64>(12)? != 0,
                })
            },
        )
        .optional()
        .map_err(|e| e.to_string())?;
    Ok(row)
}

fn memory_lineage(
    conn: &rusqlite::Connection,
    record_id: &str,
) -> Result<Vec<TemporalValidityV1>, String> {
    let mut current = record_id.to_string();
    let mut chain = Vec::new();
    while let Some(record) = load_memory_record(conn, &current)? {
        current = conn
            .query_row(
                "SELECT id FROM memories WHERE superseded_by=?1
                 ORDER BY valid_at,id LIMIT 1",
                [&current],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|e| e.to_string())?
            .unwrap_or_default();
        chain.push(record);
        if current.is_empty() || chain.len() > 10_000 {
            break;
        }
    }
    chain.reverse();
    let mut successor = chain.last().and_then(|record| record.superseded_by.clone());
    while let Some(id) = successor {
        let Some(record) = load_memory_record(conn, &id)? else { break };
        successor = record.superseded_by.clone();
        chain.push(record);
        if chain.len() > 10_000 {
            break;
        }
    }
    Ok(chain)
}

fn load_record(conn: &rusqlite::Connection, record_id: &str) -> Result<Option<TemporalValidityV1>, String> {
    let row = conn
        .query_row(
            "SELECT subject,predicate,scope_id,object_json,authority,veracity,valid_at,
                    valid_at_unavailable_reason,recorded_at,recorded_at_unavailable_reason,
                    invalid_at,superseded_by,expires_at,revoked,independently_verified
             FROM membrane_temporal_fact WHERE fact_id=?1",
            [record_id],
            |row| {
                let veracity: String = row.get(5)?;
                let valid_at: Option<String> = row.get(6)?;
                let valid_at = valid_at.filter(|value| !value.is_empty());
                let valid_at_reason: Option<String> = row.get(7)?;
                let recorded_at: Option<String> = row.get(8)?;
                let recorded_at = recorded_at.filter(|value| !value.is_empty());
                let recorded_at_reason: Option<String> = row.get(9)?;
                Ok(TemporalValidityV1 {
                    record_id: record_id.to_string(),
                    subject: row.get(0)?,
                    predicate: row.get(1)?,
                    scope_id: row.get(2)?,
                    object: serde_json::from_str(&row.get::<_, String>(3)?).unwrap_or(serde_json::Value::Null),
                    authority: row.get(4)?,
                    valid_at: valid_at
                        .map(|value| TemporalInstantV1::known(value))
                        .unwrap_or_else(|| TemporalInstantV1::unavailable(valid_at_reason.unwrap_or_else(|| UNKNOWN_AUTHORED_TIME.into()))),
                    recorded_at: recorded_at
                        .map(|value| TemporalInstantV1::known(value))
                        .unwrap_or_else(|| TemporalInstantV1::unavailable(recorded_at_reason.unwrap_or_else(|| UNKNOWN_RECORDED_TIME.into()))),
                    invalid_at: row.get(10)?,
                    superseded_by: row.get(11)?,
                    expires_at: row.get(12)?,
                    revoked: row.get::<_, i64>(13)? != 0 || veracity == "revoked",
                    independently_verified: row.get::<_, i64>(14)? != 0 || veracity == "independently_verified",
                })
            },
        )
        .optional()
        .map_err(|e| e.to_string())?;
    Ok(row)
}

impl TryFrom<TemporalValidityV1> for TemporalFact {
    type Error = String;

    fn try_from(record: TemporalValidityV1) -> Result<Self, Self::Error> {
        let valid_from = record
            .valid_at
            .value()
            .ok_or_else(|| "temporal_fact_valid_at_unavailable".to_string())?;
        let observed_at = record
            .recorded_at
            .value()
            .ok_or_else(|| "temporal_fact_recorded_at_unavailable".to_string())?;
        Ok(Self {
            fact_id: record.record_id,
            subject: record.subject,
            predicate: record.predicate,
            object: record.object,
            scope_id: record.scope_id,
            authority: record.authority,
            veracity: if record.revoked { "revoked".into() } else { "supported".into() },
            observed_at: observed_at.into(),
            valid_from: valid_from.into(),
            valid_until: record.invalid_at,
            expires_at: record.expires_at,
            supersedes: record.superseded_by,
        })
    }
}

fn validate_fact(f: &TemporalFact) -> Result<(), String> {
    if f.fact_id.trim().is_empty()
        || f.subject.trim().is_empty()
        || f.predicate.trim().is_empty()
        || f.scope_id.trim().is_empty()
    {
        return Err("temporal_fact_identity_invalid".into());
    }
    if !valid_instant(&f.observed_at)
        || !valid_instant(&f.valid_from)
        || f.valid_until.as_deref().is_some_and(|v| !valid_instant(v))
        || f.expires_at.as_deref().is_some_and(|v| !valid_instant(v))
    {
        return Err("temporal_fact_timestamp_invalid".into());
    }
    if f.valid_until.as_ref().is_some_and(|v| v <= &f.valid_from) {
        return Err("temporal_fact_interval_invalid".into());
    }
    if !matches!(f.authority.as_str(), "A0" | "A1" | "A2" | "A3" | "A4" | "A5") {
        return Err("temporal_fact_authority_invalid".into());
    }
    Ok(())
}

/// Validate caller temporal payload without mutating canonical state.
pub fn validate_fact_proposal(fact: &TemporalFact) -> Result<(), String> {
    validate_fact(fact)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fact(id: &str, value: &str, from: &str) -> TemporalFact {
        TemporalFact {
            fact_id: id.into(),
            subject: "repo".into(),
            predicate: "owner".into(),
            object: serde_json::json!(value),
            scope_id: "scope-a".into(),
            authority: "A1".into(),
            veracity: "supported".into(),
            observed_at: from.into(),
            valid_from: from.into(),
            valid_until: None,
            expires_at: None,
            supersedes: None,
        }
    }

    #[test]
    fn supersession_and_as_of_are_deterministic() {
        let db = MemDb::open_in_memory();
        let store = TemporalFactStore::new(db);
        store.record(fact("a", "old", "2026-08-01T00:00:00Z"), true).unwrap();
        let receipt = store.record(fact("b", "new", "2026-08-02T00:00:00Z"), true).unwrap();
        assert_eq!(receipt.transitions[0].from_fact_id, "a");
        let old = store.query(TemporalFactQuery { scope_chain: vec!["scope-a".into()], subject: "repo".into(), predicate: "owner".into(), as_of: "2026-08-01T12:00:00Z".into() }).unwrap();
        assert_eq!(old[0].fact_id, "a");
        let new = store.query(TemporalFactQuery { scope_chain: vec!["scope-a".into()], subject: "repo".into(), predicate: "owner".into(), as_of: "2026-08-03T00:00:00Z".into() }).unwrap();
        assert_eq!(new[0].fact_id, "b");
    }

    #[test]
    fn invalid_as_of_and_conflict_rejected() {
        let db = MemDb::open_in_memory();
        let store = TemporalFactStore::new(db);
        let f = fact("a", "old", "2026-08-01T00:00:00Z");
        store.record(f.clone(), false).unwrap();
        assert_eq!(store.record(fact("a", "other", "2026-08-01T00:00:00Z"), false).unwrap_err(), "temporal_fact_conflict");
        assert_eq!(store.query(TemporalFactQuery { scope_chain: vec!["scope-a".into()], subject: "repo".into(), predicate: "owner".into(), as_of: "tomorrow".into() }).unwrap_err(), "temporal_fact_query_invalid");
    }
}
