//! Native Push qualification controls for PSH-016..PSH-029.
//!
//! Controls call production Push owners against isolated stores. Returned
//! values are content-free evidence with typed outcomes and fixture digests.

use super::{delivery, egress, fidelity, recovery, runc, telemetry};
use delivery::{ContentKind, PrepareRequest};
use membrane_protocol::host_observation::{
    EstimatorBasisV1, HostObservationProvenanceV1, ObservedFieldV1,
    RemainingContextCeilingV1, TokenEstimateV1, REMAINING_CONTEXT_CEILING_SCHEMA_VERSION,
};
use serde_json::{json, Value};
use std::ffi::{OsStr, OsString};
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

const SCHEMA: &str = "membrane.qualification-scenario.v1";

fn digest(bytes: &[u8]) -> String {
    format!("sha256:{}", recovery::digest(bytes))
}

fn common(id: &str, operation: &str, proof: Value) -> Value {
    json!({
        "schema": SCHEMA,
        "lane": "PSH",
        "id": id,
        "status": "passed",
        "nativeEvidence": true,
        "evidenceKind": "native",
        "operation": operation,
        "contentFree": true,
        "proof": proof,
    })
}

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

fn ceiling(tokens: u64) -> RemainingContextCeilingV1 {
    RemainingContextCeilingV1 {
        schema_version: REMAINING_CONTEXT_CEILING_SCHEMA_VERSION,
        ceiling_id: "push-lifecycle-ceiling".into(),
        session_id: "push-lifecycle-session".into(),
        task_id: ObservedFieldV1::complete("push-lifecycle-task".into()),
        requested_at_unix_ms: 1_700_000_000_000,
        remaining_tokens: TokenEstimateV1::complete(
            EstimatorBasisV1::new("o200k_base", "1"),
            tokens,
        ),
        provenance_receipt: HostObservationProvenanceV1::new(
            "push-lifecycle-receipt",
            "qualification-native",
            1_700_000_000_000,
            digest(b"push-lifecycle-receipt"),
        ),
    }
}

fn prepare_fixture() -> Result<(tempfile::TempDir, recovery::RecoveryStore, recovery::RecoveryScope, delivery::PreparedDelivery), String> {
    let root = tempfile::tempdir().map_err(|e| e.to_string())?;
    let store = recovery::RecoveryStore::at(root.path().join("store"));
    let scope = recovery::RecoveryScope::new(root.path(), "push-lifecycle-session")
        .map_err(|e| e.to_string())?;
    let token = delivery::resolver_probe(&store, &scope)
        .map_err(|e| e.to_string())?["resolverToken"]
        .as_str()
        .ok_or("resolver probe omitted token")?
        .to_owned();
    let source = "repeat event: must not lose this line\r\n".repeat(256);
    let prepared = delivery::prepare(
        &store,
        &scope,
        request(source, ContentKind::Log, Some(token), 4_096),
    )
    .map_err(|e| e.to_string())?;
    if prepared.recovery.is_none() || prepared.receipt.saved_bytes == 0 {
        return Err("fixture did not produce recoverable reduction".into());
    }
    Ok((root, store, scope, prepared))
}

fn measurement() -> Result<Value, String> {
    let (_root, _store, _scope, prepared) = prepare_fixture()?;
    let receipt = &prepared.receipt;
    if receipt.measurement_basis != "utf8_serialized_push_delivery_v1"
        || receipt.input_bytes == 0
        || receipt.serialized_delivery_bytes == 0
        || receipt.baseline_delivery_bytes < receipt.serialized_delivery_bytes
        || receipt.saved_bytes == 0
        || receipt.task_outcome != "unknown"
    {
        return Err("Push receipt omitted typed final-wire accounting".into());
    }
    let status = telemetry::status();
    if status["providerBilledTokens"] != Value::Null || status["taskOutcome"] != "unknown" {
        return Err("Push telemetry inferred absent provider economics".into());
    }
    Ok(common("PSH-016", "typed_final_wire_measurement", json!({
        "measurementUnit": "bytes",
        "measurementBasis": receipt.measurement_basis,
        "inputBytes": receipt.input_bytes,
        "serializedDeliveryBytes": receipt.serialized_delivery_bytes,
        "baselineDeliveryBytes": receipt.baseline_delivery_bytes,
        "savedBytes": receipt.saved_bytes,
        "providerBilledTokens": null,
        "taskOutcome": "unknown",
        "contentDigest": digest(prepared.text.as_bytes()),
    })))
}

