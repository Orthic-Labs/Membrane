//! Shadow-only Doc Spine registration. Artifacts are source references, never memories.

use crate::MemDb;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct DocArtifactV1 {
    pub doc_id: String,
    pub repository_root: String,
    pub repository_id: String,
    pub revision: String,
    pub path: String,
    pub content_hash: String,
    pub parser_version: String,
    pub document_class: String,
    pub lifecycle_state: String,
    pub trust_label: String,
    pub influence_class: String,
    pub sensitivity: String,
    pub generated: bool,
    pub index_generation: i64,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct DocSyncReport {
    pub registered: usize,
    pub tombstoned: usize,
    pub excluded_health: usize,
    pub index_generation: i64,
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn classify(path: &str) -> (&'static str, &'static str, &'static str, bool) {
    let lower = path.to_ascii_lowercase();
    let generated = lower.contains("/generated/") || lower.ends_with(".generated.md");
    let class = if generated {
        "generated"
    } else if lower.contains("runbook") {
        "runbook"
    } else if lower.contains("decision") {
        "decision"
    } else if lower.contains("policy") {
        "policy"
    } else if lower.contains("histor") {
        "historical"
    } else if lower.contains("content/") {
        "content"
    } else {
        "knowledge"
    };
    let influence = if class == "policy" {
        "authority"
    } else if class == "runbook" {
        "procedure"
    } else {
        "reference"
    };
    (
        class,
        influence,
        if lower.contains("secret") {
            "restricted"
        } else {
            "normal"
        },
        generated,
    )
}
fn walk(
    root: &Path,
    output: &mut Vec<PathBuf>,
    excluded_health: &mut usize,
) -> std::io::Result<()> {
    let mut pending = vec![(root.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = pending.pop() {
        if depth > 64 {
            continue;
        }
        for item in std::fs::read_dir(&dir)? {
            let item = item?;
            let path = item.path();
            let relative = path.strip_prefix(root).unwrap_or(&path);
            let kind = item.file_type()?;
            if kind.is_symlink() {
                continue;
            }
            if kind.is_dir() {
                let name = path.file_name().and_then(|v| v.to_str()).unwrap_or("");
                if matches!(name, ".git" | "node_modules" | "target" | ".cache") {
                    continue;
                }
                if relative
                    .components()
                    .next()
                    .and_then(|v| v.as_os_str().to_str())
                    == Some("Health")
                {
                    *excluded_health += 1;
                    continue;
                }
                pending.push((path, depth + 1));
            } else if path
                .extension()
                .and_then(|v| v.to_str())
                .is_some_and(|v| v.eq_ignore_ascii_case("md"))
            {
                output.push(path);
            }
        }
    }
    Ok(())
}

#[inline(never)]
pub fn sync(db: &MemDb, root: &Path) -> Result<DocSyncReport, String> {
    let root = std::fs::canonicalize(root).map_err(|e| e.to_string())?;
    let root_s = root.to_string_lossy().replace('\\', "/");
    let revision = std::process::Command::new("git")
        .args(["-C", &root_s, "rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "worktree".into());
    let mut files = Vec::new();
    let mut excluded_health = 0;
    walk(&root, &mut files, &mut excluded_health).map_err(|e| e.to_string())?;
    let mut conn = db.lock();
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS doc_artifacts (
            doc_id TEXT PRIMARY KEY, repository_root TEXT NOT NULL, repository_id TEXT NOT NULL,
            revision TEXT NOT NULL, path TEXT NOT NULL, content_hash TEXT NOT NULL,
            parser_version TEXT NOT NULL, document_class TEXT NOT NULL,
            lifecycle_state TEXT NOT NULL DEFAULT 'active', trust_label TEXT NOT NULL,
            influence_class TEXT NOT NULL, sensitivity TEXT NOT NULL, generated INTEGER NOT NULL DEFAULT 0,
            index_generation INTEGER NOT NULL, updated_at_ms INTEGER NOT NULL,
            UNIQUE(repository_root, path)
        );
        CREATE INDEX IF NOT EXISTS idx_doc_artifacts_root_state
          ON doc_artifacts(repository_root, lifecycle_state, index_generation);"
    ).map_err(|e| e.to_string())?;
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    let generation: i64 = tx.query_row("SELECT COALESCE(MAX(index_generation),0)+1 FROM doc_artifacts WHERE repository_root=?1", [&root_s], |r| r.get(0)).map_err(|e| e.to_string())?;
    let now = crate::time::now_millis() as i64;
    let mut registered = 0;
    for file in files {
        let bytes = std::fs::read(&file).map_err(|e| e.to_string())?;
        let relative = file
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let (class, influence, sensitivity, generated) = classify(&relative);
        let hash = digest(&bytes);
        let id = format!(
            "doc:{}:{}",
            digest(root_s.as_bytes())[..16].to_string(),
            digest(relative.as_bytes())[..16].to_string()
        );
        tx.execute("INSERT INTO doc_artifacts (doc_id, repository_root, repository_id, revision, path, content_hash, parser_version, document_class, lifecycle_state, trust_label, influence_class, sensitivity, generated, index_generation, updated_at_ms)
          VALUES (?1,?2,?2,?3,?4,?5,'comrak-0.54.0',?6,'active','catalogued',?7,?8,?9,?10,?11)
          ON CONFLICT(repository_root,path) DO UPDATE SET revision=excluded.revision, content_hash=excluded.content_hash, parser_version=excluded.parser_version, document_class=excluded.document_class, lifecycle_state='active', trust_label=excluded.trust_label, influence_class=excluded.influence_class, sensitivity=excluded.sensitivity, generated=excluded.generated, index_generation=excluded.index_generation, updated_at_ms=excluded.updated_at_ms",
          rusqlite::params![id, root_s, revision, relative, hash, class, influence, sensitivity, generated as i64, generation, now]).map_err(|e| e.to_string())?;
        registered += 1;
    }
    let tombstoned = tx.execute("UPDATE doc_artifacts SET lifecycle_state='tombstoned', index_generation=?2, updated_at_ms=?3 WHERE repository_root=?1 AND lifecycle_state='active' AND index_generation < ?2", rusqlite::params![root_s, generation, now]).map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(DocSyncReport {
        registered,
        tombstoned,
        excluded_health,
        index_generation: generation,
    })
}
