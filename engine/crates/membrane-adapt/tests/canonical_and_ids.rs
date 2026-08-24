//! Scenario 1: canonical JSON, sha256 IDs, determinism & collision
//! resistance across category/scope boundaries.

use membrane_adapt::canonical::*;

#[test]
fn canonical_json_is_key_sorted_and_stable() {
    let a = to_canonical_json(&serde_json::json!({ "b": 1, "a": 2 }));
    let b = to_canonical_json(&serde_json::json!({ "a": 2, "b": 1 }));
    assert_eq!(a, b);
    assert_eq!(a, r#"{"a":2,"b":1}"#);
}

#[test]
fn preference_ids_resist_category_boundary_collision() {
    let ab_cd = derive_preference_id("ab", "cd", "rule one two three four five six");
    let a_bcd = derive_preference_id("a", "bcd", "rule one two three four five six");
    assert_ne!(ab_cd, a_bcd);
}

#[test]
fn ids_are_deterministic_across_calls() {
    let x = derive_preference_id("repo-x", "verification", "always run focused tests");
    let y = derive_preference_id("repo-x", "verification", "always run focused tests");
    assert_eq!(x, y);
    assert!(x.starts_with("adapt-verification-")); // prefix is the category, not the scope
}

#[test]
fn episode_ids_are_order_independent_over_evidence() {
    let e1 = vec![("e2".into(), 0i64, 10i64), ("e1".into(), 5, 9)];
    let e2 = vec![("e1".into(), 5, 9), ("e2".into(), 0, 10)];
    assert_eq!(derive_episode_id("d", &e1), derive_episode_id("d", &e2));
}
