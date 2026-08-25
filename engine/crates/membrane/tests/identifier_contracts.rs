use membrane_runtime::{CheckpointSourceRefV1, CheckpointV1};
use sha2::Digest;
use std::process::{Command, Output};

fn checkpoint_with_source(source_ref: &str, expected_content_hash: &str) -> CheckpointV1 {
    CheckpointV1 {
        checkpoint_id: "checkpoint/identifier-contract/100".into(),
        installation_id: "550e8400-e29b-41d4-a716-446655440000".into(),
        client: "codex".into(),
        session_id: "identifier-contract".into(),
        repository_id: "repo-1".into(),
        worktree_rev: "rev-1".into(),
        scope_id: "repo-1".into(),
        summary: "freeze identifier compatibility".into(),
        goal_snapshot: None,
        task_snapshot: None,
        created_at_ms: 100,
        expires_at_ms: 200,
        source_refs: vec![CheckpointSourceRefV1 {
            source_ref: source_ref.into(),
            expected_content_hash: expected_content_hash.into(),
            anchor_id: "sec:ledger:1".into(),
            label: "ledger".into(),
        }],
    }
}

fn source_status(root: &std::path::Path, source_ref: &str, expected_hash: &str) -> String {
    membrane_runtime::checkpoint::resolve_source_refs(
        &checkpoint_with_source(source_ref, expected_hash),
        root,
    )[0]
    .status
    .clone()
}

fn run_restore(spill: &std::path::Path, anchor: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_membrane"))
        .args([
            "cli",
            "push",
            "restore",
            anchor,
            "--spill-dir",
            spill.to_str().unwrap(),
        ])
        .output()
        .unwrap()
}

fn terminal_error(output: Output) -> String {
    let stderr = String::from_utf8(output.stderr).unwrap();
    let line = stderr.lines().last().unwrap_or_default();
    line.strip_prefix("membrane: ").unwrap_or(line).into()
}

#[test]
fn worktree_document_identifier_compatibility_corpus() {
    let root = tempfile::tempdir().unwrap();
    let content = "# Ledger\nbody";
    std::fs::create_dir_all(root.path().join("docs")).unwrap();
    std::fs::write(root.path().join("docs/ledger.md"), content).unwrap();
    std::fs::write(root.path().join("docs/guía.md"), content).unwrap();
    let expected_hash = membrane_runtime::ledger::outline::build_outline(
        "doc://repo/worktree/docs/ledger.md",
        content,
        "comrak-0.54.0",
    )
    .content_hash;

    for source_ref in [
        "doc://repo/worktree/docs/ledger.md",
        "doc://repo/worktree/docs/guía.md",
    ] {
        assert_eq!(source_status(root.path(), source_ref, &expected_hash), "ok");
    }
    for source_ref in [
        "docs/ledger.md",
        "Doc://repo/worktree/docs/ledger.md",
        "doc://other/worktree/docs/ledger.md",
        "doc://repo/other/docs/ledger.md",
        "doc://repo/worktree/../ledger.md",
    ] {
        assert_eq!(
            source_status(root.path(), source_ref, &expected_hash),
            "deny"
        );
    }
}

#[test]
fn anchor_identifier_compatibility_corpus() {
    let root = tempfile::tempdir().unwrap();
    let spill = root.path().join("spill");
    std::fs::create_dir(&spill).unwrap();
    let content = "exact anchor content\n";
    let digest = hex::encode(sha2::Sha256::digest(content.as_bytes()));
    std::fs::write(spill.join(format!("{digest}.log")), content).unwrap();

    let output = run_restore(&spill, &format!("mr://anchor/{digest}"));
    assert!(output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap(), content);

    for (anchor, error) in [
        (
            format!("MR://anchor/{digest}"),
            "invalid anchor: anchor reference must start with mr://anchor/",
        ),
        (
            format!("mr://other/{digest}"),
            "invalid anchor: anchor reference names another namespace",
        ),
        (
            "mr://anchor/abc".into(),
            "invalid anchor: anchor digest must be 64 lowercase hexadecimal characters",
        ),
        (
            format!("mr://anchor/{}", "g".repeat(64)),
            "invalid anchor: anchor digest must be 64 lowercase hexadecimal characters",
        ),
    ] {
        let output = run_restore(&spill, &anchor);
        assert!(!output.status.success());
        assert_eq!(terminal_error(output), error);
    }

    let missing = run_restore(&spill, &format!("mr://anchor/{}", "0".repeat(64)));
    assert!(!missing.status.success());
    assert_eq!(terminal_error(missing), "anchor not found");
}

#[test]
fn worktree_document_rejections_are_typed_before_io() {
    use membrane_runtime::ledger::identifier::{WorktreeDocRef, WorktreeDocRefError};

    for (source_ref, expected) in [
        ("doc://repo/worktree/", WorktreeDocRefError::EmptyPath),
        (
            "doc://repo/worktree/docs//guide.md",
            WorktreeDocRefError::AliasCollision,
        ),
        (
            "doc://repo/worktree/docs/%2e%2e/guide.md",
            WorktreeDocRefError::AliasCollision,
        ),
        (
            "doc://repo/worktree/C:/repo/guide.md",
            WorktreeDocRefError::WindowsPath,
        ),
        (
            "doc://repo/worktree/docs\\guide.md",
            WorktreeDocRefError::WindowsPath,
        ),
        (
            "doc://repo/worktree/docs/∕guide.md",
            WorktreeDocRefError::UnicodeAmbiguity,
        ),
    ] {
        assert_eq!(WorktreeDocRef::parse(source_ref).unwrap_err(), expected);
    }

    assert_eq!(
        WorktreeDocRef::parse("doc://other/worktree/docs/guide.md").unwrap_err(),
        WorktreeDocRefError::CrossRepository
    );
    assert_eq!(
        WorktreeDocRef::parse("Doc://repo/worktree/docs/guide.md").unwrap_err(),
        WorktreeDocRefError::PrefixMismatch
    );
}

#[test]
fn cli_anchor_rejection_precedes_anchor_store_access() {
    let root = tempfile::tempdir().unwrap();
    let unavailable_spill = root.path().join("missing-spill");
    let invalid = run_restore(
        &unavailable_spill,
        &format!("MR://anchor/{}", "0".repeat(64)),
    );
    assert!(!invalid.status.success());
    assert_eq!(
        terminal_error(invalid),
        "invalid anchor: anchor reference must start with mr://anchor/"
    );

    let valid = run_restore(
        &unavailable_spill,
        &format!("mr://anchor/{}", "0".repeat(64)),
    );
    assert!(!valid.status.success());
    assert!(terminal_error(valid).starts_with("anchor store unavailable:"));
}

#[test]
fn anchor_rejections_are_typed_before_io() {
    use membrane_runtime::ledger::identifier::{AnchorRef, AnchorRefError};

    assert_eq!(
        AnchorRef::parse(&format!("mr://anchor/{}", "A".repeat(64))).unwrap_err(),
        AnchorRefError::AliasCollision
    );
    assert_eq!(
        AnchorRef::parse(&format!("mr://anchor/{}", "g".repeat(64))).unwrap_err(),
        AnchorRefError::InvalidDigest
    );
    assert_eq!(
        AnchorRef::parse(&format!("mr://other/{}", "0".repeat(64))).unwrap_err(),
        AnchorRefError::CrossNamespace
    );
    assert_eq!(
        AnchorRef::parse(&format!("MR://anchor/{}", "0".repeat(64))).unwrap_err(),
        AnchorRefError::PrefixMismatch
    );
}
