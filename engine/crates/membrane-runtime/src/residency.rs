//! Resident-controller lease coordination.
//!
//! This is deliberately separate from bounded explicit execution: callers can
//! use explicit operations without acquiring a lease.  The registry only
//! decides when automatic work must start or drain.

use membrane_client::{
    AcquireDecision, AuthenticatedHolder, ControllerIdentity, ReleaseDecision, ResidencyError,
    ResidencyRegistry, ResidencySnapshot,
};
use membrane_protocol::{
    ResidentControllerIdentityV1, ResidentHolderCredentialV1, ResidentHolderLossKindV1,
    ResidentHolderLossV1, ResidentHolderOperationV1, ResidentHolderRequestV1,
    ResidentHolderResponseV1, ResidentHolderStatusV1, ResidentServicesUnavailableV1,
    RESIDENT_HOLDER_SCHEMA_VERSION,
};

pub use membrane_client::{AuthenticatedHolder as Holder, ControllerIdentity as Identity, HolderKind};

/// Process-local controller half used by installed service hosts.  The
/// installed SDK carries the same authenticated holder contract across host
/// boundaries; this type makes start/drain decisions atomic for one runtime
/// owner and never fabricates an explicit-operation dependency.
#[derive(Debug, Default)]
pub struct ResidentController {
    registry: ResidencyRegistry,
    loss_sequence: u64,
    last_loss: Option<ResidentHolderLossV1>,
    stable_current: Option<String>,
}

impl ResidentController {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn snapshot(&self) -> ResidencySnapshot {
        self.registry.snapshot()
    }

    /// Acquire or renew one authenticated holder.  `start_controller` is true
    /// exactly once, on transition from no holders to one or more holders.
    pub fn acquire(
        &mut self,
        identity: ControllerIdentity,
        holder: AuthenticatedHolder,
        now_ms: u64,
        expires_at_ms: u64,
    ) -> Result<AcquireDecision, ResidencyError> {
        self.registry.acquire(identity, holder, now_ms, expires_at_ms)
    }

    pub fn renew(
        &mut self,
        holder: &AuthenticatedHolder,
        now_ms: u64,
        expires_at_ms: u64,
    ) -> Result<ResidencySnapshot, ResidencyError> {
        self.registry.renew(holder, now_ms, expires_at_ms)
    }

    /// Release one holder.  Only final holder release asks owner to drain.
    pub fn release(&mut self, holder: &AuthenticatedHolder) -> Result<ReleaseDecision, ResidencyError> {
        self.registry.release(holder)
    }

    /// Expiry has same final-holder behavior as explicit release.
    pub fn reconcile_expired(&mut self, now_ms: u64) -> ReleaseDecision {
        let release = self.registry.reconcile_expired(now_ms);
        if release.drain_controller {
            let stable_current = self.stable_current.take().unwrap_or_default();
            self.record_loss(
                ResidentHolderLossKindV1::LeaseExpired,
                release.drained_identity.as_ref(),
                stable_current,
            );
        }
        release
    }

    /// Server half of installed holder IPC. Transport, OS credential binding,
    /// and connection-loss detection remain host-provided; this endpoint owns
    /// only exact request identity, lease mutation, and loss sequencing.
    pub fn dispatch_installed(
        &mut self,
        expected_stable_current: &str,
        request: ResidentHolderRequestV1,
    ) -> Result<ResidentHolderResponseV1, ResidencyError> {
        if request.schema_version != RESIDENT_HOLDER_SCHEMA_VERSION
            || request.controller.stable_current != expected_stable_current
        {
            return Err(ResidencyError::ControllerMismatch);
        }
        let identity = controller_from_protocol(&request.controller)?;
        let response = match request.operation {
            ResidentHolderOperationV1::Acquire => {
                let holder = required_holder(request.holder)?;
                self.acquire(identity, holder, request.observed_at_unix_ms, required_expiry(request.expires_at_unix_ms)?)?;
                self.stable_current = Some(request.controller.stable_current.clone());
                self.response(request.operation, request.controller, request.loss_cursor)
            }
            ResidentHolderOperationV1::Renew => {
                self.ensure_active_identity(&identity)?;
                let holder = required_holder(request.holder)?;
                self.renew(&holder, request.observed_at_unix_ms, required_expiry(request.expires_at_unix_ms)?)?;
                self.response(request.operation, request.controller, request.loss_cursor)
            }
            ResidentHolderOperationV1::Release => {
                self.ensure_active_identity(&identity)?;
                let holder = required_holder(request.holder)?;
                let release = self.registry.release(&holder)?;
                if release.drain_controller {
                    let stable_current = self.stable_current.take().unwrap_or_default();
                    self.record_loss(
                        ResidentHolderLossKindV1::FinalRelease,
                        release.drained_identity.as_ref(),
                        stable_current,
                    );
                }
                self.response(request.operation, request.controller, request.loss_cursor)
            }
            ResidentHolderOperationV1::Status => {
                self.ensure_active_identity_if_present(&identity)?;
                self.response(request.operation, request.controller, request.loss_cursor)
            }
            ResidentHolderOperationV1::SubscribeLoss => {
                let release = self.reconcile_expired(request.observed_at_unix_ms);
                if release.drain_controller {
                    // `reconcile_expired` records loss before returning.
                }
                self.ensure_active_identity_if_present(&identity)?;
                self.response(request.operation, request.controller, request.loss_cursor)
            }
        };
        Ok(response)
    }

