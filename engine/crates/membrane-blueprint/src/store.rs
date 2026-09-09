//! SQLite store boundary for Blueprint's generation-shaped graph data.

use crate::migrations::{migrate, schema_version, MigrationError, SCHEMA_VERSION};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Migration(#[from] MigrationError),
    #[error("path error: {0}")]
    Path(String),
    #[error("generation JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid generation: {0}")]
    InvalidGeneration(String),
    #[error("generation not found")]
    GenerationNotFound,
    #[error("generation mismatch: expected {expected}, observed {observed}")]
    GenerationMismatch { expected: String, observed: String },
}

/// The persisted generation envelope.  Envelope values remain JSON because
/// providers intentionally own their open metadata maps; the envelope shape,
/// transaction boundary, and generation identity stay typed here.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GenerationEnvelope {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema_version: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manifest: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_root: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub augmentation: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_observation: Option<Value>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl GenerationEnvelope {
    pub fn generation_id(&self) -> Option<&str> {
        self.manifest
            .as_ref()
            .and_then(|value| value.get("generationId").or_else(|| value.get("generation_id")))
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
    }
}

/// A complete graph body plus its typed envelope.  Nodes and edges are JSON
/// values by design: provider extensions must survive a Rust round trip,
/// including explicit null confidence/evidence and unknown future fields.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Generation {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema_version: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manifest: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_root: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub augmentation: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_observation: Option<Value>,
    #[serde(default)]
    pub nodes: Vec<Value>,
    #[serde(default)]
    pub edges: Vec<Value>,
    #[serde(default)]
    pub file_reports: Vec<Value>,
    /// Optional relational document projections. `None` means caller did not
    /// request replacement, while `Some([])` explicitly clears that family.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub documents: Option<Vec<Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claims: Option<Vec<Value>>,
    #[serde(default, rename = "claimCodeEdges", skip_serializing_if = "Option::is_none")]
    pub claim_code_edges: Option<Vec<Value>>,
    #[serde(default, rename = "documentSupersession", skip_serializing_if = "Option::is_none")]
    pub document_supersession: Option<Vec<Value>>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl PartialEq for Generation {
    fn eq(&self, other: &Self) -> bool {
        self.schema_version == other.schema_version
            && self.provider == other.provider
            && self.manifest == other.manifest
            && self.repo_root == other.repo_root
            && self.augmentation == other.augmentation
            && self.source_observation == other.source_observation
            && self.nodes == other.nodes
            && self.edges == other.edges
            && self.file_reports == other.file_reports
            && self.documents == other.documents
            && self.claims == other.claims
            && self.claim_code_edges == other.claim_code_edges
            && self.document_supersession == other.document_supersession
            && self.extra == other.extra
    }
}

impl Generation {
    pub fn envelope(&self) -> GenerationEnvelope {
        GenerationEnvelope {
            schema_version: self.schema_version,
            provider: self.provider.clone(),
            manifest: self.manifest.clone(),
            repo_root: self.repo_root.clone(),
            augmentation: self.augmentation.clone(),
            source_observation: self.source_observation.clone(),
        extra: self.extra.clone(),
        }
    }

