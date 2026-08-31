//! Provenance-bound Markdown link projection & bounded Ledger-local graph traversal.
//!
//! Links remain rebuildable navigation metadata. Traversal follows only current registered
//! documents; it never fetches targets, widens source grants, or makes planner admission choices.

use comrak::{nodes::NodeValue, parse_document, Arena, Options};
use rusqlite::{Connection, Transaction};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, VecDeque};
use std::path::Path;
use unicode_normalization::UnicodeNormalization;

use super::LedgerDb;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LedgerLinkKindV1 {
    Inline,
    Reference,
    Autolink,
    Image,
}

impl LedgerLinkKindV1 {
    fn storage_name(self) -> &'static str {
        match self {
            Self::Inline => "inline",
            Self::Reference => "reference",
            Self::Autolink => "autolink",
            Self::Image => "image",
        }
    }

    fn from_storage(value: &str) -> Option<Self> {
        match value {
            "inline" => Some(Self::Inline),
            "reference" => Some(Self::Reference),
            "autolink" => Some(Self::Autolink),
            "image" => Some(Self::Image),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkResolutionStateV1 {
    Pending,
    Resolved,
    Broken,
    External,
    MediaExcluded,
}

impl LinkResolutionStateV1 {
    fn storage_name(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Resolved => "resolved",
            Self::Broken => "broken",
            Self::External => "external",
            Self::MediaExcluded => "media_excluded",
        }
    }

    fn from_storage(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(Self::Pending),
            "resolved" => Some(Self::Resolved),
            "broken" => Some(Self::Broken),
            "external" => Some(Self::External),
            "media_excluded" => Some(Self::MediaExcluded),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct LedgerLinkTargetV1 {
    pub source_doc_id: String,
    pub source_path: String,
    pub source_revision: String,
    pub source_span_hash: String,
    pub source_start_byte: usize,
    pub source_end_byte: usize,
    pub ledger_generation: i64,
    pub link_kind: LedgerLinkKindV1,
    pub raw_target: String,
    pub normalized_target: String,
    pub target_fragment: Option<String>,
    pub resolution_state: LinkResolutionStateV1,
    pub target_doc_id: Option<String>,
    pub target_revision: Option<String>,
    pub target_content_hash: Option<String>,
    pub target_span_hash: Option<String>,
}

pub(crate) fn replace_link_projection_tx(
    tx: &Transaction<'_>,
    doc_id: &str,
    path: &str,
    markdown: &str,
    source_revision: &str,
    generation: i64,
) -> rusqlite::Result<()> {
    tx.execute(
        "DELETE FROM ledger_link_targets WHERE source_doc_id=?1",
        [doc_id],
    )?;
    let arena = Arena::new();
    let mut options = Options::default();
    options.extension.front_matter_delimiter = Some("---".to_owned());
    options.extension.autolink = true;
    options.render.sourcepos = true;
    let root = parse_document(&arena, markdown, &options);
    let starts = source_line_starts(markdown);
    for node in root.descendants() {
        let data = node.data();
        let (target, is_image) = match &data.value {
            NodeValue::Link(link) => (link.url.as_str(), false),
            NodeValue::Image(link) => (link.url.as_str(), true),
            _ => continue,
        };
        let Some((start, end)) = source_range(
            markdown,
            &starts,
            data.sourcepos.start.line,
            data.sourcepos.start.column,
            data.sourcepos.end.line,
            data.sourcepos.end.column,
        ) else {
            continue;
        };
        let raw_source = &markdown[start..end];
        let kind = classify_link_kind(raw_source, target, is_image);
        let (encoded_path, encoded_fragment) = split_fragment(target);
        let decoded_path = percent_decode(encoded_path);
        let target_path = decoded_path.as_str();
        let fragment = encoded_fragment
            .map(|value| percent_decode(&value).nfc().collect::<String>());
        let external = is_external_target(target_path);
        let media = is_image || is_media_path(target_path);
        let state = if media {
            LinkResolutionStateV1::MediaExcluded
        } else if external {
            LinkResolutionStateV1::External
        } else {
            LinkResolutionStateV1::Pending
        };
        let normalized = if external {
            target_path.to_owned()
        } else {
            normalize_relative_target(path, target_path).unwrap_or_default()
        };
        let state = if !external && !media && normalized.is_empty() {
            LinkResolutionStateV1::Broken
        } else {
            state
        };
        tx.execute(
            "INSERT INTO ledger_link_targets (
                source_doc_id,source_path,source_revision,source_span_hash,
                source_start_byte,source_end_byte,ledger_generation,link_kind,raw_target,
                normalized_target,target_fragment,resolution_state,target_doc_id,target_revision,
                target_content_hash,target_span_hash
             ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,NULL,NULL,NULL,NULL)",
            rusqlite::params![
                doc_id,
                path,
                source_revision,
                digest(raw_source.as_bytes()),
                start as i64,
                end as i64,
                generation,
                kind.storage_name(),
                target,
                normalized,
                fragment,
                state.storage_name(),
            ],
        )?;
    }
    Ok(())
}

/// Resolve all pending links from one registered root against current Ledger artifacts.
pub(crate) fn resolve_link_targets_tx(
    tx: &Transaction<'_>,
    repository_root: &str,
) -> rusqlite::Result<()> {
    tx.execute(
        "UPDATE ledger_link_targets
         SET resolution_state='pending',target_doc_id=NULL,target_revision=NULL,
             target_content_hash=NULL,target_span_hash=NULL
         WHERE source_doc_id IN (
             SELECT doc_id FROM ledger_doc_artifacts WHERE repository_root=?1
         ) AND resolution_state NOT IN ('external','media_excluded')",
        [repository_root],
    )?;
    let rows = {
        let mut statement = tx.prepare(
            "SELECT link.source_doc_id,link.source_start_byte,link.source_end_byte,
                    link.normalized_target,link.target_fragment
             FROM ledger_link_targets link
             JOIN ledger_doc_artifacts source ON source.doc_id=link.source_doc_id
             WHERE source.repository_root=?1 AND link.resolution_state='pending'",
        )?;
        let mapped = statement
            .query_map([repository_root], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                ))
            })?;
        mapped.collect::<Result<Vec<_>, _>>()?
    };
    for (source_doc_id, start, end, target_path, fragment) in rows {
        let Some((target_doc_id, target_revision, target_hash)) =
            resolve_registered_target(tx, repository_root, &target_path)?
        else {
            tx.execute(
                "UPDATE ledger_link_targets SET resolution_state='broken'
                 WHERE source_doc_id=?1 AND source_start_byte=?2 AND source_end_byte=?3",
                rusqlite::params![source_doc_id, start, end],
            )?;
            continue;
        };
        let target_span_hash = if let Some(fragment) = fragment.as_deref() {
            let exact = tx.query_row(
                "SELECT span_hash FROM ledger_nodes
                 WHERE doc_id=?1 AND (anchor_id=?2 OR anchor_id LIKE ('sec:' || ?2 || ':%'))
                 ORDER BY ordinal LIMIT 1",
                rusqlite::params![target_doc_id, fragment],
                |row| row.get::<_, String>(0),
            );
            match exact {
                Ok(span_hash) => span_hash,
                Err(rusqlite::Error::QueryReturnedNoRows) => {
                    tx.execute(
                        "UPDATE ledger_link_targets SET resolution_state='broken'
                         WHERE source_doc_id=?1 AND source_start_byte=?2 AND source_end_byte=?3",
                        rusqlite::params![source_doc_id, start, end],
                    )?;
                    continue;
                }
                Err(error) => return Err(error),
            }
        } else {
            target_hash.clone()
        };
        tx.execute(
            "UPDATE ledger_link_targets
             SET resolution_state='resolved',target_doc_id=?1,target_revision=?2,
                 target_content_hash=?3,target_span_hash=?4
             WHERE source_doc_id=?5 AND source_start_byte=?6 AND source_end_byte=?7",
            rusqlite::params![
                target_doc_id,
                target_revision,
                target_hash,
                target_span_hash,
                source_doc_id,
                start,
                end,
            ],
        )?;
    }
    Ok(())
}

