# Membrane — Pending Implementation

**Status:** pending specification. Not canon.
**Scope:** audited production-path gaps plus non-experimental target contracts, including Membrane's side of the host seam.

## Document authority

Subordinate to, and must not contradict (paths relative to `docs/subsystems/`):

1. `MEMBRANE_CANONICAL_ARCHITECTURE_AND_IMPLEMENTATION_DOCTRINE.md`
2. `BLUEPRINT_CANONICAL_SOURCE_OF_TRUTH.md`
3. `ADAPT_CANONICAL_PRODUCT_AND_ARCHITECTURE.md`
4. `LEDGER-MARKDOWN-INDEXING-AND-DOCUMENT-NAVIGATION-CANON.md`
5. `MEMBRANE-CROSS-SUBSYSTEM-IMPROVEMENTS-AND-EVIDENCE-GATES.md`
6. `CODERIGHT-MEMBRANE-OBSERVABILITY-LEARNING-AND-EVAL-INTEGRATION.md`

Canon drift is a defect. A conflict here is resolved against the canon, and this file is corrected.

## Host neutrality (binding rule for this document)

Membrane is an independent product. This document specifies Membrane's work and Membrane's
side of the seam **as a host-neutral contract**. It names no host implementation, no host
repository path, and no host-internal type. Where a capability requires something from the
host, it is stated as a required *host capability* (`H1`…`H10`, §12) that any harness may satisfy.

A first-party host implements those capabilities in its own repository under its own
specification. That document depends on this one. This one never depends on it.

## Optional work is not here

Bounded LLM semantic assistance is experimental and optional. It lives in
`MEMBRANE-SEMANTIC-ADVISOR-EXPERIMENTAL.md` and can be deleted without affecting anything below.
Everything in this document must work with that capability absent.

## What this document replaces

| Superseded | Disposition |
|---|---|
| `MEMBRANE-INTERVENTION-OUTPUT-AND-HARNESS-EVOLUTION.md` (pending) | absorbed; host-side harness-evolution loop becomes capability `H7` |
| `MEMBRANE-CROSS-SUBSYSTEM-...md` former rollout text in §4, §5.2, §5.3, §6.2, §6.3, §14 | pending work captured here; canon retains contracts and evidence gates |
| `MEMBRANE-CROSS-SUBSYSTEM-...md` §5.4, §6.4, §12 | host-side; become `H4`/`H6` in §12 |
| `CODERIGHT-MEMBRANE-OBSERVABILITY-...md` former §16 C0–C10 rollout | Membrane work captured here; host-owned work remains in the host repository |

Doctrines 1–4 are **not** superseded by this document. A doctrine describes target state; containing
unbuilt capability is its function, not a defect. This document claims landed state only where §0
records a complete production trace and bound acceptance evidence.

---

# 0. Production-path audit

The locked invariant is unchanged: a capability is landed only when its production path executes
the contract and frozen acceptance evidence shows it meets or improves the baseline it replaces.
This audit therefore records three independent facts instead of inferring status from names:

1. implementation presence;
2. production reachability from a real entry point;
3. qualification evidence bound to that production path.

`LANDED` is reserved for rows with all three. `UNWIRED` means implementation exists but the traced
production path does not call it. `GAP ON TRACED PATH` means the active path was read and does not
execute the specified contract. `NOT AUDITED` means exactly that; it never means absent.

## 0.1 Audited paths

