//! Native Rust port of `blueprint/src/lib/update/channel.mjs`.
//!
//! Lane LIB3 (r5 closure): no prior native equivalent found (grep of
//! `detectInstallOwner`/`channelEnabled`/`CHANNELS` across
//! membrane-blueprint/src and membrane-runtime/src produced no match).
//! Ported behavior: fixed channel set (`stable`/`beta`/`nightly`); a
//! channel is enabled unless offline or the `BLUEPRINT_NO_UPDATE_CHECK=1`
//! kill-switch is set; install-owner detection is `portable` only when both
//! an embedded-node and packaged-application layout are present under the
//! installation root, otherwise `source`.

use std::path::{Path, PathBuf};

pub const CHANNELS: [&str; 3] = ["stable", "beta", "nightly"];

#[derive(Debug, Clone, PartialEq)]
pub enum InstallOwner {
    Portable,
    Source,
}

#[derive(Debug, Clone, PartialEq)]
pub struct InstallOwnerInfo {
    pub owner: InstallOwner,
    pub root: PathBuf,
}

/// Mirrors `hasPortableLayout(root)`.
pub fn has_portable_layout(root: &Path, exists: impl Fn(&Path) -> bool) -> bool {
    exists(&root.join("lib").join("node")) && exists(&root.join("app").join("package"))
}

/// Mirrors `detectInstallOwner()`, with the installation root and existence
/// probe injected rather than derived from `import.meta.url`.
pub fn detect_install_owner(installation_root: &Path, exists: impl Fn(&Path) -> bool) -> InstallOwnerInfo {
    if has_portable_layout(installation_root, exists) {
        InstallOwnerInfo {
            owner: InstallOwner::Portable,
            root: installation_root.to_path_buf(),
        }
    } else {
        InstallOwnerInfo {
            owner: InstallOwner::Source,
            root: installation_root.to_path_buf(),
        }
    }
}

/// Mirrors `channelEnabled(channel, { offline })`. `update_checks_disabled`
/// mirrors reading `process.env.BLUEPRINT_NO_UPDATE_CHECK === "1"`.
pub fn channel_enabled(channel: &str, offline: bool, update_checks_disabled: bool) -> bool {
    if offline {
        return false;
    }
    if update_checks_disabled {
        return false;
    }
    CHANNELS.contains(&channel)
}

/// Read the process kill-switch used by the legacy channel implementation.
/// Callers must not accept this as request input: it is host policy, not user
/// data.
pub fn update_checks_disabled() -> bool {
    std::env::var("BLUEPRINT_NO_UPDATE_CHECK").is_ok_and(|value| value == "1")
}

/// Channel gate using authoritative host policy rather than caller-provided
/// flags.
pub fn channel_enabled_from_env(channel: &str, offline: bool) -> bool {
    channel_enabled(channel, offline, update_checks_disabled())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn portable_layout_requires_both_embedded_node_and_packaged_app() {
        let root = Path::new("/opt/blueprint");
        let only_node: HashSet<PathBuf> = [root.join("lib").join("node")].into_iter().collect();
        assert!(!has_portable_layout(root, |p| only_node.contains(p)));

        let both: HashSet<PathBuf> = [
            root.join("lib").join("node"),
            root.join("app").join("package"),
        ]
        .into_iter()
        .collect();
        assert!(has_portable_layout(root, |p| both.contains(p)));
    }

    #[test]
    fn detect_install_owner_picks_portable_or_source() {
        let root = Path::new("/opt/blueprint");
        let portable = detect_install_owner(root, |_| true);
        assert_eq!(portable.owner, InstallOwner::Portable);

        let source = detect_install_owner(root, |_| false);
        assert_eq!(source.owner, InstallOwner::Source);
    }

    #[test]
    fn channel_enabled_respects_offline_and_kill_switch_and_channel_set() {
        assert!(channel_enabled("stable", false, false));
        assert!(!channel_enabled("stable", true, false));
        assert!(!channel_enabled("stable", false, true));
        assert!(!channel_enabled("bogus", false, false));
    }
}
