//! Repository/worktree identity and stable generation fingerprints.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use xxhash_rust::xxh3::xxh3_128;
use std::{fs, path::Path, process::{Command, Stdio}};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitOrigin { pub host: String, pub owner: String, pub repo: String }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryIdentity {
    pub repo_id: String,
    pub repo_root: String,
    pub origin_url: Option<String>,
    pub origin_host: Option<String>,
    pub origin_owner: Option<String>,
    pub origin_repo: Option<String>,
    pub installation_id: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IdentityOptions { pub installation_id: Option<String>, pub local_repo_id: Option<String> }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitSourceObservation {
    pub head: String,
    pub dirty: bool,
    pub status_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdentityError { RootUnavailable, RootNotDirectory }
impl std::fmt::Display for IdentityError { fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "{self:?}") } }
impl std::error::Error for IdentityError {}

pub fn normalize_git_origin(raw: &str) -> Option<GitOrigin> {
    let input = raw.trim();
    if input.is_empty() { return None; }
    let without_prefix = input.strip_prefix("git+").unwrap_or(input);
    let cleaned = without_prefix.strip_suffix(".git").unwrap_or(without_prefix);
    let (host, path) = if let Some(rest) = cleaned.split_once('@').and_then(|(_, r)| r.split_once(':')) {
        (rest.0.to_string(), rest.1.to_string())
    } else {
        let without_scheme = cleaned.split_once("://")?.1;
        let mut parts = without_scheme.splitn(2, '/');
        let authority = parts.next()?;
        let host = authority.rsplit_once('@').map(|(_, host)| host).unwrap_or(authority);
        (host.to_string(), parts.next()?.to_string())
    };
    let path = path.trim_end_matches('/').trim_end_matches(".git");
    let parts: Vec<_> = path.split('/').filter(|p| !p.is_empty()).collect();
    if parts.len() < 2 || host.is_empty() { return None; }
    Some(GitOrigin { host, owner: parts[..parts.len()-1].join("/"), repo: parts[parts.len()-1].to_string() })
}

pub fn repository_identity(root: impl AsRef<Path>) -> Result<RepositoryIdentity, IdentityError> {
    repository_identity_with_options(root, &IdentityOptions::default())
}

pub fn repository_identity_with_options(root: impl AsRef<Path>, options: &IdentityOptions) -> Result<RepositoryIdentity, IdentityError> {
    let root = fs::canonicalize(root).map_err(|_| IdentityError::RootUnavailable)?;
    if !root.is_dir() { return Err(IdentityError::RootNotDirectory); }
    let root_string = root.to_string_lossy().replace('\\', "/");
    let origin = read_origin(&root);
    let parsed = origin.as_deref().and_then(normalize_git_origin);
    let key = if let Some(o) = &parsed {
        format!("host-owner-repo\n{}/{}/{}", o.host.to_lowercase(), o.owner.to_lowercase(), o.repo.to_lowercase())
    } else {
        let local_key = options.local_repo_id.as_deref()
            .or(options.installation_id.as_deref())
            .map(str::to_owned)
            .unwrap_or_else(|| format!("local:{:032x}", xxh3_128(root_string.as_bytes())));
        format!("local\n{local_key}")
    };
    Ok(RepositoryIdentity { repo_id: format!("xxh128:{:032x}", xxh3_128(key.as_bytes())), repo_root: root_string,
        origin_url: origin, origin_host: parsed.as_ref().map(|v| v.host.clone()), origin_owner: parsed.as_ref().map(|v| v.owner.clone()), origin_repo: parsed.map(|v| v.repo), installation_id: options.installation_id.clone() })
}

/// Observe the same bounded porcelain worktree surface used by Blueprint's
/// JS authority.  Unavailable git state is `None`, never a falsely clean tree.
pub fn git_source_observation(root: impl AsRef<Path>) -> Option<GitSourceObservation> {
    let root = root.as_ref();
    let head_output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(root)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output().ok().filter(|o| o.status.success())?;
    let head = String::from_utf8_lossy(&head_output.stdout).trim().to_ascii_lowercase();
    if head.len() < 40 || head.len() > 64 || !head.bytes().all(|b| b.is_ascii_hexdigit()) { return None; }
    let output = Command::new("git")
        .args(["status", "--porcelain=v1", "-z", "--untracked-files=all", "--", ".", ":(exclude).agent", ":(exclude).agent/**", ":(exclude)docs/product.md", ":(exclude)docs/architecture.md"])
        .current_dir(root)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output().ok().filter(|o| o.status.success())?;
    Some(GitSourceObservation { head, dirty: !output.stdout.is_empty(), status_digest: format!("{:032x}", xxh3_128(&output.stdout)) })
}

