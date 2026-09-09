use membrane_blueprint::migrations::{migrate, migrate_to, schema_version};
use rusqlite::Connection;

#[test]
fn migration_is_idempotent_and_transactional() { let mut db = Connection::open_in_memory().unwrap(); assert_eq!(migrate(&mut db).unwrap(), 20); assert_eq!(migrate(&mut db).unwrap(), 20); assert_eq!(schema_version(&db).unwrap(), 20); }

#[test]
fn legacy_v16_shape_upgrades_without_losing_rows() {
    let mut db = Connection::open_in_memory().unwrap();
    migrate_to(&mut db, 16).unwrap();
    db.execute_batch("INSERT INTO generation(key,value) VALUES('provider','{\"id\":\"blueprint-static\",\"version\":\"v1\"}'); INSERT INTO symbols(id,kind,labels,name,qualified_name,path,confidence,evidence,generation_id,extra,node_ordinal) VALUES('s','Function','[]','f','f','a.ts',1,'[]','g','{\"factProvider\":{\"id\":\"blueprint-static\",\"version\":\"v1\"}}',1);").unwrap();
    migrate(&mut db).unwrap(); assert_eq!(schema_version(&db).unwrap(), 20); let id: String = db.query_row("SELECT id FROM symbols", [], |r| r.get(0)).unwrap(); assert_eq!(id, "s");
}

#[test]
fn canonical_migrate_to_preserves_v15_dense_order_and_annotation_provider() {
    let mut db = Connection::open_in_memory().unwrap();
    migrate_to(&mut db, 15).unwrap();
    db.execute_batch("INSERT INTO generation(key,value) VALUES ('provider','{\"id\":\"blueprint-static\",\"version\":\"v1\"}'),('manifest','{\"generationId\":\"g15\"}'); INSERT INTO files(path,provider,generation_id,node_id,content_hash) VALUES ('b.ts','blueprint-static','g15','file:b','h-b'),('a.ts','blueprint-static','g15','file:a','h-a'); INSERT INTO symbols(id,kind,labels,name,qualified_name,path,confidence,evidence,generation_id,extra,node_ordinal) VALUES ('symbol:a','symbol','[]','a','a','a.ts',1,'[]','g15','{\"factProvider\":{\"id\":\"blueprint-treesitter\",\"version\":\"v2\"}}',1); INSERT INTO annotation_nodes(id,node_ordinal,payload,generation_id) VALUES ('comment:a',0,'{\"factProvider\":{\"id\":\"blueprint-treesitter\",\"version\":\"v2\"}}','g15');");
    migrate(&mut db).unwrap();
    assert_eq!(schema_version(&db).unwrap(), 20);
    assert_eq!(db.query_row("SELECT COUNT(*) FROM node_provider", [], |r| r.get::<_, i64>(0)).unwrap(), 4);
    assert_eq!(db.query_row("SELECT provider_id FROM node_provider WHERE node_id='comment:a'", [], |r| r.get::<_, String>(0)).unwrap(), "treesitter");
    let ordinals: Vec<i64> = db.prepare("SELECT node_ordinal FROM files UNION ALL SELECT node_ordinal FROM symbols UNION ALL SELECT node_ordinal FROM annotation_nodes ORDER BY node_ordinal").unwrap().query_map([], |r| r.get(0)).unwrap().collect::<Result<_, _>>().unwrap();
    assert_eq!(ordinals, vec![0, 1, 2, 3]);
}

#[test]
fn v16_collision_rolls_back_provenance_and_nullable_confidence_is_applied() {
    let mut db = Connection::open_in_memory().unwrap();
    migrate_to(&mut db, 16).unwrap();
    db.execute_batch("INSERT INTO generation(key,value) VALUES ('provider','lexical'),('manifest','{\"generationId\":\"g16\"}'); INSERT INTO files(path,provider,generation_id,node_id,content_hash,node_ordinal) VALUES ('é.ts','lexical','g16','file:one','h',0),('é.ts','lexical','g16','file:two','h',1);").unwrap();
    assert!(migrate(&mut db).is_err());
    assert_eq!(schema_version(&db).unwrap(), 16);
    assert_eq!(db.query_row("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='node_provider'", [], |r| r.get::<_, i64>(0)).unwrap(), 0);

    db.execute("DELETE FROM files WHERE node_id='file:two'", []).unwrap();
    migrate(&mut db).unwrap();
    for table in ["symbols", "edges", "claim_code_edges"] {
        let query = format!("SELECT \"notnull\" FROM pragma_table_info('{table}') WHERE name='confidence'");
        let nullable: i64 = db.query_row(&query, [], |r| r.get(0)).unwrap();
        assert_eq!(nullable, 0, "{table}.confidence must be nullable");
    }
    assert_eq!(schema_version(&db).unwrap(), 20);
}

#[test]
fn oversized_schema_version_is_rejected_without_wrapping() {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch("CREATE TABLE meta(key TEXT PRIMARY KEY,value TEXT NOT NULL); INSERT INTO meta VALUES('schema_version','4294967297');").unwrap();
    assert!(schema_version(&db).is_err());
}

#[test]
fn null_legacy_file_node_id_fails_closed_without_dropping_row() {
    let mut db = Connection::open_in_memory().unwrap();
    migrate_to(&mut db, 15).unwrap();
    db.execute("INSERT INTO files(path,provider,generation_id,node_id) VALUES ('legacy.ts','lexical','g',NULL)", []).unwrap();
    assert!(migrate_to(&mut db, 16).is_err());
    assert_eq!(schema_version(&db).unwrap(), 15);
    assert_eq!(db.query_row("SELECT COUNT(*) FROM files WHERE path='legacy.ts'", [], |row| row.get::<_, i64>(0)).unwrap(), 1);
}
