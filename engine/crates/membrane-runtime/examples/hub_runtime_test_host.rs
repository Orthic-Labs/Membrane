//! Test-only process host for Hub's in-process runtime entrypoint.
//!
//! Production residency remains inside Membrane Hub. Cross-process JS tests
//! use this example to exercise that exact library entrypoint across restart.

use std::path::PathBuf;

use membrane_runtime::service::{run_hub_runtime, LifecycleControl};

fn main() {
    let root = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .expect("usage: hub_runtime_test_host <workspace-root>");
    if let Err(error) = run_hub_runtime(&root, LifecycleControl::default()) {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
