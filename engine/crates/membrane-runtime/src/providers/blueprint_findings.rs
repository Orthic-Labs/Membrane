//! Blueprint D0a/D0b production integration (design §7, §7.1 item 6).
//!
//! The Membrane adapter consumes Blueprint exclusively through its public
//! resident findings service: the daemon IPC surface that serves
//! `findings.get` (newline-delimited JSON envelopes over a per-user Unix
//! domain socket; see `blueprint/src/service/protocol.mjs`). This module is
//! the one production seam for that consumption.
//!
//! Freshness honesty: whatever the service answers is carried verbatim —
//! `current` yields an exact `snapshot_checker_exact` lane, `stale` yields no
//! exact lane plus a typed `stale_generation` omission, and any transport or
//! protocol failure yields typed unavailability. Nothing here ever fabricates
//! a generation, freshness, or delta: when Blueprint evidence cannot be
//! obtained, the snapshot says so and Blueprint-required obligations simply
//! remain unsatisfied.

use membrane_protocol::diagnostics::{ObservationV1, SeverityHint, SourceClass, TypedOmission};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// Wire protocol version of the Blueprint daemon envelope (`protocol.mjs`).
const PROTOCOL_VERSION: u64 = 1;

/// Default bounded wait for one findings round-trip. Detection scans the
/// repository, so this matches the daemon's non-build maximum rather than a
/// handshake-scale timeout.
pub const DEFAULT_FETCH_TIMEOUT_MS: u64 = 30_000;

#[derive(Debug, Clone, PartialEq)]
pub struct BlueprintFinding {
    pub rule_id: String,
    pub path: String,
    pub start_line: Option<u32>,
    pub end_line: Option<u32>,
    pub name: Option<String>,
    pub specifier: Option<String>,
    /// Raw severity from the rule registry (`"error"` for BP001/BP002/BP003).
    pub severity: Option<String>,
    pub fingerprint: String,
}

impl BlueprintFinding {
    /// Whether the finding should gate as blocking under planner policy
    /// (registry severities of `"error"` gate; everything else advises).
    pub fn is_blocking(&self) -> bool {
        self.severity.as_deref() == Some("error")
    }

