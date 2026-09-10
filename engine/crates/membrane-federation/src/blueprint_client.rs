//! Bounded, direct native access to Blueprint for federation.
//!
//! Federation composes an injected [`membrane_blueprint::BlueprintApi`]. It
//! neither starts a process nor opens a socket/pipe; Blueprint owns graph
//! semantics, storage, resident lifecycle & response construction.

use membrane_blueprint::{
    BlueprintApi, BlueprintError, BlueprintOperation, BlueprintRequest, BlueprintResponse,
    Bounds as NativeBounds, CancellationToken as NativeCancellation, OneShotExecutor, Operation,
};
use membrane_protocol::CandidateV1;
use membrane_provider_sdk::source::{
    BlueprintResult, BlueprintSource, SourceQuery, SourceResponse, SourceResult, SourceWarning,
};
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

pub const DEFAULT_CANDIDATE_CAP: usize = 64;
pub const MAX_CANDIDATE_CAP: usize = 256;
pub const DEFAULT_PATH_CAP: usize = 256;
pub const DEFAULT_RESPONSE_BYTES: usize = 16 * 1024;
pub const DEFAULT_CACHE_TTL: Duration = Duration::from_secs(300);
pub const DEFAULT_CACHE_ENTRIES: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlueprintBounds {
    pub max_candidates: usize,
    pub max_paths: usize,
    pub max_response_bytes: usize,
}

impl Default for BlueprintBounds {
    fn default() -> Self {
        Self {
            max_candidates: DEFAULT_CANDIDATE_CAP,
            max_paths: DEFAULT_PATH_CAP,
            max_response_bytes: DEFAULT_RESPONSE_BYTES,
        }
    }
}

impl BlueprintBounds {
    pub fn bounded(self) -> Self {
        Self {
            max_candidates: self.max_candidates.clamp(1, MAX_CANDIDATE_CAP),
            max_paths: self.max_paths.max(1),
            max_response_bytes: self.max_response_bytes.clamp(1024, DEFAULT_RESPONSE_BYTES),
        }
    }

