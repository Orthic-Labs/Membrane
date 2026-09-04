#![cfg_attr(windows, windows_subsystem = "windows")]

//! Tray-owned, headless Membrane runtime entrypoint.
//!
//! stdin is both control channel & parent-lifetime signal. stdout carries only
//! typed NDJSON events; diagnostics stay on stderr & never include secrets.

use cortex_core::{ProvenanceRef, SessionEvent};
use cortex_store::{AbsorbedStore, MemDb, SessionEvent as StoreSessionEvent};
use membrane_protocol::background_review::{
    BackgroundReviewActivitySignalV1, BackgroundReviewForegroundMemoryStateV1,
    BackgroundReviewJobKindV1, BackgroundReviewReasonV1,
};
use membrane_protocol::{
    decode_command_frame, decode_launch_frame, encode_frame, DaemonCommandKind, DaemonEventKind,
    DaemonEventV1, DaemonProtocolError, DAEMON_IPC_MAX_FRAME_BYTES, DAEMON_IPC_SCHEMA_VERSION,
};
use membrane_runtime::background_review::{
    execute_background_semantic_review, AuthenticatedLoopbackSemanticReviewProvider,
    BackgroundReviewCompletion, BackgroundReviewCursorStore, BackgroundReviewProducer,
    BackgroundReviewScheduler, BackgroundSemanticReviewInputV1,
    JsonlBackgroundReviewObservationSink, JsonlBackgroundReviewProposalAdmission,
};
use membrane_runtime::service::{run_hub_runtime, LifecycleControl};
use std::{
    fs,
    io::{self, BufRead, BufReader, Read, Write},
    net::{Ipv4Addr, TcpStream},
    path::PathBuf,
    sync::mpsc,
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const CONTROL_POLL: Duration = Duration::from_millis(25);
const HEALTH_TIMEOUT: Duration = Duration::from_secs(2);
const BACKGROUND_REVIEW_OBSERVATION_PREFIX: &str = "membrane-background-review ";

#[derive(Debug)]
enum ControlSignal {
    Drain,
    ParentClosed,
    Invalid,
}

const BACKGROUND_REVIEW_INPUT_ENV: &str = "MEMBRANE_BACKGROUND_REVIEW_INPUT";
const DEFAULT_BACKGROUND_REVIEW_INPUT: &str = ".membrane/background-review-input.json";
const CORTEX_DB_RELATIVE_PATH: &str = "tools/.cache/memory/cortex-engine.db";
const BACKGROUND_REVIEW_TICK: Duration = Duration::from_millis(250);

/// Host activity/foreground snapshot consumed by daemon scheduler. It is
/// separate from lifecycle stdin, preserving tray-daemon control ownership.
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BackgroundReviewInputSnapshot {
    schema_version: u32,
    activity: BackgroundReviewActivitySignalV1,
    job_kind: BackgroundReviewJobKindV1,
    foreground_memory_state: BackgroundReviewForegroundMemoryStateV1,
    #[serde(default)]
    task_id: Option<String>,
    deadline_unix_ms: u64,
}

struct DaemonBackgroundExecutor {
    provider: Option<AuthenticatedLoopbackSemanticReviewProvider>,
    cursor_store: BackgroundReviewCursorStore,
    proposal_sink: JsonlBackgroundReviewProposalAdmission,
    last_missing_provider_at: Option<u64>,
}

impl DaemonBackgroundExecutor {
    fn new(root: &PathBuf, bearer_token: &str) -> Self {
        let provider =
            AuthenticatedLoopbackSemanticReviewProvider::from_environment(bearer_token).ok();
        Self {
            provider,
            cursor_store: BackgroundReviewCursorStore::default(),
            proposal_sink: JsonlBackgroundReviewProposalAdmission::from_workspace_root(root),
            last_missing_provider_at: None,
        }
    }

    fn input_path(root: &PathBuf) -> PathBuf {
        if root.file_name().is_some_and(|name| name == "state")
            && root
                .parent()
                .and_then(std::path::Path::file_name)
                .is_some_and(|name| name == "Membrane")
        {
            return root.join(DEFAULT_BACKGROUND_REVIEW_INPUT);
        }
        std::env::var_os(BACKGROUND_REVIEW_INPUT_ENV)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| root.join(DEFAULT_BACKGROUND_REVIEW_INPUT))
    }

    fn tick(&mut self, root: &PathBuf, scheduler: &BackgroundReviewScheduler, now: u64) {
        let Some(provider) = self.provider.as_ref() else {
            if self
                .last_missing_provider_at
                .map_or(true, |last| now.saturating_sub(last) >= 5_000)
            {
                scheduler.observe_deferred(BackgroundReviewReasonV1::SemanticProviderNotWired, now);
                self.last_missing_provider_at = Some(now);
            }
            return;
        };
        let input_path = Self::input_path(root);
        let snapshot = match fs::read_to_string(&input_path)
            .ok()
            .and_then(|value| serde_json::from_str::<BackgroundReviewInputSnapshot>(&value).ok())
        {
            Some(snapshot) => snapshot,
            None => {
                scheduler.observe_deferred(BackgroundReviewReasonV1::CursorInputUnavailable, now);
                return;
            }
        };
        if snapshot.schema_version != 1
            || snapshot.activity.validate().is_err()
            || snapshot.foreground_memory_state.validate().is_err()
        {
            scheduler.observe_deferred(BackgroundReviewReasonV1::InvalidJob, now);
            return;
        }
        let production = BackgroundReviewProducer::new(scheduler).admit(
            &snapshot.activity,
            snapshot.job_kind,
            now,
        );
        let membrane_runtime::background_review::BackgroundReviewProduction::Started {
            job, ..
        } = production
        else {
            return;
        };
        let input = match load_background_semantic_input(root, &self.cursor_store, &snapshot) {
            Ok(input) => input,
            Err(reason) => {
                let _ = scheduler.finish_with_completion(
                    &job.job_id,
                    BackgroundReviewCompletion::FailedWithReason(reason),
                    now,
                );
                return;
            }
        };
        let _execution = execute_background_semantic_review(
            scheduler,
            &job,
            &input,
            provider,
            Some(&self.proposal_sink),
            &self.cursor_store,
            snapshot.deadline_unix_ms,
            now,
        );
    }
}

