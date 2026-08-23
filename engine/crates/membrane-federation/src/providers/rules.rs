//! Rules provider for tracked, grant-authorized instruction documents.
//!
//! Filesystem and delivery persistence belong to injected owner interfaces.
//! Rule text is data: trust and instruction policy are supplied as metadata,
//! never inferred from document contents.

use membrane_protocol::{digest_str, FederationProviderStatusV1, ProviderId, ProviderOmissionV1,
    ProviderOutputV1, ProviderWarningV1, ProviderDiagnosticsV1, ReasonCode, WarningSeverity,
    PROVIDER_OUTPUT_SCHEMA_VERSION};
use membrane_provider_sdk::{CapabilityV1, Provider, ProviderContext, ProviderError, ProviderOutput, SourceQuery};
use serde::Serialize;
use serde_json::json;
use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

pub const MAX_RULE_BYTES: usize = 1_500_000;
const SELF_LOADING_CLIENTS: &[&str] = &["claude", "claude_code", "codex"];

pub type RuleFuture<T> = Pin<Box<dyn Future<Output = Result<T, RuleSourceError>> + Send>>;
pub type LedgerFuture = Pin<Box<dyn Future<Output = Result<DeliveryReceipt, LedgerError>> + Send>>;

/// Source-owned rule metadata and content.  `trust_class` and
/// `instruction_policy` are closed by the source owner; this provider never
/// derives either value from `content`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuleDocument {
    pub repository_id: String,
    pub normalized_path: String,
    pub rule_identity: String,
    pub content: String,
    pub source_hash: Option<String>,
    pub trust_class: String,
    pub instruction_policy: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuleSourceResponse {
    pub documents: Vec<RuleDocument>,
    pub generation: Option<String>,
    pub complete: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuleSourceError {
    Unavailable(String),
    Malformed(String),
    Unauthorized(String),
}

impl std::fmt::Display for RuleSourceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable(detail) => write!(f, "rules_source_unavailable:{detail}"),
            Self::Malformed(detail) => write!(f, "rules_source_malformed:{detail}"),
            Self::Unauthorized(detail) => write!(f, "rules_source_unauthorized:{detail}"),
        }
    }
}

impl std::error::Error for RuleSourceError {}

/// Owner API receives only paths already authorized by the request grant.
pub trait RuleSource: Send + Sync {
    fn read_rules(&self, query: SourceQuery, authorized_paths: Vec<String>)
        -> RuleFuture<RuleSourceResponse>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeliveryKey {
    pub repository_id: String,
    pub client: String,
    pub session_id: String,
    pub candidate_id: String,
    pub source_hash: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryMode {
    Native,
    Inline,
    Reference,
}

impl DeliveryMode {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Native => "native",
            Self::Inline => "inline",
            Self::Reference => "reference",
        }
    }
}

/// A content-free receipt returned by the owner ledger after its atomic
/// claim/record operation.  The provider persists no ledger state itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeliveryReceipt {
    pub receipt_id: String,
    pub mode: DeliveryMode,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LedgerError {
    Unavailable(String),
    Malformed(String),
}

impl std::fmt::Display for LedgerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable(detail) => write!(f, "delivery_ledger_unavailable:{detail}"),
            Self::Malformed(detail) => write!(f, "delivery_ledger_malformed:{detail}"),
        }
    }
}

impl std::error::Error for LedgerError {}

/// Ledger implementations must atomically decide first delivery versus
/// reference delivery and return a receipt for the decision.
pub trait DeliveryLedger: Send + Sync {
    fn claim(&self, key: DeliveryKey) -> LedgerFuture;
}

pub struct RulesProvider {
    source: Arc<dyn RuleSource>,
    ledger: Arc<dyn DeliveryLedger>,
}

impl std::fmt::Debug for RulesProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RulesProvider").finish_non_exhaustive()
    }
}

impl RulesProvider {
    pub fn new(source: Arc<dyn RuleSource>, ledger: Arc<dyn DeliveryLedger>) -> Self {
        Self { source, ledger }
    }

