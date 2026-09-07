//! Explicit Blueprint application requests may run without resident authority.
//! Graph semantics & storage remain in the packaged Blueprint owner.
use membrane_federation::blueprint_client::{BlueprintBounds, BlueprintClientError,
    BlueprintTransport, BlueprintWireRequest, BlueprintWireResponse};
use std::{io::{Read, Write}, path::PathBuf, process::{Command, Stdio},
    sync::mpsc, time::{Duration, Instant}};
use tokio_util::sync::CancellationToken;

pub(crate) struct OneShotTransport;

pub(crate) struct ExplicitBlueprintTransport {
    pub endpoint: Option<PathBuf>,
}

impl BlueprintTransport for ExplicitBlueprintTransport {
    fn exchange(&self, request: &BlueprintWireRequest, bounds: BlueprintBounds,
        deadline: Duration, cancellation: CancellationToken) -> Result<BlueprintWireResponse, BlueprintClientError> {
        let started = Instant::now();
        if let Some(endpoint) = &self.endpoint {
            use membrane_federation::blueprint_client::UnixBlueprintTransport;
            let response = UnixBlueprintTransport::new(endpoint.clone()).exchange(request, bounds,
                deadline.min(Duration::from_secs(2)), cancellation.clone());
            match response {
                Err(BlueprintClientError::Unavailable(_)) => {},
                Ok(response) if !response.ok && response.error.as_ref().is_some_and(|error| matches!(error.code.as_deref(), Some("root_not_enrolled" | "graph_missing" | "not_configured"))) => {},
                other => return other,
            }
        }
        OneShotTransport.exchange(request, bounds, deadline.saturating_sub(started.elapsed()), cancellation)
    }
}

/// Explicit CLI work has finite lifetime & never grants resident authority.
pub(crate) fn run_cli(args: &[String]) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|error| error.to_string())?;
    let runtime = exe.parent().ok_or("installed runtime root missing")?.join("runtime/blueprint");
    let node = runtime.join("lib").join(if cfg!(windows) { "node.exe" } else { "node" });
    let entry = runtime.join("app/package/scripts/blueprint.mjs");
    if !node.is_file() || !entry.is_file() { return Err("packaged Blueprint CLI missing".into()); }
    let mut command = Command::new(node);
    command.arg(entry).args(args).env_clear().stdin(Stdio::null())
        .stdout(Stdio::inherit()).stderr(Stdio::inherit());
    for key in ["PATH", "HOME", "USERPROFILE", "LOCALAPPDATA", "APPDATA", "SYSTEMROOT", "TEMP", "TMP", "TMPDIR"] {
        if let Some(value) = std::env::var_os(key) { command.env(key, value); }
    }
    #[cfg(windows)] {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut process = crate::providers::child_process::spawn_contained_command(command).map_err(|error| error.to_string())?;
    let started = Instant::now();
    eprintln!("{}", serde_json::json!({"event":"blueprint_cli_started","pid":process.child.id()}));
    let result = loop {
        match process.child.try_wait() {
            Ok(Some(status)) => break if status.success() { Ok(()) } else { Err(format!("Blueprint exited with {status}")) },
            Err(error) => break Err(error.to_string()),
            Ok(None) if started.elapsed() >= Duration::from_secs(300) => break Err("Blueprint explicit command deadline exceeded".into()),
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
        }
    };
    process.kill_tree();
    eprintln!("{}", serde_json::json!({"event":"blueprint_cli_finished","elapsedMs":started.elapsed().as_millis(),"status":if result.is_ok(){"completed"}else{"failed"}}));
    result
}

impl BlueprintTransport for OneShotTransport {
    fn exchange(&self, request: &BlueprintWireRequest, bounds: BlueprintBounds,
        deadline: Duration, cancellation: CancellationToken) -> Result<BlueprintWireResponse, BlueprintClientError> {
        let started = Instant::now();
        let exe = std::env::current_exe().map_err(|e| BlueprintClientError::Unavailable(e.to_string()))?;
        let generation = exe.parent().ok_or_else(|| BlueprintClientError::Unavailable("runtime root missing".into()))?;
        let runtime = generation.join("runtime").join("blueprint");
        let node = runtime.join("lib").join(if cfg!(windows) { "node.exe" } else { "node" });
        let entry = runtime.join("app/package/scripts/blueprint-one-shot.mjs");
        if !node.is_file() || !entry.is_file() {
            return Err(BlueprintClientError::Unavailable("packaged Blueprint one-shot runtime missing".into()));
        }
        let root = request.input.get("repoRoot").and_then(serde_json::Value::as_str)
            .map(PathBuf::from).ok_or_else(|| BlueprintClientError::Malformed("explicit root missing".into()))?;
        let input = serde_json::to_vec(request).map_err(|e| BlueprintClientError::Malformed(e.to_string()))?;
        if input.len() > 65536 { return Err(BlueprintClientError::Oversized("request_bytes")); }
        let mut command = Command::new(node);
        command.arg(entry).current_dir(root).env_clear()
            .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        for key in ["PATH", "HOME", "USERPROFILE", "LOCALAPPDATA", "APPDATA", "SYSTEMROOT", "TEMP", "TMP", "TMPDIR"] {
            if let Some(value) = std::env::var_os(key) { command.env(key, value); }
        }
        #[cfg(windows)] {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let mut process = crate::providers::child_process::spawn_contained_command(command)
            .map_err(|e| BlueprintClientError::Unavailable(e.to_string()))?;
        eprintln!("{}", serde_json::json!({"event":"blueprint_one_shot_started","requestId":request.request_id,"pid":process.child.id()}));
        let mut stdin = process.child.stdin.take().expect("piped stdin");
        let stdout = process.child.stdout.take().expect("piped stdout");
        let stderr = process.child.stderr.take().expect("piped stderr");
        let limit = bounds.max_response_bytes;
        let (sender, receiver) = mpsc::sync_channel(1);
        let reader = std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let result = stdout.take(limit as u64 + 1).read_to_end(&mut bytes).map(|_| bytes);
            let _ = sender.send(result);
        });
        let errors = std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = stderr.take(8192).read_to_end(&mut bytes);
        });
        let result = (|| {
            stdin.write_all(&input).map_err(|e| BlueprintClientError::Unavailable(e.to_string()))?;
            drop(stdin);
            loop {
                if cancellation.is_cancelled() { return Err(BlueprintClientError::Cancelled); }
                if started.elapsed() >= deadline { return Err(BlueprintClientError::Timeout); }
                match receiver.recv_timeout(Duration::from_millis(10)) {
                    Ok(Ok(bytes)) => {
                        if bytes.len() > limit { return Err(BlueprintClientError::Oversized("response_bytes")); }
                        return serde_json::from_slice(&bytes).map_err(|e| BlueprintClientError::Malformed(e.to_string()));
                    }
                    Ok(Err(e)) => return Err(BlueprintClientError::Unavailable(e.to_string())),
                    Err(mpsc::RecvTimeoutError::Disconnected) => return Err(BlueprintClientError::Unavailable("one-shot reader disconnected".into())),
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                }
            }
        })();
        process.kill_tree();
        let _ = reader.join();
        let _ = errors.join();
        eprintln!("{}", serde_json::json!({"event":"blueprint_one_shot_finished","requestId":request.request_id,
            "elapsedMs":started.elapsed().as_millis(),"status":if result.is_ok(){"completed"}else{"failed"}}));
        result
    }
}
