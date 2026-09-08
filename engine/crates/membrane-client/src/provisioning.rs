//! Host-injected orchestration for canonical installed-runtime readiness.

use crate::binding::comparable_stable_root;
use crate::{
    ensure_action, CanonicalBinding, ClientError, DiscoveryKind, DiscoveryOutcome, EnsureAction,
    KnownCandidate,
};
use std::fmt;
use std::time::Instant;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateContinuity {
    pub stable_install_root: String,
    pub installation_id: String,
    pub cortex_store_id: String,
}

impl From<&CanonicalBinding> for UpdateContinuity {
    fn from(binding: &CanonicalBinding) -> Self {
        Self {
            stable_install_root: binding.candidate.stable_install_root.clone(),
            installation_id: binding.identity.installation_id.clone(),
            cortex_store_id: binding.identity.cortex_store_id.clone(),
        }
    }
}

pub trait InstalledRuntimeEffects {
    fn discover(&mut self, deadline: Instant) -> Result<DiscoveryOutcome, ClientError>;

    fn provision_packaged(&mut self, deadline: Instant) -> Result<(), ClientError>;

    fn update_repair_known(
        &mut self,
        candidate: &KnownCandidate,
        deadline: Instant,
    ) -> Result<(), ClientError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnsureInstalledError {
    DeadlineExpired,
    Discovery(ClientError),
    Effect {
        action: EnsureAction,
        error: ClientError,
    },
    Refused(DiscoveryKind),
    PostEffect(DiscoveryKind),
    UpdateContinuityRequired,
    ContinuityChanged(&'static str),
}

impl fmt::Display for EnsureInstalledError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DeadlineExpired => f.write_str("installed-runtime readiness deadline expired"),
            Self::Discovery(error) => write!(f, "installed-runtime discovery failed: {error}"),
            Self::Effect { action, error } => {
                write!(f, "installed-runtime {action:?} effect failed: {error}")
            }
            Self::Refused(kind) => write!(f, "installed-runtime mutation refused for {kind:?}"),
            Self::PostEffect(kind) => {
                write!(f, "installed runtime remained {kind:?} after canonical effect")
            }
            Self::UpdateContinuityRequired => {
                f.write_str("known update/repair requires prior installation continuity")
            }
            Self::ContinuityChanged(field) => {
                write!(f, "known update/repair changed {field}")
            }
        }
    }
}

impl std::error::Error for EnsureInstalledError {}

/// Reach one compatible installer-owned runtime through one bounded effect.
///
/// Callers own discovery and canonical installer execution. This state machine
/// supplies no process launcher, fallback path, retry loop, or second runtime
/// authority. Every mutation is followed by fresh discovery before success.
pub fn ensure_installed_runtime<E: InstalledRuntimeEffects>(
    effects: &mut E,
    deadline: Instant,
    update_continuity: Option<&UpdateContinuity>,
) -> Result<CanonicalBinding, EnsureInstalledError> {
    require_time(deadline)?;
    let initial = effects
        .discover(deadline)
        .map_err(EnsureInstalledError::Discovery)?;

    match ensure_action(&initial) {
        EnsureAction::Adopt => compatible(initial),
        EnsureAction::Refuse(kind) => Err(EnsureInstalledError::Refused(kind)),
        EnsureAction::ProvisionPackaged => {
            require_time(deadline)?;
            effects
                .provision_packaged(deadline)
                .map_err(|error| EnsureInstalledError::Effect {
                    action: EnsureAction::ProvisionPackaged,
                    error,
                })?;
            rediscover(effects, deadline)
        }
        EnsureAction::UpdateRepairKnown => {
            let candidate = initial
                .candidate()
                .expect("known update/repair has candidate");
            let continuity = update_continuity
                .ok_or(EnsureInstalledError::UpdateContinuityRequired)?;
            verify_pre_update_continuity(candidate, continuity)?;
            require_time(deadline)?;
            effects
                .update_repair_known(candidate, deadline)
                .map_err(|error| EnsureInstalledError::Effect {
                    action: EnsureAction::UpdateRepairKnown,
                    error,
                })?;
            let binding = rediscover(effects, deadline)?;
            verify_post_update_continuity(candidate, continuity, &binding)?;
            Ok(binding)
        }
    }
}

