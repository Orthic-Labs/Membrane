# Adapt Canonical Product and Architecture Specification

**Status:** Canonical product and architecture source of truth  
**Date:** 2026-08-25  
**Repository:** `Orthic-Labs/Membrane`  
**Supersedes:** prior Adapt canonical/product drafts and implementation plans where they conflict with this document  
**Companion Membrane doctrine:** `docs/subsystems/MEMBRANE_CANONICAL_ARCHITECTURE_AND_IMPLEMENTATION_DOCTRINE.md`  
**Companion runtime plan:** `migration/native-rust/MEMBRANE-NATIVE-RUST-MIGRATION-AND-CODERIGHT-INTEGRATION.md`  
**Companion Ledger plan:** `LEDGER-MARKDOWN-INDEXING-AND-DOCUMENT-NAVIGATION-CANON.md`  
**Companion CodeRight integration plan:** `CODERIGHT-MEMBRANE-OBSERVABILITY-LEARNING-AND-EVAL-INTEGRATION.md`  
**Implementation status:** descriptive status MUST be re-derived from the current `main` immediately before execution; normative semantics in this document are not commit-bound  
**Research basis:** current Membrane/Adapt code; Command Code Taste; CHIRON/HORKOS; Braintrust Topics/Loop; Langfuse evaluation/annotation workflows; Arize Phoenix datasets/evaluators/experiments; agent-failure research  
**Runtime architecture:** governed by the Membrane native-Rust migration specification; Adapt is not exempt from Membrane's native-only runtime rule  
**Audience:** Adapt, Cortex, Ledger, transcript/event infrastructure, Membrane runtime, Hub, CodeRight integration, evaluation, documentation, and release-engineering implementers

This document defines **what Adapt is**, **what it owns**, **what it must never become**, the contracts for **Taste** and **Insights**, the authority and persistence boundary with Cortex, the evidence relationship with CodeRight and other hosts, and the quality gates required before behavioral learning is allowed to influence future execution.

Research and competitor material is evidence, not authority. Implementation-status sections are descriptive snapshots and MUST be refreshed from current code before work begins. If code, README copy, generated docs, historical plans, or research conflict with the normative sections, correct the conflicting projection or raise an explicit architecture amendment.

## Runtime lifecycle binding (normative)

These decisions are canonical and take precedence over any wording later in this
document that implies a different runtime topology:

- Membrane runtime exists only inside the headless child daemon of the visible
  native tray, with OS-enforced lifetime coupling. There is no standalone or
  orphanable Membrane runtime.
- There is **no embedded CodeRight Membrane backend**. CodeRight binds to
  Membrane through Hub, or it has no binding.
- MCP and CLI surfaces are **stateless daemon clients/transports**. They never
  launch, auto-start, or register a Membrane process.
- **Tray off → no Membrane context.** Requests return typed
  `membrane_unavailable { reason: hub_inactive, retryable: true }`.
- **Ledger** is the canonical subsystem name; it replaces Guide.
- Blueprint is **independently usable but not independently resident**.
  Continuous watcher/freshness runs only inside the tray-owned daemon; with tray off, Blueprint
  access is an explicit bounded one-shot operation that never daemonizes.

---

## Document authority and relationship to other canonical specs

This document is the **Adapt semantic/product authority**. It answers: what Adapt is, what Taste and Insights mean, what Adapt owns, what evidence may authorize them, and what invariants an implementation must preserve.

It does **not** replace Membrane-wide ownership or runtime doctrine. Authority divides as follows:

1. **Membrane canonical doctrine** — cross-subsystem ownership, planner authority, the six axes, and cross-cutting implementation invariants.
2. **This Adapt canonical specification** — Adapt product semantics, Taste/Insights contracts, evidence classes, authority, governance, evaluation, and feature dependencies.
3. **Native-Rust migration specification** — runtime/process cutover, packaging, deletion, Blueprint/CodeRight seams, and native-only release closure.
4. **Ledger canonical implementation plan** — the renamed Ledger subsystem's Markdown/document registry, indexing, retrieval, resolution, virtual-document, and rollout contracts.
5. **CodeRight↔Membrane integration plan** — CodeRight execution observations, traces/evals, Membrane capability binding, and the closed learning/evaluation loop.
6. `docs/subsystems/adapt.md` and `adapt/README.md` — concise projections only; they MUST NOT invent competing semantics.
7. Research/competitor documents — non-normative provenance and comparison only.
8. Historical plans/specs — evidence of prior decisions only where explicitly retained.

For a cross-cutting conflict, the narrowest relevant canonical owner controls its semantic domain.

**CodeRight is an execution host and first-class Adapt evidence producer/consumer, not the owner of Adapt semantics.** CodeRight may emit typed transcript events, user-act evidence, execution observations, and evaluation outcomes; consume admitted Taste/Insight outputs; and execute approved guards/evaluators. It does not redefine Taste authority, Insight semantics, or Cortex admission.

**Ledger is the canonical name of the Membrane document-navigation/index subsystem.** Historical `Guide`/`Spine` terminology is retired except in migration notes and compatibility tests.

# 0. Executive decision

Adapt is Membrane's **governed behavioral-learning subsystem**.

It has two first-class product surfaces:

1. **Taste** — learns how the user wants agents to behave, reason about engineering choices, produce work, and respect recurring preferences and behavioral constraints.
2. **Insights** — detects how agents or models repeatedly fail, waste effort, violate instructions, over-engineer, mis-handle tools, frustrate the user, or exhibit recurring gotchas.

The two surfaces share ingestion, provenance, evidence handling, scoping, lifecycle infrastructure, and Cortex admission plumbing. They **do not share authority semantics**.

The permanent ontology is:

| Concept | Source / author | Semantic object | Primary question |
|---|---|---|---|
| **Agent memory** | agent / host | facts, state, summaries, notes, contextual recall | What should I remember? |
| **Taste** | explicitly user-selected, source-bound transcript evidence and reviewed behavioral evidence | preferences and behavioral constraints | How does this user want me to behave or make choices? |
| **Insights** | observed agent/model/tool trajectories and outcomes | failures, gotchas, recurring bad patterns, waste | What do agents repeatedly get wrong, and what should be prevented or measured? |
| **Cortex** | shared Membrane infrastructure | admitted durable records of many semantic kinds | How are durable records admitted, stored, versioned, retrieved, and delivered? |

**Adapt is not a memory system. Taste is not a memory type. Insights is not a memory type. Cortex being able to persist their outputs does not change their semantic category.**

The canonical one-sentence description is:

> **Adapt is Membrane's governed behavioral-learning subsystem: Taste captures user-authoritative preferences, while Insights captures evidence-backed agent/model failures and gotchas; Cortex owns durable admission, lifecycle, storage, retrieval, and delivery.**

## 0.1 Non-negotiable product rules

The following are settled and MUST NOT be weakened without an explicit architecture amendment:

- Agent-authored text cannot create user-preference authority.
- Tool output, repository text, prior memories, assistant summaries, and model narration cannot silently become Taste.
- Insights cannot claim that the user prefers something merely because a failure pattern suggests a useful intervention.
- A Taste preference and an Insight may recommend similar future behavior, but their provenance and authority remain distinct.
- Authored repository policy (`AGENTS.md`, `CLAUDE.md`, organization policy, explicit current instruction) outranks inferred Taste.
- Cortex owns durable admission/storage semantics; Adapt proposes learned behavioral records and does not become a second canonical store.
- Adapt's production runtime is native Rust. Python is current implementation/migration scaffolding, not the target architecture.
- Adapt must be useful without requiring an agent to read or reconstruct a user's preferences from full memory files.
- Insights must state what was observed and what can be supported by evidence. It is not an automatic root-cause oracle.

## 0.2 Product completion rule

Adapt is product-complete only when all of the following are true:

- Taste and Insights are both documented and shipped as first-class surfaces.
- Taste accepts explicitly selected transcript evidence without authority laundering; automatic implicit host signals are optional and separately evaluated.
- Insights has measured detector precision/recall and a longitudinal issue model.
- every durable Adapt output crosses one typed Cortex admission boundary;
- applicable Taste can be delivered through a bounded core plus cheap scoped selection;
- Insights can surface, track, and measure recurrence without pretending diagnostic evidence is a user command;
- final semantic/admission integrity is tamper-evident end to end;
- the product is inspectable, reversible, scoped, and auditable;
- the native installed product runs with Python/Node absent for Membrane-owned Adapt behavior;
- canonical docs and CI prevent future agents from relabeling Adapt as memory.

---

# 1. Canonical ontology and category boundaries

## 1.1 Memory

In this architecture, **memory** means agent/host-created contextual recall: facts, summaries, project state, user facts, prior decisions, notes, episodic context, or other information retained so an agent can reconstruct what it knows.

Memory may be valuable input to reasoning. It does not, merely by existing, grant user-preference authority.

