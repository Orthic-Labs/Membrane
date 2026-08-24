//! Scenarios 9–11: multiwriter convergence, CLI-shaped pure handlers,
//! benchmark digest stability feeding the precision gate.

use std::collections::BTreeSet;

use membrane_adapt::authority::PrecedenceTier;
use membrane_adapt::benchmark::{run_benchmark, LabelledCase};
use membrane_adapt::cli_api::*;
use membrane_adapt::insights::{EventKind, TranscriptEventV1};
use membrane_adapt::multiwriter::{converge, WriterRecord};
use membrane_adapt::record::RuleKey;

fn ev(id: &str, session: &str, text: &str) -> TranscriptEventV1 {
    TranscriptEventV1 {
        event_id: id.into(),
        session_id: session.into(),
        host: "pi".into(),
        provenance: "external_user".into(),
        kind: EventKind::UserMessage,
        text: text.into(),
        timestamp: Some("2026-08-24T01:00:00Z".into()),
        byte_start: 0,
        byte_end: text.len() as i64,
        call_id: None,
        occurrence: 0,
        evidence_eligible: true,
    }
}

#[test]
fn multiwriter_convergence_is_permutation_invariant() {
    let rec = |w: &str, l: u64, rule: &str, d: &str, t: PrecedenceTier| WriterRecord {
        writer_id: w.into(),
        lamport: l,
        rule_key: RuleKey::new("workflow", rule),
        payload_digest: d.into(),
        precedence_tier: t,
    };
    let set = vec![
        rec("w1", 1, "always run tests", "d1", PrecedenceTier::ExplicitGlobalUserPreference),
        rec("w2", 2, "always run tests", "d2", PrecedenceTier::ExplicitGlobalUserPreference),
        rec("w3", 3, "always run tests", "d3", PrecedenceTier::ProvisionalCandidate),
    ];
    let mut reversed = set.clone();
    reversed.reverse();
    let forward = converge(&set);
    let backward = converge(&reversed);
    let key = RuleKey::new("workflow", "always run tests");
    assert_eq!(
        forward.winner(&key).map(|(d, _)| d),
        backward.winner(&key).map(|(d, _)| d)
    );
    assert_eq!(forward.conflicts_for(&key), backward.conflicts_for(&key));
}

#[test]
fn cli_handlers_are_deterministic_and_versioned() {
    let req = MineRequest {
        events: vec![
            ev("a", "s1", "please run the full test suite before claiming done"),
            ev("b", "s2", "Please run the FULL test suite before claiming done."),
        ],
        min_recurrence: 2,
    };
    let r1 = handle_mine(&req);
    let r2 = handle_mine(&req);
    assert_eq!(
        serde_json::to_string(&r1).unwrap(),
        serde_json::to_string(&r2).unwrap()
    );
    assert_eq!(r1.api_version, "adapt.cli.v1");
}

#[test]
fn doctor_flags_corrupt_state() {
    let mined = handle_mine(&MineRequest {
        events: vec![
            ev("a", "s1", "please run the full test suite before claiming done"),
            ev("b", "s2", "Please run the FULL test suite before claiming done."),
        ],
        min_recurrence: 2,
    });
    // Tamper: drop the episode evidence to create a dangling reference.
    let mut broken = mined.issues.clone();
    broken[0].episode_ids.push("ghost".into());
    let resp = handle_doctor(&DoctorRequest { issues: broken, episodes: mined.episodes });
    assert!(!resp.healthy);
    assert!(resp.findings.iter().any(|f| f.code == "dangling_episode_ref"));
}

#[test]
fn benchmark_digest_is_stable_and_scores_families() {
    let mk_corpus = || {
        vec![LabelledCase {
            case_id: "c1".into(),
            events: vec![
                ev("a", "s1", "this is so frustrating and annoying"),
                ev("b", "s2", "this is so frustrating and annoying"),
            ],
            expected_families: BTreeSet::from(["visible_frustration".to_string()]),
            forbidden_families: BTreeSet::new(),
        }]
    };
    let report = run_benchmark(&mk_corpus());
    assert_eq!(report.report_digest, run_benchmark(&mk_corpus()).report_digest);
    let frustration = report.by_family.get("visible_frustration").unwrap();
    assert_eq!(frustration.true_positives, 1);
    assert!(frustration.precision() > 0.0);
    // Precision gate consumption: family at 1.0 passes.
    let p = membrane_adapt::remediation::RemediationProposalV1::build(
        "i",
        "visible_frustration",
        membrane_adapt::remediation::RemediationEffect::ProcessChange,
        "text",
        vec![],
    );
    assert!(p.precision_gate(Some(frustration.precision())).is_ok());
}
