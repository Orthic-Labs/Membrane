#![cfg(windows)]

use std::{fs, io::{Read, Write}, net::TcpListener, process::{Command, Stdio}, thread, time::{Duration, SystemTime, UNIX_EPOCH}};

/// Exercises in-process hook dispatch with a bounded leaf scope (no
/// `membrane.exe hook-module` child is ever spawned).
///
/// Routing matters here: only `fence()` (invoked from `PreToolUse`/`Stop`)
/// ever shells out to `git` (see `hook_diagnostics.rs::fence` calling
/// `changed_paths_from_git` -> `bounded_git`, a tracked leaf helper under
/// strict Job/process-group containment).
/// `observe_mutation()` (the `PostToolUse` module) deliberately derives
/// changed paths from the patch payload alone and never spawns a process, so
/// driving this scenario through `PostToolUse` would never exercise leaf
/// containment at all. This test instead sends a `Stop` event, which
/// dispatches to `DiagnosticsCompletionFence` -> `fence(completion: true)`,
/// so the planted fake `git.exe` is genuinely spawned as a tracked leaf of
/// the in-process module worker.
///
/// The fake git command starts a delayed descendant write then blocks past
/// the module deadline. Scope teardown must prevent that late write before
/// HookHost can return and begin its next public invocation.
///
/// The driver binary reads one payload from stdin and runs the same
/// `run_hook_payload` dispatcher the resident `/hook` route uses, so the
/// test can plant a hostile environment (fake git on PATH, stub resident)
/// per child without mutating the test process itself.
#[test]
fn hook_timeout_reaps_delayed_descendant_before_next_module() {
    let unique = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let root = std::env::temp_dir().join(format!("membrane-hook-containment-{unique}"));
    let bin = root.join("bin"); let late = root.join("late-write.txt"); let early_late = root.join("early-root-late-write.txt");
    fs::create_dir_all(&bin).unwrap();
    // Windows resolves a bare `Command::new("git")` (see hook_diagnostics.rs
    // bounded_git) through CreateProcess, which only appends `.exe` - never
    // `.cmd`/`.bat`. A `.cmd` shim is therefore never reached; only a real
    // git.exe on PATH can intercept the call. Copy the test-support binary
    // (never a shipped product binary - see its own doc comment) into place
    // as `bin/git.exe` instead.
    let fake_git = env!("CARGO_BIN_EXE_hook_containment_fake_git");
    fs::copy(fake_git, bin.join("git.exe")).unwrap();

    // Minimal resident-status HTTP stub: `fence()` first calls
    // `GET /diagnostics/workspace/status` and only reaches
    // `changed_paths_from_git` (the fake git) once that call reports the
    // workspace's bound project root back to it.
    let canonical_root = fs::canonicalize(&root).unwrap();
    let status_body = format!(r#"{{"projectRoot":{}}}"#, serde_json::to_string(&canonical_root.to_string_lossy().into_owned()).unwrap());
    let status_response = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}", status_body.len(), status_body);
    let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let listener = TcpListener::bind(("127.0.0.1", port)).unwrap();
    let stub_body = status_response.clone();
    let stub = thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break; };
            let mut buffer = [0_u8; 4096];
            let _ = stream.read(&mut buffer);
            let _ = stream.write_all(stub_body.as_bytes());
        }
    });

    let payload = br#"{"event":"Stop"}"#;
    let executable = env!("CARGO_BIN_EXE_hook_test_driver");
    let hook_env = |command: &mut Command| {
        command.env("WORKSPACE_ROOT", &root)
            .env("MEMBRANE_DIAGNOSTICS_ENFORCE", "1")
            .env("MEMBRANE_PORT", port.to_string())
            .env("MEMBRANE_FAKE_GIT_LATE_MARKER", &late)
            .env("MEMBRANE_FAKE_GIT_EARLY_LATE_MARKER", &early_late)
            .env("PATH", format!("{};{}", bin.display(), std::env::var("PATH").unwrap_or_default()));
    };
    let started = std::time::Instant::now();
    let mut command = Command::new(executable);
    hook_env(&mut command);
    let mut output = command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap();
    output.stdin.as_mut().unwrap().write_all(payload).unwrap();
    drop(output.stdin.take());
    let result = output.wait_with_output().unwrap();
    assert!(started.elapsed() < Duration::from_secs(5), "timeout must return before delayed git");
    assert!(result.status.success());
    let response: serde_json::Value = serde_json::from_slice(&result.stdout).expect("hook response JSON");
    let diagnostics = response.pointer("/membraneHook/results").and_then(serde_json::Value::as_array)
        .and_then(|results| results.iter().find(|entry| entry["id"] == "membrane.diagnostics-completion-fence"))
        .expect("diagnostics completion-fence result");
    assert_eq!(diagnostics["error"], "module_deadline_exceeded", "fake git must cross the in-process module deadline");
    let mut next_command = Command::new(executable);
    hook_env(&mut next_command);
    let mut next = next_command
        .env("WORKSPACE_ROOT", &root)
        .stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
    next.stdin.as_mut().unwrap().write_all(br#"{"event":"FutureHostEvent"}"#).unwrap();
    drop(next.stdin.take());
    assert!(next.wait().unwrap().success(), "next hook invocation begins only after timeout containment returns");
    thread::sleep(Duration::from_secs(2));
    assert!(!late.exists(), "leaf-scope teardown must reap delayed descendant before next public hook invocation");
    let mut early_command = Command::new(executable);
    early_command.env("MEMBRANE_HOOK_EARLY_EXIT", "1");
    hook_env(&mut early_command);
    let mut early = early_command.stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
    early.stdin.as_mut().unwrap().write_all(payload).unwrap();
    drop(early.stdin.take());
    assert!(early.wait().unwrap().success(), "early-exit root hook returns");
    thread::sleep(Duration::from_secs(5));
    assert!(!early_late.exists(), "early-exit root descendant must not retain stdout or write after containment");
    // The stub thread loops on `listener.incoming()` for the life of the
    // process; it is reclaimed on test-process exit, not joined here.
    let _ = stub;
    let _ = fs::remove_dir_all(root);
}

