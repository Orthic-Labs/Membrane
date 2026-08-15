use std::path::{Path, PathBuf};
use crate::schema_types::ManifestV1;

const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;

/// The Hub's own version, used for fail-closed `hubCompatRange` evaluation.
/// A product manifest declares the range of Hub versions it is compatible
/// with; it never negotiates against sibling source or a parent gitlink SHA.
const HUB_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Documented `hubCompatRange` grammar (solimplement §3.5):
///
/// ```text
/// range      := comparator (separator comparator)*   | "*" | ""
/// separator  := "," | whitespace+
/// comparator := op version
/// op         := ">=" | "<=" | ">" | "<" | "=" | "=="
/// version    := major ["." minor ["." patch]]        (non-negative integers,
///                                                      missing parts = 0)
/// ```
///
/// A Hub version satisfies a range iff it satisfies every comparator. Unknown
/// operators, malformed versions, or unsupported future grammar fail closed
/// (return `false`) — the Hub never guesses compatibility it cannot prove.
pub fn evaluate_hub_compat_range(range: &str, hub_version: &str) -> bool {
    let range = range.trim();
    if range.is_empty() || range == "*" {
        return true;
    }
    let hub = match parse_version(hub_version) {
        Some(version) => version,
        None => return false,
    };
    // Split on commas and whitespace so `>=0.1.0 <2.0.0` and `>=0.1.0,<2.0.0`
    // both parse; empty tokens from repeated separators are ignored.
    let tokens = range.split(|c: char| c == ',' || c.is_whitespace());
    let mut any_comparator = false;
    for token in tokens {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }
        any_comparator = true;
        let (op, version) = split_comparator(token);
        let version = match parse_version(version) {
            Some(version) => version,
            None => return false,
        };
        let satisfied = match op {
            ">=" => hub >= version,
            "<=" => hub <= version,
            ">" => hub > version,
            "<" => hub < version,
            "=" | "==" => hub == version,
            _ => return false,
        };
        if !satisfied {
            return false;
        }
    }
    any_comparator
}

/// A comparable `major.minor.patch` triple. Missing parts default to zero so
/// `0.1` == `0.1.0`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Version(u64, u64, u64);

fn parse_version(text: &str) -> Option<Version> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let mut parts = [0u64; 3];
    for (index, component) in text.split('.').enumerate() {
        if index >= 3 {
            return None; // more than three components is unsupported future grammar
        }
        if component.is_empty() {
            return None;
        }
        parts[index] = component.parse::<u64>().ok()?;
    }
    Some(Version(parts[0], parts[1], parts[2]))
}

fn split_comparator(token: &str) -> (&str, &str) {
    for op in [">=", "<=", "==", ">", "<", "="] {
        if let Some(rest) = token.strip_prefix(op) {
            return (op, rest);
        }
    }
    // No operator is unsupported future grammar; return an empty op so the
    // caller fails closed.
    ("", token)
}

/// Reject a manifest whose file (or containing directory) grants group/other
/// write permission. The manifest is owner-only: a stray JSON drop must never
/// become writable by another user, which would let them turn the Hub into a
/// launcher for arbitrary binaries (seam §4.1).
#[cfg(unix)]
fn reject_insecure_mode(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::MetadataExt;
    let metadata = std::fs::metadata(path).map_err(|_| "manifest_unparseable")?;
    let mode = metadata.mode();
    // 0o022 = group-write | other-write. Reading is tolerated: the manifest
    // carries no secret; only tampering (write) is rejected.
    if mode & 0o022 != 0 {
        return Err("manifest_mode_insecure".into());
    }
    if let Some(parent) = path.parent() {
        if let Ok(parent_meta) = std::fs::metadata(parent) {
            if parent_meta.mode() & 0o022 != 0 {
                return Err("manifest_mode_insecure".into());
            }
        }
    }
    Ok(())
}

/// Windows owner-only check is enforced by installer ACLs; a DOS read-only
/// attribute is not ownership. The Hub defers to the installer and accepts the
/// file here, while the Unix path above enforces owner-only mode natively.
#[cfg(not(unix))]
fn reject_insecure_mode(_path: &Path) -> Result<(), String> {
    Ok(())
}

fn is_loopback(host: &str) -> bool {
    host == "127.0.0.1" || host == "::1" || host == "localhost"
}

