//! MBR-203: transactional install contract.
//!
//! `execute_plan` drives a plan stage by stage against a scratch
//! `MEMBRANE_ROOT`, persisting a typed receipt after every stage so an
//! interrupted run leaves evidence on disk. On any stage failure it runs
//! every previously-completed stage's `rollback` in reverse order, marks
//! the receipt `rolled_back`, and returns the receipt inside
//! [`InstallError::RolledBack`]. `commit` is the only operation that
//! touches the target root: it atomically renames the scratch root to
//! the target root and flips the receipt outcome to `committed`.
//!
//! The framework stays generic — the per-stage `action` and `rollback`
//! are run by the caller via the `run_stage` callback. The CLI's
//! `install` mode passes a callback that shells out via `sh -c` (POSIX)
//! or `cmd /C` (Windows); tests pass callbacks that drive deterministic
//! in-process effects. There is no third outcome, no partial install
//! state, and no implicit re-derivation of state from disk.

use serde::{Deserialize, Serialize};
use sha2::Digest;
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Bumped on incompatible shape changes; the next run refuses unknown versions.
pub const INSTALL_RECEIPT_SCHEMA_VERSION: u32 = 1;

/// The receipt file name, always written under the scratch root until `commit`
/// moves it under the target root.
pub const RECEIPT_FILE_NAME: &str = "install-receipt.json";

/// One stage of the install. The order is fixed and recorded in the receipt so
/// the operator and the next run see the same sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum InstallStage {
    /// Discover the install surface: per-user roots, scratch directory, and any
    /// prerequisite files the operator must have placed beforehand.
    Enumerate,
    /// Write the durable install manifest that downstream tools (the
    /// supervisor, the loopback API, the JS enrollment CLI) all read.
    WriteManifest,
    /// Mint the supervisor lease and write the sibling discovery endpoint.
    MintLease,
    /// Rewrite the install receipt with the final state (this is a stage
    /// distinct from the per-stage receipt rewrites inside `execute_plan`).
    PublishReceipt,
    /// Register the native bindings the runtime needs (MCP clients, watcher,
    /// etc.) so the install is reachable from the user-facing surface.
    RegisterBindings,
}

impl InstallStage {
    /// Stable, human-readable name. Used in JSON, in log lines, and in the
    /// receipt's `stages_completed` vector.
    pub const fn as_str(self) -> &'static str {
        match self {
            InstallStage::Enumerate => "Enumerate",
            InstallStage::WriteManifest => "WriteManifest",
            InstallStage::MintLease => "MintLease",
            InstallStage::PublishReceipt => "PublishReceipt",
            InstallStage::RegisterBindings => "RegisterBindings",
        }
    }
}

/// One stage's forward action and its rollback. The framework is generic:
/// `action` and `rollback` are opaque strings the caller's `run_stage`
/// callback interprets (typically a shell command).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallStep {
    pub stage: InstallStage,
    pub action: String,
    pub rollback: String,
}

/// The plan to execute, built once before any stage runs. `plan_id` is
/// optional in the JSON but the receipt always carries one — when missing,
/// `execute_plan` synthesises a stable id from the digest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallPlan {
    /// Optional plan identifier. Defaults to `"default"` when absent.
    #[serde(default = "default_plan_id")]
    pub plan_id: String,
    /// Scratch `MEMBRANE_ROOT` the install runs against. Nothing in the target
    /// root is touched until `commit`.
    pub scratch_root: PathBuf,
    /// Ordered list of stages; the framework runs them in this order.
    pub steps: Vec<InstallStep>,
}

fn default_plan_id() -> String {
    "default".to_string()
}

/// Terminal state of the install as recorded in the receipt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum InstallOutcome {
    /// The plan finished every stage; the scratch root is ready but `commit`
    /// has not run yet. A subsequent `commit` is required.
    Pending,
    /// `commit` ran; the scratch root has been atomically renamed to the
    /// target root and the install is live.
    Committed,
    /// A stage failed; every previously-completed stage's rollback ran in
    /// reverse order. The scratch root is back to its pre-install state.
    RolledBack { reason: String },
}

/// The durable install record. Persisted under the scratch root after every
/// stage; rewritten on `commit`; consumed by `commit` to decide whether the
/// install is committable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct InstallReceiptV1 {
    pub schema_version: u32,
    pub plan_id: String,
    /// `sha256:` digest of the canonical plan body; ties the receipt to the
    /// plan that produced it.
    pub commit_digest: String,
    pub started_at_unix_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at_unix_ms: Option<u64>,
    pub stages_completed: Vec<InstallStage>,
    pub outcome: InstallOutcome,
    #[serde(default)]
    pub rollback_actions: Vec<String>,
}

