//! Graph-structured in-memory knowledge graph over memory entries.
//!
//! Implements a bi-temporal knowledge graph (Zep/Graphiti-inspired, in-process).
//! Supports:
//! - Nodes with validity windows (created_at, valid_from, valid_to)
//! - Labeled edges between nodes
//! - Neighbor expansion and hybrid search
//! - Temporal slices (query valid entries at a timestamp)

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A memory node in the graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryNode {
    /// Unique identifier.
    pub id: String,
    /// Node content (text).
    pub content: String,
    /// When this node was created.
    pub created_at: String,
    /// Timestamp from which this node becomes valid (inclusive).
    pub valid_from: String,
    /// Timestamp after which this node is no longer valid (exclusive).
    /// None means indefinitely valid.
    pub valid_to: Option<String>,
}

/// A directed edge between two nodes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryEdge {
    /// Source node id.
    pub from: String,
    /// Target node id.
    pub to: String,
    /// Edge label (e.g., "causally_related", "implements", "uses").
    ///
    /// Raw ingest may carry any label, but only [`CANONICAL_RELATIONS`] edges
    /// with resolved endpoints are traversable (see [`MemoryGraph::canonical_neighbors`]).
    /// Everything else is diagnostic and never authorizes traversal.
    pub relation: String,
    /// When this edge was created.
    pub created_at: String,
}

/// CTX-017 closed relation vocabulary. Only resolved, provenance-bound,
/// lifecycle-applicable evidence relations in this set are traversable.
/// Dangling, unresolved, or raw wikilink relations stay diagnostic and
/// non-traversable. This set must not be widened to enable CTX-039 style
/// multi-hop evidence traversal without explicit promotion.
pub const CANONICAL_RELATIONS: &[&str] = &["supports", "contradicts", "supersedes", "derived_from"];

/// True when `relation` is a member of the closed CTX-017 vocabulary.
pub fn is_canonical_relation(relation: &str) -> bool {
    CANONICAL_RELATIONS.contains(&relation)
}

/// Rejection reason for a governed edge admission attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeRejection {
    /// Relation is outside the closed CTX-017 vocabulary.
    NonCanonicalRelation,
    /// An endpoint has no resolved node in this graph.
    DanglingEndpoint,
    /// Producer or reference missing: an evidence edge cannot be
    /// provenance-free (CTX-017).
    MissingProvenance,
}

/// Where a canonical evidence relation came from. Every durable edge carries
/// one; an edge without provenance is not admissible.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelationProvenance {
    /// Producer/subsystem identity that asserted the edge.
    pub producer: String,
    /// The specific evidence: event id, digest, or document reference.
    pub reference: String,
}

impl RelationProvenance {
    pub fn new(producer: impl Into<String>, reference: impl Into<String>) -> Self {
        Self {
            producer: producer.into(),
            reference: reference.into(),
        }
    }

    fn is_bound(&self) -> bool {
        !self.producer.trim().is_empty() && !self.reference.trim().is_empty()
    }
}

/// A provenance-bound canonical evidence relation held in the graph. Mirrors
/// the durable `memory_relation` row in `cortex_store::memdb`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceRelation {
    pub edge: MemoryEdge,
    pub provenance: RelationProvenance,
}

/// Why a stored evidence relation does not traverse as live evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RelationDiagnostic {
    /// Target has no resolved node.
    DanglingTarget,
    /// Target resolves but is not valid at the traversal timestamp
    /// (invalidated / superseded / suppressed window).
    LifecycleIneligible,
}

/// In-process bi-temporal knowledge graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryGraph {
    nodes: HashMap<String, MemoryNode>,
    edges: Vec<MemoryEdge>,
    /// CTX-017 provenance-bound evidence relations. Kept separate from the
    /// permissive `edges` list so raw ingest can never masquerade as governed
    /// evidence.
    #[serde(default)]
    relations: Vec<EvidenceRelation>,
}