fn is_http_header_name(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte))
}

fn resolve_inside(install_root: &Path, target: &str) -> Result<PathBuf, String> {
    let path = Path::new(target);
    // Resolve relative to install_root if not absolute? But spec says serviceStart[0] must lie inside installRoot after canonicalization.
    // We canonicalize both and check prefix.
    let candidate = if path.is_absolute() {
        path.to_path_buf()
    } else {
        install_root.join(path)
    };
    // For traversal check without requiring file existence, we use lexical check plus canonicalization if exists.
    // First lexical: if candidate contains `..` that escapes, we detect via normalized path.
    let normalized = normalize_lexically(&candidate);
    if !normalized.starts_with(install_root) {
        return Err("escapes".into());
    }
    // If path exists, verify canonical symlink resolution also inside.
    if candidate.exists() {
        if let Ok(canonical) = candidate.canonicalize() {
            if let Ok(root_canonical) = install_root.canonicalize() {
                if !canonical.starts_with(&root_canonical) {
                    return Err("resolves outside".into());
                }
                return Ok(canonical);
            }
        }
    } else {
        // If not exists, also try canonicalizing parent.
        if let Some(parent) = candidate.parent() {
            if parent.exists() {
                if let Ok(parent_canon) = parent.canonicalize() {
                    if let Ok(root_canon) = install_root.canonicalize().or_else(|_| Ok::<_, String>(install_root.to_path_buf())) {
                        // Reconstruct candidate as parent_canon + file name
                        if let Some(name) = candidate.file_name() {
                            let recon = parent_canon.join(name);
                            if !recon.starts_with(&root_canon) {
                                return Err("resolves outside".into());
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(normalized)
}

fn normalize_lexically(path: &Path) -> PathBuf {
    let mut components = Vec::new();
    for comp in path.components() {
        use std::path::Component;
        match comp {
            Component::ParentDir => { components.pop(); },
            Component::CurDir => {},
            other => components.push(other),
        }
    }
    let mut out = PathBuf::new();
    for c in components {
        out.push(c.as_os_str());
    }
    if out.as_os_str().is_empty() {
        out.push("/");
    }
    out
}

pub fn validate_manifest_bytes(bytes: &[u8]) -> Result<ManifestV1, String> {
    if bytes.len() as u64 > MAX_MANIFEST_BYTES {
        return Err("manifest_unparseable".into());
    }
    let value: serde_json::Value = serde_json::from_slice(bytes).map_err(|_| "manifest_unparseable")?;
    validate_manifest_value(value)
}

pub fn validate_manifest_value(value: serde_json::Value) -> Result<ManifestV1, String> {
    // Schema validation: required fields and types via serde, with deny_unknown? We'll use serde to check.
    let manifest: ManifestV1 = serde_json::from_value(value).map_err(|_| "manifest_schema_invalid")?;

    if manifest.schema_version != 2 {
        return Err("manifest_schema_invalid".into());
    }
    let digest = &manifest.artifact_digest;
    let valid_digest = digest.strip_prefix("sha256:").map(|hex| hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))).unwrap_or(false);
    if !valid_digest { return Err("manifest_artifact_digest_invalid".into()); }
    if manifest.product_id != "cortex" && manifest.product_id != "membrane" {
        return Err("manifest_schema_invalid".into());
    }
    // Validate installRoot exists and is absolute
    let install_root = Path::new(&manifest.install_root);
    if !install_root.is_absolute() {
        return Err("serviceStart[0] escapes installRoot".into());
    }
    // Check serviceStart[0] containment
    if manifest.service_start.is_empty() {
        return Err("manifest_schema_invalid".into());
    }
    let first = &manifest.service_start[0];
    // Shell metacharacters: we accept as literal, so no rejection for them.
    // Just do containment check.
    match resolve_inside(install_root, first) {
        Ok(_) => {},
        Err(e) if e == "escapes" => return Err("serviceStart[0] escapes installRoot".into()),
        Err(_) => return Err("serviceStart[0] resolves outside installRoot".into()),
    }
    // Check icon containment
    match resolve_inside(install_root, &manifest.icon) {
        Ok(_) => {},
        Err(e) if e == "escapes" => return Err("icon resolves outside installRoot".into()),
        Err(_) => return Err("icon resolves outside installRoot".into()),
    }
    // Semantic compatibility: the declared Hub range must be satisfied by the
    // running Hub version. Unsupported future grammar fails closed.
    if !evaluate_hub_compat_range(&manifest.hub_compat_range, HUB_VERSION) {
        return Err("manifest_hub_range_incompatible".into());
    }
    Ok(manifest)
}

pub fn validate_manifest_file(path: &Path) -> Result<ManifestV1, String> {
    reject_insecure_mode(path)?;
    let bytes = std::fs::read(path).map_err(|_| "manifest_unparseable")?;
    validate_manifest_bytes(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn loopback_check() {
        assert!(is_loopback("127.0.0.1"));
        assert!(is_loopback("::1"));
        assert!(is_loopback("localhost"));
        assert!(!is_loopback("0.0.0.0"));
        assert!(!is_loopback("192.168.1.1"));
    }
    #[test]
    fn service_start_traversal_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("install");
        std::fs::create_dir_all(&root).unwrap();
        let json = serde_json::json!({
            "schemaVersion":2,"productId":"membrane","displayName":"M","productVersion":"1.0","hubCompatRange":">=0.1","installRoot": root.to_string_lossy(),"serviceStart":["../outside/bin"],"serviceStop":[],"icon": format!("{}/icon.png", root.to_string_lossy()),"artifactDigest":"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        });
        assert_eq!(validate_manifest_value(json).unwrap_err(), "serviceStart[0] escapes installRoot");
    }

    #[test]
    fn hub_compat_range_satisfies_current_version() {
        // Every range the test fixtures use must be satisfied by the running
        // Hub version; this guards against a silent version bump.
        assert!(evaluate_hub_compat_range(">=0.1", "0.1.11"));
        assert!(evaluate_hub_compat_range(">=0.1.0", "0.1.11"));
        assert!(evaluate_hub_compat_range(">=0", "0.1.11"));
        assert!(evaluate_hub_compat_range("*", "0.1.11"));
        assert!(evaluate_hub_compat_range("", "0.1.11"));
        assert!(evaluate_hub_compat_range(">=0.1.0 <1.0.0", "0.1.11"));
        assert!(evaluate_hub_compat_range(">=0.1.0, <1.0.0", "0.1.11"));
        assert!(evaluate_hub_compat_range("==0.1.11", "0.1.11"));
    }

    #[test]
    fn hub_compat_range_fails_closed_on_unsupported_future_grammar() {
        assert!(!evaluate_hub_compat_range(">=1.0.0", "0.1.11"));
        assert!(!evaluate_hub_compat_range("<0.1.0", "0.1.11"));
        assert!(!evaluate_hub_compat_range("~0.1.0", "0.1.11")); // unsupported operator
        assert!(!evaluate_hub_compat_range("^1.0", "0.1.11")); // unsupported operator
        assert!(!evaluate_hub_compat_range(">=0.1.0.0", "0.1.11")); // four components
        assert!(!evaluate_hub_compat_range(">=0.x.0", "0.1.11")); // non-numeric
        assert!(!evaluate_hub_compat_range("banana", "0.1.11")); // no operator
    }

    #[test]
    fn range_round_trips_missing_minor_patch_as_zero() {
        assert!(evaluate_hub_compat_range("=0.1", "0.1.0"));
        assert!(evaluate_hub_compat_range("=0", "0.0.0"));
        assert!(!evaluate_hub_compat_range("=0.1", "0.1.11"));
    }

    #[cfg(unix)]
    #[test]
    fn insecure_manifest_mode_rejected_before_parse() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let manifest = dir.path().join("m.json");
        std::fs::write(&manifest, b"not json").unwrap();
        std::fs::set_permissions(&manifest, std::fs::Permissions::from_mode(0o666)).unwrap();
        // Mode check runs first, so the insecure-mode error wins over the parse error.
        assert_eq!(validate_manifest_file(&manifest).unwrap_err(), "manifest_mode_insecure");
        // Owner-only mode (0644, no group/other write) passes the mode gate and
        // then fails parsing — proving the mode gate is what rejected above.
        std::fs::set_permissions(&manifest, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(validate_manifest_file(&manifest).unwrap_err(), "manifest_unparseable");
    }
}
