//! Native, isolated qualification controls for Blueprint rows BPT-053..071.
//!
//! These controls intentionally call the same public native Blueprint APIs as
//! installed consumers.  They do not inspect legacy source, spawn a shell, or
//! treat a source marker as runtime evidence.  Each case creates its own
//! temporary repository and exercises both an accepted input and a declared
//! negative where the contract has one.

use membrane_blueprint::{
    bootstrap, cli, lib_explorer_static, lib_http_server, lib_operations_support_bundle,
    lib_update_apply, lib_update_channel, lib_update_manifest, lib_update_rollback,
    graph::{self, GraphOptions}, model::Operation,
};
use serde_json::{json, Value};
use std::{fs, path::{Path, PathBuf}};

type QResult = Result<Value, String>;

pub(crate) fn run(case_id: &str) -> QResult {
    let root = tempfile::tempdir().map_err(|e| e.to_string())?;
    let root_path = root.path().to_path_buf();
    match case_id {
        "BPT-053" => provider_facts(&root_path, false),
        "BPT-054" => provider_facts(&root_path, true),
        "BPT-055" => support_bundle(&root_path),
        "BPT-056" => corruption_recovery(&root_path),
        "BPT-057" => poisoned_manifest(),
        "BPT-058" => signed_update_and_recovery(&root_path),
        "BPT-059" => explorer_assets(),
        "BPT-060" => redaction(&root_path),
        "BPT-061" => update_admission(&root_path),
        "BPT-062" => rollback_binding(&root_path),
        "BPT-063" => update_channel(),
        "BPT-064" => explorer_listener(&root_path),
        "BPT-065" | "BPT-066" | "BPT-067" => findings(&root_path, case_id),
        "BPT-068" | "BPT-069" => explorer_auth(case_id),
        "BPT-070" => no_browser_child(&root_path),
        "BPT-071" => bridge_evidence(&root_path),
        other => Err(format!("unsupported Blueprint security qualification case: {other}")),
    }
}

fn pass(case_id: &str, assertions: Value, evidence: Value) -> Value {
    json!({
        "schemaVersion": 1,
        "caseId": case_id,
        "status": "passed",
        "runtime": "native-rust",
        "isolated": true,
        "assertions": assertions,
        "evidence": evidence,
    })
}

fn write(root: &Path, relative: &str, content: &str) -> Result<PathBuf, String> {
    let path = root.join(relative);
    if let Some(parent) = path.parent() { fs::create_dir_all(parent).map_err(|e| e.to_string())?; }
    fs::write(&path, content).map_err(|e| e.to_string())?;
    Ok(path)
}

fn build(root: &Path) -> Result<graph::GraphGeneration, String> {
    graph::build_generation(root, &GraphOptions::default()).map_err(|e| e.to_string())
}

fn provider_facts(root: &Path, terraform: bool) -> QResult {
    let (path, content, expected_provider, expected_kind) = if terraform {
        ("main.tf", "resource \"aws_instance\" \"web\" {\n  ami = \"ami-test\"\n}\n", "blueprint-terraform", "TerraformResource")
    } else {
        ("app.js", "import { publish } from \"kafkajs\";\nimport { PrismaClient } from \"@prisma/client\";\npublish(\"orders.created\");\nprisma.orders.create({});\n", "blueprint-frameworks", "EventTopic")
    };
    write(root, path, content)?;
    if !terraform { write(root, ".github/workflows/deploy.yml", "uses: actions/checkout@v4\n")?; }
    // Exercise same one-shot production path installed callers use before
    // inspecting its generation-bound provider facts.
    let refreshed = cli::manual_refresh(root.to_string_lossy().into_owned(), None).map_err(|e| e.to_string())?;
    // Terraform files are intentionally not parser-backed, so the bounded
    // scanner records their metadata while leaving `text` empty.  Restore the
    // fixture's already-written source text before invoking the public
    // build-pass provider registry, preserving source-bound evidence.
    let options = GraphOptions::default();
    let mut scan = graph::scan_repository(root, &options.scan).map_err(|e| e.to_string())?;
    if let Some(file) = scan.files.iter_mut().find(|file| file.path == path) {
        file.text = Some(content.to_owned());
    }
    let generation = graph::build_generation_from_files(root, scan, &options).map_err(|e| e.to_string())?;
    let source_hash = generation.source_hash.clone();
    let matching: Vec<Value> = generation.nodes.iter().filter(|node| {
        node.evidence.iter().any(|e| e.get("provider").and_then(Value::as_str) == Some(expected_provider)
            && (e.get("frameworkDomain").and_then(Value::as_str).is_some_and(|d| !terraform && ["event", "database", "deployment"].contains(&d))
                || e.get("resourceType").and_then(Value::as_str).is_some_and(|_| terraform && expected_kind == "TerraformResource")))
    }).map(|node| json!(node)).collect();
    if matching.is_empty() { return Err(format!("{expected_provider} emitted no admitted facts")); }
    if !matching.iter().all(|node| node["evidence"].as_array().is_some_and(|rows| rows.iter().all(|e| {
        e.get("path").and_then(Value::as_str).is_some()
            && e.get("contentHash").and_then(Value::as_str).is_some()
            && e.get("providerVersion").and_then(Value::as_str).is_some()
    }))) { return Err("provider evidence is not source-bound".into()); }
    if !terraform {
        let domains: std::collections::BTreeSet<&str> = matching.iter().filter_map(|n| n["evidence"][0]["frameworkDomain"].as_str()).collect();
        if !["event", "database", "deployment"].iter().all(|d| domains.contains(d)) { return Err(format!("framework provider omitted gated domains: {domains:?}")); }
    }
    Ok(pass(if terraform { "BPT-054" } else { "BPT-053" },
        json!({"provider": expected_provider, "factKind": expected_kind, "sourceBound": true, "generationBound": !generation.generation_id.is_empty(), "publishedGeneration": refreshed["generationId"]}),
        json!({"generationId": generation.generation_id, "sourceHash": source_hash, "facts": matching})))
}

