use memright::outline::build_outline;
use memright::outline::read_section;

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
        "source_changed"
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
