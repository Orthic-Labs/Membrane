//! Bounded, non-mutating reads over the existing absorbed event ledger.
use crate::{MemDb, SessionEvent};
use rusqlite::{params, OptionalExtension};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct ReferenceEventPage {
    pub available: bool,
    pub events: Vec<SessionEvent>,
    pub truncated: bool,
}

impl MemDb {
    /// This read never installs tables or converts an absent producer into an
    /// empty successful stream. Scope and event family are exact SQL filters.
    pub fn reference_events(
        &self,
        scope: &str,
        family: &str,
        limit: usize,
    ) -> Result<ReferenceEventPage, String> {
        if scope.trim().is_empty() || family.trim().is_empty() || limit == 0 || limit > 128 {
            return Err("invalid reference event bounds".into());
        }
        let conn = self.lock();
        let present = conn
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type='table' AND name='absorbed_events'",
                [],
                |_| Ok(()),
            )
            .optional()
            .map_err(|e| e.to_string())?
            .is_some();
        if !present {
            return Ok(ReferenceEventPage {
                available: false,
                events: vec![],
                truncated: false,
            });
        }
        let mut statement = conn.prepare("SELECT payload_json FROM absorbed_events WHERE scope_id=?1 AND event_type=?2 AND tombstoned=0 ORDER BY recorded_at_ms DESC,event_id LIMIT ?3")
            .map_err(|e| e.to_string())?;
        let rows = statement
            .query_map(params![scope, family, (limit + 1) as i64], |r| {
                r.get::<_, String>(0)
            })
            .map_err(|e| e.to_string())?;
        let mut events = Vec::new();
        for row in rows {
            events.push(
                serde_json::from_str(&row.map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?,
            );
        }
        let truncated = events.len() > limit;
        events.truncate(limit);
        Ok(ReferenceEventPage {
            available: true,
            events,
            truncated,
        })
    }
}
