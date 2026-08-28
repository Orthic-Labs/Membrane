# CodeRight ↔ Membrane Observability, Learning, and Evaluation Integration

**Date:** 2026-08-25  
**Status:** target integration architecture for CodeRight and Membrane  
**Scope:** CodeRight harness events, Membrane capability binding, Adapt evidence flow, Cortex/Ledger persistence boundaries, generic eval/trace infrastructure, and the closed improvement loop  
**Companion documents:** revised Adapt canon, Ledger indexing canon, Membrane cross-subsystem improvement plan, native-Rust migration specification

## Executive decision

CodeRight is the execution harness.

Membrane is mandatory CodeRight context/knowledge/learning infrastructure.

The correct high-level loop is:

```text
CodeRight execution
        ↓
typed evidence + outcomes
        ↓
Membrane normalization / subsystem routing
        ↓
Adapt behavioral learning
        ↓
Cortex durable admission
        ↓
Membrane retrieval/context delivery
        ↓
CodeRight action / evaluation / routing / guards
        ↓
measured outcomes
        └────────────────────────→ Adapt
```

But this must not be simplified into "CodeRight sends everything to Cortex."

There are distinct data classes and owners:

- CodeRight owns live execution and high-volume raw trace/eval storage.
- Membrane transcript/event infrastructure normalizes behavioral evidence.
- Adapt interprets experience into Taste and Insights.
- Cortex stores admitted durable knowledge.
- Ledger registers/indexes/resolves document-shaped sources and generated virtual documents.
- Blueprint owns repository truth.
- Pull owns final context evidence fusion/admission.
- Push owns faithful reduction.
- CodeRight consumes the resulting context/knowledge and executes the agent system.


## Runtime lifecycle binding (normative)

These decisions are canonical and take precedence over any wording later in this
document that implies a different runtime topology:

- Membrane runtime exists only inside the headless child daemon of the visible
  native tray, with OS-enforced lifetime coupling. There is no standalone or
  orphanable Membrane runtime.
- There is **no embedded CodeRight Membrane backend**. CodeRight binds through
  the active tray-owned daemon, or it has no binding.
- MCP and CLI surfaces are **stateless daemon clients/transports**. They never
  launch, auto-start, or register a Membrane process.
- **Tray off → no Membrane context.** Requests return typed
  `membrane_unavailable { reason: hub_inactive, retryable: true }`.
- **Ledger** is the canonical subsystem name; it replaces Guide.
- Blueprint is **independently usable but not independently resident**.
  Continuous watcher/freshness runs only inside the tray-owned daemon; with tray off, Blueprint
  access is an explicit bounded one-shot operation that never daemonizes.

---

# 1. Mandatory Membrane dependency

## 1.1 What "mandatory" means

A full CodeRight agent session MUST NOT start without one compatible Membrane capability binding.

The binding is exactly one thing: **a compatible Membrane capability served by
the active tray-owned daemon**, selected through the versioned handshake. There is no embedded
CodeRight Membrane backend and no second binding mode.

CodeRight must not create two knowledge universes.

If the tray-daemon binding later fails, or tray is not active:

- do not open any embedded/local fallback Cortex store;
- do not dual-write;
- memory/knowledge-required operations return typed unavailability;
- unknown commit outcomes remain unknown until the original backend receipt is reconciled;
- recovery rebinds only to the same compatible store identity unless an explicit migration/restart occurs.

The CodeRight daemon may enter a diagnostics/degraded shell without Membrane if that is useful operationally, but **agent execution requiring Membrane must remain blocked** until a valid binding exists.

## 1.2 Startup handshake

The CodeRight↔Membrane handshake should bind:

```text
protocol/integration version
Membrane build/version
capability set
six-subsystem names
Cortex store identity
Ledger index identity/version
Blueprint availability/version
Adapt contract versions
Pull/Push capability versions
native-only/runtime compatibility
installation identity
```

Required failure classes include:

- incompatible version;
- missing required capability;
- store identity mismatch;
- migration required;
- backend unavailable;
- partial/degraded optional capability.

No ambiguous "connected=true" boolean.

---

# 2. CodeRight ownership

CodeRight owns live transactional execution state and generic AI-engineering telemetry.

## 2.1 Live runtime state

CodeRight owns:

- sessions;
- goals/tasks;
- orchestration;
- workers/subagents;
- model connections;
- route decisions;
- queues;
- retries;
- cancellation;
- approvals;
- tool execution;
- verification flow;
- live budgets;
- runtime artifacts;
- agent state machines.

## 2.2 Raw trace/evaluation store

