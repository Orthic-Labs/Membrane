//! Shadow-only Doc Spine registration. Artifacts are source references, never memories.

use crate::{
    doc_projection::{
        replace_doc_projections, DocumentProjectionStoreInputV1, DocumentProjectionV1,
        ProjectionKind, ProjectionProvenanceV1,
    },
    MemDb,
};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct DocFrontmatterV1 {
    title: Option<String>,
    summary: Option<String>,
    keywords: Vec<String>,
    status: Option<String>,
    supersedes: Option<String>,
}

fn parse_frontmatter(markdown: &str) -> Result<DocFrontmatterV1, String> {
    if !markdown.starts_with("---\n") && !markdown.starts_with("---\r\n") {
        return Ok(DocFrontmatterV1::default());
    }
    let end = markdown[3..]
        .find("\n---\n")
        .or_else(|| markdown[3..].find("\n---\r\n"))
        .map(|offset| offset + 4)
        .ok_or_else(|| "unclosed frontmatter".to_owned())?;
    let block = &markdown[..end + 3];
    if block.len() > 32 * 1024 {
        return Err("frontmatter exceeds 32 KiB".to_owned());
    }
    let mut values = BTreeMap::new();
    let mut namespace_seen = false;
    let mut in_membrane = false;
    for raw in block
        .lines()
        .skip(1)
        .take_while(|line| line.trim() != "---")
    {
        if raw.trim().is_empty() || raw.trim_start().starts_with('#') {
            continue;
        }
        let indent = raw.len() - raw.trim_start_matches(' ').len();
        if indent == 0 {
            in_membrane = false;
            let Some((key, value)) = raw.split_once(':') else {
                continue;
            };
            if key.trim() != "membrane" {
                continue;
            }
            if namespace_seen {
                return Err("duplicate frontmatter namespace: membrane".to_owned());
            }
            if !value.trim().is_empty() {
                return Err("membrane frontmatter namespace must be a mapping".to_owned());
            }
            namespace_seen = true;
            in_membrane = true;
            continue;
        }
        if !in_membrane {
            continue;
        }
        let (key, value) = raw
            .trim_start()
            .split_once(':')
            .ok_or_else(|| "malformed membrane frontmatter".to_owned())?;
        let key = key.trim();
        let value = value.trim();
        if value.len() > 4 * 1024 || value.chars().any(char::is_control) {
            return Err("invalid frontmatter value".to_owned());
        }
        if matches!(
            key,
            "title" | "summary" | "keywords" | "status" | "supersedes"
        ) && values.insert(key.to_owned(), value.to_owned()).is_some()
        {
            return Err(format!("duplicate frontmatter key: {key}"));
        }
    }
    let status = values.remove("status");
    if let Some(status) = &status {
        if !matches!(
            status.as_str(),
            "active" | "draft" | "retired" | "superseded"
        ) {
            return Err("invalid frontmatter status".to_owned());
        }
    }
    let keywords = values
        .remove("keywords")
        .map(|value| -> Result<Vec<String>, String> {
            let value = value.trim();
            let value = match (value.starts_with('['), value.ends_with(']')) {
                (true, true) => &value[1..value.len() - 1],
                (false, false) => value,
                _ => return Err("invalid frontmatter keywords".to_owned()),
            };
            Ok(value
                .split(',')
                .map(|word| word.trim().trim_matches(['\'', '"']))
                .filter(|word| !word.is_empty())
                .map(str::to_owned)
                .collect())
        })
        .transpose()?
        .unwrap_or_default();
    Ok(DocFrontmatterV1 {
        title: values.remove("title"),
        summary: values.remove("summary"),
        keywords,
        status,
        supersedes: values
            .remove("supersedes")
            .filter(|value| !value.is_empty()),
    })
}

fn lexical_content(markdown: &str, frontmatter: &DocFrontmatterV1) -> String {
    if frontmatter == &DocFrontmatterV1::default() {
        return markdown.to_owned();
    }
    let mut content = String::new();
    if let Some(title) = &frontmatter.title {
        content.push_str(title);
        content.push('\n');
    }
    if let Some(summary) = &frontmatter.summary {
        content.push_str(summary);
        content.push('\n');
    }
    if !frontmatter.keywords.is_empty() {
        content.push_str(&frontmatter.keywords.join(" "));
        content.push('\n');
    }
    content.push_str(markdown);
    content
}

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

fn query_terms(query: &str) -> Vec<String> {
    query
        .to_ascii_lowercase()
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|term| !term.is_empty())
        .map(str::to_owned)
        .collect()
}

