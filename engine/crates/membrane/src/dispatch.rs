//! Mode subcommand parsing. The binary exposes exactly four modes — one of them is what the
//! user (or a launcher) wants — so the dispatcher can refuse everything else early.
//!
//! MBR-102: create one membrane executable with mode subcommands.

use crate::generate_cli_subcommands;
use clap::{Arg, Command as ClapCommand, Parser, Subcommand};
use std::ffi::OsString;

const GENERATED_CLI_SUBCOMMANDS: &str = include_str!("generated_cli_subcommands.rs");

const fn const_starts_with(value: &str, prefix: &str) -> bool {
    let value = value.as_bytes();
    let prefix = prefix.as_bytes();
    if value.len() < prefix.len() {
        return false;
    }
    let mut index = 0;
    while index < prefix.len() {
        if value[index] != prefix[index] {
            return false;
        }
        index += 1;
    }
    true
}

const _: () = {
    let _generator: fn(&[membrane_protocol::operations::OperationSpec]) -> String =
        generate_cli_subcommands;
    assert!(const_starts_with(
        GENERATED_CLI_SUBCOMMANDS,
        concat!(
            "// GENERATED — DO NOT EDIT\n// operation_registry_version: ",
            env!("MEMBRANE_OPERATION_REGISTRY_VERSION")
        )
    ));
};

/// The four product-facing modes. Anything else is rejected before reaching the dispatcher.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MembraneMode {
    /// One-shot CLI subcommands: doctor, smoke, ingest, query, etc. Returns when the work
    /// completes.
    Cli,
    /// JSON-RPC over stdio. Used by Claude/Codex/Cursor/Windsurf MCP clients.
    StdioMcp,
    /// Long-running HTTP service bound to 127.0.0.1 only. Loopback API for the Hub and CLI.
    LoopbackApi,
    /// The supervisor's resident child: owns the engine DB and accepts lease tokens from
    /// `LoopbackApi` clients.
    SupervisorChild,
    /// MBR-203: transactional install. Runs an install plan against a scratch
    /// `MEMBRANE_ROOT` and only on `commit` renames the scratch root to the
    /// target root. See `crate::install_tx` for the contract.
    Install,
    /// MBR-205: ownership-safe uninstall. Loads the ownership table from
    /// `--receipt-root`, filters `--candidate` paths through
    /// `revoke_unowned`, and only removes the ones the runtime
    /// registered. See `crate::uninstall` for the contract.
    Uninstall,
}

impl MembraneMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            MembraneMode::Cli => "cli",
            MembraneMode::StdioMcp => "stdio-mcp",
            MembraneMode::LoopbackApi => "loopback-api",
            MembraneMode::SupervisorChild => "supervisor-child",
            MembraneMode::Install => "install",
            MembraneMode::Uninstall => "uninstall",
        }
    }
}

#[derive(Debug, Parser)]
#[command(
    name = "membrane",
    bin_name = "membrane",
    about = "Membrane — one signed binary for CLI, stdio MCP, loopback API, and supervisor-child modes.",
    version,
    disable_help_subcommand = true
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// One-shot CLI subcommands: doctor, smoke, ingest, query, etc.
    Cli(CliArgs),
    /// JSON-RPC over stdio for MCP clients.
    StdioMcp(StdioArgs),
    /// HTTP service bound to 127.0.0.1 only.
    LoopbackApi(LoopbackArgs),
    /// Resident child process owned by the per-user supervisor.
    SupervisorChild(SupervisorArgs),
    /// MBR-203: transactional install against a scratch `MEMBRANE_ROOT`.
    Install(InstallArgs),
    /// MBR-205: ownership-safe uninstall. The default plan is to refuse
    /// every path the operator names as a `--candidate` unless the
    /// ownership table at `--receipt-root` records it.
    Uninstall(UninstallArgs),
}

