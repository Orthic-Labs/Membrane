use std::{collections::BTreeMap, path::{Path, PathBuf}};
use crate::manifest_validate::validate_manifest_file;
use crate::schema_types::ManifestV1;

pub fn products_dir() -> PathBuf {
    dirs_fallback()
}

#[cfg(target_os = "windows")]
fn dirs_fallback() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("C:\\"))
        .join(".orthic/hub/products.d")
}

#[cfg(not(target_os = "windows"))]
fn dirs_fallback() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(".orthic/hub/products.d")
}

pub fn scan_products_dir(dir: &Path) -> Vec<ManifestV1> {
    let mut out = BTreeMap::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        match validate_manifest_file(&path) {
            Ok(m) => {
                if out.insert(m.product_id.clone(), m).is_some() {
                    eprintln!("manifest rejected {}: duplicate productId", path.display());
                }
            }
            Err(e) => {
                eprintln!("manifest rejected {}: {}", path.display(), e);
            }
        }
    }
    out.into_values().collect()
}

pub fn discover_manifests() -> Vec<ManifestV1> {
    let dir = products_dir();
    scan_products_dir(&dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    #[test]
    fn scan_empty_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(scan_products_dir(dir.path()).len(), 0);
    }
    #[test]
    fn scan_collects_valid_only() {
        let dir = tempfile::tempdir().unwrap();
        let install = dir.path().join("prod");
        fs::create_dir_all(&install).unwrap();
        let valid = serde_json::json!({
            "schemaVersion":2,"productId":"membrane","displayName":"Membrane","productVersion":"1.0","hubCompatRange":">=0.1","installRoot": install.to_string_lossy(),"serviceStart":[format!("{}/bin", install.to_string_lossy())],"serviceStop":[],"icon": format!("{}/icon.png", install.to_string_lossy()),"artifactDigest":"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        });
        fs::write(dir.path().join("a.json"), serde_json::to_vec(&valid).unwrap()).unwrap();
        fs::write(dir.path().join("bad.json"), b"not json").unwrap();
        let found = scan_products_dir(dir.path());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].product_id, "membrane");
    }
}
