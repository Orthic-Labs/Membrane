//! Ledger document registration. Artifacts are source references, never memories.

use crate::{
    ledger::doc_projection::{
        replace_doc_projections_tx, DocumentProjectionStoreInputV1, DocumentProjectionV1,
        ProjectionKind, ProjectionProvenanceV1,
    },
    ledger::index::{
        advance_unchanged_generation_tx, document_index_is_current_tx, replace_document_index_tx,
        IndexDocumentInput,
    },
    ledger::LedgerDb,
};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct GrantedDocumentIngestV1 {
    pub repository_root: String,
    pub repository_id: String,
    pub revision: String,
    pub path: String,
    pub title: String,
    pub document: super::document_conversion::DocumentConversionInputV1,
}

/// Convert & ingest one explicitly granted non-Markdown document into Ledger's rebuildable path.
pub fn ingest_granted_document(
    db: &LedgerDb,
    grant: &super::document_conversion::DocumentConversionGrantV1,
    input: GrantedDocumentIngestV1,
) -> Result<DocArtifactV1, String> {
    if Path::new(&input.path)
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err("document_ingest_path_escape".to_owned());
    }
    let converted = super::document_conversion::convert_granted_document(grant, input.document)
        .map_err(|error| error.to_string())?;
    let doc_id = format!(
        "ledger.doc:{}:{}",
        &digest(input.repository_root.as_bytes())[..16],
        &digest(input.path.as_bytes())[..16]
    );
    let parser_version = format!(
        "{}:{}",
        converted.converter.converter, converted.converter.version
    );
    let mut conn = db.lock();
    let tx = conn.transaction().map_err(|error| error.to_string())?;
    let generation: i64 = tx
        .query_row(
            "SELECT COALESCE(MAX(index_generation),0)+1 FROM ledger_doc_artifacts",
            [],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    tx.execute("UPDATE ledger_doc_artifacts SET index_generation=?1", [generation])
        .map_err(|error| error.to_string())?;
    tx.execute("UPDATE ledger_doc_projections SET index_generation=?1", [generation])
        .map_err(|error| error.to_string())?;
    tx.execute("UPDATE ledger_nodes SET ledger_generation=?1", [generation])
        .map_err(|error| error.to_string())?;
    tx.execute(
        "UPDATE ledger_index_publications SET ledger_generation=?1",
        [generation],
    )
    .map_err(|error| error.to_string())?;
    tx.execute(
        "UPDATE ledger_query_aliases SET ledger_generation=?1",
        [generation],
    )
    .map_err(|error| error.to_string())?;
    tx.execute(
        "UPDATE ledger_link_targets SET ledger_generation=?1",
        [generation],
    )
    .map_err(|error| error.to_string())?;
    tx.execute(
        "UPDATE ledger_document_conversions SET ledger_generation=?1",
        [generation],
    )
    .map_err(|error| error.to_string())?;
    let now = crate::time::now_millis() as i64;
    tx.execute(
        "INSERT INTO ledger_doc_artifacts (
            doc_id,repository_root,repository_id,revision,path,content_hash,parser_version,
            document_class,lifecycle_state,title,summary,keywords_json,superseded_by,trust_label,
            influence_class,sensitivity,generated,index_generation,updated_at_ms
         ) VALUES (?1,?2,?3,?4,?5,?6,?7,'knowledge','active',?8,'','[]',NULL,
                   'catalogued','reference','normal',0,?9,?10)
         ON CONFLICT(repository_root,path) DO UPDATE SET
            doc_id=excluded.doc_id,repository_id=excluded.repository_id,revision=excluded.revision,
            content_hash=excluded.content_hash,parser_version=excluded.parser_version,
            lifecycle_state='active',title=excluded.title,index_generation=excluded.index_generation,
            updated_at_ms=excluded.updated_at_ms",
        rusqlite::params![
            doc_id,
            input.repository_root,
            input.repository_id,
            input.revision,
            input.path,
            converted.raw_sha256,
            parser_version,
            input.title,
            generation,
            now,
        ],
    )
    .map_err(|error| error.to_string())?;
    replace_document_index_tx(
        &tx,
        &IndexDocumentInput {
            doc_id: &doc_id,
            path: &input.path,
            title: &input.title,
            markdown: &converted.markdown,
            content_hash: &converted.raw_sha256,
            source_revision: &input.revision,
            generation,
            parser_version: &parser_version,
        },
    )
    .map_err(|error| error.to_string())?;
    replace_doc_projections_tx(
        &tx,
        &DocumentProjectionStoreInputV1 {
            parent_doc_id: doc_id.clone(),
            source_content_hash: converted.raw_sha256.clone(),
            source_revision: input.revision.clone(),
            index_generation: generation,
            projections: vec![DocumentProjectionV1 {
                kind: ProjectionKind::Lexical,
                content: converted.markdown.clone(),
                token_count: 0,
                provenance: ProjectionProvenanceV1 {
                    anchor_id: "document".to_owned(),
                    collapsed_to_parent: None,
                },
            }],
        },
    )
    .map_err(|error| error.to_string())?;
    tx.execute(
        "INSERT INTO ledger_document_conversions (
            doc_id,source_ref,input_format,raw_input,raw_sha256,markdown,markdown_sha256,
            converter,converter_version,config_digest,losses_json,omissions_json,
            source_revision,ledger_generation
         ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)
         ON CONFLICT(doc_id) DO UPDATE SET source_ref=excluded.source_ref,
            input_format=excluded.input_format,raw_input=excluded.raw_input,
            raw_sha256=excluded.raw_sha256,markdown=excluded.markdown,
            markdown_sha256=excluded.markdown_sha256,converter=excluded.converter,
            converter_version=excluded.converter_version,config_digest=excluded.config_digest,
            losses_json=excluded.losses_json,omissions_json=excluded.omissions_json,
            source_revision=excluded.source_revision,ledger_generation=excluded.ledger_generation",
        rusqlite::params![
            doc_id,
            converted.source_ref,
            converted.format.storage_name(),
            converted.raw_input,
            converted.raw_sha256,
            converted.markdown,
            converted.markdown_sha256,
            converted.converter.converter,
            converted.converter.version,
            converted.converter.config_digest,
            serde_json::to_string(&converted.losses).map_err(|error| error.to_string())?,
            serde_json::to_string(&converted.omissions).map_err(|error| error.to_string())?,
            input.revision,
            generation,
        ],
    )
    .map_err(|error| error.to_string())?;
    super::link_projection::resolve_link_targets_tx(&tx, &input.repository_root)
        .map_err(|error| error.to_string())?;
    tx.commit().map_err(|error| error.to_string())?;
    Ok(DocArtifactV1 {
        doc_id,
        repository_root: input.repository_root,
        repository_id: input.repository_id,
        revision: input.revision,
        path: input.path,
        content_hash: converted.raw_sha256,
        parser_version,
        document_class: "knowledge".to_owned(),
        lifecycle_state: "active".to_owned(),
        trust_label: "catalogued".to_owned(),
        influence_class: "reference".to_owned(),
        sensitivity: "normal".to_owned(),
        generated: false,
        index_generation: generation,
    })
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct DocFrontmatterV1 {
    title: Option<String>,
    summary: Option<String>,
    keywords: Vec<String>,
    status: Option<String>,
    supersedes: Option<String>,
}