Examples:

- `The local integration suite needs Redis on port 6379.` → memory/context.
- `The migration reached step 4 yesterday.` → memory/context.
- `The user selected SQLite for this repository.` → may be a recorded decision or contextual fact; it is not automatically Taste unless the semantics are specifically about recurring agent behavior or engineering preference.

## 1.2 Taste

Taste is **user-backed behavioral preference learning**.

A Taste item exists to influence how an agent behaves or chooses among valid alternatives in future work. It may concern workflow, verification discipline, architecture style, tooling, code style, documentation, model routing, or another approved preference category.

Examples:

- `Always run the focused test before reporting the wider build as verified.`
- `Prefer the simplest sufficient architecture; do not add abstractions before they are needed.`
- `When there is a choice, use JSONL for structured logs.`
- `Do not rewrite surrounding code when the requested fix is local.`

Taste is not defined by storage location. It is defined by **preference semantics + user-backed authority + applicability**.

## 1.3 Insights

Insights is **failure-pattern and gotcha learning**.

An Insight is about observable behavior or outcomes of agents/models/tools, for example:

- repeatedly claiming verification without corresponding evidence;
- ignoring a failed tool call and continuing as though it succeeded;
- repeated over-engineering or architecture churn;
- repeatedly violating scope or dropping requirements;
- recurring user corrections or frustration around the same behavior;
- using the wrong repository/subsystem;
- wasting context/tokens through avoidable repeated work;
- a model-specific or client-specific failure mode.

An Insight may generate a proposed guard, evaluator, routing adjustment, review warning, or remediation. That proposal remains **diagnostic/corrective evidence**, not user preference authority.

## 1.4 Cortex

Cortex is the durable admission/storage/retrieval substrate.

Cortex may store:

- agent memories;
- Taste preference records;
- Insight issue/reference records;
- other governed durable knowledge.

This does not make those semantic objects interchangeable.

The relation is:

```text
experience / user acts / agent trajectories
               |
               v
            Adapt
       +-------+-------+
       |               |
     Taste          Insights
       |               |
       +-------+-------+
               |
               v
       typed Cortex admission
               |
               v
     durable record + lifecycle
```

## 1.5 Same recommendation, different semantics

The same future-facing sentence can arise from different evidence and MUST retain that distinction.

Example intervention:

> `Do not expand a local fix into a repository-wide redesign.`

If the user explicitly states this as a recurring preference, it is Taste.

If Adapt observes five redesign loops that were reverted and detects repeated architecture churn, that is an Insight.

If both exist, delivery may combine their effect, but the system must preserve both evidence chains. The Insight cannot be relabeled as proof that the user said it, and the Taste record cannot cite model failure as its sole authority.

---

# 2. Product goals and non-goals

## 2.1 Taste goals

Taste SHOULD:

- make recurring user preferences available more cheaply than reading complete memory files;
- learn from explicit statements, corrections, choices, accepted/rejected work, and post-accept edits where the host exposes those signals;
- preserve the evidence that justified a preference;
- scope preferences narrowly enough to avoid globalizing local behavior;
- make conflicts, supersession, and provenance inspectable;
- allow users to inspect, narrow, deactivate, correct, export, and delete learned preferences;
- reduce repeated user correction without weakening authored policy.

## 2.2 Insights goals

Insights SHOULD:

- detect recurring model/agent failure modes from existing trajectories;
- surface exact evidence instead of vague self-criticism;
- separate one-off episodes from recurring issues;
- distinguish observed facts from inferred mechanisms;
- measure recurrence before and after mitigations;
- generate candidate guards/evaluators/remediations only after detector quality is measured;
- expose model/client/repository/tool/version applicability so obsolete gotchas can retire;
- help reduce repeated failures and user frustration.

## 2.3 Non-goals

Adapt is not:

- a generic memory store;
- an agent-authored memory manager;
- a repository source-of-truth system;
- a replacement for `AGENTS.md`, `CLAUDE.md`, organization policy, or current user instruction;
- a fine-tuning/retraining system;
- an automatic permission-grant system;
- a system that turns every user statement or edit into a permanent rule;
- a root-cause oracle;
- a generic observability backend;
- an excuse to persist every transcript detail;
- a second durable database beside Cortex.

---

# 3. Ownership and system architecture

## 3.1 Transcript and event substrate

Transcript normalization is infrastructure used by Adapt, not the identity of Adapt itself.

The native transcript/event substrate MUST preserve a language-neutral `TranscriptEventV1`-equivalent contract with:

- stable event/session identity;
- host/source identity;
- role/origin classification;
- byte/source spans where source bytes exist;
- event ordering;
- tool invocation/result linkage;
- timestamps where available;
- provider/model/client/agent-role metadata where available;
- parser receipts and source digests;
- typed unavailable/partial failures;
- explicit omission of private chain-of-thought;
- no silent conversion of missing input into empty-success.

External hosts such as Claude Code, Codex, Cline, Command Code, OpenCode, Qwen, Pi, Gemini, Grok, Roo, and Cursor may require transcript adapters.

CodeRight SHOULD NOT round-trip through a textual transcript when it already owns the structured runtime event. Its native host adapter should project the same canonical semantic facts directly.

Raw host transcripts remain source-host material until an explicit durable-admission decision. Native transcript/event code owns normalization, provenance, stable event identity, and typed parse failures.

Ledger may index an admitted or generated human-readable session document, but a Ledger projection is not transcript truth and MUST NOT replace the exact structured evidence used by Adapt.

## 3.2 Adapt shared behavioral-learning owner

Adapt owns:

- behavioral-learning ingestion from typed evidence;
- evidence canonicalization and provenance filtering;
- Taste candidate generation;
- Insights episode detection and issue formation;
- emergent failure-pattern discovery proposals;
- applicability/scope proposals;
- contradiction/supersession proposals;
- exact evidence binding;
- remediation/guard/evaluator proposals;
- learning audit and rollback metadata;
- delivery receipts and effectiveness semantics for Adapt-derived interventions;
- recurrence and mitigation-outcome interpretation.

Adapt does **not** own:

- the canonical durable database;
- raw high-volume CodeRight trace storage;
- generic experiment/dataset execution;
- direct ungoverned durable writes;
- repository truth;
- organization/current-instruction policy authority;
- final context budget policy for all Membrane content;
- Ledger document indexing;
- generic memory creation;
- application effect authorization;
- CodeRight routing/execution authority.

## 3.3 Native runtime destination

The Membrane-owned production implementation MUST be native Rust.

The canonical owners are:

```text
membrane-transcript
    canonical transcript/event normalization
    provenance
    source binding
    selected-transcript/user-act adaptation

membrane-adapt
    Taste
    Insights
    evidence interpretation
    authority
    scope
    issue formation
    remediation/evaluator proposals
    delivery/effectiveness semantics
```

No production Adapt operation may require Python, Node, Pi CLI, OpenCode CLI, or another interpreter-backed Membrane worker after native cutover. Release-excluded differential/evaluation tooling may remain where explicitly classified.

## 3.4 Model/proposal boundary

Models may assist with:

- extracting a possible preference from qualifying user evidence;
- compact canonical wording;
- proposing semantic duplicate/conflict groups;
- proposing clusters of semantically related failure episodes;
- discovering candidate failure patterns not covered by known detector families;
- proposing remediation or evaluator text.

Models MUST NOT decide authority by assertion.

All model-generated outputs are proposals. Deterministic code must bind them to admissible evidence and enforce provenance, scope, policy, lifecycle, and effect boundaries.

No model output can manufacture:

- a user source span;
- a user act or explicit transcript selection;
- a user preference;
- a permission grant;
- a verification receipt;
- a tool result;
- an evaluator result;
- a policy exception;
- a stronger scope than the evidence supports.

## 3.5 Three distinct admission decisions

Do not collapse these gates:

1. **Adapt proposal eligibility** — may this evidence form a Taste candidate, Insight episode, candidate issue, or candidate remediation?
2. **Cortex durable admission** — may the typed record enter governed durable knowledge with lifecycle/conflict/supersession/integrity semantics?
3. **Membrane context admission** — should a retrieved eligible record enter this task's context packet under scope, authority, freshness, sufficiency, budget, and representation policy?

Passing one grants nothing at the next.

## 3.6 Internal contract namespace

`TranscriptEventV1`, `ExecutionObservationV1`, `EvaluationOutcomeV1`, `FailureEpisodeV1`, `InsightIssueV1`, and Adapt proposal/effectiveness records are versioned internal/domain contracts. Their suffixes version their own schemas; they do not silently expand Membrane's five public V1 context protocol shapes.

A contract that must cross the CodeRight repository boundary MUST have an explicit CodeRight↔Membrane integration owner, schema/version/digest, compatibility policy, and fixture set. It need not become a generic MCP/public-client shape merely because CodeRight consumes it.