| Capability | Implementation evidence | Production trace | Qualification evidence | Disposition |
|---|---|---|---|---|
| Native provider merge | fixed provider/security ordering with versioned strategy identity | CLI and resident federation paths emit `membrane-fusion-fixed-v1` receipts | focused production-path tests pass; no frozen comparative qualification | `IMPLEMENTED + REACHABLE`; control remains active |
| RRF fusion | canonical RRF implementation and versioned receipt | same federation path can select `membrane-fusion-rrf-v1` explicitly | focused selection/receipt tests pass; no frozen comparative qualification | `IMPLEMENTED + REACHABLE`; not active by default |
| Production fusion identity and receipt | every fusion result carries named/versioned strategy data | active fixed-order and selectable RRF paths emit the receipt | focused receipt validation passes | `IMPLEMENTED + REACHABLE` |
| Typed omission accounting | federation scheduler/normalizer emits `ProviderOmissionV1`; native adapter maps omissions into planner `OmissionV1` | same CLI/resident path above → CCS → `packet.omissions` alongside candidate receipts | not revalidated in this consolidation pass | `REACHABLE`; no sufficiency claim |
| Corrective retrieval after insufficiency | bounded planner-owned sufficiency contract, evaluator and one-stage plan exist | resident runtime forwards an explicit caller-supplied contract and exposes its receipt; no first-party caller supplies one and no distinct corrective action executes | evaluator and transport tests pass; no production-path qualification | `PARTIAL`: add planner caller, distinct bounded action, then qualify |
| Query-aware Push | query-aware provider plus explicit control/query-aware policy | production `push prep` reaches either policy; control remains default | protected-span and reachability tests pass; no frozen measured baseline | `IMPLEMENTED + REACHABLE`; not qualified |
| Push qualification baseline | `docs/evidence/qualification/push-metrics.json` names desired metrics | file reports `fixture-entrypoint-defined`; it contains no measured baseline | no frozen measured baseline in that artifact | `NOT QUALIFIED` |
| Deterministic Cortex Dream | `store::dream_now_observed` invokes restricted policy plus deterministic consolidation | CLI `dream_now` and resident `dream_now_observed` call the store path | not revalidated in this consolidation pass | `REACHABLE`; no `LANDED` claim |
| Sealed remediation proposals and taste gate | sealing, effect mapping, user-evidence and precision gates | production Adapt mine path emits deterministic sealed review proposals | focused unit and current-contract tests pass | `IMPLEMENTED + REACHABLE` |
| `intervention_target` / `routing_recommendation` | orthogonal target and reachable proposal-kind contracts | production Adapt mine path seals both fields into proposal identity | schema/digest/reachability tests pass | `IMPLEMENTED + REACHABLE` |
| Background review mechanics | fail-closed config, host activity/token gate, bounded jobs, proposal-only learner interface, cancellation, retry, single-flight, typed observations and bounded H5 sink | tray-owned daemon initializes the scheduler and persists H5 receipts, but supplies no real semantic provider, cursor, foreground-memory signal or proposal sink | focused lifecycle, budget, receipt and persistence tests pass | `PARTIAL`: production semantic inputs remain absent |
| Cortex Stage 1 | proposal-only review and memory-candidate extraction contracts | no production runner supplies model output or authoritative foreground-memory signal | focused validation tests pass | `UNWIRED` |
| Learning Lineage and Insights projection | read-only receipt graph and projection with typed unavailable joins | production Adapt mine response emits both | deterministic projection tests pass | `IMPLEMENTED + REACHABLE`; host tail remains unavailable |
| `PacketReductionPlanV1` | validated versioned plan, estimator basis and largest-fit selection | production `push select` accepts an H8 ceiling and returns only a complete published representation; first-party host emits a next-request H8 candidate from real policy and last observed usage | protocol/runtime/host-producer tests pass | `PARTIAL`: request-time refresh and direct host-to-Membrane delivery remain unwired |
| Host observation seam | H4/H6/H8/H9/H10 versioned shapes, provenance and closed-shape validation | H4/H6 producers and an H8 candidate producer exist in first-party host; bounded Membrane H4/H6/H8 parser and exact-ID H4/H6 lineage join exist; caller transport remains unwired; H9/H10 have no producer | protocol, parser and host producer tests pass where inputs exist | `PARTIAL` |

## 0.2 Not audited by this pass

Procedural-asset effectiveness, closed-loop qualification, H7 outcome joins, and host capabilities
H1–H3/H9–H10 remain unaudited or unavailable at their production sources. No effectiveness
record may be synthesized until exposure, selection, application, outcome, correction, token-cost,
model, client, evaluator, dataset and experiment inputs exist with provenance.

Doctrines 1–4 also claim capabilities beyond this table. Their claims are target-state contracts,
not proof of landed behavior.

---

# 1. Corrections established by the audit

## 1.1 Query-aware Push is selectable but not qualified

Control and query-aware reduction are explicit production policies. Control remains the default.
Query-aware reduction cannot become the default until a measured control baseline and frozen
qualification show non-regression for protected evidence, semantic preservation, fit, latency and
cost. See §10.

## 1.2 Production fusion has explicit control and candidate strategies

Active federation defaults to fixed provider/security ordering and emits
`membrane-fusion-fixed-v1`. RRF is explicitly selectable as `membrane-fusion-rrf-v1`. Every result
carries strategy identity and parameters. RRF cannot replace control until frozen comparative
qualification proves it.

`membrane-federation/src/shadow.rs` is shadow *execution* of federation requests with an effect
policy. It is not fusion strategy selection and must not be overloaded.

## 1.3 Scheduling gate

Schedule only gaps supported by a traced production path or an explicit typed absence at that path.
Bind qualification evidence to the exact policy and receipt identity it evaluates.

A capability may not move to `LANDED` on source or unit tests alone.

---

# 2. Adapt — `intervention_target`

