//! Native port of `blueprint/src/graph/delta-store.mjs`.
//!
//! Correction to this module's earlier (GC6-era) doc comment: the native
//! store (`store.rs`, `migrations.rs`) is *not* schema-incompatible with the
//! legacy `better-sqlite3` store — `migrations.rs` creates the same
//! `files`/`symbols`/`edges`/`fact_owner`/`node_provider`/
//! `dependency_index`/`file_state`/`watch_state`/`event_journal`/
//! `generation`/`generation_leaf` tables `delta-store.mjs` was written
//! against, and `merkle_ledger.rs`/`identity.rs` already carry native ports
//! of `merkle-ledger.mjs`'s leaf-chain update and
//! `generation-identity.mjs`'s manifest digest. What *is* true is that
//! `store.rs` only exposes a whole-generation `save_generation` (clear every
//! body table, reinsert the full node/edge set); it has no incremental
//! per-file upsert entry point, and its `insert_node`/`insert_edge` are
//! private to that module. [`apply_file_delta`] below is therefore built on
//! new incremental primitives added in `store_delta.rs` (per this lane's
//! rule: never edit `store.rs`, add missing primitives in a new file
//! instead) rather than reusing `store.rs`'s private insert functions.
//!
//! [`apply_file_delta`] ports the legacy function's *structural* (non-doc)
//! delta path faithfully: the unchanged-digest noop short-circuit,
//! delete/rename fact retraction (edge unresolve + `deleteFactsByOwner` +
//! `files`/`file_state` row deletion + leaf-chain removal), structural fact
//! retraction-then-reinsertion per provider, `dependency_index` refresh,
//! `file_state` upsert, merkle leaf-chain update, and generation manifest
//! reseal (`resealGenerationIdentityDelta` + `refreshManifestCounts`),
//! applied-clock bookkeeping, and best-effort `event_journal` acknowledgment.
//! It returns the same `{applied, noop, path, appliedClock, orderWrites,
//! rootDigest}` result shape.
//!
//! Deliberately out of scope, with the reason:
//! - **Document-domain deltas** (`isDocumentDelta` in the legacy code:
//!   `.md`/`AGENTS.md`/`CLAUDE.md`/`README.md` paths, or an explicit doc
//!   provider/domain) are rejected with a typed error rather than silently
//!   handled. The legacy doc path additionally rewrites
//!   `claims.json`/`stale.json`/`queue.json` under the repo's `.agent`
//!   directory (`replaceDocumentArtifacts`) and records per-claim
//!   `fact_owner` rows — filesystem side effects and a document-claim
//!   projection this crate has no existing native counterpart for, and
//!   inventing one is a materially larger, separately-scoped port.
//! - **`orderWrites` is always `0`.** The legacy `reindexNodeOrdinals` is a
//!   symbol/file search-ordinal reindex (a query-projection concern); no
//!   native counterpart exists and building one is out of scope for a
//!   graph-store write port.
//! - **`provider_ranks`/`symbol_search`/`symbol_terms`** (query-projection
//!   caches touched by legacy `insertParsedFacts`/`registerProviderRanks`/
//!   `replaceSymbolSearchEntry`) are not maintained incrementally here for
//!   the same reason.
//! - **Telemetry** (`incrementTelemetry(db, "deltas_applied")`) has no
//!   native table (no `telemetry`-shaped table in `migrations.rs`) and is
//!   skipped.
//! - **`assertCompleteFileBatches`** (`publication-policy.mjs`) is not
//!   reproduced; this port does not validate factBatch completeness before
//!   applying.
//! - **Nested-transaction composition** (`options.inTransaction` /
//!   `requireOuterTransaction`, letting a caller batch several
//!   `applyFileDelta` calls inside one already-open transaction) is not
//!   supported: [`apply_file_delta`] always opens and owns its own
//!   `rusqlite` transaction. `rusqlite::Transaction` has no representation
//!   for "join an already-open transaction owned by the caller" the way the
//!   legacy `db.isTransaction` flag does.
//!
//! What else is ported here, exactly, because it is pure and self-contained:
//! - [`normalize_digest`] — the `xxh128:` prefixing helper.
//! - [`PendingDomains`] — the `watch_state` `domains_pending` CSV
//!   get/mark/clear logic (`readPendingDomains`/`markDomainPending`/
//!   `clearDomainPending`), reproduced over an in-memory value instead of a
//!   sqlite row so it stays testable without the legacy schema.
//! - [`replace_by_source`] — the splice-in-place-else-append logic used by
//!   `replaceDocumentArtifacts` to swap a source's entries for new ones while
//!   preserving the position and relative order of everything else.

