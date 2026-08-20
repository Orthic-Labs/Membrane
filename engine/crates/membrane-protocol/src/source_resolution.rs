use crate::OverlayIdentityV1;
use serde::{Deserialize, Serialize};
pub const SOURCE_RESOLUTION_RECEIPT_SCHEMA_VERSION: u32 = 1;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceResolutionStatusV1 {
    Resolved,
    Unresolved,
    HashMismatch,
    GenerationMismatch,
    PathMismatch,
    ResolverUnavailable,
    MissingIdentity,
    MissingProvider,
    MissingHash,
    MissingGeneration,
    MissingPath,
}
impl SourceResolutionStatusV1 {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Resolved => "resolved",
            Self::Unresolved => "unresolved",
            Self::HashMismatch => "hash_mismatch",
            Self::GenerationMismatch => "generation_mismatch",
            Self::PathMismatch => "path_mismatch",
            Self::ResolverUnavailable => "resolver_unavailable",
            Self::MissingIdentity => "missing_identity",
            Self::MissingProvider => "missing_provider",
            Self::MissingHash => "missing_hash",
            Self::MissingGeneration => "missing_generation",
            Self::MissingPath => "missing_path",
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceResolutionReceiptV1 {
    pub schema_version: u32,
    pub candidate_id: String,
    pub provider: String,
    pub status: SourceResolutionStatusV1,
    pub expected_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_hash: Option<String>,
    pub expected_generation: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_generation: Option<String>,
    pub expected_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_path: Option<String>,
    pub resolver: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overlay_identity: Option<OverlayIdentityV1>,
}
impl SourceResolutionReceiptV1 {
    pub fn is_exact_current_source(&self) -> bool {
        self.schema_version == SOURCE_RESOLUTION_RECEIPT_SCHEMA_VERSION
            && self.status == SourceResolutionStatusV1::Resolved
            && !self.candidate_id.is_empty()
            && !self.provider.is_empty()
            && valid_sha256(&self.expected_hash)
            && self.resolved_hash.as_deref() == Some(self.expected_hash.as_str())
            && !self.expected_generation.is_empty()
            && self.resolved_generation.as_deref() == Some(self.expected_generation.as_str())
            && !self.expected_path.is_empty()
            && self.resolved_path.as_deref() == Some(self.expected_path.as_str())
            && !self.resolver.is_empty()
    }
}
#[rustfmt::skip] fn valid_sha256(value: &str) -> bool { value.len() == 71 && value.starts_with("sha256:") && value[7..].bytes().all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f')) }
#[cfg(test)]
mod tests {
    use super::*;
    fn receipt() -> SourceResolutionReceiptV1 {
        SourceResolutionReceiptV1 {
            schema_version: 1,
            candidate_id: "blueprint:symbol".into(),
            provider: "blueprint".into(),
            status: SourceResolutionStatusV1::Resolved,
            expected_hash: format!("sha256:{}", "a".repeat(64)),
            resolved_hash: Some(format!("sha256:{}", "a".repeat(64))),
            expected_generation: "gen-1".into(),
            resolved_generation: Some("gen-1".into()),
            expected_path: "src/lib.rs".into(),
            resolved_path: Some("src/lib.rs".into()),
            resolver: "blueprint graph resolve --node symbol".into(),
            overlay_identity: None,
        }
    }
    #[test] #[rustfmt::skip] fn exact_identity_is_required() {
        assert!(receipt().is_exact_current_source());
        let mut wrong_hash = receipt();
        wrong_hash.resolved_hash = Some("sha256:other".into());
        assert!(!wrong_hash.is_exact_current_source());
        let mut wrong_generation = receipt();
        wrong_generation.resolved_generation = Some("gen-0".into());
        assert!(!wrong_generation.is_exact_current_source());
        let mut wrong_path = receipt();
        wrong_path.resolved_path = Some("other.rs".into());
        assert!(!wrong_path.is_exact_current_source());
        let mut malformed = receipt(); malformed.expected_hash = "sha256:abc".into(); malformed.resolved_hash = Some("sha256:abc".into()); assert!(!malformed.is_exact_current_source());
    }
}