## 2.1 Final proposal shape

Authority: `engine/crates/membrane-adapt/src/remediation.rs`.

`RemediationEffect`:

```text
ProcessChange | GuardrailAddition | DocumentationUpdate | ToolingFix | TasteCandidate
```

`RemediationPayloadV1`, sealed by `payload_sha256` with identity `rem_<64hex>`:

```text
record_kind, proposal_kind, source_issue_ids, canonical_proposal_text,
effect, intervention_target, authority_class, effect_boundary, user_evidence?, honesty_limit,
admission_policy_version, redaction_contract_version,
semantic_validator_receipt_id?
```

`proposal_kind`, `effect` and `intervention_target` remain orthogonal. Every proposal kind is
reachable, including `routing_recommendation`.

Enforced gates: `MissingUserEvidenceForTasteCandidate`, and `PrecisionGateNotMet` at
`PRECISION_GATE_THRESHOLD = 0.95` measured on the family's labelled corpus before a proposal may
surface as actionable.

## 2.2 Why target stays separate

`proposal_kind` names proposal semantics. `effect` names change class. `authority_class` and
`effect_boundary` carry authority and blast radius. `intervention_target` answers:

> **What artifact or surface is supposed to change?**

One `ProcessChange` can mean "rewrite a skill", "shorten a tool description" or "stop spawning a
subagent here". Three artifacts, three owners, three measurements — indistinguishable while they
share an effect class, and therefore impossible to evaluate independently.

Every proposal kind, including `routing_recommendation`, must remain reachable without collapsing
target, effect, authority or blast radius into one field.

## 2.3 Target vocabulary

Keep `intervention_target` orthogonal in the sealed payload. `RemediationEffect` keeps its meaning;
`intervention_target` names the surface.

```text
intervention_target:
  model_behavior_policy    skill_or_procedure      system_prompt
  tool_description         tool_implementation     routing_policy
  context_retrieval        context_reduction       orchestration
  guard                    evaluator               documentation_policy
```

`TasteCandidate` is deliberately absent. Every entry above answers "what surface changes?".
`TasteCandidate` answers "what semantic proposal type might result?" and keeps its status as a
special effect class behind its own user-evidence gate. Listing it as a target would let a user
preference masquerade as an optimisation objective.

One issue MAY produce several proposals with different targets. That is the point — they are
measured separately.

```text
Issue: verification_claim_without_evidence

  guard                  → deterministic completion-receipt check
  model_behavior_policy  → require explicit evidence before completion language
  evaluator              → completion-integrity evaluator
  tool_description       → clarify verification semantics
```

## 2.4 Constraints that survive the extension

- `authority_class` stays `none`. A target is not a permission.
- `effect_boundary` stays explicit and separate.
- `payload_sha256` sealing and deterministic `rem_<64hex>` identity are unchanged. Adding a payload
  field changes the digest basis, so the schema version increments.
- The `TasteCandidate` user-evidence gate and the 0.95 precision gate are untouched.
- `source_issue_ids` is provenance only, never an authority grant.

---

# 3. Adapt — procedural asset effectiveness

If Adapt proposes a skill or procedure change, we must know whether that asset does anything.
Measuring an asset must not become owning it.

```text
ProceduralAssetObservationV1     host        mechanical facts (capability H4)
ProceduralAssetEffectivenessV1   Adapt       derived effectiveness assessment
ProceduralAssetLifecycle         asset owner active | stale | archived | pinned
```

## 3.1 Adapt's record

```text
ProceduralAssetEffectivenessV1
  asset_id, assessed_at
  source_issue_id?, source_proposal_id?
  exposures, selections, applications          # from host observations
  successes, failures, corrections_after_use
  token_cost_per_turn, model, client
  effectiveness_verdict + evidence_refs[]
  honesty_limit
```

No `state` field. Adapt does not decide whether an asset is active, pinned or archived.

## 3.2 What Adapt may propose

```text
archive_candidate
reactivate_candidate
consolidation_candidate
```

The owner acts on them. **The asset's own owner changes the asset.**

## 3.3 Lifecycle rules (owner-side)

- **Archive is reversible; delete is not automatic.** No automated path deletes an authored asset.
- **Time triggers review; it does not prove obsolescence.** An asset is archived because measured
  relevance or effectiveness disappeared, not because N days elapsed. Elapsed time may only
  schedule the review.
- **Pinned and referenced assets are protected**, including any asset a scheduled job depends on.
- **Telemetry is stored separately from authored content.** Usage counts never live inside the
  asset file.
- **Effectiveness is not authority.** A well-used skill does not thereby become a Taste record.

---

# 4. Tray-owned daemon — background review contract

