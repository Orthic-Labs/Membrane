//! Membrane-native CLI entry point.
//!
//! Thin wrapper over `membrane_runtime::cli::run_cli_from` that lets
//! library callers invoke the CLI without going through the binary's mode
//! parser. Signatures are identical to the underlying runtime function.

/// Run the membrane CLI with the given argv slice. Signature matches
/// `membrane_runtime::cli::run_cli_from` exactly so callers can swap one for
/// the other without code changes.
pub fn run_cli_from(argv: &[&str]) -> Result<(), String> {
    membrane_runtime::cli::run_cli_from(argv)
}

/// Run the membrane CLI with no argv (prints help). Signature matches
/// `membrane_runtime::cli::run_cli` exactly.
pub fn run_cli() {
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
