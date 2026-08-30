//! Hub-owned Windows transport for Live Diagnostics.
//!
//! This is deliberately a byte-framed named pipe, never TCP, HTTP, MCP, or a
//! second diagnostics runtime. Frames carry the existing diagnostics route
//! method/path/body shape & dispatch to the resident `DiagnosticsService`.

use crate::live_diagnostics_service::{
    diagnostics_native_dispatch, DiagnosticsService, NativeDiagnosticsRequest,
    NativeDiagnosticsResponse,
};
use sha2::{Digest, Sha256};
use std::sync::{Arc, Mutex};

#[cfg(windows)]
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Condvar, OnceLock,
};

pub const NATIVE_DIAGNOSTICS_PIPE_SCHEMA_VERSION: u32 = 1;
pub const MAX_NATIVE_DIAGNOSTICS_FRAME_BYTES: usize = 4 * 1024 * 1024;
const PIPE_FRAME_DEADLINE: std::time::Duration = std::time::Duration::from_secs(90);

/// Canonical per-user endpoint. It is intentionally derived exactly like
/// Blueprint's Hub-owned pipe identity, so users cannot collide.
pub fn canonical_pipe_name_for_user(user_profile: &str) -> String {
    let digest = Sha256::digest(user_profile.as_bytes());
    format!(
        r"\\.\pipe\membrane-diagnostics-{}",
        hex::encode(digest)[..16].to_string()
    )
}

pub fn canonical_pipe_name() -> String {
    canonical_pipe_name_for_user(&std::env::var("USERPROFILE").unwrap_or_default())
}

/// Start one Hub-lifetime native transport. Repeated router construction does
/// not start another service or pipe instance.
#[cfg(windows)]
pub fn start_resident(service: Arc<Mutex<DiagnosticsService>>, health_identity: serde_json::Value) {
    static ACTIVE: OnceLock<Mutex<Option<ResidentServer>>> = OnceLock::new();
    let active = ACTIVE.get_or_init(|| Mutex::new(None));
    let lifecycle = crate::service::lifecycle_control();
    let service_generation = health_identity
        .get("serviceGeneration")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);

    // A replacement runtime may be installed before its predecessor has
    // observed shutdown. Stop & join that predecessor before publishing a new
    // active server, so its old identity can never serve the fresh pipe.
    let mut slot = active.lock().expect("native diagnostics active state");
    if let Some(previous) = slot.as_ref() {
        if previous.service_generation.as_deref() == service_generation.as_deref()
            && !previous.is_retired()
        {
            return;
        }
        previous.stop();
        previous.wait_stopped();
    }

    let current = ResidentServer::new(service_generation, lifecycle.clone());
    *slot = Some(current.clone());
    drop(slot);

    let pipe_name = canonical_pipe_name();
    let thread_state = current.clone();
    let active = active;
    if std::thread::Builder::new()
        .name("membrane-diagnostics-pipe".to_string())
        .spawn(move || {
            windows::serve(
                pipe_name,
                service,
                health_identity,
                lifecycle,
                thread_state.clone(),
            );
            thread_state.mark_stopped();
            let mut slot = active.lock().expect("native diagnostics active state");
            if slot.as_ref().is_some_and(|active| {
                Arc::ptr_eq(&active.stop_requested, &thread_state.stop_requested)
            }) {
                *slot = None;
            }
        })
        .is_err()
    {
        current.mark_stopped();
        let mut slot = active.lock().expect("native diagnostics active state");
        if slot
            .as_ref()
            .is_some_and(|active| Arc::ptr_eq(&active.stop_requested, &current.stop_requested))
        {
            *slot = None;
        }
    }
}

#[cfg(windows)]
#[derive(Clone)]
struct ResidentServer {
    service_generation: Option<String>,
    lifecycle: crate::service::LifecycleControl,
    stop_requested: Arc<AtomicBool>,
    stopped: Arc<(Mutex<bool>, Condvar)>,
}

#[cfg(windows)]
impl ResidentServer {
    fn new(
        service_generation: Option<String>,
        lifecycle: crate::service::LifecycleControl,
    ) -> Self {
        Self {
            service_generation,
            lifecycle,
            stop_requested: Arc::new(AtomicBool::new(false)),
            stopped: Arc::new((Mutex::new(false), Condvar::new())),
        }
    }

