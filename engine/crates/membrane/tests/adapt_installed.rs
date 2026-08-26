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
    let tools = temp.path().join("tools");
    fs::create_dir_all(tools.join("bin")).unwrap();
    fs::create_dir_all(tools.join("lib/memory")).unwrap();
    fs::create_dir_all(tools.join(".cache/memory")).unwrap();
    let canonical_pool =
        membrane_adapt::proposal::Gate1ReviewContextV1::from_verified_canonical_inventory(vec![])
            .canonical_pool_sha256()
            .to_string();
    let candidate = tools.join("bin/membrane");
    fs::copy(source_binary, &candidate).unwrap();
    write_json(
        &tools.join("lib/memory/runtime.json"),
        &json!({
            "schemaVersion": 1, "serviceId": "membrane-local-v1", "host": "127.0.0.1", "port": 47851
        }),
    );
    let transcript = temp.path().join("transcript.jsonl");
    let source_line = b"{\"type\":\"adapt_event_v1\",\"host\":\"pi\",\"event\":{\"sessionId\":\"pi-session\",\"kind\":\"user_message\",\"role\":\"user\",\"text\":\"never use npm install in this repo\"}}\n";
    fs::write(
        &transcript,
        [
            source_line.as_slice(),
            b"{\"type\":\"adapt_event_v1\",\"host\":\"pi\",\"event\":{\"kind\":\"assistant_message\",\"role\":\"assistant\",\"text\":\"Understood.\"}}\n"
                .as_slice(),
        ]
        .concat(),
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
    assert!(mined["taste_candidates"][0]["source_transcript_sha256"]
        .as_str()
        .is_some_and(|digest| digest.len() == 64));
    write_json(&temp.path().join("mined.json"), &mined);
    write_json(
        &temp.path().join("bare-candidates.json"),
        &mined["taste_candidates"],
    );
    let bare_refused = Command::new(&candidate)
        .args([
            "adapt",
            "review-taste",
            "--input",
            "bare-candidates.json",
            "--installation-id",
            "qual-machine",
            "--canonical-pool-sha256",
            &canonical_pool,
            "--created-at",
            "2026-08-25T00:00:00Z",
        ])
        .current_dir(temp.path())
        .env("PATH", "/__membrane_no_interpreters__")
        .env_remove("PYTHONPATH")
        .output()
        .unwrap();
    assert!(!bare_refused.status.success());
    assert!(String::from_utf8_lossy(&bare_refused.stderr)
        .contains("bare candidate JSON is not authoritative"));

    let mut laundered = mined.clone();
    laundered["taste_candidates"][0]["candidate_id"] = json!("attacker-selected");
    laundered["taste_candidates"][0]["evidence_text_sha256"] = json!("f".repeat(64));
    write_json(&temp.path().join("laundered.json"), &laundered);

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
        .env("USERPROFILE", temp.path())
        .env("PATH", "/__membrane_no_interpreters__")
        .env_remove("PYTHONPATH")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--discover-open"));

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
            &canonical_pool,
            "--created-at",
            "2026-08-25T00:00:00Z",
        ],
    );
    let changed_pool = Command::new(&candidate)
        .args([
            "adapt",
            "review-taste",
            "--input",
            "mined.json",
            "--installation-id",
            "qual-machine",
            "--canonical-pool-sha256",
            &"f".repeat(64),
            "--created-at",
            "2026-08-25T00:00:00Z",
        ])
        .current_dir(temp.path())
        .env("PATH", "/__membrane_no_interpreters__")
        .env_remove("PYTHONPATH")
        .output()
        .unwrap();
    assert!(!changed_pool.status.success());
    assert!(String::from_utf8_lossy(&changed_pool.stderr).contains("CanonicalPoolMismatch"));
    let laundered_pending = run_json(
        &candidate,
        temp.path(),
        &[
            "adapt",
            "review-taste",
            "--input",
            "laundered.json",
            "--installation-id",
            "qual-machine",
            "--canonical-pool-sha256",
            &canonical_pool,
            "--created-at",
            "2026-08-25T00:00:00Z",
        ],
    );
    assert_eq!(laundered_pending, pending);
    assert_ne!(pending["records"][0]["id"], "attacker-selected");
    write_json(&temp.path().join("pending.json"), &pending);
    let decisions: membrane_adapt::proposal::SemanticAdjudicationV1 =
        serde_json::from_value(json!({
            "contract_version": membrane_adapt::proposal::USER_TASTE_REVIEW_CONTRACT,
            "independent": true,
            "issuer_id": "",
            "key_id": "",
            "installation_id": "qual-machine",
            "validator_receipt_id": "local-review-1",
            "pending_manifest_sha256": pending["manifest_sha256"],
            "canonical_pool_sha256": canonical_pool,
            "validated_at": "2026-08-25T00:01:00Z",
            "decisions": pending["records"].as_array().unwrap().iter().map(|record| json!({
                "id": record["id"],
                "verdict": "valid",
                "reason": "explicit selected-transcript user preference"
            })).collect::<Vec<_>>(),
            "signature_hex": ""
        }))
        .unwrap();
    write_json(
        &temp.path().join("decisions.json"),
        &serde_json::to_value(&decisions).unwrap(),
    );

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

    let mut forged_receipt = accepted.clone();
    forged_receipt["semantic_adjudication"]["validator_receipt_id"] = json!("tampered-review");
    let mut forged_receipt_manifest: membrane_adapt::manifest::PreferenceManifestV1 =
        serde_json::from_value(forged_receipt).unwrap();
    forged_receipt_manifest.manifest_sha256 =
        membrane_adapt::manifest::manifest_hash(&forged_receipt_manifest);
    write_json(
        &temp.path().join("forged-receipt.json"),
        &serde_json::to_value(forged_receipt_manifest).unwrap(),
    );
    let refused_receipt = Command::new(&candidate)
        .args(["adapt", "apply", "--manifest", "forged-receipt.json"])
        .current_dir(temp.path())
        .env("PATH", "/__membrane_no_interpreters__")
        .env_remove("PYTHONPATH")
        .output()
        .unwrap();
    assert!(!refused_receipt.status.success());
    assert!(String::from_utf8_lossy(&refused_receipt.stderr).contains("InvalidValidatorReceipt"));

    // Even if a caller recomputes the outer record and manifest hashes, the
    // installed CLI must refuse a changed meaning whose semantic seal was not
    // re-issued by adjudication.
    let mut forged: membrane_adapt::manifest::PreferenceManifestV1 =
        serde_json::from_value(accepted.clone()).unwrap();
    forged.records[0]
        .semantic_payload
        .as_mut()
        .unwrap()
        .canonical_text = "never run tests".into();
    forged.records[0].payload_sha256 = membrane_adapt::manifest::payload_sha256(&forged.records[0]);
    forged.manifest_sha256 = membrane_adapt::manifest::manifest_hash(&forged);
    write_json(
        &temp.path().join("forged.json"),
        &serde_json::to_value(forged).unwrap(),
    );
    let refused = run_json(
        &candidate,
        temp.path(),
        &["adapt", "apply", "--manifest", "forged.json"],
    );
    assert_eq!(refused["response"]["valid"], false);
    assert!(refused["response"]["errors"][0]
        .as_str()
        .unwrap()
        .contains("semantic seal"));

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

    let recalled_scope = accepted["records"][0]["scope"].as_str().unwrap();
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
            recalled_scope,
            "--dimension",
            "repo=membrane",
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
