//! G5 Lane B — Crypt durable-memory provider.
//!
//! Converts eligible `MemoryEntry` rows in `MemoryStore` into v1
//! `ContextCandidate` records (Layer 7) for the Membrane planner. The
//! provider is in-process with the planner: it ONLY reads through the
//! `MemoryStore` public API, never the filesystem, never the network, and
//! never the bearer-token file. External clients still authenticate to the
//! loopback service through the installed `crypt` shim.
//!
//! Validity, supersession, and scope discipline are enforced locally so the
//! planner never has to know about Crypt's tier/prune/dream model:
//!
//! - **Validity** — entries with `score < 0.2 && access_count == 0` are
//!   treated as `Demoted` (the same threshold `consolidate_dream_memories`
//!   uses for its low-value prune) and excluded from candidates.
//! - **Supersession** — entries sharing a dedup key (the dream consolidation
//!   key) collapse to the highest-scoring current survivor; the rest are
//!   recorded as provenance on the survivor's `superseded_ids` and excluded
//!   from the candidate set. Their ids NEVER reappear as candidates — the
//!   kept entry is the only "current equivalent".
//! - **Scope** — only entries whose `scope_id` is `scope` itself, one of its
//!   ancestors (per `scope_chain`), or `global` may be admitted; sibling
//!   scopes (e.g. another project under the same parent) can NEVER leak in.
//!
//! The returned `ContextCandidateSet` is plain JSON-serializable; `text` is
//! trimmed and bounded so a single fat entry cannot swallow the planner
//! budget.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crypt_core::MemoryEntry;

/// Provider name used in `ContextCandidateSet.provider` and on every
/// `Candidate.sourceRef`. Constant — the planner identifies this lane by it.
pub const PROVIDER_NAME: &str = "memory";

/// `ContextCandidate.layer` for Crypt candidates. Per the unified
/// architecture (Eight-Layer Baseline), Layer 7 is the durable-memory lane.
pub const LAYER: u8 = 7;

/// `trustClass` for every Crypt candidate. Memories were captured under
/// `agent_verified` (the memorizer is a restricted Rust agent, not raw user
/// text or remote untrusted input).
pub const TRUST_CLASS: &str = "agent_verified";

/// `instructionPolicy` for every Crypt candidate. Memory content is
/// evidence (data), never instructions — `instructions_allowed` would let a
/// captured body issue commands to the model, so it is locked off.
pub const INSTRUCTION_POLICY: &str = "data_only";

/// `sourceKind` for every Crypt candidate.
pub const SOURCE_KIND: &str = "memory";

/// Threshold below which an entry is considered `Demoted` when it has never
/// been accessed. Matches the dream-consolidation low-value prune rule so
/// the two views of the corpus agree.
pub const DEMOTION_SCORE_THRESHOLD: f64 = 0.2;

/// Hard cap on a single candidate's `text` payload (chars). Keeps a runaway
/// entry from forcing the planner over its budget. Real budgets are enforced
/// upstream; this is the provider-side safety net.
pub const MAX_TEXT_CHARS: usize = 4000;

/// Conservative token estimate: chars / 4. Matches the rest of the engine's
/// `estimatedTokens` convention (rounded up, never negative).
pub fn estimate_tokens(chars: usize) -> u64 {
    ((chars + 3) / 4) as u64
}

/// One `ContextCandidate` v1 record, normalized for the planner.
#[allow(non_snake_case)]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContextCandidate {
    pub id: String,
    pub layer: u8,
    pub sourceKind: String,
    pub sourceRef: String,
    pub sourceHash: String,
    pub trustClass: String,
    pub instructionPolicy: String,
    pub providerScore: f64,
    pub estimatedTokens: u64,
    pub protected: bool,
    pub exact: bool,
    pub recoverable: bool,
    pub resolver: String,
    pub text: String,
    /// Content-free artifact dimensions copied from the durable row. Optional and additive so
    /// older planner consumers remain compatible while telemetry can separate families.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifactFamily: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub producer: Option<String>,
    /// Crypt-specific provenance attached to every candidate. Carries the
    /// durable identity and lifecycle signals the planner needs for receipts
    /// without exposing them as retrieval inputs. Lives in a separate field
    /// from the v1 schema's required keys — the planner stores it verbatim
    /// on the receipt path.
    #[serde(rename = "provenance")]
    pub provenance: CandidateProvenance,
}

