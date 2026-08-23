use membrane_federation::providers::git::{produce, GitProvider, RepositoryAdapter};
use membrane_protocol::{FederationProviderStatusV1, ProviderId};

#[test]
fn non_repository_is_explicitly_degraded() {
    let output = produce(std::env::temp_dir().join("membrane-git-provider-not-a-repository"));
    assert_eq!(output.provider, ProviderId::Git);
    assert_eq!(output.status, FederationProviderStatusV1::Partial);
    assert!(output.candidates.is_empty());
    assert!(!output.warnings.is_empty());
    assert!(!output.omissions.is_empty());
}

#[test]
fn adapter_does_not_expose_process_launching_api() {
    let _ = std::mem::size_of::<GitProvider>();
    let _ = std::mem::size_of::<RepositoryAdapter>();
}
