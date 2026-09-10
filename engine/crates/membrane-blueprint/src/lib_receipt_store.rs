//! Native Rust port of `blueprint/src/lib/receipt-store.mjs`.
//!
//! Lane LIB4 (r5 closure): this module has no prior native equivalent.
//! Host-owned orientation receipt store — data, not enforcement. Receipts
//! live outside the agent-writable repository tree by default
//! (`~/.agent/receipts` or `BLUEPRINT_RECEIPT_STORE`). The store records
//! orientation decisions keyed by session / task / repo / generation; it
//! does not block tools or classify shell commands.
//!
//! HMAC-SHA256 now uses `ring::hmac` (the prior version hand-rolled RFC 2104
//! directly against `sha2::Sha256` because no HMAC crate was believed
//! available; both constructions were verified to agree byte-for-byte on
//! the RFC 4231 vectors in this module's parity test -- no bug found in the
//! prior hand-rolled version). Random bytes and the v4-shaped UUID remain
//! built directly on `getrandom` (an existing crate dependency, and the
//! correct/only tool for this job -- lane rules permit adding `ring`/`hmac`
//! only, and no `uuid` crate is available in `engine/Cargo.lock`, so the
//! UUID formatting stays hand-written; it is not a cryptographic primitive,
//! only an opaque-id shape, matching the legacy `randomUUID()` call sites
//! which likewise only rely on uniqueness/shape, not the RFC 4122
//! algorithm).

use ring::hmac;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const RECEIPT_SCHEMA_VERSION: u32 = 1;

const GRANT_KEY_NAME: &str = "grant.key";
const GRANT_DIR_NAME: &str = "grants";

// ---------------------------------------------------------------------
// small crypto/random helpers
// ---------------------------------------------------------------------

fn hmac_sha256_hex(key: &[u8], message: &[u8]) -> String {
    let signing_key = hmac::Key::new(hmac::HMAC_SHA256, key);
    hex::encode(hmac::sign(&signing_key, message).as_ref())
}

fn random_hex_32_bytes() -> String {
    let mut buf = [0u8; 32];
    getrandom::fill(&mut buf).expect("getrandom fill");
    hex::encode(buf)
}

/// A random UUID-v4-shaped string (not cryptographically distinguishable
/// from a real UUID v4; used as an opaque receipt/grant id, matching the
/// JS `randomUUID()` call sites which only rely on uniqueness/shape).
fn random_uuid_v4() -> String {
    let mut b = [0u8; 16];
    getrandom::fill(&mut b).expect("getrandom fill");
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7], b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]
    )
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn now_iso8601() -> String {
    // Minimal RFC3339/ISO8601 UTC formatter (no chrono dependency).
    let dur = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = dur.as_secs();
    let millis = dur.subsec_millis();
    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let (h, m, s) = (time_of_day / 3600, (time_of_day % 3600) / 60, time_of_day % 60);

    // civil_from_days (Howard Hinnant's algorithm), epoch = 1970-01-01.
    let z = days as i64 + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mth = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if mth <= 2 { y + 1 } else { y };

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        year, mth, d, h, m, s, millis
    )
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

fn write_atomic(path: &Path, body: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let millis = now_ms();
    let tmp_name = format!(
        "{}.{}.{}.tmp",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("file"),
        std::process::id(),
        millis
    );
    let tmp = path.with_file_name(tmp_name);
    fs::write(&tmp, body)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

// ---------------------------------------------------------------------
// scope grants
// ---------------------------------------------------------------------

fn grant_root(repo_root: &Path, out_dir: &str) -> PathBuf {
    let resolved = if repo_root.is_absolute() {
        repo_root.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(repo_root)
    };
    resolved.join(out_dir).join("graph")
}

fn grant_key_path(repo_root: &Path, out_dir: &str) -> PathBuf {
    grant_root(repo_root, out_dir).join(GRANT_KEY_NAME)
}

fn grant_dir(repo_root: &Path, out_dir: &str) -> PathBuf {
    grant_root(repo_root, out_dir).join(GRANT_DIR_NAME)
}

fn ensure_grant_key(repo_root: &Path, out_dir: &str) -> std::io::Result<String> {
    let path = grant_key_path(repo_root, out_dir);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    if !path.exists() {
        let key = random_hex_32_bytes();
        fs::write(&path, &key)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
        }
    }
    Ok(fs::read_to_string(&path)?.trim().to_string())
}

