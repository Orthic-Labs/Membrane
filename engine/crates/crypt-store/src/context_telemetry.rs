//! Content-free, append-only telemetry ledger for context-system value accounting.
//!
//! This module owns the language-neutral event envelope at the Rust boundary. Callers may attach
//! opaque identifiers and finite measurements, but never prompts, queries, memory bodies, paths,
//! replies, arguments, or raw errors. Validation happens before the SQLite transaction and the
//! same method backs both direct Rust writes and the loopback HTTP batch endpoint.

use crate::memdb::MemDb;
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub(crate) const CONTEXT_LEDGER_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS context_installation (
    installation_id         TEXT PRIMARY KEY,
    created_at              TEXT NOT NULL,
    first_observed_at       TEXT NOT NULL,
    last_observed_at        TEXT NOT NULL,
    display_label           TEXT,
    os_type                 TEXT,
    os_version              TEXT,
    arch                    TEXT,
    identity_key_fingerprint TEXT,
    installation_epoch      INTEGER NOT NULL DEFAULT 1
);
CREATE TABLE IF NOT EXISTS context_event_log (
    seq                 INTEGER PRIMARY KEY AUTOINCREMENT,
    event_id            TEXT NOT NULL UNIQUE,
    schema_version      INTEGER NOT NULL,
    ts                  TEXT NOT NULL,
    ingested_at         TEXT NOT NULL,
    installation_id     TEXT NOT NULL,
    service_instance_id TEXT NOT NULL,
    workspace_id        TEXT NOT NULL,
    client              TEXT NOT NULL,
    client_version      TEXT,
    producer            TEXT NOT NULL,
    producer_version    TEXT,
    session_id          TEXT NOT NULL,
    turn_id             TEXT,
    trace_id            TEXT NOT NULL,
    span_id             TEXT NOT NULL,
    parent_span_id      TEXT,
    artifact_family     TEXT NOT NULL,
    provider            TEXT NOT NULL,
    provider_version    TEXT,
    release_generation  TEXT,
    phase               TEXT NOT NULL,
    operation           TEXT NOT NULL,
    status              TEXT NOT NULL,
    reason_code         TEXT,
    scope_id            TEXT,
    artifact_id         TEXT,
    artifact_sha256     TEXT,
    traffic_class       TEXT NOT NULL,
    policy_version      TEXT,
    policy_activation_sha256 TEXT,
    cohort              TEXT,
    task_class          TEXT,
    source_generation   INTEGER,
    duration_ms         REAL,
    quantity            INTEGER,
    token_count         INTEGER,
    char_count          INTEGER,
    meta                TEXT NOT NULL DEFAULT '{}',
    canonical_sha256    TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_context_event_installation_ts
    ON context_event_log(installation_id, ts);
CREATE INDEX IF NOT EXISTS idx_context_event_session_ts
    ON context_event_log(session_id, ts);
CREATE INDEX IF NOT EXISTS idx_context_event_family_phase_status
    ON context_event_log(artifact_family, phase, status, ts);
CREATE INDEX IF NOT EXISTS idx_context_event_trace_span
    ON context_event_log(trace_id, span_id);
CREATE INDEX IF NOT EXISTS idx_context_event_provider_operation
    ON context_event_log(provider, operation, status, ts);
CREATE TABLE IF NOT EXISTS context_event_measurement (
    event_id       TEXT NOT NULL,
    name           TEXT NOT NULL,
    value_type     TEXT NOT NULL CHECK (value_type IN ('integer', 'real', 'boolean')),
    value_integer  INTEGER,
    value_real     REAL,
    value_boolean  INTEGER CHECK (value_boolean IN (0, 1)),
    unit           TEXT NOT NULL,
    PRIMARY KEY (event_id, name),
    FOREIGN KEY (event_id) REFERENCES context_event_log(event_id) ON DELETE RESTRICT,
    CHECK (
        (value_type = 'integer' AND value_integer IS NOT NULL
            AND value_real IS NULL AND value_boolean IS NULL) OR
        (value_type = 'real' AND value_integer IS NULL
            AND value_real IS NOT NULL AND value_boolean IS NULL) OR
        (value_type = 'boolean' AND value_integer IS NULL
            AND value_real IS NULL AND value_boolean IS NOT NULL)
    )
);
CREATE INDEX IF NOT EXISTS idx_context_measurement_name_unit
    ON context_event_measurement(name, unit);
CREATE TABLE IF NOT EXISTS context_event_link (
    event_id        TEXT NOT NULL,
    relation        TEXT NOT NULL,
    target_event_id TEXT NOT NULL,
    PRIMARY KEY (event_id, relation, target_event_id),
    FOREIGN KEY (event_id) REFERENCES context_event_log(event_id) ON DELETE RESTRICT
);
CREATE INDEX IF NOT EXISTS idx_context_link_target
    ON context_event_link(target_event_id, relation);
CREATE TABLE IF NOT EXISTS context_ingest_checkpoint (
    source          TEXT PRIMARY KEY,
    watermark       TEXT NOT NULL,
    source_count    INTEGER NOT NULL,
    ingested_count  INTEGER NOT NULL,
    rejected_count  INTEGER NOT NULL,
    checksum_sha256 TEXT NOT NULL,
    updated_at      TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS context_event_integrity (
    seq INTEGER PRIMARY KEY,
    event_id TEXT NOT NULL UNIQUE,
    canonical_sha256 TEXT NOT NULL,
    prev_chain_sha256 TEXT NOT NULL,
    chain_sha256 TEXT NOT NULL,
    segment_key TEXT NOT NULL,
    ordinal INTEGER NOT NULL,
    FOREIGN KEY (seq) REFERENCES context_event_log(seq) ON DELETE RESTRICT
);
CREATE TABLE IF NOT EXISTS context_event_segment (
    segment_key TEXT PRIMARY KEY,
    first_seq INTEGER NOT NULL,
    last_seq INTEGER NOT NULL,
    event_count INTEGER NOT NULL,
    content_sha256 TEXT NOT NULL,
    sealed_at TEXT
);
CREATE TABLE IF NOT EXISTS context_event_retention_receipt (
    receipt_id TEXT PRIMARY KEY,
    segment_key TEXT NOT NULL,
    first_removed_seq INTEGER NOT NULL,
    last_removed_seq INTEGER NOT NULL,
    removed_count INTEGER NOT NULL,
    removed_sha256 TEXT NOT NULL,
    prior_chain_sha256 TEXT NOT NULL,
    anchor_chain_sha256 TEXT NOT NULL,
    created_at TEXT NOT NULL
);
"#;

const EVENT_SCHEMA_VERSION: u32 = 1;
const MAX_BATCH_EVENTS: usize = 256;
const MAX_CHILDREN: usize = 32;
pub const MAX_DURATION_MS: f64 = 86_400_000.0;
const INTEGRITY_GENESIS: &str = "0";

pub(crate) fn chain_digest(
    segment: &str,
    ordinal: i64,
    event_id: &str,
    canonical: &str,
    prev: &str,
) -> String {
    let mut h = Sha256::new();
    h.update(b"membrane-context-chain-v1\0");
    h.update(segment.as_bytes());
    h.update([0]);
    h.update(ordinal.to_string().as_bytes());
    h.update([0]);
    h.update(event_id.as_bytes());
    h.update([0]);
    h.update(canonical.as_bytes());
    h.update([0]);
    h.update(prev.as_bytes());
    format!("{:x}", h.finalize())
}
pub(crate) fn segment_digest(
    segment: &str,
    ordinal: i64,
    event_id: &str,
    canonical: &str,
    prev: &str,
) -> String {
    sha256_text(&format!(
        "{segment}:{ordinal}:{}",
        chain_digest(segment, ordinal, event_id, canonical, prev)
    ))
}
/// Hard ceiling on `ObservableEventQuery::limit`. Bounds a single read regardless of caller input.
const MAX_OBSERVABLE_QUERY_LIMIT: usize = 1_000;
/// Internal safety valve on how many raw ledger rows a single observable-event read will scan
/// before giving up and reporting the result as truncated. Bounds worst-case query cost when
/// origin/event_type filtering (applied in Rust, not SQL) rejects most of a wide raw window.
const MAX_OBSERVABLE_SCAN_ROWS: usize = 10_000;

const REGISTRY_JSON: &str = include_str!("../../../../lib/context-telemetry-registry.json");
const EXTENSION_PATTERN: &str = r"^ext\.[a-z0-9][a-z0-9_-]{0,31}\.[a-z0-9][a-z0-9._-]{0,46}$";

struct ContextRegistry {
    providers: HashSet<String>,
    artifact_families: HashSet<String>,
    phases: HashMap<String, PhaseBudget>,
}

struct PhaseBudget {
    max_events_per_turn: usize,
    max_bytes_per_event: usize,
}

fn context_registry() -> &'static ContextRegistry {
    static REGISTRY: OnceLock<ContextRegistry> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        let raw: Value = serde_json::from_str(REGISTRY_JSON)
            .expect("checked-in context telemetry registry must be valid JSON");
        assert_eq!(raw["schema_version"], 1);
        assert_eq!(raw["tokens"]["extension_pattern"], EXTENSION_PATTERN);
        let providers = raw["tokens"]["providers"]
            .as_object()
            .expect("context provider registry must be an object")
            .keys()
            .cloned()
            .collect();
        let artifact_families = raw["tokens"]["artifact_families"]
            .as_array()
            .expect("context artifact-family registry must be an array")
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .expect("context artifact-family token must be a string")
                    .to_string()
            })
            .collect();
        let phases = raw["phases"]
            .as_object()
            .expect("context phase registry must be an object")
            .iter()
            .map(|(phase, contract)| {
                let max_events_per_turn = contract["max_events_per_turn"]
                    .as_u64()
                    .and_then(|value| usize::try_from(value).ok())
                    .filter(|value| *value > 0)
                    .expect("phase cardinality budget must be a positive integer");
                let max_bytes_per_event = contract["max_bytes_per_event"]
                    .as_u64()
                    .and_then(|value| usize::try_from(value).ok())
                    .filter(|value| *value > 0)
                    .expect("phase byte budget must be a positive integer");
                (
                    phase.clone(),
                    PhaseBudget {
                        max_events_per_turn,
                        max_bytes_per_event,
                    },
                )
            })
            .collect();
        ContextRegistry {
            providers,
            artifact_families,
            phases,
        }
    })
}

pub fn registered_artifact_family(value: &str) -> bool {
    context_registry().artifact_families.contains(value) || valid_namespaced_extension(value)
}

/// MBR-011: the ONE stable alert id for a selected-without-delivery failure.
pub const SELECTED_WITHOUT_DELIVERY_ALERT: &str = "selected_without_delivery";

/// MBR-011: the delivery verdict for a turn. This mirrors the renderer's
/// `evaluateDeliveryOutcome` (mcp/context-renderer-lib.cjs) so the persistent
/// telemetry layer and the in-prompt renderer agree byte-for-byte on the same
/// selected/delivered counts and the same promotion decision.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryOutcome {
    /// "ok" | "degraded". Not a telemetry event status token — this is the
    /// turn-level verdict surfaced to callers and release tooling.
    pub status: &'static str,
    /// Exactly one stable alert id when the turn failed, else None.
    pub alert: Option<&'static str>,
    pub selected: i64,
    pub delivered: i64,
    /// True iff a release built on this turn must NOT be promoted.
    pub release_promotion_blocked: bool,
}

/// MBR-011: a turn that selected content but delivered NOTHING is a visible
/// failure, not a quiet success. When `selected > 0` and `delivered == 0` the
/// status is "degraded", exactly one stable alert id is emitted, and release
/// promotion is blocked.
pub fn evaluate_delivery_outcome(selected: i64, delivered: i64) -> DeliveryOutcome {
    let selected = selected.max(0);
    let delivered = delivered.max(0);
    let failed = selected > 0 && delivered == 0;
    DeliveryOutcome {
        status: if failed { "degraded" } else { "ok" },
        alert: failed.then_some(SELECTED_WITHOUT_DELIVERY_ALERT),
        selected,
        delivered,
        release_promotion_blocked: failed,
    }
}

/// MBR-012: the canonical delivered-content metrics. This mirrors
/// `canonicalDeliveryMetrics` (mcp/context-renderer-lib.cjs) so the Rust
/// telemetry and the JS renderer agree byte-for-byte on a qualification fixture.
/// Conventions: bytes = UTF-8 byte count; chars = Unicode code-point count;
/// tokens = ceil(bytes/4); sha256 = hex digest of the UTF-8 bytes.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryMetrics {
    pub bytes: u64,
    pub chars: u64,
    pub tokens: u64,
    pub sha256: String,
}

/// MBR-012: derive all four delivered-content surfaces from one UTF-8
/// serialization so they can never disagree.
pub fn canonical_delivery_metrics(text: &str) -> DeliveryMetrics {
    let bytes = text.as_bytes().len() as u64;
    let chars = text.chars().count() as u64;
    let tokens = (bytes + 3) / 4;
    let sha256 = format!("sha256:{:x}", Sha256::digest(text.as_bytes()));
    DeliveryMetrics {
        bytes,
        chars,
        tokens,
        sha256,
    }
}
const OPERATIONS: &[&str] = &[
    "read",
    "write",
    "update",
    "delete",
    "sync",
    "transform",
    "evaluate",
    "reconcile",
];
const STATUSES: &[&str] = &[
    "expected",
    "started",
    "success",
    "empty",
    "skipped",
    "filtered",
    "failed",
    "timeout",
    "unavailable",
    "contradicted",
    "unknown",
];
const TERMINAL_REASON_STATUSES: &[&str] = &[
    "empty",
    "skipped",
    "filtered",
    "failed",
    "timeout",
    "unavailable",
    "contradicted",
    "unknown",
];
const RELATIONS: &[&str] = &[
    "attempt_of",
    "terminal_of",
    "candidate_of",
    "delivered_as",
    "feedback_for",
    "replicated_from",
    "outcome_for",
];
const MEASUREMENT_UNITS: &[&str] = &[
    "ms",
    "bytes",
    "chars",
    "tokens",
    "objects",
    "candidates",
    "boolean",
    "ratio",
];

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextEventBatch {
    pub events: Vec<ContextEvent>,
}

/// Language-neutral host lifecycle event owned by Membrane's SQLite event ledger. The richer
/// ContextEvent remains the internal accounting form; this envelope is the frozen cross-host
/// contract and carries only opaque refs/digests.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ObservableEventV1 {
    pub schema: String,
    pub installation_id: String,
    pub client_id: String,
    pub session_id: String,
    pub task_id: String,
    pub turn_id: String,
    pub trace_id: String,
    pub event_id: String,
    pub event_type: String,
    pub origin: String,
    pub content_ref_or_digest: String,
    pub timestamp: String,
    pub completeness: BTreeMap<String, bool>,
    pub policy_snapshot_digest: String,
    /// Numeric-only, content-free duration (e.g. a tool call's completed_at - started_at, in
    /// milliseconds). Never content: same discipline as the existing `duration_ms`/`measurements`
    /// numeric fields on `ContextEvent`. Absent for producers that don't measure a span.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<f64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ObservableEventBatchV1 {
    pub events: Vec<ObservableEventV1>,
}

impl ObservableEventV1 {
    pub fn validate(&self) -> Result<(), ContextTelemetryError> {
        if self.schema != "orthic.observable-event.v1" {
            return Err(invalid("schema", "must be orthic.observable-event.v1"));
        }
        for (field, value) in [
            ("installation_id", &self.installation_id),
            ("client_id", &self.client_id),
            ("session_id", &self.session_id),
            ("task_id", &self.task_id),
            ("turn_id", &self.turn_id),
            ("trace_id", &self.trace_id),
            ("event_id", &self.event_id),
            ("event_type", &self.event_type),
            ("content_ref_or_digest", &self.content_ref_or_digest),
        ] {
            if value.trim().is_empty() {
                return Err(invalid(field, "must be non-empty"));
            }
        }
        if !matches!(
            self.origin.as_str(),
            "host" | "user" | "assistant" | "tool" | "repository" | "service"
        ) {
            return Err(invalid("origin", "must be a frozen origin value"));
        }
        let policy_digest = self
            .policy_snapshot_digest
            .strip_prefix("sha256:")
            .ok_or_else(|| invalid("policy_snapshot_digest", "must use sha256: prefix"))?;
        validate_sha256(policy_digest, "policy_snapshot_digest")?;
        if !valid_utc_timestamp(&self.timestamp) {
            return Err(invalid("timestamp", "must be RFC3339"));
        }
        if let Some(duration_ms) = self.duration_ms {
            if !duration_ms.is_finite() || duration_ms < 0.0 || duration_ms > MAX_DURATION_MS {
                return Err(invalid("duration_ms", "must be finite and within bounds"));
            }
        }
        Ok(())
    }
}

/// Immutable installation/process/workspace attribution attached to events emitted by the
/// resident memory engine. It contains opaque identifiers only and is resolved once when a
/// store opens, never by doing filesystem or network work on an operation path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OperationAttribution {
    pub installation_id: String,
    pub service_instance_id: String,
    pub workspace_id: String,
}

