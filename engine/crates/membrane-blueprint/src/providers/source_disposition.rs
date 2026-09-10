//! Port of `blueprint/src/providers/source-disposition.mjs`: audits the
//! exact tracked source universe used by the default clean-clone build and
//! gives every tracked path a terminal outcome. Successful indexed paths are
//! summarized rather than duplicated; exceptions remain per-path and
//! inspectable. Non-git roots report admitted-only scope instead of
//! claiming exhaustive discovery they cannot prove — same discipline as the
//! legacy module.

use serde::Serialize;
use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

/// Directory/segment names the primary scan excludes, ported verbatim from
/// `POLICY_SEGMENTS` in the legacy module (identical set, identical order
/// of intent — set membership only, order does not matter for behavior).
const POLICY_SEGMENTS: &[&str] = &[
    ".git", ".agent", ".audit", ".cache", ".next", ".nuxt", ".output", ".parcel-cache", ".pytest_cache",
    ".svelte-kit", ".turbo", ".vercel", ".worktrees", "__pycache__", ".gradle", ".idea", ".mypy_cache",
    ".ruff_cache", ".tox", ".vscode", ".yarn", ".pnpm-store", "coverage", "htmlcov", "node_modules",
    "target", "dist", "build", "out", "vendor", ".serverless", "fixture-repos",
];

const MAX_INDEXED_FILE_BYTES: u64 = 2 * 1024 * 1024;

fn normalize_path(value: &str) -> String {
    let replaced = value.replace('\\', "/");
    replaced.strip_prefix("./").map(str::to_owned).unwrap_or(replaced)
}

fn path_segments(path: &str) -> Vec<String> {
    normalize_path(path).split('/').filter(|s| !s.is_empty()).map(str::to_owned).collect()
}

