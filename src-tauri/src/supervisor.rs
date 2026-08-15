//! O2 lifecycle engine — product-neutral supervision.
//!
//! This module owns the Hub side of `orthic.lifecycle.v1` (§4.2 of the seam
//! contract). One [`Supervisor`] owns every product child process: it spawns
//! products through their declared `serviceStart` argv (never a shell), binds
//! each child to an inherited authenticated channel, and enforces the
//! lifecycle invariants the seam assigns to Orthic:
//!
//! - **No product-ID branching.** Spawn behaviour derives entirely from the
//!   manifest + runtime lifecycle data. There is no `membrane`/`cortex`
//!   discriminator anywhere in the spawn path.
//! - **Inherited authenticated channel.** A fresh, unpredictable capability
//!   secret is generated at spawn time and written *only* into the child's
//!   inherited stdin pipe (the hello frame). It never appears in the static
//!   manifest, argv, environment, logs, crash dumps, or snapshots.
//! - **Declared stop argv is honoured.** `serviceStop` is executed as declared
//!   before the bounded drain; it is never accepted then ignored.
//! - **Monotonic fence, one owner, artifact digest, readiness deadline,
//!   capped restart/backoff, crash-loop state, update handoff, parent-death
//!   exit, and a zero-orphan census.**

use crate::schema_types::ManifestV1;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Default restart/backoff schedule (R-14): 5 attempts, 250 ms base, doubling,
/// capped at 8 s.
const DEFAULT_MAX_ATTEMPTS: usize = 5;
const DEFAULT_BASE_DELAY_MS: u64 = 250;
const DEFAULT_MAX_DELAY_MS: u64 = 8000;

const LIFECYCLE_VERSION: u32 = 1;
const DEFAULT_READY_TIMEOUT: Duration = Duration::from_secs(3);
const DEFAULT_DRAIN_TIMEOUT: Duration = Duration::from_secs(3);
const DEFAULT_CRASH_WINDOW: Duration = Duration::from_secs(60);
const DEFAULT_CRASH_LOOP_THRESHOLD: usize = 3;

/// Snapshot capability header used when fetching a registered snapshot
/// endpoint. The token is the ephemeral capability the child reported over the
/// lifecycle channel — never the static manifest.
pub const SNAPSHOT_CAPABILITY_HEADER: &str = "X-Orthic-Capability";

/// Tunable lifecycle policy. Production uses [`SupervisorPolicy::default`];
/// tests may shrink the delays to keep the suite fast.
#[derive(Debug, Clone, Copy)]
pub struct SupervisorPolicy {
    pub max_attempts: usize,
    pub base_delay: Duration,
    pub max_delay: Duration,
    pub ready_timeout: Duration,
    pub drain_timeout: Duration,
    pub crash_window: Duration,
    pub crash_loop_threshold: usize,
}

impl Default for SupervisorPolicy {
    fn default() -> Self {
        Self {
            max_attempts: DEFAULT_MAX_ATTEMPTS,
            base_delay: Duration::from_millis(DEFAULT_BASE_DELAY_MS),
            max_delay: Duration::from_millis(DEFAULT_MAX_DELAY_MS),
            ready_timeout: DEFAULT_READY_TIMEOUT,
            drain_timeout: DEFAULT_DRAIN_TIMEOUT,
            crash_window: DEFAULT_CRASH_WINDOW,
            crash_loop_threshold: DEFAULT_CRASH_LOOP_THRESHOLD,
        }
    }
}

/// Deterministic default backoff schedule: `250, 500, 1000, 2000, 4000, 8000…`
/// (capped at 8 s). Kept as a free function so the schedule is directly
/// testable.
pub fn backoff_delay(attempt: usize) -> Duration {
    backoff_delay_with(attempt, Duration::from_millis(DEFAULT_BASE_DELAY_MS), Duration::from_millis(DEFAULT_MAX_DELAY_MS))
}

fn backoff_delay_with(attempt: usize, base: Duration, cap: Duration) -> Duration {
    let base_ms = base.as_millis() as u64;
    let cap_ms = cap.as_millis() as u64;
    let delay = base_ms.saturating_mul(1u64 << attempt.min(32));
    Duration::from_millis(delay.min(cap_ms))
}

/// Closed lifecycle states (`orthic.lifecycle.v1` §4.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChildState {
    Starting,
    Ready,
    Degraded,
    Draining,
    Stopped,
    Incompatible,
    Failed,
    CrashLoop,
}

/// Coarse status surfaced to the Hub runtime/tray.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProductStatus {
    Running,
    Unavailable,
    CrashLoop,
}

/// A loopback snapshot endpoint registered by a child over the lifecycle
/// channel. Never sourced from the static manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoopbackEndpoint {
    pub host: String,
    pub port: u16,
}

/// Zero-orphan census result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Census {
    pub tracked: usize,
    pub live: usize,
    pub crash_looped: usize,
}

struct ManagedChild {
    child: Child,
    stdin: Option<ChildStdin>,
    lifecycle_output: Receiver<String>,
    stop_argv: Vec<String>,
    fence: u64,
    instance_id: String,
    artifact_digest: String,
    endpoint: Option<LoopbackEndpoint>,
    capability: Option<String>,
    state: ChildState,
    spawned_at: Instant,
}

