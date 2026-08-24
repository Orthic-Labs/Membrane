//! Stable CLI-oriented request/response types for the Adapt core.
//!
//! Pure data + pure functions: NO process spawning, NO network, NO I/O. A
//! thin binary later binds these to stdio. Shapes are versioned and stable;
//! additive evolution only until a real consumer requires a new shape.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::benchmark::{run_benchmark, BenchmarkReportV1, LabelledCase};
use crate::insights::{recurrence::{form_issues, mine_issues}, InsightIssueV1};
use crate::insights::detectors::repeated_ask_signature;
use crate::insights::{FailureEpisodeV1, TranscriptEventV1};
use crate::manifest::PreferenceManifestV1;
use crate::outcomes::{OutcomeEntryV1, OutcomeLedger};

pub const CLI_API_VERSION: &str = "adapt.cli.v1";

/// `mine`: run detectors over supplied events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MineRequest {
    pub events: Vec<TranscriptEventV1>,
    #[serde(default = "default_min_recurrence")]
    pub min_recurrence: u32,
}

fn default_min_recurrence() -> u32 {
    2
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MineResponse {
    pub api_version: String,
    pub episodes: Vec<FailureEpisodeV1>,
    pub issues: Vec<InsightIssueV1>,
}

/// `review`: inspect pending candidates (episodes/issues/manifests) without
/// mutating anything.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewRequest {
    /// Issue IDs to review; empty reviews everything supplied.
    #[serde(default)]
    pub issue_ids: Vec<String>,
    pub issues: Vec<InsightIssueV1>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewResponse {
    pub api_version: String,
    /// Per issue: current state, recurrence, honesty limit.
    pub items: Vec<ReviewItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewItem {
    pub issue_id: String,
    pub family: String,
    pub state: String,
    pub recurrence_count: u32,
    pub honesty_limit: String,
}

/// `apply`: apply an already-validated manifest (pure validation + plan
/// computation; persistence stays outside this crate).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplyRequest {
    pub manifest: PreferenceManifestV1,
    /// Whether to compute the full apply plan or only validate.
    pub dry_run: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplyResponse {
    pub api_version: String,
    pub valid: bool,
    pub errors: Vec<String>,
    /// Sorted accepted record IDs when valid && !dry_run.
    #[serde(default)]
    pub accepted_record_ids: Vec<String>,
    #[serde(default)]
    pub manifest_hash: String,
}

/// `report`: outcome ledger reporting.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportRequest {
    #[serde(default)]
    pub issue_ids: Vec<String>,
    pub ledger_entries: Vec<OutcomeEntryV1>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportResponse {
    pub api_version: String,
    /// issue_id -> aggregate effectiveness (0.0–1.0)
    pub effectiveness: BTreeMap<String, f64>,
    /// issue_ids whose latest entry demands reopen
    pub reopen_recommended: Vec<String>,
}

/// `benchmark`: score detectors against a labelled corpus.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkRequest {
    pub corpus: Vec<LabelledCase>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkResponse {
    pub api_version: String,
    pub report: BenchmarkReportV1,
}

/// `doctor`: self-check of invariants over supplied state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoctorRequest {
    pub issues: Vec<InsightIssueV1>,
    pub episodes: Vec<FailureEpisodeV1>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoctorResponse {
    pub api_version: String,
    pub healthy: bool,
    pub findings: Vec<DoctorFinding>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoctorFinding {
    pub code: String,
    pub detail: String,
}

// ---- pure implementations ---------------------------------------------------

pub fn handle_mine(req: &MineRequest) -> MineResponse {
    let episodes = crate::insights::detectors::run_all_detectors(&req.events);
    let issues = form_issues(&episodes, req.min_recurrence);
    MineResponse {
        api_version: CLI_API_VERSION.into(),
        episodes,
        issues,
    }
}

pub fn handle_review(req: &ReviewRequest) -> ReviewResponse {
    let items = req
        .issues
        .iter()
        .filter(|i| req.issue_ids.is_empty() || req.issue_ids.contains(&i.issue_id))
        .map(|i| ReviewItem {
            issue_id: i.issue_id.clone(),
            family: i.family.clone(),
            state: format!("{:?}", i.state).to_lowercase(),
            recurrence_count: i.recurrence_count,
            honesty_limit: i.honesty_limit.clone(),
        })
        .collect();
    ReviewResponse { api_version: CLI_API_VERSION.into(), items }
}

pub fn handle_apply(req: &ApplyRequest) -> ApplyResponse {
    let mut errors: Vec<String> = Vec::new();
    if let Err(e) = crate::manifest::validate_schema(&req.manifest) {
        errors.push(e.to_string());
    }
    let accepted = if errors.is_empty() && !req.dry_run {
        match crate::manifest::apply_plan(&req.manifest) {
            Ok(ids) => ids,
            Err(e) => {
                errors.push(e.to_string());
                vec![]
            }
        }
    } else {
        vec![]
    };
    ApplyResponse {
        api_version: CLI_API_VERSION.into(),
        valid: errors.is_empty(),
        errors,
        manifest_hash: crate::manifest::manifest_hash(&req.manifest),
        accepted_record_ids: accepted,
    }
}

pub fn handle_report(req: &ReportRequest) -> ReportResponse {
    let mut ledger = OutcomeLedger::default();
    // Rebuild from supplied entries (append-only replay).
    for e in &req.ledger_entries {
        ledger.record(
            &e.issue_id,
            &e.mitigation_proposal_id,
            e.raw,
            e.exposure,
            &e.note,
        );
    }
    let ids: Vec<String> = if req.issue_ids.is_empty() {
        req.ledger_entries.iter().map(|e| e.issue_id.clone()).collect()
    } else {
        req.issue_ids.clone()
    };
    let mut effectiveness = BTreeMap::new();
    let mut reopen = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for id in ids {
        if seen.insert(id.clone()) {
            effectiveness.insert(id.clone(), ledger.aggregate_effectiveness(&id));
            if ledger.should_reopen(&id) {
                reopen.push(id);
            }
        }
    }
    ReportResponse { api_version: CLI_API_VERSION.into(), effectiveness, reopen_recommended: reopen }
}

pub fn handle_benchmark(req: &BenchmarkRequest) -> BenchmarkResponse {
    BenchmarkResponse { api_version: CLI_API_VERSION.into(), report: run_benchmark(&req.corpus) }
}

pub fn handle_doctor(req: &DoctorRequest) -> DoctorResponse {
    let mut findings = Vec::new();
    let episode_ids: std::collections::BTreeSet<&str> =
        req.episodes.iter().map(|e| e.episode_id.as_str()).collect();
    for issue in &req.issues {
        for ep in &issue.episode_ids {
            if !episode_ids.contains(ep.as_str()) {
                findings.push(DoctorFinding {
                    code: "dangling_episode_ref".into(),
                    detail: format!("issue {} references missing episode {}", issue.issue_id, ep),
                });
            }
        }
        if issue.recurrence_count < 2 {
            findings.push(DoctorFinding {
                code: "under_recruited_issue".into(),
                detail: format!("issue {} has recurrence {}", issue.issue_id, issue.recurrence_count),
            });
        }
        if issue.honesty_limit != crate::insights::HONESTY_LIMIT {
            findings.push(DoctorFinding {
                code: "honesty_limit_drift".into(),
                detail: format!("issue {} carries a non-canonical honesty limit", issue.issue_id),
            });
        }
    }
    DoctorResponse {
        api_version: CLI_API_VERSION.into(),
        healthy: findings.is_empty(),
        findings,
    }
}

/// Expose the deterministic repeated-ask signature for CLI parity checks.
pub fn signature_for(text: &str) -> String {
    repeated_ask_signature(text)
}

/// Convenience wrapper mirroring Python's mine_issues entry point.
pub fn mine_issues_from_events(events: &[TranscriptEventV1], min_recurrence: u32) -> Vec<InsightIssueV1> {
    mine_issues(events, min_recurrence)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::insights::EventKind;

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
    fn mine_roundtrip_is_deterministic() {
        let req = MineRequest {
            events: vec![
                ev("a", "s1", "please run the full test suite before claiming done"),
                ev("b", "s2", "Please run the FULL test suite before claiming done."),
            ],
            min_recurrence: 2,
        };
        let r1 = handle_mine(&req);
        let r2 = handle_mine(&req);
        assert_eq!(serde_json::to_string(&r1).unwrap(), serde_json::to_string(&r2).unwrap());
        assert_eq!(r1.api_version, "adapt.cli.v1");
        assert!(r1.episodes.iter().any(|e| e.family == "repeated_ask"));
    }

    #[test]
    fn review_reports_state_and_honesty() {
        let mined = handle_mine(&MineRequest {
            events: vec![
                ev("a", "s1", "please run the full test suite before claiming done"),
                ev("b", "s2", "Please run the FULL test suite before claiming done."),
            ],
            min_recurrence: 2,
        });
        let resp = handle_review(&ReviewRequest { issue_ids: vec![], issues: mined.issues });
        assert_eq!(resp.items.len(), 1);
        assert_eq!(resp.items[0].state, "observed");
        assert!(!resp.items[0].honesty_limit.is_empty());
    }

    #[test]
    fn doctor_detects_dangling_refs_and_drift() {
        let bad_issue = InsightIssueV1 {
            schema_version: "adapt.insight-issue.v1".into(),
            issue_id: "i1".into(),
            family: "f".into(),
            recurrence_signature: "sig".into(),
            canonical_description: "d".into(),
            applicability: Default::default(),
            episode_ids: vec!["ghost".into()],
            recurrence_count: 5,
            distinct_sessions: 2,
            first_seen: None,
            last_seen: None,
            confidence: 0.5,
            state: crate::insights::IssueState::Observed,
            candidate_mechanisms: vec![],
            mitigation_links: vec![],
            recurrence_after_mitigation: 0,
            honesty_limit: "custom".into(),
        };
        let resp = handle_doctor(&DoctorRequest { issues: vec![bad_issue], episodes: vec![] });
        assert!(!resp.healthy);
        assert!(resp.findings.iter().any(|f| f.code == "dangling_episode_ref"));
        assert!(resp.findings.iter().any(|f| f.code == "honesty_limit_drift"));
    }
}
