//! Installation identity and component manifest — the v1 protocol contract.
//!
//! A resident publishes one [`InstallationManifestV1`] for its active startup
//! generation. IPC peers present the same typed shape and the runtime rejects a
//! mismatch before request dispatch. The manifest binds the installation,
//! release, component set, data root, API schemas, and native platform.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Schema version of the manifest envelope itself.
pub const MANIFEST_SCHEMA_VERSION: u32 = 1;

/// Version of the core Membrane protocol shapes.
pub const PROTOCOL_SCHEMA_VERSION: u32 = 1;

/// One API schema bound into the installation manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApiSchemaV1 {
    /// Independently advancing schema version.
    pub version: u32,
    /// `sha256:<hex>` of the schema bytes shipped by this build.
    pub digest_sha256: String,
}

/// One named component shipped by the resident build.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ComponentV1 {
    /// Stable component name, e.g. `membrane-runtime`.
    pub name: String,
    /// Component release version.
    pub version: String,
    /// `sha256:<hex>` binding for the component build.
    pub digest_sha256: String,
}

/// Installation and build identity carried by an IPC peer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InstallationManifestV1 {
    /// Manifest envelope version.
    pub schema_version: u32,
    /// Stable installation UUIDv4.
    pub installation_id: String,
    /// Monotonic resident startup generation.
    pub startup_generation: u64,
    /// Digest of the complete release source/build identity.
    pub release_digest: String,
    /// Canonical data-root path retained for diagnostics.
    pub data_root: String,
    /// Digest of the canonical data-root path.
    pub data_root_digest: String,
    /// Version of the core Membrane protocol shapes.
    pub protocol_schema_version: u32,
    /// Independently versioned API schema bindings, keyed by stable shape name.
    pub api_schemas: BTreeMap<String, ApiSchemaV1>,
    /// Native target identity, such as `aarch64-apple-darwin`.
    pub platform_identity: String,
    /// Components shipped by the resident build, keyed by stable name.
    pub components: BTreeMap<String, ComponentV1>,
    /// RFC 3339 UTC instant at which the resident minted this manifest.
    pub minted_at: String,
}

/// Typed IPC handshake rejection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandshakeError {
    /// Installation ID or active startup generation differs.
    WrongInstallation {
        active_installation_id: String,
        active_startup_generation: u64,
        observed_installation_id: String,
        observed_startup_generation: u64,
    },
    /// Manifest/protocol version, release, platform, API schema, or component differs.
    IncompatibleSchema {
        active_schema_version: u32,
        observed_schema_version: u32,
        active_protocol_schema_version: u32,
        observed_protocol_schema_version: u32,
        active_release_digest: String,
        observed_release_digest: String,
        active_platform_identity: String,
        observed_platform_identity: String,
        mismatched_api_schemas: Vec<String>,
        mismatched_components: Vec<String>,
    },
    /// Canonical data-root path or its digest differs.
    UnexpectedDataRoot {
        active_data_root: String,
        observed_data_root: String,
        active_data_root_digest: String,
        observed_data_root_digest: String,
    },
}

impl HandshakeError {
    /// Stable label used in IPC rejection responses and acceptance tests.
    pub const fn label(&self) -> &'static str {
        match self {
            HandshakeError::WrongInstallation { .. } => "wrong-installation",
            HandshakeError::IncompatibleSchema { .. } => "incompatible-schema",
            HandshakeError::UnexpectedDataRoot { .. } => "unexpected-data-root",
        }
    }
}

