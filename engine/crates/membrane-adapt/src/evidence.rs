//! User-act evidence is owned by native transcript normalization & consumed
//! by Adapt without redefining its wire shape or validation rules.

pub use membrane_transcript::evidence::{
    ActKind, EvidenceClass, EvidenceError, SourceSpan, UserActEvidenceV1,
    USER_ACT_EVIDENCE_SCHEMA,
};
