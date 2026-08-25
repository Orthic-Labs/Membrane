//! `UserActEvidenceV1` — the first-class user-act evidence contract.
//!
//! Evidence objects record *what happened*; they never state the final
//! preference. Authority comes only from qualifying user acts; behavioral
//! signals may support a hypothesis but cannot self-authorize.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::canonical::sha256_hex;

/// Internal domain-contract schema tag. This is an Adapt-internal V1 schema,
/// not one of Membrane's five public protocol shapes.
pub const USER_ACT_EVIDENCE_SCHEMA: &str = "adapt.user-act-evidence.v2";
pub const USER_ACT_RECEIPT_CONTRACT: &str = "adapt.user-act-receipt.v1";

/// Kinds of user acts that may carry learning signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActKind {
    ExplicitPreference,
    Correction,
    Reject,
    Accept,
    PostAcceptEdit,
    RepeatedEdit,
    NamedChoice,
}

impl ActKind {
    /// Default weighting-ladder strength. Strength affects confidence and
    /// review policy only — it can NEVER promote a source into a higher
    /// authority class.
    pub fn default_signal_strength(self) -> f64 {
        match self {
            ActKind::ExplicitPreference => 1.00,
            ActKind::Correction => 0.95,
            ActKind::PostAcceptEdit | ActKind::RepeatedEdit => 0.85,
            ActKind::NamedChoice => 0.75,
            ActKind::Reject => 0.65,
            ActKind::Accept => 0.20,
        }
    }

    /// Only these kinds are user-authoritative when they originate from an
    /// authenticated external-user turn.
    pub fn is_user_authoritative_kind(self) -> bool {
        matches!(
            self,
            ActKind::ExplicitPreference
                | ActKind::Correction
                | ActKind::Reject
                | ActKind::NamedChoice
        )
    }
}

/// Evidence classes from the canonical provenance model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceClass {
    UserAuthoritative,
    UserBehavioral,
    Diagnostic,
    ContextOnly,
}

/// Errors that make an evidence object inadmissible.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EvidenceError {
    EmptyExcerpt,
    InvalidStrength,
    InvalidSpanRange,
    SpanDigestMismatch { expected: String, found: String },
    SilentAcceptanceAlone,
    MissingProvenanceReceipt,
    InvalidProvenanceReceipt,
    UntrustedIssuer,
    InstallationMismatch,
    HostMismatch,
    ReceiptBindingMismatch,
    ReceiptForgery,
    ReceiptReplay,
}

impl std::fmt::Display for EvidenceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EvidenceError::EmptyExcerpt => write!(f, "empty excerpt"),
            EvidenceError::InvalidStrength => write!(f, "signal strength must be in 0..=1"),
            EvidenceError::InvalidSpanRange => write!(f, "invalid span range"),
            EvidenceError::SpanDigestMismatch { expected, found } => {
                write!(
                    f,
                    "span digest mismatch: expected {expected}, found {found}"
                )
            }
            EvidenceError::SilentAcceptanceAlone => {
                write!(f, "silent acceptance cannot authorize Taste alone")
            }
            EvidenceError::MissingProvenanceReceipt => write!(f, "missing provenance receipt"),
            EvidenceError::InvalidProvenanceReceipt => write!(f, "invalid provenance receipt"),
            EvidenceError::UntrustedIssuer => write!(f, "untrusted user-act issuer"),
            EvidenceError::InstallationMismatch => write!(f, "user-act installation mismatch"),
            EvidenceError::HostMismatch => write!(f, "user-act host mismatch"),
            EvidenceError::ReceiptBindingMismatch => write!(f, "user-act receipt binding mismatch"),
            EvidenceError::ReceiptForgery => write!(f, "user-act receipt signature invalid"),
            EvidenceError::ReceiptReplay => write!(f, "user-act receipt replayed"),
        }
    }
}

