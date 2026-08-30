# Membrane — Semantic Context Advisor (experimental)

**Status:** experimental, optional, pending. Not canon.
**Scope:** bounded LLM semantic assistance inside Membrane's context plane.

## This capability is optional by construction

Everything in committed atom canons must work with this document deleted. Nothing
here is a prerequisite for those atoms. If the advisor is disabled, unavailable, times out,
returns malformed output, or exceeds budget, Membrane continues through the deterministic context
path and records the degradation in the receipt.

That is very different from Membrane itself being unavailable. A missing advisor is a quality
degradation. A missing mandatory Membrane binding remains a context-plane failure for clients that
require Membrane.

## Document authority

Subordinate to canonical architecture in `docs/current/architecture/`, & to
`docs/pending/README.md` for every deterministic mechanism it plugs into.

## Host neutrality

Same binding rule as the core pending document: this specifies Membrane's work and Membrane's side
of the seam as a host-neutral contract. Inference execution is a required host capability (`H11`,
§9), not a named host implementation.

## The central invariant

> **The model proposes; Membrane validates and decides.**

The LLM may improve **semantic judgment**. It may not establish **authority**.

## What this document replaces

`MEMBRANE-LLM-CONTEXT-PLANE-IMPLEMENTATION.md` (pending), including its §20 restatement of the
CodeRight boundary — see §12, which cites the seam canon instead of reproducing it.

---

# 0. Target-state status

This document defines an optional final shape. It makes no landed-state or rollout claim.

Phase C depends on deterministic sufficiency and corrective retrieval from core doc §9. It proposes
one bounded input to that mechanism; it never replaces or precedes the deterministic capability.
Production status for those dependencies belongs to the core document's call-chain audit.

---

# 1. Why an LLM here, and only here

Membrane already has strong deterministic mechanics for source identity, scope, provenance,
authority, freshness, exact anchors, retrieval, fusion, dedupe, budgeting, delivery lanes,
omissions and receipts. Those are the wrong places to introduce probabilistic authority.

The gap an LLM can close is **semantic interpretation across already-governed evidence**:

- a task is underspecified and several interpretations are plausible;
- a lexical query misses the intended concept;
- ten relevant candidates cover one requirement while another is uncovered;
- two sources appear contradictory although their identifiers differ;
- the deterministic first pass is insufficient and a bounded reformulation would likely recover it;
- a tight budget requires knowing which details are task-critical rather than merely similar;
- several candidates tie on deterministic rank inside the same authority and freshness class.

---

# 2. Default posture

This is a decision, not a tuning knob. The advisor is **corrective-first**.

```text
Phase A  request / evidence-requirement assistance
           OFF by default, or tiny/local model only

Phase B  candidate semantic assessment
           SHADOW by default

Phase C  corrective retrieval assistance
           ACTIVE when deterministic sufficiency fails
```

Rationale: Phase B on every ambiguous request puts a full model call on the critical path *before*
the root model call, turning a context optimisation into a latency tax on otherwise successful
requests. Phase C runs only after deterministic sufficiency already failed — where a second pass
was going to be paid for anyway.

Phase B is retained as an architectural capability and may graduate to active for specific cases
once evaluation demonstrates the economics: very large eligible candidate sets, very tight
attention budgets, cross-source conflict, an expensive root model, or a declared high-value task.

The resulting architecture:

```text
FAST PATH
  deterministic Membrane
        ↓
  sufficient? ──yes──► publish

CORRECTIVE PATH
  deterministic Membrane
        ↓
  insufficient
        ↓
  bounded semantic advice
        ↓
  deterministic validation
        ↓
  bounded corrective retrieval        (core doc §9)
        ↓
  deterministic final packet
        ↓
  PacketReductionPlanV1               (core doc §11)
```

---

# 3. The three phases

## 3.1 Phase A — request / evidence-requirement assistance

Runs before provider acquisition, which makes it the one phase with no candidates and therefore no
hard gates in front of it. It is constrained by **content minimalism**, not by a policy object.

> **Phase A is content-minimal by construction.**

Permitted input, in full:

```text
task text
protected user anchors
task mode / requested output class
```

Never:

```text
repository name, path or workspace identity
file paths
memory, Ledger, Cortex or Blueprint content
```

Those become available only after normal Membrane authorization and acquisition. A future expanded
Phase A projection may be defined for local inference or explicitly permitted environments; it is
not defined here.

A useful consequence: a small local model can serve Phase A cheaply, because it needs no repository
context at all.

Allowed output: proposed evidence requirements, query reformulations, synonyms and aliases, likely
provider families, ambiguity flags, and explicit questions the packet must answer.

```text
User: "Find why auth broke after the token refactor."

R1  current auth failure evidence
R2  token-refactor diff / decision
R3  current auth architecture / ownership
R4  relevant failing test or runtime error
```

The advisor grants access to nothing and decides no source's authority.

## 3.2 Phase B — candidate semantic assessment

Runs only after hard eligibility, source identity, authority and freshness are established.

Allowed output: candidate-to-requirement mappings, semantic relevance classes, redundancy groups,
contradiction proposals, task-critical detail markers, include/drop priority **within equivalent
policy classes**, protected exact spans or identifiers that should survive reduction, and
insufficiency diagnosis.

The model cannot revive an ineligible, stale or denied candidate.

## 3.3 Phase C — corrective retrieval assistance

Runs only after Membrane's deterministic sufficiency check reports insufficiency.

Allowed output: one bounded query reformulation, one alternate provider or lane proposal, one
deeper source-bound expansion proposal, or an explicit unknown / no-recovery recommendation.

The corrective *mechanism* is core doc §9 and is deterministic. Phase C supplies one input to it
and never executes retrieval itself. No open-ended self-RAG loop.

```text
initial semantic call:     0 or 1
corrective semantic call:  0 or 1
maximum per context trace: 2
```

A deployment may set a stricter ceiling.

---

# 4. What the advisor may and may not decide

May propose:

```text
task interpretation          evidence requirements       query wording
semantic relevance           requirement coverage        candidate redundancy
candidate contradiction      task-critical details       query-aware reduction focus
insufficiency diagnosis      bounded corrective actions
```

Prefer ordinal or structural outputs — `critical | supporting | marginal | irrelevant`, or pairwise
and group relationships — over fake probability precision. A model emitting `0.93 relevant` is not
evidence that the number is calibrated.

Must never decide or mutate — enforced in code, not in prompt text:

```text
authorization              ScopeGrant contents        installation identity
source identity            producer identity          authority class
trust class                sensitivity / DLP policy   freshness class
current-source validity    permission grants          policy exceptions
final token/byte ceiling   final packet admission     final delivery lane
durable Cortex admission   deletion / forgetting      lifecycle promotion
user Taste authority       tool execution             filesystem/process/network effects
provider credentials       host model routing
```

It also must not: manufacture evidence or source refs; introduce a candidate id absent from the
allowed input set; turn inferred content into observed evidence; strengthen scope beyond the
request's grant; treat prompt-like text inside a source as control instructions; write directly to
Cortex, Ledger or Adapt; call tools or network endpoints; or trigger further model calls.

---

# 5. Deterministic spine

```text
ContextRequest
   ↓
validate request / installation / grant
   ↓
protected anchors
   ↓
[PHASE A — content-minimal, §3.1]
   ↓
provider acquisition
   ↓
normalize CandidateV1 / evidence refs
   ↓
HARD GATES
   authorization · repository/scope · sensitivity
   source validity · authority · freshness · lifecycle visibility
   ↓
eligible candidate set
   ↓
[PHASE B — shadow by default, §3.2]
   ↓
validate advisor result
   ↓
deterministic fusion / dedupe
   ↓
deterministic sufficiency
   ↓
[PHASE C + bounded corrective pass, §3.3 + core §9]
   ↓
one global attention budget
   ↓
deterministic packet admission
   ↓
Native | Rendered | ResolverBacked | MetadataOnly
   ↓
publication revalidation
   ↓
ContextPacket + ContextReceipt + PacketReductionPlanV1
```

The advisor never sits before authorization or safety gates when candidate content is involved.

---

# 6. Replay: a recorded nondeterministic boundary

