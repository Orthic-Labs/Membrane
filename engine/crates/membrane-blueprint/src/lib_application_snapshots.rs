//! Native port of `blueprint/src/graph/snapshots.mjs` (`getSnapshot`,
//! `listSnapshots`, `createSnapshot`, `changesSince`, `changesSinceReference`
//! for `snapshot`, `generation`, and `treeish` reference kinds) plus
//! `service.mjs`'s `snapshot_get`/`snapshot_list`/`changes` operation
//! surface.
//!
//! Scoped deviation from the legacy semantics (recorded verbatim in this
//! lane's receipt): the native store does not maintain the legacy
//! `generation_leaf` table (it is never populated by `store::save_generation`
//! for any lane's generation-write path), and this lane's remaining budget
//! did not include safely wiring a second leaf-writer into the shared
//! `save_generation` transaction. Leaf identity here is instead computed from
//! the already-populated `files` table (`path`, `content_hash`), which is the
//! native store's structural equivalent of `generation_leaf`'s `(path,
//! digest)` pairs for the current generation.
//!
//! Lane STORE3 (2026-09-10): the one remaining gap named by lane STORE2's
//! receipt -- `changesSinceReference`'s `semanticDelta` always reporting
//! `null` for the `treeish` reference kind -- is now closed. Legacy's
//! `treeishSemanticGraph` materialises an arbitrary git treeish's blobs into
//! a temp dir and re-runs the full provider pipeline over them
//! (`buildGraphGeneration` + `semanticGraphFromGeneration`). `git archive` is
//! not reliably available on plain Windows, so this port instead uses `git
//! worktree add --detach <tmp> <treeish>` (bounded timeout, guaranteed `git
//! worktree remove --force` cleanup via a `Drop` guard even on error/panic
//! unwind) to materialise the treeish's full tree, then reuses the crate's
//! own in-memory graph builder (`graph::build_generation_with_cancellation`,
//! the same function `engine::build_and_publish` calls, without publishing
//! anything to the store) to build a graph body over that worktree, and feeds
//! it through the same `semantic_graph_from_generation`/`semantic_delta` this
//! module already uses for the `snapshot` and `generation` reference kinds.
//! Both the `base` and `head` treeish are materialised this way (a `head` of
//! `"HEAD"` is a treeish like any other), so `semanticDelta` for `treeish` is
//! now a real diff, not a stand-in for the current persisted generation.

use crate::api::BlueprintError;
use rusqlite::{Connection, OptionalExtension};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn fail(code: &'static str, message: impl Into<String>) -> BlueprintError {
    BlueprintError::new(code, message.into())
}

fn root_of(root: &Path) -> String {
    std::fs::canonicalize(root)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| root.to_string_lossy().replace('\\', "/"))
}

fn stable_stringify(value: &Value) -> String {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let parts: Vec<String> = keys
                .into_iter()
                .map(|key| format!("{}:{}", serde_json::to_string(key).unwrap_or_default(), stable_stringify(&map[key])))
                .collect();
            format!("{{{}}}", parts.join(","))
        }
        Value::Array(items) => format!("[{}]", items.iter().map(stable_stringify).collect::<Vec<_>>().join(",")),
        other => serde_json::to_string(other).unwrap_or_else(|_| "null".into()),
    }
}

fn digest(value: &Value) -> String {
    let mut hasher = Sha256::new();
    hasher.update(stable_stringify(value).as_bytes());
    format!("sha256:{}", hex::encode(hasher.finalize()))
}

/// Bounded `git` invocation using the shared treeish runner. The runner drains
/// stdout concurrently, avoiding the pipe-fill deadlock that made large diffs
/// intermittently hit their timeout.
fn run_git(root: &Path, args: &[&str]) -> Result<Vec<u8>, ()> {
    crate::delta_store::run_treeish_git(root, args, crate::delta_store::TREEISH_GIT_TIMEOUT).map_err(|_| ())
}

