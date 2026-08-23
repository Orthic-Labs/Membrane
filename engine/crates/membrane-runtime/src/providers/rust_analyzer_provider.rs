//! Rust-analyzer D1 adapter over LSP 3.17 stdio (design §6 ladder, §13
//! containment).
//!
//! Strictly native diagnostics by configuration *and* enforcement: the
//! `initialize` handshake sets `initializationOptions` so cargo build scripts
//! and all-targets runs are disabled and the flycheck command is `null`
//! (`checkOnSave` off as well), because rust-analyzer native diagnostics are
//! D1 while its flycheck backed by `cargo check` is V1 and must not run
//! routinely (design §6 cost discipline). As defense in depth, any incoming
//! `publishDiagnostics` entry attributed to flycheck/cargo/rustc — explicit
//! source markers, rustc `E####` codes, or pass-through rendered payloads —
//! is dropped with a typed omission recorded on the lane.
//!
//! Synchronization is full-content: every epoch change closes previously
//! opened documents and re-opens the epoch's changed files with their exact
//! sealed bytes stamped `textDocument.version = epoch`. Convergence is
//! [`ConvergenceClass::PullExact`] only when both ordering proofs hold: the
//! server answered a request issued after the didOpen batch (per-document
//! LSP processing order makes any response an ack), and it published
//! diagnostics carrying exactly that version for every synchronized file.
//! Binary absence degrades typed to [`ProviderError::Unavailable`] — probing
//! never installs. All work is synchronous: std threads plus channels
//! internally, never tokio.
//!
//! Side-effect class rationale: declared `PureAnalysis` because the disabled
//! build-script/flycheck configuration means native analysis spawns no cargo
//! build scripts, no compilers, and touches no network.

