# Adapt Canonical Product and Architecture Specification

**Status:** Canonical product and architecture source of truth
**Date:** 2026-08-24
**Repository:** `Orthic-Labs/Membrane`
**Supersedes:** `adapt/docs/plans/2026-08-24-adapt-alignment-implementation.md` as active Adapt authority; that plan remains historical implementation provenance
**Companion runtime plan:** `migration/native-rust/MEMBRANE-NATIVE-RUST-MIGRATION-AND-CODERIGHT-INTEGRATION.md`
**Current implementation baseline:** `main@7c05b49b6f9ea202116f6829e4f74949a4529592`
**Research input reconciled:** Claude `c0a1b463a6a792ea5a8c931d4791715a0e3ef497` (`docs/research/competitors/adapt-analysis.md`) plus the verified external analysis retained in the repository
**Runtime architecture:** governed by the Membrane native-Rust migration specification; Adapt is not exempt from Membrane's native-only runtime rule
**Audience:** Adapt, Cortex, transcript, Membrane runtime, Hub, CodeRight integration, evaluation, documentation, and release-engineering implementers

This document defines **what Adapt is**, **what it owns**, **what it must never become**, contracts for **Taste** and **Insights**, authority and persistence boundaries between Adapt and Cortex, language-neutral migration requirements, current implementation delta, and acceptance gates for future work.

It is deliberately different from `docs/research/competitors/adapt-analysis.md`. The research document explains why certain choices were made and how Adapt compares with external systems. **This document is normative product truth.** If research, historical plans, README copy, comments, generated documentation, or implementation details conflict with this document, they must be corrected or the conflict must be raised as an explicit architecture change.

---

## Document authority and relationship to other canonical specs

This document is the **Adapt semantic/product authority**. It answers: what Adapt is, what Taste and Insights mean, what Adapt owns, what evidence may authorize them, and what invariants an implementation must preserve.

It does **not** replace the Membrane-wide native migration specification. The documents divide authority as follows:

1. **`docs/subsystems/MEMBRANE_CANONICAL_ARCHITECTURE_AND_IMPLEMENTATION_DOCTRINE.md`** — cross-subsystem ownership and six-axis architecture.
2. **This Adapt canonical specification** — Adapt product semantics, Taste/Insights contracts, authority, governance, evaluation, and feature dependencies.
3. **`migration/native-rust/MEMBRANE-NATIVE-RUST-MIGRATION-AND-CODERIGHT-INTEGRATION.md`** — repository-wide runtime/process cutover, packaging, deletion, Blueprint/CodeRight seams, sequencing, and native-only release closure.
4. `docs/subsystems/adapt.md` and `adapt/README.md` — concise projections of this document; they MUST NOT invent competing semantics.
5. `docs/research/competitors/adapt-analysis.md` — research provenance and external comparison only; non-normative.
6. Historical plans/specs — evidence of prior decisions only after they are marked superseded where they conflict with current canon.

For a cross-cutting conflict, the narrowest relevant canonical owner controls its semantic domain. For example, the Membrane migration spec controls the rule that production Adapt must become native Rust; this document controls what the Rust port must mean by Taste and Insights.

**CodeRight is an Adapt consumer/integration host, not the owner of Adapt semantics.** CodeRight may supply transcript/user-act evidence and consume applicable Taste/Insight delivery, but it does not redefine Taste authority, Insight semantics, or Cortex admission.

---

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
| **Taste** | authenticated user acts and user-backed behavioral evidence | preferences and behavioral constraints | How does this user want me to behave or make choices? |
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
- Taste accepts explicit and selected implicit user signals without authority laundering.
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

## 3.1 Transcript substrate

Transcript normalization is infrastructure used by Adapt, not the identity of Adapt itself.

The canonical native transcript layer must produce a language-neutral event contract equivalent to `TranscriptEventV1` with:

