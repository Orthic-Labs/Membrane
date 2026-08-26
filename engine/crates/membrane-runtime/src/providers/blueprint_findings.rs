//! Blueprint D0a/D0b production integration (design §7, §7.1 item 6).
//!
//! The Membrane adapter consumes Blueprint exclusively through its public
//! resident findings service: the daemon IPC surface that serves
//! `findings.get` (newline-delimited JSON envelopes over Hub-owned IPC; see
//! `blueprint/src/service/protocol.mjs`). This module is the one production
//! seam for that consumption.
//!
//! Freshness honesty: whatever the service answers is carried verbatim —
//! `current` yields an exact `snapshot_checker_exact` lane, `stale` yields no
//! exact lane plus a typed `stale_generation` omission, and any transport or
//! protocol failure yields typed unavailability. Nothing here ever fabricates
//! a generation, freshness, or delta: when Blueprint evidence cannot be
//! obtained, the snapshot says so and Blueprint-required obligations simply
//! remain unsatisfied.
//!
//! Blueprint D0 exactness invariant (binding contract §7.1 item 5):
//! Every bundle is bound at emission to the exact sealed generation plus the
//! sha256 content hash of every scanned file. Before Blueprint may produce
//! Complete exact D0 coverage the caller MUST:
//!  1. Identify the relevant touched/required scope (typically
//!     `WorkspaceEpochV1.changed_file_hashes` intersected with the required
//!     scope).
//!  2. Compare `per_file_hashes` retained here against
//!     `WorkspaceEpochV1.changed_file_hashes` (exact byte identity).
//!  3. Prove every relevant required file has a compatible hash. Fail closed:
//!     - current + all required hashes match sealed host bytes + required scope
//!       fully covered => exact D0 MAY be Complete (subject to other obligations)
//!     - current + any relevant hash mismatch => no exact D0 coverage => typed
//!       `hash_mismatch`/`source_identity_mismatch` omission, obligation remains
//!       unsatisfied => no `clean_exact`
//!     - current + required hash evidence missing => exactness unproven =>
//!       affected obligation remains unsatisfied
//!     - stale => freshness=Stale => typed `stale_generation` omission => no exact D0 lane
//!     - unavailable => generation=None, freshness=Unknown, typed
//!       `blueprint_unavailable` => no exact D0 lane
//! This module retains and exposes the per-file hashes so the lead
//! (`live_diagnostics.rs` / `live_diagnostics_service.rs`) can perform that
//! comparison. It does NOT itself decide lane completeness.

use membrane_protocol::diagnostics::{
    ObservationV1, SeverityHint, SourceClass, TypedOmission, WorkspaceEpochV1,
};
use serde_json::Value;
#[cfg(windows)]
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Wire protocol version of the Blueprint daemon envelope (`protocol.mjs`).
const PROTOCOL_VERSION: u64 = 1;

/// Default bounded wait for one findings round-trip. Detection scans the
/// repository, so this matches the daemon's non-build maximum rather than a
/// handshake-scale timeout.
pub const DEFAULT_FETCH_TIMEOUT_MS: u64 = 30_000;

/// Coverage-affecting omission codes: when present and relevant to the
/// required scope, they prevent `Complete` exact coverage for the affected
/// capability/scope. A consumer must produce `Partial`/`Unavailable`/
/// `Unsupported` semantics for that scope instead and leave the obligation
/// unsatisfied unless another qualified exact provider satisfies it.
pub const COVERAGE_AFFECTING_OMISSION_CODES: &[&str] = &[
    "resolution_ambiguous",
    "parse_failed",
    "unsupported_language",
    "outside_scanned_set",
    "open_export_surface",
    "stale_generation",
    "hash_mismatch",
    "source_identity_mismatch",
    "missing_required_content_hash",
    "star_cycle",
    "star_depth_exceeded",
];

/// Returns true if `code` is coverage-affecting (prevents Complete exact
/// coverage when relevant to the required scope).
pub fn is_coverage_affecting_omission(code: &str) -> bool {
    COVERAGE_AFFECTING_OMISSION_CODES.contains(&code)
}

