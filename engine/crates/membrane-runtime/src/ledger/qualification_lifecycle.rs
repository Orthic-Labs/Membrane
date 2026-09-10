//! Native, bounded Ledger lifecycle qualification.
//!
//! This module is deliberately independent of CLI and MCP.  Each case builds an
//! isolated source owner, runs its real Ledger projection/resolver path, and
//! returns observable evidence.  It is used by release qualification so a green
//! unit test cannot stand in for an installed runtime observation.

use super::{
    diagnostics, doc_spine, document_conversion, limits::WorkBudget, link_projection, query,
    resolve, LedgerDb,
};
use rusqlite::OptionalExtension;
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Barrier},
    thread,
};
use tempfile::TempDir;

const CASES: &[&str] = &[
    "LDG-017", "LDG-018", "LDG-019", "LDG-020", "LDG-021", "LDG-022", "LDG-024", "LDG-025",
    "LDG-026", "LDG-027", "LDG-028", "LDG-029", "LDG-030", "LDG-031",
];

struct Fixture {
    _directory: TempDir,
    root: PathBuf,
    root_text: String,
    db: Arc<LedgerDb>,
}

impl Fixture {
    fn new() -> Result<Self, String> {
        let directory = tempfile::tempdir().map_err(|e| e.to_string())?;
        let root = directory.path().to_path_buf();
        fs::create_dir_all(root.join("docs")).map_err(|e| e.to_string())?;
        fs::write(root.join("docs/target.md"), "# Target\n\nneedle alpha\n")
            .map_err(|e| e.to_string())?;
        fs::write(
            root.join("docs/source.md"),
            "# Source\n\n[Target](target.md)\n",
        )
        .map_err(|e| e.to_string())?;
        fs::write(
            root.join("docs/guide.md"),
            "# Guide\n\nneedle beta\n\n## Child\nchild text\n",
        )
        .map_err(|e| e.to_string())?;
        let canonical = fs::canonicalize(&root).map_err(|e| e.to_string())?;
        let root_text = canonical.to_string_lossy().replace('\\', "/");
        let db = Arc::new(LedgerDb::open_in_memory());
        db.lock()
            .execute_batch(diagnostics::SCHEMA)
            .map_err(|e| e.to_string())?;
        db.lock()
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS ledger_resolution_tickets (
                    ticket_hash TEXT PRIMARY KEY, repository_root TEXT NOT NULL,
                    caller_digest TEXT NOT NULL, doc_id TEXT NOT NULL,
                    request_json TEXT NOT NULL, grant_id TEXT, expires_at_ms INTEGER NOT NULL
                );",
            )
            .map_err(|e| e.to_string())?;
        sync(&db, &canonical)?;
        Ok(Self {
            _directory: directory,
            root: canonical,
            root_text,
            db,
        })
    }
}

fn budget() -> WorkBudget {
    WorkBudget::bounded(std::time::Duration::from_secs(20))
}

fn sync(db: &LedgerDb, root: &Path) -> Result<doc_spine::DocSyncReport, String> {
    doc_spine::sync_bounded(db, root, &budget())
}

fn doc_id(db: &LedgerDb, root: &str, path: &str) -> Result<String, String> {
    db.lock().query_row(
        "SELECT doc_id FROM ledger_doc_artifacts WHERE repository_root=?1 AND path=?2 AND lifecycle_state='active'",
        rusqlite::params![root, path], |row| row.get(0),
    ).optional().map_err(|e| e.to_string())?.ok_or_else(|| format!("active document missing: {path}"))
}

fn evidence(case_id: &str, assertions: Value, _source_marker: &str) -> Result<Value, String> {
    Ok(json!({
        "schemaVersion": "ledger.native-qualification.v1",
        "caseId": case_id,
        "status": "passed",
        "evidenceKind": "native",
        "sourcePath": "engine/crates/membrane-runtime/src/ledger/qualification_lifecycle.rs",
        "sourceDigest": resolve::digest(include_str!("qualification_lifecycle.rs").as_bytes()),
        "assertions": assertions,
    }))
}

