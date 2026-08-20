//! MBR-402 bounded batch admission constants and typed terminal receipts.
//!
//! P2 §8.4: this module is also the centralized resource-budget registry. One
//! struct validates every operation budget (items, bytes, tokens, CPU, wall,
//! model calls, memory, disk, descriptors, spill, queue, cache, derived growth)
//! at startup, reserves capacity before work begins, and always preserves a
//! diagnostics lane so degraded/canceled work can still report itself.
use serde::{Deserialize, Serialize};

pub const MAX_ITEMS: usize = 50;
pub const MAX_BYTES: usize = 1024 * 1024;
pub const MAX_TOKENS: usize = 50_000;
pub const MAX_DEADLINE_MS: u64 = 5_000;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TerminalReceiptV1 {
    pub id: String,
    pub status: String,
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub data: Option<serde_json::Value>,
}

/// A single dimension-accounted reservation against a [`ResourceBudgetV1`].
///
/// Reservations are additive and saturating so a burst of small requests can
/// never wrap a counter and re-open an already-exhausted lane.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReservationV1 {
    pub items: usize,
    pub bytes: usize,
    pub tokens: usize,
    pub cpu_millis: u64,
    pub wall_millis: u64,
    pub model_calls: usize,
    pub memory_bytes: usize,
    pub disk_bytes: usize,
    pub descriptors: usize,
    pub spill_bytes: usize,
    pub queue_depth: usize,
    pub cache_bytes: usize,
    pub derived_growth_bytes: usize,
}

impl ReservationV1 {
    pub const ZERO: Self = Self {
        items: 0,
        bytes: 0,
        tokens: 0,
        cpu_millis: 0,
        wall_millis: 0,
        model_calls: 0,
        memory_bytes: 0,
        disk_bytes: 0,
        descriptors: 0,
        spill_bytes: 0,
        queue_depth: 0,
        cache_bytes: 0,
        derived_growth_bytes: 0,
    };

    /// One item whose only charged dimension is its byte size.
    pub fn one_item(bytes: usize) -> Self {
        Self {
            items: 1,
            bytes,
            tokens: bytes.saturating_div(4),
            ..Self::ZERO
        }
    }

    pub fn saturating_add(self, other: Self) -> Self {
        Self {
            items: self.items.saturating_add(other.items),
            bytes: self.bytes.saturating_add(other.bytes),
            tokens: self.tokens.saturating_add(other.tokens),
            cpu_millis: self.cpu_millis.saturating_add(other.cpu_millis),
            wall_millis: self.wall_millis.saturating_add(other.wall_millis),
            model_calls: self.model_calls.saturating_add(other.model_calls),
            memory_bytes: self.memory_bytes.saturating_add(other.memory_bytes),
            disk_bytes: self.disk_bytes.saturating_add(other.disk_bytes),
            descriptors: self.descriptors.saturating_add(other.descriptors),
            spill_bytes: self.spill_bytes.saturating_add(other.spill_bytes),
            queue_depth: self.queue_depth.saturating_add(other.queue_depth),
            cache_bytes: self.cache_bytes.saturating_add(other.cache_bytes),
            derived_growth_bytes: self
                .derived_growth_bytes
                .saturating_add(other.derived_growth_bytes),
        }
    }
}

/// Diagnostics lane (P2 §8.4): a small, always-reservable reservation so a
/// degraded or canceled operation can still report itself. It never competes
/// with the primary budget and must fit inside every validated budget.
pub const DIAGNOSTICS_LANE: ReservationV1 = ReservationV1 {
    items: 1,
    bytes: 4096,
    tokens: 64,
    cpu_millis: 250,
    wall_millis: 1_000,
    model_calls: 0,
    memory_bytes: 4096,
    disk_bytes: 4096,
    descriptors: 2,
    spill_bytes: 0,
    queue_depth: 1,
    cache_bytes: 0,
    derived_growth_bytes: 0,
};

/// Centralized resource budget. Every field is a hard ceiling; none may be zero
/// (a zero budget would silently starve that dimension instead of failing fast).
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResourceBudgetV1 {
    pub items: usize,
    pub bytes: usize,
    pub tokens: usize,
    pub cpu_millis: u64,
    pub wall_millis: u64,
    pub model_calls: usize,
    pub memory_bytes: usize,
    pub disk_bytes: usize,
    pub descriptors: usize,
    pub spill_bytes: usize,
    pub queue_depth: usize,
    pub cache_bytes: usize,
    pub derived_growth_bytes: usize,
}

impl ResourceBudgetV1 {
    /// Bounded defaults derived from the MBR-402 batch constants plus explicit
    /// resource ceilings. `defaults()` never leaves a dimension unbounded.
    pub fn defaults() -> Self {
        Self {
            items: MAX_ITEMS,
            bytes: MAX_BYTES,
            tokens: MAX_TOKENS,
            cpu_millis: MAX_DEADLINE_MS,
            wall_millis: MAX_DEADLINE_MS,
            model_calls: 1,
            memory_bytes: 64 * 1024 * 1024,
            disk_bytes: 256 * 1024 * 1024,
            descriptors: 64,
            spill_bytes: 256 * 1024 * 1024,
            queue_depth: 256,
            cache_bytes: 128 * 1024 * 1024,
            derived_growth_bytes: 64 * 1024 * 1024,
        }
    }

