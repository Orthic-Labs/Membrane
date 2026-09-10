//! Native, row-specific Blueprint provider qualification.
//!
//! This module is deliberately independent of CLI/MCP adapters.  Qualification
//! invokes the same public Blueprint graph, provider, resolver, store, and
//! atomic-publication APIs used by production callers, then returns compact
//! source-grounded evidence for BPT-002..016.  It is kept here so an installed
//! runtime can run these controls without loading any legacy JavaScript.

use membrane_blueprint::{graph, providers, CancellationToken};
use serde_json::{json, Value};
use std::fs;
use std::path::Path;

type QResult = Result<Value, String>;

fn temp_fixture() -> Result<tempfile::TempDir, String> {
    tempfile::Builder::new()
        .prefix("membrane-bpt-")
        .tempdir()
        .map_err(|e| format!("fixture: {e}"))
}

fn write(root: &Path, path: &str, text: &str) -> Result<(), String> {
    let target = root.join(path);
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
    }
    fs::write(&target, text).map_err(|e| format!("write {}: {e}", target.display()))
}

fn require(condition: bool, message: impl Into<String>) -> Result<(), String> {
    condition.then_some(()).ok_or_else(|| message.into())
}

fn evidence(case_id: &str, assertions: Vec<Value>, payload: Value) -> Value {
    json!({
        "schemaVersion": 1,
        "caseId": case_id,
        "status": "passed",
        "runtime": "native-rust",
        "evidenceKind": "installed_native",
        "assertions": assertions,
        "payload": payload,
    })
}

fn assertion(id: &str, ok: bool, detail: impl Into<String>) -> Value {
    json!({"id": id, "ok": ok, "detail": detail.into()})
}

fn route_value(route: &providers::frameworks::RouteFact) -> Value {
    json!({
        "kind": route.kind,
        "method": route.method,
        "path": route.path,
        "handler": route.handler,
        "name": route.name,
        "line": route.line,
        "confidence": route.confidence,
        "evidence": route.evidence,
    })
}

fn stack_fact_value(fact: &providers::frameworks::StackFact) -> Value {
    json!({
        "kind": fact.kind,
        "topic": fact.topic,
        "name": fact.name,
        "action": fact.action,
        "resourceType": fact.resource_type,
        "line": fact.line,
        "edge": fact.edge,
        "confidence": fact.confidence,
        "evidence": fact.evidence,
    })
}

fn build(root: &Path) -> Result<graph::GraphGeneration, String> {
    graph::build_generation(root, &graph::GraphOptions::default())
        .map_err(|e| format!("native graph build: {e}"))
}

