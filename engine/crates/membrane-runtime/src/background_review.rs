//! Tray-owned scheduling mechanics for bounded background review.
//!
//! This module owns eligibility, budgets, cancellation, content-free H5
//! observations, and the fail-closed semantic learner boundary. Learners may
//! return opaque proposal references only; Cortex writes remain outside it.

use cortex_core::review::{
    bound_memory_candidate_extraction_window_with_state, select_review_input,
    validate_memory_candidates_for_window, validate_semantic_curation_proposals,
    DeterministicReviewLimitsV1, ForegroundMemoryEmissionV1, ForegroundMemoryStateV1,
    MemoryCandidateExtractionDecisionV1,
    MemoryCandidateExtractionLimitsV1, MemoryCandidateV1, ReviewInputSelectionLimitsV1,
    SemanticCurationProposalV1, DETERMINISTIC_REVIEW_ANALYZER_ID,
    DETERMINISTIC_REVIEW_ANALYZER_VERSION, REVIEW_INPUT_NOVELTY_FLOOR,
};
use cortex_core::{EventCursor, SessionEvent};
use membrane_protocol::background_review::{
    BackgroundReviewCursorV1, BackgroundReviewEventRangeV1,
    BackgroundReviewForegroundMemoryStateV1, BackgroundReviewInputSelectionCandidateV1,
    BackgroundReviewInputSelectionSkippedV1, BackgroundReviewInputSelectionSkipReasonV1,
    BackgroundReviewInputSelectionV1, BackgroundReviewProvenanceRefV1,
    BackgroundReviewSessionEventV1, BackgroundSemanticReviewRequestV1,
    BackgroundSemanticReviewResultV1, BackgroundSemanticReviewStatusV1,
};
use membrane_protocol::{
    canonical_json_of, digest_str, BackgroundReviewActivitySignalV1, BackgroundReviewConfigError,
    BackgroundReviewConfigV1, BackgroundReviewExecutionStatusV1, BackgroundReviewExecutionV1,
    BackgroundReviewJobKindV1, BackgroundReviewJobV1, BackgroundReviewObservationReceiptV1,
    BackgroundReviewObservationV1, BackgroundReviewProposalRefV1, BackgroundReviewReasonV1,
    BackgroundReviewStatusV1,
};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    fs,
    io::{Read, Write},
    net::{IpAddr, SocketAddr, TcpStream},
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// Environment variable overriding workspace-relative review configuration.
pub const CONFIG_PATH_ENV: &str = "MEMBRANE_BACKGROUND_REVIEW_CONFIG";
/// Default fail-closed configuration location beneath workspace root.
pub const DEFAULT_CONFIG_RELATIVE_PATH: &str = ".membrane/background-review.json";
/// One initial attempt plus one retry is permitted for each job id.
pub const MAX_ATTEMPTS: u8 = 2;

/// Result of asking scheduler to admit one job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackgroundReviewDecision {
    Started { attempt: u8 },
    Deferred { reason: BackgroundReviewReasonV1 },
}

/// Terminal result supplied when producer finishes one admitted job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackgroundReviewCompletion {
    Completed,
    Failed,
    FailedWithReason(BackgroundReviewReasonV1),
}

/// Result of converting one host activity signal into one scheduler decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackgroundReviewProduction {
    Started {
        job: BackgroundReviewJobV1,
        attempt: u8,
    },
    Deferred {
        reason: BackgroundReviewReasonV1,
    },
}

/// Learner output is intentionally limited to opaque proposal references. A
/// semantic learner cannot write Cortex through this interface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackgroundReviewLearnerResult {
    Proposals(Vec<BackgroundReviewProposalRefV1>),
    Blocked(BackgroundReviewReasonV1),
    Failed(BackgroundReviewReasonV1),
}

/// Semantic learner boundary. Implementations own model/provider access; this
/// runtime never synthesizes learner output.
pub trait BackgroundReviewLearner: Send + Sync {
    fn execute(&self, job: &BackgroundReviewJobV1) -> BackgroundReviewLearnerResult;
}

/// Proposal handoff boundary. Implementations may forward opaque references to
/// a governed proposal/admission path, but cannot receive a Cortex store here.
pub trait BackgroundReviewProposalSink: Send + Sync {
    fn submit(
        &self,
        job: &BackgroundReviewJobV1,
        proposals: &[BackgroundReviewProposalRefV1],
    ) -> Result<(), BackgroundReviewReasonV1>;
}

/// Default learner while no production semantic provider is wired.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoSemanticReviewLearner;

impl BackgroundReviewLearner for NoSemanticReviewLearner {
    fn execute(&self, _job: &BackgroundReviewJobV1) -> BackgroundReviewLearnerResult {
        BackgroundReviewLearnerResult::Blocked(BackgroundReviewReasonV1::SemanticProviderNotWired)
    }
}

/// Bounded durable sink for content-free H5 receipts. The path is supplied by
/// the caller so storage ownership remains outside the learner.
pub trait BackgroundReviewObservationSink: Send + Sync {
    fn append(
        &self,
        receipts: &[BackgroundReviewObservationReceiptV1],
    ) -> Result<(), BackgroundReviewSinkError>;
}

#[derive(Debug, thiserror::Error)]
pub enum BackgroundReviewSinkError {
    #[error("background review observation is invalid: {0}")]
    Invalid(String),
    #[error("background review observation JSON could not be encoded: {0}")]
    Encode(String),
    #[error("background review observation sink received too many records")]
    BatchLimit,
    #[error("background review observation exceeds the record byte limit")]
    RecordLimit,
    #[error("background review observation sink is full")]
    FileLimit,
    #[error("background review observation sink IO failed: {0}")]
    Io(String),
}

/// JSONL sink used by the daemon when a durable H5 sidecar is configured.
#[derive(Debug, Clone)]
pub struct JsonlBackgroundReviewObservationSink {
    path: PathBuf,
}

/// Environment variable overriding the H5 observation sidecar path.
pub const OBSERVATIONS_PATH_ENV: &str = "MEMBRANE_BACKGROUND_REVIEW_OBSERVATIONS";
/// Default H5 observation sidecar beneath workspace root.
pub const DEFAULT_OBSERVATIONS_RELATIVE_PATH: &str =
    ".membrane/background-review-observations.jsonl";
/// Maximum number of receipts written by one append call.
pub const MAX_OBSERVATION_BATCH: usize = 256;
/// Maximum serialized receipt size including no trailing newline.
pub const MAX_OBSERVATION_RECORD_BYTES: usize = 16 * 1024;
/// Maximum durable H5 sidecar size.
pub const MAX_OBSERVATION_FILE_BYTES: u64 = 1024 * 1024;

impl JsonlBackgroundReviewObservationSink {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn from_workspace_root(workspace_root: impl AsRef<Path>) -> Self {
        let path = std::env::var_os(OBSERVATIONS_PATH_ENV)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                workspace_root
                    .as_ref()
                    .join(DEFAULT_OBSERVATIONS_RELATIVE_PATH)
            });
        Self::new(path)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl BackgroundReviewObservationSink for JsonlBackgroundReviewObservationSink {
    fn append(
        &self,
        receipts: &[BackgroundReviewObservationReceiptV1],
    ) -> Result<(), BackgroundReviewSinkError> {
        if receipts.is_empty() {
            return Ok(());
        }
        if receipts.len() > MAX_OBSERVATION_BATCH {
            return Err(BackgroundReviewSinkError::BatchLimit);
        }
        let mut payload = Vec::new();
        for receipt in receipts {
            receipt
                .validate()
                .map_err(|error| BackgroundReviewSinkError::Invalid(error.to_string()))?;
            let line = serde_json::to_vec(receipt)
                .map_err(|error| BackgroundReviewSinkError::Encode(error.to_string()))?;
            if line.len() > MAX_OBSERVATION_RECORD_BYTES {
                return Err(BackgroundReviewSinkError::RecordLimit);
            }
            payload.extend_from_slice(&line);
            payload.push(b'\n');
        }
        let existing = fs::metadata(&self.path)
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        // The cap bounds disk, but refusing the write forever is not what
        // bounds it — rotation does. A full sidecar used to kill the
        // observation rail permanently and make the daemon log
        // `sink_unavailable` on every tick, with no recovery short of an
        // operator deleting the file. Keep one previous generation and
        // continue: at most two files, and the rail stays alive.
        if existing.saturating_add(payload.len() as u64) > MAX_OBSERVATION_FILE_BYTES {
            if payload.len() as u64 > MAX_OBSERVATION_FILE_BYTES {
                return Err(BackgroundReviewSinkError::FileLimit);
            }
            let rotated = self.path.with_extension("jsonl.1");
            fs::rename(&self.path, &rotated)
                .map_err(|error| BackgroundReviewSinkError::Io(error.to_string()))?;
        }
        if let Some(parent) = self
            .path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)
                .map_err(|error| BackgroundReviewSinkError::Io(error.to_string()))?;
        }
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|error| BackgroundReviewSinkError::Io(error.to_string()))?;
        file.write_all(&payload)
            .and_then(|_| file.sync_data())
            .map_err(|error| BackgroundReviewSinkError::Io(error.to_string()))
    }
}

/// Convert one admitted job into a typed learner execution, then close the
/// scheduler attempt without exposing semantic content to Cortex.
pub fn execute_background_review(
    scheduler: &BackgroundReviewScheduler,
    job: &BackgroundReviewJobV1,
    learner: &dyn BackgroundReviewLearner,
    proposal_sink: Option<&dyn BackgroundReviewProposalSink>,
    observed_at_unix_ms: u64,
) -> BackgroundReviewExecutionV1 {
    if !scheduler.is_active_job(&job.job_id) {
        return BackgroundReviewExecutionV1 {
            schema_version: BackgroundReviewExecutionV1::SCHEMA_VERSION,
            job_id: job.job_id.clone(),
            kind: job.kind,
            status: BackgroundReviewExecutionStatusV1::Blocked,
            proposals: Vec::new(),
            reason: Some(BackgroundReviewReasonV1::InvalidJob),
        };
    }

    let (status, proposals, reason, completion) = match learner.execute(job) {
        BackgroundReviewLearnerResult::Proposals(proposals) => {
            if proposals
                .iter()
                .any(|proposal| proposal.validate().is_err())
            {
                (
                    BackgroundReviewExecutionStatusV1::Failed,
                    Vec::new(),
                    Some(BackgroundReviewReasonV1::InvalidProposal),
                    BackgroundReviewCompletion::FailedWithReason(
                        BackgroundReviewReasonV1::InvalidProposal,
                    ),
                )
            } else if proposals.is_empty() {
                (
                    BackgroundReviewExecutionStatusV1::Proposals,
                    proposals,
                    None,
                    BackgroundReviewCompletion::Completed,
                )
            } else {
                match proposal_sink {
                    Some(sink) => match sink.submit(job, &proposals) {
                        Ok(()) => (
                            BackgroundReviewExecutionStatusV1::Proposals,
                            proposals,
                            None,
                            BackgroundReviewCompletion::Completed,
                        ),
                        Err(reason) => (
                            BackgroundReviewExecutionStatusV1::Failed,
                            Vec::new(),
                            Some(reason),
                            BackgroundReviewCompletion::FailedWithReason(reason),
                        ),
                    },
                    None => (
                        BackgroundReviewExecutionStatusV1::Failed,
                        Vec::new(),
                        Some(BackgroundReviewReasonV1::ProposalSinkUnavailable),
                        BackgroundReviewCompletion::FailedWithReason(
                            BackgroundReviewReasonV1::ProposalSinkUnavailable,
                        ),
                    ),
                }
            }
        }
        BackgroundReviewLearnerResult::Blocked(reason) => (
            BackgroundReviewExecutionStatusV1::Blocked,
            Vec::new(),
            Some(reason),
            BackgroundReviewCompletion::FailedWithReason(reason),
        ),
        BackgroundReviewLearnerResult::Failed(reason) => (
            BackgroundReviewExecutionStatusV1::Failed,
            Vec::new(),
            Some(reason),
            BackgroundReviewCompletion::FailedWithReason(reason),
        ),
    };
    let _ = scheduler.finish_with_completion(&job.job_id, completion, observed_at_unix_ms);
    BackgroundReviewExecutionV1 {
        schema_version: BackgroundReviewExecutionV1::SCHEMA_VERSION,
        job_id: job.job_id.clone(),
        kind: job.kind,
        status,
        proposals,
        reason,
    }
}

/// Activity-to-job adapter. It accepts only host-provided activity and token
/// measurements, and reports missing measurements as typed deferrals.
pub struct BackgroundReviewProducer<'a> {
    scheduler: &'a BackgroundReviewScheduler,
}

impl<'a> BackgroundReviewProducer<'a> {
    pub fn new(scheduler: &'a BackgroundReviewScheduler) -> Self {
        Self { scheduler }
    }

    pub fn admit(
        &self,
        signal: &BackgroundReviewActivitySignalV1,
        kind: BackgroundReviewJobKindV1,
        observed_at_unix_ms: u64,
    ) -> BackgroundReviewProduction {
        if signal.validate().is_err() {
            self.scheduler
                .observe_deferred(BackgroundReviewReasonV1::InvalidJob, observed_at_unix_ms);
            return BackgroundReviewProduction::Deferred {
                reason: BackgroundReviewReasonV1::InvalidJob,
            };
        }
        self.scheduler
            .set_foreground_active(signal.foreground_active, observed_at_unix_ms);
        self.scheduler.record_activity(signal.activity_units);
        let Some(input_tokens) = signal.input_tokens else {
            self.scheduler.observe_deferred(
                BackgroundReviewReasonV1::ModelInputUnavailable,
                observed_at_unix_ms,
            );
            return BackgroundReviewProduction::Deferred {
                reason: BackgroundReviewReasonV1::ModelInputUnavailable,
            };
        };
        let job = BackgroundReviewJobV1 {
            schema_version: BackgroundReviewJobV1::SCHEMA_VERSION,
            job_id: deterministic_job_id(signal, kind),
            kind,
            turn_id: signal.turn_id.clone(),
            input_tokens,
            requested_at_unix_ms: signal.observed_at_unix_ms,
        };
        match self.scheduler.start(job.clone(), observed_at_unix_ms) {
            BackgroundReviewDecision::Started { attempt } => {
                BackgroundReviewProduction::Started { job, attempt }
            }
            BackgroundReviewDecision::Deferred { reason } => {
                BackgroundReviewProduction::Deferred { reason }
            }
        }
    }
}

fn deterministic_job_id(
    signal: &BackgroundReviewActivitySignalV1,
    kind: BackgroundReviewJobKindV1,
) -> String {
    let basis = format!(
        "{}|{}|{}|{}",
        kind.as_str(),
        signal.session_id,
        signal.turn_id,
        signal.observed_at_unix_ms
    );
    format!(
        "background-job-{}",
        digest_str(&basis).trim_start_matches("sha256:")
    )
}

/// Environment-provided authenticated provider endpoint.  The endpoint is
/// deliberately loopback-only; the tray command channel remains lifecycle
/// only.
pub const BACKGROUND_SEMANTIC_PROVIDER_ENDPOINT_ENV: &str =
    "MEMBRANE_BACKGROUND_SEMANTIC_PROVIDER_ENDPOINT";
pub const BACKGROUND_SEMANTIC_PROVIDER_TOKEN_ENV: &str =
    "MEMBRANE_BACKGROUND_SEMANTIC_PROVIDER_TOKEN";