## 3.7 CodeRight structured execution observations

Adapt MUST be able to learn from structured harness facts that are stronger than transcript inference.

A canonical `ExecutionObservationV1`-equivalent record should support, when available:

```text
observation_id
session_id
task_id?
agent_id?
agent_role?
timestamp
model
provider
client
route_policy?
observation_kind
subject_id?
tool/call identity?
outcome?
exit/status code?
duration?
usage?
scope/repository?
artifact/evidence refs[]
provenance_receipt
```

Representative observation kinds include:

- model call start/end;
- route selection;
- tool call/result/failure;
- verification command/result;
- approval requested/granted/denied;
- edit/write and artifact digest;
- retry/cancel/timeout;
- task/goal transition;
- plan/replan event;
- subagent spawn/handoff;
- completion claim;
- retrieval/context receipt;
- Push reduction/restore;
- evaluator result;
- user steer/correction linkage.

CodeRight records **what happened**. Adapt decides whether the pattern has Taste or Insights meaning.

CodeRight MUST NOT pre-label an observation as a user preference merely because the event looks preference-like. It may run an already-approved Adapt detector/evaluator, but the detector identity and result must remain explicit.

## 3.8 Evaluation outcomes and the CodeRight seam

CodeRight owns generic harness evaluation machinery: traces, datasets, experiment execution, deterministic/code evaluators, LLM judges, score aggregation, model/prompt/routing comparisons, latency/cost measurements, and deployment/harness experiments.

Adapt owns behavioral interpretation of those outcomes.

A canonical `EvaluationOutcomeV1`-equivalent record should bind:

- evaluator identity and version;
- dataset/case identity and digest;
- experiment/run identity;
- trace/session/task identity;
- score/value/verdict;
- evaluator execution receipt;
- model/client/route surface;
- compared baseline where applicable;
- timestamp;
- provenance.

An Adapt Insight may propose a new evaluator. CodeRight executes it. The result flows back to Adapt to measure recurrence or mitigation effectiveness.

## 3.9 Closed behavioral-learning loop

```text
CodeRight / external host execution
        |
        +--> TranscriptEventV1
        +--> ExecutionObservationV1
        +--> EvaluationOutcomeV1
                    |
                    v
                  Adapt
          +---------+---------+
          |                   |
        Taste              Insights
          |                   |
          +---------+---------+
                    |
             governed proposals
                    |
                    v
                  Cortex
       durable admission/lifecycle/retrieval
                    |
           Membrane Pull/context delivery
                    |
                    v
                CodeRight
      routing / context / guards / evals
                    |
              measured outcome
                    |
                    +----------> Adapt
```

Ledger participates by providing document navigation and source-bound document evidence. It is not the generic telemetry store and its generated session documents are not a substitute for CodeRight's structured event stream.

# 4. Evidence and provenance model

## 4.1 Evidence classes

Adapt should distinguish at least:

### User-authoritative evidence

- explicit persistent preference (`always`, `from now on`, `never`, `I prefer`, equivalent meaning);
- direct correction with a reason;
- explicit review comment;
- explicit choice between named alternatives;
- explicit narrowing/expansion of scope;
- explicit acceptance/rejection where the host supplies a reliable event.

### User-behavioral evidence

- post-accept edit;
- repeated manual edit pattern;
- repeated rejection followed by a consistent replacement;
- repeated selection of the same alternative;
- silent acceptance.

Behavioral evidence may support a Taste hypothesis. It does not automatically carry the same authority as an explicit persistent instruction.

### Diagnostic evidence

- agent output;
- tool call/result;
- retry/rework pattern;
- verification command/result;
- revert or corrective patch;
- user frustration/correction;
- model/client metadata;
- context/token usage;
- observed trajectory timing and ordering.

Diagnostic evidence supports Insights. It cannot become user preference authority on its own.

### Context-only evidence

- repository text;
- generic memories;
- agent summaries;
- documentation;
- environment facts;
- tool output unrelated to a behavioral failure.

Context can help interpret evidence but cannot authorize Taste.

## 4.2 Signal-strength ladder

The historical design's signal ladder remains useful as a **weighting default**, not an authority substitute:

| Signal | Suggested starting strength |
|---|---:|
| explicit persistent instruction | 1.00 |
| direct correction + reason | 0.95 |
| explicit review correction | 0.90 |
| post-accept edit | 0.85 |
| repeated consistent edits | 0.85 |
| explicit named choice | 0.75 |
| repeated merged behavioral pattern | 0.65 |
| silent acceptance | 0.20 |
| agent summary | 0.10 for context only; **0 Taste authority** |
| repository/doc text | **0 personal Taste authority** |

Strength affects confidence and review policy. It does not permit a lower-authority source to masquerade as a higher-authority source.

## 4.3 Selected-transcript source-binding contract

Explicitly user-selected transcript evidence is the qualifying default workflow. It requires a
first-class candidate binding carrying exact source identity, digest, span, and review context. Automatic implicit
host signals are an optional, separately evaluated lane and are not required for release.

```text
TasteCandidateV1
  candidate_id
  host
  session_id
  transcript_id
  transcript_sha256
  parser_digest
  event_id
  byte_start / byte_end
  evidence_text_sha256
  act_kind
    explicit_preference
    correction
  scope / scope_dimensions
  needs_review = true
```

The object records what happened. It does not itself state the final preference.

### 4.3.1 Local caller-selected review contract

`adapt.user-taste-review.v1` is the explicit local review contract for caller-selected
transcripts. It requires the review payload to bind exactly to the pending manifest,
installation identity, and canonical-pool digest:

```text
contract_version = "adapt.user-taste-review.v1"
independent = true
installation_id == pending.installation_id
pending_manifest_sha256 == pending.manifest_sha256
canonical_pool_sha256 == pending.canonical_pool_sha256
decisions = exactly one decision for every pending record id
  verdict = "valid" | "invalid"
  reason = non-empty
validator_receipt_id = non-empty
validated_at = non-empty
issuer_id = ""
key_id = ""
signature_hex = ""
```

This is a local caller-selected human review boundary; it requires no login or
authentication. A signed `adapt.semantic-adjudication.v1` remains an optional
enterprise/import lane, not a prerequisite for local review.

## 4.4 No-authority-laundering invariant

The following chain is forbidden:

```text
agent says user prefers X
        -> memory stores that sentence
        -> Adapt reads memory
        -> Taste treats it as user authority
```

Likewise forbidden:

```text
Insight detects that X would have prevented a failure
        -> system stores remediation
        -> remediation becomes a user preference
```

A Taste record must be traceable to qualifying user evidence.

---

# 5. Taste canonical contract

## 5.1 Semantic definition

A Taste record represents a **user-backed preference or behavioral constraint** that should influence future agent behavior when its applicability conditions match.

It is not a generic fact record.

## 5.2 Canonical Taste record classes

The target semantic classes are:

- **standing preference** — broad, durable behavioral preference;
- **scoped preference** — applies only to a repository, package, path, language, framework, task family, artifact type, model/client, or other explicit applicability condition;
- **operational preference/playbook** — preferred procedure under defined conditions;
- **explicit behavioral decision** — a user-authoritative decision that governs agent behavior within a declared scope.

The current implementation includes `locked_decision`, `episodic_fact`, and `unclassified`. The canonical rule is:

- a `locked_decision` is Taste only when it is behavior/choice authority rather than repository factual state;
- `episodic_fact` is **not Taste** and must not be presented or delivered as a preference. It should be rejected from the Taste semantic lane or routed to the appropriate non-Taste Cortex proposal class;
- `unclassified` is migration/review state, not a durable semantic category.

This distinction is required to stop Taste from slowly becoming a generic memory bucket.

## 5.3 Controlled preference categories

The current eight-category taxonomy is a useful closed starting contract:

- `workflow`
- `verification`
- `safety`
- `architecture`
- `tooling`
- `code-style`
- `documentation`
- `model-routing`

Adding a category is an architecture/schema change. Unknown categories must not silently map to a generic bucket that becomes active.

## 5.4 Scope and applicability

Taste applicability must be explicit and fail closed.

Minimum supported dimensions should cover:

- user / organization;
- repository;
- path prefix;
- package/module;
- language;
- framework;
- task family;
- artifact/output type;
- model;
- client/host;
- environment/platform;
- risk class;
- branch/build/deploy context where justified.

Unknown or malformed narrowing dimensions MUST reject/quarantine the candidate. Dropping an invalid narrowing key and then treating the preference as broad is forbidden.

## 5.5 Precedence

A safe starting precedence is:

1. current explicit user instruction;
2. safety / organization policy;
3. explicit repository policy and committed agent instructions;
4. explicit scoped user preference;
5. inferred scoped user preference;
6. explicit global user preference;
7. inferred global user preference;
8. trusted imported preference;
9. provisional candidate.

