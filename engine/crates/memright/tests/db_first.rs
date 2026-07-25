//! DB-first surface: put/delete/list/reindex/export_md + serve routes.
use memright::memdb::MemDb;
use memright::store::MemoryStore;

fn store() -> MemoryStore {
    MemoryStore::open(MemDb::open_in_memory())
}

#[test]
fn put_persists_and_lists_and_deletes() {
    let s = store();
    let id = s.put(
        "test-note",
        "the quarterly launch targets EU retailers",
        "D--Claude",
        memright_core::MemoryTier::Semantic,
    );
    assert_eq!(id, "D--Claude/test-note");
    let rows = s.list(Some("D--Claude"));
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].0, id);
    assert!(rows[0].2 > 10); // chars
                             // update preserves counters
    #[allow(clippy::cloned_ref_to_slice_refs)]
    s.record_injections(std::slice::from_ref(&id)).unwrap();
    s.put(
        "test-note",
        "updated content body",
        "D--Claude",
        memright_core::MemoryTier::Semantic,
    );
    assert_eq!(s.inject_count(&id), 1, "counters must survive upsert");
    assert!(s.delete(&id));
    assert!(s.list(Some("D--Claude")).is_empty());
}

#[test]
fn reindex_memoizes_unchanged_rows() {
    let s = store();
    s.put(
        "a",
        "first memory content",
        "global",
        memright_core::MemoryTier::Semantic,
    );
    s.put(
        "b",
        "second memory content",
        "global",
        memright_core::MemoryTier::Semantic,
    );
    // put already recorded (content_hash, embed_model) -> a clean reindex skips everything
    assert_eq!(s.reindex().unwrap(), (0, 2));
    // invalidate one row's model tag -> exactly that row re-embeds, then converges
    s.tamper_embed_model_for_tests("global/a", "other-model");
    assert_eq!(s.reindex().unwrap(), (1, 1));
    assert_eq!(s.reindex().unwrap(), (0, 2));
    let (content, _) = s.get_full("global/a").unwrap();
    assert_eq!(content, "first memory content");
}

#[test]
fn export_md_writes_canonical_tree() {
    let s = store();
    std::env::set_var("WORKSPACE_ROOT", r"D:\Claude");
    s.put(
        "ws-note",
        "workspace scoped",
        "D--Claude",
        memright_core::MemoryTier::Semantic,
    );
    s.put(
        "proj-note",
        "project scoped",
        "D--Claude-coderight",
        memright_core::MemoryTier::Semantic,
    );
    s.put(
        "other",
        "foreign scoped",
        "C--Other-place",
        memright_core::MemoryTier::Semantic,
    );
    let dir = std::env::temp_dir().join(format!("mr-export-{}", std::process::id()));
    let n = s.export_md(&dir);
    assert_eq!(n, 3);
    assert!(dir.join("WS/ws-note.md").is_file());
    assert!(dir.join("WS-coderight/proj-note.md").is_file());
    assert!(dir.join("C--Other-place/other.md").is_file());
    let body = std::fs::read_to_string(dir.join("WS/ws-note.md")).unwrap();
    assert!(body.contains("workspace scoped") && body.contains("id: D--Claude/ws-note"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn serve_routes_put_get_list_delete_dashboard() {
    let s = store();
    let (c, b) = memright::serve::route_for_tests(
        &s,
        "POST",
        "/put",
        r#"{"name":"r1","content":"route made memory","scope":"global"}"#,
    );
    assert_eq!(c, 200, "{b}");
    let (c, b) = memright::serve::route_for_tests(&s, "POST", "/get", r#"{"id":"global/r1"}"#);
    assert_eq!(c, 200);
    assert!(b.contains("route made memory"));
    let (c, b) = memright::serve::route_for_tests(&s, "POST", "/list", "{}");
    assert_eq!(c, 200);
    assert!(b.contains("global/r1"));
    let (c, _) = memright::serve::route_for_tests(&s, "GET", "/", "");
    assert_eq!(c, 200);
    let (c, b) = memright::serve::route_for_tests(&s, "POST", "/delete", r#"{"id":"global/r1"}"#);
    assert_eq!(c, 200);
    assert!(b.contains("true"));
    let (c, _) =
        memright::serve::route_for_tests(&s, "POST", "/put", r#"{"name":"","content":""}"#);
    assert_eq!(c, 400);
}
