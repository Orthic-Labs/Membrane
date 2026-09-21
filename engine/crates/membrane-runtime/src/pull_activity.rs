//! Persist final native Pull decisions into the existing content-free catalog.
//! This records delivery, never host insertion or a new admission decision.
use rusqlite::{params, Connection, OpenFlags};
use serde_json::Value;
use std::{collections::HashSet, sync::atomic::{AtomicU64, Ordering}, time::{Duration, SystemTime, UNIX_EPOCH}};

static SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub(crate) fn record_native_pull(arguments: &Value, result: &Value) {
    if arguments.get("operation").and_then(Value::as_str).is_some_and(|kind| kind != "context") {
        return;
    }
    let persist = || -> Result<(), String> {
        let path = crate::catalog::default_catalog_path().map_err(|e| e.to_string())?;
        // The runtime owns creation/migration. Telemetry must neither create a
        // second catalog nor hold a delivered response behind a long busy wait.
        let mut conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE)
            .map_err(|e| e.to_string())?;
        conn.busy_timeout(Duration::from_millis(25)).map_err(|e| e.to_string())?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0)).map_err(|e| e.to_string())?;
        if version != crate::catalog::CATALOG_SCHEMA_VERSION {
            return Err("catalog_schema_generation_mismatch".into());
        }
        let trace = format!("native-pull-{}-{}-{}", std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed));
        persist_result(&mut conn, result, &trace).map_err(|e| e.to_string())
    };
    if let Err(reason) = persist() {
        eprintln!("{}", serde_json::json!({"event":"native_pull_activity_unavailable","reason":reason}));
    }
}

fn field<'a>(value: &'a Value, key: &str, fallback: &'a str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or(fallback)
}

fn persist_result(conn: &mut Connection, result: &Value, trace: &str) -> rusqlite::Result<()> {
    let data = &result["result"]["data"];
    let successful = result.pointer("/result/kind").and_then(Value::as_str) == Some("success");
    let status = if successful { field(data, "status", "unavailable") } else { "error" };
    let reason = if successful { field(data, "degradationReason", "none") }
        else { field(&result["result"], "code", "context_unavailable") };
    let blocks = data.pointer("/packet/blocks").and_then(Value::as_array);
    let delivered: HashSet<&str> = blocks.into_iter().flatten()
        .filter_map(|block| block.get("id").and_then(Value::as_str)).collect();
    let status = if status == "ok" && delivered.is_empty() { "empty" } else { status };
    let receipts = data.get("receipts").and_then(Value::as_array);
    let now = crate::catalog::ContextCatalog::now_unix();
    let tx = conn.transaction()?;
    tx.execute("INSERT INTO retrieval_events
        (ts_unix,trace_id,client,mode,provider,provider_status,fallback_mode,degradation_reason,source_generation,candidate_count,admitted_count)
        VALUES (?1,?2,'native_mcp','native_pull','federation',?3,'none',?4,NULL,?5,?6)",
        params![now, trace, status, reason, receipts.map_or(0, Vec::len) as i64,
            if status == "ok" { delivered.len() as i64 } else { 0 }])?;
    for receipt in receipts.into_iter().flatten() {
        let id = receipt.get("id").and_then(Value::as_str).ok_or(rusqlite::Error::InvalidQuery)?;
        let planned = field(receipt, "decision", "rejected");
        let admitted = status == "ok" && planned == "admitted" && delivered.contains(id);
        let decision = if admitted { "admitted" } else { "rejected" };
        let receipt_reason = if planned == "admitted" && !admitted { "not_delivered" }
            else { field(receipt, "reason", "unspecified") };
        tx.execute("INSERT OR IGNORE INTO receipts
            (receipt_id,trace_id,candidate_id,decision,reason,provider,provider_status,fallback_mode,degradation_reason,written_at_unix,bytes_sha256)
            VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            params![format!("{trace}|{id}"), trace, id, decision, receipt_reason,
                field(receipt,"provider","unknown"), field(receipt,"providerStatus","unknown"),
                field(receipt,"fallbackMode","none"), field(receipt,"degradationReason","none"), now,
                crate::digest::digest_str(&receipt.to_string())])?;
    }
    tx.commit()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn native_pull_activity_updates_real_admission_snapshot() {
        let catalog = crate::catalog::ContextCatalog::open_in_memory();
        let mut conn = catalog.lock();
        persist_result(&mut conn, &json!({"result":{"kind":"success","data":{
            "status":"ok","packet":{"blocks":[{"id":"kept"}]},"receipts":[
                {"id":"kept","decision":"admitted","reason":"within_global_budget","provider":"ledger"},
                {"id":"dropped","decision":"rejected","reason":"budget_exhausted","provider":"ledger"},
                {"id":"not-in-wire","decision":"admitted","reason":"within_global_budget","provider":"ledger"}
            ]}}}), "request-1").unwrap();
        let report = crate::admission_producer::build_admission_report_from(&conn, 24).unwrap();
        assert_eq!(report.decisions_total - report.omissions_total, 1);
        assert_eq!(report.omissions_total, 2);
        assert_eq!(report.budget_pressure_total, 1);
        assert_eq!(report.last_pull_status.as_deref(), Some("ok"));
        assert!(report.last_pull_observed_at_unix_ms.unwrap() > 0);
    }

    #[test]
    fn native_pull_activity_records_empty_delivery_without_fake_candidates() {
        let catalog = crate::catalog::ContextCatalog::open_in_memory();
        let mut conn = catalog.lock();
        persist_result(&mut conn, &json!({"result":{"kind":"success","data":{
            "status":"insufficient_confidence","packet":null,"degradationReason":"blueprint_stale"
        }}}), "request-2").unwrap();
        let report = crate::admission_producer::build_admission_report_from(&conn, 24).unwrap();
        assert_eq!(report.decisions_total, 0);
        assert_eq!(report.last_pull_status.as_deref(), Some("insufficient_confidence"));
        assert_eq!(report.last_pull_reason.as_deref(), Some("blueprint_stale"));
    }
}
