//! Request-time Push plan construction & host-capacity selection.
//!
//! Pull owns admission. This module only turns the finalized planner packet
//! into a bounded, content-free reduction ladder, then applies the host's
//! validated H8 ceiling without inventing capacity.

use crate::push::prep::{is_code_ext, is_structured_text, PushPolicy};
use crate::push::{compress, skel, telemetry};
use cortex_core::planner::{BlockV1, ContextPacketV1};
use membrane_protocol::host_observation::{
    ObservationCoverageV1, ObservationUnavailableReasonV1, RemainingContextCeilingV1,
};
use membrane_protocol::push::{
    PacketReductionPlanError, PacketReductionPlanV1, PacketReductionRepresentationV1,
    PacketReductionSelectionError, PacketReductionSelectionReceiptV1,
    PACKET_REDUCTION_SELECTION_RECEIPT_SCHEMA_VERSION,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;
use thiserror::Error;

/// Typed refusal at the request boundary. No missing or non-exact H8 value is
/// converted into a numeric fallback.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RequestTimeH8Error {
    #[error("remainingContextCeiling is required on every context request")]
    Missing,
    #[error("remainingContextCeiling is invalid: {0}")]
    Invalid(String),
    #[error("request-time H8 {field} does not match request identity: expected={expected}, observed={observed}")]
    IdentityMismatch {
        field: &'static str,
        expected: String,
        observed: String,
    },
    #[error("request-time H8 is not exact: coverage={coverage:?}, reason={reason:?}")]
    Inexact {
        coverage: ObservationCoverageV1,
        reason: Option<ObservationUnavailableReasonV1>,
    },
}