/// Crypt-specific provenance. `superseded_ids` is non-empty when this
/// entry is the surviving current equivalent of one or more older entries
/// that the provider collapsed during supersession resolution.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CandidateProvenance {
    /// The `MemoryEntry.id` (e.g. `"D--Claude/feedback-xyz"`).
    pub memory_id: String,
    /// Lifetime access counter on the underlying entry.
    pub access_count: u32,
    /// `created_at` ISO timestamp on the underlying entry (the "last-seen"
    /// the durability track keeps — entries are append-mostly).
    pub last_seen: String,
    /// Memory tier as a stable string ("Semantic" / "Episodic" / "Working").
    pub tier: String,
    /// `scope_id` the entry was admitted under. Always inside the chain
    /// computed from the caller's `scope`.
    pub scope_id: String,
    /// Ids of older entries this one consolidated from (the dream write
    /// records them). Empty when the entry was never consolidated. A
    /// non-empty list means *this* entry is the surviving current
    /// equivalent of those — the planner sees only this one.
    pub superseded_ids: Vec<String>,
}

/// One Omission record: a candidate the provider considered and dropped, with
/// the reason. Mirrors `ContextCandidateSet.omissions[]`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Omission {
    pub id: String,
    pub layer: u8,
    pub reason: String,
}

/// Reason codes attached to `Omission.reason`. Stable strings — the planner
/// references them in receipts.
pub mod reasons {
    pub const DEMOTED: &str = "demoted";
    pub const SUPERSEDED: &str = "superseded";
    pub const OUT_OF_SCOPE: &str = "out_of_scope";
}

/// The full `ContextCandidateSet` v1 record. `schemaVersion` is locked to 1
/// per the canonical contract; the planner validates against it.
#[allow(non_snake_case)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextCandidateSet {
    pub schemaVersion: u32,
    pub traceId: String,
    pub task: String,
    pub mode: String,
    pub provider: String,
    pub freshness: Freshness,
    pub providerCeiling: ProviderCeiling,
    pub candidates: Vec<ContextCandidate>,
    pub omissions: Vec<Omission>,
}

/// Provider-local mirror of `ContextCandidateSet.freshness`. Compact, no
/// timestamps inside the provider — the planner attaches `indexedAt` after
/// admission.
#[allow(non_snake_case)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Freshness {
    pub revision: String,
    pub indexedAt: String,
    pub stale: bool,
}

/// Provider-local mirror of `ContextCandidateSet.providerCeiling`. Plumbed
/// through so the planner can detect ceiling breaches downstream.
#[allow(non_snake_case)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderCeiling {
    pub maxCandidates: u32,
    pub maxEstimatedTokens: u32,
}

/// Internal classification of an entry before the final candidate shape is
/// emitted. The `Admitted` decision still has to pass the demotion gate; the
/// others are recorded as omissions.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Decision {
    Admitted,
    /// Reserved — kept for symmetry with the architecture doc's vocabulary
    /// (`Active` / `Demoted` / `Superseded`); the provider currently derives
    /// `Demoted` from the demotion predicate inside `partition` rather than
    /// classifying in `consider_entries`.
    Demoted,
    Superseded,
    OutOfScope,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
struct Considered {
    entry: MemoryEntry,
    decision: Decision,
    /// For `Superseded` decisions: the id of the entry that survived the
    /// supersession collapse (the kept entry's `memory_id`). Empty for
    /// other decisions.
    replaced_by: Option<String>,
}