All background Membrane learning is scheduled inside the tray-owned daemon. The visible native tray
owns resident lifecycle; the Hub dashboard is on demand and owns no worker. There is no hidden
service and no self-triggering learner. Scheduler, gating, proposal-only execution interface and H5
persistence now exist; semantic provider, cursor and foreground-memory inputs remain unwired.

```text
BackgroundReviewContract
  lifecycle owner: visible native tray
  execution owner: tray-owned daemon
  jobs: adapt_behavioral_review | cortex_semantic_dream | cortex_memory_candidate_extraction
  gates: time_elapsed AND activity_threshold
  concurrency: single-flight lock per job kind, retry-safe on failure
               (same shape as Blueprint's singleflight initial build + one retry,
                Blueprint canon §17.1.1)
  priority: foreground user work always pre-empts
  cancellation: bounded and observable
  cache: same-model review reuses the warm prompt cache;
         a different or cheaper model receives a compact digest instead
  budgets: per-turn input budget AND aggregate input budget
  capability: restricted tool surface
  output: proposals — never durable writes
  observability: emits a background-job observation (capability H5)
  config failure: FAIL CLOSED — an unreadable background-learning
                  configuration does not run the cost-incurring learner
  hub inactive: typed `hub_inactive`, never silence
```

The aggregate budget bounds post-turn review across repeated requests. Budget first, enable second;
an unreadable configuration fails closed.

Membrane background work lives only while the visible tray and its OS-coupled daemon are active.
The public V1 reason remains the literal `hub_inactive` until a versioned protocol change replaces
it. A period with no learning is reported, not omitted.

## 4.1 "Proposals only" applies to semantic learners, not to Cortex's own operations

State this explicitly or an implementer will conclude that deterministic Dream is now forbidden
from doing its job:

```text
Daemon background SEMANTIC learners
    → proposals only, no durable writes

Cortex DETERMINISTIC lifecycle / curation operations
    → may mutate through Cortex's own governed transition APIs
```

Determinism plus a governed transition API is what makes the second line safe. A model in the loop
removes both.

---

# 5. Cortex — two-stage Dream

`engine/crates/cortex-core/src/dream.rs` is deterministic native Rust today: same-scope duplicate
consolidation, stable primary ID and scope preserved, merged source IDs and keywords,
relative-date normalisation, duplicate secondary removal, low-score quarantine — and it explicitly
refuses to attribute itself to an LLM (`DreamPolicy::restricted`, guarded by
`deterministic_dream_policy_does_not_claim_an_llm_model`). That is correct and stays as the safe
first layer.

```text
CORTEX DREAM

Stage 0 — deterministic  (production-reachable; qualification not revalidated here)
  duplicate consolidation
  date normalisation
  exact lifecycle rules
  low-value quarantine
  stale detection
        → mutates via Cortex governed transition APIs (§4.1)

Stage 1 — semantic curation proposal  (contract implemented; production runner unwired)
  contradictions
  near-duplicates
  supersession candidates
  stale semantic assumptions
  merge proposals
  split proposals
  usefulness / lifecycle review
        → proposals only
        ▼
  Cortex admission / review
```

Stage 1 never rewrites durable truth directly.

## 5.1 Memory candidate extraction

Separate from Dream, scoped to the current session:

```text
MemoryCandidateExtraction
  cursor over new session events only — no full-session reprocessing
  skipped entirely when an authoritative foreground memory emission already exists
  hard token / time / request cap
  warm-cache-aware background execution
  provenance-bound candidate output
  no direct write; Cortex admission is the only path
```

The skip rule matters: if the foreground agent already wrote memory, the background extractor must
not duplicate it.

This belongs to **Cortex, not Adapt**. Memory is not a behavioural learning object.

---

# 6. Learning Lineage — a read model

Every learned item needs a traceable history. Not another store: a projection over receipts that
already exist.

```text
experience
  ↓
episode / Taste evidence
  ↓
Insight / Taste
  ↓
InterventionProposal
  ↓
variant / experiment            (host-side, capability H7)
  ↓
deployment                      (host-side)
  ↓
outcome
```

For any learned item the user must be able to answer: where did this come from, what did it change,
what evaluation qualified it, is it active, did it actually help, can I undo it?

Every step emits a typed receipt already — sealed issue ids, `rem_<64hex>` proposal ids, experiment
receipts, admission receipts, base-artifact digests. Lineage is a join over them, not new data.
That is what makes this stronger than an agent that can only show you the memory it wrote.

---

# 7. The insights surface is a projection, not a store

A view over work, models, tools, context, Taste, Insights, skills and recommendations is worth
building, as a **read model** over host telemetry + Adapt + Cortex.

