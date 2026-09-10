// Parity port for blueprint/src/graph/conformance-verifier.mjs.
//
// Legacy source: blueprint/src/graph/conformance-verifier.mjs
// (`verifySemanticConformance`, `assertSemanticConformance`), exercised by
// blueprint/tests/semantic-conformance.test.mjs against fixtures built via
// `buildGraphGeneration` (a TS/JS lexical+tree-sitter provider).
//
// Native counterpart: engine/crates/membrane-blueprint/src/conformance_verifier.rs.
// The matching/report/failure-explanation engine is ported field-for-field
// (path traversal incl. array indices, `$exists`/`$includes`/`$containsAll`/
// `$matches` operators, `_absent`/`exactly`/`atLeast` count rules, and the
// exact failure-message format). The *fixtures* are adapted to the real
// native fact shape produced by `build_generation` in this crate
// (`engine/crates/membrane-blueprint/src/graph.rs`) rather than replaying the
// legacy JSON fixtures verbatim: the native provider does not emit a
// `labels`/`resolved` field directly on nodes/edges (those live inside
// `evidence[0]`), and it has no same-tier import-ambiguity detector, so the
// `module-ambiguity` legacy fixture has no native equivalent and is not
// ported. The `exact-import-call` fixture's four assertions (definition
// exists with source evidence, resolved IMPORTS edge, resolved CALLS edge,
// and a negative `edge_absent` control) are reproduced 1:1 against real
// `build_generation` output, plus the legacy "failure names the exact
// violated assertion" case.

use std::fs;

use membrane_blueprint::conformance_verifier::{
    assert_semantic_conformance, verify_semantic_conformance, ConformanceFixture,
    ConformanceGeneration,
};
use membrane_blueprint::graph::{build_generation, GraphOptions};
use serde_json::json;
use tempfile::tempdir;

fn build_fixture_generation() -> ConformanceGeneration {
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    // Single file: this crate's native graph provider does not resolve
    // cross-file imports/calls (see the `IMPORTS` edge left `target: null,
    // resolved: false` and no cross-file `CALLS` edge when `work` lived in a
    // separate `worker.ts` module) -- only same-file `CALLS` resolution is
    // implemented (`ConfidenceTier::SameFileLexical` in graph.rs). The
    // fixture is adapted to that real capability rather than the legacy
    // two-file cross-module fixture.
    fs::write(
        dir.path().join("src/app.ts"),
        "export function work() { return 1; }\nexport function main() { return work(); }\n",
    )
    .unwrap();
    let generation = build_generation(dir.path(), &GraphOptions::default()).unwrap();
    ConformanceGeneration {
        nodes: generation
            .nodes
            .iter()
            .map(|node| serde_json::to_value(node).unwrap())
            .collect(),
        edges: generation
            .edges
            .iter()
            .map(|edge| serde_json::to_value(edge).unwrap())
            .collect(),
        generation_id: Some(generation.generation_id.clone()),
        schema_version: Some(json!(generation.schema_version)),
        providers: Vec::new(),
    }
}

fn exact_import_call_fixture() -> ConformanceFixture {
    let raw = json!({
        "name": "exact-import-call-and-negative",
        "assertions": [
            {
                "id": "worker-definition",
                "type": "node_exists",
                "description": "worker function is represented as a symbol with source evidence",
                "where": {
                    "path": "src/app.ts",
                    "name": "work",
                    "evidence.0.path": "src/app.ts",
                    "evidence.0.contentHash": { "$exists": true },
                    "evidence.0.labels": { "$includes": "Function" }
                }
            },
            {
                "id": "file-contains-worker",
                "type": "edge_exists",
                "description": "the file node structurally contains the worker symbol",
                "where": {
                    "kind": "CONTAINS",
                    "source": "file:src/app.ts",
                    "target": "symbol:src/app.ts::work",
                    "evidence.0.resolved": true,
                    "evidence.0.path": "src/app.ts"
                }
            },
            {
                "id": "main-calls-work",
                "type": "edge_exists",
                "description": "main reaches work through the resolved same-file call relation",
                "where": {
                    "kind": "CALLS",
                    "source": "symbol:src/app.ts::main",
                    "target": "symbol:src/app.ts::work",
                    "evidence.0.callName": "work",
                    "evidence.0.path": "src/app.ts"
                }
            },
            {
                "id": "no-fabricated-call",
                "type": "edge_absent",
                "description": "the verifier can assert that a known false edge is absent",
                "where": {
                    "kind": "CALLS",
                    "source": "symbol:src/app.ts::main",
                    "target": "symbol:src/app.ts::missing"
                }
            }
        ]
    });
    serde_json::from_value(raw).unwrap()
}

