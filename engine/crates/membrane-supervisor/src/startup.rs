//! MBR-208: receipt-backed, opt-in per-user startup planning.

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
    RegisterPerUser {
        entry: String,
    },
    ReconcileLeaseAfterResume {
        entry: String,
        lease_state: &'static str,
    },
    UnregisterOwned {
        entry: String,
    },
}

/// Creates the only supported opt-in receipt. It describes a user-scoped
/// launchd agent or Task Scheduler entry; enterprise service remains an
/// explicitly separate deployment choice and never appears in this receipt.
pub fn opt_in(platform: StartupPlatform, home: &Path) -> Result<StartupReceiptV1, String> {
    let home = canonical_home(home)?;
    let (owner, entry) = match platform {
        StartupPlatform::MacosLaunchAgent => (
            MACOS_LABEL,
            home.join("Library/LaunchAgents")
                .join(format!("{MACOS_LABEL}.plist"))
                .display()
                .to_string(),
        ),
        StartupPlatform::WindowsTask => (WINDOWS_TASK, WINDOWS_TASK.to_string()),
        StartupPlatform::EnterpriseService => {
            return Err("enterprise service requires separate administrator deployment".into())
        }
    };
    Ok(StartupReceiptV1 {
        schema_version: STARTUP_RECEIPT_SCHEMA_VERSION,
        platform,
        enabled: true,
        owner: owner.into(),
        entry,
        requires_elevation: false,
        home_root: home.display().to_string(),
    })
}

fn canonical_home(home: &Path) -> Result<std::path::PathBuf, String> {
    home.canonicalize()
        .map_err(|error| format!("canonical startup home: {error}"))
}

pub fn persist(receipt_root: &Path, home: &Path, receipt: &StartupReceiptV1) -> Result<(), String> {
    if !valid_opt_in(receipt, home) {
        return Err("refusing forged startup receipt".into());
    }
    std::fs::create_dir_all(receipt_root)
        .map_err(|error| format!("create startup receipt root: {error}"))?;
    std::fs::write(
        receipt_root.join(STARTUP_RECEIPT_FILE),
        serde_json::to_vec_pretty(receipt)
            .map_err(|error| format!("serialize startup receipt: {error}"))?,
    )
    .map_err(|error| format!("write startup receipt: {error}"))
}

pub fn load(receipt_root: &Path, home: &Path) -> Result<StartupReceiptV1, String> {
    let receipt: StartupReceiptV1 = serde_json::from_slice(
        &std::fs::read(receipt_root.join(STARTUP_RECEIPT_FILE))
            .map_err(|error| format!("read startup receipt: {error}"))?,
    )
    .map_err(|error| format!("parse startup receipt: {error}"))?;
    valid_opt_in(&receipt, home)
        .then_some(receipt)
        .ok_or_else(|| "unsupported startup receipt schema".into())
}

fn owned_entry(receipt: &StartupReceiptV1, home: &Path) -> bool {
    let Ok(home) = canonical_home(home) else {
        return false;
    };
    if receipt.home_root != home.display().to_string() {
        return false;
    }
    match receipt.platform {
        StartupPlatform::MacosLaunchAgent => {
            if receipt.owner != MACOS_LABEL {
                return false;
            }
            receipt.entry
                == home
                    .join("Library/LaunchAgents")
                    .join("com.membrane.supervisor.plist")
                    .display()
                    .to_string()
        }
        StartupPlatform::WindowsTask => {
            receipt.owner == WINDOWS_TASK && receipt.entry == WINDOWS_TASK
        }
        StartupPlatform::EnterpriseService => false,
    }
}

fn valid_opt_in(receipt: &StartupReceiptV1, home: &Path) -> bool {
    receipt.schema_version == STARTUP_RECEIPT_SCHEMA_VERSION
        && !receipt.requires_elevation
        && owned_entry(receipt, home)
}

pub fn install_operation(receipt: &StartupReceiptV1, home: &Path) -> Option<StartupOperation> {
    (receipt.enabled && valid_opt_in(receipt, home)).then(|| StartupOperation::RegisterPerUser {
        entry: receipt.entry.clone(),
    })
}

/// Resume does not install again: it asks the supervisor to reconcile its
/// existing owned entry and revive its resident/lease state.
pub fn resume_operation(receipt: &StartupReceiptV1, home: &Path) -> Option<StartupOperation> {
    (receipt.enabled && valid_opt_in(receipt, home)).then(|| {
        StartupOperation::ReconcileLeaseAfterResume {
            entry: receipt.entry.clone(),
            lease_state: "resume_pending",
        }
    })
}