A lower tier cannot repeal a higher tier. When two applicable items conflict at the same tier, the system must surface conflict or apply a deterministic explicit rule; it must not let retrieval order decide.

## 5.6 Counterfactual representation

Where the evidence is a correction/edit/rejection, Taste should preserve the alternative that was rejected when safe and useful.

Example:

```text
preferred: focused local change
avoid: repository-wide abstraction rewrite
reason: repeated user correction after local-fix requests
```

This supports better conditioning than a vague imperative alone and enables future effectiveness evaluation.

## 5.7 Lifecycle

Canonical lifecycle states:

- `candidate`
- `active`
- `disputed`
- `deprecated`
- `superseded`
- `retired`

Lifecycle transitions must be explicit and receipted.

A preference should be re-evaluated when:

- the user contradicts it;
- a stronger scoped preference appears;
- repository/org policy changes;
- its applicability environment changes materially;
- the user edits/deletes it;
- an imported preference is overridden locally.

Do not implement opaque time decay as a substitute for semantic re-evaluation. Time can trigger review; it should not silently rewrite authority.

## 5.8 Delivery

Taste delivery uses two layers:

1. a **bounded always-on core** for a very small set of broad, active standing preferences;
2. **cheap scoped preference selection** for the rest.

This is not generic memory retrieval. The selection index is preference-specific and should answer applicability cheaply from structured scope before semantic search is considered.

Delivery MUST filter:

- inactive/disputed/retired items;
- machine/client-specific items that do not apply;
- nonmatching scope dimensions;
- conflicts overridden by higher-priority policy;
- non-Taste facts.

Every delivered preference should have a delivery receipt linking the selected record and applicability decision.

---

# 6. Insights canonical contract

## 6.1 Semantic definition

An Insight represents a **supported failure pattern, gotcha, waste pattern, or recurring behavioral defect** observed in agent/model/tool trajectories.

It answers:

> What has the system repeatedly done wrong, under what conditions, with what evidence, and did the problem recur after mitigation?

## 6.2 Episode vs issue

A single detected episode is not automatically a durable issue.

Use two levels:

```text
FailureEpisodeV1
  detector/family
  exact evidence spans
  session/model/client/tool metadata
  severity
  observed outcome
  honesty limit

InsightIssueV1
  issue_id
  family
  canonical description
  applicability dimensions
  episode ids / evidence digests
  recurrence count
  first_seen / last_seen
  confidence / evidence quality
  state
  candidate mechanism(s)
  candidate remediation(s)
  mitigation links
  recurrence-after-mitigation
```

## 6.3 Issue lifecycle

Recommended lifecycle:

- `observed`
- `recurring`
- `confirmed`
- `mitigation_proposed`
- `mitigated`
- `reopened`
- `obsolete`
- `dismissed`

State transitions must preserve the underlying episodes. A model upgrade, client change, tool fix, or repository migration may obsolete an Insight without deleting its history.

## 6.4 Current detector-family contract

The native Insights implementation now contains the original deterministic families plus the high-cost behavioral classes that were previously roadmap items, including:

- verification failure and false-completion classes;
- repeated asks/corrections;
- frustration/swearing signals;
- ignored tool failure;
- degraded-provider-as-success;
- false-not-found;
- unproductive broad searching;
- wrong repository/subsystem;
- stale terminology;
- silent scope narrowing;
- omitted requirements;
- unaccepted plan change;
- tests that cannot fail;
- cross-agent repeats;
- guard firings;
- overengineering;
- architecture churn;
- repeated redesign;
- planning instead of executing;
- unnecessary abstraction/dependency;
- scope expansion;
- verification theatre;
- instruction noncompliance;
- model/client/tool-specific gotchas.

The family list is not permanent product ontology. New families require an operational definition, evidence contract, hard negatives, benchmark coverage, and lifecycle/applicability rules.

## 6.5 Known-family detection and emergent discovery are separate lanes

Adapt Insights MUST support two complementary lanes:

### Known-family lane

Deterministic or otherwise qualified detectors operate against known failure families and emit evidence-bound `FailureEpisodeV1` records.

This lane is optimized for precision and reproducibility.

### Emergent-discovery lane

Adapt may analyze uncategorized/low-confidence trajectories or issue summaries to discover recurring patterns that do not map cleanly to a current family.

Braintrust Topics is the relevant product reference: it converts traces into facet summaries and clusters recurring patterns so failures can emerge without a pre-existing category. Adapt should absorb the **discovery pattern**, not Braintrust's authority model.

An emergent cluster MUST remain a `CandidatePattern`-equivalent proposal until:

1. source episodes are exact and inspectable;
2. the pattern is stable enough to describe operationally;
3. a human/reviewer or deterministic validation accepts the family boundary;
4. positive and adversarial negative cases are created;
5. a dev split is used for detector/evaluator tuning;
6. a frozen held-out set meets the promotion gate;
7. activation records the detector/evaluator version and rollback path.

A clustering model may suggest membership. It may not create durable issue authority merely by producing a cluster.

## 6.6 Evidence honesty

Insights may say:

- `the agent claimed X after tool Y failed`;
- `the same category recurred in N sessions`;
- `this model/client combination accounts for M of N episodes`;
- `the user corrected this behavior repeatedly`.

Insights must not say a root cause is proven unless the evidence supports causal attribution.

Every card/issue should expose an honesty limit appropriate to its detector or inference method.

## 6.7 Remediation is a separate object

A detected Insight may generate:

- a guard proposal;
- an evaluator proposal;
- a routing recommendation;
- a review warning;
- a workflow change proposal;
- a candidate Taste prompt only if separate qualifying user evidence exists.

The remediation object is not the issue itself and carries no user-preference authority merely because it was generated from an Insight.

## 6.8 Mitigation outcome

The north-star Insights metric is **recurrence reduction**, not number of findings.

For every automated or manual mitigation that can be linked to an Insight issue, record:

- baseline recurrence rate;
- mitigation start/version;
- comparable post-mitigation exposure;
- recurrence after mitigation;
- regressions/reopen events;
- false-positive/dismissal rate.

## 6.9 Intervention attribution — the mutation-eligibility gate

Between a confirmed `InsightIssueV1` and an actionable remediation proposal whose
`intervention_target` names a mutable instruction surface (skill/procedure, system prompt, tool
description, documentation policy), Adapt MUST answer a question §6.6 and §6.7 leave open:

> **Why do we believe changing this particular surface would have prevented the observed
> failures?**

The answer is an explicit attribution record, not an implicit assumption inside the proposal. Its
semantic requirements:

- **Counterfactual preventability.** An instruction-surface proposal is eligible only when a
  competent agent following the proposed rule would have avoided the scored failures, and the rule
  is `missing`, `wrong`, or `underspecified` on the current surface content (bound by digest). A
  claim of preventability without episode-level evidence is `unknown`, not `supported`.
- **Already-correct refusal.** When the current surface already demanded the correct behavior, that
  surface is not the intervention target. The failure attributes elsewhere — model behavior or
  variance, routing, retrieval/reduction, tooling, product, or evaluator error. A guard or
  evaluator may still be proposed; those are different targets with their own attributions.
- **No redundant restatement.** A proposal whose only change restates or hedges guidance the
  surface already carries is ineligible. The smallest general change that alters the surface's
  behavioral contract is the only admissible mutation shape.
- **Alternative-cause accounting.** Attribution must name the plausible non-target causes it
  considered (model variance, routing failure, infrastructure, product, tool implementation,
  evaluator error) and why the evidence discriminates against them — or state
  `insufficient_evidence` and remain ineligible.
- **Independent support.** Eligibility requires recurrence across independent sessions, not
  repetition inside one trajectory. Frequency × severity prioritizes; it does not by itself
  establish preventability.

For `skill_or_procedure` targets, mechanical activation evidence (host capability H4) discriminates
the surface deterministically before any semantic judgment:

```text
asset never discovered                  → registry / discovery, not the asset text
discovered, trigger never matched       → trigger/description surface
selected, not loaded or not in context  → context_retrieval / context_reduction
in context, rule present, not followed  → model_behavior_policy, or model variance
                                          (recurrence across sessions required to
                                           exclude variance)
rule followed, failure still occurred   → instruction_state = already_correct;
                                          the asset text is the wrong target
```

The host emits only the mechanical stages (discovered, trigger evaluation, selection, load,
in-context turns). Whether a rule was *relevant* to the failure and whether it was *followed* are
Adapt semantic assessments and never host-emitted facts (invariant P1).

Evaluator outcomes counted as attribution support use a three-valued applicability domain:
`applicable | not_applicable | insufficient_evidence`. An `insufficient_evidence` outcome is
removed from the applicable denominator; it never becomes a success, a failure, or a zero.

Attribution is proposal-class under §3.4: a model may draft it; deterministic code binds it to
episode evidence, the current surface digest, and the eligibility gates above. Attribution grants
no authority — a mutation-eligible verdict feeds variant generation (host capability H7) and every
existing proposal, review, precision, and admission gate still applies.

