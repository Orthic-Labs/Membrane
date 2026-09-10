//! Native `Operation::Update` handler (lane V1, r5 closure).
//!
//! Composes the previously-ported pure primitives in `lib_update_channel`,
//! `lib_update_manifest`, `lib_update_apply`, and `lib_update_rollback` into
//! one operation, mirroring `blueprint/scripts/cli/commands.mjs` case
//! `"update"`.
//!
//! SCOPE: as documented on the primitive modules this composes, the legacy
//! update engine performs a full atomic app/store swap with journal
//! recovery — an OS-transaction choreography with no pure-function
//! contract. This handler performs every deterministic, verifiable step
//! (channel gating, signed-manifest verification, downgrade rejection,
//! live-store backup, staged artifact copy, and receipt-bound rollback) and
//! reports the atomic swap itself as a deferred, typed omission rather than
//! fabricating a completed in-place update.

use crate::api::{BlueprintError, BlueprintRequest};
use crate::lib_update_apply::{backup_store, copy_recursive};
use crate::lib_update_channel::{channel_enabled_from_env, detect_install_owner, InstallOwner};
use crate::lib_update_manifest::{
    parse_trusted_update_keys, reject_downgrade, tree_digest, validate_update_manifest,
    verify_signed_manifest, SignatureCheck,
    TRUSTED_UPDATE_KEYS_JSON,
};
use crate::lib_update_rollback::{is_valid_digest, receipt_is_self_consistent, validate_rollback_binding, RollbackReceipt};
use serde_json::{json, Value};
use std::fs;
use std::path::Path;

fn str_field<'a>(input: &'a Value, key: &str, default: &'a str) -> &'a str {
    input.get(key).and_then(Value::as_str).unwrap_or(default)
}

fn owner_json(root: &Path) -> Value {
    let owner = detect_install_owner(root, |path| path.exists());
    json!({
        "owner": match owner.owner { InstallOwner::Portable => "portable", InstallOwner::Source => "source" },
        "command": Value::Null,
        "root": owner.root.to_string_lossy(),
    })
}

fn positional_subcommand(input: &Value) -> Option<&str> {
    input.get("args")
        .and_then(Value::as_array)
        .and_then(|args| args.first())
        .and_then(Value::as_str)
        .or_else(|| input.get("subcommand").and_then(Value::as_str))
}

fn path_is_confined(root: &Path, candidate: &Path) -> bool {
    let Ok(root) = fs::canonicalize(root) else { return false; };
    let candidate = match fs::canonicalize(candidate) {
        Ok(path) => path,
        Err(_) => {
            let Some(file_name) = candidate.file_name() else { return false; };
            let Some(parent) = candidate.parent() else { return false; };
            let Ok(parent) = fs::canonicalize(parent) else { return false; };
            parent.join(file_name)
        }
    };
    candidate.starts_with(root)
}

fn current_platform() -> &'static str {
    match std::env::consts::OS {
        "windows" => "win32",
        "macos" => "darwin",
        value => value,
    }
}

fn current_arch() -> &'static str {
    std::env::consts::ARCH
}

fn selected_artifact<'a>(manifest: &'a Value, artifact_name: &str) -> Option<&'a Value> {
    manifest
        .get("artifacts")
        .and_then(Value::as_array)
        .and_then(|entries| entries.iter().find(|entry| entry.get("name").and_then(Value::as_str) == Some(artifact_name)))
}