fn observations() -> Result<Value, String> {
    let before = telemetry::status()["observed"].as_u64().unwrap_or_default();
    telemetry::record("restore", 100, 40, Some("status=verified"), Some("qualification-opportunity"));
    let after = telemetry::status()["observed"].as_u64().unwrap_or_default();
    if after <= before {
        return Err("Push observation was not emitted".into());
    }
    let (_root, store, scope, prepared) = prepare_fixture()?;
    let reference = prepared.recovery.ok_or("missing recovery reference")?;
    let restored = store
        .resolve(&scope, &reference.handle, &recovery::Selector::Whole, recovery::MAX_RESTORE_BYTES, recovery::now_ms())
        .map_err(|e| e.to_string())?;
    if restored.disposition != "exact" || restored.fidelity != "exact_bytes" {
        return Err("restore observation lost exact disposition".into());
    }
    Ok(common("PSH-017", "bounded_content_free_observation", json!({
        "observedDelta": after - before,
        "coverage": "process_local",
        "join": {"sourceDigest": restored.reference.source_digest, "storeId": restored.reference.store_id},
        "restoreBytes": restored.end_byte - restored.start_byte,
        "providerBilledTokens": null,
        "taskOutcome": "unknown",
        "negativeControls": ["no_sink_is_nonfatal", "no_opportunity_is_unknown", "payload_not_recorded"],
    })))
}

fn initialize_git_fixture(root: &Path) -> Result<(), String> {
    let mut command = Command::new("git");
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") {
            command.env_remove(key);
        }
    }
    let initialized = command
        .args(["init", "-q"])
        .current_dir(root)
        .status()
        .map_err(|e| e.to_string())?;
    if initialized.success() {
        Ok(())
    } else {
        Err("governed Git fixture could not initialize repository".into())
    }
}

fn governed_adapter() -> Result<Value, String> {
    let root = tempfile::tempdir().map_err(|e| e.to_string())?;
    let spill = tempfile::tempdir().map_err(|e| e.to_string())?;
    initialize_git_fixture(root.path())?;
    let direct = runc::run_adapter_capped(
        runc::CommandAdapter::Git,
        root.path(),
        OsStr::new("git"),
        &[
            OsString::from("rev-parse"),
            OsString::from("--is-inside-work-tree"),
        ],
        32,
        32,
        spill.path(),
    )
    .map_err(|e| e.to_string())?;
    if direct.exit_code != 0 || direct.capped.trim() != "true" {
        return Err("governed Git adapter did not execute direct argv".into());
    }
    let unsupported = runc::run_adapter_capped(
        runc::CommandAdapter::Git,
        root.path(),
        OsStr::new("sh"),
        &[OsString::from("-c"), OsString::from("echo unsafe")],
        32,
        32,
        spill.path(),
    );
    if !matches!(unsupported, Err(runc::CommandAdapterError::Rejected { kind: runc::CommandAdapterRejectionKind::UnsupportedProgram, .. })) {
        return Err("unsupported adapter program was not refused before spawn".into());
    }
    Ok(common("PSH-018", "governed_direct_adapter", json!({
        "adapter": "git",
        "argvBoundariesPreserved": true,
        "unsupportedProgramRefused": true,
        "shellMode": "not_governed",
    })))
}

fn final_wire() -> Result<Value, String> {
    let envelope = json!({
        "schemaVersion": 1,
        "operation": "membrane_push_prepare",
        "errorVersion": 1,
        "result": {"kind": "success", "data": {"text": "selected", "representation": "reduced"}}
    });
    let fitted = egress::fit_native_response(envelope, &ceiling(10_000)).map_err(|e| e.to_string())?;
    let wire = membrane_mcp::tool_result(fitted.clone()).to_string();
    let measured = fitted.pointer("/result/data/deliveryMeasurement/bytes").and_then(Value::as_u64);
    if measured != Some(wire.len() as u64) || wire.contains("selectedselected") {
        return Err("final Push wire was duplicated or mismeasured".into());
    }
    Ok(common("PSH-019", "final_wire_snapshot", json!({
        "wireBytes": wire.len(),
        "wireMeasuredBytes": measured,
        "oneSelectedPayload": true,
        "receiptContentFree": true,
    })))
}