- stable event/session identity;
- host/source identity;
- role/origin classification;
- byte/source spans;
- event ordering;
- tool invocation/result relationships;
- timestamps where available;
- provider/model/client metadata where available;
- parser receipts and source digests;
- typed unavailable/partial failures;
- no silent conversion of missing input into empty-success.

The current Python `continuity.transcript` package is migration input. The final implementation belongs in native Rust and should be reusable by Adapt and other Membrane consumers.

Source hosts own raw transcripts until an explicit durable-admission decision. Native transcript code owns normalization, stable event identity, provenance, and typed parse failures. Guide may project navigation over admitted material, but it does not own transcript truth.

## 3.2 Adapt shared control plane

Adapt owns:

- transcript/event ingestion for behavioral learning;
- canonicalization of learning evidence;
- origin/provenance filtering;
- Taste candidate generation;
- Insights detection and issue formation;
- scope/applicability proposals;
- contradiction/supersession proposals;
- evidence binding;
- learning audit and rollback metadata;
- delivery receipts and effectiveness telemetry for Adapt-derived interventions.

Adapt does **not** own:

- canonical durable database authority;
- direct ungoverned durable writes;
- repository truth;
- organization/current-instruction policy authority;
- final context budget policy for all Membrane content;
- generic document indexing;
- generic memory creation;
- application effect authorization.

## 3.3 Native runtime destination

The final Membrane-owned implementation MUST be native Rust.

A practical initial destination is one `membrane-adapt` crate with internal modules rather than premature crate proliferation:

```text
engine/crates/membrane-adapt/
  src/
    lib.rs
    evidence.rs
    taste/
    insights/
    authority.rs
    scope.rs
    conflict.rs
    lifecycle.rs
    admission.rs
    delivery.rs
    receipts.rs
    model_boundary.rs
```

The transcript contract should live in a reusable native owner such as `membrane-transcript` or an equivalent module with an independently testable public contract.

The exact crate names are not normative. The ownership boundaries are.

## 3.4 Model/proposal boundary

LLMs may assist with:

- extracting a possible preference from user evidence;
- rewriting a preference candidate into compact canonical wording;
- proposing semantic duplicate/conflict groups;
- clustering semantically related failure episodes;
- proposing candidate remediation text.

LLMs MUST NOT decide authority by assertion.

All model-generated outputs are proposals. Deterministic code must bind them back to admissible evidence and enforce origin, scope, policy, and write contracts.

No model output can manufacture:

- a user source span;
- a user preference;
- a permission grant;
- a verification receipt;
- a tool result;
- a policy exception;
- a stronger scope than supported evidence.

## 3.5 Three distinct admission decisions

Do not collapse these gates:

1. **Adapt proposal eligibility** decides whether evidence may form a Taste candidate or Insight episode/issue.
2. **Cortex durable admission** decides whether a typed record may enter durable knowledge, with lifecycle, conflict, supersession, and integrity semantics.
3. **Membrane context admission** decides whether an eligible retrieved record belongs in a context packet under authority, freshness, sufficiency, budget, and representation policy.

Passing one gate grants no authority at either later gate.

## 3.6 Internal contract namespace

`TranscriptEventV1`, `UserActEvidenceV1`, `FailureEpisodeV1`, and `InsightIssueV1` are versioned internal domain contracts. Their `V1` suffix versions each subsystem schema; it does not add them to Membrane's five public V1 protocol shapes.

N1 migration fixtures must register each internal contract's canonical schema, version, and digest. Moving one onto `membrane-protocol`, MCP, or a public client surface requires a real external consumer plus an explicit public-protocol decision.

---

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

Silent acceptance alone MUST NOT activate Taste. It may increase support for a separately authorized candidate. Post-accept edits count only when a host emits authenticated human-act evidence binding before/after content, actor, session, and provenance.

## 4.3 UserActEvidenceV1 target contract

Implicit Taste requires a first-class evidence object. An equivalent contract should include:

```text
UserActEvidenceV1
  evidence_id
  installation_id
  host
  session_id
  event_id(s)
  act_kind
    explicit_preference
    correction
    reject
    accept
    post_accept_edit
    repeated_edit
    named_choice
  before_digest? / before_excerpt?
  after_digest? / after_excerpt?
  user_source_span?
  scope_context
  timestamp
  signal_strength
  provenance_receipt
```

The object records what happened. It does not itself state the final preference.

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

A safe fixed precedence is:

1. current explicit user instruction;
2. safety / organization policy;
3. explicit repository policy and committed agent instructions;
4. explicit scoped user preference;
5. explicit global user preference;
6. inferred scoped user preference;
7. inferred global user preference;
8. trusted imported preference;
9. provisional candidate.

A lower tier cannot repeal a higher tier. Authority/evidence class resolves before specificity, so inferred scoped evidence cannot override explicit global evidence. Specificity resolves only within one authority tier. When two applicable items conflict at the same tier and specificity, surface conflict or apply a deterministic explicit rule; retrieval order never decides.

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

## 6.4 Current detector families

At `main@7c05b49`, the Python implementation contains nineteen deterministic detector families, including verification failures, repeated asks, visible frustration, ignored tool failure, false-not-found, broad searching, wrong repo/subsystem, stale terminology, silent scope narrowing, omitted requirements, unaccepted plan changes, tests that cannot fail, cross-agent repeats, unfinished Forge work, guard firings, postmortem asks, and swearing/frustration signals.

Those detectors are implementation evidence, not the permanent limits of Insights.

## 6.5 Required additional failure classes

The canonical product must add first-class coverage for common high-cost behavioral failures that are currently underrepresented:

- `overengineering`
- `architecture_churn`
- `repeated_redesign`
- `planning_instead_of_executing`
- `unnecessary_abstraction`
- `unnecessary_dependency`
- `scope_expansion_without_request`
- `repeated_scope_expansion`
- `verification_theatre`
- `false_completion_claim`
- `instruction_noncompliance`
- `repeated_user_correction_same_theme`
- `model_specific_gotcha`
- `client_or_tool_specific_gotcha`

Detectors must be operationally defined. Labels without measurable criteria are not enough.

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

Adapt runtime migration, packaging, process policy, deletion gates, Blueprint/CodeRight seams, and native-only release sequencing are owned exclusively by `migration/native-rust/MEMBRANE-NATIVE-RUST-MIGRATION-AND-CODERIGHT-INTEGRATION.md`.

This document contributes Adapt semantics and language-neutral acceptance fixtures to that migration. Current Python implementation remains migration input and a temporary differential oracle only; it is not target architecture.

---

# 10. Current implementation truth at `main@7c05b49`

This section is descriptive, not normative. It exists so implementation agents know which canonical contracts already have landed evidence.

## 10.1 Taste strengths already landed

- canonical external-user origin can establish preference authority;
- assistant/developer/tool/subagent/context provenance is quarantined;
- direct evidence/source binding exists;
- immutable/reviewed manifest pipeline exists;
- payload digests and source-session digests exist;
- permission-expansion/security weakening refusal exists;
- held-out semantic validation runs after adjudication and before apply;
- semantic receipt binding exists in persistence;
- lifecycle/scoping record structures exist;
- root-only bounded core concept exists;
- secret redaction plus gitleaks scanning exists;
- multi-source frozen transcript snapshots exist beyond the narrow direct path.

## 10.2 Taste implementation gaps

- production learning does not yet consume a complete accept/reject/post-accept-edit signal stream;
- final semantic/applicability sealing is incomplete;
- semantic duplicate-group membership is model-decided;
- scope dimensions are narrower than the canonical target;
- malformed scope normalization can widen applicability if not made fail closed;
- core compilation has a literal workspace/root assumption (`D--Claude`) that is nonportable;
- local `rules.json` projection may omit lifecycle/scope semantics and must not be treated as a canonical mirror;
- signed evidence-preserving export/import is absent;
- user-facing inspect/edit/narrow/delete/distribution UX is incomplete.