Membrane's deterministic path is reproducible. Once active advice affects the packet, recomputing
the same request can produce a different packet. That must be handled explicitly or the golden-fixture
testing discipline silently degrades.

The model invocation is a **recorded nondeterministic boundary**, not a cached pure function.

```text
ContextSemanticRequestV1
        ↓
      model
        ↓
ContextSemanticAdviceV1
        ↓
    validation
        ↓
ValidatedSemanticAdvice        ← persisted as the replay artifact
```

The replay artifact is identified by the whole semantic execution identity:

```text
semantic_request_digest
prompt_version
output_schema_version
model_profile
actual_model
source / workspace generations represented in the request
```

Two execution modes:

```text
LIVE     the model is called; validated advice is recorded
REPLAY   recorded validated advice is injected; the model is NOT called
```

Golden tests run in REPLAY against pinned advice fixtures. This gives deterministic replay of an
originally nondeterministic execution — the same pattern used for external APIs, clocks and tool
results.

A cache may exist as an optimisation, but cacheability is explicit and keyed on the full identity
above. Keying on `input_digest` alone is unsafe: the same nominal input may be inappropriate to
reuse after a prompt or model change, and context advice is particularly sensitive to freshness and
generation.

---

# 7. Internal typed contracts

Do not overload the existing public `ContextPacketV1` or `ContextReceiptV1` with unversioned fields.

```text
ContextSemanticRequestV1
  request_id, context_trace_id, phase
  task
  protected_anchors[]
  current_requirements[]
  eligible_candidate_digest?
  candidates[]?
  allowed_candidate_ids[]?
  budget, deadline
  prompt_version, output_schema_version

SemanticCandidateV1
  candidate_id, provider_id, semantic_kind, source_ref
  requirement_hints[]
  authority_class          # descriptive; the model cannot change it
  freshness_class          # descriptive; the model cannot change it
  provider_rank?
  estimated_tokens
  bounded_content_view?
  protected_exact_refs[]

ContextSemanticAdviceV1
  request_id
  requirement_proposals[]   query_proposals[]
  candidate_assessments[]   redundancy_groups[]
  contradiction_proposals[] protected_detail_proposals[]
  insufficiency             corrective_actions[]
  no_op

SemanticAdviceValidationV1
  valid
  accepted_items[]  rejected_items[]  rejection_reasons[]
  input_digest      output_digest
```

Every id in model output is resolved against the original allowed set. **Unknown ids are rejected,
not guessed.**

## 7.1 Final wire contract

The visible native tray owns the headless Membrane daemon and the host is its client, so the
challenge/resume flow in §8 is cross-process. The on-demand dashboard never owns this runtime.

These contracts require: an explicit version, a canonical
Rust type, a JSON schema, canonical serialization, golden fixtures, a digest, a compatibility
policy, an unknown-field policy, and challenge replay rules.

Do not introduce an in-process host inference handle to postpone this; that would contradict the
runtime boundary.

---

# 8. Challenge / resume, not reverse RPC

Membrane must not hold host provider credentials, and must not reach back into the host over an ad
hoc callback.

```text
host
  | context()
  v
Membrane daemon
  |
  +-- deterministic path sufficient
  |     -> ContextPacket + ContextReceipt
  |
  +-- semantic assistance justified
        -> ContextInferenceChallengeV1
                 |
                 v
           host executes model  (capability H11)
                 |
                 v
        ContextInferenceResultV1
                 |
                 v
           resume same trace
                 |
                 v
        Membrane validates result
                 |
                 v
        ContextPacket + ContextReceipt
```

A client library may hide this loop so the logical operation stays `membrane_context(...)`.

Outstanding challenges are trace-bound, deadline-bound, single-use, generation-bound, cancelable and
receipt-bearing. Membrane rejects a result whose trace or request identity does not match the
outstanding challenge.

Generic clients with no inference capability are never required to implement this. They take the
deterministic path.

## 8.1 Recursion guard — mandatory

A model call made *for Membrane context planning* must not itself trigger a Membrane
context-planning request.

```text
inference_purpose      = membrane_context_semantic_advisor
context_plane_reentry  = forbidden
```