    /// Validate at startup: every ceiling must be positive and derived growth
    /// must never exceed the disk ceiling it is derived from. The diagnostics
    /// lane must remain reservable against the validated budget.
    pub fn validate(&self) -> Result<(), String> {
        if self.items == 0 {
            return Err("budget_items_zero".to_string());
        }
        if self.bytes == 0 {
            return Err("budget_bytes_zero".to_string());
        }
        if self.tokens == 0 {
            return Err("budget_tokens_zero".to_string());
        }
        if self.cpu_millis == 0 {
            return Err("budget_cpu_millis_zero".to_string());
        }
        if self.wall_millis == 0 {
            return Err("budget_wall_millis_zero".to_string());
        }
        if self.model_calls == 0 {
            return Err("budget_model_calls_zero".to_string());
        }
        if self.memory_bytes == 0 {
            return Err("budget_memory_bytes_zero".to_string());
        }
        if self.disk_bytes == 0 {
            return Err("budget_disk_bytes_zero".to_string());
        }
        if self.descriptors == 0 {
            return Err("budget_descriptors_zero".to_string());
        }
        if self.spill_bytes == 0 {
            return Err("budget_spill_bytes_zero".to_string());
        }
        if self.queue_depth == 0 {
            return Err("budget_queue_depth_zero".to_string());
        }
        if self.cache_bytes == 0 {
            return Err("budget_cache_bytes_zero".to_string());
        }
        if self.derived_growth_bytes == 0 {
            return Err("budget_derived_growth_zero".to_string());
        }
        if self.derived_growth_bytes > self.disk_bytes {
            return Err("budget_derived_growth_exceeds_disk".to_string());
        }
        if self.reserve(&DIAGNOSTICS_LANE).is_err() {
            return Err("budget_diagnostics_lane_unreservable".to_string());
        }
        Ok(())
    }

    /// Reserve before work: every requested dimension must fit its ceiling.
    /// Returns an Err naming the first exhausted dimension so callers fail
    /// closed instead of starting work they cannot finish.
    pub fn reserve(&self, requested: &ReservationV1) -> Result<(), String> {
        if requested.items > self.items {
            return Err("budget_items_exceeded".to_string());
        }
        if requested.bytes > self.bytes {
            return Err("budget_bytes_exceeded".to_string());
        }
        if requested.tokens > self.tokens {
            return Err("budget_tokens_exceeded".to_string());
        }
        if requested.cpu_millis > self.cpu_millis {
            return Err("budget_cpu_millis_exceeded".to_string());
        }
        if requested.wall_millis > self.wall_millis {
            return Err("budget_wall_millis_exceeded".to_string());
        }
        if requested.model_calls > self.model_calls {
            return Err("budget_model_calls_exceeded".to_string());
        }
        if requested.memory_bytes > self.memory_bytes {
            return Err("budget_memory_exceeded".to_string());
        }
        if requested.disk_bytes > self.disk_bytes {
            return Err("budget_disk_exceeded".to_string());
        }
        if requested.descriptors > self.descriptors {
            return Err("budget_descriptors_exceeded".to_string());
        }
        if requested.spill_bytes > self.spill_bytes {
            return Err("budget_spill_exceeded".to_string());
        }
        if requested.queue_depth > self.queue_depth {
            return Err("budget_queue_exceeded".to_string());
        }
        if requested.cache_bytes > self.cache_bytes {
            return Err("budget_cache_exceeded".to_string());
        }
        if requested.derived_growth_bytes > self.derived_growth_bytes {
            return Err("budget_derived_growth_exceeded".to_string());
        }
        Ok(())
    }

    /// True iff the diagnostics lane is reservable against this budget.
    pub fn diagnostics_lane_reserved(&self) -> bool {
        self.reserve(&DIAGNOSTICS_LANE).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_validate_and_preserve_diagnostics_lane() {
        let budget = ResourceBudgetV1::defaults();
        assert!(budget.validate().is_ok());
        assert!(budget.diagnostics_lane_reserved());
        assert_eq!(budget.items, MAX_ITEMS);
        assert_eq!(budget.bytes, MAX_BYTES);
        assert_eq!(budget.tokens, MAX_TOKENS);
    }

    #[test]
    fn reserve_admits_fits_and_rejects_exhausted_dimensions() {
        let budget = ResourceBudgetV1::defaults();
        assert!(budget.reserve(&ReservationV1::one_item(1024)).is_ok());
        let too_many_items = ReservationV1 {
            items: budget.items + 1,
            ..ReservationV1::ZERO
        };
        assert_eq!(
            budget.reserve(&too_many_items),
            Err("budget_items_exceeded".to_string())
        );
        let too_many_bytes = ReservationV1 {
            bytes: budget.bytes + 1,
            ..ReservationV1::ZERO
        };
        assert_eq!(
            budget.reserve(&too_many_bytes),
            Err("budget_bytes_exceeded".to_string())
        );
    }

    #[test]
    fn validate_rejects_zero_and_derived_over_disk() {
        let mut zero_items = ResourceBudgetV1::defaults();
        zero_items.items = 0;
        assert_eq!(zero_items.validate(), Err("budget_items_zero".to_string()));

        let mut growth = ResourceBudgetV1::defaults();
        growth.derived_growth_bytes = growth.disk_bytes + 1;
        assert_eq!(
            growth.validate(),
            Err("budget_derived_growth_exceeds_disk".to_string())
        );
    }

    #[test]
    fn reservations_add_saturating_without_wrap() {
        let a = ReservationV1::one_item(usize::MAX);
        let b = ReservationV1::one_item(8);
        assert_eq!(a.saturating_add(b).bytes, usize::MAX);
        assert_eq!(a.saturating_add(b).items, 2);
    }
}
