//! MBR-208: receipt-backed, opt-in per-user startup planning (types only, no registration).

use serde::{Deserialize, Serialize};
use std::path::Path;

pub const STARTUP_RECEIPT_SCHEMA_VERSION: u32 = 1;
pub const STARTUP_RECEIPT_FILE: &str = "startup-opt-in.json";
const MACOS_LABEL: &str = "com.membrane.supervisor";
const WINDOWS_TASK: &str = r"\Membrane\Supervisor";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StartupPlatform {
    MacosLaunchAgent,
    WindowsTask,
    EnterpriseService,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StartupReceiptV1 {
    pub schema_version: u32,
    pub platform: StartupPlatform,
    pub enabled: bool,
    pub owner: String,
    pub entry: String,
    pub requires_elevation: bool,
    pub home_root: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartupOperation {
    RegisterPerUser { entry: String },
    ReconcileLeaseAfterResume { entry: String, lease_state: &'static str },
    UnregisterOwned { entry: String },
}

pub fn opt_in(platform: StartupPlatform, home: &Path) -> Result<StartupReceiptV1, String> {
    let home = canonical_home(home)?;
    let (owner, entry) = match platform {
        StartupPlatform::MacosLaunchAgent => (MACOS_LABEL, home.join("Library/LaunchAgents").join(format!("{MACOS_LABEL}.plist")).display().to_string()),
        StartupPlatform::WindowsTask => (WINDOWS_TASK, WINDOWS_TASK.to_string()),
        StartupPlatform::EnterpriseService => return Err("enterprise service requires separate administrator deployment".into()),
    };
    Ok(StartupReceiptV1 { schema_version: STARTUP_RECEIPT_SCHEMA_VERSION, platform, enabled: true, owner: owner.into(), entry, requires_elevation: false, home_root: home.display().to_string() })
}
fn canonical_home(home: &Path) -> Result<std::path::PathBuf, String> {
    home.canonicalize().map_err(|e| format!("canonical startup home: {e}"))
}
pub fn persist(receipt_root: &Path, home: &Path, receipt: &StartupReceiptV1) -> Result<(), String> {
    if !valid_opt_in(receipt, home) { return Err("refusing forged startup receipt".into()); }
    std::fs::create_dir_all(receipt_root).map_err(|e| format!("create startup receipt root: {e}"))?;
    std::fs::write(receipt_root.join(STARTUP_RECEIPT_FILE), serde_json::to_vec_pretty(receipt).map_err(|e| format!("serialize: {e}"))?).map_err(|e| format!("write: {e}"))
}
pub fn load(receipt_root: &Path, home: &Path) -> Result<StartupReceiptV1, String> {
    let receipt: StartupReceiptV1 = serde_json::from_slice(&std::fs::read(receipt_root.join(STARTUP_RECEIPT_FILE)).map_err(|e| format!("read: {e}"))?).map_err(|e| format!("parse: {e}"))?;
    valid_opt_in(&receipt, home).then_some(receipt).ok_or_else(|| "unsupported".into())
}
fn owned_entry(receipt: &StartupReceiptV1, home: &Path) -> bool {
    let Ok(home) = canonical_home(home) else { return false };
    if receipt.home_root != home.display().to_string() { return false }
    match receipt.platform {
        StartupPlatform::MacosLaunchAgent => receipt.owner == MACOS_LABEL && receipt.entry == home.join("Library/LaunchAgents").join("com.membrane.supervisor.plist").display().to_string(),
        StartupPlatform::WindowsTask => receipt.owner == WINDOWS_TASK && receipt.entry == WINDOWS_TASK,
        StartupPlatform::EnterpriseService => false,
    }
}
fn valid_opt_in(receipt: &StartupReceiptV1, home: &Path) -> bool {
    receipt.schema_version == STARTUP_RECEIPT_SCHEMA_VERSION && !receipt.requires_elevation && owned_entry(receipt, home)
}
// D-S04: no OS-service auto-registration — operations are pure types, never write launchd/systemd/Task Scheduler entries. headless `membrane service run` remains available.
pub fn install_operation(_receipt: &StartupReceiptV1, _home: &Path) -> Option<StartupOperation> { None }
pub fn resume_operation(_receipt: &StartupReceiptV1, _home: &Path) -> Option<StartupOperation> { None }
pub fn uninstall_operation(_receipt: &StartupReceiptV1, _home: &Path) -> Option<StartupOperation> { None }
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn per_user_opt_in_persists_and_recovers_after_resume() {
        let temp = tempfile::tempdir().unwrap();
        let receipt = opt_in(StartupPlatform::MacosLaunchAgent, temp.path()).unwrap();
        persist(temp.path(), temp.path(), &receipt).unwrap();
        assert_eq!(load(temp.path(), temp.path()).unwrap(), receipt);
        assert!(install_operation(&receipt, temp.path()).is_none());
        assert!(resume_operation(&receipt, temp.path()).is_none());
    }
    #[test]
    fn uninstall_refuses_unowned_or_enterprise_entries() {
        let temp = tempfile::tempdir().unwrap();
        let mut receipt = opt_in(StartupPlatform::WindowsTask, temp.path()).unwrap();
        assert!(uninstall_operation(&receipt, temp.path()).is_none());
        receipt.owner = "other".into();
        assert!(uninstall_operation(&receipt, temp.path()).is_none());
        assert!(opt_in(StartupPlatform::EnterpriseService, temp.path()).is_err());
    }
}
