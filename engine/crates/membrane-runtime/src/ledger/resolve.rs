//! One source-bound resolver for registered Markdown and imported snapshots.
//!
//! Callers supply a canonical, authorized repository root. This module never
//! discovers a root from cwd, opens another owner's store, or fetches a URL.
//! Raw authority, normalized projection, and exact node spans are verified
//! independently. A cursor is a locator, never an authorization credential.

use super::{identifier::WorktreeDocRef, outline, LedgerDb};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::Read;
use std::path::{Component, Path};

pub const MAX_SOURCE_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_READ_BYTES: usize = 12_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolveError {
    #[error("stale")] Stale,
    #[error("missing")] Missing,
    #[error("denied")] Denied,
    #[error("ineligible")] Ineligible,
    #[error("unsupported")] Unsupported,
    #[error("ambiguous")] Ambiguous,
    #[error("unavailable")] Unavailable,
    #[error("budget_exhausted")] BudgetExhausted,
    #[error("invalid_cursor")] InvalidCursor,
    #[error("relocated")] Relocated,
}

impl From<rusqlite::Error> for ResolveError {
    fn from(_: rusqlite::Error) -> Self { Self::Unavailable }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResolveRequest {
    #[serde(default)] pub doc_id: Option<String>,
    #[serde(default)] pub node_id: Option<String>,
    pub source_ref: String,
    pub anchor_id: String,
    pub expected_content_hash: String,
    #[serde(default)] pub expected_revision: Option<String>,
    #[serde(default)] pub expected_span_hash: Option<String>,
    #[serde(default)] pub ledger_generation: Option<i64>,
    #[serde(default)] pub continuation_cursor: Option<String>,
    #[serde(default = "default_read_bytes")] pub max_bytes: usize,
}
fn default_read_bytes() -> usize { MAX_READ_BYTES }

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedDocument {
    pub raw_content_hash: String,
    pub projection_content_hash: String,
    pub source_kind: &'static str,
    pub source_revision: String,
    pub ledger_generation: i64,
    pub doc_id: String,
    pub node_id: Option<String>,
    pub converter: Option<serde_json::Value>,
    pub losses: serde_json::Value,
    pub omissions: serde_json::Value,
    pub read: outline::DocReadV1,
}

pub(crate) struct Source {
    pub doc_id: String,
    pub path: String,
    pub revision: String,
    pub raw_hash: String,
    pub projection_hash: String,
    pub generation: i64,
    pub markdown: String,
    pub imported: bool,
    pub converter: Option<serde_json::Value>,
    pub losses: serde_json::Value,
    pub omissions: serde_json::Value,
}

pub(crate) fn digest(bytes: &[u8]) -> String { hex::encode(Sha256::digest(bytes)) }
pub(crate) fn hash_matches(expected: &str, actual: &str) -> bool {
    let expected = expected.strip_prefix("sha256:").unwrap_or(expected);
    expected.len() == 64 && expected.bytes().all(|b| b.is_ascii_hexdigit())
        && expected.eq_ignore_ascii_case(actual)
}

pub(crate) fn normalized_root(root: &Path) -> Result<String, ResolveError> {
    if !root.is_absolute() { return Err(ResolveError::Denied); }
    let root = root.canonicalize().map_err(|_| ResolveError::Unavailable)?;
    if !root.is_dir() { return Err(ResolveError::Denied); }
    root.to_str().map(|s| s.replace('\\', "/")).ok_or(ResolveError::Unsupported)
}

pub(crate) fn confined_bytes(root: &Path, relative: &str) -> Result<Vec<u8>, ResolveError> {
    if relative.is_empty() || relative.len() > 4096 || relative.contains('\\')
        || relative.chars().any(char::is_control)
        || Path::new(relative).components().any(|c| !matches!(c, Component::Normal(_)))
    { return Err(ResolveError::Denied); }
    let root = root.canonicalize().map_err(|_| ResolveError::Unavailable)?;
    let mut path = root.clone();
    // Check every component, not only the final target. Symlinks are not a
    // source-owner transition and cannot broaden the registered source set.
    for component in Path::new(relative).components() {
        path.push(component.as_os_str());
        let metadata = std::fs::symlink_metadata(&path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound { ResolveError::Missing }
            else { ResolveError::Unavailable }
        })?;
        if metadata.file_type().is_symlink() { return Err(ResolveError::Denied); }
    }
    let canonical = path.canonicalize().map_err(|_| ResolveError::Unavailable)?;
    if !canonical.starts_with(&root) { return Err(ResolveError::Denied); }
    let file = File::open(&canonical).map_err(|_| ResolveError::Unavailable)?;
    let metadata = file.metadata().map_err(|_| ResolveError::Unavailable)?;
    if !metadata.is_file() { return Err(ResolveError::Denied); }
    if metadata.len() > MAX_SOURCE_BYTES as u64 { return Err(ResolveError::BudgetExhausted); }
    let mut bytes = Vec::new();
    file.take((MAX_SOURCE_BYTES + 1) as u64).read_to_end(&mut bytes)
        .map_err(|_| ResolveError::Unavailable)?;
    if bytes.len() > MAX_SOURCE_BYTES { return Err(ResolveError::BudgetExhausted); }
    if path.canonicalize().map_err(|_| ResolveError::Unavailable)? != canonical {
        return Err(ResolveError::Stale);
    }
    Ok(bytes)
}