impl MemoryGraph {
    /// Create a new empty graph.
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            edges: Vec::new(),
            relations: Vec::new(),
        }
    }

    /// Add a node to the graph.
    pub fn add_node(&mut self, node: MemoryNode) {
        self.nodes.insert(node.id.clone(), node);
    }

    /// Add an edge to the graph.
    pub fn add_edge(&mut self, edge: MemoryEdge) {
        // Ensure both endpoints exist or allow dangling edges (graph is flexible).
        self.edges.push(edge);
    }

    /// Admit a governed evidence edge. Only closed-vocabulary relations with
    /// both endpoints resolved are accepted; anything else is rejected with a
    /// typed reason and must remain diagnostic, never traversable.
    pub fn add_canonical_edge(&mut self, edge: MemoryEdge) -> Result<(), EdgeRejection> {
        if !is_canonical_relation(&edge.relation) {
            return Err(EdgeRejection::NonCanonicalRelation);
        }
        if !self.nodes.contains_key(&edge.from) || !self.nodes.contains_key(&edge.to) {
            return Err(EdgeRejection::DanglingEndpoint);
        }
        self.edges.push(edge);
        Ok(())
    }

    /// Admit a provenance-bound evidence relation (CTX-017). The relation must
    /// be in the closed vocabulary, carry non-empty provenance, and have a
    /// resolved source. A **dangling target is accepted and retained as a
    /// diagnostic**, mirroring durable storage: it is visible through
    /// [`MemoryGraph::relation_diagnostics`] but never traverses.
    pub fn add_evidence_relation(
        &mut self,
        relation: EvidenceRelation,
    ) -> Result<(), EdgeRejection> {
        if !is_canonical_relation(&relation.edge.relation) {
            return Err(EdgeRejection::NonCanonicalRelation);
        }
        if !relation.provenance.is_bound() {
            return Err(EdgeRejection::MissingProvenance);
        }
        if !self.nodes.contains_key(&relation.edge.from) {
            return Err(EdgeRejection::DanglingEndpoint);
        }
        self.relations.retain(|existing| {
            existing.edge.from != relation.edge.from
                || existing.edge.to != relation.edge.to
                || existing.edge.relation != relation.edge.relation
        });
        self.relations.push(relation);
        Ok(())
    }

    /// Every stored evidence relation on `id` with its disposition at
    /// `timestamp`: `None` means live traversable evidence. This is the
    /// diagnostic view; dangling and lifecycle-ineligible edges appear only
    /// here.
    pub fn relation_diagnostics(
        &self,
        id: &str,
        timestamp: &str,
    ) -> Vec<(&EvidenceRelation, Option<RelationDiagnostic>)> {
        self.relations
            .iter()
            .filter(|relation| relation.edge.from == id)
            .map(|relation| {
                let diagnostic = match self.nodes.get(&relation.edge.to) {
                    None => Some(RelationDiagnostic::DanglingTarget),
                    Some(node) => {
                        let live = node.valid_from.as_str() <= timestamp
                            && node
                                .valid_to
                                .as_ref()
                                .is_none_or(|until| until.as_str() > timestamp);
                        if live {
                            None
                        } else {
                            Some(RelationDiagnostic::LifecycleIneligible)
                        }
                    }
                };
                (relation, diagnostic)
            })
            .collect()
    }

    /// Traversal view: only resolved, provenance-bound, lifecycle-applicable
    /// evidence relations valid at `timestamp`.
    pub fn evidence_neighbors(&self, id: &str, timestamp: &str) -> Vec<(&EvidenceRelation, &MemoryNode)> {
        self.relation_diagnostics(id, timestamp)
            .into_iter()
            .filter(|(_, diagnostic)| diagnostic.is_none())
            .filter_map(|(relation, _)| {
                self.nodes
                    .get(&relation.edge.to)
                    .map(|node| (relation, node))
            })
            .collect()
    }

    /// All stored evidence relations, regardless of disposition.
    pub fn all_evidence_relations(&self) -> &[EvidenceRelation] {
        &self.relations
    }

    /// True when an edge is traversable evidence: closed-vocabulary relation
    /// with both endpoints resolved in this graph.
    pub fn is_traversable(&self, edge: &MemoryEdge) -> bool {
        is_canonical_relation(&edge.relation)
            && self.nodes.contains_key(&edge.from)
            && self.nodes.contains_key(&edge.to)
    }

    /// Get immediate neighbors of a node (1-hop expansion).
    /// Returns (edge relation, neighbor node) pairs.
    pub fn neighbors(&self, id: &str) -> Vec<(String, &MemoryNode)> {
        let mut result = Vec::new();

        for edge in &self.edges {
            if edge.from == id {
                if let Some(neighbor) = self.nodes.get(&edge.to) {
                    result.push((edge.relation.clone(), neighbor));
                }
            }
        }

        result
    }

    /// Traversal-safe neighbor view for governed evidence paths. Only
    /// closed-vocabulary edges with resolved endpoints are returned; raw,
    /// dangling, or non-canonical edges are excluded without error.
    pub fn canonical_neighbors(&self, id: &str) -> Vec<(String, &MemoryNode)> {
        self.neighbors(id)
            .into_iter()
            .filter(|(relation, neighbor)| {
                is_canonical_relation(relation)
                    && self.nodes.contains_key(id)
                    && self.nodes.contains_key(&neighbor.id)
            })
            .collect()
    }

    /// Mark a node as no longer valid from a given timestamp.
    pub fn invalidate(&mut self, id: &str, at: String) {
        if let Some(node) = self.nodes.get_mut(id) {
            node.valid_to = Some(at);
        }
    }

    /// Get all nodes valid at a specific timestamp (temporal slice).
    /// A node is valid at time T if:
    /// - valid_from <= T
    /// - valid_to is None OR valid_to > T
    pub fn valid_at(&self, timestamp: &str) -> Vec<&MemoryNode> {
        self.nodes
            .values()
            .filter(|node| {
                node.valid_from.as_str() <= timestamp
                    && node
                        .valid_to
                        .as_ref()
                        .is_none_or(|vto| vto.as_str() > timestamp)
            })
            .collect()
    }

    /// Hybrid search: keyword search in node content + 1-hop graph expansion.
    /// Returns seed matches plus their immediate neighbors.
    pub fn hybrid_search(&self, query: &str, k: usize) -> Vec<&MemoryNode> {
        let query_lower = query.to_lowercase();
        let mut result_ids = std::collections::HashSet::new();

        // Phase 1: keyword matches (seed).
        for (id, node) in &self.nodes {
            if node.content.to_lowercase().contains(&query_lower) {
                result_ids.insert(id.clone());
            }
        }

        // Phase 2: 1-hop expansion from seeds.
        let seed_ids: Vec<String> = result_ids.iter().cloned().collect();
        for seed_id in seed_ids {
            for (_, neighbor) in self.neighbors(&seed_id) {
                result_ids.insert(neighbor.id.clone());
            }
        }

        // Collect and return up to k nodes.
        result_ids
            .into_iter()
            .take(k)
            .filter_map(|id| self.nodes.get(&id))
            .collect()
    }

    /// Get all nodes (for iteration).
    pub fn all_nodes(&self) -> Vec<&MemoryNode> {
        self.nodes.values().collect()
    }

    /// Get all edges.
    pub fn all_edges(&self) -> Vec<&MemoryEdge> {
        self.edges.iter().collect()
    }
}

