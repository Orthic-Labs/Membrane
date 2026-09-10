//! Parity test for `blueprint/src/lib/update/channel.mjs`.

use membrane_blueprint::lib_update_channel::{
    channel_enabled, detect_install_owner, has_portable_layout, InstallOwner, CHANNELS,
};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[test]
fn channels_match_legacy_fixed_set() {
    assert_eq!(CHANNELS, ["stable", "beta", "nightly"]);
}

#[test]
fn channel_enabled_rejects_offline_and_kill_switch_and_unknown_channels() {
    assert!(channel_enabled("beta", false, false));
    assert!(!channel_enabled("beta", true, false));
    assert!(!channel_enabled("beta", false, true));
    assert!(!channel_enabled("edge", false, false));
}

#[test]
fn portable_layout_needs_both_lib_node_and_app_package() {
    let root = Path::new("/opt/app");
    let node_only: HashSet<PathBuf> = [root.join("lib/node")].into_iter().collect();
    assert!(!has_portable_layout(root, |p| node_only.contains(p)));
    let both: HashSet<PathBuf> = [root.join("lib/node"), root.join("app/package")]
        .into_iter()
        .collect();
    assert!(has_portable_layout(root, |p| both.contains(p)));
}

#[test]
fn install_owner_is_portable_only_with_full_layout_else_source() {
    let root = Path::new("/opt/app");
    assert_eq!(detect_install_owner(root, |_| true).owner, InstallOwner::Portable);
    assert_eq!(detect_install_owner(root, |_| false).owner, InstallOwner::Source);
}
