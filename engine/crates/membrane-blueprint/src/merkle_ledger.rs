//! Native port of Blueprint's JS Merkle ledger authority
//! (`blueprint/src/graph/merkle-ledger.mjs`).
//!
//! Reproduces the legacy algorithm exactly: leaf/directory digest encoding,
//! bottom-up directory rebuild ordering, and full/incremental root parity.
//! Directory digests are computed with [`crate::identity::content_digest`]
//! (xxh128) over the same JSON encoding the JS authority used
//! (`JSON.stringify` of a sorted `[name, digest]` pair array).

use crate::identity::content_digest;
use rusqlite::{params, Connection};
use serde_json::Value;

/// A source file observed by the scanner, addressed by its content digest.
/// Mirrors the loosely-typed `{ path, contentDigest | content_digest | contentHash }`
/// shape the JS authority accepts.
#[derive(Debug, Clone)]
pub struct LedgerFile {
    pub path: String,
    pub content_digest: String,
}

/// The result of comparing the recorded ledger against a fresh scan.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LedgerDiff {
    pub changed: Vec<String>,
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub root: Option<String>,
}

fn normalize_digest(value: &str) -> String {
    if value.starts_with("xxh128:") {
        value.to_string()
    } else {
        format!("xxh128:{value}")
    }
}

/// `leafDigestForFile` — the leaf digest recorded for a file is just its
/// (normalized) content digest.
pub fn leaf_digest_for_file(content_digest_value: &str) -> String {
    normalize_digest(content_digest_value)
}

/// `dirDigest` — digest a directory over its sorted `(name, digest)` child
/// entries, encoded exactly as the JS authority's
/// `JSON.stringify([[name, digest], ...])` (sorted array-of-arrays, no
/// object keys, so no key-sort ambiguity in either implementation).
pub fn dir_digest(child_entries: &[(String, String)]) -> String {
    let mut entries: Vec<(String, String)> = child_entries.to_vec();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    let json = Value::Array(
        entries
            .into_iter()
            .map(|(name, digest)| Value::Array(vec![Value::String(name), Value::String(digest)]))
            .collect(),
    );
    let encoded = serde_json::to_string(&json).expect("array-of-strings JSON never fails");
    content_digest(encoded.as_bytes())
}

/// `parentDirectories` — every ancestor directory path of `path`, starting
/// with the root (`""`), in root-to-leaf order.
fn parent_directories(path: &str) -> Vec<String> {
    let normalized = path.replace('\\', "/");
    let parts: Vec<&str> = normalized.split('/').filter(|p| !p.is_empty()).collect();
    let mut parents = vec![String::new()];
    for index in 1..parts.len() {
        parents.push(parts[..index].join("/"));
    }
    parents
}

/// `pathMetadata` — split a normalized path into its parent directory path
/// and its own name.
fn path_metadata(path: &str) -> (Option<String>, String) {
    let normalized = path.replace('\\', "/");
    if normalized.is_empty() {
        return (None, String::new());
    }
    match normalized.rfind('/') {
        Some(split) => (
            Some(normalized[..split].to_string()),
            normalized[split + 1..].to_string(),
        ),
        None => (Some(String::new()), normalized),
    }
}

/// Directory sort key used to rebuild bottom-up: deepest paths first, then
/// reverse-lexicographic — matching the JS authority's
/// `right.split("/").length - left.split("/").length || right.localeCompare(left)`.
fn directory_depth(path: &str) -> usize {
    if path.is_empty() {
        0
    } else {
        path.split('/').count()
    }
}

fn sort_directories_bottom_up(directories: &mut Vec<String>) {
    directories.sort_by(|left, right| {
        directory_depth(right)
            .cmp(&directory_depth(left))
            .then_with(|| right.cmp(left))
    });
}

