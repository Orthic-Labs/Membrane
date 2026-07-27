use memright::doc_projection::{project_markdown, ProjectionConfig, ProjectionKind, TokenCounter};

struct Words;

impl TokenCounter for Words {
    fn count(&self, text: &str) -> usize {
        text.split_whitespace().count()
    }
}

#[test]
fn lexical_projection_is_always_emitted() {
    let projections = project_markdown("# Title\n\nbody", &Words, ProjectionConfig::default());

    assert_eq!(projections[0].kind, ProjectionKind::Lexical);
    assert_eq!(projections[0].content, "# Title\n\nbody");
}

#[test]
fn tokenizer_measured_whole_document_fit_is_emitted_before_sections() {
    let projections = project_markdown(
        "# Title\n\none two three",
        &Words,
        ProjectionConfig { max_tokens: 5, ..ProjectionConfig::default() },
    );

    assert_eq!(projections.len(), 2);
    assert_eq!(projections[1].kind, ProjectionKind::WholeDocument);
    assert_eq!(projections[1].token_count, 5);
}

#[test]
fn oversized_document_cascades_at_h1_without_splitting_fenced_headings() {
    let markdown = "# First\n\none two\n\n```md\n# Not a section\n```\n\n# Second\n\nthree four";
    let projections = project_markdown(
        markdown,
        &Words,
        ProjectionConfig { max_tokens: 5, ..ProjectionConfig::default() },
    );

    let structural = &projections[1..];
    assert_eq!(structural.len(), 2);
    assert!(structural.iter().all(|projection| projection.kind == ProjectionKind::Section));
    assert_eq!(structural[0].content, "# First\n\none two\n\n```md\n# Not a section\n```\n\n");
    assert_eq!(structural[1].content, "# Second\n\nthree four");
    assert_eq!(structural[0].provenance.anchor_id, "sec:first:1");
}

#[test]
fn h2_split_is_disabled_by_default() {
    let markdown = "# Parent\n\nintro\n\n## Child\n\nbody";
    let projections = project_markdown(
        markdown,
        &Words,
        ProjectionConfig { max_tokens: 2, ..ProjectionConfig::default() },
    );

    assert_eq!(projections.len(), 2);
    assert_eq!(projections[1].provenance.anchor_id, "sec:parent:1");
}

#[test]
fn h2_split_records_collapsed_parent_provenance_when_enabled() {
    let markdown = "# Parent\n\nintro\n\n## Child\n\nbody";
    let projections = project_markdown(
        markdown,
        &Words,
        ProjectionConfig { max_tokens: 2, enable_h2_split: true },
    );

    assert_eq!(projections.len(), 3);
    assert_eq!(projections[1].provenance.anchor_id, "sec:parent:1");
    assert_eq!(projections[2].provenance.anchor_id, "sec:child:1");
    assert_eq!(projections[2].provenance.collapsed_to_parent.as_deref(), Some("sec:parent:1"));
}
