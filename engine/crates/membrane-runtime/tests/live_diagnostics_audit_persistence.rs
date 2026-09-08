use membrane_runtime::live_diagnostics_service::DiagnosticsService;

#[test]
fn unwritable_audit_sink_returns_result_with_typed_degradation_after_mutation() {
    let root = tempfile::tempdir().unwrap();
    let mut service = DiagnosticsService::with_data_root(root.path().to_path_buf()).unwrap();
    let project = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("diagnostics")).unwrap();
    service.workspace_open("repo-audit", "worktree-audit", Some(project.path().to_str().unwrap())).unwrap();
    service.mutation_begin("repo-audit", "worktree-audit").unwrap();

    std::fs::remove_dir_all(root.path().join("diagnostics")).unwrap();
    tempfile::NamedTempFile::new_in(root.path()).unwrap().persist(root.path().join("diagnostics")).unwrap();

    let result = service.mutation_abort("repo-audit", "worktree-audit").unwrap();
    assert_eq!(result["aborted"], true);
    assert_eq!(result["degradations"][0]["code"], "audit_persistence_unavailable");

    // The owner transition remains correct even though its audit append failed.
    let status = service.workspace_status("repo-audit", "worktree-audit").unwrap();
    assert_eq!(status["repoId"], "repo-audit");
    assert_eq!(status["worktreeId"], "worktree-audit");
    assert_eq!(status["openMutation"], false);
}
