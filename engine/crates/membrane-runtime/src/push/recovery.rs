//! Transient Push originals. Never Cortex knowledge. Publication is one SQLite
//! transaction; a digest is not authorization and reads never renew a lease.
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const MAX_ARTIFACT_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_RESTORE_BYTES: usize = 256 * 1024;
pub const DEFAULT_RESTORE_BYTES: usize = 64 * 1024;
const MAX_STORE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_SCOPE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_ARTIFACTS: u64 = 4096;
const MAX_TTL_MS: u64 = 7 * 24 * 60 * 60 * 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RecoveryError {
    #[error("push_invalid_anchor")] InvalidAnchor,
    #[error("push_scope_denied")] Denied,
    #[error("push_artifact_not_found")] NotFound,
    #[error("push_artifact_expired")] Expired,
    #[error("push_artifact_invalidated")] Invalidated,
    #[error("push_artifact_corrupt")] Corrupt,
    #[error("push_store_unavailable")] Unavailable,
    #[error("push_resource_limit")] Limit,
    #[error("push_invalid_selector")] InvalidSelector,
    #[error("push_selector_miss")] SelectorMiss,
    #[error("push_cancelled")] Cancelled,
}
impl RecoveryError {
    pub fn status(self) -> u16 {
        match self {
            Self::Denied => 403, Self::NotFound | Self::SelectorMiss => 404,
            Self::Expired | Self::Invalidated => 410, Self::Limit => 413,
            Self::Unavailable => 503, Self::Corrupt => 409, _ => 400,
        }
    }
}
fn db_error(_: rusqlite::Error) -> RecoveryError { RecoveryError::Unavailable }
pub fn digest(bytes: &[u8]) -> String { hex::encode(Sha256::digest(bytes)) }
pub fn now_ms() -> u64 { crate::time::now_millis().min(u64::MAX as u128) as u64 }

