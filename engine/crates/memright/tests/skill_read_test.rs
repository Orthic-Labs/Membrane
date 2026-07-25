//! `memright skill-read <name>` resolves a workspace skill body cross-repo (via --root),
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
    let bin = env!("CARGO_BIN_EXE_memright");
    let tmp = std::env::temp_dir().join(format!("cr-skillread-{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    let body = "---\nname: demo\ndescription: a demo skill body\n---\n# Demo\nrun the demo\n";
    write(&tmp.join("tools/skills/demo/SKILL.md"), body);
    let root = tmp.to_str().unwrap();
    let event_path = tmp.join("skill-events.jsonl");

    // Valid: prints the exact body on stdout, bodyHash on stderr.
    let ok = Command::new(bin)
        .env("RIGHTCONTEXT_SKILL_EVENT_PATH", &event_path)
        .env("MEMRIGHT_CLIENT", "codex")
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
        .env("RIGHTCONTEXT_SKILL_EVENT_PATH", &event_path)
        .args(["skill-read", "../evil", "--root", root])
        .output()
        .unwrap();
    assert!(!bad.status.success(), "traversal must be rejected");

    // Unknown skill fails (no such SKILL.md).
    let missing = Command::new(bin)
        .env("RIGHTCONTEXT_SKILL_EVENT_PATH", &event_path)
        .args(["skill-read", "nope", "--root", root])
        .output()
        .unwrap();
    assert!(!missing.status.success());
    let control = Command::new(bin)
        .env("RIGHTCONTEXT_SKILL_EVENT_PATH", &event_path)
        .args(["skill-read", "bad\nname", "--root", root])
        .output()
        .unwrap();
    assert!(!control.status.success());
    let oversized_name = "x".repeat(129);
    let oversized = Command::new(bin)
        .env("RIGHTCONTEXT_SKILL_EVENT_PATH", &event_path)
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
    let bin = env!("CARGO_BIN_EXE_memright");
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
        let memdb = memright::MemDb::open(&db).unwrap();
        let store = memright::MemoryStore::try_open(memdb).unwrap();
        let (ingested, _) = store.ingest_skills(&repo);
        assert_eq!(ingested, 1, "the tracked skill must ingest into the engine");
    }
    // Empty workspace root: no tools/skills anywhere near it.
    let empty = tmp.join("empty");
    fs::create_dir_all(empty.join("tools")).unwrap();
    let event_path = tmp.join("skill-events.jsonl");
    let out = std::process::Command::new(bin)
        .env("MEMRIGHT_DB", db.to_str().unwrap())
        .env("RIGHTCONTEXT_SKILL_EVENT_PATH", &event_path)
        .env("MEMRIGHT_CLIENT", "claude")
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
fn skill_read_succeeds_when_event_sink_is_unwritable() {
    let bin = env!("CARGO_BIN_EXE_memright");
    let tmp = std::env::temp_dir().join(format!("cr-skill-event-fail-{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    let body = "---\nname: demo\ndescription: demo\n---\n# Demo\n";
    write(&tmp.join("tools/skills/demo/SKILL.md"), body);
    let sink_is_directory = tmp.join("event-sink-directory");
    fs::create_dir_all(&sink_is_directory).unwrap();

    let output = Command::new(bin)
        .env("RIGHTCONTEXT_SKILL_EVENT_PATH", &sink_is_directory)
        .args(["skill-read", "demo", "--root", tmp.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout), body);

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn skill_read_does_not_emit_when_stdout_delivery_fails() {
    let bin = env!("CARGO_BIN_EXE_memright");
    let tmp = std::env::temp_dir().join(format!("cr-skill-stdout-fail-{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    let body = format!(
        "---\nname: demo\ndescription: demo\n---\n# Demo\n{}",
        "x".repeat(2 * 1024 * 1024)
    );
    write(&tmp.join("tools/skills/demo/SKILL.md"), &body);
    let event_path = tmp.join("events.jsonl");
    let mut child = Command::new(bin)
        .env("RIGHTCONTEXT_SKILL_EVENT_PATH", &event_path)
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
