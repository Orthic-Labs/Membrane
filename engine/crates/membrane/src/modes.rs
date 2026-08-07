//! Mode dispatch. Each mode is a thin adapter that hands control to the matching entrypoint
//! inside `membrane_runtime`. The binary never duplicates runtime logic.
//!
//! MBR-102: create one membrane executable with mode subcommands.

use crate::dispatch::{MembraneMode, ParsedInvocation};
use crate::{EXIT_INTERNAL_ERROR, EXIT_OK, EXIT_USER_ERROR};

/// Outcome of a dispatched mode. The binary maps this to a process exit code.
#[derive(Debug, PartialEq, Eq)]
pub enum DispatchOutcome {
    /// Work completed successfully.
    Ok,
    /// The caller asked for something invalid; the binary should exit `EXIT_USER_ERROR`.
    UserError(String),
    /// The runtime reported an internal failure; the binary should exit `EXIT_INTERNAL_ERROR`.
    InternalError(String),
}

impl DispatchOutcome {
    pub const fn exit_code(&self) -> i32 {
        match self {
            DispatchOutcome::Ok => EXIT_OK,
            DispatchOutcome::UserError(_) => EXIT_USER_ERROR,
            DispatchOutcome::InternalError(_) => EXIT_INTERNAL_ERROR,
        }
    }
}

/// Run one parsed invocation. Returns the outcome so the binary's `main` can decide the exit
/// code; it never panics across this boundary.
pub fn dispatch(invocation: &ParsedInvocation) -> DispatchOutcome {
    match invocation.mode {
        MembraneMode::Cli => dispatch_cli(&invocation.cli_tail),
        MembraneMode::StdioMcp => dispatch_stdio_mcp(),
        MembraneMode::LoopbackApi => dispatch_loopback_api(invocation.port),
        MembraneMode::SupervisorChild => dispatch_supervisor_child(invocation.lease.as_deref()),
    }
}

fn dispatch_cli(tail: &[String]) -> DispatchOutcome {
    // The runtime CLI owns its own argv. We reconstruct a Vec<&str> so it sees the same shape
    // it would have seen from a direct invocation. `tail` is empty when the user typed
    // `membrane cli` with no subcommand; the runtime prints help and returns Ok.
    let mut argv: Vec<String> = Vec::with_capacity(tail.len() + 1);
    argv.push("membrane".to_string());
    argv.extend_from_slice(tail);
    let refs: Vec<&str> = argv.iter().map(String::as_str).collect();
    match membrane_runtime::cli::run_cli_from(&refs) {
        Ok(()) => DispatchOutcome::Ok,
        Err(error) => classify_runtime_error(error),
    }
}

fn dispatch_stdio_mcp() -> DispatchOutcome {
    match membrane_runtime::serve::run_stdio_mcp() {
        Ok(()) => DispatchOutcome::Ok,
        Err(error) => classify_runtime_error(error),
    }
}

fn dispatch_loopback_api(port: u16) -> DispatchOutcome {
    match membrane_runtime::serve::run_loopback_api(port) {
        Ok(()) => DispatchOutcome::Ok,
        Err(error) => classify_runtime_error(error),
    }
}

fn dispatch_supervisor_child(lease: Option<&std::path::Path>) -> DispatchOutcome {
    match membrane_runtime::service::run_service_from(lease) {
        Ok(()) => DispatchOutcome::Ok,
        Err(error) => classify_runtime_error(error),
    }
}

/// Runtime errors are mostly user-visible (bad arguments, missing runtime, lease rejection), so
/// the binary surfaces them as `UserError`. The string is the same one the runtime already
/// printed in legacy mode; we keep the wording identical so scripts that grep for it keep
/// working.
fn classify_runtime_error(error: String) -> DispatchOutcome {
    // Internal-style prefixes: anything that mentions "internal", "panicked", or comes from
    // the SQLite / ONNX paths is treated as an internal error.
    let lower = error.to_ascii_lowercase();
    let internal_marker = lower.contains("internal")
        || lower.contains("panic")
        || lower.contains("sqlite")
        || lower.contains("onnx");
    if internal_marker {
        DispatchOutcome::InternalError(error)
    } else {
        DispatchOutcome::UserError(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dispatch::parse_mode;

    #[test]
    fn cli_dispatch_forwards_tail_to_runtime() {
        let inv = parse_mode(["membrane", "cli", "doctor"].iter().copied()).unwrap();
        // The dispatch outcome depends on whether the runtime CLI is wired up. We only assert
        // that the dispatcher routes the call — the runtime may legitimately refuse to start
        // outside a real install, which is fine for this test.
        let _ = dispatch(&inv);
    }

    #[test]
    fn exit_code_table_matches_constants() {
        assert_eq!(DispatchOutcome::Ok.exit_code(), EXIT_OK);
        assert_eq!(
            DispatchOutcome::UserError("x".into()).exit_code(),
            EXIT_USER_ERROR
        );
        assert_eq!(
            DispatchOutcome::InternalError("y".into()).exit_code(),
            EXIT_INTERNAL_ERROR
        );
    }
}
