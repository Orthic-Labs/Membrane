use membrane_federation::providers::live_files::normalize_overlay_path;

#[test]
fn overlay_paths_are_strictly_relative_and_shell_safe() {
    assert_eq!(normalize_overlay_path("src/app.rs"), Some("src/app.rs".into()));
    assert!(normalize_overlay_path("../escape.rs").is_none());
    assert!(normalize_overlay_path("src/app.rs;echo-pwned").is_none());
    assert!(normalize_overlay_path("src/app.rs\nresolver").is_none());
    assert!(normalize_overlay_path(".agent/index.json").is_none());
}

#[test]
fn overlay_path_normalization_is_deterministic() {
    assert_eq!(normalize_overlay_path("src\\nested\\app.rs"), Some("src/nested/app.rs".into()));
    assert!(normalize_overlay_path("/absolute.rs").is_none());
    assert!(normalize_overlay_path("src//app.rs").is_none());
}
