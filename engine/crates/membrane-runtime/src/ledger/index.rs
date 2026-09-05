//! Generation-bound structural nodes, safe Unicode query processing, and Ledger-local FTS.

use super::{outline::build_outline_from_ast, LedgerDb};
use comrak::{nodes::NodeValue, parse_document, Arena, Options};
use rusqlite::Transaction;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use unicode_normalization::UnicodeNormalization;

pub const PROJECTION_SCHEMA_VERSION: &str = "ledger.projection.v5";
pub const FTS_SCHEMA_VERSION: &str = "ledger.fts5.v1";
pub const TOKENIZER_ID: &str = "fts5-unicode61+identifier-cjk-ngrams-v1";
pub const QUERY_NORMALIZER_VERSION: &str = "nfkc-casefold-identifiers-v1";

// Activation is a host decision, not a property a caller can grant itself by hashing JSON.
// This receipt is minted from measured ledger-eval-v1 dev+heldout results (see
// docs/evidence/qualification/ledger-metrics.json, `evaluation.qualificationReceipt`, and
// tests/ledger_eval_v1_harness.rs, which produced the numbers the receipt's `runSha256` and
// `resultSha256` are bound to). `ledger_eval_v1_receipt_matches_evidence_file` in
// tests/ledger_indexing.rs asserts this constant stays in sync with that file.
const TRUSTED_LEDGER_FTS_RECEIPTS: &[&str] =
    &["c7547262dbc5a11109236f8b343b421cd6a248a2447df697483624166978360e"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LedgerRecallMode {
    LegacyScan,
    Shadow,
    LedgerFts,
}

impl LedgerRecallMode {
    pub fn storage_name(self) -> &'static str {
        match self {
            Self::LegacyScan => "legacy_scan",
            Self::Shadow => "shadow",
            Self::LedgerFts => "ledger_fts",
        }
    }
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LedgerQualificationReceiptV1 {
    pub schema_version: String,
    pub receipt_source: String,
    pub host_id: String,
    pub verifier_id: String,
    pub commit_sha256: String,
    pub receipt_sha256: String,
    pub corpus_version: String,
    pub corpus_sha256: String,
    pub run_sha256: String,
    pub result_sha256: String,
}

pub fn recall_mode(db: &LedgerDb) -> Result<LedgerRecallMode, String> {
    let mode: String = db
        .lock()
        .query_row(
            "SELECT mode FROM ledger_activation WHERE singleton=1",
            [],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    match mode.as_str() {
        "legacy_scan" => Ok(LedgerRecallMode::LegacyScan),
        "shadow" => Ok(LedgerRecallMode::Shadow),
        "ledger_fts" => Ok(LedgerRecallMode::LedgerFts),
        _ => Err(format!("unknown Ledger recall mode: {mode}")),
    }
}

/// Change the production retrieval lane. FTS activation is fail-closed unless a versioned,
/// content-addressed qualification receipt proves the three safety/reachability gates.
pub fn activate(
    db: &LedgerDb,
    mode: LedgerRecallMode,
    receipt: Option<&LedgerQualificationReceiptV1>,
) -> Result<(), String> {
    if mode == LedgerRecallMode::LedgerFts {
        let receipt = receipt.ok_or_else(|| "ledger_fts_requires_qualification".to_owned())?;
        let valid_hash = is_sha256(&receipt.receipt_sha256)
            && receipt.receipt_sha256 == qualification_receipt_sha256(receipt);
        if !valid_hash
            || receipt.schema_version != "ledger.qualification-receipt.v1"
            || receipt.receipt_source != "membrane-host/ledger-qualification"
            || receipt.host_id.trim().is_empty()
            || receipt.verifier_id.trim().is_empty()
            || !is_sha256(&receipt.commit_sha256)
            || receipt.corpus_version.trim().is_empty()
            || !is_sha256(&receipt.corpus_sha256)
            || !is_sha256(&receipt.run_sha256)
            || !is_sha256(&receipt.result_sha256)
        {
            return Err("ledger_fts_qualification_failed".to_owned());
        }
        if !TRUSTED_LEDGER_FTS_RECEIPTS.contains(&receipt.receipt_sha256.as_str()) {
            return Err("ledger_fts_qualification_untrusted".to_owned());
        }
    }
    let (sha, corpus) = receipt
        .map(|receipt| {
            (
                Some(receipt.receipt_sha256.as_str()),
                Some(receipt.corpus_version.as_str()),
            )
        })
        .unwrap_or((None, None));
    db.lock()
        .execute(
            "UPDATE ledger_activation SET mode=?1, qualification_receipt_sha256=?2, corpus_version=?3, activated_at_ms=?4 WHERE singleton=1",
            rusqlite::params![mode.storage_name(), sha, corpus, crate::time::now_millis() as i64],
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

/// Canonical content address for the activation-relevant receipt fields. The digest deliberately
/// excludes `receipt_sha256` itself and uses explicit field separators rather than serializer
/// output, so verification remains stable across JSON formatting and map ordering.
pub fn qualification_receipt_sha256(receipt: &LedgerQualificationReceiptV1) -> String {
    let payload = format!(
        "ledger.qualification.v1\0{}\0{}\0{}\0{}\0{}\0{}\0{}\0{}\0{}",
        receipt.schema_version,
        receipt.receipt_source,
        receipt.host_id,
        receipt.verifier_id,
        receipt.commit_sha256,
        receipt.corpus_version,
        receipt.corpus_sha256,
        receipt.run_sha256,
        receipt.result_sha256,
    );
    hex::encode(Sha256::digest(payload.as_bytes()))
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[derive(Clone, Debug)]
pub(crate) struct IndexDocumentInput<'a> {
    pub doc_id: &'a str,
    pub path: &'a str,
    pub title: &'a str,
    pub markdown: &'a str,
    pub content_hash: &'a str,
    pub source_revision: &'a str,
    pub generation: i64,
    pub parser_version: &'a str,
}

pub(crate) fn replace_document_index_tx(
    tx: &Transaction<'_>,
    input: &IndexDocumentInput<'_>,
) -> rusqlite::Result<()> {
    tx.execute(
        "DELETE FROM ledger_index_publications WHERE doc_id=?1",
        [input.doc_id],
    )?;
    tx.execute(
        "DELETE FROM ledger_node_fts WHERE doc_id=?1",
        [input.doc_id],
    )?;
    tx.execute(
        "DELETE FROM ledger_query_alias_evidence WHERE doc_id=?1",
        [input.doc_id],
    )?;
    tx.execute(
        "DELETE FROM ledger_query_aliases WHERE doc_id=?1",
        [input.doc_id],
    )?;
    tx.execute("DELETE FROM ledger_nodes WHERE doc_id=?1", [input.doc_id])?;

    let arena = Arena::new();
    let mut options = Options::default();
    options.extension.front_matter_delimiter = Some("---".to_owned());
    options.extension.table = true;
    options.extension.strikethrough = true;
    options.extension.tasklist = true;
    options.extension.autolink = true;
    options.extension.footnotes = true;
    options.render.sourcepos = true;
    let root = parse_document(&arena, input.markdown, &options);
    // One AST supplies all section, block, and link projections.
    // Presentation pagination must never truncate the internal index.
    let outline = build_outline_from_ast(
        &format!("doc://repo/worktree/{}", input.path),
        input.markdown,
        root,
        usize::MAX,
        None,
    ).map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    let mut anchor_to_node = BTreeMap::<String, String>::new();
    let mut identity_collisions = BTreeMap::<String, usize>::new();
    for (ordinal, section) in outline.sections.iter().enumerate() {
        let parent_id = section
            .parent_anchor_id
            .as_ref()
            .and_then(|anchor| anchor_to_node.get(anchor))
            .cloned();
        let node_kind = match section.heading.as_str() {
            "_frontmatter" => "frontmatter",
            "preamble" => "preamble",
            _ => "section",
        };
        let collision = next_collision_ordinal(
            &mut identity_collisions, parent_id.as_deref(), node_kind, &section.span_hash,
        );
        let node_id = stable_node_id(
            input.doc_id, parent_id.as_deref(), node_kind, &section.span_hash, collision,
        );
        anchor_to_node.insert(section.anchor_id.clone(), node_id.clone());
        let body = &input.markdown[section.start_byte..section.end_byte];
        let heading_path = section.breadcrumb.join(" > ");
        let aliases = identifier_aliases(&format!(
            "{} {} {} {} {}",
            input.path, input.title, section.heading, heading_path, body
        ))
        .join(" ");
        tx.execute(
            "INSERT INTO ledger_nodes (
                doc_id,node_id,anchor_id,parent_id,ordinal,node_kind,heading_path,heading,
                source_start_byte,source_end_byte,span_hash,searchable_text,parser_version,
                projection_schema_version,fts_schema_version,tokenizer_id,
                query_normalizer_version,source_revision,ledger_generation
             ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19)",
            rusqlite::params![
                input.doc_id,
                node_id,
                section.anchor_id,
                parent_id,
                ordinal as i64,
                node_kind,
                heading_path,
                section.heading,
                section.start_byte as i64,
                section.end_byte as i64,
                section.span_hash,
                body,
                input.parser_version,
                PROJECTION_SCHEMA_VERSION,
                FTS_SCHEMA_VERSION,
                TOKENIZER_ID,
                QUERY_NORMALIZER_VERSION,
                input.source_revision,
                input.generation,
            ],
        )?;
        tx.execute(
            "INSERT INTO ledger_node_fts(doc_id,node_id,path,title,heading,body,identifier_aliases)
             VALUES (?1,?2,?3,?4,?5,?6,?7)",
            rusqlite::params![
                input.doc_id,
                node_id,
                input.path,
                input.title,
                section.heading,
                body,
                aliases,
            ],
        )?;
        super::query_alias::replace_node_aliases_tx(
            tx,
            input.doc_id,
            &node_id,
            &section.anchor_id,
            &section.heading,
            body,
            section.start_byte,
            input.source_revision,
            &section.span_hash,
            input.generation,
        )?;
    }
    let line_starts = source_line_starts(input.markdown);
    let mut block_ordinal = outline.sections.len();
    let mut block_parents = BTreeMap::<usize, String>::new();
    for node in root.descendants() {
        let data = node.data();
        let Some(node_kind) = ast_node_kind(&data.value) else {
            continue;
        };
        let Some((start, end)) = source_range(
            input.markdown,
            &line_starts,
            data.sourcepos.start.line,
            data.sourcepos.start.column,
            data.sourcepos.end.line,
            data.sourcepos.end.column,
        ) else {
            continue;
        };
        let body = &input.markdown[start..end];
        if body.trim().is_empty() {
            continue;
        }
        let containing_section = outline
            .sections
            .iter()
            .rev()
            .find(|section| section.start_byte <= start && end <= section.end_byte);
        // Every emitted navigation anchor must be accepted by the canonical section reader.
        // Blocks outside a reader-visible section are not safe candidates and stay unindexed.
        let Some(containing_section) = containing_section else {
            continue;
        };
        let parent_id = node.ancestors().skip(1)
            .find_map(|ancestor| block_parents.get(&(ancestor as *const _ as usize)).cloned())
            .or_else(|| anchor_to_node.get(&containing_section.anchor_id).cloned());
        let heading_path = containing_section.breadcrumb.join(" > ");
        let span_hash = hex::encode(Sha256::digest(body.as_bytes()));
        let collision = next_collision_ordinal(
            &mut identity_collisions, parent_id.as_deref(), node_kind, &span_hash,
        );
        let node_id = stable_node_id(
            input.doc_id, parent_id.as_deref(), node_kind, &span_hash, collision,
        );
        let aliases = identifier_aliases(body).join(" ");
        tx.execute(
            "INSERT INTO ledger_nodes (
                doc_id,node_id,anchor_id,parent_id,ordinal,node_kind,heading_path,heading,
                source_start_byte,source_end_byte,span_hash,searchable_text,parser_version,
                projection_schema_version,fts_schema_version,tokenizer_id,
                query_normalizer_version,source_revision,ledger_generation
             ) VALUES (?1,?2,?3,?4,?5,?6,?7,'',?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18)",
            rusqlite::params![
                input.doc_id,
                node_id,
                containing_section.anchor_id,
                parent_id,
                block_ordinal as i64,
                node_kind,
                heading_path,
                start as i64,
                end as i64,
                span_hash,
                body,
                input.parser_version,
                PROJECTION_SCHEMA_VERSION,
                FTS_SCHEMA_VERSION,
                TOKENIZER_ID,
                QUERY_NORMALIZER_VERSION,
                input.source_revision,
                input.generation,
            ],
        )?;
        tx.execute(
            "INSERT INTO ledger_node_fts(doc_id,node_id,path,title,heading,body,identifier_aliases)
             VALUES (?1,?2,?3,?4,'',?5,?6)",
            rusqlite::params![
                input.doc_id,
                node_id,
                input.path,
                input.title,
                body,
                aliases
            ],
        )?;
        block_parents.insert(node as *const _ as usize, node_id.clone());
        block_ordinal += 1;
    }
    // Empty Markdown still receives a source-bound root node.
    if outline.sections.is_empty() {
        let span_hash = hex::encode(Sha256::digest(input.markdown.as_bytes()));
        let node_id = stable_node_id(input.doc_id, None, "document", &span_hash, 0);
        let aliases = identifier_aliases(&format!("{} {}", input.path, input.title)).join(" ");
        tx.execute(
            "INSERT INTO ledger_nodes (doc_id,node_id,anchor_id,parent_id,ordinal,node_kind,heading_path,heading,source_start_byte,source_end_byte,span_hash,searchable_text,parser_version,projection_schema_version,fts_schema_version,tokenizer_id,query_normalizer_version,source_revision,ledger_generation)
             VALUES (?1,?2,'document',NULL,0,'document','',?3,0,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
            rusqlite::params![input.doc_id,node_id,input.title,input.markdown.len() as i64,span_hash,input.markdown,input.parser_version,PROJECTION_SCHEMA_VERSION,FTS_SCHEMA_VERSION,TOKENIZER_ID,QUERY_NORMALIZER_VERSION,input.source_revision,input.generation],
        )?;
        tx.execute(
            "INSERT INTO ledger_node_fts(doc_id,node_id,path,title,heading,body,identifier_aliases) VALUES (?1,?2,?3,?4,?4,?5,?6)",
            rusqlite::params![input.doc_id,node_id,input.path,input.title,input.markdown,aliases],
        )?;
    }
    let node_count: i64 = tx.query_row(
        "SELECT COUNT(*) FROM ledger_nodes WHERE doc_id=?1",
        [input.doc_id],
        |row| row.get(0),
    )?;
    tx.execute(
        "INSERT INTO ledger_index_publications (
            doc_id,content_hash,node_count,parser_version,projection_schema_version,
            fts_schema_version,tokenizer_id,query_normalizer_version,source_revision,
            ledger_generation
         ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
        rusqlite::params![
            input.doc_id,
            input.content_hash,
            node_count,
            input.parser_version,
            PROJECTION_SCHEMA_VERSION,
            FTS_SCHEMA_VERSION,
            TOKENIZER_ID,
            QUERY_NORMALIZER_VERSION,
            input.source_revision,
            input.generation,
        ],
    )?;
    super::link_projection::replace_link_projection_from_ast_tx(
        tx,
        input.doc_id,
        input.path,
        input.markdown,
        input.source_revision,
        input.generation,
        root,
    )?;
    super::diagnostics::record_manifest_tx(tx, input.doc_id)?;
    Ok(())
}

pub(crate) fn advance_unchanged_generation_tx(
    tx: &Transaction<'_>,
    doc_id: &str,
    source_revision: &str,
    generation: i64,
) -> rusqlite::Result<()> {
    tx.execute(
        "UPDATE ledger_nodes SET source_revision=?1, ledger_generation=?2 WHERE doc_id=?3",
        rusqlite::params![source_revision, generation, doc_id],
    )?;
    tx.execute(
        "UPDATE ledger_index_publications
         SET source_revision=?1, ledger_generation=?2 WHERE doc_id=?3",
        rusqlite::params![source_revision, generation, doc_id],
    )?;
    tx.execute(
        "UPDATE ledger_query_aliases SET source_revision=?1, ledger_generation=?2
         WHERE doc_id=?3",
        rusqlite::params![source_revision, generation, doc_id],
    )?;
    tx.execute(
        "UPDATE ledger_link_targets SET source_revision=?1, ledger_generation=?2
         WHERE source_doc_id=?3",
        rusqlite::params![source_revision, generation, doc_id],
    )?;
    Ok(())
}

pub(crate) fn document_index_is_current_tx(
    tx: &Transaction<'_>,
    doc_id: &str,
    parser_version: &str,
    content_hash: &str,
) -> rusqlite::Result<bool> {
    tx.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM ledger_index_publications publication
             WHERE publication.doc_id=?1
               AND publication.node_count > 0
               AND publication.parser_version=?2
               AND publication.projection_schema_version=?3
               AND publication.fts_schema_version=?4
               AND publication.tokenizer_id=?5
               AND publication.query_normalizer_version=?6
               AND publication.content_hash=?7
               AND publication.node_count=(
                   SELECT COUNT(*) FROM ledger_nodes node WHERE node.doc_id=?1
               )
               AND publication.node_count=(
                   SELECT COUNT(*) FROM ledger_node_fts fts WHERE fts.doc_id=?1
               )
         )",
        rusqlite::params![
            doc_id,
            parser_version,
            PROJECTION_SCHEMA_VERSION,
            FTS_SCHEMA_VERSION,
            TOKENIZER_ID,
            QUERY_NORMALIZER_VERSION,
            content_hash,
        ],
        |row| row.get(0),
    )
}

#[derive(Clone, Debug)]
pub(crate) struct FtsHitRow {
    pub doc_id: String,
    pub node_id: String,
    pub repository_root: String,
    pub path: String,
    pub expected_hash: String,
    pub anchor_id: String,
    pub generation: i64,
    pub score: f32,
}

pub(crate) fn recall_fts(
    db: &LedgerDb,
    query: &str,
    k: usize,
) -> Result<(String, Vec<FtsHitRow>), String> {
    let normalized = normalize_query(query);
    let terms = query_terms(query);
    if terms.is_empty() || k == 0 {
        return Ok((normalized, Vec::new()));
    }
    let match_query = terms
        .iter()
        .map(|term| format!("\"{}\"", term.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" OR ");
    let conn = db.lock();
    let pinned_generation: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(index_generation), 0) FROM ledger_doc_artifacts",
            [],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    let mut statement = conn
        .prepare(
            "SELECT artifact.doc_id, fts.node_id, artifact.repository_root, artifact.path,
                    artifact.content_hash, node.anchor_id, artifact.index_generation,
                    -bm25(ledger_node_fts, 0.0, 0.0, 8.0, 6.0, 5.0, 1.0, 4.0) AS score
             FROM ledger_node_fts fts
             JOIN ledger_nodes node ON node.doc_id=fts.doc_id AND node.node_id=fts.node_id
             JOIN ledger_doc_artifacts artifact ON artifact.doc_id=fts.doc_id
             WHERE ledger_node_fts MATCH ?1
               AND artifact.lifecycle_state='active'
               AND artifact.sensitivity='normal'
               AND artifact.index_generation=?2
               AND node.ledger_generation=artifact.index_generation
               AND node.source_revision=artifact.revision
             ORDER BY score DESC, artifact.doc_id, node.ordinal
             LIMIT ?3",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(
            rusqlite::params![
                match_query,
                pinned_generation,
                (k.saturating_mul(8)).max(k) as i64
            ],
            |row| {
                Ok(FtsHitRow {
                    doc_id: row.get(0)?,
                    node_id: row.get(1)?,
                    repository_root: row.get(2)?,
                    path: row.get(3)?,
                    expected_hash: row.get(4)?,
                    anchor_id: row.get(5)?,
                    generation: row.get(6)?,
                    score: row.get::<_, f64>(7)? as f32,
                })
            },
        )
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    Ok((normalized, rows))
}

pub fn normalize_query(query: &str) -> String {
    query
        .nfkc()
        .flat_map(char::to_lowercase)
        .collect::<String>()
}

pub fn query_terms(query: &str) -> Vec<String> {
    let compatibility_normalized = query.nfkc().collect::<String>();
    identifier_aliases(&compatibility_normalized)
}

fn identifier_aliases(value: &str) -> Vec<String> {
    let mut output = BTreeSet::new();
    for token in value.split_whitespace() {
        let trimmed = token.trim_matches(|character: char| {
            character.is_ascii_punctuation() && !matches!(character, '_' | '-' | '.' | '/' | ':')
        });
        if trimmed.is_empty() {
            continue;
        }
        output.insert(trimmed.to_owned());
        for component in trimmed.split(|character: char| !character.is_alphanumeric()) {
            add_component_aliases(component, &mut output);
        }
        add_component_aliases(trimmed, &mut output);
    }
    output.into_iter().collect()
}

fn add_component_aliases(component: &str, output: &mut BTreeSet<String>) {
    if component.is_empty() {
        return;
    }
    let folded = component
        .nfkc()
        .flat_map(char::to_lowercase)
        .collect::<String>();
    output.insert(folded.clone());
    let mut start = 0;
    let chars = component.char_indices().collect::<Vec<_>>();
    for index in 1..chars.len() {
        let (_, current) = chars[index];
        let (_, previous) = chars[index - 1];
        let next_is_lower = chars
            .get(index + 1)
            .is_some_and(|(_, next)| next.is_lowercase());
        if current.is_uppercase()
            && (previous.is_lowercase()
                || previous.is_numeric()
                || (previous.is_uppercase() && next_is_lower))
        {
            let end = chars[index].0;
            output.insert(
                component[start..end]
                    .nfkc()
                    .flat_map(char::to_lowercase)
                    .collect(),
            );
            start = end;
        }
    }
    if start > 0 {
        output.insert(
            component[start..]
                .nfkc()
                .flat_map(char::to_lowercase)
                .collect(),
        );
    }
    if folded.chars().any(|character| !character.is_ascii()) {
        let chars = folded.chars().collect::<Vec<_>>();
        for width in 1..=3.min(chars.len()) {
            for window in chars.windows(width) {
                output.insert(window.iter().collect());
            }
        }
    }
}

fn next_collision_ordinal(
    collisions: &mut BTreeMap<String, usize>,
    parent_id: Option<&str>,
    kind: &str,
    span_hash: &str,
) -> usize {
    let key = format!("{}\0{kind}\0{span_hash}", parent_id.unwrap_or(""));
    let ordinal = collisions.get(&key).copied().unwrap_or(0);
    collisions.insert(key, ordinal + 1);
    ordinal
}

fn stable_node_id(
    doc_id: &str,
    parent_id: Option<&str>,
    kind: &str,
    span_hash: &str,
    collision_ordinal: usize,
) -> String {
    // Source order is not identity. An unrelated insertion before this node
    // leaves the ID unchanged. Only truly indistinguishable same-parent,
    // same-kind, same-span duplicates receive a local collision suffix;
    // inserting another identical duplicate is deliberately an ambiguous
    // relocation case rather than a false stable-identity claim.
    let evidence = if collision_ordinal == 0 {
        format!("{doc_id}\0{}\0{kind}\0{span_hash}", parent_id.unwrap_or(""))
    } else {
        format!(
            "{doc_id}\0{}\0{kind}\0{span_hash}\0duplicate:{collision_ordinal}",
            parent_id.unwrap_or("")
        )
    };
    format!(
        "ledger.node:{}",
        hex::encode(Sha256::digest(evidence.as_bytes()))
    )
}

/// §18 section-identity unification: the structural span-hash fingerprint is
/// a section's identity; slug/ordinal anchors are resolvable aliases, not
/// identity. The node registry stores that fingerprint as `span_hash` per
/// row; this helper is the one public extraction for callers that must name
/// a section structurally (the read-side anchor form lives in
/// [`crate::ledger::outline`], which resolves `span:<fingerprint>` and bare
/// fingerprints against live bytes).
pub fn section_fingerprint(span_hash: &str) -> &str {
    span_hash
}

fn ast_node_kind(value: &NodeValue) -> Option<&'static str> {
    match value {
        NodeValue::Paragraph => Some("paragraph"),
        NodeValue::CodeBlock(block) if block.fenced => Some("fenced_code"),
        NodeValue::CodeBlock(_) => Some("indented_code"),
        NodeValue::List(_) => Some("list"),
        NodeValue::Item(_) => Some("list_item"),
        NodeValue::BlockQuote | NodeValue::MultilineBlockQuote(_) => Some("blockquote"),
        NodeValue::Table(_) => Some("table"),
        NodeValue::TableRow(_) => Some("table_row"),
        NodeValue::TableCell => Some("table_cell"),
        NodeValue::HtmlBlock(_) => Some("html_block"),
        NodeValue::ThematicBreak => Some("thematic_break"),
        NodeValue::FootnoteDefinition(_) => Some("footnote_definition"),
        NodeValue::Link(_) => Some("link"),
        NodeValue::Image(_) => Some("image"),
        _ => None,
    }
}

