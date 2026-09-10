//! Native Rust port of `blueprint/src/lib/operations/support-bundle.mjs`.
//!
//! Lane LIB3 (r5 closure): no prior native equivalent found (grep of
//! `buildSupportBundle`/`SUPPORT_BUNDLE_ALLOWLIST` across
//! membrane-blueprint/src and membrane-runtime/src produced no match); this
//! port reuses the existing `lib_redaction::redact_for_egress` (LIB4) for
//! secret scrubbing rather than redefining it. Ported behavior: writes an
//! allowlisted set of redacted JSON/log records into a bundle directory,
//! rewriting home/root paths as `$HOME`/`$REPO`, and produces a checksums
//! manifest over the written files.

use crate::lib_redaction::redact_for_egress;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

pub const SUPPORT_BUNDLE_ALLOWLIST: [&str; 10] = [
    "summary.json",
    "versions.json",
    "installation.json",
    "service-status.json",
    "repository-status.json",
    "doctor.json",
    "repair-plan.json",
    "logs/watchman-tail.log",
    "logs/service-tail.log",
    "checksums.txt",
];

/// Mirrors `redactPath(value, root)`: replaces `home`/`root` occurrences and
/// normalizes backslashes to forward slashes.
pub fn redact_path(value: &str, home: &str, root: &str) -> String {
    value
        .replace(home, "$HOME")
        .replace(root, "$REPO")
        .replace('\\', "/")
}

/// Mirrors `redactRecordPaths(record, root)`: walks a JSON value, rewriting
/// string values whose key looks path-shaped (`root|path|home|dir|location`,
/// case-insensitive).
pub fn redact_record_paths(record: &Value, home: &str, root: &str) -> Value {
    match record {
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| redact_record_paths(item, home, root))
                .collect(),
        ),
        Value::Object(map) => {
            let mut out = serde_json::Map::new();
            for (key, value) in map {
                let lower = key.to_lowercase();
                let looks_path_shaped = ["root", "path", "home", "dir", "location"]
                    .iter()
                    .any(|needle| lower.contains(needle));
                let next = if looks_path_shaped {
                    match value {
                        Value::String(s) => Value::String(redact_path(s, home, root)),
                        other => redact_record_paths(other, home, root),
                    }
                } else {
                    redact_record_paths(value, home, root)
                };
                out.insert(key.clone(), next);
            }
            Value::Object(out)
        }
        other => other.clone(),
    }
}

pub fn checksum_file(path: &Path) -> std::io::Result<String> {
    let bytes = fs::read(path)?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Ok(hex::encode(hasher.finalize()))
}

pub struct SupportBundleRecords {
    pub package_channel: Option<String>,
    pub installation: Option<Value>,
    pub service_status: Option<Value>,
    pub repository_status: Option<Value>,
    pub doctor: Option<Value>,
    pub repair_plan: Option<Value>,
    pub watchman_log: Option<String>,
    pub service_log: Option<String>,
}

pub struct SupportBundleResult {
    pub path: PathBuf,
    pub files: Vec<&'static str>,
    pub checksums: Vec<(String, String)>,
}

fn write_json(path: &Path, value: &Value) -> std::io::Result<()> {
    fs::write(path, format!("{}\n", serde_json::to_string_pretty(value).unwrap()))
}

