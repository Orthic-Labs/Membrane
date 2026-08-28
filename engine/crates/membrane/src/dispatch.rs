//! Stateless client and product-maintenance subcommand parsing.
//!
//! MBR-102: create one membrane executable with mode subcommands.

use crate::generate_cli_subcommands;
use clap::{Arg, Command as ClapCommand, Parser, Subcommand};
use std::ffi::OsString;

use membrane_protocol::{ResidentHelloV1 as LifecycleHelloV1, ResidentLifecycleFrameV1};

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

/// Product-facing modes. Resident runtime modes are intentionally absent: the
/// runtime exists only inside the active Hub process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MembraneMode {
    /// One-shot CLI subcommands: doctor, smoke, ingest, query, etc. Returns when the work
    /// completes.
    Cli,
    /// JSON-RPC over stdio. Used by Claude/Codex/Cursor/Windsurf MCP clients.
    StdioMcp,
    /// MBR-203: transactional install. Runs an install plan against a scratch
    /// `MEMBRANE_ROOT` and only on `commit` renames the scratch root to the
    /// target root. See `crate::install_tx` for the contract.
    Install,
    /// MBR-205: ownership-safe uninstall. Loads the ownership table from
    /// `--receipt-root`, filters `--candidate` paths through
    /// `revoke_unowned`, and only removes the ones the runtime
    /// registered. See `crate::uninstall` for the contract.
    Uninstall,
    /// Activate installed tray/daemon, prove resident identity, & enroll
    /// native harnesses against installed `membrane stdio-mcp`.
    Activate,
    MigrateLegacy,
}

impl MembraneMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            MembraneMode::Cli => "cli",
            MembraneMode::StdioMcp => "stdio-mcp",
            MembraneMode::Install => "install",
            MembraneMode::Uninstall => "uninstall",
            MembraneMode::Activate => "activate",
            MembraneMode::MigrateLegacy => "migrate-legacy",
        }
    }
}

#[derive(Debug, Parser)]
#[command(
    name = "membrane",
    bin_name = "membrane",
    about = "Membrane — stateless clients and product maintenance for the Hub-hosted runtime.",
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
    /// MBR-203: transactional install against a scratch `MEMBRANE_ROOT`.
    Install(InstallArgs),
    /// MBR-205: ownership-safe uninstall. The default plan is to refuse
    /// every path the operator names as a `--candidate` unless the
    /// ownership table at `--receipt-root` records it.
    Uninstall(UninstallArgs),
    /// Activate installed app & register native MCP harnesses.
    Activate(ActivateArgs),
    /// Inspect installed service, release identity, & native harness projections.
    Status(ActivateArgs),
    /// MBR-210: move recognized legacy state without copying or starting a daemon.
    MigrateLegacy(MigrateLegacyArgs),
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

#[derive(Debug, clap::Args)]
struct ActivateArgs {
    /// Installed stable `current` directory. Defaults to the user-local
    /// product root's stable current path.
    #[arg(long)]
    install_root: Option<std::path::PathBuf>,
    /// Native harness to register. Repeatable; defaults to Codex plus Claude.
    #[arg(long, value_name = "codex|claude")]
    client: Vec<String>,
    /// Bounded resident readiness deadline.
    #[arg(long, default_value_t = 35_000)]
    timeout_ms: u64,
    /// Inspect activation without launching or mutating harness configuration.
    #[arg(long, default_value_t = false)]
    dry_run: bool,
}

#[derive(Debug, clap::Args)]
struct MigrateLegacyArgs {
    #[arg(long)]
    legacy_root: std::path::PathBuf,
    #[arg(long)]
    target_root: std::path::PathBuf,
}