pub const BACKGROUND_SEMANTIC_PROVIDER_PATH: &str = "/membrane/background/semantic-review";
pub const BACKGROUND_SEMANTIC_PROVIDER_TIMEOUT_MS: u64 = 2_000;
pub const BACKGROUND_SEMANTIC_CAPABILITY: &str = "proposal_only";

/// Input assembled by daemon-owned source adapters. Events must already be
/// after cursor; request validation enforces this before provider execution.
#[derive(Debug, Clone, PartialEq)]
pub struct BackgroundSemanticReviewInputV1 {
    pub task_id: Option<String>,
    pub cursor: EventCursor,
    pub events: Vec<SessionEvent>,
    /// Events before the cursor form the deterministic nearest-neighbour
    /// baseline; they are never candidates for the current run.
    pub reviewed_baseline: Vec<SessionEvent>,
    pub foreground_memory_state: ForegroundMemoryStateV1,
}

impl BackgroundSemanticReviewInputV1 {
    pub fn session_id(&self) -> &str {
        &self.cursor.session_id
    }
}

/// Proposal-only boundary. Implementations may forward validated values to a
/// governed Cortex admission/review path, but cannot receive a durable store
/// through this interface.
pub trait BackgroundReviewProposalAdmission: Send + Sync {
    fn admit_curation(
        &self,
        _job: &BackgroundReviewJobV1,
        _proposals: &[SemanticCurationProposalV1],
    ) -> Result<(), BackgroundReviewReasonV1> {
        Err(BackgroundReviewReasonV1::ProposalSinkUnavailable)
    }

    fn admit_memory_candidates(
        &self,
        _job: &BackgroundReviewJobV1,
        _candidates: &[MemoryCandidateV1],
    ) -> Result<(), BackgroundReviewReasonV1> {
        Err(BackgroundReviewReasonV1::ProposalSinkUnavailable)
    }

    fn submit_curation(
        &self,
        job: &BackgroundReviewJobV1,
        proposals: &[SemanticCurationProposalV1],
    ) -> Result<(), BackgroundReviewReasonV1> {
        self.admit_curation(job, proposals)
    }

    fn submit_memory_candidates(
        &self,
        job: &BackgroundReviewJobV1,
        candidates: &[MemoryCandidateV1],
    ) -> Result<(), BackgroundReviewReasonV1> {
        self.admit_memory_candidates(job, candidates)
    }
}

pub use BackgroundReviewProposalAdmission as BackgroundSemanticProposalSink;

/// Proposal queue used when a deployment supplies no in-process Cortex
/// admission object. It records proposal payloads for a governed consumer; it
/// never writes a durable memory record.
#[derive(Debug, Clone)]
pub struct JsonlBackgroundReviewProposalAdmission {
    path: PathBuf,
}

pub const BACKGROUND_REVIEW_PROPOSALS_PATH_ENV: &str = "MEMBRANE_BACKGROUND_REVIEW_PROPOSALS";
pub const DEFAULT_PROPOSALS_RELATIVE_PATH: &str = ".membrane/background-review-proposals.jsonl";
pub const MAX_PROPOSAL_RECORD_BYTES: usize = 128 * 1024;
pub const MAX_PROPOSAL_FILE_BYTES: u64 = 8 * 1024 * 1024;

impl JsonlBackgroundReviewProposalAdmission {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn from_workspace_root(workspace_root: impl AsRef<Path>) -> Self {
        let path = std::env::var_os(BACKGROUND_REVIEW_PROPOSALS_PATH_ENV)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                workspace_root
                    .as_ref()
                    .join(DEFAULT_PROPOSALS_RELATIVE_PATH)
            });
        Self::new(path)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn append<T: serde::Serialize>(
        &self,
        job: &BackgroundReviewJobV1,
        kind: &'static str,
        values: &[T],
    ) -> Result<(), BackgroundReviewReasonV1> {
        if values.is_empty() {
            return Ok(());
        }
        let mut bytes = Vec::new();
        for value in values {
            let record = serde_json::json!({
                "schemaVersion": BackgroundReviewExecutionV1::SCHEMA_VERSION,
                "jobId": job.job_id.as_str(),
                "kind": kind,
                "proposal": value,
            });
            let line = serde_json::to_vec(&record)
                .map_err(|_| BackgroundReviewReasonV1::ProposalSinkUnavailable)?;
            if line.len() > MAX_PROPOSAL_RECORD_BYTES {
                return Err(BackgroundReviewReasonV1::ProposalSinkUnavailable);
            }
            bytes.extend_from_slice(&line);
            bytes.push(b'\n');
        }
        let existing = fs::metadata(&self.path)
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        if existing.saturating_add(bytes.len() as u64) > MAX_PROPOSAL_FILE_BYTES {
            return Err(BackgroundReviewReasonV1::ProposalSinkUnavailable);
        }
        if let Some(parent) = self
            .path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)
                .map_err(|_| BackgroundReviewReasonV1::ProposalSinkUnavailable)?;
        }
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|_| BackgroundReviewReasonV1::ProposalSinkUnavailable)?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_data())
            .map_err(|_| BackgroundReviewReasonV1::ProposalSinkUnavailable)
    }
}

impl BackgroundReviewProposalAdmission for JsonlBackgroundReviewProposalAdmission {
    fn admit_curation(
        &self,
        job: &BackgroundReviewJobV1,
        proposals: &[SemanticCurationProposalV1],
    ) -> Result<(), BackgroundReviewReasonV1> {
        self.append(job, "semantic_curation", proposals)
    }

    fn admit_memory_candidates(
        &self,
        job: &BackgroundReviewJobV1,
        candidates: &[MemoryCandidateV1],
    ) -> Result<(), BackgroundReviewReasonV1> {
        self.append(job, "memory_candidate", candidates)
    }
}

/// In-memory monotonic cursor used by one daemon lifetime. A caller may wrap
/// it with a durable cursor source, but cursor advancement remains gated by
/// successful proposal admission in [`execute_background_semantic_review`].
#[derive(Debug, Default)]
pub struct BackgroundReviewCursorStore {
    cursors: Mutex<HashMap<String, EventCursor>>,
}

impl BackgroundReviewCursorStore {
    pub fn get(&self, session_id: &str) -> Result<EventCursor, BackgroundReviewReasonV1> {
        if session_id.trim().is_empty() {
            return Err(BackgroundReviewReasonV1::CursorInputUnavailable);
        }
        let cursors = self
            .cursors
            .lock()
            .map_err(|_| BackgroundReviewReasonV1::CursorInputUnavailable)?;
        Ok(cursors
            .get(session_id)
            .cloned()
            .unwrap_or_else(|| EventCursor {
                session_id: session_id.to_string(),
                last_seq: 0,
            }))
    }

    pub fn set_initial(&self, cursor: EventCursor) -> Result<(), BackgroundReviewReasonV1> {
        if cursor.session_id.trim().is_empty() {
            return Err(BackgroundReviewReasonV1::CursorInputUnavailable);
        }
        let mut cursors = self
            .cursors
            .lock()
            .map_err(|_| BackgroundReviewReasonV1::CursorInputUnavailable)?;
        cursors.entry(cursor.session_id.clone()).or_insert(cursor);
        Ok(())
    }

    pub fn advance(
        &self,
        cursor: &BackgroundReviewCursorV1,
    ) -> Result<(), BackgroundReviewReasonV1> {
        cursor
            .validate()
            .map_err(|_| BackgroundReviewReasonV1::CursorInputUnavailable)?;
        let mut cursors = self
            .cursors
            .lock()
            .map_err(|_| BackgroundReviewReasonV1::CursorInputUnavailable)?;
        let current = cursors
            .entry(cursor.session_id.clone())
            .or_insert_with(|| EventCursor {
                session_id: cursor.session_id.clone(),
                last_seq: 0,
            });
        if cursor.last_seq < current.last_seq {
            return Err(BackgroundReviewReasonV1::CursorInputUnavailable);
        }
        current.last_seq = cursor.last_seq;
        Ok(())
    }
}

/// Typed provider transport failures. No failure is converted into an empty
/// semantic result.
#[derive(Debug, thiserror::Error)]
pub enum BackgroundSemanticReviewProviderError {
    #[error("background semantic provider configuration is unavailable: {0}")]
    Configuration(String),
    #[error("background semantic provider is unavailable: {0}")]
    Unavailable(String),
    #[error("background semantic provider request timed out")]
    Timeout,
    #[error("background semantic provider authentication failed")]
    Authentication,
    #[error("background semantic provider protocol failed: {0}")]
    Protocol(String),
    #[error("background semantic provider returned invalid result: {0}")]
    InvalidResult(String),
    #[error("background semantic provider request encoding failed: {0}")]
    Encode(String),
    #[error("background semantic provider IO failed: {0}")]
    Io(String),
}

/// One provider seam serves Adapt review, Cortex semantic Dream, and Cortex
/// memory extraction.
pub trait BackgroundSemanticReviewProvider: Send + Sync {
    fn execute(
        &self,
        request: &BackgroundSemanticReviewRequestV1,
    ) -> Result<BackgroundSemanticReviewResultV1, BackgroundSemanticReviewProviderError>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct NoBackgroundSemanticReviewProvider;

impl BackgroundSemanticReviewProvider for NoBackgroundSemanticReviewProvider {
    fn execute(
        &self,
        _request: &BackgroundSemanticReviewRequestV1,
    ) -> Result<BackgroundSemanticReviewResultV1, BackgroundSemanticReviewProviderError> {
        Err(BackgroundSemanticReviewProviderError::Unavailable(
            "semantic_provider_not_wired".into(),
        ))
    }
}

/// First-party, in-process, deterministic semantic-review provider.
///
/// It calls no model and opens no socket.  It runs Cortex's own deterministic
/// analyzer ([`cortex_core::review::analyze_events_for_curation`] and
/// [`cortex_core::review::extract_bounded_memory_candidate`]) over the
/// request's own event window and returns proposal-only candidates.  Nothing
/// here admits, writes, or advances durable truth: the governed proposal queue
/// in [`execute_background_semantic_review`] remains the only path onward.
///
/// It intentionally refuses [`BackgroundReviewJobKindV1::AdaptBehavioralReview`]:
/// a deterministic duplicate/assertion analyzer is not a behavioral learner and
/// must not pretend to be one.
pub struct DeterministicFirstPartySemanticReviewProvider {
    limits: DeterministicReviewLimitsV1,
    now_unix_ms: Option<u64>,
}

impl std::fmt::Debug for DeterministicFirstPartySemanticReviewProvider {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DeterministicFirstPartySemanticReviewProvider")
            .field("limits", &self.limits)
            .field("now_unix_ms", &self.now_unix_ms)
            .finish()
    }
}

impl Default for DeterministicFirstPartySemanticReviewProvider {
    fn default() -> Self {
        Self {
            limits: DeterministicReviewLimitsV1::default(),
            now_unix_ms: None,
        }
    }
}

impl DeterministicFirstPartySemanticReviewProvider {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_limits(mut self, limits: DeterministicReviewLimitsV1) -> Self {
        self.limits = limits;
        self
    }

    /// Pin the clock used for the deadline check and provenance timestamp.
    pub fn with_now_unix_ms(mut self, now_unix_ms: u64) -> Self {
        self.now_unix_ms = Some(now_unix_ms);
        self
    }

    /// Truthful analyzer identity.  This is not a model name.
    pub fn provider_label() -> String {
        format!(
            "{DETERMINISTIC_REVIEW_ANALYZER_ID}@{DETERMINISTIC_REVIEW_ANALYZER_VERSION}"
        )
    }

    fn now(&self) -> u64 {
        self.now_unix_ms.unwrap_or_else(unix_ms)
    }

    fn result(
        &self,
        request: &BackgroundSemanticReviewRequestV1,
        curation_proposals: Vec<serde_json::Value>,
        memory_candidates: Vec<serde_json::Value>,
        next_cursor: Option<BackgroundReviewCursorV1>,
        status: BackgroundSemanticReviewStatusV1,
    ) -> BackgroundSemanticReviewResultV1 {
        let observed_at_unix_ms = self.now();
        // The receipt binds this result to the exact request bytes it analyzed;
        // there is no upstream host receipt to point at because no external
        // producer was involved.
        let receipt_digest = digest_str(&canonical_json_of(request));
        let provenance_receipt = membrane_protocol::HostObservationProvenanceV1::new(
            format!("{DETERMINISTIC_REVIEW_ANALYZER_ID}-{}", request.job_id),
            Self::provider_label(),
            observed_at_unix_ms,
            receipt_digest,
        );
        BackgroundSemanticReviewResultV1 {
            schema_version: BackgroundSemanticReviewResultV1::SCHEMA_VERSION,
            job_id: request.job_id.clone(),
            job_kind: request.job_kind,
            session_id: request.session_id.clone(),
            task_id: request.task_id.clone(),
            turn_id: request.turn_id.clone(),
            curation_proposals,
            memory_candidates,
            next_cursor,
            // No model ran, and usage is not measured: claiming either would be
            // a fabrication.
            model: None,
            provider: Some(Self::provider_label()),
            usage: None,
            provenance_receipt,
            status,
        }
    }

    fn refuse(
        &self,
        request: &BackgroundSemanticReviewRequestV1,
        reason: BackgroundReviewReasonV1,
    ) -> BackgroundSemanticReviewResultV1 {
        self.result(
            request,
            Vec::new(),
            Vec::new(),
            None,
            BackgroundSemanticReviewStatusV1::Blocked { reason },
        )
    }
}

/// Flatten one wire event into the analyzer's protocol-neutral view.
fn analyzer_event(
    event: &BackgroundReviewSessionEventV1,
) -> cortex_core::review::DeterministicReviewEventV1 {
    cortex_core::review::DeterministicReviewEventV1 {
        event_id: event.event_id.clone(),
        seq: event.seq,
        scope_id: event.scope_id.clone(),
        content_hash: event.content_hash.clone(),
        text: cortex_core::review::review_payload_text(&event.payload),
    }
}

/// Inverse of [`protocol_event`], so the provider can reuse Cortex's own
/// foreground gate rather than restating it.
fn core_event(event: &BackgroundReviewSessionEventV1) -> SessionEvent {
    SessionEvent {
        schema_version: event.schema_version,
        session_id: event.session_id.clone(),
        seq: event.seq,
        event_id: event.event_id.clone(),
        event_type: event.event_type.clone(),
        payload: event.payload.clone(),
        scope_id: event.scope_id.clone(),
        authority: event.authority.clone(),
        influence_class: event.influence_class.clone(),
        lifecycle: event.lifecycle.clone(),
        retention: event.retention.clone(),
        provenance: event
            .provenance
            .iter()
            .map(|item| cortex_core::ProvenanceRef {
                source: item.source.clone(),
                source_event_ids: item.source_event_ids.clone(),
                producer: item.producer.clone(),
            })
            .collect(),
        occurred_at_ms: event.occurred_at_ms,
        recorded_at_ms: event.recorded_at_ms,
        content_hash: event.content_hash.clone(),
    }
}

