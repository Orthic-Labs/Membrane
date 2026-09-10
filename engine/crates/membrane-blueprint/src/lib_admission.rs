//! Native Rust port of `blueprint/src/lib/admission.mjs`.
//!
//! Lane LIB4 (r5 closure): this module has no prior native equivalent.
//! Blueprint admission — decision-only library. Operations: recall /
//! expand / status / revoke. Returns a neutral decision contract Cortex
//! (or any host) can consume. Does NOT install hooks, classify shell, or
//! fail-closed on tool use.
//!
//! `recall`/`expand`/`status` depend on three functions the legacy JS
//! module injects via `options` and otherwise lazily imports from
//! `../graph/static-provider.mjs` (`readGeneration`,
//! `createContextCandidateSet`, `graphStatus`) — none of which are in this
//! lane's 11-file scope, and whose backing schema lives in
//! `store.rs`/`graph.rs`, which lane rules forbid touching. This port
//! mirrors the JS module's own dependency-injection shape exactly:
//! [`AdmissionDeps`] takes the three functions as trait objects (same
//! injection point the JS `options.readGeneration` /
//! `options.createContextCandidateSet` / `options.graphStatus` occupy), so
//! `recall`/`expand`/`status`/`revoke` are fully ported and testable with
//! fake deps today; a caller that later ports `static-provider.mjs` can
//! wire real deps in without touching this file. Generation/candidate-set/
//! status payloads stay `serde_json::Value` throughout, matching the
//! legacy module's own untyped duck-typing over those external shapes.

use crate::lib_orientation_evidence::{
    build_orientation_evidence, candidate_set_digest, default_evidence_path,
    write_orientation_evidence_file,
};
use crate::lib_receipt_store::{
    build_orientation_receipt, receipt_lookup_key, OrientationReceiptFields, ReceiptLookupKeyInput,
    ReceiptStore,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

pub const ADMISSION_SCHEMA_VERSION: u32 = 1;
pub const DECISION_ACTIONS: [&str; 4] = ["allow", "continue", "block", "noop"];
const DEFAULT_OPERATIONS: [&str; 4] = ["read", "search", "test", "edit"];

// ---------------------------------------------------------------------
// decision() / claimBoundaryFor() and small pure helpers
// ---------------------------------------------------------------------

/// Partial fields for [`decision`], mirroring the JS `partial` object. All
/// fields are optional, matching the JS `partial.x ?? default` pattern.
#[derive(Debug, Clone, Default)]
pub struct DecisionInput {
    pub action: Option<String>,
    pub reason: Option<String>,
    pub reason_code: Option<Value>,
    pub receipt_id: Option<Value>,
    pub candidate_set: Option<Value>,
    pub allowed_scopes: Vec<String>,
    pub omissions: Vec<Value>,
    pub claim_boundary: Option<Value>,
    pub next_action: Option<Value>,
    pub evidence: Option<Value>,
    pub evidence_path: Option<Value>,
    pub receipt: Option<Value>,
}
#[derive(Debug)]
pub struct AdmissionError(pub String);
impl std::fmt::Display for AdmissionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl std::error::Error for AdmissionError {}

/// Build a neutral admission decision. Mirrors `decision`. Errors (instead
/// of panicking) on an invalid action, matching the JS `throw`.
pub fn decision(partial: DecisionInput) -> Result<Value, AdmissionError> {
    let action = partial.action.unwrap_or_else(|| "noop".to_string());
    if !DECISION_ACTIONS.contains(&action.as_str()) {
        return Err(AdmissionError(format!("invalid admission action: {}", action)));
    }
    let claim_boundary = partial.claim_boundary.unwrap_or_else(generic_claim_boundary);
    Ok(json!({
        "schemaVersion": ADMISSION_SCHEMA_VERSION,
        "action": action,
        "reason": partial.reason.unwrap_or_default(),
        "reasonCode": partial.reason_code.unwrap_or(Value::Null),
        "receiptId": partial.receipt_id.unwrap_or(Value::Null),
        "candidateSet": partial.candidate_set.unwrap_or(Value::Null),
        "allowedScopes": partial.allowed_scopes,
        "omissions": partial.omissions,
        "claimBoundary": claim_boundary,
        "nextAction": partial.next_action.unwrap_or(Value::Null),
        "evidence": partial.evidence.unwrap_or(Value::Null),
        "evidencePath": partial.evidence_path.unwrap_or(Value::Null),
        "receipt": partial.receipt.unwrap_or(Value::Null),
    }))
}

fn generic_claim_boundary() -> Value {
    claim_boundary_for(ClaimBoundaryInput {
        permit_clean: false,
        state: "missing".to_string(),
        generation_id: None,
        omissions: vec![],
    })
}

/// Input for [`claim_boundary_for`], mirroring the JS destructured
/// parameter object.
#[derive(Debug, Clone)]
pub struct ClaimBoundaryInput {
    pub permit_clean: bool,
    pub state: String,
    pub generation_id: Option<String>,
    pub omissions: Vec<Value>,
}

/// Build a claim-boundary record. Mirrors `claimBoundaryFor`.
pub fn claim_boundary_for(input: ClaimBoundaryInput) -> Value {
    let restricted = !input.permit_clean;
    let fresh = input.state == "fresh";
    let claim_restricted = restricted || !fresh;
    let gaps: Vec<String> = input
        .omissions
        .iter()
        .filter_map(|o| o.get("reason").and_then(Value::as_str))
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect();
    let safe_claims: Vec<String> = if fresh {
        vec!["Results reflect the current sealed generation.".to_string()]
    } else if let Some(gid) = &input.generation_id {
        vec![format!(
            "Results reflect generation {}, which predates current worktree changes.",
            gid
        )]
    } else {
        vec!["No sealed graph generation is available to ground claims.".to_string()]
    };
    json!({
        "status": if claim_restricted { "restricted" } else { "clear" },
        "cleanClaimAllowed": !claim_restricted,
        "safeClaims": safe_claims,
        "prohibitedClaims": if claim_restricted { vec!["Graph-derived facts are current.".to_string()] } else { vec![] },
        "gaps": gaps,
    })
}

fn normalize_repo_path(value: &str) -> String {
    let value = value.replace('\\', "/");
    let value = value.strip_prefix("./").unwrap_or(&value);
    value.trim_end_matches('/').to_string()
}

fn is_absolute_input(value: &str) -> bool {
    Path::new(value).is_absolute()
        || (value.len() >= 2
            && value.as_bytes()[0].is_ascii_alphabetic()
            && value.as_bytes()[1] == b':'
            && value.len() > 2
            && (value.as_bytes()[2] == b'\\' || value.as_bytes()[2] == b'/'))
}

fn directory_scope(file_path: &str) -> String {
    let normalized = normalize_repo_path(file_path);
    match Path::new(&normalized).parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_string_lossy().replace('\\', "/"),
        _ => String::new(),
    }
}