#[derive(Default)]
struct Shared {
    children: HashMap<String, ManagedChild>,
    fences: HashMap<String, u64>,
    crashes: HashMap<String, usize>,
    unavailable: HashSet<String>,
}

pub struct Supervisor {
    shared: Arc<Mutex<Shared>>,
    policy: SupervisorPolicy,
}

impl Supervisor {
    pub fn new() -> Self {
        Self::with_policy(SupervisorPolicy::default())
    }

    pub fn with_policy(policy: SupervisorPolicy) -> Self {
        Self { shared: Arc::new(Mutex::new(Shared::default())), policy }
    }

    /// Spawn the product and supervise it through hello/ready. Idempotent:
    /// a live child is never double-spawned (one owner).
    pub fn start_product(&self, manifest: &ManifestV1) -> Result<ProductStatus, String> {
        if self.is_running(&manifest.product_id) {
            return Ok(ProductStatus::Running);
        }
        let mut attempts = 0usize;
        loop {
            match self.try_spawn(manifest) {
                Ok(managed) => {
                    let mut shared = self.shared.lock().map_err(|_| "lock_poisoned")?;
                    shared.unavailable.remove(&manifest.product_id);
                    if let Some(old) = shared.children.insert(manifest.product_id.clone(), managed) {
                        reap_owned(old);
                    }
                    return Ok(ProductStatus::Running);
                }
                Err(error) => {
                    attempts += 1;
                    if attempts >= self.policy.max_attempts {
                        self.shared
                            .lock()
                            .map_err(|_| "lock_poisoned")?
                            .unavailable
                            .insert(manifest.product_id.clone());
                        return Ok(ProductStatus::Unavailable);
                    }
                    let delay = backoff_delay_with(attempts - 1, self.policy.base_delay, self.policy.max_delay);
                    std::thread::sleep(delay);
                    // Secret-free diagnostic: error strings never carry the
                    // channel secret or the snapshot capability.
                    eprintln!(
                        "supervisor: retry {}/{} for {} after {}ms: {}",
                        attempts,
                        self.policy.max_attempts,
                        manifest.product_id,
                        delay.as_millis(),
                        error
                    );
                }
            }
        }
    }

    /// Restart a child that exited since its previous liveness check, or
    /// escalate to a crash-loop state once the threshold is crossed.
    pub fn supervise_product(&self, manifest: &ManifestV1) -> Result<ProductStatus, String> {
        // Phase 1: inspect child liveness under a short-lived borrow.
        let lived = {
            let mut shared = self.shared.lock().map_err(|_| "lock_poisoned")?;
            if !shared.children.contains_key(&manifest.product_id) {
                drop(shared);
                return self.start_product(manifest);
            }
            let managed = shared
                .children
                .get_mut(&manifest.product_id)
                .ok_or("service_missing_after_lookup")?;
            if managed.state == ChildState::CrashLoop {
                return Ok(ProductStatus::CrashLoop);
            }
            // Give a just-launched process one scheduler turn before declaring
            // it healthy. Children may write `ready` and exit immediately
            // (crash-loop fixtures & real startup failures); a single
            // non-blocking poll can otherwise race that exit and defer crash
            // accounting until a later hub tick.
            let mut exited = managed.child.try_wait().map_err(|_| "service_wait_failed")?.is_some();
            if !exited {
                std::thread::sleep(Duration::from_millis(1));
                exited = managed.child.try_wait().map_err(|_| "service_wait_failed")?.is_some();
            }
            if !exited {
                return Ok(ProductStatus::Running);
            }
            managed.spawned_at.elapsed()
        };

        // Phase 2: record the crash and decide (no child borrow held here).
        let mark_crash_loop = {
            let mut shared = self.shared.lock().map_err(|_| "lock_poisoned")?;
            let previous = shared.crashes.get(&manifest.product_id).copied().unwrap_or(0);
            // A child that survived past the crash window resets the counter:
            // one late crash is not a loop.
            let crash_count = if lived >= self.policy.crash_window { 1 } else { previous + 1 };
            shared.crashes.insert(manifest.product_id.clone(), crash_count);
            crash_count >= self.policy.crash_loop_threshold
        };

        if mark_crash_loop {
            if let Ok(mut shared) = self.shared.lock() {
                if let Some(managed) = shared.children.get_mut(&manifest.product_id) {
                    managed.state = ChildState::CrashLoop;
                }
            }
            return Ok(ProductStatus::CrashLoop);
        }

        // Phase 3: reap the dead child and respawn.
        let dead = self
            .shared
            .lock()
            .map_err(|_| "lock_poisoned")?
            .children
            .remove(&manifest.product_id);
        if let Some(dead) = dead {
            reap_owned(dead);
        }
        self.start_product(manifest)
    }

    /// Gracefully stop one product: run the declared `serviceStop` argv, then
    /// close the inherited channel (drain), wait a bounded drain window, and
    /// finally terminate and reap the child.
    pub fn stop_product(&self, product_id: &str) {
        let managed = self.shared.lock().ok().and_then(|mut shared| shared.children.remove(product_id));
        if let Some(mut managed) = managed {
            self.stop_managed(&mut managed, "stop");
        }
    }

