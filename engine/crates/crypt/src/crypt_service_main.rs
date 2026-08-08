//! Crypt service binary entry point.
//!
//! MBR-107: this binary is a compatibility facade over `membrane_runtime`. On
//! first invocation it emits a single structured migration notice to stderr
//! so operators know to migrate to the new `membrane` binary's
//! `supervisor-child` mode.

#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() -> Result<(), String> {
    if std::env::var("MEMBRANE_OWNER_PIPE").as_deref() != Ok("1") {
        return Err("crypt_service_requires_membrane_owner".into());
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
    // Emit the migration notice exactly once per process. Subsequent calls in
    // this binary are silent (guarded inside `vocabulary::emit_facade_notice_once`).
    let _ = crypt::facade::ensure_migration_notice();
    membrane_runtime::service::run_service()
}
