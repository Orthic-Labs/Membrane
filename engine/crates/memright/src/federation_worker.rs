//! Resident federation gateway worker — supervised `gateway.py --serve-stdio`.
//!
//! Bazel persistent-worker shape: one JSON request line in, one CCS line out,
//! amortizing the ~120 ms interpreter start across prompts. The supervisor
//! owns the whole lifecycle (plan 2026-07-26-federation-resident-gateway,
//! amendments A1/A4):
//!
//! - **Containment:** the child runs in its own POSIX process group (Windows:
//!   `taskkill /T`), and after ANY failed or timed-out request the child tree
//!   is killed and recycled — resident mode must not rely on the process-exit
//!   cleanup that one-shot `gateway.py` documents for its daemon lanes.
//! - **Readiness:** the first stdout line is `{"workerReady":{...}}`; requests
//!   are never written before it arrives.
//! - **Staleness:** the source fingerprint (gateway.py sha256 + max mtime of
//!   `providers/*.py`) is re-checked per request; a mismatch restarts the
//!   worker before the request is sent.
//! - **Circuit breaker:** three consecutive failed request cycles open the
//!   circuit for 60 s so a wedged deploy degrades to the CLI/legacy path
//!   instead of taxing every prompt with timeout + restart.

use sha2::{Digest, Sha256};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

const READY_DEADLINE: Duration = Duration::from_secs(20);
const CIRCUIT_OPEN_AFTER: u32 = 3;
const CIRCUIT_OPEN_FOR: Duration = Duration::from_secs(60);
const DROP_TERM_GRACE: Duration = Duration::from_secs(2);

pub struct ResidentGateway {
    script: PathBuf,
    child: Option<GatewayChild>,
    spawned_fingerprint: String,
    pub worker_restarts: u64,
    consecutive_failures: u32,
    unhealthy_until: Option<Instant>,
    /// Test seam: overrides the interpreter (fixtures are plain scripts).
    python_override: Option<PathBuf>,
}

struct GatewayChild {
    process: Child,
    stdin: ChildStdin,
    lines: Receiver<std::io::Result<String>>,
}

impl ResidentGateway {
    pub fn new(script: PathBuf) -> Self {
        Self {
            script,
            child: None,
            spawned_fingerprint: String::new(),
            worker_restarts: 0,
            consecutive_failures: 0,
            unhealthy_until: None,
            python_override: None,
        }
    }

    pub fn with_python(script: PathBuf, python: PathBuf) -> Self {
        let mut gateway = Self::new(script);
        gateway.python_override = Some(python);
        gateway
    }

    /// One request/response cycle. Restarts the child on any I/O, protocol,
    /// or staleness condition and retries ONCE; a second failure returns Err
    /// so the route can 503 and the hook falls back.
    pub fn request(&mut self, body: &str, timeout: Duration) -> Result<String, String> {
        if let Some(until) = self.unhealthy_until {
            if Instant::now() < until {
                return Err("worker unhealthy (circuit open)".to_string());
            }
            self.unhealthy_until = None;
            self.consecutive_failures = 0;
        }
        let mut last_error = String::new();
        for attempt in 0..2u8 {
            if self.child_stale() {
                if let Err(error) = self.restart() {
                    last_error = error;
                    self.note_failure();
                    continue;
                }
            }
            match self.roundtrip(body, timeout) {
                Ok(line) => {
                    self.consecutive_failures = 0;
                    return Ok(line);
                }
                Err(error) => {
                    // A timed-out lane may still be running inside the worker
                    // (blueprint children run up to 30 s): recycle the whole
                    // tree rather than queue behind leaked work.
                    self.kill_tree();
                    eprintln!("[federate] worker recycled (attempt {attempt}): {error}");
                    last_error = error;
                    self.note_failure();
                }
            }
        }
        Err(format!("resident gateway exhausted retries: {last_error}"))
    }

    pub fn is_healthy(&self) -> bool {
        self.unhealthy_until.is_none_or(|until| Instant::now() >= until)
    }

