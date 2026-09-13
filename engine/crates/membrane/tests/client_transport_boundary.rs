//! Transport-only boundary for `membrane-client`.
//!
//! The installed client must stay a bounded loopback transport: it frames
//! HTTP requests to the singleton engine and prints process-local packaging
//! metadata. It must not construct Membrane runtime state, own storage, or
//! run installer control. This test greps the client's source so drift back
//! into runtime dispatch fails the suite instead of shipping silently.

use std::path::PathBuf;

fn client_source() -> String {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let path = manifest.join("src/bin/membrane-client.rs");
    std::fs::read_to_string(&path).expect("read membrane-client source")
}

// Symbols that would pull Membrane runtime, storage, planner, or installer
// control into the transport binary. Each entry is (needle, reason).
const FORBIDDEN: &[(&str, &str)] = &[
    ("membrane_runtime", "runtime ownership"),
    ("membrane-runtime", "runtime crate reference"),
    ("membrane_mcp", "MCP server ownership"),
    ("membrane-mcp", "MCP crate reference"),
    ("membrane::modes", "local command dispatch"),
    ("membrane::dispatch", "local command parsing/dispatch"),
    ("dispatch(&invocation)", "local dispatch fallback"),
    ("DispatchOutcome", "local dispatch outcome"),
    ("execute_plan", "installer control"),
    ("install_tx", "installer control"),
    ("activation::", "activation control"),
    ("MemDb", "storage ownership"),
    ("MemoryStore", "storage ownership"),
    ("federat", "planner/federation ownership"),
    ("Connection: close", "per-request close defeats reuse"),
];

#[test]
fn transport_client_has_no_runtime_or_installer_symbols() {
    let source = client_source();
    // The unit-test module below `#[cfg(test)]` may name legacy behavior in
    // fixtures; the boundary applies to shipped code above it.
    let shipped = source
        .split_once("#[cfg(test)]")
        .map(|(head, _)| head)
        .unwrap_or(&source);
    let mut violations = Vec::new();
    for (needle, reason) in FORBIDDEN {
        if shipped.contains(needle) {
            violations.push(format!("{needle} ({reason})"));
        }
    }
    assert!(
        violations.is_empty(),
        "membrane-client must stay transport-only; forbidden symbols present: {}",
        violations.join(", ")
    );
}

#[test]
fn transport_client_keeps_bounded_reuse_and_local_probes() {
    let source = client_source();
    for required in [
        "POOL_MAX_CONNS",
        "Connection: keep-alive",
        "MAX_ATTEMPTS_PER_REQUEST",
        "print_build_info",
        "client_release_identity_generated",
        "installer-owned control",
        // Effectful-route retry guard: a fully-sent request on /cli or /mcp
        // must never be silently retried (possible double execution).
        "ExchangeFailure",
        "request_sent",
        "outcome uncertain after full send",
        "HOOK_PATH",
        // Bounded engine-down behavior for hosts: typed response, no hang.
        "CONNECT_TIMEOUT",
        "engine_unavailable",
    ] {
        assert!(
            source.contains(required),
            "membrane-client missing required transport element: {required}"
        );
    }
}
