//! # membrane-testkit
//!
//! The Membrane testkit. Ships the canonical Blueprint and Cortex conformance
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
//!   * `src/fixtures/blueprint/*.json` — the canonical Blueprint adapter
//!     conformance set (one file per operation/case).
//!   * `src/fixtures/cortex/*.json` — the canonical Cortex adapter
//!     conformance set (one file per operation/case).
//!   * `golden_fixtures()` — returns the union, parsed into
//!     `membrane_provider_sdk::Fixture` values.
//!   * `blueprint_fixtures()` / `cortex_fixtures()` — return the per-adapter
//!     set, useful for adapter-specific unit tests.

use membrane_provider_sdk::Fixture;

/// Embedded Blueprint fixtures. New files added to `src/fixtures/blueprint/`
/// must also be appended to this list (the manifest is hand-maintained
/// to keep the corpus self-describing).
pub const BLUEPRINT_FIXTURE_NAMES: &[&str] = &[
    "blueprint-context-scope-grant.json",
    "blueprint-source-read-anchor.json",
];

/// Embedded Cortex fixtures. New files added to `src/fixtures/cortex/`
/// must also be appended to this list.
pub const CORTEX_FIXTURE_NAMES: &[&str] = &[
    "cortex-knowledge-propose.json",
    "cortex-checkpoint-save.json",
];

/// All Blueprint fixtures, in declaration order.
pub fn blueprint_fixtures() -> Vec<Fixture> {
    BLUEPRINT_FIXTURE_NAMES
        .iter()
        .map(|name| load_fixture("blueprint", name))
        .collect()
}

/// All Cortex fixtures, in declaration order.
pub fn cortex_fixtures() -> Vec<Fixture> {
    CORTEX_FIXTURE_NAMES
        .iter()
        .map(|name| load_fixture("cortex", name))
        .collect()
}

/// The union of `blueprint_fixtures()` and `cortex_fixtures()`. This is the
/// set the Book 1 gate runs the SDK's `run_conformance` over.
pub fn golden_fixtures() -> Vec<Fixture> {
    let mut all = Vec::with_capacity(BLUEPRINT_FIXTURE_NAMES.len() + CORTEX_FIXTURE_NAMES.len());
    all.extend(blueprint_fixtures());
    all.extend(cortex_fixtures());
    all
}

/// Load one embedded fixture by adapter directory and file name.
fn load_fixture(dir: &str, name: &str) -> Fixture {
    let json = match (dir, name) {
        ("blueprint", "blueprint-context-scope-grant.json") => {
            include_str!("fixtures/blueprint/blueprint-context-scope-grant.json")
        }
        ("blueprint", "blueprint-source-read-anchor.json") => {
            include_str!("fixtures/blueprint/blueprint-source-read-anchor.json")
        }
        ("cortex", "cortex-knowledge-propose.json") => {
            include_str!("fixtures/cortex/cortex-knowledge-propose.json")
        }
        ("cortex", "cortex-checkpoint-save.json") => {
            include_str!("fixtures/cortex/cortex-checkpoint-save.json")
        }
        _ => panic!("unknown testkit fixture: {dir}/{name}"),
    };
    serde_json::from_str(json)
        .unwrap_or_else(|e| panic!("invalid JSON in testkit fixture {dir}/{name}: {e}"))
}
