//! Planner metrics: rolling latency ring + last-fallback snapshot.
//!
//! Used by `/plan_context` and surfaced (without repository content) via
//! `/health`. The lock is acquired only briefly when recording or reading —
//! the rolling windows are bounded (256 samples), so the snapshot is O(N).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const LATENCY_WINDOW: usize = 256;

pub struct PlannerLatency {
    samples: Mutex<[u64; LATENCY_WINDOW]>,
    index: Mutex<usize>,
    count: AtomicU64,
}

impl PlannerLatency {
    pub fn new() -> Self {
        Self {
            samples: Mutex::new([0u64; LATENCY_WINDOW]),
            index: Mutex::new(0),
            count: AtomicU64::new(0),
        }
    }

    /// Record a single sample (microseconds). Cheap: one mutex lock + one
    /// atomic increment. Bounded — older samples roll out of the window.
    pub fn record(&self, elapsed: Duration) {
        let us = elapsed.as_micros().min(u64::MAX as u128) as u64;
        let (mut samples, mut index) = (self.samples.lock().unwrap(), self.index.lock().unwrap());
        samples[*index] = us;
        *index = (*index + 1) % LATENCY_WINDOW;
        self.count.fetch_add(1, Ordering::Relaxed);
    }

    /// Returns `(count, p50_us, p95_us)`. If fewer than 2 samples exist,
    /// percentiles reflect whatever is on hand (and `count` is honest).
    pub fn snapshot(&self) -> (u64, u64, u64) {
        let count = self.count.load(Ordering::Relaxed);
        let (samples, _index) = (self.samples.lock().unwrap(), self.index.lock().unwrap());
        let observed = count.min(LATENCY_WINDOW as u64) as usize;
        if observed == 0 {
            return (0, 0, 0);
        }
        let mut sorted: Vec<u64> = samples[..observed].to_vec();
        sorted.sort_unstable();
        let p50 = sorted[(observed.saturating_sub(1) * 50 / 100).min(observed - 1)];
        let p95 = sorted[(observed.saturating_sub(1) * 95 / 100).min(observed - 1)];
        (count, p50, p95)
    }
}

#[derive(Default)]
pub struct LastFallback {
    inner: Mutex<LastFallbackInner>,
}

#[derive(Default, Clone)]
struct LastFallbackInner {
    reason: String,
    mode: String,
    provider_status: String,
    at_unix: i64,
}

impl LastFallback {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record(&self, reason: &str, mode: &str, provider_status: &str) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let mut inner = self.inner.lock().unwrap();
        inner.reason.clear();
        inner.reason.push_str(reason);
        inner.mode.clear();
        inner.mode.push_str(mode);
        inner.provider_status.clear();
        inner.provider_status.push_str(provider_status);
        inner.at_unix = now;
    }

    pub fn snapshot(&self) -> LastFallbackSnapshot {
        let inner = self.inner.lock().unwrap().clone();
        LastFallbackSnapshot {
            reason: inner.reason,
            mode: inner.mode,
            provider_status: inner.provider_status,
            at_unix: inner.at_unix,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LastFallbackSnapshot {
    pub reason: String,
    pub mode: String,
    pub provider_status: String,
    pub at_unix: i64,
}

impl LastFallbackSnapshot {
    pub fn is_set(&self) -> bool {
        !self.reason.is_empty() || !self.mode.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_window_reports_zero() {
        let l = PlannerLatency::new();
        let (count, p50, p95) = l.snapshot();
        assert_eq!((count, p50, p95), (0, 0, 0));
    }

    #[test]
    fn percentiles_stable_under_load() {
        let l = PlannerLatency::new();
        for us in 100..(100 + LATENCY_WINDOW as u64) {
            l.record(Duration::from_micros(us));
        }
        let (count, p50, p95) = l.snapshot();
        assert_eq!(count, LATENCY_WINDOW as u64);
        // Window is fully populated — p50 ≈ sample at 50%.
        let mut sorted: Vec<u64> = (100..(100 + LATENCY_WINDOW as u64)).collect();
        sorted.sort_unstable();
        assert_eq!(p50, sorted[sorted.len() / 2 - 1]);
        assert!(p95 >= p50);
    }

    #[test]
    fn last_fallback_round_trip() {
        let f = LastFallback::new();
        f.record("blueprint_stale", "portable_text_only", "stale");
        let snap = f.snapshot();
        assert!(snap.is_set());
        assert_eq!(snap.reason, "blueprint_stale");
        assert_eq!(snap.mode, "portable_text_only");
    }
}
