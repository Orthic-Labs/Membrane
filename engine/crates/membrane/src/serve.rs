//! Membrane-native loopback API entry point.
//!
//! MBR-107: thin wrapper over `membrane_runtime::serve::run_loopback_api` that
//! lets library callers bind the loopback API without going through the
//! binary's mode parser. Signature is identical to the underlying runtime
//! function.
//!
//! On first invocation in a process, this module emits the canonical
//! product-surface notice from `membrane_runtime::vocabulary` so operators see
//! a single structured log line telling them the legacy `crypt` binary is a
//! compatibility facade.

use membrane_runtime::vocabulary::{emit_facade_notice_once, ProductSurface};

/// Bind the loopback API on the given port. Signature matches
/// `membrane_runtime::serve::run_loopback_api` exactly.
pub fn run_loopback_api(port: u16) -> Result<(), String> {
    let _ = emit_facade_notice_once(ProductSurface::Membrane);
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
