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
| Corrective retrieval after insufficiency | bounded planner-owned sufficiency contract, evaluator and one-stage alternate-lane plan exist and execute inside `FederationEngine::federate` | resident runtime forwards an explicit caller-supplied contract and exposes its receipt; no first-party caller supplies one — the `membrane_context` tool schema (`additionalProperties: false`) rejects a `sufficiencyContract` field | `corrective-retrieval.json` freezes production-path mechanics qualification (`mechanics-qualified-no-promotion`); no task-success evidence | `PARTIAL`: add planner caller, then qualify for effect |
| Query-aware Push | query-aware provider plus explicit control/query-aware policy | production `push prep` reaches either policy; control remains default | protected-span and reachability tests pass; no frozen measured baseline | `IMPLEMENTED + REACHABLE`; not qualified |
| Push qualification baseline | `docs/evidence/qualification/push-metrics.json` freezes measured mechanics results (required-evidence retention, protected-span integrity, budget compliance, query-aware reachability) as `mechanics-qualified-no-promotion` | task correctness, latency, resolver restores and corrections remain typed `unavailable` pending host instrumentation | mechanics measured; no comparative baseline that authorizes default-policy promotion | `NOT QUALIFIED FOR PROMOTION` |
| Deterministic Cortex Dream | `store::dream_now_observed` invokes restricted policy plus deterministic consolidation | CLI `dream_now` and resident `dream_now_observed` call the store path | not revalidated in this consolidation pass | `REACHABLE`; no `LANDED` claim |
| Sealed remediation proposals and taste gate | sealing, effect mapping, user-evidence and precision gates | production Adapt mine path emits deterministic sealed review proposals | focused unit and current-contract tests pass | `IMPLEMENTED + REACHABLE` |
| `intervention_target` / `routing_recommendation` | orthogonal target and reachable proposal-kind contracts | production Adapt mine path seals both fields into proposal identity | schema/digest/reachability tests pass | `IMPLEMENTED + REACHABLE` |
| `InterventionAttributionV1` | sealed attribution identity, support/alternative-cause gates, mutation eligibility and stale-surface invalidation are implemented | remediation construction requires eligible attribution for mutable instruction surfaces before producing a consumable proposal | focused Adapt tests cover every eligibility exclusion, deterministic identity, stale digest and consumability fence | `IMPLEMENTED + REACHABLE`; H7 effectiveness remains unavailable |
| Background review mechanics | fail-closed config, host activity/token gate, bounded jobs, proposal-only learner interface, cancellation, retry, single-flight, typed observations and bounded H5 sink | tray-owned daemon initializes the scheduler and persists H5 receipts, but supplies no real semantic provider, cursor, foreground-memory signal or proposal sink | focused lifecycle, budget, receipt and persistence tests pass | `PARTIAL`: production semantic inputs remain absent |
| Cortex Stage 1 | proposal-only review and memory-candidate extraction contracts | no production runner supplies model output or authoritative foreground-memory signal | focused validation tests pass | `UNWIRED` |
| Learning Lineage and Insights projection | read-only receipt graph and projection with typed unavailable joins | production Adapt mine response emits both | deterministic projection tests pass | `IMPLEMENTED + REACHABLE`; host tail remains unavailable |
| `PacketReductionPlanV1` | validated versioned plan, estimator basis and largest-fit selection | production `push select` accepts an H8 ceiling and returns only a complete published representation; first-party host emits a next-request H8 candidate from real policy and last observed usage | protocol/runtime/host-producer tests pass | `PARTIAL`: request-time refresh and direct host-to-Membrane delivery remain unwired |
| Host observation seam | H4/H6/H8/H9/H10 versioned shapes, provenance and closed-shape validation | H4/H6 producers and an H8 candidate producer exist in first-party host; bounded Membrane H4/H6/H8 parser and exact-ID H4/H6 lineage join exist; caller transport remains unwired; H9/H10 have no producer | protocol, parser and host producer tests pass where inputs exist | `PARTIAL` |
| Native-path authorization | Rust `AuthorizationGateV1` implements installation, scope-chain, caller/target, monotone authority, cross-root and validity/revocation gates | Hub-resident executor invokes it before every non-diagnostic repository-scoped native read/write | focused native tests cover denials and authorized self access; no cross-language conformance fixture proves Rust/JS parity | `IMPLEMENTED + REACHABLE`; diagnostics carve-out remains; §15 |
| Approved-proposal promotion | native review implements named `pending → approved/rejected` transitions and governed Cortex admission | native review binds repository/scope, persists one terminal decision and admits only approved payloads | focused native review and Cortex lifecycle tests cover terminal decisions, replay, novel admission and duplicate resolution | `IMPLEMENTED + REACHABLE`; no effectiveness/promotion claim; §16.1 |
| Cortex write-time duplicate/conflict detection | `AdmissionDispositionV1` and deterministic scope-bounded pre-filter execute in one immediate transaction | every durable write reaches admission; duplicates return existing identity, evidence-bearing repeats update metadata only and conflicts persist outside active recall | focused lifecycle tests cover duplicate, conflict, same-id update, short-content specificity and typed receipts; cross-process contention still lacks a dedicated fixture | `IMPLEMENTED + REACHABLE`; no qualification/policy-promotion claim; §16.3 |
| Cortex erasure and backup/restore | governed hard erase, digest-sealed backup/restore, registry reload and conflict-aware quarantine restoration are implemented | store operations remove payload-bearing projections, retain a content-free erase event and restore recall transactionally | focused lifecycle tests cover payload erasure, absent ids, quarantine restore, tamper refusal and recall equivalence; explicit link-erasure coverage remains absent | `IMPLEMENTED`; external operational exposure remains pending; §16.4 |
| Pull publication fence | scope-grant checks execute at acquisition | no grant/policy re-validation executes before packet emission | none | `GAP ON TRACED PATH`; §17.2 |
| Typed retrieval abstention | versioned `InsufficientConfidenceV1` carries typed reason and per-lane searched counts | active federation emits it whenever no candidate survives admission | focused federation tests cover no-answer and answered paths; frozen no-answer qualification baseline has not been rerun | `IMPLEMENTED + REACHABLE`; §17.1 |
| Ledger section identity | section body span-hash is canonical while slug/ordinal remain aliases | production outline reads resolve hash and aliases to the same indexed section | focused outline/index tests cover alias/fingerprint equivalence and duplicate-heading disambiguation; frozen retrieval qualification has not been rerun | `IMPLEMENTED + REACHABLE`; §18 |
| Installed-runtime guarantees | Windows job-object coupling remains landed; macOS tray implements launchd coupling; shared JS SQLite opens match native posture | JS stores use the shared opener; macOS code refuses daemon startup when coupling cannot install | JS 194/194 and macOS Swift 7/7 pass; full macOS host behavior is not yet qualified; no RightKit-owned required tag gate exists | `PARTIAL`; §19 |

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

