//! Tiny CLI entry point for the reference Blueprint adapter.
//!
//! Usage:
//!
//!   $ blueprint_example run-conformance
//!
//! Exits 0 if every blueprint fixture in `membrane-testkit` passes the
//! SDK's `run_conformance` harness, 1 otherwise. The output is a
//! JSON-serialized `ConformanceReport` on stdout.

use blueprint_example::{run_blueprint_conformance, BlueprintExample};
use membrane_provider_sdk::Provider;
use serde_json::json;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut provider = BlueprintExample::new();
    if let Err(e) = provider.initialize(&json!({})) {
        eprintln!("blueprint_example: initialize failed: {e}");
        return ExitCode::FAILURE;
    }
    let report = run_blueprint_conformance(&provider);
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