impl std::fmt::Display for HandshakeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HandshakeError::WrongInstallation {
                active_installation_id,
                active_startup_generation,
                observed_installation_id,
                observed_startup_generation,
            } => write!(
                formatter,
                "wrong installation: active={active_installation_id}@{active_startup_generation} \
                 observed={observed_installation_id}@{observed_startup_generation}"
            ),
            HandshakeError::IncompatibleSchema {
                active_schema_version,
                observed_schema_version,
                active_protocol_schema_version,
                observed_protocol_schema_version,
                active_release_digest,
                observed_release_digest,
                active_platform_identity,
                observed_platform_identity,
                mismatched_api_schemas,
                mismatched_components,
            } => write!(
                formatter,
                "incompatible schema: manifest={active_schema_version}/{observed_schema_version} \
                 protocol={active_protocol_schema_version}/{observed_protocol_schema_version} \
                 release={active_release_digest}/{observed_release_digest} \
                 platform={active_platform_identity}/{observed_platform_identity} \
                 api_schemas={mismatched_api_schemas:?} components={mismatched_components:?}"
            ),
            HandshakeError::UnexpectedDataRoot {
                active_data_root,
                observed_data_root,
                active_data_root_digest,
                observed_data_root_digest,
            } => write!(
                formatter,
                "unexpected data root: active={active_data_root} ({active_data_root_digest}) \
                 observed={observed_data_root} ({observed_data_root_digest})"
            ),
        }
    }
}

impl std::error::Error for HandshakeError {}

/// Lexically normalize a data root without requiring it to exist.
///
/// The function removes `.` segments, resolves `..` without escaping an
/// absolute root, and preserves the platform-native prefix and separator. It
/// deliberately does not resolve symlinks: callers bind the path selected by
/// installation-root discovery, including when that root has not been created.
pub fn canonical_data_root(path: &Path) -> String {
    let mut normalized = PathBuf::new();
    let absolute = path.is_absolute();
    let mut normal_depth = 0usize;

    for component in path.components() {
        match component {
            std::path::Component::Prefix(_) | std::path::Component::RootDir => {
                normalized.push(component.as_os_str());
            }
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir if normal_depth > 0 => {
                normalized.pop();
                normal_depth -= 1;
            }
            std::path::Component::ParentDir if !absolute => normalized.push(".."),
            std::path::Component::ParentDir => {}
            std::path::Component::Normal(segment) => {
                normalized.push(segment);
                normal_depth += 1;
            }
        }
    }

    if normalized.as_os_str().is_empty() {
        ".".to_string()
    } else {
        normalized.to_string_lossy().into_owned()
    }
}

fn mismatched_keys<T: PartialEq>(
    active: &BTreeMap<String, T>,
    observed: &BTreeMap<String, T>,
) -> Vec<String> {
    let mut mismatches = Vec::new();
    for (name, active_value) in active {
        if observed.get(name) != Some(active_value) {
            mismatches.push(name.clone());
        }
    }
    for name in observed.keys() {
        if !active.contains_key(name) {
            mismatches.push(name.clone());
        }
    }
    mismatches.sort();
    mismatches
}

