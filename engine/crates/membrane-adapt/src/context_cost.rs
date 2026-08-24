//! Persistent-context cost attribution (canon §7).
//!
//! Three strictly separated cost classes:
//! - `Measured`: bytes/tokens counted from actual persisted context payloads.
//! - `Inferred`: estimates derived from record shapes (labelled, never mixed
//!   into measured totals).
//! - `Unattributed`: usage reported by a host that cannot be tied to a
//!   specific record; carried separately, never redistributed.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CostClass {
    Measured,
    Inferred,
    Unattributed,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CostAmount {
    pub bytes: u64,
    pub tokens: Option<u64>,
}

impl CostAmount {
    pub fn checked_add(self, other: CostAmount) -> Option<CostAmount> {
        Some(CostAmount {
            bytes: self.bytes.checked_add(other.bytes)?,
            tokens: match (self.tokens, other.tokens) {
                (Some(a), Some(b)) => Some(a.checked_add(b)?),
                _ => None,
            },
        })
    }
}

/// One attribution entry: which durable record contributed how much.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostAttributionV1 {
    pub cortex_record_ref: String,
    pub class: CostClass,
    pub amount: CostAmount,
}

/// Per-installation persistent-context cost report. Totals are computed per
/// class only — classes are NEVER merged.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ContextCostReportV1 {
    pub installation_id: String,
    pub by_class: BTreeMap<CostClass, CostAmount>,
    /// Per-record detail for measured entries only.
    pub measured_records: Vec<CostAttributionV1>,
    /// Records whose cost could not be attributed at all.
    pub unattributed_records: Vec<CostAttributionV1>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CostOverflow;

impl std::fmt::Display for CostOverflow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "context cost accounting overflow")
    }
}

impl std::error::Error for CostOverflow {}

impl ContextCostReportV1 {
    pub fn new(installation_id: &str) -> Self {
        Self {
            installation_id: installation_id.to_string(),
            ..Default::default()
        }
    }

    pub fn attribute(
        &mut self,
        record_ref: &str,
        class: CostClass,
        amount: CostAmount,
    ) -> Result<(), CostOverflow> {
        let slot = self.by_class.entry(class).or_insert(CostAmount { bytes: 0, tokens: None });
        *slot = slot.checked_add(amount).ok_or(CostOverflow)?;
        let entry = CostAttributionV1 {
            cortex_record_ref: record_ref.to_string(),
            class,
            amount,
        };
        match class {
            CostClass::Measured => self.measured_records.push(entry),
            CostClass::Unattributed => self.unattributed_records.push(entry),
            CostClass::Inferred => {}
        }
        Ok(())
    }

    /// The headline number is MEASURED bytes only. Inferred and unattributed
    /// are reported separately and must not be folded in.
    pub fn measured_bytes(&self) -> u64 {
        self.by_class.get(&CostClass::Measured).map(|a| a.bytes).unwrap_or(0)
    }

    pub fn inferred_bytes(&self) -> u64 {
        self.by_class.get(&CostClass::Inferred).map(|a| a.bytes).unwrap_or(0)
    }

    pub fn unattributed_bytes(&self) -> u64 {
        self.by_class.get(&CostClass::Unattributed).map(|a| a.bytes).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cost_classes_never_merge() {
        let mut r = ContextCostReportV1::new("inst");
        r.attribute("rec-1", CostClass::Measured, CostAmount { bytes: 100, tokens: Some(10) })
            .unwrap();
        r.attribute("rec-2", CostClass::Inferred, CostAmount { bytes: 50, tokens: None })
            .unwrap();
        r.attribute("rec-3", CostClass::Unattributed, CostAmount { bytes: 25, tokens: None })
            .unwrap();
        assert_eq!(r.measured_bytes(), 100);
        assert_eq!(r.inferred_bytes(), 50);
        assert_eq!(r.unattributed_bytes(), 25);
    }

    #[test]
    fn token_sum_with_partial_coverage_is_none() {
        let a = CostAmount { bytes: 10, tokens: Some(5) };
        let b = CostAmount { bytes: 10, tokens: None };
        assert_eq!(a.checked_add(b).unwrap().tokens, None);
    }
}