## 10.3 Multi-source status

One native Rust owner now discovers & parses raw Claude Code, Codex,
CommandCode, Cline, OpenCode, Qwen, Pi, Gemini, Grok Build, Roo/Cline, & Cursor
stores. `membrane adapt mine --discover-open` records typed per-source omissions;
readable zero-event input cannot report empty success. Frozen snapshots remain
accepted migration inputs, not production parser dependencies.

| Host | Native discovery | Raw parser | Source identity/provenance fixture | Notes |
|---|---:|---:|---:|---|
| Claude Code | yes | yes | yes | JSONL |
| Codex | yes | yes | yes | JSONL |
| CommandCode | yes | yes | yes | JSONL; checkpoint files excluded |
| Cline | yes | yes | yes | JSON document |
| OpenCode | yes | yes | yes | SQLite; open sessions only |
| Qwen | yes | yes | yes | standalone Qwen/Qwen Code JSONL roots |
| Pi | yes | yes | yes | JSONL |
| Gemini | yes | yes | yes | JSON or JSONL |
| Grok Build | yes | yes | yes | JSONL |
| Roo/Cline | yes | yes | yes | Cursor/Code roots on Mac & Windows |
| Cursor | yes | yes | yes | SQLite; open root composers only |

User-act authority remains evidence-contract constrained; parser coverage alone
does not upgrade assistant, tool, synthetic, or inferred text into user acts.

## 10.4 Insights strengths already landed

- deterministic episode detectors;
- exact event/span evidence;
- deterministic card identities;
- explicit honesty-limit copy;
- verification-negation handling for one known false-positive class;
- provider-reported token totals and conservative inferred attribution;
- separate reference-only Cortex persistence module.

## 10.5 Insights implementation gaps

- no portable labelled benchmark with per-detector precision/recall;
- real-transcript tests are machine-local/skipped on clean environments;
- no first-class `InsightIssueV1` longitudinal issue object;
- semantic recurrence/clustering remains weak;
- major behavioral families such as over-engineering and architecture churn are not first-class;
- no complete issue → guard/evaluator/remediation → recurrence-measurement loop;
- primary CLI/report story and persistence story are inconsistent;
- always-on context cost is measured as unexplained prefix/overhead but not resolved to specific persistent sources;
- quote/context-carried verification language remains a false-positive class to benchmark.

## 10.6 CI/runtime gap

Root repository CI does not currently establish native Adapt correctness or native-only runtime closure.

The end state must run Adapt contract/evaluation tests from the Rust workspace and include an installed-artifact test with Python/Node absent for Membrane-owned Adapt behavior.

---

# 11. Evaluation and quality gates

## 11.1 Taste evaluation

Taste must be evaluated separately for explicit and implicit evidence.

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

Per-detector benchmarking is P0 for Insights automation.

Every detector or issue-family promotion should measure:

- precision;
- recall where a labelled corpus makes recall knowable;
- false positives from negation;
- quoted/context-carried text;
- tool-result text;
- assistant narration of hypothetical failures;
- cross-session duplicate detection;
- severity calibration;
- user-dismissal rate.

Detector precision is a blocker for automatic remediation **inside the Insights lane**. It is not a global blocker for independent Taste, documentation, Rust migration, integrity, or CI work.

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

`apparently` is load-bearing: absence of lexical overlap or recall events cannot prove a persistent source had no behavioral influence.

---

# 14. Ranked implementation plan

## P0 — product boundary and runtime closure

### P0.1 Commit this canonical ontology

- make this document the Adapt normative source;
- update canonical Membrane doctrine to point to it;
- update `docs/subsystems/adapt.md`;
- regenerate product/architecture docs from corrected source templates;
- update Adapt README and agent rules.

### P0.2 Add an ontology regression gate

Fail CI when current-product docs describe:

- Adapt as a memory system/substrate/control plane;
- Taste as memory;
- Insights as memory;
- generic "learned memories" when the object is a Taste preference or Insight;
- Cortex as if it were identical to Adapt.