    pub fn generation_id(&self) -> Option<&str> {
        self.manifest
            .as_ref()
            .and_then(|value| value.get("generationId").or_else(|| value.get("generation_id")))
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepairOutcome {
    NoBackup,
    Restored { from_version: u32 },
}

/// Open a writable store, taking one exact pre-migration safety copy when an
/// existing store is being upgraded. A missing or failed backup is fatal.
pub fn open_store(path: Option<&Path>) -> Result<Connection, StoreError> {
    if let Some(path) = path {
        if path.exists() {
            let probe = Connection::open(path)?;
            let initial_version = existing_schema_version(&probe)?;
            if initial_version > 0 && initial_version < SCHEMA_VERSION {
                let backup = migration_backup_path(path, initial_version);
                if !backup.exists() {
                    // WAL contents must be in the main file before copying it.
                    probe.pragma_update(None, "journal_mode", "WAL")?;
                    probe.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")?;
                    drop(probe);
                    if let Some(parent) = backup.parent() {
                        fs::create_dir_all(parent)
                            .map_err(|error| StoreError::Path(error.to_string()))?;
                    }
                    fs::copy(path, &backup)
                        .map_err(|error| StoreError::Path(error.to_string()))?;
                }
            }
        }
    }

    let mut conn = match path {
        Some(path) => Connection::open(path)?,
        None => Connection::open_in_memory()?,
    };
    conn.pragma_update(None, "foreign_keys", "ON")?;
    if path.is_some() {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.busy_timeout(Duration::from_secs(30))?;
    }
    migrate(&mut conn)?;
    Ok(conn)
}

/// Open a read-only handle without changing schema or running migrations.
pub fn open_store_read_only(path: &Path) -> Result<Connection, StoreError> {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    conn.busy_timeout(Duration::from_secs(5))?;
    conn.pragma_update(None, "mmap_size", 268_435_456i64)?;
    conn.pragma_update(None, "cache_size", -65_536i64)?;
    Ok(conn)
}

pub fn current_schema_version(conn: &Connection) -> Result<u32, StoreError> {
    Ok(schema_version(conn)?)
}

/// Exact backup location for a specific schema version.
pub fn migration_backup_path(path: &Path, version: u32) -> PathBuf {
    path.parent()
        .unwrap_or_else(|| Path::new("."))
        .join("backups")
        .join(format!("graph.db.before-migrate-v{version}"))
}

/// Restore the exact from-version backup and re-run all migrations to current.
pub fn repair_interrupted_migration(
    path: &Path,
    from_version: u32,
) -> Result<RepairOutcome, StoreError> {
    let backup = migration_backup_path(path, from_version);
    if !backup.exists() {
        return Ok(RepairOutcome::NoBackup);
    }

    for suffix in ["", "-wal", "-shm"] {
        let target = PathBuf::from(format!("{}{}", path.to_string_lossy(), suffix));
        if target.exists() {
            fs::remove_file(target).map_err(|error| StoreError::Path(error.to_string()))?;
        }
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| StoreError::Path(error.to_string()))?;
    }
    fs::copy(&backup, path).map_err(|error| StoreError::Path(error.to_string()))?;

    let mut conn = Connection::open(path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    migrate(&mut conn)?;
    drop(conn);
    Ok(RepairOutcome::Restored { from_version })
}

fn existing_schema_version(conn: &Connection) -> Result<u32, StoreError> {
    let has_meta: Option<i64> = conn.query_row(
        "SELECT 1 FROM sqlite_master WHERE type='table' AND name='meta'",
        [],
        |row| row.get(0),
    ).optional()?;
    if has_meta.is_none() {
        return Ok(0);
    }
    Ok(schema_version(conn)?)
}

pub const LATEST_SCHEMA_VERSION: u32 = SCHEMA_VERSION;

/// Persist one complete generation in one SQLite transaction.  Existing graph
/// rows are replaced only when the envelope replacement can commit too.
pub fn save_generation(conn: &mut Connection, generation: &Generation) -> Result<(), StoreError> {
    let generation_id = generation
        .generation_id()
        .ok_or_else(|| StoreError::InvalidGeneration("manifest.generationId is required".into()))?
        .to_owned();
    validate_generation(generation, &generation_id)?;

    let tx = conn.transaction()?;
    clear_generation_rows(&tx)?;
    let file_reports = file_reports_by_path(&generation.file_reports)?;
    let mut provider_ranks = HashMap::<String, i64>::new();
    for (ordinal, node) in generation.nodes.iter().enumerate() {
        insert_node(
            &tx,
            node,
            ordinal as i64,
            &generation_id,
            file_reports.get(node_path(node).as_deref().unwrap_or_default()),
            generation.provider.as_ref(),
            &mut provider_ranks,
        )?;
    }
    for edge in &generation.edges {
        insert_edge(&tx, edge, &generation_id)?;
    }
    replace_document_rows(&tx, generation, &generation_id)?;
    write_envelope(&tx, generation)?;
    tx.commit()?;
    Ok(())
}

/// Read the full persisted generation, or `None` when no envelope is sealed.
pub fn load_generation(conn: &Connection) -> Result<Option<Generation>, StoreError> {
    let envelope = read_generation_envelope(conn)?;
    if envelope.is_none() {
        return Ok(None);
    }
    let mut generation = generation_from_envelope(envelope.unwrap())?;
    generation.nodes = load_nodes(conn)?;
    generation.edges = load_edges(conn)?;
    generation.file_reports = load_file_reports(conn)?;
    load_document_rows(conn, &mut generation)?;
    Ok(Some(generation))
}

/// Load a generation only when its persisted manifest identity equals `expected`.
pub fn load_generation_pinned(conn: &Connection, expected: &str) -> Result<Generation, StoreError> {
    let generation = load_generation(conn)?.ok_or(StoreError::GenerationNotFound)?;
    let observed = generation
        .generation_id()
        .ok_or(StoreError::GenerationNotFound)?
        .to_owned();
    if observed != expected {
        return Err(StoreError::GenerationMismatch {
            expected: expected.to_owned(),
            observed,
        });
    }
    Ok(generation)
}

/// Read only the typed envelope without materialising graph rows.
pub fn read_generation_envelope(conn: &Connection) -> Result<Option<GenerationEnvelope>, StoreError> {
    let mut statement = conn.prepare("SELECT key, value FROM generation ORDER BY rowid")?;
    let mut rows = statement.query([])?;
    let mut envelope = GenerationEnvelope::default();
    let mut found = false;
    while let Some(row) = rows.next()? {
        found = true;
        let key: String = row.get(0)?;
        let value: Value = serde_json::from_str(&row.get::<_, String>(1)?)?;
        match key.as_str() {
            "schemaVersion" => {
                let version = value.as_u64().filter(|n| *n <= u32::MAX as u64).ok_or_else(|| {
                    StoreError::InvalidGeneration("generation.schemaVersion must be a u32".into())
                })?;
                envelope.schema_version = Some(version as u32);
            }
            "provider" => envelope.provider = Some(value),
            "manifest" => envelope.manifest = Some(value),
            "repoRoot" => envelope.repo_root = Some(value),
            "augmentation" => envelope.augmentation = Some(value),
            "sourceObservation" => envelope.source_observation = Some(value),
            _ => {
                envelope.extra.insert(key, value);
            }
        }
    }
    Ok(found.then_some(envelope))
}

fn validate_generation(generation: &Generation, generation_id: &str) -> Result<(), StoreError> {
    let mut ids = HashSet::new();
    let mut paths = HashMap::new();
    for node in &generation.nodes {
        let object = node_object(node, "node")?;
        let id = required_string(object, "id", "node")?;
        if !ids.insert(id.to_owned()) {
            return Err(StoreError::InvalidGeneration(format!("duplicate node id: {id}")));
        }
        if let Some(row_generation) = object.get("generationId").and_then(Value::as_str) {
            if row_generation != generation_id {
                return Err(StoreError::GenerationMismatch {
                    expected: generation_id.to_owned(),
                    observed: row_generation.to_owned(),
                });
            }
        }
        if object.get("kind").and_then(Value::as_str) == Some("file") {
            let path = required_string(object, "path", "file node")?;
            let normalized = normalize_repo_path(path);
            if let Some(prior) = paths.insert(normalized.clone(), id.to_owned()) {
                return Err(StoreError::InvalidGeneration(format!(
                    "path normalization collision: {normalized} ({prior}, {id})"
                )));
            }
        }
    }
    for edge in &generation.edges {
        let object = node_object(edge, "edge")?;
        required_string(object, "id", "edge")?;
        required_string(object, "kind", "edge")?;
        required_string(object, "source", "edge")?;
        if let Some(row_generation) = object.get("generationId").and_then(Value::as_str) {
            if row_generation != generation_id {
                return Err(StoreError::GenerationMismatch {
                    expected: generation_id.to_owned(),
                    observed: row_generation.to_owned(),
                });
            }
        }
    }
    Ok(())
}

fn clear_generation_rows(tx: &Transaction<'_>) -> Result<(), StoreError> {
    // Keep watch/freshness state and migration metadata. These are the body
    // projections replaced by a new published generation.
    tx.execute_batch(
        "DELETE FROM files; DELETE FROM symbols;
         DELETE FROM annotation_nodes; DELETE FROM edges; DELETE FROM vectors;
         DELETE FROM symbol_search; DELETE FROM symbol_terms; DELETE FROM node_provider;
         DELETE FROM provider_ranks;",
    )?;
    Ok(())
}

fn replace_document_rows(tx: &Transaction<'_>, generation: &Generation, generation_id: &str) -> Result<(), StoreError> {
    if generation.documents.is_none()
        && generation.claims.is_none()
        && generation.claim_code_edges.is_none()
        && generation.document_supersession.is_none()
    {
        return Ok(());
    }
    // Children must be removed before documents while FK enforcement is on.
    tx.execute_batch("DELETE FROM claim_code_edges; DELETE FROM claims; DELETE FROM documents; DELETE FROM document_supersession;")?;
    if let Some(rows) = &generation.documents {
        for row in rows {
            let object = node_object(row, "document")?;
            tx.execute(
                "INSERT INTO documents(id,path,content_hash,lifecycle_status,lifecycle_superseded_by,lifecycle_superseded_on,generated_at,generation_id) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
                params![
                    required_string(object, "id", "document")?,
                    required_string(object, "path", "document")?,
                    optional_json_string(object, "contentHash", "document")?,
                    optional_json_string(object, "lifecycleStatus", "document")?,
                    optional_json_string(object, "lifecycleSupersededBy", "document")?,
                    optional_json_string(object, "lifecycleSupersededOn", "document")?,
                    optional_json_string(object, "generatedAt", "document")?,
                    row_generation(object, generation_id)?,
                ],
            )?;
        }
    }
    if let Some(rows) = &generation.claims {
        for row in rows {
            let object = node_object(row, "claim")?;
            tx.execute(
                "INSERT INTO claims(id,document_id,source,line,text,status,sha1,generation_id) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
                params![
                    required_string(object, "id", "claim")?,
                    required_string(object, "documentId", "claim")?,
                    optional_json_string(object, "source", "claim")?,
                    optional_i64(object, "line", "claim")?,
                    required_string(object, "text", "claim")?,
                    optional_json_string(object, "status", "claim")?,
                    optional_json_string(object, "sha1", "claim")?,
                    row_generation(object, generation_id)?,
                ],
            )?;
        }
    }
    if let Some(rows) = &generation.claim_code_edges {
        for row in rows { insert_claim_code_edge(tx, node_object(row, "claim code edge")?, generation_id)?; }
    }
    if let Some(rows) = &generation.document_supersession {
        for row in rows { insert_document_supersession(tx, node_object(row, "document supersession")?, generation_id)?; }
    }
    Ok(())
}

fn insert_claim_code_edge(tx: &Transaction<'_>, object: &Map<String, Value>, generation_id: &str) -> Result<(), StoreError> {
    let evidence = object.get("evidence").and_then(Value::as_object);
    let doc_ref = evidence.and_then(|value| value.get("docRef")).and_then(Value::as_object);
    let code_ref = evidence.and_then(|value| value.get("codeRef")).and_then(Value::as_object);
    let code_node = evidence.and_then(|value| value.get("codeNode")).and_then(Value::as_object);
    tx.execute(
        "INSERT INTO claim_code_edges(id,claim_id,kind,source,target,confidence,confidence_class,reason,evidence_doc_path,evidence_doc_line,evidence_doc_sha1,evidence_code_path,evidence_code_exists,evidence_code_node_id,evidence_code_content_hash,generation_id) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)",
        params![
            required_string(object, "id", "claim code edge")?,
            required_string(object, "claimId", "claim code edge")?,
            required_string(object, "kind", "claim code edge")?,
            required_string(object, "source", "claim code edge")?,
            required_string(object, "target", "claim code edge")?,
            optional_f64(object.get("confidence"), "claimCodeEdge.confidence")?,
            optional_json_string(object, "confidenceClass", "claim code edge")?,
            optional_json_string(object, "reason", "claim code edge")?,
            doc_ref.and_then(|value| value.get("path")).and_then(Value::as_str).or_else(|| object.get("evidenceDocPath").and_then(Value::as_str)),
            doc_ref.and_then(|value| value.get("line")).and_then(Value::as_i64).or_else(|| object.get("evidenceDocLine").and_then(Value::as_i64)),
            doc_ref.and_then(|value| value.get("sha1")).and_then(Value::as_str).or_else(|| object.get("evidenceDocSha1").and_then(Value::as_str)),
            code_ref.and_then(|value| value.get("path")).and_then(Value::as_str).or_else(|| object.get("evidenceCodePath").and_then(Value::as_str)),
            code_ref.and_then(|value| value.get("exists")).and_then(Value::as_bool).map(|value| if value { 1 } else { 0 }).or_else(|| object.get("evidenceCodeExists").and_then(Value::as_i64)),
            code_node.and_then(|value| value.get("id")).and_then(Value::as_str).or_else(|| object.get("evidenceCodeNodeId").and_then(Value::as_str)),
            code_node.and_then(|value| value.get("contentHash")).and_then(Value::as_str).or_else(|| object.get("evidenceCodeContentHash").and_then(Value::as_str)),
            row_generation(object, generation_id)?,
        ],
    )?;
    Ok(())
}

fn insert_document_supersession(tx: &Transaction<'_>, object: &Map<String, Value>, generation_id: &str) -> Result<(), StoreError> {
    tx.execute(
        "INSERT INTO document_supersession(id,source_kind,source_doc,source_line,source_text,source_external,target_doc,target_external,target_match,superseded_on,generation_id) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
        params![
            required_string(object, "id", "document supersession")?,
            required_string(object, "sourceKind", "document supersession")?,
            optional_json_string(object, "sourceDoc", "document supersession")?,
            optional_i64(object, "sourceLine", "document supersession")?,
            optional_json_string(object, "sourceText", "document supersession")?,
            if object.get("sourceExternal").and_then(Value::as_bool).unwrap_or(false) { 1 } else { 0 },
            optional_json_string(object, "targetDoc", "document supersession")?,
            if object.get("targetExternal").and_then(Value::as_bool).unwrap_or(false) { 1 } else { 0 },
            optional_json_string(object, "targetMatch", "document supersession")?,
            optional_json_string(object, "supersededOn", "document supersession")?,
            row_generation(object, generation_id)?,
        ],
    )?;
    Ok(())
}

fn write_envelope(tx: &Transaction<'_>, generation: &Generation) -> Result<(), StoreError> {
    let prior_marker: Option<String> = tx
        .query_row("SELECT value FROM generation WHERE key='__membraneDocumentRows'", [], |row| row.get(0))
        .optional()?;
    tx.execute("DELETE FROM generation", [])?;
    let mut values = Vec::<(&str, Value)>::new();
    if let Some(value) = generation.schema_version { values.push(("schemaVersion", Value::from(value))); }
    if let Some(value) = &generation.provider { values.push(("provider", value.clone())); }
    if let Some(value) = &generation.manifest { values.push(("manifest", value.clone())); }
    if let Some(value) = &generation.repo_root { values.push(("repoRoot", value.clone())); }
    if let Some(value) = &generation.augmentation { values.push(("augmentation", value.clone())); }
    if let Some(value) = &generation.source_observation { values.push(("sourceObservation", value.clone())); }
    for (key, value) in &generation.extra {
        if key == "__membraneDocumentRows" { continue; }
        values.push((key.as_str(), value.clone()));
    }
    if generation.documents.is_some()
        || generation.claims.is_some()
        || generation.claim_code_edges.is_some()
        || generation.document_supersession.is_some()
    {
        values.push(("__membraneDocumentRows", document_rows_marker(generation)?));
    } else if let Some(value) = prior_marker {
        values.push(("__membraneDocumentRows", serde_json::from_str(&value)?));
    }
    for (key, value) in values {
        tx.execute(
            "INSERT INTO generation(key,value) VALUES(?1,?2)",
            params![key, serde_json::to_string(&value)?],
        )?;
    }
    Ok(())
}

fn document_rows_marker(generation: &Generation) -> Result<Value, StoreError> {
    Ok(json_object([
        ("documents", generation.documents.clone().map(Value::Array)),
        ("claims", generation.claims.clone().map(Value::Array)),
        ("claimCodeEdges", generation.claim_code_edges.clone().map(Value::Array)),
        ("documentSupersession", generation.document_supersession.clone().map(Value::Array)),
    ]))
}

fn json_object<const N: usize>(pairs: [(&str, Option<Value>); N]) -> Value {
    let mut object = Map::new();
    for (key, value) in pairs { object.insert(key.to_owned(), value.unwrap_or(Value::Null)); }
    Value::Object(object)
}

fn insert_node(
    tx: &Transaction<'_>,
    node: &Value,
    ordinal: i64,
    generation_id: &str,
    report: Option<&Value>,
    generation_provider: Option<&Value>,
    provider_ranks: &mut HashMap<String, i64>,
) -> Result<(), StoreError> {
    let object = node_object(node, "node")?;
    let id = required_string(object, "id", "node")?;
    let kind = required_string(object, "kind", "node")?;
    let labels = json_field(object, "labels", Value::Array(Vec::new()));
    let evidence = json_field(object, "evidence", Value::Array(Vec::new()));
    let confidence = optional_f64(object.get("confidence"), "node.confidence")?;
    let mut extra = extra_json(object, &["id", "kind", "labels", "name", "qualifiedName", "path", "confidence", "evidence"])?;
    extra = add_absent_markers(extra, object, &["name", "qualifiedName", "confidence", "evidence"])?;
    if kind == "file" {
        let path = normalize_repo_path(required_string(object, "path", "file node")?);
        let report_object = report.and_then(Value::as_object);
        extra = add_file_report_extra(extra, report_object)?;
        let content_hash = report_object.and_then(|r| r.get("contentHash")).or_else(|| evidence.get(0).and_then(|v| v.get("contentHash")));
        tx.execute(
            "INSERT INTO files(path,content_hash,language,provider,parse_status,error_node_count,generation_id,node_id,labels,name,qualified_name,confidence,evidence,extra,node_ordinal) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
            params![
                path,
                optional_string_value(content_hash),
                report_object.and_then(|r| r.get("language")).and_then(Value::as_str),
                report_object.and_then(|r| r.get("provider")).and_then(Value::as_str),
                report_object.and_then(|r| r.get("parseStatus")).and_then(Value::as_str),
                report_object.and_then(|r| r.get("errorNodeCount")).and_then(Value::as_i64),
                generation_id, id,
                serde_json::to_string(&labels)?,
                nullable_string(object.get("name")),
                nullable_string(object.get("qualifiedName")),
                confidence,
                serde_json::to_string(&evidence)?,
                extra,
                ordinal,
            ],
        )?;
    } else if kind == "comment" {
        tx.execute(
            "INSERT INTO annotation_nodes(id,node_ordinal,payload,generation_id) VALUES(?1,?2,?3,?4)",
            params![id, ordinal, serde_json::to_string(node)?, generation_id],
        )?;
    } else {
        let name = required_string_or_empty(object, "name");
        let qualified_name = required_string_or_empty(object, "qualifiedName");
        let path = required_string_or_empty(object, "path");
        tx.execute(
            "INSERT INTO symbols(id,kind,labels,name,qualified_name,path,confidence,evidence,generation_id,extra,node_ordinal) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            params![id, kind, serde_json::to_string(&labels)?, name, qualified_name, path, confidence, serde_json::to_string(&evidence)?, generation_id, extra, ordinal],
        )?;
    }
    register_node_provider(tx, object, id, generation_provider, provider_ranks)?;
    Ok(())
}

fn insert_edge(tx: &Transaction<'_>, edge: &Value, generation_id: &str) -> Result<(), StoreError> {
    let object = node_object(edge, "edge")?;
    let id = required_string(object, "id", "edge")?;
    let kind = required_string(object, "kind", "edge")?;
    let source = required_string(object, "source", "edge")?;
    let target = optional_json_string(object, "target", "edge")?;
    let confidence = optional_f64(object.get("confidence"), "edge.confidence")?;
    let evidence = json_field(object, "evidence", Value::Array(Vec::new()));
    let confidence_tier = optional_json_string(object, "confidenceTier", "edge")?;
    let resolved = object.get("resolved").and_then(Value::as_bool).unwrap_or(true);
    let specifier = optional_json_string(object, "specifier", "edge")?;
    let mut extra = extra_json(object, &["id", "kind", "source", "target", "confidence", "confidenceTier", "evidence"])?;
    extra = add_absent_markers(extra, object, &["target", "confidence", "confidenceTier", "evidence", "resolved", "specifier", "generationId"])?;
    tx.execute(
        "INSERT INTO edges(id,kind,source,target,confidence,resolved,specifier,evidence,generation_id,confidence_tier,extra) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
        params![id, kind, source, target, confidence, if resolved { 1 } else { 0 }, specifier, serde_json::to_string(&evidence)?, generation_id, confidence_tier, extra],
    )?;
    Ok(())
}

fn load_nodes(conn: &Connection) -> Result<Vec<Value>, StoreError> {
    let mut ordered = Vec::<(i64, i64, Value)>::new();
    {
        let mut statement = conn.prepare("SELECT node_id,path,labels,name,qualified_name,confidence,evidence,extra,node_ordinal,rowid,generation_id FROM files")?;
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            let id: Option<String> = row.get(0)?;
            let path: String = row.get(1)?;
            let mut base = Map::new();
            base.insert("id".into(), Value::String(id.unwrap_or_else(|| format!("file:{path}"))));
            base.insert("kind".into(), Value::String("file".into()));
            base.insert("labels".into(), parse_json_text(row.get(2)?, Value::Array(Vec::new()))?);
            let extra: Option<String> = row.get(7)?;
            if !stored_field_absent(extra.as_ref(), "name")? { base.insert("name".into(), nullable_value(row.get(3)?)); }
            if !stored_field_absent(extra.as_ref(), "qualifiedName")? { base.insert("qualifiedName".into(), nullable_value(row.get(4)?)); }
            base.insert("path".into(), Value::String(path));
            if !stored_field_absent(extra.as_ref(), "confidence")? { base.insert("confidence".into(), nullable_f64(row.get(5)?)); }
            if !stored_field_absent(extra.as_ref(), "evidence")? { base.insert("evidence".into(), parse_json_text(row.get(6)?, Value::Array(Vec::new()))?); }
            base.insert("generationId".into(), Value::String(row.get(10)?));
            merge_extra_filtered(&mut base, extra, Some("__fileReport"))?;
            ordered.push((row.get::<_, Option<i64>>(8)?.unwrap_or(row.get(9)?), row.get(9)?, Value::Object(base)));
        }
    }
    {
        let mut statement = conn.prepare("SELECT id,kind,labels,name,qualified_name,path,confidence,evidence,extra,node_ordinal,rowid,generation_id FROM symbols")?;
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            let mut base = Map::new();
            base.insert("id".into(), Value::String(row.get(0)?));
            base.insert("kind".into(), Value::String(row.get(1)?));
            base.insert("labels".into(), parse_json_text(row.get(2)?, Value::Array(Vec::new()))?);
            let extra: Option<String> = row.get(8)?;
            if !stored_field_absent(extra.as_ref(), "name")? { base.insert("name".into(), Value::String(row.get::<_, String>(3)?)); }
            if !stored_field_absent(extra.as_ref(), "qualifiedName")? { base.insert("qualifiedName".into(), Value::String(row.get::<_, String>(4)?)); }
            base.insert("path".into(), Value::String(row.get::<_, String>(5)?));
            if !stored_field_absent(extra.as_ref(), "confidence")? { base.insert("confidence".into(), nullable_f64(row.get(6)?)); }
            if !stored_field_absent(extra.as_ref(), "evidence")? { base.insert("evidence".into(), parse_json_text(row.get(7)?, Value::Array(Vec::new()))?); }
            base.insert("generationId".into(), Value::String(row.get(11)?));
            merge_extra(&mut base, extra)?;
            ordered.push((row.get::<_, Option<i64>>(9)?.unwrap_or(row.get(10)?), row.get(10)?, Value::Object(base)));
        }
    }
    {
        let mut statement = conn.prepare("SELECT payload,node_ordinal,rowid FROM annotation_nodes")?;
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            ordered.push((row.get::<_, Option<i64>>(1)?.unwrap_or(row.get(2)?), row.get(2)?, serde_json::from_str(&row.get::<_, String>(0)?)?));
        }
    }
    ordered.sort_by_key(|(ordinal, rowid, _)| (*ordinal, *rowid));
    Ok(ordered.into_iter().map(|(_, _, value)| value).collect())
}

