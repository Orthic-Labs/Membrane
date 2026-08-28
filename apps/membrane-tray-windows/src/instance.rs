//! Per-session single-instance command signal for Windows tray ownership.

#[cfg(windows)]
mod platform {
    use std::{io, ptr::null};

    use windows_sys::Win32::{
        Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE, WAIT_OBJECT_0},
        System::Threading::{CreateEventW, SetEvent, WaitForSingleObject},
    };

    const OPEN_EVENT_NAME: &str = "Local\\MembraneTrayOpenDashboardV1";
    const ACTIVATE_EVENT_NAME: &str = "Local\\MembraneTrayActivateV1";

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum InstanceSignal {
        OpenDashboard,
        Activate,
    }

    pub struct InstanceEvent {
        open_handle: HANDLE,
        activate_handle: HANDLE,
        primary: bool,
    }

    impl InstanceEvent {
        pub fn acquire() -> io::Result<Self> {
            let open_name = OPEN_EVENT_NAME
                .encode_utf16()
                .chain([0])
                .collect::<Vec<_>>();
            let open_handle = unsafe { CreateEventW(null(), 0, 0, open_name.as_ptr()) };
            if open_handle.is_null() {
                return Err(io::Error::last_os_error());
            }
            let primary = unsafe { GetLastError() } != ERROR_ALREADY_EXISTS;
            let activate_name = ACTIVATE_EVENT_NAME
                .encode_utf16()
                .chain([0])
                .collect::<Vec<_>>();
            let activate_handle = unsafe { CreateEventW(null(), 0, 0, activate_name.as_ptr()) };
            if activate_handle.is_null() {
                let error = io::Error::last_os_error();
                let _ = unsafe { CloseHandle(open_handle) };
                return Err(error);
            }
            Ok(Self {
                open_handle,
                activate_handle,
                primary,
            })
        }

        pub fn is_primary(&self) -> bool {
            self.primary
        }

        pub fn signal(&self, signal: InstanceSignal) -> io::Result<()> {
            let handle = match signal {
                InstanceSignal::OpenDashboard => self.open_handle,
                InstanceSignal::Activate => self.activate_handle,
            };
            if unsafe { SetEvent(handle) } == 0 {
                Err(io::Error::last_os_error())
            } else {
                Ok(())
            }
        }

        pub fn take_signal(&self, signal: InstanceSignal) -> bool {
            let handle = match signal {
                InstanceSignal::OpenDashboard => self.open_handle,
                InstanceSignal::Activate => self.activate_handle,
            };
            (unsafe { WaitForSingleObject(handle, 0) }) == WAIT_OBJECT_0
        }
    }

    impl Drop for InstanceEvent {
        fn drop(&mut self) {
            let _ = unsafe { CloseHandle(self.open_handle) };
            let _ = unsafe { CloseHandle(self.activate_handle) };
        }
    }
}

#[cfg(not(windows))]
mod platform {
    use std::io;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum InstanceSignal {
        OpenDashboard,
        Activate,
    }

    pub struct InstanceEvent;

    impl InstanceEvent {
        pub fn acquire() -> io::Result<Self> {
            Ok(Self)
        }

        pub fn is_primary(&self) -> bool {
            true
        }

        pub fn signal(&self, _signal: InstanceSignal) -> io::Result<()> {
            Ok(())
        }

        pub fn take_signal(&self, _signal: InstanceSignal) -> bool {
            false
        }
    }
}

pub use platform::{InstanceEvent, InstanceSignal};
