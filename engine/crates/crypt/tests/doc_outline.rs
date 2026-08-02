use crypt::outline::build_outline;
use crypt::outline::build_outline_page;
use crypt::outline::read_section;
use crypt::outline::read_section_with_cursor;
use crypt::outline::DocReadError;

#[test]
fn outline_preserves_frontmatter_fenced_headings_and_duplicate_anchors() {
    let markdown = "---\ntitle: Fixture\n---\nPreamble.\n\n# Top\n\n## Repeat\n\n```md\n# Not a heading\n```\n\n## Repeat\n";

    let outline = build_outline("doc://repo/worktree/fixture.md", markdown, "comrak-0");

    assert_eq!(outline.schema_version, "DocOutlineV1");
    assert_eq!(outline.source_ref, "doc://repo/worktree/fixture.md");
    assert_eq!(outline.sections[0].anchor_id, "sec:_frontmatter:1");
    assert_eq!(outline.sections[1].anchor_id, "sec:preamble:1");
    assert_eq!(outline.sections[2].anchor_id, "sec:top:1");
    assert_eq!(outline.sections[3].anchor_id, "sec:repeat:1");
    assert_eq!(outline.sections[4].anchor_id, "sec:repeat:2");
    assert_eq!(
        outline.sections[4].parent_anchor_id.as_deref(),
        Some("sec:top:1")
    );
    assert_eq!(outline.sections.len(), 5);
}

#[test]
fn doc_read_refuses_changed_source_and_returns_neighbors() {
    let markdown = "# Top\nalpha\n\n## Child\nbeta\n";
    let outline = build_outline("doc://repo/worktree/fixture.md", markdown, "comrak-0");
    assert_eq!(
        read_section(
            "doc://repo/worktree/fixture.md",
            markdown,
            "sec:top:1",
            "wrong",
            100
        )
        .unwrap_err(),
        DocReadError::SourceChanged
    );
    let read = read_section(
        "doc://repo/worktree/fixture.md",
        markdown,
        "sec:top:1",
        &outline.content_hash,
        100,
    )
    .unwrap();
    assert_eq!(read.neighbor_anchors.next.as_deref(), Some("sec:child:1"));
}

#[test]
fn outline_uses_implementation_fixed_gfm_parser_projection() {
    let markdown =
        "# Visible\n\n<div>\n# Not a heading\n</div>\n\nVisible body.\n\nHeading\n=======\n";

    let outline = build_outline(
        "doc://repo/worktree/fixture.md",
        markdown,
        "spoofed-parser-0",
    );

    assert_eq!(outline.parser.name, "comrak");
    assert_eq!(outline.parser.version, "0.54.0");
    assert_eq!(
        outline
            .sections
            .iter()
            .map(|section| section.anchor_id.as_str())
            .collect::<Vec<_>>(),
        vec!["sec:visible:1", "sec:heading:1"]
    );
}

#[test]
fn doc_read_has_utf8_safe_bounded_hash_bound_continuation() {
    let markdown = "# Top\né🙂z\n\n## Child\nchild\n";
    let outline = build_outline("doc://repo/worktree/fixture.md", markdown, "comrak-0.54.0");

    let first = read_section(
        "doc://repo/worktree/fixture.md",
        markdown,
        "sec:top:1",
        &outline.content_hash,
        5,
    )
    .unwrap();
    assert_eq!(first.content, "# Top");
    assert!(first.truncated);
    assert_eq!(first.neighbor_anchors.next.as_deref(), Some("sec:child:1"));
    let cursor = first.continuation_cursor.as_deref().unwrap();

    let continuation = read_section_with_cursor(
        "doc://repo/worktree/fixture.md",
        markdown,
        "sec:top:1",
        &outline.content_hash,
        7,
        Some(cursor),
    )
    .unwrap();
    assert_eq!(continuation.content, "\né🙂");
    assert!(continuation.truncated);
    assert_eq!(
        continuation.neighbor_anchors.next.as_deref(),
        Some("sec:child:1")
    );
}

#[test]
fn doc_read_exposes_typed_source_outcomes() {
    let markdown = "# Top\nbody\n";
    let outline = build_outline("doc://repo/worktree/fixture.md", markdown, "comrak-0.54.0");

    assert_eq!(
        read_section(
            "doc://repo/worktree/fixture.md",
            markdown,
            "sec:top:1",
            "wrong",
            100,
        )
        .unwrap_err(),
        DocReadError::SourceChanged
    );
    assert_eq!(
        read_section(
            "doc://repo/worktree/fixture.md",
            markdown,
            "sec:missing:1",
            &outline.content_hash,
            100,
        )
        .unwrap_err(),
        DocReadError::Relocated
    );
    assert_eq!(DocReadError::SourceMissing.as_str(), "source_missing");
    assert_eq!(DocReadError::Deny.as_str(), "deny");
}

#[test]
fn outline_pages_are_bounded_hash_bound_and_resumable() {
    let markdown = "# One\none\n# Two\ntwo\n# Three\nthree\n";
    let first = build_outline_page(
        "doc://repo/worktree/fixture.md",
        markdown,
        "ignored",
        2,
        None,
    )
    .unwrap();

    assert_eq!(first.sections.len(), 2);
    assert!(first.truncated);
    let cursor = first
        .continuation_cursor
        .as_deref()
        .expect("truncated outline has cursor");

    let second = build_outline_page(
        "doc://repo/worktree/fixture.md",
        markdown,
        "ignored",
        2,
        Some(cursor),
    )
    .unwrap();
    assert_eq!(second.sections.len(), 1);
    assert_eq!(second.sections[0].anchor_id, "sec:three:1");
    assert!(!second.truncated);
    assert!(second.continuation_cursor.is_none());
    assert_eq!(second.content_hash, first.content_hash);
    assert_eq!(second.outline_hash, first.outline_hash);

    assert_eq!(
        build_outline_page(
            "doc://repo/worktree/fixture.md",
            "# One\nchanged\n# Two\ntwo\n# Three\nthree\n",
            "ignored",
            2,
            Some(cursor),
        )
        .unwrap_err(),
        DocReadError::SourceChanged
    );
}

#[test]
fn outline_reports_tokenizer_measured_model_estimates() {
    let markdown = "# Tokens\nhello, tokenizer world\n";
    let outline = build_outline("doc://repo/worktree/fixture.md", markdown, "ignored");
    let section = &outline.sections[0];
    let span = &markdown[section.start_byte..section.end_byte];

    assert_eq!(
        section.token_estimates.per_model.get("o200k_base"),
        Some(&crypt_core::estimate_tokens(span))
    );
    assert!(!section
        .token_estimates
        .per_model
        .contains_key("heuristic-v1"));
}
