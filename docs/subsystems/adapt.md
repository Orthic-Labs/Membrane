# Adapt — Learning Loop

**Status:** derived subsystem reference · non-normative  
**Canonical name:** Adapt  
**Parent system:** Membrane  
**Authority:** Membrane's canonical doctrine governs context policy, authority, influence, and Blueprint admission. This file summarizes Adapt's subsystem boundary.

## Purpose

Answer one question:

> **What should we have learned?**

Adapt turns experience — transcripts, observable events, and outcome evidence — into governed proposals for durable knowledge.

Experience becomes durable knowledge only through Blueprint admission.

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
- Emits proposals into Blueprint admission.
- Model-assisted extraction may propose; deterministic policy decides durable effects.
- Adapt cannot upgrade influence/authority class.

Physical repository placement is independent of semantic ownership. The Blueprint/Blueprint monorepo migration plan does not decide whether Adapt's code is physically subtree-merged.

## Invariants

1. Mining/proposal generation never bypasses Blueprint admission.
2. User-origin evidence is required before a preference can gain user-authoritative status.
3. Observable failure detection states its evidence limits.
4. Apply/adoption operations are auditable and reversible where supported.
5. Adapt is a Membrane subsystem even if its implementation remains physically separate.

## Definition of Done

- [ ] Taste proposals remain evidence-backed and reviewable.
- [ ] Insights/failure findings can become Blueprint proposals through admission, never direct writes.
- [ ] Learned gotchas/preferences surface only when Membrane policy deems them relevant and eligible.
- [ ] Doctor/qualification covers the Adapt → Blueprint proposal seam.