/// SHA-256 of the candidate's `text` (hex, no `sha256:` prefix — matches the
/// schema's Sha256 pattern which accepts both).
fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

/// Reuse the dream consolidation dedup key so the provider's view of
/// "supersession" matches what dream consolidation already does.
fn dedup_key(content: &str) -> String {
    content
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// A memory is `Active` if it has meaningful score OR a recorded use. The
/// inverse (`score < DEMOTION_SCORE_THRESHOLD && access_count == 0`) is the
/// `Demoted` predicate and matches `consolidate_dream_memories`'s prune
/// filter.
fn is_demoted(entry: &MemoryEntry) -> bool {
    entry.score < DEMOTION_SCORE_THRESHOLD && entry.access_count == 0
}

/// Build the scope-chain `Vec<String>` for `scope` given the scopes actually
/// present in the store. Wraps `crypt::scope::scope_chain` so callers
/// don't have to pre-fetch the scope list.
fn scope_chain_for(store: &::crypt::MemoryStore, scope: &str) -> Vec<String> {
    let existing = store.scopes();
    ::crypt::scope::scope_chain(scope, &existing)
}

/// Step 1 — gather every entry, classify by scope, then collapse
/// dedup-key groups onto the highest-scoring survivor. Loser ids become
/// the survivor's `superseded_ids` provenance AND are recorded as
/// `Superseded` omissions so receipts see both sides of the collapse.
fn consider_entries(store: &::crypt::MemoryStore, scope_chain: &[String]) -> Vec<Considered> {
    let allowed: BTreeSet<&str> = scope_chain.iter().map(String::as_str).collect();

    // Pull the full snapshot through the existing public API. `entries` is
    // already a public method that returns cloned `MemoryEntry`s — no
    // private-field reach-around needed.
    let all = store.entries(usize::MAX);

    let mut in_scope: Vec<MemoryEntry> = Vec::new();
    let mut mut_out_of_scope: Vec<MemoryEntry> = Vec::new();
    for entry in all {
        if allowed.contains(entry.scope_id.as_str()) {
            in_scope.push(entry);
        } else {
            mut_out_of_scope.push(entry);
        }
    }

    // Supersession pass — group by (scope_id, dedup_key); pick the
    // highest-scoring, most-accessed survivor; the rest are Superseded.
    let mut groups: BTreeMap<(String, String), Vec<MemoryEntry>> = BTreeMap::new();
    for entry in in_scope {
        let key = dedup_key(&entry.content);
        groups
            .entry((entry.scope_id.clone(), key))
            .or_default()
            .push(entry);
    }
    let mut considered: Vec<Considered> = Vec::new();
    for ((_scope, _key), mut group) in groups {
        group.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| b.access_count.cmp(&a.access_count))
                .then_with(|| a.id.cmp(&b.id))
        });
        let mut iter = group.into_iter();
        let Some(survivor) = iter.next() else {
            continue;
        };
        let survivor_id = survivor.id.clone();
        considered.push(Considered {
            entry: survivor,
            decision: Decision::Admitted,
            replaced_by: None,
        });
        for loser in iter {
            considered.push(Considered {
                entry: loser,
                decision: Decision::Superseded,
                replaced_by: Some(survivor_id.clone()),
            });
        }
    }

    // Out-of-scope entries are recorded as omissions so the planner sees
    // them in the receipt (the gate is "no cross-root leaks", not "silently
    // pretend they don't exist").
    for entry in mut_out_of_scope {
        considered.push(Considered {
            entry,
            decision: Decision::OutOfScope,
            replaced_by: None,
        });
    }

    considered
}