fn lexical_score(content: &str, terms: &[String]) -> usize {
    let content = content.to_ascii_lowercase();
    terms
        .iter()
        .map(|term| content.match_indices(term).count())
        .sum()
}

/// Recall active Doc Spine artifacts as source pointers, never memory entries.
///
/// Source is reopened & hash-checked before emitting a hit, so stale projections cannot yield a
/// pointer whose expected hash targets changed document content.
pub fn recall(db: &MemDb, query: &str, k: usize) -> Result<Vec<DocRecallHitV1>, String> {
    if k == 0 {
        return Ok(Vec::new());
    }
    let terms = query_terms(query);
    if terms.is_empty() {
        return Ok(Vec::new());
    }
    let rows = {
        let conn = db.lock();
        let mut statement = conn
            .prepare(
                "SELECT artifact.doc_id, artifact.repository_root, artifact.path, artifact.content_hash, projection.content
                 FROM doc_artifacts artifact
                 JOIN doc_projections projection ON projection.parent_doc_id=artifact.doc_id
                 WHERE artifact.lifecycle_state='active'
                   AND artifact.sensitivity='normal'
                   AND projection.kind='lexical'
                   AND projection.source_content_hash=artifact.content_hash
                   AND projection.source_revision=artifact.revision
                   AND projection.index_generation=artifact.index_generation",
            )
            .map_err(|error| error.to_string())?;
        let results = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        results
    };

    let mut hits = Vec::new();
    for (doc_id, repository_root, path, expected_hash, projection) in rows {
        let score = lexical_score(&projection, &terms);
        if score == 0
            || Path::new(&path)
                .components()
                .any(|part| matches!(part, std::path::Component::ParentDir))
        {
            continue;
        }
        let source_path = Path::new(&repository_root).join(&path);
        let Ok(markdown) = std::fs::read_to_string(source_path) else {
            continue;
        };
        if digest(markdown.as_bytes()) != expected_hash {
            continue;
        }
        let source_ref = format!("doc://repo/worktree/{}", path.trim_start_matches('/'));
        let outline = crate::outline::build_outline(&source_ref, &markdown, "comrak-0.54.0");
        let Some(section) = outline
            .sections
            .iter()
            .map(|section| {
                (
                    lexical_score(&markdown[section.start_byte..section.end_byte], &terms),
                    section,
                )
            })
            .filter(|(section_score, _)| *section_score > 0)
            .max_by_key(|(section_score, section)| {
                (
                    *section_score,
                    usize::MAX - (section.end_byte - section.start_byte),
                )
            })
            .map(|(_, section)| section)
        else {
            continue;
        };
        hits.push(DocRecallHitV1 {
            doc_id,
            source_ref,
            anchor_id: section.anchor_id.clone(),
            expected_hash,
            score: score as f32,
        });
    }
    hits.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| left.doc_id.cmp(&right.doc_id))
    });
    hits.truncate(k);
    Ok(hits)
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

/// Read-only Doc Spine recall result. Document text stays in source; callers receive only a
/// hash-bound pointer consumable by `memright doc read`.
#[derive(Clone, Debug, serde::Serialize, PartialEq)]
pub struct DocRecallHitV1 {
    pub doc_id: String,
    pub source_ref: String,
    pub anchor_id: String,
    pub expected_hash: String,
    pub score: f32,
}

#[derive(Debug)]
struct IgnoreRule {
    base: PathBuf,
    pattern: String,
}

fn ignored(root: &Path, path: &Path, rules: &[IgnoreRule]) -> bool {
    rules.iter().any(|rule| {
        let Ok(relative) = path.strip_prefix(&rule.base) else {
            return false;
        };
        let relative = relative.to_string_lossy().replace('\\', "/");
        let pattern = rule.pattern.trim_matches('/');
        if pattern.is_empty() {
            return false;
        }
        if pattern.contains('/') {
            relative == pattern || relative.starts_with(&format!("{pattern}/"))
        } else {
            relative.split('/').any(|part| part == pattern)
                || path
                    .strip_prefix(root)
                    .ok()
                    .and_then(|p| p.file_name())
                    .is_some_and(|name| name == pattern)
        }
    })
}

fn load_gitignore(dir: &Path, rules: &mut Vec<IgnoreRule>) {
    let Ok(contents) = std::fs::read_to_string(dir.join(".gitignore")) else {
        return;
    };
    for line in contents.lines() {
        let line = line.trim();
        if !line.is_empty() && !line.starts_with('#') && !line.starts_with('!') {
            rules.push(IgnoreRule {
                base: dir.to_path_buf(),
                pattern: line.to_string(),
            });
        }
    }
}