    /// Stop every tracked child; a census afterwards must report zero live
    /// children.
    pub fn stop_all(&self) {
        let children = self.shared.lock().ok().map(|mut shared| shared.children.drain().map(|(_, c)| c).collect::<Vec<_>>());
        if let Some(children) = children {
            for mut managed in children {
                self.stop_managed(&mut managed, "ownership_loss");
            }
        }
    }

    /// Update handoff: drain and stop a product so a replacement version can
    /// take over. Returns the child's last known state (or `Stopped` when it
    /// was not tracked).
    pub fn handoff_for_update(&self, product_id: &str) -> Result<ChildState, String> {
        let managed = self.shared.lock().map_err(|_| "lock_poisoned")?.children.remove(product_id);
        match managed {
            Some(mut managed) => {
                let last = managed.state;
                self.stop_managed(&mut managed, "update_handoff");
                Ok(last)
            }
            None => Ok(ChildState::Stopped),
        }
    }

    /// Registered loopback snapshot endpoint for a running child, if any.
    pub fn endpoint(&self, product_id: &str) -> Option<LoopbackEndpoint> {
        self.shared
            .lock()
            .ok()
            .and_then(|shared| shared.children.get(product_id).and_then(|m| m.endpoint.clone()))
    }

    /// Ephemeral snapshot capability reported over the lifecycle channel.
    pub fn capability(&self, product_id: &str) -> Option<String> {
        self.shared
            .lock()
            .ok()
            .and_then(|shared| shared.children.get(product_id).and_then(|m| m.capability.clone()))
    }

    pub fn status(&self, product_id: &str) -> ProductStatus {
        let mut shared = match self.shared.lock() {
            Ok(shared) => shared,
            Err(_) => return ProductStatus::Unavailable,
        };
        match shared.children.get_mut(product_id) {
            Some(managed) if managed.state == ChildState::CrashLoop => ProductStatus::CrashLoop,
            Some(managed) => {
                if managed.child.try_wait().ok().flatten().is_none() {
                    ProductStatus::Running
                } else {
                    ProductStatus::Unavailable
                }
            }
            None => ProductStatus::Unavailable,
        }
    }

    pub fn is_unavailable(&self, product_id: &str) -> bool {
        matches!(self.status(product_id), ProductStatus::Unavailable)
    }

    /// Zero-orphan census: how many children are tracked, still live, and
    /// currently crash-looped.
    pub fn census(&self) -> Census {
        let mut shared = match self.shared.lock() {
            Ok(shared) => shared,
            Err(_) => return Census { tracked: 0, live: 0, crash_looped: 0 },
        };
        let tracked = shared.children.len();
        let mut live = 0usize;
        let mut crash_looped = 0usize;
        for managed in shared.children.values_mut() {
            if managed.state == ChildState::CrashLoop {
                crash_looped += 1;
            }
            if managed.child.try_wait().ok().flatten().is_none() {
                live += 1;
                // Count live descendants too: a child that spawned a
                // sub-process (or a crash-loop survivor) is visible as a
                // residual orphan, never silently reported as zero.
                live += live_descendants(managed.child.id());
            }
        }
        Census { tracked, live, crash_looped }
    }

    fn is_running(&self, product_id: &str) -> bool {
        let mut shared = match self.shared.lock() {
            Ok(shared) => shared,
            Err(_) => return false,
        };
        match shared.children.get_mut(product_id) {
            Some(managed) if managed.state == ChildState::CrashLoop => false,
            Some(managed) => managed.child.try_wait().ok().flatten().is_none(),
            None => false,
        }
    }

