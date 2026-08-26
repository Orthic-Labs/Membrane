//! Typed, generation-bound access to Blueprint hosted inside active Hub.
//!
//! This module owns only the consumer seam.  Blueprint remains responsible
//! for parsing repositories, graph semantics, and generation publication.
//! The transport is injectable so in-process Hub composition and Hub IPC share
//! exactly one request, cap, cache, and response-validation path. This client
//! never starts a process or opens Blueprint storage.

use membrane_protocol::CandidateV1;
use membrane_provider_sdk::source::{
    BlueprintResult, BlueprintSource, SourceQuery, SourceResponse, SourceResult,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::future::Future;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

pub const BLUEPRINT_PROTOCOL_VERSION: u32 = 1;
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
            max_response_bytes: self.max_response_bytes.max(1024),
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
            policy_digest: "".to_owned(),
            expected_generation: expected.or_else(|| source.generation.clone()),
            symbol: None,
            bounds: bounds.bounded(),
            deadline,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlueprintWireRequest {
    pub protocol_version: u32,
    pub request_id: String,
    pub repo_id: Option<String>,
    pub generation: Option<String>,
    pub method: String,
    pub deadline_ms: u64,
    pub input: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlueprintWireResponse {
    pub request_id: Option<String>,
    #[serde(default)]
    pub ok: bool,
    pub generation: Option<String>,
    pub result: Option<Value>,
    pub error: Option<BlueprintWireError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlueprintWireError {
    pub code: Option<String>,
    pub message: Option<String>,
}

pub trait BlueprintTransport: Send + Sync {
    fn exchange(
        &self,
        request: &BlueprintWireRequest,
        bounds: BlueprintBounds,
        deadline: Duration,
        cancellation: CancellationToken,
    ) -> Result<BlueprintWireResponse, BlueprintClientError>;
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BlueprintClientError {
    #[error("Hub-hosted Blueprint is unavailable: {0}")]
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
    #[error("Hub-hosted Blueprint returned typed error {code}: {message}")]
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
            Self::Remote { .. } => "blueprint_remote_error",
        }
    }
}

#[derive(Clone)]
struct CacheEntry {
    key: BlueprintCacheKey,
    expires_at: Instant,
    value: BlueprintResult,
}

#[derive(Default)]
struct CacheState {
    entries: VecDeque<CacheEntry>,
}

pub struct BlueprintClient<T: BlueprintTransport> {
    transport: Arc<T>,
    cache: Mutex<CacheState>,
    cache_ttl: Duration,
    cache_entries: usize,
}

impl<T: BlueprintTransport> BlueprintClient<T> {
    pub fn new(transport: Arc<T>) -> Self {
        Self {
            transport,
            cache: Mutex::new(CacheState::default()),
            cache_ttl: DEFAULT_CACHE_TTL,
            cache_entries: DEFAULT_CACHE_ENTRIES,
        }
    }

    pub fn with_cache_limits(mut self, ttl: Duration, entries: usize) -> Self {
        self.cache_ttl = ttl;
        self.cache_entries = entries.max(1);
        self
    }

    pub fn query(&self, query: &BlueprintQuery) -> Result<BlueprintResult, BlueprintClientError> {
        self.query_with_cancellation(query, CancellationToken::new())
    }

    pub fn query_with_cancellation(
        &self,
        query: &BlueprintQuery,
        cancellation: CancellationToken,
    ) -> Result<BlueprintResult, BlueprintClientError> {
        if cancellation.is_cancelled() {
            return Err(BlueprintClientError::Cancelled);
        }
        if query.deadline.is_zero() {
            return Err(BlueprintClientError::Timeout);
        }
        let query = BlueprintQuery {
            bounds: query.bounds.bounded(),
            ..query.clone()
        };
        let key = query.cache_key();
        if query
            .expected_generation
            .as_deref()
            .is_some_and(|generation| !generation.is_empty())
        {
            if let Some(value) = self.cached(&key) {
                return Ok(value);
            }
        }
        let request = self.request(&query, "recall");
        let response =
            self.transport
                .exchange(&request, query.bounds, query.deadline, cancellation)?;
        let value = parse_result(response, &query)?;
        if query
            .expected_generation
            .as_deref()
            .is_some_and(|generation| !generation.is_empty())
        {
            self.insert(key, value.clone());
        }
        Ok(value)
    }

    pub fn resolve_symbol(
        &self,
        query: &BlueprintQuery,
        symbol: &str,
    ) -> Result<BlueprintResult, BlueprintClientError> {
        self.resolve_symbol_with_cancellation(query, symbol, CancellationToken::new())
    }

    pub fn resolve_symbol_with_cancellation(
        &self,
        query: &BlueprintQuery,
        symbol: &str,
        cancellation: CancellationToken,
    ) -> Result<BlueprintResult, BlueprintClientError> {
        if cancellation.is_cancelled() {
            return Err(BlueprintClientError::Cancelled);
        }
        if query.deadline.is_zero() {
            return Err(BlueprintClientError::Timeout);
        }
        let query = BlueprintQuery {
            symbol: Some(symbol.to_owned()),
            ..query.clone()
        };
        let request = self.request(&query, "resolve");
        let response = self.transport.exchange(
            &request,
            query.bounds.bounded(),
            query.deadline,
            cancellation,
        )?;
        parse_result(response, &query)
    }

    /// Execute one bounded native Blueprint operation without interpreting its
    /// operation-specific payload. MCP uses this for graph/snapshot methods;
    /// Blueprint remains sole owner of graph semantics & returned shape.
    pub fn execute_wire(
        &self,
        request_id: &str,
        repository_id: &str,
        method: &str,
        input: Value,
        expected_generation: Option<&str>,
        bounds: BlueprintBounds,
        deadline: Duration,
    ) -> Result<Value, BlueprintClientError> {
        if request_id.trim().is_empty() || method.trim().is_empty() || deadline.is_zero() {
            return Err(BlueprintClientError::Malformed(
                "request identity or method is empty".into(),
            ));
        }
        let bounds = bounds.bounded();
        let expected_generation = expected_generation
            .map(str::trim)
            .filter(|generation| !generation.is_empty())
            .map(str::to_owned);
        let request = BlueprintWireRequest {
            protocol_version: BLUEPRINT_PROTOCOL_VERSION,
            request_id: request_id.to_owned(),
            repo_id: (!repository_id.trim().is_empty()).then(|| repository_id.to_owned()),
            generation: expected_generation.clone(),
            method: method.to_owned(),
            deadline_ms: deadline.as_millis().clamp(1, u64::MAX as u128) as u64,
            input,
        };
        let response =
            self.transport
                .exchange(&request, bounds, deadline, CancellationToken::new())?;
        let encoded = serde_json::to_vec(&response)
            .map_err(|error| BlueprintClientError::Malformed(error.to_string()))?;
        if encoded.len() > bounds.max_response_bytes {
            return Err(BlueprintClientError::Oversized("response_bytes"));
        }
        if response.request_id.as_deref() != Some(request_id) {
            return Err(BlueprintClientError::Malformed(
                "response request identity mismatch".into(),
            ));
        }
        if !response.ok {
            let remote = response.error.unwrap_or(BlueprintWireError {
                code: None,
                message: None,
            });
            return Err(BlueprintClientError::Remote {
                code: remote
                    .code
                    .unwrap_or_else(|| "blueprint_unavailable".into()),
                message: remote.message.unwrap_or_else(|| "request failed".into()),
            });
        }
        if let Some(expected) = expected_generation {
            let observed = response
                .generation
                .clone()
                .or_else(|| {
                    response.result.as_ref().and_then(|result| {
                        result
                            .get("generationId")
                            .and_then(Value::as_str)
                            .map(str::to_owned)
                    })
                })
                .unwrap_or_default();
            if observed != expected {
                return Err(BlueprintClientError::GenerationMismatch { expected, observed });
            }
        }
        response.result.ok_or_else(|| {
            BlueprintClientError::Malformed("successful response has no result".into())
        })
    }

    fn request(&self, query: &BlueprintQuery, method: &str) -> BlueprintWireRequest {
        let mut input = json!({
            "repoRoot": query.repository_root,
            "task": query.task,
            "limit": query.bounds.max_candidates,
            "anchors": query.anchors,
        });
        if let Some(symbol) = &query.symbol {
            input["symbol"] = Value::String(symbol.clone());
        }
        BlueprintWireRequest {
            protocol_version: BLUEPRINT_PROTOCOL_VERSION,
            request_id: if query.request_id.trim().is_empty() {
                "blueprint-request".into()
            } else {
                query.request_id.clone()
            },
            repo_id: (!query.repository_id.is_empty()).then(|| query.repository_id.clone()),
            generation: query.expected_generation.clone(),
            method: method.to_owned(),
            deadline_ms: query.deadline.as_millis().clamp(1, u64::MAX as u128) as u64,
            input,
        }
    }

    fn cached(&self, key: &BlueprintCacheKey) -> Option<BlueprintResult> {
        let mut cache = self.cache.lock().ok()?;
        let now = Instant::now();
        cache.entries.retain(|entry| entry.expires_at > now);
        cache
            .entries
            .iter()
            .find(|entry| entry.key == *key)
            .map(|entry| entry.value.clone())
    }

    fn insert(&self, key: BlueprintCacheKey, value: BlueprintResult) {
        let Ok(mut cache) = self.cache.lock() else {
            return;
        };
        cache
            .entries
            .retain(|entry| entry.expires_at > Instant::now() && entry.key != key);
        cache.entries.push_back(CacheEntry {
            key,
            expires_at: Instant::now() + self.cache_ttl,
            value,
        });
        while cache.entries.len() > self.cache_entries {
            cache.entries.pop_front();
        }
    }
}

impl<T: BlueprintTransport + 'static> BlueprintSource for BlueprintClient<T> {
    fn query<'life0, 'life1, 'async_trait>(
        &'life0 self,
        source: &'life1 SourceQuery,
    ) -> Pin<Box<dyn Future<Output = SourceResult<BlueprintResult>> + Send + 'async_trait>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait,
    {
        let _ = source;
        Box::pin(async {
            Err(membrane_provider_sdk::ProviderError::MissingSource(
                "blueprint_context",
            ))
        })
    }

    fn resolve_symbol<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        source: &'life1 SourceQuery,
        symbol: &'life2 str,
    ) -> Pin<Box<dyn Future<Output = SourceResult<BlueprintResult>> + Send + 'async_trait>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait,
    {
        let _ = (source, symbol);
        Box::pin(async {
            Err(membrane_provider_sdk::ProviderError::MissingSource(
                "blueprint_context",
            ))
        })
    }
}

