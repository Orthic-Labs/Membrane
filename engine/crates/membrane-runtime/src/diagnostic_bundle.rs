//! Content-free diagnostic archive generation.
//!
//! MBR-209 records health metadata and digests only. Prompt bodies, source files, token values,
//! database rows, paths, doctor sample ids, and repair text never enter this archive.

use crate::{digest, doctor, installation_manifest, release_identity, serve};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;

pub const DIAGNOSTIC_BUNDLE_SCHEMA_VERSION: &str = "DiagnosticBundleV1";
#[cfg(test)]
const ENTRY_NAMES: [&str; 10] = [
    "adapters",
    "database",
    "delivery",
    "identity",
    "ports",
    "providers",
    "schemas",
    "signatures",
    "tokens",
    "versions",
];

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticBundleV1 {
    pub schema_version: &'static str,
    pub redaction: &'static str,
    pub entries: BTreeMap<String, Value>,
    pub manifest: BTreeMap<String, String>,
}

fn sha256(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("sha256:")
        && value[7..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn delivery_state(report: &doctor::DoctorReportV0) -> Value {
    let checks = report
        .checks
        .iter()
        .filter(|check| check.code.contains("INJECTED") || check.code.contains("CHECKPOINT"))
        .collect::<Vec<_>>();
    if checks.is_empty() {
        return json!({"state": "unavailable", "reason": "doctor-schema-has-no-delivery-check"});
    }
    let state = if checks
        .iter()
        .any(|check| check.severity == "critical" && check.count > 0)
    {
        "critical"
    } else if checks.iter().any(|check| check.count > 0) {
        "warning"
    } else {
        "ok"
    };
    json!({"state": state, "checked": checks.len()})
}

/// Construct a support archive from observed, already-redacted facts. Signals without a CLI
/// reader are typed unavailable rather than treated as a successful validation.
pub fn build(report: &doctor::DoctorReportV0, db_path: &Path) -> DiagnosticBundleV1 {
    let mut entries: BTreeMap<String, Value> = BTreeMap::new();
    let schemas = installation_manifest::active_api_schemas()
        .into_iter()
        .map(|(name, schema)| {
            (
                name,
                json!({"version": schema.version, "digest": schema.digest_sha256}),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let release_digest = release_identity::release_generation();
    let identity_ok = sha256(&release_digest) && release_identity::target_triple() != "unsupported";
    let token_path = std::env::var_os("CRYPT_API_TOKEN_FILE")
        .map(std::path::PathBuf::from)
        .or_else(|| db_path.parent().map(|parent| parent.join("api-token")));
    let token = match token_path {
        Some(path) if serve::token_file_is_valid_without_mutation(&path) => {
            json!({"state": "ok", "reason": null})
        }
        Some(_) => json!({"state": "invalid", "reason": "token-file-missing-or-invalid"}),
        None => json!({"state": "unavailable", "reason": "token-file-path-unavailable"}),
    };
    let port = match std::env::var("CRYPT_PORT") {
        Ok(value) => match value.parse::<u16>() {
            Ok(port) if port >= 1024 => json!({"state": "ok", "port": port, "reason": null}),
            _ => {
                json!({"state": "invalid", "port": null, "reason": "port-is-not-unprivileged-u16"})
            }
        },
        Err(_) => {
            json!({"state": "unavailable", "port": null, "reason": "runtime-port-not-published"})
        }
    };
    let doctor_checks = report
        .checks
        .iter()
        .map(|check| json!({"code": check.code, "severity": check.severity, "count": check.count}))
        .collect::<Vec<_>>();

    entries.insert(
        "identity".into(),
        json!({
            "state": if identity_ok { "ok" } else { "unavailable" },
            "releaseDigest": if sha256(&release_digest) { Value::String(release_digest) } else { Value::Null },
            "target": release_identity::target_triple(),
            "manifestPublished": installation_manifest::active_manifest().is_some(),
            "reason": if identity_ok { Value::Null } else { json!("build-identity-unavailable") },
        }),
    );
    entries.insert(
        "signatures".into(),
        json!({"state": "unavailable", "reason": "runtime-has-no-signature-verifier"}),
    );
    entries.insert("schemas".into(), json!({"state": "ok", "bound": schemas}));
    entries.insert("ports".into(), port);
    entries.insert("tokens".into(), token);
    entries.insert(
        "database".into(),
        json!({"state": report.status, "doctorSchema": report.schema_version, "checks": doctor_checks}),
    );
    entries.insert(
        "providers".into(),
        json!({"state": "unavailable", "reason": "cli-has-no-provider-readiness-handle"}),
    );
    entries.insert(
        "adapters".into(),
        json!({"state": "unavailable", "reason": "cli-has-no-adapter-heartbeat-handle"}),
    );
    entries.insert("delivery".into(), delivery_state(report));
    entries.insert(
        "versions".into(),
        json!({"runtime": env!("CARGO_PKG_VERSION"), "schemas": schemas.len()}),
    );

    let manifest = entries
        .iter()
        .map(|(name, entry)| {
            (
                name.clone(),
                digest::digest_bytes(&serde_json::to_vec(entry).unwrap()),
            )
        })
        .collect();
    DiagnosticBundleV1 {
        schema_version: DIAGNOSTIC_BUNDLE_SCHEMA_VERSION,
        redaction: "content-free",
        entries,
        manifest,
    }
}

/// Serialize one local archive. Parent directories must already exist.
pub fn write(path: &Path, db_path: &Path, report: &doctor::DoctorReportV0) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(&build(report, db_path))
        .map_err(|error| format!("serialize diagnostic bundle: {error}"))?;
    std::fs::write(path, bytes)
        .map_err(|error| format!("write diagnostic bundle {}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn report() -> doctor::DoctorReportV0 {
        doctor::DoctorReportV0 {
            schema_version: "CryptDoctorV0",
            system: "Membrane",
            status: "warning",
            checks: vec![doctor::DoctorCheckV0 {
                code: "MRD-CHECKPOINT-EXPIRED",
                severity: "critical",
                count: 1,
                sample_ids: vec!["prompt-content-must-not-escape".into()],
                repair: "source-content-must-not-escape",
            }],
        }
    }

    #[test]
    fn bundle_pairs_exact_entries_with_hashes_and_excludes_sensitive_content() {
        let archive = build(&report(), Path::new("/private/db.sqlite"));
        let raw = serde_json::to_string(&archive).unwrap();
        assert_eq!(archive.schema_version, DIAGNOSTIC_BUNDLE_SCHEMA_VERSION);
        assert_eq!(archive.redaction, "content-free");
        let expected = ENTRY_NAMES.iter().copied().collect::<BTreeSet<_>>();
        assert_eq!(
            archive
                .entries
                .keys()
                .map(String::as_str)
                .collect::<BTreeSet<_>>(),
            expected
        );
        assert_eq!(
            archive.entries.keys().collect::<Vec<_>>(),
            archive.manifest.keys().collect::<Vec<_>>()
        );
        assert!(!raw.contains("prompt-content-must-not-escape"));
        assert!(!raw.contains("source-content-must-not-escape"));
        assert!(!raw.contains("/private/db.sqlite"));
        for (name, entry) in &archive.entries {
            assert_eq!(
                archive.manifest[name],
                digest::digest_bytes(&serde_json::to_vec(entry).unwrap())
            );
        }
    }

    #[test]
    fn schema_enumerates_same_exact_keys_as_archive_and_manifest() {
        let expected = ENTRY_NAMES.iter().copied().collect::<BTreeSet<_>>();
        let schema: Value = serde_json::from_str(include_str!(
            "../../../../schemas/diagnostic-bundle.v1.schema.json"
        ))
        .unwrap();
        for pointer in ["/$defs/entries/required", "/$defs/manifest/required"] {
            let names = schema
                .pointer(pointer)
                .and_then(Value::as_array)
                .unwrap()
                .iter()
                .map(Value::as_str)
                .collect::<Option<Vec<_>>>()
                .unwrap();
            assert_eq!(names.into_iter().collect::<BTreeSet<_>>(), expected);
        }
        assert_eq!(
            schema.pointer("/$defs/entries/additionalProperties"),
            Some(&Value::Bool(false))
        );
        assert_eq!(
            schema.pointer("/$defs/manifest/additionalProperties"),
            Some(&Value::Bool(false))
        );
    }
}
