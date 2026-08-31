//! Revision/span-bound future-question aliases for Ledger's shadow lane.
//!
//! Aliases are separately weighted retrieval metadata. They never enter authoritative document
//! text, FTS rows, planner grants, or live recall.

use rusqlite::Transaction;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::path::Path;

use super::{index::normalize_query, LedgerDb};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct QueryAliasShadowHitV1 {
    pub doc_id: String,
    pub node_id: String,
    pub anchor_id: String,
    pub alias: String,
    pub weight: f32,
    pub score: f32,
    pub derivation: String,
    pub source_revision: String,
    pub span_hash: String,
    pub ledger_generation: i64,
    pub evidence_quote: String,
    pub evidence_start_byte: usize,
    pub evidence_end_byte: usize,
    pub evidence_sha256: String,
}

pub(crate) fn replace_node_aliases_tx(
    tx: &Transaction<'_>,
    doc_id: &str,
    node_id: &str,
    anchor_id: &str,
    _heading: &str,
    body: &str,
    section_start_byte: usize,
    source_revision: &str,
    span_hash: &str,
    generation: i64,
) -> rusqlite::Result<()> {
    let mut seen = BTreeSet::new();
    for fact in declarative_fact_aliases(body) {
        if !seen.insert(fact.alias.clone()) {
            continue;
        }
        tx.execute(
            "INSERT INTO ledger_query_aliases (
                doc_id,node_id,anchor_id,alias,weight,derivation,source_revision,span_hash,
                ledger_generation
             ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            rusqlite::params![
                doc_id,
                node_id,
                anchor_id,
                fact.alias,
                fact.weight,
                fact.derivation,
                source_revision,
                span_hash,
                generation,
            ],
        )?;
        tx.execute(
            "INSERT INTO ledger_query_alias_evidence (
                doc_id,node_id,alias,evidence_quote,evidence_start_byte,evidence_end_byte,
                evidence_sha256
             ) VALUES (?1,?2,?3,?4,?5,?6,?7)",
            rusqlite::params![
                doc_id,
                node_id,
                fact.alias,
                fact.quote,
                (section_start_byte + fact.start_byte) as i64,
                (section_start_byte + fact.end_byte) as i64,
                hex::encode(Sha256::digest(fact.quote.as_bytes())),
            ],
        )?;
    }
    Ok(())
}

