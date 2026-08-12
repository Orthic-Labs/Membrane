//! Durable cross-run memory — the other half of the learning system.
//!
//! Where the learning harness captures *rules* ("avoid X"), memory captures
//! *experience*: what was attempted for a goal and whether it worked. After
//! each conductor run the memorizer records an entry; before planning the next
//! run, relevant entries are retrieved and injected so the system draws on past
//! experience. Persisted to the `memories` table of the evolution database.

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Instant, SystemTime};

use crypt_core::{
    consolidate_dream_memories, cosine, ContextTokenAccounting, DreamAgentPolicy, DreamStatus,
    EffectivenessGate, Embedder, HashEmbedder, MemoryEntry, MemoryRegistry,
    MemoryRetrievalEvalGate, MemoryRetriever, MemoryTier, MemoryUsageRecord, Outcome,
    RetrievalTier, RoutingPolicy,
};

use crate::context_telemetry::{
    append_context_events_on, ContextEvent, ContextEventBatch, ContextEventLink,
    ContextEventMeasurement, MeasurementValue, OperationAttribution,
};
use crate::feedback::{outcome_str, parse_outcome, FeedbackRecord, FeedbackSource};
use crate::memdb::MemDb;
use rusqlite::{OptionalExtension, TransactionBehavior};

/// Cap on retrieved memories injected into context, to keep prompts bounded.
const RETRIEVE_LIMIT: usize = 5;

/// Minimum cosine similarity for a semantic-only match (no shared keyword) to
/// count as relevant. Keeps unrelated memories out of the injected context.
const SEMANTIC_THRESHOLD: f32 = 0.30;

/// Token budget for injected memories (conservative estimate to leave room for
/// the goal, instructions, and other context).
const MEMORY_TOKEN_BUDGET: usize = 2000;

/// Soft preference for earlier scopes in a scope chain. This keeps exact-project
/// memories ahead on ties, while allowing a clearly better global/parent memory
/// to surface. Hard scope-rank sorting was too blunt for global design/audit
/// memories.
const SCOPE_CHAIN_SOFT_BONUS: f32 = 0.02;

/// Hybrid RRF is only a candidate generator; keep its scoped window wide enough that
/// semantically strong rules survive lexical noise before the final cosine rerank.
const SCOPED_CANDIDATE_FLOOR: usize = 128;

/// Query embeddings are independent of corpus mutations. A small content-free
/// cache removes repeated native inference without retaining prompt text.
const QUERY_EMBEDDING_CACHE_CAPACITY: usize = 256;
const MAX_MEMORY_BATCH_ITEMS: usize = 64;
const MAX_MEMORY_BATCH_CONTENT_CHARS: usize = 256 * 1024;
/// Enabled only after frozen-replay acceptance; pin/protection never bypasses admission gates.
const PROTECTED_PRIORITY_BONUS: f32 = 0.04;

fn protected_priority_bonus_enabled() -> bool {
    matches!(
        std::env::var("CRYPT_PROTECTED_PRIORITY_BONUS")
            .ok()
            .as_deref()
            .map(str::trim),
        Some("1") | Some("true") | Some("on")
    )
}

#[cfg(test)]
#[path = "../../../../tests/outcomes/causal_promotion.rs"]
mod causal_promotion;

/// Vector dispatch v2 (resident in-process f32 index) is default-on. Set
/// `CRYPT_VECTOR_DISPATCH_V2` to `0`/`false`/`off`/`legacy` for an immediate
/// fallback to the legacy scalar-A `retrieve_hybrid` routing (restored on next
/// store open). Any other value, or unset, keeps v2 active.
fn vector_dispatch_v2_enabled() -> bool {
    !matches!(
        std::env::var("CRYPT_VECTOR_DISPATCH_V2")
            .ok()
            .as_deref()
            .map(str::trim),
        Some("0") | Some("false") | Some("off") | Some("legacy")
    )
}

fn utility_recall_score(entry: &MemoryEntry, semantic_score: f32) -> f32 {
    let utility = entry.score.clamp(0.0, 1.0) as f32;
    let frequency = ((entry.access_count as f32 + 1.0_f32).ln() * 0.01_f32).min(0.04_f32);
    (semantic_score * (0.9_f32 + utility * 0.1_f32) + frequency).clamp(0.0_f32, 1.0_f32)
}

/// Monotonic counter so entries recorded within the same millisecond get
/// distinct ids (the registry is keyed by id; collisions would overwrite).
static MEM_SEQ: AtomicU64 = AtomicU64::new(0);

#[derive(Default)]
struct QueryEmbeddingCache {
    values: HashMap<String, Vec<f32>>,
    insertion_order: VecDeque<String>,
    hits: u64,
    misses: u64,
}

impl QueryEmbeddingCache {
    fn get(&mut self, key: &str) -> Option<Vec<f32>> {
        match self.values.get(key) {
            Some(value) => {
                self.hits = self.hits.saturating_add(1);
                Some(value.clone())
            }
            None => {
                self.misses = self.misses.saturating_add(1);
                None
            }
        }
    }

    fn insert(&mut self, key: String, value: Vec<f32>) -> Vec<f32> {
        if let Some(existing) = self.values.get(&key) {
            return existing.clone();
        }
        while self.values.len() >= QUERY_EMBEDDING_CACHE_CAPACITY {
            let Some(oldest) = self.insertion_order.pop_front() else {
                break;
            };
            self.values.remove(&oldest);
        }
        self.insertion_order.push_back(key.clone());
        self.values.insert(key, value.clone());
        value
    }

    fn snapshot(&self) -> serde_json::Value {
        serde_json::json!({
            "capacity": QUERY_EMBEDDING_CACHE_CAPACITY,
            "entries": self.values.len(),
            "hits": self.hits,
            "misses": self.misses,
        })
    }
}

/// Content-free attribution attached to a lifecycle event. Callers may identify the
/// product surface and correlate a session/trace, but cannot attach arbitrary text.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemoryEventContext {
    pub surface: String,
    pub session_id: Option<String>,
    pub turn_id: Option<String>,
    pub trace_id: Option<String>,
}

/// Optional lifecycle metadata supplied by every public memory authoring surface.
#[derive(Clone, Debug, Default, serde::Deserialize, serde::Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MemoryLifecycleInputV1 {
    pub effective_from_ms: Option<i64>,
    pub effective_until_ms: Option<i64>,
    pub expires_at_ms: Option<i64>,
    pub review_after_ms: Option<i64>,
    pub priority_class: Option<String>,
    pub confidence: Option<f64>,
    pub confidence_basis: Option<String>,
    pub supersedes: Option<String>,
    pub authority: Option<String>,
    pub influence_class: Option<String>,
}

impl MemoryLifecycleInputV1 {
    pub fn validate(&self) -> Result<(), String> {
        for value in [
            self.effective_from_ms,
            self.effective_until_ms,
            self.expires_at_ms,
            self.review_after_ms,
        ] {
            if value.is_some_and(|timestamp| timestamp < 0) {
                return Err("lifecycle timestamps must be non-negative milliseconds".into());
            }
        }
        if self
            .effective_from_ms
            .zip(self.effective_until_ms)
            .is_some_and(|(from, until)| from >= until)
        {
            return Err("effective_from_ms must precede effective_until_ms".into());
        }
        if self
            .confidence
            .is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value))
        {
            return Err("confidence must be within 0..=1".into());
        }
        if self
            .priority_class
            .as_deref()
            .is_some_and(|value| !matches!(value, "normal" | "protected"))
        {
            return Err("priority_class must be normal or protected".into());
        }
        if self
            .confidence_basis
            .as_ref()
            .is_some_and(|value| value.trim().is_empty() || value.len() > 1024)
        {
            return Err("confidence_basis is invalid".into());
        }
        if let Some(authority) = self.authority.as_deref() {
            if !matches!(authority, "A0" | "A1" | "A2" | "A3" | "A4" | "A5") {
                return Err("authority must be A0..=A5".into());
            }
        }
        if self
            .influence_class
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
        {
            return Err("influence_class must be non-empty when provided".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LearningUpdateTargetV1 {
    pub memory_id: String,
    pub content_sha256: String,
    pub delta_micros: i64,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LearningExperimentV1 {
    pub schema_version: u32,
    pub experiment_id: String,
    pub policy_version: String,
    pub policy_activation_sha256: String,
    pub task_class: String,
    pub observed_since: String,
    pub observed_through: String,
    pub min_per_cohort: u32,
    pub min_effect_bps: i32,
    pub targets: Vec<LearningUpdateTargetV1>,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LearningPromotionReceiptV1 {
    pub schema_version: u32,
    pub experiment_id: String,
    pub evidence_sha256: String,
    pub control_count: u32,
    pub control_success: u32,
    pub candidate_count: u32,
    pub candidate_success: u32,
    pub effect_bps: i32,
    pub disposition: String,
    pub reason_code: String,
    pub applied_count: u32,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryBatchRequest {
    pub batch_id: String,
    pub items: Vec<MemoryBatchItem>,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryBatchItem {
    pub item_id: String,
    pub name: String,
    pub content: String,
    pub scope: String,
    pub tier: String,
    pub artifact_family: String,
    pub producer: String,
    pub record_type: String,
    pub client: String,
    pub session_id: String,
    #[serde(default)]
    pub turn_id: String,
    pub trace_id: String,
    #[serde(default)]
    pub source_ids: Vec<String>,
    #[serde(default, flatten)]
    pub lifecycle: MemoryLifecycleInputV1,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct MemoryBatchItemReceipt {
    pub item_id: String,
    pub memory_id: String,
    pub status: String,
    pub artifact_family: String,
    pub producer: String,
    pub record_type: String,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct MemoryBatchReceipt {
    pub batch_id: String,
    pub inserted: usize,
    pub duplicates: usize,
    pub complete: bool,
    pub receipts: Vec<MemoryBatchItemReceipt>,
}

#[derive(Debug, thiserror::Error)]
pub enum MemoryBatchError {
    #[error("invalid memory batch: {0}")]
    Invalid(String),
    #[error("batch_id conflicts with an existing canonical request")]
    Conflict,
    #[error("memory batch persistence failed: {0}")]
    Persist(String),
}

/// Content-free, replayable lifecycle transition for one memory row.
///
/// `event_id` is the idempotency key. A supersession is deliberately one-to-one:
/// the old row becomes terminal and points at one verified replacement.
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MemoryLifecycleEventV1 {
    pub event_id: String,
    pub kind: MemoryLifecycleKind,
    pub subject_id: String,
    pub replacement_id: String,
    pub scope_id: String,
    pub effective_at_ms: i64,
    pub actor: String,
    pub authority: String,
    pub reason_ref: String,
    pub origin_event_uid: String,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryLifecycleKind {
    Superseded,
}

/// Trusted execution identity for lifecycle mutations.  This is deliberately
/// constructed outside request JSON (manifest/startup lease/loopback), so a
/// caller cannot self-assert actor or authority in a mutation body.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifiedMemoryActor {
    pub actor: String,
    pub authority: String,
    pub origin: String,
    pub session_id: String,
    pub trace_id: String,
}

impl VerifiedMemoryActor {
    pub fn from_execution_context(
        actor: &str,
        authority: &str,
        origin: &str,
        session_id: &str,
        trace_id: &str,
    ) -> Result<Self, String> {
        let actor = actor.trim();
        let authority = authority.trim();
        let origin = origin.trim();
        if actor.is_empty() || session_id.trim().is_empty() || trace_id.trim().is_empty() {
            return Err("verified actor, session_id, and trace_id are required".into());
        }
        if !matches!(origin, "manifest" | "startup_lease" | "loopback") {
            return Err("lifecycle origin is not a verified execution context".into());
        }
        if !matches!(authority, "A1" | "A2" | "A3" | "A4" | "A5") {
            return Err(
                "lifecycle authority must be verified (A1..=A5); A0 is not accepted".into(),
            );
        }
        if !valid_registry_token(actor)
            || !valid_registry_token(session_id)
            || !valid_registry_token(trace_id)
        {
            return Err("verified actor fields contain invalid tokens".into());
        }
        Ok(Self {
            actor: actor.into(),
            authority: authority.into(),
            origin: origin.into(),
            session_id: session_id.into(),
            trace_id: trace_id.into(),
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryLifecycleOperation {
    Create,
    Update,
    Supersede,
    Forget,
    Consolidate,
    Quarantine,
    Review,
    Explain,
    Restore,
}

/// Content-free operation descriptor.  Actor/provenance is supplied by
/// [`VerifiedMemoryActor`], never by this descriptor.
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MemoryLifecycleOperationV1 {
    pub operation: MemoryLifecycleOperation,
    pub memory_id: String,
    pub replacement_id: Option<String>,
    pub scope_id: Option<String>,
    pub expected_content_sha256: Option<String>,
    pub reason_ref: String,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub tier: Option<MemoryTier>,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryLifecycleReceiptV1 {
    pub schema: &'static str,
    pub operation: MemoryLifecycleOperation,
    pub memory_id: String,
    pub status: String,
    pub reversible: bool,
    pub actor: String,
    pub authority: String,
    pub origin: String,
    pub session_id: String,
    pub trace_id: String,
    pub reason_ref: String,
}

impl MemoryLifecycleEventV1 {
    #[allow(clippy::too_many_arguments)]
    pub fn superseded(
        event_id: impl Into<String>,
        subject_id: impl Into<String>,
        replacement_id: impl Into<String>,
        scope_id: impl Into<String>,
        effective_at_ms: i64,
        actor: impl Into<String>,
        authority: impl Into<String>,
        reason_ref: impl Into<String>,
        origin_event_uid: impl Into<String>,
    ) -> Self {
        Self {
            event_id: event_id.into(),
            kind: MemoryLifecycleKind::Superseded,
            subject_id: subject_id.into(),
            replacement_id: replacement_id.into(),
            scope_id: scope_id.into(),
            effective_at_ms,
            actor: actor.into(),
            authority: authority.into(),
            reason_ref: reason_ref.into(),
            origin_event_uid: origin_event_uid.into(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum MemoryLifecycleError {
    #[error("invalid lifecycle event: {0}")]
    Invalid(String),
    #[error("lifecycle subject does not exist: {0}")]
    MissingSubject(String),
    #[error("lifecycle replacement does not exist: {0}")]
    MissingReplacement(String),
    #[error("lifecycle supersession requires subject and replacement in the same scope")]
    CrossScope,
    #[error("lifecycle supersession cannot target itself")]
    SelfSupersession,
    #[error("lifecycle supersession would create a cycle")]
    Cycle,
    #[error("lifecycle subject is not active or already has a replacement")]
    SubjectNotActive,
    #[error("lifecycle event id conflicts with an existing event: {0}")]
    EventConflict(String),
    #[error("lifecycle persistence failed: {0}")]
    Persist(#[from] rusqlite::Error),
}

#[derive(Debug, thiserror::Error)]
pub enum MemoryPriorityError {
    #[error("priority target does not exist: {0}")]
    Missing(String),
    #[error("priority class is invalid: {0}")]
    InvalidClass(String),
    #[error("priority write is not authorized for actor={0} authority={1}")]
    Unauthorized(String, String),
    #[error("priority reason reference is required")]
    MissingReason,
    #[error("priority persistence failed: {0}")]
    Persist(#[from] rusqlite::Error),
}

#[derive(Clone, Debug)]
struct MemoryRecordMetadata {
    artifact_family: String,
    producer: String,
    record_type: String,
}

impl Default for MemoryRecordMetadata {
    fn default() -> Self {
        Self {
            artifact_family: "memory".into(),
            producer: "manual".into(),
            record_type: "memory".into(),
        }
    }
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct RelationshipGraphNode {
    pub id: String,
    pub tier: String,
    pub scope: String,
    pub state: String,
    pub weight: u32,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct RelationshipGraphEdge {
    pub source: String,
    pub target: String,
    pub kind: String,
    pub score: f32,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct RelationshipGraph {
    pub schema_version: u32,
    pub nodes: Vec<RelationshipGraphNode>,
    pub edges: Vec<RelationshipGraphEdge>,
}

#[derive(Clone, Debug)]
pub struct ScoredRecallHit {
    pub entry: MemoryEntry,
    pub score: f32,
    pub origin: &'static str,
}

/// One typed normal-recall result. Temporal facts stay structured so object
/// values never enter memory text or bypass scope/lifecycle admission.
#[derive(Clone, Debug)]
pub enum RecallResult {
    Memory {
        entry: MemoryEntry,
        score: f32,
    },
    Temporal {
        fact: crypt_store::TemporalFact,
        score: f32,
    },
}

/// Content-free wall-clock decomposition for the live memory-candidate lane.
/// Values are observational only and never affect retrieval or ranking.
#[derive(Clone, Copy, Debug, Default)]
pub struct RecallStageElapsed {
    pub embed_ms: f64,
    pub recall_ms: f64,
    pub rank_ms: f64,
}

impl MemoryEventContext {
    pub fn new(surface: &str) -> Self {
        let surface = surface.trim();
        let surface = if surface.is_empty() {
            "unknown".to_string()
        } else {
            surface.chars().take(64).collect()
        };
        let trace_id = new_uuid_v4()
            .ok()
            .or_else(|| Some(opaque_correlation_token("system-trace", "trace")));
        Self {
            session_id: Some(opaque_correlation_token(
                &format!("system-session:{surface}"),
                "session",
            )),
            surface,
            turn_id: None,
            trace_id,
        }
    }

    pub fn with_session(mut self, session_id: &str) -> Self {
        self.session_id = nonempty_bounded(session_id, 160)
            .map(|value| opaque_correlation_token(&value, "session"));
        self
    }

    pub fn with_trace(mut self, trace_id: &str) -> Self {
        self.trace_id =
            nonempty_bounded(trace_id, 160).map(|value| opaque_correlation_token(&value, "trace"));
        self
    }

    pub fn with_turn(mut self, turn_id: &str) -> Self {
        self.turn_id =
            nonempty_bounded(turn_id, 160).map(|value| opaque_correlation_token(&value, "turn"));
        self
    }

    /// Resolve a content-free decision identity from the process environment. Codex exposes its
    /// provider thread as `CODEX_THREAD_ID`; other clients can provide the portable CRYPT_*
    /// variables. Missing values receive unique opaque fallbacks rather than collapsing unrelated
    /// invocations into one synthetic session.
    pub fn from_environment(fallback_surface: &str) -> Self {
        let surface = std::env::var("CRYPT_CLIENT")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| fallback_surface.to_string());
        let mut context = Self::new(&surface);
        let session = std::env::var("CRYPT_SESSION_ID")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| {
                std::env::var("CODEX_THREAD_ID")
                    .ok()
                    .filter(|value| !value.trim().is_empty())
            })
            .or_else(|| new_uuid_v4().ok());
        if let Some(session) = session.as_deref() {
            context = context.with_session(session);
        }
        let trace = std::env::var("CRYPT_TRACE_ID")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| new_uuid_v4().ok())
            .or_else(|| context.trace_id.clone());
        if let Some(trace) = trace.as_deref() {
            context = context.with_trace(trace);
        }
        let turn = std::env::var("CRYPT_TURN_ID")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| trace.clone());
        if let Some(turn) = turn.as_deref() {
            context = context.with_turn(turn);
        }
        context
    }
}

impl Default for MemoryEventContext {
    fn default() -> Self {
        let inferred = std::env::current_exe()
            .ok()
            .and_then(|path| {
                path.file_stem()
                    .map(|name| name.to_string_lossy().to_lowercase())
            })
            .filter(|name| name.contains("coderight"))
            .map(|_| "coderight".to_string())
            .unwrap_or_else(|| "library".to_string());
        Self::from_environment(&inferred)
    }
}

fn nonempty_bounded(value: &str, max_chars: usize) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.chars().take(max_chars).collect())
}

fn registry_token(value: &str, fallback: &str) -> String {
    let token = value
        .trim()
        .to_ascii_lowercase()
        .chars()
        .map(|character| {
            if character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || matches!(character, '-' | '_' | '.' | ':')
            {
                character
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .chars()
        .take(80)
        .collect::<String>();
    if token.is_empty() {
        fallback.to_string()
    } else {
        token
    }
}

fn opaque_token(value: &str, prefix: &str) -> String {
    let value = value.trim();
    let canonical_prefixes = [format!("{prefix}-"), format!("{prefix}.")];
    if canonical_prefixes.iter().any(|opaque_prefix| {
        value.strip_prefix(opaque_prefix).is_some_and(|digest| {
            matches!(digest.len(), 32 | 64)
                && digest
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
    }) {
        value.to_string()
    } else {
        format!("{prefix}-{}", &content_hash(value)[..32])
    }
}

pub(crate) fn opaque_correlation_token(value: &str, prefix: &str) -> String {
    let value = value.trim();
    let lower = value.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let canonical_uuid = bytes.len() == 36
        && bytes.iter().enumerate().all(|(index, byte)| match index {
            8 | 13 | 18 | 23 => *byte == b'-',
            _ => byte.is_ascii_hexdigit(),
        });
    if canonical_uuid {
        return lower;
    }
    let opaque_prefix = format!("{prefix}-");
    if let Some(digest) = value.strip_prefix(&opaque_prefix) {
        if digest.len() == 32
            && digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        {
            return value.to_string();
        }
    }
    format!("{prefix}-{}", &content_hash(value)[..32])
}

fn stable_artifact_id(memory_id: &str) -> String {
    format!("memory.{}", &content_hash(memory_id)[..32])
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExternalLifecycleStage {
    Validation,
    Embedding,
    Commit,
    Provider,
}

fn normalized_session(context: &MemoryEventContext) -> String {
    context
        .session_id
        .as_deref()
        .map(|value| opaque_correlation_token(value, "session"))
        .unwrap_or_else(|| {
            opaque_correlation_token(&format!("system-session:{}", context.surface), "session")
        })
}

fn normalized_trace(context: &MemoryEventContext, seed: &str) -> String {
    context
        .trace_id
        .as_deref()
        .map(|value| opaque_correlation_token(value, "trace"))
        .unwrap_or_else(|| opaque_correlation_token(seed, "trace"))
}

fn normalized_turn(context: &MemoryEventContext, trace_id: &str) -> String {
    context
        .turn_id
        .as_deref()
        .map(|value| opaque_correlation_token(value, "turn"))
        .unwrap_or_else(|| opaque_correlation_token(trace_id, "turn"))
}

#[allow(clippy::too_many_arguments)]
fn lifecycle_event(
    attribution: &OperationAttribution,
    event_id: String,
    ts: &str,
    context: &MemoryEventContext,
    trace_id: &str,
    metadata: &MemoryRecordMetadata,
    phase: &str,
    operation: &str,
    status: &str,
    reason_code: Option<&str>,
    scope_id: Option<&str>,
    memory_id: Option<&str>,
    quantity: usize,
    links: Vec<ContextEventLink>,
) -> ContextEvent {
    ContextEvent {
        schema_version: 1,
        event_id: event_id.clone(),
        ts: ts.to_string(),
        installation_id: attribution.installation_id.clone(),
        service_instance_id: attribution.service_instance_id.clone(),
        workspace_id: attribution.workspace_id.clone(),
        client: registry_token(&context.surface, "unknown"),
        client_version: None,
        producer: registry_token(&metadata.producer, "manual"),
        producer_version: None,
        session_id: normalized_session(context),
        turn_id: Some(normalized_turn(context, trace_id)),
        trace_id: trace_id.to_string(),
        span_id: opaque_correlation_token(&event_id, "span"),
        parent_span_id: None,
        artifact_family: registry_token(&metadata.artifact_family, "memory"),
        provider: "crypt".to_string(),
        provider_version: Some(env!("CARGO_PKG_VERSION").to_string()),
        release_generation: Some(crate::release_identity::release_generation()),
        phase: phase.to_string(),
        operation: operation.to_string(),
        status: status.to_string(),
        reason_code: reason_code.map(|value| registry_token(value, "unknown")),
        scope_id: scope_id.map(|value| opaque_token(value, "scope")),
        artifact_id: memory_id.map(stable_artifact_id),
        artifact_sha256: None,
        traffic_class: "production".to_string(),
        policy_version: None,
        policy_activation_sha256: None,
        cohort: None,
        task_class: None,
        source_generation: None,
        duration_ms: None,
        quantity: Some(quantity as i64),
        token_count: None,
        char_count: None,
        meta: None,
        measurements: Vec::new(),
        links,
    }
}

#[allow(clippy::too_many_arguments)]
fn terminal_lifecycle_events(
    attribution: &OperationAttribution,
    base_id: &str,
    ts: &str,
    context: &MemoryEventContext,
    metadata: &MemoryRecordMetadata,
    operation: &str,
    stage: ExternalLifecycleStage,
    terminal_status: &str,
    reason_code: Option<&str>,
    scope_id: Option<&str>,
    memory_id: Option<&str>,
    quantity: usize,
) -> Vec<ContextEvent> {
    let trace_id = normalized_trace(context, base_id);
    let mut events = Vec::new();
    if matches!(operation, "write" | "update" | "delete") {
        events.push(lifecycle_event(
            attribution,
            format!("{base_id}.write.requested"),
            ts,
            context,
            &trace_id,
            metadata,
            "write.requested",
            operation,
            "expected",
            None,
            scope_id,
            memory_id,
            quantity,
            Vec::new(),
        ));
    }
    let mut pair = |phase: &str, terminal_phase: &str, status: &str, reason: Option<&str>| {
        let start_id = format!("{base_id}.{phase}");
        events.push(lifecycle_event(
            attribution,
            start_id.clone(),
            ts,
            context,
            &trace_id,
            metadata,
            phase,
            operation,
            "started",
            None,
            scope_id,
            memory_id,
            quantity,
            Vec::new(),
        ));
        events.push(lifecycle_event(
            attribution,
            format!("{base_id}.{terminal_phase}"),
            ts,
            context,
            &trace_id,
            metadata,
            terminal_phase,
            operation,
            status,
            reason,
            scope_id,
            memory_id,
            quantity,
            vec![ContextEventLink {
                relation: "terminal_of".to_string(),
                target_event_id: start_id,
            }],
        ));
    };

    match operation {
        "read" => pair(
            "provider.started",
            "provider.terminal",
            terminal_status,
            reason_code,
        ),
        "write" | "update" | "delete" => {
            let validation_status = if stage == ExternalLifecycleStage::Validation {
                terminal_status
            } else {
                "success"
            };
            let validation_reason = (stage == ExternalLifecycleStage::Validation)
                .then_some(reason_code)
                .flatten();
            pair(
                "validation.started",
                "validation.terminal",
                validation_status,
                validation_reason,
            );
            if stage == ExternalLifecycleStage::Validation {
                return events;
            }
            if operation != "delete" && terminal_status != "skipped" {
                let embedding_status = if stage == ExternalLifecycleStage::Embedding {
                    terminal_status
                } else {
                    "success"
                };
                let embedding_reason = (stage == ExternalLifecycleStage::Embedding)
                    .then_some(reason_code)
                    .flatten();
                pair(
                    "embedding.started",
                    "embedding.terminal",
                    embedding_status,
                    embedding_reason,
                );
                if stage == ExternalLifecycleStage::Embedding {
                    return events;
                }
            }
            pair(
                "commit.started",
                "commit.terminal",
                terminal_status,
                reason_code,
            );
        }
        _ => {}
    }
    events
}

fn operation_shape(
    event_kind: &str,
) -> (
    &'static str,
    &'static str,
    &'static str,
    Option<&'static str>,
) {
    match event_kind {
        "put" => ("commit.terminal", "write", "success", None),
        "update" => ("commit.terminal", "update", "success", None),
        "delete" => ("commit.terminal", "delete", "success", None),
        "get" => ("candidate.retrieved", "read", "success", None),
        "inject" => ("block.delivered", "read", "success", None),
        "curate_consolidate" => ("transform.terminal", "transform", "success", None),
        "curate_prune" => ("transform.terminal", "delete", "success", None),
        "curate_quarantine" => ("transform.terminal", "transform", "success", None),
        "quarantine_restore" => ("transform.terminal", "transform", "success", None),
        "outcome_success" => ("turn.outcome", "evaluate", "success", None),
        "outcome_failure" => ("turn.outcome", "evaluate", "failed", Some("task_failed")),
        _ => (
            "commit.terminal",
            "evaluate",
            "unknown",
            Some("unmapped_operation"),
        ),
    }
}

fn metadata_for_on(
    conn: &rusqlite::Connection,
    memory_id: &str,
) -> rusqlite::Result<MemoryRecordMetadata> {
    let current = conn
        .query_row(
            "SELECT artifact_family, producer, record_type FROM memories WHERE id=?1",
            rusqlite::params![memory_id],
            |row| {
                Ok(MemoryRecordMetadata {
                    artifact_family: row.get(0)?,
                    producer: row.get(1)?,
                    record_type: row.get(2)?,
                })
            },
        )
        .optional()?;
    if let Some(current) = current {
        return Ok(current);
    }
    let has_local_event_table: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='context_event_log')",
        [],
        |row| row.get(0),
    )?;
    if has_local_event_table {
        return conn
            .query_row(
                "SELECT artifact_family, producer FROM context_event_log
                  WHERE artifact_id=?1 ORDER BY seq DESC LIMIT 1",
                rusqlite::params![stable_artifact_id(memory_id)],
                |row| {
                    Ok(MemoryRecordMetadata {
                        artifact_family: row.get(0)?,
                        producer: row.get(1)?,
                        record_type: "memory".to_string(),
                    })
                },
            )
            .optional()
            .map(|value| value.unwrap_or_default());
    }
    Ok(MemoryRecordMetadata::default())
}

#[allow(clippy::too_many_arguments)]
fn log_memory_event(
    attribution: &OperationAttribution,
    conn: &rusqlite::Connection,
    event_kind: &str,
    memory_id: Option<&str>,
    scope_id: Option<&str>,
    context: &MemoryEventContext,
    quantity: usize,
    metadata: Option<&MemoryRecordMetadata>,
) -> rusqlite::Result<()> {
    let ts = crate::time::now_iso();
    let session_id = normalized_session(context);
    let trace_id = normalized_trace(context, &format!("legacy-{event_kind}-{ts}"));
    let turn_id = normalized_turn(context, &trace_id);
    let mut lifecycle_context = context.clone();
    lifecycle_context.session_id = Some(session_id.clone());
    lifecycle_context.trace_id = Some(trace_id.clone());
    conn.execute(
        "INSERT INTO memory_event_log
         (ts, event_kind, memory_id, surface, session_id, trace_id, scope_id, quantity, meta)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, '{}')",
        rusqlite::params![
            ts,
            event_kind,
            memory_id,
            context.surface,
            session_id,
            trace_id,
            scope_id,
            quantity as i64,
        ],
    )?;
    let legacy_rowid = conn.last_insert_rowid();
    let event_id = format!(
        "memory-event.{}.{}",
        attribution.installation_id, legacy_rowid
    );
    let client = registry_token(&context.surface, "unknown");
    let artifact_id = memory_id.map(stable_artifact_id);
    conn.execute(
        "UPDATE memory_event_log
         SET event_uid=?1, installation_id=?2, client=?3, turn_id=?4,
             artifact_id=?5, identity_status='observed'
         WHERE event_id=?6",
        rusqlite::params![
            event_id,
            attribution.installation_id,
            client,
            turn_id,
            artifact_id,
            legacy_rowid,
        ],
    )?;
    if let (Some(memory_id), Some(artifact_id)) = (memory_id, artifact_id.as_deref()) {
        conn.execute(
            "INSERT OR IGNORE INTO memory_identity
             (memory_id, artifact_id, origin_event_uid, installation_id, client,
              session_id, turn_id, trace_id, identity_status, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'observed', ?9)",
            rusqlite::params![
                memory_id,
                artifact_id,
                event_id,
                attribution.installation_id,
                client,
                session_id,
                turn_id,
                trace_id,
                ts,
            ],
        )?;
    }
    let metadata = match (metadata, memory_id) {
        (Some(metadata), _) => metadata.clone(),
        (None, Some(memory_id)) => metadata_for_on(conn, memory_id)?,
        (None, None) => MemoryRecordMetadata::default(),
    };
    let events = match event_kind {
        "put" | "update" => terminal_lifecycle_events(
            attribution,
            &event_id,
            &ts,
            &lifecycle_context,
            &metadata,
            if event_kind == "update" {
                "update"
            } else {
                "write"
            },
            ExternalLifecycleStage::Commit,
            "success",
            None,
            scope_id,
            memory_id,
            quantity,
        ),
        "delete" => terminal_lifecycle_events(
            attribution,
            &event_id,
            &ts,
            &lifecycle_context,
            &metadata,
            "delete",
            ExternalLifecycleStage::Commit,
            "success",
            None,
            scope_id,
            memory_id,
            quantity,
        ),
        "get" => {
            let mut events = terminal_lifecycle_events(
                attribution,
                &event_id,
                &ts,
                &lifecycle_context,
                &metadata,
                "read",
                ExternalLifecycleStage::Provider,
                "success",
                None,
                scope_id,
                memory_id,
                quantity,
            );
            events.insert(
                1,
                lifecycle_event(
                    attribution,
                    format!("{event_id}.candidate.retrieved"),
                    &ts,
                    &lifecycle_context,
                    &trace_id,
                    &metadata,
                    "candidate.retrieved",
                    "read",
                    "success",
                    None,
                    scope_id,
                    memory_id,
                    quantity,
                    Vec::new(),
                ),
            );
            events
        }
        _ => {
            let (phase, operation, status, reason_code) = operation_shape(event_kind);
            vec![lifecycle_event(
                attribution,
                event_id,
                &ts,
                &lifecycle_context,
                &trace_id,
                &metadata,
                phase,
                operation,
                status,
                reason_code,
                scope_id,
                memory_id,
                quantity,
                Vec::new(),
            )]
        }
    };
    append_context_events_on(conn, &ContextEventBatch { events })
        .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    Ok(())
}

/// Pick the embedding backend. With the `fastembed` feature, real ONNX sentence
/// embeddings; otherwise the dependency-free hash embedder.
///
/// FAIL-LOUD: when the binary was built WITH fastembed, a failed init ABORTS unless
/// `CRYPT_ALLOW_HASH=1` explicitly opts into the degraded hash embedder. The old
/// silent eprintln-and-continue fallback quietly filled the workspace DB with 256-dim
/// hash vectors (467 of 512 rows by 2026-07-02) while recall looked healthy on the
/// lexical channel — a corpus-corrupting failure mode, not a graceful degradation.
#[derive(Clone, Debug)]
pub struct MemoryHealth {
    pub ok: bool,
    pub embedder_dim: usize,
    pub embedder_model: &'static str,
    pub embedder_fingerprint: String,
    pub embedder_issue: Option<String>,
    pub writes_enabled: bool,
    pub last_persist_error: Option<String>,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillIndexEntry {
    pub name: String,
    pub description: String,
    pub body_hash: String,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillsSnapshot {
    pub schema_version: u32,
    pub generation: String,
    pub skills: Vec<SkillIndexEntry>,
}

fn default_embedder() -> (Arc<dyn Embedder>, Option<String>, bool) {
    #[cfg(feature = "fastembed")]
    {
        if std::env::var("CRYPT_ALLOW_HASH").as_deref() == Ok("1") {
            let issue = "CRYPT_ALLOW_HASH=1 enabled hash embedder".to_string();
            eprintln!("[memory] {issue}");
            return (Arc::new(HashEmbedder::new()), Some(issue), true);
        }
        match crypt_core::FastEmbedder::new() {
            Ok(f) => (Arc::new(f), None, true),
            Err(e) => {
                let issue = format!(
                    "fastembed init failed ({e}); memory writes disabled to avoid hash-vector corruption. Fix ORT_DYLIB_PATH/model cache, or set CRYPT_ALLOW_HASH=1 to accept degraded embeddings."
                );
                eprintln!("[memory] {issue}");
                (Arc::new(HashEmbedder::new()), Some(issue), false)
            }
        }
    }
    #[cfg(not(feature = "fastembed"))]
    {
        (Arc::new(HashEmbedder::new()), None, true)
    }
}

fn blob_to_embedding(b: &[u8]) -> Vec<f32> {
    b.chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

/// Encode an embedding as a quantized (i8 TurboQuant) blob — ~4× smaller than
/// the f32 representation, near-lossless at 8-bit.
fn embedding_to_quantized_blob(v: &[f32]) -> Vec<u8> {
    crypt_core::QuantizedVector::quantize(v).to_bytes()
}

/// Decode a quantized embedding blob back to f32, or `None` if it isn't a valid
/// quantized blob (lets the caller fall back to the legacy f32 column).
fn quantized_blob_to_embedding(b: &[u8]) -> Option<Vec<f32>> {
    crypt_core::QuantizedVector::from_bytes(b).map(|q| q.dequantize())
}

fn validate_embedding(vector: Vec<f32>, expected_dim: usize) -> Result<Vec<f32>, String> {
    if vector.len() != expected_dim {
        return Err(format!(
            "invalid embedding dimension: expected {expected_dim}, got {}",
            vector.len()
        ));
    }
    if !vector.iter().all(|value| value.is_finite()) {
        return Err("invalid embedding contains non-finite values".into());
    }
    if vector.iter().all(|value| *value == 0.0) {
        return Err("invalid embedding is a zero vector".into());
    }
    Ok(vector)
}

impl MemoryStore {
    /// Durable temporal-fact lane sharing this store's admission database.
    pub fn temporal_facts(&self) -> crypt_store::TemporalFactStore {
        crypt_store::TemporalFactStore::new(self.db.clone())
    }

    /// Compose ordinary memory recall with an explicitly scoped temporal query.
    /// Temporal facts use the same caller scope chain and fixed as-of instant;
    /// callers must opt into this typed lane rather than receiving content-free
    /// facts accidentally through text recall.
    pub fn recall_typed_at(
        &self,
        query: &str,
        limit: usize,
        scopes: &[String],
        as_of_ms: i64,
        include_expired: bool,
        temporal: Option<crypt_store::TemporalFactQuery>,
    ) -> Vec<RecallResult> {
        let mut out = Vec::new();
        if let Some(mut request) = temporal {
            if request.scope_chain.is_empty() {
                request.scope_chain = scopes.to_vec();
            }
            if request
                .scope_chain
                .iter()
                .any(|scope| !scopes.contains(scope))
            {
                return self
                    .recall_scored_at(query, limit, scopes, as_of_ms, include_expired)
                    .into_iter()
                    .map(|(entry, score)| RecallResult::Memory { entry, score })
                    .collect();
            }
            if let Ok(facts) = self.temporal_facts().query(request) {
                for fact in facts {
                    if out.len() >= limit {
                        break;
                    }
                    out.push(RecallResult::Temporal { fact, score: 1.0 });
                }
            }
        }
        let memory_limit = limit.saturating_sub(out.len());
        out.extend(
            self.recall_scored_at(query, memory_limit, scopes, as_of_ms, include_expired)
                .into_iter()
                .map(|(entry, score)| RecallResult::Memory { entry, score }),
        );
        out
    }
    fn embed_query_cached(&self, text: &str) -> Vec<f32> {
        let pipeline = self.embedder.pipeline_fingerprint();
        let key = format!("{}:{}:{}", pipeline.digest, "query", content_hash(text));
        if let Some(vector) = self
            .query_embedding_cache
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(&key)
        {
            return vector;
        }
        // Never hold the cache lock across native inference. The FastEmbedder's
        // own single-execution gate remains authoritative; a rare concurrent
        // duplicate miss is cheaper than convoying unrelated cache hits.
        let vector = self.embedder.embed_query(text);
        self.query_embedding_cache
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(key, vector)
    }

    fn embed_for_write(&self, text: &str) -> Result<Vec<f32>, String> {
        let vector = self
            .embedder
            .try_embed(text)
            .map_err(|e| self.persist_error(format!("embedding failed: {e}")))?;
        validate_embedding(vector, self.embedder.dim()).map_err(|e| self.persist_error(e))
    }

    fn embed_for_write_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, String> {
        let vectors = self
            .embedder
            .try_embed_batch(texts)
            .map_err(|e| self.persist_error(format!("batch embedding failed: {e}")))?;
        if vectors.len() != texts.len() {
            return Err(self.persist_error(format!(
                "batch embedding count mismatch: expected {}, got {}",
                texts.len(),
                vectors.len()
            )));
        }
        vectors
            .into_iter()
            .map(|vector| {
                validate_embedding(vector, self.embedder.dim()).map_err(|e| self.persist_error(e))
            })
            .collect()
    }

    /// Test-only: corrupt a row's embed_model tag to force a reindex miss.
    pub fn tamper_embed_model_for_tests(&self, id: &str, model: &str) {
        let _ = self.db.lock().execute(
            "UPDATE memories SET embed_model = ?1 WHERE id = ?2",
            rusqlite::params![model, id],
        );
    }
}

/// Content hash for embedding memoization: a row whose (content_hash, embed_model)
/// both match the current embedder needs no re-embedding on `reindex`.
/// SKILL.md frontmatter `description`, folding `>`/`|` block scalars — byte-parity with the shared
/// YAML-free parser (`tools/lib/skill_frontmatter.py`) so catalog/verifier/engine agree.
fn skill_frontmatter_description(body: &str) -> String {
    if !body.trim_start().starts_with("---") {
        return String::new();
    }
    let mut parts = body.splitn(3, "---");
    let (_, Some(fm), Some(_)) = (parts.next(), parts.next(), parts.next()) else {
        return String::new();
    };
    let lines: Vec<&str> = fm.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        let stripped = line.trim();
        if !stripped.to_lowercase().starts_with("description:") {
            continue;
        }
        let value = stripped
            .split_once(':')
            .map(|pair| pair.1)
            .unwrap_or("")
            .trim();
        if matches!(value, ">" | "|" | ">-" | "|-" | ">+" | "|+") {
            let mut collected = Vec::new();
            for cont in &lines[i + 1..] {
                if !cont.trim().is_empty() && !cont.starts_with(' ') && !cont.starts_with('\t') {
                    break;
                }
                let t = cont.trim();
                if !t.is_empty() {
                    collected.push(t);
                }
            }
            return collected.join(" ");
        }
        return value.trim_matches('"').trim_matches('\'').to_string();
    }
    String::new()
}

fn content_hash(content: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(content.as_bytes());
    format!("{:x}", h.finalize())
}

fn valid_opaque_id(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value.is_ascii()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._:-".contains(&byte))
        && value.as_bytes()[0].is_ascii_alphanumeric()
}

fn valid_registry_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 80
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"._:-".contains(&byte)
        })
        && (value.as_bytes()[0].is_ascii_lowercase() || value.as_bytes()[0].is_ascii_digit())
}

fn memory_batch_request_hash(request: &MemoryBatchRequest) -> Result<String, MemoryBatchError> {
    use sha2::{Digest, Sha256};

    let encoded = serde_json::to_vec(request)
        .map_err(|_| MemoryBatchError::Invalid("request is not canonical JSON".into()))?;
    Ok(format!("{:x}", Sha256::digest(encoded)))
}

fn memory_batch_item_hash(item: &MemoryBatchItem) -> Result<String, MemoryBatchError> {
    use sha2::{Digest, Sha256};

    let encoded = serde_json::to_vec(item)
        .map_err(|_| MemoryBatchError::Invalid("item is not canonical JSON".into()))?;
    Ok(format!("{:x}", Sha256::digest(encoded)))
}

fn memory_batch_tier(value: &str) -> Result<MemoryTier, MemoryBatchError> {
    match value {
        "Working" => Ok(MemoryTier::Working),
        "Episodic" => Ok(MemoryTier::Episodic),
        "Semantic" => Ok(MemoryTier::Semantic),
        _ => Err(MemoryBatchError::Invalid("tier is not registered".into())),
    }
}

type DreamSnapshotRow = (String, String, String, String, u64, String, i64, String);

fn dream_snapshot_digest(mut rows: Vec<DreamSnapshotRow>) -> String {
    use sha2::{Digest, Sha256};

    rows.sort_by(|left, right| left.0.cmp(&right.0));
    let mut hash = Sha256::new();
    let mut field = |bytes: &[u8]| {
        hash.update(bytes.len().to_le_bytes());
        hash.update(bytes);
    };
    for (id, tier, content, keywords, score_bits, created_at, access_count, scope_id) in rows {
        field(id.as_bytes());
        field(tier.as_bytes());
        field(content.as_bytes());
        field(keywords.as_bytes());
        field(&score_bits.to_le_bytes());
        field(created_at.as_bytes());
        field(&access_count.to_le_bytes());
        field(scope_id.as_bytes());
    }
    format!("{:x}", hash.finalize())
}

fn dream_registry_digest(entries: &[MemoryEntry]) -> Result<String, String> {
    let rows = entries
        .iter()
        .map(|entry| {
            Ok((
                entry.id.clone(),
                serde_json::to_string(&entry.tier)
                    .map_err(|error| format!("serialize dream tier: {error}"))?,
                entry.content.clone(),
                serde_json::to_string(&entry.keywords)
                    .map_err(|error| format!("serialize dream keywords: {error}"))?,
                entry.score.to_bits(),
                entry.created_at.clone(),
                i64::from(entry.access_count),
                entry.scope_id.clone(),
            ))
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(dream_snapshot_digest(rows))
}

fn dream_database_digest(
    connection: &rusqlite::Connection,
    eligible_ids: &HashSet<String>,
) -> Result<String, String> {
    let mut statement = connection
        .prepare(
            "SELECT id, tier, content, keywords, score, created_at, access_count, scope_id
             FROM memories ORDER BY id",
        )
        .map_err(|error| format!("prepare dream corpus snapshot: {error}"))?;
    let rows = statement
        .query_map([], |row| {
            let score = row.get::<_, f64>(4)?;
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                score.to_bits(),
                row.get::<_, String>(5)?,
                row.get::<_, i64>(6)?,
                row.get::<_, String>(7)?,
            ))
        })
        .and_then(|mapped| mapped.collect::<rusqlite::Result<Vec<_>>>())
        .map_err(|error| format!("read dream corpus snapshot: {error}"))?;
    Ok(dream_snapshot_digest(
        rows.into_iter()
            .filter(|row| eligible_ids.contains(&row.0))
            .collect(),
    ))
}

fn recall_eligible_ids_on(
    connection: &rusqlite::Connection,
    as_of_ms: i64,
    include_expired: bool,
) -> rusqlite::Result<HashSet<String>> {
    let expiry_clause = if include_expired {
        ""
    } else {
        " AND (effective_from_ms IS NULL OR effective_from_ms <= ?1) AND (effective_until_ms IS NULL OR ?1 < effective_until_ms) AND (expires_at_ms IS NULL OR ?1 < expires_at_ms)"
    };
    let query = format!(
        "SELECT id FROM memories WHERE authority IN ('A1','A2','A3','A4','A5') AND ((lifecycle_state='active' AND superseded_by IS NULL) OR (lifecycle_state='superseded' AND superseded_by IS NOT NULL AND effective_until_ms IS NOT NULL AND ?1 < effective_until_ms)){expiry_clause}"
    );
    connection
        .prepare(&query)?
        .query_map(rusqlite::params![as_of_ms], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<HashSet<_>>>()
}

fn expired_curation_ids_on(
    connection: &rusqlite::Connection,
    as_of_ms: i64,
) -> rusqlite::Result<HashSet<String>> {
    connection
        .prepare(
            "SELECT id FROM memories
             WHERE authority IN ('A1','A2','A3','A4','A5')
               AND lifecycle_state='active' AND superseded_by IS NULL
               AND ((effective_until_ms IS NOT NULL AND effective_until_ms <= ?1)
                 OR (expires_at_ms IS NOT NULL AND expires_at_ms <= ?1))",
        )?
        .query_map(rusqlite::params![as_of_ms], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<HashSet<_>>>()
}

fn db_path(db: &MemDb) -> Option<PathBuf> {
    db.lock()
        .query_row("PRAGMA database_list", [], |row| row.get::<_, String>(2))
        .ok()
        .and_then(|path| {
            let trimmed = path.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(PathBuf::from(trimmed))
            }
        })
}

fn new_uuid_v4() -> Result<String, String> {
    let mut bytes = [0u8; 16];
    getrandom::getrandom(&mut bytes)
        .map_err(|error| format!("generate operation UUID: {error}"))?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Ok(format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]
    ))
}

fn workspace_root_for_store(db_path: Option<&Path>) -> Option<PathBuf> {
    if db_path.is_none() {
        return None;
    }
    if let Some(configured) = std::env::var_os("WORKSPACE_ROOT") {
        let configured = PathBuf::from(configured);
        if !configured.as_os_str().is_empty() {
            return Some(configured);
        }
    }
    db_path.and_then(|path| {
        path.ancestors()
            .find(|ancestor| ancestor.file_name().is_some_and(|name| name == "tools"))
            .and_then(Path::parent)
            .map(Path::to_path_buf)
    })
}

pub fn operation_attribution_for_store(
    db_path: Option<&Path>,
) -> Result<OperationAttribution, String> {
    let Some(workspace_root) = workspace_root_for_store(db_path) else {
        let installation_id = new_uuid_v4()?;
        let service_instance_id = new_uuid_v4()?;
        return OperationAttribution::new(
            &installation_id,
            &service_instance_id,
            &format!("ws.{}", &content_hash("workspace.unknown")[..32]),
        )
        .map_err(|error| error.to_string());
    };
    let paths = crate::installation_identity::InstallationPaths::for_workspace(&workspace_root);
    let identity = crate::installation_identity::load_or_create_installation(&paths.identity, &[])
        .map_err(|error| format!("load operation installation identity: {error}"))?;
    let service_instance_id = std::env::var("CRYPT_SERVICE_INSTANCE_ID")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map_or_else(new_uuid_v4, Ok)?;
    let workspace_seed = std::env::var("CRYPT_WORKSPACE_ID")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| {
            workspace_root
                .file_name()
                .map(|value| value.to_string_lossy().to_ascii_lowercase())
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| "unknown".to_string())
        });
    OperationAttribution::new(
        &identity.installation_id,
        &service_instance_id,
        &format!("ws.{}", &content_hash(&workspace_seed)[..32]),
    )
    .map_err(|error| error.to_string())
}

#[derive(Debug, Clone)]
struct MtimeLock {
    path: Option<PathBuf>,
    observed: Option<SystemTime>,
}

impl MtimeLock {
    fn acquire(path: Option<PathBuf>) -> Self {
        let observed = path
            .as_deref()
            .and_then(|p| std::fs::metadata(p).ok())
            .and_then(|m| m.modified().ok());
        Self { path, observed }
    }

    fn verify(&self) -> Result<(), String> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let current = std::fs::metadata(path)
            .map_err(|e| format!("memory DB mtime-lock metadata failed: {e}"))?
            .modified()
            .map_err(|e| format!("memory DB mtime-lock modified time failed: {e}"))?;
        if Some(current) == self.observed {
            Ok(())
        } else {
            Err("memory DB changed while dream consolidation was preparing writes".into())
        }
    }
}

/// Shared, cloneable handle to the memory registry.
#[derive(Clone)]
pub struct MemoryStore {
    registry: Arc<RwLock<MemoryRegistry>>,
    db: MemDb,
    db_path: Option<PathBuf>,
    embedder: Arc<dyn Embedder>,
    query_embedding_cache: Arc<Mutex<QueryEmbeddingCache>>,
    routing_policy: Arc<Mutex<RoutingPolicy>>,
    effectiveness_history: Arc<Mutex<Vec<MemoryUsageRecord>>>,
    pending_injections: Arc<Mutex<Vec<String>>>,
    last_recall_status: Arc<Mutex<Option<String>>>,
    last_route: Arc<Mutex<Option<(crypt_core::QueryFeatures, RetrievalTier)>>>,
    dream_status: Arc<Mutex<DreamStatus>>,
    embedder_issue: Option<String>,
    writes_enabled: bool,
    last_persist_error: Arc<Mutex<Option<String>>>,
    operation_attribution: OperationAttribution,
}

impl Default for MemoryStore {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryStore {
    /// Backed by an ephemeral in-memory database (tests / no home dir).
    pub fn new() -> Self {
        Self::open(MemDb::open_in_memory())
    }

    /// Load persisted entries from `db` and write through to it.
    pub fn open(db: MemDb) -> Self {
        Self::try_open(db).unwrap_or_else(|e| panic!("memory registry load failed: {e}"))
    }

    pub fn try_open(db: MemDb) -> Result<Self, String> {
        let attribution = operation_attribution_for_store(db_path(&db).as_deref())?;
        Self::try_open_with_operation_attribution(db, attribution)
    }

    pub fn open_with_operation_attribution(db: MemDb, attribution: OperationAttribution) -> Self {
        Self::try_open_with_operation_attribution(db, attribution)
            .unwrap_or_else(|error| panic!("memory registry load failed: {error}"))
    }

    pub fn try_open_with_operation_attribution(
        db: MemDb,
        attribution: OperationAttribution,
    ) -> Result<Self, String> {
        let mut registry = if vector_dispatch_v2_enabled() {
            MemoryRegistry::new_indexed()
        } else {
            MemoryRegistry::new()
        };
        let db_path = db_path(&db);
        let (embedder, embedder_issue, writes_enabled) = default_embedder();
        {
            let conn = db.lock();
            #[allow(clippy::type_complexity)]
            let rows: Vec<(
                String,
                String,
                String,
                String,
                f64,
                String,
                i64,
                Option<Vec<u8>>,
                Option<Vec<u8>>,
                String,
            )> = conn
                .prepare(
                    "SELECT id, tier, content, keywords, score, created_at, access_count, embedding, embedding_q, scope_id \
                     FROM memories",
                )
                .and_then(|mut stmt| {
                    stmt.query_map([], |r| {
                        Ok((
                            r.get::<_, String>(0)?,
                            r.get::<_, String>(1)?,
                            r.get::<_, String>(2)?,
                            r.get::<_, String>(3)?,
                            r.get::<_, f64>(4)?,
                            r.get::<_, String>(5)?,
                            r.get::<_, i64>(6)?,
                            r.get::<_, Option<Vec<u8>>>(7)?,
                            r.get::<_, Option<Vec<u8>>>(8)?,
                            r.get::<_, String>(9)?,
                        ))
                    })
                    .and_then(|rows| rows.collect())
                })
                .map_err(|e| format!("registry row load failed: {e}"))?;
            for (
                id,
                tier,
                content,
                keywords,
                score,
                created_at,
                access_count,
                embedding,
                embedding_q,
                scope_id,
            ) in rows
            {
                // Prefer the quantized blob; fall back to the legacy f32 blob.
                let embedding = match embedding_q {
                    Some(blob) => Some(quantized_blob_to_embedding(&blob).ok_or_else(|| {
                        format!("registry row {id:?} has invalid quantized embedding")
                    })?),
                    None => match embedding {
                        Some(blob) if blob.len() % 4 != 0 => {
                            return Err(format!(
                                "registry row {id:?} has invalid f32 embedding blob"
                            ));
                        }
                        Some(blob) => Some(blob_to_embedding(&blob)),
                        None => None,
                    },
                };
                let tier = serde_json::from_str(&tier)
                    .map_err(|e| format!("registry row {id:?} has invalid tier: {e}"))?;
                let keywords = serde_json::from_str(&keywords)
                    .map_err(|e| format!("registry row {id:?} has invalid keywords: {e}"))?;
                if access_count < 0 {
                    return Err(format!("registry row {id:?} has negative access_count"));
                }
                registry.insert(MemoryEntry {
                    id,
                    tier,
                    content,
                    keywords,
                    score,
                    created_at,
                    access_count: access_count as u32,
                    embedding,
                    scope_id,
                });
            }
        }
        let store = Self {
            registry: Arc::new(RwLock::new(registry)),
            db,
            db_path,
            embedder,
            query_embedding_cache: Arc::new(Mutex::new(QueryEmbeddingCache::default())),
            routing_policy: Arc::new(Mutex::new(RoutingPolicy::new())),
            effectiveness_history: Arc::new(Mutex::new(Vec::new())),
            pending_injections: Arc::new(Mutex::new(Vec::new())),
            last_recall_status: Arc::new(Mutex::new(None)),
            last_route: Arc::new(Mutex::new(None)),
            dream_status: Arc::new(Mutex::new(DreamStatus {
                agent_id: "dream-memory".into(),
                status: "idle".into(),
                model: DreamAgentPolicy::restricted().model,
                shell_allowed: false,
                read_count: 0,
                consolidated_count: 0,
                pruned_count: 0,
                quarantined_count: 0,
            })),
            embedder_issue,
            writes_enabled,
            last_persist_error: Arc::new(Mutex::new(None)),
            operation_attribution: attribution,
        };
        Ok(store)
    }

    /// Record an external operation that terminates before the normal store path can append its
    /// atomic success lifecycle. The event set is content-free and committed as one linked batch.
    #[allow(clippy::too_many_arguments)]
    pub fn record_external_lifecycle(
        &self,
        context: &MemoryEventContext,
        operation: &str,
        stage: ExternalLifecycleStage,
        status: &str,
        reason_code: &str,
        memory_id: Option<&str>,
        scope_id: Option<&str>,
        artifact_family: &str,
        producer: &str,
        quantity: usize,
    ) -> Result<(), String> {
        if !matches!(operation, "read" | "write" | "update" | "delete")
            || !matches!(
                status,
                "success" | "empty" | "skipped" | "failed" | "timeout" | "unavailable"
            )
            || reason_code.trim().is_empty()
            || !crate::context_telemetry::registered_artifact_family(artifact_family)
            || !valid_registry_token(producer)
        {
            return Err("external lifecycle attribution is invalid".into());
        }
        let base_id = format!("external.{}", new_uuid_v4()?);
        let ts = crate::time::now_iso();
        let metadata = MemoryRecordMetadata {
            artifact_family: artifact_family.to_string(),
            producer: producer.to_string(),
            record_type: "external_attempt".to_string(),
        };
        let events = terminal_lifecycle_events(
            &self.operation_attribution,
            &base_id,
            &ts,
            context,
            &metadata,
            operation,
            stage,
            status,
            Some(reason_code),
            scope_id,
            memory_id,
            quantity,
        );
        let mut conn = self.db.lock();
        let tx = conn.transaction().map_err(|error| {
            self.persist_error(format!("external lifecycle transaction failed: {error}"))
        })?;
        append_context_events_on(&tx, &ContextEventBatch { events }).map_err(|error| {
            self.persist_error(format!("external lifecycle append failed: {error}"))
        })?;
        tx.commit().map_err(|error| {
            self.persist_error(format!("external lifecycle commit failed: {error}"))
        })?;
        drop(conn);
        self.flush_event_outbox()?;
        Ok(())
    }

    /// Write one entry through to the database.
    fn persist_entry_on(
        &self,
        conn: &rusqlite::Connection,
        entry: &MemoryEntry,
        updated_at: &str,
        source_ids: &[String],
    ) -> Result<(), String> {
        self.persist_entry_with_record_on(
            conn,
            entry,
            updated_at,
            source_ids,
            &MemoryRecordMetadata::default(),
        )
    }

    fn persist_entry_with_record_on(
        &self,
        conn: &rusqlite::Connection,
        entry: &MemoryEntry,
        updated_at: &str,
        source_ids: &[String],
        metadata: &MemoryRecordMetadata,
    ) -> Result<(), String> {
        self.persist_entry_with_record_lifecycle_on(
            conn,
            entry,
            updated_at,
            source_ids,
            metadata,
            &MemoryLifecycleInputV1::default(),
        )
    }

    fn persist_entry_with_record_lifecycle_on(
        &self,
        conn: &rusqlite::Connection,
        entry: &MemoryEntry,
        updated_at: &str,
        source_ids: &[String],
        metadata: &MemoryRecordMetadata,
        lifecycle: &MemoryLifecycleInputV1,
    ) -> Result<(), String> {
        lifecycle.validate()?;
        let authority = lifecycle.authority.as_deref().unwrap_or("A2");
        let influence_class = lifecycle.influence_class.as_deref().unwrap_or("reference");
        if !self.writes_enabled {
            let err = self.embedder_issue.clone().unwrap_or_else(|| {
                "memory writes disabled because the configured embedder is unavailable".into()
            });
            self.set_last_persist_error(err.clone());
            return Err(err);
        }
        let tier = serde_json::to_string(&entry.tier).unwrap_or_else(|_| "\"Episodic\"".into());
        let keywords = serde_json::to_string(&entry.keywords).unwrap_or_else(|_| "[]".into());
        let source_ids = serde_json::to_string(source_ids)
            .map_err(|e| self.persist_error(format!("memory provenance encode failed: {e}")))?;
        // Store the compact quantized blob; leave the legacy f32 column NULL.
        let blob_q = entry.embedding.as_deref().map(embedding_to_quantized_blob);
        conn.execute(
            "INSERT INTO memories
                 (id, tier, content, keywords, score, created_at, updated_at, access_count, embedding, embedding_q, scope_id, content_hash, embed_model, source_ids, artifact_family, producer, record_type, authority, influence_class, effective_from_ms, effective_until_ms, expires_at_ms, review_after_ms, priority_class, confidence, confidence_basis)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, NULL, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, COALESCE(?23, 'normal'), ?24, ?25)
             ON CONFLICT(id) DO UPDATE SET
                 tier=excluded.tier, content=excluded.content, keywords=excluded.keywords,
                 score=excluded.score, updated_at=excluded.updated_at, embedding=NULL,
                 embedding_q=excluded.embedding_q, scope_id=excluded.scope_id,
                 content_hash=excluded.content_hash, embed_model=excluded.embed_model,
                 source_ids=excluded.source_ids, artifact_family=excluded.artifact_family,
                 producer=excluded.producer, record_type=excluded.record_type,
                 authority=excluded.authority, influence_class=excluded.influence_class,
                 effective_from_ms=COALESCE(excluded.effective_from_ms, memories.effective_from_ms),
                 effective_until_ms=COALESCE(excluded.effective_until_ms, memories.effective_until_ms),
                 expires_at_ms=COALESCE(excluded.expires_at_ms, memories.expires_at_ms),
                 review_after_ms=COALESCE(excluded.review_after_ms, memories.review_after_ms),
                 priority_class=CASE WHEN ?26 THEN excluded.priority_class ELSE memories.priority_class END,
                 confidence=COALESCE(excluded.confidence, memories.confidence),
                 confidence_basis=COALESCE(excluded.confidence_basis, memories.confidence_basis)",
            rusqlite::params![
                entry.id,
                tier,
                entry.content,
                keywords,
                entry.score,
                entry.created_at,
                updated_at,
                entry.access_count as i64,
                blob_q,
                entry.scope_id,
                content_hash(&entry.content),
                self.embedder.model_id(),
                source_ids,
                metadata.artifact_family,
                metadata.producer,
                metadata.record_type,
                authority,
                influence_class,
                lifecycle.effective_from_ms,
                lifecycle.effective_until_ms,
                lifecycle.expires_at_ms,
                lifecycle.review_after_ms,
                lifecycle.priority_class.as_deref(),
                lifecycle.confidence,
                lifecycle.confidence_basis.as_deref(),
                lifecycle.priority_class.is_some(),
            ],
        )
        .map_err(|e| self.persist_error(format!("memory persist failed for {}: {e}", entry.id)))?;
        // A (re)written id is alive again — drop any deletion tombstone so cross-machine
        // sync doesn't re-delete a re-created memory.
        conn.execute(
            "DELETE FROM deletions WHERE id = ?1",
            rusqlite::params![entry.id],
        )
        .map_err(|e| {
            self.persist_error(format!(
                "memory tombstone cleanup failed for {}: {e}",
                entry.id
            ))
        })?;
        // Persist this memory's `[[wikilink]]` edges (the OKF link graph) so linked-neighbour
        // recall can one-hop from a semantic hit. Replace the src's edge set (an edit changes links).
        conn.execute(
            "DELETE FROM links WHERE src_id = ?1",
            rusqlite::params![entry.id],
        )
        .map_err(|e| self.persist_error(format!("link cleanup failed for {}: {e}", entry.id)))?;
        for dst in crypt_core::wikilinks(&entry.content) {
            conn.execute(
                "INSERT INTO links (src_id, dst_slug) VALUES (?1, ?2) ON CONFLICT DO NOTHING",
                rusqlite::params![entry.id, dst],
            )
            .map_err(|e| {
                self.persist_error(format!("link persist failed for {}: {e}", entry.id))
            })?;
        }
        Ok(())
    }

    /// Rebuild the entire `links` table from every memory's content. Idempotent; covers rows
    /// written before link-persistence and any write path that bypasses `persist_entry_on`.
    /// Returns (edges_written, resolvable_slugs) — resolvable = distinct targets that match a real
    /// memory id, the concrete dangling-link-rate measurement.
    pub fn backfill_links(&self) -> (usize, usize) {
        let conn = self.db.lock();
        let _ = conn.execute("DELETE FROM links", []);
        let rows: Vec<(String, String)> = match conn.prepare("SELECT id, content FROM memories") {
            Ok(mut stmt) => stmt
                .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
                .map(|it| it.flatten().collect())
                .unwrap_or_default(),
            Err(_) => return (0, 0),
        };
        let mut edges = 0usize;
        for (id, content) in &rows {
            for dst in crypt_core::wikilinks(content) {
                if conn
                    .execute(
                        "INSERT INTO links (src_id, dst_slug) VALUES (?1, ?2) ON CONFLICT DO NOTHING",
                        rusqlite::params![id, dst],
                    )
                    .unwrap_or(0)
                    > 0
                {
                    edges += 1;
                }
            }
        }
        let resolvable = conn
            .query_row(
                "SELECT COUNT(DISTINCT dst_slug) FROM links l \
                 WHERE EXISTS (SELECT 1 FROM memories m WHERE m.id = l.dst_slug OR m.id LIKE '%/' || l.dst_slug)",
                [],
                |r| r.get::<_, i64>(0),
            )
            .unwrap_or(0) as usize;
        (edges, resolvable)
    }

    /// One-hop linked neighbours of the seed memories, resolved to real in-scope entries and
    /// returned at a fixed discounted score (below any direct semantic hit). Dangling links (target
    /// not written, or out of scope) resolve to nothing — no phantom candidate.
    pub fn linked_neighbors(
        &self,
        seed_ids: &[String],
        scopes: &[String],
        limit: usize,
    ) -> Vec<(MemoryEntry, f32)> {
        const NEIGHBOR_SCORE: f32 = 0.3;
        if seed_ids.is_empty() || limit == 0 {
            return Vec::new();
        }
        let slugs: Vec<String> = {
            let conn = self.db.lock();
            let mut stmt = match conn.prepare("SELECT dst_slug FROM links WHERE src_id = ?1") {
                Ok(s) => s,
                Err(_) => return Vec::new(),
            };
            let mut out = Vec::new();
            for sid in seed_ids {
                if let Ok(rows) = stmt.query_map(rusqlite::params![sid], |r| r.get::<_, String>(0))
                {
                    out.extend(rows.flatten());
                }
            }
            out
        };
        if slugs.is_empty() {
            return Vec::new();
        }
        let reg = self.registry.read().unwrap_or_else(|e| e.into_inner());
        let seed_set: std::collections::HashSet<&String> = seed_ids.iter().collect();
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for slug in slugs {
            for scope in scopes {
                let cand_id = format!("{scope}/{slug}");
                if seed_set.contains(&cand_id) || !seen.insert(cand_id.clone()) {
                    continue;
                }
                if let Some(entry) = reg.get(&cand_id) {
                    out.push((entry.clone(), NEIGHBOR_SCORE));
                    break;
                }
            }
            if out.len() >= limit {
                break;
            }
        }
        out
    }

    /// Write one entry and clear its tombstone in one transaction.
    fn persist_entry(&self, entry: &MemoryEntry) -> Result<(), String> {
        let mut conn = self.db.lock();
        let tx = conn
            .transaction()
            .map_err(|e| self.persist_error(format!("memory transaction failed: {e}")))?;
        let existed = tx
            .query_row(
                "SELECT 1 FROM memories WHERE id = ?1",
                rusqlite::params![entry.id],
                |_| Ok(()),
            )
            .optional()
            .map_err(|e| self.persist_error(format!("memory event lookup failed: {e}")))?
            .is_some();
        self.persist_entry_on(&tx, entry, &crate::time::now_iso(), &[])?;
        log_memory_event(
            &self.operation_attribution,
            &tx,
            if existed { "update" } else { "put" },
            Some(&entry.id),
            Some(&entry.scope_id),
            &MemoryEventContext::default(),
            1,
            None,
        )
        .map_err(|e| self.persist_error(format!("memory lifecycle event failed: {e}")))?;
        tx.commit()
            .map_err(|e| self.persist_error(format!("memory commit failed: {e}")))?;
        drop(conn);
        self.flush_event_outbox()?;
        self.clear_last_persist_error();
        Ok(())
    }

    fn delete_entry(&self, id: &str, context: &MemoryEventContext) -> Result<(), String> {
        let mut conn = self.db.lock();
        let tx = conn
            .transaction()
            .map_err(|e| self.persist_error(format!("memory delete transaction failed: {e}")))?;
        let snapshot = tx
            .query_row(
                "SELECT scope_id, artifact_family, producer, record_type
                   FROM memories WHERE id = ?1",
                rusqlite::params![id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        MemoryRecordMetadata {
                            artifact_family: row.get(1)?,
                            producer: row.get(2)?,
                            record_type: row.get(3)?,
                        },
                    ))
                },
            )
            .optional()
            .map_err(|e| {
                self.persist_error(format!("memory delete lookup failed for {id}: {e}"))
            })?;
        tx.execute("DELETE FROM memories WHERE id = ?1", rusqlite::params![id])
            .map_err(|e| self.persist_error(format!("memory delete failed for {id}: {e}")))?;
        // Deletion ledger: the ONLY durable record that this id was removed on purpose.
        // Cross-machine sync reads it to tombstone the mirror instead of resurrecting the
        // row from the other machine's copy (curate/dashboard deletes were silently undone
        // by the next sync before this existed).
        tx.execute(
            "INSERT INTO deletions (id, deleted_at) VALUES (?1, ?2)
             ON CONFLICT(id) DO UPDATE SET deleted_at=excluded.deleted_at",
            rusqlite::params![id, crate::time::now_iso()],
        )
        .map_err(|e| {
            self.persist_error(format!("memory deletion tombstone failed for {id}: {e}"))
        })?;
        if let Some((scope_id, metadata)) = snapshot.as_ref() {
            log_memory_event(
                &self.operation_attribution,
                &tx,
                "delete",
                Some(id),
                Some(scope_id),
                context,
                1,
                Some(metadata),
            )
            .map_err(|e| self.persist_error(format!("memory delete event failed for {id}: {e}")))?;
        }
        tx.commit()
            .map_err(|e| self.persist_error(format!("memory delete commit failed: {e}")))?;
        drop(conn);
        self.flush_event_outbox()?;
        let _ = self
            .registry
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .remove(id);
        self.clear_last_persist_error();
        Ok(())
    }

    /// Run one restricted dream consolidation pass now.
    pub fn dream_now(&self, today: &str) -> Result<DreamStatus, String> {
        self.dream_now_observed(today, &MemoryEventContext::new("curate"))
    }

    pub fn dream_now_observed(
        &self,
        today: &str,
        context: &MemoryEventContext,
    ) -> Result<DreamStatus, String> {
        let policy = DreamAgentPolicy::restricted();
        let as_of_ms = crate::time::now_millis() as i64;
        let eligible_ids = self.recall_eligible_ids_at(as_of_ms, false);
        let entries = self
            .registry
            .read()
            .unwrap()
            .all()
            .into_iter()
            .filter(|entry| eligible_ids.contains(&entry.id))
            .cloned()
            .collect::<Vec<_>>();
        let expected_corpus = dream_registry_digest(&entries)?;
        *self.dream_status.lock().unwrap() = DreamStatus {
            agent_id: "dream-memory".into(),
            status: "running".into(),
            model: policy.model.clone(),
            shell_allowed: policy.shell_allowed,
            read_count: entries.len(),
            consolidated_count: 0,
            pruned_count: 0,
            quarantined_count: 0,
        };

        if policy.shell_allowed {
            return Err("dream agent policy unexpectedly allows shell".into());
        }
        let mut plan = consolidate_dream_memories(&entries, today);
        let prepared_embeddings = plan
            .consolidated
            .iter()
            .map(|consolidated| {
                let content = consolidated.content.trim();
                if consolidated.id.is_empty() || content.is_empty() {
                    return Err("dream consolidation produced an empty id or content".into());
                }
                self.embed_for_write(content)
            })
            .collect::<Result<Vec<_>, String>>()?;
        let mut consolidated_entries = Vec::with_capacity(plan.consolidated.len());
        let mut conn = self.db.lock();
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| self.persist_error(format!("dream transaction failed: {e}")))?;
        let current_eligible = recall_eligible_ids_on(&tx, as_of_ms, false).map_err(|error| {
            self.persist_error(format!("dream eligibility lookup failed: {error}"))
        })?;
        let current_corpus = dream_database_digest(&tx, &current_eligible)?;
        if current_corpus != expected_corpus {
            let error = "memory DB changed while dream consolidation was preparing writes";
            self.dream_status.lock().unwrap().status = format!("failed: {error}");
            return Err(error.into());
        }
        let expired_ids = expired_curation_ids_on(&tx, as_of_ms)
            .map_err(|error| self.persist_error(format!("dream expired lookup failed: {error}")))?;
        plan.quarantined_ids.extend(expired_ids.iter().cloned());
        plan.quarantined_ids.sort();
        plan.quarantined_ids.dedup();
        let mut never_exposed = Vec::with_capacity(plan.quarantined_ids.len());
        for id in &plan.quarantined_ids {
            if expired_ids.contains(id) {
                never_exposed.push(id.clone());
                continue;
            }
            let exposure = tx
                .query_row(
                    "SELECT m.inject_count,
                            EXISTS(
                                SELECT 1 FROM memory_event_log e
                                WHERE e.memory_id = m.id
                                  AND e.event_kind IN ('put', 'update', 'curate_consolidate')
                            )
                     FROM memories m WHERE m.id = ?1",
                    rusqlite::params![id],
                    |row| Ok((row.get::<_, i64>(0)?, row.get::<_, bool>(1)?)),
                )
                .optional()
                .map_err(|e| {
                    self.persist_error(format!("dream exposure lookup failed for {id}: {e}"))
                })?
                .unwrap_or((0, false));
            // A zero inject count is trustworthy only for a row whose lifecycle began after
            // durable exposure accounting existed. Legacy rows without that provenance are
            // protected: absence of an event is not evidence that no preview was delivered.
            if exposure.0 == 0 && exposure.1 {
                never_exposed.push(id.clone());
            }
        }
        plan.quarantined_ids = never_exposed;
        for (consolidated, embedding) in plan.consolidated.iter().zip(prepared_embeddings) {
            let content = consolidated.content.trim();
            let (access, created_at) = tx
                .query_row(
                    "SELECT access_count, created_at FROM memories WHERE id = ?1",
                    rusqlite::params![consolidated.id],
                    |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)),
                )
                .map_err(|e| {
                    self.persist_error(format!(
                        "dream counter load failed for {}: {e}",
                        consolidated.id
                    ))
                })?;
            let entry = MemoryEntry {
                id: consolidated.id.clone(),
                tier: MemoryTier::Semantic,
                embedding: Some(embedding),
                content: content.to_string(),
                keywords: consolidated.keywords.clone(),
                score: 0.9,
                created_at,
                access_count: access as u32,
                scope_id: consolidated.scope_id.clone(),
            };
            let updated_at = crate::time::now_iso();
            let metadata = MemoryRecordMetadata {
                artifact_family: "memory".to_string(),
                producer: "dream_memory".to_string(),
                record_type: "consolidated_memory".to_string(),
            };
            self.persist_entry_with_record_on(
                &tx,
                &entry,
                &updated_at,
                &consolidated.source_ids,
                &metadata,
            )?;
            log_memory_event(
                &self.operation_attribution,
                &tx,
                "curate_consolidate",
                Some(&entry.id),
                Some(&entry.scope_id),
                context,
                1,
                Some(&metadata),
            )
            .map_err(|e| {
                self.persist_error(format!(
                    "dream consolidate event failed for {}: {e}",
                    entry.id
                ))
            })?;
            consolidated_entries.push(entry);
        }
        for id in &plan.pruned_ids {
            let snapshot = tx
                .query_row(
                    "SELECT scope_id, artifact_family, producer, record_type
                       FROM memories WHERE id = ?1",
                    rusqlite::params![id],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            MemoryRecordMetadata {
                                artifact_family: row.get(1)?,
                                producer: row.get(2)?,
                                record_type: row.get(3)?,
                            },
                        ))
                    },
                )
                .optional()
                .map_err(|e| {
                    self.persist_error(format!("dream prune lookup failed for {id}: {e}"))
                })?;
            tx.execute("DELETE FROM memories WHERE id = ?1", rusqlite::params![id])
                .map_err(|e| self.persist_error(format!("dream prune failed for {id}: {e}")))?;
            tx.execute(
                "INSERT INTO deletions (id, deleted_at) VALUES (?1, ?2)
                 ON CONFLICT(id) DO UPDATE SET deleted_at=excluded.deleted_at",
                rusqlite::params![id, crate::time::now_iso()],
            )
            .map_err(|e| {
                self.persist_error(format!("dream prune tombstone failed for {id}: {e}"))
            })?;
            if let Some((scope_id, metadata)) = snapshot.as_ref() {
                log_memory_event(
                    &self.operation_attribution,
                    &tx,
                    "curate_prune",
                    Some(id),
                    Some(scope_id),
                    context,
                    1,
                    Some(metadata),
                )
                .map_err(|e| {
                    self.persist_error(format!("dream prune event failed for {id}: {e}"))
                })?;
            }
        }
        for id in &plan.quarantined_ids {
            let snapshot = tx
                .query_row(
                    "SELECT scope_id, artifact_family, producer, record_type
                       FROM memories WHERE id = ?1",
                    rusqlite::params![id],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            MemoryRecordMetadata {
                                artifact_family: row.get(1)?,
                                producer: row.get(2)?,
                                record_type: row.get(3)?,
                            },
                        ))
                    },
                )
                .optional()
                .map_err(|e| {
                    self.persist_error(format!("dream quarantine lookup failed for {id}: {e}"))
                })?;
            let quarantined_at = crate::time::now_iso();
            tx.execute(
                "INSERT INTO memory_quarantine
                    (id, tier, content, keywords, score, created_at, updated_at, access_count,
                     embedding, embedding_q, scope_id, inject_count, content_hash, embed_model,
                     source_ids, authority, influence_class, lifecycle_state, effective_from_ms,
                     effective_until_ms, expires_at_ms, review_after_ms, superseded_by,
                     priority_class, confidence, confidence_basis, quarantined_at, reason)
                 SELECT id, tier, content, keywords, score, created_at, updated_at, access_count,
                        embedding, embedding_q, scope_id, inject_count, content_hash, embed_model,
                        source_ids, authority, influence_class, lifecycle_state, effective_from_ms,
                        effective_until_ms, expires_at_ms, review_after_ms, superseded_by,
                        priority_class, confidence, confidence_basis, ?2, 'low_effectiveness'
                   FROM memories WHERE id = ?1
                 ON CONFLICT(id) DO UPDATE SET
                    tier=excluded.tier, content=excluded.content, keywords=excluded.keywords,
                    score=excluded.score, created_at=excluded.created_at,
                    updated_at=excluded.updated_at, access_count=excluded.access_count,
                    embedding=excluded.embedding, embedding_q=excluded.embedding_q,
                    scope_id=excluded.scope_id, inject_count=excluded.inject_count,
                    content_hash=excluded.content_hash, embed_model=excluded.embed_model,
                    source_ids=excluded.source_ids, authority=excluded.authority,
                    influence_class=excluded.influence_class, lifecycle_state=excluded.lifecycle_state,
                    effective_from_ms=excluded.effective_from_ms, effective_until_ms=excluded.effective_until_ms,
                    expires_at_ms=excluded.expires_at_ms, review_after_ms=excluded.review_after_ms,
                    superseded_by=excluded.superseded_by, priority_class=excluded.priority_class,
                    confidence=excluded.confidence, confidence_basis=excluded.confidence_basis,
                    quarantined_at=excluded.quarantined_at,
                    reason=excluded.reason",
                rusqlite::params![id, quarantined_at],
            )
            .map_err(|e| {
                self.persist_error(format!("dream quarantine persist failed for {id}: {e}"))
            })?;
            tx.execute("DELETE FROM memories WHERE id = ?1", rusqlite::params![id])
                .map_err(|e| {
                    self.persist_error(format!("dream quarantine removal failed for {id}: {e}"))
                })?;
            if let Some((scope_id, metadata)) = snapshot.as_ref() {
                log_memory_event(
                    &self.operation_attribution,
                    &tx,
                    "curate_quarantine",
                    Some(id),
                    Some(scope_id),
                    context,
                    1,
                    Some(metadata),
                )
                .map_err(|e| {
                    self.persist_error(format!("dream quarantine event failed for {id}: {e}"))
                })?;
            }
        }
        tx.commit()
            .map_err(|e| self.persist_error(format!("dream commit failed: {e}")))?;
        drop(conn);
        self.flush_event_outbox()?;
        {
            let mut registry = self.registry.write().unwrap_or_else(|e| e.into_inner());
            for id in &plan.pruned_ids {
                registry.remove(id);
            }
            for id in &plan.quarantined_ids {
                registry.remove(id);
            }
            for entry in consolidated_entries {
                registry.insert(entry);
            }
        }

        let status = DreamStatus {
            agent_id: "dream-memory".into(),
            status: "completed".into(),
            model: policy.model,
            shell_allowed: policy.shell_allowed,
            read_count: entries.len(),
            consolidated_count: plan.consolidated.len(),
            pruned_count: plan.pruned_ids.len(),
            quarantined_count: plan.quarantined_ids.len(),
        };
        *self.dream_status.lock().unwrap() = status.clone();
        Ok(status)
    }

    /// Latest DreamStatus for the agent roster surface.
    #[allow(dead_code)]
    pub fn dream_status(&self) -> DreamStatus {
        self.dream_status.lock().unwrap().clone()
    }

    /// IDs currently held outside active recall by reversible curation.
    pub fn quarantined_ids(&self) -> Vec<String> {
        let conn = self.db.lock();
        let mut ids = conn
            .prepare("SELECT id FROM memory_quarantine ORDER BY id")
            .and_then(|mut statement| {
                statement
                    .query_map([], |row| row.get::<_, String>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .unwrap_or_default();
        ids.sort();
        ids
    }

    /// Restore one quarantined row exactly as it was, without creating or retaining a tombstone.
    pub fn restore_quarantined(&self, id: &str) -> Result<bool, String> {
        let lock = MtimeLock::acquire(self.db_path.clone());
        lock.verify()?;
        let restored_metadata = self
            .db
            .lock_events()
            .query_row(
                "SELECT artifact_family, producer FROM context_event_log
                  WHERE artifact_id=?1 ORDER BY seq DESC LIMIT 1",
                rusqlite::params![stable_artifact_id(id)],
                |row| {
                    Ok(MemoryRecordMetadata {
                        artifact_family: row.get(0)?,
                        producer: row.get(1)?,
                        record_type: "memory".to_string(),
                    })
                },
            )
            .optional()
            .map_err(|error| {
                self.persist_error(format!(
                    "quarantine restore metadata lookup failed: {error}"
                ))
            })?
            .unwrap_or_default();
        let mut conn = self.db.lock();
        let tx = conn.transaction().map_err(|e| {
            self.persist_error(format!("quarantine restore transaction failed: {e}"))
        })?;
        let restored = tx
            .execute(
                "INSERT INTO memories
                    (id, tier, content, keywords, score, created_at, updated_at, access_count,
                     embedding, embedding_q, scope_id, inject_count, content_hash, embed_model,
                     source_ids, authority, influence_class, lifecycle_state, effective_from_ms,
                     effective_until_ms, expires_at_ms, review_after_ms, superseded_by,
                     priority_class, confidence, confidence_basis)
                 SELECT id, tier, content, keywords, score, created_at, updated_at, access_count,
                        embedding, embedding_q, scope_id, inject_count, content_hash, embed_model,
                        source_ids, authority, influence_class, lifecycle_state, effective_from_ms,
                        effective_until_ms, expires_at_ms, review_after_ms, superseded_by,
                        priority_class, confidence, confidence_basis
                   FROM memory_quarantine WHERE id = ?1
                 ON CONFLICT(id) DO NOTHING",
                rusqlite::params![id],
            )
            .map_err(|e| self.persist_error(format!("quarantine restore failed for {id}: {e}")))?;
        if restored == 0 {
            return Ok(false);
        }
        tx.execute(
            "UPDATE memories SET artifact_family=?2, producer=?3, record_type=?4 WHERE id=?1",
            rusqlite::params![
                id,
                restored_metadata.artifact_family,
                restored_metadata.producer,
                restored_metadata.record_type,
            ],
        )
        .map_err(|error| {
            self.persist_error(format!(
                "quarantine restore metadata failed for {id}: {error}"
            ))
        })?;
        tx.execute(
            "DELETE FROM memory_quarantine WHERE id = ?1",
            rusqlite::params![id],
        )
        .map_err(|e| self.persist_error(format!("quarantine cleanup failed for {id}: {e}")))?;
        tx.execute("DELETE FROM deletions WHERE id = ?1", rusqlite::params![id])
            .map_err(|e| {
                self.persist_error(format!("quarantine tombstone cleanup failed for {id}: {e}"))
            })?;
        let context = MemoryEventContext::new("quarantine_restore");
        let scope_id = tx
            .query_row(
                "SELECT scope_id FROM memories WHERE id = ?1",
                rusqlite::params![id],
                |row| row.get::<_, String>(0),
            )
            .map_err(|e| {
                self.persist_error(format!("restored scope lookup failed for {id}: {e}"))
            })?;
        log_memory_event(
            &self.operation_attribution,
            &tx,
            "quarantine_restore",
            Some(id),
            Some(&scope_id),
            &context,
            1,
            Some(&restored_metadata),
        )
        .map_err(|e| {
            self.persist_error(format!("quarantine restore event failed for {id}: {e}"))
        })?;
        tx.commit()
            .map_err(|e| self.persist_error(format!("quarantine restore commit failed: {e}")))?;
        drop(conn);
        self.flush_event_outbox()?;

        let restored_entry = {
            let conn = self.db.lock();
            let row = conn
                .query_row(
                    "SELECT id, tier, content, keywords, score, created_at, access_count,
                            embedding, embedding_q, scope_id
                       FROM memories WHERE id = ?1",
                    rusqlite::params![id],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, f64>(4)?,
                            row.get::<_, String>(5)?,
                            row.get::<_, i64>(6)?,
                            row.get::<_, Option<Vec<u8>>>(7)?,
                            row.get::<_, Option<Vec<u8>>>(8)?,
                            row.get::<_, String>(9)?,
                        ))
                    },
                )
                .optional()
                .map_err(|e| format!("restored memory {id} reload failed: {e}"))?
                .ok_or_else(|| format!("restored memory {id} disappeared after commit"))?;
            let (
                id,
                tier,
                content,
                keywords,
                score,
                created_at,
                access_count,
                embedding,
                embedding_q,
                scope_id,
            ) = row;
            if access_count < 0 {
                return Err(format!("restored memory {id} has negative access_count"));
            }
            let embedding = match embedding_q {
                Some(blob) => Some(quantized_blob_to_embedding(&blob).ok_or_else(|| {
                    format!("restored memory {id} has invalid quantized embedding")
                })?),
                None => match embedding {
                    Some(blob) if blob.len() % 4 != 0 => {
                        return Err(format!(
                            "restored memory {id} has invalid f32 embedding blob"
                        ));
                    }
                    Some(blob) => Some(blob_to_embedding(&blob)),
                    None => None,
                },
            };
            MemoryEntry {
                id: id.clone(),
                tier: serde_json::from_str(&tier)
                    .map_err(|e| format!("restored memory {id} has invalid tier: {e}"))?,
                content,
                keywords: serde_json::from_str(&keywords)
                    .map_err(|e| format!("restored memory {id} has invalid keywords: {e}"))?,
                score,
                created_at,
                access_count: access_count as u32,
                embedding,
                scope_id,
            }
        };
        self.registry
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert(restored_entry);
        self.clear_last_persist_error();
        Ok(true)
    }

    /// Record the experience of a finished run.
    pub fn record_run(&self, goal: &str, succeeded: bool, summary: &str) {
        let outcome = if succeeded {
            "succeeded"
        } else {
            "did not complete"
        };
        let content = format!("Goal \"{goal}\" {outcome}. {summary}");
        let entry = MemoryEntry {
            id: format!(
                "mem-{}-{}",
                crate::time::now_millis(),
                MEM_SEQ.fetch_add(1, Ordering::Relaxed)
            ),
            tier: MemoryTier::Episodic,
            embedding: Some(self.embedder.embed(&content)),
            content,
            keywords: keywords_of(goal),
            // Successful runs are more worth remembering than failed ones.
            score: if succeeded { 1.0 } else { 0.3 },
            created_at: crate::time::now_iso(),
            access_count: 0,
            // P2 will thread the active project; default 'global' until then.
            scope_id: crypt_core::default_scope(),
        };
        if self.persist_entry(&entry).is_ok() {
            self.registry
                .write()
                .unwrap_or_else(|e| e.into_inner())
                .insert(entry);
        }

        let injected = std::mem::take(&mut *self.pending_injections.lock().unwrap());
        if !injected.is_empty() {
            if let Err(error) = self.record_outcomes(&injected, succeeded) {
                eprintln!("[memory] task outcome event failed: {error}");
            }
            let mut history = self.effectiveness_history.lock().unwrap();
            for entry_id in injected {
                history.push(MemoryUsageRecord::legacy(entry_id, true, succeeded));
            }
        }

        if let Some((features, tier)) = self.last_route.lock().unwrap().take() {
            let cost = match tier {
                RetrievalTier::Low => 1.0,
                RetrievalTier::Mid => 2.0,
                RetrievalTier::High => 5.0,
            };
            self.routing_policy
                .lock()
                .unwrap()
                .reward(&features, tier, succeeded, cost);
        }
    }

    fn record_outcomes(&self, ids: &[String], succeeded: bool) -> Result<(), String> {
        let mut conn = self.db.lock();
        let tx = conn
            .transaction()
            .map_err(|e| self.persist_error(format!("outcome transaction failed: {e}")))?;
        let context = MemoryEventContext::default();
        for id in ids {
            let scope_id = tx
                .query_row(
                    "SELECT scope_id FROM memories WHERE id = ?1",
                    rusqlite::params![id],
                    |row| row.get::<_, String>(0),
                )
                .optional()
                .map_err(|e| self.persist_error(format!("outcome scope load failed: {e}")))?;
            if let Some(scope_id) = scope_id.as_deref() {
                log_memory_event(
                    &self.operation_attribution,
                    &tx,
                    if succeeded {
                        "outcome_success"
                    } else {
                        "outcome_failure"
                    },
                    Some(id),
                    Some(scope_id),
                    &context,
                    1,
                    None,
                )
                .map_err(|e| self.persist_error(format!("outcome event failed: {e}")))?;
            }
        }
        tx.commit()
            .map_err(|e| self.persist_error(format!("outcome commit failed: {e}")))?;
        drop(conn);
        self.flush_event_outbox()?;
        self.clear_last_persist_error();
        Ok(())
    }

    /// Record one per-candidate feedback row (the Membrane feedback rail). Fail-closed: the
    /// record must validate (a cited-verdict row needs a resolvable `verdict_ref`) before it is
    /// persisted. Idempotent by `(trace_id, candidate_id)` — a re-run at compaction upserts, never
    /// double-counts. Rows remain evidence-only until a preregistered controlled experiment is
    /// qualified and applied; caller-selected feedback never changes ranking directly.
    pub fn record_feedback(&self, rec: &FeedbackRecord) -> Result<(), String> {
        self.record_feedback_observed(rec, &MemoryEventContext::default())
    }

    fn record_feedback_observed(
        &self,
        rec: &FeedbackRecord,
        context: &MemoryEventContext,
    ) -> Result<(), String> {
        rec.validate()?;
        let canonical_trace = opaque_correlation_token(&rec.trace_id, "trace");
        let legacy_artifact_id = stable_artifact_id(&rec.candidate_id);
        let full_digest = content_hash(&rec.candidate_id);
        let memory_artifact_id = format!("memory.{full_digest}");
        let provider_artifact_id = format!("artifact.{full_digest}");
        let feedback_target = self
            .db
            .lock_events()
            .query_row(
                "SELECT event_id, artifact_id, artifact_family, producer
                   FROM context_event_log
                  WHERE trace_id=?1 AND artifact_id IN (?2, ?3, ?4)
                    AND phase IN ('block.delivered','candidate.admitted','candidate.retrieved')
                  ORDER BY CASE phase
                    WHEN 'block.delivered' THEN 0
                    WHEN 'candidate.admitted' THEN 1
                    ELSE 2 END, seq DESC LIMIT 1",
                rusqlite::params![
                    canonical_trace,
                    legacy_artifact_id,
                    memory_artifact_id,
                    provider_artifact_id
                ],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                },
            )
            .optional()
            .map_err(|error| self.persist_error(format!("feedback link lookup failed: {error}")))?;
        let artifact_id = feedback_target
            .as_ref()
            .map(|target| target.1.clone())
            .unwrap_or(legacy_artifact_id);
        let mut conn = self.db.lock();
        let tx = conn
            .transaction()
            .map_err(|error| self.persist_error(format!("feedback transaction failed: {error}")))?;
        let ts = crate::time::now_iso();
        tx.execute(
            "INSERT INTO context_feedback
                 (trace_id, candidate_id, content_sha256, outcome, verified, verdict_ref, scope_id, ts)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(trace_id, candidate_id) DO UPDATE SET
                 content_sha256=excluded.content_sha256, outcome=excluded.outcome,
                 verified=excluded.verified, verdict_ref=excluded.verdict_ref,
                 scope_id=excluded.scope_id, ts=excluded.ts",
            rusqlite::params![
                canonical_trace,
                rec.candidate_id,
                rec.content_sha256,
                outcome_str(rec.outcome),
                rec.verified() as i64,
                rec.verdict_ref,
                rec.scope_id,
                ts,
            ],
        )
        .map_err(|e| self.persist_error(format!("feedback upsert failed: {e}")))?;
        let metadata = if let Some(target) = feedback_target.as_ref() {
            MemoryRecordMetadata {
                artifact_family: target.2.clone(),
                producer: target.3.clone(),
                record_type: "memory".to_string(),
            }
        } else {
            metadata_for_on(&tx, &rec.candidate_id).map_err(|error| {
                self.persist_error(format!("feedback metadata load failed: {error}"))
            })?
        };
        let event_id = format!("feedback.{}", new_uuid_v4()?);
        let event = ContextEvent {
            schema_version: 1,
            event_id: event_id.clone(),
            ts,
            installation_id: self.operation_attribution.installation_id.clone(),
            service_instance_id: self.operation_attribution.service_instance_id.clone(),
            workspace_id: self.operation_attribution.workspace_id.clone(),
            client: registry_token(&context.surface, "unknown"),
            client_version: None,
            producer: registry_token(&metadata.producer, "manual"),
            producer_version: None,
            session_id: normalized_session(context),
            turn_id: None,
            trace_id: canonical_trace.clone(),
            span_id: opaque_correlation_token(&event_id, "span"),
            parent_span_id: None,
            artifact_family: registry_token(&metadata.artifact_family, "memory"),
            provider: "crypt".to_string(),
            provider_version: Some(env!("CARGO_PKG_VERSION").to_string()),
            release_generation: Some(crate::release_identity::release_generation()),
            phase: "feedback.recorded".to_string(),
            operation: "evaluate".to_string(),
            status: "success".to_string(),
            reason_code: Some(outcome_str(rec.outcome).to_string()),
            scope_id: Some(opaque_token(&rec.scope_id, "scope")),
            artifact_id: Some(artifact_id.clone()),
            artifact_sha256: (rec.content_sha256.len() == 64
                && rec
                    .content_sha256
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit()))
            .then(|| rec.content_sha256.to_ascii_lowercase()),
            traffic_class: "production".to_string(),
            policy_version: None,
            policy_activation_sha256: None,
            cohort: None,
            task_class: None,
            source_generation: None,
            duration_ms: None,
            quantity: Some(1),
            token_count: None,
            char_count: None,
            meta: None,
            measurements: vec![ContextEventMeasurement {
                name: "verified".to_string(),
                value: MeasurementValue::Boolean(rec.verified()),
                unit: "boolean".to_string(),
            }],
            links: feedback_target
                .as_ref()
                .map(|target| ContextEventLink {
                    relation: "feedback_for".to_string(),
                    target_event_id: target.0.clone(),
                })
                .into_iter()
                .collect(),
        };
        let mut events = vec![event];
        if rec.verified() {
            let value_event_id = format!("value.{}", new_uuid_v4()?);
            let (phase, status, reason_code) = match rec.outcome {
                Outcome::Used => ("candidate.used", "success", None),
                Outcome::Ignored => ("candidate.ignored", "success", None),
                Outcome::Contradicted => (
                    "candidate.contradicted",
                    "contradicted",
                    Some("verified_contradiction".to_string()),
                ),
            };
            events.push(ContextEvent {
                schema_version: 1,
                event_id: value_event_id.clone(),
                ts: crate::time::now_iso(),
                installation_id: self.operation_attribution.installation_id.clone(),
                service_instance_id: self.operation_attribution.service_instance_id.clone(),
                workspace_id: self.operation_attribution.workspace_id.clone(),
                client: registry_token(&context.surface, "unknown"),
                client_version: None,
                producer: registry_token(&metadata.producer, "manual"),
                producer_version: None,
                session_id: normalized_session(context),
                turn_id: None,
                trace_id: canonical_trace.clone(),
                span_id: opaque_correlation_token(&value_event_id, "span"),
                parent_span_id: Some(opaque_correlation_token(&event_id, "span")),
                artifact_family: registry_token(&metadata.artifact_family, "memory"),
                provider: "crypt".to_string(),
                provider_version: Some(env!("CARGO_PKG_VERSION").to_string()),
                release_generation: Some(crate::release_identity::release_generation()),
                phase: phase.to_string(),
                operation: "evaluate".to_string(),
                status: status.to_string(),
                reason_code,
                scope_id: Some(opaque_token(&rec.scope_id, "scope")),
                artifact_id: Some(artifact_id.clone()),
                artifact_sha256: (rec.content_sha256.len() == 64
                    && rec
                        .content_sha256
                        .bytes()
                        .all(|byte| byte.is_ascii_hexdigit()))
                .then(|| rec.content_sha256.to_ascii_lowercase()),
                traffic_class: "production".to_string(),
                policy_version: None,
                policy_activation_sha256: None,
                cohort: None,
                task_class: None,
                source_generation: None,
                duration_ms: None,
                quantity: Some(1),
                token_count: None,
                char_count: None,
                meta: None,
                measurements: Vec::new(),
                links: feedback_target
                    .as_ref()
                    .map(|target| ContextEventLink {
                        relation: "outcome_for".to_string(),
                        target_event_id: target.0.clone(),
                    })
                    .into_iter()
                    .collect(),
            });
        }
        append_context_events_on(&tx, &ContextEventBatch { events })
            .map_err(|error| self.persist_error(format!("feedback telemetry failed: {error}")))?;
        tx.commit()
            .map_err(|error| self.persist_error(format!("feedback commit failed: {error}")))?;
        drop(conn);
        self.flush_event_outbox()?;
        self.clear_last_persist_error();
        Ok(())
    }

    fn apply_lifecycle_input_on(
        &self,
        tx: &rusqlite::Transaction<'_>,
        replacement: &MemoryEntry,
        lifecycle: &MemoryLifecycleInputV1,
        context: &MemoryEventContext,
    ) -> Result<(), String> {
        let Some(subject_id) = lifecycle.supersedes.as_deref() else {
            return Ok(());
        };
        if subject_id == replacement.id {
            return Err("lifecycle supersedes cannot reference self".into());
        }
        let subject: Option<(String, String, Option<String>)> = tx
            .query_row(
                "SELECT scope_id, lifecycle_state, superseded_by FROM memories WHERE id=?1",
                rusqlite::params![subject_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
            .map_err(|error| error.to_string())?;
        let Some((scope, state, successor)) = subject else {
            return Err("lifecycle supersedes target is missing".into());
        };
        if scope != replacement.scope_id {
            return Err("lifecycle supersedes must remain in one scope".into());
        }
        let effective_at = lifecycle
            .effective_from_ms
            .unwrap_or_else(|| crate::time::now_millis() as i64);
        let event_id = format!(
            "lifecycle-put-{}",
            &content_hash(&format!("{subject_id}:{}:{effective_at}", replacement.id))[..32]
        );
        let event = MemoryLifecycleEventV1::superseded(
            event_id.clone(),
            subject_id,
            &replacement.id,
            &replacement.scope_id,
            effective_at,
            &context.surface,
            "A1",
            "lifecycle_input",
            "lifecycle_input",
        );
        let meta = serde_json::to_string(&event).map_err(|error| error.to_string())?;
        if state == "superseded" && successor.as_deref() == Some(replacement.id.as_str()) {
            let existing: Option<String> = tx
                .query_row(
                    "SELECT meta FROM memory_event_log WHERE event_uid=?1",
                    rusqlite::params![event_id],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|error| error.to_string())?;
            return if existing.as_deref() == Some(meta.as_str()) {
                Ok(())
            } else {
                Err("lifecycle supersession conflicts with existing event".into())
            };
        }
        if state != "active" || successor.is_some() {
            return Err("lifecycle supersedes subject is not active".into());
        }
        let mut cursor = replacement.id.clone();
        let mut seen = HashSet::new();
        loop {
            if !seen.insert(cursor.clone()) || cursor == subject_id {
                return Err("lifecycle supersedes cycle".into());
            }
            let next: Option<Option<String>> = tx
                .query_row(
                    "SELECT superseded_by FROM memories WHERE id=?1",
                    rusqlite::params![cursor],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|error| error.to_string())?;
            match next {
                Some(Some(next)) => cursor = next,
                Some(None) => break,
                None => return Err("lifecycle replacement is missing".into()),
            }
        }
        let updated = tx.execute(
            "UPDATE memories SET lifecycle_state='superseded', superseded_by=?1, effective_until_ms=?2 WHERE id=?3 AND lifecycle_state='active' AND superseded_by IS NULL",
            rusqlite::params![replacement.id, effective_at, subject_id],
        ).map_err(|error| error.to_string())?;
        if updated != 1 {
            return Err("lifecycle supersedes subject is not active".into());
        }
        tx.execute(
            "INSERT INTO memory_event_log (ts, event_kind, memory_id, surface, scope_id, quantity, meta, event_uid, installation_id, client, artifact_id, identity_status) VALUES (?1, 'superseded', ?2, ?3, ?4, 1, ?5, ?6, ?7, ?8, ?9, 'observed')",
            rusqlite::params![crate::time::now_iso(), subject_id, context.surface, replacement.scope_id, meta, event_id, self.operation_attribution.installation_id, context.surface, stable_artifact_id(subject_id)],
        ).map_err(|error| error.to_string())?;
        Ok(())
    }

    /// Close a bounded, completed observation window with an explicit provisional value state.
    ///
    /// A delivery is never treated as useful merely because it was read. After the caller's
    /// bounded observation window has elapsed, this emits one `candidate.unknown` only for a
    /// delivery with no existing `outcome_for` value terminal. The original delivery identity is
    /// preserved so reconciliation can join the terminal without manual telemetry fabrication.
    pub fn close_unresolved_deliveries(
        &self,
        observed_since: &str,
        observed_through: &str,
        max_deliveries: usize,
    ) -> Result<usize, String> {
        let valid_cutoff = |value: &str| {
            value.len() >= 20
                && value.len() <= 40
                && value.ends_with('Z')
                && value.contains('T')
                && value.bytes().all(|byte| {
                    byte.is_ascii_digit() || matches!(byte, b'-' | b':' | b'.' | b'T' | b'Z')
                })
        };
        if !valid_cutoff(observed_since) || !valid_cutoff(observed_through) {
            return Err("closure requires RFC3339 UTC observation bounds".to_string());
        }
        if observed_since >= observed_through {
            return Err("closure observation bounds must be increasing".to_string());
        }
        if !(1..=10_000).contains(&max_deliveries) {
            return Err("closure max_deliveries must be within 1..=10000".to_string());
        }

        let mut conn = self.db.lock_events();
        let tx = conn
            .transaction()
            .map_err(|e| self.persist_error(format!("unknown closure transaction failed: {e}")))?;
        let mut statement = tx
            .prepare(
                "SELECT d.event_id,d.installation_id,d.service_instance_id,d.workspace_id,
                        d.client,d.client_version,d.producer,d.producer_version,d.session_id,
                        d.turn_id,d.trace_id,d.span_id,d.artifact_family,d.provider,d.provider_version,
                        d.release_generation,d.scope_id,d.artifact_id,d.artifact_sha256,
                        d.traffic_class,d.policy_version,d.policy_activation_sha256,d.cohort,d.task_class,
                        d.source_generation
                   FROM context_event_log d
                  WHERE d.phase='block.delivered' AND d.status='success'
                    AND d.traffic_class='production'
                    AND d.ts>=?1 AND d.ts<=?2
                    AND NOT EXISTS (
                        SELECT 1 FROM context_event_link link
                         WHERE link.target_event_id=d.event_id
                           AND link.relation='outcome_for'
                    )
                  ORDER BY d.seq ASC LIMIT ?",
            )
            .map_err(|e| self.persist_error(format!("unknown closure query prepare failed: {e}")))?;
        let deliveries = statement
            .query_map(
                rusqlite::params![
                    observed_since,
                    observed_through,
                    (max_deliveries + 1) as i64
                ],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, Option<String>>(5)?,
                        row.get::<_, String>(6)?,
                        row.get::<_, Option<String>>(7)?,
                        row.get::<_, String>(8)?,
                        row.get::<_, Option<String>>(9)?,
                        row.get::<_, String>(10)?,
                        row.get::<_, String>(11)?,
                        row.get::<_, String>(12)?,
                        row.get::<_, String>(13)?,
                        row.get::<_, Option<String>>(14)?,
                        row.get::<_, Option<String>>(15)?,
                        row.get::<_, Option<String>>(16)?,
                        row.get::<_, Option<String>>(17)?,
                        row.get::<_, Option<String>>(18)?,
                        row.get::<_, String>(19)?,
                        row.get::<_, Option<String>>(20)?,
                        row.get::<_, Option<String>>(21)?,
                        row.get::<_, Option<String>>(22)?,
                        row.get::<_, Option<String>>(23)?,
                        row.get::<_, Option<i64>>(24)?,
                    ))
                },
            )
            .map_err(|e| self.persist_error(format!("unknown closure query failed: {e}")))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| self.persist_error(format!("unknown closure row read failed: {e}")))?;
        drop(statement);
        if deliveries.len() > max_deliveries {
            return Err(format!(
                "closure delivery cap {max_deliveries} reached; rerun with a larger explicit bound"
            ));
        }
        let mut events = Vec::with_capacity(deliveries.len());
        for (
            delivery_id,
            installation_id,
            service_instance_id,
            workspace_id,
            client,
            client_version,
            producer,
            producer_version,
            session_id,
            turn_id,
            trace_id,
            delivery_span_id,
            artifact_family,
            provider,
            provider_version,
            release_generation,
            scope_id,
            artifact_id,
            artifact_sha256,
            traffic_class,
            policy_version,
            policy_activation_sha256,
            cohort,
            task_class,
            source_generation,
        ) in deliveries
        {
            let event_id = format!("value.{}", new_uuid_v4()?);
            events.push(ContextEvent {
                schema_version: 1,
                event_id: event_id.clone(),
                ts: crate::time::now_iso(),
                installation_id,
                service_instance_id,
                workspace_id,
                client,
                client_version,
                producer,
                producer_version,
                session_id,
                turn_id,
                trace_id,
                span_id: opaque_correlation_token(&event_id, "span"),
                parent_span_id: Some(delivery_span_id),
                artifact_family,
                provider,
                provider_version,
                release_generation,
                phase: "candidate.unknown".to_string(),
                operation: "evaluate".to_string(),
                status: "unknown".to_string(),
                reason_code: Some("observation_window_elapsed".to_string()),
                scope_id,
                artifact_id,
                artifact_sha256,
                traffic_class,
                policy_version,
                policy_activation_sha256,
                cohort,
                task_class,
                source_generation,
                duration_ms: None,
                quantity: Some(1),
                token_count: None,
                char_count: None,
                meta: None,
                measurements: Vec::new(),
                links: vec![ContextEventLink {
                    relation: "outcome_for".to_string(),
                    target_event_id: delivery_id,
                }],
            });
        }
        if events.is_empty() {
            tx.commit().map_err(|e| {
                self.persist_error(format!("unknown closure empty commit failed: {e}"))
            })?;
            self.clear_last_persist_error();
            return Ok(0);
        }
        append_context_events_on(
            &tx,
            &ContextEventBatch {
                events: events.clone(),
            },
        )
        .map_err(|error| {
            self.persist_error(format!("unknown closure telemetry failed: {error}"))
        })?;
        tx.commit()
            .map_err(|e| self.persist_error(format!("unknown closure commit failed: {e}")))?;
        self.clear_last_persist_error();
        Ok(events.len())
    }

    /// Map a `(candidate_id, outcome, verified)` DB triple into a gate record. Unverified rows
    /// carry `verified: false`, so the gate skips them from ranking automatically.
    fn feedback_row_to_record(
        candidate_id: String,
        outcome: &str,
        verified: bool,
    ) -> Option<MemoryUsageRecord> {
        let outcome = parse_outcome(outcome).ok()?;
        Some(MemoryUsageRecord {
            entry_id: candidate_id,
            injected: true,
            task_succeeded: matches!(outcome, Outcome::Used),
            outcome: Some(outcome),
            verified,
        })
    }

    /// Load ALL persisted per-candidate feedback (metrics/tests). For the recall hot path use the
    /// bounded [`Self::feedback_rows_for`] instead — a full-table scan every recall does not scale.
    pub fn feedback_rows_from_db(&self) -> Vec<MemoryUsageRecord> {
        let conn = self.db.lock();
        let mut stmt =
            match conn.prepare("SELECT candidate_id, outcome, verified FROM context_feedback") {
                Ok(s) => s,
                Err(_) => return Vec::new(),
            };
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)? != 0,
            ))
        });
        rows.into_iter()
            .flatten()
            .flatten()
            .filter_map(|(id, outcome, verified)| {
                Self::feedback_row_to_record(id, &outcome, verified)
            })
            .collect()
    }

    /// Ingest every git-tracked skill body into the engine `skills` table — the portability store.
    /// Git stays the AUTHORING source (commit → reingest, like memories); the DB row is what
    /// travels cross-machine with the engine, so a session without the skills directory can still
    /// load skills. Returns (ingested, skipped, pruned). Tracked-only (git ls-files) — untracked
    /// drafts never enter the store.
    ///
    /// Reconciles rather than only adding: a skill deleted or folded in git is removed here too.
    /// Without this the row outlives its source forever and the skills provider keeps advertising
    /// a skill whose body, pipeline, and vocabulary are all gone (observed with `adapt`, which
    /// survived its own deletion and kept pointing at a removed pipeline and the retired name
    /// "Crypt"). Pruning is skipped entirely when the workspace yielded no skills, so a machine
    /// checked out without `tools/skills/` cannot wipe the portability store it was meant to carry.
    pub fn ingest_skills(&self, workspace: &Path) -> (usize, usize, usize) {
        let skills_dir = workspace.join("tools").join("skills");
        let tracked = std::process::Command::new("git")
            .args([
                "-C",
                &workspace.to_string_lossy(),
                "ls-files",
                "--",
                "tools/skills/",
            ])
            .output();
        let Ok(out) = tracked else { return (0, 0, 0) };
        let listing = String::from_utf8_lossy(&out.stdout);
        let mut ingested = 0usize;
        let mut skipped = 0usize;
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        let conn = self.db.lock();
        for rel in listing.lines() {
            let rel = rel.trim().replace('\\', "/");
            // Only the skill entrypoint: tools/skills/<name>/SKILL.md
            let Some(rest) = rel.strip_prefix("tools/skills/") else {
                continue;
            };
            let mut parts = rest.splitn(2, '/');
            let (Some(name), Some("SKILL.md")) = (parts.next(), parts.next()) else {
                continue;
            };
            let Ok(body) = std::fs::read_to_string(skills_dir.join(name).join("SKILL.md")) else {
                skipped += 1;
                continue;
            };
            let sha = content_hash(&body);
            let desc = skill_frontmatter_description(&body);
            let ok = conn
                .execute(
                    "INSERT INTO skills (name, description, body, body_sha256, resources, updated_at)
                     VALUES (?1, ?2, ?3, ?4, '[]', ?5)
                     ON CONFLICT(name) DO UPDATE SET description=excluded.description,
                         body=excluded.body, body_sha256=excluded.body_sha256,
                         updated_at=excluded.updated_at",
                    rusqlite::params![name, desc, body, sha, crate::time::now_iso()],
                )
                .is_ok();
            if ok {
                ingested += 1;
                seen.insert(name.to_string());
            } else {
                skipped += 1
            }
        }
        // Only reconcile against a workspace that actually produced skills. An empty result means
        // "no skills tree here", never "every skill was deleted".
        let mut pruned = 0usize;
        if ingested > 0 {
            let existing: Vec<String> = conn
                .prepare("SELECT name FROM skills")
                .and_then(|mut stmt| {
                    stmt.query_map([], |row| row.get::<_, String>(0))
                        .map(|rows| rows.filter_map(Result::ok).collect())
                })
                .unwrap_or_default();
            for name in existing {
                if !seen.contains(&name)
                    && conn
                        .execute("DELETE FROM skills WHERE name=?1", rusqlite::params![name])
                        .is_ok()
                {
                    pruned += 1;
                }
            }
        }
        (ingested, skipped, pruned)
    }

    /// Engine-served skill body: (body, body_sha256) from the `skills` table, or None.
    pub fn skill_from_db(&self, name: &str) -> Option<(String, String)> {
        self.db
            .lock()
            .query_row(
                "SELECT body, body_sha256 FROM skills WHERE name = ?1",
                rusqlite::params![name],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
            )
            .optional()
            .ok()
            .flatten()
    }

    /// Stable, content-only generation seal for the engine-owned skills snapshot.
    /// Timestamps are deliberately excluded so a same-content reingest preserves the epoch.
    pub fn skills_snapshot(&self) -> Result<SkillsSnapshot, String> {
        use sha2::{Digest, Sha256};

        let conn = self.db.lock();
        let mut statement = conn
            .prepare("SELECT name, description, body_sha256, resources FROM skills ORDER BY name")
            .map_err(|error| format!("prepare skills snapshot: {error}"))?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })
            .map_err(|error| format!("read skills snapshot: {error}"))?;
        let mut hasher = Sha256::new();
        let mut skills = Vec::new();
        for row in rows {
            let (name, description, body_sha256, resources) =
                row.map_err(|error| format!("read skills snapshot row: {error}"))?;
            for field in [&name, &body_sha256, &resources] {
                hasher.update((field.len() as u64).to_be_bytes());
                hasher.update(field.as_bytes());
            }
            skills.push(SkillIndexEntry {
                name,
                description,
                body_hash: body_sha256,
            });
        }
        Ok(SkillsSnapshot {
            schema_version: 1,
            generation: format!("sha256:{:x}", hasher.finalize()),
            skills,
        })
    }

    pub fn skills_generation(&self) -> Result<String, String> {
        self.skills_snapshot().map(|snapshot| snapshot.generation)
    }

    /// Content sha256 for a memory id WITHOUT recording a use (a pure peek). Backs the delivery
    /// provenance seal: verify a delivered block's `sourceHash` matches a real, current DB row
    /// before its text reaches the model. Uses the warm registry — no `get`, no use-count churn.
    pub fn content_sha256(&self, id: &str) -> Option<String> {
        let reg = self.registry.read().unwrap_or_else(|e| e.into_inner());
        reg.get(id).map(|e| content_hash(&e.content))
    }

    /// Best-effort inline capture of an OBSERVED per-candidate action as VERIFIED feedback — the
    /// non-dark auto-caller. `get` → `Used`; delete/supersede → `Contradicted`. Skips silently when
    /// there is no `trace_id` to key on (a bare CLI action has no recall trace). Never fails the
    /// caller: a feedback write must not break a get or a delete.
    fn record_observed_feedback(
        &self,
        id: &str,
        content: &str,
        outcome: Outcome,
        scope_id: &str,
        context: &MemoryEventContext,
    ) {
        let Some(trace_id) = context
            .trace_id
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
        else {
            return;
        };
        let rec = FeedbackRecord {
            trace_id: trace_id.to_string(),
            candidate_id: id.to_string(),
            content_sha256: content_hash(content),
            outcome,
            source: FeedbackSource::ObservedAction,
            verdict_ref: None,
            scope_id: scope_id.to_string(),
        };
        if let Err(e) = self.record_feedback_observed(&rec, context) {
            eprintln!("[memory] observed feedback capture failed for {id}: {e}");
        }
    }

    /// Gate history for a recall: durable per-candidate feedback for exactly these candidates, with
    /// STALE vetoes dropped. A verified `contradicted` vetoes only while its `content_sha256` still
    /// matches the candidate's CURRENT content — a superseding edit (new bytes) clears the veto, so
    /// "veto-until-superseded" holds even for same-id updates. Bounded to the candidate slate.
    pub fn gate_history_for(&self, candidates: &[&MemoryEntry]) -> Vec<MemoryUsageRecord> {
        if candidates.is_empty() {
            return Vec::new();
        }
        let current_sha: std::collections::HashMap<&str, String> = candidates
            .iter()
            .map(|c| (c.id.as_str(), content_hash(&c.content)))
            .collect();
        let ids: Vec<&str> = candidates.iter().map(|c| c.id.as_str()).collect();
        let conn = self.db.lock();
        let placeholders = std::iter::repeat_n("?", ids.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT candidate_id, outcome, verified, content_sha256 FROM context_feedback \
             WHERE qualified_experiment_id IS NOT NULL AND EXISTS (\
                 SELECT 1 FROM learning_promotion_receipt r \
                  WHERE r.experiment_id = context_feedback.qualified_experiment_id \
                    AND r.disposition = 'qualified'\
             ) AND candidate_id IN ({placeholders})"
        );
        let mut stmt = match conn.prepare(&sql) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        let rows = stmt.query_map(rusqlite::params_from_iter(ids.iter()), |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)? != 0,
                row.get::<_, String>(3)?,
            ))
        });
        rows.into_iter()
            .flatten()
            .flatten()
            .filter_map(|(id, outcome, verified, sha)| {
                let outcome = parse_outcome(&outcome).ok()?;
                // Drop a stale veto: a verified contradiction whose sha no longer matches the
                // current bytes has been superseded — it must not veto the corrected memory.
                if verified
                    && outcome == Outcome::Contradicted
                    && current_sha
                        .get(id.as_str())
                        .map(|s| *s != sha)
                        .unwrap_or(true)
                {
                    return None;
                }
                Some(MemoryUsageRecord {
                    entry_id: id,
                    injected: true,
                    task_succeeded: matches!(outcome, Outcome::Used),
                    outcome: Some(outcome),
                    verified,
                })
            })
            .collect()
    }

    /// Load persisted feedback ONLY for the candidate ids under consideration this recall. Bounded
    /// by the candidate slate (≈ `limit * 3`), not the table size — the `idx_context_feedback_candidate`
    /// index serves the `IN (...)` lookup. This is what the `EffectivenessGate` reads on the hot path
    /// so it sees verified vetoes across restarts without an O(table) scan per recall.
    pub fn feedback_rows_for(&self, candidate_ids: &[String]) -> Vec<MemoryUsageRecord> {
        if candidate_ids.is_empty() {
            return Vec::new();
        }
        let conn = self.db.lock();
        let placeholders = std::iter::repeat_n("?", candidate_ids.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT candidate_id, outcome, verified FROM context_feedback \
             WHERE candidate_id IN ({placeholders})"
        );
        let mut stmt = match conn.prepare(&sql) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        let rows = stmt.query_map(rusqlite::params_from_iter(candidate_ids.iter()), |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)? != 0,
            ))
        });
        rows.into_iter()
            .flatten()
            .flatten()
            .filter_map(|(id, outcome, verified)| {
                Self::feedback_row_to_record(id, &outcome, verified)
            })
            .collect()
    }

    /// Add a manually curated durable memory entry.
    pub fn remember(&self, content: &str, keywords: Vec<String>) -> MemoryEntry {
        let trimmed = content.trim();
        let keywords = if keywords.is_empty() {
            keywords_of(trimmed)
        } else {
            keywords
                .into_iter()
                .map(|k| k.trim().to_lowercase())
                .filter(|k| !k.is_empty())
                .collect()
        };
        let entry = MemoryEntry {
            id: format!(
                "mem-manual-{}-{}",
                crate::time::now_millis(),
                MEM_SEQ.fetch_add(1, Ordering::Relaxed)
            ),
            tier: MemoryTier::Semantic,
            embedding: Some(self.embedder.embed(trimmed)),
            content: trimmed.to_string(),
            keywords,
            score: 1.0,
            created_at: crate::time::now_iso(),
            access_count: 0,
            // P2 will thread the active project; default 'global' until then.
            scope_id: crypt_core::default_scope(),
        };
        if self.persist_entry(&entry).is_ok() {
            self.registry
                .write()
                .unwrap_or_else(|e| e.into_inner())
                .insert(entry.clone());
        }
        entry
    }

    /// Ingest a markdown memory file under `scope` (the workspace's durable `.md` corpus → CR's
    /// engine). Parses frontmatter + `[[wikilinks]]` with the shared `crypt_core` parser, embeds
    /// `"{name} {description} {body}"`, and stores a scope-qualified Semantic entry. Returns its id.
    pub fn ingest_markdown(&self, path: &std::path::Path, scope: &str) -> Option<String> {
        let raw = std::fs::read_to_string(path).ok()?;
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("x");
        let doc = crypt_core::parse_markdown(stem, &raw);
        let embed_text = format!("{} {} {}", doc.name, doc.description, doc.body);
        let id = format!("{scope}/{}", doc.id);
        let mut keywords = vec![doc.type_.clone()];
        keywords.extend(doc.links.iter().cloned());
        let entry = MemoryEntry {
            id: id.clone(),
            tier: MemoryTier::Semantic,
            embedding: Some(self.embedder.embed(&embed_text)),
            content: doc.body,
            keywords,
            score: if doc.pinned { 1.0 } else { 0.6 },
            created_at: crate::time::now_iso(),
            access_count: 0,
            scope_id: scope.to_string(),
        };
        if self.persist_entry(&entry).is_err() {
            return None;
        }
        self.registry
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert(entry);
        Some(id)
    }

    /// Add or update a deterministic consolidation memory.
    ///
    /// Daily/background memory managers use stable ids so repeated analysis is
    /// idempotent instead of producing duplicate durable memories every night.
    pub fn remember_consolidated(
        &self,
        id: &str,
        content: &str,
        keywords: Vec<String>,
        score: f64,
    ) -> Option<MemoryEntry> {
        let id = sanitize_memory_id(id);
        let trimmed = content.trim();
        if id.is_empty() || trimmed.is_empty() {
            return None;
        }
        let keywords = if keywords.is_empty() {
            keywords_of(trimmed)
        } else {
            keywords
                .into_iter()
                .map(|k| k.trim().to_lowercase())
                .filter(|k| !k.is_empty())
                .collect()
        };
        let entry = MemoryEntry {
            id,
            tier: MemoryTier::Semantic,
            embedding: Some(self.embedder.embed(trimmed)),
            content: trimmed.to_string(),
            keywords,
            score: score.clamp(0.0, 1.0),
            created_at: crate::time::now_iso(),
            access_count: 0,
            // P2 will thread the active project; default 'global' until then.
            scope_id: crypt_core::default_scope(),
        };
        if self.persist_entry(&entry).is_err() {
            return None;
        }
        self.registry
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert(entry.clone());
        Some(entry)
    }

    /// Most recent memory entries, bounded for API/UI display.
    pub fn entries(&self, limit: usize) -> Vec<MemoryEntry> {
        let mut entries = self
            .registry
            .read()
            .unwrap()
            .all()
            .into_iter()
            .cloned()
            .collect::<Vec<_>>();
        entries.sort_by(|a, b| {
            b.created_at
                .cmp(&a.created_at)
                .then_with(|| b.id.cmp(&a.id))
        });
        entries.truncate(limit);
        entries
    }

    /// Content-free artifact dimensions for a bounded candidate slate. One prepared statement
    /// and one DB lock keep this additive provider lookup local and cheap.
    pub fn artifact_dimensions(&self, memory_ids: &[String]) -> HashMap<String, (String, String)> {
        if memory_ids.is_empty() {
            return HashMap::new();
        }
        let conn = self.db.lock();
        let Ok(mut statement) =
            conn.prepare("SELECT artifact_family, producer FROM memories WHERE id=?1")
        else {
            return HashMap::new();
        };
        memory_ids
            .iter()
            .filter_map(|memory_id| {
                statement
                    .query_row(rusqlite::params![memory_id], |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                    })
                    .optional()
                    .ok()
                    .flatten()
                    .map(|dimensions| (memory_id.clone(), dimensions))
            })
            .collect()
    }

    /// Deterministic embedding-neighbor graph for the local and anonymous dashboards.
    pub fn relationship_graph(&self, neighbors: usize, min_cosine: f32) -> RelationshipGraph {
        let mut entries = self
            .registry
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .all()
            .into_iter()
            .cloned()
            .collect::<Vec<_>>();
        entries.sort_by(|left, right| left.id.cmp(&right.id));
        let counters = self
            .list(None)
            .into_iter()
            .map(|(id, _tier, _chars, access, inject)| (id, (access, inject)))
            .collect::<std::collections::HashMap<_, _>>();
        let nodes = entries
            .iter()
            .map(|entry| {
                let (access, inject) = counters.get(&entry.id).copied().unwrap_or((0, 0));
                let activity = access.max(0) as u32 + inject.max(0) as u32;
                RelationshipGraphNode {
                    id: entry.id.clone(),
                    tier: format!("{:?}", entry.tier),
                    scope: entry.scope_id.clone(),
                    state: if access > 0 {
                        "fetched"
                    } else if inject > 0 {
                        "preview-only"
                    } else {
                        "dormant"
                    }
                    .to_string(),
                    weight: (1 + (u32::BITS - (1 + activity).leading_zeros() - 1)).min(9),
                }
            })
            .collect();
        let mut pairs = std::collections::BTreeMap::<(String, String), f32>::new();
        for (index, entry) in entries.iter().enumerate() {
            let Some(embedding) = entry.embedding.as_deref() else {
                continue;
            };
            let mut ranked = entries
                .iter()
                .enumerate()
                .filter_map(|(other_index, other)| {
                    if index == other_index {
                        return None;
                    }
                    other
                        .embedding
                        .as_deref()
                        .map(|other_embedding| (cosine(embedding, other_embedding), other_index))
                })
                .filter(|(score, _)| score.is_finite() && *score >= min_cosine)
                .collect::<Vec<_>>();
            ranked.sort_by(|left, right| {
                right
                    .0
                    .partial_cmp(&left.0)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| entries[left.1].id.cmp(&entries[right.1].id))
            });
            for (score, other_index) in ranked.into_iter().take(neighbors) {
                let (source, target) = if entry.id < entries[other_index].id {
                    (entry.id.clone(), entries[other_index].id.clone())
                } else {
                    (entries[other_index].id.clone(), entry.id.clone())
                };
                pairs
                    .entry((source, target))
                    .and_modify(|existing| *existing = existing.max(score))
                    .or_insert(score);
            }
        }
        RelationshipGraph {
            schema_version: 2,
            nodes,
            edges: pairs
                .into_iter()
                .map(|((source, target), score)| RelationshipGraphEdge {
                    source,
                    target,
                    kind: "semantic".to_string(),
                    score,
                })
                .collect(),
        }
    }

    pub fn relationship_graph_json(
        &self,
        neighbors: usize,
        min_cosine: f32,
        anonymous: bool,
    ) -> serde_json::Value {
        let graph = self.relationship_graph(neighbors, min_cosine);
        if !anonymous {
            return serde_json::to_value(graph).unwrap_or_else(|_| {
                serde_json::json!({
                    "schema_version": 2, "nodes": [], "edges": []
                })
            });
        }
        use sha2::Digest;
        let mut nodes = graph.nodes;
        nodes.sort_by_key(|node| sha2::Sha256::digest(node.id.as_bytes()).to_vec());
        let indices = nodes
            .iter()
            .enumerate()
            .map(|(index, node)| (node.id.clone(), index))
            .collect::<std::collections::HashMap<_, _>>();
        let mut scope_hashes = nodes
            .iter()
            .map(|node| format!("{:x}", sha2::Sha256::digest(node.scope.as_bytes())))
            .collect::<Vec<_>>();
        scope_hashes.sort();
        scope_hashes.dedup();
        let clusters = scope_hashes
            .iter()
            .enumerate()
            .map(|(index, digest)| (digest.clone(), index))
            .collect::<std::collections::HashMap<_, _>>();
        let anonymous_nodes = nodes
            .iter()
            .enumerate()
            .map(|(index, node)| {
                let digest = format!("{:x}", sha2::Sha256::digest(node.scope.as_bytes()));
                serde_json::json!({
                    "i": index,
                    "tier": node.tier.to_lowercase(),
                    "state": node.state,
                    "weight": node.weight,
                    "cluster": clusters[&digest],
                })
            })
            .collect::<Vec<_>>();
        let mut anonymous_edges = graph
            .edges
            .iter()
            .filter_map(|edge| {
                let source = *indices.get(&edge.source)?;
                let target = *indices.get(&edge.target)?;
                Some(serde_json::json!({
                    "s": source.min(target),
                    "t": source.max(target),
                    "kind": edge.kind,
                }))
            })
            .collect::<Vec<_>>();
        anonymous_edges.sort_by_key(|edge| {
            (
                edge.get("s").and_then(|value| value.as_u64()).unwrap_or(0),
                edge.get("t").and_then(|value| value.as_u64()).unwrap_or(0),
            )
        });
        serde_json::json!({
            "schema_version": 2,
            "nodes": anonymous_nodes,
            "edges": anonymous_edges,
        })
    }

    /// Search memory entries without marking them as injected into a model context.
    pub fn search(&self, query: &str, limit: usize) -> Vec<MemoryEntry> {
        let qvec = self.embed_query_cached(query);
        let registry = self.registry.read().unwrap_or_else(|e| e.into_inner());
        MemoryRetriever::retrieve_hybrid(&registry, query, Some(&qvec), limit)
            .into_iter()
            .cloned()
            .collect()
    }

    /// Return the content-free status of the most recent recall operation.
    /// `insufficient_confidence` means candidates existed but every one was below the
    /// authority boundary, so callers must abstain instead of treating an empty result as
    /// ordinary "no match".
    pub fn last_recall_status(&self) -> Option<String> {
        self.last_recall_status.lock().unwrap().clone()
    }

    /// Content-free lifecycle admission set for deterministic replay and audit.
    pub fn recall_eligible_ids_at(&self, as_of_ms: i64, include_expired: bool) -> HashSet<String> {
        let conn = self.db.lock();
        recall_eligible_ids_on(&conn, as_of_ms, include_expired).unwrap_or_default()
    }

    /// Content-free lifecycle fields for `/get` and dashboard editing.
    pub fn lifecycle_json_for(&self, id: &str) -> Result<serde_json::Value, String> {
        let conn = self.db.lock();
        conn.query_row(
            "SELECT effective_from_ms, effective_until_ms, expires_at_ms, review_after_ms,
                    priority_class, confidence, confidence_basis, lifecycle_state, superseded_by
             FROM memories WHERE id=?1",
            rusqlite::params![id],
            |row| {
                Ok(serde_json::json!({
                    "effectiveFromMs": row.get::<_, Option<i64>>(0)?,
                    "effectiveUntilMs": row.get::<_, Option<i64>>(1)?,
                    "expiresAtMs": row.get::<_, Option<i64>>(2)?,
                    "reviewAfterMs": row.get::<_, Option<i64>>(3)?,
                    "priorityClass": row.get::<_, String>(4)?,
                    "confidence": row.get::<_, Option<f64>>(5)?,
                    "confidenceBasis": row.get::<_, Option<String>>(6)?,
                    "state": row.get::<_, String>(7)?,
                    "supersededBy": row.get::<_, Option<String>>(8)?,
                }))
            },
        )
        .map_err(|error| format!("lifecycle lookup failed for {id}: {error}"))
    }

    /// Scope-aware recall: unscoped calls keep the measured hybrid-candidate + cosine order.
    /// Scoped calls retrieve per scope in chain order (self, ancestors, global) and sort by
    /// cosine within each scope so a large global corpus cannot drown out workspace memories.
    ///
    /// TESTED AND KEPT (2026-07-05): shipping the fused order end-to-end was tried and REVERTED
    /// same-day — the pre-flip gate (30 known-useful targets vs the frozen ship snapshot) showed
    /// fused ranking degrading mean rank 2.37 → 6.07 (+3.70; tolerance +1.0). The lexical RRF
    /// channel floods rank positions with keyword-popular entries, displacing exact semantic
    /// matches. Fused-for-candidates + cosine-for-order is the measured right architecture for
    /// this corpus; re-test only if real-traffic replay (recall_log.query_preview) says otherwise.
    pub fn recall_scored(
        &self,
        query: &str,
        limit: usize,
        scopes: &[String],
    ) -> Vec<(MemoryEntry, f32)> {
        self.recall_scored_at(
            query,
            limit,
            scopes,
            crate::time::now_millis() as i64,
            false,
        )
    }

    fn protected_ids(&self) -> HashSet<String> {
        let conn = self.db.lock();
        let mut statement =
            match conn.prepare("SELECT id FROM memories WHERE priority_class='protected'") {
                Ok(statement) => statement,
                Err(_) => return HashSet::new(),
            };
        statement
            .query_map([], |row| row.get::<_, String>(0))
            .map(|rows| rows.flatten().collect())
            .unwrap_or_default()
    }

    /// Replay/audit recall at a fixed instant. `include_expired` is audit-only and never changes ranking.
    pub fn recall_scored_at(
        &self,
        query: &str,
        limit: usize,
        scopes: &[String],
        as_of_ms: i64,
        include_expired: bool,
    ) -> Vec<(MemoryEntry, f32)> {
        self.recall_scored_detailed_timed_at(query, limit, scopes, as_of_ms, include_expired)
            .0
            .into_iter()
            .map(|hit| (hit.entry, hit.score))
            .collect()
    }

    /// Stable recall output plus a content-free decomposition used by phase-0 telemetry.
    pub fn recall_scored_timed(
        &self,
        query: &str,
        limit: usize,
        scopes: &[String],
    ) -> (Vec<(MemoryEntry, f32)>, RecallStageElapsed) {
        let (hits, elapsed) = self.recall_scored_detailed_timed_at(
            query,
            limit,
            scopes,
            crate::time::now_millis() as i64,
            false,
        );
        let hits = hits.into_iter().map(|hit| (hit.entry, hit.score)).collect();
        (hits, elapsed)
    }

    /// Shared recall with an explicit origin marker for content-free link-inclusion telemetry.
    /// Keep `recall_scored` as the stable tuple API used by existing callers.
    pub fn recall_scored_detailed(
        &self,
        query: &str,
        limit: usize,
        scopes: &[String],
    ) -> Vec<ScoredRecallHit> {
        self.recall_scored_detailed_timed_at(
            query,
            limit,
            scopes,
            crate::time::now_millis() as i64,
            false,
        )
        .0
    }

    fn recall_scored_detailed_timed_at(
        &self,
        query: &str,
        limit: usize,
        scopes: &[String],
        as_of_ms: i64,
        include_expired: bool,
    ) -> (Vec<ScoredRecallHit>, RecallStageElapsed) {
        *self.last_recall_status.lock().unwrap() = None;
        let mut elapsed = RecallStageElapsed::default();
        if limit == 0 {
            return (Vec::new(), elapsed);
        }
        let link_enabled = !scopes.is_empty()
            && std::env::var("RIGHTCONTEXT_LINK_RECALL")
                .map(|value| {
                    !matches!(
                        value.trim().to_ascii_lowercase().as_str(),
                        "0" | "false" | "off" | "no"
                    )
                })
                .unwrap_or(true);
        // Reserve a small, bounded graph lane. Direct semantic ranking keeps at least 80% of the
        // caller's budget; one-hop neighbours can use at most eight slots and never expand beyond
        // `limit`. This prevents hubs from flooding the context while allowing useful links to
        // displace only the lowest direct tail.
        let link_slots = if link_enabled && limit > 1 {
            (limit / 5).clamp(1, 8).min(limit - 1)
        } else {
            0
        };
        let direct_limit = limit - link_slots;
        let candidate_limit = limit.saturating_mul(4).max(limit);
        // embed_query, not embed: BGE is asymmetric — queries get the instruction prefix
        // (FastEmbedder override); symmetric embedders default embed_query to embed.
        let embed_started = Instant::now();
        let qvec = self.embed_query_cached(query);
        elapsed.embed_ms = embed_started.elapsed().as_secs_f64() * 1000.0;
        let lock_started = Instant::now();
        let registry = self.registry.read().unwrap_or_else(|e| e.into_inner());
        let lock_ms = lock_started.elapsed().as_secs_f64() * 1000.0;
        let recall_started = Instant::now();
        let vector_dispatch_v2 = vector_dispatch_v2_enabled();
        let mut direct_candidates: Vec<(MemoryEntry, f32)> = if scopes.is_empty() {
            let retrieved = if vector_dispatch_v2 {
                MemoryRetriever::retrieve_hybrid_indexed(
                    &registry,
                    query,
                    Some(&qvec),
                    candidate_limit,
                    None,
                )
            } else {
                MemoryRetriever::retrieve_hybrid(&registry, query, Some(&qvec), candidate_limit)
            };
            retrieved
                .into_iter()
                .map(|e| {
                    let cos = e
                        .embedding
                        .as_deref()
                        .map(|emb| cosine(&qvec, emb))
                        .unwrap_or(0.0);
                    (e.clone(), cos)
                })
                .collect()
        } else {
            let mut ranked = Vec::<(usize, MemoryEntry, f32)>::new();
            let mut seen = std::collections::HashSet::<String>::new();
            let per_scope_limit = candidate_limit.max(SCOPED_CANDIDATE_FLOOR);
            for (scope_rank, scope) in scopes.iter().enumerate() {
                let scope_filter = [scope.as_str()];
                let retrieved = if vector_dispatch_v2 {
                    MemoryRetriever::retrieve_hybrid_indexed(
                        &registry,
                        query,
                        Some(&qvec),
                        per_scope_limit,
                        Some(&scope_filter),
                    )
                } else {
                    let mut scoped_registry = MemoryRegistry::new();
                    for entry in registry.all().into_iter().filter(|e| scope == &e.scope_id) {
                        scoped_registry.insert(entry.clone());
                    }
                    let ids = MemoryRetriever::retrieve_hybrid(
                        &scoped_registry,
                        query,
                        Some(&qvec),
                        per_scope_limit,
                    )
                    .into_iter()
                    .map(|entry| entry.id.clone())
                    .collect::<Vec<_>>();
                    ids.into_iter().filter_map(|id| registry.get(&id)).collect()
                };
                for e in retrieved {
                    if !seen.insert(e.id.clone()) {
                        continue;
                    }
                    let cos = e
                        .embedding
                        .as_deref()
                        .map(|emb| cosine(&qvec, emb))
                        .unwrap_or(0.0);
                    ranked.push((scope_rank, e.clone(), cos));
                }
            }
            let chain_len = scopes.len();
            ranked.sort_by(|a, b| {
                let a_scope_bonus =
                    chain_len.saturating_sub(a.0 + 1) as f32 * SCOPE_CHAIN_SOFT_BONUS;
                let b_scope_bonus =
                    chain_len.saturating_sub(b.0 + 1) as f32 * SCOPE_CHAIN_SOFT_BONUS;
                let a_score = a.2 + a_scope_bonus;
                let b_score = b.2 + b_scope_bonus;
                b_score
                    .partial_cmp(&a_score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            ranked
                .into_iter()
                .take(candidate_limit)
                .map(|(_, entry, cos)| (entry, cos))
                .collect()
        };
        elapsed.recall_ms = lock_ms + recall_started.elapsed().as_secs_f64() * 1000.0;
        let candidate_ids = direct_candidates
            .iter()
            .map(|(entry, _)| entry.id.clone())
            .collect::<Vec<_>>();
        let eligible_ids = self.recall_eligible_ids_at(as_of_ms, include_expired);
        let confidence_abstained =
            !candidate_ids.is_empty() && candidate_ids.iter().all(|id| !eligible_ids.contains(id));
        direct_candidates.retain(|(entry, _)| eligible_ids.contains(&entry.id));
        let rank_started = Instant::now();
        if scopes.is_empty() {
            direct_candidates
                .sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            direct_candidates.truncate(candidate_limit);
        }
        drop(registry);

        // Retrieval-time utility/frequency adjustment closes the feedback loop without
        // replacing semantic relevance or allowing a low-quality row to leap the gate.
        for (entry, score) in &mut direct_candidates {
            *score = utility_recall_score(entry, *score);
        }
        direct_candidates
            .sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // Apply the feedback rail before reserving graph capacity so vetoed direct candidates are
        // backfilled from the bounded semantic slate.
        let direct_refs = direct_candidates
            .iter()
            .map(|(entry, _)| entry)
            .collect::<Vec<_>>();
        let mut history = self
            .effectiveness_history
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        history.extend(self.gate_history_for(&direct_refs));
        let gate = EffectivenessGate::default();
        direct_candidates.retain(|(entry, _)| gate.should_inject(&history, &entry.id));
        let protected = if protected_priority_bonus_enabled() {
            self.protected_ids()
        } else {
            HashSet::new()
        };
        if protected_priority_bonus_enabled() {
            for (entry, score) in &mut direct_candidates {
                if protected.contains(&entry.id) {
                    *score += PROTECTED_PRIORITY_BONUS;
                }
            }
            direct_candidates
                .sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        }

        let mut hits = direct_candidates
            .iter()
            .take(direct_limit)
            .cloned()
            .collect::<Vec<_>>();
        let mut linked_ids = std::collections::HashSet::<String>::new();
        if link_slots > 0 {
            let seed_ids = hits
                .iter()
                .map(|(entry, _)| entry.id.clone())
                .collect::<Vec<_>>();
            let mut seen = seed_ids
                .iter()
                .cloned()
                .collect::<std::collections::HashSet<_>>();
            let linked = self.linked_neighbors(&seed_ids, scopes, link_slots.saturating_mul(4));
            let linked_refs = linked.iter().map(|(entry, _)| entry).collect::<Vec<_>>();
            history.extend(self.gate_history_for(&linked_refs));
            for (entry, score) in linked {
                if !eligible_ids.contains(&entry.id) {
                    continue;
                }
                let mut score = utility_recall_score(&entry, score);
                if gate.should_inject(&history, &entry.id) && seen.insert(entry.id.clone()) {
                    if protected_priority_bonus_enabled() && protected.contains(&entry.id) {
                        score += PROTECTED_PRIORITY_BONUS;
                    }
                    linked_ids.insert(entry.id.clone());
                    hits.push((entry, score));
                }
                if hits.len() >= limit {
                    break;
                }
            }
        }
        // Any unused graph lane (no links, dangling links, or vetoed links) is returned to direct
        // semantic recall so enabling graph recall never shrinks ordinary k-result coverage.
        let mut seen = hits
            .iter()
            .map(|(entry, _)| entry.id.clone())
            .collect::<std::collections::HashSet<_>>();
        for (entry, score) in direct_candidates.into_iter().skip(direct_limit) {
            if hits.len() >= limit {
                break;
            }
            if seen.insert(entry.id.clone()) {
                hits.push((entry, score));
            }
        }
        let hits: Vec<ScoredRecallHit> = hits
            .into_iter()
            .map(|(entry, score)| ScoredRecallHit {
                origin: if linked_ids.contains(&entry.id) {
                    "link"
                } else {
                    "semantic"
                },
                entry,
                score,
            })
            .collect();
        if confidence_abstained && hits.is_empty() {
            *self.last_recall_status.lock().unwrap() = Some("insufficient_confidence".to_string());
        }
        elapsed.rank_ms = rank_started.elapsed().as_secs_f64() * 1000.0;
        (hits, elapsed)
    }

    /// Distinct scope_ids present in the store (for computing a recall scope chain).
    pub fn scopes(&self) -> Vec<String> {
        let registry = self.registry.read().unwrap_or_else(|e| e.into_inner());
        let mut s: Vec<String> = registry.all().iter().map(|e| e.scope_id.clone()).collect();
        s.sort();
        s.dedup();
        s
    }

    fn retrieval_tier_for(&self, goal: &str) -> (crypt_core::QueryFeatures, RetrievalTier) {
        let features = RoutingPolicy::features(goal);
        let tier = self.routing_policy.lock().unwrap().route(&features);
        (features, tier)
    }

    /// A compact block of relevant past experience for `goal`, or `None` if the
    /// store has nothing relevant.
    pub fn context_for(&self, goal: &str, limit: usize) -> Option<String> {
        *self.last_recall_status.lock().unwrap() = None;
        let terms = keywords_of(goal);
        let eval_gate = MemoryRetrievalEvalGate::default();
        let effectiveness_gate = EffectivenessGate::default();
        let mut token_accounting = ContextTokenAccounting::new(MEMORY_TOKEN_BUDGET);
        let (features, tier) = self.retrieval_tier_for(goal);
        *self.last_route.lock().unwrap() = Some((features, tier));
        let qvec = self.embed_query_cached(goal);
        let registry = self.registry.read().unwrap_or_else(|e| e.into_inner());

        // Decide retrieval strategy using the learned routing policy.
        let candidates = match tier {
            RetrievalTier::Low => MemoryRetriever::retrieve(&registry, goal, limit * 3),
            RetrievalTier::Mid => {
                MemoryRetriever::retrieve_hybrid(&registry, goal, Some(&qvec), limit * 2)
            }
            RetrievalTier::High => {
                MemoryRetriever::retrieve_hybrid(&registry, goal, Some(&qvec), limit * 3)
            }
        }
        .into_iter()
        .cloned()
        .collect::<Vec<_>>();
        drop(registry);

        // Apply quality eval gate: drop low-quality entries.
        let candidate_ids = candidates
            .iter()
            .map(|entry| entry.id.clone())
            .collect::<Vec<_>>();
        let eligible_ids = self.recall_eligible_ids_at(crate::time::now_millis() as i64, false);
        let confidence_abstained =
            !candidate_ids.is_empty() && candidate_ids.iter().all(|id| !eligible_ids.contains(id));
        let filtered = eval_gate
            .filter(candidates.iter().collect())
            .into_iter()
            .cloned()
            .filter(|entry| eligible_ids.contains(&entry.id))
            .collect::<Vec<_>>();
        if confidence_abstained {
            *self.last_recall_status.lock().unwrap() = Some("insufficient_confidence".to_string());
        }
        // Gate history = live in-RAM task-level rows UNION durable per-candidate feedback rows.
        // The DB rows are what survive a serve restart and carry verified `contradicted` vetoes.
        // Bounded to just this recall's candidate slate — never a full-table scan. sha-aware:
        // a superseded contradiction (content edited since) no longer vetoes.
        let mut effectiveness_history = self.effectiveness_history.lock().unwrap().clone();
        let filtered_refs = filtered.iter().collect::<Vec<_>>();
        effectiveness_history.extend(self.gate_history_for(&filtered_refs));

        // Keep only entries that are genuinely relevant (lexical or semantic match).
        let relevant: Vec<&MemoryEntry> = filtered
            .iter()
            .filter(|h| effectiveness_gate.should_inject(&effectiveness_history, &h.id))
            .filter(|h| {
                let content = h.content.to_lowercase();
                let lexical = terms.iter().any(|t| {
                    content.contains(t.as_str())
                        || h.keywords.iter().any(|k| k.eq_ignore_ascii_case(t))
                });
                let semantic = h
                    .embedding
                    .as_deref()
                    .map(|e| cosine(e, &qvec) >= SEMANTIC_THRESHOLD)
                    .unwrap_or(false);
                lexical || semantic
            })
            .collect();

        // Truncate to fit within token budget: stop adding entries if the next one
        // would exceed the budget.
        let mut block_entries = Vec::new();
        for entry in relevant.into_iter().take(limit) {
            if token_accounting.would_exceed(&entry.content) {
                break;
            }
            token_accounting.add(&entry.content);
            block_entries.push(entry);
        }

        if block_entries.is_empty() {
            None
        } else {
            let ids = block_entries
                .iter()
                .map(|entry| entry.id.clone())
                .collect::<Vec<_>>();
            *self.pending_injections.lock().unwrap() = ids.clone();
            let block = block_entries
                .into_iter()
                .map(|h| format!("- {}\n", h.content))
                .collect::<String>();
            if let Err(error) =
                self.record_injections_observed(&ids, &MemoryEventContext::default())
            {
                eprintln!("[memory] context injection event failed: {error}");
            }
            Some(block)
        }
    }

    /// Convenience: default-limit retrieval.
    pub fn context(&self, goal: &str) -> Option<String> {
        self.context_for(goal, RETRIEVE_LIMIT)
    }

    /// Number of stored entries (diagnostics).
    pub fn len(&self) -> usize {
        self.registry
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .len()
    }

    /// Whether the store has no entries.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Record one recall's full-corpus-vs-injected-skeleton size for the savings report
    /// (`crypt metrics`). Best-effort: never fails the recall it's measuring.
    ///
    /// 2026-07-09 (add-now plan, Phase 1): `client`/`session_id`/`cwd_scope`/`hook_event`/
    /// `trace_id`/`client_visibility` are optional attribution fields — when None the DB
    /// backfills client='unknown' and the rest as NULL (so older CLI/test call sites still
    /// compile and produce useful 'unknown'-bucketed rows for the metric).
    #[allow(clippy::too_many_arguments)]
    pub fn log_recall(
        &self,
        ts: &str,
        scope: Option<&str>,
        query_chars: usize,
        hit_count: usize,
        full_chars: usize,
        injected_chars: usize,
        source: &str,
        query_preview: Option<&str>,
        client: Option<&str>,
        session_id: Option<&str>,
        cwd_scope: Option<&str>,
        hook_event: Option<&str>,
        trace_id: Option<&str>,
        client_visibility: Option<&str>,
    ) {
        self.log_recall_with_replay(
            ts,
            scope,
            query_chars,
            hit_count,
            full_chars,
            injected_chars,
            source,
            query_preview,
            client,
            session_id,
            cwd_scope,
            hook_event,
            trace_id,
            client_visibility,
            "production",
            "[]",
            "[]",
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub fn log_recall_with_replay(
        &self,
        ts: &str,
        scope: Option<&str>,
        query_chars: usize,
        hit_count: usize,
        full_chars: usize,
        injected_chars: usize,
        source: &str,
        query_preview: Option<&str>,
        client: Option<&str>,
        session_id: Option<&str>,
        cwd_scope: Option<&str>,
        hook_event: Option<&str>,
        trace_id: Option<&str>,
        client_visibility: Option<&str>,
        traffic_class: &str,
        candidate_hits: &str,
        admitted_hits: &str,
    ) {
        let _ = self.log_recall_with_replay_to(
            crate::memdb::RecallLogSink::Production,
            ts,
            scope,
            query_chars,
            hit_count,
            full_chars,
            injected_chars,
            source,
            query_preview,
            client,
            session_id,
            cwd_scope,
            hook_event,
            trace_id,
            client_visibility,
            traffic_class,
            candidate_hits,
            admitted_hits,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub fn log_recall_with_replay_to(
        &self,
        sink: crate::memdb::RecallLogSink,
        ts: &str,
        scope: Option<&str>,
        query_chars: usize,
        hit_count: usize,
        full_chars: usize,
        injected_chars: usize,
        source: &str,
        query_preview: Option<&str>,
        client: Option<&str>,
        session_id: Option<&str>,
        cwd_scope: Option<&str>,
        hook_event: Option<&str>,
        trace_id: Option<&str>,
        client_visibility: Option<&str>,
        traffic_class: &str,
        candidate_hits: &str,
        admitted_hits: &str,
    ) -> Result<(), String> {
        let client = client.unwrap_or("unknown");
        let session_id = session_id
            .map(|value| opaque_correlation_token(value, "session"))
            .unwrap_or_else(|| {
                opaque_correlation_token(&format!("recall:{client}:{ts}"), "session")
            });
        let trace_id = trace_id
            .map(|value| opaque_correlation_token(value, "trace"))
            .unwrap_or_else(|| opaque_correlation_token(&format!("recall:{client}:{ts}"), "trace"));
        let turn_id = opaque_correlation_token(&trace_id, "turn");
        self.db.log_recall_with_replay_to_identity(
            sink,
            ts,
            scope,
            query_chars,
            hit_count,
            full_chars,
            injected_chars,
            source,
            query_preview,
            Some(client),
            Some(&session_id),
            cwd_scope,
            hook_event,
            Some(&trace_id),
            client_visibility,
            traffic_class,
            candidate_hits,
            admitted_hits,
            Some(&self.operation_attribution.installation_id),
            Some(&self.operation_attribution.service_instance_id),
            Some(&turn_id),
        )
    }

    /// 2026-07-09 (add-now plan, Phase 1): expose per-client serve counts so the metrics block
    /// can answer "which clients actually used recall this session". Empty if no recalls yet.
    pub fn client_recall_counts(&self) -> Vec<(String, u64)> {
        self.db.client_recall_counts()
    }

    /// Persist a deterministic session-level holdout assignment. The assignment is derived only
    /// from stable identifiers, so retries and process restarts cannot move a session between
    /// control and candidate. No prompt or memory content is stored.
    pub fn assign_context_policy(
        &self,
        session_id: &str,
        surface: &str,
        policy_version: &str,
        control_pct: u8,
        task_class: &str,
    ) -> Result<String, String> {
        use sha2::{Digest, Sha256};

        let session_id = session_id.trim();
        let surface = surface.trim();
        let policy_version = policy_version.trim();
        let task_class = task_class.trim();
        if session_id.is_empty() || surface.is_empty() || policy_version.is_empty() {
            return Err("session_id, surface, and policy_version are required".into());
        }
        if control_pct > 100 {
            return Err("control_pct must be between 0 and 100".into());
        }
        if session_id.chars().count() > 160
            || surface.chars().count() > 64
            || policy_version.chars().count() > 80
        {
            return Err("policy assignment identifier too long".into());
        }
        let mut hash = Sha256::new();
        hash.update(policy_version.as_bytes());
        hash.update([0]);
        hash.update(surface.as_bytes());
        hash.update([0]);
        hash.update(session_id.as_bytes());
        let digest = hash.finalize();
        let bucket = u64::from_be_bytes(digest[..8].try_into().expect("sha256 prefix")) % 100;
        let cohort = if bucket < u64::from(control_pct) {
            "control"
        } else {
            "candidate"
        };
        let mut conn = self.db.lock();
        let tx = conn
            .transaction()
            .map_err(|e| format!("policy assignment transaction failed: {e}"))?;
        tx.execute(
            "INSERT OR IGNORE INTO context_policy_assignment
             (session_id, surface, policy_version, cohort, assigned_at, task_class)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                session_id,
                surface,
                policy_version,
                cohort,
                crate::time::now_iso(),
                if task_class.is_empty() {
                    "unknown"
                } else {
                    task_class
                },
            ],
        )
        .map_err(|e| format!("policy assignment insert failed: {e}"))?;
        let persisted = tx
            .query_row(
                "SELECT cohort FROM context_policy_assignment
                 WHERE session_id=?1 AND surface=?2 AND policy_version=?3",
                rusqlite::params![session_id, surface, policy_version],
                |row| row.get::<_, String>(0),
            )
            .map_err(|e| format!("policy assignment load failed: {e}"))?;
        tx.commit()
            .map_err(|e| format!("policy assignment commit failed: {e}"))?;
        Ok(persisted)
    }

    /// Preregister one immutable ranking experiment and its exact ordered update set.
    pub fn register_learning_experiment(
        &self,
        request: &LearningExperimentV1,
    ) -> Result<String, String> {
        let hash_ok = |value: &str| {
            value.len() == 64
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        };
        if request.schema_version != 1
            || !valid_opaque_id(&request.experiment_id, 160)
            || !valid_opaque_id(&request.policy_version, 80)
            || !valid_registry_token(&request.task_class)
            || !hash_ok(&request.policy_activation_sha256)
            || request.observed_since >= request.observed_through
            || !request.observed_since.ends_with('Z')
            || !request.observed_through.ends_with('Z')
            || !(1..=5_000).contains(&request.min_per_cohort)
            || !(-10_000..=10_000).contains(&request.min_effect_bps)
            || request.targets.is_empty()
            || request.targets.len() > 64
        {
            return Err("learning experiment is invalid or unbounded".into());
        }
        let mut ids = HashSet::new();
        if request.targets.iter().any(|target| {
            target.memory_id.trim().is_empty()
                || target.memory_id.len() > 512
                || target.memory_id.chars().any(char::is_control)
                || !hash_ok(&target.content_sha256)
                || target.delta_micros == 0
                || !(-1_000_000..=1_000_000).contains(&target.delta_micros)
                || !ids.insert(target.memory_id.as_str())
        }) {
            return Err("learning update target is invalid or duplicated".into());
        }
        let canonical = serde_json::to_string(request).map_err(|e| e.to_string())?;
        let request_sha256 = content_hash(&canonical);
        let mut conn = self.db.lock();
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        let existing: Option<String> = tx
            .query_row(
                "SELECT request_sha256 FROM learning_experiment WHERE experiment_id=?1",
                [&request.experiment_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        if let Some(existing) = existing {
            return if existing == request_sha256 {
                Ok(existing)
            } else {
                Err("learning experiment id conflicts with prior request".into())
            };
        }
        for target in &request.targets {
            let (content, stored): (String, Option<String>) = tx
                .query_row(
                    "SELECT content,content_hash FROM memories WHERE id=?1",
                    [&target.memory_id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .map_err(|_| format!("learning target missing: {}", target.memory_id))?;
            if stored.unwrap_or_else(|| content_hash(&content)) != target.content_sha256 {
                return Err(format!(
                    "learning target hash mismatch: {}",
                    target.memory_id
                ));
            }
        }
        tx.execute(
            "INSERT INTO learning_experiment VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            rusqlite::params![
                request.experiment_id,
                request_sha256,
                request.policy_version,
                request.policy_activation_sha256,
                request.task_class,
                request.observed_since,
                request.observed_through,
                request.min_per_cohort,
                request.min_effect_bps,
                crate::time::now_iso()
            ],
        )
        .map_err(|e| e.to_string())?;
        for (ordinal, target) in request.targets.iter().enumerate() {
            tx.execute(
                "INSERT INTO learning_update_target VALUES (?1,?2,?3,?4,?5)",
                rusqlite::params![
                    request.experiment_id,
                    ordinal as i64,
                    target.memory_id,
                    target.content_sha256,
                    target.delta_micros
                ],
            )
            .map_err(|e| e.to_string())?;
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok(request_sha256)
    }

    pub fn learning_promotion_receipt(
        &self,
        experiment_id: &str,
    ) -> Result<Option<LearningPromotionReceiptV1>, String> {
        self.db.lock().query_row(
            "SELECT evidence_sha256,control_count,control_success,candidate_count,candidate_success,effect_bps,disposition,reason_code,applied_count FROM learning_promotion_receipt WHERE experiment_id=?1",
            [experiment_id], |row| Ok(LearningPromotionReceiptV1 { schema_version:1, experiment_id:experiment_id.into(), evidence_sha256:row.get(0)?, control_count:row.get::<_,i64>(1)? as u32, control_success:row.get::<_,i64>(2)? as u32, candidate_count:row.get::<_,i64>(3)? as u32, candidate_success:row.get::<_,i64>(4)? as u32, effect_bps:row.get::<_,i64>(5)? as i32, disposition:row.get(6)?, reason_code:row.get(7)?, applied_count:row.get::<_,i64>(8)? as u32 })).optional().map_err(|e| e.to_string())
    }

    /// Evaluate only persisted trusted evidence, then atomically apply the preregistered deltas.
    pub fn qualify_learning_experiment(
        &self,
        experiment_id: &str,
    ) -> Result<LearningPromotionReceiptV1, String> {
        if let Some(receipt) = self.learning_promotion_receipt(experiment_id)? {
            return Ok(receipt);
        }
        let (
            request_sha256,
            policy,
            activation,
            task,
            since,
            through,
            min_n,
            margin,
            created,
            targets,
        ) = {
            let conn = self.db.lock();
            let row = conn.query_row("SELECT request_sha256,policy_version,policy_activation_sha256,task_class,observed_since,observed_through,min_per_cohort,min_effect_bps,created_at FROM learning_experiment WHERE experiment_id=?1", [experiment_id], |r| Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?,r.get::<_,String>(4)?,r.get::<_,String>(5)?,r.get::<_,i64>(6)?,r.get::<_,i64>(7)?,r.get::<_,String>(8)?))).map_err(|_| "learning experiment not found".to_string())?;
            let targets = conn.prepare("SELECT memory_id,content_sha256,delta_micros FROM learning_update_target WHERE experiment_id=?1 ORDER BY ordinal").and_then(|mut s| s.query_map([experiment_id], |r| Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,i64>(2)?)))?.collect::<rusqlite::Result<Vec<_>>>()).map_err(|e| e.to_string())?;
            (
                row.0, row.1, row.2, row.3, row.4, row.5, row.6, row.7, row.8, targets,
            )
        };
        if through > crate::time::now_iso() {
            return Err("learning observation window is still open".into());
        }
        let artifact_ids = targets
            .iter()
            .flat_map(|(id, _, _)| {
                let digest = content_hash(id);
                [
                    stable_artifact_id(id),
                    format!("memory.{digest}"),
                    format!("artifact.{digest}"),
                ]
            })
            .collect::<HashSet<_>>();
        let mut evidence = vec![request_sha256];
        let mut candidate_traces = Vec::new();
        let mut counts = [0_i64; 4];
        if created <= since {
            let events = self.db.lock_events();
            let mut statement = events.prepare("SELECT v.event_id,v.canonical_sha256,v.trace_id,v.cohort,v.reason_code,o.event_id FROM context_event_log v JOIN context_event_link vl ON vl.event_id=v.event_id AND vl.relation='verdict_for' JOIN context_event_log o ON o.event_id=vl.target_event_id AND o.trace_id=v.trace_id AND o.phase='turn.outcome' AND o.status='success' AND o.provider IN ('live','audit') AND o.producer IN ('live','audit') AND o.traffic_class='production' AND o.ts>=?1 AND o.ts<=?2 AND o.policy_version=v.policy_version AND o.policy_activation_sha256=v.policy_activation_sha256 AND o.task_class=v.task_class AND o.cohort=v.cohort AND o.reason_code=v.reason_code WHERE v.phase='verdict.recorded' AND v.status='success' AND v.provider IN ('live','audit') AND v.producer IN ('live','audit') AND v.traffic_class='production' AND v.ts>=?1 AND v.ts<=?2 AND v.policy_version=?3 AND v.policy_activation_sha256=?4 AND v.task_class=?5 AND v.cohort IN ('control','candidate') AND EXISTS(SELECT 1 FROM context_event_log a WHERE a.trace_id=v.trace_id AND a.phase='policy.assigned' AND a.status='success' AND a.provider='policy' AND a.producer='policy' AND a.traffic_class='production' AND a.ts>=?1 AND a.ts<=?2 AND a.policy_version=v.policy_version AND a.policy_activation_sha256=v.policy_activation_sha256 AND a.cohort=v.cohort AND a.task_class=v.task_class) AND EXISTS(SELECT 1 FROM context_event_log x WHERE x.trace_id=v.trace_id AND x.phase='policy.exposed' AND x.status='success' AND x.provider='policy' AND x.producer='policy' AND x.traffic_class='production' AND x.ts>=?1 AND x.ts<=?2 AND x.policy_version=v.policy_version AND x.policy_activation_sha256=v.policy_activation_sha256 AND x.cohort=v.cohort AND x.task_class=v.task_class) ORDER BY v.event_id LIMIT 10001").map_err(|e| e.to_string())?;
            let rows = statement
                .query_map(
                    rusqlite::params![since, through, policy, activation, task],
                    |r| {
                        Ok((
                            r.get::<_, String>(0)?,
                            r.get::<_, String>(1)?,
                            r.get::<_, String>(2)?,
                            r.get::<_, String>(3)?,
                            r.get::<_, String>(4)?,
                            r.get::<_, String>(5)?,
                        ))
                    },
                )
                .map_err(|e| e.to_string())?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(|e| e.to_string())?;
            if rows.len() > 10_000 {
                return Err("learning evidence exceeds 10000 observations".into());
            }
            let mut seen = HashSet::new();
            for (event_id, canonical, trace, cohort, outcome, outcome_id) in rows {
                if !seen.insert(trace.clone()) {
                    return Err("learning evidence has duplicate verdict trace".into());
                }
                if cohort == "candidate" {
                    let mut chain = events.prepare("SELECT e.phase,e.event_id,e.canonical_sha256,e.artifact_id FROM context_event_log e WHERE e.trace_id=?1 AND e.phase IN ('candidate.retrieved','candidate.admitted','block.delivered','candidate.used') AND e.status='success' AND e.traffic_class='production' AND e.provider='live' AND e.producer='live' AND e.policy_version=?2 AND e.policy_activation_sha256=?3 AND e.task_class=?4 AND e.cohort=?5 AND e.ts>=?6 AND e.ts<=?7 ORDER BY e.event_id").map_err(|e| e.to_string())?;
                    let chain = chain
                        .query_map(
                            rusqlite::params![
                                &trace, policy, activation, task, cohort, since, through
                            ],
                            |r| {
                                Ok((
                                    r.get::<_, String>(0)?,
                                    r.get::<_, String>(1)?,
                                    r.get::<_, String>(2)?,
                                    r.get::<_, Option<String>>(3)?,
                                ))
                            },
                        )
                        .map_err(|e| e.to_string())?
                        .collect::<rusqlite::Result<Vec<_>>>()
                        .map_err(|e| e.to_string())?;
                    let complete = artifact_ids.iter().any(|artifact| {
                        let id = |phase| chain.iter().find(|row| row.0 == phase && row.3.as_deref() == Some(artifact)).map(|row| row.1.as_str());
                        let Some((retrieved,admitted,delivered,used)) = id("candidate.retrieved").zip(id("candidate.admitted")).zip(id("block.delivered").zip(id("candidate.used"))).map(|((a,b),(c,d))|(a,b,c,d)) else { return false; };
                        let linked = |source:&str, relation:&str, target:&str| events.query_row("SELECT EXISTS(SELECT 1 FROM context_event_link WHERE event_id=?1 AND relation=?2 AND target_event_id=?3)",rusqlite::params![source,relation,target],|r|r.get::<_,bool>(0)).unwrap_or(false);
                        linked(admitted,"candidate_of",retrieved) && linked(used,"observed_use_of",delivered) && linked(&outcome_id,"outcome_for",delivered)
                    });
                    if !complete {
                        continue;
                    }
                    candidate_traces.push(trace.clone());
                    counts[2] += 1;
                    counts[3] += (outcome == "task_succeeded") as i64;
                    for row in chain {
                        evidence.push(format!("{}:{}", row.1, row.2));
                    }
                } else {
                    counts[0] += 1;
                    counts[1] += (outcome == "task_succeeded") as i64;
                }
                evidence.push(format!("{event_id}:{canonical}"));
            }
        }
        evidence.sort();
        let evidence_sha256 = content_hash(&evidence.join("\n"));
        let effect = if counts[0] > 0 && counts[2] > 0 {
            (((counts[3] * counts[0] - counts[1] * counts[2]) * 10_000) / (counts[0] * counts[2]))
                as i32
        } else {
            0
        };
        let (disposition, reason) = if created > since {
            ("rejected", "experiment_not_preregistered")
        } else if counts[0] < min_n || counts[2] < min_n {
            ("rejected", "insufficient_cohort_evidence")
        } else if effect < margin as i32 {
            ("rejected", "controlled_margin_not_met")
        } else {
            ("qualified", "controlled_comparison_passed")
        };
        let applied = if disposition == "qualified" {
            targets.len() as u32
        } else {
            0
        };
        let receipt = LearningPromotionReceiptV1 {
            schema_version: 1,
            experiment_id: experiment_id.into(),
            evidence_sha256,
            control_count: counts[0] as u32,
            control_success: counts[1] as u32,
            candidate_count: counts[2] as u32,
            candidate_success: counts[3] as u32,
            effect_bps: effect,
            disposition: disposition.into(),
            reason_code: reason.into(),
            applied_count: applied,
        };
        let mut conn = self.db.lock();
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        let mut new_scores = Vec::new();
        if disposition == "qualified" {
            for (id, expected, delta) in &targets {
                let (content, stored, score): (String, Option<String>, f64) = tx
                    .query_row(
                        "SELECT content,content_hash,score FROM memories WHERE id=?1",
                        [id],
                        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                    )
                    .map_err(|_| format!("learning target missing: {id}"))?;
                if stored.unwrap_or_else(|| content_hash(&content)) != *expected {
                    return Err(format!("learning target hash changed: {id}"));
                }
                let score = (score + (*delta as f64 / 1_000_000.0)).clamp(0.0, 1.0);
                tx.execute(
                    "UPDATE memories SET score=?2,updated_at=?3 WHERE id=?1",
                    rusqlite::params![id, score, crate::time::now_iso()],
                )
                .map_err(|e| e.to_string())?;
                new_scores.push((id.clone(), score));
            }
            for trace in &candidate_traces {
                tx.execute("UPDATE context_feedback SET qualified_experiment_id=?2 WHERE trace_id=?1 AND verified=1",rusqlite::params![trace,experiment_id]).map_err(|e|e.to_string())?;
            }
        }
        tx.execute(
            "INSERT INTO learning_promotion_receipt VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            rusqlite::params![
                experiment_id,
                receipt.evidence_sha256,
                counts[0],
                counts[1],
                counts[2],
                counts[3],
                effect,
                disposition,
                reason,
                applied,
                crate::time::now_iso()
            ],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        if !new_scores.is_empty() {
            let mut registry = self.registry.write().unwrap_or_else(|e| e.into_inner());
            for (id, score) in new_scores {
                if let Some(mut entry) = registry.remove(&id) {
                    entry.score = score;
                    registry.insert(entry);
                }
            }
        }
        Ok(receipt)
    }

    /// Token-savings report for `crypt metrics`. `None` if nothing has been recalled yet.
    pub fn recall_metrics(&self) -> Option<crate::memdb::RecallMetrics> {
        self.db.recall_metrics()
    }

    /// Record that a memory entry was used (increments access_count and persists it).
    pub fn record_use(&self, id: &str) -> Result<u32, String> {
        self.record_use_observed(id, &MemoryEventContext::default())
    }

    pub fn record_use_observed(
        &self,
        id: &str,
        context: &MemoryEventContext,
    ) -> Result<u32, String> {
        let scope_id = self
            .registry
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(id)
            .map(|entry| entry.scope_id.clone())
            .ok_or_else(|| format!("memory {id:?} not found"))?;
        let mut conn = self.db.lock();
        let tx = conn
            .transaction()
            .map_err(|e| self.persist_error(format!("memory use transaction failed: {e}")))?;
        let new_count = tx
            .query_row(
                "UPDATE memories SET access_count = access_count + 1 WHERE id = ?1
                 RETURNING access_count",
                rusqlite::params![id],
                |row| row.get::<_, i64>(0),
            )
            .map_err(|e| {
                self.persist_error(format!("memory access_count update failed for {id}: {e}"))
            })? as u32;
        log_memory_event(
            &self.operation_attribution,
            &tx,
            "get",
            Some(id),
            Some(&scope_id),
            context,
            1,
            None,
        )
        .map_err(|e| self.persist_error(format!("memory get event failed for {id}: {e}")))?;
        tx.commit()
            .map_err(|e| self.persist_error(format!("memory use commit failed: {e}")))?;
        drop(conn);
        self.flush_event_outbox()?;
        {
            let mut reg = self.registry.write().unwrap_or_else(|e| e.into_inner());
            let mut entry = reg.remove(id).ok_or_else(|| {
                self.persist_error(format!("memory {id:?} missing from registry"))
            })?;
            entry.access_count = new_count;
            reg.insert(entry);
        }
        self.clear_last_persist_error();
        Ok(new_count)
    }

    /// Live-usage feed for the dashboard: recent recalls + transforms, newest first.
    pub fn activity_json(&self, n: usize) -> serde_json::Value {
        let recalls: Vec<serde_json::Value> = self
            .db
            .recent_recalls(n)
            .into_iter()
            .map(|recall| {
                let candidates = serde_json::from_str::<serde_json::Value>(&recall.candidate_hits)
                    .unwrap_or_else(|_| serde_json::json!([]));
                let admitted = serde_json::from_str::<serde_json::Value>(&recall.admitted_hits)
                    .unwrap_or_else(|_| serde_json::json!([]));
                serde_json::json!({"ts": recall.ts, "scope": recall.scope,
                                   "query_chars": recall.query_chars,
                                   "hits": recall.hit_count,
                                   "injected_chars": recall.injected_chars,
                                   "traffic_class": recall.traffic_class,
                                   "candidate_hits": candidates,
                                   "admitted_hits": admitted})
            })
            .collect();
        let transforms: Vec<serde_json::Value> = self
            .db
            .recent_transforms(n)
            .into_iter()
            .map(|(ts, verb, before, after)| {
                serde_json::json!({"ts": ts, "verb": verb, "before": before, "after": after})
            })
            .collect();
        serde_json::json!({ "recalls": recalls, "transforms": transforms })
    }

    /// The active embedder's dimension — 768 = EmbeddingGemma, 384 = BGE, 256 = the degraded hash
    /// embedder. Surfaced in `/health` so a mis-built (feature-less) or mis-configured binary is
    /// visible at a glance instead of silently writing wrong-space vectors.
    pub fn embedder_dim(&self) -> usize {
        self.embedder.dim()
    }

    /// Read-only accessor for the underlying DB. Lane B (memory provider) uses this from
    /// integration tests to set up fixtures (e.g. a row with a low score so the demotion gate
    /// can be exercised). The provider itself does NOT use this accessor — it reads through
    /// the higher-level public methods (`entries`, `scopes`).
    pub fn db(&self) -> &crate::memdb::MemDb {
        &self.db
    }

    /// Apply one typed lifecycle event as one SQLite transaction. Replaying the
    /// same event is a no-op; a reused id with different content fails closed.
    pub fn apply_lifecycle_event(
        &self,
        event: &MemoryLifecycleEventV1,
    ) -> Result<(), MemoryLifecycleError> {
        let required = [
            ("event_id", event.event_id.as_str()),
            ("subject_id", event.subject_id.as_str()),
            ("replacement_id", event.replacement_id.as_str()),
            ("scope_id", event.scope_id.as_str()),
            ("actor", event.actor.as_str()),
            ("authority", event.authority.as_str()),
            ("reason_ref", event.reason_ref.as_str()),
            ("origin_event_uid", event.origin_event_uid.as_str()),
        ];
        if let Some((field, _)) = required.iter().find(|(_, value)| value.trim().is_empty()) {
            return Err(MemoryLifecycleError::Invalid(format!(
                "{field} is required"
            )));
        }
        if event.effective_at_ms < 0 {
            return Err(MemoryLifecycleError::Invalid(
                "effective_at_ms must be an integer Unix timestamp in milliseconds".into(),
            ));
        }
        if event.subject_id == event.replacement_id {
            return Err(MemoryLifecycleError::SelfSupersession);
        }

        let meta = serde_json::to_string(event)
            .map_err(|error| MemoryLifecycleError::Invalid(format!("serialize event: {error}")))?;
        let mut conn = self.db.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;

        let existing: Option<(String, Option<String>, Option<String>, String)> = tx
            .query_row(
                "SELECT event_kind, memory_id, scope_id, meta
                 FROM memory_event_log WHERE event_uid=?1",
                rusqlite::params![&event.event_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()?;
        if let Some((kind, memory_id, scope_id, existing_meta)) = existing {
            if kind == "superseded"
                && memory_id.as_deref() == Some(event.subject_id.as_str())
                && scope_id.as_deref() == Some(event.scope_id.as_str())
                && existing_meta == meta
            {
                return Ok(());
            }
            return Err(MemoryLifecycleError::EventConflict(event.event_id.clone()));
        }

        let subject: Option<(String, String, Option<String>)> = tx
            .query_row(
                "SELECT scope_id, lifecycle_state, superseded_by FROM memories WHERE id=?1",
                rusqlite::params![&event.subject_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        let Some((subject_scope, subject_state, subject_successor)) = subject else {
            return Err(MemoryLifecycleError::MissingSubject(
                event.subject_id.clone(),
            ));
        };
        let replacement_scope: Option<String> = tx
            .query_row(
                "SELECT scope_id FROM memories WHERE id=?1",
                rusqlite::params![&event.replacement_id],
                |row| row.get(0),
            )
            .optional()?;
        let Some(replacement_scope) = replacement_scope else {
            return Err(MemoryLifecycleError::MissingReplacement(
                event.replacement_id.clone(),
            ));
        };
        if subject_scope != event.scope_id || replacement_scope != event.scope_id {
            return Err(MemoryLifecycleError::CrossScope);
        }
        if subject_state != "active" || subject_successor.is_some() {
            return Err(MemoryLifecycleError::SubjectNotActive);
        }

        let mut cursor = event.replacement_id.clone();
        let mut seen = HashSet::new();
        loop {
            if !seen.insert(cursor.clone()) || cursor == event.subject_id {
                return Err(MemoryLifecycleError::Cycle);
            }
            let next: Option<Option<String>> = tx
                .query_row(
                    "SELECT superseded_by FROM memories WHERE id=?1",
                    rusqlite::params![&cursor],
                    |row| row.get(0),
                )
                .optional()?;
            match next {
                Some(Some(next)) => cursor = next,
                Some(None) => break,
                None => {
                    return Err(MemoryLifecycleError::Invalid(
                        "replacement chain contains a missing memory".into(),
                    ))
                }
            }
        }

        let updated = tx.execute(
            "UPDATE memories
             SET lifecycle_state='superseded', superseded_by=?1, effective_until_ms=?2
             WHERE id=?3 AND lifecycle_state='active' AND superseded_by IS NULL",
            rusqlite::params![
                &event.replacement_id,
                event.effective_at_ms,
                &event.subject_id,
            ],
        )?;
        if updated != 1 {
            return Err(MemoryLifecycleError::SubjectNotActive);
        }
        tx.execute(
            "INSERT INTO memory_event_log
             (ts, event_kind, memory_id, surface, scope_id, quantity, meta, event_uid,
              installation_id, client, artifact_id, identity_status)
             VALUES (?1, 'superseded', ?2, ?3, ?4, 1, ?5, ?6, ?7, ?8, ?9, 'observed')",
            rusqlite::params![
                crate::time::now_iso(),
                &event.subject_id,
                &event.actor,
                &event.scope_id,
                meta,
                &event.event_id,
                &self.operation_attribution.installation_id,
                &event.actor,
                stable_artifact_id(&event.subject_id),
            ],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Execute an explicit lifecycle verb using a verified execution identity.
    /// Request descriptors intentionally contain no actor or authority fields.
    pub fn execute_lifecycle_operation(
        &self,
        request: &MemoryLifecycleOperationV1,
        actor: &VerifiedMemoryActor,
    ) -> Result<MemoryLifecycleReceiptV1, String> {
        if request.memory_id.trim().is_empty() || request.reason_ref.trim().is_empty() {
            return Err("memory_id and reason_ref are required".into());
        }
        if request
            .scope_id
            .as_deref()
            .is_some_and(|scope| scope.trim().is_empty())
        {
            return Err("scope_id cannot be empty".into());
        }
        let mut context = MemoryEventContext::new(&actor.origin)
            .with_session(&actor.session_id)
            .with_trace(&actor.trace_id);
        context.turn_id = Some(actor.trace_id.clone());
        let scope = request.scope_id.as_deref().unwrap_or("global");
        let expected = request.expected_content_sha256.as_deref();
        if let Some(expected) = expected {
            let actual: Option<String> = self
                .db
                .lock()
                .query_row(
                    "SELECT content_hash FROM memories WHERE id=?1",
                    rusqlite::params![&request.memory_id],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|e| e.to_string())?;
            if actual.as_deref() != Some(expected) {
                return Err("lifecycle hash guard failed".into());
            }
        }
        let (status, reversible) = match request.operation {
            MemoryLifecycleOperation::Supersede => {
                let replacement = request
                    .replacement_id
                    .as_deref()
                    .ok_or_else(|| "replacement_id is required for supersede".to_string())?;
                let event = MemoryLifecycleEventV1::superseded(
                    format!("lifecycle:{}:{}", actor.trace_id, request.memory_id),
                    &request.memory_id,
                    replacement,
                    scope,
                    crate::time::now_millis() as i64,
                    &actor.actor,
                    &actor.authority,
                    &request.reason_ref,
                    &actor.trace_id,
                );
                self.apply_lifecycle_event(&event)
                    .map_err(|e| e.to_string())?;
                ("superseded", true)
            }
            MemoryLifecycleOperation::Forget => {
                self.try_delete_observed(&request.memory_id, &context)?;
                ("forgotten", false)
            }
            MemoryLifecycleOperation::Restore => {
                if self.restore_quarantined(&request.memory_id)? {
                    ("restored", true)
                } else {
                    return Err("memory is not quarantined".into());
                }
            }
            MemoryLifecycleOperation::Review | MemoryLifecycleOperation::Explain => {
                if self
                    .db
                    .lock()
                    .query_row(
                        "SELECT 1 FROM memories WHERE id=?1 UNION ALL SELECT 1 FROM memory_quarantine WHERE id=?1 LIMIT 1",
                        rusqlite::params![&request.memory_id],
                        |_| Ok(()),
                    )
                    .optional()
                    .map_err(|e| e.to_string())?
                    .is_none()
                {
                    return Err(format!("memory {:?} not found", request.memory_id));
                }
                (
                    if matches!(request.operation, MemoryLifecycleOperation::Explain) {
                        "explained"
                    } else {
                        "reviewed"
                    },
                    true,
                )
            }
            MemoryLifecycleOperation::Consolidate => {
                if !matches!(actor.authority.as_str(), "A1" | "A2") {
                    return Err("consolidation requires A1 or A2 authority".into());
                }
                ("consolidation_requested", true)
            }
            MemoryLifecycleOperation::Create | MemoryLifecycleOperation::Update => {
                let content = request
                    .content
                    .as_deref()
                    .ok_or_else(|| "content is required for create/update".to_string())?;
                let scope_id = request.scope_id.as_deref().unwrap_or("global");
                let tier = request.tier.unwrap_or(MemoryTier::Semantic);
                let id = request
                    .memory_id
                    .rsplit('/')
                    .next()
                    .unwrap_or(&request.memory_id);
                self.try_put_observed(id, content, scope_id, tier, &context)?;
                (
                    if matches!(request.operation, MemoryLifecycleOperation::Create) {
                        "created"
                    } else {
                        "updated"
                    },
                    true,
                )
            }
            MemoryLifecycleOperation::Quarantine => {
                let mut conn = self.db.lock();
                let tx = conn.transaction().map_err(|e| e.to_string())?;
                let exists: Option<String> = tx
                    .query_row(
                        "SELECT id FROM memories WHERE id=?1",
                        rusqlite::params![&request.memory_id],
                        |row| row.get(0),
                    )
                    .optional()
                    .map_err(|e| e.to_string())?;
                if exists.is_none() {
                    return Err(format!("memory {:?} not found", request.memory_id));
                }
                let now = crate::time::now_iso();
                tx.execute("INSERT INTO memory_quarantine (id,tier,content,keywords,score,created_at,updated_at,access_count,embedding,embedding_q,scope_id,inject_count,content_hash,embed_model,source_ids,authority,influence_class,lifecycle_state,effective_from_ms,effective_until_ms,expires_at_ms,review_after_ms,superseded_by,priority_class,confidence,confidence_basis,quarantined_at,reason) SELECT id,tier,content,keywords,score,created_at,updated_at,access_count,embedding,embedding_q,scope_id,inject_count,content_hash,embed_model,source_ids,authority,influence_class,lifecycle_state,effective_from_ms,effective_until_ms,expires_at_ms,review_after_ms,superseded_by,priority_class,confidence,confidence_basis,?2,?3 FROM memories WHERE id=?1 ON CONFLICT(id) DO UPDATE SET quarantined_at=excluded.quarantined_at,reason=excluded.reason", rusqlite::params![&request.memory_id, now, &request.reason_ref]).map_err(|e| e.to_string())?;
                tx.execute(
                    "DELETE FROM memories WHERE id=?1",
                    rusqlite::params![&request.memory_id],
                )
                .map_err(|e| e.to_string())?;
                tx.commit().map_err(|e| e.to_string())?;
                self.registry
                    .write()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(&request.memory_id);
                ("quarantined", true)
            }
        };
        Ok(MemoryLifecycleReceiptV1 {
            schema: "orthic.memory-lifecycle-receipt.v1",
            operation: request.operation.clone(),
            memory_id: request.memory_id.clone(),
            status: status.into(),
            reversible,
            actor: actor.actor.clone(),
            authority: actor.authority.clone(),
            origin: actor.origin.clone(),
            session_id: actor.session_id.clone(),
            trace_id: actor.trace_id.clone(),
            reason_ref: request.reason_ref.clone(),
        })
    }

    /// Protect retention for an approved memory. This never changes recall eligibility;
    /// optional rank treatment remains post-gate and policy-flagged.
    pub fn set_priority_class(
        &self,
        memory_id: &str,
        priority_class: &str,
        actor: &str,
        authority: &str,
        reason_ref: &str,
    ) -> Result<(), MemoryPriorityError> {
        if !matches!(priority_class, "normal" | "protected") {
            return Err(MemoryPriorityError::InvalidClass(priority_class.into()));
        }
        let authorized = actor == "human"
            || actor == "admin"
            || (actor == "adapt_manifest" && matches!(authority, "A1" | "A2"));
        if !authorized {
            return Err(MemoryPriorityError::Unauthorized(
                actor.into(),
                authority.into(),
            ));
        }
        if reason_ref.trim().is_empty() {
            return Err(MemoryPriorityError::MissingReason);
        }
        let mut conn = self.db.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let changed = tx.execute(
            "UPDATE memories SET priority_class=?1 WHERE id=?2",
            rusqlite::params![priority_class, memory_id],
        )?;
        if changed != 1 {
            return Err(MemoryPriorityError::Missing(memory_id.into()));
        }
        tx.execute(
            "INSERT INTO memory_event_log
             (ts, event_kind, memory_id, surface, scope_id, quantity, meta, installation_id, client, artifact_id, identity_status)
             SELECT ?1, ?2, id, ?3, scope_id, 1, ?4, ?5, ?3, ?6, 'observed'
             FROM memories WHERE id=?7",
            rusqlite::params![
                crate::time::now_iso(),
                format!("priority_{priority_class}"),
                actor,
                serde_json::json!({"authority": authority, "reason_ref": reason_ref}).to_string(),
                &self.operation_attribution.installation_id,
                stable_artifact_id(memory_id),
                memory_id,
            ],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn health(&self) -> MemoryHealth {
        let last_persist_error = self
            .last_persist_error
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        MemoryHealth {
            ok: self.writes_enabled && last_persist_error.is_none(),
            embedder_dim: self.embedder.dim(),
            embedder_model: self.embedder.model_id(),
            embedder_fingerprint: self.embedder.pipeline_fingerprint().label,
            embedder_issue: self.embedder_issue.clone(),
            writes_enabled: self.writes_enabled,
            last_persist_error,
        }
    }

    pub fn health_json(&self) -> serde_json::Value {
        let h = self.health();
        let query_embedding_cache = self
            .query_embedding_cache
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .snapshot();
        serde_json::json!({
            "ok": h.ok,
            "embedder_dim": h.embedder_dim,
            "embedder_model": h.embedder_model,
            "embedder_fingerprint": h.embedder_fingerprint,
            "embedder_issue": h.embedder_issue,
            "writes_enabled": h.writes_enabled,
            "last_persist_error": h.last_persist_error,
            "query_embedding_cache": query_embedding_cache,
        })
    }

    /// Detailed service health adds a nonblocking, point-in-time probe of the live SQLite handle.
    /// This is separate from `health_json` so ordinary in-process status reads retain their legacy
    /// behavior and cannot fail merely because another operation briefly owns the one connection.
    pub(crate) fn detailed_health_json(&self) -> serde_json::Value {
        let mut health = self.health_json();
        let database = self.db.health_probe();
        // A structurally reachable database can still be semantically empty (e.g. the service
        // opened the wrong file, or the corpus was lost). Zero rows in a database that exists is
        // degraded, not ok — surface the count and a non-"ok" status so an empty corpus cannot
        // read as healthy, without overloading the probe's busy/error taxonomy.
        let memory_count: Option<i64> = if database == crate::memdb::MemDbProbe::Ok {
            self.db
                .lock()
                .query_row("SELECT COUNT(*) FROM memories", [], |row| row.get(0))
                .ok()
        } else {
            None
        };
        let database_status = match (database, memory_count) {
            (crate::memdb::MemDbProbe::Ok, Some(0)) => "empty",
            _ => database.status(),
        };
        health["database"] = serde_json::json!({
            "status": database_status,
            "memoryCount": memory_count,
        });
        if database != crate::memdb::MemDbProbe::Ok {
            health["ok"] = serde_json::Value::Bool(false);
        }
        health
    }

    fn persist_error(&self, err: String) -> String {
        self.set_last_persist_error(err.clone());
        err
    }

    fn flush_event_outbox(&self) -> Result<(), String> {
        self.db
            .flush_context_event_outbox()
            .map(|_| ())
            .map_err(|error| self.persist_error(format!("membrane event replay failed: {error}")))
    }

    fn set_last_persist_error(&self, err: String) {
        *self
            .last_persist_error
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(err);
    }

    fn clear_last_persist_error(&self) {
        *self
            .last_persist_error
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = None;
    }

    /// The metrics report (recall savings + per-verb transforms + curate counts) as JSON —
    /// shared by `crypt metrics` and the serve `GET /metrics` route / dashboard.
    pub fn metrics_json(&self) -> serde_json::Value {
        let mut out = match self.recall_metrics() {
            None => serde_json::json!({"recalls": 0, "note": "no recalls logged yet"}),
            Some(m) => serde_json::json!({
                "recalls": m.recalls, "hits": m.hits, "full_chars": m.full_chars,
                "injected_chars": m.injected_chars,
                "potential_chars_saved": m.chars_saved(),
                "potential_tokens_saved_est": m.tokens_saved(),
                // Compatibility aliases. These are potential preview-vs-full savings, not a
                // measured counterfactual of what the downstream model would have consumed.
                "chars_saved": m.chars_saved(),
                "tokens_saved_est": m.tokens_saved(),
                "since": m.first_ts, "through": m.last_ts,
            }),
        };
        let mut transforms = serde_json::Map::new();
        for t in self.transform_metrics() {
            if t.verb == "curate" {
                out["curate"] = serde_json::json!({
                    "runs": t.runs, "merged": t.before_chars, "pruned": t.after_chars,
                });
            } else {
                transforms.insert(
                    t.verb.clone(),
                    serde_json::json!({
                        "runs": t.runs, "before_chars": t.before_chars,
                        "after_chars": t.after_chars, "chars_saved": t.chars_saved(),
                    }),
                );
            }
        }
        out["transforms"] = serde_json::Value::Object(transforms);
        // Effectiveness block (2026-07-05, §10.2 paired-metric protocol): the fetch-after-inject
        // rate is ADVISORY — a lower bound on usefulness, suppressed by preview sufficiency and
        // fetch friction. Kill/clear decisions pair it with the blind relevance spot-check.
        let (corpus, injected_distinct, fetched) = self.db.effectiveness_metrics();
        out["effectiveness"] = serde_json::json!({
            "corpus": corpus,
            "injected_distinct": injected_distinct,
            "fetched_after_inject": fetched,
            "rate": if injected_distinct > 0 {
                (fetched as f64 / injected_distinct as f64 * 1000.0).round() / 1000.0
            } else { 0.0 },
            "note": "rate is a LOWER BOUND (advisory) — pair with the relevance spot-check",
        });
        {
            let as_of_ms = crate::time::now_millis() as i64;
            let conn = self.db.lock();
            let total: i64 = conn
                .query_row("SELECT COUNT(*) FROM memories", [], |row| row.get(0))
                .unwrap_or(0);
            let state_count = |state: &str| -> i64 {
                conn.query_row(
                    "SELECT COUNT(*) FROM memories WHERE lifecycle_state=?1",
                    rusqlite::params![state],
                    |row| row.get(0),
                )
                .unwrap_or(0)
            };
            let future: i64 = conn.query_row("SELECT COUNT(*) FROM memories WHERE effective_from_ms IS NOT NULL AND effective_from_ms > ?1", rusqlite::params![as_of_ms], |row| row.get(0)).unwrap_or(0);
            let expired: i64 = conn.query_row("SELECT COUNT(*) FROM memories WHERE (effective_until_ms IS NOT NULL AND effective_until_ms <= ?1) OR (expires_at_ms IS NOT NULL AND expires_at_ms <= ?1)", rusqlite::params![as_of_ms], |row| row.get(0)).unwrap_or(0);
            let eligible = recall_eligible_ids_on(&conn, as_of_ms, false)
                .map(|rows| rows.len() as i64)
                .unwrap_or(0);
            out["lifecycle"] = serde_json::json!({
                "total": total, "eligible": eligible, "gated_out": total.saturating_sub(eligible),
                "active": state_count("active"), "future": future, "expired": expired,
                "superseded": state_count("superseded"), "retired": state_count("retired"),
                "invalidated": state_count("invalidated"), "draft": state_count("draft"),
            });
        }
        // Feedback rail (2026-07-15): per-candidate recall outcomes. `verified_*` counts drive
        // ranking (observed actions / cited verdicts); `advisory_total` is observability only.
        // `active_vetoes` = distinct candidates currently suppressed by a verified contradiction.
        {
            let conn = self.db.lock();
            let (used, contradicted, advisory, distinct): (i64, i64, i64, i64) = conn
                .query_row(
                    "SELECT
                        COALESCE(SUM(CASE WHEN verified=1 AND outcome='used' THEN 1 ELSE 0 END),0),
                        COALESCE(SUM(CASE WHEN verified=1 AND outcome='contradicted' THEN 1 ELSE 0 END),0),
                        COALESCE(SUM(CASE WHEN verified=0 THEN 1 ELSE 0 END),0),
                        COUNT(DISTINCT candidate_id)
                     FROM context_feedback",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )
                .unwrap_or((0, 0, 0, 0));
            let active_vetoes: i64 = conn
                .query_row(
                    "SELECT COUNT(DISTINCT f.candidate_id) \
                     FROM context_feedback f JOIN memories m ON m.id=f.candidate_id \
                     WHERE f.verified=1 AND f.outcome='contradicted' \
                       AND f.content_sha256=COALESCE(m.content_hash, '')",
                    [],
                    |r| r.get(0),
                )
                .unwrap_or(0);
            drop(conn);
            out["feedback"] = serde_json::json!({
                "verified_used": used,
                "verified_contradicted": contradicted,
                "advisory_total": advisory,
                "active_vetoes": active_vetoes,
                "distinct_candidates_with_feedback": distinct,
            });
        }
        // Governance + graph delivery telemetry. Contextual enrichment is intentionally explicit:
        // the 2026-07-16 production-corpus gate exceeded its bounded runtime, so coverage stays zero
        // until a future implementation clears the frozen promotion gate.
        {
            let conn = self.db.lock();
            let quarantine_count: i64 = conn
                .query_row("SELECT COUNT(*) FROM memory_quarantine", [], |r| r.get(0))
                .unwrap_or(0);
            let link_edges: i64 = conn
                .query_row("SELECT COUNT(*) FROM links", [], |r| r.get(0))
                .unwrap_or(0);
            let dst_slugs = conn
                .prepare("SELECT DISTINCT dst_slug FROM links ORDER BY dst_slug LIMIT 10000")
                .and_then(|mut statement| {
                    statement
                        .query_map([], |row| row.get::<_, String>(0))?
                        .collect::<rusqlite::Result<Vec<_>>>()
                })
                .unwrap_or_default();
            let recent_admitted = conn
                .prepare(
                    "SELECT admitted_hits FROM recall_log \
                     WHERE source='serve' AND query_chars>0 \
                       AND traffic_class='production' ORDER BY id DESC LIMIT 1000",
                )
                .and_then(|mut statement| {
                    statement
                        .query_map([], |row| row.get::<_, String>(0))?
                        .collect::<rusqlite::Result<Vec<_>>>()
                })
                .unwrap_or_default();
            drop(conn);
            let active_ids = self
                .registry
                .read()
                .unwrap_or_else(|error| error.into_inner())
                .all()
                .iter()
                .map(|entry| entry.id.clone())
                .collect::<std::collections::HashSet<_>>();
            let active_leaf_slugs = active_ids
                .iter()
                .filter_map(|id| id.rsplit('/').next())
                .collect::<std::collections::HashSet<_>>();
            let resolvable_link_targets = dst_slugs
                .iter()
                .filter(|slug| {
                    active_ids.contains(*slug) || active_leaf_slugs.contains(slug.as_str())
                })
                .count() as i64;
            let mut link_inclusions = 0u64;
            let mut recalls_with_links = 0u64;
            for encoded in recent_admitted {
                let count = serde_json::from_str::<serde_json::Value>(&encoded)
                    .ok()
                    .and_then(|value| value.as_array().cloned())
                    .map(|hits| {
                        hits.iter()
                            .filter(|hit| {
                                hit.get("origin").and_then(|value| value.as_str()) == Some("link")
                            })
                            .count() as u64
                    })
                    .unwrap_or(0);
                if count > 0 {
                    recalls_with_links += 1;
                    link_inclusions += count;
                }
            }
            out["governance"] = serde_json::json!({
                "quarantined": quarantine_count,
                "contextual_enrichment": {
                    "status": "not_promoted",
                    "coverage": 0,
                    "reason": "production_corpus_eval_exceeded_runtime_bound"
                }
            });
            out["graph_recall"] = serde_json::json!({
                "edges": link_edges,
                "resolvable_targets": resolvable_link_targets,
                "link_inclusions": link_inclusions,
                "recalls_with_links": recalls_with_links,
                "delivery_window_recalls": 1000,
                "depth": 1,
                "lane_fraction_max": 0.2,
                "lane_slots_max": 8,
                "multi_hop": "not_promoted"
            });
        }
        // 2026-07-09 (add-now plan, Phase 1): cross-client attribution. Lets a one-line read
        // answer "which clients used recall this session" without scraping raw rows.
        let client_counts = self.db.client_recall_counts();
        let visibility_counts: std::collections::BTreeMap<String, u64> = {
            let conn = self.db.lock();
            conn.prepare(
                "SELECT client_visibility, COUNT(*) FROM recall_log \
                 WHERE source = 'serve' AND query_chars > 0 \
                   AND traffic_class = 'production' GROUP BY client_visibility",
            )
            .and_then(|mut st| {
                st.query_map([], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as u64))
                })
                .map(|rows| rows.flatten().collect())
            })
            .unwrap_or_default()
        };
        let aq = self.db.attribution_quality();
        out["attribution"] = serde_json::json!({
            "client_counts": client_counts
                .into_iter()
                .map(|(c, n)| serde_json::json!({"client": c, "count": n}))
                .collect::<Vec<_>>(),
            "visibility_counts": visibility_counts
                .into_iter()
                .map(|(v, n)| serde_json::json!({"visibility": v, "count": n}))
                .collect::<Vec<_>>(),
            "quality": {
                "attributed_meaningful": aq.attributed_meaningful,
                "unknown_total_raw": aq.unknown_total_raw,
                "unknown_legacy_no_preview": aq.unknown_legacy_no_preview,
                "unknown_empty_query": aq.unknown_empty_query,
                "unknown_current_meaningful": aq.unknown_current_meaningful,
                "unknown_current_no_metadata": aq.unknown_current_no_metadata,
                "unknown_first_ts": aq.unknown_first_ts,
                "unknown_last_ts": aq.unknown_last_ts,
                "note": "primary recall/client metrics exclude source!=serve and query_chars=0; unknown_legacy_no_preview rows predate attribution/query_preview"
            }
        });
        out
    }

    fn replay_memory_batch_on(
        conn: &rusqlite::Connection,
        batch_id: &str,
        request_sha256: &str,
    ) -> Result<Option<MemoryBatchReceipt>, MemoryBatchError> {
        let existing: Option<(String, i64)> = conn
            .query_row(
                "SELECT request_sha256, item_count FROM memory_batch_receipt WHERE batch_id=?1",
                rusqlite::params![batch_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(|e| MemoryBatchError::Persist(e.to_string()))?;
        let Some((existing_sha256, item_count)) = existing else {
            return Ok(None);
        };
        if existing_sha256 != request_sha256 {
            return Err(MemoryBatchError::Conflict);
        }
        let mut statement = conn
            .prepare(
                "SELECT item_id, memory_id, artifact_family, producer, record_type
                   FROM memory_batch_item_receipt WHERE batch_id=?1 ORDER BY ordinal",
            )
            .map_err(|e| MemoryBatchError::Persist(e.to_string()))?;
        let receipts: Vec<MemoryBatchItemReceipt> = statement
            .query_map(rusqlite::params![batch_id], |row| {
                Ok(MemoryBatchItemReceipt {
                    item_id: row.get(0)?,
                    memory_id: row.get(1)?,
                    status: "duplicate".into(),
                    artifact_family: row.get(2)?,
                    producer: row.get(3)?,
                    record_type: row.get(4)?,
                })
            })
            .map_err(|e| MemoryBatchError::Persist(e.to_string()))?
            .collect::<rusqlite::Result<_>>()
            .map_err(|e| MemoryBatchError::Persist(e.to_string()))?;
        if item_count < 0 || receipts.len() != item_count as usize {
            return Err(MemoryBatchError::Persist(
                "stored batch receipt is incomplete".into(),
            ));
        }
        Ok(Some(MemoryBatchReceipt {
            batch_id: batch_id.to_string(),
            inserted: 0,
            duplicates: receipts.len(),
            complete: true,
            receipts,
        }))
    }

    pub fn try_put_batch(
        &self,
        request: &MemoryBatchRequest,
    ) -> Result<MemoryBatchReceipt, MemoryBatchError> {
        if !self.writes_enabled {
            return Err(MemoryBatchError::Persist(
                self.embedder_issue.clone().unwrap_or_else(|| {
                    "memory writes disabled because the configured embedder is unavailable".into()
                }),
            ));
        }
        if !valid_opaque_id(&request.batch_id, 128) {
            return Err(MemoryBatchError::Invalid("invalid batch_id".into()));
        }
        if request.items.is_empty() || request.items.len() > MAX_MEMORY_BATCH_ITEMS {
            return Err(MemoryBatchError::Invalid(format!(
                "items must contain 1..={MAX_MEMORY_BATCH_ITEMS} entries"
            )));
        }
        let mut item_ids = HashSet::new();
        let mut memory_ids = HashSet::new();
        for item in &request.items {
            if !valid_opaque_id(&item.item_id, 160) || !item_ids.insert(item.item_id.clone()) {
                return Err(MemoryBatchError::Invalid(
                    "item_id is invalid or duplicated".into(),
                ));
            }
            if !valid_opaque_id(&item.name, 160)
                || item.content.trim().is_empty()
                || item.content.chars().count() > MAX_MEMORY_BATCH_CONTENT_CHARS
            {
                return Err(MemoryBatchError::Invalid(
                    "name or content is invalid".into(),
                ));
            }
            let scope = crate::scope::normalize_scope(&item.scope);
            crate::scope::validate_write_scope(&item.scope).map_err(MemoryBatchError::Invalid)?;
            if !valid_opaque_id(&scope, 160) || !memory_ids.insert(format!("{scope}/{}", item.name))
            {
                return Err(MemoryBatchError::Invalid(
                    "scope/name memory identity is invalid or duplicated".into(),
                ));
            }
            memory_batch_tier(&item.tier)?;
            if !crate::context_telemetry::registered_artifact_family(&item.artifact_family)
                || !valid_registry_token(&item.producer)
                || !valid_registry_token(&item.record_type)
                || !valid_registry_token(&item.client)
                || !valid_opaque_id(&item.session_id, 160)
                || !valid_opaque_id(&item.trace_id, 160)
            {
                return Err(MemoryBatchError::Invalid(
                    "item attribution is invalid or incomplete".into(),
                ));
            }
            if item.source_ids.len() > 256
                || item
                    .source_ids
                    .iter()
                    .any(|value| !valid_opaque_id(value, 240))
                || item.source_ids.iter().collect::<HashSet<_>>().len() != item.source_ids.len()
            {
                return Err(MemoryBatchError::Invalid(
                    "item source_ids are invalid or duplicated".into(),
                ));
            }
            item.lifecycle
                .validate()
                .map_err(MemoryBatchError::Invalid)?;
        }
        let request_sha256 = memory_batch_request_hash(request)?;
        {
            let conn = self.db.lock();
            if let Some(receipt) =
                Self::replay_memory_batch_on(&conn, &request.batch_id, &request_sha256)?
            {
                return Ok(receipt);
            }
        }

        let contents: Vec<&str> = request
            .items
            .iter()
            .map(|item| item.content.as_str())
            .collect();
        let embeddings = self
            .embed_for_write_batch(&contents)
            .map_err(MemoryBatchError::Persist)?;
        let updated_at = crate::time::now_iso();
        let mut conn = self.db.lock();
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| MemoryBatchError::Persist(e.to_string()))?;
        if let Some(receipt) =
            Self::replay_memory_batch_on(&tx, &request.batch_id, &request_sha256)?
        {
            return Ok(receipt);
        }
        tx.execute(
            "INSERT INTO memory_batch_receipt (batch_id, request_sha256, item_count, created_at)
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![
                request.batch_id,
                request_sha256,
                request.items.len() as i64,
                updated_at,
            ],
        )
        .map_err(|e| MemoryBatchError::Persist(e.to_string()))?;

        let mut entries = Vec::with_capacity(request.items.len());
        let mut receipts = Vec::with_capacity(request.items.len());
        for (ordinal, (item, embedding)) in request.items.iter().zip(embeddings).enumerate() {
            let scope = crate::scope::normalize_scope(&item.scope);
            let memory_id = format!("{scope}/{}", item.name);
            let existing: Option<(i64, String)> = tx
                .query_row(
                    "SELECT access_count, created_at FROM memories WHERE id=?1",
                    rusqlite::params![memory_id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()
                .map_err(|e| MemoryBatchError::Persist(e.to_string()))?;
            let disposition = if existing.is_some() {
                "updated"
            } else {
                "inserted"
            };
            let entry = MemoryEntry {
                id: memory_id.clone(),
                tier: memory_batch_tier(&item.tier)?,
                embedding: Some(embedding),
                content: item.content.clone(),
                keywords: keywords_of(&format!("{} {}", item.name, item.content)),
                score: 0.6,
                created_at: existing
                    .as_ref()
                    .map(|(_, created_at)| created_at.clone())
                    .unwrap_or_else(|| updated_at.clone()),
                access_count: existing.map(|(count, _)| count as u32).unwrap_or(0),
                scope_id: scope,
            };
            let metadata = MemoryRecordMetadata {
                artifact_family: item.artifact_family.clone(),
                producer: item.producer.clone(),
                record_type: item.record_type.clone(),
            };
            self.persist_entry_with_record_lifecycle_on(
                &tx,
                &entry,
                &updated_at,
                &item.source_ids,
                &metadata,
                &item.lifecycle,
            )
            .map_err(MemoryBatchError::Persist)?;
            let context = MemoryEventContext::new(&item.client)
                .with_session(&item.session_id)
                .with_trace(&item.trace_id);
            self.apply_lifecycle_input_on(&tx, &entry, &item.lifecycle, &context)
                .map_err(MemoryBatchError::Persist)?;
            log_memory_event(
                &self.operation_attribution,
                &tx,
                if disposition == "updated" {
                    "update"
                } else {
                    "put"
                },
                Some(&entry.id),
                Some(&entry.scope_id),
                &context,
                1,
                Some(&metadata),
            )
            .map_err(|e| MemoryBatchError::Persist(e.to_string()))?;
            let item_sha256 = memory_batch_item_hash(item)?;
            tx.execute(
                "INSERT INTO memory_batch_item_receipt
                     (batch_id, ordinal, item_id, memory_id, disposition, item_sha256,
                      artifact_family, producer, record_type)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                rusqlite::params![
                    request.batch_id,
                    ordinal as i64,
                    item.item_id,
                    memory_id,
                    disposition,
                    item_sha256,
                    item.artifact_family,
                    item.producer,
                    item.record_type,
                ],
            )
            .map_err(|e| MemoryBatchError::Persist(e.to_string()))?;
            receipts.push(MemoryBatchItemReceipt {
                item_id: item.item_id.clone(),
                memory_id,
                status: disposition.into(),
                artifact_family: item.artifact_family.clone(),
                producer: item.producer.clone(),
                record_type: item.record_type.clone(),
            });
            entries.push(entry);
        }
        tx.commit()
            .map_err(|e| MemoryBatchError::Persist(e.to_string()))?;
        drop(conn);
        self.flush_event_outbox()
            .map_err(MemoryBatchError::Persist)?;
        let mut registry = self.registry.write().unwrap_or_else(|e| e.into_inner());
        for entry in entries {
            registry.insert(entry);
        }
        drop(registry);
        self.clear_last_persist_error();
        Ok(MemoryBatchReceipt {
            batch_id: request.batch_id.clone(),
            inserted: receipts.len(),
            duplicates: 0,
            complete: true,
            receipts,
        })
    }

    /// DB-first write path: upsert a memory THROUGH the engine. Embeds immediately, persists,
    /// updates the in-RAM registry. Existing effectiveness counters survive the update.
    pub fn try_put(
        &self,
        name: &str,
        content: &str,
        scope: &str,
        tier: MemoryTier,
    ) -> Result<String, String> {
        self.try_put_observed(name, content, scope, tier, &MemoryEventContext::default())
    }

    pub fn try_put_observed(
        &self,
        name: &str,
        content: &str,
        scope: &str,
        tier: MemoryTier,
        context: &MemoryEventContext,
    ) -> Result<String, String> {
        self.try_put_with_metadata_observed(
            name,
            content,
            scope,
            tier,
            &crate::time::now_iso(),
            &[],
            context,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn try_put_attributed_observed(
        &self,
        name: &str,
        content: &str,
        scope: &str,
        tier: MemoryTier,
        artifact_family: &str,
        producer: &str,
        record_type: &str,
        context: &MemoryEventContext,
    ) -> Result<String, String> {
        self.try_put_attributed_lifecycle_observed(
            name,
            content,
            scope,
            tier,
            artifact_family,
            producer,
            record_type,
            context,
            &MemoryLifecycleInputV1::default(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn try_put_attributed_lifecycle_observed(
        &self,
        name: &str,
        content: &str,
        scope: &str,
        tier: MemoryTier,
        artifact_family: &str,
        producer: &str,
        record_type: &str,
        context: &MemoryEventContext,
        lifecycle: &MemoryLifecycleInputV1,
    ) -> Result<String, String> {
        if !crate::context_telemetry::registered_artifact_family(artifact_family)
            || !valid_registry_token(producer)
            || !valid_registry_token(record_type)
        {
            return Err("memory write attribution is invalid or unregistered".into());
        }
        self.try_put_with_record_metadata_observed(
            name,
            content,
            scope,
            tier,
            &crate::time::now_iso(),
            &[],
            Some(MemoryRecordMetadata {
                artifact_family: artifact_family.into(),
                producer: producer.into(),
                record_type: record_type.into(),
            }),
            context,
            lifecycle,
        )
    }

    /// Metadata-aware write used by sync/import paths. The caller-supplied update timestamp and
    /// consolidation provenance are stored verbatim for deterministic LWW/tombstone decisions.
    pub fn try_put_with_metadata(
        &self,
        name: &str,
        content: &str,
        scope: &str,
        tier: MemoryTier,
        updated_at: &str,
        source_ids: &[String],
    ) -> Result<String, String> {
        self.try_put_with_metadata_observed(
            name,
            content,
            scope,
            tier,
            updated_at,
            source_ids,
            &MemoryEventContext::default(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn try_put_with_metadata_observed(
        &self,
        name: &str,
        content: &str,
        scope: &str,
        tier: MemoryTier,
        updated_at: &str,
        source_ids: &[String],
        context: &MemoryEventContext,
    ) -> Result<String, String> {
        self.try_put_with_record_metadata_observed(
            name,
            content,
            scope,
            tier,
            updated_at,
            source_ids,
            None,
            context,
            &MemoryLifecycleInputV1::default(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn try_put_with_record_metadata_observed(
        &self,
        name: &str,
        content: &str,
        scope: &str,
        tier: MemoryTier,
        updated_at: &str,
        source_ids: &[String],
        supplied_metadata: Option<MemoryRecordMetadata>,
        context: &MemoryEventContext,
        lifecycle: &MemoryLifecycleInputV1,
    ) -> Result<String, String> {
        if updated_at.trim().is_empty() {
            return Err("updated_at is required".into());
        }
        let scope = crate::scope::normalize_scope(scope);
        let id = format!("{scope}/{name}");
        let existing = {
            let conn = self.db.lock();
            conn.query_row(
                "SELECT access_count, created_at, artifact_family, producer, record_type
                   FROM memories WHERE id = ?1",
                rusqlite::params![id],
                |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, String>(1)?,
                        MemoryRecordMetadata {
                            artifact_family: r.get(2)?,
                            producer: r.get(3)?,
                            record_type: r.get(4)?,
                        },
                    ))
                },
            )
            .optional()
            .map_err(|e| self.persist_error(format!("memory counter load failed for {id}: {e}")))?
        };
        let event_kind = if existing.is_some() { "update" } else { "put" };
        let access = existing.as_ref().map(|(count, _, _)| *count).unwrap_or(0);
        let created_at = existing
            .as_ref()
            .map(|(_, created_at, _)| created_at.clone())
            .unwrap_or_else(crate::time::now_iso);
        let metadata = supplied_metadata.unwrap_or_else(|| {
            existing
                .map(|(_, _, metadata)| metadata)
                .unwrap_or_default()
        });
        let entry = MemoryEntry {
            id: id.clone(),
            tier,
            embedding: Some(self.embed_for_write(content)?),
            content: content.to_string(),
            keywords: keywords_of(&format!("{name} {content}")),
            score: 0.6,
            created_at,
            access_count: access as u32,
            scope_id: scope,
        };
        let mut conn = self.db.lock();
        let tx = conn
            .transaction()
            .map_err(|e| self.persist_error(format!("memory transaction failed: {e}")))?;
        self.persist_entry_with_record_lifecycle_on(
            &tx, &entry, updated_at, source_ids, &metadata, lifecycle,
        )?;
        self.apply_lifecycle_input_on(&tx, &entry, lifecycle, context)?;
        log_memory_event(
            &self.operation_attribution,
            &tx,
            event_kind,
            Some(&entry.id),
            Some(&entry.scope_id),
            context,
            1,
            Some(&metadata),
        )
        .map_err(|e| self.persist_error(format!("memory {event_kind} event failed: {e}")))?;
        tx.commit()
            .map_err(|e| self.persist_error(format!("memory commit failed: {e}")))?;
        drop(conn);
        self.flush_event_outbox()?;
        self.registry
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert(entry);
        self.clear_last_persist_error();
        Ok(id)
    }

    /// Compatibility wrapper for older call sites. New service/CLI paths should use
    /// [`try_put`](Self::try_put) so write failures are visible to callers.
    pub fn put(&self, name: &str, content: &str, scope: &str, tier: MemoryTier) -> String {
        self.try_put(name, content, scope, tier)
            .unwrap_or_else(|err| {
                eprintln!("[memory] put failed: {err}");
                String::new()
            })
    }

    /// DB-first delete: removes the row and the in-RAM entry.
    pub fn try_delete(&self, id: &str) -> Result<bool, String> {
        self.try_delete_observed(id, &MemoryEventContext::default())
    }

    pub fn try_delete_observed(
        &self,
        id: &str,
        context: &MemoryEventContext,
    ) -> Result<bool, String> {
        // delete_entry removes from BOTH the DB and the registry — snapshot content+scope first so
        // we can record the deletion as an observed contradiction (the strongest "this recall was
        // wrong" signal) after the row is gone.
        let snapshot = self
            .registry
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(id)
            .map(|e| (e.content.clone(), e.scope_id.clone()));
        let existed = snapshot.is_some();
        self.delete_entry(id, context)?;
        if let Some((content, scope_id)) = snapshot {
            // Non-dark auto-caller: deleting a recalled memory is an observed contradiction.
            self.record_observed_feedback(id, &content, Outcome::Contradicted, &scope_id, context);
        }
        Ok(existed)
    }

    /// Compatibility wrapper for older call sites. New service/CLI paths should use
    /// [`try_delete`](Self::try_delete) so write failures are visible to callers.
    pub fn delete(&self, id: &str) -> bool {
        // delete_entry removes from BOTH the DB and the registry — check existence first.
        self.try_delete(id).unwrap_or_else(|err| {
            eprintln!("[memory] delete failed: {err}");
            false
        })
    }

    /// Fallible browse path for external callers: id, tier, content chars, access, inject per entry.
    pub fn try_list(
        &self,
        scope: Option<&str>,
    ) -> Result<Vec<(String, String, i64, i64, i64)>, String> {
        let conn = self.db.lock();
        let sql = "SELECT id, tier, length(content), access_count, inject_count FROM memories \
                   WHERE (?1 IS NULL OR scope_id = ?1) ORDER BY id";
        let mut statement = conn
            .prepare(sql)
            .map_err(|error| self.persist_error(format!("memory list prepare failed: {error}")))?;
        let rows = statement
            .query_map(rusqlite::params![scope], |row| {
                Ok((
                    row.get(0)?,
                    row.get::<_, String>(1)?.trim_matches('"').to_string(),
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            })
            .map_err(|error| self.persist_error(format!("memory list query failed: {error}")))?;
        let listed = rows
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|error| {
                self.persist_error(format!("memory list row decode failed: {error}"))
            })?;
        self.clear_last_persist_error();
        Ok(listed)
    }

    /// Compatibility wrapper for internal diagnostics. External HTTP/CLI paths use
    /// [`try_list`](Self::try_list) so provider failures cannot become false empty results.
    pub fn list(&self, scope: Option<&str>) -> Vec<(String, String, i64, i64, i64)> {
        self.try_list(scope).unwrap_or_else(|error| {
            eprintln!("[memory] list failed: {error}");
            Vec::new()
        })
    }

    /// Re-embed EVERY row from the DB's own content — no files involved. This is how an embedder
    /// swap (or a corpus that silently degraded to hash vectors) is repaired in place.
    pub fn reindex(&self) -> Result<(usize, usize), String> {
        if !self.writes_enabled {
            return Err(
                self.persist_error(self.embedder_issue.clone().unwrap_or_else(|| {
                    "reindex refused because memory writes are disabled".to_string()
                })),
            );
        }
        let rows: Vec<(String, String, Option<String>, Option<String>)> = {
            let conn = self.db.lock();
            conn.prepare("SELECT id, content, content_hash, embed_model FROM memories")
                .and_then(|mut st| {
                    st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
                        .and_then(|rows| rows.collect())
                })
                .map_err(|e| self.persist_error(format!("reindex row load failed: {e}")))?
        };
        let model = self.embedder.model_id();
        let (mut n, mut skipped) = (0usize, 0usize);
        let mut updates = Vec::new();
        for (id, content, hash, emb_model) in rows {
            let want = content_hash(&content);
            // Memoization: unchanged content already embedded by THIS model — skip.
            if hash.as_deref() == Some(want.as_str()) && emb_model.as_deref() == Some(model) {
                skipped += 1;
                continue;
            }
            let emb = self.embed_for_write(&content)?;
            let blob = embedding_to_quantized_blob(&emb);
            updates.push((id, emb, blob, want));
        }
        let mut conn = self.db.lock();
        let tx = conn
            .transaction()
            .map_err(|e| self.persist_error(format!("reindex transaction failed: {e}")))?;
        for (id, _emb, blob, want) in &updates {
            let changed = tx
                .execute(
                    "UPDATE memories SET embedding_q = ?1, embedding = NULL, content_hash = ?2, embed_model = ?3 WHERE id = ?4",
                    rusqlite::params![blob, want, model, id],
                )
                .map_err(|e| self.persist_error(format!("reindex update failed for {id}: {e}")))?;
            if changed != 1 {
                return Err(self.persist_error(format!("reindex target disappeared: {id}")));
            }
        }
        tx.commit()
            .map_err(|e| self.persist_error(format!("reindex commit failed: {e}")))?;
        drop(conn);
        let mut reg = self.registry.write().unwrap_or_else(|e| e.into_inner());
        for (id, emb, _, _) in updates {
            if let Some(entry) = reg.remove(&id) {
                reg.insert(MemoryEntry {
                    embedding: Some(emb),
                    ..entry
                });
            }
            n += 1;
        }
        self.clear_last_persist_error();
        Ok((n, skipped))
    }

    /// Write a deterministic, content-free-by-default vault review artifact.
    pub fn export_vault_review(
        &self,
        path: &Path,
        format: &str,
        include_content: bool,
    ) -> Result<usize, String> {
        let conn = self.db.lock();
        let mut statement = conn
            .prepare(
                "SELECT id,tier,scope_id,content_hash,lifecycle_state,effective_from_ms,\
                 effective_until_ms,expires_at_ms,review_after_ms,superseded_by,priority_class,\
                 confidence,confidence_basis,source_ids,artifact_family,producer,record_type,\
                 authority,influence_class,content FROM memories ORDER BY \
                 CASE WHEN review_after_ms IS NULL THEN 1 ELSE 0 END,review_after_ms,\
                 CASE priority_class WHEN 'protected' THEN 0 ELSE 1 END,id COLLATE BINARY",
            )
            .map_err(|error| error.to_string())?;
        let mut memories: Vec<serde_json::Value> = statement
            .query_map([], |row| {
                let sources: String = row.get(13)?;
                let mut item = serde_json::json!({
                    "id":row.get::<_,String>(0)?, "tier":row.get::<_,String>(1)?,
                    "scope":row.get::<_,String>(2)?, "contentHash":row.get::<_,Option<String>>(3)?,
                    "lifecycle":{"state":row.get::<_,String>(4)?,"effectiveFromMs":row.get::<_,Option<i64>>(5)?,"effectiveUntilMs":row.get::<_,Option<i64>>(6)?,"expiresAtMs":row.get::<_,Option<i64>>(7)?,"reviewAfterMs":row.get::<_,Option<i64>>(8)?,"supersededBy":row.get::<_,Option<String>>(9)?},
                    "priority":row.get::<_,String>(10)?, "confidence":row.get::<_,Option<f64>>(11)?,
                    "confidenceBasis":row.get::<_,Option<String>>(12)?,
                    "sourceRefs":serde_json::from_str::<serde_json::Value>(&sources).unwrap_or(serde_json::Value::Null),
                    "artifactFamily":row.get::<_,String>(14)?, "producer":row.get::<_,String>(15)?,
                    "recordType":row.get::<_,String>(16)?, "authority":row.get::<_,String>(17)?,
                    "influenceClass":row.get::<_,String>(18)?,
                });
                if include_content {
                    item["content"] = serde_json::Value::String(row.get(19)?);
                }
                Ok(item)
            })
            .map_err(|error| error.to_string())?
            .collect::<Result<_, _>>()
            .map_err(|error: rusqlite::Error| error.to_string())?;
        drop(statement);
        let links: Vec<(String, String)> = memories
            .iter()
            .filter_map(|item| {
                Some((
                    item["id"].as_str()?.into(),
                    item["lifecycle"]["supersededBy"].as_str()?.into(),
                ))
            })
            .collect();
        for (rank, item) in memories.iter_mut().enumerate() {
            let id = item["id"].as_str().unwrap_or_default().to_string();
            item["queueRank"] = serde_json::json!(rank + 1);
            item["supersedes"] = serde_json::json!(links
                .iter()
                .filter_map(|(old, new)| (new == &id).then_some(old))
                .collect::<Vec<_>>());
            let mut events = conn.prepare("SELECT ts,event_kind,surface,session_id,trace_id,event_uid,installation_id,client,turn_id,artifact_id,identity_status,meta FROM memory_event_log WHERE memory_id=?1 ORDER BY event_id").map_err(|e| e.to_string())?;
            item["events"] = serde_json::Value::Array(events.query_map([&id], |row| {
                let meta: String = row.get(11)?;
                let meta = serde_json::from_str::<serde_json::Value>(&meta).unwrap_or_default();
                Ok(serde_json::json!({"ts":row.get::<_,String>(0)?,"kind":row.get::<_,String>(1)?,"surface":row.get::<_,String>(2)?,"sessionId":row.get::<_,Option<String>>(3)?,"traceId":row.get::<_,Option<String>>(4)?,"eventUid":row.get::<_,Option<String>>(5)?,"installationId":row.get::<_,Option<String>>(6)?,"client":row.get::<_,Option<String>>(7)?,"turnId":row.get::<_,Option<String>>(8)?,"artifactId":row.get::<_,Option<String>>(9)?,"identityStatus":row.get::<_,String>(10)?,"authority":meta.get("authority"),"actor":meta.get("actor"),"reasonRef":meta.get("reason_ref"),"originEventUid":meta.get("origin_event_uid")}))
            }).map_err(|e| e.to_string())?.collect::<Result<_,_>>().map_err(|e: rusqlite::Error| e.to_string())?);
            let mut evidence = conn.prepare("SELECT trace_id,outcome,verified,verdict_ref,ts FROM context_feedback WHERE candidate_id=?1 ORDER BY ts,trace_id").map_err(|e| e.to_string())?;
            item["reviewEvidence"] = serde_json::Value::Array(evidence.query_map([&id], |row| Ok(serde_json::json!({"traceId":row.get::<_,String>(0)?,"outcome":row.get::<_,String>(1)?,"verified":row.get::<_,i64>(2)? != 0,"verdictRef":row.get::<_,Option<String>>(3)?,"ts":row.get::<_,String>(4)?}))).map_err(|e| e.to_string())?.collect::<Result<_,_>>().map_err(|e: rusqlite::Error| e.to_string())?);
        }
        drop(conn);
        let report = serde_json::json!({
            "schemaVersion":1, "kind":"crypt.vault-review", "contentIncluded":include_content,
            "ordering":"reviewAfterMs asc nulls-last, protected first, id bytewise",
            "sections":{"memories":{"status":"available","source":"memories"},"events":{"status":"available","source":"memory_event_log"},"reviewEvidence":{"status":"available","source":"context_feedback"},"approvals":{"status":"unavailable","reason":"authoritative_table_absent"},"contextPackRefs":{"status":"unavailable","reason":"authoritative_table_absent"}},
            "memories":memories
        });
        let bytes = match format {
            "json" => serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?,
            "markdown" | "md" => vault_review_markdown(&report).into_bytes(),
            _ => return Err("vault review format must be json or markdown".into()),
        };
        atomic_review_write(path, &bytes)?;
        Ok(report["memories"].as_array().map_or(0, Vec::len))
    }

    /// Export the whole store as a markdown tree — the audit trail + cross-machine sync medium.
    /// The DB stays authoritative; these files are generated FROM it (provenance DB->file).
    /// Scopes under the workspace root are canonicalized to `WS[-…]` so mirrors from different
    /// machines line up.
    pub fn export_md(&self, dir: &std::path::Path) -> usize {
        let ws_slug = crate::scope::workspace_root()
            .map(|root| crate::scope::path_to_scope(&root.to_string_lossy()));
        // Session checkpoints are short-lived orientation artifacts.  They remain
        // resident in the DB for the compaction handoff, but never enter the
        // Markdown mirror / cross-machine sync surface.
        let rows: Vec<(
            String,
            String,
            String,
            String,
            f64,
            String,
            Option<i64>,
            Option<i64>,
            Option<i64>,
            Option<i64>,
            Option<String>,
            String,
            Option<f64>,
            Option<String>,
        )> = {
            let conn = self.db.lock();
            conn.prepare(
                "SELECT id, scope_id, content, created_at, score, lifecycle_state, \
                 effective_from_ms, effective_until_ms, expires_at_ms, review_after_ms, \
                 superseded_by, priority_class, confidence, confidence_basis \
                 FROM memories WHERE COALESCE(artifact_family, '') != 'session'",
            )
            .and_then(|mut st| {
                st.query_map([], |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                        r.get(6)?,
                        r.get(7)?,
                        r.get(8)?,
                        r.get(9)?,
                        r.get(10)?,
                        r.get(11)?,
                        r.get(12)?,
                        r.get(13)?,
                    ))
                })
                .map(|rows| rows.flatten().collect())
            })
            .unwrap_or_default()
        };
        let mut n = 0;
        for (
            id,
            scope,
            content,
            created_at,
            score,
            lifecycle_state,
            effective_from_ms,
            effective_until_ms,
            expires_at_ms,
            review_after_ms,
            superseded_by,
            priority_class,
            confidence,
            confidence_basis,
        ) in rows
        {
            // Leading '-' guard: mac project slugs are "-Users-…" while path_to_scope
            // trims the dash — without this the prefix match misses and the export
            // writes raw per-machine scope dirs (the 2026-07-02 cross-machine bug).
            let s_norm = scope.trim_start_matches('-');
            let canon = if ws_slug.as_deref() == Some(s_norm) {
                "WS".to_string()
            } else if let Some(rest) = ws_slug
                .as_deref()
                .and_then(|slug| s_norm.strip_prefix(&format!("{slug}-")))
            {
                format!("WS-{rest}")
            } else {
                scope.clone()
            };
            let name = id.rsplit('/').next().unwrap_or(&id);
            let safe: String = name
                .chars()
                .map(|c| {
                    if c.is_alphanumeric() || "-_.".contains(c) {
                        c
                    } else {
                        '_'
                    }
                })
                .collect();
            let sub = dir.join(&canon);
            if std::fs::create_dir_all(&sub).is_err() {
                continue;
            }
            let integer = |value: Option<i64>| {
                value
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "null".to_string())
            };
            let decimal = |value: Option<f64>| {
                value
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "null".to_string())
            };
            let text = |value: Option<String>| value.unwrap_or_else(|| "null".to_string());
            let body = format!(
                "---\nid: {id}\nscope: {scope}\nscore: {score}\ncreated_at: {created_at}\nlifecycle_state: {lifecycle_state}\neffective_from_ms: {}\neffective_until_ms: {}\nexpires_at_ms: {}\nreview_after_ms: {}\nsuperseded_by: {}\npriority_class: {priority_class}\nconfidence: {}\nconfidence_basis: {}\nexported_by: crypt export-md\n---\n\n{content}\n",
                integer(effective_from_ms), integer(effective_until_ms), integer(expires_at_ms),
                integer(review_after_ms), text(superseded_by), decimal(confidence), text(confidence_basis),
            );
            if std::fs::write(sub.join(format!("{safe}.md")), body).is_ok() {
                n += 1;
            }
        }
        n
    }

    /// Record that these entries were injected into a context (returned by a recall). Persisted;
    /// paired with `record_use`, effectiveness = (access_count+1)/(inject_count+2). Best-effort.
    pub fn record_injections(&self, ids: &[String]) -> Result<(), String> {
        self.record_injections_observed(ids, &MemoryEventContext::default())
    }

    pub fn record_injections_observed(
        &self,
        ids: &[String],
        context: &MemoryEventContext,
    ) -> Result<(), String> {
        if ids.is_empty() {
            return Ok(());
        }
        let mut conn = self.db.lock();
        let tx = conn
            .transaction()
            .map_err(|e| self.persist_error(format!("injection transaction failed: {e}")))?;
        for id in ids {
            let changed = tx
                .execute(
                    "UPDATE memories SET inject_count = inject_count + 1 WHERE id = ?1",
                    rusqlite::params![id],
                )
                .map_err(|e| {
                    self.persist_error(format!("memory injection update failed for {id}: {e}"))
                })?;
            if changed != 1 {
                return Err(self.persist_error(format!("memory injection target not found: {id}")));
            }
            let scope_id = tx
                .query_row(
                    "SELECT scope_id FROM memories WHERE id = ?1",
                    rusqlite::params![id],
                    |row| row.get::<_, String>(0),
                )
                .map_err(|e| {
                    self.persist_error(format!("memory injection scope load failed for {id}: {e}"))
                })?;
            log_memory_event(
                &self.operation_attribution,
                &tx,
                "inject",
                Some(id),
                Some(&scope_id),
                context,
                1,
                None,
            )
            .map_err(|e| {
                self.persist_error(format!("memory injection event failed for {id}: {e}"))
            })?;
        }
        tx.commit()
            .map_err(|e| self.persist_error(format!("injection commit failed: {e}")))?;
        drop(conn);
        self.flush_event_outbox()?;
        self.clear_last_persist_error();
        Ok(())
    }

    /// Persisted inject_count for an entry (diagnostics/tests).
    pub fn inject_count(&self, id: &str) -> u32 {
        self.db
            .lock()
            .query_row(
                "SELECT inject_count FROM memories WHERE id = ?1",
                rusqlite::params![id],
                |r| r.get::<_, i64>(0),
            )
            .map(|n| n as u32)
            .unwrap_or(0)
    }

    /// Full content fetch for `id`. This records retrieval/access only; downstream value requires
    /// explicit receipt-linked feedback and is never inferred from the read itself.
    pub fn get_full(&self, id: &str) -> Result<(String, u32), String> {
        self.get_full_observed(id, &MemoryEventContext::default())
    }

    pub fn get_full_observed(
        &self,
        id: &str,
        context: &MemoryEventContext,
    ) -> Result<(String, u32), String> {
        let content = {
            let reg = self
                .registry
                .read()
                .map_err(|_| "registry lock poisoned".to_string())?;
            reg.get(id)
                .map(|e| e.content.clone())
                .ok_or_else(|| format!("no memory with id {id:?}"))?
        };
        let access_count = self.record_use_observed(id, context)?;
        Ok((content, access_count))
    }

    /// Record one transform's before/after size (per-layer savings, `crypt metrics`). Best-effort.
    pub fn log_transform(
        &self,
        ts: &str,
        verb: &str,
        scope: Option<&str>,
        before_chars: usize,
        after_chars: usize,
        meta: Option<&str>,
    ) {
        let context = MemoryEventContext::default();
        let session_id = normalized_session(&context);
        let trace_id = normalized_trace(&context, &format!("transform:{verb}:{ts}"));
        let turn_id = normalized_turn(&context, &trace_id);
        self.db.log_transform_with_identity(
            ts,
            verb,
            scope,
            before_chars,
            after_chars,
            meta,
            &self.operation_attribution.installation_id,
            &self.operation_attribution.service_instance_id,
            &context.surface,
            &session_id,
            &turn_id,
            &trace_id,
        );
    }

    /// Per-verb transform savings for `crypt metrics`.
    pub fn transform_metrics(&self) -> Vec<crate::memdb::TransformVerbMetrics> {
        self.db.transform_metrics()
    }
}

fn vault_review_markdown(report: &serde_json::Value) -> String {
    let mut out = format!(
        "# Crypt vault review\n\nSchema: 1\n\nContent included: {}\n\n## Section availability\n\n",
        report["contentIncluded"]
    );
    for name in [
        "memories",
        "events",
        "reviewEvidence",
        "approvals",
        "contextPackRefs",
    ] {
        out.push_str(&format!(
            "- `{name}`: {}\n",
            serde_json::to_string(&report["sections"][name]).unwrap_or_default()
        ));
    }
    for (index, memory) in report["memories"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
    {
        let mut metadata = memory.clone();
        let content = metadata
            .as_object_mut()
            .and_then(|item| item.remove("content"));
        out.push_str(&format!("\n## Review item {}\n\nMetadata:\n\n", index + 1));
        for line in serde_json::to_string_pretty(&metadata)
            .unwrap_or_default()
            .lines()
        {
            out.push_str(&format!("    {line}\n"));
        }
        if let Some(content) = content.and_then(|value| value.as_str().map(str::to_string)) {
            out.push_str("\nContent:\n\n");
            for line in content.lines() {
                out.push_str(&format!("    {line}\n"));
            }
        }
    }
    out
}

fn atomic_review_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut payload = bytes.to_vec();
    if !payload.ends_with(b"\n") {
        payload.push(b'\n');
    }
    let parent = path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut cursor = PathBuf::new();
    for component in parent.components() {
        if matches!(component, std::path::Component::ParentDir) {
            return Err("vault export path is unsafe".into());
        }
        cursor.push(component);
        if std::fs::symlink_metadata(&cursor).is_ok_and(|meta| meta.file_type().is_symlink()) {
            return Err("vault export path is unsafe".into());
        }
    }
    let parent_meta = std::fs::symlink_metadata(parent)
        .map_err(|e| format!("vault export parent unavailable: {e}"))?;
    if !parent_meta.is_dir() || parent_meta.file_type().is_symlink() || path.file_name().is_none() {
        return Err("vault export path is unsafe".into());
    }
    if let Ok(meta) = std::fs::symlink_metadata(path) {
        if meta.file_type().is_symlink() || !meta.is_file() {
            return Err("vault export target must be a regular file".into());
        }
        if std::fs::read(path).map_err(|e| e.to_string())? == payload {
            return Ok(());
        }
    }
    static TEMP_ID: AtomicU64 = AtomicU64::new(0);
    let name = path.file_name().unwrap().to_string_lossy();
    let temp = parent.join(format!(
        ".{name}.{}.{}.tmp",
        std::process::id(),
        TEMP_ID.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(|e| format!("vault export temp create failed: {e}"))?;
        file.write_all(&payload)
            .and_then(|_| file.sync_all())
            .map_err(|e| format!("vault export write failed: {e}"))?;
        std::fs::rename(&temp, path).map_err(|e| format!("vault export atomic replace failed: {e}"))
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

/// Extract distinctive keywords from a goal: lowercased words ≥ 4 chars,
/// deduplicated, minus a few common stopwords.
fn keywords_of(goal: &str) -> Vec<String> {
    const STOP: &[&str] = &[
        "the", "and", "with", "that", "this", "from", "into", "your", "create", "make", "file",
        "then", "have", "should", "would", "could", "about",
    ];
    let mut seen = std::collections::HashSet::new();
    goal.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() >= 4 && !STOP.contains(w))
        .filter(|w| seen.insert(w.to_string()))
        .map(|w| w.to_string())
        .collect()
}

fn sanitize_memory_id(value: &str) -> String {
    value
        .trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':') {
                c
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .chars()
        .take(160)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};

    #[test]
    fn vault_review_is_stable_ordered_and_content_opt_in() {
        let store = MemoryStore::new();
        store.db.lock().execute_batch(
            "INSERT INTO memories(id,tier,content,keywords,score,created_at,access_count,scope_id,content_hash,source_ids,artifact_family,producer,record_type,authority,influence_class,lifecycle_state,review_after_ms,superseded_by,priority_class,confidence,confidence_basis)
             VALUES('scope/a','Semantic','SECRET-A','',0.8,'2026-01-01T00:00:00Z',0,'scope','hash-a','[\"source-a\"]','memory','manual','decision','A2','reference','superseded',20,'scope/b','normal',0.7,'reviewed'),
                   ('scope/b','Semantic','SECRET-B','',0.9,'2026-01-02T00:00:00Z',0,'scope','hash-b','[]','memory','manual','decision','A1','reference','active',10,NULL,'protected',0.9,'approved');
             INSERT INTO memory_event_log(ts,event_kind,memory_id,surface,session_id,trace_id,scope_id,quantity,meta,event_uid,installation_id,client,turn_id,artifact_id,identity_status)
             VALUES('2026-01-03T00:00:00Z','superseded','scope/a','cli','session-x','trace-x','scope',1,'{\"authority\":\"A1\",\"reason_ref\":\"review-1\"}','event-1','install-1','codex','turn-1','artifact-1','observed');
             INSERT INTO context_feedback(trace_id,candidate_id,content_sha256,outcome,verified,verdict_ref,scope_id,ts)
             VALUES('trace-x','scope/a','hash-a','contradicted',1,'review-1','scope','2026-01-03T00:00:00Z');"
        ).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let json_path = dir.path().join("review.json");
        let markdown_path = dir.path().join("review.md");
        assert_eq!(
            store
                .export_vault_review(&json_path, "json", false)
                .unwrap(),
            2
        );
        let first = std::fs::read(&json_path).unwrap();
        assert_eq!(
            store
                .export_vault_review(&json_path, "json", false)
                .unwrap(),
            2
        );
        assert_eq!(first, std::fs::read(&json_path).unwrap());
        assert_eq!(
            store
                .export_vault_review(&markdown_path, "markdown", true)
                .unwrap(),
            2
        );
        drop(store);
        let report: serde_json::Value = serde_json::from_slice(&first).unwrap();
        assert_eq!(report["memories"][0]["id"], "scope/b");
        assert_eq!(
            report["memories"][1]["lifecycle"]["supersededBy"],
            "scope/b"
        );
        assert_eq!(report["memories"][0]["supersedes"][0], "scope/a");
        assert_eq!(report["memories"][1]["events"][0]["authority"], "A1");
        assert_eq!(
            report["memories"][1]["events"][0]["installationId"],
            "install-1"
        );
        assert_eq!(
            report["memories"][1]["reviewEvidence"][0]["outcome"],
            "contradicted"
        );
        assert_eq!(report["sections"]["approvals"]["status"], "unavailable");
        assert_eq!(
            report["sections"]["contextPackRefs"]["status"],
            "unavailable"
        );
        assert!(!String::from_utf8_lossy(&first).contains("SECRET"));
        let markdown = std::fs::read_to_string(markdown_path).unwrap();
        assert!(
            markdown.contains("SECRET-A") && markdown.find("Metadata:") < markdown.find("Content:")
        );
        #[cfg(unix)]
        {
            let link = dir.path().join("unsafe.json");
            std::os::unix::fs::symlink(&json_path, &link).unwrap();
            assert_eq!(
                store_export_error(&link),
                "vault export target must be a regular file"
            );
            let real = dir.path().join("real");
            std::fs::create_dir(&real).unwrap();
            let linked_parent = dir.path().join("linked");
            std::os::unix::fs::symlink(real, &linked_parent).unwrap();
            assert_eq!(
                store_export_error(&linked_parent.join("review.json")),
                "vault export path is unsafe"
            );
        }
    }

    fn store_export_error(path: &Path) -> String {
        MemoryStore::new()
            .export_vault_review(path, "json", false)
            .unwrap_err()
    }

    struct BatchOnlyEmbedder {
        calls: Arc<AtomicUsize>,
    }

    impl Embedder for BatchOnlyEmbedder {
        fn embed(&self, _text: &str) -> Vec<f32> {
            panic!("batch writer must not call single-document inference")
        }

        fn try_embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, String> {
            self.calls.fetch_add(1, AtomicOrdering::SeqCst);
            Ok(texts.iter().map(|_| vec![1.0, 0.0, 0.0]).collect())
        }

        fn dim(&self) -> usize {
            3
        }

        fn model_id(&self) -> &'static str {
            "batch-test-3"
        }
    }

    fn batch_request(batch_id: &str) -> MemoryBatchRequest {
        let make = |item_id: &str, name: &str| MemoryBatchItem {
            item_id: item_id.into(),
            name: name.into(),
            content: format!("{name} durable concept"),
            scope: "D--Claude".into(),
            tier: "Semantic".into(),
            artifact_family: "skill_output".into(),
            producer: "architect".into(),
            record_type: "architect_concept".into(),
            client: "codex".into(),
            session_id: "architect-job-1".into(),
            turn_id: "architect-turn-1".into(),
            trace_id: "architect-trace-1".into(),
            source_ids: vec![],
            lifecycle: MemoryLifecycleInputV1::default(),
        };
        MemoryBatchRequest {
            batch_id: batch_id.into(),
            items: vec![make("item-1", "batch-one"), make("item-2", "batch-two")],
        }
    }

    #[test]
    fn memory_batch_uses_one_native_batch_and_replay_uses_none() {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut store = MemoryStore::new();
        store.embedder = Arc::new(BatchOnlyEmbedder {
            calls: Arc::clone(&calls),
        });
        let request = batch_request("batch-one-call");

        let first = store.try_put_batch(&request).unwrap();
        let replay = store.try_put_batch(&request).unwrap();

        assert_eq!(calls.load(AtomicOrdering::SeqCst), 1);
        assert_eq!((first.inserted, first.duplicates), (2, 0));
        assert_eq!((replay.inserted, replay.duplicates), (0, 2));
    }

    #[test]
    fn memory_batch_rolls_back_rows_events_and_receipt_on_mid_commit_failure() {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut store = MemoryStore::new();
        store.embedder = Arc::new(BatchOnlyEmbedder {
            calls: Arc::clone(&calls),
        });
        store
            .db
            .lock()
            .execute_batch(
                "CREATE TRIGGER fail_second_batch_item BEFORE INSERT ON memories
                 WHEN NEW.id LIKE '%batch-two'
                 BEGIN SELECT RAISE(ABORT, 'forced batch failure'); END;",
            )
            .unwrap();
        let request = batch_request("batch-atomic-failure");

        assert!(matches!(
            store.try_put_batch(&request),
            Err(MemoryBatchError::Persist(_))
        ));
        let conn = store.db.lock();
        for table in ["memories", "memory_event_log", "memory_batch_receipt"] {
            let count: i64 = conn
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(count, 0, "{table} partially committed");
        }
        drop(conn);
        assert!(store.list(None).is_empty());

        store
            .db
            .lock()
            .execute_batch("DROP TRIGGER fail_second_batch_item")
            .unwrap();
        assert_eq!(store.try_put_batch(&request).unwrap().inserted, 2);
        assert_eq!(calls.load(AtomicOrdering::SeqCst), 2);
    }

    #[test]
    fn relationship_graph_uses_embedding_neighbors_not_id_tokens() {
        let store = MemoryStore::new();
        store.put(
            "shared-alpha",
            "deploy cloudflare worker production",
            "global",
            MemoryTier::Semantic,
        );
        store.put(
            "different-beta",
            "deploy cloudflare worker production",
            "global",
            MemoryTier::Semantic,
        );
        store.put(
            "shared-gamma",
            "cooking pasta tomato sauce",
            "global",
            MemoryTier::Semantic,
        );

        let graph = store.relationship_graph(1, 0.90);
        assert_eq!(graph.nodes.len(), 3);
        assert!(graph.nodes.iter().all(|node| node.weight == 1));
        assert_eq!(graph.edges.len(), 1);
        let edge = &graph.edges[0];
        assert_eq!(edge.kind, "semantic");
        assert_eq!(
            [edge.source.as_str(), edge.target.as_str()],
            ["global/different-beta", "global/shared-alpha"]
        );
    }

    #[test]
    fn anonymous_relationship_graph_contains_no_ids_or_scopes() {
        let store = MemoryStore::new();
        store.put(
            "private-name",
            "same semantic text",
            "D--Claude-secret",
            MemoryTier::Semantic,
        );
        store.put(
            "other-name",
            "same semantic text",
            "D--Claude-secret",
            MemoryTier::Semantic,
        );
        let rendered = store.relationship_graph_json(2, 0.50, true).to_string();
        assert!(!rendered.contains("private-name"));
        assert!(!rendered.contains("D--Claude-secret"));
        assert!(rendered.contains("\"nodes\""));
        assert!(rendered.contains("\"edges\""));
    }

    #[test]
    fn store_is_shareable_across_threads() {
        // Compile-level proof for the serve worker pool: every consumer in the workspace
        // build (incl. the CR daemon) inherits these bounds via cargo test --workspace.
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<MemoryStore>();
    }

    #[test]
    fn disabled_writes_surface_health_and_do_not_touch_registry() {
        let mut store = MemoryStore::new();
        store.writes_enabled = false;
        store.embedder_issue = Some("embedder unavailable in test".into());

        let err = store
            .try_put(
                "blocked",
                "this must not enter the in-memory registry",
                "global",
                MemoryTier::Semantic,
            )
            .expect_err("writes disabled should fail loudly");

        assert!(err.contains("embedder unavailable"), "err: {err}");
        assert!(
            store.entries(10).is_empty(),
            "failed writes must not diverge RAM from SQLite"
        );
        let health = store.health();
        assert!(!health.ok);
        assert!(!health.writes_enabled);
        assert_eq!(
            health.last_persist_error.as_deref(),
            Some("embedder unavailable in test")
        );
    }

    #[test]
    fn http_disabled_embedder_records_unavailable_embedding_terminal() {
        let mut store = MemoryStore::new();
        store.writes_enabled = false;
        store.embedder_issue = Some("embedder unavailable in test".into());

        let (status, _) = crate::serve::route_for_tests(
            &store,
            "POST",
            "/put",
            r#"{"name":"blocked-http","content":"body","scope":"global"}"#,
        );
        assert_eq!(status, 500);
        assert!(store.entries(10).is_empty());
        let terminal: (String, String) = store
            .db
            .lock()
            .query_row(
                "SELECT status, reason_code FROM context_event_log
                  WHERE phase='embedding.terminal' ORDER BY seq DESC LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(
            terminal,
            ("unavailable".into(), "embedding_unavailable".into())
        );
    }

    #[test]
    fn health_exposes_active_embedder_pipeline_fingerprint() {
        let store = MemoryStore::new();
        let expected = store.embedder.pipeline_fingerprint().label;
        let health = store.health();
        let json = store.health_json();

        assert_eq!(health.embedder_fingerprint, expected);
        assert_eq!(json["embedder_fingerprint"], expected);
        assert!(expected.starts_with("pf:v1:"));
        assert_eq!(expected.len(), "pf:v1:".len() + 64);
    }

    #[test]
    fn registry_load_rejects_malformed_rows() {
        let db = MemDb::open_in_memory();
        db.lock()
            .execute(
                "INSERT INTO memories
                 (id, tier, content, keywords, score, created_at, access_count, scope_id)
                 VALUES ('bad', 'not-a-tier', 'body', 'not-json', 0.5, 'now', 0, 'global')",
                [],
            )
            .unwrap();
        let err = MemoryStore::try_open(db)
            .err()
            .expect("bad row must fail load");
        assert!(err.contains("bad"), "err: {err}");
    }

    #[test]
    fn metadata_write_preserves_updated_at_and_provenance() {
        let store = MemoryStore::new();
        let id = store
            .try_put_with_metadata(
                "synced",
                "synced body",
                "global",
                MemoryTier::Semantic,
                "2026-02-03T04:05:06Z",
                &["source-a".into(), "source-b".into()],
            )
            .unwrap();
        let (updated_at, source_ids): (String, String) = store
            .db
            .lock()
            .query_row(
                "SELECT updated_at, source_ids FROM memories WHERE id=?1",
                [&id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(updated_at, "2026-02-03T04:05:06Z");
        assert_eq!(
            serde_json::from_str::<Vec<String>>(&source_ids).unwrap(),
            vec!["source-a", "source-b"]
        );
    }

    #[test]
    fn reindex_refuses_disabled_writes_and_invalid_embeddings() {
        struct ZeroEmbedder;
        impl Embedder for ZeroEmbedder {
            fn embed(&self, _text: &str) -> Vec<f32> {
                vec![0.0; 4]
            }
            fn dim(&self) -> usize {
                4
            }
            fn model_id(&self) -> &'static str {
                "zero-test"
            }
        }

        let mut disabled = MemoryStore::new();
        disabled.writes_enabled = false;
        assert!(disabled.reindex().is_err());

        let mut invalid = MemoryStore::new();
        let id = invalid
            .try_put("invalid-embedding", "body", "global", MemoryTier::Semantic)
            .unwrap();
        invalid.embedder = Arc::new(ZeroEmbedder);
        invalid.tamper_embed_model_for_tests(&id, "stale");
        let err = invalid.reindex().expect_err("zero vector must be rejected");
        assert!(err.contains("embedding"), "err: {err}");
    }

    #[test]
    fn reindex_surfaces_row_decode_errors() {
        let store = MemoryStore::new();
        store
            .db
            .lock()
            .execute(
                "INSERT INTO memories
                 (id, tier, content, keywords, score, created_at, access_count, scope_id)
                 VALUES (x'FF', '\"Semantic\"', 'body', '[]', 0.5, 'now', 0, 'global')",
                [],
            )
            .unwrap();
        let err = store.reindex().expect_err("bad row must surface");
        assert!(err.contains("reindex row load"), "err: {err}");
    }

    #[test]
    fn query_embedding_does_not_hold_the_registry_lock() {
        struct BlockingQueryEmbedder {
            entered: std::sync::mpsc::Sender<()>,
            release: Mutex<std::sync::mpsc::Receiver<()>>,
        }

        impl Embedder for BlockingQueryEmbedder {
            fn embed(&self, _text: &str) -> Vec<f32> {
                self.entered.send(()).unwrap();
                self.release.lock().unwrap().recv().unwrap();
                vec![1.0, 0.0]
            }

            fn dim(&self) -> usize {
                2
            }

            fn model_id(&self) -> &'static str {
                "blocking-query-test"
            }
        }

        fn assert_registry_available<F>(
            store: &MemoryStore,
            entered: &std::sync::mpsc::Receiver<()>,
            release: &std::sync::mpsc::Sender<()>,
            path: &'static str,
            operation: F,
        ) where
            F: FnOnce(MemoryStore) + Send + 'static,
        {
            let query_store = store.clone();
            let query = std::thread::spawn(move || operation(query_store));
            entered
                .recv_timeout(std::time::Duration::from_secs(1))
                .unwrap_or_else(|_| panic!("{path} must reach query embedding"));

            let registry_store = store.clone();
            let (access_started_tx, access_started_rx) = std::sync::mpsc::channel();
            let (access_done_tx, access_done_rx) = std::sync::mpsc::channel();
            let access = std::thread::spawn(move || {
                access_started_tx.send(()).unwrap();
                let mut registry = registry_store
                    .registry
                    .write()
                    .unwrap_or_else(|e| e.into_inner());
                let before = registry.all().len();
                registry.insert(MemoryEntry {
                    id: format!("global/unrelated-{path}"),
                    tier: MemoryTier::Semantic,
                    content: "unrelated registry mutation".into(),
                    keywords: Vec::new(),
                    score: 0.1,
                    created_at: "2026-07-18T00:00:00Z".into(),
                    access_count: 0,
                    embedding: None,
                    scope_id: "global".into(),
                });
                access_done_tx.send((before, registry.all().len())).unwrap();
            });
            access_started_rx
                .recv_timeout(std::time::Duration::from_secs(1))
                .expect("registry access thread must start");
            let while_embedding_blocked =
                access_done_rx.recv_timeout(std::time::Duration::from_millis(250));
            let completed_while_blocked = while_embedding_blocked.is_ok();

            release.send(()).unwrap();
            query.join().unwrap();
            let (before, after) = match while_embedding_blocked {
                Ok(counts) => counts,
                Err(_) => access_done_rx
                    .recv_timeout(std::time::Duration::from_secs(1))
                    .expect("registry access must finish after embedding is released"),
            };
            access.join().unwrap();

            assert!(
                completed_while_blocked,
                "{path} held the registry lock while query embedding was blocked"
            );
            assert_eq!(after, before + 1, "{path} registry write did not persist");
        }

        let mut store = MemoryStore::new();
        store
            .try_put(
                "registry-lock-probe",
                "registry lock probe",
                "global",
                MemoryTier::Semantic,
            )
            .unwrap();
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        store.embedder = Arc::new(BlockingQueryEmbedder {
            entered: entered_tx,
            release: Mutex::new(release_rx),
        });

        assert_registry_available(&store, &entered_rx, &release_tx, "search", |store| {
            let _ = store.search("registry lock probe search", 1);
        });
        assert_registry_available(&store, &entered_rx, &release_tx, "recall", |store| {
            let _ = store.recall_scored("registry lock probe recall", 1, &[]);
        });
        assert_registry_available(&store, &entered_rx, &release_tx, "context-for", |store| {
            let _ = store.context_for("registry lock probe context", 1);
        });
    }

    #[test]
    fn concurrent_registry_readers_do_not_serialize_recall() {
        let store = MemoryStore::new();
        let held_reader = store
            .registry
            .read()
            .unwrap_or_else(|error| error.into_inner());
        let recall_store = store.clone();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let recall = std::thread::spawn(move || {
            let _ = recall_store.recall_scored("concurrent reader probe", 1, &[]);
            done_tx.send(()).unwrap();
        });

        let completed_while_reader_held = done_rx
            .recv_timeout(std::time::Duration::from_millis(250))
            .is_ok();
        drop(held_reader);
        recall.join().unwrap();

        assert!(
            completed_while_reader_held,
            "a concurrent registry reader serialized prompt recall"
        );
    }

    #[test]
    fn repeated_queries_reuse_a_bounded_model_specific_embedding_cache() {
        struct CountingEmbedder {
            document_calls: Arc<AtomicU64>,
            query_calls: Arc<AtomicU64>,
            pipeline_version: u32,
        }

        impl Embedder for CountingEmbedder {
            fn embed(&self, _text: &str) -> Vec<f32> {
                self.document_calls.fetch_add(1, Ordering::SeqCst);
                vec![1.0, 0.0]
            }

            fn embed_query(&self, _text: &str) -> Vec<f32> {
                self.query_calls.fetch_add(1, Ordering::SeqCst);
                vec![1.0, 0.0]
            }

            fn dim(&self) -> usize {
                2
            }

            fn model_id(&self) -> &'static str {
                "counting-cache-test"
            }

            fn pipeline_fingerprint(&self) -> crypt_core::PipelineFingerprint {
                crypt_core::PipelineFingerprint::compute(
                    2,
                    "document",
                    "query",
                    self.model_id(),
                    "test",
                    true,
                    self.pipeline_version,
                )
            }
        }

        let mut store = MemoryStore::new();
        store
            .try_put(
                "query-cache-probe",
                "query cache probe",
                "global",
                MemoryTier::Semantic,
            )
            .unwrap();
        let document_calls = Arc::new(AtomicU64::new(0));
        let query_calls = Arc::new(AtomicU64::new(0));
        store.embedder = Arc::new(CountingEmbedder {
            document_calls: Arc::clone(&document_calls),
            query_calls: Arc::clone(&query_calls),
            pipeline_version: 1,
        });

        let _ = store.search("same query", 1);
        let _ = store.search("same query", 1);
        let _ = store.recall_scored("same query", 1, &[]);
        let _ = store.recall_scored("same query", 1, &[]);
        let _ = store.context_for("same query", 1);
        assert_eq!(document_calls.load(Ordering::SeqCst), 0);
        assert_eq!(
            query_calls.load(Ordering::SeqCst),
            1,
            "all retrieval paths must share the query embedding cache"
        );
        let stats = store.health_json()["query_embedding_cache"].clone();
        assert_eq!(stats["hits"], 4);
        assert_eq!(stats["misses"], 1);

        let replacement_calls = Arc::new(AtomicU64::new(0));
        store.embedder = Arc::new(CountingEmbedder {
            document_calls: Arc::new(AtomicU64::new(0)),
            query_calls: Arc::clone(&replacement_calls),
            pipeline_version: 2,
        });
        let _ = store.search("same query", 1);
        assert_eq!(
            replacement_calls.load(Ordering::SeqCst),
            1,
            "cache identity must include the full embedding pipeline fingerprint"
        );

        for index in 0..QUERY_EMBEDDING_CACHE_CAPACITY + 2 {
            let _ = store.search(&format!("distinct query {index}"), 1);
        }
        let bounded = store.health_json()["query_embedding_cache"].clone();
        assert_eq!(bounded["capacity"], QUERY_EMBEDDING_CACHE_CAPACITY);
        assert_eq!(bounded["entries"], QUERY_EMBEDDING_CACHE_CAPACITY);
    }

    #[test]
    #[ignore = "manual before/after query-cache eviction measurement"]
    fn measure_hot_query_retention_across_two_cache_turnovers() {
        const HOT_QUERIES: usize = 32;
        struct SlowCountingEmbedder {
            query_calls: Arc<AtomicU64>,
        }

        impl Embedder for SlowCountingEmbedder {
            fn embed(&self, _text: &str) -> Vec<f32> {
                vec![1.0, 0.0]
            }

            fn embed_query(&self, _text: &str) -> Vec<f32> {
                self.query_calls.fetch_add(1, Ordering::SeqCst);
                std::thread::sleep(std::time::Duration::from_millis(1));
                vec![1.0, 0.0]
            }

            fn dim(&self) -> usize {
                2
            }

            fn model_id(&self) -> &'static str {
                "slow-counting-lru-measurement"
            }
        }

        let mut store = MemoryStore::new();
        let query_calls = Arc::new(AtomicU64::new(0));
        store.embedder = Arc::new(SlowCountingEmbedder {
            query_calls: Arc::clone(&query_calls),
        });
        let started = Instant::now();
        for index in 0..HOT_QUERIES {
            let _ = store.search(&format!("hot query {index}"), 1);
        }
        for index in 0..QUERY_EMBEDDING_CACHE_CAPACITY * 2 {
            let _ = store.search(&format!("cold query {index}"), 1);
            for hot_index in 0..HOT_QUERIES {
                let _ = store.search(&format!("hot query {hot_index}"), 1);
            }
        }
        let elapsed_us = started.elapsed().as_micros() as u64;
        let stats = store.health_json()["query_embedding_cache"].clone();
        println!(
            "{}",
            serde_json::json!({
                "schema_version": 1,
                "capacity": QUERY_EMBEDDING_CACHE_CAPACITY,
                "cold_queries": QUERY_EMBEDDING_CACHE_CAPACITY * 2,
                "hot_working_set": HOT_QUERIES,
                "hot_reuses": QUERY_EMBEDDING_CACHE_CAPACITY * 2 * HOT_QUERIES,
                "provider_calls": query_calls.load(Ordering::SeqCst),
                "ideal_lru_provider_calls": QUERY_EMBEDDING_CACHE_CAPACITY * 2 + HOT_QUERIES,
                "elapsed_us": elapsed_us,
                "cache": stats,
                "cache_key": "embedding_pipeline_digest:query:sha256(query_text)",
                "invalidation_key": "embedding_pipeline_digest",
                "reused_work": "query embedding provider inference",
            })
        );
    }

    #[test]
    fn search_uses_the_asymmetric_query_embedding_for_ranking() {
        struct AsymmetricEmbedder;

        impl Embedder for AsymmetricEmbedder {
            fn embed(&self, _text: &str) -> Vec<f32> {
                vec![0.0, 1.0]
            }

            fn embed_query(&self, _text: &str) -> Vec<f32> {
                vec![1.0, 0.0]
            }

            fn dim(&self) -> usize {
                2
            }

            fn model_id(&self) -> &'static str {
                "asymmetric-ranking-test"
            }
        }

        let mut store = MemoryStore::new();
        store.embedder = Arc::new(AsymmetricEmbedder);
        let mut registry = store.registry.write().unwrap();
        for (id, embedding) in [("target", vec![1.0, 0.0]), ("distractor", vec![0.0, 1.0])] {
            registry.insert(MemoryEntry {
                id: format!("global/{id}"),
                tier: MemoryTier::Semantic,
                content: format!("unrelated {id} body"),
                keywords: Vec::new(),
                score: 0.6,
                created_at: "2026-07-18T00:00:00Z".into(),
                access_count: 0,
                embedding: Some(embedding),
                scope_id: "global".into(),
            });
        }
        drop(registry);

        let hits = store.search("query with no lexical overlap", 2);
        assert_eq!(
            hits.first().map(|entry| entry.id.as_str()),
            Some("global/target")
        );
    }

    #[test]
    fn context_for_releases_registry_before_feedback_db_reads() {
        struct GatedEmbedder {
            entered: std::sync::mpsc::Sender<()>,
            release: Mutex<std::sync::mpsc::Receiver<()>>,
        }

        impl Embedder for GatedEmbedder {
            fn embed(&self, _text: &str) -> Vec<f32> {
                self.entered.send(()).unwrap();
                self.release.lock().unwrap().recv().unwrap();
                vec![1.0, 0.0]
            }

            fn dim(&self) -> usize {
                2
            }
        }

        let mut store = MemoryStore::new();
        store
            .try_put(
                "context-lock-probe",
                "context registry feedback lock probe",
                "global",
                MemoryTier::Semantic,
            )
            .unwrap();
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        store.embedder = Arc::new(GatedEmbedder {
            entered: entered_tx,
            release: Mutex::new(release_rx),
        });

        let db_guard = store.db.lock();
        let context_store = store.clone();
        let context = std::thread::spawn(move || {
            context_store.context_for("context registry feedback lock probe", 1)
        });
        entered_rx
            .recv_timeout(std::time::Duration::from_secs(1))
            .expect("context_for must reach query embedding");
        release_tx.send(()).unwrap();

        // On the old path context_for holds registry indefinitely while waiting for db_guard.
        // Observe that state when present so the red assertion cannot race the query thread.
        let observe_until = Instant::now() + std::time::Duration::from_millis(250);
        while Instant::now() < observe_until {
            match store.registry.try_write() {
                Err(std::sync::TryLockError::WouldBlock) => break,
                Err(std::sync::TryLockError::Poisoned(_)) => panic!("registry lock poisoned"),
                Ok(guard) => drop(guard),
            }
            std::thread::yield_now();
        }

        let registry_store = store.clone();
        let (read_started_tx, read_started_rx) = std::sync::mpsc::channel();
        let (read_done_tx, read_done_rx) = std::sync::mpsc::channel();
        let read = std::thread::spawn(move || {
            read_started_tx.send(()).unwrap();
            let count = registry_store
                .registry
                .write()
                .unwrap_or_else(|e| e.into_inner())
                .all()
                .len();
            read_done_tx.send(count).unwrap();
        });
        read_started_rx
            .recv_timeout(std::time::Duration::from_secs(1))
            .expect("registry read thread must start");
        let while_db_blocked = read_done_rx.recv_timeout(std::time::Duration::from_millis(250));
        let completed_while_db_blocked = while_db_blocked.is_ok();

        drop(db_guard);
        context.join().unwrap();
        let count = match while_db_blocked {
            Ok(count) => count,
            Err(_) => read_done_rx
                .recv_timeout(std::time::Duration::from_secs(1))
                .expect("registry read must finish after DB lock is released"),
        };
        read.join().unwrap();

        assert!(
            completed_while_db_blocked,
            "context_for held registry while waiting for feedback DB reads"
        );
        assert!(count >= 1);
    }

    #[test]
    fn recall_scored_sorts_by_cosine_over_fused_candidates() {
        // The 2026-07-05 T2.1 gate outcome, pinned as a test: candidates come from the fused
        // hybrid retriever, but the returned ORDER is pure query-cosine (fused-order ranking
        // was tried and reverted same-day — it degraded known-useful mean rank 2.37 -> 6.07).
        // With the trigram hash embedder, content identical to the query gives cos ~1.0, so
        // the exact-match entry must lead even though both entries carry the queried keyword.
        let store = MemoryStore::new();
        let exact = store.remember("alpha beta procedure", vec!["alpha".into()]);
        let _lexical_heavy = store.remember(
            "totally different content about deploy pipelines and rollback drills",
            vec!["alpha".into(), "beta".into(), "procedure".into()],
        );
        let hits = store.recall_scored("alpha beta procedure", 2, &[]);
        assert!(!hits.is_empty());
        assert_eq!(hits[0].0.id, exact.id, "cosine order must lead");
        assert!(hits[0].1 >= hits.last().unwrap().1, "descending cosine");
    }

    #[test]
    fn recall_scored_filters_scope_before_candidate_generation() {
        let store = MemoryStore::new();
        let query = "alpha zebra installed crypt exact marker";
        for index in 0..32 {
            let content = format!("{query} global distractor {index}");
            store
                .try_put(
                    &format!("global-distractor-{index}"),
                    &content,
                    "global",
                    MemoryTier::Semantic,
                )
                .unwrap();
        }
        let target_id = store
            .try_put("target", query, "qa-scope", MemoryTier::Semantic)
            .unwrap();

        let hits = store.recall_scored(query, 1, &["qa-scope".to_string()]);

        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].0.id, target_id);
        assert_eq!(hits[0].0.scope_id, "qa-scope");
    }

    #[test]
    fn recall_abstains_when_all_candidates_are_a0() {
        let store = MemoryStore::new();
        let id = store
            .try_put(
                "untrusted-candidate",
                "unique authority boundary marker",
                "global",
                MemoryTier::Semantic,
            )
            .unwrap();
        store
            .db
            .lock()
            .execute(
                "UPDATE memories SET authority='A0' WHERE id=?1",
                rusqlite::params![id],
            )
            .unwrap();

        let hits = store.recall_scored(
            "unique authority boundary marker",
            5,
            &["global".to_string()],
        );

        assert!(hits.is_empty());
        assert_eq!(
            store.last_recall_status().as_deref(),
            Some("insufficient_confidence")
        );
    }

    #[test]
    fn protected_expired_and_superseded_rows_never_bypass_lifecycle_admission() {
        let store = MemoryStore::new();
        let expired = store
            .try_put(
                "expired-protected",
                "lifecycle inverse marker",
                "global",
                MemoryTier::Semantic,
            )
            .unwrap();
        let replacement = store
            .try_put(
                "replacement",
                "lifecycle inverse marker successor",
                "global",
                MemoryTier::Semantic,
            )
            .unwrap();
        store.db.lock().execute(
            "UPDATE memories SET priority_class='protected', effective_until_ms=1000 WHERE id=?1",
            rusqlite::params![expired],
        ).unwrap();
        assert!(!store.recall_eligible_ids_at(2000, false).contains(&expired));
        assert!(store.recall_eligible_ids_at(2000, true).contains(&expired));
        store
            .db
            .lock()
            .execute(
                "UPDATE memories SET superseded_by=?2 WHERE id=?1",
                rusqlite::params![expired, replacement],
            )
            .unwrap();
        assert!(!store.recall_eligible_ids_at(0, true).contains(&expired));
    }

    #[test]
    fn recall_scored_reranks_beyond_the_old_scoped_candidate_floor() {
        struct ControlledEmbedder;
        impl Embedder for ControlledEmbedder {
            fn embed(&self, text: &str) -> Vec<f32> {
                if text == "always replace never add installer updater"
                    || text == "retain one package artifact per platform"
                {
                    vec![1.0, 0.0]
                } else {
                    vec![0.5, 0.8660254]
                }
            }
            fn dim(&self) -> usize {
                2
            }
            fn model_id(&self) -> &'static str {
                "controlled-test"
            }
        }

        let mut store = MemoryStore::new();
        store.embedder = Arc::new(ControlledEmbedder);
        let target_id = store
            .try_put(
                "adapt-target",
                "retain one package artifact per platform",
                "qa-scope",
                MemoryTier::Semantic,
            )
            .unwrap();
        let mut distractor_ids = Vec::new();
        for index in 0..100 {
            let id = store
                .try_put(
                    &format!("lexical-distractor-{index}"),
                    &format!("always replace never add installer updater temporary note {index}"),
                    "qa-scope",
                    MemoryTier::Semantic,
                )
                .unwrap();
            distractor_ids.push(id);
        }
        {
            let mut registry = store.registry.write().unwrap();
            let mut target = registry.remove(&target_id).unwrap();
            target.score = 0.0;
            registry.insert(target);
            for id in &distractor_ids {
                let mut distractor = registry.remove(id).unwrap();
                distractor.score = 1.0;
                registry.insert(distractor);
            }
        }

        let registry = store.registry.read().unwrap();
        let query_embedding = store
            .embedder
            .embed_query("always replace never add installer updater");
        let old_candidates = MemoryRetriever::retrieve_hybrid(
            &registry,
            "always replace never add installer updater",
            Some(&query_embedding),
            32,
        );
        assert!(
            !old_candidates.iter().any(|entry| entry.id == target_id),
            "fixture must place the target beyond the former 32-candidate floor"
        );
        drop(registry);

        let hits = store.recall_scored(
            "always replace never add installer updater",
            4,
            &["qa-scope".to_string()],
        );

        assert_eq!(hits[0].0.id, target_id, "cosine rerank must see the target");
    }

    #[test]
    fn recall_scored_prioritizes_self_scope_over_global_chain() {
        let store = MemoryStore::new();
        let query = "alpha zebra installed crypt exact marker";
        for index in 0..32 {
            let content = format!("{query} global distractor {index}");
            store
                .try_put(
                    &format!("global-distractor-{index}"),
                    &content,
                    "global",
                    MemoryTier::Semantic,
                )
                .unwrap();
        }
        let target_id = store
            .try_put("target", query, "qa-scope", MemoryTier::Semantic)
            .unwrap();

        let hits = store.recall_scored(query, 1, &["qa-scope".to_string(), "global".to_string()]);

        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].0.id, target_id);
        assert_eq!(hits[0].0.scope_id, "qa-scope");
    }

    #[test]
    fn recall_scored_uses_scope_as_soft_bonus_not_hard_sort() {
        let store = MemoryStore::new();
        let query = "crypt codex hook hybrid retrieval observability logs";
        store
            .try_put(
                "weak-local",
                "crypt codex hook hybrid retrieval logs local note about old status naming",
                "qa-scope",
                MemoryTier::Semantic,
            )
            .unwrap();
        let global_id = store
            .try_put("strong-global", query, "global", MemoryTier::Semantic)
            .unwrap();

        let hits = store.recall_scored(query, 1, &["qa-scope".to_string(), "global".to_string()]);

        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].0.id, global_id);
        assert_eq!(hits[0].0.scope_id, "global");

        let tie_store = MemoryStore::new();
        let local_id = tie_store
            .try_put("local-tie", query, "qa-scope", MemoryTier::Semantic)
            .unwrap();
        tie_store
            .try_put("global-tie", query, "global", MemoryTier::Semantic)
            .unwrap();
        let tie_hits =
            tie_store.recall_scored(query, 1, &["qa-scope".to_string(), "global".to_string()]);

        assert_eq!(tie_hits.len(), 1);
        assert_eq!(tie_hits[0].0.id, local_id);
        assert_eq!(tie_hits[0].0.scope_id, "qa-scope");
    }

    #[test]
    fn keywords_filter_short_and_stopwords() {
        let k = keywords_of("Create a file named README with deployment instructions");
        assert!(k.contains(&"readme".to_string()));
        assert!(k.contains(&"deployment".to_string()));
        assert!(k.contains(&"instructions".to_string()));
        assert!(!k.iter().any(|w| w == "a")); // too short
        assert!(!k.iter().any(|w| w == "create")); // stopword
    }

    #[test]
    fn gate_history_requires_qualification_and_drops_superseded_veto() {
        let m = MemoryStore::new();
        let e = m.remember(
            "nginx is dockerized so diff the confs before any rebuild",
            vec![],
        );
        // A verified contradiction against the CURRENT bytes vetoes the entry.
        m.record_feedback(&FeedbackRecord {
            trace_id: "t".into(),
            candidate_id: e.id.clone(),
            content_sha256: content_hash(&e.content),
            outcome: Outcome::Contradicted,
            source: FeedbackSource::ObservedAction,
            verdict_ref: None,
            scope_id: "global".into(),
        })
        .unwrap();
        let unqualified = m.gate_history_for(&[&e]);
        assert!(EffectivenessGate::default().should_inject(&unqualified, &e.id));
        m.db.lock()
            .execute(
                "UPDATE context_feedback SET qualified_experiment_id='test-qualified'",
                [],
            )
            .unwrap();
        m.db.lock()
            .execute(
                "INSERT INTO learning_experiment VALUES ('test-qualified','x','p','x','code','2025-01-01','2026-01-01',1,0,'2025-01-01')",
                [],
            )
            .unwrap();
        m.db.lock()
            .execute(
                "INSERT INTO learning_promotion_receipt VALUES ('test-qualified','x',1,1,1,1,0,'qualified','test',1,'2025-01-01')",
                [],
            )
            .unwrap();
        let hist = m.gate_history_for(&[&e]);
        assert!(
            !EffectivenessGate::default().should_inject(&hist, &e.id),
            "a verified contradiction on the current bytes must veto"
        );
        // Supersede: same id, corrected content (new sha) -> the old veto is stale, must not apply.
        let mut corrected = e.clone();
        corrected.content = "nginx is dockerized; the confs are baked via Dockerfile COPY".into();
        let hist2 = m.gate_history_for(&[&corrected]);
        assert!(
            EffectivenessGate::default().should_inject(&hist2, &e.id),
            "a superseded contradiction (content edited) must no longer veto"
        );
    }

    #[test]
    fn recall_scored_applies_only_qualified_feedback_veto_until_superseded() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("recall-veto.db");
        let store = MemoryStore::open(MemDb::open(&db_path).unwrap());
        let id = store.put(
            "unsafe_rebuild",
            "always rebuild nginx immediately without checking configuration diffs",
            "global",
            MemoryTier::Semantic,
        );
        let entry = store
            .entries(10)
            .into_iter()
            .find(|entry| entry.id == id)
            .unwrap();
        store
            .record_feedback(&FeedbackRecord {
                trace_id: "recall-veto".into(),
                candidate_id: id.clone(),
                content_sha256: content_hash(&entry.content),
                outcome: Outcome::Contradicted,
                source: FeedbackSource::ObservedAction,
                verdict_ref: None,
                scope_id: "global".into(),
            })
            .unwrap();

        let not_vetoed = store.recall_scored(
            "nginx rebuild configuration diffs",
            5,
            &["global".to_string()],
        );
        assert!(not_vetoed.iter().any(|(entry, _)| entry.id == id));
        store
            .db
            .lock()
            .execute(
                "UPDATE context_feedback SET qualified_experiment_id='test-qualified'",
                [],
            )
            .unwrap();
        store.db.lock().execute(
            "INSERT INTO learning_experiment VALUES ('test-qualified','x','p','x','code','2025-01-01','2026-01-01',1,0,'2025-01-01')",
            [],
        ).unwrap();
        store.db.lock().execute(
            "INSERT INTO learning_promotion_receipt VALUES ('test-qualified','x',1,1,1,1,0,'qualified','test',1,'2025-01-01')",
            [],
        ).unwrap();

        let vetoed = store.recall_scored(
            "nginx rebuild configuration diffs",
            5,
            &["global".to_string()],
        );
        assert!(
            vetoed.iter().all(|(entry, _)| entry.id != id),
            "the shared recall API must enforce a current verified contradiction"
        );

        drop(store);
        let store = MemoryStore::open(MemDb::open(&db_path).unwrap());
        let vetoed_after_restart = store.recall_scored(
            "nginx rebuild configuration diffs",
            5,
            &["global".to_string()],
        );
        assert!(
            vetoed_after_restart.iter().all(|(entry, _)| entry.id != id),
            "the shared recall veto must survive a service/store restart"
        );

        store.put(
            "unsafe_rebuild",
            "inspect nginx configuration diffs before deciding whether a rebuild is required",
            "global",
            MemoryTier::Semantic,
        );
        let superseded = store.recall_scored(
            "nginx rebuild configuration diffs",
            5,
            &["global".to_string()],
        );
        assert!(
            superseded.iter().any(|(entry, _)| entry.id == id),
            "editing the bytes must clear the stale veto for the same id"
        );
    }

    #[test]
    fn linked_neighbors_surface_from_wikilink_edges() {
        let store = MemoryStore::new();
        // Neighbour B — deliberately NOT about the seed's terms; only reachable via the link.
        store.put(
            "terse_rule_xyz",
            "cut all filler and preamble from every reply",
            "global",
            MemoryTier::Semantic,
        );
        // Seed A points at B by [[wikilink]].
        let a_id = store.put(
            "brief_seed",
            "brief mode is the default; see [[terse_rule_xyz]] for the corollary",
            "global",
            MemoryTier::Semantic,
        );
        let neigh = store.linked_neighbors(&[a_id], &["global".to_string()], 5);
        assert_eq!(neigh.len(), 1, "the linked neighbour must be surfaced");
        assert_eq!(neigh[0].0.id, "global/terse_rule_xyz");
        assert!(neigh[0].1 < 0.5, "neighbour ranks below a direct hit");

        // A dangling link resolves to nothing — no phantom candidate.
        let d_id = store.put(
            "dangler",
            "points at [[nonexistent_target_abc]]",
            "global",
            MemoryTier::Semantic,
        );
        assert!(store
            .linked_neighbors(&[d_id], &["global".to_string()], 5)
            .is_empty());
    }

    #[test]
    fn recall_scored_promotes_one_hop_links_without_exceeding_limit() {
        let store = MemoryStore::new();
        store.put(
            "terse_rule_xyz",
            "cut all filler and preamble from every reply",
            "global",
            MemoryTier::Semantic,
        );
        store.put(
            "brief_seed",
            "zebracorn response invariant; see [[terse_rule_xyz]]",
            "global",
            MemoryTier::Semantic,
        );
        for index in 0..8 {
            store.put(
                &format!("distractor_{index}"),
                &format!("generic response policy unrelated detail {index}"),
                "global",
                MemoryTier::Semantic,
            );
        }

        let hits = store.recall_scored("zebracorn response invariant", 5, &["global".to_string()]);
        assert_eq!(
            hits.len(),
            5,
            "graph expansion must respect the caller limit"
        );
        assert!(
            hits.iter()
                .any(|(entry, _)| entry.id == "global/terse_rule_xyz"),
            "an in-scope one-hop neighbour must receive a bounded graph slot"
        );

        let dangling = MemoryStore::new();
        dangling.put(
            "dangler",
            "unique dangling seed [[nonexistent_target_abc]]",
            "global",
            MemoryTier::Semantic,
        );
        let dangling_hits =
            dangling.recall_scored("unique dangling seed", 5, &["global".to_string()]);
        assert_eq!(
            dangling_hits.len(),
            1,
            "dangling links must not create phantom rows"
        );
    }

    #[test]
    fn recall_scored_backfills_unused_graph_lane_with_direct_hits() {
        let store = MemoryStore::new();
        for index in 0..7 {
            store.put(
                &format!("direct-{index}"),
                &format!("zebracorn direct semantic recall policy {index}"),
                "global",
                MemoryTier::Semantic,
            );
        }

        let hits = store.recall_scored(
            "zebracorn direct semantic recall policy",
            5,
            &["global".to_string()],
        );
        assert_eq!(
            hits.len(),
            5,
            "an unused link lane must not reduce direct recall coverage"
        );
    }

    #[test]
    fn detailed_recall_marks_link_origin_and_metrics_count_it() {
        let store = MemoryStore::new();
        store.put(
            "linked-target",
            "Cut every filler phrase and opening preamble.",
            "global",
            MemoryTier::Semantic,
        );
        store.put(
            "semantic-seed",
            "zebracorn release invariant points to [[linked-target]].",
            "global",
            MemoryTier::Semantic,
        );
        for index in 0..8 {
            store.put(
                &format!("origin-distractor-{index}"),
                &format!("generic release policy unrelated detail {index}"),
                "global",
                MemoryTier::Semantic,
            );
        }

        let hits =
            store.recall_scored_detailed("zebracorn release invariant", 5, &["global".to_string()]);
        assert!(hits
            .iter()
            .any(|hit| { hit.entry.id == "global/linked-target" && hit.origin == "link" }));

        let replay = serde_json::json!([
            {"id":"global/semantic-seed","score":0.9,"origin":"semantic"},
            {"id":"global/linked-target","score":0.3,"origin":"link"}
        ])
        .to_string();
        store.log_recall_with_replay(
            "2026-07-16T00:00:00Z",
            Some("global"),
            10,
            2,
            100,
            80,
            "serve",
            Some("release signing"),
            Some("test"),
            None,
            None,
            None,
            None,
            None,
            "production",
            &replay,
            &replay,
        );
        let metrics = store.metrics_json();
        assert_eq!(metrics["graph_recall"]["link_inclusions"], 1);
        assert_eq!(metrics["graph_recall"]["recalls_with_links"], 1);
        assert_eq!(
            metrics["governance"]["contextual_enrichment"]["status"],
            "not_promoted"
        );
    }

    #[test]
    fn metrics_json_excludes_nonproduction_recall_traffic() {
        let store = MemoryStore::new();
        let production_replay = serde_json::json!([
            {"id":"global/production","score":0.9,"origin":"semantic"},
            {"id":"global/production-link","score":0.3,"origin":"link"}
        ])
        .to_string();
        let evaluation_replay = serde_json::json!([
            {"id":"global/evaluation","score":0.8,"origin":"semantic"},
            {"id":"global/evaluation-link","score":0.2,"origin":"link"}
        ])
        .to_string();

        store.log_recall_with_replay(
            "2026-07-16T00:00:00Z",
            Some("global"),
            20,
            2,
            200,
            80,
            "serve",
            Some("production replay"),
            Some("codex"),
            Some("production-session"),
            Some("D--Claude"),
            Some("UserPromptSubmit"),
            Some("production-trace"),
            Some("all"),
            "production",
            &production_replay,
            &production_replay,
        );
        store.log_recall_with_replay(
            "2026-07-16T00:00:01Z",
            Some("global"),
            20,
            2,
            200,
            80,
            "serve",
            Some("evaluation replay"),
            Some("evaluation-client"),
            Some("evaluation-session"),
            Some("D--Claude"),
            Some("spotcheck"),
            Some("evaluation-trace"),
            Some("evaluation-only"),
            "evaluation",
            &evaluation_replay,
            &evaluation_replay,
        );

        let metrics = store.metrics_json();
        assert_eq!(metrics["recalls"], 1);
        assert_eq!(metrics["hits"], 2);
        assert_eq!(metrics["graph_recall"]["link_inclusions"], 1);
        assert_eq!(metrics["graph_recall"]["recalls_with_links"], 1);
        assert_eq!(
            metrics["attribution"]["client_counts"],
            serde_json::json!([{"client":"codex","count":1}])
        );
        assert_eq!(
            metrics["attribution"]["visibility_counts"],
            serde_json::json!([{"visibility":"all","count":1}])
        );
        assert_eq!(
            metrics["attribution"]["quality"]["attributed_meaningful"],
            1
        );
    }

    #[test]
    fn backfill_links_rebuilds_from_content() {
        let store = MemoryStore::new();
        store.put("x", "links [[a]] and [[b]]", "global", MemoryTier::Semantic);
        let (edges, _resolvable) = store.backfill_links();
        assert_eq!(edges, 2);
    }

    #[test]
    fn new_rows_store_quantized_embedding_and_reload() {
        let path = std::env::temp_dir().join(format!("cr-mem-quant-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        {
            let m = MemoryStore::open(MemDb::open(&path).unwrap());
            m.record_run("deploy the quantized memory store", true, "ok");
        }
        // New rows must persist the compact quantized blob, not the legacy f32 one.
        {
            let db = MemDb::open(&path).unwrap();
            let (f32_null, quant_present): (bool, bool) = db
                .lock()
                .query_row(
                    "SELECT embedding IS NULL, embedding_q IS NOT NULL FROM memories LIMIT 1",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .unwrap();
            assert!(f32_null, "new rows should leave the legacy f32 column NULL");
            assert!(quant_present, "new rows should write the quantized column");
        }
        let reloaded = MemoryStore::open(MemDb::open(&path).unwrap());
        assert_eq!(reloaded.len(), 1);
        assert!(reloaded.context_for("quantized memory deploy", 5).is_some());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn legacy_f32_embedding_blob_still_reads() {
        let path = std::env::temp_dir().join(format!("cr-mem-legacy-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        {
            // Seed a legacy-format row: f32 blob in `embedding`, `embedding_q` NULL.
            let db = MemDb::open(&path).unwrap();
            let emb: Vec<f32> = vec![0.1, 0.2, 0.3, 0.4, 0.5];
            let f32_blob: Vec<u8> = emb.iter().flat_map(|x| x.to_le_bytes()).collect();
            db.lock()
                .execute(
                    "INSERT INTO memories
                         (id, tier, content, keywords, score, created_at, access_count, embedding, embedding_q)
                     VALUES ('legacy-1', '\"Episodic\"', 'legacy memory content', '[]', 1.0, 't', 0, ?1, NULL)",
                    rusqlite::params![f32_blob],
                )
                .unwrap();
        }
        let reloaded = MemoryStore::open(MemDb::open(&path).unwrap());
        assert_eq!(reloaded.len(), 1, "legacy f32 row must still load");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn ingest_markdown_stores_scoped_entry() {
        let dir = std::env::temp_dir().join(format!("cr-mdtest-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("my-note.md");
        std::fs::write(
            &path,
            "---\nname: my-note\ndescription: a deploy note\ntype: feedback\n---\nDeploy the worker to cloudflare. See [[other]].",
        )
        .unwrap();
        let m = MemoryStore::new();
        let id = m.ingest_markdown(&path, "D--Claude-coderight").unwrap();
        assert_eq!(id, "D--Claude-coderight/my-note");
        let reg = m.registry.read().unwrap();
        let entry = reg.get(&id).expect("ingested entry present");
        assert_eq!(entry.scope_id, "D--Claude-coderight");
        assert_eq!(entry.tier, MemoryTier::Semantic);
        assert!(entry.embedding.is_some());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn remember_consolidated_is_idempotent_by_stable_id() {
        let m = MemoryStore::new();
        let first = m
            .remember_consolidated(
                "daily analyzer/session 1",
                "Session s1 failed because cargo test was run before dependencies were installed.",
                vec!["cargo".into(), "dependencies".into()],
                0.8,
            )
            .unwrap();
        let second = m
            .remember_consolidated(
                "daily analyzer/session 1",
                "Session s1 recovered by installing dependencies before cargo test.",
                vec!["cargo".into(), "dependencies".into()],
                0.9,
            )
            .unwrap();

        assert_eq!(first.id, "daily-analyzer-session-1");
        assert_eq!(second.id, first.id);
        assert_eq!(m.len(), 1);
        let entries = m.search("installing dependencies", 5);
        assert_eq!(entries.len(), 1);
        assert!(entries[0].content.contains("installing dependencies"));
    }

    #[test]
    fn record_then_retrieve_relevant() {
        let m = MemoryStore::new();
        m.record_run(
            "Add a healthcheck endpoint to the axum server",
            true,
            "added /health route",
        );
        m.record_run("Write marketing copy for the homepage", true, "done");
        let ctx = m.context_for("healthcheck endpoint for axum", 5).unwrap();
        assert!(ctx.contains("healthcheck"), "ctx: {ctx}");
        // The unrelated marketing memory should not dominate.
        assert!(!ctx.contains("marketing"), "ctx: {ctx}");
    }

    #[test]
    fn empty_store_returns_none() {
        let m = MemoryStore::new();
        assert!(m.context_for("anything", 5).is_none());
    }

    #[test]
    fn routing_policy_selects_lexical_for_simple_queries() {
        let m = MemoryStore::new();
        let (_features, tier) = m.retrieval_tier_for("weather");
        assert_eq!(tier, RetrievalTier::Low);
    }

    #[test]
    fn retrieval_records_effectiveness_for_injected_memories() {
        let m = MemoryStore::new();
        m.record_run(
            "Add a healthcheck endpoint to the axum server",
            true,
            "added /health route",
        );

        let ctx = m.context_for("healthcheck endpoint for axum", 5).unwrap();
        assert!(ctx.contains("healthcheck"), "ctx: {ctx}");
        assert_eq!(m.pending_injections.lock().unwrap().len(), 1);

        m.record_run("healthcheck endpoint for axum", false, "regressed");
        let history = m.effectiveness_history.lock().unwrap();
        assert_eq!(history.len(), 1);
        assert!(history[0].injected);
        assert!(!history[0].task_succeeded);
        drop(history);
        let lifecycle =
            m.db.lock()
                .prepare("SELECT event_kind FROM memory_event_log ORDER BY event_id")
                .and_then(|mut statement| {
                    statement
                        .query_map([], |row| row.get::<_, String>(0))
                        .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
                })
                .unwrap();
        assert!(lifecycle.iter().any(|event| event == "inject"));
        assert!(lifecycle.iter().any(|event| event == "outcome_failure"));
    }

    #[test]
    fn persists_across_instances() {
        let path = std::env::temp_dir().join(format!("cr-mem-test-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        {
            let db = MemDb::open(&path).unwrap();
            let m = MemoryStore::open(db);
            m.record_run("deploy the worker to cloudflare", true, "ok");
        }
        let db = MemDb::open(&path).unwrap();
        let reloaded = MemoryStore::open(db);
        assert_eq!(reloaded.len(), 1);
        assert!(reloaded
            .context_for("cloudflare worker deploy", 5)
            .is_some());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn delete_records_tombstone_and_put_clears_it() {
        let m = MemoryStore::new();
        m.put(
            "tomb",
            "content that will be deleted",
            "global",
            MemoryTier::Semantic,
        );
        assert!(m.delete("global/tomb"));
        let deleted_at: Option<String> =
            m.db.lock()
                .query_row(
                    "SELECT deleted_at FROM deletions WHERE id = 'global/tomb'",
                    [],
                    |r| r.get(0),
                )
                .ok();
        assert!(
            deleted_at.is_some(),
            "delete must record a deletion tombstone"
        );

        m.put(
            "tomb",
            "recreated after deletion",
            "global",
            MemoryTier::Semantic,
        );
        let tombstones: i64 =
            m.db.lock()
                .query_row(
                    "SELECT COUNT(*) FROM deletions WHERE id = 'global/tomb'",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
        assert_eq!(tombstones, 0, "re-put must clear the tombstone");
    }

    #[test]
    fn lifecycle_events_capture_surface_without_memory_content() {
        let m = MemoryStore::new();
        let context = MemoryEventContext::new("codex")
            .with_session("session-7")
            .with_turn("turn-8")
            .with_trace("trace-9");
        let id = m
            .try_put_observed(
                "observed",
                "secret body that must never enter event metadata",
                "global",
                MemoryTier::Semantic,
                &context,
            )
            .unwrap();
        m.record_injections_observed(std::slice::from_ref(&id), &context)
            .unwrap();
        m.get_full_observed(&id, &context).unwrap();
        assert!(m.try_delete_observed(&id, &context).unwrap());

        let conn = m.db.lock();
        let rows = conn
            .prepare(
                "SELECT event_kind, surface, session_id, trace_id, scope_id, quantity, meta
                 FROM memory_event_log ORDER BY event_id",
            )
            .and_then(|mut statement| {
                statement
                    .query_map([], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, Option<String>>(2)?,
                            row.get::<_, Option<String>>(3)?,
                            row.get::<_, Option<String>>(4)?,
                            row.get::<_, i64>(5)?,
                            row.get::<_, String>(6)?,
                        ))
                    })
                    .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
            })
            .unwrap();
        assert_eq!(
            rows.iter().map(|row| row.0.as_str()).collect::<Vec<_>>(),
            vec!["put", "inject", "get", "delete"]
        );
        assert!(rows.iter().all(|row| row.1 == "codex"));
        let session = opaque_correlation_token("session-7", "session");
        let trace = opaque_correlation_token("trace-9", "trace");
        assert!(rows
            .iter()
            .all(|row| row.2.as_deref() == Some(session.as_str())));
        assert!(rows
            .iter()
            .all(|row| row.3.as_deref() == Some(trace.as_str())));
        assert!(rows.iter().all(|row| row.4.as_deref() == Some("global")));
        assert!(rows.iter().all(|row| row.5 == 1));
        let encoded = serde_json::to_string(&rows).unwrap();
        assert!(!encoded.contains("secret body"));

        let event_identity = conn
            .query_row(
                "SELECT event_uid, installation_id, client, turn_id, artifact_id, identity_status
                 FROM memory_event_log WHERE event_kind='put' AND memory_id=?1",
                rusqlite::params![id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                    ))
                },
            )
            .unwrap();
        assert!(event_identity.0.starts_with("memory-event."));
        assert!(!event_identity.1.is_empty());
        assert_eq!(event_identity.2, "codex");
        assert_eq!(event_identity.3, opaque_correlation_token("turn-8", "turn"));
        assert_eq!(event_identity.4, stable_artifact_id(&id));
        assert_eq!(event_identity.5, "observed");

        let canonical_identity = conn
            .query_row(
                "SELECT artifact_id, origin_event_uid, installation_id, client,
                        session_id, turn_id, trace_id, identity_status
                 FROM memory_identity WHERE memory_id=?1",
                rusqlite::params![id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, String>(6)?,
                        row.get::<_, String>(7)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(canonical_identity.0, stable_artifact_id(&id));
        assert_eq!(canonical_identity.1, event_identity.0);
        assert_eq!(canonical_identity.2, event_identity.1);
        assert_eq!(canonical_identity.3, "codex");
        assert_eq!(canonical_identity.4, session);
        assert_eq!(
            canonical_identity.5,
            opaque_correlation_token("turn-8", "turn")
        );
        assert_eq!(canonical_identity.6, trace);
        assert_eq!(canonical_identity.7, "observed");
    }

    #[test]
    fn recall_events_capture_complete_decision_identity() {
        let attribution = OperationAttribution::new(
            "047e84c2-0c46-4a73-a0fc-41e9a3d25d10",
            "a4881e30-22fe-4f89-9f5b-3fe891993617",
            "ws.047e84c20c464a73a0fc41e9a3d25d10",
        )
        .unwrap();
        let store =
            MemoryStore::open_with_operation_attribution(MemDb::open_in_memory(), attribution);
        store.log_recall(
            "2026-07-21T00:00:00Z",
            Some("D--Claude"),
            100,
            1,
            500,
            100,
            "serve",
            None,
            Some("codex"),
            Some("session-codex-1"),
            None,
            Some("UserPromptSubmit"),
            Some("trace-codex-1"),
            Some("all"),
        );
        let row: (
            String,
            String,
            String,
            String,
            String,
            String,
            String,
            String,
        ) = store
            .db
            .lock()
            .query_row(
                "SELECT event_uid, installation_id, service_instance_id, client,
                        session_id, turn_id, trace_id, identity_status
                 FROM recall_log LIMIT 1",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                        row.get(7)?,
                    ))
                },
            )
            .unwrap();
        assert!(row.0.starts_with("recall."));
        assert_eq!(row.1, "047e84c2-0c46-4a73-a0fc-41e9a3d25d10");
        assert_eq!(row.2, "a4881e30-22fe-4f89-9f5b-3fe891993617");
        assert_eq!(row.3, "codex");
        assert_eq!(
            row.4,
            opaque_correlation_token("session-codex-1", "session")
        );
        assert_eq!(
            row.5,
            opaque_correlation_token(&opaque_correlation_token("trace-codex-1", "trace"), "turn",)
        );
        assert_eq!(row.6, opaque_correlation_token("trace-codex-1", "trace"));
        assert_eq!(row.7, "observed");
    }

    #[test]
    fn blank_trace_still_links_one_canonical_read_attempt() {
        let store = MemoryStore::new();
        let id = store.remember("blank trace target", vec![]).id;
        let context = MemoryEventContext::new("http").with_trace("");
        store.get_full_observed(&id, &context).unwrap();

        let conn = store.db.lock();
        let traces: Vec<String> = conn
            .prepare(
                "SELECT DISTINCT trace_id FROM context_event_log
                  WHERE operation='read' AND phase IN
                    ('provider.started','candidate.retrieved','provider.terminal')",
            )
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(traces.len(), 1);
        assert!(traces[0].starts_with("trace-") && traces[0].len() == 38);
    }

    #[test]
    fn second_put_is_recorded_as_update() {
        let m = MemoryStore::new();
        let context = MemoryEventContext::new("claude");
        m.try_put_observed(
            "same",
            "first body",
            "global",
            MemoryTier::Semantic,
            &context,
        )
        .unwrap();
        m.try_put_observed(
            "same",
            "second body",
            "global",
            MemoryTier::Semantic,
            &context,
        )
        .unwrap();
        let kinds =
            m.db.lock()
                .prepare("SELECT event_kind FROM memory_event_log ORDER BY event_id")
                .and_then(|mut statement| {
                    statement
                        .query_map([], |row| row.get::<_, String>(0))
                        .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
                })
                .unwrap();
        assert_eq!(kinds, vec!["put", "update"]);
    }

    #[test]
    fn context_policy_assignment_is_deterministic_and_persisted() {
        let m = MemoryStore::new();
        let first = m
            .assign_context_policy("session-42", "codex", "crypt-v1", 10, "code")
            .unwrap();
        let second = m
            .assign_context_policy("session-42", "codex", "crypt-v1", 10, "code")
            .unwrap();
        assert_eq!(first, second);
        assert!(matches!(first.as_str(), "control" | "candidate"));
        let count: i64 =
            m.db.lock()
                .query_row(
                    "SELECT COUNT(*) FROM context_policy_assignment
                 WHERE session_id='session-42' AND surface='codex'
                   AND policy_version='crypt-v1'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
        assert_eq!(count, 1);
        let task_class: String =
            m.db.lock()
                .query_row(
                    "SELECT task_class FROM context_policy_assignment
                 WHERE session_id='session-42' AND surface='codex'
                   AND policy_version='crypt-v1'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
        assert_eq!(task_class, "code");
        assert!(m
            .assign_context_policy("", "codex", "crypt-v1", 10, "code")
            .is_err());
        assert!(m
            .assign_context_policy("s", "codex", "crypt-v1", 101, "code")
            .is_err());
    }

    #[test]
    fn put_and_tombstone_cleanup_roll_back_together() {
        let m = MemoryStore::new();
        m.db.lock()
            .execute(
                "INSERT INTO deletions (id, deleted_at) VALUES ('global/atomic-put', 'now')",
                [],
            )
            .unwrap();
        m.db.lock()
            .execute_batch(
                "CREATE TRIGGER fail_tombstone_cleanup BEFORE DELETE ON deletions
                 BEGIN SELECT RAISE(ABORT, 'cleanup failed'); END;",
            )
            .unwrap();

        let err = m
            .try_put(
                "atomic-put",
                "must roll back",
                "global",
                MemoryTier::Semantic,
            )
            .expect_err("cleanup failure must surface");
        assert!(err.contains("cleanup failed"), "err: {err}");
        let memories: i64 =
            m.db.lock()
                .query_row(
                    "SELECT COUNT(*) FROM memories WHERE id = 'global/atomic-put'",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
        assert_eq!(memories, 0, "memory insert must roll back with cleanup");
    }

    #[test]
    fn delete_and_tombstone_roll_back_together() {
        let m = MemoryStore::new();
        let id = m
            .try_put(
                "atomic-delete",
                "must survive a tombstone failure",
                "global",
                MemoryTier::Semantic,
            )
            .unwrap();
        m.db.lock()
            .execute_batch(
                "CREATE TRIGGER fail_tombstone_insert BEFORE INSERT ON deletions
                 BEGIN SELECT RAISE(ABORT, 'tombstone failed'); END;",
            )
            .unwrap();

        let err = m
            .try_delete(&id)
            .expect_err("tombstone failure must surface");
        assert!(err.contains("tombstone failed"), "err: {err}");
        let memories: i64 =
            m.db.lock()
                .query_row("SELECT COUNT(*) FROM memories WHERE id = ?1", [&id], |r| {
                    r.get(0)
                })
                .unwrap();
        assert_eq!(memories, 1, "delete must roll back with tombstone insert");
        assert!(m.registry.read().unwrap().get(&id).is_some());
    }

    #[test]
    fn injection_batch_is_atomic_and_reports_errors() {
        let m = MemoryStore::new();
        let first = m.remember("first injection", vec!["first".into()]);
        let second = m.remember("second injection", vec!["second".into()]);
        m.db.lock()
            .execute_batch(&format!(
                "CREATE TRIGGER fail_second_injection BEFORE UPDATE OF inject_count ON memories
                 WHEN OLD.id = '{}' BEGIN SELECT RAISE(ABORT, 'inject failed'); END;",
                second.id.replace('\'', "''")
            ))
            .unwrap();

        let err = m
            .record_injections(&[first.id.clone(), second.id.clone()])
            .expect_err("injection failure must surface");
        assert!(err.contains("inject failed"), "err: {err}");
        assert_eq!(m.inject_count(&first.id), 0, "first update must roll back");
        assert_eq!(m.inject_count(&second.id), 0);
        assert!(
            m.health().last_persist_error.is_some(),
            "injection failure must be reported"
        );
    }

    #[test]
    fn dream_embedding_does_not_hold_the_memdb_lock() {
        struct BlockingEmbedder {
            entered: std::sync::mpsc::Sender<()>,
            release: Mutex<std::sync::mpsc::Receiver<()>>,
        }

        impl Embedder for BlockingEmbedder {
            fn embed(&self, _text: &str) -> Vec<f32> {
                self.entered.send(()).unwrap();
                self.release.lock().unwrap().recv().unwrap();
                vec![1.0, 0.0]
            }

            fn dim(&self) -> usize {
                2
            }

            fn model_id(&self) -> &'static str {
                "blocking-test"
            }
        }

        let mut store = MemoryStore::new();
        store
            .try_put(
                "dream-lock-a",
                "Duplicate dream lock memory.",
                "global",
                MemoryTier::Episodic,
            )
            .unwrap();
        store
            .try_put(
                "dream-lock-b",
                "Duplicate dream lock memory!",
                "global",
                MemoryTier::Episodic,
            )
            .unwrap();

        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        store.embedder = Arc::new(BlockingEmbedder {
            entered: entered_tx,
            release: Mutex::new(release_rx),
        });

        let read_db = store.db.clone();
        let dream = std::thread::spawn(move || store.dream_now("2026-07-10"));
        entered_rx
            .recv_timeout(std::time::Duration::from_secs(1))
            .expect("dream must reach embedding");

        let (read_started_tx, read_started_rx) = std::sync::mpsc::channel();
        let (read_tx, read_rx) = std::sync::mpsc::channel();
        let read = std::thread::spawn(move || {
            read_started_tx.send(()).unwrap();
            let result = read_db
                .lock()
                .query_row("SELECT COUNT(*) FROM memories", [], |row| {
                    row.get::<_, i64>(0)
                });
            read_tx.send(result).unwrap();
        });
        read_started_rx
            .recv_timeout(std::time::Duration::from_secs(1))
            .expect("DB read thread must start");
        let while_embedding_blocked = read_rx.recv_timeout(std::time::Duration::from_millis(250));
        let completed_while_blocked = while_embedding_blocked.is_ok();

        release_tx.send(()).unwrap();
        let status = dream.join().unwrap().unwrap();
        let read_result = match while_embedding_blocked {
            Ok(result) => result,
            Err(_) => read_rx
                .recv_timeout(std::time::Duration::from_secs(1))
                .expect("DB read must finish after dream embedding is released"),
        };
        read.join().unwrap();

        assert_eq!(status.consolidated_count, 1);
        assert!(
            completed_while_blocked,
            "DB read was blocked while dream embedding held the MemDb lock"
        );
        assert_eq!(read_result.unwrap(), 2);
    }

    #[test]
    fn dream_aborts_if_the_corpus_changes_while_embeddings_are_prepared() {
        struct BlockingEmbedder {
            entered: std::sync::mpsc::Sender<()>,
            release: Mutex<std::sync::mpsc::Receiver<()>>,
        }

        impl Embedder for BlockingEmbedder {
            fn embed(&self, _text: &str) -> Vec<f32> {
                self.entered.send(()).unwrap();
                self.release.lock().unwrap().recv().unwrap();
                vec![1.0, 0.0]
            }

            fn dim(&self) -> usize {
                2
            }

            fn model_id(&self) -> &'static str {
                "blocking-stale-dream-test"
            }
        }

        let mut store = MemoryStore::new();
        store
            .try_put(
                "dream-stale-a",
                "Duplicate stale dream memory.",
                "global",
                MemoryTier::Episodic,
            )
            .unwrap();
        let deleted_id = store
            .try_put(
                "dream-stale-b",
                "Duplicate stale dream memory!",
                "global",
                MemoryTier::Episodic,
            )
            .unwrap();

        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        store.embedder = Arc::new(BlockingEmbedder {
            entered: entered_tx,
            release: Mutex::new(release_rx),
        });

        let dream_store = store.clone();
        let dream = std::thread::spawn(move || dream_store.dream_now("2026-07-10"));
        entered_rx
            .recv_timeout(std::time::Duration::from_secs(1))
            .expect("dream must reach embedding");
        assert!(store.try_delete(&deleted_id).unwrap());
        release_tx.send(()).unwrap();

        let error = dream
            .join()
            .unwrap()
            .expect_err("a stale dream plan must not commit");
        assert!(error.contains("changed"), "unexpected error: {error}");
        assert!(store.dream_status().status.starts_with("failed:"));
        assert!(store.registry.read().unwrap().get(&deleted_id).is_none());
        let rows: i64 = store
            .db
            .lock()
            .query_row(
                "SELECT COUNT(*) FROM memories WHERE id LIKE 'global/dream-stale-%'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            rows, 1,
            "stale consolidation must not resurrect the deleted row"
        );
    }

    #[test]
    fn dream_consolidation_and_pruning_are_one_transaction() {
        let m = MemoryStore::new();
        let a = m
            .try_put(
                "dream-a",
                "Duplicate atomic dream memory.",
                "global",
                MemoryTier::Episodic,
            )
            .unwrap();
        let b = m
            .try_put(
                "dream-b",
                "Duplicate atomic dream memory!",
                "global",
                MemoryTier::Episodic,
            )
            .unwrap();
        m.db.lock()
            .execute_batch(
                "CREATE TRIGGER fail_dream_tombstone BEFORE INSERT ON deletions
                 BEGIN SELECT RAISE(ABORT, 'dream tombstone failed'); END;",
            )
            .unwrap();

        assert!(m.dream_now("2026-07-10").is_err());
        let rows: i64 =
            m.db.lock()
                .query_row(
                    "SELECT COUNT(*) FROM memories WHERE id IN (?1, ?2)",
                    rusqlite::params![a, b],
                    |r| r.get(0),
                )
                .unwrap();
        assert_eq!(rows, 2, "failed dream must preserve every source row");
    }

    #[test]
    fn dream_persists_consolidation_source_ids() {
        let m = MemoryStore::new();
        let a = m
            .try_put(
                "provenance-a",
                "Duplicate provenance memory.",
                "global",
                MemoryTier::Episodic,
            )
            .unwrap();
        let b = m
            .try_put(
                "provenance-b",
                "Duplicate provenance memory!",
                "global",
                MemoryTier::Episodic,
            )
            .unwrap();
        m.dream_now("2026-07-10").unwrap();

        let source_ids: String =
            m.db.lock()
                .query_row("SELECT source_ids FROM memories WHERE id = ?1", [&a], |r| {
                    r.get(0)
                })
                .unwrap();
        let ids: Vec<String> = serde_json::from_str(&source_ids).unwrap();
        assert_eq!(ids, vec![a, b]);
    }

    #[test]
    fn dream_consolidation_is_in_place_preserving_id_scope_and_counters() {
        let m = MemoryStore::new();
        m.put(
            "dated",
            "today the deploy runbook changed",
            "D--Claude-testproj",
            MemoryTier::Semantic,
        );
        m.db.lock()
            .execute(
                "UPDATE memories SET access_count = 7, inject_count = 4 WHERE id = 'D--Claude-testproj/dated'",
                [],
            )
            .unwrap();
        {
            let mut registry = m.registry.write().unwrap();
            let mut entry = registry
                .remove("D--Claude-testproj/dated")
                .expect("resident registry entry");
            entry.access_count = 7;
            registry.insert(entry);
        }
        let source_date: String = m
            .db
            .lock()
            .query_row(
                "SELECT substr(created_at, 1, 10) FROM memories WHERE id = 'D--Claude-testproj/dated'",
                [],
                |r| r.get(0),
            )
            .unwrap();

        let status = m.dream_now("2026-06-29").unwrap();
        assert!(status.consolidated_count >= 1);

        let (id, scope, content, access, inject): (String, String, String, i64, i64) =
            m.db.lock()
                .query_row(
                    "SELECT id, scope_id, content, access_count, inject_count FROM memories \
                 WHERE id = 'D--Claude-testproj/dated'",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
                )
                .expect("primary row must survive under its own id");
        assert_eq!(id, "D--Claude-testproj/dated");
        assert_eq!(scope, "D--Claude-testproj", "scope must never be rewritten");
        assert!(
            content.contains(&source_date),
            "dates normalized: {content}"
        );
        assert_eq!(access, 7, "access_count preserved");
        assert_eq!(inject, 4, "inject_count preserved");

        let dreamers: i64 =
            m.db.lock()
                .query_row(
                    "SELECT COUNT(*) FROM memories WHERE id LIKE '%dream-%'",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
        assert_eq!(dreamers, 0, "no generated dream- ids anymore");
    }

    #[test]
    fn dream_quarantines_low_value_entries_and_can_restore_them() {
        let m = MemoryStore::new();
        let low = m
            .remember_consolidated("low", "Low-value stale memory", vec![], 0.1)
            .unwrap();
        let status = m.dream_now("2026-06-29").unwrap();
        assert_eq!(status.pruned_count, 0);
        assert_eq!(status.quarantined_count, 1);
        assert_eq!(m.quarantined_ids(), vec![low.id.clone()]);
        let tombstones: i64 =
            m.db.lock()
                .query_row("SELECT COUNT(*) FROM deletions", [], |r| r.get(0))
                .unwrap();
        assert_eq!(
            tombstones, 0,
            "quarantine must not emit a deletion tombstone"
        );
        assert!(m.entries(10).iter().all(|entry| entry.id != low.id));

        assert!(m.restore_quarantined(&low.id).unwrap());
        assert!(m.quarantined_ids().is_empty());
        assert!(m.entries(10).iter().any(|entry| entry.id == low.id));
    }

    #[test]
    fn dream_does_not_quarantine_an_injected_preview_as_never_used() {
        let m = MemoryStore::new();
        let low = m
            .remember_consolidated("low", "Low-value previewed memory", vec![], 0.1)
            .unwrap();
        m.record_injections(std::slice::from_ref(&low.id)).unwrap();

        let status = m.dream_now("2026-06-29").unwrap();

        assert_eq!(status.quarantined_count, 0);
        assert!(m.quarantined_ids().is_empty());
        assert!(m.entries(10).iter().any(|entry| entry.id == low.id));
        assert_eq!(m.inject_count(&low.id), 1);
    }

    #[test]
    fn dream_protects_historical_rows_with_unknown_exposure_status() {
        let m = MemoryStore::new();
        let low = m
            .remember_consolidated(
                "legacy-low",
                "Legacy row without exposure history",
                vec![],
                0.1,
            )
            .unwrap();
        {
            let conn = m.db.lock();
            conn.execute(
                "DELETE FROM memory_event_log WHERE memory_id = ?1",
                rusqlite::params![&low.id],
            )
            .unwrap();
        }

        let status = m.dream_now("2026-06-29").unwrap();

        assert_eq!(status.quarantined_count, 0);
        assert!(m.quarantined_ids().is_empty());
        assert!(m.entries(10).iter().any(|entry| entry.id == low.id));
    }

    #[test]
    fn dream_now_consolidates_duplicates_and_prunes_low_value_entries() {
        let m = MemoryStore::new();
        m.remember("Use cargo fmt before cargo test.", vec!["cargo".into()]);
        m.remember(" use cargo fmt before cargo test! ", vec!["cargo".into()]);
        m.remember_consolidated("low", "Low-value stale memory", vec![], 0.1);

        let status = m.dream_now("2026-06-29").unwrap();

        assert!(status.model.is_empty());
        assert!(!status.shell_allowed);
        assert_eq!(status.consolidated_count, 1);
        assert!(status.pruned_count >= 1);
        assert!(status.quarantined_count >= 1);
        assert_eq!(m.search("cargo fmt cargo test", 10).len(), 1);
    }

    #[test]
    fn dream_now_records_status_for_agent_roster_surface() {
        let m = MemoryStore::new();
        m.remember(
            "Yesterday cargo test failed but today cargo test passed.",
            vec!["cargo".into()],
        );

        let status = m.dream_now("2026-06-29").unwrap();
        let stored = m.dream_status();

        assert_eq!(stored, status);
        assert_eq!(stored.status, "completed");
        assert_eq!(stored.read_count, 1);
    }

    #[test]
    fn use_persists_across_reopen() {
        let path = std::env::temp_dir().join(format!("cr-mem-use-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let id: String;
        {
            let m = MemoryStore::open(MemDb::open(&path).unwrap());
            let entry = m.remember("Remember this.", vec!["remember".into()]);
            id = entry.id.clone();
            m.record_use(&id).unwrap();
        }
        let reloaded = MemoryStore::open(MemDb::open(&path).unwrap());
        let reg = reloaded.registry.read().unwrap();
        let entry = reg.get(&id).expect("entry reloaded");
        assert_eq!(entry.access_count, 1);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn recall_increments_inject_count_persisted() {
        let path = std::env::temp_dir().join(format!("cr-mem-inject-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let id: String;
        {
            let m = MemoryStore::open(MemDb::open(&path).unwrap());
            let entry = m.remember("Inject me please.", vec!["inject".into()]);
            id = entry.id.clone();
            #[allow(clippy::cloned_ref_to_slice_refs)]
            m.record_injections(std::slice::from_ref(&id)).unwrap();
            m.record_injections(std::slice::from_ref(&id)).unwrap();
            assert_eq!(m.inject_count(&id), 2);
        }
        let reloaded = MemoryStore::open(MemDb::open(&path).unwrap());
        assert_eq!(reloaded.inject_count(&id), 2, "inject_count must persist");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn get_returns_full_and_records_use() {
        let m = MemoryStore::new();
        let entry = m.remember("Full content line one.\nLine two.", vec!["full".into()]);
        let (content, access) = m.get_full(&entry.id).unwrap();
        assert!(content.contains("Line two."), "full content, not a preview");
        assert_eq!(access, 1, "the fetch IS the use signal");
        assert!(m.get_full("no-such-id").is_err());
    }

    /// L8 dream_now on an empty store should still return a well-formed
    /// status — no panic, no missing fields, zero counts. This is the entry
    /// point for the `crypt curate` CLI verb (L8 in the plan); without
    /// this test, regressions in the no-op path would only surface when a
    /// user actually runs the CLI on a fresh DB.
    #[test]
    fn dream_now_on_empty_store_returns_zero_status() {
        let path = std::env::temp_dir().join(format!("cr-mem-curate-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let m = MemoryStore::open(MemDb::open(&path).unwrap());
        let status = m.dream_now("2026-07-01").expect("dream_now on empty store");
        assert_eq!(status.read_count, 0);
        assert_eq!(status.consolidated_count, 0);
        assert_eq!(status.pruned_count, 0);
        // Status string is one of the canonical values (the impl returns
        // "completed" for an empty-store pass; we assert it's non-empty
        // rather than hard-coding so the test isn't brittle if the status
        // vocabulary grows).
        assert!(!status.status.is_empty(), "status string should be set");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn lifecycle_actor_rejects_body_style_identity_and_a0() {
        assert!(
            VerifiedMemoryActor::from_execution_context("", "A1", "loopback", "s", "t").is_err()
        );
        assert!(
            VerifiedMemoryActor::from_execution_context("agent", "A0", "loopback", "s", "t")
                .is_err()
        );
        assert!(VerifiedMemoryActor::from_execution_context(
            "agent",
            "A1",
            "request_body",
            "s",
            "t"
        )
        .is_err());
    }

    #[test]
    fn lifecycle_supersede_hash_guard_and_receipt_are_content_free() {
        let store = MemoryStore::new();
        store
            .try_put("old", "old value", "global", MemoryTier::Semantic)
            .unwrap();
        store
            .try_put("new", "new value", "global", MemoryTier::Semantic)
            .unwrap();
        let actor = VerifiedMemoryActor::from_execution_context(
            "reviewer",
            "A1",
            "loopback",
            "session-1",
            "trace-1",
        )
        .unwrap();
        let request = MemoryLifecycleOperationV1 {
            operation: MemoryLifecycleOperation::Supersede,
            memory_id: "global/old".into(),
            replacement_id: Some("global/new".into()),
            scope_id: Some("global".into()),
            expected_content_sha256: Some(content_hash("old value")),
            reason_ref: "review-1".into(),
            content: None,
            tier: None,
        };
        let receipt = store.execute_lifecycle_operation(&request, &actor).unwrap();
        assert_eq!(receipt.status, "superseded");
        assert!(receipt.reversible);
        assert_eq!(receipt.actor, "reviewer");
        assert!(!serde_json::to_string(&receipt)
            .unwrap()
            .contains("old value"));
        let wrong = MemoryLifecycleOperationV1 {
            expected_content_sha256: Some(content_hash("spoofed")),
            ..request
        };
        assert!(store.execute_lifecycle_operation(&wrong, &actor).is_err());
    }
}