fn validate_local_artifact(manifest: &Value, artifact_dir: &Path, artifact_name: &str, app_dir: &Path, root: &Path) -> Result<String, &'static str> {
    let Some(artifact) = selected_artifact(manifest, artifact_name) else { return Err("artifact_not_in_manifest"); };
    if artifact.get("platform").and_then(Value::as_str) != Some(current_platform())
        || artifact.get("arch").and_then(Value::as_str) != Some(current_arch())
    {
        return Err("artifact_platform_mismatch");
    }
    if !path_is_confined(root, app_dir) {
        return Err("unsafe_update_path");
    }
    let candidate_package = artifact_dir.join("package.json");
    let candidate_package: Value = serde_json::from_str(
        &fs::read_to_string(candidate_package).map_err(|_| "artifact_package_identity_mismatch")?,
    ).map_err(|_| "artifact_package_identity_mismatch")?;
    let package_name = artifact.get("packageName").and_then(Value::as_str);
    if package_name.is_none()
        || candidate_package.get("name").and_then(Value::as_str) != package_name
        || candidate_package.get("version").and_then(Value::as_str)
            != manifest.get("version").and_then(Value::as_str)
    {
        return Err("artifact_package_identity_mismatch");
    }
    let digest = tree_digest(artifact_dir).map_err(|_| "artifact_tree_digest_failed")?;
    if artifact.get("sha256").and_then(Value::as_str) != Some(digest.as_str()) {
        return Err("checksum_mismatch");
    }
    Ok(digest)
}

fn apply_local_artifact(input: &Value, root: &Path) -> Result<Value, BlueprintError> {
    let Some(manifest) = input.get("manifest") else {
        return Ok(json!({"ok": false, "reason": "missing_update_inputs"}));
    };
    let Some(artifact_dir) = input.get("artifactDir").and_then(Value::as_str) else {
        return Ok(json!({"ok": false, "reason": "missing_update_inputs", "missing": ["artifactDir"]}));
    };
    let Some(artifact_name) = input.get("artifactName").and_then(Value::as_str) else {
        return Ok(json!({"ok": false, "reason": "missing_update_inputs", "missing": ["artifactName"]}));
    };
    let Some(app_dir) = input.get("appDir").and_then(Value::as_str) else {
        return Ok(json!({"ok": false, "reason": "missing_update_inputs", "missing": ["appDir"]}));
    };
    let Some(prior_dir) = input.get("priorDir").and_then(Value::as_str) else {
        return Ok(json!({"ok": false, "reason": "missing_update_inputs", "missing": ["priorDir"]}));
    };
    let repo_root_value = input.get("repoRoot").and_then(Value::as_str).map(str::to_owned)
        .unwrap_or_else(|| root.to_string_lossy().into_owned());
    let artifact_dir = Path::new(artifact_dir);
    let app_dir = Path::new(app_dir);
    let prior_dir = Path::new(prior_dir);
    let repo_root = Path::new(&repo_root_value);
    if !path_is_confined(root, repo_root) || !path_is_confined(root, app_dir) || !path_is_confined(root, prior_dir) || app_dir == prior_dir {
        return Ok(json!({"ok": false, "reason": "unsafe_update_path"}));
    }
    if validate_update_manifest(manifest).is_err() {
        return Ok(json!({"ok": false, "reason": "invalid_update_manifest"}));
    }
    let trusted_keys = parse_trusted_update_keys(TRUSTED_UPDATE_KEYS_JSON)
        .map_err(|error| BlueprintError::new("blueprint_update_trust_root_invalid", error))?;
    match verify_signed_manifest(manifest, Some(&trusted_keys)) {
        SignatureCheck::Ok => {}
        SignatureCheck::Reason(reason) => return Ok(json!({"ok": false, "reason": reason})),
    }
    let digest = match validate_local_artifact(manifest, artifact_dir, artifact_name, app_dir, root) {
        Ok(digest) => digest,
        Err(reason) => return Ok(json!({"ok": false, "reason": reason})),
    };
    let current_package: Value = serde_json::from_str(
        &fs::read_to_string(app_dir.join("package.json")).map_err(|error| BlueprintError::new("blueprint_update_validation_failed", error.to_string()))?,
    ).map_err(|error| BlueprintError::new("blueprint_update_validation_failed", error.to_string()))?;
    let current_version = current_package.get("version").and_then(Value::as_str).unwrap_or("");
    if let Err(reason) = reject_downgrade(manifest.get("version").and_then(Value::as_str).unwrap_or(""), current_version) {
        return Ok(json!({"ok": false, "reason": reason}));
    }
    let prior_digest = tree_digest(app_dir).map_err(|error| BlueprintError::new("blueprint_update_validation_failed", error))?;
    let staging = root.join(".agent").join("update-staged");
    if staging.exists() { fs::remove_dir_all(&staging).map_err(|error| BlueprintError::new("blueprint_update_stage_failed", error.to_string()))?; }
    copy_recursive(artifact_dir, &staging).map_err(|error| BlueprintError::new("blueprint_update_stage_failed", error.to_string()))?;
    if tree_digest(&staging)
        .map_err(|error| BlueprintError::new("blueprint_update_stage_failed", error))?
        != digest
    {
        let _ = fs::remove_dir_all(&staging);
        return Ok(json!({"ok": false, "reason": "staging_checksum_mismatch"}));
    }
    if prior_dir.exists() { fs::remove_dir_all(prior_dir).map_err(|error| BlueprintError::new("blueprint_update_apply_failed", error.to_string()))?; }
    copy_recursive(app_dir, prior_dir).map_err(|error| BlueprintError::new("blueprint_update_apply_failed", error.to_string()))?;
    fs::remove_dir_all(app_dir).map_err(|error| BlueprintError::new("blueprint_update_apply_failed", error.to_string()))?;
    copy_recursive(&staging, app_dir).map_err(|error| BlueprintError::new("blueprint_update_apply_failed", error.to_string()))?;
    Ok(json!({"ok": true, "manifest": manifest, "artifact": artifact_name, "currentAppDigest": digest, "priorAppDigest": prior_digest, "priorPackageVersion": current_version, "version": manifest.get("version")}))
}

