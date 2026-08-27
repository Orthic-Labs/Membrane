# Membrane Cross-Subsystem Improvement Contracts and Evidence Gates

**Date:** 2026-08-25
**Status:** canonical cross-subsystem contracts; companion to canonical Membrane doctrine
**Scope:** contracts spanning Pull, Push, Cortex, Blueprint, Ledger, Adapt, tray/daemon/Hub, and CodeRight integration
**Does not supersede:** subsystem-specific semantic canons

## Executive decision

This document consolidates cross-subsystem contracts that do not belong to one subsystem owner.

The main decisions are:

1. **Ledger** is the canonical document-navigation subsystem; Guide survives only at explicit compatibility/history boundaries.
2. Add one cross-cutting Definition-of-Done invariant:
   > **A capability is not landed until the production path executes it and frozen acceptance evidence shows it meets or improves the baseline it replaces.**
3. Keep shared integrity semantics aligned across subsystems without creating an unowned generic "contracts" layer.
4. Pull uses deterministic RRF as its standing fusion baseline and owns bounded corrective retrieval.
5. Push may perform query-aware reduction and preserves planner-selected ordering without becoming a second planner.
6. Make Cortex semantic lifecycle/curation the shared durable mechanism that satisfies Adapt's reevaluation requirements.
7. Keep Blueprint build-time intelligence and deterministic graph traversal under Blueprint ownership.
8. Use Adapt as the behavioral-learning loop over subsystem outcomes, not as the implementation owner of other subsystems.
9. Make CodeRight emit the structured outcome evidence that lets these mechanisms be evaluated in real work.


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

# 1. Cross-cutting failure pattern

A recurring implementation failure in Membrane has been:

```text
mechanism exists
    ↓
helper/unit tests pass
    ↓
docs say capability landed
```

without proving:

```text
production path reaches mechanism
    +
representative outcome evidence exists
    +
mechanism beats or qualifies against baseline
```

Examples include the class of defects where:

- an index exists but shipped recall never queries it;
- a native path exists beside a still-selectable legacy path;
- detectors exist without measured real-world precision;
- a persistence module exists but primary CLI semantics differ;
- a performance test is ignored;
- a receipt/seal exists but behavior-bearing fields sit outside its binding.

These are not isolated bugs. They require a doctrine-level invariant.

---

# 2. Canonical "no unexercised capability" invariant

## 2.1 Completion evidence stack

Every claimed capability must supply the evidence levels relevant to its scope:

### Source proof

The implementation exists at the canonical owner.

### Integration proof

The real production entrypoint invokes that implementation.

### Behavior proof

The production path exercises the intended semantic behavior rather than a trivial/no-op path.

### Measured proof

When replacing, optimizing, ranking, compressing, routing, or learning:

- baseline is frozen before comparison;
- metric definitions are frozen;
- candidate configuration is frozen before held-out evaluation;
- result meets the declared threshold or non-regression requirement.

### Installed proof

When release/runtime/package behavior matters, the exact installed candidate reaches the same path.

### Cutover proof

When replacing a legacy implementation:

- old path is no longer production-selectable;
- fallback/rollback is explicit and bounded;
- final removal has a deletion/exclusion receipt.

## 2.2 Tests that do not count

The following do not prove shipment by themselves:

- "module imports";
- table/index exists;
- unit test calls helper directly;
- output is unchanged with mechanism enabled/disabled;
- benchmark calls an alternate code path;
- ignored performance test;
- synthetic-only result used to claim production quality;
- docs or comments naming the capability;
- native implementation coexisting with a selectable interpreter fallback.

## 2.3 Rollout states

For replacements use explicit states such as:

```text
off
shadow
qualified
active
rollback
retired
```

State transitions require receipts where the subsystem already uses governed activation.

---

# 3. Shared integrity doctrine without a generic-owner mistake

Claude correctly identified that source/digest binding now appears in Ledger, Adapt, Cortex, Push, and other Membrane surfaces.

Do **not** respond by creating an unowned `common-contracts` dumping ground.

Instead define a doctrine-level integrity profile and shared test vectors.

## 3.1 Integrity profile

Any subsystem binding semantics to bytes/state should record, where applicable:

```text
subject identity
source/producer identity
revision/generation
canonicalization version
algorithm
digest
source range?
parent/previous digest?
producer implementation/version
receipt identity
```

The semantic meaning of each receipt remains subsystem-owned.

## 3.2 Shared test vectors

Maintain small language-neutral fixtures for:

- canonical SHA-256;
- exact source-span hashing;
- canonical JSON hashing;
- generation/revision mismatch;
- tamper detection;
- empty/invalid digest refusal.

Each owner runs the vectors in its own tests.

This gives alignment without one shared runtime owner.

---

# 4. Ledger identity

The canonical six axes become:

| Axis | Responsibility |
|---|---|
| Pull | retrieve/fuse/admit task-relevant evidence |
| Push | faithful reversible reduction |
| Cortex | governed durable knowledge |
| Blueprint | repository truth and graph/evidence generations |
| **Ledger** | registered document corpus indexing/navigation/exact resolution |
| Adapt | governed behavioral learning |

Current product code and documentation use Ledger. Historical `Guide`/`Spine` remains only in
explicit compatibility, migration, or history boundaries.

---

# 5. Pull improvements

## 5.1 Current ownership remains correct

Pull owns:

- bounded provider acquisition;
- hard eligibility;
- freshness/authority;
- dedupe;
- cross-provider fusion;
- sufficiency;
- attention headroom;
- publication and receipts.

No provider may become a second final planner.

## 5.2 Heterogeneous fusion

Ledger lexical/BM25 ranks, Cortex vector/lexical ranks, Blueprint structural confidence, exact anchors, and other lanes are not naturally score-calibrated.

Reciprocal Rank Fusion (RRF) is the standing deterministic baseline because it combines rank positions without assuming comparable score scales. An active production path that uses another policy must identify that policy and remain a measured migration/control path until RRF cutover qualification passes.

Pull receipts should record:

- provider/lane rank;
- fusion strategy/version;
- fused rank;
- duplicate/fusion decisions;
- budget drops.

## 5.3 Corrective retrieval after insufficiency

CRAG and Self-RAG are useful research references for one principle:

> retrieval should be evaluated, and a poor/insufficient first retrieval can trigger a corrective action.

Membrane SHOULD NOT copy their model architecture.

Membrane planner owns typed sufficiency, omissions, and evidence requirements. Corrective retrieval consumes those deterministic structures.

Required flow:

```text
initial acquisition
   ↓
hard eligibility
   ↓
fusion
   ↓
sufficiency/coverage check
   ├─ sufficient -> publish
   └─ insufficient
        ↓
   bounded corrective action
        ├─ query expansion/reformulation proposal
        ├─ alternate provider/lane
        ├─ deeper source-bound expansion
        └─ explicit unknown
```

Rules:

- never exceed deadline/maxCost silently;
- record why re-query happened;
- cap stages and marginal work;
- model query reformulation, if used, is proposal-only and bounded;
- no infinite self-RAG loop.

## 5.4 Retrieval-outcome telemetry

Emit structured observations for CodeRight/Adapt:

- requested evidence dimensions;
- provider/lane coverage;
- sufficiency result;
- corrective stage count;
- manual search after packet;
- user correction attributable to missing/irrelevant context;
- final task/evaluator result.

This makes retrieval quality learnable.

---

# 6. Push improvements

## 6.1 Keep Push's boundary

Push executes planner-selected faithful transformations.

It does not:

- rank evidence;
- decide attention;
- invent missing evidence;
- become a summarization planner.

## 6.2 Query-aware reduction contract

LongLLMLingua is relevant for the high-level finding that compression can benefit from query/task awareness.

Do not automatically add an LLM compressor to Push.

Any query-aware reduction mode uses planner-supplied task/evidence metadata and remains a faithful transform, not a second attention planner.

Required behavior:

- protect query/task entities;
- protect exact identifiers/errors/tests/constraints;
- score or retain structurally relevant blocks relative to the task;
- preserve exact recovery handles;
- fall back to less reduction on uncertainty.

Qualification compares:

```text
raw control
structural query-agnostic Push
query-aware Push
```

at matched attention budgets.

Metrics:

- task correctness;
- required-evidence retention;
- protected-span integrity;
- token reduction;
- latency;
- resolver restores;
- user corrections.

## 6.3 Ordering policy contract

"Lost in the Middle" shows long-context models can use evidence differently depending on position.

This does not make Push the ranking owner.

Pull/final renderer owns explicit, versioned ordering policies such as:

- highest-authority/required evidence early;
- critical constraints early plus final recap/reference late;
- grouped evidence by dimension;
- baseline fused order.

Push must preserve selected order unless the planner explicitly chooses a representation/order policy.

## 6.4 Reduction effectiveness events

CodeRight should report:

- Push opportunity;
- transform selected;
- bytes/tokens before/after;
- protected-span status;
- restore/refetch;
- task outcome.