    fn stop_managed(&self, managed: &mut ManagedChild, command: &str) {
        // 1. Send an exact-fence lifecycle command & wait only through the
        // bounded drain window for its matching acknowledgement.
        let _ = send_lifecycle_command(managed, command, self.policy.drain_timeout);
        // 2. Honor declared stop argv after child acknowledges drain intent.
        run_declared_stop(&managed.stop_argv, self.policy.drain_timeout);
        // 3. EOF remains parent-death backstop after command delivery.
        drop(managed.stdin.take());
        managed.state = ChildState::Draining;
        // 4. Bounded drain.
        let deadline = Instant::now() + self.policy.drain_timeout;
        while Instant::now() < deadline {
            if managed.child.try_wait().ok().flatten().is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        // 5. Terminate the full child tree and reap it. On Unix the child was
        //    spawned into its own process group, so we signal the whole group
        //    (graceful SIGTERM, then SIGKILL) so no descendant is orphaned.
        terminate_tree(&mut managed.child, self.policy.drain_timeout);
        managed.state = ChildState::Stopped;
    }

    fn try_spawn(&self, manifest: &ManifestV1) -> Result<ManagedChild, String> {
        if manifest.service_start.is_empty() {
            return Err("serviceStart_empty".into());
        }
        let program = PathBuf::from(&manifest.service_start[0]);
        if !program.is_file() {
            return Err("service_missing".into());
        }
        // Bind actual artifact bytes to manifest v2 before launch.
        let artifact_digest = sha256_file(&program).map_err(|_| "artifact_digest_unavailable")?;
        if manifest.artifact_digest != artifact_digest {
            return Err("artifact_digest_mismatch".into());
        }
        let instance_id = new_instance_id(&manifest.product_id);
        let fence = self.next_fence(&manifest.product_id);
        let secret = generate_secret().map_err(|_| "channel_secret_unavailable")?;
        let declared_root = PathBuf::from(&manifest.install_root)
            .canonicalize()
            .map_err(|_| "declared_data_root_unavailable")?;
        let declared_root_text = declared_root.to_string_lossy().into_owned();

        let mut cmd = Command::new(&program);
        if manifest.service_start.len() > 1 {
            cmd.args(&manifest.service_start[1..]);
        }
        // Generic, product-neutral marker only — no discriminator, no secret.
        cmd.env("ORTHIC_LIFECYCLE_STDIO", "1");
        cmd.env("WORKSPACE_ROOT", &declared_root);
        cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
        // Isolate the child into its own process group (Unix) so terminal
        // signals never fall through to product children.
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }

        let mut child = cmd.spawn().map_err(|_| "service_start_failed".to_string())?;

        // Write the hello frame into the inherited channel. The secret lives
        // only on this pipe — never argv/env/manifest/logs/snapshots.
        let hello = serde_json::json!({
            "kind": "hello",
            "lifecycleVersion": LIFECYCLE_VERSION,
            "installationId": installation_id(&declared_root_text),
            "productId": manifest.product_id,
            "instanceId": instance_id,
            "fence": fence,
            "artifactDigest": artifact_digest,
            "declaredDataRoot": declared_root_text,
            "secret": secret,
        });
        let mut stdin = child.stdin.take().ok_or("channel_missing")?;
        writeln!(stdin, "{}", hello).map_err(|_| "channel_write_failed")?;
        stdin.flush().map_err(|_| "channel_write_failed")?;

        // Readiness deadline: the child must register its endpoint before the
        // bound expires or the spawn fails and the child is reaped.
        let (registration, lifecycle_output) = match read_registration(child.stdout.take(), self.policy.ready_timeout) {
            Ok(registration) => registration,
            Err(error) => {
                terminate_tree(&mut child, Duration::from_millis(250));
                return Err(error);
            }
        };
        let (endpoint, capability, state) = match validate_registration(&registration, fence) {
            Ok(registration) => registration,
            Err(error) => {
                terminate_tree(&mut child, Duration::from_millis(250));
                return Err(error);
            }
        };
        if !matches!(state, ChildState::Ready | ChildState::Degraded) {
            terminate_tree(&mut child, Duration::from_millis(250));
            return Err("registration_not_admitted".into());
        }

        Ok(ManagedChild {
            child,
            stdin: Some(stdin),
            lifecycle_output,
            stop_argv: manifest.service_stop.clone(),
            fence,
            instance_id,
            artifact_digest,
            endpoint,
            capability,
            state,
            spawned_at: Instant::now(),
        })
    }

    fn next_fence(&self, product_id: &str) -> u64 {
        let mut shared = self.shared.lock().expect("fence lock poisoned");
        let entry = shared.fences.entry(product_id.to_string()).or_insert(0);
        // Monotonic: the fence only ever increases for a given product.
        *entry = entry.saturating_add(1);
        *entry
    }
}