CodeRight SHOULD own a local trace/eval store for high-volume operational data.

This is not a duplicate Cortex.

It may contain:

- trace;
- span;
- model call;
- tool call/result;
- subagent;
- retrieval/context decision;
- tokens;
- latency;
- cost;
- retries;
- error;
- evaluator score;
- experiment run;
- dataset execution;
- routing comparison;
- prompt/harness version.

Retention can be bounded/configurable.

Raw trace storage is optimized for operational analysis and experiments, not semantic long-term recall.

## 2.3 Generic eval platform

The Langfuse/Phoenix-style infrastructure belongs in CodeRight:

- datasets;
- dataset cases;
- experiments;
- evaluators;
- run evaluators;
- scores;
- online evaluators;
- human annotations;
- comparison dashboards/TUI;
- prompt/model/routing/harness variants;
- CI regression gates.

Adapt consumes behaviorally relevant outcomes but does not implement the generic eval platform.

---

# 3. Membrane ownership

## 3.1 Cortex

Cortex stores admitted durable knowledge, including:

- memories;
- temporal facts;
- decisions;
- procedures;
- durable observations;
- Taste preferences;
- Insight issues;
- admitted tasks/artifacts when intended for later semantic recall;
- other governed knowledge.

Cortex does not ingest every raw trace/span automatically.

## 3.2 Ledger

Ledger is the renamed document registry/index/navigation subsystem.

It may index:

- repository Markdown;
- user documentation;
- runbooks;
- decision docs;
- policy docs;
- generated CodeRight session/handoff documents after a typed virtual-source contract exists.

Ledger is not the raw CodeRight event store.

## 3.3 Adapt

Adapt consumes evidence about behavior and produces governed Taste/Insights proposals.

## 3.4 Pull/Push/Blueprint

- Pull selects/admit/fuses current task evidence.
- Push faithfully reduces selected evidence.
- Blueprint owns repository/code truth.
- CodeRight consumes their outputs but does not replicate their canonical stores.

---

# 4. CodeRight event architecture

Current transcript mining remains useful for external hosts.

CodeRight should provide stronger first-party evidence because it owns the harness.

Use four lanes.

## 4.1 Lane A — transcript/trajectory events

Project conversation/tool trajectory into `TranscriptEventV1`-equivalent semantics:

- user message;
- assistant message;
- tool call;
- tool result;
- thinking omitted marker;
- sidechain/subagent;
- timestamp;
- source/session identity;
- repo/cwd;
- provenance.

For CodeRight these should be emitted directly from the runtime event conduit.

Do not serialize a textual transcript only to parse it back.

## 4.2 Lane B — explicit selected-transcript evidence

Membrane accepts only transcripts the user explicitly selects for Adapt. Each selected source binds:

- exact transcript digest;
- exact source span;
- external-user role;
- session/source identity;
- scope context;
- required review decision before apply.

Login, host signing, or implicit runtime capture do not grant Taste authority. CodeRight may expose a transcript reference for selection, but it does not mint an Adapt authority receipt.

Silent acceptance remains support-only and cannot independently establish Taste authority.

## 4.3 Lane C — structured execution observations

Introduce a versioned CodeRight↔Adapt observation contract.

Recommended shape:

```text
ExecutionObservationV1
  observation_id
  session_id
  task_id?
  parent_task_id?
  agent_id?
  agent_role?
  timestamp
  model
  provider
  client
  route_policy?
  observation_kind
  subject_id?
  tool?
  call_id?
  outcome?
  exit/status code?
  duration_ms?
  usage?
  repository?
  scope?
  artifact_refs[]
  evidence_refs[]
  provenance_receipt
```

Observation kinds should cover at least:

- model selected;
- route selected;
- model call started/finished/failed;
- tool call/result/failure;
- write/edit;
- verification started/result;
- approval request/result;
- retry;
- timeout;
- cancellation;
- plan created/revised;
- task scope changed;
- subagent started/finished;
- artifact produced;
- completion claim emitted;
- completion accepted/rejected;
- user correction/steer;
- Membrane retrieval/context result;
- Push reduction/restore;
- evaluator outcome.

Mechanical facts belong here rather than regex inference.

## 4.4 Lane D — evaluation outcomes

Recommended:

```text
EvaluationOutcomeV1
  outcome_id
  evaluator_id
  evaluator_version
  dataset_id?
  case_id?
  experiment_id?
  trace_id?
  session_id?
  task_id?
  model
  client
  route_policy?
  score_type
  score/value/verdict
  expected/reference?
  execution_receipt
  baseline_ref?
  timestamp
```