    fn note_failure(&mut self) {
        self.consecutive_failures += 1;
        if self.consecutive_failures >= CIRCUIT_OPEN_AFTER {
            self.unhealthy_until = Some(Instant::now() + CIRCUIT_OPEN_FOR);
            eprintln!(
                "[federate] circuit open for {}s after {} consecutive failures",
                CIRCUIT_OPEN_FOR.as_secs(),
                self.consecutive_failures
            );
        }
    }

    fn child_stale(&mut self) -> bool {
        let Some(child) = self.child.as_mut() else {
            return true;
        };
        if child.process.try_wait().ok().flatten().is_some() {
            return true;
        }
        source_fingerprint(&self.script) != self.spawned_fingerprint
    }

    fn restart(&mut self) -> Result<(), String> {
        self.kill_tree();
        let fingerprint = source_fingerprint(&self.script);
        let mut command = match self.python_override.as_ref() {
            Some(python) => Command::new(python),
            None => crate::federation::resolve_python_invoker(),
        };
        command
            .arg(&self.script)
            .arg("--serve-stdio")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(unix)]
        {
            // Own process group: providers spawn node/git children; kill(-pgid)
            // reaps the whole tree, not just the interpreter.
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        #[cfg(windows)]
        {
            // Rule 4A: never flash a console window from a background service.
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000);
        }
        let mut process = command
            .spawn()
            .map_err(|error| format!("spawn resident gateway: {error}"))?;
        let stdin = process
            .stdin
            .take()
            .ok_or_else(|| "resident gateway stdin unavailable".to_string())?;
        let stdout = process
            .stdout
            .take()
            .ok_or_else(|| "resident gateway stdout unavailable".to_string())?;
        let (sender, lines) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) => break,
                    Ok(_) => {
                        if sender.send(Ok(line)).is_err() {
                            break;
                        }
                    }
                    Err(error) => {
                        let _ = sender.send(Err(error));
                        break;
                    }
                }
            }
        });
        let mut child = GatewayChild {
            process,
            stdin,
            lines,
        };
        // Readiness gate: never write a request before workerReady arrives.
        let ready = recv_line(&mut child, READY_DEADLINE)?;
        let parsed: serde_json::Value = serde_json::from_str(&ready)
            .map_err(|error| format!("resident gateway readiness is not JSON: {error}"))?;
        if parsed.get("workerReady").is_none() {
            let _ = kill_child_tree(&mut child.process);
            return Err(format!(
                "resident gateway first line is not a readiness handshake: {}",
                ready.chars().take(120).collect::<String>()
            ));
        }
        self.child = Some(child);
        self.spawned_fingerprint = fingerprint;
        self.worker_restarts += 1;
        Ok(())
    }

    fn roundtrip(&mut self, body: &str, timeout: Duration) -> Result<String, String> {
        let child = self
            .child
            .as_mut()
            .ok_or_else(|| "resident gateway not running".to_string())?;
        child
            .stdin
            .write_all(body.as_bytes())
            .and_then(|()| child.stdin.write_all(b"\n"))
            .and_then(|()| child.stdin.flush())
            .map_err(|error| format!("write to resident gateway: {error}"))?;
        recv_line(child, timeout)
    }

    fn kill_tree(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = kill_child_tree(&mut child.process);
        }
    }

    #[cfg(test)]
    fn child_pid(&self) -> Option<u32> {
        self.child.as_ref().map(|child| child.process.id())
    }
}

impl Drop for ResidentGateway {
    /// TERM the process group, grant a short grace, then KILL. Without this,
    /// service shutdown orphans `python3 gateway.py --serve-stdio` workers.
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            terminate_tree(&mut child.process, DROP_TERM_GRACE);
        }
    }
}

