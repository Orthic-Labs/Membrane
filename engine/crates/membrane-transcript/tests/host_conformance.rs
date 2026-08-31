use std::path::PathBuf;

use membrane_transcript::{
    discover_open, parse_prefix_receipt, parse_source_events, TranscriptError,
};
use rusqlite::{params, Connection};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

#[test]
fn every_file_host_has_raw_parse_identity_provenance_and_exact_source_binding() {
    for (host, name, session) in [
        ("command_code", "command_code.jsonl", "cc-1"),
        ("pi", "pi.jsonl", "pi-1"),
        ("qwen", "qwen.jsonl", "qwen-1"),
        ("cline", "cline.json", "cline-1"),
        ("gemini", "gemini.json", "gemini-1"),
        ("gemini", "gemini.jsonl", "gemini-jsonl-1"),
        ("grok_build", "grok_build.jsonl", ""),
        ("roo_cline", "roo_cline.json", ""),
    ] {
        let path = fixture(name);
        let bytes = std::fs::read(&path).unwrap();
        let events = parse_source_events(&path, Some(host)).unwrap();
        assert!(
            events
                .iter()
                .any(|event| event.role.as_deref() == Some("user")),
            "{host}"
        );
        assert!(
            events
                .iter()
                .any(|event| event.role.as_deref() == Some("assistant")),
            "{host}"
        );
        assert!(events.iter().all(|event| event.host == host), "{host}");
        if matches!(host, "pi" | "cline") {
            assert!(
                events.iter().any(|event| event.private_reasoning_omitted),
                "{host}"
            );
        }
        if !session.is_empty() {
            assert!(
                events.iter().all(|event| event.session_id == session),
                "{host}"
            );
        }
        for event in &events {
            assert!(event.byte_end as usize <= bytes.len(), "{host}");
            assert!(
                !bytes[event.byte_start as usize..event.byte_end as usize].is_empty(),
                "{host}"
            );
        }
        let receipt = parse_prefix_receipt(&path, Some(host)).unwrap();
        assert_eq!(receipt.events_observed, events.len(), "{host}");
    }
}

