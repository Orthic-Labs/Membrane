//! Owner-bound Ledger acceptance coverage.
//!
//! These tests exercise the public registration, conversion, index, and exact
//! resolver paths. Installed daemon/grant delivery remains a host qualification
//! boundary and is intentionally not enabled by this test target.

use membrane_runtime::ledger::{
    doc_spine, document_conversion::*, index, resolve, LedgerDb,
};
use membrane_runtime::catalog::{self, ContextCatalog, GrantStatus};
use membrane_runtime::memdb::MemDb;
use membrane_runtime::store::MemoryStore;
use membrane_protocol::ReadPathV1;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{fs, sync::Arc};

fn hash(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn resolver_grant(catalog: &ContextCatalog, id: &str, repository: &str, session: &str) -> catalog::ScopeGrant {
    catalog::issue_scope_grant(
        catalog,
        id,
        "ledger-owner-test",
        &[repository.into()],
        &["source_read".into()],
        &[ReadPathV1 { path: "note.md".into(), start_line: 1, end_line: 20 }],
        "ledger-task",
        session,
        600,
        "ledger-nonce",
        "sha256:1111111111111111111111111111111111111111111111111111111111111111",
    ).unwrap()
}

struct Environment(Vec<(&'static str, Option<std::ffi::OsString>)>);
impl Environment {
    fn set(values: &[(&'static str, std::path::PathBuf)]) -> Self {
        let mut previous = Vec::new();
        for (key, value) in values {
            previous.push((*key, std::env::var_os(key)));
            std::env::set_var(key, value);
        }
        Self(previous)
    }
}
impl Drop for Environment {
    fn drop(&mut self) {
        for (key, previous) in self.0.drain(..).rev() {
            match previous {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }
}

fn call(server: &membrane_mcp::McpServer, name: &str, arguments: Value) -> Value {
    server.dispatch(&json!({
        "jsonrpc":"2.0","id":1,"method":"tools/call",
        "params":{"name":name,"arguments":arguments}
    })).unwrap()
}

fn data(response: &Value) -> &Value {
    assert_eq!(response.pointer("/result/isError"), Some(&json!(false)), "{response}");
    response.pointer("/result/structuredContent/result/data").unwrap()
}

fn section_request(db: &LedgerDb, path: &str) -> resolve::ResolveRequest {
    db.lock()
        .query_row(
            "SELECT a.doc_id,n.node_id,a.content_hash,a.revision,a.index_generation,n.span_hash
             FROM ledger_doc_artifacts a JOIN ledger_nodes n ON n.doc_id=a.doc_id
             WHERE a.path=?1 AND n.node_kind='section' ORDER BY n.ordinal LIMIT 1",
            [path],
            |row| {
                Ok(resolve::ResolveRequest {
                    doc_id: Some(row.get(0)?),
                    node_id: Some(row.get(1)?),
                    source_ref: format!("doc://repo/worktree/{path}"),
                    anchor_id: row.get(1)?,
                    expected_content_hash: row.get(2)?,
                    expected_revision: Some(row.get(3)?),
                    expected_span_hash: Some(row.get(5)?),
                    ledger_generation: Some(row.get(4)?),
                    continuation_cursor: None,
                    max_bytes: 12_000,
                })
            },
        )
        .unwrap()
}

#[test]
fn equal_content_sources_keep_distinct_identity_and_exact_resolution() {
    let left = tempfile::tempdir().unwrap();
    let right = tempfile::tempdir().unwrap();
    let bytes = b"# Same\n\nidentity remains source-bound\n";
    fs::write(left.path().join("same.md"), bytes).unwrap();
    fs::write(right.path().join("same.md"), bytes).unwrap();
    let left_db = LedgerDb::open_in_memory();
    let right_db = LedgerDb::open_in_memory();
    doc_spine::sync(&left_db, left.path()).unwrap();
    doc_spine::sync(&right_db, right.path()).unwrap();
    let left_id: String = left_db.lock().query_row(
        "SELECT doc_id FROM ledger_doc_artifacts WHERE path='same.md'", [], |r| r.get(0),
    ).unwrap();
    let right_id: String = right_db.lock().query_row(
        "SELECT doc_id FROM ledger_doc_artifacts WHERE path='same.md'", [], |r| r.get(0),
    ).unwrap();
    assert_ne!(left_id, right_id);
    let request = section_request(&left_db, "same.md");
    assert_eq!(resolve::resolve(&left_db, left.path(), &request).unwrap().doc_id, left_id);
    assert!(resolve::resolve(&left_db, right.path(), &request).is_err());
}

#[test]
fn resolver_rejects_path_escape_and_source_revision_drift() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("note.md"), "# Note\n\noriginal\n").unwrap();
    let db = LedgerDb::open_in_memory();
    doc_spine::sync(&db, root.path()).unwrap();
    let request = section_request(&db, "note.md");
    let mut escape = request.clone();
    escape.source_ref = "doc://repo/worktree/../note.md".into();
    assert_eq!(resolve::resolve(&db, root.path(), &escape).unwrap_err(), resolve::ResolveError::Denied);
    fs::write(root.path().join("note.md"), "# Note\n\nchanged\n").unwrap();
    assert_eq!(resolve::resolve(&db, root.path(), &request).unwrap_err(), resolve::ResolveError::Stale);
}

#[test]
fn conversion_is_explicit_format_granted_with_provenance_loss_and_omissions() {
    let denied = DocumentConversionGrantV1::denied();
    let input = DocumentConversionInputV1 { source_ref: "snapshot://denied".into(), format: DocumentInputFormatV1::PlainText, raw_input: b"text".to_vec() };
    assert_eq!(convert_granted_document(&denied, input).unwrap_err(), DocumentConversionErrorV1::NotGranted);

    let grant = DocumentConversionGrantV1::new([DocumentInputFormatV1::Html], 4096);
    let converted = convert_granted_document(&grant, DocumentConversionInputV1 {
        source_ref: "snapshot://html".into(), format: DocumentInputFormatV1::Html,
        raw_input: b"<h1>Title</h1><img src='x'>Body".to_vec(),
    }).unwrap();
    assert_eq!(converted.raw_sha256, hash(b"<h1>Title</h1><img src='x'>Body"));
    assert_eq!(converted.converter.converter, "ledger.html-text");
    assert_eq!(converted.converter.version, "1");
    assert!(converted.losses.contains(&ConversionLossV1::FormattingFlattened));
    assert_eq!(converted.omissions, vec![ConversionOmissionV1::EmbeddedMediaExcluded { count: 1 }]);
    assert_eq!(converted.markdown_sha256, hash(converted.markdown.as_bytes()));
}

#[test]
fn fts_activation_requires_exact_qualification_receipt() {
    let db = LedgerDb::open_in_memory();
    assert_eq!(index::recall_mode(&db).unwrap(), index::LedgerRecallMode::LegacyScan);
    assert_eq!(index::activate(&db, index::LedgerRecallMode::LedgerFts, None).unwrap_err(), "ledger_fts_requires_qualification");
}

#[test]
fn public_resolver_binds_session_and_rechecks_grant_lifecycle() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("workspace");
    fs::create_dir_all(root.join("tools/lib/memory")).unwrap();
    let markdown = "# Note\n\nexact Ledger source\n";
    fs::write(root.join("note.md"), markdown).unwrap();
    let root = root.canonicalize().unwrap();
    let repository = "repo-ledger-owner";
    let registry = temp.path().join("registry.json");
    fs::write(&registry, json!({"schema_version":2,"bindings":{
        root.to_string_lossy().as_ref():{
            "repository_id":repository,"scope_id":"installation-scope",
            "grant_policy":{"level":"read-only"}
        }
    }}).to_string()).unwrap();
    let catalog_path = temp.path().join("catalog.db");
    let _environment = Environment::set(&[
        ("MEMBRANE_PROJECT_REGISTRY", registry),
        ("MEMBRANE_CATALOG", catalog_path.clone()),
        ("WORKSPACE_ROOT", root.clone()),
    ]);
    let catalog = ContextCatalog::open(&catalog_path).unwrap();
    let executor = membrane_runtime::mcp_executor::RuntimeMcpExecutor::for_hub(
        MemoryStore::open(MemDb::open_in_memory()),
    ).unwrap();
    assert!(membrane_mcp::install_executor(Arc::new(executor)).is_ok());
    let server = membrane_mcp::McpServer::default();
    let caller = json!({"root":root,"repositoryId":repository,"scopeId":"installation-scope"});

    // Legacy known-section reads remain authorized without task grant or session.
    let outline = membrane_runtime::ledger::outline::build_outline(
        "doc://repo/worktree/note.md", markdown, "comrak-0.54.0",
    );
    let legacy = call(&server, "membrane_source_read", json!({
        "repository":repository,"caller":caller.clone(),
        "sourceRef":"doc://repo/worktree/note.md","anchorId":"sec:note:1",
        "expectedContentHash":outline.content_hash
    }));
    assert_eq!(data(&legacy)["registered"], false, "{legacy}");

    let ticket_for = |grant_id: &str, session: &str| -> Value {
        resolver_grant(&catalog, grant_id, repository, session);
        let recall = call(&server, "membrane_ledger", json!({
            "repository":repository,"caller":caller.clone(),"operation":"recall",
            "query":"exact Ledger source","k":1,"scopeGrantId":grant_id,
            "taskId":"ledger-task","sessionId":session
        }));
        data(&recall)["hits"][0].clone()
    };
    let resolve_args = |hit: &Value, session: Option<&str>| {
        let mut arguments = json!({
            "repository":repository,"caller":caller.clone(),
            "docId":hit["docId"],"nodeId":hit["nodeId"],
            "sourceRef":hit["sourceRef"],"anchorId":hit["anchorId"],
            "expectedContentHash":hit["expectedContentHash"],
            "expectedRevision":hit["expectedRevision"],
            "expectedSpanHash":hit["expectedSpanHash"],
            "ledgerGeneration":hit["ledgerGeneration"],"ledgerTicket":hit["ledgerTicket"]
        });
        if let Some(session) = session { arguments["sessionId"] = json!(session); }
        arguments
    };
    let error_message = |response: &Value| {
        assert_eq!(response.pointer("/result/isError"), Some(&json!(true)), "{response}");
        response.pointer("/result/structuredContent/result/message").and_then(Value::as_str).unwrap().to_owned()
    };

    let exact = ticket_for("ledger-session-ok", "request-session");
    let resolved = call(&server, "membrane_source_read", resolve_args(&exact, Some("request-session")));
    assert_eq!(data(&resolved)["section"]["content"], markdown);
    assert!(error_message(&call(&server, "membrane_source_read", resolve_args(&exact, None)))
        .contains("ledger_session_id_required"));
    assert!(error_message(&call(&server, "membrane_source_read", resolve_args(&exact, Some("wrong-session"))))
        .contains("ledger_scope_grant_invalid"));

    let revoked = ticket_for("ledger-session-revoked", "request-session");
    assert!(catalog::revoke_scope_grant(&catalog, "ledger-session-revoked").unwrap());
    assert_eq!(catalog::lookup_grant(&catalog, "ledger-session-revoked").unwrap().unwrap().status, GrantStatus::Revoked);
    assert!(error_message(&call(&server, "membrane_source_read", resolve_args(&revoked, Some("request-session"))))
        .contains("ledger_scope_grant_invalid"));

    let expired = ticket_for("ledger-session-expired", "request-session");
    catalog.lock().execute(
        "UPDATE scope_grants SET expires_at_unix=0 WHERE id=?1",
        [&"ledger-session-expired"],
    ).unwrap();
    assert_eq!(catalog::lookup_grant(&catalog, "ledger-session-expired").unwrap().unwrap().status, GrantStatus::Expired);
    assert!(error_message(&call(&server, "membrane_source_read", resolve_args(&expired, Some("request-session"))))
        .contains("ledger_scope_grant_invalid"));
}