impl OperationAttribution {
    pub fn new(
        installation_id: &str,
        service_instance_id: &str,
        workspace_id: &str,
    ) -> Result<Self, ContextTelemetryError> {
        if !valid_uuid(installation_id) {
            return Err(invalid("installation_id", "must be a UUID"));
        }
        require_service_id(service_instance_id, "service_instance_id")?;
        require_workspace_id(workspace_id, "workspace_id")?;
        Ok(Self {
            installation_id: installation_id.to_string(),
            service_instance_id: service_instance_id.to_string(),
            workspace_id: workspace_id.to_string(),
        })
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextEvent {
    pub schema_version: u32,
    pub event_id: String,
    pub ts: String,
    pub installation_id: String,
    pub service_instance_id: String,
    pub workspace_id: String,
    pub client: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_version: Option<String>,
    pub producer: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub producer_version: Option<String>,
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    pub trace_id: String,
    pub span_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_span_id: Option<String>,
    pub artifact_family: String,
    pub provider: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_generation: Option<String>,
    pub phase: String,
    pub operation: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_sha256: Option<String>,
    pub traffic_class: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_activation_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cohort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_class: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_generation: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quantity: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_count: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub char_count: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<BTreeMap<String, Value>>,
    pub measurements: Vec<ContextEventMeasurement>,
    pub links: Vec<ContextEventLink>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextEventMeasurement {
    pub name: String,
    pub value: MeasurementValue,
    pub unit: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum MeasurementValue {
    Boolean(bool),
    Integer(i64),
    Real(f64),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextEventLink {
    pub relation: String,
    pub target_event_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextLifecycleCandidate {
    pub artifact_id: String,
    #[serde(default)]
    pub admitted: bool,
    #[serde(default)]
    pub delivered: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextLifecycleProvider {
    pub provider: String,
    pub artifact_family: String,
    pub status: String,
    #[serde(default)]
    pub candidate_count: i64,
    #[serde(default)]
    pub upstream_count: i64,
    #[serde(default)]
    pub admitted_count: i64,
    #[serde(default)]
    pub delivered_count: i64,
    #[serde(default)]
    pub duration_ms: Option<f64>,
    #[serde(default)]
    pub reason_code: Option<String>,
    #[serde(default)]
    pub candidates: Vec<ContextLifecycleCandidate>,
}

/// Compact, content-free prompt record. The prompt process persists this bounded intent; the
/// resident service expands and validates its canonical event graph away from prompt latency.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextLifecycleIntent {
    pub schema_version: u32,
    pub installation_id: String,
    pub service_instance_id: String,
    pub workspace_id: String,
    pub session_id: String,
    pub trace_id: String,
    pub policy_version: String,
    #[serde(default)]
    pub policy_activation_sha256: Option<String>,
    pub task_class: String,
    pub ts: String,
    pub client: String,
    #[serde(default)]
    pub client_version: Option<String>,
    #[serde(default)]
    pub producer: Option<String>,
    #[serde(default)]
    pub producer_version: Option<String>,
    #[serde(default)]
    pub provider_version: Option<String>,
    #[serde(default)]
    pub release_generation: Option<String>,
    pub mode: String,
    pub cohort: String,
    pub cohorts_enabled: bool,
    pub federation_status: String,
    #[serde(default)]
    pub federation_reason: Option<String>,
    #[serde(default)]
    pub legacy_status: Option<String>,
    #[serde(default)]
    pub candidate_count: i64,
    #[serde(default)]
    pub admitted_count: i64,
    #[serde(default)]
    pub delivered_count: i64,
    #[serde(default)]
    pub delivered_chars: i64,
    #[serde(default)]
    pub truncated_count: i64,
    #[serde(default)]
    pub federation_duration_ms: Option<f64>,
    #[serde(default)]
    pub provider_results: Vec<ContextLifecycleProvider>,
    pub expected_event_count: usize,
    pub expected_terminal_count: usize,
}

#[derive(Default)]
struct LifecycleEventFields {
    artifact_family: Option<String>,
    parent_span_id: Option<String>,
    reason_code: Option<String>,
    artifact_id: Option<String>,
    duration_ms: Option<f64>,
    quantity: Option<i64>,
    char_count: Option<i64>,
    measurements: Vec<ContextEventMeasurement>,
    links: Vec<ContextEventLink>,
}

#[derive(Clone)]
struct LifecycleEventRef {
    event_id: String,
    span_id: String,
}

struct LifecycleEventBuilder<'a> {
    intent: &'a ContextLifecycleIntent,
    turn_id: String,
    events: Vec<ContextEvent>,
}

fn sha256_text(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn lifecycle_bounded_id(value: &str, prefix: &str) -> String {
    if valid_uuid(value) || prefixed_digest(value, prefix, '-', &[32]) {
        value.to_string()
    } else {
        format!("{prefix}-{}", &sha256_text(value)[..32])
    }
}

fn lifecycle_workspace_id(value: &str) -> String {
    if prefixed_digest(value, "ws", '.', &[32]) {
        value.to_string()
    } else {
        let canonical = value
            .replace([':', '\\', '/'], "-")
            .trim_matches('-')
            .to_string();
        format!(
            "ws.{}",
            &sha256_text(if canonical.is_empty() {
                "global"
            } else {
                &canonical
            })[..32]
        )
    }
}

fn lifecycle_artifact_id(value: &str, family: &str) -> String {
    if valid_uuid(value) || require_artifact_id(value, "artifact_id").is_ok() {
        return value.to_string();
    }
    let namespace: String = family
        .to_ascii_lowercase()
        .chars()
        .map(|character| {
            if character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || matches!(character, '-' | '_')
            {
                character
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches('_')
        .chars()
        .take(32)
        .collect();
    format!(
        "{}.{}",
        if namespace.is_empty() {
            "artifact"
        } else {
            &namespace
        },
        &sha256_text(value)[..32]
    )
}

/// Normalize a caller-supplied `event_type` taxonomy label into the bounded lowercase token the
/// write path persists (see `ingest_observable_events`) and the read path matches against (see
/// `query_observable_events`). Unlike `lifecycle_bounded_id`/`lifecycle_artifact_id`, this never
/// hashes on the happy path: `event_type` is a small, bounded classification label rather than an
/// opaque identifier, so it stays human-legible in storage and in query filters. Any input --
/// including one that normalizes to empty -- always yields a value that passes `validate_meta`,
/// so this can never turn a previously-accepted `ObservableEventV1` into a rejected one.
fn lifecycle_event_type_token(value: &str) -> String {
    let canonical: String = value
        .trim()
        .to_ascii_lowercase()
        .chars()
        .map(|character| {
            if character.is_ascii_lowercase() || character.is_ascii_digit() {
                character
            } else {
                '_'
            }
        })
        .take(60)
        .collect();
    let trimmed = canonical.trim_matches('_');
    if trimmed.is_empty() {
        format!("type-{}", &sha256_text(value)[..16])
    } else {
        trimmed.to_string()
    }
}

impl<'a> LifecycleEventBuilder<'a> {
    fn new(intent: &'a ContextLifecycleIntent) -> Self {
        Self {
            intent,
            turn_id: lifecycle_bounded_id(&format!("turn-{}", intent.trace_id), "turn"),
            events: Vec::with_capacity(intent.expected_event_count),
        }
    }

    fn add(
        &mut self,
        span: &str,
        provider: &str,
        phase: &str,
        status: &str,
        fields: LifecycleEventFields,
    ) -> Result<LifecycleEventRef, ContextTelemetryError> {
        let span_id = lifecycle_bounded_id(span, "span");
        let artifact_family = fields
            .artifact_family
            .unwrap_or_else(|| "context".to_string());
        let artifact_id = fields
            .artifact_id
            .map(|value| lifecycle_artifact_id(&value, &artifact_family));
        let mut event = ContextEvent {
            schema_version: EVENT_SCHEMA_VERSION,
            event_id: String::new(),
            ts: self.intent.ts.clone(),
            installation_id: self.intent.installation_id.clone(),
            service_instance_id: lifecycle_bounded_id(&self.intent.service_instance_id, "svc"),
            workspace_id: lifecycle_workspace_id(&self.intent.workspace_id),
            client: self.intent.client.clone(),
            client_version: self.intent.client_version.clone(),
            producer: self
                .intent
                .producer
                .clone()
                .unwrap_or_else(|| "rightcontext_planner".to_string()),
            producer_version: self.intent.producer_version.clone(),
            session_id: lifecycle_bounded_id(&self.intent.session_id, "session"),
            turn_id: Some(self.turn_id.clone()),
            trace_id: lifecycle_bounded_id(&self.intent.trace_id, "trace"),
            span_id: span_id.clone(),
            parent_span_id: fields.parent_span_id,
            artifact_family,
            provider: provider.to_string(),
            provider_version: self.intent.provider_version.clone(),
            release_generation: self.intent.release_generation.clone(),
            phase: phase.to_string(),
            operation: if phase.starts_with("policy.") {
                "evaluate"
            } else {
                "read"
            }
            .to_string(),
            status: status.to_string(),
            reason_code: fields.reason_code,
            scope_id: None,
            artifact_id,
            artifact_sha256: None,
            traffic_class: if self.intent.client.starts_with("smoke") {
                "smoke"
            } else {
                "production"
            }
            .to_string(),
            policy_version: Some(self.intent.policy_version.clone()),
            policy_activation_sha256: self.intent.policy_activation_sha256.clone(),
            cohort: Some(self.intent.cohort.clone()),
            task_class: Some(self.intent.task_class.clone()),
            source_generation: None,
            duration_ms: fields.duration_ms,
            quantity: fields.quantity,
            token_count: None,
            char_count: fields.char_count,
            meta: None,
            measurements: fields.measurements,
            links: fields.links,
        };
        let mut value = serde_json::to_value(&event)
            .map_err(|_| invalid("event", "could not be canonically serialized"))?;
        value
            .as_object_mut()
            .expect("event is an object")
            .remove("event_id");
        let preimage = serde_json::to_vec(&sort_json_recursively(value))
            .map_err(|_| invalid("event", "could not be canonically serialized"))?;
        let mut hasher = Sha256::new();
        hasher.update(preimage);
        event.event_id = format!("evt-{:x}", hasher.finalize());
        validate_context_event(&event)?;
        let reference = LifecycleEventRef {
            event_id: event.event_id.clone(),
            span_id,
        };
        self.events.push(event);
        Ok(reference)
    }
}

fn lifecycle_fields(parent_span_id: Option<String>) -> LifecycleEventFields {
    LifecycleEventFields {
        parent_span_id,
        ..LifecycleEventFields::default()
    }
}

fn lifecycle_link(relation: &str, target_event_id: &str) -> ContextEventLink {
    ContextEventLink {
        relation: relation.to_string(),
        target_event_id: target_event_id.to_string(),
    }
}

fn lifecycle_status(value: &str) -> bool {
    matches!(
        value,
        "success" | "empty" | "skipped" | "failed" | "timeout" | "unavailable"
    )
}

/// Expand one prompt intent into the same canonical event graph emitted by the Python and Node
/// compatibility builders. No event is returned unless the graph is complete and schema-valid.
pub fn expand_context_lifecycle_intent(
    intent: &ContextLifecycleIntent,
) -> Result<ContextEventBatch, ContextTelemetryError> {
    if intent.schema_version != EVENT_SCHEMA_VERSION
        || intent.expected_event_count == 0
        || intent.expected_event_count > MAX_BATCH_EVENTS
        || intent.expected_terminal_count == 0
        || intent.expected_terminal_count > intent.expected_event_count
    {
        return Err(invalid("lifecycle_intent", "has invalid bounds"));
    }
    if !matches!(intent.mode.as_str(), "off" | "shadow" | "on")
        || !matches!(
            intent.cohort.as_str(),
            "control" | "candidate" | "unassigned"
        )
        || !lifecycle_status(&intent.federation_status)
        || intent
            .legacy_status
            .as_deref()
            .is_some_and(|value| !lifecycle_status(value))
        || intent.provider_results.len() > MAX_CHILDREN
    {
        return Err(invalid("lifecycle_intent", "contains an unsupported token"));
    }
    let candidates = intent.candidate_count.max(0);
    let admitted = intent.admitted_count.max(0).min(candidates);
    let delivered = intent.delivered_count.max(0);
    let delivered_chars = intent.delivered_chars.max(0);
    let truncated = intent.truncated_count.max(0);
    let mut builder = LifecycleEventBuilder::new(intent);

    let turn = builder.add(
        "turn",
        "rightcontext",
        "turn.observed",
        "success",
        LifecycleEventFields::default(),
    )?;
    let (assignment_status, assignment_reason) = if !intent.cohorts_enabled {
        ("skipped", Some("cohorts_disabled".to_string()))
    } else if intent.cohort == "unassigned" {
        ("unknown", Some("assignment_unavailable".to_string()))
    } else {
        ("success", None)
    };
    let mut fields = lifecycle_fields(Some(turn.span_id.clone()));
    fields.reason_code = assignment_reason;
    builder.add(
        "policy-assigned",
        "policy",
        "policy.assigned",
        assignment_status,
        fields,
    )?;
    let mut fields = lifecycle_fields(Some(turn.span_id.clone()));
    fields.reason_code = Some(if intent.cohort == "control" {
        "cohort_control".to_string()
    } else {
        format!("mode_{}", intent.mode)
    });
    builder.add(
        "policy-exposed",
        "policy",
        "policy.exposed",
        "success",
        fields,
    )?;

    let expected = builder.add(
        "rightcontext-expected",
        "rightcontext",
        "provider.expected",
        "expected",
        lifecycle_fields(Some(turn.span_id.clone())),
    )?;
    if intent.federation_status != "skipped" {
        let mut fields = lifecycle_fields(Some(expected.span_id.clone()));
        fields
            .links
            .push(lifecycle_link("attempt_of", &expected.event_id));
        builder.add(
            "rightcontext-started",
            "rightcontext",
            "provider.started",
            "started",
            fields,
        )?;
    }
    let mut fields = lifecycle_fields(Some(expected.span_id.clone()));
    fields
        .links
        .push(lifecycle_link("terminal_of", &expected.event_id));
    if intent.federation_status != "success" {
        fields.reason_code = Some(
            intent
                .federation_reason
                .clone()
                .unwrap_or_else(|| format!("provider_{}", intent.federation_status)),
        );
    }
    fields.duration_ms = intent.federation_duration_ms.map(|value| value.max(0.0));
    let rightcontext_terminal = builder.add(
        "rightcontext-terminal",
        "rightcontext",
        "provider.terminal",
        &intent.federation_status,
        fields,
    )?;

    for lane in &intent.provider_results {
        if !context_registry().providers.contains(&lane.provider)
            || !lifecycle_status(&lane.status)
            || !context_registry()
                .artifact_families
                .contains(&lane.artifact_family)
            || lane.candidates.len() > MAX_CHILDREN
        {
            return Err(invalid("provider_results", "contains an unsupported lane"));
        }
        let prefix = format!("lane-{}-{}", lane.provider, lane.artifact_family);
        let mut fields = lifecycle_fields(Some(turn.span_id.clone()));
        fields.artifact_family = Some(lane.artifact_family.clone());
        let lane_expected = builder.add(
            &format!("{prefix}-expected"),
            &lane.provider,
            "provider.expected",
            "expected",
            fields,
        )?;
        if lane.status != "skipped" {
            let mut fields = lifecycle_fields(Some(lane_expected.span_id.clone()));
            fields.artifact_family = Some(lane.artifact_family.clone());
            fields
                .links
                .push(lifecycle_link("attempt_of", &lane_expected.event_id));
            builder.add(
                &format!("{prefix}-started"),
                &lane.provider,
                "provider.started",
                "started",
                fields,
            )?;
        }
        let mut fields = lifecycle_fields(Some(lane_expected.span_id.clone()));
        fields.artifact_family = Some(lane.artifact_family.clone());
        fields.reason_code = lane
            .reason_code
            .clone()
            .or_else(|| match lane.status.as_str() {
                "success" => None,
                "empty" => Some("no_candidates".to_string()),
                status => Some(format!("provider_{status}")),
            });
        fields.duration_ms = lane.duration_ms.map(|value| value.max(0.0));
        fields.measurements.push(ContextEventMeasurement {
            name: "upstream_candidates".to_string(),
            value: MeasurementValue::Integer(lane.upstream_count.max(lane.candidate_count).max(0)),
            unit: "candidates".to_string(),
        });
        fields
            .links
            .push(lifecycle_link("terminal_of", &lane_expected.event_id));
        let lane_terminal = builder.add(
            &format!("{prefix}-terminal"),
            &lane.provider,
            "provider.terminal",
            &lane.status,
            fields,
        )?;
        if matches!(lane.status.as_str(), "success" | "empty") {
            for (index, candidate) in lane.candidates.iter().enumerate() {
                if !matches!(candidate.artifact_id.strip_prefix("memory.").or_else(|| candidate.artifact_id.strip_prefix("artifact.")), Some(digest) if lowercase_hex(digest, &[64]))
                {
                    return Err(invalid(
                        "provider_results.candidates",
                        "contains an invalid artifact id",
                    ));
                }
                let mut fields = lifecycle_fields(Some(lane_terminal.span_id.clone()));
                fields.artifact_family = Some(lane.artifact_family.clone());
                fields.artifact_id = Some(candidate.artifact_id.clone());
                fields.quantity = Some(1);
                let retrieved = builder.add(
                    &format!("{prefix}-candidate-{index}-retrieved"),
                    &lane.provider,
                    "candidate.retrieved",
                    "success",
                    fields,
                )?;
                if candidate.admitted {
                    let mut fields = lifecycle_fields(Some(retrieved.span_id.clone()));
                    fields.artifact_family = Some(lane.artifact_family.clone());
                    fields.artifact_id = Some(candidate.artifact_id.clone());
                    fields.quantity = Some(1);
                    fields
                        .links
                        .push(lifecycle_link("candidate_of", &retrieved.event_id));
                    let admitted_event = builder.add(
                        &format!("{prefix}-candidate-{index}-admitted"),
                        &lane.provider,
                        "candidate.admitted",
                        "success",
                        fields,
                    )?;
                    if candidate.delivered {
                        let mut fields = lifecycle_fields(Some(admitted_event.span_id.clone()));
                        fields.artifact_family = Some(lane.artifact_family.clone());
                        fields.artifact_id = Some(candidate.artifact_id.clone());
                        fields.quantity = Some(1);
                        let rendered = builder.add(
                            &format!("{prefix}-candidate-{index}-rendered"),
                            &lane.provider,
                            "block.rendered",
                            "success",
                            fields,
                        )?;
                        let mut fields = lifecycle_fields(Some(rendered.span_id.clone()));
                        fields.artifact_family = Some(lane.artifact_family.clone());
                        fields.artifact_id = Some(candidate.artifact_id.clone());
                        fields.quantity = Some(1);
                        fields
                            .links
                            .push(lifecycle_link("delivered_as", &rendered.event_id));
                        builder.add(
                            &format!("{prefix}-candidate-{index}-delivered"),
                            &lane.provider,
                            "block.delivered",
                            "success",
                            fields,
                        )?;
                    }
                } else {
                    let mut fields = lifecycle_fields(Some(retrieved.span_id.clone()));
                    fields.artifact_family = Some(lane.artifact_family.clone());
                    fields.artifact_id = Some(candidate.artifact_id.clone());
                    fields.quantity = Some(1);
                    fields.reason_code = Some("admission_filtered".to_string());
                    fields
                        .links
                        .push(lifecycle_link("candidate_of", &retrieved.event_id));
                    builder.add(
                        &format!("{prefix}-candidate-{index}-filtered"),
                        &lane.provider,
                        "candidate.filtered",
                        "filtered",
                        fields,
                    )?;
                }
            }
            if lane.candidates.is_empty() {
                let mut fields = lifecycle_fields(Some(lane_terminal.span_id.clone()));
                fields.artifact_family = Some(lane.artifact_family.clone());
                fields.quantity = Some(0);
                fields.reason_code = Some("no_candidates".to_string());
                builder.add(
                    &format!("{prefix}-retrieved-summary"),
                    &lane.provider,
                    "candidate.retrieved",
                    "empty",
                    fields,
                )?;
            }
        }
    }

    if let Some(legacy_status) = intent.legacy_status.as_deref() {
        let mut fields = lifecycle_fields(Some(turn.span_id.clone()));
        fields.artifact_family = Some("memory".to_string());
        let legacy_expected = builder.add(
            "legacy-expected",
            "legacy",
            "provider.expected",
            "expected",
            fields,
        )?;
        let mut fields = lifecycle_fields(Some(legacy_expected.span_id.clone()));
        fields.artifact_family = Some("memory".to_string());
        fields
            .links
            .push(lifecycle_link("attempt_of", &legacy_expected.event_id));
        builder.add(
            "legacy-started",
            "legacy",
            "provider.started",
            "started",
            fields,
        )?;
        let mut fields = lifecycle_fields(Some(legacy_expected.span_id.clone()));
        fields.artifact_family = Some("memory".to_string());
        fields.reason_code =
            (legacy_status != "success").then(|| format!("legacy_{legacy_status}"));
        fields
            .links
            .push(lifecycle_link("terminal_of", &legacy_expected.event_id));
        builder.add(
            "legacy-terminal",
            "legacy",
            "provider.terminal",
            legacy_status,
            fields,
        )?;
    }

    if intent.federation_status != "skipped" && intent.provider_results.is_empty() {
        let mut fields = lifecycle_fields(Some(rightcontext_terminal.span_id.clone()));
        fields.quantity = Some(candidates);
        fields.reason_code = (candidates == 0).then(|| "no_candidates".to_string());
        builder.add(
            "candidate-retrieved",
            "rightcontext",
            "candidate.retrieved",
            if candidates > 0 { "success" } else { "empty" },
            fields,
        )?;
        let mut fields = lifecycle_fields(Some(rightcontext_terminal.span_id.clone()));
        fields.quantity = Some(admitted);
        fields.reason_code = (admitted == 0).then(|| "no_admitted_candidates".to_string());
        builder.add(
            "candidate-admitted",
            "rightcontext",
            "candidate.admitted",
            if admitted > 0 { "success" } else { "filtered" },
            fields,
        )?;
        let rejected = candidates - admitted;
        if rejected > 0 {
            let mut fields = lifecycle_fields(Some(rightcontext_terminal.span_id.clone()));
            fields.quantity = Some(rejected);
            fields.reason_code = Some("admission_filtered".to_string());
            builder.add(
                "candidate-filtered",
                "rightcontext",
                "candidate.filtered",
                "filtered",
                fields,
            )?;
        }
    }

    if delivered > 0 && intent.provider_results.is_empty() {
        let mut fields = lifecycle_fields(Some(turn.span_id.clone()));
        fields.artifact_family = Some(
            if intent.legacy_status.is_some() {
                "memory"
            } else {
                "context"
            }
            .to_string(),
        );
        fields.quantity = Some(delivered);
        fields.char_count = Some(delivered_chars);
        let delivery_provider = if intent.legacy_status.is_some() {
            "legacy"
        } else {
            "rightcontext"
        };
        let rendered = builder.add(
            "block-rendered",
            delivery_provider,
            "block.rendered",
            "success",
            fields,
        )?;
        let mut fields = lifecycle_fields(Some(rendered.span_id.clone()));
        fields.artifact_family = Some(
            if intent.legacy_status.is_some() {
                "memory"
            } else {
                "context"
            }
            .to_string(),
        );
        fields.quantity = Some(delivered);
        fields.char_count = Some(delivered_chars);
        fields
            .links
            .push(lifecycle_link("delivered_as", &rendered.event_id));
        builder.add(
            "block-delivered",
            delivery_provider,
            "block.delivered",
            "success",
            fields,
        )?;
    }
    if truncated > 0 {
        let mut fields = lifecycle_fields(Some(turn.span_id));
        fields.quantity = Some(truncated);
        fields.reason_code = Some("char_cap_truncated".to_string());
        builder.add(
            "delivery-truncated",
            "rightcontext",
            "delivery.failed",
            "filtered",
            fields,
        )?;
    }

    let terminal_count = builder
        .events
        .iter()
        .filter(|event| {
            matches!(
                event.phase.as_str(),
                "block.delivered"
                    | "candidate.contradicted"
                    | "candidate.ignored"
                    | "candidate.unknown"
                    | "candidate.used"
                    | "commit.terminal"
                    | "delivery.failed"
                    | "embedding.terminal"
                    | "export.terminal"
                    | "identity.collision"
                    | "outbox.shed"
                    | "peer_apply.terminal"
                    | "privacy.rejected"
                    | "provider.terminal"
                    | "reconcile.gap"
                    | "transform.terminal"
                    | "turn.outcome"
                    | "validation.terminal"
            )
        })
        .count();
    if builder.events.len() != intent.expected_event_count
        || terminal_count != intent.expected_terminal_count
    {
        return Err(invalid(
            "lifecycle_intent",
            "expanded event counts do not match",
        ));
    }
    Ok(ContextEventBatch {
        events: builder.events,
    })
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ContextEventReceipt {
    pub event_id: String,
    pub canonical_sha256: String,
    pub disposition: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ContextEventBatchReceipt {
    pub schema_version: u32,
    pub complete: bool,
    pub inserted: usize,
    pub duplicates: usize,
    pub events: Vec<ContextEventReceipt>,
}

/// Trusted local identity attached to the loopback ingest endpoint at service startup. This is
/// intentionally not deserializable from request JSON: peer evidence enters through a separate
/// typed replication path, never by impersonating the resident service.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContextIngestLease {
    installation_id: String,
    service_instance_id: String,
    startup_generation: u64,
    created_at: String,
    installation_epoch: i64,
    os_type: &'static str,
    arch: &'static str,
}

impl ContextIngestLease {
    pub fn from_startup(
        identity: &crate::installation_identity::InstallationIdentity,
        claim: &crate::installation_identity::StartupClaim,
    ) -> Result<Self, crate::installation_identity::InstallationIdentityError> {
        crate::installation_identity::validate_active_startup_claim(identity, claim)?;
        let installation_epoch = i64::try_from(identity.lineage.len())
            .ok()
            .and_then(|value| value.checked_add(1))
            .ok_or_else(|| {
                crate::installation_identity::InstallationIdentityError::Invalid(
                    "installation lineage exceeds the supported epoch range".to_string(),
                )
            })?;
        Ok(Self {
            installation_id: identity.installation_id.clone(),
            service_instance_id: claim.service_instance_id.clone(),
            startup_generation: claim.startup_generation,
            created_at: identity.created_at.clone(),
            installation_epoch,
            os_type: std::env::consts::OS,
            arch: std::env::consts::ARCH,
        })
    }

    fn bind_batch(&self, batch: &ContextEventBatch) -> Result<(), ContextTelemetryError> {
        for event in &batch.events {
            if event.installation_id != self.installation_id
                || event.service_instance_id != self.service_instance_id
            {
                return Err(ContextTelemetryError::AttributionMismatch);
            }
        }
        Ok(())
    }

    fn bind_local_batch(&self, batch: &ContextEventBatch) -> Result<(), ContextTelemetryError> {
        for event in &batch.events {
            if event.installation_id != self.installation_id {
                return Err(ContextTelemetryError::AttributionMismatch);
            }
        }
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ContextTelemetryError {
    #[error("invalid context telemetry event: {0}")]
    Invalid(String),
    #[error("event_id conflicts with an existing canonical event: {event_id}")]
    Conflict { event_id: String },
    #[error("context telemetry attribution does not match the active service lease")]
    AttributionMismatch,
    #[error("context telemetry storage unavailable")]
    Database(#[from] rusqlite::Error),
}

/// Verify canonical hashes and the append chain. Legacy `memory_event_log` is intentionally not
/// covered by this API and remains reported as an unsealed compatibility stream.
pub fn verify_context_event_integrity(
    conn: &rusqlite::Connection,
) -> Result<usize, ContextTelemetryError> {
    let logs: i64 = conn.query_row("SELECT COUNT(*) FROM context_event_log", [], |r| r.get(0))?;
    let sealed: i64 = conn.query_row("SELECT COUNT(*) FROM context_event_integrity", [], |r| {
        r.get(0)
    })?;
    if logs != sealed {
        return Err(invalid(
            "context_event_log",
            "integrity coverage is incomplete",
        ));
    }
    let mut receipts = conn.prepare("SELECT receipt_id,segment_key,first_removed_seq,last_removed_seq,removed_count,removed_sha256,prior_chain_sha256,anchor_chain_sha256 FROM context_event_retention_receipt")?;
    for row in receipts.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, i64>(2)?,
            r.get::<_, i64>(3)?,
            r.get::<_, i64>(4)?,
            r.get::<_, String>(5)?,
            r.get::<_, String>(6)?,
            r.get::<_, String>(7)?,
        ))
    })? {
        let (id, segment, first, last, count, removed_sha, prior, anchor) = row?;
        let expected = sha256_text(&format!(
            "{segment}:{first}:{last}:{count}:{removed_sha}:{prior}:{anchor}"
        ));
        if id != expected {
            return Err(invalid(
                "context_event_retention_receipt",
                "integrity verification failed",
            ));
        }
    }
    let segment_rows: i64 =
        conn.query_row("SELECT COUNT(*) FROM context_event_segment", [], |row| {
            row.get(0)
        })?;
    let distinct_segments: i64 = conn.query_row(
        "SELECT COUNT(DISTINCT segment_key) FROM context_event_integrity",
        [],
        |row| row.get(0),
    )?;
    if segment_rows != distinct_segments {
        return Err(invalid(
            "context_event_segment",
            "integrity coverage is incomplete",
        ));
    }
    let mut segs = conn.prepare(
        "SELECT segment_key,first_seq,last_seq,event_count,content_sha256,sealed_at
         FROM context_event_segment",
    )?;
    for row in segs.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, i64>(2)?,
            row.get::<_, i64>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, Option<String>>(5)?,
        ))
    })? {
        let (segment, first, last, expected, content, sealed_at) = row?;
        let (actual, actual_first, actual_last): (i64, Option<i64>, Option<i64>) = conn.query_row(
            "SELECT COUNT(*),MIN(seq),MAX(seq) FROM context_event_integrity
                 WHERE segment_key=?1",
            [&segment],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        let (last_ordinal, last_chain): (i64, String) = conn.query_row(
            "SELECT ordinal,chain_sha256 FROM context_event_integrity
             WHERE segment_key=?1 ORDER BY seq DESC LIMIT 1",
            [&segment],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let expected_content = sha256_text(&format!("{segment}:{last_ordinal}:{last_chain}"));
        if actual != expected
            || actual_first != Some(first)
            || actual_last != Some(last)
            || content != expected_content
            || (expected == 256) != sealed_at.is_some()
        {
            return Err(invalid(
                "context_event_segment",
                "integrity coverage is incomplete",
            ));
        }
    }
    let mut stmt = conn.prepare("SELECT i.seq,e.event_id,e.canonical_sha256,i.canonical_sha256,i.prev_chain_sha256,i.chain_sha256,i.segment_key,i.ordinal FROM context_event_log e JOIN context_event_integrity i ON i.seq=e.seq ORDER BY e.seq")?;
    let mut rows = stmt.query([])?;
    let mut previous: Option<(String, i64, String)> = None;
    let mut count = 0;
    while let Some(row) = rows.next()? {
        let _seq: i64 = row.get(0)?;
        let event_id: String = row.get(1)?;
        let canonical: String = row.get(2)?;
        let sealed_canonical: String = row.get(3)?;
        let prev: String = row.get(4)?;
        let chain: String = row.get(5)?;
        let segment: String = row.get(6)?;
        let ordinal: i64 = row.get(7)?;
        if canonical != sealed_canonical || canonical_row_hash(conn, _seq)? != canonical {
            return Err(invalid(
                "context_event_log",
                "integrity verification failed",
            ));
        }
        if previous.as_ref().is_some_and(|(s, o, _)| *s == segment && *o + 1 != ordinal)
            || previous.as_ref().is_some_and(|(_, _, c)| *c != prev)
            || (previous.as_ref().is_some_and(|(s, _, _)| *s != segment) && ordinal != 1)
            || (previous.is_none() && prev != INTEGRITY_GENESIS && conn.query_row("SELECT COUNT(*) FROM context_event_retention_receipt WHERE anchor_chain_sha256=?1", [&prev], |r| r.get::<_, i64>(0)).unwrap_or(0) == 0) {
            return Err(invalid("context_event_log", "integrity verification failed"));
        }
        if chain_digest(&segment, ordinal, &event_id, &canonical, &prev) != chain {
            return Err(invalid(
                "context_event_log",
                "integrity verification failed",
            ));
        }
        previous = Some((segment, ordinal, chain));
        count += 1;
    }
    Ok(count)
}

fn canonical_row_hash(
    conn: &rusqlite::Connection,
    seq: i64,
) -> Result<String, ContextTelemetryError> {
    let raw: String = conn.query_row(
        r#"SELECT json_object(
            'event_id', event_id, 'schema_version', schema_version, 'ts', ts,
            'installation_id', installation_id, 'service_instance_id', service_instance_id,
            'workspace_id', workspace_id, 'client', client, 'client_version', client_version,
            'producer', producer, 'producer_version', producer_version, 'session_id', session_id,
            'turn_id', turn_id, 'trace_id', trace_id, 'span_id', span_id,
            'parent_span_id', parent_span_id, 'artifact_family', artifact_family,
            'provider', provider, 'provider_version', provider_version,
            'release_generation', release_generation, 'phase', phase, 'operation', operation,
            'status', status, 'reason_code', reason_code, 'scope_id', scope_id,
            'artifact_id', artifact_id, 'artifact_sha256', artifact_sha256,
            'traffic_class', traffic_class, 'policy_version', policy_version,
            'policy_activation_sha256', policy_activation_sha256, 'cohort', cohort,
            'task_class', task_class, 'source_generation', source_generation,
            'duration_ms', duration_ms, 'quantity', quantity, 'token_count', token_count,
            'char_count', char_count,
            'meta', CASE WHEN meta='{}' THEN NULL ELSE json(meta) END,
            'measurements', json(COALESCE((
                SELECT json_group_array(json(item)) FROM (
                    SELECT CASE m.value_type
                        WHEN 'integer' THEN json_object('name',m.name,'value',m.value_integer,'unit',m.unit)
                        WHEN 'real' THEN json_object('name',m.name,'value',m.value_real,'unit',m.unit)
                        ELSE json_object('name',m.name,'value',json(CASE WHEN m.value_boolean=1 THEN 'true' ELSE 'false' END),'unit',m.unit)
                    END AS item
                    FROM context_event_measurement m
                    WHERE m.event_id=context_event_log.event_id
                    ORDER BY m.rowid
                )
            ), '[]')),
            'links', json(COALESCE((
                SELECT json_group_array(json(item)) FROM (
                    SELECT json_object('relation',l.relation,'target_event_id',l.target_event_id) AS item
                    FROM context_event_link l
                    WHERE l.event_id=context_event_log.event_id
                    ORDER BY l.rowid
                )
            ), '[]'))
        ) FROM context_event_log WHERE seq=?1"#,
        [seq],
        |row| row.get(0),
    )?;
    let mut value: Value = serde_json::from_str(&raw)
        .map_err(|_| invalid("context_event_log", "cannot reconstruct canonical event"))?;
    if let Some(object) = value.as_object_mut() {
        object.retain(|_, value| !value.is_null());
    }
    let event: ContextEvent = serde_json::from_value(value)
        .map_err(|_| invalid("context_event_log", "cannot reconstruct canonical event"))?;
    Ok(format!(
        "{:x}",
        Sha256::digest(canonical_event_bytes(&event)?)
    ))
}

/// Remove one complete sealed segment, leaving a hash receipt as its retention anchor.
pub fn truncate_sealed_context_segment(
    conn: &mut rusqlite::Connection,
    segment: &str,
) -> Result<String, ContextTelemetryError> {
    verify_context_event_integrity(conn)?;
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let (first, last, content, sealed): (i64, i64, String, Option<String>) = tx.query_row(
        "SELECT first_seq,last_seq,content_sha256,sealed_at
         FROM context_event_segment WHERE segment_key=?1",
        [segment],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    )?;
    if sealed.is_none() {
        return Err(ContextTelemetryError::Invalid(
            "segment is not sealed".into(),
        ));
    }
    let (removed, prior): (i64, String) = tx.query_row("SELECT COUNT(*),COALESCE((SELECT chain_sha256 FROM context_event_integrity WHERE segment_key=?1 ORDER BY seq DESC LIMIT 1),'') FROM context_event_integrity WHERE segment_key=?1", [segment], |r| Ok((r.get(0)?, r.get(1)?)))?;
    let anchor: String = tx.query_row("SELECT COALESCE(prev_chain_sha256,'') FROM context_event_integrity WHERE seq>?1 ORDER BY seq LIMIT 1", [last], |r| r.get(0)).optional()?.unwrap_or_default();
    if anchor.is_empty() {
        return Err(ContextTelemetryError::Invalid(
            "cannot truncate newest segment".into(),
        ));
    }
    if !anchor.is_empty() && anchor != prior {
        return Err(ContextTelemetryError::Invalid(
            "retention anchor mismatch".into(),
        ));
    }
    let receipt = sha256_text(&format!(
        "{segment}:{first}:{last}:{removed}:{content}:{prior}:{anchor}"
    ));
    tx.execute("INSERT INTO context_event_retention_receipt(receipt_id,segment_key,first_removed_seq,last_removed_seq,removed_count,removed_sha256,prior_chain_sha256,anchor_chain_sha256,created_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)", params![receipt,segment,first,last,removed,content,prior,anchor,crate::time::now_iso()])?;
    tx.execute("DELETE FROM context_event_measurement WHERE event_id IN (SELECT event_id FROM context_event_log WHERE seq BETWEEN ?1 AND ?2)", params![first,last])?;
    tx.execute("DELETE FROM context_event_link WHERE event_id IN (SELECT event_id FROM context_event_log WHERE seq BETWEEN ?1 AND ?2)", params![first,last])?;
    tx.execute(
        "DELETE FROM context_event_integrity WHERE segment_key=?1",
        [segment],
    )?;
    tx.execute(
        "DELETE FROM context_event_log WHERE seq BETWEEN ?1 AND ?2",
        params![first, last],
    )?;
    tx.execute(
        "DELETE FROM context_event_segment WHERE segment_key=?1",
        [segment],
    )?;
    tx.commit()?;
    Ok(receipt)
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PromptIngressDrain {
    pub records: usize,
    pub inserted: usize,
    pub duplicates: usize,
    pub rejected: usize,
    pub cursor: u64,
}

#[derive(Clone)]
struct PreparedEvent<'a> {
    event: &'a ContextEvent,
    canonical_sha256: String,
    meta_json: String,
}

fn invalid(field: &str, requirement: &str) -> ContextTelemetryError {
    ContextTelemetryError::Invalid(format!("{field} {requirement}"))
}

fn has_token_tail(value: &str, lowercase: bool, allow_plus: bool) -> bool {
    let mut bytes = value.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    let valid_alnum = |byte: u8| {
        if lowercase {
            byte.is_ascii_lowercase() || byte.is_ascii_digit()
        } else {
            byte.is_ascii_alphanumeric()
        }
    };
    valid_alnum(first)
        && bytes.all(|byte| {
            valid_alnum(byte)
                || matches!(byte, b'-' | b'_' | b'.' | b':')
                || (allow_plus && byte == b'+')
        })
}

fn lowercase_hex(value: &str, lengths: &[usize]) -> bool {
    lengths.contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn prefixed_digest(value: &str, prefix: &str, separator: char, lengths: &[usize]) -> bool {
    value
        .strip_prefix(&format!("{prefix}{separator}"))
        .is_some_and(|digest| lowercase_hex(digest, lengths))
}

fn require_service_id(value: &str, field: &str) -> Result<(), ContextTelemetryError> {
    if valid_uuid(value) || prefixed_digest(value, "svc", '-', &[32]) {
        return Ok(());
    }
    Err(invalid(field, "must be a UUID or opaque service digest"))
}

fn require_workspace_id(value: &str, field: &str) -> Result<(), ContextTelemetryError> {
    if prefixed_digest(value, "ws", '.', &[32]) {
        return Ok(());
    }
    Err(invalid(field, "must be an opaque workspace digest"))
}

fn require_correlation_id(
    value: &str,
    field: &str,
    prefix: &str,
) -> Result<(), ContextTelemetryError> {
    if valid_uuid(value) || prefixed_digest(value, prefix, '-', &[32]) {
        return Ok(());
    }
    Err(invalid(
        field,
        "must be a UUID or field-specific opaque digest",
    ))
}

fn require_scope_id(value: &str, field: &str) -> Result<(), ContextTelemetryError> {
    if valid_uuid(value)
        || prefixed_digest(value, "scope", '-', &[32, 64])
        || prefixed_digest(value, "scope", '.', &[32, 64])
    {
        return Ok(());
    }
    Err(invalid(field, "must be an opaque scope digest"))
}

fn require_artifact_id(value: &str, field: &str) -> Result<(), ContextTelemetryError> {
    if valid_uuid(value) {
        return Ok(());
    }
    let valid = value.split_once('.').is_some_and(|(namespace, digest)| {
        !namespace.is_empty()
            && namespace.len() <= 32
            && namespace.as_bytes()[0].is_ascii_lowercase()
            && namespace.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
            })
            && lowercase_hex(digest, &[32, 64])
    });
    if valid {
        return Ok(());
    }
    Err(invalid(field, "must be an opaque artifact digest"))
}

fn require_opaque_reference(value: &str, field: &str) -> Result<(), ContextTelemetryError> {
    if valid_uuid(value) {
        return Ok(());
    }
    let safe = (32..=160).contains(&value.len())
        && value.as_bytes()[0].is_ascii_alphabetic()
        && has_token_tail(value, false, false);
    if !safe {
        return Err(invalid(field, "must be a cryptographic opaque reference"));
    }
    if value
        .split(['.', ':', '-'])
        .any(|segment| lowercase_hex(segment, &[32, 64]))
    {
        return Ok(());
    }
    let bytes = value.as_bytes();
    let embedded_uuid = (0..=bytes.len().saturating_sub(36)).any(|start| {
        let end = start + 36;
        (start == 0 || matches!(bytes[start - 1], b'.' | b':' | b'-'))
            && (end == bytes.len() || matches!(bytes[end], b'.' | b':' | b'-'))
            && valid_uuid(&value[start..end])
    });
    if embedded_uuid {
        return Ok(());
    }
    Err(invalid(field, "must be a cryptographic opaque reference"))
}

fn require_lower_token(value: &str, field: &str) -> Result<(), ContextTelemetryError> {
    if value.len() > 80 || !has_token_tail(value, true, false) {
        return Err(invalid(field, "must be a bounded lowercase token"));
    }
    Ok(())
}

fn optional_lower_token(value: Option<&str>, field: &str) -> Result<(), ContextTelemetryError> {
    if let Some(value) = value {
        require_lower_token(value, field)?;
    }
    Ok(())
}

fn optional_version(value: Option<&str>, field: &str) -> Result<(), ContextTelemetryError> {
    if value.is_some_and(|value| value.len() > 80 || !has_token_tail(value, false, true)) {
        return Err(invalid(field, "must be a bounded version token"));
    }
    Ok(())
}

fn require_registered(
    value: &str,
    field: &str,
    registry: &[&str],
) -> Result<(), ContextTelemetryError> {
    if !registry.contains(&value) {
        return Err(invalid(field, "is not registered"));
    }
    Ok(())
}

fn valid_namespaced_extension(value: &str) -> bool {
    let Some(rest) = value.strip_prefix("ext.") else {
        return false;
    };
    let Some((namespace, name)) = rest.split_once('.') else {
        return false;
    };
    if namespace.is_empty()
        || namespace.len() > 32
        || name.is_empty()
        || name.len() > 47
        || !namespace.as_bytes()[0].is_ascii_lowercase()
            && !namespace.as_bytes()[0].is_ascii_digit()
        || !name.as_bytes()[0].is_ascii_lowercase() && !name.as_bytes()[0].is_ascii_digit()
    {
        return false;
    }
    namespace.bytes().all(|byte| {
        byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-')
    }) && name.bytes().all(|byte| {
        byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
    })
}

fn require_context_registry_token(
    value: &str,
    field: &str,
    registry: &HashSet<String>,
) -> Result<(), ContextTelemetryError> {
    if registry.contains(value) || valid_namespaced_extension(value) {
        return Ok(());
    }
    Err(invalid(field, "is not registered"))
}

fn valid_uuid(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 36
        && bytes.get(8) == Some(&b'-')
        && bytes.get(13) == Some(&b'-')
        && bytes.get(18) == Some(&b'-')
        && bytes.get(23) == Some(&b'-')
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| matches!(index, 8 | 13 | 18 | 23) || byte.is_ascii_hexdigit())
        && bytes
            .get(14)
            .is_some_and(|byte| matches!(byte.to_ascii_lowercase(), b'1'..=b'8'))
        && bytes
            .get(19)
            .is_some_and(|byte| matches!(byte.to_ascii_lowercase(), b'8' | b'9' | b'a' | b'b'))
}

fn valid_utc_timestamp(value: &str) -> bool {
    if value.len() < 20 || value.len() > 40 || !value.ends_with('Z') {
        return false;
    }
    let bytes = value.as_bytes();
    if bytes.get(4) != Some(&b'-')
        || bytes.get(7) != Some(&b'-')
        || bytes.get(10) != Some(&b'T')
        || bytes.get(13) != Some(&b':')
        || bytes.get(16) != Some(&b':')
    {
        return false;
    }
    let digit = |range: std::ops::Range<usize>| {
        bytes
            .get(range)
            .is_some_and(|part| part.iter().all(u8::is_ascii_digit))
    };
    if !digit(0..4)
        || !digit(5..7)
        || !digit(8..10)
        || !digit(11..13)
        || !digit(14..16)
        || !digit(17..19)
    {
        return false;
    }
    let number = |range: std::ops::Range<usize>| {
        std::str::from_utf8(&bytes[range])
            .ok()
            .and_then(|part| part.parse::<u32>().ok())
            .unwrap_or(u32::MAX)
    };
    if !(1..=12).contains(&number(5..7))
        || !(1..=31).contains(&number(8..10))
        || number(11..13) > 23
        || number(14..16) > 59
        || number(17..19) > 60
    {
        return false;
    }
    if value.len() == 20 {
        return true;
    }
    bytes.get(19) == Some(&b'.')
        && bytes[20..bytes.len() - 1].iter().all(u8::is_ascii_digit)
        && bytes.len() > 21
}

fn validate_sha256(value: &str, field: &str) -> Result<(), ContextTelemetryError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(invalid(field, "must be a lowercase SHA-256 hex digest"));
    }
    Ok(())
}

fn validate_nonnegative(value: Option<i64>, field: &str) -> Result<(), ContextTelemetryError> {
    if value.is_some_and(|number| number < 0) {
        return Err(invalid(field, "must be nonnegative"));
    }
    Ok(())
}

fn validate_meta(meta: &BTreeMap<String, Value>) -> Result<(), ContextTelemetryError> {
    if meta.len() > 12 {
        return Err(invalid("meta", "must contain at most 12 fields"));
    }
    const BOOLEAN_KEYS: &[&str] = &["cache_hit", "complete", "reconciled"];
    const NUMERIC_KEYS: &[&str] = &[
        "retry_count",
        "queue_depth",
        "shed_count",
        "installation_generation",
    ];
    // Bounded taxonomy label carried alongside an observable event so it can be filtered on read
    // (see `query_observable_events`). `lifecycle_event_type_token` is the only producer and
    // always emits a value matching this shape, so this is purely additive: it never rejects a
    // meta map that validated before this key existed.
    const STRING_TOKEN_KEYS: &[&str] = &["event_type"];
    for (key, value) in meta {
        if BOOLEAN_KEYS.contains(&key.as_str()) && value.is_boolean() {
            continue;
        }
        if NUMERIC_KEYS.contains(&key.as_str()) && value.as_i64().is_some_and(|number| number >= 0)
        {
            continue;
        }
        if key == "transport" {
            let allowed = &["local", "http", "cli", "git", "live", "anchor"];
            if value
                .as_str()
                .is_some_and(|candidate| allowed.contains(&candidate))
            {
                continue;
            }
        }
        if STRING_TOKEN_KEYS.contains(&key.as_str())
            && value.as_str().is_some_and(|candidate| {
                candidate.len() <= 80 && has_token_tail(candidate, true, false)
            })
        {
            continue;
        }
        return Err(invalid(
            "meta",
            "contains an unregistered key or value type",
        ));
    }
    Ok(())
}

fn validate_event(event: &ContextEvent) -> Result<(), ContextTelemetryError> {
    if event.schema_version != EVENT_SCHEMA_VERSION {
        return Err(invalid("schema_version", "is unsupported"));
    }
    require_opaque_reference(&event.event_id, "event_id")?;
    if !valid_utc_timestamp(&event.ts) {
        return Err(invalid("ts", "must be an ISO-8601 UTC timestamp"));
    }
    if !valid_uuid(&event.installation_id) {
        return Err(invalid("installation_id", "must be a UUID"));
    }
    require_service_id(&event.service_instance_id, "service_instance_id")?;
    require_workspace_id(&event.workspace_id, "workspace_id")?;
    require_correlation_id(&event.session_id, "session_id", "session")?;
    require_correlation_id(&event.trace_id, "trace_id", "trace")?;
    require_correlation_id(&event.span_id, "span_id", "span")?;
    if let Some(value) = event.turn_id.as_deref() {
        require_correlation_id(value, "turn_id", "turn")?;
    }
    if let Some(value) = event.parent_span_id.as_deref() {
        require_correlation_id(value, "parent_span_id", "span")?;
    }
    if let Some(value) = event.scope_id.as_deref() {
        require_scope_id(value, "scope_id")?;
    }
    if let Some(value) = event.artifact_id.as_deref() {
        require_artifact_id(value, "artifact_id")?;
    }
    optional_version(event.policy_version.as_deref(), "policy_version")?;
    optional_version(event.release_generation.as_deref(), "release_generation")?;
    for (value, field) in [
        (event.client.as_str(), "client"),
        (event.producer.as_str(), "producer"),
    ] {
        require_lower_token(value, field)?;
    }
    let registry = context_registry();
    require_context_registry_token(
        &event.artifact_family,
        "artifact_family",
        &registry.artifact_families,
    )?;
    require_context_registry_token(&event.provider, "provider", &registry.providers)?;
    for (value, field) in [
        (event.reason_code.as_deref(), "reason_code"),
        (event.cohort.as_deref(), "cohort"),
        (event.task_class.as_deref(), "task_class"),
    ] {
        optional_lower_token(value, field)?;
    }
    for (value, field) in [
        (event.client_version.as_deref(), "client_version"),
        (event.producer_version.as_deref(), "producer_version"),
        (event.provider_version.as_deref(), "provider_version"),
    ] {
        optional_version(value, field)?;
    }
    if !registry.phases.contains_key(&event.phase) {
        return Err(invalid("phase", "is not registered"));
    }
    require_registered(&event.operation, "operation", OPERATIONS)?;
    require_registered(&event.status, "status", STATUSES)?;
    if TERMINAL_REASON_STATUSES.contains(&event.status.as_str()) && event.reason_code.is_none() {
        return Err(invalid(
            "reason_code",
            "is required for a non-success terminal status",
        ));
    }
    if !matches!(
        event.traffic_class.as_str(),
        "production" | "smoke" | "eval"
    ) {
        return Err(invalid("traffic_class", "is not registered"));
    }
    if let Some(hash) = event.artifact_sha256.as_deref() {
        validate_sha256(hash, "artifact_sha256")?;
    }
    if let Some(hash) = event.policy_activation_sha256.as_deref() {
        validate_sha256(hash, "policy_activation_sha256")?;
    }
    if event.duration_ms.is_some_and(|duration| {
        !duration.is_finite() || !(0.0..=MAX_DURATION_MS).contains(&duration)
    }) {
        return Err(invalid("duration_ms", "must be finite and bounded"));
    }
    validate_nonnegative(event.quantity, "quantity")?;
    validate_nonnegative(event.token_count, "token_count")?;
    validate_nonnegative(event.char_count, "char_count")?;
    validate_nonnegative(event.source_generation, "source_generation")?;
    if let Some(meta) = event.meta.as_ref() {
        validate_meta(meta)?;
    }

    if event.measurements.len() > MAX_CHILDREN {
        return Err(invalid("measurements", "must contain at most 32 items"));
    }
    let mut measurement_names = HashSet::new();
    for measurement in &event.measurements {
        require_lower_token(&measurement.name, "measurement.name")?;
        require_registered(&measurement.unit, "measurement.unit", MEASUREMENT_UNITS)?;
        if !measurement_names.insert(measurement.name.as_str()) {
            return Err(invalid("measurements", "contains a duplicate name"));
        }
        if let MeasurementValue::Real(value) = measurement.value {
            if !value.is_finite() {
                return Err(invalid("measurement.value", "must be finite and bounded"));
            }
        }
        if measurement.unit == "ms" {
            let valid = match measurement.value {
                MeasurementValue::Integer(value) => value >= 0 && value <= MAX_DURATION_MS as i64,
                MeasurementValue::Real(value) => {
                    value.is_finite() && (0.0..=MAX_DURATION_MS).contains(&value)
                }
                MeasurementValue::Boolean(_) => false,
            };
            if !valid {
                return Err(invalid(
                    "measurement.value",
                    "must be between zero and the millisecond maximum",
                ));
            }
        }
    }

    if event.links.len() > MAX_CHILDREN {
        return Err(invalid("links", "must contain at most 32 items"));
    }
    let mut links = HashSet::new();
    for link in &event.links {
        require_registered(&link.relation, "link.relation", RELATIONS)?;
        require_opaque_reference(&link.target_event_id, "link.target_event_id")?;
        if !links.insert((link.relation.as_str(), link.target_event_id.as_str())) {
            return Err(invalid("links", "contains a duplicate relation target"));
        }
    }
    Ok(())
}

/// Decode and validate one raw JSON event without returning serde diagnostics that could echo
/// content-bearing input. This is the public Rust boundary corresponding to the shared schema.
pub fn parse_context_event(value: Value) -> Result<ContextEvent, ContextTelemetryError> {
    let event = serde_json::from_value::<ContextEvent>(value)
        .map_err(|_| invalid("event", "does not match the shared schema"))?;
    validate_context_event(&event)?;
    Ok(event)
}

/// Validate a typed event against the finite, content-free shared contract.
pub fn validate_context_event(event: &ContextEvent) -> Result<(), ContextTelemetryError> {
    validate_event(event)
}

fn sort_json_recursively(value: Value) -> Value {
    match value {
        Value::Array(values) => {
            Value::Array(values.into_iter().map(sort_json_recursively).collect())
        }
        Value::Object(values) => {
            let mut entries: Vec<_> = values.into_iter().collect();
            entries.sort_by(|left, right| left.0.cmp(&right.0));
            let mut sorted = serde_json::Map::new();
            for (key, value) in entries {
                sorted.insert(key, sort_json_recursively(value));
            }
            Value::Object(sorted)
        }
        scalar => scalar,
    }
}

/// Validate and encode one event using stable recursive object-key ordering. Array order is part
/// of the shared contract and is therefore preserved.
pub fn canonical_event_bytes(event: &ContextEvent) -> Result<Vec<u8>, ContextTelemetryError> {
    validate_context_event(event)?;
    let value = serde_json::to_value(event)
        .map_err(|_| invalid("event", "could not be canonically serialized"))?;
    serde_json::to_vec(&sort_json_recursively(value))
        .map_err(|_| invalid("event", "could not be canonically serialized"))
}

fn prepare_batch(
    batch: &ContextEventBatch,
) -> Result<Vec<PreparedEvent<'_>>, ContextTelemetryError> {
    if batch.events.is_empty() || batch.events.len() > MAX_BATCH_EVENTS {
        return Err(invalid("events", "must contain between 1 and 256 items"));
    }
    let mut prepared = Vec::with_capacity(batch.events.len());
    let mut phase_counts: HashMap<(&str, &str, &str, &str), usize> = HashMap::new();
    for event in &batch.events {
        let canonical = canonical_event_bytes(event)?;
        let budget = context_registry()
            .phases
            .get(&event.phase)
            .ok_or_else(|| invalid("phase", "is not registered"))?;
        if canonical.len() > budget.max_bytes_per_event {
            return Err(invalid("phase", "exceeds phase byte budget"));
        }
        let turn = event.turn_id.as_deref().unwrap_or(&event.trace_id);
        let count = phase_counts
            .entry((
                &event.installation_id,
                &event.session_id,
                turn,
                &event.phase,
            ))
            .or_default();
        *count += 1;
        if *count > budget.max_events_per_turn {
            return Err(invalid("events", "exceeds phase turn cardinality"));
        }
        let canonical_sha256 = format!("{:x}", Sha256::digest(canonical));
        let meta_json = match event.meta.as_ref() {
            Some(meta) => serde_json::to_string(meta)
                .map_err(|_| invalid("meta", "could not be serialized"))?,
            None => "{}".to_string(),
        };
        prepared.push(PreparedEvent {
            event,
            canonical_sha256,
            meta_json,
        });
    }
    Ok(prepared)
}

/// Append a validated batch through an existing SQLite transaction/connection. Mutation owners
/// use this primitive so their durable state, legacy lifecycle row, and canonical lifecycle row
/// have one commit boundary. The public batch API below wraps the same primitive in its own
/// transaction.
pub fn append_context_events_on(
    conn: &rusqlite::Connection,
    batch: &ContextEventBatch,
) -> Result<ContextEventBatchReceipt, ContextTelemetryError> {
    let prepared = prepare_batch(batch)?;
    let has_event_table: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='context_event_log')",
        [],
        |row| row.get(0),
    )?;
    if !has_event_table {
        let batch_json = serde_json::to_string(batch)
            .map_err(|error| ContextTelemetryError::Invalid(error.to_string()))?;
        let batch_sha256 = sha256_text(&batch_json);
        conn.execute(
            "INSERT OR IGNORE INTO membrane_event_outbox
                 (batch_sha256, batch_json, created_at) VALUES (?1, ?2, ?3)",
            params![batch_sha256, batch_json, crate::time::now_iso()],
        )?;
        let events = prepared
            .into_iter()
            .map(|item| ContextEventReceipt {
                event_id: item.event.event_id.clone(),
                canonical_sha256: item.canonical_sha256,
                disposition: "queued".to_string(),
            })
            .collect::<Vec<_>>();
        return Ok(ContextEventBatchReceipt {
            schema_version: EVENT_SCHEMA_VERSION,
            complete: true,
            inserted: events.len(),
            duplicates: 0,
            events,
        });
    }
    let mut receipts = Vec::with_capacity(prepared.len());
    let mut inserted = 0usize;
    let mut duplicates = 0usize;

    for prepared_event in prepared {
        let event = prepared_event.event;
        let existing: Option<String> = conn
            .query_row(
                "SELECT canonical_sha256 FROM context_event_log WHERE event_id=?1",
                [&event.event_id],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(existing) = existing {
            if existing != prepared_event.canonical_sha256 {
                return Err(ContextTelemetryError::Conflict {
                    event_id: event.event_id.clone(),
                });
            }
            duplicates += 1;
            receipts.push(ContextEventReceipt {
                event_id: event.event_id.clone(),
                canonical_sha256: prepared_event.canonical_sha256,
                disposition: "duplicate".to_string(),
            });
            continue;
        }

        conn.execute(
            "INSERT INTO context_event_log (
                event_id, schema_version, ts, ingested_at, installation_id,
                service_instance_id, workspace_id, client, client_version, producer,
                producer_version, session_id, turn_id, trace_id, span_id, parent_span_id,
                artifact_family, provider, provider_version, release_generation, phase, operation, status,
                reason_code, scope_id, artifact_id, artifact_sha256, traffic_class,
                policy_version, policy_activation_sha256, cohort, task_class, source_generation, duration_ms, quantity,
                token_count, char_count, meta, canonical_sha256
             ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
                ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28,
                ?29, ?30, ?31, ?32, ?33, ?34, ?35, ?36, ?37, ?38, ?39
             )",
            params![
                event.event_id,
                event.schema_version,
                event.ts,
                crate::time::now_iso(),
                event.installation_id,
                event.service_instance_id,
                event.workspace_id,
                event.client,
                event.client_version,
                event.producer,
                event.producer_version,
                event.session_id,
                event.turn_id,
                event.trace_id,
                event.span_id,
                event.parent_span_id,
                event.artifact_family,
                event.provider,
                event.provider_version,
                event.release_generation,
                event.phase,
                event.operation,
                event.status,
                event.reason_code,
                event.scope_id,
                event.artifact_id,
                event.artifact_sha256,
                event.traffic_class,
                event.policy_version,
                event.policy_activation_sha256,
                event.cohort,
                event.task_class,
                event.source_generation,
                event.duration_ms,
                event.quantity,
                event.token_count,
                event.char_count,
                prepared_event.meta_json,
                prepared_event.canonical_sha256,
            ],
        )?;
        let seq = conn.last_insert_rowid();
        let active: Option<(String, i64, String)> = conn.query_row(
            "SELECT segment_key, ordinal, chain_sha256 FROM context_event_integrity ORDER BY seq DESC LIMIT 1",
            [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).optional()?;
        let (segment, ordinal, prev) = match active {
            Some((segment, ordinal, prev)) if ordinal < 256 => (segment, ordinal + 1, prev),
            Some((_, _, prev)) => (format!("seg-{seq}"), 1, prev),
            _ => (format!("seg-{seq}"), 1, INTEGRITY_GENESIS.to_string()),
        };
        let chain = chain_digest(
            &segment,
            ordinal,
            &event.event_id,
            &prepared_event.canonical_sha256,
            &prev,
        );
        conn.execute(
            "INSERT INTO context_event_integrity(seq,event_id,canonical_sha256,prev_chain_sha256,chain_sha256,segment_key,ordinal)
             VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![seq, event.event_id, prepared_event.canonical_sha256, prev, chain, segment, ordinal],
        )?;
        let content = segment_digest(
            &segment,
            ordinal,
            &event.event_id,
            &prepared_event.canonical_sha256,
            &prev,
        );
        conn.execute(
            "INSERT INTO context_event_segment(segment_key,first_seq,last_seq,event_count,content_sha256,sealed_at)
             VALUES (?1,?2,?2,1,?3,CASE WHEN ?4=256 THEN ?5 END)
             ON CONFLICT(segment_key) DO UPDATE SET last_seq=excluded.last_seq,event_count=context_event_segment.event_count+1,content_sha256=excluded.content_sha256,sealed_at=CASE WHEN context_event_segment.event_count+1=256 THEN ?5 ELSE context_event_segment.sealed_at END",
            params![segment, seq, content, ordinal, crate::time::now_iso()],
        )?;

        for measurement in &event.measurements {
            let (value_type, value_integer, value_real, value_boolean) = match measurement.value {
                MeasurementValue::Integer(value) => ("integer", Some(value), None, None),
                MeasurementValue::Real(value) => ("real", None, Some(value), None),
                MeasurementValue::Boolean(value) => ("boolean", None, None, Some(i64::from(value))),
            };
            conn.execute(
                "INSERT INTO context_event_measurement
                    (event_id,name,value_type,value_integer,value_real,value_boolean,unit)
                 VALUES (?1,?2,?3,?4,?5,?6,?7)",
                params![
                    event.event_id,
                    measurement.name,
                    value_type,
                    value_integer,
                    value_real,
                    value_boolean,
                    measurement.unit,
                ],
            )?;
        }
        for link in &event.links {
            conn.execute(
                "INSERT INTO context_event_link (event_id,relation,target_event_id)
                 VALUES (?1,?2,?3)",
                params![event.event_id, link.relation, link.target_event_id],
            )?;
        }
        inserted += 1;
        receipts.push(ContextEventReceipt {
            event_id: event.event_id.clone(),
            canonical_sha256: prepared_event.canonical_sha256,
            disposition: "inserted".to_string(),
        });
    }
    Ok(ContextEventBatchReceipt {
        schema_version: EVENT_SCHEMA_VERSION,
        complete: true,
        inserted,
        duplicates,
        events: receipts,
    })
}

