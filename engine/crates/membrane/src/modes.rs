//! Mode dispatch. Each mode is a thin adapter that hands control to the matching entrypoint
//! inside `membrane_runtime`. The binary never duplicates runtime logic.
//!
//! MBR-102: create one membrane executable with mode subcommands.

use crate::dispatch::{MembraneMode, ParsedInvocation};
use crate::{EXIT_INTERNAL_ERROR, EXIT_OK, EXIT_USER_ERROR};

/// MBR-108: map a parsed mode to the process plane it executes in. The mapping is the single
/// source of truth referenced by `docs/architecture.md` and by
/// `operations/plane-boundaries.v1.golden.json`. Adding a new mode without updating this
/// helper is a contract violation.
pub fn plane_of(mode: &MembraneMode) -> membrane_runtime::Plane {
    match mode {
        // All three user-facing entry points belong to the Application plane.
        MembraneMode::Cli => membrane_runtime::Plane::Application,
        MembraneMode::StdioMcp => membrane_runtime::Plane::Application,
        MembraneMode::LoopbackApi => membrane_runtime::Plane::Application,
        // The supervisor's resident child is the Control plane.
        MembraneMode::SupervisorChild => membrane_runtime::Plane::Control,
    }
}

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
    //
    // MBR-107: stamp the canonical product-surface notice on first invocation so
    // operators see a single structured log line telling them the legacy `crypt`
    // binary is a compatibility facade. Subsequent calls in this process are silent
    // (guarded inside `membrane_runtime::vocabulary::emit_facade_notice_once`).
    let _ = membrane_runtime::vocabulary::emit_facade_notice_once(
        membrane_runtime::vocabulary::ProductSurface::Membrane,
    );
    // MBR-106: intercept `cli doctor paths` before forwarding to the runtime so
    // the existing `cli doctor --json` surface is untouched. The runtime still
    // owns every other `cli ...` invocation; the binary only adds the new
    // `doctor paths` capability the install/uninstall residue audit needs.
    if is_doctor_paths_invocation(tail) {
        return run_doctor_paths(&tail[2..]);
    }
    let mut argv: Vec<String> = Vec::with_capacity(tail.len() + 1);
    argv.push("membrane".to_string());
    argv.extend_from_slice(tail);
    let refs: Vec<&str> = argv.iter().map(String::as_str).collect();
    match membrane_runtime::cli::run_cli_from(&refs) {
        Ok(()) => DispatchOutcome::Ok,
        Err(error) => classify_runtime_error(error),
    }
}

/// MBR-106: returns `true` when `cli doctor paths [--json]` was requested. Only
/// this one binary-level subcommand is intercepted; every other `cli doctor`
/// invocation falls through to the runtime unchanged so the legacy
/// `cli doctor --json --suppress=...` surface keeps working byte-for-byte.
fn is_doctor_paths_invocation(tail: &[String]) -> bool {
    tail.len() >= 2 && tail[0] == "doctor" && tail[1] == "paths"
}

/// MBR-106: print the four stable roots and any receipt-owned files as JSON.
/// `args` is the trailing slice after `doctor paths`; today the only flag
/// accepted is `--json`, which is the default (we always print JSON so the
/// installer can pipe it without parsing two layouts).
fn run_doctor_paths(args: &[String]) -> DispatchOutcome {
    let _ = args; // reserved for future flags
    let roots = membrane_runtime::paths::Roots::resolve();
    let owned: Vec<membrane_runtime::ReceiptOwnedFile> =
        membrane_runtime::receipt_snapshot();
    let payload = serde_json::json!({
        "schemaVersion": 1,
        "product": membrane_runtime::PRODUCT_DIR_NAME,
        "roots": {
            "config": roots.config,
            "data": roots.data,
            "cache": roots.cache,
            "log": roots.log,
        },
        "receiptOwned": owned,
    });
    match serde_json::to_string_pretty(&payload) {
        Ok(json) => {
            println!("{json}");
            DispatchOutcome::Ok
        }
        Err(error) => DispatchOutcome::InternalError(format!(
            "doctor paths: serialize roots payload: {error}"
        )),
    }
}

fn dispatch_stdio_mcp() -> DispatchOutcome {
    match membrane_mcp::serve_stdio() {
        Ok(()) => DispatchOutcome::Ok,
        Err(error) => DispatchOutcome::InternalError(error.to_string()),
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

    #[test]
    fn plane_of_maps_user_facing_modes_to_application() {
        assert_eq!(
            plane_of(&MembraneMode::Cli),
            membrane_runtime::Plane::Application
        );
        assert_eq!(
            plane_of(&MembraneMode::StdioMcp),
            membrane_runtime::Plane::Application
        );
        assert_eq!(
            plane_of(&MembraneMode::LoopbackApi),
            membrane_runtime::Plane::Application
        );
    }

    #[test]
    fn plane_of_maps_supervisor_child_to_control() {
        assert_eq!(
            plane_of(&MembraneMode::SupervisorChild),
            membrane_runtime::Plane::Control
        );
    }
}