impl InstallationManifestV1 {
    /// Build a well-formed manifest and derive its data-root digest.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        installation_id: impl Into<String>,
        startup_generation: u64,
        data_root: &Path,
        release_digest: impl Into<String>,
        platform_identity: impl Into<String>,
        minted_at: impl Into<String>,
        api_schemas: BTreeMap<String, ApiSchemaV1>,
        components: BTreeMap<String, ComponentV1>,
    ) -> Self {
        let data_root = canonical_data_root(data_root);
        let data_root_digest = crate::digest_str(&data_root);
        Self {
            schema_version: MANIFEST_SCHEMA_VERSION,
            installation_id: installation_id.into(),
            startup_generation,
            release_digest: release_digest.into(),
            data_root,
            data_root_digest,
            protocol_schema_version: PROTOCOL_SCHEMA_VERSION,
            api_schemas,
            platform_identity: platform_identity.into(),
            components,
            minted_at: minted_at.into(),
        }
    }

    /// Compare a peer manifest with the resident's active manifest.
    ///
    /// Rejections use deterministic precedence: installation, schema/build,
    /// then data root. `minted_at` is diagnostic metadata and is intentionally
    /// excluded from compatibility.
    pub fn verify_handshake(active: &Self, observed: &Self) -> Result<(), HandshakeError> {
        if active.installation_id != observed.installation_id
            || active.startup_generation != observed.startup_generation
        {
            return Err(HandshakeError::WrongInstallation {
                active_installation_id: active.installation_id.clone(),
                active_startup_generation: active.startup_generation,
                observed_installation_id: observed.installation_id.clone(),
                observed_startup_generation: observed.startup_generation,
            });
        }

        let mismatched_api_schemas = mismatched_keys(&active.api_schemas, &observed.api_schemas);
        let mismatched_components = mismatched_keys(&active.components, &observed.components);
        if active.schema_version != MANIFEST_SCHEMA_VERSION
            || observed.schema_version != MANIFEST_SCHEMA_VERSION
            || active.protocol_schema_version != observed.protocol_schema_version
            || active.release_digest != observed.release_digest
            || active.platform_identity != observed.platform_identity
            || !mismatched_api_schemas.is_empty()
            || !mismatched_components.is_empty()
        {
            return Err(HandshakeError::IncompatibleSchema {
                active_schema_version: active.schema_version,
                observed_schema_version: observed.schema_version,
                active_protocol_schema_version: active.protocol_schema_version,
                observed_protocol_schema_version: observed.protocol_schema_version,
                active_release_digest: active.release_digest.clone(),
                observed_release_digest: observed.release_digest.clone(),
                active_platform_identity: active.platform_identity.clone(),
                observed_platform_identity: observed.platform_identity.clone(),
                mismatched_api_schemas,
                mismatched_components,
            });
        }

        let active_data_root = canonical_data_root(Path::new(&active.data_root));
        let observed_data_root = canonical_data_root(Path::new(&observed.data_root));
        let active_data_root_digest = crate::digest_str(&active_data_root);
        let observed_data_root_digest = crate::digest_str(&observed_data_root);
        if active_data_root != observed_data_root
            || active.data_root_digest != active_data_root_digest
            || observed.data_root_digest != observed_data_root_digest
            || active.data_root_digest != observed.data_root_digest
        {
            return Err(HandshakeError::UnexpectedDataRoot {
                active_data_root,
                observed_data_root,
                active_data_root_digest: active.data_root_digest.clone(),
                observed_data_root_digest: observed.data_root_digest.clone(),
            });
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest(fill: char) -> String {
        format!("sha256:{}", fill.to_string().repeat(64))
    }

    fn sample_api_schemas() -> BTreeMap<String, ApiSchemaV1> {
        BTreeMap::from([
            (
                "ContextPacketV1".to_string(),
                ApiSchemaV1 {
                    version: 1,
                    digest_sha256: digest('3'),
                },
            ),
            (
                "ScopeGrantV1".to_string(),
                ApiSchemaV1 {
                    version: 1,
                    digest_sha256: digest('4'),
                },
            ),
        ])
    }

    fn sample_components() -> BTreeMap<String, ComponentV1> {
        BTreeMap::from([
            (
                "membrane-protocol".to_string(),
                ComponentV1 {
                    name: "membrane-protocol".to_string(),
                    version: "0.1.0".to_string(),
                    digest_sha256: digest('1'),
                },
            ),
            (
                "membrane-runtime".to_string(),
                ComponentV1 {
                    name: "membrane-runtime".to_string(),
                    version: "0.1.0".to_string(),
                    digest_sha256: digest('2'),
                },
            ),
        ])
    }

    fn sample_active() -> InstallationManifestV1 {
        InstallationManifestV1::new(
            "00000000-0000-4000-8000-000000000001",
            7,
            Path::new("/workspace/alpha"),
            digest('a'),
            "aarch64-apple-darwin",
            "2026-08-07T12:00:00Z",
            sample_api_schemas(),
            sample_components(),
        )
    }

    #[test]
    fn matching_manifests_pass() {
        let active = sample_active();
        assert_eq!(
            InstallationManifestV1::verify_handshake(&active, &active),
            Ok(())
        );
    }

    #[test]
    fn installation_id_and_generation_are_bound() {
        let active = sample_active();
        let mut wrong_id = active.clone();
        wrong_id.installation_id = "00000000-0000-4000-8000-000000000002".to_string();
        assert_eq!(
            InstallationManifestV1::verify_handshake(&active, &wrong_id)
                .unwrap_err()
                .label(),
            "wrong-installation"
        );

        let mut wrong_generation = active.clone();
        wrong_generation.startup_generation -= 1;
        assert_eq!(
            InstallationManifestV1::verify_handshake(&active, &wrong_generation)
                .unwrap_err()
                .label(),
            "wrong-installation"
        );
    }

    #[test]
    fn envelope_protocol_release_and_platform_are_schema_bindings() {
        let active = sample_active();
        for mutate in [
            |manifest: &mut InstallationManifestV1| manifest.schema_version += 1,
            |manifest: &mut InstallationManifestV1| manifest.protocol_schema_version += 1,
            |manifest: &mut InstallationManifestV1| manifest.release_digest = digest('b'),
            |manifest: &mut InstallationManifestV1| {
                manifest.platform_identity = "x86_64-pc-windows-msvc".to_string()
            },
        ] {
            let mut observed = active.clone();
            mutate(&mut observed);
            assert_eq!(
                InstallationManifestV1::verify_handshake(&active, &observed)
                    .unwrap_err()
                    .label(),
                "incompatible-schema"
            );
        }
    }

    #[test]
    fn api_schema_and_component_manifests_are_exact() {
        let active = sample_active();
        let mut wrong_api = active.clone();
        wrong_api
            .api_schemas
            .get_mut("ScopeGrantV1")
            .unwrap()
            .digest_sha256 = digest('9');
        assert_eq!(
            InstallationManifestV1::verify_handshake(&active, &wrong_api)
                .unwrap_err()
                .label(),
            "incompatible-schema"
        );

        let mut wrong_component = active.clone();
        wrong_component
            .components
            .get_mut("membrane-runtime")
            .unwrap()
            .name = "renamed-runtime".to_string();
        assert_eq!(
            InstallationManifestV1::verify_handshake(&active, &wrong_component)
                .unwrap_err()
                .label(),
            "incompatible-schema"
        );
    }

    #[test]
    fn path_or_digest_mismatch_is_an_unexpected_data_root() {
        let active = sample_active();
        let mut wrong_path = active.clone();
        wrong_path.data_root = "/workspace/beta".to_string();
        wrong_path.data_root_digest = crate::digest_str(&wrong_path.data_root);
        assert_eq!(
            InstallationManifestV1::verify_handshake(&active, &wrong_path)
                .unwrap_err()
                .label(),
            "unexpected-data-root"
        );

        let mut wrong_digest = active.clone();
        wrong_digest.data_root_digest = digest('f');
        assert_eq!(
            InstallationManifestV1::verify_handshake(&active, &wrong_digest)
                .unwrap_err()
                .label(),
            "unexpected-data-root"
        );
    }

    #[test]
    fn canonical_data_root_collapses_segments_without_escaping_root() {
        assert_eq!(
            canonical_data_root(Path::new("/workspace/alpha/./beta/../gamma")),
            "/workspace/alpha/gamma"
        );
        assert_eq!(canonical_data_root(Path::new("/../escape")), "/escape");
        assert_eq!(canonical_data_root(Path::new("a/../../b")), "../b");
    }

    #[test]
    fn constructor_pins_versions_and_derives_data_root_digest() {
        let manifest = sample_active();
        assert_eq!(manifest.schema_version, MANIFEST_SCHEMA_VERSION);
        assert_eq!(manifest.protocol_schema_version, PROTOCOL_SCHEMA_VERSION);
        assert_eq!(
            manifest.data_root_digest,
            crate::digest_str(&manifest.data_root)
        );
    }
}
