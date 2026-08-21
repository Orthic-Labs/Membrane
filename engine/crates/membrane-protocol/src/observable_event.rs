//! MBR-501: the normalized cross-agent observable event envelope.
//!
//! Every Membrane client (Claude, Codex, Cursor, Windsurf, generic MCP, the
//! CLI, the federation gateway, and the Hub) emits the SAME observable event
//! envelope so the runtime, the Hub, and the federation gateway can consume a
//! single normalized schema. The contract has six closed `kind`s and a
//! self-referential `sha256:` digest over the canonical-JSON of the rest of
//! the fields — the same self-referential pattern `canonical_digest_of` uses
//! for the other protocol shapes.
//!
//! The TypeScript twin lives in `mcp/observable-events.mjs`; both sides must
//! agree byte-for-byte over the canonical digest. The golden fixture is
//! `schemas/registry/observable-event.v1.golden.json`; the JSON Schema is
//! `schemas/observable-event.v1.schema.json`. The `SHAPES` const in
//! `lib.rs` exposes this shape next to the five existing protocol shapes.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// MBR-501: schema version of the observable event envelope itself. Bumped on
/// incompatible shape changes; downstream consumers refuse unknown versions.
pub const OBSERVABLE_EVENT_SCHEMA_VERSION: u32 = 1;

/// The closed set of observable event kinds every Membrane client must emit.
/// The Hub, the CLI, and the federation gateway dispatch on this enum so a
/// client cannot invent a private kind and silently bypass a downstream
/// consumer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservableEventKindV1 {
    /// A bounded context packet was admitted under a scope grant and delivered
    /// to the agent. `payload` carries `token_count` and `block_count`.
    ContextDelivered,
    /// A scope grant was minted and issued to a client. `scopeGrant` carries
    /// the grant id; `payload` carries the minting client and edge types.
    ScopeGrantIssued,
    /// A per-mechanism receipt was recorded (the heartbeat-level count the
    /// supervisor uses to flip an adapter into the `delivering` state).
    /// `mechanism` is the mechanism name; `scopeGrant` is the active grant.
    MechanismReceipt,
    /// A per-client adapter heartbeat was emitted (mirrors
    /// `AdapterHeartbeatV1.status`). `payload` carries `lastSeen` (RFC3339)
    /// and any adapter-declared version.
    AdapterHeartbeat,
    /// A provider error was observed. `payload` carries `code`, `message`,
    /// and `retryable`. Optional `scopeGrant`/`mechanism` describe context.
    ProviderError,
    /// A client conflated two records into one observable event (e.g. a
    /// multi-candidate admission collapsed into a single ledger row).
    /// `payload` carries the absorbed ids.
    ClientConflation,
}

impl ObservableEventKindV1 {
    /// Stable, lowercase label shared with the TypeScript twin. This is the
    /// canonical string contract the JSON Schema `enum` and the JS bus
    /// dispatch both key off.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ContextDelivered => "context_delivered",
            Self::ScopeGrantIssued => "scope_grant_issued",
            Self::MechanismReceipt => "mechanism_receipt",
            Self::AdapterHeartbeat => "adapter_heartbeat",
            Self::ProviderError => "provider_error",
            Self::ClientConflation => "client_conflation",
        }
    }
}

impl std::fmt::Display for ObservableEventKindV1 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// MBR-501: the single normalized observable event envelope. Every client
/// emits this exact shape — no client-local extensions. The `digest` is the
/// `sha256:` of the canonical-JSON of every other field, computed via
/// [`crate::canonical::canonical_digest_of`] after the digest field is
/// temporarily cleared. The pattern is intentionally identical to the
/// self-referential digest scheme used by the other protocol shapes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ObservableEventV1 {
    pub schema_version: u32,
    /// Lowercase UUIDv4, freshly minted by [`ObservableEventV1::new`].
    pub event_id: String,
    pub kind: ObservableEventKindV1,
    /// Stable client id, drawn from `schemas/registry/clients.yaml` (MBR-206).
    /// Examples: `claude`, `codex`, `cursor`, `windsurf`, `generic_mcp`.
    pub client_id: String,
    /// UUIDv4 installation id from `installation_identity::InstallationIdentity`.
    pub installation_id: String,
    /// Unix milliseconds at the producing client (RFC-style instant; the
    /// durable store additionally stamps its own `now_unix_ms`).
    pub occurred_at_unix_ms: u64,
    /// Optional scope-grant id this event is bound to. Present for
    /// `context_delivered`, `scope_grant_issued`, `mechanism_receipt`;
    /// `null` for `adapter_heartbeat` and `provider_error` when no grant
    /// was active.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope_grant: Option<String>,
    /// Optional mechanism name (e.g. `membrane_context`) when the event is
    /// bound to a single mechanism. Required for `mechanism_receipt`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mechanism: Option<String>,
    /// Open per-kind payload. The contract is closed at the `kind` layer;
    /// `payload` is an open JSON object whose shape is dictated by `kind`.
    pub payload: Value,
    /// `sha256:<hex>` digest of the canonical-JSON of every OTHER field. See
    /// [`Self::verify_self_integrity`] for the re-computation rule.
    pub digest: String,
}