fn parse_frontmatter(markdown: &str) -> Result<DocFrontmatterV1, String> {
    if !markdown.starts_with("---\n") && !markdown.starts_with("---\r\n") {
        return Ok(DocFrontmatterV1::default());
    }
    let end = markdown[3..]
        .find("\n---\n")
        .or_else(|| markdown[3..].find("\n---\r\n"))
        .map(|offset| offset + 4)
        .ok_or_else(|| "unclosed frontmatter".to_owned())?;
    let block = &markdown[..end + 3];
    if block.len() > 32 * 1024 {
        return Err("frontmatter exceeds 32 KiB".to_owned());
    }
    let mut values = BTreeMap::new();
    let mut namespace_seen = false;
    let mut in_membrane = false;
    for raw in block
        .lines()
        .skip(1)
        .take_while(|line| line.trim() != "---")
    {
        if raw.trim().is_empty() || raw.trim_start().starts_with('#') {
            continue;
        }
        let indent = raw.len() - raw.trim_start_matches(' ').len();
        if indent == 0 {
            in_membrane = false;
            let Some((key, value)) = raw.split_once(':') else {
                continue;
            };
            if key.trim() != "membrane" {
                continue;
            }
            if namespace_seen {
                return Err("duplicate frontmatter namespace: membrane".to_owned());
            }
            if !value.trim().is_empty() {
                return Err("membrane frontmatter namespace must be a mapping".to_owned());
            }
            namespace_seen = true;
            in_membrane = true;
            continue;
        }
        if !in_membrane {
            continue;
        }
        let (key, value) = raw
            .trim_start()
            .split_once(':')
            .ok_or_else(|| "malformed membrane frontmatter".to_owned())?;
        let key = key.trim();
        let value = value.trim();
        if value.len() > 4 * 1024 || value.chars().any(char::is_control) {
            return Err("invalid frontmatter value".to_owned());
        }
        if matches!(
            key,
            "title" | "summary" | "keywords" | "status" | "supersedes"
        ) && values.insert(key.to_owned(), value.to_owned()).is_some()
        {
            return Err(format!("duplicate frontmatter key: {key}"));
        }
    }
    let status = values.remove("status");
    if let Some(status) = &status {
        if !matches!(
            status.as_str(),
            "active" | "draft" | "retired" | "superseded"
        ) {
            return Err("invalid frontmatter status".to_owned());
        }
    }
    let keywords = values
        .remove("keywords")
        .map(|value| -> Result<Vec<String>, String> {
            let value = value.trim();
            let value = match (value.starts_with('['), value.ends_with(']')) {
                (true, true) => &value[1..value.len() - 1],
                (false, false) => value,
                _ => return Err("invalid frontmatter keywords".to_owned()),
            };
            Ok(value
                .split(',')
                .map(|word| word.trim().trim_matches(['\'', '"']))
                .filter(|word| !word.is_empty())
                .map(str::to_owned)
                .collect())
        })
        .transpose()?
        .unwrap_or_default();
    Ok(DocFrontmatterV1 {
        title: values.remove("title"),
        summary: values.remove("summary"),
        keywords,
        status,
        supersedes: values
            .remove("supersedes")
            .filter(|value| !value.is_empty()),
    })
}