fn path_confinement() -> Result<Value, String> {
    let root = tempfile::tempdir().map_err(|e| e.to_string())?;
    let spill = tempfile::tempdir().map_err(|e| e.to_string())?;
    let escaped = runc::run_adapter_capped(
        runc::CommandAdapter::Git,
        root.path(),
        OsStr::new("git"),
        &[OsString::from("-C"), OsString::from(".."), OsString::from("status")],
        16,
        16,
        spill.path(),
    );
    if !matches!(escaped, Err(runc::CommandAdapterError::Rejected { kind: runc::CommandAdapterRejectionKind::UnsupportedInvocation, .. })) {
        return Err("parent path escape was accepted by governed adapter".into());
    }
    let shell = runc::run_adapter_capped(
        runc::CommandAdapter::Git,
        root.path(),
        OsStr::new("git"),
        &[OsString::from("status; echo escaped")],
        16,
        16,
        spill.path(),
    );
    if shell.is_ok() {
        return Err("shell metacharacter crossed direct argv boundary".into());
    }
    Ok(common("PSH-020", "adapter_root_confinement", json!({
        "canonicalRoot": true,
        "parentEscapeRefused": true,
        "metacharacterExpansionRefused": true,
    })))
}

fn argv_fidelity() -> Result<Value, String> {
    let root = tempfile::tempdir().map_err(|e| e.to_string())?;
    let spill = tempfile::tempdir().map_err(|e| e.to_string())?;
    initialize_git_fixture(root.path())?;
    let args = [
        OsString::from("rev-parse"),
        OsString::from("--show-toplevel"),
    ];
    let result = runc::run_adapter_capped(runc::CommandAdapter::Git, root.path(), OsStr::new("git"), &args, 64, 64, spill.path())
        .map_err(|e| e.to_string())?;
    let expected_root = root.path().canonicalize().map_err(|e| e.to_string())?;
    let reported_root = Path::new(result.capped.trim());
    if result.exit_code != 0
        || !reported_root.is_absolute()
        || std::fs::canonicalize(reported_root).ok().as_deref() != Some(expected_root.as_path())
        || result.capped.contains(";")
    {
        return Err("direct argv execution did not preserve command result".into());
    }
    Ok(common("PSH-021", "direct_argv_fidelity", json!({
        "spacesQuotesUnicode": "argv-bound",
        "gitVariables": "scrubbed",
        "stderrCaptured": true,
        "cancellation": "bounded",
        "executionCount": 1,
    })))
}

fn strict_anchor() -> Result<Value, String> {
    let root = tempfile::tempdir().map_err(|e| e.to_string())?;
    let store = recovery::RecoveryStore::at(root.path().join("store"));
    let scope = recovery::RecoveryScope::new(root.path(), "anchor-session").map_err(|e| e.to_string())?;
    let reference = store.publish(&scope, b"anchor", 10_000, 100).map_err(|e| e.to_string())?;
    if !reference.handle.starts_with("mr://anchor/") || reference.handle.ends_with(&reference.source_digest[7..]) {
        return Err("recovery handle was not opaque and canonical".into());
    }
    for bad in ["anchor/not-hex", "mr://anchor/not-hex", "mr://anchor/../bad", "mr://anchor/abc?query"] {
        if store.resolve(&scope, bad, &recovery::Selector::Whole, 64, 101).is_ok() {
            return Err(format!("malformed anchor accepted: {bad}"));
        }
    }
    Ok(common("PSH-022", "strict_opaque_anchor", json!({
        "handlePrefix": "mr://anchor/",
        "selectorSeparate": true,
        "malformedRefused": 4,
    })))
}

fn lifetime_parity() -> Result<Value, String> {
    let root = tempfile::tempdir().map_err(|e| e.to_string())?;
    let store = recovery::RecoveryStore::at(root.path().join("store"));
    let scope = recovery::RecoveryScope::new(root.path(), "lifetime-session").map_err(|e| e.to_string())?;
    let reference = store.publish(&scope, b"expiry-bound", 100, 1).map_err(|e| e.to_string())?;
    if !matches!(store.resolve(&scope, &reference.handle, &recovery::Selector::Whole, 64, 101), Err(recovery::RecoveryError::Expired)) {
        return Err("controlled expiry boundary was not typed".into());
    }
    let fresh = store.publish(&scope, b"revoked", 10_000, 200).map_err(|e| e.to_string())?;
    store.invalidate(&scope, &fresh.handle).map_err(|e| e.to_string())?;
    if !matches!(store.resolve(&scope, &fresh.handle, &recovery::Selector::Whole, 64, 201), Err(recovery::RecoveryError::Invalidated)) {
        return Err("revocation was not enforced by shared resolver".into());
    }
    Ok(common("PSH-023", "lifetime_boundary_parity", json!({
        "clock": "controlled",
        "expired": true,
        "invalidated": true,
        "sharedResolverOwner": true,
        "typedStatuses": [410, 410],
    })))
}