/// Returns true if the typed omission is coverage-affecting.
pub fn is_coverage_affecting_typed_omission(omission: &TypedOmission) -> bool {
    is_coverage_affecting_omission(&omission.code)
}

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
    /// Per-file sha256 content hashes bound at emission (`§7.1 item 5`):
    /// `Map<repoRelativePath, "sha256:<hex>">` covering every scanned file that
    /// contributed to this bundle. Retained so the lead can compare against
    /// `WorkspaceEpochV1.changed_file_hashes` to prove byte identity before
    /// claiming Complete exact D0 coverage. Empty when the service predates the
    /// field or when no files were scanned — callers must treat missing
    /// required hashes as unproven exactness (fail closed).
    pub per_file_hashes: BTreeMap<String, String>,
}

impl BlueprintFindingsResult {
    pub fn freshness_is_current(&self) -> bool {
        self.freshness == "current"
    }

    /// Accessor for the retained per-file content hashes.
    pub fn per_file_hashes(&self) -> &BTreeMap<String, String> {
        &self.per_file_hashes
    }

    /// Hash for a single path, if retained.
    pub fn hash_for(&self, path: &str) -> Option<&str> {
        self.per_file_hashes.get(path).map(|v| v.as_str())
    }

    /// Coverage-affecting omissions present in this result (relevant to
    /// exact coverage). The caller should filter by required scope when
    /// known; this helper reports any coverage-affecting omission at all.
    pub fn coverage_affecting_omissions(&self) -> Vec<&TypedOmission> {
        self.omissions
            .iter()
            .filter(|o| is_coverage_affecting_typed_omission(o))
            .collect()
    }

    /// True if any coverage-affecting omission is present.
    pub fn has_coverage_affecting_omissions(&self) -> bool {
        self.omissions
            .iter()
            .any(|o| is_coverage_affecting_typed_omission(o))
    }

    /// Verify exact byte identity against a sealed [`WorkspaceEpochV1`].
    ///
    /// Returns typed omissions describing hash mismatches or missing required
    /// hashes. The caller SHOULD intersect `required_paths` with the epoch's
    /// changed files; when `required_paths` is empty this checks every
    /// `changed_file_hashes` entry. An empty return means every relevant hash
    /// matches and is present; a non-empty return means exact D0 MUST NOT
    /// become `Complete` and must surface these omissions instead.
    pub fn verify_hashes_against_epoch(
        &self,
        epoch: &WorkspaceEpochV1,
        required_paths: Option<&[String]>,
    ) -> Vec<TypedOmission> {
        let mut issues = Vec::new();
        let required_set: Option<std::collections::HashSet<&str>> =
            required_paths.map(|paths| paths.iter().map(|s| s.as_str()).collect());
        for changed in &epoch.changed_file_hashes {
            if let Some(filter) = &required_set {
                if !filter.contains(changed.path.as_str()) {
                    continue;
                }
            }
            match self.per_file_hashes.get(&changed.path) {
                None => issues.push(TypedOmission {
                    code: "missing_required_content_hash".to_string(),
                    detail: format!(
                        "blueprint bundle missing hash for required file {} (generation {})",
                        changed.path, self.generation_id
                    ),
                }),
                Some(bundle_hash) if bundle_hash != &changed.hash => issues.push(TypedOmission {
                    code: "hash_mismatch".to_string(),
                    detail: format!(
                        "hash mismatch for {}: blueprint {} vs sealed epoch {}",
                        changed.path, bundle_hash, changed.hash
                    ),
                }),
                Some(_) => {}
            }
        }
        issues
    }

    /// Convenience: true only when freshness is current AND every relevant hash
    /// is present and matches the sealed epoch.
    pub fn is_exact_identity_verified_against(
        &self,
        epoch: &WorkspaceEpochV1,
        required_paths: Option<&[String]>,
    ) -> bool {
        if !self.freshness_is_current() {
            return false;
        }
        self.verify_hashes_against_epoch(epoch, required_paths)
            .is_empty()
    }