Adapt uses these to determine whether mitigations or Taste delivery changed outcomes.

---

# 5. What CodeRight should not emit as facts

Do not emit:

```text
user_prefers_simple_architecture = true
```

unless this is a projection of an already-admitted Taste record.

Instead emit the evidence:

```text
user rejected architecture proposal B
user accepted proposal A
user edited broad rewrite into local patch
```

Adapt interprets the behavioral preference.

Do not emit:

```text
insight = architecture_churn
```

as a raw runtime fact unless CodeRight is explicitly executing a versioned approved Adapt detector/evaluator.

Instead emit:

```text
proposal A accepted
proposal B introduced without revisit trigger
files changed
proposal C introduced
user reverted
```

Adapt owns issue semantics.

---

# 6. CodeRight trace/eval data model

## 6.1 Trace

A trace represents one end-to-end task/session execution unit.

Recommended linkage:

```text
Trace
  session
  task/goal
  model/route
  context receipt
  spans[]
  final outcome
  scores[]
```

## 6.2 Span

Span types:

- model;
- tool;
- retrieval;
- Membrane context;
- Push transform;
- subagent;
- verifier;
- evaluator;
- external service.

Every span should have stable identity and parentage.

## 6.3 Score

A generic score model is appropriate inside CodeRight.

Inspired by Langfuse:

```text
Score
  name
  type: numeric | categorical | boolean | text
  value
  target: trace | span | session | dataset-run
  evaluator/user/source
  comment?
  timestamp
```

This generic score object is CodeRight eval infrastructure.

Do not use it to erase Adapt's typed `Taste`, `FailureEpisode`, or `InsightIssue` semantics.

## 6.4 Dataset

Dataset cases may come from:

- hand-authored benchmarks;
- existing CodeRight evaluation suites;
- reviewed production failures;
- Adapt `RegressionCaseProposal`;
- bug regressions;
- routing/model comparisons.

Cases need immutable identity/digest and provenance.

## 6.5 Evaluator

Evaluator types:

- deterministic code;
- test/build verifier;
- artifact/receipt verifier;
- LLM judge;
- policy checker;
- Adapt-approved behavioral evaluator;
- human annotation.

Version evaluator definitions.

Every evaluator run must be traceable to:

- evaluator version;
- case;
- app/harness/model version;
- result.

## 6.6 Experiment

An experiment compares a controlled change over a fixed dataset:

- model A/B;
- prompt A/B;
- route policy A/B;
- context policy A/B;
- Taste enabled/disabled;
- Insight guard enabled/disabled;
- Push mode;
- Ledger retrieval strategy;
- agent/orchestration strategy;
- harness change.

Same inputs and evaluation criteria are required for comparison.

---

# 7. Braintrust/Langfuse/Phoenix lessons assigned correctly

## 7.1 Braintrust

Absorb into the combined CodeRight/Adapt loop:

- production trace pattern discovery;
- recurring topic/failure discovery;
- finding → dataset;
- baseline;
- fix;
- evaluation;
- online monitoring.

Ownership split:

- CodeRight stores/executes traces and experiments;
- Adapt owns behavioral candidate patterns and Insight semantics.

## 7.2 Langfuse

Absorb into CodeRight:

- trace/observation/session scoring;
- datasets;
- experiments;
- online/offline eval;
- manual annotation;
- annotation queues;
- score analytics.

Absorb into Adapt UX:

- structured review queue for issues/patterns/remediations.

## 7.3 Phoenix

Absorb into CodeRight:

- dataset-attached evaluators;
- deterministic and LLM evaluator support;
- experiment comparison;
- evaluator execution tracing;
- failure-mode → evaluator workflow.

Absorb into Adapt:

- Insight issue → proposed evaluator;
- evaluator identity linked back to issue;
- recurrence/effectiveness interpretation.

---

# 8. Deterministic completion integrity

CodeRight has a major advantage over transcript-only systems: it knows whether tools and verifiers actually ran.

Use that.

## 8.1 Completion claim event

When an agent produces a completion claim, CodeRight should emit a typed claim record with references to:

- task/goal;
- claimed artifacts;
- claimed checks;
- claimed writes;
- claimed tests;
- claimed external effects.

## 8.2 Evidence cross-check

Approved deterministic guards may compare claims to:

- tool receipts;
- artifact digests;
- filesystem state;
- git diff;
- test result;
- external write receipt;
- re-fetch/readback where required.

HORKOS is a useful reference for this architecture.

## 8.3 Ownership

Exact deterministic completion guard:

- CodeRight hot path / policy/evaluator.

