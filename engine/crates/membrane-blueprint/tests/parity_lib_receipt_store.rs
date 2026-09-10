//! Parity tests for `lib_receipt_store` (native port of
//! `blueprint/src/lib/receipt-store.mjs`), lane LIB4 (r5 closure).

use membrane_blueprint::lib_receipt_store::{
    build_orientation_receipt, check_scope_grant, default_receipt_store_dir, issue_scope_grant,
    receipt_lookup_key, CheckScopeGrantInput, IssueScopeGrantInput, OrientationReceiptFields,
    ReceiptLookupKeyInput, ReceiptStore,
};
use serde_json::json;
use std::fs;

fn tempdir(name: &str) -> std::path::PathBuf {
    let mut dir = std::env::temp_dir();
    dir.push(format!(
        "membrane-lib4-receipts-{}-{}",
        name,
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create tempdir");
    dir
}

#[test]
fn receipt_lookup_key_joins_fields_with_unit_separator() {
    let key = receipt_lookup_key(ReceiptLookupKeyInput {
        session_id: Some("s"),
        task_id: Some("t"),
        repo_identity: Some("r"),
        generation_id: Some("g"),
    });
    assert_eq!(key, "s\u{001f}t\u{001f}r\u{001f}g");
}

#[test]
fn default_receipt_store_dir_falls_back_to_home_dot_agent_receipts() {
    std::env::remove_var("BLUEPRINT_RECEIPT_STORE");
    let home = std::path::PathBuf::from("/home/testuser");
    let dir = default_receipt_store_dir(&home);
    assert_eq!(dir, home.join(".agent").join("receipts"));
}

#[test]
fn build_orientation_receipt_fills_defaults() {
    let receipt = build_orientation_receipt(OrientationReceiptFields {
        session_id: Some("s1".to_string()),
        task_id: Some("t1".to_string()),
        repo_identity: Some("repo@x".to_string()),
        ..Default::default()
    });
    assert_eq!(receipt["schemaVersion"], json!(1));
    assert_eq!(receipt["kind"], json!("blueprint_orientation_receipt"));
    assert_eq!(receipt["status"], json!("active"));
    assert_eq!(receipt["sessionId"], json!("s1"));
    assert_eq!(receipt["taskId"], json!("t1"));
    assert_eq!(
        receipt["allowedOperations"],
        json!(["read", "search", "test", "edit"])
    );
    assert_eq!(receipt["overlayRevision"], json!(0.0));
    assert!(receipt["receiptId"].as_str().unwrap().len() > 0);
    assert_eq!(receipt["issuedAt"], receipt["updatedAt"]);
}

#[test]
fn receipt_store_put_get_find_active_roundtrip() {
    let dir = tempdir("store");
    let store = ReceiptStore::new(dir.clone()).expect("create store");
    let receipt = build_orientation_receipt(OrientationReceiptFields {
        session_id: Some("s".to_string()),
        task_id: Some("t".to_string()),
        repo_identity: Some("repo".to_string()),
        generation_id: Some(json!("g1")),
        ..Default::default()
    });
    let receipt_id = receipt["receiptId"].as_str().unwrap().to_string();
    store.put(receipt.clone()).expect("put");

    let fetched = store.get(&receipt_id).expect("get");
    assert_eq!(fetched["receiptId"], receipt["receiptId"]);

    let active = store
        .find_active(ReceiptLookupKeyInput {
            session_id: Some("s"),
            task_id: Some("t"),
            repo_identity: Some("repo"),
            generation_id: Some("g1"),
        })
        .expect("find active");
    assert_eq!(active["receiptId"], receipt["receiptId"]);

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn receipt_store_revoke_excludes_from_find_active() {
    let dir = tempdir("revoke");
    let store = ReceiptStore::new(dir.clone()).expect("create store");
    let receipt = build_orientation_receipt(OrientationReceiptFields {
        session_id: Some("s".to_string()),
        task_id: Some("t".to_string()),
        repo_identity: Some("repo".to_string()),
        ..Default::default()
    });
    let receipt_id = receipt["receiptId"].as_str().unwrap().to_string();
    store.put(receipt).expect("put");
    store.revoke(&receipt_id, "explicit_revoke", None).expect("revoke");

    let active = store.find_active(ReceiptLookupKeyInput {
        session_id: Some("s"),
        task_id: Some("t"),
        repo_identity: Some("repo"),
        generation_id: None,
    });
    assert!(active.is_none());

    let listed_default = store.list(false);
    assert!(listed_default.is_empty());
    let listed_with_revoked = store.list(true);
    assert_eq!(listed_with_revoked.len(), 1);
    assert_eq!(listed_with_revoked[0]["status"], json!("revoked"));

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn receipt_store_list_sorted_by_issued_at() {
    let dir = tempdir("list-sort");
    let store = ReceiptStore::new(dir.clone()).expect("create store");
    let r1 = build_orientation_receipt(OrientationReceiptFields {
        issued_at: Some("2024-01-02T00:00:00.000Z".to_string()),
        ..Default::default()
    });
    let r2 = build_orientation_receipt(OrientationReceiptFields {
        issued_at: Some("2024-01-01T00:00:00.000Z".to_string()),
        ..Default::default()
    });
    store.put(r1).unwrap();
    store.put(r2).unwrap();
    let listed = store.list(false);
    assert_eq!(listed.len(), 2);
    assert_eq!(listed[0]["issuedAt"], json!("2024-01-01T00:00:00.000Z"));
    assert_eq!(listed[1]["issuedAt"], json!("2024-01-02T00:00:00.000Z"));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn receipt_store_clear_removes_all_files() {
    let dir = tempdir("clear");
    let store = ReceiptStore::new(dir.clone()).expect("create store");
    let r = build_orientation_receipt(OrientationReceiptFields::default());
    store.put(r).unwrap();
    assert_eq!(store.list(true).len(), 1);
    store.clear().unwrap();
    assert_eq!(store.list(true).len(), 0);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn issue_and_check_scope_grant_round_trip() {
    let repo_root = tempdir("grant-repo");
    let grant = issue_scope_grant(IssueScopeGrantInput {
        repo_root: repo_root.clone(),
        task_id: "task-1".to_string(),
        paths: vec!["src/**".to_string(), "docs/readme.md".to_string()],
        ttl_minutes: 60.0,
        ..Default::default()
    })
    .expect("issue grant");
    assert!(grant.path.exists());
    assert_eq!(grant.grant["taskId"], json!("task-1"));

    let check = check_scope_grant(CheckScopeGrantInput {
        repo_root: &repo_root,
        generation_id: None,
        task_id: "task-1",
        path: "src/lib/foo.rs",
        out_dir: ".agent",
        now_ms: grant.grant["issuedMs"].as_i64().unwrap() + 1000,
    });
    assert!(check.allowed, "expected grant to match path under src/**");
    assert_eq!(check.reason, "grant_match");

    let miss = check_scope_grant(CheckScopeGrantInput {
        repo_root: &repo_root,
        generation_id: None,
        task_id: "task-1",
        path: "unrelated/file.rs",
        out_dir: ".agent",
        now_ms: grant.grant["issuedMs"].as_i64().unwrap() + 1000,
    });
    assert!(!miss.allowed);
    assert_eq!(miss.reason, "grant_miss");

    let _ = fs::remove_dir_all(&repo_root);
}

#[test]
fn check_scope_grant_rejects_after_ttl_expiry() {
    let repo_root = tempdir("grant-expiry");
    let grant = issue_scope_grant(IssueScopeGrantInput {
        repo_root: repo_root.clone(),
        task_id: "task-2".to_string(),
        paths: vec!["**".to_string()],
        ttl_minutes: 1.0,
        ..Default::default()
    })
    .expect("issue grant");
    let issued_ms = grant.grant["issuedMs"].as_i64().unwrap();
    let ttl_ms = grant.grant["ttlMs"].as_i64().unwrap();

    let expired = check_scope_grant(CheckScopeGrantInput {
        repo_root: &repo_root,
        generation_id: None,
        task_id: "task-2",
        path: "any/file.rs",
        out_dir: ".agent",
        now_ms: issued_ms + ttl_ms + 1,
    });
    assert!(!expired.allowed);
    assert_eq!(expired.reason, "grant_miss");

    let _ = fs::remove_dir_all(&repo_root);
}

#[test]
fn check_scope_grant_missing_key_reports_typed_reason() {
    let repo_root = tempdir("grant-nokey");
    let check = check_scope_grant(CheckScopeGrantInput {
        repo_root: &repo_root,
        generation_id: None,
        task_id: "whatever",
        path: "x",
        out_dir: ".agent",
        now_ms: 0,
    });
    assert!(!check.allowed);
    assert_eq!(check.reason, "grant_key_missing");
    let _ = fs::remove_dir_all(&repo_root);
}

#[test]
fn issue_scope_grant_requires_task_id_and_paths() {
    let repo_root = tempdir("grant-invalid");
    let missing_task = issue_scope_grant(IssueScopeGrantInput {
        repo_root: repo_root.clone(),
        task_id: "  ".to_string(),
        paths: vec!["a".to_string()],
        ..Default::default()
    });
    assert!(missing_task.is_err());

    let missing_paths = issue_scope_grant(IssueScopeGrantInput {
        repo_root: repo_root.clone(),
        task_id: "t".to_string(),
        paths: vec![],
        ..Default::default()
    });
    assert!(missing_paths.is_err());

    let _ = fs::remove_dir_all(&repo_root);
}
