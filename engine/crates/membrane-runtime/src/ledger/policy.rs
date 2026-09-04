//! Repository-local Git-ignore semantics shared by scan, query and resolution.
//!
//! Policy is deliberately explicit: nested .gitignore only, case-sensitive
//! matching, no global ignore files, no .ignore overrides, no followed symlinks.
//! A whitelist never overrides a source grant or mandatory exclusion.
use super::{limits::WorkBudget, resolve::digest};
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

pub const POLICY_VERSION: &str = "ledger.gitignore.repo-only.case-sensitive.v1";
const MAX_IGNORE_BYTES: usize = 64 * 1024;
const MAX_DOCUMENTS: usize = 16_384;

pub fn mandatory_exclusion(relative: &Path) -> bool {
    relative.components().filter_map(|c| c.as_os_str().to_str()).any(|name| {
        let name = name.to_ascii_lowercase();
        matches!(name.as_str(), ".git" | "node_modules" | "target" | ".cache" |
            ".venv" | "vendor" | "memory-mirror" | "health" | "memory.md")
    })
}

pub struct SourcePolicy {
    root: PathBuf,
    matchers: BTreeMap<PathBuf, Gitignore>,
    observations: BTreeMap<PathBuf, Option<String>>,
}
impl SourcePolicy {
    pub fn new(root: &Path) -> Result<Self, String> {
        let root = root.canonicalize().map_err(|_| "ledger_root_unavailable")?;
        Ok(Self { root, matchers: BTreeMap::new(), observations: BTreeMap::new() })
    }
    fn matcher(&mut self, directory: &Path, budget: &WorkBudget) -> Result<(), String> {
        if self.matchers.contains_key(directory) { return Ok(()); }
        budget.visit()?;
        let path = directory.join(".gitignore");
        let bytes = read_policy_file(&path)?;
        let mut builder = GitignoreBuilder::new(directory);
        if let Some(bytes) = &bytes {
            budget.charge_bytes(bytes.len())?;
            let text = std::str::from_utf8(bytes).map_err(|_| "ledger_ignore_unsupported_encoding")?;
            for line in text.lines() {
                builder.add_line(Some(path.clone()), line).map_err(|_| "ledger_ignore_invalid")?;
            }
        }
        let matcher = builder.build().map_err(|_| "ledger_ignore_invalid")?;
        self.observations.insert(path, bytes.as_deref().map(digest));
        self.matchers.insert(directory.to_owned(), matcher);
        Ok(())
    }
    pub fn allows(&mut self, relative: &str, is_directory: bool, budget: &WorkBudget) -> Result<bool, String> {
        budget.check()?;
        let relative_path = Path::new(relative);
        if relative.is_empty() || relative.contains('\\') || relative.len() > 4096
            || relative.chars().any(char::is_control)
            || relative_path.components().any(|c| !matches!(c, Component::Normal(_)))
        { return Err("ledger_path_denied".into()); }
        if mandatory_exclusion(relative_path) { return Ok(false); }
        let components = relative_path.components().collect::<Vec<_>>();
        if components.len() > 64 { return Err("ledger_depth_budget_exhausted".into()); }
        let mut parent = self.root.clone();
        let mut ancestors = Vec::new();
        for (index, component) in components.iter().enumerate() {
            self.matcher(&parent, budget)?;
            ancestors.push(parent.clone());
            let candidate = parent.join(component.as_os_str());
            let is_dir = index + 1 < components.len() || is_directory;
            // The nearest matching rule wins. An excluded parent is never
            // traversed, so a nested file cannot re-include itself illegally.
            for ancestor in ancestors.iter().rev() {
                let matched = self.matchers[ancestor].matched(&candidate, is_dir);
                if matched.is_ignore() { return Ok(false); }
                if matched.is_whitelist() { break; }
            }
            parent = candidate;
        }
        Ok(true)
    }
    pub fn digest(&self) -> String {
        let mut bytes = POLICY_VERSION.as_bytes().to_vec();
        for (path, hash) in &self.observations {
            bytes.extend_from_slice(path.strip_prefix(&self.root).unwrap_or(path).to_string_lossy().as_bytes());
            bytes.push(0);
            bytes.extend_from_slice(hash.as_deref().unwrap_or("missing").as_bytes());
            bytes.push(0);
        }
        digest(&bytes)
    }
    pub fn revalidate(&self, budget: &WorkBudget) -> Result<(), String> {
        for (path, expected) in &self.observations {
            budget.check()?;
            let bytes = read_policy_file(path)?;
            if let Some(bytes) = &bytes { budget.charge_bytes(bytes.len())?; }
            if bytes.as_deref().map(digest) != *expected { return Err("ledger_policy_changed".into()); }
        }
        Ok(())
    }
}

