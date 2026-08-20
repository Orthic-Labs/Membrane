//! `membrane cli pull plan-context` CLI handler.
//!
//! Reads a ContextCandidateSet v1 (from --candidate-set path or stdin when the
//! path is "-"), invokes Pull's deterministic pure-in-process planner, and
//! emits a JSON payload to stdout with:
//!   - `packet`: the bounded ContextPacket v1
//!   - `receipts`: content-free ContextReceipt v2 list
//!   - `providerStatus`, `fallbackMode`, `degradationReason`, `sourceGeneration`
//!   - `structured_event`: a `context_fallback` event when degraded
//!
//! Errors are emitted as a JSON `{"error": "...", "kind": "planner_error"}`
//! payload so callers (test harnesses, the orchestrator) can distinguish
//! planner-rejected input from CLI misuse. Exit code is non-zero on error.

use crate::pull::planner::{
    plan, ContextCandidateSetV1, PlannerError, PlannerInput, PlannerOutput,
};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

pub fn run(
    candidate_set: PathBuf,
    max_tokens: usize,
    packet_char_budget_override: Option<usize>,
    packet_char_budget_model: Option<String>,
    accepted_receipt_versions: Vec<u32>,
) -> Result<(), String> {
    let raw = read_input(&candidate_set).map_err(|e| format!("read input: {e}"))?;
    let ccs: ContextCandidateSetV1 = serde_json::from_str(&raw)
        .map_err(|e| format!("candidate_set is not a ContextCandidateSet v1: {e}"))?;
    let versions = if accepted_receipt_versions.is_empty() {
        vec![2]
    } else {
        accepted_receipt_versions
    };
    let input = PlannerInput {
        candidate_set: ccs,
        max_tokens,
        packet_char_budget_override,
        packet_char_budget_model,
        accepted_receipt_versions: versions,
        trace_id_override: None,
        scope_grant_present: false,
    };
    match plan(&input) {
        Ok(out) => print_output(&out),
        Err(err) => print_error(&err),
    }
}

fn read_input(path: &Path) -> io::Result<String> {
    if path.as_os_str() == "-" {
        let mut buf = String::new();
        io::stdin().read_to_string(&mut buf)?;
        return Ok(buf);
    }
    std::fs::read_to_string(path)
}

fn print_output(out: &PlannerOutput) -> Result<(), String> {
    let payload = serde_json::json!({
        "packet": out.packet,
        "receipts": out.receipts,
        "providerStatus": out.provider_status,
        "fallbackMode": out.fallback_mode,
        "degradationReason": out.degradation_reason,
        "sourceGeneration": out.source_generation,
        "structuredEvent": out.structured_event,
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&payload).map_err(|e| format!("serialize: {e}"))?
    );
    Ok(())
}

fn print_error(err: &PlannerError) -> Result<(), String> {
    let kind = match err {
        PlannerError::UnknownSchemaVersion { .. } => "unknown_schema_version",
        PlannerError::EmptyTask => "empty_task",
        PlannerError::EmptyProvider => "empty_provider",
        PlannerError::ReceiptVersionUnsupported { .. } => "receipt_version_unsupported",
        PlannerError::ZeroBudget => "zero_budget",
    };
    let payload = serde_json::json!({
        "error": err.to_string(),
        "kind": kind,
    });
    println!(
        "{}",
        serde_json::to_string(&payload).map_err(|e| format!("serialize: {e}"))?
    );
    Ok(())
}