It is not a new product. It is a panel in the Hub dashboard window and inherits that design system
rather than inventing one.

- Its prose is never stored in Cortex as truth. It is regenerated from sources.
- It creates no new semantic object. Adapt Insights keeps its identity: **failure / gotcha / waste
  learning.**
- **Positive performance does not become an Insight.** "What worked well" is host outcome and
  evaluation evidence read through Adapt effectiveness measurement. That still answers "this
  routing policy produces fewer architecture-churn episodes" without inventing positive Insights.
- Recommendations rendered are sealed proposals, not advice authored at render time.
- Learning Lineage (§6) is a view within this surface, not a separate product.

## 7.1 It renders in the admission-ledger form

Hub's workspace signature is `subject · verdict · evidence · observed`, with the verdict carried by
a shape **plus** a word so it survives greyscale and colour-vision deficiency, and the evidence
column printing the typed reason verbatim.

Every row takes that form: an Insight, a proposal, an asset effectiveness verdict and a coverage
gap alike.

This is not decoration. It is the honesty contract expressed in the UI: an unmeasured thing prints
`not_instrumented` or `hub_inactive`, never a blank cell, a zero, or a paraphrase. A dashboard that
renders an unknown as `0` has laundered a missing observation into a fact.

---

# 8. Pull — qualify RRF against fixed-order control

## 8.1 Final contract

Ledger lexical/BM25 ranks, Cortex vector/lexical ranks, Blueprint structural confidence and exact
anchors are not score-calibrated against each other. Fixed-order control and RRF therefore remain
separate named production strategies with explicit parameters and receipts.

Required active-path contract:

- one explicit, versioned strategy identity rather than a second synonym if `policy` is retained;
- `FusionReceiptV1` records provider/lane rank, strategy identity, fused rank,
  duplicate/fusion decisions, and budget drops;
- production federation emits that receipt;
- any comparison mechanism remains distinct from `membrane-federation/src/shadow.rs` (§1.2).

## 8.2 Qualification and activation

Canonical doctrine names RRF as the standing deterministic baseline. Fixed provider/security
ordering remains active control until a measured cutover:

```text
1  freeze current production fixed-order behavior as control
2  define representative multi-provider tasks
3  execute RRF under its versioned policy identity
4  compare paired task and retrieval metrics
5  include diversity/duplicate and required-evidence coverage
6  freeze k and any other parameters on the development corpus
7  run held-out corpus once
8  activate canonical RRF only if production-path qualification passes
```

Held-out tuning is forbidden (canon §13.1). Report uncertainty, not only point estimates.

---

# 9. Pull — corrective retrieval after insufficiency

Retrieval should be evaluated, and a poor first retrieval should be able to trigger a bounded
corrective action. Production federation already emits typed provider omissions and the planner
records admission/budget decisions. Federation can now evaluate an explicit planner-supplied
sufficiency contract and plan one corrective stage. Resident transport forwards that contract and
receipt. A first-party planner caller and a distinct corrective action still have to be connected;
repeating the same request against the same provider is forbidden.

```text
initial acquisition
   ↓
hard eligibility
   ↓
fusion
   ↓
sufficiency / coverage check
   ├─ sufficient   → publish
   └─ insufficient
        ↓
   bounded corrective action
        ├─ deterministic query expansion / reformulation
        ├─ alternate provider or lane
        ├─ deeper source-bound expansion
        └─ explicit unknown
```

Rules:

- never exceed deadline or max cost silently;
- record why the re-query happened, in the receipt;
- cap stages and marginal work;
- no unbounded loop; a corrective pass that fails publishes the insufficiency, it does not retry.

The corrective *actions* above are deterministic. A model-proposed reformulation is an optional
extension specified in `MEMBRANE-SEMANTIC-ADVISOR-EXPERIMENTAL.md` and MUST NOT be a prerequisite
for this section. This section is the mechanism; the advisor is one possible source of one input
to it.

---

# 10. Push — qualify query-aware reduction, then evaluate ordering

## 10.1 Final contract

Production exposes control and query-aware policies explicitly, carries planner-supplied
task/evidence metadata, and keeps control reachable as the default arm.

Push's boundary is unchanged: it executes planner-selected faithful transformations. It does not
rank evidence, decide attention, invent missing evidence, or become a summarization planner.

Behaviour to preserve and verify:

- protect query/task entities;
- protect exact identifiers, errors, tests and constraints;
- preserve exact recovery handles;
- fall back to less reduction on uncertainty.

## 10.2 Qualify it

```text
raw control
structural query-agnostic Push
query-aware Push
```