The checker should allow legitimate phrases such as `Cortex durable memory`, `agent memory`, and historical research quotations in explicitly marked historical/research locations.

### P0.3 Bind Adapt delivery to native migration

Implement Adapt runtime cutover only through the companion migration plan. Freeze this document's contracts as language-neutral fixtures before porting and preserve Python only as a bounded differential oracle until its deletion gate passes.

### P0.4 Complete semantic/applicability sealing

Implement immutable semantic payload + receipted mutable lifecycle transitions.

### P0.5 Build a portable labelled Insights benchmark

Include positive, negative, negated, quoted, tool-carried, hypothetical, cross-session, and real failure cases.

### P0.6 Resolve the Insights admission boundary

All durable Insight issues/references must cross the same explicit Cortex admission architecture, with reference-only influence by default.

### P0.7 Put Adapt in root/native CI

During migration: run Python-vs-Rust contract/differential tests.

Final state: Rust tests are authoritative; installed artifact runs with Python/Node absent for Membrane-owned Adapt behavior.

## P1 — richer Taste

### P1.1 Add user-act signal capture

Implement host-specific accept/reject/post-accept-edit/named-choice adapters with explicit capability reporting.

### P1.2 Add counterfactual preferences

Preserve rejected/preferred alternatives where evidence supports them.

### P1.3 Risk-tier review UX

Example policy:

- explicit high-confidence user preference with safe bounded scope → low-friction admission;
- inferred edit pattern → review or provisional;
- broad global inference → review required;
- safety/security/permission-adjacent candidate → fail closed / explicit review.

### P1.4 Delivery receipts and effectiveness telemetry

Measure selection, adherence, correction, override, and retirement.

### P1.5 Signed export/import

Export semantic record + evidence digests + provenance + scope + authority + lifecycle history. Imports remain lower precedence than local explicit preference unless explicitly promoted.

## P1 — operationalize Insights

### P1.6 Add `InsightIssueV1`

Cluster episodes into longitudinal issues while preserving episode evidence.

### P1.7 Add missing behavioral families

Prioritize over-engineering, architecture churn, scope expansion, planning-instead-of-doing, false completion, and instruction noncompliance.

### P1.8 Add hybrid recurrence discovery

Use deterministic detectors for high-precision signatures plus semantic clustering for recurrence discovery. Model-assisted clustering remains a proposal until evidence grouping is verified.

### P1.9 Add guard/evaluator/remediation proposals

Keep effect/authority separate from the Insight record.

### P1.10 Add mitigation outcome tracking

Measure recurrence after intervention and reopen issues on regression.

### P1.11 Implement persistent-context cost attribution

Use measured totals and honest inferred splits.

## P2 — measured adaptive behavior

- Taste counterfactual A/B evaluation;
- per-model/client Taste effectiveness;
- per-model/client Insight recurrence;
- automatic retirement suggestions after model/tool changes;
- team/org Taste packaging only after origin and precedence contracts are proven;
- organization-level aggregate Insight analytics without leaking personal/private transcript content.

---

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

Research is evidence for improvement, not product authority. External product behavior may change and must be reverified before making time-sensitive claims.

## 16.1 Taste comparators

### Command Code Taste

Closest direct shipping comparator identified. Useful mechanisms:

- accept/reject/post-accept-edit signals;
- project/global/remote Taste packaging;
- inspect/list/lint/push/pull/compose UX;
- immediate in-session conditioning.

Adapt should absorb signal breadth and distribution UX without adopting opaque authority or allowing inferred preferences to outrank explicit committed policy.

### Vorpl

Relevant behavioral-learning comparator because it learns corrections/preferences/repeated patterns and measures rule effectiveness. Useful lesson: measure whether learned interventions actually change outcomes.

### CHIRON

Relevant correction-to-rule compiler. Useful lesson: keep correction/gotcha evidence inspectable and contradiction-aware.

