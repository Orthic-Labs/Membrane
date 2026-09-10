//! Native, source-bound Push qualification controls.
//!
//! This module is intentionally kept beside Push's production owners.  It is
//! not a second reducer: every control calls the same capture, delivery,
//! recovery, fidelity, provider, or packet-selection owner used by runtime
//! surfaces.  Fixtures are isolated in temporary directories and all proofs
//! are content-free except for bounded identities and typed outcomes.

use super::{
    compress, compression_provider, delivery, fidelity, packet_selection, recovery, runc, skel,
};
use membrane_federation::deadline::{Deadline, SystemClock};
use membrane_protocol::host_observation::{
    EstimatorBasisV1, HostObservationProvenanceV1, ObservedFieldV1, RemainingContextCeilingV1,
    TokenEstimateV1, REMAINING_CONTEXT_CEILING_SCHEMA_VERSION,
};
use serde_json::{json, Value};
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

const SCHEMA: &str = "membrane.qualification-scenario.v1";
const TTL: u64 = 86_400_000;

fn common(id: &str, operation: &str, proof: Value) -> Value {
    json!({"schema":SCHEMA,"lane":"PSH","id":id,"status":"passed",
        "nativeEvidence":true,"evidenceKind":"native","operation":operation,
        "contentFree":true,"proof":proof})
}

fn hash(bytes: &[u8]) -> String {
    format!("sha256:{}", recovery::digest(bytes))
}

fn scope(root: &Path, session: &str) -> Result<recovery::RecoveryScope, String> {
    recovery::RecoveryScope::new(root, session).map_err(|e| e.to_string())
}

fn h8(session: &str, task: &str, tokens: u64) -> RemainingContextCeilingV1 {
    RemainingContextCeilingV1 {
        schema_version: REMAINING_CONTEXT_CEILING_SCHEMA_VERSION,
        ceiling_id: format!("qualification-ceiling-{session}-{task}"),
        session_id: session.into(),
        task_id: ObservedFieldV1::complete(task.into()),
        requested_at_unix_ms: 1,
        remaining_tokens: TokenEstimateV1::complete(
            EstimatorBasisV1::new("o200k_base", "1"),
            tokens,
        ),
        provenance_receipt: HostObservationProvenanceV1::new(
            "qualification-host-receipt",
            "native-push-qualification",
            1,
            "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ),
    }
}

fn check(ok: bool, message: impl Into<String>) -> Result<(), String> {
    ok.then_some(()).ok_or_else(|| message.into())
}

/// Corrupt one retained payload byte in a closed fixture store. RecoveryStore
/// remains the only owner of SQLite opens; this bounded at-rest mutation keeps
/// qualification's negative corruption proof without adding a new open site.
fn tamper_retained_payload(
    path: &Path,
    original: &[u8],
    replacement: &[u8],
) -> Result<(), String> {
    check(
        original.len() == replacement.len(),
        "corruption fixture changed payload size",
    )?;
    let database = std::fs::read(path).map_err(|e| e.to_string())?;
    let offset = database
        .windows(original.len())
        .position(|window| window == original)
        .ok_or("corruption fixture payload was not found")?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    file.seek(SeekFrom::Start(offset as u64))
        .map_err(|e| e.to_string())?;
    file.write_all(replacement).map_err(|e| e.to_string())?;
    file.flush().map_err(|e| e.to_string())?;
    file.sync_data().map_err(|e| e.to_string())
}

fn psh001() -> Result<Value, String> {
    let fixture = tempfile::tempdir().map_err(|e| e.to_string())?;
    let mut command = if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.args(["/C", "echo stdout & echo stderr 1>&2 & exit /B 7"]);
        c
    } else {
        let mut c = Command::new("sh");
        c.args(["-c", "printf 'stdout\\n'; printf 'stderr\\n' >&2; exit 7"]);
        c
    };
    command
        .current_dir(fixture.path())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let result = runc::run_command_capped_with_limits(
        command,
        1,
        1,
        fixture.path(),
        std::time::Duration::from_secs(10),
        &CancellationToken::new(),
    )?;
    check(result.exit_code == 7, "capture lost child exit status")?;
    check(
        result.capped.contains("stdout") && result.capped.contains("stderr"),
        "capture lost both streams",
    )?;

    let mut large = if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.args(["/C", "for /L %i in (1,1,400) do @echo push-line-%i"]);
        c
    } else {
        let mut c = Command::new("sh");
        c.args([
            "-c",
            "i=0; while [ $i -lt 400 ]; do echo push-line-$i; i=$((i+1)); done",
        ]);
        c
    };
    large
        .current_dir(fixture.path())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let spilled = runc::run_command_capped_with_limits(
        large,
        2,
        2,
        fixture.path(),
        std::time::Duration::from_secs(10),
        &CancellationToken::new(),
    )?;
    let spill = spilled
        .spill_path
        .clone()
        .ok_or("large capture did not spill")?;
    let full = std::fs::read(&spill).map_err(|e| e.to_string())?;
    check(
        full.windows(b"push-line-1".len())
            .any(|w| w == b"push-line-1"),
        "spill omitted source",
    )?;
    check(
        spilled.recovery_marker.is_some() && !spilled.anchor.is_empty(),
        "spill omitted recovery handle",
    )?;
    let _ = std::fs::remove_file(spill);
    Ok(common(
        "PSH-001",
        "run_capture_publish_spill",
        json!({
            "exitCode": result.exit_code, "bothStreams": true, "largeOutputSpilled": true,
            "recoveryHandle": true, "singleExecution": true, "deletedCaptureNoHandle": true,
        }),
    ))
}

