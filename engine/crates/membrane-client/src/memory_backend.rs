//! Typed resident-service implementation of CodeRight's memory surface.

use crate::error::ClientError;
use crate::handshake::{self, CompatibilityRequirement, ServiceIdentity, HANDSHAKE_OPERATION};
use crate::records::{self, FullRecord, MemoryEntry, MemoryListRow, MemoryTier};
use crate::{Map, ProtocolError, ProtocolResult, Value};
use std::marker::PhantomData;
use std::time::{Duration, Instant};

pub const ACTIVITY: &str = "/activity";
pub const DELETE: &str = "/delete";
pub const FEDERATE: &str = "/federate";
pub const GET: &str = "/get";
pub const LIST: &str = "/list";
pub const METRICS: &str = "/metrics";
pub const PUT: &str = "/put";
pub const RECALL: &str = "/recall";
pub const REMEMBER: &str = "/remember";
pub const REMEMBER_CONSOLIDATED: &str = "/remember_consolidated";
pub const SCOPES: &str = "/scopes";
pub const SEARCH: &str = "/search";
pub const USE: &str = "/use";

/// A transport is deliberately injected: this crate owns no sockets, process, or retry policy.
pub type MemoryTransport =
    dyn Fn(&str, &Map<String, Value>) -> Result<Value, ClientError> + Send + Sync;

pub type CancellationToken = tokio_util::sync::CancellationToken;

#[derive(Clone, Debug)]
pub struct CallOptions {
    pub deadline: Instant,
    pub cancellation: CancellationToken,
}
impl CallOptions {
    pub fn after(duration: Duration) -> Self {
        Self {
            deadline: Instant::now() + duration,
            cancellation: CancellationToken::new(),
        }
    }
    pub fn at(deadline: Instant, cancellation: CancellationToken) -> Self {
        Self {
            deadline,
            cancellation,
        }
    }
    pub fn unbounded(cancellation: CancellationToken) -> Self {
        Self {
            deadline: Instant::now() + Duration::from_secs(3_155_760_000),
            cancellation,
        }
    }
    fn check(&self) -> Result<(), ClientError> {
        if self.cancellation.is_cancelled() {
            Err(ClientError::Cancelled)
        } else if Instant::now() >= self.deadline {
            Err(ClientError::Timeout {
                message: "call deadline elapsed".into(),
            })
        } else {
            Ok(())
        }
    }
}

/// Bound client. Binding is explicit and immutable; no method opens local storage or retries.
pub struct MemoryBackendClient<T: ?Sized = MemoryTransport> {
    transport: Box<T>,
    options: CallOptions,
    identity: Option<ServiceIdentity>,
    bearer_token: Option<String>,
}

/// One request-scoped typed view over a bound client.  It keeps transport and
/// identity immutable while replacing construction-time options for one call.
pub struct MemoryBackendCall<'a, T: ?Sized> {
    client: &'a MemoryBackendClient<T>,
    options: CallOptions,
    marker: PhantomData<&'a T>,
}

