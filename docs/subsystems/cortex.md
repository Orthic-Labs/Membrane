# Cortex — Durable Knowledge Engine

**Status:** canonical subsystem doctrine · draft for adoption
**Was:** MemRight → Crypt. `crypt*` binaries/env remain the compatibility facade until the rename lands.
**Parent:** `docs/SYSTEM.md` · `MEMBRANE_CANONICAL_ARCHITECTURE_AND_IMPLEMENTATION_DOCTRINE.md` §8

## Purpose
Answer one question: **what do we durably know?** — decisions, preferences (taste), gotchas, procedures, observations, temporal facts, episodic session packets — each with evidence, scope, authority, validity, lifecycle, and supersession.

## Owns
- The memory SQLite store (authored, irreplaceable; `synchronous=FULL` for authored writes).
- `KnowledgeRecord` (logical id, content hash, scope, kind, evidence refs, epistemic state, authority, influence, sensitivity, temporal validity, lifecycle, supersession, derivation) + mutable signal sidecar.
- Admission-before-write: schema → scope → producer → DLP → epistemic class → identity → novelty → dup/near-dup → conflict/supersession → utility → disposition → receipt.
- Write dispositions: `retain update_metadata_only supersede merge conflict no_op proposal quarantine reject expire forget restore`.
- Conflict ≠ overwrite; temporal facts (`observed_at / valid_from / valid_until / expires_at / recorded_at`); lifecycle (deterministic, versioned, hysteretic, archive-first); Dream as reversible proposals.
- Retrieval channels over its own store: exact · FTS5/BM25 · vector · temporal · (bounded depth-1 relation) · working.
- Observable-event telemetry (content-free) and the scoped read paths Adapt uses (`TasteUserOnly`, `InsightsFullStream`).
- Erasure across canonical + FTS + vectors + relations + artifact refs; tombstones without payload.

## Does not own
Context policy/attention (Planner) · repository facts or re-anchoring (Blueprint) · markdown section index (Spine) · compression (Push) · transcript mining (Adapt) · host lifecycle (Hub).

## Public contract
- Typed candidates to the Planner via `providers/cortex` (ex-`crypt.py`) under `membrane-provider-sdk` conformance.
- `membrane_temporal_fact`, `membrane_knowledge_propose`, `membrane_feedback` (host tools routed through Planner/hosts).
- Proposal ingress from Adapt → admission. No direct writes.
- Read-only knowledge inspection model for Hub.

## Invariants
1. Only `engine/crates/cortex-store/**` opens the store.
2. Admission precedes durable truth; `no_op` is success.
3. Feedback moves retrieval pressure only; never authority.
4. Derived summaries never self-promote to truth; Dream ties go to quarantine.
5. A memory repeatedly selected does not become immortal.
6. Erased payload cannot reappear from any projection, cache, backup, or in-flight publish.
7. Spine and Push tables do **not** live in this store (migration item).

## Definition of Done
- [ ] Every durable item answers: what am I · where from · what supports me · whose scope · how authoritative · when observed · when valid · what superseded me · what I derived from · lifecycle state · why retained.
- [ ] Admission pipeline with typed dispositions and receipts.
- [ ] Conflicts preserve both evidence sets; supersession explicit.
- [ ] FTS5/BM25 production projection; retrieval works with embeddings disabled.
- [ ] Lifecycle deterministic/versioned/reversible; Dream undoable.
- [ ] Backup/restore drill preserves logical keys, lineage, recall equivalence.
- [ ] Store file renamed `cortex.db` with in-place migration; `crypt*` facade still resolves.
