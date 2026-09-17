//! Native document candidate adapter. Pull remains the admission authority.
use super::{limits::WorkBudget, service::{Caller, LedgerService, validate_task_grant}};
use membrane_protocol::{CandidateV1, FederationProviderStatusV1, FreshnessClass, ProviderDiagnosticsV1,
    ProviderId, ProviderOmissionV1, ProviderOutputV1, ProviderWarningV1, ReasonCode,
    WarningSeverity, PROVIDER_OUTPUT_SCHEMA_VERSION};
use membrane_provider_sdk::{Provider, ProviderContext, ProviderError, ProviderOutput};
use serde_json::json;
use std::{collections::BTreeMap, future::Future, path::Path, pin::Pin, sync::Arc, time::Duration};

/// Minimum remaining request budget worth spending on a ledger lane. Every
/// lane call first reconciles its document index against the worktree
/// (`sync_locked` → `walk_markdown`): a policy-filtered tree walk plus content
/// hashing that takes seconds on a real repository. Under a latency-bound
/// deadline — the SessionStart hook's resident probe carries 600ms — the
/// reconcile cannot finish, so attempting it spends the entire request window
/// inside a blocking task the deadline cannot preempt and still produces
/// nothing. A typed gap preserves sibling lanes' evidence instead; mirrors
/// the one-shot Blueprint status floor in `freshness.rs`.
const LEDGER_LANE_MIN_REMAINING_MS: u64 = 5_000;

pub(crate) struct LedgerProvider { owner: Option<Arc<LedgerService>> }
impl LedgerProvider {
    pub(crate) fn new(owner: Option<Arc<LedgerService>>) -> Self { Self { owner } }
}
struct CancelWork(tokio_util::sync::CancellationToken);
impl Drop for CancelWork { fn drop(&mut self) { self.0.cancel(); } }

impl Provider for LedgerProvider {
    fn provide<'life0, 'life1, 'async_trait>(&'life0 self, context: &'life1 ProviderContext)
        -> Pin<Box<dyn Future<Output=Result<ProviderOutput,ProviderError>> + Send + 'async_trait>>
    where 'life0:'async_trait, 'life1:'async_trait, Self:'async_trait
    {
        Box::pin(async move {
            if context.is_cancelled() { return Ok(gap(ReasonCode::ProviderCancelled,"ledger_cancelled")); }
            if context.is_deadline_exhausted() { return Ok(gap(ReasonCode::DeadlineExhausted,"ledger_deadline_exhausted")); }
            if context.remaining() < Duration::from_millis(LEDGER_LANE_MIN_REMAINING_MS) {
                return Ok(gap(ReasonCode::ProviderUnavailable,"ledger_sync_budget_insufficient"));
            }
            let Some(owner) = self.owner.clone() else { return Ok(gap(ReasonCode::ProviderUnavailable,"ledger_owner_unavailable")); };
            let context = context.clone();
            let cancellation = context.cancellation.child_token();
            let guard = CancelWork(cancellation.clone());
            let budget = WorkBudget::new(context.deadline,cancellation);
            // SQLite and parsing stay off the federation reactor. The drop
            // guard cancels inherited work if the scheduler drops this future.
            let result = tokio::task::spawn_blocking(move || materialize(&owner,&context,&budget)).await;
            drop(guard);
            Ok(match result {
                Ok(Ok(output)) => output,
                Ok(Err(reason)) => {
                    let code = if reason.contains("grant") || reason.contains("denied") || reason.contains("enrolled") {
                        ReasonCode::ScopeGrantInvalid
                    } else if reason.contains("deadline") { ReasonCode::DeadlineExhausted }
                    else if reason.contains("cancelled") { ReasonCode::ProviderCancelled }
                    else { ReasonCode::ProviderUnavailable };
                    gap(code,&safe_reason(&reason))
                }
                Err(_) => gap(ReasonCode::ProviderFailed,"ledger_worker_failed"),
            })
        })
    }
}