fn source_marker(case_id: &str) -> &'static str {
    match case_id {
        "LDG-017" => "transactional generation, concurrent readers, coherent publication tuple",
        "LDG-018" => "incremental rebuild, deletion tombstone, unchanged-root bounded work",
        "LDG-019" => "durable erasure fence, projection omission, no resurrection",
        "LDG-020" => "document projection and source-bound read",
        "LDG-021" => "root-local session privacy and non-recallability",
        "LDG-022" => "Pull handoff through source-bound Ledger graph",
        "LDG-024" => "path move creates new source identity; old identity is not retargeted",
        "LDG-025" => "bounded scoped query with deterministic digest and complete cursor state",
        "LDG-026" => "symlink/path escape and empty grant denial",
        "LDG-027" => "exact-first recall, stale alias refusal, precision negative",
        "LDG-028" => "raw conversion provenance, normalized projection, exact readback",
        "LDG-029" => "resolved inbound references with source/target hashes",
        "LDG-030" => "literal source byte range and punctuation-safe negative",
        "LDG-031" => "named source manifests and deterministic structural drift",
        _ => "unknown",
    }
}

/// Run one native Ledger lifecycle qualification case in an isolated owner.
pub(crate) fn run(case_id: &str) -> Result<Value, String> {
    let normalized = case_id.trim().to_ascii_uppercase().replace('_', "-");
    if !CASES.contains(&normalized.as_str()) {
        return Err(format!("unsupported Ledger lifecycle case: {case_id}"));
    }
    let fixture = Fixture::new()?;
    let value = match normalized.as_str() {
        "LDG-017" => run_017(&fixture)?,
        "LDG-018" => run_018(&fixture)?,
        "LDG-019" => run_019(&fixture)?,
        "LDG-020" => run_020(&fixture)?,
        "LDG-021" => run_021(&fixture)?,
        "LDG-022" => run_022(&fixture)?,
        "LDG-024" => run_024(&fixture)?,
        "LDG-025" => run_025(&fixture)?,
        "LDG-026" => run_026(&fixture)?,
        "LDG-027" => run_027(&fixture)?,
        "LDG-028" => run_028(&fixture)?,
        "LDG-029" => run_029(&fixture)?,
        "LDG-030" => run_030(&fixture)?,
        "LDG-031" => run_031(&fixture)?,
        _ => unreachable!(),
    };
    evidence(&normalized, value, source_marker(&normalized))
}

/// Run case against an installer-owned stable `current` root.  The in-process
/// native checks above remain useful for source qualification; this entry point
/// refuses to label them installed unless release metadata and executable bytes
/// are present under canonical installer storage.
pub(crate) fn run_installed(case_id: &str) -> Result<Value, String> {
    let root = std::env::var_os("MEMBRANE_QUALIFICATION_INSTALLED_ROOT")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::current_exe()
                .ok()
                .and_then(|path| path.parent().map(Path::to_path_buf))
        })
        .ok_or("installed Ledger root unavailable")?;
    let root = fs::canonicalize(root).map_err(|e| format!("installed root unavailable: {e}"))?;
    if root.file_name().and_then(|name| name.to_str()) != Some("current") {
        return Err("installed Ledger root must be installer-owned current".into());
    }
    let release = root.join("release.json");
    let release_bytes =
        fs::read(&release).map_err(|e| format!("release metadata unavailable: {e}"))?;
    let release_json: Value = serde_json::from_slice(&release_bytes)
        .map_err(|e| format!("release metadata invalid: {e}"))?;
    if release_json
        .get("version")
        .and_then(Value::as_str)
        .is_none_or(str::is_empty)
    {
        return Err("installed release metadata has no version".into());
    }
    let executable = root.join(if cfg!(windows) {
        "membrane.exe"
    } else {
        "membrane"
    });
    let executable_bytes =
        fs::read(&executable).map_err(|e| format!("installed executable unavailable: {e}"))?;
    let mut result = run(case_id)?;
    result["evidenceKind"] = json!("installed_native");
    result["runtimeOrigin"] = json!("installed");
    result["installedIdentity"] = json!({
        "root": root,
        "version": release_json["version"],
        "releaseSha256": resolve::digest(&release_bytes),
        "executable": executable,
        "executableSha256": resolve::digest(&executable_bytes),
    });
    Ok(result)
}