fn load_background_semantic_input(
    root: &PathBuf,
    cursor_store: &BackgroundReviewCursorStore,
    snapshot: &BackgroundReviewInputSnapshot,
) -> Result<BackgroundSemanticReviewInputV1, BackgroundReviewReasonV1> {
    let session_id = &snapshot.activity.session_id;
    let cursor = cursor_store.get(session_id)?;
    let database_path = root.join(CORTEX_DB_RELATIVE_PATH);
    if !database_path.is_file() {
        return Err(BackgroundReviewReasonV1::CursorInputUnavailable);
    }
    let db = MemDb::open(&database_path)
        .map_err(|_| BackgroundReviewReasonV1::CursorInputUnavailable)?;
    let store =
        AbsorbedStore::new(db).map_err(|_| BackgroundReviewReasonV1::CursorInputUnavailable)?;
    let high_water = store
        .cursor(session_id)
        .map_err(|_| BackgroundReviewReasonV1::CursorInputUnavailable)?;
    let end_seq = high_water
        .last_seq
        .min(cursor.last_seq.saturating_add(256))
        .saturating_add(1);
    let stored_events = if end_seq > cursor.last_seq.saturating_add(1) {
        store
            .events_range(session_id, cursor.last_seq.saturating_add(1), end_seq)
            .map_err(|_| BackgroundReviewReasonV1::CursorInputUnavailable)?
    } else {
        Vec::new()
    };
    // The baseline is read-only context from before the background cursor.
    // Bound it so selection remains deterministic and cannot turn a review
    // tick into an unbounded historical read.
    let baseline_start = cursor.last_seq.saturating_sub(256).saturating_add(1);
    let reviewed_baseline = if cursor.last_seq >= baseline_start {
        store
            .events_range(session_id, baseline_start, cursor.last_seq.saturating_add(1))
            .map_err(|_| BackgroundReviewReasonV1::CursorInputUnavailable)?
    } else {
        Vec::new()
    };
    let events = stored_events
        .into_iter()
        .map(store_event_to_core)
        .collect::<Vec<_>>();
    let reviewed_baseline = reviewed_baseline
        .into_iter()
        .map(store_event_to_core)
        .collect::<Vec<_>>();
    Ok(BackgroundSemanticReviewInputV1 {
        task_id: snapshot.task_id.clone(),
        cursor,
        events,
        reviewed_baseline,
        foreground_memory_state: match &snapshot.foreground_memory_state {
            BackgroundReviewForegroundMemoryStateV1::Unavailable => {
                cortex_core::review::ForegroundMemoryStateV1::Unavailable
            }
            BackgroundReviewForegroundMemoryStateV1::AvailableNoEmission => {
                cortex_core::review::ForegroundMemoryStateV1::AvailableNoEmission
            }
            BackgroundReviewForegroundMemoryStateV1::AvailableEmission { range } => {
                cortex_core::review::ForegroundMemoryStateV1::AvailableEmission(
                    cortex_core::review::ForegroundMemoryEmissionV1 {
                        emission_id: format!(
                            "background-signal-{}-{}",
                            range.start_seq, range.end_seq
                        ),
                        session_id: session_id.clone(),
                        start_seq: range.start_seq,
                        end_seq: range.end_seq,
                    },
                )
            }
        },
    })
}