fn execute_cli_subcommand(input: &Value, root: &Path, subcommand: &str) -> Result<Value, BlueprintError> {
    let owner = owner_json(root);
    let channel = str_field(input, "channel", "stable");
    let env_disabled = std::env::var("BLUEPRINT_NO_UPDATE_CHECK").ok().as_deref() == Some("1");
    match subcommand {
        "check" => {
            let offline = input.get("offline").and_then(Value::as_bool).unwrap_or(false);
            let enabled = channel_enabled_from_env(channel, offline);
            let reason = if enabled { "enabled" } else if offline { "offline" } else if env_disabled { "disabled_by_env" } else { "disabled" };
            Ok(json!({
                "schemaVersion": 1,
                "owner": owner,
                "channel": channel,
                "enabled": enabled,
                "currentVersion": "0.2.0",
                "reason": reason,
                "updateCommand": Value::Null,
            }))
        }
        "apply" => {
            if input.get("publicKey").is_some() {
                return Ok(json!({"schemaVersion": 1, "owner": owner, "action": "apply-local-artifact", "ok": false, "reason": "public_key_argument_forbidden"}));
            }
            if input.get("currentVersion").is_some() {
                return Ok(json!({"schemaVersion": 1, "owner": owner, "action": "apply-local-artifact", "ok": false, "reason": "current_version_argument_forbidden"}));
            }
            let local = ["manifest", "artifactDir", "artifactName", "appDir", "priorDir"];
            if local.iter().any(|key| input.get(*key).is_some()) {
                let mut result = apply_local_artifact(input, root)?;
                if let Value::Object(object) = &mut result {
                    object.insert("schemaVersion".into(), Value::from(1));
                    object.insert("owner".into(), owner);
                    object.insert("action".into(), Value::from("apply-local-artifact"));
                }
                return Ok(result);
            }
            Ok(json!({"schemaVersion": 1, "owner": owner, "action": "require-signed-manifest", "reason": "GitHub Release updates require a signed manifest and matching checksum"}))
        }
        "rollback" => Ok(json!({"schemaVersion": 1, "owner": owner, "action": "rollback", "note": "rollback restores the prior app version and compatible store backup"})),
        other => Err(BlueprintError::new("usage", format!("blueprint update {other} is not a known subcommand"))),
    }
}

