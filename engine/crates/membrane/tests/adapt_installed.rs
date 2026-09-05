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
fn copied_candidate_mines_offline_but_never_opens_canonical_store_without_daemon() {
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
            "canonical Adapt operation escaped inactive Hub"
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
