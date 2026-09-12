//! Incremental single-file write primitives for the native store, added to
//! support [`crate::delta_store::apply_file_delta`].
//!
//! `store.rs` only exposes a whole-generation `save_generation` (clear every
//! body table, then reinsert the full node/edge set) — there is no
//! incremental per-file upsert entry point, and `insert_node`/`insert_edge`
//! are private to that module. Per this lane's boundary (edit only
//! `delta_store.rs`, its parity test, and new files; never edit
//! `store.rs`), the missing incremental primitives are reimplemented here
//! against the *same* schema (`files`, `symbols`, `edges`, `fact_owner`,
//! `node_provider`, `dependency_index`, `file_state`, `watch_state`,
//! `event_journal`, `generation`, `generation_leaf` — see
//! `migrations.rs`), which is in fact schema-identical to the legacy
//! `better-sqlite3` store `delta-store.mjs` was written against.
//!
//! Deviations from the legacy `insertParsedFacts`/`fileNode` path, all
//! intentional and out of this lane's scope to close:
//! - `provider_ranks`/`symbol_search`/`symbol_terms` (query-projection
//!   caches) are not maintained here; they are rebuilt by other paths
//!   (`save_generation`, dedicated reindex jobs) and are not part of the
//!   graph-store write semantics `applyFileDelta` is responsible for.
//! - `annotation_nodes` (comment nodes) are supported for retraction but not
//!   for insertion in this lane; comment-kind delta nodes are ignored (not
//!   an error) on insert, matching no known production delta producer today.
//! - The legacy `duplicate_node_id` physical-collision guard
//!   (`insertParsedFacts`) is not reproduced; inserts are idempotent
//!   upserts instead, which is the correct behavior for a store that must
//!   tolerate re-application of the same delta (the legacy noop/digest
//!   short-circuit in `applyFileDelta` covers the common case; the
//!   remaining divergence is that a genuine node-id collision across
//!   distinct source files silently overwrites rather than throwing).

use crate::identity::compute_manifest_digest_value;
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde_json::{Map, Value};
use xxhash_rust::xxh3::xxh3_128;

fn normalize_repo_path(path: &str) -> String {
    path.replace('\\', "/")
}

fn xxh3_128_hex(bytes: &[u8]) -> String {
    format!("{:032x}", xxh3_128(bytes))
}

/// Mirrors `resealGenerationIdentityDelta` (`generation-identity.mjs`): folds
/// the previous generation id, the new merkle root digest, and the applied
/// clock into a fresh `generationId`, then recomputes `manifestDigest`.
/// Returns the updated manifest. Errors if `manifest` is absent.
pub fn reseal_generation_identity_delta(
    manifest: &mut Value,
    source_observation: Option<&Value>,
    root_digest: Option<&str>,
    applied_clock: i64,
) -> Result<(), String> {
    let previous_generation_id = manifest.get("generationId").and_then(Value::as_str).map(str::to_owned);
    let stable = serde_json::json!({
        "previousGenerationId": previous_generation_id,
        "rootMerkleDigest": root_digest,
        "appliedClock": applied_clock,
    });
    let stable_text = canonical_json(&stable);
    let generation_id = format!("xxh128:{}", xxh3_128_hex(stable_text.as_bytes()));
    let object = manifest.as_object_mut().ok_or("manifest must be an object")?;
    object.insert("generationId".into(), Value::String(generation_id.clone()));
    let short = generation_id.strip_prefix("xxh128:").unwrap_or(&generation_id);
    let short: String = short.chars().take(16).collect();
    object.insert("generatedAt".into(), Value::String(format!("gen:{short}")));
    let digest = compute_manifest_digest_value(manifest, source_observation);
    manifest
        .as_object_mut()
        .unwrap()
        .insert("manifestDigest".into(), Value::String(digest));
    Ok(())
}

