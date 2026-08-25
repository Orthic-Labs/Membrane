//! Deterministic duplicate grouping and reviewed semantic-merge receipts.

use std::collections::{BTreeMap, BTreeSet};

use ring::signature;
use serde::{Deserialize, Serialize};

use crate::canonical::{canonical_object, normalize_text, sha256_canonical, sha256_hex};

pub const DUPLICATE_GROUP_CONTRACT: &str = "adapt.duplicate-group.v1";
pub const DETERMINISTIC_EXACT_ALGORITHM: &str = "exact-normalized-text-scope-v1";
pub const SEMANTIC_MERGE_RECEIPT_CONTRACT: &str = "adapt.semantic-merge-receipt.v1";
const SIGNATURE_DOMAIN: &[u8] = b"membrane.adapt-semantic-merge.v1\0";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DuplicateCandidateV1 {
    pub record_id: String,
    pub canonical_text: String,
    pub scope: String,
    pub semantic_seal: String,
    /// Digest over meaning/applicability fields, excluding evidence and
    /// provenance. This prevents textual equality from collapsing distinct
    /// authority, category, class, scope-dimension, or machine semantics.
    pub semantic_equivalence_digest: String,
    pub evidence_count: u32,
    pub existing_canonical: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DuplicateMemberV1 {
    pub record_id: String,
    pub semantic_seal: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DuplicateGroupV1 {
    pub contract_version: String,
    pub algorithm: String,
    pub members: Vec<DuplicateMemberV1>,
    pub group_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DuplicateDispositionV1 {
    Merge { winner_id: String },
    PreserveDistinct,
    Abstain,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DuplicateResolutionV1 {
    pub group_digest: String,
    pub disposition: DuplicateDispositionV1,
    /// Every non-winning member remains addressable/history-preserved.
    pub preserved_member_ids: Vec<String>,
    pub review_receipt_sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewedSemanticMergeReceiptV1 {
    pub contract_version: String,
    pub reviewer_id: String,
    pub key_id: String,
    pub group_digest: String,
    pub disposition: DuplicateDispositionV1,
    pub reviewed_at: String,
    pub receipt_sha256: String,
    pub signature_hex: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DuplicateGroupError {
    InvalidMember,
    DuplicateMember,
    GroupDigestMismatch,
    UntrustedReviewer,
    InvalidReviewReceipt,
}

fn group_material(algorithm: &str, members: &[DuplicateMemberV1]) -> serde_json::Value {
    canonical_object([
        (
            "contract_version",
            serde_json::Value::String(DUPLICATE_GROUP_CONTRACT.into()),
        ),
        ("algorithm", serde_json::Value::String(algorithm.into())),
        (
            "members",
            serde_json::to_value(members).expect("members serialize"),
        ),
    ])
}

pub fn build_group(
    members: impl IntoIterator<Item = DuplicateMemberV1>,
    algorithm: &str,
) -> Result<DuplicateGroupV1, DuplicateGroupError> {
    let mut members: Vec<_> = members.into_iter().collect();
    members.sort_by(|left, right| {
        left.record_id
            .cmp(&right.record_id)
            .then(left.semantic_seal.cmp(&right.semantic_seal))
    });
    if members.len() < 2
        || algorithm.trim().is_empty()
        || members.iter().any(|item| {
            item.record_id.trim().is_empty()
                || item.semantic_seal.len() != 64
                || !item
                    .semantic_seal
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit())
        })
    {
        return Err(DuplicateGroupError::InvalidMember);
    }
    if members
        .windows(2)
        .any(|pair| pair[0].record_id == pair[1].record_id)
    {
        return Err(DuplicateGroupError::DuplicateMember);
    }
    let group_digest = sha256_canonical(&group_material(algorithm, &members));
    Ok(DuplicateGroupV1 {
        contract_version: DUPLICATE_GROUP_CONTRACT.into(),
        algorithm: algorithm.into(),
        members,
        group_digest,
    })
}

/// Supported deterministic class: identical normalized semantic text in the
/// same declared scope. Anything more semantic remains proposal-only.
pub fn deterministic_exact_groups(
    candidates: &[DuplicateCandidateV1],
) -> Result<Vec<DuplicateGroupV1>, DuplicateGroupError> {
    let mut buckets: BTreeMap<(String, String), Vec<&DuplicateCandidateV1>> = BTreeMap::new();
    for candidate in candidates {
        if candidate.record_id.trim().is_empty() || candidate.semantic_seal.len() != 64 {
            return Err(DuplicateGroupError::InvalidMember);
        }
        if candidate.semantic_equivalence_digest.len() != 64 {
            return Err(DuplicateGroupError::InvalidMember);
        }
        buckets
            .entry((
                candidate.scope.trim().to_lowercase(),
                format!(
                    "{}\0{}",
                    normalize_text(&candidate.canonical_text),
                    candidate.semantic_equivalence_digest
                ),
            ))
            .or_default()
            .push(candidate);
    }
    let mut groups = Vec::new();
    for bucket in buckets.values().filter(|items| items.len() > 1) {
        groups.push(build_group(
            bucket.iter().map(|item| DuplicateMemberV1 {
                record_id: item.record_id.clone(),
                semantic_seal: item.semantic_seal.clone(),
            }),
            DETERMINISTIC_EXACT_ALGORITHM,
        )?);
    }
    groups.sort_by(|left, right| left.group_digest.cmp(&right.group_digest));
    Ok(groups)
}

fn deterministic_winner<'a>(
    group: &DuplicateGroupV1,
    candidates: &'a [DuplicateCandidateV1],
) -> Result<&'a str, DuplicateGroupError> {
    let member_ids: BTreeSet<&str> = group
        .members
        .iter()
        .map(|item| item.record_id.as_str())
        .collect();
    let mut by_id = BTreeMap::new();
    for candidate in candidates {
        if by_id
            .insert(candidate.record_id.as_str(), candidate)
            .is_some()
        {
            return Err(DuplicateGroupError::DuplicateMember);
        }
    }
    for member in &group.members {
        let Some(candidate) = by_id.get(member.record_id.as_str()) else {
            return Err(DuplicateGroupError::InvalidMember);
        };
        if candidate.semantic_seal != member.semantic_seal {
            return Err(DuplicateGroupError::GroupDigestMismatch);
        }
    }
    candidates
        .iter()
        .filter(|item| member_ids.contains(item.record_id.as_str()))
        .min_by_key(|item| {
            (
                !item.existing_canonical,
                std::cmp::Reverse(item.evidence_count),
                item.canonical_text.len(),
                item.record_id.clone(),
            )
        })
        .map(|item| item.record_id.as_str())
        .ok_or(DuplicateGroupError::InvalidMember)
}

pub fn resolve_deterministic_group(
    group: &DuplicateGroupV1,
    candidates: &[DuplicateCandidateV1],
) -> Result<DuplicateResolutionV1, DuplicateGroupError> {
    if group.contract_version != DUPLICATE_GROUP_CONTRACT
        || group.algorithm != DETERMINISTIC_EXACT_ALGORITHM
    {
        return Err(DuplicateGroupError::GroupDigestMismatch);
    }
    let rebuilt = build_group(group.members.clone(), &group.algorithm)?;
    if rebuilt.group_digest != group.group_digest {
        return Err(DuplicateGroupError::GroupDigestMismatch);
    }
    let winner = deterministic_winner(group, candidates)?.to_string();
    let preserved_member_ids = group
        .members
        .iter()
        .filter(|item| item.record_id != winner)
        .map(|item| item.record_id.clone())
        .collect();
    Ok(DuplicateResolutionV1 {
        group_digest: group.group_digest.clone(),
        disposition: DuplicateDispositionV1::Merge { winner_id: winner },
        preserved_member_ids,
        review_receipt_sha256: None,
    })
}

fn receipt_material(receipt: &ReviewedSemanticMergeReceiptV1) -> serde_json::Value {
    canonical_object([
        (
            "contract_version",
            serde_json::Value::String(receipt.contract_version.clone()),
        ),
        (
            "reviewer_id",
            serde_json::Value::String(receipt.reviewer_id.clone()),
        ),
        ("key_id", serde_json::Value::String(receipt.key_id.clone())),
        (
            "group_digest",
            serde_json::Value::String(receipt.group_digest.clone()),
        ),
        (
            "disposition",
            serde_json::to_value(&receipt.disposition).expect("disposition serializes"),
        ),
        (
            "reviewed_at",
            serde_json::Value::String(receipt.reviewed_at.clone()),
        ),
    ])
}

pub fn verify_reviewed_resolution(
    group: &DuplicateGroupV1,
    receipt: Option<&ReviewedSemanticMergeReceiptV1>,
    trusted_reviewer_keys: &BTreeMap<(String, String), Vec<u8>>,
) -> Result<DuplicateResolutionV1, DuplicateGroupError> {
    let rebuilt = build_group(group.members.clone(), &group.algorithm)?;
    if group.contract_version != DUPLICATE_GROUP_CONTRACT
        || rebuilt.group_digest != group.group_digest
    {
        return Err(DuplicateGroupError::GroupDigestMismatch);
    }
    let Some(receipt) = receipt else {
        return Ok(DuplicateResolutionV1 {
            group_digest: group.group_digest.clone(),
            disposition: DuplicateDispositionV1::Abstain,
            preserved_member_ids: group
                .members
                .iter()
                .map(|item| item.record_id.clone())
                .collect(),
            review_receipt_sha256: None,
        });
    };
    if receipt.contract_version != SEMANTIC_MERGE_RECEIPT_CONTRACT
        || receipt.group_digest != group.group_digest
        || receipt.reviewer_id.trim().is_empty()
        || receipt.key_id.trim().is_empty()
        || receipt.reviewed_at.trim().is_empty()
    {
        return Err(DuplicateGroupError::InvalidReviewReceipt);
    }
    let key = trusted_reviewer_keys
        .get(&(receipt.reviewer_id.clone(), receipt.key_id.clone()))
        .ok_or(DuplicateGroupError::UntrustedReviewer)?;
    let material = receipt_material(receipt);
    let digest = sha256_canonical(&material);
    if digest != receipt.receipt_sha256 {
        return Err(DuplicateGroupError::InvalidReviewReceipt);
    }
    let mut signed = SIGNATURE_DOMAIN.to_vec();
    signed.extend_from_slice(digest.as_bytes());
    let sig = hex::decode(&receipt.signature_hex)
        .map_err(|_| DuplicateGroupError::InvalidReviewReceipt)?;
    signature::UnparsedPublicKey::new(&signature::ED25519, key)
        .verify(&signed, &sig)
        .map_err(|_| DuplicateGroupError::InvalidReviewReceipt)?;
    let member_ids: BTreeSet<&str> = group
        .members
        .iter()
        .map(|item| item.record_id.as_str())
        .collect();
    if let DuplicateDispositionV1::Merge { winner_id } = &receipt.disposition {
        if !member_ids.contains(winner_id.as_str()) {
            return Err(DuplicateGroupError::InvalidReviewReceipt);
        }
    }
    let winner = match &receipt.disposition {
        DuplicateDispositionV1::Merge { winner_id } => Some(winner_id),
        _ => None,
    };
    let preserved_member_ids = group
        .members
        .iter()
        .filter(|item| winner.is_none_or(|winner| &item.record_id != winner))
        .map(|item| item.record_id.clone())
        .collect();
    Ok(DuplicateResolutionV1 {
        group_digest: group.group_digest.clone(),
        disposition: receipt.disposition.clone(),
        preserved_member_ids,
        review_receipt_sha256: Some(sha256_hex(
            &serde_json::to_vec(receipt).expect("receipt serializes"),
        )),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::signature::{Ed25519KeyPair, KeyPair};

    fn candidate(
        id: &str,
        text: &str,
        seal: char,
        count: u32,
        existing: bool,
    ) -> DuplicateCandidateV1 {
        DuplicateCandidateV1 {
            record_id: id.into(),
            canonical_text: text.into(),
            scope: "repo".into(),
            semantic_seal: seal.to_string().repeat(64),
            semantic_equivalence_digest: "e".repeat(64),
            evidence_count: count,
            existing_canonical: existing,
        }
    }

    #[test]
    fn exact_groups_and_winner_are_permutation_invariant_and_preserve_losers() {
        let candidates = vec![
            candidate("b", "Always run tests.", 'b', 5, false),
            candidate("a", "always  run tests", 'a', 1, true),
        ];
        let group = deterministic_exact_groups(&candidates).unwrap().remove(0);
        let mut reversed = candidates.clone();
        reversed.reverse();
        assert_eq!(
            group,
            deterministic_exact_groups(&reversed).unwrap().remove(0)
        );
        let resolution = resolve_deterministic_group(&group, &candidates).unwrap();
        assert_eq!(
            resolution.disposition,
            DuplicateDispositionV1::Merge {
                winner_id: "a".into()
            }
        );
        assert_eq!(resolution.preserved_member_ids, ["b"]);
    }

    #[test]
    fn uncertain_group_abstains_without_review() {
        let group = build_group(
            [
                DuplicateMemberV1 {
                    record_id: "a".into(),
                    semantic_seal: "a".repeat(64),
                },
                DuplicateMemberV1 {
                    record_id: "b".into(),
                    semantic_seal: "b".repeat(64),
                },
            ],
            "model-proposed-v1",
        )
        .unwrap();
        let resolution = verify_reviewed_resolution(&group, None, &BTreeMap::new()).unwrap();
        assert_eq!(resolution.disposition, DuplicateDispositionV1::Abstain);
        assert_eq!(resolution.preserved_member_ids, ["a", "b"]);
        let candidates = vec![
            candidate("a", "same", 'a', 1, false),
            candidate("b", "same", 'b', 1, false),
        ];
        assert_eq!(
            resolve_deterministic_group(&group, &candidates).unwrap_err(),
            DuplicateGroupError::GroupDigestMismatch
        );
    }

    #[test]
    fn trusted_review_can_merge_and_tampering_fails() {
        let key = Ed25519KeyPair::from_seed_unchecked(&[4; 32]).unwrap();
        let group = build_group(
            [
                DuplicateMemberV1 {
                    record_id: "a".into(),
                    semantic_seal: "a".repeat(64),
                },
                DuplicateMemberV1 {
                    record_id: "b".into(),
                    semantic_seal: "b".repeat(64),
                },
            ],
            "model-proposed-v1",
        )
        .unwrap();
        let mut receipt = ReviewedSemanticMergeReceiptV1 {
            contract_version: SEMANTIC_MERGE_RECEIPT_CONTRACT.into(),
            reviewer_id: "reviewer".into(),
            key_id: "key".into(),
            group_digest: group.group_digest.clone(),
            disposition: DuplicateDispositionV1::Merge {
                winner_id: "b".into(),
            },
            reviewed_at: "2026-08-26T00:00:00Z".into(),
            receipt_sha256: String::new(),
            signature_hex: String::new(),
        };
        receipt.receipt_sha256 = sha256_canonical(&receipt_material(&receipt));
        let mut signed = SIGNATURE_DOMAIN.to_vec();
        signed.extend_from_slice(receipt.receipt_sha256.as_bytes());
        receipt.signature_hex = hex::encode(key.sign(&signed).as_ref());
        let trust = BTreeMap::from([(
            ("reviewer".into(), "key".into()),
            key.public_key().as_ref().to_vec(),
        )]);
        let resolved = verify_reviewed_resolution(&group, Some(&receipt), &trust).unwrap();
        assert_eq!(resolved.preserved_member_ids, ["a"]);
        receipt.disposition = DuplicateDispositionV1::PreserveDistinct;
        assert_eq!(
            verify_reviewed_resolution(&group, Some(&receipt), &trust).unwrap_err(),
            DuplicateGroupError::InvalidReviewReceipt
        );
    }

    #[test]
    fn winner_metadata_must_be_bijectively_bound_to_group_seals() {
        let candidates = vec![
            candidate("a", "same text", 'a', 1, false),
            candidate("b", "same text", 'b', 9, false),
        ];
        let group = deterministic_exact_groups(&candidates).unwrap().remove(0);
        assert_eq!(
            resolve_deterministic_group(&group, &candidates[..1]).unwrap_err(),
            DuplicateGroupError::InvalidMember
        );
        let mut forged = candidates.clone();
        forged[1].semantic_seal = "c".repeat(64);
        assert_eq!(
            resolve_deterministic_group(&group, &forged).unwrap_err(),
            DuplicateGroupError::GroupDigestMismatch
        );
        let mut duplicate = candidates.clone();
        duplicate.push(candidates[0].clone());
        assert_eq!(
            resolve_deterministic_group(&group, &duplicate).unwrap_err(),
            DuplicateGroupError::DuplicateMember
        );
    }

    #[test]
    fn text_and_scope_do_not_group_different_semantics() {
        let mut different = vec![
            candidate("a", "same text", 'a', 1, false),
            candidate("b", "same text", 'b', 1, false),
        ];
        different[1].semantic_equivalence_digest = "f".repeat(64);
        assert!(deterministic_exact_groups(&different).unwrap().is_empty());
    }
}
