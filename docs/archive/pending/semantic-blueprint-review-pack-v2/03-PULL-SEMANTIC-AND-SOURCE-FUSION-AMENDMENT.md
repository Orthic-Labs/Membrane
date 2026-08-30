# Pull Amendment — Semantic Knowledge and Exact Source Fusion

> **Status:** Fable review draft — not canonical.
> **Date:** 2026-08-29
> Existing canonical subsystem documents win on conflict. Re-derive implementation state before execution.

## Decision

Pull owns the request-time choice between:

- Cortex durable semantic knowledge;
- Ledger exact document evidence;
- Blueprint repository truth;
- existing providers.

No provider upgrades its own authority.

## Runtime pattern

```text
query
  |
  +--> Cortex semantic candidates
  |       |
  |       +-- sufficient + policy permits -> admit
  |       |
  |       +-- source required / stale / contested / insufficient
  |               |
  |               v
  |          Ledger exact resolve
  |
  +--> Ledger direct retrieval when query/task predicts source detail
  |
  +--> Blueprint / other providers
             |
             v
          Pull fusion
```

Whether Ledger acquisition is concurrent or lazy must be evaluated rather than hard-coded as doctrine.

## Candidate distinction

Keep semantic and source candidates typed separately:

```text
provider=cortex
kind=durable_semantic_knowledge
evidence_refs=[ledger...]

provider=ledger
kind=source_document_span
doc_id/node_id/span_hash/...
```

Their scores are heterogeneous; Pull fuses them.

## Context minimization

A document hit never means “send the file.”

Pull requests the smallest sufficient Ledger structural region and expands only when required.

Receipts should expose:

```text
ledger_seed_node
resolved_node_count
expansion_hops
resolved_source_bytes
resolved_tokens
whole_document_bytes
context_savings_ratio
sufficiency_outcome
```

## Stale evidence

If a Cortex record cites Ledger evidence that no longer resolves current:

- do not silently treat it as current source-backed knowledge;
- fetch current source if possible or omit/abstain;
- emit a typed request-time receipt.

Cortex owns persistent lifecycle updates.

## Qualification

Compare Cortex-only, Ledger-only, combined, and compiled-semantic paths.

Measure task success, tokens, latency, source-resolution frequency, stale evidence caught, unsupported semantic records admitted, and manual correction/search rate.