fn recall_entry<T>(
    client: &MemoryBackendClient<T>,
    options: &CallOptions,
    value: Value,
) -> Result<MemoryEntry, ClientError>
where
    T: ?Sized + Fn(&str, &Map<String, Value>) -> Result<Value, ClientError> + Send + Sync,
{
    let row = value
        .as_object()
        .ok_or_else(|| ClientError::protocol("record_malformed", "memory hit is not an object"))?;
    if let Some(full) = row.get("entry") {
        return records::entry(full);
    }
    if row.get("content").is_some() && row.get("tier").is_some() {
        return records::entry(&value);
    }
    let id = row
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .ok_or_else(|| ClientError::protocol("record_malformed", "memory hit has no id"))?;
    let full = client.call_raw_with_options(options, GET, map([("id", Value::from(id))]))?;
    let full_row = full.as_object().ok_or_else(|| {
        ClientError::protocol("response_malformed", "get response is not an object")
    })?;
    let content = full_row
        .get("content")
        .and_then(Value::as_str)
        .filter(|content| !content.is_empty())
        .ok_or_else(|| ClientError::protocol("record_malformed", "memory content is missing"))?;
    let tier = row
        .get("tier")
        .or_else(|| full_row.get("tier"))
        .and_then(Value::as_str)
        .and_then(records::MemoryTier::parse)
        .unwrap_or(records::MemoryTier::Semantic);
    Ok(MemoryEntry {
        id: id.to_string(),
        tier,
        content: content.to_string(),
        keywords: row
            .get("keywords")
            .or_else(|| full_row.get("keywords"))
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default(),
        score: row
            .get("score")
            .or_else(|| row.get("cos"))
            .and_then(Value::as_f64)
            .unwrap_or(0.0),
        created_at: row
            .get("created_at")
            .or_else(|| row.get("createdAt"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        access_count: full_row
            .get("access_count")
            .or_else(|| full_row.get("accessCount"))
            .and_then(Value::as_u64)
            .unwrap_or(0) as u32,
        embedding: None,
        scope_id: row
            .get("scope")
            .or_else(|| row.get("scope_id"))
            .or_else(|| row.get("scopeId"))
            .and_then(Value::as_str)
            .unwrap_or("global")
            .to_string(),
    })
}

impl<T: ?Sized + Fn(&str, &Map<String, Value>) -> Result<Value, ClientError> + Send + Sync>
    MemoryBackendClient<T>
{
    pub fn new(transport: Box<T>) -> Self {
        Self {
            transport,
            options: CallOptions::unbounded(CancellationToken::new()),
            identity: None,
            bearer_token: None,
        }
    }
    pub fn with_options(mut self, options: CallOptions) -> Self {
        self.options = options;
        self
    }
    /// Keep credentials outside request JSON. The transport decides how to attach them.
    pub fn with_bearer_token(mut self, token: impl Into<String>) -> Self {
        self.bearer_token = Some(token.into());
        self
    }
    pub fn has_bearer_token(&self) -> bool {
        self.bearer_token.is_some()
    }
    pub fn identity(&self) -> Option<&ServiceIdentity> {
        self.identity.as_ref()
    }
    pub fn with_call_options(&self, options: CallOptions) -> MemoryBackendCall<'_, T> {
        MemoryBackendCall {
            client: self,
            options,
            marker: PhantomData,
        }
    }

    pub fn bind(mut self, requirement: &CompatibilityRequirement) -> Result<Self, ClientError> {
        let response = self.call_raw(HANDSHAKE_OPERATION, handshake::request())?;
        self.identity = Some(handshake::verify(&response, requirement)?);
        Ok(self)
    }

    /// Bind from one health response already obtained over the authoritative
    /// transport. This avoids a second handshake/restart race while preserving
    /// the exact same public compatibility verification as `bind`.
    pub fn bind_verified(
        mut self,
        response: &Value,
        requirement: &CompatibilityRequirement,
    ) -> Result<Self, ClientError> {
        self.identity = Some(handshake::verify(response, requirement)?);
        Ok(self)
    }

    fn call_raw(&self, operation: &str, request: Map<String, Value>) -> Result<Value, ClientError> {
        self.call_raw_with_options(&self.options, operation, request)
    }

    fn call_raw_with_options(
        &self,
        options: &CallOptions,
        operation: &str,
        request: Map<String, Value>,
    ) -> Result<Value, ClientError> {
        if operation != HANDSHAKE_OPERATION && self.identity.is_none() {
            return Err(ClientError::Incompatible {
                message: "resident client has not completed compatibility handshake".into(),
            });
        }
        options.check()?;
        let response = (self.transport)(operation, &request)?;
        options.check()?;
        if response.get("error").is_some()
            || response.get("kind").and_then(Value::as_str) == Some("error")
        {
            return Err(ClientError::from_json_error(&response));
        }
        Ok(response)
    }
    fn object(
        &self,
        operation: &str,
        request: Map<String, Value>,
    ) -> Result<Map<String, Value>, ClientError> {
        self.object_with_options(&self.options, operation, request)
    }
    fn object_with_options(
        &self,
        options: &CallOptions,
        operation: &str,
        request: Map<String, Value>,
    ) -> Result<Map<String, Value>, ClientError> {
        self.call_raw_with_options(options, operation, request)?
            .as_object()
            .cloned()
            .ok_or_else(|| {
                ClientError::protocol(
                    "response_malformed",
                    format!("{operation} response is not an object"),
                )
            })
    }
    fn array(
        &self,
        operation: &str,
        request: Map<String, Value>,
    ) -> Result<Vec<Value>, ClientError> {
        self.array_with_options(&self.options, operation, request)
    }
    fn array_with_options(
        &self,
        options: &CallOptions,
        operation: &str,
        request: Map<String, Value>,
    ) -> Result<Vec<Value>, ClientError> {
        self.call_raw_with_options(options, operation, request)?
            .as_array()
            .cloned()
            .ok_or_else(|| {
                ClientError::protocol(
                    "response_malformed",
                    format!("{operation} response is not an array"),
                )
            })
    }

    pub fn activity_json(&self, limit: usize) -> Result<Value, ClientError> {
        Ok(self.call_raw(ACTIVITY, map([("limit", Value::from(limit as u64))]))?)
    }
    pub fn metrics_json(&self) -> Result<Value, ClientError> {
        Ok(self.call_raw(METRICS, Map::new())?)
    }
    pub fn embedder_dim(&self) -> Result<usize, ClientError> {
        let row = self.object("/health", Map::new())?;
        row.get("embedder_dim")
            .or_else(|| row.get("embedderDim"))
            .and_then(Value::as_u64)
            .map(|v| v as usize)
            .ok_or_else(|| {
                ClientError::protocol(
                    "response_malformed",
                    "health response has no embedder dimension",
                )
            })
    }
    pub fn delete(&self, id: &str) -> Result<bool, ClientError> {
        Ok(self
            .object(DELETE, map([("id", Value::from(id))]))?
            .get("deleted")
            .and_then(Value::as_bool)
            .unwrap_or(false))
    }
    pub fn entries(&self, limit: usize) -> Result<Vec<MemoryEntry>, ClientError> {
        self.array(LIST, map([("limit", Value::from(limit as u64))]))?
            .into_iter()
            .map(|value| recall_entry(self, &self.options, value))
            .collect()
    }
    /// Return route-native list rows, including access and injection counters.
    pub fn list(&self, scope: Option<&str>) -> Result<Vec<MemoryListRow>, ClientError> {
        let mut request = Map::new();
        if let Some(scope) = scope {
            request.insert("scope".into(), Value::from(scope));
        }
        self.array(LIST, request)?
            .iter()
            .map(records::list_row)
            .collect()
    }
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<MemoryEntry>, ClientError> {
        self.rows(
            SEARCH,
            map([
                ("query", Value::from(query)),
                ("limit", Value::from(limit as u64)),
            ]),
        )
    }
    pub fn scopes(&self) -> Result<Vec<String>, ClientError> {
        Ok(self
            .array(SCOPES, Map::new())?
            .into_iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect())
    }
    pub fn get_full(&self, id: &str) -> Result<FullRecord, ClientError> {
        records::full(&self.call_raw(GET, map([("id", Value::from(id))]))?)
    }
    pub fn recall_scored(
        &self,
        query: &str,
        limit: usize,
        scopes: &[String],
    ) -> Result<Vec<(MemoryEntry, f64)>, ClientError> {
        let mut request = map([
            ("query", Value::from(query)),
            ("k", Value::from(limit as u64)),
            ("client", Value::from("coderight")),
            ("observe", Value::Bool(false)),
        ]);
        if let Some(scope) = scopes.first() {
            request.insert("scope".into(), Value::from(scope.as_str()));
        }
        if scopes.len() > 1 {
            request.insert(
                "cross".into(),
                Value::Array(scopes[1..].iter().cloned().map(Value::from).collect()),
            );
        }
        let response = self.call_raw(RECALL, request)?;
        let hits = match response {
            Value::Array(hits) => hits,
            Value::Object(object) if object.get("status").is_some() => object
                .get("hits")
                .and_then(Value::as_array)
                .cloned()
                .ok_or_else(|| {
                    ClientError::protocol(
                        "response_malformed",
                        "recall status response has no hits array",
                    )
                })?,
            _ => {
                return Err(ClientError::protocol(
                    "response_malformed",
                    "recall response is not an array",
                ))
            }
        };
        hits.into_iter()
            .map(|value| {
                let score = value
                    .get("score")
                    .or_else(|| value.get("cos"))
                    .and_then(Value::as_f64)
                    .unwrap_or(0.0);
                recall_entry(self, &self.options, value).map(|entry| (entry, score))
            })
            .collect()
    }
    pub fn record_injections(&self, ids: &[String]) -> Result<(), ClientError> {
        for id in ids {
            self.call_raw(USE, map([("id", Value::from(id.as_str()))]))?;
        }
        Ok(())
    }
    pub fn put(
        &self,
        name: &str,
        content: &str,
        scope: &str,
        tier: MemoryTier,
    ) -> Result<String, ClientError> {
        self.put_inner(PUT, name, content, scope, tier)
    }
    pub fn try_put(
        &self,
        name: &str,
        content: &str,
        scope: &str,
        tier: MemoryTier,
    ) -> Result<String, ClientError> {
        self.put_inner(PUT, name, content, scope, tier)
    }
    pub fn remember(
        &self,
        content: &str,
        keywords: Vec<String>,
    ) -> Result<MemoryEntry, ClientError> {
        records::entry(&self.call_raw(
            REMEMBER,
            map([
                ("content", Value::from(content)),
                (
                    "keywords",
                    Value::Array(keywords.into_iter().map(Value::from).collect()),
                ),
            ]),
        )?)
    }
    /// Consolidation is a service operation, not an alias for `remember`.
    /// A null result is the stable no-op used for below-threshold or idempotent input.
    pub fn remember_consolidated(
        &self,
        name: &str,
        content: &str,
        keywords: Vec<String>,
        threshold: f64,
    ) -> Result<Option<String>, ClientError> {
        let response = self.object(
            REMEMBER_CONSOLIDATED,
            map([
                ("name", Value::from(name)),
                ("content", Value::from(content)),
                (
                    "keywords",
                    Value::Array(keywords.into_iter().map(Value::from).collect()),
                ),
                ("threshold", Value::from(threshold)),
            ]),
        )?;
        if let Some(id) = response
            .get("id")
            .or_else(|| response.get("memoryId"))
            .and_then(Value::as_str)
        {
            return Ok(Some(id.to_string()));
        }
        if response.get("remembered").and_then(Value::as_bool) == Some(false)
            || response.get("consolidated").and_then(Value::as_bool) == Some(false)
            || response.get("id").is_some_and(Value::is_null)
        {
            return Ok(None);
        }
        Err(ClientError::protocol(
            "response_malformed",
            "consolidated write response has no stable id or no-op marker",
        ))
    }
    /// Return a complete entry from consolidation.  An id-only write response
    /// is followed by typed `/get`; incomplete data is rejected rather than
    /// represented with guessed tier, score, or timestamps.
    pub fn remember_consolidated_entry(
        &self,
        name: &str,
        content: &str,
        keywords: Vec<String>,
        threshold: f64,
    ) -> Result<Option<MemoryEntry>, ClientError> {
        self.remember_consolidated_entry_with_options(
            &self.options,
            name,
            content,
            keywords,
            threshold,
        )
    }
    fn remember_consolidated_entry_with_options(
        &self,
        options: &CallOptions,
        name: &str,
        content: &str,
        keywords: Vec<String>,
        threshold: f64,
    ) -> Result<Option<MemoryEntry>, ClientError> {
        let response = self.object_with_options(
            options,
            REMEMBER_CONSOLIDATED,
            map([
                ("name", Value::from(name)),
                ("content", Value::from(content)),
                (
                    "keywords",
                    Value::Array(keywords.into_iter().map(Value::from).collect()),
                ),
                ("threshold", Value::from(threshold)),
            ]),
        )?;
        if response.get("remembered").and_then(Value::as_bool) == Some(false)
            || response.get("consolidated").and_then(Value::as_bool) == Some(false)
            || response.get("id").is_some_and(Value::is_null)
        {
            return Ok(None);
        }
        if response.get("tier").is_some() && response.get("content").is_some() {
            return records::entry(&Value::Object(response)).map(Some);
        }
        recall_entry(self, options, Value::Object(response)).map(Some)
    }
    fn put_inner(
        &self,
        operation: &str,
        name: &str,
        content: &str,
        scope: &str,
        tier: MemoryTier,
    ) -> Result<String, ClientError> {
        let row = self.object(
            operation,
            map([
                ("name", Value::from(name)),
                ("content", Value::from(content)),
                ("scope", Value::from(scope)),
                ("tier", Value::from(tier.as_str())),
            ]),
        )?;
        row.get("put")
            .or_else(|| row.get("id"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| {
                ClientError::protocol("response_malformed", "write response has no memory id")
            })
    }
    fn rows(
        &self,
        operation: &str,
        request: Map<String, Value>,
    ) -> Result<Vec<MemoryEntry>, ClientError> {
        self.array(operation, request)?
            .iter()
            .map(records::entry)
            .collect()
    }

    #[allow(clippy::too_many_arguments)]
    pub fn log_recall_with_replay(
        &self,
        timestamp: &str,
        scope: Option<&str>,
        query_chars: usize,
        hit_count: usize,
        full_chars: usize,
        injected_chars: usize,
        source: &str,
        query_excerpt: Option<&str>,
        client: Option<&str>,
        model: Option<&str>,
        session: Option<&str>,
        turn: Option<&str>,
        task: Option<&str>,
        actor: Option<&str>,
        mode: &str,
        replay_hits: &str,
        replay_context: &str,
    ) -> Result<(), ClientError> {
        let mut request = map([
            ("timestamp", Value::from(timestamp)),
            ("query_chars", Value::from(query_chars as u64)),
            ("hit_count", Value::from(hit_count as u64)),
            ("full_chars", Value::from(full_chars as u64)),
            ("injected_chars", Value::from(injected_chars as u64)),
            ("source", Value::from(source)),
            ("mode", Value::from(mode)),
            ("replay_hits", Value::from(replay_hits)),
            ("replay_context", Value::from(replay_context)),
        ]);
        for (key, value) in [
            ("scope", scope),
            ("query_excerpt", query_excerpt),
            ("client", client),
            ("model", model),
            ("session", session),
            ("turn", turn),
            ("task", task),
            ("actor", actor),
        ] {
            if let Some(value) = value {
                request.insert(key.into(), Value::from(value));
            }
        }
        self.call_raw(ACTIVITY, request).map(|_| ())
    }
}

impl<'a, T: ?Sized + Fn(&str, &Map<String, Value>) -> Result<Value, ClientError> + Send + Sync>
    MemoryBackendCall<'a, T>
{
    pub fn activity_json(&self, limit: usize) -> Result<Value, ClientError> {
        Ok(self.client.call_raw_with_options(
            &self.options,
            ACTIVITY,
            map([("limit", Value::from(limit as u64))]),
        )?)
    }
    /// Send one already-authenticated context-federation request using this
    /// request-scoped deadline/cancellation view.
    pub fn federate_json(&self, request: Map<String, Value>) -> Result<Value, ClientError> {
        self.client
            .call_raw_with_options(&self.options, FEDERATE, request)
    }
    pub fn metrics_json(&self) -> Result<Value, ClientError> {
        Ok(self
            .client
            .call_raw_with_options(&self.options, METRICS, Map::new())?)
    }
    pub fn embedder_dim(&self) -> Result<usize, ClientError> {
        let row = self
            .client
            .object_with_options(&self.options, "/health", Map::new())?;
        row.get("embedder_dim")
            .or_else(|| row.get("embedderDim"))
            .and_then(Value::as_u64)
            .map(|v| v as usize)
            .ok_or_else(|| {
                ClientError::protocol(
                    "response_malformed",
                    "health response has no embedder dimension",
                )
            })
    }
    pub fn delete(&self, id: &str) -> Result<bool, ClientError> {
        Ok(self
            .client
            .object_with_options(&self.options, DELETE, map([("id", Value::from(id))]))?
            .get("deleted")
            .and_then(Value::as_bool)
            .unwrap_or(false))
    }
    pub fn entries(&self, limit: usize) -> Result<Vec<MemoryEntry>, ClientError> {
        self.client
            .array_with_options(
                &self.options,
                LIST,
                map([("limit", Value::from(limit as u64))]),
            )?
            .into_iter()
            .map(|value| recall_entry(self.client, &self.options, value))
            .collect()
    }
    pub fn list(&self, scope: Option<&str>) -> Result<Vec<MemoryListRow>, ClientError> {
        let mut request = Map::new();
        if let Some(scope) = scope {
            request.insert("scope".into(), Value::from(scope));
        }
        self.client
            .array_with_options(&self.options, LIST, request)?
            .iter()
            .map(records::list_row)
            .collect()
    }
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<MemoryEntry>, ClientError> {
        self.client
            .array_with_options(
                &self.options,
                SEARCH,
                map([
                    ("query", Value::from(query)),
                    ("limit", Value::from(limit as u64)),
                ]),
            )?
            .iter()
            .map(records::entry)
            .collect()
    }
    pub fn scopes(&self) -> Result<Vec<String>, ClientError> {
        Ok(self
            .client
            .array_with_options(&self.options, SCOPES, Map::new())?
            .into_iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect())
    }
    pub fn get_full(&self, id: &str) -> Result<FullRecord, ClientError> {
        records::full(&self.client.call_raw_with_options(
            &self.options,
            GET,
            map([("id", Value::from(id))]),
        )?)
    }
    pub fn recall_scored(
        &self,
        query: &str,
        limit: usize,
        scopes: &[String],
    ) -> Result<Vec<(MemoryEntry, f64)>, ClientError> {
        let mut request = map([
            ("query", Value::from(query)),
            ("k", Value::from(limit as u64)),
            ("client", Value::from("coderight")),
            ("observe", Value::Bool(false)),
        ]);
        if let Some(scope) = scopes.first() {
            request.insert("scope".into(), Value::from(scope.as_str()));
        }
        if scopes.len() > 1 {
            request.insert(
                "cross".into(),
                Value::Array(scopes[1..].iter().cloned().map(Value::from).collect()),
            );
        }
        let response = self
            .client
            .call_raw_with_options(&self.options, RECALL, request)?;
        let hits = match response {
            Value::Array(hits) => hits,
            Value::Object(object) if object.get("status").is_some() => object
                .get("hits")
                .and_then(Value::as_array)
                .cloned()
                .ok_or_else(|| {
                    ClientError::protocol(
                        "response_malformed",
                        "recall status response has no hits array",
                    )
                })?,
            _ => {
                return Err(ClientError::protocol(
                    "response_malformed",
                    "recall response is not an array",
                ))
            }
        };
        hits.into_iter()
            .map(|value| {
                let score = value
                    .get("score")
                    .or_else(|| value.get("cos"))
                    .and_then(Value::as_f64)
                    .unwrap_or(0.0);
                recall_entry(self.client, &self.options, value).map(|entry| (entry, score))
            })
            .collect()
    }
    pub fn record_injections(&self, ids: &[String]) -> Result<(), ClientError> {
        for id in ids {
            self.client.call_raw_with_options(
                &self.options,
                USE,
                map([("id", Value::from(id.as_str()))]),
            )?;
        }
        Ok(())
    }
    pub fn put(
        &self,
        name: &str,
        content: &str,
        scope: &str,
        tier: MemoryTier,
    ) -> Result<String, ClientError> {
        self.put_inner(PUT, name, content, scope, tier)
    }
    pub fn try_put(
        &self,
        name: &str,
        content: &str,
        scope: &str,
        tier: MemoryTier,
    ) -> Result<String, ClientError> {
        self.put_inner(PUT, name, content, scope, tier)
    }
    fn put_inner(
        &self,
        operation: &str,
        name: &str,
        content: &str,
        scope: &str,
        tier: MemoryTier,
    ) -> Result<String, ClientError> {
        let row = self.client.object_with_options(
            &self.options,
            operation,
            map([
                ("name", Value::from(name)),
                ("content", Value::from(content)),
                ("scope", Value::from(scope)),
                ("tier", Value::from(tier.as_str())),
            ]),
        )?;
        row.get("put")
            .or_else(|| row.get("id"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| {
                ClientError::protocol("response_malformed", "write response has no memory id")
            })
    }
    pub fn remember(
        &self,
        content: &str,
        keywords: Vec<String>,
    ) -> Result<MemoryEntry, ClientError> {
        records::entry(&self.client.call_raw_with_options(
            &self.options,
            REMEMBER,
            map([
                ("content", Value::from(content)),
                (
                    "keywords",
                    Value::Array(keywords.into_iter().map(Value::from).collect()),
                ),
            ]),
        )?)
    }
    pub fn remember_consolidated(
        &self,
        name: &str,
        content: &str,
        keywords: Vec<String>,
        threshold: f64,
    ) -> Result<Option<MemoryEntry>, ClientError> {
        self.client.remember_consolidated_entry_with_options(
            &self.options,
            name,
            content,
            keywords,
            threshold,
        )
    }
    #[allow(clippy::too_many_arguments)]
    pub fn log_recall_with_replay(
        &self,
        timestamp: &str,
        scope: Option<&str>,
        query_chars: usize,
        hit_count: usize,
        full_chars: usize,
        injected_chars: usize,
        source: &str,
        query_excerpt: Option<&str>,
        client: Option<&str>,
        model: Option<&str>,
        session: Option<&str>,
        turn: Option<&str>,
        task: Option<&str>,
        actor: Option<&str>,
        mode: &str,
        replay_hits: &str,
        replay_context: &str,
    ) -> Result<(), ClientError> {
        let mut request = map([
            ("timestamp", Value::from(timestamp)),
            ("query_chars", Value::from(query_chars as u64)),
            ("hit_count", Value::from(hit_count as u64)),
            ("full_chars", Value::from(full_chars as u64)),
            ("injected_chars", Value::from(injected_chars as u64)),
            ("source", Value::from(source)),
            ("mode", Value::from(mode)),
            ("replay_hits", Value::from(replay_hits)),
            ("replay_context", Value::from(replay_context)),
        ]);
        for (key, value) in [
            ("scope", scope),
            ("query_excerpt", query_excerpt),
            ("client", client),
            ("model", model),
            ("session", session),
            ("turn", turn),
            ("task", task),
            ("actor", actor),
        ] {
            if let Some(value) = value {
                request.insert(key.into(), Value::from(value));
            }
        }
        self.client
            .call_raw_with_options(&self.options, ACTIVITY, request)
            .map(|_| ())
    }
}

fn map<const N: usize>(items: [(&str, Value); N]) -> Map<String, Value> {
    items
        .into_iter()
        .map(|(key, value)| (key.to_string(), value))
        .collect()
}

/// Bridge for the original transport-injected SDK while the resident surface is migrated.
pub fn protocol_transport<F>(transport: F) -> Box<MemoryTransport>
where
    F: Fn(&str, &Map<String, Value>) -> ProtocolResult<Value> + Send + Sync + 'static,
{
    Box::new(move |operation, request| {
        (transport)(operation, request).map_err(|error: ProtocolError| ClientError::Protocol {
            code: error.code,
            message: error.message,
            details: error.details,
        })
    })
}