/// Host-issued proof over every authority-bearing user-act field. The host
/// signs the canonical [`UserActReceiptPayloadV1`] bytes; callers cannot make
/// a row authoritative by inserting an actor label or arbitrary digest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserActProvenanceReceiptV1 {
    pub contract_version: String,
    pub issuer_id: String,
    pub key_id: String,
    pub installation_id: String,
    pub host: String,
    pub session_id: String,
    pub sequence: u64,
    pub nonce: String,
    pub payload_sha256: String,
    pub signature_hex: String,
}

/// Exact signed material. Struct field order is the versioned canonical wire
/// order for this internal contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserActReceiptPayloadV1 {
    pub contract_version: String,
    pub issuer_id: String,
    pub key_id: String,
    pub evidence_id: String,
    pub installation_id: String,
    pub host: String,
    pub session_id: String,
    pub event_ids: Vec<String>,
    pub act_kind: ActKind,
    pub before_digest: Option<String>,
    pub after_digest: Option<String>,
    pub user_source_span: Option<SourceSpan>,
    pub scope_context: BTreeMap<String, String>,
    pub timestamp: String,
    pub sequence: u64,
    pub nonce: String,
}

impl std::error::Error for EvidenceError {}

/// A byte-span binding back to exact transcript/source bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceSpan {
    pub event_id: String,
    pub session_id: String,
    pub byte_start: i64,
    pub byte_end: i64,
    /// SHA-256 of the exact source bytes this span covers.
    pub bytes_sha256: String,
}

impl SourceSpan {
    pub fn new(
        event_id: &str,
        session_id: &str,
        byte_start: i64,
        byte_end: i64,
        source_bytes: &[u8],
    ) -> Self {
        Self {
            event_id: event_id.to_string(),
            session_id: session_id.to_string(),
            byte_start,
            byte_end,
            bytes_sha256: sha256_hex(source_bytes),
        }
    }
}

/// `UserActEvidenceV1`: records a single user act with provenance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UserActEvidenceV1 {
    pub schema_version: String,
    pub evidence_id: String,
    pub installation_id: String,
    pub host: String,
    pub session_id: String,
    pub event_ids: Vec<String>,
    pub act_kind: ActKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before_digest: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before_excerpt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after_digest: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after_excerpt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_source_span: Option<SourceSpan>,
    /// Structured scope context observed at act time (repo, language, ...).
    pub scope_context: BTreeMap<String, String>,
    /// ISO-8601 timestamp of the act.
    pub timestamp: String,
    /// Weighting strength in 0..=1.
    pub signal_strength: f64,
    /// Authenticated host receipt binding this evidence to its source.
    pub provenance_receipt: UserActProvenanceReceiptV1,
}

impl UserActEvidenceV1 {
    /// Construct and validate a new evidence object.
    ///
    /// Fail-closed rules: a declared span must be ordered; silent acceptance
    /// alone carries no Taste authority; and a provenance receipt digest is
    /// mandatory.
    pub fn new(
        evidence_id: &str,
        installation_id: &str,
        host: &str,
        session_id: &str,
        event_ids: Vec<String>,
        act_kind: ActKind,
        user_source_span: Option<SourceSpan>,
        scope_context: BTreeMap<String, String>,
        timestamp: &str,
        provenance_receipt: UserActProvenanceReceiptV1,
    ) -> Result<Self, EvidenceError> {
        if provenance_receipt.contract_version != USER_ACT_RECEIPT_CONTRACT
            || provenance_receipt.issuer_id.trim().is_empty()
            || provenance_receipt.key_id.trim().is_empty()
            || provenance_receipt.installation_id.trim().is_empty()
            || provenance_receipt.host.trim().is_empty()
            || provenance_receipt.session_id.trim().is_empty()
            || provenance_receipt.sequence == 0
            || provenance_receipt.nonce.trim().is_empty()
            || provenance_receipt.payload_sha256.len() != 64
            || provenance_receipt.signature_hex.len() != 128
        {
            return Err(EvidenceError::MissingProvenanceReceipt);
        }
        if let Some(span) = &user_source_span {
            if span.byte_end < span.byte_start {
                return Err(EvidenceError::InvalidSpanRange);
            }
        }
        Ok(Self {
            schema_version: USER_ACT_EVIDENCE_SCHEMA.to_string(),
            evidence_id: evidence_id.to_string(),
            installation_id: installation_id.to_string(),
            host: host.to_string(),
            session_id: session_id.to_string(),
            event_ids,
            act_kind,
            before_digest: None,
            before_excerpt: None,
            after_digest: None,
            after_excerpt: None,
            user_source_span,
            scope_context,
            timestamp: timestamp.to_string(),
            signal_strength: act_kind.default_signal_strength(),
            provenance_receipt,
        })
    }

