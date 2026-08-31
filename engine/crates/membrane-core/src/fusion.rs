//! Deterministic RRF fusion for completed provider candidate sets.
//!
//! Provider-local scores establish only a lane-local rank. Cross-provider
//! selection uses reciprocal-rank position, then explicit stable tie-breaks;
//! completion order is never an input to selection.

use membrane_protocol::{
    canonical_json_of, CandidateV1, ContextCandidateSetV1, FreshnessClass, FusionDecisionV1,
    FusionReceiptV1,
};
use std::cmp::Ordering;
use std::collections::{BTreeMap, HashSet};

pub const DEFAULT_RRF_K: u32 = 60;
pub const DEFAULT_MAX_ITEMS: u32 = 32;

/// Hard bounds for one fusion pass. Missing provider entries use that lane's
/// submitted count, capped by [`max_items`](Self::max_items) after fusion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FusionBounds {
    pub provider_quotas: BTreeMap<String, u32>,
    pub rrf_k: u32,
    pub max_items: u32,
}

impl Default for FusionBounds {
    fn default() -> Self {
        Self {
            provider_quotas: BTreeMap::new(),
            rrf_k: DEFAULT_RRF_K,
            max_items: DEFAULT_MAX_ITEMS,
        }
    }
}

/// Selected candidates plus a content-free, canonically ordered receipt.
#[derive(Debug, Clone, PartialEq)]
pub struct FusionResult {
    pub candidates: Vec<CandidateV1>,
    pub receipt: FusionReceiptV1,
}

struct RankedCandidate {
    provider: String,
    candidate: CandidateV1,
    provider_rank: u32,
    rrf_score: f64,
}

fn provider_candidate_order(left: &CandidateV1, right: &CandidateV1) -> Ordering {
    trust_rank(&left.trust_class)
        .cmp(&trust_rank(&right.trust_class))
        .then_with(|| {
            freshness_rank(left.freshness_class).cmp(&freshness_rank(right.freshness_class))
        })
        .then_with(|| right.provider_score.total_cmp(&left.provider_score))
        .then_with(|| left.id.cmp(&right.id))
        .then_with(|| left.source_hash.cmp(&right.source_hash))
        .then_with(|| left.source_ref.cmp(&right.source_ref))
        .then_with(|| left.text.cmp(&right.text))
        .then_with(|| canonical_json_of(left).cmp(&canonical_json_of(right)))
}

/// Canonical authority order from `tools/lib/context-contracts.schema.json`.
/// Unknown values fail low; `remote_untrusted` is retained only as the lowest
/// deterministic lane because planner admission rejects it before delivery.
fn trust_rank(value: &str) -> u8 {
    match value {
        "user_direct" => 0,
        // Protocol golden fixtures use this local authority label.
        "local_trusted" => 1,
        "workspace_tracked" => 2,
        "workspace_generated" => 3,
        "agent_verified" => 4,
        "remote_untrusted" => 5,
        _ => 6,
    }
}

const fn freshness_rank(value: Option<FreshnessClass>) -> u8 {
    match value {
        Some(FreshnessClass::Current) => 0,
        Some(FreshnessClass::CommittedSnapshot) => 1,
        Some(FreshnessClass::DirtyOverlay) => 2,
        Some(FreshnessClass::StaleSnapshot) => 3,
        Some(FreshnessClass::Unknown) | None => 4,
    }
}

fn fusion_order(left: &RankedCandidate, right: &RankedCandidate) -> Ordering {
    trust_rank(&left.candidate.trust_class)
        .cmp(&trust_rank(&right.candidate.trust_class))
        .then_with(|| {
            freshness_rank(left.candidate.freshness_class)
                .cmp(&freshness_rank(right.candidate.freshness_class))
        })
        // Authority and freshness are admission precedence. Within an
        // equivalent lane, aggregate reciprocal-rank evidence is the active
        // cross-provider signal; deterministic identity fields finish ties.
        .then_with(|| right.rrf_score.total_cmp(&left.rrf_score))
        .then_with(|| left.provider_rank.cmp(&right.provider_rank))
        .then_with(|| left.candidate.id.cmp(&right.candidate.id))
        .then_with(|| left.provider.cmp(&right.provider))
        .then_with(|| left.candidate.source_hash.cmp(&right.candidate.source_hash))
        .then_with(|| left.candidate.source_ref.cmp(&right.candidate.source_ref))
        .then_with(|| left.candidate.text.cmp(&right.candidate.text))
        .then_with(|| canonical_json_of(&left.candidate).cmp(&canonical_json_of(&right.candidate)))
}