    fn native(self) -> NativeBounds {
        NativeBounds {
            max_request_bytes: DEFAULT_RESPONSE_BYTES,
            max_frame_bytes: DEFAULT_RESPONSE_BYTES,
            max_response_bytes: self.max_response_bytes,
            max_candidates: self.max_candidates,
            max_paths: self.max_paths,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BlueprintCacheKey {
    pub repository_id: String,
    pub worktree: String,
    pub query_digest: String,
    pub symbol: Option<String>,
    pub anchors: Vec<String>,
    pub policy_digest: String,
    pub expected_generation: Option<String>,
    pub max_candidates: usize,
    pub max_paths: usize,
    pub max_response_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlueprintQuery {
    pub request_id: String,
    pub repository_id: String,
    pub repository_root: String,
    pub worktree: String,
    pub task: String,
    pub anchors: Vec<String>,
    pub policy_digest: String,
    pub expected_generation: Option<String>,
    pub symbol: Option<String>,
    pub bounds: BlueprintBounds,
    pub deadline: Duration,
}

impl BlueprintQuery {
    pub fn cache_key(&self) -> BlueprintCacheKey {
        BlueprintCacheKey {
            repository_id: self.repository_id.clone(),
            worktree: self.worktree.clone(),
            query_digest: stable_digest(&self.task),
            symbol: self.symbol.clone(),
            anchors: self.anchors.clone(),
            policy_digest: self.policy_digest.clone(),
            expected_generation: self.expected_generation.clone(),
            max_candidates: self.bounds.max_candidates,
            max_paths: self.bounds.max_paths,
            max_response_bytes: self.bounds.max_response_bytes,
        }
    }

    pub fn from_source(
        source: &SourceQuery,
        expected: Option<String>,
        bounds: BlueprintBounds,
        deadline: Duration,
    ) -> Self {
        Self {
            request_id: source.request_id.clone(),
            repository_id: source.repository_id.clone(),
            repository_root: source.repository_root.clone(),
            worktree: source.repository_root.clone(),
            task: source.task.clone(),
            anchors: source.anchors.clone(),
            policy_digest: String::new(),
            expected_generation: expected.or_else(|| source.generation.clone()),
            symbol: None,
            bounds: bounds.bounded(),
            deadline,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BlueprintClientError {
    #[error("native Blueprint is unavailable: {0}")]
    Unavailable(String),
    #[error("Blueprint request deadline exhausted")]
    Timeout,
    #[error("Blueprint request was cancelled")]
    Cancelled,
    #[error("Blueprint response is malformed: {0}")]
    Malformed(String),
    #[error("Blueprint response exceeded a configured bound: {0}")]
    Oversized(&'static str),
    #[error("Blueprint generation mismatch: expected {expected}, observed {observed}")]
    GenerationMismatch { expected: String, observed: String },
    #[error("native Blueprint returned typed error {code}: {message}")]
    Remote { code: String, message: String },
}

impl BlueprintClientError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Unavailable(_) => "blueprint_unavailable",
            Self::Timeout => "provider_timeout",
            Self::Cancelled => "provider_cancelled",
            Self::Malformed(_) => "blueprint_malformed",
            Self::Oversized(_) => "blueprint_oversized",
            Self::GenerationMismatch { .. } => "blueprint_stale",
            Self::Remote { code, .. } => match code.as_str() {
                "request_cancelled" => "provider_cancelled",
                "deadline_exceeded" => "provider_timeout",
                "generation_mismatch" => "blueprint_stale",
                _ => "blueprint_remote_error",
            },
        }
    }
}

#[derive(Clone)]
struct CacheEntry { key: BlueprintCacheKey, expires_at: Instant, value: BlueprintResult }
#[derive(Default)]
struct CacheState { entries: VecDeque<CacheEntry> }

/// Direct native Blueprint client. Injected API can be resident service or
/// one-shot executor; no federation fallback reaches legacy transport.
pub struct BlueprintClient {
    api: Arc<dyn BlueprintApi>,
    cache: Mutex<CacheState>,
    cache_ttl: Duration,
    cache_entries: usize,
}

impl BlueprintClient {
    pub fn new(api: Arc<dyn BlueprintApi>) -> Self {
        Self { api, cache: Mutex::new(CacheState::default()), cache_ttl: DEFAULT_CACHE_TTL, cache_entries: DEFAULT_CACHE_ENTRIES }
    }

    /// Bind direct native graph operation for Hub-off federation. The native
    /// one-shot executor preserves Blueprint validation & typed responses.
    pub fn from_operation(operation: Arc<dyn BlueprintOperation>) -> Self {
        Self::new(Arc::new(OneShotExecutor::new(operation)))
    }

    pub fn with_cache_limits(mut self, ttl: Duration, entries: usize) -> Self {
        self.cache_ttl = ttl;
        self.cache_entries = entries.max(1);
        self
    }

    pub fn query(&self, query: &BlueprintQuery) -> Result<BlueprintResult, BlueprintClientError> {
        self.query_with_cancellation(query, CancellationToken::new())
    }

    pub fn query_with_cancellation(&self, query: &BlueprintQuery, cancellation: CancellationToken) -> Result<BlueprintResult, BlueprintClientError> {
        let query = bounded_query(query, cancellation.clone())?;
        let key = query.cache_key();
        if query.expected_generation.as_deref().is_some_and(|value| !value.is_empty()) {
            if let Some(value) = self.cached(&key) { return Ok(value); }
        }
        let value = self.dispatch(&query, Operation::Recall, cancellation)?;
        if query.expected_generation.as_deref().is_some_and(|value| !value.is_empty()) { self.insert(key, value.clone()); }
        Ok(value)
    }

    pub fn resolve_symbol(&self, query: &BlueprintQuery, symbol: &str) -> Result<BlueprintResult, BlueprintClientError> {
        self.resolve_symbol_with_cancellation(query, symbol, CancellationToken::new())
    }

    pub fn resolve_symbol_with_cancellation(&self, query: &BlueprintQuery, symbol: &str, cancellation: CancellationToken) -> Result<BlueprintResult, BlueprintClientError> {
        let mut query = bounded_query(query, cancellation.clone())?;
        query.symbol = Some(symbol.to_owned());
        self.dispatch(&query, Operation::Resolve, cancellation)
    }

    fn dispatch(&self, query: &BlueprintQuery, method: Operation, cancellation: CancellationToken) -> Result<BlueprintResult, BlueprintClientError> {
        if cancellation.is_cancelled() { return Err(BlueprintClientError::Cancelled); }
        let bounds = query.bounds.bounded();
        let request = native_request(query, method);
        // Blueprint's API is synchronous, so bridge caller cancellation into
        // its native token while dispatch is in flight. This keeps cancellation
        // observable by operations that checkpoint their RequestContext rather
        // than only checking once transport returns.
        let native_cancellation = NativeCancellation::new();
        let watcher_token = native_cancellation.clone();
        let caller_cancellation = cancellation.clone();
        let deadline = Instant::now().checked_add(query.deadline);
        let watcher = std::thread::spawn(move || {
            while !watcher_token.is_cancelled() {
                if caller_cancellation.is_cancelled() || deadline.is_some_and(|at| Instant::now() >= at) {
                    watcher_token.cancel();
                    break;
                }
                std::thread::sleep(Duration::from_millis(1));
            }
        });
        let response = self.api.dispatch(request.clone(), native_cancellation.clone());
        let dispatch_finished_at = Instant::now();
        // Stop & join watcher before returning so no background thread retains
        // caller state after this synchronous boundary has completed.
        native_cancellation.cancel();
        let _ = watcher.join();
        if cancellation.is_cancelled() { return Err(BlueprintClientError::Cancelled); }
        if deadline.is_some_and(|at| dispatch_finished_at >= at) { return Err(BlueprintClientError::Timeout); }
        parse_result(response, &request, query, bounds)
    }

    fn cached(&self, key: &BlueprintCacheKey) -> Option<BlueprintResult> {
        let mut cache = self.cache.lock().ok()?;
        let now = Instant::now();
        cache.entries.retain(|entry| entry.expires_at > now);
        cache.entries.iter().find(|entry| entry.key == *key).map(|entry| entry.value.clone())
    }

    fn insert(&self, key: BlueprintCacheKey, value: BlueprintResult) {
        let Ok(mut cache) = self.cache.lock() else { return; };
        cache.entries.retain(|entry| entry.expires_at > Instant::now() && entry.key != key);
        cache.entries.push_back(CacheEntry { key, expires_at: Instant::now() + self.cache_ttl, value });
        while cache.entries.len() > self.cache_entries { cache.entries.pop_front(); }
    }
}

impl BlueprintSource for BlueprintClient {
    fn query<'life0, 'life1, 'async_trait>(&'life0 self, _source: &'life1 SourceQuery) -> Pin<Box<dyn Future<Output = SourceResult<BlueprintResult>> + Send + 'async_trait>>
    where 'life0: 'async_trait, 'life1: 'async_trait, Self: 'async_trait {
        Box::pin(async { Err(membrane_provider_sdk::ProviderError::MissingSource("blueprint_context")) })
    }

    fn resolve_symbol<'life0, 'life1, 'life2, 'async_trait>(&'life0 self, _source: &'life1 SourceQuery, _symbol: &'life2 str) -> Pin<Box<dyn Future<Output = SourceResult<BlueprintResult>> + Send + 'async_trait>>
    where 'life0: 'async_trait, 'life1: 'async_trait, 'life2: 'async_trait, Self: 'async_trait {
        Box::pin(async { Err(membrane_provider_sdk::ProviderError::MissingSource("blueprint_context")) })
    }
}

/// Request-aware seam used by federation so caller deadline & cancellation
/// remain part of native dispatch rather than transport-local policy.
pub trait ContextualBlueprintSource: Send + Sync {
    fn query_with_context<'life0, 'life1, 'async_trait>(&'life0 self, source: &'life1 SourceQuery, deadline: Instant, cancellation: CancellationToken) -> Pin<Box<dyn Future<Output = SourceResult<BlueprintResult>> + Send + 'async_trait>>
    where 'life0: 'async_trait, 'life1: 'async_trait, Self: 'async_trait;
    fn resolve_symbol_with_context<'life0, 'life1, 'life2, 'async_trait>(&'life0 self, source: &'life1 SourceQuery, symbol: &'life2 str, deadline: Instant, cancellation: CancellationToken) -> Pin<Box<dyn Future<Output = SourceResult<BlueprintResult>> + Send + 'async_trait>>
    where 'life0: 'async_trait, 'life1: 'async_trait, 'life2: 'async_trait, Self: 'async_trait;
}

impl ContextualBlueprintSource for BlueprintClient {
    fn query_with_context<'life0, 'life1, 'async_trait>(&'life0 self, source: &'life1 SourceQuery, deadline: Instant, cancellation: CancellationToken) -> Pin<Box<dyn Future<Output = SourceResult<BlueprintResult>> + Send + 'async_trait>>
    where 'life0: 'async_trait, 'life1: 'async_trait, Self: 'async_trait {
        Box::pin(async move {
            let query = BlueprintQuery::from_source(source, source.generation.clone(), BlueprintBounds::default(), deadline.saturating_duration_since(Instant::now()));
            let result = self.query_with_cancellation(&query, cancellation).map_err(client_error)?;
            Ok(source_response(result))
        })
    }

    fn resolve_symbol_with_context<'life0, 'life1, 'life2, 'async_trait>(&'life0 self, source: &'life1 SourceQuery, symbol: &'life2 str, deadline: Instant, cancellation: CancellationToken) -> Pin<Box<dyn Future<Output = SourceResult<BlueprintResult>> + Send + 'async_trait>>
    where 'life0: 'async_trait, 'life1: 'async_trait, 'life2: 'async_trait, Self: 'async_trait {
        let symbol = symbol.to_owned();
        Box::pin(async move {
            let query = BlueprintQuery::from_source(source, source.generation.clone(), BlueprintBounds::default(), deadline.saturating_duration_since(Instant::now()));
            let result = self.resolve_symbol_with_cancellation(&query, &symbol, cancellation).map_err(client_error)?;
            Ok(source_response(result))
        })
    }
}

fn bounded_query(query: &BlueprintQuery, cancellation: CancellationToken) -> Result<BlueprintQuery, BlueprintClientError> {
    if cancellation.is_cancelled() { return Err(BlueprintClientError::Cancelled); }
    if query.deadline.is_zero() { return Err(BlueprintClientError::Timeout); }
    Ok(BlueprintQuery { bounds: query.bounds.bounded(), ..query.clone() })
}

fn native_request(query: &BlueprintQuery, method: Operation) -> BlueprintRequest {
    let mut input = json!({ "repoRoot": query.repository_root, "worktree": query.worktree, "task": query.task, "limit": query.bounds.max_candidates, "anchors": query.anchors, "allowStale": true });
    if let Some(symbol) = &query.symbol {
        // Blueprint owns Resolve semantics.  `target` is its stable request
        // field; retain `symbol` as a compatible alias for older producers.
        input["target"] = Value::String(symbol.clone());
        input["symbol"] = Value::String(symbol.clone());
    }
    let request_id = if query.request_id.trim().is_empty() {
        "blueprint-request".to_owned()
    } else {
        query.request_id.clone()
    };
    let mut request = BlueprintRequest::new(request_id, method, &query.repository_root);
    request.repo_id = (!query.repository_id.trim().is_empty()).then(|| query.repository_id.clone());
    request.generation = query.expected_generation.clone();
    request.deadline_ms = query.deadline.as_millis().clamp(10, 30_000) as u64;
    if let Some(object) = input.as_object_mut() {
        if let Some(repo_id) = request.repo_id.clone() { object.insert("repoId".into(), Value::String(repo_id)); }
        if let Some(generation) = request.generation.clone() { object.insert("generation".into(), Value::String(generation)); }
    }
    request.input = input;
    request
}

/// Preserve Blueprint's own terminal disposition at the source boundary.
/// Federation may translate this metadata, but must not promote a bounded,
/// ambiguous, stale, or otherwise incomplete native result to complete.
fn source_response(result: BlueprintResult) -> SourceResponse<BlueprintResult> {
    let payload = result.payload.as_ref();
    let state = payload.and_then(|value| value.get("state")).and_then(Value::as_str);
    let omissions = payload
        .and_then(|value| value.get("omissions"))
        .and_then(Value::as_array);
    let complete = payload
        .and_then(|value| value.get("complete"))
        .and_then(Value::as_bool)
        .unwrap_or_else(|| state == Some("complete") && omissions.is_none_or(Vec::is_empty));
    let mut warnings = payload
        .and_then(|value| value.get("warnings"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|warning| {
            let code = warning.get("code").or_else(|| warning.get("reason"))?.as_str()?;
            Some(SourceWarning {
                code: code.to_owned(),
                detail_id: warning.get("detailId").and_then(Value::as_str).map(str::to_owned),
            })
        })
        .collect::<Vec<_>>();
    if !complete && warnings.is_empty() {
        warnings.push(SourceWarning {
            code: state.unwrap_or("blueprint_incomplete").to_owned(),
            detail_id: omissions.and_then(|values| values.first())
                .and_then(|value| value.get("reason"))
                .and_then(Value::as_str).map(str::to_owned),
        });
    }
    SourceResponse { generation: Some(result.generation.clone()), complete, warnings, value: result }
}

fn client_error(error: BlueprintClientError) -> membrane_provider_sdk::ProviderError {
    use membrane_provider_sdk::ProviderError;
    match error {
        BlueprintClientError::Timeout => ProviderError::DeadlineExceeded,
        BlueprintClientError::Cancelled => ProviderError::Cancelled,
        BlueprintClientError::Unavailable(message) => ProviderError::Unavailable(message),
        BlueprintClientError::Remote { code, message } => ProviderError::Typed { operation: "blueprint".into(), code, message, details: None },
        BlueprintClientError::GenerationMismatch { expected, observed } => ProviderError::IdentityMismatch(format!("expected {expected}, observed {observed}")),
        other => ProviderError::SourceFailure(other.to_string()),
    }
}

fn parse_result(response: BlueprintResponse, request: &BlueprintRequest, query: &BlueprintQuery, bounds: BlueprintBounds) -> Result<BlueprintResult, BlueprintClientError> {
    response.validate(bounds.native()).map_err(native_error)?;
    if response.request_id.as_deref() != Some(request.request_id.as_str()) { return Err(BlueprintClientError::Malformed("response request identity mismatch".into())); }
    if !response.ok {
        return Err(native_error(response.error.unwrap_or_else(|| BlueprintError::new("blueprint_unavailable", "request failed"))));
    }
    let raw = response.result.ok_or_else(|| BlueprintClientError::Malformed("successful response has no result".into()))?;
    let observed = response.generation.or_else(|| raw.get("generationId").and_then(Value::as_str).map(str::to_owned))
        .ok_or_else(|| BlueprintClientError::Malformed("response has no generation identity".into()))?;
    if let Some(expected) = query.expected_generation.as_deref().filter(|value| !value.is_empty()) {
        if observed != expected { return Err(BlueprintClientError::GenerationMismatch { expected: expected.to_owned(), observed }); }
    }
    let candidates = raw.get("candidateSet").and_then(|set| set.get("candidates")).or_else(|| raw.get("candidates"))
        .ok_or_else(|| BlueprintClientError::Malformed("recall result has no candidates".into()))?
        .as_array().ok_or_else(|| BlueprintClientError::Malformed("candidates is not an array".into()))?;
    if candidates.len() > bounds.max_candidates { return Err(BlueprintClientError::Oversized("candidate_count")); }
    let mut paths = std::collections::BTreeSet::new();
    for value in candidates {
        let path = value.get("sourceRef").and_then(Value::as_str).ok_or_else(|| BlueprintClientError::Malformed("candidate sourceRef is missing".into()))?;
        if path.len() > 4096 { return Err(BlueprintClientError::Oversized("path_length")); }
        paths.insert(path);
        if paths.len() > bounds.max_paths { return Err(BlueprintClientError::Oversized("path_count")); }
    }
    const PROVENANCE: [&str; 3] = ["recallCircuitId", "evidencePathId", "evidenceEnvelope"];
    let typed = candidates.iter().cloned().map(|mut value| {
        if let Some(object) = value.as_object_mut() { for key in PROVENANCE { object.remove(key); } }
        serde_json::from_value::<CandidateV1>(value).map_err(|error| BlueprintClientError::Malformed(error.to_string()))
    }).collect::<Result<Vec<_>, _>>()?;
    Ok(BlueprintResult { generation: observed, candidates: typed, payload: Some(raw) })
}

fn native_error(error: BlueprintError) -> BlueprintClientError {
    match error.code.as_str() {
        "request_cancelled" => BlueprintClientError::Cancelled,
        "deadline_exceeded" | "deadline_invalid" => BlueprintClientError::Timeout,
        "blueprint_oversized" => BlueprintClientError::Oversized("response"),
        "generation_mismatch" => {
            let expected = error.details.as_ref().and_then(|value| value.get("expected")).and_then(Value::as_str).unwrap_or_default().to_owned();
            let observed = error.details.as_ref().and_then(|value| value.get("observed")).and_then(Value::as_str).unwrap_or_default().to_owned();
            BlueprintClientError::GenerationMismatch { expected, observed }
        }
        "blueprint_malformed" | "invalid_request" | "required_field_missing" | "protocol_version_mismatch" => BlueprintClientError::Malformed(error.message),
        "service_not_ready" | "hub_inactive" | "not_configured" => BlueprintClientError::Unavailable(error.message),
        code => BlueprintClientError::Remote { code: code.to_owned(), message: error.message },
    }
}

fn stable_digest(value: &str) -> String {
    let mut state: u64 = 0xcbf29ce484222325;
    for byte in value.as_bytes() { state ^= *byte as u64; state = state.wrapping_mul(0x100000001b3); }
    format!("fnv1a:{state:016x}")
}