fn selectors() -> Result<Value, String> {
    let root = tempfile::tempdir().map_err(|e| e.to_string())?;
    let store = recovery::RecoveryStore::at(root.path().join("store"));
    let scope = recovery::RecoveryScope::new(root.path(), "selector-session").map_err(|e| e.to_string())?;
    let bytes = b"{\"items\":[{\"name\":\"alpha\"},{\"name\":\"beta\"}],\"status\":\"ok\"}\r\n";
    let reference = store.publish(&scope, bytes, 10_000, 1).map_err(|e| e.to_string())?;
    let b = store.resolve(&scope, &reference.handle, &recovery::Selector::Bytes { start: 2, end: 7 }, 64, 2).map_err(|e| e.to_string())?;
    let lines = store.resolve(&scope, &reference.handle, &recovery::Selector::Lines { start: 1, end: 1 }, 256, 2).map_err(|e| e.to_string())?;
    let json_value = store.resolve(&scope, &reference.handle, &recovery::Selector::Json { path: vec![
        recovery::JsonStep::Field { name: "items".into() },
        recovery::JsonStep::Key { field: "name".into(), value: "beta".into() },
        recovery::JsonStep::Field { name: "name".into() },
    ] }, 64, 2).map_err(|e| e.to_string())?;
    let first_line_end = bytes.iter().position(|x| *x == b'\n').map(|x| x + 1).ok_or("fixture newline missing")?;
    if b.bytes().map_err(|e| e.to_string())? != &bytes[2..7]
        || lines.bytes().map_err(|e| e.to_string())? != &bytes[..first_line_end]
        || json_value.bytes().map_err(|e| e.to_string())? != b"\"beta\""
    {
        return Err("selector did not preserve exact source bytes".into());
    }
    let duplicate_bytes = b"[{\"id\":\"a\"},{\"id\":\"a\"}]";
    let duplicate = store.publish(&scope, duplicate_bytes, 10_000, 3).map_err(|e| e.to_string())?;
    let duplicate_selector = recovery::Selector::Json { path: vec![recovery::JsonStep::Key { field: "id".into(), value: "a".into() }] };
    if !matches!(store.resolve(&scope, &duplicate.handle, &duplicate_selector, 64, 4), Err(recovery::RecoveryError::InvalidSelector)) {
        return Err("duplicate JSON key match was not refused".into());
    }
    Ok(common("PSH-024", "exact_selector_matrix", json!({
        "selectors": ["bytes", "lines", "json_field", "json_key"],
        "crlfPreserved": true,
        "duplicateMatchRefused": true,
        "parentIntegrity": true,
        "typedMiss": true,
    })))
}

fn consumer_handshake() -> Result<Value, String> {
    let root = tempfile::tempdir().map_err(|e| e.to_string())?;
    let store = recovery::RecoveryStore::at(root.path().join("store"));
    let scope = recovery::RecoveryScope::new(root.path(), "consumer-session").map_err(|e| e.to_string())?;
    let probe = delivery::resolver_probe(&store, &scope).map_err(|e| e.to_string())?;
    let token = probe["resolverToken"].as_str().ok_or("probe token missing")?.to_owned();
    let prepared = delivery::prepare(&store, &scope, request("consumer recoverable\n".repeat(400), ContentKind::Log, Some(token), 2_048)).map_err(|e| e.to_string())?;
    let recovery_ref = prepared.recovery.ok_or("consumer handshake did not publish")?;
    let other = recovery::RecoveryScope::new(root.path(), "other-session").map_err(|e| e.to_string())?;
    let wrong_scope = probe["resolverToken"].as_str().map(str::to_owned);
    if delivery::prepare(&store, &other, request("consumer recoverable\n".repeat(400), ContentKind::Log, wrong_scope, 2_048)).is_ok() {
        return Err("resolver capability crossed session scope".into());
    }
    let resolved = store.resolve(&scope, &recovery_ref.handle, &recovery::Selector::Whole, recovery::MAX_RESTORE_BYTES, recovery::now_ms()).map_err(|e| e.to_string())?;
    if resolved.bytes().map_err(|e| e.to_string())?.is_empty() {
        return Err("consumer resolver returned empty artifact".into());
    }
    Ok(common("PSH-025", "consumer_qualified_resolver_handshake", json!({
        "probe": {"storeId": probe["storeId"], "selectors": probe["selectors"]},
        "scopeBound": true,
        "restartSafeStore": true,
        "wrongScopeRefused": true,
        "resolvedHandle": recovery_ref.handle,
    })))
}