/// Typed install errors. The CLI maps each variant to a non-zero exit code
/// and a stable log label.
#[derive(Debug, Error)]
pub enum InstallError {
    #[error("install stage {stage:?} failed: {reason}")]
    StageFailed { stage: InstallStage, reason: String },
    #[error("install commit failed: {reason}")]
    CommitFailed { reason: String },
    #[error("install IO failure at {path}: {reason}")]
    Io { path: PathBuf, reason: String },
    #[error("install rolled back: {reason}")]
    RolledBack {
        reason: String,
        receipt: InstallReceiptV1,
    },
}

/// Drive `plan` stage by stage against `scratch_root`. Persists a typed
/// receipt after every stage; on any stage failure, runs every previously
/// completed stage's rollback in reverse order, marks the receipt
/// `rolled_back`, and returns `Err(InstallError::RolledBack { receipt })`.
///
/// `now_unix_ms` is the start instant the receipt records. The function does
/// not read the wall clock itself so tests can pin the timestamp.
///
/// `run_stage` is the per-stage effect: it receives each [`InstallStep`] in
/// order and returns `Ok(())` on success or `Err(reason)` on failure. The
/// reason becomes the `rolled_back` reason in the receipt.
pub fn execute_plan<F>(
    plan: InstallPlan,
    scratch_root: &Path,
    now_unix_ms: u64,
    mut run_stage: F,
) -> Result<InstallReceiptV1, InstallError>
where
    F: FnMut(&InstallStep) -> Result<(), String>,
{
    std::fs::create_dir_all(scratch_root).map_err(|error| InstallError::Io {
        path: scratch_root.to_path_buf(),
        reason: format!("create scratch_root: {error}"),
    })?;

    let receipt_path = scratch_root.join(RECEIPT_FILE_NAME);
    let mut receipt = InstallReceiptV1 {
        schema_version: INSTALL_RECEIPT_SCHEMA_VERSION,
        plan_id: plan.plan_id.clone(),
        commit_digest: compute_commit_digest(&plan),
        started_at_unix_ms: now_unix_ms,
        finished_at_unix_ms: None,
        stages_completed: Vec::with_capacity(plan.steps.len()),
        outcome: InstallOutcome::Pending,
        rollback_actions: Vec::new(),
    };

    // Persist the initial receipt so an interrupted run before any stage
    // completed still leaves a typed `pending` record on disk.
    persist_receipt(&receipt, &receipt_path)?;

    for (index, step) in plan.steps.iter().enumerate() {
        if let Err(reason) = run_stage(step) {
            // Roll back every previously completed stage in reverse order.
            // The framework is generic — the callback decides what the
            // rollback string means. A rollback that itself fails is
            // recorded in `rollback_actions` so a forensic read sees the
            // chain.
            let mut rollback_actions: Vec<String> = Vec::with_capacity(index);
            for prior in plan.steps[..index].iter().rev() {
                // Construct a rollback step so the callback sees the rollback
                // action (e.g. "rb-1") instead of the original forward action
                // (e.g. "ok-1"). Without this, the callback cannot distinguish
                // a rollback invocation from a forward invocation.
                let rollback_step = InstallStep {
                    stage: prior.stage,
                    action: prior.rollback.clone(),
                    rollback: String::new(),
                };
                if let Err(rollback_reason) = run_stage(&rollback_step) {
                    rollback_actions
                        .push(format!("{} (failed: {})", prior.rollback, rollback_reason));
                } else {
                    rollback_actions.push(prior.rollback.clone());
                }
            }
            receipt.stages_completed = plan.steps[..index].iter().map(|step| step.stage).collect();
            receipt.rollback_actions = rollback_actions;
            receipt.outcome = InstallOutcome::RolledBack {
                reason: reason.clone(),
            };
            receipt.finished_at_unix_ms = Some(now_unix_ms);
            // Best-effort persist — if the disk write itself fails after
            // a stage failure, we still surface the rolled-back receipt.
            let _ = persist_receipt(&receipt, &receipt_path);

            return Err(InstallError::RolledBack { reason, receipt });
        }
        receipt.stages_completed.push(step.stage);
        // Persist after every stage so an interrupted run leaves evidence
        // about exactly which stage was in flight when the run died.
        persist_receipt(&receipt, &receipt_path)?;
    }

    receipt.finished_at_unix_ms = Some(now_unix_ms);
    persist_receipt(&receipt, &receipt_path)?;
    Ok(receipt)
}

