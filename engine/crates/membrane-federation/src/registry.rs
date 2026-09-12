//! Frozen registry for the ten canonical federation lanes.

use crate::error::RegistryError;
use crate::requirements::{EvidenceDimensionV1, ProviderCapabilityV1};
use membrane_protocol::ProviderId;
use membrane_provider_sdk::{Provider, ProviderRegistration};
use std::sync::Arc;

/// Provider registrations are validated once and cannot be changed after
/// construction.  Canonical accessors always use protocol provider order.
pub struct ProviderRegistry {
    registrations: Vec<ProviderRegistration>,
}

impl std::fmt::Debug for ProviderRegistry {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProviderRegistry")
            .field("providers", &self.ids())
            .finish()
    }
}

impl ProviderRegistry {
    pub fn new(registrations: Vec<ProviderRegistration>) -> Result<Self, RegistryError> {
        if registrations.len() != ProviderId::ALL.len() {
            return Err(RegistryError::Incomplete);
        }
        let mut seen = Vec::with_capacity(registrations.len());
        for registration in &registrations {
            if seen.contains(&registration.id) {
                return Err(RegistryError::DuplicateProvider(registration.id));
            }
            seen.push(registration.id);
        }
        if ProviderId::ALL.iter().any(|id| !seen.contains(id)) {
            return Err(RegistryError::Incomplete);
        }
        // Reuse SDK validation for implementation keys, dependency integrity,
        // and cycle rejection while retaining a federation-owned frozen view.
        let validated = membrane_provider_sdk::registry::ProviderRegistry::new(registrations)
            .map_err(|error| RegistryError::ProviderSdk(error.to_string()))?;
        let registrations = validated
            .order()
            .into_iter()
            .filter_map(|id| validated.get(id))
            .map(|registration| ProviderRegistration {
                id: registration.id,
                implementation_key: registration.implementation_key.clone(),
                dependencies: registration.dependencies.clone(),
                provider: Arc::clone(&registration.provider),
            })
            .collect();
        Ok(Self { registrations })
    }

    pub fn from_names<I, S>(names: I) -> Result<Vec<ProviderId>, RegistryError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut ids = Vec::new();
        for name in names {
            let name = name.as_ref();
            let id = ProviderId::parse(name)
                .ok_or_else(|| RegistryError::UnknownProvider(name.to_owned()))?;
            if ids.contains(&id) {
                return Err(RegistryError::DuplicateProvider(id));
            }
            ids.push(id);
        }
        if ids.len() != ProviderId::ALL.len() || ProviderId::ALL.iter().any(|id| !ids.contains(id))
        {
            return Err(RegistryError::Incomplete);
        }
        Ok(ids)
    }

    pub fn ids(&self) -> Vec<ProviderId> {
        ProviderId::ALL
            .into_iter()
            .filter(|id| {
                self.registrations
                    .iter()
                    .any(|registration| registration.id == *id)
            })
            .collect()
    }

    pub fn get(&self, id: ProviderId) -> Option<&ProviderRegistration> {
        self.registrations
            .iter()
            .find(|registration| registration.id == id)
    }

    pub fn registrations(&self) -> &[ProviderRegistration] {
        &self.registrations
    }

    pub fn providers(&self) -> Vec<(ProviderId, Arc<dyn Provider>)> {
        self.ids()
            .into_iter()
            .filter_map(|id| {
                self.get(id)
                    .map(|registration| (id, Arc::clone(&registration.provider)))
            })
            .collect()
    }

    /// Content-free provider capabilities used for deterministic requirement
    /// planning. Registration remains source of provider authority; this
    /// catalog never grants access or changes provider execution order.
    pub fn capability_catalog(&self) -> Vec<ProviderCapabilityV1> {
        self.ids().into_iter().map(|provider| ProviderCapabilityV1 {
            provider,
            dimensions: match provider {
                ProviderId::Anchors | ProviderId::Blueprint | ProviderId::Architect | ProviderId::Ledger => vec![EvidenceDimensionV1::RepositoryTruth],
                ProviderId::LiveFiles => vec![EvidenceDimensionV1::CurrentState],
                ProviderId::Rules => vec![EvidenceDimensionV1::Policy],
                ProviderId::Git | ProviderId::Audit => vec![EvidenceDimensionV1::History, EvidenceDimensionV1::Diagnostics],
                ProviderId::Skills | ProviderId::Cortex => vec![EvidenceDimensionV1::DurableKnowledge],
            },
            authoritative: true,
            fresh: true,
            ready: true,
            cost_rank: provider.rank() as u32,
            omission: None,
        }).collect()
    }
}