    /// Returns true if this result may contribute an exact D0 lane. Conditions:
    /// freshness is current, no coverage-affecting omissions, and hash identity
    /// is verified against the epoch. The lead must still also ensure the
    /// required scope is fully covered by Blueprint's capabilities.
    pub fn may_produce_exact_lane(
        &self,
        epoch: &WorkspaceEpochV1,
        required_paths: Option<&[String]>,
    ) -> bool {
        self.freshness_is_current()
            && !self.has_coverage_affecting_omissions()
            && self.is_exact_identity_verified_against(epoch, required_paths)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlueprintFindingsError {
    /// The public findings service could not be reached at all.
    Unavailable(String),
    /// The service did not answer within the bounded wait.
    DeadlineExceeded,
    /// Blueprint is live, but this explicit root is not enrolled.
    RootNotEnrolled(String),
    /// Blueprint is live, but its enrolled root has no sealed graph yet.
    GraphMissing(String),
    /// Blueprint is live, but exact evidence is stale or generation-mismatched.
    Stale(String),
    /// The service answered but not in the expected envelope.
    Protocol(String),
}

impl std::fmt::Display for BlueprintFindingsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable(detail) => write!(f, "unavailable: {detail}"),
            Self::DeadlineExceeded => write!(f, "deadline exceeded"),
            Self::RootNotEnrolled(detail) => write!(f, "root_not_enrolled: {detail}"),
            Self::GraphMissing(detail) => write!(f, "graph_missing: {detail}"),
            Self::Stale(detail) => write!(f, "stale: {detail}"),
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
        paths: &[String],
    ) -> Result<BlueprintFindingsResult, BlueprintFindingsError>;
}

/// Default endpoint resolution mirroring `blueprint/src/service/paths.mjs`.
/// Windows uses Hub's per-user named pipe; Membrane never starts a daemon or
/// falls back to an embedded/one-shot provider.
pub fn daemon_endpoint_from_environment() -> PathBuf {
    if let Ok(value) = std::env::var("BLUEPRINT_DAEMON_ENDPOINT") {
        if !value.trim().is_empty() {
            return PathBuf::from(value);
        }
    }
    #[cfg(windows)]
    {
        let profile = std::env::var("USERPROFILE").unwrap_or_else(|_| "".to_owned());
        let suffix = hex::encode(Sha256::digest(profile.as_bytes()));
        return PathBuf::from(format!(r"\\.\pipe\membrane-blueprint-{}", &suffix[..16]));
    }
    #[cfg(not(windows))]
    {
        if let Some(home) = std::env::var_os("HOME") {
            let mut path = PathBuf::from(home);
            path.push(".blueprint");
            path.push("blueprint.sock");
            return path;
        }
        PathBuf::from(".blueprint/blueprint.sock")
    }
}

/// Production client speaking daemon newline-delimited JSON over Hub-owned
/// Unix socket or Windows named pipe. Membrane never launches Blueprint or
/// substitutes an embedded provider.
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
        paths: &[String],
    ) -> Result<BlueprintFindingsResult, BlueprintFindingsError> {
        fetch_over_socket(&self.endpoint, repo_root, timeout_ms, paths)
    }
}

fn fetch_over_socket(
    endpoint: &Path,
    repo_root: &Path,
    timeout_ms: u64,
    paths: &[String],
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
        let mut input = serde_json::json!({ "repoRoot": repo_root.to_string_lossy() });
        if !paths.is_empty() {
            input["paths"] = serde_json::json!(paths);
        }
        let request = serde_json::json!({
            "protocolVersion": PROTOCOL_VERSION,
            "requestId": request_id,
            "repoId": null,
            "generation": null,
            "method": "findings.get",
            "deadlineMs": timeout_ms.min(30_000).max(10),
            "input": input,
        });
        let mut line = request.to_string();
        line.push('\n');
        stream.write_all(line.as_bytes()).map_err(|error| {
            BlueprintFindingsError::Unavailable(format!("write failed: {error}"))
        })?;
        stream.flush().map_err(|error| {
            BlueprintFindingsError::Unavailable(format!("flush failed: {error}"))
        })?;
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
        #[cfg(windows)]
        {
            let timeout = std::time::Duration::from_millis(timeout_ms.clamp(10, 30_000));
            let request_id = format!(
                "membrane-diag-{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|elapsed| elapsed.as_millis())
                    .unwrap_or(0)
            );
            let mut input = serde_json::json!({ "repoRoot": repo_root.to_string_lossy() });
            if !paths.is_empty() {
                input["paths"] = serde_json::json!(paths);
            }
            let mut request = serde_json::json!({
                "protocolVersion": PROTOCOL_VERSION,
                "requestId": request_id,
                "repoId": null,
                "generation": null,
                "method": "findings.get",
                "deadlineMs": timeout_ms.clamp(10, 30_000),
                "input": input,
            })
            .to_string()
            .into_bytes();
            request.push(b'\n');
            let response = membrane_federation::blueprint_client::exchange_windows_named_pipe(
                endpoint,
                &request,
                16 * 1024,
                timeout,
            )
            .map_err(|error| {
                if error == "__blueprint_pipe_timeout__" {
                    BlueprintFindingsError::DeadlineExceeded
                } else {
                    BlueprintFindingsError::Unavailable(error)
                }
            })?;
            let line = std::str::from_utf8(&response).map_err(|error| {
                BlueprintFindingsError::Protocol(format!("response is not UTF-8: {error}"))
            })?;
            return parse_envelope(line);
        }
        #[cfg(not(windows))]
        {
            let _ = (endpoint, repo_root, timeout_ms, paths);
            Err(BlueprintFindingsError::Unavailable(
                "Blueprint daemon IPC is unavailable on this host".into(),
            ))
        }
    }
}

