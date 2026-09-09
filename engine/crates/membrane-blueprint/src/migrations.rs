//! Schema-v20 SQLite migrations for Blueprint.
//!
//! This is a direct Rust port of `blueprint/src/graph/store-sqlite.mjs`.
//! Migration functions are intentionally store-local: they do not depend on
//! the evolving public contracts or identity modules.

use rusqlite::{Connection, OptionalExtension};
use thiserror::Error;
use serde_json::Value;
use unicode_normalization::UnicodeNormalization;

pub const SCHEMA_VERSION: u32 = 20;

#[derive(Debug, Error)]
pub enum MigrationError {
    #[error("sqlite migration failed: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("unsupported schema version {0}")]
    UnsupportedVersion(i64),
    #[error("invalid schema version: {0}")]
    InvalidVersion(String),
    #[error("migration {from}->{to} failed: {detail}")]
    Failed { from: u32, to: u32, detail: String },
}

pub fn schema_version(conn: &Connection) -> Result<u32, MigrationError> {
    let value: Option<String> = conn
        .query_row("SELECT value FROM meta WHERE key='schema_version'", [], |r| r.get(0))
        .optional()?;
    match value {
        None => Ok(0),
        Some(v) => v.parse::<i64>()
            .map_err(|_| MigrationError::InvalidVersion(v.clone()))
            .and_then(|n| {
                if n < 0 { Err(MigrationError::InvalidVersion(v)) }
                else if n > SCHEMA_VERSION as i64 { Err(MigrationError::UnsupportedVersion(n)) }
                else { Ok(n as u32) }
            }),
    }
}

/// Apply migrations atomically. Every individual version is committed only as
/// part of this transaction, so any failure rolls the database back exactly.
pub fn migrate(conn: &mut Connection) -> Result<u32, MigrationError> {
    migrate_to(conn, SCHEMA_VERSION)
}

pub fn migrate_to(conn: &mut Connection, target: u32) -> Result<u32, MigrationError> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);")?;
    let current = schema_version(conn)?;
    if target > SCHEMA_VERSION { return Err(MigrationError::UnsupportedVersion(target as i64)); }
    if current > target { return Err(MigrationError::UnsupportedVersion(current as i64)); }
    for version in current..target {
        let tx = conn.transaction()?;
        apply_one(&tx, version)?;
        tx.execute("INSERT INTO meta(key,value) VALUES('schema_version',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value", [((version + 1) as u32).to_string()])?;
        tx.commit()?;
    }
    Ok(target)
}