fn read_policy_file(path: &Path) -> Result<Option<Vec<u8>>, String> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(value) => value,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("ledger_ignore_unavailable".into()),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() { return Err("ledger_ignore_denied".into()); }
    if metadata.len() > MAX_IGNORE_BYTES as u64 { return Err("ledger_ignore_budget_exhausted".into()); }
    let mut bytes = Vec::new();
    std::fs::File::open(path).map_err(|_| "ledger_ignore_unavailable")?
        .take((MAX_IGNORE_BYTES + 1) as u64).read_to_end(&mut bytes).map_err(|_| "ledger_ignore_unavailable")?;
    if bytes.len() > MAX_IGNORE_BYTES { return Err("ledger_ignore_budget_exhausted".into()); }
    Ok(Some(bytes))
}

pub(crate) fn walk_markdown(root: &Path, budget: &WorkBudget) -> Result<(Vec<PathBuf>, usize, SourcePolicy), String> {
    let mut policy = SourcePolicy::new(root)?;
    let mut pending = vec![(root.to_owned(), 0usize)];
    let mut files = Vec::new();
    let mut excluded_health = 0;
    while let Some((directory, depth)) = pending.pop() {
        budget.check()?;
        if depth > 64 { return Err("ledger_depth_budget_exhausted".into()); }
        let mut entries = Vec::new();
        for entry in std::fs::read_dir(&directory).map_err(|_| "ledger_scan_unavailable")? {
            budget.visit()?;
            entries.push(entry.map_err(|_| "ledger_scan_unavailable")?);
        }
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            budget.check()?;
            let path = entry.path();
            let kind = entry.file_type().map_err(|_| "ledger_scan_unavailable")?;
            if kind.is_symlink() { continue; }
            let relative = path.strip_prefix(root).map_err(|_| "ledger_path_denied")?
                .to_str().ok_or("ledger_path_unsupported")?.replace('\\', "/");
            if Path::new(&relative).components().any(|c| c.as_os_str().to_str().is_some_and(|v| v.eq_ignore_ascii_case("health"))) {
                excluded_health += 1;
            }
            if !policy.allows(&relative, kind.is_dir(), budget)? { continue; }
            if kind.is_dir() { pending.push((path, depth + 1)); }
            else if kind.is_file() && path.extension().and_then(|s| s.to_str()).is_some_and(|s| s.eq_ignore_ascii_case("md")) {
                files.push(path);
                if files.len() > MAX_DOCUMENTS { return Err("ledger_document_budget_exhausted".into()); }
            }
        }
    }
    files.sort();
    policy.revalidate(budget)?;
    Ok((files, excluded_health, policy))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    #[test]
    fn glob_negation_nested_rules_and_mandatory_exclusions() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("docs")).unwrap();
        std::fs::write(root.path().join(".gitignore"), "*.private.md\n!allowed.private.md\nignored/\n!memory.md\n").unwrap();
        std::fs::write(root.path().join("docs/.gitignore"), "nested.md\n!local.private.md\n").unwrap();
        let budget = WorkBudget::bounded(Duration::from_secs(5));
        let mut policy = SourcePolicy::new(root.path()).unwrap();
        assert!(!policy.allows("hidden.private.md", false, &budget).unwrap());
        assert!(policy.allows("allowed.private.md", false, &budget).unwrap());
        assert!(!policy.allows("docs/nested.md", false, &budget).unwrap());
        assert!(policy.allows("docs/local.private.md", false, &budget).unwrap());
        assert!(!policy.allows("ignored/child.md", false, &budget).unwrap());
        assert!(!policy.allows("memory.md", false, &budget).unwrap());
        std::fs::write(root.path().join(".gitignore"), "new-rule\n").unwrap();
        assert_eq!(policy.revalidate(&budget).unwrap_err(), "ledger_policy_changed");
    }
}
