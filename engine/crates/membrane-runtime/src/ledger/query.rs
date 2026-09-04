//! Source-set-scoped document retrieval. Eligibility precedes ranking.
//!
//! The temporary range table belongs to the serialized Ledger owner request;
//! it is not persisted authority. SQL, literal verification, and raw reads all
//! consume the same approved source set and inherited work budget.
use super::{index, limits::WorkBudget, policy::SourcePolicy, resolve, LedgerDb};
use membrane_protocol::ReadPathV1;
use rusqlite::{params, OptionalExtension};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub const MAX_HITS: usize = 32;
const MAX_DOCUMENTS: usize = 16_384;
const MAX_POOL: usize = 256;

/// Constructed only by the runtime's authorization/source-grant boundary.
/// `None` means a separately authorized repository-wide source grant;
/// `Some([])` means no permitted ranges, never unrestricted access.
#[derive(Clone)]
pub(crate) struct QueryScope {
    pub root: String,
    pub ranges: Option<Vec<ReadPathV1>>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LedgerHit {
    pub doc_id: String,
    pub node_id: String,
    pub source_ref: String,
    pub anchor_id: String,
    pub expected_content_hash: String,
    pub expected_revision: String,
    pub expected_span_hash: String,
    pub ledger_generation: i64,
    pub source_kind: String,
    pub node_kind: String,
    pub start_byte: usize,
    pub end_byte: usize,
    pub lane: String,
    pub score: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub literal_range: Option<(usize, usize)>,
}
impl LedgerHit {
    pub fn resolve_request(&self) -> resolve::ResolveRequest {
        resolve::ResolveRequest { doc_id: Some(self.doc_id.clone()), node_id: Some(self.node_id.clone()),
            source_ref: self.source_ref.clone(), anchor_id: self.anchor_id.clone(),
            expected_content_hash: self.expected_content_hash.clone(),
            expected_revision: Some(self.expected_revision.clone()), expected_span_hash: Some(self.expected_span_hash.clone()),
            ledger_generation: Some(self.ledger_generation), continuation_cursor: None,
            max_bytes: resolve::MAX_READ_BYTES }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryResult {
    pub schema_version: u32,
    pub hits: Vec<LedgerHit>,
    pub complete: bool,
    pub omissions: Vec<String>,
    pub publication_generation: i64,
    pub policy_digest: String,
    pub lane: String,
    pub query_digest: String,
    pub source_bytes_checked: usize,
}

#[derive(Clone)]
struct Document { id: String, path: String, generation: i64 }
#[derive(Clone)]
struct Node { doc: String, id: String, kind: String, start: usize, end: usize, span: String, score: f64, exact: bool }
#[derive(Clone)]
struct Allowed { doc: String, start: usize, end: usize }

fn eligible(
    db: &LedgerDb, scope: &QueryScope, budget: &WorkBudget,
) -> Result<(Vec<Document>, Vec<Allowed>, SourcePolicy, Vec<String>), String> {
    let root = resolve::normalized_root(Path::new(&scope.root)).map_err(|e| e.to_string())?;
    if root != scope.root { return Err("ledger_root_binding_changed".into()); }
    let mut policy = SourcePolicy::new(Path::new(&root))?;
    let rows = {
        let conn = db.lock();
        let mut statement = conn.prepare(
            "SELECT doc_id,path,index_generation FROM ledger_doc_artifacts
             WHERE repository_root=?1 AND lifecycle_state='active' AND sensitivity='normal'
             ORDER BY path LIMIT 16385").map_err(|e| e.to_string())?;
        let result = statement.query_map([&root], |r| Ok(Document {
            id: r.get(0)?, path: r.get(1)?, generation: r.get(2)?,
        })).map_err(|e| e.to_string())?.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
        result
    };
    if rows.len() > MAX_DOCUMENTS { return Err("ledger_document_budget_exhausted".into()); }
    let mut documents = Vec::new();
    let mut ranges = Vec::new();
    let mut omissions = Vec::new();
    for document in rows {
        budget.visit()?;
        let erased: bool = db.lock().query_row(
            "SELECT EXISTS(SELECT 1 FROM ledger_erasure_fences WHERE repository_root=?1 AND path_digest=?2)",
            params![root,resolve::digest(document.path.as_bytes())],|r|r.get(0)).map_err(|e|e.to_string())?;
        if erased || !policy.allows(&document.path, false, budget)? { continue; }
        if let Some(granted) = &scope.ranges {
            let matching = granted.iter().filter(|r| r.path == document.path).collect::<Vec<_>>();
            if matching.is_empty() { continue; }
            let source = match resolve::load_source(db, &root, &document.id) {
                Ok(source) => source,
                Err(error) => { omissions.push(format!("source_{error}")); continue; }
            };
            budget.charge_bytes(source.markdown.len())?;
            // Raw Office/PDF line ranges do not authorize invented normalized
            // coordinates. Imported snapshots need their explicit whole-source grant.
            if source.imported { omissions.push("snapshot_range_unsupported".into()); continue; }
            let starts = std::iter::once(0).chain(source.markdown.match_indices('\n').map(|(i, _)| i + 1)).collect::<Vec<_>>();
            for grant in matching {
                if grant.start_line == 0 || grant.end_line < grant.start_line { return Err("ledger_range_grant_invalid".into()); }
                let Some(start) = starts.get(grant.start_line as usize - 1).copied() else {
                    omissions.push("source_range_stale".into()); continue;
                };
                let end = starts.get(grant.end_line as usize).copied().unwrap_or(source.markdown.len());
                ranges.push(Allowed { doc: document.id.clone(), start, end });
            }
        } else {
            ranges.push(Allowed { doc: document.id.clone(), start: 0, end: i64::MAX as usize });
        }
        documents.push(document);
    }
    policy.revalidate(budget)?;
    Ok((documents, ranges, policy, omissions))
}

fn install_ranges(db: &LedgerDb, ranges: &[Allowed]) -> Result<(), String> {
    let conn = db.lock();
    conn.execute_batch("CREATE TEMP TABLE IF NOT EXISTS ledger_query_scope (
        doc_id TEXT NOT NULL, start_byte INTEGER NOT NULL, end_byte INTEGER NOT NULL,
        PRIMARY KEY(doc_id,start_byte,end_byte)); DELETE FROM ledger_query_scope;")
        .map_err(|e| e.to_string())?;
    let mut insert = conn.prepare("INSERT OR IGNORE INTO ledger_query_scope VALUES (?1,?2,?3)").map_err(|e| e.to_string())?;
    for range in ranges { insert.execute(params![range.doc, range.start as i64, range.end as i64]).map_err(|e| e.to_string())?; }
    Ok(())
}

fn node_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Node> {
    let start: i64 = row.get(3)?;
    let end: i64 = row.get(4)?;
    if start < 0 || end < start { return Err(rusqlite::Error::InvalidQuery); }
    Ok(Node { doc: row.get(0)?, id: row.get(1)?, kind: row.get(2)?, start: start as usize,
        end: end as usize, span: row.get(5)?, score: row.get(6)?, exact: row.get(7)? })
}

fn nodes_for_document(db: &LedgerDb, document: &Document, sections_only: bool) -> Result<Vec<Node>, String> {
    let conn = db.lock();
    let mut statement = conn.prepare(
        "SELECT n.doc_id,n.node_id,n.node_kind,n.source_start_byte,n.source_end_byte,n.span_hash,0.0,0
         FROM ledger_nodes n WHERE n.doc_id=?1 AND n.ledger_generation=?2
         AND (?3=0 OR n.node_kind IN ('document','section','preamble','frontmatter'))
         AND EXISTS(SELECT 1 FROM ledger_query_scope allowed WHERE allowed.doc_id=n.doc_id
                    AND n.source_start_byte>=allowed.start_byte AND n.source_end_byte<=allowed.end_byte)
         ORDER BY n.ordinal LIMIT 32769").map_err(|e| e.to_string())?;
    let rows = statement.query_map(params![document.id, document.generation, sections_only as i32], node_row)
        .map_err(|e| e.to_string())?.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
    if rows.len() > 32768 { return Err("ledger_node_budget_exhausted".into()); }
    Ok(rows)
}

fn fts_nodes(db: &LedgerDb, root: &str, query: &str) -> Result<Vec<Node>, String> {
    let terms = index::query_terms(query);
    if terms.len() > 128 { return Err("ledger_query_budget_exhausted".into()); }
    if terms.is_empty() { return Ok(Vec::new()); }
    let expression = terms.iter().map(|t| format!("\"{}\"", t.replace('"', "\"\""))).collect::<Vec<_>>().join(" OR ");
    let conn = db.lock();
    let mut statement = conn.prepare(
        "SELECT n.doc_id,n.node_id,n.node_kind,n.source_start_byte,n.source_end_byte,n.span_hash,
                -bm25(ledger_node_fts,0.0,0.0,8.0,6.0,5.0,1.0,4.0),0
         FROM ledger_node_fts f JOIN ledger_nodes n ON n.doc_id=f.doc_id AND n.node_id=f.node_id
         JOIN ledger_doc_artifacts a ON a.doc_id=n.doc_id
         WHERE ledger_node_fts MATCH ?1 AND a.repository_root=?2
           AND a.lifecycle_state='active' AND a.sensitivity='normal'
           AND n.ledger_generation=a.index_generation AND n.source_revision=a.revision
           AND n.projection_schema_version=?3
           AND EXISTS(SELECT 1 FROM ledger_query_scope allowed WHERE allowed.doc_id=n.doc_id
                      AND n.source_start_byte>=allowed.start_byte AND n.source_end_byte<=allowed.end_byte)
         ORDER BY 7 DESC,a.doc_id,n.ordinal LIMIT 257").map_err(|e| e.to_string())?;
    let rows = statement.query_map(params![expression, root, index::PROJECTION_SCHEMA_VERSION], node_row)
        .map_err(|e| e.to_string())?.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
    Ok(rows)
}

fn exact_nodes(db: &LedgerDb, root: &str, query: &str) -> Result<Vec<Node>, String> {
    let conn = db.lock();
    let mut statement = conn.prepare(
        "SELECT n.doc_id,n.node_id,n.node_kind,n.source_start_byte,n.source_end_byte,n.span_hash,1.0,1
         FROM ledger_nodes n JOIN ledger_doc_artifacts a ON a.doc_id=n.doc_id
         WHERE a.repository_root=?1 AND a.lifecycle_state='active' AND a.sensitivity='normal'
           AND n.ledger_generation=a.index_generation AND n.source_revision=a.revision
           AND n.projection_schema_version=?3
           AND (n.anchor_id=?2 OR n.node_id=?2 OR n.heading=?2
                OR ((a.path=?2 OR a.title=?2) AND n.node_kind IN ('document','section','preamble')))
           AND EXISTS(SELECT 1 FROM ledger_query_scope allowed WHERE allowed.doc_id=n.doc_id
                      AND n.source_start_byte>=allowed.start_byte AND n.source_end_byte<=allowed.end_byte)
         ORDER BY a.doc_id,n.ordinal LIMIT 257").map_err(|e| e.to_string())?;
    let rows = statement.query_map(params![root, query, index::PROJECTION_SCHEMA_VERSION], node_row)
        .map_err(|e| e.to_string())?.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
    Ok(rows)
}

fn source_hit(source: &resolve::Source, node: &Node, lane: &str, score: f64) -> Result<LedgerHit, String> {
    let span = source.markdown.get(node.start..node.end).ok_or("ledger_span_stale")?;
    if resolve::digest(span.as_bytes()) != node.span { return Err("ledger_span_stale".into()); }
    Ok(LedgerHit { doc_id: source.doc_id.clone(), node_id: node.id.clone(),
        source_ref: if source.imported { format!("ledger://doc/{}", source.doc_id) }
            else { format!("doc://repo/worktree/{}", source.path) },
        anchor_id: node.id.clone(), expected_content_hash: source.raw_hash.clone(),
        expected_revision: source.revision.clone(), expected_span_hash: node.span.clone(),
        ledger_generation: source.generation, source_kind: if source.imported { "imported_snapshot" } else { "worktree" }.into(),
        node_kind: node.kind.clone(), start_byte: node.start, end_byte: node.end,
        lane: lane.to_owned(), score, literal_range: None })
}

pub(crate) fn search(db: &LedgerDb, scope: &QueryScope, query: &str, k: usize, literal: bool, budget: &WorkBudget) -> Result<QueryResult, String> {
    if query.trim().is_empty() || query.len() > 4096 || k == 0 || k > MAX_HITS {
        return Err("ledger_query_invalid".into());
    }
    budget.check()?;
    let (documents, ranges, policy, mut omissions) = eligible(db, scope, budget)?;
    install_ranges(db, &ranges)?;
    let generation = documents.iter().map(|d| d.generation).max().unwrap_or(0);
    let mode = index::recall_mode(db)?;
    let lane = if literal { "literal" } else { mode.storage_name() };
    let mut hits = Vec::new();
    let mut sources = BTreeMap::new();
    let mut candidates = Vec::new();
    if !literal { candidates.extend(exact_nodes(db, &scope.root, query.trim())?); }
    if !literal && mode == index::LedgerRecallMode::LedgerFts {
        let rows = fts_nodes(db, &scope.root, query)?;
        if rows.len() > MAX_POOL { omissions.push("candidate_pool_truncated".into()); }
        candidates.extend(rows.into_iter().take(MAX_POOL));
    } else {
        let terms = index::query_terms(query);
        if terms.len() > 128 { return Err("ledger_query_budget_exhausted".into()); }
        for document in &documents {
            budget.visit()?;
            let source = match resolve::load_source(db, &scope.root, &document.id) {
                Ok(source) => source,
                Err(error) => { omissions.push(format!("source_{error}")); continue; }
            };
            budget.charge_bytes(source.markdown.len())?;
            let mut best: Option<Node> = None;
            for mut node in nodes_for_document(db, document, !literal)? {
                budget.visit()?;
                let Some(text) = source.markdown.get(node.start..node.end) else {
                    omissions.push("span_stale".into()); continue;
                };
                if literal {
                    if !matches!(node.kind.as_str(), "fenced_code" | "indented_code" | "paragraph" | "table_cell") { continue; }
                    if let Some(offset) = text.find(query) {
                        let mut hit = source_hit(&source, &node, "literal", 1.0)?;
                        hit.literal_range = Some((node.start + offset, node.start + offset + query.len()));
                        hits.push(hit);
                        if hits.len() > MAX_POOL { omissions.push("literal_results_truncated".into()); break; }
                    }
                } else {
                    let normalized = index::normalize_query(text);
                    node.score = terms.iter().map(|term| normalized.match_indices(term).count()).sum::<usize>() as f64;
                    if node.score > 0.0 && best.as_ref().is_none_or(|old| node.score > old.score
                        || (node.score == old.score && node.end-node.start < old.end-old.start)) { best = Some(node); }
                }
            }
            if let Some(node) = best { candidates.push(node); }
            sources.insert(document.id.clone(), source);
            if hits.len() > MAX_POOL { break; }
        }
    }
    candidates.sort_by(|a,b| b.exact.cmp(&a.exact).then_with(|| b.score.total_cmp(&a.score))
        .then_with(|| (a.end-a.start).cmp(&(b.end-b.start))).then_with(|| a.doc.cmp(&b.doc)).then_with(|| a.id.cmp(&b.id)));
    let mut seen = BTreeSet::new();
    for node in candidates {
        budget.visit()?;
        if !seen.insert(node.id.clone()) { continue; }
        if !sources.contains_key(&node.doc) {
            match resolve::load_source(db, &scope.root, &node.doc) {
                Ok(source) => { budget.charge_bytes(source.markdown.len())?; sources.insert(node.doc.clone(), source); }
                Err(error) => { omissions.push(format!("source_{error}")); continue; }
            }
        }
        let source = &sources[&node.doc];
        let score = if node.exact { 1.0 } else { 1.0 / (2.0 + hits.len() as f64) };
        match source_hit(source, &node, if node.exact { "exact" } else { lane }, score) {
            Ok(hit) => hits.push(hit),
            Err(_) => omissions.push("span_stale".into()),
        }
        if hits.len() >= MAX_POOL { break; }
    }
    hits.sort_by(|a,b| b.score.total_cmp(&a.score).then_with(|| a.doc_id.cmp(&b.doc_id))
        .then_with(|| (a.end_byte-a.start_byte).cmp(&(b.end_byte-b.start_byte))).then_with(|| a.node_id.cmp(&b.node_id)));
    let mut unique: Vec<LedgerHit> = Vec::new();
    for hit in hits {
        if unique.iter().any(|old| old.doc_id == hit.doc_id && old.start_byte <= hit.start_byte && hit.end_byte <= old.end_byte) { continue; }
        unique.push(hit);
        if unique.len() == k { break; }
    }
    policy.revalidate(budget)?;
    budget.check()?;
    omissions.sort(); omissions.dedup();
    Ok(QueryResult { schema_version: 1, hits: unique, complete: omissions.is_empty(), omissions,
        publication_generation: generation, policy_digest: policy.digest(), lane: lane.to_owned(),
        query_digest: resolve::digest(query.as_bytes()), source_bytes_checked: budget.consumed_bytes() })
}