/// Enrolled caller plus grant-narrowed ranges, shared by the document lane
/// (LDG-022) and the skill-document lane (LDG-032). Both lanes apply the same
/// grant binding checks before any source work happens.
fn enrolled_caller(
    context: &ProviderContext,
) -> Result<(Caller, Option<Vec<membrane_protocol::ReadPathV1>>, Option<String>), String> {
    let caller = Caller::enrolled(Path::new(&context.repository_root),&context.repository_id)?;
    caller.authorize("context")?;
    let (ranges, grant_id) = match &context.scope_grant {
        None => (None,None), // Separately verified repository-enrollment grant.
        Some(grant) => {
            validate_task_grant(Some(&grant.id),&caller,Some(&grant.task_id),Some(&context.session_id))?;
            if grant.repository_root != context.repository_root || grant.repository_id != context.repository_id
                || grant.session_id != context.session_id {
                return Err("ledger_scope_grant_binding_mismatch".into());
            }
            // The current catalog mirror loses exact ranges. Missing evidence
            // is a typed refusal, not an unrestricted source set.
            if !grant.permitted_edge_types.iter().any(|edge| edge == "source_read") {
                return Err("ledger_source_read_not_granted".into());
            }
            if grant.read_paths.is_empty() { return Err("ledger_scope_ranges_unavailable".into()); }
            let mut ranges = Vec::new();
            for value in &grant.read_paths {
                let (path,lines) = value.rsplit_once(':').ok_or("ledger_range_grant_invalid")?;
                let (start,end) = lines.split_once('-').ok_or("ledger_range_grant_invalid")?;
                let start_line: u32 = start.parse().map_err(|_| "ledger_range_grant_invalid")?;
                let end_line: u32 = end.parse().map_err(|_| "ledger_range_grant_invalid")?;
                if start_line == 0 || end_line < start_line { return Err("ledger_range_grant_invalid".into()); }
                ranges.push(membrane_protocol::ReadPathV1 {path:path.into(),start_line,end_line});
            }
            (Some(ranges),Some(grant.id.clone()))
        }
    };
    Ok((caller, ranges, grant_id))
}

fn materialize(owner: &LedgerService, context: &ProviderContext, budget: &WorkBudget) -> Result<ProviderOutput,String> {
    let (caller, ranges, grant_id) = enrolled_caller(context)?;
    let (result,tickets) = owner.search(&caller,&context.task,12,false,ranges,grant_id.as_deref(),budget)?;
    let generation = context.freshness.generation.clone().or(context.release_generation.clone());
    let observed_count = result.hits.len();
    let mut candidates = Vec::new();
    for (hit,ticket) in result.hits.iter().zip(tickets) {
        candidates.push(candidate_for_hit(
            hit, &caller.repository_id, caller.envelope(), &context.session_id, ticket,
        )?);
    }
    let mut output = ProviderOutputV1 {schema_version:PROVIDER_OUTPUT_SCHEMA_VERSION,provider:ProviderId::Ledger,
        status:if result.complete {FederationProviderStatusV1::Complete}else{FederationProviderStatusV1::Partial},
        generation:generation.clone(),candidates,warnings:Vec::new(),omissions:Vec::new(),
        diagnostics:Some(ProviderDiagnosticsV1 {provider:ProviderId::Ledger,elapsed_ms:None,generation,
            attributes:BTreeMap::from([("provenance".into(),"ledger-source-owner".into()),
                ("ledger_generation".into(),result.publication_generation.to_string()),
                ("mode".into(),"live".into())])}),
        extensions:BTreeMap::from([("ledger".into(),json!({"observedCandidates":observed_count,
            "complete":result.complete,"omissions":result.omissions,"lane":result.lane,
            "sourceBytesChecked":result.source_bytes_checked,"policyDigest":result.policy_digest,
            "publicationGeneration":result.publication_generation,"graph":result.graph,"delivered":true}))])};
    if !result.complete {
        output.warnings.push(ProviderWarningV1 {provider:ProviderId::Ledger,reason:ReasonCode::ProviderFailed,
            severity:WarningSeverity::Warning,detail_id:Some("ledger_source_incomplete".into()),stage:Some("source".into()),message:None});
    }
    caller.authorize("context")?;
    validate_task_grant(grant_id.as_deref(),&caller,None,Some(&context.session_id))?;
    budget.check()?;
    Ok(output)
}

/// Native portable skill-document candidate adapter (LDG-032). This is the
/// production Pull skill-document provider lane: it enumerates registered
/// `tools/skills/<name>/SKILL.md` projections through the daemon-owned
/// `LedgerService` skill adapter and emits `membrane_source_read`-bound
/// candidates — the same source/revision/span/ticket binding as the document
/// lane. Cortex's `skill-read` resolver is never used; separately admitted
/// durable skill insights remain a Cortex lane, not this index.
///
/// Registration lives in `pull/native_federation.rs` (`ProviderId::Skills`).
pub(crate) struct LedgerSkillProvider { owner: Option<Arc<LedgerService>> }
impl LedgerSkillProvider {
    pub(crate) fn new(owner: Option<Arc<LedgerService>>) -> Self { Self { owner } }
}

