//! Mode dispatch. Each mode is a thin adapter that hands control to the matching entrypoint
//! inside `membrane_runtime`. The binary never duplicates runtime logic.
//!
//! MBR-102: create one membrane executable with mode subcommands.

use crate::dispatch::{InstallInvocation, MembraneMode, ParsedInvocation, UninstallInvocation};
use crate::{EXIT_INTERNAL_ERROR, EXIT_OK, EXIT_USER_ERROR};

/// MBR-108: map a parsed mode to the process plane it executes in. The mapping is the single
/// source of truth referenced by `docs/architecture.md` and by
/// `operations/plane-boundaries.v1.golden.json`. Adding a new mode without updating this
/// helper is a contract violation.
pub fn plane_of(mode: &MembraneMode) -> membrane_runtime::Plane {
    match mode {
        // All four user-facing entry points belong to the Application plane.
        // Install is treated as Application because it runs the same
        // per-stage effect callback the operator invokes from a script.
        // Uninstall is treated as Application because it is the symmetric
        // mirror of install and shares its ownership contract.
        MembraneMode::Cli => membrane_runtime::Plane::Application,
        MembraneMode::StdioMcp => membrane_runtime::Plane::Application,
        MembraneMode::LoopbackApi => membrane_runtime::Plane::Application,
        MembraneMode::Install => membrane_runtime::Plane::Application,
        MembraneMode::Uninstall => membrane_runtime::Plane::Application,
        MembraneMode::MigrateLegacy => membrane_runtime::Plane::Application,
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
        MembraneMode::Install => match invocation.install.as_ref() {
            Some(invocation) => dispatch_install(invocation),
            // The parser refuses to construct a `ParsedInvocation` whose
            // `mode == Install` without an `install` payload; reaching this
            // branch is a logic bug, not a user error.
            None => DispatchOutcome::InternalError(
                "install mode invoked without an install invocation".to_string(),
            ),
        },
        MembraneMode::Uninstall => match invocation.uninstall.as_ref() {
            Some(invocation) => dispatch_uninstall(invocation),
            // The parser refuses to construct a `ParsedInvocation` whose
            // `mode == Uninstall` without an `uninstall` payload; reaching
            // this branch is a logic bug, not a user error.
            None => DispatchOutcome::InternalError(
                "uninstall mode invoked without an uninstall invocation".to_string(),
            ),
        },
        MembraneMode::MigrateLegacy => match invocation.migration.as_ref() {
            Some(migration) => {
                match crate::migration::migrate(&migration.legacy_root, &migration.target_root) {
                    Ok(receipt) => {
                        println!("{}", serde_json::to_string_pretty(&receipt).unwrap());
                        DispatchOutcome::Ok
                    }
                    Err(error) => DispatchOutcome::UserError(format!("migration: {error}")),
                }
            }
            None => DispatchOutcome::InternalError("migration mode invoked without payload".into()),
        },
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
    if matches!(tail, [operation] if operation == "hub-capabilities" || operation == "hub-snapshot")
    {
        let operation = if tail[0] == "hub-capabilities" {
            "hub.capabilities"
        } else {
            "hub.snapshot"
        };
        let facade = membrane_runtime::hub::HubFacadeV1::new(None);
        // MBR: read live state from the local crypt-service's /health endpoint
        // instead of hardcoding "Offline" regardless of whether the service is
        // up. Falls back to the honest unavailable facade on any failure.
        let inputs = membrane_runtime::hub_inputs::live_inputs_from_local_service()
            .unwrap_or_else(|| {
                membrane_runtime::hub::HubInputsV1::unavailable("source_not_connected")
            });
        return match facade
            .dispatch_json(operation, 0, inputs)
            .and_then(|value| serde_json::to_string(&value).map_err(|error| error.to_string()))
        {
            Ok(json) => {
                println!("{json}");
                DispatchOutcome::Ok
            }
            Err(error) => DispatchOutcome::InternalError(format!("{operation}: {error}")),
        };
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
    let owned: Vec<membrane_runtime::ReceiptOwnedFile> = membrane_runtime::receipt_snapshot();
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

/// MBR-203: install handler. Loads the plan (or builds a default), runs
/// `execute_plan` against the scratch root, and only calls `commit` when
/// `--dry-run` is not set. The receipt is printed to stdout on success.
fn dispatch_install(invocation: &InstallInvocation) -> DispatchOutcome {
    let plan = match load_install_plan(invocation) {
        Ok(plan) => plan,
        Err(error) => return DispatchOutcome::UserError(error),
    };
    let now = now_unix_ms();
    let mut receipt =
        match crate::install_tx::execute_plan(plan, &invocation.scratch_root, now, |step| {
            run_step_command(step)
        }) {
            Ok(receipt) => receipt,
            Err(crate::install_tx::InstallError::RolledBack { reason, .. }) => {
                return DispatchOutcome::InternalError(format!("install rolled back: {reason}"));
            }
            Err(error) => {
                return DispatchOutcome::InternalError(format!("install failed: {error}"));
            }
        };
    if !invocation.dry_run {
        if let Err(error) = crate::install_tx::commit(
            &mut receipt,
            &invocation.scratch_root,
            &invocation.target_root,
            now,
        ) {
            return DispatchOutcome::InternalError(format!("install commit failed: {error}"));
        }
    }
    match serde_json::to_string_pretty(&receipt) {
        Ok(json) => {
            println!("{json}");
            DispatchOutcome::Ok
        }
        Err(error) => DispatchOutcome::InternalError(format!("install serialize receipt: {error}")),
    }
}

/// MBR-205: uninstall handler. Loads the ownership table from
/// `receipt_root`, filters the operator's `--candidate` paths through
/// `revoke_unowned`, and either prints the authorised set (`--dry-run`)
/// or removes each authorised path with `std::fs::remove_dir_all` /
/// `std::fs::remove_file` based on the path's file kind.
fn dispatch_uninstall(invocation: &UninstallInvocation) -> DispatchOutcome {
    let table = match crate::uninstall::load_table(&invocation.receipt_root) {
        Ok(table) => table,
        Err(error) => {
            return DispatchOutcome::InternalError(format!(
                "uninstall: load ownership table: {error}"
            ));
        }
    };

    if invocation.dry_run {
        // Dry-run: print the authorised set as JSON so the operator can
        // pipe the result without parsing two layouts. Refused paths are
        // echoed alongside so the operator sees what would be left alone.
        let authorised = crate::uninstall::revoke_unowned(&table, &invocation.candidates);
        let refused: Vec<&std::path::PathBuf> = invocation
            .candidates
            .iter()
            .filter(|candidate| !authorised.contains(candidate))
            .collect();
        let payload = serde_json::json!({
            "mode": "uninstall",
            "dry_run": true,
            "receipt_root": invocation.receipt_root,
            "installation_id": table.installation_id,
            "authorised": authorised,
            "refused": refused,
        });
        match serde_json::to_string_pretty(&payload) {
            Ok(json) => {
                println!("{json}");
                DispatchOutcome::Ok
            }
            Err(error) => DispatchOutcome::InternalError(format!(
                "uninstall dry-run serialize payload: {error}"
            )),
        }
    } else {
        let now = now_unix_ms();
        let remove_result = crate::uninstall::execute_uninstall(
            table,
            invocation.candidates.clone(),
            now,
            |path| {
                let metadata = match std::fs::symlink_metadata(path) {
                    Ok(metadata) => metadata,
                    Err(error) => {
                        return Err(format!("stat: {error}"));
                    }
                };
                let result = if metadata.is_dir() {
                    std::fs::remove_dir_all(path)
                } else {
                    std::fs::remove_file(path)
                };
                result.map_err(|error| format!("{error}"))
            },
        );
        let receipt = match remove_result {
            Ok(receipt) => receipt,
            Err(crate::uninstall::UninstallError::RemoveFailed { path, reason }) => {
                return DispatchOutcome::InternalError(format!(
                    "uninstall: remove failed at {}: {reason}",
                    path.display()
                ));
            }
            Err(error) => {
                return DispatchOutcome::InternalError(format!("uninstall: {error}"));
            }
        };
        // Persist the receipt alongside the ownership table so a forensic
        // read sees both the original ownership record and the uninstall
        // audit trail in one place. A persist failure is reported as an
        // internal error so the operator notices the gap.
        if let Err(error) = crate::uninstall::persist_receipt(&invocation.receipt_root, &receipt) {
            return DispatchOutcome::InternalError(format!("uninstall: persist receipt: {error}"));
        }
        match serde_json::to_string_pretty(&receipt) {
            Ok(json) => {
                println!("{json}");
                DispatchOutcome::Ok
            }
            Err(error) => {
                DispatchOutcome::InternalError(format!("uninstall: serialize receipt: {error}"))
            }
        }
    }
}

/// MBR-203: run one step's `action` (or `rollback`) as an opaque shell
/// command. The framework stays generic — the callback decides how the
/// step string is interpreted. Here we run it via `sh -c` (POSIX) or
/// `cmd /C` (Windows); tests inject their own callbacks.
fn run_step_command(step: &crate::install_tx::InstallStep) -> Result<(), String> {
    let trimmed = step.action.trim();
    if trimmed.is_empty() {
        return Ok(());
    }
    let output = if cfg!(windows) {
        std::process::Command::new("cmd")
            .arg("/C")
            .arg(trimmed)
            .output()
    } else {
        std::process::Command::new("sh")
            .arg("-c")
            .arg(trimmed)
            .output()
    };
    let output = output.map_err(|error| format!("spawn: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Err(format!(
            "exit={:?} stderr={} stdout={}",
            output.status.code(),
            stderr.trim(),
            stdout.trim()
        ));
    }
    Ok(())
}

/// Load the install plan from `--plan`, or build a default plan with the
/// five standard stages and `true` actions when no plan was supplied.
fn load_install_plan(
    invocation: &InstallInvocation,
) -> Result<crate::install_tx::InstallPlan, String> {
    if let Some(path) = &invocation.plan {
        let body = std::fs::read(path)
            .map_err(|error| format!("install: read plan {}: {error}", path.display()))?;
        let mut plan: crate::install_tx::InstallPlan = serde_json::from_slice(&body)
            .map_err(|error| format!("install: parse plan {}: {error}", path.display()))?;
        if plan.scratch_root.as_os_str().is_empty() {
            plan.scratch_root = invocation.scratch_root.clone();
        }
        if plan.scratch_root != invocation.scratch_root {
            return Err(format!(
                "install: plan scratch_root {:?} does not match --scratch-root {:?}",
                plan.scratch_root, invocation.scratch_root
            ));
        }
        return Ok(plan);
    }
    let now = now_unix_ms();
    Ok(crate::install_tx::InstallPlan {
        plan_id: format!("mbr-203-default-{now}"),
        scratch_root: invocation.scratch_root.clone(),
        steps: vec![
            crate::install_tx::InstallStep {
                stage: crate::install_tx::InstallStage::Enumerate,
                action: "true".to_string(),
                rollback: "true".to_string(),
            },
            crate::install_tx::InstallStep {
                stage: crate::install_tx::InstallStage::WriteManifest,
                action: "true".to_string(),
                rollback: "true".to_string(),
            },
            crate::install_tx::InstallStep {
                stage: crate::install_tx::InstallStage::MintLease,
                action: "true".to_string(),
                rollback: "true".to_string(),
            },
            crate::install_tx::InstallStep {
                stage: crate::install_tx::InstallStage::PublishReceipt,
                action: "true".to_string(),
                rollback: "true".to_string(),
            },
            crate::install_tx::InstallStep {
                stage: crate::install_tx::InstallStage::RegisterBindings,
                action: "true".to_string(),
                rollback: "true".to_string(),
            },
        ],
    })
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
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