fn recv_line(child: &mut GatewayChild, timeout: Duration) -> Result<String, String> {
    match child.lines.recv_timeout(timeout) {
        Ok(Ok(line)) => Ok(line.trim().to_string()),
        Ok(Err(error)) => Err(format!("read from resident gateway: {error}")),
        Err(RecvTimeoutError::Timeout) => Err(format!(
            "resident gateway timed out after {} ms",
            timeout.as_millis()
        )),
        Err(RecvTimeoutError::Disconnected) => {
            Err("resident gateway closed its stdout".to_string())
        }
    }
}

/// gateway.py bytes hash + newest providers/*.py mtime. Re-stat per request
/// is the staleness contract (plan Task 5); the hash half only re-reads when
/// the script's own mtime changed.
fn source_fingerprint(script: &Path) -> String {
    let mut hasher = Sha256::new();
    if let Ok(bytes) = std::fs::read(script) {
        hasher.update(&bytes);
    }
    let mut newest_mtime = 0u128;
    if let Some(parent) = script.parent() {
        if let Ok(entries) = std::fs::read_dir(parent.join("providers")) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|ext| ext.to_str()) != Some("py") {
                    continue;
                }
                if let Ok(modified) = entry.metadata().and_then(|meta| meta.modified()) {
                    if let Ok(elapsed) = modified.duration_since(std::time::UNIX_EPOCH) {
                        newest_mtime = newest_mtime.max(elapsed.as_nanos());
                    }
                }
            }
        }
    }
    format!("{:x}|{newest_mtime}", hasher.finalize())
}

fn kill_child_tree(process: &mut Child) -> std::io::Result<()> {
    signal_tree(process, false);
    let _ = process.kill();
    let _ = process.wait();
    Ok(())
}