/// Stable (sorted-key) JSON stringification, matching the legacy
/// `stableStringify` used by `resealGenerationIdentityDelta`.
fn canonical_json(value: &Value) -> String {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let parts: Vec<String> = keys
                .into_iter()
                .map(|key| format!("{}:{}", serde_json::to_string(key).unwrap(), canonical_json(&map[key])))
                .collect();
            format!("{{{}}}", parts.join(","))
        }
        Value::Array(items) => {
            let parts: Vec<String> = items.iter().map(canonical_json).collect();
            format!("[{}]", parts.join(","))
        }
        other => serde_json::to_string(other).unwrap(),
    }
}

// ---------------------------------------------------------------------
// file_state
// ---------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct FileStateRow {
    pub content_digest: String,
    pub size: i64,
    pub mtime_ms: Option<f64>,
    pub file_identity: Option<String>,
    pub last_event_seq: Option<i64>,
    pub applied_clock: i64,
}

/// Mirrors `loadFileState`.
pub fn load_file_state(tx: &Transaction<'_>, path: &str) -> rusqlite::Result<Option<FileStateRow>> {
    tx.query_row(
        "SELECT content_digest, size, mtime_ms, file_identity, last_event_seq, applied_clock FROM file_state WHERE path = ?1",
        params![path],
        |row| {
            Ok(FileStateRow {
                content_digest: row.get(0)?,
                size: row.get(1)?,
                mtime_ms: row.get(2)?,
                file_identity: row.get(3)?,
                last_event_seq: row.get(4)?,
                applied_clock: row.get(5)?,
            })
        },
    )
    .optional()
}

/// Mirrors `updateFileState`: upsert one `file_state` row.
pub fn update_file_state(
    tx: &Transaction<'_>,
    path: &str,
    digest: &str,
    source_clock: i64,
    file_identity: Option<&str>,
    size: i64,
    mtime_ms: Option<f64>,
    event_seq: Option<i64>,
) -> rusqlite::Result<()> {
    tx.execute(
        "INSERT INTO file_state(path, content_digest, size, mtime_ms, file_identity, last_event_seq, applied_clock)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(path) DO UPDATE SET content_digest=excluded.content_digest, size=excluded.size,
           mtime_ms=excluded.mtime_ms, file_identity=excluded.file_identity, last_event_seq=excluded.last_event_seq,
           applied_clock=excluded.applied_clock",
        params![path, digest, size, mtime_ms, file_identity, event_seq, source_clock],
    )?;
    Ok(())
}

/// Acknowledges an already-applied file without touching its digest: mirrors
/// the noop-short-circuit branch's `UPDATE file_state SET applied_clock=...`.
pub fn acknowledge_file_state(
    tx: &Transaction<'_>,
    path: &str,
    acknowledged_clock: i64,
    journal_seq: Option<i64>,
    size: Option<i64>,
    mtime_ms: Option<f64>,
    file_identity: Option<&str>,
) -> rusqlite::Result<()> {
    tx.execute(
        "UPDATE file_state SET applied_clock=?1, last_event_seq=COALESCE(?2, last_event_seq),
           size=COALESCE(?3, size), mtime_ms=COALESCE(?4, mtime_ms), file_identity=COALESCE(?5, file_identity)
         WHERE path=?6",
        params![acknowledged_clock, journal_seq, size, mtime_ms, file_identity, path],
    )?;
    Ok(())
}

pub fn delete_file_state(tx: &Transaction<'_>, path: &str) -> rusqlite::Result<()> {
    tx.execute("DELETE FROM file_state WHERE path = ?1", params![path])?;
    Ok(())
}

// ---------------------------------------------------------------------
// watch_state (applied_clock + domains_pending)
// ---------------------------------------------------------------------

pub fn ensure_watch_state(tx: &Transaction<'_>) -> rusqlite::Result<()> {
    tx.execute_batch("CREATE TABLE IF NOT EXISTS watch_state (key TEXT PRIMARY KEY, value TEXT NOT NULL)")
}

/// Mirrors `clock(db, key, fallback)`.
pub fn read_clock(tx: &Transaction<'_>, key: &str, fallback: i64) -> rusqlite::Result<i64> {
    let value: Option<String> = tx
        .query_row("SELECT value FROM watch_state WHERE key = ?1", params![key], |row| row.get(0))
        .optional()?;
    Ok(value.and_then(|v| v.parse::<i64>().ok()).unwrap_or(fallback))
}

