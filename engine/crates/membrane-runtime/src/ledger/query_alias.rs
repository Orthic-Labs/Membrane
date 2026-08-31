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
}

pub(crate) fn replace_node_aliases_tx(
    tx: &Transaction<'_>,
    doc_id: &str,
    node_id: &str,
    anchor_id: &str,
    heading: &str,
    body: &str,
    source_revision: &str,
    span_hash: &str,
    generation: i64,
) -> rusqlite::Result<()> {
    let mut aliases = Vec::new();
    let mut seen = BTreeSet::new();
    for line in body.lines() {
        let line = line
            .trim()
            .trim_start_matches(|character: char| {
                matches!(character, '#' | '-' | '*' | '>' | ' ')
            })
            .trim();
        if line.ends_with('?') && line.len() <= 240 {
            if seen.insert(line.to_owned()) {
                aliases.push((line.to_owned(), 1.0f64, "source_question"));
            }
        }
    }
    let heading = heading.trim();
    if !heading.is_empty() && !matches!(heading, "preamble" | "_frontmatter") {
        let alias = if heading.ends_with('?') {
            heading.to_owned()
        } else {
            format!("{heading}?")
        };
        if seen.insert(alias.clone()) {
            aliases.push((alias, 0.65f64, "heading_question"));
        }
    }
    for (alias, weight, derivation) in aliases {
        tx.execute(
            "INSERT INTO ledger_query_aliases (
                doc_id,node_id,anchor_id,alias,weight,derivation,source_revision,span_hash,
                ledger_generation
             ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            rusqlite::params![
                doc_id,
                node_id,
                anchor_id,
                alias,
                weight,
                derivation,
                source_revision,
                span_hash,
                generation,
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
                        artifact.content_hash
                 FROM ledger_query_aliases alias
                 JOIN ledger_nodes node
                   ON node.doc_id=alias.doc_id AND node.node_id=alias.node_id
                 JOIN ledger_doc_artifacts artifact ON artifact.doc_id=alias.doc_id
                 JOIN ledger_index_publications publication ON publication.doc_id=alias.doc_id
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
            )| {
                let source_path = Path::new(&repository_root).join(path);
                let live = std::fs::read(source_path).ok()?;
                if hex::encode(Sha256::digest(&live)) != content_hash {
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