    /// Produce one typed rules lane.  Grant validation happens before source
    /// access, and every source/ledger failure remains explicit in output.
    pub async fn produce(&self, context: &ProviderContext) -> ProviderOutputV1 {
        let mut output = empty_output(context.release_generation.clone());
        if context.is_cancelled() {
            output.status = FederationProviderStatusV1::Cancelled;
            output.omissions.push(omission(ReasonCode::ProviderCancelled, "cancelled"));
            return output;
        }
        if context.is_deadline_exhausted() {
            output.status = FederationProviderStatusV1::Failed;
            output.omissions.push(omission(ReasonCode::ProviderTimeout, "deadline_exhausted"));
            return output;
        }
        let Some(grant) = context.scope_grant.as_ref() else {
            output.status = FederationProviderStatusV1::Failed;
            output.omissions.push(omission(ReasonCode::ScopeGrantMissing, "grant_required"));
            return output;
        };
        if grant.repository_id != context.repository_id
            || grant.repository_root != context.repository_root
            || grant.session_id != context.session_id
        {
            output.status = FederationProviderStatusV1::Failed;
            output.omissions.push(omission(ReasonCode::ScopeGrantInvalid, "grant_binding_mismatch"));
            return output;
        }
        let authorized_paths = grant
            .read_paths
            .iter()
            .filter_map(|entry| normalize_grant_path(entry))
            .collect::<Vec<_>>();
        if authorized_paths.is_empty() {
            output.status = FederationProviderStatusV1::Partial;
            output.omissions.push(omission(ReasonCode::ScopeGrantInvalid, "no_rule_paths"));
            return output;
        }
        let response = match self.source.read_rules(context.query(), authorized_paths).await {
            Ok(response) => response,
            Err(error) => {
                let reason = match &error {
                    RuleSourceError::Unauthorized(_) => ReasonCode::ScopeGrantInvalid,
                    RuleSourceError::Malformed(_) => ReasonCode::ProviderMalformed,
                    RuleSourceError::Unavailable(_) => ReasonCode::ProviderUnavailable,
                };
                output.status = FederationProviderStatusV1::Partial;
                output.warnings.push(warning(reason, "rules_source"));
                output.omissions.push(omission(reason, "source_unavailable"));
                return output;
            }
        };
        output.generation = response.generation.or_else(|| context.release_generation.clone());
        let mut documents = response.documents;
        documents.sort_by(|left, right| {
            left.normalized_path
                .cmp(&right.normalized_path)
                .then_with(|| left.rule_identity.cmp(&right.rule_identity))
        });
        let self_loading = SELF_LOADING_CLIENTS.contains(&context.client.trim());
        let mut modes = BTreeMap::new();
        let mut receipts = BTreeMap::new();
        for document in documents {
            if context.is_cancelled() {
                output.status = FederationProviderStatusV1::Cancelled;
                output.omissions.push(omission(ReasonCode::ProviderCancelled, "cancelled"));
                break;
            }
            if context.is_deadline_exhausted() {
                output.status = FederationProviderStatusV1::Partial;
                output.omissions.push(omission(ReasonCode::ProviderTimeout, "deadline_exhausted"));
                break;
            }
            let normalized = match normalize_rule_path(&document.normalized_path) {
                Ok(path) => path,
                Err(detail) => {
                    output.omissions.push(omission(ReasonCode::ProviderMalformed, detail));
                    continue;
                }
            };
            if !authorized_paths_contains(grant, &normalized) {
                output.omissions.push(omission(ReasonCode::ScopeGrantInvalid, "path_not_granted"));
                continue;
            }
            if document.repository_id != context.repository_id {
                output.omissions.push(omission(ReasonCode::ProviderMalformed, "repository_mismatch"));
                continue;
            }
            if document.rule_identity.trim().is_empty()
                || document.trust_class.trim().is_empty()
                || document.instruction_policy.trim().is_empty()
            {
                output.omissions.push(omission(ReasonCode::ProviderMalformed, "source_metadata_missing"));
                continue;
            }
            let bytes = document.content.as_bytes();
            if bytes.len() > MAX_RULE_BYTES {
                output.omissions.push(omission(ReasonCode::ProviderMalformed, "content_limit"));
                continue;
            }
            let source_hash = digest_str(&document.content);
            if let Some(declared) = document.source_hash.as_deref() {
                if declared != source_hash {
                    output.omissions.push(omission(ReasonCode::ProviderMalformed, "source_hash_mismatch"));
                    continue;
                }
            }
            let candidate_id = stable_rule_candidate_id(
                &context.repository_id,
                &normalized,
                &source_hash,
                &document.rule_identity,
            );
            let (mode, body, receipt_id) = if self_loading {
                (DeliveryMode::Native, String::new(), None)
            } else {
                let key = DeliveryKey {
                    repository_id: context.repository_id.clone(),
                    client: context.client.trim().to_owned(),
                    session_id: context.session_id.clone(),
                    candidate_id: candidate_id.clone(),
                    source_hash: source_hash.clone(),
                };
                match self.ledger.claim(key).await {
                    Ok(receipt) => (receipt.mode, if receipt.mode == DeliveryMode::Inline { document.content.clone() } else { String::new() }, Some(receipt.receipt_id)),
                    Err(_) => {
                        output.warnings.push(warning(ReasonCode::ProviderUnavailable, "delivery_ledger"));
                        output.omissions.push(omission(ReasonCode::ProviderUnavailable, "delivery_ledger_unavailable"));
                        continue;
                    }
                }
            };
            modes.insert(candidate_id.clone(), mode.as_str().to_owned());
            if let Some(receipt_id) = receipt_id { receipts.insert(candidate_id.clone(), receipt_id); }
            output.candidates.push(candidate(
                candidate_id,
                normalized,
                source_hash,
                document,
                body,
            ));
        }
        output.extensions.insert("deliveryModes".into(), json!(modes));
        output.extensions.insert("deliveryReceipts".into(), json!(receipts));
        if output.status != FederationProviderStatusV1::Cancelled {
            output.status = if output.omissions.is_empty() && response.complete {
                FederationProviderStatusV1::Complete
            } else if output.candidates.is_empty() {
                FederationProviderStatusV1::Failed
            } else {
                FederationProviderStatusV1::Partial
            };
        }
        output
    }
}