at matched attention budgets, measuring task correctness, required-evidence retention,
protected-span integrity, token reduction, latency, resolver restores and user corrections.
`docs/evidence/qualification/push-metrics.json` currently names metrics and reports
`fixture-entrypoint-defined`; it is not measured baseline evidence. First freeze measured control
results from the active production path, then require query-aware Push to beat or match them.

## 10.3 Ordering policy evaluation

Long-context models use evidence differently depending on position. This does not make Push the
ranking owner. Pull and the final renderer evaluate ordering policies:

- highest-authority and required evidence early;
- critical constraints early with a final recap or reference late;
- grouped by evidence dimension;
- baseline fused order.

Push preserves the selected order unless the planner explicitly chooses a representation or order
policy.

---

# 11. Packet reduction plan — fitting to real host capacity

A host computes its own remaining rendered-context ceiling and passes it in. Between that
computation and serialization the host's own state can change, so the admitted packet may no longer
fit.

The wrong answers are: the host silently drops admitted blocks (a second planner), or Membrane runs
a whole new context trace on the critical path (an expensive round trip).

Membrane publishes enough structure for **mechanical shrinkage without delegating judgement**:

```text
PacketReductionPlanV1
  estimator_basis
  representations:
    full       → tokens
    reduced_1  → tokens
    reduced_2  → tokens
    floor      → tokens
  protected[]                  # never absent from any representation
  minimum_viable_tokens
  coverage_note_per_representation
```

Representations are the unit rather than individual items, because arbitrary item-by-item dropping
breaks requirement coverage. Each representation is Membrane-authored and internally coherent.

> **The host chooses the largest Membrane-authored representation that fits. It never chooses which
> evidence matters.**

```text
Membrane   decides reduction semantics
host       applies its own capacity
```

Every representation retains parent and evidence refs and exact resolver paths. If protected
material is absent from a representation, that representation is invalid and is not published.

---

# 12. Host seam contract — what Membrane requires and emits

Membrane functions with any harness that satisfies these capabilities. A capability that is absent
produces a typed degradation, never silence and never a fallback authority.

## 12.1 Required host capabilities

| id | Capability | Membrane consumer | Absent → |
|---|---|---|---|
| H1 | stable session / task / trace / span / model-call / tool-call / artifact identity | receipt binding | `not_instrumented` |
| H2 | normalized transcript events on the transcript adapter | Adapt detectors | degraded episode fidelity |
| H3 | explicit selected-transcript references (source, hash, span) | Adapt review boundary | no user-selected evidence |
| H4 | structured execution observations: session mechanics, per-tool and per-asset cost, completion emissions | Adapt mechanical facts | Adapt may not infer them from prose |
| H5 | background-job observations for daemon-scheduled learners | §4 observability | learner stays disabled |
| H6 | evaluation outcomes: dataset, case, evaluator, score, experiment | Adapt effectiveness | no effectiveness verdict |
| H7 | variant generation, experiment execution and deployment for approved proposals | §6 lineage tail | proposals stay unqualified |
| H8 | true remaining rendered-context ceiling at request time | §11 admission | Membrane budgets blind |
| H9 | already-loaded context identities, updated after host compaction | Native delivery lane | Native lane must not be used |
| H10 | packet-delivery acknowledgement after host serialization | delivery proof | delivery unverified |

Every observation crossing this seam obeys four invariants, which Membrane validates on ingest:

- **P1 — no semantic pre-labelling.** A host record may not assert an Adapt category
  (`preference`, `insight`, `issue`, `taste`). Hosts emit observations, outcomes and measurements;
  Adapt assigns meaning.
- **P2 — no admission bypass.** Nothing becomes durable Membrane truth by being emitted. It crosses
  Cortex admission or it stays host telemetry.
- **P3 — provenance or silence, per field.** Every record carries a provenance receipt and every
  non-exact field carries its own coverage marker. Absent evidence is `unavailable`, never zero.
- **P4 — unavailability is typed and survives to the surface.** An unobserved field carries a typed
  reason (`not_instrumented`, `hub_inactive`, `provider_omitted`, `host_unsupported`), never a
  paraphrase and never a blank.

Membrane rejects at the seam rather than admitting at low confidence.

## 12.2 What Membrane emits

