// Parity test for legacy blueprint/src/lib/findings/specifier.mjs, a thin
// re-export of blueprint/src/graph/resolution/index.mjs, ported natively as
// engine/crates/membrane-blueprint/src/lib_findings_specifier.rs.
//
// Scenarios translated from blueprint/tests/resolution-owner.test.mjs:
//   - "candidatePaths exact-first ordering (exact, .ts rewrite, index)"
//   - "resolveSpecifier exact-first first match wins with alternatives evidence"
//   - "isRelativeSpecifier gates external vs relative"
//   - "classifyResolution emits typed omission for unsupported/partial/dynamic/generated/external, null only when closed"

use membrane_blueprint::lib_findings_specifier::{candidate_paths, classify_resolution, is_relative_specifier, resolve_specifier, resolution_unsupported_omission, ClassifyInput, OmissionInput, RESOLUTION_OMISSION_CODE};
use std::collections::BTreeSet;

fn set(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|s| s.to_string()).collect()
}

#[test]
fn candidate_paths_exact_first_ordering() {
    let candidates = candidate_paths("src/views/a.ts", "./b.js");
    assert_eq!(candidates[0], "src/views/b.js");
    assert!(candidates.contains(&"src/views/b.ts".to_owned()));
    assert!(candidates.contains(&"src/views/b/index.ts".to_owned()));
    assert!(candidates.contains(&"src/views/b/index.js".to_owned()));
    let dedup: BTreeSet<&String> = candidates.iter().collect();
    assert_eq!(candidates.len(), dedup.len());
}

#[test]
fn resolve_specifier_exact_first_first_match_wins_with_alternatives_evidence() {
    let file_set = set(&["src/b.ts", "src/b.js", "src/b/index.ts"]);
    let result = resolve_specifier("src/a.ts", "./b.js", &file_set);
    assert_eq!(result.resolved.as_deref(), Some("src/b.js"));
    assert_eq!(result.alternatives, 2);

    let only_exact = resolve_specifier("src/a.ts", "./b", &set(&["src/b.ts"]));
    assert_eq!(only_exact.resolved.as_deref(), Some("src/b.ts"));

    let missing = resolve_specifier("src/a.ts", "./missing.js", &set(&["src/other.ts"]));
    assert_eq!(missing.resolved, None);
    assert_eq!(missing.alternatives, 0);
}

#[test]
fn is_relative_specifier_gates_external_vs_relative() {
    assert!(is_relative_specifier("./a"));
    assert!(is_relative_specifier("../a"));
    assert!(!is_relative_specifier("some-package"));
    assert!(!is_relative_specifier("@scope/pkg"));
    assert!(!is_relative_specifier("/absolute"));
}

#[test]
fn resolution_unsupported_omission_carries_typed_fields() {
    let omission = resolution_unsupported_omission(&OmissionInput {
        detail: Some("bare_specifier"),
        reason: Some("external"),
        specifier: Some("react"),
        path: Some("src/a.ts"),
        line: Some(3.0),
    });
    assert_eq!(omission["code"], RESOLUTION_OMISSION_CODE);
    assert_eq!(omission["reason"], "external");
    assert_eq!(omission["detail"], "bare_specifier");
    assert_eq!(omission["specifier"], "react");
}

#[test]
fn classify_resolution_emits_typed_omission_or_null_when_closed() {
    // external
    let external = classify_resolution(&ClassifyInput { specifier: Some("react"), ..Default::default() }).unwrap();
    assert_eq!(external["code"], "resolution_unsupported");
    assert_eq!(external["reason"], "external");

    // generated
    let generated = classify_resolution(&ClassifyInput { specifier: Some("./a"), is_generated: true, ..Default::default() }).unwrap();
    assert_eq!(generated["reason"], "generated");

    // partial parse
    let partial = classify_resolution(&ClassifyInput { specifier: Some("./a"), parse_status: Some("failed"), ..Default::default() }).unwrap();
    assert_eq!(partial["reason"], "partial");

    // open surface
    let open_surface = classify_resolution(&ClassifyInput {
        specifier: Some("./a"),
        target_surface_open_reasons: Some(vec!["commonjs_exports".to_owned()]),
        parse_status: Some("ok"),
        ..Default::default()
    })
    .unwrap();
    assert_eq!(open_surface["reason"], "partial");

    // closed surface -> no omission
    let closed = classify_resolution(&ClassifyInput {
        specifier: Some("./a"),
        target_surface_open_reasons: Some(vec![]),
        parse_status: Some("ok"),
        ..Default::default()
    });
    assert!(closed.is_none());
}