/// Context-aware source seam used by federation so caller deadline and
/// cancellation reach transport instead of being replaced by a local timeout.
pub trait ContextualBlueprintSource: Send + Sync {
    fn query_with_context<'life0, 'life1, 'async_trait>(
        &'life0 self,
        source: &'life1 SourceQuery,
        deadline: Instant,
        cancellation: CancellationToken,
    ) -> Pin<Box<dyn Future<Output = SourceResult<BlueprintResult>> + Send + 'async_trait>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;

    fn resolve_symbol_with_context<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        source: &'life1 SourceQuery,
        symbol: &'life2 str,
        deadline: Instant,
        cancellation: CancellationToken,
    ) -> Pin<Box<dyn Future<Output = SourceResult<BlueprintResult>> + Send + 'async_trait>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait;
}

impl<T: BlueprintTransport + 'static> ContextualBlueprintSource for BlueprintClient<T> {
    fn query_with_context<'life0, 'life1, 'async_trait>(
        &'life0 self,
        source: &'life1 SourceQuery,
        deadline: Instant,
        cancellation: CancellationToken,
    ) -> Pin<Box<dyn Future<Output = SourceResult<BlueprintResult>> + Send + 'async_trait>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move {
            let query = BlueprintQuery::from_source(
                source,
                source.generation.clone(),
                BlueprintBounds::default(),
                deadline.saturating_duration_since(Instant::now()),
            );
            let result = self
                .query_with_cancellation(&query, cancellation)
                .map_err(client_error)?;
            Ok(SourceResponse {
                generation: Some(result.generation.clone()),
                complete: true,
                warnings: Vec::new(),
                value: result,
            })
        })
    }

    fn resolve_symbol_with_context<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        source: &'life1 SourceQuery,
        symbol: &'life2 str,
        deadline: Instant,
        cancellation: CancellationToken,
    ) -> Pin<Box<dyn Future<Output = SourceResult<BlueprintResult>> + Send + 'async_trait>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait,
    {
        let symbol = symbol.to_owned();
        Box::pin(async move {
            let query = BlueprintQuery::from_source(
                source,
                source.generation.clone(),
                BlueprintBounds::default(),
                deadline.saturating_duration_since(Instant::now()),
            );
            let result = self
                .resolve_symbol_with_cancellation(&query, &symbol, cancellation)
                .map_err(client_error)?;
            Ok(SourceResponse {
                generation: Some(result.generation.clone()),
                complete: true,
                warnings: Vec::new(),
                value: result,
            })
        })
    }
}

