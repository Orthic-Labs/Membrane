use membrane_blueprint::migrations::{migrate, migrate_to, schema_version, SCHEMA_VERSION};
use membrane_blueprint::store::{
    migration_backup_path, open_store, open_store_read_only, repair_interrupted_migration,
    RepairOutcome,
};
use rusqlite::Connection;
use std::fs;

#[test]
fn schema_v20_has_exact_tables_indexes_and_nullable_confidence() {
    let mut db = Connection::open_in_memory().unwrap();
    migrate(&mut db).unwrap();
    assert_eq!(schema_version(&db).unwrap(), 20);
    assert_eq!(SCHEMA_VERSION, 20);

    let tables: Vec<String> = db
        .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' AND name NOT LIKE 'symbol_search_%' ORDER BY name")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    let expected_tables = [
        "annotation_nodes", "artifact_state", "claim_code_edges", "claims", "dependency_index",
        "document_supersession", "documents", "edges", "event_journal", "fact_owner", "file_state",
        "files", "generation", "generation_leaf", "generation_receipt", "meta", "named_snapshot",
        "node_provider", "provider_ranks", "symbol_search", "symbol_terms", "symbols", "vectors",
        "watch_state",
    ];
    assert_eq!(tables, expected_tables);

    let indexes: Vec<String> = db
        .prepare("SELECT name FROM sqlite_master WHERE type='index' AND name NOT LIKE 'sqlite_%' ORDER BY name")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    let expected_indexes = [
        "idx_annotation_nodes_generation_ordinal", "idx_claim_code_edges_claim", "idx_claim_code_edges_generation",
        "idx_claim_code_edges_kind", "idx_claims_document", "idx_claims_generation", "idx_dep_source",
        "idx_document_supersession_generation", "idx_documents_generation", "idx_documents_path", "idx_edges_gen_source_kind", "idx_edges_gen_target_kind",
        "idx_edges_generation", "idx_edges_kind", "idx_edges_source", "idx_edges_target", "idx_edges_tier",
        "idx_event_journal_applied", "idx_fact_owner_domain", "idx_fact_owner_generation", "idx_fact_owner_path",
        "idx_fact_owner_provider_path", "idx_fact_owner_repo_root", "idx_files_generation", "idx_files_node_ordinal",
        "idx_generation_leaf_parent", "idx_generation_receipt_created", "idx_named_snapshot_generation",
        "idx_node_provider_provider_path", "idx_symbol_terms_symbol",
        "idx_symbols_generation", "idx_symbols_generation_name", "idx_symbols_generation_qualified", "idx_symbols_path",
        "idx_vectors_generation", "idx_vectors_model",
    ];
    assert_eq!(indexes, expected_indexes);

    for table in ["symbols", "edges", "claim_code_edges"] {
        let nullable: i64 = db
            .query_row(
                &format!("SELECT \"notnull\" FROM pragma_table_info('{table}') WHERE name='confidence'"),
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(nullable, 0, "{table}.confidence must be nullable");
    }
}

