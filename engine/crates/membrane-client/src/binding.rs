//! Canonical installed-runtime discovery and binding semantics for hosts.
//!
//! This module owns the public state machine, not sockets, process launch, or
//! installer execution. Hosts supply discovery/transport effects and consume
//! these typed outcomes without inventing fallback policy.

use crate::{ClientError, CompatibilityRequirement, ServiceIdentity};
use membrane_protocol::ResidentEndpointV1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownCandidate {
    pub stable_install_root: String,
    pub endpoint: ResidentEndpointV1,
    pub expected_installation_id: Option<String>,
    pub expected_startup_generation: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalBinding {
    pub candidate: KnownCandidate,
    pub identity: ServiceIdentity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscoveryKind {
    NotFound,
    OfflineKnown,
    Incompatible,
    Denied,
    CorruptOrRotation,
    TimeoutKnown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiscoveryOutcome {
    Compatible(CanonicalBinding),
    NotFound,
    OfflineKnown { candidate: KnownCandidate, message: String },
    Incompatible { candidate: KnownCandidate, message: String },
    Denied { candidate: KnownCandidate, message: String },
    CorruptOrRotation { candidate: KnownCandidate, message: String },
    TimeoutKnown { candidate: KnownCandidate, message: String },
}

impl DiscoveryOutcome {
    pub fn kind(&self) -> Option<DiscoveryKind> {
        match self {
            Self::Compatible(_) => None,
            Self::NotFound => Some(DiscoveryKind::NotFound),
            Self::OfflineKnown { .. } => Some(DiscoveryKind::OfflineKnown),
            Self::Incompatible { .. } => Some(DiscoveryKind::Incompatible),
            Self::Denied { .. } => Some(DiscoveryKind::Denied),
            Self::CorruptOrRotation { .. } => Some(DiscoveryKind::CorruptOrRotation),
            Self::TimeoutKnown { .. } => Some(DiscoveryKind::TimeoutKnown),
        }
    }

    pub fn candidate(&self) -> Option<&KnownCandidate> {
        match self {
            Self::Compatible(binding) => Some(&binding.candidate),
            Self::NotFound => None,
            Self::OfflineKnown { candidate, .. }
            | Self::Incompatible { candidate, .. }
            | Self::Denied { candidate, .. }
            | Self::CorruptOrRotation { candidate, .. }
            | Self::TimeoutKnown { candidate, .. } => Some(candidate),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnsureAction {
    Adopt,
    ProvisionPackaged,
    Refuse(DiscoveryKind),
}

/// Decide what a host may do after discovery.
///
/// Provisioning is legal only after a proven absence. A known candidate that
/// is offline, denied, incompatible, corrupt/rotating, or timed out is never
/// replaced by a second installation.
pub fn ensure_action(outcome: &DiscoveryOutcome) -> EnsureAction {
    match outcome {
        DiscoveryOutcome::Compatible(_) => EnsureAction::Adopt,
        DiscoveryOutcome::NotFound => EnsureAction::ProvisionPackaged,
        other => EnsureAction::Refuse(other.kind().expect("non-compatible outcome")),
    }
}

pub fn bind_candidate(
    candidate: KnownCandidate,
    identity: ServiceIdentity,
    requirement: &CompatibilityRequirement,
) -> Result<CanonicalBinding, ClientError> {
    if identity.runtime_origin != "installed" {
        return Err(ClientError::Incompatible {
            message: "canonical binding requires installed runtime origin".into(),
        });
    }
    if identity.stable_install_root.as_deref() != Some(candidate.stable_install_root.as_str()) {
        return Err(ClientError::Incompatible {
            message: "health stable install root does not match discovered candidate".into(),
        });
    }
    if let Some(expected) = candidate.expected_installation_id.as_deref() {
        if identity.installation_id != expected {
            return Err(ClientError::Incompatible {
                message: "discovered installation identity changed".into(),
            });
        }
    }
    if let Some(expected) = candidate.expected_startup_generation {
        if identity.startup_generation != expected {
            return Err(ClientError::Incompatible {
                message: "discovered startup generation changed".into(),
            });
        }
    }
    if identity.protocol_version != requirement.protocol_version
        || identity.schema_version != requirement.schema_version
    {
        return Err(ClientError::Incompatible {
            message: "bound service protocol/schema changed after discovery".into(),
        });
    }
    if let Some(expected) = requirement.installation_id.as_deref() {
        if identity.installation_id != expected {
            return Err(ClientError::Incompatible {
                message: "bound installation identity does not match requirement".into(),
            });
        }
    }
    if let Some(expected) = requirement.cortex_store_id.as_deref() {
        if identity.cortex_store_id != expected {
            return Err(ClientError::Incompatible {
                message: "bound Cortex store identity does not match requirement".into(),
            });
        }
    }
    Ok(CanonicalBinding { candidate, identity })
}

pub fn classify_known_candidate(
    candidate: KnownCandidate,
    result: Result<ServiceIdentity, ClientError>,
    requirement: &CompatibilityRequirement,
) -> DiscoveryOutcome {
    match result.and_then(|identity| bind_candidate(candidate.clone(), identity, requirement)) {
        Ok(binding) => DiscoveryOutcome::Compatible(binding),
        Err(ClientError::BackendUnavailable { message })
        | Err(ClientError::Unavailable { message })
        | Err(ClientError::Transport { message }) => {
            DiscoveryOutcome::OfflineKnown { candidate, message }
        }
        Err(ClientError::Timeout { message }) => DiscoveryOutcome::TimeoutKnown { candidate, message },
        Err(ClientError::Incompatible { message }) => DiscoveryOutcome::Incompatible { candidate, message },
        Err(ClientError::Denied { message }) => DiscoveryOutcome::Denied { candidate, message },
        Err(ClientError::CorruptOrRotation { message }) => {
            DiscoveryOutcome::CorruptOrRotation { candidate, message }
        }
        Err(error) => DiscoveryOutcome::CorruptOrRotation {
            candidate,
            message: error.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate() -> KnownCandidate {
        KnownCandidate {
            stable_install_root: r"C:\Users\test\AppData\Local\Orthic Labs\Membrane\current".into(),
            endpoint: ResidentEndpointV1 { host: "127.0.0.1".into(), port: 47_851 },
            expected_installation_id: Some("install-1".into()),
            expected_startup_generation: Some(7),
        }
    }

    fn identity() -> ServiceIdentity {
        ServiceIdentity {
            service_id: "membrane-hub".into(),
            installation_id: "install-1".into(),
            cortex_store_id: "store-1".into(),
            release_generation: "release-1".into(),
            startup_generation: 7,
            runtime_origin: "installed".into(),
            stable_install_root: Some(candidate().stable_install_root),
            protocol_version: 1,
            schema_version: 1,
            native_only: true,
            subsystems: ["pull", "push", "cortex", "blueprint", "ledger", "adapt"]
                .into_iter().map(str::to_owned).collect(),
            capabilities: vec!["memory".into(), "diagnostics".into()],
        }
    }

    #[test]
    fn only_proven_not_found_allows_packaged_provisioning() {
        assert_eq!(ensure_action(&DiscoveryOutcome::NotFound), EnsureAction::ProvisionPackaged);
        let known = candidate();
        for outcome in [
            DiscoveryOutcome::OfflineKnown { candidate: known.clone(), message: "off".into() },
            DiscoveryOutcome::Incompatible { candidate: known.clone(), message: "bad".into() },
            DiscoveryOutcome::Denied { candidate: known.clone(), message: "no".into() },
            DiscoveryOutcome::CorruptOrRotation { candidate: known.clone(), message: "rotating".into() },
            DiscoveryOutcome::TimeoutKnown { candidate: known.clone(), message: "slow".into() },
        ] {
            assert!(matches!(ensure_action(&outcome), EnsureAction::Refuse(_)));
        }
    }

    #[test]
    fn canonical_binding_rejects_reused_endpoint_for_other_installation() {
        let mut other = identity();
        other.installation_id = "install-2".into();
        let outcome = classify_known_candidate(candidate(), Ok(other), &CompatibilityRequirement::default());
        assert!(matches!(outcome, DiscoveryOutcome::Incompatible { .. }));
    }

    #[test]
    fn changed_endpoint_can_rebind_only_after_identity_verification() {
        let mut moved = candidate();
        moved.endpoint.port = 47_852;
        let outcome = classify_known_candidate(moved.clone(), Ok(identity()), &CompatibilityRequirement::default());
        let DiscoveryOutcome::Compatible(binding) = outcome else { panic!("expected compatible rebind") };
        assert_eq!(binding.candidate.endpoint, moved.endpoint);
        assert_eq!(binding.identity.installation_id, "install-1");
    }

    #[test]
    fn new_startup_generation_invalidates_stale_candidate_then_revalidates() {
        let mut restarted = identity();
        restarted.startup_generation = 8;
        let stale = classify_known_candidate(candidate(), Ok(restarted.clone()), &CompatibilityRequirement::default());
        assert!(matches!(stale, DiscoveryOutcome::Incompatible { .. }));
        let mut refreshed = candidate();
        refreshed.expected_startup_generation = Some(8);
        assert!(matches!(classify_known_candidate(refreshed, Ok(restarted), &CompatibilityRequirement::default()), DiscoveryOutcome::Compatible(_)));
    }
}
