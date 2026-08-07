//! MBR-106 integration tests for the runtime receipt module.
//!
//! These tests assert two contracts:
//!   1. The `register → is_known → remove` cycle round-trips every
//!      receipt-owned entry and only those entries.
//!   2. `remove_receipt_owned` only unlinks entries that were
//!      previously registered; unrelated paths the installer may have
//!      observed are never touched.
//!
//! Tests compile but are not executed here; the Book 1 gate runs them.
//! See MEMBRANE-BOOK-MODE-EXECUTION-RULES.md for the deferredCommands
//! list (cargo fmt --check and cargo test --workspace).

use membrane_runtime::paths::{cache_root, config_root, data_root, log_root, Roots};
use membrane_runtime::receipt::{
    clear_receipt_registry, is_receipt_owned, register_receipt_owned, register_receipt_owned_path,
    remove_receipt_owned, snapshot as receipt_snapshot, ReceiptError, ReceiptOwnedFile,
    UninstallReceipt,
};
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

static RECEIPT_TEST_LOCK: Mutex<()> = Mutex::new(());

fn receipt_test_guard() -> MutexGuard<'static, ()> {
    RECEIPT_TEST_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Build a path under the system temp dir so the receipt module can
/// guarantee the path lives outside any of the four stable roots, even
/// on a host where the operator has overridden one or more roots with
/// `MEMBRANE_*` environment variables.
fn scratch_path(label: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!(
        "membrane-receipt-it-{}-{}-{}",
        std::process::id(),
        label,
        // A monotonically increasing suffix so tests that share the
        // process id never collide on the same temp file.
        receipt_snapshot().len()
    ));
    p
}

#[test]
fn register_is_known_and_remove_round_trip() {
    let _guard = receipt_test_guard();
    clear_receipt_registry();

    let path = scratch_path("round-trip");
    // Pre-condition: an unknown path reports false.
    assert!(!is_receipt_owned(&path));

    // Register via the convenience constructor used by the runtime.
    register_receipt_owned_path(&path).expect("scratch path is outside the stable roots");
    assert!(is_receipt_owned(&path));

    // The snapshot lists the entry exactly once.
    let snap = receipt_snapshot();
    let matches: Vec<&ReceiptOwnedFile> = snap.iter().filter(|e| e.path == path).collect();
    assert_eq!(matches.len(), 1, "registered entry must appear in snapshot");
    assert_eq!(matches[0].owner, "membrane-runtime");
    assert!(!matches[0].directory);

    // Remove via a predicate that matches our path; nothing else.
    let removed = remove_receipt_owned(|entry| entry.path == path);
    assert_eq!(removed.len(), 1);
    assert_eq!(removed[0].path, path);

    // Post-condition: removed from the registry, no longer known.
    assert!(!is_receipt_owned(&path));
    assert!(!receipt_snapshot().iter().any(|e| e.path == path));
}

#[test]
fn remove_only_touches_receipt_owned_paths() {
    let _guard = receipt_test_guard();
    clear_receipt_registry();

    let owned = scratch_path("owned");
    let unrelated = scratch_path("unrelated");
    register_receipt_owned_path(&owned).expect("outside stable roots");

    // Pass a predicate that matches BOTH the registered path and an
    // unrelated path the runtime never wrote. The receipt module must
    // still only remove the registered one.
    let removed = remove_receipt_owned(|entry| entry.path == owned || entry.path == unrelated);
    assert_eq!(removed.len(), 1);
    assert_eq!(removed[0].path, owned);

    // The unrelated path was never observed by the registry, so its
    // membership in the predicate has no effect on registry state.
    assert!(!is_receipt_owned(&unrelated));
}

#[test]
fn registering_a_path_inside_a_stable_root_is_rejected() {
    let _guard = receipt_test_guard();
    clear_receipt_registry();

    // Pick whichever root is the most likely to exist on this host.
    // config_root() always returns a real PathBuf because HOME/USERPROFILE
    // is set by every supported CI and dev environment.
    let inside_root = config_root().join("receipt-should-never-track-this.json");
    let err = register_receipt_owned_path(&inside_root).unwrap_err();
    assert!(matches!(err, ReceiptError::InsideStableRoot(_)));
    assert!(!is_receipt_owned(&inside_root));

    // The same rule applies for paths under every other stable root.
    for inside in [
        data_root().join("nested/file"),
        cache_root().join("nested/file"),
        log_root().join("nested/file"),
    ] {
        let err = register_receipt_owned(ReceiptOwnedFile::file(&inside, "test")).unwrap_err();
        assert!(matches!(err, ReceiptError::InsideStableRoot(_)));
    }
}

