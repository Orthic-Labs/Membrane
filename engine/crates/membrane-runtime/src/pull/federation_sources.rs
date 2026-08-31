//! Runtime-owned source bindings for native Pull federation.
//!
//! Providers receive typed owner handles. They do not open Cortex, catalog
//! storage, or Blueprint transport themselves.

use membrane_federation::blueprint_client::{
    BlueprintClient, ContextualBlueprintSource, UnixBlueprintTransport,
};
use membrane_federation::providers::rules::{
    DeliveryKey, DeliveryLedger, DeliveryMode, DeliveryReceipt, LedgerError, RuleDocument,
    RuleFuture, RuleSource, RuleSourceError, RuleSourceResponse,
};
use membrane_federation::release::{ReleaseError, ReleaseIdentity, ReleaseSource};
use membrane_protocol::{CandidateV1, FreshnessSnapshotV1};
use membrane_provider_sdk::{
    AuditFindingSource, BlueprintSource, DecisionRecordSource, FreshnessSource, MemoryCandidate,
    MemoryCandidateSource, ScopeGrantSource, SkillCatalogEntry, SkillCatalogSource, SourceQuery,
    SourceResponse, SourceResult, SourceSet, SourceWarning,
};
#[cfg(windows)]
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashMap};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;

type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Runtime-owned source handles captured by one native federation
/// composition. Empty owner state is represented by a typed empty response.
#[derive(Clone)]
pub struct NativeSourceBindings {
    pub audit: Option<Arc<dyn AuditFindingSource>>,
    pub decisions: Option<Arc<dyn DecisionRecordSource>>,
    pub skills: Option<Arc<dyn SkillCatalogSource>>,
    pub memory: Option<Arc<dyn MemoryCandidateSource>>,
    pub scope_grant: Option<Arc<dyn ScopeGrantSource>>,
    pub freshness: Option<Arc<dyn FreshnessSource>>,
    pub blueprint: Option<Arc<dyn BlueprintSource>>,
    pub blueprint_contextual: Option<Arc<dyn ContextualBlueprintSource>>,
    pub release: Option<RuntimeReleaseSource>,
    pub(crate) cancellations: Arc<Mutex<HashMap<String, CancellationToken>>>,
}

impl std::fmt::Debug for NativeSourceBindings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeSourceBindings")
            .field("audit", &self.audit.is_some())
            .field("decisions", &self.decisions.is_some())
            .field("skills", &self.skills.is_some())
            .field("memory", &self.memory.is_some())
            .field("scope_grant", &self.scope_grant.is_some())
            .field("freshness", &self.freshness.is_some())
            .field("blueprint", &self.blueprint.is_some())
            .field("blueprint_contextual", &self.blueprint_contextual.is_some())
            .field("release", &self.release.is_some())
            .finish()
    }
}

impl NativeSourceBindings {
    /// Compose concrete runtime owners for one repository request.
    pub fn for_repository(
        repository_root: &Path,
        scope_grant_id: Option<&str>,
    ) -> Result<Self, String> {
        let db_path = crate::pull::federation::db_path_for(repository_root);
        let db = crate::MemDb::open(&db_path)
            .map_err(|error| format!("open Cortex database: {error}"))?;
        let store = crate::MemoryStore::try_open(db)
            .map_err(|error| format!("open Cortex store: {error}"))?;
        let catalog_path = crate::catalog::default_catalog_path()
            .map_err(|error| format!("resolve context catalog: {error}"))?;
        let catalog = crate::catalog::ContextCatalog::open(catalog_path)
            .map_err(|error| format!("open context catalog: {error}"))?;
        let endpoint = hub_blueprint_endpoint()?;
        let blueprint = Arc::new(BlueprintClient::new(Arc::new(UnixBlueprintTransport::new(
            endpoint,
        ))));
        let cancellations = Arc::new(Mutex::new(HashMap::new()));

        Ok(Self {
            audit: Some(Arc::new(EmptyAuditSource)),
            decisions: Some(Arc::new(EmptyDecisionSource)),
            skills: Some(Arc::new(RuntimeSkillsSource {
                store: store.clone(),
            })),
            memory: Some(Arc::new(RuntimeMemorySource {
                store: store.clone(),
                cancellations: cancellations.clone(),
            })),
            scope_grant: Some(Arc::new(RuntimeScopeGrantSource {
                catalog,
                grant_id: scope_grant_id.map(str::to_owned),
            })),
            freshness: Some(Arc::new(RuntimeFreshnessSource { store })),
            blueprint: Some(blueprint.clone()),
            blueprint_contextual: Some(blueprint),
            release: Some(RuntimeReleaseSource),
            cancellations,
        })
    }

