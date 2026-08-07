//! Membrane — the single signed binary that services CLI, stdio MCP, loopback API, and
//! supervisor-child modes. This crate owns the binary shape; every real work step is delegated
//! to `membrane_runtime` so the product surface has exactly one executable on a clean user
//! machine.
//!
//! MBR-102: create one membrane executable with mode subcommands.

pub mod cli;
pub mod cli_parity;
pub mod dispatch;
pub mod install_tx;
pub mod migration;
pub mod modes;
pub mod serve;
pub mod uninstall;
pub mod update;

pub use dispatch::{parse_mode, MembraneMode, ParsedInvocation};

pub use cli::{run_cli, run_cli_from};
pub use cli_parity::{check_parity, generate_cli_subcommands, CliParityReport};
pub use install_tx::{
    commit, execute_plan, InstallError, InstallOutcome, InstallPlan, InstallReceiptV1,
    InstallStage, InstallStep, INSTALL_RECEIPT_SCHEMA_VERSION,
};
pub use migration::{migrate, MigrationReceiptV1, MIGRATION_RECEIPT_FILE_NAME, MIGRATION_RECEIPT_SCHEMA_VERSION};
pub use serve::run_loopback_api;
pub use uninstall::{
    execute_uninstall, load_table, register, revoke_unowned, OwnershipClaim, OwnershipKind,
    OwnershipTable, UninstallError, UninstallOutcome, UninstallReceiptV1,
    UNINSTALL_RECEIPT_SCHEMA_VERSION,
};
pub use update::{update, UpdateHooks, UpdatePhase, UpdatePlan, UpdateReceiptV1, UPDATE_RECEIPT_FILE, UPDATE_RECEIPT_SCHEMA_VERSION};

/// Process-wide exit-code contract. The binary returns these to the OS so scripts and the
/// supervisor can distinguish "user error" from "internal failure" without parsing stderr.
pub const EXIT_OK: i32 = 0;
pub const EXIT_USER_ERROR: i32 = 2;
pub const EXIT_INTERNAL_ERROR: i32 = 1;

/// Re-exported so mode subcommands can be tested without recompiling the binary.
pub use membrane_protocol as protocol;
pub use membrane_runtime as runtime;
