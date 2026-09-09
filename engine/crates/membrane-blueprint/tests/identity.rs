use membrane_blueprint::identity::*;
use tempfile::tempdir;

#[test]
fn origin_forms_normalize_to_same_identity_components() {
    assert_eq!(normalize_git_origin("https://github.com/acme/widgets.git"), normalize_git_origin("git@github.com:acme/widgets.git"));
    assert_eq!(normalize_git_origin("ssh://git@github.com/acme/widgets.git"), normalize_git_origin("git://github.com/acme/widgets.git"));
    assert_eq!(normalize_git_origin("https://gitlab.com/group/sub/repo.git").unwrap().owner, "group/sub");
}

#[test]
fn repository_id_is_remote_stable_but_root_remains_distinct() {
    let a=tempdir().unwrap(); let b=tempdir().unwrap();
    std::fs::create_dir_all(a.path().join(".git")).unwrap(); std::fs::create_dir_all(b.path().join(".git")).unwrap();
    let cfg="[remote \"origin\"]\n\turl = https://example.com/acme/widgets.git\n";
    std::fs::write(a.path().join(".git/config"),cfg).unwrap(); std::fs::write(b.path().join(".git/config"),cfg).unwrap();
    let ia=repository_identity(a.path()).unwrap(); let ib=repository_identity(b.path()).unwrap();
    assert_eq!(ia.repo_id, ib.repo_id); assert_ne!(ia.repo_root, ib.repo_root); assert_eq!(ia.origin_repo.as_deref(),Some("widgets"));
    assert!(ia.repo_id.starts_with("xxh128:") && ia.repo_id.len() == 39);
}

#[test]
fn repository_identity_reads_linked_worktree_common_config_without_git() {
    let root = tempdir().unwrap();
    let common = tempdir().unwrap();
    let worktree_git_dir = common.path().join(".git/worktrees/fixture");
    std::fs::create_dir_all(&worktree_git_dir).unwrap();
    std::fs::write(
        common.path().join(".git/config"),
        "[remote \"origin\"]\n\turl = ssh://git@example.com/acme/widgets.git\n",
    )
    .unwrap();
    std::fs::write(
        root.path().join(".git"),
        format!("gitdir: {}\n", worktree_git_dir.display()),
    )
    .unwrap();

    let identity = repository_identity(root.path()).unwrap();
    assert_eq!(identity.origin_host.as_deref(), Some("example.com"));
    assert_eq!(identity.origin_owner.as_deref(), Some("acme"));
    assert_eq!(identity.origin_repo.as_deref(), Some("widgets"));
    assert_eq!(identity.repo_id, "xxh128:ea4264e0b9fb3822c6acb7ba0c5df81e");
}

#[test]
fn local_identity_uses_installation_id_and_typed_unavailable_root_errors() {
    let dir = tempdir().unwrap();
    let with_installation = repository_identity_with_options(
        dir.path(),
        &IdentityOptions { installation_id: Some("install-1".into()), local_repo_id: None },
    ).unwrap();
    let with_same_installation = repository_identity_with_options(
        dir.path(),
        &IdentityOptions { installation_id: Some("install-1".into()), local_repo_id: None },
    ).unwrap();
    assert_eq!(with_installation.repo_id, with_same_installation.repo_id);
    assert_eq!(with_installation.installation_id.as_deref(), Some("install-1"));
    assert!(matches!(repository_identity("identity-test-path-does-not-exist"), Err(IdentityError::RootUnavailable)));
}

#[test]
fn git_observation_is_typed_unavailable_when_git_state_cannot_be_read() {
    let dir = tempdir().unwrap();
    assert_eq!(git_source_observation(dir.path()), None);
}

#[test]
fn generation_and_manifest_digests_are_stable() {
    let nodes=serde_json::json!([]); let edges=serde_json::json!([]);
    let nodes=serde_json::to_string(&nodes).unwrap(); let edges=serde_json::to_string(&edges).unwrap();
    assert_eq!(compute_generation_id(&nodes,&edges,Some("source")),compute_generation_id(&nodes,&edges,Some("source")));
}

#[test]
fn authority_digest_vectors_match_js() {
    assert_eq!(content_digest(b"abc"), "xxh128:06b05ab6733a618578af5f94892f3950");
    let nodes=serde_json::json!([{"id":"file:a.ts","kind":"file","path":"a.ts"}]);
    let edges=serde_json::json!([{"id":"e","kind":"IMPORTS","source":"file:a.ts","target":"file:b.ts"}]);
    assert_eq!(compute_generation_id(&serde_json::to_string(&nodes).unwrap(),&serde_json::to_string(&edges).unwrap(),Some("xxh128:source")), "xxh128:7e181cc7eb09c7f33e41de468adcd7e2");
    let manifest=serde_json::json!({"schemaVersion":1,"provider":"blueprint-static","providerComposition":[{"id":"lexical","version":"1"}],"counts":{"files":1,"symbols":0,"edges":1},"repo":{"rootName":"repo","sourceHash":"xxh128:source","fileCount":1},"sourceObservation":{"head":"0123456789012345678901234567890123456789","dirty":false,"statusDigest":"abc"}});
    assert_eq!(compute_manifest_digest_value(&manifest,None), "sha256:fb6f79d61b7361271d5853d91f5dd2139f2f53645dbcd2669e2ebd3d999d74a8");
}

#[test]
fn generation_digest_preserves_serializer_field_order_like_json_stringify() {
    #[derive(serde::Serialize)]
    struct NonLexicalNode { z: u8, a: u8 }
    let nodes = [NonLexicalNode { z: 1, a: 2 }];
    let nodes = serde_json::to_string(&nodes).unwrap();
    assert_eq!(
        compute_generation_id(&nodes, "[]", Some("x")),
        "xxh128:76dcc5edf2967042938178138e13973d",
    );
}
