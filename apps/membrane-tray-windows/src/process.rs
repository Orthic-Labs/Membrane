//! Windows daemon launch primitive.
//!
//! The process is placed in its Job Object by `PROC_THREAD_ATTRIBUTE_JOB_LIST`
//! during `CreateProcessW`. There is intentionally no spawn-then-assign gap:
//! closing the tray's job handle kills every daemon descendant.

use std::path::Path;

use membrane_protocol::{encode_frame, DaemonCommandV1, DaemonLaunchV1};

#[cfg(windows)]
mod windows_impl {
    use super::*;
    use std::{
        ffi::OsStr,
        mem::size_of,
        os::windows::ffi::OsStrExt,
        ptr::{null, null_mut},
        fs::{create_dir_all, OpenOptions},
        io::Write,
        path::PathBuf,
        sync::mpsc::{self, Sender},
        thread,
        time::Duration,
    };

    use windows_sys::Win32::{
        Foundation::{
            CloseHandle, DuplicateHandle, GetLastError, SetHandleInformation,
            DUPLICATE_SAME_ACCESS, HANDLE, HANDLE_FLAG_INHERIT,
        },
        Security::SECURITY_ATTRIBUTES,
        Storage::FileSystem::{ReadFile, WriteFile},
        System::{
            JobObjects::{
                CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject,
                JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            },
            Pipes::CreatePipe,
            Threading::{
                CreateProcessW, DeleteProcThreadAttributeList, GetCurrentProcess,
                GetExitCodeProcess, InitializeProcThreadAttributeList, UpdateProcThreadAttribute,
                WaitForSingleObject, CREATE_NO_WINDOW, EXTENDED_STARTUPINFO_PRESENT, INFINITE,
                PROCESS_INFORMATION, PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
                PROC_THREAD_ATTRIBUTE_JOB_LIST, STARTF_USESTDHANDLES, STARTUPINFOEXW,
            },
        },
    };

    const CONTROL_WRITE_TIMEOUT: Duration = Duration::from_millis(500);

    /// Events emitted by process readers. Event contains only decoded stdout
    /// protocol frames; stderr is consumed separately and never enters channel.
    #[derive(Debug)]
    pub enum ProcessEvent {
        Event(Vec<u8>),
        ProtocolInvalid,
        Exited { code: u32 },
    }

    #[derive(Debug)]
    pub struct DaemonProcess {
        process: HANDLE,
        stdin_write: HANDLE,
        stdout_read: HANDLE,
        stderr_read: HANDLE,
        job: HANDLE,
    }

    // Handles are kernel objects. Each reader only uses its own copied value
    // while supervisor retains ownership until process exit.
    unsafe impl Send for DaemonProcess {}
    unsafe impl Sync for DaemonProcess {}

    fn failed() -> std::io::Error {
        std::io::Error::from_raw_os_error(unsafe { GetLastError() } as i32)
    }

    fn wide(value: &OsStr) -> Vec<u16> {
        value.encode_wide().chain([0]).collect()
    }

    fn close_if_valid(handle: HANDLE) {
        if !handle.is_null() {
            unsafe { CloseHandle(handle) };
        }
    }

