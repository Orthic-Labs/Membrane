//! Native Rust port of `blueprint/src/lib/orientation-evidence.mjs`.
//!
//! Lane LIB4 (r5 closure): this module has no prior native equivalent.
//! Forge-consumable orientation evidence — typed blob derived from
//! receipts. Blueprint emits evidence; Forge attests and gates signoff.
//! This module does not enforce signoff — it only shapes the record Forge
//! can consume.

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

pub const ORIENTATION_EVIDENCE_KIND: &str = "blueprint_orientation";
pub const ORIENTATION_EVIDENCE_SCHEMA_VERSION: u32 = 1;

/// Deterministic (key-sorted) JSON stringification, mirroring
/// `stableStringify` in the legacy JS module.
pub fn stable_stringify(value: &Value) -> String {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let parts: Vec<String> = keys
                .into_iter()
                .map(|k| {
                    format!(
                        "{}:{}",
                        serde_json::to_string(k).unwrap(),
                        stable_stringify(&map[k])
                    )
                })
                .collect();
            format!("{{{}}}", parts.join(","))
        }
        Value::Array(items) => {
            let parts: Vec<String> = items.iter().map(stable_stringify).collect();
            format!("[{}]", parts.join(","))
        }
        other => serde_json::to_string(other).unwrap(),
    }
}

/// sha256 hex digest of a value: strings are hashed as-is, everything else
/// via `stable_stringify`. Mirrors `sha256Hex`.
pub fn sha256_hex_value(value: &Value) -> String {
    let bytes = match value {
        Value::String(s) => s.clone(),
        other => stable_stringify(other),
    };
    hex::encode(Sha256::digest(bytes.as_bytes()))
}

pub fn sha256_hex_str(value: &str) -> String {
    hex::encode(Sha256::digest(value.as_bytes()))
}

/// Digest a ContextCandidateSet (or equivalent) for receipt / evidence
/// binding. Mirrors `candidateSetDigest`.
pub fn candidate_set_digest(candidate_set: &Value) -> String {
    let candidates: Vec<Value> = candidate_set
        .get("candidates")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|c| {
            json!({
                "id": c.get("id").cloned().unwrap_or(Value::Null),
                "sourceRef": c.get("sourceRef").cloned().unwrap_or(Value::Null),
                "sourceHash": c.get("sourceHash").cloned().unwrap_or(Value::Null),
                "protected": c.get("protected").cloned().unwrap_or(Value::Null),
                "exact": c.get("exact").cloned().unwrap_or(Value::Null),
            })
        })
        .collect();

    let omissions: Vec<Value> = candidate_set
        .get("omissions")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|o| {
            json!({
                "id": o.get("id").cloned().unwrap_or(Value::Null),
                "reason": o.get("reason").cloned().unwrap_or(Value::Null),
            })
        })
        .collect();

    let payload = json!({
        "schemaVersion": candidate_set.get("schemaVersion").cloned().unwrap_or(json!(1)),
        "provider": candidate_set.get("provider").cloned().unwrap_or(Value::Null),
        "freshness": candidate_set.get("freshness").cloned().unwrap_or(Value::Null),
        "candidates": candidates,
        "omissions": omissions,
    });
    format!("sha256:{}", sha256_hex_value(&payload))
}

#[derive(Debug)]
pub struct OrientationEvidenceError(pub String);

impl std::fmt::Display for OrientationEvidenceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl std::error::Error for OrientationEvidenceError {}

fn get_str<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

/// Build a Forge-compatible evidence record from an orientation receipt.
/// Mirrors `buildOrientationEvidence`. `supports_or_refutes` mirrors the JS
/// `options.supportsOrRefutes` (default `"supports"`).
pub fn build_orientation_evidence(
    receipt: &Value,
    supports_or_refutes: Option<&str>,
) -> Result<Value, OrientationEvidenceError> {
    let receipt_id = get_str(receipt, "receiptId").ok_or_else(|| {
        OrientationEvidenceError(
            "orientation evidence requires a receipt with receiptId".to_string(),
        )
    })?;

    let overlay_revision = receipt
        .get("overlayRevision")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let manifest_digest = get_str(receipt, "manifestDigest").unwrap_or("unknown");

    let content_hash = if let Some(digest) = get_str(receipt, "candidateSetDigest") {
        digest.to_string()
    } else if let Some(ch) = receipt.get("contentHash") {
        if !ch.is_null() {
            match ch {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            }
        } else {
            format!("sha256:{}", sha256_hex_str(receipt_id))
        }
    } else {
        format!("sha256:{}", sha256_hex_str(receipt_id))
    };

    // Format overlay_revision the way JS `Number(...)` template-literal
    // interpolation would (integers without a trailing ".0").
    let overlay_revision_str = if overlay_revision.fract() == 0.0 {
        format!("{}", overlay_revision as i64)
    } else {
        format!("{}", overlay_revision)
    };

    Ok(json!({
        "schemaVersion": ORIENTATION_EVIDENCE_SCHEMA_VERSION,
        "kind": ORIENTATION_EVIDENCE_KIND,
        "locator": format!("blueprint://receipt/{}", receipt_id),
        "content_hash": content_hash,
        "invalidation_key": format!("{}:{}", manifest_digest, overlay_revision_str),
        "trust_class": "host_attested",
        "supports_or_refutes": supports_or_refutes.unwrap_or("supports"),
        "receiptId": receipt_id,
        "generationId": receipt.get("generationId").cloned().unwrap_or(Value::Null),
        "manifestDigest": receipt.get("manifestDigest").cloned().unwrap_or(Value::Null),
        "sessionId": receipt.get("sessionId").cloned().unwrap_or(Value::Null),
        "taskId": receipt.get("taskId").cloned().unwrap_or(Value::Null),
        "repoIdentity": receipt.get("repoIdentity").cloned().unwrap_or(Value::Null),
        "issuedAt": receipt.get("issuedAt").cloned().unwrap_or(Value::Null),
        "status": receipt.get("status").and_then(Value::as_str).unwrap_or("active"),
    }))
}

/// Write evidence to a file Forge can attest (atomic replace). Returns the
/// absolute path written plus the evidence value. Mirrors
/// `writeOrientationEvidenceFile`.
pub fn write_orientation_evidence_file(
    receipt: &Value,
    out_path: &Path,
    supports_or_refutes: Option<&str>,
) -> Result<(PathBuf, Value), OrientationEvidenceError> {
    let evidence = build_orientation_evidence(receipt, supports_or_refutes)?;
    let target = if out_path.is_absolute() {
        out_path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|e| OrientationEvidenceError(e.to_string()))?
            .join(out_path)
    };
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|e| OrientationEvidenceError(e.to_string()))?;
    }
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let mut tmp_name = target
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("evidence")
        .to_string();
    tmp_name.push_str(&format!(".{}.{}.tmp", std::process::id(), millis));
    let tmp = target.with_file_name(tmp_name);
    let body = format!("{}\n", serde_json::to_string_pretty(&evidence).unwrap());
    fs::write(&tmp, body).map_err(|e| OrientationEvidenceError(e.to_string()))?;
    fs::rename(&tmp, &target).map_err(|e| OrientationEvidenceError(e.to_string()))?;
    Ok((target, evidence))
}

/// Default evidence filename for a receipt under a directory. Mirrors
/// `defaultEvidencePath`.
pub fn default_evidence_path(dir: &Path, receipt_id: &str) -> PathBuf {
    let resolved = if dir.is_absolute() {
        dir.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_default()
            .join(dir)
    };
    resolved.join(format!("blueprint-orientation-{}.json", receipt_id))
}
