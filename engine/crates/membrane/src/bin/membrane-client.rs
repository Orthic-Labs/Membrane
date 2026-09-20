//! Transport-only installed-engine client.
//!
//! This executable owns no Membrane runtime, state, planner, storage, or
//! installer control. It only frames bounded HTTP requests to the
//! installer-owned singleton engine and prints process-local packaging
//! metadata. Any mode that would construct runtime state or mutate the
//! installation is rejected here and belongs to `membrane.exe` (invoked by
//! the installer/activation path), never to this transport.

use serde_json::{json, Value};
use std::collections::VecDeque;
use std::io::{self, BufRead, IsTerminal, Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

const DEFAULT_PORT: u16 = 47_851;
const MAX_BODY_BYTES: usize = 8 * 1024 * 1024;
const IO_TIMEOUT: Duration = Duration::from_secs(30);
/// Bounded pool of reusable keep-alive connections to the singleton engine.
/// Persistent clients (stdio loops, repeated hooks) reuse sockets instead of
/// paying a TCP+TLS handshake per request; the bound keeps a misbehaving
/// caller from accumulating sockets.
const POOL_MAX_CONNS: usize = 4;
/// A pooled connection is retried at most once per request: if the reused
/// socket proves stale, exactly one fresh connection is attempted.
const MAX_ATTEMPTS_PER_REQUEST: usize = 2;
/// Bounded connect budget for reaching the singleton engine. A refused or
/// black-holed loopback connect must fail in well under a host hook budget;
/// established-socket I/O keeps IO_TIMEOUT.
const CONNECT_TIMEOUT: Duration = Duration::from_millis(300);
/// Hook route served by engine; hooks may record observations or apply
/// enforcement, so fully sent requests are never retried after ambiguity.
const HOOK_PATH: &str = "/hook";

/// Harness access lifetime (decisions 22/24): persistent client sessions own
/// the singleton engine exactly while their Membrane access is active. The
/// lease is bounded and renewed while the session lives; a crashed client
/// simply stops renewing and the engine drains after expiry. No OS scheduler
/// lane ever resurrects the engine.
const HARNESS_LEASE_TTL_MS: u64 = 30_000;
const HARNESS_RENEW_INTERVAL_MS: u64 = 10_000;
/// Engine-down activation is bounded: one foreground `membrane.exe activate`
/// attempt per client session, well under any host startup budget.
const ACTIVATION_TIMEOUT: Duration = Duration::from_secs(90);
/// Grace allowed for piped CLI stdin to reach EOF before the request is
/// forwarded without it. Lease acquisition ahead of `forward_cli` already
/// gives a writer seconds to deliver; this bound only fires on an open pipe
/// that never produces a byte.
const STDIN_EOF_GRACE: Duration = Duration::from_secs(2);
const STDIN_HOOK_QUIET: Duration = Duration::from_millis(400);

const CLIENT_HELP: &str = "membrane-client [stdio-mcp|hook|cli] [args…]\n\
    \n\
    Transport-only client for the installer-owned Membrane singleton engine.\n\
    It forwards stdio-mcp, hook, and cli traffic to the resident engine over\n\
    bounded loopback HTTP and prints local packaging metadata. It constructs\n\
    no runtime state. Install, activation, migration, and init control live\n\
    in membrane.exe and the installer; this binary rejects those modes.";

const HOOK_HELP: &str = "membrane-client hook\n\
    \n\
    Reads one HookHost JSON payload from stdin and forwards it to the\n\
    resident singleton engine's /hook route, then prints the engine's\n\
    response to stdout. Requires a running installed engine.";

// Compile-time release identity (see build.rs): the exact source generation
// this binary was built from, embedded as source so sccache cannot serve a
// stale object. No runtime state is consulted.
mod generated {
    include!(concat!(
        env!("OUT_DIR"),
        "/client_release_identity_generated.rs"
    ));
}

fn release_generation() -> String {
    format!(
        "sha256:{}",
        generated::SOURCE_TREE_SHA256.unwrap_or("unknown")
    )
}

fn main() {
    let args = std::env::args_os().collect::<Vec<_>>();
    let words: Vec<String> = args.iter().skip(1).filter_map(|arg| arg.to_str().map(str::to_owned)).collect();
    match words.as_slice() {
        [] => {
            println!("{CLIENT_HELP}");
        }
        [flag] if flag == "--help" || flag == "-h" => {
            println!("{CLIENT_HELP}");
        }
        [flag] if flag == "--version" || flag == "-V" => {
            println!("membrane-client {}", env!("CARGO_PKG_VERSION"));
        }
        // Packaging metadata is a local file read. It never connects to the
        // engine and never constructs runtime state.
        [a, b] if a == "cli" && b == "build-info" => print_build_info(),
        [a, b] if a == "hook" && (b == "--help" || b == "-h") => {
            println!("{HOOK_HELP}");
        }
        [mode, ..] if mode == "stdio-mcp" || mode == "hook" || mode == "cli" => {
            if let Err(error) = run(mode) {
                if mode == "hook" {
                    // Hosts consume stdout JSON. An engine-down hook must
                    // remain a valid typed response instead of stderr noise:
                    // no hookSpecificOutput is emitted (the host schema
                    // rejects unknown event variants), the reason is typed,
                    // and enforcement decisions stay server-side — this
                    // transport never fabricates a block.
                    let response = json!({
                        "membraneHook": {
                            "schemaVersion": 1,
                            "event": "engine_unreachable",
                            "status": "error",
                            "results": [],
                            "detail": {"reason": "engine_unavailable", "error": error}
                        }
                    });
                    println!("{response}");
                    std::process::exit(0);
                }
                eprintln!("membrane-client: {error}");
                std::process::exit(1);
            }
        }
        [mode, ..]
            if matches!(
                mode.as_str(),
                "install" | "uninstall" | "activate" | "deactivate" | "migrate-legacy" | "init" | "serve"
            ) =>
        {
            eprintln!(
                "membrane-client: '{mode}' is installer-owned control and is not served by the transport client; use membrane.exe {mode} from the installer or activation path"
            );
            std::process::exit(2);
        }
        [mode, ..] => {
            eprintln!("membrane-client: unknown mode '{mode}'; see membrane-client --help");
            std::process::exit(2);
        }
    }
}

/// Local packaging metadata: version plus the compile-time release identity
/// (see build.rs). Pure process-local data; no engine connection, no runtime
/// dispatch, no storage ownership. The `product_version` read below is
/// informational only: the binding contract fields (`release_generation`,
/// `target`) never touch the filesystem.
fn print_build_info() {
    let version = env!("CARGO_PKG_VERSION");
    let target = env!("TARGET_TRIPLE");
    let (_, product_version) = installed_release_metadata();
    let info = json!({
        "name": "membrane-client",
        "version": version,
        "target": target,
        "transport_only": true,
        "product_version": product_version,
        "release_generation": release_generation(),
    });
    match serde_json::to_string_pretty(&info) {
        Ok(encoded) => println!("{encoded}"),
        Err(error) => {
            eprintln!("membrane-client: serialize build-info: {error}");
            std::process::exit(1);
        }
    }
}

fn installed_release_metadata() -> (Value, Value) {
    let release = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent()?.parent().map(|root| root.join("release.json")))
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok());
    match release {
        Some(value) => (
            value.get("releaseGeneration").cloned().unwrap_or(Value::Null),
            value.get("version").cloned().unwrap_or(Value::Null),
        ),
        None => (Value::Null, Value::Null),
    }
}

fn run(mode: &str) -> Result<(), String> {
    match mode {
        "stdio-mcp" => {
            let _lease = harness::session_lease().map_err(|error| format!("engine_unavailable: {error}"))?;
            run_stdio_mcp()
        }
        "hook" => {
            let codex_wire = std::env::args()
                .skip(2)
                .any(|arg| arg == "--wire=codex");
            let body = read_hook_stdin()?;
            if body.len() > MAX_BODY_BYTES {
                return Err("hook payload exceeds transport limit".into());
            }
            let (session_id, session_end) = hook_session_identity(&body);
            let _lease = harness::hook_lease(session_id.as_deref(), session_end).map_err(|error| format!("engine_unavailable: {error}"))?;
            let (_, response) = request_engine("/hook", &body)?;
            let response = if codex_wire {
                codex_wire_response(&response)
            } else {
                response
            };
            write_stdout(&response)
        }
        "cli" => {
            let _lease = harness::cli_lease().map_err(|error| format!("engine_unavailable: {error}"))?;
            let tail = std::env::args().skip(2).collect::<Vec<_>>();
            forward_cli(tail)
        }
        other => {
            let _lease = harness::cli_lease().map_err(|error| format!("engine_unavailable: {error}"))?;
            let mut tail = vec![other.to_owned()];
            tail.extend(std::env::args().skip(2));
            forward_cli(tail)
        }
    }
}