    fn make_security_attributes() -> SECURITY_ATTRIBUTES {
        SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: null_mut(),
            bInheritHandle: 1,
        }
    }

    fn duplicate_for_thread(handle: HANDLE) -> std::io::Result<HANDLE> {
        let mut duplicate = null_mut();
        let ok = unsafe {
            DuplicateHandle(
                GetCurrentProcess(),
                handle,
                GetCurrentProcess(),
                &mut duplicate,
                0,
                0,
                DUPLICATE_SAME_ACCESS,
            )
        };
        if ok == 0 {
            Err(failed())
        } else {
            Ok(duplicate)
        }
    }

    /// Launch executable with three inherited anonymous pipes.
    ///
    /// Bearer token is not part of command line or environment. Caller sends
    /// it through [`DaemonProcess::send_launch`] after process creation.
    pub fn launch(executable: &Path) -> std::io::Result<DaemonProcess> {
        unsafe {
            let mut security = make_security_attributes();
            let (
                mut child_stdin,
                mut stdin_write,
                mut stdout_read,
                mut child_stdout,
                mut stderr_read,
                mut child_stderr,
            ) = (
                null_mut(),
                null_mut(),
                null_mut(),
                null_mut(),
                null_mut(),
                null_mut(),
            );
            let mut job: HANDLE = null_mut();
            let mut attribute_list:
                windows_sys::Win32::System::Threading::LPPROC_THREAD_ATTRIBUTE_LIST = null_mut();
            let mut attributes_initialized = false;
            let mut process_info: PROCESS_INFORMATION = std::mem::zeroed();

            macro_rules! fail {
                ($error:expr) => {{
                    for handle in [
                        child_stdin,
                        stdin_write,
                        stdout_read,
                        child_stdout,
                        stderr_read,
                        child_stderr,
                    ] {
                        close_if_valid(handle);
                    }
                    if attributes_initialized {
                        DeleteProcThreadAttributeList(attribute_list);
                    }
                    if !job.is_null() {
                        CloseHandle(job);
                    }
                    return Err($error);
                }};
            }

            if CreatePipe(&mut child_stdin, &mut stdin_write, &mut security, 0) == 0 {
                fail!(failed());
            }
            if CreatePipe(&mut stdout_read, &mut child_stdout, &mut security, 0) == 0 {
                fail!(failed());
            }
            if CreatePipe(&mut stderr_read, &mut child_stderr, &mut security, 0) == 0 {
                fail!(failed());
            }

            // Parent ends must never leak. Child ends remain inheritable and
            // are restricted by the explicit handle-list attribute below.
            for handle in [stdin_write, stdout_read, stderr_read] {
                if SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0) == 0 {
                    fail!(failed());
                }
            }

            job = CreateJobObjectW(null(), null());
            if job.is_null() {
                fail!(failed());
            }
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &limits as *const _ as *const _,
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            ) == 0
            {
                fail!(failed());
            }

            // First call asks for required attribute-list bytes.
            let mut attribute_bytes = 0_usize;
            InitializeProcThreadAttributeList(null_mut(), 2, 0, &mut attribute_bytes);
            if attribute_bytes == 0 {
                fail!(failed());
            }
            let mut attribute_storage = vec![0_u8; attribute_bytes];
            attribute_list = attribute_storage.as_mut_ptr() as _;
            if InitializeProcThreadAttributeList(attribute_list, 2, 0, &mut attribute_bytes) == 0 {
                fail!(failed());
            }
            attributes_initialized = true;

            let job_list = [job];
            if UpdateProcThreadAttribute(
                attribute_list,
                0,
                PROC_THREAD_ATTRIBUTE_JOB_LIST as usize,
                job_list.as_ptr() as *const _,
                size_of::<HANDLE>(),
                null_mut(),
                null(),
            ) == 0
            {
                fail!(failed());
            }
            let inherited = [child_stdin, child_stdout, child_stderr];
            if UpdateProcThreadAttribute(
                attribute_list,
                0,
                PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
                inherited.as_ptr() as *const _,
                size_of::<[HANDLE; 3]>(),
                null_mut(),
                null(),
            ) == 0
            {
                fail!(failed());
            }

            // Supplying application name separately prevents executable path
            // from becoming an argument. Mutable command line is required by
            // CreateProcessW even when it only repeats quoted executable.
            let application = wide(executable.as_os_str());
            let mut command_line = wide(OsStr::new(&format!("\"{}\"", executable.display())));
            let mut startup: STARTUPINFOEXW = std::mem::zeroed();
            startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
            startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
            startup.StartupInfo.hStdInput = child_stdin;
            startup.StartupInfo.hStdOutput = child_stdout;
            startup.StartupInfo.hStdError = child_stderr;
            startup.lpAttributeList = attribute_list;

            let created = CreateProcessW(
                application.as_ptr(),
                command_line.as_mut_ptr(),
                null(),
                null(),
                1,
                EXTENDED_STARTUPINFO_PRESENT | CREATE_NO_WINDOW,
                null(),
                null(),
                &startup.StartupInfo,
                &mut process_info,
            );

            // Child-side handles are owned by daemon now. Closing them here is
            // required so EOF reaches readers when daemon exits.
            close_if_valid(child_stdin);
            close_if_valid(child_stdout);
            close_if_valid(child_stderr);
            DeleteProcThreadAttributeList(attribute_list);

            if created == 0 {
                let error = failed();
                close_if_valid(process_info.hProcess);
                close_if_valid(process_info.hThread);
                for handle in [stdin_write, stdout_read, stderr_read] {
                    close_if_valid(handle);
                }
                close_if_valid(job);
                return Err(error);
            }

            close_if_valid(process_info.hThread);
            Ok(DaemonProcess {
                process: process_info.hProcess,
                stdin_write,
                stdout_read,
                stderr_read,
                job,
            })
        }
    }

    impl DaemonProcess {
        pub fn process_id(&self) -> u32 {
            unsafe { windows_sys::Win32::System::Threading::GetProcessId(self.process) }
        }

        pub fn send_launch(&self, launch: &DaemonLaunchV1) -> std::io::Result<()> {
            let frame = encode_frame(launch).map_err(|error| {
                std::io::Error::new(std::io::ErrorKind::InvalidInput, error.to_string())
            })?;
            self.write_all(&frame)
        }

        pub fn send_command(&self, command: &DaemonCommandV1) -> std::io::Result<()> {
            let frame = encode_frame(command).map_err(|error| {
                std::io::Error::new(std::io::ErrorKind::InvalidInput, error.to_string())
            })?;
            self.write_all(&frame)
        }

        fn write_all(&self, bytes: &[u8]) -> std::io::Result<()> {
            let byte_count = u32::try_from(bytes.len()).map_err(|_| {
                std::io::Error::new(std::io::ErrorKind::InvalidInput, "control frame too large")
            })?;
            let handle = duplicate_for_thread(self.stdin_write)?;
            let payload = bytes.to_vec();
            let handle_value = handle as usize;
            let (sender, receiver) = mpsc::sync_channel(1);
            thread::spawn(move || {
                let handle = handle_value as HANDLE;
                let mut written = 0_u32;
                let ok = unsafe {
                    WriteFile(
                        handle,
                        payload.as_ptr(),
                        byte_count,
                        &mut written,
                        null_mut(),
                    )
                };
                let result = if ok == 0 || written != byte_count {
                    Err(failed())
                } else {
                    Ok(())
                };
                close_if_valid(handle);
                let _ = sender.send(result);
            });
            match receiver.recv_timeout(CONTROL_WRITE_TIMEOUT) {
                Ok(result) => result,
                Err(mpsc::RecvTimeoutError::Timeout) => Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "daemon control pipe write timed out",
                )),
                Err(mpsc::RecvTimeoutError::Disconnected) => Err(std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    "daemon control pipe writer stopped",
                )),
            }
        }

        /// Start blocking pipe readers. Threads finish on child EOF.
        pub fn start_readers(&self, sender: Sender<ProcessEvent>) {
            // Reader/wait threads own duplicates. This lets supervisor close
            // its handles immediately on a protocol error without leaving a
            // thread waiting on a handle value that could be reused.
            let Ok(stdout) = duplicate_for_thread(self.stdout_read) else {
                let _ = sender.send(ProcessEvent::ProtocolInvalid);
                return;
            };
            let Ok(stderr) = duplicate_for_thread(self.stderr_read) else {
                close_if_valid(stdout);
                let _ = sender.send(ProcessEvent::ProtocolInvalid);
                return;
            };
            let Ok(process) = duplicate_for_thread(self.process) else {
                close_if_valid(stdout);
                close_if_valid(stderr);
                let _ = sender.send(ProcessEvent::ProtocolInvalid);
                return;
            };
            let event_sender = sender.clone();
            let stdout_value = stdout as usize;
            thread::spawn(move || read_events(stdout_value as HANDLE, event_sender));
            let stderr_value = stderr as usize;
            thread::spawn(move || drain_stderr(stderr_value as HANDLE));
            let process_value = process as usize;
            thread::spawn(move || {
                let process = process_value as HANDLE;
                unsafe { WaitForSingleObject(process, INFINITE) };
                let mut code = 1_u32;
                unsafe { GetExitCodeProcess(process, &mut code) };
                close_if_valid(process);
                let _ = sender.send(ProcessEvent::Exited { code });
            });
        }
    }

    fn read_events(handle: HANDLE, sender: Sender<ProcessEvent>) {
        let mut bytes = [0_u8; 4096];
        let mut pending = Vec::new();
        loop {
            let mut read = 0_u32;
            let ok = unsafe {
                ReadFile(
                    handle,
                    bytes.as_mut_ptr(),
                    bytes.len() as u32,
                    &mut read,
                    null_mut(),
                )
            };
            if ok == 0 || read == 0 {
                if !pending.is_empty() {
                    let _ = sender.send(ProcessEvent::ProtocolInvalid);
                }
                close_if_valid(handle);
                return;
            }
            pending.extend_from_slice(&bytes[..read as usize]);
            while let Some(index) = pending.iter().position(|byte| *byte == b'\n') {
                let frame: Vec<u8> = pending.drain(..=index).collect();
                if frame.len() > membrane_protocol::DAEMON_IPC_MAX_FRAME_BYTES {
                    let _ = sender.send(ProcessEvent::ProtocolInvalid);
                    close_if_valid(handle);
                    return;
                }
                let _ = sender.send(ProcessEvent::Event(frame));
            }
            if pending.len() > membrane_protocol::DAEMON_IPC_MAX_FRAME_BYTES {
                let _ = sender.send(ProcessEvent::ProtocolInvalid);
                close_if_valid(handle);
                return;
            }
        }
    }

    /// Where the daemon's diagnostics are appended. Matches
    /// `membrane_runtime::paths::log_root()` on Windows, so `membrane cli
    /// doctor paths` names the directory this file actually appears in.
    fn daemon_log_path() -> Option<PathBuf> {
        let root = std::env::var_os("MEMBRANE_LOG_ROOT")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("LOCALAPPDATA").map(|base| PathBuf::from(base).join("Membrane"))
            })?;
        create_dir_all(&root).ok()?;
        Some(root.join("membrane-daemon.log"))
    }

    /// Append the daemon's stderr to its own log file.
    ///
    /// These bytes were previously discarded on the reasoning that diagnostics
    /// must not be mistaken for protocol events. That separation is real, but
    /// discarding is not what preserves it — the file does. The daemon is the
    /// process that serves every request, and with its only diagnostic channel
    /// thrown away a request the daemon dropped left no trace anywhere: an
    /// empty log root, a healthy-looking tray, and a client reporting nothing
    /// but a closed socket. stdout still carries framed protocol and is still
    /// never written here.
    fn drain_stderr(handle: HANDLE) {
        let mut log = daemon_log_path().and_then(|path| {
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .ok()
        });
        let mut bytes = [0_u8; 2048];
        loop {
            let mut read = 0_u32;
            let ok = unsafe {
                ReadFile(
                    handle,
                    bytes.as_mut_ptr(),
                    bytes.len() as u32,
                    &mut read,
                    null_mut(),
                )
            };
            if ok == 0 || read == 0 {
                close_if_valid(handle);
                return;
            }
            // A logging failure never stops the daemon: drop the sink and keep
            // draining, or the pipe fills and the daemon blocks on its own
            // diagnostics.
            if let Some(file) = log.as_mut() {
                if file.write_all(&bytes[..read as usize]).is_err() {
                    log = None;
                } else {
                    let _ = file.flush();
                }
            }
        }
    }

    impl Drop for DaemonProcess {
        fn drop(&mut self) {
            close_if_valid(self.process);
            close_if_valid(self.stdin_write);
            close_if_valid(self.stdout_read);
            close_if_valid(self.stderr_read);
            close_if_valid(self.job);
            self.process = null_mut();
            self.stdin_write = null_mut();
            self.stdout_read = null_mut();
            self.stderr_read = null_mut();
            self.job = null_mut();
        }
    }
}