use crate::live_diagnostics::{
    AbsoluteDeadline, CapabilityKind, ConvergenceProof, DiagnosticsProvider,
    ProviderCapabilities, ProviderError, ProviderOutput, RequestId, SideEffectClass,
};
use crate::providers::child_process::{
    default_search_path, drain_frames_until, kill_direct_child, lsp_frame_bytes,
    probe_search_path, recv_with_deadline, recv_within, sanitized_child_env,
    spawn_bounded_reader, spawn_sanitized, spawn_stderr_drainer, FrameOutcome, LspDecoder,
};
use membrane_protocol::diagnostics::{
    CapabilityVocabulary, ConvergenceClass, CoverageLaneV1, CostClass, LaneState, ObservationV1,
    SeverityHint, SourceClass, SourceRange, TypedOmission, WorkspaceEpochV1,
};
use serde_json::{json, Value};
use sha2::{Digest as _, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Instant;

/// Stable provider identity used for qualification and selection.
pub const PROVIDER_ID: &str = "rust-analyzer-native-d1";
/// Adapter protocol version reported in capabilities and observations.
pub const ADAPTER_VERSION: &str = "adapter-v1";
/// Bound on queued server messages before overflow drops begin.
const EVENT_QUEUE_CAPACITY: usize = 256;
/// Bounded wait for the LSP initialize response (no deadline parameter exists
/// on the lifecycle contract's initialize step).
const INITIALIZE_HANDSHAKE_TIMEOUT_MS: u64 = 15_000;
/// Bounded wait for the LSP shutdown response during teardown.
const SHUTDOWN_HANDSHAKE_TIMEOUT_MS: u64 = 5_000;

/// Declared capabilities of this D1 adapter: one interactive pure-analysis
/// native language service lane with pull convergence.
pub fn qualified_capabilities() -> ProviderCapabilities {
    ProviderCapabilities {
        provider_id: PROVIDER_ID.to_string(),
        version: ADAPTER_VERSION.to_string(),
        capabilities: BTreeSet::from([CapabilityKind::NativeLanguageService]),
        side_effect_class: SideEffectClass::PureAnalysis,
        convergence_class: ConvergenceClass::PullExact,
        cost_class: CostClass::Interactive,
    }
}

// ---------------------------------------------------------------------------
// Session plumbing
// ---------------------------------------------------------------------------

struct LspSession {
    child: Child,
    stdin: Option<ChildStdin>,
    frames: Receiver<String>,
    overflow_dropped: Arc<AtomicUsize>,
    reader_handle: JoinHandle<()>,
    stderr_handle: JoinHandle<()>,
}

impl LspSession {
    fn write_json(&mut self, value: &Value) -> Result<(), ProviderError> {
        let wire = lsp_frame_bytes(value.to_string().as_bytes());
        let Some(stdin) = self.stdin.as_mut() else {
            return Err(ProviderError::Crashed(
                "rust-analyzer stdin already closed".into(),
            ));
        };
        stdin
            .write_all(&wire)
            .and_then(|()| stdin.flush())
            .map_err(|error| {
                ProviderError::Crashed(format!("rust-analyzer stdin write failed: {error}"))
            })
    }

    fn kill_and_join(&mut self) {
        let _ = self.stdin.take();
        kill_direct_child(&mut self.child);
        let _ = self.reader_handle.join();
        let _ = self.stderr_handle.join();
    }
}

fn start_session(
    binary: &Path,
    project_root: &Path,
    search_path: &[PathBuf],
) -> Result<LspSession, ProviderError> {
    let env = sanitized_child_env(search_path);
    let mut child = spawn_sanitized(binary, &[], project_root, &env).map_err(|error| {
        ProviderError::Unavailable(format!(
            "failed to spawn {}: {error}",
            binary.display()
        ))
    })?;
    let stdin = child.stdin.take();
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| ProviderError::Crashed("rust-analyzer stdout was not piped".into()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| ProviderError::Crashed("rust-analyzer stderr was not piped".into()))?;
    let overflow_dropped = Arc::new(AtomicUsize::new(0));
    let pump = spawn_bounded_reader(
        stdout,
        LspDecoder::new(),
        EVENT_QUEUE_CAPACITY,
        Arc::clone(&overflow_dropped),
    );
    let stderr_handle = spawn_stderr_drainer(stderr);
    Ok(LspSession {
        child,
        stdin,
        frames: pump.frames,
        overflow_dropped,
        reader_handle: pump.handle,
        stderr_handle,
    })
}

// ---------------------------------------------------------------------------
// Wire message parsing and conversion (pure, unit-testable)
// ---------------------------------------------------------------------------

/// Percent-encode an absolute filesystem path into a `file://` URI per RFC
/// 3986 unreserved/reserved rules. Encoding is byte-wise over the whole
/// scheme-prefixed form; `/` separators survive.
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

/// One routed inbound LSP message relevant to acquisition.
#[derive(Debug, PartialEq)]
enum InboundMessage {
    /// A response to one of our requests (result or error either acks).
    Response(i64),
    /// A server-initiated notification.
    Notification { method: String, params: Value },
    Ignored,
}

fn parse_lsp_message(text: &str) -> InboundMessage {
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return InboundMessage::Ignored;
    };
    if let Some(method) = value["method"].as_str() {
        return InboundMessage::Notification {
            method: method.to_string(),
            params: value.get("params").cloned().unwrap_or(Value::Null),
        };
    }
    if value["id"].as_i64().is_some()
        && (value.get("result").is_some() || value.get("error").is_some())
    {
        return InboundMessage::Response(value["id"].as_i64().unwrap_or_default());
    }
    InboundMessage::Ignored
}

const FLYCHECK_SOURCES: [&str; 3] = ["cargo", "rustc", "flycheck"];

fn is_flycheck_source(value: &Value) -> bool {
    value
        .as_str()
        .map(|source| FLYCHECK_SOURCES.contains(&source))
        .unwrap_or(false)
}

/// Rustc error codes look like `E0432`: capital E plus at least four ASCII
/// digits. Native rust-analyzer diagnostic codes are names such as
/// `unresolved-import`, never E-codes.
fn is_rustc_error_code(code: &str) -> bool {
    let bytes = code.as_bytes();
    bytes.len() >= 5 && bytes[0] == b'E' && bytes[1..].iter().all(u8::is_ascii_digit)
}

/// Defense-in-depth flycheck attribution test applied to every incoming
/// diagnostic even though flycheck is disabled at initialization (design §6).
fn is_flycheck_attributed(diagnostic: &Value) -> bool {
    is_flycheck_source(&diagnostic["source"])
        || is_flycheck_source(&diagnostic["data"]["source"])
        || diagnostic["data"]["rendered"].is_string()
        || diagnostic["code"]
            .as_str()
            .map(is_rustc_error_code)
            .unwrap_or(false)
}