/// Fully-parsed invocation handed to the dispatcher. The dispatcher never touches argv again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedInvocation {
    pub mode: MembraneMode,
    /// For `Cli` mode, the forwarded argv tail. Empty for the other modes.
    pub cli_tail: Vec<String>,
    /// For `StdioMcp`, the framing override (always "jsonl" today).
    pub framing: String,
    /// Reserved transport port. Stateless modes resolve the active Hub endpoint.
    pub port: u16,
    /// MBR-203: for `Install` mode, the scratch root, target root, optional
    /// plan path, and dry-run flag. `None` for every other mode.
    pub install: Option<InstallInvocation>,
    /// MBR-205: for `Uninstall` mode, the receipt root, the candidate
    /// paths the operator asked about, and the dry-run flag. `None` for
    /// every other mode.
    pub uninstall: Option<UninstallInvocation>,
    pub activation: Option<ActivationInvocation>,
    pub migration: Option<MigrationInvocation>,
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivationInvocation {
    pub install_root: Option<std::path::PathBuf>,
    pub clients: Vec<String>,
    pub timeout_ms: u64,
    pub dry_run: bool,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationInvocation {
    pub legacy_root: std::path::PathBuf,
    pub target_root: std::path::PathBuf,
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
                install: None,
                uninstall: None,
                activation: None,
                migration: None,
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
    // Pull, Push, Diagnostics, and Adapt are first-class Membrane commands. They
    // bypass the transport mode parser while retaining one runtime CLI
    // implementation; `diagnostics` is intercepted again inside the CLI
    // dispatcher so its subtree never reaches the runtime argv parser.
    if matches!(
        collected.get(1).and_then(|value| value.to_str()),
        Some("pull" | "push" | "diagnostics" | "adapt")
    ) {
        return Ok(ParsedInvocation {
            mode: MembraneMode::Cli,
            cli_tail: collected
                .iter()
                .skip(1)
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect(),
            framing: String::new(),
            port: 0,
            install: None,
            uninstall: None,
            activation: None,
            migration: None,
        });
    }
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
            install: None,
            uninstall: None,
            activation: None,
            migration: None,
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
                install: None,
                uninstall: None,
                activation: None,
                migration: None,
            }
        }
        Command::Install(args) => ParsedInvocation {
            mode: MembraneMode::Install,
            cli_tail: Vec::new(),
            framing: String::new(),
            port: 0,
            install: Some(InstallInvocation {
                scratch_root: args.scratch_root,
                target_root: args.target_root,
                plan: args.plan,
                dry_run: args.dry_run,
            }),
            uninstall: None,
            activation: None,
            migration: None,
        },
        Command::Uninstall(args) => ParsedInvocation {
            mode: MembraneMode::Uninstall,
            cli_tail: Vec::new(),
            framing: String::new(),
            port: 0,
            install: None,
            uninstall: Some(UninstallInvocation {
                receipt_root: args.receipt_root,
                candidates: args.candidate,
                dry_run: args.dry_run,
            }),
            activation: None,
            migration: None,
        },
        Command::Status(args) => ParsedInvocation {
            mode: MembraneMode::Activate,
            cli_tail: Vec::new(),
            framing: String::new(),
            port: 0,
            install: None,
            uninstall: None,
            activation: Some(ActivationInvocation {
                install_root: args.install_root,
                clients: args.client,
                timeout_ms: args.timeout_ms,
                dry_run: true,
            }),
            migration: None,
        },
        Command::Activate(args) => ParsedInvocation {
            mode: MembraneMode::Activate,
            cli_tail: Vec::new(),
            framing: String::new(),
            port: 0,
            install: None,
            uninstall: None,
            activation: Some(ActivationInvocation {
                install_root: args.install_root,
                clients: args.client,
                timeout_ms: args.timeout_ms,
                dry_run: args.dry_run,
            }),
            migration: None,
        },
        Command::MigrateLegacy(args) => ParsedInvocation {
            mode: MembraneMode::MigrateLegacy,
            cli_tail: Vec::new(),
            framing: String::new(),
            port: 0,
            install: None,
            uninstall: None,
            activation: None,
            migration: Some(MigrationInvocation {
                legacy_root: args.legacy_root,
                target_root: args.target_root,
            }),
        },
    };
    Ok(invocation)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleCommand {
    Drain,
    Stop,
    UpdateHandoff,
    OwnershipLoss,
    ParentEof,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LifecycleEndpoint {
    pub host: String,
    pub port: u16,
}

/// A validated inherited channel. Capability material remains in memory only & is emitted only
/// on the inherited channel's ready registration; it never enters environment, argv, or logs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LifecycleChannel {
    fence: u64,
    capability: String,
}