/// Load one registered source, scoped in SQL before any payload is acquired.
/// Imported bytes are immutable snapshots; their presence never proves that
/// an external live source is current.
pub(crate) fn load_source(db: &LedgerDb, root: &str, doc_id: &str) -> Result<Source, ResolveError> {
    let (path, revision, raw_hash, generation, lifecycle, sensitivity) = db.lock().query_row(
        "SELECT path,revision,content_hash,index_generation,lifecycle_state,sensitivity
         FROM ledger_doc_artifacts WHERE repository_root=?1 AND doc_id=?2",
        params![root, doc_id],
        |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?,
                 r.get::<_, i64>(3)?, r.get::<_, String>(4)?, r.get::<_, String>(5)?)),
    ).optional()?.ok_or(ResolveError::Missing)?;
    if lifecycle != "active" || sensitivity != "normal" { return Err(ResolveError::Ineligible); }
    let conversion = db.lock().query_row(
        "SELECT raw_input,raw_sha256,markdown,markdown_sha256,converter,converter_version,
                config_digest,losses_json,omissions_json,source_revision,ledger_generation
         FROM ledger_document_conversions WHERE doc_id=?1",
        [doc_id], |r| Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, String>(1)?,
            r.get::<_, String>(2)?, r.get::<_, String>(3)?, r.get::<_, String>(4)?,
            r.get::<_, String>(5)?, r.get::<_, String>(6)?, r.get::<_, String>(7)?,
            r.get::<_, String>(8)?, r.get::<_, String>(9)?, r.get::<_, i64>(10)?)),
    ).optional()?;
    if let Some((raw, stored_raw, markdown, stored_markdown, converter, version, config,
                 losses, omissions, source_revision, source_generation)) = conversion {
        if raw.len() > MAX_SOURCE_BYTES || markdown.len() > MAX_SOURCE_BYTES {
            return Err(ResolveError::BudgetExhausted);
        }
        if source_revision != revision || source_generation != generation
            || !hash_matches(&raw_hash, &digest(&raw)) || stored_raw != raw_hash
            || !hash_matches(&stored_markdown, &digest(markdown.as_bytes()))
        { return Err(ResolveError::Stale); }
        if converter.is_empty() || version.is_empty() || config.len() != 64
            || !config.bytes().all(|b| b.is_ascii_hexdigit())
        { return Err(ResolveError::Unsupported); }
        return Ok(Source { doc_id: doc_id.to_owned(), path, revision, raw_hash,
            projection_hash: stored_markdown, generation, markdown, imported: true,
            converter: Some(serde_json::json!({"name":converter,"version":version,"configDigest":config})),
            losses: serde_json::from_str(&losses).map_err(|_| ResolveError::Unsupported)?,
            omissions: serde_json::from_str(&omissions).map_err(|_| ResolveError::Unsupported)?,
        });
    }
    let bytes = confined_bytes(Path::new(root), &path)?;
    if !hash_matches(&raw_hash, &digest(&bytes)) { return Err(ResolveError::Stale); }
    let markdown = String::from_utf8(bytes).map_err(|_| ResolveError::Unsupported)?;
    Ok(Source { doc_id: doc_id.to_owned(), path, revision, projection_hash: raw_hash.clone(),
        raw_hash, generation, markdown, imported: false, converter: None,
        losses: serde_json::json!([]), omissions: serde_json::json!([]) })
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Cursor {
    version: u8, root_digest: String, doc_id: String, node_id: String,
    raw_hash: String, projection_hash: String, revision: String,
    span_hash: String, start: usize, end: usize, offset: usize,
}