/// Typed refusal while constructing or selecting a request-time plan.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PacketReductionRequestError {
    #[error("request-time H8 refused: {0}")]
    H8(#[source] RequestTimeH8Error),
    #[error("packet reduction plan cannot be built: packet {0} is empty")]
    EmptyPacket(&'static str),
    #[error("packet reduction plan cannot be built: block {block} has empty {field}")]
    EmptyBlockField { block: String, field: &'static str },
    #[error("packet reduction plan cannot be built: no protected material")]
    NoProtectedMaterial,
    #[error("packet reduction plan cannot be built: protected floor exceeds full packet")]
    NoViableFloor,
    #[error("packet reduction plan is invalid: {0}")]
    InvalidPlan(#[source] PacketReductionPlanError),
    #[error("packet reduction content serialization failed: {0}")]
    ContentSerialization(String),
    #[error("packet reduction selection refused: {0}")]
    Selection(#[source] PacketReductionSelectionError),
}

impl PacketReductionRequestError {
    /// Stable response kind for route consumers. Detail remains in `reason`.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::H8(RequestTimeH8Error::Missing)
            | Self::H8(RequestTimeH8Error::Inexact { .. }) => "h8_unavailable",
            Self::H8(RequestTimeH8Error::IdentityMismatch { .. }) => "h8_identity_mismatch",
            Self::H8(RequestTimeH8Error::Invalid(_)) => "h8_invalid",
            Self::EmptyPacket(_)
            | Self::EmptyBlockField { .. }
            | Self::NoProtectedMaterial
            | Self::NoViableFloor
            | Self::InvalidPlan(_)
            | Self::ContentSerialization(_)
            | Self::Selection(_) => "packet_reduction_refused",
        }
    }
}

/// Wire result returned only after plan validation & same-request selection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PacketReductionSelectionV1 {
    pub plan: PacketReductionPlanV1,
    pub selected_representation: PacketReductionRepresentationV1,
    pub selection_receipt: PacketReductionSelectionReceiptV1,
}

/// Parse, validate & bind the H8 row carried on one `/federate` request.
pub fn parse_request_time_h8(
    body: &Value,
    expected_session_id: &str,
    expected_task: &str,
) -> Result<RemainingContextCeilingV1, RequestTimeH8Error> {
    let raw = body
        .get("remainingContextCeiling")
        .ok_or(RequestTimeH8Error::Missing)?;
    let ceiling: RemainingContextCeilingV1 = serde_json::from_value(raw.clone())
        .map_err(|error| RequestTimeH8Error::Invalid(error.to_string()))?;
    ceiling
        .validate()
        .map_err(|error| RequestTimeH8Error::Invalid(error.to_string()))?;

    if ceiling.session_id != expected_session_id {
        return Err(RequestTimeH8Error::IdentityMismatch {
            field: "sessionId",
            expected: expected_session_id.to_owned(),
            observed: ceiling.session_id.clone(),
        });
    }

    if ceiling.requested_at_unix_ms == 0 || ceiling.provenance_receipt.observed_at_unix_ms == 0 {
        return Err(RequestTimeH8Error::Invalid(
            "request and provenance timestamps must be non-zero".to_owned(),
        ));
    }

    if ceiling.task_id.coverage != ObservationCoverageV1::Complete {
        return Err(RequestTimeH8Error::Inexact {
            coverage: ceiling.task_id.coverage,
            reason: ceiling.task_id.unavailable_reason,
        });
    }
    let observed_task = ceiling.task_id.value.as_deref().ok_or_else(|| {
        RequestTimeH8Error::Inexact {
            coverage: ceiling.task_id.coverage,
            reason: ceiling.task_id.unavailable_reason,
        }
    })?;
    if observed_task.trim().is_empty() {
        return Err(RequestTimeH8Error::Invalid(
            "taskId value must not be empty".to_owned(),
        ));
    }
    if observed_task != expected_task {
        return Err(RequestTimeH8Error::IdentityMismatch {
            field: "taskId",
            expected: expected_task.to_owned(),
            observed: observed_task.to_owned(),
        });
    }

    let estimate = &ceiling.remaining_tokens.estimate;
    if estimate.coverage != ObservationCoverageV1::Complete || estimate.value.is_none() {
        return Err(RequestTimeH8Error::Inexact {
            coverage: estimate.coverage,
            reason: estimate.unavailable_reason,
        });
    }

    Ok(ceiling)
}

pub use super::packet_selection::*;

#[cfg(test)]
mod tests {
    use super::*;
    use cortex_core::planner::{BudgetV1, ContextPacketV1};
    use membrane_protocol::host_observation::{
        EstimatorBasisV1, HostObservationProvenanceV1, ObservedFieldV1, TokenEstimateV1,
        REMAINING_CONTEXT_CEILING_SCHEMA_VERSION,
    };

    fn block(id: &str, protected: bool, selected_tokens: usize) -> BlockV1 {
        BlockV1 {
            id: id.to_owned(),
            layer: 1,
            provider: "test".to_owned(),
            source_kind: "file".to_owned(),
            source_ref: format!("source://{id}"),
            source_hash: format!("sha256:{:0<64}", id),
            trust_class: "trusted".to_owned(),
            instruction_policy: "data".to_owned(),
            base_commit: None,
            overlay_digest: None,
            freshness_class: None,
            snapshot_id: None,
            priority: 1,
            estimated_tokens: selected_tokens,
            delivery_stage: None,
            delivery_class: None,
            selected_tokens: Some(selected_tokens),
            allotted_tokens: Some(selected_tokens.saturating_sub(8)),
            rendered_tokens: None,
            delivered_chars: None,
            drop_reason: None,
            protected,
            recoverable: true,
            resolver: format!("resolver://{id}"),
            text: format!("text for {id}"),
        }
    }

    fn packet() -> ContextPacketV1 {
        ContextPacketV1 {
            schema_version: 1,
            trace_id: "trace-1".to_owned(),
            task: "task-1".to_owned(),
            mode: "test".to_owned(),
            budget: BudgetV1 {
                max_tokens: 128,
                admitted_tokens: 128,
                packet_char_budget_default: None,
                packet_char_budget_override: None,
                packet_char_budget_model: None,
                configured_packet_char_budget: None,
                effective_packet_char_budget: None,
            },
            allocations: std::collections::BTreeMap::new(),
            provider_accounting: std::collections::BTreeMap::new(),
            blocks: vec![block("protected", true, 32), block("ordinary", false, 96)],
            omissions: Vec::new(),
        }
    }

    fn ceiling() -> RemainingContextCeilingV1 {
        RemainingContextCeilingV1 {
            schema_version: REMAINING_CONTEXT_CEILING_SCHEMA_VERSION,
            ceiling_id: "ceiling-1".to_owned(),
            session_id: "session-1".to_owned(),
            task_id: ObservedFieldV1::complete("task-1".to_owned()),
            requested_at_unix_ms: 1_700_000_000_000,
            remaining_tokens: TokenEstimateV1::complete(
                EstimatorBasisV1::new("o200k_base", "1"),
                100_000,
            ),
            provenance_receipt: HostObservationProvenanceV1::new(
                "receipt-1",
                "test-host",
                1_700_000_000_000,
                "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            ),
        }
    }

    fn prepared(packet: &ContextPacketV1, policy: &PushPolicy) -> PacketReductionSelectionV1 {
        let temp = tempfile::tempdir().unwrap();
        let store = super::super::recovery::RecoveryStore::at(temp.path());
        let scope = super::super::recovery::RecoveryScope::new(temp.path(), "session-1").unwrap();
        let proof = super::super::delivery::resolver_probe(&store, &scope).unwrap();
        let owner = RecoveryContext {store:&store,scope:&scope,resolver_token:proof["resolverToken"].as_str().unwrap()};
        select_packet_for_h8_with_recovery(packet, &ceiling(), policy, Some(&owner)).unwrap()
    }

    #[test]
    fn builds_ladder_and_selects_largest_fitting_representation() {
        let result = select_packet_for_h8(&packet(), &ceiling()).unwrap();
        assert_eq!(result.plan.representations.len(), 3);
        assert_eq!(result.plan.representations[0].id, "full");
        assert_eq!(result.plan.representations[1].id, "reduced_1");
        assert_eq!(result.plan.representations[2].id, "floor");
        assert_eq!(result.selected_representation.id, "full");
        assert_eq!(result.selection_receipt.decision, "selected");
    }

    /// A Rust source block big enough that budget-bounded skeletonization
    /// (which drops function bodies) actually shrinks it relative to raw
    /// compression, so the two transforms are distinguishable in a test.
    fn code_block(id: &str, protected: bool) -> BlockV1 {
        let mut text = String::new();
        for n in 0..40 {
            text.push_str(&format!(
                "fn function_{n}(value: i32) -> i32 {{\n    let doubled = value * 2;\n    doubled + {n}\n}}\n\n"
            ));
        }
        let selected_tokens = compress::estimate_tokens(&text);
        BlockV1 {
            id: id.to_owned(),
            layer: 1,
            provider: "test".to_owned(),
            source_kind: "file".to_owned(),
            source_ref: format!("crates/example/src/{id}.rs"),
            source_hash: format!("sha256:{:0<64}", id),
            trust_class: "trusted".to_owned(),
            instruction_policy: "data".to_owned(),
            base_commit: None,
            overlay_digest: None,
            freshness_class: None,
            snapshot_id: None,
            priority: 1,
            estimated_tokens: selected_tokens,
            delivery_stage: None,
            delivery_class: None,
            selected_tokens: Some(selected_tokens),
            // Budget must clear the plain tree-sitter *signature* tier (no
            // bodies) but stay well under the full source, so this test
            // distinguishes skeletonization from raw compression rather than
            // accidentally degrading further to the unsupported-language
            // path-stub tier.
            allotted_tokens: Some(
                compress::estimate_tokens(&skel::skeletonize(
                    Path::new(&format!("crates/example/src/{id}.rs")),
                    &text,
                )) + 16,
            ),
            rendered_tokens: None,
            delivered_chars: None,
            drop_reason: None,
            protected,
            recoverable: true,
            resolver: format!("resolver://{id}"),
            text,
        }
    }

    #[test]
    fn code_block_in_reduced_1_is_skeletonized_not_raw_compressed() {
        let mut packet = packet();
        packet.blocks = vec![block("protected", true, 32), code_block("ordinary", false)];
        let result = prepared(&packet, &PushPolicy::Control);
        let reduced = &result.plan.representations[1];
        let reduced_blocks: Vec<cortex_core::planner::BlockV1> =
            serde_json::from_value(reduced.content["blocks"].clone()).unwrap();
        let ordinary = reduced_blocks
            .iter()
            .find(|b| b.id == "ordinary")
            .expect("ordinary block present");
        // Skeletonization drops function bodies (tree-sitter signature
        // extraction); a plain LLMLingua-style compress pass does not
        // recognize Rust syntax and would not remove every body wholesale.
        assert!(
            !ordinary.text.contains("let doubled"),
            "expected skeletonization to elide function bodies, got: {}",
            ordinary.text
        );
        assert!(
            ordinary.text.contains("fn function_0"),
            "expected skeletonization to keep signatures, got: {}",
            ordinary.text
        );
    }

    #[test]
    fn protected_spans_survive_reduced_1_with_new_dispatch() {
        let mut packet = packet();
        let protected_text = block("protected", true, 32).text;
        packet.blocks = vec![block("protected", true, 32), code_block("ordinary", false)];
        let result = prepared(&packet, &PushPolicy::Control);
        let reduced_blocks: Vec<cortex_core::planner::BlockV1> =
            serde_json::from_value(result.plan.representations[1].content["blocks"].clone())
                .unwrap();
        let floor_blocks: Vec<cortex_core::planner::BlockV1> =
            serde_json::from_value(result.plan.representations[2].content["blocks"].clone())
                .unwrap();
        for blocks in [&reduced_blocks, &floor_blocks] {
            let protected_block = blocks
                .iter()
                .find(|b| b.id == "protected")
                .expect("protected block must survive every representation");
            assert_eq!(protected_block.text, protected_text);
        }
    }

    #[test]
    fn production_selection_emits_telemetry_unconditionally() {
        let temp = tempfile::tempdir().unwrap();
        let telemetry_path = temp.path().join("push-telemetry.jsonl");
        std::env::set_var("MEMBRANE_PUSH_TELEMETRY_PATH", &telemetry_path);
        let mut packet = packet();
        packet.blocks = vec![block("protected", true, 32), code_block("ordinary", false)];
        let _ = prepared(&packet, &PushPolicy::Control);
        std::env::remove_var("MEMBRANE_PUSH_TELEMETRY_PATH");
        let contents = std::fs::read_to_string(&telemetry_path)
            .expect("production selection must emit telemetry unconditionally");
        assert!(
            contents.lines().any(|line| line.contains("\"axis\":\"push\"")),
            "expected at least one content-free push observation, got: {contents}"
        );
    }

    #[test]
    fn query_aware_policy_reaches_reduced_1_dispatch() {
        let mut packet = packet();
        packet.blocks = vec![
            block("protected", true, 32),
            code_block("ordinary", false),
        ];
        let control = prepared(&packet, &PushPolicy::Control);
        let query_aware = select_packet_for_h8_with_policy(
            &packet,
            &ceiling(),
            &PushPolicy::query_aware("function_0", true, true),
        )
        .unwrap();
        // Both must still satisfy plan invariants; policy selection is
        // reachable and content-free (no query text leaks into the receipt).
        assert_eq!(control.plan.representations.len(), 3);
        assert_eq!(query_aware.plan.representations.len(), 3);
        let receipt_json = serde_json::to_string(&query_aware.selection_receipt).unwrap();
        assert!(!receipt_json.contains("function_0"));
    }

    #[test]
    fn parser_binds_session_and_task_identity() {
        let value = serde_json::json!({
            "remainingContextCeiling": ceiling(),
        });
        assert!(parse_request_time_h8(&value, "session-1", "task-1").is_ok());
        assert!(matches!(
            parse_request_time_h8(&value, "other", "task-1"),
            Err(RequestTimeH8Error::IdentityMismatch {
                field: "sessionId",
                ..
            })
        ));
    }
}