# 2. Adapt — `intervention_target` and intervention attribution

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

## 2.5 `InterventionAttributionV1` — the gate between issue and proposal

Implemented in `membrane-adapt::attribution` and consumed by remediation construction. Attribution
identity, support, alternative-cause, instruction-state, counterfactual, stale-surface and
mutation-eligibility rules execute deterministically; mutable instruction-surface proposals
without eligible attribution are not consumable. This closes mechanics, not H7 effectiveness.

Semantic authority: Adapt canon §6.9. `intervention_target` names the surface; attribution answers
why changing that surface would have prevented the observed failures. Nothing in the sealed
proposal currently answers that; a proposal for a mutable instruction surface is not actionable
until it does.

```text
InterventionAttributionV1                        sealed; identity att_<64hex>
  attribution_id
  source_issue_id
  candidate_target                               # InterventionTarget enum member
  owning_surface_ref?                            # artifact identity where the surface is an artifact
  current_surface_digest?                        # exact digest of the surface version examined
  instruction_state
    missing | wrong | underspecified | already_correct | not_applicable
  counterfactual_preventability
    supported | unsupported | unknown
  alternative_causes[]
    none | model_variance | routing_failure | infrastructure | product |
    tool_implementation | evaluator_error | insufficient_evidence
  support
    episode_count, independent_session_count, severity, recurrence_rate
    # each with its own coverage marker; absent evidence is unavailable, never zero
  activation_evidence_refs[]                     # H4 asset-activation observations (§2.6)
  evaluator_outcome_refs[]                       # H6; tri-state applicability (§2.7)
  mutation_eligible                              # derived deterministically from the gates below
  ineligibility_reason?                          # typed
  honesty_limit
  attribution_policy_version
```