/// Translate the wire foreground signal into Cortex's three-state signal.  The
/// emission id is synthesized from the range because the wire contract carries
/// only the range; identity of the foreground writer is not invented.
fn core_foreground_state(
    state: &BackgroundReviewForegroundMemoryStateV1,
    session_id: &str,
) -> ForegroundMemoryStateV1 {
    match state {
        BackgroundReviewForegroundMemoryStateV1::Unavailable => {
            ForegroundMemoryStateV1::Unavailable
        }
        BackgroundReviewForegroundMemoryStateV1::AvailableNoEmission => {
            ForegroundMemoryStateV1::AvailableNoEmission
        }
        BackgroundReviewForegroundMemoryStateV1::AvailableEmission { range } => {
            ForegroundMemoryStateV1::AvailableEmission(ForegroundMemoryEmissionV1 {
                emission_id: format!(
                    "foreground-emission-{session_id}-{}-{}",
                    range.start_seq, range.end_seq
                ),
                session_id: session_id.to_string(),
                start_seq: range.start_seq,
                end_seq: range.end_seq,
            })
        }
    }
}

impl BackgroundSemanticReviewProvider for DeterministicFirstPartySemanticReviewProvider {
    fn execute(
        &self,
        request: &BackgroundSemanticReviewRequestV1,
    ) -> Result<BackgroundSemanticReviewResultV1, BackgroundSemanticReviewProviderError> {
        request
            .validate()
            .map_err(|error| BackgroundSemanticReviewProviderError::Protocol(error.to_string()))?;
        if !request
            .restricted_capabilities
            .iter()
            .any(|capability| capability == BACKGROUND_SEMANTIC_CAPABILITY)
        {
            return Ok(self.refuse(request, BackgroundReviewReasonV1::InvalidJob));
        }
        if self.now() >= request.deadline_unix_ms {
            return Ok(self.refuse(request, BackgroundReviewReasonV1::TimeGate));
        }
        if request.per_turn_budget_remaining == 0 {
            return Ok(self.refuse(request, BackgroundReviewReasonV1::PerTurnBudgetExceeded));
        }
        if request.aggregate_budget_remaining == 0 {
            return Ok(self.refuse(request, BackgroundReviewReasonV1::AggregateBudgetExceeded));
        }
        let budget = request
            .per_turn_budget_remaining
            .min(request.aggregate_budget_remaining)
            .min(usize::MAX as u64) as usize;

        match request.job_kind {
            // Not a behavioral learner. Refusing is truthful; inventing Adapt
            // categories from lexical rules would not be.
            BackgroundReviewJobKindV1::AdaptBehavioralReview => Ok(self.refuse(
                request,
                BackgroundReviewReasonV1::SemanticProviderNotWired,
            )),
            BackgroundReviewJobKindV1::CortexSemanticDream => {
                // Budget bounds the window analyzed. Truncation is safe because
                // the cursor only advances over the prefix actually analyzed.
                let mut spent = 0usize;
                let mut analyzed = Vec::new();
                for event in &request.events {
                    let cost = cortex_core::estimate_tokens(&event.payload.to_string());
                    if spent.saturating_add(cost) > budget {
                        break;
                    }
                    spent = spent.saturating_add(cost);
                    analyzed.push(event);
                }
                if analyzed.is_empty() {
                    return Ok(
                        self.refuse(request, BackgroundReviewReasonV1::PerTurnBudgetExceeded)
                    );
                }
                let events = analyzed
                    .iter()
                    .map(|event| analyzer_event(event))
                    .collect::<Vec<_>>();
                let findings =
                    cortex_core::review::analyze_events_for_curation(&events, self.limits);
                let mut proposals = Vec::new();
                for proposal in &findings.proposals {
                    let value = serde_json::to_value(proposal).map_err(|error| {
                        BackgroundSemanticReviewProviderError::Encode(error.to_string())
                    })?;
                    proposals.push(value);
                }
                let next_cursor = if proposals.is_empty() {
                    None
                } else {
                    analyzed.last().map(|event| BackgroundReviewCursorV1 {
                        session_id: request.session_id.clone(),
                        last_seq: event.seq,
                    })
                };
                Ok(self.result(
                    request,
                    proposals,
                    Vec::new(),
                    next_cursor,
                    BackgroundSemanticReviewStatusV1::Proposals,
                ))
            }
            BackgroundReviewJobKindV1::CortexMemoryCandidateExtraction => {
                let cursor = EventCursor {
                    session_id: request.cursor.session_id.clone(),
                    last_seq: request.cursor.last_seq,
                };
                let events = request.events.iter().map(core_event).collect::<Vec<_>>();
                let foreground_state =
                    core_foreground_state(&request.foreground_memory_state, &request.session_id);
                let limits = MemoryCandidateExtractionLimitsV1 {
                    max_events:
                        membrane_protocol::background_review::BACKGROUND_SEMANTIC_REVIEW_MAX_EVENTS,
                    max_input_tokens: budget.max(1),
                    max_duration_ms: request.deadline_unix_ms.saturating_sub(self.now()).max(1),
                    max_model_requests: 1,
                };
                // CTX-024's gate, evaluated by Cortex's own function: extraction
                // proceeds only when authoritative foreground memory does not
                // already cover this cursor range.
                let decision = bound_memory_candidate_extraction_window_with_state(
                    &cursor,
                    &events,
                    &foreground_state,
                    limits,
                    true,
                );
                let window = match decision {
                    Ok(MemoryCandidateExtractionDecisionV1::WindowBound { window }) => window,
                    Ok(MemoryCandidateExtractionDecisionV1::Skipped { .. }) => {
                        // Covered by foreground memory, or no new events: a
                        // completed no-op, never a fabricated candidate.
                        return Ok(self.result(
                            request,
                            Vec::new(),
                            Vec::new(),
                            None,
                            BackgroundSemanticReviewStatusV1::Proposals,
                        ));
                    }
                    Ok(MemoryCandidateExtractionDecisionV1::Blocked { reason }) => {
                        let reason = match reason {
                            cortex_core::review::MemoryCandidateExtractionBlockerV1::ForegroundMemoryEmissionSignalUnavailable => {
                                BackgroundReviewReasonV1::ForegroundMemoryEmissionSignalUnavailable
                            }
                            cortex_core::review::MemoryCandidateExtractionBlockerV1::ModelInputUnavailable => {
                                BackgroundReviewReasonV1::ModelInputUnavailable
                            }
                            cortex_core::review::MemoryCandidateExtractionBlockerV1::CursorInputUnavailable => {
                                BackgroundReviewReasonV1::CursorInputUnavailable
                            }
                        };
                        return Ok(self.refuse(request, reason));
                    }
                    // The only errors here are input-budget overruns and cursor
                    // mismatches; both are refusals, never a partial candidate.
                    Err(_) => {
                        return Ok(
                            self.refuse(request, BackgroundReviewReasonV1::PerTurnBudgetExceeded)
                        )
                    }
                };
                let analyzer_events = request
                    .events
                    .iter()
                    .map(analyzer_event)
                    .collect::<Vec<_>>();
                let Some(candidate) = cortex_core::review::extract_bounded_memory_candidate(
                    &window,
                    &analyzer_events,
                ) else {
                    return Ok(self.result(
                        request,
                        Vec::new(),
                        Vec::new(),
                        None,
                        BackgroundSemanticReviewStatusV1::Proposals,
                    ));
                };
                let value = serde_json::to_value(&candidate).map_err(|error| {
                    BackgroundSemanticReviewProviderError::Encode(error.to_string())
                })?;
                let next_cursor = request.events.last().map(|event| BackgroundReviewCursorV1 {
                    session_id: request.session_id.clone(),
                    last_seq: event.seq,
                });
                Ok(self.result(
                    request,
                    Vec::new(),
                    vec![value],
                    next_cursor,
                    BackgroundSemanticReviewStatusV1::Proposals,
                ))
            }
        }
    }
}

/// Authenticated HTTP/1.1 loopback client. It intentionally uses the standard
/// library so no second network/model stack is introduced into runtime.
pub struct AuthenticatedLoopbackSemanticReviewProvider {
    endpoint: String,
    host: String,
    port: u16,
    path: String,
    bearer_token: String,
    timeout: Duration,
}

impl std::fmt::Debug for AuthenticatedLoopbackSemanticReviewProvider {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AuthenticatedLoopbackSemanticReviewProvider")
            .field("endpoint", &self.endpoint)
            .field("host", &self.host)
            .field("port", &self.port)
            .field("path", &self.path)
            .field("bearer_token", &"<redacted>")
            .field("timeout", &self.timeout)
            .finish()
    }
}

impl AuthenticatedLoopbackSemanticReviewProvider {
    pub fn new(
        endpoint: impl Into<String>,
        bearer_token: impl Into<String>,
    ) -> Result<Self, BackgroundSemanticReviewProviderError> {
        let endpoint = endpoint.into();
        let bearer_token = bearer_token.into();
        if bearer_token.trim().is_empty()
            || bearer_token.contains('\r')
            || bearer_token.contains('\n')
        {
            return Err(BackgroundSemanticReviewProviderError::Configuration(
                "bearer token is missing or contains header delimiters".into(),
            ));
        }
        let (host, port, path) = parse_loopback_endpoint(&endpoint)?;
        Ok(Self {
            endpoint,
            host,
            port,
            path,
            bearer_token,
            timeout: Duration::from_millis(BACKGROUND_SEMANTIC_PROVIDER_TIMEOUT_MS),
        })
    }

    pub fn from_environment(
        fallback_token: &str,
    ) -> Result<Self, BackgroundSemanticReviewProviderError> {
        let endpoint = std::env::var(BACKGROUND_SEMANTIC_PROVIDER_ENDPOINT_ENV).map_err(|_| {
            BackgroundSemanticReviewProviderError::Configuration(
                "provider endpoint is not configured".into(),
            )
        })?;
        let token = std::env::var(BACKGROUND_SEMANTIC_PROVIDER_TOKEN_ENV)
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| fallback_token.to_string());
        Self::new(endpoint, token)
    }

    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    fn timeout_for(&self, deadline_unix_ms: u64) -> Duration {
        let now = unix_ms();
        let remaining = deadline_unix_ms.saturating_sub(now);
        self.timeout.min(Duration::from_millis(remaining.max(1)))
    }
}

impl BackgroundSemanticReviewProvider for AuthenticatedLoopbackSemanticReviewProvider {
    fn execute(
        &self,
        request: &BackgroundSemanticReviewRequestV1,
    ) -> Result<BackgroundSemanticReviewResultV1, BackgroundSemanticReviewProviderError> {
        request
            .validate()
            .map_err(|error| BackgroundSemanticReviewProviderError::Protocol(error.to_string()))?;
        let payload = serde_json::to_vec(request)
            .map_err(|error| BackgroundSemanticReviewProviderError::Encode(error.to_string()))?;
        if payload.len()
            > membrane_protocol::background_review::BACKGROUND_SEMANTIC_REVIEW_MAX_FRAME_BYTES
        {
            return Err(BackgroundSemanticReviewProviderError::Protocol(
                "request_frame_too_large".into(),
            ));
        }
        let timeout = self.timeout_for(request.deadline_unix_ms);
        let address = SocketAddr::new(
            self.host
                .parse::<IpAddr>()
                .unwrap_or(IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)),
            self.port,
        );
        let mut stream = TcpStream::connect_timeout(&address, timeout).map_err(|error| {
            if matches!(
                error.kind(),
                std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
            ) {
                BackgroundSemanticReviewProviderError::Timeout
            } else {
                BackgroundSemanticReviewProviderError::Unavailable(error.to_string())
            }
        })?;
        stream
            .set_read_timeout(Some(timeout))
            .map_err(|error| BackgroundSemanticReviewProviderError::Io(error.to_string()))?;
        stream
            .set_write_timeout(Some(timeout))
            .map_err(|error| BackgroundSemanticReviewProviderError::Io(error.to_string()))?;
        let header = format!(
            "POST {} HTTP/1.1\r\nHost: {}\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            self.path,
            self.host,
            self.bearer_token,
            payload.len()
        );
        stream
            .write_all(header.as_bytes())
            .and_then(|_| stream.write_all(&payload))
            .map_err(|error| BackgroundSemanticReviewProviderError::Io(error.to_string()))?;
        let mut response = Vec::new();
        let mut chunk = [0_u8; 8192];
        loop {
            let read = stream.read(&mut chunk).map_err(|error| {
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                ) {
                    BackgroundSemanticReviewProviderError::Timeout
                } else {
                    BackgroundSemanticReviewProviderError::Io(error.to_string())
                }
            })?;
            if read == 0 {
                break;
            }
            if response.len().saturating_add(read)
                > membrane_protocol::background_review::BACKGROUND_SEMANTIC_REVIEW_MAX_FRAME_BYTES
            {
                return Err(BackgroundSemanticReviewProviderError::Protocol(
                    "response_frame_too_large".into(),
                ));
            }
            response.extend_from_slice(&chunk[..read]);
        }
        let (status, body) = parse_http_response(&response)?;
        match status {
            401 | 403 => return Err(BackgroundSemanticReviewProviderError::Authentication),
            408 | 504 => return Err(BackgroundSemanticReviewProviderError::Timeout),
            200 => {}
            other => {
                return Err(BackgroundSemanticReviewProviderError::Protocol(format!(
                    "unexpected_status_{other}"
                )))
            }
        }
        let result: BackgroundSemanticReviewResultV1 =
            serde_json::from_slice(body).map_err(|error| {
                BackgroundSemanticReviewProviderError::InvalidResult(error.to_string())
            })?;
        result.validate_against(request).map_err(|error| {
            BackgroundSemanticReviewProviderError::InvalidResult(error.to_string())
        })?;
        Ok(result)
    }
}

fn parse_loopback_endpoint(
    endpoint: &str,
) -> Result<(String, u16, String), BackgroundSemanticReviewProviderError> {
    let remainder = endpoint.strip_prefix("http://").ok_or_else(|| {
        BackgroundSemanticReviewProviderError::Configuration(
            "provider endpoint must use http loopback".into(),
        )
    })?;
    if remainder.contains('?') || remainder.contains('#') {
        return Err(BackgroundSemanticReviewProviderError::Configuration(
            "provider endpoint query/fragment is not allowed".into(),
        ));
    }
    let (authority, path_suffix) = remainder
        .split_once('/')
        .map_or((remainder, ""), |(authority, path)| (authority, path));
    if authority.contains('@') {
        return Err(BackgroundSemanticReviewProviderError::Configuration(
            "provider endpoint userinfo is not allowed".into(),
        ));
    }
    let (host, port) = if let Some(rest) = authority.strip_prefix('[') {
        let (host, port) = rest.split_once(']').ok_or_else(|| {
            BackgroundSemanticReviewProviderError::Configuration(
                "provider endpoint IPv6 authority is invalid".into(),
            )
        })?;
        let port = port.strip_prefix(':').ok_or_else(|| {
            BackgroundSemanticReviewProviderError::Configuration(
                "provider endpoint port is missing".into(),
            )
        })?;
        (host.to_string(), port)
    } else {
        let (host, port) = authority.split_once(':').ok_or_else(|| {
            BackgroundSemanticReviewProviderError::Configuration(
                "provider endpoint port is missing".into(),
            )
        })?;
        (host.to_string(), port)
    };
    let is_localhost = host == "localhost";
    let is_loopback = host
        .parse::<IpAddr>()
        .map(|address| address.is_loopback())
        .unwrap_or(false);
    if !is_localhost && !is_loopback {
        return Err(BackgroundSemanticReviewProviderError::Configuration(
            "provider endpoint must resolve to loopback".into(),
        ));
    }
    let port = port.parse::<u16>().map_err(|_| {
        BackgroundSemanticReviewProviderError::Configuration(
            "provider endpoint port is invalid".into(),
        )
    })?;
    if port == 0 {
        return Err(BackgroundSemanticReviewProviderError::Configuration(
            "provider endpoint port is zero".into(),
        ));
    }
    let path = if path_suffix.is_empty() {
        BACKGROUND_SEMANTIC_PROVIDER_PATH.to_string()
    } else {
        format!("/{path_suffix}")
    };
    Ok((host, port, path))
}