/// Fields defining a grant payload, mirroring `grantPayload` in the JS
/// module.
#[derive(Debug, Clone)]
pub struct GrantPayload {
    pub task_id: String,
    pub repo_root: String,
    pub generation_id: Option<String>,
    pub receipt_id: String,
    pub paths: Vec<String>,
    pub issued_ms: i64,
    pub ttl_ms: i64,
}

fn grant_payload_json(g: &GrantPayload) -> Value {
    json!({
        "taskId": g.task_id,
        "repoRoot": g.repo_root,
        "generationId": g.generation_id,
        "receiptId": g.receipt_id,
        "paths": g.paths,
        "issuedMs": g.issued_ms,
        "ttlMs": g.ttl_ms,
    })
}

fn sign_grant(payload: &GrantPayload, key: &[u8]) -> String {
    let body = serde_json::to_string(&grant_payload_json(payload)).unwrap();
    hmac_sha256_hex(key, body.as_bytes())
}

fn grant_file_path(repo_root: &Path, receipt_id: &str, out_dir: &str) -> PathBuf {
    grant_dir(repo_root, out_dir).join(format!("{}.json", receipt_id))
}

/// Translate a glob (`*`, `**`, `?`) to an anchored regex, mirroring
/// `globRegex`.
fn glob_regex(glob: &str) -> regex::Regex {
    let value = glob.replace('\\', "/");
    let mut pattern = String::new();
    let chars: Vec<char> = value.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '*' && chars.get(i + 1) == Some(&'*') {
            pattern.push_str(".*");
            i += 1;
        } else if c == '*' {
            pattern.push_str("[^/]*");
        } else if c == '?' {
            pattern.push_str("[^/]");
        } else if ".+^${}()|[]\\".contains(c) {
            pattern.push('\\');
            pattern.push(c);
        } else {
            pattern.push(c);
        }
        i += 1;
    }
    regex::Regex::new(&format!("^{}$", pattern)).expect("valid glob regex")
}

fn grant_matches_path(paths: &[String], path: &str) -> bool {
    let normalized = path.replace('\\', "/");
    let normalized = normalized.strip_prefix("./").unwrap_or(&normalized);
    paths.iter().any(|glob| glob_regex(glob).is_match(normalized))
}

/// Input for [`issue_scope_grant`], mirroring the JS destructured parameter
/// object.
pub struct IssueScopeGrantInput {
    pub repo_root: PathBuf,
    pub generation_id: Option<String>,
    pub task_id: String,
    pub paths: Vec<String>,
    pub ttl_minutes: f64,
    pub out_dir: String,
    pub receipt_id: Option<String>,
    pub issued_ms: Option<i64>,
}

impl Default for IssueScopeGrantInput {
    fn default() -> Self {
        Self {
            repo_root: PathBuf::new(),
            generation_id: None,
            task_id: String::new(),
            paths: Vec::new(),
            ttl_minutes: 120.0,
            out_dir: ".agent".to_string(),
            receipt_id: None,
            issued_ms: None,
        }
    }
}

#[derive(Debug)]
pub struct ReceiptStoreError(pub String);
impl std::fmt::Display for ReceiptStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl std::error::Error for ReceiptStoreError {}

pub struct IssuedGrant {
    pub grant: Value,
    pub path: PathBuf,
}

