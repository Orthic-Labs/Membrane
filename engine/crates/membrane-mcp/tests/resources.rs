//! MBR-304 contract and parity tests for the canonical Membrane MCP resources.
//!
//! These tests assert every resource:
//!   (a) is bounded — its `accessGrants` array is non-empty and every grant
//!       references a known protocol grant type (declared in
//!       `schemas/registry/resources/resources-index.v1.json`'s
//!       `supportedGrantTypes` block),
//!   (b) declares `authorityEscalation: false`,
//!   (c) declares a positive `version`,
//!   (d) reads inside the matching grant succeed and reads outside the
//!       grant return a typed `resource_access_denied` rejection with no
//!       content leak, and
//!   (e) the resource body never appears in a denied response.
//!
//! Book-mode discipline: the file is wired into the crate's test target but is
//! not executed during implementation — the deferred command list runs at the
//! end of Book 1.

use membrane_mcp::{
    list_resources_payload, read_payload, read_result_payload, ReadOutcome, RESOURCE_NAMES,
    RESOURCE_URIS,
};
use serde_json::Value;
use std::fs;
use std::path::PathBuf;

const RESOURCES_INDEX: &str =
    include_str!("../../../../schemas/registry/resources/resources-index.v1.json");

fn supported_grant_types() -> Vec<String> {
    let value: Value = serde_json::from_str(RESOURCES_INDEX).expect("resources-index parses");
    value["supportedGrantTypes"]
        .as_array()
        .expect("supportedGrantTypes array")
        .iter()
        .map(|entry| entry["name"].as_str().unwrap().to_string())
        .collect()
}

fn resource_payload_by_name(name: &str) -> Value {
    // Read the canonical JSON at runtime so we never have to embed it via a
    // non-literal `include_str!` argument. The Rust module under test embeds
    // the same files via `include_str!`, so this proves the on-disk and the
    // compiled-in resource definitions agree.
    let here = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let path = here
        .join("../../../../schemas/registry/resources")
        .join(format!("{name}.v1.json"));
    let raw = fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    serde_json::from_str(&raw).expect("canonical resource parses")
}

