//! `cortex skill-read <name>` resolves a workspace skill body cross-repo (via --root),
//! prints the body + bodyHash, and refuses path traversal / escapes.

use std::fs;
use std::process::Command;

use sha2::{Digest, Sha256};

fn write(path: &std::path::Path, data: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, data).unwrap();
}

#[test]
fn skill_read_resolves_body_and_guards_traversal() {
    let bin = env!("CARGO_BIN_EXE_cortex");
    let tmp = std::env::temp_dir().join(format!("cr-skillread-{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    let body = "---\nname: demo\ndescription: a demo skill body\n---\n# Demo\nrun the demo\n";
    write(&tmp.join("tools/skills/demo/SKILL.md"), body);
    let root = tmp.to_str().unwrap();
    let event_path = tmp.join("skill-events.jsonl");

    // Valid: prints the exact body on stdout, bodyHash on stderr.
    let ok = Command::new(bin)
        .env("MEMBRANE_SKILL_EVENT_PATH", &event_path)
        .env("CORTEX_CLIENT", "codex")
        .args(["skill-read", "demo", "--root", root])
        .output()
        .unwrap();
    assert!(
        ok.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&ok.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&ok.stdout), body);
    assert!(String::from_utf8_lossy(&ok.stderr).contains("bodyHash="));
    let rows: Vec<serde_json::Value> = fs::read_to_string(&event_path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(row["schema_version"], 1);
    assert_eq!(row["event"], "skill_resolved");
    assert_eq!(row["skill_id"], "skills:demo");
    assert_eq!(row["resource_class"], "skill_md");
    assert_eq!(row["source"], "disk");
    assert_eq!(row["client"], "codex");
    assert_eq!(row["bytes_returned"], body.len());
    assert_eq!(
        row["body_sha256"],
        format!("{:x}", Sha256::digest(body.as_bytes()))
    );
    let keys: std::collections::BTreeSet<&str> = row
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        [
            "body_sha256",
            "bytes_returned",
            "client",
            "event",
            "resource_class",
            "schema_version",
            "skill_id",
            "source",
            "ts",
        ]
        .into_iter()
        .collect()
    );
    let serialized = serde_json::to_string(row).unwrap();
    assert!(!serialized.contains("run the demo"));
    assert!(!serialized.contains(root));

    // Traversal is rejected before any read.
    let bad = Command::new(bin)
        .env("MEMBRANE_SKILL_EVENT_PATH", &event_path)
        .args(["skill-read", "../evil", "--root", root])
        .output()
        .unwrap();
    assert!(!bad.status.success(), "traversal must be rejected");

    // Unknown skill fails (no such SKILL.md).
    let missing = Command::new(bin)
        .env("MEMBRANE_SKILL_EVENT_PATH", &event_path)
        .args(["skill-read", "nope", "--root", root])
        .output()
        .unwrap();
    assert!(!missing.status.success());
    let control = Command::new(bin)
        .env("MEMBRANE_SKILL_EVENT_PATH", &event_path)
        .args(["skill-read", "bad\nname", "--root", root])
        .output()
        .unwrap();
    assert!(!control.status.success());
    let oversized_name = "x".repeat(129);
    let oversized = Command::new(bin)
        .env("MEMBRANE_SKILL_EVENT_PATH", &event_path)
        .args(["skill-read", &oversized_name, "--root", root])
        .output()
        .unwrap();
    assert!(!oversized.status.success());
    assert_eq!(fs::read_to_string(&event_path).unwrap().lines().count(), 1);

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn skill_read_serves_from_engine_without_skills_directory() {
    // THE portability contract: a machine/session with only the engine DB (no tools/skills
    // checkout, no symlink) still loads skills. Seed a DB with an ingested skill, point the
    // workspace root at an EMPTY dir, and require the body to come back (source=engine).
    let bin = env!("CARGO_BIN_EXE_cortex");
    let tmp = std::env::temp_dir().join(format!("cr-skilldb-{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).unwrap();
    let db = tmp.join("engine.db");
    // Seed via a git repo + ingest (the real write path).
    let repo = tmp.join("repo");
    let body = "---\nname: dbskill\ndescription: engine-served skill\n---\n# DbSkill\nserved from the DB\n";
    write(&repo.join("tools/skills/dbskill/SKILL.md"), body);
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(args)
            .output()
            .unwrap()
    };
    git(&["init", "-q"]);
    git(&["config", "user.email", "t@t.t"]);
    git(&["config", "user.name", "t"]);
    git(&["add", "-A"]);
    {
        let memdb = cortex::MemDb::open(&db).unwrap();
        let store = cortex::MemoryStore::try_open(memdb).unwrap();
        let (ingested, _, _) = store.ingest_skills(&repo);
        assert_eq!(ingested, 1, "the tracked skill must ingest into the engine");
    }
    // Empty workspace root: no tools/skills anywhere near it.
    let empty = tmp.join("empty");
    fs::create_dir_all(empty.join("tools")).unwrap();
    let event_path = tmp.join("skill-events.jsonl");
    let out = std::process::Command::new(bin)
        .env("CORTEX_DB", db.to_str().unwrap())
        .env("MEMBRANE_SKILL_EVENT_PATH", &event_path)
        .env("CORTEX_CLIENT", "claude")
        .args(["skill-read", "dbskill", "--root", empty.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), body);
    assert!(String::from_utf8_lossy(&out.stderr).contains("source=engine"));
    let row: serde_json::Value =
        serde_json::from_str(fs::read_to_string(&event_path).unwrap().trim()).unwrap();
    assert_eq!(row["event"], "skill_resolved");
    assert_eq!(row["skill_id"], "skills:dbskill");
    assert_eq!(row["source"], "engine");
    assert_eq!(row["client"], "claude");
    assert_eq!(row["bytes_returned"], body.len());

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn ingest_skills_prunes_a_skill_deleted_from_git_but_never_an_absent_tree() {
    // The `adapt` failure: its source was deleted, but the engine row survived because ingest only
    // added and updated. The skills provider kept advertising it and disk-first/engine-fallback
    // resolution kept serving a body whose pipeline and vocabulary were both gone.
    let tmp = std::env::temp_dir().join(format!("cr-skillprune-{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).unwrap();
    let db = tmp.join("engine.db");
    let repo = tmp.join("repo");
    write(
        &repo.join("tools/skills/keeper/SKILL.md"),
        "---\nname: keeper\ndescription: stays\n---\n# Keeper\n",
    );
    write(
        &repo.join("tools/skills/ghost/SKILL.md"),
        "---\nname: ghost\ndescription: gets deleted\n---\n# Ghost\n",
    );
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(args)
            .output()
            .unwrap()
    };
    git(&["init", "-q"]);
    git(&["config", "user.email", "t@t.t"]);
    git(&["config", "user.name", "t"]);
    git(&["add", "-A"]);

    let memdb = cortex::MemDb::open(&db).unwrap();
    let store = cortex::MemoryStore::try_open(memdb).unwrap();
    assert_eq!(store.ingest_skills(&repo), (2, 0, 0));
    assert!(store.skill_from_db("ghost").is_some());

    // Delete the skill the way a fold/retire actually happens: gone from disk and from the index.
    fs::remove_dir_all(repo.join("tools/skills/ghost")).unwrap();
    git(&["add", "-A"]);
    assert_eq!(
        store.ingest_skills(&repo),
        (1, 0, 1),
        "a skill removed from git must not survive in the engine"
    );
    assert!(
        store.skill_from_db("ghost").is_none(),
        "the retired skill must stop being served"
    );
    assert!(
        store.skill_from_db("keeper").is_some(),
        "the surviving skill must be untouched"
    );

    // Portability guard: a checkout WITHOUT tools/skills must never be read as "all skills were
    // deleted", or the engine store this exists to carry would erase itself.
    let bare = tmp.join("bare");
    fs::create_dir_all(bare.join("tools")).unwrap();
    std::process::Command::new("git")
        .arg("-C")
        .arg(&bare)
        .args(["init", "-q"])
        .output()
        .unwrap();
    assert_eq!(store.ingest_skills(&bare), (0, 0, 0));
    assert!(
        store.skill_from_db("keeper").is_some(),
        "an empty skills tree must not prune the portability store"
    );

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn skill_read_succeeds_when_event_sink_is_unwritable() {
    let bin = env!("CARGO_BIN_EXE_cortex");
    let tmp = std::env::temp_dir().join(format!("cr-skill-event-fail-{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    let body = "---\nname: demo\ndescription: demo\n---\n# Demo\n";
    write(&tmp.join("tools/skills/demo/SKILL.md"), body);
    let sink_is_directory = tmp.join("event-sink-directory");
    fs::create_dir_all(&sink_is_directory).unwrap();

    let output = Command::new(bin)
        .env("MEMBRANE_SKILL_EVENT_PATH", &sink_is_directory)
        .args(["skill-read", "demo", "--root", tmp.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout), body);

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn skill_read_does_not_emit_when_stdout_delivery_fails() {
    let bin = env!("CARGO_BIN_EXE_cortex");
    let tmp = std::env::temp_dir().join(format!("cr-skill-stdout-fail-{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    let body = format!(
        "---\nname: demo\ndescription: demo\n---\n# Demo\n{}",
        "x".repeat(2 * 1024 * 1024)
    );
    write(&tmp.join("tools/skills/demo/SKILL.md"), &body);
    let event_path = tmp.join("events.jsonl");
    let mut child = Command::new(bin)
        .env("MEMBRANE_SKILL_EVENT_PATH", &event_path)
        .args(["skill-read", "demo", "--root", tmp.to_str().unwrap()])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    drop(child.stdout.take());
    let status = child.wait().unwrap();
    assert!(!status.success());
    assert!(!event_path.exists());

    let _ = fs::remove_dir_all(&tmp);
}
