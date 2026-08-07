//! Mode subcommand parsing. The binary exposes exactly four modes — one of them is what the
//! user (or a launcher) wants — so the dispatcher can refuse everything else early.
//!
//! MBR-102: create one membrane executable with mode subcommands.

use clap::{Parser, Subcommand};
use std::ffi::OsString;

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
}

impl MembraneMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            MembraneMode::Cli => "cli",
            MembraneMode::StdioMcp => "stdio-mcp",
            MembraneMode::LoopbackApi => "loopback-api",
            MembraneMode::SupervisorChild => "supervisor-child",
            MembraneMode::Install => "install",
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

pub fn parse_mode<I, T>(args: I) -> Result<ParsedInvocation, String>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    // `clap` insists on consuming argv; we rebuild a Vec so error messages stay clean.
    let collected: Vec<OsString> = args.into_iter().map(Into::into).collect();
    let parsed = Cli::try_parse_from(collected.iter()).map_err(|err| err.to_string())?;
    let invocation = match parsed.command {
        Command::Cli(args) => ParsedInvocation {
            mode: MembraneMode::Cli,
            cli_tail: args.passthrough,
            framing: String::new(),
            port: 0,
            lease: None,
            install: None,
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
            }
        }
        Command::SupervisorChild(args) => ParsedInvocation {
            mode: MembraneMode::SupervisorChild,
            cli_tail: Vec::new(),
            framing: String::new(),
            port: 0,
            lease: args.lease,
            install: None,
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
        let err =
            parse_mode(["membrane", "stdio-mcp", "--framing", "lsp"].iter().copied()).unwrap_err();
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
        let err = parse_mode(["membrane", "loopback-api", "--port", "80"].iter().copied())
            .unwrap_err();
        assert!(err.contains("port 80 is privileged"));
    }

    #[test]
    fn supervisor_child_accepts_lease_path() {
        let inv =
            parse_mode(["membrane", "supervisor-child", "--lease", "/var/run/lease"].iter().copied())
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