    /// Authoritative server entrypoint. Network callers cannot choose clock
    /// or controller truth: both come from installed runtime state.
    pub fn dispatch_authoritative(
        &mut self,
        identity: ResidentControllerIdentityV1,
        now_unix_ms: u64,
        request: ResidentHolderRequestV1,
        services_ready: bool,
    ) -> Result<ResidentHolderResponseV1, ResidencyError> {
        self.dispatch_authoritative_reporting(identity, now_unix_ms, request, services_ready, None)
    }

    /// As [`Self::dispatch_authoritative`], but the caller supplies the typed
    /// reason resident services are withheld so a store, catalog, or
    /// lifecycle fault is not misreported as a watcher fault. `None` falls
    /// back to a conservative mapping: `Draining` once the controller is
    /// inactive, `BlueprintWatcherUnavailable` while it stays active.
    pub fn dispatch_authoritative_reporting(
        &mut self,
        identity: ResidentControllerIdentityV1,
        now_unix_ms: u64,
        mut request: ResidentHolderRequestV1,
        services_ready: bool,
        services_unavailable: Option<ResidentServicesUnavailableV1>,
    ) -> Result<ResidentHolderResponseV1, ResidencyError> {
        if request.controller != identity {
            return Err(ResidencyError::ControllerMismatch);
        }
        if request
            .expires_at_unix_ms
            .is_some_and(|expiry| expiry > now_unix_ms.saturating_add(60_000))
        {
            return Err(ResidencyError::InvalidExpiry);
        }
        request.observed_at_unix_ms = now_unix_ms;
        let mut response = self.dispatch_installed(&identity.stable_current, request)?;
        response.status.services_ready = response.status.controller_active && services_ready;
        response.status.services_unavailable_reason = (!response.status.services_ready).then(|| {
            services_unavailable.unwrap_or(if response.status.controller_active {
                ResidentServicesUnavailableV1::BlueprintWatcherUnavailable
            } else {
                ResidentServicesUnavailableV1::Draining
            })
        });
        Ok(response)
    }

    fn ensure_active_identity(&self, identity: &ControllerIdentity) -> Result<(), ResidencyError> {
        match self.registry.snapshot().controller {
            Some(active) if active == *identity => Ok(()),
            Some(_) => Err(ResidencyError::ControllerMismatch),
            None => Err(ResidencyError::HolderMissing),
        }
    }

    fn ensure_active_identity_if_present(&self, identity: &ControllerIdentity) -> Result<(), ResidencyError> {
        match self.registry.snapshot().controller {
            Some(active) if active == *identity => Ok(()),
            Some(_) => Err(ResidencyError::ControllerMismatch),
            None => Ok(()),
        }
    }

    fn response(
        &self,
        operation: ResidentHolderOperationV1,
        controller: ResidentControllerIdentityV1,
        loss_cursor: Option<u64>,
    ) -> ResidentHolderResponseV1 {
        let snapshot = self.registry.snapshot();
        ResidentHolderResponseV1 {
            schema_version: RESIDENT_HOLDER_SCHEMA_VERSION,
            operation,
            controller,
            status: ResidentHolderStatusV1 {
                controller_active: snapshot.controller.is_some(),
                services_ready: false,
                services_unavailable_reason: Some(ResidentServicesUnavailableV1::BlueprintWatcherUnavailable),
                hub_holders: snapshot.hub_holders.try_into().unwrap_or(u32::MAX),
                coderight_daemon_holders: snapshot.coderight_daemon_holders.try_into().unwrap_or(u32::MAX),
                harness_holders: snapshot.harness_holders.try_into().unwrap_or(u32::MAX),
            },
            loss: (operation == ResidentHolderOperationV1::SubscribeLoss)
                .then(|| self.last_loss.clone())
                .flatten()
                .filter(|loss| loss.sequence > loss_cursor.unwrap_or(0)),
        }
    }

    fn record_loss(
        &mut self,
        kind: ResidentHolderLossKindV1,
        identity: Option<&ControllerIdentity>,
        stable_current: String,
    ) {
        let Some(identity) = identity else { return };
        self.loss_sequence = self.loss_sequence.saturating_add(1);
        self.last_loss = Some(ResidentHolderLossV1 {
            sequence: self.loss_sequence,
            kind,
            controller: protocol_identity(identity, stable_current),
        });
    }
}

fn required_expiry(value: Option<u64>) -> Result<u64, ResidencyError> {
    value.ok_or(ResidencyError::InvalidExpiry)
}