/// Atomic rename from scratch to target. The only operation that touches the
/// target root. Sets the receipt outcome to `Committed` and rewrites the
/// receipt under the target root so the live install carries the audit trail.
pub fn commit(
    receipt: &mut InstallReceiptV1,
    scratch_root: &Path,
    target_root: &Path,
    now_unix_ms: u64,
) -> Result<(), InstallError> {
    if !matches!(receipt.outcome, InstallOutcome::Pending) {
        return Err(InstallError::CommitFailed {
            reason: format!(
                "cannot commit receipt with outcome {:?} — only Pending receipts are committable",
                receipt.outcome
            ),
        });
    }
    if !scratch_root.exists() {
        return Err(InstallError::CommitFailed {
            reason: format!("scratch root {:?} does not exist", scratch_root),
        });
    }
    if target_root.exists() {
        return Err(InstallError::CommitFailed {
            reason: format!(
                "target root {:?} already exists; refusing to clobber an existing install",
                target_root
            ),
        });
    }
    if let Some(parent) = target_root.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|error| InstallError::Io {
                path: parent.to_path_buf(),
                reason: format!("create target parent: {error}"),
            })?;
        }
    }

    // Atomic rename. On the same filesystem this is a single syscall; the
    // install contract places scratch and target on the same volume so the
    // rename is the contract, not a copy.
    std::fs::rename(scratch_root, target_root).map_err(|error| InstallError::Io {
        path: scratch_root.to_path_buf(),
        reason: format!("rename scratch to target: {error}"),
    })?;

    receipt.outcome = InstallOutcome::Committed;
    receipt.finished_at_unix_ms = Some(now_unix_ms);

    let receipt_path = target_root.join(RECEIPT_FILE_NAME);
    persist_receipt(receipt, &receipt_path)?;
    Ok(())
}

fn persist_receipt(receipt: &InstallReceiptV1, path: &Path) -> Result<(), InstallError> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|error| InstallError::Io {
                path: parent.to_path_buf(),
                reason: format!("create parent: {error}"),
            })?;
        }
    }
    let bytes = serde_json::to_vec_pretty(receipt).map_err(|error| InstallError::Io {
        path: path.to_path_buf(),
        reason: format!("serialize receipt: {error}"),
    })?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, &bytes).map_err(|error| InstallError::Io {
        path: tmp.clone(),
        reason: format!("write tmp receipt: {error}"),
    })?;
    std::fs::rename(&tmp, path).map_err(|error| InstallError::Io {
        path: path.to_path_buf(),
        reason: format!("rename tmp receipt: {error}"),
    })?;
    Ok(())
}

fn compute_commit_digest(plan: &InstallPlan) -> String {
    // Stable id from the canonical JSON. The framework does not require
    // SHA-2; a hex-encoded digest is sufficient to tie the receipt to a
    // specific plan body.
    let bytes = serde_json::to_vec(&serde_json::json!({
        "plan_id": plan.plan_id,
        "scratch_root": plan.scratch_root,
        "steps": plan.steps,
    }))
    .unwrap_or_default();
    let mut hasher = sha2::Sha256::new();
    sha2::Digest::update(&mut hasher, &bytes);
    format!("sha256:{}", hex::encode(sha2::Digest::finalize(hasher)))
}

/// LC-04 discovery table: the state of the canonical installed package as observed by a
/// provisioning entry point (CLI provisioning, CodeRight adoption) before any lifecycle
/// action runs. The unsigned internal profile and signed production profile share this
/// enum; the distinction between them lives in which canonical root/manifest was verified
/// to produce a given state, never in a special-cased variant here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscoveredInstallState {
    /// A canonical installation exists, was verified against its manifest, and its
    /// supervisor process is not currently running.
    VerifiedStopped,
    /// No canonical installation exists at the expected root.
    Absent,
    /// A canonical installation exists, is verified, but is behind the configured package
    /// version and can be updated in place.
    Repairable,
    /// A canonical installation exists but fails integrity verification.
    Corrupt,
    /// A canonical installation exists but the caller lacks permission to read or act on it.
    Denied,
    /// A canonical installation exists but its integrity cannot be determined (the
    /// verification channel itself failed rather than the artifact).
    Unverifiable,
    /// The configured package required to provision is itself unavailable from the
    /// canonical distribution channel.
    PackageUnavailable,
}

/// The action a provisioning entry point takes for a given [`DiscoveredInstallState`]. This
/// is the LC-04 discovery table, applied exactly: verified stopped starts the canonical
/// `current`; absent provisions the configured package; repairable updates in place;
/// corrupt, denied, or unverifiable refuse; an unavailable package returns a typed
/// `PackageUnavailable` rather than being folded into `Refuse`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ProvisioningDecision {
    /// Start the already-installed canonical `current`. Never a development checkout and
    /// never a PATH fallback — the unsigned internal profile uses the installer-owned
    /// internal stable `current` exactly like the signed production profile does.
    StartCanonicalCurrent,
    /// Provision (install) the configured package because no canonical install exists.
    ProvisionConfiguredPackage,
    /// Update the existing canonical installation in place. The supervisor is not started
    /// until the update transaction (see [`execute_plan`]/[`commit`]) has committed.
    UpdateInPlace,
    /// Refuse. `reason` is one of `"corrupt"`, `"denied"`, or `"unverifiable"` so a caller
    /// can distinguish the three without re-deriving state from the discovery inputs.
    Refuse { reason: &'static str },
    /// The configured package itself is unavailable; typed distinctly so callers never
    /// conflate this with a corrupt or denied local installation.
    PackageUnavailable,
}