---

# 7. Shared admission, integrity, and Cortex contracts

## 7.1 One durable admission boundary

All durable Adapt outputs must cross one typed Cortex admission contract.

Taste and Insights may have different authority/influence classes, but neither may create a parallel durable truth store.

The current `insight_persistence.py` path proves reference-only persistence is possible. The canonical target is to make the seam explicit and consistent rather than maintaining ambiguous parallel persistence behavior.

## 7.2 Influence classes

At minimum:

- active user-authoritative Taste may be eligible for behavioral/directive influence subject to policy and scope;
- inferred/provisional Taste should have weaker influence or review requirements;
- Insight records are `reference`/diagnostic by default;
- an Insight cannot itself grant permission or establish user intent;
- remediation/guard artifacts have their own explicit authority and effect boundary.

## 7.3 Semantic sealing

The current implementation binds candidate payload hashes and held-out semantic-validation receipts but does not fully seal every apply-semantic field.

The target should separate immutable semantics from mutable lifecycle state.

### Immutable semantic payload

Hash an immutable canonical payload containing every field that changes meaning/applicability, including:

- semantic record kind;
- category/family;
- canonical text/description;
- scope and scope dimensions;
- authority class;
- influence class;
- machine/client/model applicability where semantic;
- source/evidence digests;
- canonical pool/version binding;
- admission policy version;
- semantic validator receipt id/digest;
- redaction/provenance contract version.

### Mutable state envelope

Lifecycle state, verification counts, observations, last-seen timestamps, and similar mutable counters should change only through explicit transition/observation events with receipts.

Do not "solve" mutability by leaving meaning-bearing fields outside the seal.

## 7.4 Deterministic IDs and idempotency

Stable semantic identity should derive from canonical semantics and declared scope, not incidental processing order.

Batch application must be:

- idempotent;
- atomic at the Cortex boundary;
- retry safe;
- receipt producing;
- installation/source aware;
- resistant to duplicate writes after partial failures.

## 7.5 Duplicate and contradiction handling

Exact duplicate detection may be deterministic.

Semantic grouping may use models as proposers, but model-decided group membership must not silently become authoritative. The system needs one of:

- deterministic measurable grouping for supported classes;
- a reviewed semantic-merge receipt;
- a conservative abstain path when grouping is uncertain.

Conflicts must preserve history. Do not delete the losing record merely because a later one wins precedence.

---

# 8. Taste and Insights interaction rules

## 8.1 Taste may influence prevention

An applicable Taste preference can help prevent a known Insight failure.

Example:

```text
Taste: user prefers local changes over broad redesigns.
Insight: model repeatedly expands local fixes into architecture rewrites.
```

The resulting delivery packet may include the Taste preference and a diagnostic guard derived from the Insight.

## 8.2 Insights cannot authorize Taste

The system must never reason:

```text
agent failed because it over-engineered
=> user must prefer minimalism
=> create standing preference "never over-engineer"
```

That conclusion requires user-backed evidence to become Taste.

## 8.3 User complaint may create both evidence types

A user message such as:

> `You keep rewriting the whole architecture when I ask for a small fix. Stop doing that.`

can simultaneously provide:

- explicit Taste evidence (`stop expanding local fixes`);
- diagnostic Insights evidence (`repeated architecture churn / scope expansion`).

The system should create two linked proposals with different semantic types and authority.

---

# 9. Runtime relationship

Adapt runtime migration, packaging, process policy, deletion gates, Blueprint/CodeRight seams, and native-only release sequencing are owned exclusively by the Membrane native-Rust migration specification.

This document contributes Adapt semantics, internal/domain contracts, host-integration requirements, and acceptance evidence.

The native Adapt and transcript owners exist and are the target authority. Legacy Python Adapt remains only where explicitly classified as release-excluded differential/oracle tooling and MUST NOT be used as evidence that a native defect still exists unless the same defect is shown on the production Rust path.

The historical Python `D--Claude` root-scope literal is therefore tracked as a **legacy-oracle/parity/deletion concern**, not automatically as a native production defect. Native root/global selection must be judged from the current Rust delivery/core implementation.

Runtime completion is never inferred from coexistence. Installed execution and process-tree qualification decide the native-only result.

# 10. Current implementation truth — refresh-required snapshot

This section is **descriptive, not normative**. Before implementation begins, an agent MUST re-derive it from current `main` and update the status table in the same change if the facts have moved. Do not freeze the entire product canon to a moving commit header.

## 10.1 Native platform already demonstrated

Current repository evidence demonstrates these mechanisms exist in the native target:

- `membrane-transcript` native owner with multi-host discovery/parsing;
- `membrane-adapt` native owner;
- Taste/Insights semantic separation;
- three distinct admission types/boundaries;
- immutable semantic/applicability sealing plus receipted mutable state;
- explicitly selected transcript/user-act evidence contract with exact source binding;
- structured scope selection;
- native Taste delivery/effectiveness receipts;
- signed Taste export/import;
- `FailureEpisodeV1`/`InsightIssueV1`-style native issue formation;
- expanded high-cost behavioral detector families;
- remediation proposal types with effect separation;
- precision gating before actionable remediation;
- mitigation/recurrence tracking;
- adaptive A/B/effectiveness/retirement measurement structures;
- persistent-context cost accounting with measured/inferred/unattributed separation;
- root CI capable of running the locked Rust workspace.

Presence does not equal production qualification. Each mechanism still inherits the Membrane production-path/evidence gate.

## 10.2 Taste remaining product gaps

The important remaining Taste work is no longer "port the Python pipeline."

Pending work is:

- prove host-specific accept/reject/post-accept-edit/named-choice signals on real CodeRight and other supported hosts;
- complete end-user inspect/edit/narrow/delete/review UX;
- accumulate real effectiveness evidence showing fewer repeated corrections without correctness/security regression;
- validate bounded always-on core selection on the native path;
- keep legacy Python-only assumptions such as `D--Claude` outside production and delete or bound them as oracle evidence;
- qualify signed import/export and organization packaging against real origin/precedence scenarios;
- ensure the CodeRight structured-observation seam cannot launder runtime events into Taste authority.

## 10.3 Multi-source transcript/event status

The native transcript owner supports direct discovery/parsing across the supported external-host set. Before implementation, regenerate the host capability matrix from the native source and tests rather than copying a stale table.

Parser coverage alone does not establish user-act authority. Explicit transcript selection,
exact source hash/rebinding, and required review establish authority for the selected-transcript
workflow; assistant, tool, model, and repository text remain non-authoritative. Automatic implicit
UI/host signals remain optional and separately evaluated.

For CodeRight, structured direct emission is preferred to file scraping.

## 10.4 Insights already landed

Current native evidence includes:

- deterministic episode detectors;
- high-cost behavioral families such as overengineering, architecture churn, scope expansion, verification theatre, false completion, and instruction noncompliance;
- exact evidence linkage;
- longitudinal issue/recurrence semantics;
- remediation proposals;
- recurrence-after-mitigation logic;
- a sealed portable synthetic conformance corpus;
- a Rust benchmark gate over that corpus;
- model/client surface measurement structures.

## 10.5 Insights remaining quality gaps

The most important remaining gaps are:

1. **Real held-out validation.** The synthetic portable corpus is a contract/conformance suite, not empirical proof of detector precision across messy real sessions.
2. **Emergent failure discovery.** Known-family detectors do not discover failure modes nobody predeclared.
3. **Confirmed issue → regression/evaluator loop.** The proposal types exist; the complete reviewed dataset/evaluator/CodeRight execution/recurrence loop must be operationalized.
4. **Human review queue.** Operators need a first-class confirm/dismiss/split/merge/remediation workflow.
5. **Structured harness evidence.** CodeRight should supply direct execution observations so Insights does not infer mechanical facts from prose when the harness already knows them.
6. **Production effectiveness.** Recurrence reduction must be demonstrated on comparable exposure after mitigation.
7. **Retrieval/context learning.** Pull/Ledger/Push outcome signals should become Insights evidence for recurring retrieval and context failures without granting Adapt authority over those subsystems.

## 10.6 Synthetic vs real evidence

The portable synthetic benchmark remains mandatory because it is deterministic, portable, adversarial, and reproducible.

It is **not** sufficient for claims such as "detector precision is 95% in production."

Production-quality claims require a separately governed real-world corpus with:

- consent/redaction policy;
- host/model/client/task segmentation;
- frozen labelled examples;
- positives and hard negatives;
- no training/tuning leakage into the final held-out split;
- reviewer agreement or adjudication;
- confidence intervals;
- reproducible detector/evaluator version binding.

## 10.7 Canon drift is a defect

If the implementation-status section says a native feature is absent when current code contains it, or claims a feature is shipped when the production path cannot reach it, the document is defective.