fn required_holder(value: Option<ResidentHolderCredentialV1>) -> Result<AuthenticatedHolder, ResidencyError> {
    let value = value.ok_or(ResidencyError::InvalidHolder)?;
    let kind = match value.holder_kind.as_str() {
        "hub" => HolderKind::Hub,
        "coderight_daemon" => HolderKind::CodeRightDaemon,
        "harness" => HolderKind::Harness,
        _ => return Err(ResidencyError::InvalidHolder),
    };
    Ok(AuthenticatedHolder { kind, holder_id: value.holder_id, credential_id: value.credential_id })
}

fn controller_from_protocol(value: &ResidentControllerIdentityV1) -> Result<ControllerIdentity, ResidencyError> {
    let identity = ControllerIdentity {
        installation_id: value.installation_id.clone(),
        cortex_store_id: value.cortex_store_id.clone(),
        release_generation: value.release_generation.clone(),
        startup_generation: value.startup_generation,
    };
    if identity.installation_id.trim().is_empty()
        || identity.cortex_store_id.trim().is_empty()
        || identity.release_generation.trim().is_empty()
        || identity.startup_generation == 0
    {
        Err(ResidencyError::InvalidController)
    } else {
        Ok(identity)
    }
}

fn protocol_identity(value: &ControllerIdentity, stable_current: String) -> ResidentControllerIdentityV1 {
    ResidentControllerIdentityV1 {
        installation_id: value.installation_id.clone(),
        cortex_store_id: value.cortex_store_id.clone(),
        release_generation: value.release_generation.clone(),
        startup_generation: value.startup_generation,
        stable_current,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity() -> ResidentControllerIdentityV1 {
        ResidentControllerIdentityV1 {
            installation_id: "install-1".into(),
            cortex_store_id: "cortex-1".into(),
            release_generation: "gen-1".into(),
            startup_generation: 1,
            stable_current: "D:/Membrane/current".into(),
        }
    }

    fn status_request(controller: ResidentControllerIdentityV1) -> ResidentHolderRequestV1 {
        ResidentHolderRequestV1 {
            schema_version: RESIDENT_HOLDER_SCHEMA_VERSION,
            operation: ResidentHolderOperationV1::Status,
            controller,
            holder: None,
            expires_at_unix_ms: None,
            observed_at_unix_ms: 0,
            loss_cursor: None,
        }
    }

    fn acquire_request(controller: ResidentControllerIdentityV1) -> ResidentHolderRequestV1 {
        ResidentHolderRequestV1 {
            schema_version: RESIDENT_HOLDER_SCHEMA_VERSION,
            operation: ResidentHolderOperationV1::Acquire,
            controller,
            holder: Some(ResidentHolderCredentialV1 {
                holder_kind: "hub".into(),
                holder_id: "hub-1".into(),
                credential_id: "cred-1".into(),
            }),
            expires_at_unix_ms: Some(1_000),
            observed_at_unix_ms: 0,
            loss_cursor: None,
        }
    }

    #[test]
    fn authoritative_status_reports_the_callers_typed_unavailable_reason() {
        let mut controller = ResidentController::new();
        let identity = identity();
        controller
            .dispatch_authoritative_reporting(
                identity.clone(),
                100,
                acquire_request(identity.clone()),
                true,
                None,
            )
            .expect("acquire succeeds");
        let response = controller
            .dispatch_authoritative_reporting(
                identity.clone(),
                200,
                status_request(identity),
                false,
                Some(ResidentServicesUnavailableV1::CatalogUnavailable),
            )
            .expect("status succeeds");
        assert!(!response.status.services_ready);
        assert_eq!(
            response.status.services_unavailable_reason,
            Some(ResidentServicesUnavailableV1::CatalogUnavailable),
            "a catalog fault must not be misreported as a watcher fault"
        );
    }

    #[test]
    fn authoritative_status_on_inactive_controller_defaults_to_draining_not_watcher() {
        let mut controller = ResidentController::new();
        let identity = identity();
        let response = controller
            .dispatch_authoritative_reporting(
                identity.clone(),
                100,
                status_request(identity.clone()),
                false,
                None,
            )
            .expect("status succeeds");
        assert!(!response.status.controller_active);
        assert_eq!(
            response.status.services_unavailable_reason,
            Some(ResidentServicesUnavailableV1::Draining),
            "an inactive controller is draining, not watcher-failed"
        );
    }

    #[test]
    fn authoritative_status_reports_no_reason_when_services_ready() {
        let mut controller = ResidentController::new();
        let identity = identity();
        controller
            .dispatch_authoritative_reporting(
                identity.clone(),
                100,
                acquire_request(identity.clone()),
                true,
                None,
            )
            .expect("acquire succeeds");
        let response = controller
            .dispatch_authoritative_reporting(
                identity.clone(),
                200,
                status_request(identity),
                true,
                None,
            )
            .expect("status succeeds");
        assert!(response.status.services_ready);
        assert_eq!(response.status.services_unavailable_reason, None);
    }
}