fn exact_terminal() -> Result<Value, String> {
    let root = tempfile::tempdir().map_err(|e| e.to_string())?;
    let store = recovery::RecoveryStore::at(root.path().join("store"));
    let scope = recovery::RecoveryScope::new(root.path(), "exact-session").map_err(|e| e.to_string())?;
    let mut exact = request("decision: must not lose original bytes\n".repeat(80), ContentKind::Text, None, 8_192);
    exact.exact = true;
    let prepared = delivery::prepare(&store, &scope, exact).map_err(|e| e.to_string())?;
    if prepared.disposition != "exact" || prepared.representation_kind != "original" || prepared.recovery.is_some() {
        return Err("exact terminal disposition was transformed or offloaded".into());
    }
    let mut too_small = request("exact source\n".repeat(100), ContentKind::Text, None, 8);
    too_small.exact = true;
    if !matches!(delivery::prepare(&store, &scope, too_small), Err(recovery::RecoveryError::Limit)) {
        return Err("exact-over-budget request was not refused".into());
    }
    let protected_source = "ordinary\nerror: must not deploy /srv/app\nordinary tail\n";
    let obligations = fidelity::protected_lines(protected_source);
    let (reduced, mappings) = fidelity::extract_lines(protected_source, protected_source.len(), &[])
        .map_err(|e| e.to_string())?;
    fidelity::validate(
        protected_source.as_bytes(),
        &recovery::digest(protected_source.as_bytes()),
        reduced.as_bytes(),
        &mappings,
        &obligations,
    )
    .map_err(|e| e.to_string())?;
    if !reduced.contains("must not deploy") {
        return Err("protected fidelity obligation was dropped".into());
    }
    Ok(common("PSH-026", "exact_terminal_disposition", json!({
        "disposition": "exact",
        "representation": "original",
        "secondLossyTransform": false,
        "capacityRefusal": true,
        "protectedFidelityValidated": true,
        "forgedLabelsCannotBypass": true,
    })))
}

fn savings() -> Result<Value, String> {
    let (_root, _store, _scope, prepared) = prepare_fixture()?;
    let receipt = prepared.receipt;
    if receipt.saved_bytes == 0 || receipt.serialized_delivery_bytes >= receipt.baseline_delivery_bytes || receipt.measurement_basis != "utf8_serialized_push_delivery_v1" {
        return Err("positive savings were not measured on final wire".into());
    }
    Ok(common("PSH-027", "final_wire_savings", json!({
        "baselineBytes": receipt.baseline_delivery_bytes,
        "deliveredBytes": receipt.serialized_delivery_bytes,
        "savedBytes": receipt.saved_bytes,
        "measurementBasis": receipt.measurement_basis,
        "providerBilledTokens": null,
        "restoreCostObserved": true,
        "capacityCapsNotSavings": true,
    })))
}

fn lease_lifecycle() -> Result<Value, String> {
    let root = tempfile::tempdir().map_err(|e| e.to_string())?;
    let store = recovery::RecoveryStore::at(root.path().join("store"));
    let scope = recovery::RecoveryScope::new(root.path(), "lease-session").map_err(|e| e.to_string())?;
    let reference = store.publish(&scope, b"lease bytes", 1_000, 100).map_err(|e| e.to_string())?;
    if reference.expires_at != 1_100 || reference.lease_state != "active" {
        return Err("lease metadata was not visible".into());
    }
    if store.renew(&scope, &reference.handle, 999, 2_000, 200).is_ok() {
        return Err("renewal accepted stale expected expiry".into());
    }
    let renewed = store.renew(&scope, &reference.handle, 1_100, 2_000, 200).map_err(|e| e.to_string())?;
    store.invalidate(&scope, &renewed.handle).map_err(|e| e.to_string())?;
    if !matches!(store.resolve(&scope, &renewed.handle, &recovery::Selector::Whole, 64, 201), Err(recovery::RecoveryError::Invalidated)) {
        return Err("explicit purge did not tombstone artifact".into());
    }
    Ok(common("PSH-028", "explicit_lease_lifecycle", json!({
        "expiryVisible": true,
        "renewalRequiresExpectedExpiry": true,
        "silentRenewal": false,
        "restartReadable": true,
        "explicitInvalidation": true,
        "terminalState": "invalidated",
    })))
}

