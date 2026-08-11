use std::path::{Path, PathBuf};
use crate::schema_types::ManifestV1;

const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;

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

    if manifest.schema_version != 1 {
        return Err("manifest_schema_invalid".into());
    }
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
    // statusEndpoint host loopback check
    if !is_loopback(&manifest.status_endpoint.host) {
        return Err("statusEndpoint_not_loopback".into());
    }
    if manifest.status_endpoint.port == 0
        || !is_http_header_name(&manifest.status_endpoint.auth_header)
        || manifest.status_endpoint.auth_token.contains(['\r', '\n'])
    {
        return Err("manifest_schema_invalid".into());
    }

    Ok(manifest)
}

pub fn validate_manifest_file(path: &Path) -> Result<ManifestV1, String> {
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
            "schemaVersion":1,"productId":"membrane","displayName":"M","productVersion":"1.0","hubCompatRange":">=0.1","installRoot": root.to_string_lossy(),"serviceStart":["../outside/bin"],"serviceStop":[],"statusEndpoint":{"host":"127.0.0.1","port":8080,"authHeader":"H","authToken":"T"},"icon": format!("{}/icon.png", root.to_string_lossy())
        });
        assert_eq!(validate_manifest_value(json).unwrap_err(), "serviceStart[0] escapes installRoot");
    }
}