impl Default for MemoryGraph {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_node(id: &str, content: &str) -> MemoryNode {
        MemoryNode {
            id: id.to_string(),
            content: content.to_string(),
            created_at: "2026-06-01T00:00:00Z".to_string(),
            valid_from: "2026-06-01T00:00:00Z".to_string(),
            valid_to: None,
        }
    }

    fn make_edge(from: &str, to: &str, relation: &str) -> MemoryEdge {
        MemoryEdge {
            from: from.to_string(),
            to: to.to_string(),
            relation: relation.to_string(),
            created_at: "2026-06-01T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn graph_add_node_and_retrieve() {
        let mut graph = MemoryGraph::new();
        let node = make_node("n1", "hello world");
        graph.add_node(node);
        assert_eq!(graph.all_nodes().len(), 1);
    }

    #[test]
    fn graph_add_edge() {
        let mut graph = MemoryGraph::new();
        graph.add_node(make_node("n1", "foo"));
        graph.add_node(make_node("n2", "bar"));
        graph.add_edge(make_edge("n1", "n2", "related"));
        assert_eq!(graph.all_edges().len(), 1);
    }

    #[test]
    fn graph_neighbors_finds_direct_outgoing() {
        let mut graph = MemoryGraph::new();
        graph.add_node(make_node("n1", "foo"));
        graph.add_node(make_node("n2", "bar"));
        graph.add_node(make_node("n3", "baz"));
        graph.add_edge(make_edge("n1", "n2", "related"));
        graph.add_edge(make_edge("n1", "n3", "depends_on"));

        let neighbors = graph.neighbors("n1");
        assert_eq!(neighbors.len(), 2);
        assert!(neighbors.iter().any(|(_, n)| n.id == "n2"));
        assert!(neighbors.iter().any(|(_, n)| n.id == "n3"));
    }

    #[test]
    fn graph_neighbors_empty_for_leaf() {
        let mut graph = MemoryGraph::new();
        graph.add_node(make_node("n1", "foo"));
        graph.add_node(make_node("n2", "bar"));
        graph.add_edge(make_edge("n1", "n2", "related"));

        let neighbors = graph.neighbors("n2");
        assert_eq!(neighbors.len(), 0);
    }

    #[test]
    fn graph_neighbors_ignore_dangling_edges() {
        let mut graph = MemoryGraph::new();
        graph.add_node(make_node("n1", "foo"));
        graph.add_edge(make_edge("n1", "missing", "related"));

        let neighbors = graph.neighbors("n1");
        assert!(neighbors.is_empty());
        assert_eq!(graph.all_edges().len(), 1);
    }

    #[test]
    fn graph_invalidate_sets_valid_to() {
        let mut graph = MemoryGraph::new();
        graph.add_node(make_node("n1", "content"));
        assert!(graph.nodes.get("n1").unwrap().valid_to.is_none());

        graph.invalidate("n1", "2026-06-05T00:00:00Z".to_string());
        assert_eq!(
            graph.nodes.get("n1").unwrap().valid_to,
            Some("2026-06-05T00:00:00Z".to_string())
        );
    }

    #[test]
    fn graph_valid_at_before_valid_from() {
        let mut graph = MemoryGraph::new();
        let mut node = make_node("n1", "content");
        node.valid_from = "2026-06-05T00:00:00Z".to_string();
        graph.add_node(node);

        let valid = graph.valid_at("2026-06-01T00:00:00Z");
        assert_eq!(valid.len(), 0); // Node not yet valid
    }

    #[test]
    fn graph_valid_at_within_window() {
        let mut graph = MemoryGraph::new();
        let mut node = make_node("n1", "content");
        node.valid_from = "2026-06-01T00:00:00Z".to_string();
        node.valid_to = Some("2026-06-10T00:00:00Z".to_string());
        graph.add_node(node);

        let valid = graph.valid_at("2026-06-05T00:00:00Z");
        assert_eq!(valid.len(), 1);
    }

    #[test]
    fn graph_valid_at_after_valid_to() {
        let mut graph = MemoryGraph::new();
        let mut node = make_node("n1", "content");
        node.valid_from = "2026-06-01T00:00:00Z".to_string();
        node.valid_to = Some("2026-06-10T00:00:00Z".to_string());
        graph.add_node(node);

        let valid = graph.valid_at("2026-06-15T00:00:00Z");
        assert_eq!(valid.len(), 0); // Node has expired
    }

    #[test]
    fn graph_valid_at_treats_valid_to_as_exclusive_boundary() {
        let mut graph = MemoryGraph::new();
        let mut node = make_node("n1", "content");
        node.valid_from = "2026-06-01T00:00:00Z".to_string();
        node.valid_to = Some("2026-06-10T00:00:00Z".to_string());
        graph.add_node(node);

        let valid = graph.valid_at("2026-06-10T00:00:00Z");
        assert!(valid.is_empty());
    }

    #[test]
    fn graph_valid_at_indefinite_validity() {
        let mut graph = MemoryGraph::new();
        let node = make_node("n1", "content");
        graph.add_node(node);

        let valid = graph.valid_at("2026-12-31T23:59:59Z");
        assert_eq!(valid.len(), 1); // Valid indefinitely
    }

    #[test]
    fn graph_hybrid_search_keyword_match() {
        let mut graph = MemoryGraph::new();
        graph.add_node(make_node("n1", "rust async await"));
        graph.add_node(make_node("n2", "python django"));

        let results = graph.hybrid_search("rust", 10);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "n1");
    }