Implementation agents MUST fix status drift before using §10 as work-dispatch authority.

# 11. Evaluation and quality gates

## 11.1 Taste evaluation

Taste may be evaluated separately for explicit selected-transcript evidence and optional implicit
host signals. The explicit workflow does not require a held-out corpus; held-out
evaluation remains evidence for optional automatic extraction and broader quality claims.

Minimum dimensions:

- extraction precision;
- extraction recall;
- authority false-positive rate;
- scope/applicability precision;
- contradiction/supersession correctness;
- preference adherence on future tasks;
- repeated-correction reduction;
- user edit/delete correctness;
- token/context overhead;
- correctness/security regression rate;
- imported/local precedence correctness.

Existing Adapt quality work has used high precision as the primary goal (for example precision around `>= 0.95` and recall around `>= 0.75` where defined). Canonical policy is to remain **precision-first** for durable preference admission; thresholds may only become stricter without an architecture decision, and any relaxation requires evidence and review.

## 11.2 Insights evaluation

Insights has two distinct evaluation layers.

### A. Portable conformance corpus

The checked-in synthetic corpus proves:

- detector contract behavior;
- explicit positive and negative cases;
- negation handling;
- quoted/context-carried text handling;
- tool-result/hypothetical traps;
- deterministic portability;
- schema/seal integrity.

It MUST remain fast and deterministic.

### B. Real held-out corpus

Before production precision claims or broad automated remediation, evaluate against sanitized/consented real trajectories.

Methodology:

1. define the target population and minimum detectable effect;
2. build a labelled pool across hosts/models/clients/task classes;
3. split into **train/authoring**, **dev/tuning**, and **held-out test**;
4. tune regexes, thresholds, clustering/evaluator prompts, and weights only on dev;
5. freeze implementation and thresholds;
6. touch held-out test only for the promotion decision;
7. report paired bootstrap confidence intervals or another predeclared interval method;
8. preserve family-level confusion matrices and dismissal rates.

A small corpus may be adequate for contract regressions but not for fine-grained rank/precision claims. Sample size must be justified by the effect the gate is expected to detect.

Every promoted detector/discovery/evaluator family should measure where applicable:

- precision;
- recall;
- false positives from negation/quotation/tool-carried text;
- cross-session recurrence correctness;
- severity calibration;
- user/reviewer dismissal rate;
- model/client/task drift.

## 11.3 Outcome metrics

Taste north-star:

> How often did an applicable admitted preference prevent a repeated correction while preserving task correctness and policy compliance?

Insights north-star:

> How much did confirmed recurring failure frequency fall after a linked mitigation, adjusted for comparable exposure?

Do not optimize for number of learned records or number of Insight cards.

## 11.4 Shadow/counterfactual evaluation

Where feasible, evaluate:

- baseline with no Adapt intervention;
- Taste selection only;
- same preferences statically injected (to separate learning value from delivery value);
- competitor/reference preference package where legally/technically possible;
- Insight guard/evaluator enabled vs disabled;
- native Rust vs frozen Python oracle during migration.

---

# 12. Product UX contracts

## 12.1 User-facing separation

The UI/CLI must never merge Taste and Insights under a generic `Memory` heading.

Recommended surfaces:

```text
Adapt
  Taste
    learned preferences
    pending review
    conflicts
    scope
    import/export

  Insights
    recurring issues
    recent episodes
    mitigations
    recurrence
    token/context waste
```

## 12.2 Taste controls

Users should be able to:

- inspect the exact evidence behind a preference;
- see whether it was explicit or inferred;
- edit wording without silently changing evidence/authority;
- narrow scope;
- deactivate/retire;
- supersede with a new explicit preference;
- delete where policy permits;
- export/import with evidence and origin metadata intact.

## 12.3 Insights controls

Users/operators should be able to:

- inspect exact episodes;
- dismiss false positives;
- merge/split issues with receipts;
- view model/client/repo/tool applicability;
- attach or approve mitigations;
- see whether the issue recurred;
- mark obsolete after model/tool/version changes.

## 12.4 Receipts

Adapt should surface compact receipts such as:

- `Taste learned from 3 user corrections; applies to repo X / Python files.`
- `Insight recurring: verification claim without evidence, 5 episodes across model Y.`
- `Mitigation active; no recurrence in 18 comparable tasks.`

Receipts must not imply certainty that the evidence does not support.

---

# 13. Always-on context and token-efficiency contract

Insights should attribute recurring always-on context cost where evidence permits.

Provider-reported billed totals remain authoritative. Inferred splits must be labelled as inferred.

The canonical method is:

1. resolve persistent prefix sources visible for the session/host;
2. record byte sizes and digests;
3. reconcile an inferred split against the measured prefix/overhead total;
4. leave unresolved remainder explicitly unattributed;
5. multiply persistent source share by billed turns to estimate recurring impact;
6. mark analysis-time file-state uncertainty when source files changed after the session.

Initial findings:

- `apparently_unused_always_on_context`
- `oversized_instruction_file`
- `mcp_tool_definitions_dominate`
- `always_on_prefix_dominates`
- `memory_recall_never_used` only when recall observability is sufficient

This feature belongs to Insights. It does not make Adapt a memory system merely because memory/index files may be one possible source of persistent prompt cost.

---

# 14. Ranked implementation plan

The native migration sequence is not repeated here. This is the **remaining Adapt product roadmap**.

## P0 — truth, evidence, and CodeRight seam

### P0.1 Reconcile canon with current implementation

- re-derive §10 from current `main`;
- remove dead/missing research-plan references;
- update current-product terminology from historical Guide to **Ledger**;
- ensure README/agent rules/generated truth project the same semantics;
- keep implementation baseline metadata separate from normative product semantics.

### P0.2 Enforce the Membrane production-path evidence invariant

Adapt inherits the cross-cutting doctrine:

> A capability is not landed merely because code exists. Completion requires proof that the production path executes it and frozen evidence that it meets or improves the acceptance baseline.

For Adapt this means:

- native CLI/runtime path reaches the mechanism;
- test/trace/receipt proves reachability;
- representative benchmark/outcome evidence exists;
- rollback/deactivation is defined where behavior changes;
- a parallel legacy path cannot silently remain selectable after cutover.

### P0.3 Build the real held-out Insights corpus

Keep the portable synthetic corpus as conformance.

Add a governed real-world corpus with:

- explicit corpus charter;
- redaction/consent rules;
- host/model/client/task stratification;
- train/dev/test separation;
- paired evaluation where possible;
- confidence intervals;
- exact detector/evaluator versioning;
- no final-test tuning.

Automatic remediation remains blocked for families that do not meet the real-corpus gate unless the effect is independently deterministic and verified at execution time.

### P0.4 Add the CodeRight typed evidence seam

CodeRight should emit:

- transcript/event facts;
- explicitly selected transcript/user-act evidence with exact source binding;
- execution observations;
- evaluation outcomes;
- Membrane context/retrieval receipts.

Adapt consumes these through typed native APIs/contracts, not by reparsing CodeRight prose.

Mechanical facts known to the harness SHOULD NOT be re-inferred from text.

### P0.5 Validate native bounded-core/root semantics

- prove current native global/root standing-preference selection;
- ensure malformed or unknown narrowing metadata cannot broaden scope;
- confirm inactive/disputed/retired records never enter the core;
- classify legacy Python `D--Claude` behavior as oracle/deletion evidence only unless reproduced in native code;
- add production-path coverage for the native core.

## P1 — operationalize richer learning

### P1.1 Braintrust-style emergent failure discovery

Add a proposal-only discovery lane over uncategorized/low-confidence trajectories:

```text
events/observations
    -> bounded summaries/features
    -> semantic clustering
    -> CandidatePattern
    -> reviewer validation
    -> detector/evaluator authoring
    -> dev tuning
    -> held-out promotion
```

Do not let a model-created cluster become a durable issue or guard without this promotion path.

### P1.2 Insights review queue

Borrow the useful human-workflow idea from Langfuse annotation queues without adopting its generic score model as Adapt ontology.

The review surface should support:

- confirm/dismiss episode;
- confirm/dismiss issue;
- split/merge with receipts;
- inspect exact evidence;
- review candidate mechanism;
- approve/reject remediation;
- approve/reject regression case;
- mark obsolete;
- review recurrence after mitigation.

### P1.3 Confirmed Insight → regression case

A confirmed issue may generate a privacy-safe `RegressionCaseProposal`.

Promotion should preserve:

- source issue/evidence references;
- redaction/synthesis provenance;
- expected failure/non-failure behavior;
- case family;
- evaluator needs;
- review receipt;
- immutable case digest.

Do not blindly copy private transcript text into permanent eval datasets.

### P1.4 Versioned evaluator lifecycle

Adapt may define behavioral evaluator proposals; CodeRight owns generic evaluator execution.