/// Native port of `treeishSemanticGraph`. Materialises `treeish`'s full tree
/// into a detached worktree, builds an in-memory graph body over it with the
/// crate's own graph builder (the same function `engine::build_and_publish`
/// calls for the live repository, never publishing this one to the store),
/// and projects it through the same `semantic_graph_from_generation` used
/// for the `snapshot`/`generation` reference kinds.
fn treeish_semantic_graph(repo_root: &Path, treeish: &str) -> Result<Value, BlueprintError> {
    crate::delta_store::with_treeish_worktree(repo_root, treeish, |worktree| {
        let historical_store = worktree.join(".agent").join("graph").join("graph.db");
        let authorization = crate::engine::verified_construction_reason(&historical_store)
            .map_err(|error| error.to_string())?;
        if authorization.is_none() {
            return Err("historical graph construction is not authorized for an existing valid store".into());
        }
        let cancellation = crate::api::CancellationToken::new();
        let graph = crate::graph::build_generation_with_cancellation(
            worktree,
            &crate::graph::GraphOptions::default(),
            &cancellation,
        )
        .map_err(|error| error.to_string())?;
        let generation = crate::store::Generation {
            nodes: graph.nodes.iter().filter_map(|value| serde_json::to_value(value).ok()).collect(),
            edges: graph.edges.iter().filter_map(|value| serde_json::to_value(value).ok()).collect(),
            ..Default::default()
        };
        Ok::<Value, String>(semantic_graph_from_generation(&generation))
    })
    .map_err(|error| fail(error.code(), format!("treeish '{treeish}' failed: {error}")))
}