/// Uninstall may remove only an enabled receipt with its exact canonical owner.
pub fn uninstall_operation(receipt: &StartupReceiptV1, home: &Path) -> Option<StartupOperation> {
    let owner = match receipt.platform {
        StartupPlatform::MacosLaunchAgent => MACOS_LABEL,
        StartupPlatform::WindowsTask => WINDOWS_TASK,
        StartupPlatform::EnterpriseService => return None,
    };
    (receipt.enabled && receipt.owner == owner && valid_opt_in(receipt, home)).then(|| {
        StartupOperation::UnregisterOwned {
            entry: receipt.entry.clone(),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn per_user_opt_in_persists_and_recovers_after_resume() {
        let temp = tempfile::tempdir().unwrap();
        let receipt = opt_in(StartupPlatform::MacosLaunchAgent, temp.path()).unwrap();
        persist(temp.path(), temp.path(), &receipt).unwrap();
        assert_eq!(load(temp.path(), temp.path()).unwrap(), receipt);
        assert!(matches!(
            install_operation(&receipt, temp.path()),
            Some(StartupOperation::RegisterPerUser { .. })
        ));
        assert!(matches!(
            resume_operation(&receipt, temp.path()),
            Some(StartupOperation::ReconcileLeaseAfterResume {
                lease_state: "resume_pending",
                ..
            })
        ));
    }
    #[test]
    fn uninstall_refuses_unowned_or_enterprise_entries() {
        let temp = tempfile::tempdir().unwrap();
        let mut receipt = opt_in(StartupPlatform::WindowsTask, temp.path()).unwrap();
        assert!(matches!(
            uninstall_operation(&receipt, temp.path()),
            Some(StartupOperation::UnregisterOwned { .. })
        ));
        receipt.owner = "other".into();
        assert!(uninstall_operation(&receipt, temp.path()).is_none());
        assert!(opt_in(StartupPlatform::EnterpriseService, temp.path()).is_err());
    }

    #[test]
    fn rejects_forged_entry_and_schema() {
        let temp = tempfile::tempdir().unwrap();
        let mut receipt = opt_in(StartupPlatform::WindowsTask, temp.path()).unwrap();
        receipt.entry = r"\Other\Task".into();
        assert!(install_operation(&receipt, temp.path()).is_none());
        assert!(resume_operation(&receipt, temp.path()).is_none());
        assert!(uninstall_operation(&receipt, temp.path()).is_none());
        receipt = opt_in(StartupPlatform::WindowsTask, temp.path()).unwrap();
        receipt.schema_version = 99;
        assert!(persist(temp.path(), temp.path(), &receipt).is_err());
        let mut mac = opt_in(StartupPlatform::MacosLaunchAgent, temp.path()).unwrap();
        mac.entry = "/forged/LaunchAgents/com.membrane.supervisor.plist".into();
        assert!(persist(temp.path(), temp.path(), &mac).is_err());
        std::fs::write(
            temp.path().join(STARTUP_RECEIPT_FILE),
            serde_json::to_vec(&mac).unwrap(),
        )
        .unwrap();
        assert!(load(temp.path(), temp.path()).is_err());
    }

    #[test]
    fn all_operations_refuse_elevated_mismatched_disabled_or_forged_mac_receipts() {
        let temp = tempfile::tempdir().unwrap();
        let base = opt_in(StartupPlatform::MacosLaunchAgent, temp.path()).unwrap();
        let mut elevated = base.clone();
        elevated.requires_elevation = true;
        let mut mismatched_home = base.clone();
        mismatched_home.home_root = "/forged/home".into();
        let mut disabled = base.clone();
        disabled.enabled = false;
        let mut forged_entry = base;
        forged_entry.entry = "/forged/LaunchAgents/com.membrane.supervisor.plist".into();
        for (name, receipt, persist_valid) in [
            ("elevated", elevated, false),
            ("mismatched_home", mismatched_home, false),
            ("disabled", disabled, true),
            ("forged_entry", forged_entry, false),
        ] {
            assert!(install_operation(&receipt, temp.path()).is_none(), "{name}");
            assert!(resume_operation(&receipt, temp.path()).is_none(), "{name}");
            assert!(
                uninstall_operation(&receipt, temp.path()).is_none(),
                "{name}"
            );
            assert_eq!(
                persist(temp.path(), temp.path(), &receipt).is_ok(),
                persist_valid,
                "{name}"
            );
        }
    }
}