/// Mirrors `buildSupportBundle({ root, outDir, destination, records })`.
/// `now`/`platform_arch`/`home` are injected instead of read from process
/// globals so the port stays deterministic and host-agnostic.
pub fn build_support_bundle(
    root: &Path,
    bundle_root: &Path,
    home: &str,
    now_iso: &str,
    platform_arch: &str,
    records: &SupportBundleRecords,
) -> std::io::Result<SupportBundleResult> {
    fs::create_dir_all(bundle_root)?;
    fs::create_dir_all(bundle_root.join("logs"))?;

    let root_str = root.to_string_lossy().to_string();
    let redacted_root = redact_path(&root_str, home, &root_str);

    let summary = json!({
        "schemaVersion": 1,
        "product": "blueprint",
        "createdAt": now_iso,
        "repoRoot": redacted_root,
        "records": SUPPORT_BUNDLE_ALLOWLIST,
    });
    write_json(&bundle_root.join("summary.json"), &summary)?;

    let versions = json!({
        "platform": platform_arch,
        "packageChannel": records.package_channel.clone().unwrap_or_else(|| "unknown".to_string()),
    });
    write_json(
        &bundle_root.join("versions.json"),
        &redact_for_egress(&versions, false),
    )?;

    let installation = records
        .installation
        .clone()
        .unwrap_or_else(|| json!({ "scope": "unknown", "root": redacted_root }));
    write_json(
        &bundle_root.join("installation.json"),
        &redact_for_egress(&redact_record_paths(&installation, home, &root_str), false),
    )?;

    let service_status = records
        .service_status
        .clone()
        .unwrap_or_else(|| json!({ "state": "unknown" }));
    write_json(
        &bundle_root.join("service-status.json"),
        &redact_for_egress(&service_status, false),
    )?;

    let repository_status = records
        .repository_status
        .clone()
        .unwrap_or_else(|| json!({ "state": "unknown", "repoIds": [] }));
    write_json(
        &bundle_root.join("repository-status.json"),
        &redact_for_egress(&repository_status, false),
    )?;

    let doctor = records.doctor.clone().unwrap_or_else(|| json!({ "state": "unknown" }));
    write_json(&bundle_root.join("doctor.json"), &redact_for_egress(&doctor, false))?;

    let repair_plan = records
        .repair_plan
        .clone()
        .unwrap_or_else(|| json!({ "schemaVersion": 1, "actions": [] }));
    write_json(
        &bundle_root.join("repair-plan.json"),
        &redact_for_egress(&repair_plan, false),
    )?;

    let log_dir = bundle_root.join("logs");
    for (name, content) in [
        ("watchman-tail.log", records.watchman_log.as_deref().unwrap_or("")),
        ("service-tail.log", records.service_log.as_deref().unwrap_or("")),
    ] {
        let redacted = content
            .split(['\n'])
            .map(|line| {
                let path_redacted = redact_path(line.trim_end_matches('\r'), home, &root_str);
                match redact_for_egress(&Value::String(path_redacted), false) {
                    Value::String(s) => s,
                    other => other.to_string(),
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(log_dir.join(name), format!("{redacted}\n"))?;
    }

    let mut checksums = Vec::new();
    for name in SUPPORT_BUNDLE_ALLOWLIST {
        let path = bundle_root.join(name);
        if path.exists() {
            checksums.push((name.to_string(), checksum_file(&path)?));
        }
    }
    let checksums_text = checksums
        .iter()
        .map(|(name, hash)| format!("{hash}  {name}"))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    fs::write(bundle_root.join("checksums.txt"), checksums_text)?;

    Ok(SupportBundleResult {
        path: bundle_root.to_path_buf(),
        files: SUPPORT_BUNDLE_ALLOWLIST.to_vec(),
        checksums,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redact_path_rewrites_home_before_root_matching_legacy_replaceall_order() {
        // Legacy order is replaceAll(home) then replaceAll(root); when home
        // is a prefix of root (as in a typical repo-under-home layout), the
        // home substitution consumes root's own text first, so the visible
        // result is $HOME-prefixed rather than $REPO-prefixed. This mirrors
        // that exact order-dependent behavior rather than "fixing" it.
        let out = redact_path(r"C:\Users\me\repo\src\file.rs", r"C:\Users\me", r"C:\Users\me\repo");
        assert_eq!(out, "$HOME/repo/src/file.rs");
    }

    #[test]
    fn redact_path_rewrites_root_when_root_is_not_prefixed_by_home() {
        let out = redact_path(r"/var/repo/src/file.rs", "/home/me", "/var/repo");
        assert_eq!(out, "$REPO/src/file.rs");
    }

    #[test]
    fn redact_record_paths_only_touches_path_shaped_keys() {
        let record = json!({
            "root": "/var/repo",
            "message": "/var/repo/notes.txt",
        });
        let redacted = redact_record_paths(&record, "/home/me", "/var/repo");
        assert_eq!(redacted["root"], json!("$REPO"));
        // "message" does not match the path-shaped key heuristic, so it is
        // left untouched by this pass (though redact_for_egress may still
        // scrub secret-shaped values inside it separately).
        assert_eq!(redacted["message"], json!("/var/repo/notes.txt"));
    }

    #[test]
    fn build_support_bundle_writes_allowlisted_files_and_checksums() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("repo");
        fs::create_dir_all(&root).unwrap();
        let bundle_root = root.join(".agent").join("support-bundle");
        let records = SupportBundleRecords {
            package_channel: Some("stable".to_string()),
            installation: None,
            service_status: None,
            repository_status: None,
            doctor: None,
            repair_plan: None,
            watchman_log: Some("line one\nline two".to_string()),
            service_log: None,
        };
        let result = build_support_bundle(
            &root,
            &bundle_root,
            "/home/me",
            "2026-01-01T00:00:00.000Z",
            "linux-x64",
            &records,
        )
        .unwrap();
        assert_eq!(result.files.len(), SUPPORT_BUNDLE_ALLOWLIST.len());
        assert!(bundle_root.join("summary.json").exists());
        assert!(bundle_root.join("checksums.txt").exists());
        assert!(bundle_root.join("logs/watchman-tail.log").exists());
        // Every allowlisted JSON/log file produced a checksum entry.
        assert_eq!(result.checksums.len(), SUPPORT_BUNDLE_ALLOWLIST.len() - 1);

        let summary: Value =
            serde_json::from_str(&fs::read_to_string(bundle_root.join("summary.json")).unwrap())
                .unwrap();
        assert_eq!(summary["product"], json!("blueprint"));
    }
}