/// Extract sorted, de-duplicated repo-relative paths from a candidate
/// set's `candidates[].sourceRef` fields. Mirrors `pathsFromCandidateSet`.
pub fn paths_from_candidate_set(candidate_set: &Value) -> Vec<String> {
    let mut paths: BTreeSet<String> = BTreeSet::new();
    if let Some(candidates) = candidate_set.get("candidates").and_then(Value::as_array) {
        for candidate in candidates {
            let ref_str = candidate.get("sourceRef").and_then(Value::as_str).unwrap_or("");
            let path = match ref_str.rfind(':') {
                Some(idx) => &ref_str[..idx],
                None => ref_str,
            };
            let normalized = normalize_repo_path(path);
            if !normalized.is_empty() {
                paths.insert(normalized);
            }
        }
    }
    paths.into_iter().collect()
}

/// Extract sorted, de-duplicated directory scopes from a path list.
/// Mirrors `scopesFromPaths`.
pub fn scopes_from_paths(paths: &[String]) -> Vec<String> {
    let mut scopes: BTreeSet<String> = BTreeSet::new();
    for path in paths {
        let dir = directory_scope(path);
        if !dir.is_empty() {
            scopes.insert(dir);
        }
    }
    scopes.into_iter().collect()
}

fn turn_digest(task: &str, session_id: &str) -> String {
    let payload = format!("{}\n{}", session_id, task);
    format!("sha256:{}", hex::encode(Sha256::digest(payload.as_bytes())))
}

fn repo_identity_from_root(repo_root: &Path) -> String {
    let root = if repo_root.is_absolute() {
        repo_root.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(repo_root)
    };
    let base = root
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();
    format!("{}@{}", base, root.to_string_lossy())
}

fn now_iso8601() -> String {
    let dur = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();
    let millis = dur.subsec_millis();
    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let (h, m, s) = (time_of_day / 3600, (time_of_day % 3600) / 60, time_of_day % 60);
    let z = days as i64 + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mth = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if mth <= 2 { y + 1 } else { y };
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z", year, mth, d, h, m, s, millis)
}

// ---------------------------------------------------------------------
// Injected dependencies (mirrors options.readGeneration /
// options.createContextCandidateSet / options.graphStatus)
// ---------------------------------------------------------------------

/// Injected dependencies for [`Admission`], mirroring the three
/// `../graph/static-provider.mjs` functions the JS module lazily imports
/// (or accepts as `options` overrides). This port requires callers to
/// supply them explicitly (no lazy default import), since
/// `static-provider.mjs` is out of this lane's scope.
pub struct AdmissionDeps<'a> {
    /// `(repo_root, out_dir) -> Option<generation>`. Mirrors `readGeneration`.
    pub read_generation: Box<dyn Fn(&Path, &str) -> Option<Value> + 'a>,
    /// `(generation, options) -> candidateSet`. Mirrors `createContextCandidateSet`.
    pub create_context_candidate_set: Box<dyn Fn(&Value, &Value) -> Value + 'a>,
    /// `(repo_root, out_dir, options) -> status`. Mirrors `graphStatus`.
    pub graph_status: Box<dyn Fn(&Path, &str, &Value) -> Value + 'a>,
}

