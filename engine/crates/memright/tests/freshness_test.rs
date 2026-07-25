use memright::freshness::{
    canonical_repo_root, evaluate_freshness, FreshnessEpoch, FreshnessProbe, GraphState,
    OverlayObservation,
};
use serde::Deserialize;
use std::collections::{BTreeMap, VecDeque};
use std::path::Path;
use std::process::Command;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Fixture {
    schema_version: u32,
    cases: Vec<FixtureCase>,
}

#[derive(Debug, Deserialize)]
struct FixtureCase {
    name: String,
    attempts: Vec<FixtureAttempt>,
    expected: Expected,
}

#[derive(Debug, Deserialize)]
struct FixtureAttempt {
    before: FreshnessEpoch,
    overlay: OverlayObservation,
    after: FreshnessEpoch,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Expected {
    graph_state: GraphState,
    stable: bool,
    attempts: usize,
    blueprint_class: String,
    blueprint_usable: bool,
}

struct ScriptedProbe {
    epochs: VecDeque<FreshnessEpoch>,
    overlays: VecDeque<OverlayObservation>,
}

fn git(repo: &Path, args: &[&str]) -> String {
    let mut command = Command::new("git");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let output = command
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .expect("git starts");
    assert!(output.status.success(), "git {:?} failed", args);
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

impl FreshnessProbe for ScriptedProbe {
    fn read_epoch(&mut self) -> Result<FreshnessEpoch, String> {
        self.epochs
            .pop_front()
            .ok_or_else(|| "scripted epoch exhausted".to_string())
    }

    fn read_overlay(&mut self) -> Result<OverlayObservation, String> {
        self.overlays
            .pop_front()
            .ok_or_else(|| "scripted overlay exhausted".to_string())
    }
}

#[test]
fn shared_fixture_covers_clean_dirty_partial_and_concurrent_epochs() {
    let fixture: Fixture = serde_json::from_str(include_str!(
        "../../../federation/fixtures/freshness-conformance-v1.json"
    ))
    .expect("freshness fixture parses");
    assert_eq!(fixture.schema_version, 1);

    for case in fixture.cases {
        let mut epochs = VecDeque::new();
        let mut overlays = VecDeque::new();
        for attempt in case.attempts {
            epochs.push_back(attempt.before);
            epochs.push_back(attempt.after);
            overlays.push_back(attempt.overlay);
        }
        let mut probe = ScriptedProbe { epochs, overlays };
        let verdict = evaluate_freshness(&mut probe, 3);
        assert_eq!(
            verdict.graph_state, case.expected.graph_state,
            "{}",
            case.name
        );
        assert_eq!(verdict.stable, case.expected.stable, "{}", case.name);
        assert_eq!(verdict.attempts, case.expected.attempts, "{}", case.name);
        assert_eq!(
            verdict.providers.blueprint.freshness_class, case.expected.blueprint_class,
            "{}",
            case.name
        );
        assert_eq!(
            verdict.providers.blueprint.usable, case.expected.blueprint_usable,
            "{}",
            case.name
        );
    }
}

#[test]
fn overlay_digest_covers_entries_beyond_the_return_cap() {
    let epoch = FreshnessEpoch::coherent_for_test("a", "g", "s");
    let entries = (0..70)
        .map(|index| memright::freshness::OverlayEntry {
            path: format!("src/{index}.rs"),
            status: " M".to_string(),
            content_hash: format!("sha256:{index:064x}"),
        })
        .collect::<Vec<_>>();
    let mut first = ScriptedProbe {
        epochs: VecDeque::from([epoch.clone(), epoch.clone()]),
        overlays: VecDeque::from([OverlayObservation {
            stable: true,
            entries: entries.clone(),
            limit_exceeded: false,
            stage_elapsed_ms: BTreeMap::new(),
        }]),
    };
    let verdict = evaluate_freshness(&mut first, 3);
    assert_eq!(verdict.overlay_count, 70);
    assert_eq!(verdict.overlay_entries.len(), 64);
    assert!(verdict.overlay_truncated);

    let mut changed = entries;
    changed[69].content_hash = format!("sha256:{:064x}", 999u64);
    let mut second = ScriptedProbe {
        epochs: VecDeque::from([epoch.clone(), epoch]),
        overlays: VecDeque::from([OverlayObservation {
            stable: true,
            entries: changed,
            limit_exceeded: false,
            stage_elapsed_ms: BTreeMap::new(),
        }]),
    };
    let changed_verdict = evaluate_freshness(&mut second, 3);
    assert_ne!(verdict.overlay_digest, changed_verdict.overlay_digest);
}

#[test]
fn overlay_limit_failure_is_typed_indeterminate() {
    let epoch = FreshnessEpoch::coherent_for_test("a", "g", "s");
    let mut probe = ScriptedProbe {
        epochs: VecDeque::from([epoch.clone(), epoch]),
        overlays: VecDeque::from([OverlayObservation {
            stable: false,
            entries: vec![],
            limit_exceeded: true,
            stage_elapsed_ms: BTreeMap::new(),
        }]),
    };
    let verdict = evaluate_freshness(&mut probe, 3);
    assert_eq!(verdict.graph_state, GraphState::Indeterminate);
    assert!(!verdict.stable);
    assert!(verdict
        .reasons
        .iter()
        .any(|reason| reason == "overlay_limit_exceeded"));
}

#[test]
fn partial_reindex_degrades_blueprint_without_disabling_stable_overlay_or_skills() {
    let mut epoch = FreshnessEpoch::coherent_for_test("a", "old-generation", "skills");
    epoch.blueprint_generation = Some("new-generation".to_string());
    let mut probe = ScriptedProbe {
        epochs: VecDeque::from([epoch.clone(), epoch]),
        overlays: VecDeque::from([OverlayObservation {
            stable: true,
            entries: vec![memright::freshness::OverlayEntry {
                path: "src/app.rs".to_string(),
                status: " M".to_string(),
                content_hash: format!("sha256:{:064x}", 7u64),
            }],
            limit_exceeded: false,
            stage_elapsed_ms: BTreeMap::new(),
        }]),
    };

    let verdict = evaluate_freshness(&mut probe, 3);

    assert_eq!(verdict.graph_state, GraphState::PartialReindex);
    assert!(!verdict.providers.blueprint.usable);
    assert!(verdict.providers.dirty_overlay.usable);
    assert!(verdict.providers.skills.usable);
}

#[test]
fn legacy_blueprint_without_base_commit_keeps_head_overlay_available_without_claiming_currency() {
    let mut epoch = FreshnessEpoch::coherent_for_test(
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "generation-a",
        "skills-a",
    );
    epoch.base_commit = None;
    let mut probe = ScriptedProbe {
        epochs: VecDeque::from([epoch.clone(), epoch]),
        overlays: VecDeque::from([OverlayObservation {
            stable: true,
            entries: vec![memright::freshness::OverlayEntry {
                path: "src/app.rs".to_string(),
                status: " M".to_string(),
                content_hash:
                    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                        .to_string(),
            }],
            limit_exceeded: false,
            stage_elapsed_ms: BTreeMap::new(),
        }]),
    };

    let verdict = evaluate_freshness(&mut probe, 3);

    assert_eq!(verdict.graph_state, GraphState::Indeterminate);
    assert_eq!(
        verdict.base_commit.as_deref(),
        Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
    );
    assert_eq!(verdict.blueprint_base_commit, None);
    assert!(!verdict.providers.blueprint.usable);
    assert!(verdict.providers.dirty_overlay.usable);
}

#[test]
fn git_status_stage_accumulates_across_freshness_retries() {
    let first = FreshnessEpoch::coherent_for_test("a", "g", "s");
    let second = FreshnessEpoch::coherent_for_test("b", "g", "s");
    let mut probe = ScriptedProbe {
        epochs: VecDeque::from([first.clone(), second.clone(), second.clone(), second]),
        overlays: VecDeque::from([
            OverlayObservation {
                stable: true,
                entries: Vec::new(),
                limit_exceeded: false,
                stage_elapsed_ms: BTreeMap::from([("git_status".to_string(), 4)]),
            },
            OverlayObservation {
                stable: true,
                entries: Vec::new(),
                limit_exceeded: false,
                stage_elapsed_ms: BTreeMap::from([("git_status".to_string(), 6)]),
            },
        ]),
    };

    let verdict = evaluate_freshness(&mut probe, 3);

    assert_eq!(verdict.attempts, 2);
    assert_eq!(verdict.stage_elapsed_ms.get("git_status"), Some(&10));
}

#[test]
fn freshness_route_returns_versioned_content_free_service_metadata() {
    let store = memright::MemoryStore::new();
    let workspace = std::env::current_dir().unwrap().canonicalize().unwrap();
    let repo = tempfile::Builder::new()
        .prefix("freshness-route-")
        .tempdir_in(&workspace)
        .unwrap();
    git(repo.path(), &["init", "--quiet"]);
    git(
        repo.path(),
        &["config", "user.email", "test@example.invalid"],
    );
    git(repo.path(), &["config", "user.name", "Test"]);
    std::fs::write(repo.path().join("fixture.txt"), "fixture\n").unwrap();
    git(repo.path(), &["add", "fixture.txt"]);
    git(repo.path(), &["commit", "--quiet", "-m", "fixture"]);
    let repo_root = repo.path().canonicalize().unwrap();
    let body = serde_json::json!({ "repoRoot": repo_root }).to_string();
    let (status, payload) = memright::serve::route_for_tests(&store, "POST", "/freshness", &body);
    assert_eq!(status, 200, "{payload}");
    let value: serde_json::Value = serde_json::from_str(&payload).unwrap();
    assert_eq!(value["schemaVersion"], 1);
    assert!(value["serviceGeneration"]
        .as_str()
        .is_some_and(|generation| generation.starts_with("sha256:")));
    assert!(value["firstAfterIdle"].is_boolean());
    assert!(value["stageElapsedMs"]["git_status"].is_u64());
    assert!(value.get("content").is_none());
}

#[test]
fn filesystem_probe_binds_blueprint_to_commit_and_reports_verified_dirty_overlay() {
    let repo = tempfile::tempdir().unwrap();
    git(repo.path(), &["init", "--quiet"]);
    git(
        repo.path(),
        &["config", "user.email", "test@example.invalid"],
    );
    git(repo.path(), &["config", "user.name", "Test"]);
    std::fs::write(repo.path().join("app.rs"), "fn main() {}\n").unwrap();
    git(repo.path(), &["add", "app.rs"]);
    git(repo.path(), &["commit", "--quiet", "-m", "fixture"]);
    let head = git(repo.path(), &["rev-parse", "HEAD"]);
    let generation = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    std::fs::create_dir_all(repo.path().join(".blueprint")).unwrap();
    std::fs::create_dir_all(repo.path().join(".agent/graph")).unwrap();
    std::fs::write(
        repo.path().join(".blueprint/manifest.json"),
        serde_json::json!({"generation": {"id": generation, "baseCommit": head}}).to_string(),
    )
    .unwrap();
    std::fs::write(
        repo.path().join(".agent/graph/manifest.json"),
        serde_json::json!({"generationId": generation}).to_string(),
    )
    .unwrap();
    std::fs::write(
        repo.path().join(".agent/graph/graph.json"),
        serde_json::json!({"manifest": {"generationId": generation}}).to_string(),
    )
    .unwrap();
    let store = memright::MemoryStore::new();

    let clean = memright::freshness::evaluate_repository_freshness(
        &store,
        repo.path().canonicalize().unwrap(),
    );
    assert_eq!(clean.graph_state, GraphState::Clean);
    assert_eq!(clean.base_commit.as_deref(), Some(head.as_str()));
    assert_eq!(clean.blueprint_base_commit.as_deref(), Some(head.as_str()));
    assert!(clean.providers.blueprint.usable);
    assert!(clean.stage_elapsed_ms.contains_key("git_status"));

    std::fs::write(
        repo.path().join("app.rs"),
        "fn main() { println!(\"dirty\"); }\n",
    )
    .unwrap();
    let dirty = memright::freshness::evaluate_repository_freshness(
        &store,
        repo.path().canonicalize().unwrap(),
    );
    assert_eq!(dirty.graph_state, GraphState::DirtyOverlay);
    assert!(dirty.providers.dirty_overlay.usable);
    assert_eq!(
        dirty.providers.dirty_overlay.freshness_class,
        "dirty_overlay"
    );
    assert_eq!(dirty.overlay_entries.len(), 1);
    assert_eq!(dirty.overlay_entries[0].path, "app.rs");
    assert!(dirty.idle_gap_ms.is_some());
}

#[test]
fn canonical_repo_root_rejects_a_sibling_escape() {
    let workspace = tempfile::tempdir().unwrap();
    let allowed = workspace.path().join("allowed");
    let sibling = workspace.path().join("sibling");
    std::fs::create_dir_all(&allowed).unwrap();
    std::fs::create_dir_all(&sibling).unwrap();
    let error = canonical_repo_root(&sibling, &allowed).unwrap_err();
    assert_eq!(error, "repoRoot is outside the configured workspace");
}

#[test]
fn skills_generation_changes_only_when_skill_snapshot_content_changes() {
    let workspace = tempfile::tempdir().unwrap();
    let skill_dir = workspace.path().join("tools/skills/demo");
    std::fs::create_dir_all(&skill_dir).unwrap();
    std::fs::write(
        skill_dir.join("SKILL.md"),
        "---\ndescription: demo\n---\nfirst\n",
    )
    .unwrap();
    run_git(workspace.path(), &["init"]);
    run_git(workspace.path(), &["add", "tools/skills/demo/SKILL.md"]);

    let store = memright::MemoryStore::new();
    assert_eq!(store.ingest_skills(workspace.path()), (1, 0));
    let first = store.skills_generation().unwrap();
    let snapshot = store.skills_snapshot().unwrap();
    assert_eq!(snapshot.generation, first);
    assert_eq!(snapshot.skills.len(), 1);
    assert_eq!(snapshot.skills[0].name, "demo");
    assert_eq!(snapshot.skills[0].body_hash.len(), 64);
    assert_eq!(store.ingest_skills(workspace.path()), (1, 0));
    assert_eq!(store.skills_generation().unwrap(), first);

    std::fs::write(
        skill_dir.join("SKILL.md"),
        "---\ndescription: demo\n---\nsecond\n",
    )
    .unwrap();
    assert_eq!(store.ingest_skills(workspace.path()), (1, 0));
    assert_ne!(store.skills_generation().unwrap(), first);
}

fn run_git(root: &std::path::Path, args: &[&str]) {
    let mut command = std::process::Command::new("git");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let output = command.arg("-C").arg(root).args(args).output().unwrap();
    assert!(
        output.status.success(),
        "git failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