/// Mirrors `normalizeDigest`: ensures the value carries the `xxh128:` scheme
/// prefix exactly once.
pub fn normalize_digest(value: &str) -> String {
    if value.starts_with("xxh128:") {
        value.to_string()
    } else {
        format!("xxh128:{value}")
    }
}

/// Mirrors the `watch_state` table's `domains_pending` key: a sorted,
/// deduplicated, comma-joined set of pending freshness domains
/// (`readPendingDomains`/`markDomainPending`/`clearDomainPending`).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PendingDomains {
    domains: Vec<String>,
}

impl PendingDomains {
    /// Mirrors `readPendingDomains`: parses a stored CSV value (or an absent
    /// one, i.e. `""`) into a trimmed, non-empty, sorted list.
    pub fn from_stored(value: &str) -> Self {
        let mut domains: Vec<String> = value
            .split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(str::to_string)
            .collect();
        domains.sort();
        Self { domains }
    }

    /// Mirrors `markDomainPending`: adds `domain` if not already present,
    /// keeping the set sorted.
    pub fn mark(&mut self, domain: &str) {
        if !self.domains.iter().any(|item| item == domain) {
            self.domains.push(domain.to_string());
            self.domains.sort();
        }
    }

    /// Mirrors `clearDomainPending`: removes `domain` if present.
    pub fn clear(&mut self, domain: &str) {
        self.domains.retain(|item| item != domain);
    }

    /// The domains currently pending, in sorted order.
    pub fn domains(&self) -> &[String] {
        &self.domains
    }

    /// Mirrors the stored representation: `None` when empty (the legacy code
    /// deletes the `watch_state` row rather than storing `""`), otherwise the
    /// comma-joined sorted set.
    pub fn to_stored(&self) -> Option<String> {
        if self.domains.is_empty() {
            None
        } else {
            Some(self.domains.join(","))
        }
    }
}

/// One entry in a source-keyed list, as used by `replaceDocumentArtifacts`
/// (e.g. a stale claim, a missing reference, an invalid supersession
/// marker). Only the `source` key matters for the splice logic; the payload
/// is opaque and carried through unchanged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceKeyed<T> {
    pub source: String,
    pub payload: T,
}

/// Mirrors `replaceBySource(entries, source, replacements)`: removes every
/// entry whose `source` matches, inserting `replacements` in the position of
/// the *first* removed entry (or appended at the end if none matched),
/// preserving the relative order of all other entries.
pub fn replace_by_source<T: Clone>(
    entries: &[SourceKeyed<T>],
    source: &str,
    replacements: &[SourceKeyed<T>],
) -> Vec<SourceKeyed<T>> {
    let mut output = Vec::with_capacity(entries.len() + replacements.len());
    let mut inserted = false;
    for entry in entries {
        if entry.source == source {
            if !inserted {
                output.extend(replacements.iter().cloned());
                inserted = true;
            }
            continue;
        }
        output.push(entry.clone());
    }
    if !inserted {
        output.extend(replacements.iter().cloned());
    }
    output
}

// ---------------------------------------------------------------------
// bounded git treeish materialisation
// ---------------------------------------------------------------------

use std::fmt::Display;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Upper bound for one git operation used by treeish projections.
pub const TREEISH_GIT_TIMEOUT: Duration = Duration::from_secs(10);
/// Shorter bound used while unregistering a temporary worktree. Cleanup is
/// best-effort from `Drop`, but never allowed to hold a request indefinitely.
pub const TREEISH_CLEANUP_TIMEOUT: Duration = Duration::from_secs(5);

