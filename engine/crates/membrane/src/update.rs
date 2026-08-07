use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
pub const UPDATE_RECEIPT_SCHEMA_VERSION: u32 = 1;
pub const UPDATE_RECEIPT_FILE: &str = "update-receipt.json";
pub const UPDATE_HOOK_DEADLINE: Duration = Duration::from_secs(30);
#[derive(Debug, Clone, Copy)]
pub struct UpdateControl {
    deadline: Instant,
}
impl UpdateControl {
    fn new(deadline: Duration) -> Self {
        Self {
            deadline: Instant::now() + deadline,
        }
    }
    pub fn check(&self, cancelled: bool) -> Result<(), String> {
        if cancelled {
            return Err("update cancelled".into());
        }
        if Instant::now() >= self.deadline {
            return Err("update deadline exceeded".into());
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdatePhase {
    Quiesce,
    VerifyStaging,
    Activate,
    MigrateSchema,
    PublishReceipt,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdatePlan {
    pub active: PathBuf,
    pub staging: PathBuf,
    pub receipt_root: PathBuf,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateReceiptV1 {
    pub schema_version: u32,
    pub active: PathBuf,
    pub staging: PathBuf,
    pub phases: Vec<UpdatePhase>,
    pub outcome: &'static str,
    pub rollback: &'static str,
}
pub trait UpdateHooks {
    fn cancelled(&self) -> bool;
    fn quiesce(&mut self, control: &UpdateControl) -> Result<(), String>;
    fn verify_staging(&mut self, staging: &Path, control: &UpdateControl) -> Result<(), String>;
    fn migrate_schema(&mut self, active: &Path, control: &UpdateControl) -> Result<(), String>;
    fn rollback_schema(&mut self, _active: &Path) -> Result<(), String> {
        Ok(())
    }
}
pub fn update(
    plan: &UpdatePlan,
    hooks: &mut impl UpdateHooks,
    fault: Option<UpdatePhase>,
) -> Result<UpdateReceiptV1, String> {
    let mut phases = Vec::new();
    let backup = plan.active.with_extension("rollback");
    match std::fs::symlink_metadata(&backup) {
        Ok(_) => {
            return Err(format!(
                "rollback release {} already exists",
                backup.display()
            ))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(format!(
                "inspect rollback release {}: {error}",
                backup.display()
            ))
        }
    }
    if !plan.active.is_dir() || !plan.staging.is_dir() {
        return Err("active and staging releases must be directories".into());
    }
    let control = UpdateControl::new(UPDATE_HOOK_DEADLINE);
    check_phase(UpdatePhase::Quiesce, hooks, fault, &control)?;
    hooks.quiesce(&control)?;
    control.check(hooks.cancelled())?;
    phases.push(UpdatePhase::Quiesce);
    check_phase(UpdatePhase::VerifyStaging, hooks, fault, &control)?;
    hooks.verify_staging(&plan.staging, &control)?;
    control.check(hooks.cancelled())?;
    phases.push(UpdatePhase::VerifyStaging);
    check_phase(UpdatePhase::Activate, hooks, fault, &control)?;
    activate(&plan.active, &plan.staging, &backup)?;
    phases.push(UpdatePhase::Activate);
    let schema_started = check_phase(UpdatePhase::MigrateSchema, hooks, fault, &control).is_ok();
    if let Err(error) = if schema_started {
        hooks.migrate_schema(&plan.active, &control)
    } else {
        check_phase(UpdatePhase::MigrateSchema, hooks, fault, &control)
    } {
        let schema_error = schema_started
            .then(|| hooks.rollback_schema(&plan.active))
            .transpose()
            .err();
        let fs_error = rollback(&plan.active, &plan.staging, &backup).err();
        return Err(join_errors(error, schema_error, fs_error));
    }
    phases.push(UpdatePhase::MigrateSchema);
    if let Err(error) = check_phase(UpdatePhase::PublishReceipt, hooks, fault, &control) {
        let schema_error = hooks.rollback_schema(&plan.active).err();
        let fs_error = rollback(&plan.active, &plan.staging, &backup).err();
        return Err(join_errors(error, schema_error, fs_error));
    }
    phases.push(UpdatePhase::PublishReceipt);
    let receipt = UpdateReceiptV1 {
        schema_version: UPDATE_RECEIPT_SCHEMA_VERSION,
        active: plan.active.clone(),
        staging: plan.staging.clone(),
        phases,
        outcome: "updated",
        rollback: "restore active from .rollback and staging from active",
    };
    if let Err(error) = write_receipt(&plan.receipt_root, &receipt) {
        let schema_error = hooks.rollback_schema(&plan.active).err();
        let fs_error = rollback(&plan.active, &plan.staging, &backup).err();
        return Err(join_errors(error, schema_error, fs_error));
    }
    remove_known_backup(&backup)?;
    Ok(receipt)
}
fn check_phase(
    phase: UpdatePhase,
    hooks: &impl UpdateHooks,
    fault: Option<UpdatePhase>,
    control: &UpdateControl,
) -> Result<(), String> {
    control
        .check(hooks.cancelled())
        .map_err(|error| format!("{error} before {phase:?}"))?;
    if fault == Some(phase) {
        return Err(format!("injected fault at {phase:?}"));
    }
    Ok(())
}
fn remove_known_backup(backup: &Path) -> Result<(), String> {
    let metadata = std::fs::symlink_metadata(backup)
        .map_err(|error| format!("inspect rollback release: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("rollback release is not a real directory".into());
    }
    std::fs::remove_dir_all(backup).map_err(|error| format!("remove rollback release: {error}"))
}
fn activate(active: &Path, staging: &Path, backup: &Path) -> Result<(), String> {
    std::fs::rename(active, backup).map_err(|error| format!("backup active release: {error}"))?;
    if let Err(error) = std::fs::rename(staging, active) {
        let restore = std::fs::rename(backup, active);
        return Err(format!(
            "activate staged release: {error}; restore active: {:?}",
            restore.err()
        ));
    }
    Ok(())
}
fn rollback(active: &Path, staging: &Path, backup: &Path) -> Result<(), String> {
    let mut errors = Vec::new();
    if active.exists() {
        if let Err(error) = std::fs::rename(active, staging) {
            errors.push(format!("restore staging: {error}"));
        }
    }
    if backup.exists() {
        if let Err(error) = std::fs::rename(backup, active) {
            errors.push(format!("restore active: {error}"));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}
fn join_errors(primary: String, extras: Option<String>, fs: Option<String>) -> String {
    [Some(primary), extras, fs]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("; ")
}
fn write_receipt(root: &Path, receipt: &UpdateReceiptV1) -> Result<(), String> {
    std::fs::create_dir_all(root).map_err(|error| format!("create receipt root: {error}"))?;
    let path = root.join(UPDATE_RECEIPT_FILE);
    let tmp = root.join(format!(".{UPDATE_RECEIPT_FILE}.tmp"));
    let bytes =
        serde_json::to_vec(&receipt).map_err(|error| format!("serialize receipt: {error}"))?;
    std::fs::write(&tmp, bytes).map_err(|error| format!("write receipt: {error}"))?;
    std::fs::rename(tmp, path).map_err(|error| format!("activate receipt: {error}"))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Default)]
    struct Hooks {
        cancelled: bool,
        fail_migration: bool,
        rollback_calls: usize,
    }
    impl UpdateHooks for Hooks {
        fn cancelled(&self) -> bool {
            self.cancelled
        }
        fn quiesce(&mut self, control: &UpdateControl) -> Result<(), String> {
            control.check(self.cancelled)?;
            Ok(())
        }
        fn verify_staging(
            &mut self,
            staging: &Path,
            control: &UpdateControl,
        ) -> Result<(), String> {
            control.check(self.cancelled)?;
            if staging.join("release").is_file() {
                Ok(())
            } else {
                Err("unverified staging".into())
            }
        }
        fn migrate_schema(&mut self, active: &Path, control: &UpdateControl) -> Result<(), String> {
            control.check(self.cancelled)?;
            std::fs::write(active.join("schema"), "2").map_err(|error| error.to_string())?;
            if self.fail_migration {
                Err("migration failed".into())
            } else {
                Ok(())
            }
        }
        fn rollback_schema(&mut self, active: &Path) -> Result<(), String> {
            self.rollback_calls += 1;
            std::fs::remove_file(active.join("schema")).map_err(|error| error.to_string())
        }
    }
    fn plan() -> (tempfile::TempDir, UpdatePlan) {
        let temp = tempfile::tempdir().unwrap();
        let active = temp.path().join("active");
        let staging = temp.path().join("staging");
        std::fs::create_dir(&active).unwrap();
        std::fs::create_dir(&staging).unwrap();
        std::fs::write(active.join("release"), "old").unwrap();
        std::fs::write(staging.join("release"), "new").unwrap();
        let plan = UpdatePlan {
            active,
            staging,
            receipt_root: temp.path().join("receipts"),
        };
        (temp, plan)
    }
    fn assert_restored(plan: &UpdatePlan) {
        for (root, expected) in [(&plan.active, "old"), (&plan.staging, "new")] {
            assert_eq!(
                std::fs::read_to_string(root.join("release")).unwrap(),
                expected
            );
        }
    }
    #[test]
    fn every_phase_fault_or_migration_failure_restores_prior_release() {
        for phase in [
            UpdatePhase::Quiesce,
            UpdatePhase::VerifyStaging,
            UpdatePhase::Activate,
            UpdatePhase::MigrateSchema,
            UpdatePhase::PublishReceipt,
        ] {
            let (_temp, plan) = plan();
            let mut hooks = Hooks::default();
            assert!(update(&plan, &mut hooks, Some(phase)).is_err());
            assert_restored(&plan);
            if phase == UpdatePhase::MigrateSchema {
                assert_eq!(hooks.rollback_calls, 0);
            }
        }
        let (_temp, plan) = plan();
        let mut hooks = Hooks {
            fail_migration: true,
            ..Hooks::default()
        };
        assert!(update(&plan, &mut hooks, None).is_err());
        assert_restored(&plan);
        assert!(!plan.active.join("schema").exists());
        assert_eq!(hooks.rollback_calls, 1);
    }
    #[test]
    fn cancellation_prevents_quiescence_or_activation() {
        let (_temp, plan) = plan();
        let mut hooks = Hooks {
            cancelled: true,
            ..Hooks::default()
        };
        assert!(update(&plan, &mut hooks, None)
            .unwrap_err()
            .contains("cancelled"));
        assert_restored(&plan);
        assert!(UpdateControl::new(Duration::ZERO).check(false).is_err());
    }
    #[test]
    fn success_removes_nonempty_backup() {
        let (_temp, plan) = plan();
        let mut hooks = Hooks::default();
        assert!(update(&plan, &mut hooks, None).is_ok());
        assert!(!plan.active.with_extension("rollback").exists());
    }
    #[cfg(unix)]
    #[test]
    fn broken_symlink_backup_refuses_before_side_effects() {
        let (_temp, plan) = plan();
        std::os::unix::fs::symlink("missing", plan.active.with_extension("rollback")).unwrap();
        let mut hooks = Hooks::default();
        assert!(update(&plan, &mut hooks, None)
            .unwrap_err()
            .contains("already exists"));
        assert_restored(&plan);
    }
}
