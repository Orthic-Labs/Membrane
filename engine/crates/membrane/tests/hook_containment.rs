#![cfg(windows)]

use std::{fs, io::Write, process::{Command, Stdio}, thread, time::{Duration, SystemTime, UNIX_EPOCH}};

/// Exercises installed-binary `hook` -> private `hook-module` containment.
/// The fake git command starts a delayed descendant write then blocks past the
/// module deadline. Job-object teardown must prevent that late write before
/// HookHost can return and begin its next public invocation.
#[test]
fn hook_timeout_reaps_delayed_descendant_before_next_module() {
    let unique = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let root = std::env::temp_dir().join(format!("membrane-hook-containment-{unique}"));
    let bin = root.join("bin"); let late = root.join("late-write.txt"); let early_late = root.join("early-root-late-write.txt");
    fs::create_dir_all(&bin).unwrap();
    let script = format!("@echo off\r\nif \"%MEMBRANE_HOOK_EARLY_EXIT%\"==\"1\" goto early\r\nstart \"\" /b cmd /c \"%SystemRoot%\\System32\\ping.exe -n 5 127.0.0.1 ^>nul ^& echo late>{}\"\r\n%SystemRoot%\\System32\\ping.exe -n 11 127.0.0.1 >nul\r\nexit /b 0\r\n:early\r\nstart \"\" /b cmd /c \"%SystemRoot%\\System32\\ping.exe -n 5 127.0.0.1 ^>nul ^& echo late>{}\"\r\nexit /b 0\r\n", late.display(), early_late.display());
    fs::write(bin.join("git.cmd"), script).unwrap();
    let payload = r#"{"event":"PostToolUse","tool_name":"apply_patch","tool_input":{"patch":"*** Begin Patch\n*** Update File: changed.rs\n"}}"#;
    let executable = env!("CARGO_BIN_EXE_membrane");
    let started = std::time::Instant::now();
    let mut output = Command::new(executable).arg("hook").env("WORKSPACE_ROOT", &root)
        .env("PATH", format!("{};{}", bin.display(), std::env::var("PATH").unwrap_or_default()))
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap();
    output.stdin.as_mut().unwrap().write_all(payload.as_bytes()).unwrap();
    drop(output.stdin.take());
    let result = output.wait_with_output().unwrap();
    assert!(started.elapsed() < Duration::from_secs(5), "timeout must return before delayed git");
    assert!(result.status.success());
    let response: serde_json::Value = serde_json::from_slice(&result.stdout).expect("hook response JSON");
    let diagnostics = response.pointer("/membraneHook/results").and_then(serde_json::Value::as_array)
        .and_then(|results| results.iter().find(|entry| entry["id"] == "membrane.diagnostics-observe"))
        .expect("diagnostics observation result");
    assert_eq!(diagnostics["error"], "module_deadline_exceeded", "fake git must cross contained module deadline");
    let mut next = Command::new(executable).arg("hook").env("WORKSPACE_ROOT", &root)
        .stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
    next.stdin.as_mut().unwrap().write_all(br#"{"event":"FutureHostEvent"}"#).unwrap();
    drop(next.stdin.take());
    assert!(next.wait().unwrap().success(), "next hook invocation begins only after timeout containment returns");
    thread::sleep(Duration::from_secs(2));
    assert!(!late.exists(), "Job containment must reap delayed descendant before next public hook invocation");
    let mut early = Command::new(executable).arg("hook").env("WORKSPACE_ROOT", &root).env("MEMBRANE_HOOK_EARLY_EXIT", "1")
        .env("PATH", format!("{};{}", bin.display(), std::env::var("PATH").unwrap_or_default()))
        .stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
    early.stdin.as_mut().unwrap().write_all(payload.as_bytes()).unwrap();
    drop(early.stdin.take());
    assert!(early.wait().unwrap().success(), "early-exit root hook returns");
    thread::sleep(Duration::from_secs(5));
    assert!(!early_late.exists(), "early-exit root descendant must not retain stdout or write after containment");
    let _ = fs::remove_dir_all(root);
}

/// LC-06 negative control: "Hook that executes an interpreter fails." The
/// installed `hook` surface must service the module directly through the
/// native binary; it must never shell out to a `python`/`node`/`sh`
/// interpreter to service a host event. Stripping `PATH` down to a
/// directory containing none of those interpreters and confirming the
/// installed binary still returns a typed, successful hook response proves
/// no interpreter dependency exists on this path — a shim-routed
/// implementation would fail to spawn once the interpreter is unreachable.
#[test]
fn hook_module_services_event_without_any_interpreter_on_path() {
    let unique = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let root = std::env::temp_dir().join(format!("membrane-hook-no-interpreter-{unique}"));
    fs::create_dir_all(&root).unwrap();
    let payload = r#"{"event":"PostToolUse","tool_name":"apply_patch","tool_input":{"patch":"*** Begin Patch\n*** Update File: changed.rs\n"}}"#;
    let executable = env!("CARGO_BIN_EXE_membrane");
    let mut child = Command::new(executable)
        .arg("hook")
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