fn require_time(deadline: Instant) -> Result<(), EnsureInstalledError> {
    if Instant::now() >= deadline {
        Err(EnsureInstalledError::DeadlineExpired)
    } else {
        Ok(())
    }
}

fn rediscover<E: InstalledRuntimeEffects>(
    effects: &mut E,
    deadline: Instant,
) -> Result<CanonicalBinding, EnsureInstalledError> {
    require_time(deadline)?;
    let outcome = effects
        .discover(deadline)
        .map_err(EnsureInstalledError::Discovery)?;
    match outcome {
        DiscoveryOutcome::Compatible(binding) => Ok(binding),
        other => Err(EnsureInstalledError::PostEffect(
            other.kind().expect("non-compatible post-effect outcome"),
        )),
    }
}

fn compatible(outcome: DiscoveryOutcome) -> Result<CanonicalBinding, EnsureInstalledError> {
    match outcome {
        DiscoveryOutcome::Compatible(binding) => Ok(binding),
        _ => unreachable!("Adopt is emitted only for compatible discovery"),
    }
}

fn verify_pre_update_continuity(
    candidate: &KnownCandidate,
    continuity: &UpdateContinuity,
) -> Result<(), EnsureInstalledError> {
    if comparable_stable_root(&candidate.stable_install_root)
        != comparable_stable_root(&continuity.stable_install_root)
    {
        return Err(EnsureInstalledError::ContinuityChanged(
            "stable install root before effect",
        ));
    }
    if candidate
        .expected_installation_id
        .as_deref()
        .is_some_and(|expected| expected != continuity.installation_id)
    {
        return Err(EnsureInstalledError::ContinuityChanged(
            "installation identity before effect",
        ));
    }
    if continuity.installation_id.trim().is_empty() || continuity.cortex_store_id.trim().is_empty() {
        return Err(EnsureInstalledError::UpdateContinuityRequired);
    }
    Ok(())
}

