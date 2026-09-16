//! Native `Operation::Update` handler (lane V1, r5 closure).
//!
//! Composes the previously-ported pure primitives in `lib_update_channel`,
//! `lib_update_manifest`, `lib_update_apply`, and `lib_update_rollback` into
//! one operation, mirroring `blueprint/scripts/cli/commands.mjs` case
//! `"update"`.
//!
//! OWNERSHIP (BPT-058/061/062/063): the canonical Membrane installer owns
//! every update, rollback, and release transaction — including the atomic
//! app/store swap and journal recovery. This handler performs only
//! Blueprint's retained responsibilities: candidate verification (channel
//! gating, signed-manifest verification, downgrade rejection, artifact
//! identity/checksum confinement, receipt-bound rollback validation) plus a
//! read-only graph/schema compatibility report. On success it returns a
//! `delegated` verdict the installer can consume as admission evidence; it
//! never moves, copies, or deletes live app/store state itself.

use crate::api::{BlueprintError, BlueprintRequest};
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
    // The atomic app/store swap is the canonical Membrane installer's
    // transaction. Blueprint's role ends at verification: the candidate is
    // admitted only with a signed manifest, a digest-matched artifact, a
    // non-downgrade version, and a graph/schema compatibility report.
    Ok(json!({
        "ok": true,
        "delegated": true,
        "delegate": "membrane_installer",
        "manifest": manifest,
        "artifact": artifact_name,
        "artifactDigest": digest,
        "priorAppDigest": prior_digest,
        "priorPackageVersion": current_version,
        "version": manifest.get("version"),
        "graphCompatibility": graph_compatibility(repo_root),
    }))
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
                "graphCompatibility": graph_compatibility(root),
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
        "rollback" => Ok(json!({"schemaVersion": 1, "owner": owner, "action": "rollback", "delegated": true, "delegate": "membrane_installer", "note": "the canonical installer restores the prior app version and compatible store backup; Blueprint verifies the receipt-bound digests"})),
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
    // Receipt-bound verification is complete: the prior tree's digest and the
    // current tree's digest both match the receipt. The file-level restore is
    // the canonical Membrane installer's transaction; Blueprint reports the
    // verified binding as delegation evidence instead of swapping trees.
    Ok(json!({
        "ok": true,
        "rolledBack": false,
        "delegated": true,
        "delegate": "membrane_installer",
        "verifiedBinding": {
            "currentAppDigest": receipt.current_app_digest,
            "priorAppDigest": receipt.prior_app_digest,
            "currentPackageVersion": receipt.current_package_version,
            "priorPackageVersion": receipt.prior_package_version,
            "priorAppDir": prior_app_dir,
        },
        "graphCompatibility": graph_compatibility(root),
    }))
}

/// Mirrors `case "update"` in `commands.mjs`: channel-gate, verify the
/// signed manifest and reject downgrades, then either return the plan
/// (`--dry-run`) or return a `delegated` admission verdict for the
/// canonical Membrane installer, which owns the backup/stage/swap
/// transaction. `--rollback` (a self-consistent receipt naming
/// `priorAppDir`) verifies the receipt-bound digests and delegates the
/// restore instead of mutating files itself.
pub fn execute_update(request: &BlueprintRequest, root: &Path) -> Result<Value, BlueprintError> {
    let input = &request.input;

    // `blueprint update`'s facade accepts check/apply/rollback positional
    // subcommands. The bounded operation also accepts those through the
    // parser's `args` field; direct operation callers with update fields
    // take the verify-and-delegate path below.
    if let Some(subcommand) = positional_subcommand(input) {
        return execute_cli_subcommand(input, root, subcommand);
    }

    // CLI's omitted subcommand defaults to `check`; direct operation callers
    // that provide update fields retain normal verify-and-delegate semantics.
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

    // When a verified manifest names a local artifact, run the full
    // read-only admission check (platform/arch, confinement, package
    // identity, tree digest) so the delegated verdict carries verified
    // artifact evidence. An artifact without a verified manifest is an
    // unsafe transition and is refused, never staged.
    let mut artifact_admission = Value::Null;
    if let Some(artifact_dir) = input.get("artifactDir").and_then(Value::as_str) {
        if !manifest_verified {
            return Ok(json!({"ok": false, "reason": "artifact_manifest_unverified"}));
        }
        let Some(artifact_name) = input.get("artifactName").and_then(Value::as_str) else {
            return Ok(json!({"ok": false, "reason": "artifact_name_missing"}));
        };
        match validate_local_artifact(
            manifest.expect("manifest verified"), Path::new(artifact_dir), artifact_name, root, root,
        ) {
            Ok(digest) => {
                artifact_admission = json!({
                    "name": artifact_name,
                    "digest": digest,
                    "platform": current_platform(),
                    "arch": current_arch(),
                });
            }
            Err(reason) => return Ok(json!({"ok": false, "reason": reason})),
        }
    }

    if dry_run {
        return Ok(json!({
            "ok": true,
            "dryRun": true,
            "channel": channel,
            "manifestVerified": manifest_verified,
            "graphCompatibility": graph_compatibility(root),
        }));
    }

    // Verification complete. The backup/stage/swap transaction belongs to
    // the canonical Membrane installer; Blueprint reports the admission
    // decision and graph/schema compatibility, and never mutates live state.
    Ok(json!({
        "ok": true,
        "dryRun": false,
        "channel": channel,
        "manifestVerified": manifest_verified,
        "delegated": true,
        "delegate": "membrane_installer",
        "artifact": artifact_admission,
        "graphCompatibility": graph_compatibility(root),
    }))
}