The host must execute these through a dedicated inference path, not through its normal user-turn
context assembly. Without this the loop is unbounded.

---

# 9. Host inference capability (H11)

Proposes an experimental extension to current host capability atoms.

```text
H11  bounded structured inference execution
     absent → advisor disabled; deterministic path unaffected
```

```text
ContextInferenceExecutor
    execute(ContextSemanticRequestV1) -> ContextSemanticModelResultV1 + execution_receipt
```

| Membrane owns | Host owns |
|---|---|
| whether the call is justified | provider authentication and credentials |
| the semantic task | exact provider/model resolution |
| prompt and template version | provider API transport and formatting |
| structured input and allowed candidate set | provider health |
| output schema | cost enforcement at the host boundary |
| maximum input/output budget | returning output plus an execution receipt |
| validation of the result | |
| whether any proposal influences the packet | |

## 9.1 Model profile, not exact vendor model

Membrane requests an abstract capability:

```text
semantic_context_fast | semantic_context_strong | local_only_semantic_context
```

with constraints such as `structured_output = required`, `max_latency_ms`, `max_cost`,
`remote_egress = allowed | denied`.

The host resolves that to an exact executable model. Membrane does not become a second model
router. If the host cannot satisfy a declared constraint it returns typed incompatibility; it must
never substitute a model outside the constraint.

## 9.2 The host may deny on budget

Per-trace budgets prevent one bad request from exploding. They do not prevent a semantic call on
every turn, nor contention with other host inference work happening in the same turn.

Membrane requests; the host may refuse:

```text
denied: turn_inference_budget_exhausted
```

Membrane treats a denial exactly like unavailability: deterministic path, recorded degradation. The
host owns hierarchical budgeting across user, session and turn, and owns arbitration between the
advisor and any other inference it runs concurrently. Membrane neither sees nor manages those.

---

# 10. Activation gate and budget

Do not put an LLM on every context request.

Triggers (subject to the §2 default posture):

```text
deterministic sufficiency failure        ← the Phase C trigger, active by default
ambiguous task interpretation            cross-provider contradiction
low retrieval score separation           many semantically similar candidates
tight packet budget with high volume     high-value task mode
explicit semantic-assist request
```

Non-triggers:

```text
exact anchor lookup                      single authoritative match
simple known-id source read              small candidate set, clear ranking
model budget exhausted                   deadline too short
sensitive policy forbids remote inference
```

```text
ContextInferenceBudgetV1
  max_calls  max_input_tokens  max_output_tokens
  max_wall_ms  max_cost  allowed_model_profile
```

Budget exhaustion returns to deterministic planning. It never relaxes a hard policy.

---

# 11. Influence limits

Ordering is preserved; the advisor does not replace it with a score.

```text
hard eligibility
  > authority class
  > freshness / current-source class
  > provider-local and fused deterministic relevance
  > bounded semantic advice WITHIN equivalent policy classes
  > diversity / redundancy handling
  > utility-per-token admission
```

Advice may break ties, improve requirement coverage, penalise redundancy, identify task-critical
evidence, and request a bounded corrective pass. It cannot make stale, weak or denied evidence
outrank current authoritative evidence.

## 11.1 Protection is stronger than ranking, and is bounded too

The advisor may propose spans that must survive reduction:

```text
protected:
  exact error: E_AUTH_4017
  function:    refresh_access_token
  file:        src/auth/session.rs
  decision id: ADR-042
```

Pinning content into a packet is a stronger power than reordering it, so protection proposals are
budget-bound and overridable by deterministic admission. A protection proposal that would breach the
attention budget is rejected, not honoured at the cost of dropping admitted evidence.

Push remains a faithful transformation subsystem. The advisor never rewrites a source into an
untraceable summary. Lossy output retains parent and evidence refs and exact resolver paths. If
protected material disappears, deterministic verification restores exact spans or selects a less
aggressive representation (core doc §11).

---

# 12. Host integration

The canonical host↔Membrane ownership and runtime boundary is defined by
`docs/current/architecture/integrations/coderight.md`. It is not
reproduced here.

This feature adds exactly one requirement to that seam:

> Membrane may issue a bounded semantic-inference challenge; the host may execute it through host
> inference (H11); Membrane remains the sole validator and the sole context publisher.

---

# 13. Privacy and prompt-injection handling

Model input is a derived view, never an unrestricted dump.

Before inference: hard authorization and sensitivity gates run; candidate content is bounded; exact
source identity stays outside model control; source text is labelled as untrusted evidence; and
control-looking strings inside source content are never treated as Membrane instructions.

The advisor has no tools, no filesystem or network capability, no provider credentials, and cannot
read additional sources. If it proposes a deeper read, deterministic Membrane code decides whether
that operation is allowed and bounded.

---

# 14. Receipts

Every context trace states whether semantic assistance was used.

```text
SemanticAssistanceReceiptV1
  mode                 off | shadow | active | corrective
  execution_mode       live | replay
  trigger_reason
  request_id
  prompt_version
  model_profile_requested
  execution_receipt_ref?
  replay_artifact_ref?
  input_digest  output_digest?
  validation_status
  accepted_proposal_count  rejected_proposal_count
  fallback_reason?
  latency_ms  input_tokens?  output_tokens?  cost?
```

No raw sensitive candidate payloads in the receipt.

The `ContextReceipt` remains authoritative for admitted, omitted, denied, stale, unavailable,
timeout, budget drop and delivery lane. The semantic receipt explains an advisory step; it is not a
second context authority.

---

# 15. Rollout and evaluation

Shadow first. The deterministic packet is the canonical result; the advisor records what it would
have included, dropped, proposed and corrected, and does not change the delivered packet. Activate
per task class only after qualification. Keep a permanent kill switch.

Benchmark arms:

```text
A  deterministic Membrane baseline
B  A + local/rule query expansion
C  A + advisor in shadow
D  A + Phase C active on sufficiency failure       ← the default posture
E  D + Phase B active
```

Measure: task success, required-evidence recall, irrelevant/contradictory/duplicate context rates,
packet tokens, root-model tokens, context preparation latency p50/p95, semantic call latency, cost,
call rate, corrective rate, malformed output rate, unknown-candidate-id hallucination rate,
deterministic fallback rate, user correction attributable to missing context, manual search after
delivery.

Hard safety metrics, all of which must be zero:

```text
scope violations              authority promotions by model
denied candidate resurrection credential exposure
unbounded recursion           durable writes from advisor
```

Do not make active assistance the default because it looks better on examples.

---

# 16. Final component set

Final shape contains versioned wire contracts and fixtures, LIVE/REPLAY artifacts, advisor policy,
input projection, output validation, deterministic fallback, trace-bound challenge/resume,
Phase C integration with core corrective retrieval, receipts, diagnostics, frozen shadow
evaluation, evidence-gated Phase B activation, and optional content-minimal Phase A.

---

# 17. Non-goals

No general Membrane agent loop, chain-of-thought orchestration, autonomous tools, arbitrary model
tool use, second model router, provider credentials inside context packets, LLM compressor on every
packet, LLM lifecycle decisions, automatic durable memory from model output, external vector or
graph database, new process plane, or host-specific types in Membrane core.

---

# 18. Definition of done

1. deterministic Membrane remains fully functional with assistance disabled;
2. assistance is optional and budgeted;
3. model output cannot change scope, authority, freshness or security;
4. every model-returned id is validated against the allowed input set;
5. malformed, timeout, unavailable and host-denied inference all fall back to deterministic planning;
6. no context-planning inference can recursively invoke context assembly;
7. every active semantic call is receipt-linked and replayable;
8. a golden test suite runs entirely in REPLAY mode;
9. active mode demonstrably improves held-out evidence/task metrics;
10. a host can supply inference without giving Membrane provider credentials;
11. clients without inference support continue to receive valid deterministic packets;
12. final packet policy still has exactly one authority: Membrane.

---

# 19. Open questions

Deliberately unresolved and benchmark-gated. Everything else here is a decision.

1. When Phase B graduates from shadow to active, and for which task classes.
2. Which model profile Phase C actually needs.
3. Whether Phase A ever needs more than task text plus anchors.
4. How aggressive a protection proposal may be before it competes with admission (§11.1).
