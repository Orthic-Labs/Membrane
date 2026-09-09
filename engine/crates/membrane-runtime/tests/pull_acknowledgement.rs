use membrane_protocol::{
    HostObservationProvenanceV1, LoadedContextIdentitiesV1, LoadedContextIdentityV1,
    ObservedFieldV1, PacketDeliveryAcknowledgementStatusV1,
    PacketDeliveryAcknowledgementV1, LOADED_CONTEXT_IDENTITIES_SCHEMA_VERSION,
    PACKET_DELIVERY_ACKNOWLEDGEMENT_SCHEMA_VERSION,
};
use membrane_runtime::catalog::{
    record_pending_pull_publication, ContextCatalog, PendingPullPublicationV1,
};
use membrane_runtime::adapt_observations::AdaptObservationRequestV1;
use membrane_runtime::pull::delivery_acknowledgement::{acknowledge_delivery, suppression_eligible};

fn digest(byte: char) -> String { format!("sha256:{}", byte.to_string().repeat(64)) }

fn publication() -> PendingPullPublicationV1 {
    PendingPullPublicationV1 {
        repository_id: "repo-1".into(), request_id: "request-1".into(), trace_id: "trace-1".into(),
        task_id: "task-1".into(), session_id: "session-1".into(), context_epoch: 7,
        publication_id: "publication-1".into(), representation_identity: "evidence-1".into(),
        source_ref: "source-1".into(), representation_digest: digest('a'), packet_digest: digest('b'),
    }
}

fn provenance() -> HostObservationProvenanceV1 {
    HostObservationProvenanceV1::new("receipt-1", "coderight", 10, digest('c'))
}

fn loaded(publication: &PendingPullPublicationV1, epoch: u64) -> LoadedContextIdentitiesV1 {
    LoadedContextIdentitiesV1 {
        schema_version: LOADED_CONTEXT_IDENTITIES_SCHEMA_VERSION, snapshot_id: "snapshot-1".into(),
        session_id: publication.session_id.clone(), compaction_generation: ObservedFieldV1::complete(epoch),
        identities: ObservedFieldV1::complete(vec![LoadedContextIdentityV1 {
            identity: publication.representation_identity.clone(), source_ref: publication.source_ref.clone(),
            source_digest: publication.representation_digest.clone(),
        }]), observed_at_unix_ms: 20, provenance_receipt: provenance(),
    }
}

fn acknowledgement(publication: &PendingPullPublicationV1) -> PacketDeliveryAcknowledgementV1 {
    PacketDeliveryAcknowledgementV1 {
        schema_version: PACKET_DELIVERY_ACKNOWLEDGEMENT_SCHEMA_VERSION, acknowledgement_id: "ack-1".into(),
        packet_digest: publication.packet_digest.clone(), host_serialized_digest: publication.representation_digest.clone(),
        session_id: publication.session_id.clone(), task_id: ObservedFieldV1::complete(publication.task_id.clone()),
        status: PacketDeliveryAcknowledgementStatusV1::Acknowledged,
        serialized_bytes: ObservedFieldV1::complete(1), acknowledged_at_unix_ms: 11,
        provenance_receipt: provenance(),
    }
}

#[test]
fn h10_ack_requires_pending_publication_and_exact_host_retention() {
    let catalog = ContextCatalog::open_in_memory();
    let publication = publication();
    let ack = acknowledgement(&publication);
    let retained = loaded(&publication, publication.context_epoch);
    assert!(acknowledge_delivery(&catalog, &publication, &ack, &retained).is_err());
    record_pending_pull_publication(&catalog, &publication).unwrap();
    let mut rejected = ack.clone();
    rejected.status = PacketDeliveryAcknowledgementStatusV1::Rejected;
    assert!(acknowledge_delivery(&catalog, &publication, &rejected, &retained).is_err());
    assert!(acknowledge_delivery(&catalog, &publication, &ack, &retained).unwrap());
    assert!(suppression_eligible(&catalog, &publication, &retained).unwrap());
    let mut wrong = ack.clone();
    wrong.host_serialized_digest = digest('d');
    assert!(acknowledge_delivery(&catalog, &publication, &wrong, &retained).is_err());
    let mut wrong_task = ack;
    wrong_task.task_id = ObservedFieldV1::complete("other-task".into());
    assert!(acknowledge_delivery(&catalog, &publication, &wrong_task, &retained).is_err());
}

#[test]
fn pull_correlation_uses_opaque_camel_case_wire_shape() {
    let publication = publication();
    let request: AdaptObservationRequestV1 = serde_json::from_value(serde_json::json!({
        "operation": "acknowledge",
        "scope": publication.repository_id.clone(),
        "acknowledgement": acknowledgement(&publication),
        "loaded": loaded(&publication, publication.context_epoch),
        "pullPublication": {"schemaVersion": 1, "publicationId": publication.publication_id.clone()},
    }))
    .unwrap();
    assert!(matches!(request, AdaptObservationRequestV1::Acknowledge {
        emission_receipt_id: None,
        pull_publication: Some(_),
        ..
    }));
}


#[test]
fn catalog_reopen_persists_but_host_context_restart_restores_eligibility() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("catalog.db");
    let publication = publication();
    let retained = loaded(&publication, publication.context_epoch);
    let first = ContextCatalog::open(&path).unwrap();
    record_pending_pull_publication(&first, &publication).unwrap();
    acknowledge_delivery(&first, &publication, &acknowledgement(&publication), &retained).unwrap();
    drop(first);
    let reopened = ContextCatalog::open(&path).unwrap();
    assert!(suppression_eligible(&reopened, &publication, &retained).unwrap());
    assert!(!suppression_eligible(&reopened, &publication, &loaded(&publication, 8)).unwrap());
}