/// An admission API bound to a receipt store and injected graph deps.
/// Mirrors the object returned by `createAdmission`.
pub struct Admission<'a> {
    pub store: ReceiptStore,
    pub out_dir: String,
    pub evidence_dir: Option<PathBuf>,
    pub deps: AdmissionDeps<'a>,
}

fn attach_evidence(evidence_dir: &Option<PathBuf>, receipt: &Value) -> (Value, Value) {
    if receipt.is_null() {
        return (Value::Null, Value::Null);
    }
    let evidence = build_orientation_evidence(receipt, None).unwrap_or(Value::Null);
    let mut evidence_path = Value::Null;
    if let Some(dir) = evidence_dir {
        if let Some(receipt_id) = receipt.get("receiptId").and_then(Value::as_str) {
            let target = default_evidence_path(dir, receipt_id);
            if let Ok((written, _)) = write_orientation_evidence_file(receipt, &target, None) {
                evidence_path = json!(written.to_string_lossy());
            }
        }
    }
    (evidence, evidence_path)
}

/// Input for [`Admission::recall`], mirroring the JS `input` object.
#[derive(Debug, Clone, Default)]
pub struct RecallInput {
    pub task: Option<String>,
    pub query: Option<String>,
    pub session_id: Option<String>,
    pub task_id: Option<String>,
    pub repo_root: Option<PathBuf>,
    pub repo_identity: Option<String>,
    pub anchors: Vec<String>,
    pub status_options: Value,
    pub expected_generation: Option<String>,
    pub force: bool,
    pub max_candidates: Option<i64>,
    pub max_estimated_tokens: Option<i64>,
    pub trace_id: Option<String>,
    pub mode: Option<String>,
    pub repo_id: Option<String>,
    pub receipt_id: Option<String>,
    pub freshness_receipt: Option<Value>,
    pub neighborhood: Option<Value>,
    pub turn_digest: Option<String>,
    pub worktree_identity: Option<String>,
    pub allowed_operations: Option<Vec<String>>,
}

impl<'a> Admission<'a> {
    pub fn new(store: ReceiptStore, out_dir: Option<String>, evidence_dir: Option<PathBuf>, deps: AdmissionDeps<'a>) -> Self {
        Self {
            store,
            out_dir: out_dir.unwrap_or_else(|| ".agent".to_string()),
            evidence_dir,
            deps,
        }
    }