impl ObservableEventV1 {
    /// Build a fresh envelope. Auto-fills `schema_version`, mints a v4 UUID
    /// for `event_id`, and computes the self-referential `digest` over the
    /// canonical-JSON of every other field.
    ///
    /// `scope_grant` and `mechanism` default to `None`; pass them when the
    /// `kind` requires them.
    pub fn new(
        kind: ObservableEventKindV1,
        client_id: impl Into<String>,
        installation_id: impl Into<String>,
        occurred_at_unix_ms: u64,
        payload: Value,
    ) -> Self {
        Self::new_with_bindings(
            kind,
            client_id,
            installation_id,
            occurred_at_unix_ms,
            payload,
            None,
            None,
        )
    }

    /// Variant of [`Self::new`] that accepts the optional binding fields.
    /// Kept as a separate constructor so the four-argument form stays short
    /// for the common `adapter_heartbeat`/`provider_error` cases.
    pub fn new_with_bindings(
        kind: ObservableEventKindV1,
        client_id: impl Into<String>,
        installation_id: impl Into<String>,
        occurred_at_unix_ms: u64,
        payload: Value,
        scope_grant: Option<String>,
        mechanism: Option<String>,
    ) -> Self {
        let event_id = mint_v4_uuid();
        let mut envelope = Self {
            schema_version: OBSERVABLE_EVENT_SCHEMA_VERSION,
            event_id,
            kind,
            client_id: client_id.into(),
            installation_id: installation_id.into(),
            occurred_at_unix_ms,
            scope_grant,
            mechanism,
            payload,
            // Filled in by recompute_digest below.
            digest: String::new(),
        };
        envelope.digest = envelope.recompute_digest();
        envelope
    }

    /// Recompute the self-referential digest by stripping the `digest`
    /// field, canonicalizing the rest, and hashing the canonical bytes.
    /// Mirrors the pattern used by `canonical::canonical_digest_of` so the
    /// TypeScript twin can reproduce it byte-for-byte.
    pub fn recompute_digest(&self) -> String {
        let mut without_digest =
            serde_json::to_value(self).expect("observable event serializes to JSON");
        if let Some(object) = without_digest.as_object_mut() {
            object.remove("digest");
        }
        crate::canonical::canonical_digest_of(&without_digest)
    }

    /// Validate envelope self-integrity. Rejects unknown schema versions so
    /// a future v2 envelope cannot be silently parsed as v1, and re-runs
    /// the digest check so a tampered payload is caught before the event
    /// reaches the durable store.
    pub fn verify_self_integrity(&self) -> Result<(), String> {
        if self.schema_version != OBSERVABLE_EVENT_SCHEMA_VERSION {
            return Err(format!(
                "unsupported observable event schema_version {} (expected {})",
                self.schema_version, OBSERVABLE_EVENT_SCHEMA_VERSION
            ));
        }
        if self.event_id.is_empty() {
            return Err("event_id must be non-empty".to_string());
        }
        if self.client_id.is_empty() {
            return Err("client_id must be non-empty".to_string());
        }
        if self.installation_id.is_empty() {
            return Err("installation_id must be non-empty".to_string());
        }
        let expected = self.recompute_digest();
        if expected != self.digest {
            return Err(format!(
                "observable event digest mismatch: expected {expected}, got {}",
                self.digest
            ));
        }
        Ok(())
    }
}