fn verify_post_update_continuity(
    candidate: &KnownCandidate,
    continuity: &UpdateContinuity,
    binding: &CanonicalBinding,
) -> Result<(), EnsureInstalledError> {
    if comparable_stable_root(&binding.candidate.stable_install_root)
        != comparable_stable_root(&continuity.stable_install_root)
        || comparable_stable_root(&binding.candidate.stable_install_root)
            != comparable_stable_root(&candidate.stable_install_root)
    {
        return Err(EnsureInstalledError::ContinuityChanged(
            "stable install root after effect",
        ));
    }
    if binding.identity.installation_id != continuity.installation_id {
        return Err(EnsureInstalledError::ContinuityChanged(
            "installation identity after effect",
        ));
    }
    if binding.identity.cortex_store_id != continuity.cortex_store_id {
        return Err(EnsureInstalledError::ContinuityChanged(
            "Cortex store identity after effect",
        ));
    }
    if candidate
        .expected_startup_generation
        .is_some_and(|generation| binding.identity.startup_generation <= generation)
    {
        return Err(EnsureInstalledError::ContinuityChanged(
            "startup generation after effect",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ServiceIdentity;
    use membrane_protocol::ResidentEndpointV1;
    use std::collections::VecDeque;
    use std::time::Duration;

    struct Effects {
        outcomes: VecDeque<DiscoveryOutcome>,
        provision_calls: usize,
        update_calls: usize,
    }

    impl InstalledRuntimeEffects for Effects {
        fn discover(&mut self, _deadline: Instant) -> Result<DiscoveryOutcome, ClientError> {
            Ok(self.outcomes.pop_front().expect("fixture discovery outcome"))
        }

        fn provision_packaged(&mut self, _deadline: Instant) -> Result<(), ClientError> {
            self.provision_calls += 1;
            Ok(())
        }

        fn update_repair_known(
            &mut self,
            _candidate: &KnownCandidate,
            _deadline: Instant,
        ) -> Result<(), ClientError> {
            self.update_calls += 1;
            Ok(())
        }
    }

    fn candidate(generation: u64) -> KnownCandidate {
        KnownCandidate {
            stable_install_root: "C:/Membrane/current".into(),
            endpoint: ResidentEndpointV1 {
                host: "127.0.0.1".into(),
                port: 47_851,
            },
            expected_installation_id: Some("install-1".into()),
            expected_startup_generation: Some(generation),
        }
    }

    fn binding(generation: u64, store: &str) -> CanonicalBinding {
        CanonicalBinding {
            candidate: candidate(generation),
            identity: ServiceIdentity {
                service_id: "service-1".into(),
                installation_id: "install-1".into(),
                cortex_store_id: store.into(),
                release_generation: "release-2".into(),
                startup_generation: generation,
                runtime_origin: "installed".into(),
                stable_install_root: Some("C:/Membrane/current".into()),
                protocol_version: 1,
                schema_version: 1,
                native_only: true,
                subsystems: vec![],
                capabilities: vec![],
            },
        }
    }

    fn deadline() -> Instant {
        Instant::now() + Duration::from_secs(5)
    }

    fn continuity() -> UpdateContinuity {
        UpdateContinuity {
            stable_install_root: "C:/Membrane/current".into(),
            installation_id: "install-1".into(),
            cortex_store_id: "store-1".into(),
        }
    }

    #[test]
    fn compatible_runtime_is_adopted_without_effect() {
        let expected = binding(7, "store-1");
        let mut effects = Effects {
            outcomes: [DiscoveryOutcome::Compatible(expected.clone())].into(),
            provision_calls: 0,
            update_calls: 0,
        };
        assert_eq!(
            ensure_installed_runtime(&mut effects, deadline(), None).unwrap(),
            expected
        );
        assert_eq!((effects.provision_calls, effects.update_calls), (0, 0));
    }

    #[test]
    fn absence_provisions_once_then_rediscovers() {
        let expected = binding(1, "store-new");
        let mut effects = Effects {
            outcomes: [
                DiscoveryOutcome::NotFound,
                DiscoveryOutcome::Compatible(expected.clone()),
            ]
            .into(),
            provision_calls: 0,
            update_calls: 0,
        };
        assert_eq!(
            ensure_installed_runtime(&mut effects, deadline(), None).unwrap(),
            expected
        );
        assert_eq!((effects.provision_calls, effects.update_calls), (1, 0));
    }

    #[test]
    fn incompatible_runtime_updates_once_with_full_continuity() {
        let known = candidate(7);
        let expected = binding(8, "store-1");
        let mut effects = Effects {
            outcomes: [
                DiscoveryOutcome::Incompatible {
                    candidate: known,
                    message: "old release".into(),
                },
                DiscoveryOutcome::Compatible(expected.clone()),
            ]
            .into(),
            provision_calls: 0,
            update_calls: 0,
        };
        assert_eq!(
            ensure_installed_runtime(&mut effects, deadline(), Some(&continuity())).unwrap(),
            expected
        );
        assert_eq!((effects.provision_calls, effects.update_calls), (0, 1));
    }

    #[test]
    fn known_refusal_never_runs_installer_effect() {
        let mut effects = Effects {
            outcomes: [DiscoveryOutcome::OfflineKnown {
                candidate: candidate(7),
                message: "offline".into(),
            }]
            .into(),
            provision_calls: 0,
            update_calls: 0,
        };
        assert_eq!(
            ensure_installed_runtime(&mut effects, deadline(), None),
            Err(EnsureInstalledError::Refused(DiscoveryKind::OfflineKnown))
        );
        assert_eq!((effects.provision_calls, effects.update_calls), (0, 0));
    }

    #[test]
    fn update_rejects_changed_store_after_rediscovery() {
        let mut effects = Effects {
            outcomes: [
                DiscoveryOutcome::Incompatible {
                    candidate: candidate(7),
                    message: "old release".into(),
                },
                DiscoveryOutcome::Compatible(binding(8, "store-2")),
            ]
            .into(),
            provision_calls: 0,
            update_calls: 0,
        };
        assert_eq!(
            ensure_installed_runtime(&mut effects, deadline(), Some(&continuity())),
            Err(EnsureInstalledError::ContinuityChanged(
                "Cortex store identity after effect"
            ))
        );
    }

    #[test]
    fn expired_deadline_stops_before_discovery() {
        let mut effects = Effects {
            outcomes: VecDeque::new(),
            provision_calls: 0,
            update_calls: 0,
        };
        assert_eq!(
            ensure_installed_runtime(&mut effects, Instant::now(), None),
            Err(EnsureInstalledError::DeadlineExpired)
        );
    }
}