    /// Normalized observation bound to the emitting generation as provider
    /// version so evidence identity survives correlation (design §5.1).
    pub fn to_observation(&self, generation_id: &str) -> ObservationV1 {
        let message = match (&self.name, &self.specifier) {
            (Some(name), Some(specifier)) => {
                format!("{} imports {}", name, specifier)
            }
            (Some(name), None) => name.clone(),
            (None, Some(specifier)) => format!("specifier {specifier}"),
            (None, None) => self.rule_id.clone(),
        };
        let anchor = self.name.as_deref().map(|name| format!("symbol:{name}"));
        ObservationV1 {
            observation_id: format!("blueprint:{}", self.fingerprint),
            provider_id: "blueprint".to_string(),
            provider_version: generation_id.to_string(),
            code: self.rule_id.clone(),
            path: self.path.clone(),
            range: membrane_protocol::diagnostics::SourceRange {
                start_line: self.start_line.unwrap_or(1).max(1),
                start_column: 1,
                end_line: self.end_line.unwrap_or(self.start_line.unwrap_or(1)).max(1),
                end_column: 1,
            },
            message,
            semantic_anchor: anchor,
            source_class: SourceClass::RepositoryFinding,
            cost_class: membrane_protocol::diagnostics::CostClass::Instant,
            severity_hint: if self.is_blocking() {
                SeverityHint::Blocking
            } else {
                SeverityHint::Advisory
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct BlueprintFindingsResult {
    /// Sealed generation id the bundle was computed against (never synthesized).
    pub generation_id: String,
    /// `"current"` or `"stale"` exactly as the service reported it.
    pub freshness: String,
    pub findings: Vec<BlueprintFinding>,
    /// Typed omissions from the detection pipeline (ambiguous resolution,
    /// unsupported languages, stale overlays, ...).
    pub omissions: Vec<TypedOmission>,
}

impl BlueprintFindingsResult {
    pub fn freshness_is_current(&self) -> bool {
        self.freshness == "current"
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlueprintFindingsError {
    /// The public findings service could not be reached at all.
    Unavailable(String),
    /// The service did not answer within the bounded wait.
    DeadlineExceeded,
    /// The service answered but not in the expected envelope.
    Protocol(String),
}

impl std::fmt::Display for BlueprintFindingsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable(detail) => write!(f, "unavailable: {detail}"),
            Self::DeadlineExceeded => write!(f, "deadline exceeded"),
            Self::Protocol(detail) => write!(f, "protocol violation: {detail}"),
        }
    }
}

/// One production seam for consuming Blueprint's public findings service.
/// Test doubles replace the transport entirely.
pub trait BlueprintFindingsClient: Send {
    fn fetch(
        &mut self,
        repo_root: &Path,
        timeout_ms: u64,
    ) -> Result<BlueprintFindingsResult, BlueprintFindingsError>;
}

/// Default endpoint resolution mirroring `blueprint/src/service/paths.mjs`:
/// `BLUEPRINT_DAEMON_ENDPOINT` first, then `~/.blueprint/blueprint.sock`.
pub fn daemon_endpoint_from_environment() -> PathBuf {
    if let Ok(value) = std::env::var("BLUEPRINT_DAEMON_ENDPOINT") {
        if !value.trim().is_empty() {
            return PathBuf::from(value);
        }
    }
    if let Some(home) = std::env::var_os("HOME") {
        let mut path = PathBuf::from(home);
        path.push(".blueprint");
        path.push("blueprint.sock");
        return path;
    }
    PathBuf::from(".blueprint/blueprint.sock")
}

/// Production client speaking the daemon newline-delimited JSON protocol over
/// a Unix domain socket. Windows named-pipe IPC degrades to typed
/// unavailability until the pipe transport lands; the degradation is visible
/// in every snapshot as `blueprint_unavailable`, never hidden.
pub struct DaemonFindingsClient {
    endpoint: PathBuf,
}

impl DaemonFindingsClient {
    pub fn new(endpoint: PathBuf) -> Self {
        Self { endpoint }
    }

    pub fn from_environment() -> Self {
        Self::new(daemon_endpoint_from_environment())
    }
}

impl BlueprintFindingsClient for DaemonFindingsClient {
    fn fetch(
        &mut self,
        repo_root: &Path,
        timeout_ms: u64,
    ) -> Result<BlueprintFindingsResult, BlueprintFindingsError> {
        fetch_over_socket(&self.endpoint, repo_root, timeout_ms)
    }
}

fn fetch_over_socket(
    endpoint: &Path,
    repo_root: &Path,
    timeout_ms: u64,
) -> Result<BlueprintFindingsResult, BlueprintFindingsError> {
    #[cfg(unix)]
    {
        use std::io::{BufRead, BufReader, Write};
        use std::os::unix::net::UnixStream;
        if !endpoint.exists() {
            return Err(BlueprintFindingsError::Unavailable(format!(
                "daemon endpoint {} does not exist",
                endpoint.display()
            )));
        }
        let stream = UnixStream::connect(endpoint).map_err(|error| {
            BlueprintFindingsError::Unavailable(format!(
                "connect to {} failed: {error}",
                endpoint.display()
            ))
        })?;
        let timeout = std::time::Duration::from_millis(timeout_ms.max(1));
        let _ = stream.set_read_timeout(Some(timeout));
        let _ = stream.set_write_timeout(Some(timeout));
        let mut stream = stream;
        let request_id = format!(
            "membrane-diag-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|elapsed| elapsed.as_millis())
                .unwrap_or(0)
        );
        let request = serde_json::json!({
            "protocolVersion": PROTOCOL_VERSION,
            "requestId": request_id,
            "repoId": null,
            "generation": null,
            "method": "findings.get",
            "deadlineMs": timeout_ms.min(30_000).max(10),
            "input": { "repoRoot": repo_root.to_string_lossy() },
        });
        let mut line = request.to_string();
        line.push('\n');
        stream
            .write_all(line.as_bytes())
            .map_err(|error| BlueprintFindingsError::Unavailable(format!("write failed: {error}")))?;
        stream
            .flush()
            .map_err(|error| BlueprintFindingsError::Unavailable(format!("flush failed: {error}")))?;
        let mut reader = BufReader::new(stream);
        let mut response = String::new();
        match reader.read_line(&mut response) {
            Ok(0) => Err(BlueprintFindingsError::Unavailable(
                "daemon closed the socket without responding".into(),
            )),
            Ok(_) => parse_envelope(&response),
            Err(error)
                if error.kind() == std::io::ErrorKind::WouldBlock
                    || error.kind() == std::io::ErrorKind::TimedOut =>
            {
                Err(BlueprintFindingsError::DeadlineExceeded)
            }
            Err(error) => Err(BlueprintFindingsError::Unavailable(format!(
                "read failed: {error}"
            ))),
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (endpoint, repo_root, timeout_ms);
        Err(BlueprintFindingsError::Unavailable(
            "blueprint daemon IPC requires a unix-domain socket transport".into(),
        ))
    }
}

fn parse_envelope(line: &str) -> Result<BlueprintFindingsResult, BlueprintFindingsError> {
    let value: Value = serde_json::from_str(line.trim())
        .map_err(|error| BlueprintFindingsError::Protocol(format!("invalid JSON envelope: {error}")))?;
    if value["ok"].as_bool() != Some(true) {
        let detail = value["error"]["detail"]
            .as_str()
            .or_else(|| value["error"]["code"].as_str())
            .unwrap_or("findings service returned ok=false");
        // A typed refusal (unknown root, stale-blocked) still proves the
        // service exists; classify as unavailable with its own words.
        return Err(BlueprintFindingsError::Unavailable(detail.to_string()));
    }
    let result = &value["result"];
    let generation_id = result["generationId"]
        .as_str()
        .filter(|id| !id.is_empty())
        .ok_or_else(|| {
            BlueprintFindingsError::Protocol("result is missing a non-empty generationId".into())
        })?
        .to_string();
    let freshness = result["freshness"].as_str().unwrap_or("stale").to_string();
    let mut findings = Vec::new();
    if let Some(items) = result["findings"].as_array() {
        for item in items {
            let rule_id = item["ruleId"].as_str().unwrap_or_default();
            let path = item["path"].as_str().unwrap_or_default();
            if rule_id.is_empty() || path.is_empty() {
                continue;
            }
            findings.push(BlueprintFinding {
                rule_id: rule_id.to_string(),
                path: path.to_string(),
                start_line: item["startLine"].as_u64().and_then(|v| u32::try_from(v).ok()),
                end_line: item["endLine"].as_u64().and_then(|v| u32::try_from(v).ok()),
                name: item["name"].as_str().map(str::to_string),
                specifier: item["specifier"].as_str().map(str::to_string),
                severity: item["severity"].as_str().map(str::to_string),
                fingerprint: item["fingerprint"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
            });
        }
    }
    let mut omissions = Vec::new();
    if let Some(items) = result["omissions"].as_array() {
        for omission in items {
            let code = omission["code"].as_str().unwrap_or_default();
            if code.is_empty() {
                continue;
            }
            omissions.push(TypedOmission {
                code: code.to_string(),
                detail: omission["detail"].as_str().unwrap_or_default().to_string(),
            });
        }
    }
    Ok(BlueprintFindingsResult {
        generation_id,
        freshness,
        findings,
        omissions,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_envelope_maps_findings_omissions_and_freshness() {
        let line = serde_json::json!({
            "protocolVersion": 1,
            "requestId": "r1",
            "ok": true,
            "generation": null,
            "result": {
                "schemaVersion": 1,
                "kind": "findings.get",
                "root": "/repo",
                "generationId": "gen-abc",
                "freshness": "current",
                "findings": [
                    {
                        "fingerprint": "fp-1",
                        "ruleId": "BP001",
                        "path": "src/main.ts",
                        "startLine": 3,
                        "endLine": 4,
                        "name": "RunPolicy",
                        "specifier": "./run-policy.js",
                        "severity": "error"
                    },
                    {"fingerprint": "fp-2", "path": "src/x.ts"}
                ],
                "omissions": [
                    {"code": "unsupported_language", "detail": "elided"}
                ]
            },
            "error": null
        })
        .to_string();
        let parsed = parse_envelope(&line).unwrap();
        assert_eq!(parsed.generation_id, "gen-abc");
        assert!(parsed.freshness_is_current());
        // The malformed second entry (missing ruleId) is skipped without
        // poisoning the well-formed first one.
        assert_eq!(parsed.findings.len(), 1);
        assert_eq!(parsed.findings[0].rule_id, "BP001");
        assert_eq!(parsed.findings[0].start_line, Some(3));
        assert_eq!(parsed.omissions.len(), 1);
        assert_eq!(parsed.omissions[0].code, "unsupported_language");

        let observation = parsed.findings[0].to_observation(&parsed.generation_id);
        assert_eq!(observation.provider_id, "blueprint");
        assert_eq!(observation.provider_version, "gen-abc");
        assert_eq!(observation.code, "BP001");
        assert_eq!(observation.semantic_anchor, Some("symbol:RunPolicy".into()));
        assert!(matches!(observation.severity_hint, SeverityHint::Blocking));
        assert!(matches!(
            observation.source_class,
            SourceClass::RepositoryFinding
        ));
    }

    #[test]
    fn parse_envelope_refuses_error_envelopes_as_unavailable() {
        let line = serde_json::json!({
            "protocolVersion": 1,
            "requestId": "r2",
            "ok": false,
            "generation": null,
            "result": null,
            "error": {"code": "graph_missing", "detail": "Graph store is missing."}
        })
        .to_string();
        let error = parse_envelope(&line).unwrap_err();
        assert!(matches!(error, BlueprintFindingsError::Unavailable(_)));
        assert!(error.to_string().contains("Graph store is missing."));
    }

    #[test]
    fn parse_envelope_rejects_results_without_generation() {
        let line = serde_json::json!({
            "protocolVersion": 1,
            "requestId": "r3",
            "ok": true,
            "generation": null,
            "result": {"kind": "findings.get", "freshness": "current", "findings": []},
            "error": null
        })
        .to_string();
        assert!(matches!(
            parse_envelope(&line),
            Err(BlueprintFindingsError::Protocol(_))
        ));
    }
}