#[derive(Debug, clap::Args)]
struct CliArgs {
    /// Forwarded verbatim to the membrane runtime CLI. Reserved for the runtime; the binary
    /// passes argv[2..] through unchanged.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    passthrough: Vec<String>,
}

#[derive(Debug, clap::Args)]
struct StdioArgs {
    /// Reserved for explicit framing overrides (e.g. LSP headers). The default is line-delimited
    /// JSON-RPC, which is what every MCP client expects today.
    #[arg(long, default_value = "jsonl")]
    framing: String,
}

#[derive(Debug, clap::Args)]
struct LoopbackArgs {
    /// Port for the loopback API. The dispatcher only validates the port range; the runtime
    /// binds it. Must be ≥ 1024 to keep the supervisor's lease authority unprivileged.
    #[arg(long, default_value_t = 47851)]
    port: u16,
}

#[derive(Debug, clap::Args)]
struct SupervisorArgs {
    /// Path to the supervisor lease file handed to the resident child at start. The runtime
    /// reads this and refuses to serve if the lease is missing, stale, or signed by a different
    /// build.
    #[arg(long)]
    lease: Option<std::path::PathBuf>,
}

/// MBR-203: install subcommand arguments. The binary accepts an optional
/// `--plan` JSON; when omitted, it executes a default plan with the five
/// standard stages and `true` actions so the operator can hand-edit the JSON
/// to populate the real work. `--dry-run` runs the plan against the scratch
/// root and prints the receipt without renaming scratch to target.
#[derive(Debug, clap::Args)]
struct InstallArgs {
    /// Scratch `MEMBRANE_ROOT` — the install plan runs against this root first.
    /// Nothing in the target root is touched until `commit`.
    #[arg(long)]
    scratch_root: std::path::PathBuf,
    /// Target `MEMBRANE_ROOT` — the scratch root is atomically renamed to
    /// this path on `commit`.
    #[arg(long)]
    target_root: std::path::PathBuf,
    /// Path to a JSON file describing the install plan. When omitted the
    /// binary executes a default plan with the five standard stages and
    /// no-op actions.
    #[arg(long)]
    plan: Option<std::path::PathBuf>,
    /// Run the plan end-to-end against the scratch root and emit a receipt
    /// without renaming the scratch root to the target root.
    #[arg(long, default_value_t = false)]
    dry_run: bool,
}

/// MBR-205: uninstall subcommand arguments. The binary reads the
/// ownership table from `--receipt-root`, filters `--candidate` paths
/// through `revoke_unowned`, and only removes the ones the runtime
/// registered. `--dry-run` prints the authorised set without removing
/// anything.
#[derive(Debug, clap::Args)]
struct UninstallArgs {
    /// Receipt root — the directory `install-receipt.json` and
    /// `ownership.json` live under. The ownership table is loaded from
    /// `<receipt-root>/ownership.json`; when the file is missing the
    /// uninstall refuses everything and exits `RefusedAll`.
    #[arg(long)]
    receipt_root: std::path::PathBuf,
    /// Path the operator wants to remove. May be supplied multiple
    /// times. Anything not in the ownership table is refused.
    #[arg(long, value_name = "PATH")]
    candidate: Vec<std::path::PathBuf>,
    /// Print the authorised set and exit 0 without removing anything.
    #[arg(long, default_value_t = false)]
    dry_run: bool,
}

/// Fully-parsed invocation handed to the dispatcher. The dispatcher never touches argv again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedInvocation {
    pub mode: MembraneMode,
    /// For `Cli` mode, the forwarded argv tail. Empty for the other modes.
    pub cli_tail: Vec<String>,
    /// For `StdioMcp`, the framing override (always "jsonl" today).
    pub framing: String,
    /// For `LoopbackApi`, the port to bind.
    pub port: u16,
    /// For `SupervisorChild`, the lease path if the supervisor provided one.
    pub lease: Option<std::path::PathBuf>,
    /// MBR-203: for `Install` mode, the scratch root, target root, optional
    /// plan path, and dry-run flag. `None` for every other mode.
    pub install: Option<InstallInvocation>,
    /// MBR-205: for `Uninstall` mode, the receipt root, the candidate
    /// paths the operator asked about, and the dry-run flag. `None` for
    /// every other mode.
    pub uninstall: Option<UninstallInvocation>,
}