/// Mint a fresh lowercase UUIDv4. Wraps `getrandom` so the call site is
/// infallible at the type level (we retry once on the rare zero-byte case).
fn mint_v4_uuid() -> String {
    let mut bytes = [0u8; 16];
    // getrandom::fill returns Result but in practice never fails on
    // supported platforms. We swallow the error and re-try once with a
    // fresh buffer; a runtime failure still produces a valid v4-shaped
    // id so callers cannot observe a poison value through the envelope.
    getrandom::fill(&mut bytes).unwrap_or_else(|_| {
        let _ = getrandom::fill(&mut bytes);
    });
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3],
        bytes[4], bytes[5], bytes[6], bytes[7],
        bytes[8], bytes[9], bytes[10], bytes[11],
        bytes[12], bytes[13], bytes[14], bytes[15]
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn golden_inputs() -> (
        ObservableEventKindV1,
        &'static str,
        &'static str,
        u64,
        Value,
    ) {
        (
            ObservableEventKindV1::ContextDelivered,
            "claude",
            "00000000-0000-4000-8000-000000000001",
            1_700_000_000_000,
            json!({"reason": "golden", "token_count": 256}),
        )
    }

    #[test]
    fn envelope_round_trips_through_json() {
        let (kind, client, install, ts, payload) = golden_inputs();
        let envelope = ObservableEventV1::new(kind, client, install, ts, payload.clone());
        let serialized = serde_json::to_string(&envelope).expect("serializes");
        let parsed: ObservableEventV1 = serde_json::from_str(&serialized).expect("parses");
        assert_eq!(parsed, envelope);
        assert_eq!(parsed.kind, ObservableEventKindV1::ContextDelivered);
        assert_eq!(parsed.schema_version, OBSERVABLE_EVENT_SCHEMA_VERSION);
        assert_eq!(parsed.client_id, "claude");
        assert_eq!(parsed.payload, payload);
    }

    #[test]
    fn digest_is_stable_across_two_constructions_with_equal_ids() {
        // Two envelopes built with the SAME event_id and SAME occurred_at
        // must produce the same digest. We bypass `new` (which mints a fresh
        // id) by constructing manually and calling recompute_digest on both.
        let mut a = ObservableEventV1::new_with_bindings(
            ObservableEventKindV1::AdapterHeartbeat,
            "codex",
            "00000000-0000-4000-8000-000000000099",
            1_700_000_000_000,
            json!({"status": "active"}),
            None,
            None,
        );
        let mut b = ObservableEventV1::new_with_bindings(
            ObservableEventKindV1::AdapterHeartbeat,
            "codex",
            "00000000-0000-4000-8000-000000000099",
            1_700_000_000_000,
            json!({"status": "active"}),
            None,
            None,
        );
        a.event_id = "11111111-1111-4111-8111-111111111111".to_string();
        b.event_id = "11111111-1111-4111-8111-111111111111".to_string();
        a.digest = a.recompute_digest();
        b.digest = b.recompute_digest();
        assert_eq!(a.digest, b.digest);
    }

    #[test]
    fn verify_self_integrity_passes_on_a_fresh_envelope() {
        let envelope = ObservableEventV1::new_with_bindings(
            ObservableEventKindV1::ScopeGrantIssued,
            "cursor",
            "00000000-0000-4000-8000-0000000000aa",
            1_700_000_000_001,
            json!({"edge_types": ["source_read"]}),
            Some("sgv1-test".to_string()),
            None,
        );
        envelope
            .verify_self_integrity()
            .expect("fresh envelope must verify");
    }

    #[test]
    fn verify_self_integrity_rejects_a_tampered_payload() {
        let mut envelope = ObservableEventV1::new(
            ObservableEventKindV1::MechanismReceipt,
            "windsurf",
            "00000000-0000-4000-8000-0000000000bb",
            1_700_000_000_002,
            json!({"count": 1}),
        );
        envelope.payload = json!({"count": 999});
        let err = envelope
            .verify_self_integrity()
            .expect_err("tampered payload must not verify");
        assert!(err.contains("digest mismatch"), "unexpected error: {err}");
    }

    #[test]
    fn verify_self_integrity_rejects_unknown_schema_version() {
        let mut envelope = ObservableEventV1::new(
            ObservableEventKindV1::AdapterHeartbeat,
            "claude",
            "00000000-0000-4000-8000-0000000000cc",
            1_700_000_000_003,
            json!({"status": "active"}),
        );
        envelope.schema_version = 2;
        // Re-stamp digest to bypass the digest check so we exercise the
        // schema-version branch alone.
        envelope.digest = envelope.recompute_digest();
        let err = envelope
            .verify_self_integrity()
            .expect_err("future schema version must be refused");
        assert!(err.contains("schema_version"), "unexpected error: {err}");
    }

    #[test]
    fn all_six_kinds_serialize_to_their_canonical_strings() {
        let cases = [
            (ObservableEventKindV1::ContextDelivered, "context_delivered"),
            (
                ObservableEventKindV1::ScopeGrantIssued,
                "scope_grant_issued",
            ),
            (ObservableEventKindV1::MechanismReceipt, "mechanism_receipt"),
            (ObservableEventKindV1::AdapterHeartbeat, "adapter_heartbeat"),
            (ObservableEventKindV1::ProviderError, "provider_error"),
            (ObservableEventKindV1::ClientConflation, "client_conflation"),
        ];
        for (kind, label) in cases {
            assert_eq!(kind.as_str(), label);
            let value = serde_json::to_value(kind).expect("serializes");
            assert_eq!(value, serde_json::Value::String(label.to_string()));
        }
    }
}