impl Provider for RulesProvider {
    fn provide<'life0, 'life1, 'async_trait>(
        &'life0 self,
        context: &'life1 ProviderContext,
    ) -> Pin<Box<dyn Future<Output = Result<ProviderOutput, ProviderError>> + Send + 'async_trait>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move { Ok(self.produce(context).await) })
    }

    fn list_capabilities(&self) -> Vec<CapabilityV1> {
        Vec::new()
    }
}

fn empty_output(generation: Option<String>) -> ProviderOutputV1 {
    ProviderOutputV1 {
        schema_version: PROVIDER_OUTPUT_SCHEMA_VERSION,
        provider: ProviderId::Rules,
        status: FederationProviderStatusV1::Partial,
        generation,
        candidates: Vec::new(),
        warnings: Vec::new(),
        omissions: Vec::new(),
        diagnostics: Some(ProviderDiagnosticsV1 {
            provider: ProviderId::Rules,
            elapsed_ms: None,
            generation: None,
            attributes: BTreeMap::new(),
        }),
        extensions: BTreeMap::new(),
    }
}

fn candidate(
    id: String,
    path: String,
    source_hash: String,
    document: RuleDocument,
    text: String,
) -> membrane_protocol::CandidateV1 {
    membrane_protocol::CandidateV1 {
        id,
        layer: 2,
        provider: Some(ProviderId::Rules.as_str().to_owned()),
        source_kind: "doc".into(),
        source_ref: path.clone(),
        source_hash,
        trust_class: document.trust_class,
        instruction_policy: document.instruction_policy,
        provider_score: 0.3,
        score_components: BTreeMap::from([("rule_relevance".into(), 0.3)]),
        base_commit: None,
        overlay_digest: None,
        freshness_class: None,
        snapshot_id: None,
        estimated_tokens: (text.len() / 4).max(1) as u32,
        protected: false,
        exact: false,
        recoverable: true,
        resolver: format!("read {path}"),
        text,
    }
}

pub fn stable_rule_candidate_id(repository_id: &str, path: &str, source_hash: &str, identity: &str) -> String {
    digest_str(&format!("{repository_id}\0{path}\0{source_hash}\0{identity}"))
}

pub fn normalize_rule_path(path: &str) -> Result<String, &'static str> {
    let path = path.trim();
    if path.is_empty() || path.starts_with('/') || path.contains('\\') || path.contains('\0') {
        return Err("invalid_rule_path");
    }
    let mut parts = Vec::new();
    for part in path.split('/') {
        if part.is_empty() || part == "." || part == ".." { return Err("invalid_rule_path"); }
        parts.push(part);
    }
    Ok(parts.join("/"))
}

fn normalize_grant_path(entry: &str) -> Option<String> {
    let path = entry.split_once(':').map(|(path, _)| path).unwrap_or(entry);
    normalize_rule_path(path).ok()
}

fn authorized_paths_contains(grant: &membrane_provider_sdk::ValidatedScopeGrantView, path: &str) -> bool {
    grant.read_paths.iter().filter_map(|entry| normalize_grant_path(entry)).any(|entry| entry == path)
}

fn stable_detail(detail: &str) -> Option<String> { Some(detail.to_owned()) }

fn warning(reason: ReasonCode, detail: &str) -> ProviderWarningV1 {
    ProviderWarningV1 { provider: ProviderId::Rules, reason, severity: WarningSeverity::Warning, detail_id: stable_detail(detail), stage: Some("rules".into()), message: None }
}

fn omission(reason: ReasonCode, detail: &str) -> ProviderOmissionV1 {
    ProviderOmissionV1 { provider: ProviderId::Rules, reason, candidate_id: None, detail_id: stable_detail(detail), stage: Some("rules".into()) }
}
