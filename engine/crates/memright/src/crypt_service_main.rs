#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() -> Result<(), String> {
    membrane_runtime::service::run_service()
}
