//! Runtime-owned source bindings for native Pull federation.
//!
//! Providers receive typed owner handles. They do not open Cortex, catalog
//! storage, or Blueprint transport themselves.

use membrane_federation::blueprint_client::{BlueprintBounds, BlueprintClient, BlueprintQuery, ContextualBlueprintSource};
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
use std::collections::{BTreeSet, HashMap};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;

type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Runtime-owned source handles captured by one native federation
/// composition. Unbound owners remain absent so providers report an omission.
#[derive(Clone)]
pub struct NativeSourceBindings {
    pub(crate) ledger: Option<Arc<crate::ledger::service::LedgerService>>,
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
    pub(crate) temporal_queries: Arc<Mutex<HashMap<String, cortex_store::TemporalFactQuery>>>,
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
            .field("ledger", &self.ledger.is_some())
            .finish()
    }
}

impl NativeSourceBindings {
    /// Compose concrete runtime owners for one repository request.
    pub fn for_repository(
        repository_root: &Path,
        scope_grant_id: Option<&str>,
    ) -> Result<Self, String> {
        let store = crate::service::open_installed_store()?;
        Self::with_store(repository_root, scope_grant_id, store)
    }

    /// Hub-off hook bindings read the published Blueprint generation.  This
    /// path must not trigger a cold graph rebuild for freshness accounting.
    pub fn for_ambient_repository(
        repository_root: &Path,
        scope_grant_id: Option<&str>,
    ) -> Result<Self, String> {
        let store = crate::service::open_installed_lexical_store()?;
        Self::with_ambient_store(repository_root, scope_grant_id, store)
    }

    pub fn with_store(
        _repository_root: &Path,
        scope_grant_id: Option<&str>,
        store: crate::MemoryStore,
    ) -> Result<Self, String> {
        Self::with_store_and_deadline(_repository_root, scope_grant_id, store, None)
    }

    pub(crate) fn with_ambient_store(
        repository_root: &Path,
        scope_grant_id: Option<&str>,
        store: crate::MemoryStore,
    ) -> Result<Self, String> {
        Self::with_store_and_deadline_mode(repository_root, scope_grant_id, store, None, true)
    }

    pub(crate) fn with_store_and_deadline(
        _repository_root: &Path,
        scope_grant_id: Option<&str>,
        store: crate::MemoryStore,
        deadline: Option<membrane_federation::deadline::Deadline>,
    ) -> Result<Self, String> {
        Self::with_store_and_deadline_mode(_repository_root, scope_grant_id, store, deadline, false)
    }