fn source_line_starts(markdown: &str) -> Vec<usize> {
    std::iter::once(0)
        .chain(markdown.match_indices('\n').map(|(index, _)| index + 1))
        .collect()
}

fn source_range(
    markdown: &str,
    starts: &[usize],
    start_line: usize,
    start_column: usize,
    end_line: usize,
    end_column: usize,
) -> Option<(usize, usize)> {
    if start_line == 0 || end_line == 0 {
        return None;
    }
    let start = starts
        .get(start_line - 1)?
        .saturating_add(start_column.saturating_sub(1))
        .min(markdown.len());
    let line_end = starts.get(end_line).copied().unwrap_or(markdown.len());
    let mut end = starts
        .get(end_line - 1)?
        .saturating_add(end_column)
        .min(line_end)
        .min(markdown.len());
    while start < markdown.len() && !markdown.is_char_boundary(start) {
        return None;
    }
    while end > start && !markdown.is_char_boundary(end) {
        end -= 1;
    }
    (start < end).then_some((start, end))
}

#[cfg(test)]
mod trusted_receipt_tests {
    use super::TRUSTED_LEDGER_FTS_RECEIPTS;

    /// The allowlisted `ledger_fts` qualification receipt must be exactly the one measured and
    /// recorded in docs/evidence/qualification/ledger-metrics.json — activation trust must never
    /// drift from the evidence file that justifies it. This test reads that file directly (no
    /// re-derivation) so a hand-edited allowlist entry that no longer matches recorded evidence
    /// fails loudly.
    #[test]
    fn ledger_eval_v1_receipt_matches_evidence_file() {
        let evidence_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("docs/evidence/qualification/ledger-metrics.json");
        let bytes = std::fs::read(&evidence_path)
            .unwrap_or_else(|error| panic!("read {}: {error}", evidence_path.display()));
        let value: serde_json::Value =
            serde_json::from_slice(&bytes).expect("ledger-metrics.json must be valid JSON");
        assert_eq!(
            value.get("status").and_then(|v| v.as_str()),
            Some("qualified"),
            "ledger-metrics.json status must be 'qualified' when a trusted ledger_fts receipt is allowlisted"
        );
        let recorded_receipt_sha256 = value
            .pointer("/evaluation/qualificationReceipt/receiptSha256")
            .and_then(|v| v.as_str())
            .expect(
                "ledger-metrics.json must record evaluation.qualificationReceipt.receiptSha256",
            );
        assert!(
            TRUSTED_LEDGER_FTS_RECEIPTS.contains(&recorded_receipt_sha256),
            "recorded receiptSha256 {recorded_receipt_sha256} is not in TRUSTED_LEDGER_FTS_RECEIPTS"
        );
        assert_eq!(
            TRUSTED_LEDGER_FTS_RECEIPTS.len(),
            1,
            "allowlist should contain exactly the one receipt recorded in ledger-metrics.json; \
             update this test if a second receipt is deliberately added"
        );
    }
}