    /// Establish task-scoped recall. Decision-only — does not block hosts.
    /// Mirrors `recall`.
    pub fn recall(&self, input: RecallInput) -> Result<Value, AdmissionError> {
        let task = input.task.clone().or_else(|| input.query.clone()).unwrap_or_default();
        let task = task.trim().to_string();
        let session_id = input.session_id.clone().unwrap_or_else(|| "default".to_string());
        let task_id = input.task_id.clone().unwrap_or_else(|| {
            if task.is_empty() { "untasked".to_string() } else { task.clone() }
        });
        let repo_root = input
            .repo_root
            .clone()
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
        let repo_root = if repo_root.is_absolute() { repo_root } else { std::env::current_dir().unwrap_or_default().join(&repo_root) };
        let repo_identity = input
            .repo_identity
            .clone()
            .unwrap_or_else(|| repo_identity_from_root(&repo_root));
        let anchors: Vec<String> = input.anchors.iter().map(|a| normalize_repo_path(a)).collect();

        let status = (self.deps.graph_status)(&repo_root, &self.out_dir, &input.status_options);
        let state = status.get("state").and_then(Value::as_str).unwrap_or("missing");
        if status.is_null() || state == "missing" || state == "incomplete" {
            return decision(DecisionInput {
                action: Some("block".to_string()),
                reason: Some("No complete Blueprint graph generation is available for recall.".to_string()),
                reason_code: Some(json!("missing_graph")),
                next_action: Some(json!(format!("blueprint build --out {}", self.out_dir))),
                omissions: vec![json!({"reason": "missing_graph"})],
                claim_boundary: Some(claim_boundary_for(ClaimBoundaryInput {
                    permit_clean: false,
                    state: state.to_string(),
                    generation_id: None,
                    omissions: vec![json!({"reason": "missing_graph"})],
                })),
                ..Default::default()
            });
        }

        let generation = (self.deps.read_generation)(&repo_root, &self.out_dir);
        let generation_id = generation
            .as_ref()
            .and_then(|g| g.get("manifest"))
            .and_then(|m| m.get("generationId"))
            .and_then(Value::as_str)
            .map(|s| s.to_string());
        let generation = match (&generation, &generation_id) {
            (Some(g), Some(_)) => g.clone(),
            _ => {
                return decision(DecisionInput {
                    action: Some("block".to_string()),
                    reason: Some("Graph store could not load a sealed generation for recall.".to_string()),
                    reason_code: Some(json!("missing_generation")),
                    next_action: Some(json!(format!("blueprint build --out {}", self.out_dir))),
                    claim_boundary: Some(claim_boundary_for(ClaimBoundaryInput {
                        permit_clean: false,
                        state: state.to_string(),
                        generation_id: None,
                        omissions: vec![json!({"reason": "missing_generation"})],
                    })),
                    ..Default::default()
                })
            }
        };
        let generation_id = generation_id.unwrap();
        let manifest_digest = generation
            .get("manifest")
            .and_then(|m| m.get("manifestDigest"))
            .cloned()
            .unwrap_or(Value::Null);

        if let Some(expected) = &input.expected_generation {
            if expected != &generation_id {
                return decision(DecisionInput {
                    action: Some("block".to_string()),
                    reason: Some(format!(
                        "Pinned generation {} does not match live {}.",
                        expected, generation_id
                    )),
                    reason_code: Some(json!("generation_mismatch")),
                    next_action: Some(json!("Re-run recall against the current generation, or rebuild the graph.")),
                    claim_boundary: Some(claim_boundary_for(ClaimBoundaryInput {
                        permit_clean: false,
                        state: state.to_string(),
                        generation_id: Some(generation_id.clone()),
                        omissions: vec![],
                    })),
                    ..Default::default()
                });
            }
        }

        let existing = self.store.find_active(ReceiptLookupKeyInput {
            session_id: Some(&session_id),
            task_id: Some(&task_id),
            repo_identity: Some(&repo_identity),
            generation_id: Some(&generation_id),
        });
        if let Some(existing) = &existing {
            if !input.force {
                let (evidence, evidence_path) = attach_evidence(&self.evidence_dir, existing);
                let omissions = existing.get("omissions").and_then(Value::as_array).cloned().unwrap_or_default();
                return decision(DecisionInput {
                    action: Some("continue".to_string()),
                    reason: Some("Active recall receipt already exists for this session/task/repo/generation.".to_string()),
                    reason_code: Some(json!("receipt_reuse")),
                    receipt_id: existing.get("receiptId").cloned(),
                    allowed_scopes: existing
                        .get("allowedPaths")
                        .and_then(Value::as_array)
                        .map(|a| a.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
                        .unwrap_or_default(),
                    omissions: omissions.clone(),
                    evidence: Some(evidence),
                    evidence_path: Some(evidence_path),
                    receipt: Some(existing.clone()),
                    candidate_set: Some(Value::Null),
                    claim_boundary: Some(claim_boundary_for(ClaimBoundaryInput {
                        permit_clean: false,
                        state: state.to_string(),
                        generation_id: Some(generation_id.clone()),
                        omissions,
                    })),
                    ..Default::default()
                });
            }
        }

        let candidate_options = json!({
            "task": task,
            "query": input.query.clone().unwrap_or_else(|| task.clone()),
            "anchors": anchors,
            "maxCandidates": input.max_candidates,
            "maxEstimatedTokens": input.max_estimated_tokens,
            "traceId": input.trace_id,
            "mode": input.mode.clone().unwrap_or_else(|| "survey".to_string()),
            "repoId": input.repo_id,
            "repoRoot": repo_root.to_string_lossy(),
            "receiptId": input
                .freshness_receipt
                .as_ref()
                .and_then(|r| r.get("receiptId").cloned())
                .or_else(|| input.receipt_id.clone().map(|s| json!(s)))
                .unwrap_or(Value::Null),
        });
        let mut candidate_set = (self.deps.create_context_candidate_set)(&generation, &candidate_options);
        if let Some(fr) = &input.freshness_receipt {
            candidate_set["freshnessReceipt"] = fr.clone();
            candidate_set["receiptId"] = fr.get("receiptId").cloned().unwrap_or(Value::Null);
        }
        if let Some(n) = &input.neighborhood {
            candidate_set["neighborhood"] = n.clone();
        }

        let mut allowed_paths_set: BTreeSet<String> = anchors.iter().cloned().collect();
        for p in paths_from_candidate_set(&candidate_set) {
            allowed_paths_set.insert(p);
        }
        let allowed_paths: Vec<String> = allowed_paths_set.into_iter().collect();
        let allowed_directory_scopes = scopes_from_paths(&allowed_paths);
        let digest = candidate_set_digest(&candidate_set);

        let source_observation_digest = generation.get("sourceObservation").filter(|v| !v.is_null()).map(|so| {
            format!(
                "sha256:{}",
                hex::encode(Sha256::digest(serde_json::to_string(so).unwrap_or_default().as_bytes()))
            )
        });

        let dirty_overlay = status
            .get("capabilities")
            .and_then(|c| c.get("dirtyOverlayFileCount"))
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let omissions_vec = candidate_set.get("omissions").and_then(Value::as_array).cloned().unwrap_or_default();

        let receipt = build_orientation_receipt(OrientationReceiptFields {
            receipt_id: Some(self.store.next_id()),
            session_id: Some(session_id.clone()),
            task_id: Some(task_id.clone()),
            turn_digest: Some(json!(input.turn_digest.clone().unwrap_or_else(|| turn_digest(&task, &session_id)))),
            repo_identity: Some(repo_identity.clone()),
            worktree_identity: Some(json!(input.worktree_identity.clone().unwrap_or_else(|| {
                format!("file://{}", repo_root.to_string_lossy().replace('\\', "/"))
            }))),
            generation_id: Some(json!(generation_id)),
            manifest_digest: Some(manifest_digest),
            source_observation_digest: source_observation_digest.map(|s| json!(s)),
            candidate_set_digest: Some(json!(digest)),
            explicit_anchors: anchors.iter().map(|a| json!(a)).collect(),
            allowed_paths: allowed_paths.iter().map(|a| json!(a)).collect(),
            allowed_directory_scopes: allowed_directory_scopes.iter().map(|a| json!(a)).collect(),
            allowed_operations: Some(
                input
                    .allowed_operations
                    .clone()
                    .unwrap_or_else(|| DEFAULT_OPERATIONS.iter().map(|s| s.to_string()).collect())
                    .into_iter()
                    .map(|s| json!(s))
                    .collect(),
            ),
            overlay_revision: Some(if dirty_overlay > 0 { 1.0 } else { 0.0 }),
            omissions: omissions_vec.clone(),
            ..Default::default()
        });
        self.store.put(receipt.clone()).map_err(|e| AdmissionError(e.to_string()))?;
        let (evidence, evidence_path) = attach_evidence(&self.evidence_dir, &receipt);

        let action = if state == "stale" || state == "indeterminate" { "continue" } else { "allow" };
        decision(DecisionInput {
            action: Some(action.to_string()),
            reason: Some(if action == "allow" {
                "Recall established against the sealed graph generation.".to_string()
            } else {
                format!("Recall established under {} graph state; overlay/freshness is explicit on the receipt.", state)
            }),
            reason_code: Some(json!(if action == "allow" { "recalled".to_string() } else { format!("recalled_{}", state) })),
            receipt_id: receipt.get("receiptId").cloned(),
            candidate_set: Some(candidate_set),
            allowed_scopes: allowed_paths,
            omissions: omissions_vec.clone(),
            next_action: if state == "stale" { Some(json!(format!("blueprint build --out {}", self.out_dir))) } else { Some(Value::Null) },
            evidence: Some(evidence),
            evidence_path: Some(evidence_path),
            receipt: Some(receipt),
            claim_boundary: Some(claim_boundary_for(ClaimBoundaryInput {
                permit_clean: action == "allow",
                state: state.to_string(),
                generation_id: Some(generation_id),
                omissions: omissions_vec,
            })),
        })
    }

    /// Expand an existing receipt with graph-supported paths for a
    /// path/symbol/query. Mirrors `expand`.
    #[allow(clippy::too_many_arguments)]
    pub fn expand(
        &self,
        receipt_id: Option<&str>,
        query: Option<&str>,
        path: Option<&str>,
        symbol: Option<&str>,
        task: Option<&str>,
        paths: &[String],
        repo_root_override: Option<&Path>,
        max_candidates: Option<i64>,
        max_estimated_tokens: Option<i64>,
        mode: Option<&str>,
        repo_id: Option<&str>,
        freshness_receipt: Option<&Value>,
        neighborhood: Option<&Value>,
    ) -> Result<Value, AdmissionError> {
        let receipt_id = match receipt_id {
            Some(r) if !r.is_empty() => r,
            _ => {
                return decision(DecisionInput {
                    action: Some("block".to_string()),
                    reason: Some("expand requires receiptId.".to_string()),
                    reason_code: Some(json!("missing_receipt_id")),
                    next_action: Some(json!("Call recall first, then expand with the returned receiptId.")),
                    ..Default::default()
                })
            }
        };
        let receipt = match self.store.get(receipt_id) {
            Some(r) => r,
            None => {
                return decision(DecisionInput {
                    action: Some("block".to_string()),
                    reason: Some(format!("No receipt found for {}.", receipt_id)),
                    reason_code: Some(json!("receipt_not_found")),
                    next_action: Some(json!("Call recall to establish a receipt.")),
                    ..Default::default()
                })
            }
        };
        if receipt.get("status").and_then(Value::as_str) == Some("revoked") {
            return decision(DecisionInput {
                action: Some("block".to_string()),
                reason: Some("Receipt is revoked; recall again before expanding.".to_string()),
                reason_code: Some(json!("receipt_revoked")),
                receipt_id: Some(json!(receipt_id)),
                next_action: Some(json!("Call recall with force=true or a new session/task.")),
                ..Default::default()
            });
        }

        let query_str = [query, path, symbol, task]
            .into_iter()
            .find_map(|v| v)
            .unwrap_or("")
            .trim()
            .to_string();
        let relative_paths: Vec<String> = paths.to_vec();

        // Models cannot self-approve arbitrary absolute scopes — only
        // relative repo paths / queries.
        for p in &relative_paths {
            if is_absolute_input(p) {
                return decision(DecisionInput {
                    action: Some("block".to_string()),
                    reason: Some("expand rejects absolute self-approved paths; pass a relative path or graph query.".to_string()),
                    reason_code: Some(json!("absolute_path_rejected")),
                    receipt_id: Some(json!(receipt_id)),
                    ..Default::default()
                });
            }
        }
        if let Some(p) = path {
            if is_absolute_input(p) {
                return decision(DecisionInput {
                    action: Some("block".to_string()),
                    reason: Some("expand rejects absolute self-approved paths; pass a relative path or graph query.".to_string()),
                    reason_code: Some(json!("absolute_path_rejected")),
                    receipt_id: Some(json!(receipt_id)),
                    ..Default::default()
                });
            }
        }

        if query_str.is_empty() && relative_paths.is_empty() {
            return decision(DecisionInput {
                action: Some("block".to_string()),
                reason: Some("expand requires a path, symbol, or query grounded in the graph.".to_string()),
                reason_code: Some(json!("missing_expand_query")),
                receipt_id: Some(json!(receipt_id)),
                next_action: Some(json!("Pass path, symbol, or query.")),
                ..Default::default()
            });
        }

        let repo_identity = receipt.get("repoIdentity").and_then(Value::as_str).unwrap_or("");
        let identity_path = repo_identity.find('@').map(|idx| &repo_identity[idx + 1..]).unwrap_or("");
        let repo_root = repo_root_override
            .map(|p| p.to_path_buf())
            .or_else(|| if identity_path.is_empty() { None } else { Some(PathBuf::from(identity_path)) })
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
        let repo_root = if repo_root.is_absolute() { repo_root } else { std::env::current_dir().unwrap_or_default().join(&repo_root) };

        let generation = (self.deps.read_generation)(&repo_root, &self.out_dir);
        let generation_id = generation
            .as_ref()
            .and_then(|g| g.get("manifest"))
            .and_then(|m| m.get("generationId"))
            .and_then(Value::as_str)
            .map(|s| s.to_string());
        let generation = match (&generation, &generation_id) {
            (Some(g), Some(_)) => g.clone(),
            _ => {
                return decision(DecisionInput {
                    action: Some("block".to_string()),
                    reason: Some("Graph generation missing; cannot expand from evidence.".to_string()),
                    reason_code: Some(json!("missing_generation")),
                    receipt_id: Some(json!(receipt_id)),
                    next_action: Some(json!(format!("blueprint build --out {}", self.out_dir))),
                    ..Default::default()
                })
            }
        };
        let generation_id = generation_id.unwrap();
        let receipt_generation_id = receipt.get("generationId").and_then(Value::as_str);
        if let Some(rgid) = receipt_generation_id {
            if !rgid.is_empty() && rgid != generation_id {
                return decision(DecisionInput {
                    action: Some("block".to_string()),
                    reason: Some("Graph generation changed after recall; recall again before expand.".to_string()),
                    reason_code: Some(json!("generation_changed")),
                    receipt_id: Some(json!(receipt_id)),
                    next_action: Some(json!("Call recall against the current generation.")),
                    ..Default::default()
                });
            }
        }

        let expand_query = if !query_str.is_empty() {
            query_str.clone()
        } else {
            relative_paths.first().cloned().unwrap_or_default()
        };

        let receipt_anchors: Vec<String> = receipt
            .get("explicitAnchors")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
            .unwrap_or_default();
        let mut anchors: Vec<String> = receipt_anchors.clone();
        anchors.extend(relative_paths.iter().map(|p| normalize_repo_path(p)));
        if let Some(p) = path {
            anchors.push(normalize_repo_path(p));
        }

        let candidate_options = json!({
            "task": expand_query,
            "query": expand_query,
            "anchors": anchors,
            "maxCandidates": max_candidates,
            "maxEstimatedTokens": max_estimated_tokens,
            "mode": mode.unwrap_or("survey"),
            "repoId": repo_id,
            "repoRoot": repo_root.to_string_lossy(),
            "receiptId": freshness_receipt
                .and_then(|r| r.get("receiptId").cloned())
                .unwrap_or_else(|| json!(receipt_id)),
        });
        let mut candidate_set = (self.deps.create_context_candidate_set)(&generation, &candidate_options);
        if let Some(fr) = freshness_receipt {
            candidate_set["freshnessReceipt"] = fr.clone();
            candidate_set["receiptId"] = fr.get("receiptId").cloned().unwrap_or(Value::Null);
        }
        if let Some(n) = neighborhood {
            candidate_set["neighborhood"] = n.clone();
        }
        let added_paths = paths_from_candidate_set(&candidate_set);

        let existing_allowed: Vec<String> = receipt
            .get("allowedPaths")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
            .unwrap_or_default();
        let mut allowed_set: BTreeSet<String> = existing_allowed.into_iter().collect();
        for a in &anchors {
            allowed_set.insert(a.clone());
        }
        for a in &added_paths {
            allowed_set.insert(a.clone());
        }
        let allowed_paths: Vec<String> = allowed_set.into_iter().collect();

        let mut anchors_set: BTreeSet<String> = receipt_anchors.into_iter().collect();
        for a in &anchors {
            anchors_set.insert(a.clone());
        }
        let merged_anchors: Vec<String> = anchors_set.into_iter().collect();

        let existing_omissions = receipt.get("omissions").and_then(Value::as_array).cloned().unwrap_or_default();
        let new_omissions = candidate_set.get("omissions").and_then(Value::as_array).cloned().unwrap_or_default();
        let mut merged_omissions = existing_omissions;
        merged_omissions.extend(new_omissions.clone());

        let overlay_revision = receipt.get("overlayRevision").and_then(Value::as_f64).unwrap_or(0.0) + 1.0;

        let mut updated = receipt.clone();
        updated["explicitAnchors"] = json!(merged_anchors);
        updated["allowedPaths"] = json!(allowed_paths);
        updated["allowedDirectoryScopes"] = json!(scopes_from_paths(&allowed_paths));
        updated["candidateSetDigest"] = json!(candidate_set_digest(&candidate_set));
        updated["omissions"] = json!(merged_omissions);
        updated["overlayRevision"] = json!(overlay_revision);
        updated["updatedAt"] = json!(now_iso8601());

        self.store.put(updated.clone()).map_err(|e| AdmissionError(e.to_string()))?;
        let (evidence, evidence_path) = attach_evidence(&self.evidence_dir, &updated);

        decision(DecisionInput {
            action: Some("continue".to_string()),
            reason: Some(format!("Expanded recall with {} graph-supported path(s).", added_paths.len())),
            reason_code: Some(json!("expanded")),
            receipt_id: updated.get("receiptId").cloned(),
            candidate_set: Some(candidate_set),
            allowed_scopes: allowed_paths,
            omissions: new_omissions,
            next_action: Some(Value::Null),
            evidence: Some(evidence),
            evidence_path: Some(evidence_path),
            receipt: Some(updated),
            ..Default::default()
        })
    }

    /// Inspect the current receipt / graph recall state. Mirrors `status`.
    pub fn status_lookup(&self, receipt_id: Option<&str>, session_id: Option<&str>, task_id: Option<&str>, repo_identity: Option<&str>, repo_root: Option<&Path>, generation_id: Option<&str>, status_options: &Value) -> Result<Value, AdmissionError> {
        let mut receipt: Option<Value> = None;
        if let Some(rid) = receipt_id {
            receipt = self.store.get(rid);
        } else if session_id.is_some() || task_id.is_some() || repo_identity.is_some() || generation_id.is_some() {
            let repo_identity_resolved = repo_identity
                .map(|s| s.to_string())
                .or_else(|| repo_root.map(repo_identity_from_root));
            receipt = self.store.find_active(ReceiptLookupKeyInput {
                session_id: Some(session_id.unwrap_or("default")),
                task_id: Some(task_id.unwrap_or("")),
                repo_identity: repo_identity_resolved.as_deref(),
                generation_id: Some(generation_id.unwrap_or("")),
            });
            if receipt.is_none() && generation_id.is_none() {
                let needle = receipt_lookup_key(ReceiptLookupKeyInput {
                    session_id: Some(session_id.unwrap_or("default")),
                    task_id: Some(task_id.unwrap_or("")),
                    repo_identity: repo_identity_resolved.as_deref(),
                    generation_id: Some(""),
                });
                let needle = needle.trim_end_matches('\u{001f}').to_string();
                let mut list = self.store.list(true);
                list.reverse();
                receipt = list.into_iter().find(|row| {
                    let key = receipt_lookup_key(ReceiptLookupKeyInput {
                        session_id: row.get("sessionId").and_then(Value::as_str),
                        task_id: row.get("taskId").and_then(Value::as_str),
                        repo_identity: row.get("repoIdentity").and_then(Value::as_str),
                        generation_id: row.get("generationId").and_then(Value::as_str),
                    });
                    key.starts_with(&needle) && row.get("status").and_then(Value::as_str) != Some("revoked")
                });
            }
        }

        let receipt = match receipt {
            Some(r) => r,
            None => {
                return decision(DecisionInput {
                    action: Some("noop".to_string()),
                    reason: Some("No recall receipt found.".to_string()),
                    reason_code: Some(json!("no_receipt")),
                    next_action: Some(json!("Call recall for the active session/task/repo.")),
                    claim_boundary: Some(claim_boundary_for(ClaimBoundaryInput {
                        permit_clean: false,
                        state: "missing".to_string(),
                        generation_id: None,
                        omissions: vec![json!({"reason": "no_receipt"})],
                    })),
                    ..Default::default()
                })
            }
        };

        if receipt.get("status").and_then(Value::as_str) == Some("revoked") {
            return decision(DecisionInput {
                action: Some("block".to_string()),
                reason: Some("Recall receipt is revoked.".to_string()),
                reason_code: Some(json!("receipt_revoked")),
                receipt_id: receipt.get("receiptId").cloned(),
                receipt: Some(receipt.clone()),
                next_action: Some(json!("Call recall to establish a new receipt.")),
                claim_boundary: Some(claim_boundary_for(ClaimBoundaryInput {
                    permit_clean: false,
                    state: "revoked".to_string(),
                    generation_id: receipt.get("generationId").and_then(Value::as_str).map(|s| s.to_string()),
                    omissions: receipt.get("omissions").and_then(Value::as_array).cloned().unwrap_or_default(),
                })),
                ..Default::default()
            });
        }

        let (evidence, evidence_path) = attach_evidence(&self.evidence_dir, &receipt);
        let worktree_identity = receipt.get("worktreeIdentity").and_then(Value::as_str);
        let resolved_root = repo_root
            .map(|p| p.to_path_buf())
            .or_else(|| {
                worktree_identity.and_then(|w| {
                    w.strip_prefix("file://").map(PathBuf::from)
                })
            })
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
        let graph_state = (self.deps.graph_status)(&resolved_root, &self.out_dir, status_options)
            .get("state")
            .and_then(Value::as_str)
            .unwrap_or("missing")
            .to_string();

        decision(DecisionInput {
            action: Some("continue".to_string()),
            reason: Some("Active recall receipt.".to_string()),
            reason_code: Some(json!("receipt_active")),
            receipt_id: receipt.get("receiptId").cloned(),
            allowed_scopes: receipt
                .get("allowedPaths")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
                .unwrap_or_default(),
            omissions: receipt.get("omissions").and_then(Value::as_array).cloned().unwrap_or_default(),
            evidence: Some(evidence),
            evidence_path: Some(evidence_path),
            claim_boundary: Some(claim_boundary_for(ClaimBoundaryInput {
                permit_clean: true,
                state: graph_state,
                generation_id: receipt.get("generationId").and_then(Value::as_str).map(|s| s.to_string()),
                omissions: receipt.get("omissions").and_then(Value::as_array).cloned().unwrap_or_default(),
            })),
            receipt: Some(receipt),
            ..Default::default()
        })
    }

    /// Explicitly invalidate a receipt. Mirrors `revoke`.
    pub fn revoke(&self, receipt_id: Option<&str>, reason: Option<&str>) -> Result<Value, AdmissionError> {
        let receipt_id = match receipt_id {
            Some(r) if !r.is_empty() => r,
            _ => {
                return decision(DecisionInput {
                    action: Some("block".to_string()),
                    reason: Some("revoke requires receiptId.".to_string()),
                    reason_code: Some(json!("missing_receipt_id")),
                    ..Default::default()
                })
            }
        };
        let existing = match self.store.get(receipt_id) {
            Some(e) => e,
            None => {
                return decision(DecisionInput {
                    action: Some("noop".to_string()),
                    reason: Some(format!("No receipt found for {}.", receipt_id)),
                    reason_code: Some(json!("receipt_not_found")),
                    receipt_id: Some(json!(receipt_id)),
                    ..Default::default()
                })
            }
        };
        if existing.get("status").and_then(Value::as_str) == Some("revoked") {
            let (evidence, _) = attach_evidence(&self.evidence_dir, &existing);
            return decision(DecisionInput {
                action: Some("noop".to_string()),
                reason: Some("Receipt already revoked.".to_string()),
                reason_code: Some(json!("already_revoked")),
                receipt_id: Some(json!(receipt_id)),
                evidence: Some(evidence),
                receipt: Some(existing),
                ..Default::default()
            });
        }
        let revoked = self
            .store
            .revoke(receipt_id, reason.unwrap_or("explicit_revoke"), None)
            .map_err(|e| AdmissionError(e.to_string()))?
            .ok_or_else(|| AdmissionError("revoke: receipt disappeared".to_string()))?;
        let (evidence, evidence_path) = attach_evidence(&self.evidence_dir, &revoked);
        decision(DecisionInput {
            action: Some("allow".to_string()),
            reason: Some("Recall receipt revoked.".to_string()),
            reason_code: Some(json!("revoked")),
            receipt_id: Some(json!(receipt_id)),
            evidence: Some(evidence),
            evidence_path: Some(evidence_path),
            receipt: Some(revoked),
            next_action: Some(json!("Call recall before further repository recall-dependent work.")),
            ..Default::default()
        })
    }
}