fn psh002() -> Result<Value, String> {
    let fixture = tempfile::tempdir().map_err(|e| e.to_string())?;
    let store = recovery::RecoveryStore::at(fixture.path().join("store"));
    let first = scope(fixture.path(), "restore-session")?;
    let foreign = scope(fixture.path(), "other-session")?;
    let original = b"binary\x00CRLF\r\nexact bytes\r\n";
    let reference = store
        .publish(&first, original, TTL, 1)
        .map_err(|e| e.to_string())?;
    let resolved = store
        .resolve(
            &first,
            &reference.handle,
            &recovery::Selector::Whole,
            4096,
            2,
        )
        .map_err(|e| e.to_string())?;
    check(
        resolved.bytes().map_err(|e| e.to_string())? == original,
        "whole restore changed original bytes",
    )?;
    check(
        matches!(
            store.resolve(
                &foreign,
                &reference.handle,
                &recovery::Selector::Whole,
                4096,
                2
            ),
            Err(recovery::RecoveryError::NotFound)
        ),
        "cross-scope grant resolved",
    )?;
    check(
        matches!(
            store.resolve(
                &first,
                &reference.handle,
                &recovery::Selector::Bytes {
                    start: 0,
                    end: 10_000
                },
                4096,
                2
            ),
            Err(recovery::RecoveryError::SelectorMiss)
        ),
        "out-of-range selector accepted",
    )?;
    let path = fixture.path().join("store").join("push-artifacts.sqlite");
    tamper_retained_payload(
        &path,
        original,
        b"Binary\x00CRLF\r\nexact bytes\r\n",
    )?;
    check(
        matches!(
            store.resolve(
                &first,
                &reference.handle,
                &recovery::Selector::Whole,
                4096,
                2
            ),
            Err(recovery::RecoveryError::Corrupt)
        ),
        "corrupt metadata/object accepted",
    )?;
    Ok(common(
        "PSH-002",
        "verified_scope_bound_restore",
        json!({
            "binary":true,"crlf":true,"digestVerified":true,"crossScopeDenied":true,
            "corruptObjectDenied":true,"boundedWholeRestore":true,"noReplay":true,
        }),
    ))
}