| Output | Producer | Host use |
|---|---|---|
| `SealedInsightIssueV1` | Adapt | dataset seeds, regression cases, guard targets |
| `SealedRemediationProposalV1` + `intervention_target` (§2) | Adapt | variant generation input |
| Evaluator proposal (`ToolingFix` effect) | Adapt | evaluator to implement and run |
| Regression-case proposal | Adapt | promotion into the host eval store |
| Context packet + receipt | Pull | what the model actually saw |
| `PacketReductionPlanV1` (§11) | Pull | fit to real host capacity |
| Reduction artifact + receipt | Push | context-policy variant evidence |
| Cortex admitted record | Cortex | durable behaviour the harness must respect |
| Taste preference record | Adapt → Cortex | routing / presentation constraint, never a target to optimise away |
| `ProceduralAssetEffectivenessV1` (§3) | Adapt | asset review input, not an owner decision |

## 12.3 Ownership lines that do not move

- Ledger indexes document-shaped sources. It is never a raw event store; transcript events reach it
  only as a qualified session-document projection through the virtual-source contract.
- A host may keep its own operational trace and eval database. That is its canonical domain. It may
  not create a second durable semantic-memory universe beside Cortex, a second document index
  beside Ledger for the same function, or a second repository graph beside Blueprint.
- Adapt emits proposals. Approval and deployment are host acts behind their own gates.
- Membrane never opens host storage; the host never opens Cortex durable storage.

---

# 13. Closed integration designs

These interfaces close remaining architecture choices. Implementations may change internal call
sites, but may not change ownership, failure behavior, or wire meaning without revising this
section first.

## 13.1 First-party corrective retrieval

`membrane_context` accepts an optional planner-authored `SufficiencyContractV1` and transports it
unchanged to `/federate`. Membrane never invents requirements from task prose. When absent,
sufficiency remains `not_evaluated`.

After initial merge, federation evaluates the contract. If insufficient, it executes exactly one
**alternate-provider-lane** action through the existing provider interface:

```text
initial outputs + SufficiencyContractV1
  → evaluate_sufficiency
  → choose one acceptable active provider not used as trigger
  → run that provider once under remaining request deadline and budget
  → append output, re-merge, re-evaluate
  → publish CorrectiveRetrievalReceiptV1
```

Provider preference is deterministic. The corrective stage preserves original request identity,
scope grant, authority, cancellation and deadline. It may repeat the request against a different
provider; it may never repeat against the trigger provider. Missing alternate provider, exhausted
deadline, provider failure or second insufficiency publishes typed insufficiency and stops. No
query-rewrite contract or provider-trait extension is required.

## 13.2 Background semantic execution and Cortex Stage 1

The tray-daemon command channel remains lifecycle-only. Semantic work uses one authenticated,
host-neutral loopback provider seam owned by the daemon runtime:

```text
BackgroundSemanticReviewRequestV1
  job_id, job_kind, session_id, task_id?, turn_id
  cursor + bounded new session events with provenance
  foreground_memory_state:
    unavailable | available_no_emission | available_emission(range)
  per-turn and aggregate budget remainder
  deadline, restricted_capabilities

BackgroundSemanticReviewResultV1
  matching job and request identity
  curation_proposals[] | memory_candidates[]
  next_cursor?
  model/provider identity and measured usage when available
  provenance receipt
  status: proposals | blocked(reason) | failed(reason)
```

The provider returns untrusted proposal material only. The daemon validates Cortex semantic
curation output as `SemanticCurationProposalV1` and extraction output as `MemoryCandidateV1`, then
passes valid results to Cortex admission/review. It converts accepted proposal identities into
content-free `BackgroundReviewProposalRefV1` observations. No semantic result writes durable truth
directly.

The daemon reads only events after the stored cursor. It advances the cursor only after a valid
result reaches the proposal sink. `available_no_emission` permits extraction;
`available_emission(range)` skips overlapping work; `unavailable` fails closed with the existing
typed reason. Missing provider, cursor, proposal sink or foreground signal remains observable. One
provider seam serves Adapt review, Cortex semantic Dream and memory extraction; no second model
stack is created.

## 13.3 Request-time H8 and packet selection

The first-party host creates `RemainingContextCeilingV1` immediately before each Membrane context
request and sends it as `remainingContextCeiling` on that same `/federate` request. Membrane binds
session/task identity, validates provenance and exact coverage, and refuses mismatched estimator
basis. A cached or next-request ceiling is not request-time H8.

For the finalized admitted packet, Membrane publishes a coherent reduction ladder:

```text
full       emitted packet
reduced_1  planner-selected faithful Push reduction using same task/evidence metadata
floor      protected material + exact evidence/resolver refs only
```

Additional reductions are optional; arbitrary item dropping is forbidden. Every representation
uses the H8 estimator basis, preserves plan-wide protected material, and carries parent/evidence
refs, resolver paths, token count and coverage note. Membrane applies
`PacketReductionPlanV1::select_for_capacity` during the same request and returns plan, selected
representation and selection receipt. The host serializes exactly that complete representation or
reports typed delivery failure; it never edits membership. Unavailable/partial H8, basis mismatch,
no viable floor or changed capacity fails typed and never becomes a guessed budget.

