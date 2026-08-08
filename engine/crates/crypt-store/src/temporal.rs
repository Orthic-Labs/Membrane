//! Durable temporal facts. Rows are immutable facts; supersession is recorded as a
//! content-free transition hash and closes the predecessor's validity interval.

use crate::MemDb;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

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

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TemporalTransition {
    pub from_fact_id: String,
    pub to_fact_id: String,
    pub effective_at: String,
    pub transition_sha256: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
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
    {
        return false;
    }
    if !b.ends_with(b"Z") {
        return false;
    }
    for (i, c) in b.iter().enumerate() {
        if matches!(i, 4 | 7 | 10 | 13 | 16) || (i == b.len() - 1) {
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
    Ok(format!("sha256:{:x}", Sha256::digest(bytes)))
}

impl TemporalFactStore {
    pub fn record(
        &self,
        fact: TemporalFact,
        single_valued: bool,
    ) -> Result<TemporalFactReceipt, String> {
        self.db.lock().execute_batch("CREATE TABLE IF NOT EXISTS membrane_temporal_fact (fact_id TEXT PRIMARY KEY, subject TEXT NOT NULL, predicate TEXT NOT NULL, object_json TEXT NOT NULL, scope_id TEXT NOT NULL, authority TEXT NOT NULL, veracity TEXT NOT NULL, observed_at TEXT NOT NULL, valid_from TEXT NOT NULL, valid_until TEXT, expires_at TEXT, supersedes TEXT, payload_sha256 TEXT NOT NULL, transition_sha256 TEXT NOT NULL)").map_err(|e| e.to_string())?;
        validate_fact(&fact)?;
        let mut fact = fact;
        let conn = self.db.lock();
        let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
        if let Some((existing, stored_supersedes)) = tx
            .query_row(
                "SELECT payload_sha256,supersedes FROM membrane_temporal_fact WHERE fact_id=?1",
                [&fact.fact_id],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?)),
            )
            .optional()
            .map_err(|e| e.to_string())?
        {
            fact.supersedes = stored_supersedes;
            if existing != digest(&fact)? {
                return Err("temporal_fact_conflict".into());
            }
            let payload_sha256 = digest(&fact)?;
            return Ok(TemporalFactReceipt {
                fact_id: fact.fact_id,
                payload_sha256,
                transitions: vec![],
            });
        }
        let mut transitions = Vec::new();
        if single_valued {
            let mut stmt = tx.prepare("SELECT fact_id,valid_from FROM membrane_temporal_fact WHERE scope_id=?1 AND subject=?2 AND predicate=?3 AND veracity='supported' AND valid_until IS NULL ORDER BY valid_from,fact_id").map_err(|e| e.to_string())?;
            let current: Vec<(String, String)> = stmt
                .query_map(
                    rusqlite::params![fact.scope_id, fact.subject, fact.predicate],
                    |r| Ok((r.get(0)?, r.get(1)?)),
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
                tx.execute("UPDATE membrane_temporal_fact SET valid_until=?1, supersedes=CASE WHEN supersedes IS NULL THEN ?2 ELSE supersedes END, transition_sha256=?3 WHERE fact_id=?4", rusqlite::params![fact.valid_from, fact.fact_id, transition_sha256, from_id]).map_err(|e| e.to_string())?;
                transitions.push(TemporalTransition {
                    from_fact_id: from_id,
                    to_fact_id: fact.fact_id.clone(),
                    effective_at: fact.valid_from.clone(),
                    transition_sha256,
                });
            }
        }
        let payload_sha256 = digest(&fact)?;
        tx.execute("INSERT INTO membrane_temporal_fact(fact_id,subject,predicate,object_json,scope_id,authority,veracity,observed_at,valid_from,valid_until,expires_at,supersedes,payload_sha256,transition_sha256) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?)", rusqlite::params![fact.fact_id, fact.subject, fact.predicate, serde_json::to_string(&fact.object).map_err(|e| e.to_string())?, fact.scope_id, fact.authority, fact.veracity, fact.observed_at, fact.valid_from, fact.valid_until, fact.expires_at, fact.supersedes, payload_sha256, ""]).map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(TemporalFactReceipt {
            fact_id: fact.fact_id,
            payload_sha256,
            transitions,
        })
    }

    pub fn query(&self, query: TemporalFactQuery) -> Result<Vec<TemporalFact>, String> {
        if query.scope_chain.is_empty()
            || query.subject.trim().is_empty()
            || query.predicate.trim().is_empty()
            || !valid_instant(&query.as_of)
        {
            return Err("temporal_fact_query_invalid".into());
        }
        let conn = self.db.lock();
        for scope in query.scope_chain {
            let mut stmt = conn.prepare("SELECT fact_id,subject,predicate,object_json,scope_id,authority,veracity,observed_at,valid_from,valid_until,expires_at,supersedes FROM membrane_temporal_fact WHERE scope_id=?1 AND subject=?2 AND predicate=?3 AND veracity='supported' AND valid_from<=?4 AND (valid_until IS NULL OR ?4<valid_until) AND (expires_at IS NULL OR ?4<expires_at) ORDER BY valid_from DESC,fact_id DESC").map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map(
                    rusqlite::params![scope, query.subject, query.predicate, query.as_of],
                    |r| {
                        Ok(TemporalFact {
                            fact_id: r.get(0)?,
                            subject: r.get(1)?,
                            predicate: r.get(2)?,
                            object: serde_json::from_str(&r.get::<_, String>(3)?)
                                .unwrap_or(serde_json::Value::Null),
                            scope_id: r.get(4)?,
                            authority: r.get(5)?,
                            veracity: r.get(6)?,
                            observed_at: r.get(7)?,
                            valid_from: r.get(8)?,
                            valid_until: r.get(9)?,
                            expires_at: r.get(10)?,
                            supersedes: r.get(11)?,
                        })
                    },
                )
                .map_err(|e| e.to_string())?;
            let scoped = rows
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(|e| e.to_string())?;
            // Most-specific scope wins; ancestors are fallback only.
            if !scoped.is_empty() {
                return Ok(scoped);
            }
        }
        Ok(Vec::new())
    }
}

