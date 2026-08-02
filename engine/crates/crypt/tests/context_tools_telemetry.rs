use std::process::Command;

#[test]
fn failed_skel_invocation_is_persisted_without_content() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("crypt.db");
    let missing = temp.path().join("missing.rs");

    let output = Command::new(env!("CARGO_BIN_EXE_crypt"))
        .args([
            "--db",
            db.to_str().unwrap(),
            "skel",
            missing.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let connection = rusqlite::Connection::open(db).unwrap();
    let (before, after, meta, event_uid, client, session_id, turn_id, trace_id, identity_status): (
        i64,
        i64,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
    ) = connection
        .query_row(
            "select before_chars, after_chars, meta, event_uid, client, session_id,
                    turn_id, trace_id, identity_status
             from transform_log where verb = 'skel'",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                    row.get(8)?,
                ))
            },
        )
        .unwrap();
    assert_eq!((before, after), (0, 0));
    assert_eq!(meta, "status=error;kind=input_read");
    assert!(event_uid.starts_with("transform."));
    assert_eq!(client, "cli");
    assert!(!session_id.is_empty());
    assert!(!turn_id.is_empty());
    assert!(!trace_id.is_empty());
    assert_eq!(identity_status, "observed");
}

#[test]
fn runc_uses_workspace_cache_and_prints_a_summary() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("crypt.db");
    let output = Command::new(env!("CARGO_BIN_EXE_crypt"))
        .current_dir(temp.path())
        .args([
            "--db",
            db.to_str().unwrap(),
            "runc",
            "--head",
            "1",
            "--tail",
            "1",
            "--",
            "printf 'one\\ntwo\\nthree\\n'",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let spill_dir = temp.path().join(".cache").join("runc");
    assert!(
        spill_dir.is_dir(),
        "missing workspace spill dir: {spill_dir:?}"
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("runc: exit=0"));
}

#[test]
fn runc_links_to_the_exact_recommendation_opportunity() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("crypt.db");
    let opportunity_uid = "opportunity.runc.cli-test";
    let create = Command::new(env!("CARGO_BIN_EXE_crypt"))
        .args([
            "--db",
            db.to_str().unwrap(),
            "transform-opportunity",
            "--verb",
            "runc",
            "--opportunity",
            opportunity_uid,
            "--source",
            "brief-bash",
            "--reason",
            "broad-read",
            "--client",
            "claude",
            "--session",
            "session-1",
            "--turn",
            "turn-1",
            "--trace",
            "trace-1",
        ])
        .output()
        .unwrap();
    assert!(
        create.status.success(),
        "{}",
        String::from_utf8_lossy(&create.stderr)
    );

    let run = Command::new(env!("CARGO_BIN_EXE_crypt"))
        .current_dir(temp.path())
        .args([
            "--db",
            db.to_str().unwrap(),
            "runc",
            "--opportunity",
            opportunity_uid,
            "--head",
            "1",
            "--tail",
            "1",
            "--",
            "printf 'one\\ntwo\\nthree\\n'",
        ])
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );

    let connection = rusqlite::Connection::open(db).unwrap();
    let row: (String, String, String, String, String) = connection
        .query_row(
            "select o.outcome, o.client, o.session_id, o.turn_id, t.opportunity_uid
             from transform_opportunity_log o
             join transform_log t using (opportunity_uid)
             where o.opportunity_uid=?1",
            [opportunity_uid],
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
            "used".into(),
            "claude".into(),
            "session-1".into(),
            "turn-1".into(),
            opportunity_uid.into(),
        )
    );
}