/// Mirrors `setClock(db, key, value)`.
pub fn set_clock(tx: &Transaction<'_>, key: &str, value: i64) -> rusqlite::Result<()> {
    tx.execute(
        "INSERT INTO watch_state(key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![key, value.to_string()],
    )?;
    Ok(())
}

// ---------------------------------------------------------------------
// dependency_index
// ---------------------------------------------------------------------

/// Mirrors `refreshDependencies`: replace every `dependency_index` row
/// touching `path` (as source or dependent) with the supplied set.
pub fn refresh_dependencies(
    tx: &Transaction<'_>,
    path: &str,
    dependencies: &[(String, String, String)],
) -> rusqlite::Result<()> {
    tx.execute(
        "DELETE FROM dependency_index WHERE source_path = ?1 OR dependent_path = ?1",
        params![path],
    )?;
    let mut insert = tx.prepare(
        "INSERT OR REPLACE INTO dependency_index(source_path, dependent_path, reason) VALUES (?1, ?2, ?3)",
    )?;
    for (source_path, dependent_path, reason) in dependencies {
        insert.execute(params![normalize_repo_path(source_path), normalize_repo_path(dependent_path), reason])?;
    }
    Ok(())
}

// ---------------------------------------------------------------------
// fact_owner / node_provider / files / symbols / edges retraction+upsert
// ---------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct FactOwnerRow {
    pub fact_id: String,
    pub fact_kind: String, // "node" | "edge"
    pub provider_id: String,
}