impl LifecycleChannel {
    pub const fn fence(&self) -> u64 {
        self.fence
    }
    pub fn capability(&self) -> &str {
        &self.capability
    }

    /// Emit lifecycle state without capability material. Runtime calls this after every
    /// admission dependency is live, never during hello parsing.
    pub fn ready(&self, endpoint: LifecycleEndpoint) -> Result<(), String> {
        if endpoint.host != "127.0.0.1" || endpoint.port < 1024 {
            return Err("lifecycle ready endpoint invalid".into());
        }
        emit_lifecycle_frame(serde_json::json!({
            "kind": "register", "state": "ready", "fence": self.fence,
            "endpoint": {"host": endpoint.host, "port": endpoint.port},
            "capability": self.capability,
        }))
    }

    /// Runtime owns admission closure, bounded draining, exact-fence acknowledgement, then
    /// natural return. This channel parser intentionally cannot stop a running service itself.
    pub fn acknowledge_drained(&self, command: LifecycleCommand) -> Result<(), String> {
        let command = match command {
            LifecycleCommand::Drain => "drain",
            LifecycleCommand::Stop => "stop",
            LifecycleCommand::UpdateHandoff => "update_handoff",
            LifecycleCommand::OwnershipLoss => "ownership_loss",
            LifecycleCommand::ParentEof => return Ok(()),
        };
        emit_lifecycle_frame(
            serde_json::json!({"kind":"ack","command":command,"fence":self.fence,"capability":self.capability}),
        )
    }

    pub fn failed(&self) -> Result<(), String> {
        emit_lifecycle_frame(serde_json::json!({
            "kind": "register", "state": "failed", "fence": self.fence
        }))
    }
}

type LifecycleCommandFrame = ResidentLifecycleFrameV1;

fn emit_lifecycle_frame(frame: serde_json::Value) -> Result<(), String> {
    use std::io::Write;
    let encoded = serde_json::to_string(&frame)
        .map_err(|error| format!("lifecycle frame encode: {error}"))?;
    let mut stdout = std::io::stdout().lock();
    writeln!(stdout, "{encoded}").map_err(|error| format!("lifecycle frame write: {error}"))?;
    stdout
        .flush()
        .map_err(|error| format!("lifecycle frame flush: {error}"))
}

/// Consume and bind hello before a resident begins startup. Ordinary launches have no channel.
pub fn consume_lifecycle_channel() -> Result<Option<LifecycleChannel>, String> {
    if std::env::var("MEMBRANE_LIFECYCLE_STDIO").as_deref() != Ok("1") {
        return Ok(None);
    }
    use std::io::BufRead;
    let mut line = String::new();
    std::io::stdin()
        .lock()
        .read_line(&mut line)
        .map_err(|error| format!("lifecycle hello read failed: {error}"))?;
    let hello: LifecycleHelloV1 =
        serde_json::from_str(&line).map_err(|error| format!("lifecycle hello invalid: {error}"))?;
    validate_lifecycle_hello_shape(&hello)?;
    let declared_root = std::fs::canonicalize(&hello.declared_data_root)
        .map_err(|_| "lifecycle data root unavailable")?;
    let configured_root = std::env::var_os("MEMBRANE_STATE_ROOT")
        .map(std::path::PathBuf::from)
        .and_then(|path| std::fs::canonicalize(path).ok())
        .ok_or("lifecycle configured data root unavailable")?;
    if declared_root != configured_root {
        return Err("lifecycle data root mismatch".into());
    }
    if hello.installation_id != sha256_text(&declared_root.to_string_lossy()) {
        return Err("lifecycle installation identity mismatch".into());
    }
    let exe = std::env::current_exe().map_err(|_| "lifecycle executable unavailable")?;
    if hello.artifact_digest != sha256_file(&exe).map_err(|_| "lifecycle artifact unavailable")? {
        return Err("lifecycle artifact identity mismatch".into());
    }
    let channel = LifecycleChannel {
        fence: hello.fence,
        capability: hello.capability,
    };
    emit_lifecycle_frame(
        serde_json::json!({"kind":"register","state":"starting","fence":channel.fence}),
    )?;
    Ok(Some(channel))
}