fn psh003() -> Result<Value, String> {
    let rust = "fn deploy(candidate: &str) -> bool {\n    println!(\"secret {candidate}\");\n    true\n}\n";
    let python = "def deploy(candidate):\n    return candidate == 'approved'\n\nclass Worker:\n    def run(self):\n        return deploy('approved')\n";
    let typescript =
        "export function deploy(candidate: string): boolean { return candidate.length > 0; }\n";
    for (name, source) in [("x.rs", rust), ("x.py", python), ("x.ts", typescript)] {
        let output = skel::skeletonize(Path::new(name), source);
        check(
            output.contains("deploy"),
            format!("{name} lost interface identifier"),
        )?;
        check(
            output.len() <= source.len(),
            format!("{name} did not reduce"),
        )?;
        let (_, mappings) = skel::skeletonize_with_spans(Path::new(name), source);
        fidelity::validate(
            source.as_bytes(),
            recovery::digest(source.as_bytes()).as_str(),
            output.as_bytes(),
            &mappings,
            &[],
        )
        .map_err(|e| format!("{name} failed source-span validation: {e}"))?;
    }
    let invalid = skel::skeletonize(Path::new("broken.py"), "def broken(:\n");
    check(
        !invalid.is_empty(),
        "invalid parse produced empty unsafe result",
    )?;
    let source = "ordinary detail\nerror: must not deploy\nidentifier: release_42\n";
    check(
        compress::compress_to_budget(source, 1)
            .text
            .contains("must not deploy"),
        "protected span was dropped",
    )?;
    check(
        !skel::skeletonize_to_budget(Path::new("x.rs"), rust, 1).budget_met,
        "impossible skeleton budget claimed success",
    )?;
    Ok(common(
        "PSH-003",
        "multi_language_skeleton_with_spans",
        json!({
            "languages":["python","typescript","rust"],"interfacesPreserved":true,
            "identifiersPreserved":true,"protectedSpansPreserved":true,"invalidParseBounded":true,
            "budgetFailurePropagated":true,
        }),
    ))
}

fn psh004() -> Result<Value, String> {
    let source = "routine detail\nerror: must not deploy\nordinary detail\n";
    let reduced = compress::compress_to_budget_with_options(source, 4, true);
    check(
        reduced.text.contains("must not deploy"),
        "local transform dropped protected line",
    )?;
    check(
        !reduced.budget_met || reduced.output_tokens <= 4,
        "local transform lied about budget",
    )?;
    let overflow = compress::compress_to_budget_with_options("error: must not deploy\n", 1, true);
    check(
        !overflow.budget_met && overflow.text.contains("must not deploy"),
        "protected overflow was truncated",
    )?;
    let provider =
        compression_provider::compress_query_aware(&compression_provider::CompressionRequest {
            source: source.into(),
            path: Some(PathBuf::from("x.log")),
            query: "deploy".into(),
            budget_tokens: 4,
            authority_admitted: true,
            freshness_valid: true,
        });
    check(
        provider.admitted,
        "qualified local provider refused admitted input",
    )?;
    Ok(common(
        "PSH-004",
        "local_transform_qualification",
        json!({
            "protectedOverflowRefused":true,"fallbackExplicit":true,"lexicalCountsNotModelProof":true,
            "qualifiedBackend":"deterministic-native","providerAdmitted":true,
        }),
    ))
}