pub struct TemporalFactStore {
    pub(crate) db: MemDb,
}
impl TemporalFactStore {
    pub fn new(db: MemDb) -> Self {
        Self { db }
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
    if !matches!(
        f.authority.as_str(),
        "A0" | "A1" | "A2" | "A3" | "A4" | "A5"
    ) {
        return Err("temporal_fact_authority_invalid".into());
    }
    Ok(())
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
        store
            .record(fact("a", "old", "2026-08-01T00:00:00Z"), true)
            .unwrap();
        let receipt = store
            .record(fact("b", "new", "2026-08-02T00:00:00Z"), true)
            .unwrap();
        assert_eq!(receipt.transitions[0].from_fact_id, "a");
        let old = store
            .query(TemporalFactQuery {
                scope_chain: vec!["scope-a".into()],
                subject: "repo".into(),
                predicate: "owner".into(),
                as_of: "2026-08-01T12:00:00Z".into(),
            })
            .unwrap();
        assert_eq!(old[0].fact_id, "a");
        let new = store
            .query(TemporalFactQuery {
                scope_chain: vec!["scope-a".into()],
                subject: "repo".into(),
                predicate: "owner".into(),
                as_of: "2026-08-03T00:00:00Z".into(),
            })
            .unwrap();
        assert_eq!(new[0].fact_id, "b");
    }
    #[test]
    fn invalid_as_of_and_conflict_rejected() {
        let db = MemDb::open_in_memory();
        let store = TemporalFactStore::new(db);
        let f = fact("a", "old", "2026-08-01T00:00:00Z");
        store.record(f.clone(), false).unwrap();
        assert_eq!(
            store
                .record(fact("a", "other", "2026-08-01T00:00:00Z"), false)
                .unwrap_err(),
            "temporal_fact_conflict"
        );
        assert_eq!(
            store
                .query(TemporalFactQuery {
                    scope_chain: vec!["scope-a".into()],
                    subject: "repo".into(),
                    predicate: "owner".into(),
                    as_of: "tomorrow".into()
                })
                .unwrap_err(),
            "temporal_fact_query_invalid"
        );
    }

    #[test]
    fn scope_precedence_and_expiry_are_enforced() {
        let db = MemDb::open_in_memory();
        let store = TemporalFactStore::new(db);
        let mut local = fact("local", "local", "2026-08-01T00:00:00Z");
        local.scope_id = "workspace".into();
        local.expires_at = Some("2026-08-02T00:00:00Z".into());
        store.record(local, false).unwrap();
        let mut parent = fact("parent", "parent", "2026-08-01T00:00:00Z");
        parent.scope_id = "global".into();
        store.record(parent, false).unwrap();
        let current = store
            .query(TemporalFactQuery {
                scope_chain: vec!["workspace".into(), "global".into()],
                subject: "repo".into(),
                predicate: "owner".into(),
                as_of: "2026-08-01T12:00:00Z".into(),
            })
            .unwrap();
        assert_eq!(current[0].fact_id, "local");
        let expired = store
            .query(TemporalFactQuery {
                scope_chain: vec!["workspace".into(), "global".into()],
                subject: "repo".into(),
                predicate: "owner".into(),
                as_of: "2026-08-03T00:00:00Z".into(),
            })
            .unwrap();
        assert_eq!(expired[0].fact_id, "parent");
    }
}
