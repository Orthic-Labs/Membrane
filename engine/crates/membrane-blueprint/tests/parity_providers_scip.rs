//! Parity port of `blueprint/tests/scip-normalizer.test.mjs` against the
//! native `providers::scip` module (ported from
//! `blueprint/src/providers/compilers/scip-normalize.mjs` and
//! `blueprint/src/providers/compilers/python-scip.mjs`).
//!
//! Fixture is copied verbatim from
//! `blueprint/tests/fixtures/compiler-adapters/python/index.scip.json`.

use std::path::PathBuf;

use membrane_blueprint::providers::scip::{
    collect, normalize_scip_index, normalize_scip_roles, read_normalized_scip_index,
    scip_python_manifest, ScipContext,
};
use serde_json::json;

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/providers/scip-python")
}

fn fixture_index_path() -> PathBuf {
    fixture_dir().join("index.scip.json")
}

#[test]
fn scip_role_normalization_gives_string_and_bitmask_forms_the_same_contract() {
    let from_names = normalize_scip_roles(&json!(["definition", "read"]));
    assert_eq!(from_names.into_iter().collect::<Vec<_>>(), vec!["definition", "read"]);

    let from_bits = normalize_scip_roles(&json!(1 | 4));
    assert_eq!(from_bits.into_iter().collect::<Vec<_>>(), vec!["definition", "read"]);

    let from_bits2 = normalize_scip_roles(&json!(2 | 8));
    assert_eq!(from_bits2.into_iter().collect::<Vec<_>>(), vec!["reference", "write"]);
}

#[test]
fn canonical_normalizer_builds_exact_cross_document_definition_identity() {
    let index = read_normalized_scip_index(&fixture_index_path()).expect("fixture index reads");

    let total_symbol = index
        .definitions_by_symbol
        .keys()
        .find(|symbol| symbol.ends_with("Item#total()."))
        .cloned()
        .expect("fixture exposes the exact total() SCIP symbol");

    let definition_idx = *index.definitions_by_symbol.get(&total_symbol).unwrap();
    let definition = &index.occurrences[definition_idx];
    assert_eq!(definition.document_path, "pkg/models.py");

    let cross_document_reference = index
        .occurrences
        .iter()
        .find(|occurrence| {
            occurrence.document_path == "pkg/service.py"
                && occurrence.symbol == total_symbol
                && occurrence.roles.contains("reference")
        })
        .expect("cross-document reference exists");
    assert_eq!(
        index.definitions_by_symbol.get(&cross_document_reference.symbol),
        index.definitions_by_symbol.get(&total_symbol)
    );
}

#[test]
fn normalizer_preserves_symbol_information_relationships_external_symbols_and_position_metadata() {
    let parsed = json!({
        "metadata": { "version": "fixture", "textDocumentEncoding": "UTF8" },
        "documents": [{
            "relativePath": "src/a.ts",
            "occurrences": [{ "symbol": "scip-typescript npm pkg 1.0 src/a Foo#", "roles": 1, "range": [0, 0, 0, 3] }],
            "symbols": [{
                "symbol": "scip-typescript npm pkg 1.0 src/a Foo#",
                "displayName": "Foo",
                "documentation": ["A documented type."],
                "relationships": [{ "symbol": "scip-typescript npm pkg 1.0 src/base Base#", "isImplementation": true }],
            }],
        }],
        "externalSymbols": [{
            "symbol": "scip-typescript npm dep 2.0 index External#",
            "displayName": "External",
            "relationships": [{ "symbol": "scip-typescript npm dep 2.0 index Parent#", "isTypeDefinition": true }],
        }],
    });

    let index = normalize_scip_index(&parsed, None).expect("inline index normalizes");

    assert_eq!(index.metadata.get("textDocumentEncoding").and_then(|v| v.as_str()), Some("UTF8"));
    let foo = index
        .symbol_information_by_symbol
        .get("scip-typescript npm pkg 1.0 src/a Foo#")
        .expect("Foo symbol information present");
    assert_eq!(foo.display_name.as_deref(), Some("Foo"));
    assert_eq!(foo.documentation, vec!["A documented type.".to_string()]);
    assert!(foo.relationships[0].is_implementation);

    assert_eq!(index.external_symbols.len(), 1);
    assert!(index.external_symbols[0].relationships[0].is_type_definition);
}

#[test]
fn python_scip_provider_collects_exact_normalized_symbol_targets() {
    let context = ScipContext { repo_root: fixture_dir(), scip_index_path: None };
    let result = collect(&context);

    assert_eq!(result.index.get("state").and_then(|v| v.as_str()), Some("ok"));

    let total_definition = result
        .nodes
        .iter()
        .find(|node| node.get("symbol").and_then(|v| v.as_str()).map(|s| s.ends_with("Item#total().")).unwrap_or(false))
        .expect("total() definition node present");
    let total_symbol = total_definition.get("symbol").and_then(|v| v.as_str()).unwrap().to_string();
    let total_id = total_definition.get("id").and_then(|v| v.as_str()).unwrap().to_string();

    let generic_reference = result
        .edges
        .iter()
        .find(|edge| {
            edge.get("kind").and_then(|v| v.as_str()) == Some("REFERENCES")
                && edge
                    .get("evidence")
                    .and_then(|v| v.as_array())
                    .and_then(|a| a.first())
                    .and_then(|e| e.get("path"))
                    .and_then(|v| v.as_str())
                    == Some("pkg/service.py")
                && edge
                    .get("evidence")
                    .and_then(|v| v.as_array())
                    .and_then(|a| a.first())
                    .and_then(|e| e.get("symbol"))
                    .and_then(|v| v.as_str())
                    == Some(total_symbol.as_str())
        })
        .expect("cross-document REFERENCES edge present");
    assert_eq!(generic_reference.get("target").and_then(|v| v.as_str()), Some(total_id.as_str()));
    assert_eq!(generic_reference.get("confidenceTier").and_then(|v| v.as_str()), Some("EXACT_RESOLUTION"));
}

#[test]
fn embedded_manifest_matches_legacy_provider_identity() {
    let manifest = scip_python_manifest();
    assert_eq!(manifest.get("id").and_then(|v| v.as_str()), Some("scip-python"));
    assert_eq!(manifest.get("entry").and_then(|v| v.as_str()), Some("src/providers/compilers/python-scip.mjs"));
}
