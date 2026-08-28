//! Per-user Windows startup + first-run marker.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

pub const RUN_KEY_PATH: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
pub const RUN_VALUE_NAME: &str = "Membrane";
pub const LEGACY_RUN_VALUE_NAME: &str = "Membrane Tray";
pub const LOGIN_LAUNCH_ARG: &str = "--login-launch";

pub fn quote_windows_path(path: &Path) -> String {
    let value = path.to_string_lossy().replace('"', "\\\"");
    format!("\"{value}\"")
}

/// Exact command stored in HKCU Run. Startup points at tray executable only;
/// daemon + dashboard are never registered independently.
pub fn run_key_command(exe: &Path) -> String {
    format!("{} {LOGIN_LAUNCH_ARG}", quote_windows_path(exe))
}

#[cfg(windows)]
fn wide(value: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    std::ffi::OsStr::new(value)
        .encode_wide()
        .chain([0])
        .collect()
}

#[cfg(windows)]
pub fn install_for_current_user(exe: &Path) -> io::Result<()> {
    use windows_sys::Win32::{
        Foundation::ERROR_SUCCESS,
        System::Registry::{
            RegCloseKey, RegCreateKeyExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE,
            REG_SZ,
        },
    };
    let _ = delete_value_for_current_user(LEGACY_RUN_VALUE_NAME);
    let key_w = wide(RUN_KEY_PATH);
    let name_w = wide(RUN_VALUE_NAME);
    let value_w = wide(&run_key_command(exe));
    let mut handle: HKEY = std::ptr::null_mut();
    let result = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            key_w.as_ptr(),
            0,
            std::ptr::null(),
            0,
            KEY_SET_VALUE,
            std::ptr::null(),
            &mut handle,
            std::ptr::null_mut(),
        )
    };
    if result != ERROR_SUCCESS {
        return Err(io::Error::from_raw_os_error(result as i32));
    }
    let result = unsafe {
        RegSetValueExW(
            handle,
            name_w.as_ptr(),
            0,
            REG_SZ,
            value_w.as_ptr() as *const u8,
            (value_w.len() * 2) as u32,
        )
    };
    unsafe {
        RegCloseKey(handle);
    }
    if result == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(result as i32))
    }
}

#[cfg(not(windows))]
pub fn install_for_current_user(_exe: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Windows startup is only supported on Windows",
    ))
}

#[cfg(windows)]
fn delete_value_for_current_user(value_name: &str) -> io::Result<()> {
    use windows_sys::Win32::{
        Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS},
        System::Registry::{
            RegCloseKey, RegDeleteValueW, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE,
        },
    };
    let key_w = wide(RUN_KEY_PATH);
    let name_w = wide(value_name);
    let mut handle: HKEY = std::ptr::null_mut();
    let result = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            key_w.as_ptr(),
            0,
            KEY_SET_VALUE,
            &mut handle,
        )
    };
    if result == ERROR_FILE_NOT_FOUND {
        return Ok(());
    }
    if result != ERROR_SUCCESS {
        return Err(io::Error::from_raw_os_error(result as i32));
    }
    let result = unsafe { RegDeleteValueW(handle, name_w.as_ptr()) };
    unsafe {
        RegCloseKey(handle);
    }
    if result == ERROR_SUCCESS || result == ERROR_FILE_NOT_FOUND {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(result as i32))
    }
}

#[cfg(windows)]
pub fn remove_for_current_user() -> io::Result<()> {
    delete_value_for_current_user(RUN_VALUE_NAME)?;
    delete_value_for_current_user(LEGACY_RUN_VALUE_NAME)
}

#[cfg(not(windows))]
pub fn remove_for_current_user() -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Windows startup is only supported on Windows",
    ))
}

#[cfg(windows)]
pub fn is_enabled_for_current_user(exe: &Path) -> io::Result<bool> {
    use windows_sys::Win32::{
        Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS},
        System::Registry::{
            RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE,
            REG_SZ, REG_VALUE_TYPE,
        },
    };
    let key_w = wide(RUN_KEY_PATH);
    let name_w = wide(RUN_VALUE_NAME);
    let mut handle: HKEY = std::ptr::null_mut();
    let result = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            key_w.as_ptr(),
            0,
            KEY_QUERY_VALUE,
            &mut handle,
        )
    };
    if result == ERROR_FILE_NOT_FOUND {
        return Ok(false);
    }
    if result != ERROR_SUCCESS {
        return Err(io::Error::from_raw_os_error(result as i32));
    }
    let mut kind: REG_VALUE_TYPE = 0;
    let mut bytes = 0_u32;
    let result = unsafe {
        RegQueryValueExW(
            handle,
            name_w.as_ptr(),
            std::ptr::null(),
            &mut kind,
            std::ptr::null_mut(),
            &mut bytes,
        )
    };
    if result == ERROR_FILE_NOT_FOUND {
        unsafe {
            RegCloseKey(handle);
        };
        return Ok(false);
    }
    if result != ERROR_SUCCESS || kind != REG_SZ || bytes == 0 {
        unsafe {
            RegCloseKey(handle);
        };
        return Ok(false);
    }
    let mut value = vec![0_u8; bytes as usize];
    let result = unsafe {
        RegQueryValueExW(
            handle,
            name_w.as_ptr(),
            std::ptr::null(),
            &mut kind,
            value.as_mut_ptr(),
            &mut bytes,
        )
    };
    unsafe {
        RegCloseKey(handle);
    }
    if result != ERROR_SUCCESS {
        return Err(io::Error::from_raw_os_error(result as i32));
    }
    let words = value[..bytes as usize]
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .take_while(|word| *word != 0)
        .collect::<Vec<_>>();
    let command = String::from_utf16_lossy(&words);
    Ok(command == run_key_command(exe))
}

#[cfg(not(windows))]
pub fn is_enabled_for_current_user(_exe: &Path) -> io::Result<bool> {
    Ok(false)
}

/// Keep first-run marker under per-user app data. `MEMBRANE_APP_DATA_DIR`
/// exists only for deterministic test/demo runs; normal Windows uses APPDATA.
pub fn first_run_marker_path() -> PathBuf {
    if let Some(root) = std::env::var_os("MEMBRANE_APP_DATA_DIR") {
        return PathBuf::from(root).join("first-run.json");
    }
    if let Some(root) = std::env::var_os("APPDATA") {
        return PathBuf::from(root).join("Membrane").join("first-run.json");
    }
    std::env::temp_dir().join("Membrane").join("first-run.json")
}

pub fn should_show_first_run(login_launch: bool) -> bool {
    !login_launch && !first_run_marker_path().exists()
}

pub fn mark_first_run() -> io::Result<()> {
    let marker = first_run_marker_path();
    if let Some(parent) = marker.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = marker.with_extension("json.tmp");
    fs::write(&temporary, b"{\"version\":1}\n")?;
    fs::rename(temporary, marker)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_value_is_quoted_and_points_only_at_tray_with_login_marker() {
        assert_eq!(
            run_key_command(Path::new(r"C:\Program Files\Membrane\membrane-tray.exe")),
            r#""C:\Program Files\Membrane\membrane-tray.exe" --login-launch"#
        );
    }

    #[test]
    fn marker_policy_hides_login_launch_even_when_marker_is_absent() {
        assert!(!should_show_first_run(true));
    }
}