    fn stop(&self) {
        self.stop_requested.store(true, Ordering::Release);
    }

    fn wait_stopped(&self) {
        let (lock, wake) = &*self.stopped;
        let mut stopped = lock.lock().expect("native diagnostics stop state");
        while !*stopped {
            stopped = wake.wait(stopped).expect("native diagnostics stop state");
        }
    }

    fn mark_stopped(&self) {
        let (lock, wake) = &*self.stopped;
        if let Ok(mut stopped) = lock.lock() {
            *stopped = true;
            wake.notify_all();
        }
    }

    fn is_stopping(&self) -> bool {
        self.stop_requested.load(Ordering::Acquire)
    }

    fn is_retired(&self) -> bool {
        self.is_stopping()
            || self.lifecycle.shutdown_requested()
            || self
                .stopped
                .0
                .lock()
                .map(|stopped| *stopped)
                .unwrap_or(true)
    }
}

#[cfg(not(windows))]
pub fn start_resident(
    _service: Arc<Mutex<DiagnosticsService>>,
    _health_identity: serde_json::Value,
) {
}

fn validate_identity(
    request: &NativeDiagnosticsRequest,
    health_identity: &serde_json::Value,
) -> Result<(), (&'static str, String)> {
    let expected = health_identity
        .as_object()
        .and_then(|value| {
            Some([
                value.get("installationId")?.as_str()?,
                value.get("cortexStoreId")?.as_str()?,
                value.get("releaseGeneration")?.as_str()?,
                value.get("serviceGeneration")?.as_str()?,
            ])
        })
        .ok_or_else(|| {
            (
                "identity_unavailable",
                "native diagnostics Hub identity is unavailable".to_string(),
            )
        })?;
    if [
        request.installation_id.as_str(),
        request.cortex_store_id.as_str(),
        request.release_generation.as_str(),
        request.service_generation.as_str(),
    ] != expected
    {
        return Err((
            "identity_mismatch",
            "native diagnostics request identity fence mismatch".into(),
        ));
    }
    Ok(())
}

#[cfg(windows)]
mod windows {
    use super::*;
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr::null_mut;
    use std::time::{Duration, Instant};

    type Handle = *mut c_void;
    const INVALID_HANDLE_VALUE: Handle = -1isize as Handle;
    const PIPE_ACCESS_DUPLEX: u32 = 0x0000_0003;
    const PIPE_TYPE_BYTE: u32 = 0x0000_0000;
    const PIPE_READMODE_BYTE: u32 = 0x0000_0000;
    const PIPE_NOWAIT: u32 = 0x0000_0001;
    // Local Hub transport only; SMB/remote clients are never peers.
    const PIPE_REJECT_REMOTE_CLIENTS: u32 = 0x0000_0008;
    const ERROR_PIPE_CONNECTED: u32 = 535;
    const ERROR_PIPE_LISTENING: u32 = 536;
    const ERROR_NO_DATA: u32 = 232;
    const ERROR_BROKEN_PIPE: u32 = 109;

    #[link(name = "kernel32")]
    extern "system" {
        fn CreateNamedPipeW(
            name: *const u16,
            open_mode: u32,
            pipe_mode: u32,
            max_instances: u32,
            out_size: u32,
            in_size: u32,
            timeout: u32,
            security: *mut c_void,
        ) -> Handle;
        fn ConnectNamedPipe(pipe: Handle, overlapped: *mut c_void) -> i32;
        fn DisconnectNamedPipe(pipe: Handle) -> i32;
        fn CloseHandle(handle: Handle) -> i32;
        fn ReadFile(
            handle: Handle,
            buffer: *mut c_void,
            bytes: u32,
            read: *mut u32,
            overlapped: *mut c_void,
        ) -> i32;
        fn WriteFile(
            handle: Handle,
            buffer: *const c_void,
            bytes: u32,
            written: *mut u32,
            overlapped: *mut c_void,
        ) -> i32;
        fn GetLastError() -> u32;
    }

    struct Pipe(Handle);
    impl Drop for Pipe {
        fn drop(&mut self) {
            unsafe {
                let _ = DisconnectNamedPipe(self.0);
                let _ = CloseHandle(self.0);
            }
        }
    }