pub fn content_digest(bytes: &[u8]) -> String { format!("xxh128:{:032x}", xxh3_128(bytes)) }

pub fn compute_generation_id(nodes_json: &str, edges_json: &str, source_hash: Option<&str>) -> String {
    // Callers supply JSON.stringify-equivalent body bytes: serde_json::Value
    // sorts map keys under this crate's dependency features and cannot retain
    // the insertion order that participates in the JS authority's identity.
    let source = serde_json::to_string(&source_hash).unwrap();
    let value = format!("{{\"nodes\":{nodes_json},\"edges\":{edges_json},\"sourceHash\":{source}}}");
    format!("xxh128:{:032x}", xxh3_128(value.as_bytes()))
}

/// Digest the runtime manifest identity surface. Only the JS authority's
/// whitelist participates; hub lifecycle fields and unrelated extensions do not.
pub fn compute_manifest_digest_value(manifest: &Value, source_observation: Option<&Value>) -> String {
    let observation = source_observation.or_else(|| manifest.get("sourceObservation"));
    let value = serde_json::json!({
        "schemaVersion": manifest.get("schemaVersion").cloned().unwrap_or(Value::from(1)),
        "provider": manifest.get("provider").cloned().unwrap_or(Value::Null),
        "providerComposition": manifest.get("providerComposition").cloned().unwrap_or(Value::Null),
        "counts": manifest.get("counts").cloned().unwrap_or(Value::Null),
        "repo": manifest.get("repo").cloned().unwrap_or(Value::Null),
        "sourceObservation": observation.cloned().unwrap_or(Value::Null),
    });
    let mut h = Sha256::new(); h.update(canonical_json(&value).as_bytes()); format!("sha256:{}", hex::encode(h.finalize()))
}

fn read_origin(root: &Path) -> Option<String> {
    let command_origin = Command::new("git")
        .args(["-C", &root.to_string_lossy(), "config", "--get", "remote.origin.url"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .filter(|value| !value.is_empty());
    command_origin.or_else(|| read_origin_from_git_config(root))
}

fn read_origin_from_git_config(root: &Path) -> Option<String> {
    let dot_git = root.join(".git");
    let config = if dot_git.is_dir() {
        dot_git.join("config")
    } else {
        let pointer = fs::read_to_string(&dot_git).ok()?;
        let git_dir = pointer
            .lines()
            .find_map(|line| line.trim().strip_prefix("gitdir:").map(str::trim))?;
        let git_dir = Path::new(git_dir);
        let git_dir = if git_dir.is_absolute() { git_dir.to_path_buf() } else { root.join(git_dir) };
        let local = git_dir.join("config");
        if local.is_file() {
            local
        } else {
            git_dir.parent()?.parent()?.join("config")
        }
    };
    if fs::metadata(&config).ok()?.len() > 1_048_576 { return None; }
    parse_origin_config(&fs::read_to_string(config).ok()?)
}

fn parse_origin_config(config: &str) -> Option<String> {
    let mut in_origin = false;
    for raw in config.lines() {
        let line = raw.trim();
        if line.starts_with('[') {
            in_origin = line.eq_ignore_ascii_case("[remote \"origin\"]");
            continue;
        }
        if !in_origin || line.is_empty() || line.starts_with('#') || line.starts_with(';') { continue; }
        let Some((key, value)) = line.split_once('=') else { continue; };
        if key.trim().eq_ignore_ascii_case("url") {
            let value = value.trim().trim_matches('"').trim();
            if !value.is_empty() { return Some(value.to_owned()); }
        }
    }
    None
}

fn canonical_json(value: &Value) -> String {
    match value {
        Value::Null => "null".into(),
        Value::Bool(v) => v.to_string(),
        Value::Number(v) => v.to_string(),
        Value::String(v) => serde_json::to_string(v).unwrap(),
        Value::Array(v) => format!("[{}]", v.iter().map(canonical_json).collect::<Vec<_>>().join(",")),
        Value::Object(v) => {
            // Match the JS authority's stableStringify: object keys are sorted,
            // while array order and scalar JSON representations are preserved.
            let mut keys: Vec<_> = v.keys().collect();
            keys.sort();
            format!("{{{}}}", keys.into_iter().map(|key| {
                format!("{}:{}", serde_json::to_string(key).unwrap(), canonical_json(&v[key]))
            }).collect::<Vec<_>>().join(","))
        }
    }
}
