use crate::{AdapterStatus, HubStateV1, InstallationManifestV1};
use serde::{Deserialize, Serialize};

pub const MULTIDIMENSIONAL_STATUS_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallationReasonV1 {
    Verified,
    EvidenceUnavailable,
    WrongInstallation,
    IncompatibleSchema,
    UnexpectedDataRoot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryReasonV1 {
    Delivered,
    NotSelected,
    SelectedWithoutDelivery,
    EvidenceUnavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InstallationStatusV1 {
    pub state: HubStateV1,
    pub reason: InstallationReasonV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AdapterLifecycleStatusV1 {
    pub state: HubStateV1,
    pub status: Option<AdapterStatus>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeliveryStatusV1 {
    pub state: HubStateV1,
    pub reason: DeliveryReasonV1,
    pub selected: Option<u64>,
    pub delivered: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alert: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MultidimensionalStatusV1 {
    pub schema_version: u32,
    pub state: HubStateV1,
    pub installation: InstallationStatusV1,
    pub adapter: AdapterLifecycleStatusV1,
    pub delivery: DeliveryStatusV1,
}

impl MultidimensionalStatusV1 {
    pub fn evaluate(
        active: Option<&InstallationManifestV1>,
        observed: Option<&InstallationManifestV1>,
        adapter_status: Option<AdapterStatus>,
        selected: Option<u64>,
        delivered: Option<u64>,
    ) -> Self {
        let installation = match (active, observed) {
            (Some(active), Some(observed)) => {
                let (state, reason) =
                    match InstallationManifestV1::verify_handshake(active, observed) {
                        Ok(()) => (HubStateV1::Available, InstallationReasonV1::Verified),
                        Err(crate::HandshakeError::WrongInstallation { .. }) => (
                            HubStateV1::Unavailable,
                            InstallationReasonV1::WrongInstallation,
                        ),
                        Err(crate::HandshakeError::IncompatibleSchema { .. }) => (
                            HubStateV1::Unavailable,
                            InstallationReasonV1::IncompatibleSchema,
                        ),
                        Err(crate::HandshakeError::UnexpectedDataRoot { .. }) => (
                            HubStateV1::Unavailable,
                            InstallationReasonV1::UnexpectedDataRoot,
                        ),
                    };
                InstallationStatusV1 { state, reason }
            }
            _ => InstallationStatusV1 {
                state: HubStateV1::Unavailable,
                reason: InstallationReasonV1::EvidenceUnavailable,
            },
        };
        let adapter = AdapterLifecycleStatusV1 {
            state: match adapter_status {
                Some(AdapterStatus::Delivering) => HubStateV1::Available,
                Some(AdapterStatus::Installed | AdapterStatus::Active) => HubStateV1::Degraded,
                Some(AdapterStatus::Stale) | None => HubStateV1::Unavailable,
            },
            status: adapter_status,
        };
        let delivery = match (selected, delivered) {
            (Some(selected), Some(delivered)) if selected > 0 && delivered == 0 => {
                DeliveryStatusV1 {
                    state: HubStateV1::Degraded,
                    reason: DeliveryReasonV1::SelectedWithoutDelivery,
                    selected: Some(selected),
                    delivered: Some(delivered),
                    alert: Some("selected_without_delivery".into()),
                }
            }
            (Some(0), Some(delivered)) => DeliveryStatusV1 {
                state: HubStateV1::Available,
                reason: DeliveryReasonV1::NotSelected,
                selected: Some(0),
                delivered: Some(delivered),
                alert: None,
            },
            (Some(selected), Some(delivered)) => DeliveryStatusV1 {
                state: HubStateV1::Available,
                reason: DeliveryReasonV1::Delivered,
                selected: Some(selected),
                delivered: Some(delivered),
                alert: None,
            },
            _ => DeliveryStatusV1 {
                state: HubStateV1::Unavailable,
                reason: DeliveryReasonV1::EvidenceUnavailable,
                selected,
                delivered,
                alert: None,
            },
        };
        let state = [installation.state, adapter.state, delivery.state]
            .into_iter()
            .max_by_key(|state| match state {
                HubStateV1::Available => 0,
                HubStateV1::Degraded => 1,
                HubStateV1::Unavailable => 2,
            })
            .expect("three status dimensions");
        Self {
            schema_version: MULTIDIMENSIONAL_STATUS_SCHEMA_VERSION,
            state,
            installation,
            adapter,
            delivery,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::BTreeMap, path::Path};

    fn manifest(id: &str) -> InstallationManifestV1 {
        InstallationManifestV1::new(
            id,
            1,
            Path::new("/membrane"),
            "sha256:release",
            "test-platform",
            "2026-08-08T00:00:00Z",
            BTreeMap::new(),
            BTreeMap::new(),
        )
    }

    #[test]
    fn wrong_installation_and_selected_without_delivery_never_render_healthy() {
        let active = manifest("active");
        let foreign = manifest("foreign");
        assert_eq!(
            MultidimensionalStatusV1::evaluate(
                Some(&active),
                Some(&foreign),
                Some(AdapterStatus::Delivering),
                Some(3),
                Some(3),
            )
            .state,
            HubStateV1::Unavailable
        );
        assert_eq!(
            MultidimensionalStatusV1::evaluate(
                Some(&active),
                Some(&active),
                Some(AdapterStatus::Delivering),
                Some(3),
                Some(0),
            )
            .state,
            HubStateV1::Degraded
        );
        let status = MultidimensionalStatusV1::evaluate(
            Some(&active),
            Some(&foreign),
            Some(AdapterStatus::Delivering),
            Some(3),
            Some(0),
        );
        assert_eq!(status.state, HubStateV1::Unavailable);
        assert_eq!(
            status.installation.reason,
            InstallationReasonV1::WrongInstallation
        );
        assert_eq!(
            status.delivery.reason,
            DeliveryReasonV1::SelectedWithoutDelivery
        );
        let golden: MultidimensionalStatusV1 = serde_json::from_str(include_str!(
            "../../../../tests/status/multidimensional-status.v1.golden.json"
        ))
        .unwrap();
        assert_eq!(status, golden);
    }

    #[test]
    fn missing_evidence_is_unavailable_and_complete_evidence_can_be_healthy() {
        assert_eq!(
            MultidimensionalStatusV1::evaluate(None, None, None, None, None).state,
            HubStateV1::Unavailable
        );
        let active = manifest("active");
        assert_eq!(
            MultidimensionalStatusV1::evaluate(
                Some(&active),
                Some(&active),
                Some(AdapterStatus::Delivering),
                Some(1),
                Some(1),
            )
            .state,
            HubStateV1::Available
        );
    }
}