fn load_edges(conn: &Connection) -> Result<Vec<Value>, StoreError> {
        let mut statement = conn.prepare("SELECT id,kind,source,target,confidence,resolved,specifier,evidence,confidence_tier,extra,generation_id FROM edges ORDER BY rowid")?;
    let mut rows = statement.query([])?;
    let mut result = Vec::new();
    while let Some(row) = rows.next()? {
        let mut base = Map::new();
        base.insert("id".into(), Value::String(row.get(0)?));
        base.insert("kind".into(), Value::String(row.get(1)?));
        base.insert("source".into(), Value::String(row.get(2)?));
        let extra: Option<String> = row.get(9)?;
        if !stored_field_absent(extra.as_ref(), "target")? { base.insert("target".into(), nullable_value(row.get(3)?)); }
        if !stored_field_absent(extra.as_ref(), "confidence")? { base.insert("confidence".into(), nullable_f64(row.get(4)?)); }
        if !stored_field_absent(extra.as_ref(), "resolved")? { base.insert("resolved".into(), Value::Bool(row.get::<_, i64>(5)? != 0)); }
        if !stored_field_absent(extra.as_ref(), "specifier")? { base.insert("specifier".into(), nullable_value(row.get(6)?)); }
        if !stored_field_absent(extra.as_ref(), "generationId")? { base.insert("generationId".into(), Value::String(row.get(10)?)); }
        if let Some(value) = row.get::<_, Option<String>>(8)? { base.insert("confidenceTier".into(), Value::String(value)); }
        else if !stored_field_absent(extra.as_ref(), "confidenceTier")? { base.insert("confidenceTier".into(), Value::Null); }
        if !stored_field_absent(extra.as_ref(), "evidence")? { base.insert("evidence".into(), parse_json_text(row.get(7)?, Value::Array(Vec::new()))?); }
        merge_extra(&mut base, extra)?;
        result.push(Value::Object(base));
    }
    Ok(result)
}