#[test]
fn registering_the_same_path_twice_keeps_a_single_entry() {
    let _guard = receipt_test_guard();
    clear_receipt_registry();

    let path = scratch_path("dedup");
    register_receipt_owned(ReceiptOwnedFile::file(&path, "first-owner")).unwrap();
    // Re-registering the same path with a different owner is allowed:
    // the latest write wins, the older label is gone. Uninstall uses
    // the latest label when it logs residue by category.
    register_receipt_owned(ReceiptOwnedFile::file(&path, "second-owner")).unwrap();

    let snap = receipt_snapshot();
    let matches: Vec<&ReceiptOwnedFile> = snap.iter().filter(|e| e.path == path).collect();
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].owner, "second-owner");
}

#[test]
fn directory_flag_is_preserved_through_the_cycle() {
    let _guard = receipt_test_guard();
    clear_receipt_registry();

    let dir = scratch_path("scratch-dir");
    register_receipt_owned(ReceiptOwnedFile::dir(&dir, "sandbox-scratch")).unwrap();

    let snap = receipt_snapshot();
    let entry = snap
        .iter()
        .find(|e| e.path == dir)
        .expect("directory entry must be present in snapshot");
    assert!(entry.directory);

    let removed = remove_receipt_owned(|entry| entry.path == dir);
    assert_eq!(removed.len(), 1);
    assert!(removed[0].directory);
    assert_eq!(removed[0].owner, "sandbox-scratch");
}

#[test]
fn uninstall_receipt_capture_is_anchored_to_config_root_and_lists_owned_files() {
    let _guard = receipt_test_guard();
    clear_receipt_registry();

    let a = scratch_path("audit-a");
    let b = scratch_path("audit-b");
    register_receipt_owned_path(&a).unwrap();
    register_receipt_owned_path(&b).unwrap();

    let receipt = UninstallReceipt::capture();
    assert_eq!(receipt.schema_version, 1);
    assert_eq!(receipt.config_root, config_root());
    // Snapshot is in BTreeMap order — i.e. lexicographic by path.
    let mut owned_paths: Vec<PathBuf> =
        receipt.owned_files.iter().map(|e| e.path.clone()).collect();
    owned_paths.sort();
    let mut expected = vec![a.clone(), b.clone()];
    expected.sort();
    assert_eq!(owned_paths, expected);

    // The receipt is JSON-serializable so the installer can persist it
    // next to the uninstall marker.
    let json = serde_json::to_string(&receipt).expect("serialize receipt");
    assert!(json.contains("schemaVersion"));
    assert!(json.contains("configRoot"));
    assert!(json.contains("ownedFiles"));
}

#[test]
fn remove_with_predicate_that_matches_nothing_removes_nothing() {
    let _guard = receipt_test_guard();
    clear_receipt_registry();

    let path = scratch_path("noop");
    register_receipt_owned_path(&path).unwrap();

    let removed = remove_receipt_owned(|_| false);
    assert!(
        removed.is_empty(),
        "predicate that matches nothing must remove nothing"
    );
    assert!(
        is_receipt_owned(&path),
        "no-op remove must leave the registry unchanged"
    );
}

#[test]
fn roots_contains_matches_receipt_rejection_rule() {
    let _guard = receipt_test_guard();
    // This cross-checks `paths::Roots::contains` against the receipt's
    // stable-root rejection rule so a future change to one module
    // cannot silently disagree with the other.
    let roots = Roots::resolve();
    let inside = roots.config.join("nested/anything");
    let outside = scratch_path("outside");

    assert!(roots.contains(&inside));
    assert!(!roots.contains(&outside));

    assert!(matches!(
        register_receipt_owned_path(&inside).unwrap_err(),
        ReceiptError::InsideStableRoot(_)
    ));
    assert!(register_receipt_owned_path(&outside).is_ok());
}
