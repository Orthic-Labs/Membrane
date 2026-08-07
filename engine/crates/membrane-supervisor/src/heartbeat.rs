//! Per-client adapter heartbeat manager. Owns one `HeartbeatTable` keyed by
//! `client_id`, classifies each adapter into exactly one
//! [`AdapterStatus`](membrane_protocol::AdapterStatus) tier, and refuses the
//! MBR-207 acceptance condition: a single client being reported as both
//! `installed` and `delivering`.
//!
//! The Hub and the CLI read [`HeartbeatTable::snapshot`] through the same
//! table, so they cannot drift on which adapter is in which state. The
//! `detect_conflation` gate is the canonical answer to "did we ever
//! conflate installed with delivering?" and is what makes the acceptance
//! criterion testable.
//!
//! MBR-207: add per-client adapter heartbeat and mechanism receipt.

use std::collections::HashMap;

use membrane_protocol::heartbeat::{AdapterHeartbeatV1, AdapterStatus};

pub use membrane_protocol::heartbeat::{
    AdapterHeartbeatV1 as HeartbeatEnvelope, AdapterStatus as HeartbeatStatus,
    MechanismReceiptV1 as HeartbeatReceipt,
    ADAPTER_HEARTBEAT_SCHEMA_VERSION as HEARTBEAT_SCHEMA_VERSION,
};

use crate::error::{Result, SupervisorError};

/// Per-client heartbeat table. Keyed by `client_id`; one heartbeat per client
/// at a time. The supervisor, the Hub, and the CLI all read from the same
/// instance so they cannot disagree on the answer to "is client X active?".
#[derive(Debug, Default, Clone)]
pub struct HeartbeatTable {
    /// One heartbeat per `client_id`. The most recent `record` call wins.
    by_client: HashMap<String, AdapterHeartbeatV1>,
}

impl HeartbeatTable {
    /// Build an empty table.
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of distinct clients currently tracked. Exposed for diagnostics
    /// and the Hub/CLI status summary; not used by the conflation gate.
    pub fn len(&self) -> usize {
        self.by_client.len()
    }

    /// `true` when the table has no recorded heartbeats.
    pub fn is_empty(&self) -> bool {
        self.by_client.is_empty()
    }

    /// Upsert a heartbeat after validating its self-integrity. Refuses a
    /// heartbeat whose `verify_self_integrity()` returns `Err` so a malformed
    /// envelope cannot poison the table. The supervisor maps the resulting
    /// `HeartbeatConflation` error to its `heartbeat-conflation` exit label.
    pub fn record(&mut self, hb: AdapterHeartbeatV1) -> Result<()> {
        hb.verify_self_integrity()
            .map_err(|reason| SupervisorError::InvalidConfig { reason })?;
        self.by_client.insert(hb.client_id.clone(), hb);
        Ok(())
    }

    /// Canonical status for `client_id` at `now_unix`. Resolves the
    /// MBR-207 rules in this order:
    ///
    /// 1. No record at all → `Installed` (the adapter is on disk but has
    ///    not yet phoned home).
    /// 2. `last_seen` older than `now - staleness_threshold_secs` → `Stale`,
    ///    regardless of the adapter's self-declared status. The supervisor
    ///    refuses to trust a heartbeat that has not been refreshed.
    /// 3. Any `MechanismReceiptV1` with `count > 0` → `Delivering`. Receipts
    ///    are the only thing that can flip an adapter into the "currently
    ///    doing work" state.
    /// 4. Otherwise the table honours the adapter's self-declared status,
    ///    clamped to `{Active, Delivering}`. `Installed` is only ever a
    ///    "no heartbeat yet" answer; an adapter that emits a heartbeat but
    ///    still claims `Installed` is mis-self-describing and we coerce to
    ///    `Active` (no receipts ⇒ at least active).
    pub fn summarize_status(
        &self,
        client_id: &str,
        now_unix: u64,
        staleness_threshold_secs: u64,
    ) -> AdapterStatus {
        let Some(hb) = self.by_client.get(client_id) else {
            return AdapterStatus::Installed;
        };
        let last_seen = parse_rfc3339_utc(&hb.last_seen);
        let is_stale = match last_seen {
            Some(secs) => now_unix.saturating_sub(secs) > staleness_threshold_secs,
            None => true,
        };
        if is_stale {
            return AdapterStatus::Stale;
        }
        if hb.mechanism_receipts.iter().any(|r| r.count > 0) {
            return AdapterStatus::Delivering;
        }
        match hb.status {
            AdapterStatus::Delivering | AdapterStatus::Stale | AdapterStatus::Installed => {
                AdapterStatus::Active
            }
            AdapterStatus::Active => AdapterStatus::Active,
        }
    }

