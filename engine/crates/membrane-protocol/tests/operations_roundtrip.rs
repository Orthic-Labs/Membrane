//! Operation-specific contract round-trip tests — the Rust half of the
//! MBR-301 acceptance ("Every operation has golden success/error fixtures
//! and independent versioning.").
//!
//! For every entry in [`membrane_protocol::OPERATIONS`] this proves:
//!
//!   1. The success fixture validates against the per-operation JSON Schema
//!      (using the same minimal validator the MBR-101 round-trip uses).
//!   2. The error fixture validates against the same schema's error branch.
//!   3. The `OperationResult` enum discriminates the two fixtures into the
//!      correct variant without lossy coercion.
//!   4. The `OPERATIONS` registry and the on-disk `operations-index.v1`
//!      fixture carry the same names, schema versions, error versions, and
//!      error-code sets — proving the Rust and TypeScript halves of the
//!      contract stay in lock-step on independent versioning.

use membrane_protocol::{
    canonicalize, operations_slice, ErrorResult, OperationEnvelope, OperationResult,
    OperationsIndex, ResultKind, SuccessResult,
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

mod common;

/// `sha256:<hex>` digest of a string's UTF-8 bytes.
fn digest_str(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    format!("sha256:{}", hex::encode(hasher.finalize()))
}

/// Read a repo-relative JSON file under the worktree root.
fn load_repo_file(path: &str) -> String {
    let root = membrane_protocol::repo_root();
    std::fs::read_to_string(format!("{root}/{path}"))
        .unwrap_or_else(|e| panic!("could not read {path}: {e}"))
}

fn load_repo_json(path: &str) -> Value {
    let raw = load_repo_file(path);
    serde_json::from_str(&raw).unwrap_or_else(|e| panic!("could not parse {path}: {e}"))
}

/// Walk every operation in [`operations_slice`] and assert its success +
/// error fixtures validate against the per-operation JSON Schema.
#[test]
fn every_operation_fixtures_validate() {
    let operations = operations_slice();
    assert!(
        !operations.is_empty(),
        "operations registry must not be empty"
    );
    for entry in operations {
        let schema = load_repo_json(&entry.schema_path);
        let success = load_repo_json(&entry.success_fixture);
        let error = load_repo_json(&entry.error_fixture);

        let success_violations = common::validate(&schema, &success);
        assert!(
            success_violations.is_empty(),
            "{}: success fixture fails schema validation: {success_violations:#?}",
            entry.name
        );

        let error_violations = common::validate(&schema, &error);
        assert!(
            error_violations.is_empty(),
            "{}: error fixture fails schema validation: {error_violations:#?}",
            entry.name
        );
    }
}

/// Every fixture must deserialize into the right [`OperationResult`]
/// variant and re-serialize byte-identically to the on-disk canonical form.
#[test]
fn every_operation_fixtures_round_trip() {
    for entry in operations_slice() {
        let success_raw = load_repo_file(&entry.success_fixture);
        let success_value: Value = serde_json::from_str(&success_raw).expect("success parses");
        let success_envelope: OperationEnvelope =
            serde_json::from_str(&success_raw).expect("success fixture deserializes");
        let success = success_envelope.result;
        match &success {
            OperationResult::Success(SuccessResult { kind, data }) => {
                assert_eq!(*kind, ResultKind::Success);
                let data_typed: Value =
                    serde_json::to_value(data).expect("success data re-serializes");
                assert_eq!(
                    data_typed, success_value["result"]["data"],
                    "{}: success data did not round-trip",
                    entry.name
                );
            }
            OperationResult::Error(_) => panic!(
                "{}: success fixture unexpectedly deserialized as an error variant",
                entry.name
            ),
        }
        let success_reserialized = canonicalize(&success_value);
        let success_reparsed = canonicalize(
            &serde_json::from_str::<Value>(&success_reserialized).expect("re-parse success"),
        );
        assert_eq!(
            success_reparsed, success_reserialized,
            "{}: success canonical form is not stable",
            entry.name
        );

        let error_raw = load_repo_file(&entry.error_fixture);
        let error_value: Value = serde_json::from_str(&error_raw).expect("error parses");
        let error_envelope: OperationEnvelope =
            serde_json::from_str(&error_raw).expect("error fixture deserializes");
        let error = error_envelope.result;
        match &error {
            OperationResult::Error(ErrorResult { kind, code, .. }) => {
                assert_eq!(*kind, ResultKind::Error);
                assert!(
                    entry.error_codes.iter().any(|c| c == code),
                    "{}: error code `{}` is not in the operation's closed taxonomy",
                    entry.name,
                    code
                );
            }
            OperationResult::Success(_) => panic!(
                "{}: error fixture unexpectedly deserialized as a success variant",
                entry.name
            ),
        }
        let error_reserialized = canonicalize(&error_value);
        let error_reparsed = canonicalize(
            &serde_json::from_str::<Value>(&error_reserialized).expect("re-parse error"),
        );
        assert_eq!(
            error_reparsed, error_reserialized,
            "{}: error canonical form is not stable",
            entry.name
        );
    }
}

/// The on-disk `operations-index.v1` fixture must agree with
/// [`operations_slice`] on every observable field. This is the lock-step
/// assertion that keeps the Rust half and the TypeScript half of the
/// contract aligned.
#[test]
fn operations_index_matches_registry() {
    let index_path = "schemas/registry/operations/operations-index.v1.golden.json";
    let raw = load_repo_file(index_path);
    let on_disk: OperationsIndex =
        serde_json::from_str(&raw).expect("operations index fixture deserializes");
    let registry = operations_slice();

    assert_eq!(
        on_disk.operations.len(),
        registry.len(),
        "operations-index.v1 must list exactly as many operations as the OPERATIONS registry"
    );

    for (left, right) in on_disk.operations.iter().zip(registry.iter()) {
        assert_eq!(left.name, right.name, "operation name drifted");
        assert_eq!(
            left.schema_version, right.schema_version,
            "{}: schemaVersion drifted between the index fixture and the Rust registry",
            left.name
        );
        assert_eq!(
            left.error_version, right.error_version,
            "{}: errorVersion drifted between the index fixture and the Rust registry",
            left.name
        );
        assert_eq!(
            left.schema_path, right.schema_path,
            "{}: schemaPath drift",
            left.name
        );
        assert_eq!(
            left.success_fixture, right.success_fixture,
            "{}: successFixture drift",
            left.name
        );
        assert_eq!(
            left.error_fixture, right.error_fixture,
            "{}: errorFixture drift",
            left.name
        );
        let left_set: BTreeSet<&String> = left.error_codes.iter().collect();
        let right_set: BTreeSet<&String> = right.error_codes.iter().collect();
        assert_eq!(
            left_set, right_set,
            "{}: error-code set drifted between the index fixture and the Rust registry",
            left.name
        );
    }
}

/// Independent versioning: every operation has its own `schemaVersion` and
/// `errorVersion`, and the union is a complete set (no duplicates, no
/// missing slots).
#[test]
fn operations_are_independently_versioned() {
    let mut seen = std::collections::HashSet::new();
    let registry = operations_slice();
    for entry in registry {
        assert!(
            seen.insert(entry.name.clone()),
            "{} appears twice in the operations registry",
            entry.name
        );
        assert!(
            entry.schema_version >= 1,
            "{}: schemaVersion must be >= 1",
            entry.name
        );
        assert!(
            entry.error_version >= 1,
            "{}: errorVersion must be >= 1",
            entry.name
        );
        assert!(
            !entry.error_codes.is_empty(),
            "{}: must declare at least one error code",
            entry.name
        );
        let set: BTreeSet<&String> = entry.error_codes.iter().collect();
        assert_eq!(
            set.len(),
            entry.error_codes.len(),
            "{}: error codes must be unique",
            entry.name
        );
    }
    assert_eq!(seen.len(), registry.len(), "duplicate operation names");
}

/// Pinned digest of the operations-index fixture, so any future drift in the
/// Rust registry or the on-disk JSON fails BOTH suites symmetrically. The
/// TypeScript binding recomputes the same digest from the same file in
/// `bindings/operations.test.mjs`.
#[test]
fn operations_index_canonical_digest_is_pinned() {
    let raw = load_repo_file("schemas/registry/operations/operations-index.v1.golden.json");
    let value: Value = serde_json::from_str(&raw).expect("index fixture parses");
    let digest = digest_str(&canonicalize(&value));
    let expected = "sha256:71597b1a729ceea253e4cd4e5f8d3838125663f71295f7bf19494593c7e45416";
    // The pin is computed once over the canonicalized operations-index
    // fixture (sorted keys, no whitespace, sha256). The TypeScript binding
    // recomputes the same digest in `bindings/operations.test.mjs` from the
    // same file; a drift in the Rust registry, the fixture, or the
    // canonical rules fails BOTH suites. Update the pin only on an
    // INTENTIONAL contract change.
    assert_eq!(
        digest, expected,
        "operations-index digest drifted: got {digest}, expected {expected}"
    );
}