const MAX_SKILL_CANDIDATES: usize = 5;

impl Provider for LedgerSkillProvider {
    fn provide<'life0, 'life1, 'async_trait>(&'life0 self, context: &'life1 ProviderContext)
        -> Pin<Box<dyn Future<Output=Result<ProviderOutput,ProviderError>> + Send + 'async_trait>>
    where 'life0:'async_trait, 'life1:'async_trait, Self:'async_trait
    {
        Box::pin(async move {
            if context.is_cancelled() { return Ok(gap(ReasonCode::ProviderCancelled,"ledger_cancelled")); }
            if context.is_deadline_exhausted() { return Ok(gap(ReasonCode::DeadlineExhausted,"ledger_deadline_exceeded")); }
            if context.remaining() < Duration::from_millis(LEDGER_LANE_MIN_REMAINING_MS) {
                return Ok(gap(ReasonCode::ProviderUnavailable,"ledger_sync_budget_insufficient"));
            }
            let Some(owner) = self.owner.clone() else { return Ok(gap(ReasonCode::ProviderUnavailable,"ledger_owner_unavailable")); };
            let context = context.clone();
            let cancellation = context.cancellation.child_token();
            let guard = CancelWork(cancellation.clone());
            let budget = WorkBudget::new(context.deadline,cancellation);
            let result = tokio::task::spawn_blocking(move || skill_materialize(&owner,&context,&budget)).await;
            drop(guard);
            Ok(match result {
                Ok(Ok(output)) => output,
                Ok(Err(reason)) => {
                    let code = if reason.contains("grant") || reason.contains("denied") || reason.contains("enrolled") {
                        ReasonCode::ScopeGrantInvalid
                    } else if reason.contains("deadline") { ReasonCode::DeadlineExhausted }
                    else if reason.contains("cancelled") { ReasonCode::ProviderCancelled }
                    else { ReasonCode::ProviderUnavailable };
                    gap(code,&safe_reason(&reason))
                }
                Err(_) => gap(ReasonCode::ProviderFailed,"ledger_worker_failed"),
            })
        })
    }
}

fn skill_materialize(owner: &LedgerService, context: &ProviderContext, budget: &WorkBudget) -> Result<ProviderOutput,String> {
    let (caller, ranges, grant_id) = enrolled_caller(context)?;
    let (result, skills) = owner.skill_documents(
        &caller, &context.task, MAX_SKILL_CANDIDATES, ranges, grant_id.as_deref(), budget)?;
    let generation = context.freshness.generation.clone().or(context.release_generation.clone());
    let observed_count = skills.len();
    let mut candidates = Vec::new();
    for skill in &skills {
        candidates.push(candidate_for_skill(
            skill, &caller.repository_id, caller.envelope(), &context.session_id,
        )?);
    }
    let mut output = ProviderOutputV1 {schema_version:PROVIDER_OUTPUT_SCHEMA_VERSION,provider:ProviderId::Skills,
        status:if result.complete {FederationProviderStatusV1::Complete}else{FederationProviderStatusV1::Partial},
        generation:generation.clone(),candidates,warnings:Vec::new(),omissions:Vec::new(),
        diagnostics:Some(ProviderDiagnosticsV1 {provider:ProviderId::Skills,elapsed_ms:None,generation,
            attributes:BTreeMap::from([("provenance".into(),"ledger-source-owner".into()),
                ("ledger_generation".into(),result.publication_generation.to_string()),
                ("representation".into(),"index_only".into()),
                ("mode".into(),"live".into())])}),
        extensions:BTreeMap::from([("ledger".into(),json!({"observedCandidates":observed_count,
            "complete":result.complete,"omissions":result.omissions,"lane":result.lane,
            "sourceBytesChecked":result.source_bytes_checked,"policyDigest":result.policy_digest,
            "publicationGeneration":result.publication_generation,"delivered":true}))])};
    if !result.complete {
        output.warnings.push(ProviderWarningV1 {provider:ProviderId::Skills,reason:ReasonCode::ProviderFailed,
            severity:WarningSeverity::Warning,detail_id:Some("ledger_source_incomplete".into()),stage:Some("source".into()),message:None});
    }
    caller.authorize("context")?;
    validate_task_grant(grant_id.as_deref(),&caller,None,Some(&context.session_id))?;
    budget.check()?;
    Ok(output)
}