Adapt may detect recurring waste or reduction failures but cannot rewrite Push policy directly.

---

# 7. Cortex improvements

## 7.1 Cortex remains the durable owner

Cortex stores governed durable knowledge, including:

- memories;
- temporal facts;
- decisions;
- procedures;
- observations;
- Taste preferences;
- Insight issues;
- other admitted long-lived knowledge.

Semantic type remains explicit.

## 7.2 Semantic lifecycle reevaluation

Adapt canon correctly rejects blind time decay.

Cortex curation should own the generic durable reevaluation mechanism.

Reevaluation triggers may include:

- stronger contradictory knowledge;
- policy change;
- source invalidation;
- Ledger source relocation/disappearance;
- repository/Blueprint revision change where relevant;
- model/client/tool version change;
- repeated failed retrieval/usefulness outcome;
- Adapt retirement suggestion;
- user edit/delete;
- explicit review.

Time may schedule review. It must not silently rewrite authority.

## 7.3 A-MEM/Evo-memory lessons

Research on evolving memory systems is relevant to:

- link/relationship maintenance;
- semantic consolidation;
- mutation history;
- context-aware reorganization.

Absorb only through Cortex's governed lifecycle/curation rules.

Do not let agent-generated consolidation self-promote to authority.

## 7.4 Ledger source references

Durable knowledge may cite Ledger document/node identities.

At retrieval or validation:

- resolve current source through Ledger;
- honor relocation/stale/missing;
- do not copy the whole source corpus into Cortex merely for navigation.

---

# 8. Blueprint improvements

## 8.1 Build-time intelligence, deterministic query-time traversal

Blueprint is the correct owner for graph-enhanced repository evidence.

The useful principle from graph/repository research is:

> spend expensive structural work at build/update time; make query-time graph traversal deterministic, bounded, and provenance-rich.

Blueprint should continue to own:

- code parsing;
- symbols/types/references/imports/calls;
- source identity/generations;
- repository graph;
- code↔doc verification;
- relocation/drift.

## 8.2 Query-time contract

Expose source-bound graph evidence with:

- generation pin;
- node/edge identity;
- provenance;
- confidence/evidence class;
- caps;
- typed incompleteness.

Pull decides whether/how much graph evidence enters attention.

## 8.3 No duplicate relation graph

Ledger's document links do not become the Blueprint code graph.

Cortex knowledge relations do not become the Blueprint graph.

Each graph/projection answers a distinct ownership question.

---

# 9. Ledger improvements

Detailed implementation is governed by:

`LEDGER-MARKDOWN-INDEXING-AND-DOCUMENT-NAVIGATION-CANON.md`

Cross-system summary:

- full Guide → Ledger rename;
- fix ASCII-only query correctness immediately;
- source-positioned Markdown AST;
- Ledger-local FTS5;
- query normalization/identifier splitting;
- link/alias/relocation;
- train/dev/held-out evaluation;
- paired confidence intervals or justified sample size;
- title-chain prefix as measured ablation;
- production-path FTS proof;
- session generated document as typed derived projection, not subsystem identity.

---

# 10. Adapt improvements

Detailed semantics are governed by the revised Adapt canon.

Cross-system responsibilities:

- consume CodeRight structured observations, not transcripts only;
- preserve transcript analysis for semantic/user-language signals;
- real held-out detector/evaluator corpus;
- Braintrust-style emergent failure discovery as proposal-only;
- Langfuse-style human review queue;
- Phoenix-style versioned evaluator/regression workflow, executed by CodeRight;
- CHIRON/HORKOS lesson: deterministic capture and exact completion/artifact receipts where possible;
- learn from Pull/Ledger/Push outcomes without becoming their policy owner.

---

# 11. Hub and diagnostics

Hub is where operator-visible subsystem capability truth should converge.

After Guide → Ledger:

- subsystem status must expose `Ledger`;
- no current UI says Guide;
- readiness should distinguish:
  - implementation present;
  - production path active;
  - degraded fallback;
  - not configured;
  - qualification incomplete where relevant.

For measured features, diagnostics SHOULD expose:

- strategy/mode/version;
- activation receipt;
- last qualification corpus/result;
- fallback/rollback state.

A healthy process is not evidence that an optional optimization is active.

---

# 12. CodeRight as the outcome producer

CodeRight is the harness and therefore the richest source of operational outcome evidence.

It should emit typed facts about:

