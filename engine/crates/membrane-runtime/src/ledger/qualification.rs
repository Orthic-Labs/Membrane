//! Release-bound qualification for automatic Ledger delivery.
//!
//! An earlier FTS benchmark does not qualify a changed service, source-grant
//! boundary, transport, or resolver. Only reviewed release receipts enable
//! the automatic provider. The current branch has check-only evidence.

#[derive(Clone, Copy)]
pub(crate) struct QualifiedDelivery {
    pub release_generation: &'static str,
    pub service_version: &'static str,
    pub resolver_version: &'static str,
    pub projection_version: &'static str,
    pub policy_version: &'static str,
    pub receipt_sha256: &'static str,
}

// Add a receipt only with its managed end-to-end qualification evidence.
const QUALIFIED_DELIVERIES: &[QualifiedDelivery] = &[];

/// The owner/resolver-composition `ledger_fts` activation this build ships,
/// in const-friendly form. `LedgerQualificationReceiptV1` fields are owned
/// `String`s and cannot be constructed in a const context, so the pinned
/// activation is stored as `&'static str` fields and rebuilt at call time;
/// `index::activate` re-derives `receipt_sha256` from those fields, so a
/// hand-edited entry that no longer matches its content address fails closed.
#[derive(Clone, Copy)]
pub(crate) struct QualifiedFtsActivation {
    pub host_id: &'static str,
    pub verifier_id: &'static str,
    pub commit_sha256: &'static str,
    pub corpus_version: &'static str,
    pub corpus_sha256: &'static str,
    pub run_sha256: &'static str,
    pub result_sha256: &'static str,
    pub receipt_sha256: &'static str,
}

/// `None` until a `ledger.qualification-receipt.v1` measured through the
/// production service composition (`membrane_ledger` dispatch into the daemon
/// owner -> `run_read` on the WAL reader -> `query::search` on the persisted
/// activation -> catalog ticket issuance) is recorded under
/// `docs/evidence/qualification/` and its `receipt_sha256` is added to
/// `TRUSTED_LEDGER_FTS_RECEIPTS`. When present, the resident owner treats
/// activation as a host decision: `LedgerService` reconciles the persisted
/// activation row to this receipt at open, and an activation bound to a
/// receipt this build no longer trusts is degraded to `shadow`.
const QUALIFIED_FTS_ACTIVATION: Option<QualifiedFtsActivation> = None;

pub(crate) fn qualified_fts_activation() -> Option<super::index::LedgerQualificationReceiptV1> {
    QUALIFIED_FTS_ACTIVATION.map(|activation| super::index::LedgerQualificationReceiptV1 {
        schema_version: "ledger.qualification-receipt.v1".into(),
        receipt_source: "membrane-host/ledger-qualification".into(),
        host_id: activation.host_id.into(),
        verifier_id: activation.verifier_id.into(),
        commit_sha256: activation.commit_sha256.into(),
        corpus_version: activation.corpus_version.into(),
        corpus_sha256: activation.corpus_sha256.into(),
        run_sha256: activation.run_sha256.into(),
        result_sha256: activation.result_sha256.into(),
        receipt_sha256: activation.receipt_sha256.into(),
    })
}

pub(crate) fn delivery_allowed(release: Option<&str>) -> bool {
    let Some(release) = release else { return false; };
    QUALIFIED_DELIVERIES.iter().any(|entry| {
        entry.release_generation == release
            && entry.service_version == super::service::SERVICE_VERSION
            && entry.resolver_version == super::service::RESOLVER_VERSION
            && entry.projection_version == super::index::PROJECTION_SCHEMA_VERSION
            && entry.policy_version == super::policy::POLICY_VERSION
            && entry.receipt_sha256.len() == 64
            && entry.receipt_sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn old_fts_receipt_does_not_qualify_document_delivery() {
        assert!(!delivery_allowed(None));
        assert!(!delivery_allowed(Some("c7547262dbc5a11109236f8b343b421cd6a248a2447df697483624166978360e")));
    }
}
