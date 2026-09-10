//! Native port of the legacy `blueprint/src/graph/dependency-dag.mjs`
//! projection-dependency DAG.
//!
//! This is not a source-code call graph: it is the small, fixed bipartite
//! DAG that binds Blueprint's five parent evidence dimensions (`source`,
//! `provider`, `config`, `schema`, `generation`) to the named query
//! projections that consume them (`bm25`, `structural_search`,
//! `signatures`, `contracts`, `processes`, `conventions`, `orientation`).
//! It exists so cache invalidation is explicit and projection-specific
//! rather than blanket-applied on every parent change.
//!
//! Behavior ported 1:1 from the legacy module:
//! - [`PROJECTION_DEPENDENCIES`] is the same fixed declaration table.
//! - [`build_projection_dependency_dag`] builds `parent:*` and
//!   `projection:*` nodes and the declared edges between them.
//! - [`invalidated_projections`] returns the sorted, deterministic set of
//!   projections whose declared dependencies intersect the changed parent
//!   set (parent ids may be given bare or with a `parent:` prefix).
//! - [`projection_fingerprint`] hashes only a projection's declared parent
//!   values (sha256 over a stably-sorted JSON encoding), so undeclared
//!   parent dimensions never perturb its fingerprint.
//! - [`ProjectionCache`] is a small bounded LRU-by-insertion cache keyed by
//!   projection name, reporting `hit` / `miss` / `invalidated` like the
//!   legacy `getOrBuild`, and evicting the oldest entry once `max_entries`
//!   is exceeded.

use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};

/// The five parent evidence dimensions, in the legacy module's declared
/// order (also the node emission order for `parent:*` nodes).
pub const PARENT_IDS: [&str; 5] = ["source", "provider", "config", "schema", "generation"];

/// The fixed declaration table: projection name -> parent dimensions it
/// depends on. Order matches the legacy `PROJECTION_DEPENDENCIES` object
/// and is preserved for node emission; `invalidated_projections` sorts its
/// output independently, matching the legacy `.sort()` call.
pub const PROJECTION_DEPENDENCIES: &[(&str, &[&str])] = &[
    ("bm25", &["source", "provider", "schema", "generation"]),
    ("structural_search", &["source", "provider", "schema", "generation"]),
    ("signatures", &["source", "provider", "schema", "generation"]),
    ("contracts", &["source", "provider", "config", "schema", "generation"]),
    ("processes", &["source", "provider", "config", "schema", "generation"]),
    ("conventions", &["source", "config", "generation"]),
    ("orientation", &["source", "provider", "config", "schema", "generation"]),
];

fn dependencies_for(projection: &str) -> Option<&'static [&'static str]> {
    PROJECTION_DEPENDENCIES
        .iter()
        .find(|(name, _)| *name == projection)
        .map(|(_, deps)| *deps)
}

/// A node in the DAG: either a `parent:<id>` node carrying its bound value,
/// or a `projection:<id>` node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DagNode {
    pub id: String,
    pub kind: &'static str,
    /// Present (possibly `Value::Null`) only for `kind == "parent"`.
    pub value: Option<Value>,
}

/// A directed edge `from` a `parent:*` node `to` a `projection:*` node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DagEdge {
    pub from: String,
    pub to: String,
}

/// The built projection-dependency DAG. `parents` mirrors the legacy
/// object's `parents` map (dimension id -> bound value, `None` == `null`).
#[derive(Debug, Clone)]
pub struct ProjectionDependencyDag {
    pub schema_version: u32,
    pub parents: BTreeMap<&'static str, Option<String>>,
    pub nodes: Vec<DagNode>,
    pub edges: Vec<DagEdge>,
}

/// Optional parent values used to build a DAG; mirrors the legacy
/// destructured options object (`sourceHash`, `providerDigest`,
/// `configDigest`, `schemaVersion`, `generationId`).
#[derive(Debug, Clone, Default)]
pub struct ParentValues {
    pub source_hash: Option<String>,
    pub provider_digest: Option<String>,
    pub config_digest: Option<String>,
    pub schema_version: Option<String>,
    pub generation_id: Option<String>,
}

impl ParentValues {
    fn get(&self, dimension: &str) -> Option<String> {
        match dimension {
            "source" => self.source_hash.clone(),
            "provider" => self.provider_digest.clone(),
            "config" => self.config_digest.clone(),
            "schema" => self.schema_version.clone(),
            "generation" => self.generation_id.clone(),
            _ => None,
        }
    }
}

/// Build the fixed bipartite projection-dependency DAG for a given set of
/// parent evidence values. Ports `buildProjectionDependencyDag`.
pub fn build_projection_dependency_dag(values: &ParentValues) -> ProjectionDependencyDag {
    let mut parents: BTreeMap<&'static str, Option<String>> = BTreeMap::new();
    let mut nodes = Vec::new();

    for id in PARENT_IDS {
        let value = values.get(id);
        parents.insert(id, value.clone());
        nodes.push(DagNode {
            id: format!("parent:{id}"),
            kind: "parent",
            value: Some(value.map(Value::String).unwrap_or(Value::Null)),
        });
    }

    let mut edges = Vec::new();
    for (projection, dependencies) in PROJECTION_DEPENDENCIES {
        nodes.push(DagNode {
            id: format!("projection:{projection}"),
            kind: "projection",
            value: None,
        });
        for dependency in *dependencies {
            edges.push(DagEdge {
                from: format!("parent:{dependency}"),
                to: format!("projection:{projection}"),
            });
        }
    }

    ProjectionDependencyDag { schema_version: 1, parents, nodes, edges }
}

