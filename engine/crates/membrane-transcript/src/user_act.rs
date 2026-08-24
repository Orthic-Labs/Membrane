//! Host user-act adapters & explicit capability reporting.
//!
//! Transcript text can support explicit preferences/corrections. UI acts such
//! as accept, reject, post-accept edit, or named choice are admitted only from
//! an authenticated `adapt_user_act_v1` row emitted by host integration.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::adapters::GENERIC_HOSTS;
use crate::evidence::{ActKind, EvidenceError, UserActEvidenceV1};

pub const USER_ACT_ADAPTER_VERSION: &str = "membrane.user-act-adapter.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityState {
    Supported,
    TranscriptOnly,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostActCapabilityReportV1 {
    pub schema_version: String,
    pub host: String,
    pub capabilities: BTreeMap<ActKind, CapabilityState>,
    pub authenticated_signal_rows: u32,
    pub omissions: Vec<String>,
}

fn known_host(host: &str) -> bool {
    matches!(host, "claude_code" | "codex") || GENERIC_HOSTS.contains(&host)
}

fn parse_kind(value: &str) -> Option<ActKind> {
    match value {
        "explicit_preference" => Some(ActKind::ExplicitPreference),
        "correction" => Some(ActKind::Correction),
        "reject" => Some(ActKind::Reject),
        "accept" => Some(ActKind::Accept),
        "post_accept_edit" => Some(ActKind::PostAcceptEdit),
        "repeated_edit" => Some(ActKind::RepeatedEdit),
        "named_choice" => Some(ActKind::NamedChoice),
        _ => None,
    }
}

/// Parse one host-emitted, authenticated user-act row. Generic transcript
/// messages cannot fabricate these rows; provenance receipt & actor binding
/// are mandatory.
pub fn parse_user_act_row(row: &Value) -> Result<Option<UserActEvidenceV1>, EvidenceError> {
    if row.get("type").and_then(Value::as_str) != Some("adapt_user_act_v1") {
        return Ok(None);
    }
    let host = row.get("host").and_then(Value::as_str).unwrap_or("");
    if !known_host(host) || row.get("actor").and_then(Value::as_str) != Some("authenticated_user") {
        return Err(EvidenceError::MissingProvenanceReceipt);
    }
    let act_kind = row
        .get("act_kind")
        .and_then(Value::as_str)
        .and_then(parse_kind)
        .ok_or(EvidenceError::MissingProvenanceReceipt)?;
    let strings = |name: &str| -> Vec<String> {
        row.get(name)
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default()
    };
    let scope_context: BTreeMap<String, String> = row
        .get("scope_context")
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
        .unwrap_or_default();
    let mut evidence = UserActEvidenceV1::new(
        row.get("evidence_id").and_then(Value::as_str).unwrap_or(""),
        row.get("installation_id")
            .and_then(Value::as_str)
            .unwrap_or(""),
        host,
        row.get("session_id").and_then(Value::as_str).unwrap_or(""),
        strings("event_ids"),
        act_kind,
        None,
        scope_context,
        row.get("timestamp").and_then(Value::as_str).unwrap_or(""),
        row.get("provenance_receipt")
            .and_then(Value::as_str)
            .unwrap_or(""),
    )?;
    if row.get("before_excerpt").is_some() || row.get("after_excerpt").is_some() {
        evidence.set_counterfactual(
            row.get("before_excerpt").and_then(Value::as_str),
            row.get("after_excerpt").and_then(Value::as_str),
        )?;
    }
    Ok(Some(evidence))
}

/// Report capabilities from actual source rows. Explicit preference &
/// correction remain transcript-derived; UI acts become supported only when
/// authenticated host rows are present, otherwise capability is unavailable.
pub fn capability_report(host: &str, rows: &[Value]) -> HostActCapabilityReportV1 {
    let mut observed = BTreeSet::new();
    let mut count = 0u32;
    for row in rows {
        if row.get("host").and_then(Value::as_str) == Some(host)
            && row.get("type").and_then(Value::as_str) == Some("adapt_user_act_v1")
            && row.get("actor").and_then(Value::as_str) == Some("authenticated_user")
        {
            if let Some(kind) = row
                .get("act_kind")
                .and_then(Value::as_str)
                .and_then(parse_kind)
            {
                observed.insert(kind);
                count += 1;
            }
        }
    }
    let mut capabilities = BTreeMap::new();
    for kind in [
        ActKind::ExplicitPreference,
        ActKind::Correction,
        ActKind::Reject,
        ActKind::Accept,
        ActKind::PostAcceptEdit,
        ActKind::RepeatedEdit,
        ActKind::NamedChoice,
    ] {
        let state = if observed.contains(&kind) {
            CapabilityState::Supported
        } else if matches!(kind, ActKind::ExplicitPreference | ActKind::Correction)
            && known_host(host)
        {
            CapabilityState::TranscriptOnly
        } else {
            CapabilityState::Unavailable
        };
        capabilities.insert(kind, state);
    }
    let omissions = capabilities
        .iter()
        .filter(|(_, state)| **state == CapabilityState::Unavailable)
        .map(|(kind, _)| format!("{kind:?}:host_signal_unavailable"))
        .collect();
    HostActCapabilityReportV1 {
        schema_version: USER_ACT_ADAPTER_VERSION.into(),
        host: host.to_string(),
        capabilities,
        authenticated_signal_rows: count,
        omissions,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unavailable_ui_signals_are_reported_honestly() {
        let report = capability_report("codex", &[]);
        assert_eq!(
            report.capabilities[&ActKind::Accept],
            CapabilityState::Unavailable
        );
        assert_eq!(
            report.capabilities[&ActKind::Correction],
            CapabilityState::TranscriptOnly
        );
    }

    #[test]
    fn authenticated_rows_preserve_counterfactuals() {
        let row = serde_json::json!({
            "type": "adapt_user_act_v1",
            "host": "opencode",
            "actor": "authenticated_user",
            "evidence_id": "uae_1",
            "installation_id": "inst",
            "session_id": "s",
            "event_ids": ["e1", "e2"],
            "act_kind": "post_accept_edit",
            "before_excerpt": "large rewrite",
            "after_excerpt": "focused patch",
            "scope_context": {"repo": "membrane"},
            "timestamp": "2026-08-24T00:00:00Z",
            "provenance_receipt": "sha256:receipt"
        });
        let evidence = parse_user_act_row(&row).unwrap().unwrap();
        assert_eq!(evidence.act_kind, ActKind::PostAcceptEdit);
        assert_eq!(evidence.after_excerpt.as_deref(), Some("focused patch"));
        let report = capability_report("opencode", &[row]);
        assert_eq!(
            report.capabilities[&ActKind::PostAcceptEdit],
            CapabilityState::Supported
        );
    }
}