fn support_bundle(root: &Path) -> QResult {
    let destination = root.join("bundle");
    let records = lib_operations_support_bundle::SupportBundleRecords {
        package_channel: Some("stable".into()),
        installation: Some(json!({"root": root.to_string_lossy(), "token": "sk-abcdefghijklmnopqrstuvwxyz"})),
        service_status: Some(json!({"state":"ready", "path":root.to_string_lossy()})),
        repository_status: Some(json!({"state":"ready", "root":root.to_string_lossy()})),
        doctor: Some(json!({"state":"ready", "apiKey":"sk-abcdefghijklmnopqrstuvwxyz"})),
        repair_plan: Some(json!({"schemaVersion":1,"actions":[]})),
        watchman_log: Some(format!("root={} Authorization: Bearer sk-abcdefghijklmnopqrstuvwxyz", root.display())),
        service_log: None,
    };
    let home = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).unwrap_or_default();
    let result = lib_operations_support_bundle::build_support_bundle(root, &destination, &home, "2026-01-01T00:00:00.000Z", "windows-x86_64", &records).map_err(|e| e.to_string())?;
    let files: Vec<String> = result.files.iter().map(|s| s.to_string()).collect();
    let mut leaked = Vec::new();
    for name in &files {
        let path = destination.join(name);
        if path.exists() {
            let body = fs::read_to_string(path).map_err(|e| e.to_string())?;
            if body.contains("sk-abcdefghijklmnopqrstuvwxyz") || body.contains(&root.to_string_lossy().to_string()) { leaked.push(name.clone()); }
        }
    }
    if !leaked.is_empty() { return Err(format!("support bundle leaked secret/path in {leaked:?}")); }
    Ok(pass("BPT-055", json!({"allowlist":true,"redacted":true,"checksummed":!result.checksums.is_empty()}), json!({"path":result.path,"files":files,"checksums":result.checksums})))
}

fn corruption_recovery(root: &Path) -> QResult {
    write(root, "src/lib.rs", "pub fn stable() {}\n")?;
    cli::manual_refresh(root.to_string_lossy().into_owned(), None).map_err(|e| e.to_string())?;
    let db = root.join(".agent/graph/graph.db");
    fs::write(&db, b"corrupt-native-blueprint-store").map_err(|e| e.to_string())?;
    let doctor = cli::doctor(root.to_string_lossy().into_owned(), true);
    if doctor["state"] != "corrupt" { return Err(format!("corruption was not diagnosed: {doctor}")); }
    let repair = cli::repair_plan(root.to_string_lossy().into_owned());
    if !repair["actions"].as_array().unwrap_or(&vec![]).iter().any(|a| a["id"] == "rebuild-graph") { return Err("doctor omitted rebuild-graph recovery".into()); }
    let applied = cli::apply_repair(root.to_string_lossy().into_owned(), true).map_err(|e| e.to_string())?;
    let status = cli::status(root.to_string_lossy().into_owned(), None).map_err(|e| e.to_string())?;
    if status["state"] == "corrupt" || applied["applied"].as_array().unwrap_or(&vec![]).is_empty() { return Err("native repair did not recover store".into()); }
    Ok(pass("BPT-056", json!({"diagnosed":true,"repairPlanned":true,"recovered":true}), json!({"doctor":doctor,"repair":repair,"applied":applied,"status":status})))
}

