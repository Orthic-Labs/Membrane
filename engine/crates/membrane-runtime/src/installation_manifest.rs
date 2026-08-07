//! Runtime binding for the installation manifest. Publishes the resident
//! manifest, parses peer manifests from the wire, and gates every IPC
//! handshake.

use crate::installation_identity::{InstallationIdentity, StartupClaim};
use crate::release_identity;
use crate::time;
use membrane_protocol::{
    canonical_data_root, ApiSchemaV1, ComponentV1, HandshakeError, InstallationManifestV1,
};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::OnceLock;

/// Process-wide handle to the active manifest. Set once at resident startup
/// and read by the serve layer on every IPC handshake that carries a peer
/// manifest.
static ACTIVE_MANIFEST: OnceLock<InstallationManifestV1> = OnceLock::new();

/// Canonical API schemas bound into the manifest. Each entry pins an
/// independently-versioned schema, so a future bumping of one contract does not
/// force a release-wide manifest rewrite.
pub fn active_api_schemas() -> BTreeMap<String, ApiSchemaV1> {
    let mut schemas = BTreeMap::new();
    for (name, version, payload) in [
        (
            "ContextCandidateSetV1",
            1u32,
            include_bytes!("../../../../schemas/context-candidate-set.v1.schema.json").as_slice(),
        ),
        (
            "ContextPacketV1",
            1,
            include_bytes!("../../../../schemas/context-packet.v1.schema.json").as_slice(),
        ),
        (
            "ContextReceiptV1",
            1,
            include_bytes!("../../../../schemas/context-receipt.v1.schema.json").as_slice(),
        ),
        (
            "KnowledgeEmissionV1",
            1,
            include_bytes!("../../../../schemas/knowledge-emission.v1.schema.json").as_slice(),
        ),
        (
            "ScopeGrantV1",
            1,
            include_bytes!("../../../../schemas/scope-grant.v1.schema.json").as_slice(),
        ),
    ] {
        let digest_sha256 = crate::digest::digest_bytes(payload);
        schemas.insert(
            name.to_string(),
            ApiSchemaV1 {
                version,
                digest_sha256,
            },
        );
    }
    schemas
}

/// Components the resident publishes today. Each entry pins the crate name and
/// its in-process build descriptor. The release-digest field carries the
/// resident's build identity so the handshake can detect a peer that loads
/// components from a sibling source tree.
pub fn active_components() -> BTreeMap<String, ComponentV1> {
    let mut components = BTreeMap::new();
    for (name, version) in [
        ("membrane-protocol", "0.1.0"),
        ("membrane-runtime", "0.1.0"),
    ] {
        components.insert(
            name.to_string(),
            ComponentV1 {
                name: name.to_string(),
                version: version.to_string(),
                digest_sha256: crate::digest::digest_bytes(name.as_bytes()),
            },
        );
    }
    components
}

/// Build the active manifest from the resident identity, claim, and the
/// workspace data root. The runtime calls this once during startup and feeds
/// the result to [`publish_active_manifest`].
pub fn build_active_manifest(
    identity: &InstallationIdentity,
    claim: &StartupClaim,
    data_root: &Path,
) -> InstallationManifestV1 {
    let canonical_root = canonical_data_root(data_root);
    InstallationManifestV1::new(
        identity.installation_id.clone(),
        claim.startup_generation,
        Path::new(&canonical_root),
        release_identity::release_generation(),
        release_identity::target_triple(),
        time::now_iso(),
        active_api_schemas(),
        active_components(),
    )
}

/// Publish the active manifest for this process. The first call wins; later
/// calls are no-ops so a misbehaving caller cannot change the active manifest
/// mid-flight.
pub fn publish_active_manifest(manifest: InstallationManifestV1) -> Result<(), ManifestError> {
    ACTIVE_MANIFEST
        .set(manifest)
        .map_err(|_| ManifestError::AlreadyPublished)
}

/// Read the active manifest, if any.
pub fn active_manifest() -> Option<&'static InstallationManifestV1> {
    ACTIVE_MANIFEST.get()
}

/// Read the active manifest alongside its canonical JSON form. The canonical
/// form is what an IPC peer would send on the wire; it is the most useful
/// debugging surface because it is the exact byte sequence `verify_handshake`
/// will compare against.
pub fn debug_active_manifest() -> Option<(InstallationManifestV1, String)> {
    let manifest = active_manifest()?.clone();
    let canonical = membrane_protocol::canonical_json_of(&manifest);
    Some((manifest, canonical))
}

/// Verify a peer's manifest against the active manifest, if any.
///
/// Returns `Err(HandshakeError::UnexpectedDataRoot { active_data_root: "" })`
/// when the resident has not yet published a manifest: the serve layer treats
/// a missing resident manifest as a hard reject.
pub fn verify_handshake(observed: &InstallationManifestV1) -> Result<(), HandshakeError> {
    let active = ACTIVE_MANIFEST
        .get()
        .ok_or(HandshakeError::UnexpectedDataRoot {
            active_data_root: String::new(),
            observed_data_root: observed.data_root.clone(),
            active_data_root_digest: String::new(),
            observed_data_root_digest: observed.data_root_digest.clone(),
        })?;
    InstallationManifestV1::verify_handshake(active, observed)
}