fn parse_http_response(
    response: &[u8],
) -> Result<(u16, &[u8]), BackgroundSemanticReviewProviderError> {
    let header_end = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| BackgroundSemanticReviewProviderError::Protocol("missing_headers".into()))?;
    let headers = &response[..header_end];
    let body = &response[header_end + 4..];
    if body.len() > membrane_protocol::background_review::BACKGROUND_SEMANTIC_REVIEW_MAX_FRAME_BYTES
    {
        return Err(BackgroundSemanticReviewProviderError::Protocol(
            "response_frame_too_large".into(),
        ));
    }
    let mut lines = headers.split(|byte| *byte == b'\n');
    let status_line = lines
        .next()
        .ok_or_else(|| BackgroundSemanticReviewProviderError::Protocol("missing_status".into()))?;
    let status_text = std::str::from_utf8(status_line)
        .map_err(|_| BackgroundSemanticReviewProviderError::Protocol("status_encoding".into()))?
        .trim_end_matches('\r');
    let mut status_parts = status_text.split_whitespace();
    let _http = status_parts.next();
    let status = status_parts
        .next()
        .ok_or_else(|| {
            BackgroundSemanticReviewProviderError::Protocol("missing_status_code".into())
        })?
        .parse::<u16>()
        .map_err(|_| {
            BackgroundSemanticReviewProviderError::Protocol("invalid_status_code".into())
        })?;
    for line in lines {
        let line = std::str::from_utf8(line)
            .map_err(|_| BackgroundSemanticReviewProviderError::Protocol("header_encoding".into()))?
            .trim_end_matches('\r');
        if let Some(value) = line.strip_prefix("Content-Length:") {
            let declared = value.trim().parse::<usize>().map_err(|_| {
                BackgroundSemanticReviewProviderError::Protocol("invalid_content_length".into())
            })?;
            if declared != body.len() {
                return Err(BackgroundSemanticReviewProviderError::Protocol(
                    "content_length_mismatch".into(),
                ));
            }
        }
    }
    Ok((status, body))
}

/// Build authenticated provider request from Cortex-owned bounded input.
pub fn build_background_semantic_review_request(
    job: &BackgroundReviewJobV1,
    input: &BackgroundSemanticReviewInputV1,
    per_turn_budget_remaining: u64,
    aggregate_budget_remaining: u64,
    deadline_unix_ms: u64,
) -> Result<BackgroundSemanticReviewRequestV1, BackgroundSemanticReviewProviderError> {
    let (selected_events, review_input_selection) = if job.kind
        == BackgroundReviewJobKindV1::AdaptBehavioralReview
    {
        let max_input_tokens = per_turn_budget_remaining
            .min(aggregate_budget_remaining)
            .min(usize::MAX as u64) as usize;
        let selection = select_review_input(
            &input.cursor,
            &input.events,
            &input.reviewed_baseline,
            ReviewInputSelectionLimitsV1 {
                max_input_tokens,
                novelty_floor: REVIEW_INPUT_NOVELTY_FLOOR,
            },
        )
        .map_err(|error| BackgroundSemanticReviewProviderError::Protocol(error.to_string()))?;
        let receipt = protocol_selection(&selection.receipt);
        (selection.events, Some(receipt))
    } else {
        (input.events.clone(), None)
    };
    let events = selected_events.iter().map(protocol_event).collect::<Vec<_>>();
    let foreground_memory_state = match &input.foreground_memory_state {
        ForegroundMemoryStateV1::Unavailable => {
            BackgroundReviewForegroundMemoryStateV1::Unavailable
        }
        ForegroundMemoryStateV1::AvailableNoEmission => {
            BackgroundReviewForegroundMemoryStateV1::AvailableNoEmission
        }
        ForegroundMemoryStateV1::AvailableEmission(emission) => {
            BackgroundReviewForegroundMemoryStateV1::AvailableEmission {
                range: BackgroundReviewEventRangeV1 {
                    start_seq: emission.start_seq,
                    end_seq: emission.end_seq,
                },
            }
        }
    };
    let request = BackgroundSemanticReviewRequestV1 {
        schema_version: BackgroundSemanticReviewRequestV1::SCHEMA_VERSION,
        job_id: job.job_id.clone(),
        job_kind: job.kind,
        session_id: input.cursor.session_id.clone(),
        task_id: input.task_id.clone(),
        turn_id: job.turn_id.clone(),
        cursor: BackgroundReviewCursorV1 {
            session_id: input.cursor.session_id.clone(),
            last_seq: input.cursor.last_seq,
        },
        events,
        review_input_selection,
        foreground_memory_state,
        per_turn_budget_remaining,
        aggregate_budget_remaining,
        deadline_unix_ms,
        restricted_capabilities: vec![
            BACKGROUND_SEMANTIC_CAPABILITY.to_string(),
            "no_durable_writes".to_string(),
        ],
    };
    request
        .validate()
        .map_err(|error| BackgroundSemanticReviewProviderError::Protocol(error.to_string()))?;
    Ok(request)
}

fn protocol_selection(
    selection: &cortex_core::review::ReviewInputSelectionV1,
) -> BackgroundReviewInputSelectionV1 {
    BackgroundReviewInputSelectionV1 {
        schema_version: BackgroundReviewInputSelectionV1::SCHEMA_VERSION,
        candidates_considered: selection
            .candidates_considered
            .iter()
            .map(|candidate| BackgroundReviewInputSelectionCandidateV1 {
                event_id: candidate.event_id.clone(),
                seq: candidate.seq,
                novelty_score: candidate.novelty_score,
                estimated_input_tokens: candidate.estimated_input_tokens,
            })
            .collect(),
        selected: selection.selected.clone(),
        skipped: selection
            .skipped
            .iter()
            .map(|entry| BackgroundReviewInputSelectionSkippedV1 {
                event_id: entry.event_id.clone(),
                reason: match entry.reason {
                    cortex_core::review::ReviewInputSelectionSkipReasonV1::BudgetExhausted => {
                        BackgroundReviewInputSelectionSkipReasonV1::BudgetExhausted
                    }
                    cortex_core::review::ReviewInputSelectionSkipReasonV1::BelowNoveltyFloor => {
                        BackgroundReviewInputSelectionSkipReasonV1::BelowNoveltyFloor
                    }
                },
            })
            .collect(),
        quiet_period: selection.quiet_period,
    }
}

fn protocol_event(event: &SessionEvent) -> BackgroundReviewSessionEventV1 {
    BackgroundReviewSessionEventV1 {
        schema_version: event.schema_version,
        session_id: event.session_id.clone(),
        seq: event.seq,
        event_id: event.event_id.clone(),
        event_type: event.event_type.clone(),
        payload: event.payload.clone(),
        scope_id: event.scope_id.clone(),
        authority: event.authority.clone(),
        influence_class: event.influence_class.clone(),
        lifecycle: event.lifecycle.clone(),
        retention: event.retention.clone(),
        provenance: event
            .provenance
            .iter()
            .map(|item| BackgroundReviewProvenanceRefV1 {
                source: item.source.clone(),
                source_event_ids: item.source_event_ids.clone(),
                producer: item.producer.clone(),
            })
            .collect(),
        occurred_at_ms: event.occurred_at_ms,
        recorded_at_ms: event.recorded_at_ms,
        content_hash: event.content_hash.clone(),
    }
}

/// Run one scheduler-admitted semantic attempt. Provider output is parsed and
/// validated before admission; cursor advances only after sink success.
pub fn execute_background_semantic_review(
    scheduler: &BackgroundReviewScheduler,
    job: &BackgroundReviewJobV1,
    input: &BackgroundSemanticReviewInputV1,
    provider: &dyn BackgroundSemanticReviewProvider,
    proposal_sink: Option<&dyn BackgroundReviewProposalAdmission>,
    cursor_store: &BackgroundReviewCursorStore,
    deadline_unix_ms: u64,
    observed_at_unix_ms: u64,
) -> BackgroundReviewExecutionV1 {
    let blocked = |status: BackgroundReviewExecutionStatusV1,
                   reason: BackgroundReviewReasonV1|
     -> BackgroundReviewExecutionV1 {
        let _ = scheduler.finish_with_completion(
            &job.job_id,
            BackgroundReviewCompletion::FailedWithReason(reason),
            observed_at_unix_ms,
        );
        BackgroundReviewExecutionV1 {
            schema_version: BackgroundReviewExecutionV1::SCHEMA_VERSION,
            job_id: job.job_id.clone(),
            kind: job.kind,
            status,
            proposals: Vec::new(),
            reason: Some(reason),
        }
    };
    if !scheduler.is_active_job(&job.job_id) {
        return BackgroundReviewExecutionV1 {
            schema_version: BackgroundReviewExecutionV1::SCHEMA_VERSION,
            job_id: job.job_id.clone(),
            kind: job.kind,
            status: BackgroundReviewExecutionStatusV1::Blocked,
            proposals: Vec::new(),
            reason: Some(BackgroundReviewReasonV1::InvalidJob),
        };
    }
    let Some((per_turn_budget_remaining, aggregate_budget_remaining)) =
        scheduler.budget_remaining(&job.turn_id)
    else {
        return blocked(
            BackgroundReviewExecutionStatusV1::Blocked,
            BackgroundReviewReasonV1::ModelInputUnavailable,
        );
    };
    let request = match build_background_semantic_review_request(
        job,
        input,
        per_turn_budget_remaining,
        aggregate_budget_remaining,
        deadline_unix_ms,
    ) {
        Ok(request) => request,
        Err(_) => {
            return blocked(
                BackgroundReviewExecutionStatusV1::Failed,
                BackgroundReviewReasonV1::CursorInputUnavailable,
            )
        }
    };

    let extraction_window = if job.kind
        == BackgroundReviewJobKindV1::CortexMemoryCandidateExtraction
    {
        let max_input_tokens = per_turn_budget_remaining
            .min(aggregate_budget_remaining)
            .min(usize::MAX as u64) as usize;
        let limits = MemoryCandidateExtractionLimitsV1 {
            max_events: membrane_protocol::background_review::BACKGROUND_SEMANTIC_REVIEW_MAX_EVENTS,
            max_input_tokens: max_input_tokens.max(1),
            max_duration_ms: deadline_unix_ms.saturating_sub(observed_at_unix_ms).max(1),
            max_model_requests: 1,
        };
        match bound_memory_candidate_extraction_window_with_state(
            &input.cursor,
            &input.events,
            &input.foreground_memory_state,
            limits,
            true,
        ) {
            Ok(MemoryCandidateExtractionDecisionV1::WindowBound { window }) => Some(window),
            Ok(MemoryCandidateExtractionDecisionV1::Skipped { .. }) => {
                let _ = scheduler.finish_with_completion(
                    &job.job_id,
                    BackgroundReviewCompletion::Completed,
                    observed_at_unix_ms,
                );
                return BackgroundReviewExecutionV1 {
                    schema_version: BackgroundReviewExecutionV1::SCHEMA_VERSION,
                    job_id: job.job_id.clone(),
                    kind: job.kind,
                    status: BackgroundReviewExecutionStatusV1::Proposals,
                    proposals: Vec::new(),
                    reason: None,
                };
            }
            Ok(MemoryCandidateExtractionDecisionV1::Blocked { reason }) => {
                let reason = match reason {
                    cortex_core::review::MemoryCandidateExtractionBlockerV1::ModelInputUnavailable => {
                        BackgroundReviewReasonV1::ModelInputUnavailable
                    }
                    cortex_core::review::MemoryCandidateExtractionBlockerV1::CursorInputUnavailable => {
                        BackgroundReviewReasonV1::CursorInputUnavailable
                    }
                    cortex_core::review::MemoryCandidateExtractionBlockerV1::ForegroundMemoryEmissionSignalUnavailable => {
                        BackgroundReviewReasonV1::ForegroundMemoryEmissionSignalUnavailable
                    }
                };
                return blocked(BackgroundReviewExecutionStatusV1::Blocked, reason);
            }
            Err(_) => {
                return blocked(
                    BackgroundReviewExecutionStatusV1::Failed,
                    BackgroundReviewReasonV1::InvalidProposal,
                )
            }
        }
    } else {
        None
    };

    // A mechanical quiet period or exhausted selection budget is a completed
    // no-op, not permission to ask a semantic provider to invent input.
    if job.kind == BackgroundReviewJobKindV1::AdaptBehavioralReview
        && request
            .review_input_selection
            .as_ref()
            .is_some_and(|selection| selection.selected.is_empty())
    {
        let _ = scheduler.finish_with_completion(
            &job.job_id,
            BackgroundReviewCompletion::Completed,
            observed_at_unix_ms,
        );
        return BackgroundReviewExecutionV1 {
            schema_version: BackgroundReviewExecutionV1::SCHEMA_VERSION,
            job_id: job.job_id.clone(),
            kind: job.kind,
            status: BackgroundReviewExecutionStatusV1::Proposals,
            proposals: Vec::new(),
            reason: None,
        };
    }

    let result = match provider.execute(&request) {
        Ok(result) => result,
        Err(error) => {
            let reason = match error {
                BackgroundSemanticReviewProviderError::Configuration(_)
                | BackgroundSemanticReviewProviderError::Unavailable(_) => {
                    BackgroundReviewReasonV1::SemanticProviderNotWired
                }
                BackgroundSemanticReviewProviderError::Timeout
                | BackgroundSemanticReviewProviderError::Authentication
                | BackgroundSemanticReviewProviderError::Protocol(_)
                | BackgroundSemanticReviewProviderError::InvalidResult(_)
                | BackgroundSemanticReviewProviderError::Encode(_)
                | BackgroundSemanticReviewProviderError::Io(_) => BackgroundReviewReasonV1::Failed,
            };
            return blocked(BackgroundReviewExecutionStatusV1::Failed, reason);
        }
    };
    if let Err(_) = result.validate_against(&request) {
        return blocked(
            BackgroundReviewExecutionStatusV1::Failed,
            BackgroundReviewReasonV1::InvalidProposal,
        );
    }
    match result.status {
        BackgroundSemanticReviewStatusV1::Blocked { reason } => {
            let reason = execution_reason(reason);
            return blocked(BackgroundReviewExecutionStatusV1::Blocked, reason);
        }
        BackgroundSemanticReviewStatusV1::Failed { reason } => {
            let reason = execution_reason(reason);
            return blocked(BackgroundReviewExecutionStatusV1::Failed, reason);
        }
        BackgroundSemanticReviewStatusV1::Proposals => {}
    }
    let mut refs = Vec::new();
    if job.kind == BackgroundReviewJobKindV1::CortexMemoryCandidateExtraction {
        if !result.curation_proposals.is_empty() {
            return blocked(
                BackgroundReviewExecutionStatusV1::Failed,
                BackgroundReviewReasonV1::InvalidProposal,
            );
        }
        let candidates = result
            .memory_candidates
            .iter()
            .map(|value| serde_json::from_value::<MemoryCandidateV1>(value.clone()))
            .collect::<Result<Vec<_>, _>>();
        let Ok(candidates) = candidates else {
            return blocked(
                BackgroundReviewExecutionStatusV1::Failed,
                BackgroundReviewReasonV1::InvalidProposal,
            );
        };
        let Some(window) = extraction_window.as_ref() else {
            return blocked(
                BackgroundReviewExecutionStatusV1::Failed,
                BackgroundReviewReasonV1::InvalidProposal,
            );
        };
        if validate_memory_candidates_for_window(window, &candidates).is_err() {
            return blocked(
                BackgroundReviewExecutionStatusV1::Failed,
                BackgroundReviewReasonV1::InvalidProposal,
            );
        }
        if !candidates.is_empty() {
            let Some(sink) = proposal_sink else {
                return blocked(
                    BackgroundReviewExecutionStatusV1::Failed,
                    BackgroundReviewReasonV1::ProposalSinkUnavailable,
                );
            };
            if let Err(reason) = sink.submit_memory_candidates(job, &candidates) {
                return blocked(BackgroundReviewExecutionStatusV1::Failed, reason);
            }
            refs.extend(
                candidates
                    .iter()
                    .map(|candidate| proposal_ref(&candidate.candidate_id, candidate)),
            );
        }
    } else {
        if !result.memory_candidates.is_empty() {
            return blocked(
                BackgroundReviewExecutionStatusV1::Failed,
                BackgroundReviewReasonV1::InvalidProposal,
            );
        }
        let proposals = result
            .curation_proposals
            .iter()
            .map(|value| serde_json::from_value::<SemanticCurationProposalV1>(value.clone()))
            .collect::<Result<Vec<_>, _>>();
        let Ok(proposals) = proposals else {
            return blocked(
                BackgroundReviewExecutionStatusV1::Failed,
                BackgroundReviewReasonV1::InvalidProposal,
            );
        };
        if validate_semantic_curation_proposals(&proposals).is_err() {
            return blocked(
                BackgroundReviewExecutionStatusV1::Failed,
                BackgroundReviewReasonV1::InvalidProposal,
            );
        }
        if !proposals.is_empty() {
            let Some(sink) = proposal_sink else {
                return blocked(
                    BackgroundReviewExecutionStatusV1::Failed,
                    BackgroundReviewReasonV1::ProposalSinkUnavailable,
                );
            };
            if let Err(reason) = sink.submit_curation(job, &proposals) {
                return blocked(BackgroundReviewExecutionStatusV1::Failed, reason);
            }
            refs.extend(
                proposals
                    .iter()
                    .map(|proposal| proposal_ref(&proposal.proposal_id, proposal)),
            );
        }
    }
    if refs.len() > 64 {
        return blocked(
            BackgroundReviewExecutionStatusV1::Failed,
            BackgroundReviewReasonV1::InvalidProposal,
        );
    }
    if let Some(next_cursor) = result.next_cursor.as_ref() {
        // Selection may leave a higher-scoring event after an eligible gap.
        // Never let a provider cursor jump over that skipped event: only the
        // selected contiguous prefix is consumable in this run.
        let next_cursor = safe_selected_cursor(&request, next_cursor);
        if refs.is_empty() || cursor_store.advance(&next_cursor).is_err() {
            return blocked(
                BackgroundReviewExecutionStatusV1::Failed,
                BackgroundReviewReasonV1::CursorInputUnavailable,
            );
        }
    }
    let _ = scheduler.finish_with_completion(
        &job.job_id,
        BackgroundReviewCompletion::Completed,
        observed_at_unix_ms,
    );
    BackgroundReviewExecutionV1 {
        schema_version: BackgroundReviewExecutionV1::SCHEMA_VERSION,
        job_id: job.job_id.clone(),
        kind: job.kind,
        status: BackgroundReviewExecutionStatusV1::Proposals,
        proposals: refs,
        reason: None,
    }
}