static TREEISH_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Typed failures from disposable git state used by treeish projections.
#[derive(Debug, thiserror::Error)]
pub enum TreeishError {
    #[error("treeish reference is required")]
    ReferenceRequired,
    #[error("treeish '{reference}' is invalid ({operation})")]
    Invalid { reference: String, operation: &'static str },
    #[error("git {operation} exceeded its bounded timeout")]
    Timeout { operation: &'static str },
    #[error("git {operation} failed")]
    Unavailable { operation: &'static str },
    #[error("treeish callback failed: {0}")]
    Callback(String),
    #[error("treeish worktree cleanup failed")]
    CleanupFailed,
}

impl TreeishError {
    /// Stable wire-facing code for callers translating errors into
    /// `BlueprintError` without parsing display text.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::ReferenceRequired => "treeish_base_required",
            Self::Invalid { .. } => "treeish_invalid",
            Self::Timeout { .. } => "treeish_timeout",
            Self::Unavailable { .. } => "treeish_unavailable",
            Self::Callback(_) => "treeish_build_failed",
            Self::CleanupFailed => "treeish_cleanup_failed",
        }
    }
}

fn treeish_temp_path() -> PathBuf {
    let sequence = TREEISH_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_nanos())
        .unwrap_or_default();
    std::env::temp_dir().join(format!(
        "membrane-blueprint-treeish-{}-{nanos}-{sequence}",
        std::process::id()
    ))
}

/// Run git with bounded lifetime while draining stdout concurrently. Waiting
/// for a child before reading a piped stdout can deadlock once git fills the
/// pipe (the source of intermittent treeish timeouts); the reader starts
/// before polling the child, so output volume cannot block process progress.
pub fn run_treeish_git(
    repo_root: &Path,
    args: &[&str],
    timeout: Duration,
) -> Result<Vec<u8>, TreeishError> {
    let mut child = Command::new("git")
        .args(args)
        .current_dir(repo_root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| TreeishError::Unavailable { operation: "spawn" })?;
    let mut stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(TreeishError::Unavailable { operation: "stdout" });
        }
    };
    let reader = std::thread::spawn(move || {
        let mut output = Vec::new();
        let result = stdout.read_to_end(&mut output);
        (result, output)
    });
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) if started.elapsed() < timeout => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = reader.join();
                return Err(TreeishError::Timeout { operation: "command" });
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = reader.join();
                return Err(TreeishError::Unavailable { operation: "wait" });
            }
        }
    }?;
    let (read_result, output) = reader
        .join()
        .map_err(|_| TreeishError::Unavailable { operation: "stdout" })?;
    read_result.map_err(|_| TreeishError::Unavailable { operation: "stdout" })?;
    if status.success() {
        Ok(output)
    } else {
        Err(TreeishError::Unavailable { operation: "command" })
    }
}

fn run_treeish_git_status(
    repo_root: &Path,
    args: &[&str],
    timeout: Duration,
) -> Result<(), TreeishError> {
    let mut child = Command::new("git")
        .args(args)
        .current_dir(repo_root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| TreeishError::Unavailable { operation: "spawn" })?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return if status.success() {
                    Ok(())
                } else {
                    Err(TreeishError::Unavailable { operation: "command" })
                };
            }
            Ok(None) if started.elapsed() < timeout => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(TreeishError::Timeout { operation: "command" });
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(TreeishError::Unavailable { operation: "wait" });
            }
        }
    }
}

fn worktree_registered(repo_root: &Path, path: &Path) -> bool {
    let listing = match run_treeish_git(
        repo_root,
        &["worktree", "list", "--porcelain"],
        TREEISH_CLEANUP_TIMEOUT,
    ) {
        Ok(listing) => listing,
        // An unreadable registration list is unsafe to interpret as clean.
        Err(_) => return true,
    };
    let expected = path.to_string_lossy().replace('\\', "/");
    String::from_utf8_lossy(&listing).lines().any(|line| {
        line.strip_prefix("worktree ")
            .map(|value| value.replace('\\', "/").eq_ignore_ascii_case(&expected))
            .unwrap_or(false)
    })
}

/// Disposable detached worktree for one arbitrary git treeish. The path is
/// never reused, and explicit cleanup runs before return; `Drop` repeats it
/// on all early-error and panic-unwind paths.
pub struct TreeishWorktree {
    repo_root: PathBuf,
    path: PathBuf,
    cleaned: bool,
}