fn has_health_component(path: &Path) -> bool {
    path.components().any(|component| {
        component
            .as_os_str()
            .to_str()
            .is_some_and(|name| name.eq_ignore_ascii_case("health"))
    })
}

fn hard_excluded(path: &Path) -> bool {
    let components = path
        .components()
        .filter_map(|component| component.as_os_str().to_str())
        .map(|component| component.to_ascii_lowercase())
        .collect::<Vec<_>>();
    components.iter().any(|component| {
        matches!(
            component.as_str(),
            ".git" | "node_modules" | ".cache" | "target" | ".venv" | "vendor" | "memory-mirror"
        )
    }) || components.last().is_some_and(|name| name == "memory.md")
}

fn walk(
    root: &Path,
    output: &mut Vec<PathBuf>,
    excluded_health: &mut usize,
) -> std::io::Result<()> {
    let mut pending = vec![(root.to_path_buf(), 0usize)];
    let mut ignore_rules = Vec::new();
    while let Some((dir, depth)) = pending.pop() {
        if depth > 64 {
            continue;
        }
        load_gitignore(&dir, &mut ignore_rules);
        for item in std::fs::read_dir(&dir)? {
            let item = item?;
            let path = item.path();
            let relative = path.strip_prefix(root).unwrap_or(&path);
            let kind = item.file_type()?;
            if kind.is_symlink() {
                continue;
            }
            if has_health_component(relative) {
                *excluded_health += 1;
                continue;
            }
            if hard_excluded(relative) {
                continue;
            }
            if ignored(root, &path, &ignore_rules) {
                continue;
            }
            if kind.is_dir() {
                let name = path.file_name().and_then(|v| v.to_str()).unwrap_or("");
                if matches!(
                    name,
                    ".git"
                        | "node_modules"
                        | "target"
                        | ".cache"
                        | ".venv"
                        | "vendor"
                        | "memory-mirror"
                ) {
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
            doc_id TEXT NOT NULL, repository_root TEXT NOT NULL, repository_id TEXT NOT NULL,
            revision TEXT NOT NULL, path TEXT NOT NULL, content_hash TEXT NOT NULL,
            parser_version TEXT NOT NULL, document_class TEXT NOT NULL,
            lifecycle_state TEXT NOT NULL DEFAULT 'active', title TEXT NOT NULL DEFAULT '',
            summary TEXT NOT NULL DEFAULT '', keywords_json TEXT NOT NULL DEFAULT '[]',
            superseded_by TEXT, trust_label TEXT NOT NULL,
            influence_class TEXT NOT NULL, sensitivity TEXT NOT NULL, generated INTEGER NOT NULL DEFAULT 0,
            index_generation INTEGER NOT NULL, updated_at_ms INTEGER NOT NULL,
            UNIQUE(repository_root, path)
        );
        CREATE INDEX IF NOT EXISTS idx_doc_artifacts_root_state
          ON doc_artifacts(repository_root, lifecycle_state, index_generation);"
    ).map_err(|e| e.to_string())?;
    for column in [
        "title TEXT NOT NULL DEFAULT ''",
        "summary TEXT NOT NULL DEFAULT ''",
        "keywords_json TEXT NOT NULL DEFAULT '[]'",
        "superseded_by TEXT",
    ] {
        let name = column.split_whitespace().next().unwrap();
        let exists = {
            let mut statement = conn
                .prepare("PRAGMA table_info(doc_artifacts)")
                .map_err(|e| e.to_string())?;
            let rows = statement
                .query_map([], |row| row.get::<_, String>(1))
                .map_err(|e| e.to_string())?;
            let found = rows.filter_map(Result::ok).any(|entry| entry == name);
            found
        };
        if !exists {
            conn.execute(
                &format!("ALTER TABLE doc_artifacts ADD COLUMN {column}"),
                [],
            )
            .map_err(|e| e.to_string())?;
        }
    }
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    let generation: i64 = tx.query_row("SELECT COALESCE(MAX(index_generation),0)+1 FROM doc_artifacts WHERE repository_root=?1", [&root_s], |r| r.get(0)).map_err(|e| e.to_string())?;
    let now = crate::time::now_millis() as i64;
    let mut registered = 0;
    let mut projection_inputs = Vec::new();
    let mut supersessions = Vec::new();
    for file in files {
        let bytes = std::fs::read(&file).map_err(|e| e.to_string())?;
        let relative = file
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let markdown = String::from_utf8_lossy(&bytes).into_owned();
        let frontmatter = parse_frontmatter(&markdown)?;
        let (class, influence, sensitivity, generated) = classify(&relative);
        let hash = digest(&bytes);
        let default_id = format!(
            "doc:{}:{}",
            digest(root_s.as_bytes())[..16].to_string(),
            digest(relative.as_bytes())[..16].to_string()
        );
        let id: String = tx
            .query_row(
                "SELECT doc_id FROM doc_artifacts WHERE repository_root=?1 AND content_hash=?2 AND lifecycle_state='active' AND path<>?3 ORDER BY updated_at_ms DESC LIMIT 1",
                rusqlite::params![root_s, hash, relative],
                |row| row.get(0),
            )
            .unwrap_or(default_id);
        let lifecycle = frontmatter.status.as_deref().unwrap_or("active");
        let keywords_json =
            serde_json::to_string(&frontmatter.keywords).map_err(|e| e.to_string())?;
        tx.execute("INSERT INTO doc_artifacts (doc_id, repository_root, repository_id, revision, path, content_hash, parser_version, document_class, lifecycle_state, title, summary, keywords_json, superseded_by, trust_label, influence_class, sensitivity, generated, index_generation, updated_at_ms)
          VALUES (?1,?2,?2,?3,?4,?5,'comrak-0.54.0',?6,?7,?8,?9,?10,NULL,'catalogued',?11,?12,?13,?14,?15)
          ON CONFLICT(repository_root,path) DO UPDATE SET revision=excluded.revision, content_hash=excluded.content_hash, parser_version=excluded.parser_version, document_class=excluded.document_class, lifecycle_state=excluded.lifecycle_state, title=excluded.title, summary=excluded.summary, keywords_json=excluded.keywords_json, superseded_by=NULL, trust_label=excluded.trust_label, influence_class=excluded.influence_class, sensitivity=excluded.sensitivity, generated=excluded.generated, index_generation=excluded.index_generation, updated_at_ms=excluded.updated_at_ms",
          rusqlite::params![id, root_s, revision, relative, hash, class, lifecycle, frontmatter.title.clone().unwrap_or_default(), frontmatter.summary.clone().unwrap_or_default(), keywords_json, influence, sensitivity, generated as i64, generation, now]).map_err(|e| e.to_string())?;
        if let Some(target) = &frontmatter.supersedes {
            supersessions.push((id.clone(), relative.clone(), target.clone()));
        }
        projection_inputs.push(DocumentProjectionStoreInputV1 {
            parent_doc_id: id,
            source_content_hash: hash,
            source_revision: revision.clone(),
            index_generation: generation,
            projections: vec![DocumentProjectionV1 {
                kind: ProjectionKind::Lexical,
                content: lexical_content(&markdown, &frontmatter),
                token_count: 0,
                provenance: ProjectionProvenanceV1 {
                    anchor_id: "document".to_owned(),
                    collapsed_to_parent: None,
                },
            }],
        });
        registered += 1;
    }
    let mut edges = BTreeMap::new();
    for (new_id, new_path, target_path) in &supersessions {
        if target_path == new_path {
            return Err("frontmatter supersedes self".to_owned());
        }
        let target_id: String = tx.query_row(
            "SELECT doc_id FROM doc_artifacts WHERE repository_root=?1 AND path=?2 AND lifecycle_state NOT IN ('tombstoned')",
            rusqlite::params![root_s, target_path], |row| row.get(0),
        ).map_err(|_| format!("frontmatter supersedes target missing: {target_path}"))?;
        edges.insert(new_id.clone(), target_id);
    }
    for start in edges.keys() {
        let mut seen = BTreeSet::new();
        let mut current = start.as_str();
        while let Some(next) = edges.get(current) {
            if !seen.insert(current.to_owned()) || next == start {
                return Err("frontmatter supersedes cycle".to_owned());
            }
            current = next;
        }
    }
    for (new_id, _, target_path) in &supersessions {
        tx.execute("UPDATE doc_artifacts SET lifecycle_state='superseded', superseded_by=?1, updated_at_ms=?2 WHERE repository_root=?3 AND path=?4", rusqlite::params![new_id, now, root_s, target_path]).map_err(|e| e.to_string())?;
    }
    let tombstoned = tx.execute("UPDATE doc_artifacts SET lifecycle_state='tombstoned', index_generation=?2, updated_at_ms=?3 WHERE repository_root=?1 AND lifecycle_state IN ('active','draft','retired') AND index_generation < ?2", rusqlite::params![root_s, generation, now]).map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    drop(conn);
    for input in &projection_inputs {
        replace_doc_projections(db, input).map_err(|e| e.to_string())?;
    }
    Ok(DocSyncReport {
        registered,
        tombstoned,
        excluded_health,
        index_generation: generation,
    })
}
