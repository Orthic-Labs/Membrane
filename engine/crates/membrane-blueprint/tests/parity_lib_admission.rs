//! Parity tests for `lib_admission` (native port of
//! `blueprint/src/lib/admission.mjs`), lane LIB4 (r5 closure).

use membrane_blueprint::lib_admission::{
    claim_boundary_for, decision, paths_from_candidate_set, scopes_from_paths, Admission,
    AdmissionDeps, ClaimBoundaryInput, DecisionInput, RecallInput, ADMISSION_SCHEMA_VERSION,
    DECISION_ACTIONS,
};
use membrane_blueprint::lib_receipt_store::ReceiptStore;
use serde_json::json;
use std::fs;
use std::path::PathBuf;

fn tempdir(name: &str) -> PathBuf {
    let mut dir = std::env::temp_dir();
    dir.push(format!("membrane-lib4-admission-{}-{}", name, std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create tempdir");
    dir
}

fn fresh_deps() -> AdmissionDeps<'static> {
    AdmissionDeps {
        read_generation: Box::new(|_root, _out_dir| {
            Some(json!({
                "manifest": { "generationId": "gen-1", "manifestDigest": "sha256:manifest" },
                "sourceObservation": { "head": "abc" },
            }))
        }),
        create_context_candidate_set: Box::new(|_generation, _options| {
            json!({
                "candidates": [
                    { "id": "c1", "sourceRef": "src/foo.rs:1-10" },
                    { "id": "c2", "sourceRef": "src/bar/baz.rs:1-5" },
                ],
                "omissions": [],
            })
        }),
        graph_status: Box::new(|_root, _out_dir, _options| json!({ "state": "fresh" })),
    }
}

fn missing_graph_deps() -> AdmissionDeps<'static> {
    AdmissionDeps {
        read_generation: Box::new(|_root, _out_dir| None),
        create_context_candidate_set: Box::new(|_generation, _options| json!({})),
        graph_status: Box::new(|_root, _out_dir, _options| json!({ "state": "missing" })),
    }
}

#[test]
fn constants_match_legacy() {
    assert_eq!(ADMISSION_SCHEMA_VERSION, 1);
    assert_eq!(DECISION_ACTIONS, ["allow", "continue", "block", "noop"]);
}

#[test]
fn decision_defaults_and_rejects_invalid_action() {
    let d = decision(DecisionInput::default()).unwrap();
    assert_eq!(d["action"], json!("noop"));
    assert_eq!(d["schemaVersion"], json!(1));
    assert_eq!(d["allowedScopes"], json!([]));

    let err = decision(DecisionInput {
        action: Some("bogus".to_string()),
        ..Default::default()
    });
    assert!(err.is_err());
}

#[test]
fn claim_boundary_for_fresh_state_is_clear() {
    let cb = claim_boundary_for(ClaimBoundaryInput {
        permit_clean: true,
        state: "fresh".to_string(),
        generation_id: Some("g1".to_string()),
        omissions: vec![],
    });
    assert_eq!(cb["status"], json!("clear"));
    assert_eq!(cb["cleanClaimAllowed"], json!(true));
}

#[test]
fn claim_boundary_for_missing_state_is_restricted() {
    let cb = claim_boundary_for(ClaimBoundaryInput {
        permit_clean: false,
        state: "missing".to_string(),
        generation_id: None,
        omissions: vec![json!({"reason": "missing_graph"})],
    });
    assert_eq!(cb["status"], json!("restricted"));
    assert_eq!(cb["cleanClaimAllowed"], json!(false));
    assert_eq!(cb["gaps"], json!(["missing_graph"]));
}

#[test]
fn paths_from_candidate_set_strips_line_range_and_sorts() {
    let cs = json!({
        "candidates": [
            { "sourceRef": "src/b.rs:5-10" },
            { "sourceRef": "src/a.rs:1-2" },
        ]
    });
    let paths = paths_from_candidate_set(&cs);
    assert_eq!(paths, vec!["src/a.rs".to_string(), "src/b.rs".to_string()]);
}

