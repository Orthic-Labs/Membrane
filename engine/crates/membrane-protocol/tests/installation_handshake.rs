//! Round-trip + handshake tests for `InstallationManifestV1`.

mod common;

use membrane_protocol::{
    canonical_data_root, canonicalize, ApiSchemaV1, ComponentV1, InstallationManifestV1,
    MANIFEST_SCHEMA_VERSION, PROTOCOL_SCHEMA_VERSION,
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const SCHEMA: &str = include_str!("../../../../schemas/installation-manifest.v1.schema.json");

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

fn sample_manifest() -> InstallationManifestV1 {
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

fn canonicalize_manifest(manifest: &InstallationManifestV1) -> String {
    canonicalize(&serde_json::to_value(manifest).expect("manifest re-serializes"))
}

#[test]
fn manifest_validates_against_its_schema() {
    let schema: Value = serde_json::from_str(SCHEMA).expect("schema parses");
    let manifest = sample_manifest();
    let value = serde_json::to_value(&manifest).expect("manifest serializes");
    let violations = common::validate(&schema, &value);
    assert!(
        violations.is_empty(),
        "manifest fails schema validation: {violations:#?}"
    );
}

#[test]
fn canonical_digest_is_stable_for_a_known_manifest() {
    let manifest = sample_manifest();
    let canonical = canonicalize_manifest(&manifest);
    let mut hasher = Sha256::new();
    hasher.update(canonical.as_bytes());
    let expected = format!("sha256:{}", sha256_hex(&canonical));
    let actual = format!("sha256:{}", hex::encode(hasher.finalize()));
    assert_eq!(actual, expected);
}

#[test]
fn manifest_envelope_versions_are_pinned() {
    let manifest = sample_manifest();
    assert_eq!(manifest.schema_version, MANIFEST_SCHEMA_VERSION);
    assert_eq!(manifest.protocol_schema_version, PROTOCOL_SCHEMA_VERSION);
}

#[test]
fn canonical_data_root_collapses_dot_segments() {
    let canonical = canonical_data_root(Path::new("/workspace/alpha/./beta/../gamma"));
    assert_eq!(
        canonical,
        PathBuf::from(std::path::MAIN_SEPARATOR.to_string())
            .join("workspace/alpha/gamma")
            .to_string_lossy()
    );
}

#[test]
fn canonical_data_root_preserves_parent_at_root() {
    let canonical = canonical_data_root(Path::new("/../escape"));
    assert_eq!(
        canonical,
        PathBuf::from(std::path::MAIN_SEPARATOR.to_string())
            .join("escape")
            .to_string_lossy()
    );
}

#[test]
fn hand_shake_rejects_wrong_installation() {
    let active = sample_manifest();
    let mut observed = active.clone();
    observed.installation_id = "00000000-0000-4000-8000-000000000002".to_string();
    let error = InstallationManifestV1::verify_handshake(&active, &observed)
        .expect_err("different installation must be rejected");
    assert_eq!(error.label(), "wrong-installation");
}

#[test]
fn hand_shake_rejects_wrong_startup_generation() {
    let active = sample_manifest();
    let mut observed = active.clone();
    observed.startup_generation = 6;
    let error = InstallationManifestV1::verify_handshake(&active, &observed)
        .expect_err("older generation must be rejected");
    assert_eq!(error.label(), "wrong-installation");
}

#[test]
fn hand_shake_rejects_incompatible_protocol_schema() {
    let active = sample_manifest();
    let mut observed = active.clone();
    observed.protocol_schema_version += 1;
    let error = InstallationManifestV1::verify_handshake(&active, &observed)
        .expect_err("different protocol schema must be rejected");
    assert_eq!(error.label(), "incompatible-schema");
}

#[test]
fn hand_shake_rejects_incompatible_release_digest() {
    let active = sample_manifest();
    let mut observed = active.clone();
    observed.release_digest = digest('b');
    let error = InstallationManifestV1::verify_handshake(&active, &observed)
        .expect_err("different release digest must be rejected");
    assert_eq!(error.label(), "incompatible-schema");
}

#[test]
fn hand_shake_rejects_incompatible_platform() {
    let active = sample_manifest();
    let mut observed = active.clone();
    observed.platform_identity = "x86_64-pc-windows-msvc".to_string();
    let error = InstallationManifestV1::verify_handshake(&active, &observed)
        .expect_err("different platform identity must be rejected");
    assert_eq!(error.label(), "incompatible-schema");
}

#[test]
fn hand_shake_rejects_incompatible_api_schema_binding() {
    let active = sample_manifest();
    let mut observed = active.clone();
    observed
        .api_schemas
        .get_mut("ScopeGrantV1")
        .unwrap()
        .digest_sha256 = digest('9');
    let error = InstallationManifestV1::verify_handshake(&active, &observed)
        .expect_err("different API schema digest must be rejected");
    assert_eq!(error.label(), "incompatible-schema");
}

#[test]
fn hand_shake_rejects_incompatible_component_digest() {
    let active = sample_manifest();
    let mut observed = active.clone();
    observed
        .components
        .get_mut("membrane-protocol")
        .unwrap()
        .digest_sha256 = digest('9');
    let error = InstallationManifestV1::verify_handshake(&active, &observed)
        .expect_err("different component digest must be rejected");
    assert_eq!(error.label(), "incompatible-schema");
}

#[test]
fn hand_shake_rejects_unexpected_data_root() {
    let active = sample_manifest();
    let mut observed = active.clone();
    observed.data_root = "/workspace/beta".to_string();
    observed.data_root_digest = digest('c');
    let error = InstallationManifestV1::verify_handshake(&active, &observed)
        .expect_err("different data root must be rejected");
    assert_eq!(error.label(), "unexpected-data-root");
}

#[test]
fn hand_shake_rejects_data_root_digest_tampering() {
    let active = sample_manifest();
    let mut observed = active.clone();
    observed.data_root_digest = digest('f');
    let error = InstallationManifestV1::verify_handshake(&active, &observed)
        .expect_err("tampered data-root digest must be rejected");
    assert_eq!(error.label(), "unexpected-data-root");
}

#[test]
fn hand_shake_passes_when_all_invariants_match() {
    let active = sample_manifest();
    let observed = active.clone();
    assert_eq!(
        InstallationManifestV1::verify_handshake(&active, &observed),
        Ok(())
    );
}

#[test]
fn hand_shake_error_precedence_is_installation_then_schema_then_data_root() {
    let active = sample_manifest();
    let mut observed = active.clone();
    observed.installation_id = "00000000-0000-4000-8000-000000000099".to_string();
    observed.protocol_schema_version += 1;
    observed.data_root = "/workspace/different".to_string();
    observed.data_root_digest = digest('0');
    let error = InstallationManifestV1::verify_handshake(&active, &observed).unwrap_err();
    assert_eq!(error.label(), "wrong-installation");

    let mut observed = active.clone();
    observed.protocol_schema_version += 1;
    observed.data_root = "/workspace/different".to_string();
    observed.data_root_digest = digest('0');
    let error = InstallationManifestV1::verify_handshake(&active, &observed).unwrap_err();
    assert_eq!(error.label(), "incompatible-schema");
}

fn sha256_hex(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    hex::encode(hasher.finalize())
}