fn load_file_reports(conn: &Connection) -> Result<Vec<Value>, StoreError> {
    let mut statement = conn.prepare("SELECT path,content_hash,language,provider,parse_status,error_node_count,extra FROM files ORDER BY node_ordinal,rowid")?;
    let mut rows = statement.query([])?;
    let mut reports = Vec::new();
    while let Some(row) = rows.next()? {
        let path: String = row.get(0)?;
        let content_hash: Option<String> = row.get(1)?;
        let language: Option<String> = row.get(2)?;
        let provider: Option<String> = row.get(3)?;
        let parse_status: Option<String> = row.get(4)?;
        let error_node_count: Option<i64> = row.get(5)?;
        let extra_text: Option<String> = row.get(6)?;
        let stored_extra = extra_text.as_ref().map(|text| serde_json::from_str::<Value>(text)).transpose()?;
        let file_extra = stored_extra.as_ref().and_then(|value| value.get("__fileReport")).and_then(Value::as_object);
        if content_hash.is_none() && language.is_none() && provider.is_none() && parse_status.is_none() && error_node_count.is_none() && file_extra.is_none() { continue; }
        let mut report = Map::new();
        report.insert("path".into(), Value::String(path));
        if let Some(value) = content_hash { report.insert("contentHash".into(), Value::String(value)); }
        if let Some(value) = language { report.insert("language".into(), Value::String(value)); }
        if let Some(value) = provider { report.insert("provider".into(), Value::String(value)); }
        if let Some(value) = parse_status { report.insert("parseStatus".into(), Value::String(value)); }
        if let Some(value) = error_node_count { report.insert("errorNodeCount".into(), Value::from(value)); }
        if let Some(file_extra) = file_extra {
            for (key, value) in file_extra { report.insert(key.clone(), value.clone()); }
        }
        reports.push(Value::Object(report));
    }
    Ok(reports)
}