/// Issue a signed scope grant for `paths` under `repo_root`. Mirrors
/// `issueScopeGrant`.
pub fn issue_scope_grant(input: IssueScopeGrantInput) -> Result<IssuedGrant, ReceiptStoreError> {
    let mut normalized_paths: Vec<String> = input
        .paths
        .iter()
        .map(|p| {
            let p = p.replace('\\', "/");
            p.strip_prefix("./").unwrap_or(&p).to_string()
        })
        .filter(|p| !p.is_empty())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    normalized_paths.sort();

    if input.task_id.trim().is_empty() {
        return Err(ReceiptStoreError("grant taskId is required".to_string()));
    }
    if normalized_paths.is_empty() {
        return Err(ReceiptStoreError("grant paths are required".to_string()));
    }

    let ttl_ms = ((input.ttl_minutes * 60_000.0) as i64).max(1);
    let receipt_id = input.receipt_id.unwrap_or_else(random_uuid_v4);
    let issued_ms = input.issued_ms.unwrap_or_else(now_ms);
    let repo_root_abs = if input.repo_root.is_absolute() {
        input.repo_root.clone()
    } else {
        std::env::current_dir()
            .map_err(|e| ReceiptStoreError(e.to_string()))?
            .join(&input.repo_root)
    };

    let payload = GrantPayload {
        task_id: input.task_id,
        repo_root: repo_root_abs.to_string_lossy().to_string(),
        generation_id: input.generation_id,
        receipt_id: receipt_id.clone(),
        paths: normalized_paths,
        issued_ms,
        ttl_ms,
    };

    let key = ensure_grant_key(&input.repo_root, &input.out_dir)
        .map_err(|e| ReceiptStoreError(e.to_string()))?;
    let signature = sign_grant(&payload, key.as_bytes());

    let mut grant = grant_payload_json(&payload);
    grant["signature"] = json!(signature);

    let path = grant_file_path(&input.repo_root, &receipt_id, &input.out_dir);
    let body = format!("{}\n", serde_json::to_string_pretty(&grant).unwrap());
    write_atomic(&path, &body).map_err(|e| ReceiptStoreError(e.to_string()))?;

    Ok(IssuedGrant { grant, path })
}

/// Input for [`check_scope_grant`], mirroring the JS destructured parameter
/// object.
pub struct CheckScopeGrantInput<'a> {
    pub repo_root: &'a Path,
    pub generation_id: Option<&'a str>,
    pub task_id: &'a str,
    pub path: &'a str,
    pub out_dir: &'a str,
    pub now_ms: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GrantCheckResult {
    pub allowed: bool,
    pub reason: &'static str,
    pub grant: Option<Value>,
}

/// Check whether an active, non-expired, signature-valid grant permits
/// `path` for `task_id`/`generation_id`. Mirrors `checkScopeGrant`.
pub fn check_scope_grant(input: CheckScopeGrantInput) -> GrantCheckResult {
    let root = if input.repo_root.is_absolute() {
        input.repo_root.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(input.repo_root)
    };
    let key_path = grant_key_path(&root, input.out_dir);
    let key = match fs::read_to_string(&key_path) {
        Ok(k) => k.trim().to_string(),
        Err(_) => {
            return GrantCheckResult {
                allowed: false,
                reason: "grant_key_missing",
                grant: None,
            }
        }
    };
    let directory = grant_dir(&root, input.out_dir);
    let entries = match fs::read_dir(&directory) {
        Ok(e) => e,
        Err(_) => {
            return GrantCheckResult {
                allowed: false,
                reason: "grant_missing",
                grant: None,
            }
        }
    };

    let mut names: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().map(|e| e == "json").unwrap_or(false))
        .collect();
    names.sort();

    for path in names {
        let content = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let grant: Value = match serde_json::from_str(&content) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let payload = GrantPayload {
            task_id: grant.get("taskId").and_then(Value::as_str).unwrap_or("").to_string(),
            repo_root: grant.get("repoRoot").and_then(Value::as_str).unwrap_or("").to_string(),
            generation_id: grant.get("generationId").and_then(Value::as_str).map(|s| s.to_string()),
            receipt_id: grant.get("receiptId").and_then(Value::as_str).unwrap_or("").to_string(),
            paths: grant
                .get("paths")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
                .unwrap_or_default(),
            issued_ms: grant.get("issuedMs").and_then(Value::as_i64).unwrap_or(0),
            ttl_ms: grant.get("ttlMs").and_then(Value::as_i64).unwrap_or(0),
        };
        let expected = sign_grant(&payload, key.as_bytes());
        let actual = grant.get("signature").and_then(Value::as_str).unwrap_or("");
        if !constant_time_eq(actual.as_bytes(), expected.as_bytes()) {
            continue;
        }
        if payload.task_id != input.task_id {
            continue;
        }
        if let Some(gen) = input.generation_id {
            if payload.generation_id.as_deref() != Some(gen) {
                continue;
            }
        }
        if payload.issued_ms + payload.ttl_ms <= input.now_ms {
            continue;
        }
        if !grant_matches_path(&payload.paths, input.path) {
            continue;
        }
        return GrantCheckResult {
            allowed: true,
            reason: "grant_match",
            grant: Some(grant),
        };
    }

    GrantCheckResult {
        allowed: false,
        reason: "grant_miss",
        grant: None,
    }
}

