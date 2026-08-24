# Native contract fixture corpus — N1 freeze (v1)

Language-neutral, hashed, versioned fixtures for Membrane's **internal domain
contracts**, frozen per migration spec section N1. They exist independently of
any legacy Python/Node implementation and are consumed by native Rust ports as
differential/golden corpora.

## Internal, not public

`TranscriptEventV1`, `UserActEvidenceV1`, `FailureEpisodeV1`, `InsightIssueV1`,
and the Adapt record contracts here are internal domain contracts. Their
schema/version/digest records live in `../native-contracts-manifest.v1.json`;
they are deliberately NOT added to the public protocol registry. The five
public V1 shapes (`ScopeGrantV1`, `ContextCandidateSetV1`, `ContextPacketV1`,
`ContextReceiptV1`, `KnowledgeEmissionV1`) remain unchanged.

## Files

| File | Contract | Source of truth |
|---|---|---|
| `transcript-event-v1.schema.json` | TranscriptEventV1 | `engine/crates/membrane-transcript/src/event.rs` |
| `user-act-evidence-v1.schema.json` | UserActEvidenceV1 | `engine/crates/membrane-transcript/src/evidence.rs` |
| `failure-episode-v1.schema.json` | FailureEpisodeV1 | `engine/crates/membrane-adapt/src/insights/mod.rs` |
| `insight-issue-v1.schema.json` | InsightIssueV1 | `engine/crates/membrane-adapt/src/insights/mod.rs` |
| `preference-record-v1.schema.json` | PreferenceRecordV1 | `engine/crates/membrane-adapt/src/record.rs` |
| `frozen-preference-manifest.schema.json` | preference manifest 1.3.0 | frozen snapshot of `adapt/src/adapt/preference-manifest.schema.json` |
| `frozen-remediation-proposal.schema.json` | remediation proposal | frozen snapshot of `adapt/src/adapt/remediation-proposal.schema.json` |
| `examples/*.example.json` | golden instances validated by `scripts/ci/check-native-contract-fixtures.mjs` | — |

## Immutability

Every file is content-hashed in `../native-contracts-manifest.v1.json`.
Changing a fixture requires a new corpus id (`native-contracts-v2`, …) and a
recorded reason; in-place edits fail CI. Known-wrong legacy behavior is never
frozen here as normative — intentional corrections must be recorded as
versioned expected deltas in the manifest's `intentionalDeltas`.
