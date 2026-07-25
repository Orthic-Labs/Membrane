//! In-memory store of memory entries.

use std::collections::HashMap;

use crate::types::{MemoryEntry, MemoryTier};

/// Errors that can occur when operating on the registry.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RegistryError {
    #[error("memory entry not found: {0}")]
    NotFound(String),
}

/// An in-memory store of [`MemoryEntry`] values keyed by id.
#[derive(Debug, Default)]
pub struct MemoryRegistry {
    entries: HashMap<String, MemoryEntry>,
}

impl MemoryRegistry {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    /// Insert (or replace) an entry.
    pub fn insert(&mut self, entry: MemoryEntry) {
        self.entries.insert(entry.id.clone(), entry);
    }

    pub fn get(&self, id: &str) -> Option<&MemoryEntry> {
        self.entries.get(id)
    }

    pub fn remove(&mut self, id: &str) -> Option<MemoryEntry> {
        self.entries.remove(id)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// All entries currently in the given tier.
    pub fn entries_in_tier(&self, tier: MemoryTier) -> Vec<&MemoryEntry> {
        self.entries.values().filter(|e| e.tier == tier).collect()
    }

    /// All entries.
    pub fn all(&self) -> Vec<&MemoryEntry> {
        self.entries.values().collect()
    }

    /// Promote an entry to a higher tier. Returns an error if the entry is unknown.
    /// If `new_tier` is not strictly higher than the current tier, the entry is left
    /// unchanged (no downgrade) but no error is returned.
    pub fn promote(&mut self, id: &str, new_tier: MemoryTier) -> Result<(), RegistryError> {
        let entry = self
            .entries
            .get_mut(id)
            .ok_or_else(|| RegistryError::NotFound(id.to_string()))?;
        if new_tier.rank() > entry.tier.rank() {
            entry.tier = new_tier;
        }
        Ok(())
    }

    /// Increment the access count for an entry and return the new count.
    pub fn record_access(&mut self, id: &str) -> Result<u32, RegistryError> {
        let entry = self
            .entries
            .get_mut(id)
            .ok_or_else(|| RegistryError::NotFound(id.to_string()))?;
        entry.access_count = entry.access_count.saturating_add(1);
        Ok(entry.access_count)
    }
}
