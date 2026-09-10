//! Partial native port of `blueprint/src/graph/delta-store.mjs`.
//!
//! `delta-store.mjs`'s centerpiece, `applyFileDelta`, is not a portable pure
//! function: it is an orchestration of writes against the legacy
//! `better-sqlite3` schema (`files`, `symbols`, `edges`, `fact_owner`,
//! `node_provider`, `dependency_index`, `file_state`, `watch_state`,
//! `event_journal`, `generation`/`generation_leaf` tables) interleaved with
//! `generation-identity.mjs` reseal calls and `merkle-ledger.mjs` leaf-chain
//! updates, plus filesystem side effects for doc artifacts
//! (`claims.json`/`stale.json`/`queue.json`). This crate's native engine
//! (`store.rs`, `engine.rs`) does not share that schema or an equivalent
//! incremental-apply entry point, so there is no native counterpart to port
//! *to* for the transactional core, and reproducing the legacy sqlite schema
//! wholesale is out of this lane's scope.
//!
//! What *is* ported here, exactly, because it is pure and self-contained:
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