    /// MBR-207 acceptance gate. Returns `Err(SupervisorError::HeartbeatConflation)` if
    /// any single client has BOTH `installed = true` AND `delivering = true` in
    /// its observed history. A single status report is fine: only the coexistence
    /// of the two states on the same client is the failure mode the supervisor
    /// exists to prevent.
    pub fn detect_conflation(&self, client_id: &str) -> Result<()> {
        let Some(hb) = self.by_client.get(client_id) else {
            return Ok(());
        };
        let installed = matches!(hb.status, AdapterStatus::Installed);
        let delivering = matches!(hb.status, AdapterStatus::Delivering)
            || hb.mechanism_receipts.iter().any(|r| r.count > 0);
        if installed && delivering {
            return Err(SupervisorError::HeartbeatConflation {
                client_id: client_id.to_string(),
                installed: true,
                delivering: true,
            });
        }
        Ok(())
    }

    /// Read-only snapshot of every recorded heartbeat. The Hub and CLI both
    /// consume this; they MUST NOT derive their own booleans from it.
    pub fn snapshot(&self) -> Vec<AdapterHeartbeatV1> {
        let mut out: Vec<AdapterHeartbeatV1> = self.by_client.values().cloned().collect();
        out.sort_by(|a, b| a.client_id.cmp(&b.client_id));
        out
    }
}

/// Module-internal helper exported under the documented alias so the
/// supervisor's `lib.rs` re-export surfaces the right function names.
pub fn summarize_status(
    table: &HeartbeatTable,
    client_id: &str,
    now_unix: u64,
    staleness_threshold_secs: u64,
) -> AdapterStatus {
    table.summarize_status(client_id, now_unix, staleness_threshold_secs)
}

/// Module-internal helper exported under the documented alias so callers can
/// probe conflation directly. Equivalent to `table.detect_conflation`.
pub fn detect_conflation(table: &HeartbeatTable, client_id: &str) -> Result<()> {
    table.detect_conflation(client_id)
}

/// Parse the same RFC 3339 / ISO 8601 `YYYY-MM-DDTHH:MM:SSZ` shape that
/// `membrane-supervisor::admission` accepts. Returns `None` for anything else
/// so a malformed `last_seen` is treated as stale by `summarize_status`
/// rather than silently zero-epoch. The supervisor's `bin` crate already
/// mints whole-second UTC timestamps, so this subset is sufficient.
fn parse_rfc3339_utc(value: &str) -> Option<u64> {
    let bytes = value.as_bytes();
    if bytes.len() != 20 || bytes[10] != b'T' || bytes[bytes.len() - 1] != b'Z' {
        return None;
    }
    let year = digits(&bytes[0..4])?;
    let month = digits(&bytes[5..7])?;
    let day = digits(&bytes[8..10])?;
    let hour = digits(&bytes[11..13])?;
    let minute = digits(&bytes[14..16])?;
    let second = digits(&bytes[17..19])?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let secs = days_from_civil(year as i64, month as i64, day as i64)?
        .checked_mul(86_400)?
        .checked_add(hour as i64 * 3600 + minute as i64 * 60 + second as i64)?;
    if secs < 0 {
        return None;
    }
    Some(secs as u64)
}

fn digits(slice: &[u8]) -> Option<u64> {
    let mut total: u64 = 0;
    for byte in slice {
        if !byte.is_ascii_digit() {
            return None;
        }
        total = total.checked_mul(10)?.checked_add(u64::from(byte - b'0'))?;
    }
    Some(total)
}

