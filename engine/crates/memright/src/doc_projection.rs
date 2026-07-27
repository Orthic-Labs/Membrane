//! Machine-local Markdown projections for lexical & structural retrieval.

use crate::outline::build_outline;
use crate::MemDb;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProjectionConfig {
    pub max_tokens: usize,
    pub h2_replay_gate: H2ReplayGateReceiptV1,
}

/// Explicit replay evidence required before heading-child projection is enabled.
///
/// New installations remain document/H1-only until frozen replay closes this gate.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct H2ReplayGateReceiptV1 {
    pub passed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectionKind {
    Lexical,
    WholeDocument,
    Section,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectionProvenanceV1 {
    pub anchor_id: String,
    pub collapsed_to_parent: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocumentProjectionV1 {
    pub kind: ProjectionKind,
    pub content: String,
    pub token_count: usize,
    pub provenance: ProjectionProvenanceV1,
}

/// Regenerable, machine-local projection set for one registered document.
///
/// Replacing this input removes every prior projection belonging to `parent_doc_id` in the same
/// transaction, so reconciliation cannot expose rows from an older source revision.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocumentProjectionStoreInputV1 {
    pub parent_doc_id: String,
    pub source_content_hash: String,
    pub source_revision: String,
    pub index_generation: i64,
    pub projections: Vec<DocumentProjectionV1>,
}

pub trait TokenCounter {
    fn count(&self, text: &str) -> usize;
}

impl ProjectionKind {
    fn storage_name(self) -> &'static str {
        match self {
            Self::Lexical => "lexical",
            Self::WholeDocument => "whole_document",
            Self::Section => "section",
        }
    }
}

/// Atomically replace machine-local projections for one document.
///
/// This table is deliberately outside mirrored memory tables: projections are derived from each
/// machine's checkout & are rebuilt by document reconciliation.
pub fn replace_doc_projections(
    db: &MemDb,
    input: &DocumentProjectionStoreInputV1,
) -> rusqlite::Result<()> {
    let mut conn = db.lock();
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS doc_projections (
            parent_doc_id TEXT NOT NULL,
            kind TEXT NOT NULL,
            content TEXT NOT NULL,
            token_count INTEGER NOT NULL,
            anchor_id TEXT NOT NULL,
            collapsed_to_parent TEXT,
            source_content_hash TEXT NOT NULL,
            source_revision TEXT NOT NULL,
            index_generation INTEGER NOT NULL,
            PRIMARY KEY(parent_doc_id, kind, anchor_id)
        );
        CREATE INDEX IF NOT EXISTS idx_doc_projections_parent_generation
            ON doc_projections(parent_doc_id, index_generation);",
    )?;
    let tx = conn.transaction()?;
    tx.execute(
        "DELETE FROM doc_projections WHERE parent_doc_id=?1",
        [&input.parent_doc_id],
    )?;
    {
        let mut insert = tx.prepare(
            "INSERT INTO doc_projections (
                parent_doc_id, kind, content, token_count, anchor_id, collapsed_to_parent,
                source_content_hash, source_revision, index_generation
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        )?;
        for projection in &input.projections {
            insert.execute(rusqlite::params![
                input.parent_doc_id,
                projection.kind.storage_name(),
                projection.content,
                projection.token_count as i64,
                projection.provenance.anchor_id,
                projection.provenance.collapsed_to_parent,
                input.source_content_hash,
                input.source_revision,
                input.index_generation,
            ])?;
        }
    }
    tx.commit()
}

#[cfg(feature = "llmlingua-onnx")]
impl TokenCounter for tokenizers::Tokenizer {
    fn count(&self, text: &str) -> usize {
        self.encode(text, false)
            .map(|encoding| encoding.len())
            .unwrap_or(usize::MAX)
    }
}

pub fn project_markdown(
    markdown: &str,
    tokenizer: &impl TokenCounter,
    config: ProjectionConfig,
) -> Vec<DocumentProjectionV1> {
    let whole_document_tokens = tokenizer.count(markdown);
    let mut projections = vec![DocumentProjectionV1 {
        kind: ProjectionKind::Lexical,
        content: markdown.to_owned(),
        token_count: whole_document_tokens,
        provenance: ProjectionProvenanceV1 {
            anchor_id: "document".to_owned(),
            collapsed_to_parent: None,
        },
    }];

    if config.max_tokens == 0 || whole_document_tokens <= config.max_tokens {
        projections.push(DocumentProjectionV1 {
            kind: ProjectionKind::WholeDocument,
            content: markdown.to_owned(),
            token_count: whole_document_tokens,
            provenance: ProjectionProvenanceV1 {
                anchor_id: "document".to_owned(),
                collapsed_to_parent: None,
            },
        });
        return projections;
    }

    let outline = build_outline("doc://projection", markdown, "ignored");
    for (index, section) in outline.sections.iter().enumerate() {
        if section.level == 1 {
            projections.extend(cascade_section(
                markdown,
                tokenizer,
                config,
                &outline.sections,
                index,
            ));
        }
    }
    projections
}