impl TreeishWorktree {
    pub fn create(repo_root: &Path, treeish: &str) -> Result<Self, TreeishError> {
        let treeish = treeish.trim();
        if treeish.is_empty() {
            return Err(TreeishError::ReferenceRequired);
        }
        let repo_root = std::fs::canonicalize(repo_root)
            .map_err(|_| TreeishError::Unavailable { operation: "repository" })?;
        let path = treeish_temp_path();
        let mut worktree = Self { repo_root, path, cleaned: false };
        let add_result = run_treeish_git_status(
            &worktree.repo_root,
            &["worktree", "add", "--detach", &worktree.path.to_string_lossy(), treeish],
            TREEISH_GIT_TIMEOUT,
        );
        if let Err(error) = add_result {
            let mapped = match error {
                TreeishError::Timeout { .. } => TreeishError::Timeout { operation: "worktree add" },
                TreeishError::Unavailable { .. } => TreeishError::Invalid {
                    reference: treeish.to_owned(),
                    operation: "worktree add",
                },
                other => other,
            };
            // `git worktree add` can register its metadata before a timeout;
            // clean that partial checkout before returning the typed failure.
            let cleanup = worktree.cleanup();
            return match cleanup {
                Ok(()) => Err(mapped),
                Err(cleanup_error) => Err(cleanup_error),
            };
        }
        Ok(worktree)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn cleanup(&mut self) -> Result<(), TreeishError> {
        if self.cleaned {
            return Ok(());
        }
        // Git must unregister worktree before its directory is removed. A
        // retry handles transient index/lock contention; prune after removal
        // clears metadata left by a failed remove.
        let mut removed = false;
        for _ in 0..3 {
            if run_treeish_git_status(
                &self.repo_root,
                &["worktree", "remove", "--force", &self.path.to_string_lossy()],
                TREEISH_CLEANUP_TIMEOUT,
            )
            .is_ok()
            {
                removed = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let directory_removed = !self.path.exists() || std::fs::remove_dir_all(&self.path).is_ok();
        if !removed && directory_removed {
            let _ = run_treeish_git_status(
                &self.repo_root,
                &["worktree", "prune", "--expire", "now"],
                TREEISH_CLEANUP_TIMEOUT,
            );
            removed = true;
        }
        self.cleaned = removed && directory_removed && !worktree_registered(&self.repo_root, &self.path);
        if self.cleaned { Ok(()) } else { Err(TreeishError::CleanupFailed) }
    }
}

impl Drop for TreeishWorktree {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}

/// Materialise one treeish, run graph-building callback, then unregister the
/// temporary worktree before returning. A panic still reaches `Drop`.
pub fn with_treeish_worktree<T, E, F>(
    repo_root: &Path,
    treeish: &str,
    callback: F,
) -> Result<T, TreeishError>
where
    E: Display,
    F: FnOnce(&Path) -> Result<T, E>,
{
    let mut worktree = TreeishWorktree::create(repo_root, treeish)?;
    let result = callback(worktree.path()).map_err(|error| TreeishError::Callback(error.to_string()));
    let cleanup = worktree.cleanup();
    match (result, cleanup) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), Ok(())) => Err(error),
        (Err(_), Err(cleanup_error)) => Err(cleanup_error),
        (Ok(_), Err(error)) => Err(error),
    }
}

// ---------------------------------------------------------------------
// apply_file_delta
// ---------------------------------------------------------------------

use crate::store_delta::{
    acknowledge_file_state, acknowledge_journal, delete_facts_by_owner, delete_file_row,
    delete_file_state, ensure_watch_state, has_table, load_file_state, read_clock,
    read_manifest, read_source_observation, refresh_dependencies, refresh_manifest_counts,
    reseal_generation_identity_delta, root_digest_tx, select_fact_owners, set_clock,
    unresolve_edges_targeting, update_file_state, upsert_parsed_edge, upsert_parsed_node,
    FactOwnerRow,
};
use crate::merkle_ledger::update_leaf_chain;
use rusqlite::{params, Connection, Transaction};
use serde_json::{Map, Value};

/// Mirrors `delta.eventKind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    Create,
    Modify,
    Delete,
    Rename,
    Repair,
}

/// One `factBatches[]` entry: a provider's parsed nodes/edges/dependencies
/// for this file. Mirrors `{ provider, parsed: { nodes, edges,
/// dependencies }, fileReport }` (this port does not use `fileReport`; see
/// module docs on `provider_ranks`/search-projection scope).
#[derive(Debug, Clone, Default)]
pub struct FactBatch {
    pub provider_id: String,
    pub provider_version: String,
    pub nodes: Vec<Value>,
    pub edges: Vec<Value>,
    /// `(sourcePath, dependentPath, reason)` triples, mirrors
    /// `parsed.dependencies`.
    pub dependencies: Vec<(String, String, String)>,
}

