# Adapt Insights — harness-efficiency gap closure

**Status:** pending target-state extension; no landed-state claim

**Owners:** Each execution host/job owner owns mechanical facts & operational traces (CodeRight for
harness execution; the tray/daemon job owner for internal background work). Adapt Insights owns
failure/waste meaning; Cortex owns admitted durable issues; Hub renders read-only projections.

## 1. Purpose

Adapt Insights must be primary semantic mechanism for identifying observable harness failures &
inefficiencies. This includes orchestration, dispatch, routing, context, cache, model, tool,
verification, retry, subagent, integration & background-learning waste.

“Primary” does not mean omniscient. Every supported execution must run every applicable qualified
detector & publish coverage. Unobserved facts stay typed `unavailable`; unknown failure families
enter emergent discovery. Absence of a finding means only that no qualified detector fired under
recorded coverage.

This document extends Adapt atoms `ADP-036`–`ADP-038` & CodeRight target
contracts `CODERIGHT-EVIDENCE-PRODUCTION-FOR-MEMBRANE.md` and
`CODERIGHT-MEMBRANE-CONTEXT-INTEGRATION.md`. It adds no second trace store, semantic store,
evaluator platform or authority.

## 2. Confirmed current gap

Today’s pending contracts already require:

- stable session/task/trace/model-call/tool-call/artifact identity;
- session tool calls, failures, retries, files changed, subagents & verification;
- fresh input, cache read, cache write, output, reasoning & cost;
- per-tool schema/result cost;
- `retry_loop_cost`, `subagent_context_duplication`, context rehydration & repeated retrieval
  findings;
- same-input experiments across orchestration & harness variants.

They do not yet close:

- dispatch, assignment, lane & parent/child execution identity;
- orchestrator, worker, integrator & verifier role identity;
- assignment-specific request/token/context/tool/time/fan-out budgets;
- detection of one assignment executed by multiple workers;
- detection of an orchestrator doing lane-owned work while also dispatching that lane;
- overlap between worker scopes, reads, writes, tool calls & accepted artifacts;
- no-progress model/tool loops;
- cost per accepted assignment outcome or artifact;
- detector coverage showing what ran, skipped or lacked evidence;
- qualification proving high cache reuse alone does not become a false waste finding.

Current native `TranscriptEventV1` cannot close these gaps. It carries text, provenance, spans,
timestamp & call identity, but no execution graph or provider-usage facts. Native persistent-context
cost analysis remains separate from native Insights. H4 transport into Adapt remains pending.

## 3. Incident evidence — Membrane dispatch wave A, 2026-08-29

Committed dispatch intended one integration owner plus six disjoint, edit-only lanes. Workers had
exact file allowlists, no tests/builds & at most two retries.

Observed Pi execution diverged:

1. `membrane-dispatch-a-glm` received an explicit worker prompt to execute **all six lanes one at a
   time**.
2. Dedicated lane sessions later executed those same six assignments.
3. Full-packet worker therefore duplicated work assigned to bounded lane workers; it was not acting
   only as orchestrator/integrator.
4. Full-packet GLM session remained active from 07:33 through 10:00 while dedicated lanes ran from
   08:34 through 10:00, proving overlapping execution rather than a clean handoff.

Provider-reported usage across eight dispatch-related sessions:

| Measure | Observed |
|---|---:|
| model requests | 1,138 |
| fresh input | 1,719,302 |
| cache read | 236,328,256 |
| output | 785,724 |
| cache share of billed tokens | 98.95% |
| cache read ÷ fresh-plus-output | 94.34× |
| full-packet GLM worker requests | 814 |
| full-packet GLM worker cache read | 211,134,464 |

This evidence proves dispatch duplication & extreme context replay. It does not, by itself, prove
every cached token was waste. Adapt must join execution graph, assignment contract, progress,
artifacts & outcomes before assigning causal meaning.

## 4. Required execution graph

CodeRight emits immutable H4 observations. Adapt never reconstructs these facts from prose.

```text
DispatchExecutionObservationV1
  observation_id
  installation_id, workspace_id, repository_id, baseline_revision
  dispatch_id, assignment_id, lane_id?
  parent_assignment_id?, parent_session_id?, session_id, task_id, trace_id
  declared_role: orchestrator | worker | integrator | verifier
  scope_receipt
  allowed_read_roots[], allowed_write_paths[]
  expected_outputs[], intended_checks[]
  model, provider, client, route_policy?
  started_at, ended_at?, terminal_state
  child_assignment_ids[], handoff_refs[], retry_of?
  artifact_refs[], accepted_artifact_refs[], verification_refs[], outcome_refs[]
  provenance_receipt
```

`declared_role`, assignment identity & scope come from dispatch contract, not model narration.
Observed behavior may contradict declared role; that contradiction is detector input.