fn load_document_rows(conn: &Connection, generation: &mut Generation) -> Result<(), StoreError> {
    if let Some(marker) = generation.extra.get("__membraneDocumentRows").and_then(Value::as_object).cloned() {
        generation.documents = marker.get("documents").and_then(|value| (!value.is_null()).then(|| value.as_array().cloned()).flatten());
        generation.claims = marker.get("claims").and_then(|value| (!value.is_null()).then(|| value.as_array().cloned()).flatten());
        generation.claim_code_edges = marker.get("claimCodeEdges").and_then(|value| (!value.is_null()).then(|| value.as_array().cloned()).flatten());
        generation.document_supersession = marker.get("documentSupersession").and_then(|value| (!value.is_null()).then(|| value.as_array().cloned()).flatten());
        generation.extra.remove("__membraneDocumentRows");
        return Ok(());
    }
    generation.documents = load_documents(conn)?;
    generation.claims = load_claims(conn)?;
    generation.claim_code_edges = load_claim_code_edges(conn)?;
    generation.document_supersession = load_document_supersession(conn)?;
    Ok(())
}

fn load_documents(conn: &Connection) -> Result<Option<Vec<Value>>, StoreError> {
    let mut statement = conn.prepare("SELECT id,path,content_hash,lifecycle_status,lifecycle_superseded_by,lifecycle_superseded_on,generated_at,generation_id FROM documents ORDER BY rowid")?;
    let mut rows = statement.query([])?;
    let mut values = Vec::new();
    while let Some(row) = rows.next()? {
        let mut object = Map::new();
        object.insert("id".into(), Value::String(row.get(0)?));
        object.insert("path".into(), Value::String(row.get(1)?));
        object.insert("contentHash".into(), nullable_value(row.get(2)?));
        object.insert("lifecycleStatus".into(), nullable_value(row.get(3)?));
        object.insert("lifecycleSupersededBy".into(), nullable_value(row.get(4)?));
        object.insert("lifecycleSupersededOn".into(), nullable_value(row.get(5)?));
        object.insert("generatedAt".into(), nullable_value(row.get(6)?));
        object.insert("generationId".into(), Value::String(row.get(7)?));
        values.push(Value::Object(object));
    }
    Ok((!values.is_empty()).then_some(values))
}