    #[test]
    fn graph_hybrid_search_with_neighbor_expansion() {
        let mut graph = MemoryGraph::new();
        graph.add_node(make_node("n1", "rust async"));
        graph.add_node(make_node("n2", "tokio runtime"));
        graph.add_node(make_node("n3", "python django"));
        // n1 -> n2 (depends on)
        graph.add_edge(make_edge("n1", "n2", "depends_on"));

        let results = graph.hybrid_search("rust", 10);
        // Should return n1 (keyword match) + n2 (neighbor).
        assert_eq!(results.len(), 2);
        let ids: Vec<_> = results.iter().map(|n| n.id.as_str()).collect();
        assert!(ids.contains(&"n1"));
        assert!(ids.contains(&"n2"));
    }

    #[test]
    fn graph_hybrid_search_respects_k() {
        let mut graph = MemoryGraph::new();
        for i in 0..10 {
            graph.add_node(make_node(&format!("n{i}"), "rust content"));
        }

        let results = graph.hybrid_search("rust", 3);
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn graph_hybrid_search_is_case_insensitive() {
        let mut graph = MemoryGraph::new();
        graph.add_node(make_node("n1", "Rust Async Runtime"));

        let results = graph.hybrid_search("rust async", 10);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "n1");
    }

    #[test]
    fn graph_hybrid_search_zero_limit_returns_no_results() {
        let mut graph = MemoryGraph::new();
        graph.add_node(make_node("n1", "rust content"));

        let results = graph.hybrid_search("rust", 0);
        assert!(results.is_empty());
    }

