//! Checkpoint lineage must survive the public CLI boundary without becoming durable memory.

use std::fs;
use std::process::Command;

#[test]
fn checkpoint_cli_round_trip_preserves_lineage_resolves_docs_and_stays_session_only() {
    let temp = tempfile::tempdir().unwrap();
    let bin = env!("CARGO_BIN_EXE_cortex");
    let db = temp.path().join("cortex.db");
    let document = temp.path().join("runbook.md");
    let markdown = "# P4 Runbook\n\nResume exact CLI lineage proof.\n";
    fs::write(&document, markdown).unwrap();
    // Guide owns document resolution; Cortex consumes only its source-bound
    // result while this test exercises Cortex's durable checkpoint boundary.
    let source_ref = "doc://repo/worktree/runbook.md";
    let outline = membrane_runtime::guide::outline::build_outline(
        source_ref,
        markdown,
        "comrak-0.54.0",
    );
    let content_hash = outline.content_hash;
    let anchor_id = outline.sections[0].anchor_id.clone();
    let read = membrane_runtime::guide::outline::read_section(
        source_ref,
        markdown,
        &anchor_id,
        &content_hash,
        12_000,
    )
    .unwrap();
    assert_eq!(read.source_ref, source_ref);
    assert_eq!(read.content_hash, content_hash);
    assert_eq!(read.anchor_id, anchor_id);
    assert_eq!(read.content, markdown);

    let checkpoint_id = "checkpoint/p4-cli/100";
    let installation_id = "550e8400-e29b-41d4-a716-446655440000";
    let input = temp.path().join("checkpoint.json");
    fs::write(
        &input,
        serde_json::to_vec(&serde_json::json!({
            "checkpointId": checkpoint_id,
            "installationId": installation_id,
            "client": "codex",
            "sessionId": "p4-cli-session",
            "repositoryId": "membrane",
            "worktreeRev": "p4-cli-revision",
            "scopeId": "membrane",
            "summary": "Resume P4 CLI lineage proof.",
            "goalSnapshot": "finish P4 checkpoint proof",
            "taskSnapshot": "load source-bound session state",
            "createdAtMs": 100,
            "expiresAtMs": 200,
            "sourceRefs": [{
                "sourceRef": source_ref,
                "expectedContentHash": content_hash,
                "anchorId": anchor_id,
                "label": "P4 runbook"
            }]
        }))
        .unwrap(),
    )
    .unwrap();

    let saved = Command::new(bin)
        .args([
            "--db",
            db.to_str().unwrap(),
            "checkpoint",
            "save",
            "--input",
            input.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        saved.status.success(),
        "save stderr: {}",
        String::from_utf8_lossy(&saved.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&saved.stdout).unwrap(),
        serde_json::json!({"checkpoint_id": checkpoint_id, "saved": true})
    );

    let loaded = Command::new(bin)
        .env("WORKSPACE_ROOT", temp.path())
        .args([
            "--db",
            db.to_str().unwrap(),
            "checkpoint",
            "load",
            checkpoint_id,
            "--as-of-ms",
            "150",
        ])
        .output()
        .unwrap();
    assert!(
        loaded.status.success(),
        "load stderr: {}",
        String::from_utf8_lossy(&loaded.stderr)
    );
    let loaded: serde_json::Value = serde_json::from_slice(&loaded.stdout).unwrap();
    let checkpoint = &loaded["checkpoint"];
    assert_eq!(checkpoint["checkpointId"], checkpoint_id);
    assert_eq!(checkpoint["installationId"], installation_id);
    assert_eq!(checkpoint["repositoryId"], "membrane");
    assert_eq!(checkpoint["worktreeRev"], "p4-cli-revision");
    assert_eq!(checkpoint["scopeId"], "membrane");
    assert_eq!(checkpoint["goalSnapshot"], "finish P4 checkpoint proof");
    assert_eq!(
        checkpoint["taskSnapshot"],
        "load source-bound session state"
    );
    assert_eq!(checkpoint["sourceRefs"][0]["sourceRef"], source_ref);
    assert_eq!(
        loaded["source_resolutions"],
        serde_json::json!([{
            "sourceRef": source_ref,
            "anchorId": anchor_id,
            "status": "ok"
        }])
    );

    let connection = rusqlite::Connection::open(db).unwrap();
    let row: (String, String, String, String, String) = connection
        .query_row(
            "SELECT artifact_family, record_type, authority, influence_class, lifecycle_state
             FROM memories WHERE id=?1",
            [checkpoint_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(
        row,
        (
            "session".into(),
            "checkpoint".into(),
            "A0".into(),
            "orientation".into(),
            "active".into(),
        )
    );
    let durable_rows: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM memories WHERE id=?1 AND artifact_family != 'session'",
            [checkpoint_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        durable_rows, 0,
        "checkpoint CLI save must not create durable memory"
    );
}