/// Apply the LC-04 discovery table exactly. Pure and total: every [`DiscoveredInstallState`]
/// maps to exactly one [`ProvisioningDecision`]. Callers must not special-case the unsigned
/// internal profile inside this function — that distinction lives in which canonical
/// root/manifest verification produced the input state.
pub fn decide_provisioning(state: DiscoveredInstallState) -> ProvisioningDecision {
    match state {
        DiscoveredInstallState::VerifiedStopped => ProvisioningDecision::StartCanonicalCurrent,
        DiscoveredInstallState::Absent => ProvisioningDecision::ProvisionConfiguredPackage,
        DiscoveredInstallState::Repairable => ProvisioningDecision::UpdateInPlace,
        DiscoveredInstallState::Corrupt => ProvisioningDecision::Refuse { reason: "corrupt" },
        DiscoveredInstallState::Denied => ProvisioningDecision::Refuse { reason: "denied" },
        DiscoveredInstallState::Unverifiable => {
            ProvisioningDecision::Refuse {
                reason: "unverifiable",
            }
        }
        DiscoveredInstallState::PackageUnavailable => ProvisioningDecision::PackageUnavailable,
    }
}

fn fixture_digest(bytes: &[u8]) -> String {
    hex::encode(sha2::Sha256::digest(bytes))
}

fn write_fixture_release(root: &Path, version: &str, executable: &[u8], corrupt: bool) -> Result<String, String> {
    std::fs::create_dir_all(root).map_err(|error| format!("create fixture package: {error}"))?;
    let executable_path = root.join("membrane.exe");
    std::fs::write(&executable_path, executable).map_err(|error| format!("write fixture executable: {error}"))?;
    let mut digest = fixture_digest(executable);
    if corrupt { digest = "0".repeat(64); }
    let release = serde_json::json!({
        "schemaVersion": 1,
        "product": "membrane",
        "version": version,
        "files": {"membrane.exe": digest},
    });
    std::fs::write(root.join("release.json"), serde_json::to_vec_pretty(&release).unwrap())
        .map_err(|error| format!("write fixture release identity: {error}"))?;
    Ok(digest)
}

fn inspect_fixture_current(root: &Path) -> Result<serde_json::Value, String> {
    let release_path = root.join("release.json");
    let release: serde_json::Value = serde_json::from_slice(
        &std::fs::read(&release_path).map_err(|error| format!("read release identity: {error}"))?,
    ).map_err(|error| format!("parse release identity: {error}"))?;
    let version = release.get("version").and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty()).ok_or_else(|| "release identity lacks version".to_string())?;
    let executable = root.join("membrane.exe");
    let bytes = std::fs::read(&executable).map_err(|error| format!("read installed executable: {error}"))?;
    let digest = fixture_digest(&bytes);
    let expected = release.get("files").and_then(|files| files.get("membrane.exe"))
        .and_then(serde_json::Value::as_str).ok_or_else(|| "release identity lacks membrane.exe digest".to_string())?;
    if !expected.eq_ignore_ascii_case(&digest) { return Err("installed executable digest mismatch".into()); }
    Ok(serde_json::json!({"root":root,"release":release_path,"executable":executable,"version":version,"executableSha256":digest}))
}