    #[test]
    fn graph_hybrid_search_empty() {
        let graph = MemoryGraph::new();
        let results = graph.hybrid_search("nonexistent", 10);
        assert_eq!(results.len(), 0);
    }

    #[test]
    fn ctx017_closed_vocabulary_gates_traversal_without_breaking_diagnostic_ingest() {
        assert!(is_canonical_relation("supports"));
        assert!(is_canonical_relation("contradicts"));
        assert!(is_canonical_relation("supersedes"));
        assert!(is_canonical_relation("derived_from"));
        assert!(!is_canonical_relation("related"));
        assert!(!is_canonical_relation("depends_on"));
        assert!(!is_canonical_relation("wikilink"));

        let mut graph = MemoryGraph::new();
        graph.add_node(make_node("n1", "foo"));
        graph.add_node(make_node("n2", "bar"));
        // Diagnostic ingest still accepts raw labels and dangling endpoints.
        graph.add_edge(make_edge("n1", "n2", "related"));
        graph.add_edge(make_edge("n1", "missing", "supports"));
        assert_eq!(graph.all_edges().len(), 2);
        // Governed admission rejects both failure modes with typed reasons.
        assert_eq!(
            graph.add_canonical_edge(make_edge("n1", "n2", "related")).unwrap_err(),
            EdgeRejection::NonCanonicalRelation
        );
        assert_eq!(
            graph.add_canonical_edge(make_edge("n1", "missing", "supports")).unwrap_err(),
            EdgeRejection::DanglingEndpoint
        );
        graph.add_canonical_edge(make_edge("n1", "n2", "supports")).unwrap();
        // Traversal-safe view exposes only the canonical resolved edge.
        let canonical = graph.canonical_neighbors("n1");
        assert_eq!(canonical.len(), 1);
        assert_eq!(canonical[0].0, "supports");
        assert_eq!(canonical[0].1.id, "n2");
        assert!(graph.canonical_neighbors("missing").is_empty());
        // Legacy tolerant view is unchanged for diagnostic consumers.
        assert_eq!(graph.neighbors("n1").len(), 2);
    }