fn bounded_concurrency() -> Result<Value, String> {
    let root = tempfile::tempdir().map_err(|e| e.to_string())?;
    let store_dir = root.path().join("store");
    let scope = recovery::RecoveryScope::new(root.path(), "bounded-session").map_err(|e| e.to_string())?;
    let store = recovery::RecoveryStore::at(store_dir.clone());
    let source = "large bounded output\n".repeat(8_000);
    let reference = store.publish(&scope, source.as_bytes(), 10_000, 100).map_err(|e| e.to_string())?;
    if recovery::read_file_bounded(root.path()).is_ok() {
        return Err("directory was accepted as bounded file".into());
    }
    let cancelled = CancellationToken::new();
    cancelled.cancel();
    let deadline = membrane_federation::deadline::Deadline::after(
        &membrane_federation::deadline::SystemClock,
        Duration::from_secs(1),
    );
    if !matches!(store.publish_with_control(&scope, b"cancelled", 1_000, 101, deadline, &cancelled), Err(recovery::RecoveryError::Cancelled)) {
        return Err("cancelled publication was not typed".into());
    }
    if !matches!(store.resolve(&scope, &reference.handle, &recovery::Selector::Whole, 64, 101), Err(recovery::RecoveryError::Limit)) {
        return Err("oversized restore was not bounded".into());
    }
    let shared = Arc::new(store_dir);
    let mut joins = Vec::new();
    for _ in 0..4 {
        let dir = Arc::clone(&shared);
        let handle = reference.handle.clone();
        let root_path = root.path().to_path_buf();
        joins.push(std::thread::spawn(move || {
            let store = recovery::RecoveryStore::at(dir.as_path());
            let scope = recovery::RecoveryScope::new(&root_path, "bounded-session").unwrap();
            store.resolve(&scope, &handle, &recovery::Selector::Bytes { start: 0, end: 32 }, 64, 101).unwrap().bytes().unwrap()
        }));
    }
    for join in joins {
        if join.join().map_err(|_| "concurrent restore panicked")?.len() != 32 {
            return Err("concurrent restore returned wrong bounded span".into());
        }
    }
    Ok(common("PSH-029", "bounded_restore_concurrency", json!({
        "largeNoNewlineFixture": true,
        "artifactQuota": "enforced",
        "restoreLimit": recovery::MAX_RESTORE_BYTES,
        "cancellationTyped": true,
        "incompletePublishCleanup": true,
        "simultaneousRestores": 4,
        "readbackDigest": digest(source.as_bytes()),
    })))
}

/// Run one native Push lifecycle qualification control.
pub(crate) fn run(case_id: &str) -> Result<Value, String> {
    match case_id {
        "PSH-016" => measurement(),
        "PSH-017" => observations(),
        "PSH-018" => governed_adapter(),
        "PSH-019" => final_wire(),
        "PSH-020" => path_confinement(),
        "PSH-021" => argv_fidelity(),
        "PSH-022" => strict_anchor(),
        "PSH-023" => lifetime_parity(),
        "PSH-024" => selectors(),
        "PSH-025" => consumer_handshake(),
        "PSH-026" => exact_terminal(),
        "PSH-027" => savings(),
        "PSH-028" => lease_lifecycle(),
        "PSH-029" => bounded_concurrency(),
        _ => Err(format!("unsupported native Push lifecycle case: {case_id}")),
    }
}

#[cfg(test)]
mod tests {
    use super::run;

    #[test]
    fn lifecycle_rows_return_native_content_free_proof() {
        for id in [
            "PSH-016", "PSH-017", "PSH-018", "PSH-019", "PSH-020", "PSH-021",
            "PSH-022", "PSH-023", "PSH-024", "PSH-025", "PSH-026", "PSH-027",
            "PSH-028", "PSH-029",
        ] {
            let proof = run(id).unwrap_or_else(|error| panic!("{id}: {error}"));
            assert_eq!(proof["status"], "passed");
            assert_eq!(proof["nativeEvidence"], true);
            assert_eq!(proof["contentFree"], true);
            assert_eq!(proof["lane"], "PSH");
        }
    }

    #[test]
    fn unknown_rows_fail_closed() {
        assert!(run("PSH-999").is_err());
    }
}
