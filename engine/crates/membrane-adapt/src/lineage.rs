//! Read-only Learning Lineage and Insights projections.
//!
//! The projection joins identities and receipts already carried by Adapt
//! inputs. It owns no store, creates no semantic prose, and emits typed gaps
//! whenever a host-side receipt is not present.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::insights::sealed_issue::SealedInsightIssueV1;
use crate::insights::{FailureEpisodeV1, InsightIssueV1, TranscriptEventV1};
use crate::outcomes::OutcomeEntryV1;
use crate::procedural_effectiveness::ProceduralAssetEffectivenessV1;
use crate::remediation::SealedRemediationProposalV1;

pub const LEARNING_LINEAGE_SCHEMA: &str = "adapt.learning-lineage.v1";
pub const INSIGHTS_PROJECTION_SCHEMA: &str = "adapt.insights-projection.v1";

/// A coverage reason that can safely be shown to an operator. Missing data is
/// never represented as zero or as a synthetic success/failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LineageUnavailableReason {
    NotInstrumented,
    HubInactive,
    ProviderOmitted,
    HostUnsupported,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineageCoverageGapV1 {
    /// Stable field path, for example `variant`, `experiment`, or
    /// `issue.seal_receipt`.
    pub field: String,
    pub reason: LineageUnavailableReason,
}