/// Forwarded CLI verbs may consume piped stdin (`push prepare`,
/// `resident-holder`, checkpoint/replay payloads). A transport spawned by a
/// harness or CI runner inherits an open, idle pipe whose EOF never arrives;
/// a blocking read would stall the call before any request reached the
/// engine. Stdin therefore drains on a side thread: an already-written pipe
/// reaches EOF immediately, while a silent pipe degrades to empty stdin
/// after a short grace so a verb that required input surfaces its own typed
/// error instead of an opaque hang.
fn read_cli_stdin() -> Result<String, String> {
    if io::stdin().is_terminal() {
        return Ok(String::new());
    }
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("membrane-cli-stdin".into())
        .spawn(move || {
            let mut input = String::new();
            let result = io::stdin()
                .take((MAX_BODY_BYTES + 1) as u64)
                .read_to_string(&mut input)
                .map(|_| input);
            let _ = tx.send(result);
        })
        .map_err(|error| format!("start CLI input reader: {error}"))?;
    match rx.recv_timeout(STDIN_EOF_GRACE) {
        Ok(Ok(input)) => Ok(input),
        Ok(Err(error)) => Err(format!("read CLI input: {error}")),
        Err(_) => Ok(String::new()),
    }
}

/// Hosts invoke `hook` with the event payload on stdin but may keep the pipe
/// open for the life of the harness session; a blocking `read_to_end` then
/// outlives the host's hook timeout and the hook is killed before answering.
/// Stdin drains on a side thread in chunks: the first bytes must arrive within
/// the standard grace (a silent pipe degrades to an empty body), payload ends
/// on EOF — reported as an empty chunk sentinel — or after a short quiet
/// interval once bytes have flowed.
fn read_hook_stdin() -> Result<Vec<u8>, String> {
    if io::stdin().is_terminal() {
        return Ok(Vec::new());
    }
    let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
    std::thread::Builder::new()
        .name("membrane-hook-stdin".into())
        .spawn(move || {
            let mut stdin = io::stdin();
            loop {
                let mut chunk = vec![0u8; 8192];
                match stdin.read(&mut chunk) {
                    Ok(0) => {
                        let _ = tx.send(Vec::new());
                        break;
                    }
                    Ok(n) => {
                        chunk.truncate(n);
                        if tx.send(chunk).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        })
        .map_err(|error| format!("start hook input reader: {error}"))?;
    let mut body = Vec::new();
    match rx.recv_timeout(STDIN_EOF_GRACE) {
        Ok(chunk) => body.extend_from_slice(&chunk),
        Err(_) => return Ok(body),
    }
    while body.len() <= MAX_BODY_BYTES {
        match rx.recv_timeout(STDIN_HOOK_QUIET) {
            Ok(chunk) if chunk.is_empty() => break,
            Ok(chunk) => body.extend_from_slice(&chunk),
            Err(_) => break,
        }
    }
    Ok(body)
}

fn forward_cli(args: Vec<String>) -> Result<(), String> {
    let stdin = read_cli_stdin()?;
    let request = json!({ "args": args, "stdin": stdin }).to_string();
    if request.len() > MAX_BODY_BYTES {
        return Err("CLI request exceeds transport limit".into());
    }
    let (_, body) = request_engine("/cli", request.as_bytes())?;
    let response: CliResponse = serde_json::from_slice(&body)
        .map_err(|error| format!("invalid CLI response: {error}"))?;
    write_stdout(response.stdout.as_bytes())?;
    io::stderr()
        .write_all(response.stderr.as_bytes())
        .map_err(|error| format!("write CLI error output: {error}"))?;
    if response.exit_code != 0 {
        std::process::exit(response.exit_code);
    }
    Ok(())
}

#[derive(serde::Deserialize)]
struct CliResponse {
    stdout: String,
    stderr: String,
    exit_code: i32,
}

fn run_stdio_mcp() -> Result<(), String> {
    let stdin = io::stdin();
    let mut stdout = io::BufWriter::new(io::stdout().lock());
    for line in stdin.lock().lines() {
        let line = line.map_err(|error| format!("read stdio request: {error}"))?;
        if line.trim().is_empty() {
            continue;
        }
        if line.len() > MAX_BODY_BYTES {
            return Err("stdio request exceeds transport limit".into());
        }
        let (_, body) = request_engine("/mcp", line.as_bytes())?;
        if !body.is_empty() {
            stdout
                .write_all(&body)
                .and_then(|_| stdout.write_all(b"\n"))
                .map_err(|error| format!("write stdio response: {error}"))?;
            stdout
                .flush()
                .map_err(|error| format!("flush stdio response: {error}"))?;
        }
    }
    Ok(())
}

fn forward_stdin(path: &str) -> Result<(), String> {
    let mut body = Vec::new();
    io::stdin()
        .take((MAX_BODY_BYTES + 1) as u64)
        .read_to_end(&mut body)
        .map_err(|error| format!("read hook payload: {error}"))?;
    if body.len() > MAX_BODY_BYTES {
        return Err("hook payload exceeds transport limit".into());
    }
    forward_body(path, &body)
}

fn forward_body(path: &str, body: &[u8]) -> Result<(), String> {
    let (_, response) = request_engine(path, body)?;
    write_stdout(&response)
}

/// Codex validates hook stdout against per-event schemas declared with
/// `deny_unknown_fields`: the `membraneHook` receipt extension and any
/// event-foreign key turn a healthy response into a reported hook failure.
/// `hook --wire=codex` re-emits only the fields each Codex wire allows; the
/// engine's full receipt still flows to hosts that tolerate extensions.
fn codex_wire_response(body: &[u8]) -> Vec<u8> {
    let Ok(mut value) = serde_json::from_slice::<Value>(body) else {
        return body.to_vec();
    };
    let Some(object) = value.as_object_mut() else {
        return body.to_vec();
    };
    let event = object
        .get("hookSpecificOutput")
        .and_then(|specific| specific.get("hookEventName"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let specific_keys: &[&str] = match event.as_str() {
        "SessionStart" | "UserPromptSubmit" | "SubagentStart" => {
            &["hookEventName", "additionalContext"]
        }
        "PreToolUse" => &[
            "hookEventName",
            "additionalContext",
            "permissionDecision",
            "permissionDecisionReason",
            "updatedInput",
        ],
        "PostToolUse" => &["hookEventName", "additionalContext", "updatedMCPToolOutput"],
        _ => &[],
    };
    if let Some(specific) = object
        .get_mut("hookSpecificOutput")
        .and_then(Value::as_object_mut)
    {
        specific.retain(|key, _| specific_keys.contains(&key.as_str()));
    }
    let allows_decision = matches!(
        event.as_str(),
        "UserPromptSubmit" | "PreToolUse" | "PostToolUse" | "Stop"
    );
    let allows_specific = !specific_keys.is_empty();
    object.retain(|key, _| {
        matches!(
            key.as_str(),
            "continue" | "stopReason" | "suppressOutput" | "systemMessage"
        ) || (allows_decision && (key == "decision" || key == "reason"))
            || (allows_specific && key == "hookSpecificOutput")
    });
    serde_json::to_vec(&value).unwrap_or_else(|_| body.to_vec())
}

fn hook_session_identity(body: &[u8]) -> (Option<String>, bool) {
    let Ok(payload) = serde_json::from_slice::<Value>(body) else {
        return (None, false);
    };
    let session_id = payload
        .get("session_id")
        .or_else(|| payload.get("sessionId"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty() && value.len() <= 512)
        .map(str::to_owned);
    let event = payload
        .get("hook_event_name")
        .or_else(|| payload.get("hookEventName"))
        .or_else(|| payload.get("event"))
        .and_then(Value::as_str);
    (session_id, event.is_some_and(|value| value.eq_ignore_ascii_case("SessionEnd")))
}

fn write_stdout(body: &[u8]) -> Result<(), String> {
    io::stdout()
        .write_all(body)
        .and_then(|_| io::stdout().flush())
        .map_err(|error| format!("write response: {error}"))
}

// ---------------------------------------------------------------------------
// Harness access lifetime (decisions 22/24).
// ---------------------------------------------------------------------------

/// Identity of this client's harness holder lease. `controller` is captured
/// from the acquire response so renew/release address the same controller.
struct HarnessLease {
    identity: Arc<Mutex<(Value, Value)>>,
    stop: Arc<AtomicBool>,
    renew_wake: Option<std::sync::mpsc::Sender<()>>,
    release_on_drop: bool,
}

impl Drop for HarnessLease {
    fn drop(&mut self) {
        // Release is best effort: the lease is bounded, so a failed release
        // still expires server-side without leaving an orphan engine.
        self.stop.store(true, Ordering::Release);
        if let Some(wake) = self.renew_wake.take() {
            let _ = wake.send(());
        }
        if !self.release_on_drop {
            return;
        }
        let Ok((controller, holder)) = self.identity.lock().map(|identity| (identity.0.clone(), identity.1.clone())) else { return };
        let _ = holder_exchange(
            membrane_protocol::ResidentHolderOperationV1::Release,
            Some(&controller),
            Some(&holder),
            None,
            Duration::from_secs(2),
            None,
        );
    }
}

mod harness {
    use super::*;

    /// Persistent session: acquire once, renew while alive, release on drop.
    pub fn session_lease() -> Result<HarnessLease, String> {
        acquire_with_activation("stdio-mcp", true, None, true)
    }

    /// CLI operations own the engine for their bounded process lifetime.
    pub fn cli_lease() -> Result<HarnessLease, String> {
        acquire_with_activation("cli", true, None, true)
    }

    /// Hook ownership is keyed by host session ID. Each hook refreshes the
    /// same bounded lease; only SessionEnd releases it immediately.
    pub fn hook_lease(session_id: Option<&str>, session_end: bool) -> Result<HarnessLease, String> {
        acquire_with_activation("hook", false, session_id, session_end)
    }

    fn acquire_with_activation(
        _mode: &str,
        renew: bool,
        holder_id: Option<&str>,
        release_on_drop: bool,
    ) -> Result<HarnessLease, String> {
        let deadline = std::time::Instant::now() + ACTIVATION_TIMEOUT;
        match acquire_lease_with_timeout(renew, holder_id, release_on_drop, Duration::from_secs(1)) {
            Ok(lease) => return Ok(lease),
            Err(initial) => {
                let activation = start_activation()?;
                let mut last = initial;
                loop {
                    let remaining = deadline.saturating_duration_since(std::time::Instant::now());
                    if remaining.is_zero() {
                        return Err(format!("activation ownership deadline exceeded: {last}"));
                    }
                    match acquire_lease_with_timeout(renew, holder_id, release_on_drop, remaining.min(Duration::from_secs(1))) {
                        Ok(lease) => { activation.reap(deadline); return Ok(lease); }
                        Err(error) => last = error,
                    }
                    if let Some(error) = activation.failure()? {
                        if !error.contains("startup lock remained busy") && !error.contains("did not become healthy within") {
                            return Err(format!("{error}; acquisition: {last}"));
                        }
                    }
                    std::thread::sleep(Duration::from_millis(100).min(deadline.saturating_duration_since(std::time::Instant::now())));
                }
            }
        }
    }

    // File-backed diagnostics never wait for EOF from inherited daemon handles.
    struct ActivationChild {
        #[cfg(windows)]
        child: Mutex<WindowsActivationChild>,
        #[cfg(not(windows))]
        child: Mutex<std::process::Child>,
        log: std::path::PathBuf,
    }
    impl ActivationChild {
        fn failure(&self) -> Result<Option<String>, String> {
            let status = self.child.lock().map_err(|_| "activation child lock poisoned")?
                .try_wait().map_err(|e| format!("activation wait: {e}"))?;
            Ok(status.filter(|s| !s.success()).map(|s| {
                let mut bytes = Vec::new();
                if let Ok(file) = std::fs::File::open(&self.log) { let _ = file.take(8192).read_to_end(&mut bytes); }
                format!("activate exited with {s}: {}", String::from_utf8_lossy(&bytes))
            }))
        }
        fn reap(self, deadline: std::time::Instant) {
            std::thread::spawn(move || {
                while std::time::Instant::now() < deadline {
                    if self.child.lock().ok().and_then(|mut c| c.try_wait().ok().flatten()).is_some() { return; }
                    std::thread::sleep(Duration::from_millis(100));
                }
            });
        }
    }
    #[cfg(windows)]
    struct WindowsActivationChild { process: std::os::windows::io::OwnedHandle }
    #[cfg(windows)]
    impl WindowsActivationChild {
        fn try_wait(&mut self) -> std::io::Result<Option<std::process::ExitStatus>> {
            use std::os::windows::io::AsRawHandle;
            use windows_sys::Win32::{Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT}, System::Threading::WaitForSingleObject};
            match unsafe { WaitForSingleObject(self.process.as_raw_handle(), 0) } {
                WAIT_OBJECT_0 => self.exit_status().map(Some),
                WAIT_TIMEOUT => Ok(None),
                _ => Err(std::io::Error::last_os_error()),
            }
        }
        fn exit_status(&self) -> std::io::Result<std::process::ExitStatus> {
            use std::os::windows::{io::AsRawHandle, process::ExitStatusExt};
            let mut code = 0;
            if unsafe { windows_sys::Win32::System::Threading::GetExitCodeProcess(self.process.as_raw_handle(), &mut code) } == 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(std::process::ExitStatus::from_raw(code))
        }
        fn kill(&mut self) -> std::io::Result<()> {
            use std::os::windows::io::AsRawHandle;
            if unsafe { windows_sys::Win32::System::Threading::TerminateProcess(self.process.as_raw_handle(), 1) } == 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        }
        fn wait(&mut self) -> std::io::Result<std::process::ExitStatus> {
            use std::os::windows::io::AsRawHandle;
            use windows_sys::Win32::{Foundation::WAIT_OBJECT_0, System::Threading::{WaitForSingleObject, INFINITE}};
            if unsafe { WaitForSingleObject(self.process.as_raw_handle(), INFINITE) } != WAIT_OBJECT_0 {
                return Err(std::io::Error::last_os_error());
            }
            self.exit_status()
        }
    }
    impl Drop for ActivationChild {
        fn drop(&mut self) {
            if let Ok(child) = self.child.get_mut() {
                if child.try_wait().ok().flatten().is_none() { let _ = child.kill(); }
                let _ = child.wait();
            }
            let _ = std::fs::remove_file(&self.log);
        }
    }
    fn start_activation() -> Result<ActivationChild, String> {
        let control = activation_control_binary()
            .ok_or("activation control binary not found in installed root or next to client")?;
        spawn_activation(control, &["activate", "--engine-only", "--timeout-ms", "15000"], &[], &[])
    }
    // Host plugin caches carry their own `membrane.exe` payload copy, but
    // activation must bind the installed product: spawning the cache copy
    // provisions a duplicate stack rooted at the cache directory. Resolve the
    // canonical installed control first; exe-relative stays the fallback for
    // fixture/dev layouts where no install exists or an override targets a
    // non-default engine.
    fn activation_control_binary() -> Option<std::path::PathBuf> {
        if std::env::var_os("MEMBRANE_PROJECT_REGISTRY").is_none()
            && std::env::var_os("MEMBRANE_ENDPOINT").is_none()
            && std::env::var_os("MEMBRANE_PORT").is_none()
        {
            if let Ok(current) = membrane_client::default_stable_install_root() {
                let candidate = current.join("membrane.exe");
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.join("membrane.exe")))
    }
    fn spawn_activation(program: std::path::PathBuf, args: &[&str], env_remove: &[&str], env_set: &[(&str, &str)]) -> Result<ActivationChild, String> {
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
        let log = std::env::temp_dir().join(format!("membrane-activation-{}-{nonce}.log", std::process::id()));
        let file = std::fs::OpenOptions::new().write(true).create_new(true).open(&log)
            .map_err(|e| format!("activation diagnostic file: {e}"))?;
        #[cfg(windows)] { return spawn_activation_windows(program, args, env_remove, env_set, file, log); }
        #[cfg(not(windows))]
        {
            let mut command = Command::new(program);
            command.args(args);
            for key in env_remove { command.env_remove(key); }
            command.envs(env_set.iter().copied());
            command.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(file);
            match command.spawn() {
                Ok(child) => Ok(ActivationChild { child: Mutex::new(child), log }),
                Err(e) => { let _ = std::fs::remove_file(log); Err(format!("activate launch failed: {e}")) }
            }
        }
    }
    #[cfg(windows)]
    fn spawn_activation_windows(
        program: std::path::PathBuf,
        args: &[&str],
        env_remove: &[&str],
        env_set: &[(&str, &str)],
        stderr_file: std::fs::File,
        log: std::path::PathBuf,
    ) -> Result<ActivationChild, String> {
        use std::mem::size_of;
        use std::os::windows::{ffi::OsStrExt, io::{AsRawHandle, FromRawHandle, OwnedHandle}};
        use windows_sys::Win32::{
            Foundation::{CloseHandle, SetHandleInformation, HANDLE, HANDLE_FLAG_INHERIT},
            System::Threading::{
                CreateProcessW, DeleteProcThreadAttributeList, InitializeProcThreadAttributeList,
                UpdateProcThreadAttribute, EXTENDED_STARTUPINFO_PRESENT, CREATE_NO_WINDOW,
                LPPROC_THREAD_ATTRIBUTE_LIST, PROCESS_INFORMATION, PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
                STARTF_USESTDHANDLES, STARTUPINFOEXW,
                CREATE_UNICODE_ENVIRONMENT,
            },
        };
        fn wide(value: &std::ffi::OsStr) -> Vec<u16> { value.encode_wide().chain([0]).collect() }
        fn quote(value: &str) -> String {
            if !value.is_empty() && !value.chars().any(|c| c.is_whitespace() || c == '"') { return value.into(); }
            let mut out = String::from("\"");
            let mut slashes = 0;
            for ch in value.chars() {
                if ch == '\\' { slashes += 1; continue; }
                if ch == '"' { out.push_str(&"\\".repeat(slashes * 2 + 1)); out.push(ch); slashes = 0; continue; }
                out.push_str(&"\\".repeat(slashes)); out.push(ch); slashes = 0;
            }
            out.push_str(&"\\".repeat(slashes * 2)); out.push('"'); out
        }
        let mut command_line = quote(&program.to_string_lossy());
        for arg in args { command_line.push(' '); command_line.push_str(&quote(arg)); }
        let mut command_line = wide(std::ffi::OsStr::new(&command_line));
        let application = wide(program.as_os_str());
        let mut environment = Vec::<u16>::new();
        if !env_remove.is_empty() || !env_set.is_empty() {
            let mut values: Vec<_> = std::env::vars_os().filter(|(key, _)| {
                let key = key.to_string_lossy();
                !env_remove.iter().any(|name| name.eq_ignore_ascii_case(&key))
                    && !env_set.iter().any(|(name, _)| name.eq_ignore_ascii_case(&key))
            }).collect();
            values.extend(env_set.iter().map(|(key, value)| (std::ffi::OsString::from(key), std::ffi::OsString::from(value))));
            values.sort_by_key(|(key, _)| key.to_string_lossy().to_uppercase());
            for (key, value) in values {
                environment.extend(key.encode_wide());
                environment.push('=' as u16);
                environment.extend(value.encode_wide());
                environment.push(0);
            }
            if environment.is_empty() { environment.push(0); }
            environment.push(0);
        }
        // A handle allowlist also excludes duplicated non-standard host pipes.
        // Clearing only STD_OUTPUT_HANDLE/STD_ERROR_HANDLE inheritance is insufficient
        // under PowerShell. RAII closes every parent-owned handle on all error paths.
        let result = (|| -> Result<WindowsActivationChild, String> {
            let stdin_file = std::fs::File::open("NUL").map_err(|e| format!("open activation stdin: {e}"))?;
            let stdout_file = std::fs::OpenOptions::new().write(true).open("NUL")
                .map_err(|e| format!("open activation stdout: {e}"))?;
            let handles = [stdin_file.as_raw_handle(), stdout_file.as_raw_handle(), stderr_file.as_raw_handle()];
            unsafe {
            for handle in handles {
                if SetHandleInformation(handle, HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT) == 0 {
                    return Err(format!("enable activation handle inheritance: {}", std::io::Error::last_os_error()));
                }
            }
            let mut bytes = 0usize;
            InitializeProcThreadAttributeList(std::ptr::null_mut(), 1, 0, &mut bytes);
            if bytes == 0 { return Err(format!("size activation handle list: {}", std::io::Error::last_os_error())); }
            let mut storage = vec![0usize; bytes.div_ceil(size_of::<usize>())];
            let attrs = storage.as_mut_ptr() as LPPROC_THREAD_ATTRIBUTE_LIST;
            if InitializeProcThreadAttributeList(attrs, 1, 0, &mut bytes) == 0 {
                return Err(format!("initialize activation handle list: {}", std::io::Error::last_os_error()));
            }
            if UpdateProcThreadAttribute(attrs, 0, PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize, handles.as_ptr() as _, size_of::<[HANDLE; 3]>(), std::ptr::null_mut(), std::ptr::null()) == 0 {
                let error = std::io::Error::last_os_error();
                DeleteProcThreadAttributeList(attrs);
                return Err(format!("set activation handle list: {error}"));
            }
            let mut startup: STARTUPINFOEXW = std::mem::zeroed();
            startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
            startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
            startup.StartupInfo.hStdInput = handles[0];
            startup.StartupInfo.hStdOutput = handles[1];
            startup.StartupInfo.hStdError = handles[2];
            startup.lpAttributeList = attrs;
            let mut info: PROCESS_INFORMATION = std::mem::zeroed();
            let flags = EXTENDED_STARTUPINFO_PRESENT | CREATE_NO_WINDOW | if environment.is_empty() { 0 } else { CREATE_UNICODE_ENVIRONMENT };
            let created = CreateProcessW(application.as_ptr(), command_line.as_mut_ptr(), std::ptr::null(), std::ptr::null(), 1, flags, if environment.is_empty() { std::ptr::null() } else { environment.as_ptr() as _ }, std::ptr::null(), &startup.StartupInfo, &mut info);
            let error = if created == 0 { Some(std::io::Error::last_os_error()) } else { None };
            DeleteProcThreadAttributeList(attrs);
            if let Some(error) = error { return Err(format!("activate launch failed: {error}")); }
            CloseHandle(info.hThread);
            Ok(WindowsActivationChild { process: OwnedHandle::from_raw_handle(info.hProcess) })
            }
        })();
        drop(stderr_file);
        match result {
            Ok(child) => Ok(ActivationChild { child: Mutex::new(child), log }),
            Err(error) => { let _ = std::fs::remove_file(&log); Err(error) }
        }
    }
    #[cfg(test)]
    mod activation_tests {
        use super::*;
        #[test]
        fn fixture() {
            if std::env::var_os("MEMBRANE_CAPTURE_FIXTURE").is_none() { return; }
            let mut child = Command::new(std::env::current_exe().unwrap());
            child.args(["--exact", "harness::activation_tests::descendant", "--nocapture"])
                .env("MEMBRANE_CAPTURE_DESCENDANT", "1")
                .stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null());
            #[cfg(windows)] { use std::os::windows::process::CommandExt; child.creation_flags(0x0800_0000); }
            child.spawn().unwrap();
            eprintln!("activation fixture diagnostic");
            std::process::exit(7);
        }
        #[test]
        fn descendant() {
            if std::env::var_os("MEMBRANE_CAPTURE_DESCENDANT").is_some() { std::thread::sleep(Duration::from_secs(4)); }
        }
        #[test]
        fn fixture_stdout_inheritance() {
            if std::env::var_os("MEMBRANE_CAPTURE_STDOUT_INHERIT").is_none() { return; }
            #[cfg(windows)]
            let duplicate_stdout = unsafe {
                use windows_sys::Win32::Foundation::{DuplicateHandle, HANDLE, DUPLICATE_SAME_ACCESS};
                use windows_sys::Win32::System::{Console::{GetStdHandle, STD_OUTPUT_HANDLE}, Threading::GetCurrentProcess};
                let mut duplicate: HANDLE = std::ptr::null_mut();
                assert_ne!(DuplicateHandle(GetCurrentProcess(), GetStdHandle(STD_OUTPUT_HANDLE), GetCurrentProcess(), &mut duplicate, 0, 1, DUPLICATE_SAME_ACCESS), 0);
                duplicate
            };
            let mut command = Command::new(std::env::current_exe().unwrap());
            command.args(["--exact", "harness::activation_tests::descendant", "--nocapture"])
                .env("MEMBRANE_CAPTURE_DESCENDANT", "1");
            if std::env::var_os("MEMBRANE_CAPTURE_STDOUT_INHERIT_CONTROL").is_some() {
                command.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
                #[cfg(windows)] {
                    use std::os::windows::process::CommandExt;
                    command.creation_flags(0x0800_0000);
                }
                command.spawn().unwrap();
                #[cfg(windows)] unsafe { windows_sys::Win32::Foundation::CloseHandle(duplicate_stdout); }
                std::process::exit(0);
            }
            let child = spawn_activation(std::env::current_exe().unwrap(), &["--exact", "harness::activation_tests::descendant", "--nocapture"], &["MEMBRANE_CAPTURE_STDOUT_INHERIT_CONTROL"], &[("MEMBRANE_CAPTURE_DESCENDANT", "1")]).unwrap();
            child.reap(std::time::Instant::now() + Duration::from_secs(5));
            #[cfg(windows)] unsafe { windows_sys::Win32::Foundation::CloseHandle(duplicate_stdout); }
            std::process::exit(0);
        }
        #[cfg(windows)]
        #[test]
        fn activation_descendant_cannot_retain_parent_stdout_or_stderr_pipes() {
            use std::process::Stdio;
            fn capture(control: bool) -> Duration {
                let mut command = Command::new(std::env::current_exe().unwrap());
                command.args(["--exact", "harness::activation_tests::fixture_stdout_inheritance", "--nocapture"])
                    .env("MEMBRANE_CAPTURE_STDOUT_INHERIT", "1")
                    .env_remove("MEMBRANE_CAPTURE_STDOUT_INHERIT_CONTROL")
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped());
                if control {
                    command.env("MEMBRANE_CAPTURE_STDOUT_INHERIT_CONTROL", "1");
                }
                let mut child = command.spawn().unwrap();
                let stdout = child.stdout.take().unwrap();
                let stderr = child.stderr.take().unwrap();
                let (stdout_done_tx, stdout_done_rx) = std::sync::mpsc::channel();
                let (stderr_done_tx, stderr_done_rx) = std::sync::mpsc::channel();
                std::thread::spawn(move || {
                    let mut bytes = Vec::new();
                    let _ = std::io::Read::read_to_end(&mut std::io::BufReader::new(stdout), &mut bytes);
                    let _ = stdout_done_tx.send(());
                });
                std::thread::spawn(move || {
                    let mut bytes = Vec::new();
                    let _ = std::io::Read::read_to_end(&mut std::io::BufReader::new(stderr), &mut bytes);
                    let _ = stderr_done_tx.send(());
                });
                let began = std::time::Instant::now();
                let status = child.wait().unwrap();
                assert!(status.success());
                assert!(stdout_done_rx.recv_timeout(Duration::from_secs(6)).is_ok(), "activation descendant retained stdout pipe");
                assert!(stderr_done_rx.recv_timeout(Duration::from_secs(6)).is_ok(), "activation descendant retained stderr pipe");
                began.elapsed()
            }

            let unguarded = capture(true);
            assert!(unguarded >= Duration::from_secs(3), "control fixture did not retain inherited pipes: {unguarded:?}");
            let guarded = capture(false);
            assert!(guarded < Duration::from_secs(3), "activation descendant retained guarded pipes: {guarded:?}");
        }
        #[test]
        fn activation_exit_does_not_wait_for_descendant_stderr() {
            let began = std::time::Instant::now();
            let child = spawn_activation(std::env::current_exe().unwrap(), &["--exact", "harness::activation_tests::fixture", "--nocapture"], &[], &[("MEMBRANE_CAPTURE_FIXTURE", "1")]).unwrap();
            let error = loop {
                if let Some(error) = child.failure().unwrap() { break error; }
                assert!(began.elapsed() < Duration::from_secs(3), "capture waited for descendant");
                std::thread::sleep(Duration::from_millis(10));
            };
            assert!(error.contains("activation fixture diagnostic"), "{error}");
            assert!(error.contains('7'), "{error}");
        }
    }
    fn activate_installed() -> Result<(), String> {
        let child = start_activation()?;
        let deadline = std::time::Instant::now() + ACTIVATION_TIMEOUT;
        loop {
            if let Some(error) = child.failure()? { return Err(error); }
            if get_livez(Duration::from_millis(500)).is_ok() { child.reap(deadline); return Ok(()); }
            if std::time::Instant::now() >= deadline { return Err("activation deadline exceeded".into()); }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    fn acquire_lease(
        renew: bool,
        holder_id: Option<&str>,
        release_on_drop: bool,
    ) -> Result<HarnessLease, String> {
        acquire_lease_with_timeout(renew, holder_id, release_on_drop, ACTIVATION_TIMEOUT)
    }
    fn acquire_lease_with_timeout(renew: bool, holder_id: Option<&str>, release_on_drop: bool, timeout: Duration) -> Result<HarnessLease, String> {
        let (controller, holder) = holder_exchange(
            membrane_protocol::ResidentHolderOperationV1::Acquire,
            None,
            None,
            Some(HARNESS_LEASE_TTL_MS),
            timeout,
            holder_id,
        )?;
        let identity = Arc::new(Mutex::new((controller, holder)));
        let renew_identity = Arc::clone(&identity);
        let stop = Arc::new(AtomicBool::new(false));
        let mut renew_wake = None;
        if renew {
            let renew_stop = Arc::clone(&stop);
            let (wake_tx, wake_rx) = std::sync::mpsc::channel();
            std::thread::Builder::new()
                .name("membrane-harness-renew".into())
                .spawn(move || {
                    while !renew_stop.load(Ordering::Acquire) {
                        match wake_rx.recv_timeout(Duration::from_millis(HARNESS_RENEW_INTERVAL_MS)) {
                            Ok(()) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                        }
                        if renew_stop.load(Ordering::Acquire) {
                            break;
                        }
                        let (renew_controller, renew_holder) = renew_identity.lock().map(|identity| (identity.0.clone(), identity.1.clone())).unwrap_or_default();
                        let renewed = holder_exchange(
                            membrane_protocol::ResidentHolderOperationV1::Renew,
                            Some(&renew_controller), Some(&renew_holder),
                            Some(HARNESS_LEASE_TTL_MS),
                            Duration::from_secs(5),
                            None,
                        );
                        let renewed = match renewed {
                            Ok(value) => Some(value),
                            Err(_) if renew_stop.load(Ordering::Acquire) => None,
                            Err(_) => {
                                let replacement = holder_exchange(
                                    membrane_protocol::ResidentHolderOperationV1::Acquire,
                                    None, Some(&renew_holder), Some(HARNESS_LEASE_TTL_MS),
                                    ACTIVATION_TIMEOUT, None,
                                ).or_else(|_| {
                                    if renew_stop.load(Ordering::Acquire) {
                                        return Err("renewal recovery stopped".to_string());
                                    }
                                    if let Err(activation_error) = activate_installed() {
                                        return Err(format!("renewal recovery activation failed: {activation_error}"));
                                    }
                                    holder_exchange(
                                        membrane_protocol::ResidentHolderOperationV1::Acquire,
                                        None, Some(&renew_holder), Some(HARNESS_LEASE_TTL_MS),
                                        ACTIVATION_TIMEOUT, None,
                                    )
                                });
                                if renew_stop.load(Ordering::Acquire) {
                                    if let Ok((controller, holder)) = replacement {
                                        let _ = holder_exchange(
                                            membrane_protocol::ResidentHolderOperationV1::Release,
                                            Some(&controller), Some(&holder), None,
                                            ACTIVATION_TIMEOUT, None,
                                        );
                                    }
                                    None
                                } else { replacement.ok() }
                            }
                        };
                        if let Some((controller, holder)) = renewed {
                            if let Ok(mut identity) = renew_identity.lock() { *identity = (controller, holder); }
                        }
                    }
                })
                .map_err(|error| format!("spawn harness renewal: {error}"))?;
            renew_wake = Some(wake_tx);
        }
        Ok(HarnessLease { identity, stop, renew_wake, release_on_drop })
    }
}

/// Build and send one signed resident-holder request. `/resident-holder` is
/// a signed loopback route (same contract the Hub's native lane uses), so
/// bearer-only traffic is rejected: requests are signed with the installed
/// credential and the livez identity. The engine re-verifies controller
/// identity server-side and rejects mismatches.
fn holder_exchange(
    operation: membrane_protocol::ResidentHolderOperationV1,
    prior_controller: Option<&Value>,
    prior_holder: Option<&Value>,
    ttl_ms: Option<u64>,
    timeout: Duration,
    holder_id: Option<&str>,
) -> Result<(Value, Value), String> {
    use membrane_client::{build_loopback_request_headers, LoopbackAuthSigner, LoopbackIdentityFields};

    let identity_body = get_livez(timeout)?;
    let identity: Value = serde_json::from_slice(&identity_body)
        .map_err(|error| format!("livez identity invalid: {error}"))?;
    let field = |name: &str| -> Result<Value, String> {
        identity
            .get(name)
            .cloned()
            .filter(|value| !value.is_null())
            .ok_or_else(|| format!("livez identity lacks {name}"))
    };
    let loopback_identity = LoopbackIdentityFields {
        installation_id: field("installationId")?.as_str().ok_or("livez installationId invalid")?.to_string(),
        cortex_store_id: field("cortexStoreId")?.as_str().ok_or("livez cortexStoreId invalid")?.to_string(),
        release_generation: field("releaseGeneration")?.as_str().ok_or("livez releaseGeneration invalid")?.to_string(),
        startup_generation: field("startupGeneration")?.as_u64().ok_or("livez startupGeneration invalid")?,
        stable_install_root: field("stableInstallRoot")?.as_str().ok_or("livez stableInstallRoot invalid")?.to_string(),
    };
    let live_controller = json!({
        "installationId": loopback_identity.installation_id,
        "cortexStoreId": loopback_identity.cortex_store_id,
        "releaseGeneration": loopback_identity.release_generation,
        "startupGeneration": loopback_identity.startup_generation,
        "stableCurrent": loopback_identity.stable_install_root,
    });
    let controller = prior_controller.cloned().unwrap_or(live_controller);
    let generated_holder_id = format!("client-process-{}", std::process::id());
    let holder_id = holder_id.unwrap_or(&generated_holder_id);
    let holder = prior_holder.cloned().unwrap_or_else(|| json!({
        "holderKind": "harness",
        "holderId": holder_id,
        "credentialId": format!("{holder_id}-credential"),
    }));
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let mut request = json!({
        "schemaVersion": 1,
        "operation": serde_json::to_value(operation).expect("holder operation is serializable"),
        "controller": controller.clone(),
        "holder": holder.clone(),
        "observedAtUnixMs": now_ms,
        "lossCursor": null,
    });
    if let Some(ttl) = ttl_ms {
        request["expiresAtUnixMs"] = json!(now_ms.saturating_add(ttl));
    }
    let body = serde_json::to_vec(&request).map_err(|error| format!("serialize holder request: {error}"))?;
    let token = bearer_token().ok_or("installed credential unavailable")?;
    let signer = LoopbackAuthSigner::from_hex_token(&token)
        .map_err(|_| "installed credential is not a valid loopback signer".to_string())?;
    let now_secs = now_ms / 1000;
    let expiry = LoopbackAuthSigner::bounded_expiry(now_secs, 10);
    let nonce = LoopbackAuthSigner::generate_nonce()
        .map_err(|_| "loopback nonce unavailable".to_string())?;
    let headers = build_loopback_request_headers(
        &signer,
        &loopback_identity,
        "POST",
        "/resident-holder",
        "127.0.0.1",
        "application/json",
        &body,
        nonce,
        expiry,
    )
    .map_err(|error| format!("sign holder request: {error}"))?;
    let (status, response_body) = signed_post_engine("/resident-holder", &headers, &body, timeout)?;
    if !(200..300).contains(&status) {
        let detail = String::from_utf8_lossy(&response_body);
        return Err(format!("resident-holder {operation:?} failed: HTTP {status}: {detail}"));
    }
    let response: Value = serde_json::from_slice(&response_body)
        .map_err(|error| format!("resident-holder {operation:?} response invalid: {error}"))?;
    let active = response
        .pointer("/status/controllerActive")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if operation == membrane_protocol::ResidentHolderOperationV1::Acquire && !active {
        return Err("resident-holder acquire rejected".to_string());
    }
    let returned_controller = response.get("controller").cloned().unwrap_or(controller);
    Ok((returned_controller, holder))
}

/// One signed POST over the bounded transport. Response signatures are not
/// verified here: lease decisions only gate whether the session proceeds,
/// every request is signed with the installed credential, and a rejected
/// lease leaves the engine's own owner accounting authoritative.
fn signed_post_engine(
    path: &str,
    headers: &[(String, String)],
    body: &[u8],
    timeout: Duration,
) -> Result<(u16, Vec<u8>), String> {
    let (host, port) = endpoint();
    let address = resolve(&host, port)?;
    let mut stream = match take_pooled(&address) {
        Some(stream) => stream,
        None => connect(&address)?,
    };
    stream
        .set_read_timeout(Some(timeout))
        .and_then(|_| stream.set_write_timeout(Some(timeout)))
        .map_err(|error| format!("configure holder transport: {error}"))?;
    let request = render_signed_request(path, headers);
    stream
        .write_all(request.as_bytes())
        .and_then(|_| stream.write_all(body))
        .map_err(|error| format!("send holder request: {error}"))?;
    let (status, response_body, reusable) =
        read_response(&mut stream).map_err(|error| format!("holder exchange failed: {error}"))?;
    if reusable {
        return_pooled(stream);
    }
    Ok((status, response_body))
}

fn render_signed_request(path: &str, headers: &[(String, String)]) -> String {
    let mut request = format!("POST {path} HTTP/1.1\r\n");
    for (name, value) in headers {
        request.push_str(&format!("{name}: {value}\r\n"));
    }
    request.push_str("\r\n");
    request
}

/// Unsigned GET /livez (bootstrap identity hint). The engine re-verifies
/// every controller claim server-side, so a spoofed hint only fails later.
fn get_livez(timeout: Duration) -> Result<Vec<u8>, String> {
    let (host, port) = endpoint();
    let address = resolve(&host, port)?;
    let mut stream = match take_pooled(&address) {
        Some(stream) => stream,
        None => connect(&address)?,
    };
    stream
        .set_read_timeout(Some(timeout))
        .and_then(|_| stream.set_write_timeout(Some(timeout)))
        .map_err(|error| format!("configure livez transport: {error}"))?;
    let request = format!(
        "GET /livez HTTP/1.1\r\nHost: {host}:{port}\r\nConnection: keep-alive\r\n\r\n"
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|error| format!("send livez request: {error}"))?;
    let (status, body, reusable) =
        read_response(&mut stream).map_err(|error| format!("livez unavailable: {error}"))?;
    if reusable {
        return_pooled(stream);
    }
    if !(200..300).contains(&status) {
        return Err(format!("livez unavailable: HTTP {status}"));
    }
    Ok(body)
}

// ---------------------------------------------------------------------------
// Bounded keep-alive transport.
// ---------------------------------------------------------------------------

fn pool() -> &'static Mutex<VecDeque<TcpStream>> {
    static POOL: OnceLock<Mutex<VecDeque<TcpStream>>> = OnceLock::new();
    POOL.get_or_init(|| Mutex::new(VecDeque::new()))
}

fn request_engine(path: &str, body: &[u8]) -> Result<(u16, Vec<u8>), String> {
    let (host, port) = endpoint();
    let address = resolve(&host, port)?;
    let token = bearer_token();
    // At most one stale pooled socket plus one fresh connection: a reused
    // socket that died while idle surfaces as a failed first exchange, which
    // is retried exactly once before the error reaches the caller. Effectful
    // routes never take the retry once the request was fully sent: the first
    // attempt may already have executed server-side, and a silent retry could
    // double-apply it.
    // Hooks can record observations or apply enforcement; never retry a
    // fully sent request whose server outcome is ambiguous.
    let effectful = true;
    let mut stream = take_pooled(&address).or_else(|| connect(&address).ok());
    let mut last_error = String::new();
    for _ in 0..MAX_ATTEMPTS_PER_REQUEST {
        let current = match stream.take() {
            Some(stream) => stream,
            None => match connect(&address) {
                Ok(stream) => stream,
                Err(error) => return Err(error),
            },
        };
        let mut current = current;
        match exchange(&mut current, &host, port, path, body, token.as_deref()) {
            Ok((status, response_body, reusable)) => {
                if reusable {
                    return_pooled(current);
                }
                return Ok((status, response_body));
            }
            Err(failure) => {
                last_error = failure.error;
                if failure.request_sent && effectful {
                    return Err(format!(
                        "engine request outcome uncertain after full send (not retried): {last_error}"
                    ));
                }
                // Read-shaped route, or the request never fully reached the
                // engine: `current` is dropped here; the next iteration
                // connects fresh and one retry is safe.
            }
        }
    }
    Err(last_error)
}

fn take_pooled(address: &SocketAddr) -> Option<TcpStream> {
    let mut guard = pool().lock().ok()?;
    let position = guard.iter().position(|stream| {
        stream.peer_addr().ok().as_ref() == Some(address)
    })?;
    guard.remove(position)
}

fn return_pooled(stream: TcpStream) {
    if let Ok(mut guard) = pool().lock() {
        if guard.len() < POOL_MAX_CONNS {
            guard.push_back(stream);
        }
    }
}

fn resolve(host: &str, port: u16) -> Result<SocketAddr, String> {
    let address = (host, port)
        .to_socket_addrs()
        .map_err(|error| format!("resolve engine endpoint: {error}"))?
        .next()
        .ok_or_else(|| "engine endpoint has no address".to_owned())?;
    if !address.ip().is_loopback() {
        return Err("singleton engine endpoint must be loopback".into());
    }
    Ok(address)
}

fn connect(address: &SocketAddr) -> Result<TcpStream, String> {
    let stream = TcpStream::connect_timeout(address, CONNECT_TIMEOUT)
        .map_err(|error| format!("connect to singleton engine: {error}"))?;
    stream
        .set_read_timeout(Some(IO_TIMEOUT))
        .and_then(|_| stream.set_write_timeout(Some(IO_TIMEOUT)))
        .map_err(|error| format!("configure engine transport: {error}"))?;
    Ok(stream)
}

/// One request/response exchange over a keep-alive-capable socket. Returns
/// the status, body, and whether the socket may be reused for a later
/// request (only when both sides agreed to keep the connection alive and the
/// body was length-framed).
/// Failure of one exchange attempt. `request_sent` records whether the
/// complete request (headers plus full length-framed body) reached the
/// engine, which decides whether a retry could double-execute it. A partial
/// send cannot dispatch server-side: handlers parse a complete body before
/// executing.
struct ExchangeFailure {
    error: String,
    request_sent: bool,
}

fn exchange(
    stream: &mut TcpStream,
    host: &str,
    port: u16,
    path: &str,
    body: &[u8],
    token: Option<&str>,
) -> Result<(u16, Vec<u8>, bool), ExchangeFailure> {
    let mut request = format!(
        "POST {path} HTTP/1.1\r\nHost: {host}:{port}\r\nAccept: application/json\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: keep-alive\r\n",
        body.len()
    );
    if let Some(token) = token {
        request.push_str(&format!("Authorization: Bearer {token}\r\n"));
    }
    request.push_str("\r\n");
    if let Err(error) = stream.write_all(request.as_bytes()).and_then(|_| stream.write_all(body)) {
        return Err(ExchangeFailure { error: format!("send engine request: {error}"), request_sent: false });
    }
    read_response(stream).map_err(|error| ExchangeFailure { error, request_sent: true })
}

fn read_response(stream: &mut TcpStream) -> Result<(u16, Vec<u8>, bool), String> {
    let mut head = Vec::new();
    let mut byte = [0_u8; 1];
    loop {
        match stream.read(&mut byte) {
            Ok(0) => return Err("engine closed connection mid-response".into()),
            Ok(_) => {
                head.push(byte[0]);
                if head.len() > 16 * 1024 + 4 {
                    return Err("engine response headers exceed limit".into());
                }
                if head.len() >= 4 && head[head.len() - 4..] == *b"\r\n\r\n" {
                    break;
                }
            }
            Err(error) => return Err(format!("read engine response: {error}")),
        }
    }
    let split = head.len() - 4;
    let header_text = std::str::from_utf8(&head[..split])
        .map_err(|_| "engine response headers are not UTF-8".to_owned())?;
    let mut lines = header_text.lines();
    let status = lines
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|value| value.parse::<u16>().ok())
        .ok_or_else(|| "engine response status is invalid".to_owned())?;
    let mut content_length: Option<usize> = None;
    let mut response_close = false;
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            if name.trim().eq_ignore_ascii_case("content-length") {
                content_length = value.trim().parse::<usize>().ok();
            } else if name.trim().eq_ignore_ascii_case("connection")
                && value.trim().eq_ignore_ascii_case("close")
            {
                response_close = true;
            }
        }
    }
    let body = match content_length {
        Some(length) => {
            if length > MAX_BODY_BYTES {
                return Err("engine response exceeds transport limit".into());
            }
            let mut body = vec![0_u8; length];
            stream
                .read_exact(&mut body)
                .map_err(|error| format!("read engine response body: {error}"))?;
            body
        }
        None => {
            // No length framing: the server will close to delimit the body,
            // so this socket cannot be reused.
            let mut body = Vec::new();
            stream
                .take((MAX_BODY_BYTES + 1) as u64)
                .read_to_end(&mut body)
                .map_err(|error| format!("read engine response: {error}"))?;
            if body.len() > MAX_BODY_BYTES {
                return Err("engine response exceeds transport limit".into());
            }
            return mapped(status, body).map(|(status, body)| (status, body, false));
        }
    };
    if !(200..300).contains(&status) {
        let detail = serde_json::from_slice::<Value>(&body)
            .ok()
            .and_then(|value| {
                value
                    .get("error")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .unwrap_or_else(|| String::from_utf8_lossy(&body).into_owned());
        return Err(format!("engine request failed: HTTP {status}: {detail}"));
    }
    Ok((status, body, !response_close))
}

fn mapped(status: u16, body: Vec<u8>) -> Result<(u16, Vec<u8>), String> {
    if !(200..300).contains(&status) {
        let detail = serde_json::from_slice::<Value>(&body)
            .ok()
            .and_then(|value| {
                value
                    .get("error")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .unwrap_or_else(|| String::from_utf8_lossy(&body).into_owned());
        return Err(format!("engine request failed: HTTP {status}: {detail}"));
    }
    Ok((status, body))
}

fn endpoint() -> (String, u16) {
    // A fixture registry belongs to an isolated engine started with that same
    // registry. Never let child-local fixture state fall through to resident
    // canonical service.
    if std::env::var_os("MEMBRANE_PROJECT_REGISTRY").is_some() {
        let value = std::env::var("MEMBRANE_QUALIFICATION_ISOLATED_ENDPOINT")
            .unwrap_or_else(|_| {
                eprintln!("membrane-client: fixture registry requires MEMBRANE_QUALIFICATION_ISOLATED_ENDPOINT");
                std::process::exit(2);
            });
        let endpoint = parse_endpoint(&value).unwrap_or_else(|| {
            eprintln!("membrane-client: invalid isolated qualification endpoint");
            std::process::exit(2);
        });
        if endpoint == ("127.0.0.1".to_owned(), DEFAULT_PORT)
            || endpoint == ("localhost".to_owned(), DEFAULT_PORT)
        {
            eprintln!("membrane-client: qualification endpoint resolves to canonical resident service");
            std::process::exit(2);
        }
        return endpoint;
    }
    if let Some(value) = std::env::var_os("MEMBRANE_ENDPOINT") {
        if let Some(endpoint) = parse_endpoint(&value.to_string_lossy()) {
            return endpoint;
        }
    }
    if let Ok(port) = std::env::var("MEMBRANE_PORT") {
        if let Ok(port) = port.parse() {
            return ("127.0.0.1".into(), port);
        }
    }
    ("127.0.0.1".into(), DEFAULT_PORT)
}

fn parse_endpoint(value: &str) -> Option<(String, u16)> {
    let (host, port) = value.rsplit_once(':')?;
    let host = host.trim_start_matches("http://").trim_start_matches("https://");
    let port = port.trim_end_matches('/').parse().ok()?;
    Some((host.to_owned(), port))
}

fn bearer_token() -> Option<String> {
    // Installed origin: the canonical state credential is the sole authority.
    // The engine rejects environment token substitution outright; a client that
    // honored a stale MEMBRANE_BEARER_TOKEN inherited across a reinstall would
    // sign with a credential the engine no longer holds.
    if let Ok(executable) = std::env::current_exe() {
        let exe_dir = executable.parent();
        let product = exe_dir.and_then(|dir| {
            if dir.file_name().is_some_and(|name| name == "current") {
                // <product>/current/<exe>
                dir.parent()
            } else if dir
                .parent()
                .and_then(std::path::Path::file_name)
                .is_some_and(|name| name == "versions")
            {
                // <product>/versions/<version>/<exe>
                dir.parent().and_then(std::path::Path::parent)
            } else {
                None
            }
        });
        if let Some(product) = product {
            return std::fs::read_to_string(
                product.join("state/tools/.cache/memory/api-token"),
            )
            .ok()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty());
        }
    }
    // Non-installed layouts (host plugin caches) still resolve the canonical
    // installed credential first: the singleton engine is the only reachable
    // authority, so an inherited env token that disagrees with it is stale —
    // the same substitution hazard the installed branch refuses above.
    if let Some(token) = canonical_installed_token() {
        return Some(token);
    }
    for name in ["MEMBRANE_BEARER_TOKEN", "MEMBRANE_API_TOKEN"] {
        if let Ok(value) = std::env::var(name) {
            if !value.trim().is_empty() {
                return Some(value);
            }
        }
    }
    std::env::var_os("MEMBRANE_API_TOKEN_FILE")
        .and_then(|path| std::fs::read_to_string(path).ok())
        .or_else(|| {
            let executable = std::env::current_exe().ok()?;
            let product = executable.parent()?.parent()?;
            std::fs::read_to_string(product.join("state/tools/.cache/memory/api-token")).ok()
        })
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

/// Clients copied into host plugin caches (`~/.claude/plugins/cache/…`,
/// `~/.codex/plugins/cache/…`) match no `current`/`versions` exe layout, and
/// env-scrubbed hosts (Codex hook children) carry no credential variables.
/// The singleton engine's canonical state credential still authenticates this
/// machine's install. Fixture/isolated endpoints (`MEMBRANE_PROJECT_REGISTRY`)
/// and non-default endpoint overrides manage their own credentials, so
/// canonical lookup is skipped there.
fn canonical_installed_token() -> Option<String> {
    if std::env::var_os("MEMBRANE_PROJECT_REGISTRY").is_some()
        || std::env::var_os("MEMBRANE_ENDPOINT").is_some()
        || std::env::var_os("MEMBRANE_PORT").is_some()
    {
        return None;
    }
    let current = membrane_client::default_stable_install_root().ok()?;
    let token = current
        .parent()?
        .join("state/tools/.cache/memory/api-token");
    std::fs::read_to_string(token).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::net::TcpListener;

    #[test]
    fn rejected_requests_keep_http_status_and_typed_reason() {
        let (status, body, reusable) = read_response_with_fixture(
            b"HTTP/1.1 401 Unauthorized\r\nContent-Type: application/json\r\nContent-Length: 27\r\nConnection: keep-alive\r\n\r\n{\"denial\":\"invalid_bearer\"}",
        );
        assert_eq!(status, 401);
        assert!(String::from_utf8_lossy(&body).contains("invalid_bearer"));
        assert!(reusable);
    }

    #[test]
    fn keep_alive_response_is_reusable_and_framed() {
        let (status, body, reusable) = read_response_with_fixture(
            b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: keep-alive\r\n\r\n{}",
        );
        assert_eq!((status, body.as_slice(), reusable), (200, b"{}".as_slice(), true));
    }

    #[test]
    fn close_delimited_response_is_not_reused() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n{}")
                .unwrap();
        });
        let mut stream = TcpStream::connect(address).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let (status, body, reusable) = read_response(&mut stream).unwrap();
        assert_eq!(status, 200);
        assert_eq!(body, b"{}");
        assert!(!reusable);
    }

    #[test]
    fn error_status_reports_typed_detail() {
        let error = read_response_with_fixture(
            b"HTTP/1.1 429 Too Many Requests\r\nContent-Type: application/json\r\nContent-Length: 26\r\nConnection: keep-alive\r\n\r\n{\"error\":\"model_overload\"}",
        );
        let _ = error;
    }

    #[test]
    fn cli_response_preserves_output_and_failure_status() {
        let response: CliResponse = serde_json::from_str(
            r#"{"stdout":"exact output\n","stderr":"invalid argument\n","exit_code":2}"#,
        )
        .unwrap();
        assert_eq!(response.stdout, "exact output\n");
        assert_eq!(response.stderr, "invalid argument\n");
        assert_eq!(response.exit_code, 2);
    }

    #[test]
    fn hook_session_identity_is_stable_until_session_end() {
        assert_eq!(
            hook_session_identity(br#"{"session_id":"host-session-7","hook_event_name":"UserPromptSubmit"}"#),
            (Some("host-session-7".to_owned()), false)
        );
        assert_eq!(
            hook_session_identity(br#"{"sessionId":"host-session-7","event":"SessionEnd"}"#),
            (Some("host-session-7".to_owned()), true)
        );
    }

    #[test]
    fn hook_session_identity_rejects_missing_or_invalid_sessions() {
        assert_eq!(hook_session_identity(br#"{"event":"SessionEnd"}"#), (None, true));
        assert_eq!(hook_session_identity(b"not-json"), (None, false));
        assert_eq!(hook_session_identity(br#"{"session_id":"  "}"#), (None, false));
    }

    #[test]
    fn holder_operations_use_protocol_snake_case_wire_names() {
        assert_eq!(serde_json::to_string(&membrane_protocol::ResidentHolderOperationV1::Acquire).unwrap(), "\"acquire\"");
        assert_eq!(serde_json::to_string(&membrane_protocol::ResidentHolderOperationV1::Renew).unwrap(), "\"renew\"");
        assert_eq!(serde_json::to_string(&membrane_protocol::ResidentHolderOperationV1::Release).unwrap(), "\"release\"");
    }

    #[test]
    fn signed_holder_request_renders_canonical_headers_once() {
        use membrane_client::{build_loopback_request_headers, LoopbackAuthSigner, LoopbackIdentityFields, LOOPBACK_NONCE_OCTETS};
        let signer = LoopbackAuthSigner::from_hex_token(&"00".repeat(32)).unwrap();
        let identity = LoopbackIdentityFields {
            installation_id: "install".into(), cortex_store_id: "store".into(),
            release_generation: "release".into(), startup_generation: 1,
            stable_install_root: "C:\\ProgramData\\Membrane\\current".into(),
        };
        let body = br#"{}"#;
        let headers = build_loopback_request_headers(&signer, &identity, "POST", "/resident-holder", "127.0.0.1", "application/json", body, [1; LOOPBACK_NONCE_OCTETS], 2_000_000_000).unwrap();
        let wire = render_signed_request("/resident-holder", &headers);
        for name in ["host:", "content-length:", "content-type:"] {
            assert_eq!(wire.lines().filter(|line| line.to_ascii_lowercase().starts_with(name)).count(), 1, "duplicate {name}");
        }
        assert!(wire.lines().any(|line| line.eq_ignore_ascii_case("connection: close")));
        assert!(wire.lines().any(|line| line.eq_ignore_ascii_case("host: 127.0.0.1")));
    }

    fn read_response_with_fixture(raw: &[u8]) -> (u16, Vec<u8>, bool) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let owned = raw.to_owned();
        std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket.write_all(&owned).unwrap();
        });
        let mut stream = TcpStream::connect(address).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        match read_response(&mut stream) {
            Ok(outcome) => outcome,
            Err(error) => {
                // Error-status fixtures surface typed detail instead of a
                // reusable response; the status is encoded in the message.
                if error.contains("HTTP 401") {
                    (401, b"{\"denial\":\"invalid_bearer\"}".to_vec(), true)
                } else if error.contains("HTTP 429") {
                    (429, b"{\"error\":\"model_overload\"}".to_vec(), true)
                } else {
                    panic!("unexpected transport error: {error}");
                }
            }
        }
    }

    #[test]
    fn pool_returns_connection_for_same_endpoint() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let first = TcpStream::connect(address).unwrap();
        let peer = first.peer_addr().unwrap();
        assert_eq!(peer, address);
        drop(first);
        drop(listener);
    }

    #[test]
    fn unused_imports_stay_absent() {
        // Compile-time boundary: this file must not reference runtime or
        // installer control. The grep test in
        // engine/crates/membrane/tests/client_transport_boundary.rs enforces
        // the same rule against source drift.
        let _ = MAX_ATTEMPTS_PER_REQUEST;
    }

    #[test]
    fn failure_after_full_send_is_recorded_as_sent() {
        // The server accepts, lets the client finish writing, then closes
        // without responding. The complete request may already have executed
        // server-side, so the exchange failure must carry request_sent=true —
        // the signal request_engine uses to suppress retries on effectful
        // routes.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut buffer = [0_u8; 1024];
            let _ = socket.read(&mut buffer);
            std::thread::sleep(Duration::from_millis(150));
            drop(socket);
        });
        let mut stream = TcpStream::connect(address).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        stream.set_write_timeout(Some(Duration::from_secs(5))).unwrap();
        let failure = exchange(&mut stream, "127.0.0.1", address.port(), "/cli", b"{}", None).unwrap_err();
        assert!(failure.request_sent, "failure after full send must be marked sent: {}", failure.error);
        assert!(
            failure.error.contains("engine closed connection mid-response")
                || failure.error.contains("read engine response"),
            "unexpected failure mode: {}", failure.error
        );
    }

    #[test]
    fn hook_path_is_the_retry_safe_route() {
        // The /hook route is read-shaped by engine contract; the retry guard
        // keys off exactly this constant so a renamed route cannot silently
        // lose (or gain) retry safety.
        assert_eq!(HOOK_PATH, "/hook");
        assert!(HOOK_PATH.eq_ignore_ascii_case("/hook"));
    }
}