/// Fuse completed provider sets with MBR-407's selectable RRF baseline.
///
/// Each provider lane is first sorted by its local score, candidate id, source
/// hash, source reference, text, then canonical candidate bytes. Provider ids are
/// bytewise sorted. Every admitted candidate contributes `1 / (k + rank)` to
/// its source identity; aggregate reciprocal-rank score then orders equivalent
/// authority/freshness lanes. These orderings make input completion order
/// irrelevant and form part of receipt contract.
pub fn fuse(candidate_sets: &[ContextCandidateSetV1], bounds: FusionBounds) -> FusionResult {
    let mut by_provider: BTreeMap<String, Vec<CandidateV1>> = BTreeMap::new();
    for set in candidate_sets {
        by_provider
            .entry(set.provider.clone())
            .or_default()
            .extend(set.candidates.iter().cloned());
    }

    let received = by_provider.values().map(Vec::len).sum::<usize>();
    let provider_order = by_provider.keys().cloned().collect::<Vec<_>>();
    let mut decisions = Vec::new();
    let mut ranked = Vec::new();
    let mut rrf_scores: BTreeMap<String, f64> = BTreeMap::new();

    for (provider, candidates) in &mut by_provider {
        candidates.sort_by(provider_candidate_order);
        let quota = bounds
            .provider_quotas
            .get(provider)
            .copied()
            .unwrap_or_else(|| u32::try_from(candidates.len()).unwrap_or(u32::MAX));
        for (index, candidate) in candidates.iter().cloned().enumerate() {
            let rank = u32::try_from(index + 1).unwrap_or(u32::MAX);
            let denominator = bounds.rrf_k.saturating_add(rank);
            if candidate.source_kind.trim().is_empty()
                || candidate.source_ref.trim().is_empty()
                || candidate.source_hash.trim().is_empty()
            {
                decisions.push(FusionDecisionV1 {
                    id: candidate.id,
                    provider: provider.clone(),
                    provider_rank: rank,
                    rrf_denominator: denominator,
                    fused_rank: None,
                    decision: "rejected".to_string(),
                    reason: "invalid_source".to_string(),
                });
            } else if rank > quota {
                decisions.push(FusionDecisionV1 {
                    id: candidate.id,
                    provider: provider.clone(),
                    provider_rank: rank,
                    rrf_denominator: denominator,
                    fused_rank: None,
                    decision: "rejected".to_string(),
                    reason: "provider_quota".to_string(),
                });
            } else {
                *rrf_scores.entry(candidate.source_hash.clone()).or_default() +=
                    1.0 / f64::from(denominator.max(1));
                ranked.push(RankedCandidate {
                    provider: provider.clone(),
                    candidate,
                    provider_rank: rank,
                    rrf_score: 0.0,
                });
            }
        }
    }

    for entry in &mut ranked {
        entry.rrf_score = rrf_scores
            .get(&entry.candidate.source_hash)
            .copied()
            .unwrap_or_default();
    }
    ranked.sort_by(fusion_order);
    let mut selected = Vec::new();
    let mut seen = HashSet::new();
    let mut fused_ranks: BTreeMap<String, u32> = BTreeMap::new();
    let mut next_fused_rank = 1u32;
    for entry in ranked {
        let denominator = bounds.rrf_k.saturating_add(entry.provider_rank);
        let duplicate = !entry.candidate.protected && seen.contains(&entry.candidate.source_hash);
        let fused_rank = *fused_ranks
            .entry(entry.candidate.source_hash.clone())
            .or_insert_with(|| {
                let rank = next_fused_rank;
                next_fused_rank = next_fused_rank.saturating_add(1);
                rank
            });
        seen.insert(entry.candidate.source_hash.clone());
        let capped = u32::try_from(selected.len()).unwrap_or(u32::MAX) >= bounds.max_items;
        let (decision, reason) = if duplicate {
            ("rejected", "duplicate_source_hash")
        } else if capped {
            ("rejected", "max_items")
        } else {
            selected.push(entry.candidate.clone());
            ("selected", "within_bounds")
        };
        decisions.push(FusionDecisionV1 {
            id: entry.candidate.id,
            provider: entry.provider,
            provider_rank: entry.provider_rank,
            rrf_denominator: denominator,
            fused_rank: Some(fused_rank),
            decision: decision.to_string(),
            reason: reason.to_string(),
        });
    }

    decisions.sort_by(|left, right| {
        left.rrf_denominator
            .cmp(&right.rrf_denominator)
            .then_with(|| left.id.cmp(&right.id))
            .then_with(|| left.provider.cmp(&right.provider))
            .then_with(|| left.reason.cmp(&right.reason))
    });
    let candidates_selected = u32::try_from(selected.len()).unwrap_or(u32::MAX);
    FusionResult {
        candidates: selected,
        receipt: FusionReceiptV1 {
            schema_version: FusionReceiptV1::SCHEMA_VERSION,
            policy: FusionReceiptV1::RRF_POLICY.to_string(),
            fallback_policy: FusionReceiptV1::RRF_FALLBACK_POLICY.to_string(),
            provider_order,
            provider_quotas: bounds.provider_quotas,
            rrf_k: bounds.rrf_k,
            max_items: bounds.max_items,
            candidates_received: u32::try_from(received).unwrap_or(u32::MAX),
            candidates_selected,
            decisions,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use membrane_protocol::{FreshnessV1, ProviderCeilingV1};

    fn candidate(id: &str, score: f64, source_hash: &str) -> CandidateV1 {
        CandidateV1 {
            id: id.to_string(),
            layer: 0,
            provider: None,
            source_kind: "fixture".to_string(),
            source_ref: format!("fixture:{id}"),
            source_hash: source_hash.to_string(),
            trust_class: "trusted".to_string(),
            instruction_policy: "data".to_string(),
            provider_score: score,
            score_components: BTreeMap::new(),
            base_commit: None,
            overlay_digest: None,
            freshness_class: None,
            snapshot_id: None,
            estimated_tokens: 1,
            protected: false,
            exact: true,
            recoverable: true,
            resolver: "fixture".to_string(),
            text: id.to_string(),
        }
    }

    fn set(provider: &str, candidates: Vec<CandidateV1>) -> ContextCandidateSetV1 {
        ContextCandidateSetV1 {
            schema_version: 1,
            trace_id: "fixture".to_string(),
            indexed_at: "fixture".to_string(),
            task: "fixture".to_string(),
            mode: "fixture".to_string(),
            provider: provider.to_string(),
            freshness: FreshnessV1 {
                revision: "fixture".to_string(),
                indexed_at: "fixture".to_string(),
                stale: false,
                graph_state: None,
                snapshot_id: None,
                base_commit: None,
                overlay_digest: None,
                expected_release_generation: None,
                observed_release_generation: None,
                release_generation_status: None,
                overlay_identity: None,
            },
            provider_ceiling: ProviderCeilingV1 {
                max_candidates: 8,
                max_estimated_tokens: 8,
            },
            candidates,
            omissions: Vec::new(),
        }
    }

    #[test]
    fn fixed_corpus_is_identical_for_every_completion_order() {
        let corpus = vec![
            set(
                "blueprint",
                vec![
                    candidate("c2", 0.5, "hash-c2"),
                    candidate("c1", 0.9, "hash-shared"),
                ],
            ),
            set(
                "memory",
                vec![
                    candidate("m2", 0.5, "hash-m2"),
                    candidate("m1", 0.9, "hash-shared"),
                ],
            ),
            set("diagnostic", vec![candidate("d1", 0.9, "hash-d1")]),
        ];
        let bounds = FusionBounds {
            provider_quotas: BTreeMap::from([
                ("blueprint".to_string(), 2),
                ("memory".to_string(), 2),
            ]),
            rrf_k: 60,
            max_items: 3,
        };
        let expected = fuse(&corpus, bounds.clone());
        for first in 0..corpus.len() {
            for second in 0..corpus.len() {
                if second == first {
                    continue;
                }
                let third = (0..corpus.len())
                    .find(|index| *index != first && *index != second)
                    .unwrap();
                let ordered = vec![
                    corpus[first].clone(),
                    corpus[second].clone(),
                    corpus[third].clone(),
                ];
                assert_eq!(fuse(&ordered, bounds.clone()), expected);
            }
        }
        assert_eq!(
            expected
                .candidates
                .iter()
                .map(|candidate| candidate.id.as_str())
                .collect::<Vec<_>>(),
            vec!["c1", "d1", "c2"]
        );
        assert_eq!(
            expected.receipt.provider_order,
            vec![
                "blueprint".to_string(),
                "diagnostic".to_string(),
                "memory".to_string()
            ]
        );
        assert_eq!(expected.receipt.policy, FusionReceiptV1::RRF_POLICY);
    }

    #[test]
    fn aggregate_rrf_promotes_cross_provider_agreement() {
        let corpus = vec![
            set(
                "blueprint",
                vec![
                    candidate("a1", 1.0, "hash-a1"),
                    candidate("a2", 0.9, "hash-agreed"),
                ],
            ),
            set(
                "memory",
                vec![
                    candidate("b1", 1.0, "hash-agreed"),
                    candidate("b2", 0.9, "hash-b2"),
                ],
            ),
        ];
        let result = fuse(
            &corpus,
            FusionBounds {
                provider_quotas: BTreeMap::from([
                    ("blueprint".to_owned(), 2),
                    ("memory".to_owned(), 2),
                ]),
                rrf_k: 60,
                max_items: 3,
            },
        );

        // The shared source hash receives rank-2 + rank-1 contributions and
        // outranks the rank-1 singleton, while memory's rank-1 representative
        // wins the deterministic duplicate tie.
        assert_eq!(
            result
                .candidates
                .iter()
                .map(|candidate| candidate.id.as_str())
                .collect::<Vec<_>>(),
            vec!["b1", "a1", "b2"]
        );
        let shared = result
            .receipt
            .decisions
            .iter()
            .find(|decision| decision.id == "b1")
            .expect("selected shared candidate receipt");
        assert_eq!(shared.fused_rank, Some(1));
        assert_eq!(
            result
                .receipt
                .decisions
                .iter()
                .find(|decision| decision.id == "a2")
                .map(|decision| (
                    decision.decision.as_str(),
                    decision.reason.as_str(),
                    decision.fused_rank
                )),
            Some(("rejected", "duplicate_source_hash", Some(1)))
        );
    }

    #[test]
    fn single_arm_survives_with_invalid_sources_excluded_and_cap_receipted() {
        let mut invalid = candidate("invalid", 1.0, "hash-invalid");
        invalid.source_ref.clear();
        let result = fuse(
            &[set(
                "lexical",
                vec![
                    invalid,
                    candidate("one", 0.9, "hash-one"),
                    candidate("two", 0.8, "hash-two"),
                    candidate("three", 0.7, "hash-three"),
                ],
            )],
            FusionBounds {
                rrf_k: 60,
                max_items: 2,
                ..FusionBounds::default()
            },
        );
        assert_eq!(
            result
                .candidates
                .iter()
                .map(|candidate| candidate.id.as_str())
                .collect::<Vec<_>>(),
            vec!["one", "two"]
        );
        assert_eq!(result.receipt.policy, FusionReceiptV1::RRF_POLICY);
        assert_eq!(
            result.receipt.fallback_policy,
            FusionReceiptV1::RRF_FALLBACK_POLICY
        );
        assert_eq!(result.receipt.max_items, 2);
        assert!(result.receipt.decisions.iter().any(|decision| {
            decision.id == "invalid"
                && decision.decision == "rejected"
                && decision.reason == "invalid_source"
        }));
        assert!(result.receipt.decisions.iter().any(|decision| {
            decision.id == "three"
                && decision.decision == "rejected"
                && decision.reason == "max_items"
        }));
    }

    #[test]
    fn canonical_authority_and_freshness_precede_score_for_every_permutation() {
        let classes = [
            "user_direct",
            "local_trusted",
            "workspace_tracked",
            "workspace_generated",
            "agent_verified",
            "remote_untrusted",
        ];
        let sets = classes
            .iter()
            .enumerate()
            .map(|(index, class)| {
                let mut value = candidate(class, 1.0 - index as f64 / 10.0, class);
                value.trust_class = (*class).into();
                if *class == "workspace_tracked" {
                    value.freshness_class = Some(FreshnessClass::StaleSnapshot);
                }
                set(class, vec![value])
            })
            .collect::<Vec<_>>();
        let bounds = FusionBounds {
            max_items: 6,
            ..FusionBounds::default()
        };
        let expected = fuse(&sets, bounds.clone());
        assert_eq!(
            expected
                .candidates
                .iter()
                .map(|value| value.id.as_str())
                .collect::<Vec<_>>(),
            classes
        );
        for a in 0..sets.len() {
            for b in 0..sets.len() {
                for c in 0..sets.len() {
                    for d in 0..sets.len() {
                        for e in 0..sets.len() {
                            for f in 0..sets.len() {
                                if [a, b, c, d, e, f]
                                    .iter()
                                    .collect::<std::collections::BTreeSet<_>>()
                                    .len()
                                    == sets.len()
                                {
                                    assert_eq!(
                                        fuse(
                                            &[
                                                sets[a].clone(),
                                                sets[b].clone(),
                                                sets[c].clone(),
                                                sets[d].clone(),
                                                sets[e].clone(),
                                                sets[f].clone()
                                            ],
                                            bounds.clone()
                                        ),
                                        expected
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
        assert!(trust_rank("unrecognized") > trust_rank("remote_untrusted"));
    }
}