### Gensyn CodeAssist

Historical precedent for learning from typing/fixes/deletions/retained output. Useful for signal design, not a current shipping benchmark.

## 16.2 Insights comparators

### LangSmith Insights / Engine

Strong reference for recurring failure discovery and the detect → diagnose → fix → evaluator → regression/reopen loop.

### Braintrust Topics / Loop

Useful for semantic issue discovery and turning recurring problems into datasets/scorers/optimization loops.

### HORKOS

Narrow deterministic enforcement reference for explicit guardable failure classes.

### Phoenix

Useful observability/evaluation/annotation substrate; not by itself a complete Adapt Insights product.

### Failure research

MAST, TRAIL, coding-agent failure studies, Who&When Pro, and related work provide taxonomies and labelled trajectories useful for benchmarking Insights. They are not a reason to broaden Insights beyond what can be honestly detected from available evidence.

## 16.3 Adjacent memory systems

Claude Code auto memory, Codex memories, Cline memory banks, Cursor rules, and similar systems may provide useful storage/retrieval/editing UX patterns.

They are **not direct Adapt competitors** and must not be used to define Adapt's semantic category.

---

# 17. Source register

## 17.1 Current Membrane / Adapt implementation

- `main@7c05b49b6f9ea202116f6829e4f74949a4529592` — held-out semantic admission repair
- `f602fbbaec1d13629e6b09ca4d6d4c07277ad7ba` — multi-source transcript learning
- `adapt/src/adapt/taste_v2.py`
- `adapt/src/adapt/taste_v2_pipeline.py`
- `adapt/src/adapt/preference_record.py`
- `adapt/src/adapt/manifest.py`
- `adapt/src/adapt/taste_apply.py`
- `adapt/src/adapt/semantic_validate_manifest.py`
- `adapt/src/adapt/consolidate_manifest.py`
- `adapt/src/adapt/insights.py`
- `adapt/src/adapt/insight_persistence.py`
- `adapt/src/adapt/token_spend.py`
- `adapt/src/adapt/transcript_snapshots.py`
- `adapt/src/adapt/mine_snapshot_manifest.py`
- `continuity/transcript/`
- `scripts/run-adapt-installed-current.mjs`
- `docs/subsystems/MEMBRANE_CANONICAL_ARCHITECTURE_AND_IMPLEMENTATION_DOCTRINE.md`
- `docs/subsystems/adapt.md`
- `adapt/README.md`

## 17.2 Reconciled Adapt research

- Claude consolidated analysis commit `c0a1b463a6a792ea5a8c931d4791715a0e3ef497`
- prior Claude research commit `35711f965463b80d8d2a24875e83f1305f28d69d`
- `docs/research/competitors/adapt-analysis.md`
- external verified analysis previously retained as `sources/ADAPT-BEHAVIORAL-LEARNING-ANALYSIS.md`

## 17.3 Taste references

- https://commandcode.ai/docs/taste
- https://commandcode.ai/blog/taste-skills-rules
- https://commandcode.ai/docs/core-concepts/memory — relevant specifically because Command Code separates memory from Taste
- https://vorpl.ai/
- https://github.com/eragonlonelyboy-lab/chiron
- https://docs.gensyn.ai/testnet/codeassist

## 17.4 Insights references

- https://docs.langchain.com/langsmith/insights
- https://docs.langchain.com/langsmith/engine
- https://www.braintrust.dev/docs/observe/topics
- https://www.braintrust.dev/docs/loop
- https://arize.com/docs/phoenix
- https://github.com/eragonlonelyboy-lab/horkos
- https://arxiv.org/abs/2605.29442
- https://github.com/multi-agent-systems-failure-taxonomy/MAST
- https://arxiv.org/abs/2607.09996
- https://arxiv.org/abs/2607.18754

---

# 18. Codex / implementation-agent instructions

An agent asked to implement or repair Adapt MUST begin from this section and the current repository head.

## 18.1 Preflight