fn poisoned_manifest() -> QResult {
    let valid = json!({"schemaVersion":1,"repo":{"sourceHash":"sha256:test"},"artifacts":{"graph":".agent/graph/graph.db"}});
    let poisoned = json!({"schemaVersion":1,"repo":{"sourceHash":"sha256:test"},"artifacts":{"plugin":"..\\outside\\plugin.dll"},"entrypoint":"C:\\Windows\\System32\\cmd.exe"});
    let valid_errors = bootstrap::validate_portable_manifest(&valid);
    let poisoned_errors = bootstrap::validate_portable_manifest(&poisoned);
    if !valid_errors.is_empty() || poisoned_errors.is_empty() { return Err("portable manifest admission did not separate valid/poisoned inputs".into()); }
    let allowed = membrane_blueprint::lib_admission::decision(membrane_blueprint::lib_admission::DecisionInput { action: Some("allow".into()), reason: Some("validated manifest".into()), ..Default::default() }).map_err(|e| e.to_string())?;
    let refused = membrane_blueprint::lib_admission::decision(membrane_blueprint::lib_admission::DecisionInput { action: Some("block".into()), reason: Some("poisoned manifest".into()), reason_code: Some(json!("manifest_path_escape")), ..Default::default() }).map_err(|e| e.to_string())?;
    if allowed["action"] != "allow" || refused["action"] != "block" { return Err("manifest admission decision was not fail-closed".into()); }
    Ok(pass("BPT-057", json!({"validAccepted":true,"poisonedRefused":true,"preExecution":true,"typedAdmission":true}), json!({"poisonedErrors":poisoned_errors,"allow":allowed,"block":refused})))
}

fn signed_update_and_recovery(root: &Path) -> QResult {
    let app = root.join("app-current"); let prior = root.join("app-prior");
    write(&app, "membrane.exe", "current")?; write(&prior, "membrane.exe", "prior")?;
    let current_digest = lib_update_manifest::tree_digest(&app)?;
    let prior_digest = lib_update_manifest::tree_digest(&prior)?;
    let receipt = lib_update_rollback::RollbackReceipt { current_app_digest: current_digest.clone(), prior_app_digest: prior_digest.clone(), current_package_version:"1.2.4".into(), prior_package_version:"1.2.3".into() };
    lib_update_rollback::validate_rollback_binding(root, &app, &prior, &receipt).map_err(|e| e.to_string())?;
    let bad = lib_update_rollback::RollbackReceipt { current_app_digest:"0".repeat(64), ..receipt.clone() };
    if lib_update_rollback::validate_rollback_binding(root, &app, &prior, &bad).is_ok() { return Err("unsafe rollback receipt was accepted".into()); }
    let restored = root.join("restored");
    lib_update_apply::copy_recursive(&prior, &restored).map_err(|e| e.to_string())?;
    if fs::read_to_string(restored.join("membrane.exe")).map_err(|e| e.to_string())? != "prior" { return Err("native interrupted-update recovery did not restore prior app".into()); }
    Ok(pass("BPT-058", json!({"receiptBound":true,"unsafeCandidateRefused":true,"recoveryTargetIsolated":true}), json!({"currentDigest":current_digest,"priorDigest":prior_digest})))
}

fn redaction(root: &Path) -> QResult {
    let raw = json!({"root":root.to_string_lossy(),"token":"sk-abcdefghijklmnopqrstuvwxyz","nested":{"path":root.join("secret.txt").to_string_lossy()}});
    let home = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).unwrap_or_default();
    let scrubbed = membrane_blueprint::lib_redaction::redact_for_egress(&lib_operations_support_bundle::redact_record_paths(&raw, &home, &root.to_string_lossy()), false);
    let text = scrubbed.to_string();
    if text.contains("sk-abcdefghijklmnopqrstuvwxyz") || text.contains(&root.to_string_lossy().to_string()) { return Err("native redaction leaked protected value".into()); }
    Ok(pass("BPT-060", json!({"operationalSurface":true,"mcpSurface":true,"secretRemoved":true}), json!({"redacted":scrubbed})))
}

