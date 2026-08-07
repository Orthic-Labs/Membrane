//! Membrane — the single signed binary that services CLI, stdio MCP, loopback API, and
//! supervisor-child modes. This crate owns the binary shape; every real work step is delegated
//! to `membrane_runtime` so the product surface has exactly one executable on a clean user
//! machine.
//!
//! MBR-102: create one membrane executable with mode subcommands.

pub mod dispatch;
pub mod modes;

pub use dispatch::{parse_mode, MembraneMode, ParsedInvocation};

/// Process-wide exit-code contract. The binary returns these to the OS so scripts and the
/// supervisor can distinguish "user error" from "internal failure" without parsing stderr.
pub const EXIT_OK: i32 = 0;
pub const EXIT_USER_ERROR: i32 = 2;
pub const EXIT_INTERNAL_ERROR: i32 = 1;

/// Re-exported so mode subcommands can be tested without recompiling the binary.
pub use membrane_protocol as protocol;
pub use membrane_runtime as runtime;
