//! Parity test for `blueprint/src/lib/update/apply.mjs` (scoped -- see
//! lib_update_apply.rs module docs).

use membrane_blueprint::lib_update_apply::{backup_store, copy_recursive, durable_json};
use serde_json::json;
use std::fs;

#[test]
fn durable_json_round_trips_and_leaves_no_temp_file_behind() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("transaction.json");
    durable_json(&path, &json!({ "phase": "prepared" })).unwrap();
    let read: serde_json::Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(read["phase"], "prepared");
    let leftover_temps = fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().contains(".tmp-"))
        .count();
    assert_eq!(leftover_temps, 0);
}

#[test]
fn durable_json_overwrites_an_existing_file_atomically() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("receipt.json");
    durable_json(&path, &json!({ "version": 1 })).unwrap();
    durable_json(&path, &json!({ "version": 2 })).unwrap();
    let read: serde_json::Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(read["version"], 2);
}

#[test]
fn copy_recursive_replicates_a_full_directory_tree() {
    let src = tempfile::tempdir().unwrap();
    fs::create_dir_all(src.path().join("bin")).unwrap();
    fs::write(src.path().join("bin/app"), b"binary").unwrap();
    fs::write(src.path().join("package.json"), b"{}").unwrap();
    let dst = tempfile::tempdir().unwrap().path().join("staged");
    copy_recursive(src.path(), &dst).unwrap();
    assert_eq!(fs::read(dst.join("bin/app")).unwrap(), b"binary");
    assert_eq!(fs::read(dst.join("package.json")).unwrap(), b"{}");
}

#[test]
fn backup_store_reports_no_backup_when_no_db_exists_yet() {
    let dir = tempfile::tempdir().unwrap();
    let result = backup_store(dir.path(), ".agent");
    assert!(!result.backed_up);
    assert!(result.path.is_none());
    assert!(result.error.is_none());
}

#[test]
fn backup_store_produces_a_readable_sqlite_backup() {
    let dir = tempfile::tempdir().unwrap();
    let graph_dir = dir.path().join(".agent").join("graph");
    fs::create_dir_all(&graph_dir).unwrap();
    let conn = rusqlite::Connection::open(graph_dir.join("graph.db")).unwrap();
    conn.execute_batch("CREATE TABLE nodes (id TEXT); INSERT INTO nodes VALUES ('n1');")
        .unwrap();
    drop(conn);

    let result = backup_store(dir.path(), ".agent");
    assert!(result.backed_up, "backup error: {:?}", result.error);
    let backup_path = result.path.unwrap();
    let backup_conn = rusqlite::Connection::open(&backup_path).unwrap();
    let count: i64 = backup_conn
        .query_row("SELECT COUNT(*) FROM nodes", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 1);
}
