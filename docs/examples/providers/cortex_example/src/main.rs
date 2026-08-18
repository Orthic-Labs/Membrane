//! Tiny CLI entry point for the reference Cortex adapter.
//!
//! Usage:
//!
//!   $ cortex_example run-conformance
//!
//! Exits 0 if every cortex fixture in `membrane-testkit` passes the
//! SDK's `run_conformance` harness, 1 otherwise. The output is a
//! JSON-serialized `ConformanceReport` on stdout.

use cortex_example::{run_cortex_conformance, CortexExample};
use membrane_provider_sdk::Provider;
use serde_json::json;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut provider = CortexExample::new();
    if let Err(e) = provider.initialize(&json!({})) {
        eprintln!("cortex_example: initialize failed: {e}");
        return ExitCode::FAILURE;
    }
    let report = run_cortex_conformance(&provider);
    println!(
        "{}",
        serde_json::to_string_pretty(&report).expect("report is JSON-serializable")
    );
    if report.is_conformant() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
