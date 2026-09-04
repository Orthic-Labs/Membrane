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
