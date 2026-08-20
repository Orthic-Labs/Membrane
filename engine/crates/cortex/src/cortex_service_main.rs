//! Cortex durable-knowledge service entry point.

#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() -> Result<(), String> {
    if std::env::var("MEMBRANE_OWNER_PIPE").as_deref() != Ok("1") {
        return Err("cortex_service_requires_membrane_owner".into());
    }
    std::thread::spawn(|| {
        use std::io::Read;
        let mut byte = [0_u8; 1];
        loop {
            match std::io::stdin().read(&mut byte) {
                Ok(0) | Err(_) => std::process::exit(0),
                Ok(_) => {}
            }
        }
    });
    membrane_runtime::service::run_service()
}