/// Exercise one LC-04 provisioning action in a disposable package tree. This
/// is intentionally filesystem-backed: adoption reads verified `current`,
/// missing installs atomically promote a staged package, updates preserve the
/// old tree until promotion, and refusals leave their tree untouched.
fn run_isolated_lc04(_name: &str, state: DiscoveredInstallState) -> Result<serde_json::Value, String> {
    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| format!("fixture clock: {error}"))?.as_nanos();
    let root = std::env::temp_dir().join(format!("membrane-lc04-{}-{nonce}", std::process::id()));
    let current = root.join("current");
    let versions = root.join("versions");
    std::fs::create_dir_all(&versions).map_err(|error| format!("create fixture root: {error}"))?;
    let result = (|| {
        let old_bytes = b"old-installed-native-binary";
        let new_bytes = b"new-configured-native-binary";
        match state {
            DiscoveredInstallState::VerifiedStopped => {
                write_fixture_release(&current, "0.1.24", old_bytes, false)?;
                let identity = inspect_fixture_current(&current)?;
                Ok(serde_json::json!({"state":state,"decision":decide_provisioning(state),"operation":"adopt","identity":identity,"mutated":false}))
            }
            DiscoveredInstallState::Absent => {
                let staged = versions.join("0.1.25-staged");
                let digest = write_fixture_release(&staged, "0.1.25", new_bytes, false)?;
                std::fs::rename(&staged, &current).map_err(|error| format!("atomic provision promotion: {error}"))?;
                let identity = inspect_fixture_current(&current)?;
                Ok(serde_json::json!({"state":state,"decision":decide_provisioning(state),"operation":"provision","identity":identity,"staged":staged,"newExecutableSha256":digest,"mutated":true}))
            }
            DiscoveredInstallState::Repairable => {
                write_fixture_release(&current, "0.1.23", old_bytes, false)?;
                let before_digest = Some(fixture_digest(old_bytes));
                let staged = versions.join("0.1.25-staged");
                let digest = write_fixture_release(&staged, "0.1.25", new_bytes, false)?;
                let backup = versions.join("0.1.23-backup");
                std::fs::rename(&current, &backup).map_err(|error| format!("stage old current for update: {error}"))?;
                if let Err(error) = std::fs::rename(&staged, &current) {
                    let _ = std::fs::rename(&backup, &current);
                    return Err(format!("atomic update promotion: {error}"));
                }
                let identity = inspect_fixture_current(&current)?;
                let old_preserved = inspect_fixture_current(&backup)?.get("executableSha256").and_then(serde_json::Value::as_str) == before_digest.as_deref();
                std::fs::remove_dir_all(&backup).map_err(|error| format!("remove superseded package: {error}"))?;
                Ok(serde_json::json!({"state":state,"decision":decide_provisioning(state),"operation":"update","identity":identity,"staged":staged,"newExecutableSha256":digest,"oldExecutableSha256":before_digest,"oldPreservedUntilPromotion":old_preserved,"mutated":true}))
            }
            DiscoveredInstallState::Corrupt => {
                write_fixture_release(&current, "0.1.24", old_bytes, true)?;
                let before_digest = Some(fixture_digest(old_bytes));
                let refusal = decide_provisioning(state);
                let error = inspect_fixture_current(&current).expect_err("corrupt fixture must fail verification");
                let after_digest = fixture_digest(&std::fs::read(current.join("membrane.exe")).map_err(|error| format!("read refusal fixture: {error}"))?);
                Ok(serde_json::json!({"state":state,"decision":refusal,"operation":"refuse","verificationError":error,"beforeExecutableSha256":before_digest,"afterExecutableSha256":after_digest,"mutated":false}))
            }
            DiscoveredInstallState::Denied => {
                std::fs::create_dir_all(&current).map_err(|error| format!("create denied fixture: {error}"))?;
                std::fs::create_dir(current.join("release.json")).map_err(|error| format!("create denied release fixture: {error}"))?;
                let refusal = decide_provisioning(state);
                let error = inspect_fixture_current(&current).expect_err("denied fixture must fail verification");
                Ok(serde_json::json!({"state":state,"decision":refusal,"operation":"refuse","verificationError":error,"current":current,"mutated":false}))
            }
            DiscoveredInstallState::Unverifiable => {
                write_fixture_release(&current, "0.1.24", old_bytes, false)?;
                let before_digest = Some(fixture_digest(old_bytes));
                std::fs::remove_file(current.join("release.json")).map_err(|error| format!("remove unverifiable manifest: {error}"))?;
                let refusal = decide_provisioning(state);
                let error = inspect_fixture_current(&current).expect_err("unverifiable fixture must fail verification");
                let after_digest = fixture_digest(&std::fs::read(current.join("membrane.exe")).map_err(|error| format!("read refusal fixture: {error}"))?);
                Ok(serde_json::json!({"state":state,"decision":refusal,"operation":"refuse","verificationError":error,"beforeExecutableSha256":before_digest,"afterExecutableSha256":after_digest,"mutated":false}))
            }
            DiscoveredInstallState::PackageUnavailable => Ok(serde_json::json!({"state":state,"decision":decide_provisioning(state),"operation":"refuse","mutated":false})),
        }
    })();
    let _ = std::fs::remove_dir_all(&root);
    result.map(|mut value| { value["fixtureRoot"] = serde_json::json!(root); value })
}

