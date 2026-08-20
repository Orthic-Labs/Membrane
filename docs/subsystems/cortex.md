# Cortex — Durable Knowledge

**Status:** derived subsystem reference · non-normative  
**Canonical name:** Cortex  
**Parent system:** Membrane  
**Implementation path:** `engine/crates/cortex*`
**Authority:** `MEMBRANE_CANONICAL_ARCHITECTURE_AND_IMPLEMENTATION_DOCTRINE.md`.

## Purpose

Answer one question:

> **What do we durably know?**

Cortex preserves governed long-lived knowledge: decisions, preferences, gotchas, procedures, observations, temporal facts, and bounded episodic/session knowledge, each with evidence, scope, authority, validity, lifecycle, derivation, and supersession.

## Owns

- The authored durable-knowledge SQLite store.
- The canonical durable `KnowledgeRecord` model and rebuildable retrieval projections.
- Admission before persistence:
  `schema → scope → producer → sensitivity/DLP → epistemic class → identity → novelty → duplicate/near-duplicate → conflict/supersession → durability/utility → disposition → receipt`.
- Write dispositions:
  `retain`, `update_metadata_only`, `supersede`, `merge`, `conflict`, `no_op`, `proposal`, `quarantine`, `reject`, `expire`, `forget`, `restore`.
- Exact, lexical/FTS5-BM25, vector, temporal, working, and bounded depth-1 relation retrieval over its own knowledge.
- Conflict and supersession semantics.
- Observed/valid/expiry/recorded-time distinctions where behavior requires them.
- Deterministic, versioned, reversible lifecycle policy.
- Dream/curation as reversible proposals; derived output never self-promotes to truth.
- Bounded session continuity packets.
- Erasure across canonical records and all projections/artifacts it owns.

## Does not own

- final context/attention policy — Membrane planner;
- repository facts, code graph, source identity, re-anchoring — Blueprint;
- document/section navigation index — Guide;
- compression/reduction mechanics — Push;
- transcript/event learning — Adapt;
- OS/process lifecycle — Hub/integration layer.

## Public seam

Cortex produces typed durable-knowledge candidates through the Membrane provider seam.

Knowledge writes are governed operations such as proposal/temporal-fact/feedback flows routed through Membrane policy and Cortex admission. There is no raw public `membrane_cortex` memory CRUD surface.

Adapt may submit proposals. It cannot write Cortex truth directly.

## Canonical runtime namespace

```text
Rust crates:
  cortex
  cortex-core
  cortex-store
  cortex-format

binaries:
  cortex
  membrane supervisor-child

environment:
  MEMBRANE_PORT
  MEMBRANE_API_TOKEN
  MEMBRANE_API_TOKEN_FILE

store:
  cortex-engine.db
```

## Invariants

1. Only the canonical Cortex store owner opens the durable store.
2. Admission precedes durable truth; `no_op` is success.
3. Feedback may affect retrieval pressure, never authority.
4. Conflict is not overwrite.
5. Temporal supersession is not simultaneous contradiction.
6. Derived summaries never self-promote to truth.
7. Repeated selection never makes a memory immortal.
8. Erasure must remove payload from every Cortex-owned projection/cache/artifact path.
9. Guide index tables and Push artifact state do not live in the Cortex durable store.
10. Repository truth never enters Cortex; Blueprint owns it.

## Definition of Done

- [ ] Every durable item can explain origin, evidence, scope, authority, validity, supersession, derivation, lifecycle state, and retention reason.
- [ ] Exact duplicates no-op; conflicts preserve both evidence sets.
- [ ] FTS5/BM25 production projection exists and retrieval works with embeddings disabled.
- [ ] Lifecycle is deterministic/versioned/reversible where possible.
- [ ] Dream is undoable and never creates authority.
- [ ] Backup/restore preserves logical identities, lineage, and recall equivalence.
- [ ] Canonical runtime uses `cortex`, `membrane supervisor-child`, `MEMBRANE_*`, and `cortex-engine.db` exclusively.