```text
ExecutionBudgetV1
  budget_id, assignment_id
  max_model_calls?
  max_fresh_input_tokens?, max_cache_read_tokens?, max_cache_write_tokens?
  max_output_tokens?, max_total_billed_tokens?
  max_context_tokens_per_call?
  max_tool_calls?, max_retries?, max_children?
  max_active_duration_ms?, deadline?
  budget_basis, policy_version
```

Missing optional limits remain `not_declared`. A required policy limit missing from a dispatch is
observable `budget_not_declared`; it is never interpreted as unlimited or zero.

## 5. Per-call mechanical evidence

Every model call binds its assignment, parent call, exact route & usage:

```text
ModelCallObservationV1
  model_call_id, assignment_id, session_id, task_id, trace_id, parent_model_call_id?
  model, provider, route_policy, request_kind
  started_at, ended_at, stop_reason, retry_of?
  context_window, rendered_context_tokens?
  fresh_input, cache_read, cache_write, output, reasoning?
  measured_or_calculated_cost
  context_receipt_id?, prompt_prefix_digest?, tool_catalog_digest?
  progress_refs[]
  provenance_receipt
```

Every tool call binds exact input/output identity & effects:

```text
ToolCallObservationV1
  tool_call_id, model_call_id, assignment_id, trace_id
  tool_id, server_id?, input_digest
  started_at, ended_at, status, exit_code?, retry_of?
  output_digest?, output_bytes, output_token_estimate?
  artifact_reads[], artifact_writes[], verification_result_ref?
  provenance_receipt
```

CodeRight may assert byte/hash identity & exact assignment overlap. Semantic equivalence remains
Adapt-owned.

## 6. Progress & useful-work boundary

Files or lines changed are not automatically useful. Analysis, diagnosis or verification lanes may
produce no source diff & still succeed. Large diffs may still be harmful.

CodeRight emits only mechanical progress refs:

- new accepted artifact digest;
- assignment output accepted by parent/integrator;
- new verified decision or resolved blocker;
- verification state transition;
- exact required path completed;
- typed terminal outcome.

Adapt joins those facts with evaluator outcomes & explicit user corrections. It may report:

- requests/tokens/cost per accepted assignment;
- requests/tokens/cost per accepted artifact;
- cost until first verified completion;
- cost after last meaningful progress;
- abandoned or rejected work cost;
- overlap cost between assignments.

It must not equate lines changed, tool calls, elapsed time or silence with value.

## 7. Required Insights detector families

| Family | Minimum evidence |
|---|---|
| `duplicate_assignment_execution` | same assignment/scope dispatched to multiple non-retry sessions |
| `orchestrator_role_leakage` | orchestrator writes lane-owned artifacts while lane worker is assigned |
| `lane_scope_overlap` | concurrent assignments share undeclared write ownership or duplicate exact work |
| `bounded_lane_budget_exceeded` | assignment exceeds declared request/token/tool/time/fan-out budget |
| `missing_efficiency_budget` | policy requires a budget but dispatch provides none |
| `fanout_without_incremental_value` | child cost with no accepted unique output or resolved dependency |
| `subagent_context_duplication` | repeated context/handoff material across child calls beyond declared strategy |
| `context_replay_amplification` | repeated context cost is disproportionate to progress under comparable work |
| `cold_cache_rebuild` | large stable prefix repeatedly written with no cache reuse |
| `cache_invalidation_churn` | prefix/catalog/context identity changes repeatedly cause avoidable rebuilds |
| `no_progress_model_loop` | bounded call window yields no new progress ref, decision, blocker or terminal state |
| `duplicate_tool_work` | identical tool work repeats outside declared verification/retry policy |
| `semantic_tool_work_overlap` | Adapt-qualified equivalent searches/reads/checks recur across workers |
| `oversized_tool_result_replay` | large tool result remains repeatedly billed after usefulness ends |
| `retry_loop_cost` | retries exceed policy or repeat unchanged failing preconditions |
| `verification_churn` | repeated equivalent verification adds cost without state change |
| `replan_churn` | repeated plan changes add no accepted scope/decision change |
| `routing_cost_mismatch` | actual model/provider violates route policy or task-tier constraint |
| `integration_rework_from_lane_failure` | integrator repeats worker work due rejected/incomplete lane output |
| `stranded_worker_work` | completed child output is never accepted, rejected or explicitly superseded |
| `background_learning_over_budget` | learning cost exceeds declared budget or attributable mitigated value |

### 7.1 Semantic-compilation worker profile

The pending source-bound semantic-compilation review pack adds one internal background-work
profile. Its execution owner must emit batch/source/compiler identity, per-call usage, resource
contention, candidate counts & every Cortex admission terminal state. Adapt then qualifies:

- `duplicate_semantic_batch_execution`;
- `unchanged_node_recompilation`;
- `semantic_candidate_rejection_churn`;
- `orphaned_semantic_candidate`;
- `semantic_worker_foreground_interference`;
- `semantic_worker_over_budget`;
- `semantic_compilation_low_yield`.

