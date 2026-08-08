use membrane_protocol::{
    HubCapabilitiesV1, HubSectionV1, HubSnapshotV1, HubStateV1, HubStreamV1, HUB_SCHEMA_VERSION,
};
use serde_json::Value;

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
                    items,
                    resolver: metadata.resolver,
                    source: metadata.source,
                    evidence: metadata.evidence,
                    observed_at_unix_ms: metadata.observed_at_unix_ms,
                    cache_age_ms: metadata.cache_age_ms,
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
                    items,
                    resolver: metadata.resolver,
                    source: metadata.source,
                    evidence: metadata.evidence,
                    observed_at_unix_ms: metadata.observed_at_unix_ms,
                    cache_age_ms: metadata.cache_age_ms,
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
        HubSnapshotV1 {
            schema_version: HUB_SCHEMA_VERSION,
            observed_at_unix_ms,
            deliveries: inputs.deliveries.section(),
            providers: inputs.providers.section(),
            repositories: inputs.repositories.section(),
            adapters: inputs.adapters.section(),
            devices: inputs.devices.section(),
            memory: inputs.memory.section(),
            sentinel: inputs.sentinel.section(),
            alerts: inputs.alerts.section(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
            items: vec![json!({"provider":"cortex","status":"unknown"})],
            metadata: HubMetadataV1::default(),
        };
        let snapshot = facade.snapshot(42, inputs);
        assert_eq!(snapshot.deliveries.state, HubStateV1::Available);
        assert_eq!(snapshot.providers.state, HubStateV1::Degraded);
        assert_eq!(snapshot.adapters.state, HubStateV1::Unavailable);
        assert!(snapshot.adapters.items.is_empty());
        let capabilities = facade.capabilities();
        assert!(capabilities.read_only);
        assert_eq!(capabilities.resources, HUB_RESOURCES);
        let stream = capabilities.stream.unwrap();
        assert_eq!(stream.state, HubStateV1::Unavailable);
        assert!(stream.resolver.is_none());
        let encoded = serde_json::to_value(snapshot).unwrap();
        assert!(encoded["deliveries"]["resolver"].is_null());
        assert!(encoded["deliveries"]["source"].is_null());
        assert!(encoded["deliveries"]["evidence"].is_null());
        assert_eq!(encoded["schemaVersion"], 1);
    }
}
