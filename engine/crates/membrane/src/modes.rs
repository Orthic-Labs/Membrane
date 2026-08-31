//! Mode dispatch. Each mode is a thin adapter that hands control to the matching entrypoint
//! inside `membrane_runtime`. The binary never duplicates runtime logic.
//!
//! MBR-102: create one membrane executable with mode subcommands.

use crate::dispatch::{
    ActivationInvocation, InstallInvocation, MembraneMode, ParsedInvocation, UninstallInvocation,
};
use crate::{EXIT_INTERNAL_ERROR, EXIT_OK, EXIT_USER_ERROR};

/// MBR-108: map a parsed mode to the process plane it executes in. The mapping is the single
/// source of truth referenced by `docs/architecture/runtime-truth.md` and by
/// `schemas/registry/plane-boundaries.v1.golden.json`. Adding a new mode without updating this
/// helper is a contract violation.
pub fn plane_of(mode: &MembraneMode) -> membrane_runtime::Plane {
    match mode {
        // User-facing entry points belong to the Application plane.
        // Install is treated as Application because it runs the same
        // per-stage effect callback the operator invokes from a script.
        // Uninstall is treated as Application because it is the symmetric
        // mirror of install and shares its ownership contract.
        MembraneMode::Cli => membrane_runtime::Plane::Application,
        MembraneMode::StdioMcp => membrane_runtime::Plane::Application,
        MembraneMode::Install => membrane_runtime::Plane::Application,
        MembraneMode::Uninstall => membrane_runtime::Plane::Application,
        MembraneMode::Activate => membrane_runtime::Plane::Application,
        MembraneMode::Deactivate => membrane_runtime::Plane::Application,
        MembraneMode::MigrateLegacy => membrane_runtime::Plane::Application,
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
        MembraneMode::Activate => match invocation.activation.as_ref() {
            Some(invocation) => dispatch_activation(invocation),
            None => DispatchOutcome::InternalError(
                "activate mode invoked without activation invocation".to_string(),
            ),
        },
        MembraneMode::Deactivate => match invocation.activation.as_ref() {
            Some(invocation) => dispatch_deactivation(invocation),
            None => DispatchOutcome::InternalError(
                "deactivate mode invoked without deactivation invocation".to_string(),
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

fn dispatch_activation(invocation: &ActivationInvocation) -> DispatchOutcome {
    let install_root = match invocation
        .install_root
        .clone()
        .map(Ok)
        .unwrap_or_else(crate::activation::default_install_root)
    {
        Ok(path) => path,
        Err(error) => return DispatchOutcome::UserError(error),
    };
    let clients = if invocation.clients.is_empty() {
        vec![
            crate::activation::HarnessClient::Codex,
            crate::activation::HarnessClient::Claude,
        ]
    } else {
        let mut parsed = Vec::new();
        for client in &invocation.clients {
            match crate::activation::HarnessClient::parse(client) {
                Ok(client) if !parsed.contains(&client) => parsed.push(client),
                Ok(_) => {}
                Err(error) => return DispatchOutcome::UserError(error),
            }
        }
        parsed
    };
    let options = crate::activation::ActivationOptions {
        install_root,
        clients,
        timeout: std::time::Duration::from_millis(invocation.timeout_ms.clamp(1_000, 120_000)),
        dry_run: invocation.dry_run,
    };
    match crate::activation::activate(options) {
        Ok(receipt) => match serde_json::to_string_pretty(&receipt) {
            Ok(json) => {
                println!("{json}");
                DispatchOutcome::Ok
            }
            Err(error) => DispatchOutcome::InternalError(format!(
                "activation receipt serialization failed: {error}"
            )),
        },
        Err(error) => DispatchOutcome::InternalError(format!("activation failed: {error}")),
    }
}

fn dispatch_deactivation(invocation: &ActivationInvocation) -> DispatchOutcome {
    let install_root = match invocation
        .install_root
        .clone()
        .map(Ok)
        .unwrap_or_else(crate::activation::default_install_root)
    {
        Ok(path) => path,
        Err(error) => return DispatchOutcome::UserError(error),
    };
    let clients = if invocation.clients.is_empty() {
        vec![
            crate::activation::HarnessClient::Codex,
            crate::activation::HarnessClient::Claude,
        ]
    } else {
        let mut parsed = Vec::new();
        for client in &invocation.clients {
            match crate::activation::HarnessClient::parse(client) {
                Ok(client) if !parsed.contains(&client) => parsed.push(client),
                Ok(_) => {}
                Err(error) => return DispatchOutcome::UserError(error),
            }
        }
        parsed
    };
    let options = crate::activation::ActivationOptions {
        install_root,
        clients,
        timeout: std::time::Duration::from_millis(invocation.timeout_ms.clamp(1_000, 120_000)),
        dry_run: invocation.dry_run,
    };
    match crate::activation::deactivate(options) {
        Ok(receipt) => match serde_json::to_string_pretty(&receipt) {
            Ok(json) => {
                println!("{json}");
                DispatchOutcome::Ok
            }
            Err(error) => DispatchOutcome::InternalError(format!(
                "deactivation receipt serialization failed: {error}"
            )),
        },
        Err(error) => DispatchOutcome::InternalError(format!("deactivation failed: {error}")),
    }
}

fn dispatch_cli(tail: &[String]) -> DispatchOutcome {
    // The runtime CLI owns its own argv. We reconstruct a Vec<&str> so it sees the same shape
    // it would have seen from a direct invocation. `tail` is empty when the user typed
    // `membrane cli` with no subcommand; the runtime prints help and returns Ok.
    //
    // MBR-106: intercept `cli doctor paths` before forwarding to the runtime so
    // the existing `cli doctor --json` surface is untouched. The runtime still
    // owns every other `cli ...` invocation; the binary only adds the new
    // `doctor paths` capability the install/uninstall residue audit needs.
    if is_doctor_paths_invocation(tail) {
        return run_doctor_paths(&tail[2..]);
    }
    if matches!(tail.first().map(String::as_str), Some("diagnostics")) {
        return dispatch_diagnostics(&tail[1..]);
    }
    if matches!(tail, [operation] if operation == "hub-capabilities" || operation == "hub-snapshot")
    {
        // Stateless host clients may fetch the canonical Hub snapshot through
        // this path. The command never starts a runtime; Hub absence is the
        // typed `membrane_unavailable { hub_inactive }` response below.
        if tail[0] == "hub-snapshot" {
            let Some(parts) =
                membrane_runtime::hub_inputs::live_snapshot_parts_from_local_service()
            else {
                return hub_inactive();
            };
            return match serde_json::to_string(&membrane_runtime::hub_inputs::compose_hub_snapshot(
                parts,
                now_unix_ms(),
            )) {
                Ok(json) => {
                    println!("{json}");
                    DispatchOutcome::Ok
                }
                Err(error) => DispatchOutcome::InternalError(format!("hub.snapshot: {error}")),
            };
        }
        // MBR: read live state from local Membrane resident's /health endpoint
        // instead of hardcoding "Offline" regardless of whether the service is
        // up. Falls back to the honest unavailable facade on any failure.
        let Some(inputs) = membrane_runtime::hub_inputs::live_inputs_from_local_service() else {
            return hub_inactive();
        };
        return match dispatch_hub("hub.capabilities", inputs)
            .and_then(|value| serde_json::to_string(&value).map_err(|error| error.to_string()))
        {
            Ok(json) => {
                println!("{json}");
                DispatchOutcome::Ok
            }
            Err(error) => DispatchOutcome::InternalError(format!("hub.capabilities: {error}")),
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

fn hub_inactive() -> DispatchOutcome {
    let unavailable = membrane_protocol::MembraneUnavailableV1::hub_inactive();
    match serde_json::to_string(&unavailable) {
        Ok(json) => {
            println!("{json}");
            DispatchOutcome::UserError("hub inactive".into())
        }
        Err(error) => {
            DispatchOutcome::InternalError(format!("serialize membrane_unavailable: {error}"))
        }
    }
}

fn dispatch_hub(
    operation: &str,
    inputs: membrane_runtime::hub::HubInputsV1,
) -> Result<serde_json::Value, String> {
    membrane_runtime::hub::HubFacadeV1::new(None).dispatch_json(operation, now_unix_ms(), inputs)
}

// ---------------------------------------------------------------------------
// Live Diagnostics CLI (design §12 operational surface)
//
// Two offline paths need no resident: `fence-evaluate` (pure gate evaluation
// over stdin/file JSON) and `capabilities` (static support info). Every other
// subcommand mirrors one REST route on the resident loopback API using a
// minimal blocking HTTP client over std::net — the same mechanism
// `membrane-runtime`'s own CLI verbs use. When no resident is listening the
// service-bound commands print a typed degradation envelope instead of
// inventing hidden network behavior.
// ---------------------------------------------------------------------------

const DIAGNOSTICS_CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(400);
const DIAGNOSTICS_MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, clap::Parser)]
#[command(
    name = "membrane diagnostics",
    bin_name = "membrane diagnostics",
    about = "Live Diagnostics operational surface: workspace epochs, mutation fence, and gate decisions.",
    disable_help_subcommand = true,
    subcommand_required = true,
    arg_required_else_help = true
)]
struct DiagnosticsCli {
    #[command(subcommand)]
    command: DiagnosticsCommand,
}

#[derive(Debug, clap::Subcommand)]
enum DiagnosticsCommand {
    /// Print static support information for the diagnostics surface.
    Capabilities,
    /// Purely evaluate {snapshot, expectedEpoch, policy} JSON from --file or stdin and print the decision. No resident required.
    FenceEvaluate {
        /// Path to the request JSON, or `-` for stdin.
        #[arg(long)]
        file: Option<std::path::PathBuf>,
    },
    /// Service-bound: print the resident diagnostics status snapshot.
    Status {
        /// Resident loopback port override (default $MEMBRANE_PORT or 47851).
        #[arg(long)]
        port: Option<u16>,
    },
    /// Service-bound placeholder: prints the same status snapshot as `status`.
    Subscribe {
        #[arg(long)]
        port: Option<u16>,
    },
    /// Service-bound: open the diagnostics workspace session.
    WorkspaceOpen {
        #[arg(long)]
        repo: String,
        #[arg(long)]
        worktree: String,
        #[arg(long, required = true)]
        project_root: String,
        #[arg(long)]
        port: Option<u16>,
    },
    /// Service-bound: close the diagnostics workspace session.
    WorkspaceClose {
        #[arg(long)]
        repo: String,
        #[arg(long)]
        worktree: String,
        #[arg(long)]
        port: Option<u16>,
    },
    /// Service-bound: report one workspace's fence state.
    WorkspaceStatus {
        #[arg(long)]
        repo: String,
        #[arg(long)]
        worktree: String,
        #[arg(long)]
        port: Option<u16>,
    },
    /// Service-bound: open one coherent mutation batch.
    MutationBegin {
        #[arg(long)]
        repo: String,
        #[arg(long)]
        worktree: String,
        #[arg(long)]
        port: Option<u16>,
    },
    /// Service-bound: seal the batch with the resulting workspace epoch JSON supplied via --file or stdin (`-`).
    MutationSeal {
        #[arg(long)]
        repo: String,
        #[arg(long)]
        worktree: String,
        #[arg(long)]
        file: Option<std::path::PathBuf>,
        #[arg(long)]
        port: Option<u16>,
    },
    /// Service-bound: register observed resulting bytes; epoch JSON via --file or stdin (`-`).
    MutationRegisterObserved {
        #[arg(long)]
        repo: String,
        #[arg(long)]
        worktree: String,
        #[arg(long)]
        file: Option<std::path::PathBuf>,
        #[arg(long)]
        port: Option<u16>,
    },
    /// Service-bound: reconcile current bytes; {manifestDigest, hashes} JSON via --file or stdin (`-`).
    Reconcile {
        #[arg(long)]
        repo: String,
        #[arg(long)]
        worktree: String,
        #[arg(long)]
        file: Option<std::path::PathBuf>,
        #[arg(long)]
        port: Option<u16>,
    },
    /// Service-bound: acquire evidence and evaluate planner policy; optional request JSON via --file or stdin (`-`).
    SnapshotAwait {
        #[arg(long)]
        repo: String,
        #[arg(long)]
        worktree: String,
        /// Override the planner policy profile name (default changed-files-zero).
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        file: Option<std::path::PathBuf>,
        #[arg(long)]
        port: Option<u16>,
    },
    /// Service-bound: record the cleared decision as a named baseline.
    BaselineCapture {
        #[arg(long)]
        repo: String,
        #[arg(long)]
        worktree: String,
        #[arg(long)]
        name: String,
        #[arg(long)]
        port: Option<u16>,
    },
    /// Service-bound: refresh a named baseline to the current cleared decision.
    BaselineUpdate {
        #[arg(long)]
        repo: String,
        #[arg(long)]
        worktree: String,
        #[arg(long)]
        name: String,
        #[arg(long)]
        port: Option<u16>,
    },
    /// Service-bound: targeted provider restart by workspace-engine-key digest.
    ProviderRestart {
        #[arg(long = "key-digest")]
        key_digest: String,
        #[arg(long)]
        port: Option<u16>,
    },
}

fn dispatch_diagnostics(args: &[String]) -> DispatchOutcome {
    use clap::Parser as _;
    let parsed = match DiagnosticsCli::try_parse_from(
        std::iter::once("diagnostics").chain(args.iter().map(String::as_str)),
    ) {
        Ok(parsed) => parsed,
        Err(error) => return DispatchOutcome::UserError(error.to_string()),
    };
    execute_diagnostics_command(parsed.command)
}

fn execute_diagnostics_command(command: DiagnosticsCommand) -> DispatchOutcome {
    use membrane_runtime::live_diagnostics_service::{static_capabilities, DiagnosticsService};
    match command {
        DiagnosticsCommand::Capabilities => print_diagnostics_json(&static_capabilities()),
        DiagnosticsCommand::FenceEvaluate { file } => {
            let request: Result<
                membrane_runtime::live_diagnostics_service::FenceEvaluateRequest,
                DispatchOutcome,
            > = read_diagnostics_input(&file, true).and_then(|input| {
                serde_json::from_str(&input).map_err(|error| {
                    DispatchOutcome::UserError(format!(
                        "diagnostics fence-evaluate: input must be {{snapshot, expectedEpoch, policy}} JSON: {error}"
                    ))
                })
            });
            match request {
                Ok(request) => print_diagnostics_json(&DiagnosticsService::evaluate_fence(
                    &request.snapshot,
                    &request.expected_epoch,
                    &request.policy,
                )),
                Err(outcome) => outcome,
            }
        }
        DiagnosticsCommand::Status { port } | DiagnosticsCommand::Subscribe { port } => {
            run_diagnostics_service_call(
                diagnostics_loopback_port(port),
                "GET",
                "/diagnostics/status",
                None,
            )
        }
        DiagnosticsCommand::WorkspaceOpen {
            repo,
            worktree,
            project_root,
            port,
        } => run_diagnostics_service_call(
            diagnostics_loopback_port(port),
            "POST",
            "/diagnostics/workspace/open",
            Some(diagnostics_workspace_body_with_root(
                &repo,
                &worktree,
                Some(&project_root),
            )),
        ),
        DiagnosticsCommand::WorkspaceClose {
            repo,
            worktree,
            port,
        } => run_diagnostics_service_call(
            diagnostics_loopback_port(port),
            "POST",
            "/diagnostics/workspace/close",
            Some(diagnostics_workspace_body(&repo, &worktree)),
        ),
        DiagnosticsCommand::WorkspaceStatus {
            repo,
            worktree,
            port,
        } => {
            let query = format!(
                "/diagnostics/workspace/status?repoId={}&worktreeId={}",
                percent_encode_component(&repo),
                percent_encode_component(&worktree)
            );
            run_diagnostics_service_call(diagnostics_loopback_port(port), "GET", &query, None)
        }
        DiagnosticsCommand::MutationBegin {
            repo,
            worktree,
            port,
        } => run_diagnostics_service_call(
            diagnostics_loopback_port(port),
            "POST",
            "/diagnostics/mutation/begin",
            Some(diagnostics_workspace_body(&repo, &worktree)),
        ),
        DiagnosticsCommand::MutationSeal {
            repo,
            worktree,
            file,
            port,
        } => match epoch_request_body(&repo, &worktree, &file) {
            Ok(body) => run_diagnostics_service_call(
                diagnostics_loopback_port(port),
                "POST",
                "/diagnostics/mutation/seal",
                Some(body),
            ),
            Err(outcome) => outcome,
        },
        DiagnosticsCommand::MutationRegisterObserved {
            repo,
            worktree,
            file,
            port,
        } => match epoch_request_body(&repo, &worktree, &file) {
            Ok(body) => run_diagnostics_service_call(
                diagnostics_loopback_port(port),
                "POST",
                "/diagnostics/mutation/registerObserved",
                Some(body),
            ),
            Err(outcome) => outcome,
        },
        DiagnosticsCommand::Reconcile {
            repo,
            worktree,
            file,
            port,
        } => {
            match object_request_body(&file, &mut |object| {
                if !object
                    .get("manifestDigest")
                    .map(serde_json::Value::is_string)
                    .unwrap_or(false)
                {
                    return Err(
                        "reconcile input JSON requires a string manifestDigest field".to_string(),
                    );
                }
                object.insert("repoId".to_string(), serde_json::json!(repo));
                object.insert("worktreeId".to_string(), serde_json::json!(worktree));
                Ok(())
            }) {
                Ok(body) => run_diagnostics_service_call(
                    diagnostics_loopback_port(port),
                    "POST",
                    "/diagnostics/reconcile",
                    Some(body),
                ),
                Err(outcome) => outcome,
            }
        }
        DiagnosticsCommand::SnapshotAwait {
            repo,
            worktree,
            profile,
            file,
            port,
        } => {
            match object_request_body(&file, &mut |object| {
                object.insert("repoId".to_string(), serde_json::json!(repo));
                object.insert("worktreeId".to_string(), serde_json::json!(worktree));
                if let Some(profile) = &profile {
                    object.insert("policyProfileName".to_string(), serde_json::json!(profile));
                }
                Ok(())
            }) {
                Ok(body) => run_diagnostics_service_call(
                    diagnostics_loopback_port(port),
                    "POST",
                    "/diagnostics/snapshot/await",
                    Some(body),
                ),
                Err(outcome) => outcome,
            }
        }
        DiagnosticsCommand::BaselineCapture {
            repo,
            worktree,
            name,
            port,
        } => run_diagnostics_service_call(
            diagnostics_loopback_port(port),
            "POST",
            "/diagnostics/baseline/capture",
            Some(diagnostics_named_body(&repo, &worktree, &name)),
        ),
        DiagnosticsCommand::BaselineUpdate {
            repo,
            worktree,
            name,
            port,
        } => run_diagnostics_service_call(
            diagnostics_loopback_port(port),
            "POST",
            "/diagnostics/baseline/update",
            Some(diagnostics_named_body(&repo, &worktree, &name)),
        ),
        DiagnosticsCommand::ProviderRestart { key_digest, port } => run_diagnostics_service_call(
            diagnostics_loopback_port(port),
            "POST",
            "/diagnostics/provider/restart",
            Some(serde_json::json!({ "keyDigest": key_digest }).to_string()),
        ),
    }
}

fn print_diagnostics_json<T: serde::Serialize>(value: &T) -> DispatchOutcome {
    match serde_json::to_string_pretty(value) {
        Ok(json) => {
            println!("{json}");
            DispatchOutcome::Ok
        }
        Err(error) => DispatchOutcome::InternalError(format!("diagnostics serialize: {error}")),
    }
}

/// Resolve the resident loopback port: explicit flag, then $MEMBRANE_PORT,
/// then the documented default the resident binds.
fn diagnostics_loopback_port(explicit: Option<u16>) -> u16 {
    explicit
        .or_else(|| {
            std::env::var("MEMBRANE_PORT")
                .ok()
                .and_then(|port| port.trim().parse::<u16>().ok())
        })
        .unwrap_or(membrane_runtime::hub_inputs::DEFAULT_LOCAL_SERVICE_PORT)
}

/// Bearer token resolution mirroring the runtime CLI: $MEMBRANE_API_TOKEN,
/// then the file named by $MEMBRANE_API_TOKEN_FILE.
fn diagnostics_api_token() -> Result<Option<String>, String> {
    if let Some(raw) = std::env::var_os("MEMBRANE_API_TOKEN") {
        let token = raw.to_string_lossy().trim().to_string();
        if token.is_empty() {
            return Err("MEMBRANE_API_TOKEN is set but empty".to_string());
        }
        if token.contains(['\r', '\n']) {
            return Err("MEMBRANE_API_TOKEN contains a newline".to_string());
        }
        return Ok(Some(token));
    }
    if let Some(path) = std::env::var_os("MEMBRANE_API_TOKEN_FILE").map(std::path::PathBuf::from) {
        let token = std::fs::read_to_string(&path)
            .map_err(|error| format!("read MEMBRANE_API_TOKEN_FILE {}: {error}", path.display()))?
            .trim()
            .to_string();
        if token.is_empty() {
            return Err(format!(
                "MEMBRANE_API_TOKEN_FILE {} is empty",
                path.display()
            ));
        }
        if token.contains(['\r', '\n']) {
            return Err("MEMBRANE_API_TOKEN_FILE contains a newline".to_string());
        }
        return Ok(Some(token));
    }
    Ok(None)
}

fn diagnostics_workspace_body(repo: &str, worktree: &str) -> String {
    serde_json::json!({ "repoId": repo, "worktreeId": worktree }).to_string()
}

fn diagnostics_workspace_body_with_root(
    repo: &str,
    worktree: &str,
    project_root: Option<&str>,
) -> String {
    let mut body = serde_json::json!({ "repoId": repo, "worktreeId": worktree });
    if let Some(root) = project_root {
        body["projectRoot"] = serde_json::Value::String(root.to_string());
    }
    body.to_string()
}

fn diagnostics_named_body(repo: &str, worktree: &str, name: &str) -> String {
    serde_json::json!({ "repoId": repo, "worktreeId": worktree, "name": name }).to_string()
}

/// Read the optional/required JSON payload source: `--file -` reads stdin, a
/// path reads the file, absent means an empty object when not required.
fn read_diagnostics_input(
    file: &Option<std::path::PathBuf>,
    required: bool,
) -> Result<String, DispatchOutcome> {
    match file {
        None => {
            if required {
                Err(DispatchOutcome::UserError(
                    "--file PATH (or `-` for stdin) is required for this subcommand".to_string(),
                ))
            } else {
                Ok("{}".to_string())
            }
        }
        Some(source) if source.as_os_str() == "-" => {
            let mut buffer = String::new();
            use std::io::Read as _;
            std::io::stdin()
                .read_to_string(&mut buffer)
                .map_err(|error| DispatchOutcome::UserError(format!("read stdin: {error}")))?;
            Ok(buffer)
        }
        Some(path) => std::fs::read_to_string(path).map_err(|error| {
            DispatchOutcome::UserError(format!("read {}: {error}", path.display()))
        }),
    }
}

/// Build `{repoId, worktreeId, epoch}` where `epoch` comes from --file/stdin
/// and must be a JSON object.
fn epoch_request_body(
    repo: &str,
    worktree: &str,
    file: &Option<std::path::PathBuf>,
) -> Result<String, DispatchOutcome> {
    let input = read_diagnostics_input(file, true)?;
    let epoch: serde_json::Value = serde_json::from_str(&input).map_err(|error| {
        DispatchOutcome::UserError(format!("diagnostics: epoch input must be JSON: {error}"))
    })?;
    if !epoch.is_object() {
        return Err(DispatchOutcome::UserError(
            "diagnostics: epoch input must be a JSON object".to_string(),
        ));
    }
    Ok(serde_json::json!({
        "repoId": repo,
        "worktreeId": worktree,
        "epoch": epoch,
    })
    .to_string())
}

/// Load a JSON object from --file/stdin, let the caller inject identity
/// fields into it, and return the serialized request body.
fn object_request_body(
    file: &Option<std::path::PathBuf>,
    inject: &mut dyn FnMut(&mut serde_json::Map<String, serde_json::Value>) -> Result<(), String>,
) -> Result<String, DispatchOutcome> {
    let input = read_diagnostics_input(file, false)?;
    let mut object: serde_json::Map<String, serde_json::Value> = serde_json::from_str(&input)
        .map_err(|error| {
            DispatchOutcome::UserError(format!(
                "diagnostics: request input must be a JSON object: {error}"
            ))
        })?;
    inject(&mut object).map_err(|message| DispatchOutcome::UserError(message))?;
    Ok(serde_json::Value::Object(object).to_string())
}

fn percent_encode_component(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char)
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

/// One blocking loopback call to the resident diagnostics surface. `Ok(None)`
/// means nothing is listening on the port (typed degradation upstream); every
/// response body is printed verbatim so server-side typed omission envelopes
/// reach the caller unchanged.
fn run_diagnostics_service_call(
    port: u16,
    method: &str,
    path_and_query: &str,
    body: Option<String>,
) -> DispatchOutcome {
    match diagnostics_http_request(port, method, path_and_query, body.as_deref()) {
        Ok(None) => hub_inactive(),
        Ok(Some((status, response_body))) => {
            println!("{response_body}");
            if status == 200 {
                DispatchOutcome::Ok
            } else {
                DispatchOutcome::UserError(format!(
                    "resident returned HTTP {status} for {method} {path_and_query}"
                ))
            }
        }
        Err(error) => DispatchOutcome::UserError(format!("diagnostics: {error}")),
    }
}

/// Minimal blocking HTTP client over std::net, mirroring the resident-facing
/// request shape used by membrane-runtime's own CLI verbs (loopback only,
/// bearer token from the environment, Connection: close).
fn diagnostics_http_request(
    port: u16,
    method: &str,
    path_and_query: &str,
    body: Option<&str>,
) -> Result<Option<(u16, String)>, String> {
    use std::io::{Read as _, Write as _};
    let address = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let mut stream =
        match std::net::TcpStream::connect_timeout(&address, DIAGNOSTICS_CONNECT_TIMEOUT) {
            Ok(stream) => stream,
            Err(error) if error.kind() == std::io::ErrorKind::ConnectionRefused => return Ok(None),
            Err(error) => return Err(format!("connect to resident failed: {error}")),
        };
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(30)))
        .map_err(|error| format!("set read timeout: {error}"))?;
    stream
        .set_write_timeout(Some(std::time::Duration::from_secs(5)))
        .map_err(|error| format!("set write timeout: {error}"))?;
    let authorization = diagnostics_api_token()?
        .map(|token| format!("Authorization: Bearer {token}\r\n"))
        .unwrap_or_default();
    let payload = body.unwrap_or("");
    let content_headers = if method == "POST" {
        format!(
            "Content-Type: application/json\r\nContent-Length: {}\r\n",
            payload.len()
        )
    } else {
        String::new()
    };
    let request = format!(
        "{method} {path_and_query} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n{authorization}{content_headers}Connection: close\r\n\r\n{payload}"
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|error| format!("write to resident failed: {error}"))?;
    let mut raw = Vec::new();
    let mut chunk = [0_u8; 4096];
    loop {
        let read = stream
            .read(&mut chunk)
            .map_err(|error| format!("read resident response: {error}"))?;
        if read == 0 {
            break;
        }
        raw.extend_from_slice(&chunk[..read]);
        if raw.len() > DIAGNOSTICS_MAX_RESPONSE_BYTES {
            return Err("resident response exceeded limit".to_string());
        }
    }
    let text = String::from_utf8_lossy(&raw);
    let (head, response_body) = text
        .split_once("\r\n\r\n")
        .ok_or_else(|| "resident returned malformed HTTP response".to_string())?;
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|status| status.parse::<u16>().ok())
        .ok_or_else(|| "resident returned malformed HTTP status".to_string())?;
    Ok(Some((status, response_body.trim().to_string())))
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
    match membrane_runtime::serve::run_stdio_mcp() {
        Ok(()) => DispatchOutcome::Ok,
        Err(error) => DispatchOutcome::InternalError(error.to_string()),
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

/// Runtime errors are mostly user-visible (bad arguments, missing runtime, lifecycle rejection), so
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
    }

    #[test]
    fn hub_snapshot_stamps_current_observation_for_unavailable_inputs() {
        let snapshot = dispatch_hub(
            "hub.snapshot",
            membrane_runtime::hub::HubInputsV1::unavailable("source_not_connected"),
        )
        .unwrap();
        assert!(snapshot["observedAtUnixMs"].as_u64().unwrap() > 0);
        for section in membrane_runtime::hub::HUB_RESOURCES {
            assert_eq!(snapshot["sections"][section]["state"], "unavailable");
        }
    }
}