Exact contract, hard negatives & ownership constraints live in
`semantic-blueprint-review-pack-v2/04-ADAPT-DOCUMENT-COMPILATION-BOUNDARY-AMENDMENT.md`.
This does not route generic document extraction through Adapt. Expected abstention, negative-node
batches, high cache reuse & high rejection rates remain hard negatives unless joined to a budget,
duplication, terminal-state, interference or qualified-baseline failure.

Thresholds must be task/lane-policy or calibrated-baseline based. No universal “five files should
take N calls” rule is valid.

High cache share alone is not a failure. A finding requires budget breach, avoidable duplication,
no-progress evidence, outcome comparison or a qualified baseline.

## 8. Coverage receipt

Every analyzed assignment emits:

```text
InsightDetectorCoverageV1
  coverage_id, dispatch_id?, assignment_id, session_ids[]
  observation_range, observation_digest
  detector_id, detector_version
  status: ran | skipped | unavailable | failed
  required_fields[]
  unavailable_fields[] + typed reasons
  episode_ids[]
  evaluator_receipt?
  honesty_limit
```

Hub must show coverage beside findings. This prevents “no issue found” from hiding missing usage,
missing parent/child identity or an uninstrumented provider.

## 9. Episode, issue & remediation flow

```text
CodeRight operational trace
  → H4 execution observations
  → Adapt deterministic efficiency detectors
  → FailureEpisodeV1
  → recurrence grouping / emergent discovery
  → InsightIssueV1
  → reviewed remediation or routing/evaluator proposal
  → CodeRight experiment
  → outcome observations
  → Adapt recurrence-after-mitigation
```

Single catastrophic runs may emit high-severity episodes immediately. Durable issue confirmation &
instruction-surface mutation still obey recurrence, attribution, review & held-out qualification.

Likely remediation target must remain explicit:

- dispatch packet/assignment compiler;
- orchestrator policy;
- routing policy;
- context/handoff policy;
- tool implementation/schema;
- retry/verification controller;
- model behavior;
- provider/cache infrastructure;
- evaluator or detector.

Current instruction text is not a valid mutation target when it already required bounded execution.

## 10. Incident detector expectation

Given 2026-08-29 dispatch evidence, qualified detectors should emit at least:

- `duplicate_assignment_execution` — full-packet worker plus dedicated lane workers;
- `orchestrator_role_leakage` or equivalent role mismatch — supposed orchestration path executed all
  lane work directly;
- `bounded_lane_budget_exceeded` — if request/token budgets had been declared;
- `missing_efficiency_budget` — current packet capped retries but not requests, tokens, context or
  tool calls;
- `context_replay_amplification` — only after joining progress & accepted outputs;
- `routing_cost_mismatch` only if actual route violated packet/config policy.

This incident should not be reduced to `user_frustration`, `overengineering` or high cache share.
Those are secondary signals, not mechanical diagnosis.

## 11. Qualification

Required positive & hard-negative cases:

1. One orchestrator dispatches six disjoint lanes, performs no lane writes & integrates accepted
   outputs: no duplication/role-leakage finding.
2. Full-packet worker executes all lanes, then six dedicated workers repeat them: duplication &
   role-leakage fire even when prose differs.
3. Explicit retry reuses same assignment after typed failure: classified as retry, not duplicate.
4. High cache-read share remains within budget & produces steady accepted progress: no waste finding.
5. High cache-read share plus no-progress loop or duplicate assignment: implicated cost is reported.
6. Analysis-only lane produces no diff but returns accepted diagnosis: no “zero useful work” finding.
7. Large intended migration touches many files within contract: no size-only finding.
8. Usage unavailable from provider: coverage says `provider_omitted`; no invented zero or cost.
9. Two workers issue semantically similar but byte-different searches: only qualified Adapt semantic
   detector may report overlap.
10. Orchestrator integrates or repairs only after a typed rejected lane result: allowed integration,
    not role leakage.

Promotion requires deterministic mechanics fixtures, development corpus, frozen held-out corpus,
production-path H4 transport proof, detector precision/recall reporting, false-positive review &
recurrence-after-mitigation evidence.

## 12. Implementation closure

Capability remains pending until all are true:

1. CodeRight persists dispatch graph, assignment contracts, per-call usage, tool effects, progress &
   outcomes in its operational trace store.
2. CodeRight emits joinable H4 observations with per-field coverage & provenance.
3. Internal background-job owners emit equivalent typed observations, budgets, terminal outcomes &
   coverage without routing generic traces through Adapt.
4. Membrane validates & transports H4 without semantic pre-labelling.
5. Native Adapt Insights consumes execution observations alongside transcript events.
6. Qualified detectors emit evidence-bound episodes & coverage receipts.
7. Cortex admits only reviewed/sealed durable issues.
8. Hub shows assignment graph, measured cost, implicated cost, evidence, coverage, candidate cause &
   remediation target.
9. Controlled replay of this dispatch fixture detects duplication without treating cache reuse or
   file count alone as failure.
