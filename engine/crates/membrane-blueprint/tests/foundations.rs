use membrane_blueprint::{
    contracts::{OpenMap, RepositoryManifestV1, ScopeGrantV1},
    identity::{repository_identity_with_options, IdentityOptions},
    migrations::SCHEMA_VERSION,
    store::{current_schema_version, open_store},
};

#[test]
fn public_foundations_compose_without_private_module_access() {
    let root = tempfile::tempdir().unwrap();
    let identity = repository_identity_with_options(
        root.path(),
        &IdentityOptions {
            installation_id: None,
            local_repo_id: Some("foundation-fixture".into()),
        },
    )
    .unwrap();
    assert!(identity.repo_id.starts_with("xxh128:"));

    let store = open_store(None).unwrap();
    assert_eq!(current_schema_version(&store).unwrap(), SCHEMA_VERSION);

    let grant = ScopeGrantV1 {
        task_id: "task-foundation".into(),
        repo_root: identity.repo_root.clone(),
        generation_id: Some("generation-foundation".into()),
        receipt_id: "receipt-foundation".into(),
        paths: vec!["src".into()],
        issued_ms: 1,
        ttl_ms: 1_000,
        signature: "0".repeat(64),
    };
    grant.validate().unwrap();

    let manifest = RepositoryManifestV1 {
        schema_version: 1,
        generation_id: grant.generation_id.clone().unwrap(),
        manifest_digest: "sha256:foundation".into(),
        provider: "lexical".into(),
        complete: true,
        repo: identity.repo_id,
        counts: OpenMap::new(),
        details: None,
        properties: None,
        extensions: None,
    };
    let encoded = serde_json::to_value(manifest).unwrap();
    assert_eq!(encoded["generationId"], "generation-foundation");
    assert_eq!(encoded["repo"].as_str().unwrap().len(), 39);
}
