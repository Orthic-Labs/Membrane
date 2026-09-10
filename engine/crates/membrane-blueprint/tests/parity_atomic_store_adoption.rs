//! Parity coverage for `blueprint/src/graph/atomic-store-adoption.mjs`
//! (legacy test: `blueprint/tests/atomic-store-adoption.test.mjs`).
//!
//! No existing equivalent was found elsewhere in the crate — `findings.rs`
//! has its own private `replace_atomic` used only for baseline files, with
//! no validation hook and no platform-conditioned backup strategy — so this
//! port lives in its own module, `src/atomic_adopt.rs`, exercised here
//! against the same two scenarios the legacy test drives.

use std::fs;

use membrane_blueprint::atomic_adopt::{adopt_file_atomically, AtomicAdoptError};

#[test]
fn fresh_store_replaces_prior_file_only_after_validation() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("graph.db");
    let source = dir.path().join("fresh.db");
    fs::write(&target, "old").unwrap();
    fs::write(&source, "fresh").unwrap();
    let outcome = adopt_file_atomically(&source, &target, std::env::consts::OS, |path| {
        assert_eq!(fs::read_to_string(path).unwrap(), "fresh");
        Ok(())
    })
    .unwrap();
    assert!(outcome.replaced);
    assert_eq!(fs::read_to_string(&target).unwrap(), "fresh");
    assert!(!source.exists());
}

#[test]
fn failed_validation_restores_prior_file() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("graph.db");
    let source = dir.path().join("fresh.db");
    fs::write(&target, "old").unwrap();
    fs::write(&source, "broken").unwrap();
    let error = adopt_file_atomically(&source, &target, std::env::consts::OS, |_| Err(AtomicAdoptError { message: "invalid".into() }))
        .unwrap_err();
    assert!(error.message.contains("invalid"));
    assert_eq!(fs::read_to_string(&target).unwrap(), "old");
}