const PROMPT_INGRESS_MAX_RECORD_BYTES: usize = 256 * 1024;
const PROMPT_INGRESS_MAX_RECORDS_PER_DRAIN: usize = 1_000;
const PROMPT_INGRESS_REJECTION_MAX_BYTES: u64 = 1024 * 1024;
const PROMPT_INGRESS_ROTATE_BYTES: u64 = 1024 * 1024;
const PROMPT_INGRESS_SEALED_QUIET_SECS: u64 = 2;

fn ingress_sidecar(path: &Path, suffix: &str) -> PathBuf {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("context-telemetry-ingress.jsonl");
    path.with_file_name(format!("{name}.{suffix}"))
}

pub fn default_prompt_telemetry_ingress(db_path: &Path) -> PathBuf {
    std::env::var_os("CRYPT_TELEMETRY_INGRESS")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            db_path
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join("context-telemetry-ingress.jsonl")
        })
}

fn read_ingress_cursor(path: &Path, journal_len: u64) -> u64 {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|value| *value <= journal_len)
        .unwrap_or(0)
}

fn persist_ingress_cursor(path: &Path, cursor: u64) -> Result<(), String> {
    let mut file = File::create(path).map_err(|_| "persist prompt telemetry cursor".to_string())?;
    file.write_all(cursor.to_string().as_bytes())
        .and_then(|_| file.sync_data())
        .map_err(|_| "persist prompt telemetry cursor".to_string())
}