/// Return the sorted, deduplicated set of projection names whose declared
/// dependencies intersect `changed_parents`. Entries may be given bare
/// (`"config"`) or prefixed (`"parent:config"`), matching the legacy
/// `.replace(/^parent:/, "")` normalization. Ports `invalidatedProjections`.
pub fn invalidated_projections<S: AsRef<str>>(changed_parents: &[S]) -> Vec<String> {
    let changed: HashSet<String> = changed_parents
        .iter()
        .map(|value| value.as_ref().trim_start_matches("parent:").to_string())
        .collect();
    let mut projections: Vec<String> = PROJECTION_DEPENDENCIES
        .iter()
        .filter(|(_, deps)| deps.iter().any(|dep| changed.contains(*dep)))
        .map(|(name, _)| name.to_string())
        .collect();
    projections.sort();
    projections
}

/// Error returned by [`projection_fingerprint`] for an undeclared
/// projection name. Ports the legacy `projection_unknown` error code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectionUnknown(pub String);

impl std::fmt::Display for ProjectionUnknown {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unknown projection {}", self.0)
    }
}
impl std::error::Error for ProjectionUnknown {}

/// Deterministically stable-sort a JSON value's object keys, recursively,
/// mirroring the legacy `stable()` helper used before hashing.
fn stable(value: &Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.iter().map(stable).collect()),
        Value::Object(map) => {
            let mut sorted = Map::new();
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            for key in keys {
                sorted.insert(key.clone(), stable(map.get(key).unwrap()));
            }
            Value::Object(sorted)
        }
        other => other.clone(),
    }
}

fn hash_value(value: &Value) -> String {
    let stabilized = stable(value);
    let encoded = serde_json::to_string(&stabilized).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(encoded.as_bytes());
    format!("sha256:{}", hex::encode(hasher.finalize()))
}

/// Fingerprint a projection over only its declared parent dimensions, so a
/// change to an undeclared parent never perturbs the result. Ports
/// `projectionFingerprint`.
pub fn projection_fingerprint(
    dag: &ProjectionDependencyDag,
    projection: &str,
) -> Result<String, ProjectionUnknown> {
    let dependencies =
        dependencies_for(projection).ok_or_else(|| ProjectionUnknown(projection.to_string()))?;
    let mut parents = Map::new();
    for dependency in dependencies {
        let value = dag
            .parents
            .get(dependency)
            .cloned()
            .flatten()
            .map(Value::String)
            .unwrap_or(Value::Null);
        parents.insert((*dependency).to_string(), value);
    }
    let payload = json!({ "projection": projection, "parents": Value::Object(parents) });
    Ok(hash_value(&payload))
}

/// Outcome of [`ProjectionCache::get_or_build`], matching the legacy
/// `{ value, cache, fingerprint }` shape's `cache` discriminant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheOutcome {
    Hit,
    Miss,
    Invalidated,
}

struct CacheEntry<T> {
    fingerprint: String,
    value: T,
    /// Monotonically increasing insertion order, used to evict the oldest
    /// entry (Map insertion order in the legacy JS `Map`).
    order: u64,
}

/// Bounded per-projection build cache. Ports the legacy `ProjectionCache`
/// class: builds are keyed by projection name and invalidated whenever the
/// projection's fingerprint (over its declared parents only) changes.
pub struct ProjectionCache<T> {
    max_entries: usize,
    entries: HashMap<String, CacheEntry<T>>,
    next_order: u64,
}

impl<T: Clone> ProjectionCache<T> {
    pub fn new(max_entries: usize) -> Self {
        Self { max_entries: max_entries.max(1), entries: HashMap::new(), next_order: 0 }
    }

    /// Build (or reuse) the cached value for `projection` under `dag`,
    /// calling `builder` only on a cache miss or invalidation.
    pub fn get_or_build(
        &mut self,
        projection: &str,
        dag: &ProjectionDependencyDag,
        builder: impl FnOnce() -> T,
    ) -> Result<(T, CacheOutcome, String), ProjectionUnknown> {
        let fingerprint = projection_fingerprint(dag, projection)?;
        if let Some(existing) = self.entries.get(projection) {
            if existing.fingerprint == fingerprint {
                return Ok((existing.value.clone(), CacheOutcome::Hit, fingerprint));
            }
        }
        let outcome = if self.entries.contains_key(projection) {
            CacheOutcome::Invalidated
        } else {
            CacheOutcome::Miss
        };
        let value = builder();
        let order = self.next_order;
        self.next_order += 1;
        self.entries.insert(
            projection.to_string(),
            CacheEntry { fingerprint: fingerprint.clone(), value: value.clone(), order },
        );
        while self.entries.len() > self.max_entries {
            if let Some(oldest_key) = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.order)
                .map(|(key, _)| key.clone())
            {
                self.entries.remove(&oldest_key);
            } else {
                break;
            }
        }
        Ok((value, outcome, fingerprint))
    }

    /// Drop cache entries for every projection invalidated by
    /// `changed_parents`, returning that (sorted) projection list. Ports
    /// `ProjectionCache.invalidate`.
    pub fn invalidate<S: AsRef<str>>(&mut self, changed_parents: &[S]) -> Vec<String> {
        let projections = invalidated_projections(changed_parents);
        for projection in &projections {
            self.entries.remove(projection);
        }
        projections
    }
}
