//! MBR: producer for the Hub's admission ledger aggregate.
//!
//! `membrane-runtime::catalog::record_receipt` already persists one
//! content-free row per candidate decision (`receipts.decision`,
//! `receipts.reason`) into the catalog sqlite database every time
//! `serve.rs`'s planner route runs (`serve.rs:4961`). `receipts.decision` is
//! either `admitted` or `rejected`; `receipts.reason` carries the real
//! typed reason from `cortex-core::planner` (`budget_exhausted`,
//! `packet_block_limit`, `cross_root`, `superseded_version`,
//! `duplicate_id`, …). This module only aggregates those already-recorded
//! rows over a trailing window — it adds no new instrumentation and
//! fabricates nothing.
//!
//! `budget_exhausted` and `packet_block_limit` are the two reasons the
//! planner emits when a candidate is dropped specifically because the
//! attention/packet budget is full (`cortex-core/src/planner.rs:807,812`);
//! every other rejection reason is an admission omission but not budget
//! pressure.

use crate::hub_readonly_db::{now_unix_ms, open_readonly_catalog};
use membrane_protocol::{AdmissionReasonCountV1, HubAdmissionV1, HUB_ADMISSION_SCHEMA_VERSION};

const WINDOW_HOURS: u32 = 24;
const BUDGET_REASONS: [&str; 2] = ["budget_exhausted", "packet_block_limit"];

pub fn build_admission_report() -> Option<HubAdmissionV1> {
    let conn = open_readonly_catalog()?;
    build_admission_report_from(&conn, WINDOW_HOURS)
}

pub(crate) fn build_admission_report_from(
    conn: &rusqlite::Connection,
    window_hours: u32,
) -> Option<HubAdmissionV1> {
    let window_start_unix = (now_unix_ms() / 1000) as i64 - i64::from(window_hours) * 3600;

    let decisions_total: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM receipts WHERE written_at_unix >= ?1",
            [window_start_unix],
            |row| row.get(0),
        )
        .ok()?;

    let mut stmt = conn
        .prepare(
            "SELECT reason, COUNT(*) FROM receipts
             WHERE written_at_unix >= ?1 AND decision != 'admitted'
             GROUP BY reason ORDER BY COUNT(*) DESC, reason ASC",
        )
        .ok()?;
    let rows = stmt
        .query_map([window_start_unix], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as u64))
        })
        .ok()?;

    let mut omissions_by_reason = Vec::new();
    let mut omissions_total: u64 = 0;
    let mut budget_pressure_by_reason = Vec::new();
    let mut budget_pressure_total: u64 = 0;
    for row in rows {
        let (reason, count) = row.ok()?;
        omissions_total += count;
        if BUDGET_REASONS.contains(&reason.as_str()) {
            budget_pressure_total += count;
            budget_pressure_by_reason.push(AdmissionReasonCountV1 {
                reason: reason.clone(),
                count,
            });
        }
        omissions_by_reason.push(AdmissionReasonCountV1 { reason, count });
    }

    Some(HubAdmissionV1 {
        schema_version: HUB_ADMISSION_SCHEMA_VERSION,
        window_hours,
        decisions_total: decisions_total as u64,
        omissions_total,
        omissions_by_reason,
        budget_pressure_total,
        budget_pressure_by_reason,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog;

    fn seed(catalog: &catalog::ContextCatalog, decision: &str, reason: &str, age_secs: i64) {
        let conn = catalog.lock();
        let written_at = catalog::ContextCatalog::now_unix() - age_secs;
        conn.execute(
            "INSERT INTO receipts (receipt_id, trace_id, candidate_id, decision, reason,
              provider, provider_status, fallback_mode, degradation_reason,
              written_at_unix, bytes_sha256)
             VALUES (?1, 't', 'c', ?2, ?3, 'cortex', 'ok', 'none', 'none', ?4, 'sha')",
            rusqlite::params![
                format!("{decision}-{reason}-{age_secs}"),
                decision,
                reason,
                written_at
            ],
        )
        .unwrap();
    }

    #[test]
    fn aggregates_real_rejection_reasons_and_splits_budget_pressure() {
        let catalog = catalog::ContextCatalog::open_in_memory();
        seed(&catalog, "admitted", "none", 60);
        seed(&catalog, "rejected", "budget_exhausted", 60);
        seed(&catalog, "rejected", "budget_exhausted", 120);
        seed(&catalog, "rejected", "packet_block_limit", 60);
        seed(&catalog, "rejected", "cross_root", 60);
        // Outside the window: must not be counted.
        seed(&catalog, "rejected", "budget_exhausted", 100 * 3600);

        let conn = catalog.lock();
        let report = build_admission_report_from(&conn, 24).unwrap();
        assert_eq!(report.decisions_total, 5);
        assert_eq!(report.omissions_total, 4);
        assert_eq!(report.budget_pressure_total, 3);
        let cross_root = report
            .omissions_by_reason
            .iter()
            .find(|r| r.reason == "cross_root")
            .unwrap();
        assert_eq!(cross_root.count, 1);
        let budget_exhausted = report
            .budget_pressure_by_reason
            .iter()
            .find(|r| r.reason == "budget_exhausted")
            .unwrap();
        assert_eq!(budget_exhausted.count, 2);
    }

    #[test]
    fn empty_ledger_reports_zero_not_absent() {
        let catalog = catalog::ContextCatalog::open_in_memory();
        let conn = catalog.lock();
        let report = build_admission_report_from(&conn, 24).unwrap();
        assert_eq!(report.decisions_total, 0);
        assert_eq!(report.omissions_total, 0);
        assert!(report.omissions_by_reason.is_empty());
    }

    #[test]
    fn incompatible_catalog_schema_yields_none() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        assert!(build_admission_report_from(&conn, 24).is_none());
    }
}
