# Adapt — Learning Loop

**Status:** canonical subsystem doctrine · draft for adoption
**Code today:** separate repo `/Volumes/D/claude/adapt` (Python) — to be subtree-merged as `adapt/`
**Parent:** `docs/subsystems/SYSTEM.md`

## Purpose
Answer one question: **what should we have learned?** Turn experience (transcripts, observable events, outcomes) into *proposals* for durable knowledge. It is the only path by which experience becomes memory — and it goes through Cortex admission like everything else.

## Owns
- Transcript ingestion (Claude/Codex), canonicalization, provenance filter, origin quarantine.
- **Taste:** durable-preference mining → immutable review manifest (accepted/rejected/pending) → conformance gate → transactional apply via Cortex. Ships.
- **Insights:** 19 deterministic failure detectors over `TranscriptEventV1` → `FailureCardV1` (stable card id, evidence excerpt, honesty limit). Built; report-only today.
- Learning outcomes / run journal / rollback.

## Does not own
Any store · any direct write · authority (only user-origin evidence can create preference authority) · context policy.

## Public contract
- Reads: transcripts on disk; Cortex observable events via the scoped read paths (`TasteUserOnly` for Taste, `InsightsFullStream` for Insights). Never intercepts live tokens.
- Writes: **proposals only** into Cortex admission. Taste → `preference` records. Insights → `gotcha` records (`trigger, applies_to, avoid, prefer, severity, confidence, source, verification`).
- Manifests are the audit/undo unit.

## Invariants
1. Mining never writes; only an adjudicated manifest applies; apply is transactional with rollback.
2. Insights states its limit in-product: only observable failure signals are detectable.
3. Model-assisted extraction proposes; deterministic policy decides.
4. Adapt cannot upgrade influence class; memory stays descriptive by default.

## Definition of Done
- [ ] Repo merged as `adapt/`; CI runs its suite.
- [ ] Insights → gotcha proposal path exists and is admission-gated; gotchas surface in the Planner when a planned action matches `trigger`.
- [ ] README no longer says Insights is "deferred".
- [ ] Doctor covers Adapt ↔ Cortex wiring.
