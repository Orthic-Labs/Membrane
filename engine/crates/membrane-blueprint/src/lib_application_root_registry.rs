//! Native port of `blueprint/src/lib/application/root-registry.mjs`.
//!
//! D06: canonical root registry enrollment and resolution. Uses
//! `crate::identity::repository_identity` as the native counterpart of the
//! legacy `repositoryIdentity` import from `graph/static-provider.mjs`.

use crate::identity::repository_identity;
use crate::lib_application_errors::BlueprintError;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Canonicalize a path the way the legacy `canonical()` helper does:
/// `resolve()` then `realpathSync.native()` when the path exists, else the
/// resolved (non-canonicalized) absolute path.
fn canonical(value: &str) -> String {
    let absolute = if Path::new(value).is_absolute() {
        PathBuf::from(value)
    } else {
        std::env::current_dir().unwrap_or_default().join(value)
    };
    match fs::canonicalize(&absolute) {
        Ok(real) => real.to_string_lossy().replace('\\', "/"),
        Err(_) => absolute.to_string_lossy().replace('\\', "/"),
    }
}

fn root_not_enrolled(repo_root: Option<&str>) -> BlueprintError {
    let normalized_root = repo_root.map(canonical);
    let next_operation = match &normalized_root {
        Some(root) => format!("blueprint init --root {}", serde_json::to_string(root).unwrap_or_default()),
        None => "blueprint init --root <repository-root>".to_owned(),
    };
    let message = match &normalized_root {
        Some(root) => format!("Blueprint root is not enrolled: {root}"),
        None => "No enrolled Blueprint repository matches this request.".to_owned(),
    };
    let mut details = serde_json::json!({});
    if let Some(root) = &normalized_root {
        details["normalizedRoot"] = serde_json::json!(root);
    }
    details["remediation"] = serde_json::json!({
        "summary": "Enroll the normalized Blueprint root before querying.",
        "nextOperation": next_operation,
        "arguments": normalized_root.as_ref().map(|root| serde_json::json!({"repoRoot": root})).unwrap_or_else(|| serde_json::json!({})),
    });
    BlueprintError::new("root_not_enrolled", message, Some(details))
}

#[derive(Debug, Clone)]
pub struct RootEntry {
    pub repo_id: String,
    pub root: String,
    pub installation_id: String,
    pub worktree_id: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Default)]
pub struct ResolveInput {
    pub repo_id: Option<String>,
    pub repo_root: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct AddInput {
    pub root: String,
    pub repo_id: Option<String>,
    pub installation_id: Option<String>,
    pub worktree_id: Option<String>,
    pub enabled: Option<bool>,
}

/// Native port of legacy `class RootRegistry`.
#[derive(Debug, Default)]
pub struct RootRegistry {
    by_repo_id: BTreeMap<String, RootEntry>,
    by_root: BTreeMap<String, RootEntry>,
}

impl RootRegistry {
    pub fn new(entries: Vec<AddInput>) -> Self {
        let mut registry = Self::default();
        for entry in entries {
            registry.add(entry);
        }
        registry
    }

    pub fn add(&mut self, entry: AddInput) -> RootEntry {
        let root = canonical(&entry.root);
        let identity = repository_identity(&root).ok();
        let repo_id = entry.repo_id.unwrap_or_else(|| identity.as_ref().map(|i| i.repo_id.clone()).unwrap_or_else(|| root.clone()));
        let installation_id = entry
            .installation_id
            .or_else(|| identity.as_ref().and_then(|i| i.installation_id.clone()))
            .unwrap_or_else(|| root.clone());
        let worktree_id = entry.worktree_id.unwrap_or_else(|| root.clone());
        let enabled = entry.enabled.unwrap_or(true);
        let normalized = RootEntry { repo_id: repo_id.clone(), root: root.clone(), installation_id, worktree_id, enabled };
        self.by_repo_id.insert(repo_id, normalized.clone());
        self.by_root.insert(root, normalized.clone());
        normalized
    }

    /// Mirrors `resolve({ repoId, repoRoot })`. Returns the canonical enrolled
    /// root or `root_not_enrolled` / `root_escape`.
    pub fn resolve(&self, input: &ResolveInput) -> Result<String, BlueprintError> {
        let by_id = input.repo_id.as_deref().and_then(|id| self.by_repo_id.get(id));
        let canonical_root = input.repo_root.as_deref().map(canonical);
        let by_path = canonical_root.as_deref().and_then(|root| self.by_root.get(root));

        // An explicit repoRoot that is not enrolled must never be silently
        // ignored, even when a repoId also resolves (D06: resolve only an
        // enrolled repoId or an exact enrolled root).
        if input.repo_root.is_some() && by_path.is_none() {
            return Err(root_not_enrolled(input.repo_root.as_deref()));
        }

        // The single-entry fallback applies only when the caller names no
        // explicit selector.
        let fallback = if input.repo_id.is_none() && input.repo_root.is_none() && self.by_repo_id.len() == 1 {
            self.by_repo_id.values().next()
        } else {
            None
        };
        let entry = by_id.or(by_path).or(fallback);
        let Some(entry) = entry else {
            return Err(root_not_enrolled(input.repo_root.as_deref()));
        };
        if !entry.enabled {
            return Err(root_not_enrolled(input.repo_root.as_deref()));
        }
        if let (Some(by_id), Some(by_path)) = (by_id, by_path) {
            if by_id.repo_id != by_path.repo_id {
                return Err(BlueprintError::new("root_escape", "Repository ID and root resolve to different enrollments.", None));
            }
        }
        Ok(entry.root.clone())
    }

    pub fn list(&self) -> Vec<RootEntry> {
        self.by_repo_id.values().cloned().collect()
    }
}
