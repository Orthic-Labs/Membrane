//! Parity tests for `src/liveness.rs` against the legacy
//! `blueprint/src/graph/liveness.mjs` (and its `entry-points.mjs`
//! dependency, already ported at `src/entry_points.rs`), porting every case
//! from `blueprint/tests/liveness.test.mjs`.

use membrane_blueprint::entry_points::build_entry_point_registry;
use membrane_blueprint::liveness::{build_liveness_projection, LivenessOptions, LIVENESS_STATES};
use membrane_blueprint::store::Generation;
use serde_json::{json, Value};

fn ev(path: &str) -> Value {
    json!([{ "path": path, "startLine": 1, "endLine": 1, "contentHash": format!("{path}-hash") }])
}

fn fixture_generation() -> Generation {
    Generation {
        manifest: Some(json!({ "generationId": "g-live", "complete": true })),
        nodes: vec![
            json!({ "id": "entry", "kind": "symbol", "path": "src/main.ts", "labels": ["Function", "EntryPoint"], "evidence": ev("src/main.ts") }),
            json!({ "id": "live", "kind": "symbol", "path": "src/live.ts", "labels": ["Function"], "evidence": ev("src/live.ts") }),
            json!({ "id": "orphan", "kind": "symbol", "path": "src/orphan.ts", "labels": ["Function"], "evidence": ev("src/orphan.ts") }),
            json!({ "id": "candidate", "kind": "symbol", "path": "src/candidate.ts", "labels": ["Function"], "evidence": ev("src/candidate.ts") }),
            json!({ "id": "leaf", "kind": "symbol", "path": "src/leaf.ts", "labels": ["Function"], "evidence": ev("src/leaf.ts") }),
        ],
        edges: vec![
            json!({ "id": "e1", "source": "entry", "target": "live", "resolved": true, "confidenceTier": "EXACT_RESOLUTION", "evidence": ev("src/main.ts") }),
            json!({ "id": "e2", "source": "candidate", "target": "leaf", "resolved": true, "confidenceTier": "EXACT_RESOLUTION", "evidence": ev("src/candidate.ts") }),
        ],
        ..Default::default()
    }
}

fn find_result<'a>(results: &'a Value, node_id: &str) -> &'a Value {
    results
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["nodeId"] == node_id)
        .unwrap_or_else(|| panic!("missing result for {node_id}"))
}

#[test]
fn entry_registry_separates_explicit_evidence_from_zero_inbound_structural_candidates() {
    let generation = fixture_generation();
    let rows = build_entry_point_registry(&generation, true);
    let entry = rows.iter().find(|r| r["id"] == "entry").unwrap();
    assert_eq!(entry["authority"], "explicit");
    let candidate = rows.iter().find(|r| r["id"] == "candidate").unwrap();
    assert_eq!(candidate["authority"], "structural_candidate");
}

#[test]
fn liveness_emits_only_live_unreached_unknown_and_never_calls_zero_inbound_dead() {
    let generation = fixture_generation();
    let result = build_liveness_projection(&generation, &LivenessOptions::default());
    assert_eq!(LIVENESS_STATES, ["LIVE", "UNREACHED", "UNKNOWN"]);
    assert_eq!(find_result(&result["results"], "entry")["state"], "LIVE");
    assert_eq!(find_result(&result["results"], "live")["state"], "LIVE");
    assert_eq!(find_result(&result["results"], "candidate")["state"], "UNREACHED");
    assert_eq!(find_result(&result["results"], "orphan")["state"], "UNREACHED");
    for row in result["results"].as_array().unwrap() {
        let state = row["state"].as_str().unwrap();
        assert!(state != "DEAD" && state != "UNUSED");
    }
    assert_eq!(find_result(&result["results"], "live")["reachabilityPath"], json!(["entry", "live"]));
}

#[test]
fn liveness_fails_to_unknown_when_source_or_entrypoint_basis_is_not_trustworthy() {
    let generation = fixture_generation();

    let stale = build_liveness_projection(&generation, &LivenessOptions { source_state: Some("stale".to_string()), ..Default::default() });
    for row in stale["results"].as_array().unwrap() {
        assert_eq!(row["state"], "UNKNOWN");
    }

    let mut no_explicit = generation.clone();
    no_explicit.nodes = no_explicit
        .nodes
        .into_iter()
        .map(|mut node| {
            if let Some(labels) = node.get_mut("labels").and_then(Value::as_array_mut) {
                labels.retain(|label| label.as_str() != Some("EntryPoint"));
            }
            node
        })
        .collect();
    let no_explicit_result = build_liveness_projection(&no_explicit, &LivenessOptions::default());
    for row in no_explicit_result["results"].as_array().unwrap() {
        assert_eq!(row["state"], "UNKNOWN");
    }

    let mut incomplete = generation.clone();
    incomplete.manifest = Some(json!({ "generationId": "g-live", "complete": false }));
    let incomplete_result = build_liveness_projection(&incomplete, &LivenessOptions::default());
    for row in incomplete_result["results"].as_array().unwrap() {
        assert_eq!(row["state"], "UNKNOWN");
    }
}