/// Read-only graph/schema compatibility report for installer admission
/// (BPT-058/061/062/063). Mirrors the eligibility criteria in
/// `engine::verified_construction_reason` — missing store, unreadable
/// store, store schema newer than supported, and generation schema/provider
/// mismatch — but never opens the store writable, never migrates, and never
/// constructs a graph.
fn graph_compatibility(root: &Path) -> Value {
    let db_path = root.join(".agent").join("graph").join("graph.db");
    if !db_path.is_file() {
        return json!({"state": "missing", "compatible": false, "reason": "graph_missing"});
    }
    let connection = match crate::store::open_store_read_only(&db_path) {
        Ok(connection) => connection,
        Err(_) => return json!({"state": "unreadable", "compatible": false, "reason": "unrecoverable_corruption"}),
    };
    let schema_version = match crate::store::current_schema_version(&connection) {
        Ok(version) => version,
        Err(_) => return json!({"state": "unreadable", "compatible": false, "reason": "unrecoverable_corruption"}),
    };
    if schema_version > crate::migrations::SCHEMA_VERSION {
        return json!({
            "state": "unsupported_newer_schema",
            "compatible": false,
            "reason": "blueprint_schema_unsupported",
            "schemaVersion": schema_version,
        });
    }
    let envelope = match crate::store::read_generation_envelope(&connection) {
        Ok(Some(envelope)) => envelope,
        Ok(None) => return json!({"state": "missing", "compatible": false, "reason": "graph_missing", "schemaVersion": schema_version}),
        Err(_) => return json!({"state": "unreadable", "compatible": false, "reason": "unrecoverable_corruption", "schemaVersion": schema_version}),
    };
    let graph_schema = envelope.schema_version.unwrap_or(crate::graph::GRAPH_SCHEMA_VERSION);
    let provider = envelope.provider.as_ref()
        .and_then(|value| value.get("id")).and_then(Value::as_str).unwrap_or("native-rust");
    let provider_version = envelope.provider.as_ref()
        .and_then(|value| value.get("version")).and_then(Value::as_str).unwrap_or(crate::graph::PROVIDER_VERSION);
    if graph_schema != crate::graph::GRAPH_SCHEMA_VERSION
        || provider != "native-rust"
        || provider_version != crate::graph::PROVIDER_VERSION
    {
        return json!({
            "state": "incompatible",
            "compatible": false,
            "reason": "blueprint_generation_incompatible",
            "graphSchema": graph_schema,
            "provider": provider,
            "providerVersion": provider_version,
        });
    }
    let generation_id = envelope.manifest.as_ref()
        .and_then(|manifest| manifest.get("generationId")).and_then(Value::as_str);
    if schema_version < crate::migrations::SCHEMA_VERSION {
        return json!({
            "state": "migration_required",
            "compatible": true,
            "schemaVersion": schema_version,
            "generationId": generation_id,
        });
    }
    json!({
        "state": "compatible",
        "compatible": true,
        "schemaVersion": schema_version,
        "generationId": generation_id,
    })
}

#[allow(dead_code)]
fn is_digest(value: &str) -> bool {
    is_valid_digest(value)
}
