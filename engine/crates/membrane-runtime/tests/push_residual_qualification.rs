//! Public-path qualification for committed Push residuals.
//!
//! Mapping (the assertions are intentionally behavioral, not inventory-only):
//!
//! * PSH-001/002/005/009/022/023/024/025/028/029: `capture_and_recovery_public_path`
//!   and `recovery_selectors_expiry_and_limits`.
//! * PSH-003/014/015/026: `protected_fidelity_refuses_loss_and_preserves_order`.
//! * PSH-004/006/007/010/013/027: `representation_policy_is_governed_and_bounded`.
//! * PSH-008/011/012/019: `selection_receipt_and_h8_are_typed_and_content_free`.
//! * PSH-016/017: `measurement_and_observations_are_typed_and_joinable`.
//! * PSH-018/020/021: `governed_adapter_is_direct_and_confined`.
//!
//! These tests call exported Push owners. They do not duplicate the frozen
//! recovery hardening/end-to-end suites or assert implementation-private state.

use cortex_core::planner::{BlockV1, BudgetV1, ContextPacketV1};
use membrane_protocol::host_observation::{
    EstimatorBasisV1, HostObservationProvenanceV1, ObservedFieldV1,
    RemainingContextCeilingV1, TokenEstimateV1, REMAINING_CONTEXT_CEILING_SCHEMA_VERSION,
};
use membrane_runtime::push::{
    compress, delivery, egress, fidelity, packet_selection, prep, runc, selection, telemetry,
    PushPolicy,
};
use membrane_runtime::push::delivery::{ContentKind, PrepareRequest};
use membrane_runtime::push::recovery::{RecoveryError, RecoveryScope, RecoveryStore, Selector};
use membrane_runtime::push::runc::{CommandAdapter, CommandAdapterError, CommandAdapterRejectionKind};
use serde_json::json;
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

fn request(text: impl Into<String>, kind: ContentKind, token: Option<String>, max_bytes: usize) -> PrepareRequest {
    PrepareRequest {
        text: text.into(),
        kind,
        source_path: None,
        max_bytes,
        resolver_token: token,
        exact: false,
        optimize: true,
        protected_spans: Vec::new(),
    }
}

