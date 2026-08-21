//! The shared client fixture labels raw-path and pre-slug gateway inputs. Canonicalization is owned
//! here at the Rust memory boundary; adapters must consume the same post-gateway envelope.

use cortex_core::MemoryTier;
use membrane_runtime::scope::canonical_scope_chain;
use membrane_runtime::MemoryStore;
use std::collections::BTreeMap;

#[test]
fn planner_conformance_scope_forms_share_one_canonical_chain() {
    let fixture_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../federation/fixtures/planner-conformance-v1.json");
    let fixture: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(fixture_path).expect("read shared conformance fixture"),
    )
    .expect("parse shared conformance fixture");
    let forms = fixture["dimensions"]["scopeForms"]
        .as_array()
        .expect("scopeForms array");
    let existing = vec!["D--Claude".to_string(), "global".to_string()];
    for form in forms {
        let input = form["input"].as_str().expect("scope input");
        let canonical = form["canonicalScope"].as_str().expect("canonical scope");
        assert_eq!(
            canonical_scope_chain(input, &existing),
            vec![canonical.to_string(), "global".to_string()],
            "scope form {}",
            form["id"]
        );
    }
}

#[test]
fn frozen_scope_isolation_matrix_preserves_visibility_and_order() {
    let fixture_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../federation/fixtures/scope-isolation-v1.json");
    let fixture: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(fixture_path).expect("read scope-isolation fixture"),
    )
    .expect("parse scope-isolation fixture");
    assert_eq!(fixture["schemaVersion"], 1);

    let query = fixture["query"].as_str().expect("query");
    let k = fixture["k"].as_u64().expect("k") as usize;
    let decoys = fixture["globalDecoys"].as_object().expect("globalDecoys");
    let decoy_count = decoys["count"].as_u64().expect("decoy count") as usize;
    assert!(
        decoy_count > k,
        "fixture must contain more than k global decoys"
    );

    let store = MemoryStore::new();
    for row in fixture["rows"].as_array().expect("rows") {
        store
            .try_put(
                row["name"].as_str().expect("row name"),
                row["content"].as_str().expect("row content"),
                row["scope"].as_str().expect("row scope"),
                MemoryTier::Semantic,
            )
            .expect("seed fixture row");
    }
    for index in 0..decoy_count {
        store
            .try_put(
                &format!(
                    "{}{}",
                    decoys["namePrefix"].as_str().expect("decoy name prefix"),
                    index
                ),
                &format!(
                    "{} {index}",
                    decoys["contentPrefix"]
                        .as_str()
                        .expect("decoy content prefix")
                ),
                "global",
                MemoryTier::Semantic,
            )
            .expect("seed global decoy");
    }

    let existing = store.scopes();
    let states = fixture["provenanceStates"]
        .as_array()
        .expect("provenanceStates");
    assert_eq!(states.len(), 2);
    assert_eq!(states[0]["freshnessClass"], "clean");
    assert_eq!(states[0]["stale"], false);
    assert_eq!(states[1]["freshnessClass"], "dirty_overlay");
    assert_eq!(states[1]["stale"], false);
    assert!(states[1]["baseCommit"].as_str().is_some());
    assert!(states[1]["overlayDigest"].as_str().is_some());

    let mut case_count = 0usize;
    let mut cross_scope_admissions = 0usize;
    let mut orders = BTreeMap::<(String, String, String), Vec<String>>::new();
    for scope_case in fixture["scopeCases"].as_array().expect("scopeCases") {
        let scope_id = scope_case["id"].as_str().expect("scope id");
        let canonical = scope_case["canonicalScope"]
            .as_str()
            .expect("canonicalScope");
        let allowed_scopes = scope_case["allowedScopes"]
            .as_array()
            .expect("allowedScopes")
            .iter()
            .map(|value| value.as_str().expect("allowed scope"))
            .collect::<Vec<_>>();
        let required_ids = scope_case["requiredIds"]
            .as_array()
            .expect("requiredIds")
            .iter()
            .map(|value| value.as_str().expect("required id"))
            .collect::<Vec<_>>();
        let forbidden_ids = scope_case["forbiddenIds"]
            .as_array()
            .expect("forbiddenIds")
            .iter()
            .map(|value| value.as_str().expect("forbidden id"))
            .collect::<Vec<_>>();

        for state in states {
            let state_id = state["id"].as_str().expect("state id");
            for form in scope_case["forms"].as_array().expect("forms") {
                case_count += 1;
                let form_id = form["id"].as_str().expect("form id");
                let chain =
                    canonical_scope_chain(form["input"].as_str().expect("scope input"), &existing);
                assert_eq!(chain.first().map(String::as_str), Some(canonical));

                let ids = store
                    .recall_scored(query, k, &chain)
                    .into_iter()
                    .map(|(entry, _score)| {
                        assert!(
                            allowed_scopes.contains(&entry.scope_id.as_str()),
                            "{} admitted disallowed scope {}",
                            scope_case["id"],
                            entry.scope_id
                        );
                        entry.id
                    })
                    .collect::<Vec<_>>();
                for required in &required_ids {
                    assert!(
                        ids.iter().any(|id| id == required),
                        "missing {required}: {ids:?}"
                    );
                }
                cross_scope_admissions += forbidden_ids
                    .iter()
                    .filter(|forbidden| ids.iter().any(|id| id == **forbidden))
                    .count();
                orders.insert(
                    (
                        scope_id.to_string(),
                        state_id.to_string(),
                        form_id.to_string(),
                    ),
                    ids,
                );
            }
        }
    }

    assert_eq!(
        case_count,
        fixture["expected"]["caseCount"].as_u64().unwrap() as usize
    );
    assert_eq!(
        cross_scope_admissions,
        fixture["expected"]["crossScopeAdmissions"]
            .as_u64()
            .unwrap() as usize
    );
    for scope_case in fixture["scopeCases"].as_array().unwrap() {
        let forms = scope_case["forms"].as_array().unwrap();
        if forms.len() != 2 {
            continue;
        }
        let scope_id = scope_case["id"].as_str().unwrap();
        for state in states {
            let state_id = state["id"].as_str().unwrap();
            assert_eq!(
                orders[&(scope_id.into(), state_id.into(), "path".into())],
                orders[&(scope_id.into(), state_id.into(), "slug".into())],
                "path/slug order drift for {scope_id}/{state_id}"
            );
        }
    }
}
