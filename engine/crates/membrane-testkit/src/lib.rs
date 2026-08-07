//! # membrane-testkit
//!
//! The Membrane testkit. Ships the canonical Cortex and Crypt conformance
//! fixture corpus as embedded JSON, plus a `golden_fixtures()` accessor
//! that returns the union of both sets as a `Vec<Fixture>`.
//!
//! The fixtures are versioned with the testkit. When a fixture is
//! updated, the testkit version is bumped and the conformance test in
//! `membrane-provider-sdk` is updated to assert against the new fixture
//! bytes. This keeps the fixture corpus and the conformance harness in
//! lockstep.
//!
//! ## Layout
//!
//!   * `src/fixtures/cortex/*.json` — the canonical Cortex adapter
//!     conformance set (one file per operation/case).
//!   * `src/fixtures/crypt/*.json` — the canonical Crypt adapter
//!     conformance set (one file per operation/case).
//!   * `golden_fixtures()` — returns the union, parsed into
//!     `membrane_provider_sdk::Fixture` values.
//!   * `cortex_fixtures()` / `crypt_fixtures()` — return the per-adapter
//!     set, useful for adapter-specific unit tests.

use membrane_provider_sdk::Fixture;

/// Embedded Cortex fixtures. New files added to `src/fixtures/cortex/`
/// must also be appended to this list (the manifest is hand-maintained
/// to keep the corpus self-describing).
pub const CORTEX_FIXTURE_NAMES: &[&str] = &[
    "cortex-context-scope-grant.json",
    "cortex-source-read-anchor.json",
];

/// Embedded Crypt fixtures. New files added to `src/fixtures/crypt/`
/// must also be appended to this list.
pub const CRYPT_FIXTURE_NAMES: &[&str] =
    &["crypt-knowledge-propose.json", "crypt-checkpoint-save.json"];

/// All Cortex fixtures, in declaration order.
pub fn cortex_fixtures() -> Vec<Fixture> {
    CORTEX_FIXTURE_NAMES
        .iter()
        .map(|name| load_fixture("cortex", name))
        .collect()
}

/// All Crypt fixtures, in declaration order.
pub fn crypt_fixtures() -> Vec<Fixture> {
    CRYPT_FIXTURE_NAMES
        .iter()
        .map(|name| load_fixture("crypt", name))
        .collect()
}

/// The union of `cortex_fixtures()` and `crypt_fixtures()`. This is the
/// set the Book 1 gate runs the SDK's `run_conformance` over.
pub fn golden_fixtures() -> Vec<Fixture> {
    let mut all = Vec::with_capacity(CORTEX_FIXTURE_NAMES.len() + CRYPT_FIXTURE_NAMES.len());
    all.extend(cortex_fixtures());
    all.extend(crypt_fixtures());
    all
}

/// Load one embedded fixture by adapter directory and file name.
fn load_fixture(dir: &str, name: &str) -> Fixture {
    let json = match (dir, name) {
        ("cortex", "cortex-context-scope-grant.json") => {
            include_str!("fixtures/cortex/cortex-context-scope-grant.json")
        }
        ("cortex", "cortex-source-read-anchor.json") => {
            include_str!("fixtures/cortex/cortex-source-read-anchor.json")
        }
        ("crypt", "crypt-knowledge-propose.json") => {
            include_str!("fixtures/crypt/crypt-knowledge-propose.json")
        }
        ("crypt", "crypt-checkpoint-save.json") => {
            include_str!("fixtures/crypt/crypt-checkpoint-save.json")
        }
        _ => panic!("unknown testkit fixture: {dir}/{name}"),
    };
    serde_json::from_str(json)
        .unwrap_or_else(|e| panic!("invalid JSON in testkit fixture {dir}/{name}: {e}"))
}