fn load_claims(conn: &Connection) -> Result<Option<Vec<Value>>, StoreError> {
    let mut statement = conn.prepare("SELECT id,document_id,source,line,text,status,sha1,generation_id FROM claims ORDER BY rowid")?;
    let mut rows = statement.query([])?;
    let mut values = Vec::new();
    while let Some(row) = rows.next()? {
        let mut object = Map::new();
        object.insert("id".into(), Value::String(row.get(0)?));
        object.insert("documentId".into(), Value::String(row.get(1)?));
        object.insert("source".into(), nullable_value(row.get(2)?));
        object.insert("line".into(), row.get::<_, Option<i64>>(3)?.map(Value::from).unwrap_or(Value::Null));
        object.insert("text".into(), Value::String(row.get(4)?));
        object.insert("status".into(), nullable_value(row.get(5)?));
        object.insert("sha1".into(), nullable_value(row.get(6)?));
        object.insert("generationId".into(), Value::String(row.get(7)?));
        values.push(Value::Object(object));
    }
    Ok((!values.is_empty()).then_some(values))
}

fn load_claim_code_edges(conn: &Connection) -> Result<Option<Vec<Value>>, StoreError> {
    let mut statement = conn.prepare("SELECT id,claim_id,kind,source,target,confidence,confidence_class,reason,evidence_doc_path,evidence_doc_line,evidence_doc_sha1,evidence_code_path,evidence_code_exists,evidence_code_node_id,evidence_code_content_hash,generation_id FROM claim_code_edges ORDER BY rowid")?;
    let mut rows = statement.query([])?;
    let mut values = Vec::new();
    while let Some(row) = rows.next()? {
        let mut object = Map::new();
        object.insert("id".into(), Value::String(row.get(0)?));
        object.insert("claimId".into(), Value::String(row.get(1)?));
        object.insert("kind".into(), Value::String(row.get(2)?));
        object.insert("source".into(), Value::String(row.get(3)?));
        object.insert("target".into(), Value::String(row.get(4)?));
        object.insert("confidence".into(), nullable_f64(row.get(5)?));
        object.insert("confidenceClass".into(), nullable_value(row.get(6)?));
        object.insert("reason".into(), nullable_value(row.get(7)?));
        let mut evidence = Map::new();
        let mut doc_ref = Map::new();
        if let Some(value) = row.get::<_, Option<String>>(8)? { doc_ref.insert("path".into(), Value::String(value)); }
        doc_ref.insert("line".into(), row.get::<_, Option<i64>>(9)?.map(Value::from).unwrap_or(Value::Null));
        doc_ref.insert("sha1".into(), nullable_value(row.get(10)?));
        evidence.insert("docRef".into(), Value::Object(doc_ref));
        let mut code_ref = Map::new();
        if let Some(value) = row.get::<_, Option<String>>(11)? { code_ref.insert("path".into(), Value::String(value)); }
        code_ref.insert("exists".into(), row.get::<_, Option<i64>>(12)?.map(|value| Value::Bool(value != 0)).unwrap_or(Value::Null));
        evidence.insert("codeRef".into(), Value::Object(code_ref));
        let mut code_node = Map::new();
        if let Some(value) = row.get::<_, Option<String>>(13)? { code_node.insert("id".into(), Value::String(value)); }
        code_node.insert("contentHash".into(), nullable_value(row.get(14)?));
        evidence.insert("codeNode".into(), Value::Object(code_node));
        object.insert("evidence".into(), Value::Object(evidence));
        object.insert("generationId".into(), Value::String(row.get(15)?));
        values.push(Value::Object(object));
    }
    Ok((!values.is_empty()).then_some(values))
}