/// LSP DiagnosticSeverity 1 (Error) blocks; warnings/info/hints stay advisory.
fn lsp_severity_hint(diagnostic: &Value) -> SeverityHint {
    if diagnostic["severity"].as_i64() == Some(1) {
        SeverityHint::Blocking
    } else {
        SeverityHint::Advisory
    }
}

fn lsp_position_u32(value: &Value) -> u32 {
    value
        .as_u64()
        .map(|position| u32::try_from(position).unwrap_or(u32::MAX))
        .unwrap_or(0)
}

/// Convert an LSP 0-based line/character range into the half-open 1-based
/// [`SourceRange`] shape with saturation at `u32::MAX`.
fn lsp_range_to_source_range(diagnostic: &Value) -> SourceRange {
    let range = &diagnostic["range"];
    SourceRange {
        start_line: lsp_position_u32(&range["start"]["line"]).saturating_add(1),
        start_column: lsp_position_u32(&range["start"]["character"]).saturating_add(1),
        end_line: lsp_position_u32(&range["end"]["line"]).saturating_add(1),
        end_column: lsp_position_u32(&range["end"]["character"]).saturating_add(1),
    }
}

fn lsp_code_string(diagnostic: &Value) -> String {
    match diagnostic.get("code") {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Number(number)) => number.to_string(),
        _ => String::new(),
    }
}

fn observation_from_lsp_diagnostic(
    provider_version: &str,
    relative_path: &str,
    index: usize,
    diagnostic: &Value,
) -> ObservationV1 {
    ObservationV1 {
        observation_id: format!("{PROVIDER_ID}:publish:{relative_path}:{index}"),
        provider_id: PROVIDER_ID.to_string(),
        provider_version: provider_version.to_string(),
        code: lsp_code_string(diagnostic),
        path: relative_path.to_string(),
        range: lsp_range_to_source_range(diagnostic),
        message: diagnostic["message"].as_str().unwrap_or_default().to_string(),
        semantic_anchor: None,
        source_class: SourceClass::NativeLanguageService,
        cost_class: CostClass::Interactive,
        severity_hint: lsp_severity_hint(diagnostic),
    }
}

struct SyncTarget {
    uri: String,
    relative: String,
    content: String,
}

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
        let candidate = Path::new(&entry.path);
        let absolute = if candidate.is_absolute() {
            candidate.to_path_buf()
        } else {
            project_root.join(candidate)
        };
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

