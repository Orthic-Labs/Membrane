//! Core data types for the memory tier system.

use serde::{Deserialize, Serialize};

/// The tier a memory entry lives in. Tiers form a hierarchy where entries can be
/// promoted from `Working` -> `Episodic` -> `Semantic`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MemoryTier {
    Working,
    Episodic,
    Semantic,
}

impl MemoryTier {
    /// Ordinal rank used to decide whether a promotion moves to a higher tier.
    pub fn rank(self) -> u8 {
        match self {
            MemoryTier::Working => 0,
            MemoryTier::Episodic => 1,
            MemoryTier::Semantic => 2,
        }
    }
}

/// A single memory entry stored in the registry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub id: String,
    pub tier: MemoryTier,
    pub content: String,
    pub keywords: Vec<String>,
    /// Relevance/quality score.
    pub score: f64,
    pub created_at: String,
    pub access_count: u32,
    /// Semantic embedding of `content` (L2-normalized). `None` until embedded;
    /// `#[serde(default)]` keeps older persisted entries loadable.
    #[serde(default)]
    pub embedding: Option<Vec<f32>>,
    /// Project/scope this memory belongs to (Claude project-slug, or `global`). Lets ONE store hold
    /// many projects with isolation — the multi-project dimension the workspace needs and CR (a
    /// many-repo Claude Code replacement) will too. `#[serde(default)]` → `global` keeps old entries
    /// and single-scope use unchanged.
    #[serde(default = "default_scope")]
    pub scope_id: String,
}

/// Default scope for entries with no explicit project (old rows, single-repo use).
pub fn default_scope() -> String {
    "global".to_string()
}
