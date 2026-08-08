use serde::{Deserialize, Serialize};

pub const PROVIDER_READINESS_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderReadinessStateV1 {
    Ready,
    Degraded,
    Unavailable,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderIdentityV1 {
    pub provider_id: String,
    pub installation_id: String,
    pub service_id: String,
    pub release_generation: String,
    pub data_root_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderObservationV1 {
    pub observed_at_unix_ms: u64,
    pub fresh_for_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderTestQueryV1 {
    pub name: String,
    pub succeeded: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderReadinessV1 {
    pub schema_version: u32,
    pub state: ProviderReadinessStateV1,
    pub identity: ProviderIdentityV1,
    pub observation: Option<ProviderObservationV1>,
    pub test_query: Option<ProviderTestQueryV1>,
    pub reason: String,
}

impl ProviderReadinessV1 {
    pub fn unknown(identity: ProviderIdentityV1, reason: impl Into<String>) -> Self {
        Self {
            schema_version: PROVIDER_READINESS_SCHEMA_VERSION,
            state: ProviderReadinessStateV1::Unknown,
            identity,
            observation: None,
            test_query: None,
            reason: reason.into(),
        }
    }

    pub fn contract_error(&self) -> Option<&'static str> {
        if self.schema_version != PROVIDER_READINESS_SCHEMA_VERSION {
            return Some("schema_version_mismatch");
        }
        if self.reason.is_empty()
            || self.identity.provider_id.is_empty()
            || self.identity.installation_id.is_empty()
            || self.identity.service_id.is_empty()
            || self.identity.release_generation.is_empty()
            || self.identity.data_root_digest.is_empty()
        {
            return Some("required_identity_or_reason_missing");
        }
        if let Some(query) = &self.test_query {
            if query.name.is_empty() {
                return Some("test_query_name_missing");
            }
        }
        if matches!(self.observation, Some(ref observation) if observation.fresh_for_ms == 0) {
            return Some("observation_freshness_invalid");
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golden_round_trips_and_all_states_are_distinct() {
        let raw =
            include_str!("../../../../tests/provider-readiness/provider-readiness.v1.golden.json");
        let value: ProviderReadinessV1 = serde_json::from_str(raw).unwrap();
        assert_eq!(value.contract_error(), None);
        assert_eq!(value.state, ProviderReadinessStateV1::Ready);
        let encoded = serde_json::to_value(value).unwrap();
        assert_eq!(
            encoded,
            serde_json::from_str::<serde_json::Value>(raw).unwrap()
        );
        assert_eq!(
            serde_json::to_value([
                ProviderReadinessStateV1::Ready,
                ProviderReadinessStateV1::Degraded,
                ProviderReadinessStateV1::Unavailable,
                ProviderReadinessStateV1::Unknown,
            ])
            .unwrap(),
            serde_json::json!(["ready", "degraded", "unavailable", "unknown"])
        );
    }
}