/// Mirrors the subset of the legacy `delta` object this port acts on.
#[derive(Debug, Clone)]
pub struct FileDelta {
    pub path: String,
    pub event_kind: EventKind,
    /// Raw digest (with or without the `xxh128:` prefix); `None` means the
    /// legacy `delta.contentDigest == null`.
    pub content_digest: Option<String>,
    pub rename_to: Option<String>,
    pub fact_batches: Vec<FactBatch>,
    pub source_clock: Option<i64>,
    pub journal_seq: Option<i64>,
    pub size: Option<i64>,
    pub mtime_ms: Option<f64>,
    pub file_identity: Option<String>,
    /// Mirrors `isDocumentDelta`; see module docs — a `true` value makes
    /// [`apply_file_delta`] return [`ApplyFileDeltaError::UnsupportedDocumentDelta`].
    pub is_document_delta: bool,
    /// Optional publication metadata supplied by the refresh coordinator.
    /// Keeping it on the delta makes graph identity and source observation
    /// part of the same transaction as row mutation.
    pub source_hash: Option<String>,
    /// Resolver configuration identity for the complete source scan.
    pub config_digest: Option<String>,
    pub source_observation: Option<Value>,
    pub file_report: Option<Value>,
}

impl Default for FileDelta {
    fn default() -> Self {
        Self {
            path: String::new(),
            event_kind: EventKind::Modify,
            content_digest: None,
            rename_to: None,
            fact_batches: Vec::new(),
            source_clock: None,
            journal_seq: None,
            size: None,
            mtime_ms: None,
            file_identity: None,
            is_document_delta: false,
            source_hash: None,
            config_digest: None,
            source_observation: None,
            file_report: None,
        }
    }
}

/// Mirrors `options` (`{ inTransaction, deferJournalAck }`); see module docs
/// on why `inTransaction` composition is not supported.
#[derive(Debug, Clone, Copy, Default)]
pub struct ApplyOptions {
    pub defer_journal_ack: bool,
}

/// Mirrors `applyFileDelta`'s return shape.
#[derive(Debug, Clone, PartialEq)]
pub struct ApplyResult {
    pub applied: bool,
    pub noop: bool,
    pub path: String,
    pub applied_clock: i64,
    pub order_writes: i64,
    pub root_digest: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ApplyFileDeltaError {
    #[error("document deltas are not supported by this native port (path: {0}); see module docs")]
    UnsupportedDocumentDelta(String),
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error("{0}")]
    Store(String),
}

/// Native port of `applyFileDelta` for structural (non-document) deltas.
/// See the module docs for the exact behavioral contract and deviations.
pub fn apply_file_delta(
    conn: &mut Connection,
    delta: &FileDelta,
    options: ApplyOptions,
) -> Result<ApplyResult, ApplyFileDeltaError> {
    let tx = conn.transaction()?;
    let result = apply_file_delta_tx(&tx, delta, options)?;
    tx.commit()?;
    Ok(result)
}

/// Apply one source event set inside one caller-owned transaction. All deltas
/// either publish together or leave prior generation identity untouched.
pub fn apply_file_deltas(
    conn: &mut Connection,
    deltas: &[FileDelta],
    options: ApplyOptions,
) -> Result<Vec<ApplyResult>, ApplyFileDeltaError> {
    let tx = conn.transaction()?;
    let mut results = Vec::with_capacity(deltas.len());
    for delta in deltas {
        results.push(apply_file_delta_tx(&tx, delta, options)?);
    }
    tx.commit()?;
    Ok(results)
}

