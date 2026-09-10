//! Parity test for `blueprint/src/lib/init/detect-hosts.mjs`.

use membrane_blueprint::lib_init_detect_hosts::{detect_hosts, DetectHostsError, HOSTS};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[test]
fn hosts_constant_matches_legacy_fixed_set() {
    assert_eq!(HOSTS, ["claude-code", "codex", "cursor", "generic"]);
}

#[test]
fn explicit_flag_wins_over_everything() {
    let result = detect_hosts(Some("codex"), Path::new("/repo"), |_| true, |_| true).unwrap();
    assert_eq!(result, vec!["codex".to_string()]);
}

#[test]
fn explicit_auto_defers_to_detection_precedence() {
    let existing: HashSet<PathBuf> = [PathBuf::from("/repo/.mcp.json")].into_iter().collect();
    let result = detect_hosts(
        Some("auto"),
        Path::new("/repo"),
        |p| existing.contains(p),
        |_| true,
    )
    .unwrap();
    assert_eq!(result, vec!["claude-code".to_string()]);
}

#[test]
fn invalid_explicit_value_is_rejected() {
    let err = detect_hosts(Some("emacs"), Path::new("/repo"), |_| false, |_| false).unwrap_err();
    assert!(matches!(err, DetectHostsError::InvalidHost(_)));
}

#[test]
fn command_probe_only_used_when_no_config_files_found() {
    // Config files present for cursor should win even though a claude
    // command probe would also succeed.
    let existing: HashSet<PathBuf> =
        [PathBuf::from("/repo/.cursor/mcp.json")].into_iter().collect();
    let result = detect_hosts(
        None,
        Path::new("/repo"),
        |p| existing.contains(p),
        |cmd| cmd == "claude",
    )
    .unwrap();
    assert_eq!(result, vec!["cursor".to_string()]);
}

#[test]
fn nothing_detected_falls_back_to_generic() {
    let result = detect_hosts(None, Path::new("/repo"), |_| false, |_| false).unwrap();
    assert_eq!(result, vec!["generic".to_string()]);
}
