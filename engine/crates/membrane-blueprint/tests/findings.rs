use membrane_blueprint::api::{BlueprintRequest, Bounds, Operation};
use membrane_blueprint::findings::execute_findings;
use membrane_blueprint::store::Generation;
use serde_json::json;
use std::path::Path;

fn generation() -> Generation {
    serde_json::from_value(json!({
        "manifest": {"generationId":"g1"},
        "nodes": [
          {"kind":"file","path":"src/a.js","evidence":[{"contentHash":"sha256:a","moduleSurface":{"path":"src/a.js","parseStatus":"ok","exports":[],"starReexports":[],"requests":[{"kind":"import","name":"missing","specifier":"./b.js","line":2}],"open":[]}}]},
          {"kind":"file","path":"src/b.js","evidence":[{"contentHash":"sha256:b","moduleSurface":{"path":"src/b.js","parseStatus":"ok","exports":[],"starReexports":[],"requests":[],"open":[]}}]}
        ], "edges": [], "fileReports": []
    })).unwrap()
}

#[test]
fn detects_missing_binding_from_loaded_surface() {
    let req = BlueprintRequest::new("r1", Operation::FindingsGet, ".");
    let ctx = req.validate(Bounds::default()).unwrap();
    let out = execute_findings(&generation(), &req, &ctx, Path::new("target/findings-test-state")).unwrap();
    assert_eq!(out["findings"][0]["ruleId"], "BP001");
    assert_eq!(out["generationId"], "g1");
}

#[test]
fn missing_module_is_bp002() {
    let mut g = generation();
    g.nodes[0]["evidence"][0]["moduleSurface"]["requests"][0]["specifier"] = json!("./missing.js");
    let req = BlueprintRequest::new("r1", Operation::FindingsGet, ".");
    let ctx = req.validate(Bounds::default()).unwrap();
    let out = execute_findings(&g, &req, &ctx, Path::new("target/findings-test-state")).unwrap();
    assert_eq!(out["findings"][0]["ruleId"], "BP002");
}

#[test]
fn explain_requires_exact_fingerprint() {
    let req = BlueprintRequest::new("r1", Operation::FindingsExplain, ".");
    let ctx = req.validate(Bounds::default()).unwrap();
    let err = execute_findings(&generation(), &req, &ctx, Path::new("target/findings-test-state")).unwrap_err();
    assert_eq!(err.code, "required_field_missing");
}

#[test]
fn detects_reexport_binding_as_bp003() {
    let mut g = generation();
    g.nodes[0]["evidence"][0]["moduleSurface"]["requests"][0]["kind"] = json!("reexport");
    let req = BlueprintRequest::new("r1", Operation::FindingsGet, ".");
    let ctx = req.validate(Bounds::default()).unwrap();
    let out = execute_findings(&g, &req, &ctx, Path::new("target/findings-test-state")).unwrap();
    assert_eq!(out["findings"][0]["ruleId"], "BP003");
}

#[test]
fn unsupported_surface_is_omission_not_finding() {
    let mut g = generation();
    g.nodes[1]["evidence"][0]["moduleSurface"]["parseStatus"] = json!("failed");
    let req = BlueprintRequest::new("r1", Operation::FindingsGet, ".");
    let ctx = req.validate(Bounds::default()).unwrap();
    let out = execute_findings(&g, &req, &ctx, Path::new("target/findings-test-state")).unwrap();
    assert!(out["omissions"].as_array().unwrap().iter().any(|x| x["reason"] == "parse_failed"));
}

#[test]
fn explicit_pack_selection_is_required() {
    let req = BlueprintRequest::new("r1", Operation::FindingsEvidencePack, ".");
    let ctx = req.validate(Bounds::default()).unwrap();
    let err = execute_findings(&generation(), &req, &ctx, Path::new("target/findings-test-state")).unwrap_err();
    assert_eq!(err.code, "required_field_missing");
}

#[test]
fn stale_can_be_blocked_explicitly() {
    let mut req = BlueprintRequest::new("r1", Operation::FindingsGet, ".");
    req.input["stale"] = json!(true);
    req.input["allowStale"] = json!(false);
    let ctx = req.validate(Bounds::default()).unwrap();
    let err = execute_findings(&generation(), &req, &ctx, Path::new("target/findings-test-state")).unwrap_err();
    assert_eq!(err.code, "stale_blocked");
}

#[test]
fn coverage_distinguishes_scanned_from_parsed() {
    let mut g = generation();
    g.nodes[1]["evidence"][0]["moduleSurface"]["parseStatus"] = json!("failed");
    let req = BlueprintRequest::new("r1", Operation::FindingsGet, ".");
    let ctx = req.validate(Bounds::default()).unwrap();
    let out = execute_findings(&g, &req, &ctx, Path::new("target/findings-test-state")).unwrap();
    assert_eq!(out["coverage"]["filesScanned"], 2);
    assert_eq!(out["coverage"]["filesParsed"], 1);
}
