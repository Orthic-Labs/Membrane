//! Parity tests for `lib_comment_claims` (native port of
//! `blueprint/src/lib/comment-claims.mjs`), lane LIB4 (r5 closure).

use membrane_blueprint::lib_comment_claims::{
    extract_comment_claims, ExtractCommentClaimsInput, COMMENT_CLAIM_KINDS,
};

#[test]
fn claim_kinds_match_legacy_set() {
    assert_eq!(
        COMMENT_CLAIM_KINDS,
        ["NOTE", "WHY", "HACK", "TODO", "DEPRECATED", "OWNER"]
    );
}

#[test]
fn sha1_field_matches_known_answer_vector() {
    // Cross-checked against Node's `createHash("sha1")` for the same
    // cleaned text (the prior hand-rolled FIPS 180-4 SHA-1 in this module
    // agreed byte-for-byte with this vector too -- no bug found there):
    //   node -e "console.log(require('crypto').createHash('sha1').update('TODO: abc').digest('hex'))"
    // => b9aa1504e77b617f560697e7df5c7fdb4a0adfbe
    let claims = extract_comment_claims(ExtractCommentClaimsInput {
        text: "TODO: abc",
        path: "src/foo.rs",
        line: 1,
        enclosing_symbol_id: None,
    });
    assert_eq!(claims.len(), 1);
    assert_eq!(claims[0].sha1, "b9aa1504e77b617f560697e7df5c7fdb4a0adfbe");
}

#[test]
fn extracts_todo_claim_with_owner() {
    let claims = extract_comment_claims(ExtractCommentClaimsInput {
        text: "// TODO(alice): fix the retry loop",
        path: "src/foo.rs",
        line: 10,
        enclosing_symbol_id: Some("sym:foo".to_string()),
    });
    assert_eq!(claims.len(), 1);
    let claim = &claims[0];
    assert_eq!(claim.document_id, "comment:src/foo.rs:10");
    assert_eq!(claim.source, "src/foo.rs");
    assert_eq!(claim.line, 10);
    assert_eq!(claim.status, "implemented");
    assert!(claim.id.starts_with("claim:"));
    assert_eq!(claim.edges.len(), 1);
    let edge = &claim.edges[0];
    assert_eq!(edge.kind, "COMMENT_CLAIM");
    assert_eq!(edge.claim_kind, "TODO");
    assert_eq!(edge.owner.as_deref(), Some("alice"));
    assert_eq!(edge.body, "fix the retry loop");
    assert_eq!(edge.lifecycle, "data_only");
    assert_eq!(edge.enclosing_symbol_id.as_deref(), Some("sym:foo"));
    assert_eq!(edge.span_start_line, 10);
    assert_eq!(edge.span_end_line, 10);
}

#[test]
fn extracts_hack_claim_without_owner() {
    let claims = extract_comment_claims(ExtractCommentClaimsInput {
        text: "# HACK: workaround for flaky upstream API",
        path: "src/bar.py",
        line: 5,
        enclosing_symbol_id: None,
    });
    assert_eq!(claims.len(), 1);
    let edge = &claims[0].edges[0];
    assert_eq!(edge.claim_kind, "HACK");
    assert_eq!(edge.owner, None);
    assert_eq!(edge.body, "workaround for flaky upstream API");
    assert_eq!(edge.enclosing_symbol_id, None);
}

#[test]
fn extracts_deprecated_claim_with_description_as_owner_and_body() {
    // JS quirk preserved: for DEPRECATED/OWNER patterns there is only one
    // capture group, so it is used as BOTH `owner` and `body` (match[2] is
    // undefined, falls back to match[1]).
    let claims = extract_comment_claims(ExtractCommentClaimsInput {
        text: "// deprecated: use newFn instead",
        path: "src/old.rs",
        line: 1,
        enclosing_symbol_id: None,
    });
    assert_eq!(claims.len(), 1);
    let edge = &claims[0].edges[0];
    assert_eq!(edge.claim_kind, "DEPRECATED");
    assert_eq!(edge.owner.as_deref(), Some("use newFn instead"));
    assert_eq!(edge.body, "use newFn instead");
}

#[test]
fn extracts_owner_claim() {
    let claims = extract_comment_claims(ExtractCommentClaimsInput {
        text: "// owner: team-platform",
        path: "src/svc.rs",
        line: 2,
        enclosing_symbol_id: None,
    });
    assert_eq!(claims.len(), 1);
    let edge = &claims[0].edges[0];
    assert_eq!(edge.claim_kind, "OWNER");
    assert_eq!(edge.owner.as_deref(), Some("team-platform"));
    assert_eq!(edge.body, "team-platform");
}

#[test]
fn plain_comment_with_no_matching_kind_yields_no_claims() {
    let claims = extract_comment_claims(ExtractCommentClaimsInput {
        text: "// just a normal comment, nothing special here",
        path: "src/plain.rs",
        line: 3,
        enclosing_symbol_id: None,
    });
    assert!(claims.is_empty());
}

#[test]
fn only_kind_reaching_true_end_of_cleaned_text_matches_across_lines() {
    // The legacy KIND_PATTERNS regexes have no `m` flag, so `(.+)$` must
    // reach the true end of the (multi-line) cleaned string -- `.` cannot
    // cross a newline in JS regex without the `s` flag. With two stacked
    // comment lines, only the LAST line's kind can match; an earlier
    // line's pattern cannot satisfy `$` because more text follows it.
    let claims = extract_comment_claims(ExtractCommentClaimsInput {
        text: "// TODO: fix this\n// NOTE: careful here",
        path: "src/multi.rs",
        line: 7,
        enclosing_symbol_id: None,
    });
    let kinds: Vec<&str> = claims.iter().map(|c| c.edges[0].claim_kind).collect();
    assert_eq!(kinds, vec!["NOTE"]);
}

#[test]
fn claim_ids_are_deterministic_and_content_bound() {
    let make = || {
        extract_comment_claims(ExtractCommentClaimsInput {
            text: "// WHY: because reasons",
            path: "src/det.rs",
            line: 42,
            enclosing_symbol_id: None,
        })
    };
    let a = make();
    let b = make();
    assert_eq!(a[0].id, b[0].id);

    // Different line -> different id.
    let c = extract_comment_claims(ExtractCommentClaimsInput {
        text: "// WHY: because reasons",
        path: "src/det.rs",
        line: 43,
        enclosing_symbol_id: None,
    });
    assert_ne!(a[0].id, c[0].id);
}

#[test]
fn sha1_field_is_stable_content_fingerprint_of_cleaned_text() {
    let claims1 = extract_comment_claims(ExtractCommentClaimsInput {
        text: "// TODO: same body",
        path: "a.rs",
        line: 1,
        enclosing_symbol_id: None,
    });
    let claims2 = extract_comment_claims(ExtractCommentClaimsInput {
        text: "// TODO: same body",
        path: "b.rs",
        line: 99,
        enclosing_symbol_id: None,
    });
    // sha1 is computed over the cleaned comment text only, independent of
    // path/line, so it matches across different locations with identical
    // comment bodies.
    assert_eq!(claims1[0].sha1, claims2[0].sha1);
    assert_eq!(claims1[0].sha1.len(), 40);
}
