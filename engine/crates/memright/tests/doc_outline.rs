use memright::outline::build_outline;
use memright::outline::read_section;
use memright::outline::read_section_with_cursor;
use memright::outline::DocReadError;

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
    let markdown = "# Visible\n\n<div>\n# Not a heading\n</div>\n\nVisible body.\n\nHeading\n=======\n";

    let outline = build_outline("doc://repo/worktree/fixture.md", markdown, "spoofed-parser-0");

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
    assert_eq!(continuation.neighbor_anchors.next.as_deref(), Some("sec:child:1"));
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
