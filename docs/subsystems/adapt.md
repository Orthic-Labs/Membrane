# Adapt — Learning Loop

**Status:** derived subsystem reference · non-normative  
**Canonical name:** Adapt  
**Parent system:** Membrane  
**Implementation path:** `adapt/`
**Authority:** Membrane's canonical doctrine governs context policy, authority, influence, and Cortex admission. This file summarizes Adapt's subsystem boundary.

## Purpose

Answer one question:

> **What should we have learned?**

Adapt turns experience — transcripts, observable events, and outcome evidence — into governed proposals for durable knowledge.

Experience becomes durable knowledge only through Cortex admission.

## Owns

- Transcript/event ingestion, canonicalization, provenance filtering, and origin quarantine.
- Taste-style durable-preference mining into reviewable proposals/manifests.
- Insights-style deterministic failure detection into evidence-backed failure/gotcha proposals.
- Learning-run audit/rollback metadata needed to explain what was proposed and why.

## Does not own

- any canonical truth store;
- direct durable writes;
- final authority decisions;
- final context/attention policy;
- repository truth;
- document indexing;
- reduction.

## Public seam

- Reads only explicitly permitted transcript/event sources.
- Emits proposals into Cortex admission.
- Model-assisted extraction may propose; deterministic policy decides durable effects.
- Adapt cannot upgrade influence/authority class.

Adapt lives in `adapt/` within Membrane. Physical co-location does not change semantic ownership.

## Invariants

1. Mining/proposal generation never bypasses Cortex admission.
2. User-origin evidence is required before a preference can gain user-authoritative status.
3. Observable failure detection states its evidence limits.
4. Apply/adoption operations are auditable and reversible where supported.
5. Adapt is a Membrane subsystem with one implementation under `adapt/`.

## Definition of Done

- [ ] Taste proposals remain evidence-backed and reviewable.
- [ ] Insights/failure findings can become Cortex proposals through admission, never direct writes.
- [ ] Learned gotchas/preferences surface only when Membrane policy deems them relevant and eligible.
- [ ] Doctor/qualification covers the Adapt → Cortex proposal seam.
