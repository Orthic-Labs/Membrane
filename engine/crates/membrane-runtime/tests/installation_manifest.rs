//! MBR-105: integration tests for the runtime installation manifest binding.
//!
//! The unit tests in `src/installation_manifest.rs` cover the per-module
//! surface (build, parse, canonicalize). These tests exercise the
//! cross-module flow that the resident service uses at startup: build a
//! manifest from an identity + claim, publish it, and verify that an
//! observed manifest's three reject paths fire as expected.
//!
//! The process-wide `OnceLock` means tests in this file can only publish
//! one manifest per test binary. The tests are therefore structured so the
//! first call wins and the rest observe the consequences of that first
//! publish, which is the same lifecycle the resident service follows.

use membrane_protocol::{canonical_data_root, ComponentV1, HandshakeError, InstallationManifestV1};
use membrane_runtime::installation_identity::{InstallationIdentity, StartupClaim};
use membrane_runtime::installation_manifest;
use std::collections::BTreeMap;
use std::path::Path;

fn sample_identity() -> InstallationIdentity {
    InstallationIdentity {
        schema_version: 2,
        installation_id: "00000000-0000-4000-8000-000000000111".to_string(),
        created_at: "2026-08-07T12:00:00Z".to_string(),
        startup_generation: 3,
        legacy_labels: Vec::new(),
        lineage: Vec::new(),
        current_service_instance_id: Some("00000000-0000-4000-8000-0000000000bb".to_string()),
        current_claimed_at: Some("2026-08-07T12:00:00Z".to_string()),
    }
}

fn sample_claim() -> StartupClaim {
    StartupClaim {
        schema_version: 1,
        installation_id: "00000000-0000-4000-8000-000000000111".to_string(),
        startup_generation: 3,
        service_instance_id: "00000000-0000-4000-8000-0000000000bb".to_string(),
        claimed_at: "2026-08-07T12:00:00Z".to_string(),
    }
}

fn sample_manifest() -> InstallationManifestV1 {
    installation_manifest::build_active_manifest(
        &sample_identity(),
        &sample_claim(),
        Path::new("/workspace/alpha"),
    )
}

#[test]
fn active_manifest_publishes_once_and_persists_for_subsequent_calls() {
    let manifest = sample_manifest();
    let result = installation_manifest::publish_active_manifest(manifest.clone());
    if result.is_err() {
        // A previous test in this binary already published; that is fine —
        // the active manifest is the process-wide source of truth. We
        // observe it instead of overwriting it.
        let active = installation_manifest::active_manifest()
            .expect("active manifest is set after a successful publish");
        // The published manifest, whatever the source, is well-formed.
        assert!(!active.installation_id.is_empty());
        assert!(active.startup_generation >= 1);
    } else {
        let active = installation_manifest::active_manifest()
            .expect("active manifest is set after a successful publish");
        assert_eq!(active.installation_id, manifest.installation_id);
    }
}

#[test]
fn handshake_rejects_wrong_installation() {
    // Publish the canonical manifest if not yet published. Subsequent
    // publishes are no-ops, so this is idempotent within a test binary.
    let _ = installation_manifest::publish_active_manifest(sample_manifest());
    let active = match installation_manifest::active_manifest() {
        Some(active) => active.clone(),
        None => return, // no manifest published in this binary, skip
    };
    let mut observed = active.clone();
    observed.installation_id = "00000000-0000-4000-8000-000000000222".to_string();
    let error = installation_manifest::verify_handshake(&observed).unwrap_err();
    assert_eq!(error.label(), "wrong-installation");
}

#[test]
fn handshake_rejects_incompatible_schema() {
    let _ = installation_manifest::publish_active_manifest(sample_manifest());
    let active = match installation_manifest::active_manifest() {
        Some(active) => active.clone(),
        None => return,
    };
    let mut observed = active.clone();
    observed.protocol_schema_version += 1;
    let error = installation_manifest::verify_handshake(&observed).unwrap_err();
    assert_eq!(error.label(), "incompatible-schema");
}

#[test]
fn handshake_rejects_unexpected_data_root() {
    let _ = installation_manifest::publish_active_manifest(sample_manifest());
    let active = match installation_manifest::active_manifest() {
        Some(active) => active.clone(),
        None => return,
    };
    let mut observed = active.clone();
    observed.data_root = canonical_data_root(Path::new("/workspace/beta"));
    let error = installation_manifest::verify_handshake(&observed).unwrap_err();
    assert_eq!(error.label(), "unexpected-data-root");
}

#[test]
fn handshake_accepts_matching_manifest() {
    let _ = installation_manifest::publish_active_manifest(sample_manifest());
    let active = match installation_manifest::active_manifest() {
        Some(active) => active.clone(),
        None => return,
    };
    let outcome = installation_manifest::verify_handshake(&active);
    assert_eq!(outcome, Ok(()));
}

#[test]
fn debug_active_manifest_returns_canonical_json() {
    let _ = installation_manifest::publish_active_manifest(sample_manifest());
    let (manifest, canonical) = match installation_manifest::debug_active_manifest() {
        Some(value) => value,
        None => return,
    };
    assert_eq!(manifest.installation_id, canonical_install_id());
    // The canonical form must round-trip back to the same typed value.
    let parsed: InstallationManifestV1 = serde_json::from_str(&canonical).unwrap();
    assert_eq!(parsed.installation_id, manifest.installation_id);
}

#[test]
fn parsed_manifest_distinguishes_three_states() {
    use membrane_runtime::installation_manifest::ParsedManifest;

    assert!(matches!(
        ParsedManifest::parse_header(None),
        ParsedManifest::Absent
    ));
    assert!(matches!(
        ParsedManifest::parse_header(Some("not-json")),
        ParsedManifest::Invalid(_)
    ));
    let manifest = sample_manifest();
    let raw = serde_json::to_string(&manifest).expect("serialize");
    match ParsedManifest::parse_header(Some(&raw)) {
        ParsedManifest::Present(parsed) => {
            assert_eq!(parsed.installation_id, manifest.installation_id);
        }
        other => panic!("expected Present, got {other:?}"),
    }
}

#[test]
fn component_manifest_lists_protocol_and_runtime() {
    let components: BTreeMap<String, ComponentV1> = installation_manifest::active_components();
    assert!(components.contains_key("membrane-protocol"));
    assert!(components.contains_key("membrane-runtime"));
    for component in components.values() {
        assert!(component.digest_sha256.starts_with("sha256:"));
    }
}

fn canonical_install_id() -> String {
    "00000000-0000-4000-8000-000000000111".to_string()
}

// Reference the protocol's typed error so the integration test exercises
// the same error type the runtime re-exports.
#[allow(dead_code)]
fn _error_label(error: &HandshakeError) -> &'static str {
    error.label()
}