#[cfg(test)]
mod section_identity_tests {
    /// §18 — the index layer's structural fingerprint and the read layer's
    /// span anchor name the same evidence: the section's exact span bytes.
    /// Alias (`sec:<slug>:<ordinal>`) reads stay unchanged; fingerprint reads
    /// resolve to the identical section.
    use super::section_fingerprint;
    use crate::ledger::outline::{read_section, span_anchor, DocReadError};

    const DOC: &str = "# Intro\n\nintro paragraph with content.\n\n## Detail\n\ndetail section with specific wording.\n";

    #[test]
    fn index_span_hash_is_the_section_identity_for_reads() {
        let hash = crate::ledger::outline::hash(DOC);
        let alias_read = read_section(
            "doc://repo/worktree/guide.md",
            DOC,
            "sec:detail:1",
            &hash,
            12_000,
        )
        .expect("alias read");
        // The fingerprint the index seals on the node row is exactly what the
        // reader accepts as the structural identity anchor.
        let fingerprint = section_fingerprint(&alias_read.span.span_hash);
        let identity_read = read_section(
            "doc://repo/worktree/guide.md",
            DOC,
            &span_anchor(fingerprint),
            &hash,
            12_000,
        )
        .expect("fingerprint read");
        assert_eq!(identity_read.anchor_id, "sec:detail:1");
        assert_eq!(identity_read.span.span_hash, fingerprint);
        assert_eq!(identity_read.content, alias_read.content);
    }

    #[test]
    fn alias_semantics_are_untouched() {
        let hash = crate::ledger::outline::hash(DOC);
        assert_eq!(
            read_section(
                "doc://repo/worktree/guide.md",
                DOC,
                "sec:intro:1",
                &hash,
                12_000,
            )
            .expect("intro read")
            .anchor_id,
            "sec:intro:1"
        );
        assert_eq!(
            read_section(
                "doc://repo/worktree/guide.md",
                DOC,
                "sec:intro:1",
                "sha256:deadbeef",
                12_000,
            )
            .err(),
            Some(DocReadError::SourceChanged)
        );
        assert_eq!(
            read_section(
                "doc://repo/worktree/guide.md",
                DOC,
                "sec:absent:7",
                &hash,
                12_000,
            )
            .err(),
            Some(DocReadError::Relocated)
        );
    }
}