Recurring false-completion pattern:

- Adapt Insights.

Durable confirmed recurring issue:

- Cortex after admission.

This prevents Adapt from being a synchronous bottleneck for mechanically verifiable facts.

---

# 9. Membrane context observability

Every CodeRight model call SHOULD bind the Membrane context receipt that produced its delivered context.

This enables measurement of:

- which Cortex records were delivered;
- which Ledger nodes were delivered;
- Blueprint evidence;
- provider omissions;
- Pull sufficiency;
- Push reductions;
- tokens by source/class;
- whether the model later used/ignored/requeried evidence.

Do not store private reasoning to infer "usage."

Use observable behavior such as:

- citations/references;
- tool/search follow-up;
- user correction;
- evaluator outcome;
- explicit retrieval/refetch;
- task success.

"Apparently unused" is the strongest claim when influence cannot be observed directly.

---

# 10. Adapt feedback loop inside CodeRight

## 10.1 Taste

Flow:

```text
explicit user-selected transcript evidence
   ↓
Adapt Taste proposal
   ↓
Cortex admission
   ↓
Membrane Pull selects applicable preference
   ↓
CodeRight model call
   ↓
delivery receipt bound to execution
   ↓
CodeRight outcome/user action
   ↓
Adapt effectiveness
```

## 10.2 Insights

Flow:

```text
execution observations + transcript semantics + eval outcomes
   ↓
known detector / emergent discovery
   ↓
FailureEpisode
   ↓
InsightIssue
   ↓
remediation / regression / evaluator proposal
   ↓
CodeRight experiment / live guard
   ↓
outcome
   ↓
Adapt recurrence tracking
```

---

# 11. Reviewed production failure → regression case

This is the key improvement loop.

Do not automatically copy raw private traces into permanent datasets.

Pipeline:

1. Adapt identifies/receives a confirmed issue.
2. Produce `RegressionCaseProposal`.
3. Redact or synthesize the minimal behaviorally equivalent test.
4. Preserve source issue/provenance/digest.
5. Human/reviewer approves.
6. CodeRight dataset owner admits the case.
7. Attach evaluator(s).
8. Run baseline.
9. Implement remediation.
10. Re-run.
11. Deploy only if gates pass.
12. Monitor online recurrence.

---

# 12. Human review surfaces

CodeRight CLI/TUI is the natural front end.

## 12.1 Adapt Insights queue

Show:

- family;
- evidence;
- recurrence;
- models/clients;
- severity;
- detector/evaluator version;
- precision/qualification status;
- candidate mechanism;
- remediation;
- post-mitigation recurrence.

Actions:

- confirm;
- dismiss;
- split;
- merge;
- approve regression case;
- approve remediation;
- mark obsolete.

## 12.2 Taste queue

Show:

- exact user evidence;
- explicit vs behavioral;
- scope;
- precedence;
- conflict;
- effect;
- delivery/effectiveness.

Actions:

- approve;
- edit wording with new receipt;
- narrow;
- deactivate;
- supersede;
- delete/forget where allowed.

---

# 13. Data routing table

| Data | Canonical owner |
|---|---|
| live queue/attempt/agent state | CodeRight |
| raw high-volume traces/spans | CodeRight |
| eval datasets/experiment runs | CodeRight |
| generic evaluator definitions | CodeRight |
| generic scores/annotations | CodeRight |
| raw external transcript before admission | source host |
| normalized transcript/event | Membrane transcript/event layer |
| selected transcript source/hash/span & review | Membrane Adapt selection/review boundary |
| structured CodeRight execution observation | CodeRight→Membrane integration contract |
| durable memory | Cortex |
| durable Taste preference | Adapt semantics → Cortex |
| durable Insight issue | Adapt semantics → Cortex |
| repository truth | Blueprint |
| registered Markdown/index | Ledger |
| generated session/handoff document | Ledger after virtual-source qualification |
| context packet/receipt | Pull/Membrane |
| reduction artifact/receipt | Push |
| routing/action | CodeRight |

---

# 14. Store non-duplication rule

"Do not duplicate Membrane stores" does **not** mean CodeRight cannot have its own operational trace database.

The actual rule is:

- CodeRight does not create a second durable semantic-memory/knowledge universe beside Cortex;
- CodeRight does not create a second document index beside Ledger for the same canonical function;
- CodeRight does not create a second repository graph beside Blueprint;
- CodeRight may store its own execution/eval telemetry because that is its canonical domain.

When selected raw operational records need long-term semantic recall, they cross explicit Cortex admission.

---

# 15. Retrieval and Ledger outcome events

