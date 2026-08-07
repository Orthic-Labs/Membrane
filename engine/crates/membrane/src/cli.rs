//! Membrane-native CLI entry point.
//!
//! MBR-107: thin wrapper over `membrane_runtime::cli::run_cli_from` that lets
//! library callers invoke the CLI without going through the binary's mode
//! parser. Signatures are identical to the underlying runtime function.
//!
//! On first invocation in a process, this module emits the canonical
//! product-surface notice from `membrane_runtime::vocabulary` so operators see
//! a single structured log line telling them the legacy `crypt` binary is a
//! compatibility facade.

use membrane_runtime::vocabulary::{emit_facade_notice_once, ProductSurface};

/// Run the membrane CLI with the given argv slice. Signature matches
/// `membrane_runtime::cli::run_cli_from` exactly so callers can swap one for
/// the other without code changes.
pub fn run_cli_from(argv: &[&str]) -> Result<(), String> {
    // Stamp the product-surface notice once per process. The guard lives in
    // `vocabulary`, so the membrane binary's `cli` mode and any library caller
    // converge on the same wording.
    let _ = emit_facade_notice_once(ProductSurface::Membrane);
    membrane_runtime::cli::run_cli_from(argv)
}

/// Run the membrane CLI with no argv (prints help). Signature matches
/// `membrane_runtime::cli::run_cli` exactly.
pub fn run_cli() {
    let _ = emit_facade_notice_once(ProductSurface::Membrane);
    membrane_runtime::cli::run_cli();
}

#[cfg(test)]
mod tests {
    // No runtime here; we only assert that the membrane-native entry points
    // exist and have the same signatures as the runtime functions. The
    // signature equality is checked structurally: the function pointers must be
    // assignable to the same fn-pointer type as the runtime functions.
    #[test]
    fn run_cli_signature_matches_runtime_run_cli() {
        let _runtime_fn: fn() = membrane_runtime::cli::run_cli;
        let _membrane_fn: fn() = crate::run_cli;
    }

    #[test]
    fn run_cli_from_signature_matches_runtime_run_cli_from() {
        let _runtime_fn: fn(&[&str]) -> Result<(), String> = membrane_runtime::cli::run_cli_from;
        let _membrane_fn: fn(&[&str]) -> Result<(), String> = crate::run_cli_from;
    }
}