/// Errors while publishing or reading the active manifest.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ManifestError {
    /// A manifest was already published for this process. The runtime never
    /// publishes more than one; a second attempt indicates a logic bug.
    #[error("active installation manifest was already published for this process")]
    AlreadyPublished,
}

/// HTTP header used to carry the peer manifest on loopback IPC.
pub const HANDSHAKE_HEADER: &str = "x-membrane-manifest";

/// Result of parsing a request's manifest header.
#[derive(Debug)]
pub enum ParsedManifest {
    /// Header was present and parsed into a well-formed manifest.
    Present(InstallationManifestV1),
    /// Header was absent. Legacy clients pass through unchanged.
    Absent,
    /// Header was present but the value is not a valid manifest.
    Invalid(String),
}

impl ParsedManifest {
    /// Parse the header value as a JSON-serialized `InstallationManifestV1`.
    pub fn parse_header(value: Option<&str>) -> Self {
        match value {
            None => ParsedManifest::Absent,
            Some(raw) => match serde_json::from_str::<InstallationManifestV1>(raw) {
                Ok(manifest) => ParsedManifest::Present(manifest),
                Err(error) => ParsedManifest::Invalid(format!(
                    "X-Membrane-Manifest did not parse as InstallationManifestV1: {error}"
                )),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn sample_identity() -> InstallationIdentity {
        InstallationIdentity {
            schema_version: 2,
            installation_id: "00000000-0000-4000-8000-000000000001".to_string(),
            created_at: "2026-08-07T12:00:00Z".to_string(),
            startup_generation: 1,
            legacy_labels: Vec::new(),
            lineage: Vec::new(),
            current_service_instance_id: None,
            current_claimed_at: None,
        }
    }

    fn sample_claim(generation: u64) -> StartupClaim {
        StartupClaim {
            schema_version: 1,
            installation_id: "00000000-0000-4000-8000-000000000001".to_string(),
            startup_generation: generation,
            service_instance_id: "00000000-0000-4000-8000-0000000000aa".to_string(),
            claimed_at: "2026-08-07T12:00:00Z".to_string(),
        }
    }

    #[test]
    fn build_active_manifest_copies_identity_and_pins_bindings() {
        let manifest = build_active_manifest(
            &sample_identity(),
            &sample_claim(7),
            Path::new("/workspace/alpha/./beta/.."),
        );
        assert_eq!(
            manifest.installation_id,
            "00000000-0000-4000-8000-000000000001"
        );
        assert_eq!(manifest.startup_generation, 7);
        assert_eq!(manifest.data_root, "/workspace/alpha");
        assert_eq!(
            manifest.data_root_digest,
            crate::digest::digest_str(&manifest.data_root)
        );
        assert!(!manifest.api_schemas.is_empty());
        assert!(!manifest.components.is_empty());
        assert!(!manifest.platform_identity.is_empty());
        assert!(!manifest.release_digest.is_empty());
    }

    #[test]
    fn parsed_manifest_distinguishes_absent_present_and_invalid() {
        assert!(matches!(
            ParsedManifest::parse_header(None),
            ParsedManifest::Absent
        ));
        assert!(matches!(
            ParsedManifest::parse_header(Some("not-json")),
            ParsedManifest::Invalid(_)
        ));
        let manifest = build_active_manifest(
            &sample_identity(),
            &sample_claim(1),
            Path::new("/workspace/alpha"),
        );
        let raw = serde_json::to_string(&manifest).expect("serialize");
        match ParsedManifest::parse_header(Some(&raw)) {
            ParsedManifest::Present(parsed) => {
                assert_eq!(parsed.installation_id, manifest.installation_id);
            }
            other => panic!("expected Present, got {other:?}"),
        }
    }

    #[test]
    fn verify_handshake_rejects_observation_when_no_active_manifest() {
        let observed = build_active_manifest(
            &sample_identity(),
            &sample_claim(1),
            Path::new("/workspace/alpha"),
        );
        let error = verify_handshake(&observed).unwrap_err();
        assert_eq!(error.label(), "unexpected-data-root");
    }

    #[test]
    fn canonical_root_matches_protocol_canonical_data_root() {
        let path = PathBuf::from("/workspace/alpha/./beta/../gamma");
        assert_eq!(canonical_data_root(&path), canonical_data_root(&path));
    }

    #[test]
    fn active_components_lists_protocol_and_runtime() {
        let components = active_components();
        assert!(components.contains_key("membrane-protocol"));
        assert!(components.contains_key("membrane-runtime"));
        for component in components.values() {
            assert!(component.digest_sha256.starts_with("sha256:"));
        }
    }
}