1. Confirm `main` HEAD; do not assume `7c05b49` is still current.
2. Read this canonical document.
3. Read current Membrane canonical doctrine and native-runtime migration spec.
4. Inspect `adapt/`, `continuity/`, Cortex admission APIs, root CI, generated docs, and installed Adapt launch sites.
5. Classify work as one of:
   - ontology/docs;
   - native migration;
   - Taste behavior;
   - Insights behavior;
   - shared admission/integrity;
   - evaluation/CI.
6. Do not mix unrelated categories in one large implementation unless dependency order requires it.

## 18.2 Hard constraints

- Do not call Adapt memory.
- Do not treat agent-authored memories as Taste authority.
- Do not let Insights establish user preference.
- Do not create a second durable store.
- Do not add new production Python/Node implementation as a shortcut.
- Do not weaken explicit policy precedence.
- Do not widen malformed scopes.
- Do not preserve known integrity defects for bug-for-bug parity during the Rust port.
- Do not automate remediation from unmeasured Insight detectors.
- Do not claim completion while installed Adapt still needs Python.

## 18.3 First documentation repair

The first bounded docs change should:

- add this canonical document at a stable path such as `docs/subsystems/ADAPT_CANONICAL_PRODUCT_AND_ARCHITECTURE.md`;
- point `docs/subsystems/adapt.md` to it;
- update Membrane doctrine's Adapt section;
- rewrite Adapt README product description/surfaces;
- update Adapt `AGENTS.md`/`CLAUDE.md` guidance;
- update the product-truth generator source, then regenerate generated docs;
- add an ontology terminology checker to CI;
- mark older `coding-taste memory` plans as historical/superseded rather than current truth.

## 18.4 Runtime work order

Follow the companion migration plan's N0-N10 sequence. This document decides Adapt meaning and feature dependencies; it does not create a second runtime sequence.

## 18.5 Acceptance evidence

An implementation is not complete because code exists. Required evidence includes:

- contract tests;
- adversarial authority tests;
- scope fail-closed tests;
- semantic-seal mutation tests;
- portable Insights benchmark results;
- native Rust tests;
- installed-artifact process-tree evidence;
- Python/Node-absent Adapt execution evidence;
- regenerated docs check;
- ontology terminology CI pass;
- deletion receipt for retired production Python paths.

---

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
10. **Insights is episode/issue/outcome oriented and must measure recurrence reduction.**
11. **Durable semantics are sealed; mutable lifecycle changes are receipted.**
12. **Malformed narrowing scope fails closed.**
13. **`episodic_fact` is not a Taste semantic class.**
14. **Model-generated extraction/clustering/remediation is proposal-only until bound to evidence and deterministic policy.**
15. **The final Membrane-owned Adapt runtime is native Rust.**
16. **Python Adapt is migration scaffolding and must be deleted from installed production paths after parity/cutover.**
17. **Insights detector measurement gates automation in the Insights lane, not unrelated Adapt work.**
18. **Competitor research is evidence, not product ontology.**
19. **CI must prevent Adapt/memory conflation from re-entering current documentation.**
20. **The measure of success is fewer repeated corrections and fewer recurring failures, without weakening correctness, security, or user authority.**

---

# 20. Canonical short copy

## One line

> **Adapt learns how agents should improve: Taste captures user-backed preferences; Insights captures recurring failures and gotchas.**

## Short description

> **Adapt is Membrane's governed behavioral-learning subsystem. Taste learns user-authoritative preferences from explicit instructions, corrections, choices, and supported behavioral signals. Insights detects recurring agent/model failures, gotchas, instruction violations, frustration, and waste. Both remain evidence-bound, scoped, reviewable, and admitted through Cortex; neither is generic agent memory.**

## Architecture boundary

> **Adapt owns learning from experience. Cortex owns durable admission and storage. Agent memory owns contextual recall. Repository policy owns explicit project rules. These boundaries must remain visible in code, docs, schemas, evaluation, and UI.**