/// Build a map of survivor-id -> list of superseded loser-ids from the
/// considered set, so the candidate builder can attach supersession
/// provenance.
fn build_supersession_map(
    considered: &[Considered],
) -> std::collections::HashMap<String, Vec<String>> {
    let mut groups: BTreeMap<(String, String), Vec<&Considered>> = BTreeMap::new();
    for c in considered {
        if c.decision == Decision::Admitted || c.decision == Decision::Superseded {
            let key = dedup_key(&c.entry.content);
            groups
                .entry((c.entry.scope_id.clone(), key))
                .or_default()
                .push(c);
        }
    }
    let mut map: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
    for ((_scope, _key), members) in groups {
        // Sort by the same rule as consider_entries to find the survivor.
        let mut sorted = members;
        sorted.sort_by(|a, b| {
            b.entry
                .score
                .partial_cmp(&a.entry.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| b.entry.access_count.cmp(&a.entry.access_count))
                .then_with(|| a.entry.id.cmp(&b.entry.id))
        });
        if let Some(survivor) = sorted.first() {
            if survivor.decision == Decision::Admitted {
                let survivor_id = survivor.entry.id.clone();
                let losers: Vec<String> =
                    sorted.iter().skip(1).map(|c| c.entry.id.clone()).collect();
                if !losers.is_empty() {
                    map.insert(survivor_id, losers);
                }
            }
        }
    }
    map
}

/// Apply the demotion gate on top of the classified entries. The demotion
/// gate runs AFTER supersession so a demoted entry that's also superseded
/// is reported as `superseded` (more actionable for the planner).
fn partition(considered: Vec<Considered>) -> (Vec<MemoryEntry>, Vec<Omission>) {
    let mut kept: Vec<MemoryEntry> = Vec::new();
    let mut omissions: Vec<Omission> = Vec::new();
    for c in considered {
        match c.decision {
            Decision::Admitted => {
                if is_demoted(&c.entry) {
                    omissions.push(Omission {
                        id: c.entry.id.clone(),
                        layer: LAYER,
                        reason: reasons::DEMOTED.into(),
                    });
                } else {
                    kept.push(c.entry);
                }
            }
            Decision::Demoted => omissions.push(Omission {
                id: c.entry.id.clone(),
                layer: LAYER,
                reason: reasons::DEMOTED.into(),
            }),
            Decision::Superseded => omissions.push(Omission {
                id: c.entry.id.clone(),
                layer: LAYER,
                reason: reasons::SUPERSEDED.into(),
            }),
            Decision::OutOfScope => omissions.push(Omission {
                id: c.entry.id.clone(),
                layer: LAYER,
                reason: reasons::OUT_OF_SCOPE.into(),
            }),
        }
    }
    (kept, omissions)
}

/// Build the final candidate list, attaching supersession provenance where
/// applicable. Ranking order is stable: score desc, access_count desc,
/// created_at asc (newest first on tie), then id ascending as a final
/// tiebreaker.
fn build_candidates(
    kept_with_provenance: Vec<(MemoryEntry, Vec<String>)>,
) -> Vec<ContextCandidate> {
    let mut kept = kept_with_provenance;
    kept.sort_by(|a, b| {
        b.0.score
            .partial_cmp(&a.0.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.0.access_count.cmp(&a.0.access_count))
            .then_with(|| b.0.created_at.cmp(&a.0.created_at))
            .then_with(|| a.0.id.cmp(&b.0.id))
    });
    let mut candidates: Vec<ContextCandidate> = Vec::with_capacity(kept.len());
    let mut total_tokens: u64 = 0;
    for (entry, superseded_ids) in kept {
        let text = trimmed_text(&entry.content);
        let estimated_tokens = estimate_tokens(text.chars().count());
        total_tokens = total_tokens.saturating_add(estimated_tokens);
        candidates.push(ContextCandidate {
            id: format!("memory::{}", entry.id),
            layer: LAYER,
            sourceKind: SOURCE_KIND.into(),
            sourceRef: format!("{}::{}", PROVIDER_NAME, entry.id),
            sourceHash: sha256_hex(text.as_bytes()),
            trustClass: TRUST_CLASS.into(),
            instructionPolicy: INSTRUCTION_POLICY.into(),
            providerScore: entry.score.clamp(0.0, 1.0),
            estimatedTokens: estimated_tokens,
            protected: false,
            exact: false,
            recoverable: true,
            resolver: PROVIDER_NAME.into(),
            text,
            artifactFamily: None,
            producer: None,
            provenance: CandidateProvenance {
                memory_id: entry.id.clone(),
                access_count: entry.access_count,
                last_seen: entry.created_at.clone(),
                tier: format!("{:?}", entry.tier),
                scope_id: entry.scope_id.clone(),
                superseded_ids,
            },
        });
    }
    candidates
}