fn safe_selected_cursor(
    request: &BackgroundSemanticReviewRequestV1,
    proposed: &BackgroundReviewCursorV1,
) -> BackgroundReviewCursorV1 {
    let Some(selection) = request.review_input_selection.as_ref() else {
        return proposed.clone();
    };
    let selected = selection.selected.iter().map(String::as_str).collect::<HashSet<_>>();
    let mut contiguous = request.cursor.last_seq;
    let mut candidates = selection.candidates_considered.iter().collect::<Vec<_>>();
    candidates.sort_by_key(|candidate| candidate.seq);
    for candidate in candidates {
        if candidate.seq != contiguous.saturating_add(1) {
            if candidate.seq > contiguous {
                break;
            }
            continue;
        }
        if !selected.contains(candidate.event_id.as_str()) {
            break;
        }
        contiguous = candidate.seq;
    }
    BackgroundReviewCursorV1 {
        session_id: proposed.session_id.clone(),
        last_seq: proposed.last_seq.min(contiguous),
    }
}

fn proposal_ref<T: serde::Serialize>(
    proposal_id: &str,
    proposal: &T,
) -> BackgroundReviewProposalRefV1 {
    BackgroundReviewProposalRefV1 {
        proposal_id: proposal_id.to_string(),
        proposal_digest: digest_str(&canonical_json_of(proposal)),
    }
}

fn execution_reason(reason: BackgroundReviewReasonV1) -> BackgroundReviewReasonV1 {
    match reason {
        BackgroundReviewReasonV1::ModelInputUnavailable
        | BackgroundReviewReasonV1::SemanticProviderNotWired
        | BackgroundReviewReasonV1::ProposalSinkUnavailable
        | BackgroundReviewReasonV1::ForegroundMemoryEmissionSignalUnavailable
        | BackgroundReviewReasonV1::CursorInputUnavailable
        | BackgroundReviewReasonV1::InvalidProposal
        | BackgroundReviewReasonV1::InvalidJob
        | BackgroundReviewReasonV1::Failed => reason,
        _ => BackgroundReviewReasonV1::Failed,
    }
}

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConfigState {
    Ready,
    Unavailable,
    Invalid,
}

#[derive(Debug)]
struct ActiveJob {
    job: BackgroundReviewJobV1,
    attempt: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct JobIdentity {
    kind: BackgroundReviewJobKindV1,
    turn_id: String,
    input_tokens: u64,
}

impl JobIdentity {
    fn from_job(job: &BackgroundReviewJobV1) -> Self {
        Self {
            kind: job.kind,
            turn_id: job.turn_id.clone(),
            input_tokens: job.input_tokens,
        }
    }
}

#[derive(Debug, Default)]
struct SchedulerState {
    hub_active: bool,
    foreground_active: bool,
    last_started_at_unix_ms: Option<u64>,
    activity_units: u64,
    active: HashMap<BackgroundReviewJobKindV1, ActiveJob>,
    attempts: HashMap<String, u8>,
    job_identities: HashMap<String, JobIdentity>,
    completed_jobs: HashSet<String>,
    turn_input_tokens: HashMap<String, u64>,
    aggregate_input_tokens: u64,
    observations: VecDeque<BackgroundReviewObservationV1>,
}

/// Thread-safe scheduler state owned by tray daemon.
pub struct BackgroundReviewScheduler {
    config: Option<BackgroundReviewConfigV1>,
    config_state: ConfigState,
    state: Mutex<SchedulerState>,
}

impl std::fmt::Debug for BackgroundReviewScheduler {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("BackgroundReviewScheduler")
            .field("config", &self.config)
            .field("config_state", &self.config_state)
            .finish_non_exhaustive()
    }
}

impl BackgroundReviewScheduler {
    /// Construct scheduler from already validated configuration.
    pub fn new(config: BackgroundReviewConfigV1) -> Result<Self, BackgroundReviewConfigError> {
        config
            .validate()
            .map_err(BackgroundReviewConfigError::Invalid)?;
        Ok(Self {
            config: Some(config),
            config_state: ConfigState::Ready,
            state: Mutex::new(SchedulerState::default()),
        })
    }

    /// Construct fail-closed scheduler that records startup reason.
    pub fn disabled(reason: BackgroundReviewReasonV1, observed_at_unix_ms: u64) -> Self {
        let config_state = match reason {
            BackgroundReviewReasonV1::ConfigUnavailable => ConfigState::Unavailable,
            BackgroundReviewReasonV1::ConfigInvalid => ConfigState::Invalid,
            _ => ConfigState::Invalid,
        };
        let scheduler = Self {
            config: None,
            config_state,
            state: Mutex::new(SchedulerState::default()),
        };
        scheduler.record_observation(
            None,
            None,
            BackgroundReviewStatusV1::Deferred,
            reason,
            observed_at_unix_ms,
            0,
            0,
        );
        scheduler
    }

    /// Read configuration from path; unreadable or invalid input never runs a job.
    pub fn from_config_path(path: impl AsRef<Path>, observed_at_unix_ms: u64) -> Self {
        let path = path.as_ref();
        match fs::read_to_string(path) {
            Ok(contents) => match BackgroundReviewConfigV1::from_json(&contents) {
                Ok(config) => Self::new(config).unwrap_or_else(|_| {
                    Self::disabled(BackgroundReviewReasonV1::ConfigInvalid, observed_at_unix_ms)
                }),
                Err(BackgroundReviewConfigError::Invalid(_))
                | Err(BackgroundReviewConfigError::Json(_))
                | Err(BackgroundReviewConfigError::Io(_)) => {
                    Self::disabled(BackgroundReviewReasonV1::ConfigInvalid, observed_at_unix_ms)
                }
            },
            Err(_) => Self::disabled(
                BackgroundReviewReasonV1::ConfigUnavailable,
                observed_at_unix_ms,
            ),
        }
    }