fn load_document_supersession(conn: &Connection) -> Result<Option<Vec<Value>>, StoreError> {
    let mut statement = conn.prepare("SELECT id,source_kind,source_doc,source_line,source_text,source_external,target_doc,target_external,target_match,superseded_on,generation_id FROM document_supersession ORDER BY rowid")?;
    let mut rows = statement.query([])?;
    let mut values = Vec::new();
    while let Some(row) = rows.next()? {
        let mut object = Map::new();
        object.insert("id".into(), Value::String(row.get(0)?));
        object.insert("sourceKind".into(), Value::String(row.get(1)?));
        object.insert("sourceDoc".into(), nullable_value(row.get(2)?));
        object.insert("sourceLine".into(), row.get::<_, Option<i64>>(3)?.map(Value::from).unwrap_or(Value::Null));
        object.insert("sourceText".into(), nullable_value(row.get(4)?));
        object.insert("sourceExternal".into(), Value::Bool(row.get::<_, i64>(5)? != 0));
        object.insert("targetDoc".into(), nullable_value(row.get(6)?));
        object.insert("targetExternal".into(), Value::Bool(row.get::<_, i64>(7)? != 0));
        object.insert("targetMatch".into(), nullable_value(row.get(8)?));
        object.insert("supersededOn".into(), nullable_value(row.get(9)?));
        object.insert("generationId".into(), Value::String(row.get(10)?));
        values.push(Value::Object(object));
    }
    Ok((!values.is_empty()).then_some(values))
}

fn generation_from_envelope(envelope: GenerationEnvelope) -> Result<Generation, StoreError> {
    Ok(Generation {
        schema_version: envelope.schema_version,
        provider: envelope.provider,
        manifest: envelope.manifest,
        repo_root: envelope.repo_root,
        augmentation: envelope.augmentation,
        source_observation: envelope.source_observation,
        nodes: Vec::new(),
        edges: Vec::new(),
        file_reports: Vec::new(),
        documents: None,
        claims: None,
        claim_code_edges: None,
        document_supersession: None,
        extra: envelope.extra,
    })
}

fn node_object<'a>(value: &'a Value, label: &str) -> Result<&'a Map<String, Value>, StoreError> {
    value
        .as_object()
        .ok_or_else(|| StoreError::InvalidGeneration(format!("{label} must be an object")))
}

fn required_string<'a>(object: &'a Map<String, Value>, key: &str, label: &str) -> Result<&'a str, StoreError> {
    object
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| StoreError::InvalidGeneration(format!("{label}.{key} must be a non-empty string")))
}

fn required_string_or_empty(object: &Map<String, Value>, key: &str) -> String {
    object.get(key).and_then(Value::as_str).unwrap_or_default().to_owned()
}

fn nullable_string(value: Option<&Value>) -> Option<String> {
    value.and_then(Value::as_str).map(ToOwned::to_owned)
}

fn optional_json_string(object: &Map<String, Value>, key: &str, label: &str) -> Result<Option<String>, StoreError> {
    match object.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(_) => Err(StoreError::InvalidGeneration(format!("{label}.{key} must be a string or null"))),
    }
}

fn optional_string_value(value: Option<&Value>) -> Option<String> { nullable_string(value) }

fn nullable_value(value: Option<String>) -> Value {
    value.map(Value::String).unwrap_or(Value::Null)
}

fn nullable_f64(value: Option<f64>) -> Value {
    value.map(Value::from).unwrap_or(Value::Null)
}

fn optional_f64(value: Option<&Value>, field: &str) -> Result<Option<f64>, StoreError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_f64()
            .ok_or_else(|| StoreError::InvalidGeneration(format!("{field} must be a number or null")))
            .map(Some),
    }
}

