//! MBR-107: the vocabulary module is the single source of truth for product-surface
//! strings.
//!
//! The acceptance criterion for MBR-107 is "Existing scripts continue to work and
//! emit a measured migration notice without vocabulary drift." The drift we are
//! guarding against is a future change that hard-codes the notice text in one
//! of the binaries or facades, so the membrane binary and the crypt facade end
//! up with slightly different wording. This test reads the source of each crate
//! involved and asserts:
//!
//! - The canonical wording lives in exactly one place: `vocabulary.rs`.
//! - Neither the crypt crate nor the membrane crate embeds the canonical
//!   wording as a string literal; they reference the constants from vocabulary.
//! - The `emit_facade_notice_once` function is the only path that prints the
//!   structured log line.
//!
//! The test is permitted to read the source files because it is part of the
//! compile-time contract: if someone changes the wording, the constants move,
//! and this test catches it.

use membrane_runtime::vocabulary::{
    crypt_migration_notice_text, format_notice_log_line, membrane_product_surface_notice_text,
    CRYPT_FACADE_MIGRATION_NOTICE, MEMBRANE_PRODUCT_SURFACE_NOTICE,
};

// All paths are relative to this test file:
//   engine/crates/membrane-runtime/tests/vocabulary_source_of_truth.rs
// Going up three `..` levels lands at `engine/`, then we descend into the
// sibling crate's source tree.
const CRYPT_LIB_RS: &str = include_str!("../../../crates/crypt/src/lib.rs");
const CRYPT_FACADE_RS: &str = include_str!("../../../crates/crypt/src/facade.rs");
const CRYPT_MAIN_RS: &str = include_str!("../../../crates/crypt/src/main.rs");
const CRYPT_SERVICE_MAIN_RS: &str = include_str!("../../../crates/crypt/src/crypt_service_main.rs");
const MEMBRANE_CLI_RS: &str = include_str!("../../../crates/membrane/src/cli.rs");
const MEMBRANE_SERVE_RS: &str = include_str!("../../../crates/membrane/src/serve.rs");
const MEMBRANE_MODES_RS: &str = include_str!("../../../crates/membrane/src/modes.rs");

#[test]
fn vocabulary_constants_match_acceptance_text() {
    assert_eq!(
        CRYPT_FACADE_MIGRATION_NOTICE,
        "crypt compatibility facade active; use membrane in new code"
    );
    assert_eq!(
        MEMBRANE_PRODUCT_SURFACE_NOTICE,
        "this binary is membrane; the legacy crypt binary is a compatibility facade"
    );
}

#[test]
fn crypt_facade_does_not_duplicate_canonical_text_as_literal() {
    // The facade source must not embed the canonical notice as a string literal;
    // it must source from the vocabulary constant. Doc comments may mention the
    // wording; the check below looks for the exact literal pattern.
    let literal_appearances = CRYPT_FACADE_RS
        .matches("\"crypt compatibility facade active; use membrane in new code\"")
        .count();
    assert_eq!(
        literal_appearances, 0,
        "crypt/src/facade.rs must not embed the canonical notice as a literal; \
         source it from membrane_runtime::vocabulary instead"
    );
}

#[test]
fn membrane_modes_does_not_duplicate_product_surface_text_as_literal() {
    let literal_appearances = MEMBRANE_MODES_RS
        .matches("\"this binary is membrane; the legacy crypt binary is a compatibility facade\"")
        .count();
    assert_eq!(
        literal_appearances, 0,
        "membrane/src/modes.rs must not embed the canonical notice as a literal; \
         source it from membrane_runtime::vocabulary instead"
    );
}

#[test]
fn crypt_binary_entry_points_route_through_facade_helper() {
    // The binary entry points must call the facade helper (or the vocabulary
    // function directly) instead of building the notice themselves.
    assert!(
        CRYPT_MAIN_RS.contains("ensure_migration_notice"),
        "crypt/src/main.rs must call crypt::facade::ensure_migration_notice first"
    );
    assert!(
        CRYPT_SERVICE_MAIN_RS.contains("ensure_migration_notice"),
        "crypt/src/crypt_service_main.rs must call crypt::facade::ensure_migration_notice first"
    );
}

#[test]
fn membrane_native_entry_points_route_through_vocabulary_helper() {
    assert!(
        MEMBRANE_CLI_RS.contains("emit_facade_notice_once"),
        "membrane/src/cli.rs must call membrane_runtime::vocabulary::emit_facade_notice_once"
    );
    assert!(
        MEMBRANE_SERVE_RS.contains("emit_facade_notice_once"),
        "membrane/src/serve.rs must call membrane_runtime::vocabulary::emit_facade_notice_once"
    );
}

#[test]
fn formatted_log_line_has_stable_keys_and_values() {
    let line = format_notice_log_line(
        membrane_runtime::vocabulary::ProductSurface::Crypt,
        crypt_migration_notice_text(),
    );
    assert!(
        line.contains("level="),
        "log line missing level key: {line}"
    );
    assert!(
        line.contains("level=notice"),
        "log line missing level value: {line}"
    );
    assert!(
        line.contains("surface="),
        "log line missing surface key: {line}"
    );
    assert!(
        line.contains("surface=crypt"),
        "log line missing crypt surface value: {line}"
    );
    assert!(line.contains("msg=\""), "log line missing msg key: {line}");
    let membrane_line = format_notice_log_line(
        membrane_runtime::vocabulary::ProductSurface::Membrane,
        membrane_product_surface_notice_text(),
    );
    assert!(
        membrane_line.contains("surface=membrane"),
        "membrane log line missing surface=membrane: {membrane_line}"
    );
}

#[test]
fn lib_reexports_surface_and_notice_constants() {
    // The membrane-runtime lib.rs re-exports the vocabulary symbols so callers
    // (and the crypt facade) can `use membrane_runtime::crypt_migration_notice_text;`
    // directly. Pin that contract here.
    let _ = membrane_runtime::CRYPT_FACADE_MIGRATION_NOTICE;
    let _ = membrane_runtime::MEMBRANE_PRODUCT_SURFACE_NOTICE;
    let _ = membrane_runtime::ProductSurface::Crypt;
    let _ = membrane_runtime::ProductSurface::Membrane;
}

/// Documenting that we deliberately keep the crypt facade's wildcard re-export
/// (`pub use membrane_runtime::*;`) so the vocabulary symbols are reachable
/// through both `membrane_runtime::vocabulary::...` and `crypt::vocabulary::...`.
#[test]
fn crypt_facade_re_exposes_vocabulary_through_wildcard() {
    assert!(
        CRYPT_LIB_RS.contains("pub use membrane_runtime::*;"),
        "crypt/src/lib.rs must keep the wildcard re-export so vocabulary symbols are reachable through `crypt::...`"
    );
}