/// The skills-lane counterpart of [`candidate_for_hit`]: identical
/// `membrane_source_read` resolver envelope and hash/span/generation binding,
/// with the candidate identity and trust class the Pull skills lane expects.
fn candidate_for_skill(
    skill: &super::skill_documents::TicketedSkillDocumentV1,
    repository_id: &str,
    caller: serde_json::Value,
    session_id: &str,
) -> Result<CandidateV1, String> {
    let hit = &skill.hit;
    let mut arguments = serde_json::to_value(hit.resolve_request()).map_err(|e|e.to_string())?;
    arguments["repository"] = json!(repository_id);
    arguments["caller"] = caller;
    arguments["ledgerTicket"] = json!(skill.ticket);
    arguments["sessionId"] = json!(session_id);
    let resolver = json!({"tool":"membrane_source_read","arguments":arguments}).to_string();
    let text = format!("Skill document: {} ({}, {} bytes). Resolve the captured document span.",
        skill.title,hit.source_ref,hit.end_byte-hit.start_byte);
    let estimated_tokens = cortex_core::estimate_tokens(&format!("{text}\n{resolver}")) as u32;
    Ok(CandidateV1 {id:format!("skills:{}",skill.skill_id),layer:7,provider:Some(ProviderId::Skills.as_str().into()),
        source_kind:"skill".into(),source_ref:format!("{}#{}",hit.source_ref,hit.node_id),
        source_hash:format!("sha256:{}",hit.expected_span_hash),trust_class:"workspace_tracked".into(),
        instruction_policy:"data_only".into(),provider_score:hit.score.clamp(0.0,1.0),
        score_components:BTreeMap::from([("lexical".into(),hit.score),("freshness".into(),1.0)]),
        base_commit:Some(hit.expected_revision.clone()),overlay_digest:Some(hit.expected_content_hash.clone()),
        freshness_class:Some(if hit.source_kind=="imported_snapshot" {FreshnessClass::CommittedSnapshot}else{FreshnessClass::Current}),
        snapshot_id:Some(format!("ledger:{}",hit.ledger_generation)),estimated_tokens,
        protected:false,exact:true,recoverable:true,resolver,text})
}