fn unique_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("membrane-host-{label}-{}", std::process::id()));
    if dir.exists() {
        std::fs::remove_dir_all(&dir).unwrap();
    }
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn opencode_open_session_database_is_parsed_natively() {
    let dir = unique_dir("opencode");
    let path = dir.join("opencode.db");
    let db = Connection::open(&path).unwrap();
    db.execute_batch("CREATE TABLE session(id TEXT PRIMARY KEY,directory TEXT,parent_id TEXT,agent TEXT,model TEXT,time_created INTEGER,time_archived INTEGER);CREATE TABLE message(id TEXT PRIMARY KEY,session_id TEXT,time_created INTEGER,data TEXT);CREATE TABLE part(id TEXT PRIMARY KEY,message_id TEXT,session_id TEXT,time_created INTEGER,data TEXT);").unwrap();
    db.execute(
        "INSERT INTO session VALUES(?1,'/repo',NULL,'build','ox',1,NULL)",
        ["oc-1"],
    )
    .unwrap();
    db.execute(
        "INSERT INTO message VALUES('m1',?1,2,?2)",
        params!["oc-1", r#"{"role":"user","time":{"created":2}}"#],
    )
    .unwrap();
    db.execute(
        "INSERT INTO part VALUES('p1','m1',?1,3,?2)",
        params![
            "oc-1",
            r#"{"type":"text","text":"Keep behavior deterministic."}"#
        ],
    )
    .unwrap();
    db.execute(
        "INSERT INTO message VALUES('m2',?1,4,?2)",
        params!["oc-1", r#"{"role":"assistant","time":{"created":4}}"#],
    )
    .unwrap();
    db.execute("INSERT INTO part VALUES('p2','m2',?1,5,?2)", params!["oc-1", r#"{"type":"tool","tool":"read","callID":"oc-call","state":{"status":"completed","input":{"path":"x"},"output":"ok"}}"#]).unwrap();
    db.execute(
        "INSERT INTO part VALUES('p3','m2',?1,6,?2)",
        params!["oc-1", r#"{"type":"reasoning","text":"private"}"#],
    )
    .unwrap();
    drop(db);
    let events = parse_source_events(&path, Some("opencode")).unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|event| event.session_id == "oc-1")
            .count(),
        4
    );
    assert!(events
        .iter()
        .any(|event| event.kind == "tool_result" && event.tool_call_event_id.is_some()));
    assert!(events.iter().any(|event| event.private_reasoning_omitted));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn cursor_open_root_composer_database_is_parsed_natively() {
    let dir = unique_dir("cursor");
    let path = dir.join("state.vscdb");
    let db = Connection::open(&path).unwrap();
    db.execute_batch("CREATE TABLE composerHeaders(composerId TEXT PRIMARY KEY,workspaceId TEXT,createdAt INTEGER,isArchived INTEGER,isSubagent INTEGER,value TEXT);CREATE TABLE cursorDiskKV(key TEXT PRIMARY KEY,value BLOB);").unwrap();
    db.execute(
        "INSERT INTO composerHeaders VALUES('cu-1','/repo',1,0,0,'{}')",
        [],
    )
    .unwrap();
    db.execute(
        "INSERT INTO cursorDiskKV VALUES('bubbleId:cu-1:01',?1)",
        [r#"{"type":1,"text":"Keep changes scoped."}"#],
    )
    .unwrap();
    db.execute("INSERT INTO cursorDiskKV VALUES('bubbleId:cu-1:02',?1)", [r#"{"type":2,"toolFormerData":{"name":"read","toolCallId":"cu-call","params":{"path":"x"},"result":"ok","status":"completed"}}"#]).unwrap();
    drop(db);
    let events = parse_source_events(&path, Some("cursor")).unwrap();
    assert!(events.iter().all(|event| event.session_id == "cu-1"));
    assert!(events.iter().any(|event| event.kind == "user_message"));
    assert!(events
        .iter()
        .any(|event| event.kind == "tool_result" && event.tool_call_event_id.is_some()));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn discovery_maps_real_roots_and_excludes_commandcode_checkpoints() {
    let home = unique_dir("discover");
    for (relative, body) in [
        (".commandcode/projects/repo/live.jsonl", "{}\n"),
        (".commandcode/projects/repo/live.checkpoints.jsonl", "{}\n"),
        (".pi/agent/sessions/repo/pi.jsonl", "{}\n"),
        (".qwen/sessions/qwen.jsonl", "{}\n"),
        (".cline/data/sessions/s/s.messages.json", "{}"),
        (".copilot/session-state/s/copilot.jsonl", "{}\n"),
        (
            ".gemini/antigravity/brain/s/.system_generated/logs/events.jsonl",
            "{}\n",
        ),
    ] {
        let path = home.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }
    let found = discover_open(&home);
    assert!(found.iter().any(|item| item.host == "command_code"));
    assert!(!found
        .iter()
        .any(|item| item.path.to_string_lossy().contains("checkpoints")));
    for host in ["pi", "qwen", "cline", "copilot", "antigravity"] {
        assert!(found.iter().any(|item| item.host == host), "{host}");
    }
    std::fs::remove_dir_all(home).unwrap();
}

#[test]
fn valid_but_unrecognized_raw_source_is_typed_degradation() {
    let dir = unique_dir("no-events");
    let path = dir.join("raw.jsonl");
    std::fs::write(&path, "{\"type\":\"model_change\",\"model\":\"x\"}\n").unwrap();
    let error = parse_source_events(&path, Some("command_code")).unwrap_err();
    assert!(matches!(error, TranscriptError::NoEvents { .. }));
    std::fs::remove_dir_all(dir).unwrap();
}
