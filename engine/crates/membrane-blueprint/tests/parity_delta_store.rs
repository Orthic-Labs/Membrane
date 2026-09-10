//! Parity tests for `membrane_blueprint::delta_store` against the pure
//! logic ported from `blueprint/src/graph/delta-store.mjs`
//! (`normalizeDigest`, the `domains_pending` watch-state helpers, and
//! `replaceBySource`). See that module's doc comment for why
//! `applyFileDelta` itself (the sqlite-orchestrated transactional core) is
//! out of scope: this crate's native engine has no equivalent incremental
//! schema to port it onto.

use membrane_blueprint::delta_store::{normalize_digest, replace_by_source, PendingDomains, SourceKeyed};

#[test]
fn normalize_digest_matches_legacy_prefixing() {
    // Legacy: normalizeDigest(value) => text.startsWith("xxh128:") ? text : `xxh128:${text}`
    assert_eq!(normalize_digest("deadbeef"), "xxh128:deadbeef");
    assert_eq!(normalize_digest("xxh128:deadbeef"), "xxh128:deadbeef");
    assert_eq!(normalize_digest(""), "xxh128:");
}

#[test]
fn pending_domains_matches_legacy_watch_state_semantics() {
    // Legacy readPendingDomains: split(",").trim().filter(Boolean).sort() —
    // note this does NOT dedup, matching the ported behaviour exactly.
    let read = PendingDomains::from_stored("structural, doc,,doc");
    assert_eq!(read.domains(), &["doc".to_string(), "doc".to_string(), "structural".to_string()]);

    // Legacy markDomainPending: append if absent, store sorted-join.
    let mut marked = PendingDomains::from_stored("");
    marked.mark("structural");
    assert_eq!(marked.to_stored().as_deref(), Some("structural"));
    marked.mark("doc");
    assert_eq!(marked.to_stored().as_deref(), Some("doc,structural"));

    // Legacy clearDomainPending: filter out; delete row (None) when empty.
    marked.clear("structural");
    assert_eq!(marked.to_stored().as_deref(), Some("doc"));
    marked.clear("doc");
    assert_eq!(marked.to_stored(), None, "legacy deletes the watch_state row once no domain remains pending");
}

#[test]
fn replace_by_source_matches_legacy_splice_semantics() {
    // Mirrors replaceDocumentArtifacts's use of replaceBySource to swap a
    // renamed/edited document's stale-claims entries in place.
    let entries = vec![
        SourceKeyed { source: "docs/a.md".to_string(), payload: "old-a-1" },
        SourceKeyed { source: "docs/b.md".to_string(), payload: "b" },
        SourceKeyed { source: "docs/a.md".to_string(), payload: "old-a-2" },
    ];
    let replacements = vec![SourceKeyed { source: "docs/a.md".to_string(), payload: "new-a" }];

    let result = replace_by_source(&entries, "docs/a.md", &replacements);
    assert_eq!(
        result,
        vec![
            SourceKeyed { source: "docs/a.md".to_string(), payload: "new-a" },
            SourceKeyed { source: "docs/b.md".to_string(), payload: "b" },
        ],
        "both old entries for the source collapse to the replacement set, inserted at the first match's position"
    );
}

#[test]
fn replace_by_source_is_a_pure_append_for_a_brand_new_source() {
    let entries = vec![SourceKeyed { source: "docs/b.md".to_string(), payload: "b" }];
    let replacements = vec![SourceKeyed { source: "docs/new.md".to_string(), payload: "new" }];
    let result = replace_by_source(&entries, "docs/new.md", &replacements);
    assert_eq!(
        result,
        vec![
            SourceKeyed { source: "docs/b.md".to_string(), payload: "b" },
            SourceKeyed { source: "docs/new.md".to_string(), payload: "new" },
        ]
    );
}

#[test]
fn replace_by_source_with_empty_replacements_deletes_the_source() {
    let entries = vec![
        SourceKeyed { source: "docs/a.md".to_string(), payload: "a" },
        SourceKeyed { source: "docs/b.md".to_string(), payload: "b" },
    ];
    let result: Vec<SourceKeyed<&str>> = replace_by_source(&entries, "docs/a.md", &[]);
    assert_eq!(result, vec![SourceKeyed { source: "docs/b.md".to_string(), payload: "b" }]);
}
