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

/// The `ledger.qualification-receipt.v1` measured through the production
/// service composition (`membrane_ledger` dispatch into the daemon owner ->
/// `run_read` on the WAL reader -> `query::search` on the persisted
/// activation -> catalog ticket issuance), recorded under
/// `docs/evidence/qualification/ledger-metrics.json` with its
/// `receipt_sha256` in `TRUSTED_LEDGER_FTS_RECEIPTS`. The resident owner
/// treats activation as a host decision: `LedgerService` reconciles the
/// persisted activation row to this receipt at open, and an activation bound
/// to a receipt this build no longer trusts is degraded to `shadow`.
const QUALIFIED_FTS_ACTIVATION: Option<QualifiedFtsActivation> =
    Some(QualifiedFtsActivation {
        host_id: "membrane-eval-harness/ledger-service-composition-v1",
        verifier_id: "ledger-service-composition-eval",
        commit_sha256: "3e9889f700cb61d612f4f9f591b7fbd8a0ebca0c02c57921204ff73e2c44fd56",
        corpus_version: "ledger-eval-v1",
        corpus_sha256: "be0421e5790306e237b933fbf040a7ca70e03532826135eb82158e930d05f7af",
        run_sha256: "d51f984aa07771c2132f68885a0d728b8a6ac3679a228b089fce0b0ca85ba4ca",
        result_sha256: "30d56cd00962aec4c58c24a43800c105b6b027f64f0812774b45ea543a724cde",
        receipt_sha256: "a796a687cb1275aa50c2ebd5661f6019ce4243748d57314cf90adc9faad6dbb6",
    });

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
