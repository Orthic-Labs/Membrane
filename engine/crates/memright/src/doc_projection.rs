//! Machine-local Markdown projections for lexical & structural retrieval.

use crate::outline::build_outline;

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

pub trait TokenCounter {
    fn count(&self, text: &str) -> usize;
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
    let selected = outline.sections.iter().filter(|section| {
        section.level == 1 || (config.enable_h2_split && section.level == 2)
    });
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