fn terminate_tree(process: &mut Child, grace: Duration) {
    signal_tree(process, true);
    let deadline = Instant::now() + grace;
    while Instant::now() < deadline {
        if process.try_wait().ok().flatten().is_some() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    signal_tree(process, false);
    let _ = process.kill();
    let _ = process.wait();
}

#[cfg(unix)]
fn signal_tree(process: &mut Child, graceful: bool) {
    // The child was spawned with process_group(0); a negative pid signals the
    // whole group. /bin/kill avoids a libc dependency for this one call.
    let signal = if graceful { "-TERM" } else { "-KILL" };
    let _ = Command::new("kill")
        .args([signal, &format!("-{}", process.id())])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[cfg(windows)]
fn signal_tree(process: &mut Child, _graceful: bool) {
    let _ = Command::new("taskkill")
        .args(["/PID", &process.id().to_string(), "/T", "/F"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn fixture(dir: &Path, body: &str) -> PathBuf {
        let script = dir.join("gateway.py");
        std::fs::write(
            &script,
            format!(
                "#!/usr/bin/env python3\nimport json, sys, time\n\
                 print(json.dumps({{'workerReady': {{'gatewaySha256': '0'*64, 'pid': 1}}}}), flush=True)\n\
                 {body}\n"
            ),
        )
        .unwrap();
        script
    }

    fn echo_fixture(dir: &Path) -> PathBuf {
        fixture(
            dir,
            "for line in iter(sys.stdin.readline, ''):\n    \
             line = line.strip()\n    \
             if not line: continue\n    \
             if line == '\"die\"': sys.exit(3)\n    \
             if line == '\"stall\"': time.sleep(30)\n    \
             print(json.dumps({'echo': json.loads(line)}), flush=True)",
        )
    }

    fn python() -> PathBuf {
        PathBuf::from("python3")
    }

    #[test]
    fn worker_restart_after_child_death_serves_the_next_request() {
        let dir = tempfile::tempdir().unwrap();
        let script = echo_fixture(dir.path());
        let mut gateway = ResidentGateway::with_python(script, python());

        let first = gateway.request("\"one\"", Duration::from_secs(10)).unwrap();
        assert_eq!(first, "{\"echo\": \"one\"}");
        // Kill mid-session: the request that hits the dead child must restart
        // once and still succeed within the same call.
        gateway.request("\"die\"", Duration::from_secs(10)).ok();
        let after = gateway.request("\"two\"", Duration::from_secs(10)).unwrap();
        assert_eq!(after, "{\"echo\": \"two\"}");
        assert!(gateway.worker_restarts >= 2);
    }

    #[test]
    fn worker_timeout_recycles_the_child_pid() {
        let dir = tempfile::tempdir().unwrap();
        let script = echo_fixture(dir.path());
        let mut gateway = ResidentGateway::with_python(script, python());
        gateway.request("\"warm\"", Duration::from_secs(10)).unwrap();
        let stalled_pid = gateway.child_pid().unwrap();

        let result = gateway.request("\"stall\"", Duration::from_millis(300));

        assert!(result.is_err(), "stalled request must error");
        // The stalled tree was killed — a fresh request gets a NEW pid.
        gateway.request("\"again\"", Duration::from_secs(10)).unwrap();
        assert_ne!(gateway.child_pid().unwrap(), stalled_pid);
        let alive = Command::new("kill")
            .args(["-0", &stalled_pid.to_string()])
            .status()
            .unwrap();
        assert!(!alive.success(), "old worker pid must be dead");
    }

    #[test]
    fn circuit_opens_after_three_consecutive_failures() {
        let dir = tempfile::tempdir().unwrap();
        // A worker that dies before every response.
        let script = fixture(dir.path(), "sys.exit(4)");
        let mut gateway = ResidentGateway::with_python(script, python());
        for _ in 0..3 {
            assert!(gateway
                .request("\"x\"", Duration::from_millis(1500))
                .is_err());
        }
        assert!(!gateway.is_healthy(), "circuit must be open");
        let error = gateway
            .request("\"x\"", Duration::from_millis(200))
            .unwrap_err();
        assert!(error.contains("circuit open"), "{error}");
    }

    #[test]
    fn source_change_restarts_before_the_request() {
        let dir = tempfile::tempdir().unwrap();
        let script = echo_fixture(dir.path());
        let mut gateway = ResidentGateway::with_python(script.clone(), python());
        gateway.request("\"a\"", Duration::from_secs(10)).unwrap();
        let restarts = gateway.worker_restarts;

        std::fs::write(&script, std::fs::read(&script).unwrap()).unwrap();
        let mut edited = std::fs::read_to_string(&script).unwrap();
        edited.push('\n');
        std::fs::write(&script, edited).unwrap();

        gateway.request("\"b\"", Duration::from_secs(10)).unwrap();
        assert_eq!(gateway.worker_restarts, restarts + 1);
    }

    #[test]
    fn drop_kills_the_worker_within_grace() {
        let dir = tempfile::tempdir().unwrap();
        let script = echo_fixture(dir.path());
        let mut gateway = ResidentGateway::with_python(script, python());
        gateway.request("\"warm\"", Duration::from_secs(10)).unwrap();
        let pid = gateway.child_pid().unwrap();
        drop(gateway);
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let alive = Command::new("kill")
                .args(["-0", &pid.to_string()])
                .status()
                .unwrap()
                .success();
            if !alive {
                break;
            }
            assert!(Instant::now() < deadline, "worker survived Drop");
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    #[test]
    fn repeated_timeouts_do_not_leak_pids() {
        let dir = tempfile::tempdir().unwrap();
        let script = echo_fixture(dir.path());
        let mut gateway = ResidentGateway::with_python(script, python());
        let mut recycled = Vec::new();
        for _ in 0..2 {
            gateway.request("\"warm\"", Duration::from_secs(10)).unwrap();
            recycled.push(gateway.child_pid().unwrap());
            gateway.request("\"stall\"", Duration::from_millis(200)).ok();
            gateway.consecutive_failures = 0; // soak isolates leak-checking from the breaker
        }
        for pid in recycled {
            let alive = Command::new("kill")
                .args(["-0", &pid.to_string()])
                .status()
                .unwrap()
                .success();
            assert!(!alive, "recycled worker {pid} still alive");
        }
    }
}
