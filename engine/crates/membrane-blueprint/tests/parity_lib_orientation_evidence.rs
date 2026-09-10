//! Parity tests for `lib_orientation_evidence` (native port of
//! `blueprint/src/lib/orientation-evidence.mjs`), lane LIB4 (r5 closure).

use membrane_blueprint::lib_orientation_evidence::{
    build_orientation_evidence, candidate_set_digest, default_evidence_path,
    write_orientation_evidence_file, ORIENTATION_EVIDENCE_KIND,
};
use serde_json::json;
use std::fs;

fn tempdir(name: &str) -> std::path::PathBuf {
    let mut dir = std::env::temp_dir();
    dir.push(format!(
        "membrane-lib4-orient-{}-{}",
        name,
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create tempdir");
    dir
}

#[test]
fn candidate_set_digest_stable_for_equivalent_sets_and_order_sensitive() {
    let a = json!({
        "schemaVersion": 1,
        "provider": "blueprint-static",
        "freshness": { "revision": "xxh128:1" },
        "candidates": [
            { "id": "symbol:a", "sourceRef": "a.ts:1-2", "sourceHash": "xxh128:aa", "protected": true, "exact": false },
            { "id": "symbol:b", "sourceRef": "b.ts:1-2", "sourceHash": "xxh128:bb", "protected": false, "exact": true },
        ],
        "omissions": [{ "id": "symbol:c", "reason": "over_candidate_ceiling" }],
    });
    let mut b = a.clone();
    let candidates = b["candidates"].as_array().unwrap().clone();
    b["candidates"] = json!([candidates[1].clone(), candidates[0].clone()]);

    assert_eq!(candidate_set_digest(&a), candidate_set_digest(&a.clone()));
    assert_ne!(candidate_set_digest(&a), candidate_set_digest(&b));
}

#[test]
fn build_orientation_evidence_matches_forge_consumable_shape() {
    let receipt = json!({
        "receiptId": "rec-1",
        "sessionId": "s",
        "taskId": "t",
        "repoIdentity": "demo@/tmp/demo",
        "generationId": "xxh128:gen",
        "manifestDigest": "sha256:manifest",
        "candidateSetDigest": "sha256:candidates",
        "overlayRevision": 2,
    });
    let evidence = build_orientation_evidence(&receipt, None).expect("builds");
    assert_eq!(evidence["kind"], json!(ORIENTATION_EVIDENCE_KIND));
    assert_eq!(evidence["locator"], json!("blueprint://receipt/rec-1"));
    assert_eq!(evidence["content_hash"], json!("sha256:candidates"));
    assert_eq!(evidence["invalidation_key"], json!("sha256:manifest:2"));
    assert_eq!(evidence["trust_class"], json!("host_attested"));
    assert_eq!(evidence["supports_or_refutes"], json!("supports"));
}

#[test]
fn build_orientation_evidence_requires_receipt_id() {
    let receipt = json!({"sessionId": "s"});
    let result = build_orientation_evidence(&receipt, None);
    assert!(result.is_err());
}

#[test]
fn build_orientation_evidence_falls_back_to_sha256_of_receipt_id() {
    let receipt = json!({"receiptId": "rec-nohash"});
    let evidence = build_orientation_evidence(&receipt, None).expect("builds");
    let ch = evidence["content_hash"].as_str().unwrap();
    assert!(ch.starts_with("sha256:"));
    assert_eq!(ch.len(), "sha256:".len() + 64);
}

#[test]
fn write_orientation_evidence_file_emits_attestable_json() {
    let dir = tempdir("write");
    let receipt = json!({
        "receiptId": "rec-file",
        "generationId": "xxh128:g",
        "manifestDigest": "sha256:m",
        "candidateSetDigest": "sha256:c",
    });
    let path = default_evidence_path(&dir, "rec-file");
    let (written_path, evidence) =
        write_orientation_evidence_file(&receipt, &path, None).expect("writes");
    assert!(written_path.exists());
    let on_disk: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&written_path).unwrap()).unwrap();
    assert_eq!(on_disk, evidence);
    assert_eq!(on_disk["kind"], json!("blueprint_orientation"));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn default_evidence_path_uses_expected_filename_pattern() {
    let dir = tempdir("path");
    let path = default_evidence_path(&dir, "rec-42");
    assert!(path
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .contains("blueprint-orientation-rec-42.json"));
    let _ = fs::remove_dir_all(&dir);
}