/// Stages in canonical Learning Lineage order. Receipt IDs remain metadata on
/// each observed node rather than being mistaken for new semantic objects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LineageStage {
    Experience,
    TasteEvidence,
    Episode,
    Insight,
    Proposal,
    Variant,
    Experiment,
    Deployment,
    Outcome,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineageRefV1 {
    pub stage: LineageStage,
    pub id: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub receipt_ids: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineageEdgeV1 {
    pub from: String,
    pub relation: String,
    pub to: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LearningLineageV1 {
    pub schema_version: String,
    pub item_id: String,
    pub nodes: Vec<LineageRefV1>,
    pub edges: Vec<LineageEdgeV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub coverage: Vec<LineageCoverageGapV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InsightProjectionRowV1 {
    pub issue_id: String,
    pub family: String,
    pub canonical_description: String,
    pub state: String,
    pub recurrence_count: u32,
    pub episode_ids: Vec<String>,
    pub proposal_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub receipt_ids: Vec<String>,
    pub first_seen: Option<String>,
    pub last_seen: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub coverage: Vec<LineageCoverageGapV1>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InsightsProjectionV1 {
    pub schema_version: String,
    pub rows: Vec<InsightProjectionRowV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub coverage: Vec<LineageCoverageGapV1>,
}

/// Inputs accepted by the pure read model. All fields are caller-provided
/// snapshots; this type has no persistence or host I/O behavior.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LineageInputV1 {
    #[serde(default)]
    pub events: Vec<TranscriptEventV1>,
    #[serde(default)]
    pub episodes: Vec<FailureEpisodeV1>,
    #[serde(default)]
    pub issues: Vec<InsightIssueV1>,
    #[serde(default)]
    pub sealed_issues: Vec<SealedInsightIssueV1>,
    #[serde(default)]
    pub remediation_proposals: Vec<SealedRemediationProposalV1>,
    #[serde(default)]
    pub outcomes: Vec<OutcomeEntryV1>,
    #[serde(default)]
    pub asset_effectiveness: Vec<ProceduralAssetEffectivenessV1>,
}

impl LineageInputV1 {
    /// Build the production `mine` snapshot. Host experiment/deployment
    /// receipts are intentionally absent because this call has no such input.
    pub fn from_mine(
        events: &[TranscriptEventV1],
        episodes: &[FailureEpisodeV1],
        issues: &[InsightIssueV1],
        remediation_proposals: &[SealedRemediationProposalV1],
    ) -> Self {
        Self {
            events: events.to_vec(),
            episodes: episodes.to_vec(),
            issues: issues.to_vec(),
            sealed_issues: Vec::new(),
            remediation_proposals: remediation_proposals.to_vec(),
            outcomes: Vec::new(),
            asset_effectiveness: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MineProjectionV1 {
    pub lineage: Vec<LearningLineageV1>,
    pub insights: InsightsProjectionV1,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub asset_effectiveness: Vec<ProceduralAssetEffectivenessV1>,
}

fn issue_state(issue: &InsightIssueV1) -> String {
    format!("{:?}", issue.state).to_lowercase()
}

fn digest<T: Serialize>(value: &T) -> String {
    crate::canonical::sha256_canonical(
        &serde_json::to_value(value).expect("lineage source serializes"),
    )
}

fn node_key(node: &LineageRefV1) -> String {
    format!("{}:{:?}:{}", node.stage as u8, node.stage, node.id)
}

fn push_node(nodes: &mut BTreeMap<String, LineageRefV1>, node: LineageRefV1) {
    let key = node_key(&node);
    if let Some(existing) = nodes.get_mut(&key) {
        for receipt in node.receipt_ids {
            if !existing.receipt_ids.contains(&receipt) {
                existing.receipt_ids.push(receipt);
            }
        }
        if existing.digest.is_none() {
            existing.digest = node.digest;
        }
    } else {
        nodes.insert(key, node);
    }
}

fn receipt_ids<I>(receipts: I) -> Vec<String>
where
    I: IntoIterator<Item = String>,
{
    let mut ids = Vec::new();
    for id in receipts {
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    ids
}

fn add_edge(edges: &mut BTreeSet<(String, String, String)>, from: &str, relation: &str, to: &str) {
    edges.insert((from.to_string(), relation.to_string(), to.to_string()));
}

fn gap(field: impl Into<String>, reason: LineageUnavailableReason) -> LineageCoverageGapV1 {
    LineageCoverageGapV1 {
        field: field.into(),
        reason,
    }
}

fn proposal_ids_for_issue(
    issue: &InsightIssueV1,
    proposals: &[SealedRemediationProposalV1],
) -> Vec<String> {
    let mut ids: BTreeSet<String> = issue.mitigation_links.iter().cloned().collect();
    for proposal in proposals {
        if proposal
            .payload
            .source_issue_ids
            .iter()
            .any(|source| source == &issue.issue_id)
        {
            ids.insert(proposal.proposal_id.clone());
        }
    }
    ids.into_iter().collect()
}

fn issue_receipts(
    issue: &InsightIssueV1,
    sealed_issues: &[SealedInsightIssueV1],
) -> (Vec<String>, Option<String>) {
    let Some(sealed) = sealed_issues
        .iter()
        .find(|candidate| candidate.issue_id == issue.issue_id)
    else {
        return (Vec::new(), None);
    };
    (
        receipt_ids(
            sealed
                .state
                .receipts
                .iter()
                .map(|receipt| receipt.receipt_id.clone()),
        ),
        Some(sealed.payload_sha256.clone()),
    )
}

fn proposal_node(proposal: &SealedRemediationProposalV1) -> LineageRefV1 {
    LineageRefV1 {
        stage: LineageStage::Proposal,
        id: proposal.proposal_id.clone(),
        receipt_ids: receipt_ids(
            proposal
                .state
                .receipts
                .iter()
                .map(|receipt| receipt.receipt_id.clone()),
        ),
        digest: Some(proposal.payload_sha256.clone()),
    }
}

fn outcome_node(outcome: &OutcomeEntryV1) -> LineageRefV1 {
    LineageRefV1 {
        stage: LineageStage::Outcome,
        id: outcome.entry_id.clone(),
        receipt_ids: Vec::new(),
        digest: Some(digest(outcome)),
    }
}

/// Build one lineage read model per mined Insight issue.
pub fn build_lineage(input: &LineageInputV1) -> Vec<LearningLineageV1> {
    let episodes_by_id: BTreeMap<&str, &FailureEpisodeV1> = input
        .episodes
        .iter()
        .map(|episode| (episode.episode_id.as_str(), episode))
        .collect();
    let events_by_id: BTreeMap<&str, &TranscriptEventV1> = input
        .events
        .iter()
        .map(|event| (event.event_id.as_str(), event))
        .collect();
    let proposals_by_id: BTreeMap<&str, &SealedRemediationProposalV1> = input
        .remediation_proposals
        .iter()
        .map(|proposal| (proposal.proposal_id.as_str(), proposal))
        .collect();
    let mut lineages = Vec::new();

    for issue in &input.issues {
        let mut nodes = BTreeMap::new();
        let mut edges = BTreeSet::new();
        let mut coverage = Vec::new();
        let issue_key = format!("issue:{}", issue.issue_id);
        let (issue_receipt_ids, issue_digest) = issue_receipts(issue, &input.sealed_issues);
        push_node(
            &mut nodes,
            LineageRefV1 {
                stage: LineageStage::Insight,
                id: issue.issue_id.clone(),
                receipt_ids: issue_receipt_ids.clone(),
                digest: issue_digest,
            },
        );
        if issue_receipt_ids.is_empty() {
            coverage.push(gap(
                "issue.seal_receipt",
                LineageUnavailableReason::NotInstrumented,
            ));
        }

        for episode_id in &issue.episode_ids {
            let episode_key = format!("episode:{episode_id}");
            let Some(episode) = episodes_by_id.get(episode_id.as_str()) else {
                coverage.push(gap(
                    format!("episode:{episode_id}"),
                    LineageUnavailableReason::ProviderOmitted,
                ));
                continue;
            };
            push_node(
                &mut nodes,
                LineageRefV1 {
                    stage: LineageStage::Episode,
                    id: episode.episode_id.clone(),
                    receipt_ids: Vec::new(),
                    digest: Some(digest(*episode)),
                },
            );
            add_edge(&mut edges, &issue_key, "formed_from", &episode_key);
            for evidence in &episode.evidence {
                let event_key = format!("experience:{}", evidence.event_id);
                if let Some(event) = events_by_id.get(evidence.event_id.as_str()) {
                    push_node(
                        &mut nodes,
                        LineageRefV1 {
                            stage: LineageStage::Experience,
                            id: event.event_id.clone(),
                            receipt_ids: Vec::new(),
                            digest: Some(digest(*event)),
                        },
                    );
                    add_edge(&mut edges, &episode_key, "observed_in", &event_key);
                } else {
                    coverage.push(gap(
                        format!("experience.event:{}", evidence.event_id),
                        LineageUnavailableReason::ProviderOmitted,
                    ));
                }
            }
        }

        for proposal_id in proposal_ids_for_issue(issue, &input.remediation_proposals) {
            let proposal_key = format!("proposal:{proposal_id}");
            let Some(proposal) = proposals_by_id.get(proposal_id.as_str()) else {
                coverage.push(gap(
                    format!("proposal:{proposal_id}"),
                    LineageUnavailableReason::ProviderOmitted,
                ));
                continue;
            };
            push_node(&mut nodes, proposal_node(proposal));
            add_edge(&mut edges, &issue_key, "proposes", &proposal_key);
            if proposal.payload.user_evidence.is_none()
                && proposal.payload.proposal_kind == "taste_candidate"
            {
                coverage.push(gap(
                    format!("proposal:{proposal_id}.user_evidence"),
                    LineageUnavailableReason::ProviderOmitted,
                ));
            }
            if let Some(evidence) = &proposal.payload.user_evidence {
                for signal in &evidence.signals {
                    let evidence_key = format!("taste-evidence:{}", signal.evidence_digest);
                    push_node(
                        &mut nodes,
                        LineageRefV1 {
                            stage: LineageStage::TasteEvidence,
                            id: signal.evidence_digest.clone(),
                            receipt_ids: Vec::new(),
                            digest: None,
                        },
                    );
                    add_edge(&mut edges, &proposal_key, "backed_by", &evidence_key);
                }
            }
        }

        let mut matched_outcomes = 0usize;
        for outcome in &input.outcomes {
            if outcome.issue_id != issue.issue_id {
                continue;
            }
            let outcome_key = format!("outcome:{}", outcome.entry_id);
            push_node(&mut nodes, outcome_node(outcome));
            let target_key = format!("proposal:{}", outcome.mitigation_proposal_id);
            let relation_from =
                if proposals_by_id.contains_key(outcome.mitigation_proposal_id.as_str()) {
                    target_key
                } else {
                    coverage.push(gap(
                        format!("outcome:{}.proposal", outcome.entry_id),
                        LineageUnavailableReason::ProviderOmitted,
                    ));
                    issue_key.clone()
                };
            add_edge(&mut edges, &relation_from, "measured_by", &outcome_key);
            matched_outcomes += 1;
        }
        if matched_outcomes == 0 {
            coverage.push(gap("outcome", LineageUnavailableReason::NotInstrumented));
        }

        // No host-side H7 input is accepted here, so these stages remain
        // explicit gaps instead of fabricated variant/experiment/deployment
        // records.
        for field in ["variant", "experiment", "deployment"] {
            coverage.push(gap(field, LineageUnavailableReason::NotInstrumented));
        }
        coverage.sort_by(|a, b| a.field.cmp(&b.field).then(a.reason.cmp(&b.reason)));
        coverage.dedup();

        lineages.push(LearningLineageV1 {
            schema_version: LEARNING_LINEAGE_SCHEMA.into(),
            item_id: issue.issue_id.clone(),
            nodes: nodes.into_values().collect(),
            edges: edges
                .into_iter()
                .map(|(from, relation, to)| LineageEdgeV1 { from, relation, to })
                .collect(),
            coverage,
        });
    }

    lineages.sort_by(|a, b| a.item_id.cmp(&b.item_id));
    lineages
}

/// Build the operator-facing Insights read model from the same source
/// snapshots. Descriptions and timestamps are copied from issues; no summary
/// or diagnosis is authored by this projection.
pub fn build_insights_projection(input: &LineageInputV1) -> InsightsProjectionV1 {
    let mut rows = Vec::new();
    let mut coverage = Vec::new();
    for issue in &input.issues {
        let proposal_ids = proposal_ids_for_issue(issue, &input.remediation_proposals);
        let (receipt_ids, _) = issue_receipts(issue, &input.sealed_issues);
        let mut row_coverage = Vec::new();
        if issue.first_seen.is_none() {
            row_coverage.push(gap(
                "observed.first_seen",
                LineageUnavailableReason::NotInstrumented,
            ));
        }
        if issue.last_seen.is_none() {
            row_coverage.push(gap(
                "observed.last_seen",
                LineageUnavailableReason::NotInstrumented,
            ));
        }
        if receipt_ids.is_empty() {
            row_coverage.push(gap(
                "issue.seal_receipt",
                LineageUnavailableReason::NotInstrumented,
            ));
        }
        if proposal_ids.is_empty() {
            row_coverage.push(gap(
                "remediation_proposal",
                LineageUnavailableReason::NotInstrumented,
            ));
        }
        coverage.extend(row_coverage.iter().cloned());
        rows.push(InsightProjectionRowV1 {
            issue_id: issue.issue_id.clone(),
            family: issue.family.clone(),
            canonical_description: issue.canonical_description.clone(),
            state: issue_state(issue),
            recurrence_count: issue.recurrence_count,
            episode_ids: issue.episode_ids.clone(),
            proposal_ids,
            receipt_ids,
            first_seen: issue.first_seen.clone(),
            last_seen: issue.last_seen.clone(),
            coverage: row_coverage,
        });
    }
    rows.sort_by(|a, b| a.issue_id.cmp(&b.issue_id));
    coverage.sort_by(|a, b| a.field.cmp(&b.field).then(a.reason.cmp(&b.reason)));
    coverage.dedup();
    InsightsProjectionV1 {
        schema_version: INSIGHTS_PROJECTION_SCHEMA.into(),
        rows,
        coverage,
    }
}

pub fn project_mine(
    events: &[TranscriptEventV1],
    episodes: &[FailureEpisodeV1],
    issues: &[InsightIssueV1],
    remediation_proposals: &[SealedRemediationProposalV1],
) -> MineProjectionV1 {
    let input = LineageInputV1::from_mine(events, episodes, issues, remediation_proposals);
    MineProjectionV1 {
        lineage: build_lineage(&input),
        insights: build_insights_projection(&input),
        asset_effectiveness: input.asset_effectiveness,
    }
}

/// Production read model variant for callers that supplied host-derived
/// effectiveness rows. Rows remain caller-owned snapshots; no lifecycle state
/// is inferred here.
pub fn project_mine_with_effectiveness(
    events: &[TranscriptEventV1],
    episodes: &[FailureEpisodeV1],
    issues: &[InsightIssueV1],
    remediation_proposals: &[SealedRemediationProposalV1],
    asset_effectiveness: &[ProceduralAssetEffectivenessV1],
) -> MineProjectionV1 {
    let mut input = LineageInputV1::from_mine(events, episodes, issues, remediation_proposals);
    input.asset_effectiveness = asset_effectiveness.to_vec();
    MineProjectionV1 {
        lineage: build_lineage(&input),
        insights: build_insights_projection(&input),
        asset_effectiveness: input.asset_effectiveness,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::insights::detectors::run_all_detectors;
    use crate::insights::recurrence::form_issues;
    use crate::insights::EventKind;

    fn event(id: &str, session: &str, text: &str) -> TranscriptEventV1 {
        TranscriptEventV1 {
            event_id: id.into(),
            session_id: session.into(),
            host: "test".into(),
            provenance: "external_user".into(),
            kind: EventKind::UserMessage,
            text: text.into(),
            timestamp: Some("2026-08-26T00:00:00Z".into()),
            byte_start: 0,
            byte_end: text.len() as i64,
            call_id: None,
            occurrence: 0,
            evidence_eligible: true,
        }
    }

    fn mined() -> (
        Vec<TranscriptEventV1>,
        Vec<FailureEpisodeV1>,
        Vec<InsightIssueV1>,
        Vec<SealedRemediationProposalV1>,
    ) {
        let events = vec![
            event(
                "e1",
                "s1",
                "please run the full test suite before claiming done",
            ),
            event(
                "e2",
                "s2",
                "Please run the FULL test suite before claiming done.",
            ),
        ];
        let episodes = run_all_detectors(&events);
        let issues = form_issues(&episodes, 2);
        let proposals = crate::remediation::seal_review_proposals(&issues);
        (events, episodes, issues, proposals)
    }

    #[test]
    fn production_mine_lineage_contains_observed_receipts_and_typed_gaps() {
        let (events, episodes, issues, proposals) = mined();
        let projection = project_mine(&events, &episodes, &issues, &proposals);
        assert_eq!(projection.lineage.len(), issues.len());
        let lineage = &projection.lineage[0];
        assert!(lineage
            .nodes
            .iter()
            .any(|node| node.stage == LineageStage::Experience));
        assert!(lineage
            .nodes
            .iter()
            .any(|node| node.stage == LineageStage::Episode));
        assert!(lineage
            .nodes
            .iter()
            .any(|node| node.stage == LineageStage::Insight));
        let proposal = lineage
            .nodes
            .iter()
            .find(|node| node.stage == LineageStage::Proposal)
            .expect("mine reaches sealed proposal");
        assert!(proposal
            .receipt_ids
            .iter()
            .all(|receipt| receipt.starts_with("rcpt_")));
        assert!(lineage.nodes.iter().all(|node| {
            !matches!(
                node.stage,
                LineageStage::Variant | LineageStage::Experiment | LineageStage::Deployment
            )
        }));
        assert!(lineage.coverage.iter().any(|gap| {
            gap.field == "experiment" && gap.reason == LineageUnavailableReason::NotInstrumented
        }));
        assert!(lineage.coverage.iter().any(|gap| {
            gap.field == "outcome" && gap.reason == LineageUnavailableReason::NotInstrumented
        }));
    }

    #[test]
    fn supplied_outcome_is_joined_without_host_records() {
        let (events, episodes, issues, proposals) = mined();
        let mut input = LineageInputV1::from_mine(&events, &episodes, &issues, &proposals);
        let proposal_id = proposals[0].proposal_id.clone();
        input.outcomes.push(OutcomeEntryV1 {
            entry_id: "out-test".into(),
            issue_id: issues[0].issue_id.clone(),
            mitigation_proposal_id: proposal_id,
            raw: crate::outcomes::RawOutcome::NoRecurrence,
            exposure: crate::outcomes::Exposure {
                opportunities: 10,
                baseline: 10,
            },
            adjusted: crate::outcomes::AdjustedOutcome::Effective,
            note: String::new(),
        });
        let lineage = build_lineage(&input).remove(0);
        assert!(lineage
            .nodes
            .iter()
            .any(|node| node.stage == LineageStage::Outcome && node.id == "out-test"));
        assert!(!lineage
            .nodes
            .iter()
            .any(|node| node.stage == LineageStage::Experiment));
        assert!(lineage.coverage.iter().any(|gap| gap.field == "deployment"));
        assert!(!lineage.coverage.iter().any(|gap| gap.field == "outcome"));
    }

    #[test]
    fn insights_projection_copies_source_text_and_surfaces_missing_receipts() {
        let (events, episodes, issues, proposals) = mined();
        let input = LineageInputV1::from_mine(&events, &episodes, &issues, &proposals);
        let expected = input.issues[0].canonical_description.clone();
        let projection = build_insights_projection(&input);
        assert_eq!(projection.schema_version, INSIGHTS_PROJECTION_SCHEMA);
        assert_eq!(projection.rows[0].canonical_description, expected);
        assert!(projection.rows[0].coverage.iter().any(|gap| {
            gap.field == "issue.seal_receipt"
                && gap.reason == LineageUnavailableReason::NotInstrumented
        }));
    }
}