    pub fn source_set(&self) -> SourceSet {
        SourceSet {
            audit: self.audit.clone(),
            decisions: self.decisions.clone(),
            skills: self.skills.clone(),
            memory: self.memory.clone(),
            scope_grant: self.scope_grant.clone(),
            freshness: self.freshness.clone(),
            blueprint: self.blueprint.clone(),
        }
    }
}

fn hub_blueprint_endpoint() -> Result<PathBuf, String> {
    if let Some(endpoint) = std::env::var_os("BLUEPRINT_DAEMON_ENDPOINT") {
        return Ok(PathBuf::from(endpoint));
    }
    #[cfg(windows)]
    {
        let profile =
            std::env::var("USERPROFILE").map_err(|_| "USERPROFILE is unavailable".to_owned())?;
        let suffix = hex::encode(Sha256::digest(profile.as_bytes()));
        return Ok(PathBuf::from(format!(
            r"\\.\pipe\membrane-blueprint-{}",
            &suffix[..16]
        )));
    }
    #[cfg(not(windows))]
    {
        let home = std::env::var_os("HOME").ok_or_else(|| "HOME is unavailable".to_owned())?;
        Ok(PathBuf::from(home)
            .join(".blueprint")
            .join("blueprint.sock"))
    }
}

/// Runtime-owned release identity.  Its generation is compiled into this
/// binary by `release_identity`; it never falls back to repository contents.
#[derive(Clone, Copy, Debug, Default)]
pub struct RuntimeReleaseSource;

impl RuntimeReleaseSource {
    pub fn generation() -> Result<String, String> {
        RuntimeReleaseSource
            .current_release()
            .map(|identity| identity.generation)
            .map_err(|error| error.to_string())
    }
}