/// Recall exact-current query aliases in a separate, non-authoritative shadow projection.
pub fn recall_query_aliases_shadow(
    db: &LedgerDb,
    query: &str,
    k: usize,
) -> Result<Vec<QueryAliasShadowHitV1>, String> {
    if k == 0 {
        return Ok(Vec::new());
    }
    let normalized_query = normalize_query(query);
    let terms = normalized_query
        .split_whitespace()
        .filter(|term| !term.is_empty())
        .collect::<Vec<_>>();
    if terms.is_empty() {
        return Ok(Vec::new());
    }
    let rows = {
        let conn = db.lock();
        let mut statement = conn
            .prepare(
                "SELECT alias.doc_id,alias.node_id,alias.anchor_id,alias.alias,alias.weight,
                        alias.derivation,alias.source_revision,alias.span_hash,
                        alias.ledger_generation,artifact.repository_root,artifact.path,
                        artifact.content_hash,evidence.evidence_quote,
                        evidence.evidence_start_byte,evidence.evidence_end_byte,
                        evidence.evidence_sha256,conversion.raw_input,conversion.markdown
                 FROM ledger_query_aliases alias
                 JOIN ledger_nodes node
                   ON node.doc_id=alias.doc_id AND node.node_id=alias.node_id
                 JOIN ledger_doc_artifacts artifact ON artifact.doc_id=alias.doc_id
                 JOIN ledger_index_publications publication ON publication.doc_id=alias.doc_id
                 JOIN ledger_query_alias_evidence evidence
                   ON evidence.doc_id=alias.doc_id AND evidence.node_id=alias.node_id
                  AND evidence.alias=alias.alias
                 LEFT JOIN ledger_document_conversions conversion
                   ON conversion.doc_id=alias.doc_id
                  AND conversion.raw_sha256=artifact.content_hash
                  AND conversion.source_revision=artifact.revision
                  AND conversion.ledger_generation=artifact.index_generation
                 WHERE artifact.lifecycle_state='active' AND artifact.sensitivity='normal'
                   AND alias.source_revision=artifact.revision
                   AND alias.source_revision=node.source_revision
                   AND alias.span_hash=node.span_hash
                   AND alias.ledger_generation=artifact.index_generation
                   AND alias.ledger_generation=node.ledger_generation
                   AND publication.content_hash=artifact.content_hash
                   AND publication.source_revision=artifact.revision
                   AND publication.ledger_generation=artifact.index_generation",
            )
            .map_err(|error| error.to_string())?;
        let mapped = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, f64>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, i64>(8)?,
                    row.get::<_, String>(9)?,
                    row.get::<_, String>(10)?,
                    row.get::<_, String>(11)?,
                    row.get::<_, String>(12)?,
                    row.get::<_, i64>(13)?,
                    row.get::<_, i64>(14)?,
                    row.get::<_, String>(15)?,
                    row.get::<_, Option<Vec<u8>>>(16)?,
                    row.get::<_, Option<String>>(17)?,
                ))
            })
            .map_err(|error| error.to_string())?;
        mapped
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?
    };
    let mut hits = rows
        .into_iter()
        .filter_map(
            |(
                doc_id,
                node_id,
                anchor_id,
                alias,
                weight,
                derivation,
                revision,
                span,
                generation,
                repository_root,
                path,
                content_hash,
                evidence_quote,
                evidence_start_byte,
                evidence_end_byte,
                evidence_sha256,
                converted_raw,
                converted_markdown,
            )| {
                let evidence_bytes = if let (Some(raw), Some(markdown)) =
                    (converted_raw, converted_markdown)
                {
                    if hex::encode(Sha256::digest(&raw)) != content_hash {
                        return None;
                    }
                    markdown.into_bytes()
                } else {
                    let source_path = Path::new(&repository_root).join(path);
                    let live = std::fs::read(source_path).ok()?;
                    if hex::encode(Sha256::digest(&live)) != content_hash {
                        return None;
                    }
                    live
                };
                let start = evidence_start_byte as usize;
                let end = evidence_end_byte as usize;
                if evidence_bytes.get(start..end) != Some(evidence_quote.as_bytes())
                    || hex::encode(Sha256::digest(evidence_quote.as_bytes())) != evidence_sha256
                {
                    return None;
                }
                let normalized_alias = normalize_query(&alias);
                let overlap = terms
                    .iter()
                    .filter(|term| normalized_alias.contains(*term))
                    .count();
                (overlap > 0).then(|| QueryAliasShadowHitV1 {
                    doc_id,
                    node_id,
                    anchor_id,
                    alias,
                    weight: weight as f32,
                    score: overlap as f32 * weight as f32,
                    derivation,
                    source_revision: revision,
                    span_hash: span,
                    ledger_generation: generation,
                    evidence_quote,
                    evidence_start_byte: start,
                    evidence_end_byte: end,
                    evidence_sha256,
                })
            },
        )
        .collect::<Vec<_>>();
    hits.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| left.doc_id.cmp(&right.doc_id))
            .then_with(|| left.node_id.cmp(&right.node_id))
    });
    hits.truncate(k);
    Ok(hits)
}

struct DeclarativeAlias {
    alias: String,
    quote: String,
    start_byte: usize,
    end_byte: usize,
    weight: f64,
    derivation: &'static str,
}

fn declarative_fact_aliases(body: &str) -> Vec<DeclarativeAlias> {
    let mut output = Vec::new();
    let mut line_offset = 0usize;
    for line_with_newline in body.split_inclusive('\n') {
        let line = line_with_newline
            .trim_end_matches(|character| matches!(character, '\r' | '\n'));
        let mut sentence_start = 0usize;
        for (period, _) in line.match_indices('.') {
            let raw = &line[sentence_start..=period];
            let leading = raw.len() - raw.trim_start().len();
            let quote = raw.trim();
            let start = line_offset + sentence_start + leading;
            sentence_start = period + 1;
            if quote.len() < 8 || quote.len() > 320 || quote.contains('?') {
                continue;
            }
            let statement = quote.trim_end_matches('.').trim();
            let lower = statement.to_ascii_lowercase();
            let (separator, question_word, derivation) =
                if let Some(index) = lower.find(" is ") {
                    (index, "What is", "declarative_copula_is")
                } else if let Some(index) = lower.find(" are ") {
                    (index, "What are", "declarative_copula_are")
                } else {
                    continue;
                };
            let subject = statement[..separator]
                .trim()
                .trim_start_matches(|character: char| matches!(character, '-' | '*' | '>' | '#'))
                .trim();
            if subject.len() < 2
                || subject.len() > 120
                || subject
                    .chars()
                    .any(|character| matches!(character, '[' | ']' | '(' | ')' | ':'))
            {
                continue;
            }
            output.push(DeclarativeAlias {
                alias: format!("{question_word} {subject}?"),
                quote: quote.to_owned(),
                start_byte: start,
                end_byte: start + quote.len(),
                weight: 1.0,
                derivation,
            });
        }
        line_offset += line_with_newline.len();
    }
    output
}
