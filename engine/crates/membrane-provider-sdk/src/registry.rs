//! Frozen native provider registry.

use crate::error::{ProviderError, Result};
use crate::provider::Provider;
use membrane_protocol::ProviderId;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// One declarative provider registration.  Dependencies are provider IDs,
/// never module paths or dynamic import keys.
pub struct ProviderRegistration {
    pub id: ProviderId,
    pub implementation_key: String,
    pub dependencies: Vec<ProviderId>,
    pub provider: Arc<dyn Provider>,
}

impl std::fmt::Debug for ProviderRegistration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProviderRegistration")
            .field("id", &self.id)
            .field("implementation_key", &self.implementation_key)
            .field("dependencies", &self.dependencies)
            .finish_non_exhaustive()
    }
}

impl ProviderRegistration {
    pub fn new(
        id: ProviderId,
        implementation_key: impl Into<String>,
        dependencies: Vec<ProviderId>,
        provider: Arc<dyn Provider>,
    ) -> Self {
        Self {
            id,
            implementation_key: implementation_key.into(),
            dependencies,
            provider,
        }
    }

    pub fn named(
        name: &str,
        implementation_key: impl Into<String>,
        dependencies: Vec<ProviderId>,
        provider: Arc<dyn Provider>,
    ) -> Result<Self> {
        let id = ProviderId::parse(name)
            .ok_or_else(|| ProviderError::UnknownProvider(name.to_string()))?;
        Ok(Self::new(id, implementation_key, dependencies, provider))
    }
}

/// Registry is validated and frozen at construction.  There is no mutation
/// API after `new` returns, so provider ordering cannot drift during a plan.
pub struct ProviderRegistry {
    registrations: Vec<ProviderRegistration>,
    index: HashMap<ProviderId, usize>,
}

impl std::fmt::Debug for ProviderRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProviderRegistry")
            .field("providers", &self.order())
            .finish()
    }
}

impl ProviderRegistry {
    pub fn new(registrations: Vec<ProviderRegistration>) -> Result<Self> {
        let mut index = HashMap::new();
        for (position, registration) in registrations.iter().enumerate() {
            if index.insert(registration.id, position).is_some() {
                return Err(ProviderError::DuplicateProvider(
                    registration.id.as_str().to_string(),
                ));
            }
            if registration.implementation_key.trim().is_empty() {
                return Err(ProviderError::InvalidRegistry(format!(
                    "provider {} has an empty implementation key",
                    registration.id
                )));
            }
            let mut dependencies = HashSet::new();
            for dependency in &registration.dependencies {
                if !index.contains_key(dependency)
                    && !registrations
                        .iter()
                        .any(|candidate| candidate.id == *dependency)
                {
                    return Err(ProviderError::UnknownProvider(
                        dependency.as_str().to_string(),
                    ));
                }
                if !dependencies.insert(*dependency) {
                    return Err(ProviderError::InvalidRegistry(format!(
                        "provider {} repeats dependency {}",
                        registration.id, dependency
                    )));
                }
            }
        }

        let registry = Self {
            registrations,
            index,
        };
        registry.reject_cycles()?;
        Ok(registry)
    }

    fn reject_cycles(&self) -> Result<()> {
        fn visit(
            id: ProviderId,
            registry: &ProviderRegistry,
            visiting: &mut HashSet<ProviderId>,
            visited: &mut HashSet<ProviderId>,
        ) -> Result<()> {
            if visiting.contains(&id) {
                return Err(ProviderError::InvalidRegistry(format!(
                    "dependency cycle at {id}"
                )));
            }
            if !visited.insert(id) {
                return Ok(());
            }
            visiting.insert(id);
            let registration = &registry.registrations[registry.index[&id]];
            for dependency in &registration.dependencies {
                visit(*dependency, registry, visiting, visited)?;
            }
            visiting.remove(&id);
            Ok(())
        }

        let mut visiting = HashSet::new();
        let mut visited = HashSet::new();
        for registration in &self.registrations {
            visit(registration.id, self, &mut visiting, &mut visited)?;
        }
        Ok(())
    }

    /// Registrations in canonical federation order, independent of insertion
    /// or completion order.
    pub fn registrations(&self) -> &[ProviderRegistration] {
        &self.registrations
    }

    pub fn get(&self, id: ProviderId) -> Option<&ProviderRegistration> {
        self.index
            .get(&id)
            .map(|position| &self.registrations[*position])
    }

    pub fn order(&self) -> Vec<ProviderId> {
        ProviderId::ALL
            .into_iter()
            .filter(|id| self.index.contains_key(id))
            .collect()
    }

    pub fn providers(&self) -> Vec<(ProviderId, Arc<dyn Provider>)> {
        self.order()
            .into_iter()
            .filter_map(|id| {
                self.get(id)
                    .map(|registration| (id, registration.provider.clone()))
            })
            .collect()
    }
}