fn handle_rollback(input: &Value, root: &Path) -> Result<Value, BlueprintError> {
    let receipt_value = input.get("rollback").cloned().unwrap_or(Value::Null);
    let receipt = RollbackReceipt {
        current_app_digest: receipt_value.get("currentAppDigest").and_then(Value::as_str).unwrap_or("").to_string(),
        prior_app_digest: receipt_value.get("priorAppDigest").and_then(Value::as_str).unwrap_or("").to_string(),
        current_package_version: receipt_value.get("currentPackageVersion").and_then(Value::as_str).unwrap_or("").to_string(),
        prior_package_version: receipt_value.get("priorPackageVersion").and_then(Value::as_str).unwrap_or("").to_string(),
    };
    if !receipt_is_self_consistent(&receipt) {
        return Ok(json!({"ok": false, "reason": "rollback_receipt_invalid"}));
    }
    let prior_app_dir = receipt_value.get("priorAppDir").and_then(Value::as_str);
    let Some(prior_app_dir) = prior_app_dir else {
        return Ok(json!({"ok": false, "reason": "rollback_receipt_missing_prior_app_dir"}));
    };
    let prior_path = Path::new(prior_app_dir);
    if !prior_path.is_dir() {
        return Ok(json!({"ok": false, "reason": "rollback_prior_app_dir_missing"}));
    }
    let current_app_dir = receipt_value.get("appDir").and_then(Value::as_str)
        .map(Path::new).unwrap_or(root);
    if let Err(reason) = validate_rollback_binding(root, current_app_dir, prior_path, &receipt) {
        return Ok(json!({"ok": false, "reason": reason}));
    }
    if current_app_dir != root && current_app_dir.exists() {
        fs::remove_dir_all(current_app_dir)
            .map_err(|error| BlueprintError::new("blueprint_rollback_failed", error.to_string()))?;
    }
    copy_recursive(prior_path, current_app_dir)
        .map_err(|error| BlueprintError::new("blueprint_rollback_failed", error.to_string()))?;
    let restored_digest = tree_digest(current_app_dir)
        .map_err(|error| BlueprintError::new("blueprint_rollback_failed", error))?;
    if restored_digest != receipt.prior_app_digest {
        return Ok(json!({"ok": false, "reason": "rollback_target_digest_mismatch"}));
    }
    Ok(json!({
        "ok": true,
        "rolledBack": true,
        "priorAppDigest": receipt.prior_app_digest,
        "priorPackageVersion": receipt.prior_package_version,
    }))
}