## 13.4 Remaining dependency constraints

Only these surviving gaps remain schedulable:

- add a first-party planner caller for runtime sufficiency requirements, execute one distinct
  bounded corrective action, and qualify it;
- freeze measured fixed-order and Push controls, then qualify RRF and query-aware reduction;
- refresh the first-party H8 candidate at request time and deliver it directly into
  `PacketReductionPlanV1` selection;
- wire the daemon to real background semantic inputs and proposal execution;
- connect Cortex Stage 1 to model output and an authoritative foreground-memory signal;
- wire caller transport for joinable H4/H6 observations, then implement procedural-asset effectiveness;
- provide H7, H9 and H10 host observations before outcome, deployment and closed-loop claims;
- preserve typed unavailability for every absent source field.

These dependency constraints still apply:

- wire active-path fusion identity and receipts before retrieval-policy experiments;
- compare RRF against active fixed-order control before cutover;
- freeze a measured Push control before qualifying query-aware Push;
- establish background-job budgets, fail-closed configuration, cancellation, and observability
  before enabling any semantic learner;
- make remediation proposal production reachability explicit before extending its payload;
- establish target/effect identity before procedural-effectiveness or lineage joins;
- define `PacketReductionPlanV1` before a host performs mechanical packet shrinkage;
- require host capabilities H4 and H6 before asset-effectiveness or evaluation loops;
- run closed-loop qualification only after preceding production paths emit joinable receipts.

The insights panel (§7) is not a milestone. It renders only production-backed projections already
available from audited sources.

Evaluation hygiene for every qualification step is governed by cross-subsystem canon §13: mechanics
fixtures, a development corpus, a frozen held-out corpus run once, and production-path operational
proof. Synthetic fixtures prove mechanics; they do not support product-quality claims.

---

# 14. Required tests

## Adapt

- A proposal cannot be admitted with `authority_class` other than `none`.
- `taste_candidate` without separate qualifying user evidence is rejected.
- `intervention_target` does not accept `taste_candidate`.
- Every `RemediationEffect` maps to a reachable `proposal_kind`, and every enum member of
  `proposal_kind` is reachable — the test that would have caught `routing_recommendation`.
- Adding `intervention_target` changes the sealed digest basis and increments the schema version.

## Background learning and Cortex

- A background **semantic** job cannot write a durable Cortex record; only admission can.
- A Cortex **deterministic** lifecycle operation *can* mutate through its governed transition API,
  covered by a test that would fail if §4.1 were collapsed.
- An unreadable background-learning config fails closed: the learner does not run and the refusal
  is observable.
- Stage 1 Dream output is always proposal-shaped, never a mutation.
- Memory candidate extraction is skipped when a foreground memory emission exists for the same
  cursor range.
- A background job exceeding its aggregate budget is still observable, with the exhaustion recorded.

## Pull and Push

- The production path executes the query-aware provider when the policy selects it; the test fails
  if the provider becomes unreachable again.
- A protected span absent from a published representation fails publication.
- `FusionReceiptV1` records strategy id and version; a fused result without them fails validation.
- An insufficiency triggers at most the declared number of corrective stages, and the receipt
  records why.
- Every `PacketReductionPlanV1` representation contains every protected item, and
  `minimum_viable_tokens` is achievable.

## Seam

- A host record asserting an Adapt category is rejected at the seam.
- An unavailable value never serializes as `0`; coverage `unavailable` carries no value.
- Coverage other than `complete` without a typed reason fails validation.
- Two token estimates with different estimator bases cannot be summed or compared; the attempt is a
  validation error.
- Ledger cannot ingest a raw transcript event; only a qualified session-document projection.
- A packet delivered without a host acknowledgement is reported as unverified, not as delivered.

## Surface

- A window with tray/daemon inactive renders the public V1 reason `hub_inactive`, not an empty or
  zeroed panel.
- Learning Lineage renders entirely from existing receipts; it has no writable store.

---

# 15. Open questions

Genuinely unresolved. Everything else in this document is a decision.

1. Whether `PacketReductionPlanV1` should express representations only, or also permit
   coverage-bundle drops within a representation. Resolve with measurement once real packets exist.
2. Whether ordering policy (§10.3) belongs to Pull or to the final renderer as a separate published
   policy id.
3. What the activity threshold in §4 should be, expressed in observable units rather than elapsed time.
4. Whether the doctrine status sweep (§0.2) produces rows in this ledger or a separate per-doctrine
   status file.