fn persist_ingress_rejection(
    path: &Path,
    offset: u64,
    record: &[u8],
    reason_code: &str,
    event_count: usize,
) -> Result<(), String> {
    let mut hasher = Sha256::new();
    hasher.update(record);
    let payload = serde_json::to_vec(&serde_json::json!({
        "schema_version": 1,
        "offset": offset,
        "record_sha256": format!("{:x}", hasher.finalize()),
        "reason_code": reason_code,
        "event_count": event_count,
    }))
    .map_err(|_| "encode prompt telemetry rejection".to_string())?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|_| "open prompt telemetry rejection ledger".to_string())?;
    let current = file
        .metadata()
        .map_err(|_| "inspect prompt telemetry rejection ledger".to_string())?
        .len();
    if current.saturating_add(payload.len() as u64 + 1) > PROMPT_INGRESS_REJECTION_MAX_BYTES {
        return Err("prompt telemetry rejection ledger is full".to_string());
    }
    file.write_all(&payload)
        .and_then(|_| file.write_all(b"\n"))
        .and_then(|_| file.sync_data())
        .map_err(|_| "persist prompt telemetry rejection".to_string())
}

fn lifecycle_expansion_rejection_reason(error: &ContextTelemetryError) -> &'static str {
    let ContextTelemetryError::Invalid(message) = error else {
        return "intent.event_graph_invalid";
    };
    if message == "lifecycle_intent expanded event counts do not match" {
        "intent.event_count_mismatch"
    } else if message == "lifecycle_intent has invalid bounds" {
        "intent.invalid_bounds"
    } else if message == "lifecycle_intent contains an unsupported token" {
        "intent.unsupported_token"
    } else if message.starts_with("provider_results.candidates ") {
        "intent.invalid_candidate"
    } else if message.starts_with("provider_results ") {
        "intent.invalid_provider_lane"
    } else {
        "intent.event_graph_invalid"
    }
}

