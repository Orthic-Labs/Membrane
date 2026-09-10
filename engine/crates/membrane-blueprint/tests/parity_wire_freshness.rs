// Lane WIRE2 (NCL-02): proves the native production build path
// (`engine::build_and_publish`) persists source observation using the
// canonical `git_source_observation` port (ported from
// `blueprint/src/graph/git-source-observation.mjs`), not a second,
// independently drifted git observer. Before this lane's fix,
// `engine.rs` called `lib_application_snapshots::current_git_identity`,
// which excludes a different porcelain path set and never computes
// `statusDigest` -- the exact split the legacy module's header comment
// warns produces false `changed_since_generation` drift.
//
// This is a real git fixture repo (`git init` + commit), not a mock: the
// test shells out to git itself so the assertion is against ground truth,
// then checks the persisted generation's `sourceObservation` against
// `membrane_blueprint::git_source_observation::git_source_observation`
// called independently on the same fixture root.

use membrane_blueprint::{
    git_source_observation, native_blueprint_operation, BlueprintOperation, BlueprintRequest,
    Bounds, CancellationToken, Operation,
};
use membrane_blueprint::freshness_observation::{
    changed_paths_since_generation, observe_current_vcs_state, observe_repository_freshness,
};
use std::fs;
use std::process::Command;
use tempfile::tempdir;

fn git(root: &std::path::Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(root)
        .status()
        .expect("git available for fixture setup");
    assert!(status.success(), "git {:?} failed in fixture", args);
}

fn init_fixture_repo(root: &std::path::Path) {
    git(root, &["init", "-q"]);
    git(root, &["config", "user.email", "wire2@example.test"]);
    git(root, &["config", "user.name", "wire2"]);
    fs::write(root.join("main.rs"), "fn entry() {}\n").unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "initial"]);
}

fn request(id: &str, method: Operation, root: &std::path::Path) -> BlueprintRequest {
    let mut request = BlueprintRequest::new(id, method, root.to_string_lossy());
    request.deadline_ms = if method.is_build() { 120_000 } else { 30_000 };
    request
}

fn execute(
    operation: &dyn BlueprintOperation,
    request: &BlueprintRequest,
) -> Result<serde_json::Value, String> {
    let mut context = request.validate(Bounds::one_shot()).map_err(|error| error.to_string())?;
    context.cancellation = CancellationToken::new();
    operation.execute(request, &context).map_err(|error| error.to_string())
}

#[test]
fn build_persists_canonical_git_source_observation_on_clean_tree() {
    let root = tempdir().unwrap();
    init_fixture_repo(root.path());

    let expected = git_source_observation::git_source_observation(&root.path().to_string_lossy())
        .expect("fixture is a real git repo with a commit");
    assert!(!expected.dirty, "fixture tree is freshly committed and must be clean");

    let operation = native_blueprint_operation();
    execute(&*operation, &request("build", Operation::Build, root.path())).unwrap();

    let status = execute(&*operation, &request("status", Operation::Status, root.path())).unwrap();
    // status() does not itself round-trip sourceObservation, so read it back
    // through a query response, which does (see engine::load_current_with_observation).
    let query = request("query", Operation::Search, root.path());
    let result = execute(&*operation, &query).unwrap();
    let observation = &result["sourceObservation"];

    assert_eq!(observation["head"], expected.head, "persisted head must match the canonical observer, not a drifted one");
    assert_eq!(observation["dirty"], expected.dirty);
    assert_eq!(
        observation["statusDigest"], expected.status_digest,
        "statusDigest must be the canonical xxh3-128 porcelain digest, proving build wired \
         git_source_observation rather than a second ad-hoc git call"
    );
    assert_eq!(status["state"], "fresh");
}

#[test]
fn build_persists_canonical_git_source_observation_on_dirty_tree() {
    let root = tempdir().unwrap();
    init_fixture_repo(root.path());
    // Dirty the tree after the commit the generation will be built from.
    fs::write(root.path().join("main.rs"), "fn entry() { /* changed */ }\n").unwrap();

    let expected = git_source_observation::git_source_observation(&root.path().to_string_lossy())
        .expect("fixture is a real git repo with a commit");
    assert!(expected.dirty, "fixture tree was mutated after commit and must be dirty");

    let operation = native_blueprint_operation();
    execute(&*operation, &request("build", Operation::Build, root.path())).unwrap();

    let query = request("query", Operation::Search, root.path());
    let result = execute(&*operation, &query).unwrap();
    let observation = &result["sourceObservation"];

    assert_eq!(observation["head"], expected.head);
    assert_eq!(observation["dirty"], true);
    assert_eq!(observation["statusDigest"], expected.status_digest);
}

#[test]
fn freshness_observation_supplies_receipt_current_state_and_changed_paths() {
    let root = tempdir().unwrap();
    init_fixture_repo(root.path());
    let head = git_source_observation::git_base_commit(&root.path().to_string_lossy()).unwrap();

    let clean = observe_repository_freshness(root.path(), Some(&head));
    assert!(clean.available);
    assert!(clean.stable);
    assert_eq!(clean.commit_distance, Some(0));
    assert!(clean.entries.is_empty());
    assert!(clean.stage_elapsed_ms.contains_key("git_status"));

    fs::write(root.path().join("main.rs"), "fn entry() { /* changed */ }\n").unwrap();
    let current = observe_current_vcs_state(root.path());
    assert!(current.available);
    assert_eq!(current.vcs_revision.as_deref(), Some(head.as_str()));
    assert_eq!(current.dirty, Some(true));

    let dirty = observe_repository_freshness(root.path(), Some(&head));
    assert!(dirty.available);
    assert!(dirty.stable);
    assert_eq!(dirty.entries.len(), 1);
    assert_eq!(dirty.entries[0].path, "main.rs");
    assert_eq!(dirty.entries[0].status, " M");
    assert!(dirty.entries[0].content_hash.as_deref().is_some_and(|hash| hash.starts_with("sha256:")));

    let changed = changed_paths_since_generation(root.path(), Some(&head), current.available);
    assert!(changed.complete);
    assert_eq!(changed.paths, vec!["main.rs"]);
}