fn client_error(error: BlueprintClientError) -> membrane_provider_sdk::ProviderError {
    use membrane_provider_sdk::ProviderError;
    match error {
        BlueprintClientError::Timeout => ProviderError::DeadlineExceeded,
        BlueprintClientError::Cancelled => ProviderError::Cancelled,
        BlueprintClientError::Unavailable(message) => ProviderError::Unavailable(message),
        BlueprintClientError::Remote { code, message } => ProviderError::Typed {
            operation: "blueprint".into(),
            code,
            message,
            details: None,
        },
        BlueprintClientError::GenerationMismatch { expected, observed } => {
            ProviderError::IdentityMismatch(format!("expected {expected}, observed {observed}"))
        }
        other => ProviderError::SourceFailure(other.to_string()),
    }
}

fn parse_result(
    response: BlueprintWireResponse,
    query: &BlueprintQuery,
) -> Result<BlueprintResult, BlueprintClientError> {
    let response_bytes = serde_json::to_vec(&response)
        .map_err(|error| BlueprintClientError::Malformed(error.to_string()))?;
    if response_bytes.len() > query.bounds.max_response_bytes {
        return Err(BlueprintClientError::Oversized("response_bytes"));
    }
    if let Some(expected_id) =
        (!query.request_id.trim().is_empty()).then_some(query.request_id.as_str())
    {
        if response.request_id.as_deref() != Some(expected_id)
            && response.request_id.as_deref() != Some("blueprint-request")
        {
            return Err(BlueprintClientError::Malformed(
                "response request identity mismatch".into(),
            ));
        }
    }
    if !response.ok {
        let error = response.error.unwrap_or(BlueprintWireError {
            code: None,
            message: None,
        });
        return Err(BlueprintClientError::Remote {
            code: error.code.unwrap_or_else(|| "blueprint_unavailable".into()),
            message: error.message.unwrap_or_else(|| "request failed".into()),
        });
    }
    let raw = response.result.ok_or_else(|| {
        BlueprintClientError::Malformed("successful response has no result".into())
    })?;
    let observed = response
        .generation
        .or_else(|| {
            raw.get("generationId")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .or_else(|| query.expected_generation.clone())
        .unwrap_or_default();
    if let Some(expected) = query
        .expected_generation
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        if observed != expected {
            return Err(BlueprintClientError::GenerationMismatch {
                expected: expected.to_owned(),
                observed,
            });
        }
    }
    let candidate_value = raw
        .get("candidateSet")
        .and_then(|set| set.get("candidates"))
        .or_else(|| raw.get("candidates"))
        .ok_or_else(|| BlueprintClientError::Malformed("recall result has no candidates".into()))?;
    let candidates = candidate_value
        .as_array()
        .ok_or_else(|| BlueprintClientError::Malformed("candidates is not an array".into()))?;
    if candidates.len() > query.bounds.max_candidates {
        return Err(BlueprintClientError::Oversized("candidate_count"));
    }
    let mut paths = std::collections::BTreeSet::new();
    for value in candidates {
        let Some(path) = value.get("sourceRef").and_then(Value::as_str) else {
            return Err(BlueprintClientError::Malformed(
                "candidate sourceRef is missing".into(),
            ));
        };
        if path.len() > 4096 {
            return Err(BlueprintClientError::Oversized("path_length"));
        }
        paths.insert(path);
        if paths.len() > query.bounds.max_paths {
            return Err(BlueprintClientError::Oversized("path_count"));
        }
    }
    let typed = candidates
        .iter()
        .cloned()
        .map(|value| {
            serde_json::from_value::<CandidateV1>(value)
                .map_err(|error| BlueprintClientError::Malformed(error.to_string()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let payload = Some(raw);
    Ok(BlueprintResult {
        generation: observed,
        candidates: typed,
        payload,
    })
}

fn stable_digest(value: &str) -> String {
    // A deterministic, machine-independent query component without bringing
    // another hashing dependency into the consumer seam.
    let mut state: u64 = 0xcbf29ce484222325;
    for byte in value.as_bytes() {
        state ^= *byte as u64;
        state = state.wrapping_mul(0x100000001b3);
    }
    format!("fnv1a:{state:016x}")
}

#[cfg(unix)]
#[derive(Debug, Clone)]
pub struct UnixBlueprintTransport {
    pub endpoint: PathBuf,
}

#[cfg(unix)]
impl UnixBlueprintTransport {
    pub fn new(endpoint: impl Into<PathBuf>) -> Self {
        Self {
            endpoint: endpoint.into(),
        }
    }
}

#[cfg(unix)]
impl BlueprintTransport for UnixBlueprintTransport {
    fn exchange(
        &self,
        request: &BlueprintWireRequest,
        bounds: BlueprintBounds,
        deadline: Duration,
        cancellation: CancellationToken,
    ) -> Result<BlueprintWireResponse, BlueprintClientError> {
        use std::os::unix::net::UnixStream;
        let mut socket = UnixStream::connect(&self.endpoint)
            .map_err(|error| BlueprintClientError::Unavailable(error.to_string()))?;
        socket
            .set_read_timeout(Some(deadline.min(Duration::from_millis(25))))
            .map_err(|error| BlueprintClientError::Unavailable(error.to_string()))?;
        socket
            .set_write_timeout(Some(deadline))
            .map_err(|error| BlueprintClientError::Unavailable(error.to_string()))?;
        let wire = serde_json::to_vec(request)
            .map_err(|error| BlueprintClientError::Malformed(error.to_string()))?;
        if wire.len() + 1 > bounds.max_response_bytes {
            return Err(BlueprintClientError::Oversized("request_frame"));
        }
        socket
            .write_all(&wire)
            .and_then(|_| socket.write_all(b"\n"))
            .map_err(|error| BlueprintClientError::Unavailable(error.to_string()))?;
        let mut reader = BufReader::new(socket);
        let mut line = Vec::new();
        let started = Instant::now();
        loop {
            if cancellation.is_cancelled() {
                return Err(BlueprintClientError::Cancelled);
            }
            if started.elapsed() >= deadline {
                return Err(BlueprintClientError::Timeout);
            }
            match reader.read_until(b'\n', &mut line) {
                Ok(0) => {
                    return Err(BlueprintClientError::Unavailable(
                        "daemon returned no response".into(),
                    ))
                }
                Ok(_) => break,
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                    ) =>
                {
                    continue
                }
                Err(error) => return Err(BlueprintClientError::Unavailable(error.to_string())),
            }
        }
        if line.len() > bounds.max_response_bytes {
            return Err(BlueprintClientError::Oversized("response_frame"));
        }
        serde_json::from_slice(&line)
            .map_err(|error| BlueprintClientError::Malformed(error.to_string()))
    }
}

#[cfg(not(unix))]
#[derive(Debug, Clone)]
pub struct UnixBlueprintTransport {
    pub endpoint: PathBuf,
}

#[cfg(not(unix))]
impl UnixBlueprintTransport {
    pub fn new(endpoint: impl Into<PathBuf>) -> Self {
        Self {
            endpoint: endpoint.into(),
        }
    }
}

#[cfg(not(unix))]
impl BlueprintTransport for UnixBlueprintTransport {
    fn exchange(
        &self,
        request: &BlueprintWireRequest,
        bounds: BlueprintBounds,
        deadline: Duration,
        cancellation: CancellationToken,
    ) -> Result<BlueprintWireResponse, BlueprintClientError> {
        if cancellation.is_cancelled() {
            return Err(BlueprintClientError::Cancelled);
        }
        if deadline.is_zero() {
            return Err(BlueprintClientError::Timeout);
        }
        let wire = serde_json::to_vec(request)
            .map_err(|error| BlueprintClientError::Malformed(error.to_string()))?;
        if wire.len() + 1 > bounds.max_response_bytes {
            return Err(BlueprintClientError::Oversized("request_frame"));
        }
        let mut framed = wire;
        framed.push(b'\n');
        let response = exchange_windows_named_pipe(
            &self.endpoint,
            &framed,
            bounds.max_response_bytes,
            deadline,
        )
        .map_err(|error| {
            if error == "__blueprint_pipe_timeout__" {
                BlueprintClientError::Timeout
            } else if error == "response_frame_exceeds_limit" {
                BlueprintClientError::Oversized("response_frame")
            } else if error == "request_frame_exceeds_limit" {
                BlueprintClientError::Oversized("request_frame")
            } else {
                BlueprintClientError::Unavailable(error)
            }
        })?;
        if cancellation.is_cancelled() {
            return Err(BlueprintClientError::Cancelled);
        }
        serde_json::from_slice(&response)
            .map_err(|error| BlueprintClientError::Malformed(error.to_string()))
    }
}

/// Exchange one bounded frame with Hub's Windows named pipe.
///
/// `std::fs::File::read` can block forever on a pipe whose peer disappeared.
/// The Win32 availability probe keeps reads non-blocking and gives one-shot
/// callers a hard upper bound without leaving a detached worker behind.
#[cfg(not(unix))]
pub fn exchange_windows_named_pipe(
    endpoint: &std::path::Path,
    request: &[u8],
    max_frame_bytes: usize,
    deadline: Duration,
) -> Result<Vec<u8>, String> {
    #[cfg(not(windows))]
    {
        let _ = (endpoint, request, max_frame_bytes, deadline);
        return Err("Windows named-pipe transport is unavailable on this host".to_owned());
    }

    #[cfg(windows)]
    {
        use std::ffi::c_void;
        use std::os::windows::ffi::OsStrExt;
        use std::os::windows::io::AsRawHandle;
        use std::ptr::null_mut;

        const TIMEOUT_SENTINEL: &str = "__blueprint_pipe_timeout__";

        #[link(name = "kernel32")]
        extern "system" {
            fn WaitNamedPipeW(name: *const u16, timeout_ms: u32) -> i32;
            fn PeekNamedPipe(
                pipe: *mut c_void,
                buffer: *mut c_void,
                buffer_len: u32,
                bytes_read: *mut u32,
                bytes_available: *mut u32,
                bytes_left: *mut u32,
            ) -> i32;
        }

        if request.len() > max_frame_bytes {
            return Err("request_frame_exceeds_limit".to_owned());
        }
        if deadline.is_zero() {
            return Err(TIMEOUT_SENTINEL.to_owned());
        }
        let started = Instant::now();
        let remaining = deadline.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            return Err(TIMEOUT_SENTINEL.to_owned());
        }
        let timeout_ms = remaining.as_millis().clamp(1, u32::MAX as u128) as u32;
        let name = endpoint
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        let ready = unsafe { WaitNamedPipeW(name.as_ptr(), timeout_ms) };
        if ready == 0 {
            let error = std::io::Error::last_os_error();
            return if error.kind() == std::io::ErrorKind::TimedOut {
                Err(TIMEOUT_SENTINEL.to_owned())
            } else {
                Err(format!("wait for Hub Blueprint pipe: {error}"))
            };
        }
        let mut pipe = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(endpoint)
            .map_err(|error| format!("open Hub Blueprint pipe: {error}"))?;
        pipe.write_all(request)
            .and_then(|_| pipe.flush())
            .map_err(|error| format!("write Hub Blueprint pipe: {error}"))?;

        let mut frame = Vec::new();
        let mut chunk = [0_u8; 1024];
        loop {
            if started.elapsed() >= deadline {
                return Err(TIMEOUT_SENTINEL.to_owned());
            }
            let mut available = 0_u32;
            let observed = unsafe {
                PeekNamedPipe(
                    pipe.as_raw_handle() as *mut c_void,
                    null_mut(),
                    0,
                    null_mut(),
                    &mut available,
                    null_mut(),
                )
            };
            if observed == 0 {
                return Err(format!(
                    "read Hub Blueprint pipe availability: {}",
                    std::io::Error::last_os_error()
                ));
            }
            if available == 0 {
                let remaining = deadline.saturating_sub(started.elapsed());
                std::thread::sleep(remaining.min(Duration::from_millis(5)));
                continue;
            }
            let read_len = (available as usize).min(chunk.len());
            let count = pipe
                .read(&mut chunk[..read_len])
                .map_err(|error| format!("read Hub Blueprint pipe: {error}"))?;
            if count == 0 {
                return Err("Hub Blueprint pipe returned no response".to_owned());
            }
            frame.extend_from_slice(&chunk[..count]);
            if frame.len() > max_frame_bytes {
                return Err("response_frame_exceeds_limit".to_owned());
            }
            if frame.contains(&b'\n') {
                break;
            }
        }
        let end = frame
            .iter()
            .position(|byte| *byte == b'\n')
            .unwrap_or(frame.len());
        if end == 0 {
            return Err("Hub Blueprint pipe returned an empty response".to_owned());
        }
        Ok(frame[..end].to_vec())
    }
}

#[cfg(all(test, not(unix)))]
mod windows_transport_tests {
    use super::exchange_windows_named_pipe;
    use std::path::Path;
    use std::time::Duration;

    #[test]
    fn request_frame_is_capped_before_pipe_open() {
        let error = exchange_windows_named_pipe(
            Path::new(r"\\.\pipe\missing"),
            b"too-large",
            1,
            Duration::from_secs(1),
        )
        .expect_err("oversized request must fail before opening a pipe");
        assert_eq!(error, "request_frame_exceeds_limit");
    }

    #[test]
    fn zero_deadline_is_bounded_before_pipe_open() {
        let error = exchange_windows_named_pipe(
            Path::new(r"\\.\pipe\missing"),
            b"{}\n",
            1024,
            Duration::ZERO,
        )
        .expect_err("zero deadline must fail before opening a pipe");
        assert_eq!(error, "__blueprint_pipe_timeout__");
    }
}
