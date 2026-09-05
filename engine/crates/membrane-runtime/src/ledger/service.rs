//! The daemon's sole operational Ledger owner.
//!
//! CLI and MCP are clients of this owner. Lazy index repair is a cache effect
//! under an enrolled source grant, never permission to edit source documents.
//! Resolution tickets retain the original source expectations and task grant;
//! neither a cursor nor an agent-supplied root can increase read authority.

use super::{doc_spine, index, limits::WorkBudget, policy::SourcePolicy, query,
    resolve::{self, ResolveRequest}, LedgerDb};
use crate::authorization::{self, AuthorizationRequest};
use membrane_protocol::ReadPathV1;
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock, TryLockError};
use std::time::Duration;

pub const SERVICE_VERSION: &str = "ledger.owner.v1";
pub const RESOLVER_VERSION: &str = "ledger.exact-node.v1";
const TICKET_TTL_MS: i64 = 10 * 60 * 1000;
const OWNER_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS ledger_owner_roots (
    repository_root TEXT PRIMARY KEY, generation INTEGER NOT NULL,
    policy_digest TEXT NOT NULL, synced_at_ms INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS ledger_resolution_tickets (
    ticket_hash TEXT PRIMARY KEY, repository_root TEXT NOT NULL,
    caller_digest TEXT NOT NULL, doc_id TEXT NOT NULL,
    request_json TEXT NOT NULL, grant_id TEXT, expires_at_ms INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS ledger_tickets_root ON ledger_resolution_tickets(repository_root,expires_at_ms);
CREATE TABLE IF NOT EXISTS ledger_erasure_fences (
    repository_root TEXT NOT NULL, path_digest TEXT NOT NULL,
    erased_at_ms INTEGER NOT NULL, PRIMARY KEY(repository_root,path_digest)
);
"#;

static DAEMON_OWNER: OnceLock<Result<Arc<LedgerService>, String>> = OnceLock::new();

/// Called only when the tray-owned daemon installs its native operation owner.
/// Failure is isolated to Ledger; clients cannot create an alternative owner.
pub(crate) fn install_daemon_owner() {
    DAEMON_OWNER.get_or_init(|| LedgerDb::open_default().and_then(LedgerService::new).map(Arc::new));
}
pub(crate) fn active_owner() -> Result<Arc<LedgerService>, String> {
    DAEMON_OWNER.get().ok_or("membrane_unavailable:hub_inactive")?.clone()
}

#[derive(Clone)]
pub(crate) struct Caller {
    pub root: String,
    registry_root: String,
    pub repository_id: String,
    pub scope_id: String,
    descriptor: Option<Value>,
    level: Option<String>,
}
impl Caller {
    pub(crate) fn from_arguments(arguments: &Value) -> Result<Self, String> {
        let get = |key: &str| arguments.pointer(&format!("/caller/{key}"))
            .and_then(Value::as_str).filter(|s| !s.trim().is_empty()).map(str::to_owned)
            .ok_or_else(|| "ledger_caller_required".to_owned());
        let root = resolve::normalized_root(Path::new(&get("root")?)).map_err(|e| e.to_string())?;
        let caller = Self { root, registry_root: get("root")?, repository_id: get("repositoryId")?, scope_id: get("scopeId")?,
            descriptor: arguments.pointer("/caller/scopeDescriptor").cloned(),
            level: arguments.get("taskGrantLevel").and_then(Value::as_str).map(str::to_owned) };
        if arguments.get("repository").and_then(Value::as_str) != Some(caller.repository_id.as_str()) {
            return Err("ledger_repository_binding_denied".into());
        }
        Ok(caller)
    }
    /// Native federation has already canonicalized the root. Its repository
    /// identifier still must match, and the source grant must exist in the
    /// installation registry; a request string is not source enrollment.
    pub(crate) fn enrolled(root: &Path, repository: &str) -> Result<Self, String> {
        let canonical = resolve::normalized_root(root).map_err(|e| e.to_string())?;
        if membrane_federation::root::canonical_repository_id(root) != repository {
            return Err("ledger_repository_binding_denied".into());
        }
        let registry = authorization::load_installation_registry().map_err(|e| e.to_string())?;
        let binding = registry.bindings().iter().find(|binding| {
            resolve::normalized_root(Path::new(&binding.root)).ok().as_deref() == Some(&canonical)
        }).ok_or("ledger_root_not_enrolled")?;
        Ok(Self { root: canonical, registry_root: binding.root.clone(), repository_id: binding.repository_id.clone(),
            scope_id: binding.scope_id.clone(), descriptor: binding.scope_descriptor.clone(), level: None })
    }
    pub(crate) fn authorize(&self, action: &str) -> Result<(), String> {
        authorization::authorize(&AuthorizationRequest {
            caller_root: &self.registry_root, caller_repository_id: &self.repository_id,
            caller_scope_id: &self.scope_id, caller_scope_descriptor: self.descriptor.as_ref(),
            target_repository: &self.repository_id, task_grant_level: self.level.as_deref(), action,
        }).map(|_| ()).map_err(|e| format!("{}:{}", e.code(), e))
    }
    pub(crate) fn envelope(&self) -> Value {
        let mut value = json!({"root":self.registry_root,"repositoryId":self.repository_id,"scopeId":self.scope_id});
        if let Some(descriptor) = &self.descriptor { value["scopeDescriptor"] = descriptor.clone(); }
        value
    }
    fn digest(&self) -> String {
        resolve::digest(format!("{}\0{}\0{}", self.root, self.repository_id, self.scope_id).as_bytes())
    }
}

pub(crate) struct LedgerService {
    db: LedgerDb,
    catalog: crate::catalog::ContextCatalog,
    operation: Mutex<()>,
}
struct ResetProgress<'a>(&'a LedgerDb);
impl Drop for ResetProgress<'_> {
    fn drop(&mut self) { self.0.lock().progress_handler(0, None::<fn() -> bool>); }
}
impl LedgerService {
    fn new(db: LedgerDb) -> Result<Self, String> {
        let catalog = crate::catalog::ContextCatalog::open(crate::catalog::default_catalog_path().map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
        Self::with_catalog(db,catalog)
    }
    fn with_catalog(db:LedgerDb,catalog:crate::catalog::ContextCatalog)->Result<Self,String> {
        db.lock().execute_batch(super::diagnostics::SCHEMA).map_err(|e|e.to_string())?;
        if index::recall_mode(&db)? == index::LedgerRecallMode::LedgerFts {
            index::activate(&db,index::LedgerRecallMode::Shadow,None)?;
        }
        db.lock().execute_batch(OWNER_SCHEMA).map_err(|e| e.to_string())?;
        Ok(Self { db, catalog, operation: Mutex::new(()) })
    }
    #[cfg(test)]
    pub(crate) fn in_memory() -> Self { Self::with_catalog(LedgerDb::open_in_memory(),crate::catalog::ContextCatalog::open_in_memory()).unwrap() }

    fn run<T>(&self, caller: &Caller, action: &str, budget: &WorkBudget,
        work: impl FnOnce(&LedgerDb) -> Result<T, String>) -> Result<T, String>
    {
        caller.authorize(action)?;
        let _operation = loop {
            budget.check()?;
            match self.operation.try_lock() {
                Ok(guard) => break guard,
                Err(TryLockError::Poisoned(_)) => return Err("ledger_owner_unavailable".into()),
                Err(TryLockError::WouldBlock) => std::thread::sleep(Duration::from_millis(1)),
            }
        };
        let observed = budget.clone();
        self.db.lock().progress_handler(1000, Some(move || observed.interrupted()));
        let _reset = ResetProgress(&self.db);
        super::erasure::synchronize(&self.catalog,&self.db,&caller.root,budget)?;
        let result = work(&self.db);
        budget.check()?;
        // Revocation is checked again before anything leaves the owner.
        caller.authorize(action)?;
        result
    }

    fn sync_locked(db: &LedgerDb, caller: &Caller, budget: &WorkBudget) -> Result<doc_spine::DocSyncReport, String> {
        let report = doc_spine::sync_bounded(db, Path::new(&caller.root), budget)?;
        db.lock().execute("INSERT INTO ledger_owner_roots VALUES (?1,?2,?3,?4)
            ON CONFLICT(repository_root) DO UPDATE SET generation=excluded.generation,
            policy_digest=excluded.policy_digest,synced_at_ms=excluded.synced_at_ms",
            params![caller.root, report.index_generation, report.policy_digest, crate::time::now_millis() as i64])
            .map_err(|e| e.to_string())?;
        Ok(report)
    }

    pub(crate) fn search(&self, caller: &Caller, task: &str, k: usize, literal: bool,
        ranges: Option<Vec<ReadPathV1>>, grant_id: Option<&str>, budget: &WorkBudget)
        -> Result<(query::QueryResult, Vec<String>), String>
    {
        self.run(caller, "context", budget, |db| {
            validate_task_grant(grant_id, caller, None, None)?;
            // Repository enrollment is the authority for discovering sources.
            // An explicit range grant remains a narrowing restriction.
            Self::sync_locked(db, caller, budget)?;
            let result = query::search(db, &query::QueryScope { root: caller.root.clone(), ranges },
                task, k, literal, budget)?;
            let mut tickets = Vec::new();
            for hit in &result.hits {
                budget.check()?;
                tickets.push(issue_ticket(db, caller, hit, grant_id)?);
            }
            validate_task_grant(grant_id, caller, None, None)?;
            Ok((result, tickets))
        })
    }

    pub(crate) fn operation(&self, arguments: &Value, budget: &WorkBudget) -> Result<Value, String> {
        let caller = Caller::from_arguments(arguments)?;
        let operation = arguments.get("operation").and_then(Value::as_str).ok_or("ledger_operation_required")?;
        match operation {
            "recall" | "literal" => {
                let query = arguments.get("query").and_then(Value::as_str).ok_or("ledger_query_required")?;
                let k = arguments.get("k").and_then(Value::as_u64).unwrap_or(6) as usize;
                let grant_id = arguments.get("scopeGrantId").and_then(Value::as_str);
                let ranges = if let Some(grant_id) = grant_id {
                    let task_id = arguments.get("taskId").and_then(Value::as_str)
                        .filter(|value| !value.trim().is_empty())
                        .ok_or("ledger_task_id_required")?;
                    validate_task_grant(Some(grant_id), &caller, Some(task_id), Some(&caller.scope_id))?;
                    let grant = crate::catalog::lookup_grant(&self.catalog, grant_id)
                        .map_err(|e|e.to_string())?.ok_or("ledger_scope_grant_missing")?;
                    if grant.read_paths.is_empty() { return Err("ledger_scope_ranges_unavailable".into()); }
                    Some(grant.read_paths)
                } else { None };
                let (result, tickets) = self.search(&caller, query, k, operation == "literal", ranges, grant_id, budget)?;
                let mut value = serde_json::to_value(result).map_err(|e| e.to_string())?;
                if let Some(hits) = value.get_mut("hits").and_then(Value::as_array_mut) {
                    for (hit, ticket) in hits.iter_mut().zip(tickets) { hit["ledgerTicket"] = json!(ticket); }
                }
                Ok(value)
            }
            "sync" => self.run(&caller, "context", budget, |db| {
                serde_json::to_value(Self::sync_locked(db, &caller, budget)?).map_err(|e| e.to_string())
            }),
            "status" => self.run(&caller, "system_status", budget, |db| {
                let (active, total): (i64, i64) = db.lock().query_row(
                    "SELECT COALESCE(SUM(lifecycle_state='active' AND sensitivity='normal'),0),COUNT(*)
                     FROM ledger_doc_artifacts WHERE repository_root=?1", [&caller.root], |r| Ok((r.get(0)?,r.get(1)?)))
                    .map_err(|e| e.to_string())?;
                let state = db.lock().query_row("SELECT generation,policy_digest,synced_at_ms FROM ledger_owner_roots WHERE repository_root=?1",
                    [&caller.root], |r| Ok(json!({"generation":r.get::<_,i64>(0)?,"policyDigest":r.get::<_,String>(1)?,"syncedAtMs":r.get::<_,i64>(2)?})))
                    .optional().map_err(|e| e.to_string())?;
                Ok(json!({"schemaVersion":1,"serviceVersion":SERVICE_VERSION,"resolverVersion":RESOLVER_VERSION,
                    "owner":"tray-daemon","repositoryId":caller.repository_id,"enrolled":true,
                    "indexState":if state.is_some(){"published"}else{"not_indexed"},"publication":state,
                    "activeDocuments":active,"registeredDocuments":total,"mode":index::recall_mode(db)?.storage_name(),
                    "providerDelivery":"shadow_unqualified","runtimeQualified":false,
                    "literalMatch":"source_bytes","cursorSupported":true,"sourceByteLimit":resolve::MAX_SOURCE_BYTES}))
            }),
            "outline" => self.run(&caller, "source_read", budget, |db| {
                let path = arguments.get("path").and_then(Value::as_str).ok_or("ledger_path_required")?;
                permitted_path(db, &caller.root, path, budget)?;
                let bytes = resolve::confined_bytes(Path::new(&caller.root), path).map_err(|e| e.to_string())?;
                budget.charge_bytes(bytes.len())?;
                let markdown = String::from_utf8(bytes).map_err(|_| "ledger_unsupported_encoding")?;
                let page = arguments.get("maxSections").and_then(Value::as_u64).unwrap_or(128).clamp(1,256) as usize;
                let outline = super::outline::build_outline_page(&format!("doc://repo/worktree/{path}"), &markdown,
                    "comrak-0.54.0", page, arguments.get("continuationCursor").and_then(Value::as_str)).map_err(|e| e.to_string())?;
                permitted_path(db, &caller.root, path, budget)?;
                serde_json::to_value(outline).map_err(|e| e.to_string())
            }),
            "activate" => self.run(&caller, "checkpoint", budget, |db| {
                let mode = match arguments.get("mode").and_then(Value::as_str) {
                    Some("legacy_scan") => index::LedgerRecallMode::LegacyScan,
                    Some("shadow") => index::LedgerRecallMode::Shadow,
                    // Existing allowlisted evidence qualifies the old query path,
                    // not the newly scoped owner/resolver composition.
                    Some("ledger_fts") => return Err("ledger_owner_qualification_required".into()),
                    _ => return Err("ledger_mode_invalid".into()),
                };
                index::activate(db, mode, None)?;
                Ok(json!({"mode":mode.storage_name(),"providerDelivery":"shadow_unqualified"}))
            }),
            "erase" => self.run(&caller, "checkpoint", budget, |db| erase(db, &self.catalog, &caller, arguments)),
            "backlinks" | "related" | "manifests" | "drift" => self.run(&caller, "context", budget, |db| {
                let doc = required_string(arguments,"docId")?;
                match operation {
                    "backlinks" => super::diagnostics::backlinks(db,&caller.root,&doc,arguments.get("nodeId").and_then(Value::as_str),
                        arguments.get("limit").and_then(Value::as_u64).unwrap_or(64) as usize,budget),
                    "related" => super::diagnostics::related(db,&caller.root,&doc,&required_string(arguments,"nodeId")?,
                        arguments.get("limit").and_then(Value::as_u64).unwrap_or(64) as usize,budget),
                    "manifests" => super::diagnostics::manifests(db,&caller.root,&doc,budget),
                    _ => super::diagnostics::drift(db,&caller.root,&doc,&required_string(arguments,"fromManifest")?,&required_string(arguments,"toManifest")?,budget),
                }
            }),
            _ => Err("ledger_operation_unsupported".into()),
        }
    }

    pub(crate) fn read(&self, arguments: &Value, budget: &WorkBudget) -> Result<Value, String> {
        let caller = Caller::from_arguments(arguments)?;
        let request = ResolveRequest {
            doc_id: optional_string(arguments,"docId"), node_id: optional_string(arguments,"nodeId"),
            source_ref: required_string(arguments,"sourceRef")?, anchor_id: required_string(arguments,"anchorId")?,
            expected_content_hash: required_string(arguments,"expectedContentHash")?,
            expected_revision: optional_string(arguments,"expectedRevision"), expected_span_hash: optional_string(arguments,"expectedSpanHash"),
            ledger_generation: arguments.get("ledgerGeneration").and_then(Value::as_i64),
            continuation_cursor: optional_string(arguments,"continuationCursor"),
            max_bytes: arguments.get("maxBytes").and_then(Value::as_u64).unwrap_or(12_000).min(12_000) as usize,
        };
        self.run(&caller, "source_read", budget, |db| {
            let ticket = arguments.get("ledgerTicket").and_then(Value::as_str);
            if request.node_id.is_some() || request.anchor_id.starts_with("ledger.node:") || request.source_ref.starts_with("ledger://") {
                validate_ticket(db, &caller, ticket.ok_or("ledger_ticket_required")?, &request)?;
            }
            if let Some(doc)=request.doc_id.as_deref().or_else(||request.source_ref.strip_prefix("ledger://doc/")) {
                let path:String=db.lock().query_row("SELECT path FROM ledger_doc_artifacts WHERE repository_root=?1 AND doc_id=?2",
                    params![caller.root,doc],|r|r.get(0)).map_err(|_|"ledger_source_missing")?;
                permitted_path(db,&caller.root,&path,budget)?;
            }
            if let Ok(reference) = super::identifier::WorktreeDocRef::parse(&request.source_ref) {
                permitted_path(db, &caller.root, reference.relative_path(), budget)?;
            }
            let result = match resolve::resolve(db, Path::new(&caller.root), &request) {
                Ok(result) => {
                    budget.charge_bytes(result.read.content.len())?;
                    json!({"ok":true,"contentSha256":result.projection_content_hash,"sourceRef":request.source_ref,
                        "section":result.read,"rawContentHash":result.raw_content_hash,"sourceKind":result.source_kind,
                        "sourceRevision":result.source_revision,"ledgerGeneration":result.ledger_generation,
                        "docId":result.doc_id,"nodeId":result.node_id,"converter":result.converter,
                        "losses":result.losses,"omissions":result.omissions,"resolverVersion":RESOLVER_VERSION})
                }
                Err(resolve::ResolveError::Missing) if ticket.is_none() && request.doc_id.is_none() && request.node_id.is_none()
                    && !request.anchor_id.starts_with("ledger.node:") => {
                    // Preserve the authorized known-section reader without
                    // silently registering sources or bypassing erasure fences.
                    let reference = super::identifier::WorktreeDocRef::parse(&request.source_ref).map_err(|_| "ledger_denied")?;
                    let bytes = resolve::confined_bytes(Path::new(&caller.root), reference.relative_path()).map_err(|e| e.to_string())?;
                    budget.charge_bytes(bytes.len())?;
                    let markdown = String::from_utf8(bytes).map_err(|_| "ledger_unsupported_encoding")?;
                    let expected = request.expected_content_hash.strip_prefix("sha256:").unwrap_or(&request.expected_content_hash);
                    let read = super::outline::read_section_with_cursor(&request.source_ref,&markdown,&request.anchor_id,
                        expected,request.max_bytes,request.continuation_cursor.as_deref()).map_err(|e| e.to_string())?;
                    json!({"ok":true,"contentSha256":read.content_hash,"section":read,"sourceRef":request.source_ref,
                        "sourceKind":"worktree","registered":false,"resolverVersion":RESOLVER_VERSION})
                }
                Err(error) => return Err(error.to_string()),
            };
            if let Some(ticket) = ticket { validate_ticket(db, &caller, ticket, &request)?; }
            if let Some(doc)=request.doc_id.as_deref().or_else(||request.source_ref.strip_prefix("ledger://doc/")) {
                let path:String=db.lock().query_row("SELECT path FROM ledger_doc_artifacts WHERE repository_root=?1 AND doc_id=?2",
                    params![caller.root,doc],|r|r.get(0)).map_err(|_|"ledger_source_missing")?;
                permitted_path(db,&caller.root,&path,budget)?;
            }
            if let Ok(reference) = super::identifier::WorktreeDocRef::parse(&request.source_ref) {
                permitted_path(db, &caller.root, reference.relative_path(), budget)?;
            }
            Ok(result)
        })
    }
}

fn required_string(value: &Value, field: &str) -> Result<String, String> {
    optional_string(value,field).filter(|s| !s.trim().is_empty()).ok_or_else(|| format!("ledger_{field}_required"))
}
fn optional_string(value: &Value, field: &str) -> Option<String> { value.get(field).and_then(Value::as_str).map(str::to_owned) }

pub(crate) fn validate_task_grant(id: Option<&str>, caller: &Caller, task: Option<&str>, session: Option<&str>) -> Result<(), String> {
    let Some(id) = id else { return Ok(()); };
    let catalog = crate::catalog::ContextCatalog::open(crate::catalog::default_catalog_path().map_err(|e|e.to_string())?)
        .map_err(|e| e.to_string())?;
    let grant = crate::catalog::lookup_grant(&catalog,id).map_err(|e| e.to_string())?.ok_or("ledger_scope_grant_missing")?;
    let canonical = membrane_federation::root::canonical_repository_id(Path::new(&caller.root));
    if !grant.permits() || !grant.repository_ids.iter().any(|id| id == &caller.repository_id || id == &canonical)
        || !grant.permitted_edge_types.iter().any(|edge| edge == "source_read")
        || task.is_some_and(|task| grant.task_id != task) || session.is_some_and(|session| grant.session_id != session)
    { return Err("ledger_scope_grant_invalid".into()); }
    Ok(())
}

pub(crate) fn permitted_path(db: &LedgerDb, root: &str, path: &str, budget: &WorkBudget) -> Result<(), String> {
    let erased: bool = db.lock().query_row("SELECT EXISTS(SELECT 1 FROM ledger_erasure_fences WHERE repository_root=?1 AND path_digest=?2)",
        params![root,resolve::digest(path.as_bytes())],|r|r.get(0)).map_err(|e| e.to_string())?;
    if erased { return Err("ledger_source_erased".into()); }
    let mut policy = SourcePolicy::new(Path::new(root))?;
    if !policy.allows(path,false,budget)? { return Err("ledger_source_ineligible".into()); }
    policy.revalidate(budget)
}

fn issue_ticket(db: &LedgerDb, caller: &Caller, hit: &query::LedgerHit, grant_id: Option<&str>) -> Result<String, String> {
    let mut random = [0u8;32];
    getrandom::fill(&mut random).map_err(|_| "ledger_ticket_entropy_unavailable")?;
    let ticket = format!("ledger-ticket:{}",hex::encode(random));
    let now = crate::time::now_millis() as i64;
    let conn = db.lock();
    conn.execute("DELETE FROM ledger_resolution_tickets WHERE expires_at_ms<=?1",[now]).map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM ledger_resolution_tickets WHERE ticket_hash IN (
        SELECT ticket_hash FROM ledger_resolution_tickets WHERE repository_root=?1 ORDER BY expires_at_ms DESC LIMIT -1 OFFSET 480)",
        [&caller.root]).map_err(|e|e.to_string())?;
    conn.execute("INSERT INTO ledger_resolution_tickets VALUES (?1,?2,?3,?4,?5,?6,?7)",
        params![resolve::digest(ticket.as_bytes()),caller.root,caller.digest(),hit.doc_id,
            serde_json::to_string(&hit.resolve_request()).map_err(|e|e.to_string())?,grant_id,now+TICKET_TTL_MS])
        .map_err(|e| e.to_string())?;
    Ok(ticket)
}
fn validate_ticket(db: &LedgerDb, caller: &Caller, ticket: &str, request: &ResolveRequest) -> Result<(), String> {
    if ticket.len() != 78 || !ticket.starts_with("ledger-ticket:") { return Err("ledger_ticket_invalid".into()); }
    let (request_json, grant): (String,Option<String>) = db.lock().query_row(
        "SELECT request_json,grant_id FROM ledger_resolution_tickets WHERE ticket_hash=?1 AND repository_root=?2
         AND caller_digest=?3 AND expires_at_ms>?4",params![resolve::digest(ticket.as_bytes()),caller.root,caller.digest(),crate::time::now_millis() as i64],
        |r| Ok((r.get(0)?,r.get(1)?))).optional().map_err(|e|e.to_string())?.ok_or("ledger_ticket_expired_or_denied")?;
    let expected: ResolveRequest = serde_json::from_str(&request_json).map_err(|_| "ledger_ticket_invalid")?;
    if expected.doc_id != request.doc_id || expected.node_id != request.node_id
        || expected.source_ref != request.source_ref || expected.anchor_id != request.anchor_id
        || expected.expected_content_hash != request.expected_content_hash
        || expected.expected_revision != request.expected_revision || expected.expected_span_hash != request.expected_span_hash
        || expected.ledger_generation != request.ledger_generation
    { return Err("ledger_ticket_binding_mismatch".into()); }
    validate_task_grant(grant.as_deref(),caller,None,Some(&caller.scope_id))
}

fn erase(db: &LedgerDb, catalog: &crate::catalog::ContextCatalog, caller: &Caller, arguments: &Value) -> Result<Value, String> {
    let doc_id = required_string(arguments,"docId")?;
    let expected = required_string(arguments,"expectedContentHash")?;
    let mut conn = db.lock();
    let tx = conn.transaction().map_err(|e|e.to_string())?;
    let (path, hash): (String,String) = tx.query_row("SELECT path,content_hash FROM ledger_doc_artifacts WHERE repository_root=?1 AND doc_id=?2",
        params![caller.root,doc_id],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(|e|e.to_string())?.ok_or("ledger_source_missing")?;
    if !resolve::hash_matches(&expected,&hash) { return Err("ledger_source_stale".into()); }
    let identities:i64=tx.query_row("SELECT COUNT(*) FROM ledger_doc_artifacts WHERE doc_id=?1",[&doc_id],|r|r.get(0)).map_err(|e|e.to_string())?;
    if identities!=1 {return Err("ledger_ambiguous_source_identity".into());}
    super::erasure::record(catalog,&caller.root,&resolve::digest(path.as_bytes()))?;
    tx.execute("INSERT OR REPLACE INTO ledger_erasure_fences VALUES (?1,?2,?3)",
        params![caller.root,resolve::digest(path.as_bytes()),crate::time::now_millis() as i64]).map_err(|e|e.to_string())?;
    for table in ["ledger_node_fts","ledger_nodes","ledger_index_publications","ledger_query_alias_evidence",
        "ledger_query_aliases","ledger_document_conversions","ledger_resolution_tickets","ledger_document_manifests"] {
        tx.execute(&format!("DELETE FROM {table} WHERE doc_id=?1"),[&doc_id]).map_err(|e|e.to_string())?;
    }
    tx.execute("DELETE FROM ledger_doc_projections WHERE parent_doc_id=?1",[&doc_id]).map_err(|e|e.to_string())?;
    tx.execute("DELETE FROM ledger_link_targets WHERE source_doc_id=?1 OR target_doc_id=?1",[&doc_id]).map_err(|e|e.to_string())?;
    tx.execute("DELETE FROM ledger_doc_artifacts WHERE repository_root=?1 AND doc_id=?2",params![caller.root,doc_id]).map_err(|e|e.to_string())?;
    tx.commit().map_err(|e|e.to_string())?;
    Ok(json!({"schemaVersion":1,"operation":"erase","docId":doc_id,"logicalProjectionErasure":true,
        "sourceFilesChanged":false,"physicalErasure":"not_claimed","automaticReindexFenced":true}))
}