fn validate_lifecycle_hello_shape(hello: &LifecycleHelloV1) -> Result<(), String> {
    if hello.kind != "hello" || hello.lifecycle_version != 1 {
        return Err("lifecycle schema incompatible".into());
    }
    if hello.fence < 1 {
        return Err("lifecycle fence must be >= 1".into());
    }
    if hello.product_id != "membrane" {
        return Err("lifecycle product identity mismatch".into());
    }
    for (name, value) in [
        ("installationId", &hello.installation_id),
        ("instanceId", &hello.instance_id),
    ] {
        if !bounded_lifecycle_id(value) {
            return Err(format!("lifecycle {name} invalid"));
        }
    }
    if hello.declared_data_root.is_empty()
        || hello.declared_data_root.len() > 4096
        || hello.declared_data_root.chars().any(char::is_control)
    {
        return Err("lifecycle declaredDataRoot invalid".into());
    }
    if !is_sha256_digest(&hello.artifact_digest)
        || !is_sha256_digest(&hello.release_generation)
        || !is_sha256_digest(&hello.instance_id)
    {
        return Err("lifecycle artifact digest invalid".into());
    }
    if !is_hex64(&hello.capability) || hello.capability.len() != 64 {
        return Err("lifecycle capability must be exactly 64 lowercase hex characters".into());
    }
    Ok(())
}

fn bounded_lifecycle_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b':' | b'/')
        })
}

fn is_hex64(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}
fn is_sha256_digest(value: &str) -> bool {
    value.len() == 71 && value.starts_with("sha256:") && is_hex64(&value[7..])
}

fn sha256_text(value: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(value.as_bytes());
    format!("sha256:{}", hex::encode(digest))
}

fn sha256_file(path: &std::path::Path) -> std::io::Result<String> {
    use sha2::{Digest, Sha256};
    let bytes = std::fs::read(path)?;
    let digest = Sha256::digest(bytes);
    Ok(format!("sha256:{}", hex::encode(digest)))
}