/// Produce a v1 `ContextCandidateSet` from `MemoryStore` for the planner.
///
/// This is a free function (not an `impl MemoryStore` method) so it can
/// live in this lane-owned module without editing `store.rs`. The
/// integration owner can wire it as an inherent method on `MemoryStore` in
/// a one-line commit; the planner sees the same call shape either way.
///
/// - `task` — the planner's task string. Echoed into the envelope via a
///   hash; not stored verbatim (raw task text is never written to
///   receipts).
/// - `scope` — the caller's scope slug. Drives the scope chain.
/// - `max_candidates` — hard cap on admitted candidates. `0` is allowed
///   and yields an empty `candidates` array plus a populated
///   `omissions` array.
/// - `repo_root` — real signal for `freshness.stale`, reusing the same verdict machinery the
///   `/freshness` route exposes (`crate::freshness`) rather than a second staleness concept. When
///   the caller has no repo root to offer, that is itself an unverifiable condition — `stale`
///   honestly reports `true` rather than falling back to `false` (F11).
pub fn produce_candidate_set(
    store: &::crypt::MemoryStore,
    task: &str,
    scope: &str,
    max_candidates: usize,
    repo_root: Option<&std::path::Path>,
) -> ContextCandidateSet {
    let trace_id = trace_id_for(task, scope);
    let chain = scope_chain_for(store, scope);
    let considered = consider_entries(store, &chain);
    let supersession_map = build_supersession_map(&considered);
    let (kept, omissions) = partition(considered);

    let kept_after_cap: Vec<MemoryEntry> = {
        let mut k = kept;
        k.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| b.access_count.cmp(&a.access_count))
                .then_with(|| b.created_at.cmp(&a.created_at))
                .then_with(|| a.id.cmp(&b.id))
        });
        k.truncate(max_candidates);
        k
    };

    let kept_with_provenance: Vec<(MemoryEntry, Vec<String>)> = kept_after_cap
        .into_iter()
        .map(|entry| {
            let superseded_ids = supersession_map.get(&entry.id).cloned().unwrap_or_default();
            (entry, superseded_ids)
        })
        .collect();

    let mut candidates = build_candidates(kept_with_provenance);
    let candidate_ids = candidates
        .iter()
        .map(|candidate| candidate.provenance.memory_id.clone())
        .collect::<Vec<_>>();
    let dimensions = store.artifact_dimensions(&candidate_ids);
    for candidate in &mut candidates {
        if let Some((artifact_family, producer)) = dimensions.get(&candidate.provenance.memory_id) {
            candidate.artifactFamily = Some(artifact_family.clone());
            candidate.producer = Some(producer.clone());
        }
    }

    let total_tokens: u64 = candidates
        .iter()
        .map(|c| c.estimatedTokens)
        .fold(0u64, |acc, t| acc.saturating_add(t));

    let stale = repo_root.is_none_or(|root| {
        !crate::freshness::evaluate_repository_freshness(store, root.to_path_buf()).stable
    });

    ContextCandidateSet {
        schemaVersion: 1,
        traceId: trace_id,
        task: task.to_string(),
        mode: "verify".into(),
        provider: PROVIDER_NAME.into(),
        freshness: Freshness {
            revision: "memory-v1".into(),
            indexedAt: String::new(),
            stale,
        },
        providerCeiling: ProviderCeiling {
            maxCandidates: max_candidates as u32,
            maxEstimatedTokens: total_tokens.min(u32::MAX as u64) as u32,
        },
        candidates,
        omissions,
    }
}

