//! Machine-local Markdown projections for lexical & structural retrieval.

use crate::outline::build_outline;
use crate::MemDb;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProjectionConfig {
    pub max_tokens: usize,
    pub enable_h2_split: bool,
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
    let selected = outline
        .sections
        .iter()
        .filter(|section| section.level == 1 || (config.enable_h2_split && section.level == 2));
    for section in selected {
        let content = markdown[section.start_byte..section.end_byte].to_owned();
        projections.push(DocumentProjectionV1 {
            kind: ProjectionKind::Section,
            token_count: tokenizer.count(&content),
            content,
            provenance: ProjectionProvenanceV1 {
                anchor_id: section.anchor_id.clone(),
                collapsed_to_parent: (config.enable_h2_split && section.level == 2)
                    .then(|| section.parent_anchor_id.clone())
                    .flatten(),
            },
        });
    }
    projections
}
