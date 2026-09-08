//! Public additive holder lifecycle for one canonical installed controller.

use std::collections::BTreeMap;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum HolderKind {
    Hub,
    CodeRightDaemon,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct AuthenticatedHolder {
    pub kind: HolderKind,
    pub holder_id: String,
    pub credential_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControllerIdentity {
    pub installation_id: String,
    pub cortex_store_id: String,
    pub release_generation: String,
    pub startup_generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HolderLease {
    credential_id: String,
    expires_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResidencySnapshot {
    pub controller: Option<ControllerIdentity>,
    pub hub_holders: usize,
    pub coderight_daemon_holders: usize,
}

impl ResidencySnapshot {
    pub fn residents_required(&self) -> bool {
        self.hub_holders + self.coderight_daemon_holders > 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcquireDecision {
    pub start_controller: bool,
    pub snapshot: ResidencySnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseDecision {
    pub drain_controller: bool,
    pub drained_identity: Option<ControllerIdentity>,
    pub snapshot: ResidencySnapshot,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ResidencyError {
    #[error("holder identity fields must be non-empty")]
    InvalidHolder,
    #[error("controller identity fields and startup generation must be non-empty")]
    InvalidController,
    #[error("holder lease must expire after current time")]
    InvalidExpiry,
    #[error("controller identity does not match active controller")]
    ControllerMismatch,
    #[error("holder credential does not match active lease")]
    CredentialMismatch,
    #[error("holder lease is not active")]
    HolderMissing,
    #[error("holder lease has expired")]
    HolderExpired,
}

#[derive(Debug, Default)]
pub struct ResidencyRegistry {
    controller: Option<ControllerIdentity>,
    holders: BTreeMap<(HolderKind, String), HolderLease>,
}

impl ResidencyRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn snapshot(&self) -> ResidencySnapshot {
        let mut hub_holders = 0;
        let mut coderight_daemon_holders = 0;
        for kind in self.holders.keys().map(|(kind, _)| kind) {
            match kind {
                HolderKind::Hub => hub_holders += 1,
                HolderKind::CodeRightDaemon => coderight_daemon_holders += 1,
            }
        }
        ResidencySnapshot {
            controller: self.controller.clone(),
            hub_holders,
            coderight_daemon_holders,
        }
    }

    pub fn acquire(
        &mut self,
        controller: ControllerIdentity,
        holder: AuthenticatedHolder,
        now_ms: u64,
        expires_at_ms: u64,
    ) -> Result<AcquireDecision, ResidencyError> {
        validate_controller(&controller)?;
        validate_holder(&holder)?;
        if expires_at_ms <= now_ms {
            return Err(ResidencyError::InvalidExpiry);
        }
        self.reconcile_expired(now_ms);
        if self.controller.as_ref().is_some_and(|active| active != &controller) {
            return Err(ResidencyError::ControllerMismatch);
        }

        let key = (holder.kind, holder.holder_id);
        if self
            .holders
            .get(&key)
            .is_some_and(|lease| lease.credential_id != holder.credential_id)
        {
            return Err(ResidencyError::CredentialMismatch);
        }

        let start_controller = self.holders.is_empty();
        self.controller.get_or_insert(controller);
        self.holders.insert(
            key,
            HolderLease {
                credential_id: holder.credential_id,
                expires_at_ms,
            },
        );
        Ok(AcquireDecision {
            start_controller,
            snapshot: self.snapshot(),
        })
    }

    pub fn renew(
        &mut self,
        holder: &AuthenticatedHolder,
        now_ms: u64,
        expires_at_ms: u64,
    ) -> Result<ResidencySnapshot, ResidencyError> {
        validate_holder(holder)?;
        if expires_at_ms <= now_ms {
            return Err(ResidencyError::InvalidExpiry);
        }
        let lease = self
            .holders
            .get_mut(&(holder.kind, holder.holder_id.clone()))
            .ok_or(ResidencyError::HolderMissing)?;
        if lease.credential_id != holder.credential_id {
            return Err(ResidencyError::CredentialMismatch);
        }
        if lease.expires_at_ms <= now_ms {
            return Err(ResidencyError::HolderExpired);
        }
        lease.expires_at_ms = expires_at_ms;
        Ok(self.snapshot())
    }

    pub fn release(
        &mut self,
        holder: &AuthenticatedHolder,
    ) -> Result<ReleaseDecision, ResidencyError> {
        validate_holder(holder)?;
        let key = (holder.kind, holder.holder_id.clone());
        if let Some(lease) = self.holders.get(&key) {
            if lease.credential_id != holder.credential_id {
                return Err(ResidencyError::CredentialMismatch);
            }
            self.holders.remove(&key);
        }
        Ok(self.finalize_if_unheld())
    }

    pub fn reconcile_expired(&mut self, now_ms: u64) -> ReleaseDecision {
        self.holders.retain(|_, lease| lease.expires_at_ms > now_ms);
        self.finalize_if_unheld()
    }

    fn finalize_if_unheld(&mut self) -> ReleaseDecision {
        let drained_identity = if self.holders.is_empty() {
            self.controller.take()
        } else {
            None
        };
        ReleaseDecision {
            drain_controller: drained_identity.is_some(),
            drained_identity,
            snapshot: self.snapshot(),
        }
    }
}

fn validate_holder(holder: &AuthenticatedHolder) -> Result<(), ResidencyError> {
    if holder.holder_id.trim().is_empty() || holder.credential_id.trim().is_empty() {
        Err(ResidencyError::InvalidHolder)
    } else {
        Ok(())
    }
}

fn validate_controller(controller: &ControllerIdentity) -> Result<(), ResidencyError> {
    if controller.installation_id.trim().is_empty()
        || controller.cortex_store_id.trim().is_empty()
        || controller.release_generation.trim().is_empty()
        || controller.startup_generation == 0
    {
        Err(ResidencyError::InvalidController)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn controller() -> ControllerIdentity {
        ControllerIdentity {
            installation_id: "install-1".into(),
            cortex_store_id: "store-1".into(),
            release_generation: "release-1".into(),
            startup_generation: 1,
        }
    }

    fn holder(kind: HolderKind, id: &str) -> AuthenticatedHolder {
        AuthenticatedHolder {
            kind,
            holder_id: id.into(),
            credential_id: format!("credential-{id}"),
        }
    }

    #[test]
    fn all_four_holder_states_share_one_controller_and_drain_only_last() {
        let mut registry = ResidencyRegistry::new();
        let hub = holder(HolderKind::Hub, "hub-1");
        let coderight = holder(HolderKind::CodeRightDaemon, "coderight-1");
        assert!(registry.acquire(controller(), hub.clone(), 10, 100).unwrap().start_controller);
        assert!(!registry.acquire(controller(), coderight.clone(), 10, 100).unwrap().start_controller);
        assert_eq!((registry.snapshot().hub_holders, registry.snapshot().coderight_daemon_holders), (1, 1));
        assert!(!registry.release(&hub).unwrap().drain_controller);
        let neither = registry.release(&coderight).unwrap();
        assert!(neither.drain_controller);
        assert_eq!(neither.drained_identity, Some(controller()));
    }

    #[test]
    fn leases_are_idempotent_bounded_and_authenticated() {
        let mut registry = ResidencyRegistry::new();
        let hub = holder(HolderKind::Hub, "hub-1");
        assert!(registry.acquire(controller(), hub.clone(), 1, 10).unwrap().start_controller);
        assert!(!registry.acquire(controller(), hub.clone(), 2, 20).unwrap().start_controller);
        assert_eq!(registry.renew(&hub, 3, 30).unwrap().hub_holders, 1);
        let mut wrong = hub.clone();
        wrong.credential_id = "wrong".into();
        assert_eq!(registry.release(&wrong), Err(ResidencyError::CredentialMismatch));
        assert!(registry.reconcile_expired(30).drain_controller);
    }

    #[test]
    fn active_controller_rejects_split_identity() {
        let mut registry = ResidencyRegistry::new();
        registry.acquire(controller(), holder(HolderKind::Hub, "hub-1"), 1, 10).unwrap();
        let mut changed = controller();
        changed.cortex_store_id = "other-store".into();
        assert_eq!(
            registry.acquire(changed, holder(HolderKind::CodeRightDaemon, "coderight-1"), 1, 10),
            Err(ResidencyError::ControllerMismatch)
        );
    }
}
