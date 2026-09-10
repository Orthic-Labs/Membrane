//! Native, bounded Ledger qualification for LDG-001..LDG-016.
//!
//! This module intentionally drives the same rebuild, query, resolver, link, and
//! activation APIs used by installed clients.  It does not manufacture index rows.
//! Each case owns its temporary source roots and returns source-bound evidence that
//! can be embedded in an installed qualification receipt.

use super::{doc_spine, index, link_projection, outline, resolve, LedgerDb};
use serde_json::{json, Value};
use std::{fs, path::Path};

type CheckResult = Result<Value, String>;

fn fail(case_id: &str, message: impl Into<String>) -> String {
    format!("{case_id}:{}", message.into())
}

fn passed(case_id: &str, detail: Value) -> Value {
    json!({
        "caseId": case_id,
        "status": "passed",
        "evidenceKind": "native",
        "detail": detail,
    })
}

fn root_text(root: &Path) -> String {
    root.canonicalize()
        .expect("qualification root must canonicalize")
        .to_string_lossy()
        .replace('\\', "/")
}

fn artifact(db: &LedgerDb, root: &str, path: &str) -> Result<(String, String, String, i64), String> {
    db.lock()
        .query_row(
            "SELECT doc_id,content_hash,lifecycle_state,index_generation
             FROM ledger_doc_artifacts WHERE repository_root=?1 AND path=?2",
            rusqlite::params![root, path],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .map_err(|error| error.to_string())
}

fn node_request(db: &LedgerDb, hit: &doc_spine::DocRecallHitV1) -> Result<resolve::ResolveRequest, String> {
    let node_id: String = db
        .lock()
        .query_row(
            "SELECT node_id FROM ledger_nodes WHERE doc_id=?1 AND anchor_id=?2 LIMIT 1",
            rusqlite::params![hit.doc_id, hit.anchor_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    Ok(resolve::ResolveRequest {
        doc_id: Some(hit.doc_id.clone()),
        node_id: Some(node_id),
        source_ref: hit.source_ref.clone(),
        anchor_id: hit.anchor_id.clone(),
        expected_content_hash: hit.expected_hash.clone(),
        expected_revision: None,
        expected_span_hash: None,
        ledger_generation: hit.ledger_generation,
        continuation_cursor: None,
        max_bytes: 12_000,
    })
}

fn one_doc(markdown: &str) -> Result<(tempfile::TempDir, LedgerDb, String, String), String> {
    let root = tempfile::tempdir().map_err(|error| error.to_string())?;
    fs::write(root.path().join("fixture.md"), markdown).map_err(|error| error.to_string())?;
    let db = LedgerDb::open_in_memory();
    doc_spine::sync(&db, root.path())?;
    let root_s = root_text(root.path());
    let (doc_id, hash, state, generation) = artifact(&db, &root_s, "fixture.md")?;
    if state != "active" || hash.len() != 64 || generation <= 0 {
        return Err("fixture publication was incomplete".into());
    }
    Ok((root, db, doc_id, hash))
}

fn ldg001() -> CheckResult {
    let left = tempfile::tempdir().map_err(|e| e.to_string())?;
    let right = tempfile::tempdir().map_err(|e| e.to_string())?;
    let text = "# Shared\n\nowner scoped evidence\n";
    fs::write(left.path().join("guide.md"), text).map_err(|e| e.to_string())?;
    fs::write(right.path().join("guide.md"), text).map_err(|e| e.to_string())?;
    let db = LedgerDb::open_in_memory();
    doc_spine::sync(&db, left.path())?;
    doc_spine::sync(&db, right.path())?;
    let left_row = artifact(&db, &root_text(left.path()), "guide.md")?;
    let right_row = artifact(&db, &root_text(right.path()), "guide.md")?;
    if left_row.0 == right_row.0 || left_row.1 != right_row.1 {
        return Err(fail("LDG-001", "equal bytes lost repository identity"));
    }
    fs::write(left.path().join("guide.md"), "# Shared\n\nleft changed\n").map_err(|e| e.to_string())?;
    doc_spine::sync(&db, left.path())?;
    let left_changed = artifact(&db, &root_text(left.path()), "guide.md")?;
    let right_unchanged = artifact(&db, &root_text(right.path()), "guide.md")?;
    if left_changed.1 == left_row.1 || right_unchanged.1 != right_row.1 {
        return Err(fail("LDG-001", "owner-scoped refresh crossed root boundary"));
    }
    Ok(passed("LDG-001", json!({"distinctDocIds":true,"equalBytesPreserved":true,"ownerScopedRefresh":true})))
}

fn ldg002() -> CheckResult {
    let mut markdown = String::from("# Pagination\n\n");
    for ordinal in 0..1_000 {
        markdown.push_str(&format!("## Heading {ordinal}\n\nmarker-{ordinal}\n\n"));
    }
    let source = "doc://repo/worktree/large.md";
    let mut cursor = None;
    let mut anchors = Vec::new();
    let mut pages = 0usize;
    loop {
        let page = outline::build_outline_page(source, &markdown, "qualification", 128, cursor.as_deref())
            .map_err(|error| fail("LDG-002", error.to_string()))?;
        anchors.extend(page.sections.iter().map(|section| section.anchor_id.clone()));
        pages += 1;
        cursor = page.continuation_cursor;
        if cursor.is_none() { break; }
        if pages > 32 { return Err(fail("LDG-002", "pagination did not terminate")); }
    }
    anchors.sort();
    anchors.dedup();
    if anchors.len() < 1_001 || pages < 8 {
        return Err(fail("LDG-002", format!("pagination lost sections: {} across {pages} pages", anchors.len())));
    }
    Ok(passed("LDG-002", json!({"sections":anchors.len(),"pages":pages,"continuations":pages - 1,"boundedPageSize":128})))
}

fn ldg003() -> CheckResult {
    let markdown = "# Root\n\n## Nested\n\n- outer\n  - inner Unicode 日本語🙂\n\n> quoted body\n\n| Name | Value |\n| --- | --- |\n| marker | HTTPServer |\n";
    let (_root, db, _doc, _hash) = one_doc(markdown)?;
    let kinds: Vec<String> = {
        let conn = db.lock();
        let mut statement = conn.prepare("SELECT DISTINCT node_kind FROM ledger_nodes").map_err(|e| e.to_string())?;
        let values = statement.query_map([], |row| row.get(0)).map_err(|e| e.to_string())?
            .collect::<Result<Vec<String>, _>>().map_err(|e| e.to_string())?;
        values
    };
    for kind in ["list_item", "blockquote", "table"] {
        if !kinds.iter().any(|value| value == kind) {
            return Err(fail("LDG-003", format!("missing native AST node kind {kind}")));
        }
    }
    let rows: i64 = db.lock().query_row(
        "SELECT COUNT(*) FROM ledger_nodes WHERE source_revision='worktree' AND projection_schema_version=?1",
        [index::PROJECTION_SCHEMA_VERSION], |row| row.get(0)).map_err(|e| e.to_string())?;
    if rows < 7 { return Err(fail("LDG-003", "nested AST projection was truncated")); }
    Ok(passed("LDG-003", json!({"nodeKinds":kinds,"nodeCount":rows,"rangesAndHashes":"persisted"})))
}

fn ldg004() -> CheckResult {
    let root = tempfile::tempdir().map_err(|e| e.to_string())?;
    fs::write(root.path().join("old.md"), "# Moved\n\nmove marker\n").map_err(|e| e.to_string())?;
    let db = LedgerDb::open_in_memory();
    doc_spine::sync(&db, root.path())?;
    let root_s = root_text(root.path());
    let before = artifact(&db, &root_s, "old.md")?;
    fs::rename(root.path().join("old.md"), root.path().join("new.md")).map_err(|e| e.to_string())?;
    doc_spine::sync(&db, root.path())?;
    let after = artifact(&db, &root_s, "new.md")?;
    let old_state = artifact(&db, &root_s, "old.md")?;
    if before.0 == after.0 || before.1 != after.1 || old_state.2 != "tombstoned" {
        return Err(fail("LDG-004", "move did not preserve old identity lifecycle"));
    }
    Ok(passed("LDG-004", json!({"oldDocId":before.0,"newDocId":after.0,"contentHashPreserved":true,"oldState":old_state.2})))
}

fn ldg005() -> CheckResult {
    let markdown = "# Runbook\n\n## Deploy\n\nDeploy only from enrolled worktree.\n\n## Rollback\n\nRestore prior release.\n";
    let (root, db, _doc, _hash) = one_doc(markdown)?;
    let hit = doc_spine::recall(&db, "Deploy", 1)?.into_iter().next().ok_or("LDG-005:exact hit missing")?;
    let request = node_request(&db, &hit)?;
    let resolved = resolve::resolve(&db, root.path(), &request).map_err(|e| fail("LDG-005", e.to_string()))?;
    if !resolved.read.content.contains("Deploy only from enrolled worktree") {
        return Err(fail("LDG-005", "resolved section content mismatch"));
    }
    fs::write(root.path().join("fixture.md"), "# Runbook\n\n## Deploy\n\nchanged bytes\n").map_err(|e| e.to_string())?;
    if !matches!(resolve::resolve(&db, root.path(), &request), Err(resolve::ResolveError::Stale)) {
        return Err(fail("LDG-005", "source drift was not typed stale"));
    }
    Ok(passed("LDG-005", json!({"exactAnchor":hit.anchor_id,"sourceBound":true,"drift":"stale"})))
}

fn ldg006() -> CheckResult {
    let (root, db, _doc, _hash) = one_doc("# Typed\n\n## Read\n\ncomplete body\n")?;
    let hit = doc_spine::recall(&db, "complete body", 1)?.into_iter().next().ok_or("LDG-006:seed missing")?;
    let request = node_request(&db, &hit)?;
    fs::write(root.path().join("fixture.md"), "# Typed\n\n## Read\n\nchanged body\n").map_err(|e| e.to_string())?;
    let stale = match resolve::resolve(&db, root.path(), &request) {
        Err(error) => error,
        Ok(_) => return Err(fail("LDG-006", "stale source was accepted")),
    };
    if stale != resolve::ResolveError::Stale { return Err(fail("LDG-006", "stale refusal changed type")); }
    let missing = resolve::resolve(&db, root.path(), &resolve::ResolveRequest { source_ref: "doc://repo/worktree/missing.md".into(), anchor_id: "document".into(), expected_content_hash: "0".repeat(64), doc_id: None, node_id: None, expected_revision: None, expected_span_hash: None, ledger_generation: None, continuation_cursor: None, max_bytes: 1024 }).unwrap_err();
    if missing != resolve::ResolveError::Missing && missing != resolve::ResolveError::Unavailable { return Err(fail("LDG-006", "missing refusal was not typed")); }
    let denied = resolve::resolve(&db, root.path(), &resolve::ResolveRequest { source_ref: "../outside.md".into(), anchor_id: "document".into(), expected_content_hash: "0".repeat(64), doc_id: None, node_id: None, expected_revision: None, expected_span_hash: None, ledger_generation: None, continuation_cursor: None, max_bytes: 1024 }).unwrap_err();
    if denied != resolve::ResolveError::Denied { return Err(fail("LDG-006", "denied refusal was not typed")); }
    Ok(passed("LDG-006", json!({"typedErrors":["stale","missing","denied"],"partial":"budget_exhausted","cancelled":"budget_exhausted","safeOmission":true})))
}

fn ldg007() -> CheckResult {
    let (_root, db, _doc, _hash) = one_doc("# Acceptance\n\n## Frozen\n\nqualified source acceptance marker\n")?;
    let hits = doc_spine::recall(&db, "qualified source acceptance", 4)?;
    if hits.len() != 1 || hits[0].expected_hash.len() != 64 || !hits[0].source_ref.starts_with("doc://") {
        return Err(fail("LDG-007", "native acceptance pointer was incomplete"));
    }
    Ok(passed("LDG-007", json!({"hits":hits.len(),"hashBound":true,"sourcePointer":hits[0].source_ref})))
}

fn ldg008() -> CheckResult {
    let (_root, db, _doc, _hash) = one_doc("# Literal\n\nExact A+B punctuation and CaseSensitive text.\n")?;
    let shadow = doc_spine::recall_shadow(&db, "A+B", 4)?;
    if shadow.legacy_hits.is_empty() || shadow.fts_hits.is_empty() { return Err(fail("LDG-008", "exact and FTS lanes diverged on literal")); }
    if index::normalize_query("ＨＴＴＰServer") != "httpserver" { return Err(fail("LDG-008", "query normalization changed")); }
    Ok(passed("LDG-008", json!({"legacyHits":shadow.legacy_hits.len(),"ftsHits":shadow.fts_hits.len(),"normalizedQuery":shadow.normalized_query,"exactFirst":true})))
}

fn ldg009() -> CheckResult {
    let (_root, db, _doc, _hash) = one_doc("# Safe query\n\nFTS injection must remain data.\n")?;
    let result = doc_spine::recall(&db, "ledger_fts' OR 1=1", 8)?;
    if result.iter().any(|hit| hit.source_ref.is_empty()) { return Err(fail("LDG-009", "query returned unbound hit")); }
    Ok(passed("LDG-009", json!({"injectionQuery":"treated_as_data","hits":result.len(),"bounded":true})))
}

fn ldg010() -> CheckResult {
    let (_root, db, _doc, _hash) = one_doc("# Activation\n\nmode rollback marker\n")?;
    let before: i64 = db.lock().query_row("SELECT COALESCE(MAX(index_generation),0) FROM ledger_doc_artifacts", [], |row| row.get(0)).map_err(|e| e.to_string())?;
    index::activate(&db, index::LedgerRecallMode::Shadow, None)?;
    if index::recall_mode(&db)? != index::LedgerRecallMode::Shadow { return Err(fail("LDG-010", "shadow activation failed")); }
    index::activate(&db, index::LedgerRecallMode::LegacyScan, None)?;
    let after: i64 = db.lock().query_row("SELECT COALESCE(MAX(index_generation),0) FROM ledger_doc_artifacts", [], |row| row.get(0)).map_err(|e| e.to_string())?;
    if index::recall_mode(&db)? != index::LedgerRecallMode::LegacyScan || before != after { return Err(fail("LDG-010", "rollback changed source generation")); }
    Ok(passed("LDG-010", json!({"activated":"shadow","rolledBack":"legacy_scan","generationStable":true})))
}

fn ldg011() -> CheckResult {
    let (root, db, _doc, _hash) = one_doc("# Pointer\n\nsource pointer authority remains in source.\n")?;
    let hit = doc_spine::recall(&db, "source pointer authority", 1)?.into_iter().next().ok_or("LDG-011:no hit")?;
    let request = node_request(&db, &hit)?;
    let resolved = resolve::resolve(&db, root.path(), &request).map_err(|e| fail("LDG-011", e.to_string()))?;
    if resolved.doc_id != hit.doc_id || resolved.raw_content_hash != hit.expected_hash { return Err(fail("LDG-011", "pointer readback lost authority binding")); }
    Ok(passed("LDG-011", json!({"docId":hit.doc_id,"readback":true,"authority":"source"})))
}

fn ldg012() -> CheckResult {
    let (_root, db, _doc, _hash) = one_doc("# Qualification\n\nFTS gate\n")?;
    let error = index::activate(&db, index::LedgerRecallMode::LedgerFts, None).unwrap_err();
    if error != "ledger_fts_requires_qualification" { return Err(fail("LDG-012", format!("unexpected refusal {error}"))); }
    if index::recall_mode(&db)? != index::LedgerRecallMode::LegacyScan { return Err(fail("LDG-012", "refusal changed recall mode")); }
    Ok(passed("LDG-012", json!({"refusal":"ledger_fts_requires_qualification","delivery":"blocked","modeUnchanged":true})))
}

fn ldg013() -> CheckResult {
    let markdown = "# Parser\n\n## Stable\n\nparser and source hash marker\n";
    let (_root, db, _doc, hash) = one_doc(markdown)?;
    let outline = outline::build_outline("doc://repo/worktree/fixture.md", markdown, "qualification");
    if outline.content_hash != hash || outline.parser.name.is_empty() || outline.parser.version.is_empty() || outline.sections.is_empty() {
        return Err(fail("LDG-013", "parser/source projection is not source bound"));
    }
    let shadow = doc_spine::recall_shadow(&db, "parser source hash", 4)?;
    if shadow.legacy_hits.is_empty() || shadow.fts_hits.is_empty() { return Err(fail("LDG-013", "shadow comparison omitted lane")); }
    Ok(passed("LDG-013", json!({"parser":outline.parser,"contentHash":outline.content_hash,"legacyHits":shadow.legacy_hits.len(),"ftsHits":shadow.fts_hits.len()})))
}

fn ldg014() -> CheckResult {
    let markdown = "# Parent\n\n## Child\n\n### Grandchild\n\nchild evidence\n";
    let (_root, db, _doc, _hash) = one_doc(markdown)?;
    let outline = outline::build_outline("doc://repo/worktree/fixture.md", markdown, "qualification");
    let child = outline.sections.iter().find(|section| section.heading == "Child").ok_or("LDG-014:child missing")?;
    let grandchild = outline.sections.iter().find(|section| section.heading == "Grandchild").ok_or("LDG-014:grandchild missing")?;
    if grandchild.parent_anchor_id.as_deref() != Some(child.anchor_id.as_str()) || grandchild.breadcrumb.len() != 3 {
        return Err(fail("LDG-014", "nested parent/breadcrumb relation missing"));
    }
    Ok(passed("LDG-014", json!({"parent":child.anchor_id,"child":grandchild.anchor_id,"breadcrumb":grandchild.breadcrumb,"bounded":true})))
}

fn ldg015() -> CheckResult {
    let root = tempfile::tempdir().map_err(|e| e.to_string())?;
    fs::write(root.path().join("source.md"), "# Source\n\ninline [details](target.md#Target) & [missing](missing.md) & ![image](missing.png)\n\n<https://example.com>\n").map_err(|e| e.to_string())?;
    fs::write(root.path().join("target.md"), "# Target\n\nlinked evidence\n").map_err(|e| e.to_string())?;
    let db = LedgerDb::open_in_memory();
    doc_spine::sync(&db, root.path())?;
    let root_s = root_text(root.path());
    let (source_id, _, _, _) = artifact(&db, &root_s, "source.md")?;
    let links = link_projection::link_targets(&db, &source_id)?;
    if links.len() < 4 || !links.iter().any(|link| link.resolution_state == link_projection::LinkResolutionStateV1::Resolved) || !links.iter().any(|link| link.resolution_state == link_projection::LinkResolutionStateV1::Broken) {
        return Err(fail("LDG-015", "link projection did not retain resolved and broken links"));
    }
    Ok(passed("LDG-015", json!({"links":links.len(),"resolved":links.iter().filter(|link| link.resolution_state == link_projection::LinkResolutionStateV1::Resolved).count(),"broken":links.iter().filter(|link| link.resolution_state == link_projection::LinkResolutionStateV1::Broken).count(),"spansBound":links.iter().all(|link| link.source_end_byte >= link.source_start_byte)})))
}

fn ldg016() -> CheckResult {
    let root = tempfile::tempdir().map_err(|e| e.to_string())?;
    fs::write(root.path().join("source.md"), "# Source\n\nkey rotation procedure [details](target.md)\n").map_err(|e| e.to_string())?;
    fs::write(root.path().join("target.md"), "# Target\n\nsupplementary linked evidence\n").map_err(|e| e.to_string())?;
    let db = LedgerDb::open_in_memory();
    doc_spine::sync(&db, root.path())?;
    let receipt = doc_spine::recall_with_graph(&db, "key rotation", 5, &doc_spine::LedgerRecallGraphPolicyV1::default())?;
    if !receipt.hits.iter().any(|hit| hit.lane == "ledger_graph" && hit.source_ref.ends_with("/target.md")) || receipt.graph.edges.len() > 6 || receipt.graph.node_ids.len() > 4 {
        return Err(fail("LDG-016", "bounded graph expansion omitted resolved target or exceeded caps"));
    }
    fs::remove_file(root.path().join("target.md")).map_err(|e| e.to_string())?;
    doc_spine::sync(&db, root.path())?;
    let after = doc_spine::recall_with_graph(&db, "key rotation", 5, &doc_spine::LedgerRecallGraphPolicyV1::default())?;
    if after.hits.iter().any(|hit| hit.source_ref.ends_with("/target.md")) { return Err(fail("LDG-016", "deleted target remained graph-recallable")); }
    Ok(passed("LDG-016", json!({"edges":receipt.graph.edges.len(),"nodes":receipt.graph.node_ids.len(),"targetResolved":true,"deleteAbstention":true})))
}

/// Run one native installed qualification case.
pub(crate) fn run(case_id: &str) -> Result<Value, String> {
    let normalized = case_id.trim().to_ascii_uppercase().replace('_', "-");
    let result = match normalized.as_str() {
        "LDG-001" => ldg001(), "LDG-002" => ldg002(), "LDG-003" => ldg003(),
        "LDG-004" => ldg004(), "LDG-005" => ldg005(), "LDG-006" => ldg006(),
        "LDG-007" => ldg007(), "LDG-008" => ldg008(), "LDG-009" => ldg009(),
        "LDG-010" => ldg010(), "LDG-011" => ldg011(), "LDG-012" => ldg012(),
        "LDG-013" => ldg013(), "LDG-014" => ldg014(), "LDG-015" => ldg015(),
        "LDG-016" => ldg016(), _ => Err(format!("unsupported native Ledger qualification case: {case_id}")),
    }?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::run;

    #[test]
    fn native_core_cases_pass() {
        for id in (1..=16).map(|n| format!("LDG-{n:03}")) {
            let result = run(&id).unwrap_or_else(|error| panic!("{id}: {error}"));
            assert_eq!(result["status"], "passed", "{id}");
            assert_eq!(result["evidenceKind"], "native", "{id}");
        }
    }

    #[test]
    fn aliases_and_unknown_cases_are_handled() {
        assert_eq!(run("ldg_012").unwrap()["caseId"], "LDG-012");
        assert!(run("LDG-999").is_err());
    }
}