    pub fn receipt_payload(&self) -> UserActReceiptPayloadV1 {
        let mut event_ids = self.event_ids.clone();
        event_ids.sort();
        event_ids.dedup();
        UserActReceiptPayloadV1 {
            contract_version: self.provenance_receipt.contract_version.clone(),
            issuer_id: self.provenance_receipt.issuer_id.clone(),
            key_id: self.provenance_receipt.key_id.clone(),
            evidence_id: self.evidence_id.clone(),
            installation_id: self.installation_id.clone(),
            host: self.host.clone(),
            session_id: self.session_id.clone(),
            event_ids,
            act_kind: self.act_kind,
            before_digest: self.before_digest.clone(),
            after_digest: self.after_digest.clone(),
            user_source_span: self.user_source_span.clone(),
            scope_context: self.scope_context.clone(),
            timestamp: self.timestamp.clone(),
            sequence: self.provenance_receipt.sequence,
            nonce: self.provenance_receipt.nonce.clone(),
        }
    }

    /// Attach before/after content for edit/reject-style counterfactuals.
    /// Both excerpt and digest must agree; digests are computed here.
    pub fn set_counterfactual(
        &mut self,
        before_excerpt: Option<&str>,
        after_excerpt: Option<&str>,
    ) -> Result<(), EvidenceError> {
        let bind = |excerpt: Option<&str>| -> Option<(String, String)> {
            excerpt.map(|e| {
                let trimmed = e.trim();
                (
                    trimmed.chars().take(400).collect::<String>(),
                    sha256_hex(trimmed.as_bytes()),
                )
            })
        };
        match (bind(before_excerpt), bind(after_excerpt)) {
            (None, None) => Err(EvidenceError::EmptyExcerpt),
            (b, a) => {
                self.before_excerpt = b.as_ref().map(|(e, _)| e.clone());
                self.before_digest = b.map(|(_, d)| d);
                self.after_excerpt = a.as_ref().map(|(e, _)| e.clone());
                self.after_digest = a.map(|(_, d)| d);
                Ok(())
            }
        }
    }

    /// Verify a stored span against fresh source bytes (exact-source binding).
    pub fn verify_span(&self, source_bytes: &[u8]) -> Result<(), EvidenceError> {
        let Some(span) = &self.user_source_span else {
            return Err(EvidenceError::MissingProvenanceReceipt);
        };
        let start = span.byte_start.max(0) as usize;
        let end = span.byte_end.max(0) as usize;
        if start > end || end > source_bytes.len() {
            return Err(EvidenceError::InvalidSpanRange);
        }
        let found = sha256_hex(&source_bytes[start..end]);
        if found != span.bytes_sha256 {
            return Err(EvidenceError::SpanDigestMismatch {
                expected: span.bytes_sha256.clone(),
                found,
            });
        }
        Ok(())
    }
}

/// Authority-bearing evidence produced only by trusted signature, binding,
/// and replay verification. It deliberately has no public constructor and is
/// not deserializable.
#[derive(Debug, Clone)]
pub struct VerifiedUserActEvidence {
    evidence: UserActEvidenceV1,
    receipt_sha256: String,
}

impl VerifiedUserActEvidence {
    pub(crate) fn new(evidence: UserActEvidenceV1, receipt_sha256: String) -> Self {
        Self {
            evidence,
            receipt_sha256,
        }
    }

