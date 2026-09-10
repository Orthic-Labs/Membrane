// Parity test for src/git_source_observation.rs against the legacy
// blueprint/src/graph/git-source-observation.mjs, using a real temporary git
// repository the way blueprint/tests/freshness-receipt.test.mjs's
// `withGitRepo` helper does (git init, configure identity, commit, then
// mutate the worktree and observe).
//
// Note (documented assumption): the legacy module hashes the raw porcelain
// status bytes with `hash-wasm`'s xxh3-128; this port uses the
// `xxhash-rust` xxh3_128 implementation already used elsewhere in this
// crate (see src/freshness.rs, src/identity.rs, src/phase2.rs) for the same
// algorithm family. Byte-for-byte cross-runtime digest equality with the JS
// hash-wasm output was not independently verified in this lane (no Node
// runtime harness was wired into this Rust test), so this parity test
// verifies documented *behavior* (head format, dirty transitions, digest
// stability/sensitivity) rather than asserting a literal JS-vs-Rust digest
// match. This is flagged as a blocker in the lane receipt.

use std::fs;
use std::process::Command;

use membrane_blueprint::git_source_observation::{git_base_commit, git_source_observation};
use tempfile::tempdir;

fn git(root: &std::path::Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .status()
        .expect("git invocation failed");
    assert!(status.success(), "git {:?} failed", args);
}

fn init_repo() -> tempfile::TempDir {
    let dir = tempdir().expect("tempdir");
    git(dir.path(), &["init", "--quiet"]);
    git(dir.path(), &["config", "user.email", "test@example.invalid"]);
    git(dir.path(), &["config", "user.name", "Parity Test"]);
    dir
}

#[test]
fn git_base_commit_is_none_before_first_commit() {
    let dir = init_repo();
    let root = dir.path().to_str().unwrap();
    assert_eq!(git_base_commit(root), None);
    assert_eq!(git_source_observation(root), None);
}

#[test]
fn git_base_commit_is_lowercase_hex_after_commit() {
    let dir = init_repo();
    let root = dir.path().to_str().unwrap();
    fs::write(dir.path().join("a.txt"), "hello\n").unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "--quiet", "-m", "init"]);

    let head = git_base_commit(root).expect("head commit");
    assert!(head.len() >= 40 && head.len() <= 64);
    assert!(head.chars().all(|c| c.is_ascii_hexdigit()));
    assert_eq!(head, head.to_lowercase());
}

#[test]
fn clean_worktree_is_not_dirty_and_digest_is_stable() {
    let dir = init_repo();
    let root = dir.path().to_str().unwrap();
    fs::write(dir.path().join("a.txt"), "hello\n").unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "--quiet", "-m", "init"]);

    let obs1 = git_source_observation(root).expect("observation");
    let obs2 = git_source_observation(root).expect("observation");
    assert!(!obs1.dirty);
    assert_eq!(obs1, obs2, "identical worktree state must produce identical observations");
}

#[test]
fn untracked_file_marks_worktree_dirty_with_a_different_digest() {
    let dir = init_repo();
    let root = dir.path().to_str().unwrap();
    fs::write(dir.path().join("a.txt"), "hello\n").unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "--quiet", "-m", "init"]);

    let clean = git_source_observation(root).expect("observation");
    assert!(!clean.dirty);

    fs::write(dir.path().join("untracked.txt"), "new file\n").unwrap();
    let dirty = git_source_observation(root).expect("observation");
    assert!(dirty.dirty);
    assert_eq!(dirty.head, clean.head, "head must not move for an uncommitted change");
    assert_ne!(
        dirty.status_digest, clean.status_digest,
        "status digest must change when the porcelain status bytes change"
    );
}

#[test]
fn nonexistent_root_yields_none() {
    let bogus = "Z:\\this\\path\\does\\not\\exist\\at\\all";
    assert_eq!(git_base_commit(bogus), None);
    assert_eq!(git_source_observation(bogus), None);
}

/// Cross-runtime known-answer: legacy `hash-wasm` createXXHash128 of "hello"
/// (computed with node in blueprint/) must equal xxhash-rust xxh3_128 formatted
/// as the native module formats status digests.
#[test]
fn xxh3_128_digest_matches_legacy_hash_wasm_known_answer() {
    let digest = format!("{:032x}", xxhash_rust::xxh3::xxh3_128(b"hello"));
    assert_eq!(digest, "b5e9c1ad071b3e7fc779cfaa5e523818");
}