A promoted evaluator should bind:

- evaluator id/version;
- source Insight issue/family;
- code/prompt digest;
- dataset/case contract;
- score/verdict semantics;
- optimization direction;
- applicable models/clients/tasks;
- execution receipt requirements;
- retirement/supersession state.

Phoenix's dataset-evaluator model is a useful reference: attach reusable evaluators to stable test cases and trace evaluator execution.

### P1.5 Close the improvement loop

Operationalize:

```text
production failure
 -> Insight issue
 -> reviewed regression case/evaluator
 -> CodeRight baseline experiment
 -> remediation/harness/prompt/routing change
 -> repeat experiment
 -> deploy if gate passes
 -> online recurrence measurement
 -> close or reopen Insight
```

This follows the useful Braintrust/Langfuse/Phoenix loop while preserving Adapt authority and Cortex admission.

### P1.6 Complete host user-act integration

For CodeRight and every supported host that can expose the signals, qualify:

- accept;
- reject;
- post-accept edit;
- repeated edit;
- named choice;
- explicit correction.

Unavailable signals must remain explicitly unavailable; do not infer user acts from assistant,
tool, model, or repository text.

### P1.7 Deterministic completion/evidence integrity

Absorb CHIRON/HORKOS lessons:

- prefer deterministic capture when the event is mechanically knowable;
- completion claims should be cross-referenced against real execution/artifact/verification receipts;
- zero-LLM live guards are preferable for exact write/verification facts;
- every false-positive fix becomes a benchmark case.

CodeRight may enforce approved completion guards live. Adapt records recurrence/effectiveness and learns broader patterns; it does not need to be in the blocking hot path for every exact receipt check.

### P1.8 Retrieval/context failure learning

Consume typed Pull/Ledger/Push outcome signals such as:

- repeated irrelevant Ledger retrieval;
- required evidence omitted then searched manually;
- stale/missing resolver failures;
- context packet insufficient then corrected;
- Push reduction restored because protected evidence was lost;
- persistent context source repeatedly selected but apparently unused.

Adapt may propose ranking, alias, chunking, query, or reduction changes. The owning subsystem must evaluate and promote them against its frozen corpus.

## P2 — measured adaptive behavior

### P2.1 Taste effectiveness in real workloads

Measure baseline vs Taste-enabled comparable tasks for:

- repeated-correction rate;
- correctness;
- policy compliance;
- token/context overhead;
- user overrides;
- per-model/client effects.

### P2.2 Insight mitigation effectiveness

Track exposure-adjusted recurrence before and after mitigation, not raw counts alone.

### P2.3 Model/client-specific routing evidence

Adapt may identify recurring surface-specific gotchas. CodeRight's routing/eval plane decides whether a routing change is beneficial through controlled experiments.

### P2.4 Retirement and drift

Use meaningful model/client/tool/repository changes plus measured effectiveness to propose retirement or revalidation. Never silently decay authority with time.

### P2.5 Organization aggregates

Only after privacy and origin contracts are proven, support aggregate failure/preference effectiveness without leaking transcript text or individual user identity.

---

## Roadmap exclusion rule

Do not add a feature to Adapt merely because it appears in Braintrust, Langfuse, Phoenix, CHIRON, HORKOS, or another comparator.

A feature belongs in Adapt only when it is specifically about **behavioral learning from experience**.

Generic tracing, dataset storage, experiment execution, model comparison, and scorer infrastructure belong in CodeRight.

Document indexing belongs in Ledger.

Durable admission/lifecycle/storage/retrieval belongs in Cortex.

# 15. Documentation and terminology firewall

## 15.1 Required language

Every primary Adapt overview should state all four of these ideas:

1. Adapt is governed behavioral learning.
2. Taste is user-preference learning.
3. Insights is failure/gotcha learning.
4. Cortex is the durable admission/storage/retrieval substrate and is separate from Adapt.

## 15.2 Forbidden current-product phrasing

Do not use, except when quoting/criticizing historical material:

- `Adapt memory system`
- `Adapt memory substrate`
- `Adapt memory control plane`
- `Taste memory`
- `coding-taste memory`
- `continuous coding-taste memory`
- `Insights memory`
- `Adapt memories`
- `admitted memory` when the actual object is an Adapt preference/Insight

Prefer:

- `Adapt behavioral-learning subsystem`
- `Taste preference`
- `learned preference`
- `Taste record`
- `Insight episode`
- `Insight issue`
- `failure/gotcha finding`
- `Adapt-derived intervention`
- `Cortex record` when discussing storage without changing semantic type

## 15.3 Allowed use of memory

`memory` is valid when referring to:

- Cortex's memory/durable knowledge capabilities;
- agent/host memory features;
- external competitor memory systems;
- memory files as one source of context/token overhead;
- historical design text explicitly labelled historical.

## 15.4 Canonical README copy

Use wording equivalent to:

> Adapt learns how agents should improve from real work. Taste captures user-backed preferences and recurring choices. Insights detects repeated agent/model failures, gotchas, and waste. Learned outputs are evidence-bound, scoped, reviewable, and admitted through Cortex; Adapt is not generic agent memory.

## 15.5 Historical documents

Historical specs may retain old terminology only if clearly marked:

> Historical terminology: this document predates the canonical Adapt ontology. `memory` references below do not define current Adapt product semantics.

Do not silently rewrite historical evidence in a way that changes what was originally decided; add supersession markers instead.

---

# 16. Research-derived lessons (non-normative)

Research is evidence for improvement, not product authority. External products change; reverify their current behavior before relying on specific shipping claims.

## 16.1 Taste comparators

### Command Code Taste

Closest direct shipping comparator for the Taste product surface.

Useful mechanisms:

- explicit and behavioral user signals;
- project/global/distributed Taste packaging;
- inspect/list/lint/push/pull/compose UX;
- immediate conditioning.

Adapt should absorb signal breadth and distribution UX without allowing inferred preferences to outrank authored/current policy.

### CHIRON

Verified public repository lessons:

- deterministic gates;
- zero LLM in the capture path;
- reviewable correction-to-rule records;
- contradictions surfaced rather than silently resolved;
- reproducible local benchmark suite.

The public README currently reports a 35/35 benchmark suite. This is vendor/self-reported evidence, but the benchmarks are shipped and rerunnable.

Adapt's lesson is not "copy CHIRON's ontology." It is: when a fact can be captured deterministically, do not insert a model into the authority path merely for convenience.

### Vorpl / Gensyn / adjacent systems

Useful primarily for behavioral-signal and effectiveness ideas. They do not define Adapt's authority model.

## 16.2 Insights comparators

### Braintrust Topics / eval improvement loop

Official Braintrust material describes:

- trace preprocessing;
- LLM-derived facets including Issues;
- recurring-topic clustering;
- classification of new traces into discovered topics;
- production finding → dataset → baseline → fix → eval → deploy → online monitoring.

Adapt should absorb:

- unknown failure-pattern discovery;
- recurring pattern review;
- conversion of confirmed production failures into governed regression cases.

Adapt must retain stronger proposal/evidence/authority separation than a generic observability topic classifier.

### Langfuse

Official Langfuse evaluation material provides useful workflow patterns:

- traces/observations/sessions;
- generic scores;
- online and offline evaluation;
- production examples promoted into datasets;
- experiments comparing prompt/model/code variants;
- deterministic or LLM evaluators;
- annotation queues for structured human review.

The generic tracing/score/dataset/experiment platform belongs in CodeRight. Adapt should absorb the **behavioral review workflow** and consume typed evaluation outcomes.

### Arize Phoenix

Official Phoenix material provides useful evaluator/experiment patterns:

- datasets as stable test suites;
- reusable dataset evaluators;
- deterministic code evaluators and LLM evaluators;
- evaluator execution traces;
- experiments over prompts/models/application changes;
- failure-mode review followed by evaluator creation.

Again, generic execution belongs in CodeRight. Adapt owns the semantic link from an Insight issue to a proposed evaluator and the recurrence/effectiveness interpretation afterward.

### HORKOS

Verified public repository lessons:

- live write ledger;
- completion claims cross-checked against real writes/artifacts;
- deterministic/zero-LLM audit path;
- explicit unknown/unverified behavior;
- offline-rerunnable audit receipts;
- false positives turned into regression scenarios.

The public README currently reports 59/59 in its full deterministic benchmark suite.

Adapt should treat exact completion/receipt mismatches as structured evidence. CodeRight may enforce an approved exact guard live; Adapt should learn whether and where the failure recurs.

### Warp Skill Doctor

