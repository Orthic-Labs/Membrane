//! Frozen registry for the nine canonical federation lanes.

use crate::error::RegistryError;
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
}