// ---------------------------------------------------------------------
// receipt store
// ---------------------------------------------------------------------

/// Default receipt store directory: `$BLUEPRINT_RECEIPT_STORE`, or
/// `<home>/.agent/receipts`. Mirrors `defaultReceiptStoreDir`.
pub fn default_receipt_store_dir(home_dir: &Path) -> PathBuf {
    if let Ok(from_env) = std::env::var("BLUEPRINT_RECEIPT_STORE") {
        if !from_env.trim().is_empty() {
            let p = PathBuf::from(from_env.trim());
            return if p.is_absolute() {
                p
            } else {
                std::env::current_dir().unwrap_or_default().join(p)
            };
        }
    }
    home_dir.join(".agent").join("receipts")
}

fn receipt_path(store_dir: &Path, receipt_id: &str) -> PathBuf {
    store_dir.join(format!("{}.json", receipt_id))
}

fn index_path(store_dir: &Path) -> PathBuf {
    store_dir.join("index.json")
}

fn read_index(store_dir: &Path) -> Value {
    let path = index_path(store_dir);
    match fs::read_to_string(&path) {
        Ok(content) => serde_json::from_str(&content)
            .unwrap_or_else(|_| json!({"schemaVersion": 1, "byKey": {}, "byReceipt": {}})),
        Err(_) => json!({"schemaVersion": 1, "byKey": {}, "byReceipt": {}}),
    }
}

fn write_index_atomic(store_dir: &Path, index: &Value) -> std::io::Result<()> {
    let path = index_path(store_dir);
    let body = format!("{}\n", serde_json::to_string_pretty(index).unwrap());
    write_atomic(&path, &body)
}

fn write_receipt_atomic(store_dir: &Path, receipt: &Value) -> Result<(), ReceiptStoreError> {
    let receipt_id = receipt
        .get("receiptId")
        .and_then(Value::as_str)
        .ok_or_else(|| ReceiptStoreError("receipt.receiptId is required".to_string()))?;
    let path = receipt_path(store_dir, receipt_id);
    let body = format!("{}\n", serde_json::to_string_pretty(receipt).unwrap());
    write_atomic(&path, &body).map_err(|e| ReceiptStoreError(e.to_string()))
}

/// Lookup-key fields: session + task + repo + generation. Mirrors
/// `receiptLookupKey`.
pub struct ReceiptLookupKeyInput<'a> {
    pub session_id: Option<&'a str>,
    pub task_id: Option<&'a str>,
    pub repo_identity: Option<&'a str>,
    pub generation_id: Option<&'a str>,
}

/// Composite lookup key: session + task + repo + generation, joined by the
/// unit-separator control character (matches JS ``).
pub fn receipt_lookup_key(input: ReceiptLookupKeyInput) -> String {
    [
        input.session_id.unwrap_or(""),
        input.task_id.unwrap_or(""),
        input.repo_identity.unwrap_or(""),
        input.generation_id.unwrap_or(""),
    ]
    .join("\u{001f}")
}

fn receipt_lookup_key_from_value(receipt: &Value) -> String {
    receipt_lookup_key(ReceiptLookupKeyInput {
        session_id: receipt.get("sessionId").and_then(Value::as_str),
        task_id: receipt.get("taskId").and_then(Value::as_str),
        repo_identity: receipt.get("repoIdentity").and_then(Value::as_str),
        generation_id: receipt.get("generationId").and_then(Value::as_str),
    })
}