#[test]
fn nullable_confidence_fk_cascade_and_ordered_reads_round_trip() {
    let db = open_store(None).unwrap();
    db.execute_batch("PRAGMA foreign_keys = ON;") .unwrap();
    db.execute("INSERT INTO documents(id,path,generation_id) VALUES ('doc-2','b.md','g'),('doc-1','a.md','g')", []).unwrap();
    db.execute("INSERT INTO claims(id,document_id,text,generation_id) VALUES ('claim-2','doc-2','two','g'),('claim-1','doc-1','one','g')", []).unwrap();
    db.execute("INSERT INTO claim_code_edges(id,claim_id,kind,source,target,confidence,generation_id) VALUES ('join-2','claim-2','implements','doc-2','code-2',NULL,'g'),('join-1','claim-1','implements','doc-1','code-1',NULL,'g')", []).unwrap();
    db.execute("INSERT INTO symbols(id,kind,labels,name,qualified_name,path,confidence,evidence,generation_id) VALUES ('s-1','symbol','[]','s','s','a.ts',NULL,'[]','g')", []).unwrap();
    db.execute("INSERT INTO edges(id,kind,source,target,confidence,evidence,generation_id) VALUES ('e-1','calls','s-1',NULL,NULL,'[]','g')", []).unwrap();

    let symbol_confidence: Option<f64> = db.query_row("SELECT confidence FROM symbols WHERE id='s-1'", [], |row| row.get(0)).unwrap();
    let edge_confidence: Option<f64> = db.query_row("SELECT confidence FROM edges WHERE id='e-1'", [], |row| row.get(0)).unwrap();
    let join_confidence: Option<f64> = db.query_row("SELECT confidence FROM claim_code_edges WHERE id='join-1'", [], |row| row.get(0)).unwrap();
    assert_eq!(symbol_confidence, None);
    assert_eq!(edge_confidence, None);
    assert_eq!(join_confidence, None);

    let docs: Vec<String> = db.prepare("SELECT id FROM documents WHERE generation_id='g' ORDER BY rowid").unwrap().query_map([], |row| row.get(0)).unwrap().collect::<Result<_, _>>().unwrap();
    let claims: Vec<String> = db.prepare("SELECT id FROM claims WHERE generation_id='g' ORDER BY rowid").unwrap().query_map([], |row| row.get(0)).unwrap().collect::<Result<_, _>>().unwrap();
    let joins: Vec<String> = db.prepare("SELECT id FROM claim_code_edges WHERE generation_id='g' ORDER BY rowid").unwrap().query_map([], |row| row.get(0)).unwrap().collect::<Result<_, _>>().unwrap();
    assert_eq!(docs, vec!["doc-2".to_string(), "doc-1".to_string()]);
    assert_eq!(claims, vec!["claim-2".to_string(), "claim-1".to_string()]);
    assert_eq!(joins, vec!["join-2".to_string(), "join-1".to_string()]);

    db.execute("DELETE FROM documents WHERE id='doc-2'", []).unwrap();
    let claims_left: i64 = db.query_row("SELECT COUNT(*) FROM claims WHERE document_id='doc-2'", [], |row| row.get(0)).unwrap();
    let joins_left: i64 = db.query_row("SELECT COUNT(*) FROM claim_code_edges WHERE claim_id='claim-2'", [], |row| row.get(0)).unwrap();
    assert_eq!(claims_left, 0);
    assert_eq!(joins_left, 0);
}

#[test]
fn foreign_keys_are_enabled_by_store_boundary() {
    let db = open_store(None).unwrap();
    let on: i64 = db.query_row("PRAGMA foreign_keys", [], |row| row.get(0)).unwrap();
    assert_eq!(on, 1);
}

#[test]
fn file_store_backup_repair_and_read_only_are_version_bound() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("graph.db");
    let mut legacy = Connection::open(&path).unwrap();
    migrate_to(&mut legacy, 19).unwrap();
    drop(legacy);

    let backup = migration_backup_path(&path, 19);
    let wrong_backup = migration_backup_path(&path, 18);
    let db = open_store(Some(&path)).unwrap();
    assert_eq!(schema_version(&db).unwrap(), 20);
    drop(db);
    assert!(backup.is_file());
    assert!(!wrong_backup.exists());

    let outcome = repair_interrupted_migration(&path, 19).unwrap();
    assert_eq!(outcome, RepairOutcome::Restored { from_version: 19 });
    let repaired = open_store_read_only(&path).unwrap();
    assert_eq!(schema_version(&repaired).unwrap(), 20);
    drop(repaired);
    fs::remove_file(&backup).unwrap();
    assert_eq!(repair_interrupted_migration(&path, 19).unwrap(), RepairOutcome::NoBackup);

    let readonly_path = temp.path().join("readonly.db");
    let mut readonly_source = Connection::open(&readonly_path).unwrap();
    migrate_to(&mut readonly_source, 19).unwrap();
    drop(readonly_source);
    let readonly = open_store_read_only(&readonly_path).unwrap();
    assert_eq!(schema_version(&readonly).unwrap(), 19, "read-only open must not migrate");
    let busy_timeout: i64 = readonly.query_row("PRAGMA busy_timeout", [], |row| row.get(0)).unwrap();
    let mmap_size: i64 = readonly.query_row("PRAGMA mmap_size", [], |row| row.get(0)).unwrap();
    let cache_size: i64 = readonly.query_row("PRAGMA cache_size", [], |row| row.get(0)).unwrap();
    assert_eq!(busy_timeout, 5_000);
    assert_eq!(mmap_size, 268_435_456);
    assert_eq!(cache_size, -65_536);
}