impl ReleaseSource for RuntimeReleaseSource {
    fn current_release(&self) -> Result<ReleaseIdentity, ReleaseError> {
        ReleaseIdentity::new(
            crate::release_identity::release_generation(),
            "membrane-runtime.release_identity",
            Some("release_identity::release_generation".to_owned()),
        )
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct EmptyAuditSource;

impl AuditFindingSource for EmptyAuditSource {
    fn findings<'a, 'b, 'c>(
        &'a self,
        query: &'b SourceQuery,
    ) -> BoxFuture<'c, SourceResult<Vec<membrane_provider_sdk::AuditFinding>>>
    where
        'a: 'c,
        'b: 'c,
        Self: 'c,
    {
        Box::pin(async {
            Ok(SourceResponse {
                value: Vec::new(),
                generation: None,
                complete: true,
                warnings: Vec::new(),
            })
        })
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct EmptyDecisionSource;

impl DecisionRecordSource for EmptyDecisionSource {
    fn decisions<'a, 'b, 'c>(
        &'a self,
        _query: &'b SourceQuery,
    ) -> BoxFuture<'c, SourceResult<Vec<membrane_provider_sdk::DecisionRecord>>>
    where
        'a: 'c,
        'b: 'c,
        Self: 'c,
    {
        Box::pin(async {
            Ok(SourceResponse {
                value: Vec::new(),
                generation: None,
                complete: true,
                warnings: Vec::new(),
            })
        })
    }
}

#[derive(Clone)]
struct RuntimeMemorySource {
    store: crate::MemoryStore,
    cancellations: Arc<Mutex<HashMap<String, CancellationToken>>>,
}

impl MemoryCandidateSource for RuntimeMemorySource {
    fn candidates<'a, 'b, 'c>(
        &'a self,
        query: &'b SourceQuery,
    ) -> BoxFuture<'c, SourceResult<Vec<MemoryCandidate>>>
    where
        'a: 'c,
        'b: 'c,
        Self: 'c,
    {
        let store = self.store.clone();
        let cancellations = self.cancellations.clone();
        let query = query.clone();
        Box::pin(async move {
            let cancellation = cancellations
                .lock()
                .ok()
                .and_then(|tokens| tokens.get(&query.request_id).cloned())
                .unwrap_or_default();
            let descriptor = crate::scope::ScopeDescriptorV1::filesystem(&query.repository_root);
            let payload = crate::pull::federation::memory_candidates_payload_for_descriptor_cancellable(
                &store,
                &query.task,
                &descriptor,
                64,
                Some(Path::new(&query.repository_root)),
                &cancellation,
            )
            .map_err(membrane_provider_sdk::ProviderError::Unavailable)?;
            let generation = query
                .generation
                .clone()
                .unwrap_or_else(|| "runtime-memory".to_owned());
            let values = payload
                .get("candidates")
                .and_then(serde_json::Value::as_array)
                .cloned()
                .unwrap_or_default();
            let mut candidates = Vec::with_capacity(values.len());
            for value in values {
                let candidate: CandidateV1 = serde_json::from_value(value).map_err(|error| {
                    membrane_provider_sdk::ProviderError::MalformedOutput(error.to_string())
                })?;
                candidates.push(MemoryCandidate {
                    id: candidate.id.clone(),
                    repository_id: query.repository_id.clone(),
                    generation: generation.clone(),
                    source_hash: candidate.source_hash.clone(),
                    candidate,
                });
            }
            let completeness = payload
                .get("completeness")
                .cloned()
                .and_then(|value| serde_json::from_value::<crate::store::CortexCompletenessV1>(value).ok())
                .unwrap_or_else(|| crate::store::CortexCompletenessV1::lower_bound(
                    "completeness_unavailable", candidates.len(), candidates.len(), 0
                ));
            let warnings = completeness
                .causes
                .iter()
                .map(|cause| SourceWarning {
                    code: cause.clone(),
                    detail_id: Some(query.request_id.clone()),
                })
                .collect();
            Ok(SourceResponse {
                value: candidates,
                generation: Some(generation),
                complete: completeness.is_exact(),
                warnings,
            })
        })
    }
}

#[derive(Clone)]
struct RuntimeSkillsSource {
    store: crate::MemoryStore,
}

impl SkillCatalogSource for RuntimeSkillsSource {
    fn skills<'a, 'b, 'c>(
        &'a self,
        query: &'b SourceQuery,
    ) -> BoxFuture<'c, SourceResult<Vec<SkillCatalogEntry>>>
    where
        'a: 'c,
        'b: 'c,
        Self: 'c,
    {
        let store = self.store.clone();
        let repository_id = query.repository_id.clone();
        Box::pin(async move {
            let snapshot = store
                .skills_snapshot()
                .map_err(membrane_provider_sdk::ProviderError::Unavailable)?;
            let generation = snapshot.generation.clone();
            let results = store
                .search_skills(&query.task, 64)
                .map_err(membrane_provider_sdk::ProviderError::Unavailable)?;
            let value = results
                .items
                .into_iter()
                .map(|entry| SkillCatalogEntry {
                    id: entry.name,
                    repository_id: repository_id.clone(),
                    generation: generation.clone(),
                    source_hash: entry.body_hash,
                    title: entry.description,
                    keywords: Vec::new(),
                })
                .collect();
            Ok(SourceResponse {
                value,
                generation: Some(generation),
                complete: results.completeness.is_exact(),
                warnings: results
                    .completeness
                    .causes
                    .into_iter()
                    .map(|code| SourceWarning {
                        code,
                        detail_id: Some(query.request_id.clone()),
                    })
                    .collect(),
            })
        })
    }
}

#[derive(Clone)]
struct RuntimeFreshnessSource {
    store: crate::MemoryStore,
}

impl FreshnessSource for RuntimeFreshnessSource {
    fn freshness<'a, 'b, 'c>(
        &'a self,
        query: &'b SourceQuery,
    ) -> BoxFuture<'c, SourceResult<FreshnessSnapshotV1>>
    where
        'a: 'c,
        'b: 'c,
        Self: 'c,
    {
        let store = self.store.clone();
        let root = PathBuf::from(&query.repository_root);
        Box::pin(async move {
            let verdict = crate::freshness::evaluate_repository_freshness(&store, root);
            let graph_state = serde_json::to_string(&verdict.graph_state)
                .unwrap_or_else(|_| "\"indeterminate\"".to_owned())
                .trim_matches('"')
                .to_owned();
            let stale = !verdict.stable
                || matches!(
                    verdict.graph_state,
                    crate::freshness::GraphState::StaleSnapshot
                );
            let warning = (!verdict.stable).then(|| SourceWarning {
                code: "freshness_unavailable".to_owned(),
                detail_id: verdict.reasons.first().cloned(),
            });
            Ok(SourceResponse {
                value: FreshnessSnapshotV1 {
                    graph_state,
                    generation: verdict.blueprint_generation.clone(),
                    snapshot_id: Some(verdict.snapshot_id),
                    base_commit: verdict.base_commit,
                    overlay_digest: Some(verdict.overlay_digest),
                    stale,
                },
                generation: verdict.blueprint_generation,
                complete: verdict.stable,
                warnings: warning.into_iter().collect(),
            })
        })
    }
}