fn git(root: &Path, args: &[&str]) -> Result<String, String> {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .map_err(|e| format!("git spawn: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn case_002() -> QResult {
    let fixture = temp_fixture()?;
    let root = fixture.path();
    write(root, "src/main.rs", "fn main() { println!(\"ok\"); }\n")?;
    git(root, &["init", "-q"])?;
    git(
        root,
        &["config", "user.email", "qualification@example.invalid"],
    )?;
    git(root, &["config", "user.name", "Blueprint Qualification"])?;
    git(root, &["add", "."])?;
    git(root, &["commit", "-qm", "fixture"])?;
    let clean = membrane_blueprint::git_source_observation::git_source_observation_at(root)
        .ok_or_else(|| "git observation unavailable".to_string())?;
    write(
        root,
        "src/main.rs",
        "fn main() { println!(\"changed\"); }\n",
    )?;
    let dirty = membrane_blueprint::git_source_observation::git_source_observation_at(root)
        .ok_or_else(|| "dirty git observation unavailable".to_string())?;
    require(clean.head == dirty.head, "working-tree edit changed HEAD")?;
    require(
        !clean.dirty && dirty.dirty,
        "git clean/dirty transition not observed",
    )?;
    Ok(evidence(
        "BPT-002",
        vec![assertion(
            "git-source-observation",
            true,
            "same HEAD with bounded clean→dirty status transition",
        )],
        json!({"clean": clean, "dirty": dirty}),
    ))
}

fn case_003() -> QResult {
    let fixture = temp_fixture()?;
    let root = fixture.path();
    write(root, "admitted.rs", "pub fn admitted() {}\n")?;
    write(root, "unsupported.bin", "not binary\n")?;
    let report =
        providers::source_disposition::audit_source_dispositions(root, &["admitted.rs".into()]);
    require(report.complete, "source disposition report incomplete")?;
    require(report.indexed == 1, "admitted source was not counted")?;
    Ok(evidence(
        "BPT-003",
        vec![assertion(
            "source-disposition",
            true,
            "admitted source plus terminal non-admitted disposition",
        )],
        serde_json::to_value(report).map_err(|e| e.to_string())?,
    ))
}

fn case_004() -> QResult {
    let fixture = temp_fixture()?;
    write(
        fixture.path(),
        "src/lib.rs",
        "pub fn service() { helper(); }\nfn helper() {}\n",
    )?;
    write(
        fixture.path(),
        "src/worker.py",
        "def worker():\n    return 1\n",
    )?;
    let generation = build(fixture.path())?;
    let ast = generation
        .files
        .iter()
        .filter(|f| f.precision == graph::PrecisionTier::Ast)
        .count();
    let symbols = generation
        .nodes
        .iter()
        .filter(|n| n.kind == "symbol")
        .count();
    require(
        ast > 0 && symbols >= 2,
        "native AST provider did not emit symbols",
    )?;
    Ok(evidence(
        "BPT-004",
        vec![assertion(
            "ast-facts",
            true,
            format!("{ast} AST reports, {symbols} symbols"),
        )],
        json!({
            "generationId": generation.generation_id,
            "sourceHash": generation.source_hash,
            "astReports": ast,
            "symbols": symbols,
        }),
    ))
}

fn case_005() -> QResult {
    let known = membrane_blueprint::lib_cli_languages::language_by_extension(".RS");
    let unknown = membrane_blueprint::lib_cli_languages::language_by_extension(".qualification");
    require(
        known["fallback"] == Value::Bool(false),
        "known Rust extension fell back",
    )?;
    require(
        unknown["fallback"] == Value::Bool(true),
        "unknown extension was not lexical fallback",
    )?;
    let catalog = membrane_blueprint::lib_cli_languages::languages_json();
    require(
        catalog["languages"]
            .as_array()
            .map_or(false, |v| !v.is_empty()),
        "language catalog empty",
    )?;
    Ok(evidence(
        "BPT-005",
        vec![assertion(
            "language-capability",
            true,
            "known parser & explicit lexical fallback",
        )],
        json!({"known": known, "unknown": unknown, "catalog": catalog}),
    ))
}

fn case_006() -> QResult {
    let valid = json!({"metadata":{"tool":"qualification"},"documents":[{"relativePath":"main.py","occurrences":[{"symbol":"pkg/main.py/Thing","range":[1,0,1,4],"roles":["definition"]}]}]});
    let normalized =
        providers::scip::normalize_scip_index(&valid, None).map_err(|e| e.to_string())?;
    require(
        normalized.documents.len() == 1 && normalized.occurrences.len() == 1,
        "valid SCIP occurrence was dropped",
    )?;
    let malformed = providers::scip::normalize_scip_index(&json!({"documents": [{"relativePath":"bad.py","occurrences":[{"symbol":"bad","range":[1],"roles":["definition"]}]}]}), None).map_err(|e| e.to_string())?;
    require(
        malformed.skipped_occurrences == 1,
        "malformed SCIP occurrence was not typed/skipped",
    )?;
    Ok(evidence(
        "BPT-006",
        vec![assertion(
            "scip-integrity",
            true,
            "valid occurrence retained; malformed range skipped",
        )],
        json!({
            "validOccurrences": normalized.occurrences.len(), "skippedOccurrences": malformed.skipped_occurrences,
        }),
    ))
}

fn case_007() -> QResult {
    let fixture = temp_fixture()?;
    let root = fixture.path();
    write(
        root,
        "src/main.ts",
        "import { run } from './worker';\nrun();\n",
    )?;
    write(root, "src/worker.ts", "export function run() {}\n")?;
    let source = root.join("src/main.ts");
    let resolved = membrane_blueprint::module_resolution::resolve_js_module(
        membrane_blueprint::module_resolution::JsResolveInput {
            specifier: "./worker",
            from_file: source.to_str().unwrap_or_default(),
            repo_root: root.to_str(),
            is_typescript: true,
        },
    );
    require(
        matches!(
            resolved,
            membrane_blueprint::module_resolution::JsResolution::Resolved { .. }
        ),
        "exact JS module did not resolve",
    )?;
    let outside = membrane_blueprint::module_resolution::resolve_js_module(
        membrane_blueprint::module_resolution::JsResolveInput {
            specifier: "../../outside",
            from_file: source.to_str().unwrap_or_default(),
            repo_root: root.to_str(),
            is_typescript: true,
        },
    );
    require(
        matches!(
            outside,
            membrane_blueprint::module_resolution::JsResolution::Unresolved { .. }
                | membrane_blueprint::module_resolution::JsResolution::OutsideRepo
        ),
        "missing/outside JS module resolved",
    )?;
    Ok(evidence(
        "BPT-007",
        vec![assertion(
            "module-resolution",
            true,
            "exact-first resolution & outside/missing refusal",
        )],
        json!({
            "resolved": format!("{resolved:?}"), "negative": format!("{outside:?}"),
        }),
    ))
}

fn case_008() -> QResult {
    let imports = providers::frameworks::imported_packages("import express from 'express';\n");
    require(
        providers::frameworks::framework_gate_active(&imports, "next-express"),
        "Express gate inactive",
    )?;
    let routes = providers::frameworks::extract_routes(
        "next-express",
        "app.get('/health', health); // app.get('/fake', nope)\n",
        "src/routes.js",
    );
    require(
        routes.iter().any(|r| r.path.as_deref() == Some("/health")),
        "gated HTTP route missing",
    )?;
    require(
        !routes.iter().any(|r| r.path.as_deref() == Some("/fake")),
        "comment fabricated HTTP route",
    )?;
    Ok(evidence(
        "BPT-008",
        vec![assertion(
            "framework-gate",
            true,
            "import-gated route extraction ignores commented route",
        )],
        json!({
            "imports": imports,
            "routes": routes.iter().map(route_value).collect::<Vec<_>>(),
        }),
    ))
}

fn case_009() -> QResult {
    let facts = providers::frameworks::extract_database_facts(
        "model User { id Int }\nclient.find(\"User\"); client.create(\"User\");\n",
        "schema.prisma",
    );
    require(
        facts
            .iter()
            .any(|f| f.kind == "model" && f.name.as_deref() == Some("User")),
        "SQL/model declaration missing",
    )?;
    require(
        facts.iter().any(|f| f.edge.as_deref() == Some("READS")),
        "database read fact missing",
    )?;
    require(
        facts.iter().any(|f| f.edge.as_deref() == Some("WRITES")),
        "database write fact missing",
    )?;
    Ok(evidence(
        "BPT-009",
        vec![assertion(
            "sql-facts",
            true,
            "model/read/write facts are source-line bound",
        )],
        json!({
            "facts": facts.iter().map(stack_fact_value).collect::<Vec<_>>(),
        }),
    ))
}

fn case_010() -> QResult {
    let registry = providers::registry();
    require(!registry.is_empty(), "native provider registry empty")?;
    let ids: Vec<&str> = registry.iter().map(|d| d.id).collect();
    let positions: Vec<usize> = ids
        .iter()
        .filter_map(|id| {
            providers::PROVIDER_ORDER
                .iter()
                .position(|expected| expected == id)
        })
        .collect();
    require(
        positions.windows(2).all(|w| w[0] <= w[1]),
        "provider registry order is not deterministic",
    )?;
    require(
        !ids.iter().any(|id| *id == "legacy-js"),
        "legacy provider admitted",
    )?;
    Ok(evidence(
        "BPT-010",
        vec![assertion(
            "provider-admission",
            true,
            "native registry ordered & legacy provider absent",
        )],
        json!({"providers": ids}),
    ))
}

fn case_011() -> QResult {
    let fixture = temp_fixture()?;
    write(fixture.path(), "src/main.rs", "pub fn observed() {}\n")?;
    let generation = build(fixture.path())?;
    require(
        generation
            .nodes
            .iter()
            .any(|n| n.name.as_deref() == Some("observed")),
        "document/code source was not observed",
    )?;
    require(
        generation
            .nodes
            .iter()
            .filter_map(|n| n.evidence.first())
            .any(|e| e.get("contentHash").and_then(Value::as_str).is_some()),
        "observation lacks content hash",
    )?;
    Ok(evidence(
        "BPT-011",
        vec![assertion(
            "document-truth-source",
            true,
            "observed symbol carries source content hash",
        )],
        json!({"generationId": generation.generation_id, "nodes": generation.nodes.len()}),
    ))
}

fn case_012() -> QResult {
    let cancelled = CancellationToken::new();
    cancelled.cancel();
    let fixture = temp_fixture()?;
    write(fixture.path(), "src/main.rs", "fn main() {}\n")?;
    let result = graph::build_generation_with_cancellation(
        fixture.path(),
        &graph::GraphOptions::default(),
        &cancelled,
    );
    require(result.is_err(), "cancelled provider/build was not refused")?;
    let unsupported = providers::scip::normalize_scip_index(&json!({"notDocuments": []}), None);
    require(unsupported.is_err(), "incompatible SCIP input was accepted")?;
    Ok(evidence(
        "BPT-012",
        vec![assertion(
            "bounded-faults",
            true,
            "cancellation & incompatible provider input fail closed",
        )],
        json!({
            "cancelled": result.err().map(|e| e.to_string()), "unsupported": unsupported.err().map(|e| e.to_string()),
        }),
    ))
}

fn case_013() -> QResult {
    let fixture = temp_fixture()?;
    let root = fixture.path();
    write(root, "src/old.rs", "pub fn stable_anchor() {}\n")?;
    let before = build(root)?;
    fs::rename(root.join("src/old.rs"), root.join("src/new.rs")).map_err(|e| e.to_string())?;
    let after = build(root)?;
    let old = before
        .nodes
        .iter()
        .find(|n| n.name.as_deref() == Some("stable_anchor"))
        .ok_or("old anchor missing")?;
    let new = after
        .nodes
        .iter()
        .find(|n| n.name.as_deref() == Some("stable_anchor"))
        .ok_or("new anchor missing")?;
    require(
        old.evidence.first().and_then(|v| v.get("contentHash"))
            == new.evidence.first().and_then(|v| v.get("contentHash")),
        "rename changed source identity",
    )?;
    require(old.path != new.path, "rename did not change path")?;
    Ok(evidence(
        "BPT-013",
        vec![assertion(
            "rename-reanchor",
            true,
            "same content hash re-anchored from old path to new path",
        )],
        json!({"before": old, "after": new}),
    ))
}

fn case_014() -> QResult {
    let fixture = temp_fixture()?;
    write(
        fixture.path(),
        "src/search.rs",
        "pub fn qualification_marker() {}\n",
    )?;
    let generation = build(fixture.path())?;
    let marker = generation
        .nodes
        .iter()
        .find(|n| n.name.as_deref() == Some("qualification_marker"))
        .ok_or("search marker missing")?;
    let hash = marker
        .evidence
        .first()
        .and_then(|e| e.get("contentHash"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    require(
        !hash.is_empty() && !generation.generation_id.is_empty(),
        "search evidence is not generation/freshness bound",
    )?;
    Ok(evidence(
        "BPT-014",
        vec![assertion(
            "freshness-receipt",
            true,
            "search source evidence includes content hash & generation",
        )],
        json!({
            "generationId": generation.generation_id, "contentHash": hash, "sourceHash": generation.source_hash,
        }),
    ))
}

fn case_015() -> QResult {
    let fixture = temp_fixture()?;
    let target = fixture.path().join("graph.db");
    let source = fixture.path().join("candidate.db");
    write(fixture.path(), "graph.db", "last-known-good")?;
    write(fixture.path(), "candidate.db", "candidate")?;
    let adopted = membrane_blueprint::atomic_adopt::adopt_file_atomically(
        &source,
        &target,
        "win32",
        |path| {
            (fs::read_to_string(path).map_err(|e| {
                membrane_blueprint::atomic_adopt::AtomicAdoptError {
                    message: e.to_string(),
                }
            })? == "candidate")
                .then_some(())
                .ok_or_else(|| membrane_blueprint::atomic_adopt::AtomicAdoptError {
                    message: "candidate mismatch".into(),
                })
        },
    )
    .map_err(|e| e.to_string())?;
    require(
        adopted.replaced && fs::read_to_string(&target).map_err(|e| e.to_string())? == "candidate",
        "atomic candidate adoption failed",
    )?;
    write(fixture.path(), "candidate.db", "corrupt")?;
    let failed = membrane_blueprint::atomic_adopt::adopt_file_atomically(
        &source,
        &target,
        "win32",
        |_path| {
            Err(membrane_blueprint::atomic_adopt::AtomicAdoptError {
                message: "integrity failure".into(),
            })
        },
    );
    require(failed.is_err(), "invalid candidate was accepted")?;
    require(
        fs::read_to_string(&target).map_err(|e| e.to_string())? == "candidate",
        "failed adoption did not preserve current store",
    )?;
    Ok(evidence(
        "BPT-015",
        vec![assertion(
            "atomic-publication",
            true,
            "candidate swap commits only after validation; failed input preserves target",
        )],
        json!({
            "adopted": {"target": adopted.target, "replaced": adopted.replaced},
            "failed": failed.err().map(|e| e.to_string()),
        }),
    ))
}

fn case_016() -> QResult {
    let mut pending =
        membrane_blueprint::delta_store::PendingDomains::from_stored("provider,source,provider");
    pending.mark("writer");
    require(
        pending
            .domains()
            .iter()
            .map(String::as_str)
            .eq(["provider", "provider", "source", "writer"]),
        "writer/domain lease state was not deterministic",
    )?;
    pending.clear("provider");
    require(
        pending.to_stored().as_deref() == Some("source,writer"),
        "pending domain recovery did not clear acknowledged writer",
    )?;
    let fixture = temp_fixture()?;
    let target = fixture.path().join("lease");
    write(fixture.path(), "lease", "stable")?;
    let source = fixture.path().join("lease.new");
    write(fixture.path(), "lease.new", "next")?;
    let failure = membrane_blueprint::atomic_adopt::adopt_file_atomically(
        &source,
        &target,
        "win32",
        |_path| {
            Err(membrane_blueprint::atomic_adopt::AtomicAdoptError {
                message: "writer validation failed".into(),
            })
        },
    );
    require(
        failure.is_err() && fs::read_to_string(&target).map_err(|e| e.to_string())? == "stable",
        "lease recovery lost stable generation",
    )?;
    Ok(evidence(
        "BPT-016",
        vec![assertion(
            "writer-lease-recovery",
            true,
            "pending domains acknowledge deterministically & failed writer restores generation",
        )],
        json!({
            "pendingAfterClear": pending.to_stored(), "failedWriter": failure.err().map(|e| e.to_string()),
        }),
    ))
}

/// Run one installed-native Blueprint provider qualification row.
pub(crate) fn run(case_id: &str) -> QResult {
    match case_id {
        "BPT-002" => case_002(),
        "BPT-003" => case_003(),
        "BPT-004" => case_004(),
        "BPT-005" => case_005(),
        "BPT-006" => case_006(),
        "BPT-007" => case_007(),
        "BPT-008" => case_008(),
        "BPT-009" => case_009(),
        "BPT-010" => case_010(),
        "BPT-011" => case_011(),
        "BPT-012" => case_012(),
        "BPT-013" => case_013(),
        "BPT-014" => case_014(),
        "BPT-015" => case_015(),
        "BPT-016" => case_016(),
        other => Err(format!(
            "unsupported Blueprint provider qualification row: {other}"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bpt_provider_rows_are_native_and_row_specific() {
        for id in [
            "BPT-002", "BPT-003", "BPT-004", "BPT-005", "BPT-006", "BPT-007", "BPT-008", "BPT-009",
            "BPT-010", "BPT-011", "BPT-012", "BPT-013", "BPT-014", "BPT-015", "BPT-016",
        ] {
            let report = run(id).unwrap_or_else(|e| panic!("{id}: {e}"));
            assert_eq!(report["caseId"], id);
            assert_eq!(report["runtime"], "native-rust");
            assert_eq!(report["evidenceKind"], "installed_native");
            assert!(report["assertions"]
                .as_array()
                .is_some_and(|items| !items.is_empty()));
        }
    }

    #[test]
    fn unknown_rows_fail_closed() {
        assert!(run("BPT-999").is_err());
    }
}