pub fn monitor_lifecycle_channel(
    channel: LifecycleChannel,
    control: membrane_runtime::service::LifecycleControl,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        use std::io::BufRead;
        let stdin = std::io::stdin();
        let mut input = stdin.lock();
        let mut line = String::new();
        loop {
            line.clear();
            let read = match input.read_line(&mut line) {
                Ok(read) => read,
                Err(_) => {
                    control.fail("lifecycle command read failed");
                    return;
                }
            };
            if read == 0 {
                control.request_drain(Some("parent_eof"));
                return;
            }
            let frame: LifecycleCommandFrame = match serde_json::from_str(&line) {
                Ok(frame) => frame,
                Err(_) => {
                    control.fail("lifecycle command invalid");
                    return;
                }
            };
            if frame.kind != "command"
                || frame.fence != channel.fence()
                || frame.capability.as_deref() != Some(channel.capability())
            {
                control.fail("lifecycle command fence mismatch");
                return;
            }
            let command = match frame.command.as_deref() {
                Some("drain") | Some("stop") | Some("update_handoff") | Some("ownership_loss") => {
                    frame.command.clone().unwrap_or_default()
                }
                _ => {
                    control.fail("lifecycle command unsupported");
                    return;
                }
            };
            control.request_drain(Some(&command));
            return;
        }
    })
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
    fn pull_and_push_are_first_class_cli_tails() {
        for axis in ["pull", "push"] {
            let inv = parse_mode(["membrane", axis, "status"].iter().copied()).unwrap();
            assert_eq!(inv.mode, MembraneMode::Cli);
            assert_eq!(inv.cli_tail, vec![axis, "status"]);
        }
    }

    #[test]
    fn adapt_is_a_first_class_cli_tail() {
        let inv = parse_mode(["membrane", "adapt", "doctor"].iter().copied()).unwrap();
        assert_eq!(inv.mode, MembraneMode::Cli);
        assert_eq!(inv.cli_tail, vec!["adapt", "doctor"]);
    }

    #[test]
    fn diagnostics_is_first_class_cli_tail() {
        let inv = parse_mode(["membrane", "diagnostics", "capabilities"].iter().copied()).unwrap();
        assert_eq!(inv.mode, MembraneMode::Cli);
        assert_eq!(inv.cli_tail, vec!["diagnostics", "capabilities"]);
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
    fn activate_parses_installed_root_clients_and_deadline() {
        let inv = parse_mode(
            [
                "membrane",
                "activate",
                "--install-root",
                r"C:\Membrane",
                "--client",
                "codex",
                "--timeout-ms",
                "45000",
            ]
            .iter()
            .copied(),
        )
        .unwrap();
        assert_eq!(inv.mode, MembraneMode::Activate);
        assert_eq!(
            inv.activation,
            Some(ActivationInvocation {
                install_root: Some(std::path::PathBuf::from(r"C:\Membrane")),
                clients: vec!["codex".to_string()],
                timeout_ms: 45_000,
                dry_run: false,
            })
        );
    }

    #[test]
    fn retired_resident_modes_are_rejected() {
        assert!(parse_mode(["membrane", "serve"].iter().copied()).is_err());
        assert!(parse_mode(["membrane", "loopback-api"].iter().copied()).is_err());
        assert!(parse_mode(["membrane", "supervisor-child"].iter().copied()).is_err());
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

    fn valid_hello() -> serde_json::Value {
        serde_json::json!({
            "kind": "hello",
            "lifecycleVersion": 1,
            "fence": 1,
            "installationId": "installation-1",
            "productId": "membrane",
            "instanceId": format!("sha256:{}", "c".repeat(64)),
            "releaseGeneration": format!("sha256:{}", "d".repeat(64)),
            "artifactDigest": format!("sha256:{}", "a".repeat(64)),
            "declaredDataRoot": "/tmp/membrane",
            "capability": "b".repeat(64),
        })
    }

    #[test]
    fn lifecycle_hello_is_closed_and_bounded() {
        let hello: LifecycleHelloV1 = serde_json::from_value(valid_hello()).unwrap();
        validate_lifecycle_hello_shape(&hello).unwrap();

        let mut unknown = valid_hello();
        unknown["ignored"] = serde_json::json!(true);
        assert!(serde_json::from_value::<LifecycleHelloV1>(unknown).is_err());

        for (field, value) in [
            ("fence", serde_json::json!(0)),
            ("productId", serde_json::json!("blueprint")),
            ("capability", serde_json::json!("A".repeat(64))),
            ("artifactDigest", serde_json::json!("sha256:short")),
        ] {
            let mut invalid = valid_hello();
            invalid[field] = value;
            let hello: LifecycleHelloV1 = serde_json::from_value(invalid).unwrap();
            assert!(
                validate_lifecycle_hello_shape(&hello).is_err(),
                "field={field}"
            );
        }
    }
}