#[cfg(windows)]
pub use windows_impl::{launch, DaemonProcess, ProcessEvent};

#[cfg(all(test, windows))]
mod native_tests {
    use super::*;

    #[test]
    fn configured_daemon_can_be_created_inside_kill_on_close_job() {
        let Some(path) = std::env::var_os("MEMBRANE_TEST_DAEMON_PATH") else {
            return;
        };
        let process = launch(Path::new(&path)).expect("native daemon process launch failed");
        assert_ne!(process.process_id(), 0);
        drop(process);
    }
}

#[cfg(not(windows))]
mod non_windows_impl {
    use super::*;
    use std::sync::mpsc::Sender;

    #[derive(Debug)]
    pub enum ProcessEvent {}

    #[derive(Debug)]
    pub struct DaemonProcess;

    pub fn launch(_executable: &Path) -> std::io::Result<DaemonProcess> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "Windows tray is only supported on Windows",
        ))
    }

    impl DaemonProcess {
        pub fn process_id(&self) -> u32 {
            0
        }
        pub fn send_launch(&self, _launch: &DaemonLaunchV1) -> std::io::Result<()> {
            Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "Windows only",
            ))
        }
        pub fn send_command(&self, _command: &DaemonCommandV1) -> std::io::Result<()> {
            Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "Windows only",
            ))
        }
        pub fn start_readers(&self, _sender: Sender<ProcessEvent>) {}
    }
}

#[cfg(not(windows))]
pub use non_windows_impl::{launch, DaemonProcess, ProcessEvent};