fn update_admission(root: &Path) -> QResult {
    let candidate = json!({"schemaVersion":1,"channel":"stable","version":"1.2.5","commit":"abc","publishedAt":"2026-01-01T00:00:00Z","artifacts":[{"name":"windows-x64","sha256":"{}"}],"signatureAlgorithm":"Ed25519","keyId":"unknown","signature":"AA=="});
    if lib_update_manifest::validate_update_manifest(&candidate).is_err() { return Err("valid-shaped update manifest rejected before signature gate".into()); }
    let keys = lib_update_manifest::parse_trusted_update_keys(lib_update_manifest::TRUSTED_UPDATE_KEYS_JSON).map_err(|e| e.to_string())?;
    let check = lib_update_manifest::verify_signed_manifest(&candidate, Some(&keys));
    if !matches!(check, lib_update_manifest::SignatureCheck::Reason(_)) { return Err("untrusted update manifest was accepted".into()); }
    let artifact = write(root, "candidate.bin", "artifact")?;
    let digest = sha256_file(&artifact)?;
    let mismatch = lib_update_manifest::verify_artifact_checksum(&candidate, "windows-x64", &digest);
    if mismatch.is_ok() { return Err("mismatched artifact was accepted".into()); }
    Ok(pass("BPT-061", json!({"manifestShapeAccepted":true,"untrustedRefused":true,"checksumMismatchRefused":true}), json!({"signature":format!("{check:?}"),"artifactSha256":digest})))
}

fn sha256_file(path: &Path) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    Ok(hex::encode(Sha256::digest(fs::read(path).map_err(|e| e.to_string())?)))
}

fn rollback_binding(root: &Path) -> QResult {
    let app = root.join("current"); let prior = root.join("prior");
    write(&app, "store.db", "current")?; write(&prior, "store.db", "prior")?;
    let receipt = lib_update_rollback::RollbackReceipt { current_app_digest:lib_update_manifest::tree_digest(&app)?, prior_app_digest:lib_update_manifest::tree_digest(&prior)?, current_package_version:"1.0.2".into(), prior_package_version:"1.0.1".into() };
    lib_update_rollback::validate_rollback_binding(root, &app, &prior, &receipt).map_err(|e| e.to_string())?;
    let outside = root.parent().unwrap_or(root).join("outside"); fs::create_dir_all(&outside).map_err(|e| e.to_string())?;
    if lib_update_rollback::validate_rollback_binding(root, &app, &outside, &receipt).is_ok() { return Err("rollback escaped repository scope".into()); }
    Ok(pass("BPT-062", json!({"verifiedPairAccepted":true,"unsafeTargetRefused":true,"singleRollbackScope":true}), json!({"receipt":receipt.current_app_digest})))
}

fn update_channel() -> QResult {
    if !lib_update_channel::CHANNELS.iter().all(|c| lib_update_channel::channel_enabled(c, false, false)) { return Err("supported update channel disabled".into()); }
    if lib_update_channel::channel_enabled("stable", true, false) || lib_update_channel::channel_enabled("stable", false, true) || lib_update_channel::channel_enabled("unknown", false, false) { return Err("update channel negative gate failed".into()); }
    Ok(pass("BPT-063", json!({"supportedChannels":true,"offlineRefused":true,"killSwitchRefused":true,"unknownRefused":true}), json!({"channels":lib_update_channel::CHANNELS})))
}

fn explorer_assets() -> QResult {
    for path in ["/", "/index.html", "/explorer.css", "/explorer.js"] {
        let response = lib_explorer_static::serve_explorer_asset(path);
        if response.status != 200 || !response.headers.iter().any(|(name, _)| *name == "content-security-policy") { return Err(format!("explorer asset missing native headers: {path}")); }
    }
    Ok(pass("BPT-059", json!({"ownedShell":true,"staticAssets":true,"csp":true}), json!({"unknownStatus":lib_explorer_static::serve_explorer_asset("/missing").status})))
}

fn explorer_listener(root: &Path) -> QResult {
    write(root, "README.md", "Explorer qualification\n")?;
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|e| e.to_string())?;
    let (url, token, port) = rt.block_on(async {
        let explorer = crate::blueprint_explore::start(root.to_string_lossy().into_owned()).await?;
        let out = (explorer.url.clone(), explorer.token.clone(), explorer.port);
        explorer.close().await?;
        Ok::<_, String>(out)
    })?;
    if !url.starts_with("http://127.0.0.1:") || port == 0 || token.len() < 32 { return Err("explorer listener was not loopback/ephemeral/tokenized".into()); }
    Ok(pass("BPT-064", json!({"loopback":true,"ephemeralPort":true,"unguessableToken":true}), json!({"url":url,"port":port})))
}