fn cascade_section(
    markdown: &str,
    tokenizer: &impl TokenCounter,
    config: ProjectionConfig,
    sections: &[crate::outline::DocSectionV1],
    index: usize,
) -> Vec<DocumentProjectionV1> {
    let section = &sections[index];
    let content = &markdown[section.start_byte..section.end_byte];
    if tokenizer.count(content) <= config.max_tokens {
        return vec![DocumentProjectionV1 {
            kind: ProjectionKind::Section,
            token_count: tokenizer.count(content),
            content: content.to_owned(),
            provenance: ProjectionProvenanceV1 {
                anchor_id: section.anchor_id.clone(),
                collapsed_to_parent: section.parent_anchor_id.clone(),
            },
        }];
    }

    let children = sections
        .iter()
        .enumerate()
        .filter(|(_, candidate)| candidate.parent_anchor_id.as_deref() == Some(&section.anchor_id))
        .collect::<Vec<_>>();
    if config.h2_replay_gate.passed && !children.is_empty() {
        let mut projections = Vec::new();
        let first_child_start = children[0].1.start_byte;
        projections.extend(subdivide_content(
            tokenizer,
            config.max_tokens,
            &markdown[section.start_byte..first_child_start],
            &section.anchor_id,
            None,
        ));
        for (child_index, _) in children {
            projections.extend(cascade_section(
                markdown,
                tokenizer,
                config,
                sections,
                child_index,
            ));
        }
        return projections;
    }

    subdivide_content(
        tokenizer,
        config.max_tokens,
        content,
        &section.anchor_id,
        section.parent_anchor_id.clone(),
    )
}

fn subdivide_content(
    tokenizer: &impl TokenCounter,
    max_tokens: usize,
    content: &str,
    anchor_id: &str,
    collapsed_to_parent: Option<String>,
) -> Vec<DocumentProjectionV1> {
    let blocks = structural_blocks(content);
    let mut chunks = Vec::<String>::new();
    let mut current = String::new();
    for (block, is_fence) in blocks {
        if !current.is_empty() && tokenizer.count(&(current.clone() + &block)) > max_tokens {
            chunks.push(std::mem::take(&mut current));
        }
        if tokenizer.count(&block) > max_tokens && !is_fence {
            if !current.is_empty() {
                chunks.push(std::mem::take(&mut current));
            }
            chunks.extend(bounded_subdivide(tokenizer, max_tokens, &block));
        } else {
            current.push_str(&block);
        }
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    let chunk_count = chunks.len();
    chunks
        .into_iter()
        .enumerate()
        .filter(|(_, content)| !content.trim().is_empty())
        .map(|(index, content)| DocumentProjectionV1 {
            kind: ProjectionKind::Section,
            token_count: tokenizer.count(&content),
            content,
            provenance: ProjectionProvenanceV1 {
                anchor_id: if chunk_count == 1 {
                    anchor_id.to_owned()
                } else {
                    format!("{anchor_id}:part:{}", index + 1)
                },
                collapsed_to_parent: collapsed_to_parent.clone(),
            },
        })
        .collect()
}

/// Paragraph/table blocks are retained together; fenced blocks remain indivisible.
fn structural_blocks(content: &str) -> Vec<(String, bool)> {
    let lines = content.split_inclusive('\n').collect::<Vec<_>>();
    let mut blocks = Vec::new();
    let mut current = String::new();
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index];
        let trimmed = line.trim_start();
        let fence = trimmed.starts_with("```") || trimmed.starts_with("~~~");
        if fence {
            if !current.is_empty() {
                blocks.push((std::mem::take(&mut current), false));
            }
            let marker = &trimmed[..3];
            let mut fenced = String::from(line);
            index += 1;
            while index < lines.len() {
                let candidate = lines[index];
                fenced.push_str(candidate);
                index += 1;
                if candidate.trim_start().starts_with(marker) {
                    break;
                }
            }
            blocks.push((fenced, true));
            continue;
        }
        current.push_str(line);
        index += 1;
        if line.trim().is_empty() && !current.trim().is_empty() {
            blocks.push((std::mem::take(&mut current), false));
        }
    }
    if !current.is_empty() {
        blocks.push((current, false));
    }
    blocks
}

fn bounded_subdivide(
    tokenizer: &impl TokenCounter,
    max_tokens: usize,
    content: &str,
) -> Vec<String> {
    if max_tokens == 0 || tokenizer.count(content) <= max_tokens {
        return vec![content.to_owned()];
    }
    let mut remaining = content;
    let mut chunks = Vec::new();
    while !remaining.is_empty() && tokenizer.count(remaining) > max_tokens {
        let mut end = 0;
        for (offset, character) in remaining.char_indices() {
            let candidate_end = offset + character.len_utf8();
            if tokenizer.count(&remaining[..candidate_end]) > max_tokens {
                break;
            }
            end = candidate_end;
        }
        if end == 0 {
            end = remaining
                .char_indices()
                .nth(1)
                .map(|(offset, _)| offset)
                .unwrap_or(remaining.len());
        }
        let whitespace_end = remaining[..end]
            .char_indices()
            .filter_map(|(offset, character)| {
                character
                    .is_whitespace()
                    .then_some(offset + character.len_utf8())
            })
            .last()
            .unwrap_or(end);
        chunks.push(remaining[..whitespace_end].to_owned());
        remaining = &remaining[whitespace_end..];
    }
    if !remaining.is_empty() {
        chunks.push(remaining.to_owned());
    }
    chunks
}
