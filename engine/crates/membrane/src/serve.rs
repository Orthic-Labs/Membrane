//! Membrane-native loopback API entry point.
//!
//! Thin wrapper over `membrane_runtime::serve::run_loopback_api` that
//! lets library callers bind the loopback API without going through the
//! binary's mode parser. Signature is identical to the underlying runtime
//! function.

/// Bind the loopback API on the given port. Signature matches
/// `membrane_runtime::serve::run_loopback_api` exactly.
pub fn run_loopback_api(port: u16) -> Result<(), String> {
    membrane_runtime::serve::run_loopback_api(port)
}

#[cfg(test)]
mod tests {
    #[test]
    fn run_loopback_api_signature_matches_runtime() {
        let _runtime_fn: fn(u16) -> Result<(), String> = membrane_runtime::serve::run_loopback_api;
        let _membrane_fn: fn(u16) -> Result<(), String> = crate::run_loopback_api;
    }
}