/// Mirrors `case "update"` in `commands.mjs`: channel-gate, verify the
/// signed manifest and reject downgrades, then either return the plan
/// (`--dry-run`) or back up the live store and stage the artifact copy,
/// reporting the atomic swap as a deferred omission. `--rollback` (a
/// self-consistent receipt naming `priorAppDir`) restores files directly
/// instead of running the normal update path.
pub fn execute_update(request: &BlueprintRequest, root: &Path) -> Result<Value, BlueprintError> {
    let input = &request.input;

    // `blueprint update`'s facade accepts check/apply/rollback positional
    // subcommands. The bounded operation also accepts those through the
    // parser's `args` field; direct operation callers with no args retain
    // the deterministic staging path below used by native tests.
    if let Some(subcommand) = positional_subcommand(input) {
        return execute_cli_subcommand(input, root, subcommand);
    }

    // CLI's omitted subcommand defaults to `check`; direct operation callers
    // that provide update fields retain normal staging semantics.
    let has_update_fields = input.as_object()
        .map(|object| object.keys().any(|key| key != "repoRoot"))
        .unwrap_or(false);
    if !has_update_fields {
        return execute_cli_subcommand(input, root, "check");
    }

    if input.get("rollback").is_some() {
        return handle_rollback(input, root);
    }

    let channel = str_field(input, "channel", "stable");
    let offline = input.get("offline").and_then(Value::as_bool).unwrap_or(false);
    let dry_run = input.get("dryRun").and_then(Value::as_bool).unwrap_or(false);

    if !channel_enabled_from_env(channel, offline) {
        return Ok(json!({"ok": false, "reason": "channel_disabled", "channel": channel}));
    }

    let manifest = input.get("manifest");
    let mut manifest_verified = false;
    if let Some(manifest) = manifest {
        let trusted_keys = parse_trusted_update_keys(TRUSTED_UPDATE_KEYS_JSON)
            .map_err(|error| BlueprintError::new("blueprint_update_trust_root_invalid", error))?;
        // Validate complete V1 shape before trust lookup/cryptographic work.
        // Unknown keys still produce legacy's precise trust error once shape
        // is known to be a valid manifest.
        let algorithm = manifest.get("signatureAlgorithm").and_then(Value::as_str);
        let key_id = manifest.get("keyId").and_then(Value::as_str);
        if validate_update_manifest(manifest).is_err() {
            return Ok(json!({"ok": false, "reason": "invalid_update_manifest"}));
        }
        let unknown_key = algorithm == Some("Ed25519")
            && key_id.is_some()
            && (trusted_keys.is_empty() || !trusted_keys.contains_key(key_id.unwrap()));
        if unknown_key {
            return Ok(json!({"ok": false, "reason": "untrusted_key_id"}));
        }
        let verification = verify_signed_manifest(manifest, Some(&trusted_keys));
        match verification {
            SignatureCheck::Ok => manifest_verified = true,
            SignatureCheck::Reason(reason) => {
                return Ok(json!({"ok": false, "reason": reason}));
            }
        }
        let Some(candidate) = manifest.get("version").and_then(Value::as_str) else {
            return Ok(json!({"ok": false, "reason": "invalid_update_manifest"}));
        };
        let Some(current) = input.get("currentVersion").and_then(Value::as_str) else {
            return Ok(json!({"ok": false, "reason": "current_version_missing"}));
        };
        if let Err(reason) = reject_downgrade(candidate, current) {
            return Ok(json!({"ok": false, "reason": reason}));
        }
    }

    if dry_run {
        return Ok(json!({
            "ok": true,
            "dryRun": true,
            "channel": channel,
            "manifestVerified": manifest_verified,
        }));
    }

    let backup = backup_store(root, ".agent");
    if let Some(error) = &backup.error {
        return Ok(json!({"ok": false, "reason": "backup_failed", "detail": error}));
    }

    let mut staged = false;
    if let Some(artifact_dir) = input.get("artifactDir").and_then(Value::as_str) {
        let staging = root.join(".agent").join("update-staged");
        if staging.exists() {
            fs::remove_dir_all(&staging)
                .map_err(|error| BlueprintError::new("blueprint_update_stage_failed", error.to_string()))?;
        }
        if manifest_verified {
            let Some(artifact_name) = input.get("artifactName").and_then(Value::as_str) else {
                return Ok(json!({"ok": false, "reason": "artifact_name_missing"}));
            };
            if let Err(reason) = validate_local_artifact(
                manifest.expect("manifest verified"), Path::new(artifact_dir), artifact_name, root, root,
            ) {
                return Ok(json!({"ok": false, "reason": reason}));
            }
        }
        copy_recursive(Path::new(artifact_dir), &staging)
            .map_err(|error| BlueprintError::new("blueprint_update_stage_failed", error.to_string()))?;
        if manifest_verified {
            let expected = tree_digest(&staging).map_err(|error| BlueprintError::new("blueprint_update_stage_failed", error))?;
            let artifact = selected_artifact(manifest.expect("manifest verified"), input.get("artifactName").and_then(Value::as_str).unwrap_or(""));
            if artifact.and_then(|entry| entry.get("sha256")).and_then(Value::as_str) != Some(expected.as_str()) {
                let _ = fs::remove_dir_all(&staging);
                return Ok(json!({"ok": false, "reason": "staging_checksum_mismatch"}));
            }
        }
        staged = true;
    }

    Ok(json!({
        "ok": true,
        "dryRun": false,
        "channel": channel,
        "manifestVerified": manifest_verified,
        "backup": {
            "backedUp": backup.backed_up,
            "path": backup.path.map(|p| p.to_string_lossy().to_string()),
        },
        "staged": staged,
        "omissions": if staged { json!([{
            "code": "orchestration_out_of_scope",
            "detail": "The atomic app/store swap and journal recovery are not performed by the bounded native operation; the artifact was staged and the live store was backed up, but the swap into place is deferred.",
        }]) } else { json!([]) },
    }))
}

#[allow(dead_code)]
fn is_digest(value: &str) -> bool {
    is_valid_digest(value)
}
