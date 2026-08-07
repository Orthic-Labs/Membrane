//! Tiny CLI entry point for the reference Crypt adapter.
//!
//! Usage:
//!
//!   $ crypt_example run-conformance
//!
//! Exits 0 if every crypt fixture in `membrane-testkit` passes the
//! SDK's `run_conformance` harness, 1 otherwise. The output is a
//! JSON-serialized `ConformanceReport` on stdout.

use crypt_example::{run_crypt_conformance, CryptExample};
use membrane_provider_sdk::Provider;
use serde_json::json;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut provider = CryptExample::new();
    if let Err(e) = provider.initialize(&json!({})) {
        eprintln!("crypt_example: initialize failed: {e}");
        return ExitCode::FAILURE;
    }
    let report = run_crypt_conformance(&provider);
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
