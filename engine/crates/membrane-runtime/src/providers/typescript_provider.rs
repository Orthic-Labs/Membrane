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
    AbsoluteDeadline, CapabilityKind, ConvergenceProof, DiagnosticsProvider,
    ProviderCapabilities, ProviderError, ProviderOutput, RequestId, SideEffectClass,
};
use crate::providers::child_process::{
    default_search_path, kill_direct_child, probe_search_path, recv_with_deadline,
    sanitized_child_env, spawn_bounded_reader, spawn_sanitized, spawn_stderr_drainer,
    tsserver_line_bytes, FrameOutcome, LineFrameDecoder,
};
use membrane_protocol::diagnostics::{
    CapabilityVocabulary, ConvergenceClass, CoverageLaneV1, CostClass, LaneState, ObservationV1,
    SeverityHint, SourceClass, SourceRange, TypedOmission, WorkspaceEpochV1,
};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::thread::JoinHandle;

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

// ---------------------------------------------------------------------------
// Session plumbing
// ---------------------------------------------------------------------------

struct TsserverSession {
    child: Child,
    stdin: Option<ChildStdin>,
    frames: Receiver<String>,
    overflow_dropped: Arc<AtomicUsize>,
    reader_handle: JoinHandle<()>,
    stderr_handle: JoinHandle<()>,
}

impl TsserverSession {
    fn write_json(&mut self, value: &Value) -> Result<(), ProviderError> {
        let wire = tsserver_line_bytes(&value.to_string());
        let Some(stdin) = self.stdin.as_mut() else {
            return Err(ProviderError::Crashed(
                "tsserver stdin already closed".into(),
            ));
        };
        stdin
            .write_all(&wire)
            .and_then(|()| stdin.flush())
            .map_err(|error| ProviderError::Crashed(format!("tsserver stdin write failed: {error}")))
    }

    fn kill_and_join(&mut self) {
        let _ = self.stdin.take();
        kill_direct_child(&mut self.child);
        let _ = self.reader_handle.join();
        let _ = self.stderr_handle.join();
    }
}

