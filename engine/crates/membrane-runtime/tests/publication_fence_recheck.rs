//! Production publication-fence producer coverage.
//!
//! These tests exercise the grant-owner comparison used by both native
//! federation routes without accepting a caller-supplied verdict.

use membrane_protocol::{PublicationFenceChangeV1, PublicationFenceStatusV1};
use membrane_runtime::pull::federation::{
    fence_packet_emission, publication_fence_for_observations, PublicationGrantObservation,
};

fn observation(policy_epoch: &str, revoked: bool) -> PublicationGrantObservation {
    PublicationGrantObservation {
        grant_id: "sgv1-held".to_owned(),
        policy_epoch: policy_epoch.to_owned(),
        revoked,
    }
}

#[test]
fn held_grant_emits_normally() {
    let admitted = observation("epoch-1", false);
    let current = observation("epoch-1", false);
    let fence = publication_fence_for_observations(Some(&admitted), Some(&current))
        .expect("fence comparison should succeed")
        .expect("bound grant produces a fence");

    assert_eq!(fence.status, PublicationFenceStatusV1::Held);
    assert!(fence_packet_emission(Some(fence)).is_ok());
}

#[test]
fn grant_epoch_or_revocation_change_is_typed_and_emits_no_packet() {
    let admitted = observation("epoch-1", false);

    for (current, expected) in [
        (observation("epoch-2", false), PublicationFenceChangeV1::PolicyEpoch),
        (observation("epoch-1", true), PublicationFenceChangeV1::Revocation),
        (
            PublicationGrantObservation {
                grant_id: "sgv1-replaced".to_owned(),
                policy_epoch: "epoch-1".to_owned(),
                revoked: false,
            },
            PublicationFenceChangeV1::GrantIdentity,
        ),
    ] {
        let fence = publication_fence_for_observations(Some(&admitted), Some(&current))
            .expect("fence comparison should succeed")
            .expect("bound grant produces a fence");
        assert_eq!(fence.status, PublicationFenceStatusV1::PolicyChanged);
        assert_eq!(fence.change, Some(expected));
        assert!(fence_packet_emission(Some(fence)).is_err());
    }
}

#[test]
fn scope_free_request_without_bound_grant_is_typed_no_op() {
    let fence = publication_fence_for_observations(None, None)
        .expect("scope-free fence comparison should succeed");
    assert!(fence.is_none());
    assert!(fence_packet_emission(None).is_ok());
}
