//! Portable skill-document adapter over Ledger's existing registration,
//! index and exact-resolution mechanisms (LDG-032).
//!
//! Skill bodies live in ordinary repository source files under
//! `tools/skills/<name>/SKILL.md`. Once `doc_spine` has synced the enrolled
//! worktree they are already source-bound Ledger projections: registration
//! stays in `ledger_doc_artifacts`, spans in `ledger_nodes`, and exact reads
//! stay on the ticketed `membrane_source_read` path. This module is only the
//! narrow lane that enumerates those registrations and scopes a query to
//! them — it adds no storage, no second index and no body copies.
//!
//! The adapter inherits every existing safeguard rather than re-implementing
//! them: source policy, erasure fences, generation/revision/span-hash
//! binding, grant narrowing, drift refusal at resolution, and bounded work.
//! Cortex may retain separately admitted durable skill insights; it never
//! owns the authoritative skill-body index.

use super::{
    limits::WorkBudget,
    policy::SourcePolicy,
    query::{self, LedgerHit},
    resolve, LedgerDb,
};
use membrane_protocol::ReadPathV1;
use rusqlite::params;
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::Path;

/// Portable skill documents are rooted at `tools/skills/<name>/SKILL.md`.
/// This is the same convention the portability store ingests; the adapter
/// adds no new layout.
pub const SKILL_DIRECTORY: &str = "tools/skills";
/// Only the skill entrypoint is a skill document; auxiliary files inside a
/// skill directory remain ordinary documents.
pub const SKILL_FILE_NAME: &str = "SKILL.md";
/// Matches the skills provider's published snapshot bound; the adapter never
/// widens it.
pub const MAX_SKILL_DOCUMENTS: usize = 512;
const MAX_SKILL_ID_BYTES: usize = 128;

/// Resolve a registered document path to its portable skill identity.
///
/// Returns `None` for anything that is not exactly
/// `tools/skills/<name>/SKILL.md` under the repository root: a deeper path,
/// a different file name, an empty or traversal-shaped name, or a name that
/// could not travel safely inside resolver arguments.
pub fn skill_id_from_path(path: &str) -> Option<String> {
    let rest = path.strip_prefix("tools/skills/")?;
    let (name, file) = rest.split_once('/')?;
    if file != SKILL_FILE_NAME
        || name.is_empty()
        || name.len() > MAX_SKILL_ID_BYTES
        || matches!(name, "." | "..")
        || name.contains('/')
        || name.chars().any(|c| c.is_control() || c == '\\')
    {
        return None;
    }
    Some(name.to_owned())
}

/// One registered skill document's index metadata. Source identity, revision,
/// content hash and publication generation are bound here; bodies are never
/// copied into this catalog.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillDocumentEntryV1 {
    pub skill_id: String,
    pub doc_id: String,
    pub path: String,
    pub title: String,
    pub summary: String,
    pub keywords: Vec<String>,
    pub revision: String,
    pub content_hash: String,
    pub ledger_generation: i64,
}

/// A query-lane hit on a registered skill document. The wrapped
/// [`LedgerHit`] carries the complete `membrane_source_read` binding —
/// document/node identity, source reference, expected content hash, expected
/// revision, expected span hash and Ledger generation — so the owner can
/// issue a resolution ticket over it unchanged.
#[derive(Clone, Debug)]
pub struct SkillDocumentHitV1 {
    pub skill_id: String,
    pub title: String,
    pub hit: LedgerHit,
}

/// One skill hit plus its owner-issued resolution ticket — the pair a Pull
/// candidate must carry so exact recovery stays ticket-bound.
#[derive(Clone, Debug)]
pub struct TicketedSkillDocumentV1 {
    pub skill_id: String,
    pub title: String,
    pub hit: LedgerHit,
    pub ticket: String,
}

/// Scoped skill-document search outcome: the underlying [`query::QueryResult`]
/// keeps its completeness, omission, lane and policy receipts while `skills`
/// carries only registered skill-document hits.
#[derive(Debug)]
pub struct SkillDocumentQueryV1 {
    pub result: query::QueryResult,
    pub skills: Vec<SkillDocumentHitV1>,
}