#[derive(Clone)]
struct RuntimeScopeGrantSource {
    catalog: crate::catalog::ContextCatalog,
    grant_id: Option<String>,
}

impl ScopeGrantSource for RuntimeScopeGrantSource {
    fn grant<'a, 'b, 'c>(
        &'a self,
        query: &'b SourceQuery,
    ) -> BoxFuture<'c, SourceResult<membrane_provider_sdk::ScopeGrantView>>
    where
        'a: 'c,
        'b: 'c,
        Self: 'c,
    {
        let catalog = self.catalog.clone();
        let grant_id = self.grant_id.clone();
        let query = query.clone();
        Box::pin(async move {
            let Some(id) = grant_id else {
                return Err(membrane_provider_sdk::ProviderError::Unavailable(
                    "scope_grant_missing".into(),
                ));
            };
            let grant = crate::catalog::lookup_grant(&catalog, &id)
                .map_err(|error| {
                    membrane_provider_sdk::ProviderError::Unavailable(error.to_string())
                })?
                .ok_or_else(|| {
                    membrane_provider_sdk::ProviderError::Unavailable("scope_grant_missing".into())
                })?;
            let complete = grant.permits();
            let value = membrane_provider_sdk::ScopeGrantView {
                id: grant.id,
                repository_id: query.repository_id,
                repository_root: query.repository_root,
                task_id: grant.task_id,
                session_id: grant.session_id,
                manifest_digest: grant.manifest_digest,
                blueprint_generation: query.generation.unwrap_or_else(|| "unknown".to_owned()),
                permitted_edge_types: grant.permitted_edge_types,
                read_paths: Vec::new(),
            };
            Ok(SourceResponse {
                value,
                generation: None,
                complete,
                warnings: Vec::new(),
            })
        })
    }
}

/// Filesystem owner adapter for grant-authorized rule paths.
#[derive(Clone, Copy, Debug, Default)]
pub struct RuntimeRuleSource;

impl RuleSource for RuntimeRuleSource {
    fn read_rules(
        &self,
        query: SourceQuery,
        authorized_paths: Vec<String>,
    ) -> RuleFuture<RuleSourceResponse> {
        Box::pin(async move {
            let root = PathBuf::from(&query.repository_root)
                .canonicalize()
                .map_err(|error| RuleSourceError::Unavailable(error.to_string()))?;
            let mut documents = Vec::new();
            for relative in authorized_paths {
                let candidate = root.join(&relative);
                let path = candidate
                    .canonicalize()
                    .map_err(|error| RuleSourceError::Unavailable(error.to_string()))?;
                if !path.starts_with(&root) {
                    return Err(RuleSourceError::Unauthorized(
                        "path_outside_repository".into(),
                    ));
                }
                let content = std::fs::read_to_string(&path)
                    .map_err(|error| RuleSourceError::Unavailable(error.to_string()))?;
                documents.push(RuleDocument {
                    repository_id: query.repository_id.clone(),
                    normalized_path: relative.clone(),
                    rule_identity: relative,
                    source_hash: None,
                    trust_class: "workspace_tracked".into(),
                    instruction_policy: "data_only".into(),
                    content,
                });
            }
            Ok(RuleSourceResponse {
                documents,
                generation: query.generation,
                complete: true,
            })
        })
    }
}

/// Process-local ledger for non-self-loading clients. It retains only
/// identity hashes, never rule bodies or prompt text.
#[derive(Clone, Default)]
pub struct RuntimeDeliveryLedger {
    claimed: Arc<Mutex<BTreeSet<String>>>,
}

impl DeliveryLedger for RuntimeDeliveryLedger {
    fn claim(
        &self,
        key: DeliveryKey,
    ) -> Pin<Box<dyn Future<Output = Result<DeliveryReceipt, LedgerError>> + Send>> {
        let claimed = self.claimed.clone();
        Box::pin(async move {
            let identity = format!(
                "{}\n{}\n{}\n{}\n{}",
                key.repository_id, key.client, key.session_id, key.candidate_id, key.source_hash
            );
            let digest = <sha2::Sha256 as sha2::Digest>::digest(identity.as_bytes());
            let receipt_id = format!("native-delivery-{}", hex::encode(digest));
            let mut claimed = claimed
                .lock()
                .map_err(|_| LedgerError::Unavailable("ledger_poisoned".into()))?;
            let first = claimed.insert(receipt_id.clone());
            Ok(DeliveryReceipt {
                receipt_id,
                mode: if first {
                    DeliveryMode::Inline
                } else {
                    DeliveryMode::Reference
                },
            })
        })
    }
}