/// `writeDirectory` — recompute one directory's digest from its recorded
/// children and upsert it; deletes the directory row (returning `None`) once
/// it has no remaining children, mirroring the JS authority.
fn write_directory(conn: &Connection, directory: &str) -> rusqlite::Result<Option<String>> {
    let mut stmt = conn.prepare(
        "SELECT name, digest FROM generation_leaf WHERE parent_path = ?1 ORDER BY name",
    )?;
    let children: Vec<(String, String)> = stmt
        .query_map(params![directory], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<_, _>>()?;

    if !directory.is_empty() && children.is_empty() {
        conn.execute(
            "DELETE FROM generation_leaf WHERE path = ?1 AND kind = 'dir'",
            params![directory],
        )?;
        return Ok(None);
    }

    let digest = dir_digest(&children);
    let (parent_path, name) = path_metadata(directory);
    conn.execute(
        "INSERT INTO generation_leaf(path, kind, digest, parent_path, name) VALUES (?1, 'dir', ?2, ?3, ?4)
         ON CONFLICT(path) DO UPDATE SET kind='dir', digest=excluded.digest, parent_path=excluded.parent_path, name=excluded.name",
        params![directory, digest, parent_path, name],
    )?;
    Ok(Some(digest))
}

fn root_digest_or_empty(conn: &Connection) -> rusqlite::Result<String> {
    let digest: Option<String> = conn
        .query_row(
            "SELECT digest FROM generation_leaf WHERE path = '' AND kind = 'dir'",
            [],
            |row| row.get(0),
        )
        .ok();
    Ok(digest.unwrap_or_else(|| dir_digest(&[])))
}

/// `rebuildDirectories` — recompute every directory digest from the current
/// file leaves, deepest-first, returning the new root digest.
fn rebuild_directories(conn: &Connection) -> rusqlite::Result<String> {
    conn.execute("DELETE FROM generation_leaf WHERE kind = 'dir'", [])?;

    let mut directories: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    directories.insert(String::new());
    {
        let mut stmt = conn.prepare("SELECT path FROM generation_leaf WHERE kind = 'file'")?;
        let paths: Vec<String> = stmt
            .query_map([], |row| row.get(0))?
            .collect::<Result<_, _>>()?;
        for path in paths {
            for parent in parent_directories(&path) {
                directories.insert(parent);
            }
        }
    }

    let mut ordered: Vec<String> = directories.into_iter().collect();
    sort_directories_bottom_up(&mut ordered);
    for directory in &ordered {
        write_directory(conn, directory)?;
    }

    root_digest_or_empty(conn)
}

/// `updateLeafChain` — upsert (or delete, when `content_digest` is `None`) one
/// file leaf, then recompute every ancestor directory bottom-up. Returns the
/// new root digest.
pub fn update_leaf_chain(
    conn: &Connection,
    path: &str,
    content_digest_or_none: Option<&str>,
) -> rusqlite::Result<String> {
    let normalized = path.replace('\\', "/");
    match content_digest_or_none {
        None => {
            conn.execute(
                "DELETE FROM generation_leaf WHERE path = ?1 AND kind = 'file'",
                params![normalized],
            )?;
        }
        Some(digest) => {
            let (parent_path, name) = path_metadata(&normalized);
            conn.execute(
                "INSERT INTO generation_leaf(path, kind, digest, parent_path, name) VALUES (?1, 'file', ?2, ?3, ?4)
                 ON CONFLICT(path) DO UPDATE SET kind='file', digest=excluded.digest, parent_path=excluded.parent_path, name=excluded.name",
                params![normalized, leaf_digest_for_file(digest), parent_path, name],
            )?;
        }
    }

    let mut parents = parent_directories(&normalized);
    sort_directories_bottom_up(&mut parents);
    for directory in &parents {
        write_directory(conn, directory)?;
    }

    root_digest_or_empty(conn)
}

/// `computeFullLedger` — replace the entire ledger from a fresh file scan and
/// return the new root digest.
pub fn compute_full_ledger(conn: &Connection, files: &[LedgerFile]) -> rusqlite::Result<String> {
    conn.execute("DELETE FROM generation_leaf", [])?;
    {
        let mut insert = conn.prepare(
            "INSERT INTO generation_leaf(path, kind, digest, parent_path, name) VALUES (?1, 'file', ?2, ?3, ?4)",
        )?;
        for file in files {
            if file.content_digest.is_empty() {
                continue;
            }
            let path = file.path.replace('\\', "/");
            let (parent_path, name) = path_metadata(&path);
            insert.execute(params![
                path,
                leaf_digest_for_file(&file.content_digest),
                parent_path,
                name
            ])?;
        }
    }
    rebuild_directories(conn)
}

/// `diffLedgerAgainstTree` — compare the recorded file leaves against a fresh
/// scan without mutating the store, sorted exactly as the JS authority
/// (`Array.prototype.sort()` lexicographic order).
pub fn diff_ledger_against_tree(
    conn: &Connection,
    root: Option<&str>,
    scan_files: &[LedgerFile],
) -> rusqlite::Result<LedgerDiff> {
    let mut recorded: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    {
        let mut stmt =
            conn.prepare("SELECT path, digest FROM generation_leaf WHERE kind = 'file'")?;
        let rows: Vec<(String, String)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<Result<_, _>>()?;
        recorded.extend(rows);
    }

    let current: std::collections::HashMap<String, String> = scan_files
        .iter()
        .map(|file| (file.path.clone(), leaf_digest_for_file(&file.content_digest)))
        .collect();

    let mut changed = Vec::new();
    let mut added = Vec::new();
    for (path, digest) in &current {
        match recorded.get(path) {
            None => added.push(path.clone()),
            Some(existing) if existing != digest => changed.push(path.clone()),
            Some(_) => {}
        }
    }
    let mut removed: Vec<String> = recorded
        .keys()
        .filter(|path| !current.contains_key(*path))
        .cloned()
        .collect();

    changed.sort();
    added.sort();
    removed.sort();

    let root = match root {
        Some(value) => Some(value.to_string()),
        None => {
            let digest: Option<String> = conn
                .query_row(
                    "SELECT digest FROM generation_leaf WHERE path = '' AND kind = 'dir'",
                    [],
                    |row| row.get(0),
                )
                .ok();
            digest
        }
    };

    Ok(LedgerDiff {
        changed,
        added,
        removed,
        root,
    })
}