/// Enumerate registered skill documents for one canonical repository root.
///
/// The catalog reuses the same eligibility gates as document recall: the
/// artifact must be active and normal-sensitivity, its path must pass the
/// effective repository policy, and an erasure fence removes it. When the
/// caller carries a task grant, `ranges` narrows the catalog to granted paths
/// — enumeration may never disclose ungranted documents. Rows are index
/// metadata only; bodies are never loaded or returned.
pub(crate) fn catalog(
    db: &LedgerDb,
    root: &str,
    ranges: Option<&[ReadPathV1]>,
    budget: &WorkBudget,
) -> Result<Vec<SkillDocumentEntryV1>, String> {
    let normalized = resolve::normalized_root(Path::new(root)).map_err(|e| e.to_string())?;
    if normalized != root {
        return Err("ledger_root_binding_changed".into());
    }
    let mut policy = SourcePolicy::new(Path::new(&normalized))?;
    let rows: Vec<(String, String, String, String, String, String, String, i64)> = {
        let conn = db.lock();
        let mut statement = conn
            .prepare(
                "SELECT doc_id,path,title,summary,keywords_json,revision,content_hash,
                        index_generation
                 FROM ledger_doc_artifacts
                 WHERE repository_root=?1 AND lifecycle_state='active' AND sensitivity='normal'
                 ORDER BY path",
            )
            .map_err(|e| e.to_string())?;
        let collected = statement
            .query_map([&normalized], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                ))
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string());
        collected?
    };
    let mut entries = Vec::new();
    for (doc_id, path, title, summary, keywords_json, revision, content_hash, generation) in rows {
        budget.visit()?;
        let Some(skill_id) = skill_id_from_path(&path) else { continue };
        // A caller grant narrows enumeration to granted paths; it can never
        // widen the catalog past registered skill documents.
        if let Some(granted) = ranges {
            if !granted.iter().any(|range| range.path == path) {
                continue;
            }
        }
        if entries.len() >= MAX_SKILL_DOCUMENTS {
            return Err("ledger_skill_catalog_budget_exhausted".into());
        }
        let erased: bool = db
            .lock()
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM ledger_erasure_fences
                 WHERE repository_root=?1 AND path_digest=?2)",
                params![normalized, resolve::digest(path.as_bytes())],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        if erased || !policy.allows(&path, false, budget)? {
            continue;
        }
        entries.push(SkillDocumentEntryV1 {
            skill_id,
            doc_id,
            path,
            title,
            summary,
            keywords: serde_json::from_str(&keywords_json).unwrap_or_default(),
            revision,
            content_hash,
            ledger_generation: generation,
        });
    }
    policy.revalidate(budget)?;
    Ok(entries)
}

/// Search only registered skill documents through the existing scoped query
/// path (LDG-032).
///
/// The adapter's own source set is expressed as a grant-shaped narrowing: with
/// no caller grant the enrolled skill documents are the whole eligible set;
/// with one, only granted ranges on skill paths remain and their line bounds
/// still bind. An empty intersection is fail-closed — it yields no hits, it
/// never widens into the general document set. Eligibility, policy, erasure,
/// source loading, span-hash verification, ranking and graph receipts all run
/// inside [`query::search`]; this adapter only restricts the corpus and
/// re-attaches skill identity to each emitted hit.
pub(crate) fn search(
    db: &LedgerDb,
    root: &str,
    task: &str,
    k: usize,
    grant_ranges: Option<Vec<ReadPathV1>>,
    budget: &WorkBudget,
) -> Result<SkillDocumentQueryV1, String> {
    let entries = catalog(db, root, grant_ranges.as_deref(), budget)?;
    let by_doc: BTreeMap<&str, &SkillDocumentEntryV1> =
        entries.iter().map(|entry| (entry.doc_id.as_str(), entry)).collect();
    let ranges: Vec<ReadPathV1> = match grant_ranges {
        // Repository enrollment authorizes every registered skill document.
        None => entries
            .iter()
            .map(|entry| ReadPathV1 {
                path: entry.path.clone(),
                start_line: 1,
                end_line: u32::MAX,
            })
            .collect(),
        // A caller grant narrows further; it can never widen past registered
        // skill documents or past its own line ranges.
        Some(granted) => granted
            .into_iter()
            .filter(|range| by_doc.values().any(|entry| entry.path == range.path))
            .collect(),
    };
    let scope = query::QueryScope {
        root: root.to_owned(),
        ranges: Some(ranges),
    };
    let mut result = query::search(
        db,
        &scope,
        task,
        k.min(query::MAX_HITS).max(1),
        false,
        budget,
    )?;
    let mut skills = Vec::new();
    for hit in result.hits.iter() {
        budget.visit()?;
        match by_doc.get(hit.doc_id.as_str()) {
            Some(entry) => skills.push(SkillDocumentHitV1 {
                skill_id: entry.skill_id.clone(),
                title: if entry.title.is_empty() {
                    entry.skill_id.clone()
                } else {
                    entry.title.clone()
                },
                hit: hit.clone(),
            }),
            // The installed scope already restricts candidates to registered
            // skill paths; a stray hit is recorded as an omission, never
            // silently emitted.
            None => result.omissions.push("skill_hit_unbound".into()),
        }
    }
    if !result.omissions.is_empty() {
        result.omissions.sort();
        result.omissions.dedup();
        result.complete = false;
    }
    Ok(SkillDocumentQueryV1 { result, skills })
}