/// Selects `fact_owner` rows for `path`, optionally restricted to a set of
/// provider ids (mirrors the `structuralProviders.length` branch in
/// `applyFileDelta`).
pub fn select_fact_owners(
    tx: &Transaction<'_>,
    path: &str,
    provider_ids: Option<&[String]>,
) -> rusqlite::Result<Vec<FactOwnerRow>> {
    match provider_ids {
        None => {
            let mut stmt = tx.prepare("SELECT fact_id, fact_kind, provider_id FROM fact_owner WHERE source_path = ?1")?;
            let rows = stmt
                .query_map(params![path], |row| {
                    Ok(FactOwnerRow { fact_id: row.get(0)?, fact_kind: row.get(1)?, provider_id: row.get(2)? })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        }
        Some(providers) => {
            let placeholders = providers.iter().map(|_| "?").collect::<Vec<_>>().join(",");
            let sql = format!(
                "SELECT fact_id, fact_kind, provider_id FROM fact_owner WHERE source_path = ? AND provider_id IN ({placeholders})"
            );
            let mut stmt = tx.prepare(&sql)?;
            let mut param_values: Vec<&dyn rusqlite::ToSql> = vec![&path];
            for provider in providers {
                param_values.push(provider);
            }
            let rows = stmt
                .query_map(param_values.as_slice(), |row| {
                    Ok(FactOwnerRow { fact_id: row.get(0)?, fact_kind: row.get(1)?, provider_id: row.get(2)? })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        }
    }
}

/// Mirrors `deleteFactsByOwner(db, path, providerId)`: deletes the
/// `symbols`/`edges`/`annotation_nodes`/`node_provider` rows owned by
/// `path` (optionally restricted to one `providerId`), then the owning
/// `fact_owner` rows themselves. `files` rows are not touched here (the
/// legacy caller deletes `files` separately only on delete/rename).
pub fn delete_facts_by_owner(
    tx: &Transaction<'_>,
    path: &str,
    provider_id: Option<&str>,
) -> rusqlite::Result<Vec<FactOwnerRow>> {
    let owners = match provider_id {
        None => select_fact_owners(tx, path, None)?,
        Some(provider) => select_fact_owners(tx, path, Some(std::slice::from_ref(&provider.to_string())))?,
    };
    for owner in &owners {
        if owner.fact_kind == "node" {
            tx.execute("DELETE FROM symbols WHERE id = ?1", params![owner.fact_id])?;
            tx.execute("DELETE FROM annotation_nodes WHERE id = ?1", params![owner.fact_id])?;
            tx.execute("DELETE FROM node_provider WHERE node_id = ?1", params![owner.fact_id])?;
        } else {
            tx.execute("DELETE FROM edges WHERE id = ?1", params![owner.fact_id])?;
        }
    }
    match provider_id {
        None => {
            tx.execute("DELETE FROM fact_owner WHERE source_path = ?1", params![path])?;
        }
        Some(provider) => {
            tx.execute(
                "DELETE FROM fact_owner WHERE source_path = ?1 AND provider_id = ?2",
                params![path, provider],
            )?;
        }
    }
    if provider_id.is_none() {
        // Complete generations written by the native builder predate
        // `fact_owner`; recover their ownership from path/provider indexes so
        // the first incremental repair can replace rows without collisions.
        let mut node_ids = Vec::new();
        {
            let mut statement = tx.prepare("SELECT node_id FROM node_provider WHERE source_path = ?1")?;
            let rows = statement.query_map(params![path], |row| row.get::<_, String>(0))?;
            node_ids.extend(rows.collect::<Result<Vec<_>, _>>()?);
        }
        for node_id in &node_ids {
            tx.execute("DELETE FROM symbols WHERE id = ?1", params![node_id])?;
            tx.execute("DELETE FROM annotation_nodes WHERE id = ?1", params![node_id])?;
            tx.execute("DELETE FROM node_provider WHERE node_id = ?1", params![node_id])?;
        }
        let edge_pattern = format!("%\"path\":\"{}\"%", path.replace('"', ""));
        let mut edge_ids = Vec::new();
        {
            let mut statement = tx.prepare("SELECT id FROM edges WHERE evidence LIKE ?1")?;
            let rows = statement.query_map(params![edge_pattern], |row| row.get::<_, String>(0))?;
            edge_ids.extend(rows.collect::<Result<Vec<_>, _>>()?);
        }
        for edge_id in edge_ids { tx.execute("DELETE FROM edges WHERE id = ?1", params![edge_id])?; }
        tx.execute("DELETE FROM files WHERE path = ?1", params![path])?;
        tx.execute("DELETE FROM symbols WHERE path = ?1", params![path])?;
        tx.execute("DELETE FROM node_provider WHERE source_path = ?1", params![path])?;
    }
    Ok(owners)
}

/// Unresolves every edge whose `target` is one of `node_ids` (mirrors the
/// delete/rename branch's `UPDATE edges SET resolved = 0 WHERE target = ?`
/// loop).
pub fn unresolve_edges_targeting(tx: &Transaction<'_>, node_ids: &[String]) -> rusqlite::Result<()> {
    let mut stmt = tx.prepare("UPDATE edges SET resolved = 0 WHERE target = ?1")?;
    for id in node_ids {
        stmt.execute(params![id])?;
    }
    Ok(())
}

pub fn delete_file_row(tx: &Transaction<'_>, path: &str) -> rusqlite::Result<()> {
    tx.execute("DELETE FROM files WHERE path = ?1", params![path])?;
    Ok(())
}

/// One parsed node or edge, in the same JSON shape `insertParsedFacts`
/// consumes (`{id, kind, labels?, name?, qualifiedName?, path?, confidence?,
/// evidence?, ...extra}` for nodes; `{id, kind, source, target?,
/// confidence?, confidenceTier?, evidence?, resolved?, specifier?,
/// ...extra}` for edges).
pub type ParsedValue = Value;

fn confidence_or_default(value: Option<&Value>) -> f64 {
    value.and_then(Value::as_f64).unwrap_or(1.0)
}

/// Upserts one parsed node into `files` or `symbols` (comment-kind nodes are
/// skipped — see module docs), then records `node_provider` and
/// `fact_owner`. Mirrors the node-insertion loop inside `insertParsedFacts`
/// plus its trailing `insertOwner.run(...)` call for nodes.
pub fn upsert_parsed_node(
    tx: &Transaction<'_>,
    node: &ParsedValue,
    generation_id: &str,
    source_digest: &str,
    provider_id: &str,
    provider_version: &str,
    repo_root: Option<&str>,
) -> Result<(), String> {
    let object = node.as_object().ok_or("parsed node must be an object")?;
    let id = object.get("id").and_then(Value::as_str).ok_or("parsed node.id is required")?;
    let kind = object.get("kind").and_then(Value::as_str).ok_or("parsed node.kind is required")?;
    let labels = object.get("labels").cloned().unwrap_or_else(|| Value::Array(vec![Value::String("File".into())]));
    let evidence = object.get("evidence").cloned().unwrap_or_else(|| Value::Array(Vec::new()));
    let confidence = confidence_or_default(object.get("confidence"));
    let core = ["id", "kind", "labels", "name", "qualifiedName", "path", "confidence", "evidence"];
    let extra: Map<String, Value> = object
        .iter()
        .filter(|(key, _)| !core.contains(&key.as_str()))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    let extra_text = if extra.is_empty() { None } else { Some(serde_json::to_string(&extra).map_err(|e| e.to_string())?) };

    if kind == "comment" {
        // Comment (annotation) node insertion is out of this lane's scope
        // for the incremental path; see module docs.
        return Ok(());
    }

    if kind == "file" {
        let path = normalize_repo_path(object.get("path").and_then(Value::as_str).ok_or("file node.path is required")?);
        let content_hash = source_digest.strip_prefix("xxh128:").unwrap_or(source_digest);
        let name = object.get("name").and_then(Value::as_str).map(str::to_owned).unwrap_or_else(|| path.rsplit('/').next().unwrap_or(&path).to_owned());
        let qualified_name = object.get("qualifiedName").and_then(Value::as_str).map(str::to_owned).unwrap_or_else(|| path.clone());
        tx.execute(
            "INSERT INTO files(path, content_hash, language, provider, parse_status, error_node_count, generation_id, node_id, labels, name, qualified_name, confidence, evidence, extra, node_ordinal)
             VALUES (?1, ?2, NULL, ?3, NULL, NULL, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 0)
             ON CONFLICT(path) DO UPDATE SET content_hash=excluded.content_hash, provider=excluded.provider,
               generation_id=excluded.generation_id, node_id=excluded.node_id, labels=excluded.labels, name=excluded.name,
               qualified_name=excluded.qualified_name, confidence=excluded.confidence, evidence=excluded.evidence, extra=excluded.extra",
            params![
                path, content_hash, provider_id, generation_id, id,
                serde_json::to_string(&labels).map_err(|e| e.to_string())?, name, qualified_name, confidence,
                serde_json::to_string(&evidence).map_err(|e| e.to_string())?, extra_text,
            ],
        ).map_err(|e| e.to_string())?;
    } else {
        let name = object.get("name").and_then(Value::as_str).unwrap_or_default();
        let qualified_name = object.get("qualifiedName").and_then(Value::as_str).unwrap_or(name);
        let path = object.get("path").and_then(Value::as_str).unwrap_or_default();
        tx.execute(
            "INSERT INTO symbols(id, kind, labels, name, qualified_name, path, confidence, evidence, generation_id, extra)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(id) DO UPDATE SET kind=excluded.kind, labels=excluded.labels, name=excluded.name,
               qualified_name=excluded.qualified_name, path=excluded.path, confidence=excluded.confidence,
               evidence=excluded.evidence, generation_id=excluded.generation_id, extra=excluded.extra",
            params![
                id, kind, serde_json::to_string(&labels).map_err(|e| e.to_string())?, name, qualified_name, path,
                confidence, serde_json::to_string(&evidence).map_err(|e| e.to_string())?, generation_id, extra_text,
            ],
        ).map_err(|e| e.to_string())?;
    }

    tx.execute(
        "INSERT INTO node_provider(node_id, provider_id, source_path, provider_version) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(node_id) DO UPDATE SET provider_id=excluded.provider_id, source_path=excluded.source_path, provider_version=excluded.provider_version",
        params![id, provider_id, object.get("path").and_then(Value::as_str).unwrap_or(""), provider_version],
    ).map_err(|e| e.to_string())?;

    tx.execute(
        "INSERT OR REPLACE INTO fact_owner(fact_id, fact_kind, source_path, source_digest, provider_id, provider_version, freshness_domain, fact_kind_detail)
         VALUES (?1, 'node', ?2, ?3, ?4, ?5, 'structural', ?6)",
        params![id, object.get("path").and_then(Value::as_str).unwrap_or(""), source_digest, provider_id, provider_version, kind],
    ).map_err(|e| e.to_string())?;
    let _ = repo_root; // repo_root is carried by callers for reseal only; fact_owner here has no repo_root column in this schema version.
    Ok(())
}

/// Upserts one parsed edge into `edges`, then records `fact_owner` for it
/// keyed by its source node's path. Mirrors the edge-insertion loop inside
/// `insertParsedFacts`.
pub fn upsert_parsed_edge(
    tx: &Transaction<'_>,
    edge: &ParsedValue,
    generation_id: &str,
    source_digest: &str,
    provider_id: &str,
    provider_version: &str,
    source_node_path: Option<&str>,
) -> Result<(), String> {
    let object = edge.as_object().ok_or("parsed edge must be an object")?;
    let id = object.get("id").and_then(Value::as_str).ok_or("parsed edge.id is required")?;
    let kind = object.get("kind").and_then(Value::as_str).ok_or("parsed edge.kind is required")?;
    let source = object.get("source").and_then(Value::as_str).ok_or("parsed edge.source is required")?;
    let target = object.get("target").and_then(Value::as_str);
    let confidence = confidence_or_default(object.get("confidence"));
    let evidence = object.get("evidence").cloned().unwrap_or_else(|| Value::Array(Vec::new()));
    let confidence_tier = object.get("confidenceTier").and_then(Value::as_str);
    let resolved = object.get("resolved").and_then(Value::as_bool).unwrap_or(true);
    let specifier = object.get("specifier").and_then(Value::as_str);
    let core = ["id", "kind", "source", "target", "confidence", "confidenceTier", "evidence"];
    let extra: Map<String, Value> = object
        .iter()
        .filter(|(key, _)| !core.contains(&key.as_str()))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    let extra_text = if extra.is_empty() { None } else { Some(serde_json::to_string(&extra).map_err(|e| e.to_string())?) };

    tx.execute(
        "INSERT INTO edges(id, kind, source, target, confidence, resolved, specifier, evidence, generation_id, confidence_tier, extra)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
         ON CONFLICT(id) DO UPDATE SET kind=excluded.kind, source=excluded.source, target=excluded.target,
           confidence=excluded.confidence, resolved=excluded.resolved, specifier=excluded.specifier, evidence=excluded.evidence,
           generation_id=excluded.generation_id, confidence_tier=excluded.confidence_tier, extra=excluded.extra",
        params![
            id, kind, source, target, confidence, if resolved { 1 } else { 0 }, specifier,
            serde_json::to_string(&evidence).map_err(|e| e.to_string())?, generation_id, confidence_tier, extra_text,
        ],
    ).map_err(|e| e.to_string())?;

    let source_path = source_node_path.or_else(|| {
        evidence.as_array().and_then(|items| items.iter().find_map(|item| item.get("path").and_then(Value::as_str)))
    });
    if let Some(source_path) = source_path {
        tx.execute(
            "INSERT OR REPLACE INTO fact_owner(fact_id, fact_kind, source_path, source_digest, provider_id, provider_version, freshness_domain, fact_kind_detail)
             VALUES (?1, 'edge', ?2, ?3, ?4, ?5, 'structural', ?6)",
            params![id, source_path, source_digest, provider_id, provider_version, kind],
        ).map_err(|e| e.to_string())?;
    }
    Ok(())
}

// ---------------------------------------------------------------------
// manifest counts + generation envelope helpers
// ---------------------------------------------------------------------

/// Mirrors `refreshManifestCounts`: recompute `manifest.counts.nodes`
/// (`files + symbols + annotations`) and `manifest.counts.edges`.
pub fn refresh_manifest_counts(tx: &Transaction<'_>, manifest: &mut Value) -> rusqlite::Result<()> {
    let files: i64 = tx.query_row("SELECT COUNT(*) FROM files", [], |row| row.get(0))?;
    let symbols: i64 = tx.query_row("SELECT COUNT(*) FROM symbols", [], |row| row.get(0))?;
    let annotations: i64 = tx.query_row("SELECT COUNT(*) FROM annotation_nodes", [], |row| row.get(0))?;
    let edges: i64 = tx.query_row("SELECT COUNT(*) FROM edges", [], |row| row.get(0))?;
    let object = manifest.as_object_mut().expect("manifest must be an object");
    let counts = object.entry("counts").or_insert_with(|| Value::Object(Map::new()));
    let counts_object = counts.as_object_mut().expect("manifest.counts must be an object");
    counts_object.insert("nodes".into(), Value::from(files + symbols + annotations));
    counts_object.insert("edges".into(), Value::from(edges));
    Ok(())
}

/// Reads the current `manifest` value out of the `generation` table, or
/// `None` if unset.
pub fn read_manifest(tx: &Transaction<'_>) -> rusqlite::Result<Option<Value>> {
    let text: Option<String> = tx
        .query_row("SELECT value FROM generation WHERE key='manifest'", [], |row| row.get(0))
        .optional()?;
    Ok(match text {
        Some(text) => Some(serde_json::from_str(&text).unwrap_or(Value::Null)),
        None => None,
    })
}

pub fn read_source_observation(tx: &Transaction<'_>) -> rusqlite::Result<Option<Value>> {
    let text: Option<String> = tx
        .query_row("SELECT value FROM generation WHERE key='sourceObservation'", [], |row| row.get(0))
        .optional()?;
    Ok(match text {
        Some(text) => Some(serde_json::from_str(&text).unwrap_or(Value::Null)),
        None => None,
    })
}

/// Writes `manifest` back into `generation`. Mirrors
/// `UPDATE generation SET value=? WHERE key='manifest'`.
pub fn write_manifest(tx: &Transaction<'_>, manifest: &Value) -> rusqlite::Result<()> {
    tx.execute(
        "UPDATE generation SET value=?1 WHERE key='manifest'",
        params![serde_json::to_string(manifest).unwrap()],
    )?;
    Ok(())
}

/// Mirrors `db.prepare("SELECT digest FROM generation_leaf WHERE path = '' AND kind = 'dir'").get()?.digest`.
pub fn root_digest(conn: &Connection) -> rusqlite::Result<Option<String>> {
    conn.query_row(
        "SELECT digest FROM generation_leaf WHERE path = '' AND kind = 'dir'",
        [],
        |row| row.get(0),
    )
    .optional()
}

pub fn root_digest_tx(tx: &Transaction<'_>) -> rusqlite::Result<Option<String>> {
    tx.query_row(
        "SELECT digest FROM generation_leaf WHERE path = '' AND kind = 'dir'",
        [],
        |row| row.get(0),
    )
    .optional()
}

pub fn has_table(tx: &Transaction<'_>, name: &str) -> rusqlite::Result<bool> {
    tx.query_row(
        "SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1",
        params![name],
        |_| Ok(true),
    )
    .optional()
    .map(|value| value.unwrap_or(false))
}

/// Mirrors the journal-ack branch:
/// `UPDATE event_journal SET applied = 1, applied_clock = ? WHERE seq = ?`.
pub fn acknowledge_journal(tx: &Transaction<'_>, journal_seq: i64, applied_clock: i64) -> rusqlite::Result<()> {
    tx.execute(
        "UPDATE event_journal SET applied = 1, applied_clock = ?1 WHERE seq = ?2",
        params![applied_clock, journal_seq],
    )?;
    Ok(())
}
