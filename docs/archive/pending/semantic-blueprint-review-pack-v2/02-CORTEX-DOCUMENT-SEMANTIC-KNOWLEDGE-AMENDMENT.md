# Cortex Amendment — Document-Derived Semantic Knowledge

> **Status:** Fable review draft — not canonical.
> **Date:** 2026-08-29
> Existing canonical subsystem documents win on conflict. Re-derive implementation state before execution.

## Decision

Cortex owns durable document-derived semantic knowledge because it already owns admission, identity, duplicate/conflict handling, supersession, lifecycle, storage, and retrieval.

The compiler is a **Cortex admission producer** consuming Ledger evidence through a typed seam. It is not a new subsystem and does not directly access Ledger storage.

## Pipeline

```text
Ledger SemanticSourceDeltaV1
        |
        v
semantic compiler
        |
        v
DocumentKnowledgeCandidateV1
        |
        v
Cortex admission
  retain | merge | supersede | conflict
  no_op | proposal | quarantine | reject
```

## Candidate

```text
DocumentKnowledgeCandidateV1
  semantic_kind
  statement
  scope
  confidence
  applicability?
  valid_from?
  valid_until?
  evidence[]: LedgerEvidenceRefV1
  derivation
    compiler_version
    model_id?
    model_digest?
    prompt_schema_version?
  ambiguous?
```

`confidence` is extractor confidence, never authority.

A document-derived candidate without valid Ledger evidence is inadmissible as source-backed knowledge.

## Admission rules

Before durable admission, verify:

1. producer authorization;
2. evidence resolves to the expected current Ledger source/span;
3. schema validity;
4. duplicate/near-duplicate identity;
5. conflict/supersession semantics;
6. temporal validity where relevant;
7. underlying source authority is preserved, never upgraded;
8. repository truth remains Blueprint-owned.

Derived knowledge may be useful and durable without becoming source truth.

## Revalidation

Records retain Ledger evidence refs.

When cited source evidence changes:

```text
Cortex record
   |
   v
re-resolve Ledger evidence
   |
   + current   -> keep
   + relocated -> update evidence after validation
   + stale     -> needs_revalidation
   + missing   -> evidence_missing / retire by policy
```

Stale derived knowledge must not silently appear current.

## Retrieval

Cortex can return the semantic record cheaply. Pull requests exact Ledger source when:

- the task/risk class requires verification;
- the caller asks for source;
- evidence is old, contested, or stale;
- the semantic record is insufficient.

This is the main token-saving benefit: semantic recall first when safe, exact source recovery when necessary.

## Automatic admission policy

Do not assume all extracted classes should auto-admit.

Initial policy should favor precision:

- low-confidence/ambiguous -> drop or proposal;
- unsupported evidence -> reject;
- contradiction -> conflict, never overwrite;
- high-risk semantic kinds may require stricter review.

The admissible auto-write set is a qualification decision, not an extractor decision.

## Evaluation

Label real Ledger nodes containing positives, negatives, negation, temporal state, contradiction, procedures, tables/code, ambiguous prose, and instruction-like text.

Measure:

- precision/recall by semantic kind;
- unsupported-claim rate;
- evidence-binding accuracy;
- conflict/supersession correctness;
- stale-revalidation correctness;
- downstream task success/context savings.

For unattended automatic admission, precision and unsupported-claim rate dominate recall.