fn ceiling(session: &str, task: &str, tokens: u64) -> RemainingContextCeilingV1 {
    RemainingContextCeilingV1 {
        schema_version: REMAINING_CONTEXT_CEILING_SCHEMA_VERSION,
        ceiling_id: "push-residual-ceiling".into(),
        session_id: session.into(),
        task_id: ObservedFieldV1::complete(task.into()),
        requested_at_unix_ms: 1_700_000_000_000,
        remaining_tokens: TokenEstimateV1::complete(
            EstimatorBasisV1::new("o200k_base", "1"),
            tokens,
        ),
        provenance_receipt: HostObservationProvenanceV1::new(
            "push-residual-receipt",
            "qualification-host",
            1_700_000_000_000,
            "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ),
    }
}

fn block(id: &str, protected: bool, text: &str) -> BlockV1 {
    let tokens = compress::estimate_tokens(text);
    BlockV1 {
        id: id.into(),
        layer: 1,
        provider: "qualification".into(),
        source_kind: "file".into(),
        source_ref: format!("source://{id}.txt"),
        source_hash: format!("sha256:{:0<64}", id),
        trust_class: "trusted".into(),
        instruction_policy: "data".into(),
        base_commit: None,
        overlay_digest: None,
        freshness_class: None,
        snapshot_id: None,
        priority: 1,
        estimated_tokens: tokens,
        delivery_stage: None,
        delivery_class: None,
        selected_tokens: Some(tokens),
        allotted_tokens: Some(tokens),
        rendered_tokens: None,
        delivered_chars: None,
        drop_reason: None,
        protected,
        recoverable: true,
        resolver: format!("resolver://{id}"),
        text: text.into(),
    }
}

fn packet() -> ContextPacketV1 {
    ContextPacketV1 {
        schema_version: 1,
        trace_id: "push-residual-trace".into(),
        task: "push-residual-task".into(),
        mode: "qualification".into(),
        budget: BudgetV1 {
            max_tokens: 512,
            admitted_tokens: 0,
            packet_char_budget_default: None,
            packet_char_budget_override: None,
            packet_char_budget_model: None,
            configured_packet_char_budget: None,
            effective_packet_char_budget: None,
        },
        allocations: std::collections::BTreeMap::new(),
        provider_accounting: std::collections::BTreeMap::new(),
        blocks: vec![
            block("protected", true, "decision: never deploy; error=E42\n"),
            block("ordinary", false, "ordinary implementation detail repeated repeated repeated\n"),
        ],
        omissions: Vec::new(),
    }
}

#[test]
fn capture_and_recovery_public_path() {
    // PSH-001: one capture retains status plus a deterministic capped view.
    // PSH-002/005/009/025: exact recovery is advertised only after durable publication
    // and a resolver proof; the proof is scope/store bound.
    let repo = tempfile::tempdir().unwrap();
    let spills = tempfile::tempdir().unwrap();
    let mut command = Command::new("node");
    command.current_dir(repo.path()).args([
        "-e",
        "process.stdout.write(Array.from({length: 12}, (_, i) => `line${i}\\n`).join(''))",
    ]);
    let captured = runc::run_command_capped_with_limits(
        command,
        2,
        2,
        spills.path(),
        Duration::from_secs(10),
        &tokio_util::sync::CancellationToken::new(),
    )
    .unwrap();
    assert_eq!(captured.exit_code, 0);
    assert!(captured.capped.contains("lines elided"));
    let handle = captured.anchor.as_str();
    assert!(handle.starts_with("mr://anchor/"));
    let scope = RecoveryScope::new(repo.path(), "local").unwrap();
    let store = RecoveryStore::at(spills.path());
    let exact = store
        .resolve(&scope, handle, &Selector::Whole, 64 * 1024, membrane_runtime::push::recovery::now_ms())
        .unwrap();
    assert_eq!(exact.bytes().unwrap(), std::fs::read(captured.spill_path.unwrap()).unwrap());

    // A missing proof cannot turn an optimized lossy request into an opaque pointer.
    let denied = delivery::prepare(&store, &scope, request("event\n".repeat(2_000), ContentKind::Log, None, 2_500));
    assert!(matches!(denied, Err(RecoveryError::Denied) | Err(RecoveryError::Limit)));
}

#[test]
fn recovery_selectors_expiry_and_limits() {
    // PSH-022/023/024/028/029: canonical opaque anchors, typed selector/lease
    // outcomes, independent expiry, explicit invalidation, and bounded restore.
    let temp = tempfile::tempdir().unwrap();
    let store = RecoveryStore::at(temp.path());
    let scope = RecoveryScope::new(temp.path(), "selector-session").unwrap();
    let bytes = br#"{"items":[{"name":"alpha"},{"name":"beta"}],"status":"ok"}"#;
    let reference = store.publish(&scope, bytes, 100, 1).unwrap();
    assert!(reference.handle.starts_with("mr://anchor/"));
    assert!(!reference.handle.ends_with(&reference.source_digest[7..]));
    assert_eq!(reference.lease_state, "active");

    let selected = store
        .resolve(&scope, &reference.handle, &Selector::Bytes { start: 2, end: 7 }, 64, 2)
        .unwrap();
    assert_eq!(selected.bytes().unwrap(), &bytes[2..7]);
    let json_value = store
        .resolve(
            &scope,
            &reference.handle,
            &Selector::Json { path: vec![
                membrane_runtime::push::recovery::JsonStep::Field { name: "items".into() },
                membrane_runtime::push::recovery::JsonStep::Index { index: 1 },
                membrane_runtime::push::recovery::JsonStep::Field { name: "name".into() },
            ] },
            64,
            2,
        )
        .unwrap();
    assert_eq!(json_value.bytes().unwrap(), br#""beta""#);
    assert!(matches!(
        store.resolve(&scope, &reference.handle, &Selector::Whole, 4, 2),
        Err(RecoveryError::Limit)
    ));
    assert!(matches!(
        store.resolve(&scope, "mr://anchor/not-hex", &Selector::Whole, 64, 2),
        Err(RecoveryError::InvalidAnchor)
    ));
    assert!(matches!(
        store.resolve(&scope, &reference.handle, &Selector::Whole, 64, 101),
        Err(RecoveryError::Expired)
    ));

    let fresh = store.publish(&scope, bytes, 100, 102).unwrap();
    store.invalidate(&scope, &fresh.handle).unwrap();
    assert!(matches!(
        store.resolve(&scope, &fresh.handle, &Selector::Whole, 64, 103),
        Err(RecoveryError::Invalidated)
    ));
    assert!(matches!(
        store.publish(&scope, &vec![0u8; membrane_runtime::push::recovery::MAX_ARTIFACT_BYTES + 1], 100, 1),
        Err(RecoveryError::Limit)
    ));
}

#[test]
fn protected_fidelity_refuses_loss_and_preserves_order() {
    // PSH-003/014: mandatory source spans are validated against immutable bytes;
    // mutation, wrong occurrence, and a protected floor over budget are refused.
    let source = "ordinary\nerror: must not deploy /srv/app\nordinary tail\n";
    let protected = fidelity::protected_lines(source);
    let (reduced, mappings) = fidelity::extract_lines(source, source.len(), &[]).unwrap();
    fidelity::validate(source.as_bytes(), &membrane_runtime::push::recovery::digest(source.as_bytes()), reduced.as_bytes(), &mappings, &protected).unwrap();
    assert!(reduced.contains("must not deploy"));
    assert!(matches!(fidelity::extract_lines(source, 4, &protected), Err(RecoveryError::Limit)));
    let mapping = fidelity::SpanMapping {
        source: fidelity::Span { start: 0, end: 8 },
        output: fidelity::Span { start: 0, end: 8 },
    };
    assert!(matches!(
        fidelity::validate(source.as_bytes(), &membrane_runtime::push::recovery::digest(source.as_bytes()), b"mutated!", &[mapping], &[]),
        Err(RecoveryError::Corrupt)
    ));

    // PSH-015/026: packet ordering/atomic grouping survive each representation;
    // exact inputs remain exact and are not re-reduced on re-entry.
    let plan = packet_selection::build_packet_reduction_plan(
        &packet(),
        EstimatorBasisV1::new("o200k_base", "1"),
    )
    .unwrap();
    for representation in &plan.representations {
        let blocks: Vec<BlockV1> = serde_json::from_value(representation.content["blocks"].clone()).unwrap();
        assert_eq!(blocks.iter().map(|b| b.id.as_str()).collect::<Vec<_>>(), ["protected", "ordinary"]);
        assert_eq!(blocks[0].text, "decision: never deploy; error=E42\n");
    }
    let temp = tempfile::tempdir().unwrap();
    let store = RecoveryStore::at(temp.path());
    let scope = RecoveryScope::new(temp.path(), "exact-session").unwrap();
    let mut exact = request("exact source\n".repeat(50), ContentKind::Text, None, 2_048);
    exact.exact = true;
    exact.optimize = true;
    let prepared = delivery::prepare(&store, &scope, exact).unwrap();
    assert_eq!(prepared.disposition, "exact");
    assert_eq!(prepared.representation_kind, "original");
}

#[test]
fn representation_policy_is_governed_and_bounded() {
    // PSH-004/006: deterministic fallback compression plus shared bounded prep.
    let source = "ordinary text\nerror: must not drop\n".repeat(40);
    assert_eq!(
        compress::compress_to_budget_with_options(&source, 24, true).text,
        compress::compress_to_budget_with_options(&source, 24, true).text
    );
    assert_eq!(compress::compress_to_budget_with_options(&source, usize::MAX, true).text, source);
    let temp = tempfile::tempdir().unwrap();
    let first = temp.path().join("first.rs");
    let second = temp.path().join("second.rs");
    std::fs::write(&first, "fn first() { let value = 1; println!(\"{value}\"); }\n").unwrap();
    std::fs::write(&second, "fn second() { let value = 2; println!(\"{value}\"); }\n").unwrap();
    let originals = [std::fs::read(&first).unwrap(), std::fs::read(&second).unwrap()];
    let manifest = prep::prep_files_with_budget_and_policy(
        &temp.path().join("prepared"),
        &[first.clone(), second.clone()],
        0.5,
        1,
        Some(80),
        PushPolicy::Control,
    );
    assert_eq!(manifest.len(), 2);
    assert_eq!(std::fs::read(&first).unwrap(), originals[0]);
    assert_eq!(std::fs::read(&second).unwrap(), originals[1]);
    assert!(manifest.iter().all(|entry| entry.prepared.is_some()));

    // PSH-007/010/013/027: provider-local policy cannot mint authority/freshness;
    // rejected economics return typed refusal, while positive savings are measured.
    let refused = prep::prepare_query_aware(membrane_runtime::push::compression_provider::CompressionRequest {
        source: source.clone(), path: Some(PathBuf::from("notes.txt")), query: "error".into(),
        budget_tokens: 24, authority_admitted: false, freshness_valid: true,
    });
    assert!(!refused.admitted);
    assert_eq!(refused.refusal, Some("authority_not_admitted"));
    let temp = tempfile::tempdir().unwrap();
    let store = RecoveryStore::at(temp.path());
    let scope = RecoveryScope::new(temp.path(), "economics").unwrap();
    let token = delivery::resolver_probe(&store, &scope).unwrap()["resolverToken"].as_str().unwrap().to_owned();
    let reduced = delivery::prepare(&store, &scope, request(source, ContentKind::Log, Some(token), 2_500)).unwrap();
    assert!(reduced.receipt.saved_bytes > 0);
    assert_eq!(reduced.receipt.measurement_basis, "utf8_serialized_push_delivery_v1");
}

#[test]
fn selection_receipt_and_h8_are_typed_and_content_free() {
    // PSH-008/011/012/019: final selection uses exact serialized measurements,
    // refuses absent/inexact/identity-mismatched H8, and keeps payload out of receipt.
    let packet = packet();
    let valid = ceiling("push-session", "push-task", 100_000);
    let selected = selection::select_packet_for_h8(&packet, &valid).unwrap();
    assert_eq!(selected.selected_representation.id, "full");
    assert_eq!(selected.selection_receipt.decision, "selected");
    assert!(!serde_json::to_string(&selected.selection_receipt).unwrap().contains("ordinary implementation"));
    let body = json!({"remainingContextCeiling": valid});
    assert!(matches!(selection::parse_request_time_h8(&body, "other", "push-task"), Err(selection::RequestTimeH8Error::IdentityMismatch { field: "sessionId", .. })));
    let no_h8 = json!({});
    assert!(matches!(selection::parse_request_time_h8(&no_h8, "push-session", "push-task"), Err(selection::RequestTimeH8Error::Missing)));
    let mut inexact = valid;
    inexact.remaining_tokens.estimate.value = None;
    assert!(matches!(selection::parse_request_time_h8(&json!({"remainingContextCeiling": inexact}), "push-session", "push-task"), Err(selection::RequestTimeH8Error::Inexact { .. })));
    let measurement = packet_selection::measure_packet(&json!({"text":"payload"}), &EstimatorBasisV1::new("wrong", "1"));
    assert!(matches!(measurement, Err(selection::PacketReductionRequestError::H8(selection::RequestTimeH8Error::Invalid(_)))));
}

#[test]
fn measurement_and_observations_are_typed_and_joinable() {
    // PSH-016/017: unit/basis are explicit, provider billing stays unknown, and
    // observations carry a stable Push join identity without payload bodies.
    let before = telemetry::status()["observed"].as_u64().unwrap();
    telemetry::record("prepare", 100, 40, Some("status=reduced;scope=serialized_delivery"), Some("source-digest"));
    let status = telemetry::status();
    assert!(status["observed"].as_u64().unwrap() > before);
    assert_eq!(status["coverage"], "process_local");
    assert!(status["providerBilledTokens"].is_null());
    assert_eq!(status["taskOutcome"], "unknown");

    // The native egress owner measures the complete tool-result wire shape,
    // including its own measurement fields, and refuses unsupported basis.
    let envelope = json!({"schemaVersion":1,"operation":"membrane_push_prepare","errorVersion":1,
        "result":{"kind":"success","data":{"text":"selected","representation":"reduced"}}});
    let fitted = egress::fit_native_response(envelope.clone(), &ceiling("push-session", "push-task", 100_000)).unwrap();
    let wire = membrane_mcp::tool_result(fitted.clone()).to_string();
    assert_eq!(fitted["result"]["data"]["deliveryMeasurement"]["bytes"], wire.len());
    assert_eq!(fitted["result"]["data"]["deliveryMeasurement"]["basis"], "o200k_base/1");
    assert!(matches!(egress::fit_native_response(envelope, &RemainingContextCeilingV1 {
        remaining_tokens: TokenEstimateV1::complete(EstimatorBasisV1::new("wrong", "1"), 100_000),
        ..ceiling("push-session", "push-task", 100_000)
    }), Err(RecoveryError::Denied)));
}

#[test]
fn governed_adapter_is_direct_and_confined() {
    // PSH-018/020/021: only named adapters execute direct argv within a
    // canonical repository root; shell programs, root escapes, and unsupported
    // invocations are refused before spawn.
    let repo = tempfile::tempdir().unwrap();
    let spills = tempfile::tempdir().unwrap();
    let unsupported = runc::run_adapter_capped(
        CommandAdapter::Git,
        repo.path(),
        OsStr::new("sh"),
        &[OsString::from("-c"), OsString::from("echo unsafe")],
        10,
        10,
        spills.path(),
    );
    assert!(matches!(unsupported, Err(CommandAdapterError::Rejected { kind: CommandAdapterRejectionKind::UnsupportedProgram, .. })));
    let escaped = runc::run_adapter_capped(
        CommandAdapter::Git,
        repo.path(),
        OsStr::new("/tmp/git"),
        &[OsString::from("status")],
        10,
        10,
        spills.path(),
    );
    assert!(matches!(escaped, Err(CommandAdapterError::Rejected { kind: CommandAdapterRejectionKind::RootEscape, .. })));
    let invocation = runc::run_adapter_capped(
        CommandAdapter::Git,
        repo.path(),
        OsStr::new("git"),
        &[OsString::from("-C"), OsString::from(".."), OsString::from("status")],
        10,
        10,
        spills.path(),
    );
    assert!(matches!(invocation, Err(CommandAdapterError::Rejected { kind: CommandAdapterRejectionKind::UnsupportedInvocation, .. })));
}