impl Default for Supervisor {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Supervisor {
    fn drop(&mut self) {
        // Parent-death backstop: if the supervisor is dropped while children
        // are still owned, drain them. The authoritative parent-death signal
        // is the inherited-channel EOF the child observes when the Hub dies.
        self.stop_all();
    }
}

/// Signal a child's entire process group on Unix. The child is spawned with
/// `process_group(0)`, so its pgid equals its pid; delivering to `-(pgid)`
/// reaches the product and every descendant it spawned. Returns `false` when
/// the group signal could not be delivered (non-Unix, pid already recycled, or
/// the `kill` utility is unavailable), in which case the caller falls back to
/// the direct child only.
#[cfg(unix)]
fn signal_process_group(child: &Child, sig: &str) -> bool {
    let pid = child.id() as i32;
    if pid <= 1 {
        return false;
    }
    std::process::Command::new("kill")
        .arg(format!("-{sig}"))
        .arg(format!("-{pid}"))
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(windows)]
fn signal_process_group(child: &Child, _sig: &str) -> bool {
    // Windows has no POSIX process groups. `taskkill /T` is the native
    // equivalent: terminate the direct child and every descendant in its
    // tree, preventing an orphan when a product spawned helpers.
    std::process::Command::new("taskkill")
        .args(["/PID", &child.id().to_string(), "/T", "/F"])
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

#[cfg(all(not(unix), not(windows)))]
fn signal_process_group(_child: &Child, _sig: &str) -> bool {
    false
}

/// Terminate a child and its full descendant tree, then reap the direct child.
/// Graceful group SIGTERM first (bounded grace), then group SIGKILL for
/// anything still alive. On non-Unix this degrades to direct-child kill only.
fn terminate_tree(child: &mut Child, grace: Duration) {
    if child.try_wait().ok().flatten().is_some() {
        return; // already exited and reaped
    }
    let _ = signal_process_group(child, "TERM");
    let deadline = Instant::now() + grace;
    let mut reaped = false;
    while !reaped && Instant::now() < deadline {
        match child.try_wait() {
            Ok(Some(_)) => reaped = true,
            _ => std::thread::sleep(Duration::from_millis(20)),
        }
    }
    // Force the whole remaining group (descendants that ignored SIGTERM), then
    // reap the direct child if it is still alive.
    let _ = signal_process_group(child, "KILL");
    if !reaped {
        let _ = child.kill();
        let _ = child.wait();
    }
}

/// Count live descendants (transitive) of `pid` on Unix, bounded to avoid a
/// runaway process-tree walk. Used by [`Supervisor::census`] so a residual
/// orphan descendant is never reported as zero.
#[cfg(unix)]
fn live_descendants(pid: u32) -> usize {
    let mut queue = std::collections::VecDeque::new();
    queue.push_back(pid);
    let mut count = 0usize;
    let mut budget = 4096usize;
    while let Some(parent) = queue.pop_front() {
        if budget == 0 {
            break;
        }
        budget -= 1;
        let Ok(out) = std::process::Command::new("pgrep").arg("-P").arg(parent.to_string()).output() else {
            continue;
        };
        if !out.status.success() {
            continue;
        }
        for line in String::from_utf8_lossy(&out.stdout).lines() {
            let line = line.trim();
            if let Ok(pid) = line.parse::<u32>() {
                count += 1;
                queue.push_back(pid);
            }
        }
    }
    count
}

#[cfg(windows)]
fn live_descendants(pid: u32) -> usize {
    let script = format!(
        "$p=@({pid}); $n=0; while($p.Count) {{ $c=@(Get-CimInstance Win32_Process -Filter ('ParentProcessId='+$p[0]) -ErrorAction SilentlyContinue | %% ProcessId); $p=$p[1..($p.Count-1)]; if($c) {{ $n += $c.Count; $p += $c }} }}; $n"
    );
    std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .and_then(|text| text.trim().parse::<usize>().ok())
        .unwrap_or(0)
}

#[cfg(all(not(unix), not(windows)))]
fn live_descendants(_pid: u32) -> usize { 0 }

/// Reap a replaced/removed child and its full tree so no zombie survives.
fn reap_owned(mut managed: ManagedChild) {
    drop(managed.stdin.take());
    terminate_tree(&mut managed.child, Duration::from_secs(1));
    managed.state = ChildState::Stopped;
}

/// Execute the declared `serviceStop` argv (argv exec, never a shell) with a
/// bounded wait; kill it if it overruns.
fn run_declared_stop(argv: &[String], timeout: Duration) {
    let Some((program, args)) = argv.split_first() else {
        return;
    };
    let mut cmd = Command::new(program);
    cmd.args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    let Ok(mut child) = cmd.spawn() else {
        return;
    };
    let deadline = Instant::now() + timeout;
    loop {
        if child.try_wait().ok().flatten().is_some() {
            return;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Preserve every stdout frame after readiness. A single reader owns the pipe
/// for its lifetime so an acknowledgement already buffered behind `register`
/// remains available to the supervisor.
fn read_registration(
    stdout: Option<impl Read + Send + 'static>,
    deadline: Duration,
) -> Result<(String, Receiver<String>), String> {
    let stdout = stdout.ok_or("channel_missing")?;
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            let Ok(line) = line else { break };
            if sender.send(line).is_err() { break; }
        }
    });
    match receiver.recv_timeout(deadline) {
        Ok(line) if !line.trim().is_empty() => Ok((line, receiver)),
        Ok(_) => Err("registration_empty".into()),
        Err(RecvTimeoutError::Timeout) => Err("readiness_deadline_exceeded".into()),
        Err(RecvTimeoutError::Disconnected) => Err("registration_channel_closed".into()),
    }
}

/// Deliver one lifecycle command and accept only its matching exact-fence ack.
/// Unrelated buffered stdout frames are ignored until this bounded receipt
/// window closes.
fn send_lifecycle_command(managed: &mut ManagedChild, command: &str, timeout: Duration) -> bool {
    let Some(stdin) = managed.stdin.as_mut() else { return false };
    let frame = serde_json::json!({"kind":"command","command":command,"fence":managed.fence});
    if writeln!(stdin, "{frame}").is_err() || stdin.flush().is_err() {
        return false;
    }
    let deadline = Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() { return false; }
        match managed.lifecycle_output.recv_timeout(remaining) {
            Ok(line) => {
                let Ok(ack) = serde_json::from_str::<serde_json::Value>(&line) else { continue };
                if ack.get("kind").and_then(serde_json::Value::as_str) == Some("ack")
                    && ack.get("command").and_then(serde_json::Value::as_str) == Some(command)
                    && ack.get("fence").and_then(serde_json::Value::as_u64) == Some(managed.fence)
                {
                    return true;
                }
            }
            Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => return false,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LifecycleRegistration {
    kind: Option<String>,
    state: Option<String>,
    endpoint: Option<RegistrationEndpoint>,
    capability: Option<String>,
    fence: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RegistrationEndpoint {
    host: Option<String>,
    port: Option<u16>,
}

/// Validate a registration line against the fence we issued. A stale fence is
/// rejected; a non-loopback endpoint is rejected; an unknown state is
/// rejected. Fails closed, never guesses.
fn validate_registration(
    line: &str,
    expected_fence: u64,
) -> Result<(Option<LoopbackEndpoint>, Option<String>, ChildState), String> {
    let registration: LifecycleRegistration =
        serde_json::from_str(line.trim()).map_err(|_| "registration_schema_invalid")?;
    if registration.kind.as_deref() != Some("register") {
        return Err("registration_kind_invalid".into());
    }
    if registration.fence != expected_fence {
        return Err("stale_fence".into());
    }
    let state = match registration.state.as_deref() {
        Some("starting") => ChildState::Starting,
        Some("ready") => ChildState::Ready,
        Some("degraded") => ChildState::Degraded,
        Some("incompatible") => ChildState::Incompatible,
        Some("failed") => ChildState::Failed,
        _ => return Err("registration_state_invalid".into()),
    };
    let endpoint = match registration.endpoint {
        Some(endpoint) => {
            let host = endpoint.host.unwrap_or_else(|| "127.0.0.1".to_string());
            if host != "127.0.0.1" && host != "localhost" && host != "::1" {
                return Err("endpoint_not_loopback".into());
            }
            let port = endpoint.port.ok_or("endpoint_port_missing")?;
            if port == 0 {
                return Err("endpoint_port_invalid".into());
            }
            Some(LoopbackEndpoint { host, port })
        }
        None => None,
    };
    let capability = registration
        .capability
        .filter(|value| !value.is_empty() && value.len() <= 256);
    if state == ChildState::Ready && (endpoint.is_none() || capability.is_none()) {
        return Err("ready_registration_incomplete".into());
    }
    Ok((endpoint, capability, state))
}

/// Generate a fresh capability secret from the host CSPRNG.
fn generate_secret() -> Result<String, String> {
    #[cfg(unix)]
    {
        let mut bytes = [0u8; 32];
        let mut file = std::fs::File::open("/dev/urandom").map_err(|_| "entropy_unavailable")?;
        file.read_exact(&mut bytes).map_err(|_| "entropy_unavailable")?;
        return Ok(hex_encode(&bytes));
    }
    #[cfg(windows)]
    {
        let mut bytes = [0u8; 32];
        // `BCryptGenRandom` is Windows' system CSPRNG and requires no extra
        // crate or process invocation.
        use std::ptr::null_mut;
        #[link(name = "bcrypt")]
        extern "system" { fn BCryptGenRandom(h: *mut std::ffi::c_void, p: *mut u8, n: u32, flags: u32) -> i32; }
        const BCRYPT_USE_SYSTEM_PREFERRED_RNG: u32 = 0x00000002;
        let status = unsafe { BCryptGenRandom(null_mut(), bytes.as_mut_ptr(), bytes.len() as u32, BCRYPT_USE_SYSTEM_PREFERRED_RNG) };
        if status != 0 { return Err("entropy_unavailable".into()); }
        Ok(hex_encode(&bytes))
    }
}

fn new_instance_id(product_id: &str) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nonce = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{product_id}-{nonce:016x}")
}

fn installation_id(install_root: &str) -> String {
    format!("sha256:{}", hex_encode(&sha256(install_root.as_bytes())))
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file = std::fs::File::open(path).map_err(|_| "artifact_read_failed")?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(|_| "artifact_read_failed")?;
    Ok(format!("sha256:{}", hex_encode(&sha256(&bytes))))
}

/// Minimal, dependency-free SHA-256 (FIPS 180-4). Used to bind the launched
/// artifact bytes into the lifecycle hello frame.
fn sha256(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
        0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
        0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
        0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
        0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
        0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
    ];
    let mut message = data.to_vec();
    let bit_len = (data.len() as u64).wrapping_mul(8);
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bit_len.to_be_bytes());

    for block in message.chunks_exact(64) {
        let mut w = [0u32; 64];
        for (index, chunk) in block.chunks_exact(4).enumerate() {
            w[index] = u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        }
        for index in 16..64 {
            let s0 = w[index - 15].rotate_right(7) ^ w[index - 15].rotate_right(18) ^ (w[index - 15] >> 3);
            let s1 = w[index - 2].rotate_right(17) ^ w[index - 2].rotate_right(19) ^ (w[index - 2] >> 10);
            w[index] = w[index - 16].wrapping_add(s0).wrapping_add(w[index - 7]).wrapping_add(s1);
        }
        let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh) =
            (h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]);
        for index in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[index])
                .wrapping_add(w[index]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }

    let mut out = [0u8; 32];
    for (index, value) in h.iter().enumerate() {
        out[index * 4..index * 4 + 4].copy_from_slice(&value.to_be_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    fn fast_policy() -> SupervisorPolicy {
        SupervisorPolicy {
            max_attempts: 2,
            base_delay: Duration::from_millis(1),
            max_delay: Duration::from_millis(2),
            ready_timeout: Duration::from_secs(2),
            drain_timeout: Duration::from_millis(300),
            crash_window: Duration::from_secs(5),
            crash_loop_threshold: 3,
        }
    }

    fn manifest(start: Vec<String>, stop: Vec<String>) -> ManifestV1 {
        let artifact_digest = start.first().and_then(|path| sha256_file(Path::new(path)).ok());
        ManifestV1 {
            schema_version: 2,
            product_id: "sample".into(),
            display_name: "Sample".into(),
            product_version: "1".into(),
            hub_compat_range: ">=0".into(),
            install_root: "/tmp".into(),
            service_start: start,
            service_stop: stop,
            icon: "/tmp/icon".into(),
            artifact_digest: artifact_digest.unwrap_or_default(),
        }
    }

    /// A child that reads the hello line, echoes its argv + env to files, then
    /// registers and sleeps.
    fn cooperative_script(channel_file: &str, argv_file: &str, env_file: &str) -> String {
        format!(
            "IFS= read -r line; printf '%s' \"$line\" > '{channel}'; fence=$(printf '%s' \"$line\" | sed -E 's/.*\"fence\":([0-9]+).*/\\1/'); printf '%s' \"$*\" > '{argv}'; env > '{env}'; printf '%s\\n' '{{\"kind\":\"register\",\"state\":\"ready\",\"fence\":'\"$fence\"',\"endpoint\":{{\"host\":\"127.0.0.1\",\"port\":1}},\"capability\":\"cap-1\"}}'; sleep 30",
            channel = channel_file,
            argv = argv_file,
            env = env_file,
        )
    }

    fn acknowledging_script(command_file: &str) -> String {
        format!(
            "IFS= read -r hello; fence=$(printf '%s' \"$hello\" | sed -E 's/.*\"fence\":([0-9]+).*/\\1/'); printf '%s\\n' '{{\"kind\":\"register\",\"state\":\"ready\",\"fence\":'\"$fence\"',\"endpoint\":{{\"host\":\"127.0.0.1\",\"port\":1}},\"capability\":\"cap-1\"}}'; IFS= read -r frame; command=$(printf '%s' \"$frame\" | sed -E 's/.*\"command\":\"([^\"]+)\".*/\\1/'); printf '%s\\n' \"$command\" >> '{command_file}'; printf '%s\\n' '{{\"kind\":\"ack\",\"command\":\"'\"$command\"'\",\"fence\":'\"$fence\"'}}'",
            command_file = command_file,
        )
    }

    #[test]
    fn backoff_is_strictly_increasing_then_capped() {
        let delays: Vec<u64> = (0..6).map(|a| backoff_delay(a).as_millis() as u64).collect();
        assert_eq!(delays, vec![250, 500, 1000, 2000, 4000, 8000]);
        assert_eq!(backoff_delay(10).as_millis(), 8000);
        assert_eq!(backoff_delay(100).as_millis(), 8000);
        for window in delays.windows(2) {
            assert!(window[0] <= window[1]);
        }
    }

    #[test]
    fn sha256_matches_fips_vectors() {
        assert_eq!(
            hex_encode(&sha256(b"")),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            hex_encode(&sha256(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn secret_is_fresh_and_hex_encoded() {
        let first = generate_secret().unwrap();
        let second = generate_secret().unwrap();
        assert_eq!(first.len(), 64);
        assert_ne!(first, second);
        assert!(first.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn registration_validation_rejects_stale_fence_and_bad_inputs() {
        let ready = r#"{"kind":"register","state":"ready","fence":7,"endpoint":{"host":"127.0.0.1","port":9},"capability":"c"}"#;
        let (endpoint, capability, state) = validate_registration(ready, 7).unwrap();
        assert_eq!(endpoint, Some(LoopbackEndpoint { host: "127.0.0.1".into(), port: 9 }));
        assert_eq!(capability.as_deref(), Some("c"));
        assert_eq!(state, ChildState::Ready);

        // Stale fence is rejected even when everything else is valid.
        let stale = r#"{"kind":"register","state":"ready","fence":6,"endpoint":{"host":"127.0.0.1","port":9}}"#;
        assert_eq!(validate_registration(stale, 7).unwrap_err(), "stale_fence");

        // Non-loopback endpoint fails closed.
        let remote = r#"{"kind":"register","state":"ready","fence":7,"endpoint":{"host":"10.0.0.1","port":9}}"#;
        assert_eq!(validate_registration(remote, 7).unwrap_err(), "endpoint_not_loopback");

        // Unknown state and missing port fail closed.
        assert_eq!(
            validate_registration(r#"{"kind":"register","state":"stopped","fence":7}"#, 7).unwrap_err(),
            "registration_state_invalid"
        );
        assert_eq!(
            validate_registration(
                r#"{"kind":"register","state":"ready","fence":7,"endpoint":{"host":"127.0.0.1"}}"#,
                7
            )
            .unwrap_err(),
            "endpoint_port_missing"
        );
    }

    #[test]
    fn spawn_delivers_secret_only_over_the_channel() {
        let dir = tempfile::tempdir().unwrap();
        let channel = dir.path().join("channel").display().to_string();
        let argv = dir.path().join("argv").display().to_string();
        let env = dir.path().join("env").display().to_string();
        let script = cooperative_script(&channel, &argv, &env);
        let manifest = manifest(
            vec!["/bin/sh".into(), "-c".into(), script, "--product".into(), "sample".into()],
            vec![],
        );
        let supervisor = Supervisor::with_policy(fast_policy());
        assert_eq!(supervisor.start_product(&manifest).unwrap(), ProductStatus::Running);

        let channel_bytes = std::fs::read(&channel).unwrap();
        let channel_text = String::from_utf8_lossy(&channel_bytes);
        let hello: serde_json::Value = serde_json::from_str(channel_text.trim()).unwrap();
        let secret = hello["secret"].as_str().unwrap().to_string();
        assert_eq!(hello["kind"], "hello");
        assert_eq!(hello["lifecycleVersion"], 1);
        assert!(hello["fence"].as_u64().unwrap() >= 1);

        let argv_bytes = std::fs::read(&argv).unwrap();
        let argv_text = String::from_utf8_lossy(&argv_bytes);
        let env_bytes = std::fs::read(&env).unwrap();
        let env_text = String::from_utf8_lossy(&env_bytes);
        assert!(
            !argv_text.contains(&secret) && !env_text.contains(&secret),
            "channel secret must never appear in argv or environment"
        );
        supervisor.stop_all();
        assert_eq!(supervisor.census().live, 0);
    }

    #[test]
    fn declared_stop_argv_is_honoured() {
        let dir = tempfile::tempdir().unwrap();
        let stop_marker = dir.path().join("stopped").display().to_string();
        let channel = dir.path().join("channel").display().to_string();
        let argv = dir.path().join("argv").display().to_string();
        let env = dir.path().join("env").display().to_string();
        let script = cooperative_script(&channel, &argv, &env);
        let manifest = manifest(
            vec!["/bin/sh".into(), "-c".into(), script],
            vec!["/bin/sh".into(), "-c".into(), format!("printf stopped > '{stop_marker}'")],
        );
        let supervisor = Supervisor::with_policy(fast_policy());
        assert_eq!(supervisor.start_product(&manifest).unwrap(), ProductStatus::Running);
        supervisor.stop_product("sample");
        assert_eq!(std::fs::read_to_string(&stop_marker).unwrap(), "stopped");
        assert_eq!(supervisor.census().live, 0);
    }

    #[test]
    fn lifecycle_stops_send_exact_fence_commands_and_receive_acks() {
        let dir = tempfile::tempdir().unwrap();
        let commands = dir.path().join("commands").display().to_string();
        let script = acknowledging_script(&commands);
        let manifest = manifest(vec!["/bin/sh".into(), "-c".into(), script], vec![]);
        let supervisor = Supervisor::with_policy(fast_policy());

        supervisor.start_product(&manifest).unwrap();
        supervisor.stop_product("sample");
        supervisor.start_product(&manifest).unwrap();
        supervisor.handoff_for_update("sample").unwrap();
        supervisor.start_product(&manifest).unwrap();
        supervisor.stop_all();

        assert_eq!(std::fs::read_to_string(commands).unwrap().lines().collect::<Vec<_>>(), ["stop", "update_handoff", "ownership_loss"]);
        assert_eq!(supervisor.census().live, 0);
    }

    #[test]
    fn crash_loop_escalates_after_threshold() {
        let script = "IFS= read -r line; fence=$(printf '%s' \"$line\" | sed -E 's/.*\"fence\":([0-9]+).*/\\1/'); printf '%s\\n' '{\"kind\":\"register\",\"state\":\"ready\",\"fence\":'\"$fence\"',\"endpoint\":{\"host\":\"127.0.0.1\",\"port\":1},\"capability\":\"c\"}'; exit 0";
        let manifest = manifest(vec!["/bin/sh".into(), "-c".into(), script.into()], vec![]);
        let supervisor = Supervisor::with_policy(fast_policy());
        assert_eq!(supervisor.start_product(&manifest).unwrap(), ProductStatus::Running);
        let mut status = ProductStatus::Running;
        for _ in 0..4 {
            status = supervisor.supervise_product(&manifest).unwrap();
        }
        assert_eq!(status, ProductStatus::CrashLoop);
        assert_eq!(supervisor.census().crash_looped, 1);
    }

    #[test]
    fn missing_service_is_unavailable_without_product_branching() {
        let manifest = manifest(vec!["/nonexistent-binary-orthic".into()], vec![]);
        let supervisor = Supervisor::with_policy(fast_policy());
        assert_eq!(supervisor.start_product(&manifest).unwrap(), ProductStatus::Unavailable);
        assert!(supervisor.is_unavailable("sample"));
    }

    #[test]
    fn registration_never_accepts_a_non_loopback_endpoint() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let line = format!(
            r#"{{"kind":"register","state":"ready","fence":1,"endpoint":{{"host":"127.0.0.1","port":{port}}},"capability":"c"}}"#
        );
        let (endpoint, _, state) = validate_registration(&line, 1).unwrap();
        assert_eq!(state, ChildState::Ready);
        assert_eq!(endpoint.unwrap().port, port);
        drop(listener);
    }
}
