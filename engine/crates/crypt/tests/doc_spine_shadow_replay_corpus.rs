use std::{collections::HashSet, fs, path::Path};

use crypt::doc_shadow::{evaluate_frozen_shadow_replay, ShadowReplayCaseV1, ShadowReplayReportV1};
use serde::Deserialize;
use sha2::{Digest, Sha256};

const FIXTURE_ROOT: &str = "tests/fixtures/doc-spine-shadow-replay-v1";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FrozenCorpusManifestV1 {
    schema_version: String,
    fixture_status: String,
    repository_id: String,
    revision: String,
    index_generation: u64,
    sources: Vec<FrozenSourceV1>,
    cases: Vec<ShadowReplayCaseV1>,
    expected_report: ShadowReplayReportV1,
    h2_gate: H2GateDispositionV1,
    safety_leak_checks: SafetyLeakChecksV1,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FrozenSourceV1 {
    doc_id: String,
    path: String,
    sha256: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct H2GateDispositionV1 {
    passed: bool,
    disposition: String,
    reason: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SafetyLeakChecksV1 {
    cross_repository_candidate_count: usize,
    secret_leakage_count: usize,
    authority_escape_count: usize,
    superseded_leakage_count: usize,
    duplicate_leakage_count: usize,
}

fn fixture_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(FIXTURE_ROOT)
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[test]
fn frozen_doc_spine_shadow_replay_corpus_v1_is_deterministic_shadow_only_and_safe() {
    let root = fixture_root();
    let manifest: FrozenCorpusManifestV1 = serde_json::from_slice(
        &fs::read(root.join("manifest.json")).expect("frozen replay manifest reads"),
    )
    .expect("frozen replay manifest parses");

    assert_eq!(
        manifest.schema_version,
        "crypt.doc_spine_shadow_replay_fixture.v1"
    );
    assert_eq!(
        manifest.fixture_status,
        "synthetic_test_fixture_not_operational_evidence"
    );
    assert_eq!(manifest.repository_id, "fixture/doc-spine-shadow-v1");
    assert_eq!(manifest.revision, "fixture@doc-spine-shadow-v1");
    assert_eq!(manifest.index_generation, 1);

    let source_ids = manifest
        .sources
        .iter()
        .map(|source| source.doc_id.as_str())
        .collect::<HashSet<_>>();
    assert_eq!(
        source_ids.len(),
        manifest.sources.len(),
        "source ids are unique"
    );
    for source in &manifest.sources {
        assert!(
            !source.path.contains(".."),
            "source path stays fixture-local"
        );
        assert!(
            !source.path.starts_with("Health/"),
            "Health material is never eligible for Doc Spine replay"
        );
        let bytes = fs::read(root.join(&source.path)).expect("frozen source reads");
        assert_eq!(
            sha256_hex(&bytes),
            source.sha256,
            "{} hash drifted",
            source.path
        );
        let text = String::from_utf8(bytes).expect("fixture sources are utf-8");
        assert!(
            text.contains("Synthetic"),
            "source remains a synthetic test fixture"
        );
        let lower = text.to_ascii_lowercase();
        assert!(
            !lower.contains("api_key="),
            "fixture source contains credential-shaped text"
        );
        assert!(
            !lower.contains("password="),
            "fixture source contains credential-shaped text"
        );
        assert!(
            !lower.contains("ignore previous instructions"),
            "fixture source contains an authority-escape instruction"
        );
    }

    for case in &manifest.cases {
        let baseline_ids = case
            .baseline
            .iter()
            .map(|candidate| candidate.doc_id.as_str())
            .collect::<HashSet<_>>();
        for candidate in &case.with_docs {
            assert!(
                source_ids.contains(candidate.doc_id.as_str())
                    || baseline_ids.contains(candidate.doc_id.as_str()),
                "new +docs candidate must be from this frozen repository"
            );
            assert!(
                !candidate.superseded,
                "superseded candidate leaked into +docs"
            );
            assert!(
                !candidate.duplicate,
                "duplicate candidate leaked into +docs"
            );
        }
    }

    let receipt = evaluate_frozen_shadow_replay(manifest.cases.clone());
    assert_eq!(receipt.schema_version, "crypt.doc_shadow_receipt.v1");
    assert_eq!(receipt.report, manifest.expected_report);
    assert_eq!(receipt.report.displacement_count, 0);
    assert_eq!(receipt.report.superseded_leakage_count, 0);
    assert_eq!(receipt.report.duplicate_leakage_count, 0);

    assert!(
        !manifest.h2_gate.passed,
        "synthetic corpus cannot close deployed H2 gate"
    );
    assert_eq!(manifest.h2_gate.disposition, "RemainDocumentLevel");
    assert!(manifest.h2_gate.reason.contains("not deployed"));
    assert_eq!(
        manifest.safety_leak_checks.cross_repository_candidate_count,
        0
    );
    assert_eq!(manifest.safety_leak_checks.secret_leakage_count, 0);
    assert_eq!(manifest.safety_leak_checks.authority_escape_count, 0);
    assert_eq!(
        manifest.safety_leak_checks.superseded_leakage_count,
        receipt.report.superseded_leakage_count
    );
    assert_eq!(
        manifest.safety_leak_checks.duplicate_leakage_count,
        receipt.report.duplicate_leakage_count
    );
}