/// Materialize one catalog entry as the complete set of ticketable
/// top-level spans so the owner can issue resolution tickets (LDG-032
/// catalog-to-candidate half).
///
/// Reuses [`query::document_hits`], which applies the same source load and
/// span-hash verification as the query lanes; a drifted source fails here
/// before a ticket is ever issued. The ordered hits partition the document —
/// whole-file delivery is the set, never a synthesized node.
pub(crate) fn document_hits(
    db: &LedgerDb,
    root: &str,
    entry: &SkillDocumentEntryV1,
    budget: &WorkBudget,
) -> Result<Vec<SkillDocumentHitV1>, String> {
    budget.check()?;
    let title = if entry.title.is_empty() {
        entry.skill_id.clone()
    } else {
        entry.title.clone()
    };
    let hits = query::document_hits(db, root, &entry.doc_id, "ledger_skill", 1.0)?;
    Ok(hits
        .into_iter()
        .map(|hit| SkillDocumentHitV1 {
            skill_id: entry.skill_id.clone(),
            title: title.clone(),
            hit,
        })
        .collect())
}

/// The grant-bound half of [`document_hits`]: the caller's ranges must cover
/// the whole source before any span ticket may be issued for it.
///
/// Ticket validation binds request identity and grant validity, but path
/// coverage lives only in the query scope — mirroring `eligible`'s
/// line-range-to-byte mapping here keeps a narrow grant from ticketing a
/// document it does not cover. A grant over an unrelated path or a partial
/// line range is refused, never silently widened; partial reads remain
/// available through the scoped [`search`] lane.
pub(crate) fn document_hits_granted(
    db: &LedgerDb,
    root: &str,
    entry: &SkillDocumentEntryV1,
    ranges: &[ReadPathV1],
    budget: &WorkBudget,
) -> Result<Vec<SkillDocumentHitV1>, String> {
    budget.check()?;
    let granted: Vec<&ReadPathV1> = ranges
        .iter()
        .filter(|range| range.path == entry.path)
        .collect();
    if granted.is_empty() {
        return Err("ledger_skill_grant_excludes_document".into());
    }
    // Load once for the grant's line-to-byte mapping; `document_hits` reloads
    // and re-verifies the same source before emitting hits.
    let source = resolve::load_source(db, root, &entry.doc_id).map_err(|e| e.to_string())?;
    if source.imported {
        return Err("snapshot_range_unsupported".into());
    }
    let starts: Vec<usize> = std::iter::once(0)
        .chain(source.markdown.match_indices('\n').map(|(index, _)| index + 1))
        .collect();
    let length = source.markdown.len();
    let covered = granted.iter().any(|grant| {
        if grant.start_line == 0 || grant.end_line < grant.start_line {
            return false;
        }
        let start = starts
            .get(grant.start_line as usize - 1)
            .copied()
            .unwrap_or(usize::MAX);
        let end = starts.get(grant.end_line as usize).copied().unwrap_or(length);
        // Whole-source coverage: [0, len) contains every top-level span.
        start == 0 && end >= length
    });
    if !covered {
        return Err("ledger_skill_grant_excludes_document".into());
    }
    document_hits(db, root, entry, budget)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ledger::doc_spine;
    use std::fs;
    use std::time::Duration;

    fn budget() -> WorkBudget {
        WorkBudget::bounded(Duration::from_secs(10))
    }

    fn root_text(root: &tempfile::TempDir) -> String {
        root.path()
            .canonicalize()
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/")
    }

    fn fixture() -> (tempfile::TempDir, LedgerDb, String) {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("tools/skills/deploy")).unwrap();
        fs::create_dir_all(root.path().join("tools/skills/deploy-stage")).unwrap();
        fs::create_dir_all(root.path().join("docs")).unwrap();
        fs::write(
            root.path().join("tools/skills/deploy/SKILL.md"),
            "---\nname: deploy\ndescription: Ship a release\n---\n\n# Deploy\n\nskill needle gamma\n",
        )
        .unwrap();
        fs::write(
            root.path().join("tools/skills/deploy-stage/SKILL.md"),
            "# Deploy Stage\n\nstaging needle\n",
        )
        .unwrap();
        // Auxiliary files inside a skill directory and ordinary documents are
        // never skill documents.
        fs::write(
            root.path().join("tools/skills/deploy/notes.md"),
            "# Notes\n\nskill needle gamma auxiliary\n",
        )
        .unwrap();
        fs::write(
            root.path().join("docs/guide.md"),
            "# Guide\n\nskill needle gamma ordinary\n",
        )
        .unwrap();
        let db = LedgerDb::open_in_memory();
        doc_spine::sync(&db, root.path()).unwrap();
        let text = root_text(&root);
        (root, db, text)
    }

    #[test]
    fn skill_path_convention_matches_portability_store() {
        assert_eq!(
            skill_id_from_path("tools/skills/deploy/SKILL.md").as_deref(),
            Some("deploy")
        );
        for path in [
            "tools/skills/deploy/notes.md",
            "tools/skills/deploy/extra/SKILL.md",
            "tools/skills/SKILL.md",
            "tools/skills//SKILL.md",
            "tools/skills/../SKILL.md",
            "docs/tools/skills/deploy/SKILL.md",
            "skills/deploy/SKILL.md",
            "tools/skills/deploy/skill.md",
            "tools/skills/deploy",
            "",
        ] {
            assert_eq!(skill_id_from_path(path), None, "{path}");
        }
    }

    #[test]
    fn catalog_enumerates_only_registered_skill_documents() {
        let (_root, db, text) = fixture();
        let entries = catalog(&db, &text, None, &budget()).unwrap();
        let ids: Vec<&str> = entries.iter().map(|e| e.skill_id.as_str()).collect();
        assert_eq!(ids, ["deploy", "deploy-stage"]);
        let deploy = &entries[0];
        assert_eq!(deploy.path, "tools/skills/deploy/SKILL.md");
        assert_eq!(deploy.content_hash.len(), 64);
        assert!(!deploy.revision.is_empty());
        assert!(deploy.ledger_generation > 0);
    }

    #[test]
    fn search_returns_source_bound_skill_hits_only() {
        let (_root, db, text) = fixture();
        let outcome = search(&db, &text, "skill needle gamma", 8, None, &budget()).unwrap();
        assert!(!outcome.skills.is_empty());
        assert!(outcome
            .skills
            .iter()
            .all(|s| s.hit.source_ref.ends_with("/SKILL.md")));
        let deploy = outcome
            .skills
            .iter()
            .find(|s| s.skill_id == "deploy")
            .expect("deploy skill hit");
        assert_eq!(deploy.hit.expected_span_hash.len(), 64);
        assert_eq!(deploy.hit.expected_content_hash.len(), 64);
        assert!(!deploy.hit.expected_revision.is_empty());
        assert!(deploy.hit.ledger_generation > 0);
        let request = deploy.hit.resolve_request();
        assert_eq!(request.expected_span_hash.as_deref().unwrap().len(), 64);
        // Ordinary documents and auxiliary files never enter the skill lane.
        assert!(outcome
            .skills
            .iter()
            .all(|s| matches!(s.skill_id.as_str(), "deploy" | "deploy-stage")));
    }

    #[test]
    fn grant_ranges_narrow_but_never_widen_the_skill_lane() {
        let (_root, db, text) = fixture();
        // A grant over an ordinary document empties the skill lane entirely.
        let narrowed = search(
            &db,
            &text,
            "skill needle gamma",
            8,
            Some(vec![ReadPathV1 {
                path: "docs/guide.md".into(),
                start_line: 1,
                end_line: u32::MAX,
            }]),
            &budget(),
        )
        .unwrap();
        assert!(narrowed.skills.is_empty());
        assert!(narrowed.result.hits.is_empty());
        // A grant over one skill keeps only that skill.
        let granted = search(
            &db,
            &text,
            "skill needle gamma",
            8,
            Some(vec![ReadPathV1 {
                path: "tools/skills/deploy/SKILL.md".into(),
                start_line: 1,
                end_line: u32::MAX,
            }]),
            &budget(),
        )
        .unwrap();
        assert!(granted
            .skills
            .iter()
            .all(|s| s.skill_id == "deploy"));
        assert!(!granted.skills.is_empty());
    }

    #[test]
    fn erasure_fence_removes_skill_from_catalog_and_search() {
        let (_root, db, text) = fixture();
        db.lock()
            .execute(
                "INSERT OR REPLACE INTO ledger_erasure_fences VALUES (?1,?2,?3)",
                rusqlite::params![
                    text,
                    resolve::digest("tools/skills/deploy/SKILL.md".as_bytes()),
                    crate::time::now_millis() as i64
                ],
            )
            .unwrap();
        let entries = catalog(&db, &text, None, &budget()).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].skill_id, "deploy-stage");
        let outcome = search(&db, &text, "skill needle gamma", 8, None, &budget()).unwrap();
        assert!(outcome.skills.iter().all(|s| s.skill_id != "deploy"));
    }

    #[test]
    fn catalog_narrows_to_granted_paths() {
        let (_root, db, text) = fixture();
        let grant = [ReadPathV1 {
            path: "tools/skills/deploy/SKILL.md".into(),
            start_line: 1,
            end_line: u32::MAX,
        }];
        let entries = catalog(&db, &text, Some(&grant), &budget()).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].skill_id, "deploy");
        // A grant that names no skill path empties the catalog.
        let other = [ReadPathV1 {
            path: "docs/guide.md".into(),
            start_line: 1,
            end_line: u32::MAX,
        }];
        assert!(catalog(&db, &text, Some(&other), &budget()).unwrap().is_empty());
    }

    #[test]
    fn granted_document_hits_require_whole_document_coverage() {
        let (_root, db, text) = fixture();
        let entries = catalog(&db, &text, None, &budget()).unwrap();
        let deploy = entries.iter().find(|e| e.skill_id == "deploy").unwrap();
        // Whole-document grant covers every top-level span.
        let whole = [ReadPathV1 {
            path: "tools/skills/deploy/SKILL.md".into(),
            start_line: 1,
            end_line: u32::MAX,
        }];
        assert!(document_hits_granted(&db, &text, deploy, &whole, &budget()).is_ok());
        // An exact grant over the last content line also covers the file.
        let exact = [ReadPathV1 {
            path: "tools/skills/deploy/SKILL.md".into(),
            start_line: 1,
            end_line: 8,
        }];
        assert!(document_hits_granted(&db, &text, deploy, &exact, &budget()).is_ok());
        // A grant on a different path or a partial range is refused.
        let wrong_path = [ReadPathV1 {
            path: "tools/skills/deploy-stage/SKILL.md".into(),
            start_line: 1,
            end_line: u32::MAX,
        }];
        assert!(document_hits_granted(&db, &text, deploy, &wrong_path, &budget()).is_err());
        let partial = [ReadPathV1 {
            path: "tools/skills/deploy/SKILL.md".into(),
            start_line: 2,
            end_line: 3,
        }];
        assert!(document_hits_granted(&db, &text, deploy, &partial, &budget()).is_err());
        let short = [ReadPathV1 {
            path: "tools/skills/deploy/SKILL.md".into(),
            start_line: 1,
            end_line: 7,
        }];
        assert!(document_hits_granted(&db, &text, deploy, &short, &budget()).is_err());
        // An empty grant set refuses.
        assert!(document_hits_granted(&db, &text, deploy, &[], &budget()).is_err());
    }

    #[test]
    fn document_hits_are_source_and_span_verified() {
        let (_root, db, text) = fixture();
        let entries = catalog(&db, &text, None, &budget()).unwrap();
        let deploy = entries
            .iter()
            .find(|e| e.skill_id == "deploy")
            .unwrap();
        let hits = document_hits(&db, &text, deploy, &budget()).unwrap();
        let length = fs::read(_root.path().join("tools/skills/deploy/SKILL.md"))
            .unwrap()
            .len();
        // Frontmatter + one top-level section partition the file exactly.
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].hit.start_byte, 0);
        assert_eq!(hits[0].hit.end_byte, hits[1].hit.start_byte);
        assert_eq!(hits[1].hit.end_byte, length);
        assert!(hits.iter().all(|s| s.skill_id == "deploy"));
        assert!(hits.iter().all(|s| s.hit.lane == "ledger_skill"));
        assert!(hits.iter().all(|s| s.hit.node_kind != "document"));
        // A skill without frontmatter binds its single top-level section.
        let staged = entries
            .iter()
            .find(|e| e.skill_id == "deploy-stage")
            .unwrap();
        let hits = document_hits(&db, &text, staged, &budget()).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].hit.node_kind, "section");
        assert_eq!(hits[0].hit.start_byte, 0);
        // Source drift fails before any ticket could be issued.
        fs::write(
            _root.path().join("tools/skills/deploy/SKILL.md"),
            "# Deploy\n\nchanged bytes\n",
        )
        .unwrap();
        assert!(document_hits(&db, &text, deploy, &budget()).is_err());
    }
}
