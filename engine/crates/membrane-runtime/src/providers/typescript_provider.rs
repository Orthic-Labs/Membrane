//! TypeScript D1 adapter over `tsgo`/`tsserver` (design §6 ladder, §13
//! containment).
//!
//! The adapter probes `tsgo` and then `tsserver` on the injected search path —
//! it never installs anything — and speaks the tsserver newline-delimited JSON
//! protocol over stdio with a sanitized environment. Synchronization is
//! full-content: on every epoch change previously opened files are closed and
//! the epoch's changed files are re-opened with their exact bytes, so the
//! server's document versions are pinned to sealed workspace epochs. Each
//! acquisition issues one `geterr` request; the `requestCompleted` event for
//! that exact request sequence is the proven completion barrier, so the lane
//! claims [`ConvergenceClass::PushVersionedExact`] only when the response for
//! the exact synchronized document versions arrived.
//!
//! Diagnostics map to [`ObservationV1`] with `NativeLanguageService` source
//! class, `Interactive` cost class, and `Blocking` severity hint exactly for
//! the `error` category (everything else stays advisory policy input). All
//! work is synchronous: std threads plus channels internally, never tokio.

use crate::live_diagnostics::{
    AbsoluteDeadline, CapabilityKind, ConvergenceProof, DiagnosticsProvider, ProviderCapabilities,
    ProviderError, ProviderOutput, RequestId, SideEffectClass,
};
use crate::providers::child_process::{
    default_search_path, drain_frames_until, lsp_frame_bytes, probe_search_path,
    recv_with_deadline, recv_within, sanitized_child_env, spawn_bounded_reader, spawn_sanitized,
    spawn_stderr_drainer, tsserver_line_bytes, FrameOutcome, LineFrameDecoder, LspDecoder,
    SanitizedProcess,
};
use crate::providers::identity;
use membrane_protocol::diagnostics::{
    CapabilityVocabulary, ConvergenceClass, CostClass, CoverageLaneV1, LaneState, ObservationV1,
    SeverityHint, SourceClass, SourceRange, TypedOmission, WorkspaceEpochV1,
};
use serde_json::{json, Value};
use sha2::{Digest as _, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::ChildStdin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Instant;

/// Stable provider identity used for qualification and selection.
pub const PROVIDER_ID: &str = "typescript-native-d1";
/// Adapter protocol version reported in capabilities and observations.
pub const ADAPTER_VERSION: &str = "adapter-v1";
/// Bound on queued server events before overflow drops begin.
const EVENT_QUEUE_CAPACITY: usize = 256;

/// Declared capabilities of this D1 adapter: one interactive pure-analysis
/// native language service lane with versioned-push convergence.
pub fn qualified_capabilities() -> ProviderCapabilities {
    ProviderCapabilities {
        provider_id: PROVIDER_ID.to_string(),
        version: ADAPTER_VERSION.to_string(),
        capabilities: BTreeSet::from([CapabilityKind::NativeLanguageService]),
        side_effect_class: SideEffectClass::PureAnalysis,
        convergence_class: ConvergenceClass::PushVersionedExact,
        cost_class: CostClass::Interactive,
    }
}

/// Resolve the engine binary this adapter would launch, without starting it.
/// Deterministic: first candidate found on the injected search path wins.
/// Used by the service layer to derive real `WorkspaceEngineKey` digests
/// before registration; `None` means no engine is installed and the provider
/// must not be registered under invented identity.
pub fn resolve_engine(search_path: &[PathBuf]) -> Option<(&'static str, PathBuf)> {
    for spec in CANDIDATE_SPECS {
        if let Some(binary) = probe_search_path(spec.binary_name, search_path) {
            return Some((spec.binary_name, binary));
        }
    }
    None
}

/// Real identity inputs for one resolved TypeScript engine (design §3):
/// binary digest from the engine's bytes, toolchain digest from its install
/// directory, and config digest from the project files the engine reads.
#[derive(Debug, Clone, Default)]
pub struct ProviderIdentityInputs {
    pub binary: Option<String>,
    pub toolchain: Option<String>,
    pub config: Option<String>,
}

/// Config files that define this adapter's project semantics.
const TS_CONFIG_FILES: [&str; 3] = ["tsconfig.json", "jsconfig.json", "package.json"];

pub fn identity_inputs(project_root: &Path, binary_path: &Path) -> ProviderIdentityInputs {
    ProviderIdentityInputs {
        binary: identity::binary_digest(binary_path),
        toolchain: identity::toolchain_digest(binary_path),
        config: identity::project_config_digest(project_root, &TS_CONFIG_FILES),
    }
}

// ---------------------------------------------------------------------------
// Session plumbing and TypeScript 7 LSP/tsserver dual-mode
// ---------------------------------------------------------------------------

const INITIALIZE_HANDSHAKE_TIMEOUT_MS: u64 = 15_000;
const SHUTDOWN_HANDSHAKE_TIMEOUT_MS: u64 = 5_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EngineProtocol {
    Lsp,
    Tsserver,
}

struct CandidateSpec {
    binary_name: &'static str,
    args: &'static [&'static str],
    protocol: EngineProtocol,
}

/// Probe order: prefer `tsc --lsp --stdio` (TypeScript 7 primary, LSP),
/// then `tsgo`/`typescript-go` with LSP args, fallback to `tsserver` tsserver
/// protocol. Bare `tsgo` with no args is never launched.
const CANDIDATE_SPECS: &[CandidateSpec] = &[
    CandidateSpec {
        binary_name: "tsc",
        args: &["--lsp", "--stdio"],
        protocol: EngineProtocol::Lsp,
    },
    CandidateSpec {
        binary_name: "tsgo",
        args: &["lsp", "--stdio"],
        protocol: EngineProtocol::Lsp,
    },
    CandidateSpec {
        binary_name: "tsgo",
        args: &["--lsp", "--stdio"],
        protocol: EngineProtocol::Lsp,
    },
    CandidateSpec {
        binary_name: "typescript-go",
        args: &["lsp", "--stdio"],
        protocol: EngineProtocol::Lsp,
    },
    CandidateSpec {
        binary_name: "typescript-go",
        args: &["--lsp", "--stdio"],
        protocol: EngineProtocol::Lsp,
    },
    CandidateSpec {
        binary_name: "tsserver",
        args: &[],
        protocol: EngineProtocol::Tsserver,
    },
];