    // ---- CTX-017: provenance-bound evidence relations -----------------------

    fn evidence(from: &str, to: &str, relation: &str) -> EvidenceRelation {
        EvidenceRelation {
            edge: make_edge(from, to, relation),
            provenance: RelationProvenance::new("cortex.ingest", "event.abc123"),
        }
    }

    const NOW: &str = "2026-09-07T00:00:00Z";

    #[test]
    fn each_canonical_relation_traverses_when_resolved_and_live() {
        let mut graph = MemoryGraph::new();
        graph.add_node(make_node("a", "source"));
        for relation in CANONICAL_RELATIONS {
            graph.add_node(make_node(relation, "target"));
            graph
                .add_evidence_relation(evidence("a", relation, relation))
                .expect("admitted");
        }
        let mut kinds: Vec<&str> = graph
            .evidence_neighbors("a", NOW)
            .into_iter()
            .map(|(relation, _)| relation.edge.relation.as_str())
            .collect();
        kinds.sort_unstable();
        let mut expected = CANONICAL_RELATIONS.to_vec();
        expected.sort_unstable();
        assert_eq!(kinds, expected);
        assert!(graph
            .evidence_neighbors("a", NOW)
            .iter()
            .all(|(relation, _)| relation.provenance.producer == "cortex.ingest"));
    }

    #[test]
    fn dangling_evidence_relation_is_diagnostic_but_never_traversable() {
        let mut graph = MemoryGraph::new();
        graph.add_node(make_node("a", "source"));
        graph
            .add_evidence_relation(evidence("a", "missing", "derived_from"))
            .expect("retained as diagnostic");
        let diagnostics = graph.relation_diagnostics("a", NOW);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].1, Some(RelationDiagnostic::DanglingTarget));
        assert!(graph.evidence_neighbors("a", NOW).is_empty());
    }

    #[test]
    fn lifecycle_ineligible_target_is_diagnostic_but_never_traversable() {
        let mut graph = MemoryGraph::new();
        graph.add_node(make_node("a", "source"));
        graph.add_node(make_node("b", "target"));
        graph
            .add_evidence_relation(evidence("a", "b", "contradicts"))
            .expect("admitted");
        assert_eq!(graph.evidence_neighbors("a", NOW).len(), 1);

        graph.invalidate("b", "2026-08-01T00:00:00Z".to_string());
        let diagnostics = graph.relation_diagnostics("a", NOW);
        assert_eq!(diagnostics.len(), 1, "still visible diagnostically");
        assert_eq!(
            diagnostics[0].1,
            Some(RelationDiagnostic::LifecycleIneligible)
        );
        assert!(graph.evidence_neighbors("a", NOW).is_empty());
    }

    #[test]
    fn evidence_relations_require_vocabulary_provenance_and_resolved_source() {
        let mut graph = MemoryGraph::new();
        graph.add_node(make_node("a", "source"));
        graph.add_node(make_node("b", "target"));
        assert_eq!(
            graph.add_evidence_relation(evidence("a", "b", "mentions")),
            Err(EdgeRejection::NonCanonicalRelation)
        );
        let mut unprovenanced = evidence("a", "b", "supports");
        unprovenanced.provenance.reference = "  ".to_string();
        assert_eq!(
            graph.add_evidence_relation(unprovenanced),
            Err(EdgeRejection::MissingProvenance)
        );
        assert_eq!(
            graph.add_evidence_relation(evidence("nobody", "b", "supports")),
            Err(EdgeRejection::DanglingEndpoint)
        );
        assert!(graph.all_evidence_relations().is_empty());

        // Re-admitting the same triple refreshes rather than duplicating.
        graph
            .add_evidence_relation(evidence("a", "b", "supports"))
            .unwrap();
        let mut refreshed = evidence("a", "b", "supports");
        refreshed.provenance.reference = "event.def456".to_string();
        graph.add_evidence_relation(refreshed).unwrap();
        assert_eq!(graph.all_evidence_relations().len(), 1);
        assert_eq!(
            graph.all_evidence_relations()[0].provenance.reference,
            "event.def456"
        );
    }
}