enum PromptIngressBatch {
    Context(ContextEventBatch),
    Observable(ObservableEventBatchV1),
}

fn decode_prompt_ingress_record(record: &[u8]) -> Result<PromptIngressBatch, &'static str> {
    let value: Value = serde_json::from_slice(record).map_err(|_| "record.invalid_json")?;
    let object = value.as_object().ok_or("record.invalid_wrapper")?;
    if object.len() != 1 {
        return Err("record.invalid_wrapper");
    }
    if let Some(events) = object.get("events") {
        return serde_json::from_value::<ContextEventBatch>(
            serde_json::json!({ "events": events }),
        )
        .map(PromptIngressBatch::Context)
        .map_err(|_| "record.invalid_batch");
    }
    if let Some(events) = object.get("observable_events") {
        return serde_json::from_value::<ObservableEventBatchV1>(
            serde_json::json!({ "events": events }),
        )
        .map(PromptIngressBatch::Observable)
        .map_err(|_| "record.invalid_observable_batch");
    }
    if let Some(intent) = object.get("lifecycle_intent") {
        let intent = serde_json::from_value::<ContextLifecycleIntent>(intent.clone())
            .map_err(|_| "intent.invalid_schema")?;
        return expand_context_lifecycle_intent(&intent)
            .map(PromptIngressBatch::Context)
            .map_err(|error| lifecycle_expansion_rejection_reason(&error));
    }
    Err("record.invalid_wrapper")
}

