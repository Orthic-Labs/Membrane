//! Pull acknowledgement integration over H9/H10 host observations.
//!
//! H10 remains sole acknowledgement authority. Pull only binds it to a
//! pending publication it recorded before host serialization.

use crate::catalog::{
    has_delivery_acknowledgement, has_pending_pull_publication,
    record_delivery_acknowledgement, ContextCatalog, DeliveryAcknowledgementRecordV1,
    PendingPullPublicationV1,
};
use membrane_protocol::{
    LoadedContextIdentitiesV1, ObservationCoverageV1,
    PacketDeliveryAcknowledgementStatusV1, PacketDeliveryAcknowledgementV1,
    RepresentationClassV1, RepresentationHandleV1,
};

/// Canonical handles are JSON encoded in the publication identity so older
/// plain identities remain readable. Once a handle is supplied, all binding,
/// epoch, source and lane invariants are mandatory.
fn canonical_handle_matches(
    catalog: &ContextCatalog,
    publication: &PendingPullPublicationV1,
    loaded: &LoadedContextIdentitiesV1,
) -> bool {
    let identity = publication.representation_identity.trim();
    if !identity.starts_with('{') {
        return true;
    }
    let Some(handle) = canonical_representation_handle(publication) else {
        return false;
    };
    handle.validate().is_ok()
        && handle.class != RepresentationClassV1::MetadataOnly
        && handle.installation_id == catalog.startup_report().catalog_installation_id
        && handle.context_epoch == publication.context_epoch
        && handle.source_ref == publication.source_ref
        && handle.source_digest == publication.representation_digest
        && loaded.identities.value.as_ref().is_some_and(|identities| {
            identities.iter().any(|identity| {
                identity.identity == handle.handle
                    && identity.source_ref == handle.source_ref
                    && identity.source_digest == handle.source_digest
            })
        })
}

fn canonical_representation_handle(
    publication: &PendingPullPublicationV1,
) -> Option<RepresentationHandleV1> {
    let identity = publication.representation_identity.trim();
    identity
        .starts_with('{')
        .then(|| serde_json::from_str::<RepresentationHandleV1>(identity).ok())
        .flatten()
}

fn acknowledgement_record(
    publication: &PendingPullPublicationV1,
    host_serialized_digest: &str,
) -> DeliveryAcknowledgementRecordV1 {
    DeliveryAcknowledgementRecordV1 {
        repository_id: publication.repository_id.clone(),
        request_id: publication.request_id.clone(),
        trace_id: publication.trace_id.clone(),
        task_id: publication.task_id.clone(),
        session_id: publication.session_id.clone(),
        context_epoch: publication.context_epoch.to_string(),
        publication_id: publication.publication_id.clone(),
        representation_digest: publication.representation_digest.clone(),
        packet_digest: publication.packet_digest.clone(),
        host_serialized_digest: host_serialized_digest.to_owned(),
    }
}

fn host_retained(publication: &PendingPullPublicationV1, loaded: &LoadedContextIdentitiesV1) -> bool {
    let expected_identity = canonical_representation_handle(publication)
        .map(|handle| handle.handle)
        .unwrap_or_else(|| publication.representation_identity.trim().to_owned());
    loaded.validate().is_ok()
        && loaded.session_id == publication.session_id
        && matches!(loaded.compaction_generation.coverage, ObservationCoverageV1::Complete)
        && loaded.compaction_generation.value == Some(publication.context_epoch)
        && matches!(loaded.identities.coverage, ObservationCoverageV1::Complete)
        && loaded.identities.value.as_ref().map_or(false, |identities| identities.iter().any(|identity| {
            identity.identity == expected_identity
                && identity.source_ref == publication.source_ref
                && identity.source_digest == publication.representation_digest
        }))
}

#[cfg(test)]
mod tests {
    use super::canonical_representation_handle;
    use crate::catalog::PendingPullPublicationV1;

    fn publication(identity: &str) -> PendingPullPublicationV1 {
        PendingPullPublicationV1 {
            repository_id: "repo".into(), request_id: "request".into(),
            trace_id: "trace".into(), task_id: "task".into(), session_id: "session".into(),
            context_epoch: 1, publication_id: "publication".into(),
            representation_identity: identity.into(), source_ref: "packet://trace".into(),
            representation_digest: "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            packet_digest: "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into(),
        }
    }

    #[test]
    fn canonical_identity_resolves_to_opaque_handle_token() {
        let value = serde_json::json!({
            "schemaVersion": 1, "handle": "opaque-handle", "class": "rendered_full",
            "installationId": "installation", "contextEpoch": 1,
            "sourceRef": "packet://trace",
            "sourceDigest": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        });
        assert_eq!(canonical_representation_handle(&publication(&value.to_string())).unwrap().handle, "opaque-handle");
    }

    #[test]
    fn malformed_canonical_identity_is_rejected() {
        assert!(canonical_representation_handle(&publication("{\"handle\":\"broken\"}")).is_none());
    }
}

/// Accept H10 only for a Pull-recorded pending publication retained in exact
/// H9 host context. Unknown/rejected/mismatched observations fail closed.
pub fn acknowledge_delivery(
    catalog: &ContextCatalog,
    publication: &PendingPullPublicationV1,
    acknowledgement: &PacketDeliveryAcknowledgementV1,
    loaded: &LoadedContextIdentitiesV1,
) -> rusqlite::Result<bool> {
    acknowledgement.validate().map_err(|_| rusqlite::Error::InvalidQuery)?;
    if !has_pending_pull_publication(catalog, publication)?
        || acknowledgement.status != PacketDeliveryAcknowledgementStatusV1::Acknowledged
        || acknowledgement.packet_digest != publication.packet_digest
        || acknowledgement.session_id != publication.session_id
        || acknowledgement.task_id.value.as_deref() != Some(publication.task_id.as_str())
        || !matches!(acknowledgement.task_id.coverage, ObservationCoverageV1::Complete)
        || !matches!(acknowledgement.serialized_bytes.coverage, ObservationCoverageV1::Complete)
        || acknowledgement.serialized_bytes.value.unwrap_or(0) == 0
        || acknowledgement.acknowledged_at_unix_ms == 0
        || loaded.observed_at_unix_ms < acknowledgement.acknowledged_at_unix_ms
        || acknowledgement.host_serialized_digest != loaded.provenance_receipt.receipt_digest
        || acknowledgement.host_serialized_digest
            != acknowledgement.provenance_receipt.receipt_digest
        || !host_retained(publication, loaded)
        || !canonical_handle_matches(catalog, publication, loaded)
    {
        return Err(rusqlite::Error::InvalidQuery);
    }
    record_delivery_acknowledgement(
        catalog,
        &acknowledgement_record(publication, &acknowledgement.host_serialized_digest),
    )
}

/// Catalog reopen preserves records; a restarted/compacted H9 context changes
/// epoch or identities & restores eligibility rather than suppressing proof.
pub fn suppression_eligible(
    catalog: &ContextCatalog,
    publication: &PendingPullPublicationV1,
    loaded: &LoadedContextIdentitiesV1,
) -> rusqlite::Result<bool> {
    Ok(has_pending_pull_publication(catalog, publication)?
        && has_delivery_acknowledgement(catalog, &acknowledgement_record(publication, &loaded.provenance_receipt.receipt_digest))?
        && host_retained(publication, loaded)
        && canonical_handle_matches(catalog, publication, loaded))
}
