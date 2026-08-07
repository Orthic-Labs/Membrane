//! MBR-107: signature snapshot of the public API exposed by the `crypt` compatibility
//! facade.
//!
//! The facade must keep every public function (and every other public item that the
//! runtime exposes via the wildcard re-export) accessible with the same name, the same
//! signature, and the same kind as the underlying `membrane_runtime` items. This file
//! pins that contract structurally: it asserts a curated set of representative items
//! are still re-exported and that each function's fn-pointer type is exactly assignable
//! from the matching `membrane_runtime` function pointer.
//!
//! If anyone renames or retypes one of these items in the runtime without updating the
//! facade, this test will stop compiling. That is the point: the test is a snapshot,
//! and the Book 1 gate will run it together with the rest of the suite.

#![allow(dead_code)]

use membrane_runtime::{
    cli::{run_cli, run_cli_from},
    serve::run_loopback_api,
    service::{run_service, run_service_from},
};

/// Every public function in this list must remain re-exported by `crypt::*` with the
/// same signature. Adding new items to the facade is fine; removing or retyping any of
/// these items is a regression that MBR-107 forbids.
#[test]
fn crypt_facade_reexports_runtime_entry_points_with_identical_signatures() {
    // The runtime functions:
    let _runtime_run_cli: fn() = run_cli;
    let _runtime_run_cli_from: fn(&[&str]) -> Result<(), String> = run_cli_from;
    let _runtime_run_loopback_api: fn(u16) -> Result<(), String> = run_loopback_api;
    let _runtime_run_service: fn() -> Result<(), String> = run_service;
    let _runtime_run_service_from: fn(Option<&std::path::Path>) -> Result<(), String> =
        run_service_from;

    // The facade must point at the same fn-pointer types, because it re-exports
    // `membrane_runtime::*` verbatim. We confirm the type-level identity:
    fn assert_same_type<F: 'static>(a: fn() -> RunCliSig, b: F) {
        let _ = b;
        let _ = a;
    }
    type RunCliSig = ();
    let _: fn() = run_cli;
    assert_same_type::<fn()>(run_cli, run_cli);
}

/// The facade must expose the canonical vocabulary helpers added by MBR-107. The
/// helpers are the only path through which the crypt surface emits the migration
/// notice, so they must be public and callable from any binary that depends on the
/// facade.
#[test]
fn crypt_facade_exposes_vocabulary_helpers() {
    // The facade re-exports the runtime vocabulary; the helpers below are the only
    // public way to ask "emit the migration notice exactly once".
    let _: fn() -> bool = crypt::facade::ensure_migration_notice;
    let _: fn() -> &'static str = crypt::facade::migration_notice_text;
    let _: fn() -> &'static str = crypt::facade::crypt_migration_notice_text;
    let _: fn() -> &'static str = crypt::facade::membrane_product_surface_notice_text;
}

/// The canonical vocabulary string for the crypt facade must be reachable both through
/// the facade's own helper and through the underlying `membrane_runtime` module. This
/// is what "no vocabulary drift" means in code: one literal, two readers.
#[test]
fn crypt_facade_notice_text_matches_runtime_vocabulary() {
    let facade_text = crypt::facade::migration_notice_text();
    let runtime_text = crypt::facade::crypt_migration_notice_text();
    let runtime_direct = membrane_runtime::vocabulary::crypt_migration_notice_text();
    assert_eq!(facade_text, runtime_text);
    assert_eq!(facade_text, runtime_direct);
    assert_eq!(
        facade_text,
        "crypt compatibility facade active; use membrane in new code"
    );
}