fn collect_sync_targets(
    project_root: &Path,
    epoch: &WorkspaceEpochV1,
) -> (Vec<SyncTarget>, Vec<TypedOmission>) {
    let mut ordered: BTreeMap<String, SyncTarget> = BTreeMap::new();
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
        let candidate = Path::new(raw_path);
        let absolute = if candidate.is_absolute() {
            candidate.to_path_buf()
        } else {
            project_root.join(candidate)
        };
        let relative = absolute
            .strip_prefix(project_root)
            .unwrap_or(candidate)
            .to_string_lossy()
            .replace('\\', "/");
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
                ordered.insert(
                    relative.clone(),
                    SyncTarget {
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
    (ordered.into_values().collect(), omissions)
}

// ---------------------------------------------------------------------------
// Provider
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct PublishRecord {
    version: Option<i64>,
    observations: Vec<ObservationV1>,
}

#[derive(Debug, Clone)]
struct ConvergenceRecord {
    epoch: u64,
    exact: bool,
    detail: String,
}

/// Rust-analyzer D1 provider implementing the design §3 lifecycle contract.
pub struct RustAnalyzerProvider {
    project_root: PathBuf,
    search_path: Vec<PathBuf>,
    session: Option<LspSession>,
    declared_version: String,
    next_id: i64,
    synced_epoch: Option<u64>,
    sync_targets: Vec<SyncTarget>,
    uri_relative: BTreeMap<String, String>,
    sync_omissions: Vec<TypedOmission>,
    active_lsp_ids: HashSet<i64>,
    cancelled_lsp_ids: HashSet<i64>,
    convergence: Option<ConvergenceRecord>,
}

impl RustAnalyzerProvider {
    /// Build an unstarted provider probing the parent `PATH`; call
    /// `initialize` to start the engine and complete the LSP handshake.
    pub fn new(project_root: impl Into<PathBuf>) -> Self {
        Self::with_search_path(project_root, default_search_path())
    }

    /// Build an unstarted provider probing an explicit allowlisted search path.
    pub fn with_search_path(project_root: impl Into<PathBuf>, search_path: Vec<PathBuf>) -> Self {
        Self {
            project_root: project_root.into(),
            search_path,
            session: None,
            declared_version: ADAPTER_VERSION.to_string(),
            next_id: 1,
            synced_epoch: None,
            sync_targets: Vec::new(),
            uri_relative: BTreeMap::new(),
            sync_omissions: Vec::new(),
            active_lsp_ids: HashSet::new(),
            cancelled_lsp_ids: HashSet::new(),
            convergence: None,
        }
    }

    /// Native-only analysis options: cargo build scripts off, all targets
    /// off, proc macros off, flycheck command null, check-on-save off (design §6).
    /// PureAnalysis requires proc macros disabled in addition to build scripts
    /// and flycheck (otherwise proc-macro crates execute arbitrary code).
    fn native_only_initialization_options() -> Value {
        json!({
            "cargo": {
                "allTargets": false,
                "buildScripts": { "enable": false },
            },
            "procMacro": { "enable": false },
            "flycheck": Value::Null,
            "checkOnSave": false,
        })
    }

    fn await_initialize_response(session: &LspSession, request_id: i64) -> Result<(), ProviderError> {
        let started = Instant::now();
        loop {
            let elapsed_ms = started.elapsed().as_millis() as u64;
            if elapsed_ms >= INITIALIZE_HANDSHAKE_TIMEOUT_MS {
                return Err(ProviderError::Crashed(
                    "rust-analyzer initialize handshake timed out".into(),
                ));
            }
            match recv_within(
                &session.frames,
                INITIALIZE_HANDSHAKE_TIMEOUT_MS - elapsed_ms,
            ) {
                FrameOutcome::Frame(text) => {
                    if let InboundMessage::Response(id) = parse_lsp_message(&text) {
                        if id == request_id {
                            return Ok(());
                        }
                    }
                }
                FrameOutcome::QueueClosed => {
                    return Err(ProviderError::Crashed(
                        "rust-analyzer exited during the initialize handshake".into(),
                    ));
                }
                FrameOutcome::DeadlineExceeded => continue,
            }
        }
    }
}

impl DiagnosticsProvider for RustAnalyzerProvider {
    /// Probe `rust-analyzer` on the injected search path, spawn it with a
    /// sanitized environment, and complete the LSP 3.17 initialize handshake
    /// with native-only options: build scripts off, all targets off, flycheck
    /// command null. Missing binaries degrade typed:
    /// `Err(ProviderError::Unavailable)`, no auto-install (design §13).
    fn initialize(&mut self, capabilities: &ProviderCapabilities) -> Result<(), ProviderError> {
        if self.session.is_some() {
            return Err(ProviderError::InvalidRequest(
                "rust-analyzer provider initialized twice".into(),
            ));
        }
        self.declared_version = if capabilities.version.is_empty() {
            ADAPTER_VERSION.to_string()
        } else {
            capabilities.version.clone()
        };
        let Some(binary) = probe_search_path("rust-analyzer", &self.search_path) else {
            return Err(ProviderError::Unavailable(
                "rust-analyzer was not found on the injected search path; \
                 automatic installation is disabled"
                    .into(),
            ));
        };
        let mut session =
            start_session(&binary, &self.project_root, &self.search_path)?;
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
                "initializationOptions": Self::native_only_initialization_options(),
            }
        });
        session.write_json(&envelope)?;
        Self::await_initialize_response(&session, request_id)?;
        session.write_json(&json!({
            "jsonrpc": "2.0",
            "method": "initialized",
            "params": {}
        }))?;
        self.session = Some(session);
        Ok(())
    }

    /// Full-content resynchronization on every epoch change: didClose all
    /// previously opened documents, then didOpen the new epoch's changed files
    /// with exact sealed bytes and `textDocument.version = epoch`.
    fn synchronize(&mut self, epoch: &WorkspaceEpochV1) -> Result<(), ProviderError> {
        let session = self.session.as_mut().ok_or_else(|| {
            ProviderError::InvalidRequest("synchronize called before initialize".into())
        })?;
        let (targets, omissions) = collect_sync_targets(&self.project_root, epoch);
        let stale_uris: Vec<String> = self.sync_targets.iter().map(|t| t.uri.clone()).collect();
        let version = i64::try_from(epoch.epoch).unwrap_or(i64::MAX);

        for uri in &stale_uris {
            session.write_json(&json!({
                "jsonrpc": "2.0",
                "method": "textDocument/didClose",
                "params": { "textDocument": { "uri": uri } }
            }))?;
        }
        for target in &targets {
            session.write_json(&json!({
                "jsonrpc": "2.0",
                "method": "textDocument/didOpen",
                "params": {
                    "textDocument": {
                        "uri": target.uri,
                        "languageId": "rust",
                        "version": version,
                        "text": target.content,
                    }
                }
            }))?;
        }

        self.synced_epoch = Some(epoch.epoch);
        self.uri_relative = targets
            .iter()
            .map(|target| (target.uri.clone(), target.relative.clone()))
            .collect();
        self.sync_targets = targets;
        self.sync_omissions = omissions;
        self.active_lsp_ids.clear();
        self.cancelled_lsp_ids.clear();
        self.convergence = None;
        Ok(())
    }

    /// Prove ordering with one post-open request (any response acks the whole
    /// preceding notification batch under LSP per-document processing order),
    /// then pump framed messages until every synchronized file has published
    /// diagnostics bound to the exact synced version, or `deadline` expires.
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
        // Hash verification before claiming exact: re-hash current disk bytes
        // against declared epoch hashes; any mismatch prevents exact claim.
        let pre_hash_mismatches = current_hash_mismatches(&self.project_root, epoch);
        if self.sync_targets.is_empty() {
            let has_mismatch = !pre_hash_mismatches.is_empty()
                || self.sync_omissions.iter().any(|o| o.code == "hash_mismatch");
            let exact = !has_mismatch;
            self.convergence = Some(ConvergenceRecord {
                epoch: epoch.epoch,
                exact,
                detail: if has_mismatch {
                    format!(
                        "hash_mismatch for epoch {}: on-disk bytes do not match declared changed_file_hashes",
                        epoch.epoch
                    )
                } else {
                    format!(
                        "pull_exact: empty changed-file scope for epoch {}",
                        epoch.epoch
                    )
                },
            });
            return Ok(self.build_output(epoch, BTreeMap::new(), 0, 0));
        }

        let request_id = self.next_id;
        self.next_id += 1;
        let ack_uri = self.sync_targets[0].uri.clone();
        let expected_version = i64::try_from(epoch.epoch).unwrap_or(i64::MAX);
        let target_uris: Vec<String> =
            self.sync_targets.iter().map(|target| target.uri.clone()).collect();

        let session = self.session.as_mut().expect("session checked above");
        let overflow_before = session.overflow_dropped.load(Ordering::Relaxed);
        session.write_json(&json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "method": "textDocument/hover",
            "params": {
                "textDocument": { "uri": ack_uri },
                "position": { "line": 0, "character": 0 }
            }
        }))?;
        self.active_lsp_ids.insert(request_id);

        let mut published: BTreeMap<String, PublishRecord> = BTreeMap::new();
        let mut ack_received = false;
        let mut flycheck_dropped = 0usize;
        loop {
            // Borrow the queue only for the receive so deadline handling can
            // still reach the session to send `$/cancelRequest`.
            match recv_with_deadline(&session.frames, deadline) {
                FrameOutcome::DeadlineExceeded => {
                    self.cancelled_lsp_ids.insert(request_id);
                    self.active_lsp_ids.remove(&request_id);
                    let _ = session.write_json(&json!({
                        "jsonrpc": "2.0",
                        "method": "$/cancelRequest",
                        "params": { "id": request_id }
                    }));
                    return Err(ProviderError::DeadlineExceeded);
                }
                FrameOutcome::QueueClosed => {
                    self.active_lsp_ids.clear();
                    return Err(ProviderError::Crashed(
                        "rust-analyzer closed its output before publishing diagnostics".into(),
                    ));
                }
                FrameOutcome::Frame(text) => match parse_lsp_message(&text) {
                    InboundMessage::Response(id) if id == request_id => {
                        if !self.cancelled_lsp_ids.contains(&id) {
                            ack_received = true;
                            self.active_lsp_ids.remove(&id);
                        }
                    }
                    InboundMessage::Notification { method, params }
                        if method == "publishDiagnostics" =>
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
                                if is_flycheck_attributed(diagnostic) {
                                    flycheck_dropped += 1;
                                    continue;
                                }
                                observations.push(observation_from_lsp_diagnostic(
                                    &self.declared_version,
                                    &relative,
                                    index,
                                    diagnostic,
                                ));
                            }
                        }
                        published.insert(
                            uri,
                            PublishRecord {
                                version: params.get("version").and_then(Value::as_i64),
                                observations,
                            },
                        );
                    }
                    _ => {}
                },
            }
            let every_target_published = target_uris.iter().all(|uri| published.contains_key(uri));
            if ack_received && every_target_published {
                break;
            }
        }

        self.active_lsp_ids.clear();
        let overflow_after = session.overflow_dropped.load(Ordering::Relaxed);
        let overflow_delta = overflow_after.saturating_sub(overflow_before);
        let hash_mismatches = current_hash_mismatches(&self.project_root, epoch);
        let has_hash_mismatch = !hash_mismatches.is_empty()
            || self.sync_omissions.iter().any(|o| o.code == "hash_mismatch");
        let all_versioned = published.values().all(|record| record.version == Some(expected_version));
        let exact = !has_hash_mismatch && all_versioned;
        self.convergence = Some(ConvergenceRecord {
            epoch: epoch.epoch,
            exact,
            detail: if has_hash_mismatch {
                format!(
                    "pull_exact: hash_mismatch for epoch {} prevents exact convergence ({} file(s) published, hash_mismatch)",
                    epoch.epoch,
                    published.len()
                )
            } else {
                format!(
                    "pull_exact: didOpen batch acked and publishDiagnostics received for {} file(s) at epoch {}{}",
                    published.len(),
                    epoch.epoch,
                    if exact { "" } else { " (some publishes were unversioned)" }
                )
            },
        });
        Ok(self.build_output(epoch, published, overflow_delta, flycheck_dropped))
    }

    /// Map supervisor cancellation onto LSP `$/cancelRequest` for every
    /// outstanding acquisition request; late responses for cancelled ids are
    /// ignored by the routing loops. The supervisor's `RequestId` itself is
    /// not part of the wire mapping: an instance runs at most one synchronous
    /// acquisition at a time, so cancelling means "cancel this instance's
    /// outstanding requests".
    fn cancel(&mut self, _request_id: &RequestId) {
        let Some(session) = self.session.as_mut() else {
            return;
        };
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

    /// Converged exactly when the current epoch's didOpen batch was acked,
    /// version-exact publishes arrived for every synchronized file, and
    /// current on-disk hashes still match the declared epoch hashes.
    fn prove_convergence(&mut self, epoch: &WorkspaceEpochV1) -> ConvergenceProof {
        // Hash verification gates exact convergence: current disk bytes must
        // still match declared changed_file_hashes.
        let hash_mismatches = current_hash_mismatches(&self.project_root, epoch);
        let has_hash_mismatch = !hash_mismatches.is_empty()
            || self.sync_omissions.iter().any(|o| o.code == "hash_mismatch");
        if has_hash_mismatch {
            return ConvergenceProof {
                converged: false,
                detail: format!(
                    "hash_mismatch for epoch {}: on-disk bytes do not match declared changed_file_hashes",
                    epoch.epoch
                ),
            };
        }
        let record = self
            .convergence
            .as_ref()
            .filter(|record| record.epoch == epoch.epoch && self.synced_epoch == Some(epoch.epoch));
        let converged = record.is_some_and(|record| record.exact);
        let detail = match record {
            Some(record) if record.exact => record.detail.clone(),
            Some(record) => format!("not exact for epoch {}: {}", epoch.epoch, record.detail),
            None => format!(
                "no pull barrier observed yet for epoch {} (didOpen ack plus versioned publishes required)",
                epoch.epoch
            ),
        };
        ConvergenceProof { converged, detail }
    }

    /// Best-effort LSP shutdown request plus exit notification, then direct-
    /// child kill and reader joins.
    fn shutdown(mut self) -> Result<(), ProviderError> {
        if let Some(mut session) = self.session.take() {
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
            session.kill_and_join();
        }
        Ok(())
    }
}

