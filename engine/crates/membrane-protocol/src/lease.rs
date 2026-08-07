//! Component lease — the v1 protocol shape a supervisor-resident pair exchanges
//! before they are allowed to share a live service. The lease binds the
//! installation identity, the verified build identity, the canonical data
//! root, the validity window, and the operator signature; the admission gate
//! rejects any IPC handshake whose candidate lease diverges from the active
//! lease on installation, release, data root, or expiry.
//!
//! MBR-202: implement exact-build and data-root admission leases.

use serde::{Deserialize, Serialize};

/// Schema version of the component-lease envelope itself. Bumped on
/// incompatible shape changes; the admission gate refuses unknown versions.
pub const COMPONENT_LEASE_SCHEMA_VERSION: u32 = 1;

/// `ComponentLeaseV1` — the typed authority a supervisor hands to its resident
/// child (or that two peer processes exchange) before they are allowed to
/// share a live service. The shape is intentionally narrow: every field is
/// compared byte-for-byte during admission, so adding a new binding requires a
/// new schema version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ComponentLeaseV1 {
    /// Stable installation UUIDv4. Two leases with different installation ids
    /// belong to different Membrane installations on the same machine and
    /// coexist as independent live services.
    pub installation_id: String,
    /// `sha256:<hex>` digest of the verified source tree that produced the
    /// resident build. Two builds of the same installation carry distinct
    /// release generations and must not share a live service.
    pub release_generation: String,
    /// `sha256:<hex>` digest of the canonical data-root path the resident was
    /// configured against. A divergent digest indicates the candidate is
    /// pointing at a stale or foreign data root.
    pub data_root_digest: String,
    /// RFC 3339 UTC instant at which the lease was minted. Diagnostic only;
    /// not compared by the admission gate.
    pub issued_at: String,
    /// RFC 3339 UTC instant at which the lease becomes invalid. A candidate
    /// lease past its expiry cannot be admitted against a freshly issued
    /// active lease.
    pub expires_at: String,
    /// Self-integrity signature over the canonical bytes of the lease. The
    /// shape records it as opaque text; the supervisor and resident verify it
    /// out-of-band before admission.
    pub signature: String,
}