/// MBR-203: install invocation handed to the dispatcher's install handler.
/// Lives next to [`ParsedInvocation`] so the CLI argument list stays in one
/// place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallInvocation {
    pub scratch_root: std::path::PathBuf,
    pub target_root: std::path::PathBuf,
    pub plan: Option<std::path::PathBuf>,
    pub dry_run: bool,
}

/// MBR-205: uninstall invocation handed to the dispatcher's uninstall
/// handler. The handler loads the ownership table from `receipt_root`,
/// filters `candidates` through `revoke_unowned`, and either prints
/// the authorised set (dry-run) or removes them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UninstallInvocation {
    pub receipt_root: std::path::PathBuf,
    pub candidates: Vec<std::path::PathBuf>,
    pub dry_run: bool,
}

/// Hardcoded clap projection checked against the operation registry by the
/// `cli-parity` binary. The empty query returns the subcommand inventory.
pub fn cli_parser_snapshot(name: &str) -> Option<Vec<(String, String)>> {
    include!("generated_cli_subcommands.rs")
}

fn parse_registry_operation(args: &[OsString]) -> Option<Result<ParsedInvocation, String>> {
    let requested = args.get(1)?.to_str()?;
    let spec = membrane_protocol::operations::OPERATIONS
        .iter()
        .find(|spec| crate::cli_parity::cli_subcommand_name(spec.id) == requested)?;
    let command_name: &'static str = Box::leak(requested.to_string().into_boxed_str());
    let mut operation = ClapCommand::new(command_name).about(spec.help);
    for parameter in spec.parameters {
        let mut arg = Arg::new(parameter.name)
            .long(parameter.name)
            .help(parameter.help)
            .num_args(1);
        if let Some(default) = parameter.default {
            arg = arg.default_value(default);
        }
        operation = operation.arg(arg);
    }
    let parser = ClapCommand::new("membrane")
        .disable_help_subcommand(true)
        .subcommand_required(true)
        .subcommand(operation);
    Some(
        parser
            .try_get_matches_from(args.iter())
            .map(|_| ParsedInvocation {
                mode: MembraneMode::Cli,
                cli_tail: args
                    .iter()
                    .skip(1)
                    .map(|arg| arg.to_string_lossy().into_owned())
                    .collect(),
                framing: String::new(),
                port: 0,
                lease: None,
                install: None,
                uninstall: None,
            })
            .map_err(|error| error.to_string()),
    )
}