pub fn resolve(db: &LedgerDb, repository_root: &Path, request: &ResolveRequest)
    -> Result<ResolvedDocument, ResolveError>
{
    let root = normalized_root(repository_root)?;
    let path_ref = WorktreeDocRef::parse(&request.source_ref).ok();
    let uri_doc = request.source_ref.strip_prefix("ledger://doc/");
    if path_ref.is_none() && uri_doc.is_none() { return Err(ResolveError::Denied); }
    if let (Some(id), Some(uri_id)) = (request.doc_id.as_deref(), uri_doc) {
        if id != uri_id { return Err(ResolveError::Denied); }
    }
    let requested_id = request.doc_id.as_deref().or(uri_doc);
    let relative = path_ref.as_ref().map(|r| r.relative_path()).unwrap_or("");
    let doc_id: String = db.lock().query_row(
        "SELECT doc_id FROM ledger_doc_artifacts WHERE repository_root=?1
         AND ((?2 IS NOT NULL AND doc_id=?2) OR (?2 IS NULL AND path=?3))",
        params![root, requested_id, relative], |r| r.get(0),
    ).optional()?.ok_or(ResolveError::Missing)?;
    let source = load_source(db, &root, &doc_id)?;
    if path_ref.is_some() && source.path != relative { return Err(ResolveError::Denied); }
    if !hash_matches(&request.expected_content_hash, &source.raw_hash)
        || request.expected_revision.as_ref().is_some_and(|r| r != &source.revision)
        || request.ledger_generation.is_some_and(|g| g != source.generation)
    { return Err(ResolveError::Stale); }
    let maximum = request.max_bytes.min(MAX_READ_BYTES);
    if maximum == 0 { return Err(ResolveError::BudgetExhausted); }
    let node_id = request.node_id.as_deref().or_else(|| {
        request.anchor_id.starts_with("ledger.node:").then_some(request.anchor_id.as_str())
    });
    let read = if let Some(node_id) = node_id {
        let node = db.lock().query_row(
            "SELECT source_start_byte,source_end_byte,span_hash,heading_path,parent_id,
                    projection_schema_version,ordinal
             FROM ledger_nodes WHERE doc_id=?1 AND node_id=?2
               AND source_revision=?3 AND ledger_generation=?4",
            params![doc_id, node_id, source.revision, source.generation],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, String>(2)?,
                r.get::<_, String>(3)?, r.get::<_, Option<String>>(4)?,
                r.get::<_, String>(5)?, r.get::<_, i64>(6)?)),
        ).optional()?.ok_or(ResolveError::Relocated)?;
        let (start, end, span_hash, headings, parent, version, ordinal) = node;
        if version != super::index::PROJECTION_SCHEMA_VERSION { return Err(ResolveError::Unsupported); }
        if start < 0 || end < start { return Err(ResolveError::Stale); }
        let (start, end) = (start as usize, end as usize);
        let full = source.markdown.get(start..end).ok_or(ResolveError::Stale)?;
        if digest(full.as_bytes()) != span_hash
            || request.expected_span_hash.as_ref().is_some_and(|h| !hash_matches(h, &span_hash))
        { return Err(ResolveError::Stale); }
        let mut cursor = Cursor { version: 1, root_digest: digest(root.as_bytes()),
            doc_id: doc_id.clone(), node_id: node_id.to_owned(), raw_hash: source.raw_hash.clone(),
            projection_hash: source.projection_hash.clone(), revision: source.revision.clone(),
            span_hash: span_hash.clone(), start, end, offset: 0 };
        if let Some(encoded) = &request.continuation_cursor {
            if encoded.len() > 8192 { return Err(ResolveError::InvalidCursor); }
            let bytes = hex::decode(encoded.strip_prefix("ledger1:").ok_or(ResolveError::InvalidCursor)?)
                .map_err(|_| ResolveError::InvalidCursor)?;
            let observed: Cursor = serde_json::from_slice(&bytes).map_err(|_| ResolveError::InvalidCursor)?;
            cursor.offset = observed.offset;
            if observed != cursor || cursor.offset > full.len() || !full.is_char_boundary(cursor.offset) {
                return Err(ResolveError::InvalidCursor);
            }
        }
        let mut page_end = cursor.offset.saturating_add(maximum).min(full.len());
        while !full.is_char_boundary(page_end) { page_end -= 1; }
        if page_end == cursor.offset && page_end != full.len() { return Err(ResolveError::BudgetExhausted); }
        let content = full[cursor.offset..page_end].to_owned();
        cursor.offset = page_end;
        let truncated = page_end != full.len();
        let continuation_cursor = if truncated {
            Some(format!("ledger1:{}", hex::encode(serde_json::to_vec(&cursor).map_err(|_| ResolveError::Unavailable)?)))
        } else { None };
        let sibling = |previous: bool| -> Result<Option<String>, ResolveError> {
            let conn = db.lock();
            let sql = if previous {
                "SELECT node_id FROM ledger_nodes WHERE doc_id=?1 AND parent_id IS ?2 AND ordinal<?3 AND ledger_generation=?4 ORDER BY ordinal DESC LIMIT 1"
            } else {
                "SELECT node_id FROM ledger_nodes WHERE doc_id=?1 AND parent_id IS ?2 AND ordinal>?3 AND ledger_generation=?4 ORDER BY ordinal LIMIT 1"
            };
            Ok(conn.query_row(sql, params![doc_id, parent, ordinal, source.generation], |r| r.get(0)).optional()?)
        };
        outline::DocReadV1 { schema_version: "DocReadV1", source_ref: request.source_ref.clone(),
            content_hash: source.projection_hash.clone(), anchor_id: node_id.to_owned(), content,
            breadcrumb: headings.split(" > ").filter(|s| !s.is_empty()).map(str::to_owned).collect(),
            span: outline::DocSpanV1 { start_byte: start, end_byte: end,
                start_line: source.markdown[..start].bytes().filter(|b| *b == b'\n').count() + 1,
                end_line: source.markdown[..end.saturating_sub(1)].bytes().filter(|b| *b == b'\n').count() + 1,
                span_hash },
            neighbor_anchors: outline::NeighborAnchorsV1 { parent: parent.clone(), previous: sibling(true)?, next: sibling(false)? },
            truncated, continuation_cursor }
    } else {
        let read = outline::read_section_with_cursor(&request.source_ref, &source.markdown,
            &request.anchor_id, &source.projection_hash, maximum, request.continuation_cursor.as_deref())
            .map_err(|e| match e {
                outline::DocReadError::SourceChanged => ResolveError::Stale,
                outline::DocReadError::SourceMissing => ResolveError::Missing,
                outline::DocReadError::Relocated => ResolveError::Relocated,
                outline::DocReadError::Deny => ResolveError::Denied,
            })?;
        if request.expected_span_hash.as_ref().is_some_and(|h| !hash_matches(h, &read.span.span_hash)) {
            return Err(ResolveError::Stale);
        }
        if read.truncated && read.content.is_empty() { return Err(ResolveError::BudgetExhausted); }
        read
    };
    Ok(ResolvedDocument { raw_content_hash: source.raw_hash, projection_content_hash: source.projection_hash,
        source_kind: if source.imported { "imported_snapshot" } else { "worktree" },
        source_revision: source.revision, ledger_generation: source.generation, doc_id,
        node_id: node_id.map(str::to_owned), converter: source.converter,
        losses: source.losses, omissions: source.omissions, read })
}