fn candidate_for_hit(
    hit: &super::query::LedgerHit,
    repository_id: &str,
    caller: serde_json::Value,
    session_id: &str,
    ticket: String,
) -> Result<CandidateV1, String> {
    let mut arguments = serde_json::to_value(hit.resolve_request()).map_err(|e|e.to_string())?;
    arguments["repository"] = json!(repository_id);
    arguments["caller"] = caller;
    arguments["ledgerTicket"] = json!(ticket);
    arguments["sessionId"] = json!(session_id);
    let resolver = json!({"tool":"membrane_source_read","arguments":arguments}).to_string();
    let text = format!("Ledger evidence: {} ({}, {} bytes). Resolve the captured span.",
        hit.source_ref,hit.node_kind,hit.end_byte-hit.start_byte);
    let estimated_tokens = cortex_core::estimate_tokens(&format!("{text}\n{resolver}")) as u32;
    Ok(CandidateV1 {id:hit.node_id.clone(),layer:2,provider:Some("ledger".into()),
        source_kind:"doc".into(),source_ref:format!("{}#{}",hit.source_ref,hit.node_id),
        source_hash:format!("sha256:{}",hit.expected_span_hash),trust_class:"workspace_document".into(),
        instruction_policy:"data_only".into(),provider_score:hit.score.clamp(0.0,1.0),
        score_components:BTreeMap::from([("lexical".into(),hit.score),("freshness".into(),1.0)]),
        base_commit:Some(hit.expected_revision.clone()),overlay_digest:Some(hit.expected_content_hash.clone()),
        freshness_class:Some(if hit.source_kind=="imported_snapshot" {FreshnessClass::CommittedSnapshot}else{FreshnessClass::Current}),
        snapshot_id:Some(format!("ledger:{}",hit.ledger_generation)),estimated_tokens,
        protected:false,exact:true,recoverable:true,resolver,text})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_pull_candidate_keeps_hash_bound_source_resolution() {
        let hit = super::super::query::LedgerHit {
            doc_id: "doc-1".into(), node_id: "node-1".into(),
            source_ref: "doc://README.md".into(), anchor_id: "anchor-1".into(),
            expected_content_hash: "content-hash".into(), expected_revision: "commit-1".into(),
            expected_span_hash: "span-hash".into(), ledger_generation: 7,
            source_kind: "markdown".into(), node_kind: "paragraph".into(),
            start_byte: 10, end_byte: 24, lane: "exact".into(), score: 1.0,
            literal_range: None,
        };
        let candidate = candidate_for_hit(
            &hit, "repo-1", json!({"root":"/repo","repositoryId":"repo-1"}),
            "session-1", "ticket-1".into(),
        ).unwrap();
        assert_eq!(candidate.provider.as_deref(), Some("ledger"));
        assert_eq!(candidate.source_ref, "doc://README.md#node-1");
        assert_eq!(candidate.source_hash, "sha256:span-hash");
        assert!(candidate.recoverable);
        assert!(candidate.resolver.contains("expectedSpanHash"));
        assert!(candidate.resolver.contains("ticket-1"));
    }

    #[test]
    fn skill_candidate_carries_source_read_binding_not_cortex_resolver() {
        let hit = super::super::query::LedgerHit {
            doc_id: "doc-skill".into(), node_id: "node-skill".into(),
            source_ref: "doc://tools/skills/deploy/SKILL.md".into(), anchor_id: "anchor-s".into(),
            expected_content_hash: "content-hash".into(), expected_revision: "rev-1".into(),
            expected_span_hash: "span-hash".into(), ledger_generation: 3,
            source_kind: "worktree".into(), node_kind: "document".into(),
            start_byte: 0, end_byte: 120, lane: "ledger_skill".into(), score: 0.8,
            literal_range: None,
        };
        let skill = super::super::skill_documents::TicketedSkillDocumentV1 {
            skill_id: "deploy".into(), title: "Deploy".into(), hit, ticket: "ticket-s".into(),
        };
        let candidate = candidate_for_skill(
            &skill, "repo-1", json!({"root":"/repo","repositoryId":"repo-1"}), "session-1",
        ).unwrap();
        assert_eq!(candidate.id, "skills:deploy");
        assert_eq!(candidate.provider.as_deref(), Some("skills"));
        assert_eq!(candidate.source_kind, "skill");
        assert!(candidate.exact && candidate.recoverable);
        assert!(candidate.resolver.contains("membrane_source_read"));
        assert!(candidate.resolver.contains("ticket-s"));
        assert!(!candidate.resolver.contains("cortex"));
    }

    fn budget_context(deadline: std::time::Instant) -> ProviderContext {
        ProviderContext::new(
            "request", ".", "repo", "task", "session", "client", vec![], None, None,
            membrane_protocol::FreshnessSnapshotV1 {
                graph_state: "ready".into(), generation: None, snapshot_id: None,
                base_commit: None, overlay_digest: None, stale: false,
            },
            deadline, tokio_util::sync::CancellationToken::new(), "trace",
            membrane_provider_sdk::SourceSet::default(),
        )
    }

    #[test]
    fn ledger_lanes_gap_below_sync_budget_floor_without_owner() {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        runtime.block_on(async {
            let tight = budget_context(
                std::time::Instant::now() + Duration::from_millis(LEDGER_LANE_MIN_REMAINING_MS - 1),
            );
            let output = LedgerSkillProvider::new(None).provide(&tight).await.unwrap();
            assert_eq!(output.omissions[0].detail_id.as_deref(), Some("ledger_sync_budget_insufficient"));
            let output = LedgerProvider::new(None).provide(&tight).await.unwrap();
            assert_eq!(output.omissions[0].detail_id.as_deref(), Some("ledger_sync_budget_insufficient"));
            // A comfortable budget falls through to the owner check, not the floor.
            let ample = budget_context(
                std::time::Instant::now() + Duration::from_millis(LEDGER_LANE_MIN_REMAINING_MS + 5_000),
            );
            let output = LedgerSkillProvider::new(None).provide(&ample).await.unwrap();
            assert_eq!(output.omissions[0].detail_id.as_deref(), Some("ledger_owner_unavailable"));
        });
    }
}
fn safe_reason(reason:&str)->String {
    let first = reason.split(':').next().unwrap_or("ledger_unavailable");
    if first.len() <= 96 && first.chars().all(|c|c.is_ascii_alphanumeric()||c=='_') {first.into()} else {"ledger_unavailable".into()}
}
fn omission(reason:ReasonCode,detail:&str)->ProviderOmissionV1 {
    ProviderOmissionV1 {provider:ProviderId::Ledger,reason,candidate_id:None,detail_id:Some(detail.into()),stage:Some("ledger".into())}
}
fn gap(reason:ReasonCode,detail:&str)->ProviderOutputV1 {
    ProviderOutputV1 {schema_version:PROVIDER_OUTPUT_SCHEMA_VERSION,provider:ProviderId::Ledger,
        status:FederationProviderStatusV1::Failed,generation:None,candidates:Vec::new(),warnings:Vec::new(),
        omissions:vec![omission(reason,detail)],diagnostics:None,extensions:BTreeMap::new()}
}