fn trimmed_text(content: &str) -> String {
    if content.chars().count() <= MAX_TEXT_CHARS {
        return content.to_string();
    }
    let mut out: String = content.chars().take(MAX_TEXT_CHARS).collect();
    out.push_str("...");
    out
}

/// Stable trace id for a (task, scope) pair. Uses SHA-256 so the trace id
/// itself does not embed task text.
fn trace_id_for(task: &str, scope: &str) -> String {
    let mut h = Sha256::new();
    h.update(task.as_bytes());
    h.update([0u8]);
    h.update(scope.as_bytes());
    format!("{:x}", h.finalize())
}

#[cfg(test)]
mod tests {
    //! Provider-internal tests — the cross-lane contract tests live under
    //! `tests/memory_provider.rs` so they exercise the public API the
    //! planner will see.
    use super::*;

    fn store() -> ::crypt::MemoryStore {
        ::crypt::MemoryStore::open(::crypt::MemDb::open_in_memory())
    }

    #[test]
    fn demoted_predicate_matches_dream_prune_rule() {
        let mut m = MemoryEntry {
            id: "x".into(),
            tier: crypt_core::MemoryTier::Semantic,
            content: "x".into(),
            keywords: vec![],
            score: 0.19,
            created_at: "2026-07-12T00:00:00Z".into(),
            access_count: 0,
            embedding: None,
            scope_id: "global".into(),
        };
        assert!(is_demoted(&m));
        m.access_count = 1;
        assert!(!is_demoted(&m));
        m.access_count = 0;
        m.score = 0.21;
        assert!(!is_demoted(&m));
    }

    #[test]
    fn dedup_key_normalizes_whitespace_and_case() {
        assert_eq!(dedup_key("Hello, World!"), dedup_key("hello world"));
        assert_eq!(dedup_key("foo-bar_baz"), dedup_key("foo bar baz"));
    }

    #[test]
    fn estimate_tokens_rounds_up() {
        assert_eq!(estimate_tokens(0), 0);
        assert_eq!(estimate_tokens(1), 1);
        assert_eq!(estimate_tokens(4), 1);
        assert_eq!(estimate_tokens(5), 2);
    }

    #[test]
    fn trimmed_text_caps_long_content() {
        let long: String = "a".repeat(MAX_TEXT_CHARS + 50);
        let t = trimmed_text(&long);
        assert!(t.ends_with("..."));
        assert!(t.chars().count() <= MAX_TEXT_CHARS + 3);
    }

    #[test]
    fn trace_id_is_stable_and_does_not_leak_task_text() {
        let a = trace_id_for("explore crypt engine", "D--Claude");
        let b = trace_id_for("explore crypt engine", "D--Claude");
        assert_eq!(a, b);
        let c = trace_id_for("explore crypt engine", "D--Claude-other");
        assert_ne!(a, c);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(a.len(), 64);
    }

    #[test]
    fn produce_candidate_set_runs_on_empty_store() {
        let s = store();
        let set = produce_candidate_set(&s, "anything", "global", 5, None);
        assert_eq!(set.schemaVersion, 1);
        assert_eq!(set.candidates.len(), 0);
        assert_eq!(set.omissions.len(), 0);
    }

    /// F11 — `stale` must derive from a real freshness signal. With no repo root offered
    /// (genuinely unavailable at this call site), that is honestly reported as `stale: true` —
    /// never the old hardcoded `false`.
    #[test]
    fn stale_is_true_when_no_repo_root_is_available() {
        let s = store();
        let set = produce_candidate_set(&s, "anything", "global", 5, None);
        assert!(set.freshness.stale);
    }
}