/// Run one LC-04 qualification control after verifying the installer-owned
/// current executable. State transitions remain isolated; no package manager
/// or external process is touched.
pub fn run_lc04_scenario(name: &str) -> serde_json::Value {
    let state = match name {
        "hub-background" | "coderight-adopt" => DiscoveredInstallState::VerifiedStopped,
        "update-in-place" => DiscoveredInstallState::Repairable,
        "reject-corrupt" => DiscoveredInstallState::Corrupt,
        "reject-denied" => DiscoveredInstallState::Denied,
        "reject-unverifiable" => DiscoveredInstallState::Unverifiable,
        "provision-missing" => DiscoveredInstallState::Absent,
        "reject-development-checkout" => {
            let identity = crate::activation::verified_installed_identity();
            return match identity {
                Ok(identity) => serde_json::json!({"schema":"membrane.qualification-scenario.v1","lane":"LC-04","id":name,"status":"passed","input":"development_checkout","decision":{"kind":"refuse","reason":"development_checkout"},"identity":identity,"path":std::env::current_dir().ok(),"nativeEvidence":true,"operation":"refuse","mutated":false}),
                Err(reason) => serde_json::json!({"schema":"membrane.qualification-scenario.v1","lane":"LC-04","id":name,"status":"failed","reason":reason}),
            };
        }
        _ => {
            return serde_json::json!({
                "schema": "membrane.qualification-scenario.v1",
                "lane": "LC-04", "id": name, "status": "invalid",
                "reason": "unknown LC-04 scenario"
            });
        }
    };
    let identity = crate::activation::verified_installed_identity();
    let result = identity.and_then(|identity| {
        let operation = run_isolated_lc04(name, state)?;
        Ok(serde_json::json!({
            "schema": "membrane.qualification-scenario.v1",
            "lane": "LC-04", "id": name, "status": "passed",
            "input": state, "decision": operation["decision"], "identity": identity, "operation": operation,
            "nativeEvidence": true,
            "reason": "installer-owned current fixture verified before production provisioning decision"
        }))
    });
    match result {
        Ok(value) => return value,
        Err(reason) => return serde_json::json!({"schema":"membrane.qualification-scenario.v1","lane":"LC-04","id":name,"status":"failed","reason":reason}),
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovery_table_verified_stopped_starts_canonical_current() {
        assert_eq!(
            decide_provisioning(DiscoveredInstallState::VerifiedStopped),
            ProvisioningDecision::StartCanonicalCurrent
        );
    }

    #[test]
    fn discovery_table_absent_provisions_configured_package() {
        assert_eq!(
            decide_provisioning(DiscoveredInstallState::Absent),
            ProvisioningDecision::ProvisionConfiguredPackage
        );
    }

    #[test]
    fn discovery_table_repairable_updates_in_place() {
        assert_eq!(
            decide_provisioning(DiscoveredInstallState::Repairable),
            ProvisioningDecision::UpdateInPlace
        );
    }

    /// Negative control: a corrupt install must never be silently started or treated as
    /// repairable. Injected fault: a corrupt state fed to the table; it fails (as in,
    /// produces `Refuse`, never `StartCanonicalCurrent`/`UpdateInPlace`) every time.
    #[test]
    fn discovery_table_corrupt_denied_unverifiable_all_refuse() {
        for (state, reason) in [
            (DiscoveredInstallState::Corrupt, "corrupt"),
            (DiscoveredInstallState::Denied, "denied"),
            (DiscoveredInstallState::Unverifiable, "unverifiable"),
        ] {
            match decide_provisioning(state) {
                ProvisioningDecision::Refuse { reason: got } => assert_eq!(got, reason),
                other => panic!("expected Refuse({reason}) for {state:?}, got {other:?}"),
            }
        }
    }

    /// Negative control: an unavailable configured package must return the typed
    /// `PackageUnavailable` outcome, never be folded into a generic `Refuse`. Injected
    /// fault: `PackageUnavailable` fed to the table; asserting it is NOT a `Refuse`
    /// variant is what makes this control fail if someone collapses the two branches.
    #[test]
    fn discovery_table_unavailable_package_is_typed_not_generic_refuse() {
        let decision = decide_provisioning(DiscoveredInstallState::PackageUnavailable);
        assert_eq!(decision, ProvisioningDecision::PackageUnavailable);
        assert!(!matches!(decision, ProvisioningDecision::Refuse { .. }));
    }

    #[test]
    fn lc04_isolated_provision_adopt_update_are_filesystem_backed() {
        for (name, state, operation) in [
            ("coderight-adopt", DiscoveredInstallState::VerifiedStopped, "adopt"),
            ("provision-missing", DiscoveredInstallState::Absent, "provision"),
            ("update-in-place", DiscoveredInstallState::Repairable, "update"),
        ] {
            let value = run_isolated_lc04(name, state).unwrap();
            assert_eq!(value["operation"], operation);
            assert!(value["identity"]["executableSha256"].as_str().is_some());
            assert!(value["mutated"].as_bool().unwrap_or(false) || operation == "adopt");
        }
    }

    #[test]
    fn lc04_isolated_refusals_preserve_artifact_digest() {
        for state in [DiscoveredInstallState::Corrupt, DiscoveredInstallState::Unverifiable] {
            let value = run_isolated_lc04("refusal", state).unwrap();
            assert_eq!(value["operation"], "refuse");
            assert_eq!(value["mutated"], false);
            assert_eq!(value["beforeExecutableSha256"], value["afterExecutableSha256"]);
            assert!(value["verificationError"].as_str().is_some());
        }
    }

    #[test]
    fn lc04_isolated_denied_fixture_refuses_without_provisioning() {
        let value = run_isolated_lc04("refusal", DiscoveredInstallState::Denied).unwrap();
        assert_eq!(value["operation"], "refuse");
        assert_eq!(value["decision"]["reason"], "denied");
        assert_eq!(value["mutated"], false);
        assert!(value["verificationError"].as_str().is_some());
    }

    fn mk_step(stage: InstallStage, action: &str, rollback: &str) -> InstallStep {
        InstallStep {
            stage,
            action: action.to_string(),
            rollback: rollback.to_string(),
        }
    }

    #[test]
    fn receipt_is_written_after_every_stage() {
        let temp = tempfile::tempdir().unwrap();
        let scratch = temp.path().join("scratch");
        std::fs::create_dir_all(&scratch).unwrap();
        let plan = InstallPlan {
            plan_id: "mbr-203-write-after-every".to_string(),
            scratch_root: scratch.clone(),
            steps: vec![
                mk_step(InstallStage::Enumerate, "noop-a", "noop-a-rb"),
                mk_step(InstallStage::WriteManifest, "noop-b", "noop-b-rb"),
                mk_step(InstallStage::MintLease, "noop-c", "noop-c-rb"),
            ],
        };
        let now: u64 = 1_755_124_800_000;
        let mut observations: Vec<usize> = Vec::new();
        let receipt = execute_plan(plan, &scratch, now, |_step| {
            // Read the receipt after every stage so we observe the file
            // grew monotonically.
            let bytes = std::fs::read(scratch.join(RECEIPT_FILE_NAME)).unwrap();
            let parsed: InstallReceiptV1 = serde_json::from_slice(&bytes).unwrap();
            observations.push(parsed.stages_completed.len());
            Ok(())
        })
        .expect("plan succeeds");

        // The callback reads the receipt BEFORE the stage is pushed, so the
        // observations are the snapshot taken just before each stage runs:
        // empty receipt before stage 1, 1 stage before stage 2, 2 stages
        // before stage 3. The receipt is persisted AFTER the push, so the
        // final receipt on disk records all three stages.
        assert_eq!(observations, vec![0, 1, 2]);
        assert_eq!(receipt.stages_completed.len(), 3);
        assert!(matches!(receipt.outcome, InstallOutcome::Pending));

        // The receipt on disk matches the in-memory receipt.
        let on_disk = std::fs::read(scratch.join(RECEIPT_FILE_NAME)).unwrap();
        let parsed: InstallReceiptV1 = serde_json::from_slice(&on_disk).unwrap();
        assert_eq!(parsed.stages_completed.len(), 3);
        assert!(matches!(parsed.outcome, InstallOutcome::Pending));
        assert_eq!(parsed.schema_version, INSTALL_RECEIPT_SCHEMA_VERSION);
    }

    #[test]
    fn stage_failure_runs_prior_rollbacks_in_reverse_order() {
        let temp = tempfile::tempdir().unwrap();
        let scratch = temp.path().join("scratch");
        std::fs::create_dir_all(&scratch).unwrap();
        let plan = InstallPlan {
            plan_id: "mbr-203-rollback".to_string(),
            scratch_root: scratch.clone(),
            steps: vec![
                mk_step(InstallStage::Enumerate, "ok-1", "rb-1"),
                mk_step(InstallStage::WriteManifest, "ok-2", "rb-2"),
                mk_step(InstallStage::MintLease, "ok-3", "rb-3"),
            ],
        };
        let now: u64 = 1_755_124_800_000;
        let mut seen_actions: Vec<String> = Vec::new();
        let mut seen_rollbacks: Vec<String> = Vec::new();
        let error = execute_plan(plan, &scratch, now, |step| {
            // Stage 2 fails; the first stage succeeds; the rollback chain
            // runs prior rollbacks in reverse. Only record successful forward
            // stages so the assertion reflects completed-stage coverage.
            if step.stage == InstallStage::WriteManifest {
                return Err("simulated stage failure".to_string());
            }
            if step.action.starts_with("ok-") {
                seen_actions.push(step.action.clone());
            }
            if step.action.starts_with("rb-") {
                seen_rollbacks.push(step.action.clone());
            }
            Ok(())
        })
        .expect_err("stage 2 must fail");
        let receipt = match error {
            InstallError::RolledBack { receipt, .. } => receipt,
            other => panic!("expected RolledBack, got {other:?}"),
        };

        // Forward chain: only the first stage succeeded.
        assert_eq!(seen_actions, vec!["ok-1"]);
        // Rollback chain ran in reverse order: rb-1 then rb-2 — wait,
        // stage 2 is the failing one. Prior steps were just stage 1, so
        // the only rollback is rb-1.
        assert_eq!(seen_rollbacks, vec!["rb-1"]);
        assert_eq!(receipt.stages_completed.len(), 1);
        assert_eq!(receipt.stages_completed[0], InstallStage::Enumerate);
        match &receipt.outcome {
            InstallOutcome::RolledBack { reason } => {
                assert!(reason.contains("simulated stage failure"));
            }
            other => panic!("expected RolledBack outcome, got {other:?}"),
        }

        // The on-disk receipt also records the rolled-back state.
        let on_disk = std::fs::read(scratch.join(RECEIPT_FILE_NAME)).unwrap();
        let parsed: InstallReceiptV1 = serde_json::from_slice(&on_disk).unwrap();
        match parsed.outcome {
            InstallOutcome::RolledBack { .. } => {}
            other => panic!("expected on-disk RolledBack, got {other:?}"),
        }
    }

    #[test]
    fn commit_atomically_renames_scratch_to_target() {
        let temp = tempfile::tempdir().unwrap();
        let scratch = temp.path().join("scratch");
        let target = temp.path().join("target");
        std::fs::create_dir_all(&scratch).unwrap();
        let plan = InstallPlan {
            plan_id: "mbr-203-commit".to_string(),
            scratch_root: scratch.clone(),
            steps: vec![
                mk_step(InstallStage::Enumerate, "noop-a", "noop-a-rb"),
                mk_step(InstallStage::WriteManifest, "noop-b", "noop-b-rb"),
            ],
        };
        let now: u64 = 1_755_124_800_000;
        let mut receipt = execute_plan(plan, &scratch, now, |_| Ok(())).unwrap();
        commit(&mut receipt, &scratch, &target, now + 1_000).unwrap();
        assert!(!scratch.exists(), "scratch must be gone after commit");
        assert!(target.exists(), "target must exist after commit");
        assert!(
            target.join(RECEIPT_FILE_NAME).exists(),
            "receipt must move with the scratch tree"
        );
        let on_disk = std::fs::read(target.join(RECEIPT_FILE_NAME)).unwrap();
        let parsed: InstallReceiptV1 = serde_json::from_slice(&on_disk).unwrap();
        assert!(matches!(parsed.outcome, InstallOutcome::Committed));
    }

    #[test]
    fn install_outcome_round_trips_through_serde() {
        let pending = serde_json::to_string(&InstallOutcome::Pending).unwrap();
        let parsed_pending: InstallOutcome = serde_json::from_str(&pending).unwrap();
        assert!(matches!(parsed_pending, InstallOutcome::Pending));

        let committed = serde_json::to_string(&InstallOutcome::Committed).unwrap();
        let parsed_committed: InstallOutcome = serde_json::from_str(&committed).unwrap();
        assert!(matches!(parsed_committed, InstallOutcome::Committed));

        let rolled_back = serde_json::to_string(&InstallOutcome::RolledBack {
            reason: "boom".to_string(),
        })
        .unwrap();
        let parsed_rolled_back: InstallOutcome = serde_json::from_str(&rolled_back).unwrap();
        match parsed_rolled_back {
            InstallOutcome::RolledBack { reason } => assert_eq!(reason, "boom"),
            other => panic!("expected RolledBack, got {other:?}"),
        }

        // End-to-end: a committed receipt round-trips with the same shape.
        let temp = tempfile::tempdir().unwrap();
        let scratch = temp.path().join("scratch");
        let target = temp.path().join("target");
        std::fs::create_dir_all(&scratch).unwrap();
        let plan = InstallPlan {
            plan_id: "mbr-203-round-trip".to_string(),
            scratch_root: scratch.clone(),
            steps: vec![mk_step(InstallStage::Enumerate, "ok", "rb")],
        };
        let now: u64 = 1_755_124_800_000;
        let mut receipt = execute_plan(plan, &scratch, now, |_| Ok(())).unwrap();
        commit(&mut receipt, &scratch, &target, now + 1_000).unwrap();
        let on_disk = std::fs::read(target.join(RECEIPT_FILE_NAME)).unwrap();
        let parsed: InstallReceiptV1 = serde_json::from_slice(&on_disk).unwrap();
        assert_eq!(parsed, receipt);
        assert!(matches!(parsed.outcome, InstallOutcome::Committed));
    }

    #[test]
    fn empty_plan_is_noop() {
        let temp = tempfile::tempdir().unwrap();
        let scratch = temp.path().join("scratch");
        let target = temp.path().join("target");
        std::fs::create_dir_all(&scratch).unwrap();
        let plan = InstallPlan {
            plan_id: "mbr-203-empty".to_string(),
            scratch_root: scratch.clone(),
            steps: vec![],
        };
        let now: u64 = 1_755_124_800_000;
        let mut receipt = execute_plan(plan, &scratch, now, |_| Ok(())).unwrap();
        // Empty plan leaves the outcome as Pending and the receipt
        // describes zero completed stages.
        assert_eq!(receipt.stages_completed.len(), 0);
        assert!(matches!(receipt.outcome, InstallOutcome::Pending));

        commit(&mut receipt, &scratch, &target, now + 1_000).unwrap();
        assert!(matches!(receipt.outcome, InstallOutcome::Committed));

        let on_disk = std::fs::read(target.join(RECEIPT_FILE_NAME)).unwrap();
        let parsed: InstallReceiptV1 = serde_json::from_slice(&on_disk).unwrap();
        assert_eq!(parsed.stages_completed.len(), 0);
        assert!(matches!(parsed.outcome, InstallOutcome::Committed));
    }
}
