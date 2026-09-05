//! Native document candidate adapter. Pull remains the admission authority.
use super::{limits::WorkBudget, service::{Caller, LedgerService, validate_task_grant}};
use membrane_protocol::{CandidateV1, FederationProviderStatusV1, FreshnessClass, ProviderDiagnosticsV1,
    ProviderId, ProviderOmissionV1, ProviderOutputV1, ProviderWarningV1, ReasonCode,
    WarningSeverity, PROVIDER_OUTPUT_SCHEMA_VERSION};
use membrane_provider_sdk::{Provider, ProviderContext, ProviderError, ProviderOutput};
use serde_json::json;
use std::{collections::BTreeMap, future::Future, path::Path, pin::Pin, sync::Arc};

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
            let Some(owner) = self.owner.clone() else { return Ok(gap(ReasonCode::ProviderUnavailable,"ledger_owner_unavailable")); };
            if !super::doc_candidate_provider::is_doc_provider_enabled() {
                return Ok(gap(ReasonCode::ProviderUnavailable,"ledger_delivery_disabled"));
            }
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

fn materialize(owner: &LedgerService, context: &ProviderContext, budget: &WorkBudget) -> Result<ProviderOutput,String> {
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
            (Some(ranges),Some(grant.id.as_str()))
        }
    };
    let (result,tickets) = owner.search(&caller,&context.task,12,false,ranges,grant_id,budget)?;
    let generation = context.freshness.generation.clone().or(context.release_generation.clone());
    let live = super::qualification::delivery_allowed(context.release_generation.as_deref());
    let observed_count = result.hits.len();
    let mut candidates = Vec::new();
    if live {
        for (hit,ticket) in result.hits.iter().zip(tickets) {
            let mut arguments = serde_json::to_value(hit.resolve_request()).map_err(|e|e.to_string())?;
            arguments["repository"] = json!(caller.repository_id);
            arguments["caller"] = caller.envelope();
            arguments["ledgerTicket"] = json!(ticket);
            let resolver = json!({"tool":"membrane_source_read","arguments":arguments}).to_string();
            let text = format!("Ledger evidence: {} ({}, {} bytes). Resolve the captured span.",
                hit.source_ref,hit.node_kind,hit.end_byte-hit.start_byte);
            let estimated_tokens = cortex_core::estimate_tokens(&format!("{text}\n{resolver}")) as u32;
            candidates.push(CandidateV1 {id:hit.node_id.clone(),layer:2,provider:Some("ledger".into()),
                source_kind:"doc".into(),source_ref:format!("{}#{}",hit.source_ref,hit.node_id),
                source_hash:format!("sha256:{}",hit.expected_span_hash),trust_class:"workspace_document".into(),
                instruction_policy:"data_only".into(),provider_score:hit.score.clamp(0.0,1.0),
                score_components:BTreeMap::from([("lexical".into(),hit.score),("freshness".into(),1.0)]),
                base_commit:Some(hit.expected_revision.clone()),overlay_digest:Some(hit.expected_content_hash.clone()),
                freshness_class:Some(if hit.source_kind=="imported_snapshot" {FreshnessClass::CommittedSnapshot}else{FreshnessClass::Current}),
                snapshot_id:Some(format!("ledger:{}",hit.ledger_generation)),estimated_tokens,
                protected:false,exact:true,recoverable:true,resolver,text});
        }
    }
    let mut output = ProviderOutputV1 {schema_version:PROVIDER_OUTPUT_SCHEMA_VERSION,provider:ProviderId::Ledger,
        status:if result.complete && live {FederationProviderStatusV1::Complete}else{FederationProviderStatusV1::Partial},
        generation:generation.clone(),candidates,warnings:Vec::new(),omissions:Vec::new(),
        diagnostics:Some(ProviderDiagnosticsV1 {provider:ProviderId::Ledger,elapsed_ms:None,generation,
            attributes:BTreeMap::from([("provenance".into(),"ledger-source-owner".into()),
                ("ledger_generation".into(),result.publication_generation.to_string()),
                ("mode".into(),if live{"live"}else{"shadow_unqualified"}.into())])}),
        extensions:BTreeMap::from([("ledger".into(),json!({"observedCandidates":observed_count,
            "complete":result.complete,"omissions":result.omissions,"lane":result.lane,
            "sourceBytesChecked":result.source_bytes_checked,"policyDigest":result.policy_digest,
            "publicationGeneration":result.publication_generation,"graph":result.graph,"delivered":live}))])};
    if !live { output.omissions.push(omission(ReasonCode::ProviderUnavailable,"ledger_delivery_qualification_required")); }
    if !result.complete {
        output.warnings.push(ProviderWarningV1 {provider:ProviderId::Ledger,reason:ReasonCode::ProviderFailed,
            severity:WarningSeverity::Warning,detail_id:Some("ledger_source_incomplete".into()),stage:Some("source".into()),message:None});
    }
    caller.authorize("context")?;
    validate_task_grant(grant_id,&caller,None,Some(&context.session_id))?;
    budget.check()?;
    Ok(output)
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