#[test]
fn scopes_from_paths_derives_directories() {
    let paths = vec!["src/a.rs".to_string(), "src/nested/b.rs".to_string(), "top.rs".to_string()];
    let scopes = scopes_from_paths(&paths);
    assert!(scopes.contains(&"src".to_string()));
    assert!(scopes.contains(&"src/nested".to_string()));
    assert!(!scopes.contains(&"".to_string()));
}

#[test]
fn recall_blocks_on_missing_graph() {
    let dir = tempdir("recall-missing");
    let store = ReceiptStore::new(dir.clone()).unwrap();
    let admission = Admission::new(store, None, None, missing_graph_deps());
    let result = admission
        .recall(RecallInput {
            task: Some("investigate".to_string()),
            repo_root: Some(dir.clone()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(result["action"], json!("block"));
    assert_eq!(result["reasonCode"], json!("missing_graph"));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn recall_allows_on_fresh_graph_and_persists_receipt() {
    let dir = tempdir("recall-fresh");
    let store = ReceiptStore::new(dir.clone()).unwrap();
    let admission = Admission::new(store, None, None, fresh_deps());
    let result = admission
        .recall(RecallInput {
            task: Some("investigate foo".to_string()),
            session_id: Some("s1".to_string()),
            task_id: Some("t1".to_string()),
            repo_root: Some(dir.clone()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(result["action"], json!("allow"));
    assert_eq!(result["reasonCode"], json!("recalled"));
    assert!(result["receiptId"].as_str().is_some());
    let scopes = result["allowedScopes"].as_array().unwrap();
    assert!(scopes.iter().any(|s| s == "src/foo.rs"));

    // Recalling again for the same session/task/repo/generation reuses the
    // receipt rather than minting a new one.
    let result2 = admission
        .recall(RecallInput {
            task: Some("investigate foo".to_string()),
            session_id: Some("s1".to_string()),
            task_id: Some("t1".to_string()),
            repo_root: Some(dir.clone()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(result2["action"], json!("continue"));
    assert_eq!(result2["reasonCode"], json!("receipt_reuse"));
    assert_eq!(result2["receiptId"], result["receiptId"]);

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn recall_blocks_on_generation_mismatch() {
    let dir = tempdir("recall-mismatch");
    let store = ReceiptStore::new(dir.clone()).unwrap();
    let admission = Admission::new(store, None, None, fresh_deps());
    let result = admission
        .recall(RecallInput {
            task: Some("x".to_string()),
            repo_root: Some(dir.clone()),
            expected_generation: Some("some-other-gen".to_string()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(result["action"], json!("block"));
    assert_eq!(result["reasonCode"], json!("generation_mismatch"));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn expand_requires_receipt_id() {
    let dir = tempdir("expand-noreceipt");
    let store = ReceiptStore::new(dir.clone()).unwrap();
    let admission = Admission::new(store, None, None, fresh_deps());
    let result = admission
        .expand(None, Some("query"), None, None, None, &[], None, None, None, None, None, None, None)
        .unwrap();
    assert_eq!(result["action"], json!("block"));
    assert_eq!(result["reasonCode"], json!("missing_receipt_id"));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn expand_rejects_absolute_paths() {
    let dir = tempdir("expand-abs");
    let store = ReceiptStore::new(dir.clone()).unwrap();
    let admission = Admission::new(store, None, None, fresh_deps());
    let recall_result = admission
        .recall(RecallInput { task: Some("x".to_string()), repo_root: Some(dir.clone()), ..Default::default() })
        .unwrap();
    let receipt_id = recall_result["receiptId"].as_str().unwrap().to_string();

    #[cfg(windows)]
    let abs_path = r"C:\absolute\path.rs".to_string();
    #[cfg(not(windows))]
    let abs_path = "/absolute/path.rs".to_string();

    let result = admission
        .expand(Some(&receipt_id), None, None, None, None, &[abs_path], Some(&dir), None, None, None, None, None, None)
        .unwrap();
    assert_eq!(result["action"], json!("block"));
    assert_eq!(result["reasonCode"], json!("absolute_path_rejected"));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn expand_adds_paths_and_increments_overlay_revision() {
    let dir = tempdir("expand-ok");
    let store = ReceiptStore::new(dir.clone()).unwrap();
    let admission = Admission::new(store, None, None, fresh_deps());
    let recall_result = admission
        .recall(RecallInput { task: Some("x".to_string()), repo_root: Some(dir.clone()), ..Default::default() })
        .unwrap();
    let receipt_id = recall_result["receiptId"].as_str().unwrap().to_string();

    let result = admission
        .expand(Some(&receipt_id), Some("more context"), None, None, None, &[], Some(&dir), None, None, None, None, None, None)
        .unwrap();
    assert_eq!(result["action"], json!("continue"));
    assert_eq!(result["reasonCode"], json!("expanded"));
    let overlay = result["receipt"]["overlayRevision"].as_f64().unwrap();
    assert_eq!(overlay, 1.0);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn status_lookup_reports_no_receipt() {
    let dir = tempdir("status-none");
    let store = ReceiptStore::new(dir.clone()).unwrap();
    let admission = Admission::new(store, None, None, fresh_deps());
    let result = admission
        .status_lookup(None, Some("s1"), Some("t1"), Some("repo@x"), None, None, &json!({}))
        .unwrap();
    assert_eq!(result["action"], json!("noop"));
    assert_eq!(result["reasonCode"], json!("no_receipt"));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn status_lookup_finds_active_receipt_by_id() {
    let dir = tempdir("status-active");
    let store = ReceiptStore::new(dir.clone()).unwrap();
    let admission = Admission::new(store, None, None, fresh_deps());
    let recall_result = admission
        .recall(RecallInput { task: Some("x".to_string()), repo_root: Some(dir.clone()), ..Default::default() })
        .unwrap();
    let receipt_id = recall_result["receiptId"].as_str().unwrap().to_string();

    let result = admission
        .status_lookup(Some(&receipt_id), None, None, None, Some(&dir), None, &json!({}))
        .unwrap();
    assert_eq!(result["action"], json!("continue"));
    assert_eq!(result["reasonCode"], json!("receipt_active"));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn revoke_requires_receipt_id() {
    let dir = tempdir("revoke-noid");
    let store = ReceiptStore::new(dir.clone()).unwrap();
    let admission = Admission::new(store, None, None, fresh_deps());
    let result = admission.revoke(None, None).unwrap();
    assert_eq!(result["action"], json!("block"));
    assert_eq!(result["reasonCode"], json!("missing_receipt_id"));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn revoke_then_status_reports_revoked() {
    let dir = tempdir("revoke-flow");
    let store = ReceiptStore::new(dir.clone()).unwrap();
    let admission = Admission::new(store, None, None, fresh_deps());
    let recall_result = admission
        .recall(RecallInput { task: Some("x".to_string()), repo_root: Some(dir.clone()), ..Default::default() })
        .unwrap();
    let receipt_id = recall_result["receiptId"].as_str().unwrap().to_string();

    let revoke_result = admission.revoke(Some(&receipt_id), None).unwrap();
    assert_eq!(revoke_result["action"], json!("allow"));
    assert_eq!(revoke_result["reasonCode"], json!("revoked"));

    let revoke_again = admission.revoke(Some(&receipt_id), None).unwrap();
    assert_eq!(revoke_again["action"], json!("noop"));
    assert_eq!(revoke_again["reasonCode"], json!("already_revoked"));

    let status_result = admission
        .status_lookup(Some(&receipt_id), None, None, None, Some(&dir), None, &json!({}))
        .unwrap();
    assert_eq!(status_result["action"], json!("block"));
    assert_eq!(status_result["reasonCode"], json!("receipt_revoked"));

    let _ = fs::remove_dir_all(&dir);
}
