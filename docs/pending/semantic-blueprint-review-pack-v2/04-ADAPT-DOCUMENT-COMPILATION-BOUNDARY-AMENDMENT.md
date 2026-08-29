# Adapt Amendment — Boundary With Document Semantic Compilation

> **Status:** Fable review draft — not canonical.
> **Date:** 2026-08-29
> Existing canonical subsystem documents win on conflict. Re-derive implementation state before execution.

## Decision

Generic Ledger-document semantic extraction is **not Adapt**.

Adapt remains:

- **Taste:** user-backed behavioral preferences.
- **Insights:** evidence-backed agent/model/tool failures and gotchas.

A document fact, decision, invariant, or procedure is not behavioral learning merely because an LLM extracted it.

## Explicit boundary

Add to Adapt's non-goals:

> Adapt does not compile general document semantic knowledge from Ledger into Cortex. Document-derived semantic knowledge is produced through a Cortex admission producer using Ledger source evidence.

`Cortex admission producer` describes semantic ownership of candidates and admission. It does not
yet decide which tray/daemon application job schedules Ledger deltas, owns retries, or invokes a
local/cloud compiler. Fable must close that runtime orchestration boundary without allowing either
subsystem to open the other's store.

## What Adapt may consume

Adapt may learn from outcomes of the pipeline:

- irrelevant Ledger retrieval;
- unsupported semantic extraction;
- missed explicit constraints;
- temporal/negation errors;
- stale evidence reaching Pull;
- repeated user correction of semantic records;
- context over-expansion;
- semantic-worker resource contention.

These are Insights/evaluation evidence.

Adapt may propose evaluator, guard, prompt/model, or routing changes. It cannot directly deploy them or mutate Ledger/Cortex/Pull policy.

## Required pipeline observations

The execution owner must emit mechanically known facts; Adapt must not infer them from prose:

```text
SemanticCompilationExecutionObservationV1
  job_id, batch_id, source_revision, ledger_generation
  workload_class, execution_budget_ref, qualified_baseline_ref?
  node_ids[], changed_node_count, removed_node_count
  node_attempts[]
    node_id, span_hash, model_call_ids[], candidate_ids[]
    reusable_prior_result_ref?, terminal_state
  compiler_version, prompt_schema_version?, model_id?, model_digest?
  prompt_digest?, prompt_prefix_digest?, cache_identity_digest?
  model_call_ids[], model_calls, fresh_input, cache_read, cache_write, output, measured_cost?
  elapsed_ms, cold_load_ms?, peak_rss_bytes?, accelerator_time_ms?
  queue_wait_ms, foreground_preemptions
  foreground_baseline_ref?, foreground_latency_delta_ms?, resource_budget_breach?
  no_candidate_count, candidate_count
  admitted_count, merged_count, superseded_count, conflict_count
  proposal_count, quarantined_count, rejected_count, no_op_count
  retry_of?, terminal_state, omission_reasons[]
  provenance_receipt
```

Provider-omitted usage remains typed `unavailable`; it is never converted to zero. Every candidate
must reach an explicit admission terminal state or remain linked to a pending job.

## Required Adapt Insights coverage

Compilation-specific detector families extend, rather than replace, generic harness-efficiency
detectors in `../ADAPT-HARNESS-EFFICIENCY-INSIGHTS.md`:

| Family | Minimum evidence |
|---|---|
| `duplicate_semantic_batch_execution` | identical source revision, Ledger generation, node set & compiler identity execute more than once outside declared retry/replay policy |
| `unchanged_node_recompilation` | unchanged node/compiler identity invokes inference despite a qualified reusable prior terminal result |
| `semantic_candidate_rejection_churn` | exact semantic digest or qualified-equivalent unsupported/conflicting candidates repeatedly consume work after a stable rejection without source/compiler change |
| `orphaned_semantic_candidate` | emitted candidate has no pending or terminal Cortex admission state |
| `semantic_worker_foreground_interference` | measured foreground latency/resource regression breaches declared isolation budget |
| `semantic_worker_over_budget` | job exceeds declared call/token/cost/time/memory/accelerator budget |
| `semantic_compilation_low_yield` | cost per admitted or otherwise accepted candidate breaches a qualified workload baseline; an expected all-negative batch is a hard negative |

High rejection, abstention, cache use, or zero-candidate output alone is not failure. Findings require
budget breach, duplication, missing terminal state, measured interference, or a qualified baseline.

Required hard negatives include source revision change, compiler/prompt/schema experiment change,
declared retry, expected all-negative corpus, warm-cache reuse within budget, background pre-emption
without foreground regression, and revalidation required by a changed evidence reference.

## Separate flows

```text
CodeRight/session trajectory
  -> Adapt
  -> Taste / Insights
  -> Cortex admission

Ledger document
  -> semantic compiler
  -> Cortex admission
```

Do not conflate them.

## Invariants

1. Document prose cannot create Taste authority.
2. Generic document knowledge does not route through Adapt.
3. Adapt may evaluate the pipeline but is not its store or compiler.
4. Remediation proposals still require subsystem-specific qualification.
5. Generic execution traces stay with their execution owner; Adapt stores only evidence-bound
   episodes/issues admitted through Cortex.
6. A semantic worker receives no additional source, policy, or mutation authority because Adapt
   evaluates its outcomes.