/// Howard Hinnant's `days_from_civil` (CC0). Returns the signed number of
/// days from the Unix epoch (1970-01-01) to the supplied civil date in UTC.
fn days_from_civil(year: i64, month: i64, day: i64) -> Option<i64> {
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as i64;
    if !(0..=399).contains(&yoe) {
        return None;
    }
    let m = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * m + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let total = era
        .checked_mul(146_097)?
        .checked_add(doe)?
        .checked_sub(719_468)?;
    Some(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use membrane_protocol::heartbeat::MechanismReceiptV1;

    fn heartbeat(
        client_id: &str,
        status: AdapterStatus,
        last_seen: &str,
        receipts: Vec<MechanismReceiptV1>,
    ) -> AdapterHeartbeatV1 {
        AdapterHeartbeatV1::new(client_id, "0.1.0", status, last_seen, receipts)
    }

    fn receipt(count: u64) -> MechanismReceiptV1 {
        MechanismReceiptV1 {
            mechanism: "membrane_context".to_string(),
            scope_grant: "grant:test".to_string(),
            last_used: "2026-08-07T00:00:00Z".to_string(),
            count,
        }
    }

    #[test]
    fn record_and_summarize_no_receipts_is_active() {
        let mut table = HeartbeatTable::new();
        let now: u64 = 1_755_124_800; // 2025-08-07T00:00:00Z
        table
            .record(heartbeat(
                "cursor",
                AdapterStatus::Active,
                "2026-08-07T00:00:00Z",
                vec![],
            ))
            .expect("record succeeds");
        let status = table.summarize_status("cursor", now + 86_400, 60);
        assert_eq!(status, AdapterStatus::Active);
        assert!(table.detect_conflation("cursor").is_ok());
    }

    #[test]
    fn non_zero_receipt_count_promotes_to_delivering() {
        let mut table = HeartbeatTable::new();
        let now: u64 = 1_755_124_800;
        table
            .record(heartbeat(
                "windsurf",
                AdapterStatus::Active,
                "2026-08-07T00:00:00Z",
                vec![receipt(3)],
            ))
            .expect("record succeeds");
        let status = table.summarize_status("windsurf", now + 60, 120);
        assert_eq!(status, AdapterStatus::Delivering);
    }

    #[test]
    fn stale_last_seen_overrides_self_declared_status() {
        let mut table = HeartbeatTable::new();
        // `now` is 2025-07-28T16:00:00Z; the heartbeat is 2 hours earlier so
        // the staleness threshold of 60s is exceeded by 7199 seconds.
        let now: u64 = 1_755_124_800;
        table
            .record(heartbeat(
                "codex",
                AdapterStatus::Delivering,
                "2025-07-28T14:00:00Z",
                vec![receipt(5)],
            ))
            .expect("record succeeds");
        // Two hours after the heartbeat, the staleness threshold is 60s.
        let status = table.summarize_status("codex", now + 7_200, 60);
        assert_eq!(status, AdapterStatus::Stale);
    }

    #[test]
    fn detect_conflation_fires_on_installed_and_delivering() {
        let mut table = HeartbeatTable::new();
        // A heartbeat that simultaneously claims `Installed` and carries a
        // non-zero receipt is the exact conflation the gate forbids.
        table
            .record(heartbeat(
                "broken",
                AdapterStatus::Installed,
                "2026-08-07T00:00:00Z",
                vec![receipt(1)],
            ))
            .expect("record succeeds");
        let err = table
            .detect_conflation("broken")
            .expect_err("conflation must be flagged");
        match err {
            SupervisorError::HeartbeatConflation {
                ref client_id,
                installed,
                delivering,
            } => {
                assert_eq!(client_id, "broken");
                assert!(installed);
                assert!(delivering);
            }
            other => panic!("expected HeartbeatConflation, got {other:?}"),
        }
        assert_eq!(err.label(), "heartbeat-conflation");
    }

    #[test]
    fn unknown_client_is_installed_and_clean() {
        let table = HeartbeatTable::new();
        assert_eq!(
            table.summarize_status("nobody", 1_755_124_800, 60),
            AdapterStatus::Installed
        );
        assert!(table.detect_conflation("nobody").is_ok());
    }

    #[test]
    fn snapshot_returns_one_record_per_client() {
        let mut table = HeartbeatTable::new();
        table
            .record(heartbeat(
                "codex",
                AdapterStatus::Active,
                "2026-08-07T00:00:00Z",
                vec![],
            ))
            .unwrap();
        table
            .record(heartbeat(
                "cursor",
                AdapterStatus::Delivering,
                "2026-08-07T00:00:00Z",
                vec![receipt(2)],
            ))
            .unwrap();
        let snapshot = table.snapshot();
        assert_eq!(snapshot.len(), 2);
        assert_eq!(snapshot[0].client_id, "codex");
        assert_eq!(snapshot[1].client_id, "cursor");
        // Re-recording the same client_id replaces, never duplicates.
        table
            .record(heartbeat(
                "codex",
                AdapterStatus::Stale,
                "2026-08-07T01:00:00Z",
                vec![],
            ))
            .unwrap();
        assert_eq!(table.snapshot().len(), 2);
    }

    #[test]
    fn record_rejects_unsupported_schema_version() {
        let mut table = HeartbeatTable::new();
        let mut hb = heartbeat("bad", AdapterStatus::Active, "2026-08-07T00:00:00Z", vec![]);
        hb.schema_version = 2;
        let err = table
            .record(hb)
            .expect_err("unknown schema must be rejected");
        assert!(matches!(err, SupervisorError::InvalidConfig { .. }));
    }
}
