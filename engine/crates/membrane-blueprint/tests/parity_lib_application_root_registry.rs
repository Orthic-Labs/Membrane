// Parity test for legacy blueprint/src/lib/application/root-registry.mjs,
// ported natively as
// engine/crates/membrane-blueprint/src/lib_application_root_registry.rs.
//
// Scenarios translated from blueprint/tests/root-registry.test.mjs:
//   - "registry enrolls entries with canonical roots"
//   - "resolve by repoId returns the canonical root"
//   - "resolve by exact enrolled root works"
//   - "single enrolled repo resolves without explicit id"
//   - "unregistered root returns root_not_enrolled with canonical enrollment remediation"
//   - "repoId and root pointing at different enrollments raise root_escape"
//   - "disabled entries are never resolved"

use membrane_blueprint::lib_application_root_registry::{AddInput, ResolveInput, RootRegistry};
use std::fs;
use std::path::PathBuf;

fn make_repo(name: &str) -> PathBuf {
    let mut dir = std::env::temp_dir();
    dir.push(format!("blueprint-registry-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join(".git")).unwrap();
    fs::write(dir.join("README.md"), format!("# {name}\n")).unwrap();
    fs::canonicalize(&dir).unwrap()
}

fn cleanup(path: &PathBuf) {
    let _ = fs::remove_dir_all(path);
}

fn as_str(p: &PathBuf) -> String {
    p.to_string_lossy().replace('\\', "/")
}

#[test]
fn registry_enrolls_entries_with_canonical_roots() {
    let root = make_repo("a");
    let registry = RootRegistry::new(vec![AddInput { root: as_str(&root), ..Default::default() }]);
    let listed = registry.list();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].root, fs::canonicalize(&root).unwrap().to_string_lossy().replace('\\', "/"));
    assert!(listed[0].enabled);
    cleanup(&root);
}

#[test]
fn resolve_by_repo_id_returns_the_canonical_root() {
    let root = make_repo("b");
    let registry = RootRegistry::new(vec![AddInput { root: as_str(&root), repo_id: Some("repo-1".into()), ..Default::default() }]);
    let resolved = registry.resolve(&ResolveInput { repo_id: Some("repo-1".into()), repo_root: None }).unwrap();
    assert_eq!(resolved, fs::canonicalize(&root).unwrap().to_string_lossy().replace('\\', "/"));
    cleanup(&root);
}

#[test]
fn resolve_by_exact_enrolled_root_works() {
    let root = make_repo("c");
    let registry = RootRegistry::new(vec![AddInput { root: as_str(&root), repo_id: Some("repo-1".into()), ..Default::default() }]);
    let resolved = registry.resolve(&ResolveInput { repo_id: None, repo_root: Some(as_str(&root)) }).unwrap();
    assert_eq!(resolved, fs::canonicalize(&root).unwrap().to_string_lossy().replace('\\', "/"));
    cleanup(&root);
}

#[test]
fn single_enrolled_repo_resolves_without_explicit_id() {
    let root = make_repo("d");
    let registry = RootRegistry::new(vec![AddInput { root: as_str(&root), ..Default::default() }]);
    let resolved = registry.resolve(&ResolveInput::default()).unwrap();
    assert_eq!(resolved, fs::canonicalize(&root).unwrap().to_string_lossy().replace('\\', "/"));
    cleanup(&root);
}

#[test]
fn unregistered_root_returns_root_not_enrolled_with_canonical_remediation() {
    let root = make_repo("e");
    let other = make_repo("other-e");
    let registry = RootRegistry::new(vec![AddInput { root: as_str(&root), ..Default::default() }]);
    let err = registry.resolve(&ResolveInput { repo_id: None, repo_root: Some(as_str(&other)) }).unwrap_err();
    assert_eq!(err.code, "root_not_enrolled");
    let canonical_other = fs::canonicalize(&other).unwrap().to_string_lossy().replace('\\', "/");
    assert_eq!(err.details.get("normalizedRoot").and_then(|v| v.as_str()).unwrap(), canonical_other);
    let remediation = err.remediation.unwrap();
    assert_eq!(remediation.summary, "Enroll the normalized Blueprint root before querying.");
    cleanup(&root);
    cleanup(&other);
}

#[test]
fn repo_id_and_root_pointing_at_different_enrollments_raise_root_escape() {
    let a = make_repo("f");
    let b = make_repo("g");
    let registry = RootRegistry::new(vec![
        AddInput { root: as_str(&a), repo_id: Some("repo-a".into()), ..Default::default() },
        AddInput { root: as_str(&b), repo_id: Some("repo-b".into()), ..Default::default() },
    ]);
    let err = registry.resolve(&ResolveInput { repo_id: Some("repo-a".into()), repo_root: Some(as_str(&b)) }).unwrap_err();
    assert_eq!(err.code, "root_escape");
    cleanup(&a);
    cleanup(&b);
}

#[test]
fn disabled_entries_are_never_resolved() {
    let root = make_repo("h");
    let registry = RootRegistry::new(vec![AddInput { root: as_str(&root), repo_id: Some("repo-1".into()), enabled: Some(false), ..Default::default() }]);
    let err = registry.resolve(&ResolveInput { repo_id: Some("repo-1".into()), repo_root: None }).unwrap_err();
    assert_eq!(err.code, "root_not_enrolled");
    cleanup(&root);
}