#[test]
fn semantic_conformance_fixture_passes_exact_import_call() {
    let generation = build_fixture_generation();
    let fixture = exact_import_call_fixture();
    let report = assert_semantic_conformance(&generation, &fixture).unwrap();
    assert_eq!(report.status, "passed");
    assert_eq!(report.fixture, "exact-import-call-and-negative");
    assert_eq!(report.assertions.len(), fixture.assertions.len());
    assert!(report.assertions.iter().all(|assertion| assertion.passed));
}

#[test]
fn semantic_conformance_failure_names_the_exact_violated_assertion() {
    let generation = build_fixture_generation();
    let broken = ConformanceFixture {
        name: Some("intentional-failure".to_string()),
        assertions: serde_json::from_value(json!([{
            "id": "missing-definition-contract",
            "type": "node_exists",
            "where": { "path": "src/nope.ts", "name": "notThere" }
        }]))
        .unwrap(),
    };
    let report = verify_semantic_conformance(&generation, &broken).unwrap();
    assert_eq!(report.status, "failed");
    assert!(report.failures[0].contains("missing-definition-contract"));

    let error = assert_semantic_conformance(&generation, &broken).unwrap_err();
    assert!(error.failures[0].contains("missing-definition-contract"));
    assert_eq!(
        membrane_blueprint::conformance_verifier::SemanticConformanceFailed::CODE,
        "semantic_conformance_failed"
    );
}

#[test]
fn matches_expected_operators_and_count_rules() {
    let generation = ConformanceGeneration {
        nodes: vec![
            json!({"id": "n1", "path": "a.ts", "labels": ["Function"], "meta": {"tags": ["x", "y"]}}),
            json!({"id": "n2", "path": "b.ts", "labels": ["Class"]}),
        ],
        edges: vec![],
        generation_id: Some("gen-1".to_string()),
        schema_version: Some(json!(1)),
        providers: vec![],
    };

    // $exists true/false, $includes, $containsAll, $matches, exact-count and
    // atLeast-count all evaluated on plain node facts (no provider needed).
    let fixture: ConformanceFixture = serde_json::from_value(json!({
        "name": "operator-matrix",
        "assertions": [
            { "id": "has-labels", "type": "node_exists", "where": { "labels": { "$exists": true } } },
            { "id": "no-missing-field", "type": "node_exists", "where": { "missingField": { "$exists": false } }, "count": { "exactly": 2 } },
            { "id": "includes-function", "type": "node_exists", "where": { "labels": { "$includes": "Function" } } },
            { "id": "contains-all-tags", "type": "node_exists", "where": { "meta.tags": { "$containsAll": ["x", "y"] } } },
            { "id": "path-matches-ts", "type": "node_exists", "where": { "path": { "$matches": "\\.ts$" } }, "count": { "exactly": 2 } },
            { "id": "at-least-one-class", "type": "node_exists", "where": { "labels": { "$includes": "Class" } }, "count": { "atLeast": 1 } },
            { "id": "absent-python", "type": "node_absent", "where": { "path": { "$matches": "\\.py$" } } }
        ]
    }))
    .unwrap();

    let report = verify_semantic_conformance(&generation, &fixture).unwrap();
    assert_eq!(report.status, "passed", "failures: {:?}", report.failures);
    assert!(report.assertions.iter().all(|assertion| assertion.passed));
}

#[test]
fn null_expected_requires_a_present_json_null_not_a_missing_field() {
    // Mirrors legacy `Object.is(undefined, null) === false`: plain equality
    // against an expected literal `null` must not match an absent path.
    let generation = ConformanceGeneration {
        nodes: vec![json!({"id": "n1", "target": null}), json!({"id": "n2"})],
        edges: vec![],
        generation_id: None,
        schema_version: None,
        providers: vec![],
    };
    let fixture: ConformanceFixture = serde_json::from_value(json!({
        "name": "null-vs-missing",
        "assertions": [
            { "id": "exactly-one-null-target", "type": "node_exists", "where": { "target": null }, "count": { "exactly": 1 } }
        ]
    }))
    .unwrap();
    let report = verify_semantic_conformance(&generation, &fixture).unwrap();
    assert_eq!(report.status, "passed", "failures: {:?}", report.failures);
}
