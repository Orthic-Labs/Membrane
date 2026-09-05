//! Shared request budget. Sub-operations inherit this clock and counters.
use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub struct WorkBudget {
    pub deadline: Instant,
    pub cancellation: CancellationToken,
    bytes: Arc<AtomicUsize>,
    items: Arc<AtomicUsize>,
    max_bytes: usize,
    max_items: usize,
}
impl WorkBudget {
    pub fn new(deadline: Instant, cancellation: CancellationToken) -> Self {
        Self { deadline, cancellation, bytes: Arc::new(AtomicUsize::new(0)),
            items: Arc::new(AtomicUsize::new(0)), max_bytes: 64 * 1024 * 1024,
            max_items: 100_000 }
    }
    pub fn bounded(duration: Duration) -> Self {
        Self::new(Instant::now() + duration, CancellationToken::new())
    }
    pub fn check(&self) -> Result<(), String> {
        if self.cancellation.is_cancelled() { return Err("ledger_cancelled".into()); }
        if Instant::now() >= self.deadline { return Err("ledger_deadline_exhausted".into()); }
        Ok(())
    }
    pub fn interrupted(&self) -> bool { self.check().is_err() }
    pub fn charge_bytes(&self, amount: usize) -> Result<(), String> {
        self.check()?;
        self.bytes.fetch_update(Ordering::Relaxed, Ordering::Relaxed,
            |n| n.checked_add(amount).filter(|total| *total <= self.max_bytes))
            .map(|_| ()).map_err(|_| "ledger_source_byte_budget_exhausted".into())
    }
    pub fn visit(&self) -> Result<(), String> {
        self.check()?;
        self.items.fetch_update(Ordering::Relaxed, Ordering::Relaxed,
            |n| n.checked_add(1).filter(|total| *total <= self.max_items))
            .map(|_| ()).map_err(|_| "ledger_item_budget_exhausted".into())
    }
    pub fn consumed_bytes(&self) -> usize { self.bytes.load(Ordering::Relaxed) }
}
