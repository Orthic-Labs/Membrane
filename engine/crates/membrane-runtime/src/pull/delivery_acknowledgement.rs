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
};

fn acknowledgement_record(publication: &PendingPullPublicationV1) -> DeliveryAcknowledgementRecordV1 {
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
    }
}

fn host_retained(publication: &PendingPullPublicationV1, loaded: &LoadedContextIdentitiesV1) -> bool {
    loaded.validate().is_ok()
        && loaded.session_id == publication.session_id
        && matches!(loaded.compaction_generation.coverage, ObservationCoverageV1::Complete)
        && loaded.compaction_generation.value == Some(publication.context_epoch)
        && matches!(loaded.identities.coverage, ObservationCoverageV1::Complete)
        && loaded.identities.value.as_ref().map_or(false, |identities| identities.iter().any(|identity| {
            identity.identity == publication.representation_identity
                && identity.source_ref == publication.source_ref
                && identity.source_digest == publication.representation_digest
        }))
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
        || acknowledgement.host_serialized_digest != publication.representation_digest
        || acknowledgement.session_id != publication.session_id
        || acknowledgement.task_id.value.as_deref() != Some(publication.task_id.as_str())
        || !matches!(acknowledgement.task_id.coverage, ObservationCoverageV1::Complete)
        || !matches!(acknowledgement.serialized_bytes.coverage, ObservationCoverageV1::Complete)
        || acknowledgement.serialized_bytes.value.unwrap_or(0) == 0
        || acknowledgement.acknowledged_at_unix_ms == 0
        || loaded.observed_at_unix_ms < acknowledgement.acknowledged_at_unix_ms
        || !host_retained(publication, loaded)
    {
        return Err(rusqlite::Error::InvalidQuery);
    }
    record_delivery_acknowledgement(catalog, &acknowledgement_record(publication))
}

/// Catalog reopen preserves records; a restarted/compacted H9 context changes
/// epoch or identities & restores eligibility rather than suppressing proof.
pub fn suppression_eligible(
    catalog: &ContextCatalog,
    publication: &PendingPullPublicationV1,
    loaded: &LoadedContextIdentitiesV1,
) -> rusqlite::Result<bool> {
    Ok(has_pending_pull_publication(catalog, publication)?
        && has_delivery_acknowledgement(catalog, &acknowledgement_record(publication))?
        && host_retained(publication, loaded))
}