fn lexical_content(markdown: &str, frontmatter: &DocFrontmatterV1) -> String {
    if frontmatter == &DocFrontmatterV1::default() {
        return markdown.to_owned();
    }
    let mut content = String::new();
    if let Some(title) = &frontmatter.title {
        content.push_str(title);
        content.push('\n');
    }
    if let Some(summary) = &frontmatter.summary {
        content.push_str(summary);
        content.push('\n');
    }
    if !frontmatter.keywords.is_empty() {
        content.push_str(&frontmatter.keywords.join(" "));
        content.push('\n');
    }
    content.push_str(markdown);
    content
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct DocArtifactV1 {
    pub doc_id: String,
    pub repository_root: String,
    pub repository_id: String,
    pub revision: String,
    pub path: String,
    pub content_hash: String,
    pub parser_version: String,
    pub document_class: String,
    pub lifecycle_state: String,
    pub trust_label: String,
    pub influence_class: String,
    pub sensitivity: String,
    pub generated: bool,
    pub index_generation: i64,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct DocSyncReport {
    pub registered: usize,
    pub tombstoned: usize,
    pub excluded_health: usize,
    pub index_generation: i64,
    pub scanned: usize,
    pub hashed: usize,
    pub parsed: usize,
    pub skipped: usize,
    pub deleted: usize,
    pub invalidated: usize,
}

const DOC_PARSER_VERSION: &str = "comrak-0.54.0";

fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn query_terms(query: &str) -> Vec<String> {
    super::index::query_terms(query)
}

fn lexical_score(content: &str, terms: &[String]) -> usize {
    let content = super::index::normalize_query(content);
    terms
        .iter()
        .map(|term| content.match_indices(term).count())
        .sum()
}

fn load_registered_markdown(
    db: &LedgerDb,
    doc_id: &str,
    repository_root: &str,
    path: &str,
    expected_hash: &str,
) -> Option<(String, String)> {
    let converted = {
        let conn = db.lock();
        conn.query_row(
            "SELECT conversion.raw_input,conversion.markdown,conversion.source_ref
             FROM ledger_document_conversions conversion
             JOIN ledger_doc_artifacts artifact ON artifact.doc_id=conversion.doc_id
             WHERE conversion.doc_id=?1 AND conversion.raw_sha256=?2
               AND conversion.source_revision=artifact.revision
               AND conversion.ledger_generation=artifact.index_generation",
            rusqlite::params![doc_id, expected_hash],
            |row| {
                Ok((
                    row.get::<_, Vec<u8>>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .ok()
    };
    if let Some((raw, markdown, source_ref)) = converted {
        return (digest(&raw) == expected_hash).then_some((markdown, source_ref));
    }
    let source_path = Path::new(repository_root).join(path);
    let bytes = std::fs::read(source_path).ok()?;
    if digest(&bytes) != expected_hash {
        return None;
    }
    let markdown = String::from_utf8(bytes).ok()?;
    Some((
        markdown,
        format!("doc://repo/worktree/{}", path.trim_start_matches('/')),
    ))
}

#[derive(Debug, serde::Serialize)]
pub struct RegisteredDocReadV1 {
    pub raw_content_hash: String,
    pub read: super::outline::DocReadV1,
}

/// Resolve a registered Markdown or converted document section through Ledger's current source.
pub fn read_registered_section(
    db: &LedgerDb,
    doc_id: &str,
    anchor: &str,
    max_bytes: usize,
) -> Result<RegisteredDocReadV1, String> {
    let (root, path, raw_content_hash) = {
        let conn = db.lock();
        conn.query_row(
            "SELECT repository_root,path,content_hash FROM ledger_doc_artifacts
             WHERE doc_id=?1 AND lifecycle_state='active'",
            [doc_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .map_err(|error| error.to_string())?
    };
    let (markdown, source_ref) =
        load_registered_markdown(db, doc_id, &root, &path, &raw_content_hash)
            .ok_or_else(|| "source_changed".to_owned())?;
    let markdown_hash = digest(markdown.as_bytes());
    let read = super::outline::read_section(
        &source_ref,
        &markdown,
        anchor,
        &markdown_hash,
        max_bytes,
    )
    .map_err(|error| error.to_string())?;
    Ok(RegisteredDocReadV1 {
        raw_content_hash,
        read,
    })
}

/// Recall active Ledger artifacts as source pointers, never memory entries.
///
/// Source is reopened & hash-checked before emitting a hit, so stale projections cannot yield a
/// pointer whose expected hash targets changed document content.
pub fn recall(db: &LedgerDb, query: &str, k: usize) -> Result<Vec<DocRecallHitV1>, String> {
    Ok(recall_with_graph(db, query, k, &LedgerRecallGraphPolicyV1::default())?.hits)
}

fn recall_base(db: &LedgerDb, query: &str, k: usize) -> Result<Vec<DocRecallHitV1>, String> {
    match super::index::recall_mode(db)? {
        super::index::LedgerRecallMode::LedgerFts => recall_fts(db, query, k),
        super::index::LedgerRecallMode::Shadow => Ok(recall_shadow(db, query, k)?.legacy_hits),
        super::index::LedgerRecallMode::LegacyScan => recall_legacy(db, query, k),
    }
}

#[derive(Clone, Debug, serde::Serialize, PartialEq)]
pub struct LedgerRecallGraphPolicyV1 {
    pub min_query_terms: usize,
    pub min_seed_term_coverage: f32,
    pub expansion: super::link_projection::LedgerGraphExpansionPolicyV1,
}

impl Default for LedgerRecallGraphPolicyV1 {
    fn default() -> Self {
        Self {
            min_query_terms: 2,
            min_seed_term_coverage: 0.8,
            expansion: super::link_projection::LedgerGraphExpansionPolicyV1 {
                min_seed_strength: 0.8,
                max_hops: 1,
                max_nodes: 4,
                max_edges: 6,
            },
        }
    }
}

#[derive(Clone, Debug, serde::Serialize, PartialEq)]
pub struct LedgerRecallWithGraphV1 {
    pub hits: Vec<DocRecallHitV1>,
    pub graph: super::link_projection::LedgerGraphExpansionV1,
}

/// Production document recall with strongly gated, bounded Ledger-link expansion.
pub fn recall_with_graph(
    db: &LedgerDb,
    query: &str,
    k: usize,
    policy: &LedgerRecallGraphPolicyV1,
) -> Result<LedgerRecallWithGraphV1, String> {
    let mut hits = recall_base(db, query, k)?;
    let query_terms = super::index::normalize_query(query)
        .split_whitespace()
        .map(|term| term.trim_matches(|character: char| character.is_ascii_punctuation()))
        .filter(|term| !term.is_empty())
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    let seeds = if query_terms.len() < policy.min_query_terms {
        Vec::new()
    } else {
        hits.iter()
            .filter_map(|hit| {
                recall_seed_strength(db, hit, &query_terms)
                    .filter(|strength| *strength >= policy.min_seed_term_coverage)
                    .map(|strength| super::link_projection::LedgerGraphSeedV1 {
                        doc_id: hit.doc_id.clone(),
                        strength,
                    })
            })
            .collect::<Vec<_>>()
    };
    let graph = super::link_projection::expand_strong_seed_graph(db, &seeds, &policy.expansion)?;
    let existing = hits
        .iter()
        .map(|hit| hit.doc_id.clone())
        .collect::<BTreeSet<_>>();
    for doc_id in graph.node_ids.iter().filter(|doc_id| !existing.contains(*doc_id)) {
        if hits.len() == k {
            break;
        }
        let target_span = graph
            .edges
            .iter()
            .find(|edge| &edge.target_doc_id == doc_id)
            .map(|edge| edge.target_span_hash.as_str());
        if let Some(hit) = graph_recall_hit(db, doc_id, target_span, query, &seeds) {
            hits.push(hit);
        }
    }
    Ok(LedgerRecallWithGraphV1 { hits, graph })
}

fn recall_seed_strength(
    db: &LedgerDb,
    hit: &DocRecallHitV1,
    query_terms: &BTreeSet<String>,
) -> Option<f32> {
    let (root, path) = {
        let conn = db.lock();
        conn.query_row(
            "SELECT repository_root,path FROM ledger_doc_artifacts WHERE doc_id=?1",
            [&hit.doc_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .ok()?
    };
    let (markdown, _) = load_registered_markdown(db, &hit.doc_id, &root, &path, &hit.expected_hash)?;
    let read = super::outline::read_section(
        &hit.source_ref,
        &markdown,
        &hit.anchor_id,
        &hex::encode(Sha256::digest(markdown.as_bytes())),
        usize::MAX,
    )
    .ok()?;
    let normalized = super::index::normalize_query(&read.content);
    let matched = query_terms
        .iter()
        .filter(|term| normalized.contains(term.as_str()))
        .count();
    Some(matched as f32 / query_terms.len() as f32)
}

fn graph_recall_hit(
    db: &LedgerDb,
    doc_id: &str,
    target_span: Option<&str>,
    query: &str,
    seeds: &[super::link_projection::LedgerGraphSeedV1],
) -> Option<DocRecallHitV1> {
    let (root, path, expected_hash, generation, node_id, anchor_id) = {
        let conn = db.lock();
        conn.query_row(
            "SELECT artifact.repository_root,artifact.path,artifact.content_hash,
                    artifact.index_generation,node.node_id,node.anchor_id
             FROM ledger_doc_artifacts artifact
             JOIN ledger_nodes node ON node.doc_id=artifact.doc_id
             WHERE artifact.doc_id=?1 AND artifact.lifecycle_state='active'
               AND node.source_revision=artifact.revision
               AND node.ledger_generation=artifact.index_generation
             ORDER BY CASE WHEN node.span_hash=?2 THEN 0 ELSE 1 END,node.ordinal LIMIT 1",
            rusqlite::params![doc_id, target_span.unwrap_or("")],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                ))
            },
        )
        .ok()?
    };
    let (_, source_ref) = load_registered_markdown(db, doc_id, &root, &path, &expected_hash)?;
    Some(DocRecallHitV1 {
        doc_id: doc_id.to_owned(),
        node_id: Some(node_id),
        source_ref,
        anchor_id,
        expected_hash,
        score: seeds
            .iter()
            .map(|seed| seed.strength)
            .max_by(f32::total_cmp)
            .unwrap_or_default()
            * 0.5,
        ledger_generation: Some(generation),
        lane: "ledger_graph".to_owned(),
        normalized_query: super::index::normalize_query(query),
    })
}

#[derive(Clone, Debug, serde::Serialize, PartialEq)]
pub struct LedgerShadowRecallV1 {
    pub normalized_query: String,
    pub legacy_hits: Vec<DocRecallHitV1>,
    pub fts_hits: Vec<DocRecallHitV1>,
    pub alias_hits: Vec<super::query_alias::QueryAliasShadowHitV1>,
}

/// Execute both lanes for qualification/debugging while leaving caller-visible production
/// behavior on the legacy result. This remains available before FTS activation is trusted.
pub fn recall_shadow(db: &LedgerDb, query: &str, k: usize) -> Result<LedgerShadowRecallV1, String> {
    let legacy_hits = recall_legacy(db, query, k)?;
    let fts_hits = recall_fts(db, query, k)?;
    let alias_hits = super::query_alias::recall_query_aliases_shadow(db, query, k)?;
    Ok(LedgerShadowRecallV1 {
        normalized_query: super::index::normalize_query(query),
        legacy_hits,
        fts_hits,
        alias_hits,
    })
}

fn recall_legacy(db: &LedgerDb, query: &str, k: usize) -> Result<Vec<DocRecallHitV1>, String> {
    if k == 0 {
        return Ok(Vec::new());
    }
    let terms = query_terms(query);
    if terms.is_empty() {
        return Ok(Vec::new());
    }
    let rows = {
        let conn = db.lock();
        let mut statement = conn
            .prepare(
                "SELECT artifact.doc_id, artifact.repository_root, artifact.path, artifact.content_hash, projection.content
                 FROM ledger_doc_artifacts artifact
                 JOIN ledger_doc_projections projection ON projection.parent_doc_id=artifact.doc_id
                 WHERE artifact.lifecycle_state='active'
                   AND artifact.sensitivity='normal'
                   AND projection.kind='lexical'
                   AND projection.source_content_hash=artifact.content_hash
                   AND projection.source_revision=artifact.revision
                   AND projection.index_generation=artifact.index_generation",
            )
            .map_err(|error| error.to_string())?;
        let results = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        results
    };

    let mut hits = Vec::new();
    for (doc_id, repository_root, path, expected_hash, projection) in rows {
        let score = lexical_score(&projection, &terms);
        if score == 0
            || Path::new(&path)
                .components()
                .any(|part| matches!(part, std::path::Component::ParentDir))
        {
            continue;
        }
        let Some((markdown, source_ref)) = load_registered_markdown(
            db,
            &doc_id,
            &repository_root,
            &path,
            &expected_hash,
        ) else {
            continue;
        };
        let outline = super::outline::build_outline_page(
            &source_ref, &markdown, DOC_PARSER_VERSION, usize::MAX, None,
        ).map_err(|error| error.to_string())?;
        let Some(section) = outline
            .sections
            .iter()
            .map(|section| {
                (
                    lexical_score(&markdown[section.start_byte..section.end_byte], &terms),
                    section,
                )
            })
            .filter(|(section_score, _)| *section_score > 0)
            .max_by_key(|(section_score, section)| {
                (
                    *section_score,
                    usize::MAX - (section.end_byte - section.start_byte),
                )
            })
            .map(|(_, section)| section)
        else {
            continue;
        };
        hits.push(DocRecallHitV1 {
            doc_id,
            node_id: None,
            source_ref,
            anchor_id: section.anchor_id.clone(),
            expected_hash,
            score: score as f32,
            ledger_generation: None,
            lane: "legacy_scan".to_owned(),
            normalized_query: super::index::normalize_query(query),
        });
    }
    hits.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| left.doc_id.cmp(&right.doc_id))
    });
    hits.truncate(k);
    Ok(hits)
}

fn recall_fts(db: &LedgerDb, query: &str, k: usize) -> Result<Vec<DocRecallHitV1>, String> {
    let (normalized_query, rows) = super::index::recall_fts(db, query, k)?;
    let mut hits = Vec::new();
    for row in rows {
        if Path::new(&row.path)
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
        {
            continue;
        }
        let Some((_markdown, source_ref)) = load_registered_markdown(
            db,
            &row.doc_id,
            &row.repository_root,
            &row.path,
            &row.expected_hash,
        ) else {
            continue;
        };
        hits.push(DocRecallHitV1 {
            doc_id: row.doc_id,
            node_id: Some(row.node_id),
            source_ref,
            anchor_id: row.anchor_id,
            expected_hash: row.expected_hash,
            score: row.score,
            ledger_generation: Some(row.generation),
            lane: "ledger_fts".to_owned(),
            normalized_query: normalized_query.clone(),
        });
        if hits.len() == k {
            break;
        }
    }
    Ok(hits)
}

fn classify(path: &str) -> (&'static str, &'static str, &'static str, bool) {
    let lower = path.to_ascii_lowercase();
    let generated = lower.contains("/generated/") || lower.ends_with(".generated.md");
    let class = if generated {
        "generated"
    } else if lower.contains("runbook") {
        "runbook"
    } else if lower.contains("decision") {
        "decision"
    } else if lower.contains("policy") {
        "policy"
    } else if lower.contains("histor") {
        "historical"
    } else if lower.contains("content/") {
        "content"
    } else {
        "knowledge"
    };
    let influence = if class == "policy" {
        "authority"
    } else if class == "runbook" {
        "procedure"
    } else {
        "reference"
    };
    (
        class,
        influence,
        if lower.contains("secret") {
            "restricted"
        } else {
            "normal"
        },
        generated,
    )
}

/// Read-only Ledger recall result. Document text stays in source; callers receive only a
/// hash-bound pointer consumable by `membrane cli doc read`.
#[derive(Clone, Debug, serde::Serialize, PartialEq)]
pub struct DocRecallHitV1 {
    pub doc_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    pub source_ref: String,
    pub anchor_id: String,
    pub expected_hash: String,
    pub score: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ledger_generation: Option<i64>,
    pub lane: String,
    pub normalized_query: String,
}

#[derive(Debug)]
struct IgnoreRule {
    base: PathBuf,
    pattern: String,
}

fn ignored(root: &Path, path: &Path, rules: &[IgnoreRule]) -> bool {
    rules.iter().any(|rule| {
        let Ok(relative) = path.strip_prefix(&rule.base) else {
            return false;
        };
        let relative = relative.to_string_lossy().replace('\\', "/");
        let pattern = rule.pattern.trim_matches('/');
        if pattern.is_empty() {
            return false;
        }
        if pattern.contains('/') {
            relative == pattern || relative.starts_with(&format!("{pattern}/"))
        } else {
            relative.split('/').any(|part| part == pattern)
                || path
                    .strip_prefix(root)
                    .ok()
                    .and_then(|p| p.file_name())
                    .is_some_and(|name| name == pattern)
        }
    })
}

fn load_gitignore(dir: &Path, rules: &mut Vec<IgnoreRule>) {
    let Ok(contents) = std::fs::read_to_string(dir.join(".gitignore")) else {
        return;
    };
    for line in contents.lines() {
        let line = line.trim();
        if !line.is_empty() && !line.starts_with('#') && !line.starts_with('!') {
            rules.push(IgnoreRule {
                base: dir.to_path_buf(),
                pattern: line.to_string(),
            });
        }
    }
}

fn has_health_component(path: &Path) -> bool {
    path.components().any(|component| {
        component
            .as_os_str()
            .to_str()
            .is_some_and(|name| name.eq_ignore_ascii_case("health"))
    })
}

fn hard_excluded(path: &Path) -> bool {
    let components = path
        .components()
        .filter_map(|component| component.as_os_str().to_str())
        .map(|component| component.to_ascii_lowercase())
        .collect::<Vec<_>>();
    components.iter().any(|component| {
        matches!(
            component.as_str(),
            ".git" | "node_modules" | ".cache" | "target" | ".venv" | "vendor" | "memory-mirror"
        )
    }) || components.last().is_some_and(|name| name == "memory.md")
}

fn walk(
    root: &Path,
    output: &mut Vec<PathBuf>,
    excluded_health: &mut usize,
) -> std::io::Result<()> {
    let mut pending = vec![(root.to_path_buf(), 0usize)];
    let mut ignore_rules = Vec::new();
    while let Some((dir, depth)) = pending.pop() {
        if depth > 64 {
            continue;
        }
        load_gitignore(&dir, &mut ignore_rules);
        for item in std::fs::read_dir(&dir)? {
            let item = item?;
            let path = item.path();
            let relative = path.strip_prefix(root).unwrap_or(&path);
            let kind = item.file_type()?;
            if kind.is_symlink() {
                continue;
            }
            if has_health_component(relative) {
                *excluded_health += 1;
                continue;
            }
            if hard_excluded(relative) {
                continue;
            }
            if ignored(root, &path, &ignore_rules) {
                continue;
            }
            if kind.is_dir() {
                let name = path.file_name().and_then(|v| v.to_str()).unwrap_or("");
                if matches!(
                    name,
                    ".git"
                        | "node_modules"
                        | "target"
                        | ".cache"
                        | ".venv"
                        | "vendor"
                        | "memory-mirror"
                ) {
                    continue;
                }
                pending.push((path, depth + 1));
            } else if path
                .extension()
                .and_then(|v| v.to_str())
                .is_some_and(|v| v.eq_ignore_ascii_case("md"))
            {
                output.push(path);
            }
        }
    }
    Ok(())
}

#[inline(never)]
pub fn sync(db: &LedgerDb, root: &Path) -> Result<DocSyncReport, String> {
    let root = std::fs::canonicalize(root).map_err(|e| e.to_string())?;
    let root_s = root.to_string_lossy().replace('\\', "/");
    let revision = std::process::Command::new("git")
        .args(["-C", &root_s, "rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "worktree".into());
    let mut files = Vec::new();
    let mut excluded_health = 0;
    walk(&root, &mut files, &mut excluded_health).map_err(|e| e.to_string())?;
    let mut conn = db.lock();
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    let generation: i64 = tx
        .query_row(
            "SELECT COALESCE(MAX(index_generation),0)+1 FROM ledger_doc_artifacts",
            [],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    // A sync publishes one database-wide generation. Previously verified rows from other roots
    // advance in the same transaction; recall still hash-checks their live source before emit.
    tx.execute(
        "UPDATE ledger_doc_artifacts SET index_generation=?1 WHERE repository_root<>?2 OR EXISTS(SELECT 1 FROM ledger_document_conversions owned_conversion WHERE owned_conversion.doc_id=ledger_doc_artifacts.doc_id)",
        rusqlite::params![generation, root_s],
    )
    .map_err(|error| error.to_string())?;
    tx.execute(
        "UPDATE ledger_doc_projections SET index_generation=?1
         WHERE parent_doc_id IN (
             SELECT doc_id FROM ledger_doc_artifacts WHERE repository_root<>?2 OR EXISTS(SELECT 1 FROM ledger_document_conversions owned_conversion WHERE owned_conversion.doc_id=ledger_doc_artifacts.doc_id)
         )",
        rusqlite::params![generation, root_s],
    )
    .map_err(|error| error.to_string())?;
    tx.execute(
        "UPDATE ledger_nodes SET ledger_generation=?1
         WHERE doc_id IN (
             SELECT doc_id FROM ledger_doc_artifacts WHERE repository_root<>?2 OR EXISTS(SELECT 1 FROM ledger_document_conversions owned_conversion WHERE owned_conversion.doc_id=ledger_doc_artifacts.doc_id)
         )",
        rusqlite::params![generation, root_s],
    )
    .map_err(|error| error.to_string())?;
    tx.execute(
        "UPDATE ledger_index_publications SET ledger_generation=?1
         WHERE doc_id IN (
             SELECT doc_id FROM ledger_doc_artifacts WHERE repository_root<>?2 OR EXISTS(SELECT 1 FROM ledger_document_conversions owned_conversion WHERE owned_conversion.doc_id=ledger_doc_artifacts.doc_id)
         )",
        rusqlite::params![generation, root_s],
    )
    .map_err(|error| error.to_string())?;
    tx.execute(
        "UPDATE ledger_query_aliases SET ledger_generation=?1
         WHERE doc_id IN (
             SELECT doc_id FROM ledger_doc_artifacts WHERE repository_root<>?2 OR EXISTS(SELECT 1 FROM ledger_document_conversions owned_conversion WHERE owned_conversion.doc_id=ledger_doc_artifacts.doc_id)
         )",
        rusqlite::params![generation, root_s],
    )
    .map_err(|error| error.to_string())?;
    tx.execute(
        "UPDATE ledger_link_targets SET ledger_generation=?1
         WHERE source_doc_id IN (
             SELECT doc_id FROM ledger_doc_artifacts WHERE repository_root<>?2 OR EXISTS(SELECT 1 FROM ledger_document_conversions owned_conversion WHERE owned_conversion.doc_id=ledger_doc_artifacts.doc_id)
         )",
        rusqlite::params![generation, root_s],
    )
    .map_err(|error| error.to_string())?;
    tx.execute(
        "UPDATE ledger_document_conversions SET ledger_generation=?1
         WHERE doc_id IN (
             SELECT doc_id FROM ledger_doc_artifacts WHERE repository_root<>?2 OR EXISTS(SELECT 1 FROM ledger_document_conversions owned_conversion WHERE owned_conversion.doc_id=ledger_doc_artifacts.doc_id)
         )",
        rusqlite::params![generation, root_s],
    )
    .map_err(|error| error.to_string())?;
    let projections_available: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='ledger_doc_projections')",
            [],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    let now = crate::time::now_millis() as i64;
    let mut registered = 0;
    let scanned = files.len();
    let mut hashed = 0;
    let mut parsed = 0;
    let mut skipped = 0;
    let mut invalidated = 0;
    let mut projection_inputs = Vec::new();
    let mut supersessions = Vec::new();
    for file in files {
        let bytes = std::fs::read(&file).map_err(|e| e.to_string())?;
        hashed += 1;
        let relative = file
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let hash = digest(&bytes);
        let existing = match tx.query_row(
            "SELECT doc_id, content_hash, parser_version, superseded_by, \
                        lifecycle_state, EXISTS(SELECT 1 FROM ledger_doc_artifacts child \
                               WHERE child.repository_root=ledger_doc_artifacts.repository_root \
                                 AND child.superseded_by=ledger_doc_artifacts.doc_id) \
                 FROM ledger_doc_artifacts \
                 WHERE repository_root=?1 AND path=?2",
            rusqlite::params![root_s, relative],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, bool>(5)?,
                ))
            },
        ) {
            Ok(existing) => Some(existing),
            Err(rusqlite::Error::QueryReturnedNoRows) => None,
            Err(error) => return Err(error.to_string()),
        };
        if let Some((id, old_hash, parser_version, superseded_by, lifecycle, supersedes)) = &existing {
            let has_projection = projections_available
                && tx
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM ledger_doc_projections \
                         WHERE parent_doc_id=?1 AND source_content_hash=?2)",
                        rusqlite::params![id, old_hash],
                        |row| row.get(0),
                    )
                    .map_err(|e| e.to_string())?;
            let has_current_index =
                document_index_is_current_tx(&tx, id, DOC_PARSER_VERSION, &hash)
                    .map_err(|error| error.to_string())?;
            if old_hash == &hash
                && parser_version == DOC_PARSER_VERSION
                && superseded_by.is_none()
                && matches!(lifecycle.as_str(), "active" | "draft" | "retired")
                && !supersedes
                && has_projection
                && has_current_index
            {
                tx.execute(
                    "UPDATE ledger_doc_artifacts \
                     SET revision=?1, index_generation=?2, updated_at_ms=?3 \
                     WHERE repository_root=?4 AND path=?5",
                    rusqlite::params![revision, generation, now, root_s, relative],
                )
                .map_err(|e| e.to_string())?;
                tx.execute(
                    "UPDATE ledger_doc_projections \
                     SET source_revision=?1, index_generation=?2 \
                     WHERE parent_doc_id=?3 AND source_content_hash=?4",
                    rusqlite::params![revision, generation, id, hash],
                )
                .map_err(|e| e.to_string())?;
                advance_unchanged_generation_tx(&tx, id, &revision, generation)
                    .map_err(|e| e.to_string())?;
                registered += 1;
                skipped += 1;
                continue;
            }
            if parser_version != DOC_PARSER_VERSION {
                invalidated += 1;
            }
        }
        let markdown = String::from_utf8(bytes)
            .map_err(|_| "ledger_document_unsupported_encoding".to_owned())?;
        parsed += 1;
        let frontmatter = parse_frontmatter(&markdown)?;
        let (class, influence, sensitivity, generated) = classify(&relative);
        let default_id = format!(
            "ledger.doc:{}:{}",
            digest(root_s.as_bytes())[..16].to_string(),
            digest(relative.as_bytes())[..16].to_string()
        );
        // A content hash identifies bytes, not a source. In particular, copying
        // a document must not replace the original's nodes or authorization.
        // A separate, qualified relocation operation owns move history.
        let id = existing.as_ref().map(|row| row.0.clone()).unwrap_or(default_id);
        let lifecycle = frontmatter.status.as_deref().unwrap_or("active");
        let keywords_json =
            serde_json::to_string(&frontmatter.keywords).map_err(|e| e.to_string())?;
        let title = frontmatter.title.clone().unwrap_or_default();
        tx.execute("INSERT INTO ledger_doc_artifacts (doc_id, repository_root, repository_id, revision, path, content_hash, parser_version, document_class, lifecycle_state, title, summary, keywords_json, superseded_by, trust_label, influence_class, sensitivity, generated, index_generation, updated_at_ms)
          VALUES (?1,?2,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,NULL,'catalogued',?12,?13,?14,?15,?16)
          ON CONFLICT(repository_root,path) DO UPDATE SET revision=excluded.revision, content_hash=excluded.content_hash, parser_version=excluded.parser_version, document_class=excluded.document_class, lifecycle_state=excluded.lifecycle_state, title=excluded.title, summary=excluded.summary, keywords_json=excluded.keywords_json, superseded_by=NULL, trust_label=excluded.trust_label, influence_class=excluded.influence_class, sensitivity=excluded.sensitivity, generated=excluded.generated, index_generation=excluded.index_generation, updated_at_ms=excluded.updated_at_ms",
          rusqlite::params![id, root_s, revision, relative, hash, DOC_PARSER_VERSION, class, lifecycle, title, frontmatter.summary.clone().unwrap_or_default(), keywords_json, influence, sensitivity, generated as i64, generation, now]).map_err(|e| e.to_string())?;
        replace_document_index_tx(
            &tx,
            &IndexDocumentInput {
                doc_id: &id,
                path: &relative,
                title: &title,
                markdown: &markdown,
                content_hash: &hash,
                source_revision: &revision,
                generation,
                parser_version: DOC_PARSER_VERSION,
            },
        )
        .map_err(|e| e.to_string())?;
        if let Some(target) = &frontmatter.supersedes {
            supersessions.push((id.clone(), relative.clone(), target.clone()));
        }
        projection_inputs.push(DocumentProjectionStoreInputV1 {
            parent_doc_id: id,
            source_content_hash: hash,
            source_revision: revision.clone(),
            index_generation: generation,
            projections: vec![DocumentProjectionV1 {
                kind: ProjectionKind::Lexical,
                content: lexical_content(&markdown, &frontmatter),
                token_count: 0,
                provenance: ProjectionProvenanceV1 {
                    anchor_id: "document".to_owned(),
                    collapsed_to_parent: None,
                },
            }],
        });
        registered += 1;
    }
    let mut edges = BTreeMap::new();
    for (new_id, new_path, target_path) in &supersessions {
        if target_path == new_path {
            return Err("frontmatter supersedes self".to_owned());
        }
        let target_id: String = tx.query_row(
            "SELECT doc_id FROM ledger_doc_artifacts WHERE repository_root=?1 AND path=?2 AND lifecycle_state NOT IN ('tombstoned')",
            rusqlite::params![root_s, target_path], |row| row.get(0),
        ).map_err(|_| format!("frontmatter supersedes target missing: {target_path}"))?;
        edges.insert(new_id.clone(), target_id);
    }
    for start in edges.keys() {
        let mut seen = BTreeSet::new();
        let mut current = start.as_str();
        while let Some(next) = edges.get(current) {
            if !seen.insert(current.to_owned()) || next == start {
                return Err("frontmatter supersedes cycle".to_owned());
            }
            current = next;
        }
    }
    for (new_id, _, target_path) in &supersessions {
        tx.execute("UPDATE ledger_doc_artifacts SET lifecycle_state='superseded', superseded_by=?1, updated_at_ms=?2 WHERE repository_root=?3 AND path=?4", rusqlite::params![new_id, now, root_s, target_path]).map_err(|e| e.to_string())?;
    }
    let tombstoned = tx.execute("UPDATE ledger_doc_artifacts SET lifecycle_state='tombstoned', index_generation=?2, updated_at_ms=?3 WHERE repository_root=?1 AND lifecycle_state IN ('active','draft','retired') AND index_generation < ?2 AND NOT EXISTS(SELECT 1 FROM ledger_document_conversions conversion WHERE conversion.doc_id=ledger_doc_artifacts.doc_id)", rusqlite::params![root_s, generation, now]).map_err(|e| e.to_string())?;
    super::link_projection::resolve_link_targets_tx(&tx, &root_s)
        .map_err(|error| error.to_string())?;
    for input in &projection_inputs {
        replace_doc_projections_tx(&tx, input).map_err(|e| e.to_string())?;
    }
    tx.commit().map_err(|e| e.to_string())?;
    Ok(DocSyncReport {
        registered,
        tombstoned,
        excluded_health,
        index_generation: generation,
        scanned,
        hashed,
        parsed,
        skipped,
        deleted: tombstoned,
        invalidated,
    })
}