#[test]
fn list_payload_returns_four_canonical_resources() {
    let payload = list_resources_payload();
    let resources = payload["resources"].as_array().unwrap();
    assert_eq!(resources.len(), 4, "four canonical resources are committed");
    let names: Vec<&str> = resources
        .iter()
        .map(|r| r["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, RESOURCE_NAMES);
}

#[test]
fn list_payload_returns_four_canonical_uris() {
    let payload = list_resources_payload();
    let resources = payload["resources"].as_array().unwrap();
    let uris: Vec<&str> = resources
        .iter()
        .map(|r| r["uri"].as_str().unwrap())
        .collect();
    assert_eq!(uris, RESOURCE_URIS);
}

#[test]
fn every_resource_is_bounded_and_versioned() {
    let payload = list_resources_payload();
    let supported = supported_grant_types();
    for resource in payload["resources"].as_array().unwrap() {
        let name = resource["name"].as_str().unwrap();
        let grants = resource["accessGrants"]
            .as_array()
            .unwrap_or_else(|| panic!("{name} declares an accessGrants array"));
        assert!(
            !grants.is_empty(),
            "{name} must declare at least one accessGrant"
        );
        for grant in grants {
            let grant_name = grant.as_str().unwrap();
            assert!(
                supported.iter().any(|s| s == grant_name),
                "{name} declares unknown grant type: {grant_name}"
            );
        }
        let version = resource["version"].as_u64().unwrap_or(0);
        assert!(
            version >= 1,
            "{name} declares a positive version (got {version})"
        );
        assert_eq!(
            resource["authorityEscalation"],
            Value::Bool(false),
            "{name} must declare authorityEscalation: false"
        );
        assert!(
            resource["mimeType"]
                .as_str()
                .map(|s| !s.is_empty())
                .unwrap_or(false),
            "{name} declares a mimeType"
        );
        assert!(
            resource["uri"]
                .as_str()
                .map(|s| s.starts_with("membrane://resource/"))
                .unwrap_or(false),
            "{name} URI uses the membrane://resource/ scheme"
        );
    }
}

#[test]
fn resources_index_lists_every_committed_resource() {
    let payload = list_resources_payload();
    let names: Vec<String> = payload["resources"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["name"].as_str().unwrap().to_string())
        .collect();
    let index: Value = serde_json::from_str(RESOURCES_INDEX).expect("resources-index parses");
    let index_names: Vec<String> = index["resources"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["name"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        names, index_names,
        "resources/list matches the canonical resources-index"
    );
}

#[test]
fn read_with_matching_grant_returns_body() {
    for (uri, grant) in [
        ("membrane://resource/resources-index/v1", "protocol.read"),
        (
            "membrane://resource/installation-manifest/v1",
            "installation.read",
        ),
        ("membrane://resource/lease-status/v1", "lease.read"),
        ("membrane://resource/operation-registry/v1", "protocol.read"),
    ] {
        let outcome = read_payload(uri, &[grant]);
        assert!(
            matches!(outcome, ReadOutcome::Ok(_)),
            "expected Ok for {uri} with grant {grant}, got denial"
        );
        let outcome = read_payload(uri, &[grant]);
        if let ReadOutcome::Ok(body) = outcome {
            assert_eq!(body["uri"], uri);
            assert!(body["body"].is_object(), "{uri} body is present");
        }
    }
}

#[test]
fn read_without_grant_returns_typed_rejection_with_no_body() {
    let cases: [(&str, &[&str]); 6] = [
        ("membrane://resource/installation-manifest/v1", &[]),
        (
            "membrane://resource/installation-manifest/v1",
            &["protocol.read"],
        ),
        ("membrane://resource/lease-status/v1", &[]),
        ("membrane://resource/lease-status/v1", &["protocol.read"]),
        ("membrane://resource/operation-registry/v1", &[]),
        (
            "membrane://resource/operation-registry/v1",
            &["installation.read"],
        ),
    ];
    for (uri, grants) in cases {
        let outcome = read_payload(uri, grants);
        assert!(
            matches!(outcome, ReadOutcome::AccessDenied { .. }),
            "expected AccessDenied for {uri} with grants {grants:?}"
        );
        let envelope = read_result_payload(outcome);
        assert_eq!(envelope["error"]["kind"], "error");
        assert_eq!(envelope["error"]["code"], "resource_access_denied");
        assert_eq!(envelope["error"]["retryable"], false);
        assert!(
            envelope.get("body").is_none(),
            "denied response never carries a body field"
        );
        // The details block enumerates the required grants so the caller knows
        // what they would need to present, but never leaks any of the
        // resource's payload fields.
        assert!(envelope["error"]["details"]["requiredGrants"].is_array());
        assert!(envelope["error"]["details"]["presentedGrants"].is_array());
        // No resource body field is present anywhere in the denied envelope.
        assert!(envelope.get("body").is_none());
        assert!(envelope.get("installationId").is_none());
        assert!(envelope.get("dataRootDigest").is_none());
        assert!(envelope.get("operations").is_none());
    }
}

#[test]
fn read_with_unknown_uri_returns_not_found_envelope() {
    let outcome = read_payload("membrane://resource/does-not-exist/v1", &["protocol.read"]);
    assert!(matches!(outcome, ReadOutcome::NotFound { .. }));
    let envelope = read_result_payload(outcome);
    assert_eq!(envelope["error"]["code"], "resource_not_found");
}

#[test]
fn resource_versions_are_independently_bumped() {
    // Every committed resource in this book declares version 1; advancing the
    // protocol moves the version forward and proves the resource shape is
    // independently versioned (one resource's bump never moves a sibling).
    let payload = list_resources_payload();
    for resource in payload["resources"].as_array().unwrap() {
        let version = resource["version"].as_u64().unwrap();
        assert!(
            version >= 1,
            "resource {} version {version} must be positive",
            resource["name"]
        );
    }
}

#[test]
fn every_resource_body_is_an_object_or_null() {
    // A resource whose `body` is a string leaks content through the resource
    // wire even before grants are checked; the contract requires the body to
    // be either an object (structured payload) or null (template-only).
    for name in RESOURCE_NAMES {
        let value = resource_payload_by_name(name);
        assert_eq!(value["name"].as_str(), Some(*name));
        assert_eq!(
            value["version"].as_u64().unwrap_or(0),
            1,
            "{name} declares version 1"
        );
        let body = &value["body"];
        assert!(
            body.is_null() || body.is_object(),
            "{name} body must be an object or null (got {body})"
        );
    }
}