- model/route;
- tool calls/outcomes;
- approvals;
- edits/artifacts;
- verification;
- completion;
- tokens/latency;
- agents/subagents;
- user actions;
- Membrane retrieval/context receipts;
- Push reductions/restores;
- evaluator scores;
- task/goal outcome.

Raw high-volume trace/eval storage remains CodeRight-owned.

Selected durable semantic knowledge is admitted to Cortex.

Document-shaped virtual/handoff projections may enter Ledger through typed virtual-source contracts.

Adapt consumes the behaviorally relevant subset.

---

# 13. Cross-subsystem evaluation hygiene

Any subsystem optimizing a measurable behavior should use:

```text
mechanics fixtures
+
development/tuning corpus
+
frozen held-out corpus
+
production-path operational proof
```

## 13.1 No held-out tuning

Do not:

- select FTS tokenizer on final eval;
- tune BM25 weights on final eval;
- tune detector regexes on final detector test;
- tune routing thresholds on final benchmark;
- tune Push protected-span thresholds on final quality set.

## 13.2 Paired comparisons

Where the same task/query/case can be run through both baseline and candidate, prefer paired analysis.

Report uncertainty, not only point estimates.

## 13.3 Real vs synthetic

Synthetic fixtures prove semantics/mechanics.

Real representative corpora support product-quality claims.

Do not substitute one for the other.

---

# 14. Pending implementation ownership

This canonical document defines contracts and evidence gates, not rollout order. Non-experimental
Membrane implementation work lives in `docs/pending/MEMBRANE-PENDING-IMPLEMENTATION.md` and may be
scheduled only after its production-path audit establishes a verified gap. Host-owned work remains
in the host repository.

---

# 15. Rejected cross-system moves

Do not:

- turn Ledger into Cortex;
- turn Adapt into observability storage;
- turn CodeRight generic traces into Cortex durable knowledge by default;
- turn Push into a second planner;
- turn Blueprint graph into a universal ontology;
- create a generic shared-contract crate merely because several subsystems hash things;
- adopt RRF/query-aware compression/semantic clustering without measured qualification;
- claim a capability because its source files exist;
- maintain Guide and Ledger as permanent dual names.

---

# 16. Research references

External references supporting the recommendations:

- Cormack, Clarke & Büttcher — Reciprocal Rank Fusion (SIGIR 2009)
- Corrective Retrieval Augmented Generation — arXiv:2401.15884
- Self-RAG — arXiv:2310.11511 / ICLR 2024
- Lost in the Middle — arXiv:2307.03172
- LongLLMLingua — arXiv:2310.06839
- Braintrust Topics / eval improvement loop
- Langfuse evaluation / annotation queues / datasets / experiments
- Arize Phoenix datasets / evaluators / experiments

These motivate hypotheses and workflow patterns. Membrane's own frozen evaluation decides whether a mechanism is activated.

---

# 17. Final system statement

> **Membrane has six axes: Pull, Push, Cortex, Blueprint, Ledger, and Adapt. Each owns a distinct evidence problem. A mechanism is not complete because code exists; it is complete only when the production path executes it and qualified evidence shows it satisfies the baseline. CodeRight supplies real execution outcomes, Adapt learns behavioral patterns, Cortex governs durable knowledge, Ledger navigates registered documents, Blueprint owns repository truth, Pull decides attention, and Push faithfully reduces what Pull selected.**


---

# Appendix A — Runtime lifecycle test matrix (required)

This matrix is a hard gate for the Hub/Blueprint lifecycle. Every row is an
installed-artifact test, not a unit test.

| Scenario | Required result |
|---|---|
| Tray off, agent invokes Membrane | typed `membrane_unavailable { hub_inactive }`; zero Membrane processes spawned |
| Tray on | visible tray plus one headless child daemon; Hub dashboard optional and on demand |
| Tray quits | daemon disappears with tray; no orphan, no restart |
| Agent launches stdio MCP | only a stateless adapter process exists; it launches nothing |
| Tray off, explicit Blueprint query | bounded one-shot operation runs, reports generation + freshness, exits |
| Tray off, normal Membrane context request | typed unavailable; **no** Blueprint one-shot fallback is invoked |
| Tray on, one-shot Blueprint writer attempted | routes through the owner or fails `resident_owner_active`; never a second writer |
| Crashed tray or daemon | stale lease metadata cannot permanently lock the Blueprint store; OS lock semantics release it |
| Blueprint graph queried after source changed | freshness reports `changed_since_generation`, not silent success |
| OS login with tray disabled | no Membrane and no Blueprint resident processes exist |