fn apply_file_delta_tx(
    tx: &Transaction<'_>,
    delta: &FileDelta,
    options: ApplyOptions,
) -> Result<ApplyResult, ApplyFileDeltaError> {
    let path = delta.path.replace('\\', "/");
    if delta.is_document_delta {
        return Err(ApplyFileDeltaError::UnsupportedDocumentDelta(path));
    }
    let content_digest_value = delta.content_digest.as_deref().map(normalize_digest);
    let new_path = match delta.event_kind {
        EventKind::Rename => delta.rename_to.as_deref().map(|p| p.replace('\\', "/")).unwrap_or_else(|| path.clone()),
        _ => path.clone(),
    };

    let prior = load_file_state(&tx, &path)?;
    ensure_watch_state(&tx)?;
    let previous_applied = read_clock(&tx, "applied_clock", 0)?;
    let applied_clock = delta.source_clock.unwrap_or(previous_applied + 1);

    // --- noop short-circuit -------------------------------------------------
    let is_short_circuit_eligible = !matches!(delta.event_kind, EventKind::Delete | EventKind::Rename | EventKind::Repair);
    if let (Some(digest), Some(prior_state)) = (content_digest_value.as_deref(), prior.as_ref()) {
        if is_short_circuit_eligible && prior_state.content_digest == digest {
            let acknowledged_clock = previous_applied.max(applied_clock);
            set_clock(&tx, "applied_clock", acknowledged_clock)?;
            acknowledge_file_state(
                &tx,
                &path,
                acknowledged_clock,
                delta.journal_seq,
                delta.size,
                delta.mtime_ms,
                delta.file_identity.as_deref(),
            )?;
            if let Some(seq) = delta.journal_seq {
                if !options.defer_journal_ack && has_table(&tx, "event_journal")? {
                    acknowledge_journal(&tx, seq, acknowledged_clock)?;
                }
            }
            let root_digest = root_digest_tx(&tx)?;
            return Ok(ApplyResult {
                noop: true,
                applied: true,
                path,
                applied_clock: acknowledged_clock,
                order_writes: 0,
                root_digest,
            });
        }
    }

    let structural_providers: Vec<String> = {
        let mut seen = std::collections::BTreeSet::new();
        for batch in &delta.fact_batches {
            if !batch.provider_id.is_empty() {
                seen.insert(batch.provider_id.clone());
            }
        }
        seen.into_iter().collect()
    };
    // An empty provider batch is a complete replacement for this source
    // path. It is used by dependency repair so stale lexical/tree-sitter
    // owners cannot survive beside refreshed cross-file facts.
    let replace_all_providers = delta.fact_batches.iter().any(|batch| batch.provider_id.is_empty());

    let old_owners: Vec<FactOwnerRow> = if matches!(delta.event_kind, EventKind::Delete | EventKind::Rename) || replace_all_providers || structural_providers.is_empty() {
        select_fact_owners(&tx, &path, None)?
    } else {
        select_fact_owners(&tx, &path, Some(&structural_providers))?
    };
    let old_node_ids: Vec<String> = old_owners
        .iter()
        .filter(|owner| owner.fact_kind == "node")
        .map(|owner| owner.fact_id.clone())
        .collect();

    if matches!(delta.event_kind, EventKind::Delete | EventKind::Rename) {
        unresolve_edges_targeting(&tx, &old_node_ids)?;
        delete_facts_by_owner(&tx, &path, None)?;
        delete_file_row(&tx, &path)?;
        delete_file_state(&tx, &path)?;
        update_leaf_chain(&tx, &path, None)?;
    } else {
        if replace_all_providers {
            delete_facts_by_owner(&tx, &path, None)?;
        } else {
            for provider_id in &structural_providers {
                delete_facts_by_owner(&tx, &path, Some(provider_id.as_str()))?;
            }
        }
    }

    refresh_dependencies(&tx, &path, &[])?;

    let order_writes: i64 = 0;
    let root_digest;

    if !matches!(delta.event_kind, EventKind::Delete) && !delta.fact_batches.is_empty() {
        let mut manifest = read_manifest(&tx)?.ok_or_else(|| {
            ApplyFileDeltaError::Store("generation envelope is missing manifest".into())
        })?;
        let source_observation = delta.source_observation.clone().or(read_source_observation(&tx)?);
        let root_before = update_leaf_chain(&tx, &new_path, content_digest_value.as_deref())?;
        reseal_generation_identity_delta(&mut manifest, source_observation.as_ref(), Some(root_before.as_str()), applied_clock)
            .map_err(ApplyFileDeltaError::Store)?;
        let generation_id = manifest
            .get("generationId")
            .and_then(Value::as_str)
            .ok_or_else(|| ApplyFileDeltaError::Store("resealed manifest missing generationId".into()))?
            .to_owned();

        let mut dependencies: Vec<(String, String, String)> = Vec::new();
        let source_digest_for_facts = content_digest_value.clone().unwrap_or_default();
        for batch in &delta.fact_batches {
            let provider_id = if batch.provider_id.is_empty() { "native-rust" } else { batch.provider_id.as_str() };
            for node in &batch.nodes {
                upsert_parsed_node(
                    &tx,
                    node,
                    &generation_id,
                    &source_digest_for_facts,
                    provider_id,
                    &batch.provider_version,
                    None,
                )
                .map_err(ApplyFileDeltaError::Store)?;
            }
            for edge in &batch.edges {
                let source_node_path = edge.get("source").and_then(Value::as_str).and_then(|source_id| {
                    batch.nodes.iter().find(|node| node.get("id").and_then(Value::as_str) == Some(source_id))
                        .and_then(|node| node.get("path").and_then(Value::as_str))
                });
                upsert_parsed_edge(
                    &tx,
                    edge,
                    &generation_id,
                    &source_digest_for_facts,
                    provider_id,
                    &batch.provider_version,
                    source_node_path,
                )
                .map_err(ApplyFileDeltaError::Store)?;
            }
            dependencies.extend(batch.dependencies.iter().cloned());
        }
        let mut unique = std::collections::BTreeMap::new();
        for dependency in dependencies {
            unique.insert(format!("{}:{}:{}", dependency.0, dependency.1, dependency.2), dependency);
        }
        let unique_dependencies: Vec<(String, String, String)> = unique.into_values().collect();
        refresh_dependencies(&tx, &new_path, &unique_dependencies)?;
        update_file_state(
            &tx,
            &new_path,
            content_digest_value.as_deref().unwrap_or_default(),
            applied_clock,
            delta.file_identity.as_deref(),
            delta.size.unwrap_or(0),
            delta.mtime_ms,
            delta.journal_seq,
        )?;
        refresh_manifest_counts(&tx, &mut manifest)?;
        if let Some(source_hash) = &delta.source_hash {
            manifest.as_object_mut().unwrap().insert("sourceHash".into(), Value::String(source_hash.clone()));
        }
        if let Some(config_digest) = &delta.config_digest {
            manifest.as_object_mut().unwrap().insert("configDigest".into(), Value::String(config_digest.clone()));
        }
        let digest = crate::identity::compute_manifest_digest_value(&manifest, source_observation.as_ref());
        manifest.as_object_mut().unwrap().insert("manifestDigest".into(), Value::String(digest));
        crate::store_delta::write_manifest(&tx, &manifest)?;
        root_digest = root_digest_tx(&tx)?;
    } else {
        // Delete, or a non-delete event with no fact batches: reseal without
        // touching node/edge rows, mirroring the legacy `else if
        // (!isDocumentDelta)` branch.
        if let Some(mut manifest) = read_manifest(&tx)? {
            let source_observation = delta.source_observation.clone().or(read_source_observation(&tx)?);
            let leaf_root = update_leaf_chain(&tx, &path, content_digest_value.as_deref())?;
            reseal_generation_identity_delta(&mut manifest, source_observation.as_ref(), Some(leaf_root.as_str()), applied_clock)
                .map_err(ApplyFileDeltaError::Store)?;
            refresh_manifest_counts(&tx, &mut manifest)?;
            if let Some(source_hash) = &delta.source_hash {
                manifest.as_object_mut().unwrap().insert("sourceHash".into(), Value::String(source_hash.clone()));
            }
            if let Some(config_digest) = &delta.config_digest {
                manifest.as_object_mut().unwrap().insert("configDigest".into(), Value::String(config_digest.clone()));
            }
            let digest = crate::identity::compute_manifest_digest_value(&manifest, source_observation.as_ref());
            manifest.as_object_mut().unwrap().insert("manifestDigest".into(), Value::String(digest));
            crate::store_delta::write_manifest(&tx, &manifest)?;
        }
        root_digest = root_digest_tx(&tx)?;
    }

    set_clock(&tx, "applied_clock", applied_clock)?;
    if let Some(seq) = delta.journal_seq {
        if !options.defer_journal_ack && has_table(&tx, "event_journal")? {
            acknowledge_journal(&tx, seq, applied_clock)?;
        }
    }

    if let Some(observation) = &delta.source_observation {
        write_source_observation(&tx, observation)?;
    }
    if let Some(report) = &delta.file_report {
        update_file_report(&tx, &path, report)?;
    }
    // A delta publishes a new generation for the complete graph body. Keep
    // every generation-bound row aligned with the resealed manifest, not only
    // rows touched by this file, so pinned reads cannot observe mixed ids.
    if let Some(manifest) = read_manifest(&tx)? {
        if let Some(generation_id) = manifest.get("generationId").and_then(Value::as_str) {
            for table in ["files", "symbols", "annotation_nodes", "edges", "vectors", "symbol_search", "symbol_terms", "fact_owner", "documents", "claims", "claim_code_edges", "document_supersession"] {
                tx.execute(&format!("UPDATE {table} SET generation_id=?1"), params![generation_id])?;
            }
            crate::store_delta::write_manifest(&tx, &manifest)?;
        }
    }
    Ok(ApplyResult {
        applied: true,
        noop: false,
        path,
        applied_clock,
        order_writes,
        root_digest,
    })
}