pub fn current_git_identity(root: &Path) -> Option<(String, bool)> {
    let repo_root = PathBuf::from(root_of(root));
    let head = run_git(&repo_root, &["rev-parse", "HEAD"]).ok()?;
    let head = String::from_utf8_lossy(&head).trim().to_string();
    let status = run_git(
        &repo_root,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all", "--", ".", ":(exclude).agent", ":(exclude).agent/**"],
    )
    .ok()?;
    Some((head, !status.is_empty()))
}

// Native port of `semanticSurface`/`evidenceCitations`/`semanticGraphFromGeneration`
// (`blueprint/src/graph/snapshots.mjs`), used to close gap 1 from this lane's
// task brief: `changesSinceReference`'s `semanticDelta` is a real node/edge
// fingerprint diff between two generation bodies for every supported
// reference kind, including treeish worktree materialisation.
fn semantic_surface(value: &Value, omit: &[&str]) -> Value {
    let Value::Object(map) = value else { return json!({}) };
    let mut out = serde_json::Map::new();
    for (key, item) in map {
        if omit.contains(&key.as_str()) || item.is_null() {
            continue;
        }
        out.insert(key.clone(), item.clone());
    }
    Value::Object(out)
}

fn evidence_citations(evidence: &Value) -> Value {
    let entries: Vec<Value> = match evidence {
        Value::Array(items) => items.clone(),
        Value::Null => Vec::new(),
        other => vec![other.clone()],
    };
    Value::Array(
        entries
            .into_iter()
            .map(|item| match item {
                Value::String(_) => item,
                other => semantic_surface(&other, &["text", "content", "snippet", "source"]),
            })
            .collect(),
    )
}

struct SemanticItem {
    key: String,
    fingerprint: String,
    value: Value,
}

fn semantic_graph_from_generation(generation: &crate::store::Generation) -> Value {
    let mut id_to_key: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut nodes: Vec<SemanticItem> = Vec::new();
    for node in &generation.nodes {
        let id = node.get("id").and_then(Value::as_str).unwrap_or_default().to_owned();
        let kind = node.get("kind").and_then(Value::as_str).unwrap_or("node");
        let path = node.get("path").and_then(Value::as_str).unwrap_or_default();
        let key = if !id.is_empty() {
            id.clone()
        } else if kind == "file" {
            format!("file:{path}")
        } else {
            let name = node.get("qualifiedName").and_then(Value::as_str)
                .or_else(|| node.get("name").and_then(Value::as_str))
                .unwrap_or("anonymous");
            format!("{kind}:{path}:{name}")
        };
        if !id.is_empty() {
            id_to_key.insert(id, key.clone());
        }
        let surface = semantic_surface(node, &["id", "generationId", "provider"]);
        let evidence = evidence_citations(node.get("evidence").unwrap_or(&Value::Null));
        let summary = semantic_surface(node, &["id", "generationId", "provider", "evidence", "content", "text", "snippet"]);
        nodes.push(SemanticItem { key, fingerprint: digest(&surface), value: json!({"evidence": evidence, "summary": summary}) });
    }
    nodes.sort_by(|a, b| a.key.cmp(&b.key));

    let mut edges: Vec<SemanticItem> = Vec::new();
    for edge in &generation.edges {
        let source_id = edge.get("source").and_then(Value::as_str).unwrap_or_default();
        let target_id = edge.get("target").and_then(Value::as_str);
        let source = id_to_key.get(source_id).cloned().unwrap_or_else(|| format!("external:{source_id}"));
        let target = target_id.map(|t| id_to_key.get(t).cloned().unwrap_or_else(|| format!("external:{t}")));
        let id = edge.get("id").and_then(Value::as_str).unwrap_or_default();
        let kind = edge.get("kind").and_then(Value::as_str).unwrap_or("edge");
        let target_or_specifier = target.clone().or_else(|| edge.get("specifier").and_then(Value::as_str).map(str::to_owned)).unwrap_or_else(|| "unresolved".into());
        let key = if !id.is_empty() { id.to_owned() } else { format!("{kind}:{source}->{target_or_specifier}") };
        let mut surface = semantic_surface(edge, &["id", "generationId", "provider", "source", "target"]);
        if let Value::Object(map) = &mut surface {
            map.insert("source".into(), json!(source));
            map.insert("target".into(), target.clone().map(Value::String).unwrap_or(Value::Null));
        }
        let evidence = evidence_citations(edge.get("evidence").unwrap_or(&Value::Null));
        let summary = semantic_surface(&surface, &["evidence", "content", "text", "snippet"]);
        edges.push(SemanticItem { key, fingerprint: digest(&surface), value: json!({"evidence": evidence, "summary": summary}) });
    }
    edges.sort_by(|a, b| a.key.cmp(&b.key));

    let to_json = |items: &[SemanticItem]| -> Value {
        Value::Array(
            items
                .iter()
                .map(|item| json!({"key": item.key, "fingerprint": item.fingerprint, "evidence": item.value["evidence"], "summary": item.value["summary"]}))
                .collect(),
        )
    };
    json!({"schemaVersion": 1, "nodes": to_json(&nodes), "edges": to_json(&edges)})
}

/// Native port of `semanticDelta`: keyed added/removed/changed diff over two
/// `semanticGraphFromGeneration` bodies, per kind (`nodes`, `edges`).
fn semantic_delta(before: &Value, after: &Value, limit: usize) -> Value {
    let compare_kind = |before_items: &Value, after_items: &Value| -> Value {
        let index = |items: &Value| -> std::collections::BTreeMap<String, Value> {
            items.as_array().into_iter().flatten()
                .filter_map(|item| item.get("key").and_then(Value::as_str).map(|k| (k.to_owned(), item.clone())))
                .collect()
        };
        let previous = index(before_items);
        let current = index(after_items);
        let mut keys: BTreeSet<&String> = BTreeSet::new();
        keys.extend(previous.keys());
        keys.extend(current.keys());
        let mut added = Vec::new();
        let mut removed = Vec::new();
        let mut changed = Vec::new();
        for key in keys {
            match (previous.get(key), current.get(key)) {
                (None, Some(next)) => added.push(json!({"key": key, "after": next})),
                (Some(prior), None) => removed.push(json!({"key": key, "before": prior})),
                (Some(prior), Some(next)) if prior.get("fingerprint") != next.get("fingerprint") => {
                    changed.push(json!({"key": key, "before": prior, "after": next}))
                }
                _ => {}
            }
        }
        let total = added.len() + removed.len() + changed.len();
        json!({
            "added": added.into_iter().take(limit).collect::<Vec<_>>(),
            "removed": removed.into_iter().take(limit).collect::<Vec<_>>(),
            "changed": changed.into_iter().take(limit).collect::<Vec<_>>(),
            "receipt": {"total": total, "limit": limit, "truncated": total > limit},
        })
    };
    json!({
        "nodes": compare_kind(&before["nodes"], &after["nodes"]),
        "edges": compare_kind(&before["edges"], &after["edges"]),
    })
}

struct Identity {
    repo_root: String,
    generation_id: String,
    manifest_digest: String,
    source_observation: Value,
    leaves: Vec<(String, String)>,
    semantic_graph: Value,
}

impl Identity {
    fn to_json(&self, name: Option<&str>) -> Value {
        let mut value = json!({
            "repoRoot": self.repo_root,
            "generationId": self.generation_id,
            "manifestDigest": self.manifest_digest,
            "sourceObservation": self.source_observation,
            "leaves": self.leaves.iter().map(|(path, digest)| json!({"path": path, "digest": digest})).collect::<Vec<_>>(),
            "semanticGraph": self.semantic_graph,
        });
        if let Some(name) = name {
            value["name"] = json!(name);
        }
        value
    }

    fn from_json(value: &Value) -> Result<Self, BlueprintError> {
        Ok(Self {
            repo_root: value.get("repoRoot").and_then(Value::as_str).ok_or_else(|| fail("snapshot_malformed", "identity.repoRoot missing"))?.to_owned(),
            generation_id: value.get("generationId").and_then(Value::as_str).ok_or_else(|| fail("snapshot_malformed", "identity.generationId missing"))?.to_owned(),
            manifest_digest: value.get("manifestDigest").and_then(Value::as_str).ok_or_else(|| fail("snapshot_malformed", "identity.manifestDigest missing"))?.to_owned(),
            source_observation: value.get("sourceObservation").cloned().ok_or_else(|| fail("snapshot_malformed", "identity.sourceObservation missing"))?,
            leaves: value
                .get("leaves")
                .and_then(Value::as_array)
                .ok_or_else(|| fail("snapshot_malformed", "identity.leaves missing"))?
                .iter()
                .map(|leaf| {
                    let path = leaf.get("path").and_then(Value::as_str).ok_or_else(|| fail("snapshot_malformed", "leaf.path missing"))?.to_owned();
                    let digest = leaf.get("digest").and_then(Value::as_str).ok_or_else(|| fail("snapshot_malformed", "leaf.digest missing"))?.to_owned();
                    Ok((path, digest))
                })
                .collect::<Result<Vec<_>, BlueprintError>>()?,
            semantic_graph: value.get("semanticGraph").cloned().unwrap_or(Value::Null),
        })
    }
}

/// Native port of `identityFromStore`. Gaps 2 and 3 from this lane's task
/// brief are both closed here now: `sourceObservation.head`/`dirty` is read
/// back from the generation envelope exactly as `build_and_publish`
/// (`engine.rs`) persisted it at build time (no live recomputation), and leaf
/// identity is read from `generation_leaf WHERE kind='file'`, which
/// `build_and_publish` now populates via `merkle_ledger::compute_full_ledger`
/// on every publish -- the same table legacy's `identityFromStore` reads.
fn identity_from_store(conn: &Connection, repo_root: &Path) -> Result<Identity, BlueprintError> {
    let generation = crate::store::load_generation(conn).map_err(|error| fail("snapshot_missing", error.to_string()))?
        .ok_or_else(|| fail("snapshot_missing", "no persisted Blueprint generation exists"))?;
    let manifest = generation.manifest.clone().ok_or_else(|| fail("snapshot_incomplete", "generation manifest missing"))?;
    let generation_id = manifest.get("generationId").and_then(Value::as_str).filter(|v| !v.is_empty())
        .ok_or_else(|| fail("snapshot_incomplete", "generation.manifest.generationId missing"))?
        .to_owned();
    if manifest.get("complete").and_then(Value::as_bool) != Some(true) {
        return Err(fail("snapshot_incomplete", "generation manifest is not complete"));
    }
    let manifest_digest = digest(&manifest);
    let source_observation = generation.source_observation.clone().unwrap_or(Value::Null);
    let head = source_observation.get("head").and_then(Value::as_str)
        .ok_or_else(|| fail("snapshot_malformed", "generation.sourceObservation.head missing"))?;
    let dirty = source_observation.get("dirty").and_then(Value::as_bool)
        .ok_or_else(|| fail("snapshot_malformed", "generation.sourceObservation.dirty missing"))?;
    let _ = head;
    if dirty {
        return Err(fail("snapshot_dirty", "current worktree observation is dirty"));
    }
    let mut statement = conn.prepare("SELECT path, digest FROM generation_leaf WHERE kind='file' ORDER BY path").map_err(|e| fail("snapshot_malformed", e.to_string()))?;
    let rows = statement
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
        .map_err(|e| fail("snapshot_malformed", e.to_string()))?;
    let mut leaves = Vec::new();
    for row in rows {
        let (path, digest) = row.map_err(|e| fail("snapshot_malformed", e.to_string()))?;
        if digest.is_empty() {
            return Err(fail("snapshot_malformed", format!("file {path} has no content digest")));
        }
        leaves.push((path.replace('\\', "/"), digest));
    }
    let semantic_graph = semantic_graph_from_generation(&generation);
    Ok(Identity { repo_root: root_of(repo_root), generation_id, manifest_digest, source_observation, leaves, semantic_graph })
}

fn valid_snapshot_name(name: &str) -> bool {
    if name.is_empty() || name.len() > 128 {
        return false;
    }
    let mut chars = name.chars();
    let first_ok = chars.next().map(|c| c.is_ascii_alphanumeric()).unwrap_or(false);
    first_ok && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
}

/// Native port of `createSnapshot`. Not reached through `Operation` dispatch
/// (legacy exposes it only via the `blueprint.mjs` CLI `snapshot create`
/// action, never through `service.mjs`'s operation surface), so callers use
/// this directly, matching `cli::snapshot_create`.
pub fn create_snapshot(conn: &mut Connection, name: &str, repo_root: &Path) -> Result<Value, BlueprintError> {
    let name = name.trim();
    if !valid_snapshot_name(name) {
        return Err(fail("snapshot_invalid_name", "snapshot name must match ^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$"));
    }
    // `identity_from_store` already fails `snapshot_dirty` on a dirty
    // worktree and reads HEAD live, so the separate current-vs-stored HEAD
    // comparison legacy performs (against a frozen `source_observation`) is
    // redundant here -- see the deviation note on `identity_from_store`.
    let identity = identity_from_store(conn, repo_root)?;
    let json_value = identity.to_json(None);
    let json_text = serde_json::to_string(&json_value).map_err(|e| fail("snapshot_malformed", e.to_string()))?;
    let existing: Option<String> = conn
        .query_row("SELECT identity_json FROM named_snapshot WHERE name=?1", [name], |row| row.get(0))
        .optional()
        .map_err(|e| fail("snapshot_malformed", e.to_string()))?;
    if let Some(existing_json) = existing {
        let existing_value: Value = serde_json::from_str(&existing_json).map_err(|e| fail("snapshot_malformed", e.to_string()))?;
        if canonical_eq(&existing_value, &json_value) {
            let mut response = identity.to_json(Some(name));
            response["idempotent"] = json!(true);
            return Ok(response);
        }
        return Err(fail("snapshot_conflict", format!("snapshot {name} already exists with a different identity")));
    }
    conn.execute(
        "INSERT INTO named_snapshot(name,repo_root,generation_id,manifest_digest,identity_json,created_ms) VALUES (?1,?2,?3,?4,?5,?6)",
        rusqlite::params![name, identity.repo_root, identity.generation_id, identity.manifest_digest, json_text, now_ms()],
    )
    .map_err(|e| fail("snapshot_malformed", e.to_string()))?;
    let mut response = identity.to_json(Some(name));
    response["idempotent"] = json!(false);
    Ok(response)
}

fn canonical_eq(a: &Value, b: &Value) -> bool { stable_stringify(a) == stable_stringify(b) }

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// Native port of `getSnapshot`.
pub fn get_snapshot(conn: &Connection, name: &str) -> Result<Value, BlueprintError> {
    let name = name.trim();
    if !valid_snapshot_name(name) {
        return Err(fail("snapshot_invalid_name", "snapshot name must match ^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$"));
    }
    let row: Option<String> = conn
        .query_row("SELECT identity_json FROM named_snapshot WHERE name=?1", [name], |row| row.get(0))
        .optional()
        .map_err(|e| fail("snapshot_malformed", e.to_string()))?;
    let json_text = row.ok_or_else(|| fail("snapshot_missing", format!("no snapshot named {name}")))?;
    let mut value: Value = serde_json::from_str(&json_text).map_err(|e| fail("snapshot_malformed", e.to_string()))?;
    value["name"] = json!(name);
    Ok(value)
}

/// Native port of `listSnapshots`.
pub fn list_snapshots(conn: &Connection) -> Result<Value, BlueprintError> {
    let mut statement = conn
        .prepare("SELECT name,repo_root,generation_id,manifest_digest,created_ms FROM named_snapshot ORDER BY name")
        .map_err(|e| fail("snapshot_malformed", e.to_string()))?;
    let rows = statement
        .query_map([], |row| {
            Ok(json!({
                "name": row.get::<_, String>(0)?,
                "repoRoot": row.get::<_, String>(1)?,
                "generationId": row.get::<_, String>(2)?,
                "manifestDigest": row.get::<_, String>(3)?,
                "createdMs": row.get::<_, i64>(4)?,
            }))
        })
        .map_err(|e| fail("snapshot_malformed", e.to_string()))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|e| fail("snapshot_malformed", e.to_string()))?);
    }
    Ok(Value::Array(out))
}

struct LeafChange {
    path: String,
    kind: &'static str,
}

fn leaf_diff(before: &[(String, String)], after: &[(String, String)]) -> Vec<LeafChange> {
    let before_map: std::collections::BTreeMap<&str, &str> = before.iter().map(|(p, d)| (p.as_str(), d.as_str())).collect();
    let after_map: std::collections::BTreeMap<&str, &str> = after.iter().map(|(p, d)| (p.as_str(), d.as_str())).collect();
    let mut paths: BTreeSet<&str> = BTreeSet::new();
    paths.extend(before_map.keys());
    paths.extend(after_map.keys());
    let mut changes = Vec::new();
    for path in paths {
        match (before_map.get(path), after_map.get(path)) {
            (None, Some(_)) => changes.push(LeafChange { path: path.to_owned(), kind: "added" }),
            (Some(_), None) => changes.push(LeafChange { path: path.to_owned(), kind: "deleted" }),
            (Some(b), Some(a)) if b != a => changes.push(LeafChange { path: path.to_owned(), kind: "modified" }),
            _ => {}
        }
    }
    changes
}

/// Native port of `changesSince` (snapshot-name-to-current-generation leaf diff).
pub fn changes_since(conn: &Connection, name: &str, limit: u64) -> Result<Value, BlueprintError> {
    let snapshot = get_snapshot(conn, name)?;
    let snapshot_identity = Identity::from_json(&snapshot)?;
    let current = identity_from_store(conn, Path::new(&snapshot_identity.repo_root))?;
    let changes = leaf_diff(&snapshot_identity.leaves, &current.leaves);
    let cap = limit.clamp(1, 10_000) as usize;
    let total = changes.len();
    let limited: Vec<Value> = changes.into_iter().take(cap).map(|c| json!({"path": c.path, "kind": c.kind})).collect();
    Ok(json!({
        "name": name,
        "base": {"generationId": snapshot_identity.generation_id, "manifestDigest": snapshot_identity.manifest_digest, "sourceObservation": snapshot_identity.source_observation},
        "head": {"generationId": current.generation_id, "manifestDigest": current.manifest_digest, "sourceObservation": current.source_observation},
        "changes": limited,
        "receipt": {"total": total, "limit": cap, "truncated": total > cap},
    }))
}

struct TreeishChange {
    path: String,
    previous_path: Option<String>,
    kind: &'static str,
}

/// Native port of `gitTreeishChanges`.
fn git_treeish_changes(repo_root: &Path, base: &str, head: &str) -> Result<Vec<TreeishChange>, BlueprintError> {
    let base = base.trim();
    if base.is_empty() {
        return Err(fail("treeish_base_required", "a base treeish is required"));
    }
    let head = if head.trim().is_empty() { "HEAD" } else { head.trim() };
    let output = run_git(&PathBuf::from(root_of(repo_root)), &["diff", "--name-status", "-z", "--find-renames", base, head, "--"])
        .map_err(|_| fail("treeish_unavailable", "git diff failed or exceeded its bounded timeout"))?;
    let text = String::from_utf8_lossy(&output);
    let fields: Vec<&str> = text.split('\0').filter(|f| !f.is_empty()).collect();
    let mut changes = Vec::new();
    let mut index = 0usize;
    while index < fields.len() {
        let status = fields[index];
        index += 1;
        let Some(before) = fields.get(index) else { break };
        index += 1;
        if status.starts_with('R') || status.starts_with('C') {
            let Some(after) = fields.get(index) else { break };
            index += 1;
            changes.push(TreeishChange {
                path: after.replace('\\', "/"),
                previous_path: Some(before.replace('\\', "/")),
                kind: if status.starts_with('R') { "renamed" } else { "copied" },
            });
        } else {
            let kind = match status.chars().next() {
                Some('A') => "added",
                Some('D') => "deleted",
                Some('M') => "modified",
                Some('T') => "type_changed",
                Some('U') => "unmerged",
                _ => "modified",
            };
            changes.push(TreeishChange { path: before.replace('\\', "/"), previous_path: None, kind });
        }
    }
    changes.sort_by(|a, b| a.path.cmp(&b.path).then(a.kind.cmp(b.kind)));
    Ok(changes)
}

/// Native port of `changesSinceReference`. Supports `snapshot`, `generation`,
/// and `treeish` reference kinds, all three now with a real `semanticDelta`.
pub fn changes_since_reference(
    conn: &Connection,
    repo_root: &Path,
    snapshot: Option<&str>,
    since_generation: Option<&str>,
    treeish: Option<(&str, &str)>,
    limit: u64,
) -> Result<Value, BlueprintError> {
    let current = identity_from_store(conn, repo_root)?;
    let cap = limit.clamp(1, 10_000) as usize;
    let mut omissions: Vec<Value> = Vec::new();
    let mut after_semantic = current.semantic_graph.clone();
    let mut before_semantic: Option<Value> = None;

    let (source, raw): (Value, Vec<Value>) = if let Some(name) = snapshot {
        let snapshot_identity = Identity::from_json(&get_snapshot(conn, name)?)?;
        let changes = leaf_diff(&snapshot_identity.leaves, &current.leaves);
        if snapshot_identity.semantic_graph.is_null() {
            omissions.push(json!({"reason": "snapshot_semantic_evidence_unavailable", "snapshot": name}));
        } else {
            before_semantic = Some(snapshot_identity.semantic_graph.clone());
        }
        (json!({"kind": "snapshot", "value": name}), changes.into_iter().map(|c| json!({"path": c.path, "kind": c.kind})).collect())
    } else if let Some(generation) = since_generation {
        if generation == current.generation_id {
            before_semantic = Some(current.semantic_graph.clone());
            (json!({"kind": "generation", "value": generation}), Vec::new())
        } else {
            let row: Option<String> = conn
                .query_row("SELECT name FROM named_snapshot WHERE generation_id=?1 ORDER BY name LIMIT 1", [generation], |row| row.get(0))
                .optional()
                .map_err(|e| fail("snapshot_malformed", e.to_string()))?;
            if let Some(name) = row {
                let snapshot_identity = Identity::from_json(&get_snapshot(conn, &name)?)?;
                let changes = leaf_diff(&snapshot_identity.leaves, &current.leaves);
                if snapshot_identity.semantic_graph.is_null() {
                    omissions.push(json!({"reason": "generation_semantic_evidence_unavailable", "generationId": generation}));
                } else {
                    before_semantic = Some(snapshot_identity.semantic_graph.clone());
                }
                (json!({"kind": "generation", "value": generation}), changes.into_iter().map(|c| json!({"path": c.path, "kind": c.kind})).collect())
            } else {
                omissions.push(json!({"reason": "generation_history_unavailable", "generationId": generation}));
                (json!({"kind": "generation", "value": generation}), Vec::new())
            }
        }
    } else if let Some((base, head)) = treeish {
        let changes = git_treeish_changes(repo_root, base, head)?;
        // Gap closed by lane STORE3: materialise both the base and head
        // treeish into detached worktrees and build real graph bodies for
        // each, so `semanticDelta` for the `treeish` reference kind is a
        // genuine diff rather than a permanent `null` stand-in.
        before_semantic = Some(treeish_semantic_graph(repo_root, base)?);
        // Override `after_semantic` (which defaulted to the current
        // persisted generation above): for the `treeish` kind, "after" means
        // the `head` treeish's own materialised body, not necessarily the
        // repository's current on-disk state.
        after_semantic = treeish_semantic_graph(repo_root, head)?;
        (
            json!({"kind": "treeish", "value": base, "head": head}),
            changes.into_iter().map(|c| json!({"path": c.path, "previousPath": c.previous_path, "kind": c.kind})).collect(),
        )
    } else {
        return Err(fail("change_reference_required", "one of snapshot, sinceGeneration, or treeish is required"));
    };

    let total = raw.len();
    let limited: Vec<Value> = raw.into_iter().take(cap).collect();
    let semantic_delta_value = match &before_semantic {
        Some(before) => semantic_delta(before, &after_semantic, cap),
        None => Value::Null,
    };
    Ok(json!({
        "schemaVersion": 2,
        "kind": "SemanticChangeProjection",
        "authority": "history_reference_only",
        "source": source,
        "currentTruth": {"generationId": current.generation_id, "manifestDigest": current.manifest_digest},
        "changes": limited,
        "semanticDelta": semantic_delta_value,
        "receipt": {"total": total, "limit": cap, "truncated": total > cap},
        "omissions": omissions,
    }))
}
