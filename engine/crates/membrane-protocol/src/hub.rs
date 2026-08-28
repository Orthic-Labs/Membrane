use crate::membrane_status::MembraneParentState;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

pub const HUB_SCHEMA_VERSION: u32 = 1;

/// Closed reason taxonomy for an unavailable Membrane capability binding.
/// Hub inactivity is deliberately distinct from a degraded subsystem: when
/// no Hub is active there is no Membrane runtime to service the request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MembraneUnavailableReasonV1 {
    HubInactive,
}

/// Canonical Hub-off response shared by stateless clients and host adapters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MembraneUnavailableV1 {
    pub kind: String,
    pub reason: MembraneUnavailableReasonV1,
    pub retryable: bool,
}

impl MembraneUnavailableV1 {
    pub fn hub_inactive() -> Self {
        Self {
            kind: "membrane_unavailable".into(),
            reason: MembraneUnavailableReasonV1::HubInactive,
            retryable: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HubStateV1 {
    Available,
    Degraded,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HubSectionV1 {
    pub state: HubStateV1,
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub items: Option<Vec<Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolver: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evidence: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observed_at_unix_ms: Option<u64>,
}

impl HubSectionV1 {
    pub fn unavailable(reason: impl Into<String>) -> Self {
        let reason = reason.into();
        Self {
            state: HubStateV1::Unavailable,
            reason: if reason.is_empty() {
                "reason_unavailable".into()
            } else {
                reason
            },
            items: None,
            resolver: None,
            evidence: None,
            observed_at_unix_ms: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HubStreamV1 {
    pub state: HubStateV1,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolver: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HubCapabilitiesV1 {
    pub schema_version: u32,
    pub read_only: bool,
    pub resources: Vec<String>,
    pub operations: Vec<String>,
    pub installation_id: String,
    pub service_id: String,
    pub release_generation: String,
    pub data_root_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream: Option<HubStreamV1>,
}

/// Semantic state of one Membrane subsystem. `NotConfigured` is a first-class
/// wire value: "no instrumentation exists" must never be encoded as
/// `Unavailable` plus a magic reason that presentation code has to decode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubsystemStateV1 {
    Available,
    Degraded,
    Unavailable,
    NotConfigured,
}

/// One semantic subsystem surface (Pull/Push/Cortex/Blueprint/Ledger/Adapt).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HubSubsystemV1 {
    pub state: SubsystemStateV1,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub items: Option<Vec<Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_at_unix_ms: Option<u64>,
}

impl HubSubsystemV1 {
    pub fn not_configured(reason: impl Into<String>) -> Self {
        Self {
            state: SubsystemStateV1::NotConfigured,
            reason: reason.into(),
            items: None,
            evidence: None,
            observed_at_unix_ms: None,
        }
    }
}

/// The six semantic Membrane subsystems — closed, named fields so no producer
/// can emit an unnamed or missing subsystem on the wire.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HubSubsystemsV1 {
    pub pull: HubSubsystemV1,
    pub push: HubSubsystemV1,
    pub cortex: HubSubsystemV1,
    pub blueprint: HubSubsystemV1,
    pub ledger: HubSubsystemV1,
    pub adapt: HubSubsystemV1,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HubSnapshotV1 {
    pub schema_version: u32,
    pub product_id: String,
    pub observed_at_unix_ms: u64,
    pub sections: BTreeMap<String, HubSectionV1>,
    /// Frozen parent service state from the resident producer. Typed, never a
    /// free-form string; `None` only for snapshots written before this field
    /// existed (old cached payloads).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub membrane_state: Option<MembraneParentState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subsystems: Option<HubSubsystemsV1>,
    /// Additive V1 admission-ledger aggregate (omissions + budget pressure).
    /// `None` when the resident's catalog receipt store is unreadable or the
    /// snapshot predates this field — never a fabricated zero.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub admission: Option<HubAdmissionV1>,
}

pub const HUB_ADMISSION_SCHEMA_VERSION: u32 = 1;

/// One typed decision reason (verbatim from `cortex-core`'s planner
/// receipts, e.g. `budget_exhausted`, `packet_block_limit`, `cross_root`)
/// with its count over the report window. Never paraphrased.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AdmissionReasonCountV1 {
    pub reason: String,
    pub count: u64,
}

/// Aggregate of the catalog's persisted, content-free receipt ledger
/// (`membrane-runtime::catalog` `receipts` table) over a fixed trailing
/// window. Additive V1 shape — never mutates the five public V1 shapes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HubAdmissionV1 {
    pub schema_version: u32,
    pub window_hours: u32,
    pub decisions_total: u64,
    pub omissions_total: u64,
    pub omissions_by_reason: Vec<AdmissionReasonCountV1>,
    pub budget_pressure_total: u64,
    pub budget_pressure_by_reason: Vec<AdmissionReasonCountV1>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hub_inactive_is_closed_typed_and_retryable() {
        let unavailable = MembraneUnavailableV1::hub_inactive();
        assert_eq!(
            serde_json::to_value(&unavailable).unwrap(),
            serde_json::json!({
                "kind": "membrane_unavailable",
                "reason": "hub_inactive",
                "retryable": true
            })
        );
        assert!(
            serde_json::from_value::<MembraneUnavailableV1>(serde_json::json!({
                "kind": "membrane_unavailable",
                "reason": "hub_inactive",
                "retryable": true,
                "unexpected": true
            }))
            .is_err()
        );
    }

    #[test]
    fn admission_aggregate_is_additive_versioned_and_closed() {
        let admission = HubAdmissionV1 {
            schema_version: HUB_ADMISSION_SCHEMA_VERSION,
            window_hours: 24,
            decisions_total: 9,
            omissions_total: 3,
            omissions_by_reason: vec![AdmissionReasonCountV1 {
                reason: "cross_root".into(),
                count: 1,
            }],
            budget_pressure_total: 2,
            budget_pressure_by_reason: vec![AdmissionReasonCountV1 {
                reason: "budget_exhausted".into(),
                count: 2,
            }],
        };
        let encoded = serde_json::to_value(&admission).unwrap();
        assert_eq!(encoded["schemaVersion"], 1);
        assert_eq!(encoded["windowHours"], 24);
        assert_eq!(encoded["omissionsByReason"][0]["reason"], "cross_root");
        assert_eq!(
            encoded["budgetPressureByReason"][0]["reason"],
            "budget_exhausted"
        );
        assert!(serde_json::from_value::<HubAdmissionV1>(serde_json::json!({
            "schemaVersion": 1,
            "windowHours": 24,
            "decisionsTotal": 0,
            "omissionsTotal": 0,
            "omissionsByReason": [],
            "budgetPressureTotal": 0,
            "budgetPressureByReason": [],
            "unexpected": true
        }))
        .is_err());
    }

    #[test]
    fn legacy_snapshot_without_admission_still_deserializes() {
        let snapshot: HubSnapshotV1 = serde_json::from_value(serde_json::json!({
            "schemaVersion": 1,
            "productId": "membrane",
            "observedAtUnixMs": 42,
            "sections": {}
        }))
        .unwrap();
        assert!(snapshot.admission.is_none());
    }
}