fn uri_from_path(path: &Path) -> String {
    let text = path.to_string_lossy();
    let with_scheme = if text.starts_with('/') {
        format!("file://{text}")
    } else {
        format!("file:///{text}")
    };
    let mut encoded = String::with_capacity(with_scheme.len());
    for byte in with_scheme.bytes() {
        let allowed = byte.is_ascii_alphanumeric()
            || matches!(
                byte,
                b'-' | b'.'
                    | b'_'
                    | b'~'
                    | b'!'
                    | b'$'
                    | b'&'
                    | b'\''
                    | b'('
                    | b')'
                    | b'*'
                    | b'+'
                    | b','
                    | b';'
                    | b'='
                    | b':'
                    | b'@'
                    | b'/'
            );
        if allowed {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

struct TsSession {
    process: SanitizedProcess,
    stdin: Option<ChildStdin>,
    frames: Receiver<String>,
    overflow_dropped: Arc<AtomicUsize>,
    reader_handle: Option<JoinHandle<()>>,
    stderr_handle: Option<JoinHandle<()>>,
    protocol: EngineProtocol,
}

impl TsSession {
    fn write_json(&mut self, value: &Value) -> Result<(), ProviderError> {
        let wire = match self.protocol {
            EngineProtocol::Lsp => lsp_frame_bytes(value.to_string().as_bytes()),
            EngineProtocol::Tsserver => tsserver_line_bytes(&value.to_string()),
        };
        let Some(stdin) = self.stdin.as_mut() else {
            return Err(ProviderError::Crashed(format!(
                "{} stdin already closed",
                match self.protocol {
                    EngineProtocol::Lsp => "ts lsp",
                    EngineProtocol::Tsserver => "tsserver",
                }
            )));
        };
        stdin
            .write_all(&wire)
            .and_then(|()| stdin.flush())
            .map_err(|error| {
                ProviderError::Crashed(format!(
                    "{} stdin write failed: {error}",
                    match self.protocol {
                        EngineProtocol::Lsp => "ts lsp",
                        EngineProtocol::Tsserver => "tsserver",
                    }
                ))
            })
    }

    fn kill_and_join(&mut self) {
        let _ = self.stdin.take();
        self.process.kill_tree();
        if let Some(handle) = self.reader_handle.take() {
            let _ = handle.join();
        }
        if let Some(handle) = self.stderr_handle.take() {
            let _ = handle.join();
        }
    }
}

fn start_session(
    binary: &Path,
    args: &[String],
    project_root: &Path,
    search_path: &[PathBuf],
    protocol: EngineProtocol,
) -> Result<TsSession, ProviderError> {
    let env = sanitized_child_env(search_path);
    let mut process = spawn_sanitized(binary, args, project_root, &env).map_err(|error| {
        ProviderError::Unavailable(format!("failed to spawn {}: {error}", binary.display()))
    })?;
    let stdin = process.child.stdin.take();
    let stdout = process.child.stdout.take().ok_or_else(|| {
        ProviderError::Crashed(format!(
            "{} stdout was not piped",
            match protocol {
                EngineProtocol::Lsp => "ts lsp",
                EngineProtocol::Tsserver => "tsserver",
            }
        ))
    })?;
    let stderr = process.child.stderr.take().ok_or_else(|| {
        ProviderError::Crashed(format!(
            "{} stderr was not piped",
            match protocol {
                EngineProtocol::Lsp => "ts lsp",
                EngineProtocol::Tsserver => "tsserver",
            }
        ))
    })?;
    let overflow_dropped = Arc::new(AtomicUsize::new(0));
    let pump = match protocol {
        EngineProtocol::Lsp => spawn_bounded_reader(
            stdout,
            LspDecoder::new(),
            EVENT_QUEUE_CAPACITY,
            Arc::clone(&overflow_dropped),
        ),
        EngineProtocol::Tsserver => spawn_bounded_reader(
            stdout,
            LineFrameDecoder::new(),
            EVENT_QUEUE_CAPACITY,
            Arc::clone(&overflow_dropped),
        ),
    };
    let stderr_handle = spawn_stderr_drainer(stderr);
    Ok(TsSession {
        process,
        stdin,
        frames: pump.frames,
        overflow_dropped,
        reader_handle: Some(pump.handle),
        stderr_handle: Some(stderr_handle),
        protocol,
    })
}

fn await_lsp_initialize_response(
    session: &TsSession,
    request_id: i64,
) -> Result<(), ProviderError> {
    let started = Instant::now();
    loop {
        let elapsed_ms = started.elapsed().as_millis() as u64;
        if elapsed_ms >= INITIALIZE_HANDSHAKE_TIMEOUT_MS {
            return Err(ProviderError::Crashed(
                "ts lsp initialize handshake timed out".into(),
            ));
        }
        match recv_within(
            &session.frames,
            INITIALIZE_HANDSHAKE_TIMEOUT_MS - elapsed_ms,
        ) {
            FrameOutcome::Frame(text) => {
                if let Ok(value) = serde_json::from_str::<Value>(&text) {
                    if value["id"].as_i64() == Some(request_id)
                        && (value.get("result").is_some() || value.get("error").is_some())
                    {
                        return Ok(());
                    }
                }
            }
            FrameOutcome::QueueClosed => {
                return Err(ProviderError::Crashed(
                    "ts lsp exited during the initialize handshake".into(),
                ));
            }
            FrameOutcome::DeadlineExceeded => continue,
        }
    }
}

// ---------------------------------------------------------------------------
// Wire message parsing (pure, unit-testable)
// ---------------------------------------------------------------------------

/// One decoded diagnostic span from a tsserver diag event. Lines and offsets
/// are already 1-based per the tsserver protocol.
#[derive(Debug, PartialEq)]
struct TsserverSpan {
    start_line: u64,
    start_offset: u64,
    end_line: u64,
    end_offset: u64,
    message: String,
    code: String,
    category: String,
}

/// One routed server event relevant to acquisition.
#[derive(Debug, PartialEq)]
enum ServerEvent {
    /// A `syntaxDiag`/`semanticDiag`/`suggestionDiag` event for one file.
    Diagnostics {
        kind: String,
        file: String,
        spans: Vec<TsserverSpan>,
    },
    /// The completion barrier for one outstanding request sequence.
    RequestCompleted(u64),
    /// Anything else (responses to configure, telemetry, malformed lines).
    Ignored,
}

fn parse_span(value: &Value) -> Option<TsserverSpan> {
    Some(TsserverSpan {
        start_line: value["start"]["line"].as_u64()?,
        start_offset: value["start"]["offset"].as_u64()?,
        end_line: value["end"]["line"].as_u64()?,
        end_offset: value["end"]["offset"].as_u64()?,
        message: value["message"].as_str().unwrap_or_default().to_string(),
        code: match value.get("code") {
            Some(Value::Number(number)) => number.to_string(),
            Some(Value::String(text)) => text.clone(),
            _ => String::new(),
        },
        category: value["category"].as_str().unwrap_or_default().to_string(),
    })
}

fn parse_server_message(line: &str) -> ServerEvent {
    let Ok(value) = serde_json::from_str::<Value>(line) else {
        return ServerEvent::Ignored;
    };
    if value["type"].as_str() != Some("event") {
        return ServerEvent::Ignored;
    }
    match value["event"].as_str() {
        Some(kind @ ("syntaxDiag" | "semanticDiag" | "suggestionDiag")) => {
            let body = &value["body"];
            let Some(file) = body["file"].as_str() else {
                return ServerEvent::Ignored;
            };
            let spans = body["diagnostics"]
                .as_array()
                .map(|items| items.iter().filter_map(parse_span).collect::<Vec<_>>())
                .unwrap_or_default();
            ServerEvent::Diagnostics {
                kind: kind.to_string(),
                file: file.to_string(),
                spans,
            }
        }
        Some("requestCompleted") => match value["body"]["request_seq"].as_u64() {
            Some(request_seq) => ServerEvent::RequestCompleted(request_seq),
            None => ServerEvent::Ignored,
        },
        _ => ServerEvent::Ignored,
    }
}

#[derive(Debug, PartialEq)]
enum LspInboundMessage {
    Response(i64),
    Notification { method: String, params: Value },
    Ignored,
}

fn parse_lsp_message(text: &str) -> LspInboundMessage {
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return LspInboundMessage::Ignored;
    };
    if let Some(method) = value["method"].as_str() {
        return LspInboundMessage::Notification {
            method: method.to_string(),
            params: value.get("params").cloned().unwrap_or(Value::Null),
        };
    }
    if value["id"].as_i64().is_some()
        && (value.get("result").is_some() || value.get("error").is_some())
    {
        return LspInboundMessage::Response(value["id"].as_i64().unwrap_or_default());
    }
    LspInboundMessage::Ignored
}

fn lsp_ts_severity_hint(diagnostic: &Value) -> SeverityHint {
    if diagnostic["severity"].as_i64() == Some(1) {
        SeverityHint::Blocking
    } else {
        SeverityHint::Advisory
    }
}

fn lsp_ts_code_string(diagnostic: &Value) -> String {
    match diagnostic.get("code") {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Number(number)) => number.to_string(),
        _ => String::new(),
    }
}

fn lsp_ts_position_u32(value: &Value) -> u32 {
    value
        .as_u64()
        .map(|pos| u32::try_from(pos).unwrap_or(u32::MAX))
        .unwrap_or(0)
}

fn lsp_ts_range_to_source_range(diagnostic: &Value) -> SourceRange {
    let range = &diagnostic["range"];
    SourceRange {
        start_line: lsp_ts_position_u32(&range["start"]["line"]).saturating_add(1),
        start_column: lsp_ts_position_u32(&range["start"]["character"]).saturating_add(1),
        end_line: lsp_ts_position_u32(&range["end"]["line"]).saturating_add(1),
        end_column: lsp_ts_position_u32(&range["end"]["character"]).saturating_add(1),
    }
}

fn observation_from_lsp_ts_diagnostic(
    provider_version: &str,
    _request_seq_or_zero: u64,
    relative_path: &str,
    index: usize,
    diagnostic: &Value,
) -> ObservationV1 {
    let code_raw = lsp_ts_code_string(diagnostic);
    ObservationV1 {
        observation_id: format!("{PROVIDER_ID}:publish:{relative_path}:{index}"),
        provider_id: PROVIDER_ID.to_string(),
        provider_version: provider_version.to_string(),
        code: normalize_ts_code(&code_raw),
        path: relative_path.to_string(),
        range: lsp_ts_range_to_source_range(diagnostic),
        message: diagnostic["message"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
        semantic_anchor: None,
        source_class: SourceClass::NativeLanguageService,
        cost_class: CostClass::Interactive,
        severity_hint: lsp_ts_severity_hint(diagnostic),
    }
}

/// tsserver category → severity hint: exactly the `error` category blocks;
/// warnings, suggestions, and messages stay advisory policy input.
fn category_to_severity_hint(category: &str) -> SeverityHint {
    if category == "error" {
        SeverityHint::Blocking
    } else {
        SeverityHint::Advisory
    }
}

/// Numeric tsserver codes normalize to their conventional `TS<code>` spelling;
/// other spellings pass through unchanged.
fn normalize_ts_code(code: &str) -> String {
    if !code.is_empty() && code.chars().all(|character| character.is_ascii_digit()) {
        format!("TS{code}")
    } else {
        code.to_string()
    }
}

fn clamp_u32(value: u64) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

/// tsserver spans are 1-based line/offset pairs already; copy through with
/// saturation into the half-open 1-based [`SourceRange`] shape.
fn span_to_source_range(span: &TsserverSpan) -> SourceRange {
    SourceRange {
        start_line: clamp_u32(span.start_line),
        start_column: clamp_u32(span.start_offset),
        end_line: clamp_u32(span.end_line),
        end_column: clamp_u32(span.end_offset),
    }
}

/// Script kind hint so `.ts`/`.tsx`/`.js`/`.jsx` files open with the right
/// parser; unknown extensions default to TypeScript.
fn script_kind_for(path: &str) -> &'static str {
    let extension = Path::new(path)
        .extension()
        .map(|extension| extension.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    match extension.as_str() {
        "tsx" => "TSX",
        "jsx" => "JSX",
        "js" | "mjs" | "cjs" => "JS",
        _ => "TS",
    }
}

fn observation_from_span(
    provider_version: &str,
    request_seq: u64,
    kind: &str,
    relative_path: &str,
    index: usize,
    span: &TsserverSpan,
) -> ObservationV1 {
    ObservationV1 {
        observation_id: format!("{PROVIDER_ID}:{request_seq}:{kind}:{relative_path}:{index}"),
        provider_id: PROVIDER_ID.to_string(),
        provider_version: provider_version.to_string(),
        code: normalize_ts_code(&span.code),
        path: relative_path.to_string(),
        range: span_to_source_range(span),
        message: span.message.clone(),
        semantic_anchor: None,
        source_class: SourceClass::NativeLanguageService,
        cost_class: CostClass::Interactive,
        severity_hint: category_to_severity_hint(&span.category),
    }
}

/// Typed lane omissions: bounded-queue drops and unreadable epoch files are
/// recorded instead of silently narrowing coverage (design §5.3, §12).
fn lane_omissions(sync_omissions: &[TypedOmission], overflow_delta: usize) -> Vec<TypedOmission> {
    let mut omissions = sync_omissions.to_vec();
    if overflow_delta > 0 {
        omissions.push(TypedOmission {
            code: "event_queue_overflow".to_string(),
            detail: format!(
                "{overflow_delta} tsserver events dropped because the bounded queue was full"
            ),
        });
    }
    omissions
}

fn resolve_under_root(root: &Path, raw: &str) -> (PathBuf, String) {
    let candidate = Path::new(raw);
    let absolute = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        root.join(candidate)
    };
    let relative = absolute
        .strip_prefix(root)
        .unwrap_or(candidate)
        .to_string_lossy()
        .replace('\\', "/");
    (absolute, relative)
}

// ---------------------------------------------------------------------------
// Provider
// ---------------------------------------------------------------------------

fn hash_matches(bytes: &[u8], declared: &str) -> bool {
    let hex_digest = hex::encode(Sha256::digest(bytes));
    let declared_hex = declared.strip_prefix("sha256:").unwrap_or(declared);
    hex_digest == declared_hex
}

fn current_hash_mismatches(project_root: &Path, epoch: &WorkspaceEpochV1) -> Vec<TypedOmission> {
    if epoch.changed_file_hashes.is_empty() {
        return Vec::new();
    }
    let mut mismatches = Vec::new();
    for entry in &epoch.changed_file_hashes {
        let (absolute, _) = resolve_under_root(project_root, &entry.path);
        match std::fs::read(&absolute) {
            Ok(bytes) => {
                if !hash_matches(&bytes, &entry.hash) {
                    let hex_digest = hex::encode(Sha256::digest(&bytes));
                    mismatches.push(TypedOmission {
                        code: "hash_mismatch".to_string(),
                        detail: format!(
                            "{}: on-disk sha256:{hex_digest} does not match declared {}",
                            entry.path, entry.hash
                        ),
                    });
                }
            }
            Err(error) => mismatches.push(TypedOmission {
                code: "source_unreadable".to_string(),
                detail: format!("{}: {error}", entry.path),
            }),
        }
    }
    mismatches
}

struct SyncTarget {
    absolute: String,
    uri: String,
    relative: String,
    content: String,
}

/// Collect the epoch's changed files under `project_root`, reading exact bytes
/// for full-content synchronization. Unreadable files become typed omissions
/// rather than aborting the whole epoch. When `changed_file_hashes` is
/// populated, hashes are verified immediately; mismatches become `hash_mismatch`
/// omissions and prevent exact convergence.
fn collect_sync_targets(
    project_root: &Path,
    epoch: &WorkspaceEpochV1,
) -> (BTreeMap<String, SyncTarget>, Vec<TypedOmission>) {
    let mut targets = BTreeMap::new();
    let mut omissions = Vec::new();
    let declared_by_path: BTreeMap<String, String> = epoch
        .changed_file_hashes
        .iter()
        .map(|entry| (entry.path.clone(), entry.hash.clone()))
        .collect();
    let paths: Vec<&str> = if epoch.changed_file_hashes.is_empty() {
        epoch.changed_paths.iter().map(String::as_str).collect()
    } else {
        epoch
            .changed_file_hashes
            .iter()
            .map(|entry| entry.path.as_str())
            .collect()
    };
    for raw_path in paths {
        let (absolute, relative) = resolve_under_root(project_root, raw_path);
        let absolute_str = absolute.to_string_lossy().into_owned();
        match std::fs::read(&absolute) {
            Ok(bytes) => {
                if let Some(declared) = declared_by_path.get(raw_path) {
                    if !hash_matches(&bytes, declared) {
                        let hex_digest = hex::encode(Sha256::digest(&bytes));
                        omissions.push(TypedOmission {
                            code: "hash_mismatch".to_string(),
                            detail: format!(
                                "{raw_path}: on-disk sha256:{hex_digest} does not match declared {declared}"
                            ),
                        });
                    }
                }
                targets.insert(
                    absolute_str.clone(),
                    SyncTarget {
                        absolute: absolute_str,
                        uri: uri_from_path(&absolute),
                        relative,
                        content: String::from_utf8_lossy(&bytes).into_owned(),
                    },
                );
            }
            Err(error) => omissions.push(TypedOmission {
                code: "source_unreadable".to_string(),
                detail: format!("{raw_path}: {error}"),
            }),
        }
    }
    (targets, omissions)
}

/// TypeScript D1 provider implementing the design §3 lifecycle contract.
///
/// Binary absence is typed degradation: `initialize` returns
/// [`ProviderError::Unavailable`] and never installs anything.
pub struct TypeScriptProvider {
    project_root: PathBuf,
    search_path: Vec<PathBuf>,
    session: Option<TsSession>,
    engine_name: String,
    engine_protocol: EngineProtocol,
    declared_version: String,
    next_seq: u64,
    next_id: i64,
    synced_epoch: Option<u64>,
    sync_targets: BTreeMap<String, SyncTarget>,
    uri_relative: BTreeMap<String, String>,
    sync_omissions: Vec<TypedOmission>,
    active_geterr: Option<u64>,
    active_lsp_ids: HashSet<i64>,
    cancelled_lsp_ids: HashSet<i64>,
    cancelled_seqs: HashSet<u64>,
    last_completed: Option<(u64, u64)>,
    lsp_published_versions: BTreeMap<String, Option<i64>>,
    lsp_ack_received: bool,
}

impl TypeScriptProvider {
    /// Build an unstarted provider probing the parent `PATH`; call
    /// `initialize` to actually start the engine.
    pub fn new(project_root: impl Into<PathBuf>) -> Self {
        Self::with_search_path(project_root, default_search_path())
    }

    /// Build an unstarted provider probing an explicit allowlisted search path.
    pub fn with_search_path(project_root: impl Into<PathBuf>, search_path: Vec<PathBuf>) -> Self {
        Self {
            project_root: project_root.into(),
            search_path,
            session: None,
            engine_name: String::new(),
            engine_protocol: EngineProtocol::Tsserver,
            declared_version: ADAPTER_VERSION.to_string(),
            next_seq: 1,
            next_id: 1,
            synced_epoch: None,
            sync_targets: BTreeMap::new(),
            uri_relative: BTreeMap::new(),
            sync_omissions: Vec::new(),
            active_geterr: None,
            active_lsp_ids: HashSet::new(),
            cancelled_lsp_ids: HashSet::new(),
            cancelled_seqs: HashSet::new(),
            last_completed: None,
            lsp_published_versions: BTreeMap::new(),
            lsp_ack_received: false,
        }
    }
}

impl DiagnosticsProvider for TypeScriptProvider {
    /// Probe TypeScript engines on the injected search path, preferring
    /// `tsc --lsp --stdio` (LSP) then `tsgo`/`typescript-go` with LSP args,
    /// falling back to `tsserver` tsserver protocol. Bare `tsgo` with no args
    /// is never launched (design §6, TS7). Missing binaries degrade typed to
    /// `Err(ProviderError::Unavailable)`, no auto-install (design §13).
    fn initialize(&mut self, capabilities: &ProviderCapabilities) -> Result<(), ProviderError> {
        if self.session.is_some() {
            return Err(ProviderError::InvalidRequest(
                "typescript provider initialized twice".into(),
            ));
        }
        self.declared_version = if capabilities.version.is_empty() {
            ADAPTER_VERSION.to_string()
        } else {
            capabilities.version.clone()
        };
        let mut found: Option<(CandidateSpec, PathBuf)> = None;
        for spec in CANDIDATE_SPECS {
            if let Some(binary) = probe_search_path(spec.binary_name, &self.search_path) {
                found = Some((
                    CandidateSpec {
                        binary_name: spec.binary_name,
                        args: spec.args,
                        protocol: spec.protocol,
                    },
                    binary,
                ));
                break;
            }
        }
        let Some((spec, binary)) = found else {
            return Err(ProviderError::Unavailable(
                "neither tsc (lsp) nor tsgo/typescript-go (lsp) nor tsserver was found on the injected search path;                  automatic installation is disabled"
                    .into(),
            ));
        };
        let args: Vec<String> = spec.args.iter().map(|s| s.to_string()).collect();
        let mut session = start_session(
            &binary,
            &args,
            &self.project_root,
            &self.search_path,
            spec.protocol,
        )?;
        if spec.protocol == EngineProtocol::Lsp {
            let request_id = self.next_id;
            self.next_id += 1;
            let envelope = json!({
                "jsonrpc": "2.0",
                "id": request_id,
                "method": "initialize",
                "params": {
                    "processId": Value::Null,
                    "rootUri": uri_from_path(&self.project_root),
                    "capabilities": {},
                    "initializationOptions": {}
                }
            });
            session.write_json(&envelope)?;
            await_lsp_initialize_response(&session, request_id)?;
            session.write_json(&json!({
                "jsonrpc": "2.0",
                "method": "initialized",
                "params": {}
            }))?;
        }
        self.engine_name = spec.binary_name.to_string();
        self.engine_protocol = spec.protocol;
        self.session = Some(session);
        Ok(())
    }

    /// Full-content resynchronization on every epoch change: close all
    /// previously opened files, then open the new epoch's changed files with
    /// their exact sealed bytes. For LSP mode uses `textDocument/didClose` and
    /// `textDocument/didOpen` with `version = epoch`; for tsserver uses
    /// `close`/`open`. Document versions are therefore pinned to the workspace
    /// epoch number.
    fn synchronize(&mut self, epoch: &WorkspaceEpochV1) -> Result<(), ProviderError> {
        let session = self.session.as_mut().ok_or_else(|| {
            ProviderError::InvalidRequest("synchronize called before initialize".into())
        })?;
        let (targets, omissions) = collect_sync_targets(&self.project_root, epoch);

        match session.protocol {
            EngineProtocol::Tsserver => {
                for previous in self.sync_targets.keys() {
                    session.write_json(&json!({
                        "seq": self.next_seq,
                        "type": "request",
                        "command": "close",
                        "arguments": { "file": previous },
                    }))?;
                    self.next_seq += 1;
                }
                for target in targets.values() {
                    session.write_json(&json!({
                        "seq": self.next_seq,
                        "type": "request",
                        "command": "open",
                        "arguments": {
                            "file": target.absolute,
                            "fileContent": target.content,
                            "scriptKindName": script_kind_for(&target.relative),
                        },
                    }))?;
                    self.next_seq += 1;
                }
            }
            EngineProtocol::Lsp => {
                let version = i64::try_from(epoch.epoch).unwrap_or(i64::MAX);
                let stale_uris: Vec<String> =
                    self.sync_targets.values().map(|t| t.uri.clone()).collect();
                for uri in &stale_uris {
                    session.write_json(&json!({
                        "jsonrpc": "2.0",
                        "method": "textDocument/didClose",
                        "params": { "textDocument": { "uri": uri } }
                    }))?;
                }
                for target in targets.values() {
                    session.write_json(&json!({
                        "jsonrpc": "2.0",
                        "method": "textDocument/didOpen",
                        "params": {
                            "textDocument": {
                                "uri": target.uri,
                                "languageId": "typescript",
                                "version": version,
                                "text": target.content,
                            }
                        }
                    }))?;
                }
            }
        }

        self.synced_epoch = Some(epoch.epoch);
        self.uri_relative = targets
            .values()
            .map(|t| (t.uri.clone(), t.relative.clone()))
            .collect();
        self.sync_targets = targets;
        self.sync_omissions = omissions;
        self.active_geterr = None;
        self.active_lsp_ids.clear();
        self.cancelled_lsp_ids.clear();
        self.cancelled_seqs.clear();
        self.last_completed = None;
        self.lsp_published_versions.clear();
        self.lsp_ack_received = false;
        Ok(())
    }

    /// Acquire for both protocols: tsserver pumps `geterr` until
    /// `requestCompleted` for that exact seq; LSP sends a post-open `hover`
    /// ack and collects `publishDiagnostics` until every synchronized file has
    /// published at the exact epoch version, or deadline expires. Hash
    /// mismatches before claiming exact downgrade the lane.
    fn acquire(
        &mut self,
        epoch: &WorkspaceEpochV1,
        deadline: AbsoluteDeadline,
    ) -> Result<ProviderOutput, ProviderError> {
        if self.session.is_none() {
            return Err(ProviderError::InvalidRequest(
                "acquire called before initialize".into(),
            ));
        }
        if self.synced_epoch != Some(epoch.epoch) {
            self.synchronize(epoch)?;
        }
        // Hash verification gates exactness for both protocols.
        let pre_hash_mismatches = current_hash_mismatches(&self.project_root, epoch);
        let has_pre_mismatch = !pre_hash_mismatches.is_empty()
            || self
                .sync_omissions
                .iter()
                .any(|o| o.code == "hash_mismatch");
        if self.sync_targets.is_empty() {
            // Even empty scope must not claim exact if hashes mismatched.
            let exact = !has_pre_mismatch;
            self.last_completed = if exact { Some((epoch.epoch, 0)) } else { None };
            self.lsp_ack_received = exact;
            return Ok(self.build_output(epoch, Vec::new(), 0));
        }

        match self
            .session
            .as_mut()
            .expect("session checked above")
            .protocol
        {
            EngineProtocol::Tsserver => {
                let request_seq = self.next_seq;
                self.next_seq += 1;
                let overflow_before;
                {
                    let session = self.session.as_mut().unwrap();
                    overflow_before = session.overflow_dropped.load(Ordering::Relaxed);
                    let files: Vec<String> = self.sync_targets.keys().cloned().collect();
                    session.write_json(&json!({
                        "seq": request_seq,
                        "type": "request",
                        "command": "geterr",
                        "arguments": { "delay": 0, "files": files },
                    }))?;
                }
                self.active_geterr = Some(request_seq);

                let mut observations = Vec::new();
                loop {
                    let frame = {
                        let session = self.session.as_mut().unwrap();
                        recv_with_deadline(&session.frames, deadline)
                    };
                    match frame {
                        FrameOutcome::DeadlineExceeded => {
                            self.cancelled_seqs.clear();
                            self.cancelled_seqs.insert(request_seq);
                            self.active_geterr = None;
                            return Err(ProviderError::DeadlineExceeded);
                        }
                        FrameOutcome::QueueClosed => {
                            self.active_geterr = None;
                            return Err(ProviderError::Crashed(
                                "tsserver closed its output before completing geterr".into(),
                            ));
                        }
                        FrameOutcome::Frame(line) => {
                            if self.cancelled_seqs.contains(&request_seq) {
                                continue;
                            }
                            match parse_server_message(&line) {
                                ServerEvent::RequestCompleted(completed)
                                    if completed == request_seq =>
                                {
                                    break;
                                }
                                ServerEvent::Diagnostics { kind, file, spans } => {
                                    let Some(target) = self.sync_targets.get(&file) else {
                                        continue;
                                    };
                                    for (index, span) in spans.iter().enumerate() {
                                        observations.push(observation_from_span(
                                            &self.declared_version,
                                            request_seq,
                                            &kind,
                                            &target.relative,
                                            index,
                                            span,
                                        ));
                                    }
                                }
                                ServerEvent::RequestCompleted(_) | ServerEvent::Ignored => {}
                            }
                        }
                    }
                }
                self.active_geterr = None;
                // Only claim exact completion if hashes still match at barrier time.
                let fresh_mismatches = current_hash_mismatches(&self.project_root, epoch);
                if !fresh_mismatches.is_empty() || has_pre_mismatch {
                    self.last_completed = None;
                } else {
                    self.last_completed = Some((epoch.epoch, request_seq));
                }
                let overflow_after = self
                    .session
                    .as_ref()
                    .unwrap()
                    .overflow_dropped
                    .load(Ordering::Relaxed);
                Ok(self.build_output(
                    epoch,
                    observations,
                    overflow_after.saturating_sub(overflow_before),
                ))
            }
            EngineProtocol::Lsp => {
                let request_id = self.next_id;
                self.next_id += 1;
                let ack_uri = self
                    .sync_targets
                    .values()
                    .next()
                    .map(|t| t.uri.clone())
                    .unwrap_or_else(|| uri_from_path(&self.project_root));
                let expected_version = i64::try_from(epoch.epoch).unwrap_or(i64::MAX);
                let target_uris: Vec<String> =
                    self.sync_targets.values().map(|t| t.uri.clone()).collect();
                let overflow_before;
                {
                    let session = self.session.as_mut().unwrap();
                    overflow_before = session.overflow_dropped.load(Ordering::Relaxed);
                    session.write_json(&json!({
                        "jsonrpc": "2.0",
                        "id": request_id,
                        "method": "textDocument/hover",
                        "params": {
                            "textDocument": { "uri": ack_uri },
                            "position": { "line": 0, "character": 0 }
                        }
                    }))?;
                }
                self.active_lsp_ids.insert(request_id);
                let mut published: BTreeMap<String, (Option<i64>, Vec<ObservationV1>)> =
                    BTreeMap::new();
                let mut ack_received = false;
                loop {
                    let frame = {
                        let session = self.session.as_mut().unwrap();
                        recv_with_deadline(&session.frames, deadline)
                    };
                    match frame {
                        FrameOutcome::DeadlineExceeded => {
                            self.cancelled_lsp_ids.insert(request_id);
                            self.active_lsp_ids.remove(&request_id);
                            if let Some(s) = self.session.as_mut() {
                                let _ = s.write_json(&json!({
                                    "jsonrpc": "2.0",
                                    "method": "$/cancelRequest",
                                    "params": { "id": request_id }
                                }));
                            }
                            return Err(ProviderError::DeadlineExceeded);
                        }
                        FrameOutcome::QueueClosed => {
                            self.active_lsp_ids.clear();
                            return Err(ProviderError::Crashed(
                                "ts lsp closed its output before publishing diagnostics".into(),
                            ));
                        }
                        FrameOutcome::Frame(text) => match parse_lsp_message(&text) {
                            LspInboundMessage::Response(id) if id == request_id => {
                                if !self.cancelled_lsp_ids.contains(&id) {
                                    ack_received = true;
                                    self.active_lsp_ids.remove(&id);
                                }
                            }
                            LspInboundMessage::Notification { method, params }
                                if method == "textDocument/publishDiagnostics" =>
                            {
                                let Some(uri) = params["uri"].as_str().map(str::to_string) else {
                                    continue;
                                };
                                if !self.uri_relative.contains_key(&uri) {
                                    continue;
                                }
                                let relative =
                                    self.uri_relative.get(&uri).cloned().unwrap_or_default();
                                let mut observations = Vec::new();
                                if let Some(diagnostics) = params["diagnostics"].as_array() {
                                    for (index, diagnostic) in diagnostics.iter().enumerate() {
                                        observations.push(observation_from_lsp_ts_diagnostic(
                                            &self.declared_version,
                                            request_id as u64,
                                            &relative,
                                            index,
                                            diagnostic,
                                        ));
                                    }
                                }
                                let version = params.get("version").and_then(Value::as_i64);
                                published.insert(uri, (version, observations));
                            }
                            _ => {}
                        },
                    }
                    let every_published = target_uris.iter().all(|uri| published.contains_key(uri));
                    if ack_received && every_published {
                        break;
                    }
                }
                self.active_lsp_ids.clear();
                let overflow_after = self
                    .session
                    .as_ref()
                    .unwrap()
                    .overflow_dropped
                    .load(Ordering::Relaxed);
                let overflow_delta = overflow_after.saturating_sub(overflow_before);
                let hash_mismatches = current_hash_mismatches(&self.project_root, epoch);
                let has_hash_mismatch = !hash_mismatches.is_empty() || has_pre_mismatch;
                let all_versioned = published
                    .values()
                    .all(|(v, _)| *v == Some(expected_version));
                let exact = !has_hash_mismatch && ack_received && all_versioned;
                self.lsp_ack_received = ack_received;
                self.lsp_published_versions = published
                    .iter()
                    .map(|(uri, (v, _))| (uri.clone(), *v))
                    .collect();
                if exact {
                    self.last_completed = Some((epoch.epoch, request_id as u64));
                } else {
                    self.last_completed = None;
                }
                // Flatten observations for output
                let observations: Vec<ObservationV1> =
                    published.into_values().flat_map(|(_, obs)| obs).collect();
                Ok(self.build_output(epoch, observations, overflow_delta))
            }
        }
    }

    /// Map supervisor cancellation onto the active protocol.
    fn cancel(&mut self, request_id: &RequestId) {
        match self.session.as_mut().map(|s| s.protocol) {
            Some(EngineProtocol::Lsp) => {
                if let Some(session) = self.session.as_mut() {
                    let outstanding: Vec<i64> = self.active_lsp_ids.drain().collect();
                    for lsp_id in outstanding {
                        let _ = session.write_json(&json!({
                            "jsonrpc": "2.0",
                            "method": "$/cancelRequest",
                            "params": { "id": lsp_id }
                        }));
                        self.cancelled_lsp_ids.insert(lsp_id);
                    }
                }
            }
            _ => {
                let matches_active = self.active_geterr == Some(request_id.0);
                if matches_active {
                    self.cancelled_seqs.clear();
                    self.cancelled_seqs.insert(request_id.0);
                    self.active_geterr = None;
                }
            }
        }
    }

    /// Converged exactly when the completion barrier for the current epoch's
    /// document versions has been observed and on-disk hashes still match.
    fn prove_convergence(&mut self, epoch: &WorkspaceEpochV1) -> ConvergenceProof {
        let hash_mismatches = current_hash_mismatches(&self.project_root, epoch);
        let has_hash_mismatch = !hash_mismatches.is_empty()
            || self
                .sync_omissions
                .iter()
                .any(|o| o.code == "hash_mismatch");
        if has_hash_mismatch {
            return ConvergenceProof {
                converged: false,
                detail: format!(
                    "hash_mismatch for epoch {}: on-disk bytes do not match declared changed_file_hashes",
                    epoch.epoch
                ),
            };
        }
        let converged = self.last_completed.is_some_and(|(barrier_epoch, _)| {
            barrier_epoch == epoch.epoch && self.synced_epoch == Some(epoch.epoch)
        });
        let detail = match self.last_completed {
            Some((barrier_epoch, seq)) if barrier_epoch == epoch.epoch => match self.engine_protocol {
                EngineProtocol::Lsp => format!(
                    "pull_exact: lsp ack {seq} and versioned publishDiagnostics for epoch {} over {} file(s)",
                    epoch.epoch,
                    self.sync_targets.len()
                ),
                EngineProtocol::Tsserver => format!(
                    "push_versioned_exact: geterr request {seq} completed for epoch {} over {} synchronized file(s)",
                    epoch.epoch,
                    self.sync_targets.len()
                ),
            },
            Some((barrier_epoch, seq)) => format!(
                "stale barrier: request {seq} completed for epoch {barrier_epoch}, not {}",
                epoch.epoch
            ),
            None => format!(
                "no completion barrier observed yet for epoch {} ({})",
                epoch.epoch, self.engine_name
            ),
        };
        ConvergenceProof { converged, detail }
    }

    /// Best-effort shutdown: LSP `shutdown`+`exit` or tsserver `exit`, then
    /// process-group kill and reader joins.
    fn shutdown(mut self: Box<Self>) -> Result<(), ProviderError> {
        if let Some(mut session) = self.session.take() {
            match session.protocol {
                EngineProtocol::Lsp => {
                    let request_id = self.next_id;
                    self.next_id += 1;
                    let _ = session.write_json(&json!({
                        "jsonrpc": "2.0",
                        "id": request_id,
                        "method": "shutdown",
                        "params": Value::Null
                    }));
                    drain_frames_until(&session.frames, SHUTDOWN_HANDSHAKE_TIMEOUT_MS);
                    let _ = session.write_json(&json!({
                        "jsonrpc": "2.0",
                        "method": "exit",
                        "params": Value::Null
                    }));
                }
                EngineProtocol::Tsserver => {
                    let exit = json!({
                        "seq": self.next_seq,
                        "type": "request",
                        "command": "exit",
                    });
                    let _ = session.write_json(&exit);
                }
            }
            session.kill_and_join();
        }
        Ok(())
    }
}

impl TypeScriptProvider {
    fn build_output(
        &self,
        epoch: &WorkspaceEpochV1,
        observations: Vec<ObservationV1>,
        overflow_delta: usize,
    ) -> ProviderOutput {
        let mut omissions = lane_omissions(&self.sync_omissions, overflow_delta);
        // Re-verify current disk hashes at output time (TOCTOU); merge fresh mismatches
        for mismatch in current_hash_mismatches(&self.project_root, epoch) {
            if !omissions.iter().any(|o| o.detail == mismatch.detail) {
                omissions.push(mismatch);
            }
        }
        let has_hash_mismatch = omissions.iter().any(|o| o.code == "hash_mismatch");
        let state = if omissions.is_empty() {
            LaneState::Complete
        } else {
            LaneState::Partial
        };
        let convergence_class = if has_hash_mismatch {
            ConvergenceClass::PushUnversionedAdvisory
        } else {
            // Preserve exact class; for LSP mode PullExact also qualifies as exact,
            // but we keep PushVersionedExact for tsserver and either is fine for
            // LSP – both are in the exact trio. Use protocol-appropriate exact.
            match self.engine_protocol {
                EngineProtocol::Lsp => ConvergenceClass::PullExact,
                EngineProtocol::Tsserver => ConvergenceClass::PushVersionedExact,
            }
        };
        ProviderOutput {
            observations,
            lane: CoverageLaneV1 {
                provider_id: PROVIDER_ID.to_string(),
                scope: self
                    .sync_targets
                    .values()
                    .map(|target| target.relative.clone())
                    .collect(),
                capabilities_covered: vec![
                    CapabilityVocabulary::Syntax,
                    CapabilityVocabulary::ImportExportBinding,
                    CapabilityVocabulary::NameResolution,
                    CapabilityVocabulary::TypeSemantics,
                ],
                convergence_class,
                bound_workspace_epoch: epoch.epoch,
                state,
                omissions,
            },
        }
    }
}

impl Drop for TypeScriptProvider {
    fn drop(&mut self) {
        if let Some(mut session) = self.session.take() {
            session.kill_and_join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_category_maps_blocking_and_everything_else_advisory() {
        assert_eq!(category_to_severity_hint("error"), SeverityHint::Blocking);
        assert_eq!(category_to_severity_hint("warning"), SeverityHint::Advisory);
        assert_eq!(
            category_to_severity_hint("suggestion"),
            SeverityHint::Advisory
        );
        assert_eq!(category_to_severity_hint("message"), SeverityHint::Advisory);
        assert_eq!(category_to_severity_hint(""), SeverityHint::Advisory);
    }

    #[test]
    fn numeric_codes_gain_ts_prefix_and_other_spellings_pass_through() {
        assert_eq!(normalize_ts_code("2322"), "TS2322");
        assert_eq!(normalize_ts_code("TS2551"), "TS2551");
        assert_eq!(normalize_ts_code("xyz"), "xyz");
        assert_eq!(normalize_ts_code(""), "");
    }

    #[test]
    fn tsserver_spans_copy_through_as_one_based_and_saturate() {
        let span = TsserverSpan {
            start_line: 3,
            start_offset: 8,
            end_line: 3,
            end_offset: 14,
            message: "m".into(),
            code: "2322".into(),
            category: "error".into(),
        };
        assert_eq!(
            span_to_source_range(&span),
            SourceRange {
                start_line: 3,
                start_column: 8,
                end_line: 3,
                end_column: 14,
            }
        );
        let huge = TsserverSpan {
            start_line: u64::MAX,
            start_offset: 0,
            end_line: 1,
            end_offset: u64::MAX,
            message: String::new(),
            code: String::new(),
            category: String::new(),
        };
        let saturated = span_to_source_range(&huge);
        assert_eq!(saturated.start_line, u32::MAX);
        assert_eq!(saturated.end_column, u32::MAX);
        assert_eq!(saturated.start_column, 0);
    }

    #[test]
    fn script_kind_follows_extension_case_insensitively() {
        assert_eq!(script_kind_for("/repo/src/a.ts"), "TS");
        assert_eq!(script_kind_for("/repo/src/b.TSX"), "TSX");
        assert_eq!(script_kind_for("/repo/src/c.js"), "JS");
        assert_eq!(script_kind_for("/repo/src/d.mjs"), "JS");
        assert_eq!(script_kind_for("/repo/src/e.jsx"), "JSX");
        assert_eq!(script_kind_for("/repo/src/f.unknown"), "TS");
    }

    #[test]
    fn semantic_diag_event_parses_into_routed_diagnostics() {
        let message = json!({
            "seq": 9,
            "type": "event",
            "event": "semanticDiag",
            "body": {
                "file": "/repo/src/main.ts",
                "diagnostics": [
                    {
                        "start": { "line": 3, "offset": 8 },
                        "end": { "line": 3, "offset": 14 },
                        "message": "Type 'number' is not assignable to type 'string'.",
                        "code": 2322,
                        "category": "error"
                    },
                    {
                        "start": { "line": 7, "offset": 1 },
                        "end": { "line": 7, "offset": 9 },
                        "message": "consider renaming",
                        "code": "TS2551",
                        "category": "suggestion"
                    }
                ]
            }
        })
        .to_string();
        match parse_server_message(&message) {
            ServerEvent::Diagnostics { kind, file, spans } => {
                assert_eq!(kind, "semanticDiag");
                assert_eq!(file, "/repo/src/main.ts");
                assert_eq!(spans.len(), 2);
                let observation =
                    observation_from_span(ADAPTER_VERSION, 4, &kind, "src/main.ts", 0, &spans[0]);
                assert_eq!(observation.code, "TS2322");
                assert_eq!(observation.severity_hint, SeverityHint::Blocking);
                assert_eq!(observation.range.start_line, 3);
                assert_eq!(observation.range.start_column, 8);
                assert_eq!(observation.source_class, SourceClass::NativeLanguageService);
                assert_eq!(observation.cost_class, CostClass::Interactive);
                let advisory =
                    observation_from_span(ADAPTER_VERSION, 4, &kind, "src/main.ts", 1, &spans[1]);
                assert_eq!(advisory.severity_hint, SeverityHint::Advisory);
                assert_eq!(
                    advisory.observation_id,
                    format!("{PROVIDER_ID}:4:semanticDiag:src/main.ts:1")
                );
            }
            other => panic!("expected diagnostics event, got {other:?}"),
        }
    }

    #[test]
    fn request_completed_parses_the_convergence_barrier() {
        let message = json!({
            "seq": 10,
            "type": "event",
            "event": "requestCompleted",
            "body": { "request_seq": 7 }
        })
        .to_string();
        assert_eq!(
            parse_server_message(&message),
            ServerEvent::RequestCompleted(7)
        );
        let noise = json!({"seq": 11, "type": "response", "command": "configure"}).to_string();
        assert_eq!(parse_server_message(&noise), ServerEvent::Ignored);
        let garbage = "not json at all";
        assert_eq!(parse_server_message(garbage), ServerEvent::Ignored);
    }

    #[test]
    fn lane_omissions_surface_overflow_and_sync_degradations_as_partial() {
        assert!(lane_omissions(&[], 0).is_empty());
        let unreadable = vec![TypedOmission {
            code: "source_unreadable".to_string(),
            detail: "missing.ts".to_string(),
        }];
        let omissions = lane_omissions(&unreadable, 3);
        assert_eq!(omissions.len(), 2);
        assert_eq!(omissions[0].code, "source_unreadable");
        assert_eq!(omissions[1].code, "event_queue_overflow");
        assert!(omissions[1].detail.contains('3'));
    }

    #[test]
    fn qualified_capabilities_declare_interactive_pure_analysis_d1() {
        let capabilities = qualified_capabilities();
        assert_eq!(capabilities.provider_id, PROVIDER_ID);
        assert!(capabilities
            .capabilities
            .contains(&CapabilityKind::NativeLanguageService));
        assert_eq!(
            capabilities.side_effect_class,
            SideEffectClass::PureAnalysis
        );
        assert_eq!(capabilities.cost_class, CostClass::Interactive);
        assert_eq!(
            capabilities.convergence_class,
            ConvergenceClass::PushVersionedExact
        );
    }

    #[test]
    fn resolve_under_root_normalizes_relative_paths_to_forward_slashes() {
        let root = Path::new("/repo");
        let (absolute, relative) = resolve_under_root(root, "src/main.ts");
        assert_eq!(absolute, PathBuf::from("/repo/src/main.ts"));
        assert_eq!(relative, "src/main.ts");
        let (absolute_nested, _) =
            resolve_under_root(Path::new("/repo/nested"), "/repo/nested/a.ts");
        assert_eq!(absolute_nested, PathBuf::from("/repo/nested/a.ts"));
    }
}
