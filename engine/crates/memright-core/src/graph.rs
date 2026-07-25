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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEdge {
    /// Source node id.
    pub from: String,
    /// Target node id.
    pub to: String,
    /// Edge label (e.g., "causally_related", "implements", "uses").
    pub relation: String,
    /// When this edge was created.
    pub created_at: String,
}

/// In-process bi-temporal knowledge graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryGraph {
    nodes: HashMap<String, MemoryNode>,
    edges: Vec<MemoryEdge>,
}

impl MemoryGraph {
    /// Create a new empty graph.
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            edges: Vec::new(),
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
}
