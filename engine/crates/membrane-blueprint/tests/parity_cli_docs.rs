//! Parity tests for `blueprint docs [--limit N] [--json]`
//! (`blueprint/scripts/cli/commands.mjs:175` case `"docs"` ->
//! `service.documentTruth({...common, limit})`), ported to the native
//! `membrane_blueprint::cli::docs` dispatch. Legacy's `docs` verb and the
//! MCP-facing document-truth path call the exact same
//! `service.documentTruth`; this wrapper forwards to the already-native
//! `Operation::DocumentTruth` dispatch, so these tests exercise the CLI
//! surface (root + limit binding, and the `blueprint_store_missing` failure
//! mode) rather than re-proving document-truth projection semantics.

use membrane_blueprint::cli;
use std::fs;

#[test]
fn docs_reports_blueprint_store_missing_for_a_repo_with_no_published_generation() {
    let dir = tempfile::tempdir().unwrap();
    let err = cli::docs(dir.path().to_string_lossy().to_string(), None, None).unwrap_err();
    assert_eq!(err.code, "blueprint_store_missing");
}

#[test]
fn docs_succeeds_against_a_real_built_repository_and_honors_limit() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("main.rs"), "fn main() {}\n").unwrap();
    let root = dir.path().to_string_lossy().to_string();
    let refreshed = cli::manual_refresh(root.clone(), None);
    assert!(refreshed.is_ok(), "manual_refresh failed: {:?}", refreshed.err());

    let payload = cli::docs(root.clone(), None, None).unwrap();
    assert_eq!(payload["schemaVersion"], 1);
    assert_eq!(payload["kind"], "document-truth-grounding");
    assert!(payload["claims"].is_array());

    let limited = cli::docs(root, Some(0), None).unwrap();
    assert_eq!(limited["claims"].as_array().unwrap().len(), 0);
}
