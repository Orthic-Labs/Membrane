//! Port of `blueprint/tests/*plugin-loader*.mjs`: registry-order contract
//! for `membrane_blueprint::providers`.

use membrane_blueprint::providers::{registry, PROVIDER_ORDER};

#[test]
fn provider_order_matches_legacy_build_mjs_sequence() {
    assert_eq!(
        PROVIDER_ORDER,
        &[
            "ingestion",
            "plugins",
            "blueprint-modules",
            "blueprint-frameworks",
            "blueprint-sql",
            "blueprint-terraform",
            "scip-python",
            "blueprint-bridges",
            "structural-intelligence",
            "framework-intelligence",
            "portable-identity",
            "conventions",
        ]
    );
}

#[test]
fn registry_entries_appear_in_legacy_order_regardless_of_registration_order() {
    let reg = registry();
    let ordinals: Vec<usize> = reg.iter().map(|d| PROVIDER_ORDER.iter().position(|id| *id == d.id).expect("registered provider id must be in PROVIDER_ORDER")).collect();
    let mut sorted = ordinals.clone();
    sorted.sort_unstable();
    assert_eq!(ordinals, sorted, "registry() must yield providers in legacy plugin-loader/build.mjs order");
}