fn apply_one(db: &rusqlite::Transaction<'_>, version: u32) -> Result<(), MigrationError> {
    let sql = match version {
        0 => r#"CREATE TABLE IF NOT EXISTS files(path TEXT PRIMARY KEY, content_hash TEXT, language TEXT, provider TEXT, parse_status TEXT, error_node_count INTEGER, generation_id TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS idx_files_generation ON files(generation_id);
CREATE TABLE IF NOT EXISTS symbols(id TEXT PRIMARY KEY, kind TEXT NOT NULL, labels TEXT NOT NULL, name TEXT NOT NULL, qualified_name TEXT NOT NULL, path TEXT NOT NULL, confidence REAL NOT NULL, evidence TEXT NOT NULL, generation_id TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS idx_symbols_path ON symbols(path); CREATE INDEX IF NOT EXISTS idx_symbols_generation ON symbols(generation_id); CREATE INDEX IF NOT EXISTS idx_symbols_generation_name ON symbols(generation_id,name); CREATE INDEX IF NOT EXISTS idx_symbols_generation_qualified ON symbols(generation_id,qualified_name);
CREATE TABLE IF NOT EXISTS edges(id TEXT PRIMARY KEY, kind TEXT NOT NULL, source TEXT NOT NULL, target TEXT, confidence REAL NOT NULL, resolved INTEGER NOT NULL DEFAULT 1, specifier TEXT, evidence TEXT NOT NULL, generation_id TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS idx_edges_source ON edges(source); CREATE INDEX IF NOT EXISTS idx_edges_target ON edges(target); CREATE INDEX IF NOT EXISTS idx_edges_kind ON edges(kind); CREATE INDEX IF NOT EXISTS idx_edges_generation ON edges(generation_id);"#,
        1 => r#"CREATE TABLE IF NOT EXISTS vectors(node_id TEXT PRIMARY KEY, dim INTEGER NOT NULL, emb BLOB NOT NULL, model TEXT NOT NULL, generation_id TEXT NOT NULL); CREATE INDEX IF NOT EXISTS idx_vectors_generation ON vectors(generation_id); CREATE INDEX IF NOT EXISTS idx_vectors_model ON vectors(model);"#,
        2 => r#"ALTER TABLE files ADD COLUMN node_id TEXT; ALTER TABLE files ADD COLUMN labels TEXT; ALTER TABLE files ADD COLUMN name TEXT; ALTER TABLE files ADD COLUMN qualified_name TEXT; ALTER TABLE files ADD COLUMN confidence REAL; ALTER TABLE files ADD COLUMN evidence TEXT; ALTER TABLE files ADD COLUMN extra TEXT; ALTER TABLE symbols ADD COLUMN extra TEXT; ALTER TABLE edges ADD COLUMN confidence_tier TEXT; ALTER TABLE edges ADD COLUMN extra TEXT; CREATE INDEX IF NOT EXISTS idx_edges_tier ON edges(confidence_tier); CREATE TABLE IF NOT EXISTS generation(key TEXT PRIMARY KEY,value TEXT NOT NULL);"#,
        3 => r#"CREATE TABLE IF NOT EXISTS file_state(path TEXT PRIMARY KEY,content_digest TEXT NOT NULL,size INTEGER NOT NULL,mtime_ms REAL,file_identity TEXT,last_event_seq INTEGER,applied_clock INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS fact_owner(fact_id TEXT NOT NULL,fact_kind TEXT NOT NULL CHECK(fact_kind IN ('node','edge')),source_path TEXT NOT NULL,source_digest TEXT NOT NULL,provider_id TEXT NOT NULL,provider_version TEXT NOT NULL,freshness_domain TEXT NOT NULL CHECK(freshness_domain IN ('structural','doc','semantic')),fact_kind_detail TEXT,PRIMARY KEY(fact_id,fact_kind,provider_id)); CREATE INDEX IF NOT EXISTS idx_fact_owner_path ON fact_owner(source_path); CREATE INDEX IF NOT EXISTS idx_fact_owner_domain ON fact_owner(freshness_domain);
CREATE TABLE IF NOT EXISTS dependency_index(source_path TEXT NOT NULL,dependent_path TEXT NOT NULL,reason TEXT NOT NULL CHECK(reason IN ('import','call','route','schema','config','manifest')),PRIMARY KEY(source_path,dependent_path,reason)); CREATE INDEX IF NOT EXISTS idx_dep_source ON dependency_index(source_path); CREATE TABLE IF NOT EXISTS generation_leaf(path TEXT PRIMARY KEY,kind TEXT NOT NULL CHECK(kind IN ('file','dir')),digest TEXT NOT NULL);"#,
        4 => r#"CREATE TABLE IF NOT EXISTS watch_state(key TEXT PRIMARY KEY,value TEXT NOT NULL); CREATE TABLE IF NOT EXISTS event_journal(seq INTEGER PRIMARY KEY AUTOINCREMENT,observed_ms INTEGER NOT NULL,event_kind TEXT NOT NULL CHECK(event_kind IN ('create','modify','delete','rename')),path TEXT NOT NULL,rename_to TEXT,source_clock INTEGER NOT NULL,applied INTEGER NOT NULL DEFAULT 0 CHECK(applied IN (0,1,2)),applied_clock INTEGER,UNIQUE(observed_ms,event_kind,path,rename_to,source_clock)); CREATE INDEX IF NOT EXISTS idx_event_journal_applied ON event_journal(applied,seq);"#,
        5 => r#"CREATE TABLE IF NOT EXISTS generation_receipt(receipt_id TEXT PRIMARY KEY,created_ms INTEGER NOT NULL,repo_root TEXT NOT NULL,generation_id TEXT,source_clock INTEGER NOT NULL,applied_clock INTEGER NOT NULL,event_gap INTEGER NOT NULL,barrier_result TEXT NOT NULL CHECK(barrier_result IN ('caught_up','gap_blocked','timeout')),details_json TEXT NOT NULL); CREATE INDEX IF NOT EXISTS idx_generation_receipt_created ON generation_receipt(created_ms);"#,
        6 => "CREATE TABLE IF NOT EXISTS artifact_state(artifact TEXT PRIMARY KEY,generation_id TEXT NOT NULL,fingerprint TEXT NOT NULL,updated_ms INTEGER NOT NULL);",
        7 => "ALTER TABLE generation_leaf ADD COLUMN parent_path TEXT; ALTER TABLE generation_leaf ADD COLUMN name TEXT; CREATE INDEX IF NOT EXISTS idx_generation_leaf_parent ON generation_leaf(parent_path,name);",
        8 => "",
        9 | 10 => "CREATE TABLE IF NOT EXISTS symbol_terms(generation_id TEXT NOT NULL,token TEXT NOT NULL,symbol_id TEXT NOT NULL,PRIMARY KEY(generation_id,token,symbol_id)) WITHOUT ROWID; CREATE INDEX IF NOT EXISTS idx_symbol_terms_symbol ON symbol_terms(symbol_id);",
        11 => "ALTER TABLE fact_owner ADD COLUMN generation_id TEXT; ALTER TABLE fact_owner ADD COLUMN repo_root TEXT; CREATE INDEX IF NOT EXISTS idx_fact_owner_generation ON fact_owner(generation_id); CREATE INDEX IF NOT EXISTS idx_fact_owner_repo_root ON fact_owner(repo_root);",
        12 => "CREATE INDEX IF NOT EXISTS idx_edges_gen_source_kind ON edges(generation_id,source,kind); CREATE INDEX IF NOT EXISTS idx_edges_gen_target_kind ON edges(generation_id,target,kind);",
        13 => r#"CREATE TABLE IF NOT EXISTS documents(id TEXT PRIMARY KEY,path TEXT NOT NULL,content_hash TEXT,lifecycle_status TEXT,lifecycle_superseded_by TEXT,lifecycle_superseded_on TEXT,generated_at TEXT,generation_id TEXT NOT NULL); CREATE INDEX IF NOT EXISTS idx_documents_generation ON documents(generation_id); CREATE INDEX IF NOT EXISTS idx_documents_path ON documents(path);
CREATE TABLE IF NOT EXISTS claims(id TEXT PRIMARY KEY,document_id TEXT NOT NULL,source TEXT,line INTEGER,text TEXT NOT NULL,status TEXT,sha1 TEXT,generation_id TEXT NOT NULL,FOREIGN KEY(document_id) REFERENCES documents(id) ON DELETE CASCADE); CREATE INDEX IF NOT EXISTS idx_claims_generation ON claims(generation_id); CREATE INDEX IF NOT EXISTS idx_claims_document ON claims(document_id);
CREATE TABLE IF NOT EXISTS claim_code_edges(id TEXT PRIMARY KEY,claim_id TEXT NOT NULL,kind TEXT NOT NULL,source TEXT NOT NULL,target TEXT NOT NULL,confidence REAL NOT NULL,confidence_class TEXT,reason TEXT,evidence_doc_path TEXT,evidence_doc_line INTEGER,evidence_doc_sha1 TEXT,evidence_code_path TEXT,evidence_code_exists INTEGER,evidence_code_node_id TEXT,evidence_code_content_hash TEXT,generation_id TEXT NOT NULL,FOREIGN KEY(claim_id) REFERENCES claims(id) ON DELETE CASCADE); CREATE INDEX IF NOT EXISTS idx_claim_code_edges_generation ON claim_code_edges(generation_id); CREATE INDEX IF NOT EXISTS idx_claim_code_edges_claim ON claim_code_edges(claim_id); CREATE INDEX IF NOT EXISTS idx_claim_code_edges_kind ON claim_code_edges(kind);
CREATE TABLE IF NOT EXISTS document_supersession(id TEXT PRIMARY KEY,source_kind TEXT NOT NULL,source_doc TEXT,source_line INTEGER,source_text TEXT,source_external INTEGER NOT NULL DEFAULT 0,target_doc TEXT,target_external INTEGER NOT NULL DEFAULT 0,target_match TEXT,superseded_on TEXT,generation_id TEXT NOT NULL); CREATE INDEX IF NOT EXISTS idx_document_supersession_generation ON document_supersession(generation_id);"#,
        14 => "ALTER TABLE symbols ADD COLUMN node_ordinal INTEGER; UPDATE symbols SET node_ordinal=rowid; CREATE TABLE IF NOT EXISTS annotation_nodes(id TEXT PRIMARY KEY,node_ordinal INTEGER NOT NULL,payload TEXT NOT NULL,generation_id TEXT NOT NULL); CREATE INDEX IF NOT EXISTS idx_annotation_nodes_generation_ordinal ON annotation_nodes(generation_id,node_ordinal);",
        15 => "ALTER TABLE files ADD COLUMN node_ordinal INTEGER; CREATE TABLE provider_ranks(provider_id TEXT PRIMARY KEY,rank INTEGER NOT NULL UNIQUE); CREATE INDEX idx_files_node_ordinal ON files(node_ordinal); CREATE INDEX IF NOT EXISTS idx_fact_owner_provider_path ON fact_owner(provider_id,source_path);",
        16 => "CREATE TABLE node_provider(node_id TEXT PRIMARY KEY,provider_id TEXT NOT NULL,source_path TEXT NOT NULL,provider_version TEXT NOT NULL DEFAULT 'unknown'); CREATE INDEX idx_node_provider_provider_path ON node_provider(provider_id,source_path);",
        17 => "CREATE TABLE IF NOT EXISTS named_snapshot(name TEXT PRIMARY KEY,repo_root TEXT NOT NULL,generation_id TEXT NOT NULL,manifest_digest TEXT NOT NULL,identity_json TEXT NOT NULL,created_ms INTEGER NOT NULL); CREATE INDEX IF NOT EXISTS idx_named_snapshot_generation ON named_snapshot(generation_id);",
        18 | 19 => "",
        _ => return Err(MigrationError::Failed { from: version, to: version + 1, detail: "unknown migration".into() }),
    };
    if !sql.is_empty() { db.execute_batch(sql).map_err(|e| MigrationError::Failed { from: version, to: version + 1, detail: e.to_string() })?; }
    if version == 7 { backfill_leaf_parents(db)?; }
    if version == 8 { create_symbol_search(db)?; }
    if version == 15 { rebuild_dense_order(db)?; }
    if version == 16 { rebuild_node_provenance(db)?; }
    if version == 18 { make_nullable(db, "symbols", "symbols_nullable_confidence_v19")?; make_nullable(db, "edges", "edges_nullable_confidence_v19")?; }
    if version == 19 { make_nullable(db, "claim_code_edges", "claim_code_edges_nullable_confidence_v20")?; }
    Ok(())
}

fn backfill_leaf_parents(db: &rusqlite::Transaction<'_>) -> Result<(), MigrationError> {
    let mut q = db.prepare("SELECT path FROM generation_leaf")?;
    let paths: Vec<String> = q.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
    let mut put = db.prepare("UPDATE generation_leaf SET parent_path=?1,name=?2 WHERE path=?3")?;
    for path in paths { let normalized = path.replace('\\', "/"); let (parent, name) = normalized.rsplit_once('/').map(|(p,n)|(p.to_string(),n.to_string())).unwrap_or_else(|| (String::new(), normalized.clone())); put.execute(rusqlite::params![if normalized.is_empty(){None::<String>}else{Some(parent)}, name, path])?; }
    Ok(())
}

fn create_symbol_search(db: &rusqlite::Transaction<'_>) -> Result<(), MigrationError> {
    if db.execute_batch("CREATE VIRTUAL TABLE IF NOT EXISTS symbol_search USING fts5(id UNINDEXED,generation_id UNINDEXED,name,qualified_name,path)").is_err() {
        // JS authority deliberately falls back for every FTS failure: minimal
        // SQLite builds, malformed pre-existing virtual tables, and extensions
        // that reject the requested schema must remain readable.
        db.execute_batch("CREATE TABLE IF NOT EXISTS symbol_search(id TEXT NOT NULL,generation_id TEXT NOT NULL,name TEXT NOT NULL,qualified_name TEXT NOT NULL,path TEXT NOT NULL,PRIMARY KEY(id,generation_id)); CREATE INDEX IF NOT EXISTS idx_symbol_search_generation ON symbol_search(generation_id,id)")?;
    }
    Ok(())
}

fn rebuild_dense_order(db: &rusqlite::Transaction<'_>) -> Result<(), MigrationError> {
    let mut ordinal = 0i64;
    let mut providers = Vec::<String>::new();
    let mut file_statement = db.prepare("SELECT path,node_id,provider,extra FROM files")?;
    let mut files = file_statement.query_map([], |r| Ok((r.get::<_,String>(0)?,r.get::<_,Option<String>>(1)?,r.get::<_,Option<String>>(2)?,r.get::<_,Option<String>>(3)?)))?.collect::<Result<Vec<_>, _>>()?;
    files.sort_by(|a,b| compare_repo_paths(&a.0,&b.0));
    for (path,id,file_provider,extra) in files { let id=id.ok_or_else(||migration_failed(15,16,"duplicate_node_id:null"))?; let provider = provider_for_legacy(db,&id,extra.as_deref(),file_provider.as_deref())?.ok_or_else(|| migration_failed(15,16,format!("provider_rebuild_required:{id}")))?; if !providers.contains(&provider){providers.push(provider);} db.execute("UPDATE files SET node_ordinal=?1 WHERE path=?2", rusqlite::params![ordinal, path])?; ordinal += 1; }
    let mut non_file_statement = db.prepare("SELECT id,node_ordinal,extra,NULL AS payload FROM symbols UNION ALL SELECT id,node_ordinal,NULL AS extra,payload FROM annotation_nodes")?;
    let mut non_files = non_file_statement.query_map([], |r| Ok((r.get::<_,String>(0)?,r.get::<_,i64>(1)?,r.get::<_,Option<String>>(2)?,r.get::<_,Option<String>>(3)?)))?.collect::<Result<Vec<_>, _>>()?;
    non_files.sort_by(|a,b| a.1.cmp(&b.1).then_with(|| compare_repo_paths(&a.0,&b.0)));
    for (id,_,extra,payload) in non_files { let provider = provider_for_legacy(db,&id,payload.as_deref().or(extra.as_deref()),None)?.ok_or_else(|| migration_failed(15,16,format!("provider_rebuild_required:{id}")))?; if !providers.contains(&provider){providers.push(provider);} let table=if payload.is_some(){"annotation_nodes"}else{"symbols"}; db.execute(&format!("UPDATE {table} SET node_ordinal=?1 WHERE id=?2"), rusqlite::params![ordinal,id])?; ordinal += 1; }
    for (rank, provider) in providers.iter().enumerate() { db.execute("INSERT OR IGNORE INTO provider_ranks(provider_id,rank) VALUES(?1,?2)", rusqlite::params![provider, rank as i64])?; }
    Ok(())
}

fn rebuild_node_provenance(db: &rusqlite::Transaction<'_>) -> Result<(), MigrationError> {
    let generation_provider = generation_provider(db)?;
    let mut old_ranks=std::collections::HashMap::<String,i64>::new();
    let mut rank_statement=db.prepare("SELECT provider_id,rank FROM provider_ranks ORDER BY rank,provider_id")?;
    let rank_rows=rank_statement.query_map([],|r|Ok((canonical_provider(&r.get::<_,String>(0)?),r.get::<_,i64>(1)?)))?.collect::<Result<Vec<_>,_>>()?;
    for (p,r) in rank_rows { old_ranks.entry(p).and_modify(|v|*v=(*v).min(r)).or_insert(r); }
    let mut structural_statement=db.prepare("SELECT fact_id,fact_kind,source_path,source_digest,provider_id,provider_version,freshness_domain,fact_kind_detail,generation_id,repo_root FROM fact_owner WHERE freshness_domain='structural' ORDER BY rowid")?;
    let structural = structural_statement.query_map([],|r|Ok(Owner{fact_id:r.get(0)?,fact_kind:r.get(1)?,source_path:r.get(2)?,source_digest:r.get(3)?,provider_id:canonical_provider(&r.get::<_,String>(4)?),provider_version:r.get(5)?,freshness_domain:r.get(6)?,fact_kind_detail:r.get(7)?,generation_id:r.get(8)?,repo_root:r.get(9)?}))?.collect::<Result<Vec<_>,_>>()?;
    let mut owner_by_fact=std::collections::HashMap::<String,Vec<Owner>>::new(); for owner in &structural { let list=owner_by_fact.entry(owner.fact_id.clone()).or_default(); if !list.iter().any(|x|x.provider_id==owner.provider_id){list.push(owner.clone());} }
    let mut rows=legacy_rows(db)?; let mut seen=std::collections::HashSet::new(); let mut paths=std::collections::HashMap::new();
    for row in &mut rows { if !seen.insert(row.id.clone()){return Err(migration_failed(16,17,format!("duplicate_node_id:{}",row.id)));} let embedded=parse_json(row.payload.as_deref()); row.path=normalize_path(if !row.path.is_empty(){&row.path}else{embedded.get("path").and_then(Value::as_str).or_else(||owner_by_fact.get(&row.id).and_then(|v|v.first()).map(|o|o.source_path.as_str())).unwrap_or("")}); if row.table=="files" {if let Some(old)=paths.insert(row.path.clone(),row.id.clone()){if old!=row.id{return Err(migration_failed(16,17,format!("path_normalization_collision:{}",row.path)));}}} let owners=owner_by_fact.get(&row.id); let owner=owners.and_then(|os|os.iter().min_by(|a,b| old_ranks.get(&a.provider_id).unwrap_or(&i64::MAX).cmp(old_ranks.get(&b.provider_id).unwrap_or(&i64::MAX)).then_with(||compare_repo_paths(&a.provider_id,&b.provider_id)))); let claim=embedded.get("factProvider").or_else(||embedded.get("provider")); let claimed=claim.and_then(|v|v.as_str().or_else(||v.get("id").and_then(Value::as_str))); row.provider_id=canonical_provider(owner.map(|o|o.provider_id.as_str()).or(claimed).or(row.file_provider.as_deref()).or(generation_provider.as_deref()).unwrap_or("")); row.provider_version=owner.map(|o|o.provider_version.clone()).or_else(||claim.and_then(|v|v.get("version").and_then(Value::as_str).map(str::to_owned))).unwrap_or_else(||"unknown".into()); if row.provider_id.is_empty(){return Err(migration_failed(16,17,format!("provider_rebuild_required:{}",row.id)));} old_ranks.entry(row.provider_id.clone()).or_insert(i64::MAX); }
    let mut providers=vec!["lexical".into(),"treesitter".into(),"doctruth".into()]; let mut custom:Vec<_>=old_ranks.into_iter().filter(|(p,_)|!providers.contains(p)).collect(); custom.sort_by(|a,b|a.1.cmp(&b.1).then_with(||compare_repo_paths(&a.0,&b.0))); providers.extend(custom.into_iter().map(|x|x.0));
    db.execute_batch("DELETE FROM provider_ranks; DELETE FROM fact_owner WHERE freshness_domain='structural';")?; for (rank,p) in providers.iter().enumerate(){db.execute("INSERT INTO provider_ranks(provider_id,rank) VALUES(?1,?2)",rusqlite::params![p,rank as i64])?;}
    for o in structural.iter().filter(|o|o.fact_kind=="edge"){db.execute("INSERT OR IGNORE INTO fact_owner(fact_id,fact_kind,source_path,source_digest,provider_id,provider_version,freshness_domain,fact_kind_detail,generation_id,repo_root) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",rusqlite::params![o.fact_id,o.fact_kind,normalize_path(&o.source_path),o.source_digest,o.provider_id,o.provider_version,o.freshness_domain,o.fact_kind_detail,o.generation_id,o.repo_root])?;}
    let mut counts=std::collections::HashMap::new(); for r in &rows{if r.ordinal>=0{*counts.entry(r.ordinal).or_insert(0usize)+=1;}} let corrupt=rows.iter().any(|r|r.ordinal<0||counts.get(&r.ordinal)!=Some(&1)); if corrupt{rows.sort_by(|a,b|((b.table=="files") as i32).cmp(&((a.table=="files") as i32)).then_with(||providers.iter().position(|p|p==&a.provider_id).unwrap_or(usize::MAX).cmp(&providers.iter().position(|p|p==&b.provider_id).unwrap_or(usize::MAX))).then_with(||compare_repo_paths(&a.path,&b.path)).then_with(||compare_repo_paths(&a.table,&b.table)).then_with(||compare_repo_paths(&a.id,&b.id)));}else{rows.sort_by(|a,b|a.ordinal.cmp(&b.ordinal));}
    db.execute("DELETE FROM node_provider",[])?; for (ordinal,row) in rows.iter().enumerate(){match row.table.as_str(){"files"=>db.execute("UPDATE files SET node_ordinal=?1,path=?2 WHERE node_id=?3",rusqlite::params![ordinal as i64,row.path,row.id])?,"symbols"=>db.execute("UPDATE symbols SET node_ordinal=?1,path=?2 WHERE id=?3",rusqlite::params![ordinal as i64,row.path,row.id])?,_=>db.execute("UPDATE annotation_nodes SET node_ordinal=?1 WHERE id=?2",rusqlite::params![ordinal as i64,row.id])?}; db.execute("INSERT INTO node_provider(node_id,provider_id,source_path,provider_version) VALUES(?1,?2,?3,?4)",rusqlite::params![row.id,row.provider_id,row.path,row.provider_version])?;}
    let file_digest=rows.iter().filter(|r|r.table=="files").map(|r|(r.path.clone(),normalize_digest(r.content_hash.as_deref().unwrap_or("unknown")))).collect::<std::collections::HashMap<_,_>>(); let manifest=generation_value(db,"manifest").and_then(|v|parse_json(Some(&v)).get("generationId").and_then(Value::as_str).map(str::to_owned)); let repo=generation_value(db,"repoRoot").map(|v|parse_json(Some(&v)).as_str().map(str::to_owned).unwrap_or(v)); for r in &rows{db.execute("INSERT OR IGNORE INTO fact_owner(fact_id,fact_kind,source_path,source_digest,provider_id,provider_version,freshness_domain,fact_kind_detail,generation_id,repo_root) VALUES(?1,'node',?2,?3,?4,?5,'structural',?6,?7,?8)",rusqlite::params![r.id,r.path,file_digest.get(&r.path).cloned().unwrap_or_else(||normalize_digest("unknown")),r.provider_id,r.provider_version,r.kind,manifest,repo])?;} if db.query_row("SELECT COUNT(*) FROM node_provider",[],|r|r.get::<_,i64>(0))? != rows.len() as i64{return Err(migration_failed(16,17,"provider_rebuild_required:cardinality"));}
    Ok(())
}

#[derive(Clone)] struct Owner { fact_id:String,fact_kind:String,source_path:String,source_digest:String,provider_id:String,provider_version:String,freshness_domain:String,fact_kind_detail:Option<String>,generation_id:Option<String>,repo_root:Option<String> }
#[derive(Clone)] struct LegacyRow { table:String,id:String,path:String,ordinal:i64,payload:Option<String>,file_provider:Option<String>,provider_id:String,provider_version:String,kind:String,content_hash:Option<String> }
fn migration_failed(from:u32,to:u32,detail:impl Into<String>)->MigrationError{MigrationError::Failed{from,to,detail:detail.into()}}
fn parse_json(value:Option<&str>)->Value{value.and_then(|v|serde_json::from_str(v).ok()).unwrap_or(Value::Object(Default::default()))}
fn generation_value(db:&rusqlite::Transaction<'_>,key:&str)->Option<String>{db.query_row("SELECT value FROM generation WHERE key=?1",[key],|r|r.get(0)).optional().ok().flatten()}
fn generation_provider(db:&rusqlite::Transaction<'_>)->Result<Option<String>,MigrationError>{Ok(generation_value(db,"provider").and_then(|v|{let x=parse_json(Some(&v));Some(canonical_provider(x.as_str().unwrap_or_else(||x.get("id").and_then(Value::as_str).unwrap_or(""))))}))}
fn provider_for_legacy(db:&rusqlite::Transaction<'_>,id:&str,payload:Option<&str>,fallback:Option<&str>)->Result<Option<String>,MigrationError>{let mut statement=db.prepare("SELECT DISTINCT provider_id FROM fact_owner WHERE fact_id=?1 AND fact_kind='node' ORDER BY provider_id")?; let owners:Vec<String>=statement.query_map([id],|r|r.get(0))?.collect::<Result<_,_>>()?; if owners.len()>1{return Err(migration_failed(15,16,format!("duplicate_provider_claim:{id}")));} Ok(owners.into_iter().next().or_else(||{let x=parse_json(payload);x.get("factProvider").or_else(||x.get("provider")).and_then(|v|v.as_str().or_else(||v.get("id").and_then(Value::as_str))).map(str::to_owned)}).or_else(||fallback.map(str::to_owned))) }
fn legacy_rows(db:&rusqlite::Transaction<'_>)->Result<Vec<LegacyRow>,MigrationError>{
    let mut out=Vec::new();
    let mut file_statement=db.prepare("SELECT node_id,path,node_ordinal,extra,provider,content_hash FROM files")?;
    let file_rows=file_statement.query_map([],|r|Ok((r.get::<_,Option<String>>(0)?,r.get::<_,String>(1)?,r.get::<_,Option<i64>>(2)?,r.get::<_,Option<String>>(3)?,r.get::<_,Option<String>>(4)?,r.get::<_,Option<String>>(5)?)))?.collect::<Result<Vec<_>,_>>()?;
    for (id,path,ordinal,payload,file_provider,content_hash) in file_rows {
        let id=id.ok_or_else(||migration_failed(16,17,"duplicate_node_id:null"))?;
        if id.is_empty(){return Err(migration_failed(16,17,"duplicate_node_id:null"));}
        out.push(LegacyRow{table:"files".into(),id,path,ordinal:ordinal.unwrap_or(-1),payload,file_provider,provider_id:String::new(),provider_version:"unknown".into(),kind:"file".into(),content_hash});
    }
    let mut symbol_statement=db.prepare("SELECT id,path,node_ordinal,extra,kind FROM symbols")?;
    let symbol_rows=symbol_statement.query_map([],|r|Ok(LegacyRow{table:"symbols".into(),id:r.get(0)?,path:r.get(1)?,ordinal:r.get::<_,Option<i64>>(2)?.unwrap_or(-1),payload:r.get(3)?,file_provider:None,provider_id:String::new(),provider_version:"unknown".into(),kind:r.get(4)?,content_hash:None}))?.collect::<Result<Vec<_>,_>>()?;
    out.extend(symbol_rows);
    let mut annotation_statement=db.prepare("SELECT id,'' AS path,node_ordinal,payload FROM annotation_nodes")?;
    let annotation_rows=annotation_statement.query_map([],|r|Ok(LegacyRow{table:"annotation_nodes".into(),id:r.get(0)?,path:String::new(),ordinal:r.get::<_,Option<i64>>(2)?.unwrap_or(-1),payload:r.get(3)?,file_provider:None,provider_id:String::new(),provider_version:"unknown".into(),kind:"comment".into(),content_hash:None}))?.collect::<Result<Vec<_>,_>>()?;
    out.extend(annotation_rows);
    Ok(out)
}
fn normalize_digest(value:&str)->String{if value.starts_with("xxh128:"){value.into()}else{format!("xxh128:{value}")}}
fn normalize_path(path: &str) -> String { let mut value=path.replace('\\', "/"); while value.starts_with("./") { value=value[2..].to_string(); } value.nfc().collect() }
fn compare_repo_paths(a:&str,b:&str)->std::cmp::Ordering{normalize_path(a).as_bytes().cmp(normalize_path(b).as_bytes()).then_with(||a.replace('\\',"/").trim_start_matches("./").as_bytes().cmp(b.replace('\\',"/").trim_start_matches("./").as_bytes()))}
fn canonical_provider(provider: &str) -> String { match provider { "blueprint-static"|"lexical"=>"lexical", "blueprint-treesitter"|"treesitter"=>"treesitter", "blueprint-scip"|"scip"=>"scip", other=>other }.to_string() }

fn make_nullable(db: &rusqlite::Transaction<'_>, table: &str, temp: &str) -> Result<(), MigrationError> {
    let sql: String = db.query_row("SELECT sql FROM sqlite_master WHERE type='table' AND name=?1", [table], |r| r.get(0))?;
    let info = {
        let mut statement = db.prepare(&format!("PRAGMA table_info(\"{table}\")"))?;
        let rows = statement
            .query_map([], |r| Ok((r.get::<_, String>(1)?, r.get::<_, i64>(3)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    let notnull = info.iter().find(|(n, _)| n == "confidence").map(|(_, n)| *n).unwrap_or(0);
    if notnull == 0 { return Ok(()); }
    let nullable = sql.replacen("confidence REAL NOT NULL", "confidence REAL", 1);
    if nullable == sql { return Err(MigrationError::Failed { from: 19, to: 20, detail: format!("unrecognized {table} definition") }); }
    let create = nullable.replacen(table, temp, 1);
    let mut dependent_statement = db.prepare("SELECT sql FROM sqlite_master WHERE tbl_name=?1 AND type IN ('index','trigger') AND sql IS NOT NULL ORDER BY type,name")?;
    let dependents: Vec<String> = dependent_statement.query_map([table], |r| r.get(0))?.collect::<Result<_, _>>()?;
    db.execute_batch(&create)?;
    let cols = info.iter().map(|(n, _)| format!("\"{n}\"")).collect::<Vec<_>>().join(",");
    db.execute_batch(&format!("INSERT INTO \"{temp}\" (rowid,{cols}) SELECT rowid,{cols} FROM \"{table}\" ORDER BY rowid; DROP TABLE \"{table}\"; ALTER TABLE \"{temp}\" RENAME TO \"{table}\";"))?;
    for dependent in dependents { db.execute_batch(&dependent)?; }
    Ok(())
}