Deterministic eligibility gates (`mutation_eligible = true` requires all):

1. `counterfactual_preventability = supported`, bound to episode evidence and
   `current_surface_digest`;
2. `instruction_state ∈ { missing, wrong, underspecified }` for instruction-surface targets —
   `already_correct` is ineligible for that target (a guard or evaluator target gets its own
   attribution);
3. no dominant alternative cause; `insufficient_evidence` in `alternative_causes[]` is ineligible;
4. `support` meets the family's independent-session threshold;
5. the proposed change alters the surface's behavioral contract — redundant restatement or hedging
   is rejected at review.

Constraints:

- Attribution is proposal-class (canon §3.4): a model may draft it; deterministic code binds and
  gates it. It grants no authority and bypasses no existing proposal, review, precision, or
  admission gate.
- A `SealedRemediationProposalV1` whose target is a mutable instruction surface
  (`skill_or_procedure`, `system_prompt`, `tool_description`, `documentation_policy`) MUST
  reference a `mutation_eligible` attribution before variant generation (H7) may consume it.
  Additive `guard` and `evaluator` targets are not blocked by attribution, only informed by it.
- A stale `current_surface_digest` invalidates the attribution the same way
  `base_artifact_digest` staleness invalidates a variant: rebase and re-derive.

## 2.6 Asset-activation evidence (H4 extension)

For `skill_or_procedure` discrimination, H4 observations include per-asset mechanical activation
stages the harness knows directly:

```text
AssetActivationObservationV1                     # host-emitted, mechanical only
  asset_id, session_id, turn_id?
  discovered                                     # present in the host registry
  trigger_evaluated, trigger_matched             # router/trigger decision as it happened
  selected, load_result
  in_context_turn_ids[]                          # from context receipts
  invoked?                                       # where the asset is invocable
  provenance_receipt
```

The host never emits `rule_relevant` or `rule_followed` — those are Adapt semantic assessments
(invariant P1). The discrimination ladder over these facts is canon §6.9.

## 2.7 Evaluator applicability is three-valued

Evaluator outcomes joined into `support` carry
`applicable | not_applicable | insufficient_evidence`. `insufficient_evidence` removes the
observation from the applicable denominator. Forcing every trajectory into a score would launder
"cannot judge" into a verdict, violating P3/P4.

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

Usage-derived signals are confounded lower bounds: an asset can influence an outcome through
mere presence in context without a recorded selection or application. Absence of recorded
application is therefore never, by itself, evidence of ineffectiveness; a negative
`effectiveness_verdict` requires applicable evaluator outcomes (§2.7), not just low usage counts.

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

## 4.2 Review-input selection under budget

The aggregate input budget makes episode selection a real decision: `adapt_behavioral_review`
cannot read every eligible event window, so what enters the bounded review determines what the
learner can ever propose. Left unspecified, selection degrades silently into recency order and the
budget is spent on the newest episodes rather than the most informative ones.

