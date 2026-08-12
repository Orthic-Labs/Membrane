use std::collections::HashMap;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use crate::schema_types::ManifestV1;

/// Exponential backoff schedule for child supervision (R-14).
/// Proposed: 5 attempts, 250ms base, doubling, capped at 8s.
const MAX_ATTEMPTS: usize = 5;
const BASE_DELAY_MS: u64 = 250;
const MAX_DELAY_MS: u64 = 8000;

pub fn backoff_delay(attempt: usize) -> Duration {
    // attempt 0 -> 250ms, 1 -> 500ms, 2 -> 1000ms, 3 -> 2000ms, 4 -> 4000ms, 5 -> 8000ms capped
    let delay = BASE_DELAY_MS * (1u64 << attempt.min(10));
    Duration::from_millis(delay.min(MAX_DELAY_MS))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProductStatus {
    Running,
    Unavailable,
}

pub struct Supervisor {
    children: Arc<Mutex<HashMap<String, ManagedChild>>>,
}

/// A child owned by this Hub or a product unavailable under Hub ownership.
enum ManagedChild {
    Running(Child),
    Unavailable,
}

fn stop_owned_child(mut child: Child) {
    // Closing Hub-owned stdin is the portable graceful-stop contract for both
    // Membrane and Cortex. Kill is only a bounded fallback.
    drop(child.stdin.take());
    for _ in 0..20 {
        if child.try_wait().ok().flatten().is_some() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = child.kill();
    let _ = child.wait();
}

impl Supervisor {
    pub fn new() -> Self {
        Self { children: Arc::new(Mutex::new(HashMap::new())) }
    }

    pub fn start_product(&self, manifest: &ManifestV1) -> Result<ProductStatus, String> {
        let mut attempts = 0usize;
        loop {
            match Self::try_spawn(manifest) {
                Ok(child) => {
                    self.children.lock().map_err(|_| "lock_poisoned")?.insert(manifest.product_id.clone(), ManagedChild::Running(child));
                    return Ok(ProductStatus::Running);
                }
                Err(e) => {
                    attempts += 1;
                    if attempts >= MAX_ATTEMPTS {
                        self.children.lock().map_err(|_| "lock_poisoned")?.insert(manifest.product_id.clone(), ManagedChild::Unavailable);
                        return Ok(ProductStatus::Unavailable);
                    }
                    let delay = backoff_delay(attempts - 1);
                    std::thread::sleep(delay);
                    eprintln!("supervisor: retry {}/{} for {} after {}ms: {}", attempts, MAX_ATTEMPTS, manifest.product_id, delay.as_millis(), e);
                }
            }
        }
    }

    /// Restarts a child that exited since its previous liveness check.
    pub fn supervise_product(&self, manifest: &ManifestV1) -> Result<ProductStatus, String> {
        let state = {
            let mut children = self.children.lock().map_err(|_| "lock_poisoned")?;
            match children.get_mut(&manifest.product_id) {
                Some(ManagedChild::Running(child)) => {
                    if child.try_wait().map_err(|_| "service_wait_failed")?.is_some() {
                        None
                    } else {
                        Some(ProductStatus::Running)
                    }
                }
                Some(ManagedChild::Unavailable) | None => Some(ProductStatus::Unavailable),
            }
        };
        match state {
            Some(status) => Ok(status),
            None => self.start_product(manifest),
        }
    }

    fn try_spawn(manifest: &ManifestV1) -> Result<Child, String> {
        if manifest.service_start.is_empty() {
            return Err("serviceStart_empty".into());
        }
        let program = std::path::PathBuf::from(&manifest.service_start[0]);
        if !program.is_file() {
            return Err("service_missing".into());
        }
        let mut cmd = Command::new(&program);
        if manifest.service_start.len() > 1 {
            cmd.args(&manifest.service_start[1..]);
        }
        cmd.env("ORTHIC_HUB_CHILD", "1");
        // Membrane also requires its existing owner-pipe marker.
        if manifest.product_id == "membrane" {
            cmd.env("MEMBRANE_OWNER_PIPE", "1");
            if let Ok(root) = crate::workspace::resolve() {
                cmd.env("WORKSPACE_ROOT", root.root);
            }
        }
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut child = cmd.spawn().map_err(|_| "service_start_failed".to_string())?;
        std::thread::sleep(Duration::from_millis(120));
        if child.try_wait().map_err(|_| "service_wait_failed")?.is_some() {
            return Err("service_start_failed".into());
        }
        Ok(child)
    }

    pub fn stop_product(&self, product_id: &str) {
        if let Ok(mut map) = self.children.lock() {
            if let Some(ManagedChild::Running(child)) = map.remove(product_id) {
                stop_owned_child(child);
            } else {
                map.remove(product_id);
            }
        }
    }

    pub fn stop_all(&self) {
        if let Ok(mut map) = self.children.lock() {
            for (_, child) in map.drain() {
                if let ManagedChild::Running(c) = child {
                    stop_owned_child(c);
                }
            }
        }
    }

    pub fn is_unavailable(&self, product_id: &str) -> bool {
        // Check if marked unavailable (None without running process but we treat absence as unavailable after max attempts)
        // For now, if not in map, unavailable.
        self.children.lock().map(|m| matches!(m.get(product_id), Some(ManagedChild::Unavailable) | None)).unwrap_or(true)
    }
}

impl Default for Supervisor {
    fn default() -> Self { Self::new() }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn backoff_is_strictly_increasing_then_capped() {
        let delays: Vec<u64> = (0..6).map(|a| backoff_delay(a).as_millis() as u64).collect();
        // 0:250,1:500,2:1000,3:2000,4:4000,5:8000 (capped)
        assert_eq!(delays, vec![250,500,1000,2000,4000,8000]);
        // Further attempts stay capped
        assert_eq!(backoff_delay(10).as_millis(), 8000);
        for w in delays.windows(2) {
            assert!(w[0] <= w[1]);
            if w[1] < 8000 { assert!(w[0] < w[1]); }
        }
    }
    #[test]
    fn backoff_capped_after_max() {
        assert_eq!(backoff_delay(100).as_millis(), 8000);
    }
}
