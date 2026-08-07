//! MBR-202 acceptance integration tests. Exercises the supervisor's lease
//! admission policy through its public surface so a stale or wrong build
//! cannot share a live service with the active one.
//!
//! These tests compile but **do not run** at task-commit time. The Book 1 gate
//! invokes them along with the rest of the workspace suite.
//!
//! MBR-202: implement exact-build and data-root admission leases.

use membrane_protocol::ComponentLeaseV1;
use membrane_supervisor::admission::{evaluate, AdmissionDecision};

fn sha256_digest(digit: char) -> String {
    format!("sha256:{}", digit.to_string().repeat(64))
}

fn build_lease(
    installation_id: &str,
    release_digit: char,
    digest_digit: char,
    expires_at: &str,
) -> ComponentLeaseV1 {
    ComponentLeaseV1 {
        installation_id: installation_id.to_string(),
        release_generation: sha256_digest(release_digit),
        data_root_digest: sha256_digest(digest_digit),
        issued_at: "2026-08-07T00:00:00Z".to_string(),
        expires_at: expires_at.to_string(),
        signature: "lease-sig-fixture".to_string(),
    }
}

/// Acceptance test #1 — same installation, distinct release generations.
///
/// Two leases for the same installation but different verified builds. The
/// first same-identity lease admits because there is no active prior build.
/// A subsequent same-installation lease with a different `releaseGeneration`
/// is rejected as `RejectOldBuild`: an old build cannot piggy-back onto a
/// freshly issued active lease.
#[test]
fn same_installation_with_different_release_generations_rejects_old_build() {
    let mut active = build_lease(
        "00000000-0000-4000-8000-000000000001",
        '1',
        'a',
        "2099-01-01T00:00:00Z",
    );
    let mut first_renewal = build_lease(
        "00000000-0000-4000-8000-000000000001",
        '1',
        'a',
        "2099-01-01T00:00:00Z",
    );
    let stale_build = build_lease(
        "00000000-0000-4000-8000-000000000001",
        '2',
        'a',
        "2099-01-01T00:00:00Z",
    );

    // First renewal: same identity, same build, same data root, future expiry.
    // Must admit.
    match evaluate(&first_renewal, &active) {
        AdmissionDecision::Admit { .. } => {}
        other => panic!("first same-identity lease should admit, got {other:?}"),
    }

    // Promote the renewal to active, then evaluate the stale build. The stale
    // build shares installation id and data root with the active lease, but
    // its release_generation differs.
    std::mem::swap(&mut active, &mut first_renewal);
    match evaluate(&stale_build, &active) {
        AdmissionDecision::RejectOldBuild {
            observed_release_generation,
            candidate_release_generation,
        } => {
            assert_eq!(observed_release_generation, active.release_generation);
            assert_eq!(candidate_release_generation, stale_build.release_generation);
        }
        other => panic!("expected RejectOldBuild, got {other:?}"),
    }
}

/// Acceptance test #2 — same installation and build, divergent data root.
///
/// A second lease for the same installation and release but a different
/// canonical data root. The first same-build lease admits; the second is
/// rejected as `RejectWrongDataRoot`.
#[test]
fn same_install_and_build_with_divergent_data_root_rejects_wrong_data_root() {
    let mut active = build_lease(
        "00000000-0000-4000-8000-000000000002",
        '3',
        'b',
        "2099-01-01T00:00:00Z",
    );
    let mut first_renewal = build_lease(
        "00000000-0000-4000-8000-000000000002",
        '3',
        'b',
        "2099-01-01T00:00:00Z",
    );
    let wrong_root = build_lease(
        "00000000-0000-4000-8000-000000000002",
        '3',
        'c',
        "2099-01-01T00:00:00Z",
    );

    match evaluate(&first_renewal, &active) {
        AdmissionDecision::Admit { .. } => {}
        other => panic!("first same-build lease should admit, got {other:?}"),
    }

    std::mem::swap(&mut active, &mut first_renewal);
    match evaluate(&wrong_root, &active) {
        AdmissionDecision::RejectWrongDataRoot {
            observed_data_root_digest,
            candidate_data_root_digest,
        } => {
            assert_eq!(observed_data_root_digest, active.data_root_digest);
            assert_eq!(candidate_data_root_digest, wrong_root.data_root_digest);
        }
        other => panic!("expected RejectWrongDataRoot, got {other:?}"),
    }
}

/// Acceptance test #3 — lease past its expiry window.
#[test]
fn expired_lease_rejects_stale_lease() {
    let mut active = build_lease(
        "00000000-0000-4000-8000-000000000003",
        '4',
        'd',
        "2099-01-01T00:00:00Z",
    );
    let mut first_renewal = build_lease(
        "00000000-0000-4000-8000-000000000003",
        '4',
        'd',
        "2099-01-01T00:00:00Z",
    );
    let expired = build_lease(
        "00000000-0000-4000-8000-000000000003",
        '4',
        'd',
        "1999-01-01T00:00:00Z",
    );

    match evaluate(&first_renewal, &active) {
        AdmissionDecision::Admit { .. } => {}
        other => panic!("first same-build lease should admit, got {other:?}"),
    }

    std::mem::swap(&mut active, &mut first_renewal);
    match evaluate(&expired, &active) {
        AdmissionDecision::RejectStaleLease {
            observed_expires_at,
            candidate_expires_at,
        } => {
            assert_eq!(observed_expires_at, active.expires_at);
            assert_eq!(candidate_expires_at, expired.expires_at);
        }
        other => panic!("expected RejectStaleLease, got {other:?}"),
    }
}

/// Acceptance test #4 — different installations coexist.
///
/// Two leases with different `installationId` values represent two distinct
/// Membrane installations on the same machine. Each must admit against the
/// other; they never share a live service.
#[test]
fn different_installations_coexist() {
    let install_a = build_lease(
        "00000000-0000-4000-8000-000000000010",
        '5',
        'e',
        "2099-01-01T00:00:00Z",
    );
    let install_b = build_lease(
        "00000000-0000-4000-8000-000000000011",
        '5',
        'e',
        "2099-01-01T00:00:00Z",
    );
    match evaluate(&install_a, &install_b) {
        AdmissionDecision::Admit { .. } => {}
        other => panic!("distinct install must admit against distinct active, got {other:?}"),
    }
    match evaluate(&install_b, &install_a) {
        AdmissionDecision::Admit { .. } => {}
        other => panic!("distinct install must admit against distinct active, got {other:?}"),
    }
}
