use membrane_protocol::{
    HubCapabilitiesV1, HubSectionV1, HubSnapshotV1, HubStateV1, HubStreamV1, ProviderIdentityV1,
    ProviderReadinessStateV1, ProviderReadinessV1, HUB_SCHEMA_VERSION,
};
use serde_json::Value;
use std::collections::BTreeMap;

pub const HUB_RESOURCES: [&str; 8] = [
    "deliveries",
    "providers",
    "repositories",
    "adapters",
    "devices",
    "memory",
    "sentinel",
    "alerts",
];
pub const HUB_OPERATIONS: [&str; 2] = ["hub.capabilities", "hub.snapshot"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderReadinessAdapterV1 {
    pub expected_identity: ProviderIdentityV1,
    pub now_unix_ms: u64,
}

impl ProviderReadinessAdapterV1 {
    pub fn evaluate(
        &self,
        reply: Option<ProviderReadinessV1>,
        process_exists: bool,
    ) -> ProviderReadinessV1 {
        let Some(mut readiness) = reply else {
            return ProviderReadinessV1::unknown(
                self.expected_identity.clone(),
                if process_exists {
                    "process_exists_without_readiness"
                } else {
                    "readiness_missing"
                },
            );
        };
        if let Some(reason) = readiness.contract_error() {
            readiness.state = ProviderReadinessStateV1::Unknown;
            readiness.reason = reason.into();
            return readiness;
        }
        if readiness.identity != self.expected_identity {
            readiness.state = ProviderReadinessStateV1::Unavailable;
            readiness.reason = "identity_drift".into();
            return readiness;
        }
        let Some(observation) = &readiness.observation else {
            readiness.state = ProviderReadinessStateV1::Unknown;
            readiness.reason = "observation_missing".into();
            return readiness;
        };
        if observation
            .observed_at_unix_ms
            .saturating_add(observation.fresh_for_ms)
            < self.now_unix_ms
        {
            readiness.state = ProviderReadinessStateV1::Unavailable;
            readiness.reason = "readiness_stale".into();
            return readiness;
        }
        if readiness.state == ProviderReadinessStateV1::Ready
            && !matches!(readiness.test_query, Some(ref query) if query.succeeded)
        {
            readiness.state = ProviderReadinessStateV1::Degraded;
            readiness.reason = "test_query_failed".into();
        }
        readiness
    }
}

pub fn provider_readiness_hub_read(readiness: ProviderReadinessV1) -> HubReadV1 {
    let reason = readiness.reason.clone();
    let item = serde_json::to_value(readiness).expect("provider readiness serializes");
    match item["state"].as_str() {
        Some("ready") => HubReadV1::Available {
            items: vec![item],
            metadata: HubMetadataV1::default(),
        },
        Some("degraded") => HubReadV1::Degraded {
            reason,
            items: vec![item],
            metadata: HubMetadataV1::default(),
        },
        _ => HubReadV1::Unavailable { reason },
    }
}
pub fn scratchpad_hub_read(session: &str) -> HubReadV1 {
    match crate::scratchpad::hub_summary(session) {
        Ok(item) => HubReadV1::Available {
            items: vec![item],
            metadata: HubMetadataV1::default(),
        },
        Err(reason) => HubReadV1::Unavailable { reason },
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct HubMetadataV1 {
    pub resolver: Option<String>,
    pub source: Option<String>,
    pub evidence: Option<String>,
    pub observed_at_unix_ms: u64,
    pub cache_age_ms: u64,
}

impl HubMetadataV1 {
    fn normalized(mut self) -> Self {
        self.resolver = self.resolver.filter(|value| !value.is_empty());
        self.source = self.source.filter(|value| !value.is_empty());
        self.evidence = self.evidence.filter(|value| !value.is_empty());
        self
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum HubReadV1 {
    Available {
        items: Vec<Value>,
        metadata: HubMetadataV1,
    },
    Degraded {
        reason: String,
        items: Vec<Value>,
        metadata: HubMetadataV1,
    },
    Unavailable {
        reason: String,
    },
}

impl HubReadV1 {
    fn section(self) -> HubSectionV1 {
        match self {
            Self::Available { items, metadata } => {
                let metadata = metadata.normalized();
                HubSectionV1 {
                    state: HubStateV1::Available,
                    reason: "observed".into(),
                    items: Some(items),
                    resolver: metadata.resolver,
                    evidence: metadata.evidence,
                    observed_at_unix_ms: (metadata.observed_at_unix_ms > 0)
                        .then_some(metadata.observed_at_unix_ms),
                }
            }
            Self::Degraded {
                reason,
                items,
                metadata,
            } => {
                let metadata = metadata.normalized();
                HubSectionV1 {
                    state: HubStateV1::Degraded,
                    reason: if reason.is_empty() {
                        "reason_unavailable".into()
                    } else {
                        reason
                    },
                    items: Some(items),
                    resolver: metadata.resolver,
                    evidence: metadata.evidence,
                    observed_at_unix_ms: (metadata.observed_at_unix_ms > 0)
                        .then_some(metadata.observed_at_unix_ms),
                }
            }
            Self::Unavailable { reason } => HubSectionV1::unavailable(reason),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct HubInputsV1 {
    pub deliveries: HubReadV1,
    pub providers: HubReadV1,
    pub repositories: HubReadV1,
    pub adapters: HubReadV1,
    pub devices: HubReadV1,
    pub memory: HubReadV1,
    pub sentinel: HubReadV1,
    pub alerts: HubReadV1,
}

impl HubInputsV1 {
    pub fn unavailable(reason: &str) -> Self {
        let unavailable = || HubReadV1::Unavailable {
            reason: reason.into(),
        };
        Self {
            deliveries: unavailable(),
            providers: unavailable(),
            repositories: unavailable(),
            adapters: unavailable(),
            devices: unavailable(),
            memory: unavailable(),
            sentinel: unavailable(),
            alerts: unavailable(),
        }
    }
}

/// Six semantic Membrane subsystems — distinct from the eight operational
/// Hub resources. Each reports Available/Degraded/Unavailable/Not configured
/// independently.
#[derive(Debug, Clone, PartialEq)]
pub struct HubSubsystemInputsV1 {
    pub pull: HubReadV1,
    pub push: HubReadV1,
    pub cortex: HubReadV1,
    pub blueprint: HubReadV1,
    pub guide: HubReadV1,
    pub adapt: HubReadV1,
}

impl HubSubsystemInputsV1 {
    pub fn unavailable(reason: &str) -> Self {
        let unavailable = || HubReadV1::Unavailable {
            reason: reason.into(),
        };
        Self {
            pull: unavailable(),
            push: unavailable(),
            cortex: unavailable(),
            blueprint: unavailable(),
            guide: unavailable(),
            adapt: unavailable(),
        }
    }

    /// Map to the wire representation consumed by `HubSnapshotV1::subsystems`.
    pub fn sections(&self) -> BTreeMap<String, HubSectionV1> {
        BTreeMap::from([
            ("pull".into(), self.pull.clone().section()),
            ("push".into(), self.push.clone().section()),
            ("cortex".into(), self.cortex.clone().section()),
            ("blueprint".into(), self.blueprint.clone().section()),
            ("guide".into(), self.guide.clone().section()),
            ("adapt".into(), self.adapt.clone().section()),
        ])
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HubFacadeV1 {
    stream: Option<HubStreamV1>,
}

impl HubFacadeV1 {
    pub fn dispatch_json(
        &self,
        operation: &str,
        observed_at_unix_ms: u64,
        inputs: HubInputsV1,
    ) -> Result<Value, String> {
        match operation {
            "hub.capabilities" => {
                serde_json::to_value(self.capabilities()).map_err(|e| e.to_string())
            }
            "hub.snapshot" => serde_json::to_value(self.snapshot(observed_at_unix_ms, inputs))
                .map_err(|e| e.to_string()),
            _ => Err("hub_operation_unavailable".into()),
        }
    }
    pub fn new(stream: Option<HubStreamV1>) -> Self {
        Self {
            stream: stream.map(|mut value| {
                if value.reason.is_empty() {
                    value.reason = "reason_unavailable".into();
                }
                if value.resolver.as_deref() == Some("") {
                    value.resolver = None;
                }
                value
            }),
        }
    }

    pub fn capabilities(&self) -> HubCapabilitiesV1 {
        HubCapabilitiesV1 {
            schema_version: HUB_SCHEMA_VERSION,
            read_only: true,
            resources: HUB_RESOURCES.iter().map(|value| (*value).into()).collect(),
            operations: HUB_OPERATIONS.iter().map(|value| (*value).into()).collect(),
            installation_id: "unavailable".into(),
            service_id: "unavailable".into(),
            release_generation: "unavailable".into(),
            data_root_digest: "unavailable".into(),
            stream: self.stream.clone(),
        }
    }

    pub fn snapshot(&self, observed_at_unix_ms: u64, inputs: HubInputsV1) -> HubSnapshotV1 {
        self.snapshot_with_subsystems(observed_at_unix_ms, inputs, None, None)
    }

    pub fn snapshot_with_subsystems(
        &self,
        observed_at_unix_ms: u64,
        inputs: HubInputsV1,
        membrane_state: Option<String>,
        subsystems: Option<BTreeMap<String, HubSectionV1>>,
    ) -> HubSnapshotV1 {
        HubSnapshotV1 {
            schema_version: HUB_SCHEMA_VERSION,
            product_id: "membrane".into(),
            observed_at_unix_ms,
            sections: BTreeMap::from([
                ("deliveries".into(), inputs.deliveries.section()),
                ("providers".into(), inputs.providers.section()),
                ("repositories".into(), inputs.repositories.section()),
                ("adapters".into(), inputs.adapters.section()),
                ("devices".into(), inputs.devices.section()),
                ("memory".into(), inputs.memory.section()),
                ("sentinel".into(), inputs.sentinel.section()),
                ("alerts".into(), inputs.alerts.section()),
            ]),
            membrane_state,
            subsystems,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn identity(provider: &str) -> ProviderIdentityV1 {
        ProviderIdentityV1 {
            provider_id: provider.into(),
            installation_id: "installation".into(),
            service_id: "service".into(),
            release_generation: "release".into(),
            data_root_digest: "root".into(),
        }
    }

    fn readiness(state: ProviderReadinessStateV1) -> ProviderReadinessV1 {
        ProviderReadinessV1 {
            schema_version: 1,
            state,
            identity: identity("provider"),
            observation: Some(membrane_protocol::ProviderObservationV1 {
                observed_at_unix_ms: 100,
                fresh_for_ms: 50,
            }),
            test_query: Some(membrane_protocol::ProviderTestQueryV1 {
                name: "smoke".into(),
                succeeded: true,
            }),
            reason: "authoritative".into(),
        }
    }

    #[test]
    fn readiness_requires_authoritative_fresh_identity_bound_evidence() {
        let adapter = ProviderReadinessAdapterV1 {
            expected_identity: identity("provider"),
            now_unix_ms: 125,
        };
        for state in [
            ProviderReadinessStateV1::Ready,
            ProviderReadinessStateV1::Degraded,
            ProviderReadinessStateV1::Unavailable,
            ProviderReadinessStateV1::Unknown,
        ] {
            assert_eq!(adapter.evaluate(Some(readiness(state)), false).state, state);
        }
        let mut drift = readiness(ProviderReadinessStateV1::Ready);
        drift.identity.provider_id = "foreign".into();
        assert_eq!(
            adapter.evaluate(Some(drift), true).state,
            ProviderReadinessStateV1::Unavailable
        );
        let stale_adapter = ProviderReadinessAdapterV1 {
            now_unix_ms: 151,
            ..adapter.clone()
        };
        let stale = stale_adapter.evaluate(Some(readiness(ProviderReadinessStateV1::Ready)), true);
        assert_eq!(stale.reason, "readiness_stale");
        let missing = adapter.evaluate(None, true);
        assert_eq!(missing.state, ProviderReadinessStateV1::Unknown);
        assert_eq!(missing.reason, "process_exists_without_readiness");
        for (state, rank) in [
            (ProviderReadinessStateV1::Ready, 0),
            (ProviderReadinessStateV1::Degraded, 1),
            (ProviderReadinessStateV1::Unavailable, 2),
            (ProviderReadinessStateV1::Unknown, 2),
        ] {
            let read = provider_readiness_hub_read(readiness(state));
            assert!(matches!(
                (rank, read),
                (0, HubReadV1::Available { .. })
                    | (1, HubReadV1::Degraded { .. })
                    | (2, HubReadV1::Unavailable { .. })
            ));
        }
    }

    #[test]
    fn facade_preserves_observed_truth_without_inferred_liveness() {
        let facade = HubFacadeV1::new(Some(HubStreamV1 {
            state: HubStateV1::Unavailable,
            reason: "stream_not_configured".into(),
            resolver: Some(String::new()),
        }));
        let mut inputs = HubInputsV1::unavailable("source_not_connected");
        inputs.deliveries = HubReadV1::Available {
            items: vec![json!({"receiptId":"delivery-1"})],
            metadata: HubMetadataV1 {
                resolver: Some(String::new()),
                source: Some(String::new()),
                evidence: Some(String::new()),
                ..HubMetadataV1::default()
            },
        };
        inputs.providers = HubReadV1::Degraded {
            reason: "readiness_handle_missing".into(),
            items: vec![json!({"provider":"blueprint","status":"unknown"})],
            metadata: HubMetadataV1::default(),
        };
        let snapshot = facade.snapshot(42, inputs);
        assert_eq!(snapshot.sections["deliveries"].state, HubStateV1::Available);
        assert_eq!(snapshot.sections["providers"].state, HubStateV1::Degraded);
        assert_eq!(snapshot.sections["adapters"].state, HubStateV1::Unavailable);
        assert!(snapshot.sections["adapters"].items.is_none());
        let capabilities = facade.capabilities();
        assert!(capabilities.read_only);
        assert_eq!(capabilities.resources, HUB_RESOURCES);
        let stream = capabilities.stream.unwrap();
        assert_eq!(stream.state, HubStateV1::Unavailable);
        assert!(stream.resolver.is_none());
        let encoded = serde_json::to_value(snapshot).unwrap();
        assert_eq!(encoded["productId"], "membrane");
        assert!(encoded["sections"]["deliveries"].get("resolver").is_none());
        assert!(encoded["sections"]["deliveries"].get("source").is_none());
        assert!(encoded["sections"]["deliveries"].get("evidence").is_none());
        assert_eq!(encoded["schemaVersion"], 1);
        let snapshot_fields = encoded.as_object().unwrap();
        assert_eq!(snapshot_fields.len(), 4);
        for field in ["schemaVersion", "productId", "observedAtUnixMs", "sections"] {
            assert!(snapshot_fields.contains_key(field));
        }
    }
    #[test]
    fn scratchpad_hub_is_content_free_and_validates_scope() {
        assert!(
            matches!(scratchpad_hub_read("s"), HubReadV1::Available { ref items, .. } if items[0]["entries"] == 0)
        );
        assert!(matches!(
            scratchpad_hub_read(""),
            HubReadV1::Unavailable { .. }
        ));
    }
}
