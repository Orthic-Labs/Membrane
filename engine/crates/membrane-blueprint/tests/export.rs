use membrane_blueprint::{build_evidence_pack, execute_export, findings_to_sarif, BlueprintRequest, Bounds, CancellationToken, Operation};
use membrane_blueprint::store::Generation;
use serde_json::json;

fn request(input: serde_json::Value) -> BlueprintRequest { let mut request = BlueprintRequest::new("export", Operation::Export, "."); request.deadline_ms = 30_000; request.input = input; request }

#[test]
fn export_is_lossless_and_deterministic() {
    let generation = Generation { manifest: Some(json!({"generationId":"g"})), nodes: vec![json!({"id":"n"})], ..Generation::default() };
    let request = request(json!({"repoRoot":"."}));
    let context = request.validate(Bounds::one_shot()).unwrap();
    let a = execute_export(&generation, &request, &context).unwrap();
    let b = execute_export(&generation, &request, &context).unwrap();
    assert_eq!(a, b);
    assert_eq!(a["nodes"][0]["id"], "n");
}

#[test]
fn evidence_pack_and_sarif_bind_generation() {
    let finding = json!({"ruleId":"r1","fingerprint":"f","message":"m","severity":"error","path":"a.ts","generationId":"g","startLine":2,"endLine":3});
    let pack = build_evidence_pack(Some("repo"), "g", &[finding.clone()]).unwrap();
    assert_eq!(pack["generationId"], "g");
    assert!(pack["markdown"].as_str().unwrap().contains("a.ts:2-3"));
    assert_eq!(findings_to_sarif(&[finding], "v")["version"], "2.1.0");
}

#[test]
fn export_mermaid_is_bounded() {
    let generation = Generation { manifest: Some(json!({"generationId":"g"})), nodes: (0..4).map(|i| json!({"id":format!("n{i}"),"kind":"file","path":format!("{i}.ts")})).collect(), ..Generation::default() };
    let request = request(json!({"repoRoot":".","format":"mermaid","limit":2}));
    let context = request.validate(Bounds::one_shot()).unwrap();
    let value = execute_export(&generation, &request, &context).unwrap();
    assert!(value["text"].as_str().unwrap().contains("flowchart LR"));
    assert_eq!(value["omissions"][0]["reason"], "node_cap");
    assert!(!context.cancellation.is_cancelled());
    let _ = CancellationToken::new();
}