/// Negative control: "Hook that executes an interpreter fails." The
/// installed `hook` surface must service the module directly through the
/// native binary; it must never shell out to a `python`/`node`/`sh`
/// interpreter to service a host event. Stripping `PATH` down to a
/// directory containing none of those interpreters and confirming the
/// driver still returns a typed, successful hook response proves no
/// interpreter dependency exists on this path — a shim-routed
/// implementation would fail to spawn once the interpreter is unreachable.
#[test]
fn hook_module_services_event_without_any_interpreter_on_path() {
    let unique = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let root = std::env::temp_dir().join(format!("membrane-hook-no-interpreter-{unique}"));
    fs::create_dir_all(&root).unwrap();
    let payload = r#"{"event":"PostToolUse","tool_name":"apply_patch","tool_input":{"patch":"*** Begin Patch\n*** Update File: changed.rs\n"}}"#;
    let executable = env!("CARGO_BIN_EXE_hook_test_driver");
    let mut child = Command::new(executable)
        .env("WORKSPACE_ROOT", &root)
        .env("PATH", "/__membrane_no_interpreters__")
        .env_remove("PYTHONPATH")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("native hook module starts without any interpreter reachable");
    child.stdin.as_mut().unwrap().write_all(payload.as_bytes()).unwrap();
    drop(child.stdin.take());
    let result = child.wait_with_output().unwrap();
    assert!(
        result.status.success(),
        "hook module must service the event natively with no interpreter on PATH"
    );
    let response: serde_json::Value =
        serde_json::from_slice(&result.stdout).expect("hook response JSON");
    assert!(
        response.get("membraneHook").is_some(),
        "response must be the typed native hook payload, not an interpreter fallback"
    );
    let _ = fs::remove_dir_all(root);
}