```text
ReviewInputSelectionV1
  candidate basis: cursor over new, unreviewed events only (§5.1 cursor discipline)
  scoring:  deterministic novelty/anomaly score per candidate episode against the
            already-reviewed episode baseline (e.g. nearest-neighbour distance over
            the existing episode index) — no model call decides what a model call
            will read
  selection: top-K by score within the per-run input budget; ties break by recency
  receipts:  every run records candidates_considered, selected[], and a typed skip
             reason for the remainder (budget_exhausted | below_novelty_floor);
             skipped-but-eligible episodes remain eligible for later runs
  rate:      shares §4 gates; selection never triggers a run by itself
```

Constraints:

- Selection is mechanical (invariant P1): the score orders candidates; it may not assert an Adapt
  category or pre-label an episode.
- A quiet period where nothing clears the novelty floor is reported as such (§7.1 honesty), never
  padded with low-value episodes to spend the budget.
- Disposition: pending specification only — no implementation claim. This section exists so the
  semantic-provider wiring (§0.1 background-review row) lands with a specified input policy
  instead of an implicit one.

---

# 5. Cortex — two-stage Dream

`engine/crates/cortex-core/src/dream.rs` is deterministic native Rust today: same-scope duplicate
consolidation, stable primary ID and scope preserved, merged source IDs and keywords,
relative-date normalisation, duplicate secondary removal, low-score quarantine — and it explicitly
refuses to attribute itself to an LLM (`DreamAgentPolicy::restricted`, guarded by
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
`docs/evidence/qualification/push-metrics.json` freezes measured mechanics results
(`mechanics-qualified-no-promotion`): retention, protected-span integrity, budget compliance and
query-aware reachability are measured on the production path; task correctness, latency, resolver
restores and corrections are typed `unavailable` pending host instrumentation. Mechanics evidence
does not authorize promotion. First freeze measured control results for the task-level metrics,
then require query-aware Push to beat or match them.

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
| H4 | structured execution observations: session mechanics, per-tool and per-asset cost, per-asset activation stages (§2.6), completion emissions | Adapt mechanical facts | Adapt may not infer them from prose |
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
| `InterventionAttributionV1` (§2.5) | Adapt | mutation-eligibility gate H7 must check before consuming a mutable-instruction-surface proposal |
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
- preserve typed unavailability for every absent source field;
- freeze one JS/Rust authorization conformance corpus and run it against both surfaces (§15);
- close the diagnostics authorization carve-out: `membrane_diagnostic_*` operations perform
  stateful writes (workspace open/reconcile, mutation begin/seal, baseline capture/update,
  provider restart) keyed to an envelope-supplied repository identity that bypasses the shared
  gate on both the JS surface and the native executor; bind them to verified installation
  identity or route them through the §15 gate;
- qualify same-machine ACL isolation for the local trust surface: the `api-token` bearer file
  (Unix mode 0600; Windows DACL unproven) and the per-user named pipes (remote clients rejected;
  same-machine user isolation not explicitly qualified);
- add a real post-fusion pre-publication grant/policy recheck owned by the grant source (§17.2);
- qualify macOS launchd lifetime coupling on-host and move the installed-artifact tag gate into
  RightKit-owned workflow generation (§19).

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

Two reviewed pending extensions feed this queue without replacing it. The semantic
compilation and Blueprint architecture-view proposals
(`docs/pending/semantic-blueprint-review-pack-v2/`, Fable review 2026-08-29: accept with
amendments) and the harness-efficiency Insights extension
(`docs/pending/ADAPT-HARNESS-EFFICIENCY-INSIGHTS.md`) become schedulable only as entries in this
list, after their own qualification prerequisites; their accepted decisions bind to existing
contracts here (§4/§13.2 daemon seam, §16.1 promotion, §16.2 temporal vocabulary, §16.3 dedup,
§17.1 abstention, §18 section identity).

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
- An attribution with `instruction_state = already_correct` cannot yield `mutation_eligible = true`
  for that instruction surface.
- An attribution with `counterfactual_preventability != supported` cannot yield
  `mutation_eligible = true`.
- A proposal targeting a mutable instruction surface without a referenced `mutation_eligible`
  attribution is not consumable by variant generation.
- A stale `current_surface_digest` invalidates the attribution; adoption on the stale digest fails.
- A host record asserting `rule_relevant` or `rule_followed` is rejected at the seam.
- An `insufficient_evidence` evaluator outcome never contributes to the applicable aggregate as
  success, failure, or zero.

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

## Security, durable lifecycle and operations

- A native-path request with an unauthorized or mismatched repository identity is denied by the
  shared authorization module with the failed gate named; the test fails if the native executor
  stops calling that module.
- An approved proposal either reaches Cortex admission through the wired consumer, or the
  `approved` state is unreachable in production — no third outcome.
- A near-duplicate durable write under a different id yields a `duplicate` or `conflict`
  disposition, never a second silent record.
- Quarantined rows restore transactionally; hard erase leaves no payload in any Cortex-owned
  projection; backup → wipe → restore proves recall equivalence.
- A no-answer query publishes typed `insufficient_confidence`, never below-floor hits.
- A grant or policy-epoch change between admission and emission publishes `policy_changed`; the
  stale-authorized packet is not emitted.

---

# 15. Security — native-path authorization

Native authorization executes before every non-diagnostic repository-scoped request. The Rust gate
verifies installation enrollment, scope chain, caller/target binding, monotone authority,
cross-root reach and validity/revocation before retrieval or admission. Bearer possession alone no
longer authorizes a declared repository identity.

```text
AuthorizationGateV1                     executes before any repository-scoped read or write
  order:   installation grant → repository scope chain → caller/target binding
           → authority level (monotone minimum) → cross-root denial
           → validity interval / revocation
  parity:  native Rust and JS execute the same ordered contract, but currently
           through separate implementations
  failure: typed authorization_denied naming the failed gate; never silent scope
           widening, never downgrade-and-continue
  timing:  the gate runs before retrieval scoring and before admission —
           unauthorized evidence never enters a candidate set to be labelled
           untrusted later
```

Constraints:

- Bearer transport authenticates the channel, never the scope. Self-declared identity is a claim
  to verify against the installation registry, not a grant.
- A frozen cross-language conformance corpus is still required. Separate implementations without
  shared fixtures can drift even when both local suites are green.
- `membrane_diagnostic_*` remains outside this gate on both surfaces and stays a live §13.4 gap.

---

# 16. Cortex — durable-truth lifecycle completion

## 16.1 Approved-proposal consumer

Implemented on the native review path. Review names an existing pending proposal, verifies
repository/scope ownership, permits one terminal `approved` or `rejected` transition and records
reviewer/time. Approval immediately invokes `MemoryStore::admit_approved_proposal`; novel payloads
enter Cortex through the governed admission path, while duplicate/conflict outcomes remain typed.
Admission failure restores the proposal to `pending` rather than stranding an approved row.

This closes production reachability only. It does not authorize automatic proposal approval or
claim that promoted proposals improve task outcomes; those remain review and qualification
questions.

## 16.2 Temporal validity and supersession

```text
TemporalValidityV1                      adopted target contract
  valid_at         authored/asserted time — never defaulted to ingest time
  recorded_at      ingest time, kept distinct
  invalid_at?      set when a conflicting admitted record supersedes this one
  superseded_by?   successor record identity
```

This contract consolidates temporal machinery that already partially exists rather than starting
greenfield: the durable `memories` table carries `effective_from_ms`, `effective_until_ms` and an
actively written `superseded_by` column, and `cortex-store/src/temporal.rs` implements a narrower
`TemporalFact` store with `valid_from`/`valid_until`/`supersedes` and as-of queries. Adoption means
unifying those onto this vocabulary, not adding a third scheme beside them.

Supersede, never delete: conflict resolution marks the losing record invalid rather than removing
it, point-in-time recall (`as_of`) stays answerable, and lineage is queryable per record.
Deterministic read-time conflict ordering: discard revoked → prefer valid-at-requested-time →
prefer higher authority → prefer independently verified → surface the unresolved conflict typed.
Silently blending conflicting records is forbidden.

## 16.3 Write-time duplicate and conflict detection

Implemented on the durable write path. Exact normalization, a specificity gate and bounded
4-shingle Jaccard scan run scope-locally inside the same immediate transaction as admission.
Distinct-id duplicates produce typed `no_op` receipts and return the existing active identity;
repeats carrying explicit new evidence refs produce `update_metadata_only`; ambiguous content
produces `conflict_quarantined`, persists the candidate outside active recall and emits a typed
receipt. Same-id lifecycle updates bypass the pre-filter. No model call participates in the
decision, and concurrent writers cannot both pass an unlocked scan.

Conflict-candidate restoration re-enters this same admission path. The legacy id-returning V1
boundary never returns a quarantined candidate as if it were active truth.

## 16.4 Erasure, quarantine and backup/restore

Store-layer implementation preserves reversible quarantine as the default destructive
path. Explicit `hard_erase` transactionally removes the named payload from active memory,
quarantine, link and tombstone projections plus the resident registry, while retaining a
content-free erase event. `backup_cortex` emits a
versioned digest-sealed envelope; `restore_cortex` rejects tampering, restores active and
quarantined rows plus links transactionally and reloads the registry. Focused acceptance proves
backup → governed wipe → restore → recall equivalence.

Operational exposure of hard erase and backup/restore remains pending. No UI, CLI or unattended
job may infer authority from these store methods; any future surface must add explicit authorization,
scope binding and irreversible-action confirmation without weakening quarantine-first behavior.

---

# 17. Pull — abstention and publication fence

## 17.1 Typed abstention

```text
InsufficientConfidenceV1
  status:            insufficient_confidence
  searched:          per-lane candidate counts (lexical, semantic, graph, doc)
  reason:            no_authorized_candidate_above_threshold | no_candidates | evidence_floor
  suggested_action?
```

Protocol and federation emit this instead of below-floor hits when nothing
clears the admission floor; consumed by the
sufficiency evaluator (§9) and rendered under the §7.1 honesty contract. This also closes the
measured no-answer gap in Ledger qualification: a no-answer query returns typed abstention, not
weak matches.

A string-only `insufficient_confidence` status already exists on the separate Cortex/Taste
memory-recall path. This contract does not reuse it implicitly: the Pull shape above is versioned
and typed; the Cortex recall path either adopts the same shape or keeps its status explicitly
distinct — two spellings of one meaning with different structures is forbidden.

## 17.2 Publication fence

Target contract: grant and policy state are re-validated immediately before packet emission. Grant
identity, policy epoch and revocation are checked once more after fusion completes; a change
publishes typed `policy_changed`, and a packet authorized under a superseded grant is never emitted.

Current code validates a caller-supplied `PublicationFenceV1` before provider execution and stamps a
held receipt into the response. That makes malformed/tripped receipts typed, but it is not the
required post-fusion recheck and cannot detect a policy change occurring during federation. This
section remains pending until the grant owner supplies or executes the second observation at the
publication boundary.

## 17.3 Calibrated fusion candidate

A per-query logistic calibration of dense scores fused with lexical logits may enter §8's frame
only as a third named, versioned strategy under the identical qualification ladder (frozen
development corpus, one held-out run, production-path proof). It never displaces the
non-calibration default without that evidence.

---

# 18. Routed subsystem work — Blueprint and Ledger

Spec authority for these lives in their own canons; this ledger records the decision and tracks
disposition only:

- **Ledger — implemented:** section reads accept the structural body span-hash while slug/ordinal
  anchors remain resolvable aliases. Frozen retrieval qualification still needs rerunning before
  any recall-improvement claim.
- **Blueprint**: adopted directions for the existing re-anchoring and incremental trains —
  string-based self-describing symbol identity; per-file precompute with query-time stitching;
  typed per-file staleness status; confidence-tiered edges; an enumerated cache-invalidation
  matrix.

---

# 19. Operations — installed-runtime guarantees

- **Implemented, awaiting macOS host qualification:** the tray installs a
  per-session launchd kill guarantee, publishes the supervised daemon PID and refuses startup when coupling cannot install.
  Qualification must prove crash, normal drain, PID cleanup and no-orphan behavior on macOS.
- **Still pending:** installed-artifact qualification must become a required release-candidate gate.
  Generated workflow must not be hand-edited; add the capability to RightKit ownership, then
  regenerate this repository.
- **Implemented and verified:** every JS-side shared-SQLite open uses the native WAL,
  `busy_timeout=5000`, `synchronous=NORMAL` and in-memory temp-store posture. Proposal,
  working-context and readback restart suites exercise that shared opener.

---

# 20. Absorption triage register

External prior-art survey (2026-08-29) triaged against this ledger. Adopted items are specified
above as Membrane contracts; sources are reference material only, never authority.

| Item (origin) | Disposition |
|---|---|
| Ordered pre-scoring authorization gate (predecessor archive) | adopted → §15 |
| Bitemporal validity / supersede-never-delete (graphiti; MemoryOS; codebase-graph) | adopted → §16.2 |
| Deterministic query-time conflict ordering (predecessor archive) | adopted → §16.2 |
| Entropy/MinHash write-time dedup pre-filter (graphiti; semantica; mnemon) | adopted → §16.3 |
| Reversible quarantine erasure (predecessor archive schema-v10) | adopted → §16.4 |
| Typed abstention contract (predecessor archive; superlocalmemory) | adopted → §17.1 |
| Per-query calibrated fusion (txtai LogOdds) | adopted as candidate strategy → §17.3 |
| Surprisal-sampled review-input selection (honcho) | adopted → §4.2 |
| Usage-signal bias caution (predecessor archive) | adopted → §3.1 |
| Symbol identity, partial-path stitching, typed staleness, edge confidence, cache-invalidation matrix (SCIP; stack-graphs; GitNexus; infigraph; dependency-cruiser) | routed → Blueprint canon (§18) |
| Section-identity unification, occurrence model (SCIP) | routed → Ledger canon (§18) |
| Pinned-commit retrieval eval corpora and metrics (octocode; sense; zep; cognee) | governed by cross-subsystem canon §13; adopt corpus format at each qualification step |
| Iterative local/global retrieval escalation (graphrag DRIFT) | rejected — §13.1 deliberately fixes one bounded alternate-provider action |
| Item-level knapsack context packing (lean-ctx) | rejected — representation-level fitting is the decided §11 contract |
| Execute-code-as-reduction (context-mode) | rejected — outside Push's faithful-reduction boundary |
| On-demand retrieve-back compression (headroom) | rejected as redundant — recovery markers and resolver refs already provide reversibility; the remaining gap is egress wiring (§0.1) |
| Per-type decay curves (mnemosyne; memory-lancedb) | deferred — no measured problem; revisit with Dream qualification evidence |
| Outcome-driven memory-ranking reweighting (caura) | deferred — feedback remains offline-only until H7 outcome joins exist |

---

# 21. Open questions

Genuinely unresolved. Everything else in this document is a decision.

1. Whether `PacketReductionPlanV1` should express representations only, or also permit
   coverage-bundle drops within a representation. Resolve with measurement once real packets exist.
2. Whether ordering policy (§10.3) belongs to Pull or to the final renderer as a separate published
   policy id.
3. What the activity threshold in §4 should be, expressed in observable units rather than elapsed time.
4. Whether the doctrine status sweep (§0.2) produces rows in this ledger or a separate per-doctrine
   status file.
