# Adapt — implementation architecture

This is a current, non-normative implementation map. Product semantics come from
[`../../docs/subsystems/ADAPT_CANONICAL_PRODUCT_AND_ARCHITECTURE.md`](../../docs/subsystems/ADAPT_CANONICAL_PRODUCT_AND_ARCHITECTURE.md).

Adapt's production owner is native Rust. `membrane-transcript` discovers and
normalizes supported host transcripts; `membrane-adapt` owns Taste and Insights
contracts, extraction, admission, manifests, evaluation, and cost analysis; the
native `membrane adapt` command owns the installed workflow. Cortex alone owns
durable admission, lifecycle, retrieval, and delivery storage.

## Production flow

1. Native host adapters produce canonical transcript events with source spans and
   provenance. An explicitly user-selected transcript is eligible when its exact source
   hash/rebinding is verified; no ambient host or login signal grants transcript authority.
2. Taste extraction emits candidates. Deterministic authority and safety checks can
   reject or quarantine them; optional model output remains proposal-only.
3. Required review and independent adjudication bind the pending manifest, canonical pool,
   complete decisions, and validation time, then bind every accepted record to one immutable
   semantic payload and digest. Duplicate groups
   are deterministic and default to abstention, preserving every member unless a valid
   reviewed merge receipt is supplied.
4. Apply revalidates the full manifest and each semantic seal immediately before one
   authenticated Cortex batch. Authority and influence are derived from the sealed
   payload, not mutable envelope fields.
5. Recall asks Cortex for scoped records. Taste selection is bounded, lifecycle-aware,
   and receipted; omission is explicit and adherence is never inferred.
6. Insights detection and persistent-context cost attribution are report-only. Native
   provider usage is measured; bounded allocation is labelled inferred; unresolved
   usage remains unattributed.

## Native components

| Area | Owner |
|---|---|
| Transcript discovery, parsing, provenance | `engine/crates/membrane-transcript/` |
| Taste extraction, authority, admission | `engine/crates/membrane-adapt/src/taste.rs`, `authority.rs`, `admission.rs` |
| Manifest, semantic seal, duplicate groups | `engine/crates/membrane-adapt/src/manifest.rs`, `seal.rs`, `duplicate_groups.rs` |
| Insights and context-cost evaluation | `engine/crates/membrane-adapt/src/insights.rs`, `context_cost.rs`, `delivery.rs` |
| Installed CLI and Cortex boundary | `engine/crates/membrane-runtime/src/cli.rs`, `store.rs` |
| Hub-owned scheduling/lifecycle | `engine/crates/membrane-runtime/`, `apps/membrane-hub/src-tauri/` |

## Evidence and current limits

- The committed Insights corpus and scorer are detector-conformance evidence, not
  proof of automated mitigation effectiveness.
- The committed Taste corpus is synthetic conformance evidence. Exact installed-artifact
  qualification remains separate from selected-transcript processing, which binds exact
  source hashes and requires review.
- Installed reviewed-merge receipt delivery remains separate qualification work; assistant,
  tool, model, and repository text remain non-authoritative.
- Legacy Python under `adapt/src/adapt/` is migration/differential material. The final
  product-wide native-only claim remains blocked until packaging proves it is absent
  from installed artifacts and the remaining native migration lanes close.
