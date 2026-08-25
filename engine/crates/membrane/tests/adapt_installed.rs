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
fn copied_candidate_runs_full_adapt_flow_without_interpreters_or_checkout() {
    let source_binary = Path::new(env!("CARGO_BIN_EXE_membrane"));
    let temp = tempfile::tempdir().unwrap();
    assert!(!temp.path().starts_with(env!("CARGO_MANIFEST_DIR")));
    let candidate = temp.path().join("membrane");
    fs::copy(source_binary, &candidate).unwrap();

    let transcript = temp.path().join("transcript.jsonl");
    fs::write(
        &transcript,
        concat!(
            "{\"type\":\"adapt_event_v1\",\"host\":\"pi\",\"event\":{\"kind\":\"user_message\",\"role\":\"user\",\"text\":\"never use npm install in this repo\"}}\n",
            "{\"type\":\"adapt_event_v1\",\"host\":\"pi\",\"event\":{\"kind\":\"assistant_message\",\"role\":\"assistant\",\"text\":\"Understood.\"}}\n"
        ),
    )
    .unwrap();

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
    assert_eq!(
        mined.pointer("/response/api_version"),
        Some(&json!("adapt.cli.v1"))
    );
    assert_eq!(mined["taste_candidates"].as_array().unwrap().len(), 1);
    write_json(&temp.path().join("mined.json"), &mined);

    let pi_session = temp.path().join(".pi/agent/sessions/repo/session.jsonl");
    fs::create_dir_all(pi_session.parent().unwrap()).unwrap();
    fs::write(
        &pi_session,
        concat!(
            "{\"type\":\"session\",\"id\":\"pi-discovered\",\"cwd\":\"/repo\"}\n",
            "{\"type\":\"message\",\"message\":{\"role\":\"user\",\"content\":\"always preserve unrelated changes\"}}\n"
        ),
    )
    .unwrap();
    let output = Command::new(&candidate)
        .args(["adapt", "mine", "--discover-open"])
        .current_dir(temp.path())
        .env("HOME", temp.path())
        .env("PATH", "/__membrane_no_interpreters__")
        .env_remove("PYTHONPATH")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let discovered: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(discovered["taste_candidates"].as_array().unwrap().len(), 1);

    let reviewed = run_json(
        &candidate,
        temp.path(),
        &["adapt", "review", "--input", "mined.json"],
    );
    assert_eq!(reviewed["api_version"], "adapt.cli.v1");

    let pending = run_json(
        &candidate,
        temp.path(),
        &[
            "adapt",
            "review-taste",
            "--input",
            "mined.json",
            "--installation-id",
            "qual-machine",
            "--canonical-pool-sha256",
            "pool-v1",
            "--created-at",
            "2026-08-25T00:00:00Z",
        ],
    );
    write_json(&temp.path().join("pending.json"), &pending);
    let decisions = json!({
        "independent": true,
        "validator_receipt_id": "qual-validator",
        "validator_receipt_sha256": "b".repeat(64),
        "canonical_pool_sha256": "pool-v1",
        "decisions": pending["records"].as_array().unwrap().iter().map(|record| json!({
            "id": record["id"],
            "verdict": "valid",
            "reason": "explicit authenticated user preference"
        })).collect::<Vec<_>>()
    });
    write_json(&temp.path().join("decisions.json"), &decisions);

    let accepted = run_json(
        &candidate,
        temp.path(),
        &[
            "adapt",
            "adjudicate-taste",
            "--manifest",
            "pending.json",
            "--decisions",
            "decisions.json",
            "--validated-at",
            "2026-08-25T00:01:00Z",
        ],
    );
    write_json(&temp.path().join("accepted.json"), &accepted);

    let applied = run_json(
        &candidate,
        temp.path(),
        &[
            "adapt",
            "--db",
            "cortex.db",
            "apply",
            "--manifest",
            "accepted.json",
        ],
    );
    assert_eq!(
        applied["response"]["accepted_record_ids"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(applied["cortex_receipt"]["complete"], true);
    assert_eq!(applied["cortex_receipt"]["inserted"], 1);

    let recalled = run_json(
        &candidate,
        temp.path(),
        &[
            "adapt",
            "--db",
            "cortex.db",
            "recall",
            "npm",
            "--scope",
            "workspace",
        ],
    );
    assert_eq!(recalled["records"].as_array().unwrap().len(), 1);
    assert_eq!(
        recalled["records"][0]["record"]["lifecycle_state"],
        "active"
    );
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
