//! Parity tests ported from the legacy `blueprint/tests/dependency-dag.test.mjs`
//! onto the native `dependency_dag` module. Each test names the legacy
//! `test(...)` block it reproduces.

use membrane_blueprint::dependency_dag::{
    build_projection_dependency_dag, invalidated_projections, projection_fingerprint,
    CacheOutcome, ParentValues, ProjectionCache,
};

fn dag(overrides: impl FnOnce(&mut ParentValues)) -> membrane_blueprint::dependency_dag::ProjectionDependencyDag {
    let mut values = ParentValues {
        source_hash: Some("s1".to_string()),
        provider_digest: Some("p1".to_string()),
        config_digest: Some("c1".to_string()),
        schema_version: Some("20".to_string()),
        generation_id: Some("g1".to_string()),
    };
    overrides(&mut values);
    build_projection_dependency_dag(&values)
}

// "projection DAG explicitly binds source provider config schema and generation parents"
#[test]
fn projection_dag_explicitly_binds_declared_parents() {
    let value = dag(|_| {});
    assert!(value.nodes.iter().any(|n| n.id == "parent:source"));
    assert!(value.nodes.iter().any(|n| n.id == "projection:orientation"));
    assert!(value
        .edges
        .iter()
        .any(|e| e.from == "parent:config" && e.to == "projection:contracts"));
    assert!(value
        .edges
        .iter()
        .any(|e| e.from == "parent:generation" && e.to == "projection:bm25"));
}

// "invalidation closure is projection-specific and deterministic"
#[test]
fn invalidation_closure_is_projection_specific_and_deterministic() {
    assert_eq!(
        invalidated_projections(&["config"]),
        vec!["contracts", "conventions", "orientation", "processes"]
    );
    assert!(invalidated_projections(&["source"]).iter().any(|p| p == "bm25"));
    assert!(!invalidated_projections(&["config"]).iter().any(|p| p == "bm25"));
}

// "projection cache invalidates when any declared parent changes"
#[test]
fn projection_cache_invalidates_when_declared_parent_changes() {
    let mut cache: ProjectionCache<i32> = ProjectionCache::new(16);
    let mut builds = 0;
    let base = dag(|_| {});

    let (first_value, first_outcome, _) = cache
        .get_or_build("bm25", &base, || {
            builds += 1;
            builds
        })
        .unwrap();
    assert_eq!(first_outcome, CacheOutcome::Miss);
    assert_eq!(first_value, 1);

    let (second_value, second_outcome, _) = cache
        .get_or_build("bm25", &base, || {
            builds += 1;
            builds
        })
        .unwrap();
    assert_eq!(second_outcome, CacheOutcome::Hit);
    assert_eq!(second_value, 1);

    let changed_dag = dag(|v| {
        v.source_hash = Some("s2".to_string());
        v.generation_id = Some("g2".to_string());
    });
    let (changed_value, changed_outcome, _) = cache
        .get_or_build("bm25", &changed_dag, || {
            builds += 1;
            builds
        })
        .unwrap();
    assert_eq!(changed_outcome, CacheOutcome::Invalidated);
    assert_eq!(changed_value, 2);
}

// "fingerprint ignores undeclared parent dimensions for a projection"
#[test]
fn fingerprint_ignores_undeclared_parent_dimensions() {
    let a = dag(|v| v.config_digest = Some("c1".to_string()));
    let b = dag(|v| v.config_digest = Some("c2".to_string()));
    assert_eq!(
        projection_fingerprint(&a, "bm25").unwrap(),
        projection_fingerprint(&b, "bm25").unwrap()
    );
    assert_ne!(
        projection_fingerprint(&a, "contracts").unwrap(),
        projection_fingerprint(&b, "contracts").unwrap()
    );
}

// projection_fingerprint on an unknown projection reports the legacy
// `projection_unknown` error rather than panicking.
#[test]
fn fingerprint_reports_unknown_projection() {
    let base = dag(|_| {});
    let err = projection_fingerprint(&base, "not_a_projection").unwrap_err();
    assert_eq!(err.0, "not_a_projection");
}

// Cache eviction: exceeding max_entries drops the oldest-inserted entry,
// mirroring the legacy Map-insertion-order eviction in `getOrBuild`.
#[test]
fn cache_evicts_oldest_entry_once_over_capacity() {
    let mut cache: ProjectionCache<i32> = ProjectionCache::new(1);
    let base = dag(|_| {});
    cache.get_or_build("bm25", &base, || 1).unwrap();
    cache.get_or_build("contracts", &base, || 2).unwrap();
    // bm25 was evicted, so rebuilding it must be a fresh miss, not a hit.
    let (_, outcome, _) = cache.get_or_build("bm25", &base, || 3).unwrap();
    assert_eq!(outcome, CacheOutcome::Miss);
}