CodeRight should help measure Membrane retrieval quality.

For each relevant model call/task record:

- query/task fingerprint;
- required evidence dimensions;
- Pull receipt;
- Ledger candidate ids;
- Cortex record ids;
- Blueprint evidence refs;
- omissions;
- Push transforms;
- model route;
- final evaluator outcome.

Then record observable follow-up:

- repeated manual search for missing material;
- resolver/refetch;
- user "wrong docs"/"you missed..." correction;
- task failure caused by missing context;
- task success with smaller context.

Adapt may convert recurring failures into Insights.

The owning subsystem remains Pull/Ledger/Push.

---

# 16. Implementation ownership

This canonical document defines cross-product ownership, contracts, and acceptance conditions. It
does not prescribe rollout phases.

- Membrane-owned non-experimental work lives in
  `docs/pending/MEMBRANE-PENDING-IMPLEMENTATION.md` and is scheduled only from verified
  production-path gaps.
- CodeRight-owned implementation work lives in CodeRight's repository under its own pending
  specification.
- Neither repository may infer landed state from this doctrine; required tests below must bind to
  each product's real production path.

---

# 17. Required tests

## Startup/backend

- compatible tray-daemon binding;
- tray inactive at startup — typed `membrane_unavailable { hub_inactive }`, no binding, no spawned process;
- tray quits mid-session — binding lost, typed unavailability, no fallback store;
- incompatible version;
- store identity mismatch;
- backend death;
- no local fallback after tray-daemon bind;
- migration-required path.

## Event/evidence

- message/tool event identity;
- tool call/result linkage;
- no private reasoning capture;
- user-act authentication;
- post-accept before/after digest;
- forged user act rejected;
- execution observation idempotency.

## Eval

- dataset immutability;
- evaluator version binding;
- deterministic evaluator repeatability;
- LLM evaluator prompt/model version binding;
- experiment same-input comparison;
- score attachment;
- annotation queue lifecycle.

## Adapt seam

- observation cannot self-authorize Taste;
- Insight cannot create Taste without user evidence;
- regression case provenance;
- evaluator proposal promotion;
- recurrence after mitigation;
- model/client applicability.

## Persistence boundary

- raw trace not silently inserted into Cortex;
- admitted durable record goes to Cortex;
- document virtual source goes to Ledger only after qualification;
- no duplicate CodeRight memory DB when the tray-daemon Cortex binding is selected.

---

# 18. Metrics

CodeRight should expose metrics for both harness engineering and Adapt learning.

## Harness

- task success;
- verification success;
- latency;
- token usage;
- cost;
- retries;
- tool failures;
- model/route performance;
- subagent efficiency;
- context tokens;
- evaluator scores.

## Taste

- selected;
- adhered;
- corrected;
- overridden;
- repeated correction rate;
- correctness/policy regression.

## Insights

- episode rate;
- issue recurrence;
- dismissals;
- precision/recall corpus version;
- mitigation start;
- comparable exposure;
- post-mitigation recurrence;
- reopen rate.

## Context

- Pull sufficiency;
- corrective retrieval rate;
- Ledger retrieval success;
- stale/relocation failures;
- manual search after packet;
- Push token reduction/restores.

---

# 19. Rejected designs

Do not:

- keep Adapt transcript-only inside CodeRight;
- send every trace to Cortex;
- use a generated Markdown session ledger as the only Adapt evidence source;
- let CodeRight invent Taste preferences;
- let CodeRight invent Insight authority from arbitrary labels;
- make Adapt the generic trace/eval database;
- make Cortex the generic high-volume observability database;
- maintain a CodeRight document index competing with Ledger;
- open any local/embedded memory store after a tray-daemon binding is lost;
- execute unversioned evaluators;
- tune routing/harness changes on the final held-out benchmark;
- claim model/harness improvement without baseline comparison.

---

# 20. Final canonical integration statement

> **CodeRight is the execution and evaluation harness. Membrane is mandatory context, durable-knowledge, document-navigation, repository-evidence, reduction, and behavioral-learning infrastructure. CodeRight emits exact runtime events, transcript references, execution observations, and evaluation outcomes. Users explicitly select transcripts for Adapt; exact source/hash/span binding plus required review governs Taste proposals. Cortex governs durable admission and retrieval; Ledger indexes and resolves registered documents; Blueprint owns repository truth; Pull decides task attention; Push faithfully reduces selected evidence. CodeRight then consumes those outputs to route models, build context, run guards/evaluators, and improve the harness, while measured outcomes flow back into Adapt and the subsystem-specific evaluation loops.**