/// Drain complete prompt records in file order. The cursor checkpoint follows a completed drain;
/// a crash in between safely replays content-addressed duplicates on the next pass.
pub fn drain_prompt_telemetry_ingress_once(
    db: &MemDb,
    lease: &ContextIngestLease,
    ingress_path: &Path,
) -> Result<PromptIngressDrain, String> {
    let mut result = PromptIngressDrain::default();
    let file = match File::open(ingress_path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(result),
        Err(_) => return Err("open prompt telemetry ingress".to_string()),
    };
    let journal_len = file
        .metadata()
        .map_err(|_| "inspect prompt telemetry ingress".to_string())?
        .len();
    let cursor_path = ingress_sidecar(ingress_path, "cursor");
    let rejection_path = ingress_sidecar(ingress_path, "rejected.jsonl");
    let mut cursor = read_ingress_cursor(&cursor_path, journal_len);
    let mut reader = BufReader::new(file);
    reader
        .seek(SeekFrom::Start(cursor))
        .map_err(|_| "seek prompt telemetry ingress".to_string())?;
    for _ in 0..PROMPT_INGRESS_MAX_RECORDS_PER_DRAIN {
        let offset = cursor;
        let mut record = Vec::new();
        let read = reader
            .read_until(b'\n', &mut record)
            .map_err(|_| "read prompt telemetry ingress".to_string())?;
        if read == 0 || record.last() != Some(&b'\n') {
            break;
        }
        cursor = reader
            .stream_position()
            .map_err(|_| "locate prompt telemetry ingress".to_string())?;
        let event_count_hint = serde_json::from_slice::<Value>(&record)
            .ok()
            .and_then(|value| {
                value
                    .get("events")
                    .and_then(Value::as_array)
                    .map(Vec::len)
                    .or_else(|| {
                        value
                            .get("observable_events")
                            .and_then(Value::as_array)
                            .map(Vec::len)
                    })
                    .or_else(|| {
                        value
                            .get("lifecycle_intent")
                            .and_then(|intent| intent.get("expected_event_count"))
                            .and_then(Value::as_u64)
                            .and_then(|count| usize::try_from(count).ok())
                    })
            })
            .unwrap_or(0);
        let decoded = if record.len() > PROMPT_INGRESS_MAX_RECORD_BYTES {
            Err("record.too_large")
        } else {
            decode_prompt_ingress_record(&record)
        };
        match decoded {
            Ok(batch) => {
                let (batch_len, ingested) = match batch {
                    PromptIngressBatch::Context(batch) => {
                        let length = batch.events.len();
                        (length, db.ingest_local_context_events(&batch, lease))
                    }
                    PromptIngressBatch::Observable(batch) => {
                        let length = batch.events.len();
                        (length, db.ingest_observable_events(&batch, lease))
                    }
                };
                match ingested {
                    Ok(receipt) => {
                        result.inserted += receipt.inserted;
                        result.duplicates += receipt.duplicates;
                    }
                    Err(ContextTelemetryError::Database(_)) => {
                        return Err("ingest prompt telemetry unavailable".to_string());
                    }
                    Err(ContextTelemetryError::Invalid(_)) => {
                        persist_ingress_rejection(
                            &rejection_path,
                            offset,
                            &record,
                            "batch.invalid",
                            batch_len,
                        )?;
                        result.rejected += 1;
                    }
                    Err(ContextTelemetryError::Conflict { .. }) => {
                        persist_ingress_rejection(
                            &rejection_path,
                            offset,
                            &record,
                            "batch.event_conflict",
                            batch_len,
                        )?;
                        result.rejected += 1;
                    }
                    Err(ContextTelemetryError::AttributionMismatch) => {
                        persist_ingress_rejection(
                            &rejection_path,
                            offset,
                            &record,
                            "batch.attribution_mismatch",
                            batch_len,
                        )?;
                        result.rejected += 1;
                    }
                }
            }
            Err(reason_code) => {
                persist_ingress_rejection(
                    &rejection_path,
                    offset,
                    &record,
                    reason_code,
                    event_count_hint,
                )?;
                result.rejected += 1;
            }
        }
        result.records += 1;
        result.cursor = cursor;
    }
    // A checkpoint after the bounded drain removes one fsync per record from the
    // producer's contention domain. If this write is interrupted, the next drain
    // replays already-committed, content-addressed events safely.
    if result.records > 0 {
        persist_ingress_cursor(&cursor_path, cursor)?;
    }
    Ok(result)
}

fn merge_prompt_ingress_drain(target: &mut PromptIngressDrain, source: PromptIngressDrain) {
    target.records += source.records;
    target.inserted += source.inserted;
    target.duplicates += source.duplicates;
    target.rejected += source.rejected;
    target.cursor = source.cursor;
}

fn sealed_prompt_ingress_paths(active: &Path) -> Vec<PathBuf> {
    let Some(name) = active.file_name().and_then(|value| value.to_str()) else {
        return Vec::new();
    };
    let prefix = format!("{name}.sealed-");
    let mut paths: Vec<_> = active
        .parent()
        .and_then(|parent| std::fs::read_dir(parent).ok())
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|value| value.to_str())
                .and_then(|value| value.strip_prefix(&prefix))
                .is_some_and(|suffix| {
                    !suffix.is_empty()
                        && suffix
                            .bytes()
                            .all(|byte| byte.is_ascii_digit() || byte == b'-')
                })
        })
        .collect();
    paths.sort();
    paths
}

fn sealed_prompt_ingress_is_quiet(path: &Path, cursor: u64) -> bool {
    let Ok(metadata) = path.metadata() else {
        return false;
    };
    if metadata.len() != cursor {
        return false;
    }
    metadata
        .modified()
        .ok()
        .and_then(|modified| modified.elapsed().ok())
        .is_some_and(|elapsed| elapsed.as_secs() >= PROMPT_INGRESS_SEALED_QUIET_SECS)
}

fn rotate_consumed_prompt_ingress(active: &Path, cursor: u64) -> Result<(), String> {
    let length = match active.metadata() {
        Ok(metadata) => metadata.len(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err("inspect prompt telemetry ingress for rotation".to_string()),
    };
    if length < PROMPT_INGRESS_ROTATE_BYTES || length != cursor {
        return Ok(());
    }
    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| "name prompt telemetry rotation".to_string())?
        .as_nanos();
    let sealed = active.with_file_name(format!(
        "{}.sealed-{}-{suffix}",
        active
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("context-telemetry-ingress.jsonl"),
        std::process::id(),
    ));
    let active_cursor = ingress_sidecar(active, "cursor");
    let sealed_cursor = ingress_sidecar(&sealed, "cursor");
    let cursor_moved = match std::fs::rename(&active_cursor, &sealed_cursor) {
        Ok(()) => true,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(_) => return Err("rotate prompt telemetry cursor".to_string()),
    };
    if std::fs::rename(active, &sealed).is_err() && cursor_moved {
        let _ = std::fs::rename(&sealed_cursor, &active_cursor);
    }
    Ok(())
}

/// Drain sealed journals before the active one, delete only fully consumed journals that have
/// remained unchanged beyond the writer race window, and rotate the active file below its cap.
pub fn drain_prompt_telemetry_ingress(
    db: &MemDb,
    lease: &ContextIngestLease,
    ingress_path: &Path,
) -> Result<PromptIngressDrain, String> {
    let mut result = PromptIngressDrain::default();
    for sealed in sealed_prompt_ingress_paths(ingress_path) {
        let drained = drain_prompt_telemetry_ingress_once(db, lease, &sealed)?;
        let cursor = drained.cursor.max(read_ingress_cursor(
            &ingress_sidecar(&sealed, "cursor"),
            sealed.metadata().map(|value| value.len()).unwrap_or(0),
        ));
        merge_prompt_ingress_drain(&mut result, drained);
        if sealed_prompt_ingress_is_quiet(&sealed, cursor) {
            std::fs::remove_file(&sealed)
                .map_err(|_| "remove consumed prompt telemetry journal".to_string())?;
            let _ = std::fs::remove_file(ingress_sidecar(&sealed, "cursor"));
        }
    }
    let active = drain_prompt_telemetry_ingress_once(db, lease, ingress_path)?;
    let active_cursor = active.cursor.max(read_ingress_cursor(
        &ingress_sidecar(ingress_path, "cursor"),
        ingress_path
            .metadata()
            .map(|value| value.len())
            .unwrap_or(0),
    ));
    merge_prompt_ingress_drain(&mut result, active);
    rotate_consumed_prompt_ingress(ingress_path, active_cursor)?;
    Ok(result)
}

impl MemDb {
    /// Convert the frozen host envelope into the canonical Membrane event ledger form.
    pub fn ingest_observable_events(
        &self,
        batch: &ObservableEventBatchV1,
        lease: &ContextIngestLease,
    ) -> Result<ContextEventBatchReceipt, ContextTelemetryError> {
        if batch.events.is_empty() || batch.events.len() > MAX_BATCH_EVENTS {
            return Err(invalid("events", "must contain between 1 and 256 items"));
        }
        for event in &batch.events {
            event.validate()?;
        }
        let events = batch
            .events
            .iter()
            .map(|event| ContextEvent {
                schema_version: EVENT_SCHEMA_VERSION,
                event_id: format!("event-{}", sha256_text(&event.event_id)),
                ts: event.timestamp.clone(),
                installation_id: lease.installation_id.clone(),
                service_instance_id: lease.service_instance_id.clone(),
                workspace_id: lifecycle_workspace_id(&lease.service_instance_id),
                client: event.client_id.clone(),
                client_version: None,
                producer: "host-adapter".to_string(),
                producer_version: None,
                session_id: lifecycle_bounded_id(&event.session_id, "session"),
                turn_id: Some(lifecycle_bounded_id(&event.turn_id, "turn")),
                parent_span_id: None,
                trace_id: lifecycle_bounded_id(&event.trace_id, "trace"),
                span_id: lifecycle_bounded_id(&event.event_id, "span"),
                artifact_family: "context".to_string(),
                provider: "membrane".to_string(),
                provider_version: None,
                release_generation: None,
                phase: "turn.observed".to_string(),
                operation: "evaluate".to_string(),
                status: "success".to_string(),
                reason_code: Some(event.origin.clone()),
                scope_id: None,
                artifact_id: Some(lifecycle_artifact_id(&event.task_id, "task")),
                artifact_sha256: event
                    .content_ref_or_digest
                    .strip_prefix("sha256:")
                    .map(str::to_string),
                traffic_class: "production".to_string(),
                policy_version: None,
                policy_activation_sha256: Some(
                    event
                        .policy_snapshot_digest
                        .strip_prefix("sha256:")
                        .unwrap_or(&event.policy_snapshot_digest)
                        .to_string(),
                ),
                cohort: None,
                task_class: Some("observable".to_string()),
                source_generation: None,
                duration_ms: event.duration_ms,
                quantity: None,
                token_count: None,
                char_count: None,
                meta: Some(BTreeMap::from([
                    (
                        "complete".to_string(),
                        Value::Bool(event.completeness.values().all(|value| *value)),
                    ),
                    (
                        "event_type".to_string(),
                        Value::String(lifecycle_event_type_token(&event.event_type)),
                    ),
                ])),
                measurements: Vec::new(),
                links: Vec::new(),
            })
            .collect();
        self.ingest_local_context_events(&ContextEventBatch { events }, lease)
    }

    /// Validate and append a bounded event batch in one SQLite transaction.
    pub fn ingest_context_events(
        &self,
        batch: &ContextEventBatch,
    ) -> Result<ContextEventBatchReceipt, ContextTelemetryError> {
        let mut conn = self.lock_events();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let receipt = append_context_events_on(&tx, batch)?;
        tx.commit()?;
        Ok(receipt)
    }

    /// Replay durable memory-side event intents into Membrane's physical event database.
    /// Event insertion precedes outbox deletion, so process failure can only cause idempotent replay.
    pub fn flush_context_event_outbox(&self) -> Result<usize, ContextTelemetryError> {
        let mut flushed = 0usize;
        loop {
            let next = {
                let conn = self.lock();
                conn.query_row(
                    "SELECT id, batch_sha256, batch_json FROM membrane_event_outbox ORDER BY id LIMIT 1",
                    [],
                    |row| {
                        Ok((
                            row.get::<_, i64>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                        ))
                    },
                )
                .optional()?
            };
            let Some((id, batch_sha256, batch_json)) = next else {
                return Ok(flushed);
            };
            if sha256_text(&batch_json) != batch_sha256 {
                return Err(ContextTelemetryError::Invalid(
                    "membrane event outbox digest mismatch".to_string(),
                ));
            }
            let batch: ContextEventBatch = serde_json::from_str(&batch_json)
                .map_err(|error| ContextTelemetryError::Invalid(error.to_string()))?;
            {
                let mut events = self.lock_events();
                let tx = events.transaction_with_behavior(TransactionBehavior::Immediate)?;
                append_context_events_on(&tx, &batch)?;
                tx.commit()?;
            }
            let deleted = self.lock().execute(
                "DELETE FROM membrane_event_outbox WHERE id=?1 AND batch_sha256=?2",
                params![id, batch_sha256],
            )?;
            if deleted != 1 {
                return Err(ContextTelemetryError::Invalid(
                    "membrane event outbox acknowledgement conflict".to_string(),
                ));
            }
            flushed += batch.events.len();
        }
    }

    /// Ingest locally produced events only when every envelope is bound to the active service
    /// lease. Installation registration and the event batch share one transaction, so a rejected
    /// or conflicting event cannot leave partial installation metadata behind.
    pub fn ingest_bound_context_events(
        &self,
        batch: &ContextEventBatch,
        lease: &ContextIngestLease,
    ) -> Result<ContextEventBatchReceipt, ContextTelemetryError> {
        lease.bind_batch(batch)?;
        let mut conn = self.lock_events();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let observed_at = crate::time::now_iso();
        tx.execute(
            "INSERT INTO context_installation (
                installation_id, created_at, first_observed_at, last_observed_at,
                display_label, os_type, os_version, arch, identity_key_fingerprint,
                installation_epoch
             ) VALUES (?1, ?2, ?3, ?3, NULL, ?4, NULL, ?5, NULL, ?6)
             ON CONFLICT(installation_id) DO UPDATE SET
                last_observed_at=excluded.last_observed_at,
                os_type=excluded.os_type,
                arch=excluded.arch,
                installation_epoch=excluded.installation_epoch",
            params![
                lease.installation_id,
                lease.created_at,
                observed_at,
                lease.os_type,
                lease.arch,
                lease.installation_epoch,
            ],
        )?;
        let receipt = append_context_events_on(&tx, batch)?;
        tx.commit()?;
        Ok(receipt)
    }

    /// Ingest an authenticated local producer batch. The installation remains fail-closed, while
    /// the producer service instance is preserved so an append-only queue survives a resident
    /// service restart without erasing which process made the decision.
    pub fn ingest_local_context_events(
        &self,
        batch: &ContextEventBatch,
        lease: &ContextIngestLease,
    ) -> Result<ContextEventBatchReceipt, ContextTelemetryError> {
        lease.bind_local_batch(batch)?;
        let mut conn = self.lock_events();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let observed_at = crate::time::now_iso();
        tx.execute(
            "INSERT INTO context_installation (
                installation_id, created_at, first_observed_at, last_observed_at,
                display_label, os_type, os_version, arch, identity_key_fingerprint,
                installation_epoch
             ) VALUES (?1, ?2, ?3, ?3, NULL, ?4, NULL, ?5, NULL, ?6)
             ON CONFLICT(installation_id) DO UPDATE SET
                last_observed_at=excluded.last_observed_at,
                os_type=excluded.os_type,
                arch=excluded.arch,
                installation_epoch=excluded.installation_epoch",
            params![
                lease.installation_id,
                lease.created_at,
                observed_at,
                lease.os_type,
                lease.arch,
                lease.installation_epoch,
            ],
        )?;
        let receipt = append_context_events_on(&tx, batch)?;
        tx.commit()?;
        Ok(receipt)
    }

    /// USER-ORIGIN evidence only. This is the sole read path Morph Taste may use: there is no
    /// parameter on `ObservableEventQuery` that can widen it, because origin scope is selected by
    /// which function is called, never by caller-supplied data.
    pub fn query_observable_events_for_taste(
        &self,
        filter: &ObservableEventQuery,
    ) -> Result<ObservableEventQueryResult, ContextTelemetryError> {
        self.query_observable_events(filter, ObservableOriginScope::TasteUserOnly)
    }

    /// Full authorized observable-event stream (every frozen origin), for Morph Insights.
    pub fn query_observable_events_for_insights(
        &self,
        filter: &ObservableEventQuery,
    ) -> Result<ObservableEventQueryResult, ContextTelemetryError> {
        self.query_observable_events(filter, ObservableOriginScope::InsightsFullStream)
    }

    /// TOOL-ORIGIN evidence only (duration-bearing tool_receipt/tool_receipt_failed events). This
    /// is the sole read path Forge time-accounting may use: mirrors `_for_taste`'s isolation --
    /// scope is fixed by which function is called, never by caller-supplied data.
    pub fn query_observable_events_for_forge(
        &self,
        filter: &ObservableEventQuery,
    ) -> Result<ObservableEventQueryResult, ContextTelemetryError> {
        self.query_observable_events(filter, ObservableOriginScope::ForgeToolOnly)
    }

    /// Shared read path behind the two public entry points above. Private so external callers can
    /// never choose `ObservableOriginScope` themselves -- the only way to select a scope is to
    /// call one of the two named public functions.
    fn query_observable_events(
        &self,
        filter: &ObservableEventQuery,
        scope: ObservableOriginScope,
    ) -> Result<ObservableEventQueryResult, ContextTelemetryError> {
        if filter.limit == 0 || filter.limit > MAX_OBSERVABLE_QUERY_LIMIT {
            return Err(invalid(
                "limit",
                "must be between 1 and MAX_OBSERVABLE_QUERY_LIMIT",
            ));
        }
        for (value, field) in [
            (filter.since.as_deref(), "since"),
            (filter.until.as_deref(), "until"),
        ] {
            if value.is_some_and(|value| !valid_utc_timestamp(value)) {
                return Err(invalid(field, "must be RFC3339"));
            }
        }
        if let (Some(since), Some(until)) = (filter.since.as_deref(), filter.until.as_deref()) {
            if since > until {
                return Err(invalid("since", "must not be after until"));
            }
        }

        // Apply the same deterministic normalization the write path used at ingest, so a caller
        // may filter using the same raw ids it originally sent to `ingest_observable_events`.
        let session_key = filter
            .session_id
            .as_deref()
            .map(|value| lifecycle_bounded_id(value, "session"));
        let task_key = filter
            .task_id
            .as_deref()
            .map(|value| lifecycle_artifact_id(value, "task"));
        let trace_key = filter
            .trace_id
            .as_deref()
            .map(|value| lifecycle_bounded_id(value, "trace"));
        let event_type_key = filter.event_type.as_deref().map(lifecycle_event_type_token);

        let conn = self.lock_events();
        // `task_class = 'observable'` is the discriminator `ingest_observable_events` writes on
        // every row it produces (and only that function ever sets it), so this scopes the read to
        // ObservableEventV1-derived rows without needing a schema change or a new write-path flag.
        let mut statement = conn.prepare(
            "SELECT seq, event_id, ts, installation_id, session_id, artifact_id, trace_id,
                    turn_id, reason_code, artifact_sha256, policy_activation_sha256, meta,
                    duration_ms
             FROM context_event_log
             WHERE task_class = 'observable'
               AND (?1 IS NULL OR ts >= ?1)
               AND (?2 IS NULL OR ts <= ?2)
               AND (?3 IS NULL OR installation_id = ?3)
               AND (?4 IS NULL OR session_id = ?4)
               AND (?5 IS NULL OR artifact_id = ?5)
               AND (?6 IS NULL OR trace_id = ?6)
               AND (?7 IS NULL OR seq > ?7)
             ORDER BY seq ASC
             LIMIT ?8",
        )?;
        let scan_limit = i64::try_from(MAX_OBSERVABLE_SCAN_ROWS).unwrap_or(i64::MAX);
        let mut rows = statement.query(params![
            filter.since,
            filter.until,
            filter.installation_id,
            session_key,
            task_key,
            trace_key,
            filter.after_sequence,
            scan_limit,
        ])?;

        let mut matched: Vec<ObservableEventRow> = Vec::new();
        let mut scanned = 0usize;
        let mut last_seen_sequence: Option<i64> = None;
        while let Some(row) = rows.next()? {
            scanned += 1;
            let sequence: i64 = row.get(0)?;
            last_seen_sequence = Some(sequence);
            // Origin lives in `reason_code` for observable-derived rows only (see
            // `ingest_observable_events`); scope enforcement happens here, before a row is ever
            // materialized into the result, so a Taste-scoped caller cannot see it even
            // transiently.
            let origin: Option<String> = row.get(8)?;
            let origin = origin.unwrap_or_default();
            if !scope.allows(&origin) {
                continue;
            }
            let task_id: Option<String> = row.get(5)?;
            let task_id = task_id.unwrap_or_default();
            let meta_json: String = row.get(11)?;
            let meta: BTreeMap<String, Value> =
                serde_json::from_str(&meta_json).unwrap_or_default();
            let event_type = meta
                .get("event_type")
                .and_then(Value::as_str)
                .map(str::to_string);
            if let Some(expected) = event_type_key.as_deref() {
                if event_type.as_deref() != Some(expected) {
                    continue;
                }
            }
            let complete = meta.get("complete").and_then(Value::as_bool);
            let artifact_sha256: Option<String> = row.get(9)?;
            let policy_activation_sha256: Option<String> = row.get(10)?;
            let duration_ms: Option<f64> = row.get(12)?;
            let duration_ms = duration_ms.map(|value| value.round() as i64);
            matched.push(ObservableEventRow {
                sequence,
                event_id: row.get(1)?,
                ts: row.get(2)?,
                installation_id: row.get(3)?,
                session_id: row.get(4)?,
                task_id,
                trace_id: row.get(6)?,
                turn_id: row.get(7)?,
                origin,
                event_type,
                content_digest: artifact_sha256.map(|hash| format!("sha256:{hash}")),
                policy_snapshot_digest: policy_activation_sha256
                    .map(|hash| format!("sha256:{hash}")),
                complete,
                duration_ms,
            });
            if matched.len() > filter.limit {
                break;
            }
        }
        let hit_scan_cap = scanned >= MAX_OBSERVABLE_SCAN_ROWS;
        let truncated = matched.len() > filter.limit || hit_scan_cap;
        matched.truncate(filter.limit);
        let next_cursor = if truncated {
            matched
                .last()
                .map(|row| row.sequence)
                .or(last_seen_sequence)
        } else {
            None
        };
        Ok(ObservableEventQueryResult {
            rows: matched,
            limit: filter.limit,
            truncated,
            next_cursor,
        })
    }
}