    pub(super) fn serve(
        name: String,
        service: Arc<Mutex<DiagnosticsService>>,
        health_identity: serde_json::Value,
        lifecycle: crate::service::LifecycleControl,
        server: ResidentServer,
    ) {
        while !lifecycle.shutdown_requested() && !server.is_stopping() {
            let pipe = match create(&name) {
                Ok(pipe) => pipe,
                Err(error) => {
                    eprintln!("native diagnostics pipe unavailable: {error}");
                    if server.is_stopping() || lifecycle.shutdown_requested() {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(10));
                    continue;
                }
            };
            if !connect(pipe.0, &lifecycle, &server) {
                continue;
            }
            let deadline = Instant::now() + PIPE_FRAME_DEADLINE;
            let response = match read_frame(pipe.0, deadline, &lifecycle, &server) {
                Ok(frame) if !should_stop(&lifecycle, &server) => {
                    dispatch(&service, &health_identity, frame)
                        .unwrap_or_else(|(code, detail)| error_response(code, detail))
                }
                Ok(_) => break,
                Err((code, detail)) => error_response(code, detail),
            };
            if !should_stop(&lifecycle, &server) {
                let _ = write_frame(pipe.0, &response, deadline, &lifecycle, &server);
            }
        }
    }

    fn should_stop(lifecycle: &crate::service::LifecycleControl, server: &ResidentServer) -> bool {
        lifecycle.shutdown_requested() || server.is_stopping()
    }

    fn create(name: &str) -> std::io::Result<Pipe> {
        let wide: Vec<u16> = std::ffi::OsStr::new(name)
            .encode_wide()
            .chain(Some(0))
            .collect();
        let handle = crate::serve::windows_owner_only_security(|security, _dacl| {
            let handle = unsafe {
                CreateNamedPipeW(
                    wide.as_ptr(),
                    PIPE_ACCESS_DUPLEX,
                    PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_NOWAIT | PIPE_REJECT_REMOTE_CLIENTS,
                    1,
                    MAX_NATIVE_DIAGNOSTICS_FRAME_BYTES as u32,
                    MAX_NATIVE_DIAGNOSTICS_FRAME_BYTES as u32,
                    1000,
                    security,
                )
            };
            if handle == INVALID_HANDLE_VALUE {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(handle)
            }
        })?;
        Ok(Pipe(handle))
    }