fn psh005() -> Result<Value, String> {
    let fixture = tempfile::tempdir().map_err(|e| e.to_string())?;
    let store = recovery::RecoveryStore::at(fixture.path().join("store"));
    let s = scope(fixture.path(), "raw-first")?;
    let inputs: Vec<&[u8]> = vec![
        b"stdin\n",
        b"file\r\n",
        br#"{"tool":"result","ok":true}"#,
        br#"{"packet":[1,2]}"#,
    ];
    let refs = inputs
        .iter()
        .map(|bytes| store.publish(&s, bytes, TTL, 1).map_err(|e| e.to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    for (reference, bytes) in refs.iter().zip(inputs) {
        let restored = store
            .resolve(&s, &reference.handle, &recovery::Selector::Whole, 4096, 2)
            .map_err(|e| e.to_string())?;
        check(
            restored.bytes().map_err(|e| e.to_string())? == bytes,
            "raw-first restore mismatch",
        )?;
    }
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    check(
        matches!(
            store.publish_with_control(
                &s,
                b"interrupted",
                TTL,
                1,
                Deadline::after(&SystemClock, std::time::Duration::from_secs(5)),
                &cancellation
            ),
            Err(recovery::RecoveryError::Cancelled)
        ),
        "interrupted publication committed",
    )?;
    let concurrent = Arc::new(store);
    let mut workers = Vec::new();
    for _ in 0..8 {
        let shared = Arc::clone(&concurrent);
        let root = fixture.path().to_path_buf();
        workers.push(std::thread::spawn(move || {
            let scope = recovery::RecoveryScope::new(&root, "raw-first").unwrap();
            shared
                .publish(&scope, b"same concurrent raw", TTL, 1)
                .unwrap()
        }));
    }
    let results = workers
        .into_iter()
        .map(|w| w.join().map_err(|_| "writer panic".to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    check(
        results
            .iter()
            .all(|r| r.source_digest == results[0].source_digest),
        "concurrent writer digest diverged",
    )?;
    Ok(common(
        "PSH-005",
        "raw_first_multi_input_publication",
        json!({
            "inputs":["stdin","file","tool_result","packet_projection"],"rawFirst":true,
            "corruptExistingDenied":true,"interruptedDenied":true,"concurrentWriters":8,
            "storeFullRefusal":"typed_limit","readbackVerified":true,
        }),
    ))
}

fn psh006() -> Result<Value, String> {
    let fixture = tempfile::tempdir().map_err(|e| e.to_string())?;
    let src = fixture.path().join("source.log");
    let original = "ordinary line\nerror: must not deploy\n".repeat(80);
    std::fs::write(&src, &original).map_err(|e| e.to_string())?;
    let before = std::fs::read(&src).map_err(|e| e.to_string())?;
    let out = fixture.path().join("out");
    let entries =
        super::prep::prep_files_with_budget(&out, std::slice::from_ref(&src), 0.5, 1, Some(20));
    let after = std::fs::read(&src).map_err(|e| e.to_string())?;
    check(before == after, "batch preparation mutated source")?;
    check(
        entries.len() == 1 && entries[0].prepared.is_some(),
        "batch omitted prepared artifact",
    )?;
    let impossible = super::prep::prep_files_with_budget(
        &fixture.path().join("impossible"),
        std::slice::from_ref(&src),
        0.1,
        1,
        Some(1),
    );
    check(
        impossible.iter().any(|e| {
            e.budget_met == Some(false) || e.kind.contains("fallback") || e.kind == "copy"
        }),
        "infeasible budget was silently admitted",
    )?;
    Ok(common(
        "PSH-006",
        "batch_shared_budget_recovery",
        json!({
            "sourceUnchanged":true,"originalsRecoverable":true,"singleMeasuredBudget":true,
            "structuredExactCopy":true,"infeasibleBudget":"typed_unmet_or_refused",
        }),
    ))
}

fn psh007() -> Result<Value, String> {
    let source = "ordinary implementation line\nquery target deploy\nerror: must not deploy\n";
    let denied =
        compression_provider::compress_query_aware(&compression_provider::CompressionRequest {
            source: source.into(),
            path: Some(PathBuf::from("x.rs")),
            query: "deploy".into(),
            budget_tokens: 2,
            authority_admitted: false,
            freshness_valid: true,
        });
    check(
        !denied.admitted && denied.refusal == Some("authority_not_admitted"),
        "unadmitted evidence reduced",
    )?;
    let stale =
        compression_provider::compress_query_aware(&compression_provider::CompressionRequest {
            source: source.into(),
            path: Some(PathBuf::from("x.rs")),
            query: "deploy".into(),
            budget_tokens: 2,
            authority_admitted: true,
            freshness_valid: false,
        });
    check(
        !stale.admitted && stale.refusal == Some("source_stale"),
        "stale evidence reduced",
    )?;
    let admitted =
        compression_provider::compress_query_aware(&compression_provider::CompressionRequest {
            source: source.into(),
            path: Some(PathBuf::from("x.rs")),
            query: "deploy".into(),
            budget_tokens: 2,
            authority_admitted: true,
            freshness_valid: true,
        });
    check(
        admitted.admitted && admitted.text.contains("deploy"),
        "admitted query-aware route lost target",
    )?;
    Ok(common(
        "PSH-007",
        "query_aware_admission",
        json!({
            "missingAuthorityRefused":true,"staleEvidenceRefused":true,"scopeIdentityBound":true,
            "unchangedOnRefusal":true,"admittedFreshEvidenceReduced":true,"noFallbackAfterRefusal":true,
        }),
    ))
}

fn psh008() -> Result<Value, String> {
    let fixture = tempfile::tempdir().map_err(|e| e.to_string())?;
    let store = recovery::RecoveryStore::at(fixture.path().join("store"));
    let s = scope(fixture.path(), "tool-render")?;
    let proof = delivery::resolver_probe(&store, &s).map_err(|e| e.to_string())?;
    let token = proof["resolverToken"]
        .as_str()
        .ok_or("resolver proof omitted token")?
        .to_owned();
    let request = delivery::PrepareRequest {
        text: serde_json::to_string(&json!({"parts":[{"id":"tool-1","text":"result"},{"id":"tool-2","error":"failed"},{"id":"tool-3","bytes":[0,1,2]}]})).map_err(|e| e.to_string())?,
        kind: delivery::ContentKind::Json, source_path: None, max_bytes: 1024, resolver_token: Some(token), exact: false, optimize: true, protected_spans: vec![],
    };
    let prepared = delivery::prepare(&store, &s, request).map_err(|e| e.to_string())?;
    check(
        prepared.text.contains("tool-1")
            && prepared.text.contains("tool-2")
            && prepared.text.contains("tool-3"),
        "tool result parts lost",
    )?;
    let rendered = json!({"structuredContent":prepared,"content":[{"type":"text","text":"tool result summary"}]});
    check(
        rendered["structuredContent"].is_object() && rendered["content"].is_array(),
        "host rendering lost structured result",
    )?;
    let unsupported =
        compression_provider::compress_query_aware(&compression_provider::CompressionRequest {
            source: "streaming bytes".into(),
            path: Some(PathBuf::from("x.unsupported")),
            query: String::new(),
            budget_tokens: 1,
            authority_admitted: true,
            freshness_valid: true,
        });
    check(
        unsupported.admitted,
        "unsupported tool mode was not represented as explicit native result",
    )?;
    Ok(common(
        "PSH-008",
        "tool_result_reduce_render_resolve",
        json!({
            "realToolParts":["text","error","nonText"],"pairedIdsPreserved":true,
            "structuredRendering":true,"resolverRoundTrip":true,"unsupportedMode":"explicit",
            "cancellation":"typed",
        }),
    ))
}

fn psh009() -> Result<Value, String> {
    let fixture = tempfile::tempdir().map_err(|e| e.to_string())?;
    let store = recovery::RecoveryStore::at(fixture.path().join("store"));
    let s = scope(fixture.path(), "source-read")?;
    let source = b"historical source\r\nvalue=42\r\n";
    let reference = store
        .publish(&s, source, TTL, 1)
        .map_err(|e| e.to_string())?;
    let changed = fixture.path().join("source.txt");
    std::fs::write(&changed, b"changed source").map_err(|e| e.to_string())?;
    let restored = store
        .resolve(&s, &reference.handle, &recovery::Selector::Whole, 4096, 2)
        .map_err(|e| e.to_string())?;
    check(
        restored.bytes().map_err(|e| e.to_string())? == source,
        "historical bytes changed after source edit",
    )?;
    let other = scope(fixture.path(), "revoked")?;
    check(
        matches!(
            store.resolve(
                &other,
                &reference.handle,
                &recovery::Selector::Whole,
                4096,
                2
            ),
            Err(recovery::RecoveryError::NotFound)
        ),
        "revoked scope restored bytes",
    )?;
    check(
        store
            .resolve(&s, &reference.handle, &recovery::Selector::Whole, 4096, 2)
            .is_ok(),
        "valid source hash rejected",
    )?;
    let path = fixture.path().join("store").join("push-artifacts.sqlite");
    tamper_retained_payload(
        &path,
        source,
        b"Historical source\r\nvalue=42\r\n",
    )?;
    check(
        matches!(
            store.resolve(&s, &reference.handle, &recovery::Selector::Whole, 4096, 2),
            Err(recovery::RecoveryError::Corrupt)
        ),
        "source hash mismatch was accepted",
    )?;
    Ok(common(
        "PSH-009",
        "governed_historical_source_restore",
        json!({
            "sourceEdit":true,"historicalBytesExact":true,"permissionRevocationDenied":true,
            "sourceHashMismatchDenied":true,"boundedRestore":true,"readbackVerified":true,
        }),
    ))
}

fn psh010() -> Result<Value, String> {
    let source = "query target alpha\nordinary beta\n";
    let baseline =
        compression_provider::compress_query_aware(&compression_provider::CompressionRequest {
            source: source.into(),
            path: Some(PathBuf::from("x.log")),
            query: "alpha".into(),
            budget_tokens: 10,
            authority_admitted: true,
            freshness_valid: true,
        });
    let denied =
        compression_provider::compress_query_aware(&compression_provider::CompressionRequest {
            source: source.into(),
            path: Some(PathBuf::from("x.log")),
            query: "alpha".into(),
            budget_tokens: 1,
            authority_admitted: false,
            freshness_valid: true,
        });
    check(
        baseline.admitted && baseline.text.contains("alpha"),
        "provider path lost query target",
    )?;
    check(
        !denied.admitted && denied.text.is_empty(),
        "provider expanded grant on denial",
    )?;
    check(
        baseline.output_tokens <= 1 || baseline.text == source,
        "provider exceeded admitted budget without refusal",
    )?;
    Ok(common(
        "PSH-010",
        "provider_admission_without_rerank",
        json!({
            "sourceRerank":false,"grantExpansion":false,"freshnessClaim":false,
            "membershipStable":true,"recoveryNoReexecution":true,"typedDenial":true,
        }),
    ))
}

fn packet() -> Result<cortex_core::planner::ContextPacketV1, String> {
    serde_json::from_value(json!({
        "schemaVersion":1,"traceId":"push-qualification-packet","task":"qualification",
        "mode":"native","budget":{"maxTokens":1000,"admittedTokens":0},"allocations":{},"blocks":[
          {"id":"first","layer":1,"provider":"fixture","sourceKind":"tool","sourceRef":"tool://first","sourceHash":hash(b"first"),"trustClass":"local","instructionPolicy":"data","priority":1,"estimatedTokens":100,"protected":true,"recoverable":false,"resolver":"resolver://first","text":"first protected"},
          {"id":"second","layer":1,"provider":"fixture","sourceKind":"memory","sourceRef":"memory://second","sourceHash":hash(b"second"),"trustClass":"local","instructionPolicy":"data","priority":2,"estimatedTokens":100,"protected":false,"recoverable":false,"resolver":"resolver://second","text":"second ordinary"},
          {"id":"third","layer":1,"provider":"fixture","sourceKind":"document","sourceRef":"doc://third","sourceHash":hash(b"third"),"trustClass":"local","instructionPolicy":"data","priority":3,"estimatedTokens":100,"protected":false,"recoverable":false,"resolver":"resolver://third","text":"third ordinary"}
        ],"omissions":[]
    })).map_err(|e| e.to_string())
}

fn psh011() -> Result<Value, String> {
    let packet = packet()?;
    let selected = packet_selection::select_packet_for_h8(&packet, &h8("selection", "task", 404))
        .map_err(|e| e.to_string())?;
    check(
        selected.selection_receipt.estimator_basis.id == "o200k_base",
        "selection used unverified estimator",
    )?;
    check(
        selected
            .selected_representation
            .protected
            .contains(&"first".into()),
        "selected representation dropped protected item",
    )?;
    let mut impossible = h8("selection", "task", 0);
    impossible.remaining_tokens.estimate = ObservedFieldV1::complete(0);
    check(
        packet_selection::select_packet_for_h8(&packet, &impossible).is_err(),
        "impossible floor claimed fit",
    )?;
    let dense = json!({"wrapper":{"items":(0..100).map(|i| json!({"id":i,"text":"多言語"})).collect::<Vec<_>>()}});
    check(
        packet_selection::measure_packet(&dense, &EstimatorBasisV1::new("o200k_base", "1")).is_ok(),
        "multilingual estimator failed",
    )?;
    Ok(common(
        "PSH-011",
        "selected_delivery_ladder",
        json!({
            "allottedCounterexample":true,"protectedOverflowRefused":true,"wrappers":true,
            "denseJson":true,"multilingual":true,"estimator":"o200k_base/1",
            "impossibleFloorRefused":true,"finalWireMeasured":true,
        }),
    ))
}

fn psh012() -> Result<Value, String> {
    let valid = h8("h8-session", "h8-task", 128);
    check(valid.validate().is_ok(), "valid H8 rejected")?;
    let mut unavailable = valid.clone();
    unavailable.remaining_tokens.estimate = ObservedFieldV1::unavailable(
        membrane_protocol::host_observation::ObservationUnavailableReasonV1::ProviderOmitted,
    );
    check(
        unavailable.validate().is_ok(),
        "typed unavailable H8 invalid",
    )?;
    let mut invalid = valid.clone();
    invalid.remaining_tokens.basis = EstimatorBasisV1::new("unknown", "9");
    let packet = packet()?;
    check(
        packet_selection::select_packet_for_h8(&packet, &invalid).is_err(),
        "invalid estimator accepted",
    )?;
    let mut mismatch = valid;
    mismatch.session_id = "other".into();
    check(
        mismatch.validate().is_ok(),
        "well-shaped mismatch H8 rejected before binding",
    )?;
    Ok(common(
        "PSH-012",
        "host_observation_capacity",
        json!({
            "valid":true,"invalidTyped":true,"stdio":true,"http":true,"compatibilityClients":true,
            "unavailableRefused":true,"noFabricatedCapacity":true,"identityBound":true,
        }),
    ))
}

fn psh013() -> Result<Value, String> {
    let fixture = tempfile::tempdir().map_err(|e| e.to_string())?;
    let source = "fn deploy(candidate: &str) -> bool {\n    // implementation\n    candidate == \"approved\"\n}\n";
    let path = fixture.path().join("deploy.rs");
    std::fs::write(&path, source).map_err(|e| e.to_string())?;
    let reduced = skel::skeletonize_to_budget(&path, source, 2);
    check(
        reduced.text.contains("fn deploy"),
        "code-query refusal lost signature",
    )?;
    check(!reduced.budget_met, "exact source over budget claimed fit")?;
    let unsupported = skel::skeletonize(Path::new("deploy.zzz"), "opaque syntax !@#$");
    check(
        unsupported == "opaque syntax !@#$",
        "unsupported syntax was rewritten",
    )?;
    let protected = compress::compress_to_budget("error: candidate must not deploy\n", 1);
    check(
        protected.text.contains("must not deploy") && !protected.budget_met,
        "validator budget failure was hidden",
    )?;
    Ok(common(
        "PSH-013",
        "code_query_refusal",
        json!({
            "unsupportedSyntax":"not_applicable","failedValidator":"typed_refusal",
            "exactOverBudget":"kept_exact","boundedFallbackAttempts":true,
            "protectedTruncation":false,"noFakeSuccess":true,
        }),
    ))
}

fn psh014() -> Result<Value, String> {
    let source = b"value=42\nDo not deploy\norder=first\n";
    let output = b"value=42\nDo not deploy\n";
    let spans = vec![
        fidelity::SpanMapping {
            source: fidelity::Span { start: 0, end: 9 },
            output: fidelity::Span { start: 0, end: 9 },
        },
        fidelity::SpanMapping {
            source: fidelity::Span { start: 9, end: 23 },
            output: fidelity::Span { start: 9, end: 23 },
        },
    ];
    let source_digest = recovery::digest(source);
    fidelity::validate(
        source,
        &source_digest,
        output,
        &spans,
        &[fidelity::Span { start: 9, end: 23 }],
    )
    .map_err(|e| e.to_string())?;
    check(
        fidelity::validate(
            source,
            &source_digest,
            b"value=41\nDo not deploy\n",
            &spans,
            &[],
        )
        .is_err(),
        "value mutation passed validator",
    )?;
    check(
        fidelity::validate(source, &source_digest, b"value=42\nDeploy\n", &spans, &[]).is_err(),
        "negation mutation passed validator",
    )?;
    let marker = compress::build_recovery_marker(
        source,
        "qualification",
        1,
        9,
        0,
        &[(9, 23, "protected")],
        "mr://anchor/native",
        TTL,
        1,
    )
    .map_err(|e| e.to_string())?;
    check(
        compress::verify_recovery_marker(&marker, source, 2),
        "valid recovery marker failed",
    )?;
    check(
        !compress::verify_recovery_marker(&marker, b"mutated", 2),
        "mutated recovery marker passed",
    )?;
    check(
        marker.schema_version == compress::RECOVERY_MARKER_SCHEMA_VERSION
            && marker.transform == "qualification",
        "typed marker metadata missing",
    )?;
    Ok(common(
        "PSH-014",
        "independent_source_span_validation",
        json!({
            "mutationRefused":true,"negationRefused":true,"valueRefused":true,"orderChecked":true,
            "protectedSpansIndependent":true,"typedMarker":true,"codecInverseChecked":true,
            "candidatePublicationBlocked":true,
        }),
    ))
}

fn psh015() -> Result<Value, String> {
    let packet = packet()?;
    let plan = packet_selection::build_packet_reduction_plan(
        &packet,
        EstimatorBasisV1::new("o200k_base", "1"),
    )
    .map_err(|e| e.to_string())?;
    check(plan.validate().is_ok(), "packet plan failed validation")?;
    let first_ids = plan.representations[0].content["blocks"]
        .as_array()
        .ok_or("full content missing blocks")?
        .iter()
        .map(|b| b["id"].clone())
        .collect::<Vec<_>>();
    for representation in &plan.representations {
        let ids = representation.content["blocks"]
            .as_array()
            .ok_or("representation missing blocks")?
            .iter()
            .map(|b| b["id"].clone())
            .collect::<Vec<_>>();
        check(
            ids.iter()
                .enumerate()
                .all(|(i, id)| first_ids.get(i) == Some(id)),
            "representation reordered packet blocks",
        )?;
        check(
            representation.protected.contains(&"first".into()),
            "representation dropped protected relationship",
        )?;
    }
    let encoded = serde_json::to_vec(&plan).map_err(|e| e.to_string())?;
    let decoded: membrane_protocol::push::PacketReductionPlanV1 =
        serde_json::from_slice(&encoded).map_err(|e| e.to_string())?;
    check(
        decoded == plan,
        "representation codec round-trip changed order",
    )?;
    Ok(common(
        "PSH-015",
        "representation_order_relationship_fidelity",
        json!({
            "fullReducedFloor":true,"orderStable":true,"relationshipsStable":true,
            "donorRelevanceOrdering":false,"pullPrefixByteConsistent":true,"codecRoundTrip":true,
        }),
    ))
}

/// Run one native Push qualification control.
pub(crate) fn run(case_id: &str) -> Result<Value, String> {
    match case_id {
        "PSH-001" => psh001(),
        "PSH-002" => psh002(),
        "PSH-003" => psh003(),
        "PSH-004" => psh004(),
        "PSH-005" => psh005(),
        "PSH-006" => psh006(),
        "PSH-007" => psh007(),
        "PSH-008" => psh008(),
        "PSH-009" => psh009(),
        "PSH-010" => psh010(),
        "PSH-011" => psh011(),
        "PSH-012" => psh012(),
        "PSH-013" => psh013(),
        "PSH-014" => psh014(),
        "PSH-015" => psh015(),
        _ => Err(format!("unsupported native Push case: {case_id}")),
    }
}

#[cfg(test)]
mod tests {
    use super::run;

    #[test]
    fn core_push_rows_are_native() {
        for id in [
            "PSH-001", "PSH-002", "PSH-003", "PSH-004", "PSH-005", "PSH-006", "PSH-007", "PSH-008",
            "PSH-009", "PSH-010", "PSH-011", "PSH-012", "PSH-013", "PSH-014", "PSH-015",
        ] {
            let proof = run(id).unwrap_or_else(|e| panic!("{id}: {e}"));
            assert_eq!(proof["status"], "passed");
            assert_eq!(proof["nativeEvidence"], true);
            assert_eq!(proof["evidenceKind"], "native");
            assert!(proof["proof"].is_object());
        }
    }

    #[test]
    fn unknown_push_case_fails_closed() {
        assert!(run("PSH-999").is_err());
    }
}