/// Read observable link projections. Rows are source-ordered & retain broken/excluded typing.
pub fn link_targets(db: &LedgerDb, doc_id: &str) -> Result<Vec<LedgerLinkTargetV1>, String> {
    let conn = db.lock();
    let mut statement = conn
        .prepare(
            "SELECT link.source_doc_id,link.source_path,link.source_revision,link.source_span_hash,
                    link.source_start_byte,link.source_end_byte,link.ledger_generation,
                    link.link_kind,link.raw_target,link.normalized_target,link.target_fragment,
                    link.resolution_state,link.target_doc_id,link.target_revision,
                    link.target_content_hash,link.target_span_hash,
                    source.repository_root,source.path,source.content_hash
             FROM ledger_link_targets link
             JOIN ledger_doc_artifacts source ON source.doc_id=link.source_doc_id
             WHERE link.source_doc_id=?1
               AND source.lifecycle_state='active'
               AND link.source_revision=source.revision
               AND link.ledger_generation=source.index_generation
               AND (
                   link.resolution_state<>'resolved' OR EXISTS (
                       SELECT 1 FROM ledger_doc_artifacts target
                       WHERE target.doc_id=link.target_doc_id
                         AND target.lifecycle_state='active'
                         AND target.revision=link.target_revision
                         AND target.content_hash=link.target_content_hash
                         AND (
                             link.target_span_hash=target.content_hash OR EXISTS (
                                 SELECT 1 FROM ledger_nodes target_node
                                 WHERE target_node.doc_id=link.target_doc_id
                                   AND target_node.span_hash=link.target_span_hash
                                   AND target_node.source_revision=target.revision
                                   AND target_node.ledger_generation=target.index_generation
                             )
                         )
                   )
               )
             ORDER BY link.source_start_byte",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([doc_id], |row| {
            let kind: String = row.get(7)?;
            let state: String = row.get(11)?;
            Ok((LedgerLinkTargetV1 {
                source_doc_id: row.get(0)?,
                source_path: row.get(1)?,
                source_revision: row.get(2)?,
                source_span_hash: row.get(3)?,
                source_start_byte: row.get::<_, i64>(4)? as usize,
                source_end_byte: row.get::<_, i64>(5)? as usize,
                ledger_generation: row.get(6)?,
                link_kind: LedgerLinkKindV1::from_storage(&kind)
                    .ok_or_else(|| rusqlite::Error::InvalidQuery)?,
                raw_target: row.get(8)?,
                normalized_target: row.get(9)?,
                target_fragment: row.get(10)?,
                resolution_state: LinkResolutionStateV1::from_storage(&state)
                    .ok_or_else(|| rusqlite::Error::InvalidQuery)?,
                target_doc_id: row.get(12)?,
                target_revision: row.get(13)?,
                target_content_hash: row.get(14)?,
                target_span_hash: row.get(15)?,
            }, row.get::<_, String>(16)?, row.get::<_, String>(17)?, row.get::<_, String>(18)?))
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    Ok(rows
        .into_iter()
        .filter_map(|(link, repository_root, path, content_hash)| {
            let live = std::fs::read(Path::new(&repository_root).join(path)).ok()?;
            (digest(&live) == content_hash).then_some(link)
        })
        .collect())
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct LedgerGraphSeedV1 {
    pub doc_id: String,
    pub strength: f32,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct LedgerGraphExpansionPolicyV1 {
    pub min_seed_strength: f32,
    pub max_hops: usize,
    pub max_nodes: usize,
    pub max_edges: usize,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct LedgerGraphEdgeV1 {
    pub source_doc_id: String,
    pub target_doc_id: String,
    pub source_revision: String,
    pub source_span_hash: String,
    pub target_revision: String,
    pub target_content_hash: String,
    pub target_span_hash: String,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphExpansionAbstentionV1 {
    NoStrongSeed,
    InvalidCaps,
    HopCap,
    NodeCap,
    EdgeCap,
    Cycle,
    SourceDrift,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct LedgerGraphExpansionV1 {
    pub node_ids: Vec<String>,
    pub edges: Vec<LedgerGraphEdgeV1>,
    pub abstentions: Vec<GraphExpansionAbstentionV1>,
}

/// Expand current resolved links from strong seeds under hard hop/node/edge bounds.
pub fn expand_strong_seed_graph(
    db: &LedgerDb,
    seeds: &[LedgerGraphSeedV1],
    policy: &LedgerGraphExpansionPolicyV1,
) -> Result<LedgerGraphExpansionV1, String> {
    if policy.max_nodes == 0 || policy.max_edges == 0 {
        return Ok(LedgerGraphExpansionV1 {
            node_ids: Vec::new(),
            edges: Vec::new(),
            abstentions: vec![GraphExpansionAbstentionV1::InvalidCaps],
        });
    }
    let mut strong = seeds
        .iter()
        .filter(|seed| seed.strength >= policy.min_seed_strength)
        .cloned()
        .collect::<Vec<_>>();
    strong.sort_by(|left, right| {
        right
            .strength
            .total_cmp(&left.strength)
            .then_with(|| left.doc_id.cmp(&right.doc_id))
    });
    if strong.is_empty() {
        return Ok(LedgerGraphExpansionV1 {
            node_ids: Vec::new(),
            edges: Vec::new(),
            abstentions: vec![GraphExpansionAbstentionV1::NoStrongSeed],
        });
    }
    let conn = db.lock();
    let mut visited = BTreeSet::new();
    let mut queue = VecDeque::new();
    let mut abstentions = BTreeSet::new();
    for seed in strong {
        if visited.len() == policy.max_nodes {
            abstentions.insert("node_cap");
            break;
        }
        let current = document_is_live(&conn, &seed.doc_id);
        if current && visited.insert(seed.doc_id.clone()) {
            queue.push_back((seed.doc_id, 0usize));
        } else if !current {
            abstentions.insert("source_drift");
        }
    }
    if visited.is_empty() {
        return Ok(LedgerGraphExpansionV1 {
            node_ids: Vec::new(),
            edges: Vec::new(),
            abstentions: vec![if abstentions.contains("source_drift") {
                GraphExpansionAbstentionV1::SourceDrift
            } else {
                GraphExpansionAbstentionV1::NoStrongSeed
            }],
        });
    }
    let mut edges = Vec::new();
    while let Some((source_doc_id, depth)) = queue.pop_front() {
        if !document_is_live(&conn, &source_doc_id) {
            abstentions.insert("source_drift");
            continue;
        }
        if depth >= policy.max_hops {
            abstentions.insert("hop_cap");
            continue;
        }
        let rows = {
            let mut statement = conn
                .prepare(
                    "SELECT link.target_doc_id,link.source_revision,link.source_span_hash,
                            link.target_revision,link.target_content_hash,link.target_span_hash
                     FROM ledger_link_targets link
                     JOIN ledger_doc_artifacts source ON source.doc_id=link.source_doc_id
                     JOIN ledger_doc_artifacts target ON target.doc_id=link.target_doc_id
                     WHERE link.source_doc_id=?1 AND link.resolution_state='resolved'
                       AND source.lifecycle_state='active' AND target.lifecycle_state='active'
                       AND link.source_revision=source.revision
                       AND link.ledger_generation=source.index_generation
                       AND link.target_revision=target.revision
                       AND link.target_content_hash=target.content_hash
                       AND (
                           link.target_span_hash=target.content_hash OR EXISTS (
                               SELECT 1 FROM ledger_nodes target_node
                               WHERE target_node.doc_id=link.target_doc_id
                                 AND target_node.span_hash=link.target_span_hash
                                 AND target_node.source_revision=target.revision
                                 AND target_node.ledger_generation=target.index_generation
                           )
                       )
                     ORDER BY link.target_doc_id,link.source_start_byte",
                )
                .map_err(|error| error.to_string())?;
            let mapped = statement
                .query_map([&source_doc_id], |row| {
                    Ok(LedgerGraphEdgeV1 {
                        source_doc_id: source_doc_id.clone(),
                        target_doc_id: row.get(0)?,
                        source_revision: row.get(1)?,
                        source_span_hash: row.get(2)?,
                        target_revision: row.get(3)?,
                        target_content_hash: row.get(4)?,
                        target_span_hash: row.get(5)?,
                    })
                })
                .map_err(|error| error.to_string())?;
            mapped
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| error.to_string())?
        };
        for edge in rows {
            if !document_is_live(&conn, &edge.target_doc_id) {
                abstentions.insert("source_drift");
                continue;
            }
            if edges.len() == policy.max_edges {
                abstentions.insert("edge_cap");
                queue.clear();
                break;
            }
            let target = edge.target_doc_id.clone();
            if !visited.contains(&target) && visited.len() == policy.max_nodes {
                abstentions.insert("node_cap");
                continue;
            }
            edges.push(edge);
            if visited.contains(&target) {
                abstentions.insert("cycle");
                continue;
            }
            visited.insert(target.clone());
            queue.push_back((target, depth + 1));
        }
    }
    let abstentions = abstentions
        .into_iter()
        .filter_map(|value| match value {
            "hop_cap" => Some(GraphExpansionAbstentionV1::HopCap),
            "node_cap" => Some(GraphExpansionAbstentionV1::NodeCap),
            "edge_cap" => Some(GraphExpansionAbstentionV1::EdgeCap),
            "cycle" => Some(GraphExpansionAbstentionV1::Cycle),
            "source_drift" => Some(GraphExpansionAbstentionV1::SourceDrift),
            _ => None,
        })
        .collect();
    Ok(LedgerGraphExpansionV1 {
        node_ids: visited.into_iter().collect(),
        edges,
        abstentions,
    })
}

fn classify_link_kind(raw_source: &str, target: &str, image: bool) -> LedgerLinkKindV1 {
    if image {
        LedgerLinkKindV1::Image
    } else if raw_source.trim_start().starts_with('<') || raw_source.trim() == target {
        LedgerLinkKindV1::Autolink
    } else if raw_source.contains("](") {
        LedgerLinkKindV1::Inline
    } else {
        LedgerLinkKindV1::Reference
    }
}

fn split_fragment(target: &str) -> (&str, Option<String>) {
    match target.split_once('#') {
        Some((path, fragment)) => (path, (!fragment.is_empty()).then(|| fragment.to_owned())),
        None => (target, None),
    }
}

fn is_external_target(target: &str) -> bool {
    let Some((scheme, _)) = target.split_once(':') else {
        return false;
    };
    !scheme.is_empty()
        && scheme
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '+' | '-' | '.'))
}

fn is_media_path(target: &str) -> bool {
    let target = target.split('?').next().unwrap_or(target).to_ascii_lowercase();
    [
        ".png", ".jpg", ".jpeg", ".gif", ".webp", ".svg", ".mp3", ".wav", ".mp4",
        ".mov", ".webm",
    ]
    .iter()
    .any(|extension| target.ends_with(extension))
}

fn normalize_relative_target(source_path: &str, target: &str) -> Option<String> {
    let target = target.split('?').next().unwrap_or(target);
    let mut parts = if target.starts_with('/') {
        Vec::new()
    } else {
        source_path
            .replace('\\', "/")
            .rsplit_once('/')
            .map(|(parent, _)| parent.split('/').map(str::to_owned).collect())
            .unwrap_or_default()
    };
    if target.is_empty() {
        return Some(source_path.replace('\\', "/").nfc().collect());
    }
    for part in target.replace('\\', "/").trim_start_matches('/').split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            part => parts.push(part.nfc().collect()),
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let (Some(high), Some(low)) = (hex_digit(bytes[index + 1]), hex_digit(bytes[index + 2])) {
                output.push((high << 4) | low);
                index += 3;
                continue;
            }
        }
        output.push(bytes[index]);
        index += 1;
    }
    String::from_utf8(output).unwrap_or_else(|_| value.to_owned())
}

fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
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
    let start = starts.get(start_line - 1)?.checked_add(start_column.checked_sub(1)?)?;
    let line_end = starts.get(end_line).copied().unwrap_or(markdown.len());
    let mut end = starts
        .get(end_line - 1)?
        .saturating_add(end_column)
        .min(line_end)
        .min(markdown.len());
    if start >= end || start >= markdown.len() || !markdown.is_char_boundary(start) {
        return None;
    }
    while end > start && !markdown.is_char_boundary(end) {
        end -= 1;
    }
    (start < end).then_some((start, end))
}

fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn resolve_registered_target(
    tx: &Transaction<'_>,
    repository_root: &str,
    target_path: &str,
) -> rusqlite::Result<Option<(String, String, String)>> {
    let exact = tx.query_row(
        "SELECT doc_id,revision,content_hash FROM ledger_doc_artifacts
         WHERE repository_root=?1 AND path=?2 AND lifecycle_state='active'",
        rusqlite::params![repository_root, target_path],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        },
    );
    match exact {
        Ok(target) => return Ok(Some(target)),
        Err(rusqlite::Error::QueryReturnedNoRows) => {}
        Err(error) => return Err(error),
    }
    let normalized_target: String = target_path.nfc().collect();
    let mut statement = tx.prepare(
        "SELECT doc_id,revision,content_hash,path FROM ledger_doc_artifacts
         WHERE repository_root=?1 AND lifecycle_state='active' ORDER BY path",
    )?;
    let mapped = statement.query_map([repository_root], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
        ))
    })?;
    for row in mapped {
        let (doc_id, revision, hash, path) = row?;
        if path.nfc().eq(normalized_target.chars()) {
            return Ok(Some((doc_id, revision, hash)));
        }
    }
    Ok(None)
}

fn document_is_live(conn: &Connection, doc_id: &str) -> bool {
    let current = conn.query_row(
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
    );
    current.ok().is_some_and(|(root, path, expected)| {
        std::fs::read(Path::new(&root).join(path))
            .ok()
            .is_some_and(|bytes| digest(&bytes) == expected)
    })
}