/// Mirrors `trackedPaths`: `git -C root ls-files -z --cached --`. Returns
/// `None` exactly when the legacy code would (non-zero exit, or the root is
/// not a git worktree) so callers fall back to admitted-only scope.
fn tracked_paths(root: &Path) -> Option<Vec<String>> {
    let output = Command::new("git").arg("-C").arg(root).args(["ls-files", "-z", "--cached", "--"]).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut paths: Vec<String> = text.split('\0').map(normalize_path).filter(|p| !p.is_empty()).collect();
    paths.sort();
    Some(paths)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Disposition {
    IgnoredPolicy,
    Unsupported,
    Rejected,
    Failed,
}

impl Disposition {
    fn as_key(&self) -> &'static str {
        match self {
            Self::IgnoredPolicy => "ignored_policy",
            Self::Unsupported => "unsupported",
            Self::Rejected => "rejected",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Exception {
    pub path: String,
    pub disposition: Disposition,
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// Mirrors `classifyAbsent`: give a tracked-but-not-admitted path a typed
/// terminal outcome. Order of checks matches the legacy module exactly —
/// policy-segment exclusion first, then existence, then file-vs-directory,
/// then size, then a NUL-byte binary probe.
fn classify_absent(root: &Path, path: &str) -> Exception {
    let segments = path_segments(path);
    let policy_hit = segments.iter().any(|s| POLICY_SEGMENTS.contains(&s.as_str()))
        || segments.first().is_some_and(|first| first == ".agent" || first.starts_with(".agent-"));
    if policy_hit {
        return Exception { path: path.to_owned(), disposition: Disposition::IgnoredPolicy, reason: "primary_scan_exclusion".into(), size: None, detail: None };
    }
    let absolute = root.join(path);
    // Node's statSync follows symlinks; metadata preserves that parity.
    let meta = match std::fs::metadata(&absolute) {
        Ok(meta) => meta,
        Err(_) => {
            return Exception { path: path.to_owned(), disposition: Disposition::Failed, reason: "tracked_path_missing".into(), size: None, detail: None };
        }
    };
    if !meta.is_file() {
        return Exception { path: path.to_owned(), disposition: Disposition::IgnoredPolicy, reason: "not_regular_file".into(), size: None, detail: None };
    }
    if meta.len() > MAX_INDEXED_FILE_BYTES {
        return Exception { path: path.to_owned(), disposition: Disposition::IgnoredPolicy, reason: "file_too_large".into(), size: Some(meta.len()), detail: None };
    }
    match std::fs::read(&absolute) {
        Ok(bytes) => {
            if bytes.contains(&0) {
                Exception { path: path.to_owned(), disposition: Disposition::Rejected, reason: "binary_nul_in_text_source".into(), size: Some(meta.len()), detail: None }
            } else {
                Exception { path: path.to_owned(), disposition: Disposition::Unsupported, reason: "not_admitted_by_primary_scan".into(), size: None, detail: None }
            }
        }
        Err(error) => Exception { path: path.to_owned(), disposition: Disposition::Failed, reason: "source_read_failed".into(), size: None, detail: Some(error.to_string()) },
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SourceDispositionCounts {
    pub ignored_policy: usize,
    pub unsupported: usize,
    pub rejected: usize,
    pub failed: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    AdmittedOnly,
    GitTracked,
}

#[derive(Debug, Clone, Serialize)]
pub struct SourceDispositionReport {
    pub schema_version: u32,
    pub scope: Scope,
    pub complete: bool,
    pub considered: usize,
    pub indexed: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub counts: Option<SourceDispositionCounts>,
    pub exceptions: Vec<Exception>,
}

/// Mirrors `auditSourceDispositions(root, admittedFiles)`.
pub fn audit_source_dispositions(root: &Path, admitted_paths: &[String]) -> SourceDispositionReport {
    let admitted: BTreeSet<String> = admitted_paths.iter().map(|p| normalize_path(p)).filter(|p| !p.is_empty()).collect();
    let Some(tracked) = tracked_paths(root) else {
        return SourceDispositionReport {
            schema_version: 1,
            scope: Scope::AdmittedOnly,
            complete: true,
            considered: admitted.len(),
            indexed: admitted.len(),
            terminal: None,
            counts: None,
            exceptions: Vec::new(),
        };
    };

    let mut exceptions = Vec::new();
    let mut indexed = 0usize;
    for path in &tracked {
        if admitted.contains(path) {
            indexed += 1;
        } else {
            exceptions.push(classify_absent(root, path));
        }
    }
    exceptions.sort_by(|a, b| a.path.cmp(&b.path).then_with(|| a.disposition.as_key().cmp(b.disposition.as_key())));
    let terminal = indexed + exceptions.len();
    let counts = SourceDispositionCounts {
        ignored_policy: exceptions.iter().filter(|e| e.disposition == Disposition::IgnoredPolicy).count(),
        unsupported: exceptions.iter().filter(|e| e.disposition == Disposition::Unsupported).count(),
        rejected: exceptions.iter().filter(|e| e.disposition == Disposition::Rejected).count(),
        failed: exceptions.iter().filter(|e| e.disposition == Disposition::Failed).count(),
    };
    SourceDispositionReport {
        schema_version: 1,
        scope: Scope::GitTracked,
        complete: terminal == tracked.len(),
        considered: tracked.len(),
        indexed,
        terminal: Some(terminal),
        counts: Some(counts),
        exceptions,
    }
}

/// Registry entry for ingestion accounting. Source disposition is deliberately
/// side-effect free; graph build owns publication of its typed summary.
pub fn run(ctx: &crate::providers::ProviderContext<'_>) -> crate::providers::ProviderOutput {
    let admitted = ctx.files.iter().map(|file| file.path.clone()).collect::<Vec<_>>();
    let report = audit_source_dispositions(ctx.repo_root, &admitted);
    let evidence = serde_json::to_value(report).unwrap_or(serde_json::Value::Null);
    crate::providers::ProviderOutput {
        nodes: vec![crate::model::GraphNode {
            id: "provider:ingestion".into(), kind: "provider_diagnostic".into(), path: None,
            name: Some("ingestion".into()), generation_id: String::new(),
            evidence: vec![evidence],
        }],
        edges: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn init_git_repo(dir: &Path) {
        let run = |args: &[&str]| {
            let status = Command::new("git").arg("-C").arg(dir).args(args).status().expect("git available");
            assert!(status.success(), "git {args:?} failed");
        };
        run(&["init", "-q"]);
        run(&["config", "user.email", "test@example.com"]);
        run(&["config", "user.name", "test"]);
    }

    #[test]
    fn non_git_root_reports_admitted_only_scope() {
        let dir = std::env::temp_dir().join(format!("bp-src-disp-nongit-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let report = audit_source_dispositions(&dir, &["a.rs".to_string()]);
        assert!(matches!(report.scope, Scope::AdmittedOnly));
        assert!(report.complete);
        assert_eq!(report.considered, 1);
        assert_eq!(report.indexed, 1);
        assert!(report.exceptions.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn git_tracked_scope_classifies_unindexed_and_policy_excluded_paths() {
        let dir = std::env::temp_dir().join(format!("bp-src-disp-git-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("node_modules")).unwrap();
        init_git_repo(&dir);
        fs::write(dir.join("admitted.rs"), b"fn main() {}").unwrap();
        fs::write(dir.join("skipped.txt"), b"not admitted").unwrap();
        fs::write(dir.join("node_modules/dep.js"), b"module.exports = {}").unwrap();
        Command::new("git").arg("-C").arg(&dir).args(["add", "-A"]).status().unwrap();

        let report = audit_source_dispositions(&dir, &["admitted.rs".to_string()]);
        assert!(matches!(report.scope, Scope::GitTracked));
        assert_eq!(report.considered, 3);
        assert_eq!(report.indexed, 1);
        assert!(report.complete);
        let by_path: std::collections::HashMap<_, _> = report.exceptions.iter().map(|e| (e.path.as_str(), e)).collect();
        assert_eq!(by_path["skipped.txt"].disposition, Disposition::Unsupported);
        assert_eq!(by_path["node_modules/dep.js"].disposition, Disposition::IgnoredPolicy);
        assert_eq!(by_path["node_modules/dep.js"].reason, "primary_scan_exclusion");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn binary_nul_source_is_rejected() {
        let dir = std::env::temp_dir().join(format!("bp-src-disp-bin-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        init_git_repo(&dir);
        fs::write(dir.join("odd.bin"), [b'a', 0u8, b'b']).unwrap();
        Command::new("git").arg("-C").arg(&dir).args(["add", "-A"]).status().unwrap();

        let report = audit_source_dispositions(&dir, &[]);
        assert_eq!(report.exceptions.len(), 1);
        assert_eq!(report.exceptions[0].disposition, Disposition::Rejected);
        assert_eq!(report.exceptions[0].reason, "binary_nul_in_text_source");
        let _ = fs::remove_dir_all(&dir);
    }
}