/// A host-owned receipt store rooted at `store_dir`. Mirrors the object
/// returned by `createReceiptStore`.
pub struct ReceiptStore {
    pub store_dir: PathBuf,
}

impl ReceiptStore {
    /// Create (and ensure) a receipt store rooted at `store_dir`. Mirrors
    /// `createReceiptStore({ storeDir })`.
    pub fn new(store_dir: PathBuf) -> std::io::Result<Self> {
        fs::create_dir_all(&store_dir)?;
        Ok(Self { store_dir })
    }

    pub fn next_id(&self) -> String {
        random_uuid_v4()
    }

    pub fn put(&self, receipt: Value) -> Result<Value, ReceiptStoreError> {
        write_receipt_atomic(&self.store_dir, &receipt)?;
        let mut index = read_index(&self.store_dir);
        let key = receipt_lookup_key_from_value(&receipt);
        let receipt_id = receipt.get("receiptId").and_then(Value::as_str).unwrap();
        index["byKey"][&key] = json!(receipt_id);
        index["byReceipt"][receipt_id] = json!({
            "key": key,
            "status": receipt.get("status").and_then(Value::as_str).unwrap_or("active"),
            "updatedAt": receipt.get("updatedAt").or_else(|| receipt.get("issuedAt")).cloned().unwrap_or(Value::Null),
        });
        write_index_atomic(&self.store_dir, &index).map_err(|e| ReceiptStoreError(e.to_string()))?;
        Ok(receipt)
    }

    pub fn get(&self, receipt_id: &str) -> Option<Value> {
        if receipt_id.is_empty() {
            return None;
        }
        let path = receipt_path(&self.store_dir, receipt_id);
        let content = fs::read_to_string(path).ok()?;
        serde_json::from_str(&content).ok()
    }

    pub fn find_active(&self, query: ReceiptLookupKeyInput) -> Option<Value> {
        let index = read_index(&self.store_dir);
        let key = receipt_lookup_key(query);
        let receipt_id = index.get("byKey").and_then(|v| v.get(&key)).and_then(Value::as_str)?;
        let receipt = self.get(receipt_id)?;
        if receipt.get("status").and_then(Value::as_str) == Some("revoked") {
            return None;
        }
        Some(receipt)
    }

    pub fn list(&self, include_revoked: bool) -> Vec<Value> {
        let mut out: Vec<Value> = Vec::new();
        let entries = match fs::read_dir(&self.store_dir) {
            Ok(e) => e,
            Err(_) => return out,
        };
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            let name = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n,
                None => continue,
            };
            if !name.ends_with(".json") || name == "index.json" {
                continue;
            }
            let content = match fs::read_to_string(&path) {
                Ok(c) => c,
                Err(_) => continue,
            };
            let receipt: Value = match serde_json::from_str(&content) {
                Ok(v) => v,
                Err(_) => continue,
            };
            if !include_revoked && receipt.get("status").and_then(Value::as_str) == Some("revoked") {
                continue;
            }
            out.push(receipt);
        }
        out.sort_by(|a, b| {
            let ia = a.get("issuedAt").and_then(Value::as_str).unwrap_or("");
            let ib = b.get("issuedAt").and_then(Value::as_str).unwrap_or("");
            ia.cmp(ib)
        });
        out
    }

    pub fn revoke(&self, receipt_id: &str, reason: &str, at: Option<&str>) -> Result<Option<Value>, ReceiptStoreError> {
        let receipt = match self.get(receipt_id) {
            Some(r) => r,
            None => return Ok(None),
        };
        if receipt.get("status").and_then(Value::as_str) == Some("revoked") {
            return Ok(Some(receipt));
        }
        let at = at.map(|s| s.to_string()).unwrap_or_else(now_iso8601);
        let mut updated = receipt;
        updated["status"] = json!("revoked");
        updated["revokedAt"] = json!(at);
        updated["revokeReason"] = json!(reason);
        updated["updatedAt"] = json!(at);
        Ok(Some(self.put(updated)?))
    }

    pub fn clear(&self) -> std::io::Result<()> {
        if !self.store_dir.exists() {
            return Ok(());
        }
        for entry in fs::read_dir(&self.store_dir)?.filter_map(|e| e.ok()) {
            let _ = fs::remove_file(entry.path());
        }
        Ok(())
    }
}