impl RustAnalyzerProvider {
    fn build_output(
        &self,
        epoch: &WorkspaceEpochV1,
        published: BTreeMap<String, PublishRecord>,
        overflow_delta: usize,
        flycheck_dropped: usize,
    ) -> ProviderOutput {
        let mut omissions = self.sync_omissions.clone();
        // Re-verify current disk hashes at output time (TOCTOU): if disk
        // changed after synchronize, we must not claim exact.
        for mismatch in current_hash_mismatches(&self.project_root, epoch) {
            if !omissions.iter().any(|o| o.detail == mismatch.detail) {
                omissions.push(mismatch);
            }
        }
        for (uri, record) in &published {
            if record.version.is_none() {
                let detail = self
                    .uri_relative
                    .get(uri)
                    .cloned()
                    .unwrap_or_else(|| uri.clone());
                omissions.push(TypedOmission {
                    code: "publish_missing_version".to_string(),
                    detail: format!("{detail}: publishDiagnostics carried no version"),
                });
            }
        }
        if flycheck_dropped > 0 {
            omissions.push(TypedOmission {
                code: "flycheck_diagnostic_dropped".to_string(),
                detail: format!(
                    "{flycheck_dropped} flycheck (cargo check)-attributed diagnostics dropped; \
                     native analysis lane only (design §6)"
                ),
            });
        }
        if overflow_delta > 0 {
            omissions.push(TypedOmission {
                code: "event_queue_overflow".to_string(),
                detail: format!(
                    "{overflow_delta} server messages dropped because the bounded queue was full"
                ),
            });
        }
        let has_hash_mismatch = omissions.iter().any(|o| o.code == "hash_mismatch");
        let state = if !omissions.is_empty() {
            LaneState::Partial
        } else {
            LaneState::Complete
        };
        let convergence_class = if has_hash_mismatch {
            ConvergenceClass::PushUnversionedAdvisory
        } else {
            ConvergenceClass::PullExact
        };
        let observations = published
            .into_values()
            .flat_map(|record| record.observations)
            .collect();
        ProviderOutput {
            observations,
            lane: CoverageLaneV1 {
                provider_id: PROVIDER_ID.to_string(),
                scope: self.sync_targets.iter().map(|t| t.relative.clone()).collect(),
                capabilities_covered: vec![
                    CapabilityVocabulary::Syntax,
                    CapabilityVocabulary::RepositoryModuleResolution,
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

impl Drop for RustAnalyzerProvider {
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
    fn uri_encoding_percent_escapes_spaces_and_keeps_reserved_separators() {
        assert_eq!(
            uri_from_path(Path::new("/repo/my work/src.rs")),
            "file:///repo/my%20work/src.rs"
        );
        assert_eq!(
            uri_from_path(Path::new("/plain/path/x.rs")),
            "file:///plain/path/x.rs"
        );
        assert_eq!(
            uri_from_path(Path::new("C:\\repo\\x.rs")),
            "file:///C:%5Crepo%5Cx.rs"
        );
    }

    #[test]
    fn lsp_ranges_convert_zero_based_to_one_based_with_saturation() {
        let diagnostic = json!({
            "range": {
                "start": { "line": 2, "character": 7 },
                "end": { "line": 2, "character": 19 }
            }
        });
        assert_eq!(
            lsp_range_to_source_range(&diagnostic),
            SourceRange {
                start_line: 3,
                start_column: 8,
                end_line: 3,
                end_column: 20,
            }
        );
        let huge = json!({
            "range": {
                "start": { "line": 4294967295u64, "character": 0 },
                "end": { "line": 0, "character": 4294967295u64 }
            }
        });
        let saturated = lsp_range_to_source_range(&huge);
        assert_eq!(saturated.start_line, u32::MAX);
        assert_eq!(saturated.end_column, u32::MAX);
        let missing = json!({ "range": {} });
        let defaulted = lsp_range_to_source_range(&missing);
        assert_eq!(defaulted.start_line, 1);
        assert_eq!(defaulted.start_column, 1);
    }

    #[test]
    fn severity_one_blocks_everything_else_stays_advisory() {
        assert_eq!(lsp_severity_hint(&json!({ "severity": 1 })), SeverityHint::Blocking);
        assert_eq!(lsp_severity_hint(&json!({ "severity": 2 })), SeverityHint::Advisory);
        assert_eq!(lsp_severity_hint(&json!({ "severity": 3 })), SeverityHint::Advisory);
        assert_eq!(lsp_severity_hint(&json!({ "severity": 4 })), SeverityHint::Advisory);
        assert_eq!(lsp_severity_hint(&json!({})), SeverityHint::Advisory);
    }

    #[test]
    fn flycheck_attribution_is_detected_by_source_marker() {
        assert!(is_flycheck_attributed(&json!({
            "source": "cargo",
            "code": "E0432",
            "message": "unresolved import"
        })));
        assert!(is_flycheck_attributed(&json!({
            "source": "rustc",
            "message": "mismatched types"
        })));
        assert!(is_flycheck_attributed(&json!({
            "source": "flycheck",
            "message": "linking failed"
        })));
        assert!(is_flycheck_attributed(&json!({
            "data": { "source": "rustc" },
            "message": "nested attribution"
        })));
    }

    #[test]
    fn flycheck_attribution_is_detected_by_rustc_e_codes_and_rendered_payloads() {
        assert!(is_flycheck_attributed(&json!({
            "code": "E0432",
            "message": "unresolved import `crate::missing`"
        })));
        assert!(is_flycheck_attributed(&json!({
            "code": "E0308",
            "message": "mismatched types"
        })));
        assert!(is_flycheck_attributed(&json!({
            "message": "external",
            "data": { "rendered": "error[E0432]: unresolved import" }
        })));
    }

    #[test]
    fn native_diagnostics_are_never_flycheck_attributed() {
        assert!(!is_flycheck_attributed(&json!({
            "code": "unresolved-import",
            "message": "`missing` in module tree"
        })));
        assert!(!is_flycheck_attributed(&json!({
            "code": "inactive-code",
            "message": "code is inactive due to #[cfg]"
        })));
        assert!(!is_flycheck_attributed(&json!({})));
        assert!(!is_flycheck_attributed(&json!({ "code": "E043" })));
        assert!(!is_flycheck_attributed(&json!({ "code": "e0432" })));
        assert!(!is_flycheck_attributed(&json!({
            "data": { "rendered": 42 }
        })));
    }

    #[test]
    fn lsp_messages_route_responses_notifications_and_noise() {
        let response = json!({"jsonrpc":"2.0","id":7,"result":null}).to_string();
        assert_eq!(parse_lsp_message(&response), InboundMessage::Response(7));
        let error_response =
            json!({"jsonrpc":"2.0","id":9,"error":{"code":-32601,"message":"nope"}}).to_string();
        assert_eq!(parse_lsp_message(&error_response), InboundMessage::Response(9));
        let notification = json!({
            "jsonrpc":"2.0",
            "method":"textDocument/publishDiagnostics",
            "params":{ "uri":"file:///x.rs", "version": 12, "diagnostics": [] }
        })
        .to_string();
        match parse_lsp_message(&notification) {
            InboundMessage::Notification { method, params } => {
                assert_eq!(method, "textDocument/publishDiagnostics");
                assert_eq!(params["version"].as_i64(), Some(12));
            }
            other => panic!("expected notification, got {other:?}"),
        }
        assert_eq!(
            parse_lsp_message("garbage bytes"),
            InboundMessage::Ignored
        );
    }

    #[test]
    fn publish_without_version_is_recorded_as_an_unversioned_omission_input() {
        let with_version = json!({ "uri": "file:///a.rs", "version": 5, "diagnostics": [] });
        assert_eq!(with_version["version"].as_i64(), Some(5));
        let without_version = json!({ "uri": "file:///a.rs", "diagnostics": [] });
        assert_eq!(without_version.get("version").and_then(Value::as_i64), None);
    }

    #[test]
    fn qualified_capabilities_declare_interactive_pure_analysis_pull_exact() {
        let capabilities = qualified_capabilities();
        assert_eq!(capabilities.provider_id, PROVIDER_ID);
        assert!(capabilities
            .capabilities
            .contains(&CapabilityKind::NativeLanguageService));
        assert_eq!(capabilities.side_effect_class, SideEffectClass::PureAnalysis);
        assert_eq!(capabilities.cost_class, CostClass::Interactive);
        assert_eq!(capabilities.convergence_class, ConvergenceClass::PullExact);
    }

    #[test]
    fn initialization_options_disable_cargo_and_flycheck() {
        let options = RustAnalyzerProvider::native_only_initialization_options();
        assert_eq!(options["cargo"]["buildScripts"]["enable"], json!(false));
        assert_eq!(options["cargo"]["allTargets"], json!(false));
        assert_eq!(options["procMacro"]["enable"], json!(false));
        assert_eq!(options["flycheck"], Value::Null);
        assert_eq!(options["checkOnSave"], json!(false));
    }
}