/// Construct only after the transport's normal repository authorization gate.
/// Canonical root and session are both part of the storage namespace.
#[derive(Debug, Clone)]
pub struct RecoveryScope { id: String }
impl RecoveryScope {
    pub fn new(root: &Path, session: &str) -> Result<Self, RecoveryError> {
        if session.is_empty() || session.len() > 256 { return Err(RecoveryError::Denied); }
        let root = root.canonicalize().map_err(|_| RecoveryError::Denied)?;
        if !root.is_dir() { return Err(RecoveryError::Denied); }
        let identity = serde_json::to_vec(&(root.to_string_lossy(), session))
            .map_err(|_| RecoveryError::Denied)?;
        Ok(Self { id: digest(&identity) })
    }
    pub fn binding(&self) -> &str { &self.id }
    pub fn local() -> Result<Self, RecoveryError> {
        let session = std::env::var("MEMBRANE_PUSH_SESSION").unwrap_or_else(|_| "local".into());
        Self::new(&workspace_root(), &session)
    }
}
pub fn workspace_root() -> PathBuf {
    if let Some(root) = std::env::var_os("MEMBRANE_REPO_ROOT").or_else(|| std::env::var_os("WORKSPACE_ROOT")) {
        return root.into();
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    cwd.ancestors().find(|p| p.join(".git").exists()).unwrap_or(&cwd).to_path_buf()
}
pub fn default_directory() -> PathBuf {
    std::env::var_os("MEMBRANE_ANCHOR_DIR").map(PathBuf::from)
        .unwrap_or_else(|| workspace_root().join("tools/.cache/runc"))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecoveryReference {
    pub schema_version: u32,
    pub handle: String,
    pub source_digest: String,
    pub size_bytes: usize,
    pub store_id: String,
    pub expires_at: u64,
    pub observed_at: u64,
    pub lease_state: String,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Selector {
    #[default] Whole,
    /// Zero-based, half-open original byte range.
    Bytes { start: usize, end: usize },
    /// One-based inclusive lines; original terminators remain in the result.
    Lines { start: usize, end: usize },
    /// Exact source JSON value bytes, including quotes for string values.
    Json { path: Vec<JsonStep> },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum JsonStep {
    Field { name: String },
    Index { index: usize },
    /// String-key equality only; duplicate matches are an error, never first-win.
    Key { field: String, value: String },
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedArtifact {
    pub reference: RecoveryReference,
    pub start_byte: usize,
    pub end_byte: usize,
    pub selected_digest: String,
    pub content_encoding: &'static str,
    pub content: String,
    pub disposition: &'static str,
    pub fidelity: &'static str,
}
impl ResolvedArtifact {
    pub fn bytes(&self) -> Result<Vec<u8>, RecoveryError> {
        if self.content_encoding == "utf-8" { Ok(self.content.as_bytes().to_vec()) }
        else { hex::decode(&self.content).map_err(|_| RecoveryError::Corrupt) }
    }
}

pub struct RecoveryStore { directory: PathBuf }
impl RecoveryStore {
    pub fn at(directory: impl Into<PathBuf>) -> Self { Self { directory: directory.into() } }
    pub fn configured() -> Self { Self::at(default_directory()) }
    fn connection(&self) -> Result<Connection, RecoveryError> {
        std::fs::create_dir_all(&self.directory).map_err(|_| RecoveryError::Unavailable)?;
        #[cfg(unix)] {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&self.directory, std::fs::Permissions::from_mode(0o700))
                .map_err(|_| RecoveryError::Unavailable)?;
        }
        let path = self.directory.join("push-artifacts.sqlite");
        if std::fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(RecoveryError::Denied);
        }
        let connection = Connection::open(path).map_err(db_error)?;
        connection.busy_timeout(Duration::from_millis(250)).map_err(db_error)?;
        connection.execute_batch("PRAGMA synchronous=FULL;
            CREATE TABLE IF NOT EXISTS push_store (id INTEGER PRIMARY KEY CHECK(id=1), identity TEXT NOT NULL);
            INSERT OR IGNORE INTO push_store VALUES(1, lower(hex(randomblob(32))));
            CREATE TABLE IF NOT EXISTS push_originals (
              scope TEXT NOT NULL, digest TEXT NOT NULL, content BLOB NOT NULL,
              size INTEGER NOT NULL, created INTEGER NOT NULL, expires INTEGER NOT NULL,
              invalidated INTEGER NOT NULL DEFAULT 0, PRIMARY KEY(scope,digest));
            CREATE INDEX IF NOT EXISTS push_expiry ON push_originals(expires);")
            .map_err(db_error)?;
        Ok(connection)
    }
    pub fn identity(&self) -> Result<String, RecoveryError> {
        self.connection()?.query_row("SELECT identity FROM push_store WHERE id=1", [], |r| r.get(0)).map_err(db_error)
    }
    fn reference(connection: &Connection, hash: &str, size: usize, expires: u64, now: u64) -> Result<RecoveryReference, RecoveryError> {
        let store_id = connection.query_row("SELECT identity FROM push_store WHERE id=1", [], |r| r.get(0)).map_err(db_error)?;
        Ok(RecoveryReference { schema_version: 1, handle: format!("mr://anchor/{hash}"),
            source_digest: format!("sha256:{hash}"), size_bytes: size, store_id,
            expires_at: expires, observed_at: now,
            lease_state: if expires.saturating_sub(now) <= 60_000 { "near_expiry" } else { "active" }.into() })
    }
    pub fn publish(&self, scope: &RecoveryScope, bytes: &[u8], ttl_ms: u64, now: u64) -> Result<RecoveryReference, RecoveryError> {
        if bytes.len() > MAX_ARTIFACT_BYTES || ttl_ms == 0 || ttl_ms > MAX_TTL_MS || now > i64::MAX as u64 - ttl_ms {
            return Err(RecoveryError::Limit);
        }
        let hash = digest(bytes);
        let mut connection = self.connection()?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(db_error)?;
        // Expiry frees logical quota, not a live promise. No access extends expiry.
        // Expired/invalidation tombstones prevent an old handle silently becoming
        // valid again. Explicit retention maintenance is separate from publication.
        let old_size: Option<(usize, usize, u64)> = tx.query_row(
            "SELECT size,length(content),expires FROM push_originals WHERE scope=?1 AND digest=?2",
            params![scope.id, hash], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(db_error)?;
        if let Some((size, actual, expires)) = old_size {
            if now >= expires { return Err(RecoveryError::Expired); }
            if size != actual || size > MAX_ARTIFACT_BYTES { return Err(RecoveryError::Corrupt); }
        }
        let existing: Option<(Vec<u8>, usize, u64, bool)> = tx.query_row(
            "SELECT content,size,expires,invalidated FROM push_originals WHERE scope=?1 AND digest=?2",
            params![scope.id, hash], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).optional().map_err(db_error)?;
        let expires = if let Some((retained, size, expires, invalidated)) = existing {
            if invalidated { return Err(RecoveryError::Invalidated); }
            if retained.len() != size || retained != bytes || digest(&retained) != hash { return Err(RecoveryError::Corrupt); }
            expires
        } else {
            let (total, scoped, count): (u64, u64, u64) = tx.query_row(
                "SELECT COALESCE(SUM(size),0), COALESCE(SUM(CASE WHEN scope=?1 THEN size ELSE 0 END),0), COUNT(*) FROM push_originals",
                [&scope.id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).map_err(db_error)?;
            if total + bytes.len() as u64 > MAX_STORE_BYTES || scoped + bytes.len() as u64 > MAX_SCOPE_BYTES || count >= MAX_ARTIFACTS {
                return Err(RecoveryError::Limit);
            }
            let expires = now + ttl_ms;
            tx.execute("INSERT INTO push_originals(scope,digest,content,size,created,expires) VALUES(?1,?2,?3,?4,?5,?6)",
                params![scope.id, hash, bytes, bytes.len(), now, expires]).map_err(db_error)?;
            expires
        };
        // Read back inside the transaction before exposing a handle.
        let retained: Vec<u8> = tx.query_row("SELECT content FROM push_originals WHERE scope=?1 AND digest=?2", params![scope.id, hash], |r| r.get(0)).map_err(db_error)?;
        if retained != bytes || digest(&retained) != hash { return Err(RecoveryError::Corrupt); }
        let reference = Self::reference(&tx, &hash, bytes.len(), expires, now)?;
        tx.commit().map_err(db_error)?;
        Ok(reference)
    }
    pub fn resolve(&self, scope: &RecoveryScope, handle: &str, selector: &Selector, max_bytes: usize, now: u64) -> Result<ResolvedArtifact, RecoveryError> {
        let hash = crate::ledger::identifier::AnchorRef::parse(handle).map_err(|_| RecoveryError::InvalidAnchor)?.digest();
        if max_bytes == 0 || max_bytes > MAX_RESTORE_BYTES { return Err(RecoveryError::Limit); }
        let mut connection = self.connection()?;
        let tx = connection.transaction().map_err(db_error)?;
        let metadata: Option<(usize, usize, u64, bool)> = tx.query_row(
            "SELECT size,length(content),expires,invalidated FROM push_originals WHERE scope=?1 AND digest=?2",
            params![scope.id, hash], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).optional().map_err(db_error)?;
        let (size, stored_size, expires, invalidated) = metadata.ok_or(RecoveryError::NotFound)?;
        if invalidated { return Err(RecoveryError::Invalidated); }
        if now >= expires { return Err(RecoveryError::Expired); }
        if size != stored_size || size > MAX_ARTIFACT_BYTES { return Err(RecoveryError::Corrupt); }
        let bytes: Vec<u8> = tx.query_row("SELECT content FROM push_originals WHERE scope=?1 AND digest=?2", params![scope.id, hash], |r| r.get(0)).map_err(db_error)?;
        if digest(&bytes) != hash { return Err(RecoveryError::Corrupt); }
        let (start, end) = select_bytes(&bytes, selector)?;
        if end - start > max_bytes { return Err(RecoveryError::Limit); }
        let selected = &bytes[start..end];
        let (content_encoding, content) = match std::str::from_utf8(selected) {
            Ok(text) => ("utf-8", text.to_owned()), Err(_) => ("hex", hex::encode(selected)),
        };
        Ok(ResolvedArtifact { reference: Self::reference(&tx, &hash, size, expires, now)?,
            start_byte: start, end_byte: end, selected_digest: format!("sha256:{}", digest(selected)),
            content_encoding, content, disposition: "exact", fidelity: "exact_bytes" })
    }
    pub fn invalidate(&self, scope: &RecoveryScope, handle: &str) -> Result<(), RecoveryError> {
        let hash = crate::ledger::identifier::AnchorRef::parse(handle).map_err(|_| RecoveryError::InvalidAnchor)?.digest();
        let changed = self.connection()?.execute("UPDATE push_originals SET invalidated=1 WHERE scope=?1 AND digest=?2", params![scope.id, hash]).map_err(db_error)?;
        if changed == 0 { Err(RecoveryError::NotFound) } else { Ok(()) }
    }
}

pub fn select_bytes(bytes: &[u8], selector: &Selector) -> Result<(usize, usize), RecoveryError> {
    match selector {
        Selector::Whole => Ok((0, bytes.len())),
        Selector::Bytes { start, end } if start <= end && *end <= bytes.len() => Ok((*start, *end)),
        Selector::Bytes { .. } => Err(RecoveryError::SelectorMiss),
        Selector::Lines { start, end } => {
            if *start == 0 || end < start { return Err(RecoveryError::InvalidSelector); }
            let mut line = 1;
            let mut begin = None;
            for (offset, chunk) in byte_lines(bytes) {
                if line == *start { begin = Some(offset); }
                if line == *end { return Ok((begin.ok_or(RecoveryError::SelectorMiss)?, offset + chunk.len())); }
                line += 1;
            }
            Err(RecoveryError::SelectorMiss)
        }
        Selector::Json { path } => {
            if path.len() > 64 { return Err(RecoveryError::Limit); }
            let tree = JsonNode::parse(bytes)?;
            let mut node = &tree;
            for step in path {
                node = match (step, &node.children) {
                    (JsonStep::Field { name }, JsonChildren::Object(fields)) => fields.iter().find(|(key, _)| key == name).map(|(_, v)| v),
                    (JsonStep::Index { index }, JsonChildren::Array(values)) => values.get(*index),
                    (JsonStep::Key { field, value }, JsonChildren::Array(values)) => {
                        let matches = values.iter().filter(|n| match &n.children {
                            JsonChildren::Object(fields) => fields.iter().any(|(key, n)| key == field && serde_json::from_slice::<String>(&bytes[n.start..n.end]).is_ok_and(|s| &s == value)),
                            _ => false,
                        }).collect::<Vec<_>>();
                        if matches.len() > 1 { return Err(RecoveryError::InvalidSelector); }
                        matches.first().copied()
                    }
                    _ => return Err(RecoveryError::InvalidSelector),
                }.ok_or(RecoveryError::SelectorMiss)?;
            }
            Ok((node.start, node.end))
        }
    }
}
pub fn byte_lines(bytes: &[u8]) -> impl Iterator<Item = (usize, &[u8])> {
    let mut offset = 0;
    bytes.split_inclusive(|b| *b == b'\n').map(move |line| { let at = offset; offset += line.len(); (at, line) })
}

// A bounded lossless JSON span index. serde validates scalar syntax; this parser
// additionally rejects duplicate keys instead of laundering them through Value.
struct JsonNode { start: usize, end: usize, children: JsonChildren }
enum JsonChildren { Scalar, Object(Vec<(String, JsonNode)>), Array(Vec<JsonNode>) }
impl JsonNode {
    fn parse(bytes: &[u8]) -> Result<Self, RecoveryError> {
        if bytes.len() > MAX_ARTIFACT_BYTES { return Err(RecoveryError::Limit); }
        let mut cursor = 0;
        let mut nodes = 0;
        let node = Self::value(bytes, &mut cursor, 0, &mut nodes)?;
        Self::ws(bytes, &mut cursor);
        if cursor != bytes.len() { return Err(RecoveryError::InvalidSelector); }
        Ok(node)
    }
    fn ws(bytes: &[u8], at: &mut usize) { while bytes.get(*at).is_some_and(|b| matches!(b, b' ' | b'\t' | b'\r' | b'\n')) { *at += 1; } }
    fn string_end(bytes: &[u8], at: &mut usize) -> Result<(), RecoveryError> {
        if bytes.get(*at) != Some(&b'"') { return Err(RecoveryError::InvalidSelector); }
        *at += 1;
        loop {
            match bytes.get(*at) {
                Some(b'"') => { *at += 1; return Ok(()); }
                Some(b'\\') => { *at += 2; }
                Some(_) => *at += 1,
                None => return Err(RecoveryError::InvalidSelector),
            }
        }
    }
    fn value(bytes: &[u8], at: &mut usize, depth: usize, nodes: &mut usize) -> Result<Self, RecoveryError> {
        *nodes += 1;
        if depth > 64 || *nodes > 100_000 { return Err(RecoveryError::Limit); }
        Self::ws(bytes, at);
        let start = *at;
        let children = match bytes.get(*at) {
            Some(b'{') => {
                *at += 1;
                Self::ws(bytes, at);
                let mut fields = Vec::new();
                let mut keys = std::collections::HashSet::new();
                if bytes.get(*at) != Some(&b'}') { loop {
                    Self::ws(bytes, at);
                    let key_start = *at;
                    Self::string_end(bytes, at)?;
                    let key: String = serde_json::from_slice(&bytes[key_start..*at]).map_err(|_| RecoveryError::InvalidSelector)?;
                    if !keys.insert(key.clone()) { return Err(RecoveryError::InvalidSelector); }
                    Self::ws(bytes, at);
                    if bytes.get(*at) != Some(&b':') { return Err(RecoveryError::InvalidSelector); }
                    *at += 1;
                    fields.push((key, Self::value(bytes, at, depth + 1, nodes)?));
                    Self::ws(bytes, at);
                    match bytes.get(*at) { Some(b',') => *at += 1, Some(b'}') => break, _ => return Err(RecoveryError::InvalidSelector) }
                } }
                *at += 1;
                JsonChildren::Object(fields)
            }
            Some(b'[') => {
                *at += 1;
                Self::ws(bytes, at);
                let mut values = Vec::new();
                if bytes.get(*at) != Some(&b']') { loop {
                    values.push(Self::value(bytes, at, depth + 1, nodes)?);
                    Self::ws(bytes, at);
                    match bytes.get(*at) { Some(b',') => *at += 1, Some(b']') => break, _ => return Err(RecoveryError::InvalidSelector) }
                } }
                *at += 1;
                JsonChildren::Array(values)
            }
            Some(b'"') => { Self::string_end(bytes, at)?; JsonChildren::Scalar }
            Some(_) => {
                while bytes.get(*at).is_some_and(|b| !matches!(b, b',' | b'}' | b']' | b' ' | b'\n' | b'\r' | b'\t')) { *at += 1; }
                JsonChildren::Scalar
            }
            None => return Err(RecoveryError::InvalidSelector),
        };
        // Validate leaf syntax without converting/reformatting original numbers.
        if matches!(children, JsonChildren::Scalar) {
            serde_json::from_slice::<serde_json::Value>(&bytes[start..*at]).map_err(|_| RecoveryError::InvalidSelector)?;
        }
        Ok(Self { start, end: *at, children })
    }
}

/// Formatting-only JSON codec. Strings and number lexemes are unchanged.
pub fn minify_json(text: &str) -> Option<String> {
    JsonNode::parse(text.as_bytes()).ok()?;
    let mut quoted = false;
    let mut escaped = false;
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        if quoted {
            out.push(ch);
            if escaped { escaped = false; }
            else if ch == '\\' { escaped = true; }
            else if ch == '"' { quoted = false; }
        } else if ch == '"' { quoted = true; out.push(ch); }
        else if !matches!(ch, ' ' | '\t' | '\r' | '\n') { out.push(ch); }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn scope(temp: &tempfile::TempDir, session: &str) -> RecoveryScope { RecoveryScope::new(temp.path(), session).unwrap() }
    #[test]
    fn scoped_publish_restore_restart_and_no_silent_renewal() {
        let temp = tempfile::tempdir().unwrap();
        let store = RecoveryStore::at(temp.path());
        let s = scope(&temp, "a");
        let raw = b"first\r\nsecond\r\n";
        let reference = store.publish(&s, raw, 1000, 100).unwrap();
        assert_eq!(store.publish(&s, raw, 2000, 200).unwrap().expires_at, 1100);
        let restarted = RecoveryStore::at(temp.path());
        let result = restarted.resolve(&s, &reference.handle, &Selector::Lines { start: 2, end: 2 }, 100, 300).unwrap();
        assert_eq!(result.bytes().unwrap(), b"second\r\n");
        assert_eq!(result.disposition, "exact");
        assert!(matches!(restarted.resolve(&scope(&temp, "b"), &reference.handle, &Selector::Whole, 100, 300), Err(RecoveryError::NotFound)));
        assert!(matches!(restarted.resolve(&s, &reference.handle, &Selector::Whole, 100, 1100), Err(RecoveryError::Expired)));
    }
    #[test]
    fn binary_empty_bounds_invalidation_and_corruption() {
        let temp = tempfile::tempdir().unwrap();
        let store = RecoveryStore::at(temp.path());
        let s = scope(&temp, "a");
        let reference = store.publish(&s, &[0, 255, 1], 1000, 1).unwrap();
        assert_eq!(store.resolve(&s, &reference.handle, &Selector::Whole, 3, 2).unwrap().bytes().unwrap(), [0,255,1]);
        assert!(matches!(store.resolve(&s, &reference.handle, &Selector::Whole, 2, 2), Err(RecoveryError::Limit)));
        store.connection().unwrap().execute("UPDATE push_originals SET content=x'000001'", []).unwrap();
        assert!(matches!(store.resolve(&s, &reference.handle, &Selector::Whole, 3, 2), Err(RecoveryError::Corrupt)));
        assert!(matches!(store.publish(&s, &[0,255,1], 1000, 2), Err(RecoveryError::Corrupt)));
        let empty = store.publish(&s, b"", 1000, 2).unwrap();
        assert_eq!(store.resolve(&s, &empty.handle, &Selector::Whole, 10, 3).unwrap().content, "");
        store.invalidate(&s, &empty.handle).unwrap();
        assert!(matches!(store.resolve(&s, &empty.handle, &Selector::Whole, 10, 3), Err(RecoveryError::Invalidated)));
    }
    #[test]
    fn exact_json_selectors_preserve_spelling_and_reject_ambiguity() {
        let text = br#"{ "items": [{"id":"a","n":1.00},{"id":"b","n":2e3}] }"#;
        let selector = Selector::Json { path: vec![JsonStep::Field { name:"items".into() }, JsonStep::Key { field:"id".into(), value:"b".into() }, JsonStep::Field { name:"n".into() }] };
        let (a,b) = select_bytes(text, &selector).unwrap();
        assert_eq!(&text[a..b], b"2e3");
        assert!(select_bytes(br#"{"a":1,"a":2}"#, &Selector::Json { path:vec![] }).is_err());
        assert!(select_bytes(br#"[1,]"#, &Selector::Json { path:vec![] }).is_err());
        let duplicate = br#"[{"id":"a"},{"id":"a"}]"#;
        assert!(select_bytes(duplicate, &Selector::Json { path: vec![JsonStep::Key {field:"id".into(),value:"a".into()}] }).is_err());
        let (a,b) = select_bytes(text, &Selector::Json { path:vec![JsonStep::Field{name:"items".into()},JsonStep::Index{index:0}] }).unwrap();
        assert_eq!(&text[a..b], br#"{"id":"a","n":1.00}"#);
        assert_eq!(minify_json(" {\n\"a\": 1.00, \"b\": \"a b\" } ").unwrap(), "{\"a\":1.00,\"b\":\"a b\"}");
    }
}