pub fn parse_mode<I, T>(args: I) -> Result<ParsedInvocation, String>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    // `clap` insists on consuming argv; we rebuild a Vec so error messages stay clean.
    let collected: Vec<OsString> = args.into_iter().map(Into::into).collect();
    if let Some(operation) = parse_registry_operation(&collected) {
        return operation;
    }
    let parsed = Cli::try_parse_from(collected.iter()).map_err(|err| err.to_string())?;
    let invocation = match parsed.command {
        Command::Cli(args) => ParsedInvocation {
            mode: MembraneMode::Cli,
            cli_tail: args.passthrough,
            framing: String::new(),
            port: 0,
            lease: None,
            install: None,
            uninstall: None,
        },
        Command::StdioMcp(args) => {
            if args.framing != "jsonl" {
                return Err(format!(
                    "unsupported stdio-mcp framing '{}' (only 'jsonl' is accepted)",
                    args.framing
                ));
            }
            ParsedInvocation {
                mode: MembraneMode::StdioMcp,
                cli_tail: Vec::new(),
                framing: args.framing,
                port: 0,
                lease: None,
                install: None,
                uninstall: None,
            }
        }
        Command::LoopbackApi(args) => {
            if args.port < 1024 {
                return Err(format!(
                    "loopback-api port {} is privileged; the supervisor refuses ports < 1024",
                    args.port
                ));
            }
            ParsedInvocation {
                mode: MembraneMode::LoopbackApi,
                cli_tail: Vec::new(),
                framing: String::new(),
                port: args.port,
                lease: None,
                install: None,
                uninstall: None,
            }
        }
        Command::SupervisorChild(args) => ParsedInvocation {
            mode: MembraneMode::SupervisorChild,
            cli_tail: Vec::new(),
            framing: String::new(),
            port: 0,
            lease: args.lease,
            install: None,
            uninstall: None,
        },
        Command::Install(args) => ParsedInvocation {
            mode: MembraneMode::Install,
            cli_tail: Vec::new(),
            framing: String::new(),
            port: 0,
            lease: None,
            install: Some(InstallInvocation {
                scratch_root: args.scratch_root,
                target_root: args.target_root,
                plan: args.plan,
                dry_run: args.dry_run,
            }),
            uninstall: None,
        },
        Command::Uninstall(args) => ParsedInvocation {
            mode: MembraneMode::Uninstall,
            cli_tail: Vec::new(),
            framing: String::new(),
            port: 0,
            lease: None,
            install: None,
            uninstall: Some(UninstallInvocation {
                receipt_root: args.receipt_root,
                candidates: args.candidate,
                dry_run: args.dry_run,
            }),
        },
    };
    Ok(invocation)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_mode_accepts_forwarded_tail() {
        let inv = parse_mode(["membrane", "cli", "doctor", "--strict"].iter().copied()).unwrap();
        assert_eq!(inv.mode, MembraneMode::Cli);
        assert_eq!(inv.cli_tail, vec!["doctor", "--strict"]);
    }

    #[test]
    fn stdio_mcp_defaults_to_jsonl() {
        let inv = parse_mode(["membrane", "stdio-mcp"].iter().copied()).unwrap();
        assert_eq!(inv.mode, MembraneMode::StdioMcp);
        assert_eq!(inv.framing, "jsonl");
    }

    #[test]
    fn stdio_mcp_rejects_unknown_framing() {
        let err = parse_mode(
            ["membrane", "stdio-mcp", "--framing", "lsp"]
                .iter()
                .copied(),
        )
        .unwrap_err();
        assert!(err.contains("unsupported stdio-mcp framing"));
    }

    #[test]
    fn loopback_api_defaults_to_supervisor_port() {
        let inv = parse_mode(["membrane", "loopback-api"].iter().copied()).unwrap();
        assert_eq!(inv.mode, MembraneMode::LoopbackApi);
        assert_eq!(inv.port, 47851);
    }

    #[test]
    fn loopback_api_rejects_privileged_port() {
        let err =
            parse_mode(["membrane", "loopback-api", "--port", "80"].iter().copied()).unwrap_err();
        assert!(err.contains("port 80 is privileged"));
    }

    #[test]
    fn supervisor_child_accepts_lease_path() {
        let inv = parse_mode(
            ["membrane", "supervisor-child", "--lease", "/var/run/lease"]
                .iter()
                .copied(),
        )
        .unwrap();
        assert_eq!(inv.mode, MembraneMode::SupervisorChild);
        assert_eq!(
            inv.lease.as_deref().unwrap(),
            std::path::Path::new("/var/run/lease")
        );
    }

    #[test]
    fn unknown_mode_is_rejected() {
        let err = parse_mode(["membrane", "wat"].iter().copied()).unwrap_err();
        assert!(err.contains("unrecognized subcommand") || err.contains("invalid subcommand"));
    }

    #[test]
    fn no_mode_is_rejected() {
        let err = parse_mode(["membrane"].iter().copied()).unwrap_err();
        assert!(!err.is_empty());
    }
}