    pub fn evidence(&self) -> &UserActEvidenceV1 {
        &self.evidence
    }

    pub fn receipt_sha256(&self) -> &str {
        &self.receipt_sha256
    }

    pub fn classify(&self) -> EvidenceClass {
        if self.evidence.act_kind.is_user_authoritative_kind() {
            EvidenceClass::UserAuthoritative
        } else {
            EvidenceClass::UserBehavioral
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn receipt() -> UserActProvenanceReceiptV1 {
        UserActProvenanceReceiptV1 {
            contract_version: USER_ACT_RECEIPT_CONTRACT.into(),
            issuer_id: "issuer".into(),
            key_id: "key".into(),
            installation_id: "inst-1".into(),
            host: "pi".into(),
            session_id: "sess-1".into(),
            sequence: 1,
            nonce: "nonce-1".into(),
            payload_sha256: "a".repeat(64),
            signature_hex: "b".repeat(128),
        }
    }

    fn sample() -> UserActEvidenceV1 {
        UserActEvidenceV1::new(
            "ev-test-0001",
            "inst-1",
            "pi",
            "sess-1",
            vec!["event-1".into()],
            ActKind::ExplicitPreference,
            None,
            BTreeMap::new(),
            "2026-08-24T00:00:00Z",
            receipt(),
        )
        .unwrap()
    }

    #[test]
    fn missing_provenance_receipt_fails_closed() {
        let err = UserActEvidenceV1::new(
            "id",
            "i",
            "h",
            "s",
            vec![],
            ActKind::Correction,
            None,
            BTreeMap::new(),
            "t",
            UserActProvenanceReceiptV1 {
                issuer_id: String::new(),
                ..receipt()
            },
        )
        .unwrap_err();
        assert_eq!(err, EvidenceError::MissingProvenanceReceipt);
    }

    #[test]
    fn silent_acceptance_is_never_standalone_authority() {
        let mut ev = sample();
        ev.act_kind = ActKind::Accept;
        ev.signal_strength = 0.20;
        assert_eq!(
            VerifiedUserActEvidence::new(ev, "r".into()).classify(),
            EvidenceClass::UserBehavioral
        );
    }

    #[test]
    fn explicit_correction_from_authenticated_user_is_authoritative() {
        let mut ev = sample();
        ev.act_kind = ActKind::Correction;
        assert_eq!(
            VerifiedUserActEvidence::new(ev, "r".into()).classify(),
            EvidenceClass::UserAuthoritative
        );
    }

    #[test]
    fn span_verification_detects_mutation() {
        let bytes = b"always run focused tests";
        let mut ev = sample();
        ev.user_source_span = Some(SourceSpan::new("e", "s", 6, 13, &bytes[6..13]));
        assert!(ev.verify_span(bytes).is_ok());
        let tampered = b"never run focused tests";
        assert!(matches!(
            ev.verify_span(tampered),
            Err(EvidenceError::SpanDigestMismatch { .. })
        ));
    }

    #[test]
    fn span_verification_rejects_out_of_bounds_end_instead_of_clamping() {
        let bytes = b"always run focused tests";
        let mut ev = sample();
        ev.user_source_span = Some(SourceSpan::new("e", "s", 0, bytes.len() as i64, bytes));
        ev.user_source_span.as_mut().unwrap().byte_end += 1;
        assert_eq!(
            ev.verify_span(bytes).unwrap_err(),
            EvidenceError::InvalidSpanRange
        );
    }

    #[test]
    fn counterfactual_requires_content_and_binds_digests() {
        let mut ev = sample();
        assert_eq!(
            ev.set_counterfactual(None, None),
            Err(EvidenceError::EmptyExcerpt)
        );
        ev.set_counterfactual(Some("broad rewrite"), Some("local fix"))
            .unwrap();
        assert_ne!(ev.before_digest, ev.after_digest);
        assert_eq!(ev.before_excerpt.as_deref(), Some("broad rewrite"));
    }
}
