//! Crypt CLI binary entry point.
//!
//! MBR-107: this binary is a compatibility facade over `membrane_runtime`. On
//! first invocation it emits a single structured migration notice to stderr
//! so operators know to migrate to the new `membrane` binary.

fn main() {
    // Emit the migration notice exactly once per process. Subsequent calls in
    // this binary are silent (guarded inside `vocabulary::emit_facade_notice_once`).
    let _ = crypt::facade::ensure_migration_notice();
    membrane_runtime::cli::run_cli();
}