fn store_event_to_core(event: StoreSessionEvent) -> SessionEvent {
    SessionEvent {
        schema_version: event.schema_version,
        session_id: event.session_id,
        seq: event.seq,
        event_id: event.event_id,
        event_type: event.event_type,
        payload: event.payload,
        scope_id: event.scope_id,
        authority: event.authority,
        influence_class: event.influence_class,
        lifecycle: event.lifecycle,
        retention: event.retention,
        provenance: event
            .provenance
            .into_iter()
            .map(|item| ProvenanceRef {
                source: item.source,
                source_event_ids: item.source_event_ids,
                producer: item.producer,
            })
            .collect(),
        occurred_at_ms: event.occurred_at_ms,
        recorded_at_ms: event.recorded_at_ms,
        content_hash: event.content_hash,
    }
}

fn main() {
    if let Err(reason) = run() {
        eprintln!("membrane-daemon: {reason}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), &'static str> {
    let mut reader = BufReader::new(io::stdin());
    let launch_frame = read_frame(&mut reader)
        .map_err(|_| "daemon_protocol_invalid")?
        .ok_or("daemon_protocol_invalid")?;
    let launch = decode_launch_frame(&launch_frame).map_err(|_| "daemon_protocol_invalid")?;

    let control = LifecycleControl::from_lifecycle_capability(&launch.bearer_token)
        .map_err(|_| "daemon_protocol_invalid")?;
    let runtime_control = control.clone();
    let runtime_failure_control = control.clone();
    let root = PathBuf::from(&launch.workspace_root);
    let background_review = BackgroundReviewScheduler::from_workspace_root(&root, now_unix_ms());
    let background_observation_sink =
        JsonlBackgroundReviewObservationSink::from_workspace_root(&root);
    let mut background_executor = DaemonBackgroundExecutor::new(&root, &launch.bearer_token);
    emit_background_observations(&background_review, &background_observation_sink);
    let runtime_root = root.clone();
    let runtime = thread::Builder::new()
        .name("membrane-daemon-runtime".into())
        .spawn(move || {
            let result = run_hub_runtime(&runtime_root, runtime_control);
            if let Err(error) = &result {
                runtime_failure_control.fail(stable_runtime_reason(error));
            }
            result
        })
        .map_err(|_| "daemon_spawn_failed")?;

    let (signal_tx, signal_rx) = mpsc::channel();
    let control_for_loop = control.clone();
    let launch_sequence = launch.sequence;
    let _control_thread = thread::Builder::new()
        .name("membrane-daemon-control".into())
        .spawn(move || control_loop(reader, launch_sequence, signal_tx, control_for_loop));
    if _control_thread.is_err() {
        control.fail("daemon_control_spawn_failed");
        let _ = runtime.join();
        return Err("daemon_spawn_failed");
    }

    let pid = std::process::id();
    let mut event_sequence = 0_u64;
    let ready_port = match control.wait_until_ready() {
        Ok(port) if port == launch.http_port && health_answers(port) => port,
        Ok(_) => {
            background_review.set_hub_active(false, now_unix_ms());
            background_review.observe_idle(now_unix_ms());
            emit_background_observations(&background_review, &background_observation_sink);
            control.fail("daemon_ready_failed");
            emit_event(
                &mut event_sequence,
                DaemonEventKind::Fatal,
                pid,
                None,
                Some("daemon_ready_failed"),
            )?;
            let _ = runtime.join();
            return Err("daemon_ready_failed");
        }
        Err(_) => {
            let reason = match signal_rx.try_recv() {
                Ok(ControlSignal::ParentClosed) => "daemon_parent_closed",
                Ok(ControlSignal::Invalid) => "daemon_protocol_invalid",
                _ if control.failure().as_deref() == Some("daemon_protocol_invalid") => {
                    "daemon_protocol_invalid"
                }
                _ if control.command().as_deref() == Some("parent_closed") => {
                    "daemon_parent_closed"
                }
                _ => control
                    .failure()
                    .as_deref()
                    .map(stable_runtime_reason)
                    .unwrap_or("daemon_ready_failed"),
            };
            background_review.set_hub_active(false, now_unix_ms());
            background_review.observe_idle(now_unix_ms());
            emit_background_observations(&background_review, &background_observation_sink);
            control.fail(reason);
            emit_event(
                &mut event_sequence,
                DaemonEventKind::Fatal,
                pid,
                None,
                Some(reason),
            )?;
            let _ = runtime.join();
            return Err(reason);
        }
    };

    emit_event(
        &mut event_sequence,
        DaemonEventKind::Ready,
        pid,
        Some(format!("http://127.0.0.1:{ready_port}")),
        None,
    )?;
    background_review.set_hub_active(true, now_unix_ms());
    background_review.observe_idle(now_unix_ms());
    background_executor.tick(&root, &background_review, now_unix_ms());
    emit_background_observations(&background_review, &background_observation_sink);

    let mut requested_drain = false;
    let mut parent_closed = false;
    let mut last_background_tick = now_unix_ms();
    while !runtime.is_finished() {
        match signal_rx.recv_timeout(CONTROL_POLL) {
            Ok(ControlSignal::Drain) if !requested_drain => {
                requested_drain = true;
                background_review.set_hub_active(false, now_unix_ms());
                emit_background_observations(&background_review, &background_observation_sink);
                control.request_drain(Some("tray_drain"));
                emit_event(
                    &mut event_sequence,
                    DaemonEventKind::Draining,
                    pid,
                    None,
                    Some("daemon_draining"),
                )?;
            }
            Ok(ControlSignal::ParentClosed) => {
                parent_closed = true;
                background_review.set_hub_active(false, now_unix_ms());
                emit_background_observations(&background_review, &background_observation_sink);
                control.request_drain(Some("parent_closed"));
            }
            Ok(ControlSignal::Invalid) => {
                background_review.set_hub_active(false, now_unix_ms());
                emit_background_observations(&background_review, &background_observation_sink);
                control.fail("daemon_protocol_invalid");
                emit_event(
                    &mut event_sequence,
                    DaemonEventKind::Fatal,
                    pid,
                    None,
                    Some("daemon_protocol_invalid"),
                )?;
            }
            Ok(ControlSignal::Drain) | Err(mpsc::RecvTimeoutError::Timeout) => {
                let now = now_unix_ms();
                if now.saturating_sub(last_background_tick)
                    >= BACKGROUND_REVIEW_TICK.as_millis() as u64
                {
                    background_executor.tick(&root, &background_review, now);
                    emit_background_observations(&background_review, &background_observation_sink);
                    last_background_tick = now;
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) if !requested_drain => {
                parent_closed = true;
                background_review.set_hub_active(false, now_unix_ms());
                emit_background_observations(&background_review, &background_observation_sink);
                control.request_drain(Some("parent_closed"));
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {}
        }
    }

    background_review.set_hub_active(false, now_unix_ms());
    background_review.observe_idle(now_unix_ms());
    emit_background_observations(&background_review, &background_observation_sink);
    let result = runtime.join().map_err(|_| "daemon_exited")?;
    if requested_drain && result.is_ok() {
        emit_event(
            &mut event_sequence,
            DaemonEventKind::Drained,
            pid,
            None,
            None,
        )?;
        return Ok(());
    }
    if parent_closed {
        return result.map_err(|_| "daemon_exited");
    }
    let reason = result
        .err()
        .as_deref()
        .map(stable_runtime_reason)
        .unwrap_or("daemon_exited");
    emit_event(
        &mut event_sequence,
        DaemonEventKind::Fatal,
        pid,
        None,
        Some(reason),
    )?;
    Err(reason)
}

fn control_loop<R: BufRead>(
    mut reader: R,
    mut last_sequence: u64,
    sender: mpsc::Sender<ControlSignal>,
    lifecycle: LifecycleControl,
) {
    loop {
        let frame = match read_frame(&mut reader) {
            Ok(Some(frame)) => frame,
            Ok(None) => {
                lifecycle.request_drain(Some("parent_closed"));
                let _ = sender.send(ControlSignal::ParentClosed);
                return;
            }
            Err(_) => {
                lifecycle.fail("daemon_protocol_invalid");
                let _ = sender.send(ControlSignal::Invalid);
                return;
            }
        };
        match decode_command_frame(&frame, Some(last_sequence)) {
            Ok(command) => {
                last_sequence = command.sequence;
                if command.kind == DaemonCommandKind::Drain {
                    lifecycle.request_drain(Some("tray_drain"));
                    let _ = sender.send(ControlSignal::Drain);
                    return;
                }
            }
            Err(_) => {
                lifecycle.fail("daemon_protocol_invalid");
                let _ = sender.send(ControlSignal::Invalid);
                return;
            }
        }
    }
}

fn read_frame<R: BufRead>(reader: &mut R) -> Result<Option<Vec<u8>>, DaemonProtocolError> {
    let mut frame = Vec::new();
    let read = reader
        .take((DAEMON_IPC_MAX_FRAME_BYTES + 1) as u64)
        .read_until(b'\n', &mut frame)
        .map_err(|_| DaemonProtocolError::InvalidLine)?;
    if read == 0 {
        return Ok(None);
    }
    if frame.len() > DAEMON_IPC_MAX_FRAME_BYTES {
        return Err(DaemonProtocolError::FrameTooLarge);
    }
    if frame.last() != Some(&b'\n') {
        return Err(DaemonProtocolError::InvalidLine);
    }
    Ok(Some(frame))
}

fn emit_event(
    sequence: &mut u64,
    kind: DaemonEventKind,
    pid: u32,
    endpoint: Option<String>,
    reason: Option<&str>,
) -> Result<(), &'static str> {
    *sequence = sequence.checked_add(1).ok_or("daemon_protocol_invalid")?;
    let event = DaemonEventV1 {
        schema_version: DAEMON_IPC_SCHEMA_VERSION,
        sequence: *sequence,
        kind,
        pid,
        observed_at_unix_ms: now_unix_ms(),
        endpoint,
        reason: reason.map(str::to_owned),
    };
    let frame = encode_frame(&event).map_err(|_| "daemon_protocol_invalid")?;
    let mut stdout = io::stdout().lock();
    stdout
        .write_all(&frame)
        .map_err(|_| "daemon_event_pipe_closed")?;
    stdout.flush().map_err(|_| "daemon_event_pipe_closed")
}

fn emit_background_observations(
    scheduler: &BackgroundReviewScheduler,
    sink: &JsonlBackgroundReviewObservationSink,
) {
    // A persistent sink failure repeats on every tick. Reporting it each time
    // buried the log in thousands of identical lines that said nothing about
    // the cause; report the reason, and only when it changes.
    static LAST: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
    match scheduler.persist_observations(sink) {
        Ok(_) => {
            if let Ok(mut last) = LAST.lock() {
                *last = None;
            }
        }
        Err(error) => {
            let reason = format!("{error}");
            let repeated = LAST
                .lock()
                .map(|mut last| {
                    let same = last.as_deref() == Some(reason.as_str());
                    if !same {
                        *last = Some(reason.clone());
                    }
                    same
                })
                .unwrap_or(false);
            if !repeated {
                eprintln!(
                    "{BACKGROUND_REVIEW_OBSERVATION_PREFIX}sink_unavailable {} ({})",
                    reason,
                    sink.path().display()
                );
            }
        }
    }
}

#[cfg(test)]
fn background_observation_line(
    observation: &membrane_protocol::BackgroundReviewObservationV1,
) -> Result<String, serde_json::Error> {
    serde_json::to_string(observation)
        .map(|json| format!("{BACKGROUND_REVIEW_OBSERVATION_PREFIX}{json}"))
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn health_answers(port: u16) -> bool {
    let address = std::net::SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let Ok(mut stream) = TcpStream::connect_timeout(&address, HEALTH_TIMEOUT) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(HEALTH_TIMEOUT));
    let _ = stream.set_write_timeout(Some(HEALTH_TIMEOUT));
    if stream
        .write_all(b"GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
        .is_err()
    {
        return false;
    }
    let mut response = [0_u8; 64];
    let Ok(read) = stream.read(&mut response) else {
        return false;
    };
    response[..read].starts_with(b"HTTP/1.1 200") || response[..read].starts_with(b"HTTP/1.0 200")
}

fn stable_runtime_reason(error: &str) -> &'static str {
    if error.contains("runtime.json") || error.contains("runtime identity") {
        "daemon_runtime_config_unavailable"
    } else if error.contains("installation identity") || error.contains("installation manifest") {
        "daemon_identity_unavailable"
    } else if error.contains("bind") || error.contains("address") {
        "daemon_bind_failed"
    } else {
        "daemon_exited"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use membrane_protocol::{decode_event_frame, DaemonCommandV1};
    use membrane_runtime::background_review::{CONFIG_PATH_ENV, DEFAULT_CONFIG_RELATIVE_PATH};
    use std::io::Cursor;

    #[test]
    fn bounded_reader_accepts_one_line_and_rejects_missing_newline() {
        let mut valid = Cursor::new(b"{}\n".to_vec());
        assert_eq!(read_frame(&mut valid).unwrap(), Some(b"{}\n".to_vec()));
        assert!(matches!(
            read_frame(&mut Cursor::new(b"{}".to_vec())),
            Err(DaemonProtocolError::InvalidLine)
        ));
    }

    #[test]
    fn bounded_reader_rejects_oversize_before_json_decode() {
        let mut bytes = vec![b'x'; DAEMON_IPC_MAX_FRAME_BYTES + 1];
        bytes.push(b'\n');
        assert!(matches!(
            read_frame(&mut Cursor::new(bytes)),
            Err(DaemonProtocolError::FrameTooLarge)
        ));
    }

    #[test]
    fn control_loop_accepts_drain_after_launch_sequence() {
        let frame = encode_frame(&DaemonCommandV1 {
            schema_version: DAEMON_IPC_SCHEMA_VERSION,
            sequence: 2,
            kind: DaemonCommandKind::Drain,
        })
        .unwrap();
        let (sender, receiver) = mpsc::channel();
        let lifecycle = LifecycleControl::default();
        control_loop(Cursor::new(frame), 1, sender, lifecycle.clone());
        assert!(matches!(receiver.recv().unwrap(), ControlSignal::Drain));
        assert!(lifecycle.shutdown_requested());
    }

    #[test]
    fn control_loop_treats_eof_or_sequence_regression_as_terminal() {
        let (sender, receiver) = mpsc::channel();
        let lifecycle = LifecycleControl::default();
        control_loop(Cursor::new(Vec::new()), 1, sender, lifecycle.clone());
        assert!(matches!(
            receiver.recv().unwrap(),
            ControlSignal::ParentClosed
        ));
        assert!(lifecycle.shutdown_requested());

        let frame = encode_frame(&DaemonCommandV1 {
            schema_version: DAEMON_IPC_SCHEMA_VERSION,
            sequence: 1,
            kind: DaemonCommandKind::Drain,
        })
        .unwrap();
        let (sender, receiver) = mpsc::channel();
        let lifecycle = LifecycleControl::default();
        control_loop(Cursor::new(frame), 1, sender, lifecycle.clone());
        assert!(matches!(receiver.recv().unwrap(), ControlSignal::Invalid));
        assert!(lifecycle.shutdown_requested());
    }

    #[test]
    fn ready_event_is_bounded_and_decodes_with_monotonic_sequence() {
        let event = DaemonEventV1 {
            schema_version: DAEMON_IPC_SCHEMA_VERSION,
            sequence: 1,
            kind: DaemonEventKind::Ready,
            pid: 42,
            observed_at_unix_ms: 1,
            endpoint: Some("http://127.0.0.1:4317".into()),
            reason: None,
        };
        let frame = encode_frame(&event).unwrap();
        assert!(frame.len() <= DAEMON_IPC_MAX_FRAME_BYTES);
        assert_eq!(decode_event_frame(&frame, None).unwrap(), event);
    }

    #[test]
    fn background_observation_line_is_typed_content_free_json() {
        let observation = membrane_protocol::BackgroundReviewObservationV1 {
            schema_version: membrane_protocol::BACKGROUND_REVIEW_SCHEMA_VERSION,
            job_id: None,
            kind: None,
            status: membrane_protocol::BackgroundReviewStatusV1::Deferred,
            reason: membrane_protocol::BackgroundReviewReasonV1::HubInactive,
            observed_at_unix_ms: 1,
            attempt: 0,
            input_tokens: 0,
            turn_input_tokens: 0,
            aggregate_input_tokens: 0,
            activity_units: 0,
            hub_active: false,
            foreground_active: false,
        };
        let line = background_observation_line(&observation).unwrap();
        let json = line
            .strip_prefix(BACKGROUND_REVIEW_OBSERVATION_PREFIX)
            .expect("typed observation prefix");
        let value: serde_json::Value = serde_json::from_str(json).unwrap();
        assert_eq!(value["schemaVersion"], 1);
        assert_eq!(value["reason"], "hub_inactive");
        assert!(value.get("output").is_none());
    }

    #[test]
    fn background_config_path_has_explicit_workspace_default() {
        let path = BackgroundReviewScheduler::config_path_for_workspace("C:\\workspace");
        assert!(path.ends_with(DEFAULT_CONFIG_RELATIVE_PATH));
        assert_eq!(CONFIG_PATH_ENV, "MEMBRANE_BACKGROUND_REVIEW_CONFIG");
    }
}