fn write_source_observation(tx: &Transaction<'_>, observation: &Value) -> rusqlite::Result<()> {
    tx.execute(
        "INSERT INTO generation(key,value) VALUES('sourceObservation',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![serde_json::to_string(observation).unwrap()],
    )?;
    Ok(())
}

fn update_file_report(tx: &Transaction<'_>, path: &str, report: &Value) -> rusqlite::Result<()> {
    let Some(object) = report.as_object() else { return Ok(()); };
    let mut extra = Map::new();
    extra.insert("__fileReport".into(), report.clone());
    tx.execute(
        "UPDATE files SET language=?1, provider=?2, parse_status=?3, error_node_count=?4, extra=?5 WHERE path=?6",
        params![
            object.get("language").and_then(Value::as_str),
            object.get("provider").and_then(Value::as_str),
            object.get("parseStatus").and_then(Value::as_str),
            object.get("errorNodeCount").and_then(Value::as_i64),
            serde_json::to_string(&Value::Object(extra)).unwrap(),
            path,
        ],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_digest_prefixes_once() {
        assert_eq!(normalize_digest("abc"), "xxh128:abc");
        assert_eq!(normalize_digest("xxh128:abc"), "xxh128:abc");
    }

    #[test]
    fn pending_domains_from_stored_sorts_but_does_not_dedup() {
        // Matches legacy `readPendingDomains`: split/trim/filter/sort only —
        // no dedup on read (a duplicate could only arise from a hand-edited
        // or corrupted stored value, since `mark` itself is dedup-safe).
        let domains = PendingDomains::from_stored(" doc, structural ,doc");
        assert_eq!(domains.domains(), &["doc".to_string(), "doc".to_string(), "structural".to_string()]);
    }

    #[test]
    fn pending_domains_mark_is_dedup_safe() {
        let mut domains = PendingDomains::from_stored("");
        domains.mark("doc");
        domains.mark("structural");
        domains.mark("doc");
        assert_eq!(domains.to_stored().as_deref(), Some("doc,structural"));
    }

    #[test]
    fn pending_domains_mark_then_clear() {
        let mut domains = PendingDomains::from_stored("");
        assert_eq!(domains.to_stored(), None);
        domains.mark("doc");
        domains.mark("structural");
        domains.mark("doc"); // idempotent
        assert_eq!(domains.to_stored().as_deref(), Some("doc,structural"));
        domains.clear("doc");
        assert_eq!(domains.to_stored().as_deref(), Some("structural"));
        domains.clear("structural");
        assert_eq!(domains.to_stored(), None);
    }

    #[test]
    fn replace_by_source_preserves_position_and_order() {
        let entries = vec![
            SourceKeyed { source: "a.md".into(), payload: 1 },
            SourceKeyed { source: "b.md".into(), payload: 2 },
            SourceKeyed { source: "a.md".into(), payload: 3 },
            SourceKeyed { source: "c.md".into(), payload: 4 },
        ];
        let replacements = vec![SourceKeyed { source: "a.md".into(), payload: 99 }];
        let result = replace_by_source(&entries, "a.md", &replacements);
        assert_eq!(
            result,
            vec![
                SourceKeyed { source: "a.md".into(), payload: 99 },
                SourceKeyed { source: "b.md".into(), payload: 2 },
                SourceKeyed { source: "c.md".into(), payload: 4 },
            ]
        );
    }

    #[test]
    fn replace_by_source_appends_when_no_match() {
        let entries = vec![SourceKeyed { source: "b.md".into(), payload: 2 }];
        let replacements = vec![SourceKeyed { source: "a.md".into(), payload: 1 }];
        let result = replace_by_source(&entries, "a.md", &replacements);
        assert_eq!(
            result,
            vec![
                SourceKeyed { source: "b.md".into(), payload: 2 },
                SourceKeyed { source: "a.md".into(), payload: 1 },
            ]
        );
    }
}
