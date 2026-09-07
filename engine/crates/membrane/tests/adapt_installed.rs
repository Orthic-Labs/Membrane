//! N5 exact-candidate qualification for native Adapt.

use std::{fs, path::Path, process::Command};

use serde_json::{json, Value};

fn run_json(binary: &Path, cwd: &Path, args: &[&str]) -> Value {
    let output = Command::new(binary)
        .args(args)
        .current_dir(cwd)
        .env("PATH", "/__membrane_no_interpreters__")
        .env_remove("PYTHONPATH")
        .output()
        .expect("native Membrane candidate starts");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("native Adapt emits JSON")
}

fn write_json(path: &Path, value: &Value) {
    fs::write(path, serde_json::to_vec(value).unwrap()).unwrap();
}

#[test]
fn installed_explicit_adapt_uses_canonical_store_with_hub_off() {
    let temp = tempfile::tempdir().unwrap();
    let product = temp.path().join("Membrane");
    let version = product.join("versions/test");
    let current = product.join("current");
    fs::create_dir_all(&version).unwrap();
    let filename = if cfg!(windows) { "membrane.exe" } else { "membrane" };
    fs::copy(env!("CARGO_BIN_EXE_membrane"), version.join(filename)).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&version, &current).unwrap();
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let linked = Command::new("powershell.exe").args(["-NoProfile", "-NonInteractive", "-Command",
            "New-Item -ItemType Junction -Path $env:MEMBRANE_TEST_LINK -Target $env:MEMBRANE_TEST_TARGET -ErrorAction Stop | Out-Null"])
            .env("MEMBRANE_TEST_LINK", &current).env("MEMBRANE_TEST_TARGET", &version)
            .creation_flags(0x08000000).output().unwrap();
        assert!(linked.status.success(), "{}", String::from_utf8_lossy(&linked.stderr));
    }
    let output = Command::new(current.join(filename))
        .args(["adapt", "status"])
        .current_dir(temp.path())
        .env("HOME", temp.path()).env("USERPROFILE", temp.path())
        .env("LOCALAPPDATA", temp.path().join("local"))
        .env("APPDATA", temp.path().join("roaming"))
        .env("MEMBRANE_CACHE_ROOT", temp.path().join("cache"))
        .env("MEMBRANE_DATA_ROOT", temp.path().join("data"))
        .env("MEMBRANE_CONFIG_ROOT", temp.path().join("config"))
        .env_remove("WORKSPACE_ROOT").env_remove("CORTEX_DB")
        .output().unwrap();
    assert!(output.status.success(), "{}\n{}",
        String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    let _: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(product.join("state/tools/.cache/memory/cortex-engine.db").is_file());
    assert!(!temp.path().join(".claude/cortex/cortex.db").exists());
    assert!(!product.join("state/tools/.cache/memory/api-token").exists());

    // Exercise real installed stdio with one session & no Hub identity/token.
    use std::io::Write;
    use std::process::Stdio;
    let repo = temp.path().join("workspace/repo");
    fs::create_dir_all(&repo).unwrap();
    let manifest = temp.path().join("workspace/tools/lib/memory/runtime.json");
    fs::create_dir_all(manifest.parent().unwrap()).unwrap();
    write_json(&manifest, &json!({"port":47851,"serviceId":"membrane-local-v1","host":"127.0.0.1"}));
    let registry = temp.path().join("registry.json");
    let mut bindings = serde_json::Map::new();
    bindings.insert(repo.to_string_lossy().into_owned(), json!({
        "repository_id":"repo-fixture","scope_id":"scope-fixture",
        "grant_policy":{"level":"write-trusted"},
        "token_grant":{"generation":1,"issued_at":"2025-01-01T00:00:00Z"}
    }));
    write_json(&registry, &json!({"schema_version":2,"bindings":bindings}));
    // Separate CLI invocations must share canonical logical workspace state.
    for args in [
        vec!["diagnostics", "workspace-open", "--repo", "repo-fixture", "--worktree", "wt-fixture", "--project-root", repo.to_str().unwrap()],
        vec!["diagnostics", "mutation-begin", "--repo", "repo-fixture", "--worktree", "wt-fixture"],
        vec!["diagnostics", "workspace-status", "--repo", "repo-fixture", "--worktree", "wt-fixture"],
        vec!["diagnostics", "workspace-status", "--repo", "repo-fixture", "--worktree", "wt-fixture", "--port", "1"],
    ] {
        let output = Command::new(current.join(filename)).args(&args)
            .current_dir(&repo).env("HOME", temp.path()).env("USERPROFILE", temp.path())
            .env("LOCALAPPDATA", temp.path().join("local")).env("APPDATA", temp.path().join("roaming"))
            .env("MEMBRANE_CACHE_ROOT", temp.path().join("cache"))
            .env("MEMBRANE_DATA_ROOT", temp.path().join("data"))
            .env("MEMBRANE_CONFIG_ROOT", temp.path().join("config"))
            .env("MEMBRANE_PROJECT_REGISTRY", &registry)
            .env_remove("MEMBRANE_PORT").env_remove("MEMBRANE_API_TOKEN").env_remove("MEMBRANE_API_TOKEN_FILE")
            .env_remove("WORKSPACE_ROOT").env_remove("CORTEX_DB").output().unwrap();
        assert!(output.status.success(), "{args:?}: {}\n{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
        let response: Value = serde_json::from_slice(&output.stdout).unwrap();
        if args[1] == "workspace-status" { assert_eq!(response["openMutation"], true); }
    }
    let mut child = Command::new(current.join(filename)).arg("stdio-mcp")
        .current_dir(&repo).env("HOME", temp.path()).env("USERPROFILE", temp.path())
        .env("LOCALAPPDATA", temp.path().join("local")).env("APPDATA", temp.path().join("roaming"))
        .env("MEMBRANE_CACHE_ROOT", temp.path().join("cache"))
        .env("MEMBRANE_DATA_ROOT", temp.path().join("data"))
        .env("MEMBRANE_CONFIG_ROOT", temp.path().join("config"))
        .env("MEMBRANE_PROJECT_REGISTRY", &registry)
        .env_remove("WORKSPACE_ROOT").env_remove("CORTEX_DB")
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let mut input = child.stdin.take().unwrap();
    let requests = [
        ("membrane_scratchpad", json!({"operation":"save","sessionId":"session","taskId":"task","scratchpad":{"note":"Hub off"}})),
        ("membrane_scratchpad", json!({"operation":"load","sessionId":"session","taskId":"task"})),
        ("membrane_ledger", json!({"operation":"status"})),
        ("membrane_feedback", json!({"receiptId":"fixture-receipt","outcome":"used"})),
    ];
    for (id, (name, mut arguments)) in requests.into_iter().enumerate() {
        arguments["repository"] = json!("repo-fixture");
        arguments["caller"] = json!({"root":repo,"repositoryId":"repo-fixture","scopeId":"scope-fixture"});
        writeln!(input, "{}", json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":name,"arguments":arguments}})).unwrap();
    }
    drop(input);
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let responses: Vec<Value> = String::from_utf8(output.stdout).unwrap().lines()
        .map(|line| serde_json::from_str(line).unwrap()).collect();
    assert_eq!(responses.len(), 4);
    for response in &responses {
        assert_eq!(response["result"]["isError"], false, "{response}");
    }
    assert!(responses[1].to_string().contains("Hub off"));
    assert!(!product.join("state/tools/.cache/memory/api-token").exists());
}

#[test]
fn copied_candidate_mines_offline_but_cannot_impersonate_installed_storage_owner() {
    let source_binary = Path::new(env!("CARGO_BIN_EXE_membrane"));
    let temp = tempfile::tempdir().unwrap();
    let candidate = temp.path().join("membrane");
    fs::copy(source_binary, &candidate).unwrap();
    fs::write(temp.path().join("transcript.jsonl"), concat!(
        "{\"type\":\"adapt_event_v1\",\"host\":\"pi\",\"event\":{\"sessionId\":\"pi-session\",\"kind\":\"user_message\",\"role\":\"user\",\"text\":\"always preserve unrelated changes\"}}\n"
    )).unwrap();
    let mined = run_json(
        &candidate,
        temp.path(),
        &[
            "adapt",
            "mine",
            "--host",
            "pi",
            "--scope",
            "workspace",
            "transcript.jsonl",
        ],
    );
    assert_eq!(mined["response"]["api_version"], "adapt.cli.v1");
    for args in [
        vec!["adapt", "status"],
        vec!["adapt", "recall", "anything"],
        vec!["adapt", "--db", "forbidden.db", "recall", "anything"],
    ] {
        let output = Command::new(&candidate)
            .args(args)
            .current_dir(temp.path())
            .env("HOME", temp.path())
            .env("USERPROFILE", temp.path())
            .env("PATH", "/__membrane_no_interpreters__")
            .env_remove("CORTEX_DB")
            .env_remove("WORKSPACE_ROOT")
            .output()
            .unwrap();
        assert!(
            !output.status.success(),
            "uninstalled candidate gained canonical storage authority"
        );
    }
    assert!(!temp.path().join("forbidden.db").exists());
    assert!(!temp.path().join(".cortex").exists());
}

#[test]
fn copied_candidate_analyzes_supplied_context_cost_observations_without_interpreters() {
    let source_binary = Path::new(env!("CARGO_BIN_EXE_membrane"));
    let temp = tempfile::tempdir().unwrap();
    let candidate = temp.path().join("membrane");
    fs::copy(source_binary, &candidate).unwrap();
    write_json(
        &temp.path().join("context-cost.json"),
        &json!({
            "installationId": "qual-machine",
            "analysisTimestamp": "2026-08-26T00:00:00Z",
            "usageObservations": [{
                "observationId": "usage-1",
                "turnId": "turn-1",
                "sessionId": "session-1",
                "host": "coderight",
                "provider": "example-provider",
                "model": "example-model",
                "usage": {
                    "freshInputTokens": 2000,
                    "cacheReadInputTokens": 8000,
                    "cacheWriteInputTokens": 0,
                    "outputTokens": 500
                },
                "measuredPersistentPrefixTokens": 4000
            }],
            "persistentSources": [{
                "sourceId": "repo-instructions",
                "kind": "instruction_file",
                "path": "/repo/AGENTS.md",
                "capturedDigest": "sha256:captured",
                "capturedBytes": 8000,
                "capturedTokenEstimate": 2000,
                "fileState": {"state": "current", "analysis_digest": "sha256:captured"},
                "alwaysOn": true,
                "visibleTurnIds": ["turn-1"],
                "observedUse": {"coverage": "complete", "count": 0}
            }]
        }),
    );

    let report = run_json(
        &candidate,
        temp.path(),
        &["adapt", "context-cost", "--input", "context-cost.json"],
    );
    assert_eq!(report["schemaVersion"], "adapt.context-cost-analysis.v1");
    assert_eq!(report["providerBilledTokens"], 10_500);
    assert_eq!(report["inferredPersistentSourceTokens"], 2_000);
    assert_eq!(report["unattributedPersistentPrefixTokens"], 2_000);
    assert!(report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|finding| { finding["detector"] == "apparently_unused_always_on_context" }));
}