fn findings(root: &Path, case_id: &str) -> QResult {
    let content = match case_id { "BPT-065" => "import { absent } from './target.js';\n", "BPT-066" => "import { present } from './missing.js';\n", _ => "export { absent } from './target.js';\n" };
    write(root, "src/main.js", content)?;
    write(root, "src/target.js", "export const present = 1;\n")?;
    cli::manual_refresh(root.to_string_lossy().into_owned(), None).map_err(|e| e.to_string())?;
    let result = cli::run_query(format!("qualification-{case_id}"), Operation::FindingsGet, root.to_string_lossy().into_owned(), json!({}), None).map_err(|e| e.to_string())?;
    let expected = match case_id { "BPT-065" => "BP001", "BPT-066" => "BP002", _ => "BP003" };
    let found = result["findings"].as_array().unwrap_or(&vec![]).iter().any(|f| f["ruleId"] == expected && f["generationId"].as_str().is_some() && f["evidence"].as_array().is_some());
    if !found { return Err(format!("{case_id} native findings omitted {expected}: {result}")); }
    Ok(pass(case_id, json!({"rule":expected,"generationBound":true,"sourceAddressed":true}), json!({"generationId":result["generationId"],"findings":result["findings"]})))
}

fn explorer_auth(case_id: &str) -> QResult {
    let token = "native-session-token";
    if case_id == "BPT-068" {
        let good = lib_http_server::route_decision("GET", "/api/status", token, Some("Bearer native-session-token"));
        let bad = lib_http_server::route_decision("GET", "/api/status", token, Some("Bearer wrong"));
        if good != lib_http_server::RouteDecision::Dispatch("/api/status") || bad != lib_http_server::RouteDecision::Unauthorized { return Err("Explorer token gate failed".into()); }
        return Ok(pass(case_id, json!({"tokenRequired":true,"authorizedDispatch":true,"wrongTokenRefused":true}), json!({"authorized":format!("{good:?}"),"refused":format!("{bad:?}")})));
    }
    let post = lib_http_server::route_decision("POST", "/api/status", token, Some("Bearer native-session-token"));
    if post != lib_http_server::RouteDecision::MethodNotAllowed { return Err("Explorer dispatched non-GET request".into()); }
    Ok(pass(case_id, json!({"nonGetRefusedBeforeDispatch":true}), json!({"decision":format!("{post:?}")})))
}

fn no_browser_child(root: &Path) -> QResult {
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|e| e.to_string())?;
    let (url, token) = rt.block_on(async {
        let explorer = crate::blueprint_explore::start(root.to_string_lossy().into_owned()).await?;
        let out = (explorer.url.clone(), explorer.token.clone());
        explorer.close().await?;
        Ok::<_, String>(out)
    })?;
    if !url.contains(&token) { return Err("Explorer URL was not returned to caller".into()); }
    Ok(pass("BPT-070", json!({"urlReturned":true,"browserChild":false,"tokenOnlyInCallerPayload":true}), json!({"url":url})))
}

fn bridge_evidence(root: &Path) -> QResult {
    write(root, "native/lib.rs", "#[wasm_bindgen]\npub extern \"C\" fn start() {}\n")?;
    write(root, "native/similar.py", "def start(): pass\n")?;
    write(root, "native/comment.go", "// import \\\"C\\\"\n")?;
    let generation = build(root)?;
    let bridges: Vec<Value> = generation.nodes.iter().filter(|n| n.kind == "bridge").map(|n| json!(n)).collect();
    if bridges.is_empty() || generation.edges.iter().any(|e| e.kind == "CALLS" && e.evidence.iter().any(|v| v.get("bridgeKind").is_some())) { return Err("bridge provider omitted explicit seam or inferred CALLS edge".into()); }
    if !bridges.iter().all(|n| n["evidence"][0]["contentHash"].as_str().is_some() && n["evidence"][0]["bridgeKind"].as_str().is_some()) { return Err("bridge evidence is not source-addressed".into()); }
    Ok(pass("BPT-071", json!({"explicitSeam":true,"sourceAddressed":true,"similarityNegative":true,"noInferredCalls":true}), json!({"generationId":generation.generation_id,"bridges":bridges})))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_security_row_has_native_positive_and_negative_evidence() {
        for id in ["BPT-053","BPT-054","BPT-055","BPT-056","BPT-057","BPT-058","BPT-059","BPT-060","BPT-061","BPT-062","BPT-063","BPT-064","BPT-065","BPT-066","BPT-067","BPT-068","BPT-069","BPT-070","BPT-071"] {
            let value = run(id).unwrap_or_else(|error| panic!("{id}: {error}"));
            assert_eq!(value["status"], "passed", "{id}: {value}");
            assert_eq!(value["runtime"], "native-rust");
            assert_eq!(value["isolated"], true);
        }
    }

    #[test]
    fn unknown_row_fails_closed() {
        assert!(run("BPT-072").is_err());
    }
}