/// Which origin class a read of the observable-event ledger is authorized to return. Private:
/// the only way to obtain one is through `query_observable_events_for_taste` or
/// `query_observable_events_for_insights`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ObservableOriginScope {
    /// Morph Taste: user-origin evidence only, per the L1 isolation mandate.
    TasteUserOnly,
    /// Morph Insights: the full authorized stream, every frozen origin value.
    InsightsFullStream,
    /// Forge time-accounting: tool-origin evidence only (tool_receipt/tool_receipt_failed
    /// events carrying duration_ms), never user/assistant content.
    ForgeToolOnly,
}

impl ObservableOriginScope {
    fn allows(self, origin: &str) -> bool {
        match self {
            ObservableOriginScope::TasteUserOnly => origin == "user",
            ObservableOriginScope::InsightsFullStream => true,
            ObservableOriginScope::ForgeToolOnly => origin == "tool",
        }
    }
}

/// Bounded, explicit filter set for reading back persisted `ObservableEventV1` rows. Every
/// identifier filter, when present, is matched using the same deterministic normalization the
/// write path applies at ingest (`lifecycle_bounded_id` / `lifecycle_artifact_id` /
/// `lifecycle_event_type_token`), so callers pass the same raw ids/labels they originally sent to
/// `ingest_observable_events` -- never an internal hash. `limit` is required and bounded by
/// `MAX_OBSERVABLE_QUERY_LIMIT`; `after_sequence` resumes a prior truncated read from its
/// `next_cursor`, preserving append-order across pages.
#[derive(Clone, Debug, Default)]
pub struct ObservableEventQuery {
    pub since: Option<String>,
    pub until: Option<String>,
    pub event_type: Option<String>,
    pub session_id: Option<String>,
    pub task_id: Option<String>,
    pub trace_id: Option<String>,
    pub installation_id: Option<String>,
    pub after_sequence: Option<i64>,
    pub limit: usize,
}

/// Content-free, opaque projection of one persisted observable-event ledger row. Every field is
/// an id, a digest, a taxonomy label, or a boolean -- exactly the discipline `ObservableEventV1`
/// itself enforces at ingest. No prompt, reply, argument, path, or raw error ever appears here.
#[derive(Clone, Debug, Serialize, Eq, PartialEq)]
pub struct ObservableEventRow {
    /// Append-order position in the ledger (`context_event_log.seq`). This is the lineage/order
    /// primitive: it is assigned once at insert, never reassigned, and is what makes replaying a
    /// planted session's exact event order and lineage possible.
    pub sequence: i64,
    pub event_id: String,
    pub ts: String,
    pub installation_id: String,
    pub session_id: String,
    pub task_id: String,
    pub trace_id: String,
    pub turn_id: Option<String>,
    pub origin: String,
    pub event_type: Option<String>,
    pub content_digest: Option<String>,
    pub policy_snapshot_digest: Option<String>,
    pub complete: Option<bool>,
    /// Numeric-only duration in whole milliseconds (rounded from the stored REAL), when the
    /// ingesting producer measured one. Integer, not f64, so this row can keep deriving `Eq`.
    /// Same content-free discipline as every other field on this row.
    pub duration_ms: Option<i64>,
}

/// Honest completeness envelope for an observable-event read. `truncated` and `next_cursor` are
/// always populated together (`next_cursor.is_some() == truncated`, whenever at least one row was
/// scanned), so a capped or partially-covered result can never be mistaken for a complete one.
#[derive(Clone, Debug, Serialize, Eq, PartialEq)]
pub struct ObservableEventQueryResult {
    pub rows: Vec<ObservableEventRow>,
    pub limit: usize,
    pub truncated: bool,
    pub next_cursor: Option<i64>,
}

#[cfg(test)]
mod lifecycle_intent_tests {
    use super::*;

    fn off_intent_value() -> Value {
        serde_json::json!({
            "schema_version": 1,
            "installation_id": "8b145eb2-1e94-4eb2-966c-8f12008ba107",
            "service_instance_id": "svc-bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            "workspace_id": "ws.cccccccccccccccccccccccccccccccc",
            "session_id": "session-dddddddddddddddddddddddddddddddd",
            "trace_id": "12345678-1234-4123-8123-123456789abc",
            "policy_version": "rightcontext-policy-v1",
            "policy_activation_sha256": "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
            "task_class": "code",
            "ts": "2026-07-21T00:00:00Z",
            "client": "codex",
            "mode": "off",
            "cohort": "unassigned",
            "cohorts_enabled": false,
            "federation_status": "skipped",
            "federation_reason": "mode_off",
            "legacy_status": "success",
            "candidate_count": 0,
            "admitted_count": 0,
            "delivered_count": 0,
            "delivered_chars": 0,
            "truncated_count": 0,
            "federation_duration_ms": null,
            "provider_results": [],
            "expected_event_count": 8,
            "expected_terminal_count": 2
        })
    }

    fn off_intent() -> ContextLifecycleIntent {
        serde_json::from_value(off_intent_value()).unwrap()
    }

    fn lease() -> ContextIngestLease {
        ContextIngestLease {
            installation_id: "8b145eb2-1e94-4eb2-966c-8f12008ba107".to_string(),
            service_instance_id: "svc-bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string(),
            startup_generation: 1,
            created_at: "2026-07-21T00:00:00Z".to_string(),
            installation_epoch: 1,
            os_type: std::env::consts::OS,
            arch: std::env::consts::ARCH,
        }
    }

    #[test]
    fn mbr011_selected_without_delivery_is_a_degraded_visible_failure() {
        // selected>0 and delivered==0 → degraded, one stable alert, promotion blocked.
        let outcome = evaluate_delivery_outcome(3, 0);
        assert_eq!(outcome.status, "degraded");
        assert_eq!(outcome.alert, Some(SELECTED_WITHOUT_DELIVERY_ALERT));
        assert_eq!(outcome.alert, Some("selected_without_delivery"));
        assert!(outcome.release_promotion_blocked);
        assert_eq!(outcome.selected, 3);
        assert_eq!(outcome.delivered, 0);
    }

    #[test]
    fn mbr011_any_delivery_or_no_selection_clears_the_failure() {
        // delivered>0 → ok, no alert, promotion allowed.
        let delivered = evaluate_delivery_outcome(3, 1);
        assert_eq!(delivered.status, "ok");
        assert_eq!(delivered.alert, None);
        assert!(!delivered.release_promotion_blocked);
        // nothing selected → ok even with zero delivered.
        let empty = evaluate_delivery_outcome(0, 0);
        assert_eq!(empty.status, "ok");
        assert_eq!(empty.alert, None);
        assert!(!empty.release_promotion_blocked);
        // negative inputs are clamped, never panic.
        let clamped = evaluate_delivery_outcome(-5, -1);
        assert_eq!(clamped.status, "ok");
    }

    #[test]
    fn mbr012_canonical_delivery_metrics_match_js_fixture_byte_for_byte() {
        // The same fixture is pinned in mcp/delivery-serialization.test.mjs; the
        // two implementations must agree byte-for-byte on all four surfaces.
        let metrics = canonical_delivery_metrics("Membrane café ☕");
        assert_eq!(metrics.bytes, 18);
        assert_eq!(metrics.chars, 15);
        assert_eq!(metrics.tokens, 5);
        assert_eq!(
            metrics.sha256,
            "sha256:8444cb5ce2a629fbeae804c4ec724128c835fd6885e37c98fb17bca2b5a3456e"
        );
        // tokens derive from bytes: ceil(bytes/4).
        assert_eq!(metrics.tokens, (metrics.bytes + 3) / 4);
    }

    #[test]
    fn mbr012_empty_content_has_zero_metrics_and_empty_sha256() {
        let metrics = canonical_delivery_metrics("");
        assert_eq!(metrics.bytes, 0);
        assert_eq!(metrics.chars, 0);
        assert_eq!(metrics.tokens, 0);
        assert_eq!(
            metrics.sha256,
            "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn compact_off_intent_matches_python_canonical_event_ids() {
        let intent = off_intent();
        let batch = expand_context_lifecycle_intent(&intent).unwrap();
        let ids: Vec<_> = batch
            .events
            .iter()
            .map(|event| event.event_id.as_str())
            .collect();
        assert_eq!(
            ids,
            [
                "evt-7977050ea9a29d6e10a375dc6d39a9aa775c2c29557ace81792ba008c16a04e0",
                "evt-ea9cdc262a03158f70a9370b956121b55c6dd9e38e5d5eebe48f1aecad6e1325",
                "evt-0de7089ba5c659a1103c06cdc18d8dce9d825bc43df14e038d66fede07282a27",
                "evt-ec5b3728a309e75b6d465bb90ca6c3ee59a2c7c074f1a9fa502bc0cca7e430b5",
                "evt-31e4ef552d15a1116456a73e9b3e32e8f832fba5684e603129b52467d3d3f6bb",
                "evt-bc59ead3a971ed33e3086ce1ca37ce6230f9b1ff1cbe81ca0c3854c2d09c1909",
                "evt-a5ab37e5399f9ebca19accb2c1c086a9afab6e81ce0f411d7c46d53c0e85c32f",
                "evt-3f15212c58ba7daea57788147b7a0ce0a30655be5851eca7036609d4999a7b83",
            ]
        );
        assert!(batch
            .events
            .iter()
            .all(|event| event.policy_activation_sha256.as_deref()
                == Some("ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff")));
    }

    #[test]
    fn prompt_ingress_is_exact_and_crash_replay_is_idempotent() {
        let temporary = tempfile::tempdir().unwrap();
        let ingress = temporary.path().join("ingress.jsonl");
        let record =
            serde_json::to_vec(&serde_json::json!({ "lifecycle_intent": off_intent_value() }))
                .unwrap();
        std::fs::write(&ingress, [record.as_slice(), b"\n"].concat()).unwrap();
        let db = MemDb::open_in_memory();

        let first = drain_prompt_telemetry_ingress_once(&db, &lease(), &ingress).unwrap();
        assert_eq!(
            (
                first.records,
                first.inserted,
                first.duplicates,
                first.rejected
            ),
            (1, 8, 0, 0)
        );
        std::fs::remove_file(ingress_sidecar(&ingress, "cursor")).unwrap();
        let replay = drain_prompt_telemetry_ingress_once(&db, &lease(), &ingress).unwrap();
        assert_eq!(
            (
                replay.records,
                replay.inserted,
                replay.duplicates,
                replay.rejected
            ),
            (1, 0, 8, 0)
        );
        let count: i64 = db
            .lock()
            .query_row("SELECT COUNT(*) FROM context_event_log", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 8);
    }

    #[test]
    fn queued_prompt_ingress_survives_service_restart_on_same_installation() {
        let temporary = tempfile::tempdir().unwrap();
        let ingress = temporary.path().join("ingress.jsonl");
        let mut value = off_intent_value();
        value["service_instance_id"] = serde_json::json!("svc-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        let record = serde_json::to_vec(&serde_json::json!({
            "lifecycle_intent": value
        }))
        .unwrap();
        std::fs::write(&ingress, [record.as_slice(), b"\n"].concat()).unwrap();
        let db = MemDb::open_in_memory();

        let result = drain_prompt_telemetry_ingress_once(&db, &lease(), &ingress).unwrap();

