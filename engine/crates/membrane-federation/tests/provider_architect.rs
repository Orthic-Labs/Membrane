use membrane_federation::migrate_decisions::{read_decisions, DecisionJsonlReader, DecisionMatch};
use membrane_federation::providers::architect::{normalize_decision, DecisionNormalizationError};
use membrane_provider_sdk::DecisionRecord;

fn record() -> DecisionRecord {
    DecisionRecord {
        id: "architect:decision:abc".into(),
        repository_id: "repo-a".into(),
        generation: "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
        source_hash: "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into(),
        rationale: "keep one planner".into(),
        alternatives: vec!["duplicate planner".into()],
        risks: vec!["migration drift".into()],
    }
}

#[test]
fn decision_normalization_preserves_identity_and_provenance() {
    let candidate = normalize_decision(&record(), "repo-a", Some(&record().generation)).expect("valid decision");
    assert_eq!(candidate.id, "architect:decision:abc");
    assert_eq!(candidate.provider.as_deref(), Some("architect"));
    assert_eq!(candidate.source_hash, record().source_hash);
    assert!(candidate.text.contains("keep one planner"));
    assert!(candidate.text.contains("migration drift"));
}

#[test]
fn decision_normalization_rejects_scope_or_generation_drift() {
    assert_eq!(normalize_decision(&record(), "other", None), Err(DecisionNormalizationError::RepositoryMismatch));
    assert_eq!(normalize_decision(&record(), "repo-a", Some("sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc")), Err(DecisionNormalizationError::GenerationMismatch));
}

#[test]
fn jsonl_reader_is_read_only_and_line_diagnostic() {
    let path = std::env::temp_dir().join(format!("membrane-decisions-{}.jsonl", std::process::id()));
    let body = r#"{"schemaVersion":1,"id":"d1","repositoryId":"repo-a","linkedGraphGeneration":"g1","rationale":"use typed source","alternatives":["python"],"risks":["drift"],"currentStatus":"accepted"}
not-json
"#;
    std::fs::write(&path, body).expect("fixture write");
    let report = read_decisions(&path).expect("read");
    assert_eq!(report.records.len(), 1);
    assert!(!report.complete);
    assert_eq!(report.diagnostics[0].line, 2);
    assert_eq!(report.evidence[0].lifecycle.as_deref(), Some("accepted"));
    let _ = std::fs::remove_file(path);
}

#[test]
fn jsonl_matching_preserves_scope_lifecycle_mode_and_revisit_provenance() {
    let path = std::env::temp_dir().join(format!("membrane-decisions-match-{}.jsonl", std::process::id()));
    let body = r#"{"schemaVersion":1,"id":"d2","repositoryId":"repo-a","scopeId":"scope-a","linkedGraphGeneration":"g2","rationale":"typed decisions","alternatives":[],"risks":[],"revisitTriggers":["graph drift"],"provenance":["adr:2"],"currentStatus":"accepted","mode":"edit"}
{"schemaVersion":1,"id":"d3","repositoryId":"repo-a","scopeId":"scope-b","linkedGraphGeneration":"g2","rationale":"wrong scope","currentStatus":"accepted","mode":"edit"}
"#;
    std::fs::write(&path, body).expect("fixture write");
    let matching = DecisionMatch::new("repo-a", "scope-a").generation("g2").lifecycle("accepted").mode("edit");
    let report = DecisionJsonlReader::new().read_matching(&path, &matching).expect("read");
    assert_eq!(report.records.len(), 1);
    assert_eq!(report.evidence[0].revisit_triggers, vec!["graph drift"]);
    assert_eq!(report.evidence[0].provenance, vec!["adr:2"]);
    let _ = std::fs::remove_file(path);
}