fn parse_envelope(line: &str) -> Result<BlueprintFindingsResult, BlueprintFindingsError> {
    let value: Value = serde_json::from_str(line.trim()).map_err(|error| {
        BlueprintFindingsError::Protocol(format!("invalid JSON envelope: {error}"))
    })?;
    if value["ok"].as_bool() != Some(true) {
        let code = value["error"]["code"]
            .as_str()
            .unwrap_or("blueprint_failed");
        let detail = value["error"]["detail"]
            .as_str()
            .or_else(|| value["error"]["code"].as_str())
            .unwrap_or("findings service returned ok=false");
        return Err(match code {
            "root_not_enrolled" => BlueprintFindingsError::RootNotEnrolled(detail.to_string()),
            "graph_missing" => BlueprintFindingsError::GraphMissing(detail.to_string()),
            "not_configured" => BlueprintFindingsError::Unavailable(detail.to_string()),
            "stale_blocked" | "generation_mismatch" => {
                BlueprintFindingsError::Stale(detail.to_string())
            }
            _ => BlueprintFindingsError::Protocol(format!("{code}: {detail}")),
        });
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
                start_line: item["startLine"]
                    .as_u64()
                    .and_then(|v| u32::try_from(v).ok()),
                end_line: item["endLine"].as_u64().and_then(|v| u32::try_from(v).ok()),
                name: item["name"].as_str().map(str::to_string),
                specifier: item["specifier"].as_str().map(str::to_string),
                severity: item["severity"].as_str().map(str::to_string),
                fingerprint: item["fingerprint"].as_str().unwrap_or_default().to_string(),
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
    // Per-file content hashes (§7.1 item 5): Map<String,String> of sha256 hashes.
    // Retained verbatim so the lead can prove byte identity against
    // WorkspaceEpochV1.changed_file_hashes before claiming Complete exact D0.
    let mut per_file_hashes = BTreeMap::new();
    if let Some(map) = result["perFileContentHashes"].as_object() {
        for (path, hash_value) in map {
            if let Some(hash) = hash_value.as_str() {
                if !hash.is_empty() {
                    per_file_hashes.insert(path.clone(), hash.to_string());
                }
            }
        }
    }
    Ok(BlueprintFindingsResult {
        generation_id,
        freshness,
        findings,
        omissions,
        per_file_hashes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use membrane_protocol::diagnostics::{ChangedFileHashV1, WorkspaceEpochV1};

    fn epoch_with_hashes(pairs: &[(&str, &str)]) -> WorkspaceEpochV1 {
        let mut epoch = WorkspaceEpochV1::default();
        epoch.epoch = 5;
        epoch.changed_file_hashes = pairs
            .iter()
            .map(|(path, hash)| ChangedFileHashV1 {
                path: path.to_string(),
                hash: hash.to_string(),
            })
            .collect();
        epoch
    }

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
        assert!(parsed.per_file_hashes.is_empty());

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
    fn parse_envelope_retains_per_file_hashes() {
        let line = serde_json::json!({
            "protocolVersion": 1,
            "requestId": "r1",
            "ok": true,
            "generation": null,
            "result": {
                "schemaVersion": 1,
                "kind": "findings.get",
                "root": "/repo",
                "generationId": "gen-hashes",
                "freshness": "current",
                "findings": [],
                "omissions": [],
                "perFileContentHashes": {
                    "src/a.ts": "sha256:aaa",
                    "src/b.ts": "sha256:bbb"
                }
            },
            "error": null
        })
        .to_string();
        let parsed = parse_envelope(&line).unwrap();
        assert_eq!(parsed.per_file_hashes.len(), 2);
        assert_eq!(
            parsed.per_file_hashes.get("src/a.ts").unwrap(),
            "sha256:aaa"
        );
        assert_eq!(
            parsed.per_file_hashes.get("src/b.ts").unwrap(),
            "sha256:bbb"
        );
        assert_eq!(parsed.hash_for("src/a.ts"), Some("sha256:aaa"));
        // Missing file returns None.
        assert_eq!(parsed.hash_for("src/missing.ts"), None);
    }

    #[test]
    fn parse_envelope_tolerates_missing_per_file_hashes_field() {
        let line = serde_json::json!({
            "protocolVersion": 1,
            "requestId": "r1",
            "ok": true,
            "generation": null,
            "result": {
                "schemaVersion": 1,
                "kind": "findings.get",
                "root": "/repo",
                "generationId": "gen-no-hashes",
                "freshness": "current",
                "findings": [],
                "omissions": []
            },
            "error": null
        })
        .to_string();
        let parsed = parse_envelope(&line).unwrap();
        assert!(parsed.per_file_hashes.is_empty());
        // Verification must then fail closed when required hashes are needed.
        let epoch = epoch_with_hashes(&[("src/a.ts", "sha256:aaa")]);
        let issues = parsed.verify_hashes_against_epoch(&epoch, None);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].code, "missing_required_content_hash");
    }

    #[test]
    fn parse_envelope_preserves_graph_missing_refusal() {
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
        assert!(matches!(error, BlueprintFindingsError::GraphMissing(_)));
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

    #[test]
    fn hash_verification_passes_when_all_match() {
        let line = serde_json::json!({
            "protocolVersion": 1,
            "requestId": "r1",
            "ok": true,
            "generation": null,
            "result": {
                "schemaVersion": 1,
                "kind": "findings.get",
                "root": "/repo",
                "generationId": "gen-1",
                "freshness": "current",
                "findings": [],
                "omissions": [],
                "perFileContentHashes": {
                    "src/a.ts": "sha256:aaa",
                    "src/b.ts": "sha256:bbb"
                }
            },
            "error": null
        })
        .to_string();
        let parsed = parse_envelope(&line).unwrap();
        let epoch = epoch_with_hashes(&[("src/a.ts", "sha256:aaa"), ("src/b.ts", "sha256:bbb")]);
        assert!(parsed.is_exact_identity_verified_against(&epoch, None));
        assert!(parsed.may_produce_exact_lane(&epoch, None));
    }

    #[test]
    fn hash_mismatch_produces_typed_omission_and_blocks_exact_lane() {
        let line = serde_json::json!({
            "protocolVersion": 1,
            "requestId": "r1",
            "ok": true,
            "generation": null,
            "result": {
                "schemaVersion": 1,
                "kind": "findings.get",
                "root": "/repo",
                "generationId": "gen-1",
                "freshness": "current",
                "findings": [],
                "omissions": [],
                "perFileContentHashes": {
                    "src/a.ts": "sha256:old",
                    "src/b.ts": "sha256:bbb"
                }
            },
            "error": null
        })
        .to_string();
        let parsed = parse_envelope(&line).unwrap();
        let epoch = epoch_with_hashes(&[("src/a.ts", "sha256:new"), ("src/b.ts", "sha256:bbb")]);
        let issues = parsed.verify_hashes_against_epoch(&epoch, None);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].code, "hash_mismatch");
        assert!(issues[0].detail.contains("src/a.ts"));
        assert!(!parsed.is_exact_identity_verified_against(&epoch, None));
        assert!(!parsed.may_produce_exact_lane(&epoch, None));
    }

    #[test]
    fn missing_hash_blocks_exact_lane() {
        let line = serde_json::json!({
            "protocolVersion": 1,
            "requestId": "r1",
            "ok": true,
            "generation": null,
            "result": {
                "schemaVersion": 1,
                "kind": "findings.get",
                "root": "/repo",
                "generationId": "gen-1",
                "freshness": "current",
                "findings": [],
                "omissions": [],
                "perFileContentHashes": {
                    "src/a.ts": "sha256:aaa"
                }
            },
            "error": null
        })
        .to_string();
        let parsed = parse_envelope(&line).unwrap();
        let epoch = epoch_with_hashes(&[("src/a.ts", "sha256:aaa"), ("src/b.ts", "sha256:bbb")]);
        let issues = parsed.verify_hashes_against_epoch(&epoch, None);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].code, "missing_required_content_hash");
        assert!(!parsed.is_exact_identity_verified_against(&epoch, None));
    }

    #[test]
    fn required_paths_filter_limits_verification_scope() {
        let line = serde_json::json!({
            "protocolVersion": 1,
            "requestId": "r1",
            "ok": true,
            "generation": null,
            "result": {
                "schemaVersion": 1,
                "kind": "findings.get",
                "root": "/repo",
                "generationId": "gen-1",
                "freshness": "current",
                "findings": [],
                "omissions": [],
                "perFileContentHashes": {
                    "src/a.ts": "sha256:old"
                }
            },
            "error": null
        })
        .to_string();
        let parsed = parse_envelope(&line).unwrap();
        // Epoch has two changed files, but required scope only asks for b.ts which is not in bundle
        // — should still report missing for b.ts only, not a.ts mismatch if a.ts excluded.
        let epoch = epoch_with_hashes(&[("src/a.ts", "sha256:new"), ("src/b.ts", "sha256:bbb")]);
        let required = vec!["src/b.ts".to_string()];
        let issues = parsed.verify_hashes_against_epoch(&epoch, Some(&required));
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].code, "missing_required_content_hash");
        assert!(issues[0].detail.contains("src/b.ts"));

        // If required scope is only a.ts, we get mismatch for a.ts.
        let required_a = vec!["src/a.ts".to_string()];
        let issues_a = parsed.verify_hashes_against_epoch(&epoch, Some(&required_a));
        assert_eq!(issues_a.len(), 1);
        assert_eq!(issues_a[0].code, "hash_mismatch");
    }

    #[test]
    fn freshness_stale_never_verifies_exact() {
        let line = serde_json::json!({
            "protocolVersion": 1,
            "requestId": "r1",
            "ok": true,
            "generation": null,
            "result": {
                "schemaVersion": 1,
                "kind": "findings.get",
                "root": "/repo",
                "generationId": "gen-1",
                "freshness": "stale",
                "findings": [],
                "omissions": [{"code": "stale_generation", "detail": "dirty"}],
                "perFileContentHashes": {
                    "src/a.ts": "sha256:aaa"
                }
            },
            "error": null
        })
        .to_string();
        let parsed = parse_envelope(&line).unwrap();
        assert!(!parsed.freshness_is_current());
        let epoch = epoch_with_hashes(&[("src/a.ts", "sha256:aaa")]);
        assert!(!parsed.is_exact_identity_verified_against(&epoch, None));
        assert!(!parsed.may_produce_exact_lane(&epoch, None));
    }

    #[test]
    fn coverage_affecting_classification() {
        assert!(is_coverage_affecting_omission("resolution_ambiguous"));
        assert!(is_coverage_affecting_omission("parse_failed"));
        assert!(is_coverage_affecting_omission("unsupported_language"));
        assert!(is_coverage_affecting_omission("outside_scanned_set"));
        assert!(is_coverage_affecting_omission("open_export_surface"));
        assert!(is_coverage_affecting_omission("stale_generation"));
        assert!(is_coverage_affecting_omission("hash_mismatch"));
        assert!(is_coverage_affecting_omission(
            "missing_required_content_hash"
        ));
        // Advisory: package_specifier is NOT in the minimal coverage-affecting set.
        // (It indicates a bare specifier outside repo scope, not a gap in required scope.)
        assert!(!is_coverage_affecting_omission("package_specifier"));
        // Unknown custom code is not coverage-affecting by default.
        assert!(!is_coverage_affecting_omission("baseline_unknown"));
    }

    #[test]
    fn coverage_affecting_omissions_block_exact_lane() {
        let line = serde_json::json!({
            "protocolVersion": 1,
            "requestId": "r1",
            "ok": true,
            "generation": null,
            "result": {
                "schemaVersion": 1,
                "kind": "findings.get",
                "root": "/repo",
                "generationId": "gen-1",
                "freshness": "current",
                "findings": [],
                "omissions": [{"code": "resolution_ambiguous", "detail": "two matches"}],
                "perFileContentHashes": {
                    "src/a.ts": "sha256:aaa"
                }
            },
            "error": null
        })
        .to_string();
        let parsed = parse_envelope(&line).unwrap();
        assert!(parsed.has_coverage_affecting_omissions());
        assert_eq!(parsed.coverage_affecting_omissions().len(), 1);
        assert_eq!(
            parsed.coverage_affecting_omissions()[0].code,
            "resolution_ambiguous"
        );
        let epoch = epoch_with_hashes(&[("src/a.ts", "sha256:aaa")]);
        assert!(!parsed.may_produce_exact_lane(&epoch, None));
        // But a non-coverage omission does NOT block.
        let line2 = serde_json::json!({
            "protocolVersion": 1,
            "requestId": "r1",
            "ok": true,
            "generation": null,
            "result": {
                "schemaVersion": 1,
                "kind": "findings.get",
                "root": "/repo",
                "generationId": "gen-1",
                "freshness": "current",
                "findings": [],
                "omissions": [{"code": "package_specifier", "detail": "react"}],
                "perFileContentHashes": {
                    "src/a.ts": "sha256:aaa"
                }
            },
            "error": null
        })
        .to_string();
        let parsed2 = parse_envelope(&line2).unwrap();
        assert!(!parsed2.has_coverage_affecting_omissions());
        assert!(parsed2.may_produce_exact_lane(&epoch, None));
    }

    #[test]
    fn advisory_omission_does_not_invalidate_unrelated_capability() {
        // Demonstrates: unaffected optional/advisory omission does not invalidate
        // unrelated exact capability. A package_specifier advisory should not
        // imply syntax or module-resolution is incomplete if those obligations
        // are otherwise satisfied by other evidence. This unit only proves the
        // classification; integration proves it does not surface as Partial for
        // unrelated capability.
        let line = serde_json::json!({
            "protocolVersion": 1,
            "requestId": "r1",
            "ok": true,
            "generation": null,
            "result": {
                "schemaVersion": 1,
                "kind": "findings.get",
                "root": "/repo",
                "generationId": "gen-1",
                "freshness": "current",
                "findings": [],
                "omissions": [{"code": "package_specifier", "detail": "node:fs"}],
                "perFileContentHashes": {
                    "src/a.ts": "sha256:aaa"
                }
            },
            "error": null
        })
        .to_string();
        let parsed = parse_envelope(&line).unwrap();
        assert!(!is_coverage_affecting_omission(&parsed.omissions[0].code));
        let epoch = epoch_with_hashes(&[("src/a.ts", "sha256:aaa")]);
        assert!(parsed.is_exact_identity_verified_against(&epoch, None));
    }

    #[test]
    fn each_coverage_affecting_code_blocks_exact_lane_regression() {
        // Regression: every listed coverage-affecting code must prevent Complete exact.
        for code in COVERAGE_AFFECTING_OMISSION_CODES {
            let line = serde_json::json!({
                "protocolVersion": 1,
                "requestId": "r1",
                "ok": true,
                "generation": null,
                "result": {
                    "schemaVersion": 1,
                    "kind": "findings.get",
                    "root": "/repo",
                    "generationId": "gen-1",
                    "freshness": "current",
                    "findings": [],
                    "omissions": [{"code": code, "detail": "test"}],
                    "perFileContentHashes": {
                        "src/a.ts": "sha256:aaa"
                    }
                },
                "error": null
            })
            .to_string();
            let parsed = parse_envelope(&line).unwrap();
            let epoch = epoch_with_hashes(&[("src/a.ts", "sha256:aaa")]);
            assert!(
                !parsed.may_produce_exact_lane(&epoch, None),
                "code {} should block exact lane",
                code
            );
        }
    }
}
