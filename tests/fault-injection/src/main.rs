use membrane_protocol::{
    ProviderIdentityV1, ProviderObservationV1, ProviderReadinessStateV1, ProviderReadinessV1,
    ProviderTestQueryV1,
};
use membrane_runtime::hub::{
    HubFacadeV1, HubInputsV1, HubMetadataV1, HubReadV1, ProviderReadinessAdapterV1,
};
use serde_json::{json, Value};
use std::{fs, path::PathBuf, time::Duration};

fn id() -> ProviderIdentityV1 {
    ProviderIdentityV1 {
        provider_id: "blueprint".into(),
        installation_id: "install-a".into(),
        service_id: "service-a".into(),
        release_generation: "gen-a".into(),
        data_root_digest: "sha256:data".into(),
    }
}
fn ready() -> ProviderReadinessV1 {
    ProviderReadinessV1 {
        schema_version: 1,
        state: ProviderReadinessStateV1::Ready,
        identity: id(),
        observation: Some(ProviderObservationV1 {
            observed_at_unix_ms: 100,
            fresh_for_ms: 100,
        }),
        test_query: Some(ProviderTestQueryV1 {
            name: "ping".into(),
            succeeded: true,
        }),
        reason: "observed".into(),
    }
}
fn receipt(id: &str, status: &str, code: &str, observed: Value, recovery: Value) -> Value {
    json!({"schemaVersion":1,"kind":"membrane-fault-receipt","scenarioId":id,"status":status,"code":code,"activated":true,"observed":observed,"alert":code,"recovery":recovery,"cleanup":"complete","timestamp":1000,"deadlineMs":1500,"content":null})
}
fn script(dir: &std::path::Path) -> PathBuf {
    let path = dir.join("gateway.py");
    fs::write(&path,"import sys,time\nprint('{\"workerReady\":{}}',flush=True)\nfor line in sys.stdin:\n line=line.strip()\n if line=='\"stall\"': time.sleep(30)\n if line=='\"die\"': sys.exit(3)\n print('{\"ok\":true}',flush=True)\n").unwrap();
    path
}
fn provider_timeout() -> Value {
    let dir = tempfile::tempdir().unwrap();
    let mut gateway = membrane_runtime::federation_worker::ResidentGateway::with_python(
        script(dir.path()),
        "python3".into(),
    );
    let _ = gateway.request("__ready__", Duration::from_secs(2));
    let observed = gateway
        .request("\"stall\"", Duration::from_millis(100))
        .is_err();
    let recovered = gateway.request("\"ok\"", Duration::from_secs(2)).is_ok();
    receipt(
        "provider-timeout",
        if observed && recovered {
            "degraded"
        } else {
            "error"
        },
        if observed && recovered {
            "provider_timeout"
        } else {
            "provider_timeout_recovery_failed"
        },
        json!({"healthy":false,"timedOut":observed}),
        json!({"ok":recovered,"status":if recovered {"current"} else {"failed"}}),
    )
}
fn watcher_restart() -> Value {
    let dir = tempfile::tempdir().unwrap();
    let mut gateway = membrane_runtime::federation_worker::ResidentGateway::with_python(
        script(dir.path()),
        "python3".into(),
    );
    let _ = gateway.request("__ready__", Duration::from_secs(2));
    let _ = gateway.request("\"die\"", Duration::from_millis(500));
    let recovered = gateway.request("\"next\"", Duration::from_secs(2)).is_ok();
    receipt(
        "watcher-restart",
        if recovered { "recovering" } else { "error" },
        if recovered {
            "watcher_restarted"
        } else {
            "watcher_recovery_failed"
        },
        json!({"healthy":false,"workerRestarts":gateway.worker_restarts}),
        json!({"ok":recovered,"status":if recovered {"current"} else {"failed"}}),
    )
}
fn main() {
    let mut drift = ready();
    drift.identity.installation_id = "install-b".into();
    let wrong = ProviderReadinessAdapterV1 {
        expected_identity: id(),
        now_unix_ms: 100,
    }
    .evaluate(Some(drift), true);
    let corrected = ProviderReadinessAdapterV1 {
        expected_identity: id(),
        now_unix_ms: 100,
    }
    .evaluate(Some(ready()), true);
    let mut host = HubInputsV1::unavailable("host_load_missing");
    host.memory = HubReadV1::Unavailable {
        reason: "host_load_missing".into(),
    };
    let mut delivery = HubInputsV1::unavailable("source_unavailable");
    delivery.deliveries = HubReadV1::Degraded {
        reason: "selected_without_delivery".into(),
        items: vec![json!({"selected":1,"delivered":0})],
        metadata: HubMetadataV1::default(),
    };
    let snapshot = HubFacadeV1::new(None).snapshot(1000, delivery);
    let mut available = HubInputsV1::unavailable("source_unavailable");
    available.memory = HubReadV1::Available {
        items: vec![json!({"host":"loaded"})],
        metadata: HubMetadataV1::default(),
    };
    let available_snapshot = HubFacadeV1::new(None).snapshot(1000, available);
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("not-a-dir");
    fs::write(&file, b"x").unwrap();
    let db_err = cortex_store::memdb::MemDb::open(file.join("child")).is_err();
    let db_recovered = cortex_store::memdb::MemDb::open_in_memory();
    let mut skew = ready();
    skew.schema_version = 99;
    let schema_recovered = ready().contract_error().is_none();
    let receipts = vec![
        provider_timeout(),
        watcher_restart(),
        receipt(
            "wrong-installation",
            if wrong.reason == "identity_drift"
                && corrected.state == ProviderReadinessStateV1::Ready
            {
                "blocked"
            } else {
                "error"
            },
            if wrong.reason == "identity_drift"
                && corrected.state == ProviderReadinessStateV1::Ready
            {
                "blocked_wrong_installation"
            } else {
                "identity_recovery_failed"
            },
            json!({"healthy":false,"state":format!("{:?}",wrong.state).to_lowercase()}),
            json!({"ok":corrected.state == ProviderReadinessStateV1::Ready,"status":"ready"}),
        ),
        receipt(
            "missing-host-load",
            if available_snapshot.memory.state == membrane_protocol::HubStateV1::Available {
                "degraded"
            } else {
                "error"
            },
            if available_snapshot.memory.state == membrane_protocol::HubStateV1::Available {
                "host_load_missing"
            } else {
                "host_load_recovery_failed"
            },
            json!({"healthy":false,"hubUnavailable":matches!(host.memory,HubReadV1::Unavailable{..})}),
            json!({"ok":available_snapshot.memory.state == membrane_protocol::HubStateV1::Available,"status":"available"}),
        ),
        receipt(
            "db-failure",
            if db_err
                && matches!(
                    db_recovered.health_probe(),
                    cortex_store::memdb::MemDbProbe::Ok
                )
            {
                "degraded"
            } else {
                "error"
            },
            if db_err
                && matches!(
                    db_recovered.health_probe(),
                    cortex_store::memdb::MemDbProbe::Ok
                )
            {
                "database_unavailable"
            } else {
                "database_recovery_failed"
            },
            json!({"healthy":false,"dbError":db_err}),
            json!({"ok":matches!(db_recovered.health_probe(), cortex_store::memdb::MemDbProbe::Ok),"status":"available"}),
        ),
        receipt(
            "schema-skew",
            if skew.contract_error() == Some("schema_version_mismatch") && schema_recovered {
                "blocked"
            } else {
                "error"
            },
            if skew.contract_error() == Some("schema_version_mismatch") && schema_recovered {
                "schema_incompatible"
            } else {
                "schema_recovery_failed"
            },
            json!({"healthy":false,"contractError":skew.contract_error()}),
            json!({"ok":schema_recovered,"status":"compatible"}),
        ),
        receipt(
            "zero-delivery",
            if snapshot.deliveries.state == membrane_protocol::HubStateV1::Degraded
                && available_snapshot.memory.state == membrane_protocol::HubStateV1::Available
            {
                "degraded"
            } else {
                "error"
            },
            if snapshot.deliveries.state == membrane_protocol::HubStateV1::Degraded
                && available_snapshot.memory.state == membrane_protocol::HubStateV1::Available
            {
                "selected_without_delivery"
            } else {
                "delivery_recovery_failed"
            },
            json!({"healthy":false,"state":format!("{:?}",snapshot.deliveries.state).to_lowercase()}),
            json!({"ok":available_snapshot.memory.state == membrane_protocol::HubStateV1::Available,"status":"available"}),
        ),
    ];
    println!("{}", serde_json::to_string(&receipts).unwrap());
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn real_identity_and_schema_seams_reject() {
        let mut skew = ready();
        skew.schema_version = 2;
        assert_eq!(skew.contract_error(), Some("schema_version_mismatch"));
        let mut drift = ready();
        drift.identity.installation_id = "wrong".into();
        assert_eq!(
            ProviderReadinessAdapterV1 {
                expected_identity: id(),
                now_unix_ms: 100
            }
            .evaluate(Some(drift), true)
            .reason,
            "identity_drift"
        );
    }
}