fn optional_i64(object: &Map<String, Value>, key: &str, label: &str) -> Result<Option<i64>, StoreError> {
    match object.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(value)) => value.as_i64().ok_or_else(|| StoreError::InvalidGeneration(format!("{label}.{key} must be an integer or null"))).map(Some),
        Some(_) => Err(StoreError::InvalidGeneration(format!("{label}.{key} must be an integer or null"))),
    }
}

fn row_generation(object: &Map<String, Value>, generation_id: &str) -> Result<String, StoreError> {
    match object.get("generationId") {
        None | Some(Value::Null) => Ok(generation_id.to_owned()),
        Some(Value::String(value)) if value == generation_id => Ok(value.clone()),
        Some(Value::String(value)) => Err(StoreError::GenerationMismatch { expected: generation_id.to_owned(), observed: value.clone() }),
        Some(_) => Err(StoreError::InvalidGeneration("generationId must be a string or null".into())),
    }
}

fn json_field(object: &Map<String, Value>, key: &str, default: Value) -> Value {
    object.get(key).cloned().unwrap_or(default)
}

fn extra_json(object: &Map<String, Value>, core: &[&str]) -> Result<Option<String>, StoreError> {
    let core = core.iter().copied().collect::<HashSet<_>>();
    let extra = object
        .iter()
        .filter(|(key, _)| !core.contains(key.as_str()))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect::<Map<_, _>>();
    if extra.is_empty() { Ok(None) } else { Ok(Some(serde_json::to_string(&extra)?)) }
}

fn parse_json_text(text: Option<String>, default: Value) -> Result<Value, StoreError> {
    match text {
        None => Ok(default),
        Some(text) => Ok(serde_json::from_str(&text)?),
    }
}

fn merge_extra(base: &mut Map<String, Value>, text: Option<String>) -> Result<(), StoreError> {
    merge_extra_filtered(base, text, None)
}

fn merge_extra_filtered(base: &mut Map<String, Value>, text: Option<String>, skip: Option<&str>) -> Result<(), StoreError> {
    let Some(text) = text else { return Ok(()); };
    let extra: Value = serde_json::from_str(&text)?;
    let Some(extra) = extra.as_object() else {
        return Err(StoreError::InvalidGeneration("stored extra must be a JSON object".into()));
    };
    for (key, value) in extra {
        if key.starts_with("__membrane") || skip == Some(key.as_str()) { continue; }
        base.insert(key.clone(), value.clone());
    }
    Ok(())
}

fn add_file_report_extra(existing: Option<String>, report: Option<&Map<String, Value>>) -> Result<Option<String>, StoreError> {
    let Some(report) = report else { return Ok(existing); };
    let mut all = match existing {
        Some(text) => serde_json::from_str::<Value>(&text)?.as_object().cloned().ok_or_else(|| StoreError::InvalidGeneration("stored extra must be a JSON object".into()))?,
        None => Map::new(),
    };
    all.insert("__fileReport".into(), Value::Object(report.clone()));
    Ok(Some(serde_json::to_string(&all)?))
}

fn add_absent_markers(existing: Option<String>, object: &Map<String, Value>, fields: &[&str]) -> Result<Option<String>, StoreError> {
    let absent: Vec<Value> = fields.iter().filter(|field| !object.contains_key(**field)).map(|field| Value::String((*field).to_owned())).collect();
    if absent.is_empty() { return Ok(existing); }
    let mut all = match existing {
        Some(text) => serde_json::from_str::<Value>(&text)?.as_object().cloned().ok_or_else(|| StoreError::InvalidGeneration("stored extra must be a JSON object".into()))?,
        None => Map::new(),
    };
    all.insert("__membraneAbsent".into(), Value::Array(absent));
    Ok(Some(serde_json::to_string(&all)?))
}

fn stored_field_absent(text: Option<&String>, field: &str) -> Result<bool, StoreError> {
    let Some(text) = text else { return Ok(false); };
    let value: Value = serde_json::from_str(text)?;
    Ok(value.get("__membraneAbsent").and_then(Value::as_array).map(|values| values.iter().any(|value| value.as_str() == Some(field))).unwrap_or(false))
}

fn normalize_repo_path(path: &str) -> String { path.replace('\\', "/") }

fn node_path(value: &Value) -> Option<String> {
    value.get("path").and_then(Value::as_str).map(normalize_repo_path)
}

fn file_reports_by_path(reports: &[Value]) -> Result<HashMap<String, Value>, StoreError> {
    let mut result = HashMap::new();
    for report in reports {
        let object = node_object(report, "file report")?;
        let path = required_string(object, "path", "file report")?;
        result.insert(normalize_repo_path(path), report.clone());
    }
    Ok(result)
}

fn register_node_provider(
    tx: &Transaction<'_>,
    object: &Map<String, Value>,
    id: &str,
    generation_provider: Option<&Value>,
    ranks: &mut HashMap<String, i64>,
) -> Result<(), StoreError> {
    let declared = object.get("factProvider").or_else(|| object.get("provider"));
    let provider_value = declared.or(generation_provider);
    let Some(provider_value) = provider_value else { return Ok(()); };
    let (provider_id, provider_version) = match provider_value {
        Value::String(id) => (id.clone(), "unknown".to_owned()),
        Value::Object(value) => (
            value.get("id").and_then(Value::as_str).unwrap_or("unknown").to_owned(),
            value.get("version").and_then(Value::as_str).unwrap_or("unknown").to_owned(),
        ),
        _ => return Ok(()),
    };
    if provider_id.is_empty() { return Ok(()); }
    let rank = if let Some(rank) = ranks.get(&provider_id) { *rank } else {
        let rank = ranks.len() as i64;
        tx.execute("INSERT INTO provider_ranks(provider_id,rank) VALUES(?1,?2)", params![provider_id, rank])?;
        ranks.insert(provider_id.clone(), rank);
        rank
    };
    let _ = rank;
    tx.execute(
        "INSERT INTO node_provider(node_id,provider_id,source_path,provider_version) VALUES(?1,?2,?3,?4)",
        params![id, provider_id, object.get("path").and_then(Value::as_str).unwrap_or(""), provider_version],
    )?;
    Ok(())
}