fn start_session(binary: &Path, project_root: &Path, search_path: &[PathBuf]) -> Result<TsserverSession, ProviderError> {
    let env = sanitized_child_env(search_path);
    let mut child = spawn_sanitized(binary, &[], project_root, &env).map_err(|error| {
        ProviderError::Unavailable(format!("failed to spawn {}: {error}", binary.display()))
    })?;
    let stdin = child.stdin.take();
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| ProviderError::Crashed("tsserver stdout was not piped".into()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| ProviderError::Crashed("tsserver stderr was not piped".into()))?;
    let overflow_dropped = Arc::new(AtomicUsize::new(0));
    let pump = spawn_bounded_reader(
        stdout,
        LineFrameDecoder::new(),
        EVENT_QUEUE_CAPACITY,
        Arc::clone(&overflow_dropped),
    );
    let stderr_handle = spawn_stderr_drainer(stderr);
    Ok(TsserverSession {
        child,
        stdin,
        frames: pump.frames,
        overflow_dropped,
        reader_handle: pump.handle,
        stderr_handle,
    })
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
        kind: &'static str,
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
                kind,
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
            detail: format!("{overflow_delta} tsserver events dropped because the bounded queue was full"),
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

struct SyncTarget {
    relative: String,
    content: String,
}

/// Collect the epoch's changed files under `project_root`, reading exact bytes
/// for full-content synchronization. Unreadable files become typed omissions
/// rather than aborting the whole epoch.
fn collect_sync_targets(
    project_root: &Path,
    epoch: &WorkspaceEpochV1,
) -> (BTreeMap<String, SyncTarget>, Vec<TypedOmission>) {
    let mut targets = BTreeMap::new();
    let mut omissions = Vec::new();
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
        match std::fs::read(&absolute) {
            Ok(bytes) => {
                targets.insert(
                    absolute.to_string_lossy().into_owned(),
                    SyncTarget {
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
    session: Option<TsserverSession>,
    engine_name: String,
    declared_version: String,
    next_seq: u64,
    synced_epoch: Option<u64>,
    sync_targets: BTreeMap<String, SyncTarget>,
    sync_omissions: Vec<TypedOmission>,
    active_geterr: Option<u64>,
    cancelled_seqs: HashSet<u64>,
    last_completed: Option<(u64, u64)>,
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
            declared_version: ADAPTER_VERSION.to_string(),
            next_seq: 1,
            synced_epoch: None,
            sync_targets: BTreeMap::new(),
            sync_omissions: Vec::new(),
            active_geterr: None,
            cancelled_seqs: HashSet::new(),
            last_completed: None,
        }
    }
}

impl DiagnosticsProvider for TypeScriptProvider {
    /// Probe `tsgo` then `tsserver` on the injected search path and spawn the
    /// first match with a sanitized environment. Missing binaries degrade
    /// typed: `Err(ProviderError::Unavailable)`, no auto-install (design §13).
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
        const CANDIDATES: [&str; 2] = ["tsgo", "tsserver"];
        let mut found = None;
        for name in CANDIDATES {
            if let Some(binary) = probe_search_path(name, &self.search_path) {
                found = Some((name, binary));
                break;
            }
        }
        let Some((engine_name, binary)) = found else {
            return Err(ProviderError::Unavailable(
                "neither tsgo nor tsserver was found on the injected search path; \
                 automatic installation is disabled"
                    .into(),
            ));
        };
        self.engine_name = engine_name.to_string();
        self.session = Some(start_session(
            &binary,
            &self.project_root,
            &self.search_path,
        )?);
        Ok(())
    }

    /// Full-content resynchronization on every epoch change: close all
    /// previously opened files, then open the new epoch's changed files with
    /// their exact sealed bytes. Document versions are therefore pinned to the
    /// workspace epoch number.
    fn synchronize(&mut self, epoch: &WorkspaceEpochV1) -> Result<(), ProviderError> {
        let session = self.session.as_mut().ok_or_else(|| {
            ProviderError::InvalidRequest("synchronize called before initialize".into())
        })?;
        let (targets, omissions) = collect_sync_targets(&self.project_root, epoch);

        for previous in self.sync_targets.keys() {
            session.write_json(&json!({
                "seq": self.next_seq,
                "type": "request",
                "command": "close",
                "arguments": { "file": previous },
            }))?;
            self.next_seq += 1;
        }
        for (absolute, target) in &targets {
            session.write_json(&json!({
                "seq": self.next_seq,
                "type": "request",
                "command": "open",
                "arguments": {
                    "file": absolute,
                    "fileContent": target.content,
                    "scriptKindName": script_kind_for(&target.relative),
                },
            }))?;
            self.next_seq += 1;
        }

        self.synced_epoch = Some(epoch.epoch);
        self.sync_targets = targets;
        self.sync_omissions = omissions;
        self.active_geterr = None;
        self.cancelled_seqs.clear();
        self.last_completed = None;
        Ok(())
    }

    /// Issue one `geterr` over the epoch's synchronized files and pump framed
    /// events until the completion barrier for that exact request sequence
    /// arrives or `deadline` expires. Honoring the deadline here is mandatory:
    /// the supervisor re-enforces it, but the provider reports its own
    /// `DeadlineExceeded` from the reader timeout.
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
        if self.sync_targets.is_empty() {
            self.last_completed = Some((epoch.epoch, 0));
            return Ok(self.build_output(epoch, Vec::new(), 0));
        }

        let request_seq = self.next_seq;
        self.next_seq += 1;
        let session = self.session.as_mut().expect("session checked above");
        let overflow_before = session.overflow_dropped.load(Ordering::Relaxed);
        let files: Vec<&String> = self.sync_targets.keys().collect();
        session.write_json(&json!({
            "seq": request_seq,
            "type": "request",
            "command": "geterr",
            "arguments": { "delay": 0, "files": files },
        }))?;
        self.active_geterr = Some(request_seq);

        let mut observations = Vec::new();
        loop {
            match recv_with_deadline(&session.frames, deadline) {
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
                        ServerEvent::RequestCompleted(completed) if completed == request_seq => {
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
                                    kind,
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
        self.last_completed = Some((epoch.epoch, request_seq));
        let overflow_after = session.overflow_dropped.load(Ordering::Relaxed);
        Ok(self.build_output(epoch, observations, overflow_after.saturating_sub(overflow_before)))
    }

    /// Map supervisor cancellation onto the tsserver protocol.
    ///
    /// Limitation (documented): tsserver has no wire-level cancellation
    /// command. The cancelled request's sequence is remembered so any late
    /// diagnostics or completion events for it are dropped by the routing
    /// loop, and the next full-content synchronize supersedes pending server
    /// work by replacing the documents it refers to.
    fn cancel(&mut self, request_id: &RequestId) {
        let matches_active = self.active_geterr == Some(request_id.0);
        if matches_active {
            self.cancelled_seqs.clear();
            self.cancelled_seqs.insert(request_id.0);
            self.active_geterr = None;
        }
    }

    /// Converged exactly when the `geterr` completion barrier for the current
    /// epoch's document versions has been observed.
    fn prove_convergence(&mut self, epoch: &WorkspaceEpochV1) -> ConvergenceProof {
        let converged = self.last_completed.is_some_and(|(barrier_epoch, _)| {
            barrier_epoch == epoch.epoch && self.synced_epoch == Some(epoch.epoch)
        });
        let detail = match self.last_completed {
            Some((barrier_epoch, seq)) if barrier_epoch == epoch.epoch => format!(
                "push_versioned_exact: geterr request {seq} completed for epoch {} over {} synchronized file(s)",
                epoch.epoch,
                self.sync_targets.len()
            ),
            Some((barrier_epoch, seq)) => format!(
                "stale barrier: geterr request {seq} completed for epoch {barrier_epoch}, not {}",
                epoch.epoch
            ),
            None => format!(
                "no geterr completion barrier observed yet for epoch {} ({})",
                epoch.epoch, self.engine_name
            ),
        };
        ConvergenceProof { converged, detail }
    }

    /// Best-effort `exit` command, then direct-child kill and reader joins.
    fn shutdown(mut self) -> Result<(), ProviderError> {
        if let Some(mut session) = self.session.take() {
            let exit = json!({
                "seq": self.next_seq,
                "type": "request",
                "command": "exit",
            });
            let _ = session.write_json(&exit);
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
        let omissions = lane_omissions(&self.sync_omissions, overflow_delta);
        let state = if omissions.is_empty() {
            LaneState::Complete
        } else {
            LaneState::Partial
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
                convergence_class: ConvergenceClass::PushVersionedExact,
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
        assert_eq!(category_to_severity_hint("suggestion"), SeverityHint::Advisory);
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
                    observation_from_span(ADAPTER_VERSION, 4, kind, "src/main.ts", 0, &spans[0]);
                assert_eq!(observation.code, "TS2322");
                assert_eq!(observation.severity_hint, SeverityHint::Blocking);
                assert_eq!(observation.range.start_line, 3);
                assert_eq!(observation.range.start_column, 8);
                assert_eq!(observation.source_class, SourceClass::NativeLanguageService);
                assert_eq!(observation.cost_class, CostClass::Interactive);
                let advisory =
                    observation_from_span(ADAPTER_VERSION, 4, kind, "src/main.ts", 1, &spans[1]);
                assert_eq!(advisory.severity_hint, SeverityHint::Advisory);
                assert_eq!(advisory.observation_id, format!("{PROVIDER_ID}:4:semanticDiag:src/main.ts:1"));
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
        assert!(capabilities.capabilities.contains(&CapabilityKind::NativeLanguageService));
        assert_eq!(capabilities.side_effect_class, SideEffectClass::PureAnalysis);
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
        let (absolute_nested, _) = resolve_under_root(Path::new("/repo/nested"), "/repo/nested/a.ts");
        assert_eq!(absolute_nested, PathBuf::from("/repo/nested/a.ts"));
    }
}
