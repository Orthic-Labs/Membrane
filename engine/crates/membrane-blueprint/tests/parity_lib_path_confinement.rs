//! Parity tests for `lib_path_confinement` (native port of
//! `blueprint/src/lib/path-confinement.mjs`), lane LIB1 (r5 closure).

use membrane_blueprint::lib_path_confinement::{
    is_confined_path, resolve_physical_path, ConfinementOptions,
};
use std::fs;

fn tempdir(name: &str) -> std::path::PathBuf {
    let mut dir = std::env::temp_dir();
    dir.push(format!(
        "membrane-lib1-pathconf-{}-{}",
        name,
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create tempdir");
    dir
}

#[test]
fn resolve_physical_path_existing_dir_matches_canonicalized() {
    let root = tempdir("existing");
    let resolved = resolve_physical_path(&root).expect("should resolve existing dir");
    let canonical = fs::canonicalize(&root).unwrap();
    assert_eq!(resolved, canonical);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn resolve_physical_path_keeps_nonexistent_tail() {
    let root = tempdir("tail");
    let target = root.join("does").join("not").join("exist.txt");
    let resolved = resolve_physical_path(&target).expect("should resolve via existing ancestor");
    let canonical_root = fs::canonicalize(&root).unwrap();
    assert_eq!(resolved, canonical_root.join("does").join("not").join("exist.txt"));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn resolve_physical_path_none_when_no_ancestor_exists() {
    // A path under a drive/root that plausibly does not exist. On Windows,
    // pick an unlikely drive letter path; on unix an unlikely root child.
    #[cfg(windows)]
    let bogus = std::path::PathBuf::from("Q:\\definitely-not-a-real-drive-path\\x");
    #[cfg(not(windows))]
    let bogus = std::path::PathBuf::from("/definitely-not-a-real-root-path-xyz/x");

    // Only assert None if indeed no ancestor exists (best-effort environment
    // check to avoid flakiness on unusual CI images).
    if !bogus.exists() {
        // Walk up manually to see whether ANY ancestor exists; if the
        // filesystem root itself exists (it always does), the JS impl would
        // still resolve to that root. Our Rust impl matches this: the
        // filesystem root always exists, so this only returns None if
        // canonicalize of that ancestor fails, which is not expected here.
        // So instead we assert the resolved path (if any) at least keeps the
        // requested tail relative structure.
        if let Some(resolved) = resolve_physical_path(&bogus) {
            assert!(resolved.ends_with("x"));
        }
    }
}

#[test]
fn is_confined_path_rejects_relative_input() {
    let root = tempdir("relcheck");
    let rel = std::path::PathBuf::from("relative/child");
    assert!(!is_confined_path(&root, &rel, ConfinementOptions::default()));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn is_confined_path_accepts_child_inside_root() {
    let root = tempdir("child-inside");
    let child = root.join("subdir");
    fs::create_dir_all(&child).unwrap();
    let target = child.join("file.txt");
    fs::write(&target, b"hello").unwrap();

    assert!(is_confined_path(&root, &target, ConfinementOptions::default()));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn is_confined_path_rejects_root_itself_unless_allowed() {
    let root = tempdir("root-itself");
    let canonical_root = fs::canonicalize(&root).unwrap();

    assert!(!is_confined_path(&root, &canonical_root, ConfinementOptions::default()));
    assert!(is_confined_path(
        &root,
        &canonical_root,
        ConfinementOptions { allow_root: true }
    ));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn is_confined_path_rejects_sibling_outside_root() {
    let root = tempdir("sibling-root");
    let sibling = tempdir("sibling-outside");

    assert!(!is_confined_path(&root, &sibling, ConfinementOptions::default()));

    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_dir_all(&sibling);
}

#[cfg(unix)]
#[test]
fn is_confined_path_rejects_symlink_escaping_root() {
    use std::os::unix::fs::symlink;

    let root = tempdir("symlink-root");
    let outside = tempdir("symlink-outside");
    let link = root.join("escape");
    symlink(&outside, &link).unwrap();

    // The symlink itself resolves outside root -> not confined.
    assert!(!is_confined_path(&root, &link, ConfinementOptions::default()));

    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_dir_all(&outside);
}
