//! Signature snapshot for canonical Cortex API.

#![allow(dead_code)]

use membrane_runtime::{
    cli::{run_cli, run_cli_from},
    serve::run_loopback_api,
    service::{run_service, run_service_from},
};

#[test]
fn cortex_reexports_canonical_runtime_entry_points() {
    let _: fn() = cortex::cli::run_cli;
    let _: fn(&[&str]) -> Result<(), String> = cortex::cli::run_cli_from;
    let _: fn(u16) -> Result<(), String> = cortex::serve::run_loopback_api;
    let _: fn() -> Result<(), String> = cortex::service::run_service;
    let _: fn(Option<&std::path::Path>) -> Result<(), String> = cortex::service::run_service_from;

    let _: fn() = run_cli;
    let _: fn(&[&str]) -> Result<(), String> = run_cli_from;
    let _: fn(u16) -> Result<(), String> = run_loopback_api;
    let _: fn() -> Result<(), String> = run_service;
    let _: fn(Option<&std::path::Path>) -> Result<(), String> = run_service_from;
}