    fn with_store_and_deadline_mode(
        _repository_root: &Path,
        scope_grant_id: Option<&str>,
        store: crate::MemoryStore,
        deadline: Option<membrane_federation::deadline::Deadline>,
        persisted_freshness: bool,
    ) -> Result<Self, String> {
        let catalog_path = crate::catalog::default_catalog_path()
            .map_err(|error| format!("resolve context catalog: {error}"))?;
        let catalog = if deadline.is_some() { None } else {
            Some(crate::catalog::ContextCatalog::open(&catalog_path)
                .map_err(|error| format!("open context catalog: {error}"))?)
        };
        let blueprint = Arc::new(BlueprintClient::from_operation(
            membrane_blueprint::native_blueprint_operation(),
        ));
        let cancellations = Arc::new(Mutex::new(HashMap::new()));
        let temporal_queries = Arc::new(Mutex::new(HashMap::new()));

        Ok(Self {
            ledger: crate::ledger::service::active_owner().or_else(|_| crate::ledger::service::open_explicit_owner()).ok(),
            audit: None,
            decisions: None,
            skills: Some(Arc::new(RuntimeSkillsSource {
                store: store.clone(),
            })),
            memory: Some(Arc::new(RuntimeMemorySource {
                store: store.clone(),
                cancellations: cancellations.clone(),
                temporal_queries: temporal_queries.clone(),
                lexical_only: persisted_freshness,
            })),
            scope_grant: Some(Arc::new(RuntimeScopeGrantSource {
                catalog,
                catalog_path,
                deadline,
                grant_id: scope_grant_id.map(str::to_owned),
            })),
            freshness: Some(Arc::new(RuntimeFreshnessSource { store, deadline, blueprint: blueprint.clone(), persisted_freshness })),
            blueprint: Some(blueprint.clone()),
            blueprint_contextual: Some(blueprint),
            release: Some(RuntimeReleaseSource),
            cancellations,
            temporal_queries,
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

#[derive(Clone)]
struct RuntimeMemorySource {
    store: crate::MemoryStore,
    cancellations: Arc<Mutex<HashMap<String, CancellationToken>>>,
    temporal_queries: Arc<Mutex<HashMap<String, cortex_store::TemporalFactQuery>>>,
    lexical_only: bool,
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
        let temporal_queries = self.temporal_queries.clone();
        let lexical_only = self.lexical_only;
        let query = query.clone();
        Box::pin(async move {
            let cancellation = cancellations
                .lock()
                .ok()
                .and_then(|tokens| tokens.get(&query.request_id).cloned())
                .unwrap_or_default();
            let descriptor = crate::scope::ScopeDescriptorV1::filesystem(&query.repository_root);
            let temporal = temporal_queries
                .lock()
                .ok()
                .and_then(|queries| queries.get(&query.request_id).cloned());
            let payload = if lexical_only {
                crate::pull::federation::memory_candidates_payload_for_descriptor_lexical(
                    &store, &query.task, &descriptor, 64, Some(Path::new(&query.repository_root)),
                )
            } else {
                crate::pull::federation::memory_candidates_payload_for_descriptor_cancellable_with_temporal(
                    &store, &query.task, &descriptor, 64,
                    Some(Path::new(&query.repository_root)), &cancellation, temporal,
                )
            }
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
                .and_then(|value| {
                    serde_json::from_value::<crate::store::CortexCompletenessV1>(value).ok()
                })
                .unwrap_or_else(|| {
                    crate::store::CortexCompletenessV1::lower_bound(
                        "completeness_unavailable",
                        candidates.len(),
                        candidates.len(),
                        0,
                    )
                });
            let warnings = completeness
                .causes
                .iter()
                .map(|cause| cortex_completeness_warning(cause, &query.request_id))
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

fn cortex_completeness_warning(cause: &str, request_id: &str) -> SourceWarning {
    let code = match cause {
        "cancelled" => "cancelled",
        "temporal_scope_rejected" => "scope_grant_invalid",
        "temporal_unavailable" | "completeness_unavailable" => "provider_unavailable",
        _ => "provider_failed",
    };
    SourceWarning {
        code: code.to_owned(),
        detail_id: Some(format!("{cause}:{request_id}")),
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
    deadline: Option<membrane_federation::deadline::Deadline>,
    blueprint: Arc<BlueprintClient>,
    persisted_freshness: bool,
}

fn persisted_blueprint_freshness(
    blueprint: &BlueprintClient,
    query: &SourceQuery,
) -> Result<SourceResponse<FreshnessSnapshotV1>, String> {
    let request = BlueprintQuery {
        request_id: format!("{}:freshness", query.request_id),
        repository_id: query.repository_id.clone(),
        repository_root: query.repository_root.clone(),
        worktree: query.repository_root.clone(),
        task: query.task.clone(),
        anchors: query.anchors.clone(),
        policy_digest: String::new(),
        expected_generation: None,
        symbol: None,
        bounds: BlueprintBounds { max_candidates: 1, max_paths: 8, max_response_bytes: 4096 },
        deadline: std::time::Duration::from_millis(1_200),
    };
    let result = blueprint
        .query(&request)
        .map_err(|error| error.to_string())?;
    let observation = result
        .payload
        .as_ref()
        .and_then(|payload| payload.get("sourceObservation"))
        .ok_or_else(|| "blueprint_source_observation_missing".to_owned())?;
    let indexed_head = observation.get("head").and_then(serde_json::Value::as_str)
        .ok_or_else(|| "blueprint_source_head_missing".to_owned())?;
    let indexed_status = observation.get("statusDigest").and_then(serde_json::Value::as_str)
        .ok_or_else(|| "blueprint_status_digest_missing".to_owned())?;
    let current = membrane_blueprint::git_source_observation::git_source_observation_at(Path::new(&query.repository_root));
    let (graph_state, stale, overlay_digest, complete, reason) = match current {
        Some(current) if current.head != indexed_head => (
            "stale_snapshot", true, Some(current.status_digest), false,
            Some("blueprint_generation_stale"),
        ),
        Some(current) if current.status_digest != indexed_status => (
            // Published graph does not contain current working-tree edits;
            // without an attached overlay, keep Blueprint explicitly stale.
            "stale_snapshot", true, Some(current.status_digest), false,
            Some("blueprint_overlay_unattached"),
        ),
        Some(_) => ("clean", false, None, true, None),
        None => ("indeterminate", true, None, false, Some("source_observation_unavailable")),
    };
    let snapshot_id = format!("blueprint:{}:{}", result.generation, indexed_head);
    Ok(SourceResponse {
        value: FreshnessSnapshotV1 {
            graph_state: graph_state.to_owned(),
            generation: Some(result.generation.clone()),
            snapshot_id: Some(snapshot_id),
            base_commit: Some(indexed_head.to_owned()),
            overlay_digest,
            stale,
        },
        generation: Some(result.generation),
        complete,
        warnings: reason.into_iter().map(|code| SourceWarning {
            code: code.to_owned(),
            detail_id: Some(query.request_id.clone()),
        }).collect(),
    })
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
        let deadline = self.deadline;
        let blueprint = self.blueprint.clone();
        let persisted_freshness = self.persisted_freshness;
        Box::pin(async move {
            if persisted_freshness {
                return persisted_blueprint_freshness(&blueprint, query)
                    .map_err(membrane_provider_sdk::ProviderError::Unavailable);
            }
            let verdict = crate::freshness::evaluate_repository_freshness_until(&store, root, deadline);
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
    catalog: Option<crate::catalog::ContextCatalog>,
    catalog_path: PathBuf,
    deadline: Option<membrane_federation::deadline::Deadline>,
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
        let catalog_path = self.catalog_path.clone();
        let deadline = self.deadline;
        let grant_id = self.grant_id.clone();
        let query = query.clone();
        Box::pin(async move {
            let Some(id) = grant_id else {
                return Err(membrane_provider_sdk::ProviderError::Unavailable(
                    "scope_grant_missing".into(),
                ));
            };
            let grant = match deadline {
                Some(deadline) => crate::catalog::lookup_grant_until(&catalog_path, &id, deadline),
                None => crate::catalog::lookup_grant(catalog.as_ref().expect("unbounded catalog owner"), &id)
                    .map_err(|error| error.to_string()),
            }
                .map_err(|error| {
                    membrane_provider_sdk::ProviderError::Unavailable(error.to_string())
                })?
                .ok_or_else(|| {
                    membrane_provider_sdk::ProviderError::Unavailable("scope_grant_missing".into())
                })?;
            let complete = grant.permits();
            if !grant.repository_ids.iter().any(|repository_id| repository_id == &query.repository_id) {
                return Err(membrane_provider_sdk::ProviderError::Unavailable(
                    "scope_grant_repository_mismatch".into(),
                ));
            }
            let value = membrane_provider_sdk::ScopeGrantView {
                id: grant.id,
                repository_id: query.repository_id,
                repository_root: query.repository_root,
                task_id: grant.task_id,
                session_id: grant.session_id,
                manifest_digest: grant.manifest_digest,
                blueprint_generation: query.generation.unwrap_or_else(|| "unknown".to_owned()),
                permitted_edge_types: grant.permitted_edge_types,
                read_paths: grant.read_paths.into_iter()
                    .map(|path| format!("{}:{}-{}", path.path, path.start_line, path.end_line))
                    .collect(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cortex_competitive_temporal_gaps_map_to_typed_provider_reasons() {
        let scope = cortex_completeness_warning("temporal_scope_rejected", "request-1");
        assert_eq!(scope.code, "scope_grant_invalid");
        assert_eq!(
            scope.detail_id.as_deref(),
            Some("temporal_scope_rejected:request-1")
        );
        let unavailable = cortex_completeness_warning("temporal_unavailable", "request-2");
        assert_eq!(unavailable.code, "provider_unavailable");
        let cancelled = cortex_completeness_warning("cancelled", "request-3");
        assert_eq!(cancelled.code, "cancelled");
    }

    #[test]
    fn cortex_competitive_native_memory_source_emits_requested_temporal_candidate() {
        let store = crate::MemoryStore::new();
        store.put(
            "release-memory",
            "release channel operational procedure",
            "global",
            cortex_core::MemoryTier::Semantic,
        );
        store
            .temporal_facts()
            .record(
                cortex_store::TemporalFact {
                    fact_id: "native-release-channel".to_owned(),
                    subject: "release".to_owned(),
                    predicate: "channel".to_owned(),
                    object: serde_json::json!("stable"),
                    scope_id: "global".to_owned(),
                    authority: "A1".to_owned(),
                    veracity: "supported".to_owned(),
                    observed_at: "2026-08-31T00:00:00Z".to_owned(),
                    valid_from: "2026-08-31T00:00:00Z".to_owned(),
                    valid_until: None,
                    expires_at: None,
                    supersedes: None,
                },
                true,
            )
            .unwrap();
        let request_id = "native-temporal-request".to_owned();
        let cancellation = CancellationToken::new();
        let cancellations = Arc::new(Mutex::new(HashMap::from([(
            request_id.clone(),
            cancellation,
        )])));
        let temporal_queries = Arc::new(Mutex::new(HashMap::from([(
            request_id.clone(),
            cortex_store::TemporalFactQuery {
                scope_chain: vec!["global".to_owned()],
                subject: "release".to_owned(),
                predicate: "channel".to_owned(),
                as_of: "2026-08-31T12:00:00Z".to_owned(),
            },
        )])));
        let source = RuntimeMemorySource {
            store,
            cancellations,
            temporal_queries,
            lexical_only: false,
        };
        let repository = tempfile::tempdir().unwrap();
        let query = SourceQuery {
            request_id,
            repository_id: "repository:test".to_owned(),
            repository_root: repository.path().to_string_lossy().into_owned(),
            task: "release channel operational procedure".to_owned(),
            session_id: "session-test".to_owned(),
            generation: Some("generation-test".to_owned()),
            anchors: Vec::new(),
        };
        let response = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(source.candidates(&query))
            .unwrap();
        let temporal = response
            .value
            .iter()
            .find(|candidate| candidate.id == "memory:temporal:native-release-channel")
            .expect("native Cortex source must emit requested temporal fact");
        assert_eq!(temporal.candidate.source_kind, "memory");
        assert_eq!(temporal.candidate.instruction_policy, "data_only");
        assert!(!temporal.candidate.recoverable);
        let fact: serde_json::Value = serde_json::from_str(&temporal.candidate.text).unwrap();
        assert_eq!(fact["factId"], "native-release-channel");
        assert_eq!(fact["object"], "stable");
    }
}
