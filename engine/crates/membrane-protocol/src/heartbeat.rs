//! Per-client adapter heartbeat and mechanism receipt — the typed contract
//! the supervisor uses to distinguish **installed**, **active**, **delivering**,
//! and **stale** adapters. The Hub and CLI both consume the same heartbeat
//! table so they cannot conflate "installed" with "active" or "delivering".
//!
//! MBR-207: add per-client adapter heartbeat and mechanism receipt.

use serde::{Deserialize, Serialize};

/// Schema version of the adapter heartbeat envelope itself. Bumped on
/// incompatible shape changes; downstream consumers refuse unknown versions.
pub const ADAPTER_HEARTBEAT_SCHEMA_VERSION: u32 = 1;

/// What the supervisor knows about an adapter's lifecycle. The four states
/// are **mutually exclusive**: an adapter is exactly one of these at any
/// observation point. The Hub and CLI MUST use this enum rather than rolling
/// their own boolean flags, because conflating "installed" with "active" or
/// "active" with "delivering" is the MBR-207 acceptance condition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdapterStatus {
    /// The adapter is installed on the host but has not yet emitted a
    /// heartbeat. It cannot be trusted to deliver anything yet.
    Installed,
    /// The adapter has emitted a recent heartbeat with no mechanism receipts.
    /// It is live, but it is not currently doing delivery work.
    Active,
    /// The adapter is currently delivering mechanism-scoped work — at least
    /// one mechanism receipt has `count > 0`. This is the only state the
    /// CLI is allowed to label "delivering".
    Delivering,
    /// The adapter's last heartbeat is older than the staleness threshold
    /// (default 60 s). The supervisor treats it as gone for routing purposes
    /// even though the install is still present on disk.
    Stale,
}

impl AdapterStatus {
    /// Stable, lowercase label shared with the JS adapter registry and the
    /// Hub UI. The label is the canonical string contract.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Installed => "installed",
            Self::Active => "active",
            Self::Delivering => "delivering",
            Self::Stale => "stale",
        }
    }
}

impl std::fmt::Display for AdapterStatus {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// One mechanism receipt — a per-client count of work done under one
/// scope-grant-protected mechanism. A non-zero `count` is what flips the
/// heartbeat's [`AdapterStatus`] to [`AdapterStatus::Delivering`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MechanismReceiptV1 {
    /// Stable mechanism name, e.g. `membrane_context`, `membrane_source_read`.
    pub mechanism: String,
    /// The scope-grant id that authorised the most recent delivery under
    /// this mechanism. The supervisor cross-checks the grant against the
    /// active scope grant so a stale receipt cannot keep a delivery alive.
    pub scope_grant: String,
    /// RFC 3339 / ISO 8601 UTC instant at which the mechanism was last
    /// exercised. The supervisor uses this to derive whether a receipt is
    /// still "hot" (within the delivery window) or merely historical.
    pub last_used: String,
    /// Cumulative delivery count under this mechanism. A receipt is "hot"
    /// when this is strictly greater than zero.
    pub count: u64,
}

/// One adapter's heartbeat. The supervisor appends the most recent
/// heartbeat per `client_id` and the Hub + CLI read the same table; both
/// UIs therefore report the same status for the same client.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AdapterHeartbeatV1 {
    pub schema_version: u32,
    /// Stable client id, e.g. `cursor`, `windsurf`, `claude`, `codex`,
    /// `windsurf-mcp`. Treated as opaque by the supervisor: it never
    /// dereferences the id, so a future adapter does not require a code
    /// change here. This is intentional so MBR-207 does not fork on
    /// MBR-206's `operations/clients.yaml` arriving later.
    pub client_id: String,
    /// Adapter self-declared version (semver-ish). Captured for the Hub
    /// "what is installed" view; never parsed by the supervisor.
    pub adapter_version: String,
    /// Self-declared status. The supervisor MAY upgrade or downgrade this
    /// during [`summarize_status`] but never below the truth the adapter
    /// asserted — see the staleness rules in
    /// `membrane_supervisor::heartbeat`.
    pub status: AdapterStatus,
    /// RFC 3339 / ISO 8601 UTC instant at which the heartbeat was minted.
    /// The supervisor compares this against `now()` to compute staleness.
    pub last_seen: String,
    /// One receipt per mechanism the adapter has actually exercised. An
    /// empty list is valid (and means "active but not delivering").
    pub mechanism_receipts: Vec<MechanismReceiptV1>,
}

impl AdapterHeartbeatV1 {
    /// Build a heartbeat with the current schema version. The caller fills
    /// in every other field.
    pub fn new(
        client_id: impl Into<String>,
        adapter_version: impl Into<String>,
        status: AdapterStatus,
        last_seen: impl Into<String>,
        mechanism_receipts: Vec<MechanismReceiptV1>,
    ) -> Self {
        Self {
            schema_version: ADAPTER_HEARTBEAT_SCHEMA_VERSION,
            client_id: client_id.into(),
            adapter_version: adapter_version.into(),
            status,
            last_seen: last_seen.into(),
            mechanism_receipts,
        }
    }

    /// Validate envelope self-integrity. Rejects unknown schema versions so
    /// a future v2 envelope cannot be silently parsed as v1.
    pub fn verify_self_integrity(&self) -> Result<(), String> {
        if self.schema_version != ADAPTER_HEARTBEAT_SCHEMA_VERSION {
            return Err(format!(
                "unsupported adapter heartbeat schema_version {} (expected {})",
                self.schema_version, ADAPTER_HEARTBEAT_SCHEMA_VERSION
            ));
        }
        if self.client_id.is_empty() {
            return Err("client_id must be non-empty".to_string());
        }
        Ok(())
    }
}