    /// Resolve workspace configuration path, honoring explicit environment override.
    pub fn config_path_for_workspace(workspace_root: impl AsRef<Path>) -> PathBuf {
        std::env::var_os(CONFIG_PATH_ENV)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| workspace_root.as_ref().join(DEFAULT_CONFIG_RELATIVE_PATH))
    }

    /// Read fail-closed configuration beneath workspace root.
    pub fn from_workspace_root(workspace_root: impl AsRef<Path>, observed_at_unix_ms: u64) -> Self {
        let path = Self::config_path_for_workspace(workspace_root);
        Self::from_config_path(path, observed_at_unix_ms)
    }

    /// Return loaded configuration, if configuration was readable and valid.
    pub fn config(&self) -> Option<BackgroundReviewConfigV1> {
        self.config.clone()
    }

    /// Mark Hub residency. Deactivation cancels all admitted work synchronously.
    pub fn set_hub_active(&self, active: bool, observed_at_unix_ms: u64) {
        let mut state = self.state.lock().expect("background review state poisoned");
        let was_active = state.hub_active;
        state.hub_active = active;
        if active || !was_active {
            return;
        }
        self.cancel_all_locked(
            &mut state,
            BackgroundReviewReasonV1::HubInactive,
            observed_at_unix_ms,
        );
    }

    /// Mark foreground work. Starting foreground work pre-empts admitted review jobs.
    pub fn set_foreground_active(&self, active: bool, observed_at_unix_ms: u64) {
        let mut state = self.state.lock().expect("background review state poisoned");
        state.foreground_active = active;
        if active {
            self.cancel_all_locked(
                &mut state,
                BackgroundReviewReasonV1::ForegroundPreempted,
                observed_at_unix_ms,
            );
        }
    }

    /// Add activity units used by activity gate. Counter saturates on overflow.
    pub fn record_activity(&self, activity_units: u64) {
        let mut state = self.state.lock().expect("background review state poisoned");
        state.activity_units = state.activity_units.saturating_add(activity_units);
    }

    /// Report why no review was admitted at an idle checkpoint.
    pub fn observe_idle(&self, observed_at_unix_ms: u64) {
        let mut state = self.state.lock().expect("background review state poisoned");
        let reason = match self.config_state {
            ConfigState::Unavailable => BackgroundReviewReasonV1::ConfigUnavailable,
            ConfigState::Invalid => BackgroundReviewReasonV1::ConfigInvalid,
            ConfigState::Ready if !self.config.as_ref().is_some_and(|config| config.enabled) => {
                BackgroundReviewReasonV1::Disabled
            }
            ConfigState::Ready if !state.hub_active => BackgroundReviewReasonV1::HubInactive,
            ConfigState::Ready if state.foreground_active => {
                BackgroundReviewReasonV1::ForegroundPreempted
            }
            ConfigState::Ready if !state.active.is_empty() => {
                BackgroundReviewReasonV1::AlreadyRunning
            }
            ConfigState::Ready
                if state.last_started_at_unix_ms.is_some_and(|started| {
                    observed_at_unix_ms.saturating_sub(started)
                        < self
                            .config
                            .as_ref()
                            .expect("ready configuration exists")
                            .min_elapsed_ms
                }) =>
            {
                BackgroundReviewReasonV1::TimeGate
            }
            ConfigState::Ready
                if state.activity_units
                    < self
                        .config
                        .as_ref()
                        .expect("ready configuration exists")
                        .activity_threshold =>
            {
                BackgroundReviewReasonV1::ActivityGate
            }
            ConfigState::Ready => BackgroundReviewReasonV1::NoEligibleWork,
        };
        self.observe_locked(
            &mut state,
            None,
            None,
            BackgroundReviewStatusV1::Deferred,
            reason,
            observed_at_unix_ms,
            0,
            0,
            None,
        );
    }

    /// Ask scheduler to admit one daemon-owned job.
    pub fn start(
        &self,
        job: BackgroundReviewJobV1,
        observed_at_unix_ms: u64,
    ) -> BackgroundReviewDecision {
        let mut state = self.state.lock().expect("background review state poisoned");
        if job.validate().is_err() {
            return self.defer_locked(
                &mut state,
                &job,
                BackgroundReviewReasonV1::InvalidJob,
                observed_at_unix_ms,
            );
        }
        let Some(config) = self.config.as_ref() else {
            let reason = match self.config_state {
                ConfigState::Unavailable => BackgroundReviewReasonV1::ConfigUnavailable,
                ConfigState::Invalid | ConfigState::Ready => {
                    BackgroundReviewReasonV1::ConfigInvalid
                }
            };
            return self.defer_locked(&mut state, &job, reason, observed_at_unix_ms);
        };
        if !config.enabled {
            return self.defer_locked(
                &mut state,
                &job,
                BackgroundReviewReasonV1::Disabled,
                observed_at_unix_ms,
            );
        }
        if !state.hub_active {
            return self.defer_locked(
                &mut state,
                &job,
                BackgroundReviewReasonV1::HubInactive,
                observed_at_unix_ms,
            );
        }
        if state.foreground_active {
            return self.defer_locked(
                &mut state,
                &job,
                BackgroundReviewReasonV1::ForegroundPreempted,
                observed_at_unix_ms,
            );
        }
        let identity = JobIdentity::from_job(&job);
        if state
            .job_identities
            .get(&job.job_id)
            .is_some_and(|existing| existing != &identity)
        {
            return self.defer_locked(
                &mut state,
                &job,
                BackgroundReviewReasonV1::InvalidJob,
                observed_at_unix_ms,
            );
        }
        // A completed job id is terminal. Replaying it cannot be treated as
        // retry because only failed/cancelled attempts may consume retry slot.
        if state.completed_jobs.contains(&job.job_id) {
            return self.defer_locked(
                &mut state,
                &job,
                BackgroundReviewReasonV1::RetryLimit,
                observed_at_unix_ms,
            );
        }
        if state.active.contains_key(&job.kind) {
            return self.defer_locked(
                &mut state,
                &job,
                BackgroundReviewReasonV1::AlreadyRunning,
                observed_at_unix_ms,
            );
        }
        if state.last_started_at_unix_ms.is_some_and(|started| {
            observed_at_unix_ms.saturating_sub(started) < config.min_elapsed_ms
        }) {
            return self.defer_locked(
                &mut state,
                &job,
                BackgroundReviewReasonV1::TimeGate,
                observed_at_unix_ms,
            );
        }
        if state.activity_units < config.activity_threshold {
            return self.defer_locked(
                &mut state,
                &job,
                BackgroundReviewReasonV1::ActivityGate,
                observed_at_unix_ms,
            );
        }
        let prior_attempts = state.attempts.get(&job.job_id).copied().unwrap_or_default();
        if prior_attempts >= MAX_ATTEMPTS {
            return self.defer_locked(
                &mut state,
                &job,
                BackgroundReviewReasonV1::RetryLimit,
                observed_at_unix_ms,
            );
        }
        if state
            .turn_input_tokens
            .get(&job.turn_id)
            .copied()
            .unwrap_or_default()
            .saturating_add(job.input_tokens)
            > config.per_turn_input_budget
        {
            return self.defer_locked(
                &mut state,
                &job,
                BackgroundReviewReasonV1::PerTurnBudgetExceeded,
                observed_at_unix_ms,
            );
        }
        if state
            .aggregate_input_tokens
            .saturating_add(job.input_tokens)
            > config.aggregate_input_budget
        {
            return self.defer_locked(
                &mut state,
                &job,
                BackgroundReviewReasonV1::AggregateBudgetExceeded,
                observed_at_unix_ms,
            );
        }
        let attempt = prior_attempts + 1;
        state.job_identities.insert(job.job_id.clone(), identity);
        state.attempts.insert(job.job_id.clone(), attempt);
        state
            .turn_input_tokens
            .entry(job.turn_id.clone())
            .and_modify(|tokens| *tokens = tokens.saturating_add(job.input_tokens))
            .or_insert(job.input_tokens);
        state.aggregate_input_tokens = state
            .aggregate_input_tokens
            .saturating_add(job.input_tokens);
        state.last_started_at_unix_ms = Some(observed_at_unix_ms);
        state.activity_units = 0;
        let kind = job.kind;
        state.active.insert(
            kind,
            ActiveJob {
                job: job.clone(),
                attempt,
            },
        );
        self.observe_locked(
            &mut state,
            Some(job.job_id.clone()),
            Some(kind),
            BackgroundReviewStatusV1::Started,
            BackgroundReviewReasonV1::Started,
            observed_at_unix_ms,
            attempt,
            job.input_tokens,
            Some(&job.turn_id),
        );
        BackgroundReviewDecision::Started { attempt }
    }

    /// Finish admitted job. Returns false when no matching active job exists.
    pub fn finish(
        &self,
        job_id: &str,
        completion: BackgroundReviewCompletion,
        observed_at_unix_ms: u64,
    ) -> bool {
        self.finish_with_completion(job_id, completion, observed_at_unix_ms)
    }

    /// Finish admitted job while preserving a typed learner/provider reason.
    pub fn finish_with_completion(
        &self,
        job_id: &str,
        completion: BackgroundReviewCompletion,
        observed_at_unix_ms: u64,
    ) -> bool {
        let mut state = self.state.lock().expect("background review state poisoned");
        let Some(kind) = state
            .active
            .iter()
            .find_map(|(kind, active)| (active.job.job_id == job_id).then_some(*kind))
        else {
            return false;
        };
        let active = state
            .active
            .remove(&kind)
            .expect("active job found before removal");
        let turn_input_tokens = state
            .turn_input_tokens
            .get(&active.job.turn_id)
            .copied()
            .unwrap_or_default();
        let (status, reason) = match completion {
            BackgroundReviewCompletion::Completed => (
                BackgroundReviewStatusV1::Completed,
                BackgroundReviewReasonV1::Completed,
            ),
            BackgroundReviewCompletion::Failed => (
                BackgroundReviewStatusV1::Failed,
                BackgroundReviewReasonV1::Failed,
            ),
            BackgroundReviewCompletion::FailedWithReason(reason) => {
                (BackgroundReviewStatusV1::Failed, reason)
            }
        };
        if matches!(completion, BackgroundReviewCompletion::Completed) {
            state.completed_jobs.insert(active.job.job_id.clone());
        }
        self.observe_locked_with_turn_tokens(
            &mut state,
            Some(active.job.job_id),
            Some(kind),
            status,
            reason,
            observed_at_unix_ms,
            active.attempt,
            active.job.input_tokens,
            turn_input_tokens,
        );
        true
    }

    /// Report a typed deferral when no valid job can be constructed from the
    /// host signal. No token budget is reserved.
    pub fn observe_deferred(&self, reason: BackgroundReviewReasonV1, observed_at_unix_ms: u64) {
        let mut state = self.state.lock().expect("background review state poisoned");
        self.observe_locked(
            &mut state,
            None,
            None,
            BackgroundReviewStatusV1::Deferred,
            reason,
            observed_at_unix_ms,
            0,
            0,
            None,
        );
    }

    /// Return true only for a job admitted by this scheduler. Learner
    /// execution must never bypass this check.
    pub fn is_active_job(&self, job_id: &str) -> bool {
        self.state
            .lock()
            .expect("background review state poisoned")
            .active
            .values()
            .any(|active| active.job.job_id == job_id)
    }

    /// Persist queued observations through a caller-owned durable sink. Queue
    /// entries remain intact when sink validation or IO fails.
    pub fn persist_observations<S: BackgroundReviewObservationSink + ?Sized>(
        &self,
        sink: &S,
    ) -> Result<usize, BackgroundReviewSinkError> {
        let mut state = self.state.lock().expect("background review state poisoned");
        if state.observations.is_empty() {
            return Ok(0);
        }
        let receipts = state
            .observations
            .iter()
            .cloned()
            .map(BackgroundReviewObservationReceiptV1::from_observation)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| BackgroundReviewSinkError::Invalid(error.to_string()))?;
        sink.append(&receipts)?;
        let count = receipts.len();
        state.observations.drain(..count);
        Ok(count)
    }

    /// Cancel admitted job and emit content-free cancellation observation.
    pub fn cancel(&self, job_id: &str, observed_at_unix_ms: u64) -> bool {
        let mut state = self.state.lock().expect("background review state poisoned");
        let Some(kind) = state
            .active
            .iter()
            .find_map(|(kind, active)| (active.job.job_id == job_id).then_some(*kind))
        else {
            return false;
        };
        let active = state
            .active
            .remove(&kind)
            .expect("active job found before removal");
        let turn_input_tokens = state
            .turn_input_tokens
            .get(&active.job.turn_id)
            .copied()
            .unwrap_or_default();
        self.observe_locked_with_turn_tokens(
            &mut state,
            Some(active.job.job_id),
            Some(kind),
            BackgroundReviewStatusV1::Cancelled,
            BackgroundReviewReasonV1::Cancelled,
            observed_at_unix_ms,
            active.attempt,
            active.job.input_tokens,
            turn_input_tokens,
        );
        true
    }

    /// Return number of currently admitted jobs.
    pub fn active_count(&self) -> usize {
        self.state
            .lock()
            .expect("background review state poisoned")
            .active
            .len()
    }

    /// Return aggregate input reservation consumed over scheduler lifetime.
    pub fn aggregate_input_tokens(&self) -> u64 {
        self.state
            .lock()
            .expect("background review state poisoned")
            .aggregate_input_tokens
    }

    /// Return unreserved per-turn and aggregate input budget. Missing config
    /// remains explicit instead of becoming an unbounded request.
    pub fn budget_remaining(&self, turn_id: &str) -> Option<(u64, u64)> {
        let config = self.config.as_ref()?;
        let state = self.state.lock().expect("background review state poisoned");
        let turn_used = state.turn_input_tokens.get(turn_id).copied().unwrap_or(0);
        Some((
            config.per_turn_input_budget.saturating_sub(turn_used),
            config
                .aggregate_input_budget
                .saturating_sub(state.aggregate_input_tokens),
        ))
    }

    /// Drain observations in emission order.
    pub fn drain_observations(&self) -> Vec<BackgroundReviewObservationV1> {
        let mut state = self.state.lock().expect("background review state poisoned");
        state.observations.drain(..).collect()
    }

    fn defer_locked(
        &self,
        state: &mut SchedulerState,
        job: &BackgroundReviewJobV1,
        reason: BackgroundReviewReasonV1,
        observed_at_unix_ms: u64,
    ) -> BackgroundReviewDecision {
        let attempt = state.attempts.get(&job.job_id).copied().unwrap_or_default();
        let job_id = (!job.job_id.trim().is_empty()).then(|| job.job_id.clone());
        let kind = job_id.as_ref().map(|_| job.kind);
        self.observe_locked(
            state,
            job_id,
            kind,
            BackgroundReviewStatusV1::Deferred,
            reason,
            observed_at_unix_ms,
            attempt,
            job.input_tokens,
            Some(&job.turn_id),
        );
        BackgroundReviewDecision::Deferred { reason }
    }

    fn cancel_all_locked(
        &self,
        state: &mut SchedulerState,
        reason: BackgroundReviewReasonV1,
        observed_at_unix_ms: u64,
    ) {
        let mut active_jobs: Vec<_> = state.active.drain().map(|(_, active)| active).collect();
        active_jobs.sort_by(|left, right| {
            left.job
                .job_id
                .cmp(&right.job.job_id)
                .then_with(|| left.job.kind.as_str().cmp(right.job.kind.as_str()))
        });
        for active in active_jobs {
            let turn_input_tokens = state
                .turn_input_tokens
                .get(&active.job.turn_id)
                .copied()
                .unwrap_or_default();
            self.observe_locked_with_turn_tokens(
                state,
                Some(active.job.job_id),
                Some(active.job.kind),
                BackgroundReviewStatusV1::Cancelled,
                reason,
                observed_at_unix_ms,
                active.attempt,
                active.job.input_tokens,
                turn_input_tokens,
            );
        }
    }

    fn record_observation(
        &self,
        job_id: Option<String>,
        kind: Option<BackgroundReviewJobKindV1>,
        status: BackgroundReviewStatusV1,
        reason: BackgroundReviewReasonV1,
        observed_at_unix_ms: u64,
        attempt: u8,
        input_tokens: u64,
    ) {
        let mut state = self.state.lock().expect("background review state poisoned");
        self.observe_locked(
            &mut state,
            job_id,
            kind,
            status,
            reason,
            observed_at_unix_ms,
            attempt,
            input_tokens,
            None,
        );
    }

    fn observe_locked(
        &self,
        state: &mut SchedulerState,
        job_id: Option<String>,
        kind: Option<BackgroundReviewJobKindV1>,
        status: BackgroundReviewStatusV1,
        reason: BackgroundReviewReasonV1,
        observed_at_unix_ms: u64,
        attempt: u8,
        input_tokens: u64,
        turn_id: Option<&str>,
    ) {
        let turn_input_tokens = turn_id
            .and_then(|id| state.turn_input_tokens.get(id))
            .copied()
            .unwrap_or_default();
        self.observe_locked_with_turn_tokens(
            state,
            job_id,
            kind,
            status,
            reason,
            observed_at_unix_ms,
            attempt,
            input_tokens,
            turn_input_tokens,
        );
    }

    fn observe_locked_with_turn_tokens(
        &self,
        state: &mut SchedulerState,
        job_id: Option<String>,
        kind: Option<BackgroundReviewJobKindV1>,
        status: BackgroundReviewStatusV1,
        reason: BackgroundReviewReasonV1,
        observed_at_unix_ms: u64,
        attempt: u8,
        input_tokens: u64,
        turn_input_tokens: u64,
    ) {
        state.observations.push_back(BackgroundReviewObservationV1 {
            schema_version: BackgroundReviewObservationV1::SCHEMA_VERSION,
            job_id,
            kind,
            status,
            reason,
            observed_at_unix_ms,
            attempt,
            input_tokens,
            turn_input_tokens,
            aggregate_input_tokens: state.aggregate_input_tokens,
            activity_units: state.activity_units,
            hub_active: state.hub_active,
            foreground_active: state.foreground_active,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn config() -> BackgroundReviewConfigV1 {
        BackgroundReviewConfigV1 {
            schema_version: 1,
            enabled: true,
            min_elapsed_ms: 10,
            activity_threshold: 2,
            per_turn_input_budget: 10,
            aggregate_input_budget: 15,
            cancellation_timeout_ms: 5,
        }
    }

    fn job(
        id: &str,
        kind: BackgroundReviewJobKindV1,
        turn: &str,
        tokens: u64,
    ) -> BackgroundReviewJobV1 {
        BackgroundReviewJobV1 {
            schema_version: 1,
            job_id: id.into(),
            kind,
            turn_id: turn.into(),
            input_tokens: tokens,
            requested_at_unix_ms: 0,
        }
    }

    fn scheduler() -> BackgroundReviewScheduler {
        BackgroundReviewScheduler::new(config()).unwrap()
    }

    // ---- CTX-023 / CTX-024 deterministic first-party provider ------------

    fn deterministic_event(seq: u64, id: &str, text: &str) -> BackgroundReviewSessionEventV1 {
        let payload = serde_json::json!({ "text": text });
        BackgroundReviewSessionEventV1 {
            schema_version: 1,
            session_id: "session-1".to_string(),
            seq,
            event_id: id.to_string(),
            event_type: "assistant_message".to_string(),
            payload,
            scope_id: "scope".to_string(),
            authority: "observed".to_string(),
            influence_class: "episodic".to_string(),
            lifecycle: "active".to_string(),
            retention: "session".to_string(),
            provenance: Vec::new(),
            occurred_at_ms: seq,
            recorded_at_ms: seq,
            content_hash: format!("sha256:{seq:064}"),
        }
    }

    fn deterministic_window() -> Vec<BackgroundReviewSessionEventV1> {
        vec![
            deterministic_event(1, "e1", "The release pipeline uses the windows signing runner."),
            deterministic_event(2, "e2", "the release pipeline uses the Windows signing runner"),
            deterministic_event(
                3,
                "e3",
                "The release pipeline does not use the windows signing runner.",
            ),
            deterministic_event(4, "e4", "The staging index is stale after four hours."),
            deterministic_event(5, "e5", "The staging index is stale after two hours."),
        ]
    }

    fn deterministic_request(
        kind: BackgroundReviewJobKindV1,
        foreground_memory_state: BackgroundReviewForegroundMemoryStateV1,
        per_turn_budget_remaining: u64,
        aggregate_budget_remaining: u64,
    ) -> BackgroundSemanticReviewRequestV1 {
        let request = BackgroundSemanticReviewRequestV1 {
            schema_version: BackgroundSemanticReviewRequestV1::SCHEMA_VERSION,
            job_id: "job-1".to_string(),
            job_kind: kind,
            session_id: "session-1".to_string(),
            task_id: None,
            turn_id: "turn-1".to_string(),
            cursor: BackgroundReviewCursorV1 {
                session_id: "session-1".to_string(),
                last_seq: 0,
            },
            events: deterministic_window(),
            review_input_selection: None,
            foreground_memory_state,
            per_turn_budget_remaining,
            aggregate_budget_remaining,
            deadline_unix_ms: 10_000,
            restricted_capabilities: vec![
                BACKGROUND_SEMANTIC_CAPABILITY.to_string(),
                "no_durable_writes".to_string(),
            ],
        };
        request.validate().expect("test request is well formed");
        request
    }

    fn deterministic_provider() -> DeterministicFirstPartySemanticReviewProvider {
        DeterministicFirstPartySemanticReviewProvider::new().with_now_unix_ms(1_000)
    }

    #[test]
    fn deterministic_provider_produces_curation_proposals_from_the_event_window() {
        let request = deterministic_request(
            BackgroundReviewJobKindV1::CortexSemanticDream,
            BackgroundReviewForegroundMemoryStateV1::AvailableNoEmission,
            100_000,
            100_000,
        );
        let result = deterministic_provider()
            .execute(&request)
            .expect("deterministic provider runs in process");
        result
            .validate_against(&request)
            .expect("result is bound to its request");
        assert_eq!(result.status, BackgroundSemanticReviewStatusV1::Proposals);
        assert!(result.memory_candidates.is_empty());
        let proposals = result
            .curation_proposals
            .iter()
            .map(|value| serde_json::from_value::<SemanticCurationProposalV1>(value.clone()))
            .collect::<Result<Vec<_>, _>>()
            .expect("proposals decode");
        validate_semantic_curation_proposals(&proposals).expect("proposals are well formed");
        let kinds = proposals
            .iter()
            .map(|proposal| proposal.kind)
            .collect::<Vec<_>>();
        assert!(kinds.contains(&cortex_core::review::SemanticCurationKindV1::NearDuplicate));
        assert!(kinds.contains(&cortex_core::review::SemanticCurationKindV1::Contradiction));
        assert!(kinds.contains(&cortex_core::review::SemanticCurationKindV1::Supersession));
    }

    #[test]
    fn deterministic_provider_never_claims_a_model_and_names_itself_truthfully() {
        let request = deterministic_request(
            BackgroundReviewJobKindV1::CortexSemanticDream,
            BackgroundReviewForegroundMemoryStateV1::AvailableNoEmission,
            100_000,
            100_000,
        );
        let result = deterministic_provider().execute(&request).expect("runs");
        assert!(result.model.is_none(), "no model ran");
        assert!(result.usage.is_none(), "nothing measured, nothing claimed");
        assert_eq!(
            result.provider.as_deref(),
            Some(DeterministicFirstPartySemanticReviewProvider::provider_label().as_str())
        );
        assert!(result
            .provenance_receipt
            .source
            .contains("cortex-deterministic-review-analyzer"));
        result
            .provenance_receipt
            .validate()
            .expect("receipt is well formed");
    }

    #[test]
    fn deterministic_proposals_are_proposal_only_with_recoverable_parents() {
        let request = deterministic_request(
            BackgroundReviewJobKindV1::CortexSemanticDream,
            BackgroundReviewForegroundMemoryStateV1::AvailableNoEmission,
            100_000,
            100_000,
        );
        let result = deterministic_provider().execute(&request).expect("runs");
        let known = request
            .events
            .iter()
            .map(|event| event.event_id.as_str())
            .collect::<HashSet<_>>();
        for value in &result.curation_proposals {
            let proposal =
                serde_json::from_value::<SemanticCurationProposalV1>(value.clone()).expect("decodes");
            // Nothing is admitted or targeted at durable memory by the producer.
            assert!(proposal.target_memory_ids.is_empty());
            let parents = proposal
                .evidence
                .iter()
                .flat_map(|evidence| evidence.source_event_ids.iter())
                .collect::<Vec<_>>();
            assert_eq!(parents.len(), 2);
            for parent in parents {
                assert!(known.contains(parent.as_str()));
            }
        }
    }

    #[test]
    fn deterministic_provider_truncates_the_window_to_the_remaining_budget() {
        let events = deterministic_window();
        let budget = events
            .iter()
            .take(2)
            .map(|event| cortex_core::estimate_tokens(&event.payload.to_string()) as u64)
            .sum::<u64>();
        let request = deterministic_request(
            BackgroundReviewJobKindV1::CortexSemanticDream,
            BackgroundReviewForegroundMemoryStateV1::AvailableNoEmission,
            budget,
            100_000,
        );
        let result = deterministic_provider().execute(&request).expect("runs");
        result.validate_against(&request).expect("result validates");
        // Only the affordable prefix is consumed, so nothing is silently lost.
        assert_eq!(
            result.next_cursor.as_ref().map(|cursor| cursor.last_seq),
            Some(2)
        );
        for value in &result.curation_proposals {
            let proposal =
                serde_json::from_value::<SemanticCurationProposalV1>(value.clone()).expect("decodes");
            for evidence in &proposal.evidence {
                for id in &evidence.source_event_ids {
                    assert!(id == "e1" || id == "e2", "unbudgeted event {id} was analyzed");
                }
            }
        }
    }

    #[test]
    fn deterministic_provider_refuses_exhausted_budgets_and_expired_deadlines() {
        let per_turn = deterministic_request(
            BackgroundReviewJobKindV1::CortexSemanticDream,
            BackgroundReviewForegroundMemoryStateV1::AvailableNoEmission,
            0,
            100_000,
        );
        let result = deterministic_provider().execute(&per_turn).expect("runs");
        assert_eq!(
            result.status,
            BackgroundSemanticReviewStatusV1::Blocked {
                reason: BackgroundReviewReasonV1::PerTurnBudgetExceeded
            }
        );
        result.validate_against(&per_turn).expect("validates");

        let aggregate = deterministic_request(
            BackgroundReviewJobKindV1::CortexSemanticDream,
            BackgroundReviewForegroundMemoryStateV1::AvailableNoEmission,
            100_000,
            0,
        );
        let result = deterministic_provider().execute(&aggregate).expect("runs");
        assert_eq!(
            result.status,
            BackgroundSemanticReviewStatusV1::Blocked {
                reason: BackgroundReviewReasonV1::AggregateBudgetExceeded
            }
        );

        let expired = deterministic_request(
            BackgroundReviewJobKindV1::CortexSemanticDream,
            BackgroundReviewForegroundMemoryStateV1::AvailableNoEmission,
            100_000,
            100_000,
        );
        let result = DeterministicFirstPartySemanticReviewProvider::new()
            .with_now_unix_ms(expired.deadline_unix_ms)
            .execute(&expired)
            .expect("runs");
        assert_eq!(
            result.status,
            BackgroundSemanticReviewStatusV1::Blocked {
                reason: BackgroundReviewReasonV1::TimeGate
            }
        );
        assert!(result.curation_proposals.is_empty() && result.memory_candidates.is_empty());
        result.validate_against(&expired).expect("validates");
    }

    #[test]
    fn deterministic_provider_does_not_impersonate_an_adapt_behavioral_learner() {
        let request = deterministic_request(
            BackgroundReviewJobKindV1::AdaptBehavioralReview,
            BackgroundReviewForegroundMemoryStateV1::AvailableNoEmission,
            100_000,
            100_000,
        );
        let result = deterministic_provider().execute(&request).expect("runs");
        assert_eq!(
            result.status,
            BackgroundSemanticReviewStatusV1::Blocked {
                reason: BackgroundReviewReasonV1::SemanticProviderNotWired
            }
        );
    }

    #[test]
    fn ctx_024_extraction_runs_only_when_foreground_memory_does_not_cover_the_range() {
        // Foreground memory already covers seq 1..6: background extraction must
        // produce nothing at all.
        let covered = deterministic_request(
            BackgroundReviewJobKindV1::CortexMemoryCandidateExtraction,
            BackgroundReviewForegroundMemoryStateV1::AvailableEmission {
                range: BackgroundReviewEventRangeV1 {
                    start_seq: 1,
                    end_seq: 6,
                },
            },
            100_000,
            100_000,
        );
        let result = deterministic_provider().execute(&covered).expect("runs");
        result.validate_against(&covered).expect("validates");
        assert!(result.memory_candidates.is_empty(), "foreground already covers the range");
        assert!(result.next_cursor.is_none());

        // Foreground memory is authoritative but emitted nothing here: extraction
        // is permitted.
        let uncovered = deterministic_request(
            BackgroundReviewJobKindV1::CortexMemoryCandidateExtraction,
            BackgroundReviewForegroundMemoryStateV1::AvailableNoEmission,
            100_000,
            100_000,
        );
        let result = deterministic_provider().execute(&uncovered).expect("runs");
        result.validate_against(&uncovered).expect("validates");
        assert_eq!(result.memory_candidates.len(), 1);
        assert!(result.curation_proposals.is_empty());
        let candidate =
            serde_json::from_value::<MemoryCandidateV1>(result.memory_candidates[0].clone())
                .expect("candidate decodes");
        assert_eq!(
            candidate.source_event_ids,
            uncovered
                .events
                .iter()
                .map(|event| event.event_id.clone())
                .collect::<Vec<_>>()
        );
        assert!(candidate.content.contains("release pipeline"));

        // A disjoint foreground emission does not cover this cursor range.
        let disjoint = deterministic_request(
            BackgroundReviewJobKindV1::CortexMemoryCandidateExtraction,
            BackgroundReviewForegroundMemoryStateV1::AvailableEmission {
                range: BackgroundReviewEventRangeV1 {
                    start_seq: 40,
                    end_seq: 50,
                },
            },
            100_000,
            100_000,
        );
        let result = deterministic_provider().execute(&disjoint).expect("runs");
        assert_eq!(result.memory_candidates.len(), 1);

        // No authoritative foreground signal at all: fail closed.
        let unavailable = deterministic_request(
            BackgroundReviewJobKindV1::CortexMemoryCandidateExtraction,
            BackgroundReviewForegroundMemoryStateV1::Unavailable,
            100_000,
            100_000,
        );
        let result = deterministic_provider().execute(&unavailable).expect("runs");
        assert_eq!(
            result.status,
            BackgroundSemanticReviewStatusV1::Blocked {
                reason: BackgroundReviewReasonV1::ForegroundMemoryEmissionSignalUnavailable
            }
        );
        assert!(result.memory_candidates.is_empty());
    }

    #[derive(Debug, Default)]
    struct RecordingProposalSink {
        curation: Mutex<Vec<SemanticCurationProposalV1>>,
        candidates: Mutex<Vec<MemoryCandidateV1>>,
    }

    impl BackgroundReviewProposalAdmission for RecordingProposalSink {
        fn admit_curation(
            &self,
            _job: &BackgroundReviewJobV1,
            proposals: &[SemanticCurationProposalV1],
        ) -> Result<(), BackgroundReviewReasonV1> {
            self.curation.lock().unwrap().extend_from_slice(proposals);
            Ok(())
        }

        fn admit_memory_candidates(
            &self,
            _job: &BackgroundReviewJobV1,
            candidates: &[MemoryCandidateV1],
        ) -> Result<(), BackgroundReviewReasonV1> {
            self.candidates.lock().unwrap().extend_from_slice(candidates);
            Ok(())
        }
    }

    #[test]
    fn deterministic_provider_produces_proposals_through_the_governed_path() {
        let scheduler = BackgroundReviewScheduler::new(BackgroundReviewConfigV1 {
            per_turn_input_budget: 100_000,
            aggregate_input_budget: 200_000,
            ..config()
        })
        .expect("scheduler config is valid");
        scheduler.set_hub_active(true, 0);
        scheduler.record_activity(4);
        let job = job(
            "job-1",
            BackgroundReviewJobKindV1::CortexSemanticDream,
            "turn-1",
            1,
        );
        assert!(matches!(
            scheduler.start(job.clone(), 0),
            BackgroundReviewDecision::Started { .. }
        ));
        let input = BackgroundSemanticReviewInputV1 {
            task_id: None,
            cursor: EventCursor {
                session_id: "session-1".to_string(),
                last_seq: 0,
            },
            events: deterministic_window().iter().map(core_event).collect(),
            reviewed_baseline: Vec::new(),
            foreground_memory_state: ForegroundMemoryStateV1::AvailableNoEmission,
        };
        let sink = RecordingProposalSink::default();
        let cursor_store = BackgroundReviewCursorStore::default();
        cursor_store
            .set_initial(EventCursor {
                session_id: "session-1".to_string(),
                last_seq: 0,
            })
            .expect("cursor seeds");
        let execution = execute_background_semantic_review(
            &scheduler,
            &job,
            &input,
            &deterministic_provider(),
            Some(&sink),
            &cursor_store,
            10_000,
            1,
        );
        assert_eq!(
            execution.status,
            BackgroundReviewExecutionStatusV1::Proposals,
            "{execution:?}"
        );
        assert!(!execution.proposals.is_empty(), "governed path admitted proposals");
        assert!(sink.candidates.lock().unwrap().is_empty());
        assert!(!sink.curation.lock().unwrap().is_empty());
    }

    #[test]
    fn elapsed_and_activity_gates_both_apply() {
        let scheduler = scheduler();
        scheduler.set_hub_active(true, 0);
        scheduler.record_activity(2);
        assert_eq!(
            scheduler.start(
                job(
                    "job-1",
                    BackgroundReviewJobKindV1::AdaptBehavioralReview,
                    "turn-1",
                    5
                ),
                0,
            ),
            BackgroundReviewDecision::Started { attempt: 1 }
        );
        assert!(scheduler.finish("job-1", BackgroundReviewCompletion::Completed, 1));
        scheduler.record_activity(2);
        assert_eq!(
            scheduler.start(
                job(
                    "job-2",
                    BackgroundReviewJobKindV1::CortexSemanticDream,
                    "turn-2",
                    5
                ),
                5,
            ),
            BackgroundReviewDecision::Deferred {
                reason: BackgroundReviewReasonV1::TimeGate
            }
        );
        assert_eq!(
            scheduler.start(
                job(
                    "job-2",
                    BackgroundReviewJobKindV1::CortexSemanticDream,
                    "turn-2",
                    5
                ),
                10,
            ),
            BackgroundReviewDecision::Started { attempt: 1 }
        );
        assert!(scheduler.finish("job-2", BackgroundReviewCompletion::Completed, 11));
        assert_eq!(scheduler.active_count(), 0);
    }

    #[test]
    fn missing_activity_is_reported_after_elapsed_gate() {
        let scheduler = scheduler();
        scheduler.set_hub_active(true, 0);
        scheduler.record_activity(2);
        assert!(matches!(
            scheduler.start(
                job(
                    "job-1",
                    BackgroundReviewJobKindV1::AdaptBehavioralReview,
                    "turn-1",
                    5
                ),
                0,
            ),
            BackgroundReviewDecision::Started { .. }
        ));
        assert!(scheduler.finish("job-1", BackgroundReviewCompletion::Completed, 1));
        assert_eq!(
            scheduler.start(
                job(
                    "job-2",
                    BackgroundReviewJobKindV1::CortexSemanticDream,
                    "turn-2",
                    5
                ),
                10,
            ),
            BackgroundReviewDecision::Deferred {
                reason: BackgroundReviewReasonV1::ActivityGate
            }
        );
    }

    #[test]
    fn idle_observation_reports_time_and_activity_gates() {
        let scheduler = scheduler();
        scheduler.set_hub_active(true, 0);
        scheduler.record_activity(2);
        assert!(matches!(
            scheduler.start(
                job(
                    "job-1",
                    BackgroundReviewJobKindV1::AdaptBehavioralReview,
                    "turn-1",
                    1
                ),
                0,
            ),
            BackgroundReviewDecision::Started { .. }
        ));
        assert!(scheduler.finish("job-1", BackgroundReviewCompletion::Completed, 0));
        scheduler.drain_observations();

        scheduler.observe_idle(5);
        assert_eq!(
            scheduler.drain_observations()[0].reason,
            BackgroundReviewReasonV1::TimeGate
        );
        scheduler.observe_idle(10);
        assert_eq!(
            scheduler.drain_observations()[0].reason,
            BackgroundReviewReasonV1::ActivityGate
        );
    }

    #[test]
    fn single_flight_and_one_retry_are_enforced() {
        let scheduler = scheduler();
        scheduler.set_hub_active(true, 0);
        scheduler.record_activity(2);
        let first = job(
            "job-1",
            BackgroundReviewJobKindV1::AdaptBehavioralReview,
            "turn-1",
            5,
        );
        assert_eq!(
            scheduler.start(first.clone(), 0),
            BackgroundReviewDecision::Started { attempt: 1 }
        );
        scheduler.record_activity(2);
        assert_eq!(
            scheduler.start(
                job(
                    "job-2",
                    BackgroundReviewJobKindV1::AdaptBehavioralReview,
                    "turn-2",
                    1
                ),
                10,
            ),
            BackgroundReviewDecision::Deferred {
                reason: BackgroundReviewReasonV1::AlreadyRunning
            }
        );
        assert!(scheduler.finish("job-1", BackgroundReviewCompletion::Failed, 10));
        assert_eq!(
            scheduler.start(first.clone(), 10),
            BackgroundReviewDecision::Started { attempt: 2 }
        );
        assert!(scheduler.finish("job-1", BackgroundReviewCompletion::Failed, 20));
        scheduler.record_activity(2);
        assert_eq!(
            scheduler.start(first, 20),
            BackgroundReviewDecision::Deferred {
                reason: BackgroundReviewReasonV1::RetryLimit
            }
        );
    }

    #[test]
    fn retry_cannot_change_job_identity() {
        let scheduler = scheduler();
        scheduler.set_hub_active(true, 0);
        scheduler.record_activity(2);
        let first = job(
            "job-1",
            BackgroundReviewJobKindV1::AdaptBehavioralReview,
            "turn-1",
            5,
        );
        assert!(matches!(
            scheduler.start(first, 0),
            BackgroundReviewDecision::Started { attempt: 1 }
        ));
        assert!(scheduler.finish("job-1", BackgroundReviewCompletion::Failed, 0));
        scheduler.record_activity(2);
        assert_eq!(
            scheduler.start(
                job(
                    "job-1",
                    BackgroundReviewJobKindV1::CortexSemanticDream,
                    "turn-1",
                    5,
                ),
                10,
            ),
            BackgroundReviewDecision::Deferred {
                reason: BackgroundReviewReasonV1::InvalidJob
            }
        );
    }

    #[test]
    fn completed_job_id_is_terminal_and_cannot_be_replayed() {
        let scheduler = scheduler();
        scheduler.set_hub_active(true, 0);
        scheduler.record_activity(2);
        let completed = job(
            "job-1",
            BackgroundReviewJobKindV1::AdaptBehavioralReview,
            "turn-1",
            5,
        );
        assert!(matches!(
            scheduler.start(completed.clone(), 0),
            BackgroundReviewDecision::Started { attempt: 1 }
        ));
        assert!(scheduler.finish("job-1", BackgroundReviewCompletion::Completed, 0));
        scheduler.record_activity(2);
        assert_eq!(
            scheduler.start(completed, 10),
            BackgroundReviewDecision::Deferred {
                reason: BackgroundReviewReasonV1::RetryLimit
            }
        );
    }

    #[test]
    fn per_turn_and_aggregate_budgets_are_reserved_on_start() {
        let scheduler = scheduler();
        scheduler.set_hub_active(true, 0);
        scheduler.record_activity(2);
        assert!(matches!(
            scheduler.start(
                job(
                    "job-1",
                    BackgroundReviewJobKindV1::AdaptBehavioralReview,
                    "turn-1",
                    10
                ),
                0,
            ),
            BackgroundReviewDecision::Started { .. }
        ));
        assert!(scheduler.finish("job-1", BackgroundReviewCompletion::Completed, 10));
        scheduler.record_activity(2);
        assert_eq!(
            scheduler.start(
                job(
                    "job-2",
                    BackgroundReviewJobKindV1::CortexSemanticDream,
                    "turn-1",
                    1
                ),
                10,
            ),
            BackgroundReviewDecision::Deferred {
                reason: BackgroundReviewReasonV1::PerTurnBudgetExceeded
            }
        );
        scheduler.record_activity(2);
        assert_eq!(
            scheduler.start(
                job(
                    "job-3",
                    BackgroundReviewJobKindV1::CortexSemanticDream,
                    "turn-2",
                    6
                ),
                20,
            ),
            BackgroundReviewDecision::Deferred {
                reason: BackgroundReviewReasonV1::AggregateBudgetExceeded
            }
        );
        assert_eq!(scheduler.aggregate_input_tokens(), 10);
        assert!(scheduler
            .drain_observations()
            .iter()
            .any(|observation| observation.reason
                == BackgroundReviewReasonV1::AggregateBudgetExceeded));
    }

    #[test]
    fn foreground_preemption_cancels_active_work() {
        let scheduler = scheduler();
        scheduler.set_hub_active(true, 0);
        scheduler.record_activity(2);
        assert!(matches!(
            scheduler.start(
                job(
                    "job-1",
                    BackgroundReviewJobKindV1::AdaptBehavioralReview,
                    "turn-1",
                    1
                ),
                0,
            ),
            BackgroundReviewDecision::Started { .. }
        ));
        scheduler.set_foreground_active(true, 1);
        assert_eq!(scheduler.active_count(), 0);
        assert_eq!(
            scheduler.start(
                job(
                    "job-2",
                    BackgroundReviewJobKindV1::CortexSemanticDream,
                    "turn-2",
                    1
                ),
                10,
            ),
            BackgroundReviewDecision::Deferred {
                reason: BackgroundReviewReasonV1::ForegroundPreempted
            }
        );
        assert!(
            scheduler
                .drain_observations()
                .iter()
                .any(|observation| observation.reason
                    == BackgroundReviewReasonV1::ForegroundPreempted)
        );
        scheduler.set_foreground_active(false, 2);
    }

    #[test]
    fn manual_cancellation_is_immediate_and_observable() {
        let scheduler = scheduler();
        scheduler.set_hub_active(true, 0);
        scheduler.record_activity(2);
        assert!(matches!(
            scheduler.start(
                job(
                    "job-1",
                    BackgroundReviewJobKindV1::AdaptBehavioralReview,
                    "turn-1",
                    1
                ),
                0,
            ),
            BackgroundReviewDecision::Started { .. }
        ));
        assert!(scheduler.cancel("job-1", 1));
        assert_eq!(scheduler.active_count(), 0);
        assert!(scheduler
            .drain_observations()
            .iter()
            .any(
                |observation| observation.status == BackgroundReviewStatusV1::Cancelled
                    && observation.reason == BackgroundReviewReasonV1::Cancelled
            ));
        assert!(!scheduler.cancel("job-1", 2));
    }

    #[test]
    fn hub_inactive_and_config_unavailable_are_observable() {
        let scheduler = scheduler();
        assert_eq!(
            scheduler.start(
                job(
                    "job-1",
                    BackgroundReviewJobKindV1::AdaptBehavioralReview,
                    "turn-1",
                    1
                ),
                0,
            ),
            BackgroundReviewDecision::Deferred {
                reason: BackgroundReviewReasonV1::HubInactive
            }
        );
        scheduler.observe_idle(1);
        assert!(scheduler
            .drain_observations()
            .iter()
            .all(|observation| observation.reason == BackgroundReviewReasonV1::HubInactive));

        let unavailable = BackgroundReviewScheduler::from_config_path(
            PathBuf::from("this-path-does-not-exist/background-review.json"),
            2,
        );
        assert_eq!(
            unavailable.start(
                job(
                    "job-2",
                    BackgroundReviewJobKindV1::AdaptBehavioralReview,
                    "turn-2",
                    1
                ),
                2,
            ),
            BackgroundReviewDecision::Deferred {
                reason: BackgroundReviewReasonV1::ConfigUnavailable
            }
        );
        assert!(unavailable
            .drain_observations()
            .iter()
            .any(|observation| observation.reason == BackgroundReviewReasonV1::ConfigUnavailable));
    }

    #[test]
    fn disabled_config_never_admits_job() {
        let mut config = config();
        config.enabled = false;
        let scheduler = BackgroundReviewScheduler::new(config).unwrap();
        scheduler.set_hub_active(true, 0);
        scheduler.record_activity(100);
        assert_eq!(
            scheduler.start(
                job(
                    "job-1",
                    BackgroundReviewJobKindV1::AdaptBehavioralReview,
                    "turn-1",
                    1
                ),
                0,
            ),
            BackgroundReviewDecision::Deferred {
                reason: BackgroundReviewReasonV1::Disabled
            }
        );
    }

    #[test]
    fn invalid_job_is_deferred_with_valid_observation_shape() {
        let scheduler = scheduler();
        let mut invalid = job(
            "",
            BackgroundReviewJobKindV1::AdaptBehavioralReview,
            "turn-1",
            1,
        );
        invalid.schema_version = 99;
        assert_eq!(
            scheduler.start(invalid, 0),
            BackgroundReviewDecision::Deferred {
                reason: BackgroundReviewReasonV1::InvalidJob
            }
        );
        let observations = scheduler.drain_observations();
        assert_eq!(observations.len(), 1);
        observations[0].validate().unwrap();
    }

    #[test]
    fn producer_reports_missing_model_input_without_admitting_work() {
        let scheduler = scheduler();
        scheduler.set_hub_active(true, 0);
        let producer = BackgroundReviewProducer::new(&scheduler);
        let signal = BackgroundReviewActivitySignalV1 {
            schema_version: BackgroundReviewActivitySignalV1::SCHEMA_VERSION,
            session_id: "session-1".into(),
            turn_id: "turn-1".into(),
            activity_units: 2,
            input_tokens: None,
            foreground_active: false,
            observed_at_unix_ms: 0,
        };
        assert_eq!(
            producer.admit(&signal, BackgroundReviewJobKindV1::CortexSemanticDream, 0),
            BackgroundReviewProduction::Deferred {
                reason: BackgroundReviewReasonV1::ModelInputUnavailable
            }
        );
        assert_eq!(scheduler.active_count(), 0);
        assert_eq!(
            scheduler.drain_observations()[0].reason,
            BackgroundReviewReasonV1::ModelInputUnavailable
        );
    }

    #[test]
    fn producer_admits_only_host_measured_work_and_default_learner_blocks() {
        let scheduler = scheduler();
        scheduler.set_hub_active(true, 0);
        let producer = BackgroundReviewProducer::new(&scheduler);
        let signal = BackgroundReviewActivitySignalV1 {
            schema_version: BackgroundReviewActivitySignalV1::SCHEMA_VERSION,
            session_id: "session-1".into(),
            turn_id: "turn-1".into(),
            activity_units: 2,
            input_tokens: Some(5),
            foreground_active: false,
            observed_at_unix_ms: 0,
        };
        let BackgroundReviewProduction::Started { job, attempt } =
            producer.admit(&signal, BackgroundReviewJobKindV1::CortexSemanticDream, 0)
        else {
            panic!("host-provided signal should pass configured gates");
        };
        assert_eq!(attempt, 1);
        assert!(scheduler.is_active_job(&job.job_id));
        let execution =
            execute_background_review(&scheduler, &job, &NoSemanticReviewLearner, None, 1);
        execution.validate().unwrap();
        assert_eq!(execution.status, BackgroundReviewExecutionStatusV1::Blocked);
        assert_eq!(
            execution.reason,
            Some(BackgroundReviewReasonV1::SemanticProviderNotWired)
        );
        assert!(!scheduler.is_active_job(&job.job_id));
        assert!(scheduler.drain_observations().iter().any(|observation| {
            observation.status == BackgroundReviewStatusV1::Failed
                && observation.reason == BackgroundReviewReasonV1::SemanticProviderNotWired
        }));
    }

    struct TestLearner;

    impl BackgroundReviewLearner for TestLearner {
        fn execute(&self, _job: &BackgroundReviewJobV1) -> BackgroundReviewLearnerResult {
            BackgroundReviewLearnerResult::Proposals(vec![BackgroundReviewProposalRefV1 {
                proposal_id: "proposal-1".into(),
                proposal_digest:
                    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            }])
        }
    }

    struct TestProposalSink;

    impl BackgroundReviewProposalSink for TestProposalSink {
        fn submit(
            &self,
            _job: &BackgroundReviewJobV1,
            _proposals: &[BackgroundReviewProposalRefV1],
        ) -> Result<(), BackgroundReviewReasonV1> {
            Ok(())
        }
    }

    #[test]
    fn learner_result_is_proposal_only_and_requires_explicit_sink() {
        let scheduler = scheduler();
        scheduler.set_hub_active(true, 0);
        scheduler.record_activity(2);
        let job = job(
            "job-1",
            BackgroundReviewJobKindV1::AdaptBehavioralReview,
            "turn-1",
            1,
        );
        assert!(matches!(
            scheduler.start(job.clone(), 0),
            BackgroundReviewDecision::Started { .. }
        ));
        let blocked = execute_background_review(&scheduler, &job, &TestLearner, None, 1);
        blocked.validate().unwrap();
        assert_eq!(
            blocked.reason,
            Some(BackgroundReviewReasonV1::ProposalSinkUnavailable)
        );

        scheduler.record_activity(2);
        assert!(matches!(
            scheduler.start(job.clone(), 10),
            BackgroundReviewDecision::Started { .. }
        ));
        let accepted =
            execute_background_review(&scheduler, &job, &TestLearner, Some(&TestProposalSink), 11);
        accepted.validate().unwrap();
        assert_eq!(
            accepted.status,
            BackgroundReviewExecutionStatusV1::Proposals
        );
        assert_eq!(accepted.proposals.len(), 1);
    }

    #[test]
    fn a_full_sink_rotates_instead_of_refusing_every_later_write() {
        let scheduler = scheduler();
        scheduler.observe_idle(1);
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("h5.jsonl");
        // Fill the sidecar to its cap, the state a long-running daemon reaches.
        std::fs::write(&path, vec![b'x'; MAX_OBSERVATION_FILE_BYTES as usize - 16]).unwrap();
        let sink = JsonlBackgroundReviewObservationSink::new(&path);

        assert_eq!(scheduler.persist_observations(&sink).unwrap(), 1);

        let rotated = path.with_extension("jsonl.1");
        assert!(rotated.is_file(), "the previous generation is kept");
        let current = std::fs::read_to_string(&path).unwrap();
        assert!(
            current.lines().count() == 1,
            "the live sidecar restarts with the write that triggered rotation"
        );
        assert!(
            std::fs::metadata(&path).unwrap().len() < MAX_OBSERVATION_FILE_BYTES,
            "the cap still bounds the live file"
        );
    }


    #[test]
    fn durable_sink_is_bounded_and_preserves_queue_on_success() {
        let scheduler = scheduler();
        scheduler.observe_idle(1);
        let directory = tempfile::tempdir().unwrap();
        let sink = JsonlBackgroundReviewObservationSink::new(
            directory.path().join("nested").join("h5.jsonl"),
        );
        assert_eq!(scheduler.persist_observations(&sink).unwrap(), 1);
        assert_eq!(scheduler.persist_observations(&sink).unwrap(), 0);
        let encoded = std::fs::read_to_string(sink.path()).unwrap();
        assert!(encoded.contains("background-review-"));
        assert!(!encoded.contains("prompt"));
        assert!(encoded.len() <= MAX_OBSERVATION_FILE_BYTES as usize);
    }
}