Warp's skill-doctor workflow is the closest shipping comparator for §6.9's mutation-eligibility
doctrine: it clusters failures by root cause, prioritizes frequency × severity, verifies findings
against the actual repository/configuration, requires the smallest general edit justified by
evidence, and explicitly refuses instruction changes when the instruction already demanded correct
behavior, when the failure looks like model variance, when the only change would be redundant
restatement, or when the real owner is product/infra/scorer/code. Its skill-coverage signal
(installed skill never activated in a failed conversation) motivated the mechanical activation
evidence in §6.9, refined here into stage-level facts because "didn't trigger → description
defect" is too aggressive a heuristic. Its scoring architecture (fixed weightings, curved scores,
letter grades) is product decision, not learning-system architecture, and is not absorbed.

### Agent-failure research

MAST, coding-agent failure studies, model/client-specific failure research, and other labelled-trajectory work are useful for family taxonomies and held-out evaluation. They do not justify failure labels that cannot be honestly supported by the available evidence. For causal attribution specifically, Who&When (arXiv:2505.00212) reports that even frontier models attribute the responsible agent in barely half of multi-agent failures and the exact failure step far less — direct empirical support for §6.9's refusal to treat attribution as an automatic oracle.

## 16.3 Adjacent memory systems

Claude Code auto memory, Codex/Cline/Cursor memory/rules systems and similar products may provide editing/distribution UX patterns.

They are not direct Adapt competitors and must not define Adapt's semantic category.

# 17. Source register

## 17.1 Current repository evidence

Before execution, regenerate this inventory from current `main`.

Canonical implementation owners to inspect include:

- `engine/crates/membrane-adapt/`
- `engine/crates/membrane-transcript/`
- Adapt native CLI integration in Membrane runtime;
- Cortex admission/retrieval APIs;
- Ledger document/index integration;
- Pull context receipts;
- Push reduction/restore receipts;
- root CI and runtime-language manifest;
- CodeRight integration crates/configuration where available.

Legacy sources under `adapt/src/adapt/` and legacy tests are differential/oracle evidence only where the runtime-language manifest says they are release-excluded.

## 17.2 External product/research references

Taste:

- Command Code Taste documentation
- CHIRON — https://github.com/eragonlonelyboy-lab/chiron
- Vorpl
- Gensyn CodeAssist

Insights/eval loop:

- Braintrust Topics — https://www.braintrust.dev/docs/observe/topics
- Braintrust eval improvement loop — https://www.braintrust.dev/foundations/understanding-the-eval-improvement-loop
- Langfuse evaluation overview — https://langfuse.com/docs/evaluation/overview
- Langfuse annotation queues — https://langfuse.com/docs/evaluation/evaluation-methods/annotation-queues
- Langfuse datasets/experiments — https://langfuse.com/docs/evaluation/experiments/datasets
- Phoenix datasets/evaluators/experiments — https://arize.com/docs/phoenix/datasets-and-experiments/
- HORKOS — https://github.com/eragonlonelyboy-lab/horkos

Research references should be maintained in a separate research companion when detailed claims, benchmark tables, or competitor matrices are needed. The canonical product document should retain only stable architectural lessons.

# 18. Implementation-agent instructions

An agent implementing or repairing Adapt MUST start here and from current repository truth.

## 18.1 Preflight

1. Resolve current `main`; do not trust a stale commit printed in a historical status section.
2. Read this canonical document.
3. Read current Membrane canonical doctrine.
4. Read the native-runtime migration spec.
5. Read the Ledger canonical plan and CodeRight integration plan when the task crosses those seams.
6. Inspect current native owners before touching the Python oracle.
7. Re-derive §10 and the applicable capability matrix.
8. Classify the task as:
   - ontology/docs;
   - Taste;
   - Insights;
   - CodeRight evidence seam;
   - shared admission/integrity;
   - evaluation/CI;
   - retrieval/context-feedback integration.

## 18.2 Hard constraints

- Do not call Adapt a memory system.
- Do not treat agent-authored memories as Taste authority.
- Do not let Insights establish user preference.
- Do not create a second durable store.
- Do not make CodeRight raw tracing a Cortex replacement.
- Do not make Ledger a raw transcript authority.
- Do not add production Python/Node as a shortcut.
- Do not weaken explicit policy precedence.
- Do not widen malformed scopes.
- Do not tune against the final held-out benchmark.
- Do not let a model-created cluster become an authoritative issue/family without promotion evidence.
- Do not automate broad remediation from unqualified detectors.
- Do not claim a capability is landed because a module or index exists.

## 18.3 Production-path proof rule

For every capability claimed complete, provide:

1. **source proof** — implementation exists;
2. **integration proof** — the actual production path calls it;
3. **behavior proof** — the intended semantic behavior is exercised;
4. **measured proof** — where replacing/optimizing a path, frozen evidence shows it meets the acceptance threshold;
5. **installed proof** — when packaging/runtime is relevant, the installed candidate reaches the same path.

A test that proves outputs are unchanged whether a new mechanism is present or absent is evidence that the mechanism may be inert, not evidence that it shipped.

## 18.4 Runtime work order

Follow the companion native migration plan for runtime sequencing. This document defines Adapt meaning and remaining product work; it does not create a second native-port sequence.

## 18.5 Acceptance evidence

Depending on scope, evidence includes:

- authority/adversarial tests;
- exact evidence binding;
- scope fail-closed tests;
- semantic-seal mutation tests;
- portable synthetic corpus;
- real held-out corpus and interval report;
- native Rust tests;
- production-path execution receipts;
- CodeRight integration fixtures;
- evaluation experiment receipts;
- ontology terminology CI;
- native installed-artifact evidence;
- deletion/exclusion proof for retired interpreter paths.

# 19. Final canonical decisions

The following are the final product shape unless a future architecture amendment explicitly changes them:

1. **Adapt is governed behavioral learning, not memory.**
2. **Taste and Insights are equally first-class Adapt surfaces.**
3. **Taste learns user-authoritative preferences and behavioral constraints.**
4. **Insights learns evidence-backed failure patterns, gotchas, and waste.**
5. **Taste authority comes only from qualifying user evidence.**
6. **Insights cannot create user authority.**
7. **Cortex owns durable admission/storage/lifecycle/retrieval; Adapt does not become a parallel store.**
8. **Authored/current policy outranks learned Taste.**
9. **Taste delivery is bounded-core plus cheap scoped preference selection, not full-memory reconstruction.**
10. **Insights is episode/issue/outcome oriented and optimizes recurrence reduction.**
11. **Durable semantics are sealed; mutable lifecycle changes are receipted.**
12. **Malformed narrowing scope fails closed.**
13. **`episodic_fact` is not a Taste semantic class.**
14. **Model-generated extraction/clustering/remediation is proposal-only until bound to evidence and policy.**
15. **Known-family detection and emergent failure discovery are separate Insights lanes.**
16. **The final Membrane-owned Adapt runtime is native Rust.**
17. **Legacy Python is bounded migration/oracle evidence, not production authority.**
18. **Synthetic conformance and real held-out evaluation are separate evidence classes.**
19. **A detector/evaluator may not claim production precision from the synthetic corpus alone.**
20. **CodeRight should emit structured execution observations when it knows a fact directly rather than forcing Adapt to infer it from prose.**
21. **CodeRight owns generic traces, datasets, experiments, evaluator execution, and model/harness comparison.**
22. **Adapt may propose behavioral evaluators; CodeRight executes them and returns typed outcomes.**
23. **Confirmed production failures should be convertible into privacy-safe reviewed regression cases.**
24. **Emergent clusters cannot self-promote to durable issues or guards.**
25. **Ledger is the canonical Membrane document-navigation/index subsystem name; historical Guide/Spine wording is retired.**
26. **Ledger projections do not replace exact structured execution/transcript evidence.**
27. **Competitor research is evidence, not product ontology.**
28. **CI must prevent Adapt/memory conflation from re-entering current documentation.**
29. **A capability is not landed until the production path exercises it and acceptance evidence qualifies it.**
30. **Success means fewer repeated corrections and fewer recurring failures without weakening correctness, security, authority, or task performance.**
31. **A remediation proposal targeting a mutable instruction surface is actionable only after an explicit intervention attribution supports counterfactual preventability on the current surface digest, refuses already-correct/redundant/wrong-owner mutations, and accounts for alternative causes (§6.9).**

# 20. Canonical short copy

## One line

> **Adapt learns how agents should improve: Taste captures user-backed preferences; Insights captures recurring failures and gotchas.**

## Short description

> **Adapt is Membrane's governed behavioral-learning subsystem. Taste learns user-authoritative preferences from explicit instructions, corrections, choices, and supported behavioral signals. Insights detects recurring agent/model failures, gotchas, instruction violations, frustration, and waste. Both remain evidence-bound, scoped, reviewable, and admitted through Cortex; neither is generic agent memory.**

## Architecture boundary

> **Adapt owns behavioral learning from experience. Cortex owns durable admission and storage. Ledger owns document navigation/index projections. CodeRight owns agent execution and generic eval/trace machinery. Repository policy owns explicit project rules. These boundaries must remain visible in code, docs, schemas, evaluation, and UI.**
