//! Request-owned monotonic deadlines.
//!
//! A deadline is an absolute process-local instant.  This module keeps the
//! clock behind a tiny trait so deadline arithmetic can be tested without
//! sleeping or extending a request budget while work is queued.

use membrane_protocol::DeadlineBudget;
use std::time::{Duration, Instant};

/// Clock used when constructing and inspecting a deadline.
pub trait MonotonicClock: Send + Sync {
    fn now(&self) -> Instant;
}

/// The process monotonic clock used by production composition.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemClock;

impl MonotonicClock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

/// One absolute request deadline.  It intentionally cannot be serialized or
/// reconstructed from a later queue/start timestamp.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Deadline {
    at: Instant,
}

impl Deadline {
    /// Create one deadline from a request budget and the injected clock.
    pub fn from_budget<C: MonotonicClock + ?Sized>(clock: &C, budget: DeadlineBudget) -> Self {
        Self::after(clock, budget.as_duration())
    }

    /// Create one absolute deadline from the clock's current instant.
    pub fn after<C: MonotonicClock + ?Sized>(clock: &C, duration: Duration) -> Self {
        let now = clock.now();
        Self {
            // A u64 millisecond request budget is far below the practical
            // range of monotonic clocks.  If a platform nevertheless rejects
            // the addition, keep the clock instant rather than wrapping into
            // the past or inventing a second clock reading.
            at: now.checked_add(duration).unwrap_or(now),
        }
    }

    /// Construct a deadline from an already absolute instant.
    pub const fn at(at: Instant) -> Self {
        Self { at }
    }

    pub const fn instant(self) -> Instant {
        self.at
    }

    pub fn remaining_at(self, now: Instant) -> Duration {
        self.at.saturating_duration_since(now)
    }

    pub fn remaining<C: MonotonicClock + ?Sized>(self, clock: &C) -> Duration {
        self.remaining_at(clock.now())
    }

    pub fn is_exhausted_at(self, now: Instant) -> bool {
        now >= self.at
    }

    pub fn is_exhausted<C: MonotonicClock + ?Sized>(self, clock: &C) -> bool {
        self.is_exhausted_at(clock.now())
    }

    /// Content-free deadline metric, rounded down to milliseconds.
    pub fn remaining_ms_at(self, now: Instant) -> u64 {
        self.remaining_at(now).as_millis().min(u128::from(u64::MAX)) as u64
    }
}

/// Remaining time observed at one instant.  Keeping the observation together
/// avoids accidentally taking a second clock reading between a check and a
/// launch decision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Remaining {
    pub duration: Duration,
    pub exhausted: bool,
}

impl Remaining {
    pub fn observe(deadline: Deadline, now: Instant) -> Self {
        let duration = deadline.remaining_at(now);
        Self {
            exhausted: duration.is_zero(),
            duration,
        }
    }

    pub fn milliseconds(self) -> u64 {
        self.duration.as_millis().min(u128::from(u64::MAX)) as u64
    }
}

pub fn remaining_until(deadline: Deadline, now: Instant) -> Duration {
    deadline.remaining_at(now)
}