    fn connect(
        pipe: Handle,
        lifecycle: &crate::service::LifecycleControl,
        server: &ResidentServer,
    ) -> bool {
        loop {
            if should_stop(lifecycle, server) {
                return false;
            }
            if unsafe { ConnectNamedPipe(pipe, null_mut()) } != 0 {
                return true;
            }
            match unsafe { GetLastError() } {
                ERROR_PIPE_CONNECTED => return true,
                ERROR_PIPE_LISTENING => {
                    if should_stop(lifecycle, server) {
                        return false;
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
                _ => return false,
            }
        }
    }

    fn read_frame(
        pipe: Handle,
        deadline: Instant,
        lifecycle: &crate::service::LifecycleControl,
        server: &ResidentServer,
    ) -> Result<Vec<u8>, (&'static str, String)> {
        let mut header = [0u8; 4];
        read_exact(pipe, &mut header, deadline, lifecycle, server)?;
        let length = u32::from_le_bytes(header) as usize;
        if length == 0 || length > MAX_NATIVE_DIAGNOSTICS_FRAME_BYTES {
            return Err((
                "invalid_frame",
                "frame length is outside native diagnostics limit".into(),
            ));
        }
        let mut body = vec![0u8; length];
        read_exact(pipe, &mut body, deadline, lifecycle, server)?;
        Ok(body)
    }

    fn read_exact(
        pipe: Handle,
        buffer: &mut [u8],
        deadline: Instant,
        lifecycle: &crate::service::LifecycleControl,
        server: &ResidentServer,
    ) -> Result<(), (&'static str, String)> {
        let mut offset = 0;
        while offset < buffer.len() {
            if should_stop(lifecycle, server) {
                return Err((
                    "transport_closed",
                    "native diagnostics server is stopping".into(),
                ));
            }
            if Instant::now() >= deadline {
                return Err((
                    "deadline_exceeded",
                    "native diagnostics frame read timed out".into(),
                ));
            }
            let mut count = 0u32;
            let ok = unsafe {
                ReadFile(
                    pipe,
                    buffer[offset..].as_mut_ptr().cast(),
                    (buffer.len() - offset) as u32,
                    &mut count,
                    null_mut(),
                )
            };
            if ok != 0 && count > 0 {
                offset += count as usize;
                continue;
            }
            match unsafe { GetLastError() } {
                ERROR_NO_DATA => std::thread::sleep(Duration::from_millis(2)),
                ERROR_BROKEN_PIPE => {
                    return Err((
                        "transport_closed",
                        "native diagnostics client disconnected".into(),
                    ))
                }
                error => {
                    return Err((
                        "transport_error",
                        format!("native diagnostics pipe read failed: {error}"),
                    ))
                }
            }
        }
        Ok(())
    }

    fn dispatch(
        service: &Arc<Mutex<DiagnosticsService>>,
        health_identity: &serde_json::Value,
        frame: Vec<u8>,
    ) -> Result<NativeDiagnosticsResponse, (&'static str, String)> {
        let request: NativeDiagnosticsRequest = serde_json::from_slice(&frame)
            .map_err(|error| ("invalid_request", error.to_string()))?;
        if request.method == "GET" && request.path == "/health" {
            return Ok(NativeDiagnosticsResponse {
                schema_version: NATIVE_DIAGNOSTICS_PIPE_SCHEMA_VERSION,
                id: request.id,
                status: 200,
                body: health_identity.clone(),
            });
        }
        validate_identity(&request, health_identity)?;
        Ok(diagnostics_native_dispatch(service, request))
    }

    fn error_response(code: &'static str, detail: String) -> NativeDiagnosticsResponse {
        NativeDiagnosticsResponse {
            schema_version: NATIVE_DIAGNOSTICS_PIPE_SCHEMA_VERSION,
            id: String::new(),
            status: if code == "deadline_exceeded" {
                408
            } else {
                400
            },
            body: serde_json::json!({"error":{"code":code,"detail":detail}}),
        }
    }

    fn write_frame(
        pipe: Handle,
        response: &NativeDiagnosticsResponse,
        deadline: Instant,
        lifecycle: &crate::service::LifecycleControl,
        server: &ResidentServer,
    ) -> Result<(), ()> {
        let body = serde_json::to_vec(response).map_err(|_| ())?;
        if body.len() > MAX_NATIVE_DIAGNOSTICS_FRAME_BYTES {
            return Err(());
        }
        let mut frame = Vec::with_capacity(body.len() + 4);
        frame.extend_from_slice(&(body.len() as u32).to_le_bytes());
        frame.extend_from_slice(&body);
        let mut offset = 0;
        while offset < frame.len() && Instant::now() < deadline {
            if should_stop(lifecycle, server) {
                return Err(());
            }
            let mut count = 0u32;
            let ok = unsafe {
                WriteFile(
                    pipe,
                    frame[offset..].as_ptr().cast(),
                    (frame.len() - offset) as u32,
                    &mut count,
                    null_mut(),
                )
            };
            if ok != 0 && count > 0 {
                offset += count as usize;
            } else {
                std::thread::sleep(Duration::from_millis(2));
            }
        }
        (offset == frame.len()).then_some(()).ok_or(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pipe_name_is_per_user_and_stable() {
        assert_eq!(
            canonical_pipe_name_for_user("C:\\Users\\one"),
            canonical_pipe_name_for_user("C:\\Users\\one")
        );
        assert_ne!(
            canonical_pipe_name_for_user("C:\\Users\\one"),
            canonical_pipe_name_for_user("C:\\Users\\two")
        );
        assert!(canonical_pipe_name_for_user("C:\\Users\\one")
            .starts_with(r"\\.\pipe\membrane-diagnostics-"));
    }

    #[test]
    fn native_frame_limit_covers_existing_diagnostics_limit() {
        assert_eq!(MAX_NATIVE_DIAGNOSTICS_FRAME_BYTES, 4 * 1024 * 1024);
        assert_eq!(NATIVE_DIAGNOSTICS_PIPE_SCHEMA_VERSION, 1);
    }

    #[cfg(windows)]
    #[test]
    fn resident_server_stop_state_can_be_released_for_restart() {
        let previous = ResidentServer::new(
            Some("service-1".into()),
            crate::service::LifecycleControl::default(),
        );
        previous.stop();
        assert!(previous.is_stopping());
        assert!(previous.is_retired());
        previous.mark_stopped();
        previous.wait_stopped();

        let current = ResidentServer::new(
            Some("service-2".into()),
            crate::service::LifecycleControl::default(),
        );
        assert!(!current.is_stopping());
        assert!(!current.is_retired());
    }

    #[cfg(windows)]
    #[test]
    fn resident_server_same_process_generation_restarts_after_lifecycle_drain() {
        let lifecycle = crate::service::LifecycleControl::default();
        let previous = ResidentServer::new(Some("service-1".into()), lifecycle.clone());
        assert!(!previous.is_retired());

        lifecycle.request_drain(Some("restart"));

        assert!(previous.is_retired());
    }

    #[test]
    fn native_identity_fence_rejects_stale_generation() {
        let health = serde_json::json!({
            "installationId": "install-1",
            "cortexStoreId": "store-1",
            "releaseGeneration": "release-1",
            "serviceGeneration": "service-1"
        });
        let mut request = NativeDiagnosticsRequest {
            schema_version: NATIVE_DIAGNOSTICS_PIPE_SCHEMA_VERSION,
            id: "frame-fence".into(),
            method: "GET".into(),
            path: "/diagnostics/status".into(),
            query: serde_json::Value::Null,
            body: serde_json::Value::Null,
            installation_id: "install-1".into(),
            cortex_store_id: "store-1".into(),
            release_generation: "release-1".into(),
            service_generation: "service-1".into(),
        };
        assert!(validate_identity(&request, &health).is_ok());
        request.release_generation = "stale-release".into();
        assert!(validate_identity(&request, &health).is_err());
        request.release_generation = "release-1".into();
        request.service_generation = "stale-service".into();
        assert!(validate_identity(&request, &health).is_err());
    }

    #[test]
    fn status_frame_uses_resident_service_not_a_second_runtime() {
        let root = tempfile::tempdir().expect("temporary diagnostics root");
        let service = Arc::new(Mutex::new(
            DiagnosticsService::with_data_root(root.path().to_path_buf())
                .expect("diagnostics service"),
        ));
        let expected = service.lock().expect("service lock").status();
        let actual = diagnostics_native_dispatch(
            &service,
            NativeDiagnosticsRequest {
                schema_version: NATIVE_DIAGNOSTICS_PIPE_SCHEMA_VERSION,
                id: "frame-1".into(),
                method: "GET".into(),
                path: "/diagnostics/status".into(),
                query: serde_json::Value::Null,
                body: serde_json::Value::Null,
                installation_id: String::new(),
                cortex_store_id: String::new(),
                release_generation: String::new(),
                service_generation: String::new(),
            },
        );
        assert_eq!(actual.id, "frame-1");
        assert_eq!(actual.status, 200);
        assert_eq!(actual.body, expected);
    }

    #[test]
    fn unknown_operation_is_typed_without_fallback_transport() {
        let root = tempfile::tempdir().expect("temporary diagnostics root");
        let service = Arc::new(Mutex::new(
            DiagnosticsService::with_data_root(root.path().to_path_buf())
                .expect("diagnostics service"),
        ));
        let actual = diagnostics_native_dispatch(
            &service,
            NativeDiagnosticsRequest {
                schema_version: NATIVE_DIAGNOSTICS_PIPE_SCHEMA_VERSION,
                id: "frame-2".into(),
                method: "GET".into(),
                path: "/diagnostics/not-exposed".into(),
                query: serde_json::Value::Null,
                body: serde_json::Value::Null,
                installation_id: String::new(),
                cortex_store_id: String::new(),
                release_generation: String::new(),
                service_generation: String::new(),
            },
        );
        assert_eq!(actual.status, 404);
        assert_eq!(actual.body["error"]["code"], "not_found");
    }
}