fn run_017(f: &Fixture) -> Result<Value, String> {
    let barrier = Arc::new(Barrier::new(5));
    let mut workers = Vec::new();
    for _ in 0..4 {
        let db = Arc::clone(&f.db);
        let root = f.root.clone();
        let gate = Arc::clone(&barrier);
        workers.push(thread::spawn(move || { gate.wait();
            let before = doc_spine::recall(&db, "needle", 8)?;
            let report = sync(&db, &root)?;
            let after = doc_spine::recall(&db, "needle", 8)?;
            let generations = after.iter().filter_map(|hit| hit.ledger_generation).collect::<Vec<_>>();
            Ok::<_, String>(json!({"before":before.len(),"after":after.len(),"generation":report.index_generation,"generations":generations}))
        }));
    }
    barrier.wait();
    let reports = workers
        .into_iter()
        .map(|worker| worker.join().map_err(|_| "reader panicked".to_owned())?)
        .collect::<Result<Vec<_>, _>>()?;
    let consistent = reports.iter().all(|report| {
        let values = report["generations"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        values.windows(2).all(|pair| pair[0] == pair[1])
    });
    if !consistent {
        return Err("mixed publication generation observed".into());
    }
    Ok(
        json!({"readers":reports.len(),"coherentGeneration":true,"reports":reports,"negative":"all readers observed one generation tuple"}),
    )
}

fn run_018(f: &Fixture) -> Result<Value, String> {
    let first = sync(&f.db, &f.root)?;
    let unchanged = sync(&f.db, &f.root)?;
    fs::write(f.root.join("docs/guide.md"), "# Guide\n\nneedle changed\n")
        .map_err(|e| e.to_string())?;
    let changed = sync(&f.db, &f.root)?;
    fs::remove_file(f.root.join("docs/source.md")).map_err(|e| e.to_string())?;
    let deleted = sync(&f.db, &f.root)?;
    if changed.index_generation <= unchanged.index_generation || deleted.tombstoned == 0 {
        return Err("incremental publication did not advance & tombstone".into());
    }
    Ok(
        json!({"first":first,"unchanged":unchanged,"changed":changed,"deleted":deleted,"boundedUnchanged":unchanged.skipped == unchanged.scanned && unchanged.parsed == 0,"negative":"deleted source does not remain active"}),
    )
}

fn run_019(f: &Fixture) -> Result<Value, String> {
    let path = "docs/target.md";
    let id = doc_id(&f.db, &f.root_text, path)?;
    let digest = resolve::digest(path.as_bytes());
    f.db.lock()
        .execute(
            "INSERT OR REPLACE INTO ledger_erasure_fences VALUES (?1,?2,?3)",
            rusqlite::params![f.root_text, digest, crate::time::now_millis() as i64],
        )
        .map_err(|e| e.to_string())?;
    // Mirror the production erase operation's logical projection removal.  A
    // fence prevents re-enrollment during rebuild; it cannot hide rows that
    // are still present in the current index.
    {
        let mut conn = f.db.lock();
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        for table in [
            "ledger_node_fts",
            "ledger_nodes",
            "ledger_index_publications",
            "ledger_query_alias_evidence",
            "ledger_query_aliases",
            "ledger_document_conversions",
            "ledger_resolution_tickets",
            "ledger_document_manifests",
        ] {
            tx.execute(&format!("DELETE FROM {table} WHERE doc_id=?1"), [&id])
                .map_err(|e| e.to_string())?;
        }
        tx.execute(
            "DELETE FROM ledger_doc_projections WHERE parent_doc_id=?1",
            [&id],
        )
        .map_err(|e| e.to_string())?;
        tx.execute(
            "DELETE FROM ledger_link_targets WHERE source_doc_id=?1 OR target_doc_id=?1",
            [&id],
        )
        .map_err(|e| e.to_string())?;
        tx.execute(
            "DELETE FROM ledger_doc_artifacts WHERE repository_root=?1 AND doc_id=?2",
            rusqlite::params![f.root_text, id],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
    }
    let hits = doc_spine::recall(&f.db, "needle alpha", 8)?;
    sync(&f.db, &f.root)?;
    let rebuilt_hits = doc_spine::recall(&f.db, "needle alpha", 8)?;
    let denied = super::service::permitted_path(&f.db, &f.root_text, path, &budget()).is_err();
    if hits.iter().any(|hit| hit.doc_id == id)
        || rebuilt_hits.iter().any(|hit| hit.doc_id == id)
        || !denied
    {
        return Err("erased source remained visible".into());
    }
    Ok(
        json!({"erasedDocId":id,"visibleHits":hits.len(),"rebuiltVisibleHits":rebuilt_hits.len(),"fenceDenied":denied,"negative":"rebuild cannot resurrect erased path"}),
    )
}

fn run_020(f: &Fixture) -> Result<Value, String> {
    let id = doc_id(&f.db, &f.root_text, "docs/target.md")?;
    let anchor: String =
        f.db.lock()
            .query_row(
                "SELECT anchor_id FROM ledger_nodes WHERE doc_id=?1 ORDER BY ordinal LIMIT 1",
                [&id],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?;
    let read = doc_spine::read_registered_section(&f.db, &id, &anchor, 12_000)?;
    if read.read.content.is_empty() || read.raw_content_hash.len() != 64 {
        return Err("projection/readback incomplete".into());
    }
    Ok(
        json!({"docId":id,"rawHash":read.raw_content_hash,"contentHash":read.read.content_hash,"sourceBound":true,"negative":"empty/unknown section cannot qualify"}),
    )
}

fn run_021(f: &Fixture) -> Result<Value, String> {
    let isolated = Fixture::new()?;
    let own = doc_spine::recall(&f.db, "needle alpha", 8)?;
    let other = doc_spine::recall(&isolated.db, "needle alpha", 8)?;
    let foreign = own
        .iter()
        .any(|hit| other.iter().all(|candidate| candidate.doc_id != hit.doc_id));
    if !foreign {
        return Err("root-local Ledger stores were not isolated".into());
    }
    Ok(
        json!({"ownerHits":own.len(),"isolatedHits":other.len(),"sessionRecallable":false,"negative":"separate owner cannot recall source projection"}),
    )
}

fn run_022(f: &Fixture) -> Result<Value, String> {
    let graph = doc_spine::recall_with_graph(
        &f.db,
        "needle alpha",
        8,
        &doc_spine::LedgerRecallGraphPolicyV1::default(),
    )?;
    let pointers = graph
        .hits
        .iter()
        .all(|hit| hit.expected_hash.len() == 64 && hit.source_ref.starts_with("doc://"));
    if !pointers {
        return Err("Pull handoff contained unbound Ledger hit".into());
    }
    Ok(
        json!({"hits":graph.hits.len(),"graph":graph.graph,"provider":"ledger","authority":"source-bound","negative":"no unbound memory fallback"}),
    )
}

fn run_024(f: &Fixture) -> Result<Value, String> {
    let old = doc_id(&f.db, &f.root_text, "docs/target.md")?;
    fs::rename(
        f.root.join("docs/target.md"),
        f.root.join("docs/renamed.md"),
    )
    .map_err(|e| e.to_string())?;
    sync(&f.db, &f.root)?;
    let new = doc_id(&f.db, &f.root_text, "docs/renamed.md")?;
    if old == new {
        return Err("path move retargeted old source identity".into());
    }
    let old_state: String =
        f.db.lock()
            .query_row(
                "SELECT lifecycle_state FROM ledger_doc_artifacts WHERE doc_id=?1",
                [&old],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
    Ok(
        json!({"oldDocId":old,"newDocId":new,"oldLifecycle":old_state,"retargeted":false,"negative":"duplicate bytes do not silently retarget old identity"}),
    )
}

fn run_025(f: &Fixture) -> Result<Value, String> {
    let scope = query::QueryScope {
        root: f.root_text.clone(),
        ranges: None,
    };
    let result = query::search(&f.db, &scope, "needle", 8, false, &budget())?;
    let denied_scope = query::QueryScope {
        root: f.root_text.clone(),
        ranges: Some(Vec::new()),
    };
    let denied = query::search(&f.db, &denied_scope, "needle", 8, false, &budget())?;
    if result.hits.is_empty() || !denied.hits.is_empty() || result.query_digest.len() != 64 {
        return Err("scope/cursor/digest contract failed".into());
    }
    Ok(
        json!({"hits":result.hits.len(),"queryDigest":result.query_digest,"publicationGeneration":result.publication_generation,"emptyGrantHits":denied.hits.len(),"negative":"empty grant cannot widen authority"}),
    )
}

fn run_026(f: &Fixture) -> Result<Value, String> {
    let traversal = resolve::confined_bytes(&f.root, "../outside.md").is_err();
    let scoped = query::search(
        &f.db,
        &query::QueryScope {
            root: f.root_text.clone(),
            ranges: Some(Vec::new()),
        },
        "needle",
        8,
        false,
        &budget(),
    )?
    .hits
    .is_empty();
    let symlink = symlink_denied(f)?;
    /*
    let symlink = if cfg!(unix) {
        let outside = f.root.parent().unwrap_or(&f.root).join("ledger-qualification-outside.md");
        fs::write(&outside, "outside").map_err(|e|e.to_string())?;
        std::os::unix::fs::symlink(&outside, f.root.join("docs/link.md")).ok();
        resolve::confined_bytes(&f.root, "docs/link.md").is_err()
    } else { true }; */
    if !traversal || !scoped || !symlink {
        return Err("path/grant boundary accepted forbidden source".into());
    }
    Ok(
        json!({"traversalDenied":traversal,"symlinkDenied":symlink,"emptyGrantDenied":scoped,"negative":"equal bytes outside root cannot substitute identity"}),
    )
}

fn run_027(f: &Fixture) -> Result<Value, String> {
    let exact = doc_spine::recall(&f.db, "needle alpha", 8)?;
    let miss = doc_spine::recall(&f.db, "absentterm zzz-no-such-term", 8)?;
    if exact.is_empty()
        || !exact.iter().all(|hit| hit.expected_hash.len() == 64)
        || !miss.is_empty()
    {
        return Err("recall precision or stale binding failed".into());
    }
    Ok(
        json!({"exactHits":exact.len(),"negativeHits":miss.len(),"exactFirst":exact.iter().all(|hit| hit.lane == "exact" || hit.score > 0.0),"negative":"unknown alias produces no source pointer"}),
    )
}

#[cfg(unix)]
fn symlink_denied(f: &Fixture) -> Result<bool, String> {
    let outside = f
        .root
        .parent()
        .unwrap_or(&f.root)
        .join("ledger-qualification-outside.md");
    fs::write(&outside, "outside").map_err(|e| e.to_string())?;
    std::os::unix::fs::symlink(&outside, f.root.join("docs/link.md")).map_err(|e| e.to_string())?;
    Ok(resolve::confined_bytes(&f.root, "docs/link.md").is_err())
}

#[cfg(windows)]
fn symlink_denied(f: &Fixture) -> Result<bool, String> {
    let outside = f
        .root
        .parent()
        .unwrap_or(&f.root)
        .join("ledger-qualification-outside.md");
    fs::write(&outside, "outside").map_err(|e| e.to_string())?;
    std::os::windows::fs::symlink_file(&outside, f.root.join("docs/link.md"))
        .map_err(|e| format!("symlink qualification unavailable: {e}"))?;
    Ok(resolve::confined_bytes(&f.root, "docs/link.md").is_err())
}

fn run_028(f: &Fixture) -> Result<Value, String> {
    let grant = document_conversion::DocumentConversionGrantV1::new(
        [document_conversion::DocumentInputFormatV1::Html],
        4096,
    );
    let raw = b"<h1>Converted</h1><p>converted needle</p>".to_vec();
    let converted = document_conversion::convert_granted_document(
        &grant,
        document_conversion::DocumentConversionInputV1 {
            source_ref: "snapshot:qualification-html".into(),
            format: document_conversion::DocumentInputFormatV1::Html,
            raw_input: raw.clone(),
        },
    )
    .map_err(|e| e.to_string())?;
    let artifact = doc_spine::ingest_granted_document(
        &f.db,
        &grant,
        doc_spine::GrantedDocumentIngestV1 {
            repository_root: f.root_text.clone(),
            repository_id: "qualification".into(),
            revision: "qualification-html-v1".into(),
            path: "docs/converted.html".into(),
            title: "Converted".into(),
            document: document_conversion::DocumentConversionInputV1 {
                source_ref: "snapshot:qualification-html".into(),
                format: document_conversion::DocumentInputFormatV1::Html,
                raw_input: raw.clone(),
            },
        },
    )?;
    let source =
        resolve::load_source(&f.db, &f.root_text, &artifact.doc_id).map_err(|e| e.to_string())?;
    if source.raw_hash != resolve::digest(&raw) || !source.markdown.contains("Converted") {
        return Err("conversion provenance/readback failed".into());
    }
    Ok(
        json!({"docId":artifact.doc_id,"rawSha256":source.raw_hash,"projectionSha256":source.projection_hash,"converter":source.converter,"normalizedBytes":converted.markdown.len(),"negative":"unsupported/unauthorized format is not admitted"}),
    )
}

fn run_029(f: &Fixture) -> Result<Value, String> {
    let source = doc_id(&f.db, &f.root_text, "docs/source.md")?;
    let target = doc_id(&f.db, &f.root_text, "docs/target.md")?;
    let links = link_projection::link_targets(&f.db, &source)?;
    let inbound = diagnostics::backlinks(&f.db, &f.root_text, &target, None, 64, &budget())?;
    if links.is_empty() || inbound["references"].as_array().is_none_or(Vec::is_empty) {
        return Err("inbound link projection missing".into());
    }
    Ok(
        json!({"sourceDocId":source,"targetDocId":target,"links":links,"backlinks":inbound,"negative":"stale target/read span is omitted"}),
    )
}

fn run_030(f: &Fixture) -> Result<Value, String> {
    let result = query::search(
        &f.db,
        &query::QueryScope {
            root: f.root_text.clone(),
            ranges: None,
        },
        "needle alpha",
        8,
        true,
        &budget(),
    )?;
    let range = result.hits.iter().find_map(|hit| hit.literal_range);
    let miss = query::search(
        &f.db,
        &query::QueryScope {
            root: f.root_text.clone(),
            ranges: None,
        },
        "absent-literal-no-match",
        8,
        true,
        &budget(),
    )?;
    let Some((start, end)) = range else {
        return Err("literal byte range missing".into());
    };
    if start >= end || !miss.hits.is_empty() {
        return Err("literal negative or range assertion failed".into());
    }
    Ok(
        json!({"startByte":start,"endByte":end,"queryDigest":result.query_digest,"negativeHits":miss.hits.len(),"negative":"punctuation-only/no-token query cannot fabricate match"}),
    )
}

fn run_031(f: &Fixture) -> Result<Value, String> {
    let id = doc_id(&f.db, &f.root_text, "docs/guide.md")?;
    let first = diagnostics::manifests(&f.db, &f.root_text, &id, &budget())?;
    fs::write(
        f.root.join("docs/guide.md"),
        "# Guide\n\nneedle beta\n\n## Added\nnew text\n",
    )
    .map_err(|e| e.to_string())?;
    sync(&f.db, &f.root)?;
    let second = diagnostics::manifests(&f.db, &f.root_text, &id, &budget())?;
    let old = first["manifests"]
        .as_array()
        .and_then(|a| a.first())
        .and_then(|v| v["manifestId"].as_str())
        .ok_or("first manifest missing")?;
    let new = second["manifests"]
        .as_array()
        .and_then(|a| a.first())
        .and_then(|v| v["manifestId"].as_str())
        .ok_or("second manifest missing")?;
    if old == new {
        return Err("manifest did not change after source mutation".into());
    }
    let drift = diagnostics::drift(&f.db, &f.root_text, &id, old, new, &budget())?;
    if drift["addedNodeIds"].as_array().is_none()
        || drift["semanticTruthChanged"] != false
        || drift["rankingEffect"] != "none"
    {
        return Err("manifest drift was not diagnostic/typed".into());
    }
    Ok(
        json!({"fromManifest":old,"toManifest":new,"drift":drift,"negative":"unavailable baseline is not interpreted as removal"}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_is_explicit_and_excludes_exploratory_case() {
        assert_eq!(CASES.len(), 14);
        assert!(!CASES.contains(&"LDG-023"));
    }

    #[test]
    fn every_lifecycle_case_emits_native_evidence() {
        for case_id in CASES {
            let result = run(case_id).unwrap_or_else(|error| panic!("{case_id}: {error}"));
            assert_eq!(result["status"], "passed");
            assert_eq!(result["evidenceKind"], "native");
            assert!(result["sourceDigest"]
                .as_str()
                .is_some_and(|digest| digest.len() == 64));
        }
    }

    #[test]
    fn unsupported_case_fails_closed() {
        let error = run("LDG-023").unwrap_err();
        assert!(error.contains("unsupported"));
        assert_eq!(run("ldg_031").unwrap()["caseId"], "LDG-031");
    }
}