        assert_eq!(
            (result.records, result.inserted, result.rejected),
            (1, 8, 0)
        );
        let producer_instances: Vec<String> = db
            .lock()
            .prepare("SELECT DISTINCT service_instance_id FROM context_event_log")
            .and_then(|mut statement| {
                statement
                    .query_map([], |row| row.get(0))
                    .and_then(|rows| rows.collect())
            })
            .unwrap();
        assert_eq!(producer_instances, ["svc-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"]);
    }

    #[test]
    fn queued_prompt_ingress_rejects_a_different_installation() {
        let temporary = tempfile::tempdir().unwrap();
        let ingress = temporary.path().join("ingress.jsonl");
        let mut value = off_intent_value();
        value["installation_id"] = serde_json::json!("018f0d1e-9a1f-4bb8-8da7-52f152923199");
        let record = serde_json::to_vec(&serde_json::json!({
            "lifecycle_intent": value
        }))
        .unwrap();
        std::fs::write(&ingress, [record.as_slice(), b"\n"].concat()).unwrap();
        let db = MemDb::open_in_memory();

        let result = drain_prompt_telemetry_ingress_once(&db, &lease(), &ingress).unwrap();

        assert_eq!(
            (result.records, result.inserted, result.rejected),
            (1, 0, 1)
        );
        let count: i64 = db
            .lock()
            .query_row("SELECT COUNT(*) FROM context_event_log", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn malformed_record_is_hash_accounted_and_does_not_block_valid_tail() {
        let temporary = tempfile::tempdir().unwrap();
        let ingress = temporary.path().join("ingress.jsonl");
        let valid =
            serde_json::to_vec(&serde_json::json!({ "lifecycle_intent": off_intent_value() }))
                .unwrap();
        std::fs::write(
            &ingress,
            [b"{\"unknown\":true}\n".as_slice(), valid.as_slice(), b"\n"].concat(),
        )
        .unwrap();
        let db = MemDb::open_in_memory();

        let result = drain_prompt_telemetry_ingress_once(&db, &lease(), &ingress).unwrap();
        assert_eq!(
            (
                result.records,
                result.inserted,
                result.duplicates,
                result.rejected
            ),
            (2, 8, 0, 1)
        );
        let rejection =
            std::fs::read_to_string(ingress_sidecar(&ingress, "rejected.jsonl")).unwrap();
        assert!(rejection.contains("record.invalid_wrapper"));
        assert!(!rejection.contains("unknown"));
    }

    #[test]
    fn expansion_rejection_records_specific_content_free_reason() {
        let temporary = tempfile::tempdir().unwrap();
        let ingress = temporary.path().join("ingress.jsonl");
        let mut value = off_intent_value();
        value["expected_event_count"] = serde_json::json!(10);
        let record = serde_json::to_vec(&serde_json::json!({ "lifecycle_intent": value })).unwrap();
        std::fs::write(&ingress, [record.as_slice(), b"\n"].concat()).unwrap();
        let db = MemDb::open_in_memory();

        let result = drain_prompt_telemetry_ingress_once(&db, &lease(), &ingress).unwrap();

        assert_eq!(
            (result.records, result.inserted, result.rejected),
            (1, 0, 1)
        );
        let rejection =
            std::fs::read_to_string(ingress_sidecar(&ingress, "rejected.jsonl")).unwrap();
        assert!(rejection.contains("intent.event_count_mismatch"));
        assert!(!rejection.contains("intent.expansion_rejected"));
    }

    #[test]
    fn observable_event_v1_validates_frozen_shape() {
        let event = ObservableEventV1 {
            schema: "orthic.observable-event.v1".to_string(),
            installation_id: "installation-a".to_string(),
            client_id: "claude_code".to_string(),
            session_id: "session-a".to_string(),
            task_id: "task-a".to_string(),
            turn_id: "turn-a".to_string(),
            trace_id: "trace-a".to_string(),
            event_id: "event-a".to_string(),
            event_type: "packet_delivered".to_string(),
            origin: "host".to_string(),
            content_ref_or_digest:
                "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                    .to_string(),
            timestamp: "2026-08-01T00:00:00Z".to_string(),
            completeness: BTreeMap::from([(String::from("packet"), true)]),
            policy_snapshot_digest:
                "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                    .to_string(),
            duration_ms: None,
        };
        event.validate().unwrap();
        let mut with_duration = event.clone();
        with_duration.duration_ms = Some(1_500.0);
        with_duration.validate().unwrap();
        with_duration.duration_ms = Some(-1.0);
        assert!(with_duration.validate().is_err());
        with_duration.duration_ms = Some(f64::NAN);
        assert!(with_duration.validate().is_err());
        let mut invalid = event.clone();
        invalid.origin = "model".to_string();
        assert!(invalid.validate().is_err());
        invalid = event;
        invalid.policy_snapshot_digest =
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string();
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn observable_ingress_converts_and_persists_in_membrane_ledger() {
        let temporary = tempfile::tempdir().unwrap();
        let ingress = temporary.path().join("observable.jsonl");
        let event = serde_json::json!({
            "schema": "orthic.observable-event.v1",
            "installation_id": "installation-a",
            "client_id": "claude_code",
            "session_id": "session-a",
            "task_id": "task-a",
            "turn_id": "turn-a",
            "trace_id": "trace-a",
            "event_id": "event-a",
            "event_type": "tool_receipt",
            "origin": "tool",
            "content_ref_or_digest": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "timestamp": "2026-08-01T00:00:00Z",
            "completeness": {"receipt": true},
            "policy_snapshot_digest": "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
        });
        std::fs::write(
            &ingress,
            serde_json::to_vec(&serde_json::json!({ "observable_events": [event] })).unwrap(),
        )
        .unwrap();
        std::fs::OpenOptions::new()
            .append(true)
            .open(&ingress)
            .unwrap()
            .write_all(b"\n")
            .unwrap();
        let result =
            drain_prompt_telemetry_ingress_once(&MemDb::open_in_memory(), &lease(), &ingress)
                .unwrap();
        assert_eq!(
            (result.records, result.inserted, result.rejected),
            (1, 1, 0)
        );
    }

    fn observable_event(
        event_id: &str,
        session_id: &str,
        task_id: &str,
        trace_id: &str,
        event_type: &str,
        origin: &str,
        ts: &str,
    ) -> ObservableEventV1 {
        ObservableEventV1 {
            schema: "orthic.observable-event.v1".to_string(),
            installation_id: "installation-a".to_string(),
            client_id: "claude_code".to_string(),
            session_id: session_id.to_string(),
            task_id: task_id.to_string(),
            // Each planted event gets its own turn -- the per-turn phase cardinality budget is an
            // unrelated write-path guard, not something these query tests are exercising.
            turn_id: format!("turn-{event_id}"),
            trace_id: trace_id.to_string(),
            event_id: event_id.to_string(),
            event_type: event_type.to_string(),
            origin: origin.to_string(),
            content_ref_or_digest: format!("sha256:{}", "a".repeat(64)),
            timestamp: ts.to_string(),
            completeness: BTreeMap::from([("packet".to_string(), true)]),
            policy_snapshot_digest: format!("sha256:{}", "b".repeat(64)),
            duration_ms: None,
        }
    }

    fn ingest_events(db: &MemDb, events: Vec<ObservableEventV1>) {
        let batch = ObservableEventBatchV1 { events };
        db.ingest_observable_events(&batch, &lease()).unwrap();
    }

    #[test]
    fn observable_query_reproduces_exact_order_and_lineage_for_a_planted_session() {
        let db = MemDb::open_in_memory();
        let session = "session-planted";
        ingest_events(
            &db,
            vec![
                observable_event(
                    "evt-1",
                    session,
                    "task-1",
                    "trace-1",
                    "packet_delivered",
                    "user",
                    "2026-08-01T00:00:00Z",
                ),
                observable_event(
                    "evt-2",
                    session,
                    "task-1",
                    "trace-1",
                    "tool_receipt",
                    "tool",
                    "2026-08-01T00:00:01Z",
                ),
                observable_event(
                    "evt-3",
                    session,
                    "task-1",
                    "trace-1",
                    "packet_delivered",
                    "assistant",
                    "2026-08-01T00:00:02Z",
                ),
                observable_event(
                    "evt-4",
                    session,
                    "task-1",
                    "trace-1",
                    "feedback_recorded",
                    "user",
                    "2026-08-01T00:00:03Z",
                ),
            ],
        );

        let filter = ObservableEventQuery {
            session_id: Some(session.to_string()),
            limit: 10,
            ..Default::default()
        };
        let first = db.query_observable_events_for_insights(&filter).unwrap();
        let second = db.query_observable_events_for_insights(&filter).unwrap();
        assert_eq!(
            first, second,
            "an identical query must reproduce a byte-identical order and lineage"
        );
        assert_eq!(first.rows.len(), 4);
        assert!(!first.truncated);
        let event_types: Vec<_> = first
            .rows
            .iter()
            .map(|row| row.event_type.clone().unwrap())
            .collect();
        assert_eq!(
            event_types,
            [
                "packet_delivered",
                "tool_receipt",
                "packet_delivered",
                "feedback_recorded",
            ]
        );
        let sequences: Vec<_> = first.rows.iter().map(|row| row.sequence).collect();
        let mut sorted = sequences.clone();
        sorted.sort_unstable();
        assert_eq!(
            sequences, sorted,
            "rows must come back in ascending append (lineage) order"
        );
    }

    #[test]
    fn taste_scope_never_returns_non_user_origin_events_even_when_present() {
        let db = MemDb::open_in_memory();
        let session = "session-mixed-origin";
        let non_user_origins = ["assistant", "tool", "host", "service", "repository"];
        let mut events = vec![observable_event(
            "evt-user-1",
            session,
            "task-1",
            "trace-1",
            "packet_delivered",
            "user",
            "2026-08-01T00:00:00Z",
        )];
        for (index, origin) in non_user_origins.iter().enumerate() {
            events.push(observable_event(
                &format!("evt-{origin}-{index}"),
                session,
                "task-1",
                "trace-1",
                "packet_delivered",
                origin,
                &format!("2026-08-01T00:00:0{}Z", index + 1),
            ));
        }
        events.push(observable_event(
            "evt-user-2",
            session,
            "task-1",
            "trace-1",
            "packet_delivered",
            "user",
            "2026-08-01T00:00:06Z",
        ));
        ingest_events(&db, events);

        let filter = ObservableEventQuery {
            session_id: Some(session.to_string()),
            limit: 100,
            ..Default::default()
        };
        let taste = db.query_observable_events_for_taste(&filter).unwrap();
        assert_eq!(taste.rows.len(), 2);
        assert!(taste.rows.iter().all(|row| row.origin == "user"));

        let insights = db.query_observable_events_for_insights(&filter).unwrap();
        assert_eq!(
            insights.rows.len(),
            2 + non_user_origins.len(),
            "the same store must still hold the non-user-origin rows Taste is forbidden to see"
        );
    }

    #[test]
    fn forge_scope_returns_only_tool_origin_rows_with_their_duration() {
        let db = MemDb::open_in_memory();
        let session = "session-time-accounting";
        let mut tool_call = observable_event(
            "evt-tool-1",
            session,
            "task-1",
            "trace-1",
            "tool_receipt",
            "tool",
            "2026-08-01T00:00:00Z",
        );
        tool_call.duration_ms = Some(1_234.0);
        let mut user_row = observable_event(
            "evt-user-1",
            session,
            "task-1",
            "trace-1",
            "packet_delivered",
            "user",
            "2026-08-01T00:00:01Z",
        );
        user_row.duration_ms = Some(9_999.0);
        ingest_events(&db, vec![tool_call, user_row]);

        let filter = ObservableEventQuery {
            session_id: Some(session.to_string()),
            limit: 100,
            ..Default::default()
        };
        let forge = db.query_observable_events_for_forge(&filter).unwrap();
        assert_eq!(
            forge.rows.len(),
            1,
            "forge time-accounting scope must exclude the non-tool-origin row"
        );
        assert_eq!(forge.rows[0].origin, "tool");
        assert_eq!(forge.rows[0].duration_ms, Some(1_234));

        let taste = db.query_observable_events_for_taste(&filter).unwrap();
        assert!(
            taste.rows.iter().all(|row| row.origin != "tool"),
            "taste scope must never see the tool-origin duration row"
        );
    }

    #[test]
    fn insights_scope_returns_the_full_authorized_stream() {
        let db = MemDb::open_in_memory();
        let session = "session-full-stream";
        let origins = ["user", "assistant", "tool", "host", "service", "repository"];
        let events: Vec<_> = origins
            .iter()
            .enumerate()
            .map(|(index, origin)| {
                observable_event(
                    &format!("evt-{index}"),
                    session,
                    "task-1",
                    "trace-1",
                    "packet_delivered",
                    origin,
                    &format!("2026-08-01T00:00:0{index}Z"),
                )
            })
            .collect();
        ingest_events(&db, events);

        let filter = ObservableEventQuery {
            session_id: Some(session.to_string()),
            limit: 100,
            ..Default::default()
        };
        let insights = db.query_observable_events_for_insights(&filter).unwrap();
        assert_eq!(insights.rows.len(), origins.len());
        let seen_origins: HashSet<String> =
            insights.rows.iter().map(|row| row.origin.clone()).collect();
        let expected_origins: HashSet<String> =
            origins.iter().map(|value| value.to_string()).collect();
        assert_eq!(seen_origins, expected_origins);
    }

    #[test]
    fn every_filter_dimension_selects_only_matching_rows() {
        let db = MemDb::open_in_memory();
        ingest_events(
            &db,
            vec![
                observable_event(
                    "evt-a",
                    "session-x",
                    "task-x",
                    "trace-x",
                    "packet_delivered",
                    "user",
                    "2026-08-01T00:00:00Z",
                ),
                observable_event(
                    "evt-b",
                    "session-x",
                    "task-y",
                    "trace-y",
                    "tool_receipt",
                    "tool",
                    "2026-08-01T01:00:00Z",
                ),
                observable_event(
                    "evt-c",
                    "session-y",
                    "task-x",
                    "trace-x",
                    "packet_delivered",
                    "user",
                    "2026-08-01T02:00:00Z",
                ),
                observable_event(
                    "evt-d",
                    "session-y",
                    "task-z",
                    "trace-z",
                    "feedback_recorded",
                    "assistant",
                    "2026-08-01T03:00:00Z",
                ),
            ],
        );
        let base = ObservableEventQuery {
            limit: 100,
            ..Default::default()
        };

        let by_time = db
            .query_observable_events_for_insights(&ObservableEventQuery {
                until: Some("2026-08-01T01:00:00Z".to_string()),
                ..base.clone()
            })
            .unwrap();
        assert_eq!(by_time.rows.len(), 2);

        let by_type = db
            .query_observable_events_for_insights(&ObservableEventQuery {
                event_type: Some("PACKET_DELIVERED".to_string()),
                ..base.clone()
            })
            .unwrap();
        assert_eq!(by_type.rows.len(), 2);
        assert!(by_type
            .rows
            .iter()
            .all(|row| row.event_type.as_deref() == Some("packet_delivered")));

        let by_session = db
            .query_observable_events_for_insights(&ObservableEventQuery {
                session_id: Some("session-x".to_string()),
                ..base.clone()
            })
            .unwrap();
        assert_eq!(by_session.rows.len(), 2);

        let by_task = db
            .query_observable_events_for_insights(&ObservableEventQuery {
                task_id: Some("task-x".to_string()),
                ..base.clone()
            })
            .unwrap();
        assert_eq!(by_task.rows.len(), 2);

        let by_trace = db
            .query_observable_events_for_insights(&ObservableEventQuery {
                trace_id: Some("trace-z".to_string()),
                ..base
            })
            .unwrap();
        assert_eq!(by_trace.rows.len(), 1);
        assert_eq!(
            by_trace.rows[0].trace_id,
            lifecycle_bounded_id("trace-z", "trace")
        );
    }

    #[test]
    fn truncation_and_resumption_are_reported_explicitly_never_silently() {
        let db = MemDb::open_in_memory();
        let session = "session-many";
        let events: Vec<_> = (0..5)
            .map(|index| {
                observable_event(
                    &format!("evt-{index}"),
                    session,
                    "task-1",
                    "trace-1",
                    "packet_delivered",
                    "user",
                    &format!("2026-08-01T00:00:0{index}Z"),
                )
            })
            .collect();
        ingest_events(&db, events);

        let page_one = db
            .query_observable_events_for_insights(&ObservableEventQuery {
                session_id: Some(session.to_string()),
                limit: 2,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(page_one.rows.len(), 2);
        assert!(page_one.truncated);
        assert!(page_one.next_cursor.is_some());

        let page_two = db
            .query_observable_events_for_insights(&ObservableEventQuery {
                session_id: Some(session.to_string()),
                limit: 2,
                after_sequence: page_one.next_cursor,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(page_two.rows.len(), 2);
        assert!(page_two.truncated);

        let page_three = db
            .query_observable_events_for_insights(&ObservableEventQuery {
                session_id: Some(session.to_string()),
                limit: 2,
                after_sequence: page_two.next_cursor,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(page_three.rows.len(), 1);
        assert!(!page_three.truncated);
        assert!(page_three.next_cursor.is_none());

        let paged_ids: Vec<_> = page_one
            .rows
            .iter()
            .chain(page_two.rows.iter())
            .chain(page_three.rows.iter())
            .map(|row| row.event_id.clone())
            .collect();
        let full = db
            .query_observable_events_for_insights(&ObservableEventQuery {
                session_id: Some(session.to_string()),
                limit: 100,
                ..Default::default()
            })
            .unwrap();
        let full_ids: Vec<_> = full.rows.iter().map(|row| row.event_id.clone()).collect();
        assert_eq!(
            paged_ids, full_ids,
            "paging via next_cursor must cover every row exactly once, in order"
        );
    }

    #[test]
    fn returned_rows_carry_no_raw_content_only_opaque_ids_and_digests() {
        let db = MemDb::open_in_memory();
        let raw_session = "session-contains-a-realistic-looking-user-prompt-do-not-leak-this";
        let digest = "c".repeat(64);
        let mut event = observable_event(
            "evt-content-free",
            raw_session,
            "task-1",
            "trace-1",
            "packet_delivered",
            "user",
            "2026-08-01T00:00:00Z",
        );
        event.content_ref_or_digest = format!("sha256:{digest}");
        ingest_events(&db, vec![event]);

        let result = db
            .query_observable_events_for_insights(&ObservableEventQuery {
                session_id: Some(raw_session.to_string()),
                limit: 10,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(result.rows.len(), 1);
        let row = &result.rows[0];
        assert_ne!(
            row.session_id, raw_session,
            "session id must be opaque, never the raw caller-supplied value"
        );
        assert_eq!(row.session_id, lifecycle_bounded_id(raw_session, "session"));
        assert_eq!(
            row.content_digest.as_deref(),
            Some(format!("sha256:{digest}").as_str())
        );
        let serialized = serde_json::to_string(row).unwrap();
        assert!(!serialized.contains("realistic-looking-user-prompt"));
    }
}