/// Fields for [`build_orientation_receipt`]. Optional fields mirror the JS
/// object's nullable/defaulted properties.
#[derive(Default)]
pub struct OrientationReceiptFields {
    pub receipt_id: Option<String>,
    pub status: Option<String>,
    pub session_id: Option<String>,
    pub task_id: Option<String>,
    pub turn_digest: Option<Value>,
    pub repo_identity: Option<String>,
    pub worktree_identity: Option<Value>,
    pub generation_id: Option<Value>,
    pub manifest_digest: Option<Value>,
    pub source_observation_digest: Option<Value>,
    pub candidate_set_digest: Option<Value>,
    pub explicit_anchors: Vec<Value>,
    pub allowed_paths: Vec<Value>,
    pub allowed_directory_scopes: Vec<Value>,
    pub allowed_operations: Option<Vec<Value>>,
    pub overlay_revision: Option<f64>,
    pub omissions: Vec<Value>,
    pub issued_at: Option<String>,
    pub updated_at: Option<String>,
}

/// Build a durable orientation receipt record (not yet persisted). Mirrors
/// `buildOrientationReceipt`.
pub fn build_orientation_receipt(fields: OrientationReceiptFields) -> Value {
    let issued_at = fields.issued_at.clone().unwrap_or_else(now_iso8601);
    let allowed_operations = fields.allowed_operations.unwrap_or_else(|| {
        vec![json!("read"), json!("search"), json!("test"), json!("edit")]
    });
    json!({
        "schemaVersion": RECEIPT_SCHEMA_VERSION,
        "kind": "blueprint_orientation_receipt",
        "receiptId": fields.receipt_id.unwrap_or_else(random_uuid_v4),
        "status": fields.status.unwrap_or_else(|| "active".to_string()),
        "sessionId": fields.session_id.unwrap_or_default(),
        "taskId": fields.task_id.unwrap_or_default(),
        "turnDigest": fields.turn_digest.unwrap_or(Value::Null),
        "repoIdentity": fields.repo_identity.unwrap_or_default(),
        "worktreeIdentity": fields.worktree_identity.unwrap_or(Value::Null),
        "generationId": fields.generation_id.unwrap_or(Value::Null),
        "manifestDigest": fields.manifest_digest.unwrap_or(Value::Null),
        "sourceObservationDigest": fields.source_observation_digest.unwrap_or(Value::Null),
        "candidateSetDigest": fields.candidate_set_digest.unwrap_or(Value::Null),
        "explicitAnchors": fields.explicit_anchors,
        "allowedPaths": fields.allowed_paths,
        "allowedDirectoryScopes": fields.allowed_directory_scopes,
        "allowedOperations": allowed_operations,
        "overlayRevision": fields.overlay_revision.unwrap_or(0.0),
        "omissions": fields.omissions,
        "issuedAt": issued_at,
        "updatedAt": fields.updated_at.unwrap_or(issued_at),
    })
}

#[cfg(test)]
mod crypto_tests {
    use super::hmac_sha256_hex;

    #[test]
    fn hmac_sha256_hex_matches_rfc4231_test_case_1() {
        // RFC 4231 Test Case 1, cross-checked against the prior hand-rolled
        // RFC 2104 construction this module used before `ring::hmac` was
        // available as a dependency; the two agreed byte-for-byte (no bug
        // found in the prior version).
        let key = [0x0bu8; 20];
        assert_eq!(
            hmac_sha256_hex(&key, b"Hi There"),
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
        );
    }

    #[test]
    fn hmac_sha256_hex_matches_rfc4231_test_case_2() {
        assert_eq!(
            hmac_sha256_hex(b"Jefe", b"what do ya want for nothing?"),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }
}
