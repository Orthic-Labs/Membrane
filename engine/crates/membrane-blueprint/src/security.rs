//! Bounded, fail-closed security primitives for native Blueprint.
//!
//! This module deliberately has no authority to mint grants. Signature
//! verification is injected by the trusted host so repository data cannot
//! become an authorization oracle.

use crate::contracts::ScopeGrantV1;
use serde_json::Value;
use std::path::{Component, Path, PathBuf};

pub const MAX_SECURITY_INPUT_BYTES: usize = 1_048_576;
pub const MAX_SECURITY_OUTPUT_BYTES: usize = 2_097_152;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DenialReason {
    Malformed,
    RootEscape,
    GrantMismatch,
    GrantExpired,
    SignatureInvalid,
    TokenMismatch,
    InputTooLarge,
    OutputTooLarge,
}

impl DenialReason {
    pub const fn code(self) -> &'static str {
        match self {
            Self::Malformed => "malformed",
            Self::RootEscape => "root_escape",
            Self::GrantMismatch => "grant_mismatch",
            Self::GrantExpired => "grant_expired",
            Self::SignatureInvalid => "signature_invalid",
            Self::TokenMismatch => "token_mismatch",
            Self::InputTooLarge => "input_too_large",
            Self::OutputTooLarge => "output_too_large",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecurityError(pub DenialReason);

pub fn canonical_root(root: &Path) -> Result<PathBuf, SecurityError> {
    root.canonicalize().map_err(|_| SecurityError(DenialReason::RootEscape))
}

/// Resolve the nearest existing ancestor, then append only a lexical tail.
/// This follows symlinks in the existing portion while refusing `..` and
/// dangling-link guesses.
pub fn resolve_physical_path(path: &Path) -> Result<PathBuf, SecurityError> {
    reject_path_traversal(path)?;
    let normalized = path.to_string_lossy().replace('\\', "/");
    let normalized_path = Path::new(&normalized);
    let absolute = if normalized_path.is_absolute() {
        normalized_path.to_path_buf()
    } else {
        std::env::current_dir().map_err(|_| SecurityError(DenialReason::RootEscape))?.join(normalized_path)
    };
    let mut tail = Vec::new();
    let mut ancestor = absolute.clone();
    loop {
        match ancestor.try_exists() {
            Ok(true) => {
                let physical = ancestor.canonicalize().map_err(|_| SecurityError(DenialReason::RootEscape))?;
                let mut result = physical;
                for component in tail.iter().rev() { result.push(component); }
                return Ok(result);
            }
            Ok(false) => {
                if std::fs::symlink_metadata(&ancestor).is_ok() {
                    return Err(SecurityError(DenialReason::RootEscape));
                }
                let name = ancestor.file_name().ok_or(SecurityError(DenialReason::RootEscape))?;
                if name == "." || name == ".." { return Err(SecurityError(DenialReason::RootEscape)); }
                tail.push(name.to_owned());
                ancestor.pop();
                if ancestor == absolute { return Err(SecurityError(DenialReason::RootEscape)); }
            }
            Err(_) => return Err(SecurityError(DenialReason::RootEscape)),
        }
    }
}

fn reject_path_traversal(path: &Path) -> Result<(), SecurityError> {
    let normalized = path.to_string_lossy().replace('\\', "/");
    if normalized.starts_with('/') || normalized.as_bytes().get(1) == Some(&b':') {
        // Absolute paths are valid for confinement, but a leading root is not
        // a grant-relative path; callers still pass absolute filesystem paths.
    }
    if normalized.split('/').any(|part| part == "..") {
        return Err(SecurityError(DenialReason::RootEscape));
    }
    Ok(())
}

pub fn is_confined_path(root: &Path, path: &Path, allow_root: bool) -> Result<PathBuf, SecurityError> {
    let canonical_root = canonical_root(root)?;
    let target = resolve_physical_path(path)?;
    let relative = target.strip_prefix(&canonical_root).map_err(|_| SecurityError(DenialReason::RootEscape))?;
    if !allow_root && relative.as_os_str().is_empty() { return Err(SecurityError(DenialReason::RootEscape)); }
    if relative.components().any(|c| matches!(c, Component::ParentDir | Component::RootDir | Component::Prefix(_))) {
        return Err(SecurityError(DenialReason::RootEscape));
    }
    Ok(target)
}

pub trait TrustedSignatureVerifier {
    fn verify(&self, grant: &ScopeGrantV1) -> bool;
}

impl<F: Fn(&ScopeGrantV1) -> bool> TrustedSignatureVerifier for F {
    fn verify(&self, grant: &ScopeGrantV1) -> bool { self(grant) }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedGrant {
    pub task_id: String,
    pub repo_root: PathBuf,
    pub generation_id: Option<String>,
    pub paths: Vec<String>,
}

pub fn normalize_grant_path(path: &str) -> Result<String, SecurityError> {
    if path.is_empty() || path.len() > crate::model::MAX_PATH_LENGTH {
        return Err(SecurityError(DenialReason::Malformed));
    }
    let value = path.replace('\\', "/");
    if value.starts_with('/') || value.as_bytes().get(1) == Some(&b':') {
        return Err(SecurityError(DenialReason::Malformed));
    }
    let mut out = Vec::new();
    for part in value.split('/') {
        if part.is_empty() || part == "." { continue; }
        if part == ".." { return Err(SecurityError(DenialReason::RootEscape)); }
        out.push(part);
    }
    if out.is_empty() { return Err(SecurityError(DenialReason::Malformed)); }
    Ok(out.join("/"))
}

pub fn validate_scope_grant<V: TrustedSignatureVerifier + ?Sized>(
    grant: &ScopeGrantV1,
    requested_task: &str,
    requested_root: &Path,
    requested_generation: Option<&str>,
    requested_path: &str,
    now_ms: u64,
    verifier: &V,
) -> Result<ValidatedGrant, SecurityError> {
    grant.validate().map_err(|_| SecurityError(DenialReason::Malformed))?;
    if grant.task_id != requested_task || grant.generation_id.as_deref() != requested_generation {
        return Err(SecurityError(DenialReason::GrantMismatch));
    }
    let expected_root = canonical_root(requested_root)?;
    let grant_root = Path::new(&grant.repo_root).canonicalize().map_err(|_| SecurityError(DenialReason::GrantMismatch))?;
    if expected_root != grant_root { return Err(SecurityError(DenialReason::GrantMismatch)); }
    if grant.issued_ms.checked_add(grant.ttl_ms).is_none() || grant.issued_ms.saturating_add(grant.ttl_ms) <= now_ms {
        return Err(SecurityError(DenialReason::GrantExpired));
    }
    if !verifier.verify(grant) { return Err(SecurityError(DenialReason::SignatureInvalid)); }
    let mut paths = grant.paths.iter().map(|p| normalize_grant_path(p)).collect::<Result<Vec<_>, _>>()?;
    paths.sort(); paths.dedup();
    let requested = normalize_grant_path(requested_path)?;
    if !paths.iter().any(|glob| glob_matches(glob, &requested)) {
        return Err(SecurityError(DenialReason::GrantMismatch));
    }
    Ok(ValidatedGrant { task_id: grant.task_id.clone(), repo_root: expected_root, generation_id: grant.generation_id.clone(), paths })
}

fn glob_matches(pattern: &str, value: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect(); let v: Vec<char> = value.chars().collect();
    let mut dp = vec![vec![false; v.len() + 1]; p.len() + 1]; dp[0][0] = true;
    for i in 0..p.len() { for j in 0..=v.len() {
        if !dp[i][j] { continue; }
        if p[i] == '*' {
            let recursive = i + 1 < p.len() && p[i + 1] == '*';
            dp[i + 1][j] = true;
            if j < v.len() && (recursive || v[j] != '/') { dp[i][j + 1] = true; }
        }
        else if p[i] == '?' && j < v.len() && v[j] != '/' { dp[i + 1][j + 1] = true; }
        else if j < v.len() && p[i] == v[j] { dp[i + 1][j + 1] = true; }
    }}
    dp[p.len()][v.len()]
}

pub fn generate_session_token() -> Result<[u8; 32], SecurityError> {
    let mut token = [0u8; 32];
    getrandom::fill(&mut token).map_err(|_| SecurityError(DenialReason::Malformed))?;
    Ok(token)
}

pub fn verify_session_token(expected: &[u8], provided: &[u8]) -> bool {
    let mut difference = expected.len() ^ provided.len();
    let width = expected.len().max(provided.len());
    for i in 0..width { difference |= usize::from(expected.get(i).copied().unwrap_or(0) ^ provided.get(i).copied().unwrap_or(0)); }
    difference == 0
}

fn secret_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase(); ["token", "secret", "password", "passwd", "api_key", "api-key", "authorization", "cookie", "private_key", "client_email"].iter().any(|part| key.contains(part))
}
fn digest_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase(); ["revision", "fingerprint", "digest", "generation", "content_hash", "sha", "checksum", "integrity"].iter().any(|part| key == *part || key.contains(part))
}
fn replace_secret_shapes(input: &str) -> String {
    let mut out = input.to_owned();
    // Credentials embedded in connection URLs and PEM material are always
    // redacted before broader shape handling.
    for scheme in ["postgres://", "postgresql://", "mysql://", "mongodb://", "mongodb+srv://", "redis://"] {
        let mut cursor = 0;
        while let Some(found) = out[cursor..].find(scheme) {
            let start = cursor + found;
            if let Some(at) = out[start..].find('@') {
                let at = start + at;
                if let Some(colon) = out[start + scheme.len()..at].find(':') {
                    let password = start + scheme.len() + colon + 1;
                    out.replace_range(password..at, "[REDACTED]");
                    cursor = password + 10;
                    continue;
                }
            }
            cursor = start + scheme.len();
            if cursor >= out.len() { break; }
        }
    }
    for marker in ["PRIVATE KEY", "RSA PRIVATE KEY", "EC PRIVATE KEY"] {
        let begin = format!("-----BEGIN {marker}-----");
        let end = format!("-----END {marker}-----");
        while let Some(start) = out.find(&begin) {
            let finish = out[start + begin.len()..].find(&end).map(|n| start + begin.len() + n + end.len()).unwrap_or(out.len());
            out.replace_range(start..finish, "[REDACTED]");
        }
    }
    for marker in ["ghp_", "gho_", "ghu_", "ghs_", "ghr_", "github_pat_", "npm_", "AKIA", "Bearer ", "sk-", "xoxa-", "xoxb-", "xoxp-", "xoxr-"] {
        let mut start = 0; while let Some(pos) = out[start..].find(marker) { let at = start + pos; let end = out[at..].find(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | ',' | '}' | ']')).map(|n| at+n).unwrap_or(out.len()); out.replace_range(at..end, "[REDACTED]"); start = at + 10; if start >= out.len() { break; } }
    }
    // JWTs are three base64url segments and may not carry a recognizable key.
    let mut cursor = 0;
    while let Some(pos) = out[cursor..].find("eyJ") {
        let start = cursor + pos;
        let end = out[start..].find(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | ',' | '}' | ']')).map(|n| start + n).unwrap_or(out.len());
        let candidate = &out[start..end];
        let segments: Vec<_> = candidate.split('.').collect();
        if segments.len() == 3 && segments.iter().all(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')) {
            out.replace_range(start..end, "[REDACTED]");
            cursor = start + 10;
        } else { cursor = start + 3; }
        if cursor >= out.len() { break; }
    }
    let bytes = out.as_bytes(); let mut result = String::with_capacity(out.len()); let mut i = 0;
    while i < bytes.len() { let j = (i..bytes.len()).find(|&n| !bytes[n].is_ascii_alphanumeric() && !matches!(bytes[n], b'/'|b'+'|b'=' )).unwrap_or(bytes.len()); if j-i == 40 { result.push_str("[REDACTED]"); } else { result.push_str(&out[i..j]); } if j == bytes.len() { break; } result.push(bytes[j] as char); i = j + 1; }
    result
}

pub fn redact_for_egress(value: &Value) -> Result<Value, SecurityError> {
    let input = serde_json::to_vec(value).map_err(|_| SecurityError(DenialReason::Malformed))?;
    if input.len() > MAX_SECURITY_INPUT_BYTES { return Err(SecurityError(DenialReason::InputTooLarge)); }
    fn redact(value: &Value, digest: bool) -> Value {
        match value {
            Value::Array(items) => Value::Array(items.iter().map(|v| redact(v, digest)).collect()),
            Value::Object(map) => Value::Object(map.iter().map(|(k,v)| (k.clone(), if secret_key(k) { Value::String("[REDACTED]".into()) } else { redact(v, digest_key(k)) })).collect()),
            Value::String(s) => Value::String(if digest { s.clone() } else { replace_secret_shapes(s) }),
            other => other.clone(),
        }
    }
    let output = redact(value, false); let bytes = serde_json::to_vec(&output).map_err(|_| SecurityError(DenialReason::Malformed))?;
    if bytes.len() > MAX_SECURITY_OUTPUT_BYTES { return Err(SecurityError(DenialReason::OutputTooLarge)); }
    Ok(output)
}
